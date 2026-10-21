//! Tiny rotating file log under DeskCap data root — for Win/Linux dogfood without a console.

use parking_lot::Mutex;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
/// Per-call-site throttle state, keyed by `where_`.
/// 0.6.49 修 — 原来是**单一全局** `Mutex<Option<Instant>>`，被 7 个调用点
/// （`input_worker` / `input_queue_drop` / `rdev_cb` / `uia_read` / …）共用，
/// 于是任何一次 lag 都会把其余调用点在 2 秒内的告警一并压掉：
/// 实测本机 `deskcap.log` 里 `input_worker` 独占 769 条、其他调用点 0 条，
/// 说明不是它们没慢，而是**被压制得看不见**。按 `where_` 分桶后各站点独立节流，
/// 行数上限不变（每个站点 2 秒 1 条）。
static LAST_LAG_LOG: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());

/// 超过这个毫秒数就不再算「抖动」，直接升为 ERROR 级别。
/// 0.6.49 — `input_worker` 实测峰值 17390ms（17 秒），这种量级是「某一步阻塞」
/// 而非「IPC 抖动」，混在同一行 `WARN lag` 里会被当成噪声忽略。
/// 按你的要求「无错误日志」：这类值必须能被一眼挑出来，而不是埋在 WARN 里。
const LAG_ERROR_MS: u64 = 1000;

/// 桶上限：防 `where_` 取值变化导致无限增长。超过就丢掉最旧的一批。
const LAG_BUCKETS_MAX: usize = 16;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

pub fn init(data_dir: &std::path::Path) {
    let root = data_dir
        .parent()
        .filter(|p| p.file_name().and_then(|n| n.to_str()) == Some("DeskCap"))
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| data_dir.to_path_buf());
    let dir = root.join("logs");
    let _ = fs::create_dir_all(&dir);
    *LOG_PATH.lock() = Some(dir.join("deskcap.log"));
    info("DeskCap log ready");
}

pub fn info(msg: &str) {
    write_line("INFO", msg);
}

pub fn warn(msg: &str) {
    write_line("WARN", msg);
}

/// ERROR tier — 0.6.49 新增，用于「不是抖动，是某一步真的阻塞了」的情况
/// （见 `lag_ms` 里 `LAG_ERROR_MS` 的说明）。
pub fn error(msg: &str) {
    write_line("ERROR", msg);
}

/// Log only if `elapsed_ms` is high; throttle to ≤1 line / 2s **per call-site** so hooks stay light.
/// 0.6.49 — per-site throttle (was one global slot shared by 7 call sites) + ERROR tier ≥1 s.
pub fn lag_ms(where_: &str, elapsed_ms: u64) {
    if elapsed_ms < 8 {
        return;
    }
    {
        let mut buckets = LAST_LAG_LOG.lock();
        if let Some((_, t)) = buckets.iter().find(|(k, _)| k == where_) {
            if t.elapsed() < Duration::from_secs(2) {
                return;
            }
        }
        buckets.retain(|(k, _)| k != where_);
        buckets.push((where_.to_string(), Instant::now()));
        // `where_` 实参全是 `&'static str` 字面量（7 个站点），实际不会超过 16；
        // 这个上限只是防御：万一以后有人传动态字符串，也不会无限涨。
        if buckets.len() > LAG_BUCKETS_MAX {
            let drop = buckets.len() - LAG_BUCKETS_MAX;
            buckets.drain(0..drop);
        }
    }
    if elapsed_ms >= LAG_ERROR_MS {
        error(&format!("lag {where_}={elapsed_ms}ms"));
    } else {
        warn(&format!("lag {where_}={elapsed_ms}ms"));
    }
}

fn write_line(level: &str, msg: &str) {
    let Some(path) = LOG_PATH.lock().clone() else {
        return;
    };
    let _ = rotate_if_needed(&path);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{ts} {level} {msg}");
    }
}

fn rotate_if_needed(path: &std::path::Path) {
    let Ok(meta) = fs::metadata(path) else {
        return;
    };
    if meta.len() < MAX_BYTES {
        return;
    }
    let bak = path.with_extension("log.1");
    let _ = fs::remove_file(&bak);
    let _ = fs::rename(path, &bak);
}
