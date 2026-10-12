# WorkBuddy（国内版）会话存储与用量记账核验（候选适配器前置研究）

> 只读核验：全部结论来自对**本机已安装程序**（WorkBuddy 5.7.7）留下的持久化数据
> `~/.workbuddy` 的静态分析，未写入、未移动、未清理任何被扫描数据；`workbuddy.db` 一律以
> `file:...?mode=ro` 只读 URI 打开。探针脚本只在系统临时目录运行，不入库。
>
> 文档只保留结构化字段名、表名、聚合计数与口径判断，**不含真实会话 ID、消息 ID、项目路径、
> 提示词、工具输出或模型输出的任何内容**；涉及路径的示例一律符号化。
>
> 结论分三档标注：**已验证**（本机实测或迁移 SQL 可证）／**一致解释**（多条证据指向同一
> 原因，无单点直证）／**未验证推论**（待新版本或补充样本复核）。
>
> **与同目录 [`2026-10-11-workbuddy-ai-session-storage.md`](./2026-10-11-workbuddy-ai-session-storage.md)
> 的关系**：本机并存两份独立安装的数据目录——`~/.workbuddy`（国内版，5.7.7，15 个项目
> slug、23 个本地会话、42 个转写文件）与 `~/.workbuddy-ai`（5.7.6，1 个项目 slug、1 个会话），
> 两侧会话 ID **零交集**。前篇是 `~/.workbuddy-ai` 的单会话深挖，本篇是 `~/.workbuddy` 的
> 多会话横向复核，并补足前篇标注为「未验证」的 slug 多形态、cloud 会话正文、credit 语义、
> 以及前篇未覆盖的**子 agent 文件**。公共格式（§3、§4、§5.1）为单篇自足而完整列出，与前篇
> 重复的断言只给结论与样本量，差异项另行标注。

## 1. 结论摘要

1. WorkBuddy 国内版的会话转写存放在 `~/.workbuddy/projects/<项目 slug>/<sessionId>.jsonl`，
   是**逐行追加 JSONL**；注入的 OTel 上下文使用 `codebuddy.*` 前缀（CodeBuddy 血统）。
2. **正文与索引分离**：JSONL 是内容唯一事实源；`~/.workbuddy/workbuddy.db`（SQLite）只存
   会话元数据（标题 / 模型 / cwd / 状态 / transport）与用量汇总，**不存正文**。
3. **一次 LLM 调用恰好一行、恰好一个 `providerData.messageId`**（2233 行实测零重复，
   跨文件亦全局唯一），该键即天然去重身份；`conversationRequestId` 是**用户提交粒度**
   （本机全量仅 167 个），按它计请求数会把约 13.4 次调用并成 1 次。
4. 每行并列三处 usage（`providerData.rawUsage` / `providerData.usage` / `message.usage`），
   **取值恒等**（2233/2233），任取一处即可；三处同时累加即三倍虚增。
5. `providerData.rawUsage.credit` 是服务端返回的**费用点数**（非估算）；会话级汇总在
   `workbuddy.db` 的 `session_usage.credit_json`，两者本机 **9/11 会话逐键恒等**，
   合计 3 处差额（2 处可比对会话 db 偏高 + 1 处 JSONL 侧全 0 而 db 非 0），
   **两种来源不可互相替代**（§5.5、§7）。
6. **子 agent 独立成文件、独立记账**：`<sessionId>/subagents/agent-<8|16hex>.jsonl`
   （**19 个文件中 12 个为 16 位 hex、7 个为 8 位**，发现逻辑必须按 `agent-*.jsonl` 匹配，
   不能用固定 16 位正则），与父文件 `messageId` 零交集；父会话 db 的 credit **不并入**
   子 agent 信用点。漏采该目录会漏掉约两到三成调用（三个含子 agent 的会话实测
   29.3% / 20.7% / 18.9%）。
7. **云端会话本地无正文**：db 中 `transport = 'cloud'` 的会话（首次快照 30 个、复核时 31 个，
   数量随 App 侧活动持续增长）在本地没有任何 JSONL；前篇该条「未验证推论」在本机被否证
   （正文不落本机）。
8. 跨版本稳健：对 `~/.workbuddy-ai`（5.7.6）单会话 656 条独立样本复跑，§5 的三条
   不变量（一行一调用、三处恒等、hit+miss=prompt）**全部复现**。
9. **第三个用量口径兼正文副本的警示**：`traces/<pid>/trace_*.json`（本机 181 个文件、13 个 PID
   目录、合计约 93 MiB，比 `projects/` 里的转写正文总量还大）的 `trace.modelInfo` 携带逐进程
   用量聚合（`totalInputTokens` / `totalOutputTokens` / `totalCachedTokens` / `callCount`）与
   `sessionId`；`spans[]` 中 `type = "generation"` 的条目还带 `toolInput` / `toolOutput`，
   **含完整提示词与模型响应**（全量 758 个 generation span 中 756 个带载荷）。它与 JSONL 的
   重叠度**未统计**，采集前必须先判定，重叠则不得同时采集（§3、§11）。

## 2. 方法与样本

- 主数据源：`~/.workbuddy` 只读扫描（Python `sqlite3` + 文本读取）。**规模随使用增长**：
  首次快照约 2.1 GB / 9.4 万文件，复核时（2026-10-11 15:40）为 2303 MiB / 96,989 文件。
  体积集中在 `binaries` 733 MiB、`logs` 687 MiB、`plugins` 238 MiB、`workspace` 233 MiB、
  `traces` 93 MiB；**会话正文（`projects/**/*.jsonl`）只有 41.9 MiB**。
- 会话样本：`projects/**/*.jsonl` 共 **42 个**（23 个主会话 + 19 个子 agent），
  单文件最大 16.96 MB / 3475 行，最小 0 行（空文件亦存在）；全量 **2233 条**带用量行。
- 交叉样本：`~/.workbuddy-ai`（前篇样本，5.7.6）单会话 2384 行 / 656 条带用量行，
  用于跨版本复核。
- 迁移权威文本：`~/.workbuddy/.workbuddy-sqlite-migrations/*.sql` 与 `meta/_journal.json`
  （drizzle 账本 18 条），账本编号 `0012_unify_sessions_cloud` 解释了 `transport` 列的来源
  （磁盘上另有同名 0011 遗留文件，见 §6）。
- 时间跨度：本机 `~/.workbuddy` 全部事件 `2026-08-18T03:13:04Z` 至 `2026-10-10T18:18:23Z`。
- 局限：单机单用户；`transport = 'cloud'` 会话无本地正文，其字段语义无法核验；
  遗留迁移（`migration_meta` 的 `legacy_*` 系列）多为 `skipped`，旧格式未纳入样本。

