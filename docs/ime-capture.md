# Mac Chinese path: capture the IME (not WeChat UI)

## Log verdict (2026-09-11)

| Fact | Evidence |
|------|----------|
| 1.0.17 running | Boot `11:43:30` |
| WeChat still pinyin after that | `11:47:53` → `xianzai'shi'keyi'le'ma'?`, `capture=null` |
| No `ocr_commit` / `ax_*` in daybook | All WeChat typing `capture=None` |
| No `wechat ocr` lines in `deskcap.log` | OCR/AX recovery returned empty **silently** |
| Input lag spikes | `lag input_worker=90–359ms` around typing — OCR attempt likely ran and failed |

**Conclusion:** Reading WeChat’s composer (AX / send-time OCR) is the wrong product path on WeChat 4.x + current macOS. The OS does not expose committed Chinese from the **system Pinyin IME** to a third-party process either.

## Correct architecture (habit = same 拼音手感, one input-source switch)

```
User types in ANY app (微信/某些 Electron 编辑器/…)
        │
        ▼
 DeskCap 输入法 (InputMethodKit + librime)
   └─ on commit / insertText(汉字)
        │
        ▼
 POST localhost DeskCap /ime/commit  →  daybook typing (汉字)
```

- **Cannot** silently tap 系统拼音 / 其他系统输入法 while they stay selected.
- **Can** ship DeskCap’s own IME (librime BSD, private repo OK) so **上屏汉字** is known before it enters WeChat.
- User switches input source **once** to「DeskCap拼音」; daily keys stay 拼音习惯.

## Status (2026-09-16)

| Step | State |
|------|--------|
| Stop WeChat-box OCR/AX as primary | done |
| `POST /ime/commit` | done — `capture=ime_commit` |
| Mac IMK + **librime** | **1.0.0** `/Library/Input Methods/DeskCapIME.app`（候选跟插入点；候选译词；`ime-active` 去重） |
| Full luna dict + OpenCC t2s | done |
| 候选窗（`firstRect` + 蓝胶囊） | done |
| DeskCap.app `ime-list` 维护启用列表 | done（不抢当前源） |
| 词库 / IME OTA via manifest | App 侧检查已做；`ime_url` / `ime_version` |
| 系统「添加」列表 100% 出现 | **未稳** — 公证后仍偶发；`force-enable-ime.sh` / 注销；禁双份安装与 MiniIME 幽灵 |

本地自检：`docs/mac-ops.md`。注意 Agent `/knowledge/day` 会过滤应用名含 `deskcap` 的事件。

## Product scope (honest)

| 阶段 | 内容 |
|------|------|
| **现在（采集必需）** | 能选中 DeskCap拼音、能打拼音上屏汉字、`insertText` → daybook |
| **已完成体验档** | luna 全量、简拼/模糊、候选条、SYMI 图标、共享/用户词分离 |
| **已知缺口** | 部分机「添加」对话框不列 DeskCap；跨进程启用粘性差 — 运维脚本兜底，非产品默认强制切换 |
| **更后（可选）** | 网络热词 / 云词库 — 不阻塞采集主线 |

不要用菜单栏搜索「DeskCap」；添加对话框里点左侧 **「中文」**。

## 分发给其他人（macOS）

1. **Developer ID Application**（Team `<TEAM_ID>`；本地用 `security find-identity -v -p codesigning` 查）
2. `bash macos-ime/scripts/build-and-install.sh`（嵌入 librime）
3. `bash macos-ime/scripts/notarize-ime.sh` → ditto 仅 `/Library/Input Methods`
4. 员工机：必要时注销 → 键盘 → 输入法 → `+` → 中文 → DeskCap拼音；找不到则 `force-enable-ime.sh`

主应用同样 **Developer ID + 公证**（`scripts/install-macos.sh` 优先 Developer ID）。
