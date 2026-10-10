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
  精读 `apps/zcode-cli/packages/adapters/src/storage/session-store/**`、
  `packages/desktop/src/host/**`、`packages/services/src/paths.ts`、
  `docs/zcode-agent-app-integration.md`、`docs/desktop/configurable-data-directory.md`、
  `apps/zcode-cli/docs/design/v2/session-persistence.md`。
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
| `~/.zcode/cli/db/db.sqlite` | **transcript 权威** | session / message / part / session_entry / usage 三类表 |
| `~/.zcode/v2/tasks-index.sqlite` | 列表索引（桌面侧） | 任务行、pin/archive/group/unread、全文搜索、cron 与闲时任务 |
| `<AppData>/ai.z.zcode/zcode.db` | app 元数据 / 通知 | 不存完整 transcript |

设计文档原话（`session-persistence.md`）：「核心持久化只有三层：`session`、
`message`、`part`」，并明确 `session_entry` 只做轻量 timeline 投影。
`docs/v4-refactor/15-timeline-resolver-design.md` 同样钉死三层事实源并声明
`tasks-index.sqlite` **不得作为消息顺序真相源**。

- **结论 1（已验证）**：采集 ZCode 会话正文只有 `db.sqlite` 一个来源，无需扫
  文件系统；这与 Claude Code / Codex 的 JSONL 目录形态根本不同。
- **结论 2（已验证）**：`tasks-index.sqlite` 不承载 transcript，取正文必须回
  `db.sqlite`。

## 3. CLI 会话库（已验证）

- 路径由 `getDefaultSessionDbPath()` 硬编码为 `join(homedir(), ".zcode", "cli",
  "db", "db.sqlite")`，**不读** `dataBaseDir`；契约默认值登记在
  `packages/contracts/src/config/index.ts`（`storage.sessionDbPath`），可被配置或
  `ZCODE_SESSION_DB_PATH`（兼容旧名 `ZCODE_SESSION_DB`）覆盖。
- 核心三层表：`session`（元信息 / 标题来源 / revert / 权限 / 归档时间）、
  `message`（一行一条 user 或 assistant，业务 shape 存 `data` JSON）、
  `part`（内容块：`text` / `reasoning` / `tool` / `file` / `step-start` /
  `step-finish` / `compaction` / `timeline` / `subtask`，业务 shape 存 `data` JSON）。
- 辅助表：`session_entry`（v2 UI/event 投影）、`todo`、`permission`、
  `input_history`、`session_target`、`session_input`（durable 输入账本）、
  `local_setting`、`workflow_*` / `dwf_*`、以及用量三表
  `model_usage` / `turn_usage` / `tool_usage`。
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

- 迁移账本 **27 条全部落地**（`0001`－`0022`，其中 `0019` 为 beta 前压平的单一基线）；
  前 18 条在建库时刻一次写入，后 4 条在后续版本升级时追加。
- 规模：`session` 32 行、`message` 9945 行、`part` 35147 行、`session_entry` 1859 行、
  `session_input` 408 行、`model_usage` 8081 行、`tool_usage` 8458 行、`turn_usage` 346 行、
  `input_history` 100 行（该表设计为全局上限 100，符合预期）。
- `part` 类型分布：`tool` 9011、`step-start` 8666、`step-finish` 8532、`text` 4721、
  `reasoning` 4155、`timeline` 39、`compaction` 15、`file` 8。工具状态只有
  `completed` 8840 与 `error` 171，印证「运行中不持续落库，完成时一次写」。
- `sequence` 修复彻底生效：`message` 与 `part` 的 NULL sequence **均为 0**，且
  `message_sequence_autofill` / `part_sequence_autofill` 两个触发器已在库中。
- `session_input` 账本闭环：401 条 `promoted`、4 条 `discarded`、2 条 `cancelled`、
  1 条 `admitted`；401 个 `promoted_message_id` **无悬空引用**（引用完整性成立）。

