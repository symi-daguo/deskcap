mod api;
mod apps;
mod clipboard;
mod config;
mod daybook;
mod engine;
mod event;
mod ime;
mod ime_keepalive;
mod logutil;
mod maintenance;
mod netinfo;
mod privacy;
mod typing;
mod writer;

#[cfg(target_os = "macos")]
mod input_macos;
#[cfg(target_os = "macos")]
mod ax_text;
#[cfg(target_os = "macos")]
mod wechat_mac;
#[cfg(windows)]
mod input_windows;
#[cfg(windows)]
mod uia_text;
#[cfg(windows)]
mod win_focus;

use apps::InstalledApp;
use config::CaptureConfig;
use engine::CaptureEngine;
use event::SessionStatus;
use maintenance::MaintenanceReport;
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};
#[cfg(target_os = "macos")]
use tauri::{ActivationPolicy, RunEvent};

fn show_main_window(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(ActivationPolicy::Regular);
    }
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

fn hide_to_menu_bar(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(ActivationPolicy::Accessory);
    }
}

#[tauri::command]
fn get_status(engine: tauri::State<'_, Arc<CaptureEngine>>) -> SessionStatus {
    engine.status()
}

/// Frontend calls once after JS loads and IPC works — proves UI is not a blank WebView.
#[tauri::command]
fn mark_ui_ready(engine: tauri::State<'_, Arc<CaptureEngine>>) -> Result<(), String> {
    // data_dir may be .../DeskCap/sessions — put marker at the DeskCap root.
    let data = engine.get_config().data_dir;
    let root = data
        .parent()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) == Some("DeskCap"))
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| data.clone());
    std::fs::create_dir_all(&root).map_err(|e| format!("mkdir {}: {e}", root.display()))?;
    let path = root.join("ui_ready");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    std::fs::write(&path, format!("{ts}\n")).map_err(|e| format!("write {}: {e}", path.display()))?;
    eprintln!("DeskCap: UI ready ({ts}) -> {}", path.display());
    Ok(())
}

#[tauri::command]
fn start_capture(engine: tauri::State<'_, Arc<CaptureEngine>>) -> Result<SessionStatus, String> {
    engine.start().map_err(|e| e.to_string())
}

#[tauri::command]
fn stop_capture(engine: tauri::State<'_, Arc<CaptureEngine>>) -> SessionStatus {
    engine.stop()
}

#[tauri::command]
fn get_config(engine: tauri::State<'_, Arc<CaptureEngine>>) -> CaptureConfig {
    engine.get_config()
}

#[tauri::command]
fn set_config(
    engine: tauri::State<'_, Arc<CaptureEngine>>,
    config: CaptureConfig,
) -> Result<CaptureConfig, String> {
    engine.set_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
fn recent_events(
    engine: tauri::State<'_, Arc<CaptureEngine>>,
    limit: Option<usize>,
) -> Vec<event::CaptureEvent> {
    engine.recent_events(limit.unwrap_or(30).min(80))
}

#[tauri::command]
fn rotate_api_token(engine: tauri::State<'_, Arc<CaptureEngine>>) -> Result<CaptureConfig, String> {
    let mut cfg = engine.get_config();
    cfg.agent_api_token.clear();
    engine.set_config(cfg).map_err(|e| e.to_string())
}

/// 0.6.51 — 读写 IME 候选译词语言（`GlossaryLanguage`）。
///
/// 译词语言存在 IME 自己的 UserDefaults 域（`com.office.deskcap.inputmethod.symi`），
/// 不在 DeskCap 主配置里 —— IME 是独立进程，用 `defaults(1)` 读写它的域，
/// IME 侧 0.6.48 起已缓存化、`invalidate()` 由词表文件变化触发，实时生效无需重启。
///
/// 之前只能命令行 `defaults write ... GlossaryLanguage -string en`，
/// 用户要求「设置输入法的界面也要匹配正确」——现在 UI 直接给开关。
#[tauri::command]
fn get_ime_glossary_language() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("defaults")
            .args(["read", "com.office.deskcap.inputmethod.symi", "GlossaryLanguage"])
            .output()
            .map_err(|e| e.to_string())?;
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(if s.is_empty() { "off".into() } else { s })
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok("off".into())
    }
}

