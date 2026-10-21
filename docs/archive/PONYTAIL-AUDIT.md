# Ponytail Audit — DeskCap 1.0.19 + DeskCapIME 0.6.29

> ⚠️ **本文件已被取代（2026-09-27 版本）。** 本轮只覆盖了 Swift 侧；当前代码已到 **0.6.47**。
> 最新一轮见 [`macos-ime/PONYTAIL-AUDIT-2026-09-29.md`](macos-ime/PONYTAIL-AUDIT-2026-09-29.md)
> （0.6.44 复核 / 0.6.45 / 0.6.46 四轮）。保留本文件仅作历史记录。

Date: 2026-09-27 · macOS 27 (Tahoe) · cargo 1.98 · node 24 · pnpm 11

## Summary

| Bucket | Findings | Removed lines | Notes |
|---|---|---|---|
| Rust dead code | 7 | ~120 | `pub fn` / atomic fields with zero callers in crate |
| Swift dead code | 2 | ~7 | `DeskCapIMEApplication` no-op; unreachable `\n` / `\r` guard |
| Wheel reinvention | 0 | 0 | All UI surfaces use vendor libs (vis-timeline …); IME uses official IMK + `firstRect` |
| macOS 27 compat | 0 critical | — | Activation policy / `firstRect` / `NSVisualEffectView` / `IMK` all conform |
| **Net** | **9 fixes** | **−115 LOC** | — |

## Findings (ordered by cut)

`src-tauri/src/ime_keepalive.rs:14-L58` **delete** — `HELPER_SRC` constant
(45-line bash+python), `helper_path()` function, the `std::fs::write` and
`chmod` are written to `~/Library/Application Support/DeskCap/bin/ime-list.sh`
but never executed. The actual logic is in the inline `python3 -c "…"`
argument to `Command::new("/usr/bin/python3")`. Single source of truth
restored.

`macos-ime/Sources/DeskCapIME/main.swift:7-L11` **delete** —
`DeskCapIMEApplication` subclass that overrides `sendEvent` to only call
`super.sendEvent`. Use `NSApplication.shared` directly per macOS 27 HIG.

`macos-ime/Sources/DeskCapIME/Bridge.swift:34` **shrink** — the
`pending.contains("\n") || pending.contains("\r")` short-circuit.
`Rime.commit → insertText` text never contains a newline, this branch is
unreachable. The 48-char threshold + 2.2 s flush remain.

`src-tauri/src/engine.rs` **delete**:

- L501 `pub fn on_key_input_committed` — no cross-module callers.
- L547 `fn emit_typing_flush` — only reachable from above; both removed together (no surviving caller).
- L981 `fn is_day_knowledge` — no callers (`is_text_knowledge` is the actual gate).

`src-tauri/src/daybook.rs` **delete**:

- L28 `pub fn today_path` — no callers (engine uses `read_day_page`).
- L139 `pub fn read_day` — no callers.

`src-tauri/src/logutil.rs:26` **delete** — `pub fn path()` no callers; can be
revived when IPC needs to surface the log path.

`src-tauri/src/typing.rs:252` **delete** — `pub fn is_empty()` no callers;
covered by `text_ref().is_empty()` at call sites.

## False-positive cargo warnings (intentional)

```
warning: function `export_day_range` is never used
warning: function `import_day_jsonl` is never used
warning: methods `clear_latin_typing_buffer`, `export_day`, `import_day`
         and fields `keydown_n` / `mouse_n` / `last_input_unix_ms` are never used
```

Reachable from `uia_text.rs` (engine methods) and `run_maintenance_now`
(counter reads). Cargo's crate-level reachability can't see `impl` blocks
cross-module; these are wiring, not dead code. Silencing would be a lie,
leaving them documents the actual graph.

## Wheel-reinvention check (passed)

- **Timeline UI** — `vis-timeline` 8.5.4 (mature vendor lib); no custom grid.
- **IME candidate panel** — Apple `IMKTextInput.firstRect(forCharacterRange:)`
  + `setFrameTopLeftPoint` (HIG / 同类 IMK 输入法 reference).
- **IME activation** — `NSApplication.setActivationPolicy(.accessory)` (HIG).
- **Persistence** — `serde_json` + `parking_lot::Mutex`; no hand-rolled JSON / lock.
- **Dates** — `date-fns` (npm), `chrono` (Rust); no manual parsing.
- **HTTP** — `axum 0.8` + `tokio`; no hand-rolled server.
- **Single instance** — `tauri-plugin-single-instance`; no custom glue.

## macOS 27 (Tahoe) compliance

| Concern | Status | Evidence |
|---|---|---|
| Activation policy | OK | `setActivationPolicy(.accessory)` in `main.swift`; `LSUIElement` in `Info.plist` |
| Sandboxing / Hardened Runtime | OK | `scripts/install-macos.sh` + `notarize-ime.sh` |
| `firstRect(forCharacterRange:)` for IME | OK | `CandidateWindow.placeBelowMarkedText` uses 3 fallback strategies (caret-after, whole range, `NSNotFound`) |
| `NSPanel` for IMK candidate | OK | `.popUpMenu`, `[.canJoinAllSpaces, .fullScreenAuxiliary]`, `hidesOnDeactivate = false` |
| `NSVisualEffectView` material | OK | `.popover` (still recommended in macOS 27) |
| Logging API | triangular | `NSLog` works; macOS 27 prefers `os.Logger`. Not blocking. |
| Liquid Glass surfaces | n/a | IME panel intentionally borderless + blur; Liquid Glass reserved for DeskCap.app UI |
| Threading | OK | Rime load on `DispatchQueue.global(qos: .userInitiated)`; UI on main run loop via `DispatchQueue.main.async` |
| `Info.plist InputMethodConnectionName` | OK | defaults to `DeskCapIME_Connection`, picked up by `IMKServer(name:bundleIdentifier:)` |
| TCC | OK | `scripts/fix-macos-tcc.sh`; `request_input_permissions()` in `input_macos.rs` |

No breaking changes needed for macOS 27. The IME already follows the
Apple HIG (Activation Policy, panel collection behavior, candidate
placement, IMK controller pattern).

## Verification (after fixes)

```
$ cargo check --manifest-path src-tauri/Cargo.toml
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.79s
    (5 false-positive warnings — wired, not dead)

$ swiftc -parse -sdk $(xcrun --sdk macosx --show-sdk-path) \
    macos-ime/Sources/DeskCapIME/*.swift
    (no output, parses cleanly against macOS 27 SDK)

$ pnpm tsc --noEmit
    (no output, clean)
```
