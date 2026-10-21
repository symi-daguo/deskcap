import Cocoa
import InputMethodKit

private var gServer: IMKServer?

enum DeskCapIME {
  static var server: IMKServer? { gServer }
}

@main
enum DeskCapIMEMain {
  /// 0.6.49 — Apple 规定的 NSConnection 命名规范：`<bundle-id>_Connection`。
  /// 官方NumberInput 范例给的是错的（用了 `NumberInput_Connection`），
  /// 误导了全球开发者；不规范命名在开 Sandbox 后会导致切换输入法时无法载入
  /// （Console.app 里能看到 NSConnection 失败讯息）。
  /// 这里不再硬编码字面量，改成从 bundleId 推导，与 `Info.plist` 保持单一真源。
  static func connectionName(bundleId: String) -> String { "\(bundleId)_Connection" }

  /// 进程级单实例锁。`flock` 由内核维护，进程一退出（包括崩溃）就自动释放，
  /// 不会像 pid 文件那样留下"僵尸锁"把输入法永久卡死。
  private static var instanceLockFD: Int32 = -1
  /// 拿不到单实例锁时为 true：本进程**不加载 rime**，避免双进程抢 LevelDB。
  /// IMKServer 仍然注册，保证输入法"有反应"（透传字母）而不是彻底哑掉。
  private static var rimeDisabled = false

  private static func ensureSingleInstance() {
    let dir = NSHomeDirectory() + "/Library/Application Support/DeskCap"
    try? FileManager.default.createDirectory(
      atPath: dir, withIntermediateDirectories: true)
    let path = dir + "/.ime-instance.lock"
    let fd = open(path, O_CREAT | O_RDWR, 0o600)
    guard fd >= 0 else {
      NSLog("DeskCapIME: cannot open instance lock, continuing")
      return
    }
    if flock(fd, LOCK_EX | LOCK_NB) == 0 {
      instanceLockFD = fd   // 故意不 close：锁要活到进程结束
      return
    }

    // 0.6.67 — **绝对不能 exit**（0.6.66 的教训）。
    //
    // 装机/崩溃重启时，旧实例往往还没完全退出、仍持有 flock。上一版在这里
    // 直接 `exit(0)`，而 IMK **不会重试**已退出的 server —— 结果就是用户切到
    // DeskCap拼音后**完全打不出字**（2026-10-20 15:39 实测）。
    //
    // 正确做法：等它退（最多 2 秒，对方在收尾），真等不到就**降级启动**：
    // IMKServer 照常注册、英文/数字照常透传，只是暂时不加载 rime
    // （避免两个进程抢 LevelDB 用户词典把 librime 崩掉）。
    for _ in 0..<20 {
      usleep(100_000)   // 100ms × 20 = 2s
      if flock(fd, LOCK_EX | LOCK_NB) == 0 {
        instanceLockFD = fd
        IMELog.write("instance lock acquired after wait")
        return
      }
    }
    close(fd)
    rimeDisabled = true
    IMELog.write("instance lock busy after 2s — starting WITHOUT rime (safe mode)")
  }

  /// 崩溃自愈。
  ///
  /// 2026-10-20 实测：14:07 → 14:08 → 14:09 连续三次启动即崩，崩溃栈都在
  /// `UserDictionary::Load` → `LevelDb::Open` —— 说明**上一次崩溃把 LevelDB
  /// 用户词典写坏了**，`repairCorruptUserdb` 的 CURRENT 检查看不出这种损坏
  /// （文件在、内容坏），于是每次启动又崩在同一个地方，形成死循环。
  ///
  /// 判据：同一分钟内启动第二次 ⇒ 上一次没活过一分钟 ⇒ 视为崩溃。
  /// 处置：把 `.userdb` 目录**改名备份**（不删，可人工恢复）后重建空库。
  /// 代价是丢学习记录，换的是输入法能起来 —— 对用户来说这个交换永远值得。
  private static func recoverFromBootLoop() {
    let dir = NSHomeDirectory() + "/Library/Application Support/DeskCap"
    let bootPath = dir + "/.ime-last-boot"
    let now = Date().timeIntervalSince1970
    if let raw = try? String(contentsOfFile: bootPath, encoding: .utf8),
       let last = Double(raw.trimmingCharacters(in: .whitespacesAndNewlines)),
       now - last < 60 {
      let fm = FileManager.default
      let stamp = Int(now)
      var moved: [String] = []
      for name in ["luna_pinyin.userdb", "deskcap_pinyin.userdb"] {
        let src = dir + "/rime/" + name
        guard fm.fileExists(atPath: src) else { continue }
        let dst = src + ".bak-" + String(stamp)
        if (try? fm.moveItem(atPath: src, toPath: dst)) != nil {
          moved.append(name)
        }
      }
      if !moved.isEmpty {
        NSLog("DeskCapIME: boot-loop detected (<60s) — reset userdb: %@",
              moved.joined(separator: ","))
      } else {
        NSLog("DeskCapIME: boot-loop detected but no userdb to reset")
      }
    }
    try? String(now).write(toFile: bootPath, atomically: true, encoding: .utf8)
  }

  static func main() {
    // 0.6.61 — 单实例硬保护（崩溃根因修复）。
    //
    // librime 的 LevelDB 用户词典**不能被两个进程同时打开**：
    // `UserDictionary::Load` → `LevelDb::Open` → `LevelDbWrapper::Fetch`
    // 在拿不到锁时会解引用空指针 → SIGSEGV（2026-10-20 14:09 / 14:25
    // 各崩一次，崩溃报告栈顶就是 LevelDbWrapper::Fetch + Fetch）。
    //
    // 触发场景：装机/重启时旧进程尚未完全退出，系统已拉起新进程；
    // 或者 IMK 在切换输入法时短暂并存两个 server 实例。
    //
    // 处理：用 flock 抢一个进程级锁；抢不到说明已有实例在服务输入，
    // 本进程**直接退出**（系统会保留已在跑的那个）。锁随进程消亡自动释放。
    ensureSingleInstance()
    recoverFromBootLoop()
    RimeEngine.shared.safeMode = rimeDisabled

    let app = NSApplication.shared
    app.setActivationPolicy(.accessory)

    let bundleId = Bundle.main.bundleIdentifier ?? "com.office.deskcap.inputmethod.symi"
    let conn = Bundle.main.infoDictionary?["InputMethodConnectionName"] as? String
      ?? connectionName(bundleId: bundleId)

    gServer = IMKServer(name: conn, bundleIdentifier: bundleId)
    // Do NOT block launch on rime load — first keys would freeze ~5s after IME switch.
    RimeEngine.shared.warmUpAsync()
    // 0.6.47 — 译词表同样后台预热，避免首次出候选时在主线程读盘。
    DispatchQueue.global(qos: .userInitiated).async { Glossary.shared.warmUp() }
    IMELog.write("server up conn=\(conn) bundle=\(bundleId) v\(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "?")")
    NSLog("DeskCapIME server up conn=%@ bundle=%@", conn, bundleId)
    app.run()
  }
}
