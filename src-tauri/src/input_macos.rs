//! macOS CGEventTap — lean mask (no mouse-move flood), keep-alive re-enable.

use crate::engine::CaptureEngine;
use crate::typing::is_printable_name;
use parking_lot::Mutex;
use std::ffi::c_void;
use std::os::raw::{c_longlong, c_ulong};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type CGEventRef = *mut c_void;
type CFMachPortRef = *mut c_void;
type CFRunLoopSourceRef = *mut c_void;
type CFRunLoopRef = *mut c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFRunLoopTimerRef = *mut c_void;
type CFTimeInterval = f64;

const K_CG_HID_EVENT_TAP: u32 = 0;
const K_CG_SESSION_EVENT_TAP: u32 = 1;
const K_CG_HEAD_INSERT: u32 = 0;
const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

const K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
const K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
const K_CG_EVENT_KEY_DOWN: u32 = 10;
const K_CG_EVENT_OTHER_MOUSE_DOWN: u32 = 25;

const K_CG_KEYBOARD_EVENT_KEYCODE: u32 = 9;
const K_CG_MOUSE_EVENT_BUTTON_NUMBER: u32 = 3;
const K_CG_EVENT_FLAG_MASK_SHIFT: u64 = 0x0002_0000;
const K_CG_EVENT_FLAG_MASK_ALPHA: u64 = 0x0001_0000;
const K_CG_EVENT_FLAG_MASK_CONTROL: u64 = 0x0004_0000;
const K_CG_EVENT_FLAG_MASK_ALTERNATE: u64 = 0x0008_0000;
const K_CG_EVENT_FLAG_MASK_COMMAND: u64 = 0x0010_0000;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

type CGEventTapCallBack = Option<
    unsafe extern "C" fn(
        proxy: *mut c_void,
        etype: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef,
>;

type CFRunLoopTimerCallBack = Option<unsafe extern "C" fn(timer: CFRunLoopTimerRef, info: *mut c_void)>;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        eventsOfInterest: u64,
        callback: CGEventTapCallBack,
        userInfo: *mut c_void,
    ) -> CFMachPortRef;
    fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    fn CGEventTapIsEnabled(tap: CFMachPortRef) -> bool;
    fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> c_longlong;
    fn CGEventGetFlags(event: CGEventRef) -> u64;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventKeyboardGetUnicodeString(
        event: CGEventRef,
        maxStringLength: c_ulong,
        actualStringLength: *mut c_ulong,
        unicodeString: *mut u16,
    );
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: CFAllocatorRef,
        port: CFMachPortRef,
        order: c_longlong,
    ) -> CFRunLoopSourceRef;
    fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    fn CFRunLoopAddTimer(rl: CFRunLoopRef, timer: CFRunLoopTimerRef, mode: CFStringRef);
    fn CFRunLoopTimerCreate(
        allocator: CFAllocatorRef,
        fireDate: CFTimeInterval,
        interval: CFTimeInterval,
        flags: c_ulong,
        order: c_longlong,
        callout: CFRunLoopTimerCallBack,
        context: *mut c_void,
    ) -> CFRunLoopTimerRef;
    fn CFAbsoluteTimeGetCurrent() -> CFTimeInterval;
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: CFStringRef;
}

static ENGINE: Mutex<Option<Arc<CaptureEngine>>> = Mutex::new(None);
static TAP_OK: AtomicBool = AtomicBool::new(false);
static TAP_ENABLED: AtomicBool = AtomicBool::new(false);
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);
static KEYDOWN_COUNT: AtomicU64 = AtomicU64::new(0);
static MOUSE_COUNT: AtomicU64 = AtomicU64::new(0);
static LAST_KEYDOWN_MS: AtomicU64 = AtomicU64::new(0);
static ACTIVE_TAP: Mutex<Option<usize>> = Mutex::new(None);
static TAP_CREATED_MS: AtomicU64 = AtomicU64::new(0);

pub fn is_accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Raw API — may be false for ad-hoc signed apps even when granted in Settings.
pub fn is_input_monitoring_api() -> bool {
    unsafe { CGPreflightListenEventAccess() }
}

