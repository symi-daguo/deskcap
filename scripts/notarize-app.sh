#!/usr/bin/env bash
# Notarize + staple DeskCap.app for Gatekeeper on other Macs.
# Prereq: pnpm tauri build --bundles app  (Developer ID + hardened runtime)
#         xcrun notarytool store-credentials deskcap-notary ...
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="${1:-$ROOT/src-tauri/target/release/bundle/macos/DeskCap.app}"
PROFILE="${NOTARY_PROFILE:-deskcap-notary}"
ZIP="/tmp/DeskCap-notarize.zip"

if [[ ! -d "$APP" ]]; then
  echo "Missing $APP — run: pnpm tauri build --bundles app" >&2
  exit 1
fi

if ! codesign -dv --verbose=2 "$APP" 2>&1 | grep -qE 'Developer ID Application|flags=0x10000'; then
  echo "App is not Developer ID signed. Rebuild with Developer ID cert." >&2
  codesign -dv --verbose=2 "$APP" 2>&1 | head -20 >&2
  exit 1
fi

echo "Notarizing: $APP"
codesign -dv --verbose=2 "$APP" 2>&1 | egrep 'Authority|Identifier|TeamIdentifier|flags' || true

rm -f "$ZIP"
ditto -c -k --keepParent "$APP" "$ZIP"
xcrun notarytool submit "$ZIP" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

echo ""
echo "Gatekeeper:"
echo "  spctl -a -vv \"$APP\""
echo ""
echo "Install (keeps path /Applications sticky, preserves user data):"
echo "  bash scripts/install-macos.sh"
