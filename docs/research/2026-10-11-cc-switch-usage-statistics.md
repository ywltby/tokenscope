# cc-switch 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`farion1231/cc-switch`](https://github.com/farion1231/cc-switch)，
> main 分支 HEAD `df40d53`（2026-10-10，v4.0.8 发布节点）。浅克隆到临时目录后
> 逐文件精读，**未运行其二进制、未改动本机 `~/.cc-switch` 数据**。
>
> 证据分档：**已验证**（源码可证或与本文引用的行号一致）／**一致解释**（多条代码
> 证据指向同一结论，无单点直证）／**未验证推论**（结构推断，未实测）。
>
> 本文只记录字段名、表名、路径规则与计算口径，不含任何真实会话内容。

## 1. 范围与方法

- 代码规模（`src-tauri/src`）：`services/usage_stats.rs` 214 KB（5739 行）、
  `services/session_usage_codex.rs` 160 KB、`services/session_usage.rs` 89 KB、
  `services/session_usage_pi.rs` 60 KB、`services/session_usage_grokbuild.rs` 59 KB、
  `services/session_usage_opencode.rs` 31 KB、`services/session_usage_gemini.rs` 17 KB、
  `services/session_usage_mcode.rs` 9.5 KB、`proxy/usage/{parser,logger,calculator}.rs`
  合计 89 KB、`database/schema.rs` 152 KB。
- 分析手段：`rg` 定位符号 → 分段读原文 → 交叉核对调用点与 DDL；对关键断言附带
  `文件:行号`。
- 术语：**代理链路** = 内置本地代理实时记账；**会话链路** = 回扫各工具本地会话
  日志/SQLite 导入用量；**事实表** = `proxy_request_logs`。

## 2. 总体架构：两条采集链路，一张事实表

cc-switch 的用量统计不是"每个 agent 一个统计库"，而是**双链路汇入同一张明细表**，
再由查询层决定怎么合并。这是理解它全部行为的钥匙。

| 维度 | 代理链路（proxy） | 会话链路（session） |
| --- | --- | --- |
| 触发 | 请求经本地路由转发时旁路记账 | 定时/手动回扫各工具本地数据 |
| 覆盖 | 只有走了 cc-switch 本地代理的流量 | 各工具自己落的会话记录，**不开代理也统计** |
| 写入者 | `proxy/usage/logger.rs` | 7 个 `session_usage_*.rs` 适配器 |
| `data_source` | `'proxy'`（含 NULL 兜底） | `'session_log'` / `'codex_session'` / `'gemini_session'` / `'opencode_session'` / 等等 |
| `provider_id` | 真实供应商 id | 占位符：`_session`、`_codex_session`、`_gemini_session`、`_opencode_session`、`_grok_session`、`_mcode_session`、`_pi_session` |
| 计时精度 | 有首字延迟（`first_token_ms`） | 无首字，`latency_ms` 由日志时间戳估算 |
| 成本 | 由响应 usage × 价格表当场算 | 同一套 `CostCalculator`，另可采信客户端自带成本（OpenCode） |

**已验证**：两条链路写的是同一张 `proxy_request_logs`（`database/schema.rs:200-214`），
靠 `data_source` 列区分（`sql_helpers`/`usage_stats` 全程用
`COALESCE(data_source,'proxy')` 兜底历史 NULL 行，`usage_stats.rs:319-326`）。

查询层再把「明细表 + 日汇总表」相加（见 §6），并对「同一次请求既被代理记账、
又被会话日志扫到」的情况做跨源去重（见 §7）。

### 2.1 受管应用与占位供应商名

`provider_name_coalesce` 是"会话来源"到可读名的**权威映射**（`usage_stats.rs:303-315`）：

| provider_id 占位符 | 展示名 |
| --- | --- |
| `_session` | Claude (Session) |
| `_codex_session` | Codex (Session) |
| `_gemini_session` | Gemini (Session) |
| `_opencode_session` | OpenCode (Session) |
| `_grok_session` | Grok Build (Session) |
| `_mcode_session` | MiniMax Code (Session) |
| `_pi_session` | Pi (Session) |

官方文档口径（`docs/user-manual/zh/4-proxy/4.4-usage.md`）：OpenClaw、Hermes 不支持
用量统计；Claude Desktop 只统计经本地路由的"模型映射"请求，并折叠进 Claude Code
筛选项（代码对应 `folded_app_type_sql`，`usage_stats.rs:349-351`，仅用于读侧筛选与
分组，不改动任何已存储行）。

### 2.2 覆盖矩阵：每个 agent 靠哪条链路、读什么

**七家有独立会话适配器，覆盖七种数据面（4 种 JSONL 目录 + 2 种 SQLite + 1 种 JSON/JSONL
混合）**；全部同时受代理链路覆盖（前提是流量经本地路由）。

| 应用（`app_type`） | 会话链路 | 代理链路 | 会话数据面（§12 详述） |
| --- | --- | --- | --- |
| Claude Code（`claude`） | ✓ 独立适配器 | ✓ | `~/.claude/projects/**`：主会话 + `subagents/` + `subagents/workflows/wf_*/` 下的 `*.jsonl` |
| Claude Desktop（`claude-desktop`） | ✗ **无独立适配器** | ✓（只统计经路由的"模型映射"请求） | —（展示口径折叠进 `claude`，明细仍保留原始 `app_type`） |
| Codex（`codex`） | ✓ | ✓ | `~/.codex/{sessions,archived_sessions}/**/rollout-*.jsonl`（可 `.jsonl.zst`） |
| Gemini CLI（`gemini`） | ✓ | ✓ | `~/.gemini/tmp/<project_hash>/chats/session-*.json` 或 `.jsonl`（JSONL 先回放成单对象） |
| OpenCode（`opencode`） | ✓ | ✓ | SQLite `opencode.db`（V1 `session`+`message` / V2 `session_v2`+`session_message` 双布局） |
| Grok Build（`grokbuild`） | ✓ | ✓ | `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/updates.jsonl` |
| Pi（`pi`） | ✓ | ✓ | `~/.pi/agent/sessions/**/*.jsonl`（目录布局随配置可扁平化） |
| MiniMax Code（`mcode`） | ✓ | ✓ | SQLite `<data_dir>/v2/sqlite/runtime-state.sqlite` 的 `local_runtime_token_usage` 表 |
| OpenClaw、Hermes | ✗ | 视是否经路由 | 官方文档明确"暂不支持用量统计" |

**读取方式的一句话概括**（细节与行号见 §12）：

- **文件型（Claude / Codex / Gemini / Grok / Pi）**：发现候选文件 → 按 `session_log_sync`
  游标判断增量 → 逐行（或先回放）解析 → 取"单次值"或"累计快照差分" → 组 `request_id` →
  批量写 `proxy_request_logs`，游标与数据同事务提交。
- **SQLite 型（OpenCode / MiniMax Code）**：只读打开源库 → 按源表水位
  （OpenCode 用 `time_updated`，MiniMax 用自增 `id`）取新行 → 解析 JSON 列或直接读列 →
  同样的 `request_id` + 幂等写入。
- **增量钥匙**各不相同但都落在同一张 `session_log_sync` 表：Claude 是字节偏移 + 尾部指纹，
  Codex 是 mtime+size（每次从头重放），Gemini/Grok 是 mtime（变更即整文件重读），
  Pi 是 mtime+size+尾指纹+完整位编码进 `last_synced_at`，OpenCode 是库+WAL mtime 加会话级
  `time_updated`，MiniMax 是源表自增 id。

## 3. 数据库与迁移

### 3.1 库文件与连接

- 库文件 `cc-switch.db`（SQLite），会话链路、代理链路、价格表、游标全在里面。
- 取连接统一走 `lock_conn!(db.conn)` 宏（单连接 + 互斥锁），写操作在事务/SAVEPOINT
  内提交；没有任何连接池。
- 迁移用 `PRAGMA user_version` 单调递增，`SCHEMA_VERSION = 20`；整个迁移链包在一个
  `SAVEPOINT schema_migration` 里，失败回滚（`database/schema.rs:446-603`）。库比程序
  新时直接拒绝启动（`schema.rs:452-458`）。
- 迁移节点中与用量相关的：v8（引入 `data_source` + `session_log_sync`，开启"无代理模式
  统计"）、v9（清空并重刷全部模型定价）、v11（`usage_daily_rollups` 加 `request_model`
  维度进主键）、v13（记录输入 token 缓存语义）、v15→v16（清空 Codex 会话用量与游标以便
  启动重扫）、v17（`session_usage_dedup` 持久去重账本）、v18（Claude 字节游标与尾部指纹）

### 3.2 用量明细表 `proxy_request_logs`（DDL 原文，`schema.rs:200-214`）

```sql
CREATE TABLE IF NOT EXISTS proxy_request_logs (
    request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
    request_model TEXT,
    pricing_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    input_cost_usd TEXT NOT NULL DEFAULT '0', output_cost_usd TEXT NOT NULL DEFAULT '0',
    cache_read_cost_usd TEXT NOT NULL DEFAULT '0', cache_creation_cost_usd TEXT NOT NULL DEFAULT '0',
    total_cost_usd TEXT NOT NULL DEFAULT '0', latency_ms INTEGER NOT NULL, first_token_ms INTEGER,
    duration_ms INTEGER, status_code INTEGER NOT NULL, error_message TEXT, session_id TEXT,
    provider_type TEXT, is_streaming INTEGER NOT NULL DEFAULT 0,
    cost_multiplier TEXT NOT NULL DEFAULT '1.0', created_at INTEGER NOT NULL,
    data_source TEXT NOT NULL DEFAULT 'proxy'
)
```

要点（**已验证**）：

- **金额列是 TEXT 不是 REAL**：写侧用 `rust_decimal::Decimal` 精确算，落库存十进制字符串；
  查询侧才 `CAST(total_cost_usd AS REAL)` 求和（`usage_stats.rs:960`）。避免累加误差。
