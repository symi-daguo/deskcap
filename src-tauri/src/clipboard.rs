//! Local clipboard text read (paste/copy capture). Consent via `capture_clipboard`.
//!
//! On macOS, NSPasteboard can throw Objective-C exceptions (especially from a
//! background thread after Cmd+C). Uncaught ObjC exceptions abort the whole
//! process — see crash `deskcap-2026-09-10-172511.ips` (deskcap-clipboard / SIGABRT).

use arboard::Clipboard;
use parking_lot::Mutex;
use std::sync::OnceLock;

/// Soft cap so day ledgers stay lean (agent can still get full paste intent).
pub const MAX_CLIPBOARD_CHARS: usize = 8000;

fn pasteboard_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_plain_text_inner() -> Option<(String, bool)> {
    let _guard = pasteboard_lock().lock();
    let mut cb = Clipboard::new().ok()?;
    let raw = cb.get_text().ok()?;
    sanitize(&raw, MAX_CLIPBOARD_CHARS)
}

pub fn read_plain_text() -> Option<(String, bool)> {
    #[cfg(target_os = "macos")]
    {
        // Must use objc2 @catch — Rust catch_unwind does NOT catch ObjC exceptions
        // and the runtime will abort the process (kills capture for hours).
        match objc2::exception::catch(std::panic::AssertUnwindSafe(read_plain_text_inner)) {
            Ok(v) => v,
            Err(_) => {
                crate::logutil::warn("clipboard: NSPasteboard ObjC exception swallowed");
                None
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Still isolate Rust panics so one bad clipboard read cannot take down the app.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(read_plain_text_inner)) {
            Ok(v) => v,
            Err(_) => {
                crate::logutil::warn("clipboard: panic swallowed");
                None
            }
        }
    }
}

pub fn sanitize(raw: &str, max_chars: usize) -> Option<(String, bool)> {
    let cleaned: String = raw.chars().filter(|c| *c != '\0').collect();
    let t = cleaned.trim();
    if t.is_empty() {
        return None;
    }
    let count = t.chars().count();
    if count <= max_chars {
        Some((t.to_string(), false))
    } else {
        let mut out: String = t.chars().take(max_chars).collect();
        out.push('…');
        Some((out, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_empty() {
        assert!(sanitize("   \n", 100).is_none());
    }

    #[test]
    fn truncates() {
        let s = "啊".repeat(10);
        let (out, trunc) = sanitize(&s, 3).unwrap();
        assert!(trunc);
        assert_eq!(out.chars().count(), 4); // 3 + …
    }
}