## 3. 存储分层（已验证）

| 路径 | 角色 | 是否承载正文 |
| --- | --- | --- |
| `projects/<slug>/<sessionId>.jsonl` | **会话转写（唯一事实源）** | 是 |
| `projects/<slug>/<sessionId>/subagents/agent-<8|16hex>.jsonl` | 子 agent 转写；**仅 3/23 会话有**，共 19 个文件（12 个 16 位 hex + 7 个 8 位 hex） | 是 |
| `projects/<slug>/<sessionId>/tool-results/call_*.txt` | 超长工具结果外置；**仅 2/23 会话有**，共 2 个文件（71187 / 60467 字节） | 是（正文延展） |
| `projects/<slug>/<sessionId>.meta.json` | 会话级元数据（`hostKind`、`acpConnectionId`）；**16/23 会话有** | 否 |
| `projects/<slug>/<sessionId>.file-rollback.ndjson` | 文件改动提交点（回滚账本）；**20/23 会话有** | 否 |
| `workbuddy.db` | 会话索引 / 用量汇总 / 工作区 / 自动化 | 否 |
| `sessions/<pid>.json` | 进程级注册表（PID 命名、不清理，**不是**活跃会话清单） | 否 |
| `traces/<pid>/trace_*.json` | 进程级 trace 汇总（`trace` + `spans`，含逐进程用量聚合）；本机 **181 个文件 / 13 个 PID 目录 / 约 93 MiB** | **是**：`generation` span 带 `toolInput` / `toolOutput`，含完整请求与响应载荷（全量 758 个中 756 个，见 §11） |
| `file-history/<sessionId>/<hash>@vN` | 文件快照备份；**版本后缀实测跨 `v1`–`v23`（21 个不同版本号，非固定 `@v2`）**，9 个会话目录共 280 个文件 | 否 |
| `workspace/sessions/<sessionId>/` | 工具改动备份；19 个会话目录（`snapfile` 19、`permission.json` 15、`modify_backup/` 10），同级另有 `gc_config.json` 等 2 个文件 | 否 |
| `tasks/<sessionId>/<n>.json` | 会话级待办；`<n>` 从 1 起且不保证连续，5 个会话共 53 个文件 | 否 |
| `audit-log/YYYY-MM-DD.jsonl` | 安全审计（哈希链，命令 / 文件放行决策）；14 个日文件 + `manifest.jsonl` + `spool/` | 否 |
| `blobs/<前 2 位>/<sha256>.<ext>` | 图片附件（内容寻址）；8 个文件（5 jpg / 3 png） | 附件 |
| `local_storage/entry_*.info` | Electron 侧键值存储 | 否 |

- 表中附属物**全部是可选的**：23 个主会话里只有 16 个带 `.meta.json`、20 个带回滚账本、3 个有
  `subagents/`、2 个有外置工具结果，均不得作为会话存在性或完整性的判据。
- `sessions/<pid>.json` 是**进程级心跳登记**，字段固定为
  `{pid, sessionId, cwd, startedAt, lastHeartbeat, updatedAt, kind, url, endpoint, mode, version,
  os, arch, hostname}`（69/69 文件同构）。`sessionId` 有两种形态：`<uuid>`（20 个，其中 13 个能
  对应到本地会话文件）与 `interactive-<pid>`（49 个，对应交互式进程、无对应会话文件）。
  **它不是活跃会话的可靠清单**：按 PID 命名且不清理，2026-10-11 13:27 观察时 69 个文件里仅
  1 个在 30 分钟内更新（其余最早可追溯到上月）；判定活跃必须叠加新鲜度过滤（文件 mtime 或
  `lastHeartbeat`）。该目录与用量无关。
- `audit-log` 每条带 `sequence` + `prevHash` 构成哈希链（前篇已复算确认算法），内容是
  命令安全 / 文件安全决策，**不承载会话内容**。
- **结论 1（已验证）**：采集用量只需扫 `projects/` 目录树。该结论以「`traces/` 的用量与 JSONL
  重叠」为前提；重叠度尚未判定，故 `traces/` 暂不纳入采集面（§1-9、§11）。
- **结论 2（与前篇的取舍差异，已验证）**：前篇建议「用不到 `workbuddy.db`」；本篇认为应
  **只读打开用于两件事**：会话元数据补充（标题 / 模型 / 状态）、以及 cloud 会话的本机可见性
  （本机 cloud 会话首次快照 30 个、复核时 31 个，本地零正文，完全绕开 db 会让用户看到
  「查无此会话」的空白）。
  实测 `mode=ro` 打开成功且不干扰运行期 WAL；但**不要把它放进扫描热路径**，只在需要
  元数据或做完整性核对时短事务读取。

## 4. 会话正文格式（已验证）

### 4.1 记录类型

逐行一个 JSON 对象，以 `type` 区分。本机全量枚举（42 个文件，计数为行数）：

| `type` | 行数 | 作用 |
| --- | --- | --- |
| `function_call` | 2420 | 工具调用请求（`callId` / `name` / `arguments`） |
| `function_call_result` | 2416 | 工具结果（`toolResult.renderer` / `rawResponse`） |
| `reasoning` | 1574 | 思考内容（`rawContent[].type = "reasoning_text"`） |
| `message` | 1414 | 对话消息（`role = user \| assistant`） |
| `file-history-snapshot` | 884 | 被跟踪文件备份索引（指向 `file-history/`） |
| `ai-title` | 21 | 会话标题（`aiTitle`），UI 列表用 |
| `session-meta` | 10 | 会话启动标记（`meta["codebuddy.ai/hostKind"]`），成对出现 |
| `resend-fork-notice` | 2 | 编辑重发产生的分叉（`editedUserItemId`） |

`message` 的 `role` 分布：`assistant` 1178 / `user` 236；`content[]` 块类型：
`output_text` 1178 / `input_text` 252 / `image_blob_ref` 3。`status` 分布：
`completed` 3551 / `incomplete` 43。

### 4.2 字段与出现率（**无固定 schema**）

逐行 JSON 的字段集**因记录类型而不同**：8 种 `type` 共出现 13 种键组合（仅 `message` 一类
就有 13 种变体）。全量 8741 行的顶层字段出现率如下，解析必须按「字段是否存在」取值，
**不得假设任何固定 schema**：

