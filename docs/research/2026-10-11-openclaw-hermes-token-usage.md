# OpenClaw 与 Hermes Agent 的 token 用量可读性核验（候选来源前置研究）

> 研究目标：cc-switch 目前对外声明「OpenClaw 和 Hermes 暂不支持用量统计」，本文核验该判断在
> 两个上游项目当前版本上是否仍然成立——即**外部只读工具能否从它们的本地持久化数据里可靠重建
> token 用量**，并给出可操作的读取路径。
>
> 样本（均为浅克隆 HEAD，只读分析，未运行、未改动其数据）：
> - `openclaw/openclaw` @ `e40606fd`（2026-10-10），TypeScript/Node，pnpm workspace
> - `NousResearch/hermes-agent` @ `66605471`（2026-10-10），Python 为主
>
> 本机**未安装**这两个工具（`~/.openclaw`、`~/.hermes`、`%LOCALAPPDATA%\hermes` 均不存在），
> 因此结论全部来自源码与官方文档，不含真实样本回放。
>
> 证据分档：**已验证**（代码/文档可证）／**未验证推论**。

## 1. cc-switch 侧的现状声明（核验的出发点）

- 用户手册与 UI 文案三处一致："OpenClaw 和 Hermes 暂不支持用量统计"
  （`docs/user-manual/zh/4-proxy/4.4-usage.md:22,85,143`、`src/i18n/locales/zh.json:2411`
  的 `"Hermes、OpenClaw 暂不支持用量统计"`）。
- cc-switch 的用量面板应用筛选（`KNOWN_APP_TYPES`）只含
  claude/codex/gemini/grokbuild/opencode/pi/mcode，不含 openclaw/hermes。
- cc-switch 对这两个应用的集成面**只有配置读写**：
  - OpenClaw：`~/.openclaw/openclaw.json`（JSON5）的 providers / default model / env / tools
    读写与健康检查（`src-tauri/src/openclaw_config.rs:33-47` 等）。
  - Hermes：配置目录解析为「CCS 设置 `hermes_config_dir` > `HERMES_HOME` 环境变量 >
    平台默认（Windows `%LOCALAPPDATA%\hermes`，Mac/Linux `~/.hermes`）」，
    操作 `config.yaml` 的 providers / model / mcp_servers 与 memory 文件
    （`src-tauri/src/hermes_config.rs:47-101,1006-1120`）。
- **含义**：cc-switch 说的"不支持"是指它自己没有为这两家实现用量采集适配器——并**不等于**
  两家没有可读的用量数据。本文正是要区分这两件事。

## 2. Hermes Agent：token 计量确实落盘，但粒度只到"会话 × 路由"（已验证）

Hermes 把用量写进自己的状态库 `state.db`，字段相当完整（五桶 token + 成本 + 调用次数），
**但没有逐请求行**；该差异的后果与障碍集中在 §4.1、§4.2。

### 2.1 库位置与形态（已验证）

- 默认路径 `<HERMES_HOME>/state.db`：`HERMES_HOME` 未设时，**Windows 取
  `%LOCALAPPDATA%\hermes`，POSIX 取 `~/.hermes`**（`hermes_constants.py:51-58,111-118`）；
  profile 形态下为 `<root>/profiles/<name>/state.db`
  （`hermes_state.py:176-201`、`hermes_state_profile_repair.py:4-5`）。
- 形态为 SQLite，**默认 WAL**（`apply_wal_with_fallback`，`hermes_state.py:793-806`）；
  在 mmap + 字节锁不可靠的文件系统上（NFS / SMB / 部分 FUSE / WSL1）会**回退 DELETE 模式**
  （`hermes_state_wal.py:25-28`）——这决定外部读取是否要处理 `state.db-wal` / `-shm` 边车文件。
- 只读打开有官方支持路径：`read_only_db_uri()` 生成 `file:...?mode=ro`
  （`hermes_state_holders.py:27-30`），并且 `collect_state_db_stats()` 明确演示了
  「`mode=ro` + 短超时」的只读探测（`hermes_state_dbfile.py:745-767`）。
  **对 TokenScope 这类只读工具是可用形态。**

### 2.2 用量字段与列（已验证）

`hermes_state_usage.py` 是专用模块（文件头自述"Token/usage accounting mixin for SessionDB"），
其中定义了五桶计数器与两组写入语句：

```python
# hermes_state_usage.py:17
_TOKEN_COUNTERS = ("input_tokens", "output_tokens", "cache_read_tokens",
                   "cache_write_tokens", "reasoning_tokens")
```

`UPDATE sessions SET ...` 覆盖的用量/成本/路由列（`hermes_state_usage.py:20-46`）：
`input_tokens`、`output_tokens`、`cache_read_tokens`、`cache_write_tokens`、`reasoning_tokens`、
`estimated_cost_usd`、`actual_cost_usd`、`cost_status`、`cost_source`、`pricing_version`、
`billing_provider`、`billing_base_url`、`billing_mode`、`model`、`api_call_count`。