pub fn is_input_monitoring_ok() -> bool {
    if is_input_monitoring_api() {
        return true;
    }
    // Empirical: enabled tap or real keydowns ⇒ monitoring works.
    // Mouse alone is not enough (ListenOnly can deliver clicks without keyboard).
    if TAP_ENABLED.load(Ordering::SeqCst) {
        return true;
    }
    if KEYDOWN_COUNT.load(Ordering::SeqCst) > 0 {
        return true;
    }
    false
}

pub fn is_accessibility_ok_display() -> bool {
    if is_accessibility_trusted() {
        return true;
    }
    // Only AX evidence — never infer from mouse/EventTap.
    crate::ax_text::ax_chars_captured() > 0
}

/// True when EventTap delivered a keydown recently (kept for diagnostics).
#[allow(dead_code)]
pub fn recent_keydown(within_secs: f64) -> bool {
    let ms = LAST_KEYDOWN_MS.load(Ordering::SeqCst);
    if ms == 0 {
        return false;
    }
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as u64,
        Err(_) => return false,
    };
    (now.saturating_sub(ms) as f64) / 1000.0 < within_secs
}

pub fn prompt_accessibility() {
    // null options = no dialog (safe). Manual Settings toggles still required for ad-hoc.
    // Do NOT pass a hand-rolled CFDictionary — wrong callbacks crash in CFGetTypeID.
    unsafe {
        let _ = AXIsProcessTrustedWithOptions(ptr::null());
    }
}

pub fn keydown_count() -> u64 {
    KEYDOWN_COUNT.load(Ordering::SeqCst)
}

pub fn mouse_count() -> u64 {
    MOUSE_COUNT.load(Ordering::SeqCst)
}

pub fn is_tap_ok() -> bool {
    // Only "enabled" counts — create≠enabled; never lie with "就绪".
    TAP_ENABLED.load(Ordering::SeqCst)
}

pub fn last_input_age_secs() -> Option<f64> {
    let ms = LAST_INPUT_MS.load(Ordering::SeqCst);
    if ms == 0 {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((now.saturating_sub(ms) as f64) / 1000.0)
}

pub fn request_input_permissions() {
    unsafe {
        let _ = CGRequestListenEventAccess();
    }
}

fn note_input() {
    if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
        LAST_INPUT_MS.store(d.as_millis() as u64, Ordering::SeqCst);
    }
    TAP_OK.store(true, Ordering::SeqCst);
    TAP_ENABLED.store(true, Ordering::SeqCst);
}

