# WorkBuddy AI 会话保存与 token 计数口径核验（候选适配器前置研究）

> 只读核验：扫描本机 `~/.workbuddy-ai`（桌面版 5.7.6，内核字段命名空间为
> `codebuddy.*`）的真实持久化数据。文档只保留结构化字段名、表名、聚合计数与
> 口径判断，**不含真实会话 ID、消息 ID、路径、账号标识、提示词、工具输出或
> 模型输出的任何内容**；SQLite 一律以 `file:...?mode=ro` 打开，未写入任何被扫描
> 目录，也未改动 TokenScope 缓存。
>
> 结论分三档标注：**已验证**（本机实测或文件字节闭环可证）／**一致解释**（多条
> 证据指向同一原因，无单点直证）／**未验证推论**（待补充样本或新版本复核）。
>
> **同主题对照**：国内版 WorkBuddy（`~/.workbuddy`，桌面版 5.7.7）的同项核验见
> [国内版会话存储与用量记账核验](2026-10-11-workbuddy-session-storage.md)。两篇样本
> 规模与版本不同、结论互为交叉验证；本文样本属**旧形态**（带 hit / miss 与 credit
> 字段），新版形态差异、子 agent 转写、`session_usage.credit_json` 权威汇总与
> cloud 会话边界以该篇为准。

## 1. 方法与样本

- 数据源：`~/.workbuddy-ai` 全目录只读扫描（Python `sqlite3` + 二进制/文本读取），
  重点为 `projects/`、`sessions/`、`workbuddy.db`、`workspace/`、`file-history/`、
  `changes-index/`、`changes-detail/`、`artifact-index/`、`tasks/`、`audit-log/`、
  `storage/`、`logs/migration/`、`security/`。
- 会话样本：**1 个交互式会话**（工作目录位于用户桌面下一个中文命名的项目目录），
  转写文件 11,298,971 字节 / 2,384 行 / 0 行解析失败；该会话含 8 条标记
  `startsNewUserRequest: true` 的用户消息、12 个 `conversationRequestId`、
  656 次**有用量记录**的 LLM 调用（唯一 `messageId` 数；另有 3 次失败调用既无
  `messageId` 也无用量，见 §4.5）、2 个模型。
- 补充证据：`workbuddy.db` 的 drizzle 迁移 SQL 原文（`.workbuddy-sqlite-migrations/`）、
  版本标记文件（`session_fragment_repair_done520`）、`logs/migration/` 与 `logs/startup/`
  运行日志。
- 局限：单机、单用户、单会话；账号为免费版（`editionType: free` / `isPro: false`）；
  未出现 cloud transport 会话；`projects/` 下只有一个 slug 目录，目录名派生规则
  只有单一形态的样本。

## 2. 存储分层（已验证）

WorkBuddy 把「会话」拆成三层职责，正文不在数据库里：

| 存储 | 角色 | 权威内容 |
| --- | --- | --- |
| `projects/<cwd-slug>/<sessionId>.jsonl` | **转写正文权威** | 会话全文事件流：消息、推理、工具调用与结果、文件快照、压缩摘要 |
| `workbuddy.db`（SQLite + WAL） | 元数据索引 | 会话列表行、上下文占用、自动化任务、工作区信任；**不存正文** |
| `sessions/<pid>.json` | 运行期注册表 | 进程级心跳：`pid` / `sessionId` / `cwd` / `endpoint` / `lastHeartbeat` |

- **结论 1（已验证）**：采集会话正文只需扫 `projects/*/*.jsonl`；与 Claude Code /
  Codex 的「一文件一对话 JSONL 目录」形态同构，而不是 ZCode 那种「正文进 SQLite」
  的形态。
- **结论 2（已验证）**：`sessions/<pid>.json` 不是会话存储。本机两个文件记录的是
  **同一个** `sessionId` 的两次启动（不同监听端口）；文件按 `pid` 命名，属进程注册
  表，会话正文不依赖它。
- **结论 3（已验证）**：正文不依赖数据库即可完整读取；只读采集应避开 `workbuddy.db`
  （运行期持有 `-wal`/`-shm`），用不到它。

