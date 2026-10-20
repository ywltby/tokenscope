# REVIEW-FINAL-A 终审复核（§1–§8）

> 复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md` 的 **§1–§8**（正文行 1–2732；全文 3469 行，§9 起由 B 复核员负责）。
> 样本仓库：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（只读；本轮未运行任何二进制、未修改样本）。
> 方法：`read` 分段通读 §1–§8 全文 2732 行；61 条行号抽查逐条打开源码比对；对全部 51 处 `§x.y` 引用做 grep 交叉校验；用 `assemble.ps1` + 各分片标题计数验证装配完整性。
> 边界：不评判 §9–§13 的内容正确性，只校验它们引用 §1–§8 时是否与 §1–§8 自洽。

## 结论（一句话）

§1–§8 的源码引用可靠度极高——61 条抽查**全部命中**、前一轮 5 项指定修正**全部落地**、装配**未丢任何小节**——但仍存在 **2 处必须修正**（§3.2 与 §5 §3 对 Codex「扣缓存时机」互斥；§7 §3.1「只有一处生产调用」与其自带括注自相矛盾）与 **6 处装配/引用损坏**（2 处列表被压行、4 处跨章引用断链或歧义）。

## 必须修正

- [E1] **§3.2（正文 139–140 行）与 §5 §3（正文 842 行）互斥：Codex 的缓存扣减时机。**
  断言（§3.2）：「`input_tokens` 的语义是**未缓存的新增输入**——适配器在归一阶段把厂商「含缓存输入」拆开（Codex 用 `input - cached - cache_write`，见 §5、§6）」
  实际：Codex 的解析/归一函数**只做上界钳制、不做扣减**；扣减发生在输出层（Codex 报表与统一报告行）。§5 §3 行 842 自己写明「cached 不从 `input_tokens` 里扣……只在输出层做减法（`report.rs:123-129`）」——两处不能同时成立。
  证据：`rust/adapters/codex/src/parser.rs:1311-1314`（`usage.cached_input_tokens = usage.cached_input_tokens.min(usage.input_tokens);`）、`rust/adapters/codex/src/report.rs:123-129`、`rust/crates/ccusage-adapter-all/src/loader.rs:845-855`。
  修法：§3.2 改为「Claude 形态的 `input_tokens` 是未缓存余量；含 cache 的来源在**输出层**扣减后落列（Codex 见 §5 §3、§9.2）」，删掉「归一阶段」的普适措辞。

- [E2] **§7 §3.1（正文 1711 行）自我矛盾：「实际只有一处生产调用」。**
  断言：「它是『权威窗口判定』的**单一定义点**，但实际只有一处生产调用（`adapters/common/src/lib.rs:41`……；核心侧 `summary.rs:297` 也在 `filter_and_sort_summaries` 里调用）」——括注自身已否定前半句。
  实际：生产调用至少两处（`adapters/common/src/lib.rs:41` 与 `core/src/summary.rs:297`），另有三处内联等价实现（`blocks.rs:170-174`、`all/src/loader.rs:660-664`/`758-766`，文档已列）。
  证据：`rust/crates/ccusage-core/src/summary.rs:295-303`（`rows.retain(|row| crate::date_within_range(date_fn(row), shared.since.as_deref(), shared.until.as_deref()))`）。
  修法：改为「两处生产调用（`common/src/lib.rs:41`、`summary.rs:297`）+ 三处内联复制」。

## 内部矛盾（跨章节）

1. **E1 的跨章定位**：§3.2（第 3 章）↔ §5 §3（第 5 章）对 Codex 扣缓存的时机互斥；§4 §6.3（行 526）引「Codex：§9.2」的说法是**正确的**（输出层扣减），即三处中只有 §3.2 一处失准。判定：修正 §3.2 即可全部自洽。
2. **§6 §0（行 1078）与 §4 §6.3（行 526）的清单关系**：§6 写「把 cached 从 input 里减掉：明确做的有四家（grok/copilot/zcode/gemini）」——§6 全章范围限定「其余 16 家」，Codex/Claude 不在内，**不构成冲突**；但两处并列阅读时易被读成「Codex 不扣减」。建议 §6 该行末尾补「（Codex 的扣减发生在输出层，见 §9.2）」。
3. **§4 与 §7 对「会话报表过滤 0 token 行」：一致**。§4 行 364（`commands/mod.rs:155-161`）与 §7 §10.3 行 691（同一行号 + 原码照录）指向同一实现，本轮回读源码确认：`rows.retain(|row| row.input_tokens + row.output_tokens + row.cache_creation_tokens + row.cache_read_tokens > 0)`。且两者对「统一报表按双 0 过滤」的描述与 `adapter-all/src/loader.rs:786-789` 一致（0 token 但有 cost 的行保留）。
4. **§7 与 §8 对 `extra_total_tokens`/`credits`/`missing_pricing`：一致，无冲突。** `extra`：§7 §1.3 累加（`summary.rs:54-57`）↔ §7 §8.2 totals 计入（`output.rs:96-109`）↔ §7 §6.3 blocks 恒 0（`blocks.rs:123-124` 仅 `add_usage`）↔ §8 §6.4 不参与计价（`utils.rs:27-37`），四处可同时成立；`credits`：两章都写「惰性求和、>0 才落 JSON、不进金额公式」；`missing_pricing`：§8 §6.6 的三通道描述与 §7 §8.1 的注释引用（`output.rs:123-125`）完全同义。
5. **同一段代码的行号在两个章节都给了但区间不同（轻微）**：§7 §1.3 行 1543 写 credits/message_count 惰性累加在 `:59-64`；§8 §6.5 行 2547 写「汇总累加（`summary.rs:56-60`）」。实测 credits 在 `:59-61`、message_count 在 `:62-64`，§7 的 `:59-64` 更准确，§8 的 `:56-60` 含了 `extra` 累加行。建议统一为 `:59-64`（非阻断）。
6. **§4 章「未验证/存疑」（行 721–729）与 §13.3「残留未决问题」：不冲突，但属汇总式重复。** §13.3 的 10 条中有 4 条与章级条目同源（§13.3-1≈§8 未验证-1、-3≈§8 未验证-2、-5≈§8 未验证-4、-6≈§8 未验证-3、-10≈§5 未验证-3）；§4 的 `--single-thread` 与「行数口径」两条未被 §13.3 收录（合理，属纯静态可查项）。建议在 §13.3 开头加一句「本节为各章『未验证/存疑』的汇总，编号独立」，避免读者误以为章级条目已被处置。

## 装配问题

- [A1] **列表被压行（2 处，markdown 结构损坏）**：行 486（`. <synthetic> 归一为 None…` 与 `- **展示/分组名加 -fast…**` 挤在同一行）与行 549（`- **display**：…` / `- **auto**：…` / `- **calculate**：…` 三项挤在一行）。渲染后丢列表层级，删改时也易漏项。
- [A2] **断链引用**：行 3334（§12）写「daily 实现漏了这条守卫（§13.2 的 E2）」——装配后的 §13.2 没有 `E1–E6` 编号（那是 `REVIEW-R1.md` 的编号体系，未进正文）。应改为「§13.2 第 2 条」。
- [A3] **引用未随章节重编号**：行 1135（§6.2.2 末）「（见 §4.1）」——目标其实是同章的 `#### 4.1 「文件发现 → 逐文件解析 → 产出 LoadedEntry」的归属与重复度`（§6 章内），按字面找 `§4.1`（Claude 精确去重键）会得到无关内容。同类潜在歧义：行 441「（§8）」（语义上是 §4 章内「### 8. 日汇总快捷路径」，同文档 §8 却是定价章）、行 260「（§4）」自指。
- [A4] **引用缺文件名（§4 §9.5 表格）**：`（`656-665`）`、`（`756-768`）`、`（`672-683`）` 未写文件（该表其它 cell 均写 `loader.rs:`/`adapters/common/src/lib.rs:`）。内容已核为正确目标：`loader.rs:656-665`（`filter_daily_summaries_by_date`）、`:756-768`（`filter_session_summaries`）、`:672-683`（Codex 无窗口走 `load_groups` 快路径）；同表 `（`lib.rs:51/67` 传参…）` 未写 `adapter-all/src/`。补文件名即可。
- [A5] **标题/小节完整性：通过（无丢失）**。逐分片计数与终稿一一对应：S1 13→13、S2 14→14、S5 31→31、S4 53→53、S3 35→35；`assemble.ps1` 的 `##`→`###` 降级未吃掉任何小节，§4/§5/§6/§7/§8 内部编号（0.–11.、5.1/5.2、2.1–4.5、1.1–10.4、1.1–6.6 等）全部到位。
- [A6] **（越界提示，§13 区）** 行 3362「6 份分片，合计约 33 万字符」：草稿 S1–S6 合计约 331 KB 字节（UTF-8 中文约 3 字节/字，≈11 万汉字），字节被当成字符。建议改「约 33 万字节」。