/// Work deferred off the EventTap thread — disk/locks here must NEVER run in `tap_callback`.
enum TapJob {
    Click {
        button: &'static str,
        x: f64,
        y: f64,
    },
    Key {
        keycode: i64,
        text: Option<String>,
    },
    Clipboard(&'static str),
}

static JOB_TX: Mutex<Option<std::sync::mpsc::SyncSender<TapJob>>> = Mutex::new(None);

fn enqueue(job: TapJob) {
    if let Some(tx) = JOB_TX.lock().as_ref() {
        if tx.try_send(job).is_err() {
            // Prefer dropping clicks over keys when saturated — but try_send already
            // dropped this job. Count for diagnostics; widen channel to reduce loss.
            crate::logutil::warn("input queue full — key/click dropped");
        }
    }
}

pub fn spawn_listener(engine: Arc<CaptureEngine>) {
    request_input_permissions();
    *ENGINE.lock() = Some(Arc::clone(&engine));

    let (tx, rx) = std::sync::mpsc::sync_channel::<TapJob>(8192);
    *JOB_TX.lock() = Some(tx);
    let worker = Arc::clone(&engine);
    let _ = thread::Builder::new()
        .name("deskcap-input-worker".into())
        .spawn(move || {
            crate::logutil::info("mac input worker up (EventTap enqueue-only)");
            while let Ok(job) = rx.recv() {
                if !worker.is_capturing() {
                    continue;
                }
                let t0 = std::time::Instant::now();
                match job {
                    TapJob::Click { button, x, y } => {
                        if !worker.gate_input_fast() {
                            continue;
                        }
                        worker.set_mouse_pos(x, y);
                        worker.on_mouse_click(button);
                    }
                    TapJob::Key { keycode, text } => {
                        if !worker.gate_input_fast() {
                            continue;
                        }
                        if !worker.config.read().capture_keyboard {
                            continue;
                        }
                        worker.on_key_input(keycode, text);
                    }
                    TapJob::Clipboard(action) => {
                        worker.on_clipboard_shortcut(action);
                    }
                }
                crate::logutil::lag_ms("input_worker", t0.elapsed().as_millis() as u64);
            }
        });

    let _ = thread::Builder::new()
        .name("deskcap-input".into())
        .spawn(|| {
            let _ = std::panic::catch_unwind(|| {
                let mut last_err_log = 0u64;
                loop {
                    TAP_OK.store(false, Ordering::SeqCst);
                    TAP_ENABLED.store(false, Ordering::SeqCst);
                    match run_tap_once() {
                        Ok(()) => {
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            if now.saturating_sub(last_err_log) >= 30 {
                                eprintln!("DeskCap: event tap runloop exited; retrying…");
                                last_err_log = now;
                            }
                        }
                        Err(msg) => {
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map(|d| d.as_secs())
                                .unwrap_or(0);
                            if now.saturating_sub(last_err_log) >= 30 {
                                eprintln!("DeskCap input: {msg} (retrying)");
                                crate::logutil::warn(&format!("EventTap: {msg}"));
                                if let Some(home) = std::env::var_os("HOME") {
                                    let path = std::path::PathBuf::from(home)
                                        .join("Library/Application Support/DeskCap/input-tap.log");
                                    let _ = std::fs::write(
                                        &path,
                                        format!(
                                            "FAIL {msg}\nmonitor_api={} ax_api={}\n",
                                            unsafe { CGPreflightListenEventAccess() },
                                            unsafe { AXIsProcessTrusted() }
                                        ),
                                    );
                                }
                                last_err_log = now;
                            }
                        }
                    }
                    TAP_OK.store(false, Ordering::SeqCst);
                    TAP_ENABLED.store(false, Ordering::SeqCst);
                    *ACTIVE_TAP.lock() = None;
                    thread::sleep(Duration::from_secs(2));
                    request_input_permissions();
                }
            });
        });
}

/// Mouse-move floods disable taps under macOS — never include move/drag.
fn mouse_mask() -> u64 {
    (1u64 << K_CG_EVENT_LEFT_MOUSE_DOWN)
        | (1u64 << K_CG_EVENT_RIGHT_MOUSE_DOWN)
        | (1u64 << K_CG_EVENT_OTHER_MOUSE_DOWN)
}

fn key_mask() -> u64 {
    1u64 << K_CG_EVENT_KEY_DOWN
}

fn try_create_tap(tap_loc: u32, options: u32, mask: u64) -> CFMachPortRef {
    unsafe {
        CGEventTapCreate(
            tap_loc,
            K_CG_HEAD_INSERT,
            options,
            mask,
            Some(tap_callback),
            ptr::null_mut(),
        )
    }
}

/// Only Default (active) taps — ListenOnly routinely delivers mouse but never KeyDown.
fn create_enabled_tap(mask: u64) -> Option<(CFMachPortRef, &'static str)> {
    let candidates = [
        (K_CG_HID_EVENT_TAP, K_CG_EVENT_TAP_OPTION_DEFAULT, "hid/default"),
        (K_CG_SESSION_EVENT_TAP, K_CG_EVENT_TAP_OPTION_DEFAULT, "session/default"),
    ];
    for (loc, opt, label) in candidates {
        let tap = try_create_tap(loc, opt, mask);
        if tap.is_null() {
            continue;
        }
        unsafe {
            CGEventTapEnable(tap, true);
            if CGEventTapIsEnabled(tap) {
                return Some((tap, label));
            }
        }
        eprintln!("DeskCap: EventTap {label} created but disabled");
    }
    None
}

/// Pending Default tap (may enable after user toggles Input Monitoring).
fn create_any_tap(mask: u64) -> Option<(CFMachPortRef, &'static str)> {
    let candidates = [
        (K_CG_HID_EVENT_TAP, K_CG_EVENT_TAP_OPTION_DEFAULT, "hid/default"),
        (K_CG_SESSION_EVENT_TAP, K_CG_EVENT_TAP_OPTION_DEFAULT, "session/default"),
    ];
    for (loc, opt, label) in candidates {
        let tap = try_create_tap(loc, opt, mask);
        if !tap.is_null() {
            unsafe {
                CGEventTapEnable(tap, true);
            }
            return Some((tap, label));
        }
    }
    None
}

unsafe extern "C" fn keep_alive_timer(_timer: CFRunLoopTimerRef, _info: *mut c_void) {
    let tap_ptr = match *ACTIVE_TAP.lock() {
        Some(p) => p as CFMachPortRef,
        None => return,
    };
    if tap_ptr.is_null() {
        return;
    }
    // Only re-enable — never tear down the runloop (that caused flicker + lost keys).
    let enabled = CGEventTapIsEnabled(tap_ptr);
    if !enabled {
        CGEventTapEnable(tap_ptr, true);
    }
    let now = CGEventTapIsEnabled(tap_ptr);
    TAP_ENABLED.store(now, Ordering::SeqCst);
    if now {
        TAP_OK.store(true, Ordering::SeqCst);
    }
}

fn run_tap_once() -> Result<(), String> {
    unsafe {
        let mask = key_mask() | mouse_mask();
        let (primary, how) = create_enabled_tap(mask)
            .or_else(|| create_enabled_tap(key_mask()))
            .or_else(|| {
                create_any_tap(mask).map(|t| {
                    eprintln!("DeskCap: using pending (disabled) EventTap — toggle 输入监控 then wait");
                    t
                })
            })
            .or_else(|| create_any_tap(mouse_mask()))
            .ok_or_else(|| {
                "CGEventTapCreate failed — enable Input Monitoring + Accessibility for DeskCap"
                    .to_string()
            })?;

        let rl = CFRunLoopGetCurrent();
        let primary_src = CFMachPortCreateRunLoopSource(ptr::null(), primary, 0);
        if primary_src.is_null() {
            return Err("CFMachPortCreateRunLoopSource failed".into());
        }
        CFRunLoopAddSource(rl, primary_src, kCFRunLoopCommonModes);
        CGEventTapEnable(primary, true);

        *ACTIVE_TAP.lock() = Some(primary as usize);
        let enabled = CGEventTapIsEnabled(primary);
        TAP_ENABLED.store(enabled, Ordering::SeqCst);
        TAP_OK.store(enabled, Ordering::SeqCst);
        if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
            TAP_CREATED_MS.store(d.as_millis() as u64, Ordering::SeqCst);
        }

        let timer = CFRunLoopTimerCreate(
            ptr::null(),
            CFAbsoluteTimeGetCurrent() + 0.5,
            0.5,
            0,
            0,
            Some(keep_alive_timer),
            ptr::null_mut(),
        );
        if !timer.is_null() {
            CFRunLoopAddTimer(rl, timer, kCFRunLoopCommonModes);
        }

        let line = format!(
            "DeskCap: EventTap up (enabled={} via={} monitor_api={} ax_api={})",
            enabled,
            how,
            CGPreflightListenEventAccess(),
            AXIsProcessTrusted()
        );
        eprintln!("{line}");
        // Persist one line for verify without attaching stderr to LaunchServices.
        if let Some(home) = std::env::var_os("HOME") {
            let path = std::path::PathBuf::from(home)
                .join("Library/Application Support/DeskCap/input-tap.log");
            let _ = std::fs::write(&path, format!("{line}\n"));
        }
        CFRunLoopRun();
    }
    Ok(())
}

