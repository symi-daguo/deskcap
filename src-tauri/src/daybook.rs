//! Per-day knowledge ledger for Agent retrieval (append-only, lean).
//!
//! Hot path keeps one open BufWriter and flushes in batches — never
//! open/chmod/fsync on every click (SSD wear + stalls under office load).

use crate::event::CaptureEvent;
use chrono::Local;
use parking_lot::Mutex;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn days_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or(Path::new("."))
        .join("days")
}

pub fn exports_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or(Path::new("."))
        .join("exports")
}

fn is_knowledge_event(event: &CaptureEvent) -> bool {
    matches!(
        event.event_type.as_str(),
        "typing" | "mouse_click" | "clipboard_paste" | "clipboard_copy" | "clipboard_cut"
    )
}

struct DaySink {
    day: String,
    file: BufWriter<File>,
    pending: u32,
    last_flush: Instant,
}

static SINK: Mutex<Option<DaySink>> = Mutex::new(None);

const FLUSH_LINES: u32 = 16;
const FLUSH_IDLE_MS: u128 = 500;

/// Only typing + clicks go to the day ledger (keeps disk lean for 24/7 use).
/// Session JSONL still holds the full stream including app_focus.
pub fn append_event(data_dir: &Path, event: &CaptureEvent) {
    if !is_knowledge_event(event) {
        return;
    }
    let Ok(line) = serde_json::to_string(event) else {
        crate::logutil::warn("daybook: serialize failed");
        return;
    };
    let day = Local::now().format("%Y-%m-%d").to_string();
    let mut guard = SINK.lock();
    if guard.as_ref().map(|s| s.day.as_str()) != Some(day.as_str()) {
        if let Some(mut old) = guard.take() {
            if old.file.flush().is_err() {
                crate::logutil::warn("daybook: flush previous day failed");
            }
        }
        let dir = days_dir(data_dir);
        if fs::create_dir_all(&dir).is_err() {
            crate::logutil::warn("daybook: create days dir failed");
            return;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
        }
        let path = dir.join(format!("{day}.jsonl"));
        let fresh = !path.exists();
        let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
            crate::logutil::warn(&format!("daybook: open failed {}", path.display()));
            return;
        };
        if fresh {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
            }
        }
        *guard = Some(DaySink {
            day,
            file: BufWriter::with_capacity(32 * 1024, file),
            pending: 0,
            last_flush: Instant::now(),
        });
    }
    let Some(sink) = guard.as_mut() else {
        return;
    };
    if writeln!(sink.file, "{line}").is_err() {
        crate::logutil::warn("daybook: write failed — reopening sink");
        *guard = None;
        return;
    }
    sink.pending = sink.pending.saturating_add(1);
    if sink.pending >= FLUSH_LINES || sink.last_flush.elapsed().as_millis() >= FLUSH_IDLE_MS {
        if sink.file.flush().is_err() {
            crate::logutil::warn("daybook: batch flush failed — reopening sink");
            *guard = None;
            return;
        }
        sink.pending = 0;
        sink.last_flush = Instant::now();
    }
}

/// Push buffered day lines to disk (flush thread + stop + Agent reads).
pub fn flush() {
    if let Some(sink) = SINK.lock().as_mut() {
        if sink.file.flush().is_err() {
            crate::logutil::warn("daybook: flush failed");
            return;
        }
        sink.pending = 0;
        sink.last_flush = Instant::now();
    }
}

fn flush_if_today(day: &str) {
    let today = Local::now().format("%Y-%m-%d").to_string();
    if day == today {
        flush();
    }
}

pub fn read_day_page(
    data_dir: &Path,
    day: &str,
    since: Option<f64>,
    limit: usize,
) -> Vec<CaptureEvent> {
    // 0.6.71 — `day` 来自 HTTP 查询参数，直接拼进文件路径。此前只有写入路径
    // （import_day_jsonl）校验，读取路径**没有**：
    //   GET /knowledge/day?date=../sessions/xxx
    // 会让 days/../sessions/xxx.jsonl 被读出来 —— 会话文件含 app_focus 全量流，
    // 比日账更敏感。虽然需要 Bearer 令牌，仍属应堵的缺口。
    if !is_valid_day(day) {
        crate::logutil::warn("daybook: rejected invalid day");
        return Vec::new();
    }
    flush_if_today(day);
    let path = days_dir(data_dir).join(format!("{day}.jsonl"));
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut all = Vec::new();
    for line in raw.lines() {
        let Ok(ev) = serde_json::from_str::<CaptureEvent>(line) else {
            continue;
        };
        if let Some(s) = since {
            if ev.wall_ts <= s {
                continue;
            }
        }
        all.push(ev);
    }
    // Incremental pull: first N after `since` (agent paging).
    if since.is_some() {
        all.truncate(limit);
        return all;
    }
    // Full-day UI load: keep the **newest** N so the timeline isn't stuck on morning data.
    if all.len() > limit {
        let start = all.len() - limit;
        all.drain(..start);
    }
    all
}

