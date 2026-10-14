# REVIEW-R1 对抗式复核（S1 主干与 Claude 适配器 / S2 Codex 适配器）

> 复核对象：`S1-main-and-claude.md`、`S2-codex.md`
> 样本仓库：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（已 `git rev-parse` 复核，工作树干净）
> 方法：`read` / `grep` / `glob` 逐条打开源码核对；所有行号为本轮实测；71 条抽样中含全部 12 条指定高风险项
> 复核边界：未运行样本仓库任何二进制；「需要运行时行为才能证伪」的断言一律列入「无法验证」

## 结论

两份分片的**引用行号与代码事实整体可靠**（12 条高风险项中 11 条逐字命中、1 条（H11）可从「存疑」升级为「已确认」），但存在 6 处断言不完整或表述与代码不符（其中 S1 一处内部自相矛盾、一处漏列去重结构差异，S2 四处缺前提/漏维度），**补齐「必须修正」6 条后可直接并入正式文档**。

## 抽样核验清单（71 条）

| # | 分片位置 | 断言摘要 | 源码位置 | 结果 | 源码摘录 |
| --- | --- | --- | --- | --- | --- |
| H1a | S1 §4.1 | 三档键：有 requestId 只 hash id+requestId | `claude/src/lib.rs:356-358` | 正确 | `message_id.hash(&mut hasher); request_id.hash(&mut hasher);` |
| H1b | S1 §4.1 | 无 requestId 加 session；非 `msg_` 再加 timestamp | `claude/src/lib.rs:359-363` | 正确 | `if request_id.is_none() { session_id.hash(&mut hasher); if !is_anthropic_message_id(message_id) { timestamp.hash(&mut hasher); } }` |
| H1c | S1 §4.1 | `loaded_entry_matches_dedupe_key` 附加条件已写全 | `claude/src/lib.rs:413-425` | 正确 | `&& (request_id.is_some() || (loaded_entry_session_id(entry) == session_id && (is_anthropic_message_id(message_id) \|\| entry.timestamp == timestamp)))` |
| H1d | S1 §4.1 | `is_anthropic_message_id` = `starts_with("msg_")` 启发式 | `claude/src/lib.rs:373-375` | 正确 | `message_id.starts_with("msg_")` |
| H2a | S1 §4.2 | 优先级：非 sidechain 胜 sidechain | `claude/src/lib.rs:201-203` | 正确 | `if candidate_is_sidechain != existing_is_sidechain { return existing_is_sidechain; }` |
| H2b | S1 §4.2/§4.6 | 主路径不比 cost、daily 在 token 平手时比 cost | `claude/src/lib.rs:198-212` vs `claude/src/daily.rs:592-601` | 正确 | daily：`if candidate.cost != existing.cost { return candidate.cost > existing.cost; }`（daily 完整顺序为 sidechain→token→cost→speed） |
| H2c | S1 §4.2 | `usage_token_total` = input+output+cache_creation+cache_read | `claude/src/lib.rs:190-196` | 正确 | `usage.input_tokens + usage.output_tokens + usage.cache_creation_token_count() + usage.cache_read_input_tokens` |
| H3a | S1 §4.4 | 两条路由域分隔串 `sidechain-replay` / `sidechain-replay-entry` | `claude/src/lib.rs:377-392` | 正确 | `"sidechain-replay".hash(&mut hasher);` / `"sidechain-replay-entry".hash(&mut hasher);` |
| H3b | S1 §4.4 | `sidechain-entry` 桶只装 sidechain 条目 | `claude/src/lib.rs:325-339` | 正确 | `if is_sidechain_usage_entry(&deduped[index].data) { push_new_deduped_index(… sidechain_entry_replay_dedupe_hash …) }` |
| H3c | S1 §4.4 | 匹配需「至少一侧是 sidechain」 | `claude/src/lib.rs:436-440` | 正确 | `let requestless = request_id.is_none() && entry.data.request_id.is_none(); … && (candidate_is_sidechain \|\| is_sidechain_usage_entry(&entry.data))` |
| H3d | S1 §4.4 | daily 侧路由匹配与主路径**不同**（缺 timestamp 守卫），分片未列 | `claude/src/daily.rs:493-495` | 不完整 | `(existing.message_id.as_deref() == Some(message_id) && indexed_session_id == entry.session_id.as_ref() && (candidate_is_sidechain \|\| is_sidechain_daily_entry(existing)))` |
| H4a | S1 §2.3 | 主装载 memmem 模式串为 `"usage":{`（无空格即丢） | `claude/src/lib.rs:509-513` | 正确 | `let usage_marker = memmem::Finder::new(br#""usage":{"#); … if usage_marker.find(line).is_none() { continue; }` |
| H4b | S1 §2.3 | daily 同串同实现 | `claude/src/daily.rs:302-306` | 正确 | `let usage_marker = memmem::Finder::new(br#""usage":{"#);` |
| H5a | S1 §3.5 | 五项校验：version→sessionId→requestId→message.id→model | `claude/src/lib.rs:668-707` | 正确 | `if data.session_id…is_some_and(\|s\| s.is_empty()) { return false; }`（五段结构一致） |
| H5b | S1 §3.5 | 未写 `is_semver_prefix` 实际规则 | `claude/src/lib.rs:888-900` | 不完整 | 必须 `<数字>.<数字>.<数字>=`，末段至少一位：`bytes.get(index).is_some_and(u8::is_ascii_digit)` |
| H5c | S1 §3.5 | 「0 token 行只在会话报表层被过滤」 | `loader.rs:786-789` | 不完整 | `if total_tokens == 0 && summary.total_cost == 0.0 { return None; }`（统一报表层亦过滤；与 S1 §9.4 自述冲突） |
| H6a | S2 §3 | 增量：`last_token_usage` 优先且要求累计前进 | `codex/src/parser.rs:482-497` | 正确 | `.filter(\|_\| cumulative_advanced).or_else(\|\| total_usage.as_ref().map(\|usage\| subtract_codex_raw_usage(usage, previous_totals.as_ref())))` |
| H6b | S2 §3 | 累计未前进→差分全 0→被全 0 判定跳过 | `codex/src/parser.rs:501-508` | 正确 | `if raw_usage.input_tokens == 0 && … && raw_usage.reasoning_output_tokens == 0 { return Ok(()); }` |
| H6c | S2 §3 | `previous_totals` 跨 `turn_context` 保留 | `codex/src/parser.rs:448-453`、`495-497` | 正确 | `if entry_type == Some("turn_context") { if let Some(model) = … { *current_model = Some(model); *current_model_is_fallback = false; } }`（基线未重置） |
| H6d | S2 §3 | 「cached 不预先扣除」与 `report.rs:123-129` 的扣减不矛盾 | `codex/src/report.rs:123-129` | 正确 | `input_tokens.saturating_sub(cached_input_tokens.saturating_add(cache_creation_tokens))`（输出层减法，内部保留总输入） |
| H6e | S2 §3 | 六字段 `saturating_sub` | `codex/src/parser.rs:1284-1308` | 正确 | `input_tokens: current.input_tokens.saturating_sub(previous.map_or(0, \|u\| u.input_tokens))`（六桶同构） |
| H6f | S2 §3 | 钳制两处：反序列化 + 规范化 | `codex/src/types.rs:330-340`、`parser.rs:1310-1316` | 正确 | `usage.cached_input_tokens = usage.cached_input_tokens.min(usage.input_tokens);` |
| H6g | S2 §3 | total 推导 = `input + output`（不含 reasoning），记录值须 > 0 | `codex/src/types.rs:347-353` | 正确 | `total_tokens: fields.total_tokens.filter(\|total\| *total > 0).unwrap_or_else(\|\| input.saturating_add(output)),` |
| H7a | S2 §5.1 | 阈值常量与实测注释 | `codex/src/parser.rs:112-122` | 正确 | `const CODEX_REWRITTEN_BURST_PAUSE_MS: i64 = 1_000;`（注释：bursts 10–40ms，其后停顿 5.8–15.3s） |
| H7b | S2 §5.1 | 阈值用于突发检测 | `codex/src/parser.rs:168-175` | 正确 | `.contains(&(timestamp.as_millis() - first.as_millis())).then_some(first)` |
| H7c | S2 §5.1 | 阈值用于突发内逐事件吞掉 | `codex/src/parser.rs:254-261` | 正确 | `CodexReplayState::SkippingRewrittenBurst(previous) => { if … (0..=CODEX_REWRITTEN_BURST_PAUSE_MS).contains(…)` |
| H8a | S2 §1/§未验证3 | `.jsonl.zst` 不支持：扩展名等值 `jsonl` | `common/src/lib.rs:28`、`codex/src/paths.rs:85-88` | 正确 | `path.extension().is_some_and(\|ext\| ext == extension)`；`crate::collect_usage_files(dir, &mut files)` |
| H8b | S2 §1 | 全仓无 zstd；codex 依赖表无压缩 crate | `rust/adapters/codex/Cargo.toml:7-16`；全仓 grep | 正确 | 依赖仅 `ccusage-adapter-common/core/compact_str/jiff/memchr/rustc-hash/serde/serde_json/toml`；grep `zstd` 零命中 |
| H8c | S2 §1 | `flate2` 只在 ureq HTTP 栈 | `rust/Cargo.lock` | 正确 | 反查依赖者结果唯一：`ureq`（另有 `miniz_oxide`/`zlib-rs` 为其传递依赖） |
| H9a | S2 §7/§未验证4 | 真实旗标 `--speed`，`--codex-speed` 零匹配 | `ccusage-cli-parser/src/parser.rs:619` | 正确 | `"--speed" => codex_speed = parse_codex_speed(&parser.value_for("--speed")?)?,` |
| H9b | S2 §7 | 取值 `auto/standard/fast/flex` | `ccusage-cli-parser/src/parser.rs:1194-1202` | 正确 | `match value { "auto" => …, "standard" => …, "fast" => …, "flex" => …, _ => Err(…) }` |
| H9c | S2 §7 | 仅 codex 命名空间接受该旗标 | `parser.rs:605-636`、`405-430` | 正确 | 统一命令 `CodexSpeed::Auto` 硬编码：`codex_speed: CodexSpeed::Auto,`；pi/openclaw 分支 match 无 `--speed` |
| H10 | S1 §2.3 | `timestamp_from_line` 只被测试引用 | `claude/src/paths.rs:256-267`、`lib.rs:33-34`、`ccusage/src/main.rs:248-260` | 正确 | 全仓 7 处命中：定义 3 处 + `#[doc(hidden)] pub use paths::timestamp_from_line;` + README + 唯一调用点 main.rs 的 `#[test] fn extracts_compact_jsonl_timestamp` |
| H11 | S1 §未验证4 | `LoadedFile.timestamp` 是死字段 | `ccusage-core/src/types.rs:141-145`、`claude/src/lib.rs:501/660-666/173-182` | 正确（可结案） | 结构体仅 `#[derive(Debug)]`（不序列化）；唯一构造点 lib.rs:501；消费只读 `loaded_file.entries`（lib.rs:173-182） |
| H12 | S2 §5.1 | 前缀按 6 字段逐条全等消费，index 0 失配才转启发式 | `codex/src/parser.rs:234-252` | 正确 | `if prefix.get(index) == Some(&usage) { … index + 1 … } … replay = (index == 0).then(\|\| detect_rewritten_burst(path))` |
| 25 | S1 §3.1 | null 不可接受字段共 11 项，逐字一致 | `claude/src/lib.rs:867-886` | 正确 | `b"id" \| b"cwd" \| b"model" \| b"speed" \| b"costUSD" \| b"version" \| b"sessionId" \| b"requestId" \| b"isApiErrorMessage" \| b"cache_read_input_tokens" \| b"cache_creation_input_tokens"` |
| 26 | S1 §3.1 | `deserialize_usage_line` 快路径/慢路径 | `claude/src/lib.rs:727-736` | 正确 | `if memmem::find(line, b"null").is_none() { return serde_json::from_slice(line).ok(); }` |
| 27 | S1 §3.3 | 额度耗尽识别必须 `Some(true)` + 文本 + `\|` 后纯数字秒 | `claude/src/lib.rs:918-943` | 正确 | `if is_api_error_message != Some(true) { return None; }` |
| 28 | S1 §1.1 | `CLAUDE_CONFIG_DIR` 全无效硬失败、不回落 | `claude/src/paths.rs:26-43` | 正确 | `if !paths.is_empty() { return Ok(paths); } return Err(cli_error(format!("No valid Claude data directories found…` |
| 29 | S1 §1.1 | 无变量时同时尝试 xdg 与 `~/.claude` | `claude/src/paths.rs:45-53` | 正确 | `for path in [xdg.join("claude"), home.join(".claude")] { if path.join("projects").is_dir() && seen.insert(path.clone()) {` |
| 30 | S1 §1.2 | 排序键 = 完整路径字符串 | `claude/src/paths.rs:77` | 正确 | `files.sort_by_cached_key(\|path\| path.to_string_lossy().into_owned());` |
| 31 | S1 §1.2 | `is_project_path_segment` 拒绝 4 类值 | `claude/src/paths.rs:248-254` | 正确 | `!value.is_empty() && value != "." && value != ".." && !value.contains('/') && !value.contains('\\')` |
| 32 | S1 §1.3 | workflows 深路径落到兜底（会话名 `wf_1`） | `claude/src/paths.rs:308-330` | 正确 | `if relative.len() >= 4 && relative.get(relative.len() - 2) == Some(&"subagents")`（`wf_1` 场景不命中该分支） |
| 33 | S1 §2.2 | 线程数 = `available_parallelism().min(files.len())`，单线程直通 | `common/src/lib.rs:89-101` | 正确 | `.map(usize::from).unwrap_or(1).min(files.len())`；`if worker_count <= 1 { return files.iter().map(…).collect(); }` |
| 34 | S1 §2.2 | 分块按大小贪心 + 滤空块 + `max(1)` | `common/src/lib.rs:49-81` | 正确 | `let chunk_count = chunk_count.max(1);`…`weighted_indexes.sort_unstable_by(\|a, b\| match b.1.cmp(&a.1) {` |
| 35 | S1 §2.4 | 剪枝阈值 = `since - 1 天` | `claude/src/paths.rs:81-84` | 正确 | `const MTIME_PRUNE_MARGIN_MS: i64 = MILLIS_PER_DAY;` |
| 36 | S1 §5.2 | `-fast` 后缀只影响展示名，计价靠 `fast_multiplier` | `claude/src/lib.rs:535-543`、`ccusage-core/src/cost.rs:110-115` | 正确 | `let multiplier = if matches!(usage.speed, Some(Speed::Fast)) { pricing.fast_multiplier } else { 1.0 };` |
| 37 | S1 §6.2 | 1h 缓存写倍率 2.0，无 breakdown 时全按 5m 桶 | `ccusage-core/src/cost.rs:7`、`118-131` | 正确 | `const CACHE_CREATE_1H_INPUT_MULTIPLIER: f64 = 2.0;`…`} else { (usage.cache_creation_input_tokens, 0) };` |
| 38 | S1 §6.3 | `input_tokens` 是不含缓存的余量（源码注释） | `ccusage-core/src/cost.rs:139-142` | 正确 | `// … but the vendor's tier is chosen by the request's whole context, cached or not`（含「`input_tokens` here is the uncached remainder」） |
| 39 | S1 §7.2 | 三分支本体 | `ccusage-core/src/cost.rs:39-46` | 正确 | `CostMode::Display => cost_usd.unwrap_or(0.0),` |
| 40 | S1 §9.1 | `BUILT_IN_AGENT_NAMES` 18 项逐字一致 | `ccusage-core/src/lib.rs:63-82` | 正确 | `"claude","codex","opencode","amp","droid","codebuff","hermes","pi","goose","openclaw","kilo","copilot","gemini","antigravity","kimi","qwen","grok","zcode"` |
| 41 | S1 §0 | `adapter/mod.rs:1-19` 19 个别名导出 | `ccusage/src/adapter/mod.rs:1-19` | 正确 | 文件恰 19 行 19 项（`pub(crate) use ccusage_adapter_* as *;`） |
| 42 | S1 §10.1 | `run_daily` 走日汇总 + `--instances/--project` 开分组 | `ccusage/src/commands/mod.rs:33-42` | 正确 | `load_daily_summaries(&shared, args.project.as_deref(), args.instances \|\| args.project.is_some())?` |
| 43 | S1 §10.3 | 会话列表丢四列全 0 行并按 cost 排序 | `ccusage/src/commands/mod.rs:155-161` | 正确 | `rows.retain(\|row\| row.input_tokens + row.output_tokens + row.cache_creation_tokens + row.cache_read_tokens > 0);` |
| 44 | S1 §10.5 | 无 `--limit/--page/--offset` 参数 | `ccusage-cli-parser/src/parser.rs`（全文件 grep） | 正确 | 仅命中 `--last`：`"--last" => shared.last = Some(parse_last_periods(…))` |
| 45 | S2 §6 | 分组键与 since/until 文本过滤 | `codex/src/aggregate.rs:408-430` | 正确 | `AgentReportKind::Weekly => week_start(&date, WeekDay::Monday).unwrap_or(date),` |
| 46 | S2 §6 | 长上下文按单请求输入量判定 | `codex/src/aggregate.rs:476-487` | 正确 | `let is_long_context = event.input_tokens > crate::pricing::long_context_split_threshold(model);` |
| 47 | S2 §4 | 回退模型链与 `gpt-5` 缺省、auto-review 日期表 7 项 | `codex/src/parser.rs:802-831`、`codex-auto-review-fallbacks.json` | 正确 | `let model = parsed_model.or_else(\|\| current_model.clone()).or_else(\|\| { is_fallback_model = true; … "gpt-5" … })`；表 7 项逐字一致 |
| 48 | S2 §5.2 | 去重键 `Usage(...)` 漏 session 维度 | `codex/src/aggregate.rs:762-779` | 不完整 | `let (session_hash, session_len) = if kind == AgentReportKind::Session { (hash_text(&event.session_id), event.session_id.len()) } else { (0, 0) };` |
| 49 | S2 §5.2 | 「重复保留 file_index 最小者」仅 compaction 成立 | `codex/src/aggregate.rs:720-732` | 不完整 | `if let Some(compaction) = record.compaction.as_mut() && file_index < compaction.file_index { … }`（普通事件首见者胜） |
| 50 | S2 §2 | 「`response_item`/`session_meta` 等一律忽略」缺前提 | `codex/src/parser.rs:708-713` | 不完整 | `if USAGE_FIELD_FINDER.find(line).is_some() \|\| INPUT_TOKENS_FIELD_FINDER… { return Some(CodexLineKind::Headless); }` |
| 51 | S2 §9 | 「返回三态」与 `enum CodexLineKind { Session, Headless }` 自相矛盾 | `codex/src/parser.rs:62-66`、`661-715` | 不完整 | 枚举仅两变体，函数签名为 `Option<CodexLineKind>`（None 才是第三态） |
| 52 | S2 §7 | `sessionFast` 快照 cost 为 Standard 的 2 倍 | `codex/src/snapshots/…periods_sessions_costs_and_fallback_models.snap:11/109` | 正确 | `:11 "costUSD": 0.00040425` 与 `:109 "costUSD": 0.0008085`（恰 2 倍） |
| 53 | S2 §10 | 统一管线速度固定 `Auto` | `adapter-all/src/loader.rs:675`、`688` | 正确 | `let speed = codex::resolve_codex_speed(CodexSpeed::Auto);` |
| 54 | S2 §10 | `codex_group_row`：扣缓存、成本同源、reasoning 进 metadata、明细按 cost 降序 | `adapter-all/src/loader.rs:830-886` | 正确 | `model_breakdowns.sort_by(\|a, b\| b.cost.total_cmp(&a.cost));` + `"reasoningOutputTokens": group.reasoning_output_tokens,` |
| 55 | S1 §9.2 | Codex 唯一「从 input 扣缓存」，组级同构 | `adapter-all/src/loader.rs:845-855`、`868-875` | 正确 | `let input = codex::non_cached_input_tokens(usage.input_tokens, usage.cached_input_tokens, usage.cache_creation_tokens);` |
| 56 | S1 §9.4 | 零值行丢弃（注释：only-cost 行仍计费） | `adapter-all/src/loader.rs:786-789` | 正确 | `// Cost-only rows, such as Copilot credits missing from modelMetrics, still bill.` |
| 57 | S1 §9.5 | Codex 无窗口走 `load_groups` 快路径；Claude 传 `None` project_filter | `adapter-all/src/loader.rs:633-648`、`672-683` | 正确 | `if shared.since.is_none() && shared.until.is_none() { let groups = codex::load_groups(shared, kind)?;` |
| 58 | S1 §9.1 | 并行 = `thread::scope` + `mpsc`，一 agent 一线程 | `adapter-all/src/loader.rs:430-435` | 正确 | `thread::scope(\|scope\| { let (sender, receiver) = mpsc::channel();` |
| 59 | S1 §0 | 裸 `ccusage` → `adapter::all::run`；19 命令分派 | `ccusage/src/main.rs:28-33`、`53-65` | 正确 | `None => { … adapter::all::run(args) }` |
| 60 | S1 §2.3/§5.2 | `byte_lines` 无 String 分配、`suffix_string` 为拼接 | `ccusage-core/src/fast.rs:82-110` | 正确 | `if let Some(newline) = memchr(b'\n', self.bytes) { let (line, rest) = self.bytes.split_at(newline);` |

