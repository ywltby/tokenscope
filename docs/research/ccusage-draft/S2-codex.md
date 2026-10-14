## S2 Codex 适配器

- 仓库：`ccusage`（只读样本）HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31
- 证据约定：所有 `path:line` 相对 ccusage 仓库根（`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`），行号取实际文件行号（`read` 工具核对；`Measure-Object -Line` 会少算末行，不采用）
- 方法：静态阅读（`read` / `grep` / `glob`）为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5），未修改任何样本
- 覆盖：`rust/adapters/codex/src/{loader,parser,aggregate,types,replay,report,speed,paths,lib,compaction_tests}.rs`、`rust/adapters/codex/{README.md,src/README.md}`、`rust/adapters/common/src/lib.rs`、`rust/crates/ccusage-core/src/{pricing,model_aliases}.rs`、`rust/crates/ccusage-adapter-all/src/{loader,report}.rs`、`rust/crates/ccusage-cli*/src/*`、`docs/guide/codex/index.md`

### 0. 结论速览

| 问题 | 结论 | 关键证据 |
| --- | --- | --- |
| 数据根 | `CODEX_HOME`（逗号分隔多根）> `~/.codex`；每根独立探测 `sessions/` 与 `archived_sessions/`，都没有才把根本身当 JSONL 目录 | `rust/adapters/codex/src/paths.rs:31`, `paths.rs:226` |
| 文件发现 | 递归 + **仅**扩展名等于 `jsonl` 的普通文件；无 zstd/gzip 支持 | `rust/adapters/common/src/lib.rs:14`, `lib.rs:18` |
| 增量/偏移 | 无状态文件、无读取偏移；每次全量重扫重解析（`--cache/--no-cache` 只作用于 statusline） | `rust/adapters/codex/src/paths.rs:85`, `rust/crates/ccusage/src/commands/mod.rs:327` |
| 事件模型 | 顶层 `{type,timestamp,payload}`；只认 `turn_context` / `compacted` / `token_usage_record` / `event_msg{token_count,thread_settings_applied}` 与 headless usage 行 | `rust/adapters/codex/src/parser.rs:661`, `parser.rs:342` |
| 增量语义 | `last_token_usage` 优先（且要求累计值确实前进），否则 `total - previous_total` 差分 | `rust/adapters/codex/src/parser.rs:482` |
| cached 处理 | 内部保留「总 input」，输出层 `inputTokens = input - cached - cache_creation` | `rust/adapters/codex/src/report.rs:123` |
| reasoning | 已含在 output 内，不再叠加；`total` 推导为 `input + output`（不含 reasoning） | `rust/adapters/codex/src/types.rs:347` |
| 回退模型 | 无模型 → `gpt-5` + `isFallback=true`；`codex-auto-review` → 按日期表查真模型 | `rust/adapters/codex/src/parser.rs:813`, `parser.rs:833` |
| replay 修正 | fork/子会话的前缀按「与父日志 usage 逐条完全相等」消费掉；父日志缺失时退化为「重写突发」启发式，阈值 1000 ms | `rust/adapters/codex/src/parser.rs:122`, `parser.rs:234` |
| compaction 修正 | `token_usage_record` 只在被 `compacted` 的 `compaction_response_id` 认领后计入；跨文件按 `response_id` 去重 | `rust/adapters/codex/src/parser.rs:363`, `aggregate.rs:753` |
| 速度 | 日志内 `thread_settings_applied` 逐事件生效；auto 的未分类部分查各根 `config.toml`；倍率在成本函数里放大 | `rust/adapters/codex/src/speed.rs:33`, `rust/adapters/codex/src/report.rs:272` |
| 成本 | Codex 日志无任何 cost 字段，`costUSD` 全部自算；`--mode` 只影响未定价披露，不影响 Codex 金额 | `rust/adapters/codex/src/report.rs:63`, `report.rs:709` |
| json/jsonl | 不存在独立 JSON（非 JSONL）链路；两条链路是「行格式」分叉（rollout 行 vs `codex exec --json` 行） | `rust/adapters/codex/src/parser.rs:280`, `parser.rs:287` |

### 1. 数据根与文件发现

根解析（`paths.rs:226-238`）：`CODEX_HOME` 存在即按其逗号切分（去空），完全忽略 `~/.codex`；否则取 `home_dir()/.codex`。

```rust
pub(super) fn codex_home_paths() -> Result<Vec<PathBuf>> {
    if let Ok(env_paths) = env::var("CODEX_HOME") {
        return Ok(env_paths.split(',').map(str::trim)
            .filter(|path| !path.is_empty()).map(PathBuf::from).collect());
    }
    let home = home::home_dir().ok_or_else(|| cli_error("home directory is not set"))?;
    Ok(vec![home.join(".codex")])
}
```

每根展开成 source（`paths.rs:31-64`）：`sessions/` 与 `archived_sessions/` 各自独立判断目录存在性（只有 `archived_sessions/` 的根也能贡献数据）；两者都不存在时把**根本身**当作扫描目录（`paths.rs:56-61`），官方文档把它对应为「保存 `codex exec --json` 输出的目录」（`docs/guide/codex/index.md:26`）。

文件收集 = 递归目录 + 扩展名等值比较（`rust/adapters/common/src/lib.rs:14-34`）。`collect_usage_files` 只传 `"jsonl"`，因此：