目录名派生规则（`cwd` → slug）：盘符转小写、分隔符 `\` 全部替换为 `-`，其余字符
（含中文与大写字母）原样保留。**一致解释**（单样本）：

```text
C:\<盘符段>\Users\<用户>\Desktop\<中文目录>\<项目名>
→  c-Users-<用户>-Desktop-<中文目录>-<项目名>
```

## 3. 转写文件格式（已验证）

每行一个独立 JSON 对象，无外层包装。样本行类型分布：

| `type` | 行数 | 内容 |
| --- | --- | --- |
| `function_call` | 687 | 工具调用：`name` / `arguments` / `callId`；其中 650 行的 `providerData` 携带该次请求用量 |
| `function_call_result` | 686 | 工具输出：`output` / `status`（686 行全为 `completed`）；结果主体另在 `providerData.toolResult`（内含 `rawResponse`，以及工具错误码 `tool_error_code` / `is_error`） |
| `reasoning` | 434 | 思维链，正文在 `rawContent[].text` |
| `file-history-snapshot` | 318 | 文件备份追踪状态（317 行含实际 tracked 文件） |
| `message` | 254 | user / assistant 文本（含压缩摘要），`content[]` 为 `input_text` / `output_text`；`status` 为 `completed` 222 / `incomplete` 3（后者见 §4.5）；其中 6 行为无工具调用轮次的用量载体 |
| `session-meta` | 4 | 会话头（`codebuddy.ai/hostKind`） |
| `ai-title` | 1 | AI 生成标题 |

公共字段：`id`、`parentId`、`sessionId`、`cwd`、`timestamp`（Unix 毫秒，UTC）、
`_meta.{traceparent,baggage}`（OTel 链路，baggage 内含 `codebuddy.session_id` 与
`codebuddy.conversation_request_id`）。assistant 侧行额外带
`providerData.{model, requestModelId, requestModelName, agent, conversationRequestId,
messageId, traceId}`。

### 3.1 写入语义：纯追加（已验证）

`Σ(len(line)+1)` 与文件字节数**精确相等**（delta = 0，2,384 行全量求和），即没有
重写、没有尾部截断、没有压缩包装、没有逐行加密。与旧版本的一次性修复标记
（`session_fragment_repair_done520`）及 `logs/migration/` 中的
`[SessionFragmentRepair] Scanning: ...\projects` → `projects dir not found, marking done`
相互印证：**旧版本曾按片（fragment）保存会话，5.7.6 首启做一次性合并修复**，
当前版本为单文件整篇。旧分片格式本身未取证。

### 3.2 会话重建（已验证）

- `id` 2,066 个中 2,065 个为 UUIDv7 形态、1 个为本地生成形态（`q-1`）。但**不可按 `id`
  做全局排序**：跨类型混排时 `id` 序与行序严重不符（位置一致率仅 197/2,066），只有
  **同一 `type` 内部**相邻 `id` 才近乎严格有序（`function_call` / `function_call_result` /
  `reasoning` / `session-meta` / `file-history-snapshot` / `ai-title` 相邻逆序均为 0，
  `message` 为 1）。
- `parentId` 构成 DAG：样本 2,066 个 `id` / 2,058 条父子边 / **悬挂父引用 0**。
- 行序≈时间序，仅 5 处逆序，其中 4 处落在 `session-meta` 头部补写与压缩摘要插入
  位置、1 处为倒退 14 ms 的普通写入抖动（2026-10-11 复核精确化）。
  因此**不能把行序当作严格时间序**，恢复顺序应走 `parentId` 图 + 行序；按 `id` 全局
  排序会失真（见上一条）。
- 自动压缩（`providerData.compactType: "pre-message-auto"` + `isCompacted` +
  `isCompactInternal`）会在**同一文件内追加**一行 `message`，`role=user`，正文以
  `<cb_summary>` 包裹历史（内含 `<previous_tool_call>` / `<omitted />`）。该行
  **没有 `parentId`，只有 `logicalParentId`**，指向被压缩段的最后一条消息
  （样本两条摘要行分别指向其前一条 assistant 消息）。压缩点是重建会话的必须边界：
  单行最大 522,410 字节 ≈ 510 KiB（2026-10-11 复核更正：初稿「474 KB」实为该行
  字符数 474,278，含中文按 UTF-8 计字节数更大），为样本内最大行。

## 4. token 计数口径（核心结论）

### 4.1 用量三副本，数值恒等（已验证）

同一请求的用量被写三份，三个副本的**共有键** 656/656 行逐字段相等（`message.usage`
的键集少一个，见 §4.2）：

```jsonc
"providerData": {
  "rawUsage": {                       // 上游原始（snake_case），13 个键
    "prompt_tokens": 53179, "completion_tokens": 400, "total_tokens": 53579,
    "prompt_cache_hit_tokens": 34688, "prompt_cache_miss_tokens": 18491,
    "prompt_tokens_details": { "cached_tokens": 34688 },       // 缓存真值在此
    "completion_tokens_details": { "reasoning_tokens": 270 },
    "completion_thinking_tokens": 270, "credit": 0,
    // 以下四个键在本 provider 下恒为 0（死字段，见 §4.2）
    "cached_tokens": 0, "cache_read_input_tokens": 0,
    "cache_creation_input_tokens": 0, "prompt_cache_write_tokens": 0
  },
  "usage": {                          // SDK 归一化（camelCase）
    "requests": 1, "inputTokens": 53179, "outputTokens": 400, "totalTokens": 53579,
    "inputTokensDetails": [{ "cached_tokens": 34688 }],
    "outputTokensDetails": [{ "reasoning_tokens": 270 }]
  }
},
"message": { "usage": { "input_tokens": 53179, "output_tokens": 400,
                        "total_tokens": 53579, "cache_read_input_tokens": 34688 } }
