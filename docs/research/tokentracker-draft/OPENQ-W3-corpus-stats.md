# OPEN-W3：本机真实 agent 语料统计（存疑攻坚）

> **本报告仅含统计数字与字段名，不含任何会话内容。** 全程只读打开日志（未修改、未复制、未入库、未联网）；所有数字来自对真实日志的全量流式扫描，无估算。

- **对象**：本机真实 agent 日志（只读）
  - Claude：`~/.claude/projects` —— 22 个 `*.jsonl` / 4609 行 / 24.1 MB 目录总量（另有 18 个 `subagents/*.meta.json` 等元数据文件、26 个 `.md`、5 个 `.txt`、1 个 `.py`，合计 72 文件）
  - Codex：`~/.codex/sessions/**/rollout-*.jsonl` —— 64 文件 / 179776 行 / 1272.1 MB
  - Gemini（Antigravity 布局）：`~/.gemini` —— 全目录 965 文件 / 74.6 MB；解析范围 207 个 `.jsonl`+`.json`（≤8 MB）/ 8052 行 / 13.8 MB
  - 其他：`~/.zcode` 3588 文件 / 573.0 MB；`~/.workbuddy` 70248 文件 / 2099.4 MB；`~/.codebuddy` 24 文件 / 0.7 MB（后两者只做路径形态与配置键名统计）
- **方法**：Node `readline` 逐行流式（不整文件 `-Raw`）+ 真实 JSON 解析；解析失败行计数，不入任何样本仓库
- **对应存疑**：A2、A3、A18、A11（Gemini 部分）、C24、以及 Codex reasoning/total 计价假设
- **判定分布**：已判定 10 条（其中第 4 条含一处范围限定、第 10 条为形态层判定）

---

## 1 Codex：`reasoning_output_tokens` 与 `output_tokens` 的关系

- **判定**：已判定
- **统计结果**（64 文件 / 179776 行 / 0 解析失败，全部 `token_count` 与 `token_usage_record` 事件）

| 计数字段路径 | 对数 | `reasoning > output` | `reasoning > 0` | `reasoning = 0` | `output = 0` | max(rsn/out) |
|---|---|---|---|---|---|---|
| `event_msg.token_count.info.last_token_usage` | 27802 | **0** | 24021 | 3781 | 321 | 1.0000 |
| `event_msg.token_count.info.total_token_usage` | 27802 | **0** | 27776 | 26 | 1 | 0.9249 |
| `token_usage_record.payload.usage` | 3259 | **0** | 2599 | 660 | 0 | 1.0000 |
| `token_usage_record.payload.turn_token_usage` | 3259 | **0** | 3085 | 174 | 0 | 0.9309 |
| `token_usage_record.payload.thread_token_usage` | 3259 | **0** | 3234 | 25 | 0 | 0.6781 |
| **合计** | **65381** | **0** | **60715** | — | — | ≤1.0000 |

补充：65381 对中 `reasoning == output` 328 对、两者同为 0 322 对；单字段上界 `reasoning_output_tokens` 2580853、`output_tokens` 5443449。`reasoning_output_tokens` 与 `output_tokens` 在全部 65381 对中 **100% 同现**（六键形态固定，见 §2）。

- **结论**：**`reasoning_output_tokens ≤ output_tokens` 在本机 65381 对实测中恒成立（0 反例，比值上界 1.0000）**，支持「reasoning 是 output 的子集、两者不得相加」这一计价假设；本机不存在 reasoning 与 output 需分别计价或相加的证据。
- **方法**：`codex-scan.js` 全量 64 文件流式扫描（顶层 `type` 分派 + 递归定位配对键）；`codex-scan2.js` 按路径分组复算（两次独立扫描的 65381 对与 60715 条 `reasoning>0` 一致）。

---

## 2 Codex：`total_token_usage` / `last_token_usage` 的形态与累计性

- **判定**：已判定
- **统计结果**（同一 64 文件 / 179776 行）

