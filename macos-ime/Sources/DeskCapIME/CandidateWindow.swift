import Cocoa
import InputMethodKit
import os.log

/// Horizontal candidate pill **below** marked text, aligned with macOS 系统拼音与同类 IMK 输入法:
/// click to select a candidate · scroll wheel to page · digits 1-9 to select · Space=first.
/// Position: Apple `IMKTextInput.attributes(forCharacterIndex:lineHeightRectangle:)`
/// on the **current caret** after `setMarkedText`. Multi-screen adaptive: panel
/// anchors to whichever screen the caret rect lives on, falls back to mouse-screen,
/// then main-screen. Tries both bottom-left and top-left coord interpretations of
/// every host-returned rect and picks the one that lands the panel naturally on
/// the visible frame — guards against hosts (Chromium/Electron variants) that
/// occasionally return top-left or window-relative rects by mistake.
///
/// 0.6.39 — FrequencyTracker now debounces writes (2 s after last record),
/// caps the dict at 10 000 entries with LRU eviction, and exposes `flush()` for
/// clean shutdown (no committed typing habit is ever lost). Candidates-window
/// placement logic unchanged from 0.6.38 (flush / mouse-proximity / 30 ms retry).
final class CandidateUI {
  static let shared = CandidateUI()

  /// Router that consumes candidate-window events (mouse click selection / scroll-wheel
  /// paging). Set by `DeskCapIMEController.activateServer` on the first transition.
  /// Held weakly so a Controller dealloc never strands the panel.
  weak var router: CandidateRouter?

  /// Last host client that drove a `show(...)`. Stored so the panel mouse callbacks
  /// (which arrive without a `sender` argument) can drive commit / mark through the
  /// correct IMKTextInput.
  weak var lastClient: AnyObject?

  /// 上一次 show() 的 preedit —— 用来判断「用户正在打字」还是「宿主在抖」。
  private var lastShownPreedit = ""

  private let panel: CandidatePanel
  private let root = NSStackView()
  private let candStack = NSStackView()
  private let moreLabel = NSTextField(labelWithString: "∨")
  private var candLabels: [NSTextField] = []
  private let pageSize = 5   // 0.6.62 — 与 rime default.yaml page_size 一致（系统拼音默认 5）

  // Last successful caret rect — sticky fallback when next IMK callback returns NSZero
  // (部分 Electron 宿主的 Chromium IPC 同步问题；同类 IMK 输入法也有同样限制).
  // Only reused when it's on the same screen as the current caret/mouse — a stale
  // rect from a previous window would otherwise glue the panel to the old position.
  private var lastGoodRect: NSRect = .zero
  private var deferredWork: DispatchWorkItem?

  // Hidden origin (tight: AppKit rejects extreme frame origins).
  private static let offscreen = NSPoint(x: -2_000, y: -2_000)

  /// 30 ms retry 位移阈值（pt）。
  ///
  /// 0.6.46 曾把 20 → **50**，为了压 Chromium IPC 的 5–15pt 亚像素抖动，但副作用是
  /// **打字时面板不跟手**：一个字符只移动 8–12pt，永远够不到 50pt。
  /// 0.6.60 改成**按场景分档**（见 `retryThreshold`）：
  ///   - 用户正在打字（preedit 变了）→ 8pt：跟手，允许小步移动
  ///   - 静止（preedit 未变，纯宿主抖动）→ 50pt：保持抗抖动
  private static let RETRY_MOVE_THRESHOLD_TYPING_PT: CGFloat = 8
  private static let RETRY_MOVE_THRESHOLD_IDLE_PT: CGFloat = 50

  private init() {
    panel = CandidatePanel(
      contentRect: NSRect(x: 0, y: 0, width: 720, height: 44),
      styleMask: [.borderless, .nonactivatingPanel],
      backing: .buffered,
      defer: false
    )
    panel.level = .popUpMenu
    panel.isFloatingPanel = true
    panel.hidesOnDeactivate = false
    panel.isOpaque = true
    panel.backgroundColor = .clear
    panel.hasShadow = true
    // macOS 系统拼音与同类 IMK 输入法都把候选面板当作真实 UI 层 ——
    // accept mouse clicks (commit selection) and scroll-wheel (prev/next page). The
    // panel still does not become key/visible-on-deactivate — `.nonactivatingPanel`
    // keeps the host text field focused so the user's next keystroke goes there.
    panel.ignoresMouseEvents = false
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary]
    panel.isReleasedWhenClosed = false
    panel.ui = self

    let bg = NSVisualEffectView()
    bg.material = .popover
    bg.state = .active
    bg.blendingMode = .behindWindow
    bg.wantsLayer = true
    bg.layer?.cornerRadius = 20
    bg.layer?.masksToBounds = true
    bg.layer?.borderWidth = 0.5
    bg.layer?.borderColor = NSColor.separatorColor.cgColor

    candStack.orientation = .horizontal
    candStack.spacing = 12     // tighter for 9 chips
    candStack.alignment = .centerY

    moreLabel.font = .systemFont(ofSize: 14, weight: .medium)
    moreLabel.textColor = .tertiaryLabelColor

