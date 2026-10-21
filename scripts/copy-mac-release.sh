#!/usr/bin/env bash
# Copy latest macOS DMG into releases/ (visible, clickable — same as Win).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DMG_DIR="$ROOT/src-tauri/target/release/bundle/dmg"
OUT="$ROOT/releases"
mkdir -p "$OUT"

shopt -s nullglob
cands=("$DMG_DIR"/DeskCap_*.dmg)
# Ignore temp rw.* leftovers from failed creates
filtered=()
for f in "${cands[@]}"; do
  [[ "$(basename "$f")" == rw.* ]] && continue
  filtered+=("$f")
done
if [[ ${#filtered[@]} -eq 0 ]]; then
  echo "No DMG under $DMG_DIR — build first:" >&2
  echo "  pnpm tauri build --bundles dmg" >&2
  exit 1
fi

latest="$(ls -1t "${filtered[@]}" | head -1)"
base="$(basename "$latest")"
cp -f "$latest" "$OUT/$base"
for old in "$OUT"/DeskCap_*.dmg; do
  [[ "$(basename "$old")" == "$base" ]] || rm -f "$old"
done

echo "Ready: $OUT/$base"
open -R "$OUT/$base" 2>/dev/null || true
