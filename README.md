# DeskCap 1.0.0

本机办公采集与智能体知识沉淀工具。支持 **macOS**（含拼音输入法）与 **Windows x64**（仅采集）；Linux 暂不适配。

## 它做什么

- **采集**：本机键盘输入、窗口焦点、粘贴等事件 → 本地日账（JSONL，**只追加不重写**）
- **沉淀**：日账经 HTTP API 供智能体读取（先 `summary` 再拉正文），不产生逐条点击流
- **输入法**（macOS）：内置 `DeskCapIME` —— 中文候选、候选英文释词、候选框贴字跟随
  - 走输入法路径可直接采到**上屏汉字**（`/ime/commit`），比系统事件钩子更准
  - 微信 / 企业微信等场景在系统输入法下通常只能拿到拼音拉丁，建议切到「DeskCap拼音」

## 平台支持

| 平台 | 状态 | 说明 |
|---|---|---|
| macOS | **正式版** | 采集 + `DeskCapIME` 输入法；CI 构建 + 签名 + 公证 |
| Windows x64 | **正式版** | 仅采集；无输入法（Windows 输入法需 TSF COM DLL，未实现） |
| Linux | **暂不支持** | 代码保留了部分适配分支，但**未验证、不提供安装包、CI 不构建** |

## 安装

**推荐：直接下载构建好的安装包**，不用自己编译。

- **正式版**：仓库右侧 [Releases](https://github.com/symi-daguo/deskcap/releases)
  （打 tag 时由 CI 自动发布，含 macOS dmg / Windows exe / IME zip）
- **最新提交的构建产物**：仓库 [Actions](https://github.com/symi-daguo/deskcap/actions)
  → 选一次成功的 run → 页面底部 `Artifacts` 下载（保留 30 天）

> CI 默认 ad-hoc 签名。首次打开若被拦截：右键 → 打开，
> 或在「系统设置 → 隐私与安全性」点「仍要打开」。

**从源码构建**（开发或需要自定义时）：

```bash
pnpm install
pnpm tauri build --bundles app && bash scripts/install-macos.sh

# macOS 输入法（需管理员授权；CI 不设 INSTALL_TO_SYSTEM，只构建不安装）
INSTALL_TO_SYSTEM=1 bash macos-ime/scripts/build-and-install.sh
```

- 仓库本身**不含**安装包二进制（`.gitignore` 排除 `releases/*.dmg|*.exe`），
  产物由 CI 统一构建后上传；详见 `releases/README.md`
- macOS 权限引导：`bash scripts/fix-macos-tcc.sh`
- 输入法安装后需在「系统设置 → 键盘 → 输入法」中确认已添加「DeskCap拼音」

## 使用

采集端在本机监听 `8765` 端口：

| 端点 | 说明 |
|---|---|
| `GET /health` | 健康检查（无需令牌） |
| `GET /status` | 运行状态 |
| `GET /knowledge/day/summary?date=` | 当天摘要（Agent 首选） |
| `GET /knowledge/day?date=` | 当天正文（仅在摘要有内容时拉） |
| `POST /ime/commit` | 输入法上屏汉字提交 |

访问需 `Authorization: Bearer <个人令牌>`；令牌在「DeskCap 设置 → 接入」生成，**每台机器不同，勿混用**。局域网部署见 `SECURITY.md`。

## 开发

```bash
pnpm tauri build --bundles app    # 构建桌面端
cargo check --manifest-path src-tauri/Cargo.toml
bash macos-ime/scripts/build-and-install.sh   # 构建输入法（含 swiftc typecheck）
```

提交前自查清单见 `CONTRIBUTING.md`。

### 图标（官方链路，勿手搓）

```bash
python3 scripts/gen-icons.py   # → 抠掉 Win 外围直角底板 → pnpm tauri icon + Mac HIG icns
```

| 平台 | 规范 |
|------|------|
| Windows | 源图只有圆角 logo、**外沿透明**（无炭色方底板）；[Microsoft](https://learn.microsoft.com/en-us/windows/win32/uxguide/vis-icons) 32-bit alpha ICO；[Tauri](https://v2.tauri.app/develop/icons/) `pnpm tauri icon` |
| macOS | [Apple HIG](https://developer.apple.com/design/human-interface-guidelines/app-icons)：`icon-mac.png` 不透明全铺，系统裁圆角 |

Win 改图标后需卸载 + 删除桌面旧快捷方式 + 重装。

## 资源与合规

- **内存**：实测桌面端约 70–110 MB、输入法约 70–90 MB；钩子只入队，业务在 worker
- **磁盘**：`retention_days=14`、`max_disk_mb=1024`；超限先删最旧 session，再删最旧日账；截图默认关闭
- **写入**：日账与会话均为 **append-only**，不反复重写同一文件
- **隐私**：默认不强制切换用户输入法；支付 / 密码窗口默认排除；截图像素默认关闭
- **签名**：macOS 走 Developer ID + 公证，详见 `docs/mac-ops.md` 与 `SECURITY.md`
- **OTA**：设置页填 `update_manifest_url` 后可用「检查更新」，详见 `docs/mac-ops.md`

运维与合规真源：`docs/mac-ops.md`；输入法说明：`macos-ime/README.md`。

## 安全

漏洞请按 `SECURITY.md` 私下报告，勿公开提 issue。

## 贡献

见 `CONTRIBUTING.md`。

## License

本项目自研代码以 **MIT** 许可发布（见 `LICENSE`）。

其中 Rime 相关数据与运行时为 **LGPL-3.0**，必须随包分发其许可全文；完整第三方清单与
LGPL 合规措施见 `THIRD-PARTY-NOTICES.md`，许可原文在 `licenses/`。