    for _ in 0..<pageSize {
      let lab = NSTextField(labelWithString: "")
      // 0.6.47 — 译词要占第二行，所以从单行截断改成按词换行 + 最多两行。
      // `usesSingleLineMode = false` 是 NSTextField 允许多行的前提。
      lab.lineBreakMode = .byWordWrapping
      lab.maximumNumberOfLines = 2
      lab.usesSingleLineMode = false
      lab.alignment = .center
      lab.isBezeled = false
      lab.drawsBackground = false
      lab.isEditable = false
      lab.isHidden = true
      // 单条候选宽度上限：一条长释义不能把 9 个 chip 一起顶出屏幕。
      let widthCap = lab.widthAnchor.constraint(lessThanOrEqualToConstant: 170)
      widthCap.priority = .defaultHigh
      widthCap.isActive = true
      // 0.6.61 — 用户反馈「候选框大概是居中的位置两边分」，与系统拼音的紧凑
      // 排列不同。根因：candStack 四边贴死 panel，label 默认 hugging(250)
      // 会被均匀拉伸去填满剩余宽度 → 每个 chip 等宽、间隔巨大。
      // 提到 required 后 chip 严格按内容宽度（含译词的最长行），
      // 多出来的宽度只会留在 root 的尾部，不再摊到每个候选上。
      lab.setContentHuggingPriority(.required, for: .horizontal)
      candLabels.append(lab)
      candStack.addArrangedSubview(lab)
    }

    root.orientation = .horizontal
    root.alignment = .centerY
    root.spacing = 8
    root.edgeInsets = NSEdgeInsets(top: 2, left: 14, bottom: 4, right: 14)   // 0.6.70 — 单行 chip：上/下内边距收紧，视觉间隙 ≈5pt
    root.translatesAutoresizingMaskIntoConstraints = false
    root.addArrangedSubview(candStack)
    root.addArrangedSubview(moreLabel)
    bg.addSubview(root)
    NSLayoutConstraint.activate([
      root.topAnchor.constraint(equalTo: bg.topAnchor),
      root.bottomAnchor.constraint(equalTo: bg.bottomAnchor),
      root.leadingAnchor.constraint(equalTo: bg.leadingAnchor),
      root.trailingAnchor.constraint(equalTo: bg.trailingAnchor),
    ])
    panel.contentView = bg
  }

  func show(preedit: String, candidates: [String], highlighted: Int, page: Int, pageCount: Int, client: Any?) {
    guard !candidates.isEmpty else {
      hide()
      return
    }
    lastClient = client as AnyObject?
    // 0.6.60 — preedit 变了 = 用户在打字，重定位阈值放宽到 8pt（跟手）；
    // 没变 = 宿主可能只是 IPC 抖动，保持 50pt 抗抖。
    let preeditChanged = preedit != lastShownPreedit
    lastShownPreedit = preedit
    let n = min(candidates.count, candLabels.count)
    for i in 0..<candLabels.count {
      if i < n {
        // 0.6.47 — 候选译词。查不到时 gloss 为 nil，chip 回到 0.6.46 的单行外观。
        let gloss = Glossary.shared.lookup(candidates[i])
        candLabels[i].attributedStringValue = chip(
          num: i + 1, text: candidates[i], gloss: gloss, selected: i == highlighted
        )
        candLabels[i].isHidden = false
      } else {
        candLabels[i].isHidden = true
      }
    }
    moreLabel.isHidden = pageCount <= 1

    root.layoutSubtreeIfNeeded()
    let fit = root.fittingSize
    // 0.6.48 修 — 译词让每条候选多一行，`fittingSize` 比 0.6.47 高约 14pt。
    // 但宽度上限 960pt 在启用译词后仍按单行算：9 条 × (12pt 序号 + 最多 170pt 正文
    // + 12pt 间距) ≈ 1750pt，`min(960, fit.width)` 会把面板**截断**在 960pt，
    // 后面的候选被切出可视区。这里放宽到 1600pt，并在超出时靠 chip 自身
    // 的 `widthAnchor <= 170` + byWordWrapping 自然折行，不裁内容。
    // 0.6.62 — 面板宽度必须小于屏幕可用宽度，否则 `applyRect` 的横向 clamp
    // 会把 x 钉在屏幕左边，表现就是「候选框永远不动」。
    // 实测：9 个带英文释义的 chip ≈ 1500pt，1920 屏上几乎全宽 → 必然钉死。
    // 系统拼音面板通常 400–600pt，才能自由跟随插入点。
    let maxPanelWidth = (NSScreen.main?.visibleFrame.width ?? 1440) * 0.7
    let width = max(160, min(maxPanelWidth, fit.width))
    let height = max(36, fit.height)
    IMELog.write(String(
      format: "show preedit=%@ n=%d fitW=%.0f maxW=%.0f -> w=%.0f h=%.0f",
      preedit, n, fit.width, maxPanelWidth, width, height))
    panel.setContentSize(NSSize(width: width, height: height))

    // Cache hit-test rects (panel.contentView space) so the panel's sendEvent override
    // can resolve mouse-down / mouse-up clicks to a displayed candidate index without
    // re-querying layout every event. Fresh each show() — chip visibility changes
    // page-to-page so stale rects would point at hidden labels.
    let contentView = panel.contentView
    panel.candRects = (0..<n).map { i in
      candLabels[i].convert(candLabels[i].bounds, to: contentView)
    }
    panel.moreRect = moreLabel.convert(moreLabel.bounds, to: contentView)
    panel.clearMouseTransient()

    placeBelowMarkedText(
      preedit: preedit, client: client, width: width, height: height
    )
    panel.orderFrontRegardless()

    // 30 ms retry: re-fetch the caret rect and re-place ONLY if it has moved
    // by more than `RETRY_MOVE_THRESHOLD_PT`. This handles Chromium IPC sync
    // (the synchronous callback can return a stale rect while the IPC pipeline
    // catches up) without causing visible oscillation when the rect is already
    // correct. The previous unconditional retry was the source of the
    // "panel jumps between center and text area" flip-flop on some hosts:
    // the first callback returned zero / mid-screen, the retry got the right
    // rect, and the user saw a two-stage appearance.
    deferredWork?.cancel()
    let originAtShow = panel.frame.origin
    let work = DispatchWorkItem { [weak self] in
      guard let self else { return }
      let newOrigin = self.placeBelowMarkedText(
        preedit: preedit, client: client, width: width, height: height
      )
      let moved = hypot(
        newOrigin.x - originAtShow.x, newOrigin.y - originAtShow.y
      )
      let threshold = preeditChanged
        ? Self.RETRY_MOVE_THRESHOLD_TYPING_PT
        : Self.RETRY_MOVE_THRESHOLD_IDLE_PT
      if moved > threshold {
        NSLog("DeskCapIME retry REPLACE origin=(%.1f,%.1f) -> (%.1f,%.1f) moved=%.1fpt",
              originAtShow.x, originAtShow.y, newOrigin.x, newOrigin.y, moved)
        self.panel.setFrameOrigin(newOrigin)
      } else {
        NSLog("DeskCapIME retry SKIP   origin=(%.1f,%.1f) -> (%.1f,%.1f) moved=%.1fpt (< %.0fpt)",
              originAtShow.x, originAtShow.y, newOrigin.x, newOrigin.y, moved, threshold)
      }
    }
    deferredWork = work
    DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(30), execute: work)
  }

  func hide() {
    deferredWork?.cancel()
    deferredWork = nil
    panel.orderOut(nil)
    panel.setFrameOrigin(Self.offscreen)
    panel.clearMouseState()
    // 0.6.39: persist any pending habit data immediately so an IME / app switch
    // never loses the last few seconds of typed-frequency records.
    FrequencyTracker.shared.flush()
  }

  /// 一个候选 chip。`gloss` 为 nil/空时就是 0.6.46 的单行样式。
  /// 译义独占第二行、字号更小 —— 译词是辅助信息，不能盖过要输入的文字。
  private func chip(num: Int, text: String, gloss: String?, selected: Bool) -> NSAttributedString {
    let s = NSMutableAttributedString()
    let numFont = NSFont.monospacedDigitSystemFont(ofSize: 12, weight: .medium)
    let textFont = NSFont.systemFont(ofSize: 16, weight: .regular)
    let glossFont = NSFont.systemFont(ofSize: 11, weight: .regular)
    if selected {
      s.append(NSAttributedString(
        string: " \(num) \(text) ",
        attributes: [
          .font: textFont,
          .foregroundColor: NSColor.white,
          .backgroundColor: NSColor.controlAccentColor,
        ]
      ))
    } else {
      s.append(NSAttributedString(string: "\(num)", attributes: [
        .font: numFont, .foregroundColor: NSColor.tertiaryLabelColor,
      ]))
      s.append(NSAttributedString(string: " \(text)", attributes: [
        .font: textFont, .foregroundColor: NSColor.labelColor,
      ]))
    }
    if let gloss, !gloss.isEmpty {
      // 0.6.48 修 — 选中项的译词必须跟正文同处强调色胶囊内。
      // 旧实现把 gloss 挂在 \n 之后却不给 backgroundColor，于是译义是「蓝底之外
      // 的白字」：白字落在 popover 的浅色背景上，对比度约 1.3:1，远低于 WCAG AA
      // 的 4.5:1，实际几乎读不出来（ macOS 27 系统拼音的选中译词是整块高亮）。
      // 这里给 gloss 单独补背景 + 前后各留 1 空格，让胶囊形状完整包住两行。
      let glossAttrs: [NSAttributedString.Key: Any] = selected
        ? [
            .font: glossFont,
            .foregroundColor: NSColor.white.withAlphaComponent(0.92),
            .backgroundColor: NSColor.controlAccentColor,
          ]
        : [
            .font: glossFont,
            .foregroundColor: NSColor.secondaryLabelColor,
          ]
      // 0.6.70 — 译词与候选词**同行**（原来是 \n 换行 → 两行 chip）。
      // 两行 chip 会让面板高 72pt，而候选词在面板**顶部**，于是「面板顶贴住
      // 文字底」时候选词实际离文字还有约 22pt（用户反馈的「一个文字高度留白」）。
      // 改成单行后面板高约 26pt，候选词即可紧贴文字，与原生拼音一致。
      s.append(NSAttributedString(string: "  \(gloss) ", attributes: glossAttrs))
    }
    return s
  }


