# REVIEW-R4：S5「其余 16 个 adapter 矩阵」与 S6「配置 / CLI / 输出 / TS 版本对比」对抗式复核

- 被复核分片：`docs/research/ccusage-draft/S5-other-adapters.md`、`docs/research/ccusage-draft/S6-config-output-history.md`
- 样本 A：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`（HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31）
- 样本 B：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage-v19`（tag `v19.0.3`）
- 方法：只读静态核验（`read` / 内置 grep 工具，因 `rg` 不在 PATH）。未运行样本任何二进制，未修改样本；行号均为本次实测。
- 目标：证伪。凡与断言不符记「错误」，凡遗漏相反/更复杂证据记「不完整」。

## 结论

两份分片整体可用：抽样 43 条断言中 38 条与源码逐字一致（含 14 条高风险项 M1–M14 全部覆盖），**未发现影响主结论的颠覆性错误**；但存在 1 条硬性计数错误（S5「14 家调用 `apply_total_token_fallback`」实际为 12 家 / 14 处调用）、1 条实质性描述偏差（S6 把 `last_token_usage` 归为 v20 新领域，v19 已实现优先使用）与 5 条行号/表述不完整项。修完 6 条即可定稿。

## 抽样核验清单

| 编号 | 分片位置 | 断言摘要 | 源码位置 | 结果 | 源码摘录（≤3 行） |
| --- | --- | --- | --- | --- | --- |
| R01 | S5 §1 表 / S6 §5 表 | M1：各家环境变量名逐个核对（`AMP_DATA_DIR`/`ANTIGRAVITY_DATA_DIR`/`COPILOT_OTEL_FILE_EXPORTER_PATH`/`COPILOT_HOME`/`GOOSE_PATH_ROOT`/`ZCODE_HOME`/`GROK_HOME`/`OPENCLAW_DIR`/`KIMI_DATA_DIR`/`PI_AGENT_DIR`/`QWEN_DATA_DIR`/`HERMES_HOME`/`KILO_DATA_DIR`/`CODEBUFF_DATA_DIR`/`DROID_SESSIONS_DIR`/`GEMINI_DATA_DIR`/`OPENCODE_DATA_DIR`/`XDG_DATA_HOME`） | `adapters/*/src/paths.rs`（18 个 `_ENV` 常量，grep 全量） | 一致 | `const GOOSE_PATH_ROOT_ENV: &str = "GOOSE_PATH_ROOT";`（goose:5）／`ZCODE_HOME_ENV: &str = "ZCODE_HOME"`（zcode:9）／`COPILOT_OTEL_FILE_EXPORTER_PATH_ENV: &str = "COPILOT_OTEL_FILE_EXPORTER_PATH"`（copilot:5） |
| R02 | S5 §3.15 | copilot：`COPILOT_HOME` 优先否则 `~/.copilot`；exporter 单文件必须 `is_file()` | `copilot/src/paths.rs:56-62、76-83` | 一致 | `env::var(COPILOT_HOME_ENV).ok()...or_else(|| home_dir().map(|h| h.join(".copilot")))`；`path.is_file().then_some(path)` |
| R03 | S5 §0、§4.4 | M2：SQLite 适配器 7 家（opencode/openclaw/zcode/kilo/goose/hermes/antigravity），全 `with_read_only` | grep `with_read_only` → 正好 7 处 | 一致 | `hermes:43`、`antigravity:146`、`goose:75`、`opencode:288`、`openclaw:89`、`zcode:91`、`kilo:52`，均为 `sqlite::OpenFlags::new().with_read_only()` |
| R04 | S5 §4.4 | M2：`sqlite = { version = "0.37.4", default-features = false, features = ["bundled","linkage"] }`，各 adapter `sqlite.workspace = true`；无 rusqlite | `rust/Cargo.toml:65`；`zcode/Cargo.toml:12`、`antigravity/Cargo.toml:12` | 一致 | `rust/Cargo.toml:65: sqlite = { version = "0.37.4", default-features = false, features = ["bundled", "linkage"] }`；grep `rusqlite` 全仓 0 命中 |
| R05 | S5 §3.14 | M3：antigravity 表 `gen_metadata(idx,data)`、`steps(idx,metadata)`、`trajectory_metadata_blob(data)`（按 rowid） | `antigravity/src/parser.rs:325、366-371、414-418` | 一致 | `"SELECT idx, data FROM gen_metadata ORDER BY idx ASC"`（325）／`"SELECT idx, metadata FROM steps \"`（371）／`"SELECT data FROM trajectory_metadata_blob ORDER BY rowid ASC"`（418） |
| R06 | S5 §0、§3.14 | 只读 + `BEGIN DEFERRED TRANSACTION` + `PRAGMA page_count` 判空 | `antigravity/src/parser.rs:146、154、291` | 一致 | `open_with_flags(path, sqlite::OpenFlags::new().with_read_only())`／`execute("BEGIN DEFERRED TRANSACTION")`／`prepare("PRAGMA page_count")` |
| R07 | S5 §0 表 | 16 家入口全部包 `track_usage_load` | grep `track_usage_load` → 16 家命中 | 一致 | `amp/loader.rs:9`、`qwen/loader.rs:6`、`grok/loader.rs:13`、`zcode/loader.rs:51`、`antigravity/loader.rs:12` 等；`progress.rs:290 pub fn track_usage_load` |
| R08 | S5 §0 表 | 15 家用 `common::read_files_parallel`，**antigravity 是唯一顺序 for 循环** | grep `read_files_parallel(` 调用点 | 一致 | 调用点 15 家：`amp:21`、`codebuff:29`、`copilot:49`、`droid:29`、`gemini:25`、`goose:42`、`hermes:24`、`kilo:26`、`kimi:24`、`opencode:167`、`openclaw:51`、`pi:114`、`qwen:74`、`zcode:61`、`grok:25`；antigravity 无 |
| R09 | S5 §0 表 | **「14 家调用 `apply_total_token_fallback`」** | grep `apply_total_token_fallback(` | **错误：12 家 / 14 处** | 命中 14 处：`amp:158,261`、`kimi:208,255`（各 2 处）＋ `codebuff:255`、`zcode:58`、`gemini:338`、`qwen:145`、`pi:348`、`kilo:99`、`openclaw:295`、`opencode:193`、`copilot:452`、`droid:109`；去重后 12 家 |
| R10 | S5 §2.1 | `jsonl.rs` 七个函数行号：`records:47`、`lenient_u64:75`、`lenient_i64:95`、`lenient_f64:112`、`lenient_object:131`、`lenient_array:155`、`lenient_vec:180`、`non_empty_string:196` | `common/src/jsonl.rs` | 一致 | 7 个 `pub fn` 行号逐个命中（见 75/95/112/131/155/180/196）；`records` 在 47，`serde_json::from_slice::<T>(line).ok()` 在 60 |
| R11 | S5 §2.2 | `common/src/lib.rs`：`collect_usage_files:14`、`collect_files_with_extension:18`（read_dir 失败 return）、`filter_loaded_entries_by_date:36`、`chunk_file_indexes_by_size:49`（`max(1)` 在 53）、`read_files_parallel:84`（回填 `results[index]` 在 119） | `common/src/lib.rs` | 一致 | `let Ok(entries) = fs::read_dir(dir) else { return; };`（19-21）／`let chunk_count = chunk_count.max(1);`（53）／`results[index] = Some(value);`（119） |
| R12 | S5 §2.3 | `apply_total_token_fallback`（utils.rs:22）语义：分项和 < total 先补 output，output 非 0 进 extra；`total_usage_tokens`（:14） | `ccusage-core/src/utils.rs` | 一致 | `let missing_tokens = total_tokens.saturating_sub(known_tokens);`／`if usage.output_tokens == 0 { usage.output_tokens = missing_tokens; } else { extra_total_tokens = ... }` |
| R13 | S5 §2.3 | `TokenUsageRaw:29`、`cache_creation_token_count:42`、`TokenCounts:69`、`add_usage:78`（saturating）、`total:89`（含 extra）、`LoadedEntry:125` | `ccusage-core/src/types.rs` | 不完整 | 全部行号命中；但 `LoadedEntry` 字段清单漏 `usage_limit_reset_time`（`types.rs:137`） |
| R14 | S5 §3.5 | goose：静态 SQL 常量 + 只读 + 失败仅 debug 返回空 + 去重键 `"{db_path}:{session_id}"` | `goose/src/loader.rs:11-26、74-89、58` | 一致 | `const GOOSE_SESSION_QUERY: &str = r#"SELECT id, model_config_json, ... FROM sessions WHERE model_config_json IS NOT NULL AND TRIM(model_config_json) != ''"#;`／`let key = format!("{}:{}", db_path.display(), entry.session_id);` |
| R15 | S5 §3.13 | zcode：只读 + `PRAGMA busy_timeout = 5000` + `PRAGMA query_only = ON`，任一失败返回空；SQL 用 `format!` 拼接 | `zcode/src/loader.rs:91、100、107、132-160` | 一致 | `connection.execute("PRAGMA busy_timeout = 5000")`（100）／`connection.execute("PRAGMA query_only = ON")`（107）／`let cache_creation = if schema.has_cache_creation { "m.cache_creation_input_tokens" } else { "0" };` |
| R16 | S5 §3.16 | grok：`split_input_tokens` 做 `input − cache_read − cache_creation`，注释给出「cachedRead 是 input 子集、与 logs/unified.jsonl 对齐」 | `grok/src/parser.rs:163-174` | 一致 | `let (uncached, cache_read) = split_tokens(input, cached_read);`／`let cache_creation = cache_creation.min(uncached);`／`(uncached - cache_creation, cache_read, cache_creation)` |
| R17 | S5 §3.8 | kimi：新格式仅收 `usageScope == "turn"`（session 级是累计总量须丢弃）；桶来自 `usage.inputOther/output/inputCacheRead/inputCacheCreation`；prefilter 两个标记 | `kimi/src/parser.rs:126、195-207` | 一致 | `// Only aggregate turn-level records; session records are cumulative totals.`（195）／`if line.usage_scope.as_deref() != Some("turn") { return None; }`（196）／`LinePrefilter::any(&[br#""token_usage""#, br#""usage.record""#])`（126） |
| R18 | S5 §3.1 | amp：loader 无去重键，顺序 append 后按 timestamp 排序；`AMP_DATA_DIR` 一旦设置直接返回不回落 | `amp/src/loader.rs:32-36`、`amp/src/paths.rs:21` | 一致 | `for file_entries in per_file { entries.extend(file_entries); }`（32-34）／`entries.sort_by_key(|entry| entry.timestamp);`（36）／`return Ok(paths);`（21） |
| R19 | S5 §3.9 | openclaw：`entry_id = migration_id + ":" + cost`；migration_id 为 agent+session+timestamp+model+四桶+extra 九段指纹（cost 不入指纹） | `openclaw/src/parser.rs:469-491` | 一致 | `format!("{}:{}", migration_id(entry), entry.cost)`（470）／九元素 `["openclaw", session_id, timestamp, model, input, output, cc, cr, extra].join(":")`（479-490）／`// SQLite can carry a corrected provider-billed cost, so cost must not prevent ...`（475-476） |
| R20 | S5 §3.11 | pi：排除 `subagent-artifacts/` 派生转录；`read_files_parallel` + 顺序 first-wins | `pi/src/loader.rs:103-114` | 一致 | `files.retain(|file| !is_subagent_artifact_transcript(file));`（110）／`// ... (no session header, no entry ids), so neither the replay suppression below nor entry-id dedup can collapse them.`（106-107） |
| R21 | S5 §3.12、§4.1 | qwen：`loader.rs` 只有 9 行直接转发；实现体就叫 `load_entries`（无 `_inner`）；签名 `(shared)`；pricing 按 mode 内部决定 | `qwen/src/loader.rs:5-9`、`qwen/src/parser.rs:59-68` | 一致 | `pub fn load_entries(shared: &SharedArgs) -> Result<Vec<LoadedEntry>> { track_usage_load(... parser::load_entries(shared)) }`（5-8）／`let pricing = if shared.mode == CostMode::Display { None } else { Some(...) };`（60-68）；文件实测 9 行 |
| R22 | S5 §3.10 | opencode：`DateWindow::from_shared`（:121）、`has_source`（:199）、`time_created_looks_like_millis`（:1108）、`prepare_message_query`（:1069）、fork 回退注释（:429-432） | `opencode/src/loader.rs` | 不完整 | 上述行号全部命中；但 `prepare_session_message_query` 定义在 `:754`，分片 §4.4 写 `:752` |
| R23 | S5 §2.4 | 表驱动 18 个 `AgentLoadSpec`（`loader.rs:116-358`）、Claude/Codex 占 index 0/1（117-128）、`load_summary_agent_rows:555`、detected 语义 `!entries.is_empty()`（:563）、opencode 补 `has_data()`（:145）、`paths_overlap`（:551-553，重叠报错 :503-553） | `crates/ccusage-adapter-all/src/loader.rs` | 一致 | `let mut specs = vec![AgentLoadSpec { index: 0, agent: BUILT_IN_AGENT_NAMES[0], ... }]`（116-122）／`let detected = !entries.is_empty();`（563）／`left == right || left.starts_with(right) || right.starts_with(left)`（552） |
| R24 | S5 §3.6 | hermes：SQL 内联静态串（列清单一致）、只读、失败仅 debug 返回空、session 首次胜出 | `hermes/src/loader.rs:42-82`、`:30-35` | 一致 | `connection.prepare("SELECT id, model, billing_provider, started_at, message_count, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, estimated_cost_usd, actual_cost_usd FROM sessions WHERE model IS NOT NULL AND TRIM(model) != ''")`（54-72）／`if !seen_sessions.insert(entry.session_id.clone()) { continue; }`（31-33） |
| R25 | S5 §4.3 | 成本函数/缺价函数行号：`calculate_gemini_cost:496`、`calculate_goose_cost:175`、`calculate_kilo_cost:186`、`calculate_antigravity_cost:1023`、`calculate_open_code_cost:351`、`calculate_droid_cost:145`、`calculate_hermes_cost:180`、`calculate_qwen_cost:212`、`calculate_grok_cost_at:468`、`calculate_kimi_cost:372`、`calculate_codebuff_cost:392`、`calculate_store_cost:609`；缺价 `gemini:523`、`goose:210`、`kilo:225`、`antigravity:1062`、`hermes:205`、`qwen:236`、`grok:516`、`kimi:398`、`open_code:379`、`store:665` | `adapters/*/src/parser.rs` | 一致 | 上文 22 处行号逐个命中（grep `fn calculate_*_cost|fn missing_*_pricing`） |
| R26 | S5 §3.3、§3.5 | droid：`DROID_SESSIONS_DIR` + 默认 `~/.factory/sessions` + 只留 `*.settings.json`；goose 三条默认候选与 `is_file()` 过滤 | `droid/src/paths.rs:5、23、33、14-19`；`goose/src/paths.rs:5、9、29、39-46` | 一致 | `vec![home.join(".factory").join("sessions")]`（droid:33）／`~/.local/share/goose/sessions`、`Library/Application Support/goose/sessions`、`.local/share/Block/goose/sessions`（goose:40-45） |
| R27 | S6 §1.2 | M4：查找路径 `$CWD/.ccusage/ccusage.json` → `claude_config_dirs()/ccusage.json`（`CLAUDE_CONFIG_DIR` 逗号列表，否则 `~/.config/claude`、`~/.claude`），只取第一个可解析 object，不合并 | `ccusage-config/src/config.rs:258-269、271-283、246-256` | 一致 | `paths.push(cwd.join(".ccusage").join("ccusage.json"));`（261）／`let Some(home)... vec![home.join(".config").join("claude"), home.join(".claude")]`（281）／`.find(|value| value.as_object().is_some())`（255） |
| R28 | S6 §1.2 | M5：没有 `CCUSAGE_CONFIG`；源码 env 读取只有 `CCUSAGE_VERSION`/`CCUSAGE_PRICING_JSON_PATH`/`CCUSAGE_MODEL_ALIASES` | grep `CCUSAGE_[A-Z_]+`（`*.rs`：9 处命中、3 个变量名） | 一致（但「全仓」表述不准确，见 E1） | `const MODEL_ALIASES_ENV: &str = "CCUSAGE_MODEL_ALIASES";`／`const VERSION_ENV: &str = "CCUSAGE_VERSION";`／`const PRICING_JSON_PATH_ENV: &str = "CCUSAGE_PRICING_JSON_PATH";` |
| R29 | S6 §1.4 | M6：`option_maps()` 顺序 = 根 defaults → `commands.<raw>` → `commands.<report>`（仅带 agent）→ `commands."<agent>:<report>"` → `<agent>.defaults` → `<agent>.commands.<report>` | `config.rs:92-132` | 一致 | `if let Some(defaults) = object_at(root, "defaults") { maps.push(defaults); }`（97-99）→ `object_at(commands, &self.command.raw)`（101-103）→ `object_at(commands, &self.command.report)`（104-107）→ `format!("{agent}:{}", report)`（109-113）→ `<agent>.defaults`（122-124）→ `<agent>.commands.<report>`（125-129） |
| R30 | S6 §3 | M7：`wants_json = json \|\| jq.is_some()`（不互斥）；`--jq` 调外部 jq 二进制（stdin 写 JSON、stdout inherit、失败报 `jq failed`）；`--no-cost` 在序列化前剥离 | `ccusage-core/src/output.rs:14-16、158-187、564-581` | 一致 | `shared.json \|\| shared.jq.is_some()`（15）／`std::process::Command::new("jq").arg(filter)...spawn()`（163-168）／`if no_cost { strip_cost_json(&mut value); }`（159-161） |
| R31 | S6 §3 | M8：`--last` 三条（限日/周/月、禁 since/until、禁 sections）；since>until（statusline 豁免）；`session --id` 禁 sections/by-agent | `parser.rs:1102-1118、1123-1132、444-461` | 一致 | `"The --last option is only available for the daily, weekly, and monthly reports."`（1107）／`if matches!(command, Some(Command::Statusline(_))) { return None; }`（1124-1126）／`"The --sections and --by-agent options cannot be used with session --id."`（457） |
| R32 | S6 §6.1、§6.2 | M9：URL 白名单只认 LiteLLM 与 models.dev；10s 超时；64 MiB 上限；`$XDG_CACHE_HOME/ccusage/http-cache`（XDG 必须绝对路径，否则 `~/.cache`）；`if-none-match` + 304 重校验；原子 rename | `ccusage-core/src/pricing.rs:55-57、70-78`；`ccusage/src/http.rs:11-14、37-52、75-99、139-157` | 一致 | `LITELLM_PRICING_URL`/`MODELS_DEV_API_URL`（55-57）／`match url { LITELLM_PRICING_URL => ..., MODELS_DEV_API_URL => ..., _ => None }`（73-77）／`.filter(\|path\| path.is_absolute())`（46）／`timeout_global(Some(Duration::from_secs(PRICING_FETCH_TIMEOUT_SECONDS)))`（76） |
| R33 | S6 §5 | M10：`CCUSAGE_OFFLINE` 与 `CCUSAGE_TIMEZONE` 在两版源码均无实现，只有文档 | v20 `*.rs` 0 命中；v19 `apps/`+`packages/` 0 命中；文档 `docs/guide/environment-variables.md:124/129/209`（v20）、`:96/101/189`（v19）、`docs/guide/configuration.md:38`（两版同） | 一致 | v19 实现侧仅 `CCUSAGE_BUN_AUTO_RUN`、`CCUSAGE_JSONL_WORKER_THREADS`、`CCUSAGE_JSONL_READ_CONCURRENCY`；v20 实现侧仅 3 个 |
| R34 | S6 §7.3 | M11：v19 `createUniqueHash` = `messageId` 或 `messageId:requestId`，作用域为文件内 `processedEntries`；替换规则 token 大者或带 `speed` 者 | `ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1194-1202、1629-1638、2075-2123` | 一致 | `return requestId == null ? messageId : \`${messageId}:${requestId}\`;`（1201）／`return candidate.hasSpeed && !existing.hasSpeed;`（1637）／`existingEntryIndex = processedEntries[uniqueHash];`（2091） |
| R35 | S6 §7.5 | M11：v19 blocks 结构（整点对齐、双条件切块、gap 块、活跃判定）与 v20 一致；常量 5 小时 | `session-blocks.ts:7、45-47、135、146、157-162、202、247、373` | 一致 | `export const DEFAULT_SESSION_DURATION_HOURS = 5;`（7）／`Floors a timestamp to the UTC hour boundary used as a billing-block start.`（40）／`if (timeSinceBlockStart > sessionDurationMs \|\| timeSinceLastEntry > sessionDurationMs)`（146）／`nowMs - actualEndTimeMs < sessionDurationMs && nowMs < endTime.getTime()`（202） |
| R36 | S6 §7.6 | M12：v19 价格仅 LiteLLM 单源 + 进程内 `cachedPricing`/单飞 promise + 失败回落本地 + 双向 includes 最短长度差模糊匹配；v20 双源 + 磁盘条件请求 | `pricing.ts:4-5、161-178、227-239、272-288`；`pricing.rs:55-57` | 一致 | `LITELLM_PRICING_URL`（4-5）／`if (this.cachedPricing != null) { return Result.succeed(this.cachedPricing); }`（228-230）／`if (!comparison.includes(lower) && !lower.includes(comparison)) { continue; }`（276） |
| R37 | S6 §2.3 | M13：`session --id` 分流到 Claude 专用 `Command::Session` | `parser.rs:449-461` | 一致 | `"-i" \| "--id" => args.id = Some(parser.value_for("--id")?)`（449）／`if args.id.is_some() { ... return Ok(Command::Session(args)); }`（454-461） |
| R38 | S6 §2.4 | M14：帮助文本构建期 codegen（`cli-help.json`+`cli-commands.json` → `OUT_DIR/cli-help.rs`）；`include!` 引入；`cli-help.json` 无 `jq` 条目 | `ccusage-cli-parser/build.rs:5-29`、`help_codegen.rs:5-17`、`help.rs:13` | 一致 | `#[path = "src/help_codegen.rs"] mod help_codegen;`（build.rs:5）／`fs::write(out_dir_path("cli-help.rs"), output)`（28）／`include!(concat!(env!("OUT_DIR"), "/cli-help.rs"));`（help.rs:13）；grep `jq` 在 `src/*.json` 0 命中 |
| R39 | S6 §2.2 | 共享选项表 17 行行号（745-775 连续）与文案 | `parser.rs:743-779` | 一致 | `"-s" \| "--since" => ...`（745）／`"--last" => shared.last = ...`（751）／`"-q" \| "--jq" => shared.jq = ...`（771）／`flag => return Err(format!("Unknown option '{flag}'"))`（776） |
| R40 | S6 §7.4 | v19 Codex 只做「累计 − 上一条累计」，`last_token_usage` 是 v20 新领域 | `adapter/codex/parser.ts:206-209`、`types.ts:14` | **不完整** | `let raw = parsed.lastUsage; if (raw == null && parsed.totalUsage != null) { raw = subtractRawUsage(parsed.totalUsage, previousTotals); }` —— v19 优先用 `last_token_usage`，累计差分只是回退（`lastUsage` 字段见 `types.ts:14`，用例见 `index.ts:207/274/323`） |
| R41 | S6 §2.4 | `find_help_page` 比对 `page.path` 与 `command_tokens(args)`（`help.rs:55-68`） | `help.rs:55-68` | 不完整 | 实际 `find_help_page` 定义在 59-68；55-57 是 `help_text_for_tokens`（`find_help_page(tokens).map_or_else(root_help_text, render_help_page)`） |
| R42 | S6 §1.3、§1.7、§4.1、§4.4、§6.3 | `CcusageConfig` 18 段 + camelCase（13-59，14 行 rename_all）；根 `commands` 六报告（61-70）；默认值（`types.rs:62-71/221-240`、`core/lib.rs:59-61`）；JSON 键名与空值省略（`output.rs:27-56/58-77/79-121/112-115/126-145/147-156`）；progress 判定（`progress.rs:35-37/94-96`）；版本注入与一致性测试（`ccusage/build.rs:5-18`、`main.rs:125-135`） | 各列 | 一致 | `#[serde(rename_all = "camelCase")]`（config_schema.rs:14）／`if credits > 0.0 { value["credits"] = json!(credits); }`（output.rs:113-115）／`!json && output_is_tty`（progress.rs:36）／`println!("cargo:rustc-env={VERSION_ENV}={version}")`（build.rs:18） |
| R43 | S6 §7.1、§7.2 | v19 侧文件行数（cli.ts 234、shared.ts 735、data-loader.ts 7004、table.ts 1713）、v20 侧 `config-schema.json` 14415、`parser.rs` 1247、`arg_parser.rs` 104；v19 解析框架 gunshi+valibot、`YYYYMMDD` 单格式、无 `--last` / `--jq` | 实测行数 + `shared-args.ts:1-4、19-30` | 一致 | 行数逐个相符；`import type { Args } from 'gunshi'; ... import * as v from 'valibot';`（shared-args.ts:1-3）／`since: { type: 'custom', short: 's', ... parse: parseDateArg }`（19-24）；v19 全仓无 `--last`/`--jq` |