**写入机制**（`hermes_state_usage.py:20-27,106-231`）：同一份 SQL 模块里存在**两种模式**——
delta 模式（`col + ?`，累加）与绝对模式（`?`，直接赋值）；**当前生产路径走 delta 累加**
（每次成功的模型响应后 `queue_token_counts(..., api_call_count=1)`，见 §2.5），
绝对模式仅出现在测试代码中（本文未在生产调用链上找到覆写写法，属**未验证推论**）。

因此外部读取方**不需要**做差分，但必须知道：`sessions` 行上的 token 是**会话级累计**，
而 `api_call_count` 是该会话的调用次数——**两者相除只能得到平均，无法还原逐请求明细**。

**库版本**：`SCHEMA_VERSION = 31`（`hermes_state_common.py:283`），且从 v17 起才有
`session_model_usage`、其后经历多次建表/主键 heal（`hermes_state_schema.py:864-916,1047-1051,
1139-1170`）——外部读取必须容忍缺表/缺列。

### 2.3 按模型/渠道的聚合表（已验证）

```sql
-- hermes_state_usage.py:52-59（原样）
INSERT INTO session_model_usage (
    session_id, model, billing_provider, billing_base_url, billing_mode,
    task, api_call_count, input_tokens, output_tokens,
    cache_read_tokens, cache_write_tokens, reasoning_tokens,
    estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
    first_seen, last_seen
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(session_id, model, billing_provider, billing_base_url, billing_mode, task)
DO UPDATE SET api_call_count = api_call_count + excluded.api_call_count, ...
```

**这是本研究的核心发现**：Hermes 维护一张 `session_model_usage` 表，维度是
「会话 × 模型 × 计费渠道（provider/base_url/mode）× task」，指标含 `api_call_count`、
五桶 token、估算与实付成本、`first_seen`/`last_seen`。它是**外部工具能拿到的最细粒度**——
再往下（单次调用）就没有行了，见 §2.5。
建表原文（`hermes_state_common.py:482-502`）：

```sql
CREATE TABLE IF NOT EXISTS session_model_usage (
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    billing_provider TEXT NOT NULL DEFAULT '',
    billing_base_url TEXT NOT NULL DEFAULT '',
    billing_mode TEXT NOT NULL DEFAULT '',
    task TEXT NOT NULL DEFAULT '',
    api_call_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_write_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    estimated_cost_usd REAL NOT NULL DEFAULT 0,
    actual_cost_usd REAL NOT NULL DEFAULT 0,
    cost_status TEXT,
    cost_source TEXT,
    first_seen REAL,
    last_seen REAL,
    PRIMARY KEY (session_id, model, billing_provider, billing_base_url, billing_mode, task)
);
```

### 2.4 会话主表同时带项目路径与 git 元数据（已验证）

`sessions` 表（`hermes_state_common.py:382-429`，节选关键列）：

```sql
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY, source TEXT NOT NULL, ...,
    model TEXT, model_config TEXT, parent_session_id TEXT,
    started_at REAL NOT NULL, ended_at REAL, end_reason TEXT,
    message_count INTEGER DEFAULT 0, tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0, output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0, cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    cwd TEXT, git_branch TEXT, git_repo_root TEXT, git_metadata_generation INTEGER NOT NULL DEFAULT 0,
    billing_provider TEXT, billing_base_url TEXT, billing_mode TEXT,
    estimated_cost_usd REAL, actual_cost_usd REAL,
    cost_status TEXT, cost_source TEXT, pricing_version TEXT,
    title TEXT, ..., last_activity_at REAL, api_call_count INTEGER DEFAULT 0, ...
);
```

两点值得单独指出：

- **会话行自带 `cwd` / `git_branch` / `git_repo_root`**（注意是 `sessions` 表——用量聚合表
  `session_model_usage` 里**没有**这些列，要项目维度必须 JOIN 回 `sessions`）：这是 cc-switch
  的用量表**完全没有**的维度，且 `git_repo_root` 天然就是"项目根"语义（正是 TokenScope
  项目身份模型需要的锚点）。
- `source`、`session_key`、`chat_id`/`chat_type`/`thread_id` 等列说明 Hermes 的会话来源很多
  （CLI / gateway / 平台适配器）；跨来源汇总时**必须先按 `source` 分组或过滤**，
  否则会把不同入口的会话混成一个总量。

### 2.5 写入时机、丢失边界与旁路信号（已验证）

- **写入时机**：**每次成功的模型响应后**（不是会话收尾）由 `agent/turn_usage.py:251-275` 调
  `queue_token_counts(..., api_call_count=1)`；内存队列由后台线程合并后执行 `UPDATE sessions`
  + UPSERT `session_model_usage`（`hermes_state_usage.py:106-231,273-367`）。
  持久化点：每次 flush（`agent/session_persistence.py:451-453`）、`close()` drain
  （`hermes_state.py:1525`）、进程 atexit（`hermes_state_usage.py:126-132`）。