- `model` 与 `request_model` 分离：路由接管场景下，`request_model` 保留客户端请求的别名，
  `model` 是实际路由模型。
- `pricing_model` 记录**写入时实际用于计价的模型名**（NULL = v11 之前的旧行，`''` = 未计价
  的错误行），注释见 `schema.rs:197-199`。
- `input_token_semantics` 三态：`0`=LEGACY、`1`=TOTAL（含缓存读写）、`2`=FRESH（不含缓存），
  常量定义 `services/sql_helpers.rs:30-32`。
- 索引：`(provider_id, app_type)`、`created_at`、`model`、`session_id`、`status_code`、
  `(app_type, created_at DESC)`，以及一条**表达式索引**给跨源去重用
  （`schema.rs:3650-3657`，因为查询里写的是 `COALESCE(data_source,'proxy')`，普通列索引
  匹配不上）。

### 3.3 日汇总表 `usage_daily_rollups`（DDL 原文，`schema.rs:279-300`）

```sql
CREATE TABLE IF NOT EXISTS usage_daily_rollups (
    date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL, model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '', pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0, success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    input_token_semantics INTEGER NOT NULL DEFAULT 0, total_cost_usd TEXT NOT NULL DEFAULT '0',
    avg_latency_ms INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
)
```

注意它**没有** `session_id`、没有首字延迟、没有逐请求明细——所以「30 天前的会话查不到
明细」是设计使然（`usage_stats.rs:988-992` 的注释明说）。`request_model` / `pricing_model`
进主键是为了明细被剪掉后账单仍可审计（`schema.rs:276-278`、v11 迁移 `schema.rs:1364-1370`）。

### 3.4 同步游标 `session_log_sync`（`schema.rs:309-320`）

```sql
CREATE TABLE IF NOT EXISTS session_log_sync (
    file_path TEXT PRIMARY KEY,
    last_modified INTEGER NOT NULL,
    last_line_offset INTEGER NOT NULL DEFAULT 0,
    last_synced_at INTEGER NOT NULL,
    last_byte_offset INTEGER,
    last_tail_fingerprint INTEGER
)
```

一行为一个被跟踪对象：Claude/Gemini 是「文件绝对路径」，OpenCode 是「`<db 路径>` +
`<db 路径>:<session_id>`」两套键（会话级水位，见 §8.4）。

### 3.5 去重账本 `session_usage_dedup`（`schema.rs:324-340`）

```sql
CREATE TABLE IF NOT EXISTS session_usage_dedup (
    data_source TEXT NOT NULL, request_id TEXT NOT NULL, semantic_id TEXT NOT NULL,
    has_entry_id INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (data_source, request_id)
)
```

设计意图（注释原文）：明细行 30 天后会被汇总删除，**但 fork/rewrite 去重仍然需要这些
request_id**，所以另存一份精简账本。

## 4. 输入 token 语义归一（口径统一的关键）

不同厂商对 `input_tokens` 的定义不一致，cc-switch 用一张白名单 + 三态标记在**读侧**统一：

```rust
// services/sql_helpers.rs:23
pub(crate) const CACHE_INCLUSIVE_APP_TYPES: &[&str] = &["codex", "gemini", "grokbuild"];
```

`fresh_input_sql(alias)`（`sql_helpers.rs:43-67`）生成 SQL 标量表达式：

- `input_token_semantics = 2`（FRESH）→ 原样返回；
- `app_type ∈ {codex,gemini,grokbuild}` 且 `= 1`（TOTAL）→ 减 `cache_read + cache_creation`
  （带 `>=` 防御，减不成负数就退回原值）；
- 同白名单且 `= 0`（LEGACY）→ 只减 `cache_read`；
- 其他 → 原样。

白名单用**显式列表**而非默认值，注释解释了取舍方向（`sql_helpers.rs:9-22`）：新来源默认按
Claude 风格（input 不含缓存）更安全；漏加一个 OpenAI 风格来源会表现为"缓存命中率偏低"，
比反向默认导致的"静默重复扣减"更容易被发现。同一常量还被写入侧（计价）与回填侧引用，
前端 `src/types/usage.ts` 有对应副本，注释要求三处同步修改。

**已验证**：Anthropic 风格的 `input_tokens` 本身已不含 cache_read，因此 Claude 路径不做扣减
（`calculator.rs:82-89`）。

## 5. 金额计算：价格表 + 匹配链 + 公式

### 5.1 价格来源（三处，优先级由匹配链决定）

1. **内置种子表**：硬编码在 `schema.rs::seed_model_pricing`（`schema.rs:1633` 起）的 Rust
   数组，元素为 `(model_id, display_name, input, output, cache_read, cache_creation)`，
   单位 **美元 / 百万 token**，每次启动 `INSERT OR IGNORE` 增量补齐，并修复仍等于旧内置值的行
   （`ensure_model_pricing_seeded_on_conn`，`schema.rs:3590-3594`）。
2. **models.dev 公共数据**：`https://models.dev/api.json`，由**前端**拉取
   （`src/lib/modelsDev.ts:6`，1 小时 staleTime），转换后经
   `update_model_pricing_batch` 批量写入；自动同步按 6 小时节流
   （`src/lib/modelsDevAutoSync.ts:21`），可开关、可选择导入哪些模型。
3. **用户覆盖文件**：`~/.cc-switch/model-pricing.json`，结构
   `{version, modelsDevSync:{…}, models:[{modelId, displayName, inputCostPerMillion,
   outputCostPerMillion, cacheReadCostPerMillion, cacheCreationCostPerMillion}],
   deletedModelIds:[…]}`（`services/model_pricing.rs:13-93`），与 SQLite 表双向同步，
   手工改价/删价持久化在人类可编辑的 JSON 里。

models.dev 字段映射（`src/lib/modelsDevPricing.ts:207-221`）：`cost.input → inputCostPerMillion`、
`cost.output → outputCostPerMillion`、`cost.cache_read → cacheReadCostPerMillion`、
`cost.cache_write → cacheCreationCostPerMillion`，值直接是 $/1M（`formatPrice` 只做格式化，
`modelsDevPricing.ts:57-62`）。导入前会剔除非文本模型（embedding / audio / image / video /
moderation / realtime / tts / transcribe / deprecated 标记，`modelsDevPricing.ts:22-55`），
"常用模型"按族（claude/gpt/gemini/grok/deepseek/qwen/mimo/longcat/kimi/minimax/glm）
每族最多 6 条、按发布日期倒序取（`modelsDevPricing.ts:102-189`）。

### 5.2 模型名 → 价格条目的匹配链

`find_model_pricing_row`（`usage_stats.rs:2482-2506`）分两轮：**先全候选精确匹配，再前缀回退**。

候选生成 `model_pricing_candidates`（`usage_stats.rs:2586-2621`）是一个 BFS：

```
clean_model_id_for_pricing:
  取最后一个 '/' 之后 → 截断 ':' 之后 → trim → '@'→'-' → 全小写 → 去掉 [1m] 上下文标记
（usage_stats.rs:2623-2638）
随后反复应用 6 类剥离规则，每产出一个新串都入队继续剥离：
  strip_known_model_namespace           去掉 claude- 之前的前缀 / openai. anthropic. google.
                                        moonshot. moonshotai. bedrock. global. 等命名空间
  strip_claude_desktop_non_anthropic_prefix   claude- 之后紧跟非 Anthropic 厂商标记
                                        （gpt / gemini / deepseek / glm / kimi …）时剥掉 "claude-"
  strip_bedrock_model_version_suffix    去掉 -v1 / -v2 版本后缀
  strip_model_date_suffix              去 ISO 日期 -YYYY-MM-DD、8 位 -YYYYMMDD、
                                        6 位 -YYMMDD（6 位需月 01-12、日 01-31 才剥离）
  strip_reasoning_effort_suffix         去掉 -minimal/-low/-medium/-high/-xhigh
  点号兼容                              claude- 开头且含 '.' 时把 '.' 换成 '-'
```

占位符（`unknown` / `null` / `none` / 空串）直接判为无候选（`usage_stats.rs:2532-2535`）。

前缀回退**有条件**（`should_try_pricing_prefix_match`，`usage_stats.rs:2773-2801`）：
`claude-*` 需至少 3 个连字符；`o1/o3/o4/o5*` 需至少 1 个；`gpt-/gemini-/deepseek-/qwen-/
glm-/kimi-/minimax-` 家族需至少 2 个。回退时用 `model_id LIKE '<candidate>-%'` 并按
`LENGTH(model_id) ASC` 取最短命中（`usage_stats.rs:2560-2584`）。

### 5.3 成本公式（`proxy/usage/calculator.rs:72-120`）

```rust
let million = Decimal::from(1_000_000);
// OpenAI/Gemini 风格 input 含缓存，先扣；Claude 风格不扣
let billable_input_tokens = if input_includes_cache_read {
    usage.input_tokens.saturating_sub(usage.cache_read_tokens)
                        .saturating_sub(usage.cache_creation_tokens)
} else { usage.input_tokens };

let input_cost  = Decimal::from(billable_input_tokens) * pricing.input_cost_per_million / million;
let output_cost = Decimal::from(usage.output_tokens)    * pricing.output_cost_per_million / million;
let cache_read_cost = Decimal::from(usage.cache_read_tokens) * pricing.cache_read_cost_per_million / million;

// 定价表的 cache_creation 单价对应 5 分钟写入；1 小时写入按 1.6 倍计
let cache_creation_1h_tokens = usage.cache_creation_1h_tokens.min(usage.cache_creation_tokens);
let cache_creation_cost =
    (Decimal::from(usage.cache_creation_tokens - cache_creation_1h_tokens)
        + Decimal::from(cache_creation_1h_tokens) * Decimal::new(16, 1))   // ×1.6
    * pricing.cache_creation_cost_per_million / million;

let base_total = input_cost + output_cost + cache_read_cost + cache_creation_cost;
let total_cost = base_total * cost_multiplier;   // 倍率只作用于总价，分项保持基础价
```