- **`.jsonl.zst` 不被支持**：`path.extension()` 对 `x.jsonl.zst` 得到 `zst`，与 `jsonl` 不等，直接被过滤掉。全仓 grep `zstd` 无匹配；`ccusage-adapter-codex` 依赖表里也没有任何压缩 crate（`rust/adapters/codex/Cargo.toml:7-16`，`flate2` 只在 `ureq` HTTP 栈里出现）。
- **没有 `.json` 扩展名链路**（见 §9）。

排序与去重：`files.sort()`（`paths.rs:85-90`，路径字典序），跨 source 的去重键是 `(dedupe_scope, 相对路径)`（`paths.rs:221-224`），因此同一个 Codex home 下 `sessions/` 与 `archived_sessions/` 的相对同名文件只保留先出现的 `sessions/` 副本（`paths.rs:316-340` 有测试固定该行为）。

日期预过滤（`paths.rs:110-128`）：只有给了 `--since/--until` 才启用；先用相对路径里的 `YYYY/MM/DD` 三元组当文件日期（`paths.rs:189-201`），再从 `--until` 推出 UTC 侧上界（`paths.rs:171-175`）。无法判定的文件**保留**（`paths.rs:158-168` + `paths.rs:417-430` 测试），并用文件 mtime 兜底一个长会话跨月恢复的场景（`paths.rs:387-414`）。

增量读取：**没有**任何偏移记录/状态文件。每次调用都是 `collect → filter → 全文件解析`（`loader.rs:124-148`、`aggregate.rs:142-173`）。`CodexAggregateRun.file_offset`（`aggregate.rs:72`, `aggregate.rs:117-136`, `aggregate.rs:264`）只是「全局文件序号」，用于 compaction 归属的确定性排序，不是文件读取偏移。`--cache/--no-cache` 在整仓只被 statusline 路径消费（`rust/crates/ccusage/src/commands/mod.rs:327`），与 Codex 用量解析无关。

并行策略：父日志/元数据扫描按**文件数**分块（`replay.rs:262-307`），事件解析与父 usage 读取按**字节大小**分块以求负载均衡（`loader.rs:170-217`、`replay.rs:327`、`rust/adapters/common/src/lib.rs:49-81`）；`single_thread` 与 `--json` 会退化成串行/单 source 路径（`aggregate.rs:80-84`、`loader.rs:156-160`）。

### 2. 事件模型

行信封（`types.rs:129-141`）：顶层 `type` / `timestamp` / `payload`；`payload` 用宽松反序列化（`deserialize_optional_object_lossy`，`types.rs:358-450`），字段类型异常时退化为 `None` 而不报错整行。`payload` 可承载的键（`types.rs:190-224`）：`type`、`info`、`model`、`model_name`、`response_id`、`compaction_response_id`、`usage`、`thread_token_usage`、`metadata`、`thread_settings`。

识别的行类型（分派入口 `visit_codex_session_entry`，`parser.rs:328-555`）：

- `turn_context` → 只做一件事：把 `payload.model`（或 `model_name` / `metadata.model`，`parser.rs:906-963`）记成「当前模型」，并清掉 fallback 标记（`parser.rs:448-454`）。
- `event_msg` + `payload.type == "thread_settings_applied"` → 更新当前 service tier（`parser.rs:464-478`）。
- `event_msg` + `payload.type == "token_count"` → 产出普通用量事件（`parser.rs:479-554`）。
- `token_usage_record` → 产出带 `response_id` 的 compaction 用量事件，但只在被 `compacted` 认领后才真正上报（`parser.rs:363-447`）。
- `compacted` → 记录/触发 compaction 认领（`parser.rs:342-362`）。

`session_meta` **不在**该函数里处理，只在 replay 阶段被读**首行**用来取身份（`replay.rs:391-428`）：`payload.id` = session id、`payload.forked_from_id`、`payload.source.subagent.thread_spawn.parent_thread_id`、顶层 `timestamp`。它是父日志定位的唯一依据（`replay.rs:218-249`）。

注意一个容易踩的差异：**产出事件的 `session_id` 不是** `session_meta.payload.id`，而是由相对路径去掉扩展名拼出来的（`parser.rs:892-904`）：

```rust
fn codex_session_id(sessions_dir: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(sessions_dir).unwrap_or(path);
    let mut session_id = relative.with_extension("").components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>().join("/");
    if session_id.is_empty() { session_id = "unknown".to_string(); }
    session_id
}
```

所以 `sessionId` 形如 `2026/09/19/rollout-2026-09-19T23-22-40-01a0bb8c-...`；CLI 查会话时兼容完整 id、文件名、`codex://threads/<uuid>`，并支持按 UUID 后缀匹配、歧义则报错（`rust/adapters/codex/src/lib.rs:69-116`）。

行预过滤（性能闸门）：先用 `memchr` 找 `"type":"event_msg"` 之类的字面量，再决定要不要 JSON 解析（`parser.rs:23-45`, `parser.rs:661-715`）。**判据是行内字面量，不是 JSON 字段语义**，因此「忽略」有前提：`response_item`、`turn_started`、`task_started`、`inter_agent_communication*`、`session_meta` 等未被识别的行只有在**不含** `"usage":` / `"input_tokens":` / `"prompt_tokens":` 这三个字面量时才被整体丢弃（`parser.rs:277-279` 的 `else { continue }`）；一旦行文本命中这三个字面量之一，即使没有任何已识别的 `type` 字面量，它也会被归为 `Headless` 并进入 usage 提取链路（`parser.rs:708-713`，测试 `loader.rs:1248-1276`、`loader.rs:1279-1304`）。这里有个 64 KiB 的行长门槛只作用于「嵌套 token_count / thread_settings」的探测（`parser.rs:677-682`）。

