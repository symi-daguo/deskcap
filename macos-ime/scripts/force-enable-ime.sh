#!/usr/bin/env bash
# Register DeskCap + keep system 简体拼音 + ABC. Never leave the menu empty.
set -euo pipefail
APP="/Library/Input Methods/DeskCapIME.app"
[[ -d "$APP" ]] || { echo "Missing $APP"; exit 1; }

python3 - <<'PY'
import subprocess, plistlib, pathlib
raw = subprocess.check_output(["defaults", "export", "com.apple.HIToolbox", "-"])
data = plistlib.loads(raw)
keep = [e for e in data.get("AppleEnabledInputSources", [])
        if e.get("InputSourceKind") == "Non Keyboard Input Method"]
# Always: ABC + 系统简体拼音 + DeskCap
keep.append({"InputSourceKind": "Keyboard Layout", "KeyboardLayout ID": 252, "KeyboardLayout Name": "ABC"})
keep.append({"Bundle ID": "com.apple.inputmethod.SCIM",
             "Input Mode": "com.apple.inputmethod.SCIM.ITABC",
             "InputSourceKind": "Input Mode"})
keep.append({"Bundle ID": "com.office.deskcap.inputmethod.symi",
             "InputSourceKind": "Keyboard Input Method"})
keep.append({"Bundle ID": "com.office.deskcap.inputmethod.symi",
             "Input Mode": "com.office.deskcap.inputmethod.symi.pinyin",
             "InputSourceKind": "Input Mode"})
# Preserve 微信键盘 if it was enabled
for e in data.get("AppleEnabledInputSources", []):
    if "wetype" in str(e).lower():
        keep.append(e)
data["AppleEnabledInputSources"] = keep
path = pathlib.Path("/tmp/hit-force3.plist")
path.write_bytes(plistlib.dumps(data))
subprocess.check_call(["defaults", "import", "com.apple.HIToolbox", str(path)])
print("HIToolbox: ABC + 简体拼音 + DeskCap拼音 (+微信 if present)")
PY

swift - <<'EOF'
import Carbon
import Foundation
_ = TISRegisterInputSource(URL(fileURLWithPath: "/Library/Input Methods/DeskCapIME.app") as CFURL)
let want = "com.office.deskcap.inputmethod.symi.pinyin"
for src in TISCreateInputSourceList(nil, true).takeRetainedValue() as! [TISInputSource] {
  let id = Unmanaged<CFString>.fromOpaque(TISGetInputSourceProperty(src, kTISPropertyInputSourceID)!).takeUnretainedValue() as String
  if id == want || id == "com.office.deskcap.inputmethod.symi"
      || id == "com.apple.inputmethod.SCIM.ITABC" || id == "com.apple.inputmethod.SCIM" {
    _ = TISEnableInputSource(src)
  }
}
for src in TISCreateInputSourceList(nil, false).takeRetainedValue() as! [TISInputSource] {
  let id = Unmanaged<CFString>.fromOpaque(TISGetInputSourceProperty(src, kTISPropertyInputSourceID)!).takeUnretainedValue() as String
  if id == want { print("select", TISSelectInputSource(src)) }
}
if let cur = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
   let p = TISGetInputSourceProperty(cur, kTISPropertyInputSourceID) {
  print("CURRENT", Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String)
}
EOF
killall TextInputMenuAgent 2>/dev/null || true
