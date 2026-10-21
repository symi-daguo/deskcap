import Foundation

/// Thin Swift wrapper around Homebrew librime (`rime_get_api`).
final class RimeEngine {
  struct State {
    var preedit: String = ""
    var commit: String?
    var candidates: [String] = []
    var highlighted: Int = 0
    var page: Int = 0
    var pageCount: Int = 1
  }

  static let shared = RimeEngine()

  private var api: UnsafeMutablePointer<RimeApi>?
  private var session: RimeSessionId = 0
  private var ready = false
  private(set) var isComposing = false

  private init() {}

  private let warmLock = NSLock()
  private var warming = false

  /// Non-blocking: kick off rime load on a background queue.
  func warmUpAsync() {
    warmLock.lock()
    if ready || warming {
      warmLock.unlock()
      return
    }
    warming = true
    warmLock.unlock()
    DispatchQueue.global(qos: .userInitiated).async { [weak self] in
      self?.ensureReady()
      self?.warmLock.lock()
      self?.warming = false
      self?.warmLock.unlock()
      NSLog("DeskCapIME: rime warm-up done")
    }
  }

  /// 安全模式：另一个实例持有 userdb 锁时由 main.swift 置位。
  /// 此时不加载 rime（避免双进程抢 LevelDB 把 librime 崩掉），
  /// 输入法仍能注册、字母仍能透传 —— 宁可退化，也不能"完全打不出字"。
  var safeMode = false

  func ensureReady() {
    if safeMode { return }
    if ready { return }
    let home = NSHomeDirectory() + "/Library/Application Support/DeskCap/rime"
    try? FileManager.default.createDirectory(atPath: home, withIntermediateDirectories: true)

    guard let shared = Bundle.main.resourcePath.map({ $0 + "/rime-data" }),
          FileManager.default.fileExists(atPath: shared) else {
      IMELog.write("rime: missing bundled rime-data (resourcePath=\(Bundle.main.resourcePath ?? "nil"))")
      return
    }
    IMELog.write("rime: ensureReady begin home=\(home) shared=\(shared)")
    seedUserOnly(home: home, shared: shared)
    repairCorruptUserdb(home: home)
    IMELog.write("rime: seed+repair done")

    guard let raw = rime_get_api() else {
      IMELog.write("rime: rime_get_api failed")
      return
    }
    api = raw
    var traits = RimeTraits()
    memset(&traits, 0, MemoryLayout<RimeTraits>.size)
    traits.data_size = Int32(MemoryLayout<RimeTraits>.size - MemoryLayout<Int32>.size)

    home.withCString { userPtr in
      shared.withCString { sharedPtr in
        "DeskCap".withCString { distName in
          "deskcap".withCString { codeName in
            "0.6".withCString { ver in
              "rime.deskcap".withCString { app in
                traits.shared_data_dir = sharedPtr
                traits.user_data_dir = userPtr
                traits.distribution_name = distName
                traits.distribution_code_name = codeName
                traits.distribution_version = ver
                traits.app_name = app
                IMELog.write("rime: setup+initialize begin")
                raw.pointee.setup(&traits)
                raw.pointee.initialize(&traits)
                IMELog.write("rime: setup+initialize done")
                // Incremental maintenance only — full check every launch blocks switch for seconds.
                _ = raw.pointee.start_maintenance(False)
                IMELog.write("rime: maintenance begin")
                raw.pointee.join_maintenance_thread()
                IMELog.write("rime: maintenance done")
              }
            }
          }
        }
      }
    }
    session = raw.pointee.create_session()
    ready = session != 0
    IMELog.write("rime: create_session -> \(session)")
    IMELog.write("rime ready=\(ready ? 1 : 0) session=\(session)")
  }