时间戳规范化：字符串走 `parse_ts_timestamp` 转 RFC3339 毫秒；数字 >1e10 当毫秒、否则当秒乘 1000（`parser.rs:1174-1194`）。

### 3. token 拆分与增量语义（重点）

核心在 `parser.rs:479-508`（`token_count` 分支）：

```rust
let info = payload.info.as_ref();
let total_usage = info.and_then(|info| info.total_token_usage.as_ref().copied());
let cumulative_advanced = total_usage.as_ref()
    .is_none_or(|total_usage| previous_totals.as_ref() != Some(total_usage));
let raw_usage = info.and_then(|info| info.last_token_usage.as_ref().copied())
    .filter(|_| cumulative_advanced)
    .or_else(|| total_usage.as_ref()
        .map(|usage| subtract_codex_raw_usage(usage, previous_totals.as_ref())));
if let Some(total_usage) = total_usage { *previous_totals = Some(total_usage); }
```

要点：

- **不做「取最后一次累计值」**，而是逐事件产出 delta：优先信 `last_token_usage`；只有拿不到 `last_token_usage` 时才用 `total_token_usage - previous_totals` 差分（`subtract_codex_raw_usage`，六个字段全部 `saturating_sub`，`parser.rs:1284-1308`）。
- **累计未前进的重复快照被丢弃**：`cumulative_advanced == false` 时 `last_token_usage` 被 `.filter()` 掉，差分结果为全 0，随后被全 0 判定跳过（`parser.rs:501-508`）。回归测试 `skips_repeated_last_usage_when_cumulative_total_is_unchanged` 断言两次相同快照只产 1 个事件（`loader.rs:2244-2282`）。
- **`previous_totals` 跨 `turn_context` 保留**，即模型切换不重置基线；只有出现新的累计值才覆盖（`parser.rs:495-497`）。
- **cached 不从 `input_tokens` 里扣**：`input_tokens` 全程是「总输入」，`cached_input_tokens` / `cache_creation_tokens` 是并列的两个桶；只在输出层做减法（`report.rs:123-129`）：

```rust
pub fn non_cached_input_tokens(input_tokens: u64, cached_input_tokens: u64, cache_creation_tokens: u64) -> u64 {
    input_tokens.saturating_sub(cached_input_tokens.saturating_add(cache_creation_tokens))
}
```

  并且 buckets 有上界钳制：`cached = min(input)`、`cache_creation = min(input - cached)`，在反序列化（`types.rs:330-340`）与规范化（`parser.rs:1310-1316`）两处都做。跨快照差分后缓存语义仍成立，测试 `subtracts_cache_write_usage_from_cumulative_token_totals`（`loader.rs:942-995`，第二段断言 `input=100, cached=40, cache_creation=30`）。
- **reasoning 不并入 output 之外的任何地方**（它本身就是 output 的子集），`total_tokens` 的推导只 `input + output`（`types.rs:347-353`），并有测试固定「加了 reasoning 的推导会失败」：`derives_total_tokens_without_double_counting_reasoning`（`types.rs:545-557`）与 `keeps_a_recorded_total`（`types.rs:574-589`）。
- **`total_tokens` 校验/回退**：记录值必须 > 0 才采信，否则回退 `input + output`（`saturating_add`）：

```rust
total_tokens: fields.total_tokens.filter(|total| *total > 0)
    .unwrap_or_else(|| input.saturating_add(output)),
```

  对应的 4 个单测：`derives_explicit_zero_total_tokens_without_double_counting_reasoning`（`types.rs:559-572`）、`saturates_a_derived_total_instead_of_overflowing`（`types.rs:591-602`）、`derives_total_tokens_from_openai_field_spellings`（`types.rs:604-617`）、`leaves_an_empty_usage_total_at_zero`（`types.rs:651-657`）。
- **字段名兼容**：`input_tokens|prompt_tokens|input`、`output_tokens|completion_tokens|output`、`reasoning_output_tokens|reasoning_tokens`、`cached_input_tokens|cache_read_input_tokens|cached_tokens`、`cache_write_input_tokens|cache_creation_input_tokens`（`types.rs:278-308`）；数值也容忍字符串形式（`deserialize_optional_u64_lossy`，`types.rs:452-539`）。

### 4. `is_fallback_model`

`resolve_codex_usage_model`（`parser.rs:802-831`）：

```rust
if let Some(model) = parsed_model.as_ref() { *current_model = Some(model.clone()); *current_model_is_fallback = false; }
let mut is_fallback_model = false;
let model = parsed_model.or_else(|| current_model.clone()).or_else(|| {
    is_fallback_model = true;
    *current_model_is_fallback = true;
    *current_model = Some("gpt-5".to_string());
    current_model.clone()
});
if parsed_model_is_missing(&model, current_model, *current_model_is_fallback) { is_fallback_model = true; }
```

回退情形与回退值：

1. **完全没有模型信息**（既无本条记录字段，也无更早的 `turn_context`）→ 值 `gpt-5`，`is_fallback_model = true`（`parser.rs:813-818`）；且把 `current_model_is_fallback` 置位，使后续事件继续走 fallback 判定（`parser.rs:819-821`、`parser.rs:794-800`）。
2. **继承的当前模型本身是 fallback**（本条记录有模型名但当前 turn 模型是兜底的）→ 仍标记 fallback。
3. **`codex-auto-review`**（服务端路由别名）→ 按事件日期查内置时间表（`parser.rs:833-846` + `rust/adapters/codex/src/codex-auto-review-fallbacks.json`）：

