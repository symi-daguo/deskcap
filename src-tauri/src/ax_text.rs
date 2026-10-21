//! Accessibility focused-text polling — primary path for Electron/IME when
//! CGEventTap does not deliver keyboard events.

use crate::engine::CaptureEngine;
use crate::ime::{char_lcp_len, has_cjk, strip_ime_composing};
use active_win_pos_rs::get_active_window;
use std::ffi::c_void;
use std::os::raw::c_longlong;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::cell::Cell;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type AXUIElementRef = *mut c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFArrayRef = *const c_void;

const AX_OK: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: CFAllocatorRef,
        cStr: *const i8,
        encoding: u32,
    ) -> CFStringRef;
    fn CFStringGetTypeID() -> CUlong;
    fn CFArrayGetTypeID() -> CUlong;
    fn CFGetTypeID(cf: CFTypeRef) -> CUlong;
    fn CFStringGetLength(theString: CFStringRef) -> c_longlong;
    fn CFStringGetCharacters(theString: CFStringRef, range: CFRange, buffer: *mut u16);
    fn CFArrayGetCount(theArray: CFArrayRef) -> c_longlong;
    fn CFArrayGetValueAtIndex(theArray: CFArrayRef, idx: c_longlong) -> *const c_void;
    fn CFRelease(cf: CFTypeRef);
}

type CUlong = u64;

#[repr(C)]
struct CFRange {
    location: c_longlong,
    length: c_longlong,
}

static KEY_VIA_AX: AtomicU64 = AtomicU64::new(0);
/// True when AX recently read an editable/chat composer (EventTap should defer text).
static AX_LIVE_EDITABLE: AtomicBool = AtomicBool::new(false);
static AX_LIVE_EDITABLE_MS: AtomicU64 = AtomicU64::new(0);
static FORCE_POLL: AtomicBool = AtomicBool::new(false);

pub fn ax_chars_captured() -> u64 {
    KEY_VIA_AX.load(Ordering::SeqCst)
}

pub fn request_force_poll() {
    FORCE_POLL.store(true, Ordering::SeqCst);
}

pub fn is_chat_app_name(name: &str) -> bool {
    crate::ime::is_chat_app_name(name)
}

/// EventTap should suppress Latin only while AX is actually reading a composer.
pub fn ax_live_editable() -> bool {
    if !AX_LIVE_EDITABLE.load(Ordering::SeqCst) {
        return false;
    }
    let ms = AX_LIVE_EDITABLE_MS.load(Ordering::SeqCst);
    if ms == 0 {
        return false;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    now.saturating_sub(ms) < 2500
}

fn mark_ax_live(ok: bool) {
    AX_LIVE_EDITABLE.store(ok, Ordering::SeqCst);
    if ok {
        if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
            AX_LIVE_EDITABLE_MS.store(d.as_millis() as u64, Ordering::SeqCst);
        }
    }
}

fn cfstr(s: &str) -> CFStringRef {
    let mut buf = s.as_bytes().to_vec();
    buf.push(0);
    unsafe { CFStringCreateWithCString(ptr::null(), buf.as_ptr() as *const i8, 0x0800_0100) }
}

fn cfstring_to_string(s: CFStringRef) -> Option<String> {
    if s.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(s as CFTypeRef) != CFStringGetTypeID() {
            return None;
        }
        let len = CFStringGetLength(s);
        if len <= 0 || len > 80_000 {
            return Some(String::new());
        }
        let mut utf16 = vec![0u16; len as usize];
        CFStringGetCharacters(
            s,
            CFRange {
                location: 0,
                length: len,
            },
            utf16.as_mut_ptr(),
        );
        Some(String::from_utf16_lossy(&utf16))
    }
}

fn copy_attr(el: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    let attr = cfstr(name);
    if attr.is_null() {
        return None;
    }
    let mut val: CFTypeRef = ptr::null();
    let err = unsafe { AXUIElementCopyAttributeValue(el, attr, &mut val) };
    unsafe { CFRelease(attr as CFTypeRef) };
    if err != AX_OK || val.is_null() {
        return None;
    }
    Some(val)
}

fn role_of(el: AXUIElementRef) -> String {
    copy_attr(el, "AXRole")
        .and_then(|r| {
            let s = cfstring_to_string(r as CFStringRef);
            unsafe { CFRelease(r) };
            s
        })
        .unwrap_or_default()
}

fn value_of(el: AXUIElementRef) -> Option<String> {
    let mut value = copy_attr(el, "AXValue").and_then(|v| {
        let s = cfstring_to_string(v as CFStringRef);
        unsafe { CFRelease(v) };
        s
    });
    if value.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
        if let Some(v) = copy_attr(el, "AXSelectedText") {
            value = cfstring_to_string(v as CFStringRef);
            unsafe { CFRelease(v) };
        }
    }
    value.filter(|s| !s.is_empty())
}