**已验证**：

- 全链路 `rust_decimal::Decimal`，无浮点累加误差；`cost_multiplier` **只乘总价**，分项保持基础价
  （`calculator.rs:109-111` 与单测 `test_cost_multiplier`）。
- **代理写路径当前恒传 `Decimal::ONE`**（`logger.rs:401-406`，落库 `cost_multiplier` 为 `"1"`）：
  供应商级 `costMultiplier` 与全局 `default_cost_multiplier` 已停用
  （`services/provider.rs:507-513`，测试 `proxy/response_processor.rs:1232-1299`）；
  只有**回填路径**才会读历史行自己的 `cost_multiplier` 再相乘（`usage_stats.rs:2331-2372`）
  ——即该列现在是历史兼容列，不是当前的计费开关。
- 1 小时缓存写入分档：token 数来自 `usage.cache_creation.ephemeral_1h_input_tokens`
  （`proxy/usage/parser.rs:43-48`），**只在计价时使用，不落库**（明细表没有该列）。1.6 倍的来历是
  `2 / 1.25`（1 小时写入价 ÷ 5 分钟写入价）。
- 计价入口有两套：`calculate()`（不扣缓存，Claude 用）与 `calculate_for_app(app_type, …)`
  （按 §4 白名单决定是否扣缓存）。
- **没有任何汇率换算**：CNY→USD 只在录入内置表时按 ≈7.14 一次性折过（`schema.rs:2194,2230,2445`
  注释），运行时不换算；也**没有"估算值"标记列**，`*_cost_usd` 是纯 USD 十进制字符串。

### 5.4 未知模型：不拒算、写 0、可回填

- 查不到价格时**不拒绝写入**：token 照记、四项成本与总价写 `"0"`，并打
  `log::warn!("[USG-002] 模型定价未找到，成本将记录为 0")`（`logger.rs:397-399,103-120`）。
- 前端用 `isUnpricedUsage` 识别这一类行（2xx + 四类 token 非零 + 倍率≠0 + `totalCostUsd == 0`，
  `src/types/usage.ts:329-343`），显示灰色**「未定价」**而不是 "$0.00"（§13.7）。
- **补价回填**：凡是 `total_cost_usd <= 0` 且有 token 的行，会在每次价格表变动（seed / 覆盖文件
  重放 / models.dev 批量导入）后按写入时的 `pricing_model` 重算并 UPDATE
  （`usage_stats.rs:2256-2267`、`model_pricing.rs:309-313,415-427`）；
  **已非 0 成本的行永不被改写**——这保证"补价"不会覆盖用户手工改过的账。
- 内置表的播种与修复也遵循同一原则：`seed_model_pricing` 用 `INSERT OR IGNORE`（绝不覆盖已改价），
  `repair_current_model_pricing` 只修"当前值仍等于旧内置值"的行（`schema.rs:3590-3594,2849-3538`）。
  内置条目约 196 条元组（`schema.rs:1634-2843`）。
- **已知匹配失败面**（分析时实测样本）：形如 `gpt-6-astra-cc-format` 这类**库中没有对应后缀规则**
  的名字会落到 0 成本；`doubao-seed-1-6-250615` 这类 6 位日期尾巴因前缀门（非 `claude-` 且不在
  7 个家族名单里）被挡下，同样不命中。
- 一个代码可见的隐患：前缀回退用的是 `LIKE '{candidate}-%'` 且**没有 `ESCAPE`**，
  候选串里若含 `_` 或 `%` 会被 SQLite 当通配符（未构造用例验证）。

## 6. 聚合查询：明细 + 日汇总相加，再按完整自然日切分

`get_usage_summary`（`usage_stats.rs:863-986`）的骨架是**两个子查询相加**：

```sql
SELECT COALESCE(d.total_requests,0) + COALESCE(r.total_requests,0),
       COALESCE(d.total_cost,0)     + COALESCE(r.total_cost,0),
       ...
FROM (SELECT COUNT(*) …,
             SUM(<fresh_input_sql(l)>), SUM(l.output_tokens),
             SUM(l.cache_creation_tokens), SUM(l.cache_read_tokens),
             SUM(CASE WHEN status_code BETWEEN 200 AND 299 THEN 1 ELSE 0 END)
      FROM proxy_request_logs l …) d,
     (SELECT SUM(r.request_count), SUM(CAST(r.total_cost_usd AS REAL)),
             SUM(<fresh_input_sql(r)>), …
      FROM usage_daily_rollups r …) r
```

关键机制（**已验证**）：

- **只把"被完整覆盖的本地自然日"交给 rollup**。`compute_rollup_date_bounds`
  （`usage_stats.rs:785-824`）：起点不在 `00:00:00` 就从**次日**开始用汇总，终点不在 `23:59`
  就只到**前一日**；区间为空时直接 `1 = 0`。这样避免"半天走汇总、半天走明细"导致的漏算。
- **明细与汇总不重叠**：明细查询同样带时间条件，汇总只覆盖 `created_at < cutoff` 的部分。
- **吞吐保护**：`LogCountCache`（`usage_stats.rs:537-561`）在「筛选条件与起点未变 + 连接
  `total_changes()` 未变」时复用上次的 `COUNT(*)`，另有 60 秒 TTL 兜底，避免每次刷新都全表
  count。

## 7. 跨源去重：±10 分钟 + 四桶指纹

同一次请求可能同时被代理链路和会话链路记录，去重靠**指纹匹配**而非 id
（`SESSION_PROXY_DEDUP_WINDOW_SECONDS = 10 * 60`，`usage_stats.rs:317`）：

```rust
// effective_usage_log_filter（usage_stats.rs:399-440）
NOT (
  data_source IN ('session_log','codex_session','gemini_session','opencode_session')
  AND EXISTS (SELECT 1 FROM proxy_request_logs proxy_dedup
              WHERE COALESCE(proxy_dedup.data_source,'proxy') = 'proxy'
                AND app_type 匹配（claude ↔ claude-desktop 互相认）
                AND status_code BETWEEN 200 AND 299
                AND input_tokens / output_tokens / cache_read_tokens 三元组完全相等
                AND (cache_creation 相等 或 会话侧为 0 或 代理侧为 0 的兼容分支)
                AND created_at 落在 ±600 秒
                AND (LOWER(model) 相等 或任一侧为 'unknown')))
```

**语义**：会话行若能与某条成功的代理行对上，就**整行排除**（保留代理行，因为代理行有首字
延迟等更完整的计时）。写入侧也做同样的判定：插入前 `has_matching_proxy_usage_log` 命中则
跳过；已入库的会话行在后续被补全（output 增长）后若此时能对上代理行，直接
`DELETE` 该行（`session_usage.rs:1097-1104`）。

**性能取舍**（`effective_usage_log_filter_for_range`，`usage_stats.rs:456-531`）：先数窗口内
两侧行数——任一侧为 0 就不加条件（不可能重复）；代理行更少时改写成
`rowid NOT IN (一次性子查询)`；会话行更少时保留逐行 `EXISTS`。

## 8. 采集调度与刷新

- **定时**：应用启动后立即跑一轮（含费用回填），之后**每 60 秒**一轮
  （`lib.rs:1287-1337`，常量 `SESSION_SYNC_INTERVAL_SECS = 60`），由
  `tokio::time::interval` 驱动并 `Skip` 补偿错过的时间片；整轮串行，全程持有
  `session_sync_mutex`，具体解析丢进 `spawn_blocking`。
- **开关**：`session_auto_sync_enabled` 关闭后跳过定时扫描；但启动首轮的**费用回填仍会执行**
  （注释明说回填只修补数据库既有行，不读会话文件，`lib.rs:1292-1310`）。
- **手动**：`sync_session_usage` 命令（`commands/usage.rs:313-325`）走同一把锁；
  `rebuild_codex_usage`（`commands/usage.rs:346-361`）额外先备份数据库、重置 Codex 用量再重导。
- **前端实时刷新**：任何写日志路径都调 `usage_events::notify_log_recorded()`，它做
  **200 ms 防抖合并**后向前端 emit `usage-log-recorded` 事件（`src/usage_events.rs:19-71`），
  前端据此 invalidate 查询缓存，无需等轮询。
- **保留期**：启动时与定期维护都调用 `rollup_and_prune(30)`（`database/mod.rs:151-156`、
  `database/backup.rs:457`）。

## 9. 日汇总与明细清理（`database/dao/usage_rollup.rs`）

`rollup_and_prune(retain_days)` 三步（**已验证**）：

1. 算 cutoff：`now - retain_days` 所在日的**次日 00:00 本地时间**，用
   `Local.from_local_datetime` 处理 DST（歧义取最早、间隙回退 +1 小时），保证汇总边界永远落在
   自然日上（`usage_rollup.rs:20-56`）。
2. 剪枝前先 `backfill_missing_usage_costs_on_conn` 尽力回填一次成本——因为**剪枝不可逆**，
   0 成本行一旦汇总就永远失去按 `pricing_model` 补价的机会；回填失败只告警不阻断
   （`usage_rollup.rs:79-86`）。
3. `SAVEPOINT rollup_prune` 内执行聚合 + 删除（`usage_rollup.rs:116-180`）：

```sql
INSERT OR REPLACE INTO usage_daily_rollups (date, app_type, provider_id, model,
    request_model, pricing_model, request_count, success_count, input_tokens, output_tokens,
    cache_read_tokens, cache_creation_tokens, input_token_semantics, total_cost_usd, avg_latency_ms)
SELECT date(l.created_at,'unixepoch','localtime'), l.app_type, l.provider_id, l.model,
       COALESCE(l.request_model,''), COALESCE(l.pricing_model,''),
       COALESCE(old.request_count,0) + new_req, …,
       2,                              -- 归一为 FRESH 语义
       CAST(COALESCE(CAST(old.total_cost_usd AS REAL),0) + new_cost AS TEXT),
       CASE WHEN … THEN (old.avg_latency_ms*old.request_count + new_lat*new_req)
                        / (old.request_count + new_req) ELSE 0 END
FROM (SELECT … GROUP BY d,a,p,m,rm,pm FROM proxy_request_logs l
      WHERE l.created_at < ?1 AND <effective_usage_log_filter>) agg
LEFT JOIN usage_daily_rollups old ON old.date = agg.d AND … ;
DELETE FROM proxy_request_logs WHERE created_at < ?1;
```

