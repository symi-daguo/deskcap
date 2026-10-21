# macOS capture baseline (habit-first, no IME switch)

Measured from local day JSONL `2026-09-08` … `2026-09-12` (this machine).  
Principle: **do not replace the user’s input method**.

## Storage (day + session JSONL)

| Check | Result |
|-------|--------|
| Corrupt / unparsable lines (5 days) | **0** |
| Trailing truncated EOF | **none** |
| Live app | DeskCap **1.0.0** at `/Applications`, `/health` OK |
| Data dir size | **≈ 45 MB** (cap 1024 MB; retention 14d) |
| Process RSS | **≈ 57–110 MB** (+ IME ≈ 38 MB if used) |

Write path: buffered flush (~0.4–0.5s); flush on stop / quit / Agent read of today; import flushes before clearing sink; write errors logged + sink reopen. `kill -9` may still drop the last unflushed batch (no per-event fsync). Maintenance every 30 min enforces retention + `max_disk_mb`.

## Typing: CJK vs Latin-ish by app (2026-09-12 full day)

| App | typing segs | CJK % | Notes |
|-----|-------------|-------|--------|
| WorkBuddy / Hermes | 438 / 12 | ~96%+ | Good |
| 访达 / 多数系统 UI | 140 | ~94%+ | Good |
| 某些 Electron 编辑器 | 112 | ~91% | AX committed Chinese works well |
| WPS Office | 37 | ~65% | Mixed |
| Google Chrome | 26 | ~15% | Mixed page + IME |
| **微信** | **86** | **~3%** | Events captured; almost only pinyin Latin (expected) |
| Terminal | 12 | ~0% | Latin expected |

## Honest product line

- **Default path stays EventTap + AX** — no forced IME change.
- Apps where AX sees the composer → Chinese is already captured.
- WeChat (and similar) → keep pinyin pipeline + Agent time-window decode; not 100% Chinese.
- **1.0.17:** on Enter/click in WeChat, try deep AX then **send-time Vision OCR** of the composer strip (`ocr_commit`). Not always-on. If OCR/AX miss, still pinyin.
- **1.0.0 + IME 1.0.0（历史：1.0.19 + IME 0.6.29）:** optional DeskCap拼音 (`/ime/commit`) when user switches input source; default stays EventTap+AX (no forced switch). Candidate bar under caret. Menu icon =「亖」(macOS 27). Workbench lists size pages from container height. Enable/Add sheet still flaky — see `docs/ime-capture.md`.
- **IME now 1.0.0** (same product;采集路径 `/ime/commit` + `ime-active` 去重不变). 变化在候选框 placement、排序,以及**新增候选译词**(默认关闭),见 `docs/mac-ops.md`。
- Ops / legal checklist: `docs/mac-ops.md`.
