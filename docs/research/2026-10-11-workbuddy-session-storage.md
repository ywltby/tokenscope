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
> 以及前篇未覆盖的**子 agent 文件**。两篇结论一致的项不再重复论证，只列断言与样本量。

## 1. 结论摘要

1. WorkBuddy 国内版的会话转写存放在 `~/.workbuddy/projects/<项目 slug>/<sessionId>.jsonl`，
   是**逐行追加 JSONL**；注入的 OTel 上下文使用 `codebuddy.*` 前缀（CodeBuddy 血统）。
2. **正文与索引分离**：JSONL 是内容唯一事实源；`~/.workbuddy/workbuddy.db`（SQLite）只存
   会话元数据（标题 / 模型 / cwd / 状态 / transport）与用量汇总，**不存正文**。
3. **一次 LLM 调用恰好一行、恰好一个 `providerData.messageId`**（2233 行实测零重复），
   该键即天然去重身份；请求数按 `conversationRequestId` 计会少算两个数量级。
4. 每行并列三处 usage（`providerData.rawUsage` / `providerData.usage` / `message.usage`），
   **取值恒等**（2233/2233），任取一处即可；三处同时累加即三倍虚增。
5. `providerData.rawUsage.credit` 是服务端返回的**费用点数**（非估算）；会话级汇总在
   `workbuddy.db` 的 `session_usage.credit_json`，两者本机 **9/11 会话逐键恒等**，
   2 个会话 db 偏高，**两处不可互相替代**（§5.5、§7）。
6. **子 agent 独立成文件、独立记账**：`<sessionId>/subagents/agent-<16hex>.jsonl`，与父文件
   `messageId` 零交集；父会话 db 的 credit **不并入**子 agent 信用点。漏采该目录会漏掉
   约三成调用（本机某会话主 70 次 / 子 29 次）。
7. **云端会话本地无正文**：db 中 `transport = 'cloud'` 的 30 个会话（App 侧自动化对话）在
   本地没有任何 JSONL；前篇的该条「未验证推论」在本机得到否证性答案。
8. 跨版本稳健：对 `~/.workbuddy-ai`（5.7.6）单会话 656 条独立样本复跑，§5 的三条
   不变量（一行一调用、三处恒等、hit+miss=prompt）**全部复现**。

## 2. 方法与样本

- 主数据源：`~/.workbuddy` 只读扫描（Python `sqlite3` + 文本读取），合计约 2.1 GB /
  9.4 万文件，其中会话正文约 40 MB。
- 会话样本：`projects/**/*.jsonl` 共 **42 个**（23 个主会话 + 19 个子 agent），
  单文件最大 16.96 MB / 3475 行，最小 0 行（空文件亦存在）；全量 **2233 条**带用量行。
- 交叉样本：`~/.workbuddy-ai`（前篇样本，5.7.6）单会话 2384 行 / 656 条带用量行，
  用于跨版本复核。
- 迁移权威文本：`~/.workbuddy/.workbuddy-sqlite-migrations/*.sql`（drizzle 账本 18 条 +
  `_journal.json`），`0011_unify_sessions_cloud.sql` 解释了 `transport` 列的来源。
- 时间跨度：本机 `~/.workbuddy` 全部事件 `2026-08-18T03:13:04Z` 至 `2026-10-10T18:18:23Z`。
- 局限：单机单用户；`transport = 'cloud'` 会话无本地正文，其字段语义无法核验；
  遗留迁移（`migration_meta` 的 `legacy_*` 系列）多为 `skipped`，旧格式未纳入样本。

## 3. 存储分层（已验证）