同时出现率：`event_msg.token_count.info` 含 `total_token_usage` 27802 条、含 `last_token_usage` 27802 条，**两者同现 27802 / 27802（100.0%）**，单侧出现 0 条、双侧皆无 0 条；`token_count` 事件另 100% 带 `rate_limits`（27787 条含 `limit_id/primary/secondary/credits/plan_type` 等子键）。本机 Codex 为**双形态并存**：老形态 `event_msg.payload.type == "token_count"` 27822 条事件 + 新形态 `token_usage_record` 3259 条事件。

单调性（累计性判据）：

| 序列 | 有 ≥2 点的文件 | 单调不减 | 有回退 | 回退次数 | 序列长度范围 |
|---|---|---|---|---|---|
| `event_msg … total_token_usage.total_tokens` | 52 | 49 | 3 | 3 | 2 – 10887 |
| `event_msg … last_token_usage.total_tokens` | 52 | 16 | 36 | 766 | 2 – 10887 |
| `token_usage_record.thread_token_usage.total_tokens` | 17 | **17** | 0 | 0 | 8 – 1027 |

回退幅度：`total` 三处回退为 −2965231 / −3780242 / −34082848（跨会话/段重置量级，非小幅抖动）；`last` 回退 30 例样本范围 −39 … −204437（本节判据下 `last` 不是累计量）。

抽样序列（同一文件的真实数值，仅数字）：

```
thread_token_usage.total_tokens : 21886, 45365, 83224, 121453, 164638, 208077, 277304, 317784   (严格递增)
event_msg total_token_usage     : 8592, 43236, 82663, 135434, 215294, 301016, 390500, 480999   (严格递增)
event_msg last_token_usage      : 8592, 34644, 39427, 52771, 79860, 85722, 89484, 90499        (非累计)
```

增量/累计交叉校验（`codex-scan4.js` / `codex-scan5.js`）：

- 段内对比（以 `total` 回退切段，55 段）：`sum(last.total_tokens)` 与段末 `total_token_usage.total_tokens` **精确相等 15 段**、偏差 ≤1% 9 段、≤5% 13 段、>5% 18 段；方向 37 段偏高、3 段偏低。
- 跨形态文件级对比（17 个同时含两形态的文件）：`sum(last.total_tokens) / thread_token_usage 最终值` 比值分位 min 0.7135 / p25 1.0000 / **中位 1.0000** / p75 1.0326 / max 1.5201；`token_count 段末 total / thread 最终值` 11/17 精确为 1、6/17 低 1–5%。
- 逐请求增量校验：`sum(token_usage_record.payload.usage.total_tokens)` 与 `thread_token_usage.total_tokens` 最终值 **16/17 文件精确相等**（1 个超出 5%）。

`total_tokens` 与分量关系：65381 对中 `total_tokens == input_tokens + output_tokens` 65060 对；余 321 对中 **320 对出现在 `last_token_usage` 且全部 `output_tokens == 0`**（差值范围 6456 – 205888，方向恒为 `total > input`），1 对在 `total_token_usage`（差值 121600）。

- **结论**：`total_token_usage` 与 `last_token_usage` **恒定同时出现**；`total` 是累计量（跳过程内重置则单调不减），`last` 是逐请求增量；**`payload.usage` 与 `thread_token_usage` 的相对关系在 16/17 文件上精确闭合，可确认「增量字段 + 累计字段」二元结构**。落地口径：逐事件累加只能用增量字段（`last_token_usage` / `payload.usage`），累计字段只能取末值；把 `total_token_usage`（27802 条重复快照，output 求和 35351865647 vs 增量求和 16395234）逐条累加会**约 2000 倍重复计费**。另有 320 条 `output_tokens == 0` 但 `total_tokens > input_tokens` 的记录，按 `output_tokens` 计价会在这些行上少计。
- **方法**：`codex-scan.js`（同现率/单调性）、`codex-scan4.js`（按 `total` 回退切段，逐段 `sum(last)` vs 段末 `total`）、`codex-scan5.js`（按 `thread_id` 分组跨形态对比，输出比值分位）。

---

## 3 Codex：`service_tier` 分布（对照上游注释）

- **判定**：已判定
- **统计结果**（64 文件 / 179776 行）

| 指标 | 本机实测 | 上游注释 |
|---|---|---|
| 含该字段的文件数 | **32 / 64（50.0%）** | 1213 文件 / 167 命中 / 25% |
| 事件数 | **433** | （未给事件数） |
| 值分布 | `default` 385 / `priority` 48 | default 814 / priority 58 |