/// Resolve caret placement, apply to panel, return the new top-left origin.
/// Used by both `show()` (real placement) and the 30 ms retry (diff against
/// the at-show origin so we only re-place when the caret shifted > threshold).
///
/// 0.6.45 — bug fix: hosts (Cursor / VS Code / Electron Chromium) commonly
/// return **window-relative** top-left coords from `attributes(...)` and
/// `firstRect(...)` instead of the HIG-mandated **bottom-left screen coords**.
/// Old logic trusted these as screen coords and ran them through per-screen
/// `yFlipped` candidates; the panel ended up at `setFrameTopLeftPoint(origin)`
/// with origin ≈ (15, 6) — i.e. the **screen top-left**. New logic explicitly
/// classifies the rect:
///   - origin in some `NSScreen.frame` ⇒ bottom-left screen coords (HIG)
///   - origin in the host `NSWindow.frame` ⇒ window-relative; convert via
///     `window.convertToScreen(rect)` so we get real screen coords
///   - neither ⇒ drop, fall through to the next probe
@discardableResult
private func placeBelowMarkedText(
  preedit: String, client: Any?, width: CGFloat, height: CGFloat
) -> NSPoint {
  guard let client = client as? IMKTextInput else {
    applyMouseAnchor(width: width, height: height)
    return panel.frame.origin
  }
  let utf16 = (preedit as NSString).length
  let charIndex = caretCharIndex(in: client, utf16: utf16)

  // 1. firstRect(**marked range**) — 0.6.60 提到第一位。
  //
  // 用户反馈「候选框没跟着文字位置走」。根因不只是阈值：`attributes(...)` 按 HIG
  // 返回的是**整行** line-height rect，其 `minX` 是行首（含文本框内边距），
  // 候选框就会贴到比拼音起点更左的位置。`firstRect(marked range)` 返回的正是
  // 这段 marked text 自身的 rect，`minX` 就是拼音串起点 —— 与系统拼音一致。
  var actual = NSRange(location: 0, length: 0)

  // 0.6.63 — 首选「插入点」探测。
  //
  // 用户的实机描述是关键线索：「鼠标移动到哪里打字，候选框就在哪里；正常应该是
  // 点进输入区域的位置」。也就是说 429 次全部落到 `FALLBACK (mouse anchor)` ——
  // 三级探测**一次都没拿到可用 rect**。
  //
  // 根因有两条，都在这一小段里：
  //   a. `location: 0` 是**文档开头**，不是光标位置。文档里有字时，
  //      firstRect 给的是第一个字符的矩形（或直接拒绝）。
  //   b. `usable()` 里 `width <= 0` 会把**插入点矩形**（宽度天然为 0）判成无效，
  //      而插入点恰恰是最准的锚点。
  //
  // 现在按「插入点 → marked range → attributes」的顺序探测，并把原始 rect
  // 记进日志 —— 以后任何位置问题都能一眼看出 host 到底返回了什么。
  let caretProbe = NSRange(location: charIndex, length: 0)
  let caretRect = client.firstRect(forCharacterRange: caretProbe, actualRange: &actual)
  IMELog.write(String(
    format: "probe caret idx=%d raw=(%.1f,%.1f %.1fx%.1f)",
    charIndex, caretRect.origin.x, caretRect.origin.y,
    caretRect.width, caretRect.height))
  if let screenRect = makeScreenRect(caretRect, from: client, allowZeroWidth: true) {
    lastGoodRect = screenRect
    applyRect(screenRect, width: width, height: height)
    IMELog.write(String(
      format: "place[caret] rect=(%.0f,%.0f w=%.0f) -> panel=(%.0f,%.0f)",
      screenRect.origin.x, screenRect.origin.y, screenRect.width,
      panel.frame.origin.x, panel.frame.origin.y))
    return panel.frame.origin
  }

  // 用真实光标位置构造 marked range（不再从文档 0 起算）。
  let markedStart = max(0, charIndex - utf16)
  let markedRange = NSRange(location: markedStart, length: utf16)
  let markedRect = client.firstRect(forCharacterRange: markedRange, actualRange: &actual)
  IMELog.write(String(
    format: "probe marked range=(%d,%d) raw=(%.1f,%.1f %.1fx%.1f)",
    markedStart, utf16, markedRect.origin.x, markedRect.origin.y,
    markedRect.width, markedRect.height))
  if let screenRect = makeScreenRect(markedRect, from: client) {
    lastGoodRect = screenRect
    applyRect(screenRect, width: width, height: height)
    IMELog.write(String(
      format: "place[marked] rect=(%.0f,%.0f w=%.0f) -> panel=(%.0f,%.0f)",
      screenRect.origin.x, screenRect.origin.y, screenRect.width,
      panel.frame.origin.x, panel.frame.origin.y))
    return panel.frame.origin
  }

  // 2. attributes(...) — IMK 官方入口。**index 语义在 host 之间不一致**：
  //    有的期望「文档绝对位置」（charIndex），有的期望「marked 起点相对 0」。
  //    系统拼音传的是相对 0。这里两种都试，谁先给出可用 rect 用谁。
  for probeIndex in [0, charIndex] {
    var lineRect = NSRect.zero
    client.attributes(forCharacterIndex: probeIndex, lineHeightRectangle: &lineRect)
    IMELog.write(String(
      format: "probe attrs idx=%d raw=(%.1f,%.1f %.1fx%.1f)",
      probeIndex, lineRect.origin.x, lineRect.origin.y,
      lineRect.width, lineRect.height))
    if let screenRect = makeScreenRect(lineRect, from: client, allowZeroWidth: true) {
      lastGoodRect = screenRect
      applyRect(screenRect, width: width, height: height)
      IMELog.write(String(
        format: "place[attrs idx=%d] rect=(%.0f,%.0f w=%.0f) -> panel=(%.0f,%.0f)",
        probeIndex, screenRect.origin.x, screenRect.origin.y, screenRect.width,
        panel.frame.origin.x, panel.frame.origin.y))
      return panel.frame.origin
    }
  }

  // 3. firstRect 的其余探测（caret-after / NSNotFound）—— 部分宿主只认这些。
  for rng in [
    NSRange(location: utf16, length: 0),
    NSRange(location: NSNotFound, length: 0),
  ] {
    let r = client.firstRect(forCharacterRange: rng, actualRange: &actual)
    if let screenRect = makeScreenRect(r, from: client) {
      lastGoodRect = screenRect
      applyRect(screenRect, width: width, height: height)
      IMELog.write(String(
        format: "place[firstRect %d,%d] rect=(%.0f,%.0f) -> panel=(%.0f,%.0f)",
        rng.location, rng.length, screenRect.origin.x, screenRect.origin.y,
        panel.frame.origin.x, panel.frame.origin.y))
      return panel.frame.origin
    }
  }

  IMELog.write("place[FALLBACK] no rect from marked/attrs — using mouse anchor")

  // 3. lastGoodRect — sticky across frames but only on the same screen as the
  //    current caret/mouse. A stale rect from a previous window would otherwise
  //    glue the panel to the old position.
  if lastGoodRect != .zero, screenForRect(lastGoodRect) == screenForMouse() {
    applyRect(lastGoodRect, width: width, height: height)
    return panel.frame.origin
  }

  applyMouseAnchor(width: width, height: height)
  return panel.frame.origin
}

