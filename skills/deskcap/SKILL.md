# DeskCap — local office capture knowledge (read-only)

**Product version: 1.0.22**（Mac IME **0.6.55**；本机 macOS **27**）。macOS 正式版；Windows 安装包可测（需 WebView2）；Linux 代码已适配、安装包未发。

This Skill text is **built into the DeskCap Agent tab** (collapsed by default — expand to copy).  
Customers do not need a separate `SKILL.md` file.

## Contract (locked)

| Rule | Detail |
|------|--------|
| Read-only | Never write/delete/patch DeskCap storage |
| No cloud | `local_only` / `cloud_upload: false` — data stays on the capture PC |
| LAN OK | API binds `0.0.0.0:8765`；Hermes uses `http://<employee-lan-ip>:8765` + **that user's** Bearer |
| Mutations | UI only（export 时段切片；**无导入**） |
| Self-exclude | DeskCap.app never returned |
| Secrets | Never echo tokens/keys/passwords；每人 token 只给自己的 bot |
| No re-ingest | Watermark + dedupe in **agent DB** |

## What agents should collect

| Need | Endpoint | Returns |
|------|----------|---------|
| App 活跃度 / **点击次数** | **`/knowledge/day/summary?date=`** | `by_app[].mouse_click` 等汇总 |
| 打字 / 剪贴板正文 | `/knowledge/day?date=&since=&limit=` | `typing` + `clipboard_*` only；正文在 **`data.text`** |
| Live 文本尾 | `/knowledge` | 近 ~80 条文本 |

### Hard rule — clicks

**禁止**引导 Agent 拉取逐条 `mouse_click`（如「微信 · left」连刷）。  
点击只看 summary 里的 **次数**；明细 API **默认不返回** `mouse_click`。

```
# 对：今天微信点了多少次
GET /knowledge/day/summary?date=今天  →  by_app[name=微信].mouse_click

# 错：分页拉一整天 left/left/left …
GET /knowledge/day   # 不再含 mouse_click
```

UI 时间轴仍可显示点击（给人看）；Agent 契约以 API 为准。

## Auth（Hermes 局域网）

```bash
# Token on the capture PC (or store once in the employee's bot secrets)
# macOS
TOKEN=$(cat "$HOME/Library/Application Support/DeskCap/agent.token")
# Windows（PowerShell）
TOKEN=$(Get-Content "$env:APPDATA\DeskCap\agent.token" -Raw).Trim()
# Linux
TOKEN=$(cat "${XDG_CONFIG_HOME:-$HOME/.config}/DeskCap/agent.token")

AUTH="Authorization: Bearer $TOKEN"
# From Hermes host — use the employee PC LAN IP (DeskCap Agent tab /status.agent_api)
BASE=http://192.168.x.x:8765
# On the capture PC itself you may also use:
# BASE=http://127.0.0.1:8765

curl -sS "$BASE/health"   # public
curl -sS -H "$AUTH" "$BASE/status"
```

Token: `dc_` + UUID v4（~122-bit random）。Never paste into chat/cloud.  
Wrong token → 401. Open firewall TCP **8765** on the capture PC (macOS / Windows / Linux).

## Recommended flow

```
1. GET /health
2. GET /status
3. GET /knowledge/day/summary?date=今天     # 点击量 / App 排名 — 通常到此足够
4. 仅当需要正文:
   GET /knowledge/day?date=&limit=80&since=cursor
   → typing/clipboard → 去重 → 隐私
   → **分类**（cjk / pinyin / english / noise）→ 拼音仅试译+confidence → 丢弃噪声
   → agent DB（必留 raw_text）
   → cursor = next_since
```

**禁止：** 把拼音静默写成唯一汉字当事实；把英文当拼音硬译；把 UI 残片/`正在进入` 写进用户意图。

## What is captured (honest — not “all operations”)

DeskCap records **local office signals**, not a perfect screen replay.

