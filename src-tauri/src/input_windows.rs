//! Windows input without WH_MOUSE_LL.
//!
//! Why not only WH_KEYBOARD_LL?
//! When a Chromium/Electron/WebView2 window is focused (Cursor, Chrome, VS Code,
//! DeskCap's own UI), Windows often **silently stops delivering** LL keyboard
//! hooks (LowLevelHooksTimeout / Chromium input path). Mac CGEventTap does not
//! have this failure mode — that is why Mac Cursor typing works and Win did not.
//!
//! Strategy:
//! 1. WH_KEYBOARD_LL — fast path for native apps (Notepad, Win32).
//! 2. GetAsyncKeyState edge poll (~10ms) — fallback when LL goes deaf (Cursor…).
//! 3. Worker-side 40ms vk dedupe so hook+poll never double-insert.
//! 4. Raw Input for mouse buttons only (never WH_MOUSE_LL).

use crate::engine::CaptureEngine;
use parking_lot::Mutex;
use std::mem::{size_of, zeroed};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static STARTED: AtomicBool = AtomicBool::new(false);
static LAST_INPUT_MS: AtomicU64 = AtomicU64::new(0);
static KEYDOWN_N: AtomicU64 = AtomicU64::new(0);
static MOUSE_N: AtomicU64 = AtomicU64::new(0);
static KEY_VIA_POLL: AtomicU64 = AtomicU64::new(0);
static LAST_DEDUP_VK: AtomicU32 = AtomicU32::new(0);
static LAST_DEDUP_MS: AtomicU64 = AtomicU64::new(0);
static LAST_DEDUP_CLICK_MS: AtomicU64 = AtomicU64::new(0);
static LAST_DEDUP_CLICK_BTN: AtomicU32 = AtomicU32::new(0);
static LAST_RAW_CLICK_MS: AtomicU64 = AtomicU64::new(0);
static JOB_TX: Mutex<Option<SyncSender<Job>>> = Mutex::new(None);

