use crate::daybook;
use crate::config::CaptureConfig;
use crate::event::{launched_from_applications, platform_name, running_exe_path, AppContext, CaptureEvent, SessionStatus};
use crate::maintenance::{self, MaintenanceReport};
use crate::typing::{is_printable_name, TypingAggregator};
use crate::writer::SegmentWriter;
use active_win_pos_rs::get_active_window;
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use xcap::Monitor;

const RECENT_CAP: usize = 80;

/// Coalesce same-button clicks at the same pixel within this window. Catches
/// jitter / stuck-button / buggy-mouse noise; well under any human
/// double/triple-click (physical floor ~50-100 ms, system multi-click ~500 ms).
const CLICK_DEDUP_WINDOW: Duration = Duration::from_millis(100);

#[derive(Clone, Copy)]
struct LastClick {
    x: f64,
    y: f64,
    button: u8, // 0=left, 1=right, 2=middle, 3=other — stable int keys
    at: Instant,
}

fn click_button_id(button: &str) -> u8 {
    match button {
        "left" => 0,
        "right" => 1,
        "middle" => 2,
        _ => 3,
    }
}

/// Pure dedup predicate — testable in isolation.
fn should_drop_click(prev: LastClick, button: &str, x: f64, y: f64, now: Instant) -> bool {
    prev.button == click_button_id(button)
        && prev.x == x
        && prev.y == y
        && now.duration_since(prev.at) < CLICK_DEDUP_WINDOW
}

pub struct CaptureEngine {
    /// Serialize typing / flush / emit / focus side-effects — prevents lock-order deadlocks
    /// between EventTap worker, AX poller, window poller, and idle flush.
    pipeline: Mutex<()>,
    pub config: RwLock<CaptureConfig>,
    config_path: PathBuf,
    running: AtomicBool,
    bootstrapped: AtomicBool,
    session_id: RwLock<Option<String>>,
    started: RwLock<Option<Instant>>,
    started_wall: RwLock<Option<f64>>,
    event_count: RwLock<u64>,
    segment_path: RwLock<Option<String>>,
    current_app: RwLock<Option<AppContext>>,
    excluded_now: AtomicBool,
    recent: RwLock<VecDeque<CaptureEvent>>,
    last_mouse: RwLock<(f64, f64)>,
    /// Dedup window for `on_mouse_click` — skip identical consecutive clicks.
    last_click: Mutex<Option<LastClick>>,
    /// How many clicks were suppressed by the dedup window (diagnostic).
    mouse_click_dedup: AtomicU64,
    typing: Mutex<TypingAggregator>,
    /// EventTap Latin held for chat apps until CJK commit (AX / unicode) arrives.
    ime_staging: Mutex<String>,
    /// Last key into `ime_staging` (idle flush uses this, not typing.last_activity).
    ime_staging_at: Mutex<Option<Instant>>,
    /// Last emitted typing text (trimmed) — strip prefix / drop exact dupes so Win UIA
    /// cannot re-flush a whole composer after idle segments (Mac-style incremental).
    last_emitted_typing: Mutex<Option<String>>,
    active_writer: Mutex<Option<Arc<Mutex<SegmentWriter>>>>,
    last_maintenance: Mutex<Option<MaintenanceReport>>,
    bg_handles: Mutex<Vec<JoinHandle<()>>>,
    keydown_n: AtomicU64,
    mouse_n: AtomicU64,
    last_input_unix_ms: AtomicU64,
}

impl CaptureEngine {
    pub fn new() -> Arc<Self> {
        let config_path = CaptureConfig::config_path();
        let mut config = CaptureConfig::load_or_default(&config_path);
        config.capture_mouse_move = false;
        let _ = config.save(&config_path);
        Arc::new(Self {
            pipeline: Mutex::new(()),
            config: RwLock::new(config),
            config_path,
            running: AtomicBool::new(false),
            bootstrapped: AtomicBool::new(false),
            session_id: RwLock::new(None),
            started: RwLock::new(None),
            started_wall: RwLock::new(None),
            event_count: RwLock::new(0),
            segment_path: RwLock::new(None),
            current_app: RwLock::new(None),
            excluded_now: AtomicBool::new(false),
            recent: RwLock::new(VecDeque::with_capacity(RECENT_CAP)),
            last_mouse: RwLock::new((0.0, 0.0)),
            last_click: Mutex::new(None),
            mouse_click_dedup: AtomicU64::new(0),
            typing: Mutex::new(TypingAggregator::default()),
            ime_staging: Mutex::new(String::new()),
            ime_staging_at: Mutex::new(None),
            last_emitted_typing: Mutex::new(None),
            active_writer: Mutex::new(None),
            last_maintenance: Mutex::new(None),
            bg_handles: Mutex::new(Vec::new()),
            keydown_n: AtomicU64::new(0),
            mouse_n: AtomicU64::new(0),
            last_input_unix_ms: AtomicU64::new(0),
        })
    }

