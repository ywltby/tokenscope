# TokenTracker 研究复核清单（供对抗式复核 agent 使用）

样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`（`xiufengsun/TokenTracker`，
HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，v1.3.14）。
被复核对象：`docs/research/tokentracker-draft/` 下的分片文档（S0/S1/S1b/S2/S3a/S3b/S4/S5/S6）。

## 0. 行号口径警告（**先读这一节，否则会白验**）

本仓库源码含中文注释，其中 **15 行以 ≥0x81 的字节结尾**（行尾是多字节字符的末字节）。
PowerShell 5.1 的 `Get-Content`（不指定编码）会以 GBK 解码，把这些行尾字节与后面的 `0x0A` 一起吞掉，
导致**行数少 15、且从第一个此类行之后所有行号整体偏移**（累计最多 15 行）。

实测：

| 口径 | `src/lib/rollout.js` 结果 |
| --- | --- |
| `read` 工具（权威，UTF-8/LF） | 24932 行 |
| `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8).Count` | 24932 |
| `(Get-Content $p -Encoding UTF8).Count` | 24932 |
| `(Get-Content $p).Count`（默认编码，**错**） | 24917 |
| `(Get-Content $p \| Measure-Object -Line).Lines`（**错**，且忽略空行） | 23260 |

**复核时必须使用**（任选其一）：

```powershell
# 1) 安全读第 N 行
$lines = [System.IO.File]::ReadAllLines("$env:TEMP\tt-research\src\lib\rollout.js", [System.Text.Encoding]::UTF8)
$lines[4221]   # 0-based → 第 4222 行

# 2) 安全 grep（带行号）
Select-String -Path $env:TEMP\tt-research\src\lib\rollout.js -Pattern "function bucketKey" -Encoding UTF8
```

或直接用 `read` 工具的 `offset`/`limit`（与权威口径一致）。
**任何行号不一致的分片内容都必须记为【错误】并给出实际行号。**

## 1. 复核方法

对每一条断言：

1. 打开指明的源码位置，**逐字**确认行号与内容是否与断言一致；
2. 若内容与断言不符 → 记为 **错误（E）**，给出「断言 / 实际」两行对照与实际行号；
3. 若源码中存在与断言相反、或更复杂的证据（另有分支、另有覆盖顺序、另有调用点） → 记为 **不完整（I）**；
4. 若无法从源码判定（需要运行、需要外部数据） → 记为 **无法验证（U）**，并写清楚"缺什么才能判定"；
5. 只做「能否证伪」的工作，不做风格评价、不做润色建议。

抽样要求（下限，按分片体量）：

- ≥1500 行的分片：**≥40 条**断言逐条打开源码核对（其中必须含 ≥10 条代码块引用的行号）；
- 700–1500 行：**≥25 条**；
- <700 行：**≥15 条**。

抽样必须**跨章节均匀**，不得集中在开头。每条都要在报告里写出「分片行号 / 源码文件:行号 / 结论」。

## 2. 已被主线亲自核实的锚点（A 组）—— 请抽查其中的 ≥8 条

见 `docs/research/tokentracker-draft/S0-mainline-anchors.md` 全文（A–E 五组，共 40 余条）。
复核时注意：S0 的行号是主线用 `read` 工具取的口径；若你发现某条与源码不符，
**S0 也要修**，请在报告里明确写「S0 的 X 条错误」。

## 3. 必须重点证伪的高风险断言（B 组）

这些来自各分片的「最有分量结论」，也是最容易错的地方。逐条独立验证：

| 编号 | 待证伪断言 | 来自 |
| --- | --- | --- |
| B1 | Codex 的 `reasoning_output_tokens` **不并入** `output_tokens`（解析侧独立成列），而计价侧 `computeRowCost` 却把 codex 的 reasoning 计费置 0 —— 两侧是否真的语义冲突？到底谁对？ | S2、S5 |
| B2 | 队列读端是 last-row-wins（不是求和）；修复对账却用 `setMaxTotal`（口径二） | S2、S4 |
| B3 | Claude 解析 `endOffset = st.size` 无行边界保护，可能在日志半行时永久丢量 | S2 |
| B4 | Gemini 是唯一能从源码推出「`total_tokens` 非零但 input/output 为 0 → $0 成本」的一家 | S3a |
| B5 | KiroCli 全部 token 由 `floor(字符数/4)` 估算，非真实用量 | S3a |
| B6 | 只有 `cursor` 与 `trae-cn` 必须联网/读凭据；`trae-cn` 强制 opt-in `TOKENTRACKER_TRAE_CN_USAGE=1` | S3b |
| B7 | 全仓无 `credits → token` 反推；Qoder 的 credits 不入 total | S3b |
| B8 | 45 个 `parse*Incremental` **没有**统一返回契约（五族签名） | S2 |
| B9 | 云端 5 份 edge 价格表与 `curated-overrides.json` 是两张手抄表，parity 测试不校验 `edge ⊇ curated`；`grok-build-free` 等 9 个键在云端会落到非零价 | S5 |
| B10 | `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 在 `pricing/index.js` 定义并导出却从未内部使用；真正生效的是 `codex-rollout-parser.js` 的另一份同名常量 | S5 |
| B11 | 队列唯一写点是 `rollout.js` 的 `fs.appendFile`，**无独立写锁**，崩溃窗口会留重复同键行 | S1 |
| B12 | 游标两代并存（单文件 `cursors.json` < 16 MB vs `cursor-store-v2/` 代际存储） | S1 |
| B13 | 0.26.3 的真值 repair 曾造成 2.17 亿/21.7 亿 token 丢失（注意分片里的数字量级：S1 说 "2.17B claude token"，S1b 说 "21.7 亿 token" —— 两者是否同一事件、数字是否自洽？） | S1、S1b |
| B14 | `wrapped` 全 UTC 切天、Dashboard 按 `tz` 切天 —— 同一份数据两个「最忙一天」 | S6 |
| B15 | 本地端点 `streak_days` 硬编码 0，前端自算覆盖 | S6 |
| B16 | 金额只在后端逐行计价后相加，前端零重算 | S5 |

## 4. 输出格式（复核 agent 写文件）

写到 `docs/research/tokentracker-draft/REVIEW-R1-<组号>.md`（UTF-8，用 `write` 工具）：

```
# 复核报告 R1-<组号>：<被复核分片>

## 结论
（一句话：该分片可用 / 需修订 / 不可用）

## 抽样统计
- 抽验断言数：N（其中 E=x, I=y, U=z）
- 实际打开过的源码文件清单

## 错误（E）—— 必须修
- [E1] 分片 §x.y：断言「…」/ 实际「…」（`src/...:实际行号`）
      修正建议：…

## 不完整（I）—— 建议补
- [I1] …

## 无法验证（U）
- [U1] …

## 行号漂移统计
- 抽样中行号正确的比例：x/y
- 发现漂移的位置（分片行号 → 实际行号）

## 复核到的行号抽样（证明真的打开过）
- `src/lib/rollout.js:4221` → `function bucketKey(source, model, hourStart) {`
- …
```

**注意**：报告只写你**亲自打开并在报告里贴出摘录**的行号；没打开过的不要列入。
