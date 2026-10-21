use crate::engine::CaptureEngine;
use crate::event::{CaptureEvent, SessionStatus};
use crate::netinfo::{agent_api_urls, lan_bind_addr, token_eq};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use std::sync::Arc;

/// Agent-facing HTTP API is **read-only**.
/// Binds `0.0.0.0` so Hermes bots on the office LAN can pull with Bearer token.
/// Start/stop/config/maintenance stay on Tauri UI commands so agents cannot mutate capture data.
pub async fn serve(engine: Arc<CaptureEngine>) {
    let port = engine.config.read().agent_api_port;
    let app = Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/events", get(events))
        .route("/knowledge", get(knowledge))
        .route("/knowledge/day", get(knowledge_day))
        .route("/knowledge/day/summary", get(knowledge_day_summary))
        .route("/days", get(list_days))
        .route("/config", get(get_config_public))
        // DeskCap IME companion only (Bearer). Not for Hermes agents to invent history.
        .route("/ime/commit", post(ime_commit))
        .with_state(engine);

    let addr = lan_bind_addr(port);
    let (lan, loopback, _) = agent_api_urls(port);
    eprintln!(
        "DeskCap agent API (read-only, LAN) bind {addr} loopback={loopback} lan={lan} (Bearer required)"
    );
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(err) => {
            eprintln!("agent API bind failed: {err}");
            return;
        }
    };
    if let Err(err) = axum::serve(listener, app).await {
        eprintln!("agent API stopped: {err}");
    }
}

fn unauthorized() -> (StatusCode, String) {
    (
        StatusCode::UNAUTHORIZED,
        "missing or invalid Authorization: Bearer <token>".into(),
    )
}

fn check_auth(engine: &CaptureEngine, headers: &HeaderMap) -> Result<(), (StatusCode, String)> {
    let expected = engine.config.read().agent_api_token.clone();
    if expected.is_empty() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "agent API token not initialized".into(),
        ));
    }
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .unwrap_or("")
        .trim();
    if token.is_empty() || !token_eq(token, &expected) {
        return Err(unauthorized());
    }
    Ok(())
}

fn filter_since(items: Vec<CaptureEvent>, since: Option<f64>) -> Vec<CaptureEvent> {
    match since {
        Some(s) => items.into_iter().filter(|e| e.wall_ts > s).collect(),
        None => items,
    }
}

fn next_since(items: &[CaptureEvent]) -> Option<f64> {
    items.iter().map(|e| e.wall_ts).fold(None, |acc, t| {
        Some(acc.map(|a: f64| a.max(t)).unwrap_or(t))
    })
}

fn storage_flags() -> serde_json::Value {
    serde_json::json!({
        "local_only": true,
        "cloud_upload": false,
        "lan_access": true,
        "writable": false,
    })
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({
        "ok": true,
        "service": "deskcap",
        "version": env!("CARGO_PKG_VERSION"),
        "local_only": true,
        "lan_access": true,
        "bind": "0.0.0.0",
        "auth": "bearer",
        "cloud_upload": false,
        "agent_api_mode": "read_only",
        "mutations": "ui_only",
        "skill": "skills/deskcap/SKILL.md",
        "skill_in_app": true,
        "limits": {
            "knowledge_default": 100,
            "knowledge_max": 500,
            "knowledge_recent_cap": 80,
            "knowledge_day_default": 500,
            "knowledge_day_max": 2000,
            "events_max": 200
        },
        "recommend": {
            "volume_first": "/knowledge/day/summary?date=",
            "detail": "/knowledge/day?date=&since=&limit=80"
        }
    }))
}

async fn status(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
) -> Result<Json<SessionStatus>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    Ok(Json(engine.status()))
}

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
    /// Exclusive lower bound on `wall_ts` for incremental sync.
    since: Option<f64>,
}

#[derive(Deserialize)]
struct DayQuery {
    date: Option<String>,
    limit: Option<usize>,
    since: Option<f64>,
}

async fn events(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    let items = filter_since(
        engine.knowledge(q.limit.unwrap_or(50).min(200)),
        q.since,
    );
    let next = next_since(&items);
    Ok(Json(serde_json::json!({
        "count": items.len(),
        "since": q.since,
        "next_since": next,
        "items": items,
        "writable": false,
        "lan_access": true,
        "note": "Text events only (typing/clipboard). mouse_click counts → GET /knowledge/day/summary"
    })))
}

