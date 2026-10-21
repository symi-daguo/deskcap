#!/usr/bin/env bash
# Copy latest Windows NSIS setup into releases/ (visible, clickable).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NSIS="$ROOT/src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis"
OUT="$ROOT/releases"
mkdir -p "$OUT"

shopt -s nullglob
cands=("$NSIS"/DeskCap_*_x64-setup.exe)
if [[ ${#cands[@]} -eq 0 ]]; then
  echo "No NSIS setup under $NSIS — build first:" >&2
  echo "  pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis" >&2
  exit 1
fi

# Prefer newest by mtime (version string sort breaks on 1.0.10 vs 1.0.9).
latest="$(ls -1t "${cands[@]}" | head -1)"

base="$(basename "$latest")"
cp -f "$latest" "$OUT/$base"
# Drop stale older setups in releases/
for old in "$OUT"/DeskCap_*_x64-setup.exe; do
  [[ "$(basename "$old")" == "$base" ]] || rm -f "$old"
done

echo "Ready: $OUT/$base"
open -R "$OUT/$base" 2>/dev/null || true
