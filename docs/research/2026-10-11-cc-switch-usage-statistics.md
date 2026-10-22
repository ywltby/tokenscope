# cc-switch 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`farion1231/cc-switch`](https://github.com/farion1231/cc-switch)，
> 本文对应 **v4.0.18**（HEAD `cb90a244`，2026-10-21）。上一版基于 v4.0.8（`df40d53`）；
> 两版之间的差异集中在 §1.1。
>
> 证据分档：**已验证**（源码/仓库文档可证，行号基于 `cb90a244`）／**发布说明**（只有
> release notes 依据，未逐行核对实现）／**未验证推论**。
>
> 分析方式：浅克隆只读源码，未运行其二进制、未改动本机 `~/.cc-switch` 数据。

## 1. 范围与方法

- 代码规模（`src-tauri/src`，v4.0.18；行数按 `Get-Content` **含空行**统计）：
  `services/usage_stats.rs` 5986 行、`services/session_usage_codex.rs` 6328 行、
  `services/session_usage.rs` 2592 行、`services/session_usage_pi.rs` 2229 行、
  `services/session_usage_grokbuild.rs` 1423 行、`services/session_usage_opencode.rs` 952 行、
  `services/session_usage_gemini.rs` 508 行、`services/session_usage_mcode.rs` 347 行、
  `services/session_usage_agy.rs` 302 行、`proxy/usage/{parser,logger,calculator}.rs` 合计 2339 行、
  `database/schema.rs` 4122 行。
- 分析手段：`rg` 定位符号 → 分段读原文 → 交叉核对调用点与 DDL；关键断言附 `文件:行号`。
- 术语：**代理链路** = 内置本地代理实时记账；**会话链路** = 回扫各工具本地会话日志/SQLite；
  **事实表** = `proxy_request_logs`。

### 1.1 v4.0.8 → v4.0.18 的变化（本次更新依据）

| 变化 | 依据 |
| --- | --- |
| 新增第 8 个会话适配器 **Antigravity CLI（agy）**，`app_type='agy'`、`data_source='agy_session'` | 源码 + `docs/agy-integration-zh.md` |
| `SCHEMA_VERSION` 20 → **21**，迁移只给 `mcp_servers`、`skills` 各加 `enabled_agy` | `database/mod.rs:54` |
| 后端定时扫描从 **60 秒改为 10 分钟兜底**，用量页打开时由前端按所选间隔加扫 | `lib.rs:1394-1445` |
| 统计新增**首字中位数**（`first_token_median_ms` / `est_first_token_median_ms`），供应商/模型表与请求日志加「首字」列 | `usage_stats.rs` 的 `ProviderStats`、`first_token_eligible_sql` |
| **Claude Code 适配器**新增首字估算（从 `thinking` 块反推）与**事务级价格缓存** | 源码 `session_usage.rs`（`thinking_block_start_ms`、`SessionPricingCache`） |
| 精确查价改为 **`COLLATE NOCASE`**（含大写字母的自定义定价也能命中） | `usage_stats.rs:2677` |
| 用量页新增「设置」页签：币种换算、侧栏「今日」、请求日志列、趋势图样式；并记住上次视图 | 前端 `UsageDisplaySection.tsx`、`requestLogColumns.ts`、`rememberedChoice.ts` |
| 新增内置价 Gemini 4 Argon（2026-11-30 前介绍价） | 发布说明 v4.0.14 |
| Codex 若干修复：中途换模型归属、分页子代理补导、分支翻页续写、远程压缩用量 | 发布说明 v4.0.10/11/13/17 |
| Pi 子代理改为从各自会话文件导入；MiniMax Code 从同会话找回模型名 | 发布说明 v4.0.15/16 |
| 托盘改为快捷面板（含用量趋势图） | 发布说明 v4.0.16 |

## 2. 总体架构：两条采集链路，一张事实表

| 维度 | 代理链路（proxy） | 会话链路（session） |
| --- | --- | --- |
| 触发 | 请求经本地路由转发时旁路记账 | 定时/手动/前端加扫 |
| 覆盖 | 只有走了 cc-switch 本地代理的流量 | 各工具自己的本地会话记录，**不开代理也统计** |
| 写入者 | `proxy/usage/logger.rs` | 8 个 `session_usage_*.rs` 适配器 |
| `data_source` | `'proxy'`（含 NULL 兜底） | `session_log` / `codex_session` / `gemini_session` / `opencode_session` / `grok_session` / `pi_session` / `mcode_session` / **`agy_session`** |
| `provider_id` | 真实供应商 id | 占位符 `_session`、`_codex_session`、`_gemini_session`、`_opencode_session`、`_grok_session`、`_mcode_session`、`_pi_session`、**`_agy_session`** |
| 计时精度 | 有首字延迟（`first_token_ms`） | 无首字，`latency_ms` 由日志时间戳估算 |
| 成本 | 响应 usage × 价格表当场算 | 同一套 `CostCalculator`，个别来源可采信自带成本 |

**已验证**：两条链路写同一张 `proxy_request_logs`（`database/schema.rs:202`），靠
`data_source` 区分；查询侧用 `COALESCE(data_source,'proxy')` 兜底历史 NULL 行。

### 2.1 受管应用与占位供应商名

`provider_name_coalesce`（`usage_stats.rs:381`）是"会话来源"到可读名的**权威映射**，
第 389 行新增了 `_agy_session → Antigravity (Session)`：

| provider_id | 展示名 |
| --- | --- |
| `_session` | Claude (Session) |
| `_codex_session` | Codex (Session) |
| `_gemini_session` | Gemini (Session) |
| `_opencode_session` | OpenCode (Session) |
| `_grok_session` | Grok Build (Session) |
| `_mcode_session` | MiniMax Code (Session) |
| `_pi_session` | Pi (Session) |
| **`_agy_session`** | **Antigravity (Session)** |

官方文档口径（`docs/user-manual/zh/4-proxy/4.4-usage.md`）：OpenClaw、Hermes 不支持用量统计；
Claude Desktop 只统计经本地路由的"模型映射"请求，并折叠进 Claude Code 筛选项
（代码对应 `folded_app_type_sql`，`usage_stats.rs:428`，仅用于读侧筛选与分组）。

