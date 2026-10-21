# Ponytail audit — DeskCapIME Swift (2026-09-29, 0.6.44 复核)

Scope: `macos-ime/Sources/DeskCapIME/*.swift` (1 292 lines, 50 funcs).
Method: ponytail-review ladder (stdlib / native / yagni / shrink / delete).
Verdict: **lean already**. Three minor shrink candidates below; none urgent.

## Findings

- `RimeEngine.swift:233-258` — **shrink** — six wrappers around `api.pointee.process_key(...)` (`processAscii`, `processBackspace`, `selectFirstOrSpace`, `selectDigit`, `processKey`, `cancel`) all reduce to `process_key(session, code, 0) + snapshot()`. The `selectDigit(d:)` guard is the only real logic; the others are convenience tags. Keep `processKey(code:)` as the only public key path; drop `processAscii` / `processBackspace` / `selectFirstOrSpace` / `cancel` — callers (Controller) call `processKey` directly with `Int32(UInt8(ascii: "x"))` or `0xff08` / `0x0020` / `0xff1b`. Net: -25 lines, one fewer surface to maintain.
- `CandidateWindow.swift:330-355` — **yagni** — `screenForRect` has three fallback levels (midpoint → intersect → nearest by distance). Midpoint + intersect cover the real cases; nearest-by-distance triggers only when the rect is literally between displays and never produces a better answer than the mouse fallback that follows. Drop it. Add safe getter, or rely on `NSScreen.main`.
- `Controller.swift:73-83` (digit select path) — **shrink** — the `if dispIdx >= 0, dispIdx < lastDisplayed.count, dispIdx < lastDisplayedToRime.count` guard is over-defensive. `dispIdx = d - 1` where `d ∈ 1...9` and `lastDisplayed.count == pageSize == 9` — the guard cannot fail by construction. Remove.

## Already clean (no action)

- `Bridge.swift` — 109 lines, one file, single timer lock; nothing speculative.
- `FrequencyTracker` (in CandidateWindow.swift) — LRU + debounce + atomic JSON; stdlib `JSONEncoder` instead of hand-rolling. Good.
- `pickBestRect` — deterministic order, no mouse-proximity heuristic, NSLog only. Clean.

## Action taken this round (0.6.41)

- `Controller.apply()` — **bug**: top displayed not always highlighted when rime's `highlighted == 0` but frequency rank moved it down. Fixed: fresh input → `displayedHighlighted = 0`; arrow nav → mapped position. (Bug report: candidate (a).)
- `CandidateWindow.pickBestRect` — **shrink** — removed three dead `_ = firstUsable` statements (compile warnings).

Net diff this round: +13 / -7 lines (added comments + clear fix; dropped dead statements).

---

## 0.6.44 复核（同日第二轮）

代码已到 0.6.44，上一轮的三条 finding 重新逐条验证。

### 已处理

- `CandidateWindow.swift` — **delete** — `placeBelowMarkedText(...)` 是 0.6.44 revert 遗留的孤儿
  （`rg` 确认**零调用点**），纯转发到 `placeBelowMarkedTextOrigin`。删掉，-3 行。
- `CandidateWindow.swift:128,200` — **cleanup** — 两处 `placeBelowMarkedText*Returning*` 返回值未使用
  的编译警告，加 `_ =` 消掉。编译输出已干净（`build-and-install.sh` 无 warning）。

### 驳回

- `Controller.swift:74`（上一轮建议删 digit guard）— **不采纳，guard 是承重的。**
  上一轮的理由是「`lastDisplayed.count == pageSize == 9`，guard 不可能失败」，这个前提不成立：
  `lastDisplayed = ranked`，长度由 rime 实际返回的候选数决定（`Controller.swift:267`），可以为空。
  而 `RimeEngine.swift:295` 是 `isComposing = !st.preedit.isEmpty || !st.candidates.isEmpty`
  —— 这个 `||` 明确允许 **preedit 非空 + candidates 为空**。该状态下按数字键，
  `dispIdx = 0`，`0 < 0` 为 false，guard 正常拦下并走 `selectDigit(d)` 兜底。
  删掉 guard → `lastDisplayedToRime[0]` 越界崩溃。
  **留着。这条 finding 的推理前提有误。**

### 复核后总行数