落点路径：`event_msg.payload.thread_settings.service_tier`（`event_msg.payload.type == "thread_settings_applied"`，该类型事件共 445 条，其中 12 条无 `service_tier` 键）；另有 4 次出现在 `response_item` 的工具参数 schema 中（`…parameters.properties.service_tier`，非用量字段，应排除）。`token_count` / `token_usage_record` 的 `payload` 直层**不含** `service_tier`。

- **结论**：`service_tier` 在本机由 `thread_settings_applied` 事件传递（`payload.thread_settings.service_tier`），命中率 **50.0%（32/64 文件、433 事件、default 385 / priority 48）**；上游「25% / 1213 文件」对应另一批语料，主文档应改用本机实测口径。
- **方法**：`codex-scan.js` 行级字符串命中 + 递归键定位；`codex-scan2.js` 全量 `service_tier` 键路径与值分布枚举；`codex-scan3.js` 事件级与文件级计数（两次独立扫描文件数与值分布一致）。

---

## 4 Codex：`model/rerouted` 是否出现 + 模型字段一致性（A18）

- **判定**：已判定（`rerouted` 为否；模型一致性给出范围限定）
- **统计结果**（64 文件 / 179776 行）

`rerouted` 相关：对 `rerouted` 子串做逐行匹配，**179776 行中命中 0 行**（`model/rerouted` 与 `model_rerouted` 两种写法均为 0）。

模型字段：

| 项 | 计数 |
|---|---|
| `session_meta` 总数 | 78 |
| `session_meta.payload.model` 存在 | **0** |
| `session_meta.payload.base_instructions.provenance.model` 存在 | 30 |
| `turn_context.payload.model` 出现的文件 | 62 / 64 |
| 每文件 `turn_context.model` 不同值个数 = 1 | 58 文件 |
| 每文件 `turn_context.model` 不同值个数 = 2 | 4 文件 |
| 两侧都有（同文件） | 21 文件 |
| 其中一致（`session_meta` 侧值 ∈ `turn_context` 值集合） | **20** |
| 其中不一致 | 1 |

- **结论**：本机 Codex 日志中 **`model/rerouted` / `model_rerouted` 事件不存在（0 命中，A18 在本机不成立）**；`session_meta` 没有 `payload.model` 键，模型信息只在 `payload.base_instructions.provenance.model`（30/78 条）与 `turn_context.payload.model`（62/64 文件）两处，二者在 20/21 个可比文件上一致，1 个文件不一致、4 个文件存在同一文件内模型值切换（2 个不同值）——**适配器不可假设「每文件单一模型」**。
- **方法**：`codex-scan.js` 行级子串命中 + 顶层类型字段提取；`codex-scan3.js` 按文件聚合两侧模型值集合做包含比较（只输出一/不一致计数，不输出模型串）。

---

## 5 Claude：`cache_creation_input_tokens` 与 `cache_write_input_tokens`（A2）

- **判定**：已判定
- **统计结果**（22 个 `*.jsonl` / 4609 行 / 0 解析失败；`assistant` 行 1520）

| 键 | 出现行数 | 非零行数 | 数值上界 |
|---|---|---|---|
| `message.usage.cache_creation_input_tokens` | **1520**（assistant 行的 100%） | 1164 | 225051 |
| `message.usage.cache_write_input_tokens` | **0** | **0** | — |
| 两键同现于同一行 | **0** | — | — |
| `message.usage.cache_read_input_tokens`（对照） | 1520 | 1228 | — |

- **结论**：本机 Claude 日志**从不出现 `cache_write_input_tokens`（0 行、0 次同现）**，「两键并存导致漏计/双计」的风险在本机不成立；缓存写入语义由 `cache_creation_input_tokens`（1520 行出现、1164 行非零）承载。若主文档按 `cache_write_input_tokens` 兜底取值，本机永远命中兜底分支——应按 `cache_creation_input_tokens` 为主键。
- **方法**：`claude-scan.js` 逐行解析，对 `message.usage` 做键存在性与取值计数；`tokenfield-scan.js` 复核 22 个 jsonl 的 token 键路径（未发现任何 `cache_write*` 变体）。

