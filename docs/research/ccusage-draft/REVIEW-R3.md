# REVIEW-R3：S4-aggregation.md 对抗式复核（聚合 / 时间 / blocks / 输出分片）

- 被复核文件：`docs/research/ccusage-draft/S4-aggregation.md`（645 行，全文读完）
- 证据仓库：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（只读，未运行任何二进制）
- 复核方式：`read` / `grep` 逐条打开源码与测试；每条抽样都贴出源码行作为"打开过"的证据
- 复核立场：**证伪优先**——凡断言与源码不符、或指向的实现与断言描述不同，一律列出

## 结论

抽验 58 条（N01–N58，覆盖 K1–K15 全部高风险项）：**51 条完全成立，7 条存在问题——1 条不成立、2 条部分不符、2 条不完整、2 条引用或子句错误**。草稿的证据密度与行号精度整体很高（绝大多数 `path:line` 可命中），主要问题集中在五处：

1. **结论级不成立**：§8.3 声称 `--no-cost` 会移除 `blocks[].burnRate.cost`。真实产物里该字段名是 `costPerHour`，`strip_cost_json` 只删 `totalCost`/`costUSD`/`cost` 三个键，因此 `--no-cost` 后 blocks 的 JSON 仍会保留 `burnRate.costPerHour`。草稿所引测试（`output.rs:746-819`）里的 `burnRate.cost` 是测试自造的键名，不是真实序列化字段。
2. **顺序写反**：§3.4 把三项校验的执行顺序写成"解析命令 → `config_error` → 根/命令参数解析"，实际是 `config_error` 在前、命令解析在后，且前面还有三项校验（`parser.rs:81-131`）。
3. **"共用函数"表述与实现不符**：§3.1 称 `date_within_range` 被 `blocks.rs` 与 `all/src/loader.rs` 共用；实测只有 `adapter-common` 调用它，另外三处是**内联复制的等价比较**。
4. **两处关键条件遗漏**：§6.5 漏掉 `project_block_usage` 的 `calculate_burn_rate(block)?` 前置（单条 entry 的活跃块不输出 projection/tokenLimitStatus）；§6.1 漏掉 `--session-length` 还有配置文件来源（`blocks.session_length`）。
5. **引用文件写错**：§10.3 把 `Unknown codex option '--by-agent'` 的测试断言写成在 `parser.rs`，实际在 `crates/ccusage-cli-parser/src/tests.rs:644-645`。

K1–K4、K6–K8、K10–K15 的主体断言全部核对成立（K5 因漏配置项判"不完整"）；其中 K1 的严格大于、K2 的 UTC 整点、K3 的 `is_active` 双条件、K4 的整条 entry 归属、K8 的长度白名单（20/24/25/29）、K10 的前缀界限（`2026`）、K11 的缓存路径与 pid 存活分支、K14 的 100/120 双阈值、K15 的 UTC/本地口径不一致均是**逐字符核对成立**。

## 抽样核验清单

