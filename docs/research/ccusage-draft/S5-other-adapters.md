## S5 其余 16 个 adapter 的本地数据读取实现

- 仓库：`ccusage`（只读样本）HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31
- 证据约定：所有 `path:line` 均相对仓库根；行号取实际文件行号（用 `read` 逐文件核对）
- 方法：静态阅读（`read` / `grep` / `glob`）为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5），未修改任何样本
- 覆盖：`rust/adapters/{opencode,copilot,antigravity,pi,grok,kimi,openclaw,codebuff,hermes,zcode,qwen,droid,kilo,goose,gemini,amp}`、`rust/adapters/common/src/{jsonl.rs,lib.rs,report.rs}`、`rust/crates/ccusage-adapter-all/src/{loader.rs,report.rs,types.rs}`，并以 `ccusage-core` 的 `types/cost/utils/home/date_utils/progress` 作为公共零件参照
- 边界：Claude（`rust/adapters/claude/`）与 Codex（`rust/adapters/codex/`）另见 §4 与 §5，本章只在跨适配器对比处引用其行号，不重复展开

### 0. 结论速览

| 问题 | 结论 | 关键证据 |
| --- | --- | --- |
| 有没有统一的「发现→解析→LoadedEntry」模板函数 | **没有**。`common/` 只提供零件（文件收集 / 并行读 / 日期过滤 / JSONL 迭代器），每家 adapter 自己写 `load_entries` + `load_entries_inner` | `common/src/lib.rs:14`, `common/src/lib.rs:18`, `common/src/lib.rs:84`, `common/src/jsonl.rs:47`，对照 `amp/src/loader.rs:8` |
| 16 家共有的包装 | 全部 16 家的入口都包在 `ccusage_core::progress::track_usage_load` 里（进度条 + 计时） | `ccusage-core/src/progress.rs:290`；`amp/src/loader.rs:9`, `antigravity/src/loader.rs:12`, `zcode/src/loader.rs:51` |
| 并行读文件 | 15 家用 `common::read_files_parallel`（按文件大小分块、结果按原文件序重组）；**antigravity 是唯一顺序 for 循环的** | `common/src/lib.rs:84`, `common/src/lib.rs:49`；`antigravity/src/loader.rs:23` |
| SQLite 适配器 | 7 家：`opencode`、`openclaw`、`zcode`、`kilo`、`goose`、`hermes`、`antigravity`；**全部用 `sqlite` crate（无 rusqlite）**，全部 `with_read_only()` | `rust/Cargo.toml:65`；`zcode/src/loader.rs:91`, `goose/src/loader.rs:75`, `hermes/src/loader.rs:43`, `kilo/src/loader.rs:52`, `openclaw/src/sqlite.rs:87`, `opencode/src/loader.rs:288`, `antigravity/src/parser.rs:146` |
| token 四桶归一到哪里 | `LoadedEntry.data.message.usage: TokenUsageRaw`（input/output/cache_creation_input/cache_read_input），汇总时经 `TokenCounts::add_usage` 落到 `input/output/cache_creation/cache_read` 四桶 | `ccusage-core/src/types.rs:29`, `ccusage-core/src/types.rs:69`, `ccusage-core/src/types.rs:78` |
| 把 cached 从 input 里减掉 | 明确做的有四家：`grok`（`split_input_tokens`）、`copilot`（input − cache_read − cache_creation）、`zcode`（input − cache_read − cache_creation）、`gemini`（条件性 `subtract_cached_overlap_tokens`） | `grok/src/parser.rs:170`, `copilot/src/parser.rs:445`, `zcode/src/parser.rs:47`, `gemini/src/parser.rs:400` |
| 缺字段兜底 | **12 家调用（共 14 处）** `apply_total_token_fallback(usage, extra, total)`，把「总 token 与分项之差」塞进 output 或 `extra_total_tokens`；其中 amp（`:158`/`:261`）与 kimi（`:208`/`:255`）各 2 处 | `ccusage-core/src/utils.rs:22`；`kimi/src/parser.rs:255`, `opencode/src/parser.rs:193` |
| CostMode 三态落点 | `Display`（只用厂商自有 cost）/ `Auto`（有则用、无则算）/ `Calculate`（强制按 pricing 算）；分派在 `ccusage-core/src/cost.rs:31`，各家另有自有的优先级覆盖 | `ccusage-core/src/cost.rs:39-45`；`zcode/src/parser.rs:94`, `hermes/src/parser.rs:180`, `kimi/src/parser.rs:378` |
| 目录不存在 | 绝大多数静默返回空：`collect_files_with_extension` 读不到目录直接 `return`；只有「home 目录解析不出」或「显式配置非法」才报错或降级 | `common/src/lib.rs:19`；`amp/src/paths.rs:25`, `opencode/src/paths.rs:36`, `zcode/src/paths.rs:20` |

### 1. 总表（18 行：16 家 + Claude/Codex 对照）

| # | agent | 默认根目录（环境变量覆盖） | 格式 | 关键字段 | SQLite | 自有 cost 字段 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `opencode` | `$XDG_DATA_HOME/opencode` → `~/.local/share/opencode`（`OPENCODE_DATA_DIR`） | SQLite `opencode.db` / `opencode-<channel>.db` **+** 旧 `storage/message/<sid>/<mid>.json` | `tokens{input,output,reasoning,cache.{read,write},total}`、`modelID`、`providerID`、`time.created`、`cost` | 是 | 是 `cost` |
| 2 | `copilot` | `$COPILOT_HOME` → `~/.copilot`（+`COPILOT_OTEL_FILE_EXPORTER_PATH`） | JSONL：`otel/**.jsonl` 与 `session-state/<sid>/events.jsonl` | OTel `attributes["gen_ai.usage.*"]`；session-state 的 `totalNanoAiu` | 否 | 部分：`cost_usd`（OTel 属性）＋ credits(nano-AIU) |
| 3 | `antigravity` | 6 个默认根：`~/.gemini/antigravity{,-cli,-ide,-backup,-acp}`、`~/.config/antigravity`（`ANTIGRAVITY_DATA_DIR`） | SQLite（`conversations/**.db`），blob 是 protobuf | `gen_metadata(idx,data)`、`steps(idx,metadata)`、`trajectory_metadata_blob(data)` | 是 | 否（全按 pricing 算） |
| 4 | `pi` | `~/.pi/agent/sessions`（`PI_AGENT_DIR`）＋配置 `pi.stores` | JSONL（递归 `*.jsonl`） | `timestamp`、`message.model`、`message.usage{input,output,cacheRead,cacheWrite,totalTokens,cost.total}` | 否 | 是 `message.usage.cost.total` |
| 5 | `grok` | `$GROK_HOME` → `~/.grok`（只认 `sessions/**/updates.jsonl`） | JSONL + 兄弟 `summary.json` | `params.update.usage.modelUsage{}`：`inputTokens`、`cachedReadTokens`、`cacheCreationTokens`、`outputTokens`、`costUsdTicks` | 否 | 是 `costUsdTicks`（定点） |
| 6 | `kimi` | `~/.kimi`、`~/.kimi-code`（`KIMI_DATA_DIR`；只看 `sessions/**/wire.jsonl`） | JSONL（新旧两种行格式） | 新：`usage{inputOther,output,inputCacheRead,inputCacheCreation}`；旧：`payload.token_usage.*` | 否 | 否 |
| 7 | `openclaw` | `~/.openclaw`、`~/.clawdbot`、`~/.moltbot`、`~/.moldbot`（`OPENCLAW_DIR`） | JSONL（含 `*.jsonl.deleted.*`/`.reset.*`）**+** SQLite `agents/*/agent/openclaw-agent.sqlite` | `message.usage{input,output,cacheRead,cacheWrite,totalTokens,cost.total}`；`modelId`/`provider` | 是 | 是 `usage.cost.total` |
| 8 | `codebuff` | `~/.config/{manicode,manicode-dev,manicode-staging}/projects`（`CODEBUFF_DATA_DIR`） | 单个 JSON 数组文件（只认 `chat-messages.json`） | `metadata.model`、`metadata.usage`、`metadata.codebuff.usage`、`metadata.runState…messageHistory`、`credits` | 否 | 是 `credits` |
| 9 | `hermes` | `~/.hermes/state.db`（`HERMES_HOME`） | SQLite（表 `sessions`） | `model`、`billing_provider`、`input_tokens`、`output_tokens`、`cache_read_tokens`、`cache_write_tokens`、`reasoning_tokens`、`estimated_cost_usd`、`actual_cost_usd` | 是 | 是 两个 cost 列 |
| 10 | `zcode` | `~/.zcode` → 库 `cli/db/db.sqlite`（`ZCODE_HOME`） | SQLite（`model_usage` LEFT JOIN `session`） | `input_tokens`、`output_tokens`、`cache_creation_input_tokens`、`cache_read_input_tokens`、`computed_total_tokens`、`started_at`、`model_id`、`provider_id`、`session.directory` | 是 | 否 |
| 11 | `qwen` | `~/.qwen/projects/<project>/chats/*.jsonl`（`QWEN_DATA_DIR`） | JSONL | `usageMetadata{promptTokenCount,candidatesTokenCount,thoughtsTokenCount,cachedContentTokenCount,totalTokenCount}` | 否 | 否 |
| 12 | `droid` | `~/.factory/sessions/**/*.settings.json`（`DROID_SESSIONS_DIR`） | 单个 JSON 对象（settings） | `tokenUsage{inputTokens,outputTokens,cacheCreationTokens,cacheReadTokens,thinkingTokens,totalTokens}`、`model`、`providerLock` | 否 | 否 |
| 13 | `kilo` | `~/.local/share/kilo/kilo.db`（`KILO_DATA_DIR`） | SQLite（表 `message`，`data` 列是 JSON 文本） | `tokens{input,output,reasoning,cache.{read,write},total}`、`modelID`、`time.created`、`cost` | 是 | 是 `cost` |
| 14 | `goose` | `~/.local/share/goose/sessions/sessions.db`（`GOOSE_PATH_ROOT`；另有 macOS / Block 两条候选） | SQLite（表 `sessions`） | `model_config_json`、`provider_name`、`created_at`、`total_tokens`/`input_tokens`/`output_tokens` 及其 `accumulated_*` 变体 | 是 | 否 |
| 15 | `gemini` | `~/.gemini/tmp`（`GEMINI_DATA_DIR`） | 单 JSON **与** JSONL | `tokens{input/prompt,output/candidates,cached,thoughts,tool,total}`、`sessionId`、`model` | 否 | 否 |
| 16 | `amp` | `~/.local/share/amp/threads/*.json`（`AMP_DATA_DIR`） | 单 JSON（一个 thread 一个文件） | `usageLedger.events[].tokens{input,output,total}`、`messages[].usage{cacheCreationInputTokens,cacheReadInputTokens}`、`credits` | 否 | 是 `credits` |
| — | `claude`（对照） | `~/.claude/projects/**/*.jsonl`（`CLAUDE_CONFIG_DIR`） | JSONL | 见 `claude/src/lib.rs:510` 起字节级迭代 | 否 | 是 `costUSD`：记录内厂商成本映射到 `UsageEntry.cost_usd`（`ccusage-core/src/types.rs:14-15`），`Display` 模式直接采用（`claude/src/lib.rs:525-532`） |
| — | `codex`（对照） | `~/.codex/sessions`（`CODEX_HOME`） | JSONL（字节级行分类） | `codex/src/parser.rs:661` 起按标记分类 | 否 | 否（会话日志内无成本字段）；成本按 pricing 计算：汇总输出 `codex/src/report.rs:158-180`，模型级 `calculate_codex_model_cost` `:183-217` |

