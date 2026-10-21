use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CaptureConfig {
    pub capture_keyboard: bool,
    pub capture_mouse: bool,
    /// Mouse moves are high-volume; off by default for agent-friendly logs.
    pub capture_mouse_move: bool,
    /// Capture ⌘C/⌘V/⌘X (Ctrl on Win/Linux) plain-text clipboard payloads.
    #[serde(default = "default_true")]
    pub capture_clipboard: bool,
    /// User-managed exclusions (merged with locked payment rules).
    pub exclude_apps: Vec<String>,
    /// 0 = screenshots disabled (default: safer for 24/7 office use)
    pub screenshot_interval_secs: u64,
    pub segment_duration_secs: u64,
    pub data_dir: PathBuf,
    pub agent_api_port: u16,
    /// Bearer token for local Agent API. Never leave empty.
    pub agent_api_token: String,
    /// DeskCap never uploads; this flag is informational and always true when saved.
    pub local_only: bool,
    /// Delete session folders older than N days.
    pub retention_days: u64,
    /// Soft cap for sessions directory size.
    pub max_disk_mb: u64,
    /// Write a daily local backup jsonl under ../backups.
    pub backup_enabled: bool,
    /// Launch DeskCap when the user logs in.
    pub launch_at_login: bool,
    /// When launched via login / `--silent`, keep window hidden until Dock reopen.
    pub silent_mode: bool,
    /// Privacy notice accepted (persisted — not shown every page).
    pub privacy_consented: bool,
    /// Limit capture to a daily time window (24/7 when false).
    pub schedule_enabled: bool,
    /// Local time HH:MM inclusive start.
    pub schedule_start: String,
    /// Local time HH:MM exclusive end (supports overnight if end < start).
    pub schedule_end: String,
    /// ISO weekdays 1=Mon … 7=Sun. Empty = every day.
    pub schedule_weekdays: Vec<u8>,
    /// HTTPS URL of update manifest JSON. Empty = OTA check disabled.
    pub update_manifest_url: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("DeskCap")
            .join("sessions");
        Self {
            capture_keyboard: true,
            capture_mouse: true,
            capture_mouse_move: false,
            capture_clipboard: true,
            exclude_apps: default_user_exclude_apps(),
            screenshot_interval_secs: 0,
            segment_duration_secs: 600,
            data_dir,
            agent_api_port: 8765,
            agent_api_token: new_api_token(),
            local_only: true,
            retention_days: 14,
            max_disk_mb: 1024,
            backup_enabled: true,
            // macOS menu-bar utility defaults: login item + no window on boot.
            launch_at_login: true,
            silent_mode: true,
            privacy_consented: false,
            schedule_enabled: false,
            schedule_start: "09:00".into(),
            schedule_end: "18:30".into(),
            schedule_weekdays: vec![1, 2, 3, 4, 5],
            update_manifest_url: String::new(),
        }
    }
}

fn new_api_token() -> String {
    format!("dc_{}", Uuid::new_v4().simple())
}

fn default_true() -> bool {
    true
}

/// User-editable defaults (payment apps). Locked title/app rules still always apply.
fn default_user_exclude_apps() -> Vec<String> {
    vec![
        "Alipay".into(),
        "支付宝".into(),
        "云闪付".into(),
        "UnionPay".into(),
        "PayPal".into(),
        "Wallet".into(),
        "Apple Wallet".into(),
        "招商银行".into(),
        "工商银行".into(),
        "建设银行".into(),
        "农业银行".into(),
        "交通银行".into(),
        "中国银行".into(),
        "民生银行".into(),
        "浦发银行".into(),
        "兴业银行".into(),
        "中信银行".into(),
        "Bank of".into(),
        "Chase".into(),
        "Cash App".into(),
        "Venmo".into(),
        "京东金融".into(),
        "理财通".into(),
        "微信支付".into(),
    ]
}

/// Always-on: cannot be disabled by config PUT (payment / banking / secure entry).
/// DeskCap self-exclude is **not** listed here — it uses exact app name / binary path only,
/// so Cursor titles like `deskcap - Cursor` (folder name) are never title-matched.
fn locked_app_patterns() -> &'static [&'static str] {
    &[
        "alipay",
        "支付宝",
        "云闪付",
        "unionpay",
        "paypal",
        "apple wallet",
        "wallet",
        "招商银行",
        "工商银行",
        "建设银行",
        "农业银行",
        "交通银行",
        "中国银行",
        "民生银行",
        "浦发银行",
        "兴业银行",
        "中信银行",
        "京东金融",
        "理财通",
        "微信支付",
        "cash app",
        "venmo",
        "chase",
    ]
}