    /// Start long-lived workers once (input tap + window poll + maintenance).
    pub fn bootstrap(self: &Arc<Self>) {
        if self.bootstrapped.swap(true, Ordering::SeqCst) {
            return;
        }

        crate::logutil::init(&self.config.read().data_dir);
        crate::logutil::info(&format!(
            "bootstrap platform={} capture_kb={} clipboard={}",
            crate::event::platform_name(),
            self.config.read().capture_keyboard,
            self.config.read().capture_clipboard
        ));

        #[cfg(target_os = "macos")]
        {
            crate::input_macos::request_input_permissions();
            crate::input_macos::prompt_accessibility();
            crate::input_macos::spawn_listener(Arc::clone(self));
            crate::ax_text::spawn_ax_text_poller(Arc::clone(self));
            crate::ime_keepalive::spawn_ime_keepalive();
            // Keep DeskCapIME warm so switching is not a 5s cold start.
            // Kill old process first — a stale binary keeps old candidate-placement bugs.
            let ime_bin = "/Library/Input Methods/DeskCapIME.app/Contents/MacOS/DeskCapIME";
            if std::path::Path::new(ime_bin).exists() {
                let _ = std::process::Command::new("/usr/bin/killall")
                    .arg("DeskCapIME")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
                std::thread::sleep(std::time::Duration::from_millis(250));
                let _ = std::process::Command::new(ime_bin)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                crate::logutil::info("ime prewarm spawned");
            }
        }
        #[cfg(windows)]
        {
            crate::input_windows::spawn_listener(Arc::clone(self));
            crate::uia_text::spawn_uia_text_poller(Arc::clone(self));
        }
        #[cfg(all(not(target_os = "macos"), not(windows)))]
        {
            self.spawn_rdev_listener();
        }

        // Heartbeat so silent logs prove the process is alive (Win dogfood).
        {
            let engine = Arc::clone(self);
            let _ = thread::Builder::new()
                .name("deskcap-heartbeat".into())
                .spawn(move || loop {
                    thread::sleep(Duration::from_secs(30));
                    crate::logutil::info(&format!(
                        "heartbeat capturing={} events={}{}",
                        engine.is_capturing(),
                        *engine.event_count.read(),
                        {
                            #[cfg(windows)]
                            {
                                format!(
                                    " kb_poll={} mouse={} keydown={} age={:?}",
                                    crate::input_windows::key_via_poll(),
                                    crate::input_windows::mouse_count(),
                                    crate::input_windows::keydown_count(),
                                    crate::input_windows::last_input_age_secs()
                                )
                            }
                            #[cfg(not(windows))]
                            {
                                String::new()
                            }
                        }
                    ));
                });
        }

        // Foreground window poller (low rate).
        {
            let engine = Arc::clone(self);
            let h = thread::Builder::new()
                .name("deskcap-window".into())
                .spawn(move || loop {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        engine.poll_window();
                    }));
                    #[cfg(windows)]
                    let sleep_ms = 100u64;
                    #[cfg(not(windows))]
                    let sleep_ms = 250u64;
                    thread::sleep(Duration::from_millis(sleep_ms));
                });
            if let Ok(h) = h {
                self.bg_handles.lock().push(h);
            }
        }

        // Typing idle flush + segment rotate while capturing.
        {
            let engine = Arc::clone(self);
            let h = thread::Builder::new()
                .name("deskcap-flush".into())
                .spawn(move || loop {
                    thread::sleep(Duration::from_millis(500));
                    if !engine.is_capturing() {
                        continue;
                    }
                    // Clone under a short block — never hold `active_writer` across
                    // `pipeline` (Rust extends `if let` temporaries into the body → deadlock).
                    let w = { engine.active_writer.lock().clone() };
                    let Some(w) = w else { continue };
                    engine.flush_typing_idle(&w);
                    let max = engine.config.read().segment_duration_secs;
                    {
                        let rotated = w.lock().rotate_if_needed(max);
                        if let Ok(true) = rotated {
                            *engine.segment_path.write() =
                                Some(w.lock().path().display().to_string());
                        }
                        let _ = w.lock().flush();
                    }
                    crate::daybook::flush();
                });
            if let Ok(h) = h {
                self.bg_handles.lock().push(h);
            }
        }

        // Periodic maintenance (cleanup + backup), every 30 minutes.
        {
            let engine = Arc::clone(self);
            let h = thread::Builder::new()
                .name("deskcap-maint".into())
                .spawn(move || {
                    // Run once shortly after start.
                    thread::sleep(Duration::from_secs(5));
                    loop {
                        let cfg = engine.config.read().clone();
                        let report = maintenance::run_maintenance(&cfg);
                        *engine.last_maintenance.lock() = Some(report);
                        thread::sleep(Duration::from_secs(30 * 60));
                    }
                });
            if let Ok(h) = h {
                self.bg_handles.lock().push(h);
            }
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn current_app_name(&self) -> Option<String> {
        self.current_app.read().as_ref().map(|a| a.name.clone())
    }

    pub fn set_mouse_pos(&self, x: f64, y: f64) {
        *self.last_mouse.write() = (x, y);
    }

    /// Refresh exclusion + schedule; return true if capture is allowed.
    /// Heavy: calls get_active_window — **never** invoke from OS input-hook threads.
    pub fn gate_input(&self) -> bool {
        self.refresh_focus();
        if !self.config.read().within_schedule() {
            return false;
        }
        !self.excluded_now.load(Ordering::SeqCst)
    }

    /// Hook/worker-safe gate: use window-poller caches only (no get_active_window).
    /// Per-key get_active_window on Win Electron (Cursor) is slow/flaky and starved capture.
    pub fn gate_input_fast(&self) -> bool {
        if !self.config.read().within_schedule() {
            return false;
        }
        !self.excluded_now.load(Ordering::SeqCst)
    }

    /// Clipboard shortcut: `paste` reads now; `copy`/`cut` delay so OS can update pasteboard.
    pub fn on_clipboard_shortcut(self: &Arc<Self>, action: &str) {
        if !self.config.read().capture_clipboard {
            return;
        }
        if !self.gate_input() {
            return;
        }
        match action {
            "paste" => self.emit_clipboard("clipboard_paste", "paste"),
            "copy" => Self::spawn_clipboard_read(Arc::clone(self), "clipboard_copy", "copy"),
            "cut" => Self::spawn_clipboard_read(Arc::clone(self), "clipboard_cut", "cut"),
            _ => {}
        }
    }

    fn spawn_clipboard_read(engine: Arc<Self>, event_type: &'static str, action: &'static str) {
        let _ = thread::Builder::new()
            .name("deskcap-clipboard".into())
            .spawn(move || {
                // Never let clipboard/ObjC failures kill the process (see Sep 10 SIGABRT).
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    thread::sleep(Duration::from_millis(90));
                    if !engine.is_capturing() || !engine.config.read().capture_clipboard {
                        return;
                    }
                    if !engine.gate_input() {
                        return;
                    }
                    engine.emit_clipboard(event_type, action);
                }));
            });
    }

    pub fn emit_clipboard(&self, event_type: &str, action: &str) {
        if !self.config.read().capture_clipboard {
            return;
        }
        // Pasteboard I/O must never run under `pipeline` (can block / nest locks).
        let Some((text, truncated)) = crate::clipboard::read_plain_text() else {
            return;
        };
        let w = { self.active_writer.lock().clone() };
        let Some(w) = w else { return };
        let (typing, app, clip) = {
            let _pipe = self.pipeline.lock();
            let typing = self.take_typing_segment();
            let app = self.current_app.read().clone();
            let char_count = text.chars().count();
            let clip = json!({
                "action": action,
                "text": text,
                "char_count": char_count,
                "truncated": truncated,
            });
            (typing, app, clip)
        };
        if let Some(seg) = typing {
            self.emit_typing_segment(&w, seg);
        }
        self.emit_with_app(&w, event_type, clip, app);
    }

    pub fn on_mouse_click(&self, button: &str) {
        if !self.config.read().capture_mouse {
            return;
        }
        // Coalesce jitter: same button at same pixel within CLICK_DEDUP_WINDOW is noise.
        // Linux rdev path arrives without coordinates; falls back to button-only check.
        let (mx, my) = *self.last_mouse.read();
        {
            let now = Instant::now();
            let mut last = self.last_click.lock();
            if let Some(prev) = *last {
                if should_drop_click(prev, button, mx, my, now) {
                    self.mouse_click_dedup.fetch_add(1, Ordering::Relaxed);
                    return;
                }
            }
            *last = Some(LastClick {
                x: mx,
                y: my,
                button: click_button_id(button),
                at: now,
            });
        }
        let w = { self.active_writer.lock().clone() };
        let Some(w) = w else { return };
        #[cfg(target_os = "macos")]
        let pre_upgrade = {
            let name = self.current_app_name().unwrap_or_default();
            if crate::ime::is_chat_app_name(&name) {
                crate::wechat_mac::recover_chinese_before_send()
            } else {
                None
            }
        };
        #[cfg(not(target_os = "macos"))]
        let pre_upgrade: Option<(String, &'static str)> = None;

        let (typing, click, app) = {
            let _pipe = self.pipeline.lock();
            let name = self.current_app_name().unwrap_or_default();
            if crate::ime::is_chat_app_name(&name) {
                #[cfg(target_os = "macos")]
                crate::ax_text::request_force_poll();
                #[cfg(windows)]
                crate::uia_text::request_force_poll();
                self.promote_ime_staging();
            }
            let typing = self.take_typing_segment();
            let app = self.current_app.read().clone();
            let click = json!({ "button": button, "pressed": true, "app": app });
            (typing, click, app)
        };
        if let Some(mut seg) = typing {
            if let Some((text, tag)) = pre_upgrade {
                if crate::ime::has_cjk(&text) {
                    seg.text = text;
                    let mut data = seg.to_data();
                    if let Some(obj) = data.as_object_mut() {
                        obj.insert("capture".into(), json!(tag));
                    }
                    self.emit_with_app(&w, "typing", data, seg.app);
                    self.emit_with_app(&w, "mouse_click", click, app);
                    return;
                }
            }
            self.emit_typing_segment(&w, seg);
        }
        self.emit_with_app(&w, "mouse_click", click, app);
    }

    pub fn clear_ime_staging(&self) {
        self.ime_staging.lock().clear();
        *self.ime_staging_at.lock() = None;
    }

    fn touch_ime_staging(&self) {
        *self.ime_staging_at.lock() = Some(Instant::now());
    }

    /// Move staged EventTap Latin into the typing buffer (pinyin fallback for 微信/企微).
    /// Caller must already hold `pipeline`.
    fn promote_ime_staging(&self) {
        let staged = {
            let mut g = self.ime_staging.lock();
            if g.is_empty() {
                return;
            }
            std::mem::take(&mut *g)
        };
        *self.ime_staging_at.lock() = None;
        let ts = self.mono_ts();
        let wall = wall_ts();
        let app = self.current_app.read().clone();
        let mut typing = self.typing.lock();
        if typing.text_ref().chars().any(|c| {
            matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}')
        }) {
            return;
        }
        typing.clear();
        typing.push_text(&staged, ts, wall, app);
    }

    /// Drop EventTap pinyin when AX/UIA delivers committed CJK. Caller holds `pipeline`.
    pub fn clear_typing_if_ime_raw(&self) {
        let mut typing = self.typing.lock();
        if looks_like_ime_raw(typing.text_ref())
            || typing
                .text_ref()
                .chars()
                .all(|c| c.is_ascii_whitespace() || c == ',' || c == '.' || c == '\'')
        {
            typing.clear();
        }
    }

    /// 当前打字缓冲的快照（UIA 路径用来判断「刚才是拼音还是汉字」）。
    ///
    /// 2026-10-20 补：`uia_text.rs` 一直在调它，但方法从未实现 ——
    /// **Windows 端根本编译不过**（`error[E0599]`）。macOS 构建不编
    /// `uia_text.rs`，所以本地一直没暴露；CI 首跑 Windows 才抓到。
    #[cfg(windows)]
    pub fn typing_buffer_snapshot(&self) -> String {
        self.typing.lock().text_ref().to_string()
    }

    /// Drop pure-Latin / pinyin from the open typing buffer (UIA will LCP in 汉字).
    pub fn clear_latin_typing_buffer(&self) {        let mut typing = self.typing.lock();
        let t = typing.text_ref();
        if t.is_empty() {
            return;
        }
        if crate::ime::has_cjk(t) {
            return;
        }
        if looks_like_ime_raw(t)
            || t.chars()
                .all(|c| c.is_ascii() && (c.is_alphanumeric() || c.is_whitespace() || c == '\''))
        {
            typing.clear();
        }
    }

    /// AX/UIA composer diff — one pipeline acquisition; buffer only (idle flush writes).
    pub fn apply_composer_edit(&self, prev: &str, next: &str) {
        if prev == next {
            return;
        }
        // Whole-field AX/UIA chrome (Cursor button, settings crumbs) — never buffer.
        if crate::ime::is_agent_noise_text(next)
            && (prev.is_empty() || crate::ime::is_agent_noise_text(prev))
        {
            return;
        }
        let _pipe = self.pipeline.lock();
        if !self.config.read().capture_keyboard {
            return;
        }
        let app_name = self.current_app_name().unwrap_or_default();
        let chat = crate::ime::is_chat_app_name(&app_name);

        // WeChat/企微: on-screen composer is ground truth — replace whole buffer.
        // LCP diffs break when the user deletes mid-string (情爱→亲爱的料子妈).
        if chat && crate::ime::has_cjk(next) {
            self.clear_ime_staging();
            let ts = self.mono_ts();
            let wall = wall_ts();
            let app = self.current_app.read().clone();
            self.typing.lock().replace_all(next, ts, wall, app);
            return;
        }

        if crate::ime::has_cjk(next) {
            self.clear_ime_staging();
            self.clear_typing_if_ime_raw();
        }
        let common = crate::ime::char_lcp_len(prev, next);
        let remove = prev.chars().count().saturating_sub(common);
        let add: String = next.chars().skip(common).take(800).collect();
        if crate::ime::is_agent_noise_text(&add) && prev.is_empty() {
            return;
        }
        for _ in 0..remove.min(800) {
            let _ = self.on_key_input_inner(51, Some("\u{8}".into()), false);
        }
        if !add.is_empty() {
            let _ = self.on_key_input_inner(0, Some(add), false);
        }
    }

    pub fn on_key_input(&self, keycode: i64, text: Option<String>) {
        #[cfg(target_os = "macos")]
        let pre_upgrade = {
            let is_enter = text.as_deref() == Some("\n") || keycode == 36 || keycode == 76;
            let chat = self
                .current_app_name()
                .map(|n| crate::ime::is_chat_app_name(&n))
                .unwrap_or(false);
            if is_enter && chat {
                // OCR/AX while composer still shows 汉字 — before Enter clears it.
                crate::wechat_mac::recover_chinese_before_send()
            } else {
                None
            }
        };
        #[cfg(not(target_os = "macos"))]
        let pre_upgrade: Option<(String, &'static str)> = None;

        let flush = {
            let _pipe = self.pipeline.lock();
            self.on_key_input_inner(keycode, text, true)
        };
        if let Some((w, mut seg)) = flush {
            if let Some((text, tag)) = pre_upgrade {
                if crate::ime::has_cjk(&text) {
                    seg.text = text;
                    let mut data = seg.to_data();
                    if let Some(obj) = data.as_object_mut() {
                        obj.insert("capture".into(), json!(tag));
                    }
                    self.emit_with_app(&w, "typing", data, seg.app);
                    return;
                }
            }
            self.emit_typing_segment(&w, seg);
        }
    }

    /// Chat apps (WeChat…): upgrade pinyin buffer to on-screen 汉字 without changing IME.
    /// Returns capture tag when upgraded.
    fn upgrade_chat_typing_from_ax(
        &self,
        seg: &mut crate::typing::TypingSegment,
    ) -> Option<&'static str> {
        #[cfg(target_os = "macos")]
        {
            let name = seg.app.as_ref().map(|a| a.name.as_str()).unwrap_or("");
            if !crate::ime::is_chat_app_name(name) {
                return None;
            }
            if crate::ime::has_cjk(&seg.text) {
                return None;
            }
            let looks_pinyin = crate::typing::looks_like_chat_pinyin_buffer(&seg.text)
                || seg
                    .text
                    .chars()
                    .filter(|c| c.is_ascii_alphabetic())
                    .count()
                    >= 2;
            if !looks_pinyin {
                return None;
            }
            if let Some((text, tag)) = crate::wechat_mac::recover_chinese_on_send() {
                seg.text = text;
                return Some(tag);
            }
            None
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = seg;
            None
        }
    }

    /// Returns a typing segment to emit when Enter flushes. Caller holds `pipeline`.
    fn on_key_input_inner(
        &self,
        keycode: i64,
        text: Option<String>,
        allow_ime_stage: bool,
    ) -> Option<(Arc<Mutex<SegmentWriter>>, crate::typing::TypingSegment)> {
        if !self.config.read().capture_keyboard {
            return None;
        }
        let app = self.current_app.read().clone();
        let app_name = app.as_ref().map(|a| a.name.as_str()).unwrap_or("");
        let chat = crate::ime::is_chat_app_name(app_name);
        let ts = self.mono_ts();
        let wall = wall_ts();

        if keycode == 51 || text.as_deref() == Some("\u{8}") {
            let composing = allow_ime_stage && !self.ime_staging.lock().is_empty();
            if composing {
                self.ime_staging.lock().pop();
                if self.ime_staging.lock().is_empty() {
                    *self.ime_staging_at.lock() = None;
                } else {
                    self.touch_ime_staging();
                }
            }
            {
                let mut typing = self.typing.lock();
                // Count real key presses (allow_ime_stage); AX/UIA LCP sync uses false.
                if allow_ime_stage {
                    typing.note_backspace_key();
                }
                if allow_ime_stage && chat && !composing && crate::typing::looks_like_chat_pinyin_buffer(typing.text_ref())
                {
                    // Confirmed pinyin chunks: one BS ≈ one on-screen 字 → drop last '… chunk.
                    typing.backspace_pinyin_unit();
                } else {
                    typing.backspace();
                }
            }
            // Always nudge UIA/AX — backspace during pinyin must not freeze stale composer prev.
            #[cfg(target_os = "macos")]
            crate::ax_text::request_force_poll();
            #[cfg(windows)]
            crate::uia_text::request_force_poll();
            return None;
        }
        if text.as_deref() == Some("\n") || keycode == 36 || keycode == 76 {
            // Brace so MutexGuard is not held across the rest of the pipeline section.
            let w = { self.active_writer.lock().clone() }?;
            self.promote_ime_staging();
            self.typing.lock().push_text("\n", ts, wall, app);
            let seg = self.take_typing_segment()?;
            return Some((w, seg));
        }
        if text.as_deref() == Some("\t") || keycode == 48 {
            self.typing.lock().push_text("\t", ts, wall, app);
            return None;
        }
        // WeChat/企微: Space / 1–9 = IME candidate confirm while composing.
        // Keep Latin already typed (backspaces dropped typos); do NOT append the
        // confirm key (avoids `changshi2`). Insert `'` for Agent syllable cuts.
        // Space with empty staging → fall through (English space in chat).
        if allow_ime_stage && chat && is_ime_candidate_confirm(text.as_deref(), keycode) {
            let composing = !self.ime_staging.lock().is_empty();
            if composing {
                self.on_ime_candidate_confirm(ts, wall, app.clone());
                return None;
            }
            if is_ime_digit_confirm(text.as_deref()) {
                // Stray candidate digit with no composition — never store as content.
                return None;
            }
        }
        if let Some(t) = text {
            if !is_printable_name(&t) {
                return None;
            }
            let has_cjk = t.chars().any(|c| {
                matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}')
            });
            let ime_raw = looks_like_ime_raw(&t);

            if allow_ime_stage && ime_raw && !has_cjk {
                // DeskCapIME owns commits via /ime/commit — do not also stash EventTap pinyin.
                #[cfg(target_os = "macos")]
                if crate::ime::deskcap_ime_active() {
                    return None;
                }
                // Win: when UIA is already reading CJK composers, do not also stash pinyin
                // into typing (Mac EventTap defers to AX the same way).
                #[cfg(windows)]
                if crate::uia_text::uia_defers_latin() {
                    crate::uia_text::request_force_poll();
                    return None;
                }
                self.ime_staging.lock().push_str(&t);
                self.touch_ime_staging();
                self.typing.lock().push_text(&t, ts, wall, app);
                #[cfg(target_os = "macos")]
                crate::ax_text::request_force_poll();
                #[cfg(windows)]
                crate::uia_text::request_force_poll();
                return None;
            }
            if has_cjk {
                self.clear_ime_staging();
                let mut typing = self.typing.lock();
                if looks_like_ime_raw(typing.text_ref()) {
                    typing.clear();
                }
                drop(typing);
            }
            self.typing.lock().push_text(&t, ts, wall, app);
            #[cfg(target_os = "macos")]
            crate::ax_text::request_force_poll();
            #[cfg(windows)]
            crate::uia_text::request_force_poll();
        }
        None
    }

    /// Space / digit confirmed a candidate: seal current pinyin chunk, start the next.
    fn on_ime_candidate_confirm(
        &self,
        _ts: f64,
        _wall: f64,
        _app: Option<crate::event::AppContext>,
    ) {
        let outcome = {
            let mut typing = self.typing.lock();
            typing.seal_pinyin_chunk()
        };
        match outcome {
            crate::typing::SealOutcome::Sealed | crate::typing::SealOutcome::Empty => {
                self.clear_ime_staging();
            }
            crate::typing::SealOutcome::StillComposing => {
                // Space after `sh`/`d`/… — keep Latin; restore staging so next keys continue
                // the same syllable (do NOT clear — that caused shuode→ode / duima→ima).
                let trailing = {
                    let t = self.typing.lock();
                    let s = t.text_ref();
                    let mut i = s.len();
                    while i > 0 {
                        let b = s.as_bytes()[i - 1];
                        if b.is_ascii_alphabetic() {
                            i -= 1;
                        } else {
                            break;
                        }
                    }
                    s[i..].to_string()
                };
                *self.ime_staging.lock() = trailing;
                self.touch_ime_staging();
            }
        }
        #[cfg(target_os = "macos")]
        crate::ax_text::request_force_poll();
        #[cfg(windows)]
        crate::uia_text::request_force_poll();
    }

    pub fn status(&self) -> SessionStatus {
        let cfg = self.config.read().clone();
        let elapsed = self
            .started
            .read()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let maint = self.last_maintenance.lock().clone();
        SessionStatus {
            state: if self.is_capturing() {
                "running".into()
            } else {
                "idle".into()
            },
            session_id: self.session_id.read().clone(),
            started_at: *self.started_wall.read(),
            elapsed,
            event_count: *self.event_count.read(),
            segment_path: self.segment_path.read().clone(),
            current_app: self.current_app.read().clone(),
            excluded_now: self.excluded_now.load(Ordering::SeqCst),
            platform: platform_name(),
            agent_api: {
                let (lan, _, _) = crate::netinfo::agent_api_urls(cfg.agent_api_port);
                lan
            },
            agent_api_loopback: format!("http://127.0.0.1:{}", cfg.agent_api_port),
            agent_api_lan_ip: crate::netinfo::primary_lan_ipv4(),
            agent_api_bind: "0.0.0.0".into(),
            agent_api_auth: "bearer".into(),
            local_only: true,
            lan_access: true,
            cloud_upload: false,
            capture_keyboard: cfg.capture_keyboard,
            capture_mouse: cfg.capture_mouse,
            capture_mouse_move: false,
            capture_clipboard: cfg.capture_clipboard,
            screenshot_interval_secs: cfg.screenshot_interval_secs,
            retention_days: cfg.retention_days,
            max_disk_mb: cfg.max_disk_mb,
            sessions_bytes: maint.as_ref().map(|m| m.sessions_bytes).unwrap_or(0),
            last_backup: maint.as_ref().and_then(|m| m.backup_path.clone()),
            accessibility_ok: {
                #[cfg(target_os = "macos")]
                {
                    crate::input_macos::is_accessibility_ok_display()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    true
                }
            },
            input_monitoring_ok: {
                #[cfg(target_os = "macos")]
                {
                    crate::input_macos::is_input_monitoring_ok()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    true
                }
            },
            input_listener_ok: {
                #[cfg(target_os = "macos")]
                {
                    crate::input_macos::is_tap_ok()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    true
                }
            },
            within_schedule: cfg.within_schedule(),
            schedule_enabled: cfg.schedule_enabled,
            last_input_age_secs: {
                #[cfg(target_os = "macos")]
                {
                    crate::input_macos::last_input_age_secs()
                }
                #[cfg(windows)]
                {
                    crate::input_windows::last_input_age_secs()
                }
                #[cfg(all(not(target_os = "macos"), not(windows)))]
                {
                    let ms = self.last_input_unix_ms.load(Ordering::Relaxed);
                    if ms == 0 {
                        None
                    } else {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(ms);
                        Some(now.saturating_sub(ms) as f64 / 1000.0)
                    }
                }
            },
            keydown_count: {
                #[cfg(target_os = "macos")]
                {
                    crate::input_macos::keydown_count()
                }
                #[cfg(windows)]
                {
                    crate::input_windows::keydown_count()
                }
                #[cfg(all(not(target_os = "macos"), not(windows)))]
                {
                    self.keydown_n.load(Ordering::Relaxed)
                }
            },
            mouse_click_count: {
                #[cfg(target_os = "macos")]
                {
                    crate::input_macos::mouse_count()
                }
                #[cfg(windows)]
                {
                    crate::input_windows::mouse_count()
                }
                #[cfg(all(not(target_os = "macos"), not(windows)))]
                {
                    self.mouse_n.load(Ordering::Relaxed)
                }
            },
            mouse_click_dedup: self.mouse_click_dedup.load(Ordering::Relaxed),
            ax_chars: {
                #[cfg(target_os = "macos")]
                {
                    crate::ax_text::ax_chars_captured()
                }
                #[cfg(windows)]
                {
                    crate::uia_text::uia_chars_captured()
                }
                #[cfg(all(not(target_os = "macos"), not(windows)))]
                {
                    0
                }
            },
            exe_path: running_exe_path(),
            from_applications: launched_from_applications(),
        }
    }

    pub fn get_config(&self) -> CaptureConfig {
        let mut cfg = self.config.read().clone();
        cfg.capture_mouse_move = false;
        cfg
    }

    pub fn set_config(&self, cfg: CaptureConfig) -> anyhow::Result<CaptureConfig> {
        let mut cfg = cfg.ensure_secure();
        cfg.capture_mouse_move = false;
        cfg.save(&self.config_path)?;
        *self.config.write() = cfg.clone();
        Ok(cfg)
    }

    pub fn recent_events(&self, limit: usize) -> Vec<CaptureEvent> {
        let q = self.recent.read();
        q.iter()
            .rev()
            .filter(|e| Self::is_work_event(e))
            .take(limit.min(RECENT_CAP))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn knowledge(&self, limit: usize) -> Vec<CaptureEvent> {
        let q = self.recent.read();
        q.iter()
            .rev()
            .filter(|e| Self::is_text_knowledge(e) && Self::is_work_event(e))
            .take(limit.min(RECENT_CAP))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }

    pub fn knowledge_day(&self, day: &str, limit: usize) -> Vec<CaptureEvent> {
        self.knowledge_day_since(day, None, limit)
    }

    pub fn knowledge_day_since(
        &self,
        day: &str,
        since: Option<f64>,
        limit: usize,
    ) -> Vec<CaptureEvent> {
        let dir = self.config.read().data_dir.clone();
        // Pull enough raw lines (DeskCap + mouse_click filtered below).
        let fetch = limit.saturating_mul(3).max(limit).min(12000);
        let mut items: Vec<_> = daybook::read_day_page(&dir, day, since, fetch)
            .into_iter()
            .filter(|e| Self::is_text_knowledge(e) && Self::is_work_event(e))
            .collect();
        if since.is_some() {
            items.truncate(limit);
        } else if items.len() > limit {
            // Full-day UI: keep the newest `limit` so the timeline shows recent typing.
            let start = items.len() - limit;
            items.drain(..start);
        }
        items
    }

    pub fn knowledge_day_summary(&self, day: &str) -> serde_json::Value {
        let dir = self.config.read().data_dir.clone();
        daybook::summarize_day(&dir, day)
    }

    fn is_text_knowledge(e: &CaptureEvent) -> bool {
        matches!(
            e.event_type.as_str(),
            "typing" | "clipboard_paste" | "clipboard_copy" | "clipboard_cut"
        )
    }

    fn is_work_event(e: &CaptureEvent) -> bool {
        let name = e.app.as_ref().map(|a| a.name.as_str()).unwrap_or("");
        let path = e
            .app
            .as_ref()
            .and_then(|a| a.path.as_deref())
            .unwrap_or("");
        !name.to_lowercase().contains("deskcap") && !path.to_lowercase().contains("deskcap.app")
    }

    pub fn list_days(&self) -> Vec<String> {
        daybook::list_days(&self.config.read().data_dir)
    }

    pub fn export_day(&self, day: &str) -> anyhow::Result<String> {
        self.export_day_range(day, None, None)
    }

    pub fn export_day_range(
        &self,
        day: &str,
        start_hhmm: Option<String>,
        end_hhmm: Option<String>,
    ) -> anyhow::Result<String> {
        let (path, n) = daybook::export_day_range_counted(
            &self.config.read().data_dir,
            day,
            start_hhmm.as_deref(),
            end_hhmm.as_deref(),
        )?;
        Ok(format!("{n} events → {}", path.display()))
    }

    pub fn import_day(&self, day: &str, content: &str) -> anyhow::Result<usize> {
        daybook::import_day_jsonl(&self.config.read().data_dir, day, content)
    }

    pub fn run_maintenance_now(&self) -> MaintenanceReport {
        let cfg = self.config.read().clone();
        let report = maintenance::run_maintenance(&cfg);
        *self.last_maintenance.lock() = Some(report.clone());
        report
    }

    fn resolve_foreground(&self) -> Option<AppContext> {
        #[cfg(windows)]
        {
            if let Some(fg) = crate::win_focus::foreground() {
                return Some(AppContext {
                    name: fg.name,
                    title: fg.title,
                    path: fg.path,
                });
            }
        }
        get_active_window().ok().map(|win| AppContext {
            name: win.app_name,
            title: win.title,
            path: Some(win.process_path.display().to_string()),
        })
    }

    fn refresh_focus(&self) {
        match self.resolve_foreground() {
            Some(app) => {
                let excluded = self.config.read().is_excluded_path(
                    &app.name,
                    &app.title,
                    app.path.as_deref(),
                );
                self.excluded_now.store(excluded, Ordering::SeqCst);
                *self.current_app.write() = Some(app);
            }
            None => {
                // Fail open when OS cannot resolve focus (common for some Win Electron
                // windows). Leaving excluded=true after DeskCap→Cursor switch drops all keys.
                if self.excluded_now.load(Ordering::SeqCst) {
                    let name = self.current_app_name().unwrap_or_default();
                    if name.is_empty() || name.to_lowercase().contains("deskcap") {
                        self.excluded_now.store(false, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    fn poll_window(&self) {
        match self.resolve_foreground() {
            Some(app) => {
            let excluded = self.config.read().is_excluded_path(
                &app.name,
                &app.title,
                app.path.as_deref(),
            );
            let w = { self.active_writer.lock().clone() };
            let pending = {
                let _pipe = self.pipeline.lock();
                let prev = self.current_app.read().clone();
                let switched = prev.as_ref().map(|p| p.name != app.name).unwrap_or(true);
                let mut typing = None;
                let mut focus = None;
                if self.is_capturing() && (switched || excluded) {
                    typing = self.take_typing_segment();
                }
                self.excluded_now.store(excluded, Ordering::SeqCst);
                let changed = prev
                    .as_ref()
                    .map(|p| p.name != app.name || p.title != app.title)
                    .unwrap_or(true);
                *self.current_app.write() = Some(app.clone());
                if self.is_capturing() && changed {
                    let title = if excluded {
                        "[redacted]".into()
                    } else {
                        app.title.clone()
                    };
                    focus = Some(json!({
                        "name": app.name,
                        "title": title,
                        "excluded": excluded,
                    }));
                }
                (typing, focus)
            };
            let (typing, focus) = pending;
            if let Some(w) = w {
                if let Some(seg) = typing {
                    self.emit_typing_segment(&w, seg);
                }
                if let Some(data) = focus {
                    self.emit(&w, "app_focus", data);
                }
            }
            }
            None => {
                if self.excluded_now.load(Ordering::SeqCst) {
                    let name = self.current_app_name().unwrap_or_default();
                    if name.is_empty() || name.to_lowercase().contains("deskcap") {
                        self.excluded_now.store(false, Ordering::SeqCst);
                    }
                }
            }
        }
    }

    fn mono_ts(&self) -> f64 {
        self.started
            .read()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    /// Caller holds `pipeline`. Takes open typing without writing disk.
    fn take_typing_segment(&self) -> Option<crate::typing::TypingSegment> {
        self.promote_ime_staging();
        let end_ts = self.mono_ts();
        let end_wall = wall_ts();
        let seg = self.typing.lock().flush_now(end_ts, end_wall);
        self.clear_ime_staging();
        seg
    }

    fn flush_typing_idle(&self, writer: &Arc<Mutex<SegmentWriter>>) {
        let seg = {
            let _pipe = self.pipeline.lock();
            // 0.6.58 — 统一真源：与 `typing::IDLE_FLUSH` 保持一致（见那里的注释）。
            // IME staging 的等待语义相同：多久没动静认为这句说完了。
            const IDLE: Duration = crate::typing::IDLE_FLUSH;
            /// Wait for UIA/AX to replace pinyin with 汉字 before flushing Latin.
            const IME_HOLD: Duration = Duration::from_millis(4500);
            /// Chat (微信): hold open while user edits — only Enter/app-switch should finalize.
            const CHAT_HOLD: Duration = Duration::from_millis(12000);
            let end_ts = self.mono_ts();
            let end_wall = wall_ts();

            let chat = self
                .current_app_name()
                .map(|n| crate::ime::is_chat_app_name(&n))
                .unwrap_or(false);

            let staging_idle = {
                let empty = self.ime_staging.lock().is_empty();
                if empty {
                    false
                } else {
                    self.ime_staging_at
                        .lock()
                        .map(|t| t.elapsed() >= if chat { CHAT_HOLD } else { IDLE })
                        .unwrap_or(false)
                }
            };
            if staging_idle {
                self.promote_ime_staging();
            }

            // Chat apps with CJK in buffer: wait for Enter (AX replace keeps buffer fresh).
            // Only flush after a long quiet period so mid-edit deletes don't emit fragments.
            if chat {
                let typing = self.typing.lock();
                if !typing.text_ref().is_empty() && !typing.quiet_for(CHAT_HOLD) {
                    #[cfg(target_os = "macos")]
                    crate::ax_text::request_force_poll();
                    return;
                }
            }

            // Hold pure-Latin buffers briefly so UIA can sync corrected 汉字.
            let hold_latin = {
                let typing = self.typing.lock();
                looks_like_ime_raw(typing.text_ref()) && !typing.quiet_for(IME_HOLD)
            };
            if hold_latin {
                #[cfg(target_os = "macos")]
                crate::ax_text::request_force_poll();
                #[cfg(windows)]
                crate::uia_text::request_force_poll();
                None
            } else if let Some(seg) = self.typing.lock().flush_if_idle(end_ts, end_wall) {
                self.clear_ime_staging();
                Some(seg)
            } else {
                if !self.ime_staging.lock().is_empty() {
                    #[cfg(target_os = "macos")]
                    crate::ax_text::request_force_poll();
                    #[cfg(windows)]
                    crate::uia_text::request_force_poll();
                }
                None
            }
        };
        if let Some(seg) = seg {
            self.emit_typing_segment(writer, seg);
        }
    }

    fn emit_typing_segment(
        &self,
        writer: &Arc<Mutex<SegmentWriter>>,
        mut seg: crate::typing::TypingSegment,
    ) {
        let tag = self.upgrade_chat_typing_from_ax(&mut seg);
        let mut data = seg.to_data();
        if let Some(tag) = tag {
            if let Some(obj) = data.as_object_mut() {
                obj.insert("capture".into(), json!(tag));
            }
        }
        self.emit_with_app(writer, "typing", data, seg.app);
    }

    /// DeskCap IME committed 汉字 (or Latin) — authoritative over EventTap pinyin.
    pub fn ingest_ime_commit(&self, text: &str, app_hint: Option<&str>, source: Option<&str>) -> usize {
        if !self.is_capturing() || !self.config.read().capture_keyboard {
            return 0;
        }
        let text = text.trim();
        if text.is_empty() {
            return 0;
        }
        // Prefer IME-reported app. Do not use excluded_now (may be DeskCap while HTTP lands).
        let hint = app_hint.unwrap_or("").trim();
        if !hint.is_empty() && self.config.read().is_excluded(hint, "") {
            return 0;
        }
        if hint.is_empty() && self.excluded_now.load(Ordering::SeqCst) {
            return 0;
        }
        let w = { self.active_writer.lock().clone() };
        let Some(w) = w else { return 0 };

        let app = {
            let _pipe = self.pipeline.lock();
            // IME commit is ground truth: drop EventTap pinyin **without** writing it
            // (flush_now would emit a second typing event for the same keystrokes).
            self.clear_ime_staging();
            self.typing.lock().clear();
            let cur = self.current_app.read().clone();
            match (cur, hint) {
                (Some(a), _) if hint.is_empty() || a.name == hint => Some(a),
                (_, h) if !h.is_empty() => Some(AppContext {
                    name: h.to_string(),
                    title: String::new(),
                    path: None,
                }),
                (a, _) => a,
            }
        };
        let ts = self.mono_ts();
        let wall = wall_ts();
        let data = json!({
            "text": text,
            "char_count": text.chars().count(),
            "start_ts": ts,
            "end_ts": ts,
            "start_wall": wall,
            "end_wall": wall,
            "duration_secs": 0.0,
            "backspace_count": 0,
            "capture": source.unwrap_or("ime_commit"),
        });
        self.emit_with_app(&w, "typing", data, app);
        crate::logutil::info(&format!(
            "ime_commit chars={} source={}",
            text.chars().count(),
            source.unwrap_or("ime_commit")
        ));
        1
    }

    pub fn start(self: &Arc<Self>) -> anyhow::Result<SessionStatus> {
        self.bootstrap();
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(self.status());
        }

        let cfg = self.config.read().clone();
        std::fs::create_dir_all(&cfg.data_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&cfg.data_dir, std::fs::Permissions::from_mode(0o700));
        }
        let session_id = Uuid::new_v4().to_string();
        let session_dir = cfg.data_dir.join(&session_id);
        std::fs::create_dir_all(&session_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&session_dir, std::fs::Permissions::from_mode(0o700));
        }

        let writer = SegmentWriter::open(&session_dir, &session_id)?;
        *self.segment_path.write() = Some(writer.path().display().to_string());
        *self.session_id.write() = Some(session_id);
        *self.started.write() = Some(Instant::now());
        *self.started_wall.write() = Some(wall_ts());
        *self.event_count.write() = 0;
        self.recent.write().clear();
        self.typing.lock().clear();
        *self.last_emitted_typing.lock() = None;
        *self.active_writer.lock() = Some(Arc::new(Mutex::new(writer)));

        // Optional screenshots — only while this session runs (checked via is_capturing + writer).
        if cfg.screenshot_interval_secs > 0 {
            let engine = Arc::clone(self);
            let interval = cfg.screenshot_interval_secs;
            let shot_dir = session_dir;
            let sid = self.session_id.read().clone();
            let _ = thread::Builder::new()
                .name("deskcap-shot".into())
                .spawn(move || {
                    while engine.is_capturing()
                        && engine.session_id.read().as_ref() == sid.as_ref()
                    {
                        for _ in 0..(interval.saturating_mul(10)) {
                            if !engine.is_capturing() {
                                return;
                            }
                            thread::sleep(Duration::from_millis(100));
                        }
                        if engine.excluded_now.load(Ordering::SeqCst) {
                            continue;
                        }
                        let w = { engine.active_writer.lock().clone() };
                        if let (Ok(path), Some(w)) = (capture_primary_screenshot(&shot_dir), w) {
                            engine.emit(
                                &w,
                                "screenshot",
                                json!({ "path": path.display().to_string() }),
                            );
                        }
                    }
                });
        }

        Ok(self.status())
    }

    pub fn stop(self: &Arc<Self>) -> SessionStatus {
        if !self.running.swap(false, Ordering::SeqCst) {
            return self.status();
        }
        let (w, typing) = {
            let _pipe = self.pipeline.lock();
            let typing = self.take_typing_segment();
            let w = { self.active_writer.lock().clone() };
            *self.active_writer.lock() = None;
            (w, typing)
        };
        if let Some(w) = w {
            if let Some(seg) = typing {
                self.emit_typing_segment(&w, seg);
            }
            let _ = w.lock().flush();
        }
        crate::daybook::flush();
        // Input tap stays alive (idle) — no second EventTap, no crash.
        self.status()
    }

    fn emit(&self, writer: &Arc<Mutex<SegmentWriter>>, event_type: &str, data: serde_json::Value) {
        let app = self.current_app.read().clone();
        self.emit_with_app(writer, event_type, data, app);
    }

    fn emit_with_app(
        &self,
        writer: &Arc<Mutex<SegmentWriter>>,
        event_type: &str,
        mut data: serde_json::Value,
        app: Option<AppContext>,
    ) {
        let ts = self.mono_ts();
        if event_type == "mouse_click" {
            let (x, y) = *self.last_mouse.read();
            if let Some(obj) = data.as_object_mut() {
                obj.insert("x".into(), json!(x));
                obj.insert("y".into(), json!(y));
            }
        }
        // Commercial hygiene: never persist raw secrets in day ledger / sessions.
        if matches!(
            event_type,
            "typing" | "clipboard_paste" | "clipboard_copy" | "clipboard_cut"
        ) {
            if let Some(obj) = data.as_object_mut() {
                if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
                    let (redacted, hit) = crate::privacy::redact_secrets(text);
                    if hit {
                        let n = redacted.chars().count();
                        obj.insert("text".into(), json!(redacted));
                        obj.insert("redacted".into(), json!(true));
                        obj.insert("char_count".into(), json!(n));
                    }
                }
            }
        }
        // Typing segments: Mac-style incremental — never re-emit the same composer blob.
        if event_type == "typing" {
            if let Some(obj) = data.as_object_mut() {
                let text = obj
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let trimmed = text.trim();
                if trimmed.is_empty() || crate::ime::is_agent_noise_text(trimmed) {
                    return;
                }
                let mut last = self.last_emitted_typing.lock();
                if let Some(prev) = last.as_ref() {
                    if trimmed == prev {
                        return;
                    }
                    if prev.starts_with(trimmed) {
                        // Shorter re-capture of already-emitted text.
                        return;
                    }
                    if trimmed.starts_with(prev.as_str()) {
                        let suffix = &trimmed[prev.len()..];
                        if suffix.trim().is_empty() || crate::ime::is_agent_noise_text(suffix) {
                            return;
                        }
                        let n = suffix.chars().count();
                        obj.insert("text".into(), json!(suffix));
                        obj.insert("char_count".into(), json!(n));
                        *last = Some(trimmed.to_string());
                    } else {
                        *last = Some(trimmed.to_string());
                    }
                } else {
                    *last = Some(trimmed.to_string());
                }
            }
        }
        let event = CaptureEvent {
            ts,
            wall_ts: wall_ts(),
            event_type: event_type.into(),
            app,
            data,
        };
        {
            let mut w = writer.lock();
            if let Err(e) = w.write(&event) {
                crate::logutil::warn(&format!("session jsonl write failed: {e}"));
            }
        }
        daybook::append_event(&self.config.read().data_dir, &event);
        *self.event_count.write() += 1;
        let mut recent = self.recent.write();
        if recent.len() >= RECENT_CAP {
            recent.pop_front();
        }
        recent.push_back(event);
    }

    #[cfg(all(not(target_os = "macos"), not(windows)))]
    fn note_key(&self) {
        self.keydown_n.fetch_add(1, Ordering::Relaxed);
        self.touch_input();
    }

    #[cfg(all(not(target_os = "macos"), not(windows)))]
    fn note_click(&self) {
        self.mouse_n.fetch_add(1, Ordering::Relaxed);
        self.touch_input();
    }

    #[cfg(all(not(target_os = "macos"), not(windows)))]
    fn touch_input(&self) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.last_input_unix_ms.store(ms, Ordering::Relaxed);
    }

    #[cfg(all(not(target_os = "macos"), not(windows)))]
    fn spawn_rdev_listener(self: &Arc<Self>) {
        use rdev::{listen, Event, EventType, Key};
        use std::sync::mpsc;

        enum HookJob {
            Key { keycode: i64, text: Option<String> },
            Click { button: &'static str },
        }

        // Bounded queue: hook only enqueues; worker does locks / JSONL / IME.
        // Full queue → drop (never block WH_*_LL).
        let (tx, rx) = mpsc::sync_channel::<HookJob>(512);
        let worker = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("deskcap-input-worker".into())
            .spawn(move || {
                crate::logutil::info("rdev worker up");
                while let Ok(job) = rx.recv() {
                    let t0 = Instant::now();
                    match job {
                        HookJob::Key { keycode, text } => worker.on_key_input(keycode, text),
                        HookJob::Click { button } => worker.on_mouse_click(button),
                    }
                    crate::logutil::lag_ms("input_worker", t0.elapsed().as_millis() as u64);
                }
            });

        let engine = Arc::clone(self);
        let _ = thread::Builder::new()
            .name("deskcap-input".into())
            .spawn(move || {
                crate::logutil::info(
                    "rdev listener up (ignore MouseMove; enqueue only; fast gate)",
                );
                let cb = move |event: Event| {
                    // WH_*_LL must return ASAP — no get_active_window / UIA / disk I/O.
                    if !engine.is_capturing() {
                        return;
                    }
                    if matches!(event.event_type, EventType::MouseMove { .. }) {
                        return;
                    }
                    let t0 = Instant::now();
                    let enqueue = |job: HookJob| {
                        if tx.try_send(job).is_err() {
                            // Throttled via lag_ms (≥8ms gate).
                            crate::logutil::lag_ms("input_queue_drop", 8);
                        }
                    };
                    match event.event_type {
                        EventType::KeyPress(Key::Backspace) => {
                            if !engine.gate_input_fast() {
                                engine.typing.lock().clear();
                                return;
                            }
                            engine.note_key();
                            enqueue(HookJob::Key {
                                keycode: 51,
                                text: Some("\u{8}".into()),
                            });
                        }
                        EventType::KeyPress(Key::Return)
                        | EventType::KeyPress(Key::KpReturn) => {
                            if !engine.gate_input_fast() {
                                engine.typing.lock().clear();
                                return;
                            }
                            engine.note_key();
                            enqueue(HookJob::Key {
                                keycode: 36,
                                text: Some("\n".into()),
                            });
                        }
                        EventType::KeyPress(Key::Tab) => {
                            if !engine.gate_input_fast() {
                                engine.typing.lock().clear();
                                return;
                            }
                            engine.note_key();
                            enqueue(HookJob::Key {
                                keycode: 48,
                                text: Some("\t".into()),
                            });
                        }
                        EventType::KeyPress(_) => {
                            if !engine.gate_input_fast() {
                                engine.typing.lock().clear();
                                return;
                            }
                            engine.note_key();
                            enqueue(HookJob::Key {
                                keycode: 0,
                                text: event.name,
                            });
                        }
                        EventType::ButtonPress(btn) => {
                            if !engine.gate_input_fast() {
                                engine.typing.lock().clear();
                                return;
                            }
                            engine.note_click();
                            let name = match btn {
                                rdev::Button::Left => "left",
                                rdev::Button::Right => "right",
                                rdev::Button::Middle => "middle",
                                _ => "other",
                            };
                            enqueue(HookJob::Click { button: name });
                        }
                        _ => {}
                    }
                    crate::logutil::lag_ms("rdev_cb", t0.elapsed().as_millis() as u64);
                };
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = listen(cb);
                }));
                crate::logutil::warn("rdev listener exited");
            });
    }
}

