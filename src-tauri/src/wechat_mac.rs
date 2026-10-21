//! WeChat (Mac) Chinese recovery without changing the user's IME.
//!
//! Order: AX snapshot → deep AX (ids/bubbles) → event-driven Vision OCR
//! of the composer strip (Enter/click flush only; image not kept).

use crate::ime::{has_cjk, is_agent_noise_text};
use std::ffi::c_void;
use std::os::raw::c_longlong;
use std::path::PathBuf;
use std::process::Command;
use std::ptr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use xcap::Window;

type AXUIElementRef = *mut c_void;
type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFArrayRef = *const c_void;

const AX_OK: i32 = 0;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
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
    fn CFStringGetTypeID() -> u64;
    fn CFArrayGetTypeID() -> u64;
    fn CFGetTypeID(cf: CFTypeRef) -> u64;
    fn CFStringGetLength(theString: CFStringRef) -> c_longlong;
    fn CFStringGetCharacters(theString: CFStringRef, range: CFRange, buffer: *mut u16);
    fn CFArrayGetCount(theArray: CFArrayRef) -> c_longlong;
    fn CFArrayGetValueAtIndex(theArray: CFArrayRef, idx: c_longlong) -> *const c_void;
    fn CFRelease(cf: CFTypeRef);
}

#[repr(C)]
struct CFRange {
    location: c_longlong,
    length: c_longlong,
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
        if len <= 0 || len > 8_000 {
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

fn attr_string(el: AXUIElementRef, name: &str) -> String {
    copy_attr(el, name)
        .and_then(|v| {
            let s = cfstring_to_string(v as CFStringRef);
            unsafe { CFRelease(v) };
            s
        })
        .unwrap_or_default()
}

fn wechat_pid() -> Option<i32> {
    let Ok(win) = active_win_pos_rs::get_active_window() else {
        return None;
    };
    let name = win.app_name.to_lowercase();
    if win.app_name.contains("微信") || name.contains("wechat") || name.contains("weixin") {
        Some(win.process_id as i32)
    } else {
        None
    }
}

fn accept_candidate(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t.chars().count() > 400 {
        return false;
    }
    if !has_cjk(t) || is_agent_noise_text(t) {
        return false;
    }
    true
}

/// Deep AX walk for known WeChat ids / short CJK values (composer or bubbles).
pub fn ax_recover_chinese() -> Option<String> {
    // 与 snapshot 同源：AX 树深时 200ms 内拿不到就放弃（best-effort）。
    crate::ax_text::begin_ax_budget(200);
    let pid = wechat_pid()?;
    if pid <= 0 {
        return None;
    }
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }
        let mut best: Option<String> = None;
        let mut best_score = 0i32;
        walk(app, 0, &mut best, &mut best_score);
        CFRelease(app as CFTypeRef);
        best.filter(|s| accept_candidate(s))
    }
}

unsafe fn walk(
    el: AXUIElementRef,
    depth: usize,
    best: &mut Option<String>,
    best_score: &mut i32,
) {
    if el.is_null() || depth > 16 {
        return;
    }
    let role = attr_string(el, "AXRole");
    let id = attr_string(el, "AXIdentifier");
    let title = attr_string(el, "AXTitle");
    let mut value = attr_string(el, "AXValue");
    if value.is_empty() {
        value = attr_string(el, "AXSelectedText");
    }
    if value.is_empty() {
        value = attr_string(el, "AXDescription");
    }

    let id_l = id.to_lowercase();
    let mut score = 0i32;
    if id_l.contains("chat_input") || id_l.contains("input_field") || id_l.contains("composer") {
        score += 50;
    }
    if id_l.contains("chat_bubble") || id_l.contains("bubble") || id_l.contains("virtual_cell") {
        score += 30;
    }
    if role.contains("Text") || role.contains("Edit") {
        score += 10;
    }
    if accept_candidate(&value) {
        score += 20 + (40 - value.chars().count() as i32).clamp(0, 40);
        if score > *best_score {
            *best_score = score;
            *best = Some(value.trim().to_string());
        }
    } else if accept_candidate(&title) && score >= 30 {
        score += 15;
        if score > *best_score {
            *best_score = score;
            *best = Some(title.trim().to_string());
        }
    }

    let Some(children) = copy_attr(el, "AXChildren") else {
        return;
    };
    if CFGetTypeID(children) != CFArrayGetTypeID() {
        CFRelease(children);
        return;
    }
    let cap = if depth < 3 { 64 } else { 40 };
    let count = CFArrayGetCount(children as CFArrayRef).min(cap);
    for i in 0..count {
        let child = CFArrayGetValueAtIndex(children as CFArrayRef, i) as AXUIElementRef;
        walk(child, depth + 1, best, best_score);
    }
    CFRelease(children);
}

fn ocr_helper_path() -> Option<PathBuf> {
    if let Some(p) = option_env!("DESKCAP_WECHAT_OCR") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    // Prefer helper next to the DeskCap binary (Contents/MacOS).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("deskcap-wechat-ocr");
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("macos/wechat_ocr.swift");
    if repo.is_file() {
        return Some(repo);
    }
    None
}