| 编号 | 分片位置 | 断言摘要 | 源码位置 | 结果 | 源码摘录 |
| --- | --- | --- | --- | --- | --- |
| N01 | §1.1 / §0 表 | `summarize_by_key` 用 `BTreeMap` 累加，逐 key 调 `meta_fn` 写回，返回字典序 | `summary.rs:23-35` | 符合 | `let mut groups: BTreeMap<String, UsageAccumulator> = BTreeMap::new();` / `for (key, group) in groups { let (date, project) = meta_fn(&key);` |
| N02 | §1.2 / K6 | 复合 key 用 `{}|{}` 拼接、`split_once('|')` 拆回（无 `\0` 写法） | `summary.rs:402-405` | 符合 | `|entry| format!("{}|{}", entry.date, entry.project_path)` / `let (date, project) = key.split_once('\|').unwrap();` |
| N03 | §1.2 | 会话维度不走 `summarize_by_key`，用元组索引 + `SessionAccumulator` | `commands/mod.rs:186-199` | 符合 | `let mut group_indexes = FxHashMap::<(Arc<str>, Arc<str>), usize>::default();` |
| N04 | §1.3 / K7 | `model_breakdowns` 按 cost 降序（`total_cmp`），稳定排序保留插入序 | `summary.rs:100` | 符合 | `self.breakdowns.sort_by(\|a, b\| b.cost.total_cmp(&a.cost));` |
| N05 | §1.3 / K7 | `models_used` 首见序（`breakdown_indexes` 复用 + `models.push`） | `summary.rs:67-79`, `117` | 符合 | `let index = self.breakdown_indexes.get(model.as_ref())` … `models_used: self.models,` |
| N06 | §1.3 | `credits`/`message_count` 仅 Some 时惰性建零再累加 | `summary.rs:59-64` | 符合 | `if let Some(credits) = entry.credits { *self.credits.get_or_insert(0.0) += credits; }` |
| N07 | §1.4 | `is_none_or` 保证严格 `>`/`<`，时间戳相同不覆盖 | `summary.rs:136-152` | 符合 | `.is_none_or(\|(timestamp, _, _)\| entry.timestamp > *timestamp)` |
| N08 | §1.4 / §5.2 / K12 | `last_activity`/`first_activity` 为 UTC RFC3339 毫秒串 | `summary.rs:165-166` | 符合 | `summary.last_activity = Some(format_rfc3339_millis(timestamp));` / `summary.first_activity = self.earliest.map(format_rfc3339_millis);` |
| N09 | §1.5 / K6 | 月桶 `date.get(..7)`、周桶 `week_start`、非法日期回退原文本、`date=None` 整行跳过 | `summary.rs:185-191` | 符合 | `let Some(date) = row.date.as_deref() else { continue; };` / `BucketKind::Weekly => week_start(date, start).unwrap_or_else(\|\| date.to_string()),` |
| N10 | §1.5 | 折叠时 `models_used` 去重保序、`total_cost` 用 `+=` 无饱和 | `summary.rs:230`, `245`, `252-256` | 符合 | `summary.total_cost += row.total_cost;` / `if seen_models.insert(model.clone()) { summary.models_used.push(model.clone()); }` |
| N11 | §1.6 | `sort_summaries` 纯字符串比较，Asc/Desc 反向 | `summary.rs:311-314` | 符合 | `SortOrder::Asc => date_fn(a).cmp(date_fn(b)),` |
| N12 | §1.7 | 别名作用于 `CCUSAGE_MODEL_ALIASES`，支持精确键、`-fast` 基名、JSON 与 `a=b, c=d` | `model_aliases.rs:14-26`, `33`, `39-61` | 符合 | `if let Some(base_model) = model.strip_suffix("-fast")` / `stripped.split([',', ';', '\n'])` |
| N13 | §2.1 / **K2** | `floor_to_hour` 走整数 `div_euclid(3600000)`，即 UTC 整点，不含时区换算 | `date_utils.rs:58-60` | 符合 | `Self(self.0.div_euclid(MILLIS_PER_HOUR) * MILLIS_PER_HOUR)` |
| N14 | §2.2 / **K8** | 接受长度仅 20/24/25/29；`bytes[19]` 判 `Z`/`±` 或 `.`；无时区后缀直接 `None`；毫秒固定 3 位 | `date_utils.rs:113-117` | 部分不符（细节）：长度集合与"无后缀失败"成立；但 len25+`Z` 会进入首分支后在 `parse_timezone_offset` 失败，且微秒形式（len 27/32）是**长度不匹配**早退，不会走到分隔符校验 | `20 \| 25 if bytes[19] == b'Z' \|\| bytes[19] == b'+' \|\| bytes[19] == b'-' => (0, 19),` / `24 \| 29 if bytes[19] == b'.' => (parse_digits(&bytes[20..23])?, 23),` |
| N15 | §2.2 | 偏移只支持 `Z` 或 `±HH:MM`，`±HHMM`/`+HH`/带秒均 `None` | `date_utils.rs:147-153` | 符合 | `if bytes.len() != 6 \|\| !matches!(bytes[0], b'+' \| b'-') \|\| bytes[3] != b':' { return None; }` |
| N16 | §2.3 | `is_valid_timezone` = `local`/`UTC`/`TimeZone::get`；`parse_tz` 未知名回退 `None` | `date_utils.rs:297-307` | 符合 | `matches!(value, "local" \| "UTC") \|\| JiffTimeZone::get(value).is_ok()` / `"local" => None,` |
| N17 | §2.3 / **K9** | 优先级 flag > config > 系统；statusline 用自身 `timezone`，shared 用 `config.rs:607` | `config.rs:492-493`, `607-608`；`parser.rs:93-94`, `96-107`, `220-222` | 符合 | `if let Some(timezone) = options.timezone { shared.timezone = Some(timezone); }`（config 先应用，随后 root flag 覆盖） |
| N18 | §2.3 / **K9** | 仓库自身不读 `TZ`，仅 jiff 在落到 `system()` 时读 | 全仓 grep：仅 `model_aliases.rs:33` 一处 `env::var`；`date_utils.rs:317`, `374`；`Cargo.lock:779` | 符合 | `let timezone = timezone.cloned().unwrap_or_else(JiffTimeZone::system);` / Cargo.lock `name = "jiff"` 行号 779、版本 `0.2.38` |
| N19 | §3.1 / **K10** | 两侧闭区间、`YYYYMMDD` 文本比较、前缀界限 `2026` 可用、无 bound 时短路返回 true | `date_utils.rs:196-203`；测试 `date_utils.rs:486-494` | 符合 | `since.is_none_or(\|since\| compact.as_str() >= since)` / `assert!(date_within_range("2026-01-02", Some("2026"), Some("202602")));` |
| N20 | §3.1 | "`date_within_range` 被报表与加载器共用（含 `blocks.rs:166-175`、`all/src/loader.rs:656-665`, `756-766`）" | `adapter-common/src/lib.rs:36-47`（真调用）；`blocks.rs:170-174`、`all/src/loader.rs:660-664`、`758-766`（内联） | 部分不符：只有 adapter-common 调用该函数；另外三处是行为等价的**内联复制**，未共用同一函数 | `entries.retain(\|entry\| date_within_range(&entry.date, ...))`（common） vs `shared.since.as_ref().is_none_or(\|since\| &date >= since)`（blocks/loader） |
| N21 | §3.2 | `load_entries` 忽略 since；`load_entries_since` 先丢窗口前文件、调用方须再过滤 | `claude/src/lib.rs:47-74`, `126-130` | 符合 | `/// entries dated before `since` may be missing. Callers must discard those` / `split_files_before_since(files, shared, utc_now()).kept` |
| N22 | §3.3 | `--last`：`earlier = max(count,1)-1`；Day/Week/Month 各自回推；输出 `YYYYMMDD` | `last_window.rs:20-45` | 符合 | `let earlier = i64::from(count.max(1) - 1);` / `PeriodUnit::Week => parse_date(&week_start(today, start_of_week)?)?.checked_sub(Span::new().weeks(earlier))` |
| N23 | §3.3 | 单位由命令决定、`Session` 返回 `None`、`session`/`blocks`/`statusline` 不支持、周报统一周一、结果写回 `shared.since` | `cli/last_window.rs:18-22`, `56`, `60-73` | 符合 | `shared.since = Some(since);` / `Some(Command::Session(_) \| Command::Blocks(_) \| Command::Statusline(_)) => None,` / `const UNIFIED_WEEK_START: WeekDay = WeekDay::Monday;` |
| N24 | §3.4 | "三个校验的执行顺序：解析命令 → `config_error` → 根/命令参数解析 → `last_option_error` → `date_window_error`" | `parser.rs:81-131` | **不成立**：实际 `config_error`(:90-92) 在根参数解析(:93-107) 与命令解析(:109-122) **之前**；命令解析之后才是 `last_option_error`(:126)、`date_window_error`(:129)；另有 :81-89 三项前置校验未提 | `if let Some(message) = config.config_error() { ... }` 出现于 `parse_command(...)` 调用之前 |
| N25 | §4.2 | 周 key 偏移 `shift=(day-start+7)%7`、`Sunday=0`、`Monday=1`，取周首日 `YYYY-MM-DD` | `summary.rs:317-331` | 符合 | `let shift = (day - start_num + 7) % 7;` / `Some(format_naive_date(date.checked_add_days(-shift)?))` |
| N26 | §4.2 | CLI 选项名 `--start-of-week`/`-w`，默认 Sunday，非法值 `Invalid week day '{value}'` | `parser.rs:520-523`, `530-531`, `1181-1190` | 符合 | `start_of_week: WeekDay::Sunday,` / `_ => Err(format!("Invalid week day '{value}'")),` |
| N27 | §5.1 / §5.3 | `ccusage session --id` 分流回 `Command::Session`；顶层无 id 时走 `Command::All{Session}` | `parser.rs:432-473`；`commands/mod.rs:148-150` | 符合 | `if args.id.is_some() { ... return Ok(Command::Session(args)); }` / `Ok(Command::All(AgentCommandArgs { kind: AgentReportKind::Session, session_id: None,` |
| N28 | §5.1 | `--id` 分支不走 all 报告（all 的 run 不读 `session_id`） | `all/src/lib.rs:38-70` | 符合 | `let include_agents = args.by_agent;` … 全函数无 `session_id` 引用 |
| N29 | §5.1 / **K12** | 会话分组键 `(project_path, session_id)`；行键收敛 `date→week→month→session_id`；全零行丢弃 | `all/src/loader.rs:734-746`, `778-789` | 符合 | `.entry((entry.project_path.to_string(), entry.session_id.to_string()))` / `if total_tokens == 0 && summary.total_cost == 0.0 { return None; }` |
| N30 | §5.2 / **K12** | "Last Activity" 列按任意行有 `last_activity` 出现，值截前 10 字符 | `output.rs:245`, `253-256`, `583-585` | 符合 | `let include_last_activity = rows.iter().any(\|row\| row.last_activity.is_some());` / `s.get(..10).unwrap_or(s).to_string()` |
| N31 | §5.4 | `run_session` 强制 `Desc` 后按 `total_cost` 排序（默认成本降序） | `commands/mod.rs:152-161` | 符合 | `session_shared.order = SortOrder::Desc;` / `SortOrder::Desc => b.total_cost.total_cmp(&a.total_cost),` |
| N32 | §5.4 | all 会话排序：显式 `--order asc` 才升序，否则降序；平局 `period`→`agent` | `all/src/loader.rs:398-414` | 符合 | `if shared.order_explicit && shared.order == crate::cli::SortOrder::Asc { a.total_cost.total_cmp(&b.total_cost) } else { b.total_cost.total_cmp(&a.total_cost) }` |
| N33 | §6.1 / **K5** | `DEFAULT_SESSION_DURATION_HOURS = 5.0`；`--session-length`/`-n` 默认取之；`cli.rs` 传入 | `core/lib.rs:59`；`parser.rs:195`, `208-213`；`cli.rs:19-22` | 不完整：**漏了配置文件来源** `blocks.session_length`（`config.rs:450-452`），它也在 `parse_command` 里经 `config.apply_blocks_args` 覆盖默认值 | `if let Some(session_length) = options.session_length { args.session_length = session_length; }` |
| N34 | §6.2 / **K1** | 切块条件 `since_start > duration \|\| since_last > duration`（严格大于）；仅 `since_last > duration` 插 gap；gap `start = last + duration` | `blocks.rs:73-88`, `149-155` | 符合 | `if since_start > session_duration \|\| since_last > session_duration {` / `if since_last > session_duration { blocks.push(create_gap_block(last_time, entry.timestamp, session_duration)); }` / `let start = last.checked_add_millis(duration).unwrap_or(last);` |
| N35 | §6.2 / **K2** | 块锚点 `floor_to_hour(entry.timestamp)`；表格用 `local_parts` 本地渲染 | `blocks.rs:89`, `92`, `292-315` | 符合 | `current_start = Some(floor_to_hour(entry.timestamp));` / `let parts = local_parts(timestamp);` |
| N36 | §6.2 | 恰好等于一个窗口的间隔不断块（测试） | `commands/mod.rs:1275-1279` | 符合 | `// A pause of exactly one session keeps the chain going.` / `assert_eq!(statusline_block_anchor(&[at(9), at(4), at(1)], now, session), BlockAnchor::NeedsMoreHistory);` |
| N37 | §6.3 / **K4** | 整条 entry 归入所在块，无跨块拆分；`create_block` 仅 `add_usage`（不含 extra） | `blocks.rs:123-125` | 符合（伪代码字段名不精确）：实际取 `entry.data.message.usage`，草稿写作 `entry.usage` | `token_counts.add_usage(entry.data.message.usage);` / `cost += entry.cost;` |
| N38 | §6.3 | `token_counts.total()` 含 extra，但 block 的 extra 恒 0 | `types.rs:89-95`；`blocks.rs:118`, `124` | 符合 | `saturating_add(self.extra_total_tokens)` / `let mut token_counts = TokenCounts::default();` |
| N39 | §6.4 | 单条 entry（duration<=0）→ burn rate `None`；`tokens_per_minute` 含 cache、indicator 只用 input+output | `blocks.rs:567-583` | 符合 | `if duration_minutes <= 0.0 { return None; }` / `tokens_per_minute_for_indicator: non_cache / duration_minutes,` |
| N40 | §6.5 / **K3** | `project_block_usage` 仅 `is_active` 且非 gap；`remaining_minutes` 取整；cost 保留 2 位 | `blocks.rs:586-601` | 不完整：漏掉 `let burn = calculate_burn_rate(block)?;`——单条 entry 的活跃块返回 `None`，因此不输出 projection 也不输出 `tokenLimitStatus` | `if !block.is_active \|\| block.is_gap { return None; }` / `let burn = calculate_burn_rate(block)?;` |
| N41 | §6.5 | `parse_token_limit`：`None`/`""`/`"max"` 用 `max_tokens`（>0 才 Some）；`max_tokens` 只取非 gap 非 active 块的最大值 | `blocks.rs:603-607`；`commands/mod.rs:283-288` | 符合 | `None \| Some("") \| Some("max") => (max_tokens > 0).then_some(max_tokens),` / `.filter(\|block\| !block.is_gap && !block.is_active)` |
| N42 | §6.6 | 过滤顺序 identify→date→sort→recent→active→max_tokens；`--active` 空/单块分支 | `commands/mod.rs:266-306` | 符合（补充见下）：`--json` 分支(:290-296) 提前返回，JSON 下永不打印 `No active session block found.` | `filter_blocks_by_date(&mut blocks, &shared); sort_blocks(&mut blocks, &shared.order);` / `if args.active && blocks.is_empty() { println!("No active session block found."); return Ok(()); }` |
| N43 | §6.6 | 默认 `SortOrder::Asc` → 默认最旧块在前，与 `docs/guide/blocks-reports.md` 的"newest (default)"冲突 | `types.rs:63-70`；`blocks.rs:177-182`；`docs/guide/blocks-reports.md:150` | 符合（存疑合理，未运行验证） | `order: SortOrder::Asc,` / `blocks.sort_by_key(\|block\| block.start_time); if *order == SortOrder::Desc { blocks.reverse(); }` / 文档 `# Show newest blocks first (default)` |
| N44 | §6.7 | 回看窗口 `[1,7,30]`；锚点：末条>now→全量；`now-last>=session`→无活跃块；否则取最后一段 `>session` 空隙之后；最终仍用 5 小时 | `commands/mod.rs:559`, `586-596`, `624-647`, `599-603` | 符合 | `const STATUSLINE_BLOCK_LOOKBACK_DAYS: [i64; 3] = [1, 7, 30];` / `.rposition(\|pair\| pair[1].duration_since(pair[0]) > session_ms)` / `identify_session_blocks(entries, DEFAULT_SESSION_DURATION_HOURS)` |
| N45 | §7.1 / §7.2 / **K11** | stdin JSON、空输入/解析失败文案、阈值校验、输出单行文本、参数无 `json`/`jq`/`order`/`compact`、未知项报错 | `commands/mod.rs:311-325`, `530-538`；`types.rs:156-171`；`parser.rs:224-260` | 符合 | `"🤖 {} \| 💰 {} session / {} today / {}{} \| 🧠 {}"` / `flag => return Err(format!("Unknown statusline option '{flag}'")),` |
| N46 | §7.1 | 默认阈值 50/80、`refresh_interval`=1 秒、`cache`=true | `types.rs:221-240` | 符合 | `refresh_interval: 1,` / `context_low_threshold: 50,` / `context_medium_threshold: 80,` |
| N47 | §7.3 / **K11** | 缓存路径 `env::temp_dir()/ccusage-semaphore/{session}.lock`；失效=过期或 mtime 变化；`isUpdating` 且 pid 存活时仍回旧输出；pid 存活在非 Unix 仅比较自身 | `commands/mod.rs:826-845`, `860-864`, `847-858` | 符合（补充：`last_output.is_empty()` 前置早退未提） | `env::temp_dir().join("ccusage-semaphore").join(format!("{session_id}.lock"))` / `if cache.is_updating && cache.pid.is_some_and(process_is_alive) { return Some(cache.last_output.as_str()); }` |
| N48 | §7.4 | 今日成本用 `format_date(now, args.timezone)` 写 since=until，再按 `entry.date` 精确过滤；上下文回退读 transcript 末条 assistant，窗口缺省 200_000 | `commands/mod.rs:442-453`, `541-555`, `713-772` | 符合 | `let today = format_date(now, args.timezone.as_deref()).replace('-', "");` / `total_input_tokens: input_tokens + cache_creation + cache_read,` / `.unwrap_or(200_000);` |
| N49 | §8.1 / §8.2 | `summary_json`/`session_summary_json`/`totals_json` 结构；totals 的 `totalTokens` 含 extra；`credits` 仅 >0 写入；`unpricedModels` 去重排序、空则不写 | `output.rs:27-121`, `126-145` | 符合（补充：`agent_summary_json` 另有 `messageCount`） | `"totalTokens": input.saturating_add(output)...saturating_add(extra),` / `if credits > 0.0 { value["credits"] = json!(credits); }` |
| N50 | §8.3 | `wants_json = json \|\| jq.is_some()`；`--no-cost` 递归删 `totalCost`/`costUSD`/`cost`；jq 子进程、非零退出报 `jq failed`；末句"连 `burnRate.cost` 一并移除" | `output.rs:14-16`, `158-187`, `564-581`；`blocks.rs:37-43`, `220-223`；`output.rs:746-819` | 主体符合，**末句不成立**：真实字段是 `costPerHour`，不被删除；测试里的 `burnRate.cost` 是自造键 | `map.remove("totalCost"); map.remove("costUSD"); map.remove("cost");` / `pub struct BurnRate { ... pub cost_per_hour: f64 }`（camelCase → `costPerHour`） |
| N51 | §8.4 / **K13** | `json_float` 整数值转 JSON 整数；只用于 block `costUSD`、all 报告成本、`summary_metadata` credits；summary/session/totals 用原始 f64 | `output.rs:452-462`；`blocks.rs:220`；`all/report.rs:169`, `196`；`all/loader.rs:813`；`output.rs:34`, `66`, `110` | 符合 | `if value.is_finite() && value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 { json!(value as i64) }` / `"totalCost": row.total_cost,`（summary_json 未走 json_float） |
| N52 | §9 / **K14** | 表格阈值 100（`USAGE_COMPACT_WIDTH_THRESHOLD`）、blocks 独立 120；非 TTY 不自动紧凑；宽度来源 `COLUMNS`/探测/缺省 120 | `output.rs:18-25`, `239-244`, `592-597`；`core/lib.rs:61`；`blocks.rs:20`, `343-348`；`terminal.rs:3-19` | 符合 | `shared.compact \|\| (is_stdout_tty && terminal_width < compact_width_threshold)` / `assert!(!should_use_compact_layout(&shared, false, 80, 100));` / `DEFAULT_TERMINAL_WIDTH: usize = 120;` |
| N53 | §9 | 列集合、`--no-cost` 弹末列、`format_models_multiline` 短名排序去重加 `- ` 前缀 | `output.rs:385-404`, `249-252`, `522-534` | 符合 | `headers.extend(["Cache Read", "Total Tokens", "Cost (USD)"]);` / `models.sort(); models.dedup();` |
| N54 | §10.2 | `AllAccumulator.models` 为 `BTreeSet`（按名升序）；`agent_breakdowns` 按 agent 升序；`model_breakdowns` 合并后按 cost 降序；行 agent 固定 `"all"` | `all/types.rs:72`, `113-119`, `178` | 符合 | `models: BTreeSet<String>,` / `agent_breakdowns.sort_by(\|a, b\| a.agent.cmp(b.agent));` / `breakdowns.sort_by(\|a, b\| b.cost.total_cmp(&a.cost));` |
| N55 | §10.3 | `--by-agent` 只影响 JSON 的 `agents` 键；表格始终展开子行；主行 `All`、子行 `- Claude` | `all/lib.rs:41`；`all/report.rs:148-156`, `238-255`, `412-434`, `597-620` | 符合（引用瑕疵）：草稿写"`parser.rs` 测试 `:644-645`"，该断言实际在 `crates/ccusage-cli-parser/src/tests.rs:644-645` | `format!("- {}", agent_label(row.agent))` / `} else if row.agent_breakdowns.is_some() { "All".to_string() }` / `assert_eq!(by_agent_error, "Unknown codex option '--by-agent'");`（tests.rs） |
| N56 | §10.4 | `summary_period` = date→week→month→session_id；all 的周二次归一固定周一；Codex/Claude 均用同一 `--timezone` 生成日期串 | `agent_report.rs:59-66`；`all/loader.rs:888-908`；`claude/src/lib.rs:524`；`codex/src/aggregate.rs:888` | 符合 | `row.date.as_deref().or(row.week.as_deref()).or(row.month.as_deref()).or(row.session_id.as_deref())` / `AgentReportKind::Weekly => crate::week_start(&row.period, WeekDay::Monday)` |
| N57 | 未验证 1 | blocks 默认排序与文档冲突（文档确实写 newest default） | `docs/guide/blocks-reports.md:150` | 符合（文档事实成立） | `# Show newest blocks first (default)` |
| N58 | 未验证 6 | `docs/guide/weekly-reports.md` 的 JSON 示例把 `modelBreakdowns` 写成对象映射 | `docs/guide/weekly-reports.md:138-149` vs `output.rs:36`、`types.rs:100-111` | 符合（指控成立） | 文档 `"modelBreakdowns": { "claude-opus-4-1-20250805": { ... } }`；代码 `"modelBreakdowns": row.model_breakdowns,`（数组） |

