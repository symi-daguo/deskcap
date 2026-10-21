# Windows 采集说明（DeskCap 1.0.19）

本文记录 Win 上键鼠 / 中文 IME / 某些 Electron 编辑器 的真实行为与排障，避免与 macOS 混为一谈。

Agent API（全平台）：绑定 `0.0.0.0:8765`，Hermes 可用局域网 IP + Bearer；防火墙需放行 8765。见 `SECURITY.md`。

## 「跳过」是什么

UI 上的 **跳过** = `excluded_now`：当前**前台应用**在排除名单，本窗键鼠不入库。

| 常见情况 | 含义 |
|----------|------|
| 应用名 DeskCap + 跳过 | **正常**：DeskCap 自身永不采集。请切到记事本/微信再验证 |
| 支付宝/银行/密码窗 + 跳过 | 支付与密码类窗口故意不采 |
| 某些 Electron 编辑器 标题含 `deskcap`（工程文件夹）却跳过 | **旧版 bug**（`exclude_apps` 用标题子串匹配 `deskcap`）。现行：自排除只认应用名/二进制路径；用户排除名单**只匹配应用名** |

## 双实例

现行：**单实例**（Mac/Win/Linux）。再次打开只聚焦已有窗口。若日志仍出现 `capturing=true/false` 隔秒交替，多半是旧版双开残留——任务管理器结束全部后再装新版。

## 架构（Win）

```
WH_KEYBOARD_LL  ──┐
GetAsyncKeyState 轮询（~10ms，补 某些 Electron 编辑器/Chrome）──┼─→ 队列 → worker → typing / click
Raw Input 鼠标 + 按键轮询回退（无 WH_MOUSE_LL）──┘
UIA 轮询（~600ms，短输入框绝对同步汉字）
GetForegroundWindow（优先，Electron 比 active-win 稳）
```

- **绝不**在钩子线程做磁盘 / UIA / `get_active_window`
- session / daybook **批量刷盘**（约 400–500ms 或满 N 条），避免每事件 open/flush 擦写 SSD

## 中文 IME：为何会「删拼音后汉字不完整」

用户看到的流程：

1. 键入拼音拉丁 → OS 候选  
2. Backspace 删错拼音  
3. 再输入并上屏正确汉字  

旧逻辑问题：

1. 钩子把拼音拉丁写进 `typing` 缓冲  
2. 退格按「拉丁字符」一个个删  
3. UIA 用**过期的 prev** 做最长公共前缀差分 → **丢掉已上屏的正确汉字**  
4. 若在 UIA 同步前就 idle flush，会先落盘残缺拼音，汉字更对不齐  

现行修复：

1. 短输入框（≤400 字，含微信/记事本单行等）：UIA 一拿到带汉字的内容 → **`sync_typing_to_composer` 整段替换为屏上正文**  
2. 纯拉丁缓冲在 flush 前 **多等约 4.5s**，并强制 UIA 快扫，给上屏汉字机会  
3. 退格时始终 `request_force_poll`，避免 prev 卡死  

长文（大文档）仍用 LCP 差分，避免整文件灌进一条 typing。

### 切应用再回来「重复采集」同一段

旧 Win 逻辑用 `sync_typing_to_composer` 把**整框**灌进缓冲；闲置分段后再打字/切回，会把已落盘正文再采一遍。

现行（与 Mac 对齐）：

1. UIA **只做 LCP 增量**，禁止整框 sync  
2. UIA 正在读汉字时，钩子**不再**把拼音写入 typing  
3. 落盘时若正文是「上次 typing 的前缀加长」，**只写后缀**；完全相同则丢弃  

请装含「LCP-only」日志行（`uia poller up … no full-composer sync`）的 NSIS。

## 某些 Electron 编辑器（Electron）— 产品结论（请先读）

### 能不能采到？