```rust
fn codex_log_model_fallback(model: &str, timestamp: &str) -> Option<&'static str> {
    if model != CODEX_AUTO_REVIEW_MODEL { return None; }
    let Some(date) = codex_timestamp_date(timestamp) else { return Some("gpt-5"); };
    Some(codex_auto_review_fallback_models().iter()
        .find_map(|fallback| (date >= fallback.released_on).then_some(fallback.model))
        .unwrap_or("gpt-5"))
}
```

  表（7 项，降序）：`2026-07-30 → gpt-5.6-luna`、`2026-03-05 → gpt-5.4`、`2026-02-05 → gpt-5.3-codex`、`2025-12-11 → gpt-5.2-codex`、`2025-11-13 → gpt-5.1-codex`、`2025-09-15 → gpt-5-codex`、`2025-08-07 → gpt-5`。边界测试见 `loader.rs:1031-1102`（断言 `gpt-5.3-codex / gpt-5.4 / gpt-5.6-luna` 三段与全体 `is_fallback_model == true`）。日期字符串不合法时回退 `gpt-5`（`parser.rs:837-839`）。
4. **compaction 记录额外一档**：`token_usage_record` 即使最终模型来自 `turn_context`，也强制 `is_fallback_model: is_fallback_model || model_was_inferred`（`parser.rs:401-424`，测试 `parser.rs:1510-1511`、`parser.rs:1584`）。

聚合侧只是「或」：`model_usage.is_fallback |= event.is_fallback_model`（`aggregate.rs:523`、`aggregate.rs:845`），JSON 里输出为 `"isFallback"`（`report.rs:143`）。

### 5. replay / compaction（重点）

`src/README.md:47-53` 把这两类问题并列命名为「MultiAgent V2 subagent replay」：子会话在自有回合之前会**重放父会话的历史**，若照原样累加就重复计数。仓库用**两条互补**机制处理。

#### 5.1 fork 前缀匹配（`replay.rs` + `parser.rs:96-267`）

- 父关系来源：`session_meta.payload.forked_from_id`，否则 `source/subagent/thread_spawn/parent_thread_id`（`replay.rs:407-427`）。父文件通过 `session_id → 文件` 索引取「第一个不是自己的路径」（`replay.rs:202-249`）——不按文件名猜（测试 `bounded_loading_does_not_select_a_misleading_filename`，`loader.rs:2341`）。
- 计划构建：`CodexReplayPlan::new` / `for_bounded_files` 先读所有文件首行拿元数据，再只对「确实是父」的文件读一遍 usage 流（`replay.rs:92-151`，测试 `bounded_loading_metadata_probes_history_but_reads_parent_usage_only`，`loader.rs:2578`）。
- 前缀长度受 fork 时刻约束：`forked_at = session_meta.timestamp`，只消费父流中 `timestamp <= forked_at` 的部分，避免用父会话「fork 之后」的用量去吃掉子会话自己的事件（`replay.rs:158-177`）。

```rust
let replay_len = parent.forked_at.map_or(stream.usage.len(), |forked_at| {
    stream.timestamps.iter()
        .position(|timestamp| timestamp.is_some_and(|timestamp| timestamp > forked_at))
        .unwrap_or(stream.usage.len())
});
Some(&stream.usage[..replay_len])
```

- 消费算法是**逐条完全相同**（`CodexReplayState::MatchingParent`）：child 的每个非 compaction 事件与父前缀 `CodexRawUsage` 全 6 字段比对，相等则吞掉、不等则**立刻**退出匹配状态（`parser.rs:234-253`）。一旦在 index 0 就失配（父日志缺失或历史被重写过），转到下一机制。
- **重写突发启发式**（父日志不可靠时）：`detect_rewritten_burst` 扫文件头部，找出「前两个带 usage 的 token_count 事件时间差 ≤ 阈值」的那种密集写批次，返回其起点（阈值判定点在 `parser.rs:171-173`），从该点开始的连续事件按「相邻间隔 ≤ 阈值」一路吞掉（`parser.rs:129-177` 与 `parser.rs:254-263`，阈值判定点在 `parser.rs:256-261`）。
- 阈值常量与实测依据（`parser.rs:112-122`）：

```rust
/// Longest pause tolerated inside a burst of replayed usage.
/// ... bursts spanned 10 to 40ms and the pause that followed ran from 5.8 to 15.3 seconds,
/// so a second sits two orders of magnitude above the one and well below the other.
const CODEX_REWRITTEN_BURST_PAUSE_MS: i64 = 1_000;
```

  注释还解释了为什么不按「秒」分桶：跨秒写出的突发会被切开，剩下的会被当成子会话自己的用量。测试覆盖跨秒边界（`loader.rs:1988-2013`）、突发之后的子会话用量必须保留（`loader.rs:2015-2038`）、父日志缺失的祖先场景不误吞（`loader.rs:2072-2086`）、自指父 id（`loader.rs:2107`）、父流为「不可变快照」的嵌套重放（`loader.rs:2183`）、按 payload id 而非文件名定位父（`loader.rs:2284-2339`）。

#### 5.2 compaction 认领与去重（`parser.rs:73-85`, `parser.rs:342-447`, `parser.rs:510-530`）

状态机（`CodexCompactionUsageState`，`parser.rs:73-85`）：`compacted_response_ids`、`pending_usage: response_id → {event, thread_token_usage}`、`emitted_response_ids`、`latest_usage_response_id`、`recorded_usage_timestamps`。

