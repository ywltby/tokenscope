# REVIEW-FULL-C2：§6 / §7 / §8 全文复核（L1080–L2747）

- 复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md` 的 L1080–L2747（§6 31 小节 / §7 53 小节 / §8 35 小节，共 1668 行）
- 只读样本：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage` HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`
- 实测工具：`rust\target\debug\ccusage.exe`（一律 `--offline`）；另独立解压 `target\debug\build\ccusage-core-30b61b361638d3b0\out\litellm-pricing.json.deflate` 取内嵌表真身。**本文件不含任何会话内容或具体用量数值**（运行结论只记布尔/字段名）。
- 方法：逐节通读全部 1668 行 → 对每节正文按行号回读源码 → 抽查 59 条断言（每条附源码摘录）→ 对第 8 轮的 4 条推演做「真键集 + 源码裁决语义」离线重放。

## 结论（一句话）

三章整体可采信（59 条抽查全部逐字对上，含 534 键内嵌表、DeepSeek 时间表、blocks 切块算法、14 处兜底调用、7 家 SQLite 只读等最硬证据），但**第 8 轮新改的 §8 §4.5(a) 有一处必须修正的实质错误**：`claude-3-5-haiku-20241022` 并非「模糊命中 `us.anthropic.…`」，而是**精确命中内嵌兜底表新增键**（`pricing.rs:2192`），该说法同时与 §8 §3.4 自述、§8 §4.1 的裁决规则自相矛盾；其余为 1 处表格不完整、3 处口径提示、4 处「未验证」状态标注滞后。

## 抽查清单

### §6（21 条）

| # | 稿中断言（要点） | 源码 / 实测证据（≤3 行） | 判定 |
| --- | --- | --- | --- |
| 1 | `amp` 根：`AMP_DATA_DIR` 逗号分隔、只收 `is_dir()`、设置后不回落，默认 `~/.local/share/amp` | `amp/src/paths.rs:10 env::var(AMP_DATA_DIR_ENV)`<br>`:21 return Ok(paths);`<br>`:26 home.join(".local/share/amp")` | ✅ 逐字符合 |
| 2 | `opencode` 根：`OPENCODE_DATA_DIR`（OsString）、默认 `$XDG_DATA_HOME/opencode`、XDG 须绝对路径、home 缺失报错 | `opencode/src/paths.rs:11 env::var_os(OPENCODE_DATA_DIR_ENV)`<br>`:31 .filter(\|path\| path.is_absolute())`<br>`:36 .ok_or_else(\|\| cli_error("home directory is not set"))` | ✅ |
| 3 | `copilot` 根：`COPILOT_HOME` 优先、否则 `~/.copilot`；另有单文件 `COPILOT_OTEL_FILE_EXPORTER_PATH` | `copilot/src/paths.rs:56 env::var(COPILOT_HOME_ENV)`<br>`:61 .or_else(\|\| crate::home::home_dir().map(\|home\| home.join(".copilot")))`<br>`:76 env::var(COPILOT_OTEL_FILE_EXPORTER_PATH_ENV)` | ✅ |
| 4 | `antigravity` 根：6 个默认根、`ANTIGRAVITY_DATA_DIR` 逗号分隔且**不做存在性过滤**、默认根须含 `conversations/` | `antigravity/src/paths.rs:9-16`（6 项常量）<br>`:21-30` env 直 collect，无 `is_dir` 过滤<br>`:61 if !custom_root && !root.join("conversations").is_dir() { continue; }` | ✅ |
| 5 | `pi` 根：`PI_AGENT_DIR`、默认 `~/.pi/agent/sessions`（须 is_dir）；排除 `subagent-artifacts/` | `pi/src/paths.rs:11,:19 home.join(".pi/agent/sessions")`<br>`pi/src/loader.rs:102 collect_files_with_extension(&path, "jsonl", …)`<br>`:110 files.retain(\|file\| !is_subagent_artifact_transcript(file))` | ✅ |
| 6 | `grok` 根：`GROK_HOME`（trim 非空 + is_dir）→ 默认 `~/.grok`；只认 `sessions/**/updates.jsonl` | `grok/src/paths.rs:19-23`；`:26-28 home.join(".grok")`<br>`:38-41 updates.retain(\|p\| p.file_name()… == Some("updates.jsonl"))` | ✅ |
| 7 | `kimi` 根：`KIMI_DATA_DIR`、默认 `~/.kimi` 与 `~/.kimi-code`；只认深度 3 或 5 的 `wire.jsonl` | `kimi/src/paths.rs:16 env::var(KIMI_DATA_DIR_ENV)`<br>`:31 for dir in [".kimi", ".kimi-code"]`<br>`:67-73 …filter(Component::Normal).count(), 3 \| 5` | ✅ |
| 8 | `openclaw` 根：`OPENCLAW_DIR`（custom_path 优先）、4 个默认根 | `openclaw/src/paths.rs:13 return existing_path_list(custom_path)`<br>`:15 env::var(OPENCLAW_DIR_ENV)`<br>`:23-28 [.openclaw, .clawdbot, .moltbot, .moldbot]` | ✅ |
| 9 | `codebuff` 根：`CODEBUFF_DATA_DIR`；3 频道目录 + `projects`（已是 projects 不重复拼） | `codebuff/src/paths.rs:23`；`:33-35 CHANNELS.iter().map(\|c\| home.join(".config").join(c))`<br>`:41-45 if root.file_name()… == "projects" { root } else { root.join("projects") }` | ✅ |
| 10 | `hermes` 根：`HERMES_HOME` 逗号分隔、默认 `~/.hermes`、固定 `state.db` 且须 `is_file()` | `hermes/src/paths.rs:8,:18`<br>`:23-24 .map(\|home\| home.join("state.db")).filter(\|p\| p.is_file())` | ✅ |
| 11 | `zcode` 根：`ZCODE_HOME`、默认 `~/.zcode`、库固定 `cli/db/db.sqlite`；home 缺失静默空 | `zcode/src/paths.rs:13,:24`<br>`:20-23 let Some(home) = home_dir() else { debug_log(…); return Ok(Vec::new()) }`<br>`:31 root.join(ZCODE_DB_RELATIVE_PATH)` | ✅ |
| 12 | `qwen` 根：`QWEN_DATA_DIR`、默认 `~/.qwen`；只认 `projects/<p>/chats/*.jsonl` | `qwen/src/paths.rs:12,:21`<br>`:53-59 matches!(parts, [project, chats, file] if chats=="chats" && file.ends_with(".jsonl"))` | ✅ |
| 13 | `droid` 根：`DROID_SESSIONS_DIR`、默认 `~/.factory/sessions`；只保留 `.settings.json` | `droid/src/paths.rs:23,:33 vec![home.join(".factory").join("sessions")]`<br>`:14-18 .is_some_and(\|name\| name.ends_with(".settings.json"))` | ✅ |
| 14 | `kilo` 根：`KILO_DATA_DIR`、默认 `~/.local/share/kilo`；库文件须 `is_file()` | `kilo/src/paths.rs:15,:30 home.join(".local").join("share").join("kilo")`<br>`:39-40 path.is_file().then_some(path)` | ✅ |
| 15 | `goose` 根：`GOOSE_PATH_ROOT`；三条默认候选；SQL 为静态字符串常量 | `goose/src/paths.rs:9,:39-46`（三候选均 `.join(GOOSE_DB_FILE_NAME)`）<br>`goose/src/loader.rs:11-26 GOOSE_SESSION_QUERY`（列清单与稿一致） | ✅ |
| 16 | `gemini` 根：`GEMINI_DATA_DIR`（设置后不回落）、默认 `~/.gemini/tmp`；同时收 `.json` 与 `.jsonl` 并排序去重 | `gemini/src/paths.rs:11,:22 return Ok(paths)`；`:26 home.join(".gemini").join("tmp")`<br>`:38-42 collect json/jsonl → files.sort(); files.dedup()` | ✅ |
| 17 | 7 家 SQLite 适配器全部用 `sqlite` crate、全部 `with_read_only()`；无 rusqlite | `openclaw/sqlite.rs:87`、`zcode/loader.rs:91`、`antigravity/parser.rs:146`、`opencode/loader.rs:288`、`kilo/loader.rs:52`、`goose/loader.rs:75`、`hermes/loader.rs:43`<br>`rust/Cargo.toml:65 sqlite = { version = "0.37.4", … }`；全仓 grep `rusqlite` 零命中 | ✅ 行号全对 |
| 18 | 16 家中 15 家用 `read_files_parallel`，`antigravity` 是唯一顺序 for 循环 | grep `read_files_parallel(`：adapters 下 16 处（15 家 + `claude`），`antigravity` 零命中 | ✅ |
| 19 | 缺字段兜底 **12 家 / 共 14 处**，amp 与 kimi 各 2 处 | grep `apply_total_token_fallback(`：`amp:158/:261`、`kimi:208/:255`，另 `zcode:58`/`kilo:99`/`codebuff:255`/`droid:109`/`qwen:145`/`gemini:338`/`opencode:193`/`copilot:452`/`openclaw:295`/`pi:348` | ✅ 数量与分布全对 |
| 20 | 16 家入口统一包 `track_usage_load`（进度 + 计时） | grep 命中 16 家 loaders（amp/grok/pi/zcode/droid/antigravity/gemini/codebuff/qwen/goose/kimi/opencode/hermes/openclaw/copilot/kilo）<br>`ccusage-core/src/progress.rs:290 pub fn track_usage_load<T, E>(` | ✅ |
| 21 | 自行「把 cached 从 input 里减掉」的有四家：grok / copilot / zcode / gemini | `grok/parser.rs:170-173 split_input_tokens`<br>`copilot/parser.rs:445 input.saturating_sub(cache_read.saturating_add(cache_creation))`<br>`zcode/parser.rs:47-50`；`gemini/parser.rs:400-404 + :406-416`（条件性） | ✅ |

### §7（20 条）

| # | 稿中断言（要点） | 源码 / 实测证据（≤3 行） | 判定 |
| --- | --- | --- | --- |
| 22 | 5h 块锚定首个活动时刻并按 **UTC 整点**取整；`since_last > 5h`（严格大于）断块 | `blocks.rs:75 if since_start > session_duration \|\| since_last > session_duration`<br>`:89 current_start = Some(floor_to_hour(entry.timestamp));`<br>`date_utils.rs:58-60 Self(self.0.div_euclid(MILLIS_PER_HOUR) * MILLIS_PER_HOUR)` | ✅ |
| 23 | gap 块仅在 `since_last > duration` 时插入 | `blocks.rs:82-87 if since_last > session_duration { blocks.push(create_gap_block(last_time, entry.timestamp, session_duration)); }` | ✅ |
| 24 | 常量 `BLOCKS_WARNING_THRESHOLD = 0.8`、`BLOCKS_COMPACT_WIDTH_THRESHOLD = 120` | `blocks.rs:18 const BLOCKS_WARNING_THRESHOLD: f64 = 0.8;`<br>`blocks.rs:20 const BLOCKS_COMPACT_WIDTH_THRESHOLD: usize = 120;` | ✅ |
| 25 | 块内 token 归属无跨块拆分；`create_block` 只 `add_usage`（extra 不计） | `blocks.rs:115-117 end/actual_end/is_active`<br>`:124 token_counts.add_usage(entry.data.message.usage); :125 cost += entry.cost;` | ✅ |
| 26 | `calculate_burn_rate`：单条 entry → None；indicator 用 (input+output) | `blocks.rs:567-576`（empty/gap → None、`duration_minutes <= 0.0 → None`）<br>`:577-582 tokens_per_minute / tokens_per_minute_for_indicator / cost_per_hour` | ✅ |
| 27 | `project_block_usage` 依赖 burn rate（`?` 传播），故无 projection 时也无 `tokenLimitStatus` | `blocks.rs:590 let burn = calculate_burn_rate(block)?;`<br>`:591-598 remaining_minutes / total_tokens / total_cost 保留 2 位`<br>`:603-605 parse_token_limit("max") → (max_tokens > 0).then_some(max_tokens)` | ✅ |
| 28 | 过滤顺序：identify → 日期 → 排序 → recent → active → 计算 max_tokens | `commands/mod.rs:268-288`（`:283-288` 取非 gap 非 active 块 `token_counts.total()` 的最大值） | ✅ 逐字符合 |
| 29 | `--last` 折算成 `since`：today 走命令时区、写回 `shared.since` | `cli/last_window.rs:18 let today = format_date(utc_now(), shared.timezone.as_deref());`<br>`:19-21 last_periods_since(…) else { Err("Could not resolve --last …") }`<br>`:22 shared.since = Some(since);` | ✅ |
| 30 | `--last` 单位映射；`session`/`blocks`/`statusline` 不支持；统一周起点 Monday | `cli/last_window.rs:29-35`（None/daily=Day、monthly=Month、weekly 用自身 `start_of_week`）<br>`:56 Some(Command::Session(_) \| Command::Blocks(_) \| Command::Statusline(_)) => None`<br>`:62 const UNIFIED_WEEK_START: WeekDay = WeekDay::Monday;` | ✅ |
| 31 | 会话 key 是 `(project_path, session_id)` 二元组 | `commands/mod.rs:187 FxHashMap::<(Arc<str>, Arc<str>), usize>`<br>`:189-192 let key = (Arc::clone(&entry.project_path), Arc::clone(&entry.session_id));` | ✅ |
| 32 | statusline 缓存路径 = 临时目录 `ccusage-semaphore/{session_id}.lock` | `commands/mod.rs:860-864 env::temp_dir().join("ccusage-semaphore").join(format!("{session_id}.lock"))` | ✅ |
| 33 | `SessionAccumulator`：严格 `>` 不覆盖同刻、输出 UTC RFC3339 毫秒、空组报 `empty session group` | `summary.rs:136-146 latest（is_none_or 严格 >）`；`:158-167 into_summary`（`:160 Err(cli_error("empty session group"))`、`:165-166 format_rfc3339_millis`） | ✅ |
| 34 | `UsageAccumulator`：extra 饱和加、credits/message_count 惰性、模型名走别名归一 | `summary.rs:54-57 extra_total_tokens.saturating_add(entry.extra_total_tokens)`<br>`:59-64 *self.credits.get_or_insert(0.0) += credits;`<br>`:65-66 crate::model_aliases::resolve_model_name(model)` | ✅ |
| 35 | `json_float` 分支条件（有限 + 整数 + i64 范围内 → 整数） | `output.rs:452-462 if value.is_finite() && value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 { json!(value as i64) }` | ✅ 逐字符合 |
| 36 | `unpricedModels`：按 `missingPricing` 收集、BTreeSet 去重排序、空则无键 | `output.rs:129-135 .filter(\|b\| b.missing_pricing).map(…).collect::<BTreeSet<_>>()`<br>`:141-144 if !models.is_empty() { totals["unpricedModels"] = json!(models); }` | ✅ |
| 37 | `format_utc_minute` 生产无调用点；`format_utc_second` 仅用于 `Block Started` | grep：定义 `date_utils.rs:340/:348`；使用点仅 `blocks.rs:475 println!("Block Started:   {}", format_utc_second(block.start_time));` 与 `main.rs:231-232` 测试 | ✅ |
| 38 | 实测（本轮）：`blocks --json` 数组默认**升序**（最旧在前），与代码一致 | 实跑 `blocks --json --offline`：`startTime` 严格升序、首元素最旧；`commands/mod.rs:270 sort_blocks(&mut blocks, &shared.order)`、默认 `SortOrder::Asc` | ✅ **闭环存疑 #1** |
| 39 | 实测（本轮）：`claude daily --json` 顶层 `daily`/`totals`，行键含 `totalCost`，明细无 `extraTotalTokens`、无价时不带 `missingPricing` | 实跑三态 + 解析键名：`daily,totals`；行键 `cacheCreationTokens,cacheReadTokens,date,inputTokens,modelBreakdowns,modelsUsed,outputTokens,totalCost,totalTokens`；明细键 `…,cost,modelName,…` | ✅ 字段名与 §7 §8.1 一致 |
| 40 | 实测（本轮）：`--mode` 下 auto == calculate（金额），display 的 `totalCost` 为 0 | 实跑 `--mode auto\|calculate\|display` 三态：auto 与 calculate 的 `totalCost`/`totalTokens` 相同；display `totalCost == 0` 且 token 不变（未记录任何数值） | ✅ |
| 41 | `parse_ts_timestamp` 只接受带时区后缀形态，无后缀即失败 | `date_utils.rs:113-117`（20/25 且 `bytes[19] ∈ Z+-`；24/29 且 `bytes[19]=='.'` → 毫秒取 `bytes[20..23]`）<br>`:118-125` 强制 `[4]-[7]-[10]T[13]:[16]:`<br>`:144 checked_sub_millis(timezone_offset * MILLIS_PER_MINUTE)` | ✅ |

### §8（18 条）

| # | 稿中断言（要点） | 源码 / 实测证据（≤3 行） | 判定 |
| --- | --- | --- | --- |
| 42 | 内嵌裁剪白名单：`claude-`/`anthropic.`/`anthropic/`/`us.[eu./global./jp./au.]anthropic.`/`gpt-`/`o3`/`o3-`/`o4-mini(-)`/`openai/`/`azure/`/`zai/`/`openrouter/openai/` | `build.rs:203-221`（逐条比对，**无 `apac.`**） | ✅ |
| 43 | 压缩形状字段映射 + `i`&`o` 同时存在才保留 + 内嵌体积断言 | `build.rs:168-198`（`input_cost_per_token→i`、…、`provider_specific_entry.fast/flex→fast/flex`）<br>`pricing.rs:5844-5851 assert!(json.len() < 200_000); assert!(!json.contains("vertex_ai/")); assert!(json.contains("claude-opus-4-6"));` | ✅ |
| 44 | 实测：构建产物解压 = **534 键**；`claude-*` 裸键 23；区域前缀 86 + `anthropic.` 29；azure 260 / gpt 108 / zai 16 | 解压 `litellm-pricing.json.deflate` 统计：534 / 23 / us25+eu22+global16+jp10+au13=86 / 29 / 260 / 108 / 16，与 §13.5 完全一致 | ✅ 与 §13.5 闭环 |
| 45 | 匹配链顺序：精确 → alias 精确 → fuzzy 门禁 → 本表 → 本表 alias → 在线 models.dev(alias) → 内嵌 models.dev(alias) | `pricing.rs:1405-1416`（`Fuzzy::Denied` 精确阶段，含 alias 二次尝试）<br>`:1420-1425 allows_fuzzy_lookup_with_fallbacks`<br>`:1426-1453` 四段 `or_else` 链 | ✅ 顺序逐字符合 |
| 46 | 精确第一跳是 `entries.get(model)` | `pricing.rs:1565-1567 self.entries.get(model).copied().or_else(\|\| pricing_alias(model)…)` | ✅ |
| 47 | tie-break：最长键优先，长度相同取**字典序较小者** | `pricing.rs:1610-1612 .max_by(\|(left,_),(right,_)\| left.len().cmp(&right.len()).then_with(\|\| right.cmp(left)))` | ✅（正是 M1 的判据） |
| 48 | `claude-sonnet-4-20250514`：模糊命中最长区域前缀 Bedrock 键 `global.…-v1:0`（长度 46），候选含 `anthropic.`/`us.`/`eu.`；`apac.` 不在内嵌表 | 重放：候选仅 4 键（anthropic.39 / eu.42 / us.42 / global.46），最长 global(46)；边界规则放行（后缀 `-v1:0` 非数字版本）<br>`pricing.rs:4933-4939, :4944-4950` 测试只断言「与 `anthropic.…` 的 input 相等」 | ✅ **成立** |
| 49 | `claude-opus-4-6-fast`：精确 miss → fuzzy 命中基础键 `claude-opus-4-6`；该拼写不在 models.dev 快照 | 重放：唯一候选 `claude-opus-4-6`（无更长候选）<br>grep `models-dev-pricing.json` 无 `claude-opus-4-6-fast`；`claude-opus-4-8-fast:7504`、`claude-opus-5-5-fast:7598`、`claude-opus-5-fast:7634` 存在 | ✅ |
| 50 | `gpt-5.3-codex`：内置兜底表有精确键、快照也有 → 精确命中；fast 倍率 2.0 | `pricing.rs:2388-2399 put_builtin_entry("gpt-5.3-codex", …)`<br>`models-dev-pricing.json:15868`（裸键）、`:5691 azure/…`、`:24474 openai/…`<br>`fast-multiplier-overrides.json:8 "gpt-5.3-codex": 2.0` | ✅ |
| 51 | `[pi] gpt-5.4`：查找链 override → find_at/find → display find_exact → raw find_at/find；默认 store 名 `pi`；无 override 退到 `gpt-5.4` | `pi/src/parser.rs:629-638 pricing_override_model`、`:685-695 store_pricing`、`:697-708 store_pricing_at`<br>`:226-231 Self::Default => "pi"`（loader 侧 `pi/src/loader.rs:74-79` 同）<br>`pricing.rs:2409-2430` 内置 `gpt-5.4`（fast/flex 走 overrides） | ✅ 逐字符合 |
| 52 | `exact_only`：三方法语义（contains / contains_any_spelling / id_spelled_by）+ 门禁跨数据源 | `pricing.rs:501-503`、`:507-512`、`:517-521`<br>`:1676-1681 requires_exact(model) \|\| … ⇒ Fuzzy::Denied`<br>`models-dev-pricing.json:7641 "exactOnly": true` | ✅ 行号全对 |
| 53 | `tiered_cost` 语义 + 整单切换 + 阈值 context 含缓存 + 1h = input×2 | `cost.rs:201-211`（0 早退；`above && tokens > threshold` → `threshold*base + (tokens-threshold)*above`）<br>`:143-167`（`context_tokens = input + cache_read + 5m + 1h`；`long_context = context_tokens > threshold` 全桶切档）<br>`:128 let cache_create_1h_cost = pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER;` | ✅ 逐字符合 |
| 54 | 缺价三态判定 + 两条 stderr WARN 文案（离线/在线） | `cost.rs:48-75`（`Display` 或 `Auto && cost_usd.is_some()` → None；`total_tokens == 0` → None；`find` 为 None 才报）<br>`output.rs:440-446 "WARN  Missing embedded pricing for {model}; cost excludes this model. …"` / `"WARN  Missing pricing for {model}; …"` | ✅ |
| 55 | DeepSeek V4：`CUTOFF = 1_786_896_000_000`、三段价（flash 0.14/0.28→0.22/0.66→0.44/1.32；pro 0.435/0.87→0.66/1.98→1.32/3.96）、高峰窗口按 UTC 且周末休、`[1,4)∪[6,10)` 半开 | `pricing.rs:252`、`:279-331`（三段价逐值比对一致）<br>`:333-347`（`days_since_epoch`/`rem_euclid(7)`、`!(1..=5).contains(&weekday)` → false、`(1..4).contains(&hour) \|\| (6..10).contains(&hour)`） | ✅ |
| 56 | 内置兜底默认 `or_insert`，**3 处**例外走无条件 `insert`：`gpt-5.1-codex`/`gpt-5.2-codex`/`gpt-5.2` | `pricing.rs:1980-1983 self.entries.entry(model).or_insert(pricing);`<br>`:2369-2370 self.entries.insert("gpt-5.1-codex".to_string(), gpt_5_1_pricing);`<br>`:2386-2387`、`:2400-2408`（另 `gpt-5.3-codex` 走 `put_builtin_entry`，不属这 3 处） | ✅ 三处定位精确 |
| 57 | fast/flex 倍率表逐键（fast：10 exact + 3 normalized_prefix；flex：20 键全 0.5、prefix 空） | `fast-multiplier-overrides.json:3-17`（含 `gpt-5.5:2.5`、`claude-opus-4-6:6.0`）<br>`flex-multiplier-overrides.json`（20 键全 0.5，`normalized_prefix: {}`） | ✅ |
| 58 | Codex 桶计价：`cache_read_explicit` 才用 cache_read，否则回落 input；长上下文按短/长双桶 | `codex/report.rs:344-348 let cache_read = if pricing.cache_read_explicit { pricing.cache_read } else { pricing.input };`<br>`:354-363`（`input_above_200k.unwrap_or(input)` 等回落）<br>`:384-391`（short_* / long_* 分桶求和） | ✅ |
| 59 | `extra_total_tokens` 本身不计价，但 output==0 时补进 output 会被计价 | `utils.rs:27-36 known/missing（saturating）；if usage.output_tokens == 0 { usage.output_tokens = missing_tokens } else { extra += missing }` | ✅ |

## 必须修正

1. **§8 §4.5(a) 尾句（以及 §13.5 L3497–L3498 同一句）关于 `claude-3-5-haiku-20241022` 的归属是错的。** 稿称「同理实测模糊命中 `us.anthropic.claude-3-5-haiku-20241022-v1:0`（input `0.8e-6`，与测试一致）」。事实是：
   `pricing.rs:2191-2192` 的兜底表在装载时把 **`claude-3-5-haiku-20241022` 作为精确键写入 `entries`**（`put_builtin_entry` → `or_insert`），而精确阶段的第一跳就是 `entries.get(model)`（`pricing.rs:1565-1567`），因此该名字**根本不进入模糊扫描**，命中的是兜底表条目本身，与任何 Bedrock 区域键无关。三条 Bedrock 候选（`anthropic.` 39 / `eu.` 43 / `us.` 43）**价格完全相同**（i=`8e-7`、o=`4e-6`），所以 `pricing.rs:4951-4954` 的断言无法区分路径——这正是该误判的来源。
   修正建议：删掉该从句，或改写为「`claude-3-5-haiku-20241022` 是**内嵌兜底表的精确命中**（`pricing.rs:2192`）；它的三个 Bedrock 变体虽在同价，但因精确键存在而不会被选中」。§13.5 的同一句需同步改。

2. **§8 §4.5(a) 与 §8 §3.4 自相矛盾。** §3.4 明写兜底表覆盖 `claude-3-5-haiku`(+`-20241022`)（`pricing.rs:2191-2192` 属实），§4.5(a) 却说该名字落到 Bedrock 键。两条不能同时成立，按第 1 条以 §3.4 为准。

3. **§8 §4.1 与 §4.5(a) 的裁决规则互斥。** §4.1 已正确写明「长度相同时字典序较小者胜」；若真走模糊，43 长度并列的 `eu.` 与 `us.` 会选 **`eu.`**（`eu` < `us`），而不是 §4.5(a) 所说的 `us.`。本轮按源码语义在真实 534 键上重放得到：`claude-3-5-haiku-20241022 → eu.anthropic.claude-3-5-haiku-20241022-v1:0`（`claude-sonnet-4-20250514 → global.…` ✅、`claude-opus-4-6-fast → claude-opus-4-6` ✅ 与稿一致）。修正第 1 条时请同时保证不再出现 `us.` 归属。

## 建议改述

1. **§6 §1 总表 Claude 行漏了一个默认根。** 表中写 `~/.claude/projects/**/*.jsonl`（`CLAUDE_CONFIG_DIR`），但 §4 §1.1 明确「无该变量时**同时**尝试 `$XDG_CONFIG_HOME/claude` 与 `~/.claude`」（`claude/src/paths.rs:45-53`，`:46 env::var("XDG_CONFIG_HOME")`）。该行是 18 行矩阵的一部分，建议补「+ `$XDG_CONFIG_HOME/claude`（见 §4 §1.1）」以免被当成唯一根。
2. **§7 全章的行号约定与自述不符。** §7 头声明「所有 `path:line` 均相对仓库根」，但正文使用裸文件名（`summary.rs:14`、`date_utils.rs:301`、`cli/last_window.rs:11`、`commands/mod.rs:189`、`config.rs:607` 等），而 §6/§8/§13.5 都用全路径（`rust/crates/ccusage-core/src/summary.rs`）。建议统一为全路径，或在 §7 头改为「文件名在 §7 覆盖清单内唯一定位」。
3. **§7「未验证/存疑」#1（blocks 默认排序）状态已过期。** 该条写「我没有运行二进制确认实际输出顺序」，但第 8 轮已跑 `blocks --json`（§13.5 运行清单里就有）；本轮实测数组严格升序（最旧在前），与代码默认 `SortOrder::Asc`（`commands/mod.rs:270` + `blocks.rs:177-182`）一致。建议把状态改为「实测闭环：默认升序；`docs/guide/blocks-reports.md:147-155` 的 "newest blocks first (default)" 与 v20.0.31 不符」，并从横幅覆盖里显式排除。
4. **§7 存疑 #8（block 的 `extra_total_tokens` 恒 0 是否会造成两口径不一致）可直接闭合。** `blocks` 走的是 Claude 的 `load_entries`（`main.rs:7 pub(crate) use adapter::claude::{load_daily_summaries, load_entries, load_entries_since};`），而 Claude 侧 `extra_total_tokens` 恒为 0（§4 §8.4），故该不一致在当前代码**不可达**；建议写明这一前提，避免读者以为任一适配器的溢出量都可能进入块级 `total()`。
5. **§8 §6.1 与 §6.2 相邻两节的「阈值口径」值得加一句区分。** 通用路径的整单判定用 `input + cache_read + 5m + 1h`（`cost.rs:147-152`），Codex 只按 `input_tokens > long_context_split_threshold(model)`（`aggregate.rs:476-487`、阈值回落 200K 见 `pricing.rs:2753-2763`）。两节各自表述正确（§8 §6.2 已明写 `input_tokens`），但并列阅读易被当成同一口径，建议在 §6.1 表格「阈值判定用的 context」行注明「Codex 路径只看 `input_tokens`」。
6. **§6 §0「把 cached 从 input 里减掉」的四家清单建议标注范围。** 该清单只覆盖本章 16 家；§4 §9.2 另述 Codex 在 adapter-all 统一列上做 `input − cached − cache_write` 归一。加一句「不含 Claude/Codex，后者见 §4 §9.2」即可避免读者把两处当矛盾。
7. **§8「未验证/存疑」#2、#3 建议补第 8 轮闭环注记。** 横幅已承诺「逐条状态以 §13.5 为准」，但只有 #1 有注记：#2（`format_currency` 舍入）已被 §13.5 第 3 条实测闭环（half-to-even 的具体取值），#3（models.dev 一致性）已被 §13.5 第 4/6/7 条部分闭环（键名集合比对 + 生成器已读）。建议按 #1 的体例加「（第 8 轮已闭环／部分闭环：见 §13.5 第 N 条）」，否则读者会以为这两条仍是空白。

## 内部矛盾

- **C1（实质）**：§8 §4.5(a) ↔ §8 §3.4 —— `claude-3-5-haiku-20241022` 到底是「兜底表精确键」还是「Bedrock 模糊命中」。以 §3.4 为准，见「必须修正」第 1、2 条。
- **C2（实质）**：§8 §4.5(a) ↔ §8 §4.1 —— `us.` 归属与同章「长度相同取字典序较小者」的规则互斥（真走模糊应为 `eu.`）。见「必须修正」第 3 条。
- **C3（表格层）**：§6 §1 Claude 行的默认根与 §4 §1.1 的「两个根同时尝试」不一致（漏 `$XDG_CONFIG_HOME/claude`）。
- **C4（体例层）**：§7 头的「路径相对仓库根」与 §7 正文的裸文件名互斥，且与 §6/§8/§13.5 的写法不统一。
- **C5（状态层）**：§7 存疑 #1 的「未运行二进制」与 §13.5 的「已运行 `blocks --json`」互斥。
- **C6（状态层）**：§8 横幅「逐条状态以 §13.5 为准」与 #2/#3 未标注互斥（同「建议改述」第 7 条）。
- **已逐项核对、确认无冲突的三处**：§6 表与 §4/§5 无实质冲突（Claude 行仅不完整；`SQLite 7 家`、`read_files_parallel 15/16`、`track_usage_load 16 家`、`兜底 12 家 14 处` 三处计数与 §4 §9.1-9.2、§6 §4.4 完全对齐）；§7 的 blocks 口径与 §4 的成本说明无冲突（`create_block` 只累加 `entry.cost`，而 `entry.cost` 正是 §4 §7.2 三态分派的结果；块级 extra 恒 0 与 §4 §8.4「Claude 侧该字段恒为 0」一致）；§8 的 `long_context_threshold` 两态与 §5 的 Codex 桶计价无冲突（Codex 聚合期按每请求切分、长桶走 above 费率，与通用路径「整单切换」在门槛处等价；差异仅在判定用的 token 组成，见「建议改述」第 5 条）。

## 无法验证

1. `antigravity` 的 protobuf 字段号（`parser.rs:689-703`）与真实 Antigravity 版本 schema 的对应关系——仓库内无从验证，§13.5 亦明确列为未闭环。
2. `copilot` 的 `credit_gap_entries`（`loader.rs:289-433`）与 `reconcile_session_state_entries`（`:201-288`）的窗口边界与 credits 归因规则（本轮只核到 §6 §3.15 的入口与注释）。
3. `opencode` 的 `prepare_message_query` / `prepare_session_message_query` SQL 文本细节与 `fork_copy_cutoffs` / `is_fork_copy` 判定条件；`time_created_looks_like_millis` 的采样语义。
4. `pi` 的 `replay_usage_path` 尾部与 `PiReplayUsagePath` 三个变体的消费点（仅核到 §6 §3.11 描述的入口与签名）。
5. `gemini` 跨文件重复计数风险（需真实数据；`gemini/src/loader.rs:38-43` 确认无跨文件合并）。
6. §7 §8.4 的 `json_float` 极端输入（NaN/Inf、超 i64）在 serde_json 下的实际输出形态；§13.5 只做了 `format_currency` 的舍入实验。
7. §7 statusline 以 transcript mtime 判陈旧、内容变而 mtime 不变时不失效（未构造样本）。
8. §8 §1.1 关于压缩形状里 `provider_specific_entry` 由 `compact.fast.or(compact.flex)` 重建（`pricing.rs:2612-2618`）——本轮未读该段。
9. §8 §2 表里「LiteLLM 刷新只增不删」的完整语义（`pricing.rs:1080-1100` 未逐行核）。
10. §8 §4.4 在线 models.dev 的 `exactOnly` 重推规则（`pricing.rs:674-698`、`:761-768`）与生成器的逐条对齐（§13.5 只做了键名集合比对）。
11. §8 §6.5 credits 的呈现细节（`amp/src/parser.rs:212/:320`、`codebuff/src/loader.rs:84`、`common/src/report.rs:108`）本轮未逐行核。
12. §6 §3.15 copilot 的 OTel 属性名映射（`parser.rs:418-441`）与 `should_emit_candidate` 去重分支。

## 我实际打开过的文件（节选，均 `file:line`）

- `docs/research/2026-10-14-ccusage-usage-statistics.md:1080-2747`（本任务全文）、`:190-750`（§4 交叉）、`:880-999`（§5 成本）、`:3452-3556`（§13.5）
- `rust/target/debug/build/ccusage-core-30b61b361638d3b0/out/litellm-pricing.json.deflate:1`（独立解压取内嵌表真身）
- `rust/crates/ccusage/src/blocks.rs:14-128`、`:565-612`
- `rust/crates/ccusage/src/commands/mod.rs:1-22`、`:183-204`、`:268-307`、`:853-874`
- `rust/crates/ccusage/src/cli/last_window.rs:1-80`
- `rust/crates/ccusage/src/main.rs:7`、`:231-232`
- `rust/crates/ccusage/src/adapter/mod.rs:1-19`
- `rust/crates/ccusage-core/src/pricing.rs:246-350`、`:496-527`、`:1393-1458`、`:1556-1619`、`:1644-1691`、`:1978-1991`、`:2360-2439`、`:2660-2737`、`:2745-2772`、`:4928-4959`、`:5841-5856`
- `rust/crates/ccusage-core/src/cost.rs:44-103`、`:118-212`
- `rust/crates/ccusage-core/src/output.rs:122-149`、`:422-475`
- `rust/crates/ccusage-core/src/date_utils.rs:52-65`、`:108-147`
- `rust/crates/ccusage-core/src/summary.rs:45-70`、`:128-175`
- `rust/crates/ccusage-core/src/utils.rs:1-40`
- `rust/crates/ccusage-core/src/progress.rs:290`
- `rust/crates/ccusage-core/src/models-dev-pricing.json:5691`、`:7504`、`:7598`、`:7632-7647`、`:15868`、`:15900`、`:24474`
- `rust/crates/ccusage-core/src/fast-multiplier-overrides.json:1-19`、`flex-multiplier-overrides.json:1-25`
- `rust/crates/ccusage-core/build.rs:150-229`
- `rust/adapters/{amp,antigravity,codebuff,copilot,droid,gemini,goose,grok,hermes,kilo,kimi,openclaw,opencode,pi,qwen,zcode}/src/paths.rs`（逐段）
- `rust/adapters/{amp,codebuff,copilot,droid,gemini,goose,grok,kimi,opencode,pi,zcode,antigravity,openclaw,hermes,kilo}/src/loader.rs`（逐段）
- `rust/adapters/{grok,copilot,zcode,gemini,kimi,pi,codebuff,amp}/src/parser.rs`（逐段）
- `rust/adapters/codex/src/report.rs:340-395`
- `rust/Cargo.toml:65` 与各适配器 `Cargo.toml`（`sqlite.workspace`）