### 2. 公共层：可复用的读取零件

#### 2.1 `common/src/jsonl.rs` 公共函数清单

| 函数 | 签名要点 | 行号 | 语义 |
| --- | --- | --- | --- |
| `records` | `pub fn records<'data, T>(content: &'data [u8], prefilter: Option<&'data LinePrefilter>) -> impl Iterator<Item = T> where T: DeserializeOwned + 'data` | `jsonl.rs:47` | `byte_lines` 切行 → 先跑 `memmem` 预筛 → `serde_json::from_slice::<T>(line).ok()`；失败行静默丢弃 |
| `lenient_u64` | `pub fn lenient_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>` | `jsonl.rs:75` | 对齐 `Value::as_u64`：浮点/字符串/null/负数/缺失一律 0 |
| `lenient_i64` | `pub fn lenient_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>` | `jsonl.rs:95` | 对齐 `Value::as_i64`：整数才 Some，浮点被拒 |
| `lenient_f64` | `pub fn lenient_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>` | `jsonl.rs:112` | 任意 JSON 数字都成 Some |
| `lenient_object` | `pub fn lenient_object<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>` | `jsonl.rs:131` | 非对象 → `None`，嵌套对象反序列化失败也只是 `None`（不丢整行） |
| `lenient_array` | `pub fn lenient_array<'de, D, T>(deserializer: D) -> Result<Option<Vec<T>>, D::Error>` | `jsonl.rs:155` | 数组内坏元素跳过；区分「空数组」与「非数组」 |
| `lenient_vec` | `pub fn lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>` | `jsonl.rs:180` | 同上但把缺失/非数组折叠成空 `Vec` |
| `non_empty_string` | `pub fn non_empty_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>` | `jsonl.rs:196` | 非字符串与 trim 后为空 → `None`，其余 trim 后返回 |

设计动机写在模块头注释里（`jsonl.rs:1-15`）：避免 `Value` 中间树、整文件一次读、按标记预筛行、直接反序列化到 typed struct。`records` 的 `prefilter` 语义为「必须出现在每个可接受行里」（`jsonl.rs:25-27`）。

#### 2.2 `common/src/lib.rs` 公共函数清单

| 函数 | 签名要点 | 行号 | 语义 |
| --- | --- | --- | --- |
| `collect_usage_files` | `pub fn collect_usage_files(dir: &Path, files: &mut Vec<PathBuf>)` | `lib.rs:14` | 等价于按 `jsonl` 扩展名收集 |
| `collect_files_with_extension` | `pub fn collect_files_with_extension(dir: &Path, extension: &str, files: &mut Vec<PathBuf>)` | `lib.rs:18` | 递归；`fs::read_dir` 失败**直接 return**（目录不存在 = 静默空）；用 `entry.file_type()` 判定，不跟随符号链接 |
| `filter_loaded_entries_by_date` | `pub fn filter_loaded_entries_by_date(entries: &mut Vec<LoadedEntry>, shared: &SharedArgs)` | `lib.rs:36` | `since`/`until` 都为空时短路；否则按 `entry.date` 文本闭区间过滤 |
| `chunk_file_indexes_by_size` | `pub fn chunk_file_indexes_by_size(files: &[PathBuf], chunk_count: usize) -> Vec<Vec<usize>>` | `lib.rs:49` | 贪心装箱（大文件优先），`chunk_count.max(1)` |
| `read_files_parallel` | `pub fn read_files_parallel<T, F>(files: &[PathBuf], single_thread: bool, read: F) -> Vec<T> where T: Send, F: Fn(&Path) -> T + Sync` | `lib.rs:84` | `thread::scope` 分块并发；**结果按原文件下标回填**（`results[index] = Some(value)`，`lib.rs:119`），因此顺序去重结果与单线程一致 |

`common/src/report.rs` 只有渲染：`print_table_for_agent(agent_name, kind, rows, shared)`（`report.rs:13`）、`agent_report_label`（`report.rs:285`）、breakdown 子行（`report.rs:232`）。

**关键结论：`common/` 里没有「逐文件解析并产出 `LoadedEntry`」的模板函数。** 并行读 + 顺序去重的编排在每家 adapter 内部重复实现，共 15 次（见本章 4.1）。

#### 2.3 `ccusage-core` 侧的公共零件

| 项 | 位置 | 说明 |
| --- | --- | --- |
| `UsageEntry` | `types.rs:9` | `session_id/timestamp(String)/version/message/costUSD(request_id/is_api_error_message/is_sidechain)`，`camelCase` |
| `UsageMessage` | `types.rs:22` | `usage: TokenUsageRaw` + `model` + `id`（**唯一的 message 级身份字段**） |
| `TokenUsageRaw` | `types.rs:29` | `input_tokens`、`output_tokens`、`cache_creation_input_tokens`、`cache_read_input_tokens`、`speed`、`cache_creation`（5m/1h 细分） |
| `cache_creation_token_count` | `types.rs:42` | 有细分块时用 5m+1h，否则用 `cache_creation_input_tokens` |
| `TokenCounts`（统一四桶） | `types.rs:69` | `input/output/cache_creation/cache_read` + `extra_total_tokens`；`add_usage`（`:78`）用 `saturating_add`；`total()`（`:89`）含 extra |
| `LoadedEntry` | `types.rs:125` | `data`、`timestamp: TimestampMs`、`date`、`project`、`session_id`、`project_path`、`cost`、`extra_total_tokens`、`credits`、`message_count`、`model`、`usage_limit_reset_time`（`types.rs:137`）、`missing_pricing_model` |
| `calculate_cost` | `cost.rs:9` | 用 `UsageEntry` 直接算 |
| `calculate_cost_for_usage_at` | `cost.rs:31` | `(model, usage, cost_usd, timestamp, mode, pricing)`，三态分派（`cost.rs:39-45`） |
| `missing_pricing_model_for_usage` | `cost.rs:48` | `Display` 或 `Auto+cost 存在` → `None` |
| `missing_pricing_model_for_token_total` | `cost.rs:61` | 总 token 为 0 → `None` |
| `missing_pricing_model_for_candidates` | `cost.rs:77` | 候选模型全未命中才报缺价 |
| `calculate_cost_from_pricing` | `cost.rs:118` | 纯 token 单价计算（含 1h cache 的 2× 输入价，`cost.rs:7`） |
| `apply_total_token_fallback` | `utils.rs:22` | 分项之和 < total 时，先补 `output_tokens`；output 非 0 则进 `extra_total_tokens` |
| `total_usage_tokens` | `utils.rs:14` | 四桶之和（cache_creation 走 `cache_creation_token_count`） |
| `json_value_u64` / `non_empty_json_string` | `utils.rs:5` / `utils.rs:9` | 动态 `Value` 导航 |
| `home_dir` | `home.rs:3` | `HOME` → `USERPROFILE` → `HOMEDRIVE`+`HOMEPATH`（`home.rs:12-31`） |
| `date_range_bounds_ms` | `date_utils.rs:212` | 日期范围 → 毫秒边界（copilot/opencode 的窗口） |
| `parse_tz` / `format_date_tz` | `date_utils.rs:301` / `date_utils.rs:313` | 时区解析与 `entry.date` 生成 |
| `track_usage_load` | `progress.rs:290` | 每家 `load_entries` 的统一包装 |

#### 2.4 `ccusage-adapter-all` 的装配（18+ 家如何被统一驱动）

- 模块别名集中在一处：`lib.rs:11-30`（`ccusage_adapter_amp as amp` … `ccusage_adapter_zcode as zcode`），入口 `pub fn run(args: AgentCommandArgs)`（`lib.rs:38`）。
- 表驱动：`loader.rs:116-358` 构造 18 个 `AgentLoadSpec { index, agent, progress_agent, load }`，Claude/Codex 固定占 index 0/1（`loader.rs:117-128`），其余 16 家顺位 2..17。
- 三种接线方式：
  - `load_summary_agent_rows`（`loader.rs:555`）：只需 `FnOnce() -> Result<Vec<LoadedEntry>>` + `summarize_entries`——`opencode`（`loader.rs:134`）、`openclaw`（`loader.rs:237`）、`grok`（`loader.rs:332`）。
  - `load_priced_summary_agent_rows`（`loader.rs:698`）：额外注入 `&PricingMap`——amp/droid/codebuff/hermes/goose/kilo/copilot/gemini/antigravity/kimi/zcode（`loader.rs:154-356`）。
  - 专用函数：`load_pi_format_agent_rows`（`loader.rs:572`）、`load_named_pi_store_rows_from_paths`（`loader.rs:606`）、`load_qwen_rows`（`loader.rs:715`）。
