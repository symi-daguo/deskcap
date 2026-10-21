# Mac 运维与合规（DeskCap 1.0.0 + DeskCapIME 1.0.0）

目标 OS：**macOS 13+**；本机 **macOS 27** 已验证。

> 📋 **装机 / 发版前必读（历史验证记录）：** `docs/archive/mac-verify-0.6.47.md` —— 0.6.45 起的三轮改动
> 都**未在真机验证过**，那份是逐步验收清单（含版本确认、候选框回归、译词、词表不被覆盖）。

## 2026-09-24 审查（ponytail）

| 项 | 结论 |
|----|------|
| 造轮子 | 候选定位 = **IMK firstRect + setFrameTopLeftPoint**（官方/同类 IMK 输入法） |
| 技术栈 | Tauri **2.12.0** / tauri-build **2.7.0** / React **19.3** / Vite **8.3** / TS **7**；axum/tokio 最新 |
| 内存 | IME ≈ **28–46 MB**，DeskCap ≈ **72–120 MB** |
| 候选跟随 | **marked 末尾插入点** firstRect 优先，拼音变长即后移 |

**验收：** `bash scripts/rime-type-test.sh`；打 `woxuanze` 候选应跟拼音末尾下方。

## 当前版本

| 组件 | 版本 | 状态 |
|------|------|------|
| DeskCap.app | **1.0.0** | 菜单栏「**亖**」；全屏列表按高度填满再翻页 |
| DeskCapIME | **1.0.0** | **1.0.0**：候选框贴字跟随 + 1.5 万条译词 + AX 预算 |（3s 请求超时 / 6s 资源超时）+ 失败重试 2 次（4xx 不重试）——修 DeskCap 重启期间 commit 丢失。**0.6.47**：新增**候选译词**（默认关闭，`GlossaryLanguage` 控制；`Glossary.swift` + `rime-data/glossary.<lang>.tsv`，`gen-glossary.py` 生成；用户目录词表不被重新打包覆盖）。**0.6.46**：删 0.6.45 的窗口高度半阈值 heuristic flip（fragile，且 `hostWindow(of:)` 返 nil 时仍把 top-left 当 bottom-left 用 → 候选框永在屏左上），改为 同类 IMK 输入法 式**纯信任 host `attributes(...)`** + `NSScreen.screens.contains` 兜底 + `applyRect` 严格 clamp；30ms 重试阈值 20pt → **50pt**（抗 Chromium IPC 同步抖动 5–15pt 亚像素抖动，20pt 每次都触发 re-place 导致「乱跑」）。**0.6.45**：`makeScreenRect`/`hostWindow(of:)` 引入；phraseBoost 与 preedit 音节数**解耦** —— 只要候选 ≥2 字恒得 `+1_000_000`，单字 `counts*100`，多字永远压单字。**0.6.44**：revert 0.6.43 的 `isWindowRelative` 假阳性。**0.6.41**：fresh input 永远高亮 `displayed[0]`。**0.6.40/0.6.39**：确定性 placement（无鼠标启发式）+ fuzzy/简拼/纠错走上游 `pinyin.yaml` + FrequencyTracker debounce+LRU+flush + schema `_schema_version` 自动 reseed |

## 候选条（0.6.47）

- 拼音落框 → 候选在**当前组词末尾正下方**（对齐系统拼音）
- `firstRect`：优先最后一字；候选列表 = `attributes(...)` + 三路 `firstRect(...)`，统一过 `makeScreenRect(_:from:)` —— 拿 host `NSWindow` 走 `convertPoint(toScreen:)`，**信任 host 为 bottom-left window 坐标**（HIG；Electron 系 host 返 top-left 是 host 的 bug，同类 IMK 输入法 同样救不了，靠 `applyRect` clamp 保证不飞出屏幕）
- 全拿不到 → `lastGoodRect`（同屏校验）→ 鼠标锚点；三者均无则不显示（宁可不显示也不乱飞）
- 排序：`FrequencyTracker.rank(_:)` —— 有任一多字候选时多字词组**恒定领先**（`+1_000_000`），单字 `counts*100` 仅在单字组内温和重排；同权重队内 rime 原序做 tiebreaker
- 采集：`ime_commit`；`ime-active` 防双记

