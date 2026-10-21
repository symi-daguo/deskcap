#!/usr/bin/env bash
# Keep DeskCap拼音 listed. Same HIToolbox rewrite as DeskCap.app ime-list (no TISSelect).
# --select: optional debug switch via switch-deskcap-ime.sh
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="/Library/Input Methods/DeskCapIME.app"
[[ -d "$APP" ]] || exit 0

# Reuse force-enable's list rewrite (single source).
bash "$ROOT/macos-ime/scripts/force-enable-ime.sh"

if [[ "${1:-}" == "--select" ]]; then
  bash "$ROOT/scripts/switch-deskcap-ime.sh"
fi