## 必须修正

- [E1] S6 §1.2：「全仓 grep `CCUSAGE_` 只命中 `CCUSAGE_VERSION`、`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_MODEL_ALIASES` 三个」。实际全仓（含文档/nix）还会命中：`docs/guide/environment-variables.md:124/129/209`（`CCUSAGE_OFFLINE`）、`docs/guide/configuration.md:38`（`CCUSAGE_TIMEZONE`）、`docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24、51`（`CCUSAGE_GROK_PATH`，文档自身声明该变量处理已移除）。应改为「源码中 `env::var`/`env!` 读到的 `CCUSAGE_*` 只有三个」。
- [E2] S5 §0 表：「14 家调用 `apply_total_token_fallback(usage, extra, total)`」。实际为 **12 家 adapter**（`amp`、`codebuff`、`copilot`、`droid`、`gemini`、`kilo`、`kimi`、`openclaw`、`opencode`、`pi`、`qwen`、`zcode`），共 14 处调用点（`amp/src/parser.rs:158,261`、`kimi/src/parser.rs:208,255` 各 2 处）。改写为「14 处调用，覆盖 12 家」。
- [E3] S6 §7.4：把「compaction 的 `last_token_usage`」整体归入 v20 新领域不准确。v19 已有 `lastUsage` 字段（`ccusage-v19/apps/ccusage/src/adapter/codex/types.ts:14`）并在解析时**优先**使用（`adapter/codex/parser.ts:206-209`：`raw = parsed.lastUsage`，仅当为 `null` 且存在 `totalUsage` 时才走 `subtractRawUsage`）；`index.ts:207/274/323` 已有 `last_token_usage` 与 `total_token_usage` 并存的用例。v20 的真实增量是 replay/rewrite 状态机与 cache-write 扣减，不是「首次处理 last_token_usage」。
- [E4] S6 §2.4：`find_help_page` 的 `help.rs:55-68` 应写 `:59-68`（55-57 是 `help_text_for_tokens`）。
- [E5] S5 §4.4 表：opencode 的 `prepare_session_message_query` 定义行号 `:752` 实际为 `:754`（`opencode/src/loader.rs`）。
- [E6] S5 §2.3 表：`LoadedEntry`（`ccusage-core/src/types.rs:125`）字段清单漏 `usage_limit_reset_time`（`types.rs:137`）。

