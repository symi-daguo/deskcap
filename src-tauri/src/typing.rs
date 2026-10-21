use crate::event::AppContext;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// 打字停顿多久算「一句话说完」→ 切段落库。
///
/// 0.6.58 — 1800ms → **3000ms**（用户反馈「打字和点击有很多无效数据」）。
/// 实测佐证（2026-10-14 全天 1040 条 typing）：同 App 相邻段**间隔中位数
/// 只有 1.6 秒**，54% 的间隔 < 2s —— 大量完整句子被 1.8s 阈值在思路中间
/// 切断，产生 26% 的 <5 字碎片。调到 3s 后，间隔 2–3s 的 88 次停顿被并入
/// 同段；配合提炼侧「碎片缝合」把残余碎片按语义还原（削减 65% 数据量）。
/// 触发强切段的路径不变：回车 / 切 App / 点击 / IME 上屏提交。
pub const IDLE_FLUSH: Duration = Duration::from_millis(3000);

#[derive(Debug, Default)]
pub struct TypingAggregator {
    buf: String,
    start_ts: Option<f64>,
    start_wall: Option<f64>,
    app: Option<AppContext>,
    last_activity: Option<Instant>,
    /// How many Backspace/Delete key presses during this open segment (WeChat Agent match).
    backspace_count: u32,
}

#[derive(Debug, Clone)]
pub struct TypingSegment {
    pub text: String,
    pub start_ts: f64,
    pub end_ts: f64,
    pub start_wall: f64,
    pub end_wall: f64,
    pub app: Option<AppContext>,
    pub backspace_count: u32,
}

/// Space/1–9 confirm result for WeChat pinyin staging.
#[derive(Debug, PartialEq, Eq)]
pub enum SealOutcome {
    /// Trailing Latin sealed with `'`.
    Sealed,
    /// No trailing Latin.
    Empty,
    /// Incomplete initial (e.g. `sh`/`zh`/`d`) — letters kept; caller must keep staging.
    StillComposing,
}

impl TypingSegment {
    pub fn to_data(&self) -> Value {
        json!({
            "text": self.text,
            "start_ts": self.start_ts,
            "end_ts": self.end_ts,
            "start_wall": self.start_wall,
            "end_wall": self.end_wall,
            "char_count": self.text.chars().count(),
            "duration_secs": (self.end_ts - self.start_ts).max(0.0),
            "backspace_count": self.backspace_count,
        })
    }
}

impl TypingAggregator {
    pub fn push_text(
        &mut self,
        piece: &str,
        ts: f64,
        wall: f64,
        app: Option<AppContext>,
    ) {
        if piece.is_empty() {
            return;
        }
        if self.buf.is_empty() {
            self.start_ts = Some(ts);
            self.start_wall = Some(wall);
            self.app = app;
        }
        self.buf.push_str(piece);
        self.last_activity = Some(Instant::now());
    }

    pub fn note_backspace_key(&mut self) {
        self.backspace_count = self.backspace_count.saturating_add(1);
        self.last_activity = Some(Instant::now());
    }

    pub fn backspace(&mut self) {
        self.buf.pop();
        self.last_activity = Some(Instant::now());
        if self.buf.is_empty() {
            self.clear();
        }
    }

    /// WeChat after Space/1–9 confirm: one Backspace ≈ delete one on-screen字 → drop last `'`-chunk.
    /// Trailing `?!.` etc. pop first. Incomplete sealed junk (`h'`) drops as one unit too.
    pub fn backspace_pinyin_unit(&mut self) {
        if self
            .buf
            .chars()
            .last()
            .is_some_and(is_sentence_punct)
        {
            self.buf.pop();
        } else if self.buf.ends_with('\'') {
            self.buf.pop();
            while self
                .buf
                .chars()
                .last()
                .is_some_and(|c| c.is_ascii_alphabetic())
            {
                self.buf.pop();
            }
        } else {
            self.buf.pop();
        }
        self.last_activity = Some(Instant::now());
        if self.buf.is_empty() {
            self.clear();
        }
    }

