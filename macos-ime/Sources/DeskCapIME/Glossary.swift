import Foundation

/// 候选译词（gloss）—— 在候选词下方显示目标语言释义，让语言学习自然发生在日常输入里。
///
/// **来源与许可**：交互思路（候选旁显示一条释义、一次只显示一种语言、未命中就不显示）
/// 来自公开通行做法。本文件为 DeskCap **独立实现**，未复制任何第三方源码或词表数据。
///
/// **词表格式**：TSV，每行 `词<TAB>释义`；`#` 开头为注释行；UTF-8。
///
/// **查找顺序**（后加载的覆盖先加载的）：
///   1. App bundle：`Contents/Resources/rime-data/glossary.<lang>.tsv`（随包种子）
///   2. 用户目录：`~/Library/Application Support/DeskCap/rime/glossary.<lang>.tsv`
/// 用户目录优先生效 —— 扩充词表**不需要改代码、不需要重新打包**。
///
/// **语言切换**（无需重启 IME，候选刷新时实时读取）：
/// ```sh
/// defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string en
/// # 取值：off（默认，不显示）/ en / ja / es / … 任何 glossary.<lang>.tsv 的 lang 段
/// ```
final class Glossary {

  static let shared = Glossary()

  /// 释义最大显示长度（超出截断）。防止一条长释义把候选条撑到屏幕外。
  private static let maxGlossChars = 12   // 0.6.70 — 单行排列后缩短，5 个 chip 总宽控制在屏幕内
  /// 逐字合成时最多取前 N 个字，避免 4 字词组拼出 4 段噪音。
  private static let maxComposedChars = 4

  private let lock = NSLock()
  /// lang -> 词表。空字典代表"该语言没有可用词表"（负缓存，避免每键都读盘）。
  private var cache: [String: [String: String]] = [:]
  /// `UserDefaults.string(forKey:)` 的同步读缓存。`lookup` 每键调 9 次，
  /// 不缓存等于每敲一个字母放大成一次 plist 读。只在 `invalidate()` 时失效。
  private var cachedLang: String?

  private init() {}

  /// 当前学习语言。`off` / 空 / 未设置 → 不显示释义（保持 0.6.46 原有候选条外观）。
  var language: String {
    let raw = UserDefaults.standard
      .string(forKey: "GlossaryLanguage")?
      .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    return raw.isEmpty ? "off" : raw
  }

  /// 查一个候选词的释义；查不到返回 `nil`，调用方据此保持候选条高度不变。
  ///
  /// 两级查找：
  /// 1. **整词精确命中** —— 词表里有多字词就直接给整体释义。
  /// 2. **逐字合成** —— 整词没收录时，把每个字的释义用 ` · ` 拼起来。
  ///    对语言学习反而更有用：`停下来` → `stop · come · down`（先认字，再认词）。
  ///    只要有一个字查不到就整体返回 `nil` —— 宁可空白，也不给半截噪音。
  ///
  /// 0.6.48 修 — `entries(for:)` 是**每键都调**的热路径（每次出候选 × 9 条）。
  /// `language` 每次都读 `UserDefaults`。两者都不缓存，就等于把用户每敲一个
  /// 拼音字母放大成一次 plist 读 + 一次 `String(contentsOf:)` 判断存在性。
  /// 这里把 language 也缓存住，只在 `invalidate()` 时失效。
  func lookup(_ word: String) -> String? {
    let lang = cachedLanguage()
    guard lang != "off", !word.isEmpty else { return nil }
    guard let table = entries(for: lang) else { return nil }

    if let direct = table[word] { return Self.bounded(direct) }

    // 单字查不到即无解。
    guard word.count > 1, word.count <= Self.maxComposedChars else { return nil }
    var parts: [String] = []
    parts.reserveCapacity(word.count)
    for ch in word {
      guard let g = table[String(ch)] else { return nil }
      parts.append(g)
    }
    return Self.bounded(parts.joined(separator: " · "))
  }

  /// 词表规模上限。`deskcap_freq.json` 侧已用 LRU 卡在 10 000 条；词表没有天然
  /// 上限，用户往 `glossary.<lang>.tsv` 里贴大词库时会一路读进内存。
  /// 50 000 条约 3–5 MB（Swift `String` + 字典开销），远低于 IME 进程可接受上限，
  /// 同时足够覆盖任何现实学习词表。超出的行直接丢弃并在启动时打一行日志。
  private static let maxEntries = 50_000