| 能采到 | 通常采不到 / 故意不采 |
|--------|----------------------|
| 键盘打字（分段 `typing`）、剪贴板纯文本 | 支付 / 密码窗 / DeskCap 自身（排除） |
| 点击**次数**（summary） | 逐条点击坐标流（Agent API 不提供） |
| 前台应用切换（session 内有；知识 API 以文本为主） | 鼠标轨迹、像素级画面（默认无截图） |
| Mac：多数 App 汉字较好（AX） | 微信/企微：常只有**拼音拉丁** |
| Win：记事本等 Win32 汉字较好（UIA） | Win×某些 Electron 编辑器/Chrome：弱项，不承诺 |

**结论：** 不能说「所有操作 100%」；Agent 应用 **summary 看活跃度**，用 typing/clipboard 还原「在写什么 / 聊什么」，并标注置信度。

## Agent：拼音 / 英文 / 错字解读（必读）

DeskCap **不做**服务端拼音转汉字，也不保证单条翻译正确。Agent 入库前必须自己判定，**禁止**把噪声当用户终稿。

### 0) 时间窗上下文（微信拼音 —— 优先于单条硬译）

单条拼音歧义大；**同一时段整段一起猜**才稳：

1. 取微信（企微）在目标时间窗内的 **全部** `typing` + 邻近 `clipboard_*`（建议窗：当前查询的 `since`～`next_since`，或墙钟 ±3～10 分钟）  
2. 按 `wall_ts` 排序，把连续拼音段合成一篇「草稿」再解码（见下）  
3. 用同窗 **汉字剪贴板 / 已是 CJK 的 typing / summary 主题** 作约束  
4. 输出整段 `normalized_text` + 每条仍保留各自 `raw_text`  
5. 后段覆盖前段（退格改正、前缀加长）→ 标 `superseded`，以窗内终稿为准  

Capture 侧（微信）：**正在拼写时**空格 / 1–9 选字 **不写入确认键**；完整音节插 `'`。**声母未完成**（`sh`/`zh`/`d` 等）**绝不丢弃**——空格只保持继续拼写（否则会出现 `shuode`→`ode`）。落盘 scrub 只去掉夹在中间的单字母噪声。退格：有 `'` 时按音节删，尾标点先删。

### 1) 先分类再处理

对每条 `typing.text`：

| 判定 | 条件（启发式） | Agent 动作 |
|------|----------------|------------|
| **已是汉字正文** | 含大量 CJK | 直接用；可轻量纠错，勿大改 |
| **拼音草稿** | 几乎全 ASCII 字母，出现在微信/企微/QQ 等；像音节串（`nihao`/`woshishui`/`women'zai`） | 标 `script=pinyin_latin`；**放入时间窗再译**；**必须**带 `confidence` 与原文 |
| **英文正文** | ASCII，但是词/代码/URL/邮件（空格分词、常见英文词、`http`、`Send follow-up`、标识符） | **不要**当拼音翻译；标 `script=english_or_code` |
| **混合** | 汉字后拖尾拼音、或英文夹拼音 | 只译明确的拼音段；保留已确认汉字 |
| **UI/无效噪声** | 极短重复（`助手助手`）、纯标点、`正在进入`、候选栏残片、与前后文无关的单字母连打 | **丢弃或降权**，勿写入「用户说了…」 |

### 2) 拼音「翻译」——禁止承诺 100%；窗内猜准

同音字极多（`shi`→是/事/时/十…）。正确做法：

1. **保留原文** `raw_text`（永远可回放）  
2. 按 `'` / 自然停顿切音节；可用 Rime/词典出 Top‑K，再用 **时间窗上下文** LLM 重排  
3. 写入 `normalized_text` + `confidence`：`high` / `medium` / `low`  
4. `confidence=low` 或无把握时：**只展示拼音原文**，或写「可能是…」，**禁止**当事实引用  
5. 历史脏数据可能仍含选字数字后缀（`hao3`）——翻译时去掉再猜；新版本确认键不再入库  

**永远不要：** 静默改成唯一汉字却不留 raw；把整段英文当拼音硬译；**只拿一条拼音无上下文就当用户原话**。

### 3) 英文 vs 拼音（易混）

