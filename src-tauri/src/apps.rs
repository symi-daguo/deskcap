//! List installed desktop apps for the exclude picker.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct InstalledApp {
    pub name: String,
    pub path: String,
}

pub fn list_installed_apps() -> Vec<InstalledApp> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    #[cfg(target_os = "macos")]
    {
        for dir in [
            PathBuf::from("/Applications"),
            PathBuf::from("/System/Applications"),
            dirs::home_dir()
                .unwrap_or_default()
                .join("Applications"),
        ] {
            scan_macos_apps(&dir, &mut out, &mut seen);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            scan_windows_dir(PathBuf::from(pf), &mut out, &mut seen);
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            scan_windows_dir(PathBuf::from(pf86), &mut out, &mut seen);
        }
    }

    #[cfg(target_os = "linux")]
    {
        for dir in [
            PathBuf::from("/usr/share/applications"),
            dirs::home_dir()
                .unwrap_or_default()
                .join(".local/share/applications"),
        ] {
            scan_linux_desktop(&dir, &mut out, &mut seen);
        }
    }

    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

#[cfg(target_os = "macos")]
fn scan_macos_apps(
    dir: &PathBuf,
    out: &mut Vec<InstalledApp>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("app") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("App")
            .to_string();
        let key = name.to_lowercase();
        if seen.insert(key) {
            out.push(InstalledApp {
                name,
                path: path.display().to_string(),
            });
        }
    }
}

#[cfg(target_os = "windows")]
fn scan_windows_dir(
    dir: PathBuf,
    out: &mut Vec<InstalledApp>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten().take(400) {
        let path = e.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("App")
            .to_string();
        let key = name.to_lowercase();
        if seen.insert(key) {
            out.push(InstalledApp {
                name,
                path: path.display().to_string(),
            });
        }
    }
}

#[cfg(target_os = "linux")]
fn scan_linux_desktop(
    dir: &PathBuf,
    out: &mut Vec<InstalledApp>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|s| s.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let mut name = None;
        for line in raw.lines() {
            if let Some(v) = line.strip_prefix("Name=") {
                name = Some(v.trim().to_string());
                break;
            }
        }
        let Some(name) = name else { continue };
        let key = name.to_lowercase();
        if seen.insert(key) {
            out.push(InstalledApp {
                name,
                path: path.display().to_string(),
            });
        }
    }
}