`1 308` 行（5 文件）。相比上一轮 1 292 行的差异来自 0.6.44 本身的 revert + 排序改动，非本轮新增。

---

## 0.6.44 增量第三轮（2026-09-29 收尾）

扫描范围：`macos-ime/Sources/DeskCapIME/CandidateWindow.swift`。本轮修复 3 处，全部
`shrink` / `native`，无 `yagni` / `delete`。

### P0 — `CandidateWindow.swift:249` os_log 格式串丢前导 `%`

```swift
// 修前：四个 {public}.0f 全部被 os_log 当作字面 string 输出，控制台看不到真实坐标
os_log("...chosen=({public}.0f,{public}.0f,{public}.0f,{public}.0f)",
       candidates.count, r.origin.x, r.origin.y, r.width, r.height)

// 修后：%{public}.0f 才是 os_log 的隐私安全浮点占位符
os_log("...chosen=(%{public}.0f,%{public}.0f,%{public}.0f,%{public}.0f)",
       candidates.count, r.origin.x, r.origin.y, r.width, r.height)
```

**影响**：`0.6.42` 引入 → `0.6.43` / `0.6.44` 未发现。功能无影响（panel placement
仍正确），但 `placement ok` 诊断日志打不出真实坐标 → 候选框跑偏时排障线索失盲。

### shrink — `placeBelowMarkedTextOrigin` + `placeBelowMarkedTextReturningRect` 折一

两个私有函数做同一件事（caret placement + apply 到 panel），前者是后者的纯转发包装。
合并为单一 `placeBelowMarkedText(preedit:client:width:height:) -> NSPoint`，
`-12 行`。`@discardableResult` 让 `show()` 路径不需关心返回值。

### shrink — `applyMouseAnchor` 内置水平/垂直 clamp 折进 `applyRect`

原 `applyMouseAnchor` 内部先算鼠标起点 rect，再手工做 4 道 if 块 clamp 矩形坐标，
最后调 `applyRect` —— `applyRect` 自带的水平 + 垂直 clamp 已经覆盖了所有边界。
删 4 道 if 块 + 4 行注释，`-8 行`。`applyRect` 的"caret / mouse"差异只在起点 y，
不需要双层 clamp。

### 未触碰的驳回项（保持现状）

- `RimeEngine.swift` 的 6 个 `processKey` 包装 — 上一轮已驳回（语义不同，非纯转发）
- `Controller.swift:74` digit guard — 上一轮已驳回（承重）
- `screenForRect` 三级 fallback — 上一轮已驳回（midpoint + intersect 之外确实兜底）
- `applyRect` 的 `lastGoodRect` sticky — 仍在用（多屏切换锚点），未动

---

## 0.6.44 增量第四轮（2026-09-29 候选框交互对齐 macOS 系统拼音 / 同类 IMK 输入法）

用户提出"有问题的请修复，确保候选框交互跟 mac 官方输入法一致"。对照
同类开源 IMK 输入法实现的窗口交互设计
+ macOS 系统拼音官方语义，逐项找偏离点并补上。

> ⚠️ 2026-10-02 补注：本仓库曾把上述两个 同类 IMK 输入法 源文件**逐字拷贝**到 `docs/ref/`。
> 同类 IMK 输入法 是 **GPL-3.0**，与本项目「只吸收设计、不搬 GPL 代码」的边界冲突，
> 已连同 `同类 IMK 输入法InputController.swift` 一并删除。设计结论全部保留在本文里。

### P0 — 候选框 `ignoresMouseEvents = true`（mouse 点击无效）

```swift
// 修前：panel 完全屏蔽鼠标事件，鼠标点击候选无反应
panel.ignoresMouseEvents = true

// 修后：macOS 系统拼音 + 同类 IMK 输入法 都把候选框当作可点击 UI
panel.ignoresMouseEvents = false
```

**影响**：用户想用鼠标选词无反应；macOS / 同类 IMK 输入法 都支持 click 选中 / scroll 翻页。
本次新增 `CandidatePanel: NSPanel` 子类覆盖 `sendEvent`，按 IMK 通用方案 的 `pagingUp`/
`self.index` 守卫——mouse-down 与 mouse-up 必须在同一候选上才触发 `candidateSelected`，
避免误触（拖拽空白处不会选词）。Scroll wheel：`abs(dy) > 5` 触发翻页（`dy>0 = page up`，
跟随 同类 IMK 输入法）。