| 更像英文 | 更像拼音 |
|----------|----------|
| 有空格分词：`smart office` | 无空格长串：`womenyingyonghui` |
| 常见英文词 / 代码 / 路径 / URL | 声母韵母堆叠、偶发 `'` 分词 |
| 某些 Electron 编辑器 UI：`Send follow-up` | 微信里一串 slurp 字母 |
| 含 `th`/`ing`/`ion` 等英文形态 | 以 `zh/ch/sh/ang/eng/ong` 等音节为主 |

不确定 → `script=ambiguous_latin`，**两路都不要独断**：可附「若为拼音则…；若为英文则…」。

### 4) 打错字 / 半截输入 / 重复段 / 退格次数

Capture 是**分段时间线**，不是输入法终稿：

- 用户退格改字前，可能已 idle flush → 库里会有**错误前缀**或**拼音半截**  
- 微信 typing 段带 `backspace_count`（本段内 Backspace **按键次数**）。窗内解码时：  
  - 后段覆盖前段（前缀加长 / 改正）→ 标 `superseded`  
  - `backspace_count>0` 时优先信**较晚、更短或已带 `'` 确认界**的终稿，勿把已删错拼当用户原话  
- Capture：拼写中退格删字母；**确认后**（缓冲以 `'` 结尾）一次退格按「删一字」丢掉最后一段 `…'` 音节  
- 其它 App 已是汉字：AX/UIA LCP 即可，不必靠拼音退格启发式  
- 同一意图可能多条 typing（分段）；用时间邻近 + 文本前缀关系合并，**以后段/更长段为准**  
- 后段是前段的加长 → 合并，勿当两句不同话  
- 后段与前段矛盾（明显改正）→ **采纳后段**，前段标 `superseded`  
- 明显误触（单键、重复 UI 词）→ 不进入摘要结论  

### 5) 入库字段建议（agent DB）

```
raw_text          # DeskCap 原文，必留
script            # cjk | pinyin_latin | english_or_code | mixed | noise | ambiguous_latin
normalized_text   # 可选；仅当 confidence≥medium
confidence        # high | medium | low
quality           # ok | partial | noise | superseded
app, wall_ts, id  # 去重用
```

### 6) 对用户叙述时的话术

- 有汉字：直接引用  
- 仅有拼音且 medium+：可以说「微信里输入了（拼音）…，理解为…（存疑）」  
- low / noise：**不要**编造完整中文句子冒充用户原话  

## 净化与高效提炼（0.6.58 — 本机实测规范）

**背景（2026-10-14 全天 1040 条 typing 实测）**：直接读原始 typing 会被无效数据淹没。

| 类别 | 实测占比 | 特征 | 处理 |
|------|---------|------|------|
| 汉字正文 | 85% | ≥2 个 CJK | 保留 |
| **短碎片** | **26%** | <5 字、无标点、非完整词（如「还能」「通过这个」） | **缝合**（见下），缝合后仍 <5 字才丢 |
| **拼音中间态** | **~6%** | 纯小写字母且音节可切分（`kai fa zhe`）；无空格长串 ≥4 字母也是 | 丢弃（学习场景除外） |
| 单字 / 纯符号 | ~2% | 1 个汉字、纯标点 | 丢弃 |
| 修正段 | ~2.4% | `backspace_count > 0` | 保留但标 `revised`（降权，勿当终稿） |

**碎片主因（已修）**：采集侧 `IDLE_FLUSH` 原为 1800ms，而相邻段间隔中位数
仅 1.6s —— 大量完整句子被阈值切碎。0.6.58 已调到 **3000ms**
（`typing::IDLE_FLUSH` 单一真源）。但历史数据与强切段（回车/切 App/点击）
仍会产生碎片，**提炼侧必须缝合**。

### 碎片缝合算法（四步，实测削减 65%）