- **配对**：`token_usage_record` 先进入 `pending_usage`（不立即上报）；`compacted` 到达时按 `compaction_response_id` 认领并上报，**记录顺序任意**（`compacted` 先到时走 `emitted_response_ids` 去重，`parser.rs:342-362`、`parser.rs:430-445`）。
- **本地 compaction 防重复**：本地 compaction 会在记录与标记之间多写一条前进的 `token_count`；当这条快照的用量与 pending 记录的请求用量相等、或与其 `thread_token_usage`（累计口径）相等，就把该 pending 记录直接删除——即「已在累计值里计过，不再加一次」。远端 v2 compaction 没有这条前进快照，才需要额外补一条事件（`parser.rs:510-530`，注释与测试 `compaction_tests.rs:283-311`、`compaction_tests.rs:343-355`）。
- **非法配对一律忽略**：缺 timestamp / 空 `response_id` / 全 0 usage / 缺 usage 的记录都不计入（`parser.rs:379-393`，测试 `compaction_tests.rs:252-281`；「无效记录不能掩盖待认领用量」见 `compaction_tests.rs:313-341`）。
- **compaction 不污染主基线**：compaction 事件单独产出（`response_id=Some`），不参与 §3 的 `previous_totals` 差分，也不改变当前 turn 模型（`src/README.md:27`，测试 `parser.rs:1515-1533`、`parser.rs:1561-1585`）。
- **跨文件去重键（两条链路，键不同）**：
  - 流式聚合（`load_groups*`）：有 `response_id` → `CodexEventKey::Response(response_id)`；无 → `Usage(timestamp, model, 6 桶)`，**且 session_id（hash + len）只在这个键上、且只在 `kind == AgentReportKind::Session` 时才参与**；日报/周报/月报的键里**没有** session 维度，跨会话的相同用量会被合并（`aggregate.rs:23-42`, `aggregate.rs:753-780`, 判据 `aggregate.rs:762-766`）。纯事件入口 `aggregate_events` 只有 `response_id` 一份去重（`aggregate.rs:874-878`）。
  - 装载期（`load_codex_events_from_directory`）：另一套 `CodexEventDeduplicationKey`，只有 `Response(response_id)` 与 `Usage(timestamp, model, 6 桶)`，**任何报表都无 session 维度**（`loader.rs:240-284`）。
- **键冲突的保留策略（按记录类型区分）**：带 `response_id` 的 compaction 记录**按 `file_index` 最小者重选**——更早文件的副本会替换掉后到的 session/timestamp/model（`aggregate.rs:720-731`）；无 `response_id` 的普通事件是**首见者胜**，后到的同键事件只参与 service tier 并集、本身不计入（`aggregate.rs:720-722`, `aggregate.rs:750`）。tier 冲突合并规则是「Standard 优先」（保守取低价），见 `types.rs:22-41` 与测试 `aggregate.rs:1356`。
- **日期边界下的 compaction**：`is_replayed_compaction` 保存父日志中所有已匹配的 compaction id（含已被累计快照覆盖的），使「父在时间窗之外」的受限报告仍能剔除子副本（`replay.rs:181-199`、`parser.rs:311-323`，测试 `compaction_tests.rs:94-133`、`compaction_tests.rs:135-176`）。

### 6. 聚合

- 分组键（`aggregate.rs:408-430`）：`Daily → YYYY-MM-DD`（时区 `format_date_tz`）、`Weekly → week_start(Monday)`、`Monthly → YYYY-MM`、`Session → session_id`。`since/until` 在分组前用 `YYYYMMDD` 文本比较做一次过滤（`aggregate.rs:416-423`）。
- 模型归一：`crate::model_aliases::resolve_model_name(model)`（`aggregate.rs:331`, `aggregate.rs:896`）。该函数**只读环境变量** `CCUSAGE_MODEL_ALIASES`（JSON 或 `a=b,c=d` 文本），支持 `-fast` 派生名，无配置时原样返回（`rust/crates/ccusage-core/src/model_aliases.rs:14-26`, `:39-61`）。**Codex 适配器内没有任何模型正则/映射表**；仅有的硬编码模型名单是 §4 的 `codex-auto-review` 日期表。
- 逐模型累加与长上下文拆分（`aggregate.rs:463-524`）：`is_long_context = event.input_tokens > long_context_split_threshold(model)`，按**单请求**输入量判定（注释明确说明事后无法从求和恢复），阈值取自 models.dev 快照、缺省 200K（`rust/crates/ccusage-core/src/pricing.rs:2753-2763`）。命中则另记 `long_context_*` 四桶，计价时整请求按长上下文档（`report.rs:349-392`）。
- 时间相关定价（如 DeepSeek V4 峰谷）才写 `timestamped_usage: BTreeMap<i64, ...>`（`aggregate.rs:488-511`，`pricing.rs:272-277`）。
- 记录档位桶（`recorded_standard/fast/flex_usage`）走的是一遍**后处理**：流式聚合时 `add_deduped_event_to_groups` 传 `record_service_tier=false`（`aggregate.rs:404`），等所有文件跑完，再用去重表（`seen`）里的记录补记（`aggregate.rs:582-676`, `aggregate.rs:594-676`）；纯事件入口 `aggregate_events` 则直接传 `true`（`aggregate.rs:897`）。这保证了「重复副本只按第一份归属、档位取并集」在多线程下仍然确定（测试 `aggregate.rs:938-965`、`compaction_tests.rs:178-228`）。
- 关于 `gpt-5` / `gpt-5.4` / `gpt-5.3-codex` 是否有特殊处理：**聚合层没有**；`gpt-5.3-codex` 只出现在（a）§4 的 auto-review 日期表、（b）fast 倍率表 `gpt-5.3-codex: 2.0`（`rust/crates/ccusage-core/src/fast-multiplier-overrides.json:8`）、（c）测试与快照。`gpt-5.4`/`gpt-5.6-luna` 同样只来自 §4 日期表与倍率表。别名类模型（如 `gpt-reserve`）是由计价层的 `pricing_alias` 解析（`docs/guide/codex/index.md:67`）。