**v4.0.18 变化（发布说明）**：用量页的应用筛选只列「应用」页里显示的应用；Claude Desktop
可见时 Claude Code 也保留。

## 3. 数据库与迁移

### 3.1 库文件与连接

- 库文件 `cc-switch.db`（SQLite），会话链路、代理链路、价格表、游标全在里面。
- 取连接统一走 `lock_conn!(db.conn)`（单连接 + 互斥锁），写操作在事务/SAVEPOINT 内提交；无连接池。
- 迁移用 `PRAGMA user_version` 单调递增，**`SCHEMA_VERSION = 21`**（`database/mod.rs:54`）；
  整条迁移链包在 `SAVEPOINT schema_migration` 里，失败回滚；库比程序新时拒绝启动。
- 升级前自动备份（`database/mod.rs` 的 `version > 0 && version < SCHEMA_VERSION` 分支）。
- 迁移节点（用量相关）：v8 引入 `data_source` + `session_log_sync`；v9 重刷定价；
  v11 `usage_daily_rollups` 加 `request_model`；v13 加 `input_token_semantics`；
  v16 重建 Codex 用量；v17 `session_usage_dedup`；v18 字节游标列；
  **v21 加 `enabled_agy`**。

### 3.2 用量明细表 `proxy_request_logs`（DDL 原文，`schema.rs:202`）

```sql
CREATE TABLE IF NOT EXISTS proxy_request_logs (
    request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL, model TEXT NOT NULL,
    request_model TEXT, pricing_model TEXT,
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

要点（**已验证**）：金额列是 TEXT 存十进制字符串（查询时 `CAST(... AS REAL)`）；
`model` 是实际上游模型、`request_model` 保留客户端别名、`pricing_model` 是写入时计价基准
（NULL=v11 前旧行，`''`=未计价错误行）；`input_token_semantics` 三态；
`created_at` 是 **Unix 秒**；索引含 `(provider_id, app_type)`、`created_at`、`model`、
`session_id`、`status_code`、`(app_type, created_at DESC)`，以及跨源去重用的**表达式索引**
（`COALESCE(data_source,'proxy')` 形态）。

### 3.3 日汇总表 `usage_daily_rollups`（`schema.rs:282`）

主键 `(date, app_type, provider_id, model, request_model, pricing_model)`；无 `session_id`、
无首字字段、只存 `avg_latency_ms`。`request_model`/`pricing_model` 进主键是为了明细被剪后账单可审计。

### 3.4 同步游标 `session_log_sync`（`schema.rs:312`）

`file_path` 主键 + `last_modified`（纳秒 mtime）+ `last_line_offset` + `last_synced_at`
+ `last_byte_offset` + `last_tail_fingerprint`。Claude 用字节游标 + 尾部指纹；
Codex 用它做 mtime/size 变更检测；Grok/Gemini 只用 mtime；Pi 把 revision 编码进 `last_synced_at`；
mcode 把它当源库自增 id 游标；**agy 完全不用这张表**（见 §12.9）。

### 3.5 去重账本 `session_usage_dedup`（`schema.rs:327`）

`(data_source, request_id)` 主键 + `semantic_id` + `has_entry_id`，索引
`(data_source, semantic_id, has_entry_id)`。设计意图：明细 30 天后被汇总删除，
**但 fork/rewrite 去重仍需这些 request_id**，故另存精简账本。Pi 与 **agy** 使用它。

## 4. 输入 token 语义归一（口径统一的关键）

```rust
// services/sql_helpers.rs:23
pub(crate) const CACHE_INCLUSIVE_APP_TYPES: &[&str] = &["codex", "gemini", "grokbuild"];
// :30-32
pub(crate) const INPUT_TOKEN_SEMANTICS_LEGACY: i64 = 0;
pub(crate) const INPUT_TOKEN_SEMANTICS_TOTAL: i64 = 1;
pub(crate) const INPUT_TOKEN_SEMANTICS_FRESH: i64 = 2;
// :43  fresh_input_sql(alias)：按三态 + 白名单逐行归一
// :73  real_total_tokens_sql(alias)：fresh_input + output + cache_creation + cache_read
```

`fresh_input_sql`（`sql_helpers.rs:43`）的规则：FRESH 原样返回；白名单应用在 TOTAL 时减
`cache_read + cache_creation`、在 LEGACY 时只减 `cache_read`（都带 `>=` 防御）；其余原样。

白名单仍是那三个（**不含 agy**，因为 agy 导出时 input 已排除缓存）。同一常量被写入侧（计价）、
回填侧与读侧共用，前端 `src/types/usage.ts` 有镜像（`CACHE_INCLUSIVE_APP_TYPES`，第 259 行）。

## 5. 金额计算：价格表 + 匹配链 + 公式

### 5.1 价格来源（三处）

1. **内置种子表**：硬编码在 `schema.rs::seed_model_pricing`（第 1648 行）的 Rust 数组，
   单位 **USD / 百万 token**，启动时 `INSERT OR IGNORE` 增量补齐，并修复仍等于旧内置值的行。
   v4.0.14 新增 Gemini 4 Argon（介绍价，2026-11-30 前）。
2. **models.dev**：`https://models.dev/api.json`，由**前端**拉取（1 小时 staleTime），
   转换后经 `update_model_pricing_batch` 批量写入；自动同步按 6 小时节流。
3. **用户覆盖文件**：`~/.cc-switch/model-pricing.json`，结构含
   `version / modelsDevSync / models[] / deletedModelIds[]`，与 SQLite 表双向同步。

字段映射：`cost.input → inputCostPerMillion`、`cost.output → outputCostPerMillion`、
`cost.cache_read → cacheReadCostPerMillion`、`cost.cache_write → cacheCreationCostPerMillion`。

### 5.2 模型名 → 价格条目的匹配链

`find_model_pricing_row`（`usage_stats.rs:2614`）两轮：**先全候选精确匹配，再前缀回退**。

