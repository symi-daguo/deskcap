# Ponytail audit — DeskCapIME Swift + DeskCap docs（2026-10-02, 0.6.47）

Method: ponytail-review ladder (stdlib / native / yagni / shrink / delete) + 一致性核对。
Host: **Windows**（本轮无 macOS SDK / 无 swiftc / 无 cargo / 无 node_modules）——
所以本轮**没有**做编译验证，只做了源码级核对 + 数据校验。见文末「本轮未验证」。

---

## 0. 版本元数据不一致（真 bug，已修）

**症状**：代码已经是 0.6.46，`Info.plist` 还在报 **0.6.44 / build 66**。
0.6.45、0.6.46 两轮的 `CFBundleShortVersionString` bump **从来没落到 plist 上**。

**证据**（代码自证，比文档可靠）：

| 事实 | 位置 |
|---|---|
| `RETRY_MOVE_THRESHOLD_PT = 50`（注释写明 `// 0.6.46`） | `CandidateWindow.swift:55` |
| `makeScreenRect` / `hostWindow(of:)` 信任 host 语义 | `CandidateWindow.swift:313,333` |
| phraseBoost 与音节数解耦（注释 `0.6.45 — multi-char phrases always…`） | `CandidateWindow.swift:248` |
| 审计日志里 0.6.45 / 0.6.46 两轮都已完成 | `PONYTAIL-AUDIT-2026-09-29.md:179,273` |
| plist 却还是 0.6.44 | `Info.plist:21` |

**为什么这类不一致会咬人**：排障时第一件事就是 `defaults read … CFBundleShortVersionString`
确认装的是哪一版。plist 少两版 → 你会拿 0.6.44 的观测去判断 0.6.46 的代码，
「我说修了但没看到效果」就是这么来的。

**修**：`Info.plist` → 0.6.47 / build 68（0.6.47 才是本轮真实代码状态）。
另把 9 处文档里的 0.6.29 / 0.6.44 / 0.6.46 统一到同一真源。

**留下的教训（写进流程）**：改 IME 代码和 bump `CFBundleShortVersionString`
必须同一次提交；`docs/mac-ops.md` 的验收命令里那行 `defaults read` 就是
为了抓这种漂移，别删。

---

## 1. `docs/ref/` 里是逐字拷贝的 GPL-3.0 源码（已删）

**发现**：推送前的密钥/内容核对里翻到 `docs/ref/`：

```
同类 IMK 输入法InputController.swift   24 591 B
同类 IMK 输入法Panel.swift             22 133 B
同类 IMK 输入法View.swift              33 002 B
```

`同类 IMK 输入法InputController.swift` 头部原样保留 `// 同类 IMK 输入法 / Created by Leo Liu on 5/7/24.`
—— 这是 同类开源实现 的**逐字拷贝**，
而 同类 IMK 输入法 是 **GPL-3.0**（其 README 明写「授權條款:GPL v3」，GitHub 侧标注 GPL-3.0 license）。
三个文件**都没有任何许可声明**。

**为什么必须删**：

1. 与本轮「只吸收设计、不搬 GPL 代码」的边界**直接冲突** —— 青简的 crate 我们刚决定
   搬不得，仓库里却已经躺着 79KB GPL 源码，边界就成了自欺。
2. 它们**不参与编译**（`build-and-install.sh` 的 swiftc 文件列表里没有），
   删掉对构建**零影响**；`IME-FIX-2026-09-27.md` 里那些设计结论和行号已经写进正文。
3. 推送是**公开且不易撤回**的 —— 进了 git 历史再删，历史里还在。

**顺带修掉一个坏链接**：`IME-FIX-2026-09-27.md:1163` 引用的是**别人机器上的绝对路径**
`<项目根>/docs/ref/同类 IMK 输入法Panel.swift`（NAS 挂载），
对任何其他人都是死链。已改成指向上游 GitHub 链接。

