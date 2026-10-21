# Security

DeskCap stores capture data **on the employee PC** and does **not** upload to any cloud.
Office Hermes bots may pull **read-only** over the LAN using each user’s own Bearer token.

## Guarantees (product)

| Control | Behavior |
|---------|----------|
| Network bind | Agent API listens on `0.0.0.0:<port>` (default **8765**) so LAN bots can connect |
| Auth | Sensitive routes require `Authorization: Bearer <token>` (constant-time compare) |
| `/health` | Public; capability flags only — **no token, no event payloads** |
| Storage | JSONL under local data dir (`0700` / files `0600` on Unix) |
| Cloud | No telemetry, no analytics, no remote sync in this binary (`local_only` / `cloud_upload: false`) |
| Isolation | One DeskCap install = one token; give token only to **that employee’s** Hermes bot |
| Payment | Locked app + window-title rules always skip capture (cannot be disabled) |
| Fonts / CDN | UI uses system fonts; no Google Fonts or third-party asset fetches |
| IME 译词 | 候选译词表**纯本地**（bundle + `~/Library/Application Support/DeskCap/rime/`），不联网、不外传；查词只做本地字典查表 |

## Agent API (Hermes / LAN)

```bash
# On the capture PC — discover LAN base from UI Agent tab, or:
#   GET /status → agent_api  (e.g. http://192.168.x.x:8765)

# Token files (never commit / never paste into group chat)
# macOS:  ~/Library/Application Support/DeskCap/agent.token
# Windows: %APPDATA%\DeskCap\agent.token
# Linux:   ${XDG_CONFIG_HOME:-~/.config}/DeskCap/agent.token

TOKEN=$(cat …/agent.token)   # on that PC, or copy once into the employee's bot secret store
BASE=http://<employee-lan-ip>:8765

curl -sS "$BASE/health"
curl -sS -H "Authorization: Bearer $TOKEN" "$BASE/status"
curl -sS -H "Authorization: Bearer $TOKEN" "$BASE/knowledge/day/summary?date=$(date +%F)"
```

Wrong or missing Bearer → **401**. Token never returned from `/config`.

### Firewall (required for LAN)

| OS | Action |
|----|--------|
| macOS | System Settings → Network → Firewall → allow **DeskCap** (or TCP 8765) |
| Windows | Allow DeskCap / inbound TCP **8765** in Windows Defender Firewall |
| Linux | e.g. `sudo ufw allow 8765/tcp` (or equivalent nftables/firewalld) |

## Residual risks (honest)

- LAN traffic is **HTTP** (not TLS). Use on trusted office networks only; treat token as a password.
- Any process that can read `agent.token` (same user) can read events — protect the user account.
- Exclusion depends on OS foreground window metadata; a ~100ms race remains when switching into a payment UI.
- Screenshots (if enabled) can capture sensitive pixels; default interval is `0` (off).
- Mac 部署与资源实测见 `docs/mac-ops.md`；微信汉字率与 IME 可选路径见 `docs/ime-capture.md`。

## Report issues

Treat keystroke logs as sensitive. Rotate the API token from the DeskCap UI after any suspicion of compromise (chat leak, shared PC, etc.).

## Reporting a vulnerability

请**不要**用公开 issue 报告安全漏洞。

- **首选**：通过 GitHub 私有漏洞报告提交
  （仓库页 → `Security` 标签 → `Report a vulnerability`），
  报告内容只有维护者可见，修复后才会公开讨论。
- **报告内容请包含**：复现步骤、受影响版本（DeskCap / DeskCapIME）、影响面
  （是否涉及令牌泄露、日账内容外泄、采集越界）、以及 PoC 或日志片段。
- **令牌相关**：怀疑令牌泄露时，**先在 DeskCap 设置页轮换令牌**，再报告。
  不要在 issue / 聊天里粘贴令牌、日志原文或含敏感内容的截图。

### 支持范围

| 组件 | 支持 |
|---|---|
| DeskCap 桌面端（macOS / Windows） | ✅ |
| DeskCapIME 输入法（macOS） | ✅ |
| 局域网 API 鉴权与数据边界 | ✅ |

已在 `docs/mac-ops.md` 与 `docs/ime-capture.md` 中公开的取舍（HTTP 明文、
元数据判定的 ~100ms 竞态、截图默认关闭等）属于**已知限制**，
不是新漏洞；如认为其中有可利用缺口，仍欢迎私下报告。