- 候选生成 `model_pricing_candidates`（`usage_stats.rs:2718`）：清洗（末段 `/`、截 `:`、`@`→`-`、
  小写、去 `[1m]`）后栈式派生（命名空间、`claude-` 非 Anthropic 前缀、Bedrock `-vN`、
  日期后缀、reasoning 档位、`claude-*` 点号归一）。
- 精确匹配 `query_model_pricing_exact`（`:2669`）——**v4.0.14 起 SQL 是
  `WHERE model_id = ?1 COLLATE NOCASE`（`:2677`）**，因此价格表里含大写的自定义条目也能命中。
- 前缀回退 `query_model_pricing_prefix`（`:2692`）用 `LIKE '<candidate>-%'` 取最短命中，
  且只在 `should_try_pricing_prefix_match`（`:2905`）放行时尝试（`claude-` 需 ≥3 个连字符、
  `o1/o3/o4/o5` ≥1、`gpt-/gemini-/deepseek-/qwen-/glm-/kimi-/minimax-` ≥2）。
- 占位符（`unknown`/`null`/`none`/空）直接无候选 → 不计价。

### 5.3 成本公式（`proxy/usage/calculator.rs:44-120`）

```rust
let million = Decimal::from(1_000_000);
// OpenAI/Gemini/Grok 风格 input 含缓存，先扣；Claude/agy 风格不扣
let billable_input_tokens = if input_includes_cache_read {
    usage.input_tokens.saturating_sub(usage.cache_read_tokens)
                        .saturating_sub(usage.cache_creation_tokens)
} else { usage.input_tokens };
let input_cost  = Decimal::from(billable_input_tokens) * pricing.input_cost_per_million / million;
let output_cost = Decimal::from(usage.output_tokens) * pricing.output_cost_per_million / million;
let cache_read_cost = Decimal::from(usage.cache_read_tokens) * pricing.cache_read_cost_per_million / million;
// 5 分钟缓存写用表内单价；1 小时写入按 1.6 倍（= 2 / 1.25）
let cache_creation_cost =
    (Decimal::from(usage.cache_creation_tokens - cache_creation_1h_tokens)
        + Decimal::from(cache_creation_1h_tokens) * Decimal::new(16, 1))
    * pricing.cache_creation_cost_per_million / million;
let total_cost = (input_cost + output_cost + cache_read_cost + cache_creation_cost) * cost_multiplier;
```

**已验证**：全链路 `rust_decimal`；`cost_multiplier` 只乘总价；代理写路径恒传 `Decimal::ONE`
（供应商级倍率已停用，仅旧行回填会读历史值）；1 小时分档来自
`cache_creation.ephemeral_1h_input_tokens`，只在计价时用、不落库；无汇率换算（运行时），
也**没有"估算值"标记列**。

### 5.4 未知模型：不拒算、写 0、可回填

- 查不到价：token 照记、五项成本写 `"0"`，并打 `[USG-002]` 警告。
- 前端 `isUnpricedUsage` 识别（2xx + 四类 token 非零 + 倍率≠0 + 成本 0），显示**「未定价」**。
- 补价回填：`total_cost_usd <= 0` 且有 token 的行，在价格表变动后按写入时的 `pricing_model`
  重算（`backfill_missing_usage_costs`，`usage_stats.rs:2370`）；已非 0 的行永不改写。
- 内置表的播种与修复遵循同一原则：`INSERT OR IGNORE` 绝不覆盖已改价，repair 只修"仍等于旧内置值"的行。

## 6. 聚合查询：明细 + 日汇总相加

`get_usage_summary`（`usage_stats.rs:942`）是**两个子查询相加**（明细 + rollup），
`get_daily_trends`（`:1273`）、`get_provider_stats`（`:1709`）、`get_model_stats`（`:1902`）
同样是"明细 UNION/相加 + rollup"。关键机制（**已验证**）：

- **只把"被完整覆盖的本地自然日"交给 rollup**：`compute_rollup_date_bounds`（`:864`）
  起点不在 `00:00:00` 就从次日算、终点不在 `23:59` 只到前一日，空区间直接 `1 = 0`。
- 明细查询带 `effective_usage_log_filter_for_range`（`:535`，跨源去重）。
- `LogCountCache`（键含筛选与起点、`total_changes` 校验、60 秒 TTL，`log_count_cache_key` @ `:631`）
  避免每次刷新全表 count。
- 请求日志分页 `get_request_logs`（`:2104`）是 `LIMIT/OFFSET`，每行经
  `maybe_backfill_log_costs`（`:2442`）兜底回填成本。

## 7. 跨源去重：±10 分钟 + 四桶指纹

窗口常量 `SESSION_PROXY_DEDUP_WINDOW_SECONDS = 10 * 60`（`usage_stats.rs:396`）。
`effective_usage_log_filter`（`:478`）的语义：会话行若能与某条**成功代理行**在 ±10 分钟内
对上 `input/output/cache_read` 三元组 + 模型（大小写不敏感、`unknown` 通配），
且 `cache_creation` 兼容，就**整行排除**。

`effective_usage_log_filter_for_range`（`:535`）是成本自适应版本：先数窗口内两侧行数，
任一侧为 0 就不加条件；代理行更少时改写成 `rowid NOT IN (一次性子查询)`；会话行更少时保留
逐行 `EXISTS`。

**去重来源白名单**（`usage_stats.rs:522`）：

```rust
const DEDUP_SESSION_SOURCES_SQL: &str =
    "'session_log', 'codex_session', 'gemini_session', 'opencode_session'";
```

即只有这四个来源参与跨源去重。`grok_session` 走自己的"接管守卫"（窗口内有 grokbuild 代理行就整段跳过）；
`pi_session` 靠自建身份 + 账本；`mcode_session` 靠源库游标；**`agy_session` 不在其中**
（因为 agy 没有本地代理面，见 §12.9）。

## 8. 采集调度与刷新（v4.0.18 有实质变化）

- **启动时**同步一次（含费用回填 `backfill_missing_usage_costs`）。
- **后台兜底定时改为每 10 分钟**（`lib.rs:1394-1445`，常量 `SESSION_SYNC_INTERVAL_SECS = 10 * 60`
  在第 1398 行），注释写明目的是"防 CLI 清理旧会话文件前漏导入"。