/// Full-day aggregate for agents that only need volume (e.g. clicks per app), not every timestamp.

pub fn summarize_day(data_dir: &Path, day: &str) -> serde_json::Value {
    use std::collections::BTreeMap;
    flush_if_today(day);
    let path = days_dir(data_dir).join(format!("{day}.jsonl"));
    let Ok(raw) = fs::read_to_string(path) else {
        return serde_json::json!({
            "date": day,
            "totals": { "events": 0, "typing": 0, "mouse_click": 0, "clipboard": 0, "chars": 0 },
            "by_app": [],
            "hours": vec![0u32; 24],
        });
    };

    #[derive(Default)]
    struct AppAgg {
        mouse_click: u32,
        typing: u32,
        clipboard: u32,
        chars: u64,
    }

    let mut by_app: BTreeMap<String, AppAgg> = BTreeMap::new();
    let mut hours = [0u32; 24];
    let mut totals_typing = 0u32;
    let mut totals_click = 0u32;
    let mut totals_clip = 0u32;
    let mut totals_chars = 0u64;
    let mut totals_events = 0u32;

    for line in raw.lines() {
        let Ok(ev) = serde_json::from_str::<CaptureEvent>(line) else {
            continue;
        };
        if !is_knowledge_event(&ev) {
            continue;
        }
        let name = ev
            .app
            .as_ref()
            .map(|a| a.name.as_str())
            .unwrap_or("?")
            .to_string();
        if name.to_lowercase().contains("deskcap") {
            continue;
        }
        let path = ev
            .app
            .as_ref()
            .and_then(|a| a.path.as_deref())
            .unwrap_or("");
        if path.to_lowercase().contains("deskcap.app") {
            continue;
        }

        totals_events += 1;
        let h = chrono::DateTime::from_timestamp(ev.wall_ts as i64, 0)
            .map(|dt| {
                use chrono::Timelike;
                // local hour via Local
                let local: chrono::DateTime<chrono::Local> = dt.into();
                local.hour() as usize
            })
            .unwrap_or(0)
            .min(23);
        hours[h] += 1;

        let slot = by_app.entry(name).or_default();
        match ev.event_type.as_str() {
            "typing" => {
                totals_typing += 1;
                slot.typing += 1;
                let n = ev
                    .data
                    .get("char_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_else(|| {
                        ev.data
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(|s| s.chars().count() as u64)
                            .unwrap_or(0)
                    });
                totals_chars += n;
                slot.chars += n;
            }
            "mouse_click" => {
                totals_click += 1;
                slot.mouse_click += 1;
            }
            t if t.starts_with("clipboard_") => {
                totals_clip += 1;
                slot.clipboard += 1;
                let n = ev
                    .data
                    .get("char_count")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                totals_chars += n;
                slot.chars += n;
            }
            _ => {}
        }
    }

    let mut apps: Vec<_> = by_app
        .into_iter()
        .map(|(name, a)| {
            (
                a.mouse_click + a.typing + a.clipboard,
                serde_json::json!({
                    "name": name,
                    "mouse_click": a.mouse_click,
                    "typing": a.typing,
                    "clipboard": a.clipboard,
                    "chars": a.chars,
                    "events": a.mouse_click + a.typing + a.clipboard,
                }),
            )
        })
        .collect();
    apps.sort_by(|a, b| b.0.cmp(&a.0));
    let by_app: Vec<_> = apps.into_iter().map(|(_, v)| v).collect();

    serde_json::json!({
        "date": day,
        "local_only": true,
        "writable": false,
        "totals": {
            "events": totals_events,
            "typing": totals_typing,
            "mouse_click": totals_click,
            "clipboard": totals_clip,
            "chars": totals_chars,
        },
        "by_app": by_app,
        "hours": hours.to_vec(),
        "note": "Aggregate only — no per-event payloads. Prefer this when you only need activity volume (e.g. clicks per app). Use /knowledge/day for text detail with since paging.",
        "skill": "Use summary first; pull /knowledge/day only when text/content is required."
    })
}

/// 0.6.56 — 纯磁盘扫描：只列真实存在日账文件的日期（降序）。不作保底。
/// 给 `list_days_meta` 判断「哪几天有数据」用（前端日期条上色）。
pub fn list_days_on_disk(data_dir: &Path) -> Vec<String> {
    let dir = days_dir(data_dir);
    let mut days = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(day) = name.strip_suffix(".jsonl") {
                days.push(day.to_string());
            }
        }
    }
    days.sort();
    days.reverse();
    days
}

