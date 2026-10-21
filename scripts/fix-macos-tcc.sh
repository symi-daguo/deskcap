#!/usr/bin/env bash
# Clear stale TCC rows that show "enabled" in Settings but fail AX/Listen at runtime
# (common after switching ad-hoc → Apple Development). Then open Privacy panes once.
set -euo pipefail

echo "Resetting ListenEvent + Accessibility for com.office.deskcap…"
tccutil reset ListenEvent com.office.deskcap 2>/dev/null || true
tccutil reset Accessibility com.office.deskcap 2>/dev/null || true

osascript -e 'quit app "DeskCap"' >/dev/null 2>&1 || true
pkill -x deskcap 2>/dev/null || true
sleep 1

# Sequoia / macOS 15+ URLs (fall back to legacy)
open "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_ListenEvent" 2>/dev/null \
  || open "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
sleep 1
open "x-apple.systempreferences:com.apple.settings.PrivacySecurity.extension?Privacy_Accessibility" 2>/dev/null \
  || open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"

cat <<'EOF'

【必须做一次 — 之后同 Team 更新不用再做】

1. 在「输入监控」和「辅助功能」里：删掉所有 DeskCap / deskcap 条目（含灰色幽灵）
2. 点「+」，只添加：/Applications/DeskCap.app
3. 两个开关都打开
4. 下面命令会启动应用；若弹出系统对话框点「允许」

然后看工作台：EventTap 应变绿。以后用：
  pnpm tauri build --bundles app && bash scripts/install-macos.sh && open -a DeskCap

EOF

open -a DeskCap
sleep 3
if [[ -f "$HOME/Library/Application Support/DeskCap/config.json" ]]; then
  TOKEN=$(python3 -c 'import json;from pathlib import Path;print(json.load(open(Path.home()/"Library/Application Support/DeskCap/config.json"))["agent_api_token"])')
  echo "Polling /status (grant in Settings, then wait)…"
  for i in $(seq 1 40); do
    s=$(curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8765/status 2>/dev/null || true)
    if echo "$s" | python3 -c 'import sys,json;d=json.load(sys.stdin);sys.exit(0 if d.get("input_listener_ok") else 1)' 2>/dev/null; then
      echo "$s" | python3 -c 'import sys,json;d=json.load(sys.stdin);print("OK", {k:d.get(k) for k in ["input_listener_ok","input_monitoring_ok","accessibility_ok","keydown_count"]})'
      exit 0
    fi
    sleep 2
  done
  echo "Still not ready — confirm both toggles are ON for /Applications/DeskCap.app, then: open -a DeskCap"
fi