## 必须修正

- [E1] S1 §3.5 的断言「0 token 行**只在**会话报表层被过滤」与代码事实及本分片 §9.4 自相矛盾。断言：只有 `run_session` 过滤 0 token 行；实际：统一报表（daily/weekly/monthly/session 的 `--all` 链路）在 `summary_rows` 里也丢弃 `total_tokens == 0 && total_cost == 0.0` 的行（`rust/crates/ccusage-adapter-all/src/loader.rs:786-789`），Claude 适配器自身的日汇总累加器不丢弃（`claude/src/daily.rs:617-644`）。修法：把 §3.5 末句改为「装载层接收 0 token 行；会话报表按四列之和过滤（`commands/mod.rs:155-157`），统一报表按『total 与 cost 双 0』过滤（`loader.rs:786-789`）」。
- [E2] S1 §4.6 与 §8.4 漏列第 4 条结构差异：**daily 侧的 sidechain 回放路由没有 timestamp 守卫**。断言（§8.4 只列 3 条差异，且 §4.6 表格未提路由差异）；实际：主路径 `loaded_entry_matches_sidechain_dedupe_key` 要求 `requestless || entry.timestamp == timestamp`（`claude/src/lib.rs:436-440`），daily 对应分支只有 `message_id + session + 至少一侧 sidechain`（`claude/src/daily.rs:493-496`）。后果：父条目与「带新 requestId、时间戳已改写」的 sidechain 副本，daily 会合并、主路径不会——这比 §8.4 已列的 3 条更实质（影响是否合并，而非决胜平局）。修法：§8.4 增加差异 4，并在 §4.6 表格补一行「sidechain 路由匹配条件」。
- [E3] S2 §2 断言「不匹配的行既不解析也不产出事件——即 `response_item`、`turn_started`、`task_started`、`inter_agent_communication*`、`session_meta` 等一律忽略」缺前提。实际判据是字面量扫描：行内出现 `"usage":`、`"input_tokens":` 或 `"prompt_tokens":` 时会被判为 `Headless` 并尝试产出事件（`rust/adapters/codex/src/parser.rs:708-713`），与行的 `type` 无关。修法：改为「若行内不含 `usage:/input_tokens:/prompt_tokens:` 字面量则忽略；含则按 headless 分支尝试解析（与其 `type` 是否为上述值无关）」。
- [E4] S2 §5.2 断言「无 `response_id` 则用 `Usage(timestamp, model, 6 桶)`」漏 session 维度（§未验证 7 的括注「非 Session 分组时不含 session」才是准确的）。实际 `CodexUsageKey` 含 `session_hash`/`session_len`，**仅**在 `kind == Session` 时非零，其余报表恒为 `(0, 0)`（`rust/adapters/codex/src/aggregate.rs:29-42`、`762-779`）。修法：正文写明「Session 报表下额外含 session_id 的 hash+len」。
- [E5] S2 §5.2 断言「重复时保留 `file_index` 最小者的 session/timestamp/model」缺少限定。实际只有携带 `response_id` 的 compaction 记录会按 `file_index` 比较并替换（`rust/adapters/codex/src/aggregate.rs:720-732`）；普通事件重复时直接返回 `false`、保留先见者，不比较 `file_index`（同函数 733-750）。修法：限定为「compaction 记录取 file_index 最小者；普通事件首见者胜，仅 tier 取并集（721）」。
- [E6] S2 §9 写「分叉函数 `codex_line_usage_kind` … 返回三态」，紧接着给出 `enum CodexLineKind { Session, Headless }` ——枚举只有两个变体，三态实为 `Option<CodexLineKind>` 的 `Some(Session) / Some(Headless) / None`（`rust/adapters/codex/src/parser.rs:62-66`、`661-715`）。修法：改「返回 `Option<CodexLineKind>`：Session / Headless / 忽略（None）」。

