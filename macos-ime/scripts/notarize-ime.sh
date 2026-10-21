#!/usr/bin/env bash
# Notarize + staple DeskCapIME for Gatekeeper / Input Sources on other Macs.
# Prereq: bash macos-ime/scripts/build-and-install.sh (Developer ID signed)
#         xcrun notarytool store-credentials deskcap-notary ...
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$ROOT/build/DeskCapIME.app}"
PROFILE="${NOTARY_PROFILE:-deskcap-notary}"
ZIP="/tmp/DeskCapIME-notarize.zip"

if [[ ! -d "$APP" ]]; then
  echo "Missing $APP — run: bash macos-ime/scripts/build-and-install.sh" >&2
  exit 1
fi

AUTH=$(codesign -dv --verbose=2 "$APP" 2>&1 | sed -n 's/^Authority=\(Developer ID Application:.*\)/\1/p' | head -1 || true)
if [[ -z "$AUTH" ]]; then
  # Fallback: Team + runtime flag is enough to know it's Developer ID style
  if codesign -dv --verbose=2 "$APP" 2>&1 | grep -q 'flags=0x10000(runtime)' \
    && codesign -dv "$APP" 2>&1 | grep -q "TeamIdentifier=${APPLE_TEAM_ID:?未设置 APPLE_TEAM_ID}"; then
    AUTH="Developer ID Application (team $APPLE_TEAM_ID)"
  fi
fi
if [[ -z "$AUTH" ]]; then
  echo "App is not Developer ID signed. Rebuild with Developer ID cert installed." >&2
  codesign -dv --verbose=2 "$APP" 2>&1 | head -20 >&2
  exit 1
fi

echo "Notarizing: $APP"
echo "Signer: $AUTH"

rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"

xcrun notarytool submit "$ZIP" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

echo ""
echo "=== 安装到本机（系统输入法目录）==="
echo "  sudo ditto \"$APP\" \"/Library/Input Methods/DeskCapIME.app\""
echo "然后：注销登录 → 系统设置 → 键盘 → 输入法 → + → 中文 → DeskCap拼音"
echo ""
echo "Gatekeeper 自检："
echo "  spctl -a -vv \"$APP\""