enum Job {
    Key {
        vk: u32,
        scan: u32,
        flags: u32,
        text: Option<String>,
        via_poll: bool,
    },
    Click { button: &'static str },
}

pub fn keydown_count() -> u64 {
    KEYDOWN_N.load(Ordering::Relaxed)
}
pub fn mouse_count() -> u64 {
    MOUSE_N.load(Ordering::Relaxed)
}
pub fn key_via_poll() -> u64 {
    KEY_VIA_POLL.load(Ordering::Relaxed)
}
pub fn last_input_age_secs() -> Option<f64> {
    let ms = LAST_INPUT_MS.load(Ordering::Relaxed);
    if ms == 0 {
        return None;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((now.saturating_sub(ms) as f64) / 1000.0)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn note_input() {
    LAST_INPUT_MS.store(now_ms(), Ordering::Relaxed);
}

fn accept_key_dedup(vk: u32) -> bool {
    let now = now_ms();
    let prev_vk = LAST_DEDUP_VK.load(Ordering::Relaxed);
    let prev_ms = LAST_DEDUP_MS.load(Ordering::Relaxed);
    if prev_vk == vk && now.saturating_sub(prev_ms) < 40 {
        return false;
    }
    LAST_DEDUP_VK.store(vk, Ordering::Relaxed);
    LAST_DEDUP_MS.store(now, Ordering::Relaxed);
    true
}

fn click_btn_id(button: &str) -> u32 {
    match button {
        "left" => 1,
        "right" => 2,
        "middle" => 3,
        _ => 9,
    }
}

fn accept_click_dedup(button: &str) -> bool {
    let now = now_ms();
    let id = click_btn_id(button);
    let prev_id = LAST_DEDUP_CLICK_BTN.load(Ordering::Relaxed);
    let prev_ms = LAST_DEDUP_CLICK_MS.load(Ordering::Relaxed);
    if prev_id == id && now.saturating_sub(prev_ms) < 50 {
        return false;
    }
    LAST_DEDUP_CLICK_BTN.store(id, Ordering::Relaxed);
    LAST_DEDUP_CLICK_MS.store(now, Ordering::Relaxed);
    true
}

pub fn spawn_listener(engine: Arc<CaptureEngine>) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = mpsc::sync_channel::<Job>(1024);
    *JOB_TX.lock() = Some(tx);

    let worker = Arc::clone(&engine);
    let _ = thread::Builder::new()
        .name("deskcap-input-worker".into())
        .spawn(move || {
            crate::logutil::info(
                "win input worker up (kb LL + key poll fallback + raw mouse; no WH_MOUSE_LL)",
            );
            while let Ok(job) = rx.recv() {
                if !worker.is_capturing() {
                    continue;
                }
                // Fast gate only — never get_active_window per key (Cursor/Electron on Win).
                if !worker.gate_input_fast() {
                    continue;
                }
                let t0 = Instant::now();
                match job {
                    Job::Key {
                        vk,
                        scan,
                        flags,
                        text,
                        via_poll,
                    } => {
                        if !worker.config.read().capture_keyboard {
                            continue;
                        }
                        if !accept_key_dedup(vk) {
                            continue;
                        }
                        if via_poll {
                            KEY_VIA_POLL.fetch_add(1, Ordering::Relaxed);
                        }
                        handle_key(&worker, vk, scan, flags, text);
                    }
                    Job::Click { button } => {
                        if !worker.config.read().capture_mouse {
                            continue;
                        }
                        if !accept_click_dedup(button) {
                            continue;
                        }
                        let (x, y) = cursor_pos();
                        worker.set_mouse_pos(x, y);
                        worker.on_mouse_click(button);
                    }
                }
                crate::logutil::lag_ms("input_worker", t0.elapsed().as_millis() as u64);
            }
        });

    let _ = thread::Builder::new()
        .name("deskcap-kb-hook".into())
        .spawn(|| {
            crate::logutil::info("WH_KEYBOARD_LL up");
            if let Err(e) = run_keyboard_hook() {
                crate::logutil::warn(&format!("kb hook failed: {e}"));
            }
        });

    let _ = thread::Builder::new()
        .name("deskcap-kb-poll".into())
        .spawn(|| {
            crate::logutil::info(
                "kb poll fallback up (covers Cursor/Chrome when LL hook goes deaf)",
            );
            run_key_poller();
        });

    let _ = thread::Builder::new()
        .name("deskcap-raw-mouse".into())
        .spawn(|| {
            crate::logutil::info("Raw Input mouse up");
            if let Err(e) = run_raw_mouse() {
                crate::logutil::warn(&format!("raw mouse failed: {e}"));
            }
        });

    // Same idea as kb poll: Raw Input can miss clicks; GetAsyncKeyState does not.
    let _ = thread::Builder::new()
        .name("deskcap-mouse-poll".into())
        .spawn(|| {
            crate::logutil::info("mouse button poll fallback up");
            run_mouse_poller();
        });
}

fn enqueue(job: Job) {
    if let Some(tx) = JOB_TX.lock().as_ref() {
        if tx.try_send(job).is_err() {
            crate::logutil::lag_ms("input_queue_drop", 8);
        }
    }
}

fn handle_key(
    engine: &Arc<CaptureEngine>,
    vk: u32,
    scan: u32,
    flags: u32,
    text_hint: Option<String>,
) {
    let ctrl = unsafe { GetAsyncKeyState(0x11) as u16 & 0x8000 != 0 };
    let alt = unsafe { GetAsyncKeyState(0x12) as u16 & 0x8000 != 0 };
    if ctrl && !alt {
        let action = match vk {
            0x56 => Some("paste"),
            0x43 => Some("copy"),
            0x58 => Some("cut"),
            _ => None,
        };
        if let Some(action) = action {
            if engine.config.read().capture_clipboard {
                engine.on_clipboard_shortcut(action);
            }
            return;
        }
        return;
    }
    if ctrl || alt {
        return;
    }
    match vk {
        0x08 => engine.on_key_input(51, Some("\u{8}".into())),
        0x0D => engine.on_key_input(36, Some("\n".into())),
        0x09 => engine.on_key_input(48, Some("\t".into())),
        _ => {
            let text = text_hint
                .filter(|s| !s.is_empty())
                .or_else(|| vk_latin(vk))
                .or_else(|| to_unicode(vk, scan, flags));
            if let Some(text) = text {
                engine.on_key_input(0, Some(text));
            }
        }
    }
}

fn vk_latin(vk: u32) -> Option<String> {
    unsafe {
        let shift = GetAsyncKeyState(0x10) as u16 & 0x8000 != 0;
        let caps = GetKeyState(0x14) & 1 != 0;
        let upper = shift ^ caps;
        let ch: Option<char> = match vk {
            0x41..=0x5A => {
                let b = (vk - 0x41) as u8;
                Some(if upper {
                    (b'A' + b) as char
                } else {
                    (b'a' + b) as char
                })
            }
            0x30..=0x39 => {
                if !shift {
                    Some((b'0' + (vk - 0x30) as u8) as char)
                } else {
                    Some(match vk {
                        0x30 => ')',
                        0x31 => '!',
                        0x32 => '@',
                        0x33 => '#',
                        0x34 => '$',
                        0x35 => '%',
                        0x36 => '^',
                        0x37 => '&',
                        0x38 => '*',
                        0x39 => '(',
                        _ => return None,
                    })
                }
            }
            0x20 => Some(' '),
            0xBA => Some(if shift { ':' } else { ';' }),
            0xBB => Some(if shift { '+' } else { '=' }),
            0xBC => Some(if shift { '<' } else { ',' }),
            0xBD => Some(if shift { '_' } else { '-' }),
            0xBE => Some(if shift { '>' } else { '.' }),
            0xBF => Some(if shift { '?' } else { '/' }),
            0xC0 => Some(if shift { '~' } else { '`' }),
            0xDB => Some(if shift { '{' } else { '[' }),
            0xDC => Some(if shift { '|' } else { '\\' }),
            0xDD => Some(if shift { '}' } else { ']' }),
            0xDE => Some(if shift { '"' } else { '\'' }),
            _ => None,
        };
        ch.map(|c| c.to_string())
    }
}

fn to_unicode(vk: u32, scan: u32, flags: u32) -> Option<String> {
    unsafe {
        let mut state = [0u8; 256];
        let _ = GetKeyboardState(state.as_mut_ptr());
        let mut buf = [0u16; 8];
        let mut n = ToUnicode(vk, scan, state.as_ptr(), buf.as_mut_ptr(), 8, flags);
        if n < 0 {
            let _ = ToUnicode(vk, scan, state.as_ptr(), buf.as_mut_ptr(), 8, flags);
            n = ToUnicode(vk, scan, state.as_ptr(), buf.as_mut_ptr(), 8, flags);
        }
        if n <= 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..n as usize]);
        if s.chars().all(|c| c.is_control() && c != '\t' && c != '\n') {
            return None;
        }
        if s == "\u{8}" || s == "\u{7f}" {
            return Some("\u{8}".into());
        }
        Some(s)
    }
}