  /// Copy schema yaml into the user dir. Re-seeds when the bundled `_schema_version`
  /// differs from the user's (so a new build's fuzzy / hot-word / symbol updates reach
  /// existing installs automatically). User-level data is preserved:
  /// `luna_pinyin.userdb` / `deskcap_pinyin.userdb` and `deskcap_freq.json` are
  /// NEVER in the delete list. `.bin` caches under `build/` are dropped so librime
  /// rebuilds them against the new yaml on next maintenance.
  private func seedUserOnly(home: String, shared: String) {
    let fm = FileManager.default
    let yamlNames = [
      "default.yaml", "deskcap_pinyin.schema.yaml", "luna_pinyin.dict.yaml",
      "deskcap_hot.dict.yaml", "punctuation.yaml", "key_bindings.yaml",
      "essay.txt", "symbols.yaml", "pinyin.yaml",
    ]
    let versionFile = "_schema_version"
    let bundledVer = readTrimmed(shared + "/" + versionFile)
    let userVer = readTrimmed(home + "/" + versionFile)
    let needsReseed = !bundledVer.isEmpty && bundledVer != userVer
    if needsReseed {
      for name in yamlNames {
        try? fm.removeItem(atPath: home + "/" + name)
      }
      let buildDir = home + "/build"
      if fm.fileExists(atPath: buildDir) {
        try? fm.removeItem(atPath: buildDir)
      }
      try? bundledVer.write(toFile: home + "/" + versionFile, atomically: true, encoding: .utf8)
      IMELog.write("rime: schema bumped \(userVer) -> \(bundledVer) (userdb preserved)")
    }
    for name in yamlNames {
      let src = shared + "/" + name
      let dst = home + "/" + name
      guard fm.fileExists(atPath: src) else { continue }
      if fm.fileExists(atPath: dst) { continue }
      try? fm.copyItem(atPath: src, toPath: dst)
    }
    let ocSrc = shared + "/opencc"
    let ocDst = home + "/opencc"
    if fm.fileExists(atPath: ocSrc), !fm.fileExists(atPath: ocDst) {
      try? fm.copyItem(atPath: ocSrc, toPath: ocDst)
    }
  }

  /// Read a text file and trim whitespace; returns "" when the file is missing.
  private func readTrimmed(_ path: String) -> String {
    guard let s = try? String(contentsOfFile: path, encoding: .utf8) else { return "" }
    return s.trimmingCharacters(in: .whitespacesAndNewlines)
  }

  /// LevelDB userdb can break (CURRENT → missing MANIFEST) → rime stalls/errors.
  private func repairCorruptUserdb(home: String) {
    let fm = FileManager.default
    for name in ["luna_pinyin.userdb", "deskcap_pinyin.userdb"] {
      let dir = home + "/" + name
      guard fm.fileExists(atPath: dir) else { continue }
      let currentPath = dir + "/CURRENT"
      var broken = !fm.fileExists(atPath: currentPath)
      if !broken,
         let cur = (try? String(contentsOfFile: currentPath, encoding: .utf8))?
           .trimmingCharacters(in: .whitespacesAndNewlines),
         !cur.isEmpty,
         !fm.fileExists(atPath: dir + "/" + cur) {
        broken = true
      }
      if broken {
        try? fm.removeItem(atPath: dir)
        NSLog("DeskCapIME: reset corrupt userdb \(name)")
      }
      try? fm.removeItem(atPath: dir + "/LOCK")
    }
  }

  func processAscii(_ ch: String) -> State {
    ensureReady()
    guard ready, let api, let scalar = ch.unicodeScalars.first else { return State() }
    // Only touch ascii_mode when actually stuck — avoid per-key session rebuild (switch lag).
    if CharacterSet.lowercaseLetters.contains(scalar), isAsciiMode {
      ensureChineseMode()
    }
    _ = api.pointee.process_key(session, Int32(scalar.value), 0)
    return snapshot()
  }

  var isAsciiMode: Bool {
    ensureReady()
    guard ready, let api else { return false }
    var status = RimeStatus()
    memset(&status, 0, MemoryLayout<RimeStatus>.size)
    status.data_size = Int32(MemoryLayout<RimeStatus>.size - MemoryLayout<Int32>.size)
    if api.pointee.get_status(session, &status) == 0 { return false }
    let ascii = status.is_ascii_mode != 0
    _ = api.pointee.free_status(&status)
    return ascii
  }