## 建议补充

- [I1] S1 §3.5 只给了 `is_semver_prefix` 的行号（`claude/src/lib.rs:888-900`），未写规则。补上：必须形如 `<至少1位数字>.<至少1位数字>.<至少1位数字…>`，第三段只校验首位是数字（`2.1.0-beta.1` 通过、`1.2` 拒绝），用于过滤 `version` 存在但非语义化的行。
- [I2] S2 §1 未提 `CODEX_HOME` 的边界：全为空段（如 `CODEX_HOME=",,"`）时返回**空根列表**，既不报错也不回落 `~/.codex`（`codex/src/paths.rs:227-234`）。这与 Claude 侧「全部无效即硬失败」（`claude/src/paths.rs:40-42`）策略相反，值得作为 TokenScope 兼容性差异写入。
- [I3] S2 §7 只列 CLI 旗标。补：codex 速度还有第二条入口——配置文件经 `config.apply_agent_args(&mut codex_speed, …)` 注入（`ccusage-cli-parser/src/parser.rs:613`；`ccusage-config/src/config.rs:508-516`），因此「哪些命令会带 `codex_speed`」不止 codex 子命令（pi/openclaw 分支也构造该字段，仅 CLI 旗标不可用）。
- [I4] S2 §10 表建议补一行「`totalCost` 的来源」：统一管线把 `costUSD` 键**重命名**为 `totalCost`（`codex/src/report.rs:115-119` 的 `object.remove("costUSD")` + `insert("totalCost")`），而非两套计价。
- [I5] S1 §9.2 的 extra 贡献点清单（hermes/droid/goose/kimi/copilot/codebuff/openclaw 七处）本节只抽验了代码结构，建议正式文档中逐处附一行摘录，否则读者无法复核。
- [I6] S1 §2.3 与 §11.4 已提「紧凑 JSON 是硬约束」，建议在正式文档把风险定量化：`"usage": {` 带空格的行会被 `lib.rs:509` 静默丢弃且**不计入任何错误计数**（§3.2 已述），TokenScope 若用 serde 整行解析，需在差异说明里给出「同一样本 token 总数可能高于 ccusage」这一结论方向。

