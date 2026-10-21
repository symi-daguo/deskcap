//! macOS: keep DeskCap拼音 **listed** in HIToolbox without TISSelect / swift
//! (those spam TCC dialogs and can break typing). DeskCap.app self-heals the
//! enabled list — customers never run scripts.

#[cfg(target_os = "macos")]
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
pub fn ensure_ime_listed() {
    if !PathBuf::from("/Library/Input Methods/DeskCapIME.app").is_dir() {
        return;
    }
    match Command::new("/usr/bin/python3")
        .arg("-c")
        .arg(
            r#"
import subprocess, plistlib, pathlib, os, sys
if not os.path.isdir("/Library/Input Methods/DeskCapIME.app"):
    raise SystemExit(0)
raw = subprocess.check_output(["defaults", "export", "com.apple.HIToolbox", "-"])
data = plistlib.loads(raw)
enabled = data.get("AppleEnabledInputSources", [])
# Skip import when DeskCap is already listed — rewriting HIToolbox freezes input switching.
if any("symi" in str(e).lower() for e in enabled):
    print("skip")
    raise SystemExit(0)
keep = []
for e in enabled:
    s = str(e).lower()
    if any(x in s for x in ("deskcap", "symi", "mini", "wetype")):
        continue
    keep.append(e)
if not any(e.get("KeyboardLayout Name") == "ABC" for e in keep):
    keep.append({"InputSourceKind": "Keyboard Layout", "KeyboardLayout ID": 252, "KeyboardLayout Name": "ABC"})
if not any("ITABC" in str(e) for e in keep):
    keep.append({"Bundle ID": "com.apple.inputmethod.SCIM",
                 "Input Mode": "com.apple.inputmethod.SCIM.ITABC",
                 "InputSourceKind": "Input Mode"})
keep.append({"Bundle ID": "com.office.deskcap.inputmethod.symi",
             "InputSourceKind": "Keyboard Input Method"})
keep.append({"Bundle ID": "com.office.deskcap.inputmethod.symi",
             "Input Mode": "com.office.deskcap.inputmethod.symi.pinyin",
             "InputSourceKind": "Input Mode"})
data["AppleEnabledInputSources"] = keep
p = pathlib.Path.home() / "Library/Application Support/DeskCap/hit-stable.plist"
p.write_bytes(plistlib.dumps(data))
subprocess.check_call(["defaults", "import", "com.apple.HIToolbox", str(p)])
print("ok")
"#,
        )
        .output()
    {
        Ok(o) if o.status.success() => {
            let out = String::from_utf8_lossy(&o.stdout);
            if !out.contains("skip") {
                crate::logutil::info("ime-list ok");
            }
        }
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr);
            crate::logutil::warn(&format!("ime-list failed: {err}"));
        }
        Err(e) => crate::logutil::warn(&format!("ime-list spawn: {e}")),
    }
}

#[cfg(target_os = "macos")]
pub fn spawn_ime_keepalive() {
    std::thread::Builder::new()
        .name("deskcap-ime-keepalive".into())
        .spawn(move || {
            std::thread::sleep(Duration::from_secs(3));
            ensure_ime_listed();
            loop {
                std::thread::sleep(Duration::from_secs(90));
                ensure_ime_listed();
            }
        })
        .ok();
}

#[cfg(not(target_os = "macos"))]
pub fn spawn_ime_keepalive() {}