  func setAsciiMode(_ on: Bool) {
    ensureReady()
    guard ready, let api else { return }
    api.pointee.set_option(session, "ascii_mode", on ? 1 : 0)
  }

  /// Recover to Chinese composition. Cheap no-op when already Chinese.
  func ensureChineseMode() {
    ensureReady()
    guard ready, let api else { return }
    if !isAsciiMode {
      return
    }
    api.pointee.set_option(session, "ascii_mode", 0)
    // Rebuild only if still stuck after set_option.
    if isAsciiMode {
      _ = api.pointee.destroy_session(session)
      session = api.pointee.create_session()
      api.pointee.set_option(session, "ascii_mode", 0)
      NSLog("DeskCapIME: rebuilt session after stuck ascii_mode")
    }
    isComposing = false
  }

  func processBackspace() -> State {
    ensureReady()
    guard ready, let api else { return State() }
    _ = api.pointee.process_key(session, 0xff08, 0)
    return snapshot()
  }

  func currentPreedit() -> String {
    ensureReady()
    guard ready, let api else { return "" }
    var ctx = RimeContext()
    memset(&ctx, 0, MemoryLayout<RimeContext>.size)
    ctx.data_size = Int32(MemoryLayout<RimeContext>.size - MemoryLayout<Int32>.size)
    guard api.pointee.get_context(session, &ctx) != 0 else { return "" }
    let p = ctx.composition.preedit.map { String(cString: $0) } ?? ""
    _ = api.pointee.free_context(&ctx)
    return p
  }

  func selectFirstOrSpace() -> State {
    ensureReady()
    guard ready, let api else { return State() }
    _ = api.pointee.process_key(session, 0x0020, 0)
    return snapshot()
  }

  func selectDigit(_ d: Int) -> State {
    ensureReady()
    guard ready, let api, d >= 1, d <= 9 else { return State() }
    _ = api.pointee.process_key(session, Int32(Character(String(d)).asciiValue ?? 0), 0)
    return snapshot()
  }

  func processKey(code: Int32) -> State {
    ensureReady()
    guard ready, let api else { return State() }
    _ = api.pointee.process_key(session, code, 0)
    return snapshot()
  }

  func cancel() -> State {
    ensureReady()
    guard ready, let api else { return State() }
    // XK_Escape = 0xff1b
    _ = api.pointee.process_key(session, 0xff1b, 0)
    return snapshot()
  }

  private func snapshot() -> State {
    var st = State()
    guard let api else { return st }

    var commit = RimeCommit()
    memset(&commit, 0, MemoryLayout<RimeCommit>.size)
    commit.data_size = Int32(MemoryLayout<RimeCommit>.size - MemoryLayout<Int32>.size)
    if api.pointee.get_commit(session, &commit) != 0 {
      if let cstr = commit.text {
        st.commit = String(cString: cstr)
      }
      _ = api.pointee.free_commit(&commit)
    }

    var ctx = RimeContext()
    memset(&ctx, 0, MemoryLayout<RimeContext>.size)
    ctx.data_size = Int32(MemoryLayout<RimeContext>.size - MemoryLayout<Int32>.size)
    if api.pointee.get_context(session, &ctx) != 0 {
      if let p = ctx.composition.preedit {
        st.preedit = String(cString: p)
      }
      let menu = ctx.menu
      let n = Int(menu.num_candidates)
      st.page = Int(menu.page_no)
      if n > 0, let cand = menu.candidates {
        for i in 0..<n {
          let text = cand[i].text.map { String(cString: $0) } ?? ""
          if !text.isEmpty { st.candidates.append(text) }
        }
        st.highlighted = Int(menu.highlighted_candidate_index)
        // Approximate page count from is_last_page + current page.
        st.pageCount = (menu.is_last_page != 0) ? (st.page + 1) : (st.page + 2)
      }
      _ = api.pointee.free_context(&ctx)
    }
    isComposing = !st.preedit.isEmpty || !st.candidates.isEmpty
    return st
  }
}