| 字段 | 全量出现率 | 说明 |
| --- | --- | --- |
| `timestamp` / `type` | 100% | 毫秒 epoch（全量 0 例外非整数）／记录类型 |
| `cwd` | 99.9%（8731/8741） | 缺失的 10 行即 `session-meta`；形如 `c:\Users\...`（**小写盘符 + 反斜杠，零例外**） |
| `id` | 99.1%（8661/8741） | **三种形态**：带连字符 UUIDv4 **4762 行**、UUIDv7 634 行、**32 字符无连字符 hex 3263 行**，另有 2 行本地生成为 `notice-<ts>-<rand>`；缺失 80 行 = `file-history-snapshot` 的 76 行第二形态（改带 `messageId`）+ `ai-title` 4 行 |
| `sessionId` | 89.9%（7855/8741） | 缺失 886 行 = `file-history-snapshot` 884 + `resend-fork-notice` 2（后者**仍带 `parentId`**，可挂回事件链）；`file-history-snapshot` 连 `parentId` 都没有，归属只能靠所在文件或 `cwd` 推断 |
| `providerData` | 89.5% | 供应商侧元数据（模型、用量、工具渲染等），见 §4.3 |
| `parentId` | 88.8%（7763/8741） | 事件链父节点；缺失 978 行 = `file-history-snapshot` 884 + `message` 63 + `ai-title` 21 + `session-meta` 10；压缩摘要行只有 `logicalParentId`（22 行，前篇已证） |
| `_meta` | **5.9%（513/8741）** | `traceparent` / `baggage`（baggage 内含 `codebuddy.session_id`、`codebuddy.conversation_request_id`）；**绝大多数行没有**，缺失必须能正常解析 |

其余字段为类型专属（`role` 16.2%、`status` 41.1%、`callId` / `name` 55.3%、`content` 34.2%、
`arguments` 27.7%、`output` 27.6%、`rawContent` 18.0%、`snapshot` 10.1% 等），见 §4.3。

### 4.3 关键类型的差异字段

- `message`（1414 行）：`content[]`（`input_text` / `output_text` / `image_blob_ref`）。
  行内细分：assistant + `status = completed` 992 行、assistant + `incomplete` 43 行、
  assistant 带顶层 `message` 子对象 143 行、user 236 行（**无 `status`**）。
  `providerData.messageId` 只覆盖 assistant 的 1135 行（user 行没有）；`startsNewUserRequest` /
  `skipRun` / `compactType` 分别只有 25 / 65 / 12 行。
- `image_blob_ref` 块形态：`{type, blob_id, mime, size, blob_path}`（5 键），`blob_path` 指向
  `blobs/<前 2 位>/<sha256>.<ext>`（内容寻址，可直接命中文件）。
- `reasoning`：`rawContent[]` 与 `content[]`（`content` 本机恒为空数组）。
- `function_call`：`callId`、`name`（工具名）、`arguments`（JSON **字符串**）、
  `providerData.argumentsDisplayText`（人类可读摘要）、`providerData.reasoning`。
- `function_call_result`：`callId`、`name`、`status`、
  `providerData.toolResult.{title, renderer, rawResponse}`；
  `rawResponse` 携带 `exitCode` / `signal` / `interrupted` / `sandboxDenied` /
  `stdoutBytesTruncated` / `stderrBytesTruncated` / `tool_error_code` / `is_error`。
- `file-history-snapshot`（884 行，**两种形态**）：808 行带 `id`，76 行不带 `id` 而带 `messageId`
  与 `_meta`。主体为 `snapshot.trackedFileBackups[路径] = {version, backupTime,
  existedAtTrack, backupFileName}`，`backupFileName` 即 `file-history/<sessionId>/` 下的文件名。
- 本机工具名分布（前 6）：`Bash`、`Edit`、`Read`、`Write`、`PowerShell`、`WebFetch`。

### 4.4 项目 slug 规则（补足前篇的单样本缺口，已验证）

```
slug(cwd) = cwd.replace(/[:\\]+/g, "-")
```

**盘符 `:\` 序列折叠为一个 `-`**（等价实现：删 `:` 后将 `\` 替换为 `-`，本机数据
无法区分两种写法），其余 `\` 各变一个 `-`；中文、空格、下划线、字母大小写**全部
原样保留**，不做非字母数字字符的兜底替换。本机 15 个 slug 目录覆盖了以下形态，
全部符合该规则（2026-10-11 复核更正：初稿公式误写为 `\` 与 `:` 各替换为一个 `-`，
按该公式推导 15/15 目录全部对不上——实际目录名形如 `c-Users-admin-…`，冒号与紧跟
的首个反斜杠只产生一个 `-`）：

| 形态样本 | slug 表现 |
| --- | --- |
| 纯 ASCII 路径（`Users` / `admin` 等大写保留） | 大小写原样 |
| 含中文目录名 | 中文原样，未转义 |
| 含空格目录名 | 空格原样（**未**替换为 `-`） |
| 含下划线的目录名 | 下划线原样 |
| 自动生成的工作目录（时间戳命名） | 连字符原样 |

前篇标注「含空格、盘符大小写混合形态未验证」，本篇以 **14 个可验证样本**确认（15 个 slug 目录
中有 1 个只含 0 字节空文件、取不到 `cwd`）：**唯一变换是盘符处的 `:\` 序列折叠为一个 `-`、
其余 `\` 各变一个 `-`**，与 Claude Code 的「非字母数字全替换」规则不同。

JSONL 内 `cwd` 为小写盘符 + 反斜杠（8731 行零例外），`workbuddy.db` 的 `sessions.cwd`
为大写盘符、且 23 个本地会话中 21 个为反斜杠、仅 2 个为正斜杠（2026-10-11 复核更正：
初稿称 db 侧为「大写盘符 + 正斜杠」不成立）：**同一路径的系统性差异主要是盘符
大小写**，归属判定必须先规范化。

## 5. 用量记账口径（核心）

### 5.1 三处 usage 与取值恒等（已验证）

同一行并列三个容器，键名体系不同、数值相同（逐行比对 2233/2233 零分歧）：

| 容器 | 键 |
| --- | --- |
| `providerData.rawUsage` | `prompt_tokens` / `completion_tokens` / `total_tokens` / `prompt_tokens_details.cached_tokens` / `completion_tokens_details.reasoning_tokens` /（旧形态另有）`prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` / `prompt_cache_write_tokens` / `completion_thinking_tokens` / `credit` / `cached_tokens` |
| `providerData.usage` | `requests` / `inputTokens` / `outputTokens` / `totalTokens` / `inputTokensDetails[].cached_tokens` / `outputTokensDetails[].reasoning_tokens` |
| `message.usage` | `input_tokens` / `output_tokens` / `total_tokens` / `cache_read_input_tokens`（**可能缺失**：本机 2233 行中 901 行没有该键） |

**铁律：三处只取一处。** 同时累加三处会让用量与费用三倍虚增。

### 5.2 一行一次调用（已验证）

- 带 usage 的行共 **2233** 行，其中 `providerData.messageId` 恰为 **2233 个不同值**，
  「同一 messageId 出现在多行」的次数为 **0**；交叉样本（5.7.6）同样为 656/656。
- 身份键取 `(sessionId, providerData.messageId)` 即可幂等去重。
- **采集必需字段在这 2233 行上零缺失**（实测均 100%）：`id` / `parentId` / `timestamp` /
  `sessionId` / `cwd` / `providerData.{messageId, model, requestModelId, requestModelName,
  conversationRequestId, rawUsage, usage}`。即 §4.2 的字段缺失不影响适配器的必需路径。
- usage 行在类型上分布在 `function_call` 与 `message` 两类（本机 2090:143 ≈ 14.6:1，
  交叉样本 650:6；2026-10-11 复核更正，初稿误写 69:1），**不要假设只挂在
  `message` 上**；判定条件应是「该行是否带 `providerData.rawUsage`」。

### 5.3 两代形态与缓存字段（已验证）

| 形态 | 行数 | 特征 | 缓存读取值 |
| --- | --- | --- | --- |
| 旧形态 | 1291 | 带 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` / `credit` | `prompt_tokens_details.cached_tokens` **恒等于** `prompt_cache_hit_tokens`（1291/1291） |
| 新形态 | 942 | 无 hit/miss 与 credit 字段 | 100% 带 `prompt_tokens_details.cached_tokens`（其中 52 行 >0） |

