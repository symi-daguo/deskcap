# rime-data 许可说明

本目录混有两类文件，**许可不同**，分发时请勿混为一谈：

## LGPL-3.0（来自 Rime 生态，原样分发）

`luna_pinyin.dict.yaml` · `luna_pinyin.schema.yaml` · `pinyin.yaml` ·
`essay.txt` · `punctuation.yaml` · `symbols.yaml`

上游：rime-luna-pinyin / rime-essay / rime-prelude（均为 LGPL-3.0）。
许可全文：仓库 `licenses/LGPL-3.0.txt` + `licenses/GPL-3.0.txt`。

**LGPL 合规要点**：这些文件由 IME 进程**运行时加载**（bundle 内置 +
用户目录 `~/Library/Application Support/DeskCap/rime/` 优先覆盖），
不编译进二进制；用户可自由替换/修改。

## LGPL-3.0（本项目派生）

`deskcap_pinyin.schema.yaml` —— 基于 `luna_pinyin.schema.yaml` 组合
简体 + 官方模糊/简拼模板而成，按 LGPL-3.0 同许可分发。

## 自有版权（非 LGPL）

`deskcap_hot.dict.yaml` · `key_bindings.yaml` · `default.yaml` ·
`glossary.en.tsv` · `_schema_version`

以上由 DeskCap 自建。`glossary.en.tsv` 为自撰英文释义，**不含第三方词库**。

详见仓库根 `THIRD-PARTY-NOTICES.md`。