**留下的规则**：需要对照某个 GPL 项目时 —— 记**链接 + 文件名 + 行号**，
不要把源码拷进仓库。

---

## 0.6.47 新增：候选译词（gloss）

**来源边界**（本轮最重要的决定）：青简是 **GPL-3.0-or-later**，把它的 crate 搬进来
会让 DeskCap 整体转 GPL。所以**只吸收交互设计，不搬代码**：

| 吸收的（设计 / 交互） | 自研的（实现） |
|---|---|
| 候选旁显示一条释义 | `Glossary.swift` 全部实现 |
| 一次只显示一种学习语言 | `GlossaryLanguage` + `glossary.<lang>.tsv` |
| 未命中就不显示（不猜、不凑） | 两级查找：整词 → 逐字 ` · ` 合成，任一字缺则 nil |
| 词表可离线扩充 | bundle 种子 + 用户目录覆盖，重打包不覆盖用户文件 |

**刻意没做**（避免半吊子）：

- **不做**整句 / 神经网络重排 —— librime 的 `grammar`/`language` 够用，
  换 `qingjian-neural`（candle）要拖 torch 级别依赖进 IMK 进程，
  收益不抵进程体积和崩溃面。
- **不做**词频统计页 / 生词本 UI —— 那是设置窗口的事，IMK 壳现在没有，
  先把数据层（`FrequencyTracker`）留好，等有 UI 再接。
- **不做**英/日/西三语打包 —— 机制支持任意 `<lang>`，但我们没有合法的三语词表
  数据来源，与其塞质量堪忧的机翻，不如只发一份自撰英语种子 + 明确的替换入口。

---

## Wheel-reinvention check（新增部分）

| 需求 | 用什么 | 判定 |
|---|---|---|
| TSV 解析 | 手写 20 行 `parse` | ✅ 合理。自写 `String.split` + `firstIndex(of:"\t")` 够了，引入 CSV 库反而不值 |
| 词表分发 | 现有 `rime-data/` 打包路径 | ✅ 复用，零新增机制 |
| 读用户偏好 | `UserDefaults` | ✅ 系统 API，不自己造 |
| 后台预热 | `DispatchQueue.global` | ✅ 与 `warmUpAsync` 同一模式 |
| 双行候选 | `NSTextField.maximumNumberOfLines` | ✅ AppKit 原生，没自己画 |
| 词表数据 | **生成器脚本** | ✅ 见下 |

### shrink — 词表是数据，不是代码

第一版我手写了 `glossary.en.tsv`，结果**把 `\t` 写成了两个可见字符**，
解析器一个词都读不出来。改成 `scripts/gen-glossary.py` 后：

- 真实 TAB 由 `f"{k}\t{v}"` 保证，不可能写错
- `--check` 校验重复键 / 缺 TAB / 空值，**重复键直接报错**而不是静默覆盖
- 数据改动走生成器，`git diff` 能看出是真改了数据还是手滑

实测 `python3 macos-ime/scripts/gen-glossary.py --check` → 240 单字 + 115 词组通过；
产出 5116 字节 / 355 个真实 TAB / 0 个字面 `\t` / LF-only / UTF-8 合法。

### bug — 重新打包会清空用户词表

`build-and-install.sh` 原本是 `cp -R rime-data/* $USER_RIME/`。
译词表进了 `rime-data/` 之后，**用户自己扩充的 `glossary.en.tsv` 每次重新打包都会被覆盖** ——
这正好摧毁「离线扩充」这个卖点。改成：

- `glossary.*.tsv` 从批量拷贝里排除（`find … ! -name 'glossary.*.tsv'`）
- 缺失时才种子，已存在则打印 `keep user glossary` 并保留

### bug — 候选条可能被一条长释义顶出屏幕