要点：**汇总走 `INSERT OR REPLACE` + LEFT JOIN 旧汇总行**，所以可以重复执行而不会翻倍；
`input_token_semantics` 被统一写成 `2`（FRESH），即汇总表里的 input 永远是"新鲜输入"口径；
`avg_latency_ms` 是按请求数加权平均，不是对均值再平均。删除语句**故意不带去重过滤**——
重复的会话行就此一并清掉（`usage_rollup.rs:173-179`）。

## 10. 指标口径（口径即产品）

`UsageSummary`（`usage_stats.rs:22-37`）给出的每个指标都有明确定义：

| 指标 | 口径 | 代码 |
| --- | --- | --- |
| `realTotalTokens` | 新鲜输入 + 输出 + 缓存写入 + 缓存命中（"真实消耗"牌面数） | `derive_real_total_and_hit_rate`（`usage_stats.rs:49-63`） |
| `cacheHitRate` | `cache_read / (fresh_input + cache_creation + cache_read)`，0–1 小数，UI 再乘 100 | 同上 |
| `successRate` | 2xx 请求数 / 总请求数 × 100（会话行一律记 200） | `usage_summary_from_row`（`usage_stats.rs:67-100`） |

速度指标分**两套互不相加**的口径（`usage_stats.rs:129-174`）：

- 精确：`first_token_ms` 非空、`output_tokens ≥ 100`、`latency_ms - first_token_ms ≥ 100ms`，
  分子分母分别是输出 token 之和与生成窗口毫秒之和——**先求和再相除**，不是逐条平均；
- 估算：来自会话日志（无首字），要求 `output_tokens ≥ 200`、`latency_ms ≥ 1000ms`；
  门槛更高，因为估算耗时含首字等待，短请求会让速度偏低（注释给了实测比例）。

## 11. 时区

存储层 `created_at` 一律 **UTC 秒**（`session_usage.rs:1144-1157` 用
`DateTime::timestamp()`）；本地化只发生在两处：

- 日汇总分桶：SQL 里 `date(l.created_at,'unixepoch','localtime')`（`usage_rollup.rs:148`）；
- 范围边界换算：`compute_rollup_date_bounds` / `local_day_start_rfc3339`
  （`usage_stats.rs:785-859`），用 `chrono::Local` 并显式处理歧义与 DST 间隙。

（前端时区展示、图表粒度见 §14。）

## 12. 七个会话适配器的逐家实现

调度总入口 `sync_all_unlocked`（`services/session_usage.rs:137-173`）**串行**依次调用：
Claude → Codex → Gemini → OpenCode → Grok Build → Pi → MiniOS Code（mcode），单步失败只记录到
`errors`，不影响其余（`merge_sync_step`，`session_usage.rs:124-133`）。

**共性（已验证）**：没有统一 trait，也没有共享解析框架；共用的只有
`TokenUsage` 载体（`proxy/usage/parser.rs:72-86`）、`CostCalculator`、
`find_model_pricing`、`session_log_sync` 游标表与 `SessionSyncResult` 统计结构。
**七家全部绕过 `database/dao/*` 直接写内联 SQL**，且**没有一家采集 cwd / 项目路径**
（`proxy_request_logs` 里也没有该列）。

### 12.1 汇总对照表

| 适配器 | 数据面 | 格式 | 粒度与差分 | 身份键（`request_id`） | 幂等写法 | 语义标记 |
| --- | --- | --- | --- | --- | --- | --- |
| Claude Code | `<claude 配置目录>/projects/**`（固定 4 层） | JSONL | 逐条 assistant 消息 | `session:{message.id}` | `INSERT OR IGNORE` + 补全 UPDATE | 0（LEGACY，读侧不扣） |
| Codex | `<codex 配置目录>/{sessions,archived_sessions}` | rollout JSONL（`.zst` 可） | 逐请求；优先 last，缺失才差分 | `codex_session:thread-v1:{root_thread_id}:{event_index}` | `INSERT OR IGNORE` | 1（TOTAL，读侧扣） |
| Gemini CLI | `<gemini 目录>/tmp/<hash>/chats/session-*` | JSON / JSONL | 逐消息 | `gemini_session:{sessionId}:{messageId}` | `UPSERT`（值变才更新） | 0 + 白名单命中 |
| OpenCode | `opencode.db`（SQLite，V1/V2 双布局） | SQLite | 逐消息；自带 `cost` 优先 | `opencode_session:{sessionId}:{messageId}` | `INSERT OR IGNORE` | 0（input 是 fresh） |
| Grok Build | `~/.grok/{sessions,archived_sessions}/<enc-cwd>/<sid>/updates.jsonl` | JSONL | **逐轮独立总量，禁止差分** | `grok_session:{session_id}:{prompt_id|idxN}:{model}` | `UPSERT` + 接管守卫 | 1（TOTAL，读侧扣） |
| Pi | `~/.pi/agent/sessions`（可覆盖） | JSONL | 逐记录（4 类载体） | `pi_session:{sha256(kind,entry_id,timestamp)}` | 持久账本 `session_usage_dedup` | 2（FRESH） |
| MiniMax Code | `<data_dir>/v2/sqlite/runtime-state.sqlite` | SQLite（只读） | 逐请求 | `mcode:{session_id}:{id}` | `INSERT OR IGNORE` + id 游标 | 2（FRESH） |

### 12.2 Claude Code（`session_usage.rs`，2228 行）

- **发现**：`get_claude_config_dir()/projects`，目录不存在直接返回空结果；扫描固定 4 层（不递归，
  只为避免死循环）：`项目/*.jsonl`、`项目/<session>/subagents/*.jsonl`、
  `项目/<session>/subagents/workflows/wf_*/*.jsonl`（`session_usage.rs:441-486`）。
  配置目录来自设置项 `claude_config_dir`（支持 `~` 展开），否则 `~/.claude`
  （`config.rs:91-97`）。**没有环境变量入口**（`CC_SWITCH_TEST_HOME` 只影响 home 且是测试用途）。
  扩展名按字面等于 `jsonl` 比较，无大小/时间/文件名过滤，唯一门是 mtime。
- **解析**：逐行 `serde_json::Value`（无 serde 结构体）。要计用量必须同时满足
  `type=="assistant"`、`message` 存在、`message.id` 存在、`message.usage` 存在，且
  四桶任一 > 0（写库 gate，注释解释了为什么放宽旧版"必须有 stop_reason 且 output>0"：
  Workflow/子 agent 的并行短请求常只写 `message_start` 快照，旧逻辑会整条丢弃，实测系统性
  低估约 4.1%，`session_usage.rs:822-841`）。
- **文件内合并**：按 `message.id` 择优——有 `stop_reason` 的优先，否则取 `output_tokens` 更大者
  （`session_usage.rs:790-809`）。
- **token**：`input_tokens` / `output_tokens` / `cache_read_input_tokens` /
  `cache_creation_input_tokens` 直取；1 小时缓存写入分档取自
  `usage.cache_creation.ephemeral_1h_input_tokens`（`parser.rs:43-48`），**只用于计价、不落库**。
- **耗时估算**：会话日志没有计时，用对话链回溯取请求起点——每行记入
  `ChainNodes{uuid,parentUuid,timestamp,is_attachment}`，回复完整（有 `stop_reason`）且能看到首块
  时，沿 `first_parent` 回溯最多 32 跳、跳过 `attachment` 行，取到起点后做差；
  落在 100 ms–1 h 之外写 0（`session_usage.rs:205-220,328-364,849-857`）。
- **增量**：`session_log_sync` 一行/文件，`last_modified` 存 **纳秒 mtime**；字节游标
  `last_byte_offset` + 4096 字节**尾部指纹**（域标签 `claude-session-tail-v1`）。
  文件被截断或尾部重写 → 游标**钉到当前 EOF、旧区间一概不重放**，并把原因上报给用户
  （`session_usage.rs:611-654`）；旧行号游标会先做一次"只数不解析"的字节位置换算
  （`session_usage.rs:670-688`）。整轮解析与游标推进在**同一事务**提交，避免"数据进库、
  游标没进"或反之（`session_usage.rs:812-889`）。
- **补全**：子 agent 日志逐块写入，若同步落在写到一半时，会按中间值入库；后续轮次发现
  `output_grew` 就 UPDATE 补全 token/成本/耗时；**若补全后能与某条代理行对上指纹，说明这条
  本来就是代理记录的请求，直接 DELETE 掉自己的会话行**（`session_usage.rs:1069-1131`）。
- **未处理**：`isSidechain`、`isMeta`、`<synthetic>`、`requestId`、`cwd`、`gitBranch`、
  `userType` 一律不读（`<synthetic>` 的过滤在另一个模块
  `session_manager/providers/claude.rs:1220`，与会话用量无关）。坏行静默跳过且**不计数**。

### 12.3 Codex（`session_usage_codex.rs`，4253 行）

- **发现**：`<codex 配置目录>/sessions` 递归深度 3（恰好覆盖 `YYYY/MM/DD`）+ `archived_sessions`
  扁平一层；文件名须 `rollout-` 前缀、`.jsonl` 后缀、尾部 36 字符为 UUID；`.jsonl.zst`
  压缩形态透明解压，同名并存时只留 `.jsonl`（`session_usage_codex.rs:956-1011`、
  `codex_rollout_file.rs:36-38,64-76`）。**明确不用 `state_5.sqlite`**（文件头注释：它是被替代的
  旧估算方案；state DB 现在只服务历史迁移与标题）。