### 7. 速度倍率（`--speed`，不是 `--codex-speed`）

旗标真实名称是 `--speed`，取值 `auto|standard|flex|fast`，仅 codex 命名空间接受（`rust/crates/ccusage-cli-parser/src/parser.rs:619`、`parser.rs:1194-1202`；文档 `docs/guide/codex/index.md:40`）。codex 子命令**不接受** `--by-agent`（`rust/crates/ccusage-cli-parser/src/tests.rs:644-645`）。

1. **日志内判定**：`event_msg.thread_settings_applied.thread_settings.service_tier` 逐事件更新当前档位（`parser.rs:464-478`），映射表 `default|standard → Standard`、`fast|priority → Fast`、`flex → Flex`（`parser.rs:649-659`）。缺该键的 settings 事件保持原值；键存在但值不认识则**清空**（不继承陈旧值）——注释解释了 auto-review 线程会发不带 tier 的 settings 事件（`parser.rs:464-478`）。
2. **未分类部分**（老日志、headless exec 输出、首个 settings 之前）→ `--speed auto` 读每个 root 的 `config.toml`：任一 root 为 `fast|priority` 则 Fast，否则任一为 `flex` 则 Flex，否则 Standard（`speed.rs:33-49`）。TOML 解析优先 `profiles.<profile>.service_tier`（仅当顶层 `profile = "name"` 选中它），否则顶层 `service_tier`；`"breakfast"` 这类子串不误判（`speed.rs:55-71`，测试 `speed.rs:81-199`）。
3. **倍率读取点**：`Pricing::fast_multiplier` / `flex_multiplier` 在建表时从 `provider_specific_entry.fast|flex` 读，缺失再查内置 overrides JSON，再缺省 `1.0`（`rust/crates/ccusage-core/src/pricing.rs:1071-1079`，overrides 文件 `pricing.rs:28-29`；匹配还支持 `pricing_alias`、日期后缀剥离、`/`、`:` 分段与 normalized_prefix，`pricing.rs:939-978`）。
4. **放大点**（`report.rs:234-275`）：

```rust
let standard_cost = calculate_codex_bucket_cost(&total_usage, pricing);
let unclassified_usage = subtract_codex_usage_bucket(total_usage,
    add_codex_usage_buckets(recorded_standard_usage,
        add_codex_usage_buckets(recorded_flex_usage, recorded_fast_usage)));
let (flex_usage, fast_usage) = match speed { /* Forced / Auto 各分支 */ };
standard_cost
    + calculate_codex_bucket_cost(&flex_usage, pricing) * (pricing.flex_multiplier - 1.0)
    + calculate_codex_bucket_cost(&fast_usage, pricing) * (pricing.fast_multiplier - 1.0)
```

   即**先按标准价算全量，再对 fast/flex 桶补差额**；`Forced(Standard)` 直接短路返回标准价，`Forced(Fast|Flex)` 把**全量**当 fast/flex 桶，`Auto(Standard)` 只信日志记录的桶，`Auto(Fast|Flex)` 把「未分类余量」并入对应桶（`report.rs:250-271`）。倍率为 1.0 时自然不加价，等价于文档所说「没有公布倍率就按标准价，不臆造倍率」。
5. **可观测证据**：快照测试 `sessionFast` 的 `costUSD = 0.0008085` 恰为 `Standard` 场景 `0.00040425` 的 2 倍（`rust/adapters/codex/src/snapshots/...snap:11` vs `:109`），对应 `gpt-5.3-codex` 的 2× 表项。

### 8. 成本与 `--mode`

调用链：`lib.rs:39-67` → `PricingMap::load_with_overrides` → `load_groups`（流式聚合）→ `report_from_groups` → `calculate_group_cost`（逐模型求和，`report.rs:394-404`）→ `calculate_codex_model_cost`（`report.rs:183-217`，有时间戳桶则逐时间点 `pricing.find_at`）→ `calculate_codex_usage_cost`（`report.rs:234-275`）→ `calculate_codex_bucket_cost`（`report.rs:343-392`）。

- **`costUSD` 不存在于 Codex 日志**：`CodexRawUsage` 的全部字段只有 6 个 token 桶（`types.rs:5-13`），反序列化字段表里没有任何 cost 字段（`types.rs:278-308`）。适配器里所有 `costUSD` 都是渲染出来的（`report.rs:88`, `report.rs:179`）；全仓 grep `costUSD` 在 codex 适配器内只出现在 report/测试/快照。**结论：Codex 分支没有「读日志自带成本」这条路径**。
- 桶内计价：非缓存输入按 `pricing.input`；缓存读按 `pricing.cache_read`，若价格数据未显式给出缓存读价则**回落到输入价**（`pricing.cache_read_explicit`，`report.rs:343-348`）；缓存写按 `cache_creation_input_token_cost()`（缺省 `input * 1.25`，`pricing.rs:1085-1088`）；输出按 `pricing.output`；长上下文四桶用 `*_above_200k` 对应费率，缺失则回落平档（`report.rs:349-392`）。
- 未定价模型：`codex_model_missing_pricing` 用 `missing_pricing_model_for_token_total` 判定（`report.rs:406-419`），行级加 `"missingPricing": true`、totals 里加 `"unpricedModels"`（`report.rs:28-30`, `report.rs:74-76`）；`0` 价是「价」，不算缺失（`report.rs:758-794` 测试）。
- **`--mode` 在 Codex 下的行为差异**：`CostMode::{Auto, Calculate, Display}` 定义于 `rust/crates/ccusage-cli/src/types.rs:242-248`，旗标 `-m/--mode`（`parser.rs:753`, `parser.rs:1136-1138`）。Codex 的成本计算函数**不接 mode 参数**（`report.rs:183-188`, `report.rs:394-404`），因此三种模式下金额完全相同；差异只在三处输出元数据：

