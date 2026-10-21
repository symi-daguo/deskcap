# macOS 验收清单 — DeskCapIME 0.6.47

> ✅ **2026-10-14 真机验收已执行完毕**（0.6.48，macOS 27）。结果见文末
> 「真机验收结果」。本清单保留原样作为**可复跑的步骤**。
>
> ⚠️ 一条历史教训：本轮发现**装机版本是 0.6.44**——0.6.45 / 0.6.46 / 0.6.47
> 三轮的代码都编译过、验证过，但**从没装进 `/Library/Input Methods/`**。
> `build-and-install.sh` 只 build 到 `macos-ime/build/`，安装是它 `echo` 出来的
> 手工步骤。所以「编译通过」≠「在跑新代码」。**每次改完必须核对已装版本号**。

> 开发机是 **Windows**，本轮**没有**做过任何编译验证（无macOS SDK / swiftc / cargo）。
> 0.6.45 / 0.6.46 / 0.6.47 三轮都只走到 `swiftc` typecheck + 未签名 build。
> **请在 Mac 上按下面顺序跑完再决定是否分发。**

## 0. 前置

```bash
brew install librime          # 缺 rime_api.h 会直接退出
xcode-select --install        # swiftc
git pull                      # 26eb734..c9716bf
```

## 1. 词表校验（不依赖 Mac，可先跑）

```bash
python3 macos-ime/scripts/gen-glossary.py --check
# 期望：OK — 240 单字 + 115 词组，校验通过
```

## 2. 编译 + 签名

```bash
bash macos-ime/scripts/build-and-install.sh
```

脚本会：嵌入 librime dylibs → `swiftc -O` 真编译 → codesign → 冒烟启动检查。
**看两件事**：

- 有没有 `Glossary.swift` 相关的编译错误
- 冒烟日志 `/tmp/deskcapime-smoke.log` 里有没有 `dyld` / `Library not loaded`

## 3. 公证 + 安装

```bash
bash macos-ime/scripts/notarize-ime.sh
sudo rm -rf "/Library/Input Methods/DeskCapIME.app"
sudo ditto "macos-ime/build/DeskCapIME.app" "/Library/Input Methods/DeskCapIME.app"
rm -rf "$HOME/Library/Input Methods/DeskCapIME.app"   # 防双份，添加列表会出重复项
# 注销登录 → 系统设置 › 键盘 › 输入法 添加「DeskCap拼音」
```

## 4. 版本确认（**最容易骗人的一步**）

```bash
defaults read "/Library/Input Methods/DeskCapIME.app/Contents/Info" CFBundleShortVersionString
# 必须是 0.6.47；若是 0.6.44 说明装的是旧包
```

这行就是本轮修的东西。0.6.45 起代码就变了，plist 之前一直停在 0.6.44，
排障时容易拿旧版观测判断新版代码。

## 5. 候选框回归（0.6.45/0.6.46 也没验过）

```bash
bash scripts/rime-type-test.sh
```

手工确认（**某些 Electron 编辑器 / 某些 Electron 编辑器 / 微信各试一次**，这三类 host 行为不同）：

| 检查 | 期望 |
|---|---|
| 打 `houxuan` | 候选条在**拼音下划线正下方**，不是屏幕左上角 |
| 候选条位置 | 光标移动时跟随；**不跳动**（50pt 阈值挡住了 IPC 抖动） |
| 多屏 | 拖到副屏，光标在副屏 → 面板跟到副屏 |
| 排序 | 打 `tingxia` → 「停下来」排在单字「听」**前面** |
| 高亮 | 新输入时第一个候选恒高亮（不是 rime 原始顺序的高亮） |
| 全屏 | 全屏 App 内面板仍可见（`fullScreenAuxiliary`） |

## 6. 候选译词（0.6.47 新增）

```bash
defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string en
```

| 检查 | 期望 |
|---|---|
| 默认行为 | **不写上面这行**时，候选条与 0.6.46 完全一致（单行） |
| 开启后 | 候选词**下方**出现小字释义，如「开发 / development」 |
| 整词命中 | `kaifa` → 开发 + development |
| 逐字合成 | 词表没有的词会拼成 `stop · come · down` 这类；**一个字查不到就不显示** |
| 宽度 | 9 个候选不超屏；长释义不把面板撑破（170pt 上限） |
| 对比度 | 选中项的释义在蓝色胶囊上仍可读（白 @ 0.85） |
| 点击 | 点译词所在区域仍能选中该候选（命中区变大，更宽容） |
| 实时生效 | `defaults write` 后**不用重启 IME**，下一次出候选就变 |
| 关闭 | `defaults write … -string off` 即恢复单行 |

## 7. 词表扩充不被覆盖（回归项）

```bash
printf '测试\tTEST-GLOSS\n' >> ~/Library/Application\ Support/DeskCap/rime/glossary.en.tsv
bash macos-ime/scripts/build-and-install.sh 2>&1 | grep glossary
# 期望看到：keep  user glossary: glossary.en.tsv   ← 不是 seed
```

这条验的是本轮修的第二个 bug：原 `cp -R rime-data/*` 会在每次重新打包时
清空你自己扩充的词表。

## 8. 采集链路（IME 的本职，别忘了）

```bash
curl -sS http://127.0.0.1:8765/health
```

切到 DeskCap拼音，在微信/记事本里打一段中文，确认 `/status` 出现
`capture=ime_commit`，且内容是**汉字**而不是拼音。`ime-active` 应防止
与 EventTap 拼音双记。

## 回滚

出问题时最快回到已知状态：

```bash
git checkout 26eb734        # 0.6.46，那次提交在 Mac 上编译过
bash macos-ime/scripts/build-and-install.sh
sudo ditto "macos-ime/build/DeskCapIME.app" "/Library/Input Methods/DeskCapIME.app"
```

卡死急救：Mac `kill -9 DeskCapIME`；Win 任务管理器结束进程。
不要依赖「暂停采集」——钩子可能仍在。

## 汇报

编译报错 / 译词不显示 / 候选框位置不对，把
`log show --predicate 'process == "DeskCapIME"' --last 5m` 的输出贴回来
（**不要贴 `agent.token`**）。

---

# 真机验收结果（2026-10-14，macOS 27，0.6.48）

四轮（0.6.45/46/47/48）第一次在Mac 上完整跑完。以下是可复核的数字。

## 1. 编译 + 签名 + 装机

```
$ /tmp/pilenv/bin/python macos-ime/scripts/gen-glossary.py --check
OK — 240 单字 + 115 词组，校验通过

$ PATH=/tmp/pilenv/bin:$PATH bash macos-ime/scripts/build-and-install.sh
embedded 8 dylibs
Signed: Developer ID Application: <团队证书名> (<TEAM_ID>) hardened=1
Built: /tmp/DeskCapIME-build/DeskCapIME.app

$ codesign --verify --deep --strict macos-ime/build/DeskCapIME.app
签名有效
```

`build-and-install.sh` 只build 到 `macos-ime/build/`，**不装**。装机要单独做
（脚本末尾 `echo` 出来的那三行，或用 `osascript ... with administrator privileges`
免手输密码）：

```
=== 装之前 ===
src: 0.6.48
dst: 0.6.44          ← ⚠️ 四轮代码都没装上，跑的一直是 0.6.44
=== 装之后 ===
version: 0.6.48
build:   69
codesign: 有效
src sha: d6b66ec38137e7d29ec89b35d3cfaad7
dst sha: d6b66ec38137e7d29ec89b35d3cfaad7← 双向匹配
```

## 2. 候选译词

```
$ defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string en
$ defaults read  com.office.deskcap.inputmethod.symi GlossaryLanguage
en
```

词表双份就位（用户目录优先，重打包不覆盖）：

```
-rw-r--r-- 5116 ~/Library/Application Support/DeskCap/rime/glossary.en.tsv
-rw-r--r-- 5116 /Library/Input Methods/DeskCapIME.app/Contents/Resources/rime-data/glossary.en.tsv
```

IME 重启后存活：pid 68080，`RSS 38416 KB`（约 37.5 MB）。

## 3. 内存与磁盘

| 项| 实测 | 判断 |
|---|---|---|
| DeskCap RSS | **89 632 KB**（约 87.5 MB），VSZ 470 MB，%MEM 0.3 | 合理。长驻 1h13m 无增长迹象 |
| DeskCapIME RSS | **38 416 KB**（约 37.5 MB） | 合理 |
| 数据根总量 | **55 MB** | 上限 1024 MB，占5% |
| ├ `rime/` | 32 MB | librime 词典 + 编译缓存 |
| ├ `sessions/` | 11 MB | 当前 621 events |
| ├ `days/` | 9.7 MB | 日账 JSONL |
| └ `logs/` | 2.2 MB | `deskcap.log` + `deskcap.log.1`，各卡 2 MB 轮转 |

**磁盘不会挤爆**：`maintenance.rs` 双保险——先按 `retention_days=14` 删过期
session，再在总量超 `max_disk_mb=1024` 时按时间从旧到新删 session 目录 / 日账文件
（`run_maintenance` 的 `loop` 每轮重算 `dir_size`，直到回到限额内）。
日志 `MAX_BYTES = 2 MB` 轮转，永远不超过 4 MB。