    /// Space/1–9 confirm: seal trailing Latin if it looks like a real syllable.
    /// **Never delete** incomplete initials — dropping `sh`/`d` caused `shuode`→`ode`, `duima`→`ima`.
    pub fn seal_pinyin_chunk(&mut self) -> SealOutcome {
        let chars: Vec<char> = self.buf.chars().collect();
        let mut start = chars.len();
        while start > 0 && chars[start - 1].is_ascii_alphabetic() {
            start -= 1;
        }
        if start == chars.len() {
            return SealOutcome::Empty;
        }
        let chunk: String = chars[start..].iter().collect();
        if is_plausible_pinyin_chunk(&chunk) {
            if !self.buf.ends_with('\'') {
                self.buf.push('\'');
            }
            self.last_activity = Some(Instant::now());
            SealOutcome::Sealed
        } else {
            // Keep `sh` / `d` / `zh` in the buffer — user is still composing the syllable.
            self.last_activity = Some(Instant::now());
            SealOutcome::StillComposing
        }
    }

    /// Before flush: drop sealed junk (`'h'`), and mid-phrase lone letters (`'i'`).
    /// Never strip 2+ letter pieces that still have letters (even if odd) — prefer raw accuracy.
    pub fn scrub_orphan_pinyin_chunks(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let (punct_suffix, core) = {
            let mut chars: Vec<char> = self.buf.chars().collect();
            let mut suffix = String::new();
            while chars
                .last()
                .copied()
                .is_some_and(is_sentence_punct)
            {
                suffix.insert(0, chars.pop().unwrap());
            }
            (suffix, chars.into_iter().collect::<String>())
        };
        let parts: Vec<&str> = core
            .split('\'')
            .filter(|p| !p.is_empty())
            .collect();
        let multi = parts.len() > 1;
        let mut out = String::new();
        for part in &parts {
            let len = part.chars().count();
            // Only drop clear junk: lone letter mid-phrase, or consonant-only length-1.
            if multi && len == 1 {
                continue;
            }
            if !is_plausible_pinyin_chunk(part) && len <= 1 {
                continue;
            }
            // Keep zh/ch/sh/ng and any longer run even without perfect vowel parse.
            if !out.is_empty() {
                out.push('\'');
            }
            out.push_str(part);
        }
        if !out.is_empty() && core.ends_with('\'') {
            out.push('\'');
        }
        out.push_str(&punct_suffix);
        self.buf = out;
        if self.buf.is_empty() {
            self.clear();
        }
    }

    pub fn flush_now(&mut self, end_ts: f64, end_wall: f64) -> Option<TypingSegment> {
        // Only scrub WeChat-style Latin pinyin; never touch CJK / mixed buffers.
        if looks_like_chat_pinyin_buffer(&self.buf) {
            self.scrub_orphan_pinyin_chunks();
        }
        if self.buf.trim().is_empty() {
            self.clear();
            return None;
        }
        let seg = TypingSegment {
            text: self.buf.clone(),
            start_ts: self.start_ts.unwrap_or(end_ts),
            end_ts,
            start_wall: self.start_wall.unwrap_or(end_wall),
            end_wall,
            app: self.app.clone(),
            backspace_count: self.backspace_count,
        };
        self.clear();
        Some(seg)
    }

    pub fn flush_if_idle(&mut self, end_ts: f64, end_wall: f64) -> Option<TypingSegment> {
        let idle = self
            .last_activity
            .map(|t| t.elapsed() >= IDLE_FLUSH)
            .unwrap_or(false);
        if idle {
            self.flush_now(end_ts, end_wall)
        } else {
            None
        }
    }

    /// True when buffer has been quiet for at least `d` (IME wait before flushing Latin).
    pub fn quiet_for(&self, d: Duration) -> bool {
        self.last_activity
            .map(|t| t.elapsed() >= d)
            .unwrap_or(true)
    }

    pub fn clear(&mut self) {
        self.buf.clear();
        self.start_ts = None;
        self.start_wall = None;
        self.app = None;
        self.last_activity = None;
        self.backspace_count = 0;
    }

    /// Replace entire open buffer with on-screen composer text (WeChat edit-safe).
    pub fn replace_all(&mut self, text: &str, ts: f64, wall: f64, app: Option<AppContext>) {
        self.buf = text.to_string();
        if self.start_ts.is_none() {
            self.start_ts = Some(ts);
            self.start_wall = Some(wall);
        }
        self.app = app;
        self.last_activity = Some(Instant::now());
    }

    pub fn text_ref(&self) -> &str {
        &self.buf
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.buf.trim().is_empty()
    }
}