```

### 4.2 缓存字段陷阱：同名键「位置决定语义」（已验证）

656 行全量断言，零反例：

| 断言 | 结果 |
| --- | --- |
| `prompt_cache_hit_tokens + prompt_cache_miss_tokens == prompt_tokens` | 656/656 |
| `prompt_tokens_details.cached_tokens == prompt_cache_hit_tokens` | 656/656 |
| `message.usage.cache_read_input_tokens == prompt_cache_hit_tokens` | 654/654（另 2 行**无该键**） |
| `completion_thinking_tokens <= completion_tokens` | 656/656 |
| `total_tokens == prompt_tokens + completion_tokens` | 656/656 |
| `usage.*` 三字段 == `rawUsage.*` 对应字段 | 656/656 |
| `rawUsage` 侧四个缓存键（`cached_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` / `prompt_cache_write_tokens`）均为 0 | 656/656 |

结论：

- **结论 4（已验证）**：同名键 `cache_read_input_tokens` **在两个位置语义不同，且都不
  可靠**。在 `providerData.rawUsage` 里它 656/656 恒为 0，与同侧顶层 `cached_tokens`、
  `cache_creation_input_tokens`、`prompt_cache_write_tokens` 一样是**死字段**；而在
  `message.usage` 里它承载**真实命中量**（654/654 等于 `prompt_cache_hit_tokens`，
  且 654 行全部非 0），但**该键可能整体缺失**（本样本 2/656 行没有它，此时
  `message.usage` 只剩 `input_tokens` / `output_tokens` / `total_tokens` 三键）。
  即：按字段名取数会踩两个方向的坑——取 `rawUsage` 侧得到全 0（本样本约 98.9% 的
  缓存读被丢成 0），取 `message.usage` 侧则在缺键行上取空。
- **结论 4b（已验证）**：稳定真值只有一处，即
  `prompt_tokens_details.cached_tokens`（等价物 `prompt_cache_hit_tokens` 与
  `usage.inputTokensDetails[0].cached_tokens`）；缺失时才回退
  `prompt_cache_hit_tokens`，其余同名键一律不用。
- **结论 5（已验证）**：`prompt_tokens` **包含**缓存命中量（hit + miss 恒等于
  prompt、且 `prompt >= cached` 恒成立），属 OpenAI 语义而非 Anthropic 语义。
  接入 TokenScope 前必须做桶归一：`input = prompt_tokens − cached_tokens`，
  否则与 Claude / Codex 的互斥桶口径合并会重复计费。
- **结论 6（已验证）**：`completion_thinking_tokens` / `reasoning_tokens` 是
  `completion_tokens` 的**子集**（656/656 满足 `thinking <= completion`），不可叠加。
- **结论 7（一致解释）**：`credit` 字段 656 行全为 0。本机账号为免费版，无法判断
  付费账号下该字段是否承载计费额度，也未找到 credit 语义的本地文档。

### 4.3 去重键：`messageId`，不是 `conversationRequestId`（已验证）

- **结论 8（已验证）**：全局唯一 `messageId` 恰 656 个，与带 `rawUsage` 的 656 行
  **一一重合**且两两互异（每个 `messageId` 恰好 1 条用量记录，重复行数为 0），
  `(sessionId, messageId)` 可直接做幂等去重键。一个 `messageId` 会横跨多行，两个口径
  须分清：**涉及行数为 1–24 行**（1 行 5 个、2 行 199 个、3 行 248 个、4 行 182 个、
  5 行 8 个、6 行 7 个、8 行 6 个、24 行 1 个），**涉及行类型数为 1–4 种**
  （`function_call` / `function_call_result` / `reasoning` / `message`）；行数最多那例
  是一次调用发出 11 个并行工具调用（11 + 11 + 1 + 1）。口径是「一次 LLM 调用 = 一个
  `messageId`」，而非「一行一次调用」；用量载荷本身只挂在 `function_call` 650 行与
  `message` 6 行（无工具调用的纯文本轮次）上，取数须按字段存在性。
- **结论 9（已验证）**：对应的 `conversationRequestId` 只有 **12 个**，且**不等于用户
  提交数**：样本中仅 8 个能对上 `startsNewUserRequest: true` 的用户消息，另 4 个没有
  用户触发行（其中 2 个恰好覆盖自动压缩段）。任一口径下它都比 656 少两个数量级，
  **不可用作请求计数**。
- **结论 10（已验证）**：模型名取 `providerData.requestModelId`（与 `model` 同值），
  `requestModelName` 是展示名（样本 `Deepseek-V4.1-Flash`）；样本内 2 个模型串均为
  非主流命名，不可假设模型名形态。

### 4.4 上下文占用 ≠ 累计用量（已验证）

`workbuddy.db` 的 `session_usage(session_id, used, size, updated_at, credit_json)`
中 `used` **等于最后一次请求的 `input_tokens`**（样本 198,778 与最后一条用量完全
相等），是可滚动的上下文占用条，不是会话累计消耗；`size` 来自
`storage/<用户>-personal/scoped/<hash>/model-context-window.json`
（`{"by-model": {"<modelId>": 1000000}}`）。更细的分类拆分在
`conversationContextUsage.json`（`systemPrompt` / `conversation` / `tools` / `mcp` /
`skills`），本样本 `conversation` 占 158,716 / 198,778。

- **结论 11（已验证）**：`session_usage.used` 与累计 token 用量语义不同，**不得**
  作为用量事件写入历史库。

### 4.5 失败调用：无 `messageId`、无用量（已验证）

样本有 **3 行 `status: incomplete`** 的 assistant 消息，全部带 `providerData.error`，
且**既无 `messageId` 也无任何用量字段**：

| 错误形态 | `error` 关键字段 |
| --- | --- |
| 连接中断 | `code: "ECONNRESET"` / `message: "aborted"` / `isNetworkError: true` / `isRetryable: true` |
| 限流 | `code: 6004`，`message` 含 `429 usage exceeds frequency limit` 与配额重置时间 |
| 用户中断 | `message: "Interrupted by user"` / `isRetryable: false` |

- **结论 12（已验证）**：`status` 在本样本只出现 `completed` 与 `incomplete` 两态
  （`function_call_result` 686 行全 `completed`；`message` 222 `completed` + 3 `incomplete`）。
  失败调用不写 `messageId`、不写用量，因此 §4.3 的 656 是「**产生用量记录的**调用数」，
  **不等于发起过的调用总次数**（本样本总发起数 ≥ 659，下界）。统计请求数时须明确采用
  哪种口径，且**不得**给失败调用补零凑数。
- **交叉验证**：国内版核验独立撞到同一模式（其侧 43 条 `incomplete` 全部带 `error`、
  全部不带 usage），见文首同主题对照。

## 5. 元数据库与附属存储（已验证）

`workbuddy.db`：Drizzle 管理，18 条迁移（`0000`–`0017`），SQL 原文在
`.workbuddy-sqlite-migrations/`；11 张表。会话相关两张：

- `sessions`：`id`（= 转写文件名）、`cwd`、`user_id`、`title` / `custom_title`、
  `status`、`created_at` / `updated_at` / `last_activity_at`、`deleted_at`、
  `mode` / `source_mode`、`model`、`transport`、`conversation_origin`、`project_id`、
  `unread` 等。本机 1 行，`transport = local`。
- `session_usage`：见 §4.4。本机 `credit_json` 为 NULL。

`transport` 区分 `local` 与 `cloud`：迁移 `0012` 把独立的 `cloud_conversations`
表整体并入 `sessions`，并带来 `verified_at` / `agent_dirty` / `agent_last_synced` /
`group_id` 等字段——**云端会话在本机是服务端列表的缓存**。本机无 cloud 样本，
云端会话正文是否同样落 `projects/` **未验证**。

与会话同生命周期、按 `sessionId` 归档的旁路存储：

| 路径 | 内容 |
| --- | --- |
| `projects/<slug>/<sessionId>.file-rollback.ndjson` | 回滚点账本（`requestId` / 递增 `commitSeq` / 时间） |
| `file-history/<sessionId>/<hash>@vN` | 文件快照**明文原文**（本机 173 个版本文件）。`file-history-snapshot` 行的 `trackedFileBackups` 按相对路径登记 `version` / `backupTime` / `existedAtTrack`，并以 **`backupFileName`**（形如 `<16hex>@vN`）指向实际快照文件——样本声明的 173 个名字与目录实体**完全对应**（差集为空；另有 590 个条目未带该字段） |
| `changes-index/<sessionId>.json` | 按请求聚合的变更索引；大 diff 以 `detailStatus: "content-omitted-size"` 略去正文 |
| `changes-detail/<sessionId>/cd_<id>.<id>.json` | 变更明细（含 hunk 行级 diff） |
| `artifact-index/<sessionId>.json` | 产物索引（`uri` 为 `file:///` 百分号编码路径、`_meta.toolCallId`、`touchedByRequestIds`） |
| `file-tree-manifests/<sessionId>.json` | 逐请求文件树快照（本机 8.6 MB） |
| `tasks/<sessionId>/<n>.json` | TodoList 任务（`subject` / `description` / `status`） |
| `workspace/sessions/<sessionId>/` | `modify_backup`（修改前备份）、`.modify_backup_meta`（备份名 → 原始绝对路径映射，本机 3,217 条）、`snapfile`、`excluded_backup_roots.json`；容量上限由 `gc_config.json` 的 `maxSizeMb: 3000` 控制 |
| `memory/<uid>_memory.md`（+ `.bak`） | 用户记忆块（Markdown 正文 + `RAW_JSON_START` 内联 JSON） |
| `user-<uid>-personal/{SOUL,IDENTITY,USER,BOOTSTRAP}.md` | 工作区身份文件，按轮注入上下文（在转写里可观察到注入原文） |

