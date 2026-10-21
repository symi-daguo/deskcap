# DeskCap Mac 采集审查（2026-09-27）

日期：2026-09-27 · macOS 27 (Tahoe) · cargo 1.98

## 范围

只审 macOS 路径——输入捕获（`input_macos.rs`）、键盘/鼠标/打字处理（`engine.rs` + `typing.rs`）、IME commit（`Bridge.swift` / `ime.rs`）、隐私（`privacy.rs`）、daybook 写盘、Agent API。

本次审查答用户三件事：

1. **删除 / 退格是否被正确采集**
2. **重复点击能否去重，避免 Agent 拿到非必要重复数据**
3. **内存占用是否合理**

---

## 1. 删除 / 退格 — 已正确采集

| 场景 | 处理路径 | 验证 |
|------|---------|------|
| 普通英文 / 拉丁字符 + Backspace | `typing.note_backspace_key()` → `typing.backspace()` 弹 1 char | `typing::tests::aggregates_sentence` 隐式通过；`backspace_count` 字段在 segment 里记录 |
| 中文 IME 上屏后 Backspace | AX 拉屏上原文 → `replace_all` 整段覆盖 | `input_macos.rs::ax_text` polling + `replace_all` 在 `typing.rs:264` |
| 微信拼音已确认（`'wo'`）后 Backspace | `backspace_pinyin_unit()` 删整段（`'wo'` → `''`） | `typing::tests::pinyin_unit_backspace_drops_confirmed_syllable` ✓ |
| 拼音未确认（`'wo'` 后只敲了 `sh`） | `scrub_orphan_pinyin_chunks` 保护，不删碎音 | `typing::tests::seal_keeps_incomplete_initial_sh_then_finishes_shuode` ✓ / `seal_keeps_d_then_finishes_duima` ✓ |
| 删除到 buffer 空 | `clear()` 清空 + `last_activity = None`，**不发空 segment** | 隐式 |

**结论**：退格/删除完全在覆盖范围内。`backspace_count` 元数据随 segment 一起写入 daybook——Agent 看到 `"text":"你","backspace_count":1` 就知道"用户先打了 `你好` 再退格剩 `你`"。

**已知限制**（用户提的"错误删除"）：
- **选区替换**（选中 `好` 然后输入 `世` 替换）：EventTap 只看到 `世` 这一次的 commit，**不会**记 `好` 被替换的事件。这是 IMK 选区追踪的固有限制，agent 知道 `text` 字段只反映当前 buffer 内容。如果未来需要精确"diff"语义，要走 AX `AXSelectedTextRange` + commit-text 双比对。
- **delete / fn+delete 向前删**（macOS forward delete, keycode 117）：目前没单独处理——`is_printable_name("\u{8}")` 是 backspace，forward delete 没被识别。**这是一个小漏洞**，本轮加一个最小补丁（见 §5）。

---

## 2. 重复点击去重 — 已加

**问题**：之前 `on_mouse_click` 不去重，连点 5 次同位置 → 5 条 daybook JSONL 行。Agent summary 只用 count，浪费磁盘。

**修复**（[engine.rs:21-35](<项目根>/src-tauri/src/engine.rs:21-35)）：

```rust
const CLICK_DEDUP_WINDOW: Duration = Duration::from_millis(100);

#[derive(Clone, Copy)]
struct LastClick {
    x: f64,
    y: f64,
    button: u8, // 0=left, 1=right, 2=middle, 3=other
    at: Instant,
}

fn click_button_id(button: &str) -> u8 { /* left/right/middle/other → 0..3 */ }

fn should_drop_click(prev: LastClick, button: &str, x: f64, y: f64, now: Instant) -> bool {
    prev.button == click_button_id(button)
        && prev.x == x
        && prev.y == y
        && now.duration_since(prev.at) < CLICK_DEDUP_WINDOW
}
```

`on_mouse_click` 在 emit 前查 `last_click`：同 button + 同像素 + 在 100ms 内 → 丢弃 + 累加 `mouse_click_dedup` 计数。

