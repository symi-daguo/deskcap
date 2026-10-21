//! Windows UI Automation — read focused composer text (parallel to macOS AX).
//!
//! Low-level hooks (`rdev` / ToUnicodeEx) only see pinyin Latin under Chinese IME.
//! UIA Value/Text patterns see the committed 汉字 in many apps. WeChat/Electron may
//! still be pinyin-only — staging + Agent translate remain.
//!
//! Runs on its own thread at a low rate; never call from WH_*_LL hooks.
//!
//! **Incremental only (Mac-aligned):** never dump the whole composer into typing.
//! After idle flush / focus return, `prev` is the baseline; only LCP deltas are applied.

use crate::engine::CaptureEngine;
use crate::ime::{char_lcp_len, has_cjk, is_chat_app_name, strip_ime_composing};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use uiautomation::controls::ControlType;
use uiautomation::core::UIAutomation;
use uiautomation::patterns::{UILegacyIAccessiblePattern, UITextPattern, UIValuePattern};
use uiautomation::types::UIProperty;

static FORCE_POLL: AtomicBool = AtomicBool::new(false);
static KEY_VIA_UIA: AtomicU64 = AtomicU64::new(0);
static UIA_LIVE_CJK: AtomicBool = AtomicBool::new(false);
static UIA_LIVE_AT: Mutex<Option<Instant>> = Mutex::new(None);

pub fn request_force_poll() {
    FORCE_POLL.store(true, Ordering::SeqCst);
}

pub fn uia_chars_captured() -> u64 {
    KEY_VIA_UIA.load(Ordering::SeqCst)
}

/// When true, keyboard path should not also push pinyin Latin into typing (UIA owns CJK).
pub fn uia_defers_latin() -> bool {
    if !UIA_LIVE_CJK.load(Ordering::SeqCst) {
        return false;
    }
    UIA_LIVE_AT
        .lock()
        .map(|t| t.elapsed() < Duration::from_secs(3))
        .unwrap_or(false)
}

fn mark_uia_cjk(ok: bool) {
    UIA_LIVE_CJK.store(ok, Ordering::SeqCst);
    if ok {
        *UIA_LIVE_AT.lock() = Some(Instant::now());
    }
}

fn read_focused_value(auto: &UIAutomation, chat: bool) -> Option<(String, String)> {
    let el = auto.get_focused_element().ok()?;
    let ctype = el.get_control_type().unwrap_or(ControlType::Custom);
    let role = format!("{ctype:?}");

    let editable = matches!(
        ctype,
        ControlType::Edit
            | ControlType::Document
            | ControlType::Text
            | ControlType::ComboBox
            | ControlType::DataItem
            | ControlType::Custom
            | ControlType::Group
            | ControlType::Pane
    );
    if !chat && !editable {
        return None;
    }

    let mut value = el
        .get_pattern::<UIValuePattern>()
        .ok()
        .and_then(|p| p.get_value().ok())
        .filter(|s| !s.trim().is_empty());

    if value.is_none() {
        value = el
            .get_pattern::<UITextPattern>()
            .ok()
            .and_then(|p| p.get_document_range().ok())
            .and_then(|r| r.get_text(-1).ok())
            .filter(|s| !s.trim().is_empty());
    }

    if value.is_none() {
        if let Ok(v) = el.get_property_value(UIProperty::ValueValue) {
            if let Ok(s) = v.get_string() {
                if !s.trim().is_empty() {
                    value = Some(s);
                }
            }
        }
    }

    if value.is_none() {
        value = el
            .get_pattern::<UILegacyIAccessiblePattern>()
            .ok()
            .and_then(|p| p.get_value().ok())
            .filter(|s| !s.trim().is_empty() && s.chars().count() < 4000);
    }
    if value.is_none() {
        if let Ok(v) = el.get_property_value(UIProperty::LegacyIAccessibleValue) {
            if let Ok(s) = v.get_string() {
                if !s.trim().is_empty() && s.chars().count() < 4000 {
                    value = Some(s);
                }
            }
        }
    }

    let value = value?;
    if looks_like_ui_noise(&value) {
        return None;
    }
    Some((role, value))
}

fn looks_like_ui_noise(s: &str) -> bool {
    let t = s.trim();
    t.is_empty()
        || t == "…"
        || t.chars().all(|c| c.is_whitespace())
        || (t.len() < 40
            && (t.contains("搜索") || t.contains("Search") || t.contains("Minimize")))
        || crate::ime::is_agent_noise_text(t)
}