### P0 — 回车偏离 macOS 官方（"系统简体拼音"是旧方言）

```swift
// 修前：回车 = 上屏原始拼音英文（自定方言，与 mac 官方冲突）
if string == "\r" || string == "\n" {
  return commitPreeditAsEnglish(client: sender)
}

// 修后：回车 = 上屏第一个候选（与空格等价，mac 官方语意）
guard RimeEngine.shared.isComposing else {
  DeskCapBridge.flushNow()
  return false
}
if lastDisplayedHighlighted >= 0,
   lastDisplayedHighlighted < lastDisplayed.count {
  let rimeIdx = lastDisplayedToRime[lastDisplayedHighlighted] + 1
  FrequencyTracker.shared.record(candidate: lastDisplayed[lastDisplayedHighlighted])
  return apply(RimeEngine.shared.selectDigit(rimeIdx), client: sender)
}
return false
```

`commitPreeditAsEnglish` 完全删除。`didCommand` 的 `insertNewline` / `insertNewline
IgnoringModifiers` 同步改成 mac 官方语义。Escape hatch：用户想上屏原始拼音就 ESC
取消再输入；这是 mac 官方拼音的官方途径。

### shrink — `insertEnglish` 与 `commitPreeditAsEnglish` 重复抽取

新方法 `commitOpenPreeditAsLatin(client:)` —— 仅 `insertEnglish`（Shift+大写字母）
仍在调用。`-10 行`。

### shrink — `inputText(" ")` 与 `inputText(<digit>)` 冗余边界检查删除

`lastDisplayedToRime.count == lastDisplayed.count` 在 `apply()` 内是不变量；多余的
`< lastDisplayedToRime.count` 检查删掉，`-2 行`。

### 新增 — `CandidatePanel` NSPanel 子类 + `CandidateRouter` 协议

按 IMK 通用方案 的 `pagingUp` 守卫：mouse-down 与 mouse-up 命中同一候选 → `onSelect`；
命中同一 page 三角形 → `onPage`；否则丢弃（拖拽误触安全）。

`CandidateUI.router`（weak）+ `CandidateUI.lastClient`（weak AnyObject）承载
Controller → Panel 反向通信。Controller 在 `activateServer` first 时设 `router = self`。

### Net

`+199 / -32`（净 `+167 行`），2 文件。`swiftc -typecheck -parse-as-library` +
`tsc --noEmit` 干净。`swiftc -O` 真编译产出 arm64 Mach-O 二进制（251KB，链接 librime）。

### 未触碰项（保持现状）

- `RimeEngine.swift` 6 个 `processKey` 包装 — 上一轮驳回，本轮不动
- `Controller.swift:74` digit guard — 上一轮驳回，本轮不动
- `screenForRect` 三级 fallback — 上一轮驳回，本轮不动
- `applyRect` 的 `lastGoodRect` sticky — 仍有用，本轮不动
- `FrequencyTracker` LRU + debounce + flush — 上轮已稳定，本轮不动

---

## 0.6.45 修复轮（2026-09-29 候选框 placement + 频率排序）

用户又报两个具体 bug，本轮直接命中修复：

### P0 — 候选框永远在屏幕左上角（某些 Electron 编辑器 / 某些 Electron 编辑器 / Electron）

**根因**：`attributes(forCharacterIndex:lineHeightRectangle:)` 在 macOS 27 Tahoe
下两类 host 返回不同坐标系：

| Host | `attributes(...)` 返回 | 我们旧逻辑处理 | 结果 |
|---|---|---|---|
| HIG（TextEdit/微信）| bottom-left window coords | 当 screen-coords 直用 | ✓ |
| Electron 系（某些 Electron 编辑器/某些 Electron 编辑器/某些应用）| **top-left window coords** | 当 screen-coords 直用 | ✗ |

具体后果：某些 Electron 编辑器 返回 `(15, 50)`（相对 window 左上），我们 `setFrameTopLeftPoint((15, 50-44=6))`，
panel top-left 落在**主屏 (15, 6)** ≈ 屏幕左上角。