fn wall_ts() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn looks_like_ime_raw(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    if t.chars().any(|c| !c.is_ascii()) {
        return false;
    }
    // Candidate index digits (changshi2) or pure Latin/spaces/apostrophe.
    t.chars()
        .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == ' ' || c == '\'')
        && t.chars().any(|c| c.is_ascii_alphabetic())
}

/// Space or 1–9: IME candidate confirm (not content to store in the pinyin buffer).
fn is_ime_candidate_confirm(text: Option<&str>, keycode: i64) -> bool {
    // macOS space=49; Windows VK_SPACE=0x20 — also trust text when present.
    if text == Some(" ") || keycode == 49 || keycode == 32 {
        return true;
    }
    is_ime_digit_confirm(text)
}

fn is_ime_digit_confirm(text: Option<&str>) -> bool {
    match text {
        Some(t) if t.len() == 1 => {
            let c = t.chars().next().unwrap();
            c.is_ascii_digit() && c != '0'
        }
        _ => false,
    }
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn capture_primary_screenshot(dir: &PathBuf) -> anyhow::Result<PathBuf> {
    let monitors = Monitor::all()?;
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| Monitor::all().ok().and_then(|mut m| m.pop()))
        .ok_or_else(|| anyhow::anyhow!("no monitor"))?;
    let image = monitor.capture_image()?;
    let path = dir.join(format!("shot_{}.png", wall_ms()));
    image.save(&path)?;
    Ok(path)
}