`lineBreakMode` 从 `.byTruncatingTail` 改成 `.byWordWrapping` 才能显示第二行，
但那意味着一个 60 字符的英文释义能把 9 个 chip 一起撑出可视区。
两道限：`Glossary.bounded` 截到 22 字符 + 每个 label `widthAnchor <= 170`（`defaultHigh` 优先级，
不跟 stackview 的 hugging 打架）。`root.fittingSize` 驱动高度，2 行时面板自动变高。

---

## macOS 27 / IMK 合规（新增部分）

| 关注点 | 结论 |
|---|---|
| 多行 `NSTextField` | `usesSingleLineMode = false` 是多行前提，已显式设置；`isEditable = false` 的 label 支持 |
| 宽度约束时机 | `widthAnchor` 自约束，不跨 view，激活时无需共同祖先，不会有 activate 失败日志 |
| 点击命中区 | `panel.candRects` 用 `candLabels[i].convert(bounds, to:)` —— 2 行后 rect 变高，点击区反而更宽容，无需改 |
| 强调色胶囊 | 选中项背景色只作用在第一行 run，释义行无背景色 → 胶囊只包住候选词本身 |
| 对比度 | 选中态释义用 `white @ 0.85`（压在 `controlAccentColor` 上）；未选中用 `secondaryLabelColor` |
| 线程 | `Glossary` 用 `NSLock` 保护缓存；读盘在锁外，同语言并发 miss 会重复读一次但结果幂等 |
| 主线程 | `warmUp` 在 `main.swift` 的 `DispatchQueue.global` 里预热，`lookup` 只做字典查表 |

---

## 本轮**未**验证（诚实清单）

本机是 **Windows**，以下全部做不了，**必须在 macOS 上跑**：

- `swiftc` 编译 / typecheck（`Glossary.swift` 语法、`NSLayoutConstraint` 优先级）
- codesign + notarize + 装进 `/Library/Input Methods/`
- 候选框实际观感：2 行高度、宽度上限、placement clamp 在 2 行下是否仍正确
- 译词实际命中率与断行效果
- `cargo check` / `pnpm tsc --noEmit`

已验证（本机可做的）：词表生成器 `--check` 通过、产出文件 TAB/编码正确、
`Info.plist` XML 合法、版本号全库一致、Swift 改动逐处读过。

---

# 补审 — DeskCapIME 0.6.48（2026-10-14，macOS 27 真机编译验证）

在 **macOS 27 真机**上复核 0.6.47（上一轮在 Windows 上做，只做了源码级核对）。
上一轮列的「本轮未验证」清单本轮全部跑通：`swiftc -typecheck` + `swiftc -O` 真编译
产出 arm64 Mach-O。发现并修 3 处，其中 1 处是上一轮**误判为特性**的真 bug。

## 1. [P0 · 真 bug] 选中项的译词是「蓝底之外的白字」

上一轮表格里把这条写成了「胶囊只包住候选词本身」，当成刻意设计。**它是 bug**：

```swift
// 0.6.47 修前
s.append(NSAttributedString(string: " \(num) \(text) ", attributes: [
    .backgroundColor: NSColor.controlAccentColor,   // ← 只有这一段有背景
]))
s.append(NSAttributedString(string: "\n\(gloss)", attributes: [
    .foregroundColor: selected ? .white.withAlphaComponent(0.85) : .secondaryLabelColor,
    // ← 释义行没有 backgroundColor
]))
```

**具体后果**：候选 1 被选中时，第一行是蓝底白字，第二行译义是**纯白字落在 popover
的浅色毛玻璃背景上**——对比度约 1.3:1，低于 WCAG AA 正文要求的 4.5:1，实质读不出来。
macOS 27 系统拼音的选中译词是**整块高亮**（译词与候选词同处强调色胶囊内）。

**修**：给释义段单独补 `backgroundColor`，前后各留 1 空格让胶囊形状完整包住两行。