---

## 6 Claude：`"model":"<synthetic>"` 行的 usage 是否非零（A3）

- **判定**：已判定
- **统计结果**（22 文件 / 4609 行）

| 项 | 计数 |
|---|---|
| `message.model == "<synthetic>"` 的行 | **6**（分布于 4 个文件） |
| 其中 `message.usage` 缺失 | 0 |
| 其中 usage **任意字段非零** | **0** |
| 其中 usage 字段存在量 | 12 个键各 6 次（`input_tokens` / `output_tokens` / `cache_creation_input_tokens` / `cache_read_input_tokens` 等全为 0） |

- **结论**：本机 `<synthetic>` 行共 6 条、**其 usage 全字段为 0（非零行 0 条）**，按 token 计价不会为其产生费用；这 6 条不构成「$0 风险行」，但其存在说明 `<synthetic>` 行应按 0 用量处理而非跳过（否则会丢/串会话计数）。
- **方法**：`claude-scan.js` 对 `message.model` 精确匹配 `<synthetic>` 并逐键判定零/非零。

---

## 7 Claude：usage 字段形态（C24 三处口径差异）

- **判定**：已判定
- **统计结果**（22 文件 / 4609 行）

| 位置/字段 | 出现行数 | 备注 |
|---|---|---|
| `message.usage` | **1520**（= 全部 assistant 行） | |
| 顶层 `usage` | **0** | 本机不存在顶层 usage 形态 |
| `message.usage.output_tokens_details.thinking_tokens` | **1288**（84.7% of 1520） | 非零 406，数值范围 0 – 4373 |
| `message.usage.reasoning_output_tokens`（含 `iterations[].reasoning_output_tokens` 一并检索） | **0** | 任何路径均未出现 |
| `message.usage.iterations[]` | 1288 | 与 `thinking_tokens` 同现 |

`message.usage` 键集合只有两种形状：12 键形态（`cache_creation, cache_creation_input_tokens, cache_read_input_tokens, fallback_credit, inference_geo, input_tokens, iterations, output_tokens, output_tokens_details, server_tool_use, service_tier, speed`）**1294 行**；4 键形态（`cache_creation_input_tokens, cache_read_input_tokens, input_tokens, output_tokens`）**226 行**。`service_tier`（usage 内）出现 1288 行，值为 `standard` 1288 行、`null` 6 行。

- **结论**：本机 Claude usage **只存在于 `message.usage`（1520 行），顶层 `usage` 0 行**；思考 token **只由 `output_tokens_details.thinking_tokens` 表达（1288 行，406 行非零）**，`reasoning_output_tokens` 在任何路径均为 0 行——**两侧不是「互斥」，而是本机只有前者**；口径差异应统一取 `message.usage` 并优先 `thinking_tokens`，且需兼容 226 行的 4 键简化形态（缺 `output_tokens_details`、`service_tier`、`iterations`）。
- **方法**：`claude-scan.js` 顶层 `usage` 与 `message.usage` 分别计数 + `output_tokens_details`/`iterations[]` 递归检索 + 键集合指纹聚合。

---

## 8 Claude：`isSidechain` 与子代理路径

- **判定**：已判定
- **统计结果**（22 个 jsonl / 4609 行）

| 分组 | 文件数 | 行数 | assistant 行（含 usage） | `isSidechain=true` 行 |
|---|---|---|---|---|
| 主链（`<project-slug>/<session-id>.jsonl`） | 8 | 3511 | 985 | 0 |
| 子代理（`<project-slug>/<session-id>/subagents/agent-<id>.jsonl`） | **14** | 1098 | **535** | **1094** |

`isSidechain` 键：true 1094 行、false 2239 行、无该键 1276 行；assistant 行 1520 中 true 535 行（全部落在 `subagents/` 路径文件内，交集 535/535）。子代理用量规模：`input_tokens` 合计 9014976、`output_tokens` 247787、`cache_read_input_tokens` 28367496、`cache_creation_input_tokens` 2430090；主链对应 6554302 / 1160987 / 120461368 / 8517190。目录内另有 18 个 `subagents/*.meta.json` / `*.forked-skill.json` 元数据文件（各 1 行，**不含任何 token 字段**）。