- **事件模型**：同样无 serde 结构体，先用 `line.contains("\"event_msg\"")` 之类的字符串快筛，
  再整行解析按 `type` 分支（`session_usage_codex.rs:1083-1101`）。真正产用量只有
  `event_msg` + `payload.type == "token_count"`；`session_meta` 只认首条（取线程 id、
  `forked_from_id`、子代理的 `parent_thread_id`）；`turn_context` 只更新模型。
- **token 语义（本适配器最精巧的部分）**：优先用 Codex 已经算好的
  `payload.info.last_token_usage`（单次精确值）；**只有它缺失才**用
  `total_token_usage` 与 high-water 快照相减，high-water 逐字段取 max。
  再做钳制：`cached_input.min(input)`、`cache_write.min(input - cached_input)`
  （`session_usage_codex.rs:1193-1238`）。
- **轮内重复快照防双计**：若存在可用的 total 快照，且（同一 `rate_limits.limit_id` 来源的签名
  相同 **或** 与前一条 token 事件签名相同）→ 本次 delta 直接归零；`event_index` 只在非零 delta
  时自增，因此 `request_id` 序列稳定（`session_usage_codex.rs:1193-1245`）。
- **模型归一化四步**：小写 → 剥最后一个 `/` 之前 → 剥 `-YYYY-MM-DD` → 剥 `-YYYYMMDD`
  （`session_usage_codex.rs:793-830`）。reasoning effort 不参与身份。
- **增量**：mtime+size 短路 → `line_offset` 过滤；注意 `last_byte_offset` **只用于变更检测，
  不做 seek，解析永远从头重放**（`session_usage_codex.rs:419-422,1019-1029`）。父 rollout 的
  token 时间线在进程内缓存（`CodexReplayCaches`，按 `(mtime,size)` 失效），fork 密集的历史
  靠它避免重复解析。
- **写入**：25 列（比 Claude 多一个显式的 `input_token_semantics=1`），批大小 1000，
  最后一批与游标同事务提交（`session_usage_codex.rs:1451,1827-1865,1693-1699`）。
  `reset_codex_usage` + 手动命令 `rebuild_codex_usage` 提供"备份→清理→重导"的重建路径。
- **配额不进用量表**：rollout 里的 `rate_limits` **只取 `limit_id` 当内存去重标签**；真正的配额
  查询走网络：`GET https://chatgpt.com/backend-api/wham/usage`（`services/subscription.rs:955-1054`），
  结果进前端 `SubscriptionQuota`，不落用量表。

### 12.4 Gemini CLI（`session_usage_gemini.rs`，508 行）

- **发现**：`<gemini 目录>/tmp/<project_hash>/chats/session-*`；扩展名仲裁规则：`.jsonl`
  恒收，`.json` 仅当同名 `.jsonl` 不存在时收（`session_manager/providers/gemini.rs:67-73`）。
- **JSONL 不是逐行独立记录**：先"回放"成旧版单对象 `{sessionId,…,messages:[…]}`，识别
  `$rewindTo`（回退）、带 `id` 的记录（upsert）、`$set`（检查点）三类特殊行
  （`providers/gemini.rs:93-115`）。
- **token**：`tokens.{input,output,cached,thoughts}`；**`thoughts` 并入 output 计费**，
  `tool`/`total` 字段不读；`cache_creation` 恒 0（Gemini 不暴露缓存写）。
  `gemini` 在 `CACHE_INCLUSIVE_APP_TYPES` 里，所以 input 含缓存读、计价前要扣
  （`session_usage_gemini.rs:230-237,263-264`）。
- **身份与幂等**：`gemini_session:{sessionId}:{messageId}` 主键 + UPSERT（token/model 变化才更新）。
- **增量**：只有 mtime（ns），一变就整文件重读（`session_usage_gemini.rs:143-151`）。
- **兜底**：`id`/`model` 缺失写 `"unknown"`（同会话多条缺失会共用同一 request_id 互相覆盖，
  是已知风险点）；`tokens` 全 0 跳过；`messages` 非数组时不推进游标。

### 12.5 OpenCode（`session_usage_opencode.rs`，857 行）

- **发现**：SQLite `opencode.db`，路径优先级 `$OPENCODE_DB` > `$XDG_DATA_HOME/opencode` >
  `~/.local/share/opencode`（`opencode_config.rs:75-105`）。**双布局自适应**：
  同时存在 `session_v2` + `session_message` 走 V2，否则 V1 `session` + `message`。
  **WAL 参与新鲜度判定**（取 db 与 `db-wal` 的 mtime 最大值）——这是"刚写完的会话不被跳过"的
  关键（`session_usage_opencode.rs:98-111`）。
- **两级水位**：文件键 `<db_path>`（mtime）+ 会话键 `<db_path>:<session_id>`
  （`MAX(session.time_updated, MAX(message.time_updated))`）；会话未见更新即整段跳过。
- **token 与成本**：`tokens.{input,output,reasoning,cache.{read,write}}`，`output + reasoning`
  合并；**客户端自带 `cost` > 0 时直接写 `total_cost_usd`（四项分项写 0）**，为 0 才回落本地
  价格表重算（`session_usage_opencode.rs:439-486`）。`opencode` **不在** 缓存包含白名单，
  其 `input` 本就是新鲜输入。
- **必须拦未完成消息**：因为插入用 `INSERT OR IGNORE`（已存在行永不更新），半截 token 无法事后
  回填——所以用 `time.completed` 是否存在（V2 的 compaction 用 `status ∈ {completed,failed}`）
  判定终态，未完成即跳过且**不推进该会话水位**（`session_usage_opencode.rs:186-202,313-327`）。
- **不读** `storage/message|part` 目录树与 `part` 表——那是会话浏览模块的事
  （`session_manager/providers/opencode.rs:19-37`）。

### 12.6 Grok Build（`session_usage_grokbuild.rs`，1423 行）

- **发现**：`~/.grok/{sessions,archived_sessions}/<enc-cwd>/<session-id>/updates.jsonl`，
  递归深度上限 16，**无条件跳过符号链接**，单文件 50 MiB 上限（超限静默跳过 + warn）
  （`session_usage_grokbuild.rs:139-207`）。
- **口径是本仓库里被反复强调的一条**（文件头大段警告 + 两个回归测试）：`turn_completed` 事件里的
  usage 是**该轮 user prompt 的独立总量**（轮内跨 inference loop 累加），不是进程/会话累计，
  **禁止做相邻事件差分**——曾把每轮总量当累计快照做差，导致巨量漏记
  （`session_usage_grokbuild.rs:12-20,779-842`）。相邻轮数值变小是常态。
- **字段**：`usage.modelUsage[<model>].{inputTokens, outputTokens, cachedReadTokens,
  apiDurationMs, modelCalls, costUsdTicks, costIsPartial}`；`reasoningTokens ⊂ outputTokens`
  不参与计费；`costUsdTicks` 1 tick = 1e-10 USD。`inputTokens` 含缓存读 → 写 TOTAL 语义。
- **沉降窗**：距今不足 60 秒的事件延后处理并 `break`，且**不写同步状态**，下一轮整文件重读
  （避免把正在写入的一轮记成中间态）。
- **接管守卫**：事件时刻 ±10 分钟内存在 `app_type='grokbuild' AND data_source='proxy'` 的行
  → 判为"代理接管态"，整段跳过（`usage_stats.rs:708-728`，方向保守：只漏不双）。
- **成本三档**：自报且非 partial → 总额用自报、分项用本地价（偏差 >1% 告警）；partial 且本地有价
  → 本地全额复算；彻底无价 → 自报下界或 0（`session_usage_grokbuild.rs:445-515`）。
- **身份与幂等**：`grok_session:{session_id}:{prompt_id|idxN}:{model}` + UPSERT（带
  `data_source='grok_session'` 与"值有变"双重 WHERE 守卫）。

### 12.7 Pi（`session_usage_pi.rs`，1538 行）

- **发现**：`~/.pi/agent/sessions`，目录布局由配置决定（默认两级
  `sessions/<项目目录>/<会话>.jsonl`，被覆盖后变扁平），根可由设置或
  `PI_CODING_AGENT_DIR` / `PI_CODING_AGENT_SESSION_DIR` 覆盖
  （`pi_config/mod.rs:44-48`、`session_manager/providers/pi.rs:124-177`）。
  显式上限：单文件 128 MiB、50 万 entry，超限**报错而不静默**。
- **四类用量载体**：`message(role=assistant)`、`message(role=toolResult)`、`compaction`、
  `branch_summary`，各自带独立 usage（`session_usage_pi.rs:460-473`）。
- **字段**：`usage.{input,output,cacheRead,cacheWrite}` + `usage.cost.{input,output,cacheRead,
  cacheWrite,total}`；自报成本优先，全零才回退本地定价；`input` 为 fresh → 写 FRESH 语义。
- **最强的增量设计**：文件 revision 四元组（mtime ns + size + 尾 4096 字节 SHA256 前 4 字节指纹 +
  "尾字节是否换行"的 complete 位）**位编码塞进 `session_log_sync.last_synced_at`**；旧 revision
  complete 且新 size 更大且旧边界指纹匹配 → 从旧字节位置续读，否则从 0 全量重扫，靠持久账本兜底
  （`session_usage_pi.rs:30-35,110-117,202-219,286-338`）。
- **唯一写 `session_usage_dedup` 的适配器**：记录级 `request_id = sha256("pi-session-request-v3"
  + kind + entry.id + timestamp)`，并配三态语义哈希去重（含 v1→v3 迁移的 legacy 分支）
  （`session_usage_pi.rs:36-47,651-754,769-795`）。
- **会话 header 校验**：首条有效 JSON 必须是 `{"type":"session","id":…}`，id 需过
  `is_valid_tree_id`（ASCII 字母数字加 `-_.`，1–256 字节），否则整个文件报错。

### 12.8 MiniMax Code（`session_usage_mcode.rs`，206 行）