fn role_looks_editable(role: &str) -> bool {
    let role_l = role.to_lowercase();
    role_l.contains("text")
        || role_l.contains("edit")
        || role == "AXComboBox"
        || role == "AXSearchField"
        || role == "AXWebArea"
        || role == "AXTextField"
        || role == "AXTextArea"
}

fn role_ok_for_chat(role: &str) -> bool {
    role_looks_editable(role)
        || role == "AXGroup"
        || role == "AXScrollArea"
        || role == "AXSplitGroup"
        || role.is_empty()
}

fn accept_value(role: &str, value: &str, chat_mode: bool) -> bool {
    let n = value.chars().count();
    if n == 0 || n > 12_000 {
        return false;
    }
    // Skip Finder-ish / path noise
    if value.contains("/Users/") || value.contains("/Applications/") || value.contains(".app/") {
        return false;
    }
    if role_looks_editable(role) {
        return true;
    }
    if chat_mode && role_ok_for_chat(role) && n <= 800 {
        return true;
    }
    // Mis-tagged Electron composers: accept short chat-length strings.
    n <= 800
}

thread_local! {
    /// 本轮 AX 遍历的截止时刻。`None` = 不限（由入口显式开启）。
    /// 2026-10-20 — 微信的 AX 树很深，`recover_chinese_before_send` 实测
    /// 单次耗时 1.6–4.3 s，全部阻塞在 input_worker 上（日志 15 条
    /// `ERROR lag input_worker`）。AX 只是"尽力把拼音升级成汉字"的
    /// best-effort 优化，**不该为它卡住用户输入** —— 超预算就返回 None，
    /// 上层自然退回拼音 buffer。
    static AX_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// 开启一次遍历预算（毫秒）。要在每次顶层 AX 读取前调用。
pub fn begin_ax_budget(ms: u64) {
    AX_DEADLINE.with(|d| d.set(Some(Instant::now() + Duration::from_millis(ms))));
}

/// 预算是否已耗尽。遍历的每个节点都要查 —— 这是唯一能打断递归的办法
/// （AX 调用本身不可取消）。
fn ax_over_budget() -> bool {
    AX_DEADLINE.with(|d| match d.get() {
        Some(t) => Instant::now() >= t,
        None => false,
    })
}

fn text_from_element(el: AXUIElementRef, depth: usize, chat_mode: bool) -> Option<(String, String)> {
    // 深度上限 8 层 + 时间预算，两者任一触发即放弃这棵子树。
    if depth > 8 || ax_over_budget() {
        return None;
    }
    let max_depth = if chat_mode { 10 } else { 5 };
    if el.is_null() || depth > max_depth {
        return None;
    }
    let role = role_of(el);
    if let Some(value) = value_of(el) {
        if accept_value(&role, &value, chat_mode) {
            let n = value.chars().count();
            if role_looks_editable(&role) && (n <= 2000 || role.contains("Text")) {
                return Some((role, value));
            }
            if chat_mode && n <= 500 {
                return Some((role, value));
            }
            if n <= 400 {
                return Some((role, value));
            }
        }
    }

    if let Some(focused) = copy_attr(el, "AXFocusedUIElement") {
        let hit = text_from_element(focused as AXUIElementRef, depth + 1, chat_mode);
        unsafe { CFRelease(focused) };
        if hit.is_some() {
            return hit;
        }
    }

    let children = copy_attr(el, "AXChildren")?;
    let child_cap = if chat_mode { 48 } else { 24 };
    let out = unsafe {
        if CFGetTypeID(children) != CFArrayGetTypeID() {
            CFRelease(children);
            return None;
        }
        let count = CFArrayGetCount(children as CFArrayRef).min(child_cap);
        let mut best: Option<(String, String)> = None;
        for i in 0..count {
            let child = CFArrayGetValueAtIndex(children as CFArrayRef, i) as AXUIElementRef;
            if child.is_null() {
                continue;
            }
            if let Some((r, v)) = text_from_element(child, depth + 1, chat_mode) {
                let n = v.chars().count();
                let prefer = role_looks_editable(&r) && n <= 2000;
                if prefer {
                    best = Some((r, v));
                    break;
                }
                if best.as_ref().map(|(_, b)| b.chars().count()).unwrap_or(usize::MAX) > n
                    && n <= 800
                {
                    best = Some((r, v));
                }
            }
        }
        CFRelease(children);
        best
    };
    out
}

fn focused_text(chat_mode: bool) -> Option<(String, String)> {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if !system.is_null() {
            if let Some(focused) = copy_attr(system, "AXFocusedUIElement") {
                let hit = text_from_element(focused as AXUIElementRef, 0, chat_mode);
                CFRelease(focused);
                CFRelease(system as CFTypeRef);
                if hit.is_some() {
                    return hit;
                }
            } else {
                CFRelease(system as CFTypeRef);
            }
        }

        let pid = get_active_window().ok()?.process_id as i32;
        if pid <= 0 {
            return None;
        }
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let hit = text_from_element(app, 0, chat_mode);
        CFRelease(app as CFTypeRef);
        hit
    }
}