- **结论**：本机 22 个 Claude 会话文件中 **14 个位于 `<session-id>/subagents/agent-*.jsonl`（占 63.6%）**，其 assistant 行 535 条全部带完整 usage——**子代理 token 确实照计**（子代理 input 占两组合计 11.4%）；主链文件仅 8 个。会话数统计若按文件计数会虚增 63.6%，按「子代理不计会话、只计 token」的口径才与本机结构一致；`subagents/*.meta.json` 无用量字段，不应作为用量来源。
- **方法**：`claude-scan2.js` 按路径是否含 `subagents` 分组，分别统计文件/行/assistant/usage/侧链标记与四类 token 求和（路径在报告中外形为 `<project-slug>/<session-id>/…` 模板）。

---

## 9 Gemini：是否存在「只写 total、分量字段全 0」的 usage 行（A11 的 Gemini 部分）

- **判定**：已判定
- **统计结果**（`~/.gemini` 下 207 个 `*.jsonl`+`*.json`（≤8 MB）/ 8052 行 / 13.8 MB；541 行解析失败来自多行缩进的 `.json` 元数据文件，非会话主体）

本机 `~/.gemini` **不是经典 Gemini CLI 布局**：`antigravity` / `antigravity-cli` / `antigravity-ide` 三套 Antigravity 目录；经典标记 `tmp/`、`settings.json`、`projects.json`、`oauth_creds.json` 全部不存在；会话主体为 `antigravity-cli/brain/<uuid>/.system_generated/logs/transcript.jsonl`（及 `transcript_full.jsonl`、`chunks/**`）与 `antigravity-cli/conversations/<uuid>.db`（SQLite，9 个 db + 8 wal + 8 shm，未解析）。

| 键路径 | 出现行数 | 为 0 | 非零 | 求和 |
|---|---|---|---|---|
| `input_tokens` | 3632 | 0 | 3632 | 19522889 |
| `cache_read_tokens` | 3632 | 132 | 3500 | 454782430 |
| `output_tokens` | 3632 | 0 | 3632 | 2039907 |
| **total 类键** | **0 行 / 0 个键路径** | — | — | — |
| 「total 存在且全部分量 0」的行 | **0** | — | — | — |

行类型分布：`PLANNER_RESPONSE` 3632、`GENERIC` 3331、`SYSTEM_MESSAGE` 378、`CHECKPOINT` 11、`USER_INPUT` 16、`slash_command` 6。token 字段只出现在 `PLANNER_RESPONSE`。

- **结论**：本机 Gemini 侧语料（Antigravity 形态）**不存在任何 total 类 token 字段，也不存在「只写 total、分量全 0」的 usage 行（0 行）**；其分量字段 `input_tokens` / `output_tokens` 在 3632 行上 **100% 非零**（仅 `cache_read_tokens` 有 132 行为 0）——A11 的「只写 total / 分量全 0 → $0」风险在本机 Gemini 语料上不成立；但该语料**不是经典 Gemini CLI**，经典 `~/.gemini/tmp/**/chats/*.json` 形态在本机无样本可验。
- **方法**：`tokenfield-scan.js` 对 207 文件做 `/token/i` 键路径挖掘（键路径 → 出现/零/非零/求和），并显式判定「存在 total 类键且分量与 total 皆 0」的行数；目录形态用只列路径的方式确认。

---

## 10 其他 provider（`.zcode` / `.workbuddy` / `.codebuddy`）

- **判定**：已判定（形态层；只列路径形态与配置键名，未读取会话内容）
- **统计结果**

路径形态（均为模板化外形）：

- `~/.zcode/cli/rollout/model-io-<sess-id>.jsonl`、`~/.zcode/cli/rollout/model-io-<sess-id>_subagent_agent-<id>.jsonl` —— **3 文件 / 97 行 / 17.66 MB**，行类型全部 `model_io`；另有 `~/.zcode/cli/agents/<sess-id>/agent-<id>/{metadata.json,output.txt,task.output}`（110 文件）与 `~/.zcode/cli/{db,log,exec,plugins,image-cache}`。
- `~/.workbuddy/sessions/<pid>.json`（0.4–0.5 KB 级）、`~/.workbuddy/workspace/sessions/`、`~/.workbuddy/{logs,audit-log,tasks,app/session,blobs,cache,…}` —— 70248 文件 / 2099.4 MB。
- `~/.codebuddy/logs/memwatch/cli-memwatch-<pid>.log` —— **24 文件全部是这类 memwatch 日志**，无会话日志、无 DB、无配置目录。