- **发现**：SQLite `<data_dir>/v2/sqlite/runtime-state.sqlite`，data_dir 默认 `~/.minimax`
  （可被 `MINIMAX_DATA_DIR`、旧名 `MAVIS_DATA_DIR` 覆盖），**只读打开**
  （`mcode_config.rs:12-27`、`session_manager/providers/mcode.rs:9-15`）。
- **直接读产品自己提交的用量投影表** `local_runtime_token_usage`：列
  `input_tokens, output_tokens, reasoning_tokens, cache_read_tokens, cache_write_tokens,
  cost_usd, ts, model, session_id`；`reasoning_tokens` 并入 output；`input` 为 fresh。
- **最省事的增量**：游标就是源表自增主键 `id`（存在 `last_line_offset`，键 `mcode:<sqlite 路径>`），
  查询 `WHERE id > ?1 ORDER BY id`；`INSERT OR IGNORE` 幂等；**游标推进与插入同事务提交**，
  注释说明"被剪枝的日志永不再导入"（`session_usage_mcode.rs:16-37,68-92`）。
- **模型名**：`native_model.split_once('/')` 取第一个 `/` 之后，剥掉
  `custom_provider:router/…` 这类渠道命名空间（有专门测试钉死语义）。

### 12.9 各适配器的"不做什么"（对 TokenScope 最有用）

- **都不采集项目/cwd**：`proxy_request_logs` 无此列，7 家解析器也都不读 cwd（Grok 用
  `<enc-cwd>` 只当目录名，不入库；Gemini `.project_root` 只服务会话浏览）。要做项目维度只能另起
  数据面。
- **都不处理 sidechain / isMeta / synthetic**：Claude 侧完全不看 `isSidechain`；只有
  `attachment` 行在耗时估算里被跳过。
- **坏行都不计数**：`SessionSyncResult` 只有 `imported/skipped/files_scanned/
  suspected_duplicates/deferred_files/errors`，没有 bad_lines（对比 TokenScope 的受检算术）。
- **大多数不做模型名归一**：Grok 用 `modelUsage` 的 key 原样、Pi 只截断长度、MiniMax 只剥渠道
  前缀；归一化集中在**查价层**（§5.2），不在适配器里。

## 13. 前端契约与界面

### 13.1 Tauri 命令面（23 个，`src-tauri/src/commands/usage.rs` + `lib.rs:1642-1663` 注册）

| 命令 | 参数 | 返回 |
| --- | --- | --- |
| `get_usage_summary` | startDate?, endDate?, appType?, providerName?, model? | `UsageSummary` |
| `get_session_usage_summary` | appType, sessionId | `UsageSummary` |
| `get_usage_summary_by_app` | startDate?, endDate?, providerName?, model? | `UsageSummaryByApp[]` |
| `get_usage_trends`（内部方法名 `get_daily_trends`） | 同 5 参 | `DailyStats[]` |
| `get_usage_first_date` | appType?, providerName?, model? | `string｜null` |
| `get_provider_stats` / `get_model_stats` | 同 5 参 | `ProviderStats[]` / `ModelStats[]` |
| `get_request_logs` | filters, page=0, pageSize=20 | `PaginatedLogs` |
| `get_request_detail` | requestId | `RequestLog｜null` |
| `get_model_pricing` / `update_model_pricing` / `update_model_pricing_batch` / `delete_model_pricing` | — | 定价 CRUD |
| `get_models_dev_sync_config` / `save_models_dev_sync_config` / `record_models_dev_sync_result` | — | models.dev 同步状态 |
| `check_provider_limits` | providerId, appType | `ProviderLimitStatus` |
| `sync_session_usage` / `get_session_usage_last_sync` / `rebuild_codex_usage` | — | 同步与重建 |
| `get_usage_data_sources` | — | `DataSourceSummary[]` |

**已验证**：所有查询命令都走 `run_db_query`（`commands/usage.rs:11-20`）——内部用
`tauri::async_runtime::spawn_blocking` 包住同步 DB 查询，注释明说"同步命令在 Tauri 2 里跑在
主线程上，大范围聚合会让整个窗口卡住"。写命令（定价 CRUD）则是普通同步命令。
`QueryProviderUsage` / `testUsageScript` 属**供应商额度**功能（§17），不是本地面板统计。

### 13.2 类型契约（`src/types/usage.ts` 与 Rust 结构体一一对应）

```ts
export interface UsageSummary {
  totalRequests: number; totalCost: string;
  totalInputTokens: number; totalOutputTokens: number;
  totalCacheCreationTokens: number; totalCacheReadTokens: number;
  successRate: number;
  realTotalTokens: number;   // input + output + cache_creation + cache_read（已归一）
  cacheHitRate: number;      // cache_read / (input + cache_creation + cache_read)，0–1
}
```

- 成本一律是**字符串**（十进制），前端只格式化不重算；`cacheHitRate` 是 0–1 小数，UI 乘 100
  显示（`usage_stats.rs:34-36` 注释与 TS 注释一致）。
- `RequestLog` 暴露 `requestModel` / `pricingModel` / `costMultiplier` / `dataSource`，
  即"请求模型 → 实际模型"的接管映射在明细里是可见的（README 亦以此宣传）。
- `input_token_semantics` 标了 `#[serde(skip)]`（`RequestLogDetail`，`usage_stats.rs:233-235`），
  **不暴露给前端**——语义只在后端归一，前端不必理解。

### 13.3 交互与筛选

- 预设：`today / 1d / 7d / 14d / 30d / all / custom`，默认 `today`；语义在
  `src/lib/usageRange.ts:26-63`（today = 本机当日零点→now，1d = now−24h，7/14/30d = 含今天的
  回看，all = `startDate 0`）；custom 支持 `liveEndTime`（结束时间跟随当前时刻）。
- 筛选维度：应用 chips + 供应商下拉 + 模型级联下拉（切换供应商清空模型）+ 状态码（仅日志页签，
  固定 `[200,400,401,429,500]`）。**没有项目/目录维度**——前端 `LogFilters` 与 Rust
  `LogFilters` 都没有 project/cwd 字段，全仓 grep `project|cwd` 零命中。
- 分页：请求日志走服务端 `LIMIT/OFFSET`（`usage_stats.rs:2052-2068`），每页固定 20；
  供应商/模型/定价表是前端分页。**没有导出**（全前端无 CSV/下载代码）。
- 时区：前端全用本机时区，后端 `chrono::Local` + SQLite `'localtime'`，**没有时区设置项**
  （靠"同机同进程"这一前提；跨时区 WebView 未验证）。

### 13.4 图表

- 库是 **recharts ^3.5.1**（不是 echarts）。
- 趋势图是**单指标柱状图**（tokens / 请求数 / 成本三选一），X 轴用后端给的桶时间戳字符串
  （源码注释明确禁止改用显示文字，避免重排/重复键）。
- 粒度由时间跨度决定：≤24h → 1 小时桶；>24h → 本地自然日桶；空桶补 0 行
  （`usage_stats.rs:1212-1213,1308-1336`）。
- 「全部」预设不用柱状图，改用**自绘 53 周 × 7 天热力图**（按非零天四分位分 4 档色阶），
  可继续往前展开历史周段（`UsageHeatmap.tsx:18-21,121-144,259-279`）。

### 13.5 多应用呈现

- 应用切换是**筛选行里的图标 chips**（`all` + claude/codex/gemini/grokbuild/opencode/pi/mcode），
  名字只在 hover/aria-label 里。
- **没有按 agent 的颜色映射**：图表只有单一色 `--chart-1`，热力图只按数值深浅。
- 品牌名硬编码不翻译（`AppGlyph.tsx:6-18` 注释），Claude Code 与 Claude Desktop 共用图标靠
  角标区分；后端把 `claude-desktop` 在展示口径折叠进 `claude`（§2.1）。

### 13.6 刷新机制（三条并存）

1. **轮询**：默认 30 秒（选项 0/5/10/30/60 秒，持久化到 settings 的
   `usageDashboardRefreshIntervalMs`），透传给各 `useQuery.refetchInterval`。
2. **事件**：后端写新行 → `usage-log-recorded`（200 ms 防抖）→ 前端立即 invalidate 整个
   usage 命名空间（`useUsageEventBridge.ts:23-27`）；另有 `usage-cache-updated` 只管供应商额度。
3. **后端扫描**：启动一次 + 每 60 秒（§8），页头显示"自动扫描 / 刚刚同步 / N 分钟前同步 / 已关闭"。

**扫描进度反馈只有按钮转圈**——没有进度条、没有百分比、没有进度事件；手动同步/重建期间按钮
disabled。

### 13.7 空态与错误处理（值得借鉴与警惕的两点）

- 全局空态判据是**全时间范围** `allTimeSummary.totalRequests === 0`，此时隐藏指标与图表，
  给"还没有用量数据 + 立即同步 + 配置模型定价"的引导；表格级空态是"暂无数据"。
- 未知成本：若一条 2xx 记录四类 token 非零、倍率非 0 而 `totalCostUsd` 为 0，前端显示
  **「未定价」**（`isUnpricedUsage`，`types/usage.ts:329-343`）——这与 TokenScope "不得按 0
  静默吞掉"的原则一致。
- 缓存写入不可用（codex/gemini/grokbuild 不报缓存写）时显示 `N/A` 加说明；速度不可算显示 `—`
  加门槛说明。
- **后端报错只在少数组件有 error 分支**（详情抽屉、定价面板、models.dev 面板、同步/重建 toast）；
  Hero、趋势图、请求日志表、供应商表、模型表只解构 `data/isLoading`，**拿不到错误态**——
  后端失败会呈现为 `-- / 0 / 暂无数据`，与"真的没有用量"无法区分。这是可以直接避开的坑。

## 14. 代理链路：一次请求如何被记账

### 14.1 代理本体

- 默认监听 **`127.0.0.1:15721`**（`proxy/types.rs:42-56`，`listen_port: 15721` 注释"使用较少占用的
  高位端口"；配 0 时启动后把真实端口回写全局配置）；`enable_logging` 默认 **true**。