- **没有 per-request 行**：同一 `(session, model, provider, base_url, mode, task)` 的多轮调用
  会被 UPSERT **累加折叠**成一行；`messages.token_count` 只在导入场景可能写入，
  agent 正常 flush **恒写 NULL**（`agent/session_persistence.py:230`、
  `agent/transcript_repair.py:158-161`）——所以想摊分到单次调用，**库里没有可用的锚**。
- **丢失边界**：队列在内存、writer 每批立即写；apply 失败只 warning；writer 若 10 秒内未停，
  残留 delta 不落库（`hermes_state_usage.py:197-210,233-259`）→ **崩溃最多丢最后一批**，
  运行中会话的读数可能滞后数秒。
- **辅助调用单独记**：vision / compression / title_generation / background_review / voice_chat
  走 `record_auxiliary_usage`，**只写 `session_model_usage` 的对应 `task` 行、不动 `sessions`
  汇总**（`hermes_state_usage.py:369-391`）→ 读取时必须按 `task` 区分主循环与辅助调用。
- **多路由归属**：`sessions` 行只保留一个 route（首个 accounted，或在 `/model` 切换时被覆盖），
  真正的逐路由归因在 `session_model_usage`（`hermes_state_usage.py:85-104,305-314`；
  注释原文："keyed by live model"）。
- **旁路信号（逐调用细节只能在这里看到）**：
  - `$HERMES_HOME/logs/agent.log`：每次 API 调用一行
    `API call #N: model= provider= in= out= total= latency cache=read/write ...`，
    带 `[session_id]` 标签，5 MB × 3 轮转（`hermes_logging.py:107,371-384`、
    `agent/turn_usage.py:205-210`）——**文本日志，不是事实源**，但足以核对逐次调用。
  - `$HERMES_HOME/cron/usage_audit.jsonl`：每次 cron fire 一行
    （`ts/job_id/fire_id/prompt_tokens/completion_tokens/total_tokens/model/duration_ms`，
    `cron/scheduler.py:1254-1274,2502-2522`）。
  - `hermes -z --usage-file <path>`：opt-in 的 JSON 使用报告（`hermes_cli/oneshot.py:215-235`）。
  - `$HERMES_HOME/telemetry/shared_metrics`：默认关闭；启用后是聚合计数包
    （含 model/provider/call_role/aux_task 维度，`cli-config.yaml.example:2116-2143`）。
- **纯内存、不落盘**：`agent.session_*_tokens` 与 TUI 的 `session.usage` RPC
  （`tui_gateway/methods_session.py:1454-1470`）——外部工具读不到，不要指望它们。
- **两条容易被误认为用量载体的路径（否证）**：`request_dump_<sid>_<ts>.json` 只在 **API 错误**
  时写请求体、**不含 usage**（`agent/agent_runtime_helpers.py:1576-1600`）；
  `$HERMES_HOME/sessions/<id>.jsonl` 只在 state.db 被替换时做分流兜底，**也不是用量载体**
  （`hermes_state.py:431-439`）。

## 3. OpenClaw：每条 assistant 事件自带 usage，落在 per-agent SQLite（已验证）

**先厘清层次**：下面 §3.1 官方 schema 文档描述的 `SessionEntry` 计数器只是**会话级的冗余汇总**；
真正的**事实源**是 per-agent SQLite 的 `transcript_events`——每条 assistant 事件各带一份
`usage`（含缓存读写与成本字段；个别路径会丢 `cacheWrite1h`，见 §4.3），OpenClaw 自己在实时状态
缺失时也会回读它当权威值。因此 OpenClaw 这边能重建到"逐条回复"粒度（读取路径见 §4.3）。

### 3.1 官方 schema 文档的口径（已验证）

`docs/reference/session-management-compaction/schema.md` 第 35-59 行描述 Session store：

- store 是 **per-agent SQLite**，值类型为 `SessionEntry`（定义在 `src/config/sessions.ts`）；
- 第 57 行原文：`Token counters (best-effort/provider-dependent): inputTokens, outputTokens,
  totalTokens, contextTokens`；
- 历史形态是 `sessions.json`，迁移命令为
  `openclaw doctor --session-sqlite import --session-sqlite-all-agents`（第 70-72 行），
  且明确"**不要**再编辑 `sessions.json` 并指望运行时读它"。

### 3.2 转录（transcript）事件流（已验证）

同一文档第 74-89 行：transcript **append-only**，首条是 session header
（`type: "session"`、`id`、`cwd`、`timestamp`、可选 `parentSession`），随后条目带
`id` + `parentId` 构成树；条目类型含 `message`、`custom_message`、`custom`、`compaction`
（带 `firstKeptEntryId` 与 **`tokensBefore`**）、`reset`、`branch_summary`。
转录身份已迁到 SQLite（文档称 "the SQLite transcript identity"）。