- 并行调度：`load_agent_rows_parallel`（`loader.rs:422`）用 `thread::scope` + `mpsc` 收集，错误按 index 排序后取第一个（`loader.rs:472-475`），结果再按 index 排序（`loader.rs:477`）。
- 检测语义：`load_summary_agent_rows` 用 `!entries.is_empty()` 判定 detected（`loader.rs:563`）；`opencode`（`loader.rs:145`）、`grok`（`loader.rs:339`）、`qwen`（`loader.rs:717`）额外用 `has_data()` 补齐——因为它们会在读取阶段就按日期窗口裁掉条目。
- 行级过滤：`summary_rows`（`loader.rs:770`）丢弃 `total_tokens == 0 && total_cost == 0.0` 的行，注释说明「Copilot credits 缺失 modelMetrics 但仍有账」这种行要保留（`loader.rs:786-789`）。
- 聚合：`aggregate_rows`（`loader.rs:888`）按 Daily/Monthly/Weekly/Session 归并；`AllAccumulator`（`types.rs:64`）逐桶 `saturating_add`（`types.rs:80-86`），`into_row` 输出 `agent: "all"` 并把 `agent_breakdowns` 排序（`types.rs:108-131`）。

### 3. 逐家分片

#### 3.1 `amp`

- **根与环境变量**：`AMP_DATA_DIR`（`amp/src/paths.rs:5`, `:10`），逗号分隔多路径，只收 `is_dir()` 的项（`:17`）；**一旦设置环境变量就直接返回，不回落默认**（`:21`）。默认 `~/.local/share/amp`（`:26`），`home_dir()` 解析失败才报 `home directory is not set`（`:25`）。
- **存储形态**：`<root>/threads/**/*.json`，一个 thread 一个 JSON 对象（`amp/src/loader.rs:18-20`）。
- **解析入口**：`read_thread_file(path, tz, mode, pricing)`（`amp/src/parser.rs:89`），优先 `usageLedger.events`（`:104-116`），否则退到 `messages`（`:118`）。
- **关键字段**：ledger 事件读 `timestamp`/`model`/`tokens.{input,output,total}`/`toMessageId`/`credits`（`parser.rs:52-71`, `:150-157`, `:212`）；thread 内 `messages[].usage.cacheCreationInputTokens`/`cacheReadInputTokens` 先按 `messageId` 建表（`parser.rs:331-349`），ledger 事件用 `toMessageId` 关联（`:143-148`）；第二条路径直接读 `usage.inputTokens/outputTokens/cacheCreationInputTokens/cacheReadInputTokens`（`:252-255`，role 必须是 `assistant`，`:232`）。
- **会话/项目**：session = `thread.id`（`parser.rs:168`, `:208`）；`project` 固定 `"amp"`，`project_path` 固定 `"Amp"`（`:207-209`）——**没有按路径末段推导项目**。
- **兜底与代价**：`apply_total_token_fallback(usage, 0, tokens.total)`（`parser.rs:158`, `:261`）；cost 走 `calculate_cost(&cost_data, mode, pricing)`（`:196`），它在 `CostMode::Display` 下取 `cost_usd`，而 amp 的 `data.cost_usd` 恒为 `None`（`:176`），故 **Display 模式 amp 恒为 0**；`credits` 单独透出（`:212`, `:320`）。
- **去重**：**没有去重键**——loader 只是把每个文件的条目顺序 append（`loader.rs:32-34`）后按 timestamp 排序（`:36`），文件级不重复即视为不重复。
- **公共零件**：`collect_files_with_extension`（`loader.rs:20`）、`read_files_parallel`（`:21`）、`jsonl::{non_empty_string,lenient_vec,lenient_object,lenient_array}`（`parser.rs:24/29/34/44`）。
- **坑**：读文件失败只打 debug 日志并当作空集（`loader.rs:22-30`）；JSON 解析失败同样返回空（`parser.rs:96-98`）。

#### 3.2 `codebuff`

- **根与环境变量**：`CODEBUFF_DATA_DIR`（`codebuff/src/paths.rs:5`, `:23`）；默认三个频道目录 `~/.config/{manicode,manicode-dev,manicode-staging}`（`:6`, `:33-36`），再拼 `projects`（`:41-45`，若已是 `projects` 则不重复拼）。
- **存储形态**：`**/chat-messages.json`，**文件内容是 JSON 数组**（`codebuff/src/parser.rs:42`，`serde_json::from_slice::<Vec<Value>>`）。
- **关键字段**：`metadata.model`、`metadata.usage`、`metadata.codebuff.usage`、`metadata.runState.sessionState.mainAgentState.messageHistory`（`parser.rs:136-153`）；`merge_fallback` 优先级合并（`:266`）；`credits` 取 `message.credits`（`:154-157`）。
- **会话/项目**：session_id = `{channel}/{project}/{chat_id}`（`parser.rs:122`），三段全部由目录名推导（`:97-120`）；`project` 固定 `"codebuff"`，`project_path` 固定 `"Codebuff"`（`loader.rs:79-81`）。
- **去重**：`dedup_key`（`parser.rs:345`，由 message 内容 + session + timestamp + model + usage + 序号生成，调用点 `:67-74`）；loader 用 `HashMap<String, _>` **后见胜出**（`loader.rs:41-46`），注释强调并行读之后仍按排序后的原文件顺序做去重，保证与单线程一致（`loader.rs:26-28`）。
- **兜底**：`apply_total_token_fallback(raw_usage, usage.extra_total_tokens, total_tokens)`（`parser.rs:255`）。
- **代价**：`calculate_codebuff_cost`（`parser.rs:392-423`）**不读 credits**，而是两级候选试算——先按 `entry.model` 算（`:401-408`），为 0 且 provider 已知且模型名未自带前缀时，再按 `"{provider}/{model}"` 算一次（`:415-422`）；`credits` 只透出到 `LoadedEntry.credits`（`codebuff/src/loader.rs:84`）。`missing_codebuff_pricing`（`parser.rs:425`）。
- **公共零件**：`collect_files_with_extension`（`paths.rs:11`）、`read_files_parallel`（`loader.rs:29`）。
- **坑**：非数组 root 直接返回空（`parser.rs:42-44`），没有任何告警。

#### 3.3 `droid`

- **根与环境变量**：`DROID_SESSIONS_DIR`（`droid/src/paths.rs:5`, `:23`）；默认 `~/.factory/sessions`（`:33`）；只保留 `.settings.json` 结尾的文件（`:14-19`）。
- **存储形态**：每个会话一个 `*.settings.json`（单个 JSON 对象，`droid/src/parser.rs:38-46`）。
- **关键字段**：`tokenUsage.{inputTokens,outputTokens,cacheCreationTokens,cacheReadTokens,thinkingTokens,totalTokens}`（`parser.rs:96-107`）；`model`、`providerLock`（`:50-56`）；模型缺失时读同名 sidecar `*.jsonl`（`:54`, `:301`）。
- **时间戳**：`settings_timestamp(settings, path)`（`parser.rs:126`）产出 `(timestamp, timestamp_text, pricing_timestamp)`——**pricing_timestamp 单独存在**，用于按事件时间选价（`parser.rs:15`, `:80`）。
- **会话**：文件名去掉 `.settings.json`（`parser.rs:71-76`）；`message.id` = `droid:{session_id}`（`loader.rs:68`）；`project_path` 固定 `"Droid"`（`:80`）。
- **去重**：loader 先按 timestamp 排序（`loader.rs:42`），再**逆序遍历按 session 保留最新一条**（`:43-50`，注释 `:26-28` 说明与单线程等价）。
- **兜底**：`apply_total_token_fallback(raw_usage, thinking_tokens, total_tokens)`（`parser.rs:108-109`），thinking 单独进 `extra_total_tokens`（`loader.rs:83`）。
- **代价**：`calculate_droid_cost`（`parser.rs:145`）；缺价 `missing_droid_pricing`（`:167`）；模型名归一 `normalize_droid_model_name`（`:216`）。
- **坑**：文件不存在（`NotFound`）返回 `Ok(None)` 静默跳过，其它 IO 错误才上抛（`parser.rs:33-37`）；但 JSON 解析失败是**硬错误**（`:38-43`）。

#### 3.4 `gemini`

- **根与环境变量**：`GEMINI_DATA_DIR`（`gemini/src/paths.rs:5`, `:11`，设置后不回落，`:22`）；默认 `~/.gemini/tmp`（`:26`）；同时收集 `.json` 与 `.jsonl`（`:38-39`）并排序去重（`:41-42`）。
- **存储形态**：两种——整份 JSON（`gemini/src/parser.rs:115` `parse_json_file`）与 JSONL（`:165` `parse_jsonl_file`，**不设 prefilter**，`:176`）。loader 按扩展名分派（`gemini/src/loader.rs:26-29`）。
- **关键字段**：`tokens` 块多别名读取——input: `input|prompt|input_tokens|prompt_tokens`；output: `output|candidates|output_tokens|candidates_tokens`；cached: `cached|cached_tokens`；thoughts: `thoughts|reasoning|thoughts_tokens|reasoning_tokens`；tool: `tool|tool_tokens`；total: `total|total_tokens`（`parser.rs:360-390`）。
- **会话/项目**：`sessionId`（camel）或 `session_id`（`parser.rs:30-33`）；`project_path` 固定 `"Gemini"`（`:484`）。
- **token 归一**：`normalize_session_input`（`parser.rs:406-416`）在 `cached > 0` 且 `total == input+output+thoughts+tool`（即 total 不含 cached）时，调用 `strip` 逻辑 `subtract_cached_overlap_tokens`（`:400-404`）把 cached 从 input 里减掉；否则原样保留。cache_creation 恒 0（`:450`）。
- **兜底**：`apply_total_token_fallback(display_usage, tokens.thoughts, total_tokens)`（`parser.rs:338`）。
- **代价**：`calculate_gemini_cost`（`parser.rs:496`），`Display → 0.0`（`:504`）；`missing_gemini_pricing`（`:523`）。
- **时间**：`timestamp|created_at|startTime|lastUpdated` 优先（`parser.rs:39-47`），缺省用文件修改时间（`:431`）。