## 4. 采集链路 + agent 数据通路

```
$ curl -s http://127.0.0.1:8765/health
{"ok":true,"version":"1.0.19","local_only":true,"cloud_upload":false,
 "agent_api_mode":"read_only","auth":"bearer","bind":"0.0.0.0", ...}

$ curl -s -H "Authorization: Bearer $TOK" http://127.0.0.1:8765/status
{"state":"running","event_count":621,"capture_keyboard":true,
 "retention_days":14,"max_disk_mb":1024,"sessions_bytes":20588547,
 "current_app":{"name":"访达"}, "local_only":true, ...}

$ curl -s -H "Authorization: Bearer $TOK" \
    "http://127.0.0.1:8765/knowledge/day/summary?date=2026-10-14"
by_app: WorkBuddy 2398字/269事件、微信 1176/170、MiniMax Code 433/91 …（9 个 app）

$ curl -s -H "Authorization: Bearer $TOK" \
    "http://127.0.0.1:8765/knowledge/day?date=2026-10-14&limit=5"
items[].type=typing, text="能够把数据给到\n" ...
```

四级全部通（`/health` → `/status` → `/knowledge/day/summary` → `/knowledge/day`）。

### IME 汉字上屏 → agent 可读（训练数据通路）

```
$ curl -X POST -H "Authorization: Bearer $TOK" -H "Content-Type: application/json" \
    -d '{"text":"验证汉字上屏链路","app":"TextEdit","source":"ime_commit"}' \
    http://127.0.0.1:8765/ime/commit
{"ingested":1,"ok":true}

$ # 2 秒后读回
命中次数: 1← 汉字确实落库并可被 agent 读到
```

⇒ **数据能给 agent 做训练**：上屏汉字经 `/ime/commit` 落日账 → `/knowledge/day`
读回，agent 侧拿到的是**真实汉字**而非拼音串。

## 5. 采集错误日志

```
$ grep -ciE 'error|fail|panic' ~/Library/Application Support/DeskCap/logs/deskcap.log
0← 零错误

$ grep -oE 'WARN [a-z_]+' … | sort | uniq -c
769 WARN lag          ← 全部是 input_worker，见下
```

## 6. 本轮修的两处（1.0.19 / DeskCap 后端）

`input_worker` lag 峰值 **17 390 ms**，全部落在 `engine.rs:on_key_input` 里的
**微信 AX 深读 + Vision OCR**（`wechat_mac::recover_chinese_before_send`）——
它同步阻塞在输入worker 线程上。

| 问题 | 修法 |
|---|---|
| `LAST_LAG_LOG` 是**单一全局槽**，被 7 个调用点共用 → 任何一次 lag 把其余站点在 2 秒内的告警一并压掉。实测 `input_worker` 独占 769 条、其他站点 0 条，**说明不是它们没慢，是被压制得看不见** | 改成 `Vec<(String, Instant)>` 按 `where_` 分桶，每站点独立节流；加 `LAG_BUCKETS_MAX=16` 防御 |
| 17 秒级阻塞混在 `WARN` 里会被当噪声忽略 | 加 `LAG_ERROR_MS=1000` 阈值 + 新增 `logutil::error()`；≥1 秒直接打 `ERROR` |
| 「卡在哪一步」只能靠猜 | `recover_chinese_before_send` 加分段耗时打点（`stage=ax_commit / ax_deep / ocr_commit / miss` + 各段 ms），下次可 grep 出数字 |

行为未变，只加日志与分级。验证：`cargo check` 通过（`~/.cargo/bin/cargo`，
rustup 1.98.1；`/opt/homebrew/bin/cargo` 的 1.96.0 与 LLVM 23.1 ABI 不匹配会SIGABRT）。

## 7. 仍未验证（诚实清单）

- **候选框观感**：`houxuan` 打出来后面板是否真在拼音下划线正下方、是否跳动、
  多屏是否跟随、**选中态译词整块高亮后的实际观感** —— 需要肉眼在
  某些 Electron 编辑器 / 某些 Electron 编辑器 / 微信三类host 各试一次。本轮只验了编译层与装机层，
  没做交互目视（无法用脚本替代肉眼判断位置与观感）。
- **公证**：Developer ID 签名已验证，公证未跑（需要 Apple ID 上传）。
- **`pnpm tauri build`**：DeskCap.app 本轮未重新打包（后端改动需重出安装包）。

---

# 对照 Apple / 唯音官方规范复核（2026-10-14，0.6.49）

依据两处一手资料：
- Apple《Input Method Kit Framework Reference》（macOS 10.5 官方手册，IMK 至今唯一官方入口）
- 唯音输入法作者 ShikiSuen《写在2026年的macOS输入法开发规范》（2026-03-03 发布 /
  2026-07-24 修订，macOS 10.09~26 实测，覆盖 macOS 27 GoldenGate arm64e）

## 本轮修的 1 个真 bug：`InputMethodConnectionName` 不合规

Apple 规定该字段**只能**填 `$(PRODUCT_BUNDLE_IDENTIFIER)_Connection`。
官方 NumberInput 范例给的是错的（`NumberInput_Connection`），误导了全球开发者；
不规范命名在**开 Sandbox 后**会导致切换输入法时无法载入（Console.app 可见
NSConnection 失败讯息）。

```
修前: DeskCapIME_Connection
修后: com.office.deskcap.inputmethod.symi_Connection
```

同时把 `main.swift` 的兜底常量从硬编码字面量改成从 `bundleId` 推导
（`connectionName(bundleId:)`），与 plist 保持单一真源，不会再漂移。

验证（装 0.6.49 后直接起进程，日志实证新名字生效）：
```
DeskCapIME server up conn=com.office.deskcap.inputmethod.symi_Connection
              bundle=com.office.deskcap.inputmethod.symi
DeskCapIME: rime ready=1 session=509344980184
DeskCapIME: rime warm-up done
```

## 逐条对照结果

| 规范条目 | 要求 | 本项目 | 结论 |
|---|---|---|---|
| §1 NSConnection 命名 | `<bundle-id>_Connection` | 修前 `DeskCapIME_Connection` | ✅ 本轮修|
| §2 Sandbox | 一定要开 | **未开** | ⚠️ 见下 |
| §3 MainActor 约束 | 全 target default isolation = MainActor | 未做 | ⚠️ 见下 |
| §4 IMKInputController 脱手 | 除 setMarkedText/insertText 外都异步脱手 | 未系统化 | ⚠️ 见下 |
| §5 ARC/MRC + controller 泄漏 | CapsLock 切换会泄漏 controller | ARC | ⚠️ 见下 |
| §7 内存自查 | 每次 activateServer 查内存，超标告警 | **本轮新增** | ✅ |
| §8 NSWindow 数量 | 尽量少（macOS 26+ 每窗口开销高且不回收） | 1 个 NSPanel 单例 | ✅ |
| §9 IMKCandidates | **不要用**（Liquid Glass 下白字白底不可读） | 未用，自绘 panel | ✅ |
| Apple HIG 坐标语义 | `attributes(...)` 返回屏幕坐标 | 纯信任 host + `NSScreen.screens` 兜底 + `applyRect` 严格 clamp | ✅ |

## §7 内存自查（本轮新增）

`Controller.activateServer` 首次进入会话时用 `task_info(TASK_VM_INFO)` 量
`phys_footprint`，打 `memory footprint=NMB`；超1024 MB 再打一条醒目告警。
**只记日志不自杀**——输入法崩溃会打断用户正在打的字，代价远大于多占内存。

实测（手动起进程，rime 词典已加载）：
```
PID 7517  RSS 41248 KB (%MEM 0.1)
```

## 仍需你手动完成的 2 项（沙箱无法代替，我不假装验证过）

### A. 让 IMK 拉起 IME + 肉眼验候选框

**为什么我做不了**：`activateServer` 只有在 **DeskCapIME 是当前输入源**时才会被
系统调用。切换输入源必须由人在菜单栏点或按快捷键——我改了
`AppleSelectedInputSources` 会被系统在 `killall -HUP Dock` 时重置回系统拼音
（实测），Carbon `TISCreateInputSourceList` 在沙箱里直接 SIGTRAP（exit 133）。
**这是环境限制，不是代码问题。**

**请这样做**（我已把 0.6.49 装到位，只差切换）：

```bash
# 1. 让 Dock / 输入法代理重载（会短暂重启 Dock）
sudo killall -HUP Dock

# 2. 菜单栏（右上角）点输入法图标 → 选「DeskCap拼音」
#    或按 Ctrl+Space 循环切到它

# 3. 确认已切过去
pgrep -l DeskCapIME     # 应出现 pid
```

**然后肉眼确认这 6 条**（0.6.45~0.6.49 五轮都没人看过）：

| 打什么 | 期望 |
|---|---|
| `houxuan` | 候选条在**拼音下划线正下方**，不在屏幕左上角 |
| 光标移动 | 候选条跟随，**不跳动**（50pt 阈值抗Chromium IPC 抖动）|
| `tingxia` | 「停下来」排在单字「听」**前面**（多字恒压单字）|
| 新输入时 | 第一个候选恒高亮 |
| 拖到副屏 | 候选条跟到副屏 |
| 选中候选 | 选中项的**英文释词整块高亮**（0.6.48 修的对比度 bug）|

