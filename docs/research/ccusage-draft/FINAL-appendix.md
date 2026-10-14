## 附录 A：关键常量与阈值速查

| 常量 | 值 | 位置 | 用途 |
| --- | --- | --- | --- |
| `DEFAULT_SESSION_DURATION_HOURS` | 5.0 | `rust/crates/ccusage-core/src/lib.rs:59` | 计费窗口长度（`blocks`/statusline） |
| `DEFAULT_RECENT_DAYS` | 3 | `rust/crates/ccusage-core/src/lib.rs:60` | `--recent` 的天数 |
| `USAGE_COMPACT_WIDTH_THRESHOLD` | 100 | `rust/crates/ccusage-core/src/lib.rs:61` | 表格转紧凑版的终端宽度阈值 |
| `BUILT_IN_AGENT_NAMES` | 18 项 | `rust/crates/ccusage-core/src/lib.rs:63-82` | claude/codex/opencode/amp/droid/codebuff/hermes/pi/goose/openclaw/kilo/copilot/gemini/antigravity/kimi/qwen/grok/zcode |
| `BLOCKS_WARNING_THRESHOLD` | 0.8 | `rust/crates/ccusage/src/blocks.rs:18` | 5 小时窗口用量告警比例 |
| `BLOCKS_COMPACT_WIDTH_THRESHOLD` | 120 | `rust/crates/ccusage/src/blocks.rs:20` | blocks 表紧凑阈值 |
| `MTIME_PRUNE_MARGIN_MS` | 1 天 | `rust/adapters/claude/src/paths.rs:84` | `--since` 剪枝的比较余量 |
| `CACHE_CREATE_1H_INPUT_MULTIPLIER` | 2.0 | `rust/crates/ccusage-core/src/cost.rs:7` | 1 小时缓存写入 = input 价 × 2 |
| `DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS` | 200 000 | `rust/crates/ccusage-core/src/pricing.rs:220` | `*_above_200k_tokens` 的默认分界 |
| `MODEL_DATE_SUFFIX_DIGITS` | 8 | `rust/crates/ccusage-core/src/pricing.rs:61` | 8 位数字后缀视为日期（可前缀匹配） |
| `MODELS_DEV_FAILURE_RETRY_AFTER` | 60 s | `rust/crates/ccusage-core/src/pricing.rs:58` | models.dev 抓取失败后的退避 |
| `MODELS_DEV_TRUST_OWNER` / `PLATFORM` / `RESELLER` | 3 / 2 / 1 | `rust/crates/ccusage-core/src/pricing.rs:649-651` | 目录可信度排序 |
| `DEEPSEEK_V4_PRICING_CUTOFF_MS` | 1 786 896 000 000（2026-08-16T16:00:00Z） | `rust/crates/ccusage-core/src/pricing.rs:252` | DeepSeek V4 峰谷价分界 |
| `CODEX_REWRITTEN_BURST_PAUSE_MS` | 1 000 | `rust/adapters/codex/src/parser.rs:122` | 判定「日志被重写」的突发间隔 |
| `PRICING_FETCH_TIMEOUT_SECONDS` | 10 | `rust/crates/ccusage/src/http.rs:11` | 价格抓取超时 |
| `PRICING_FETCH_MAX_BYTES` | 64 MiB | `rust/crates/ccusage/src/http.rs:12` | 价格响应体上限 |
| `CACHE_ETAG_MAX_BYTES` | 4096 | `rust/crates/ccusage/src/http.rs:13` | ETag 侧车文件上限 |
| 缓存价推导 | `cache_create = input × 1.25`、`cache_read = input × 0.1` | `rust/crates/ccusage-core/src/pricing.rs:1085-1088` | LiteLLM 未给缓存价时的默认值 |
| 缺价披露门槛 | `total_tokens == 0` 不报 | `rust/crates/ccusage-core/src/cost.rs:66-68` | 避免无用量行刷警告 |
| 日志级别 | `LOG_LEVEL` 0–5（默认 3） | `docs/guide/environment-variables.md` + `rust/crates/ccusage-core/src/logger.rs` | 0 静默、1 警告、2 日志、3 info、4 debug、5 trace |

## 附录 B：模块与入口索引

### B.1 Rust workspace