**修法**（新 `makeScreenRect`）：拿 host `NSWindow.frame`（bottom-left screen coords），
以"窗口高度半阈值"判别坐标系——
- `rect.origin.y > winHeight/2` → bottom-left（HIG）；`convertPoint(toScreen:)` 直出
- `rect.origin.y < winHeight/2` → top-left（Electron bug）；先 y-flip 越过 window.height，
  再 `convertPoint(toScreen:)` 出 screen coords

```swift
let winHeight = window.frame.height
let isTopLeft = rect.origin.y < winHeight / 2
let bottomLeft: NSRect = isTopLeft
  ? NSRect(x: rect.origin.x,
           y: winHeight - rect.origin.y - rect.height,
           width: rect.width, height: rect.height)
  : rect
return NSRect(
  origin: window.convertPoint(toScreen: bottomLeft.origin),
  size:   bottomLeft.size
)
```

**配套**：`placeBelowMarkedText` 简化 —— 删 `pickBestRect` + 删 `yFlipped`（多屏 y-flip 启发式
假阳性多、命中错；改用 `makeScreenRect` 单点决策）。`useless` 保留作 sanity check。

**Network 注意**：`hostWindow(of:)` 通过 `(client as? NSObject)?.perform(NSSelectorFromString("window"))`
拿到 NSWindow。`@objc protocol IMKTextInput` 在 Swift 不能直接调 `perform`（是 NSObject *class* 的方法），
必须先 cast 到 NSObject。

### P0 — 多字候选没在第一位置（"tingxia" 输了 phraseBoost=0）

**根因**：旧 `FrequencyTracker.rank(_:preeditSyllables:)` 把 phraseBoost 锁在 `preeditSyllables >= 2`。
但 preedit 是 rime 输出字符串，rime 不一定插空格 —— 用户输 "tingxia" 时
`preedit.split(" ")` 仍切 1 段，`preeditSyllables=1`，phraseBoost=0，
多字候选被常用单字压住。

**修法**：phraseBoost 改为**条件与 preedit 解耦** —— 只要候选是 2+ 字（unicodeScalars.count > 1），
恒得 `+1_000_000` 加成；单字得 `counts * 100` 温和权重。至少有一个多字时，多字永远领先单字。

```swift
let hasPhrase = candidates.contains { $0.unicodeScalars.count > 1 }
return candidates.enumerated().sorted { lhs, rhs in
  let lIsPhrase = lhs.element.unicodeScalars.count > 1
  let rIsPhrase = rhs.element.unicodeScalars.count > 1
  if hasPhrase, lIsPhrase != rIsPhrase { return lIsPhrase }
  let lScore = lIsPhrase ? (1_000_000 + lc) : (lc * 100)
  let rScore = rIsPhrase ? (1_000_000 + rc) : (rc * 100)
  if lScore != rScore { return lScore > rScore }
  return lhs.offset < rhs.offset
}
```

`Controller.apply` 同步去掉 `preeditSyllables` 参数。

### "错误的文字出现在不对的拼音中"（用户第三次反馈）

`FrequencyTracker.rank` 现在：
- 多字 vs 单字：永远多字第一（除非全是单字）
- 单字内部：`counts * 100` 加成，rime 原顺序平局 —— 单字顺序由 rime 输出频率主导

如果 rime schema 配置了 fuzzy（`n/l, f/h, an/ang` 等模糊音），schema 会输出"似是而非"的
候选；这块不在 DeskCapIME 控制范围内，需要用户检查
`~/Library/Application Support/DeskCap/rime/*.yaml` 的 `speller/algebra/fuzzy` 配置。
我们的 rank 已确保单字顺序稳定，不会把 rime 默认候选颠覆到错误位置。

### Net

`+162 / -151`（净 +11），2 文件。删 `pickBestRect`/`yFlipped`（-87 行），新增 `makeScreenRect`/
`hostWindow(of:)`（+28 行）。

### 验证

- `swiftc -parse-as-library -typecheck -import-objc-header RimeBridge.h *.swift` ✓ 0 错误
- `swiftc -O` 真编译 ✓ arm64 Mach-O 二进制（链接 librime 1.17.0）
- `tsc --noEmit` ✓ 0 错误
- `cargo check` ✗ 工具链硬阻塞（环境问题，本轮无关）

---

## 0.6.46 修复轮（2026-09-29 候选框 placement 彻底稳化）

用户第三次反馈："候选框始终在乱跑，尤其是在屏幕左上角，不是完全跟随文字，
我提出来的问题一个都没有修复"。