## 建议补充

- S5 §3.5 / S6 §5：`GOOSE_PATH_ROOT` 的语义不是「指向 sessions.db」——显式设置时实际拼的是 `<root>/data/sessions/sessions.db`（`goose/src/paths.rs:14-19`），且空字符串会回落默认（:11-12）。两处表格都只写了「标准 Goose 根」。
- S6 §5：`_ENV` 常量计数与事实不符。adapters 目录下 18 个、crates 下 3 个（`MODEL_ALIASES_ENV`/`VERSION_ENV`/`PRICING_JSON_PATH_ENV`），合计 21 个常量，另有 13 个直接字面量（`HOME`/`USERPROFILE`/`HOMEDRIVE`/`HOMEPATH`/`LOG_LEVEL`/`NO_COLOR`/`FORCE_COLOR`/`COLUMNS`/`XDG_CACHE_HOME`/`CLAUDE_CONFIG_DIR`/`XDG_CONFIG_HOME`/`CODEX_HOME`/`OUT_DIR`），表内实际列出 33 个变量。建议把「共 20 个变量常量」改成准确计数，或改为「按用途分为 33 个变量」。
- S6 §5 的「文档失效项」清单建议补上 `CCUSAGE_GROK_PATH`（v20 设计文档已声明移除，属同类跨版本遗留）。
- S5 §1 表 copilot 行可补一句：`session-state/<dir>/events.jsonl` 只取目录下一层，嵌套目录不递归（`copilot/src/paths.rs:33-45`，测试 `discovers_session_state_and_otel_files_without_nested_session_files`）。
- S5 §4.2 表 gemini 行的「归一/兜底」引用的是 cached 相减逻辑（`:400`/`:406`），其 `apply_total_token_fallback` 实际在 `gemini/src/parser.rs:338`，建议补行号以免误读。
- 内置 grep 不可用时（本机 `rg` 不在 PATH）建议在分片方法段记一句取证工具，便于后续复现。