- **用量页打开时由前端按所选刷新间隔加扫**（发布说明 v4.0.12）——这是 60 秒→10 分钟变更的配套：
  以前 60 秒一轮的热度被前端的加扫取代。
- 手动触发：`sync_session_usage` 命令走同一把 `session_sync_mutex`；`rebuild_codex_usage`
  先备份数据库、重置 Codex 用量再重导。
- 前端实时刷新：写库路径调 `usage_events::notify_log_recorded()`，**200 ms 防抖**后 emit
  `usage-log-recorded`（`usage_events.rs:20,23`）。
- 保留期：启动维护 `db.rollup_and_prune(30)`（`database/mod.rs:156`，同批还有
  `cleanup_old_stream_check_logs(7)` @ `:153` 与增量 VACUUM）。

## 9. 日汇总与明细清理（`database/dao/usage_rollup.rs`）

`rollup_and_prune`（`:62`）三步（**已验证**）：

1. cutoff = `now - retain_days` 所在日的**次日 00:00 本地时间**（`compute_local_midnight_cutoff`，`:20`，
   显式处理 DST 歧义与间隙）——保证汇总边界永远落在自然日上。
2. 剪枝前先 `backfill_missing_usage_costs_on_conn` 尽力补价（剪枝不可逆），失败只告警。
3. `SAVEPOINT rollup_prune` 内 `INSERT OR REPLACE INTO usage_daily_rollups`（LEFT JOIN 旧汇总行）
   + `DELETE FROM proxy_request_logs WHERE created_at < ?1`（`do_rollup_and_prune`，`:116`）。

汇总写入时 `input_token_semantics` 统一写 FRESH、`avg_latency_ms` 按请求数加权平均、
删除语句**不带去重过滤**（重复会话行就此一并清掉）。30 天前的逐请求明细不可再查。

## 10. 指标口径

`UsageSummary`（`usage_stats.rs:22`）：

| 指标 | 口径 |
| --- | --- |
| `realTotalTokens` | 新鲜输入 + 输出 + 缓存写入 + 缓存命中（`derive_real_total_and_hit_rate`，`:49`） |
| `cacheHitRate` | `cache_read / (fresh_input + cache_creation + cache_read)`，0–1 小数 |
| `successRate` | 2xx 请求数 / 总请求数 × 100 |

速度与首字（**v4.0.18 扩展**）：

- 精确速度：`first_token_ms` 非空、`output_tokens ≥ 100`、`latency_ms - first_token_ms ≥ 100ms`
  （`SPEED_MIN_OUTPUT_TOKENS` @ `:148`、`SPEED_MIN_GENERATION_MS` 同处）；
- 估算速度：会话日志行，`output_tokens ≥ 200`、`latency_ms ≥ 1000ms`
  （`SPEED_ESTIMATE_MIN_*` @ `:168`）；
- **首字中位数**（发布说明 v4.0.10）：`ProviderStats`/`ModelStats` 新增
  `first_token_median_ms` 与 `est_first_token_median_ms`，谓词是
  `first_token_eligible_sql` / `est_first_token_eligible_sql`；供应商/模型表与请求日志新增「首字」列。
  精确口径只取经路由的成功流式请求的中位数，避免排队/冷启动拉高。
- **会话侧也能估首字**：
  - Claude：`thinking_block_start_ms`（`session_usage.rs:364`）在首块是 `thinking` 且带
    `thinkingDurationMs` 时反推思考块开始时刻，再由 `estimated_first_token_ms`（`:376`）折算；
  - Codex：`output_item_started_ms`（`session_usage_codex.rs:288`）取 `item_completed` 的
    `started_at_ms`（模型输出项的生成开始时刻）。

  两者都经 `estimated_first_token_ms` 校验（结果须落在 `(0, latency_ms]`）后进入
  `est_first_token_median_ms`；响应一开头就是工具调用、没有思考/回复项时推不出来，该列为空。
- 两套速度口径（精确 vs 估算）**分开累计、不相加**；rollup 行的速度与首字列恒为 0/NULL。

## 11. 时区

`created_at` 一律 **UTC 秒**；本地化只发生在两处：日汇总分桶
（`date(l.created_at,'unixepoch','localtime')`）与范围边界换算
（`compute_rollup_date_bounds` @ `:864`、`local_day_start_rfc3339` @ `:927`，用 `chrono::Local`
并处理 DST 歧义/间隙）。

前端全用本机时区，**没有时区设置项**；隐含前提是"后端进程时区 = WebView 时区"。

## 12. 八个会话适配器

`sync_all_unlocked`（`services/session_usage.rs:137`）**串行**依次调用：
Claude → Codex → Gemini → OpenCode → Grok Build → Pi → MCode → **Antigravity**，
单步失败只进 `errors`，不影响其余。共性：无统一 trait；共用 `TokenUsage` 载体、
`CostCalculator`、`find_model_pricing`、`session_log_sync`（agy 除外）与 `SessionSyncResult`；
**全部绕过 `database/dao/*` 直接写内联 SQL**，**没有一家采集 cwd/项目路径**
（`proxy_request_logs` 无该列，项目维度只存在于会话浏览层的元数据）。

### 12.1 汇总对照表