## 必须修正

1. **§8.3 末句（N50）**：删掉"连 `blocks[].burnRate.cost` 一并移除"这一说法，改为"`strip_cost_json` 递归删除键名恰为 `totalCost`/`costUSD`/`cost` 的字段；因此 blocks 的 `costUSD` 与 `projection.totalCost` 会被移除，但 `burnRate.costPerHour`（`blocks.rs:37-43` → `:222`）不会被移除，`--no-cost` 下的 blocks JSON 仍带 burn rate 成本"。`output.rs:746-819` 只是用自造键覆盖了 `cost` 与 `totalCost` 两种拼法，不能当作真实产物字段的证据。
2. **§3.4 末段（N24）**：执行顺序改为"`report_flag_alias_error`/`agent_filter_option_error`/`unsupported_agent_report_error`（`parser.rs:81-89`）→ `config_error`（:90-92）→ 根参数解析（:93-107）→ 命令解析（:109-122）→ `last_option_error`（:126-128）→ `date_window_error`（:129-131）"。
3. **§3.1 末句（N20）**：把"被报表与加载器共用"改为"adapter-common 与各加载器采用同一比较规则"，并注明 `blocks.rs:170-174`、`all/src/loader.rs:660-664`/`758-766` 是**内联实现**而非调用 `date_within_range`（这也解释了为什么改 `date_within_range` 不会影响这些路径）。
4. **§6.5（N40）**：补上 `project_block_usage` 的 `let burn = calculate_burn_rate(block)?;`（`blocks.rs:590`）——活跃块只有 1 条 entry 时 burn rate 为 `None`，于是既不输出 `projection` 也不输出 `tokenLimitStatus`；该条件与 `parse_token_limit` 的退化是两件独立的事。
5. **§6.1（N33 / K5）**：补上"配置项"来源：`blocks.session_length` 经 `ConfigContext::apply_blocks_args` 覆盖默认值（`config.rs:450-452`、`parser.rs:197`），配置层还支持 `active`/`recent`/`token_limit`。
6. **§10.3（N55）**：`Unknown codex option '--by-agent'` 的断言位于 `crates/ccusage-cli-parser/src/tests.rs:644-645`，不在 `parser.rs`；`session --id` 与 `--by-agent` 互斥的测试同理在 `tests.rs:629-635`。