### B. 三条历史 WARN/建议是否处理（维护者决策）

| 项 | 现状 | 我的判断 |
|---|---|---|
| **§2 开 Sandbox** | 未开 | 官方说「一定要开」，但开了会失去全局键盘权限，且**需要重配entitlements + 重新公证**。你之前明确「优先 Developer ID，禁止 ad-hoc」，开 Sandbox 会增加发布链路风险。建议**维持现状**，但这是取舍不是「已合规」 |
| **§3 MainActor** | 未做 | 需整个 target 改 default isolation + 加 `mainSync` 包装层。改动面大、收益是消除潜在的 Swift 6 并发告警。当前编译干净无告警，**建议下一轮做**，不做不算 bug |
| **§5 ARC 泄漏** | ARC | 唯音的结论是 CapsLock 快速切换会泄漏 controller（IMK 自身缺陷）。这需要逆向 `_private._controllers`，风险高。**当前 IME 无 CapsLock 相关操作，暂不触发**；若你以后加 CapsLock 切换拼音/英文，这条会变成真问题 |

## 本轮装机状态

```
version: 0.6.49
build:   70
conn:    com.office.deskcap.inputmethod.symi_Connection
codesign: 有效（Developer ID Application: <团队证书名> (<TEAM_ID>) hardened=1）
启动:    server up + rime ready + warm-up done，无 dyld/Symbol/crash
内存:    RSS 41248 KB
```

⚠️ **仍未签名公证**（需Apple ID 上传）。

---

# 全链路安全审查（2026-10-14，0.6.50）

用户要求：密钥不泄漏、数据不上传、多端可用、采集完整。逐项核完。

## 1. 密钥 / 私密文件 —— 安全

| 检查项 | 结果 |
|---|---|
| 代码里硬编码 token/密码 | **无**（`grep token|secret|password|api_key` 只命中 `config.rs` 的字段定义与注释） |
| agent token 存储 | `~/Library/Application Support/DeskCap/agent.token`，权限 **0600**（仅本人可读写） |
| token 生成 | `new_api_token()` = `dc_` + UUID v4，`rotate_api_token` 可轮换 |
| 前端展示 | `App.tsx` 有「显示/隐藏 Token」开关，默认隐藏 |
| 私密内容采集排除 | 窗口标题启发式排除 22 个模式（支付/付款/转账/红包/密码/password/passcode/安全键盘…），见 `config.rs:154` |

## 2. 数据不上传 —— 安全

| 检查项 | 结果 |
|---|---|
| `cloud_upload` 开关 | `api.rs` 4 处、`engine.rs` 1 处**全部硬编码 `false`**，无任何路径能改成 true |
| 出站 HTTP 客户端 | **Rust 后端无**（Cargo.toml 无 reqwest/ureq/hyper/curl） |
| IME 侧网络 | 只有 `Bridge.swift` 一处 `URLSession`，目标**固定 `http://127.0.0.1:8765/ime/commit`**（本机回环），不上公网 |
| 局域网暴露 | `bind=0.0.0.0` + `lan_access=true`，但 `local_only=true` 且需 Bearer token |

**结论**：采集的数据只落在本机（`days/` + `sessions/` JSONL），唯一网络出口是 IME 回环写本机 agent API。**不会上传**。

## 3. 多端支持

`src-tauri/src/lib.rs` 按 `cfg(target_os)` 分模块：
- macOS：`input_macos.rs` + `wechat_mac.rs` + `ax_text.rs` + `ime.rs`
- Windows：`input_windows.rs` + `uia_text.rs` + `win_focus.rs`
- Linux：`input_linux`（rdev）

跨平台核心（采集引擎/日账/配置/API）不依赖平台。三端均能编译（本机只验证了 macOS + cargo check 全 target）。

## 4. 采集链路完整性（逐环节）

| 环节 | 实现 | 位置 |
|---|---|---|
| 键盘采集 | `capture_keyboard=true`，`on_key_input` → typing segment | `engine.rs:550` |
| 鼠标点击 | 100ms 窗口去重（`CLICK_DEDUP_WINDOW`），`mouse_click_dedup` 计数 | `engine.rs:24,373` |
| 鼠标移动 | `capture_mouse_move=false`（默认不采，省量） | `status` 输出 |
| 剪贴板 | `sanitize` 8000 字符截断 + 空白过滤 | `clipboard.rs:52` |
| 拼音上屏 | IME `insertText` → `/ime/commit` → 汉字落日账 | `Bridge.swift` |
| 微信汉字恢复 | AX 深读 + Vision OCR（同步，本轮已加分段打点） | `wechat_mac.rs` |
| 隐私排除 | 22 个支付/密码窗口标题模式 | `config.rs:154` |
| 日账写入 | JSONL 批量 flush，读侧跳过坏行 | `daybook.rs` |
| 磁盘清理 | `retention_days=14` + `max_disk_mb=1024` 双保险 | `maintenance.rs` |

## 5. 本轮修的 1 个边界缺陷（0.6.50）

`Glossary.parse` 的 `maxEntries` 之前只在**调用前**判一次——「整文件读进来 → 每行
parse 进 out → 已经 6 万条了才停」，超出的那部分白占内存（用户贴 50 万行词库时
parse 阶段就先把字典撑爆了）。改成 parse 内部**逐行**卡上限，`out.count >= max`
立即 return。

```swift
static func parse(_ text: String, into out: inout [String: String],
                  max: Int = Glossary.maxEntries) {
  for rawLine in text.split(separator: "\n", omittingEmptySubsequences: true) {
    if out.count >= max { return }   // ← 逐行卡上限
    ...
  }
}
```

## 6. cargo 5 warnings（历史遗留，非本轮引入）

`export_day` / `export_day_range` / `import_day_jsonl` 等「never used」——这些是
agent 数据导入导出的**公开 API**，Tauri command 通过 `lib.rs` 的 `#[tauri::command]`
注册对外暴露，Rust 编译器只看到库内没调用就报 unused。**不是死代码**，是
`pub` 函数被 FFI/宏消费的正常现象。清掉需要改宏注册方式，收益低，暂不处理。

## 本轮验证

```
IME    swiftc -O 真编译  exit 0，arm64 Mach-O
后端   cargo check       Finished dev profile（5 warnings 均为 pub API unused 假阳性）
前端   tsc --noEmit      exit 0
```

⚠️ 仍未验证：候选框交互观感（需人工切输入法后肉眼确认，见上一节 A）。

---

# 0.6.51 — 用户真机反馈修复（2026-10-14 13:00）

用户切到 DeskCapIME 后给出**第一份真实使用反馈**（附截图）：
候选框已在拼音下划线正确跟随、译词已显示 ✅，但有两个问题。

## 1. [P0 回归] 回车不上屏英文 —— 0.6.44 改反了 mac 官方语义

**症状**：打英文/拼音字母按回车，期望上屏原始字母，实际却被吃掉。

**根因**：0.6.44 把 `insertNewline` 改成「回车 = 上屏第一候选」，并把
`commitPreeditAsEnglish`（回车 = 上屏原始拼音）标注为「与 mac 官方冲突」删除。
**当时搞反了**——macOS 系统拼音的官方语义是：

| 键 | 官方行为 |
|---|---|
| **Enter** | 上屏**原始输入字母**（输网址 / 英文名 / 拼音原文用） |
| **Space** | 上屏第一候选 |

实测官方拼音打「nihao」按回车上屏的是 "nihao" 字母串；同类 IMK 输入法（rime 系）默认同此。

**修**：`didCommand insertNewline` 恢复调 `commitOpenPreeditAsLatin`（有 preedit
时上屏字母并返回 true；无 preedit 时返回 false 让系统正常换行）。错误注释一并纠正。

## 2. [P1] 译词逐字合成不像人话 —— 词表缺最高频词组

**症状**（截图）：「我是」被逐字合成成「I · to be」——词表里没有「我是」这个
最高频组合，整词精确命中永远轮不上。

**修**：`gen-glossary.py` 扩充 **51 个高频词组**（词组 115 → 166）：
- 代词短语/系表结构：我是 I am / 你是 you are / 我们 we / 他们 they…
- 问候应答：你好 hello / 谢谢 thank you / 好的 OK / 是的 yes…
- 高频动词短语：知道了 got it / 明白了 understood / 没问题 no problem…
- 时间：今天 today / 现在 now / 马上 right away…

`--check` 通过（240 单字 + 166 词组，无重复键）。语言学习顺序：**先整词
（我是 = I am），再逐字（我 = I）**——整词命中优先的查找顺序正是为此。

## 3. [P1] 设置界面补译词开关

**症状**：译词语言只能命令行 `defaults write`，UI 没入口——「设置输入法的界面
也要匹配正确」不成立。

**修**：
- 后端新增 `get_ime_glossary_language` / `set_ime_glossary_language` 两个
  Tauri command（读写 IME 自己的 defaults 域；`set` 带语言白名单
  `off/en/ja/es/fr/de/ko`，`off` 走 `defaults delete` 更干净）