| 路径 | 角色 | 是否承载正文 |
| --- | --- | --- |
| `projects/<slug>/<sessionId>.jsonl` | **会话转写（唯一事实源）** | 是 |
| `projects/<slug>/<sessionId>/subagents/agent-<16hex>.jsonl` | 子 agent 转写 | 是 |
| `projects/<slug>/<sessionId>/tool-results/call_*.txt` | 超长工具结果外置 | 是（正文延展） |
| `projects/<slug>/<sessionId>.meta.json` | 会话级元数据（`hostKind`、`acpConnectionId`） | 否 |
| `projects/<slug>/<sessionId>.file-rollback.ndjson` | 文件改动提交点（回滚账本） | 否 |
| `workbuddy.db` | 会话索引 / 用量汇总 / 工作区 / 自动化 | 否 |
| `sessions/<pid>.json` | 运行期进程注册表（PID 命名） | 否 |
| `traces/<pid>/trace_*.json` | OTel trace 落盘，按 PID 分目录 | 否 |
| `file-history/<sessionId>/<hash>@v2` | 文件快照备份 | 否 |
| `workspace/sessions/<sessionId>/` | 工具改动备份（`modify_backup/`、`permission.json`） | 否 |
| `tasks/<sessionId>/<n>.json` | 会话级待办 | 否 |
| `audit-log/YYYY-MM-DD.jsonl` | 安全审计（哈希链，命令 / 文件放行决策） | 否 |
| `blobs/<前 2 位>/<sha256>.<ext>` | 图片附件（内容寻址） | 附件 |
| `local_storage/entry_*.info` | Electron 侧键值存储 | 否 |

- `sessions/<pid>.json` 是**运行中进程的心跳登记**，字段为
  `{pid, sessionId, cwd, startedAt, lastHeartbeat, kind, url, endpoint, mode, version, os, arch, hostname}`；
  它是「当前有哪些会话在跑」的唯一权威，与用量无关。
- `audit-log` 每条带 `sequence` + `prevHash` 构成哈希链（前篇已复算确认算法），内容是
  命令安全 / 文件安全决策，**不承载会话内容**。
- **结论 1（已验证）**：采集用量只需扫 `projects/` 目录树。
- **结论 2（与前篇的取舍差异，已验证）**：前篇建议「用不到 `workbuddy.db`」；本篇认为应
  **只读打开用于两件事**：会话元数据补充（标题 / 模型 / 状态）、以及 cloud 会话的本机可见性
  （本机 30 个 cloud 会话本地零正文，完全绕开 db 会让用户看到「查无此会话」的空白）。
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

### 4.2 公共字段

| 字段 | 说明 |
| --- | --- |
| `id` | 事件 ID，UUIDv7 形态（少数为本地生成形态，如 `notice-<ts>-<rand>`） |
| `parentId` / `logicalParentId` | 事件链父节点；压缩摘要行只有 `logicalParentId`（前篇已证） |
| `timestamp` | **毫秒 epoch**（全量 0 例外非整数） |
| `type` | 记录类型 |
| `sessionId` | 会话 UUID（全量无缺失） |
| `cwd` | 事件时工作目录，形如 `c:\Users\...`（**小写盘符 + 反斜杠**） |
| `_meta.traceparent` / `_meta.baggage` | OTel 上下文；baggage 内含 `codebuddy.session_id`、`codebuddy.conversation_request_id` |
| `providerData` | 供应商侧元数据（模型、用量、工具渲染等），见 §4.3 |

### 4.3 关键类型的差异字段

- `message`：`content[]`（`input_text` / `output_text` / `image_blob_ref`）、
  `providerData.messageId`（assistant 回合 ID）、`providerData.startsNewUserRequest`、
  `providerData.skipRun`、`providerData.compactType`。
- `image_blob_ref` 块形态：`{blob_id, mime, size, blob_path}`，`blob_path` 指向
  `blobs/<前 2 位>/<sha256>.<ext>`（内容寻址，可直接命中文件）。
- `reasoning`：`rawContent[]` 与 `content[]`（`content` 本机恒为空数组）。
- `function_call`：`callId`、`name`（工具名）、`arguments`（JSON **字符串**）、
  `providerData.argumentsDisplayText`（人类可读摘要）、`providerData.reasoning`。
- `function_call_result`：`callId`、`name`、`status`、
  `providerData.toolResult.{title, renderer, rawResponse}`；
  `rawResponse` 携带 `exitCode` / `signal` / `interrupted` / `sandboxDenied` /
  `stdoutBytesTruncated` / `stderrBytesTruncated` / `tool_error_code` / `is_error`。
- `file-history-snapshot`：`snapshot.trackedFileBackups[路径] = {version, backupTime,
  existedAtTrack, backupFileName}`，`backupFileName` 即 `file-history/<sessionId>/` 下的文件名。