/// Convert host-returned rect to a definite bottom-left screen rect, or nil.
///
/// 0.6.46 — dropped the half-window-height heuristic flip (it couldn't reliably
/// distinguish Cursor's top-left coords from a HIG caret near window top, and
/// when the host window lookup failed the panel was trusting raw host coords
/// as screen — which is exactly what produced the "panel always at screen
/// top-left" bug reported by user). Now: **trust host as-is**, period. 同类 IMK 输入法
/// does the same. Hosts that return wrong coords are the host's bug to fix; we
/// can't paper over it. `applyRect`'s strict clamp guarantees the panel never
/// lands above the visible frame top regardless of how badly the host lies.
private func makeScreenRect(_ rect: NSRect, from client: IMKTextInput, allowZeroWidth: Bool = false) -> NSRect? {
  guard usable(rect, allowZeroWidth: allowZeroWidth) else { return nil }

  // 0.6.65 — HIG 规定 `attributes(...)` / `firstRect(...)` 返回的是**屏幕坐标**。
  // 只要 origin 已经落在某块屏幕内，就直接信任 —— **不要**再经 window.convertPoint
  // 转一次（那把屏幕坐标当窗口坐标，会把面板搬到错误位置）。同类实现也是直接用。
  //
  // 实测对照（Electron/WorkBuddy，2026-10-20）：
  //   firstRect(caret)   → (0.0, 19474.9)  ← 文档坐标（行号×行高），必须丢弃
  //   attributes(idx: 0) → (2237, 229, 1×21) ← 真·屏幕坐标，正确锚点
  if NSScreen.screens.contains(where: { $0.frame.contains(rect.origin) }) {
    return rect
  }

  // origin 不在任何屏幕内 → 才当 window-relative 试一次转换（旧 Electron 行为）。
  guard let window = hostWindow(of: client) else { return nil }
  // Treat rect.origin as bottom-left window coords (HIG). NSWindow converts to
  // screen coords. Hosts that return top-left window coords (Cursor / VS Code
  // / Slack / Discord) will produce a wrong screen y, but at least the panel
  // won't jump to the screen top-left as it did with the previous heuristic.
  let screenOrigin = window.convertPoint(toScreen: rect.origin)
  return NSRect(origin: screenOrigin, size: rect.size)
}