可解析用量字段（仅记录键名，未记录任何值）：

- zcode `model_io` 行含 `response.usage.{inputTokens, outputTokens, totalTokens, cacheReadTokens, cacheWriteTokens, reasoningTokens}`（前 4 个各 93 行、`cacheWriteTokens` 87 行、`reasoningTokens` 仅 6 行）与 `response.providerMetadata.anthropic.usage.{input_tokens, output_tokens, cache_read_input_tokens}`（87 行）；`response.providerMetadata.anthropic.cacheCreationInputTokens` 87 行存在但**非数值**，`response.usage.cacheWriteTokens` **87/87 行全为 0**。「total 与分量全 0」的行 0 行。
- Claude 系 fork 的 `settings.json` 与 `SessionEnd` hook：

| provider | `settings*.json` | `hooks` 键 | `hooks.SessionEnd` |
|---|---|---|---|
| `~/.workbuddy/settings.json` | 存在（另有 15 个 plugins 内 settings） | **不存在**（顶层键：`sandbox, enabledPlugins, claw, autoLaunchDesired, officeFileAssociationsRepairMarker, autoLaunchDefaultApplied, officeMarkdownDefaultClaimAttempted, pluginConfigs`） | 不存在 |
| `~/.codebuddy` | **无** | — | — |
| `~/.zcode` | **无** | — | — |

- **结论**：三个 provider 中只有 **zcode 存在可解析的会话级 JSONL 用量日志**（`cli/rollout/model-io-*.jsonl`，含 `response.usage.*` 与 anthropic 侧 `usage.*` 两套并行字段；其 `cacheWriteTokens` 恒 0，与 §5 的 A2 属同类形态），workbuddy 为「按 PID 命名的会话目录 + 大体量 blobs」结构、codebuddy 只有 memwatch 日志（均无 token 用量字段可采）。**本机三处均未发现 `SessionEnd` hook 配置**：workbuddy 的 `settings.json` 根本没有 `hooks` 键，codebuddy/zcode 连 `settings.json` 都不存在——「CodeBuddy/WorkBuddy 是否真消费 `SessionEnd`」在本机**无任何配置证据**，不应据此推断其支持该 hook。
- **方法**：`Get-ChildItem -Recurse` 只列路径形态与文件数/大小；`hooks-probe.js` 只解析 `settings.json` 的顶层键名与 `hooks` 子键名（不输出任何配置值）；`tokenfield-scan.js` 对 zcode 的 3 个 rollout 文件（97 行）做 token 键挖掘。

---

## 统计口径与可复现清单

- **扫描范围**：Claude 22 jsonl / 4609 行；Codex 64 jsonl / 179776 行 / 1272.1 MB；Gemini 207 文件 / 8052 行；zcode rollout 3 文件 / 97 行。全部为**全量**（非抽样），解析失败行如实计数（Claude 0、Codex 0、Gemini 541 来自多行 `.json`）。
- **临时脚本**（`%TEMP%\tt-w3\`，用完即弃，未入任何仓库）：`probe-keys.js`（键结构探测）、`codex-scan.js` / `codex-scan2.js` / `codex-scan3.js` / `codex-scan4.js` / `codex-scan5.js`、`claude-scan.js` / `claude-scan2.js`、`tokenfield-scan.js`、`hooks-probe.js`。
- **交叉验证**：Codex 的关键数字（配对总数 65381、`reasoning>0` 60715、`service_tier` 32 文件/433 事件/default 385+priority 48、模型一致性 20/21）由两轮独立扫描各自复算一致；Claude 的 1520 行 assistant usage 由两轮独立脚本（全量口径 / 分组口径）一致。
- **隐私纪律执行**：仅统计计数、字段存在性、数值分布与大小关系；未记录、未复述任何会话文本、代码、模型串或项目名（路径与 UUID 在报告中已模板化）。