- `prompt_cache_hit_tokens + prompt_cache_miss_tokens == prompt_tokens` 在旧形态
  **1291/1291 成立**（交叉样本 656/656 亦然）→ `prompt_tokens` 是**含缓存的输入总量**
  （OpenAI 语义），与 Claude 的互斥桶口径不同。
- 顶层 `rawUsage.cached_tokens` **只在旧形态 1291 行存在且恒为 0，新形态 942 行完全没有该
  键**，任何形态下都不可用作缓存读。
- `message.usage.cache_read_input_tokens` 同样**不可替代缓存真值**：本机 2233 行中 901 行缺该
  键（旧形态 11 行 + 新形态 890 行）；在同时具备该键与 `prompt_cache_hit_tokens` 的 **1280 行
  里二者恒等**，但新形态另有 52 行带该键却无 hit 键可交叉校验。
- **形态判定与成因**：判定条件取「该行 `rawUsage` 是否含 `prompt_cache_hit_tokens`」；
  两代形态的成因（模型通道差异还是版本差异）**未验证**——本机样本只确认字段集的分布，
  未确认驱动因素。
- `rawUsage.cache_read_input_tokens` / `cache_creation_input_tokens` /
  `prompt_cache_write_tokens` 同样只存在于旧形态 1291 行，取值**恒为 0**
  （前篇已在另一 provider 下确认同一现象）。
  即：**Anthropic 风格字段不可用，缓存真值只在 `prompt_tokens_details.cached_tokens`。**

**接入铁律（桶归一）**：

| TokenScope 桶（互斥） | WorkBuddy 来源 |
| --- | --- |
| `input_tokens` | `prompt_tokens - prompt_tokens_details.cached_tokens`（下限 0） |
| `cache_read_input_tokens` | `prompt_tokens_details.cached_tokens`（缺失时回退 `prompt_cache_hit_tokens`） |
| `cache_creation_input_tokens` | `cache_creation_input_tokens` 原值（本机恒 0） |
| `output_tokens` | `completion_tokens` 原值（`reasoning_tokens` 已含在内，不另加） |

不归一就直接并入 Claude / Codex 汇总会把缓存读**双计一次**。

### 5.4 失败调用与模型名（已验证）

- 43 条 `status = incomplete` 行全部带 `providerData.error`（其中 30 条含 `status`，取值见
  `429` / `499` / `400`；`code` 见 `0` / `6004`；另 13 条仅带 message 类字段），
  且**全部不带 usage**。可安全跳过错误行，不会漏算也不会误计。
- 模型标识取 `providerData.model`，回退 `requestModelName` / `requestModelId`。
  **渠道前缀出现在 `requestModelId` 而非 `model`**：`custom-local:<model>` 占 `requestModelId`
  的 39%（2957/7545），别名写法（如 `balanced-model`）也只在 `requestModelId` 中出现；
  `model` 字段仅 172/7525（2.3%）含冒号。归一必须走既有等价键规则（ASCII 小写 + 删除
  `-` `.` `_`），**不能在适配器内做字符串裁剪**，也不要假设两个字段同值。

### 5.5 credit 与 `session_usage` 的关系（已验证 + 3 处差额登记）

- `providerData.rawUsage.credit` 为**服务端返回的费用点数**（非 TokenScope 估算值），
  仅旧形态行携带，可以为 0。前篇样本（免费版账号）656 行**全为 0**；本机国内版
  出现非 0 值，可用于直接取费用，无需估算。
- `workbuddy.db` 的 `session_usage.credit_json` 是**以 `conversationRequestId` 为键的
  会话级累加**（32 位 hex，无连字符；与 `providerData.baggage` 里的同名 ID 同构）。
- 逐键闭合实测：可比对的 11 个会话中 **9 个逐键精确相等**（容差 1e-9），键级 **41/43**。
- 三处差额（前两处来自可逐键比对的 11 个会话，第三处因 JSONL 侧全 0 未进入该集合）：
  - 会话 A：db 单键 `4.18` vs JSONL 同键 `2.27`，差 `1.91`；该会话存在一个子 agent，
    其 credit 恰为 `1.91` 但**键名不同**（且不在父会话 `credit_json` 中）。**一致解释**：
    差额源于子 agent 计费并入父会话，但键改写方式未定。
  - 会话 B：db 单键 `322.00` vs JSONL 同键 `292.59`，差 `29.41`；该会话无子 agent 文件。
    **未验证推论**：差额来自未写入 JSONL 的请求（写入失败 / 历史版本重跑）。
  - 会话 C：JSONL credit 全 0 而 db 记 `5.71`（`updated_at` 2026-09-09）——因 JSONL 侧全 0
    未进入「可比对」集合；按 §9-5 对无 credit 的行以「费用未知」处理即可覆盖。
- **两种来源不可互相替代**：db 无逐请求粒度，JSONL 也可能落后于 db 账本。

## 6. 会话索引库（`workbuddy.db`，已验证）

baseline schema 见 `.workbuddy-sqlite-migrations/0000_workbuddy_sqlite_baseline.sql`；
后续 18 条迁移逐列追加（会话侧关键列：`plugin_context_json`、`addon_selection`、
`session_settings`、`context_window`、`thought_level`、`unread`、
`transport` / `conversation_origin` / `visibility` / `group_id` / `agent_*` / `verified_at`）。
迁移 `0012_unify_sessions_cloud`（账本实际生效编号）把原 `cloud_conversations` 表整体
并入 `sessions` 并 drop 旧表，`transport` 默认 `'local'`。磁盘上另有一份字节完全相同的
`0011_unify_sessions_cloud.sql` 遗留文件，但不在 drizzle 账本中（2026-10-11 复核更正：
初稿按磁盘文件名写作 0011；与姊妹篇 5.7.6 的 0012 实为同一迁移）。

