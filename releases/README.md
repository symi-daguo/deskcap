# DeskCap releases

## 安装包从哪来

**由 GitHub Actions 统一构建**，不在仓库里存二进制：

| 渠道 | 内容 | 保留 |
|------|------|------|
| [Releases](https://github.com/symi-daguo/deskcap/releases) | 打 tag `v*` 时 CI 自动发布的正式包 | 永久 |
| [Actions](https://github.com/symi-daguo/deskcap/actions) → 某次 run 底部 `Artifacts` | 每次 push 的构建产物 | 30 天 |

`.gitignore` 排除了 `releases/*.dmg` / `*.exe` / `*.msi` / `*.app` —— 仓库只保留源码。

## 当前产物清单（CI 产出）

| 平台 | 文件 | 来源 job |
|------|------|----------|
| macOS (Apple Silicon) | `DeskCap_<版本>_aarch64.dmg` | `DeskCap · macOS (dmg)` |
| macOS 输入法 | `DeskCapIME.zip`（解压得 `DeskCapIME.app`） | `DeskCapIME · macOS` |
| Windows x64 | `DeskCap_<版本>_x64-setup.exe` | `DeskCap · Windows (nsis)` |

## 签名状态

CI **默认 ad-hoc 签名**（无证书）：
- macOS 首次打开若被拦截 → 右键「打开」，或系统设置里点「仍要打开」
- Windows SmartScreen 提示 → 更多信息 → 仍要运行

维护者在仓库 Secrets 配好下面四项后，CI 自动切到 **Developer ID 签名 + Apple 公证**：

```
APPLE_CERTIFICATE_P12       base64 编码的 .p12
APPLE_CERTIFICATE_PASSWORD  .p12 密码
APPLE_ID                    Apple ID
APPLE_TEAM_ID               Team ID
APPLE_APP_PASSWORD          App 专用密码
```

## 本地构建（仅开发自测）

```bash
# 本地签名（可选；不设则 ad-hoc）——仓库里不写死任何人的证书
export APPLE_SIGNING_IDENTITY="Developer ID Application"
pnpm tauri build --bundles dmg        # → src-tauri/target/release/bundle/dmg/
bash scripts/copy-mac-release.sh      # 复制到 releases/（本地用，不进版本库）
INSTALL_TO_SYSTEM=1 bash macos-ime/scripts/build-and-install.sh   # 输入法
```

**不要**把本地产物提交进仓库 —— CI 才是唯一分发构建入口。