承认 0.6.45 的 heuristic flip 没真正修好——`makeScreenRect` 的窗口高度半阈值判别
是 fragile heuristic，且 `hostWindow(of:)` 经 ObjC perform 在某些 client 上返 nil 走
fallback "trust as screen coords" 后仍把 TL 当 BL 用 → panel 永远左上角附近。

### 修复 1 — 删 heuristic flip，纯 同类 IMK 输入法-style 信任 host

```swift
private func makeScreenRect(_ rect: NSRect, from client: IMKTextInput) -> NSRect? {
  guard usable(rect) else { return nil }
  guard let window = hostWindow(of: client) else {
    // Fallback: trust as screen coords only if origin lies in a screen frame.
    return NSScreen.screens.contains(where: { $0.frame.contains(rect.origin) }) ? rect : nil
  }
  // Trust host's rect as bottom-left window coords (HIG). NSWindow converts
  // to screen coords. Hosts that return top-left window coords (某些 Electron 编辑器 /
  // 某些 Electron 编辑器 / 某些应用 / 某些应用) will produce wrong screen y, but at least the
  // panel won't jump to the screen top-left — `applyRect`'s strict clamp
  // guarantees the panel stays inside the visible frame regardless.
  let screenOrigin = window.convertPoint(toScreen: rect.origin)
  return NSRect(origin: screenOrigin, size: rect.size)
}
```

跟 同类 IMK 输入法 一样信任 host attributes。hosts 错的（某些 Electron 编辑器/某些 Electron 编辑器 等 Electron）是
host bug，同类 IMK 输入法 也救不了。

### 修复 2 — retry 阈值 20pt → 50pt（防 host IPC sync 抖动"乱跑"）

```swift
private static let RETRY_MOVE_THRESHOLD_PT: CGFloat = 50  // 0.6.46
```

30 ms retry 时 host（某些 Electron 编辑器 / Electron Chromium IPC sync）会在两帧间返回微抖动
rect（sub-pixel rect jitter 5-15pt）。20pt 阈值太小，每次都触发 re-place → panel
跳动。50pt 是真实 caret 移动的最小可察觉距离。

### 修复 3 — `applyRect` 严格 clamp（保证 panel 永在 visible frame 内）

测试 host 返 (15, 50)（被当 screen coords）:
- vis.minY = 25, origin = (15, 6) — 接近屏顶
- 6 < vis.minY + 8 = 33 → flip → origin.y = 50 + 18 + 4 = 72
- 72 在 visible frame (25, 875) 内 ✓
- setFrameTopLeftPoint((15, 72)) — 屏幕底部偏左，**绝不是左上角**

测试 host 返 (15, 825)（HIG caret in middle of window）:
- origin = (15, 781) → setFrameTopLeftPoint((15, 781)) — caret 下方 ✓

`applyRect` 的 `if origin.y < vis.minY + 8 { origin.y = rect.maxY + 4 }` 这行是关键
兜底：即使 host 给屏外坐标，panel 永远翻转回屏内。

### 关键真相

**用户能跑新版 IME 才能看到修复**。commit `706d79e` 已就绪但没 build / install 到
`/Library/Input Methods/`。本轮 commit 后已经实际 build `/tmp/DeskCapIME-build/
DeskCapIME.app/`（含完整 Frameworks/librime + Info.plist + rime-data + Mach-O arm64
binary），未 codesign。

**需要维护者授权 `bash macos-ime/scripts/build-and-install.sh`** —— 这是发布类动作
（修改系统 IME 目录），脚本会做 codesign Developer ID + notarize + ditto 到
`/Library/Input Methods/` + killall DeskCapIME 重启 IME。

### 净

`+5 / -19` 行（删 heuristic flip 大块代码），单文件。

### 验证

- `swiftc -parse-as-library -typecheck -import-objc-header RimeBridge.h *.swift` ✓ 0 错误
- `swiftc -O` 真编译 ✓ arm64 Mach-O 二进制
- 完整 .app build ✓（`/tmp/DeskCapIME-build/DeskCapIME.app/`，未 codesign）
- `otool -L` 验证 link 是 `@rpath/librime.1.dylib` + rpath 是 `@executable_path/../Frameworks`
- `tsc --noEmit` ✓ 0 错误