unsafe extern "C" fn tap_callback(
    _proxy: *mut c_void,
    etype: u32,
    event: CGEventRef,
    _user: *mut c_void,
) -> CGEventRef {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch(etype, event);
    }));
    event
}

fn dispatch(etype: u32, event: CGEventRef) {
    // macOS disables taps under load — re-enable immediately (no capture work).
    if etype == K_CG_EVENT_TAP_DISABLED_BY_TIMEOUT || etype == K_CG_EVENT_TAP_DISABLED_BY_USER_INPUT
    {
        if let Some(p) = *ACTIVE_TAP.lock() {
            let tap = p as CFMachPortRef;
            unsafe {
                CGEventTapEnable(tap, true);
                TAP_ENABLED.store(CGEventTapIsEnabled(tap), Ordering::SeqCst);
            }
        }
        return;
    }

    // EventTap is an active filter: this function MUST stay microsecond-fast.
    // Never on_key_input / on_mouse_click / AX / disk here — enqueue only.
    // Prefer blocking lock over try_lock: try_lock miss silently dropped keydowns
    // (missing pinyin initials under contention).
    let eng = match ENGINE.lock().as_ref().cloned() {
        Some(e) => e,
        None => return,
    };
    if !eng.is_capturing() {
        return;
    }

    unsafe {
        match etype {
            K_CG_EVENT_LEFT_MOUSE_DOWN => {
                if !eng.gate_input_fast() {
                    return;
                }
                let p = CGEventGetLocation(event);
                MOUSE_COUNT.fetch_add(1, Ordering::SeqCst);
                note_input();
                enqueue(TapJob::Click {
                    button: "left",
                    x: p.x,
                    y: p.y,
                });
            }
            K_CG_EVENT_RIGHT_MOUSE_DOWN => {
                if !eng.gate_input_fast() {
                    return;
                }
                let p = CGEventGetLocation(event);
                MOUSE_COUNT.fetch_add(1, Ordering::SeqCst);
                note_input();
                enqueue(TapJob::Click {
                    button: "right",
                    x: p.x,
                    y: p.y,
                });
            }
            K_CG_EVENT_OTHER_MOUSE_DOWN => {
                if !eng.gate_input_fast() {
                    return;
                }
                let p = CGEventGetLocation(event);
                let btn = CGEventGetIntegerValueField(event, K_CG_MOUSE_EVENT_BUTTON_NUMBER);
                MOUSE_COUNT.fetch_add(1, Ordering::SeqCst);
                note_input();
                enqueue(TapJob::Click {
                    button: if btn == 2 { "middle" } else { "other" },
                    x: p.x,
                    y: p.y,
                });
            }
            K_CG_EVENT_KEY_DOWN => {
                KEYDOWN_COUNT.fetch_add(1, Ordering::SeqCst);
                if let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) {
                    LAST_KEYDOWN_MS.store(d.as_millis() as u64, Ordering::SeqCst);
                }
                note_input();
                if !eng.gate_input_fast() {
                    return;
                }
                if !eng.config.read().capture_keyboard {
                    return;
                }
                let flags = CGEventGetFlags(event);
                let cmd = (flags & K_CG_EVENT_FLAG_MASK_COMMAND) != 0;
                let ctrl = (flags & K_CG_EVENT_FLAG_MASK_CONTROL) != 0;
                let alt = (flags & K_CG_EVENT_FLAG_MASK_ALTERNATE) != 0;
                let keycode = CGEventGetIntegerValueField(event, K_CG_KEYBOARD_EVENT_KEYCODE);

                if (cmd || ctrl) && !alt {
                    let action = match keycode {
                        9 => Some("paste"),
                        8 => Some("copy"),
                        7 => Some("cut"),
                        _ => None,
                    };
                    if let Some(action) = action {
                        if eng.config.read().capture_clipboard {
                            enqueue(TapJob::Clipboard(action));
                        }
                    }
                    return;
                }
                if cmd || ctrl || alt {
                    return;
                }
                let shift = (flags & K_CG_EVENT_FLAG_MASK_SHIFT) != 0;
                let caps = (flags & K_CG_EVENT_FLAG_MASK_ALPHA) != 0;
                let raw =
                    unicode_from_event(event).or_else(|| keycode_fallback(keycode, shift ^ caps));
                let chat = eng
                    .current_app_name()
                    .map(|n| crate::ax_text::is_chat_app_name(&n))
                    .unwrap_or(false);
                let defer_to_ax =
                    !chat && is_accessibility_trusted() && crate::ax_text::ax_live_editable();
                let text = if defer_to_ax {
                    if keycode == 51 {
                        return;
                    }
                    match raw.as_deref() {
                        Some("\n") | Some("\t") => raw,
                        Some(t) if t.chars().any(|c| !c.is_ascii()) => raw,
                        _ => None,
                    }
                } else {
                    raw
                };
                if text.is_none() && keycode != 36 && keycode != 76 && keycode != 48 && keycode != 51
                {
                    return;
                }
                enqueue(TapJob::Key { keycode, text });
            }
            _ => {}
        }
    }
}