#[cfg(test)]
mod click_dedup_tests {
    use super::*;

    fn lc(x: f64, y: f64, button: u8, ago_ms: u64) -> LastClick {
        let mut at = Instant::now();
        if ago_ms > 0 {
            at -= Duration::from_millis(ago_ms);
        }
        LastClick { x, y, button, at }
    }

    #[test]
    fn drops_same_pixel_same_button_within_window() {
        let prev = lc(100.0, 200.0, 0, 50);
        assert!(should_drop_click(prev, "left", 100.0, 200.0, Instant::now()));
    }

    #[test]
    fn keeps_pixels_outside_window() {
        // 200 ms apart — human double-click pace, must NOT drop.
        let prev = lc(100.0, 200.0, 0, 200);
        assert!(!should_drop_click(prev, "left", 100.0, 200.0, Instant::now()));
    }

    #[test]
    fn keeps_different_buttons() {
        let prev = lc(100.0, 200.0, 0, 10);
        assert!(!should_drop_click(prev, "right", 100.0, 200.0, Instant::now()));
    }

    #[test]
    fn keeps_different_pixels() {
        // Drag mouse 1 px → user moved, not jitter.
        let prev = lc(100.0, 200.0, 0, 10);
        assert!(!should_drop_click(prev, "left", 101.0, 200.0, Instant::now()));
    }

    #[test]
    fn keeps_after_long_idle() {
        // Last click was 5 s ago — fresh start.
        let prev = lc(100.0, 200.0, 0, 5000);
        assert!(!should_drop_click(prev, "left", 100.0, 200.0, Instant::now()));
    }

    #[test]
    fn maps_button_names_stably() {
        assert_eq!(click_button_id("left"), 0);
        assert_eq!(click_button_id("right"), 1);
        assert_eq!(click_button_id("middle"), 2);
        assert_eq!(click_button_id("other"), 3);
        assert_eq!(click_button_id("unknown"), 3);
    }
}