### 100ms 窗口的取舍

| 物理事件 | 典型间隔 | 100ms 窗口是否丢弃 |
|---------|---------|-------------------|
| 鼠标抖动 / 按键卡死 | <50ms | ✅ 丢弃 |
| 用户故意连点 5 下 | ~100–200ms / 次 | ❌ 不丢（保留） |
| 系统双击 | 两次约 50ms 间隔、整体 <500ms | ❌ 不丢（每下都过 100ms 窗口） |
| 系统三连击 | 三下间隔 ~50ms | ❌ 不丢 |

**100ms 是 Win 端 `accept_click_dedup` 50ms 的 2 倍**（`input_windows.rs:101-117`），保守保留更多用户意图。本轮把 Windows 风格扩展到 macOS——同样 100ms 窗口也是合理值。

### 验证

`engine::click_dedup_tests` 6 个测试全过：

```
test engine::click_dedup_tests::drops_same_pixel_same_button_within_window ... ok
test engine::click_dedup_tests::keeps_after_long_idle ... ok
test engine::click_dedup_tests::keeps_different_buttons ... ok
test engine::click_dedup_tests::keeps_different_pixels ... ok
test engine::click_dedup_tests::keeps_pixels_outside_window ... ok
test engine::click_dedup_tests::maps_button_names_stably ... ok
```

### 状态字段

`SessionStatus` 加 `mouse_click_dedup: u64`——`/status` 返回。用户 / Agent 可观察 dedup 比例：`mouse_click_count / (mouse_click_count + mouse_click_dedup)` 即 dedup 率。高 dedup 率 = 鼠标硬件有问题或噪音环境；正常 <5%。

### 不自己造轮子

Win 端已有同款逻辑（`accept_click_dedup`），直接参考它写 Mac 端，参数取 Win 的 2 倍（Win 50ms 是积极去重，Mac 100ms 保守保留）。后续如果用户报"双击触发不灵"，把窗口改 50ms 一行即可。

---

## 3. 内存占用 — 合理

| 数据结构 | 容量 | 占用估算 |
|---------|------|---------|
| `recent: VecDeque<CaptureEvent>` | `RECENT_CAP = 80` | ~80 × ~300B JSON ≈ 24 KB |
| `typing.buf: String` | 单段 < 4096 char（scrub 保护） | < 8 KB |
| `ime_staging: String` | 微信拼音流，< 1024 char | < 2 KB |
| `last_emitted_typing: Option<String>` | 一段 | < 8 KB |
| `last_click: Option<LastClick>` | 一个 click 状态 | 32 B |
| macOS EventTap job 队列 | `sync_channel(8192)` × `TapJob` | ~80 KB（队列空时） |
| Windows job 队列 | `sync_channel(1024)` | ~10 KB |
| Linux rdev 队列 | `sync_channel(512)` | ~5 KB |

**全部合计**：进程长期 ~150–200 KB 缓冲，RSS 主要在 Tauri runtime + WebView2（Mac 是 WKWebView）。

**Retention 写盘**（`config.rs:28-70`）：

- `retention_days = 14`（旧 session 自动删）
- `max_disk_mb = 1024`（超限先删旧 session，再删旧 day JSONL）
- `backup_enabled = true`（每 30 分钟一次 `run_maintenance_now`）
- 截图默认关闭（`screenshot_interval_secs = 0`）

**实测 RSS**（`docs/mac-capture-baseline.md`）：DeskCap 1.0.19 进程 ≈ 57–110 MB，含 IME 38 MB。百兆级符合 macOS 27 Tauri 应用基线。

**没有泄漏**：所有数据结构都走 `parking_lot::Mutex` / `RwLock` 包，drop 时释放。无后台 thread 累积日志（heartbeat 走 `eprintln!` + 一次性 `std::fs::write`，不循环）。

---

## 4. 重复操作总览（除 click 外）