`file-tree-manifests` 与 `changes-*` 的体积远大于转写本身，若未来要做磁盘占用统计
需留意（本机 `logs/sandbox` 独占 1.38 GB，与会话无关）。

## 6. 审计链与静态加密（已验证 / 一致解释）

- **审计链（已验证）**：`audit-log/<date>.jsonl` 为哈希链。算法经逐条复算确认：
  `sha256(json.dumps(entry_without_hash, sort_keys=True, separators=(",",":"), ensure_ascii=False))`
  （2026-10-11 复核补注：**必须 `ensure_ascii=False`**——按 Python 默认值复算，
  634 条中仅 6 条纯 ASCII 条目能命中，其余全部假性失配），
  两个 segment 共 266 + 368 条**全部匹配、零失配**，`prevHash` 逐条衔接且跨 segment
  连续（次日首条的 `prevHash` 等于前日尾条的 `hash`）。`manifest.jsonl` 记录 segment
  的 `entryCount` / `firstHash` / `lastHash` / `fileSha256`，`state.json` 记录当前
  segment 与 `sequence`。事件类型集中在 `command-safety.*` 与 `file-safety.*`。
- **静态加密（一致解释）**：`keyblob` 是 JSON 结构的密钥封装
  （`version` / `keyId` / `slots[{type: "static-v1", protectorKeyId, wrapped}]`，
  wrapped 值本身是 JWE 形态的 base64）；`security/<uid>/` 下含 `cipher/`（密钥泄漏
  检测规则 `detect-rules.toml`）、`data/`、`data.lock`（**被运行中的沙箱守护进程
  独占，只读打开报 `PermissionError 13`**）与 `at-rest-failures-v1.json`
  （记录 `credential-fields` 适配器写失败与重试）。**转写文件本身不走这套加密**，
  加密面是凭据与配置。
