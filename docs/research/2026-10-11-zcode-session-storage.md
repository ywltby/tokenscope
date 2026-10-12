# ZCode 会话存储与 token 计数口径核验（候选适配器前置研究）

> 只读核验：对照开源实现 `github.com/zai-org/ZCode`（本地克隆 HEAD `aac4755`，
> 根应用 v3.15.1 / CLI 包 0.16.9）与本机真实持久化数据。文档只保留结构化字段
> 名、表名、聚合计数与口径判断，**不含真实会话 ID、消息 ID、路径、提示词、
> 工具输出或模型输出的任何内容**；所有 SQLite 打开均带 `mode=ro`，未写入任何
> 被扫描目录，也未改动 TokenScope 缓存。
>
> 结论分三档标注：**已验证**（本机实测或源码可证）／**一致解释**（多条证据
> 指向同一原因，但无单点直证）／**未验证推论**（待新版本或补充样本复核）。

## 1. 方法与样本

- 源码：`zai-org/ZCode` 浅克隆（`git log -1` = `aac4755`，tag `v3.15.1`），
  精读 `apps/zcode-cli/packages/adapters/src/storage/session-store/**`（含定义
  `getDefaultSessionDbPath()` 的 `session-store/paths.ts`）、
  `packages/desktop/src/host/**`、`packages/services/src/paths.ts`（dataBaseDir
  逻辑，不含会话库路径）、`docs/zcode-agent-app-integration.md`、
  `docs/desktop/configurable-data-directory.md`、
  `apps/zcode-cli/docs/design/v2/session-persistence.md`。
  **来源可信度需分级**：上列多为「当前有效代码 + 现行文档」；本文另引用
  `docs/v4-refactor/15-timeline-resolver-design.md`，它是 **2026-07-07 的 macOS
  平台快照式设计文档**——其「存储位置」类陈述已在本机证伪（见 §2 复核更正），
  引用时须与本机实测交叉检查，不可直接当作现行事实。
- 本地数据：`~/.zcode/cli/db/db.sqlite`、`~/.zcode/v2/tasks-index.sqlite`、
  `~/.zcode/cli/**`（rollout / artifacts / exec / image-cache）、
  运行中进程命令行（`Win32_Process` 只读查询）。
- 查询方式：Python `sqlite3` + `file:...?mode=ro` URI；`json_extract` 读 JSON 列。
- 局限：单机单用户样本；桌面 App 数据目录仍为默认根（未经历 `dataBaseDir` 迁移）；
  远端（SSH/WSL/Docker）workspace 未采样；`packages/client`、`packages/server`、
  `packages/zcode-server-cli`、`packages/android` 只确认存在，未读内容。

## 2. 存储分层（已验证）

ZCode 不保存「一文件一对话」的 JSONL 转写，会话拆进两个 SQLite，职责严格分离：

| 存储 | 角色 | 权威内容 |
| --- | --- | --- |
| `~/.zcode/cli/db/db.sqlite` | **transcript 权威** | `session` / `message` / `part` 三种核心表，加 `session_entry`、`session_input`、`todo`、`permission`、`local_setting` 等辅助表与用量表 |
| `~/.zcode/v2/tasks-index.sqlite` | 列表索引（桌面侧） | 任务行、pin/archive/group/unread、全文搜索、cron 与闲时任务 |

注意区分两种「三层」，二者含义不同：设计文档 `session-persistence.md` 说的「核心
持久化只有三层」指**表**（`session` / `message` / `part`），并要求 `session_entry`
只做轻量 timeline 投影；`docs/v4-refactor/15-timeline-resolver-design.md` 说的
「三层事实源」指**存储位置**，并声明 `tasks-index.sqlite` **不得作为消息顺序真相源**。