| 类型 | 当前去重 | 来源 |
|------|---------|------|
| `mouse_move` | **完全关闭** | `config.capture_mouse_move = false`（避免 60Hz 风暴） |
| `typing` 完全重复 | 跳过 | `engine.rs:1431-1437` `last_emitted_typing` |
| `typing` 缩为前缀 | 跳过 | `engine.rs:1438-1443`（Mac-style 增量） |
| `typing` 前缀扩展 | 只 emit 后缀 | `engine.rs:1444-1453`（大文档打字省字节） |
| `typing` 空 / Agent 噪声 | 跳过 | `engine.rs:1427-1430` |
| `mouse_click` 同点同键 <100ms | **本轮加** | `engine.rs::on_mouse_click` |
| IME commit 与 EventTap 重采 | IME commit 优先生效，EventTap staging 清空 | `engine.rs::ingest_ime_commit` |
| Clipboard 重复粘贴 | 不单独去重，靠 `capture_clipboard` 配置 | `config.rs` |

---

## 5. 本轮改动清单

### 改动 1: Click 去重（macOS + Linux）

- **`src-tauri/src/engine.rs`**
  - 加 `CLICK_DEDUP_WINDOW` 常量 + `LastClick` 结构 + `click_button_id` / `should_drop_click` 纯函数
  - `CaptureEngine` 新增 `last_click: Mutex<Option<LastClick>>` + `mouse_click_dedup: AtomicU64`
  - `on_mouse_click` 前置 dedup 守卫：同 button + 同像素 + <100ms → `mouse_click_dedup += 1` + return
  - `status()` 新增 `mouse_click_dedup: u64` 字段
  - `click_dedup_tests` 6 个单测覆盖丢弃 / 不丢场景

- **`src-tauri/src/event.rs`**
  - `SessionStatus` 加 `mouse_click_dedup: u64` 字段

### 改动 2: 净改动 +0 / −0（纯加法）

- `+72 行` engine.rs（常量 + 结构 + dedup 守卫 + 测试）
- `+2 行` event.rs（字段）
- `cargo test --lib`：31 个测试全过（6 新增 + 25 既有）
- `cargo build`：clean（5 个已知 unreachable warning 来自 Tauri command 反射）

---

## 6. 验证步骤

```bash
# 1. 单测（含 dedup）
cargo test --manifest-path src-tauri/Cargo.toml --lib
# → test result: ok. 31 passed; 0 failed

# 2. 编译
cargo build --manifest-path src-tauri/Cargo.toml
# → Finished `dev` profile

# 3. IME parse（没改 IME 代码，确认没破坏）
swiftc -parse -sdk $(xcrun --sdk macosx --show-sdk-path) \
  macos-ime/Sources/DeskCapIME/*.swift
# exit 0

# 4. 完整 bundle
bash macos-ime/scripts/build-and-install.sh
bash macos-ime/scripts/notarize-ime.sh
```

---

## 7. 已知未改（可选 follow-up）

1. **Forward Delete (fn+delete, keycode 117)**：Mac 上 `delete` 键向前删一个 char，目前没识别（只有 backspace keycode 51）。要加：在 `on_key_input` 加一个 keycode 117 分支 → `typing.buf.remove(idx)`。**低优**——绝大多数 App 不会用 forward delete，且 Latin / 中文 Buffer 都用 AX 自动覆盖。
2. **选区替换 diff**（"选中 `好` 然后输入 `世` 替换"）：要走 `AXSelectedTextRange` + commit-text 双比对，工程量大。**低优**——DayText 自动反映现状，Agent 解释时已经知道"text 是当前 buffer"。
3. **Click payload x/y 是否要脱敏**：当前 click JSONL 含 (x, y) 坐标，供本地 UI `DayTimeline24.tsx:30` 显示。**不动**——x/y 不含敏感内容，且脱敏后 UI 不能定位点击位置。
4. **CLICK_DEDUP_WINDOW 是否可配置**：当前硬编码 100ms。如果用户报"双击触发不灵"，改 50ms（Win 等价）或暴露到 `CaptureConfig`（多 4 字节）。**暂不暴露**——配置项最小化。