**与后文衔接**：这里描述的是**事件载荷结构**；现行版本把它承载在 per-agent SQLite 的
`transcript_events.event_json` 里（legacy 才是 `sessions/*.jsonl` 的文件行），详见 §4.3。
即**同一套条目结构，介质从"文件行"换成了"表行"**——读用量时不要以为存在两套数据。

### 3.3 UI 与导出暴露的字段面（已验证）

`ui/src/pages/usage/**` 是一整套用量页面（overview / timeseries / details / metrics / chart /
query），并且**自带 CSV 导出**，列头出现在测试断言里：

```
date,inputTokens,outputTokens,cacheReadTokens,cacheWriteTokens,totalTokens,
inputCost,outputCost,cacheReadCost,cacheWriteCost,totalCost
```

（`ui/src/pages/usage/query.test.ts:101`；同页还提供 JSON 导出，见下节引用的
`api-usage-costs.md:31`）

即 OpenClaw 内部**确实持有**按天聚合、带缓存分列与成本的用量，并可按 provider / model / agent /
channel 维度下钻。问题是这份数据的**权威来源与落点**——答案在下一节。

### 3.4 官方文档明确了"本地派生"这条数据线（已验证）

`docs/concepts/usage-tracking.md` 与 `docs/reference/api-usage-costs.md` 把 OpenClaw 的用量分成
**两类来源**，这对判断可读性至关重要：

1. **provider 报告的配额/账单**（需要凭据 + 联网，经各 provider 插件的 `fetchUsageSnapshot`）：
   `openclaw status --usage`、`/status` 的 provider 窗口（`X% left`）、Anthropic/OpenAI Admin API
   的组织级花费历史。**这一类本地不可读**（要密钥、要网络）。
2. **本地 session / transcript 派生的 token 与估算成本**（`docs/reference/api-usage-costs.md:27-34`）：
   > Control UI → Usage：Shows **transcript-derived** token and estimated-cost totals for the
   > selected date range, with breakdowns by provider, model, agent, channel, and token type.

   同文档还写明 `/usage cost` 是 "local cost summary aggregated from **OpenClaw session logs**"
   （`usage-tracking.md:19`），且 **Session 级 `/status` 在实时快照缺 token/model 数据时会回退到
   transcript 日志**恢复计数（`usage-tracking.md:13`、`api-usage-costs.md:18`）。

### 3.5 契约层：一轮对话可渲染的用量字段（含缓存与成本）（已验证）

`/usage full` 的模板契约（`docs/concepts/usage-tracking.md:272-294`）逐条列出了**每轮渲染时可用**的
用量路径——这是目前看到的**最完整的字段清单**。注意它是**契约层**（模板能引用的字段），
与"数据层实际落盘的字段"不完全等同（例如 `reasoning` 在数据层没有独立字段，见 §4.3）：

| 契约路径 | 含义 |
| --- | --- |
| `usage.input_tokens` / `output_tokens` / `total_tokens` | 该 turn 的聚合 |
| `usage.cache_read_tokens` / `cache_write_tokens` | 缓存读 / 缓存写 |
| `usage.cache_hit_pct` | 缓存读占 prompt token 的比例 |
| `usage.last.input_tokens` / `output_tokens` / `cache_read_tokens` / `cache_write_tokens` / `total_tokens` / `cache_hit_pct` | **最后一次模型调用**的同类指标 |
| `cost.turn_usd` / `cost.available` | 该 turn 的估算成本 / 是否有价格表可用 |
| `timing.duration_ms` | turn 挂钟耗时 |
| `model.id` / `model.actual` / `model.requested` / `model.provider` / `model.reasoning` | 模型与推理档 |
| `context.max_tokens` / `used_tokens` / `pct_used` | 上下文窗口占用 |
| `session.id` | 会话 id |

**含义**：OpenClaw 内部**每次 turn 都有输入/输出/缓存读写与成本**——问题只剩"这些值落不落盘、
落在哪、字段名是什么"。这是 §4 结论要回答的最后一环。
（注意：契约里**没有**独立的 `reasoning_tokens` 路径；缓存写的 1 小时子集落在数据层字段
`usage.cacheWrite1h`（§4.3 已验证），契约层未单列。）

## 4. 结论与可操作路径

### 4.1 一句话结论

| 上游 | 能否外部只读重建用量 | 粒度上限 | 含成本 | 含项目路径 | 历史窗口 |
| --- | --- | --- | --- | --- | --- |
| **Hermes Agent** | **部分能**（会话 / 路由 / 按天；**不能**逐请求） | 会话 × 模型 × 计费渠道 × task | 有（估算 + 实付双列 + 状态标注） | **有**（`cwd` / `git_repo_root`） | **默认仅 90 天**（`auto_prune`，见 §4.2） |
| **OpenClaw** | **部分能**（逐条 assistant 回复；需处理压缩载荷与驱逐） | 每条回复（transcript 事件） | 有（条目内 `cost`，含 `totalOrigin`） | 事件可带 `cwd` | `pruneAfter` 默认 30 天 + 磁盘预算驱逐 |