## 4. tasks-index 与 CLI 库的边界（已验证）

本机 `tasks-index.sqlite` 有 **15 行** task，CLI 库有 **32 条** session，差额完全可解释：

```text
tasks-index 15 行  ==  CLI session 中 task_type = interactive 的 15 条
差额 17 条         ==  CLI session 中 task_type = subagent_child
```

`task_id ∉ tasks-index` 的集合与 `subagent_child` 集合**完全相等，零例外**。

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

### 5.1 `message.data.tokens` 是累积上下文快照，不是单次请求用量

同一 session 内 `tokens.input` 序列严格单调递增（本机最大样本 39/39 行），而
同序列的 `tokens.output` 不单调（22/39 行）；前者是「到该轮为止的整个对话上下文
规模」，后者是本轮产出。

```text
序号推进 →  tokens.input 单调上升（例如约 22.9k → 45.9k → 67.3k → 91.3k）
            tokens.output 忽高忽低（同一区间内 266 / 550 / 39 / 1363）
```

**量级校验（本机实测）**：该 session 的 `max(tokens.input)` 为 91,254，而同一
session 在 `model_usage` 中 40 次请求的 `sum(input)` 为 2,645,421；对
`tokens.input` 逐消息求和得到约 2.49 亿，**比真实用量膨胀约 1000 倍**。

- **结论 4（已验证，最关键）**：**禁止对 `message.data.tokens` 求和**。它可用于
  取逐消息的时间戳、模型名、以及「该会话最终上下文规模」；不能用作用量总量。
- **结论 5（已验证）**：`tokens.output` 在该结构里是单轮增量，与 `model_usage`
  的 `output_tokens` 加总方向一致（未做逐条对齐，仅量级与单调性判定）。

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

本机聚合（`model_usage` 口径，占比最高的模型组）：

```text
model_id            请求数      input 合计      output 合计    cache_read 合计
（主力模型）          7247    2,883,022,714     5,081,789      2,826,623,424
```

**cache_read 占 input 的 98%**（2.83B / 2.88B）。缓存读的计费口径与普通 input
完全不同，按 input 单价计算 cache_read 会把成本高估一个量级。

- **结论 6（已验证）**：用量统计只取 `model_usage` 求和；`message.data` 只作
  交叉校验与上下文规模参考。
- **结论 7（已验证）**：`model_usage.assistant_message_id` 在本机抽样中为 `None`，
  关联到消息需用 `parent_user_message_id`。**未验证推论**：该字段在别的版本路径
  下可能被填充，使用前应先统计其非空率。

### 5.3 模型名多写法与辅助调用

- 本机 assistant 的 `modelId` 出现五种写法：`GLM-5.3-Flash`、`glm-5.3`、
  `glm-5.2`、`GLM-5.3`、`deepseek-v4.1-flash`。**大小写差异必须靠等价键吸收**，
  否则同一模型会分裂成多组。
- `message` 表的 assistant 行数（7365）与 `model_usage` 行数（7247）差 424 条，
  差额对应 `query_source` 的侧路调用（抽样可见 `session_title`，即标题生成）。
- **结论 8（已验证）**：等价键归一（ASCII 小写 + 删除 `-` `.` `_`）是必需的，不是
  防御性设计。
- **结论 9（已验证）**：`query_source` 必须参与口径决策：主会话成本与辅助调用
  成本要么分开统计，要么明确合并，不能默认混同。**未验证推论**：`query_source`
  的完整取值集合未枚举（仅抽样看到 `session_title`），实现前需先 `group by` 统计全集。

### 5.4 覆盖率（已验证）