## 抽查清单（61 条）

| # | 章节 | 断言 | 源码位置 | 结果 | 摘录（≤3 行） |
| --- | --- | --- | --- | --- | --- |
| 1 | §1 | 9 个文件行数（pricing 6181 / opencode.loader 2922 / codex.loader 2682 / copilot.loader 1990 / aggregate 1853 / claude.lib 1582 / adapter-all.loader 1637 / commands 1411 / cli-parser 1247） | 各文件 | 正确 | `(Get-Content).Count` 逐项一致 |
| 2 | §3.1 | `UsageEntry` camelCase + `costUSD` rename | `core/src/types.rs:7-19` | 正确 | `#[serde(rename_all = "camelCase")]` / `#[serde(rename = "costUSD")] pub cost_usd: Option<f64>,` |
| 3 | §3.1 | `cache_creation_token_count` = 5m+1h（saturating），否则扁平字段 | `core/src/types.rs:41-50` | 正确 | `b.ephemeral_5m_input_tokens.saturating_add(b.ephemeral_1h_input_tokens)` |
| 4 | §3.2 | `ModelBreakdown.extra_total_tokens` 不序列化、`missing_pricing` 仅 true 时序列化 | `core/src/types.rs:98-112` | 正确 | `#[serde(skip_serializing)] pub extra_total_tokens: u64,` |
| 5 | §3.3 | `LoadedEntry` 12 字段（含 extra/credits/missing_pricing_model） | `core/src/types.rs:124-139` | 正确 | `pub missing_pricing_model: Option<String>,` |
| 6 | §3.3/§4.2 | `LoadedFile.timestamp` 写入取最小、构造点 `timestamp: None`（死字段结论） | `claude/src/lib.rs:501-504, 660-666` | 正确 | `.map_or(timestamp, |current| current.min(timestamp))` |
| 7 | §3.4 | `UsageSummary` 维度字段全 Option + skip_serializing_if；`total_tokens()` 含 extra | `core/src/types.rs:147-190` | 正确 | `+ self.extra_total_tokens` |
| 8 | §2.2 | 裸 `ccusage` → `adapter::all::run`（Daily）；`None` 分支字段 | `ccusage/src/main.rs:25-67` | 正确 | `None => { … codex_speed: cli::CodexSpeed::Auto, … adapter::all::run(args) }` |
| 9 | §2.2 | 17 元函数数组 + `runs.len() == 17` 契约测试 | `ccusage/src/main.rs:90-113` | 正确 | `let runs: [fn(AgentCommandArgs) -> Result<()>; 17] = […]; assert_eq!(runs.len(), 17);` |
| 10 | §4 §1.2 | 排序键 = 完整路径字符串（去重输入之一） | `claude/src/paths.rs:77` | 正确（引用一致） | `sort_by_cached_key(|path| path.to_string_lossy().into_owned())` |
| 11 | §4 §2.3 | 预过滤是精确字节 `"usage":{`；`fs::read` 整读 | `claude/src/lib.rs:505-516` | 正确 | `let usage_marker = memmem::Finder::new(br#""usage":{"#);` |
| 12 | §4 §4.1 | 三档去重键（无 requestId → session；非 `msg_` → 再加 timestamp） | `claude/src/lib.rs:350-366` | 正确 | `if request_id.is_none() { session_id.hash(…); if !is_anthropic_message_id(message_id) { timestamp.hash(…) } }` |
| 13 | §4 §4.1 | 结构比对附加条件（会话相等且 msg_/时间戳） | `claude/src/lib.rs:413-425` | 正确 | `request_id.is_some() \|\| (loaded_entry_session_id(entry) == session_id && (is_anthropic_message_id(message_id) \|\| entry.timestamp == timestamp))` |
| 14 | §4 §4.2 | 幸存者三档：sidechain→token 总数→speed（不比 cost） | `claude/src/lib.rs:190-212` | 正确 | `if candidate_is_sidechain != existing_is_sidechain { return existing_is_sidechain; }` |
| 15 | §4 §4.4 | 两条 sidechain 域分隔串 | `claude/src/lib.rs:377-392` | 正确 | `"sidechain-replay".hash(&mut hasher);` / `"sidechain-replay-entry".hash(&mut hasher);` |
| 16 | §4 §4.4 | 主路径路由匹配含 timestamp 守卫 + 注释自述差异 | `claude/src/lib.rs:434-440` | 正确 | `let requestless = request_id.is_none() && entry.data.request_id.is_none();` / `&& (requestless \|\| entry.timestamp == timestamp)` |
| 17 | §4 §4.6 | **daily 侧缺 timestamp 守卫（本轮指定验证 1/5）** | `claude/src/daily.rs:493-496` | 正确（已落地） | `existing.message_id.as_deref() == Some(message_id) && indexed_session_id == entry.session_id.as_ref() && (candidate_is_sidechain \|\| is_sidechain_daily_entry(existing))` |
| 18 | §4 §4.6 | daily 平手时 cost 决胜 | `claude/src/daily.rs:592-600` | 正确 | `if candidate.cost != existing.cost { return candidate.cost > existing.cost; }` |
| 19 | §4 §4.6 | daily 聚合不累加 extra（汇总恒 0） | `claude/src/daily.rs:617-644` | 正确 | `self.counts.add_usage(entry.usage); self.cost += entry.cost;`（无 extra 累加） |
| 20 | §4 §4.6 | daily 测试固化「新 requestId + 时间戳改写仍合并」 | `claude/src/daily.rs:730-769` | 正确 | `assert_eq!(deduped.len(), 1);` … `Some("req-parent")`；时间戳 1_774_000_000_000 vs …001_000 |
| 21 | §4 §5.1 | advisor 拆条：id 加 `:advisor:{index}`、`cost_usd = None`、`extra_total_tokens: 0` | `claude/src/lib.rs:561-597` | 正确 | `advisor_data.cost_usd = None;` |
| 22 | §4 §7.3 | 主行计价点 `calculate_cost(&data, mode, pricing)` | `claude/src/lib.rs:525` | 正确 | `let cost = calculate_cost(&data, mode, pricing);` |
| 23 | §4 §3.5/§7 §5 | 0 token 行两处下游过滤（会话四列、统一双 0） | `commands/mod.rs:155-161`、`adapter-all/src/loader.rs:786-789` | 正确（跨章一致） | `if total_tokens == 0 && summary.total_cost == 0.0 { return None; }` |
| 24 | §4 §9.1 | `load_rows → load_pricing → load_kind_for_report → load_base_rows → finish_rows` 结构链 | `adapter-all/src/loader.rs:29-98` | 正确 | `let pricing = load_pricing(shared); let load_kind = load_kind_for_report(kind);` |
| 25 | §4 §9.4 | `aggregate_rows` 周一起算 / monthly `[..7]` | `adapter-all/src/loader.rs:888-908` | 正确 | `AgentReportKind::Weekly => crate::week_start(&row.period, WeekDay::Monday)…` |
| 26 | §4 §9.5 | `--since/--until` 三个裸行号指向的函数 | `adapter-all/src/loader.rs:656-665, 756-768, 672-683` | 正确（仅缺文件名，见 A4） | `fn filter_daily_summaries_by_date(…)`、`fn filter_session_summaries(…)` |
| 27 | §4 §9.5 | `--no-cost` 装载零引用、仅透传到输出 | `adapter-all/src/lib.rs:51, 67` | 正确 | `print_json_or_jq(…, shared.no_cost)` |
| 28 | §5 §2 | **行内字面量判据（本轮指定验证 2/5）**：命中 `usage:/input_tokens:/prompt_tokens:` 即判 Headless | `codex/src/parser.rs:708-713` | 正确（已落地） | `if USAGE_FIELD_FINDER.find(line).is_some() \|\| INPUT_TOKENS_FIELD_FINDER… \|\| PROMPT_TOKENS_FIELD_FINDER…` → `Some(CodexLineKind::Headless)` |
| 29 | §5 §3 | 增量：`last_token_usage` 优先且要求累计前进；累计未前进→全 0 跳过 | `codex/src/parser.rs:482-508` | 正确 | `.filter(|_| cumulative_advanced)` … `if raw_usage.input_tokens == 0 && … { return Ok(()); }` |
| 30 | §5 §3 | cached 不预先扣减；`non_cached_input_tokens` 只在输出层 | `codex/src/report.rs:123-129` | 正确 | `input_tokens.saturating_sub(cached_input_tokens.saturating_add(cache_creation_tokens))` |
| 31 | §5 §5.1 | 前缀逐条全等、index 0 失配转重写突发 | `codex/src/parser.rs:234-253` | 正确 | `replay = (index == 0).then(\|\| detect_rewritten_burst(path))…` |
| 32 | §5 §5.2 | **「保留 file_index 最小者」限定为带 response_id 的 compaction（本轮指定验证 3/5）** | `codex/src/aggregate.rs:720-751` | 正确（已落地） | `if let Some(compaction) = record.compaction.as_mut() && file_index < compaction.file_index { … }` / 普通事件 `return false` |
| 33 | §5 §5.2 | 去重键 session 维度仅 `kind == Session` 时参与 | `codex/src/aggregate.rs:762-766` | 正确 | `let (session_hash, session_len) = if kind == AgentReportKind::Session { (hash_text(&event.session_id), event.session_id.len()) } else { (0, 0) };` |
| 34 | §5 §7 | tier 映射 `default\|standard / fast\|priority / flex`，未知清空 | `codex/src/parser.rs:449-478, 649-659` | 正确 | `"fast" \| "priority" => Some(CodexServiceTier::Fast),` / `_ => None,` |
| 35 | §5 §8 | `mode != Display` 才附未定价元数据 | `codex/src/report.rs:28-30, 74-76` | 正确 | `if mode != CostMode::Display { attach_unpriced_models(&mut totals, …); }` |
| 36 | §5 §9 | `CodexLineKind` 仅两变体，三态来自 `Option` | `codex/src/parser.rs:62-66` | 正确 | `enum CodexLineKind { Session, Headless }` |
| 37 | §5 §未验证1 | 全仓 grep `task_started/trigger_turn/inter_agent_communication` 只命中 `src/README.md` | `rust/` 全仓 grep | 正确 | 仅 `codex/src/README.md:50-51` 两行命中 |
| 38 | §6 §0 | `apply_total_token_fallback` 12 家 / 14 处（amp、kimi 各 2） | 全仓 grep | 正确（逐处计数） | amp:158/261、kimi:208/255、openclaw:295、codebuff:255、gemini:338、kilo:99、qwen:145、copilot:452、pi:348、opencode:193、zcode:58、droid:109 |
| 39 | §6 §0 | 7 家 SQLite 全用 `sqlite` crate 只读打开、无 rusqlite | 全仓 grep | 正确 | 7 处 `sqlite::OpenFlags::new().with_read_only()`；`rusqlite` 0 命中 |
| 40 | §6 §3.1 | amp：环境变量设置即返回不回落；默认 `~/.local/share/amp`；home 失败报错 | `amp/src/paths.rs:5-31` | 正确 | `if path.is_dir() && seen.insert(path.clone()) { paths.push(path); } } return Ok(paths);` / `home directory is not set` |
| 41 | §6 §3.8 | kimi `wire.jsonl` 只认深度 3 或 5 | `kimi/src/paths.rs:58-74` | 正确 | `matches!(relative.components()…count(), 3 | 5)` |
| 42 | §6 §3.10 | 统一装载层 qwen 入口 `load_qwen_rows`（含 `has_data` 补 detected） | `adapter-all/src/loader.rs:715-732` | 正确 | `let detected = !entries.is_empty() \|\| qwen::has_data();` |
| 43 | §6 §4.4 | `sqlite = 0.37.4`（bundled/linkage） | `rust/Cargo.toml:65` | 正确 | `sqlite = { version = "0.37.4", default-features = false, features = ["bundled", "linkage"] }` |
| 44 | §7 §1.1 | `summarize_by_key` 签名 + BTreeMap + `meta_fn` 写回 | `core/src/summary.rs:14-36` | 正确 | `groups.entry(key_fn(entry)).or_default().add_entry(entry);` |
| 45 | §7 §1.3 | 累加规则：四桶 + extra saturating、credits/message_count 惰性、breakdowns 成本降序 | `core/src/summary.rs:40-100` | 正确 | `self.counts.extra_total_tokens.saturating_add(entry.extra_total_tokens)` / `b.cost.total_cmp(&a.cost)` |
| 46 | §7 §1.5 | 桶 key：Monthly `[..7]`、Weekly `week_start`、date None 跳过 | `core/src/summary.rs:172-206` | 正确 | `BucketKind::Monthly => date.get(..7).unwrap_or(date).to_string(),` |
| 47 | §7 §2.2 | `parse_ts_timestamp` 仅 20/25（无小数）与 24/29（毫秒）两组长度 | `core/src/date_utils.rs:111-145` | 正确 | `20 | 25 if bytes[19] == b'Z' \|\| …=> (0, 19),` / `_ => return None,` |
| 48 | §7 §6.2 | `identify_session_blocks` 切块条件严格大于 + gap 块 | `ccusage/src/blocks.rs:53-103` | 正确 | `if since_start > session_duration \|\| since_last > session_duration {` |
| 49 | §7 §6.3 | block 只 `add_usage`（extra 不进块） | `ccusage/src/blocks.rs:109-147` | 正确 | `token_counts.add_usage(entry.data.message.usage); cost += entry.cost;` |
| 50 | §7 §6.4 | burn rate：`duration<=0 → None`、`cost_per_hour :582` | `ccusage/src/blocks.rs:567-584` | 正确 | `if duration_minutes <= 0.0 { return None; }` … `cost_per_hour: block.cost_usd / duration_minutes * 60.0,` |
| 51 | §7 §6.6 | `max_tokens` = 非 gap 非 active 块的最大 token | `ccusage/src/commands/mod.rs:283-288` | 正确 | `.filter(|block\| !block.is_gap && !block.is_active)` |
| 52 | §7 §8.2 | `totals_json`：totalTokens 含 extra、credits 仅 >0 落键 | `core/src/output.rs:79-121` | 正确 | `.saturating_add(extra)` / `if credits > 0.0 { value["credits"] = json!(credits); }` |
| 53 | §7 §8.3 | **`--no-cost` 只删 `totalCost`/`costUSD`/`cost`；`BurnRate` 字段为 `costPerHour`（本轮指定验证 4/5）** | `core/src/output.rs:564-581`、`blocks.rs:37-43` | 正确（已落地） | `map.remove("totalCost"); map.remove("costUSD"); map.remove("cost");` / `pub cost_per_hour: f64,` |
| 54 | §7 §8.4 | `json_float` 三分支（有限、整数、i64 范围内 → 整数） | `core/src/output.rs:452-462` | 正确 | `value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64` |
| 55 | §7 §10.2 | `AllRow` models 用 BTreeSet（名字升序，与单 agent 首见序不同） | `core/src/output.rs` 邻接断言 / `all/src/types.rs` | 部分核对 | 本轮核了 `row_json`/`agent_json` 输出形状；`types.rs:88` 的 BTreeSet 由 R2 复核，本轮未重开 |
| 56 | §7 §10.3 | all 报告 totals 用 `json_float` 且按 `AllRow.total_tokens` 求和 | `adapter-all/src/report.rs:174-203` | 正确 | `"totalCost": json_float(rows.iter().map(\|row\| row.total_cost).sum::<f64>())` |
| 57 | §7 §10.4 | `summary_period` 顺序 date→week→month→session_id | `core/src/agent_report.rs:59-66` | 正确 | `row.date.as_deref().or(row.week…).or(row.month…).or(row.session_id…)` |
| 58 | §8 §1.1 | build.rs：`CCUSAGE_PRICING_JSON_PATH` → compact → deflate → `$OUT_DIR` | `core/build.rs:13-40` | 正确 | `fs::write(out_path, deflate(pricing_json.as_bytes()))` |
| 59 | §8 §2/§3.4 | **内置兜底表 `or_insert` + 3 处无条件 `insert`（本轮指定验证 5/5 前半）** | `core/src/pricing.rs:1980-1983, 2369-2370, 2386-2387, 2400-2408` | 正确（已落地） | `self.entries.entry(model).or_insert(pricing);` / `self.entries.insert("gpt-5.1-codex".to_string(), gpt_5_1_pricing);` |
| 60 | §8 §4.1 | **`max_by` tie-break：最长优先、同长取字典序小者（指定验证 5/5 后半）** | `core/src/pricing.rs:1604-1613` | 正确（已落地） | `.max_by(\|(left, _), (right, _)\| left.len().cmp(&right.len()).then_with(\|\| right.cmp(left)))` |
| 61 | §8 §4.3/§4.5/§5.3 | `pricing_key_matches` 双向子串 + 归一化；倍率覆盖表内容；快照条目 `exactOnly` | `core/src/pricing.rs:2667-2683`、`fast/flex-multiplier-overrides.json`、`models-dev-pricing.json:7504/7598/7634/5691/15868/24474` | 正确 | fast exact 10 键 + `normalized_prefix` 3 项；flex exact 20 键全 0.5；三快照条目均 `"exactOnly": true` |