## 建议补充

- §1.3 / §6.3 伪代码：`UsageAccumulator::add_entry` 与 `create_block` 取的字段是 `entry.data.message.usage`（`summary.rs:52`、`blocks.rs:124`），不是 `entry.usage`（`LoadedEntry` 无此字段）。
- §2.2：长度白名单里 `25` 只对应 `±HH:MM`；`25` 且 `bytes[19]=='Z'` 会走首分支随后在 `parse_timezone_offset`（`date_utils.rs:147-153`）失败。另外微秒/纳秒形式是长度 27/32 直接不匹配，不会进入"分隔符校验"。
- §7.3：`cached_statusline_output` 的第一个条件是 `cache.last_output.is_empty() → None`（`commands/mod.rs:832-834`），在过期判定之前。
- §6.6：`--active` 判空与单块详情分支都在 `wants_json` 之后（`commands/mod.rs:290-306`），所以 `--active --json` 空集只输出 `{"blocks": []}`，不会打印提示文本。
- §5：`run_session` 还丢弃四桶全 0 的行（`commands/mod.rs:155-157`），条件与 all 路径的 `total_tokens == 0 && total_cost == 0.0`（`all/src/loader.rs:787`）不同——后者保留"仅成本"行。
- §8.1：`agent_summary_json` 还会输出 `messageCount`（`agent_report.rs:24-26`）；`row_json` 在 `metadata` 为空时会写入 `{"agents":[...]}` 兜底（`all/report.rs:138-144`，聚合行 `metadata` 为 `None` 见 `all/types.rs:126`）。
- §8.1：`--sections` 的 `totals` 只统计命令自身 kind 的行（`all/report.rs:55-59`），不是全部 section 的汇总。
- §4.2：`ccusage claude weekly` 的起点除 CLI 默认 Sunday 外还可被配置覆盖（`parser.rs:524` → `config.rs:430-435`）。
- §1.6："顺序容器是 `BTreeMap`"宜改为"行来自 `BTreeMap`（key 唯一、字典序），`sort_by` 稳定，故同值行保持该顺序"。

