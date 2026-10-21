#!/usr/bin/env bash
# Install DeskCap preserving TCC: same Apple Team identity, in-place ditto.
# Never touches ~/Library/Application Support/DeskCap (days/sessions/token stay intact).
# Preserves Developer ID + hardened runtime + timestamp (required after notarization).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_SRC="$ROOT/src-tauri/target/release/bundle/macos/DeskCap.app"
APP_DST="/Applications/DeskCap.app"

IDENTITY="${DESKCAP_SIGN_IDENTITY:-}"
if [[ -z "$IDENTITY" ]]; then
  IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' \
    | head -1 || true)
fi
if [[ -z "$IDENTITY" ]]; then
  IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null \
    | sed -n 's/.*"\(Apple Development:[^"]*\)".*/\1/p' \
    | head -1 || true)
fi

USE_RUNTIME=0
case "$IDENTITY" in
  "Developer ID Application:"*) USE_RUNTIME=1 ;;
esac

if [[ ! -d "$APP_SRC" ]]; then
  echo "Missing $APP_SRC — run: pnpm tauri build --bundles app" >&2
  exit 1
fi

osascript -e 'quit app "DeskCap"' >/dev/null 2>&1 || true
pkill -x deskcap 2>/dev/null || true
sleep 1

# In-place update (NO rm -rf) so path /Applications/DeskCap.app stays sticky.
mkdir -p "$(dirname "$APP_DST")"
ditto "$APP_SRC" "$APP_DST"

sign_target() {
  local target="$1"
  local ident="$2"
  if [[ -z "$ident" ]]; then
    codesign --force --sign - "$target"
    return
  fi
  if [[ "$USE_RUNTIME" == 1 ]]; then
    codesign --force --options runtime --timestamp --sign "$ident" "$target"
  else
    codesign --force --sign "$ident" "$target"
  fi
}

# Bundle WeChat send-time OCR helper (Vision) next to the main binary.
HELPER_SRC="$ROOT/src-tauri/target/macos-helpers/deskcap-wechat-ocr"
if [[ ! -f "$HELPER_SRC" ]]; then
  HELPER_SRC=$(ls -1 "$ROOT"/src-tauri/target/release/build/deskcap-*/out/deskcap-wechat-ocr 2>/dev/null | head -1 || true)
fi
if [[ -n "${HELPER_SRC:-}" && -f "$HELPER_SRC" ]]; then
  cp -f "$HELPER_SRC" "$APP_DST/Contents/MacOS/deskcap-wechat-ocr"
  chmod +x "$APP_DST/Contents/MacOS/deskcap-wechat-ocr"
  echo "Bundled deskcap-wechat-ocr"
  # Nested helper first, then the app bundle (no --deep; it drops runtime flags).
  sign_target "$APP_DST/Contents/MacOS/deskcap-wechat-ocr" "$IDENTITY"
  # Restore helper identifier after generic sign.
  codesign --force --options runtime --timestamp --sign "${IDENTITY:--}" \
    --identifier com.office.deskcap.wechat-ocr \
    "$APP_DST/Contents/MacOS/deskcap-wechat-ocr" 2>/dev/null || true
  sign_target "$APP_DST" "$IDENTITY"
  echo "Re-signed app+helper with: $IDENTITY hardened_runtime=$USE_RUNTIME"
else
  echo "WARNING: deskcap-wechat-ocr missing — WeChat OCR fallback disabled"
fi

codesign -dv --verbose=2 "$APP_DST" 2>&1 | egrep 'Authority|Identifier|TeamIdentifier|Signature|flags' || true
echo ""
codesign -d -r- "$APP_DST" 2>/dev/null | sed -n 's/^designated => /DR: /p' || true
echo ""
if [[ "$USE_RUNTIME" == 1 ]]; then
  echo "Note: nested helper re-sign after install can invalidate stapled ticket."
  echo "Preferred: embed helper + sign + notarize the SOURCE bundle, then pure ditto."
  echo "If EventTap is dead after identity change: bash scripts/fix-macos-tcc.sh"
fi
echo "Data dir (untouched): ~/Library/Application Support/DeskCap/"
echo "Start ONLY with: open -a DeskCap"