**因此 cc-switch 的「暂不支持用量统计」是实现选择（它没写这两家的适配器），不是"数据不存在"。**
两家的用量都在本地落盘、且都是**外部可只读解析**的形态；差别在粒度与稳定性：
Hermes 只到"会话 × 路由"聚合且默认只保 90 天；OpenClaw 能到"每条 assistant 回复"，但数据面是
带压缩载荷的 per-agent SQLite，且**缺状态码、首字延迟这类代理侧字段**，还受保留策略与磁盘预算
驱逐影响。相比之下 cc-switch 的明细行本身带 TTFT、状态码与错误信息——接入这两家时仍要按各自
粒度重新定义事件模型，不能照搬它的字段假设。

### 4.2 Hermes 的读取路径（结构最规整，但只能到"会话 × 路由"）

发现顺序（与 Hermes 自身解析一致）：

1. `HERMES_HOME` 指定的目录（`hermes_constants.py:111-118`）；
2. 平台默认：**Windows `%LOCALAPPDATA%\hermes`，POSIX `~/.hermes`**
   （`hermes_constants.py:51-58`；目录名后缀可由 `HERMES_DATA_DIR_SUFFIX` 改）；
3. 多 profile：逐个 `<root>/profiles/*/state.db`（`hermes_cli/profiles.py:283-286`）。

按 Hermes 自己的方式**只读 URI 打开**（`file:...?mode=ro`，注意必须用 `as_uri()` 做百分号编码，
否则路径里的 `?`/`#` 会截断成错误的空库，`hermes_state_holders.py:27-30`）：

```python
uri = Path(db_path).resolve().as_uri() + "?mode=ro"
conn = sqlite3.connect(uri, uri=True, timeout=5.0)   # 活跃 writer 下需自带重试
```

四处最小读取（按用途）：

```sql
-- 1) 会话级（含项目路径、五桶 token、成本与调用次数）
SELECT id, source, model, billing_provider, billing_base_url, billing_mode,
       started_at, ended_at, parent_session_id, archived, pinned,
       input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
       api_call_count, estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
       pricing_version, cwd, git_repo_root
FROM sessions;

-- 2) 路由级（模型 / provider / 渠道 / task 拆分——外部工具能拿到的最细粒度）
SELECT session_id, model, billing_provider, billing_base_url, billing_mode, task,
       api_call_count, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
       reasoning_tokens, estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
       first_seen, last_seen
FROM session_model_usage;

-- 3) 按天（复刻 Hermes dashboard 口径：按 started_at 归日 + 本地时区）
SELECT date(started_at,'unixepoch','localtime') AS day,
       SUM(input_tokens), SUM(output_tokens), SUM(cache_read_tokens), SUM(reasoning_tokens),
       SUM(COALESCE(api_call_count,0)), COALESCE(SUM(estimated_cost_usd),0)
FROM sessions WHERE started_at > ? GROUP BY day ORDER BY day;

-- 4) 按模型/provider（注意 task 过滤）
SELECT u.model, u.billing_provider, u.task, SUM(u.api_call_count),
       SUM(u.input_tokens), SUM(u.output_tokens), COALESCE(SUM(u.estimated_cost_usd),0),
       MAX(u.last_seen)
FROM session_model_usage u JOIN sessions s ON s.id = u.session_id
WHERE s.started_at > ? GROUP BY u.model, u.billing_provider, u.task;
```

**口径陷阱与硬障碍（按影响排序）**：

- **粒度上限是"会话 × 路由 × task"，没有逐请求行**：同键多轮调用被 UPSERT 累加折叠，
  只留 `first_seen`/`last_seen`；备选字段 `messages.token_count` 在生产路径**恒为 NULL**
  （`agent/session_persistence.py:230`、`agent/transcript_repair.py:158-161`），
  不能用来摊分单次调用。
- **默认 90 天自动清理，且级联删用量行**：`sessions.auto_prune=true`、`retention_days=90`
  （`hermes_cli/config_defaults.py:2264-2297`），启动时（≥24 小时间隔）删掉
  `ended_at IS NOT NULL` 且未 pinned 的会话；`session_model_usage` 声明 `ON DELETE CASCADE`
  且写连接开启 `PRAGMA foreign_keys=ON`（`hermes_state.py:807`）→ **用量行一起消失**
  （`hermes_state_maintenance.py:322-353,421-522`）。要长期历史必须让用户改配置。
- **provider 维度必须读 `session_model_usage`**：`sessions` 行只保留一个路由三元组，且 `/model`
  切换会**无条件覆盖**它（`hermes_state_usage.py:85-104`）；按 sessions 分组会把整段会话错误
  归因（Hermes 自己在 `hermes_cli/web_routers/analytics.py:310-315` 给了同款注释）。