## 无法验证

- S6 §7.3 除 daily 外的其余去重应用点（`data-loader.ts:2136-2183`、`:2199-2264`、`:2322-2334`、`:2457-2469`、blocks 变体 `:2894-2913`）未逐个打开，仅验证了 2075-2123。
- S6 §7.6 的 `PricingMap::find`（`pricing.rs:1354-1460`）、`find_exact`（`:1484`）、`find_exact_with_fallback`（`:1501`）实现细节与本分片 §7.6 的「精确优先」结论未逐行复算。
- S5 §5 自陈未验证项（antigravity `event_to_loaded`、copilot `credit_gap_entries`/`should_emit_candidate`、pi `replay_usage_path` 尾部与 `PiReplayUsagePath` 消费点、opencode `is_fork_copy` 判定条件与 `prepare_*_query` 列裁剪、codebuff `dedup_key` 实现体、gemini 跨文件重复风险）本轮只核到函数定义行号，算法体未读。
- S6 §1.7「`blocks.recent` lookback 3 天用于 `ccusage/src/commands/mod.rs:573、659`」未核。
- S5 §1 表 gemini「单 JSON 与 JSONL」的扩展名分派（`gemini/src/loader.rs:26-29`）未核。
- S6 §7.5「v20 支持 `--session-length`、v19 的 blocks 命令另有自己的 token limit 解析（`ccusage-v19/apps/ccusage/src/commands/blocks.ts:106-114`）」未核。