```rust
// report.rs:28-30
if mode != CostMode::Display {
    attach_unpriced_models(&mut totals, codex_missing_pricing_models(groups, pricing));
}
```

  以及逐模型 `missingPricing` 标记（`report.rs:74-76`）与表格末尾的未定价告警（`report.rs:709-715`），`Display` 一律抑制。对照：其他适配器里 `Display` 会改为「只读日志自带成本、否则 0」（`rust/crates/ccusage-core/src/cost.rs:40-55`），Codex 因为没有自带成本字段，也就没有这种语义切换。

### 9. json / jsonl 双格式

- **不存在独立的 JSON（非 JSONL）文件链路**：发现阶段只收扩展名为 `jsonl` 的文件（`rust/adapters/common/src/lib.rs:14`），没有 `.json` 分支，也没有目录级别的格式开关。
- 真正的「双格式」是**行级**的：分叉函数 `codex_line_usage_kind` 返回 `Option<CodexLineKind>`（`parser.rs:661-715`），而 `CodexLineKind` 本身**只有两个变体** `Session` / `Headless`（`parser.rs:62-66`）；`None` 表示该行不产出事件，调用方据此 `continue`（`parser.rs:277-279`）。

```rust
#[derive(Clone, Copy)]
enum CodexLineKind { Session, Headless }                    // parser.rs:62-66

let Some(line_kind) = codex_line_usage_kind(&line) else {   // parser.rs:277-279
    continue;                                               // None：丢弃该行
};
match line_kind {
    CodexLineKind::Session  => visit_codex_session_entry(...),        // parser.rs:281-286
    CodexLineKind::Headless => {                                      // parser.rs:287-308
        if let Ok(value) = serde_json::from_slice::<CodexLogEntry<'_>>(&line) { add_codex_exec_event(...) }
        else { add_codex_exec_event_from_value(..., &line, ...) }
    }
}
```

- `Session`：rollout 会话行，判据是 `type` 字段为 `turn_context` / `token_usage_record` / `compacted` 或 `event_msg{token_count,thread_settings_applied}`（`parser.rs:661-707`；`codex_line_type_flags` 逐键扫描以容忍空格排版，`parser.rs:717-769`）。
- `Headless`：`codex exec --json` 之类的单行结果，判据是行内出现 `"usage":`、`"input_tokens":` 或 `"prompt_tokens":`（`parser.rs:708-713`）。usage 可从顶层 `usage`、`data.usage`、`result.usage`、`response.usage` 取（`parser.rs:1000-1046`）；模型与时间戳同样有 4 层回退（`parser.rs:922-953`, `parser.rs:1048-1128`）；时间戳缺失时用**文件 mtime** 兜底（`parser.rs:1271-1282`, `parser.rs:569-572`）。
- 典型行形态见测试 `loads_saved_codex_exec_json_usage`（`loader.rs:689-755`）：`{"type":"turn.completed","timestamp":...,"model":"gpt-5.2-codex","usage":{...}}` 与 `{"type":"result","data":{"model_name":...,"usage":{"prompt_tokens":...}}}`。
- 该分支的 `service_tier` 恒为 `None`（`parser.rs:645`），所以 headless 用量在 auto 模式下走 `config.toml` 兜底（`docs/guide/codex/index.md:73`）。

### 10. CLI 输出：`ccusage codex daily` vs `ccusage daily --by-agent`

**`ccusage codex daily --json`**（`report.rs:16-35` 组合，键名见 `report.rs:37-53`）：

- 顶层：`{"daily": [row...], "totals": {...}}`；`totals` 含 `inputTokens`（非缓存）、`cacheCreationTokens`、`cacheReadTokens`、`outputTokens`、`reasoningOutputTokens`、`totalTokens`、`costUSD`，未定价时附加 `unpricedModels`（`report.rs:147-181`）。
- 行（`report.rs:55-98`）：`date`、`inputTokens`、`cacheCreationTokens`、`cacheReadTokens`、`outputTokens`、`reasoningOutputTokens`、`totalTokens`、`costUSD`、`models{ <model>: {inputTokens(非缓存), cacheCreationTokens, cacheReadTokens, outputTokens, reasoningOutputTokens, totalTokens, isFallback} }`；`session` 视图额外加 `lastActivity` / `sessionFile` / `directory`（`report.rs:91-96`）。
- 表头名不同：`Date/Week/Month/Session, Models, Input, Output, Reasoning, Cache Create, Cache Read, Total Tokens, Cost (USD)`（`report.rs:436-467`），标题 `Codex Token Usage Report - Daily`（`report.rs:674-685`）。
- 结构快照：`rust/adapters/codex/src/snapshots/ccusage_adapter_codex__tests__snapshots_codex_reports_for_periods_sessions_costs_and_fallback_models.snap:6-59`（daily）、`:104-163`（session）。

**`ccusage daily --by-agent`**（统一报告，`--by-agent` 只在统一命令解析，`rust/crates/ccusage-cli-parser/src/parser.rs:375-400`）：

