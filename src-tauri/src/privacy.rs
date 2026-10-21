//! Secret redaction for commercial-safe capture + agent hygiene.

use regex::Regex;
use std::sync::OnceLock;

/// Redact credentials / tokens in plain text. Returns (text, did_redact).
pub fn redact_secrets(input: &str) -> (String, bool) {
    if input.is_empty() {
        return (String::new(), false);
    }
    let mut out = input.to_string();
    let mut changed = false;
    for re in secret_patterns() {
        if re.is_match(&out) {
            out = re.replace_all(&out, "[REDACTED]").into_owned();
            changed = true;
        }
    }
    (out, changed)
}

fn secret_patterns() -> &'static [Regex] {
    static PATS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATS.get_or_init(|| {
        let raw = [
            // DeskCap / generic bearer
            r"(?i)\bdc_[a-f0-9]{16,}\b",
            r"(?i)\bBearer\s+[A-Za-z0-9\-._~+/]+=*",
            // Cloud / SaaS tokens
            r"\bsk-[A-Za-z0-9]{16,}\b",
            r"\bghp_[A-Za-z0-9]{20,}\b",
            r"\bgho_[A-Za-z0-9]{20,}\b",
            r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
            r"\bAKIA[0-9A-Z]{12,}\b",
            // Key-value style
            r"(?i)\b(api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|secret[_-]?key|auth[_-]?token)\s*[:=]\s*\S+",
            r"(?i)\b(password|passwd|pwd|口令|密码)\s*[:=：]\s*\S+",
            // PEM blocks
            r"-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z0-9 ]*PRIVATE KEY-----",
            r"-----BEGIN OPENSSH PRIVATE KEY-----[\s\S]*?-----END OPENSSH PRIVATE KEY-----",
        ];
        raw.iter()
            .filter_map(|p| Regex::new(p).ok())
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_deskcap_token() {
        let (t, ok) = redact_secrets("token=dc_3d8c014210524b57bf99a543d0e067da ok");
        assert!(ok);
        assert!(t.contains("[REDACTED]"));
        assert!(!t.contains("dc_3d8c"));
    }

    #[test]
    fn keeps_normal_prose() {
        let (t, ok) = redact_secrets("我们讨论一下 token 这个词怎么翻译");
        assert!(!ok);
        assert!(t.contains("token"));
    }

    #[test]
    fn redacts_password_kv() {
        let (t, ok) = redact_secrets("password: hunter2\n下一条");
        assert!(ok);
        assert!(t.contains("[REDACTED]"));
        assert!(!t.contains("hunter2"));
    }
}
