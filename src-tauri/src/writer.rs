use crate::event::CaptureEvent;
use anyhow::Result;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Session JSONL writer — buffers to disk; flush on idle timer / stop / rotate.
/// Avoids per-event fsync (SSD wear under all-day office capture).
pub struct SegmentWriter {
    dir: PathBuf,
    session_id: String,
    segment_start: u64,
    file: BufWriter<File>,
    path: PathBuf,
    pub event_count: u64,
    pending: u32,
    last_flush: Instant,
}

const FLUSH_LINES: u32 = 24;
const FLUSH_IDLE_MS: u128 = 400;

impl SegmentWriter {
    pub fn open(dir: &Path, session_id: &str) -> Result<Self> {
        fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
        }
        let segment_start = now_secs();
        let path = dir.join(format!("{session_id}_{segment_start}.jsonl"));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        harden_file(&path);
        Ok(Self {
            dir: dir.to_path_buf(),
            session_id: session_id.to_string(),
            segment_start,
            file: BufWriter::with_capacity(64 * 1024, file),
            path,
            event_count: 0,
            pending: 0,
            last_flush: Instant::now(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&mut self, event: &CaptureEvent) -> Result<()> {
        serde_json::to_writer(&mut self.file, event)?;
        self.file.write_all(b"\n")?;
        self.event_count += 1;
        self.pending = self.pending.saturating_add(1);
        if self.pending >= FLUSH_LINES
            || self.last_flush.elapsed().as_millis() >= FLUSH_IDLE_MS
        {
            self.flush()?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.file.flush()?;
        self.pending = 0;
        self.last_flush = Instant::now();
        Ok(())
    }

    pub fn rotate_if_needed(&mut self, max_secs: u64) -> Result<bool> {
        if max_secs == 0 {
            return Ok(false);
        }
        let age = now_secs().saturating_sub(self.segment_start);
        if age < max_secs {
            return Ok(false);
        }
        self.flush()?;
        let segment_start = now_secs();
        let path = self
            .dir
            .join(format!("{}_{}.jsonl", self.session_id, segment_start));
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        harden_file(&path);
        self.segment_start = segment_start;
        self.path = path;
        self.file = BufWriter::with_capacity(64 * 1024, file);
        self.pending = 0;
        self.last_flush = Instant::now();
        Ok(true)
    }
}

fn harden_file(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    {
        let _ = path;
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