- **"总量"有两套口径**：`session_model_usage` 求和 + 与 `sessions` 残余对账
  （`agent/insights.py:358-369`：逐会话取 `max(0, sessions 值 − 已归因值)`，防漏防重）；
  全局汇总 `usage_totals()` 的过滤条件是 `parent_session_id IS NULL`（**排除子会话 / spawn**）
  **且 `message_count >= 1`**，并默认排除 archived（实测 `hermes_state_usage.py:422-425` 的
  `where = ["parent_session_id IS NULL", "message_count >= ?"]`）。
  外部工具要与 Hermes 的数字对得上，就得复刻这些规则。
- **`task` 区分主循环与辅助调用**：`task=''` 是主 agent 循环，非空是辅助任务
  （vision / compression / title_generation / background_review / voice_chat）；
  是否计入完全取决于产品口径（§2.5）。
- **成本不是账单**：`estimated_cost_usd` 是 Hermes 本地定价估算；`cost_status` 取值
  `actual / estimated / included / unknown`（`agent/usage_pricing.py:58,680-683`）——
  `included` 表示订阅内零账单，`unknown` 时数值无意义。
- **读写并发有毫秒级 `SQLITE_IOERR` 窗口**：WAL 下 `mode=ro` 读者撞上 writer 的 checkpoint
  时会被拒（ro 连接无法做 WAL 恢复），Hermes 自己为此加了重试预算
  （`hermes_state.py:183-190`）→ 外部读取要自带退避重试。
- **异步写入意味着读数滞后**：队列在内存、空闲 30 秒退休、崩溃丢最后一批
  （`hermes_state_usage.py:241-246`、`hermes_state.py:509`）。
- **schema 会漂移**：`SCHEMA_VERSION = 31`；`session_model_usage` 自 v17 才有，v20 播种、
  v22 二次重建加 `task` 进主键；Hermes 自己在 pre-v17 库上也做了缺表回退
  （`hermes_cli/web_routers/analytics.py:338-339`）。

### 4.3 OpenClaw 的读取路径（粒度更细，但要处理压缩载荷与驱逐）

用量有两条线，只有一条能离线读（§3.4）：**本地 transcript 派生那条**可以，
**provider 配额/账单那条**要凭据和网络，离线不可读（`status --usage` 属后者，
`docs/concepts/usage-tracking.md:11,20`）。

**主数据面是 per-agent SQLite，不是 JSONL**：

| 用途 | 路径 | 格式 |
| --- | --- | --- |
| 会话行 + transcript（**现行**） | `<stateDir>/agents/<agentId>/agent/openclaw-agent.sqlite` | SQLite STRICT |
| usage rollup 缓存（派生） | 同库 `cache_entries`（scope `session-cost-usage-rollup-v3`） | JSON/BLOB |
| legacy / 归档 transcript | `<stateDir>/agents/<agentId>/sessions/` | JSONL（可 `.zst`） |
| 共享状态库 | `<stateDir>/state/openclaw.sqlite` | SQLite |

状态根默认 **`~/.openclaw`**（Windows 即 `C:\Users\<user>\.openclaw`），可由
`OPENCLAW_STATE_DIR` 覆盖；home 解析序为 `OPENCLAW_HOME > HOME > USERPROFILE > Termux 目录 > os.homedir`
（`src/config/state-dir.ts:10-28`；`packages/normalization-core/src/home-dir.ts:28-53`，其中
`OPENCLAW_HOME` 在第 50 行、`HOME/USERPROFILE` 在第 32-33 行）。

transcript 表（`src/state/openclaw-agent-schema.sql:439-449`，节选）：

```sql
CREATE TABLE IF NOT EXISTS transcript_events (
  session_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  event_json TEXT,
  created_at INTEGER NOT NULL,
  event_zstd BLOB,
  event_utf8_bytes INTEGER CHECK (...),
  navigation_json TEXT,
  PRIMARY KEY (session_id, seq),
  FOREIGN KEY (session_id) REFERENCES "session_windows"(session_id) ON DELETE CASCADE
) STRICT;
```

**注意那条外键**：转录事件挂在 `session_windows` 上并 `ON DELETE CASCADE`——会话窗口被清理时
事件行随之消失，这是"历史会被驱逐"在库结构上的另一半证据（另一半是本节的保留策略）。

**粒度**：每次模型响应 = 一条 assistant 事件，各带自己的 `usage`——所以 OpenClaw 这边
**能重建到"逐条 assistant 回复"**（这正是它自己 `/usage cost` 与用量页面在做的事）。
两点限定：**多数 harness 下"一条回复 = 一次模型调用"**，但官方文档提到个别 harness
（native Codex）可能按"回合内多响应求和"上报（§4.6 的未验证项），那一类记录会是回合级；
此外能否读出还受压缩载荷与保留策略影响（见下）。最强的自证是：OpenClaw 在实时快照缺数据时
会**从持久化 transcript 回读**这份 usage 当权威值
（`src/config/sessions/session-transcript-accounting.ts:30-44`，尾部最多
`SQLITE_USAGE_TAIL_MAX_EVENTS = 512` 条）。