| 适配器 | 数据面 | 格式 | 粒度 | 身份键 | 幂等写法 | 语义 |
| --- | --- | --- | --- | --- | --- | --- |
| Claude Code | `<claude 配置目录>/projects/**` | JSONL | 逐条 assistant 消息 | `session:{message.id}` | `INSERT OR IGNORE` + 补全 UPDATE | 0（LEGACY） |
| Codex | `<codex 配置目录>/{sessions,archived_sessions}` | rollout JSONL（可 `.zst`） | 逐请求（优先 last，缺失才差分） | `codex_session:thread-v1:{root_thread_id}:{event_index}` | `INSERT OR IGNORE` | 1（TOTAL） |
| Gemini CLI | `<gemini 目录>/tmp/<hash>/chats/session-*` | JSON/JSONL | 逐消息 | `gemini_session:{sid}:{mid}` | UPSERT | 0 + 白名单 |
| OpenCode | `opencode.db`（V1/V2） | SQLite | 逐消息（自带 cost 优先） | `opencode_session:{sid}:{mid}` | `INSERT OR IGNORE` | 0（fresh） |
| Grok Build | `~/.grok/{sessions,archived_sessions}/…/updates.jsonl` | JSONL | **逐轮独立总量，禁差分** | `grok_session:{sid}:{turn_key}:{model}` | UPSERT + 接管守卫 | 1（TOTAL） |
| Pi | `~/.pi/agent/sessions` | JSONL | 逐记录（4 类载体） | `pi_session:{sha256(...)}` | 账本 `session_usage_dedup` | 2（FRESH） |
| MiniMax Code | `<data_dir>/v2/sqlite/runtime-state.sqlite` | SQLite | 逐请求 | `mcode:{sid}:{id}` | `INSERT OR IGNORE` + id 游标 | 2（FRESH） |
| **Antigravity** | `~/.gemini/{antigravity-cli,antigravity}/brain` + Key 私有目录 | JSONL | 逐条完成的模型响应 | `agy:{sid}:{step_index}` | 账本 `session_usage_dedup` | 2（FRESH） |

### 12.2 Claude Code（`services/session_usage.rs`，2592 行）

- 入口 `sync_claude_session_logs`（`:403`），目录发现 `collect_jsonl_files`（`:488`）：
  固定 4 层（`项目/*.jsonl`、`<sid>/subagents/*.jsonl`、`subagents/workflows/wf_*/*.jsonl`），
  扩展名字面 `jsonl`，mtime 是唯一门。
- 解析 `sync_single_file`（`:622`）：逐行 `serde_json::Value`；要求 `type=="assistant"`、
  `message.id`、`message.usage` 存在，四桶任一 >0 才导入；文件内按 `message.id` 择优
  （有 `stop_reason` 优先，否则 output 更大者）。
- 四桶直取（`input_tokens` / `output_tokens` / `cache_read_input_tokens` /
  `cache_creation_input_tokens`），1h 分档只用于计价。
- 游标：字节游标 + 4096 字节尾部指纹（`claude_tail_fingerprint` 写入域标签
  `claude-session-tail-v1`，`session_usage.rs:571-573`），截断/重写即钉 EOF 不重放；
  游标推进与插入同事务（`update_sync_state_on_conn` @ `:1022`）。
- 补全：`find_stored_session_row`（`:1068`）→ `session_costs`（`:1119`）→
  `upsert_session_log_entry_on_conn`（`:1235`）；output 增长时 UPDATE，若此时能对上代理指纹则删自身行。
- **v4.0.10 起新增「首字估算」**（生产增量之一）：会话日志没有请求级计时，改为
  `thinking_block_start_ms`（`:364`）从 `message.content[0]` 是 `thinking` 块且带
  `thinkingDurationMs` 时反推思考块开始时刻（`block_end_ms - duration_ms`）；
  `estimated_first_token_ms`（`:376`）用 `first_output_ms - start_ms` 且要求落在
  `(0, latency_ms]` 内。结果收进 `EstimatedTiming { latency_ms, first_token_ms }`（`:1055`），
  补全时连 `first_token_ms` 一起回填（`StoredSessionRow` 新增该字段）。
- **v4.0.15 起新增「事务级价格缓存」**（生产增量之二）：`SessionPricingCache`（`:1092`）
  在单个文件的导入事务内复用模型价格查询（`pricing_cache.get` @ `:1133`），
  避免每条记录都查一次价；`session_costs`（`:1119`）新增 `pricing_cache` 参数。
  配套测试覆盖"命中/未命中/导入中价格被更新"三种情形。
- **增量构成**：旧 2228 行 → 新 2592 行（净 +364），其中生产 1265 → 1369（+104）、
  测试 962 → 1223（+261）——增长同样主要来自上述两块的回归测试。

### 12.3 Codex（`services/session_usage_codex.rs`，6328 行）

- 入口 `sync_codex_usage`（`:1034`）；`sessions` 深度 3 + `archived_sessions` 一层；
  文件名 `rollout-*` + 尾 UUID，`.jsonl.zst` 可读。
- 事件：`event_msg` + `payload.type=="token_count"` 产用量；`session_meta` 只认首条；
  `turn_context` 更新模型。
- token：**优先 `last_token_usage`**，缺失才用 `total_token_usage` 与 high-water 差分；
  缓存钳制 `cached_input.min(input)`、`cache_write.min(input-cached)`。
- 身份前缀 `codex_session:thread-v1`（`:49`），模型归一四步（`normalize_codex_model`，`:854`）；
  批写 1000 行（`CODEX_INSERT_BATCH_SIZE`，`:1953`）。
- **v4.0.10–4.0.17 的修复**（发布说明 + 源码对照）：中途换模型按线程设置的变更归属
  （`thread_settings_applied`，第 1310/1431 行）；桌面版分页产生的续写页用
  `resolve_continuation_baseline`（第 1154 行）定位前序分段的**高水位基线**，缺前序页就
  **报错 deferred** 而不是把整段历史累计当增量；分支对话翻页的续写文件不再当新子会话；
  Codex 0.153+ 远程压缩的 `token_usage_record` 在随后 `token_count` 的 last 为 0 时由
  `push_unmatched_usage`（第 957 行）补记；父 rollout 用 `load_parent_timeline`（第 1682 行）
  + `parent_dependency_stamp`（第 1884 行）缓存时间线，避免分叉会话每分钟整份重读。
- **本文件从 4253 行涨到 6328 行（净 +2075）的构成**：`mod tests` 旧版在第 1876 行、
  新版在第 2423 行——即**生产代码 1875 → 2422 行（+547）**、**测试代码 2377 → 3905 行
  （+1528，占增长约 74%）**。增长主要不是产品功能膨胀，而是上述修复带来的回归测试
  （分页续写多页/分支/fork、retryable pending 的文件替换检测、父时间线缓存失效、
  线程设置模型切换、远程压缩、首字计时等）。

