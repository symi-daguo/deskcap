# 贡献指南

感谢参与 DeskCap。本文件说明本地开发、验证与提交的约定。

## 环境

| 组件 | 版本 |
|---|---|
| macOS | 27（输入法与桌面端完整功能） |
| Windows | 10/11 x64（仅采集，无输入法） |
| Node.js | 22+ / pnpm |
| Rust | stable |
| Xcode CLT | `xcode-select --install` |

## 常用命令

```bash
# 前端 + Tauri 桌面端
pnpm install
pnpm tauri build --bundles app        # 产出 src-tauri/target/release/bundle/macos/DeskCap.app

# macOS 输入法（构建 + 签名 + 装到 /Library/Input Methods）
INSTALL_TO_SYSTEM=1 bash macos-ime/scripts/build-and-install.sh
```

> 本地打 dmg 想要 Developer ID 签名时，显式给身份（`tauri.conf.json` 里
> `signingIdentity` 已置 `null`，不写死任何人的证书）：
>
> ```bash
> APPLE_SIGNING_IDENTITY="Developer ID Application" pnpm tauri build --bundles dmg
> ```
>
> 不设则走 ad-hoc 签名。CI 由 `Pick signing identity` step 自动决定。
```

> 安装脚本会请求管理员授权（`osascript` 弹窗）。CI 不设 `INSTALL_TO_SYSTEM`，
> 只构建不安装。

## 提交前必须自查

- [ ] `pnpm tauri build` 无错误
- [ ] `bash macos-ime/scripts/build-and-install.sh` 构建通过（`swiftc` typecheck 0 error）
- [ ] `cargo check --manifest-path src-tauri/Cargo.toml` 通过
- [ ] 第三方数据文件（`macos-ime/rime-data/luna_pinyin.*`、`essay.txt` 等）**零改动**
- [ ] 未把构建产物、截图、临时文件、日志提交进仓库
- [ ] 新增依赖时同步更新 `THIRD-PARTY-NOTICES.md` 与 `licenses/`
- [ ] 文档中的版本号、命令、路径与真实实现一致
- [ ] **GitHub Actions 全绿**（macOS / Windows / IME 三个 job；Linux 不适配、不构建）
- [ ] 安装包由 CI 统一构建，本地构建产物不要提交

## 代码约定

- 注释写**为什么**，不写「做了什么」
- 涉及 macOS 输入法坐标/生命周期时，**先查日志再改**：
  输入法日志在 `~/Library/Application Support/DeskCap/logs/ime.log`
  （系统 `log show` 在部分环境查不到，务必用文件日志）
- 不在代码与文档里写第三方实现的名称、内网地址、令牌等非必要信息
- 破坏性操作（删除数据、发布、对外材料）由维护者决定

## 安全问题

不要公开提 issue 报漏洞，按 `SECURITY.md` 的流程私下报告。