- 本机工具名分布（前 6）：`Bash`、`Edit`、`Read`、`Write`、`PowerShell`、`WebFetch`。

### 4.4 项目 slug 规则（补足前篇的单样本缺口，已验证）

```
slug(cwd) = cwd.replaceAll("\\", "-").replaceAll(":", "-")
```

**只替换分隔符与盘符冒号**，中文、空格、下划线、字母大小写**全部原样保留**，不做非字母数字
字符的兜底替换。本机 15 个 slug 目录覆盖了以下形态，全部符合该规则：

| 形态样本 | slug 表现 |
| --- | --- |
| 纯 ASCII 路径（`Users` / `admin` 等大写保留） | 大小写原样 |
| 含中文目录名 | 中文原样，未转义 |
| 含空格目录名 | 空格原样（**未**替换为 `-`） |
| 含下划线的目录名 | 下划线原样 |
| 自动生成的工作目录（时间戳命名） | 连字符原样 |

前篇标注「含空格、盘符大小写混合形态未验证」，本篇以 15 个样本确认：**唯一变换就是把
`\` 与 `:` 变成一个 `-`**，与 Claude Code 的「非字母数字全替换」规则不同。

JSONL 内 `cwd` 为小写盘符 + 反斜杠，而 `workbuddy.db` 的 `sessions.cwd` 为大写盘符 +
正斜杠（`C:/...`）：**同一路径两种写法，归属判定必须先规范化**。

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
- usage 行在类型上分布在 `function_call` 与 `message` 两类（本机 69:1，交叉样本 650:6），
  **不要假设只挂在 `message` 上**；判定条件应是「该行是否带 `providerData.rawUsage`」。

### 5.3 两代形态与缓存字段（已验证）

| 形态 | 行数 | 特征 | 缓存读取值 |
| --- | --- | --- | --- |
| 旧形态 | 1291 | 带 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` / `credit` | `prompt_tokens_details.cached_tokens` **恒等于** `prompt_cache_hit_tokens`（1291/1291） |
| 新形态 | 942 | 无 hit/miss 与 credit 字段 | 100% 带 `prompt_tokens_details.cached_tokens`（其中 52 行 >0） |

- `prompt_cache_hit_tokens + prompt_cache_miss_tokens == prompt_tokens` 在旧形态
  **1291/1291 成立**（交叉样本 656/656 亦然）→ `prompt_tokens` 是**含缓存的输入总量**
  （OpenAI 语义），与 Claude 的互斥桶口径不同。
- `rawUsage.cached_tokens`（顶层）在本机 2233 行**恒为 0，是死字段**，不可用作缓存读。
- `rawUsage.cache_read_input_tokens` / `cache_creation_input_tokens` /
  `prompt_cache_write_tokens` 在本机**恒为 0**（前篇已在另一 provider 下确认同一现象）。
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

- 43 条 `status = incomplete` 行全部带 `providerData.error`
  （`status` 见 `429` / `499` / `400`，`code` 见 `0` / `6004`），且**全部不带 usage**。
  可安全跳过错误行，不会漏算也不会误计。
- 模型标识取 `providerData.model`，回退 `requestModelName` / `requestModelId`。
  本机出现带渠道前缀的写法（`custom-local:<model>`）与别名之类别名写法，
  归一必须走既有等价键规则（ASCII 小写 + 删除 `-` `.` `_`），**不能在适配器内做字符串裁剪**。

### 5.5 credit 与 `session_usage` 的关系（已验证 + 一处未解）

- `providerData.rawUsage.credit` 为**服务端返回的费用点数**（非 TokenScope 估算值），
  仅旧形态行携带，可以为 0。前篇样本（免费版账号）656 行**全为 0**；本机国内版
  出现非 0 值，可用于直接取费用，无需估算。
- `workbuddy.db` 的 `session_usage.credit_json` 是**以 `conversationRequestId` 为键的
  会话级累加**（32 位 hex，无连字符；与 `providerData.baggage` 里的同名 ID 同构）。