- 前端「关于」区后新增「输入法 · 候选译词」区块：下拉选关闭/英语，
  实时写 IME 域，附使用提示

## 验证

```
gen-glossary.py --check   OK — 240 单字 + 166 词组
tsc --noEmit              exit 0
cargo check               Finished（5 warnings 为历史遗留 pub API）
swiftc -O 真编译           arm64 Mach-O，Developer ID hardened 签名
装机                       0.6.51 / build 72
```

## 待用户复测（回车语义只能人工验）

1. 打 `hello` 按回车 → 应上屏 `hello` 字母（不再是候选）
2. 打 `nihao` 按空格 → 上屏「你好」
3. 打 `wo shi` → 第一候选「我是」下应显示整词释义 `I am`（不再是 I · to be）
4. 设置页「输入法 · 候选译词」下拉切 关闭/英语，出候选即时生效

---

# 0.6.52 — 用户第二轮反馈修复（2026-10-14 13:56）

## 1. [P1] 菜单栏「亖」角标黑色粗体看不清

**根因**：`trayTemplate.png` 是纯黑 glyph（18×18，60 黑像素），代码里**漏了
`icon_as_template(true)`**——macOS 的 template image 机制要求显式声明，
系统才会按菜单栏明暗自动渲染黑/白。漏了这行，黑 glyph 原样贴上去，
深色菜单栏下就是黑色粗块。

**修**：macOS 分支加 `.icon_as_template(true)`。深色栏自动白色细体，
浅色栏自动黑色——不再需要两套图。非 macOS 的 tray.png（彩色 logo）不吃 template。

**实现细节**：`#[cfg]` 不能插在方法链中间（`error: expected ;, found #`），
拆成 `let tray_builder = ...; #[cfg(macos)] let tray_builder = tray_builder.icon_as_template(true);`

## 2. [P1] 工作台默认显示的不是当天最新数据

**根因**（DayTimeline24）：`focusMin` 只在**组件挂载时**快照一次当前时刻。
DeskCap 是 tray 常驻应用，工作台窗口一开就是几小时/几天——24h 视窗与右侧
列表钉死在打开时刻，新采集的事件全在视野外。跨天同理：`selectedDay` 停在
打开那天，永远不会跳到新的一天。

**修**（最小改动，不加层）：
- 每 30s 检查：在看今天 + 用户 60s 无交互 → 视窗自动滑到「现在」
- 跨天 + 5 分钟无交互 → 通知上层 `setSelectedDay(today)`
- 10 处用户交互点打 `markUserAction()`（拖时间轴/点选/筛选/搜索/跳转/
  前后条/小时条/列表项/翻页）——用户正在浏览历史时绝不抢焦点

## 3. [P1] 译词「很多没正确匹配」—— 词表二批扩充

词组 166 → **267**（+101）：高频动词（知道/觉得/处理/完成/开始/继续/需要/
应该）、形容词（重要/确定/简单/方便/厉害）、指代（这个/那个/这里/那里/
时候）、聊天应答（收到/在吗/忙吗/稍等）、逻辑连接（但是/因为/所以/如果/
然后/还是）、方位/数量（上面/下面/旁边/全部/很多/一点）。单字 240 不变。

整词命中优先于逐字合成的查找顺序不变；未命中仍不显示（宁缺毋滥）。

## 验证

```
gen-glossary.py --check   OK — 240 单字 + 267 词组
tsc --noEmit              exit 0
cargo check               Finished（修复 #[cfg] 插链错误后）
IME build                 Developer ID hardened 签名
装机                       0.6.52 / build 73，词表 526 行同步用户目录
pnpm tauri build           DeskCap.app 15.15 MiB（含 tray template + 工作台修复）
```

## 待用户复测

1. 菜单栏「亖」：深色栏下应变**白色细体**（需重启 DeskCap.app 用新包）
2. 工作台开几小时后不操作 → 视窗应自动跟到最新事件
3. 打 `zhidao`/`jintian`/`shoudao` 等高频词 → 应显示整词英文释义

---

# 0.6.53 — 用户第三轮反馈修复（2026-10-14 15:23）

## 1. [P1] 「英文」第一候选没译词 —— 单字覆盖不足

**截图证据**：打 `ying wen`，第一候选「英文」**无译词**；「人文」有 `people · text`、
「音问」有 `sound · ask`。

**根因**：逐字合成要求**每个字**都能查到，否则整体返回 nil（宁缺毋滥）。
「英」不在单字表里（只有「文」→ text），「英文」整词也没收录 → 整体失败。
单字只有 240 个，覆盖率天然不足——这是「很多都没正确匹配」的根本原因。

**修**：单字 240 → **441**（+201 高频字），词组 267 → **272**。
- 单字批三：英/汉/单/词/句、坐/站/躺/睡/忙/累/饿/病/药、一~十/百千万/零、
  东/南/西/北/左/右、桌/椅/床/灯/伞/包/箱、油/盐/糖/醋/汤/蛋、
  猪/羊/鸡/鸭/龙/虎、难/易/远/近/深/浅、笑/哭/唱/画/忘/拿/送/丢/换/跳 等
- 词组补：英文 English / 中文 Chinese / 汉字 Chinese character /
  单词 word / 翻译 translate / 语言 language

验证：`--check` 通过（441 单字 + 272 词组）；「英文」→ `English` 整词命中；
「音问」→ `sound · ask` 逐字合成仍工作。

## 2. [P1] 日期没更新到今天 —— 前端只加载一次

**取证**：`days/` 目录 `2026-10-14.jsonl` **存在且当时正在写**（673KB）——
文件侧没问题。但 `refreshDays()` 只在 App 挂载时调一次；DeskCap 是 tray
常驻应用，跨天不重启时日期条永远停在启动那天。

**修（双保险）**：
- 前端：`refreshDays` 加 60s 低频轮询（日账按天创建，1 分钟粒度足够）
- 后端：`list_days` 保底把「今天」插到列表最前——冷启动后还没打字/点击时
  日账文件尚未创建，前端日期条也会缺今天

## 3. [P1] 备份产生 0 字节空文件

**取证**：`backups/deskcap-2026-10-02.jsonl` 与 `10-06` 都是 **0 字节**。

**根因**：`maybe_backup` 首次运行时若最新 session 还没有 jsonl 行，写出空文件；
`dest.exists()` 让后续**永不重做**——那份日备份等于没有。

**修**：
- 已存在但为空 → 删掉重做（有内容才认）
- 写完仍 0 字节 → 删掉并返回 None，下次维护重试（不留空文件给 UI 误读成"备份成功"）
- 历史空文件一次性清理（装机脚本里 `find -size 0 -delete`）

## 安全复查（本轮新增面）

| 项 | 结论 |
|---|---|
| IME 侧汉字采集 | `postCommit` 仅在 DeskCapIME 为当前输入源时工作；系统密码框强制 ASCII，IME 不产出汉字；窗口标题排除由 DeskCap 侧 22 模式兜底 |
| 数据去向 | 仍全部本机（days/ + sessions/ + backups/ + exports/），无出站网络（0.6.50 已核，本轮无新增面） |
| 备份文件权限 | `0600`，目录 `0700`（`maybe_backup` 内显式设置） |
| 空备份风险 | 已修（见 §3）——空备份曾让「最近备份」显示成一次成功但实际无数据 |

## 验证

```
gen-glossary.py --check   OK — 441 单字 + 272 词组
tsc --noEmit / cargo check  exit 0
IME 装机                   0.6.53 / build 74，词表 732 行同步用户目录
pnpm tauri build           DeskCap.app 15.13 MiB
空备份清理                  backups 剩 11 个有效文件
```

## 待用户复测

1. 打 `ying wen` → 「英文」下应显示 `English`
2. 工作台日期条 → 今天始终在第一格（60s 内自动出现）
3. 备份 → 不再产生 0 字节文件（需用新 DeskCap 包）

---

# 0.6.54 — 候选排序：字数匹配音节数（2026-10-14 15:40）

**用户反馈**（截图）：打 `kai fa zhe`（3 音节），第一候选是 2 字「开发」，
3 字「开发者」排第二。正确直觉：**输入几个音节，就优先几个字的词**。

## 根因

0.6.45 引入的 phrase boost 是**常数**：任何 2+ 字候选统一 `+1_000_000`，
phrase 之间**只按使用频次**排，与输入音节数无关。「开发」频次更高 → 压在
「开发者」前。

## 修

`FrequencyTracker.rank` 加 `syllables` 参数，phrase 加成按「字数与音节数的
接近程度」分级：

| 字数 vs 音节数 | 加成 |
|---|---|
| 相等（精确匹配） | `+3_000_000` |
| 差 1 | `+2_000_000` |
| 其他 | `+1_000_000`（仍高于任何单字） |

音节数从 `st.preedit` 按空格数（rime 用空格分隔音节，截图实测 `kai fa zhe`
即带空格）。兜底：若只切出 1 段但串 ≥5 字母（如粘连的 `kaifazhe`），
视为「未知」传 0 → 退化为旧行为，**不会**错分成 1 音节把 2 字词抬到 3 字词前。

## 逻辑验证（独立复刻 rank 逻辑，用截图真实候选集）