## 我实际打开过的文件（file:line）

- `rust/adapters/amp/src/paths.rs:5,21,25`（`AMP_DATA_DIR`、设置即返回、home 报错）
- `rust/adapters/antigravity/src/paths.rs:7,21,61,66-74`（根解析、默认根过滤、收集排序）
- `rust/adapters/copilot/src/paths.rs:5-6,56-62,76-83`（变量名与 exporter 单文件）
- `rust/adapters/goose/src/loader.rs:11-26,74-89`（SQL 常量、只读、失败路径）
- `rust/adapters/zcode/src/loader.rs:91,100,107,132-160`（只读 + 两条 PRAGMA + 列名拼接）
- `rust/adapters/hermes/src/loader.rs:42-82`（只读 + 内联 SQL）
- `rust/adapters/kimi/src/parser.rs:126,195-207`（prefilter 与 turn 语义）
- `rust/adapters/grok/src/parser.rs:163-174`（`split_input_tokens` 与子集注释）
- `rust/adapters/openclaw/src/parser.rs:469-491`（entry_id / migration_id）
- `rust/adapters/pi/src/loader.rs:103-114`（subagent-artifacts 排除）
- `rust/adapters/qwen/src/loader.rs:1-9`、`qwen/src/parser.rs:59-68`（9 行转发、pricing 分支）
- `rust/adapters/opencode/src/loader.rs:121,199,306,430,754,1108`（窗口/检测/毫秒嗅探/fork 注释）
- `rust/adapters/droid/src/paths.rs:5,23,33`、`goose/src/paths.rs:5,9,29,39-46`
- `rust/adapters/common/src/lib.rs:14-126`、`common/src/jsonl.rs:47-196`
- `rust/crates/ccusage-core/src/utils.rs:5-38`、`types.rs:22-139`、`output.rs:14-25,27-156,158-187,385-404,410-462,552-581`
- `rust/crates/ccusage-core/src/progress.rs:35-37,87-96,290-295`、`home.rs:5-8`、`logger.rs:12`
- `rust/crates/ccusage-core/src/pricing.rs:51-126`、`rust/crates/ccusage/src/http.rs:11-14,25-52,61-175`
- `rust/crates/ccusage-config/src/config.rs:60-139,240-344,395-406,627`、`config_schema.rs:1-75`
- `rust/crates/ccusage-cli-parser/src/parser.rs:134-150,440-499,742-822,1085-1132`、`help.rs:1-70`、`build.rs:1-29`、`help_codegen.rs:1-60`
- `rust/crates/ccusage-cli/src/types.rs:30-79,210-254`、`rust/crates/ccusage/src/cli.rs:1-51`、`build.rs:1-19`、`main.rs:118-142`
- `rust/crates/ccusage-adapter-all/src/loader.rs:110-175,495-564`
- `rust/Cargo.toml:65`、`rust/adapters/{zcode,antigravity,hermes,goose,openclaw,opencode,kilo}/Cargo.toml`（`sqlite.workspace`）
- `ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1188-1217,1615-1654,2070-2124`
- `ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:88-147,195-224`、`adapter/codex/types.ts:14`
- `ccusage-v19/apps/ccusage/src/session-blocks.ts:1-50,125-204`
- `ccusage-v19/apps/ccusage/src/shared-args.ts:1-40`、`packages/internal/src/pricing.ts:1-40,155-184,175-264,265-294`
- `ccusage-v19/docs/guide/environment-variables.md:96,101,189`、`docs/guide/configuration.md:38`（v20 同路径 :124,:129,:209）