/// Reach the host `NSWindow` via ObjC perform — IMKTextInput doesn't expose
/// `window` in its public protocol surface, but every `NSTextInput`-derived
/// client (which is an NSObject subclass) responds to the selector. Cast
/// through NSObject to call `perform(_:)`, which is on the NSObject *class*
/// (not on NSObjectProtocol, so the `@objc protocol IMKTextInput` existential
/// can't reach it directly).
private func hostWindow(of client: IMKTextInput) -> NSWindow? {
  guard let obj = client as? NSObject else { return nil }
  let sel = NSSelectorFromString("window")
  // 0.6.65 — 必须先用 responds(to:) 探一下。`perform` 一个对象不响应的 selector
  // 会抛 NSInvalidArgumentException（doesNotRecognizeSelector），Swift 接不住
  // → std::terminate → abort。2026-10-20 15:30 连崩三次就是这个：
  //   CandidateUI.hostWindow(of:) → objc_exception_throw → __cxa_throw
  guard obj.responds(to: sel) else { return nil }
  guard let result = obj.perform(sel) else { return nil }
  return result.takeUnretainedValue() as? NSWindow
}

/// Find the screen the rect lives on. Multi-screen adaptive: when the user drags a
  /// window between displays, this picks up the new screen on every callback so the
  /// pill follows the caret across monitors.