- 逐键闭合实测：可比对的 11 个会话中 **9 个逐键精确相等**（容差 1e-9），键级 **41/43**。
- 两个不闭合样本（db 偏高）：
  - 会话 A：db 单键 `4.18` vs JSONL 同键 `2.27`，差 `1.91`；该会话存在一个子 agent，
    其 credit 恰为 `1.91` 但**键名不同**（且不在父会话 `credit_json` 中）。**一致解释**：
    差额源于子 agent 计费并入父会话，但键改写方式未定。
  - 会话 B：db 单键 `322.00` vs JSONL 同键 `292.59`，差 `29.41`；该会话无子 agent 文件。
    **未验证推论**：差额来自未写入 JSONL 的请求（写入失败 / 历史版本重跑）。
- **两处不可互相替代**：db 无逐请求粒度，JSONL 也可能落后于 db 账本。

## 6. 会话索引库（`workbuddy.db`，已验证）

baseline schema 见 `.workbuddy-sqlite-migrations/0000_workbuddy_sqlite_baseline.sql`；
后续 18 条迁移逐列追加（会话侧关键列：`plugin_context_json`、`addon_selection`、
`session_settings`、`context_window`、`thought_level`、`unread`、
`transport` / `conversation_origin` / `visibility` / `group_id` / `agent_*` / `verified_at`）。
迁移 `0011_unify_sessions_cloud.sql` 把原 `cloud_conversations` 表整体并入 `sessions`
并 drop 旧表，`transport` 默认 `'local'`。

| 表 | 行数（本机） | 说明 |
| --- | --- | --- |
| `sessions` | 53 | 会话索引：`cwd` / `title` / `custom_title` / `status` / `created_at` / `updated_at` / `last_activity_at` / `deleted_at` / `model` / `mode` / `permission_mode` / `context_window` / `thought_level` / `transport` / `conversation_origin` / `visibility` 等 |
| `session_usage` | 16 | `session_id` 主键，`used` / `size`（上下文占用与窗口）+ `credit_json` |
| `workspaces` | 4 | `path` 主键 + `last_opened_at` |
| `workspace_trust_decisions` | 6 | `workspace_key` 主键 + `decision ∈ {trusted, rejected}` |
| `automations` / `automation_runs` / `automation_runtime_state` / `automation_delivery_outbox` | 0 | 定时自动化（本机未用） |
| `buddy_snapshots` | 0 | 助手快照 |
| `migration_meta` | 16 | 遗留迁移账本（多为 `skipped`） |
| `__workbuddy_drizzle_migrations` | 18 | drizzle 迁移账本 |

- **`transport` 分布**（会话维度）：`cloud` 30 个（全部 `status = completed`、`cwd` 为空串、
  标题为 App 侧自动化对话）、`local` 16 个 `completed` + 7 个 `archived`。
- **本地 JSONL 与会话一一对应**：23 个主会话文件全部在 `sessions` 表中，**无孤儿文件**；
  30 个 `cloud` 会话本地无任何 JSONL。
- `deleted_at = -1` 是**未删除**标记（云会话恒为 -1），本地会话多为 `NULL`；归档只改状态、
  不移动文件（7 个 `archived` 会话的 JSONL 均在原位）。索引 `idx_sessions_cloud_*` 的过滤
  条件一律为 `WHERE transport = 'cloud' AND deleted_at = -1`。
- **`session_usage.used` 语义（补足前篇的会话语义确认）**：本机 16 个有 `session_usage` 行的
  会话中，**14 个的 `used` 精确等于该会话最后一条用量行的 `prompt_tokens`**
  （= `message.usage.input_tokens`，两处恒等）。两个例外：
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
| 顶层 `cached_tokens` 是死字段 | 2233 行恒为 0 | 已验证 |
| 错误行不计费 | 43 条 error 行全部不带 usage | 已验证 |
| 主子不重账 | 3 个含子 agent 的会话，主与子 usage messageId 交集均为 0 | 已验证 |
| credit 与 db 闭合 | 9/11 会话逐键相等，键级 41/43 | 已验证（含 2 处未解差额） |
| 会话文件追加不重写 | 12 个抽样文件 timestamp 单调（11 个零乱序，1 个 2 处乱序）；文件 mtime 与最后事件时间精确一致 | 已验证 |
| 实时落盘 | 运行期心跳文件 `lastHeartbeat` 与 JSONL mtime 同步推进 | 已验证 |
| 云端会话无正文 | 30 个 cloud 会话本地零 JSONL | 已验证 |
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
  `<sessionId>/subagents/agent-<16hex>.jsonl`，**必须单独采集**；本机某会话主 70 次
  调用 / 子 29 次调用，漏采即漏掉约三成调用。子 agent 目录内**只有 `.jsonl`**，
  没有伴随的 `.meta.json`（与 Qoder CN 的形态不同）。