## 无法验证

- [U1] S1 §2.4 的剪枝效果、S2 §5.1 的 replay 匹配精度与阈值实测区间（注释声称 burst 10–40ms、其后停顿 5.8–15.3s）：均为运行时/真实日志行为，静态阅读不可证伪；分片已自标存疑，复核无法推进。
- [U2] S2 §6 引用的 8 处测试断言内容（如 `aggregate.rs:938-965`、`compaction_tests.rs:283-311`）：本轮只核对了被测函数体，未逐行读测试断言，未能独立证明「测试确实断言了该行为」。
- [U3] S1 §2.4 的 statusline 细节（`ccusage/src/commands/mod.rs:337-343`、`719-751`、`826-845`、`860-864`）与 §10.4 的渐进窗口（`557-597`）：未逐行打开，仅由分片自述。
- [U4] S2 §5.1 引用的测试名与行号（如 `loader.rs:2341` 的 `bounded_loading_does_not_select_a_misleading_filename`、`loader.rs:2578`）：未打开对应测试体核对名称与行号。
- [U5] S1 §10.5「`--last N` 折算（`cli/last_window.rs:11-24`）」、§7.4 `--offline` 的 `pricing.rs:1002-1038`：未打开核对，属未覆盖抽样项。
- [U6] 全部需要真实 ccusage 日志（`~/.claude`、`~/.codex`）才能复现的字段行为（速度旗标对老日志的兜底、compaction 归属在并行下的确定性）：样本仓库为只读浅克隆，未运行二进制。