| 表 | 行数（本机） | 说明 |
| --- | --- | --- |
| `sessions` | 53（复核时 54，随云侧活动增长） | 会话索引：`cwd` / `title` / `custom_title` / `status` / `created_at` / `updated_at` / `last_activity_at` / `deleted_at` / `model` / `mode` / `permission_mode` / `context_window` / `thought_level` / `transport` / `conversation_origin` / `visibility` 等 |
| `session_usage` | 16 | `session_id` 主键，`used` / `size`（上下文占用与窗口）+ `credit_json` |
| `workspaces` | 4 | `path` 主键 + `last_opened_at` |
| `workspace_trust_decisions` | 6 | `workspace_key` 主键 + `decision ∈ {trusted, rejected}` |
| `automations` / `automation_runs` / `automation_runtime_state` / `automation_delivery_outbox` | 0 | 定时自动化（本机未用） |
| `buddy_snapshots` | 0 | 助手快照 |
| `migration_meta` | 16 | 遗留迁移账本（多为 `skipped`） |
| `__workbuddy_drizzle_migrations` | 18 | drizzle 迁移账本 |

- **`transport` 分布**（会话维度，**随 App 侧活动增长，不是稳定常量**）：首次快照 `sessions`
  53 行 = `cloud` 30 + `local` 23；复核当晚已增至 54 行（`cloud` 31）。`cloud` 会话全部
  `status = completed`、`cwd` 为空串、标题为 App 侧自动化对话；`local` 为 16 个 `completed`
  + 7 个 `archived`。
- **本地 JSONL 与会话一一对应**：23 个主会话文件全部在 `sessions` 表中，**无孤儿文件**；
  `cloud` 会话（首次快照 30、复核时 31）本地无任何 JSONL。
- `deleted_at` 三态（实测）：`-1` = 未删除（**31 个云会话全部如此**）、`NULL` = 未标记（18 个
  本地会话）、**时间戳 = 已标记删除（5 个本地会话）**。`idx_sessions_cloud_*` 共 4 个索引，
  其中 `cloud_sort` / `cloud_group` / `cloud_project` 的过滤条件为
  `WHERE transport = 'cloud' AND deleted_at = -1`，`cloud_verified` **只有** `WHERE transport = 'cloud'`。
- **归档 ≠ 删除**：7 个 `archived` 会话的 `deleted_at` 全是 `NULL`（归档只改 `status`）；反过
  来，5 个已标记删除的会话 `status` 仍是 `completed`，且其 `deleted_at` 精确等于 `updated_at`。
- **删除标记不删文件**：上述 5 个会话的 JSONL 全部留在原位、含完整用量行（合计 8 条 usage）。
  对采集的含义：这些已删除会话**仍会被扫到并计入**，是否过滤属产品决策（建议默认计入并在
  UI 标注，与既有「来源删除不删已保存用量」原则同向）。
- **`session_usage.used` 语义（补足前篇的会话语义确认）**：本机 16 个有 `session_usage` 行的
  会话中，**14 个的 `used` 精确等于该会话最后一条用量行的 `prompt_tokens`**
  （与 `message.usage.input_tokens` 恒等）。两个例外：
  - 一个被压缩过的会话 `used = 87538` vs 末条 `89257`（差 `-1719`，**未验证推论**：
    压缩后占用统计的更新时机与末条事件不同步）；
  - 一个 playground 会话 `used = 0 / size = 0`（**未验证推论**：该形态未上报上下文占用）。
  结论：`used` 是**上下文占用条**而非累计用量，**不得**作为用量事件写入历史库；
  若要复现该值，取「末条用量行的 `prompt_tokens`」是 14/16 可靠的近似，但**不应依赖**。

## 7. 交叉验证记录

| 断言 | 证据 | 结论 |
| --- | --- | --- |
| 一行 = 一次调用 | 本机 usage 行 2233，唯一 messageId 2233，多行共享 0；交叉样本 656/656 | 已验证 |
| 三处 usage 恒等 | 逐行比对 2233/2233 与 656/656，零分歧 | 已验证 |
| hit + miss = prompt | 旧形态 1291/1291；交叉样本 656/656 | 已验证 |
| 缓存读回退可用 | 新形态 942 行 100% 带 `prompt_tokens_details.cached_tokens` | 已验证 |
| 顶层 `cached_tokens` 是死字段 | 旧形态 1291 行存在且恒 0；新形态 942 行无此键 | 已验证 |
| 错误行不计费 | 43 条 error 行全部不带 usage | 已验证 |
| 主子不重账 | 3 个含子 agent 的会话，主与子 usage messageId 交集均为 0 | 已验证 |
| credit 与 db 闭合 | 9/11 会话逐键相等，键级 41/43 | 已验证（含 3 处差额，见 §5.5） |
| 会话文件追加不重写 | **全量 42 个文件**：39 个 timestamp 单调；3 个存在回退（共 5 处，最大 6396 ms）。行序回退集中在压缩摘要与元数据的补写位置，usage 行口径下回退为 0。文件 mtime 与末条事件时间差 +0.001 ~ +0.052 s（22 个非空文件；1 个空文件无法比较） | 已验证（结论不变，量级更正） |
| 实时落盘（追加） | 会话进行中 JSONL 持续追加，mtime 紧贴末条事件时间 | 已验证 |
| `sessions/<pid>.json` 非活跃清单 | 69 个文件（字段同构），`sessionId` 为 `interactive-<pid>` 的 49 个不对应任何会话文件；2026-10-11 13:27 观察时仅 1 个文件在 30 分钟内更新 | 已验证（原「同步推进」判断已更正） |
| 云端会话无正文 | cloud 会话（首次快照 30、复核时 31）本地零 JSONL | 已验证 |
| `used` = 末次 `prompt_tokens` | 16 个会话中 14 个精确相等（2 个例外已登记） | 已验证（近似 14/16） |
| 两份数据目录互不相干 | `~/.workbuddy` 23 个会话与 `~/.workbuddy-ai` 1 个会话 ID 零交集 | 已验证 |

## 8. 会话生命周期与写入语义

- **追加式写入**：会话进行中持续追加一行一事件；压缩（`compactType` 取
  `emergency-auto` / `user-command`）**不重写文件**，而是插入带
  `isSummary` / `isCompacted` / `isCompactInternal` 标记的摘要行，历史行保留
  （前篇已在 5.7.6 样本上证明该摘要行为单行大对象、只有 `logicalParentId`）。