fn cursor_pos() -> (f64, f64) {
    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }
    let mut p = Point { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut p as *mut Point as *mut _) != 0 {
            return (p.x as f64, p.y as f64);
        }
    }
    (0.0, 0.0)
}

fn resolve_vk(vk: u32, scan: u32) -> u32 {
    if vk == 0xE5 && scan != 0 {
        let mapped = unsafe { MapVirtualKeyW(scan, 1) };
        if mapped != 0 {
            return mapped;
        }
    }
    vk
}

fn run_key_poller() {
    let vks: Vec<u32> = {
        let mut v = vec![0x08u32, 0x09, 0x0D, 0x20];
        v.extend(0x30u32..=0x39);
        v.extend(0x41u32..=0x5A);
        v.extend([
            0xBA, 0xBB, 0xBC, 0xBD, 0xBE, 0xBF, 0xC0, 0xDB, 0xDC, 0xDD, 0xDE,
        ]);
        v
    };
    let mut prev = vec![false; vks.len()];
    loop {
        thread::sleep(Duration::from_millis(10));
        for (i, &vk) in vks.iter().enumerate() {
            let down = unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 };
            if down && !prev[i] {
                KEYDOWN_N.fetch_add(1, Ordering::Relaxed);
                note_input();
                enqueue(Job::Key {
                    vk,
                    scan: unsafe { MapVirtualKeyW(vk, 0) },
                    flags: 0,
                    text: vk_latin(vk),
                    via_poll: true,
                });
            }
            prev[i] = down;
        }
    }
}