字段清单（`Usage` 接口，`packages/llm-core/src/types.ts:304-327`）：

| 语义 | 字段 |
| --- | --- |
| 输入 / 输出 | `usage.input` / `usage.output` |
| 缓存读 / 写 | `usage.cacheRead` / `usage.cacheWrite` |
| 缓存写 1 小时子集 | `usage.cacheWrite1h`（5m = `cacheWrite − cacheWrite1h`，`packages/ai/src/providers/anthropic-usage.ts:39-101`） |
| 总量 | `usage.totalTokens` |
| 成本 | `usage.cost{input,output,cacheRead,cacheWrite,total}` + `totalOrigin`（`"provider-billed"` = 上游自报） |
| 模型 / provider | `message.provider` / `message.model` / `message.responseModel` / `message.api` |
| 时间 | `message.timestamp`（**毫秒**）；事件 `timestamp`（ISO 字符串）；库列 `created_at`（毫秒） |

**验证过的障碍清单**：

- **载荷可能被 zstd 压缩**：`event_json` 可能为 NULL，正文落在 `event_zstd`（标准 zstd 帧，
  按 `event_utf8_bytes` 限长解压）。触发条件（实测 `src/config/sessions/transcript-payload.ts:232-272`）：
  存在可用 zstd codec、原始载荷 ≥1024 字节（`MIN_COMPRESS_BYTES`，第 24 行）、能 UTF-8 往返、
  不含 `\u` 转义或 `\0`、且原始不超过 4 MiB（`MAX_COMPRESSED_EVENT_BYTES`，第 22 行）；
  最终还有一道**收益门槛**——压缩体（加上导航元数据）必须比原始至少小
  `max(64 字节, 10%)`，否则保持明文。解码器是 Node `zlib` 的 zstd
  （`src/infra/zstd-codec.ts:1,16`），因此任何标准 zstd 实现都能读；
  **不满足上述条件的记录会原样留在 `event_json` 里**（读取方两种列都要处理）。
- **历史会被驱逐**：`pruneAfter` 默认 **30 天**、`maxEntries` 5000、`maxDiskBytes` **10 GiB
  且默认 enforce**、high-water 0.8（`src/config/sessions/store-maintenance.ts:27-38`）——
  磁盘预算压力下会依次回收 checkpoint、删归档产物、**最后驱逐最旧的已归档会话**
  （`docs/gateway/config-agents/sessions.md:127-137`）。**历史长度没有保证**。
  上下文压缩（compaction）不会删 transcript 本体（`docs/concepts/compaction.md:54`）。
- **incognito 会话完全不可读**（历史只活在 Gateway 进程内，`store.md:58`）。
- **字段缺失与口径**：`cacheWrite1h` 在 worker 投影路径会丢
  （`src/worker/assistant-message-projection.ts:40-55`）；**reasoning token 没有独立落盘字段**；
  OpenClaw 读取端自己会把 `cacheRead` 从 `input` 里扣回"非缓存输入"
  （`src/agents/usage.ts:191-217`）——外部工具直读原始字段时必须自己对齐这个口径。
- **成本不可复现**：外部工具无法知道当时的目录价，只能采信已落盘的 `cost.total` /
  `totalOrigin`；没记录成本的条目只能读成"未知"。
- **rollup 缓存不可当事实源**：源码明说缓存可重建、语义变更即换版本忽略旧行，且绑定
  `pricingFingerprint`（`src/infra/session-cost-usage-rollup-codec.ts:7-9`，当前
  `USAGE_COST_ROLLUP_VERSION = 6`、scope 为 `session-cost-usage-rollup-v3`）——只能当加速项。
- **双源并存**：同一批用量可能同时存在于 JSONL 与 SQLite，OpenClaw 自己是"SQLite 优先 +
  文件名判定"（`src/infra/session-cost-usage-collection.ts:239-243`）。

读取骨架与去重（字段均已由源码验证）：

```sql
SELECT e.session_id, e.seq, e.created_at,
       COALESCE(e.event_json, e.event_zstd) AS payload, e.event_utf8_bytes
FROM transcript_events e
JOIN session_windows w ON w.session_id = e.session_id
WHERE e.session_id = ? ORDER BY e.seq;
```

解压后筛 `message.role = 'assistant'`，取最小字段集：
`session_id, seq, provider, model, ts, input, output, cacheRead, cacheWrite, totalTokens,
cost.total, cost.totalOrigin`。**去重用** `transcript_event_identities(session_id, event_id)`
（`message_idempotency_key` 上有唯一索引，`openclaw-agent-schema.sql:556-570`）——
重放与追加重试会产生同 key 的行，按它去重才能与 OpenClaw 自己的计数一致。

### 4.4 与 cc-switch 现有来源的对照