- **IPC（已验证）**：`wbipc/endpoint.json` 给出命名管道名与 ticket；沙箱中心管道为
  `\\.\pipe\WorkBuddy_SandboxCenter_<hash>_<pid>`，与 `epoch-marker.json` 一致。

## 7. 对 TokenScope 的落地建议

若立项 WorkBuddy 适配器，最小可用采集面是 `projects/*/*.jsonl` 单一路径，不碰 SQLite：

1. **发现**：遍历 `projects/` 下所有 `<cwd-slug>/<sessionId>.jsonl`；忽略
   `.file-rollback.ndjson` 等同目录非转写文件。
2. **解析**：逐行 JSON，只取含 `providerData.rawUsage` 的行（行类型不限，见 §4.3）。
3. **去重身份**：`(sessionId, messageId)`，天然幂等，重复扫同一文件不产生新事件。
4. **四桶归一**：`input = prompt_tokens − prompt_tokens_details.cached_tokens`；
   `output = completion_tokens`（thinking 已含在内，不另加）；
   `cache_read = prompt_tokens_details.cached_tokens`。注意同名键
   `cache_read_input_tokens` 在两个位置都不可单独采信（`rawUsage` 侧恒 0、
   `message.usage` 侧可能缺键，见 §4.2）。异常行做受检算术，负值/溢出按现有诊断口径跳过。