/// True if OS-provided name is printable content (not a bare control glyph).
pub fn is_printable_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    // Single ASCII control
    if name.len() == 1 {
        let c = name.chars().next().unwrap();
        return !c.is_control();
    }
    !name.chars().all(|c| c.is_control())
}

fn is_sentence_punct(c: char) -> bool {
    matches!(
        c,
        '?' | '!' | '.' | ',' | ';' | ':' | '。' | '？' | '！' | '，' | '、' | '…' | '~' | '～'
    )
}

/// Trailing Latin run looks like a real pinyin syllable (has vowel), not a stray `h`/`zh`.
pub fn is_plausible_pinyin_chunk(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if lower == "ng" || lower == "n" {
        // 嗯 / -ng final sometimes confirmed alone
        return true;
    }
    lower.chars().any(|c| matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'v'))
}

/// Pinyin buffer even with trailing `?` / `。` (so WeChat BS still drops syllables).
pub fn looks_like_chat_pinyin_buffer(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let core: String = t
        .chars()
        .filter(|c| !is_sentence_punct(*c) && *c != ' ')
        .collect();
    if core.is_empty() {
        return false;
    }
    if core.chars().any(|c| !c.is_ascii()) {
        return false;
    }
    core.chars()
        .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit() || c == '\'')
        && core.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_sentence() {
        let mut a = TypingAggregator::default();
        a.push_text("你", 1.0, 100.0, None);
        a.push_text("好", 1.1, 100.1, None);
        a.push_text("世界", 1.2, 100.2, None);
        let seg = a.flush_now(1.5, 100.5).unwrap();
        assert_eq!(seg.text, "你好世界");
        assert_eq!(seg.char_count_via(), 4);
        assert_eq!(seg.backspace_count, 0);
    }

    #[test]
    fn pinyin_unit_backspace_drops_confirmed_syllable() {
        let mut a = TypingAggregator::default();
        a.push_text("wo'men'", 1.0, 100.0, None);
        a.note_backspace_key();
        a.backspace_pinyin_unit();
        assert_eq!(a.text_ref(), "wo'");
        a.note_backspace_key();
        a.backspace_pinyin_unit();
        assert!(a.is_empty());
    }

    #[test]
    fn seal_keeps_incomplete_initial_sh_then_finishes_shuode() {
        let mut a = TypingAggregator::default();
        a.push_text("wo", 1.0, 100.0, None);
        assert_eq!(a.seal_pinyin_chunk(), SealOutcome::Sealed);
        assert_eq!(a.text_ref(), "wo'");
        a.push_text("sh", 1.1, 100.1, None);
        // Accidental Space after initial — must NOT drop `sh`
        assert_eq!(a.seal_pinyin_chunk(), SealOutcome::StillComposing);
        assert_eq!(a.text_ref(), "wo'sh");
        a.push_text("uode", 1.2, 100.2, None);
        assert_eq!(a.seal_pinyin_chunk(), SealOutcome::Sealed);
        assert_eq!(a.text_ref(), "wo'shuode'");
    }

    #[test]
    fn seal_keeps_d_then_finishes_duima() {
        let mut a = TypingAggregator::default();
        a.push_text("d", 1.0, 100.0, None);
        assert_eq!(a.seal_pinyin_chunk(), SealOutcome::StillComposing);
        assert_eq!(a.text_ref(), "d");
        a.push_text("uima", 1.1, 100.1, None);
        assert_eq!(a.seal_pinyin_chunk(), SealOutcome::Sealed);
        assert_eq!(a.text_ref(), "duima'");
    }

    #[test]
    fn scrub_keeps_sh_chunk_but_drops_lone_letter() {
        let mut a = TypingAggregator::default();
        a.push_text("wo'sh'uode'?", 1.0, 100.0, None);
        a.scrub_orphan_pinyin_chunks();
        // `sh` kept (2 letters); would rather merge but accuracy > beauty
        assert!(a.text_ref().contains("sh"));
        assert!(a.text_ref().contains("uode"));
        let mut b = TypingAggregator::default();
        b.push_text("weishenme'h'i'zheyang'?", 1.0, 100.0, None);
        b.scrub_orphan_pinyin_chunks();
        assert_eq!(b.text_ref(), "weishenme'zheyang'?");
    }

    impl TypingSegment {
        fn char_count_via(&self) -> usize {
            self.text.chars().count()
        }
    }
}