/// Left/right/middle click edges via GetAsyncKeyState — no WH_MOUSE_LL.
fn run_mouse_poller() {
    // VK_LBUTTON, VK_RBUTTON, VK_MBUTTON
    let buttons: [(i32, &'static str); 3] = [(0x01, "left"), (0x02, "right"), (0x04, "middle")];
    let mut prev = [false; 3];
    let mut last_fire_ms = [0u64; 3];
    loop {
        thread::sleep(Duration::from_millis(8));
        let now = now_ms();
        for (i, &(vk, name)) in buttons.iter().enumerate() {
            let down = unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 };
            if down && !prev[i] {
                // Dedupe against Raw Input (same physical click).
                if now.saturating_sub(last_fire_ms[i]) >= 40 {
                    last_fire_ms[i] = now;
                    MOUSE_N.fetch_add(1, Ordering::Relaxed);
                    note_input();
                    enqueue(Job::Click { button: name });
                }
            }
            prev[i] = down;
        }
    }
}

#[repr(C)]
struct KbdLl {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    time: u32,
    dw_extra_info: usize,
}

static mut KB_HOOK: *mut core::ffi::c_void = ptr::null_mut();

unsafe extern "system" fn kb_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code == 0 {
        if wparam == 0x0100 || wparam == 0x0104 {
            let kb = &*(lparam as *const KbdLl);
            let vk = resolve_vk(kb.vk_code, kb.scan_code);
            KEYDOWN_N.fetch_add(1, Ordering::Relaxed);
            note_input();
            enqueue(Job::Key {
                vk,
                scan: kb.scan_code,
                flags: 0,
                text: vk_latin(vk),
                via_poll: false,
            });
        }
    }
    CallNextHookEx(KB_HOOK, code, wparam, lparam)
}

fn run_keyboard_hook() -> Result<(), String> {
    unsafe {
        let hook = SetWindowsHookExW(13, Some(kb_proc), ptr::null_mut(), 0);
        if hook.is_null() {
            return Err(format!("SetWindowsHookExW kb err={}", GetLastError()));
        }
        KB_HOOK = hook;
        let mut msg: [u8; 48] = zeroed();
        while GetMessageW(msg.as_mut_ptr() as *mut _, ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(msg.as_ptr() as *const _);
            let _ = DispatchMessageW(msg.as_ptr() as *const _);
        }
        let _ = UnhookWindowsHookEx(hook);
    }
    Ok(())
}


// —— Raw Input mouse buttons (no move stream in our handler path that blocks OS) ——

#[repr(C)]
struct RawInputDevice {
    us_usage_page: u16,
    us_usage: u16,
    dw_flags: u32,
    hwnd_target: *mut core::ffi::c_void,
}

#[repr(C)]
struct RaWinputHeader {
    dw_type: u32,
    dw_size: u32,
    h_device: *mut core::ffi::c_void,
    w_param: usize,
}

fn run_raw_mouse() -> Result<(), String> {
    unsafe {
        let class: Vec<u16> = "DeskCapRawMouse\0".encode_utf16().collect();
        let mut wc: WndClassW = zeroed();
        wc.lpfn_wnd_proc = Some(raw_wnd_proc);
        wc.lpsz_class_name = class.as_ptr();
        wc.h_instance = GetModuleHandleW(ptr::null());
        if RegisterClassW(&wc) == 0 {
            let err = GetLastError();
            if err != 1410 {
                // already registered
                return Err(format!("RegisterClassW {err}"));
            }
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            ptr::null_mut(),
            GetModuleHandleW(ptr::null()),
            ptr::null_mut(),
        );
        if hwnd.is_null() {
            return Err(format!("CreateWindowExW {}", GetLastError()));
        }
        let mut rid = RawInputDevice {
            us_usage_page: 0x01,
            us_usage: 0x02, // mouse
            dw_flags: 0x00000100, // RIDEV_INPUTSINK
            hwnd_target: hwnd,
        };
        if RegisterRawInputDevices(&mut rid, 1, size_of::<RawInputDevice>() as u32) == 0 {
            return Err(format!("RegisterRawInputDevices {}", GetLastError()));
        }
        let mut msg: [u8; 48] = zeroed();
        while GetMessageW(msg.as_mut_ptr() as *mut _, ptr::null_mut(), 0, 0) > 0 {
            let _ = TranslateMessage(msg.as_ptr() as *const _);
            let _ = DispatchMessageW(msg.as_ptr() as *const _);
        }
    }
    Ok(())
}