5. **模型**：取 `providerData.requestModelId`（与 `model` 同值），展示名可用
   `requestModelName`；模型串形态不可假设，走现有等价规则与前缀回退。
6. **时间**：行内 `timestamp`（Unix 毫秒，UTC）原样存入，只在展示层按解析时区转换。
7. **项目身份**：用 `cwd` 走现有规范化绝对路径；注意同一路径在转写里可能以
   `c:\...` 小写盘符形态出现，而同机 `workbuddy.db` 内是 `C:/...` 正斜杠形态，
   务必先归一（现有 source 层已覆盖该形态差异）。
8. **不要采集**：`session_usage.used`（上下文占用）、`file-history` 与
   `changes-detail`（可回滚的文件正文，含用户代码）、`audit-log`（安全审计）。
   转写文件本身含完整对话与工具输出，属敏感数据，**测试 fixture 必须手工合成**。

## 8. 待办与未覆盖

- `cwd` → slug 派生规则只有单一形态样本；中文目录已覆盖，但含空格、点号、盘符为
  网络路径或大小写混合的形态**未验证**。需第二个项目目录样本复核。
- `transport = cloud` 会话的正文落盘位置未取证（可能只存在于服务端 + `sessions`
  表缓存）。
- 旧版本的会话「分片」格式未取证（只有修复标记与迁移日志可证其存在）。
- `conversationRequestId` 的边界语义未定：样本 12 个中只有 8 个能对上
  `startsNewUserRequest` 用户消息，其余 4 个疑为压缩后的自动续跑（不影响请求计数
  口径，但会影响「会话轮次」类展示的定义）。
- `id` 跨类型不单调的成因未取证：同一 `type` 内部严格有序，疑为不同组件各自生成。
- 免费版 `credit` 全 0；付费账号下 `credit` 是否承载计费额度未验证。
- 失败调用（`status: incomplete`）是否在服务端计入用量配额未验证；本机只能证明本地
  不写 `messageId` 与用量。样本中有 1 次为 `429` 限流，说明配额判定发生在服务端。
- 样本全部为**旧形态**（带 `prompt_cache_hit_tokens` / `prompt_cache_miss_tokens` /
  `credit`）；新版形态（无这三个字段）本机未出现，回退分支需以合成 fixture 覆盖
  （国内版核验已实测该形态，见文首同主题对照）。
- 大文件行为未观测：本样本 11.3 MB / 2,384 行且可整篇解析；真实长会话（数月、
  数万行）的读取性能与是否存在追加外的维护动作（如尾部修复、`file-history`
  清理）未验证。
- `workspace/sessions` 备份区的 GC 触发条件（`maxSizeMb: 3000`）与回收粒度未观测；
  本机已见其对 `target/` 编译产物做备份的行为，说明备份选择面较宽。

## 9. 复核修订记录（2026-10-11）

