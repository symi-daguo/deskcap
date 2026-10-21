use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppContext {
    pub name: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureEvent {
    /// Seconds since session start (monotonic)
    pub ts: f64,
    /// Unix epoch seconds
    pub wall_ts: f64,
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<AppContext>,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionStatus {
    pub state: String,
    pub session_id: Option<String>,
    pub started_at: Option<f64>,
    pub elapsed: f64,
    pub event_count: u64,
    pub segment_path: Option<String>,
    pub current_app: Option<AppContext>,
    pub excluded_now: bool,
    pub platform: String,
    pub agent_api: String,
    /// Loopback URL (always available on the capture PC).
    #[serde(default)]
    pub agent_api_loopback: String,
    /// Detected LAN IPv4 for Hermes bots (`None` if offline / undetectable).
    #[serde(default)]
    pub agent_api_lan_ip: Option<String>,
    /// Bind address of the Agent API (`0.0.0.0` = LAN).
    #[serde(default)]
    pub agent_api_bind: String,
    pub agent_api_auth: String,
    /// No cloud upload; data stays on this PC (Hermes may pull over LAN with token).
    pub local_only: bool,
    /// Agent API accepts LAN connections (Bearer required).
    #[serde(default)]
    pub lan_access: bool,
    pub cloud_upload: bool,
    pub capture_keyboard: bool,
    pub capture_mouse: bool,
    pub capture_mouse_move: bool,
    pub capture_clipboard: bool,
    pub screenshot_interval_secs: u64,
    pub retention_days: u64,
    pub max_disk_mb: u64,
    pub sessions_bytes: u64,
    pub last_backup: Option<String>,
    /// macOS: Accessibility granted.
    pub accessibility_ok: bool,
    /// macOS 10.15+: Input Monitoring (required for CGEventTap listen).
    pub input_monitoring_ok: bool,
    /// Input tap / listener started successfully.
    pub input_listener_ok: bool,
    /// Whether current wall time is inside capture coverage window.
    pub within_schedule: bool,
    pub schedule_enabled: bool,
    /// Seconds since last key/click via EventTap (None = never).
    pub last_input_age_secs: Option<f64>,
    pub keydown_count: u64,
    pub mouse_click_count: u64,
    /// Same-button same-pixel clicks suppressed by CLICK_DEDUP_WINDOW.
    /// Indicates noisy / stuck input, not user intent.
    pub mouse_click_dedup: u64,
    pub ax_chars: u64,
    /// Absolute path of the running binary (for TCC path checks).
    pub exe_path: Option<String>,
    /// True when launched from `/Applications/DeskCap.app` (macOS TCC). Other OS: always true.
    pub from_applications: bool,
}

pub fn platform_name() -> String {
    #[cfg(target_os = "macos")]
    {
        "macos".into()
    }
    #[cfg(target_os = "windows")]
    {
        "windows".into()
    }
    #[cfg(target_os = "linux")]
    {
        "linux".into()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "unknown".into()
    }
}

pub fn running_exe_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
}

#[cfg(target_os = "macos")]
pub fn launched_from_applications() -> bool {
    let Some(exe) = std::env::current_exe().ok() else {
        return false;
    };
    exe.to_string_lossy().contains("/Applications/DeskCap.app/")
}

#[cfg(not(target_os = "macos"))]
pub fn launched_from_applications() -> bool {
    true
}