- **分叉**：编辑重发写 `resend-fork-notice`（`editedUserItemId` 指向被编辑的用户消息），
  后续事件经 `logicalParentId` 重挂载。分叉产生的是**真实的新调用**，不构成重复计费。
- **子 agent**：`providerData.isSubAgent` 为真（本机 726 行），落在
  `<sessionId>/subagents/agent-<8|16hex>.jsonl`，**必须单独采集**；三个含子 agent 的会话实测
  子占比为 29.3% / 20.7% / 18.9%（主 70/子 29、主 330/子 77、主 23/子 6），漏采即漏掉这一
  区间的调用。子 agent 目录内**只有 `.jsonl`**，没有伴随的 `.meta.json`（与 Qoder CN 不同）。
- **超长工具结果外置**：`<sessionId>/tool-results/call_*.txt`（本机共 2 个，71187 与 60467 字节），
  JSONL 内保留摘要与渲染信息。
- **首行非固定**：39 个文件首行为 `message`，2 个为 `session-meta`，1 个为空文件。
  解析不得假设首行是元数据头。
- **单文件单会话**：未观察到分片文件（`.part` 之类）。迁移账本中的
  `session_fragment_repair_done*` 标记与旧版本「按片保存」的修复流程对应（前篇已由
  迁移日志佐证），当前版本为单文件整篇。

## 9. 对 TokenScope 的落地建议

1. **采集来源**：扫 `%USERPROFILE%\.workbuddy\projects`，发现规则为
   `*/<uuid>.jsonl` 加 `<uuid>/subagents/agent-*.jsonl`；目录不存在时静默零结果。
   `traces/` **不纳入采集面**（与 JSONL 的重叠度未判定，见 §11）。
   两份数据目录（`.workbuddy` 与 `.workbuddy-ai`）是**两个独立来源**，各自的会话 ID
   不重叠，可同时采集但要在来源维度上区分，不可假设互斥。
2. **用量主口径**：只累加「带 `providerData.rawUsage`」的行，三处容器只取一处；
   桶归一按 §5.3 铁律执行（`input = prompt - cached`）。
3. **身份键**：`(sessionId, providerData.messageId)`；行序 + 内容指纹作为文件级幂等兜底。
4. **子 agent**：独立文件独立采集，不与父会话合并去重（messageId 已全局唯一）；
   展示上可按父 `sessionId` 归组。
5. **费用**：逐请求费用取 `rawUsage.credit`（旧形态行）；若采用会话级
   `session_usage.credit_json`，需明确披露「口径为服务端累加、可能高于事件副本」。
   两代形态中无 credit 的行应标为「费用未知」，**不得按 0 处理**。
6. **缓存与未知**：`cache_creation` 桶本机恒 0；未收录模型仍走既有「未知不按 0」路径，
   不得用 credit 反推单价。
7. **云端会话**：`transport = 'cloud'` 会话本地无事件，UI 需明确披露为
   「仅统计本机记录」，否则用户会认为漏算。
8. **时间口径**：`timestamp` 为毫秒 epoch（UTC 语义），存储层转 ISO8601 UTC 一次，
   后续沿用既有单次时区转换约定。