- **超长工具结果外置**：`<sessionId>/tool-results/call_*.txt`（本机最大 71 KB），
  JSONL 内保留摘要与渲染信息。
- **首行非固定**：39 个文件首行为 `message`，2 个为 `session-meta`，1 个为空文件。
  解析不得假设首行是元数据头。
- **单文件单会话**：未观察到分片文件（`.part` 之类）。迁移账本中的
  `session_fragment_repair_done*` 标记与旧版本「按片保存」的修复流程对应（前篇已由
  迁移日志佐证），当前版本为单文件整篇。

## 9. 对 TokenScope 的落地建议

1. **采集来源**：扫 `%USERPROFILE%\.workbuddy\projects`，发现规则为
   `*/<uuid>.jsonl` 加 `<uuid>/subagents/agent-*.jsonl`；目录不存在时静默零结果。
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
9. **路径归一**：JSONL 内 `cwd`（小写盘符 + 反斜杠）与 db 内 `cwd`（大写盘符 + 正斜杠）、
   以及 slug 目录名（只做 `\` `:` → `-`）三者形态不同，必须走同一规范化路径。
10. **不要采集**：`session_usage.used`（上下文占用）、`file-history` 与
    `changes-detail`（可回滚的文件正文，含用户代码）、`audit-log`（安全审计）。
    转写文件本身含完整对话与工具输出，属敏感数据，**测试 fixture 必须手工合成**。
11. **风险提示（三处）**：a) 三处 usage 重复累加导致三倍虚增；b) 漏采 `subagents/`
    导致调用数偏低；c) 误用 Anthropic 风格的 `cache_read_input_tokens`（恒 0）或顶层
    `cached_tokens`（恒 0）导致缓存读丢失、费用高估。

## 10. 风险与未知

- **version 漂移**：本结论基于 WorkBuddy 5.7.7（`~/.workbuddy`）与 5.7.6
  （`~/.workbuddy-ai`）。usage 的「两代形态」已表明字段集会随通道演进；
  实现前应对新版本重跑 §7 的断言。
- **credit 差额未解**：会话 B 的 `29.41` 差额无对应事件解释，可能来自写入失败或历史重跑，
  在采用 `credit` 作费用口径前需补样本确认。
- **压缩边界**：`isSummary` / `isCompacted` 行的 usage 是否代表一次真实完整调用
  未单独核验；本机仅 12 行标记为 `isCompacted`，影响面小。
- **`used` 的两个例外**未定向归因（压缩后不同步、playground 不上报），均属推断。
- **敏感文件提示**：`~/.workbuddy/models.json` 以**明文**保存自定义模型端点的 `apiKey`
  （本机非空），`~/.workbuddy-ai/models.json` 为空数组。该文件属敏感数据，TokenScope 的
  只读原则下**不应纳入扫描面**，也不应出现在任何导出物中。
- **云侧字段不可见**：`visibility` / `group_id` / `agent_last_synced` 等云同步列本机大面积为空，
  多端同步与云端会话正文的获取方式未采样。

## 11. 待办与未覆盖

- credit 差额的补样本核验（重点：存在子 agent 的会话，比对子 agent credit 与父 db 差额）。
- 新版本（> 5.7.7）字段形态复核：`rawUsage` 是否新增键、「两代形态」是否继续分化。
- `traces/<pid>/trace_*.json` 与 JSONL 的事件重叠度未统计；若确认重叠，**不得同时采集**。
- `tasks/`、`file-history/`、`workspace/sessions/` 与 `changes-*` / `artifact-index` 目录的
  用途只做了结构确认，未做内容级核验；它们与用量无关，暂不纳入采集面。
- 多用户 / 多账号（`sessions.user_id` 出现多个值）场景未采样；本机为单用户。
- 两份数据目录的分工（国内版 vs `.workbuddy-ai`）只由版本号与内容差异推断，
  产品侧的准确命名与用途**未验证**。