> **2026-10-11 复核更正（初稿此表有误）**：初稿曾列出第三行
> `<AppData>/ai.z.zcode/zcode.db`（标注 app 元数据 / 通知）。**本机实测该存储不存在**：
> ① `ai.z.zcode` 目录在 `%APPDATA%` / `%LOCALAPPDATA%` / 用户目录下均不存在；
> ② `zcode.db` 实体文件全盘搜索无结果；③ 全仓库**生产代码**（排除测试与文档）对
> `zcode.db` **零引用**；④ `%APPDATA%\ZCode\` 的实际内容为 Electron / Chromium 运行时
> 数据——名为 `session` 的浏览器 partition（Cache / Code Cache / GPUCache / IndexedDB /
> Local Storage / Cookies / 各 `Partitions\*`）、`rum-electron-store`、`.updaterId`、
> `lockfile`、`zcode-data-size-telemetry.json`，**没有任何 SQLite 数据库**。
> 该行来源是 **2026-07-07 的 macOS 观察**（`~/Library/Application Support/ai.z.zcode/
> zcode.db`，见 `15-timeline-resolver-design.md:642`），属平台专属且可能已废弃；
> 初稿把它泛化成通用路径却未验证。**采集不受影响**——transcript 与列表都在上表两行。

- **结论 1（已验证）**：采集 ZCode 会话正文只有 `db.sqlite` 一个来源，无需扫
  文件系统；这与 Claude Code / Codex 的 JSONL 目录形态根本不同。
- **结论 2（已验证）**：`tasks-index.sqlite` 不承载 transcript，取正文必须回
  `db.sqlite`。

## 3. CLI 会话库（已验证）

- 路径由 `getDefaultSessionDbPath()` 硬编码为 `join(homedir(), ".zcode", "cli",
  "db", "db.sqlite")`，**不读** `dataBaseDir`（该函数实际位于
  `apps/zcode-cli/packages/adapters/src/storage/session-store/paths.ts`，
  `packages/services/src/paths.ts` 只有 dataBaseDir 逻辑——2026-10-11 复核补注）；
  契约默认值登记在
  `packages/contracts/src/config/index.ts`（`storage.sessionDbPath`），可被配置或
  `ZCODE_SESSION_DB_PATH`（兼容旧名 `ZCODE_SESSION_DB`）覆盖。
- 核心三层表：`session`（元信息 / 标题来源 / revert / 权限 / 归档时间）、
  `message`（一行一条 user 或 assistant，业务 shape 存 `data` JSON）、
  `part`（内容块：`text` / `reasoning` / `tool` / `file` / `step-start` /
  `step-finish` / `compaction` / `timeline` / `subtask`，业务 shape 存 `data` JSON）。
- 辅助表：`session_entry`（v2 UI/event 投影）、`todo`、`permission`、
  `input_history`、`session_target`、`session_input`（durable 输入账本）、
  `local_setting`、`workflow_*` / `dwf_*`，以及用量三表
  `model_usage`（逐请求）/ `turn_usage`（逐轮）/ `tool_usage`（逐工具调用）。
- 写入语义（`repositories/messages.ts`）：全部 upsert（`on conflict(id) do update`）；
  流式文本更新同一 `part` 行，不按 token 建行；`sequence` 由子查询
  `coalesce(max(sequence), -1) + 1` 按 scope 取队尾，同 scope 重存保留原值；
  每次写入 `touchSession` 刷新 `session.time_updated`。
- 读取顺序：`message` 用 `order by sequence is null, sequence, time_created, rowid`；
  `part` 用 `order by message_id, sequence is null, sequence, time_created, id`。
- 迁移（`migration-runner.ts`）：WAL 模式 + `pragma foreign_keys = on`；所有迁移在
  单个 `begin immediate` 事务内顺序执行，逐条 sha256 checksum 记账，历史迁移不可改
  （改了报 checksum mismatch，只允许追加）；多 Agent 共享同一库，启动锁 5 s、
  异步迁移等待上限 1 小时，`SQLITE_BUSY` 指数退避。
- **运行态不落库**：busy / cancel / shell runner 是进程内状态，重启后靠已写入的
  message/part 推断是否半截结束。

### 3.1 本机落地情况（已验证）

- 迁移账本 **22 条全部落地**（`0001`－`0022`，其中 `0019` 为 beta 前压平的单一
  基线）；前 18 条在建库时刻一次写入，后 4 条在后续版本升级时追加（2026-10-11
  复核更正：初稿误写「27 条」，与其自述的 `0001`－`0022` 和 18+4 矛盾；实测
  `time_applied` 精确聚成 2026-09-06 建库 18 条与 2026-09-17 升级 4 条两簇）。
- 规模：`session` 32 行、`message` 9945 行、`part` 35147 行、`session_entry` 1859 行、
  `session_input` 408 行、`model_usage` 8081 行、`tool_usage` 8458 行、`turn_usage` 346 行、
  `input_history` 100 行（该表设计为全局上限 100，符合预期）。
- `part` 类型分布：`tool` 9011、`step-start` 8666、`step-finish` 8532、`text` 4721、
`reasoning` 4155、`timeline` 39、`compaction` 15、`file` 8。工具状态在快照时刻只有
`completed` 8840 与 `error` 171；复核时另观测到 3 行短暂 `running`（在途调用），
说明 tool part 是「创建时以 running 落行、完成时更新」，**无流式中途写**——
「完成时一次写」措辞不准（2026-10-11 复核更正）。
- `sequence` 修复彻底生效：`message` 与 `part` 的 NULL sequence **均为 0**，且
  `message_sequence_autofill` / `part_sequence_autofill` 两个触发器已在库中。
- `session_input` 账本闭环：状态分布为 `promoted` 407 / `discarded` 4 / `cancelled` 3 /
  `admitted` 1（首轮快照 401/4/2/1，计数随活库增长）；407 个 `promoted_message_id`
  **无悬空引用**（引用完整性成立）。交付形态分布：`startNow`+`sendText` 319、
  `queue`+`backgroundNotification` 54、`queue`+`sendText` 36、`startNow`+`compact` 5、
  `startNow`+`sendGoalCommand` 1。

## 4. tasks-index 与 CLI 库的边界（已验证）

本机 `tasks-index.sqlite` 有 **15 行** task（复核末次 16 行），CLI 库有 **32 条**
session（复核末次 40 条），差额完全可解释（首轮快照值，活库持续增长）：

```text
tasks-index 15 行  ==  CLI session 中 task_type = interactive 的 15 条
差额 17 条         ==  CLI session 中 task_type = subagent_child
```

`task_id ∉ tasks-index` 的集合与 `subagent_child` 集合**完全相等，零例外**
（复核以 16 == interactive 16、不在索引的 24 个 == `subagent_child` 24 复现同一
双向零例外关系）。

- **结论 3（已验证）**：子代理会话只存在于 CLI 库、不进任务列表。因此按
  `tasks-index` 枚举任务会**漏掉子代理会话的用量**。
- `tasks-index` 侧只存轻量与可检索内容：`tasks` 表带 `searchable_text`（正文索引，
  上限 200000 字符）、`meta_json`（冗余 taskId/traceId/title/mode/model/
  thoughtLevel/provider/status/lastError），外加 `task_groups`、`automations`、
  `off_peak_tasks` 三组表。本机 groups / automations / off_peak 均为 0 行。
- 它的账本独立：`tasks_schema_migration` 仅 3 条（`0001_adopt_task_schema`、
  `0002_provider_selection`、`0003_official_glm_selection`）。
- 删除语义（源码裁决）：CLI session store 拥有会话内容，tasks-index 的 `deleted`
  拥有列表不可见性，**删除归档任务不等价于物理擦除会话存储**。只删 tasks-index
  会留下指向会话的孤儿行；只删 CLI 库会留下指不到会话的孤儿行。

## 5. token 计数口径（核心结论，已验证）

### 5.1 `message.data.tokens` 与请求一一对应，可作真实用量的交叉校验

> **2026-10-11 复核重写**：初版断言「对 `tokens.input` 逐消息求和比真实用量膨胀
> 约 1000 倍，禁止求和」，经复算**被否证**——初版样本会话（现 43 行 / 42 请求）
> 的 Σ(message tokens.input) = 2,911,757，与 `model_usage` 的 `sum(input)`
> 2,911,757 **精确相等**（比值 1.000x）；**逐会话比值区间 0.9916–1.0000、中位
> 1.0000，32 个双侧有数据的会话无一偏离 1%**。全库比值 1.028x 的超出部分**不是
> 膨胀**，而是分子多了"只有 message 记录、`model_usage` 完全没有"的早期会话
> （两个会话合计约 9,600 万 input，见 §5.4）。初版「约 2.49 亿」与其自身样本算术
> 不相容（39 行 × max 91,254 上限约 356 万）。注：初版那个「约 2.49 亿」取自**另一个
> 会话**的 `Σ(tokens.input)`，与该样本会话无关——两个会话的数字被混用，是本次错误的
> 直接成因。根因：message 与模型请求是 1:1 映射，「累积上下文快照」就是该次请求的
> input（含缓存读），求和恰好等于真实用量。

同一 session 内 `tokens.input` 随对话推进而抬升（例如约 22.9k → 45.9k → 67.3k →
91.3k），`tokens.output` 忽高忽低；但 `tokens.input` **并非严格单调**——压缩、
分叉、重试会造成回落。40 个含 tokens 的会话中约半数单调（复核末次实测 **22 单调 /
18 不单调**，随活库小幅漂移）；初版「39/39 严格单调」的样本不具代表性
（2026-10-11 复核实测）。

- **结论 4（修订）**：`message.data.tokens` 逐消息求和与 `model_usage` 的 input
  口径一致（含缓存读），可作交叉校验；统计主口径仍建议取 `model_usage`——它
  携带缓存分桶、重试（`attempt_index`）、取消/错误状态等 message 侧没有的粒度，
  而 `message.data` 适合取逐消息时间戳、模型名与会话上下文规模。
- **结论 5（维持）**：`tokens.output` 是该次请求的产出，方向与 `model_usage` 的
  `output_tokens` 一致（未做逐条对齐，仅量级判定）。

### 5.2 `model_usage` 是单次请求粒度的账本

`model_usage` 一行 = 一次模型请求，字段为真实列而非 JSON，可直接聚合：

| 字段组 | 字段 |
| --- | --- |
| 身份 | `id`、`logical_request_id`、`attempt_index`、`session_id`、`turn_id`、`trace_id`、`span_id`、`assistant_message_id`、`parent_user_message_id` |
| 分类 | `query_source`、`provider_id`、`model_id`、`variant`、`agent`、`mode`、`task_type` |
| 时间 | `started_at`、`first_token_at`、`completed_at`、`duration_ms`、`time_to_first_token_ms` |
| token | `input_tokens`、`output_tokens`、`reasoning_tokens`、`cache_creation_input_tokens`、`cache_read_input_tokens`、`provider_total_tokens`、`computed_total_tokens` |
| 结算 | `status`、`finish_reason`、`tool_call_count`、`retry_count`、`retryable`、`cancelled_by_user`、`context_exceeded`、`error_type`、`error_code`、`error_message` |
| 原始 | `raw_usage_json`、`provider_metadata_json` |

本机聚合（`model_usage` 口径，占比最高的模型组；2026-10-11 复核末次快照，活库仍增长）：

```text
model_id            请求数      input 合计      output 合计    cache_read 合计
（主力模型）          7574    2,929,651,804     5,465,213      2,872,159,936
```

**`input_tokens` 含缓存读，两列不可相加**（2026-10-11 复核补充，全量实测）：
`input_tokens >= cache_read_input_tokens` 在 **8412/8412 行恒成立、零反例**，即
`input_tokens` 是**含缓存的 prompt 总量**，`cache_read_input_tokens` 是其中的缓存
命中部分。本机 `sum(cache_read)/sum(input)` = **96.4%**。因此
`input_tokens + cache_read_input_tokens` 会把缓存读**双计**，总量约放大 2 倍——
这是 ZCode 接入时最需要防的一处口径错误。

**恒等式**：`computed_total_tokens = input_tokens + output_tokens` 在 **8412/8412 行
精确成立**（可与 Claude / Codex 的 `total` 口径直接对齐核对）。

- **结论 6（2026-10-11 复核修订）**：用量统计**优先**取 `model_usage`（缓存分桶、
  重试、状态粒度最全）；但它**不覆盖全部历史消息**（见 §5.4），缺失部分须回退
  `message.data.tokens`——两者按可用性互补，不是二选一。修订原因：初稿此处为
  「只取 `model_usage` 求和」，与 §5.4 的覆盖不对称结论冲突。
- **结论 7（2026-10-11 复核更正）**：`model_usage.assistant_message_id` 非空率
  实测 **99.79%**（复核末次 8394 非空 / 8412 行；18 条 NULL 全部落在侧路类型：
  `session_title` 10 / `compact` 5 / `target_completion_verification` 2 /
  `goal_summary_title` 1）——初版抽样误判为恒 `None`。关联消息应优先
  `assistant_message_id`，NULL 时回退 `parent_user_message_id`。

### 5.3 模型名多写法与辅助调用

- 本机 assistant 的 `modelId` 出现五种写法：`GLM-5.3-Flash`、`glm-5.3`、
  `glm-5.2`、`GLM-5.3`、`deepseek-v4.1-flash`。**大小写差异必须靠等价键吸收**，
  否则同一模型会分裂成多组。
- `message` 表 assistant 行数与 `model_usage` 行数存在差额，主因是**先于用量账本
  的历史消息**（457 条 glm-5.2 时代的 assistant 消息早于 `model_usage` 的首条记录：
  首条 message 2026-09-07、首条 model_usage 2026-09-11）与单消息子代理会话；
  `query_source` 侧路标题类调用（`session_title` + `goal_summary_title`）仅 11 行、
  约 0.13%（2026-10-11 复核更正：初稿「7365 差 424 = 侧路调用」不可复现——7247 实为
  主力模型组请求数而非表总行数，且 7365 与 §5.4 的 8705 对同一度量自相矛盾）。
- **结论 8（已验证）**：等价键归一（ASCII 小写 + 删除 `-` `.` `_`）是必需的，不是
  防御性设计。
- **结论 9（已验证）**：`query_source` 必须参与口径决策：主会话成本与辅助调用
  成本要么分开统计，要么明确合并，不能默认混同。全集已枚举（2026-10-11 复核，
  6 值）：`main_turn` 8004、`subagent` 385（约 4.6%）、`session_title` 10 /
  `compact` 5 / `target_completion_verification` 2 / `goal_summary_title` 1
  （六值合计 8407 行）——初稿「session_title 约占 5%」实为 `subagent` 的占比。

### 5.4 覆盖率（已验证）

- `message` 表中 assistant 行 **100% 带 token 字段，零缺失**（2026-10-11 复核末次
  9039/9039；首轮快照为 8705/8705——计数随活库增长，覆盖率本身恒为 100%）。
- **两侧覆盖不对称**（2026-10-11 复核新增，实现必须处理）：`model_usage` 并非
  覆盖全部 assistant 消息。实测存在**只有 message 记录、`model_usage` 完全没有**
  的会话（复核末次 2 个，均为早期版本的 `interactive` 会话，message 侧合计约
  9,600 万 input token）；此外**全部子代理会话**的 `model_usage` 行数都比 message
  侧 assistant 行数**少 1–2 条**（复核实测 24/24 个子代理会话呈此形态，差值仅
  1 或 2）。因此以 `model_usage` 为
  唯一来源会**低估历史用量**，以 `message` 为唯一来源则拿不到分桶与重试粒度——
  两者应按可用性互补，而不是二选一。这也解释了 §5.1 的全库比值 1.028x。
- **同一会话内会切换模型**（复核补充，已量化）：40 个含 assistant 行的会话中
  **8 个横跨多种 `modelId`**，最多者跨 4 种（`GLM-5.3-Flash` / `GLM-5.3` /
  `deepseek-v4.1-flash` / `glm-5.3`）。因此「按模型分组」不能假设与会话一一对应；
  模型维度必须取自**单条消息/单次请求**，不能从 `session` 级字段推断。
- `session.version` 是**创建时**的 CLI 版本，不是当前运行版本；本机存在两个取值
  （0.16.9 × 36、0.16.5 × 4），越早期的会话版本越低——初稿「所有会话均为同一
  版本值」已更正。
- `provider_total_tokens` 与 `computed_total_tokens` 的**恒等式与空值边界**（2026-10-11
  复核实测，全量 8412 行）：`computed_total_tokens` **恒非空**；`provider_total_tokens`
  在 109 行异常请求上为 NULL，这 109 行恰好是全部 `status ∈ {error(88), cancelled(21)}`
  的行，同时也是 `input_tokens = 0` 的行——**"零值"与"NULL"必须分开处理**，初版把
  这 109 行误述为"双侧 NULL"（`computed_total_tokens` 实际从未为 NULL），复核更正。
  已有 `provider_total_tokens` 非空的行与 `computed_total_tokens` 全等。建议以
  `computed_total_tokens` 为口径、`provider_total_tokens` 作交叉校验。
- **首 token 延迟字段有大面积 NULL**（复核补充）：`first_token_at` 与
  `time_to_first_token_ms` 各有 4118/8412 行为 NULL（约 49%），使用前必须先判空；
  `duration_ms` / `completed_at` / `span_id` 则恒非空。`turn_id` 仅 3 行为 NULL。

## 6. 互通性与数据目录边界（已验证）

- **桌面版启动的 Agent 就是 CLI 本体**，不是另一套实现。本机运行中进程命令行：
  `ZCode.exe "...\resources\glm\zcode.cjs" app-server --stdio --surface desktop`
  （CLI 打包为资源，用 Electron 二进制作 Node 运行时；`--surface desktop` 是
  CLI 侧的 surface 参数，用于区分前端形态）。
- 官方文档 `docs/zcode-agent-app-integration.md` 首句即：「ZCode App 只启动
  ZCode Agent（`apps/zcode-cli` 的 `zcode app-server --stdio`）」，并给出四层
  调用链 `UI → IZCodeAgentService → Local Host Process → ZCodeAgentProcessManager
  → workspaceKey-scoped app-server`。
- **会话正文完全互通**：同机多个前端（终端 TUI、桌面 App、手机 `/remote` attach）
  共享同一个 `db.sqlite`，并发靠 WAL + `busy_timeout` 协调（源码注释明言「多个
  本地或远程 Agent 会共享同一个 session DB」）。本机实测可自证：桌面进程启动时刻
  与该会话在 CLI 库中的创建时刻精确对应，且 `db.sqlite-wal` 与
  `tasks-index.sqlite-wal` 在同一分钟内有写入。
- **`dataBaseDir` 只改变 App 数据目录**：`copyDataDirectory()` 只复制
  `<base>/.zcode/v2`；已确认产品边界为「CLI 用户资源仍使用 `<HOME>/.zcode/cli`」，
  E2E 断言 `<custom>/.zcode/cli` 不存在而 `~/.zcode/cli/db/db.sqlite` 仍在原位。
- **未验证推论 / 待复核点**：`getProviderWorkspaceConfigDir()` 的实现跟随
  `getDataBaseDir()`（`join(getDataBaseDir(), ".zcode", "cli")`），而会话库固定于
  homedir。**即 provider 配置的读写位置随数据目录迁移，会话库固定不动**；这两条
  结论方向相反，迁移数据目录后若出现「provider 需重配但会话都在」属预期现象。
  文档未覆盖此点，建议实现前按 `packages/services/src/paths.ts` 的实际取值为准。

## 7. 附属资产与旧版迁移（已验证）

`~/.zcode/cli/` 下另有四类与会话关联但不承载 transcript 的数据：

| 路径 | 内容 |
| --- | --- |
| `rollout/model-io-sess_*.jsonl` | 模型原始 I/O 审计，每行一个请求（含 `requestId`、`attempt`、`model`、`request.body`；敏感字段已 `[REDACTED]`） |
| `artifacts/sess_*/` | 工具结果附件（文件名形如 `call_<id>-tool-result-<uuid>`、含 `media-N` 变体） |
| `exec/sess_*/`、`exec/shell-snapshots/` | 工具 stdout/stderr 落盘与 shell 环境快照 |
| `image-cache/sess_*/image-<hash>.png` | 图片缓存，按会话分目录 |

- `~/.zcode/v2/` 现在**不存在 `sessions/` 目录**（仅 cache、certs、checkpoints、
  crash、logs、runtime 等）。ACP 时代旧会话位于 `~/.zcode/v2/sessions/
  {workspaceHash}/{legacyTaskId}.json`（官方 `restore-legacy-sessions` 插件的
  `scan-legacy-sessions.mjs` 中 `defaultLegacyDir` 即此路径；2026-10-11 复核更正：
  初稿误写为 `~/.zcode-bak/v2/sessions`——本机不存在 `~/.zcode-bak`），该目录
  现已不在，由默认禁用的恢复插件按需扫描；恢复脚本要求每条 legacy message 至少
  写一个 `text` part，assistant 还需补 tool part 与上一条 user 的 `parentID`。
- `tasks-index` 与 legacy snapshot 共享 `workspaceHash = sha256(workspaceKey)` 前
  12 位，`workspaceKey = workspaceIdentity?.trim() || workspacePath`（远端优先
  identity）。**未验证推论**：若按路径枚举 workspace 目录名，需注意该 hash 不是
  路径明文。

## 8. 对 TokenScope 的落地建议

1. **采集来源**：正文与用量取 `%USERPROFILE%\.zcode\cli\db\db.sqlite`，**恒定不读
   `dataBaseDir`**；`tasks-index.sqlite` 取 `%USERPROFILE%\.zcode\v2\`（或自定义根
   下的 `.zcode\v2\`）。ZCode 是结构化库来源，不需要 `source` 层的文件发现逻辑。
2. **用量主口径**：优先 `SUM(model_usage.*)`（缓存分桶、重试、状态粒度最全）；
   但 `model_usage` **不覆盖全部历史消息**（存在完全没有它的早期会话，见 §5.4），
   缺失部分需回退 `message.data.tokens`——它逐消息求和与 input 口径一致，可作
   互补来源与交叉校验（2026-10-11 复核更正，初版「禁止求和」的 1000 倍膨胀论据
   不成立，见 §5.1）。两者按可用性互补，不二选一。
3. **等价键**：模型名大小写 + `-` `.` `_` 归一为必需项，否则本机即出现三组分裂。
4. **辅助调用**：`query_source` 全集已枚举（6 值；`subagent` 约 4.6–4.8%、
   `session_title` 仅 0.1%，2026-10-11 复核更正），按实测占比决定是否单列。
5. **缓存桶归一（最关键）**：`input_tokens` **已含** `cache_read_input_tokens`，
   接入时必须先做互斥化（`纯输入 = input_tokens - cache_read_input_tokens`）再与
   Claude / Codex 的互斥桶合并；直接 `input + cache_read` 会把缓存读**双计**、
   总量放大到约 2 倍（本机 cache_read/input = 96.4%，几乎全是缓存读）。同时缺失
   缓存读价的模型必须显式标注未知，不得按 input 价静默代入。
6. **子代理用量**：若按任务列表枚举会漏掉 `task_type = subagent_child` 的会话；
   应直接按 `session_id` 聚合（`model_usage.session_id` 优先、缺失时回退
   `message.session_id`），不依赖 tasks-index。
7. **活库读取**：`db.sqlite` 与 `tasks-index.sqlite` 都有常驻 `-wal`/`-shm`
   （本机 WAL 可达数 MB）。必须按「带 WAL 的活库」处理：以只读 URI 打开并接受
   读到的 WAL 内数据，不能只拷主库文件或假设主库即全量。
8. **重试归并**：`logical_request_id` + `attempt_index` 可识别重试；`retry_count`
   与 `status` 应参与统计口径，避免把重试算成独立请求。
9. **风险提示（四处）**：a) `query_source` 未区分导致辅助调用（`subagent` 约 4.8%）
   混入主口径；b) 模型名未归一致使分组分裂；c) `message.data.tokens` 缺缓存分桶与
   重试粒度，不得替代 `model_usage` 作主口径（2026-10-11 复核调整：初版 a 项
   「`tokens.input` 误求和导致数量级膨胀」不成立，见 §5.1）；**d) `input_tokens` 与
   `cache_read_input_tokens` 相加导致缓存读双计（约 2 倍），必须互斥化后再合并。**

## 9. 待办与未覆盖

- `query_source` 取值全集已枚举（2026-10-11 复核，见 §5.3）；`turn_usage` 与
  `model_usage` 的加总一致性核验仍待做。
- `provider_total_tokens` 与 `computed_total_tokens` 的全量等价性已验证
  （2026-10-11 复核，见 §5.4）。
- 源码克隆版本（v3.15.1 / CLI 0.16.9）与本机运行版本的 schema 是否完全一致，需在
  新版本发布时复核；`message` / `part` 的 JSON 字段拼写在源码 Reader 与本地载荷
  之间存在大小写差异（源码读取 `modelID`/`providerID`，本机载荷为
  `modelId`/`providerId`），**未验证推论**：可能另有归一入口或版本差异，实现解析
  时以实际载荷为准，不要照抄源码字段名。
- 远端（SSH/WSL/Docker/Server）workspace 的库位置与 `attachment` 路由未采样；
  `packages/client`、`packages/server`、`packages/zcode-server-cli`、`packages/android`
  未读内容。若要支持跨机用量归集，需单独立项。

## 10. 复核修订记录（2026-10-11）

只读复算确认（要点）：双库分工与集合关系（tasks-index 16 == interactive 16，
不在索引的 24 个 session == subagent_child 24，双向零例外）、NULL sequence 0 与
双触发器、session_input 无悬空引用、五种 modelId 写法、进程命令行逐字吻合、
`~/.zcode/v2` 无 `sessions/` 目录、源码克隆 HEAD `aac4755` / v3.15.1 与三层
持久化表述、迁移机制（WAL / begin immediate / sha256 记账 / 0019 压平基线注释）
均属实；快照表行数全部呈一致增长（活库，本复核会话也在写入）。

- 修订 1（实质）：§5.1 重写——初版「对 `message.data.tokens` 求和膨胀约 1000 倍、
  禁止求和」被否证（实测比值 1.000x–1.028x，message 与请求 1:1），单调性样本
  不具代表性（21/40 会话单调）；§8-2、§8-9 同步修正。
- 修订 2（实质）：§3.1 迁移账本条数 27 更正为 22（`0001`–`0022`，18+4）。
- 修订 3（实质）：§5.3 对账段重写——7365/8705 不可复现且互相矛盾、7247 口径
  错位；差额主因为先于用量账本的 glm-5.2 时代消息；「session_title 约 5%」
  实为 `subagent`（4.3%），`session_title` 仅 0.1%；`query_source` 全集已枚举。
- 修订 4：§5.2 结论 7 更正——`assistant_message_id` 非空率 99.79%，初版抽样误判。
- 修订 5：§5.4 `provider_total == computed_total` 升级为全量已验证（8178/8178）。
- 修订 6：§3 补注 `getDefaultSessionDbPath` 实际位于
  `apps/zcode-cli/packages/adapters/src/storage/session-store/paths.ts`。
- 修订 7：§3.1 tool part 存在短暂 `running` 态，措辞改为「无流式中途写」。
- 修订 8：§7 旧会话目录更正为 `~/.zcode/v2/sessions`（官方 restore-legacy-sessions
  插件脚本的 `defaultLegacyDir`；初稿误写 `~/.zcode-bak`，本机不存在该目录）。
- 修订 9（实质，补充遗漏）：§5.2 新增**桶包含关系**——`input_tokens` 含
  `cache_read_input_tokens`（全量 8412 行零反例，cache_read/input = 96.4%），
  两者相加会把缓存读双计、总量约放大 2 倍；并补 `computed_total_tokens =
  input_tokens + output_tokens` 恒等式（8412/8412 精确成立）。§8-5、§8-9 同步
  升级为"必须先互斥化"。
- 修订 10（实质，更正）：§5.4 的"109 行双侧 NULL"表述错误——`computed_total_tokens`
  **恒非空**，109 行实为 `provider_total_tokens` 的 NULL 与 `input_tokens = 0` 的
  同一批异常行（全部 `status ∈ {error, cancelled}`）；已补 `first_token_at` /
  `time_to_first_token_ms` 各约 49% 为空、`duration_ms` / `completed_at` 恒非空。
- 修订 11（一致性）：§2 表内"usage 三类表"措辞更正为逐个列名。**注意：中途曾把
  §3 改成"用量四表"并把 `session_input` 列入，这是错的**——`session_input` 是输入
  账本而非用量表，且已在前文辅助表列出，造成同一段内重复；已改回"用量三表"。
  计数快照归属：§3.1 的**规模数字为首轮快照**，§4 / §5.2 / §5.3 / §5.4 为
  **复核末次快照**（`model_usage` 8412、assistant 带 token 9039、侧路标题类 11 行、
  `subagent` 385 行）——两者不同时刻，**使用任何绝对计数前先重查，不要当作断言**。
- 修订 12（复核自洽性）：二次独立复核确认 §5.1 的否证成立——**逐会话**
  `Σ(tokens.input)` 与 `Σ(model_usage.input_tokens)` 比值区间 0.9916–1.0000、
  中位 1.0000，32 个双侧有数据的会话无一偏离 1%；**全库比值 1.028x 已查明归因**
  （分子含 2 个"只有 message、无 model_usage"的早期会话，约 9,600 万 input，
  非膨胀）。`input_tokens >= cache_read_input_tokens` 与 `computed_total_tokens =
  input_tokens + output_tokens` 两条不变量在全部行上零反例。
- 修订 13（实质，补充遗漏）：§5.4 新增**两侧覆盖不对称**——`model_usage` 不覆盖
  全部 assistant 消息（存在完全没有 `model_usage` 的早期会话），只按它聚合会
  低估历史用量；§5.1 的 1.028x 即由此而来。实操结论是两者按可用性互补。
- 修订 14（独立复核，全项重验）：结构性不变量**全部零反例**复现——
  `input_tokens >= cache_read_input_tokens`、`computed_total_tokens = input_tokens
  + output_tokens`、`input_tokens` / `computed_total_tokens` / `duration_ms` 无 NULL、
  `message.sequence` / `part.sequence` 无 NULL、assistant 行 100% 带 `tokens`；
  `tasks-index` 的双向零例外关系在更大规模下再次成立（索引 16 行 ==
  `interactive` 16 条，不在索引的 24 条 == `subagent_child` 24 条）；
  `schema_migration` 确为 **22 条**（18+4 两簇），双触发器与
  `provider_total_tokens` NULL 109 行（= 全部 `error` 88 + `cancelled` 21）均复现。
- 修订 15（实质，新发现）：**同一会话内会切换模型**——40 个会话中 8 个横跨多种
  `modelId`（最多 4 种）。按模型分组必须取单条消息/单次请求，不能从 `session` 级
  字段推断；同批修正 §3.1「所有会话同一版本值」的表述（实测 0.16.9 × 36、
  0.16.5 × 4 两值）。
- 修订 16（计数刷新）：§3.1 `session_input` 状态分布更新为 407/4/3/1，并补交付
  形态分布。
- 修订 18（第四次逐条核实，**发现一条从未验证的实质错误**）：§2 删除第三层存储行
  `<AppData>/ai.z.zcode/zcode.db`——本机该目录与文件均不存在、全仓库生产代码零引用；
  其来源是 **2026-07-07 的 macOS 平台专属观察**（`~/Library/Application Support/
  ai.z.zcode/zcode.db`），初稿把设计文档的陈述泛化为通用路径且未做本机验证。
  同批补充两种「三层」的语义区分（表 vs 存储位置）。**同轮独立复核确认成立**：
  `searchable_text` 上限确为 `200_000`（`TASK_SEARCH_TEXT_MAX_CHARS`，3 处定义）、
  `input_history` 上限确为 `100`（`INPUT_HISTORY_LIMIT`）、删除语义有测试直接支撑
  （`persists deleted tombstone and broadcasts task_deleted without deleting the CLI
  session`）。核实方法说明：自本轮起改为「逐条原子断言 ↔ 实测/代码」穷举对照
  （40 项数据断言中 39 项通过，唯一不符为活库漂移），不再依赖抽查。
- 修订 17（第三次全读复核）：修正 4 处内部缺陷——① §3 辅助表 `session_input`
  **重复列举**且被误列为"用量四表"（详见修订 11）；② §5.4 `session.version` 内容
  **重复两处**，已合并为单条；③ 结论 6「只取 `model_usage` 求和」与 §5.4 / §8-2 的
  「互补不二选一」**直接矛盾**，已改为"优先 model_usage，缺失回退 message"；
  ④ 结论 7 的 `8394/8409` **分母错误**（非空 8394 + NULL 18 = 8412），已更正。
  另两项精度提升：单调性统计更新为复核末次实测（**22 单调 / 18 不单调**）；
  子代理覆盖由"多数/约 25 个"改为**24/24 全部呈此形态**（独立验证确认差值仅 1 或 2）。
- **复核边界**：以上均为本机单用户、单版本（CLI 0.16.9 运行中）的只读实测。
  源码侧引用（14 个路径、`searchable_text` 上限 200000、`app-server --stdio
  --prepare-storage` spawn、E2E 的 `customCliRoot` 不存在断言、
  `defaultLegacyDir` = `~/.zcode/v2/sessions`）已逐条回查原文确认，非转述。