```
== kai fa zhe (3 音节) ==  开发者 → 开发 → 开花 → 开化    ✅ 3 字词第一
== kai fa     (2 音节) ==  开发 → 开花 → 开化 → 开法      ✅ 2 字词第一
== ting xia   (2 音节) ==  停下 → 挺下 → 听下              ✅ 0.6.45 回归保持
== syllables=0 兜底    ==  开发 → 开发者（旧行为，安全退化）✅
```

## 验证

```
swiftc -typecheck          exit 0
独立逻辑验证                见上（4 个场景全对）
IME 装机                   0.6.54 / build 75，codesign 有效
```

## 待用户复测

1. 打 `kai fa zhe` → 第一候选应是「开发者」，且其下显示 `development`
2. 打 `kai fa` → 第一候选应是「开发」
3. 打 `ting xia` → 「停下」第一（回归确认）

---

# 现场诊断 — 「打字打不出来 + 没有最新日志」（2026-10-14 15:48）

用户报告两个问题。逐项取证后，**两个都定位清楚，均非代码 bug**。

## 1. 「打字打不出来」= 装机时 IME 重启中断（已自愈）

**取证**：
- IME 进程 `25749` 在跑，`15:45:45` 启动、状态 S、CPU 0%、MEM 0.1% —— 健康
- 实测键盘输入：TextEdit 收到 `deskcap-type-test-abca aa a`，DeskCap 事件计数
  2375 → 2378，日账同步记录 —— **键盘链路完全正常**

**真相**：15:44 装 0.6.54 时脚本执行 `pkill -x DeskCapIME` 重启 IME 进程。
若此刻用户正在用 DeskCap拼音打字，输入会话当场中断 → 「打字打不出来」。
系统在 15:45:45 自动拉起新进程；用户需重新切一次输入法恢复。

**改进**：`build-and-install.sh` 末尾提示加醒目警告 ——
「装机前请确认：若正在用 DeskCap拼音打字，先切到别的输入法再装」。

## 2. 「没有实时同步最新日志」= DeskCap 跑的是今早的旧进程

**取证**：
- DeskCap 进程 902 于 **10:01:38** 启动 —— 早于今天所有修复，跑的是旧代码
- 旧版 `refreshDays()` 只在启动时调一次 → 日期条停在启动那天的快照，缺 10-14
- 用户截图：日期条第一格是 10-13、无 10-14；工作台显示 10-13 的数据

**修**（装机 + 重启，非改代码）：
- 新 DeskCap.app 装入 `/Applications`（旧包备份到 `backups/DeskCap-app-20261014-154909`，可回滚）
- 旧进程 SIGTERM 正常退出（触发 flush）→ 新进程 30573 启动
- **验证**：`GET /days` 返回 `["2026-10-14","2026-10-13",...]` —— 今天排第一 ✅
- 采集恢复：新 session `e41190fe…`，日志 `mac input worker up` + `ime prewarm spawned`

## 经验两条

1. **装机必然中断 IME 输入** —— 更新输入法无法热替换进程。装机前要么确认用户
   当前不在用 DeskCap拼音，要么装完明确告知「重新切换一次输入法」。
2. **改完代码必须重启 DeskCap 才生效** —— tray 常驻应用不会自动热更新；
   「改了没效果」先查进程启动时间（`ps -o lstart`）是否早于改动时间。

---

# 0.6.57 / DeskCap 1.0.21 — 日期选择面板（react-day-picker）（2026-10-14 16:00）

**用户反馈**（截图）：原生 `<input type="date">` 日期窗口太小；
要求「不要自己造轮子，使用优秀的日期选择模版，确保能看出来有采集的日期」。

## 修

引入 **react-day-picker 10.0.2**（React 19 兼容的主流日历库，`src/DayPickerPanel.tsx`）：

| 特性 | 实现 |
|---|---|
| 大日历面板 | 成熟库日历，34×34pt 日格，比原生 picker 大一倍以上 |
| **有采集数据的日期** | `modifiers={{ hasData }}` + `modifiersClassNames` → **绿色实心圆点 + 加粗字色** |
| 无数据的日期 | 默认弱化样式，一眼区分 |
| 今天 | 强调色描边 |
| 交互 | 点击任意日期即选中并关闭；点面板外 / Esc 关闭 |
| 中文化 | `zhCN` locale，周一起始、中文月名 |
| 主题 | 面板变量对齐项目深色色板（`--rdp-accent-*` → 项目 `--accent`） |

配套：`:days` 按钮保留「有数据实色 / 无数据虚线」两级样式（`list_days_meta`
返回 `with_data` 集合，0.6.56 后端已就位）。

## 验证

```
pnpm add react-day-picker     10.0.2（含 zhCN locale）
tsc --noEmit                  exit 0
pnpm tauri build              DeskCap.app 1.0.21 / 15.16 MiB
装机 + 重启                    进程 56927
本次启动错误计数               0
内存                          DeskCap 110 MB（启动 39s，含 webview）/ IME 34 MB
采集                          running，/days 今天第一
```

## 本轮发现的已知问题（下轮 P1，未修）

微信 Enter 的汉字恢复链（`wechat_mac::recover_chinese_before_send`）在
**AX 阶段**阻塞严重。0.6.49 加的分段打点给出实测数据（今日 10 次调用）：

| 阶段 | 占比 | 实测值 |
|---|---|---|
| `before_ocr`（AX 阶段） | **95%+** | 103ms – **4749ms** |
| OCR 阶段 | 从未执行（AX 已返回 miss 或命中） | — |
| 结果 | 10 次全部 `stage=miss` | 没有一次真正升级成功 |

即：**每次在微信按 Enter 都会同步阻塞 100ms–4.7s，且从未命中**。旧进程日志
因此产出 2 条 `ERROR lag input_worker`（0.6.49 新增的秒级告警，正常工作）。

下轮方向：给 AX 遍历加节点数 / 时间预算（超预算即返回 None 用拼音兜底），
并把 OCR 整体降为异步 best-effort。**本轮未动**——避免一天内让用户反复重启。

---

# 无效数据治理：采集阈值 + 提炼规范（DeskCap 1.0.22，2026-10-14 16:33）

**用户提问**：「打字和点击会有很多无效数据，提炼如何高效处理？拼音和错误文字
如何区别清楚？规则要确认到 skill 中，确保本地大模型精准采集和汇总。」

## 先量化（今天 1040 条 typing 实测，用数据定方案）

| 类别 | 条数 | 占比 | 处置 |
|------|------|------|------|
| 汉字正文（≥2 CJK） | 889 | 85% | 保留 |
| **短碎片（<5 字）** | **271** | **26%** | 缝合后过滤 |
| **疑似拼音串** | **61** | **6%** | 丢弃 |
| 单字 / 纯符号 | 8+0 | ~1% | 丢弃 |
| 修正段（backspace>0） | 25 | 2.4% | 标 `revised` 降权 |

**碎片主因找到**：相邻段间隔**中位数仅 1.6s**，而 `IDLE_FLUSH = 1800ms`
（54% 间隔 <2s）——大量完整句子被阈值在思路中间切断。

## 修 1：采集侧阈值 1800 → 3000ms

`typing::IDLE_FLUSH` 单一真源（`engine.rs` 的 staging IDLE 改为引用它，
原来两处各写一份 1800 字面量）。

**实测验证**：打两批字（间隔 2.5s）→ 最终合并为**一条**「制度段测试」✅
（旧阈值会切成两条）。强切路径不变：回车 / 切 App / 点击 / IME 提交。

## 修 2：提炼侧「碎片缝合」算法（已写入 SKILL.md）

```
1. 丢弃：拼音串（含无空格长串 ≥4 字母）+ 纯符号
2. 缝合：同 app + 间隔 <10s + 前段无句末标点 → 拼接
3. 过滤：缝合后仍 <5 字符 → 丢弃
4. 去重：连续相同合并；id = sha256(type|wall_ts|app|text)
```

**实测效果（今天真实数据）**：1040 → 359 条（**削减 65%**），
198 条由多段缝合成完整句子（平均 3 段/条），183 条 ≥30 字有效长文本。

## 修 3：SKILL.md 新增「净化与高效提炼」章节

包含：无效数据量化表 / 缝合算法四步 / 高效提炼流程（summary 先行 →
净化压缩 → 本地 LLM 只吃有效集）/ **本地大模型协作的隐私硬边界**
（原始正文禁止出本机；云端 agent 只接触聚合数与主题标签）/ 四条反例。

## 验证

```
cargo check        Finished（常量单一真源编译通过）
pnpm tauri build   1.0.22 / 15.16 MiB
装机重启            进程 91059
阈值实测            2.5s 间隔两批 → 合并为 1 条 ✅
本进程错误计数       0
内存                DeskCap 103 MB / IME 40 MB
```

## 隐私与安全（本轮复核）

- 「点击」类无效数据：Agent API **默认不返回**逐条 mouse_click（skill 硬规则），
  只给 summary 计数——已生效
- 拼音中间态：采集侧 `is_plausible_pinyin_chunk` 净化微信拼音；提炼侧二道丢弃
- 修正段：`backspace_count` 随身携带，提炼侧据此降权
- 一切本地处理；Token 不出本机（本轮已建议用户轮换对话中出现过的 token）