unsafe extern "system" fn raw_wnd_proc(
    hwnd: *mut core::ffi::c_void,
    msg: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    const WM_INPUT: u32 = 0x00FF;
    if msg == WM_INPUT {
        let mut size: u32 = 0;
        GetRawInputData(
            lparam as *mut _,
            0x10000003, // RID_INPUT
            ptr::null_mut(),
            &mut size,
            size_of::<RaWinputHeader>() as u32,
        );
        if size > 0 && size < 4096 {
            let mut buf = vec![0u8; size as usize];
            let got = GetRawInputData(
                lparam as *mut _,
                0x10000003,
                buf.as_mut_ptr() as *mut _,
                &mut size,
                size_of::<RaWinputHeader>() as u32,
            );
            if got != u32::MAX && got >= size_of::<RaWinputHeader>() as u32 + 24 {
                // RAWMOUSE after header: usFlags(2) ulButtons/usButtonFlags(4) …
                // Layout: header then usFlags:u16, padding?, usButtonFlags:u16, usButtonData:u16, ulRawButtons:u32, lLastX:i32, lLastY:i32
                let base = size_of::<RaWinputHeader>();
                // usButtonFlags at offset +4 from RAWMOUSE start on x64 (usFlags u16 + align u16)
                let btn_flags = u16::from_le_bytes([buf[base + 4], buf[base + 5]]);
                let button = if btn_flags & 0x0001 != 0 {
                    Some("left") // RI_MOUSE_LEFT_BUTTON_DOWN
                } else if btn_flags & 0x0004 != 0 {
                    Some("right")
                } else if btn_flags & 0x0010 != 0 {
                    Some("middle")
                } else {
                    None
                };
                if let Some(button) = button {
                    let now = now_ms();
                    let prev = LAST_RAW_CLICK_MS.load(Ordering::Relaxed);
                    if now.saturating_sub(prev) < 40 {
                        return 0;
                    }
                    LAST_RAW_CLICK_MS.store(now, Ordering::Relaxed);
                    MOUSE_N.fetch_add(1, Ordering::Relaxed);
                    note_input();
                    enqueue(Job::Click { button });
                }
            }
        }
        return 0;
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

#[repr(C)]
struct WndClassW {
    style: u32,
    lpfn_wnd_proc: Option<
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, usize, isize) -> isize,
    >,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: *mut core::ffi::c_void,
    h_icon: *mut core::ffi::c_void,
    h_cursor: *mut core::ffi::c_void,
    h_br_background: *mut core::ffi::c_void,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
}

const HWND_MESSAGE: *mut core::ffi::c_void = -3isize as *mut _;

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(
        id_hook: i32,
        lpfn: Option<unsafe extern "system" fn(i32, usize, isize) -> isize>,
        hmod: *mut core::ffi::c_void,
        dw_thread_id: u32,
    ) -> *mut core::ffi::c_void;
    fn CallNextHookEx(
        hhk: *mut core::ffi::c_void,
        n_code: i32,
        wparam: usize,
        lparam: isize,
    ) -> isize;
    fn UnhookWindowsHookEx(hhk: *mut core::ffi::c_void) -> i32;
    fn GetMessageW(
        lp_msg: *mut core::ffi::c_void,
        hwnd: *mut core::ffi::c_void,
        min: u32,
        max: u32,
    ) -> i32;
    fn TranslateMessage(lp_msg: *const core::ffi::c_void) -> i32;
    fn DispatchMessageW(lp_msg: *const core::ffi::c_void) -> isize;
    fn GetAsyncKeyState(v_key: i32) -> i16;
    fn GetKeyState(n_virt_key: i32) -> i16;
    fn GetKeyboardState(lp_key_state: *mut u8) -> i32;
    fn ToUnicode(
        w_virt_key: u32,
        w_scan_code: u32,
        lp_key_state: *const u8,
        pwsz_buff: *mut u16,
        cch_buff: i32,
        w_flags: u32,
    ) -> i32;
    fn GetCursorPos(lp_point: *mut core::ffi::c_void) -> i32;
    fn MapVirtualKeyW(u_code: u32, u_map_type: u32) -> u32;
    fn RegisterClassW(lp_wnd_class: *const WndClassW) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: *mut core::ffi::c_void,
        menu: *mut core::ffi::c_void,
        instance: *mut core::ffi::c_void,
        param: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;
    fn RegisterRawInputDevices(
        p_raw_input_devices: *mut RawInputDevice,
        ui_num_devices: u32,
        cb_size: u32,
    ) -> i32;
    fn GetRawInputData(
        h_raw_input: *mut core::ffi::c_void,
        ui_command: u32,
        p_data: *mut core::ffi::c_void,
        pcb_size: *mut u32,
        cb_size_header: u32,
    ) -> u32;
    fn DefWindowProcW(
        hwnd: *mut core::ffi::c_void,
        msg: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
    fn GetModuleHandleW(lp_module_name: *const u16) -> *mut core::ffi::c_void;
}