## 无法验证

- blocks 默认排序的真实终端输出（未运行二进制；只能确认代码默认 `Asc` 与文档冲突）。
- `TZ` 环境变量在 Windows 上 jiff `system()` 的实际分支（仓库内无 `TZ` 读取，jiff 源码不在样本内）。
- `json_float` 遇 NaN/Inf 或超出 i64 时 `serde_json` 的最终产物（只核到分支条件）。
- statusline 缓存的文件系统 mtime 粒度导致的失效边界。
- `--active` 详情行采用 UTC 是否为有意设计（只能确认口径不一致的客观事实）。
- `ccusage-terminal/src/table.rs` 的日期压缩与列宽策略（未逐行核对）。
- 草稿其余文档引用（`session-reports.md:89-100`、`cli-options.md:272-281` 已抽核为真；其余未逐条打开）。

## 我实际打开过的文件

- `rust/crates/ccusage/src/blocks.rs:53-232, 234-330, 331-463, 465-618`
- `rust/crates/ccusage-core/src/date_utils.rs:1-260, 261-495`
- `rust/crates/ccusage-core/src/summary.rs:1-340, 380-560, 600-710`
- `rust/crates/ccusage-core/src/types.rs:1-219`
- `rust/crates/ccusage-core/src/output.rs:1-200, 200-409, 410-599, 730-819`
- `rust/crates/ccusage-core/src/agent_report.rs:1-75`
- `rust/crates/ccusage-core/src/model_aliases.rs:1-150`
- `rust/crates/ccusage-core/src/lib.rs:1-90`
- `rust/crates/ccusage-core/src/last_window.rs:1-116`
- `rust/crates/ccusage-core/src/utils.rs:1-60`
- `rust/crates/ccusage/src/commands/mod.rs:1-330, 331-660, 661-960, 1262-1301`
- `rust/crates/ccusage/src/cli.rs:1-51`
- `rust/crates/ccusage/src/cli/last_window.rs:1-189`
- `rust/crates/ccusage/src/cli/timezone.rs:1-151`
- `rust/crates/ccusage/src/main.rs`（grep `format_utc_minute`/`format_utc_second`:231-232）
- `rust/crates/ccusage-cli/src/types.rs:1-260`
- `rust/crates/ccusage-cli-parser/src/parser.rs:1-150, 150-300, 428-487, 505-594, 608-677, 1030-1179, 1181-1192`
- `rust/crates/ccusage-cli-parser/src/tests.rs`（grep `by-agent`:535-645）
- `rust/crates/ccusage-config/src/config.rs:425-624`
- `rust/crates/ccusage-adapter-all/src/lib.rs:1-101`
- `rust/crates/ccusage-adapter-all/src/loader.rs:390-504, 635-814, 815-908`
- `rust/crates/ccusage-adapter-all/src/report.rs:20-234, 235-444, 445-539, 565-620`
- `rust/crates/ccusage-adapter-all/src/types.rs:1-195`
- `rust/crates/ccusage-adapter-all/src/tests.rs`（grep `agents`/`by_agent`:36-1439）
- `rust/adapters/common/src/lib.rs:1-70`
- `rust/adapters/claude/src/lib.rs:45-164, 512-541`
- `rust/adapters/codex/src/aggregate.rs:860-919`
- `rust/crates/ccusage-terminal/src/terminal.rs:1-44`
- `rust/Cargo.lock:779-781`（jiff 0.2.38）
- `docs/guide/blocks-reports.md:150`、`docs/guide/weekly-reports.md:119-178`、`docs/guide/cli-options.md:265-294`、`docs/guide/session-reports.md:51-105`