全量只读复算通过：11,298,971 字节 / 2,384 行 / Σ(len+1) delta=0（末行带换行）；
行类型分布逐项精确一致；656 条用量行、messageId 两两互异、conversationRequestId
恰 12 个；全部 656/656 算术断言零反例；`session_usage.used=198,778` 与末条
input_tokens 精确相等、`credit_json` NULL、`by-model` 1000000；DAG 2,066 id /
2,058 边 / 0 悬挂；审计链 634/634 逐条复算匹配、跨 segment 衔接连贯、manifest 与
state 闭环；附属体积（8.63 MB / 1.38 GB / 3,217 条 / 173 个版本文件）与
`gc_config maxSizeMb: 3000` 均复现。

- 修订 1：§3.2 最大单行更正为 522,410 字节 ≈ 510 KiB（初稿 474 KB 为字符数口径）。
- 修订 2：§6 审计链哈希公式补 `ensure_ascii=False`（缺省时 628 条假性失配）。
- 修订 3：§3.2 五处逆序的归因精确为 4/5（1 处为写入抖动）。
> 修订 4–9 出自同日次轮复核（方法：把正文每条断言硬编码成期望值，与磁盘实测逐条
> 对照——转写 40 项、数据库与附属 35 项、引用原文 10 项）；修订 10–12 出自第三轮全文
> 通读；修订 13 起出自第四轮**反向核查**——不再问「写的对不对」，而问「该写的有没有
> 写」，并用 PowerShell/.NET 与 Python 两条独立实现交叉复算关键数字。
- 修订 4：§1 样本口径更正为「8 条 `startsNewUserRequest` 用户消息 / 12 个
  `conversationRequestId` / 656 次 LLM 调用」（初稿把 12 个 conversationRequestId
  误写为「12 次用户提交」）。
- 修订 5：§4.3 补「一次 LLM 调用 = 一个 `messageId`、用量只挂其中一行」，并更正
  `conversationRequestId` 与用户提交的对应关系（8/12，非一一对应）。该条随附的跨行数
  表述已被修订 7 取代。
- 修订 6：§3.2 更正 `id` 排序口径——跨类型 `id` 序与行序位置一致率仅 197/2,066，
  仅同一 `type` 内部有序，不可用于全局重建排序。
- 修订 7：§4.3 `messageId` 跨行口径更正为「涉及 1–24 行 / 1–4 种行类型」（此前把行
  类型数误写成行数）；行数最多一例为一次调用发出 11 个并行工具调用。
- 修订 8：§3.2 `id` 形态精确为 2,065 个 UUIDv7 形态 + 1 个 `q-1`。
- 修订 9（复核确认，不予翻转）：§6 审计链哈希公式**必须** `ensure_ascii=False`。两次
  独立实现复算——该参数下 634/634 全匹配，改用缺省 `True` 仅 6 条纯 ASCII 条目命中、
  628 条失配。本轮复核曾得出相反结论，经反向对照实验定位为复核脚本的标签错配，
  公式维持原判。
- 修订 10：§4.1 用量示例补齐为 `rawUsage` 全部 13 个键，「死字段」注释统一为四个缓存
  键（此前示例称「字段最全」却漏列 `cached_tokens` 与 `prompt_cache_write_tokens`，
  且只标了 2 个死字段，与 §4.2 断言表对不上）。
- 修订 11：§4.3 补结论编号 8–10（此前该核心节无编号，编号从 7 直接跳到 8，与 §4.2、
  §4.4 体例不一），§4.4 相应改为结论 11。
- 修订 12：§1 补充证据去掉与主题无关的 playwright 标记文件；§3.2 抖动表述改为
  「倒退 14 ms」。
- 修订 13：新增 §4.5 失败调用口径——3 行 `status: incomplete` 全部带 `error`、全部
  无 `messageId` 与用量，故 656 是「**产生用量记录的**调用数」而非发起总次数；§1 同步
  收窄表述（此前把口径写宽了）。该缺口由反向核查发现，前三轮方向不对所以没查到。
- 修订 14：§3 行类型表补 `message` 的 `status` 分布（222 `completed` / 3 `incomplete`）
  与 `function_call_result` 的 `providerData.toolResult` 字段（两者此前均未列）。
- 修订 15：§5 补 `trackedFileBackups` 的 `backupFileName` 字段——它才是与
  `file-history/<sessionId>/` 实体建立引用的键；样本声明的 173 个名字与 173 个实体
  严格对应（差集为空），另 590 个条目未带该字段。
- 修订 16：§8 登记失败调用的服务端配额语义未验证（样本含 1 次 `429` 限流）。