/// One-shot read for chat send click (WeChat input before it clears).
pub fn snapshot_committed_text(chat_mode: bool) -> Option<String> {
    // 250 ms 预算：本机 AX 读一屏文本远用不了这么久；超时说明树异常深
    // （微信），宁可退回拼音也不卡输入线程。
    begin_ax_budget(250);
    let (_, value) = focused_text(chat_mode)?;
    let committed = strip_ime_composing(&value);
    if committed.is_empty() {
        return None;
    }
    Some(committed)
}

/// Apply AX buffer edit as backspaces + append (never dump the whole field).
fn apply_ax_edit(engine: &CaptureEngine, prev: &str, next: &str) {
    if prev == next {
        return;
    }
    let add_chars = {
        let common = char_lcp_len(prev, next);
        next.chars().count().saturating_sub(common)
    };
    if add_chars > 0 {
        KEY_VIA_AX.fetch_add(add_chars as u64, Ordering::SeqCst);
    }
    // Single pipeline lock inside engine — do not lock typing then re-enter pipeline.
    engine.apply_composer_edit(prev, next);
}

pub fn spawn_ax_text_poller(engine: Arc<CaptureEngine>) {
    let _ = thread::Builder::new()
        .name("deskcap-ax-text".into())
        .spawn(move || {
            let mut prev = String::new();
            let mut prev_app = String::new();
            loop {
                thread::sleep(Duration::from_millis(
                    if FORCE_POLL.swap(false, Ordering::SeqCst) {
                        40
                    } else {
                        180
                    },
                ));
                if !engine.is_capturing() {
                    prev.clear();
                    prev_app.clear();
                    mark_ax_live(false);
                    continue;
                }
                if !engine.config.read().capture_keyboard {
                    continue;
                }
                if !engine.gate_input() {
                    continue;
                }
                let app = engine.current_app_name().unwrap_or_default();
                if app.contains("DeskCap") {
                    continue;
                }
                if app != prev_app {
                    prev.clear();
                    prev_app = app.clone();
                    mark_ax_live(false);
                }

                let chat = is_chat_app_name(&app);
                let Some((role, value)) = focused_text(chat) else {
                    mark_ax_live(false);
                    continue;
                };
                // Chat apps: accept looser roles; others still need editable-looking roles.
                if !chat && !role_looks_editable(&role) {
                    mark_ax_live(false);
                    continue;
                }

                let committed = strip_ime_composing(&value);
                // 微信/企微: AX often returns UI noise (spaces/labels). Only upgrade on real 汉字.
                if chat {
                    if !has_cjk(&committed) {
                        mark_ax_live(false);
                        continue;
                    }
                }
                mark_ax_live(true);

                if committed == prev {
                    continue;
                }

                // Focus enter: baseline on-screen text — do not re-emit whole composer
                // (same idea as Win UIA; avoids duplicate typing after switch-away).
                if prev.is_empty() {
                    prev = committed;
                    continue;
                }

                if committed.chars().count() > 2000
                    && char_lcp_len(&prev, &committed) < prev.chars().count() / 4
                {
                    prev = committed;
                    continue;
                }

                apply_ax_edit(&engine, &prev, &committed);
                prev = committed;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_trailing_pinyin_after_cjk() {
        let s = "你这个目录页是我模版的格式吗？shou";
        assert_eq!(strip_ime_composing(s), "你这个目录页是我模版的格式吗？");
    }

    #[test]
    fn strips_leading_pinyin_before_cjk() {
        let s = "shou uye你这个目录页是我模版的格式吗？首页";
        assert_eq!(strip_ime_composing(s), "你这个目录页是我模版的格式吗？首页");
    }

    #[test]
    fn keeps_pure_english() {
        assert_eq!(strip_ime_composing("hello world"), "hello world");
    }

    #[test]
    fn lcp_handles_mid_edit() {
        assert_eq!(char_lcp_len("可以了吗", "可以了"), 3);
        assert_eq!(char_lcp_len("可以了", "可以了首页"), 3);
    }
}
