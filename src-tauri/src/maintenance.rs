//! Local retention, cleanup, and offline backup — no cloud.

use crate::config::CaptureConfig;
use chrono::{Duration as ChronoDuration, Local};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, serde::Serialize)]
pub struct MaintenanceReport {
    pub deleted_sessions: usize,
    pub deleted_bytes: u64,
    pub backup_path: Option<String>,
    pub sessions_bytes: u64,
    pub backup_bytes: u64,
}

pub fn run_maintenance(cfg: &CaptureConfig) -> MaintenanceReport {
    let mut report = MaintenanceReport {
        deleted_sessions: 0,
        deleted_bytes: 0,
        backup_path: None,
        sessions_bytes: 0,
        backup_bytes: 0,
    };

    let sessions = &cfg.data_dir;
    let backup_root = cfg
        .data_dir
        .parent()
        .unwrap_or(Path::new("."))
        .join("backups");
    let _ = fs::create_dir_all(sessions);
    let _ = fs::create_dir_all(&backup_root);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(sessions, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(&backup_root, fs::Permissions::from_mode(0o700));
    }

    // 1) Delete sessions older than retention_days
    let retain = ChronoDuration::days(cfg.retention_days.max(1) as i64);
    let cutoff = Local::now() - retain;
    if let Ok(entries) = fs::read_dir(sessions) {
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let Ok(meta) = ent.metadata() else { continue };
            let modified = meta.modified().ok().and_then(|t| {
                t.duration_since(UNIX_EPOCH).ok().map(|d| {
                    chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                        .map(|dt| dt.with_timezone(&Local))
                })
            });
            let Some(Some(mtime)) = modified else { continue };
            if mtime < cutoff {
                let size = dir_size(&path);
                if fs::remove_dir_all(&path).is_ok() {
                    report.deleted_sessions += 1;
                    report.deleted_bytes += size;
                }
            }
        }
    }

    // 1b) Prune day ledgers + exports older than retention (same root as sessions)
    let desk_root = sessions.parent().unwrap_or(Path::new("."));
    for sub in ["days", "exports"] {
        let dir = desk_root.join(sub);
        report.deleted_bytes += prune_old_files(&dir, cutoff, &mut report.deleted_sessions);
    }

    // 2) Enforce max disk across sessions + days (exports excluded from budget)
    let max_bytes = cfg.max_disk_mb.saturating_mul(1024 * 1024);
    loop {
        let sessions_sz = dir_size(sessions);
        let days_sz = dir_size(&desk_root.join("days"));
        let total = sessions_sz + days_sz;
        report.sessions_bytes = total;
        if total <= max_bytes {
            break;
        }
        // Prefer dropping oldest session dirs first, then oldest day jsonl.
        let mut dirs = list_session_dirs(sessions).1;
        dirs.sort_by_key(|(_, t, _)| *t);
        if let Some((path, _, size)) = dirs.first().cloned() {
            if fs::remove_dir_all(&path).is_ok() {
                report.deleted_sessions += 1;
                report.deleted_bytes += size;
                continue;
            }
        }
        if prune_oldest_file(&desk_root.join("days"), &mut report) {
            continue;
        }
        break;
    }

    // 3) Daily local backup of newest session jsonl (copy, not upload)
    if cfg.backup_enabled {
        if let Some(path) = maybe_backup(sessions, &backup_root) {
            report.backup_path = Some(path);
        }
    }

    // 4) Prune old backups
    let backup_retain = ChronoDuration::days(cfg.retention_days.max(1) as i64);
    let bcut = Local::now() - backup_retain;
    if let Ok(entries) = fs::read_dir(&backup_root) {
        for ent in entries.flatten() {
            let path = ent.path();
            if let Ok(meta) = ent.metadata() {
                if let Ok(mtime) = meta.modified() {
                    if let Ok(d) = mtime.duration_since(UNIX_EPOCH) {
                        if let Some(dt) = chrono::DateTime::from_timestamp(d.as_secs() as i64, 0) {
                            if dt.with_timezone(&Local) < bcut {
                                let _ = fs::remove_file(&path);
                            }
                        }
                    }
                }
            }
        }
    }
    report.backup_bytes = dir_size(&backup_root);
    report.sessions_bytes = dir_size(sessions) + dir_size(&desk_root.join("days"));
    report
}