#### 3.5 `goose`

- **根与环境变量**：`GOOSE_PATH_ROOT`（`goose/src/paths.rs:5`, `:9`）；**显式设置且非空时只读 `<root>/data/sessions/sessions.db`**（`:14-19`，root 之后固定拼 `data`/`sessions`/`sessions.db` 三段）；设为空串才回落默认；默认三条候选：`~/.local/share/goose/sessions/sessions.db`、`~/Library/Application Support/goose/sessions/sessions.db`、`~/.local/share/Block/goose/sessions/sessions.db`（`:39-46`）；只收存在且为文件者（`:29`）。
- **存储形态**：SQLite，表 `sessions`。SQL 是**静态字符串常量** `GOOSE_SESSION_QUERY`（`goose/src/loader.rs:11-26`）：
  `SELECT id, model_config_json, provider_name, created_at, total_tokens, input_tokens, output_tokens, accumulated_total_tokens, accumulated_input_tokens, accumulated_output_tokens FROM sessions WHERE model_config_json IS NOT NULL AND TRIM(model_config_json) != ''`。
- **打开模式**：只读（`loader.rs:74-75`），无 WAL/PRAGMA 处理；打开失败或 prepare 失败都只打 debug 日志并返回空（`:76-89`）。
- **关键字段**：模型名从 `model_config_json` 的 `model_name` 取（`goose/src/parser.rs:13-18`, `:115`）；token **优先累计列**（列 8/9），再退到非累计列（列 5/6）（`:32-37`）；`reasoning_tokens = total − input − output`（`:45`）并进 `extra_total_tokens`（`:92`）。
- **时间戳**：`created_at` 接受字符串或整数两种列类型（`parser.rs:106-113`），再经 `parse_goose_timestamp`（`:120`）。
- **会话**：列 `id`；`message.id` 也是 `id`（`parser.rs:63`）；去重键 `"{db_path}:{session_id}"`（`loader.rs:58`）。
- **代价**：`calculate_goose_cost`（`goose/src/parser.rs:175-208`，计费 usage 里 output 会先加上 reasoning，见 `:188` 处的 `calculate_cost_for_usage_at` 调用）；provider 归一 `normalize_provider`（`:153`）、`missing_goose_pricing`（`:210`）。
- **坑**：cache 四桶里的 cache_creation/cache_read 恒 0（`parser.rs:50-51`）——goose 库里没有缓存列。

#### 3.6 `hermes`

- **根与环境变量**：`HERMES_HOME`（`hermes/src/paths.rs:5`, `:8`，逗号分隔）；默认 `~/.hermes`（`:18`）；固定后缀 `state.db` 且必须 `is_file()`（`:23-24`）。
- **存储形态**：SQLite，表 `sessions`；SQL 为**内联静态字符串**（`hermes/src/loader.rs:54-72`）：`SELECT id, model, billing_provider, started_at, message_count, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_cost_usd, actual_cost_usd FROM sessions WHERE model IS NOT NULL AND TRIM(model) != ''`。
- **打开模式**：只读（`loader.rs:42-43`）。
- **关键字段**：四桶直接对应列（`cache_read_tokens`/`cache_write_tokens`）；`reasoning_tokens` 进 `extra_total_tokens`（`hermes/src/parser.rs:171`）；`message_count` 透出到 `LoadedEntry.message_count`（`:172`）。
- **provider 归一**：`normalize_provider`（`parser.rs:110-124`）把 anthropic/claude、openai/codex、google/vertex/gemini、openrouter、xai、groq 折叠；缺省时按模型名推断（`:126-140`，兜底 `"hermes"`）。
- **会话**：列 `id`；`message.id` = `hermes:{session_id}`（`parser.rs:156`）；session 首次出现胜出（`loader.rs:28-35`）。
- **代价**：`calculate_hermes_cost`（`parser.rs:180-183`）**优先 `actual/estimated_cost_usd`**，为 0 时按候选模型循环试算（`:189-201`）；计费 usage 用 `output_tokens + reasoning_tokens`（`:184-188`）。
- **坑**：库打不开/表查不动都只 debug 日志返回空（`loader.rs:44-52`, `:74-82`）。

#### 3.7 `kilo`

- **根与环境变量**：`KILO_DATA_DIR`（`kilo/src/paths.rs:9`, `:15`，设置后不回落，`:26`）；默认 `~/.local/share/kilo`（`:30`），库文件名 `kilo.db`（`:10`, `:38-40`）。
- **存储形态**：SQLite，表 `message`；SQL 为静态字符串 `SELECT id, session_id, data FROM message`（`kilo/src/loader.rs:60`），`data` 列是 JSON 文本（`:77-82`）。
- **打开模式**：只读（`loader.rs:51-52`）。
- **关键字段**：`tokens.{input,output,reasoning}`、`tokens.cache.{read,write}`、`tokens.total`、`modelID`、`time.created`、`id`、`sessionID`、`cost`、`providerID`（`kilo/src/parser.rs:60-138`）。
- **时间归一**：`normalize_timestamp`（`parser.rs:174-184`）：小于 `1e12` 视为秒并 ×1000。
- **会话**：`data.sessionID` 优先，否则用列 `session_id`（`parser.rs:115-118`）；`message.id` = `data.id`，否则 `"{db_path}:{row_id}"`（`:119-122`）。
- **去重**：loader 按 message id **首次胜出**（`loader.rs:29-40`）。
- **兜底**：`apply_total_token_fallback(usage, reasoning_tokens, total_tokens)`（`parser.rs:98-99`）。
- **代价**：`calculate_kilo_cost`（`parser.rs:186-200`）：`Display → data.cost_usd`（`:193`），`Auto → cost_usd 优先否则计算`（`:194-197`），`Calculate → 强制计算`（`:197`）。
- **公共零件**：`read_files_parallel`（`loader.rs:26`）。

#### 3.8 `kimi`

- **根与环境变量**：`KIMI_DATA_DIR`（`kimi/src/paths.rs:9`, `:16`，设置后不回落，`:27`）；默认同时尝试 `~/.kimi` 与 `~/.kimi-code`（`:31`）；只认 `sessions/` 下**深度为 3 或 5** 的 `wire.jsonl`（旧布局 `<group>/<session>/wire.jsonl`、新布局 `<ws>/<session>/agents/<agent>/wire.jsonl`，`:58-74`）。
- **存储形态**：JSONL；`read_wire_file` 用 `LinePrefilter::any([r#""token_usage""#, r#""usage.record""#])`（`kimi/src/parser.rs:126`）+ `jsonl::records`（`:127`）。
- **关键字段（新格式）**：`type == "usage.record"` 且 **`usageScope == "turn"`**（`:196`，注释：`session` 级记录是累计总量，必须丢弃 `:195`）；桶来自 `usage.inputOther/output/inputCacheRead/inputCacheCreation`（`:200-207`）。
- **关键字段（旧格式）**：`message.type == "StatusUpdate"` → `payload.token_usage.{input_other,output,input_cache_read,input_cache_creation,total}`，并保留 `payload.message_id`（`:235-274`）。
- **模型**：从恢复出的 store root 读 `config.json` 的 `model`（`:132-143`），缺省 `kimi-for-coding`（`:17`）；`kimi-code/` 前缀剥离（`:217-220`）。
- **会话**：目录名（兼容 `agents/<agent>/` 一层，`:288-308`）。
- **时间**：新格式 `time`（毫秒，`:212-215`）；旧格式 envelope `timestamp` 是**秒**（`:259-262` + `timestamp_from_seconds` `:277`）。
- **去重**：`kimi_entry_key`（`:310-323`）由 session+message_id+timestamp_text+model+四桶+extra 拼成，loader 首次胜出（`loader.rs:33-47`）。
- **代价**：`calculate_kimi_cost`（`parser.rs:372-396`）`Display → 0.0`（`:379`），其余按 `model_candidates` 试算（`:415-428`，含按时间切 `moonshot/kimi-k2.5` → `kimi-k2.6`，`:430-436`）。
- **兜底**：两处 `apply_total_token_fallback`（`:208` 传 0、`:255` 传 `token_usage.total`）。

#### 3.9 `openclaw`

- **根与环境变量**：`OPENCLAW_DIR`（`openclaw/src/paths.rs:9`, `:15`），命令行 `custom_path` 优先（`:12`）；默认四选一 `~/.openclaw`、`~/.clawdbot`、`~/.moltbot`、`~/.moldbot`（`:23-28`），只收存在目录。
- **文件发现**：递归、**跳过符号链接**（`paths.rs:56`），文件名含 `.jsonl` 且后缀是 `.jsonl`/`.jsonl.deleted.*`/`.jsonl.reset.*` 才算会话文件（`:75-83`）。
- **SQLite 通道**：按 `agents/*/agent/openclaw-agent.sqlite` 收集（`openclaw/src/sqlite.rs:16-18`, `:42`），表 `transcript_events`，SQL 静态字符串 `SELECT session_id, seq, event_json, created_at FROM transcript_events ORDER BY session_id ASC, seq ASC`（`:105`）；只读打开（`:87-90`）；先查 `sqlite_master`（`:180-189`）；**按 session 边界重置 model/provider 跟踪**（`:113-130`，与 JSONL 每文件重置语义对齐）。
- **关键字段**：`message.usage.{input,output,cacheRead,cacheWrite,totalTokens,cost.total}`（`openclaw/src/parser.rs:112-133`，`cost` 用专门的反序列化器容忍非对象，`:139-150`）；`message.modelId|model|provider`（`:98-107`）；`model_change`/`model-snapshot` 记录推进当前模型（`:193-201`, `:216-241`）。
- **会话**：JSONL 文件名去 `.jsonl` 后缀（`parser.rs:443-457`）；SQLite 用行内 `session_id`；展示模型加前缀 `[openclaw] `（`:317`）。
- **去重（重点）**：`entry_id = migration_id + ":" + cost`（`parser.rs:469-471`），`migration_id` 是「agent+session+timestamp+model+四桶+extra」九段指纹（`:477-491`，注释说明 cost 不能进指纹，因为 SQLite 可能带修正后的账单价）。loader 先收 JSONL（`loader.rs:63-69`），再用 SQLite 行**替换**同 session+message id 或同 migration_id 的既有行（`loader.rs:71-99`，动机注释 `:33-36`）。
- **代价**：`usage.cost.total` 经 `calculate_cost_for_usage_at`（`parser.rs:388-397`）；缺价 `missing_pricing_model_for_usage`（`:397`）。
- **兜底**：`apply_total_token_fallback(raw_usage, 0, total_tokens)`（`parser.rs:295`）。
- **坑**：`created_at` 作为兜底时间（毫秒，`sqlite.rs:131-139`），并注释 `readEventTimestamp` 会回落到 `Date.now()`（`parser.rs:434-441`）。