- 路由覆盖：`/v1/messages`、`/claude/v1/messages`、`/claude-desktop/v1/messages`、
  `/v1/chat/completions`、`/v1/responses`（含 `/codex/v1/` 前缀；GET 是 WebSocket 握手，回 426
  让 Codex 改走 HTTP）、`/v1/responses/compact`、`/v1/alpha/search`、`/v1/images/*`、
  Gemini `/v1beta/*path` 与 `/gemini/v1beta/*`、`/grokbuild/v1/responses`；请求体上限 200 MB
  （`proxy/server.rs:299-425`）。
- **接管方式是改写客户端 Live 配置里的 `base_url` + 占位 token，不是设置系统代理**
  （`mode/controller.rs:332-380`；占位符 `PROXY_TOKEN_PLACEHOLDER = "PROXY_MANAGED"`，
  `live/project/claude.rs:199`，用它确定性识别"是否已接管"）。上游是否转换格式由 adapter 决定
  （`handlers.rs:367-387`）。`proxy/http_client.rs:249-266` 里的系统代理检测只为**防止上游出站
  经本机系统代理自环**，与接管无关。

### 14.2 一次请求的生命周期（调用点行号）

1. 请求进入 `proxy/handlers.rs`，记 `start_time`。
2. **非流式**：直接读响应体 → `TokenUsage::from_*_response` → `log_usage_internal`
   （调用点 `handlers.rs:451`（Claude）、`1596/1828/1993`（Codex/Gemini 等））。
3. **流式**：`SseUsageCollector` 边转发边累积 SSE 事件，并在**首个"非元数据"事件**上打
   `first_output_time`（口径注释写明与 Sub2API 的 semantic TTFT 一致：跳过只宣告开始或保活的
   事件，`Chat Completions`/`Gemini` 的分块无事件类型一律算，`response_processor.rs:405-454`）；
   流结束时 `finish()` 取出事件数组与 `first_token_ms`，再调 `log_usage_internal`
   （`response_processor.rs:420-437,536-624`）。
4. `log_usage_internal`（`response_processor.rs:657-728`）做三件事：算 `request_id`
   （`dedup_scope_for_app`：**`claude` / `claude-desktop` 不加作用域**，以便与 Claude Code
   JSONL 的 `session:{message_id}` 收敛；其他应用加 `app_type:provider_id:` 作用域避免不同上游
   复用 envelope id 互相覆盖，`parser.rs:56-61,92-102`）；解析计价锚点模型
   （`pricing_model_source = 'request'` 时用 `outbound_model`——路由映射后真正发出去的模型，
   默认 `'response'` 用上游回显的 model）；然后丢进 `spawn_blocking`
   （注释 #7818：写库要拿单把 `Mutex<Connection>` 并做磁盘 IO，同步执行会卡住 tokio worker）。
5. `UsageLogger::log_with_calculation`（`logger.rs:390-427`）：查价 →
   `CostCalculator::try_calculate_for_app(app_type, usage, pricing, Decimal::ONE)` →
   组 `RequestLog` → `log_request` 落库；查不到价且有 usage 且模型名不是占位符时打
   `[USG-002]` 警告（§5.4）。
6. `log_request`（`logger.rs:100-222`）：按 `data_source` 与语义指纹处理主键冲突，然后
   `INSERT OR IGNORE`（或被判为覆盖会话行时 `INSERT OR REPLACE`）。

### 14.3 主键冲突的四种结局（`logger.rs:130-177`）

| 库中已有同 `request_id` 行 | 结局 |
| --- | --- |
| 无 | 直接插入 |
| `data_source = 'session_log'`（会话链路先写入的 Claude 行） | 用 `INSERT OR REPLACE` **覆盖**——代理行信息更全（测试 `logger.rs:618-668`） |
| `data_source = 'proxy'` 且语义指纹相同 | **幂等返回**（不写、不报错），避免重放双计 |
| 其他（语义不同） | 主键改写成确定性 `{request_id}:collision:{sha256(语义)}`；若连它也撞且语义相同则幂等返回，否则报"SHA-256 冲突"错误 |

**语义指纹**（`UsageSemantic` 的 SHA-256）是一个 **9 元组**：app_type、provider_id、model、
语义位（TOTAL/FRESH）、四桶 token、status（`logger.rs:41-58`）。它是"同一请求被重复记录"与
"不同请求恰好复用 id"的判据——也是代理链路能在重试/重放场景下保持幂等的关键。
Claude 系两侧**共用 `session:{message_id}` 主键**，冲突由代理侧 `INSERT OR REPLACE` 接管会话行
（测试断言基数为 1 且 `data_source` 变为 `proxy`）；反向不成立，代理行只会被同语义幂等吞掉，
不会被会话侧覆盖（测试 `logger.rs:569-668`）。

### 14.4 各协议的 usage 抽取要点（`proxy/usage/parser.rs`）

- **Claude**：非流式读 `usage` 对象；流式用 `message_start` 取 input / cache_read /
  cache_creation（含 `cache_creation.ephemeral_1h_input_tokens`）与 `message_delta` 取 output，
  若 delta 带回"更小的正 input"则用它修正 input 并同步缓存计数，最后过 `has_billable_tokens` 门
  （纯 cache-read 请求**保留**，不会被当空 usage 丢掉）。
- **OpenAI 兼容**：非流式读 `prompt_tokens/completion_tokens`；流式**倒序**找最后一个非空
  `usage` 分块。
- **Codex**：先找 `response.completed.response`，再倒序找顶层 `usage.input_tokens` 事件，
  最后回退 OpenAI 格式。
- **Gemini**：`usageMetadata`，`output = totalTokenCount - promptTokenCount`（含 thoughts），
  `cache_read = cachedContentTokenCount`；流式逐块覆盖取末值。
- **缓存字段的多源探测**（因为中转站字段五花八门，`parser.rs:12-40`）：
  read 链依次试 `cache_read_input_tokens` → `input_tokens_details.cached_tokens` →
  `prompt_tokens_details.cached_tokens` → `prompt_cache_hit_tokens`；write 链依次试
  `cache_creation_input_tokens` → `input_tokens_details.cache_write_tokens` →
  `prompt_tokens_details.cache_write_tokens`。**每一环都要求"存在且非 0"**，因为部分中转把靠前
  字段硬编码成桩 0、真值只在低顺位字段里。

### 14.5 计时与失败行

- `latency_ms = start_time.elapsed()`（`handlers.rs:612,1720,2093`、`response_processor.rs:536,563`）。
- `first_token_ms` 按 §14.2 第 3 步的"首个非元数据事件"口径计算，**只有代理链路有**；
  会话链路这列恒为 NULL。前端速度指标据此分精确/估算两套（§10、§13.4）。
- **`duration_ms` 列存在但从不写入**：全仓只有建表/迁移与读侧映射引用它，代理与七个会话适配器的
  INSERT 语句都没有这一列，因此恒为 NULL（分析时全仓 grep 确认）。
- **失败请求会落库但形态不同**：`log_forward_error` 用 `uuid::Uuid::new_v4()` 作 request_id、
  usage 全 0、`pricing_model` 空串、status 由错误类型映射（六处转发失败分支调用）。
  随机 id 意味着**失败行不可去重，每次重试各占一行**——所以"请求数"里会包含失败次数，
  但它们不计入成功率（成功率只数 2xx），也不参与跨源去重（§7）。
- 一个已存在的不对称：**非流式路径在拿不到 usage 时也照写一行全 0**，
  而流式路径对全 0 usage 是跳过（靠 `has_billable_tokens`）。两条路径对"空用量"的处理不一致。

### 14.6 与会话链路的分工

- **两条链路并行，不是二选一**：后台扫描循环只看 `session_auto_sync_enabled`，**不看代理是否在跑**
  （`lib.rs:1295,1307`）。代理开着时，同一次请求会先被代理记账、随后又被会话扫描看到，
  **去重是唯一的防线**（§7）。
- **代理是主链路**：它信息更全（TTFT、状态码、错误、真实上游模型，甚至能在 Claude 系直接
  REPLACE 会话行），去重判定也以它为保留方。
- **代理没开/未接管**：只有会话链路能看到用量——这就是 README 强调的"不开本地路由也能统计
  Token 用量和花费"的实现方式。
- **两个开关**：代理侧 `enable_logging`（默认 true，关掉后 collector 为 `None`，SSE 热路径
  完全不解析，非流式也跳过；无采样机制）与扫描侧 `session_auto_sync_enabled`（默认 true）。

## 15. 与 TokenScope 的对照：值得抄的、需要避的

### 15.1 已经一致的部分

| 主题 | cc-switch | TokenScope 现状 |
| --- | --- | --- |
| 只读来源 | 只读各工具数据目录（同） | 同（只读原则） |
| 时间口径 | 存储 UTC 秒/原样，展示折叠本地时区 | 存储 UTC RFC3339，一次时区转换 |
| 未知模型 | 不静默吞掉，写 0 + 前端"未定价" + 补价回填 | **无编译期内置价，未收录=未知**（更严格） |
| 金额精度 | `rust_decimal` + TEXT 存十进制 | 同思路（十进制字符串） |
| 缓存语义归一 | input 语义三态 + 黑白名单 SQL 归一 | 四桶独立 + 语义规则 |
| 明细剪枝 | 30 天汇总到日表并删明细，**有精度损失** | 统一 history.db，保留预算另议 |

### 15.2 值得借鉴的实现手法

- **两条链路 + 一张事实表 + `data_source` 列**：查询层统一口径、去重集中在一处；
  新来源只是多一个 `data_source` 取值，不用改聚合 SQL 结构。
- **跨源去重靠指纹（四桶 token + 模型 + ±10 分钟）而非 id**：因为两侧 id 体系天然不同。
  代价是"窗口 + 全等"只能覆盖完全一致的请求；cc-switch 接受这个漏判方向（宁可少去重，也不
  用更激进的模糊匹配导致真实用量被吞）。
