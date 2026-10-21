# DeskCap IME (macOS InputMethodKit + librime)

Captures **committed text** (`insertText`) → `POST /ime/commit` to DeskCap.

## Current (1.0.0)

> 0.6.48 fixes（macOS 27 真机编译验证后）：
> - **[P0]** 选中项的译词补`backgroundColor` —— 修前是「蓝底之外的白字」
>   （对比度约 1.3:1，远低于 WCAG AA 的 4.5:1，实质读不出来）。现整块高亮，
>   与 macOS 27 系统拼音一致
> - **[P1]** 面板宽度上限 960 → **1600pt** —— 译词让每条候选多一行，
>   `min(960, fittingSize)` 会把第 3 条以后的候选截出可视区，且 `candRects`
>   仍按未截断布局缓存，导致点击命中区与可见区错位
> - **[P1]** `Glossary` 缓存 `GlossaryLanguage` —— `lookup` 每键调 9 次，
>   修前每键都同步读 `UserDefaults` + `fileExists`，是输入延迟的常见来源
> - **[P2]** 词表规模上限 **50 000 条**（约 3–5 MB）—— 修前无上限，
>   用户塞大词库会一路读进内存。超出时跳过整个文件并打日志
>
> 0.6.47 新增：**候选译词**（gloss）—— 候选词下方显示目标语言释义。
> `Glossary.swift` 独立实现，**未引入任何第三方 GPL 源码或词表数据**；
> 交互思路（候选旁一条释义 / 一次一种语言 / 未命中不显示）取自公开通行做法。
> **默认关闭**（`GlossaryLanguage=off`），需显式开启：
> ```sh
> defaults write com.office.deskcap.inputmethod.symi GlossaryLanguage -string en
> ```
> 词表 `rime-data/glossary.en.tsv`（240 单字 + 115 词组）由 `scripts/gen-glossary.py`
> 生成；用户目录 `~/Library/Application Support/DeskCap/rime/glossary.en.tsv`
> **优先生效且不会被重新打包覆盖** —— 扩充词表无需改代码。
> 查找两级：整词精确命中 → 逐字 ` · ` 合成（任一字查不到则不显示，宁缺毋滥）。
> 支持任意 `glossary.<lang>.tsv`（`ja` / `es` / …），换一个文件名即可切换语言。

> 0.6.46 fixes: 删 0.6.45 的窗口高度半阈值 heuristic flip（`hostWindow(of:)` 返 nil 时仍把 top-left 当 bottom-left 用 → 候选框永在屏幕左上角），改为 同类 IMK 输入法 式**纯信任 host `attributes(...)`** + `NSScreen.screens.contains` 兜底 + `applyRect` 严格 clamp；30ms 重试阈值 20pt → **50pt**（抗 Chromium IPC 同步抖动）。

> 0.6.45 fixes: `makeScreenRect` / `hostWindow(of:)` 引入；phraseBoost 与 preedit 音节数**解耦** —— 只要候选 ≥2 字恒得 `+1_000_000`，多字永远压单字（修「tingxia 输给单字听」）。

> 0.6.41 起：首个候选永远高亮（fresh input → display[0]）+ placement 0 偏移 + fuzzy 走上游 `pinyin.yaml` + FrequencyTracker debounce+LRU+flush + schema `_schema_version` 自动 reseed。详 `IME-FIX-2026-09-27.md` 和 `PONYTAIL-AUDIT-2026-09-29.md`

> ✅ **0.6.48 已真机验证**（2026-10-14）：Developer ID 签名有效、装机 0.6.48/build 69、译词 `GlossaryLanguage=en` 生效、采集四级 API + `/ime/commit` 汉字落库全通。证据见 `docs/mac-verify-0.6.47.md` 文末。**候选框交互观感仍待肉眼确认**。

审计（历史，已归档）：`docs/archive/PONYTAIL-AUDIT-2026-10-02.md` 与 `docs/archive/PONYTAIL-AUDIT-2026-09-29.md`。

## 历史快照 (0.6.29)

| Feature | Status |
|---------|--------|
| 候选条 | 组词末尾**正下方**；Electron 顶左 Y 自动择优；开头 rect 则右移 X |
| 图标 | 「**亖**」 |
| 去重 | `ime-active` + `/ime/commit` |
| Developer ID + 公证 | 是 |

运维：`docs/mac-ops.md`。

## Install

```bash
bash macos-ime/scripts/build-and-install.sh && bash macos-ime/scripts/notarize-ime.sh
osascript -e 'do shell script "rm -rf \"/Library/Input Methods/DeskCapIME.app\" && ditto \"macos-ime/build/DeskCapIME.app\" \"/Library/Input Methods/DeskCapIME.app\"" with administrator privileges'
```