#### 3.10 `opencode`

- **根与环境变量**：`OPENCODE_DATA_DIR`（`opencode/src/paths.rs:5`, `:11`，支持非 UTF-8 的 `OsString`，`:12-20`，设置后不回落）；默认 `$XDG_DATA_HOME/opencode`，XDG 必须是绝对路径否则回落 `~/.local/share/opencode`（`:29-40`）；home 也缺失时**报错**（`:36`）。
- **双通道存储**：
  - SQLite：`<root>/opencode.db`，或 `<root>/opencode-<channel>.db`（列表排序取第一个，`opencode/src/loader.rs:228-255`）。
  - 旧 JSON：`storage/message/<sessionID>/<messageID>.json`（`loader.rs:146-148`），**文件 stem 就是 message id**，用于跳过 DB 已覆盖的文件（`:150-162` 注释）。
- **表与 SQL**：`message`（`:302`）、`session_message`（`:383`）、聚合回退 `session_v2` 与 `session`（`:482-487`）；SQL 是**运行时拼接**（`prepare_message_query` 定义 `:1069`、`prepare_session_message_query` 定义 `:752`，即 `fn` 关键字所在行，函数签名跨 752–755），并且会把时间窗下推到 SQL——但仅当采样显示 `time_created` 是毫秒量级，否则退化为不带过滤的扫描（`:303-317`、`time_created_looks_like_millis` `:1108-1123`）。聚合条目由 `load_session_aggregate_entries`（`:838`）读取。只读打开（`:287-288`）。
- **关键字段**：`tokens.{input,output,reasoning,total}`、`tokens.cache.{read,write}`、`modelID`、`providerID`、`time.created`、`id`、`sessionID`、`cost`（`opencode/src/parser.rs:16-74`）；v2 表把 `model.{id,modelID,providerID}` 折回 legacy 结构（`:94-143`）。
- **会话**：优先 DB/JSON 的 `sessionID`，文件路径不含 session 信息（`parser.rs:207-208`）；无则 `"unknown"`（`:245-248`）。
- **去重/增量**：
  - `entry_id`（定义 `loader.rs:975`；调用点 `:44-49`、`:136-142`、`:178-184`、以及 `push_unique_entry` `:546-558`）+ `seen/seen_message_ids` 集合，**首次胜出**。
  - fork 副本识别：`fork_copy_cutoffs` 取 fork 表的时间截断（`:615-640`），`is_fork_copy` 判定并跳过（`:433-435`，注释解释 fork 的 `session_v2` 计数仍可作为回退 `:429-432`）。
  - 聚合回退（session 级累计量）**只在无时间过滤且该 session 没有消息级条目时**使用（`:478-503`，`append_aggregate_entries` `:577-594`），因为累计值不能安全切片（`:479-480`）。
- **兜底**：`apply_total_token_fallback(usage, tokens.reasoning, total_tokens)`（`parser.rs:193`）；cost_usage 把 `extra_total_tokens` 加进 output 再计价（`:223-227`）。
- **代价**：`calculate_open_code_cost`（`parser.rs:351-377`）——**先信自有 `cost`**（>0 直接用），否则按 `open_code_model_candidates` 试算；cost > 0 时跳过缺价检查（`:237-244` 注释）。
- **检测**：`has_source(dir)`（`loader.rs:199`）只看 DB 是否存在或有无 message JSON，且刻意忽略时间窗；`lib.rs:24` 的 `has_data()` 是它的上层封装。
- **坑**：v2 行数 > 0 但产出 0 条会打点日志（`loader.rs:466-474`）；非毫秒时间戳会整库无过滤扫描（`:309-315`）。

#### 3.11 `pi`

- **根与环境变量**：`PI_AGENT_DIR`（`pi/src/paths.rs:5`, `:11`，设置后不回落）；默认 `~/.pi/agent/sessions`（`:19`，必须 `is_dir()`）；命名 store 走配置 `pi.stores`，**与默认路径重叠会被拒绝**（`ccusage-adapter-all/src/loader.rs:503-553`，重叠语义含「相等或互为祖先」`:551-553`）。
- **存储形态**：JSONL，递归收集 `.jsonl`（`pi/src/loader.rs:102`）；排除 `subagent-artifacts/` 派生转录（`:110`，理由注释 `:103-109`：那是主会话调用的纯拷贝，既无 session header 也无 entry id）。
- **关键字段**：`type`/`timestamp`/`parentSession`/`id` 与 `message.{role,model,usage}`；`usage.{input,output,cacheRead,cacheWrite,totalTokens,cost.total}`（`pi/src/parser.rs:21-94`）；prefilter 要求同时含 `"usage"` 与 `"message"`（`:315`）。
- **会话/项目**：session_id = 文件 stem 去掉 `前缀_`（`parser.rs:573-582`）；project = 路径中 `sessions/` 之后的下一段（`:584-594`），命名 store 则取相对 store root 的首段（`:596-603`）；展示模型加 `[store] ` 前缀（`:354-356`）。
- **回放抑制（该适配器最复杂的部分）**：`PiUsageSignature`（`parser.rs:104-113`）记录 timestamp/model/四桶/有效总量/计费 cost 位；`replay_usage_path`（`:439-500`）用条目链接 `id/parentId` 从叶子回溯出活跃分支，`Linear`（v1 无链接，`:443-450`）、`Invalid`（链接不完整/有环，`:456-473`）三种结论；`matching_replay_prefix`（`:428-437`）按 fork 时间点比较父子签名前缀，从而剔除子会话里重放的父会话用量。
- **去重**：`entry_id`（`parser.rs:605`，按 store 名区分）；loader 顺序 first-wins（`loader.rs:111-113` 注释 + `HashSet`）。
- **兜底**：`apply_total_token_fallback(usage, 0, total_tokens)`（`parser.rs:348`）。
- **代价**：两种来源——`PiStoreContext::cost`（`parser.rs:240-272`，默认 store 走通用 `calculate_cost_for_usage_at` `:253`）与命名 store 的 `calculate_store_cost`（`:609`）；`source_cost_for_mode`（`:640-650`）决定 `Auto` 时是否让位给外置覆盖价；缺价 `missing_store_pricing_model`（`:665-683`）。

#### 3.12 `qwen`

- **根与环境变量**：`QWEN_DATA_DIR`（`qwen/src/paths.rs:9`, `:12`）；默认 `~/.qwen`（`:21`）；只认 `projects/<project>/chats/*.jsonl`（`is_chat_file` `:48-60`）。
- **存储形态**：JSONL；prefilter 要求含 `"usageMetadata"`（`qwen/src/parser.rs:109`）。
- **关键字段**：`type == "assistant"`（`parser.rs:127`）+ `usageMetadata.{promptTokenCount,candidatesTokenCount,thoughtsTokenCount,cachedContentTokenCount,totalTokenCount}`（`:46-57`, `:131-135`）。
- **会话/项目**：`record.session_id` 优先，否则 `"{project}-{file_stem}"`（`parser.rs:161-167`）；project 由路径反查 `projects/<x>/chats/`（`paths.rs:62-77`）；缺省模型常量 `"unknown"`（`parser.rs:25`, `:168-171`）。
- **时间**：`record.timestamp`（须能被 `parse_ts_timestamp` 解析）否则文件 mtime（`parser.rs:154-159`, `file_timestamp` `:261`）。
- **去重**：`entry_id`（`parser.rs:290-302`）是 session+timestamp+model+input+output+cache_read+extra 的 JSON 数组字符串，loader first-wins（`parser.rs:85-93`）。
- **兜底**：`apply_total_token_fallback(display_usage, reasoning_tokens, total_tokens)`（`parser.rs:145`），reasoning 先作为 extra 传入。
- **代价**：`calculate_qwen_cost`（`parser.rs:212-234`）在候选模型里循环，`Display` 直接交给 core 得出 0，`Auto/Calculate` 只在价格表命中候选时算。`has_data()` 在 `lib.rs:69`。

#### 3.13 `zcode`