> ⚠️ **0.6.45–0.6.54 已真机验证（0.6.55 = 连接韧性，仅 typecheck + 签名装机）**：仅 `swiftc` typecheck + arm64 真编译 + 未签名 `.app` build 通过，**未 codesign / 未装进 `/Library/Input Methods/`**。装机后需重跑下方验收。

**验收：** 切 DeskCap拼音，在 某些 Electron 编辑器 提交框打 `houxuan` → 候选应在下划线拼音正下方。

```bash
bash scripts/rime-type-test.sh
defaults read "/Library/Input Methods/DeskCapIME.app/Contents/Info" CFBundleShortVersionString  # 0.6.55
```

## 候选译词（0.6.47）

候选词**下方**显示一条目标语言释义，字号更小，是辅助信息，不会盖过要输入的文字。
**默认关闭**（不改任何配置时行为与 0.6.46 完全一致）。

```sh
# 开启英语译词（立即生效，无需重启 IME —— 候选刷新时实时读取）
defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string en
# 关闭
defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string off
```

**词表**（TSV，`词<TAB>释义`，`#` 为注释）：

| 位置 | 作用 |
|------|------|
| `Contents/Resources/rime-data/glossary.<lang>.tsv` | 随包种子，随 bundle 分发 |
| `~/Library/Application Support/DeskCap/rime/glossary.<lang>.tsv` | **用户词表，优先生效** |

`build-and-install.sh` **不会覆盖**用户目录里的 `glossary.*.tsv`（只缺失时种子），
所以自己扩充词表后重新打包也不会丢。

**改词表**：

```bash
# 改内置种子 → 改 macos-ime/scripts/gen-glossary.py 的 CHARS / WORDS，然后
python3 macos-ime/scripts/gen-glossary.py            # 重新生成
python3 macos-ime/scripts/gen-glossary.py --check    # 只校验（重复键 / 缺 TAB / 空值）
# 或直接手改用户目录那份 —— 生效最直接
```

**换语言**：放一份 `glossary.<lang>.tsv`（如 `glossary.ja.tsv`）并把
`GlossaryLanguage` 设成 `ja`。支持任意语言段，不需要改代码。

**查找规则**：整词精确命中 → 逐字 ` · ` 合成（`停下来` → `stop · come · down`），
任一字查不到就不显示 —— 宁缺毋滥。释义截断到 22 字符，单候选宽度上限 170pt。

**数据来源与许可**：内置种子是 DeskCap **自撰**的常用词释义（**441 单字 + 272 词组**，0.6.53/0.6.54 两轮扩充），
**不含任何第三方词库数据**，与 GPL 词库（如雾凇拼音、青简的 `qingjian-translate`）无关。

## 已知缺口

个别 App `firstRect` 仍飘时靠 `lastGoodRect` + 鼠标锚点回退（**不再回退窗框** —— 那正是 0.6.43 把面板推到屏幕顶部的假阳性分支）。添加列表偶发需 `force-enable-ime.sh`。

## 界面

中控台布局对齐数字人工作台：顶栏（标题 + 状态点 + **跟随系统** / 刷新 / 开始采集）+ 横向页签（工作台 / 设置 / Agent）。色板与 `console.py` 一致。主题按钮在**顶栏右侧**，点一下在「跟随系统 → 浅色 → 深色」间循环。

工作台实时流、日账事件列表按容器高度计算每页条数（全屏拉高就多显示，不再固定 8/12 条后底部留空再翻页）。切回工作台会重新测量。

## 合规

local_only；见 `SECURITY.md`。

## 2026-09-29 技术栈核对（npm registry + cargo 实测，非记忆）