9. **路径归一**：JSONL 内 `cwd`（小写盘符 + 反斜杠）与 db 内 `cwd`（大写盘符，
   21/23 为反斜杠）、以及 slug 目录名（`:\` 序列折叠为一个 `-`）三者形态不同，
   必须走同一规范化路径。
10. **不要采集**：`session_usage.used`（上下文占用）、`file-history` 与
    `changes-detail`（可回滚的文件正文，含用户代码）、`audit-log`（安全审计）。
    转写文件本身含完整对话与工具输出，属敏感数据，**测试 fixture 必须手工合成**。
11. **风险提示（三处）**：a) 三处 usage 重复累加导致三倍虚增；b) 漏采 `subagents/`
    导致调用数偏低；c) 误用 Anthropic 风格的 `cache_read_input_tokens`（旧形态恒 0）
    或顶层 `cached_tokens`（旧形态恒 0、新形态无此键）导致缓存读丢失、费用高估。
12. **活库读取**：`workbuddy.db` 常驻 `-wal` / `-shm`，且复核时 **WAL（655,112 字节）大于主库
   （204,800 字节）**，最新写入尚未 checkpoint。必须按「带 WAL 的活库」处理：以只读 URI 打开
   并接受读到 WAL 内数据；**不要只拷主库文件**，也不要假设主库即全量。

## 10. 风险与未知

- **version 漂移**：本结论基于 WorkBuddy 5.7.7（`~/.workbuddy`）与 5.7.6
  （`~/.workbuddy-ai`）。`rawUsage` 存在两代字段形态（旧形态带 hit/miss 与 credit、新形态不带，
  成因未验证，见 §5.3）；实现前应对新版本重跑 §7 的断言。
- **credit 差额未解**：三处差额中，会话 A（`1.91`）有一致解释（子 agent 计费并入父会话），
  会话 C 属 JSONL 侧全 0、db 侧非 0 的口径差；仅会话 B 的 `29.41` 无对应事件解释，
  可能来自写入失败或历史重跑，在采用 `credit` 作费用口径前需补样本确认。
- **压缩边界**：`isSummary` / `isCompacted` 行的 usage 是否代表一次真实完整调用
  未单独核验；本机仅 12 行标记为 `isCompacted`，影响面小。
- **`used` 的两个例外**未定向归因（压缩后不同步、playground 不上报），均属推断。
- **敏感文件提示**：`~/.workbuddy/models.json` 以**明文**保存自定义模型端点的 `apiKey`
  （本机非空），`~/.workbuddy-ai/models.json` 为空数组。该文件属敏感数据，TokenScope 的
  只读原则下**不应纳入扫描面**，也不应出现在任何导出物中。
- **云侧字段**（实测非空率）：`visibility` 31/54、`group_id` 25/54、`conversation_origin` 31/54、
  `verified_at` 31/54、`agent_last_synced` **0/54**。即并非「大面积为空」，只有 `agent_last_synced`
  全空；多端同步与云端会话正文的获取方式未采样。

## 11. 待办与未覆盖

- credit 差额的补样本核验（重点：存在子 agent 的会话，比对子 agent credit 与父 db 差额）。
- 新版本（> 5.7.7）字段形态复核：`rawUsage` 是否新增键、「两代形态」是否继续分化。
- `traces/<pid>/trace_*.json` 全量实测（**2026-10-11 更正：此前仅抽查单个文件，规模被严重低估**）：
  181 个文件 / 13 个 PID 目录 / 97,707,587 字节（约 93 MiB，超过 `projects/` 转写总量），
  最大单文件 6,017,376 字节、中位 272,127 字节；mtime 跨 2026-09-11 至 2026-10-10；
  span 合计 5420（`custom` 3787 / `function` 782 / `generation` 758 / `agent` 93）；
  trace 仅覆盖 **4 个会话**。结构为 `{trace, spans}`，`trace.modelInfo` 含 `totalInputTokens` /
  `totalOutputTokens` / `totalCachedTokens` / `lastCallInputTokens` / `callCount` 且带
  `sessionId`——即**独立于 JSONL 的第三处用量聚合**（粒度＝单次进程运行）。span 通用字段为
  `{traceId, spanId, parentId, name, type, startedAt, endedAt, duration, status, error}`；
  **`generation` 758 个中 756 个、`function` 782 个中 779 个都带 `toolInput` / `toolOutput`**
  （后者另带 `toolName`），即绝大多数 span 含完整提示词或模型响应，与 JSONL 存在正文层面的
  重叠。另一条实现要点：`trace.modelInfo` **只有 20/181 个文件是 dict**（其余 161 个为 null），
  读取用量聚合前必须判空。它与 JSONL 的重叠度未逐条统计；实现前必须先判定，若重叠则
  **不得同时采集**，且该目录属敏感内容，不应纳入任何导出物。
  **未验证推论**：文件规模随时间变动（观察窗口内最大单文件字节由 7,178,731 变为 6,017,376），
  疑有轮转或重写机制，需在新样本上确认保留策略。
- `tasks/`、`file-history/`、`workspace/sessions/` 与 `changes-*` / `artifact-index` 目录的
  用途只做了结构确认，未做内容级核验；它们与用量无关，暂不纳入采集面。
- 多用户 / 多账号（`sessions.user_id` 出现多个值）场景未采样；本机为单用户。
- 两份数据目录的分工（国内版 vs `.workbuddy-ai`）只由版本号与内容差异推断，
  产品侧的准确命名与用途**未验证**。

## 12. 复核修订记录（2026-10-11）

只读复算确认（要点，均精确到个位复现）：42 个转写文件（23 主 + 19 子）、2233 条
usage 行与 2233 个唯一 messageId、三处 usage 恒等、两代形态 1291/942、
hit+miss==prompt 1291/1291、顶层缓存死字段（旧形态 1291 存在且恒 0 / 新形态 942 无该键）、
43 条错误行全部无 usage、行类型分布逐项一致、slug 目录 15 个、
db 全部表行数与 transport 分布（cloud 30 / local 16+7，为**快照值**，复核时 cloud 已增至 31）、
cloud 会话本地零 JSONL、`used` 14/16（两个例外逐字一致）、credit 闭合 9/11 会话与
41/43 键（差额 1.91 / 29.41 逐字吻合）、子 agent 726 行与主/子 messageId 零交集、
models.json 含非空 apiKey、时间跨度两端逐毫秒一致。

第三轮逐条断言复核：以独立实现重算 **96 条断言**（覆盖正文每个数字与关系），修正后全部与
本文一致、零失败。复核首次运行时 7 条不一致，构成为——3 条 db 快照漂移（`sessions` 53→54、
cloud 30→31、cwd 空 30→31，同一现象）、3 条文档实质错误（修订 11、13）、1 条为复核脚本自身
的比较口径问题（修订 12 所述「11 行不等」即由此误读产生，真值为 0 行不等）。

- 修订 1（实质）：§4.4 slug 公式更正为「`:\` 序列折叠为一个 `-`」；按初稿公式
  推导 15/15 目录全部不匹配。
- 修订 2（实质）：§4.4/§9-9 的 db 侧 `sessions.cwd` 形态更正——21/23 为大写盘符 +
  反斜杠，系统性差异是盘符大小写而非分隔符。
- 修订 3：§6 迁移编号更正为生效的 `0012_unify_sessions_cloud`。
- 修订 4：§5.5 补记第三处 db 偏高差额（JSONL credit 全 0 而 db 5.71）。
- 修订 5：§5.2 usage 行类型比 69:1 更正为 2090:143（≈14.6:1）。
- 修订 6（实质）：§5.3/§7/§9-11 顶层 `cached_tokens` 的存在范围更正——它以「键」的粒度
  只存在于旧形态 1291 行（值恒 0），新形态 942 行根本没有该键；初稿的「2233 行恒为 0」
  不成立（复算：1291/1291 存在、942/942 缺失）。
- 修订 7（实质）：§1-3 请求粒度口径更正——`conversationRequestId` 是用户提交粒度，
  本机全量 2233 次调用仅对应 167 个该 ID（13.4:1），初稿「少算两个数量级」不成立；
  同时补记 messageId 跨文件全局唯一（2233/2233，无重复）。
- 修订 8：credit 差额计数在 §1-5/§7/§9-11/§10 统一为 3 处；§12 记录的复算项另含
  JSONL 带 `cwd` 行数 8731（= 总行 8741 − `session-meta` 10 行，全部小写盘符 + 反斜杠，
  零例外）与迁移账本生效编号 `0012_unify_sessions_cloud`。
- 修订 9（新增事实）：§3/§11 补记 `traces/<pid>/trace_*.json` 的内容级抽查结果——该文件是
  进程级 trace 汇总（`{trace, spans}`），`trace.modelInfo` 携带逐进程用量聚合与 `sessionId`，
  属第三处用量口径；与 JSONL 的重叠度未统计，采集前必须先判定。（其正文载荷性质见修订 14。）
- 修订 10：§2 迁移编号与 §6 统一为账本编号 `0012`；§5.3 补两代形态的判定条件与
  「成因未验证」标注；§5.5 小标题与差额计数措辞对齐（3 处）；§1-9 与 §9-1 补记
  `traces/` 的采集边界（不纳入采集面）。
- 修订 11（实质错误）：§4.2 的 `sessionId` 断言更正——并非「全量无缺失」，实测 **886 行不含
  该字段**（`file-history-snapshot` 884 + `resend-fork-notice` 2；两者键集合中既无 `sessionId`
  也无 `parentId`），归属只能靠所在文件或 `cwd` 推断。
- 修订 12（实质错误）：§5.3 的 `message.usage.cache_read_input_tokens` 断言更正——不存在
  「11 行二者不相等」；实测 901 行缺键（旧形态 11 + 新形态 890），而在同时具备该键与
  `prompt_cache_hit_tokens` 的 **1280 行里恒等**。原表述系把「缺键」误读为「不等」。
- 修订 13（实质错误）：§3/§7 的 `sessions/<pid>.json` 断言更正——它按 PID 命名且不清理，
  本机 69 个文件里仅 1 个在 30 分钟内更新，其中 `sessionId = "interact"` 的条目不是会话文件，
  故不是「活跃会话清单」，原「心跳与 JSONL mtime 同步推进」不成立，须叠加新鲜度过滤。
  同批：§6/§7 标注 `sessions` 行数随云侧活动增长（53→54）、§1-7 同步；§8 的 tool-results
  改为精确字节数（71187 / 60467）；§7 补 mtime 与末条事件的实际差值区间（+0.001 ~ +0.052 s）。
- 修订 14（实质错误，第四轮通读时补）：§3 表格把 `traces/` 标为「不承载正文」**不成立**——
  `spans[]` 中 `type = "generation"` 的条目带 `toolInput` / `toolOutput`，含完整提示词与模型
  响应（抽查单文件 459 个 span：`custom` 306 / `generation` 51 / `function` 89 / `agent` 13）。
  已在 §1-9、§3、§11 三处改写该性质与采集边界。同批：§1-5/§5.5 的「两处」改为「两种来源」、
  §5.4 补记 43 条 error 中 13 条不含 `status`/`code`、§5.4 修正「别名之类别名」文字重复、
  §3-结论 1 补 traces 前提、§10 的字段成因表述与 §5.3 的「成因未验证」对齐。
- 修订 15（方法性错误，第五轮复核）：§4.2「公共字段」表**方法有误**——它由单文件骨架外推，
  把「结构上常见」当成「每行都有」。已改为「字段与出现率」表（实测 `_meta` 仅 5.9%、
  `id` 99.1%、`sessionId` 89.9%、`providerData` 89.5%、`parentId` 88.8%；8 种 `type` 共 13 种
  键组合）。同批补 §4.3 的 `message` 行内细分（assistant 992 / user 236 / 带 `message` 子对象
  143 / `incomplete` 43）、`file-history-snapshot` 两种形态（808 带 `id`、76 带 `messageId`）、
  §5.2「采集必需字段在 2233 条 usage 行上 100% 覆盖」。
- 修订 16（实质错误）：§1-9/§3/§11 的 `traces/` 规模更正——此前仅抽查 1 个文件（459 span），
  全量为 **181 个文件 / 13 个 PID 目录 / 97,707,587 字节 / span 5420 / `generation` 758
  （其中 756 个带完整载荷）/ 仅覆盖 4 个会话**；并登记「文件规模随时间变动」的未验证推论。
- 修订 17（实质错误，同轮）：§3 表格的附属物描述亦为**单样本外推**，已改为带覆盖率的实测值——
  `file-history` 的版本后缀是 **`v1`–`v6`（原写死 `@v2`；该值随后续观察扩至 `v1`–`v23`，
  见修订 21）**、9 个会话目录共 280 个文件；
  会话附属物覆盖率 `.meta.json` 16/23、`.file-rollback.ndjson` 20/23、`subagents/` 3/23、
  `tool-results/` 2/23；`workspace/sessions/` 19 个会话目录（`permission.json` 15/19）+
  `gc_config.json` 等 2 个文件；`tasks/` 5 个会话 / 53 个文件；`audit-log/` 14 个日文件 +
  `manifest.jsonl` + `spool/`；`blobs/` 8 个文件。并新增一句「附属物全部可选」的判据提示。
- 修订 18（实质错误，第三批）：两处以偏概全的描述更正——① §4.2 的 `id` 并非「UUIDv7（少数为
  本地生成形态）」：实测 36 字符 UUID 5396 行与**32 字符无连字符 hex 3263 行（37.7%）**两种
  形态并存；② §6 的 `deleted_at` 不是「-1 未删除 / 本地多为 NULL」这么简单：实测三态为
  `-1`（31 个云会话）、`NULL`（18 个本地会话）、**时间戳（5 个已标记删除的本地会话）**，
  且**归档 ≠ 删除**（7 个 archived 的 `deleted_at` 全为 NULL），删除标记**不删文件**（5 个
  会话的 JSONL 与 8 条 usage 行仍在原位，采集会照常计入）。
- 修订 19（第四批）：§3/§7 的 `sessions/<pid>.json` 细节更正——字段集实为 14 个（原漏
  `updatedAt`，69/69 同构）；`sessionId` 的两种形态是 `<uuid>`（20 个，13 个可对应本地会话）与
  `interactive-<pid>`（49 个；原误写作字面值 `"interact"`，系我截断显示所致）；「仅 1 个新鲜」
  为 2026-10-11 13:27 的观察快照，已标注时刻。
- 修订 20（第五批）：§2 规模改为复核时刻实测（2303 MiB / 96,989 文件；正文仅 41.9 MiB，体积
  集中在 `binaries` / `logs` / `plugins` / `workspace` / `traces`）；§9 新增第 12 条「活库读取」
  —— `workbuddy.db` 的 WAL（655,112 字节）大于主库（204,800 字节），必须按带 WAL 的活库处理。
- 修订 21（**独立复核发现的实质错误**，第六批）：由一名不共享本方结论的独立复核者重算约 110
  条断言后交叉比对，修正 9 处：① §4.2/修订 11 的「`resend-fork-notice` 连 `parentId` 也没有」
  不成立（该 2 行**含** `parentId`，同时缺两者只有 `file-history-snapshot`；`parentId` 缺失
  978 行 = 884 + `message` 63 + `ai-title` 21 + `session-meta` 10）；② §7 的「追加不重写」原用
  12 个抽样文件（抽样区间内 1 个文件 2 处回退、最大 2249 ms，该值在自己复算时可复现；复核者
  的差异数字源于回退量的计算口径不同），改为**全量 42 文件**（39 单调、3 个有回退共 5 处、
  最大 6396 ms，usage 行口径下 0 回退）；③ §6 的索引过滤条件并非「一律」带 `deleted_at = -1`
  （`cloud_verified` 只有 `transport = 'cloud'`）；④ `id` 实为 UUIDv4 4762 / UUIDv7 634 /
  32 hex 3263 / `notice-*` 2；⑤ 子 agent 文件名为 `agent-<8|16hex>`（**12 个 16 位 + 7 个 8 位**，
  按固定 16 位正则实现会漏采）；⑥ §10 云同步列并非「大面积为空」（仅 `agent_last_synced`
  0/54，`visibility` 31/54、`group_id` 25/54 有值）；⑦ §5.4 的渠道前缀在 `requestModelId`
  而非 `model`；⑧ §4.3 的 `image_blob_ref` 漏 `type` 键；⑨ §4.4 的 slug 样本实为 14 个可验证
  （1 个目录只有空文件）。另补两条 traces 事实：`function` span 也带载荷（779/782）、
  `trace.modelInfo` 仅 20/181 个文件为 dict。