- **根与环境变量**：`ZCODE_HOME`（`zcode/src/paths.rs:9`, `:13`）；默认 `~/.zcode`（`:24`）；库固定相对路径 `cli/db/db.sqlite`（`:10`）。根要 `is_dir()` 且能 canonicalize，否则 debug 日志后跳过（`:89-132`）；**配置了非法根不会回落默认**（`:15`，测试 `:178-194`）。
- **存储形态**：SQLite，`model_usage m LEFT JOIN session s ON s.id = m.session_id`；SQL 是 `format!` **拼接**（`zcode/src/loader.rs:132-159`），因为列名要按 schema 自适应。必需列清单见 `:14-23`，缺列即判「不支持的 schema」并跳过（`:225-244`）。
- **打开模式**：只读（`loader.rs:91`）＋ `PRAGMA busy_timeout = 5000`（`:100`）＋ `PRAGMA query_only = ON`（`:107`），任一失败即返回空。schema 自省用 `SELECT * FROM "table" LIMIT 0` 取列名（`:260-266`，注释说明这样不会物化内容列 `:261-262`）。
- **关键字段**：`id`、`session_id`、`started_at`、`model_id`、`input_tokens`、`output_tokens`、`cache_creation_input_tokens`（可选）、`cache_read_input_tokens`（可选）、`computed_total_tokens`（可选）、`provider_id`（可选）、`session.directory`、`session.version`（可选）（`loader.rs:157-159`）；读取时容忍 int/float/负值（`read_token_column` `:293-307`，`read_timestamp_ms` `:309-316`）。
- **token 归一**：`input − cache_read − cache_creation`（`zcode/src/parser.rs:42-56`）；**Z.ai 特例**：cache creation 会被加回 input 计费（`:75-93`，注释：Z.ai 把新内容记为 cache-creation 桶但按标准输入价计费）。
- **会话**：`session_id` 列；`project_path` 取 `session.directory`，空则 `"ZCode"`（`parser.rs:71-74`）；`message.id` 是必有的 `id` 列（`loader.rs:69-76`，`expect` 断言）。
- **去重**：message id first-wins（`loader.rs:66-79`）。
- **兜底**：`apply_total_token_fallback(usage, 0, computed_total_tokens)`（`parser.rs:57-58`）。
- **代价**：`Display → 0.0`，否则强制 `CostMode::Calculate`（`parser.rs:94-105`）；缺价用 `cost_model.is_some()` 短路（`:106-111`）。

#### 3.14 `antigravity`

- **根与环境变量**：`ANTIGRAVITY_DATA_DIR`（`antigravity/src/paths.rs:7`, `:21`，逗号分隔，**不做存在性过滤**）；默认 6 个根（`.gemini/antigravity{,-cli,-ide,-backup,-acp}`、`.config/antigravity`，`:9-16`），且默认根**必须包含 `conversations/` 子目录**，否则跳过（`:56-63`，注释说明新装用户根下只有 `conversation_summaries.db` 会导致 `no such table: gen_metadata` 而中断报告）。收集 `*.db`（`:66`），canonicalize 去重（`:68-71`），排序（`:74`）。
- **存储形态**：SQLite，三张表：`gen_metadata(idx, data)`（`parser.rs:325`）、`steps(idx, metadata)`（`:370` 附近）、`trajectory_metadata_blob(data)`（`:418`，按 rowid）；`data`/`metadata` 是 **protobuf blob**，本仓库手写最小解码器（`decode_fields` `:739`、`read_varint` `:766`、`take_length_delimited` `:795`）。
- **打开模式**：只读（`parser.rs:146`）＋ `BEGIN DEFERRED TRANSACTION`（`:154`）取一致快照；空库判定 `PRAGMA page_count`（`:291`），空库返回 `Ok(None)` 由 loader 打日志跳过（`antigravity/src/loader.rs:24-33`）。
- **proto 字段映射（用量）**：`ModelUsage` = field 2 input、3 total_output、4 cache_creation、5 cache_read、9 reasoning、10 visible_output、6 provider、7 message_id、11 response_id、12 provider_assigned_message_id（`parser.rs:689-703`）。
- **模型名**：`model_name_from_id` 硬编码数字 ID→模型（`parser.rs:849-872`，未登记 ID 走 `antigravity-model-{id}` / `model_placeholder_m{n}`），再过 `normalize_antigravity_model` 归一「Gemini 3.8 Flash (High)」这类写法（`:874-905`）。
- **会话**：session_id = 数据库文件 stem（`parser.rs:164-168`）。
- **去重**：事件带 `identities` 集合，`deduplicate_events`（`loader.rs:43-88`）用 identity→slot 索引合并，冲突时 `merge_usage_event` **逐桶取 max**（`parser.rs:563-585`）。
- **token 归一**：output/reasoning 用 `total_output_tokens` 与 `visible_output_tokens` 互相校正（`parser.rs:506-516`）；`total_tokens = input + cache_creation + cache_read + total_output`（`:551-555`）。
- **代价**：`calculate_antigravity_cost`（`parser.rs:1023`）`Display → 0.0`（`:1032`），否则 `calculate_cost_from_pricing` 或按时间点 `calculate_cost_for_usage_at`（`:1033-1060`）；另有 pricing 别名兜底 `gemini_flash_pricing_alias`（`:1102`）。
- **公共零件**：只用 `collect_files_with_extension`（`paths.rs:3`, `:66`）；**不用 `read_files_parallel`**，库是一个个顺序解析的（`loader.rs:23-34`）。

#### 3.15 `copilot`

- **根与环境变量**：`COPILOT_HOME`（`copilot/src/paths.rs:6`, `:56`）优先，否则 `~/.copilot`（`:61`）；另有单文件覆盖 `COPILOT_OTEL_FILE_EXPORTER_PATH`（`:5`, `:76-83`，必须 `is_file()`）。发现两类文件并排序去重（`:51`）：`otel/**/*.jsonl` 递归（`:27`）与 `session-state/<dir>/events.jsonl`（`:32-45`）。
- **存储形态**：JSONL；OTel 行用 `LinePrefilter::all([r#""attributes""#])`（`copilot/src/parser.rs:130`）；session-state 行用 `LinePrefilter::any([SHUTDOWN_EVENT, USAGE_CHECKPOINT_EVENT])`（`:245-247`）。
- **关键字段（OTel）**：点号字符串键 `gen_ai.usage.input_tokens`、`output_tokens`、`cache_read.input_tokens`、`cache_write|cache_creation.input_tokens`、`reasoning.output_tokens|reasoning_tokens`、`total_tokens|total.token_count`（`parser.rs:418-441`）。
- **token 归一**：`input − (cache_read + cache_creation)`（`parser.rs:442-451`，注释：Copilot 把缓存读写算进 input）+ `apply_total_token_fallback(usage, 0, total)`（`:452`）。
- **时间**：时间戳字段是「数字标量或 `[秒, 纳秒]` 数组」两种形态（结构注释 `parser.rs:19-21`），解析失败用文件 mtime 兜底（`:133`, `:467`）。
- **会话/模型**：`best_session_attr` → trace context → `trace_id`（`parser.rs:462-466`）；模型同理（`:459-461`），最终兜底 `"unknown"`/`"unknown-session"`。
- **多来源裁决**：来源枚举 `ChatSpan/InferenceLog/AgentTurnLog/AgentSummarySpan`（`parser.rs:95-101`, `:407-417`），`should_emit_candidate` 决定是否发出（`:145`），重复 chat span 检测见 `CandidateSets`（`:494-509`）。
- **累计量差分**：session-state 的用量是**按 (session, model) 累计**的，每次 resume 生成一次 shutdown 快照，loader 把相邻快照相减成区间用量（`copilot/src/loader.rs:197-200` 注释；`reconcile_session_state_entries` `:201`，`credit_gap_entries` `:289`）；OTel 行只在没有对应 session-state 记录时使用（`:92-114`）。
- **credits**：`session.shutdown` / `session.usage_checkpoint` 的 `totalNanoAiu`（1 AIU = 1 GitHub AI credit = $0.01，注释 `parser.rs:75-77`），产物是 `CopilotCreditSnapshot`（`:79-87`）。
- **去重与代价**：`dedup_key`（`parser.rs:72`, `:468-476`）同时充当 `message.id`（`loader.rs:484`）；`calculate_cost_for_usage_at`（`loader.rs:491-498`）与 `missing_pricing_model_for_usage`（`:499-505`）；`request_count` 透出到 `message_count`（`:515`）。

#### 3.16 `grok`

- **根与环境变量**：`GROK_HOME`（`grok/src/paths.rs:9`, `:19`，trim 后必须非空且 `is_dir()`）；默认 `~/.grok`（`:26-28`）。
- **存储形态**：只收 `sessions/**/updates.jsonl`（`paths.rs:38-41`，其它 `.jsonl` 一律忽略），同目录的 `summary.json` 是**可选**兄弟文件（`:55-58`）；发现结果按路径排序以保证加载顺序稳定（`:51`，测试注释 `:132`）。
- **关键字段**：只处理 `params.update.sessionUpdate == "turn_completed"` 的行（`grok/src/parser.rs:226`），用量取 `params.update.usage`（`:229`）；事件 id 取 `params.meta.eventId`（`:233`）。
- **多模型拆分**：`modelUsage{}` 非空时按模型拆成多条（逐一排序，`parser.rs:338-347`），为空时回落到顶层 `inputTokens/outputTokens/cachedReadTokens/cacheCreationTokens/reasoningTokens/costUsdTicks`（`:349-363`）。
- **token 归一**：`split_input_tokens`（`parser.rs:170-174`）做 `input − cache_read − cache_creation`；注释给出实测依据——`cachedReadTokens` 是 `inputTokens` 的子集（与 `logs/unified.jsonl` 对齐），`cacheCreationTokens` 也按兄弟子集处理，从而三段之和等于 `inputTokens`（`:163-169`）。
- **时间戳**：`params.meta.agentTimestampMs` 优先（已是毫秒，`:412-419`）；否则 envelope 的 `timestamp` 是 **Unix 秒**，需要 ×1000（`:420-425`，注释 `:423`）。
- **会话/项目**：session 默认取会话目录名，`summary.json` 的 `info.id` 能覆盖它（`parser.rs:383-393`）；project 优先 `info.cwd` 或 `gitRootDir`，否则取上两级目录名并做 URL 解码（`:374-381`, `:394-401`）；`url_decode_lightweight` 按**字节**解码 percent 三元组，避免多字节路径段变 mojibake（`:536-556`，注释 `:537-540`）。
- **去重**：文件内用 `dedupe_key(event_id, session_id, timestamp, model, usage, reasoning)`（`parser.rs:266-276`，定义 `:436`）；loader 层再做一次全局去重，键是 `"{eventId}|{model}"`，**没有 eventId 的条目一律保留**（`grok/src/loader.rs:52-61`，注释解释了为什么不能用 token 计数重建更粗的键 `:46-51`）。
- **兜底策略与别家相反**：**不调用** `apply_total_token_fallback`，`extra_total_tokens` 强制 0（`parser.rs:325`），注释说明 Grok 的 `totalTokens == inputTokens + outputTokens`，reasoning 已含在 output 中，再加一次会把费用算两遍（`:321-324`）。
- **代价**：`cost_usd_from_ticks`（`parser.rs:152-154`，定点 `costUsdTicks / COST_USD_TICKS_PER_USD`）；`calculate_grok_cost_at`（`:468`）三态——`Display` 用 ticks、`Auto` 有 ticks 就用 ticks、否则 `Calculate`（`:477-495`）；缺价 `missing_grok_pricing`（`:516`）。
- **公共零件**：`collect_files_with_extension`（`paths.rs:38`）、`read_files_parallel`（`loader.rs:25`）、`LinePrefilter::all([r#""turn_completed""#])`（`parser.rs:215`）；`has_data()`（`loader.rs:66`）供 unified report 判存在。
- **小坑**：loader 在并行任务里**重新推导** `summary.json` 路径（`loader.rs:26-32`），而不是复用发现阶段已判定的结果；两处判定逻辑等价但重复。