private func screenForRect(_ rect: NSRect) -> NSScreen? {
    let pt = NSPoint(x: rect.midX, y: rect.midY)
    // Primary: midpoint is inside a screen frame.
    if let s = NSScreen.screens.first(where: { NSMouseInRect(pt, $0.frame, false) }) {
      return s
    }
    // Secondary: any part of the rect intersects a screen (caret near an edge).
    if let s = NSScreen.screens.first(where: { $0.frame.intersects(rect) }) {
      return s
    }
    // Tertiary: nearest screen by midpoint distance (rect momentarily between displays).
    let screens = NSScreen.screens
    if let s = screens.min(by: {
      hypot($0.frame.midX - pt.x, $0.frame.midY - pt.y) <
      hypot($1.frame.midX - pt.x, $1.frame.midY - pt.y)
    }) {
      return s
    }
    return NSScreen.main
  }

  /// Clamp the panel to whichever screen contains the rect — never bleeds across
  /// displays, never lands off-screen, never sits in the dock's bottom-left corner.
  /// Flips above the caret when the area below would push the panel off-screen.
  /// Panel top is flush with the caret line (no gap), matching macOS's built-in
  /// 拼音 panel.
  private func applyRect(_ rect: NSRect, width: CGFloat, height: CGFloat) {
    guard let screen = screenForRect(rect) else { return }
    let vis = screen.visibleFrame

    // 0.6.70 — 候选词行**紧贴**文字（原生拼音手感）。
    //
    // 之前（0.6.66）把「面板**底部**」贴住 rect.minY，看着像收紧了，实际反了：
    // 面板总高 72pt（候选行 36 + 译词行 36），而**候选词那一行在面板顶部**，
    // 于是候选词实际落在 rect.minY 下方约 66pt 处 —— 正好是「一个文字高度」的
    // 空隙（截图确认）。
    //
    // 现在改为：把**面板顶部**放到文字底部下方 textGap，候选词行自然紧贴文字，
    // 译词行随之下移。textGap 取 2pt，与原生拼音一致（略小于行框底到字形
    // 视觉底的 descender 距离）。做成常量便于肉眼校准。
    // 屏幕坐标 y 向上为正：文字底 = rect.minY，面板要落在文字**下方**
    // → 面板顶 = rect.minY - textGap。
    let textGap: CGFloat = 0
    var origin = NSPoint(x: rect.minX, y: rect.minY - textGap)
    // Flip above the caret if below would push the panel off the bottom of the
    // visible frame.
    if origin.y < vis.minY + 8 {
      // 下方放不下 → 翻到上方，候选词行贴住行框顶。
      origin.y = rect.maxY + textGap
    }
    // If above also doesn't fit (caret near the top of the screen), clamp back below
    // and accept the bottom-edge overflow rather than hiding the pill.
    if origin.y + height > vis.maxY {
      origin.y = max(vis.minY + 8, vis.maxY - height - 8)
    }

    // Horizontal clamp so the panel stays within the screen's visible area.
    origin.x = min(max(origin.x, vis.minX + 8),
                   max(vis.minX + 8, vis.maxX - width - 8))
    panel.setFrameTopLeftPoint(origin)
  }

  /// Pick the character index to anchor the candidate pill under:
  /// - The host's caret (`selectedRange().location`) when the host provides one.
  /// - The end of the marked preedit (utf16 length) when nothing else is available.
  /// 同类 IMK 输入法总是用 index 0（输入框的第一个字符）—— 我们更进一步
  /// and use the real caret offset when the host exposes it.
  private func caretCharIndex(in client: IMKTextInput, utf16: Int) -> Int {
    let sel = client.selectedRange()
    if sel.location != NSNotFound, sel.location >= 0 {
      return sel.location
    }
    return utf16
  }

  /// Last-resort anchor: place the panel near the mouse cursor and clamp onto the
  /// screen containing the mouse. Used only when no caret rect is available.
  ///
  /// v0.6.40: dropped the "top-center of main screen" fallback. That fallback put
  /// the pill at `screen.midX, vis.maxY - height` when the mouse screen was
  /// unknown — the user saw it jump to the middle of the screen and then, when the
  /// 30 ms retry got a real caret rect, jump again to the text. Now we just don't
  /// show the pill at all when we have no anchor info — better than a confusing
  /// mid-screen flash.
  private func applyMouseAnchor(width: CGFloat, height: CGFloat) {
    let mp = NSEvent.mouseLocation
    guard let screen = screenForMouse() ?? NSScreen.main else {
      // No screen info at all — hide rather than show at an arbitrary location.
      panel.orderOut(nil)
      panel.setFrameOrigin(Self.offscreen)
      return
    }
    NSLog("DeskCapIME mouse-anchor screen=%@ mouse=(%.1f,%.1f)",
          screen.localizedName, mp.x, mp.y)
    // y = mp.y + 18 → applyRect computes panel top-left at rect.minY - height,
    // so the panel lands 18 pt above the cursor. Horizontal + vertical clamping
    // (including the above-cursor flip when near the top edge) is applyRect's job.
    let rect = NSRect(x: mp.x - width / 2, y: mp.y + 18, width: width, height: height)
    applyRect(rect, width: width, height: height)
  }

  /// Find the screen containing the mouse cursor (used as a fallback anchor when no
  /// caret rect is available).
  private func screenForMouse() -> NSScreen? {
    let mp = NSEvent.mouseLocation
    return NSScreen.screens.first(where: { NSMouseInRect(mp, $0.frame, false) })
  }

  /// A rect is usable as long as its midpoint is on (or very near) some screen, and
  /// it's not the "all zeros" sentinel some hosts return. Width/height sanity guards
  /// reject obviously broken values.
  private func usable(_ r: NSRect, allowZeroWidth: Bool = false) -> Bool {
    if r == .zero { return false }
    if abs(r.origin.x) < 2 && abs(r.origin.y) < 2 { return false }
    // 插入点矩形宽度天然为 0 —— 那正是最准的锚点，不能拒。
    if allowZeroWidth {
      if r.width < 0 || r.height <= 0 { return false }
    } else if r.width <= 0 || r.height <= 0 {
      return false
    }
    if r.width > 10000 || r.height > 10000 { return false }   // sanity guard
    let pt = NSPoint(x: r.midX, y: r.midY)
    return NSScreen.screens.contains(where: { NSMouseInRect(pt, $0.frame, false) })
  }
}

/// Global candidate-frequency tracker. Ranks candidates by how often the user picks
/// them — the most-used word floats to the top of the page (with rime's original order
/// as the tiebreaker). Persisted to
/// `~/Library/Application Support/DeskCap/rime/deskcap_freq.json`.
///
/// 0.6.39 stability work:
/// - `record(...)` debounces writes (2 s after the last record, not per key) so fast
///   typing doesn't hammer the disk.
/// - Dict is LRU-capped at 10 000 entries with hysteresis (eviction kicks in only
///   when we hit 20 000 entries, then trims back down to 10 000) — prevents unbounded
///   memory growth over months of use.
/// - `flush()` persists immediately and is called from `CandidateUI.hide()` so a
///   switch to another IME or app shutdown never loses the last few seconds of
///   committed habit data.
final class FrequencyTracker {
  static let shared = FrequencyTracker()

