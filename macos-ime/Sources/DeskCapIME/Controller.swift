import Cocoa
import InputMethodKit
import Foundation

@objc(DeskCapIMEController)
public class DeskCapIMEController: IMKInputController, CandidateRouter {
  private var currentCandidates: [String] = []
  // Reentrancy mirror: AppKit calls activateServer/deactivateServer in 1–10 ms cycles on
  // Cursor / Electron; only do real work on the first transition of each transition.
  private var serverActive = false

  // Frequency-ranked candidates shown to the user, plus the displayed->rime index map.
  // Controller rebuilds these on every apply(); they're consumed by the digit/space
  // selection paths to translate what the user sees back to rime's internal order.
  private var lastDisplayed: [String] = []
  private var lastDisplayedToRime: [Int] = []
  private var lastDisplayedHighlighted: Int = 0

  /// IMK calls this to populate the system candidate bar.
  @objc public func candidates() -> [Any]! {
    return currentCandidates
  }

  public override func activateServer(_ sender: Any!) {
    let first = !serverActive
    serverActive = true
    if first {
      currentCandidates = []
      CandidateUI.shared.hide()
      CandidateUI.shared.router = self
      DeskCapBridge.setImeActive(true)
      // Cheap: only clear ascii if stuck; do not rebuild session here.
      if RimeEngine.shared.isAsciiMode {
        RimeEngine.shared.setAsciiMode(false)
      }
      Self.auditMemoryOnActivate()
    }
    super.activateServer(sender)
  }