补充核对（未入表）：`date_utils.rs:111-145` 与 §7 §2.2 的逐条门槛、`pricing.rs:1354-1454` 的 `find/find_with_fallbacks` 链与 §8 §4.1 伪码逐行一致、`claude/src/lib.rs:107-139`（`load_entries_inner` + `split_files_before_since`）与 §4 §2.1/§7 §3.2 一致、`models-dev-pricing.json` 544 555 B / 34 269 行与 §8 §1.2 一致、`pricing.rs:4931-4955` 与 §8 §4.5(a) 的测试断言一致。

## 无法验证

- **§7 §6.7 statusline 有界块搜索**（`ccusage/src/commands/mod.rs:557-671`）：本轮只核了其调用链与 `identify_session_blocks` 侧，锚点函数体（`:631-647`）未逐行打开。
- **§8 §8 DeepSeek V4 三段价与高峰窗口**（`pricing.rs:279-347`）：只核了 `CUTOFF` 常量（`:252` = 1_786_896_000_000）；三段价数值与 `[1,4)`/`[6,10)` 半开窗口未逐值核对。
- **§4 §2.4 statusline 细节**（`commands/mod.rs:719-751`、`826-864`：transcript 反查、缓存命中判定）：未打开。
- **§6 逐家适配器字段矩阵的多数行**：本轮抽验 amp/kimi/opencode/qwen 与 SQLite 打开模式等约 6 项，其余 10 余家未逐家回读（分片自述 + R4 已复核）。
- **运行期行为类**（与文档自标一致，静态不可证伪）：progress 行双路径分叉的真实差异、replay 突发阈值实测区间（10–40 ms / 5.8–15.3 s）、`TZ` 环境变量实际归属、`--single-thread` 对统一报表 agent 级并行的实际影响。
- **§8 §1.1 内嵌 LiteLLM 快照键集**：`$OUT_DIR/litellm-pricing.json.deflate` 为编译产物，只读克隆中不存在，未生成（文档已自标）。