fn capture_wechat_composer_png() -> Option<PathBuf> {
    let windows = Window::all().ok()?;
    let win = windows.into_iter().find(|w| {
        let app = w.app_name().unwrap_or_default();
        let title = w.title().unwrap_or_default();
        let a = app.to_lowercase();
        app.contains("微信")
            || a.contains("wechat")
            || title.contains("微信")
            || title.to_lowercase().contains("wechat")
    })?;
    let full = win.capture_image().ok()?;
    let w = full.width();
    let h = full.height();
    if w < 80 || h < 80 {
        return None;
    }
    let y0 = (h as f32 * 0.72) as u32;
    let crop_h = h.saturating_sub(y0).max(40);
    let cropped = image::imageops::crop_imm(&full, 0, y0, w, crop_h).to_image();
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("deskcap-wechat-ocr-{ms}.png"));
    cropped.save(&path).ok()?;
    Some(path)
}

/// OCR 冷却：`ocd_*` 实测 10 次调用 0 命中，但首次 Vision 初始化要 4.7 s。
/// 60 秒内最多跑一次 —— 既保留「偶尔救回」的可能，又不会每次回车都付秒级代价。
static OCR_LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const OCR_COOLDOWN_SECS: u64 = 60;

pub fn ocr_composer_chinese() -> Option<String> {
    use std::sync::atomic::Ordering;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = OCR_LAST.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < OCR_COOLDOWN_SECS {
        return None;
    }
    OCR_LAST.store(now, Ordering::Relaxed);
    let helper = ocr_helper_path()?;
    let png = capture_wechat_composer_png()?;
    let out = if helper.extension().and_then(|e| e.to_str()) == Some("swift") {
        Command::new("swift").arg(&helper).arg(&png).output()
    } else {
        Command::new(&helper).arg(&png).output()
    };
    let _ = std::fs::remove_file(&png);
    let out = out.ok()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        if !err.trim().is_empty() {
            crate::logutil::warn(&format!("wechat ocr helper: {err}"));
        }
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if accept_candidate(&text) {
        Some(text)
    } else {
        None
    }
}

/// Full recovery used at chat flush time. Returns (text, capture tag).
pub fn recover_chinese_on_send() -> Option<(String, &'static str)> {
    if let Some(t) = crate::ax_text::snapshot_committed_text(true) {
        if accept_candidate(&t) {
            return Some((t, "ax_commit"));
        }
    }
    if let Some(t) = ax_recover_chinese() {
        return Some((t, "ax_deep"));
    }
    // Composer may clear on Enter; brief wait then bubble / OCR.
    std::thread::sleep(Duration::from_millis(80));
    if let Some(t) = ax_recover_chinese() {
        return Some((t, "ax_bubble"));
    }
    // OCR must run while composer still shows 汉字 — call BEFORE enter ideally.
    // On Enter the strip may already be empty; still try once.
    if let Some(t) = ocr_composer_chinese() {
        crate::logutil::info(&format!("wechat ocr_commit chars={}", t.chars().count()));
        return Some((t, "ocr_commit"));
    }
    None
}

/// OCR *before* promoting/flushing Enter — best chance to see on-screen 汉字.
///
/// 0.6.49 观测补丁 — 这条链跑在 `input_worker` 上（见 `engine.rs:on_key_input`），
/// 是**同步阻塞**的：AX 深读 + Vision OCR 都可能几百 ms 到秒级。
/// 本机实测 `deskcap.log` 出现 769 条 `WARN lag input_worker=`，峰值 17390ms。
/// 这里加分段耗时打点，把「卡在哪一步」变成可 grep 的数字，
/// 避免下次只能靠猜或靠整体 lag 反推。行为不变，只加日志。
pub fn recover_chinese_before_send() -> Option<(String, &'static str)> {
    let t_all = std::time::Instant::now();
    if let Some(t) = crate::ax_text::snapshot_committed_text(true) {
        if accept_candidate(&t) {
            crate::logutil::info(&format!(
                "wechat pre_send stage=ax_commit ms={}",
                t_all.elapsed().as_millis()
            ));
            return Some((t, "ax_commit"));
        }
    }
    // 总预算 400ms：AX/OCR 都只是"尽力把拼音升级成汉字"，不值得为它卡住
    // 输入线程（历史实测 1.6–4.3 s，日志 15 条 ERROR lag input_worker）。
    const TOTAL_BUDGET_MS: u128 = 400;
    if t_all.elapsed().as_millis() > TOTAL_BUDGET_MS {
        crate::logutil::info(&format!(
            "wechat pre_send stage=budget_exhausted ms={} (after ax_commit)",
            t_all.elapsed().as_millis()
        ));
        return None;
    }
    let t_ax = t_all.elapsed().as_millis();
    if let Some(t) = ax_recover_chinese() {
        crate::logutil::info(&format!(
            "wechat pre_send stage=ax_deep ms={} (ax_commit_path={}ms)",
            t_all.elapsed().as_millis(),
            t_ax
        ));
        return Some((t, "ax_deep"));
    }
    let t_deep = t_all.elapsed().as_millis();
    if t_all.elapsed().as_millis() > TOTAL_BUDGET_MS {
        crate::logutil::info(&format!(
            "wechat pre_send stage=budget_exhausted ms={} (after ax_deep)",
            t_all.elapsed().as_millis()
        ));
        return None;
    }
    if let Some(t) = ocr_composer_chinese() {
        crate::logutil::info(&format!(
            "wechat ocr_pre_send chars={} stage=ocr_commit ms={} (before_ocr={}ms)",
            t.chars().count(),
            t_all.elapsed().as_millis(),
            t_deep
        ));
        return Some((t, "ocr_commit"));
    }
    // 全miss 也要留一条，否则「没打中」和「没跑」在日志上分不开。
    crate::logutil::info(&format!(
        "wechat pre_send stage=miss ms={} (before_ocr={}ms)",
        t_all.elapsed().as_millis(),
        t_deep
    ));
    None
}
