#!/usr/bin/env bash
# Reliably switch to DeskCap拼音. Self-contained (no nested force-enable).
set -euo pipefail
APP="/Library/Input Methods/DeskCapIME.app"
[[ -d "$APP" ]] || { echo "Missing $APP"; exit 1; }

swift - <<'EOF'
import Carbon
let want = "com.office.deskcap.inputmethod.symi.pinyin"
let parent = "com.office.deskcap.inputmethod.symi"
let scim = "com.apple.inputmethod.SCIM.ITABC"

func enableAll() {
  for src in TISCreateInputSourceList(nil, true).takeRetainedValue() as! [TISInputSource] {
    let id = Unmanaged<CFString>.fromOpaque(TISGetInputSourceProperty(src, kTISPropertyInputSourceID)!).takeUnretainedValue() as String
    if id == want || id == parent || id == scim {
      _ = TISEnableInputSource(src)
    }
  }
}

func currentId() -> String {
  if let cur = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
     let p = TISGetInputSourceProperty(cur, kTISPropertyInputSourceID) {
    return Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String
  }
  return ""
}

func selectDeskCap() -> Bool {
  for src in TISCreateInputSourceList(nil, false).takeRetainedValue() as! [TISInputSource] {
    let id = Unmanaged<CFString>.fromOpaque(TISGetInputSourceProperty(src, kTISPropertyInputSourceID)!).takeUnretainedValue() as String
    if id == want {
      let r = TISSelectInputSource(src)
      print("select", r)
      return r == noErr
    }
  }
  return false
}

for attempt in 0..<3 {
  enableAll()
  _ = selectDeskCap()
  let cur = currentId()
  print("attempt\(attempt) CURRENT", cur)
  if cur == want { exit(0) }
  usleep(200_000)
}
print("FAILED to select DeskCap拼音; CURRENT", currentId())
exit(1)
EOF
killall TextInputMenuAgent 2>/dev/null || true