```swift
let glossAttrs: [NSAttributedString.Key: Any] = selected
  ? [.font: glossFont,
     .foregroundColor: NSColor.white.withAlphaComponent(0.92),
     .backgroundColor: NSColor.controlAccentColor]
  : [.font: glossFont, .foregroundColor: NSColor.secondaryLabelColor]
s.append(NSAttributedString(string: "\n \(gloss) ", attributes: glossAttrs))
```

## 2. [P1] 面板宽度上限 960pt 会截断第 3 条以后的候选

**具体后果**：`min(960, fit.width)` 是 0.6.46 单行时代留下的。0.6.47 每条候选多一行，
但 chip 的 `widthAnchor <= 170` 与 `byWordWrapping` 让 `fittingSize` 依然按 9 条全展开算
（9 × (12 序号 + 最多 170 正文 + 12 间距) ≈ 1750pt）。取 `min(960, ...)` 会把面板
**截断**在 960pt，第 3 条往后的候选被切出可视区——而 `candRects` 仍按未截断的
布局缓存，点击命中区与可见区**错位**。

**修**：上限 960 → 1600（足够放下 9 × 170 的全展开），超出部分靠 chip 自身折行，
不再裁内容。窄屏由 `applyRect` 的水平 clamp 兜底。

## 3. [P1] `Glossary` 每键读 `UserDefaults` + 每键 `fileExists`

**具体后果**：`lookup` 是**每键调 9 次**的热路径（每次出候选 × 9 条 chip）。
`language` 每次都 `UserDefaults.standard.string(forKey:)`，`userURL(lang:)` 每次都
`FileManager.default.fileExists(atPath:)`。用户连续打字时，这等于把每个拼音字母
放大成 9 次同步 plist 读 + 9 次文件系统 stat。输入法进程跑在用户输入的关键路径上，
这类同步 I/O 是 macOS 上输入延迟的常见来源。

**修**：加 `cachedLang` 字段缓存 language（只在 `invalidate()` 失效），
`warmUp()` 提前把 language 也填上。

## 4. [P2] 词表无规模上限 → 用户塞大词库会撑内存

**具体后果**：`deskcap_freq.json` 侧已用 LRU 卡在 10 000 条，词表却没有任何上限。
用户往 `glossary.<lang>.tsv` 贴一份 50 万条的词库，`entries(for:)` 会一行不落地
读进 `[String: String]`。50 万条约 30–50 MB 常驻——对输入法这种长驻进程是真实风险。

**修**：`maxEntries = 50_000`（约 3–5 MB）。超出时跳过整个文件并打一行日志说明，
而不是读一半丢一半（避免用户以为词表生效了）。

## 验证（本轮 macOS 27 真机）

```
$ swiftc -parse-as-library -typecheck -sdk $(xcrun --sdk macosx --show-sdk-path) \
      -I /opt/homebrew/include -import-objc-header RimeBridge.h *.swift
typecheck exit: 0

$ swiftc -O -parse-as-library -framework Cocoa -framework InputMethodKit \
      -I/opt/homebrew/include -L/opt/homebrew/lib -lrime -import-objc-header \
      RimeBridge.h -o .../DeskCapIME *.swift
build exit: 0
Mach-O 64-bit executable arm64
$ strings ... | grep 'glossary lang=%@'
DeskCapIME glossary lang=%@          # 新增的规模上限日志确实编进二进制
```

## 仍然未验证（诚实清单）

- **装机观感**：2 行 chip 的实际高度、1600pt 上限在多显示器 / 缩放非 100% 下的表现、
  选中态译义对比度改善后的实际观感 —— 需装到 `/Library/Input Methods/` 后肉眼验收
- **`cargo check` 仍被工具链阻塞**：本机 Homebrew LLVM 23.1.0 与 Rust 1.96.0 ABI 不匹配
  （`__ZN4llvm17OptimizationLevel2O0E` 符号缺失），与本轮改动无关
- **`docs/mac-verify-0.6.47.md` 的清单项**未逐条跑（本轮只验编译层）
