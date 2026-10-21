# 第三方组件与许可声明（THIRD-PARTY NOTICES）

DeskCap（含 `DeskCap.app` 与 macOS 输入法 `DeskCapIME.app`）为**商业软件**，
自有代码版权归 SYMI / 南京肆米科技有限公司所有。本项目**分发并使用**下列
第三方开源组件；每个组件按其自身许可条款使用，本项目不对其重新授权。

> 生成日期：2026-10-21 · 适用版本：DeskCap **1.0.0** / DeskCapIME **1.0.0**
> 许可全文见仓库 `licenses/` 目录。

## 1. 输入法运行时（macOS）

| 组件 | 版本 | 许可 | 用途 | 是否修改 |
|------|------|------|------|----------|
| [librime](https://github.com/rime/librime) | 1.x | **BSD 3-Clause** | 拼音引擎（`librime.1.dylib`） | 否，原样动态链接 |
| [OpenCC](https://github.com/BYVoid/OpenCC) | 1.4.2 | **Apache-2.0** | 简繁转换（`libopencc.1.4.dylib`） | 否 |
| [LevelDB](https://github.com/google/leveldb) | 1.x | **BSD 3-Clause** | librime 用户词典存储（`libleveldb.1.dylib`） | 否 |

三者均以**未修改的动态库**形式随 app 分发，许可文本见
`licenses/Apache-2.0.txt`；BSD 3-Clause 条款随对应上游仓库分发。

## 2. Rime 方案与数据（LGPL-3.0，重要）

`macos-ime/rime-data/` 下下列文件来自 Rime 生态，**许可为 GNU LGPL v3**：

| 文件 | 上游 | 许可 | 说明 |
|------|------|------|------|
| `luna_pinyin.dict.yaml` | [rime-luna-pinyin](https://github.com/rime/rime-luna-pinyin) | LGPL-3.0 | 朙月拼音词典（未修改） |
| `luna_pinyin.schema.yaml` | rime-luna-pinyin | LGPL-3.0 | 朙月拼音方案（未修改） |
| `pinyin.yaml` | rime-luna-pinyin | LGPL-3.0 | 模糊音/简拼/纠错规则（未修改） |
| `essay.txt` | [rime-essay](https://github.com/rime/rime-essay) | LGPL-3.0 | 八股文语料（未修改） |
| `punctuation.yaml` | [rime-prelude](https://github.com/rime/rime-prelude) | LGPL-3.0 | 标点规则（未修改） |
| `symbols.yaml` | rime-prelude | LGPL-3.0 | 符号表（未修改） |
| **`deskcap_pinyin.schema.yaml`** | **本项目派生** | **LGPL-3.0** | 基于 luna 方案 + 简体 + 官方模糊/简拼模板组合而成（**派生作品，按 LGPL 同许可分发**） |

### LGPL-3.0 合规措施（本项目的具体做法）

1. **运行时加载，不静态嵌入**：IME 进程在启动时从 app bundle 的
   `Contents/Resources/rime-data/` 与用户目录
   `~/Library/Application Support/DeskCap/rime/` 读取方案与词典；
   方案数据**不编译进任何二进制**。这符合 LGPL v3 §4(d)(1) 的
   "shared library mechanism" 等效要求。
2. **用户可替换 / 修改**：用户目录的同名文件**优先于** bundle 内置文件，
   用户可直接编辑或替换（见 `docs/mac-ops.md` 词表覆盖说明）。
3. **随附许可全文**：`licenses/LGPL-3.0.txt` + `licenses/GPL-3.0.txt`
   （LGPL v3 以 GPL v3 为基础，需同时提供）。
4. **列出修改**：除上表标注"派生"的 `deskcap_pinyin.schema.yaml` 外，
   其余 LGPL 文件均为**原样分发**，未作修改。

### 不属于 LGPL 的自有文件

- `deskcap_hot.dict.yaml`（热词）、`key_bindings.yaml`、`default.yaml`、
  `glossary.en.tsv`（候选译词表）、`_schema_version`：DeskCap 自建，
  版权归本项目。`glossary.en.tsv` 为自撰释义，**不含任何第三方词库数据**。

## 2b. 候选英文释义数据（Unicode License v3）

`macos-ime/rime-data/glossary.en.tsv` 的单字释义有 14240 条来自
[Unicode Unihan Database](https://www.unicode.org/charts/unihan.html) 的
`kDefinition` 字段（`Unihan_Readings.txt`）。

| 项 | 说明 |
|---|---|
| 许可 | **Unicode License v3**（宽松，允许商业使用与再分发，需保留版权声明） |
| 版权 | © Unicode, Inc. |
| 使用方式 | 摘取 `kDefinition` → 清洗成候选框单行释义（去括号注释、跳过姓氏/异体字、最多两义、≤44 字符） |
| 修改 | **是**（清洗与截断，见 `macos-ime/scripts/gen-glossary.py --unihan`） |
| 覆盖范围 | 仅 `essay.txt` 中 **rime 能打出**的 20746 个单字，未全量收录 |
| 自建部分 | 441 条人工校订单字 + 271 条自撰词组 **优先**，不被覆盖 |

**明确未使用**：
- ECDICT（MIT）—— 是**英→中**词典，方向与本需求相反
- CC-CEDICT（CC-BY-SA 4.0）—— ShareAlike 有传染性，不适合商业闭源分发

重新生成：`python3 macos-ime/scripts/gen-glossary.py --unihan <Unihan_Readings.txt>`

## 3. 桌面应用（跨端）

| 组件 | 版本 | 许可 | 用途 |
|------|------|------|------|
| [Tauri](https://github.com/tauri-apps/tauri) | 2.12 | MIT / Apache-2.0（双许可） | 应用外壳、打包 |
| [React](https://github.com/facebook/react) | 19.3 | MIT | 工作台 UI |
| [react-day-picker](https://github.com/gpbl/react-day-picker) | 10.0.2 | MIT | 日期选择面板 |
| [vis-timeline](https://github.com/visjs/vis-timeline) | 8.5.4 | MIT / Apache-2.0 | 24h 时间轴 |
| [date-fns](https://github.com/date-fns/date-fns) | 4.4.0 | MIT | 时间处理 |
| Rust crates | 见 `src-tauri/Cargo.lock` | 见各 crate `license` 字段 | 采集/存储/HTTP |

`cargo metadata` 全量核查结果：**Rust 依赖中无 GPL / AGPL 组件**
（2026-10-20 核查）。

## 4. 明确未使用的组件

- **无** GPL / AGPL 代码或数据
- **无** 收费 SDK / 需授权密钥的第三方服务
- **无** Google Fonts / CDN 资源（UI 仅用系统字体）
- **无** `IMKCandidates`（系统类，非第三方）

## 5. 联系

许可疑问 / 源码索取（LGPL 相关的对应源码）：DeskCap Contributors
（GitHub: [symi-daguo](https://github.com/symi-daguo)）。
LGPL 组件的"对应源码"即上述上游仓库 + 本仓库中标注为 LGPL 的文件本身
（YAML 即源码）。