| 维度 | cc-switch 会话来源（7 家） | Hermes | OpenClaw |
| --- | --- | --- | --- |
| 落点 | 各自会话日志/库 → 抄进 cc-switch 的 `proxy_request_logs` | 留在自己的 `state.db` | 留在 per-agent SQLite 的 `transcript_events`（legacy JSONL） |
| 粒度 | 多数是"逐请求/逐消息" | **会话 × 模型 × 渠道 × task 聚合** | **逐条 assistant 事件** |
| 项目路径 | **全部缺失** | **有 `cwd` / `git_repo_root`** | 事件可带 `cwd` |
| 缓存读写 | 分列（各来源能力不同） | 分列（五桶齐全） | 分列（含 `cacheWrite1h` 子集）；**无 reasoning 字段** |
| 成本 | 自己按价格表算 | 自报 + 估算双列（带 `cost_status`/`cost_source`） | 条目内 `cost`（带 `totalOrigin`），缺时才按本地价补估 |
| 时间 | 逐条时间戳 | 区间（`first_seen`/`last_seen`） | 逐条（`message.timestamp`，毫秒） |
| 离线可读 | 是 | **是**（只读 SQLite，需重试退避） | **是**（只读 SQLite，需解 zstd 载荷） |
| 历史窗口 | 30 天明细（汇总保留） | **默认 90 天**（自动清理 + 级联） | `pruneAfter` 30 天 + 10 GiB 磁盘预算驱逐 |

### 4.5 若要接入，建议的优先级与做法

1. **要项目维度与结构化数据 → 先做 Hermes**：单个 SQLite、五桶 token + 成本 + 调用次数 +
   `cwd`/`git_repo_root` 一次到位，且 Hermes 自己的 Web/CLI 就是这么读的（可对齐口径）。
   代价是必须接受"会话 × 路由"粒度与默认 90 天清理。
   做法：只读打开 `state.db` → `session_model_usage` JOIN `sessions` → 映射为**会话级用量事件**
   （不要伪造逐请求事件）；`api_call_count` 用于请求数，`task` 决定是否计入辅助调用。
2. **要更细粒度 → 再看 OpenClaw**：主数据面是
   `<stateDir>/agents/*/agent/openclaw-agent.sqlite` 的 `transcript_events`
   （每条 assistant 事件一行，比 Hermes 更接近"一次调用一行"），legacy JSONL 兼容读。
   三处风险：**载荷可能 zstd 压缩**、**历史受 30 天 + 10 GiB 驱逐**、
   **部分字段缺失**（无 reasoning；`cacheWrite1h` 在 worker 路径丢失）。
3. **若产品必须给出"每次调用"的 token**：Hermes 主库做不到，只能退到 best-effort 旁路——
   `logs/agent.log`（每次调用一行、含 cache 读写与 session 标签，但受 5 MB × 3 轮转）、
   `cron/usage_audit.jsonl`（每次 cron fire 一行，无缓存分桶）、`hermes -z --usage-file`
   （单次运行一个 JSON，需用户显式开启）。**这三者都不能当主数据源**。
4. **两家都需注意**：不要假设"逐请求"，也不要假设 `cost` 一定存在；把"未知/缺失"显式建模
   （与 TokenScope "未知不静默吞掉"的原则一致）。
5. **对 cc-switch 现状的解释（推测，未验证）**：它没做这两家，可能与粒度不匹配有关——
   cc-switch 整条链路建立在"一次请求一行"的假设上，而 Hermes 只到"会话 × 路由"聚合、
   OpenClaw 要解压缩载荷并面对保留策略驱逐；也可能纯粹是排期问题，本文无法判定。
   对 TokenScope 而言这不是阻断：统一事件模型本就能容纳"会话级聚合事件"，
   只要在展示与成本归属上说清粒度差异。

### 4.6 未验证项（不得外推）

- **两家都未在本机安装**，真实数据目录未实测（`~/.openclaw`、`%LOCALAPPDATA%\hermes` 均不存在），
  全部结论来自源码与官方文档。
- **OpenClaw**：权威数据面已确认是 per-agent SQLite 的 `transcript_events`（legacy 为 JSONL），
  `cacheWrite` / `cacheWrite1h` 均确认落盘（worker 投影路径例外）。剩余未验证：官方文档提到
  个别 harness（native Codex）可能按"回合内多响应求和"上报（`docs/reference/token-use.md:177-180`），
  即**部分记录可能是回合级而非单次调用级**；对象存储自动上传路径未穷尽核查。
- **Hermes**：保留策略与级联删除已由源码确认（默认 90 天 / `ON DELETE CASCADE`），但
  **WAL 只读窗口的真实失败率**、旧库中是否存在被"绝对模式"覆盖过的会话级累计、
  以及 schema 迁移在各历史版本上的兼容边界，均未验证。
- 两家的 **provider 配额/账单面**都需要凭据与网络，本文未做任何验证（也不属"本地可读"）。
- 未运行任何上游代码；未回放真实样本。

---