### 12.4 Gemini CLI（`session_usage_gemini.rs`，508 行）

入口 `sync_gemini_usage`（`:42`）。`<gemini 目录>/tmp/<project_hash>/chats/session-*`；
`.jsonl` 恒收、`.json` 仅当同名 `.jsonl` 不存在时收；JSONL 先回放成单对象（`$rewindTo`/`$set`）。
`tokens.{input,output,cached,thoughts}`，thoughts 并入 output；`cache_creation` 恒 0；
身份 `gemini_session:{sid}:{mid}` + UPSERT；只有 mtime 增量，变化即整文件重读。

### 12.5 OpenCode（`session_usage_opencode.rs`，952 行）

入口 `sync_opencode_usage`（`:82`）。SQLite `opencode.db`（V1/V2 双布局），WAL mtime 参与新鲜度；
会话级 `time_updated` 水位 + 文件级 mtime 两级；只导已完成消息（`time.completed`），
因为插入是 `INSERT OR IGNORE`（无法回填）；自带 `cost > 0` 时直接采信总额。
`opencode` 不在缓存包含白名单，其 input 本就是新鲜输入。

### 12.6 Grok Build（`session_usage_grokbuild.rs`，1423 行）

入口 `sync_grokbuild_usage`（`:104`）。`~/.grok/{sessions,archived_sessions}/<enc-cwd>/<sid>/updates.jsonl`；
`turn_completed` 的 usage 是**该轮独立总量**，源码明令禁止差分；60 秒沉降窗；
±10 分钟内有 grokbuild 代理行则整轮跳过（接管守卫）；身份
`grok_session:{sid}:{turn_key}:{model}` + UPSERT。

### 12.7 Pi（`session_usage_pi.rs`，2229 行）

入口 `sync_pi_usage`（`:168`）。`~/.pi/agent/sessions`；四类载体（assistant / toolResult /
compaction / branch_summary）；文件 revision 四元组（mtime/size/尾 4096 字节 SHA256/完整位）
编码进 `last_synced_at`；记录级 `pi_session:{sha256(...)}` + **账本去重**；FRESH 语义。
**v4.0.15（发布说明）**：子代理改为从各自的会话文件导入并带上真实模型/供应商，
升级后第一次导入起统计、不补算历史。

### 12.8 MiniMax Code（`session_usage_mcode.rs`，347 行）

入口 `sync_mcode_usage`（`:12`）。SQLite `<data_dir>/v2/sqlite/runtime-state.sqlite` 的
`local_runtime_token_usage` 表；游标是源表自增 id（存 `last_line_offset`）；
`reasoning_tokens` 并入 output；`INSERT OR IGNORE` + 游标同事务。
**v4.0.16（发布说明）**：模型为 unknown 的记录会从同会话同时间的回复里找回模型名并计价，
已导入的保持原样。

### 12.9 Antigravity（`session_usage_agy.rs`，302 行）—— v4.0.18 新增

入口 `sync_agy_usage`（`:14`）。完整拆解见
[`2026-10-22-cc-switch-agy-usage.md`](./2026-10-22-cc-switch-agy-usage.md)，要点：

- 数据面：三个 `brain` 根（CLI `~/.gemini/antigravity-cli/brain`、
  IDE `~/.gemini/antigravity/brain`、每个 Key 供应商的
  `~/.cc-switch/agy/<provider>/antigravity-cli/brain`），每个会话读
  `<id>/.system_generated/logs/transcript_full.jsonl`；跨根同 id 只算一次；路径 `canonicalize`
  后必须仍在根内。
- 粒度：只取 `PLANNER_RESPONSE` + `status=DONE` 的步骤；四桶直取、语义 FRESH
  （input 不含缓存、output 含 thinking，实测 100/20/5/7 → 80/20/12）。
- 模型：行内 `model` 优先，否则从 `USER_INPUT` 的 "Model Selection" 切换文本按序继承；
  缺失记 `unknown`；显示名归一为定价 ID（`Gemini 3.8 Flash (High)` → `gemini-3.8-flash-high`）。
- 保存：`request_id = agy:{sid}:{step}`，先写 `session_usage_dedup` 墓碑再插明细；
  **没有文件级游标**，每轮全量扫会话（幂等靠账本）。

### 12.10 各适配器的"不做什么"

- **都不采集项目/cwd**（`proxy_request_logs` 无此列）。
- **都不处理 sidechain/isMeta/synthetic**（Claude 只跳过 `attachment` 行做计时）。
- **坏行都不计数**：`SessionSyncResult` 只有 `imported/skipped/files_scanned/
  suspected_duplicates/deferred_files/errors`，没有 bad_lines。
- **大多数不做模型名归一**：归一集中在查价层。

## 13. 前端契约与界面

### 13.1 Tauri 命令面（`src/lib/api/usage.ts`，23 个）

`get_usage_summary` / `get_session_usage_summary` / `get_usage_summary_by_app` /
`get_usage_trends`（后端方法名 `get_daily_trends`）/ `get_usage_first_date` /
`get_provider_stats` / `get_model_stats` / `get_request_logs` / `get_request_detail` /
`get_model_pricing` / `update_model_pricing` / `update_model_pricing_batch` /
`delete_model_pricing` / `get_models_dev_sync_config` / `save_models_dev_sync_config` /
`record_models_dev_sync_result` / `check_provider_limits` / `sync_session_usage` /
`get_session_usage_last_sync` / `rebuild_codex_usage` / `get_usage_data_sources` /
`queryProviderUsage` / `testUsageScript`（后两个属供应商额度，不是本地面板统计）。
查询命令统一走 `spawn_blocking`（Tauri 同步命令在主线程，会卡窗口）。

### 13.2 类型契约

`UsageSummary`（`src/types/usage.ts:85`）字段：`totalRequests`、`totalCost`（字符串）、
四类 token 总数、`successRate`、`realTotalTokens`（`:94`）、`cacheHitRate`（`:96`，0–1 小数）。
`KNOWN_APP_TYPES`（`:236`）已含 `agy`；`CACHE_INCLUSIVE_APP_TYPES`（`:259`）是 Rust 白名单的镜像。