pub fn snapshot_committed_text(chat: bool) -> Option<String> {
    let auto = UIAutomation::new_direct().ok()?;
    let (_, value) = read_focused_value(&auto, chat)?;
    let committed = strip_ime_composing(&value);
    if committed.trim().is_empty() {
        return None;
    }
    if chat && !has_cjk(&committed) {
        return None;
    }
    Some(committed)
}

fn apply_uia_edit(engine: &CaptureEngine, prev: &str, next: &str) {
    if prev == next {
        return;
    }
    let add_chars = {
        let common = char_lcp_len(prev, next);
        next.chars().count().saturating_sub(common)
    };
    if add_chars > 0 {
        KEY_VIA_UIA.fetch_add(add_chars as u64, Ordering::SeqCst);
    }
    engine.apply_composer_edit(prev, next);
}

pub fn spawn_uia_text_poller(engine: Arc<CaptureEngine>) {
    let _ = thread::Builder::new()
        .name("deskcap-uia".into())
        .spawn(move || {
            crate::logutil::info("uia poller up (600ms, LCP-only, no full-composer sync)");
            let mut auto = UIAutomation::new_direct().ok();
            let mut prev = String::new();
            let mut prev_app = String::new();
            let mut fail_streak = 0u32;
            loop {
                let force = FORCE_POLL.swap(false, Ordering::SeqCst);
                thread::sleep(Duration::from_millis(if force { 120 } else { 600 }));
                if !engine.is_capturing() {
                    prev.clear();
                    prev_app.clear();
                    mark_uia_cjk(false);
                    continue;
                }
                if !engine.config.read().capture_keyboard {
                    continue;
                }
                // Hook-safe checks only — window poller owns focus/exclusion.
                if !engine.gate_input_fast() {
                    continue;
                }
                let app = engine.current_app_name().unwrap_or_default();
                if app.to_lowercase() == "deskcap" || app.eq_ignore_ascii_case("deskcap.exe") {
                    continue;
                }
                if app != prev_app {
                    prev.clear();
                    prev_app = app.clone();
                    mark_uia_cjk(false);
                }

                if auto.is_none() {
                    auto = UIAutomation::new_direct().ok();
                    if auto.is_none() {
                        fail_streak = fail_streak.saturating_add(1);
                        if fail_streak == 1 || fail_streak % 30 == 0 {
                            crate::logutil::warn("uia COM init failed");
                        }
                        continue;
                    }
                }
                let Some(ref uia) = auto else { continue };

                let t0 = Instant::now();
                let chat = is_chat_app_name(&app);
                let read = read_focused_value(uia, chat);
                crate::logutil::lag_ms("uia_read", t0.elapsed().as_millis() as u64);

                let Some((_role, value)) = read else {
                    fail_streak = fail_streak.saturating_add(1);
                    if fail_streak > 40 {
                        auto = None; // recreate COM after sustained failures
                        fail_streak = 0;
                    }
                    continue;
                };
                fail_streak = 0;

                let committed = strip_ime_composing(&value);
                // Chat: only CJK via UIA (pinyin stays on keyboard staging when UIA weak).
                // Other apps: UIA for committed 汉字; Latin/ASCII comes from VK map on the hook.
                if !has_cjk(&committed) {
                    continue;
                }
                mark_uia_cjk(true);
                if committed == prev {
                    continue;
                }

                let buf = engine.typing_buffer_snapshot();
                let had_latin = !buf.is_empty() && !has_cjk(&buf);
                // Drop staged pinyin so LCP does not fight Latin leftovers.
                if had_latin {
                    engine.clear_latin_typing_buffer();
                }

                // Focus enter / after app switch: baseline only — do not ingest whole draft.
                if prev.is_empty() {
                    // Only seed when pinyin was in-flight (IME just committed into an empty prev).
                    // Never seed when buffer was already empty — that re-captures a short draft
                    // after idle flush + focus return.
                    if had_latin && committed.chars().count() <= 80 {
                        apply_uia_edit(&engine, "", &committed);
                    }
                    prev = committed;
                    continue;
                }

                // Long document jump (scroll / replace): re-baseline, do not dump.
                if committed.chars().count() > 2000
                    && char_lcp_len(&prev, &committed) < prev.chars().count() / 4
                {
                    prev = committed;
                    continue;
                }

                // Incremental only — same as macOS AX.
                apply_uia_edit(&engine, &prev, &committed);
                prev = committed;
            }
        });
}