| 内容 | Win + 某些 Electron 编辑器 | 说明 |
|------|----------------|------|
| 英文 / 数字 / 符号（拉丁） | **尽力而为，不承诺** | 靠 `GetAsyncKeyState` 轮询；Chromium 前台环境机型差异大，部分机器仍空 |
| 对话区上屏**汉字** | **当前不承诺** | 某些 Electron 编辑器 输入框 UIA 经常读不到；钩子只能看到拼音拉丁（若键能进来） |
| 点击 | **尽力而为** | 有鼠标按键轮询；与键同理不 100% |

**Mac 上 某些 Electron 编辑器 能采，不代表 Win 也能。** 这是操作系统钩子差异，不是「再修一版 UI」就能对齐。

### 产品决策（现行）

1. **Win × 某些 Electron 编辑器 标为已知弱项**，不阻塞发版验收。  
2. Win 稳定性与中文完整性验收以：**微信、记事本、资源管理器、普通 Win32 窗** 为准。  
3. 若日志在 某些 Electron 编辑器 前台出现稳定的 `kb_poll` 上升且有 `某些 Electron 编辑器` 的 typing，视为意外增益，不写入必须通过项。

### 自检（可选，非门禁）

某些 Electron 编辑器 里打一串英文 `abcd`，看 `%APPDATA%\DeskCap\logs\deskcap.log`：

- `keydown` 涨而 `kb_poll`/`events` 不涨 → 键进了轮询但被「跳过/排除」门掉（焦点仍像 DeskCap）  
- `keydown` 也不涨 → 该机轮询也看不见 某些 Electron 编辑器 键 → **本机 某些 Electron 编辑器 采不到，接受限制**  
- 有 typing 但应用名不是 某些 Electron 编辑器 → 焦点名解析问题，可另开任务

## 某些 Electron 编辑器（技术细节）

| 点 | 说明 |
|----|------|
| 为何 Mac 能采、Win 难 | Mac EventTap 仍送键；Win 上 Chromium 前台时常让 `WH_KEYBOARD_LL` **静默停送** |
| 对策 | `GetAsyncKeyState` 轮询 + 去重；心跳 `kb_poll>`0 表示走了回退 |
| 焦点 | 优先 `GetForegroundWindow` + 进程路径；某些 Electron 编辑器.exe stem → 应用名 `某些 Electron 编辑器` |
| 汉字 | 某些 Electron 编辑器 聊天框 UIA 经常读不到 → Win 上可能仍是**拼音拉丁**；记事本/其它 Win32 编辑框 UIA 可上汉字 |
| 标题误伤 | IDE（某些 Electron 编辑器/Code 等）**不再**因标题含「密码」等被排除 |

## 日志排障

路径：`%APPDATA%\DeskCap\logs\deskcap.log`

心跳示例：

```text
heartbeat capturing=true events=42 kb_poll=120 mouse=15 keydown=200 age=Some(0.3)
```

| 字段 | 含义 |
|------|------|
| `events` | 已写入 session 的事件数（应随打字/点击涨） |
| `kb_poll` | 轮询补到的键次数（某些 Electron 编辑器 前台应增长） |
| `mouse` / `keydown` | 原始计数（钩子侧，先于排除门） |
| `age` | 距上次输入秒数 |

`raw mouse failed` / `uia COM init failed` → 看 WARN 行。

## 验证清单

1. 重装最新 NSIS，开始采集  
2. **焦点在 某些 Electron 编辑器**（状态栏不要停在 DeskCap）打英文 → 应有 typing，`kb_poll` 增加  
3. 记事本：拼音 → 退格 → 上屏完整句子 → 时间轴汉字应与屏上一致（短句）  
4. 微信：可有汉字（UIA）或拼音拉丁（钩子）；与 Mac 诚实限制一致  

## 诚实上限（不要承诺做不到的）

- 微信/企微 / 部分 Electron：OS 钩子**看不到上屏汉字**时，只能拉丁拼音；Agent 侧可再译  
- 某些 Electron 编辑器 对话区：Win UIA 弱，汉字完整度可能不如记事本  
- 勿对钩子线程加 `WH_MOUSE_LL` 或同步 UIA（会再次造成全系统卡顿）