### 13.3 交互与筛选

预设 `today / 1d / 7d / 14d / 30d / all / custom`（`UsageRangePreset`，`:190`），默认 `today`；
应用 chips + 供应商下拉 + 模型级联 + 状态码（日志页签）；请求日志服务端分页；
**没有项目/目录维度**；**没有导出**。

**v4.0.14 起新增「设置」页签**（`UsageDisplaySection.tsx`）：
- **币种换算**（`DisplayCurrency` + 可改汇率）：费用仍按 USD 记录，只在显示时换算；
- 侧栏「今日」显示项（`SIDEBAR_TODAY_ITEMS`，最多两项）；
- 请求日志列开关（`requestLogColumns.ts` 的 `TOGGLEABLE_REQUEST_LOG_COLUMNS`，新增缓存写入、
  命中率、总耗时、状态码四列）；
- 趋势图样式（柱形 / 曲线面积，`TREND_CHART_TYPES`）；
- 记住上次视图（`rememberedChoice.ts`；应用/供应商/模型筛选不保留）。

### 13.4 图表与刷新

recharts 柱状图（或曲线面积），≤24h 用小时桶、>24h 用本地自然日桶，空桶补 0；
「全部」预设换成自绘 53 周热力图。刷新：默认 30 秒轮询 + 事件驱动（`usage-log-recorded`，
200 ms 防抖）；**扫描本身**由前端在打开用量页时按所选间隔触发加扫（§8），
后台兜底 10 分钟。

### 13.5 空态与错误处理

全局空态按"全时间范围 0 请求"判定；未知成本显示「未定价」；缓存写不可用显示 `N/A`；
速度不可算显示 `—`。**后端报错只在少数组件有 error 分支**（详情抽屉、定价面板、
models.dev 面板、同步/重建 toast），Hero、趋势图、请求日志表、供应商表、模型表只解构
`data/isLoading`——失败会呈现为 `-- / 0 / 暂无数据`，与"真的没有用量"不可区分。

## 14. 代理链路：一次请求如何被记账

- 默认监听 **`127.0.0.1:15721`**（`proxy/types.rs`），接管方式是改写客户端 Live 配置的
  `base_url` + 占位 token `PROXY_MANAGED`，不是系统代理。
- 生命周期：`handlers.rs` 建 `RequestContext`（记 `start_time`、提取 session_id）→ 转发 →
  非流式读整包 / 流式 `SseUsageCollector` 边转发边解 SSE → `log_usage_internal`
  （算 request_id、解析计价锚点、丢 `spawn_blocking`）→ `UsageLogger::log_with_calculation`
  → `log_request` 落库。
- 首字口径：**首个"非元数据"SSE 事件**（跳过 `response.created`/`ping`/`[DONE]` 等）。
- 冲突处理（`proxy/usage/logger.rs`）：无同 id → 插入；已有 `session_log` 行 →
  `INSERT OR REPLACE` 覆盖；已有同语义 proxy 行 → 幂等返回；其他 → 改写成
  `{id}:collision:{sha256(9 元组)}`。
- 各协议抽取：Claude（`message_start` + `message_delta`，保留纯 cache-read 请求）、
  OpenAI 兼容（流式倒序取最后一个非空 usage）、Codex（`response.completed` → 顶层 usage 事件 →
  回退 OpenAI）、Gemini（`usageMetadata`，output = total − prompt）；缓存字段多源探测
  （每环要求"存在且非 0"）。
- **v4.0.18**：失败请求仍落库（随机 UUID、usage 全 0、`pricing_model` 空串），不计成功率、
  不参与跨源去重；`duration_ms` 列存在但从不写入。

## 15. 与 TokenScope 的对照

**已经一致**：只读来源、UTC 存储 + 一次时区转换、未知模型不静默吞掉、十进制金额、
缓存语义归一、明细剪枝后保留汇总。

**值得借鉴**：

- 两条链路 + 一张事实表 + `data_source` 列，查询层统一口径；
- 跨源去重靠指纹而非 id（±10 分钟 + 四桶 + 模型通配），并做成本自适应改写；
- 游标 + 尾部指纹（Claude 路径）识别外部重写后钉 EOF 不重放；
- 日汇总与明细相加、只在完整自然日走汇总；剪枝前先补价；
- 事件驱动的 200 ms 防抖刷新；
- **新增来源时的"三张白名单"纪律**：缓存包含语义、跨源去重来源、前端应用筛选；
- v4.0.14 的 `COLLATE NOCASE` 精确匹配——价格表来自用户输入，大小写不该决定命中。

**需要警惕**：

- 不采集 cwd/项目（TokenScope 的项目身份模型是超出它的能力）；
- 坏行不计数（TokenScope 有受检算术与计数披露）；
- 前端多处无错误态；
- 30 天剪枝的精度损失只在代码注释里，用户手册未披露；
- `agy_session` 未进跨源去重白名单是"因为它没有代理面"——这类例外必须有明确理由，否则会静默错账。

### 15.1 跟进核对：Claude/Codex token 用量（2026-10-22）

只看 token 用量；首字时间、生成速度不在 TokenScope 范围内。v4.0.8 → v4.0.18 中触及 Claude/Codex 会话适配器的提交逐个核对了 PR 说明与实现，并在本机 `~/.codex` 的 68 个 rollout 文件上只读统计（只输出计数与合计，脚本不入库）。§16 中「v4.0.10–4.0.17 只读了 release notes」一句，对下表与 Codex 用量有关的几项已不再适用。