fn unicode_from_event(event: CGEventRef) -> Option<String> {
    let mut buf = [0u16; 16];
    let mut actual: c_ulong = 0;
    unsafe {
        CGEventKeyboardGetUnicodeString(event, 16, &mut actual, buf.as_mut_ptr());
    }
    if actual == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..actual as usize]);
    if s == "\u{8}" || s == "\u{7f}" {
        return Some("\u{8}".into());
    }
    if s == "\r" || s == "\n" {
        return Some("\n".into());
    }
    if s == "\t" {
        return Some("\t".into());
    }
    if is_printable_name(&s) {
        Some(s)
    } else {
        None
    }
}

fn keycode_fallback(code: i64, shift: bool) -> Option<String> {
    let ch: Option<char> = match code {
        0 => Some(if shift { 'A' } else { 'a' }),
        1 => Some(if shift { 'S' } else { 's' }),
        2 => Some(if shift { 'D' } else { 'd' }),
        3 => Some(if shift { 'F' } else { 'f' }),
        4 => Some(if shift { 'H' } else { 'h' }),
        5 => Some(if shift { 'G' } else { 'g' }),
        6 => Some(if shift { 'Z' } else { 'z' }),
        7 => Some(if shift { 'X' } else { 'x' }),
        8 => Some(if shift { 'C' } else { 'c' }),
        9 => Some(if shift { 'V' } else { 'v' }),
        11 => Some(if shift { 'B' } else { 'b' }),
        12 => Some(if shift { 'Q' } else { 'q' }),
        13 => Some(if shift { 'W' } else { 'w' }),
        14 => Some(if shift { 'E' } else { 'e' }),
        15 => Some(if shift { 'R' } else { 'r' }),
        16 => Some(if shift { 'Y' } else { 'y' }),
        17 => Some(if shift { 'T' } else { 't' }),
        18 => Some(if shift { '!' } else { '1' }),
        19 => Some(if shift { '@' } else { '2' }),
        20 => Some(if shift { '#' } else { '3' }),
        21 => Some(if shift { '$' } else { '4' }),
        22 => Some(if shift { '^' } else { '6' }),
        23 => Some(if shift { '%' } else { '5' }),
        24 => Some(if shift { '+' } else { '=' }),
        25 => Some(if shift { '(' } else { '9' }),
        26 => Some(if shift { '&' } else { '7' }),
        27 => Some(if shift { '_' } else { '-' }),
        28 => Some(if shift { '*' } else { '8' }),
        29 => Some(if shift { ')' } else { '0' }),
        30 => Some(if shift { '}' } else { ']' }),
        31 => Some(if shift { 'O' } else { 'o' }),
        32 => Some(if shift { 'U' } else { 'u' }),
        33 => Some(if shift { '{' } else { '[' }),
        34 => Some(if shift { 'I' } else { 'i' }),
        35 => Some(if shift { 'P' } else { 'p' }),
        37 => Some(if shift { 'L' } else { 'l' }),
        38 => Some(if shift { 'J' } else { 'j' }),
        39 => Some(if shift { '"' } else { '\'' }),
        40 => Some(if shift { 'K' } else { 'k' }),
        41 => Some(if shift { ':' } else { ';' }),
        42 => Some(if shift { '|' } else { '\\' }),
        43 => Some(if shift { '<' } else { ',' }),
        44 => Some(if shift { '?' } else { '/' }),
        45 => Some(if shift { 'N' } else { 'n' }),
        46 => Some(if shift { 'M' } else { 'm' }),
        47 => Some(if shift { '>' } else { '.' }),
        49 => Some(' '),
        50 => Some(if shift { '~' } else { '`' }),
        36 | 76 => Some('\n'),
        48 => Some('\t'),
        51 => Some('\u{8}'),
        _ => None,
    };
    ch.map(|c| c.to_string())
}