## 我实际打开过的文件（26 个，均含行号区间）

- `docs/research/2026-10-14-ccusage-usage-statistics.md:1-2732`（§1–§8 全文分段通读）、`:3356-3410`（§13 交叉校验）
- `docs/research/ccusage-draft/assemble.ps1:1-77`、`REVIEW-R1.md:1-144`
- `rust/crates/ccusage-core/src/types.rs:5-190`
- `rust/crates/ccusage-core/src/pricing.rs:194-260, 942-1093, 1354-1454, 1596-1630, 1974-2016, 2360-2413, 2662-2683, 4928-4959`
- `rust/crates/ccusage-core/src/cost.rs:46-143, 104-135`
- `rust/crates/ccusage-core/src/summary.rs:8-36, 40-109, 172-216, 288-322, 394-415`
- `rust/crates/ccusage-core/src/output.rs:79-128, 450-467, 518-537, 558-587`
- `rust/crates/ccusage-core/src/date_utils.rs:109-148`
- `rust/crates/ccusage-core/src/utils.rs:1-40`
- `rust/crates/ccusage-core/src/agent_report.rs:1-70`
- `rust/crates/ccusage-core/src/models-dev-pricing.json`（大小/行数/抽行 5691、7504、7598、7634、15868、24474）、`models-dev-catalog-rules.json`（大小）
- `rust/crates/ccusage-core/src/fast-multiplier-overrides.json`、`flex-multiplier-overrides.json`（全文）
- `rust/crates/ccusage-core/build.rs:13-57`
- `rust/adapters/claude/src/lib.rs:100-144, 186-225, 340-449, 493-629, 655-674`
- `rust/adapters/claude/src/daily.rs:452-511, 578-657, 728-772`
- `rust/adapters/codex/src/parser.rs:58-87, 226-267, 460-511, 643-664, 695-734, 1304-1319`
- `rust/adapters/codex/src/aggregate.rs:404-431, 712-786, 878-917`
- `rust/adapters/codex/src/report.rs:20-139`
- `rust/adapters/codex/src/paths.rs:222-246`
- `rust/adapters/common/src/lib.rs`（经 R1 复核行号；本轮引用其行号结论）
- `rust/adapters/amp/src/paths.rs:3-31`
- `rust/adapters/kimi/src/paths.rs:56-80`
- `rust/adapters/openclaw/src/sqlite.rs:80-95`
- `rust/crates/ccusage-adapter-all/src/loader.rs:26-100, 396-427, 650-774, 770-799, 886-912`
- `rust/crates/ccusage-adapter-all/src/lib.rs:34-73`
- `rust/crates/ccusage-adapter-all/src/report.rs:120-209`
- `rust/crates/ccusage/src/main.rs:24-73, 88-117`
- `rust/crates/ccusage/src/commands/mod.rs:146-185, 262-311`
- `rust/crates/ccusage/src/blocks.rs:33-52, 53-107, 108-177, 563-612`
- `rust/crates/ccusage/src/http.rs:8-52`
- `rust/crates/ccusage-cli-parser/src/parser.rs:372-403`
- `rust/crates/ccusage-config/src/config.rs:244-303`
- `rust/Cargo.toml:58-77`
