import Foundation

/// IME 文件日志（0.6.62）。
///
/// **为什么不用 `os_log` / `NSLog`**：2026-10-20 实测，系统日志里**查不到任何
/// DeskCapIME 记录**（`log show --predicate 'process == "DeskCapIME"' --info
/// --debug` 返回空），连启动那一行 `NSLog("server up")` 都没有。没有日志就
/// 无法验证候选框位置这类问题 —— 只能靠猜。
///
/// 这里改用**自己的文件**：路径固定、格式简单、能直接 `tail`，不依赖系统的
/// 隐私脱敏与 predicate 匹配。
///
/// 容量控制：单文件上限 256 KB，超了把它截成后半段重写（不是删掉重建，
/// 保留最近上下文）。写入用 `O_APPEND`，不反复重写整个文件 —— 不会磨损 SSD。
enum IMELog {
  private static let maxBytes = 256 * 1024
  private static let lock = NSLock()

  static var path: String {
    NSHomeDirectory() + "/Library/Application Support/DeskCap/logs/ime.log"
  }

  static func write(_ message: String) {
    lock.lock()
    defer { lock.unlock() }

    let dir = (path as NSString).deletingLastPathComponent
    try? FileManager.default.createDirectory(
      atPath: dir, withIntermediateDirectories: true)

    // 0.6.68 — 时间必须带时分秒。之前用 ISO8601DateFormatter 只设了
    // .withTimeZone/.withFractionalSeconds，没设 .withInternetDateTime，
    // 结果每行只有 "+0800"，完全判断不了时序（排查启动/崩溃时抓瞎）。
    let fmt = DateFormatter()
    fmt.dateFormat = "HH:mm:ss.SSS"
    let ts = fmt.string(from: Date())
    guard let data = "\(ts) \(message)\n".data(using: .utf8) else { return }

    let fd = open(path, O_CREAT | O_WRONLY | O_APPEND, 0o600)
    guard fd >= 0 else { return }
    // 必须写全 `Darwin.write` —— 本枚举的 `write(_:)` 会遮蔽 C 的同名函数。
    _ = data.withUnsafeBytes { p in
      Darwin.write(fd, p.baseAddress, data.count)
    }
    close(fd)

    trimIfNeeded()
  }

  /// 超过上限时保留后一半。用 `rename` + 重写，避免并发读到半截文件。
  private static func trimIfNeeded() {
    let attrs = try? FileManager.default.attributesOfItem(atPath: path)
    let size = (attrs?[.size] as? Int) ?? 0
    guard size > maxBytes else { return }
    guard let whole = try? String(contentsOfFile: path, encoding: .utf8) else { return }
    let half = whole.suffix(whole.count / 2)
    try? String(half).write(toFile: path, atomically: true, encoding: .utf8)
  }
}