```
输入：某日 typing 列表（按 wall_ts 升序）
1. 丢弃：拼音串（见上「拼音中间态」判定）+ 纯符号 + 空
2. 缝合：遍历剩余段，相邻两段满足全部条件则拼接
     a. 同一 app.name
     b. 时间间隔 < 10s（用 data.start_wall / end_wall，缺失用 wall_ts）
     c. 前段不以句末标点结尾（。！？!?；;）
   实测：198/359 条由多段缝成（平均 3 段/条）
3. 过滤：缝合后仍 <5 字符 → 丢弃
4. 去重：连续相同文本合并；id = sha256(type|wall_ts|app|text)
```

### 高效提炼流程（先聚合后正文，省 token 更快）

```
1. GET /knowledge/day/summary?date=今天    ← 先读聚合（零正文）
   回答「今天干了啥」80% 场景到此足够（App 排名 + 点击量 + 小时分布）
2. 仅当需要内容时：
   GET /knowledge/day?date=&limit=2000     ← 一次拉齐当日 typing+clipboard
   本地跑上面四步 → 得到有效集（1040 → ~360 条）
3. 把「有效集中 ≥30 字的长文本」交给本地 LLM 做主题汇总
   （实测 183 条；单批 ≤60 条，禁整日灌 prompt）
4. LLM 输出契约：按 时段 / 主题 / 涉及 App 的结构化总结；
   引用原文片段 ≤40 字；标 confidence（保留既有 raw_text 概念）
```

### 与本地大模型协作（隐私硬边界）

- **汇总必须用本机 LLM**（如 Qwen/mlx_lm 系）或本地脚本聚合；原始正文**禁止**出本机
- 云端 agent 只允许接触：`summary` 聚合数 或 本地脚本已净化的**主题标签**
- 一切中间文件（含正文的 json/临时导出）用完即删；勿落 `/tmp` 长期留存
- Token 每人一份，禁止贴入任何对话/工单；泄露疑虑时 UI 一键轮换

### 反例（禁止）

- 把碎片（「还能」「skill和」）当用户完整意图写进总结
- 把拼音中间态（`kai fa zhe`）当终稿
- 把 `backspace_count>0` 的段当最终表达（用户正在修正）
- 用云端模型读原始正文做「智能总结」

## 近几日实采复盘（2026-09-04 / 05 / 07，本机 daybook）

| 指标 | 约值 | 含义 |
|------|------|------|
| 汉字正文 | ~66% | 可用意图主体 |
| **噪声** | **~15%** | 勿当用户原话（Capture 现行会丢弃一类 chrome） |
| 拼音拉丁 | ~9% | 几乎全在**微信**（微信文本中拼音约占可判段 97%） |
| 英文/代码 | ~8% | 某些 Electron 编辑器/Chrome；**勿当拼音** |

**噪声 Top（历史数据）：**

1. 某些 Electron 编辑器 UI：`Send follow-up` / `end follow-up`（单日可上百条）— AX 误读聊天按钮  
2. `助手` / `助手助手` — AI 助手窗标题/标签  
3. `, Apple账户` / 通行密钥残片 — 系统设置 AX 误读  
4. `正在进入` / `进入微信` — 启动态 UI  

**Agent 仍须：** 拼音试译+confidence；`Public`/`Thought`/`Edited` 等单词偏英文；微信拼音保留 raw。

Capture 侧已对上述 chrome 做 `is_agent_noise_text` 过滤（落盘前丢弃），Agent 侧规则仍保留作双保险。

## Dedup / batching（仅正文）

- id = sha256(type|wall_ts|app|text)
- 每轮 30–80 条文本；禁止整日 JSONL 灌进 prompt
- 合并 superseded / 前缀加长段后再入库，避免「同一句翻成多条事实」

## Privacy

Capture 侧已 `[REDACTED]`；Agent 入库前再扫一遍。拒绝展示 Token/密码。

## Export

UI：所选日 + HH:MM–HH:MM（或全天）→ `exports/`。无导入。

## Platform

macOS **1.0.22** 正式版。Windows x64 NSIS 可测（同版本；托盘在任务栏右下角；需 WebView2）。Linux 代码已适配、安装包未发。
macOS 权限（输入监控 / 辅助功能）仅 macOS 需要，Win/Linux 界面不展示。

