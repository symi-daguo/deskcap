# AI Agent Progressive Code Understanding Guide

## Project — DeskCap **1.0.0** (macOS + Windows 正式；Linux 暂不适配)

1. Prefer skill text **copied from DeskCap Agent tab** (built-in, collapsed by default)
2. Hermes bot：`BASE=http://<员工局域网IP>:8765` + 该员工自己的 Bearer token（勿混用他人 token）
3. `/health`（可无 token）→ `/status` → **`/knowledge/day/summary?date=`** → 仅正文时 `/knowledge/day`（**`data.text`**）
4. **Never** ingest per-event `mouse_click` streams; counts live in summary only
5. Dedupe / redact / WeChat pinyin → agent DB only; never echo API token; never mutate DeskCap
6. Pinyin≠100% 可译；英文勿当拼音；微信拼音须**时间窗整段**猜（非单条）；错字/半截/噪声见 Skill「拼音 / 英文 / 错字解读」
7. Agent API bind **`0.0.0.0`**（LAN）；`local_only`=无云上传；防火墙放行 TCP 8765

## Capture notes

- Day detail API = text only; UI timeline may still show clicks
- Retention: `retention_days` + `max_disk_mb` on sessions **and** day JSONL
- macOS 菜单栏：**亖** title only；Windows 托盘：圆角 SYMI 图标（Linux 分支保留但未验证）
- Token: `dc_` + UUID v4
- Accessibility / TCC 引导仅 macOS；Windows 端 UI 隐藏（Linux 分支保留但未验证）
- **Linux 暂不适配**：代码保留部分适配分支（cfg 条件编译），但**未验证、不提供安装包、CI 不构建**。上游 `libspa`/pipewire ABI 不匹配未解决，不承诺可用。
- 预期占用：内存约百兆级（含 WebView2）；磁盘默认上限 1GB，超限自动删最旧 session/日账
- **打字/IME（诚实）**：微信/企微在 Mac+Win 常见为**拼音拉丁**（OS 钩子看不到上屏汉字）。**正确路径：DeskCap 输入法**在 `insertText` 采汉字（见 `docs/ime-capture.md`），不是采微信框。过渡期仍保留 EventTap 拼音流水线。
- **中文精准（已发）**：`POST /ime/commit` + Mac IMK **DeskCapIME 0.6.55**（候选框贴字跟随；候选译词 `GlossaryLanguage`（默认 `off`）；多字词组恒压单字；fresh input 永远高亮 displayed[0]；0 偏移 placement + `applyRect` 严格 clamp；50pt 重试阈值抗 Chromium IPC 抖动；fuzzy/简拼/纠错走上游 pinyin.yaml；FrequencyTracker debounce+LRU+flush；`ime-active` 防双记；schema `_schema_version` 自动 reseed）。✅ 0.6.53 已真机验证（三轮用户实切反馈全修：回车语义/单字 441+词组 272/设置页开关/工作台日期与视窗自动跟随/tray 白色细体/备份空文件自愈）（回车=上屏原始字母恢复 mac 官方语义 / 词表扩至 166 词组「我是→I am」/ 设置页补译词开关；编译+签名+装机+采集全通）。证据见 `docs/mac-verify-0.6.47.md` 文末。**候选框交互观感仍需肉眼确认**（切换输入法必须人工操作，沙箱无法代替）
- **并非全操作**：无逐条点击流、默认无截图像素；支付/密码/本窗排除。Agent 先 summary 再拉正文
- **Win × 某些 Electron 编辑器**：标为**已知弱项、不承诺**（Chromium 钩子/UIA 限制）。Win 验收以微信/记事本等为准；详见 `docs/windows-capture.md`
- **Win 输入实现**：`WH_KEYBOARD_LL` + 键轮询回退 + Raw Input/按键轮询点按（**无** `WH_MOUSE_LL`）；钩子/轮询只入队，业务在 worker
- **Win 卡顿排查日志**：`%APPDATA%\DeskCap\logs\deskcap.log`。心跳含 `kb_poll=` / `mouse=` / `keydown=`（详见 `docs/windows-capture.md`）
- **单实例**：同时只允许一个 DeskCap（macOS / Windows 已验证）；再次启动会聚焦已有窗口并退出第二进程（避免双钩子 / 心跳交替）
- **UI「跳过」**：当前前台在排除名单（DeskCap **应用名/路径** / 支付 / 密码窗）。看 DeskCap 本窗时跳过正常。**不会**因 某些 Electron 编辑器 标题含文件夹名 `deskcap` 而跳过
- **Win 排障**：本机 `Get-Content %APPDATA%\DeskCap\logs\deskcap.log -Tail 100` + `/status`（勿贴 token）。详见 Skill「导出日志给研发」
- **紧急恢复**：Mac 键鼠死锁时 `kill -9` DeskCap；Win 任务管理器结束进程。勿依赖「暂停采集」（钩子可能仍在）
- **已知崩溃（已修 1.0.15）**：2026-09-10 17:25 `deskcap-clipboard` 读剪贴板时 NSPasteboard ObjC 异常 → SIGABRT 整进程退出；现已 `@catch` 吞掉，不再拖垮采集
- **日账/会话写盘**：JSONL 批量 flush（~0.4–0.5s）；停止采集 / 退出 / Agent 读当天前会 flush。import 前先 flush，避免丢缓冲。写失败打日志并重开 sink。读侧跳过坏行。`kill -9` 仍可能丢最后一批未 flush 行（设计取舍：不全量 fsync）
- **产品原则（2026-09）**：默认**不强制切换**客户输入法（习惯优先 → EventTap 拼音 + AX）；微信等要近 100% 汉字需用户切到 **DeskCap拼音**（已随包分发，非「后期再做」）。DeskCap.app `ime-list` 只维护启用列表，**不抢**当前输入源
- **多端构建（CI，唯一构建入口）**：`.github/workflows/build.yml` — **macOS + Windows 两端**编译 + **上传安装包 artifact** + **打 tag `v*` 自动发 Release**。产物：macOS(dmg) / Windows(nsis) / IME(zip)。**Linux 不参与构建。**签名：默认 ad-hoc；配置 `APPLE_CERTIFICATE_P12` / `APPLE_ID` / `APPLE_TEAM_ID` / `APPLE_APP_PASSWORD` 四个 secrets 后自动走 Developer ID + 公证。**本地不再作为分发构建入口**（仅开发自测）。CI 首跑抓到过 Windows 端真编译 bug（`typing_buffer_snapshot` 从未实现）——`#[cfg(target_os)]` 分模块必须各平台真编译
- **第三方许可**：见 `THIRD-PARTY-NOTICES.md` + `licenses/`。`luna_pinyin.*`/`pinyin.yaml`/`essay.txt`/`punctuation.yaml`/`symbols.yaml` 与派生文件 `deskcap_pinyin.schema.yaml` 为 **LGPL-3.0**；自建代码保持商业许可（rime 数据运行时加载，不静态嵌入）
- **实测基线（2026-10-20）**：IME 37 MB / 常驻 21.6 MB（0.0% CPU）；DeskCap 15 MB / 66.5 MB（0.2% CPU）；`/ime/commit` 200 @4.5 ms，无 token/错 token 均 401
- Mac 安装：**`pnpm tauri build --bundles app && bash scripts/install-macos.sh`**（**优先 Developer ID**；**禁止** ad-hoc）。权限异常跑 **`bash scripts/fix-macos-tcc.sh`**。IME：`bash macos-ime/scripts/build-and-install.sh` → `notarize-ime.sh` → ditto 仅 `/Library/Input Methods`
- 安全细则见 `SECURITY.md`（LAN HTTP + 防火墙）
- Mac 运维/合规/IME/OTA：**唯一真源** `docs/mac-ops.md`（**验收先跑 `scripts/rime-type-test.sh`**）；根目录旧 `macos-ops.md` 已删
- IME：`macos-ime/README.md`；当前 **1.0.0**（候选框贴字跟随 + 候选译词 + 多字恒压单字 + 首个候选始终高亮 + fuzzy/简拼/纠错 + 热词 96 + 习惯学习稳定；品牌「亖」；`ime-active` 去重）。审计真源 `macos-ime/PONYTAIL-AUDIT-2026-10-02.md`（最新）+ `macos-ime/PONYTAIL-AUDIT-2026-09-29.md`（根目录 `PONYTAIL-AUDIT.md` 已标取代）
- **IME 仅 macOS**：IMK `.app` 装 `/Library/Input Methods/`，**Windows 无法安装**（Win 需 TSF COM DLL，仓库内无）。Win 上 DeskCap 只做**采集**，不做输入法
- `releases/`：macOS **1.0.0** dmg 为当前版；Windows exe 仍是 **1.0.19 存档**（需 Windows 环境构建）；1.0.13 已归档。当前源码/本机 **1.0.0** + IME **1.0.0**
- macOS 菜单栏：DeskCap / 输入法图标均为「**亖**」
- 中控台布局对齐数字人工作台（顶栏 + 横向页签）；主题在顶栏「跟随系统」按钮；色板深 `#0e1014` / 浅 `#f5f7fa`。全屏时实时流与日账列表按可视高度填满，不固定条数提前翻页
- **本机 OS：** macOS **27**；菜单栏工作区约 **22pt**


## Icons（官方链路，勿手搓）

- Win 源图：`icon-masters/icon-win.png` / `app-icon.png` = **圆角 logo，外沿透明**（抠掉外围直角底板；透明像素 RGB 必须为 0；并放大到约 92% 画布）
- 生成：`python3 scripts/gen-icons.py` → **`pnpm tauri icon`** + 同步 `public/favicon.png`（ICO 含 16/24/32/48/64/256，**32px 层在第一**）
- macOS Dock：`icon-masters/icon-mac.png` → `icon.icns` 不透明全铺（系统裁圆角）
- 应用内：`src/assets/symi-mark.png` 勿为系统图标改
- Win 改图标后：**卸载 + 删旧桌面快捷方式 + 重装**；必要时清 IconCache