/// Window-title heuristics (covers WeChat/企业微信 in-app pay & password UIs).
fn locked_title_patterns() -> &'static [&'static str] {
    &[
        "支付",
        "付款",
        "转账",
        "红包",
        "收款",
        "扫码付",
        "确认支付",
        "收银台",
        "银行卡",
        "理财",
        "payment",
        "checkout",
        "pay now",
        "wechat pay",
        "支付宝",
        "密码",
        "password",
        "passcode",
        "安全键盘",
        "secure keyboard",
        "pin code",
        "输入支付密码",
        "请输入密码",
    ]
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

/// True only for DeskCap's own binary path (not arbitrary paths containing the letters).
fn path_is_deskcap_self(path: &str) -> bool {
    let p = path.replace('\\', "/").to_lowercase();
    let base = p.rsplit('/').next().unwrap_or("");
    base == "deskcap.exe"
        || base == "deskcap"
        || p.ends_with("/deskcap.app/contents/macos/deskcap")
        || p.contains("/deskcap.app/")
}

/// Exact product process/app name only — never substring (avoids `deskcap - Cursor` titles).
fn app_name_is_deskcap_self(app_name: &str) -> bool {
    let n = app_name.trim().to_lowercase();
    n == "deskcap" || n == "deskcap.exe"
}

/// IDEs / editors — capture even if the window title mentions 密码/支付 in doc text.
fn is_work_capture_app(app_name: &str) -> bool {
    let n = app_name.to_lowercase();
    n.contains("cursor")
        || n.contains("code")
        || n.contains("notepad")
        || n.contains("sublime")
        || n.contains("word")
        || n.contains("excel")
        || n.contains("winword")
        || n.contains("trae")
        || n.contains("workbuddy")
        || n.contains("terminal")
        || n.contains("powershell")
        || n.contains("cmd.exe")
}

impl CaptureConfig {
    /// Exclude by app name, window title, and optional bundle path. Locked rules always win.
    pub fn is_excluded(&self, app_name: &str, title: &str) -> bool {
        self.is_excluded_path(app_name, title, None)
    }

    pub fn is_excluded_path(&self, app_name: &str, title: &str, path: Option<&str>) -> bool {
        // Basename / exact name only — never substring-match titles or folder names.
        if path.is_some_and(path_is_deskcap_self) || app_name_is_deskcap_self(app_name) {
            return true;
        }
        for pat in locked_app_patterns() {
            if contains_ci(app_name, pat) {
                return true;
            }
        }
        // Title heuristics (支付/密码) are for browsers & chat pay sheets.
        // Do NOT apply to IDEs — Cursor titles often contain user text like「密码」.
        if !is_work_capture_app(app_name) {
            for pat in locked_title_patterns() {
                if contains_ci(title, pat) {
                    return true;
                }
            }
        }
        // User exclude list = **app name only**. Matching titles caused false skips when
        // the folder/project was named deskcap (e.g. Cursor: `deskcap - Cursor`).
        self.exclude_apps
            .iter()
            .any(|pat| !pat.is_empty() && contains_ci(app_name, pat))
    }

    pub fn ensure_secure(mut self) -> Self {
        self.local_only = true;
        if self.agent_api_token.trim().is_empty() {
            self.agent_api_token = new_api_token();
        }
        if self.agent_api_port == 0 {
            self.agent_api_port = 8765;
        }
        self.capture_mouse_move = false;
        // Full-screen PNGs are ~25MB each — keep off unless user explicitly sets >= 60s.
        if self.screenshot_interval_secs > 0 && self.screenshot_interval_secs < 60 {
            self.screenshot_interval_secs = 0;
        }
        if self.retention_days == 0 {
            self.retention_days = 14;
        }
        if self.max_disk_mb < 100 {
            self.max_disk_mb = 1024;
        }
        if !Self::valid_hhmm(&self.schedule_start) {
            self.schedule_start = "09:00".into();
        }
        if !Self::valid_hhmm(&self.schedule_end) {
            self.schedule_end = "18:30".into();
        }
        self.schedule_weekdays.retain(|d| (1..=7).contains(d));
        self.schedule_weekdays.sort_unstable();
        self.schedule_weekdays.dedup();
        // DeskCap is hard-excluded by exact name/path; drop stale list entries
        // (older builds put "deskcap" here and title-matched folder names).
        self.exclude_apps
            .retain(|e| !e.eq_ignore_ascii_case("deskcap"));
        // Merge locked apps into visible exclude list so UI shows them.
        for pat in locked_app_patterns() {
            let already = self
                .exclude_apps
                .iter()
                .any(|e| e.eq_ignore_ascii_case(pat) || e == pat);
            if !already {
                self.exclude_apps.push((*pat).to_string());
            }
        }
        self
    }

    pub fn load_or_default(path: &PathBuf) -> Self {
        if let Ok(raw) = std::fs::read_to_string(path) {
            if let Ok(cfg) = serde_json::from_str::<CaptureConfig>(&raw) {
                let cfg = cfg.ensure_secure();
                let _ = cfg.save(path);
                return cfg;
            }
        }
        let cfg = Self::default().ensure_secure();
        let _ = cfg.save(path);
        cfg
    }

    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
            }
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        std::fs::rename(&tmp, path)?;
        // Convenience token file for local agents (same mode).
        if let Some(parent) = path.parent() {
            let token_path = parent.join("agent.token");
            std::fs::write(&token_path, self.agent_api_token.as_bytes())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600));
            }
        }
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("DeskCap")
            .join("config.json")
    }

    fn valid_hhmm(s: &str) -> bool {
        let parts: Vec<_> = s.split(':').collect();
        if parts.len() != 2 {
            return false;
        }
        let Ok(h) = parts[0].parse::<u32>() else {
            return false;
        };
        let Ok(m) = parts[1].parse::<u32>() else {
            return false;
        };
        h < 24 && m < 60
    }

    fn minutes_of(s: &str) -> Option<u32> {
        let parts: Vec<_> = s.split(':').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(parts[0].parse::<u32>().ok()? * 60 + parts[1].parse::<u32>().ok()?)
    }

    /// Whether wall-clock now is inside the configured capture coverage window.
    pub fn within_schedule(&self) -> bool {
        if !self.schedule_enabled {
            return true;
        }
        let now = chrono::Local::now();
        let weekday = now.weekday().number_from_monday() as u8;
        if !self.schedule_weekdays.is_empty() && !self.schedule_weekdays.contains(&weekday) {
            return false;
        }
        let Some(start) = Self::minutes_of(&self.schedule_start) else {
            return true;
        };
        let Some(end) = Self::minutes_of(&self.schedule_end) else {
            return true;
        };
        let cur = now.hour() * 60 + now.minute();
        if start == end {
            return true;
        }
        if start < end {
            cur >= start && cur < end
        } else {
            // Overnight window e.g. 22:00–06:00
            cur >= start || cur < end
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_payment_apps_keeps_wechat_chat() {
        let cfg = CaptureConfig::default().ensure_secure();
        assert!(cfg.is_excluded("支付宝", ""));
        assert!(cfg.is_excluded("Alipay", ""));
        assert!(cfg.is_excluded("招商银行", "首页"));
        assert!(!cfg.is_excluded("WeChat", "文件传输助手"));
        assert!(!cfg.is_excluded("微信", "张三"));
        assert!(cfg.is_excluded("DeskCap", ""));
        assert!(cfg.is_excluded("deskcap", "settings"));
        assert!(cfg.is_excluded_path(
            "Cursor",
            "",
            Some("/Applications/DeskCap.app/Contents/MacOS/deskcap")
        ));
        assert!(!cfg.is_excluded_path(
            "Cursor",
            "",
            Some(r"C:\Users\me\AppData\Local\Programs\cursor\Cursor.exe")
        ));
        assert!(cfg.is_excluded_path(
            "Something",
            "",
            Some(r"C:\Users\me\AppData\Local\DeskCap\deskcap.exe")
        ));
    }

    #[test]
    fn cursor_folder_named_deskcap_is_not_excluded() {
        let cfg = CaptureConfig::default().ensure_secure();
        // Real dogfood failure: exclude_apps contained "deskcap" and matched **title**.
        assert!(!cfg.is_excluded("Cursor", "deskcap - Cursor"));
        assert!(!cfg.is_excluded_path(
            "Cursor",
            "deskcap - Cursor",
            Some(r"C:\Users\me\AppData\Local\Programs\cursor\Cursor.exe")
        ));
        assert!(!cfg.is_excluded("Cursor", "Desktop-Capture-Agent — deskcap"));
        // Title used as name fallback must still not self-exclude via substring.
        assert!(!cfg.is_excluded("deskcap - Cursor", "deskcap - Cursor"));
    }

    #[test]
    fn excludes_wechat_payment_and_password_titles() {
        let cfg = CaptureConfig::default().ensure_secure();
        assert!(cfg.is_excluded("WeChat", "微信支付"));
        assert!(cfg.is_excluded("微信", "确认支付"));
        assert!(cfg.is_excluded("企业微信", "红包"));
        assert!(!cfg.is_excluded("Cursor", "如何重置密码"));
        assert!(!cfg.is_excluded_path(
            "Cursor",
            "password reset notes",
            Some(r"C:\Users\me\AppData\Local\Programs\cursor\Cursor.exe")
        ));
        assert!(cfg.is_excluded("Chrome", "Checkout - Stripe"));
    }

    #[test]
    fn token_always_present() {
        let mut cfg = CaptureConfig::default();
        cfg.agent_api_token.clear();
        cfg = cfg.ensure_secure();
        assert!(cfg.agent_api_token.starts_with("dc_"));
        assert!(cfg.local_only);
    }
}