安装包（可见目录）：`releases/DeskCap_1.0.11_x64-setup.exe`（构建后跑 `scripts/copy-win-release.sh`）。

### 图标（多端）

- macOS Dock：`icon-mac.png` → `icon.icns` 不透明全铺（系统裁圆角）。
- Windows / Linux：从 mac 母版**抠掉外围直角底板** → `icon-win.png` / `app-icon.png`（仅圆角 logo + 透明）→ `pnpm tauri icon`。
- 应用内：`src/assets/symi-mark.png` 勿为系统图标改动。

### Typing / IME（诚实上限）

| 场景 | 预期 |
|------|------|
| 微信 / 企微（Mac + Win） | 系统拼音下常见为**拼音拉丁**；OS 钩子看不到上屏汉字 → Agent **可试译但非 100%** |
| Mac + **DeskCap拼音**（IME 0.6.47） | `insertText` → `/ime/commit`；候选跟插入点；可选候选译词（默认关）；激活时不双记 EventTap 拼音 |
| Win 其它 Win32 应用 | 英文：VK；中文：UIA **仅 LCP 增量**（与 Mac 分段一致） |
| Win 某些 Electron 编辑器 / Chrome / 某些 Electron 编辑器 / WebView2 | **不承诺**与 Mac 同级。Win 验收勿以 某些 Electron 编辑器 为门禁（见 `docs/windows-capture.md`） |
| Mac 任意应用（含 某些 Electron 编辑器） | EventTap 全局可用 + AX；汉字精准靠 DeskCap拼音 |

详见 `docs/ime-capture.md`、`docs/windows-capture.md`。解读规则见上文「Agent：拼音 / 英文 / 错字解读」。

### Windows 排障：导出日志给研发

API 监听 **`0.0.0.0:8765`**（局域网可达）。Hermes 用员工 LAN IP + 该员工 token。排障日志仍在 **DeskCap 所在电脑**；**勿**贴 token。

#### 1) 单实例（现行）

DeskCap **全平台单实例**：第二次启动会聚焦已有窗口并退出，不会出现两个 `deskcap.exe`。若仍见双进程，多半是**旧版**；请升级后任务管理器结束残留再开一次。

旧版双开时心跳会 `capturing=true/false` 隔秒交替，易误判为「采集开关在闪」。

#### 2) 复现后导出日志尾

**Windows（PowerShell）**

```powershell
Get-Content "$env:APPDATA\DeskCap\logs\deskcap.log" -Tail 100
# 或资源管理器打开：%APPDATA%\DeskCap\logs\
```

**macOS**

```bash
tail -n 100 "$HOME/Library/Application Support/DeskCap/logs/deskcap.log"
```

#### 3) 同机附带 /status（不要带 Authorization 明文到群）

```powershell
$t = (Get-Content "$env:APPDATA\DeskCap\agent.token" -Raw).Trim()
Invoke-RestMethod -Headers @{Authorization="Bearer $t"} http://127.0.0.1:8765/status |
  Select-Object state,capturing,excluded_now,event_count,current_app | ConvertTo-Json
```

研发对照：

| 现象 | 含义 |
|------|------|
| `keydown`↑ 且 `events` 不涨、`excluded_now=true` | 排除门（曾误伤标题含 deskcap 的 某些 Electron 编辑器） |
| `keydown` 也不涨 | 钩子/轮询挂了或进程卡死 |
| 心跳 `capturing` 一秒一变 | 多半双实例 |
| UI「跳过」且应用名 DeskCap | 正常（看本窗） |

#### 4) 「跳过」与工程名 deskcap

自排除只认 **应用名恰好为 DeskCap** 或 **二进制路径**，**不再**用标题/文件夹名含 `deskcap` 排除。某些 Electron 编辑器 标题 `deskcap - <编辑器名>` 应可采（Win×某些 Electron 编辑器 仍有 Electron 弱项，见上表）。

### Windows 卡顿取证（本机 Agent）

同上；卡顿时 Tail 日志搜 `WARN lag`（`uia_read` / `input_worker`），再附 `/status` 与进程 WorkingSet。
