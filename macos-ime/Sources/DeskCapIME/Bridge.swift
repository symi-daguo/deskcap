import Foundation
import AppKit

/// Batch IME commits so one sentence/phrase group hits DeskCap as a single typing event.
enum DeskCapBridge {
  private static let lock = NSLock()
  private static var pending = ""
  private static var timer: Timer?
  private static var lastApp: String?

  // Idempotent flag mirror: avoid repeating the filesystem write when the host AppKit
  // toggles activateServer/deactivateServer in a tight loop (every 1–10 ms in some hosts).
  private static var imeActiveFlag = false
  private static let ioQueue = DispatchQueue(label: "deskcap.ime.bridge.io", qos: .utility)

  private static var activeFlagURL: URL {
    let base = FileManager.default.homeDirectoryForCurrentUser
      .appendingPathComponent("Library/Application Support/DeskCap", isDirectory: true)
    try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
    return base.appendingPathComponent("ime-active")
  }

  /// DeskCap EventTap skips Latin staging while this flag exists (avoid duplicate pinyin).
  /// Idempotent + async I/O so an activate/deactivate storm does not stall the IME controller.
  static func setImeActive(_ on: Bool) {
    if imeActiveFlag == on { return }
    imeActiveFlag = on
    let url = activeFlagURL
    ioQueue.async {
      if on {
        try? "1".write(to: url, atomically: true, encoding: .utf8)
      } else {
        try? FileManager.default.removeItem(at: url)
      }
    }
  }

  /// Called after each rime commit (word / phrase).
  static func postCommit(text: String) {
    lock.lock()
    if pending.isEmpty {
      lastApp = NSWorkspace.shared.frontmostApplication?.localizedName
    }
    pending += text
    // rime `insertText` never contains a newline — only the pause/48-char threshold matters.
    let shouldFlushNow = pending.count >= 48
    lock.unlock()
    if shouldFlushNow {
      flushNow()
    } else {
      scheduleFlush()
    }
  }

  /// Enter / deactivate / long pause — send whatever is buffered.
  static func flushNow() {
    lock.lock()
    let text = pending
    pending = ""
    timer?.invalidate()
    timer = nil
    let app = lastApp ?? NSWorkspace.shared.frontmostApplication?.localizedName
    lastApp = nil
    lock.unlock()
    guard !text.isEmpty else { return }
    send(text: text, app: app)
  }

  private static func scheduleFlush() {
    lock.lock()
    timer?.invalidate()
    // Coalesce rapid word commits (真的 + 是 + 可以了) into one segment.
    let t = Timer.scheduledTimer(withTimeInterval: 2.2, repeats: false) { _ in
      flushNow()
    }
    timer = t
    lock.unlock()
  }

  /// 0.6.59 — 专用 session：本机回环 3 秒足够；`URLSession.shared` 默认 60s，
  /// DeskCap 未运行时会让一次 commit 在后台挂一分钟（多次堆积还会拖慢 IME）。
  private static let session: URLSession = {
    let cfg = URLSessionConfiguration.ephemeral
    cfg.timeoutIntervalForRequest = 3
    cfg.timeoutIntervalForResource = 6
    cfg.waitsForConnectivity = false
    return URLSession(configuration: cfg)
  }()

  /// 重试预算：DeskCap 重启 / 装机期间端口短暂不可用，重试 2 次即可覆盖
  /// （间隔 0.4s、0.9s）。**4xx 不重试** —— token 错、请求非法时重试没有意义。
  private static let maxAttempts = 3

  private static func send(text: String, app: String?) {
    let tokenPath = NSHomeDirectory() + "/Library/Application Support/DeskCap/agent.token"
    guard let token = try? String(contentsOfFile: tokenPath, encoding: .utf8)
      .trimmingCharacters(in: .whitespacesAndNewlines),
      !token.isEmpty
    else {
      NSLog("DeskCapIME: missing agent.token")
      return
    }
    post(text: text, app: app, token: token, attempt: 1)
  }

  private static func post(text: String, app: String?, token: String, attempt: Int) {
    guard let url = URL(string: "http://127.0.0.1:8765/ime/commit") else { return }
    var req = URLRequest(url: url)
    req.httpMethod = "POST"
    req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    req.setValue("application/json", forHTTPHeaderField: "Content-Type")
    let body: [String: Any] = [
      "text": text,
      "app": app as Any,
      "source": "ime_commit",
    ]
    req.httpBody = try? JSONSerialization.data(withJSONObject: body)
    session.dataTask(with: req) { _, resp, err in
      let code = (resp as? HTTPURLResponse)?.statusCode ?? -1
      // 成功（2xx）：静默。失败只打日志，下一轮重试。
      if err == nil, (200...299).contains(code) { return }
      if let err {
        NSLog("DeskCapIME commit error attempt=%d: %@", attempt, err.localizedDescription)
      } else {
        NSLog("DeskCapIME commit status=%d attempt=%d chars=%d", code, attempt, text.count)
      }
      // 4xx = 请求本身有问题（token 错等），重试无用。
      let clientError = (400...499).contains(code)
      guard !clientError, attempt < maxAttempts else { return }
      let delay = attempt == 1 ? 0.4 : 0.9
      DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + delay) {
        post(text: text, app: app, token: token, attempt: attempt + 1)
      }
    }.resume()
  }
}