  private let lock = NSLock()
  private var counts: [String: Int] = [:]

  // LRU cap. Eviction runs when the dict doubles the cap (hysteresis avoids
  // thrashing), then trims back to `maxEntries`. A 10 K-entry dict of short Chinese
  // strings is ~50 KB on disk — negligible memory cost.
  private let maxEntries = 10_000
  private let evictionTrigger = 20_000

  // Debounced save: at most one pending write per `debounceSeconds` window. Saves
  // run on a utility queue, atomic on the filesystem, so concurrent writes always
  // land the latest snapshot.
  private var saveScheduled = false
  private let debounceSeconds: TimeInterval = 2.0
  private let saveQueue = DispatchQueue(label: "deskcap.ime.freq.save", qos: .utility)

  private let url: URL = {
    let dir = FileManager.default.homeDirectoryForCurrentUser
      .appendingPathComponent("Library/Application Support/DeskCap/rime", isDirectory: true)
    try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    return dir.appendingPathComponent("deskcap_freq.json")
  }()

  private init() {
    load()
  }

  private func load() {
    guard let data = try? Data(contentsOf: url),
          let dict = try? JSONDecoder().decode([String: Int].self, from: data) else {
      return
    }
    lock.lock()
    counts = dict
    lock.unlock()
  }

  /// Persist the current snapshot immediately, bypassing the debounce timer.
  /// Call from `CandidateUI.hide()` so app/IME switches never lose typed habits.
  func flush() {
    lock.lock()
    saveScheduled = false
    let snapshot = counts
    lock.unlock()
    saveQueue.async { [weak self] in
      self?.persist(snapshot)
    }
  }

  /// Increment a candidate's usage count. Cheap lock + bounded work; persistence is
  /// debounced (one write per 2 s window under fast typing).
  func record(candidate: String) {
    guard !candidate.isEmpty else { return }
    lock.lock()
    counts[candidate, default: 0] += 1
    // Hysteresis LRU eviction: only run when we hit double the cap, then trim back
    // to `maxEntries`. Sorting N entries by count is O(N log N); for N ≤ 20 K this
    // is sub-millisecond.
    if counts.count > evictionTrigger {
      let sorted = counts.sorted { $0.value < $1.value }
      for (k, _) in sorted.prefix(counts.count - maxEntries) {
        counts.removeValue(forKey: k)
      }
    }
    let needSchedule = !saveScheduled
    if needSchedule { saveScheduled = true }
    lock.unlock()

    if needSchedule {
      saveQueue.asyncAfter(deadline: .now() + debounceSeconds) { [weak self] in
        self?.flush()
      }
    }
  }

  /// Atomic JSON write. Failures are silent — the in-memory state is preserved, and
  /// the next save will retry.
  private func persist(_ snapshot: [String: Int]) {
    guard let data = try? JSONEncoder().encode(snapshot) else { return }
    try? data.write(to: url, options: .atomic)
  }

  /// Reorder candidates so the user-intended word stays on top.
///
/// 0.6.45 — bug fix (用户反馈: "如果输入的不是一个字就要确保候选的是 2 个及以上的词在第一位置"):
/// the previous boost was gated on `preeditSyllables >= 2` — but the preedit
/// arrives as the literal pinyin string with rime-internal spaces, which means
/// "tingxia" (no space) gave `preeditSyllables == 1` and multi-char words
/// stayed behind a frequently-used single char. New rule: any candidate with
/// `unicodeScalars.count > 1` (i.e. 2+ CJK chars) gets a constant phrase boost
/// that always exceeds any realistic single-char count, so 2+ 字词 always floats
/// above single-char candidates.
///
/// 0.6.54 — 用户反馈（截图：`kai fa zhe` 第一候选是 2 字「开发」而非 3 字「开发者」）：
/// 上一版所有 phrase 共用同一个 1_000_000 常数加成，phrase 之间**只按使用频次**排，
/// 与输入的音节数无关。输入 3 个音节（kai fa zhe）时，字词长度匹配的「开发者」
/// 应排第一 —— 这是 mac 官方拼音与同类 IMK 输入法共同的排序直觉。
/// 新规则：phrase 加成按「字数与音节数的接近程度」分级 ——
///   字数 == 音节数 → +3_000_000（精确匹配，最优先）
///   差 1            → +2_000_000
///   其他 phrase     → +1_000_000（仍高于任何单字）
/// 频次只作为同级内的微调（`lc` 加在分级基数上）。
///
/// Single-char candidates keep the rime default unless the user has explicitly
/// picked them — frequency weight is multiplied by 100 (so a count of 50 yields
/// 5 000) which can't override a `1_000_000` phrase boost but can re-rank within
/// the single-char group without burying the rime default. Rime's order stays
/// as the tiebreaker so newly-released words don't get pushed out forever.
///
/// - Parameter syllables: 当前 preedit 的音节数（空格分隔的拼音段数）。
///   传 0 或负数表示未知 —— 退化为旧行为（所有 phrase 同档）。
///
/// Caller is responsible for translating the displayed index back to rime's
/// index when the user picks (digit d → look up displayed[d-1]'s rime index).
func rank(_ candidates: [String], syllables: Int = 0) -> [String] {
  guard !candidates.isEmpty else { return candidates }
  lock.lock()
  let snapshot = counts
  lock.unlock()
  let hasPhrase = candidates.contains { $0.unicodeScalars.count > 1 }
  // phrase 分级基数：字数越接近音节数越高。
  func phraseBase(_ charCount: Int) -> Int {
    guard syllables > 0 else { return 1_000_000 }
    switch abs(charCount - syllables) {
    case 0: return 3_000_000
    case 1: return 2_000_000
    default: return 1_000_000
    }
  }
  return candidates.enumerated().sorted { lhs, rhs in
    let lChars = lhs.element.count
    let rChars = rhs.element.count
    let lIsPhrase = lhs.element.unicodeScalars.count > 1
    let rIsPhrase = rhs.element.unicodeScalars.count > 1
    let lc = snapshot[lhs.element] ?? 0
    let rc = snapshot[rhs.element] ?? 0
    // 1. When there's at least one phrase candidate, phrases always precede
    //    single chars — the user typed 2+ pinyin syllables; they want a word.
    if hasPhrase, lIsPhrase != rIsPhrase { return lIsPhrase }
    // 2. Phrase 之间按「字数匹配音节数」分级，同级内按频次。
    //    单字档位保持 `lc * 100`，永不可能盖过 phrase。
    let lScore = lIsPhrase ? (phraseBase(lChars) + lc) : (lc * 100)
    let rScore = rIsPhrase ? (phraseBase(rChars) + rc) : (rc * 100)
    if lScore != rScore { return lScore > rScore }
    return lhs.offset < rhs.offset
  }.map { $0.element }
}
}

