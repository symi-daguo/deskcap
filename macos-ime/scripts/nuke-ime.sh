#!/usr/bin/env bash
# Nuclear cleanup: remove ALL DeskCap IME copies + disable TIS entries.
# After this: LOG OUT, confirm Add sheet has ZERO DeskCap, then reinstall once.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

pkill -x DeskCapIME 2>/dev/null || true

echo "== Disable TIS entries =="
swift - <<'EOF' || true
import Carbon
import Foundation
let all = TISCreateInputSourceList(nil, true).takeRetainedValue() as! [TISInputSource]
for src in all {
  guard let p = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) else { continue }
  let id = Unmanaged<CFString>.fromOpaque(p).takeUnretainedValue() as String
  if id.lowercased().contains("deskcap") {
    print("disable", id, TISDisableInputSource(src))
  }
}
EOF

echo "== Remove app bundles =="
rm -rf "$HOME/Library/Input Methods/DeskCapIME.app"
if [[ -d "/Library/Input Methods/DeskCapIME.app" ]]; then
  echo "Need password to remove /Library copy..."
  sudo rm -rf "/Library/Input Methods/DeskCapIME.app"
fi

echo "== Clean HIToolbox prefs =="
python3 - <<'PY'
import subprocess, plistlib, pathlib
raw = subprocess.check_output(["defaults", "export", "com.apple.HIToolbox", "-"])
data = plistlib.loads(raw)
for key in ("AppleEnabledInputSources", "AppleSelectedInputSources", "AppleInputSourceHistory"):
    arr = data.get(key)
    if isinstance(arr, list):
        data[key] = [e for e in arr if "deskcap" not in str(e).lower()]
path = pathlib.Path("/tmp/hit-nuke.plist")
path.write_bytes(plistlib.dumps(data))
subprocess.check_call(["defaults", "import", "com.apple.HIToolbox", str(path)])
print("HIToolbox ok")
PY

rm -rf "$HOME/Library/Caches/com.apple.IntlDataCache"* 2>/dev/null || true
killall TextInputMenuAgent cfprefsd SystemUIServer 2>/dev/null || true

echo ""
echo "=== 已卸载干净 ==="
echo "请立刻：苹果菜单 → 注销… → 重新登录"
echo "登录后打开：系统设置 → 键盘 → 输入法 → + → 中文"
echo "确认列表里 **完全没有** DeskCap拼音，再告诉我，我再给你装回「仅 1 份」。"
echo ""
echo "现在磁盘："
ls "/Library/Input Methods/" 2>/dev/null || true
ls "$HOME/Library/Input Methods/" 2>/dev/null || true