fn prune_old_files(
    dir: &Path,
    cutoff: chrono::DateTime<Local>,
    deleted_count: &mut usize,
) -> u64 {
    let mut freed = 0u64;
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Ok(meta) = ent.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        let Ok(d) = mtime.duration_since(UNIX_EPOCH) else { continue };
        let Some(dt) = chrono::DateTime::from_timestamp(d.as_secs() as i64, 0) else {
            continue;
        };
        if dt.with_timezone(&Local) < cutoff {
            let size = meta.len();
            if fs::remove_file(&path).is_ok() {
                *deleted_count += 1;
                freed += size;
            }
        }
    }
    freed
}

fn prune_oldest_file(dir: &Path, report: &mut MaintenanceReport) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut files: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    for ent in entries.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Ok(meta) = ent.metadata() else { continue };
        let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
        files.push((path, mtime, meta.len()));
    }
    files.sort_by_key(|(_, t, _)| *t);
    let Some((path, _, size)) = files.first() else {
        return false;
    };
    if fs::remove_file(path).is_ok() {
        report.deleted_sessions += 1;
        report.deleted_bytes += size;
        true
    } else {
        false
    }
}

fn list_session_dirs(root: &Path) -> (u64, Vec<(PathBuf, SystemTime, u64)>) {
    let mut total = 0u64;
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for ent in entries.flatten() {
            let path = ent.path();
            if !path.is_dir() {
                continue;
            }
            let size = dir_size(&path);
            total += size;
            let mtime = ent
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            dirs.push((path, mtime, size));
        }
    }
    (total, dirs)
}

fn dir_size(path: &Path) -> u64 {
    let mut n = 0u64;
    let walk = |p: &Path, acc: &mut u64| {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                let ep = e.path();
                if ep.is_dir() {
                    *acc += dir_size(&ep);
                } else if let Ok(m) = e.metadata() {
                    *acc += m.len();
                }
            }
        }
    };
    walk(path, &mut n);
    n
}

fn maybe_backup(sessions: &Path, backup_root: &Path) -> Option<String> {
    let day = Local::now().format("%Y-%m-%d").to_string();
    let dest = backup_root.join(format!("deskcap-{day}.jsonl"));
    // 0.6.53 修 — 已存在的**空备份**要重做。
    // 原逻辑：只要文件存在就直接返回，于是「当天维护任务在 session 刚启动、
    // 还没有任何 jsonl 行时先跑了一次」写出的 0 字节文件会永远留着 ——
    // 实测 backups/deskcap-2026-10-02.jsonl 与 10-06 都是 0 字节，那份日备份
    // 等于没有。空文件没有保留价值，删掉重做；有内容才认。
    if let Ok(meta) = dest.metadata() {
        if meta.len() > 0 {
            return Some(dest.display().to_string());
        }
        let _ = fs::remove_file(&dest);
    }
    // Concatenate latest session's jsonl files into one daily backup (cap ~32MB).
    let mut dirs = list_session_dirs(sessions).1;
    dirs.sort_by_key(|(_, t, _)| *t);
    let newest = dirs.last()?;
    let mut out = fs::File::create(&dest).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o600));
    }
    let mut written = 0u64;
    if let Ok(rd) = fs::read_dir(&newest.0) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            if f.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Ok(mut file) = fs::File::open(&f) {
                let mut buf = Vec::new();
                if file.read_to_end(&mut buf).is_ok() {
                    if written + buf.len() as u64 > 32 * 1024 * 1024 {
                        break;
                    }
                    let _ = out.write_all(&buf);
                    written += buf.len() as u64;
                }
            }
        }
    }
    let _ = out.flush();
    // 0.6.53 — 写完仍是 0 字节（最新 session 尚无 jsonl 内容）：不要留空文件，
    // 删掉并返回 None，让下一次维护重试；否则 `dest.metadata().len() > 0`
    // 检查虽能救回来，但空文件会先被 UI 的「最近备份」读到、显示成一次成功备份。
    if written == 0 {
        drop(out);
        let _ = fs::remove_file(&dest);
        return None;
    }
    Some(dest.display().to_string())
}