### 4. 共性总结

#### 4.1 「文件发现 → 逐文件解析 → 产出 LoadedEntry」的归属与重复度

- **没有统一模板函数**。`common/src/lib.rs` 与 `common/src/jsonl.rs` 只导出零件：`collect_files_with_extension`（`common/src/lib.rs:18`）、`read_files_parallel`（`:84`）、`filter_loaded_entries_by_date`（`:36`）、`jsonl::records`（`jsonl.rs:47`）。文件发现与解析的编排（tz 解析 → 路径解析 → 收集 → 并行读 → 顺序去重 → 按 timestamp 排序）在每家 adapter 内重复实现。
- **重复度量化**：16 家全部包一层 `track_usage_load`；15 家用 `read_files_parallel`（只有 `antigravity` 不用，它是顺序 for 循环）；15 家采用「`load_entries` 做进度包装 + `load_entries_inner` 做实现」的双函数形状（`qwen` 例外：`qwen/src/loader.rs:5` 只有 9 行，直接转发给 `parser::load_entries`（`qwen/src/parser.rs:59`），实现体就叫 `load_entries` 而没有 `_inner`）；`opencode` 的 inner 额外接一个 `report_kind` 参数用于决定是否允许聚合回退（`opencode/src/loader.rs:22-27`）。
- **`load_entries` 签名分布**：12 家是 `(shared: &SharedArgs, pricing: &PricingMap)`（amp/codebuff/droid/gemini/goose/hermes/kilo/kimi/grok/zcode/antigravity/copilot）；4 家例外——`opencode(shared, report_kind)`（`opencode/src/loader.rs:22`）、`pi(shared, custom_path, Option<&PricingMap>)`（`pi/src/loader.rs:13`）、`openclaw(shared, custom_path, Option<&PricingMap>)`（`openclaw/src/loader.rs:13`）、`qwen(shared)`（`qwen/src/parser.rs:59`，pricing 在函数内部按 mode 决定是否加载，`:60-68`）。
- **公共「并行读 + 顺序去重」协议**：`read_files_parallel` 保证结果按原文件顺序回填（`common/src/lib.rs:113-124`），因此各家可以在**结果上做顺序去重**而不必担心并发乱序——codebuff（`loader.rs:26-28` 注释）、droid（`:26-28`）、goose（`:39-41`）、hermes（`:21-23`）、kilo（`:23-25`）、kimi（`kimi/src/loader.rs:21-23`）、openclaw（`loader.rs:48-50`）、pi（`pi/src/loader.rs:111-113`）、qwen（`parser.rs:71-73`）、gemini（`gemini/src/loader.rs:23-24`）都写了这条注释。
- **各家自行决定的统一收尾**：几乎所有 loader 都以 `entries.sort_by_key(|entry| entry.timestamp)` 结束（amp `loader.rs:36`、codebuff `:51`、droid `:42`、gemini `:39`、goose `:64`、hermes `:37`、kilo `:41`、kimi `:48`、openclaw `:100`、opencode `:59`、qwen `parser.rs:94`、zcode `loader.rs:80`、antigravity `loader.rs:36`、grok `loader.rs:62`）。
- **日期窗口的两种时机**：多数适配器在 loader 内读时不过滤，交给 `ccusage-adapter-all` 的 `filter_loaded_entries_by_date`（`common/src/lib.rs:36`，调用点 `loader.rs:564`, `:582`, `:585`, `:625`, `:726`）；`opencode` 与 `copilot` 反而在**读取阶段**就裁窗口（`opencode/src/loader.rs:121` `DateWindow::from_shared`，`copilot/src/loader.rs:81` `date_range_bounds_ms`），这也是 `opencode` 需要 `has_data()` 补 detected 的原因（`adapter-all/src/loader.rs:141-146`）。

#### 4.2 token 四桶映射到 `TokenUsageRaw` 对照

| agent | input | output | cache_creation | cache_read | 归一/兜底 |
| --- | --- | --- | --- | --- | --- |
| `amp`（ledger） | `tokens.input` | `tokens.output` | messages 表 `cacheCreationInputTokens` | messages 表 `cacheReadInputTokens` | `apply_total_token_fallback(_, 0, tokens.total)` `parser.rs:158` |
| `amp`（messages） | `usage.inputTokens` | `usage.outputTokens` | `usage.cacheCreationInputTokens` | `usage.cacheReadInputTokens` | `apply_total_token_fallback(usage_raw, 0, total_tokens)` `:261` |
| `codebuff` | `usage.inputTokens` | `usage.outputTokens` | `cacheCreationInputTokens` | `cacheReadInputTokens` | `:255` |
| `droid` | `inputTokens` | `outputTokens` | `cacheCreationTokens` | `cacheReadTokens` | `:108-109`（thinking 计入兜底） |
| `gemini` | `tokens.input` | `tokens.output` | 恒 0 | `tokens.cached` | `subtract_cached_overlap_tokens` `:400`、`normalize_session_input` `:406`、兜底 `apply_total_token_fallback(display_usage, tokens.thoughts, total)` `:338` |
| `grok` | `input − cached − cache_creation` | `output` | `cacheCreationTokens` | `cachedReadTokens` | `split_input_tokens` `:170`；extra 强制 0 `:325` |
| `kimi` | `usage.inputOther` / `token_usage.input_other` | `.output` | `.inputCacheCreation` | `.inputCacheRead` | `:208`（0）/`:255`（total） |
| `kilo` | `tokens.input` | `tokens.output` | `tokens.cache.write` | `tokens.cache.read` | `:98`（reasoning 传入） |
| `openclaw` | `usage.input` | `usage.output` | `usage.cacheWrite` | `usage.cacheRead` | `:295` |
| `opencode` | `tokens.input` | `tokens.output` | `tokens.cache.write` | `tokens.cache.read` | `:193`（reasoning 传入） |
| `copilot` | `input − (cache_read + cache_creation)` | `output` | `cache_write`/`cache_creation` 属性 | `cache_read` 属性 | `:452` |
| `pi` | `usage.input` | `usage.output` | `usage.cacheWrite` | `usage.cacheRead` | `:348` |
| `qwen` | `promptTokenCount` | `candidatesTokenCount` | 恒 0 | `cachedContentTokenCount` | `:145`（thoughts 传入） |
| `zcode` | `input − cache_read − cache_creation` | `output_tokens` | `cache_creation_input_tokens` | `cache_read_input_tokens` | `:58`（computed_total） |
| `antigravity` | proto field 2 | field 10 与 9 校正 | field 4 | field 5 | `:506-516`，合并取 max `:567-585` |
| `goose` | `accumulated_input_tokens` 否则 `input_tokens` | 同构 | 恒 0 | 恒 0 | reasoning = total − in − out `:45` |
| `hermes` | 列 `input_tokens` | 列 `output_tokens` | 列 `cache_write_tokens` | 列 `cache_read_tokens` | 计费用 output+reasoning `:184-188` |

统一落点：`TokenCounts::add_usage`（`ccusage-core/src/types.rs:78`），cache_creation 走 `TokenUsageRaw::cache_creation_token_count`（`types.rs:42`，支持 5m/1h 细分），总和 `TokenCounts::total`（`types.rs:89`）包含 `extra_total_tokens`；`UsageSummary::total_tokens`（`types.rs:183`）同构。

#### 4.3 `CostMode` 与 pricing 的接入点（统一函数名 + 行号）

| agent | cost 函数 | 行号 | Display 语义 | 缺价函数 |
| --- | --- | --- | --- | --- |
| `opencode` | `calculate_open_code_cost` | `parser.rs:351` | 先信自有 cost，否则候选试算（`Auto/Calculate` 同码） | `missing_open_code_pricing` `:379` → `:390` |
| `copilot` | 直接调 `calculate_cost_for_usage_at` | `loader.rs:491` | 交给 core：`Display` 用 `cost_usd` | `missing_pricing_model_for_usage` `:499` |
| `antigravity` | `calculate_antigravity_cost` | `parser.rs:1023` | `Display → 0.0`（`:1032`） | `missing_antigravity_pricing` `:1062` |
| `pi` | `PiStoreContext::cost` / `calculate_store_cost` | `parser.rs:240` / `:609` | `Display` 用 `usage.cost.total`（`:619`） | `missing_store_pricing_model` `:665` |
| `grok` | `calculate_grok_cost_at` | `parser.rs:468` | `Display` 用 ticks（`:477`）；`Auto` 有 ticks 用 ticks（`:480`） | `missing_grok_pricing` `:516` |
| `kimi` | `calculate_kimi_cost` | `parser.rs:372` | `Display → 0.0`（`:379`） | `missing_kimi_pricing` `:398` |
| `openclaw` | 内联 `calculate_cost_for_usage_at` | `parser.rs:388` | 交给 core | `missing_pricing_model_for_usage` `:397` |
| `codebuff` | `calculate_codebuff_cost` | `parser.rs:392` | **不读 credits**；两级候选试算（`model` → `provider/model`，`:401-422`） | `missing_codebuff_pricing` `:425` |
| `hermes` | `calculate_hermes_cost` | `parser.rs:180` | 优先两个 cost 列（`:181`） | `missing_hermes_pricing` `:205` |
| `zcode` | 内联（`mode == Display` 短路） | `parser.rs:94` | `Display → 0.0`（`:94-95`） | 内联短路 `:106` |
| `qwen` | `calculate_qwen_cost` | `parser.rs:212` | 候选循环内 `Display` 直接交给 core（`:220`） | `missing_qwen_pricing` `:236` |
| `droid` | `calculate_droid_cost` | `parser.rs:145` | 交给 core | `missing_droid_pricing` `:167` |
| `kilo` | `calculate_kilo_cost` | `parser.rs:186` | `Display → cost_usd`（`:193`） | `missing_kilo_pricing` `:225` |
| `goose` | `calculate_goose_cost` | `parser.rs:175` | 交给 core（计费 output 含 reasoning，`:188`） | `missing_goose_pricing` `:210` |
| `gemini` | `calculate_gemini_cost` | `parser.rs:496` | `Display → 0.0`（`:504`） | `missing_gemini_pricing` `:523` |
| `amp` | `calculate_cost`（core，`UsageEntry` 版） | `parser.rs:196` → `cost.rs:9` | `Display` 用 `cost_usd`（amp 恒 `None`，故为 0） | `missing_pricing_model_for_usage` `:197` |