  /// 0.6.49 — 每次进入新的打字会话时自查常驻内存，超过阈值就打一条醒目日志。
  ///
  /// 依据：macOS 26+ AppKit 的 NSWindow 绘制效率很差，一个输入法进程的常驻
  /// 内存可能从 80MB 涨到 200MB；且**用过的 NSWindow 空间系统不主动回收**。
  /// 我们只有 1 个 NSPanel 单例（`CandidateUI.shared`），但 rime 词典缓存 +
  /// 译词表 + 频率表都是长驻的，量一下比猜靠谱。
  ///
  /// 只记日志、不自杀：输入法崩溃会打断用户正在打的字，代价远大于多占内存；
  /// 真超限时靠日志定位是哪一类缓存涨上来的更实际。
  private static func auditMemoryOnActivate() {
    var info = task_vm_info_data_t()
    var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / 4)
    let kr = withUnsafeMutablePointer(to: &info) {
      $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
        task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
      }
    }
    guard kr == KERN_SUCCESS else {
      NSLog("DeskCapIME memory audit failed kr=%d", kr)
      return
    }
    let mb = Int(info.phys_footprint) / (1024 * 1024)
    NSLog("DeskCapIME memory footprint=%dMB", mb)
    if mb > 1024 {
      NSLog("DeskCapIME memory HIGH %dMB (>1024MB) — 若持续增长请查 rime 词典缓存 / 词表 / FrequencyTracker 是否泄漏", mb)
    }
  }

  public override func deactivateServer(_ sender: Any!) {
    let wasActive = serverActive
    serverActive = false
    if wasActive {
      currentCandidates = []
      CandidateUI.shared.hide()
      DeskCapBridge.setImeActive(false)
      DeskCapBridge.flushNow()
      _ = RimeEngine.shared.cancel()
    }
    super.deactivateServer(sender)
  }

  public override func inputText(_ string: String!, client sender: Any!) -> Bool {
    guard let string, !string.isEmpty else { return false }

    // Shift+字母 → 大写英文直出（先提交未完成拼音，再插入该字母）
    if string.unicodeScalars.count == 1,
       let u = string.unicodeScalars.first,
       CharacterSet.uppercaseLetters.contains(u) {
      insertEnglish(string, client: sender)
      return true
    }

    if string.unicodeScalars.count == 1,
       let u = string.unicodeScalars.first,
       CharacterSet.letters.contains(u) {
      // processAscii handles stuck ascii_mode cheaply (only when needed)
      return apply(RimeEngine.shared.processAscii(string.lowercased()), client: sender)
    }

    if let d = Int(string), (1...9).contains(d) {
      if RimeEngine.shared.isComposing {
        // Translate the displayed position back to rime's index, then select.
        // displayedToRime.count == lastDisplayed.count is invariant (built in apply),
        // so the third check is redundant and only the first two are needed.
        let dispIdx = d - 1
        if dispIdx >= 0, dispIdx < lastDisplayed.count {
          let rimeIdx = lastDisplayedToRime[dispIdx] + 1
          FrequencyTracker.shared.record(candidate: lastDisplayed[dispIdx])
          return apply(RimeEngine.shared.selectDigit(rimeIdx), client: sender)
        }
        return apply(RimeEngine.shared.selectDigit(d), client: sender)
      }
      return false
    }

    if string == " " {
      if RimeEngine.shared.isComposing {
        // Pick the displayed-highlighted candidate (mapped back to rime's index).
        // displayedToRime.count == lastDisplayed.count is invariant (built in apply),
        // so we only need to bounds-check against the latter.
        if lastDisplayedHighlighted >= 0,
           lastDisplayedHighlighted < lastDisplayed.count {
          let rimeIdx = lastDisplayedToRime[lastDisplayedHighlighted] + 1
          FrequencyTracker.shared.record(candidate: lastDisplayed[lastDisplayedHighlighted])
          return apply(RimeEngine.shared.selectDigit(rimeIdx), client: sender)
        }
        return apply(RimeEngine.shared.selectFirstOrSpace(), client: sender)
      }
      return false
    }

    if string == "\r" || string == "\n" {
      // macOS 27 系统拼音：回车 = 上屏第一个候选（与空格一致）。无候选时不响应
      // （拼音保留在 preedit，让用户用 ESC 取消或继续输入）。`commitPreeditAsEnglish`
      // 是我们的旧方言；与 mac 官方不符，已删除。
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
    }

    // Page keys while composing
    if RimeEngine.shared.isComposing, let ch = string.unicodeScalars.first,
       ch == "-" || ch == "=" || ch == "," || ch == "." {
      return apply(RimeEngine.shared.processKey(code: Int32(ch.value)), client: sender)
    }

    // Shift+标点：走 rime 出候选；rime 无结果时 **回退直出原字符**（防吞键）
    if string.unicodeScalars.count == 1, let u = string.unicodeScalars.first {
      var key: Int32? = nil
      switch u {
      case ">": key = Int32(UInt8(ascii: "."))
      case "<": key = Int32(UInt8(ascii: ","))
      default:
        if isChinesePunctScalar(u) { key = Int32(u.value) }
      }
      if let key, !RimeEngine.shared.isAsciiMode {
        let st = RimeEngine.shared.processKey(code: key)
        if st.commit != nil || !st.preedit.isEmpty || !st.candidates.isEmpty {
          return apply(st, client: sender)
        }
        // rime swallowed the key (e.g. `\` `) — insert the original character.
        if let client = sender as? IMKTextInput {
          client.insertText(string, replacementRange: NSRange(location: NSNotFound, length: 0))
        }
        return true
      }
    }

    return false
  }

  /// ASCII punctuation routed through rime in Chinese mode.
  private func isChinesePunctScalar(_ u: Unicode.Scalar) -> Bool {
    guard let c = Character(String(u)).asciiValue else { return false }
    let set: Set<UInt8> = [
      0x2C, 0x2E, 0x3F, 0x21, 0x3A, 0x3B, // , . ? ! : ;
      0x28, 0x29, 0x5B, 0x5D, 0x7B, 0x7D, // ( ) [ ] { }
      0x3C, 0x3E, 0x22, 0x27, 0x5C, 0x2F, // < > " ' \ /
      0x7E, 0x40, 0x23, 0x24, 0x25, 0x5E, // ~ @ # $ % ^
      0x26, 0x2A, 0x5F, 0x2B, 0x3D, 0x7C, // & * _ + = |
      0x2D, // -
      0x60, // `
    ]
    return set.contains(c)
  }

  public override func didCommand(by selector: Selector!, client sender: Any!) -> Bool {
    // 回车在多数 App 走 insertNewline:，不是 inputText("\r")。
    //
    // 0.6.51 修（用户回归报告：「我输入英文直接回车不是英文了」）——
    // 0.6.44 那轮把回车改成了「上屏第一候选」，**搞反了 mac 官方语义**。
    // macOS 系统拼音的官方行为是：
    //   Enter = 上屏**原始输入字母**（用于输网址 / 英文名 / 拼音原文），
    //   Space = 上屏第一候选。
    // 实测：官方拼音打「nihao」按回车上屏的是 "nihao" 字母串，不是「你好」。
    // 同类 IMK 输入法默认同此。0.6.44 删掉的 `commitPreeditAsEnglish`
    // 才是正确语义，当时被误标为「与 mac 官方冲突」。
    if selector == #selector(NSResponder.insertNewline(_:))
      || selector == NSSelectorFromString("insertNewlineIgnoringModifiers:") {
      guard RimeEngine.shared.isComposing else {
        // 无 preedit：让系统正常换行，不拦截。
        DeskCapBridge.flushNow()
        return false
      }
      // 有 preedit：上屏原始字母（mac 官方），并上报采集（英文也是数据）。
      commitOpenPreeditAsLatin(client: sender)
      currentCandidates = []
      CandidateUI.shared.hide()
      DeskCapBridge.flushNow()
      return true
    }
    if selector == #selector(NSResponder.deleteBackward(_:)) {
      if !RimeEngine.shared.isComposing { return false }
      return apply(RimeEngine.shared.processBackspace(), client: sender)
    }
    if selector == #selector(NSResponder.cancelOperation(_:)) {
      if !RimeEngine.shared.isComposing { return false }
      return apply(RimeEngine.shared.cancel(), client: sender)
    }
    if !RimeEngine.shared.isComposing { return false }

    let pageDown = #selector(NSResponder.pageDown(_:))
    let pageUp = #selector(NSResponder.pageUp(_:))
    let down = #selector(NSResponder.moveDown(_:))
    let up = #selector(NSResponder.moveUp(_:))
    if selector == pageDown || selector == down {
      let code: Int32 = (selector == pageDown) ? 0xff56 : 0xff54
      return apply(RimeEngine.shared.processKey(code: code), client: sender)
    }
    if selector == pageUp || selector == up {
      let code: Int32 = (selector == pageUp) ? 0xff55 : 0xff52
      return apply(RimeEngine.shared.processKey(code: code), client: sender)
    }
    return false
  }

  /// Commit any open pinyin preedit as raw Latin text (joined without spaces) and
  /// clear the marked-text / candidates.
  ///
  /// 触发场景（均为 mac 官方语义）：
  ///   1. **回车**（`didCommand insertNewline`）—— mac 官方拼音的 Enter =
  ///      上屏原始字母，用于输网址 / 英文名 / 拼音原文。
  ///   2. **Shift + 大写字母**（`insertEnglish` 路径）—— 先把未完成拼音按
  ///      英文上屏，再插入该大写字母。
  ///
  /// 0.6.44 曾误删「回车 = 上屏拼音」路径并误注释为「与 mac 官方冲突」，
  /// 0.6.51 已纠正回 mac 官方语义。
  private func commitOpenPreeditAsLatin(client sender: Any?) {
    guard RimeEngine.shared.isComposing else { return }
    let preedit = RimeEngine.shared.currentPreedit()
    _ = RimeEngine.shared.cancel()
    let latin = preedit.replacingOccurrences(of: " ", with: "")
    if !latin.isEmpty, let client = sender as? IMKTextInput {
      client.insertText(latin, replacementRange: NSRange(location: NSNotFound, length: 0))
      DeskCapBridge.postCommit(text: latin)
    }
    mark(preedit: "", client: sender)
    NSLog("DeskCapIME: committed open preedit as latin len=%d", latin.count)
  }

  /// Shift+大写字母路径：先把任何未完成拼音作为英文上屏，再插入该大写字母。
  /// 这是 mac 官方系统拼音在中文 IME 下接受大写字母的官方行为。
  private func insertEnglish(_ text: String, client sender: Any?) {
    commitOpenPreeditAsLatin(client: sender)
    if let client = sender as? IMKTextInput {
      client.insertText(text, replacementRange: NSRange(location: NSNotFound, length: 0))
    }
    currentCandidates = []
    CandidateUI.shared.hide()
    DeskCapBridge.flushNow()
  }

  @discardableResult
  private func apply(_ st: RimeEngine.State, client sender: Any?) -> Bool {
    if let commit = st.commit, !commit.isEmpty {
      if let client = sender as? IMKTextInput {
        client.insertText(commit, replacementRange: NSRange(location: NSNotFound, length: 0))
      }
      DeskCapBridge.postCommit(text: commit)
    }
    mark(preedit: st.preedit, client: sender)
    currentCandidates = st.candidates

    // Frequency-based re-ranking: 0.6.45 — multi-char phrases always go to the
    // top, single chars are nudged by `counts * 100` but cannot override rime's
    // default order. 0.6.54 — phrase 内部再按「字数匹配音节数」分级：输入
    // `kai fa zhe`（3 音节）时 3 字「开发者」优先于 2 字「开发」。
    // 音节数从 preedit 数（rime 用空格分隔音节，截图实测 `kai fa zhe` 即带空格）；
    // preedit 为空时传 0（旧行为）。
    // 兜底：若只切出 1 段但串明显偏长（≥5 字母，如 "kaifazhe" 粘连形态），
    // 说明这个 host 的 preedit 可能不带分隔 —— 视为「未知」传 0，
    // 退化为 0.6.53 的同档行为，而不是错分成 1 音节把 2 字词抬到 3 字词前面。
    let rawSyllables = st.preedit
      .split(separator: " ", omittingEmptySubsequences: true)
      .count
    let syllables = (rawSyllables <= 1 && st.preedit.count >= 5) ? 0 : rawSyllables
    let ranked = FrequencyTracker.shared.rank(st.candidates, syllables: syllables)
    var rimeAvailable = Array(0..<st.candidates.count)
    var displayedToRime: [Int] = []
    for displayedIdx in 0..<ranked.count {
      let target = ranked[displayedIdx]
      if let rimeIdx = rimeAvailable.first(where: { st.candidates[$0] == target }) {
        displayedToRime.append(rimeIdx)
        rimeAvailable.removeAll(where: { $0 == rimeIdx })
      } else {
        displayedToRime.append(displayedIdx)
      }
    }
    // Standard IME behavior (macOS 27 system 拼音 / iOS 27 / 同类 IMK 输入法):
    //   - Fresh input (rime hasn't moved highlight off 0): top displayed candidate is
    //     always highlighted — even if our frequency rank put rime's candidate 0 at
    //     a different displayed position. The user reads top-down and presses Space /
    //     1 to commit; highlighting anything else confuses that flow.
    //   - User pressed up / down : rime's highlighted > 0, so we follow rime's
    //     highlight and map it back to the displayed position (preserves nav).
    let displayedHighlighted: Int
    if st.highlighted == 0 && !ranked.isEmpty {
      displayedHighlighted = 0
    } else if st.highlighted > 0,
              st.highlighted < st.candidates.count,
              let dispIdx = displayedToRime.firstIndex(of: st.highlighted) {
      displayedHighlighted = dispIdx
    } else {
      displayedHighlighted = 0
    }

    self.lastDisplayed = ranked
    self.lastDisplayedToRime = displayedToRime
    self.lastDisplayedHighlighted = displayedHighlighted

    // Show on next turn so client layout has applied setMarkedText (firstRect follows caret).
    let preedit = st.preedit
    let cands = ranked
    let hi = displayedHighlighted
    let page = st.page
    let pageCount = st.pageCount
    let clientRef = sender
    DispatchQueue.main.async {
      CandidateUI.shared.show(
        preedit: preedit,
        candidates: cands,
        highlighted: hi,
        page: page,
        pageCount: pageCount,
        client: clientRef
      )
    }
    return true
  }

  private func mark(preedit: String, client sender: Any?) {
    guard let client = sender as? IMKTextInput else { return }
    let marked = NSAttributedString(string: preedit)
    client.setMarkedText(
      marked,
      selectionRange: NSRange(location: preedit.utf16.count, length: 0),
      replacementRange: NSRange(location: NSNotFound, length: 0)
    )
  }

  // MARK: - CandidateRouter (panel mouse wiring)

  /// Mouse click on the candidate panel at the displayed index — same path as a digit
  /// key press but driven by panel hit-test instead of `inputText`.
  func candidateSelected(displayedIndex: Int) {
    guard RimeEngine.shared.isComposing else { return }
    guard displayedIndex >= 0, displayedIndex < lastDisplayed.count else { return }
    let rimeIdx = lastDisplayedToRime[displayedIndex] + 1
    FrequencyTracker.shared.record(candidate: lastDisplayed[displayedIndex])
    let client = CandidateUI.shared.lastClient as? IMKTextInput
    _ = apply(RimeEngine.shared.selectDigit(rimeIdx), client: client)
  }

  /// Scroll wheel on the candidate panel — same key code as the keyboard page key.
  func candidatePageUpDown(up: Bool) {
    guard RimeEngine.shared.isComposing else { return }
    // 0xff55 = XK_Page_Up, 0xff56 = XK_Page_Down — matches `didCommand` pageDown/pageUp
    let code: Int32 = up ? 0xff55 : 0xff56
    let client = CandidateUI.shared.lastClient as? IMKTextInput
    _ = apply(RimeEngine.shared.processKey(code: code), client: client)
  }
}