#[tauri::command]
fn set_ime_glossary_language(lang: String) -> Result<(), String> {
    // 白名单：只允许已知语言码与 off，防止把任意字符串塞进 defaults。
    let allowed = ["off", "en", "ja", "es", "fr", "de", "ko"];
    if !allowed.contains(&lang.as_str()) {
        return Err(format!("unsupported glossary language: {lang}"));
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = std::process::Command::new("defaults");
        cmd.args(["write", "com.office.deskcap.inputmethod.symi", "GlossaryLanguage", "-string", &lang]);
        if lang == "off" {
            // defaults delete 更干净：读回时空 → IME 侧解析为 off。
            let _ = std::process::Command::new("defaults")
                .args(["delete", "com.office.deskcap.inputmethod.symi", "GlossaryLanguage"])
                .output();
        } else {
            let st = cmd.status().map_err(|e| e.to_string())?;
            if !st.success() {
                return Err("defaults write failed".into());
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = lang;
        Ok(())
    }
}

#[tauri::command]
fn run_maintenance(engine: tauri::State<'_, Arc<CaptureEngine>>) -> MaintenanceReport {
    engine.run_maintenance_now()
}

#[tauri::command]
fn list_days(engine: tauri::State<'_, Arc<CaptureEngine>>) -> Vec<String> {
    engine.list_days()
}

/// 0.6.56 — 日期条需要区分「有数据 / 无数据」并支持跳任意日期。
/// `list_days` 为向后兼容保持 `Vec<String>`（`/days` HTTP API 也是它）；
/// 这里返回结构化信息：`days`（含保底今天，降序）+ `with_data`（真实存在
/// 日账文件的日期）。前端用后者给按钮上色，前者铺列表。
#[tauri::command]
fn list_days_meta(engine: tauri::State<'_, Arc<CaptureEngine>>) -> serde_json::Value {
    let all = engine.list_days();
    let cfg = engine.get_config();
    let with_data = daybook::list_days_on_disk(&cfg.data_dir);
    serde_json::json!({ "days": all, "with_data": with_data })
}

#[tauri::command]
fn knowledge_day(
    engine: tauri::State<'_, Arc<CaptureEngine>>,
    date: Option<String>,
    limit: Option<usize>,
) -> Vec<event::CaptureEvent> {
    let day = date.unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    engine.knowledge_day(&day, limit.unwrap_or(500).min(2000))
}

#[tauri::command]
fn list_installed_apps() -> Vec<InstalledApp> {
    apps::list_installed_apps()
}

#[tauri::command]
fn export_day(
    engine: tauri::State<'_, Arc<CaptureEngine>>,
    day: String,
    start_hhmm: Option<String>,
    end_hhmm: Option<String>,
) -> Result<String, String> {
    engine
        .export_day_range(&day, start_hhmm, end_hhmm)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        crate::input_macos::request_input_permissions();
        // macOS 15+ Settings URL; legacy fallback below if spawn fails silently.
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_ListenEvent")
            .spawn();
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility")
            .spawn();
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
            .spawn();
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("only available on macOS".into())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Do NOT bootstrap hooks here — a second process must exit via
    // single-instance before installing WH_KEYBOARD_LL / EventTap.
    let engine = CaptureEngine::new();
    let silent_boot = std::env::args().any(|a| a == "--silent" || a == "--autostart")
        && engine.get_config().silent_mode;

    let engine_for_setup = Arc::clone(&engine);
    tauri::Builder::default()
        // Must be first: second launch focuses existing app and exits before setup.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Runs on the *existing* primary when a second launch is attempted.
            crate::logutil::info("single-instance: focus existing DeskCap");
            show_main_window(app);
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--silent", "--autostart"])
                .build(),
        )
        .manage(Arc::clone(&engine))
        .setup(move |app| {
            let engine = Arc::clone(&engine_for_setup);
            // Primary instance only — install input hooks + heartbeat here.
            engine.bootstrap();

            tauri::async_runtime::spawn(async move {
                api::serve(engine).await;
            });

            // Tray: macOS = **one** thin「亖」template glyph (no title).
            // title + empty icon reserved an extra slot to the left of the text.
            let show_i = MenuItem::with_id(app, "show", "打开工作台", true, None::<&str>)?;
            let toggle_i =
                MenuItem::with_id(app, "toggle", "开始 / 停止采集", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let quit_i = MenuItem::with_id(app, "quit", "退出 DeskCap", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &toggle_i, &sep, &quit_i])?;
            #[cfg(target_os = "macos")]
            let tray_icon = Image::from_bytes(include_bytes!("../icons/trayTemplate.png"))
                .map_err(|e| format!("tray icon: {e}"))?;
            #[cfg(not(target_os = "macos"))]
            let tray_icon = Image::from_bytes(include_bytes!("../icons/tray.png"))
                .map_err(|e| format!("tray icon: {e}"))?;
            let engine_tray = Arc::clone(&engine_for_setup);
            // 0.6.52 修（用户反馈：深色菜单栏下「亖」是黑色粗体看不清）——
            // trayTemplate.png 是纯黑 glyph，macOS 的 template image 机制要求
            // 显式 isTemplate，系统才按菜单栏明暗自动渲染黑/白（深色栏下白色细体）。
            // 漏了这行，黑 glyph 原样贴上去，深色栏下不可读。
            // 非 macOS 的 tray.png 是彩色 logo，不吃 template。
            let tray_builder = TrayIconBuilder::with_id("deskcap-tray")
                .icon(tray_icon)
                .menu(&menu);
            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder.icon_as_template(true);
            let tray_builder = tray_builder
                .tooltip("DeskCap · by SYMI")
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "show" => show_main_window(app),
                    "toggle" => {
                        if engine_tray.is_capturing() {
                            engine_tray.stop();
                        } else if let Err(e) = engine_tray.start() {
                            eprintln!("DeskCap tray start: {e}");
                        }
                        show_main_window(app);
                    }
                    "quit" => {
                        engine_tray.stop();
                        crate::logutil::info("quit requested");
                        app.exit(0);
                        // Hooks / Raw Input pumps can keep the process alive on Windows.
                        std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(600));
                            std::process::exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            #[cfg(target_os = "macos")]
            let tray_builder = tray_builder.icon_as_template(true);
            let _tray = tray_builder.build(app)?;

            let handle = app.handle().clone();
            let silent = silent_boot;
            if silent {
                hide_to_menu_bar(&handle);
            } else {
                show_main_window(&handle);
                if let Some(win) = handle.get_webview_window("main") {
                    let _ = win.set_min_size(Some(tauri::Size::Logical(tauri::LogicalSize::new(
                        960.0, 640.0,
                    ))));
                    let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(1280.0, 800.0)));
                    let _ = win.center();
                }
            }

            // Resume capture for 24/7 office use when user already consented.
            let engine = Arc::clone(&engine_for_setup);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let cfg = engine.get_config();
                if cfg.privacy_consented && !engine.is_capturing() {
                    if let Err(e) = engine.start() {
                        eprintln!("DeskCap auto-start: {e}");
                    } else {
                        eprintln!("DeskCap: auto-started capture (privacy consented)");
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Keep agent API + menu bar — hide to Accessory, do not quit.
                api.prevent_close();
                let app = window.app_handle().clone();
                hide_to_menu_bar(&app);
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            mark_ui_ready,
            start_capture,
            stop_capture,
            get_config,
            set_config,
            recent_events,
            rotate_api_token,
            get_ime_glossary_language,
            set_ime_glossary_language,
            list_days_meta,
            run_maintenance,
            list_days,
            knowledge_day,
            list_installed_apps,
            export_day,
            open_accessibility_settings
        ])
        .build(tauri::generate_context!())
        .expect("error while building DeskCap")
        .run(|app_handle, event| {
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
            {
                let _ = has_visible_windows;
                show_main_window(app_handle);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app_handle, event);
            }
        });
}