统一分歧点：`CostMode::Auto` 的语义是「自有 cost 存在就用」，因此凡是有自有 cost 的适配器（kilo/openclaw/grok/hermes/opencode/pi/copilot/kimi）都会在 `Auto` 分支上先判断自有值是否为正或有效，再决定是否回落到 pricing 计算（典型：`kilo/src/parser.rs:194-197`、`hermes/src/parser.rs:181`、`opencode/src/parser.rs:360`、`grok/src/parser.rs:480`）。

#### 4.4 SQLite 适配器细节对照

| agent | 库路径 | 表 | SQL 形态 | 打开/事务 | schema 自省 | 打开失败 |
| --- | --- | --- | --- | --- | --- | --- |
| `opencode` | `opencode.db` 或 `opencode-<ch>.db` | `message`、`session_message`、`session_v2`、`session` | **拼接**（`:157-159` 般 `format!`，另有 `prepare_message_query` `:755`） | 只读 `:287` | `sqlite_master` 判表 `:596-605`、`SELECT * LIMIT 0` 判列 `:732-753` | debug 日志 + 空 `:290-295` |
| `openclaw` | `agents/*/agent/openclaw-agent.sqlite` | `transcript_events` | 静态 `:105` | 只读 `:87` | `sqlite_master` `:180-189` | debug + 空 `:91-96` |
| `zcode` | `cli/db/db.sqlite` | `model_usage` ⋈ `session` | **拼接** `:157-159` | 只读 + busy_timeout + `query_only=ON` `:91/100/107` | `SELECT * LIMIT 0` `:260-266` + 必需列检查 `:225-244` | debug + 空 |
| `kilo` | `kilo.db` | `message` | 静态 `:60` | 只读 `:51` | 无（靠 prepare 失败兜底） | debug + 空 |
| `goose` | `sessions/sessions.db` | `sessions` | 静态常量 `:11-26` | 只读 `:74` | 无 | debug + 空 `:76-82` |
| `hermes` | `state.db` | `sessions` | 静态内联 `:54-72` | 只读 `:42` | 无 | debug + 空 `:44-52` |
| `antigravity` | `conversations/**.db` | `gen_metadata`、`steps`、`trajectory_metadata_blob` | 静态（三处 `prepare`） | 只读 + `BEGIN DEFERRED TRANSACTION` `:146/154` | `sqlite_master` `:451-473`、`PRAGMA page_count` 判空 `:291` | **返回 Err**（`cli_error`，会打断该库解析，`:147-152`） |

crate 与版本：`rust/Cargo.toml:65` → `sqlite = { version = "0.37.4", default-features = false, features = ["bundled", "linkage"] }`；7 家的 `Cargo.toml` 均写 `sqlite.workspace = true`（如 `zcode/Cargo.toml:12`、`antigravity/Cargo.toml:12`）。**没有 rusqlite**。

#### 4.5 缺失路径 / 空目录的分支

| 行为 | 出现者 | 证据 |
| --- | --- | --- |
| 目录读不到就静默返回空（不报错） | 所有用 `collect_files_with_extension` 的适配器 | `common/src/lib.rs:19-21` |
| 列表项必须 `is_dir()` 才采纳 | amp、gemini、kilo、kimi、qwen、droid、codebuff、opencode、grok、pi、openclaw、opencode | `amp/src/paths.rs:17`、`gemini/src/paths.rs:18`、`qwen/src/paths.rs:27`、`pi/src/paths.rs:20` 等 |
| 文件必须 `is_file()` 才采纳 | goose、hermes、kilo（db）、zcode（canonicalize 后判 `is_file`）、copilot（显式 exporter 路径） | `goose/src/paths.rs:29`、`hermes/src/paths.rs:24`、`kilo/src/paths.rs:40`、`zcode/src/paths.rs:33`、`copilot/src/paths.rs:82` |
| home 解析失败 → **报错** | amp、codebuff、droid、opencode、pi、goose、hermes | `amp/src/paths.rs:25`、`codebuff/src/paths.rs:32`、`droid/src/paths.rs:32`、`opencode/src/paths.rs:36`、`pi/src/paths.rs:18`、`goose/src/paths.rs:38`、`hermes/src/paths.rs:17` |
| home 解析失败 → 静默空 | gemini、kilo、kimi、qwen、openclaw、zcode、grok、copilot、antigravity | `gemini/src/paths.rs:25`（`if let Some(home)`）、`zcode/src/paths.rs:20-23`（debug 日志 + 空）、`copilot/src/paths.rs:61`（`Option`）、`antigravity/src/paths.rs:34-42`（`into_iter` 空展开） |
| 显式配置非法 → **不回落默认**，直接空 | zcode（含测试）、amp、gemini、kilo、kimi、opencode | `zcode/src/paths.rs:15`, `zcode/src/paths.rs:178-194`（测试名 `invalid_configured_roots_do_not_fall_back_to_default`）、`amp/src/paths.rs:21`、`opencode/src/paths.rs:26` |
| 文件级读取失败 → 记 debug 日志当空 | 几乎每家 loader 的 `unwrap_or_else` | `amp/src/loader.rs:22-30`、`grok/src/loader.rs:33-42`、`kimi/src/loader.rs:25-31`、`hermes/src/loader.rs:41-53` 等 |
| 配置目录重叠（跨来源重复读）→ **报错** | `pi.stores` 与默认 pi 路径／彼此重叠 | `ccusage-adapter-all/src/loader.rs:503-553`（`paths_overlap` `:551-553`） |

### 5. 未验证 / 存疑

> 第 8 轮实测复核（§13.5）已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目；此处保留原始清单与当时的判断，逐条状态以 §13.5 为准。

- `antigravity` 的 `event_to_loaded`（`antigravity/src/parser.rs:960-1022`）未逐行读：`project` / `project_path`（是否固定 `"Antigravity"`）、`credits` 字段取值、`extra_total_tokens` 是否使用 `reasoning_tokens` 均**未验证**。
- `gemini` 的 `parse_json_file` / `parse_stats_events` 的完整分支（`gemini/src/parser.rs:115-320`）只读了函数名与部分字段别名，`stats`/`result` 两条路径的优先级**未逐行核对**。
- `copilot` 的 `credit_gap_entries`（`loader.rs:289-433`）与 `reconcile_session_state_entries`（`:201-288`）的内部算法只读了入口与注释，**窗口边界与 credits 归因的精确规则未验证**；`should_emit_candidate`（`parser.rs:145`）的去重分支同理。
- `opencode` 的 `prepare_message_query` / `prepare_session_message_query`（`loader.rs:1069` / `:752`（`fn` 行，签名 752–755））与 `time_created_looks_like_millis`（`:1108-1123`）只读了行号与注释，SQL 文本的列裁剪细节未逐行核对；`fork_copy_cutoffs`（`:615-640`）的截断语义只读到函数名与片段，`is_fork_copy`（定义 `:717`，调用点 `:433`）的判定条件未读。
- `pi` 的 `replay_usage_path`（`parser.rs:439-500`）尾部（`:490-500` 之后）未读完，`PiReplayUsagePath` 三个变体被消费的位置（哪一段代码真正剔除条目）**未验证**。
- `amp` 未发现任何去重键（`loader.rs` 全文 38 行已读），但 `usageLedger.events` 与 `messages` 两条路径**是否可能同时命中同一 thread** 只看懂了「有 ledger 就不走 messages」（`parser.rs:104-118`），更复杂的历史数据未验证。
- `codebuff` 的 `dedup_key`（`parser.rs:345-391`）实现体未逐行读，只确认了调用参数（`:67-74`）；`codebuff` 的 `has_signal`（`:290`）判定门槛未读。
- `goose` / `hermes` / `kilo` 的模型名归一与候选列表（`goose/src/parser.rs:115-187`、`hermes/src/parser.rs:222-237`、`kilo/src/parser.rs:186-263`）只读到关键分支，全量候选未罗列。
- `qwen` / `gemini` / `droid` 的「同一会话多文件重复计数」风险：三者都没有用文件级 session 合并（qwen 有 `entry_id`、droid 有 session latest-wins、gemini 完全没有跨文件去重，`gemini/src/loader.rs:38-43`），**gemini 跨文件重复的可能性未验证**（需要真实数据才能确认）。
- 并行度与顺序结论基于 `read_files_parallel` 的代码语义（`common/src/lib.rs:113-124` 按 index 回填）；未运行任何测试验证。
- 本文全部结论来自静态阅读；`antigravity` 的 protobuf 字段号映射（`parser.rs:689-703`）是**代码事实**，但其与真实 Antigravity 版本 schema 的对应关系无法在仓库内验证。