pub fn list_days(data_dir: &Path) -> Vec<String> {
    let mut days = list_days_on_disk(data_dir);
    // 0.6.53 — 保底把「今天」放进列表最前。
    // 日账文件由首个知识事件创建；冷启动后还没打字/点击时文件尚不存在，
    // 前端日期条就会缺今天、看起来「最新的日期没更新到今天」。
    // 今天的数据一定有意义（哪怕为空），始终列入。
    let today = Local::now().format("%Y-%m-%d").to_string();
    days.retain(|d| d != &today);
    days.insert(0, today);
    days
}

/// Copy day ledger to exports/ for agent backup / offline handoff. Returns path.

pub fn export_day(data_dir: &Path, day: &str) -> anyhow::Result<PathBuf> {
    flush_if_today(day);
    let src = days_dir(data_dir).join(format!("{day}.jsonl"));
    if !src.exists() {
        anyhow::bail!("no day ledger for {day}");
    }
    let dir = exports_dir(data_dir);
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let stamp = Local::now().format("%H%M%S");
    let dest = dir.join(format!("deskcap-{day}-{stamp}.jsonl"));
    fs::copy(&src, &dest)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o600));
    }
    Ok(dest)
}

/// Append-import JSONL lines into a day file. Never overwrites existing lines.
/// Returns number of knowledge events imported.

pub fn export_day_range(
    data_dir: &Path,
    day: &str,
    start_hhmm: Option<&str>,
    end_hhmm: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let (path, _n) = export_day_range_counted(data_dir, day, start_hhmm, end_hhmm)?;
    Ok(path)
}

pub fn export_day_range_counted(
    data_dir: &Path,
    day: &str,
    start_hhmm: Option<&str>,
    end_hhmm: Option<&str>,
) -> anyhow::Result<(PathBuf, usize)> {
    // `day` 来自前端 tauri command，同样要挡目录穿越（见 is_valid_day）。
    if !is_valid_day(day) {
        anyhow::bail!("invalid day (expect YYYY-MM-DD)");
    }
    flush_if_today(day);
    let src = days_dir(data_dir).join(format!("{day}.jsonl"));
    if !src.exists() {
        anyhow::bail!("no day ledger for {day}");
    }
    let start_ts = hhmm_to_wall_ts(day, start_hhmm.unwrap_or("00:00"))?;
    let end_ts = if let Some(end) = end_hhmm {
        hhmm_to_wall_ts(day, end)?
    } else {
        // exclusive end of day = next midnight
        hhmm_to_wall_ts(day, "00:00")? + 24.0 * 3600.0
    };
    if end_ts <= start_ts {
        anyhow::bail!("export end must be after start");
    }

    let dir = exports_dir(data_dir);
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let stamp = Local::now().format("%H%M%S");
    let range_tag = match (start_hhmm, end_hhmm) {
        (None, None) => "fullday".to_string(),
        (s, e) => format!(
            "{}-{}",
            s.unwrap_or("00:00").replace(':', ""),
            e.unwrap_or("2400").replace(':', "")
        ),
    };
    let dest = dir.join(format!("deskcap-{day}-{range_tag}-{stamp}.jsonl"));
    let raw = fs::read_to_string(&src)?;
    let mut n = 0usize;
    let mut out = String::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<CaptureEvent>(line) else {
            continue;
        };
        if ev.wall_ts < start_ts || ev.wall_ts >= end_ts {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        n += 1;
    }
    fs::write(&dest, out)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o600));
    }
    if n == 0 {
        anyhow::bail!("no events in selected time range");
    }
    Ok((dest, n))
}

/// 只接受 `YYYY-MM-DD`：10 位、仅数字与连字符。
/// 关键点是**不含 `/` 与 `.`** —— 这挡住了把 `day` 拼进路径时的目录穿越。
fn is_valid_day(day: &str) -> bool {
    day.len() == 10 && day.chars().all(|c| c.is_ascii_digit() || c == '-')
}

pub fn import_day_jsonl(data_dir: &Path, day: &str, content: &str) -> anyhow::Result<usize> {
    if !is_valid_day(day) {
        anyhow::bail!("invalid day (expect YYYY-MM-DD)");
    }
    // Drain in-memory buffer first — clearing SINK without flush would drop lines.
    flush();
    let dir = days_dir(data_dir);
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{day}.jsonl"));
    let mut n = 0usize;
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<CaptureEvent>(line) else {
            continue;
        };
        if !is_knowledge_event(&ev) {
            continue;
        }
        writeln!(f, "{}", serde_json::to_string(&ev)?)?;
        n += 1;
    }
    f.flush()?;
    // Drop sink so next append re-opens (avoids two writers on same file).
    *SINK.lock() = None;
    Ok(n)
}