| cc-switch 改动 | 对 TokenScope 的影响 | 本机实测 | 结论 |
| --- | --- | --- | --- |
| Codex 远程压缩用量（PR #8479，`8cfe5644`） | **漏计**：TokenScope 把 `token_usage_record` 当作累计回显忽略，又把随后全 0 的 `last_token_usage` 当零分量占位行跳过 | 5555 条记录中 5511 条与随后的 `token_count` 四分量完全相同，44 条压缩被跳过；漏计输入 10,712,650（缓存命中 9,532,672）、输出 205,483，占 Codex 总量的输入 0.22%、输出 1.18% | 跟进，见[计划](../plans/active/2026-10-22-codex-compaction-usage.md) |
| Codex 线程设置切换模型（PR #8176，`68122bc4`） | 潜在风险：TokenScope 只从 `turn_context` 取模型 | 564 条 `thread_settings_applied` 中 7 条改变模型，但之后都先有新的 `turn_context`，没有用量归错 | 一起跟进 |
| Codex 分叉父线程的分页续写（PR #8213、#8290） | 不受影响：TokenScope 不定位父线程文件 | 21 个子线程文件都带父线程的 `session_meta`、会话 id 相同，现有 `(会话, 模型, 四桶)` 去重合并 3489 条重放 | 不跟进 |
| 父 rollout 非法 UTF-8 记录（PR #8360） | 不受影响：TokenScope 按 lossy 读取，坏行计数跳过，同文件其余行照常 | — | 不跟进 |
| Claude 适配器（`a0dd3b39`、`ad9f0f20`、`c7dceb80`） | 只有首字时间与计价缓存；token 口径不变 | — | 不跟进 |
| 库版本 20 → 21 | 只给 MCP/技能表加 agy 开关；`proxy_request_logs`、`usage_daily_rollups` 不变；TokenScope 不以版本号判定支持性，`agy` 行在导入预览中列为不支持 | — | 不跟进 |

两点修正此前的认识：

- `token_usage_record.payload.usage` 是**单次**用量（与 `token_count.last_token_usage` 同口径），累计值在 `thread_token_usage` / `turn_token_usage`。`docs/stats-semantics.md` §2.2「线程级累计回显」的说法需随计划一并改正。
- cc-switch 把分叉重放中的压缩记录列为未解决项（issue 7917 第 1 项）；TokenScope 的子线程使用根会话 id，重放与父文件命中同一保守身份，补计压缩用量后仍需用测试证明不重复。

## 16. 未验证项与已知风险

- 本文行号基于 `cb90a244`；后续提交会漂移。
- 未运行 cc-switch 或任何上游 agent，未回放真实语料；仓库测试均为合成 fixture。
- 未验证：主库运行时 `journal_mode`（代码未设 WAL，Codex 适配器注释自述 `journal_mode=delete`）、
  多实例并发行为（单连接 + Mutex，无 `busy_timeout`）。
- 未逐行核对：`rollup_and_prune(30)` 的 30 是否为设置项（两处调用均硬编码）；
  v4.0.10–4.0.17 的修复只读了 release notes，未逐个核对实现。
- 前端的币种换算、列配置、记住视图等只确认了组件与类型存在，交互细节未运行验证。

## 17. 边界：容易与"用量统计"混淆的三套功能

1. **本地面板统计**（本文主题）：`proxy_request_logs` + `usage_daily_rollups` + 价格表。
2. **供应商额度/余额**：联网查询（官方订阅额度、Coding Plan、余额），结果进进程内
   `UsageCache`（不持久化），**不写用量表**；自定义用量脚本由 QuickJS 沙箱执行
   （5 秒 / 16 MiB / 256 KiB 限额）。
3. **会话浏览/管理**：会话列表、正文、删除、压缩（`session_manager/**`），读同一批本地数据，
   但落点不是用量表——agy 的 `conversation_summaries.db`、Gemini 的 `.project_root`、
   Grok 的 `info.cwd` 都只服务这里。

---

## 附录 A：关键常量与阈值速查（v4.0.18）

| 名称 | 值 | 位置 |
| --- | --- | --- |
| `SCHEMA_VERSION` | **21** | `database/mod.rs:54` |
| 后台会话扫描兜底 | **10 分钟** | `lib.rs:1398` |
| 跨源去重时间窗 | ±600 秒 | `usage_stats.rs:396` |
| 明细保留期 | 30 天 | `database/mod.rs:156` |
| 事件防抖 | 200 ms | `usage_events.rs:23` |
| 前端刷新间隔默认 | 30 秒 | `UsageDashboard.tsx` |
| models.dev 同步节流 | 6 小时 | `modelsDevAutoSync.ts` |
| 精确速度门槛 | 输出 ≥100 token 且生成窗口 ≥100 ms | `usage_stats.rs:148` |
| 估算速度门槛 | 输出 ≥200 token 且耗时 ≥1000 ms | `usage_stats.rs:168` |
| 耗时估算合理区间 | 100 ms – 1 h | `session_usage.rs` |
| Grok 沉降窗 | 60 秒 | `session_usage_grokbuild.rs` |
| Codex 批量插入 | 1000 行/批 | `session_usage_codex.rs:1953` |
| 缓存写 1 小时系数 | ×1.6（= 2 / 1.25） | `calculator.rs:105` |
| 代理默认监听 | `127.0.0.1:15721` | `proxy/types.rs` |
| 用量脚本沙箱 | 5 s / 16 MiB / 256 KiB | `usage_script.rs` |
| 日志计数缓存 TTL | 60 秒 | `usage_stats.rs` |

## 附录 B：一次请求两条链路的行为对照

| 阶段 | 代理链路 | 会话链路 |
| --- | --- | --- |
| 触发 | 请求进行中（实时） | 启动一次 + 10 分钟兜底 + 打开用量页时加扫 |
| 记录单位 | 一次上游调用 | 一条消息 / 一个 token 事件 / 一条完成响应 |
| 时间字段 | 落库时刻（UTC 秒） | 日志内时间戳（缺失退化当前时间） |
| 首字延迟 | 有（首个非元数据事件） | 无（NULL；统计侧改用估算耗时） |
| 状态码 | 真实 | 恒 200 |
| provider_id | 真实供应商 | 占位符 `_*_session` |
| 去重角色 | 保留方 | 被排除方（仅四个来源参与指纹去重） |
| 未定价处理 | 写 0 + `[USG-002]` warn | 写 0（同一套 `CostCalculator`） |

---

**文档状态**：基于 `cb90a244`（v4.0.18）的静态源码分析；§16 列出的项需在真实环境或后续版本复核。