| 包 | 本仓 | registry 最新 | 结论 |
|----|------|--------------|------|
| @tauri-apps/api | 2.12.0 | 2.12.0 | ✅ 最新 |
| @tauri-apps/cli | 2.12.0 | 2.12.0 | ✅ 最新 |
| @tauri-apps/plugin-autostart | 2.6.0 | 2.6.0 | ✅ 本轮升（was 2.5.1） |
| react / react-dom | 19.3.0 | 19.3.0 | ✅ 最新 |
| vite | 8.3.1 | 8.3.1 | ✅ 最新 |
| typescript | 7.0.2 | 7.0.2 | ✅ 最新 |
| @vitejs/plugin-react | 6.1.1 | 6.1.1 | ✅ 本轮升（was 4.6.0） |
| date-fns / vis-timeline / vis-data | 4.4.0 / 8.5.4 / 8.0.5 | 同 | ✅ 最新 |
| tauri (crate) | 2.12.0 | 3.0.0-**alpha.3** | ✅ **不动** —— 3 仍是 alpha，不追 |
| tauri-build | 2.7.0 | 2.x | ✅ 最新 |
| tauri-plugin-single-instance | 2.4.4 | 2.x | ✅ 最新 |
| librime (brew) | 1.17.0 | — | ✅ 最新 |
| rustc | 1.98.1 (2026-09-01) | — | ✅ |

**结论：全部为 2026-09 最新稳定版。** Tauri 3 处于 alpha，按「稳定优先」原则不采用。

---

## 输入法单实例与崩溃自愈（1.0.0）

### 为什么需要
`librime` 的用户词典是 LevelDB。**两个进程同时打开同一个 userdb 会段错误**
（`EXC_BAD_ACCESS`，栈顶 `LevelDb::Open` → `MetaFetch` → `Fetch`），
表现为输入法进程突然消失、切过去打不出字。

2026-10-21 12:54 就发生过一次：手动拉起一个实例、IMK 又拉起一个，
两进程抢锁 → SIGSEGV。

### 机制（`macos-ime/Sources/DeskCapIME/main.swift`）

| 阶段 | 行为 |
|---|---|
| 启动 | `flock(LOCK_EX \| LOCK_NB)` 抢 `~/Library/Application Support/DeskCap/.ime-instance.lock` |
| 抢到 | 正常加载 rime，锁持有到进程结束（不 close） |
| 抢不到 | **等 2 秒**（100ms × 20），给对方收尾时间 |
| 等不到 | **降级启动**：IMKServer 照常注册、英文数字照常透传，**不加载 rime** → 日志 `instance lock busy after 2s — starting WITHOUT rime (safe mode)` |

**绝不在抢不到锁时 `exit(0)`** —— IMK 不会重试已退出的 server，用户会彻底打不出字（0.6.66 的教训）。

### 崩溃自愈
`recoverFromBootLoop()`：同一分钟内启动第二次 ⇒ 上一次没活过一分钟 ⇒ 判定崩溃循环
⇒ 把 `luna_pinyin.userdb` / `deskcap_pinyin.userdb` **改名备份**（`.bak-<时间戳>`，
不删，可人工恢复）后重建空库。

代价是丢学习记录，换输入法能起来。崩溃循环的典型特征是三次启动都崩在同一处
（`UserDictionary::Load` → `LevelDb::Open`）。

### ⚠️ 不要手动 `open -a` 启动输入法

2026-10-21 实测：装机后为「确保起来」执行 `open -a "/Library/Input Methods/DeskCapIME.app"`，
结果**同时存在两个进程** —— 一个抢到锁（rime 正常），一个降级（`starting WITHOUT rime`）。
虽然防护生效没崩，但 IMK 若把按键路由给降级那个，用户就打不出中文。

**正确做法**：装完直接去「系统设置 → 键盘 → 输入法」切换，IMK 会自己拉起**一个**进程。
若已出现双进程，杀掉带 `safe mode` 日志的那个（`pgrep -x DeskCapIME` 后对比 PID）。

### 排查

```bash
# 单实例与降级记录
grep -E 'instance lock|safe mode' ~/Library/Application\ Support/DeskCap/logs/ime.log

# 是否有崩溃循环备份
ls -d ~/Library/Application\ Support/DeskCap/rime/*.userdb.bak-* 2>/dev/null

# 崩溃报告（IME）
ls -t ~/Library/Logs/DiagnosticReports/DeskCapIME-*.ips | head -3
```

### 正常水位（实测基线）

| 指标 | 正常值 |
|---|---|
| IME 内存 | 40–90 MB |
| IME CPU | **瞬时 0.0–0.5%**（`ps -o %cpu` 是生命周期平均，启动时摊入 rime 词典编译，会显示 10–20%，别被吓到） |
| DeskCap 内存 | 60–110 MB |
| DeskCap CPU | 0.2–1%（Vision OCR 首次初始化时会短暂升高） |