fn hhmm_to_wall_ts(day: &str, hhmm: &str) -> anyhow::Result<f64> {
    use chrono::{Local, NaiveDate, NaiveTime, TimeZone};
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map_err(|_| anyhow::anyhow!("invalid day (expect YYYY-MM-DD)"))?;
    let time = NaiveTime::parse_from_str(hhmm, "%H:%M")
        .or_else(|_| NaiveTime::parse_from_str(hhmm, "%H:%M:%S"))
        .map_err(|_| anyhow::anyhow!("invalid time (expect HH:MM)"))?;
    let ndt = date.and_time(time);
    let dt = Local
        .from_local_datetime(&ndt)
        .single()
        .ok_or_else(|| anyhow::anyhow!("invalid local datetime"))?;
    Ok(dt.timestamp() as f64 + f64::from(dt.timestamp_subsec_millis()) / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::CaptureEvent;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Global day sink is process-wide — tests must not run concurrently.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    /// 路径穿越防护：`day` 来自 HTTP 查询与前端命令，必须是 YYYY-MM-DD。
    /// 反例用 `../sessions/x` 这类真实可构造的输入。
    #[test]
    fn rejects_path_traversal_day() {
        assert!(is_valid_day("2026-10-21"));
        assert!(is_valid_day("2026-01-01"));
        // 含 `/` 或 `.` 的一律拒绝
        assert!(!is_valid_day("../sessions/abc"));
        assert!(!is_valid_day("../../etc/pas"));
        assert!(!is_valid_day("2026-10-2"));
        assert!(!is_valid_day("2026-10-211"));
        assert!(!is_valid_day(""));
        // 只有数字与连字符才通过；`----------` 虽无意义但不会穿越
        assert!(is_valid_day("----------"));
    }

    /// 读取路径必须拒绝非法 day，且不返回任何事件。
    #[test]
    fn read_day_page_rejects_traversal() {
        let _g = TEST_LOCK.lock();
        let dir = std::env::temp_dir().join("deskcap-daybook-traversal");
        let _ = fs::create_dir_all(&dir);
        assert!(read_day_page(&dir, "../sessions/x", None, 10).is_empty());
        assert!(read_day_page(&dir, "not-a-date", None, 10).is_empty());
    }

    fn sample_typing(text: &str) -> CaptureEvent {
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        CaptureEvent {
            ts: 1.0,
            wall_ts: wall,
            event_type: "typing".into(),
            app: None,
            data: json!({ "text": text, "char_count": text.chars().count() }),
        }
    }

    #[test]
    fn append_flush_roundtrip_no_corrupt_lines() {
        let _lock = TEST_LOCK.lock();
        let dir = std::env::temp_dir().join(format!("deskcap-daybook-{}", std::process::id()));
        let data = dir.join("sessions");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&data).unwrap();
        *SINK.lock() = None;

        for i in 0..20 {
            append_event(&data, &sample_typing(&format!("line{i}")));
        }
        flush();
        *SINK.lock() = None;

        let day = Local::now().format("%Y-%m-%d").to_string();
        let path = days_dir(&data).join(format!("{day}.jsonl"));
        let raw = fs::read_to_string(&path).expect("day jsonl after flush");
        assert!(raw.ends_with('\n'));
        let mut n = 0;
        for line in raw.lines() {
            let ev: CaptureEvent = serde_json::from_str(line).expect("valid jsonl");
            assert_eq!(ev.event_type, "typing");
            n += 1;
        }
        assert_eq!(n, 20);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn import_flushes_before_clearing_sink() {
        let _lock = TEST_LOCK.lock();
        let dir = std::env::temp_dir().join(format!("deskcap-import-{}", std::process::id()));
        let data = dir.join("sessions");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&data).unwrap();
        *SINK.lock() = None;

        append_event(&data, &sample_typing("buffered"));
        // Still in BufWriter — import must flush first.
        let day = Local::now().format("%Y-%m-%d").to_string();
        let n = import_day_jsonl(
            &data,
            &day,
            &serde_json::to_string(&sample_typing("imported")).unwrap(),
        )
        .unwrap();
        assert_eq!(n, 1);

        let path = days_dir(&data).join(format!("{day}.jsonl"));
        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("buffered"), "pre-import buffer must not be lost");
        assert!(raw.contains("imported"));
        *SINK.lock() = None;
        let _ = fs::remove_dir_all(&dir);
    }
}