// MARK: - Panel mouse routing (macOS 系统拼音 / 同类实现 parity)

/// Surface for `DeskCapIMEController` to consume candidate-window mouse events.
/// Wired via `CandidateUI.shared.router` in the controller's first `activateServer`.
/// Kept protocol-scoped so the candidate panel never reaches back into the controller
/// for IMK internals (selection commit, cursor paging).
protocol CandidateRouter: AnyObject {
  /// User clicked a candidate at the displayed index. The router must translate the
  /// displayed index to rime's internal index and commit via the standard apply path.
  func candidateSelected(displayedIndex: Int)
  /// User scrolled the panel. `up = true` for prev page (与同类实现一致：dy>0 → page-up).
  func candidatePageUpDown(up: Bool)
}

/// NSPanel subclass that owns candidate-window mouse routing. 同类实现也是同样做法
/// via `sendEvent` override on its panel subclass — we follow that pattern so the
/// floating panel behaves like a real clickable UI surface without ever stealing key
/// focus from the host text field.
final class CandidatePanel: NSPanel {
  weak var ui: CandidateUI?
  /// Cached hit-test rects in `contentView` coordinates. Refreshed each `show()`.
  var candRects: [NSRect] = []
  var moreRect: NSRect = .zero

  /// Sticky mouse-down state — used to require mouse-up over the SAME candidate before
  /// committing (与同类实现的 `pagingUp`/`self.index` guard 一致). Reset on mouse-up.
  private var lastDownIndex: Int = -1
  private var lastDownPageUp: Bool?

  /// Clear everything tied to a specific show() (transient). Called after each layout
  /// pass so the first mouse-down in a new panel state doesn't match a stale rect.
  func clearMouseTransient() {
    lastDownIndex = -1
    lastDownPageUp = nil
  }

  /// Clear everything including cached hit-test rects. Called from `CandidateUI.hide()`
  /// so a future show() always rebuilds from a known empty state.
  func clearMouseState() {
    clearMouseTransient()
    candRects = []
    moreRect = .zero
  }

  /// Hit-test a panel-contentView-space point. Returns either a displayed candidate
  /// index or a paging hit (`moreRect` is the trailing "∨" — click = page down,
  /// matching macOS 系统拼音 / 同类实现的 "∨" semantics).
  private func hitTest(point p: NSPoint) -> (cand: Int?, pageUp: Bool?) {
    if moreRect != .zero, moreRect.contains(p) {
      return (nil, false)
    }
    for (i, r) in candRects.enumerated() where r.contains(p) {
      return (i, nil)
    }
    return (nil, nil)
  }

  override func sendEvent(_ event: NSEvent) {
    // `event.locationInWindow` is window-space; map to contentView-space for the
    // hit test (candRects / moreRect were cached in `convert(_ bounds, to: contentView)`
    // back in `CandidateUI.show`). NSWindow itself has no `convert(_:from:)` —
    // delegate to it via the contentView.
    let p = contentView?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow
    switch event.type {
    case .leftMouseDown:
      let hit = hitTest(point: p)
      if let i = hit.cand {
        lastDownIndex = i
        lastDownPageUp = nil
      } else if let u = hit.pageUp {
        lastDownPageUp = u
        lastDownIndex = -1
      } else {
        lastDownIndex = -1
        lastDownPageUp = nil
      }
    case .leftMouseUp:
      let hit = hitTest(point: p)
      if let i = hit.cand, i == lastDownIndex {
        ui?.router?.candidateSelected(displayedIndex: i)
      } else if let u = hit.pageUp, lastDownPageUp == u {
        ui?.router?.candidatePageUpDown(up: u)
      }
      lastDownIndex = -1
      lastDownPageUp = nil
    case .scrollWheel:
      // 同类实现把 dy>0 视为 "scroll content down = page up"; dy<0 as page down.
      // Threshold 5 pt ignores trackpad jitter while still paging on every normal flick.
      let dy = event.scrollingDeltaY
      if abs(dy) > 5 {
        ui?.router?.candidatePageUpDown(up: dy > 0)
      }
    default:
      break
    }
    super.sendEvent(event)
  }
}