- JSON：`report_json_with_agents`（`rust/crates/ccusage-adapter-all/src/report.rs:28-37`）产出同样的 `{"daily":[...],"totals":{...}}` 外形，但行由 `AllRow` 渲染（`report.rs:133-172`）：`period`、`agent`、`modelsUsed`、`inputTokens`、`outputTokens`、`cacheCreationTokens`、`cacheReadTokens`、`totalTokens`、**`totalCost`**、`modelBreakdowns`，另加 `metadata`（`--by-agent` 时才插入 `agents: [...]`，`report.rs:148-156`）。
- Codex 数据进入该管线的转换在 `codex_group_row`（`rust/crates/ccusage-adapter-all/src/loader.rs:830-886`）：成本用同一个 `codex::calculate_codex_model_cost` / `calculate_group_cost`，token 同样过 `codex::non_cached_input_tokens`，但 `reasoningOutputTokens` 被塞进 `metadata`（`loader.rs:878-881`），`modelBreakdowns` 按 cost 降序（`loader.rs:863`）。

差异清单：

| 维度 | `ccusage codex daily` | `ccusage daily --by-agent` |
| --- | --- | --- |
| 成本字段名 | `costUSD` | `totalCost`（行/明细）/ `totalCost`（totals） |
| reasoning | 行级 `reasoningOutputTokens` | 藏在 `metadata.reasoningOutputTokens` |
| fallback 披露 | 行级 `isFallback` + `missingPricing` | 仅 `modelBreakdowns[].missingPricing`（`loader.rs:858-859`） |
| 模型明细 | `models` 对象（map） | `modelBreakdowns` 数组（按成本降序）+ `modelsUsed` |
| 速度策略 | `--speed` 可用 | 固定 `resolve_codex_speed(CodexSpeed::Auto)`（`loader.rs:675`, `loader.rs:688`），无旗标 |
| 段落/按 agent | 不适用 | `--by-agent` 才插 `agents[]`，否则省略（`tests.rs` `omits_by_agent_json_breakdowns_by_default`，`rust/crates/ccusage-adapter-all/src/tests.rs:544-568`） |

### 未验证/存疑

> 第 8 轮实测复核（§13.5）已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目；此处保留原始清单与当时的判断，逐条状态以 §13.5 为准。

1. **`src/README.md:47-53` 描述的 MultiAgent V2 边界在代码里不存在**：文档说用 `event_msg.task_started` 作为「重放前缀结束」、用 `inter_agent_communication_metadata`（旧名 `inter_agent_communication`）的 `trigger_turn === true` 作为「子回合开始」来切分。全仓 grep（`task_started` / `trigger_turn` / `inter_agent_communication`）**只命中该 README**，解析器没有任何相关字符串或状态。实际实现是 §5.1 的「父前缀逐条相等 + 重写突发」。两者结论方向一致但机制不同，属于文档与代码不一致；因浅克隆（`git log` 仅 1 个提交）无法判定谁先谁后。
2. **`docs/guide/codex/index.md:50` 与 `:116` 说「缺模型元数据的会话被跳过」，与代码冲突**：`parser.rs:813-818` 在无模型时回退 `gpt-5` 并标记 fallback（`src/README.md:63` 与同文档 `:54` 也是这么说的）。`aggregate.rs:328` 的 `model == None → skip` 分支在解析链上几乎不可达，目前只有手工构造事件的快照用例会命中（`lib.rs:1055-1068`）。
3. **replay 前缀匹配的精确性未在真实日志上验证**：`parser.rs:236` 要求 6 个字段完全相等；父日志被压缩/改写后完全走启发式。阈值常量本身与两处使用点已核实（定义 `parser.rs:122`，使用点 `parser.rs:171-173` 与 `parser.rs:256-261`），但注释给出的实测区间（10–40 ms vs 5.8–15.3 s）只来自源码注释（`parser.rs:114-118`），我未独立复现。
4. **`detect_rewritten_burst` 会二次打开并重扫文件头**（`parser.rs:129-177`），与主扫描重复 IO；触发条件与频率、对超大 rollout 的影响未测。
5. **session 身份是路径派生**（`parser.rs:892-904`），不是 Codex 的会话 UUID；同一会话被重命名/移动后，跨文件去重键（`aggregate.rs:29-42`，非 Session 分组时不含 session）与 `sessionId` 稳定性未验证。
6. **compaction 归属在并行下的确定性**依赖 `file_index`（`aggregate.rs:711-751`），我读了测试（`aggregate.rs:938-965`）但未实跑 `cargo test`；「文件顺序 = `files.sort()` 的字典序」这一前提未进一步验证。
7. **`--mode display` 在 Codex 下不改变金额**是从函数签名（无 mode 参数）推出的结论，仓库内没有针对 Codex 的 display 成本断言测试（只有抑制缺失定价元数据的测试 `report.rs:828-834`）。
8. 官方文档 `docs/guide/codex/index.md` 为 Beta 声明（`:5`），字段与语义随 Codex CLI 版本演进；本次精读只覆盖 v20.0.31 的单一快照。

已复核确认（原存疑项已撤销）：`.jsonl.zst` 无支持（全仓 grep `zstd` 零命中，`flate2` 仅 `ureq` 传递依赖）；速度旗标确为 `--speed`；`CODEX_REWRITTEN_BURST_PAUSE_MS = 1_000` 与两处使用点一致（`parser.rs:122`, `:171-173`, `:256-261`）；Codex 的增量语义与输出层 cached 扣减（`report.rs:123-129`）不矛盾——内部保留总 input、仅在渲染时做 `input - cached - cache_creation`。