async fn knowledge(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    let items = filter_since(
        engine.knowledge(q.limit.unwrap_or(100).min(500)),
        q.since,
    );
    let next = next_since(&items);
    let mut body = serde_json::json!({
        "kind": "text_knowledge",
        "count": items.len(),
        "since": q.since,
        "next_since": next,
        "recent_cap": 80,
        "note": "Recent in-memory ring only (~80). For full day history use /knowledge/day. Pass since=next_since for incremental sync.",
        "items": items,
        "usage": "Read-only. Items are typing + clipboard_* with data.text. Deduplicate by wall_ts+type+app in agent DB. Redact secrets before storing. Agents must not modify DeskCap storage. Hermes bots: use this PC LAN IP + this user's Bearer token only."
    });
    if let Some(obj) = body.as_object_mut() {
        for (k, v) in storage_flags().as_object().unwrap() {
            obj.insert(k.clone(), v.clone());
        }
    }
    Ok(Json(body))
}

async fn knowledge_day(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
    Query(q): Query<DayQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    let day = q
        .date
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    let cap = q.limit.unwrap_or(500).min(2000);
    let items = engine.knowledge_day_since(&day, q.since, cap);
    let next = next_since(&items);
    let path_hint = {
        #[cfg(target_os = "macos")]
        {
            format!("~/Library/Application Support/DeskCap/days/{day}.jsonl")
        }
        #[cfg(target_os = "windows")]
        {
            format!("%APPDATA%\\DeskCap\\days\\{day}.jsonl")
        }
        #[cfg(target_os = "linux")]
        {
            format!("~/.local/share/DeskCap/days/{day}.jsonl")
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            format!("DeskCap/days/{day}.jsonl")
        }
    };
    Ok(Json(serde_json::json!({
        "date": day,
        "local_only": true,
        "lan_access": true,
        "cloud_upload": false,
        "writable": false,
        "count": items.len(),
        "since": q.since,
        "next_since": next,
        "items": items,
        "path_hint": path_hint,
        "note": "Text detail only (typing + clipboard_*). Do NOT use this for click streams — use /knowledge/day/summary (by_app.mouse_click). Page with since=next_since. Max 2000/request.",
        "summary_url": format!("/knowledge/day/summary?date={day}"),
        "kinds": ["typing", "clipboard_paste", "clipboard_copy", "clipboard_cut"]
    })))
}

#[derive(Deserialize)]
struct SummaryQuery {
    date: Option<String>,
}

async fn knowledge_day_summary(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    let day = q
        .date
        .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
    Ok(Json(engine.knowledge_day_summary(&day)))
}

async fn list_days(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    Ok(Json(serde_json::json!({ "days": engine.list_days(), "writable": false, "lan_access": true })))
}

async fn get_config_public(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    let cfg = engine.get_config();
    Ok(Json(serde_json::json!({
        "writable": false,
        "capture_keyboard": cfg.capture_keyboard,
        "capture_mouse": cfg.capture_mouse,
        "capture_clipboard": cfg.capture_clipboard,
        "screenshot_interval_secs": cfg.screenshot_interval_secs,
        "retention_days": cfg.retention_days,
        "max_disk_mb": cfg.max_disk_mb,
        "backup_enabled": cfg.backup_enabled,
        "launch_at_login": cfg.launch_at_login,
        "silent_mode": cfg.silent_mode,
        "schedule_enabled": cfg.schedule_enabled,
        "schedule_start": cfg.schedule_start,
        "schedule_end": cfg.schedule_end,
        "schedule_weekdays": cfg.schedule_weekdays,
        "agent_api_port": cfg.agent_api_port,
        "local_only": true,
        "lan_access": true,
        "bind": "0.0.0.0",
        "note": "Token never returned. Mutations via DeskCap UI only. Hermes: http://<this-pc-lan-ip>:port + Bearer from Agent tab. Day ledger: typing+clicks+clipboard."
    })))
}

#[derive(Debug, Deserialize)]
struct ImeCommitBody {
    text: String,
    #[serde(default)]
    app: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

/// Local DeskCap IME posts committed 汉字 here (Bearer = agent.token).
async fn ime_commit(
    State(engine): State<Arc<CaptureEngine>>,
    headers: HeaderMap,
    Json(body): Json<ImeCommitBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&engine, &headers)?;
    let text = body.text.trim().to_string();
    if text.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty text".into()));
    }
    if text.chars().count() > 2000 {
        return Err((StatusCode::BAD_REQUEST, "text too long".into()));
    }
    let n = engine.ingest_ime_commit(&text, body.app.as_deref(), body.source.as_deref());
    Ok(Json(serde_json::json!({ "ok": true, "ingested": n })))
}