- `message` 表中 assistant 行 **8705 / 8705 带 token 字段，零缺失**。
- `session.version` 记录创建该会话时的 CLI 版本，本机所有会话均为同一版本值。
- `provider_total_tokens` 与 `computed_total_tokens` 在抽样行上相等（883 / 883）。
  **未验证推论**：二者是否在全量上恒等未做比对，建议以 `computed_total_tokens`
  为口径、`provider_total_tokens` 作交叉校验。

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
  crash、logs、runtime）。ACP 时代旧会话位于 `~/.zcode-bak/v2/sessions/
  {workspaceHash}/{legacyTaskId}.json`，由官方 `restore-legacy-sessions` 插件
  （默认禁用）做恢复；该插件的恢复脚本要求每条 legacy message 至少写一个
  `text` part，assistant 还需补 tool part 与上一条 user 的 `parentID`。
- `tasks-index` 与 legacy snapshot 共享 `workspaceHash = sha256(workspaceKey)` 前
  12 位，`workspaceKey = workspaceIdentity?.trim() || workspacePath`（远端优先
  identity）。**未验证推论**：若按路径枚举 workspace 目录名，需注意该 hash 不是
  路径明文。

## 8. 对 TokenScope 的落地建议

1. **采集来源**：正文与用量取 `%USERPROFILE%\.zcode\cli\db\db.sqlite`，**恒定不读
   `dataBaseDir`**；`tasks-index.sqlite` 取 `%USERPROFILE%\.zcode\v2\`（或自定义根
   下的 `.zcode\v2\`）。ZCode 是结构化库来源，不需要 `source` 层的文件发现逻辑。
2. **用量主口径**：`SUM(model_usage.*)`。**禁止对 `message.data.tokens` 求和**；
   后者只用于逐消息时间戳、模型名与最终上下文规模。
3. **等价键**：模型名大小写 + `-` `.` `_` 归一为必需项，否则本机即出现三组分裂。
4. **辅助调用**：先枚举 `query_source` 全集，再决定是否单列（本机已见
   `session_title`，约占 5% 请求数）。
5. **缓存口径**：`cache_read_input_tokens` 单独计价；缺失缓存读价的模型必须显式
   标注未知，不得按 input 价静默代入（本机 cache_read 占 input 98%，误算影响极
   大）。
6. **子代理用量**：若按任务列表枚举会漏掉 `task_type = subagent_child` 的会话；
   应直接按 `model_usage.session_id` 聚合，不依赖 tasks-index。
7. **活库读取**：`db.sqlite` 与 `tasks-index.sqlite` 都有常驻 `-wal`/`-shm`
   （本机 WAL 可达数 MB）。必须按「带 WAL 的活库」处理：以只读 URI 打开并接受
   读到的 WAL 内数据，不能只拷主库文件或假设主库即全量。
8. **重试归并**：`logical_request_id` + `attempt_index` 可识别重试；`retry_count`
   与 `status` 应参与统计口径，避免把重试算成独立请求。
9. **风险提示（三处）**：a) `tokens.input` 误求和导致数量级膨胀；b) `query_source`
   未区分导致辅助调用混入主口径；c) 模型名未归一致使分组分裂。

## 9. 待办与未覆盖

- `query_source` 取值全集统计；`turn_usage` 与 `model_usage` 的加总一致性核验。
- `provider_total_tokens` 与 `computed_total_tokens` 的全量等价性验证。
- 源码克隆版本（v3.15.1 / CLI 0.16.9）与本机运行版本的 schema 是否完全一致，需在
  新版本发布时复核；`message` / `part` 的 JSON 字段拼写在源码 Reader 与本地载荷
  之间存在大小写差异（源码读取 `modelID`/`providerID`，本机载荷为
  `modelId`/`providerId`），**未验证推论**：可能另有归一入口或版本差异，实现解析
  时以实际载荷为准，不要照抄源码字段名。
- 远端（SSH/WSL/Docker/Server）workspace 的库位置与 `attachment` 路由未采样；
  `packages/client`、`packages/server`、`packages/zcode-server-cli`、`packages/android`
  未读内容。若要支持跨机用量归集，需单独立项。