- **去重写法的成本自适应**（`effective_usage_log_filter_for_range`）：先数两侧行数，再决定用
  `NOT IN (子查询)` 还是逐行 `EXISTS`——大表上的实用优化，且为它专门建了一条**表达式索引**。
- **游标 + 尾部指纹**（Claude 路径）：既支持字节级增量，又能识别"文件被外部重写"这种
  size/mtime 检测不到的破坏，检出后**钉 EOF 不重放**（注释明确取舍：重放会把已剪明细双算，
  双算比丢行更糟）。
- **日汇总与明细相加、只在完整自然日走汇总**（`compute_rollup_date_bounds`）：这条不变量是
  "既有汇总表又不漏算"的关键，配合"剪枝前先补价"与"汇总写入归一成 FRESH 语义"。
- **事件驱动的 200 ms 防抖刷新**：比单纯轮询更快且不会因流式写入抖动而刷屏。
- **写入侧统一走 `spawn_blocking`**：Tauri 同步命令/异步运行时上的 SQLite 写必须离开主线程
  （TokenScope 的 GUI 主线程纪律同源）。

### 15.3 需要警惕或明确否定的部分

- **不采集 cwd/项目**，也没有项目维度筛选——TokenScope 的项目身份模型（规范化绝对路径 +
  项目根归属）是超出 cc-switch 的能力，不要为了对齐它而放弃。
- **坏行不计数**：cc-switch 的 `SessionSyncResult` 没有 `bad_lines`，解析失败静默跳过。
  TokenScope 的"异常 token 受检算术 + 计数披露"更严，应保留。
- **模型名归一散落在查价层**，7 个适配器基本不做归一；这导致"价格命中"与"模型展示"是两套
  逻辑（`effective_model_sql` 用 `pricing_model` 优先）。TokenScope 用等价身份键统一，更干净。
- **前端多处无错误态**（§13.7）：后端失败与"没有数据"不可区分；TokenScope 应显式区分。
- **剪枝不可逆且官方手册未披露 30 天精度损失**（只有代码注释说明）：TokenScope 若引入汇总
  剪枝，必须把损失写进用户可读文档。
- **前缀匹配无 `ESCAPE`**（§5.4 末）与**占位符模型名短路**（`unknown` 直接不匹配）是两个
  边界细节，TokenScope 的前缀回退规则（分隔符边界）已比它更严。

### 15.4 一个可复用的设计问题清单

如果要在 TokenScope 里对齐 cc-switch 的某项做法，先回答：

1. 这个来源的 token 是"单次值"还是"累计快照"？（Grok 与 Codex 的教训：判错就会漏记或双计。）
2. 半截记录怎么处理？该来源的写入是 UPSERT（可回填）还是 INSERT OR IGNORE（不可回填）？
   ——后者必须先拦住未完成记录（OpenCode 的做法）。
3. 文件被外部重写/截断/压缩/归档后，游标还成立吗？需不需要指纹或身份戳？
4. 这条用量是否可能与代理链路重复？指纹用什么字段？
5. 该来源缺时间戳/缺模型名时，是跳过、写 `unknown`，还是写当前时间？（三者后果不同。）

## 16. 未验证项与已知风险（不得外推）

分析全部来自**静态源码阅读**，未运行 cc-switch 二进制、未跑其测试、未写入其数据目录。

- **行号锚点**：本文引用的行号基于 `df40d53` 工作副本；若上游有本地改动或后续提交，行号会漂移，
  断言需重新核。
- **真实语料**：仓库测试全部使用合成 fixture（项目约定：真实会话日志永不入库），因此
  "解析器与真实日志字段一致"这一点**未验证**；Codex 的真实语料回放测试是 `#[ignore]`。
- **数据库运行时行为**：主库 `journal_mode` 未在实机 PRAGMA 验证（依据是代码未设 WAL +
  `session_usage_codex.rs:1650` 注释自述 `journal_mode=delete`）；也未设 `busy_timeout`，
  多实例同时打开的行为未验证。
- **`rollup_and_prune(30)` 的 30** 是硬编码（`database/mod.rs:155`、`backup.rs:457`），
  未发现设置项路径；是否可通过其他配置改变未穷尽核查。
- **剪枝后重扫的暴露面**：Gemini/OpenCode 等"全量重读"型适配器不写 `session_usage_dedup`，
  文件 mtime 变化后重扫可能重插已剪明细（结构上可推，未运行验证）；Claude/Codex 靠
  `request_id` + 游标规避，注释里明确承认这条暴露面。
- **Codex 的 `reasoning_output_tokens`** 与 `output_tokens` 的数值重叠关系源码未断言（只用它
  参与去重签名）；上游是否还有未识别的计费记录类型，未逐 Codex 版本核对。
- **OpenCode/MiniMax 的源库 schema 不在本仓库**（靠 fixture 与 SQL 文本推断），
  `time_updated` 单位与列类型未对真实库确认。
- **前端**：未构建、未启动 GUI，渲染/时区/错误态观感均未实测；"后端进程时区 = WebView 时区"
  是隐式前提。
- **`LIKE` 无 `ESCAPE`**（§5.4）与 "`u64 as u32` 截断" 均为代码可见风险，未构造用例验证。
- **内置定价条数**"约 196"是按元组起始行统计的估算，运行期真实值来自
  `pricing_data.len()` 的日志。

## 17. 边界：容易与"用量统计"混淆的三套功能

研究时务必区分，它们的数据面与生命周期完全不同：

1. **本地面板统计**（本文主题）：`proxy_request_logs` + `usage_daily_rollups`，
   两条采集链路 + 价格表，命令前缀 `get_usage_*` / `get_provider_stats` / `get_model_stats` /
   `get_request_*`。
2. **供应商额度 / 余额**：供应商卡片与托盘上的"5 小时剩余 94%""账户余额"等。
   走网络查询（官方订阅额度如 `GET https://chatgpt.com/backend-api/wham/usage`、
   coding plan、余额接口），结果进前端 `SubscriptionQuota` / `UsageCache`（进程内、
   **不持久化**，`services/usage_cache.rs:1-4` 明确"托盘展示用、重启即空"），**不写用量表**。
   同一模块还包含**自定义用量脚本**：由 `rquickjs` 在 QuickJS 沙箱里执行用户脚本
   （5 秒超时、16 MiB 内存上限、256 KiB 栈上限、`base_url` 白名单校验，
   `usage_script.rs:9-60`），这是"查询额度"而非"统计用量"。
3. **会话浏览 / 会话管理**：会话列表、正文、删除、压缩等（`session_manager/**`）。
   它同样读各工具的本地会话数据，但目的与落点都不是用量表——例如 Gemini 的 `.project_root`
   与 Grok 的 `info.cwd` 只服务这里，用量适配器完全不读。

**结论**：如果只想复刻"用量统计"，只需关注第 1 套（本文 §2-§14）；第 2 套的凭据管理、
沙箱执行与第 3 套的会话读取都属于另一条产品线，混在一起会显著放大实现面积。

---

## 附录 A：关键常量与阈值速查

| 名称 | 值 | 位置 |
| --- | --- | --- |
| 代理默认监听 | `127.0.0.1:15721` | `proxy/types.rs:42-56` |
| `SCHEMA_VERSION` | 20 | `database/mod.rs:53` |
| 跨源去重时间窗 | ±600 秒 | `usage_stats.rs:317` |
| 明细保留期 | 30 天 | `database/mod.rs:155`、`backup.rs:457` |
| 会话扫描间隔 | 60 秒 | `lib.rs:1290` |
| 事件防抖 | 200 ms | `usage_events.rs:23` |
| 前端刷新间隔默认 | 30 秒 | `UsageDashboard.tsx:58-59` |
| models.dev 同步节流 | 6 小时 | `modelsDevAutoSync.ts:21` |
| 精确速度门槛 | 输出 ≥100 token 且生成窗口 ≥100 ms | `usage_stats.rs:143-147` |
| 估算速度门槛 | 输出 ≥200 token 且耗时 ≥1000 ms | `usage_stats.rs:160-163` |
| 幅度合理区间（耗时估算） | 100 ms – 1 h | `session_usage.rs:206-209` |
| Grok 沉降窗 | 60 秒 | `session_usage_grokbuild.rs:51-66` |
| Pi 显式上限 | 单文件 128 MiB / 50 万 entry | `session_manager/providers/pi.rs:18-20` |
| Grok 单文件上限 | 50 MiB（超限静默跳过） | `session_usage_grokbuild.rs:149` |
| Codex 批量插入 | 1000 行/批 | `session_usage_codex.rs:1451` |
| 缓存写 1 小时系数 | ×1.6（= 2 / 1.25） | `calculator.rs:105` |
| 用量脚本沙箱 | 5 s / 16 MiB / 256 KiB | `usage_script.rs:35-52` |
| 日志计数缓存 TTL | 60 秒 | `usage_stats.rs:545` |

## 附录 B：一次请求两条链路的行为对照

| 阶段 | 代理链路 | 会话链路 |
| --- | --- | --- |
| 触发时机 | 请求进行中（实时） | 后续扫描（≤60 秒延迟） |
| 记录单位 | 一次上游调用 | 一条 assistant 消息 / 一个 token 事件 |
| 时间字段 | `created_at` = 落库时刻（UTC 秒） | `created_at` = 日志内时间戳（缺失退化为当前时间） |
| 首字延迟 | 有（SSE 首非元数据事件） | 无（NULL） |
| 耗时 | 实测 `elapsed()` | 由对话链/事件时间戳估算，100 ms–1 h 之外记 0 |
| 状态码 | 真实 | 恒 200 |
| provider_id | 真实供应商 id | 占位符 `_*_session` |
| 去重角色 | **保留方**（去重时胜出） | 被排除方（指纹命中即整行排除） |
| 未定价处理 | 写 0 + `[USG-002]` warn | 写 0（同一套 `CostCalculator`） |

---

**文档状态**：基于 `df40d53` 的静态源码分析；§16 列出的项需要在真实环境或后续版本上复核。