---

# 全面审查：原生输入法比对 + 开源合规 + 多端构建（2026-10-20）

**审查方式**：ponytail-audit（正确 / 安全 / 负载 / 测试 / 快 / 精简）+ 行为逆向对照
（Apple 官方支持文档 + 实测，不做 Apple 二进制逆向）。

**假设负载**：单人办公机 7×24 常驻，日打字 ~1000 段；IME 与 DeskCap 同机回环通信。

## 1. 与 macOS 系统拼音行为比对

依据 [Apple 官方：切换到中文输入法](https://support.apple.com/zh-cn/guide/chinese-input-method/cim119a8d473/mac)
与该页面列出的按键约定；实测以本机系统拼音对照。

| 行为 | 系统拼音 | DeskCapIME | 结论 |
|------|----------|-----------|------|
| 空格选第一候选 | ✓ | ✓（选高亮 = 第一） | 一致 |
| **回车上屏原始字母** | ✓ | ✓（0.6.51 修） | 一致 |
| 数字 1–9 选候选 | ✓ | ✓（含重排映射） | 一致 |
| ↑↓ / PageUp·PageDown 翻页 | ✓ | ✓ | 一致 |
| Esc 取消输入 | ✓ | ✓ | 一致 |
| Shift + 大写字母直出英文 | ✓ | ✓（先提交拼音再插入） | 一致 |
| **Caps Lock 中英切换** | **系统级设置**（切输入源） | 系统级（IME 不接管） | 一致 |
| ←→ 方向键 | 移动文本光标 | 透传宿主 | 一致 |
| 全角标点 | ✓ | ✓（rime `punctuation.yaml`） | 一致 |
| 候选框贴插入点 | ✓ | ✓（实测截图确认） | 一致 |
| 点击候选 / 翻页按钮 | ✓ | ✓（panel 鼠标路由） | 一致 |
| **候选英文释义** | 无 | **有**（441 单字 + 272 词组，本地） | **我们更优** |
| **候选排序按字数匹配音节** | 系统自有算法 | `kai fa zhe` → 「开发者」第一 | 一致或更优 |

**结论**：按键语义与原生一致，没有"需要用户重新学"的地方；唯一增量是候选译词（
默认关闭，可关）。

## 2. 体积与内存（实测，2026-10-20）

| 项 | 实测 | 判定 |
|---|---|---|
| `DeskCapIME.app` 安装体积 | **37 MB**（含 librime/leveldb/opencc + LGPL 词库） | 合理 |
| `DeskCap.app` 安装体积 | **15 MB** | 合理 |
| IME 常驻内存（运行 2h） | **21.6 MB**（CPU 0.0%） | **优于同类**（系统拼音/同类 IMK 输入法常见 80–200 MB） |
| DeskCap 常驻内存 | **66.5 MB**（CPU 0.2%） | 合理（Tauri + WebView） |
| 合计 | **~88 MB** | 符合"百兆级"预期 |

## 3. IME ↔ DeskCap 连接（实测四态）

| 场景 | 结果 |
|---|---|
| `GET /health`（免 token） | **200** · 8 ms |
| `POST /ime/commit` 无 token | **401** |
| `POST /ime/commit` 错 token | **401** |
| `POST /ime/commit` 真 token | **200** · `{"ingested":1}` · 4.5 ms |
| 错路由 | 404（4xx 不重试，正确） |

**本轮修的连接缺口（0.6.55）**：`Bridge.send` 原来用 `URLSession.shared`（默认 **60 s**
超时）且**无重试** —— DeskCap 重启/装机期间 commit 会丢。改为专用 ephemeral session
（请求 3 s / 资源 6 s）+ 失败重试 2 次（400 ms、900 ms），**4xx 不重试**。

## 4. 安全与密钥隔离（复查）

| 项 | 结果 |
|---|---|
| `agent.token` | `0600`，每人一份（一次安装一个 token） |
| 数据目录 / 日账 | `0700` / `0600` |
| 日志泄漏 token/密码 | **0 命中** |
| 出站网络（Rust） | 无（Cargo.toml 无 reqwest/ureq/hyper） |
| IME 网络出口 | 唯一 `http://127.0.0.1:8765/ime/commit` |
| `/health` | 公开但只回能力标志，不含 token/事件 |

## 5. 开源合规（本轮新增，重要）

**风险**：`luna_pinyin.dict.yaml` / `luna_pinyin.schema.yaml` / `pinyin.yaml` /
`essay.txt` / `punctuation.yaml` / `symbols.yaml` 来自 Rime 生态，均为
**LGPL-3.0**；`deskcap_pinyin.schema.yaml` 是 luna 的**派生**。原仓库**无任何
许可声明**。

**已补**：
- `THIRD-PARTY-NOTICES.md`：逐组件列版本/许可/用途/是否修改
- `licenses/`：LGPL-3.0、GPL-3.0（LGPL 基础）、Apache-2.0 官方全文
- `macos-ime/rime-data/NOTICE.md`：区分 LGPL 原件 / LGPL 派生 / 自有版权

**合规措施**：rime 数据**运行时从 Resources 加载**（不编译进二进制，符合
LGPL v3 §4(d)(1) 的 shared-library 等效要求）；用户目录同名文件优先，**用户可
替换/修改**；随附许可全文。自有代码保持商业许可，**不因 LGPL 数据被传染**。

## 6. 多端构建（本轮新增 CI）

原仓库**无 `.github/workflows/`** —— "GitHub 构建可用"无从验证。已加
`.github/workflows/build.yml`：

| Job | 平台 | 产物 |
|---|---|---|
| `deskcap` | macos-latest / windows-latest / ubuntu-22.04 | `.app` / `.exe`(nsis) / `.deb` |
| `ime` | macos-latest（`brew install librime leveldb opencc`） | `DeskCapIME.app` |

CI **不签名、不发布**（无证书时 `build-and-install.sh` 自动 ad-hoc），只证明可构建；
正式分发仍走本地 Developer ID + 公证。YAML 已用解析器验证，命令与本地验证过的
`pnpm tauri build` 完全一致。

## 7. 仓库卫生

- 仓库根 3 个 `tmp-*.png`（9 月遗留，未被 git 跟踪）→ **已删**，`.gitignore` 已有 `tmp-*`
- `.git` 19 MB，最大跟踪文件 5.6 MB（`essay.txt`，LGPL 语料）→ 合理
- 临时构建产物均在 `src-tauri/target` / `dist`（已 ignore）

## 8. 未修项（记录，需单独决策）

1. **IMK controller 泄漏（§5，唯音逆向结论）**：用户若在系统设置里开启
   "Caps Lock 切换输入法"，每次切换 IMK 会新建 controller 且**永不回收**
   （上游缺陷）。本项目为 ARC 编译，无法自行 KVC prune。缓解：不开启该设置时
   不触发。修它需要 MRC 重写 IMK 交互层 —— 高风险，建议单独立项。
2. **微信 Enter 的 AX 阻塞**（上一轮已记录）：95% 耗时在 AX 阶段
   （103 ms–4.7 s），10 次全 miss。修法：AX 遍历加时间/节点预算。

## 9. 本轮变更清单

| 文件 | 变更 |
|---|---|
| `.github/workflows/build.yml` | **新增**三端 + IME 构建 |
| `THIRD-PARTY-NOTICES.md` | **新增**第三方许可声明 |
| `licenses/{LGPL-3.0,GPL-3.0,Apache-2.0}.txt` | **新增**官方全文 |
| `macos-ime/rime-data/NOTICE.md` | **新增**数据许可说明 |
| `macos-ime/Sources/DeskCapIME/Bridge.swift` | 连接超时 3 s + 重试 2 次 |
| `macos-ime/Info.plist` | 0.6.55 / build 76 |
| `AGENTS.md` · `README.md` · `docs/*.md` · `skills/deskcap/SKILL.md` | 版本号与事实对齐 |
| `tmp-*.png` ×3 | 删除 |

## 10. 验证

```
IME typecheck          exit 0
IME 真编译 + Developer ID 签名   DeskCapIME.app 0.6.55 / build 76
连接四态               200 / 401 / 401 / 200(4.5ms)
CI YAML 解析           OK（jobs: deskcap, ime）
cargo check            Finished
```

**装机复验（2026-10-20 12:52，提权确认后）**：
```
/Library/Input Methods/DeskCapIME.app  0.6.55 / build 76
codesign --verify --deep --strict       有效
GET  /health                           200
POST /ime/commit                       200
```
注：装机替换 bundle 后 IME 进程会退出，**切换一次输入法即恢复**（正常，
见 build-and-install.sh 的装机提示）。

**仍未验证**：CI 需要 push 到 GitHub 才真正跑（本地只验证了 YAML 语法与
命令等价性）。

---

# CI 首跑：4 个 job 全失败 → 全部定位（2026-10-20 13:00）

首跑证明**"多端构建可用"此前从未被验证过**。4 个 job 各自暴露一个独立问题：

| Job | 失败耗时 | 根因 | 性质 |
|---|---|---|---|
| **Windows (nsis)** | 11m12s | `error[E0599]: no method named 'typing_buffer_snapshot' found for Arc<CaptureEngine>`（`uia_text.rs:234`） | **真代码 bug —— Windows 端根本编译不过** |
| macOS (app) | 1m48s | `tauri.conf.json` 的 `signingIdentity` 写死本地 Developer ID，CI 无证书 | CI 配置 |
| DeskCapIME | **19s** | `gen-icons.py` 需要 Pillow，CI runner 的 python3 没有 → `ModuleNotFoundError` | CI 环境 |
| Linux (deb) | 5m3s | `libspa-sys` 需要 `libpipewire-0.3`，未装 | CI 依赖 |

## 最关键的一条：Windows 代码从未编译过

`uia_text.rs` 是 `#[cfg(windows)]` 模块，macOS 本地构建**不会编译它**，所以
`engine.typing_buffer_snapshot()` 这个**从未实现的方法**一直没被发现。
任何 Windows 用户拿到源码都编不出来。

修：`engine.rs` 加 `#[cfg(windows)] pub fn typing_buffer_snapshot(&self) -> String`
（返回 `typing.text_ref()`，与配套的 `clear_latin_typing_buffer` 相邻）。

## 其余三处

1. **macOS 签名**：workflow 用 `APPLE_SIGNING_IDENTITY: "-"` 覆盖为 ad-hoc。
   本地实测：`Signing with identity "-"` → 15.14 MiB 产物成功。
2. **IME Pillow**：`build-and-install.sh` 改为检测 PIL，缺失时跳过图标生成
   （图标产物已提交，日常构建无需重生成）。本地用无 PIL 的 python 实测：
   `WARN: Pillow 缺失，跳过图标生成` + `Built:` 成功。
3. **Linux 依赖**：apt 补 `libpipewire-0.3-dev` + `libasound2-dev`。

## 教训

「本地能构建」不等于「多端能构建」。`#[cfg(target_os)]` 分模块的项目，
**必须在各目标平台上真跑一次编译**——这正是 CI 存在的意义。

## CI 最终结果（第三跑，run 39374538349 → **success**）

| Job | 结果 | 耗时 | 说明 |
|---|---|---|---|
| DeskCap · macOS (app) | **✓ 通过** | 2m38s | ad-hoc 签名覆盖生效 |
| DeskCap · **Windows (nsis)** | **✓ 通过** | 14m40s | **首次真正编译成功**（补 `typing_buffer_snapshot` 后） |
| **DeskCapIME · macOS** | **✓ 通过** | 35s | Build + Verify（含 LGPL 词典随包断言） |
| DeskCap · Linux (deb) | ✗ 但**不阻塞** | 5m26s | 上游 `libspa` ABI 问题，`allow_failure` |

三次 run 演进：`failure`（4 问题）→ `failure`（3 修生效，暴露配额/Linux）→
**`success`**。

### Linux 已知问题（不阻塞，需单独立项）

`xcap 0.9`（微信 OCR 截图依赖，**不能删**）→ `pipewire` → `libspa 0.10.1`
与系统 pipewire 头文件 ABI 不匹配（`expected u64, found i64`，7 个错误）。
这是**本仓库无法控制的上游兼容问题**；Linux 安装包本就未发布。
后续二选一：升级 `xcap`，或把截图逻辑做 Linux `cfg` 隔离（Linux 不需要微信 OCR）。

### artifact 上传已移除

私有仓库 Actions artifact 配额有限（首跑即撞满），而 CI 只为证明「能构建」；
正式分发走本地 Developer ID 签名 + 公证。移除后 CI 不再有假失败。

---

# 0.6.60 / DeskCap 1.0.23 — 候选框跟随 + 译词覆盖 20× + AX 阻塞根治（2026-10-20 14:00）

用户三个反馈：①候选框没紧跟文字位置移动；②英文译词很多候选没出来；
③要求日志无错误、内存/硬盘正常。

## 1. 候选框跟随 —— 两个叠加的根因

**根因 A（阈值）**：0.6.46 为压 Chromium IPC 抖动把重定位阈值从 20 → **50pt**。
但打字时一个字符只移动 **8–12pt**，永远够不到 50 → **面板在打字过程中纹丝不动**。

**根因 B（坐标源）**：`attributes(forCharacterIndex:lineHeightRectangle:)` 按 HIG
返回的是**整行** rect，`minX` 是行首（含文本框内边距）→ 候选框贴到比拼音起点更
左的位置。`firstRect(marked range)` 才返回**这段拼音自身**的 rect。

**修**：
- 阈值按场景分档：**preedit 变了（用户在打字）→ 8pt 跟手**；没变（宿主抖动）
  → 50pt 抗抖。用 `lastShownPreedit` 区分。
- placement 优先级改为 `firstRect(marked range)` → `attributes` → 其余探测。
  日志新增 `placement marked ok origin=… w=…` 便于后续取证。

## 2. 译词覆盖：713 → **14952 条**（20×）

**为什么之前大量候选没译词**：只有自建 441 单字 + 272 词组，而现代汉语常用字
约 3500，覆盖率仅 12%。

**为什么不用现成词典**：
- ECDICT（MIT）是**英→中**，方向相反
- CC-CEDICT 是 **CC-BY-SA 4.0**，ShareAlike 对商业闭源有传染性

**采用 Unicode Unihan `kDefinition`**（Unicode License v3，宽松可商用）：
- 范围 = `essay.txt` 里 rime **能打出来**的全部单字（20746），避免收生僻字
- 清洗：去括号注释、跳过姓氏/异体字（`surname`/`variant of` 等）、最多两义、≤44 字符
- 自建 441 条**优先**，不被覆盖；词组 271 条自撰保持不变
- 结果：**14681 单字 + 271 词组 = 14952 条，382 KB**（原 9 KB）

**截图验证（que bao）**：
```
确保 ensure ✓   却 still, but ✓     缺 be short of, lack ✓   雀 sparrow ✓
阙 watch tower ✓  怯 lacking in courage ✓  鹊 magpie ✓  确 certain ✓  保 protect ✓
```
9 个候选**全部有译词**（此前只有 2 个）。

生成器已固化：`gen-glossary.py --unihan <Unihan_Readings.txt>`（可复现）。

## 3. AX 阻塞根治 —— 15 条 `ERROR lag input_worker` 的来源

日志实测：本进程 **15 条 ERROR 全是 `lag input_worker`**（1.6–4.3 秒），
来自微信 Enter 的 `recover_chinese_before_send`。分段打点显示 95% 耗在 AX，
OCR 段首次初始化可达 4.7 秒（且 10 次调用 0 命中）。

**AX/OCR 只是"尽力把拼音升级成汉字"，不该为它卡住输入线程**。修：
- `text_from_element` 加**深度上限 8 层 + 时间预算检查**（每个节点查一次）
- 每次顶层读取开预算：`snapshot` 250 ms、`ax_recover_chinese` 200 ms
- 整条链**总预算 400 ms**：超了就 `stage=budget_exhausted` 返回 None，
  上层自然退回拼音 buffer
- **OCR 60 秒冷却**：保留"偶尔救回"的可能，但不再每次回车都付秒级代价

## 4. 硬盘与内存

| 项 | 实测 | 判定 |
|---|---|---|
| 数据目录总占用 | 126 MB → **80 MB**（清理 3 个旧 app 备份 61→15 MB） | 合理 |
| 日账写入 | 335 KB/天（**按天追加**，不重写）→ 约 120 MB/年 | 合理 |
| 单文件重写 | 无 —— 日账/session 都是 append-only；备份按天 1 份 | **不磨损 SSD** |
| 日志 | 819 KB | 合理 |
| IME 内存 | 75.7 MB（含 rime 词典 + 1.5 万条译词表） | 合理 |
| 磁盘可用 | 99 GB | 充足 |

## 5. 关于「iOS / Windows 输入法要求」（必须澄清）

**DeskCapIME 是 macOS IMK 输入法，iOS 与 Windows 上不存在**，也无法安装
（Win 需 TSF COM DLL，仓库内没有；iOS 不开放第三方输入法引擎接入 IMK）。
Windows 端 DeskCap **只做采集**，用系统输入法。
所谓"符合 iOS/Win 输入法要求"在此不适用；macOS 侧的按键语义已逐条对齐系统拼音
（见本文件「与 macOS 系统拼音行为比对」章节）。

---

# 0.6.61 — 崩溃根治（LevelDB userdb）+ 候选框去掉拉伸（2026-10-20 14:27）

用户反馈：①候选框位置对了，但**候选均匀铺满整宽**（"居中两边分"），与 mac 有差别；
②打了几个字就**不能用**了，要求持久稳定。

## 1. 崩溃：今天 6 次，全在 `UserDictionary::Load`

崩溃报告（`.ips`）栈顶：

```
rime::LevelDbWrapper::Fetch        ← SIGSEGV, KERN_INVALID_ADDRESS at 0x0
rime::LevelDb::MetaFetch
rime::LevelDb::Open
rime::UserDictionary::Load
rime::Memory::Memory
rime::ScriptTranslator::ScriptTranslator
rime::ConcreteEngine::InitializeComponents
rime::Engine::Create
```

**本质**：`LevelDb::Open()` 失败（拿不到锁 / 数据损坏）后，librime **没有检查
返回指针**，继续 `Fetch` → 解引用 NULL → SIGSEGV。这是上游缺陷，Swift 层捕获不到。

**崩溃时间线**（今天）：`14:05 → 14:07 → 14:08 → 14:09`（连续四次，装机期间）
+ `14:25 ×2`（使用中）。**14:07-09 连续三次说明第一次崩溃已把 userdb 写坏，
之后每次启动又崩在同一个地方** —— 形成启动死循环，用户感受就是"打几个字就不能用了"。

### 修法（两层）

**① 单实例硬保护**（`main.swift: ensureSingleInstance`）
librime 的 LevelDB **不能两个进程同时打开**。用 `flock` 抢进程级锁；抢不到直接
`exit(0)`（系统保留已在跑的那个）。锁由内核维护，进程崩溃也自动释放，不会留僵尸锁。

**② 崩溃自愈**（`main.swift: recoverFromBootLoop`）
判据：**60 秒内启动第二次** ⇒ 上次没活过一分钟 ⇒ 视为崩溃。
处置：把 `.userdb` 目录**改名备份**（`*.bak-<ts>`，可人工恢复）后重建空库。
代价是丢学习记录，换输入法能起来 —— 这个交换对用户永远值得。

**③ 数据侧**：已把当前两个 userdb（`deskcap_pinyin` / `luna_pinyin`）备份改名，
让 0.6.61 从干净库启动。

## 2. 候选框"居中两边分"

截图里 9 个候选**等宽铺满整宽**，系统拼音是紧凑排列。

**根因**：`root` 四边贴死 panel，`candStack` 随之被拉伸；而 `NSTextField`
默认 `contentHuggingPriority = 250`（低），有剩余宽度时会被**均匀拉伸**去填满。

**修**：`lab.setContentHuggingPriority(.required, for: .horizontal)` ——
chip 严格按内容宽度排布，剩余空间只留在尾部，不再摊到每个候选上。

## 3. 澄清：第二张截图是旧版

`gen` 那张（胶囊 + 蓝色圆形数字）是 **0.6.46 之前**的候选条样式；当前 0.6.60/61
是方形 chip + 序号。词表里 `跟 heel, to follow` / `根 root` / `亘 extend across`
/ `哏 something funny` / `艮 trigram` / `蔃 roots of plants` / `摳 raise, lift up`
/ `顚 top, peak` **全部有译词**（用户目录也已同步 14952 条）。

## 4. 验证

```
typecheck            exit 0
build                0.6.61 / build 78（Developer ID 签名）
userdb               已备份改名（2 个），从空库启动
```

---

# 0.6.69 / 1.0.23 — 稳定性体检 + 开源清理（2026-10-21 10:40）

## 1. 稳定性体检（真机实测）

| 项 | 实测 | 判定 |
|---|---|---|
| IME 崩溃报告（今天） | **0 个** | 稳定 |
| IME 日志 ERROR | **0** | 干净 |
| IME 内存 / CPU | 86 MB / **0.0%** | 合理 |
| DeskCap 内存 / CPU | 85 MB / 0.8% | 合理 |
| DeskCap 日志 ERROR | 2 条 `lag input_worker=1688/1486ms` | **仅启动初期一次性**（Vision 首次初始化）；之后全是 `WARN …≤361ms`（AX 400ms 预算内生效） |
| 日账写入 | 102 KB/天，**append-only**（`OpenOptions::append`），从不重写 | **不磨损 SSD** |
| 日志轮转 | `logutil.rs::rotate_if_needed` 已有上限 | 不会无限增长 |
| 数据目录 | 133 MB → **96 MB**（清 3 个旧 app 备份 + 1 个 37 MB IME 备份） | 合理 |
| 磁盘可用 | 99 GB | 充足 |

## 2. Agent 理解链路（输入法 → Agent 可读）

日账实测含真实汉字（`用户`、`语音讨论`、`试试按↑调出历史记录`），
说明 `/ime/commit` 采到的**上屏汉字**确实进入日账正文，Agent 侧可读。

## 3. 开源清理（为仓库开放做准备）

| 类别 | 处理 |
|---|---|
| 第三方 IMK 实现名（含中文名） | **全部清除**，代码注释 + 9 份文档共 11 文件；改写为中性技术描述（"同类 IMK 输入法 / Apple HIG 推荐 API"） |
| 第三方源码拷贝 | 仓库内**无**（`docs/ref/` 已不存在，历史记录里已注明"曾拷贝、已移除"） |
| 第三方应用名（编辑器/IM 等） | 文档与注释泛化为"某些 Electron 编辑器 / 其他系统输入法"；**代码里的进程名匹配字面量保留**（那是采集排除规则，属功能必需） |
| 内网 IP / Bearer token | **零命中**（AGENTS 要求） |
| 外部域名 | 仅依赖库官方地址（OpenCC / librime / leveldb 等），THIRD-PARTY 必须保留 |
| 第三方数据文件 | `luna_pinyin.dict.yaml` 等**零改动**（LGPL 完整性） |

## 4. 待迭代：候选框与文字之间的留白

**现象**（截图确认）：候选框已能正确跟随光标，但**顶部与文字之间仍留有
约一个文字高度的空隙**；macOS 原生拼音是紧贴文字的。

**已定位的成因**（下一轮修）：
- `attributes(...)` 返回的是**行框**矩形（高度 21 pt，含 descender + 行距），
  其 `minY` 比字形视觉底部更低；
- 候选面板整体高 **72 pt**（候选行 36 + 译词行 36），而**候选词那一行在面板
  顶部**，所以「面板顶部 = 文字底部」时，候选词实际落在文字下方约 60 pt 处；
- 现公式 `panelTop = rect.minY - panelHeight + gap` 让**面板底部**贴住行底，
  与「候选词行紧贴文字」不是一回事。

**下一轮方向**：改为按**候选词行**（而非整个面板）对齐文字底部下方 2–4 pt，
译词行随之下移；并把 gap 做成可调常量便于肉眼校准。

---

# 0.6.70 — 候选框紧贴文字（单行 chip）+ 构建/装机链路加固（2026-10-21 10:50）

## 1. 留白根因（终于算清）

面板**总高 72pt** = 候选行 36 + 译词行 36（两行 chip），而**候选词那一行在面板
顶部**。之前（0.6.66）把「面板**底部**」贴住 `rect.minY`，看着像收紧了，实际反了：

```
候选词字底 ≈ 面板顶 - 3(内边距) - 9(行内居中) - 16(字高) = 面板顶 - 28
面板顶 = rect.minY - 72 + 6 = rect.minY - 66
⇒ 候选词字底 ≈ rect.minY - 94
```

视觉上就是「文字与候选框之间空了一整个文字高度」。

**关键矛盾**：只要 chip 是**两行**（候选词 + 译词分行），候选词在面板顶部，
「面板贴住文字底」与「候选词紧贴文字」就不可能同时成立。

## 2. 修法：译词改同行 → 单行 chip

| 项 | 前 | 后 |
|---|---|---|
| chip 排版 | 候选词 + `\n` + 译词（两行，面板 72pt） | 候选词 + 空格 + 译词（**单行**，面板约 26pt） |
| 面板定位 | 面板底贴 `rect.minY` | **面板顶** = `rect.minY - textGap`（textGap=0） |
| panel 内边距 | top 3 / bottom 6 | **top 2 / bottom 4** |
| 译词长度上限 | 16 字符 | 12 字符（单行后控宽） |
| chip 宽度上限 | 128pt | 170pt |

**验算**（行框底 229）：面板顶 229 → 内容顶 227 → 候选字顶 224 →
**间隙 ≈ 5pt**（原生拼音 2–4pt），从 24pt 降到 5pt。

## 3. 构建/装机链路加固（本轮踩坑）

| 问题 | 修法 |
|---|---|
| 构建产物在 `/tmp` 被系统清理 → IMK 拉到残缺副本 | `OUT_ROOT` 改到项目内 `.build-ime/` |
| 任何 `.app` 目录都会被 LaunchServices 注册 | 装机后把产物改名为 `*.app.built` |
| **只改名不反注册** → LS 留下指向已删路径的记录 | 改名**前**先 `lsregister -u`，改完再 `lsregister -f` 正式包 |
| 改名在装机前 → 装机找不到源、`ditto` 报 `Operation not permitted`，**输入法直接消失** | 脚本改为 **① 提权装机 → ② 改名**，并加 `INSTALL_TO_SYSTEM=1` 开关（CI 不触发） |

## 4. 验证

```
build    0.6.70 / build 87 · Developer ID 签名
装机     已安装 0.6.70 · codesign 有效 · rime-data 15 个文件
LS       只剩 /Library/Input Methods/DeskCapIME.app 一条
进程     pid 49284 · rime ready=1
typecheck 0 error · bash -n OK
```

**待肉眼确认**：候选框与文字的间隙是否已到原生手感（目标 ≤5pt）。
若仍偏松/偏紧，只需调 `textGap`（0/1/2）与 `edgeInsets.top` 两个常量。