  /// 当前学习语言，同步在 `language` 之后使用（热路径，不读 UserDefaults）。
  private func cachedLanguage() -> String {
    lock.lock()
    if let cached = cachedLang {
      lock.unlock()
      return cached
    }
    lock.unlock()
    let raw = UserDefaults.standard
      .string(forKey: "GlossaryLanguage")?
      .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    let resolved = raw.isEmpty ? "off" : raw
    lock.lock()
    cachedLang = resolved
    lock.unlock()
    return resolved
  }

  /// 预热：在后台把当前语言的词表读进缓存，避免首次出候选时在主线程读盘卡一下。
  func warmUp() {
    _ = cachedLanguage()
    _ = entries(for: cachedLanguage())
  }

  /// 丢弃缓存。换词表文件后调用；下次 `lookup` 会重新读盘。
  func invalidate() {
    lock.lock()
    cache.removeAll()
    cachedLang = nil
    lock.unlock()
  }

  /// 取（并缓存）某语言的词表；该语言无数据返回 `nil`。
  private func entries(for lang: String) -> [String: String]? {
    lock.lock()
    if let cached = cache[lang] {
      lock.unlock()
      return cached.isEmpty ? nil : cached
    }
    lock.unlock()

    // 读盘在锁外：同语言并发 miss 会重复读一次，但结果相同，无副作用。
    var merged: [String: String] = [:]
    for url in [Self.bundleURL(lang: lang), Self.userURL(lang: lang)] {
      guard let url, let text = try? String(contentsOf: url, encoding: .utf8) else { continue }
      if merged.count < Self.maxEntries {
        Self.parse(text, into: &merged)
      } else {
        NSLog("DeskCapIME glossary lang=%@ 已达上限 %d 条，跳过 %@（防止内存被撑爆）",
              lang, Self.maxEntries, url.lastPathComponent)
      }
    }

    lock.lock()
    cache[lang] = merged
    lock.unlock()
    return merged.isEmpty ? nil : merged
  }

  /// 截断到 `maxGlossChars`，超长加省略号。
  private static func bounded(_ s: String) -> String {
    guard s.count > maxGlossChars else { return s }
    return String(s.prefix(maxGlossChars - 1)) + "…"
  }

  private static func bundleURL(lang: String) -> URL? {
    Bundle.main.url(
      forResource: "glossary.\(lang)", withExtension: "tsv", subdirectory: "rime-data"
    )
  }

  private static func userURL(lang: String) -> URL? {
    let url = FileManager.default.homeDirectoryForCurrentUser
      .appendingPathComponent("Library/Application Support/DeskCap/rime", isDirectory: true)
      .appendingPathComponent("glossary.\(lang).tsv")
    return FileManager.default.fileExists(atPath: url.path) ? url : nil
  }

  /// 解析 TSV。容错：坏行跳过（缺 TAB、空值、纯空白都不进表）。
  /// 0.6.50 修 — 加逐行上限：`maxEntries` 之前只在 `parse` **调用前**判一次，
  /// 于是「整文件读进来 → 每行都 parse 进 out → 已经 6 万条了才停」，超出的
  /// 那部分白占内存。改成 parse 内部逐行卡上限，一旦 `out.count >= maxEntries`
  /// 立即 return，后面的行不再处理。也顺带防住「单文件本身远超 5 万行」时
  /// 把字典撑爆（用户贴一份 50 万行词库，parse 阶段就止住，而不是读完全部）。
  static func parse(_ text: String, into out: inout [String: String], max: Int = Glossary.maxEntries) {
    for rawLine in text.split(separator: "\n", omittingEmptySubsequences: true) {
      if out.count >= max { return }
      let line = rawLine.trimmingCharacters(in: CharacterSet(charactersIn: " \t\r"))
      guard !line.isEmpty, !line.hasPrefix("#") else { continue }
      guard let tab = line.firstIndex(of: "\t") else { continue }
      let key = String(line[line.startIndex..<tab])
        .trimmingCharacters(in: .whitespaces)
      let value = String(line[line.index(after: tab)...])
        .trimmingCharacters(in: .whitespaces)
      guard !key.isEmpty, !value.isEmpty else { continue }
      out[key] = value
    }
  }
}
