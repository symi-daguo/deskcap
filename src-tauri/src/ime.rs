//! Shared IME / chat-app helpers (macOS AX + Windows UIA + rdev).

/// WeChat / 企微 and cousins — Latin from the OS hook is usually pinyin, not 汉字.
pub fn is_chat_app_name(name: &str) -> bool {
    let n = name.to_lowercase();
    name.contains("微信")
        || n.contains("wechat")
        || name.contains("企业微信")
        || n.contains("wecom")
        || n.contains("wxwork")
        || n.contains("weixin")
}

pub fn has_cjk(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}'))
}

/// DeskCapIME writes `~/Library/Application Support/DeskCap/ime-active` while selected.
/// EventTap must not stage pinyin Latin in parallel (duplicate typing vs `/ime/commit`).
#[cfg(target_os = "macos")]
pub fn deskcap_ime_active() -> bool {
    dirs::data_dir()
        .map(|d| d.join("DeskCap").join("ime-active").is_file())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn deskcap_ime_active() -> bool {
    false
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}')
}

/// Drop IME composing crumbs (trailing/leading Latin + ZWSP) when CJK already present.
/// Pure English buffers are left untouched.
pub fn strip_ime_composing(s: &str) -> String {
    let chars: Vec<char> = s
        .chars()
        .filter(|c| *c != '\u{200b}' && *c != '\u{200c}' && *c != '\u{feff}')
        .collect();
    if !chars.iter().any(|c| is_cjk(*c)) {
        return chars.into_iter().collect();
    }
    let is_pinyin = |c: char| c.is_ascii_alphabetic() || c == '\'' || c == ' ';
    let mut end = chars.len();
    while end > 0 && is_pinyin(chars[end - 1]) {
        end -= 1;
    }
    let mut start = 0;
    while start < end && is_pinyin(chars[start]) {
        start += 1;
    }
    chars[start..end].iter().collect()
}

pub fn char_lcp_len(a: &str, b: &str) -> usize {
    a.chars()
        .zip(b.chars())
        .take_while(|(x, y)| x == y)
        .count()
}

/// UI chrome / AX mis-reads that must not become agent “user intent”.
/// Empirically ~15% of Mac daybook typing (2026-09-04..07) was this class —
/// mostly Cursor `Send follow-up`, AI助手 title, 系统设置 “Apple账户”.
pub fn is_agent_noise_text(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return true;
    }
    let n = t.chars().count();
    if n <= 1 {
        return true;
    }
    let low = t.to_lowercase();
    if low == "send follow-up" || low == "end follow-up" {
        return true;
    }
    if matches!(
        t,
        "助手" | "助手助手" | "正在进入" | "进入微信" | "Apple账户" | ", Apple账户"
    ) {
        return true;
    }
    if n < 48 && (t.contains("Apple账户") || t.contains("Apple ID")) {
        return true;
    }
    if n < 80 && (t.contains("通行密钥") || low.contains("passkey")) {
        return true;
    }
    if n < 24 && (t.contains("正在进入") || t == "进入微信") {
        return true;
    }
    // Pure punctuation / bullets
    if t.chars()
        .all(|c| c.is_whitespace() || "，。、,.!！？?…·•%@".contains(c))
    {
        return true;
    }
    false
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
    fn chat_wechat() {
        assert!(is_chat_app_name("微信"));
        assert!(is_chat_app_name("WeChat"));
        assert!(is_chat_app_name("企业微信"));
        assert!(!is_chat_app_name("Cursor"));
    }

    #[test]
    fn noise_cursor_follow_up_and_chrome() {
        assert!(is_agent_noise_text("Send follow-up"));
        assert!(is_agent_noise_text("end follow-up\n"));
        assert!(is_agent_noise_text("助手"));
        assert!(is_agent_noise_text(", Apple账户"));
        assert!(is_agent_noise_text("正在进入"));
        assert!(!is_agent_noise_text("我们的应用会有这么多启动"));
        assert!(!is_agent_noise_text("nihaoshijie"));
        assert!(!is_agent_noise_text("smart office data"));
    }
}