| crate | 关键文件（行数） | 关键入口 |
| --- | --- | --- |
| `crates/ccusage` | `main.rs`（1063）、`commands/mod.rs`（1411）、`blocks.rs`（618）、`http.rs`（768）、`cli.rs`（51） | `main()`、`run_daily`/`run_bucket`/`run_weekly`/`run_session`/`run_blocks`/`run_statusline`、`identify_session_blocks` |
| `crates/ccusage-core` | `pricing.rs`（6181）、`output.rs`（975）、`summary.rs`（759）、`cost.rs`（664）、`date_utils.rs`（495）、`types.rs`（219）、`model_aliases.rs`（150）、`utils.rs`（105） | `PricingMap::{load_embedded,load_with_overrides,find,find_at}`、`calculate_cost*`、`summarize_by_key`、`print_usage_table` |
| `crates/ccusage-cli` | `types.rs`（382） | `SharedArgs`、`CostMode`、`SortOrder`、`WeekDay` |
| `crates/ccusage-cli-parser` | `parser.rs`（1247）、`help_codegen.rs`（378）、`arg_parser.rs`（104） | `parse()`、分层帮助生成 |
| `crates/ccusage-config` | `config.rs`（1404）、`config_schema.rs`（1639） | `load()`、配置合并、schema 生成 |
| `crates/ccusage-terminal` | `table.rs`（1337）、`width.rs`（194） | `SimpleTable`、终端宽度 |
| `crates/ccusage-adapter-all` | `loader.rs`（1637）、`report.rs`（620）、`types.rs`（208） | `run()`、18 家并行装载与统一报告 |
| `adapters/common` | `jsonl.rs`（410）、`lib.rs`（169）、`report.rs`（384） | `collect_files_with_extension`、`read_files_parallel`、`chunk_file_indexes_by_size` |

### B.2 适配器（`rust/adapters/<agent>`）

| agent | 主要文件（行数） | 存储形态 |
| --- | --- | --- |
| claude | `lib.rs`（1582）、`daily.rs`（1182）、`paths.rs`（555） | JSONL（`projects/**/*.jsonl`） |
| codex | `loader.rs`（2682）、`aggregate.rs`（1853）、`parser.rs`（1586）、`report.rs`（1107）、`lib.rs`（1106）、`types.rs`（658）、`paths.rs`（431）、`replay.rs`（429）、`speed.rs`（245） | JSONL（`sessions/`、`archived_sessions/`） |
| opencode | `loader.rs`（2922）、`parser.rs`（772）、`paths.rs`（223）、`report.rs`（223） | SQLite（含 JSON 迁移） |
| copilot | `loader.rs`（1990）、`parser.rs`（1095）、`paths.rs`（176） | JSONL（OTel 导出） |
| antigravity | `parser.rs`（1758）、`loader.rs`（907）、`paths.rs`（174） | SQLite（protobuf blob） |
| pi | `loader.rs`（1356）、`parser.rs`（1216）、`paths.rs`（82） | JSONL |
| grok | `parser.rs`（1165）、`loader.rs`（246）、`paths.rs`（193） | JSONL |
| openclaw | `parser.rs`（568）、`sqlite.rs`（381）、`loader.rs`（264） | SQLite + JSONL |
| zcode | `loader.rs`（545）、`parser.rs`（428）、`report.rs`（293）、`paths.rs`（214） | SQLite |
| kimi | `parser.rs`（523）、`loader.rs`（128）、`paths.rs`（115） | JSONL |
| codebuff | `parser.rs`（478）、`loader.rs`（217） | JSON |
| hermes | `parser.rs`（461）、`loader.rs`（199） | SQLite/JSON |
| qwen | `parser.rs`（413）、`paths.rs`（78） | JSONL |
| droid | `parser.rs`（404）、`loader.rs`（267） | JSONL |
| kilo | `parser.rs`（393）、`loader.rs`（204） | SQLite |
| goose | `parser.rs`（232）、`loader.rs`（211） | SQLite |
| gemini | `parser.rs`（596）、`paths.rs`（90） | JSON |
| amp | `parser.rs`（554）、`loader.rs`（38） | JSON（`threads/*.json`） |

> 逐家细节（根路径、环境变量、关键字段、去重与归一规则）见 §6；上表只作索引。