## 我实际打开过的文件（file:line）

- `rust/adapters/claude/src/lib.rs:22-45, 100-199, 185-324, 320-349, 340-469, 490-619, 620-759, 855-974`
- `rust/adapters/claude/src/daily.rs:290-409, 430-549, 555-684`
- `rust/adapters/claude/src/paths.rs:20-119, 240-349`
- `rust/adapters/common/src/lib.rs:1-60, 60-129`
- `rust/adapters/common/src/jsonl.rs:1-45`
- `rust/adapters/codex/src/parser.rs:55-99, 90-209, 210-339, 342-441, 440-569, 640-759, 780-899, 1270-1339`
- `rust/adapters/codex/src/types.rs:1-60, 320-369`
- `rust/adapters/codex/src/paths.rs:1-130, 131-280`
- `rust/adapters/codex/src/replay.rs:140-269`
- `rust/adapters/codex/src/report.rs:108-147`
- `rust/adapters/codex/src/aggregate.rs:20-69, 400-454, 458-532, 705-794`
- `rust/adapters/codex/src/speed.rs:1-80`
- `rust/adapters/codex/src/codex-auto-review-fallbacks.json:1-30`
- `rust/adapters/codex/Cargo.toml:1-20`
- `rust/crates/ccusage-core/src/types.rs:130-189`
- `rust/crates/ccusage-core/src/cost.rs:1-150`
- `rust/crates/ccusage-core/src/lib.rs:55-94`
- `rust/crates/ccusage-core/src/summary.rs:35-84`
- `rust/crates/ccusage-core/src/fast.rs:78-117`
- `rust/crates/ccusage-core/src/utils.rs:1-25`
- `rust/crates/ccusage-adapter-all/src/lib.rs:1-40`
- `rust/crates/ccusage-adapter-all/src/loader.rs:396-435, 555-704, 770-829, 825-914`
- `rust/crates/ccusage-adapter-all/src/types.rs:72-131`
- `rust/crates/ccusage-cli-parser/src/parser.rs:340-449, 600-669, 915-974, 1180-1247`
- `rust/crates/ccusage/src/main.rs:1-75, 238-277`
- `rust/crates/ccusage/src/adapter/mod.rs:1-19`
- `rust/crates/ccusage/src/commands/mod.rs:28-77, 144-208`
- `rust/Cargo.lock`（依赖反查：flate2 ← ureq）
