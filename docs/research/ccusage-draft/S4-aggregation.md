## S4 聚合与报告形态

- 仓库：`ccusage`（只读样本）HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31
- 证据约定：所有 `path:line` 均相对仓库根；行号取实际文件行号（用 `read` 逐文件核对，不用 `Measure-Object -Line`，该命令会漏掉末行）
- 方法：静态阅读（`read` / `grep` / `glob`）为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5），未修改任何样本
- **本章裸文件名约定**：未写全路径的 `summary.rs`/`date_utils.rs`/`last_window.rs`/`output.rs`/`agent_report.rs`/`types.rs` 指 `rust/crates/ccusage-core/src/` 下同名文件；`blocks.rs`/`main.rs`/`commands/mod.rs`/`cli/timezone.rs`/`cli/last_window.rs` 指 `rust/crates/ccusage/src/`；`loader.rs`/`report.rs`/`types.rs`（统一报表语境）指 `rust/crates/ccusage-adapter-all/src/`；`parser.rs` 指 `rust/crates/ccusage-cli-parser/src/parser.rs`。
- 覆盖：`ccusage-core`（summary / date_utils / last_window / output / agent_report / types）、`ccusage`（blocks / commands / cli）、`ccusage-adapter-all`（loader / report / types）、`ccusage-cli-parser`（parser）、`ccusage-cli`（types）、`ccusage-config`（config），并以 `docs/guide/*.md` 交叉验证

### 0. 结论速览

| 问题 | 结论 | 关键证据 |
| --- | --- | --- |
| 分组 key | 由调用方闭包生成 `String`，分组容器是 `BTreeMap`；需要复合 key 时手工拼分隔符再 `split_once` 拆回 | `summary.rs:14`, `summary.rs:400` |
| 模型折叠 | `models_used` 是"首见顺序 + 去重"，`model_breakdowns` 按 `cost` 降序；表格渲染时才按名字重排 | `summary.rs:65`, `summary.rs:100`, `output.rs:522` |
| 时间戳解析 | 只接受带时区后缀的 `YYYY-MM-DDTHH:MM:SS[.mmm] (Z\|±HH:MM)`，无后缀即解析失败 | `date_utils.rs:111` |
| 时区优先级 | `--timezone` > config `timezone` > `local`/缺省 = 系统时区（jiff `system()`，其内部受 `TZ` 影响） | `config.rs:607`, `date_utils.rs:301`, `jiff tz/system/mod.rs:185` |
| 日期边界 | 两侧闭区间、`YYYYMMDD` 文本比较，本身不做时区换算（时区在生成 `entry.date` 时已应用） | `date_utils.rs:196`, `claude/src/lib.rs:524` |
| `--last N` | 按命令自身的日历周期回推，换算成 `since` 后交给同一套过滤 | `cli/last_window.rs:11`, `last_window.rs:20` |
| 周键 | `YYYY-MM-DD`（周起始日，周日/周一由 `--start-of-week` 决定）；月键 `YYYY-MM` | `summary.rs:189`, `summary.rs:317` |
| 会话 key | `(project_path, session_id)` 二元组分组，展示键取 `session_id` | `commands/mod.rs:189`, `loader.rs:738` |
| 5 小时块 | 锚定首个活动时刻、按 UTC 整点取整；`since_last > 5h`（严格大于）断开并插入 gap 块 | `blocks.rs:53`, `blocks.rs:75` |
| statusline | 读 Claude Code 钩子 JSON（stdin），输出单行文本；无 JSON/sed 变体；有 temp 目录缓存 | `commands/mod.rs:310`, `commands/mod.rs:530`, `commands/mod.rs:860` |

### 1. 聚合原语

#### 1.1 `summarize_by_key`

签名（`summary.rs:14-22`）：

```rust
pub fn summarize_by_key<F, M>(
    entries: &[LoadedEntry],
    key_fn: F,                    // F: Fn(&LoadedEntry) -> String
    meta_fn: M,                   // M: Fn(&str) -> (String, Option<String>)
) -> Result<Vec<UsageSummary>>
```

语义：`BTreeMap<String, UsageAccumulator>` 累加（`summary.rs:23-26`），随后对每个 key 调用 `meta_fn` 得 `(date, project)` 写回摘要（`summary.rs:29-35`）。返回顺序 = key 的字典序，而非输入顺序；无 key 时返回空 `Vec`（`Ok` 而非错误）。

伪代码（等价重写）：

```text
groups := BTreeMap<String, UsageAccumulator>
for entry in entries: groups[key_fn(entry)].add_entry(entry)
for (key, group) in groups:                    # BTreeMap → key 升序
    (date, project) := meta_fn(key)
    summary := group.into_summary(); summary.date := date; summary.project := project
```

#### 1.2 分组 key 的构造方式（闭包，不是函数指针；复合 key 手工拼分隔符）

- 核心函数收 `Fn(&LoadedEntry) -> String` 闭包，不是 `&'static str` 函数指针（`summary.rs:20`）。
- 单个维度：`run_bucket` 用 `|entry| entry.date.clone()`（`commands/mod.rs:75-78`），`run_weekly` 同样（`commands/mod.rs:113-117`）。
- 复合维度：调用方自选分隔符 —— 测试里用 `format!("{}|{}", entry.date, entry.project_path)`，再用 `key.split_once('|')` 还原（`summary.rs:400-408`）：
  - 分隔符不由库强制，`meta_fn` 负责解析；`summarize_by_key` 只把 key 当不透明 `String`。
- 会话维度不走 `summarize_by_key`，而是直接手写 `FxHashMap<(Arc<str>, Arc<str>), usize>` 索引 + `SessionAccumulator` 向量（`commands/mod.rs:186-199`），或 `BTreeMap<(String, String), SessionAccumulator>`（`loader.rs:735-741`）。

#### 1.3 `UsageAccumulator`（内部）/ `into_summary`（`summary.rs:39-123`）

累加规则：

```text
add_entry(entry):
    counts.add_usage(entry.usage)                      # 四桶，内部 saturating_add
    counts.extra_total_tokens += entry.extra_total_tokens   # saturating（:54-57）
    cost += entry.cost
    credits / message_count: 仅当 entry 该字段为 Some 时，惰性建 0.0/0 再累加（:59-64）
    model := resolve_model_name(entry.model)           # 别名归一（:66）
    breakdown := 按 model 名索引复用（FxHashMap: breakdown_indexes）
                 首次出现 → 追加到 models 与 breakdowns（:67-79）
    合并四桶 + extra_total_tokens + cost（全部 saturating_add，:80-92）
    entry.missing_pricing_model.is_some() → breakdown.missing_pricing = true（:93-95）
```

`into_summary`（`summary.rs:99-122`）关键点：

- `breakdowns.sort_by(|a,b| b.cost.total_cmp(&a.cost))` —— 模型明细**按成本降序**（`summary.rs:100`），成本相同才保留原插入顺序（稳定排序）。
- `models_used` 保留**首次出现顺序**（`self.models` 直接搬，无排序；`summary.rs:117`）。
- `date/month/week/session_id/project_path/last_activity/first_activity/project/versions` 全部置 `None` 或由上层填充（`summary.rs:101-121`）。

#### 1.4 `SessionAccumulator`（`summary.rs:125-170`）

```text
struct SessionAccumulator:
    usage: UsageAccumulator
    latest: Option<(TimestampMs, Arc<str> session_id, Arc<str> project_path)>
    earliest: Option<TimestampMs>
    versions: BTreeSet<String>

add_entry(entry):
    usage.add_entry(entry)
    if latest 缺失 或 entry.timestamp > latest.0: latest := (ts, session_id, project_path)   # :136-146
    if earliest 缺失 或 entry.timestamp < earliest: earliest := ts                            # :147-152
    if entry.data.version = Some(v): versions.insert(v)                                       # :153-155

into_summary():
    若 latest 为 None → Err("empty session group")                                            # :159-161
    session_id / project_path ← latest 的身份
    last_activity  := format_rfc3339_millis(latest.0)   # UTC RFC3339 毫秒
    first_activity := earliest 映射为同一格式
    versions       := BTreeSet 转升序 Vec
```

- `is_none_or(...)` 保证 `>` / `<` 严格比较：**时间戳相同不覆盖**，即"最新元数据"取首次达到最大时间的那条 entry。
- 输出的时间字段是 UTC `...Z` 字符串，与展示层时区无关（`date_utils.rs:356-368`）。

#### 1.5 `BucketKind` / `summarize_summaries_by_bucket`（`summary.rs:172-289`）

```rust
pub enum BucketKind { Monthly, Weekly }                       // :172-176
pub fn summarize_summaries_by_bucket(rows, kind, start: WeekDay) -> Vec<UsageSummary>  // :178
```

桶 key 计算（`summary.rs:188-191`）：

- `Monthly`：`date.get(..7)` —— 直接取前 7 个字符，即 `YYYY-MM`（`summary.rs:189`）。
- `Weekly`：`week_start(date, start)`，返回 `YYYY-MM-DD` 周首日；非法日期回退成原 date 文本（`summary.rs:190`）。
- `row.date` 为 `None` 的行**被整行跳过**（`summary.rs:185-187`）；`BucketKind::Weekly` 时 `start` 由调用方给（`run_weekly` 传 `args.start_of_week`，`commands/mod.rs:121`；`run_bucket` 硬编码 `WeekDay::Sunday`，`commands/mod.rs:83`）。

折叠逻辑 `aggregate_summaries`（`summary.rs:208-289`）：

- 所有 token 桶与 `extra_total_tokens` 逐行 `saturating_add`；`total_cost` 用 `+=`（浮点，无饱和）；`credits`/`message_count` 只在有值时惰性求和。
- `models_used` 去重并保序：`FxHashSet` 判重后按行序 push（`summary.rs:230`, `252-256`）——即"第一行遇到的名字在前"。
- `model_breakdowns` 按 `model_name` 索引合并后，统一 `sort_by(cost 降序)`（`summary.rs:257-268`, `286-287`）。
- 折叠结果不写 `date`，只写 `month` 或 `week`（`summary.rs:199-202`）。

#### 1.6 `filter_and_sort_summaries` / `sort_summaries`（`summary.rs:291-315`）

```rust
pub fn filter_and_sort_summaries<F>(rows: &mut Vec<UsageSummary>, shared: &SharedArgs, date_fn: F)
where F: Fn(&UsageSummary) -> &str
pub fn sort_summaries<F>(rows: &mut [UsageSummary], order: &SortOrder, date_fn: F)
```

- 仅当 `since`/`until` 至少一个存在时才 `retain`（`summary.rs:295`），判定委托 `date_within_range(date_fn(row), since, until)`（`summary.rs:296-303`）。
- 排序是**纯字符串比较**（`date_fn(a).cmp(date_fn(b))`），`SortOrder::Asc` 升序、`Desc` 反向（`summary.rs:311-314`）；顺序容器是 `BTreeMap`，因此同值行保持插入序。
- 排序键由调用方定义：日期行用 `row.date`、月/周行用 `row.month`/`row.week`（`commands/mod.rs:79-87`, `122-124`）。

#### 1.7 模型别名与模型折叠的前置归一

`resolve_model_name`（`model_aliases.rs:14-26`）在累加前调用（`summary.rs:66`），作用于环境变量 `CCUSAGE_MODEL_ALIASES`（`model_aliases.rs:8`, `32-37`）：

- 精确键命中 → 返回别名；键缺失但模型以 `-fast` 结尾且基名命中 → 返回 `别名-fast`（`model_aliases.rs:20-24`）；否则原样返回（`Cow::Borrowed`）。
- 值支持 JSON 与 `a=b, c=d` 分隔两种写法（`model_aliases.rs:39-61`）。
- 效果：多个私有名可折进同一 `model_breakdowns` 条目（测试 `summary.rs:611-660`：两个私有名并入 `gpt-5.5`，token/cost 相加）。

---

### 2. 时间与时区

#### 2.1 `TimestampMs` 与毫秒常量（`date_utils.rs:5-78`）

```text
MILLIS_PER_SECOND = 1_000; MILLIS_PER_MINUTE = 60_000
MILLIS_PER_HOUR   = 3_600_000; MILLIS_PER_DAY = 86_400_000     # :5-8
floor_to_hour(ts) = floor_div(ts, MILLIS_PER_HOUR) * MILLIS_PER_HOUR   # :58-60（div_euclid，负数安全）
```

`utc_parts`（`date_utils.rs:62-77`）用纯整数算法拆 UTC 年月日时分秒毫秒（`civil_from_days`，`date_utils.rs:278-291`），不依赖系统时区；`utc_now()` 取 `SystemTime::now()` 毫秒，失败退 0（`date_utils.rs:103-109`）。

#### 2.2 `parse_ts_timestamp`：接受的格式（`date_utils.rs:111-145`）

```text
接受形态（先定 millis 与 timezone_start，再校验字段分隔符位置 :113-125）:
  len 20/25 且 bytes[19] ∈ {Z,+,-} → 无小数秒，时区从 19 开始（25 = ±HH:MM）
  len 24/29 且 bytes[19] == '.'    → 毫秒 = bytes[20..23]，时区从 23 开始
  其它长度或形态 → None（含没有时区后缀的 23 字符形式）
强制分隔符 [4]=='-' [7]=='-' [10]=='T' [13]==':' [16]==':'；hour≤23, minute≤59, second≤59   # :118-136
组装：先按字段拼"本地"毫秒，再 checked_sub_millis(offset_minutes * 60_000)                # :137-144
```

关键结论：

- **只支持毫秒精度（3 位小数）**：`bytes[20..23]` 固定读 3 位（`date_utils.rs:115`），微秒或纳秒形式的第 4 位小数会让分隔符校验失败或长度不匹配。
- **无时区后缀的字符串解析失败返回 `None`**（并非"按本地时区解释"）。调用方对 `None` 的处理是丢弃/报错，例如 Codex 抛 `Invalid Codex timestamp: ...`（`codex/src/aggregate.rs:885-887`, `912-914`）。
- 偏移只支持 `Z` 或 `±HH:MM`（`date_utils.rs:147-161`）；`±HHMM`、`+HH`、带秒偏移均返回 `None`。偏移必须与时间戳同时出现，不存在"秒级时间戳"分支。
- 毫秒**不存在**"秒 vs 毫秒自动判别"：入口不接收纯数字时间戳；另有 `TimestampMs::from_unix_seconds`（`date_utils.rs:38-40`）供个别适配器自行换算。

#### 2.3 时区解析与优先级（`date_utils.rs:293-324`）

```rust
pub fn is_valid_timezone(value: &str) -> bool          // :297-299
    = value == "local" || value == "UTC" || jiff::tz::TimeZone::get(value).is_ok()

pub fn parse_tz(timezone: Option<&str>) -> Option<JiffTimeZone>   // :301-307
    None        → None        （调用方再兜底 system()）
    "local"     → None        （同缺省：系统时区）
    "UTC"       → TimeZone::UTC
    其它        → TimeZone::get(value).ok()   （未知名 → None → 静默回退 system()）
```

- `tzdb` 来自 **jiff 0.2.38**（`rust/Cargo.lock:779-781`），并启用 `jiff-tzdb`（registry 中存在 `jiff-tzdb-0.1.9`）。库内 `TimeZone::get(name)` 走 IANA 名称查表；`format_date_tz` 用 `timestamp.to_zoned(zone)` 做跨日换算（`date_utils.rs:313-324`）。
- 未知时区名会被 `parse_tz` 静默吃掉（回退 `system()`，`date_utils.rs:301-307`），因此 CLI 在入口显式校验一次：不通过就报 `Invalid value for --timezone '...'`（`cli/timezone.rs:10-16`）；校验对象是"该命令实际会用到的时区"——命令存在时用命令副本、`statusline` 用自己的 `timezone`、缺命令时用根 `shared`（`cli/timezone.rs:21-27`）。因此 `--timezone Not/AZone daily -z UTC` 合法（后者覆盖前者），`--timezone UTC daily -z Not/AZone` 报错（测试 `cli/timezone.rs:117-127`）。
- 优先级（高→低）：① 命令行 `--timezone`/`-z`（flag 在 config 应用之后解析，`config.rs:567-625` 是 config 写入点）；② config 文件 `timezone`（statusline 见 `config.rs:492-493`，shared 见 `config.rs:607-608`；配置文件整体优先级见 `docs/guide/cli-options.md:272-281`）；③ `local` 或缺省 → jiff `TimeZone::system()`（`date_utils.rs:239`, `317`, `374`；`claude/src/paths.rs:126`）。
- **`TZ` 环境变量不由 ccusage 读取**：仓库内无 `env::var("TZ")`。`TZ` 只在"最终落到 `system()`"这条路径生效，由 jiff 实现读取 —— jiff-0.2.38 `src/tz/system/mod.rs:185` `std::env::var_os("TZ")`，其文档声明 `TZ` 在所有平台覆盖其它来源（jiff `src/tz/timezone.rs:291`, `325`）。
- 解析结果的下游用法：适配器普遍形如 `let tz = parse_tz(shared.timezone.as_deref())` 再用 `format_date_tz(ts, tz)` 生成 `entry.date`（Claude：`claude/src/lib.rs:153`, `524`；日汇总 `claude/src/daily.rs:54`, `347`；Codex：`codex/src/aggregate.rs:191`, `415`, `869`, `888`）。其余适配器（`amp`/`goose`/`qwen`/`kimi`/`copilot`/`droid`/`grok`/`zcode`/`pi`/`hermes`/`openclaw`/`codebuff`/`kilo`/`gemini`/`antigravity`/`opencode`）在各自 loader/parser 走同一模式，例如 `opencode/src/loader.rs:120`、`copilot/src/loader.rs:37`、`kilo/src/loader.rs:21`、`gemini/src/loader.rs:21`。

#### 2.4 `format_date` 与跨日边界（`date_utils.rs:309-337`）

```text
format_date(ts, tz: Option<&str>) = format_date_tz(ts, parse_tz(tz).as_ref())
format_date_tz(ts, zone):
    若 ms → jiff Timestamp 转换失败 → format_utc_date(ts)   # 用纯整数 UTC 分解兜底（:326-329）
    zone := zone 或 system()
    zoned := ts.to_zoned(zone)
    "{year:04}-{month:02}-{day:02}"
```

- 跨日边界就是"同一时间戳在目标时区下的日历日"，因此 `--since/--until` 的"日期"随 `--timezone` 变化（测试：UTC 23:30 的日志在 `Asia/Tokyo` 下落在次日，`commands/mod.rs:1004-1018`）；超出 jiff 可表示范围的极端值回退纯整数 UTC 分解，不 panic（`date_utils.rs:314-316`）。
- `format_date` 是**唯一**把时间戳落到"报表日期"的出口；因为 `entry.date` 在加载期就已按目标时区生成，所有报表过滤都只做字符串比较。

#### 2.5 `format_utc_minute` / `format_utc_second` 用途

```rust
#[doc(hidden)] pub fn format_utc_minute(ts) -> "YYYY-MM-DD HH:MM"     // date_utils.rs:339-346
pub fn format_utc_second(ts) -> "YYYY-MM-DD HH:MM:SS"                 // date_utils.rs:348-354
```

- `format_utc_minute` **在生产代码中无调用点**，只被 `main.rs` 的测试断言使用（`main.rs:231`）；`format_utc_second` 唯一生产调用点是 `--active` 单块详情的 `Block Started:`（`blocks.rs:475`）——即**该行用 UTC 书写**，与表格里用 `local_parts` 渲染的本地时间（`blocks.rs:292-315`）口径不同。
- JSON 路径统一用 `format_rfc3339_millis`（UTC `Z`，`date_utils.rs:356-368`），block 的 `id`/`startTime`/`endTime` 均如此（`blocks.rs:135`, `207-208`）。

---

### 3. 日期过滤

#### 3.1 `--since` / `--until` 的字符串比较语义（`date_utils.rs:190-203`）

```rust
pub fn date_within_range(date: &str, since: Option<&str>, until: Option<&str>) -> bool
    compact := date.replace('-', "")
    since.map_or(true, |s| compact >= s) && until.map_or(true, |u| compact <= u)     # 两侧闭区间
```

- 两侧都是**闭区间**，比较对象是 `YYYYMMDD` 文本；文档注释明确"partial bounds such as `2026` keep working"（`date_utils.rs:193-195`），测试覆盖 `2026`/`202602` 前缀界限（`date_utils.rs:486-494`）。
- 该函数本身**不做时区换算**：`date` 是调用方已按目标时区算好的字符串（`entry.date`）。它是"权威窗口判定"的**单一定义点**，生产调用有两处（`adapters/common/src/lib.rs:41`，供各适配器的 entry 过滤复用；`core/src/summary.rs:297`，在 `filter_and_sort_summaries` 里）。其余报表路径是**内联复制的等价逻辑**，不是调用：
  - `blocks.rs:170-174`：`format_date(block.start_time, shared.timezone)` → 去 `-` → `>= since && <= until`；
  - `all/src/loader.rs:660-664`（日汇总）与 `:758-766`（会话汇总）：`row.date` → 去 `-` → 同款比较。
  三处与 `date_within_range`（`date_utils.rs:196-203`）语义相同，但属于重复实现；改边界语义时需要同步修改这几处。
- 归一化发生在解析期：`--since 2026-07-10` 与 `20260710` 都会变成 `20260710`（`normalize_date_bound`，`ccusage-cli/src/types.rs:82-110`；测试 `types.rs:331-341`）。非法值（`2026-2-30`、`2026/07/10`、`2026-07-10T00:00:00Z`、非 ASCII）返回 `None`（`types.rs:357-381`），解析器报 `Invalid value for --since '...'. Expected YYYY-MM-DD or YYYYMMDD.`（`parser.rs:1047-1051`, 格式串 `types.rs:74`）。
- 另一条补充路径 `date_range_bounds_ms`（`date_utils.rs:205-241`）把完整日期界限解析为半开区间 `[since 00:00, until+1 天 00:00)` 的毫秒界，用于加载侧按时间戳剪枝（如 Copilot 适配器 `copilot/src/loader.rs:18`, `37`）；库注释声明它只是剪枝、精确判定留给调用方（`date_utils.rs:208-211`）——而"调用方"在报表侧正是 §3.1 里那几处内联比较。夏令时"当天 23 小时"由 `start_of_day_ms` 处理（`date_utils.rs:227-241`；测试 `date_utils.rs:451-460`）。

#### 3.2 加载侧剪枝：`load_entries` vs `load_entries_since`（`claude/src/lib.rs:47-139`）

- 语义差异：前者读全部历史、忽略 `since`（用于需要历史锚点的报表，如 blocks 与整会话总计）；后者先 `split_files_before_since(files, shared, utc_now())` 丢弃窗口前的文件（`lib.rs:126-139`），窗口内条目不完整，调用方**必须再按日期丢弃窗口外条目**（文档注释 `lib.rs:57-66`；实际调用点都补了过滤：`all/src/loader.rs:649`, `commands/mod.rs:185`, `210`）。函数名本身不承诺"已过滤"。

#### 3.3 `--last N`（`last_window.rs:20-45` + `cli/last_window.rs:11-24`）

```rust
pub enum PeriodUnit { Day, Week, Month }                       // last_window.rs:8-12
pub fn last_periods_since(unit, count: u32, today: &str, start_of_week: WeekDay) -> Option<String>
    earlier := max(count,1) - 1                                # :26
    Day   : parse(today) - earlier 天                            # :28-30
    Week  : week_start(today, start_of_week) - earlier 周        # :31-33
    Month : first_of_month(today) - earlier 月                    # :34-37
    输出 "YYYYMMDD"                                             # :39-44
```

- 周期语义：`count` 计入当天/本周/本月，所以 `count=1` 就是"当前周期"；`count=0` 被夹到 1（`last_window.rs:26`；测试 `last_window.rs:104-110`）。
- 非法 `today`（非 ISO 日）返回 `None`（`last_window.rs:113-115`）。
- 单位由命令决定（`cli/last_window.rs:26-58`）：无子命令=Day、`daily`=Day、`monthly`=Month、`weekly`=其 `start_of_week`（`cli/last_window.rs:32-35`）；各 agent 报告按自己的 kind 取 Day/Week/Month，`Session` 返回 `None`（`cli/last_window.rs:64-74`）；`session`/`blocks`/`statusline` 整体不支持（`cli/last_window.rs:56`）。
- 周起点归属：除 `ccusage claude weekly` 用用户设置外，其余周报统一用周一（`const UNIFIED_WEEK_START: WeekDay = WeekDay::Monday`，`cli/last_window.rs:60-62`）。
- 换算结果写回 `shared.since`（`cli/last_window.rs:22`），此后与 `--since` 完全同路；`today` 用 `format_date(utc_now(), shared.timezone)` 得到（`cli/last_window.rs:18`），即 `--last` 也受 `--timezone` 控制。
- 解析失败文案：`Could not resolve --last {last} from today's date`（`cli/last_window.rs:20`）。

#### 3.4 非法输入与非法组合（`parser.rs`）

| 情形 | 行为 | 证据 |
| --- | --- | --- |
| `--since/--until` 非法格式或非法日历日 | 解析报错 `Invalid value for {flag} '{value}'. Expected YYYY-MM-DD or YYYYMMDD.` | `parser.rs:1047-1051` |
| `--last 0` 或非数字 | `Invalid value for --last '{value}'. Expected a whole number of periods, 1 or greater.` | `parser.rs:1053-1060` |
| `--last` 用在 session/blocks/statusline | `The --last option is only available for the daily, weekly, and monthly reports.` | `parser.rs:1100-1110` |
| `--last` 与 `--since`/`--until` 并用 | `The --last option cannot be combined with --since or --until.` | `parser.rs:1111-1113` |
| `--last` 与 `--sections` 并用 | `The --last option cannot be used with --sections.` | `parser.rs:1114-1116` |
| `since > until` | `The --since date '{since}' is later than the --until date '{until}'.`（statusline 豁免） | `parser.rs:1120-1132` |
| `--timezone` 未知 | `Invalid value for --timezone '...'. Expected an IANA timezone name ...` | `cli/timezone.rs:11-14` |
| 配置里的非法日期界限 | 在 `ConfigContext::config_error()` 阶段报出，解析期不再重复 | `config.rs:568-575`, `531-535` |

- 校验执行顺序（真实顺序，`parser.rs:74-133`）：① 版本/帮助控制参数（`:76-80`）→ ② `report_flag_alias_error`（`:81-83`）→ ③ `agent_filter_option_error`（`:84-86`）→ ④ `unsupported_agent_report_error`（`:87-89`）→ ⑤ `config.config_error()`（`:90-92`，含配置里的非法日期界限）→ ⑥ 构造 `SharedArgs::with_defaults()` + `apply_shared`（`:93-94`）→ ⑦ 根参数解析（`:95-107`）→ ⑧ 命令解析（`:109-122`）→ ⑨ 多余参数报错（`:123-125`）→ ⑩ `last_option_error`（`:126-128`）→ ⑪ `date_window_error`（`:129-131`）。即 `config_error` 在根/命令解析**之前**。
- 其后 `cli.rs` 再做时区校验与 `--last` 解析（`cli.rs:26-31`），失败一律 `process::exit(2)` 并提示 `Run 'ccusage --help' for usage.`（`cli.rs:35-39`）。

---

### 4. 周报 / 月报

#### 4.1 构建路径

- `run_bucket(shared, kind)`（`commands/mod.rs:72-108`）：先 `load_entries_since` → `summarize_by_key(|e| e.date)` 得到日行 → `filter_and_sort_summaries`（`--since/--until` + 排序）→ `summarize_summaries_by_bucket` → 再次按 `month`/`week` 排序 → 输出。月报与 `ccusage monthly` 走这条路径，周起点硬编码 `WeekDay::Sunday`（`commands/mod.rs:83`）。
- `run_weekly(args)`（`commands/mod.rs:110-144`）：同上，但周起点用 `args.start_of_week`（`commands/mod.rs:121`），这正是 `ccusage claude weekly -w monday` 与 `ccusage weekly`（统一报告）周起点差异的来源。
- 统一（all）报告的周/月聚合在 `aggregate_rows`（`all/src/loader.rs:888-908`）：`Monthly` 取 `period[..7]`、`Weekly` 用 `week_start(&row.period, WeekDay::Monday)`（固定周一），键仍是字符串。

#### 4.2 key 格式与范围标注

- 月 key：`YYYY-MM`（`summary.rs:189`）；周 key：周首日 `YYYY-MM-DD`（`summary.rs:330` 用 `format_naive_date`，不带时区后缀）。
- `--week-start` / `-w`（CLI 实际选项名是 `--start-of-week`，`parser.rs:530-531`；`docs/guide/weekly-reports.md:45-58`）默认 Sunday（`parser.rs:520-523`），可选 `monday..saturday`（`parser.rs:1181-1190`，非法值 `Invalid week day '{value}'`）。
- 周 key 的偏移计算：`shift = (day_from_sunday - start_num + 7) % 7`，再 `date - shift` 天（`summary.rs:317-331`）。`Sunday=0`、`Monday=1`（`summary.rs:319-327`），与 `IsoDate::weekday_from_sunday`（`date_utils.rs:92-94`，Unix 纪元日 +4 余 7）一致。
- 周报只标注**起始日**，不含结束日；表格首列文案由调用方给（`"Week"`，`commands/mod.rs:104`, `137`；all 报告 `agent_report::first_column`，`agent_report.rs:50-57`）。JSON 中同一 key 出现在 `week` 字段（`summary_json`，`output.rs:45-47`）。
- 范围标注在文档侧被表述为"Week: Start date of the week (configurable)"（`docs/guide/weekly-reports.md:35`），与代码一致：不存在 `2026-05-11~2026-05-17` 这类区间文本。
- `--last N` 周报的边界即"当前周首日回推 N-1 周"（`last_window.rs:31-33`），与桶化用的周起点同源，因此窗口边界与桶边界对齐（`cli/last_window.rs:32-35` 传同一个 `start_of_week`）。

---

### 5. 会话报告

#### 5.1 两条路径

| 命令 | 入口 | 数据范围 | 分组 |
| --- | --- | --- | --- |
| `ccusage claude session` | `parser.rs:564`, `578` → `parse_claude_session_command`（`parser.rs:539-555`）→ `Command::Session` → `commands::run_session` | 仅 Claude（`load_entries_since`） | `(project_path, session_id)` 二元组 |
| `ccusage session`（统一报告） | `parser.rs:432-474` → `Command::All{kind: Session}` → `adapter::all::run`（`adapter-all/src/lib.rs:38-70`）→ `load_rows(Session)` | 全部内置 agent，各源独立聚合后合并 | 各源内部同样按会话分组，合并键是 `session_id` 字符串 |
| `ccusage session --id X` | `parser.rs:454-461` 分流回 `Command::Session` → 同 `commands::run_session` | 仅 Claude 日志（`--id` 分支不走 all 报告，`all/src/lib.rs:38-70` 未读取 `AgentCommandArgs.session_id`） | 同上 |

- 顶层 `session --id` 表面上属于统一命令，实际复用 Claude 路径；Codex 的按 ID 查询要用 `ccusage codex session --id`（`docs/guide/session-reports.md:20-23`）。

- 统一路径的 Claude 部分：`summarize_entry_sessions` 用 `BTreeMap<(String, String), SessionAccumulator>`，键是 `(entry.project_path, entry.session_id)`（`all/src/loader.rs:734-746`）。
- 统一路径的 Codex 部分：`period = event.session_id`（`codex/src/aggregate.rs:889-894`）。
- 行键统一收敛为 `summary.session_id`：`summary_rows` 依次尝试 `date → week → month → session_id`（`all/src/loader.rs:778-784`），因此会话行 `period` = `session_id`；同时丢弃全零行（`total_tokens == 0 && total_cost == 0.0`，`loader.rs:786-789`）。
- 也就是说：**分组用的是 `(项目路径, 会话 ID)`，展示与跨源对齐用的是 `session_id`**。两个不同项目下的同名 session_id 在统一报告里会成为两行、`period` 相同（`AllRow.agent` 不同），不会相加。

#### 5.2 `first_activity` / `last_activity`

- 由 `SessionAccumulator` 计算（见 §1.4）：`last_activity` = 组内**最大** `timestamp`（取该条 entry 的 session_id/project_path），`first_activity` = 组内**最小** `timestamp`（`summary.rs:134-169`）。
- 两者是 UTC RFC3339 毫秒串（`summary.rs:165-166`），JSON 原样输出（`output.rs:67-68`）；表格列"Last Activity"按 `truncate_rfc3339_to_date` 截成前 10 字符（`output.rs:583-585`, `307-311`），即只显示日期。
- 表格是否包含该列由"是否任意行有 `last_activity`"决定（`output.rs:245`, `253-256`），因此日/周/月报告通常不出现该列。

#### 5.3 `--id` 过滤（`commands/mod.rs:208-260`）

入口是 `run_session` 的首个分支：`if let Some(id) = args.id { return run_session_id(&id, &shared) }`（`commands/mod.rs:148-150`），因此 `ccusage session --id`、`ccusage claude session --id`、以及统一命令下的 `--id` 全部落到同一函数、只读 Claude 日志。

```text
run_session_id(id, shared): entries := load_entries_since → filter_loaded_entries_by_date
    session_entries := filter(e -> e.data.session_id == Some(id) || e.session_id == id)，按 timestamp 升序
    空结果：--json → 打印 "null"；文本 → stderr "No session found with ID: {id}"
    文本输出：标题 + Total Cost / Total Tokens / Total Entries
    JSON（:234-249）：{sessionId, totalCost, totalTokens, entries:[{timestamp, inputTokens,
                      outputTokens, cacheCreationTokens, cacheReadTokens, model, costUSD}]}
```

- `--id` 匹配同时看原始 JSON 的 `sessionId` 与适配器归一后的 `session_id`，因此 Claude 原始 ID 与派生 ID 都能命中（`commands/mod.rs:213-215`）。
- `--id` 与 `--sections`/`--by-agent` 互斥，报 `The --sections and --by-agent options cannot be used with session --id.`（`parser.rs:452-458`）。
- Codex 的 ID 形式更多（完整 ID、文件名、尾 UUID、`codex://threads/<uuid>`），短 UUID 被拒绝，歧义命中报错（`docs/guide/session-reports.md:57`）。

#### 5.4 排序规则

- 顶层 `claude session` 列表：`run_session` 强制 `session_shared.order = SortOrder::Desc`（`commands/mod.rs:152-153`），然后按 `total_cost` 降/升序排序（`commands/mod.rs:158-161`）——**默认成本降序**，与 `--order` 默认值（Asc）无关。
- 统一 all 报告：`finish_rows`（`all/src/loader.rs:398-420`）对 Session 分支先清 `metadata_agents`，再排序：显式 `--order asc` → 成本升序，否则成本降序；平局按 `period` 再按 `agent` 升序（`loader.rs:403-413`）。非 Session 分支走 `aggregate_rows` + `sort_rows`（按 `period` 字符串再按 `agent`，`Desc` 时整体反转，`report.rs:578-586`）。
- 会话报告**没有时长字段**：不输出 duration，也不按时长排序，只有 `firstActivity`/`lastActivity` 两个时间端点（`docs/guide/session-reports.md:89-100` 同口径）。
- 过滤语义（Claude）：日期过滤在会话总计之前应用，故 token/cost 只统计窗口内条目，且随 `--timezone` 变化（测试 `commands/mod.rs:964-1018`；`docs/guide/session-reports.md:142`）。

---

### 6. 5 小时 blocks（重点）

#### 6.1 常量与入口

```text
DEFAULT_SESSION_DURATION_HOURS = 5.0     # ccusage-core/src/lib.rs:59
DEFAULT_RECENT_DAYS = 3                  # lib.rs:60
BLOCKS_WARNING_THRESHOLD = 0.8           # ccusage/src/blocks.rs:18
BLOCKS_COMPACT_WIDTH_THRESHOLD = 120     # blocks.rs:20
```

- CLI `--session-length` / `-n` 默认取 `DEFAULT_SESSION_DURATION_HOURS`（`parser.rs:195`, `208-213`，由 `cli.rs:19-24` 传入），且在 `>` 0 时校验：`Session length must be a positive number`（`commands/mod.rs:263-265`）。
- 时长还能来自配置文件：配置键 `blocks.sessionLength`（schema 字段 `session_length: Option<f64>`，`config_schema.rs:463-471`, `653-660`，schema 默认 5.0 `:902`）会写进同一个 `args.session_length`（`config.rs:438-454`，赋值点 `:450-452`），命令行 `-n` 在其后解析因而可以覆盖；配置与 flag 都缺失时才用 `DEFAULT_SESSION_DURATION_HOURS` 默认值。
- 块长度单位换算：`session_duration = (hours * MILLIS_PER_HOUR as f64) as i64`（`blocks.rs:60`）。

#### 6.2 `identify_session_blocks` 完整算法（`blocks.rs:53-103`）

```text
identify_session_blocks(entries, hours):
    空输入 → []                                              # :57-59
    duration := hours * 3_600_000；entries 按 timestamp 稳定升序；now := utc_now()   # :60-62
    for entry in entries:
        if current_start is Some(start):
            last := current_entries.last().timestamp or start
            if entry.ts - start > duration or entry.ts - last > duration:      # :75 严格大于
                推送 block(start, current_entries)                              # :76-81
                if entry.ts - last > duration: 插入 gap block(last, entry.ts)    # :82-88
                current_start := floor_to_hour(entry.ts)                        # :89
        else: current_start := floor_to_hour(entry.ts)                          # :91-93
        current_entries.push(entry)
    收尾：current_entries 非空 → 推送 block(start, ...)                        # :97-101
```

关键语义（压缩版）：

- **窗口长度**：默认 5 小时，可配置；`end_time = start + duration`（`blocks.rs:115`）。
- **锚点与对齐**：块起点 = 该块首个 entry 的时间戳向下取整到整点（`blocks.rs:89`, `92`, `105-107`）；`floor_to_hour` 走整数 `div_euclid(3600000)`（`date_utils.rs:58-60`），即 **UTC 整点对齐**，显示时再按本地时区渲染（`local_parts`，`blocks.rs:292-315`）。文档同口径（`docs/guide/blocks-reports.md:48`）。
- **切分条件（或关系）**：距块起点 > duration **或** 距上一条 entry > duration 都开新块（`blocks.rs:75`）。阈值是**严格大于**：恰好等于 5 小时的间隔不断块（测试 `commands/mod.rs:1276-1279`）。
- **gap 块**：仅 `since_last > duration` 时插入（`blocks.rs:82-88`），`start = last + duration`、`end = 下一条 entry`、`id` 前缀 `gap-`、无 entries/token/cost（`blocks.rs:149-164`）。
- **时间来源**：只有 `is_active` 与 projection 用 `now`（`blocks.rs:117`, `592`），历史切分只依赖 entry 时间戳。

#### 6.3 block 内 token 归属（无跨块拆分）

```text
create_block(start, entries, now, duration):
    end       := start + duration
    actual_end:= entries.last().timestamp                       # :116
    is_active := actual_end 有值 && (now - actual_end < duration) && (now < end)   # :117
    for entry in entries:
        token_counts.add_usage(entry.usage)                     # :123-124（四桶，extra 不参与）
        cost += entry.cost                                      # :125
        model 别名归一后按首见序收集                            # :126-131
        usage_limit_reset_time := 首个 Some 值                  # :132
```

- 每条 entry **整条**归入其所在块，没有任何"按时间比例分摊"的逻辑；跨块的会话会在两处各自累计（并由 gap 块标记中间空隙）。
- `token_counts` 的 `total()` 含 `extra_total_tokens`（`types.rs:89-95`），但 `create_block` 只 `add_usage`（不含 `extra_total_tokens`），因此 block 的 `extra_total_tokens` 恒为 0 —— 与 `UsageSummary`/`ModelBreakdown` 路径不同。
- 模型列表同样是"首见序 + 去重"（`blocks.rs:120-131`），表格渲染时经 `format_models_multiline` 按名重排（`blocks.rs:234-240` → `output.rs:522-534`）。

#### 6.4 `calculate_burn_rate`（`blocks.rs:567-584`）

```text
calculate_burn_rate：entries 为空或 is_gap → None                       # :568-570
    duration_minutes := (last - first) / 60_000（块内首末 entry 差值，非窗口长度）
    duration_minutes <= 0 → None（单条 entry 即 None）                    # :574-576
    tokens_per_minute := total_tokens / duration（含 cache 全量）
    tokens_per_minute_for_indicator := (input + output) / duration       # 状态灯口径
    cost_per_hour := cost / duration * 60                               # :582
```

- 返回结构 `BurnRate` 字段为 camelCase JSON：`tokensPerMinute`/`tokensPerMinuteForIndicator`/`costPerHour`（`blocks.rs:37-43`）。
- statusline 的强度分级用 `tokens_per_minute_for_indicator`：<2000 🟢 Normal、<5000 ⚠️ Moderate、否则 🚨 High（`commands/mod.rs:461-467`）。

#### 6.5 `project_block_usage` 与限额状态（`blocks.rs:586-608`）

```text
project_block_usage(block):   仅 is_active 且非 gap
    remaining_minutes := round((end - now) / 60_000)                     # :591-592
    total_tokens := 当前 token + tokens_per_minute * remaining_minutes   # :593-594
    total_cost   := cost + cost_per_hour/60 * remaining_minutes，保留 2 位  # :595-598
parse_token_limit(value, max_tokens):
    None | "" | "max" → (max_tokens > 0).then_some(max_tokens)            # :603-605
    其它 → value.parse().ok()                                             # :606
```

- `tokenLimitStatus` 的 `status` 三态：`exceeds`（超限）、`warning`（> 80%）、`ok`（`blocks.rs:195-204`）；表格里 `%` 列与剩余/预计行也走同一阈值（`blocks.rs:393-458`）。
- **单条 entry 的活跃块没有 projection**：`project_block_usage` 第二行就是 `let burn = calculate_burn_rate(block)?`（`blocks.rs:590`），而 burn rate 在块内首末时间戳相同时返回 `None`（`blocks.rs:574-576`）；`?` 传播使整个 projection 变 `None`。由于 `tokenLimitStatus` 也建立在 projection 之上（`projection.and_then(...)`，`blocks.rs:195-204`），这类块在 JSON 里同时缺失 `projection` 与 `tokenLimitStatus`；表格侧 `PROJECTED` 行同理不出现（`blocks.rs:437-458`）。
- `max_tokens` 由命令层在过滤后计算：**非 gap 且非 active** 的块中最大的 token 总数（`commands/mod.rs:283-288`）。因此 `blocks --active -t max` 时 `max_tokens == 0`，`max` 退化为 `None`，`%` 列与限额区段都不会出现（`blocks.rs:603-605`）。这是一个容易踩的语义坑。

#### 6.6 `--active` / `--recent`（`commands/mod.rs:272-306`）

```text
--recent : cutoff := utc_now - DEFAULT_RECENT_DAYS*MILLIS_PER_DAY
           retain(block.start_time >= cutoff || block.is_active)          # :272-277
--active : retain(block.is_active)                                        # :279-281
```

- `--recent` 是"最近 3 天起的块 + 当前活跃块"，用块起点比较，且始终保留活跃块（即使起点早于 3 天）。
- `--active` 后若结果为空 → `No active session block found.`（`commands/mod.rs:298-301`）；恰好 1 块 → 打印详情视图而非表格（`commands/mod.rs:302-305`，详情函数 `blocks.rs:465-565`）。
- 过滤顺序：`identify → filter_blocks_by_date(--since/--until) → sort_blocks(order) → recent → active → 计算 max_tokens`（`commands/mod.rs:266-288`）。`--since/--until` 对块的作用是按 `block.start_time` 的本地日期比较（`blocks.rs:166-175`，内部 `format_date(..., shared.timezone)`）。
- 默认排序是 `SortOrder::Asc`（`SharedArgs::with_defaults`，`ccusage-cli/src/types.rs:63-70`），`sort_blocks` 先升序、`Desc` 时整体反转（`blocks.rs:177-182`）——即**默认最旧在前**，与 `docs/guide/blocks-reports.md:149-155` 的"newest blocks first (default)"矛盾（见"未验证/存疑"）。

#### 6.7 statusline 用的有界块搜索（`commands/mod.rs:557-671`）

```text
STATUSLINE_BLOCK_LOOKBACK_DAYS = [1, 7, 30]                       # :559
依次尝试（session 窗口 + 回看天数）的 since，用 statusline_block_anchor 判定窗口是否足以定界：
    NoActiveBlock   → None（窗口内已无活动）                        # :587
    From(start)     → 只保留 >= start 的 entry 后 identify          # :588-591
    NeedsMoreHistory→ 放宽回看天数                                  # :592
    NeedsFullHistory→ 跳出循环，全量加载                             # :593, 596
锚点判定：最后一条 > now → NeedsFullHistory；now - 最后一条 >= session → 无活跃块；
          否则取"最后一段 > session 的空隙"之后的首个时间戳为锚点     # :631-647
```

- 这是性能优化：无界块划分只依赖"最后一次超过一个 session 窗口的空闲"，所以只要窗口回看足够长就能复现同样的活跃块。
- `find_active_block` 最终仍调用 `identify_session_blocks(entries, DEFAULT_SESSION_DURATION_HOURS)` 并取 `is_active && !is_gap`（`commands/mod.rs:599-603`）——statusline 的活跃块口径与 `blocks` 命令默认口径一致（都用 5 小时，不受 `--session-length` 影响）。

---

### 7. statusline

#### 7.1 入口与输入

- `commands::run_statusline`（`commands/mod.rs:310-376`）从 **stdin 读 JSON**（Claude Code statusline 钩子载荷），空输入报 `❌ No input provided`（`commands/mod.rs:318-322`），JSON 解析失败报 `Invalid input format`（`commands/mod.rs:324-325`）。
- 钩子结构 `StatuslineHook`（`commands/mod.rs:921-951`）：`session_id`、`transcript_path`、`model{id, display_name}`、可选 `cost.total_cost_usd`、可选 `context_window{total_input_tokens, context_window_size}`、可选 `effort.level`。
- 参数校验：`context_low_threshold < context_medium_threshold`，否则报错（`commands/mod.rs:311-316`）。默认阈值 50 / 80，默认 `refresh_interval = 1` 秒、`cache = true`（`ccusage-cli/src/types.rs:221-240`）。

#### 7.2 输出格式（单行文本，无 JSON/sed 变体）

```text
render_statusline 输出模板（commands/mod.rs:530-538）:
"🤖 {model}[ ({effort})] | 💰 {session} session / {today} today / {block}{burn} | 🧠 {context}"

{block}  = "$x.xx block (2h 15m left)" 或 "No active block"        # :482-492
{burn}   = " | 🔥 $y/hr 🟢 (Normal)" 等，按 --visual-burn-rate 组合 emoji/文本   # :456-481
{context}= "120,000 (60%)"，百分比着色由阈值三元决定                   # :684-711
{cost_source=both} → "($a cc / $b ccusage)"                         # :511-520
{model}  = --model-label-aliases 命中别名，否则 display_name          # :392-413
```

- `StatuslineArgs` **没有 `json`/`jq`/`order`/`compact` 字段**（`ccusage-cli/src/types.rs:156-171`），解析器对 `statusline` 只接受 `-O/--offline`、`--no-offline`、`-B/--visual-burn-rate`、`--cost-source`、`--cache`、`--no-cache`、`--refresh-interval`、`--context-*-threshold`、`-z/--timezone`、`--config`、`--debug`（`parser.rs:219-263`，未知项报 `Unknown statusline option '...'`）。因此不存在 JSON 输出或 `sed` 变体；输出就是一行带 emoji 的文本，颜色由终端样式决定（`NO_COLOR`/`FORCE_COLOR`/TTY，`ccusage-terminal/src/style.rs:39-42`），与 `--json` 无关。

#### 7.3 缓存策略（有状态文件，位于临时目录）

```text
cache_enabled := args.cache && !args.no_cache                        # commands/mod.rs:327
cache_path    := env::temp_dir()/ccusage-semaphore/{session_id}.lock  # :860-864
StatuslineCache（serde camelCase）: date, lastOutput, lastUpdateTime, transcriptPath,
    transcriptMtime, isUpdating, pid                                  # :774-788
流程：current_mtime := transcript mtime（失败 → 0，:330, 899-907）
  命中判定 cached_statusline_output（:826-845）：
    expired := now - lastUpdateTime >= refreshInterval*1000；file_modified := mtime 不等
    expired 或 file_modified 时：isUpdating 且 pid 存活 → 仍返回旧输出（避免抖动），否则未命中
  未命中 → 先写 "updating" 占位（:345-347），渲染成功写 completed（:353-357）
  渲染失败 → 打印旧输出或 "❌ Error generating status"，release 后向上返回错误（:360-373）
pid 存活：Unix kill(pid,0)；非 Unix 平台仅比较 pid == 自身（:847-858）
```

- 缓存是"每会话一个 lock 文件"，字段名暗示它同时充当轻量互斥/防重入；缓存目录在系统临时目录下（`ccusage-semaphore`），不是用户配置目录。
- `--refresh-interval` 单位是**秒**，比较时乘 1000（`commands/mod.rs:836`）。
- `transcript` 内容变化用 mtime 近似检测，条目内容改变但 mtime 不变时不会失效（见"未验证/存疑"）。

#### 7.4 数据来源与今日成本

- 会话成本三种来源：`--cost-source cc`（用钩子里的 `cost.total_cost_usd`）、`ccusage`（自算，遍历全部 entry 匹配 session_id 求和，`commands/mod.rs:673-682`）、`auto`（先 cc 再 ccusage）、`both`（两个都显示）（`commands/mod.rs:420-440`）。
- 今日成本：`statusline_today_shared` 以 `format_date(now, args.timezone)` 得到当天 `YYYYMMDD`，同时写 `since`/`until`（`commands/mod.rs:541-555`），再按 `entry.date == since` 精确过滤求和（`commands/mod.rs:442-453`）——因为 `load_entries_since` 可能带回窗口外条目（见 §3.2）。
- 状态栏共享参数只保留 `offline` 与 `pricing_overrides`（`commands/mod.rs:378-384`），其余字段用 `SharedArgs::default()`。
- 上下文 token：优先用钩子 `context_window`；缺失时回读 transcript 最后一条 `type == "assistant"` 的 usage（`input + cache_creation + cache_read`），窗口大小取模型 context limit（未命中时 200_000）（`commands/mod.rs:713-772`）。

---

### 8. JSON 输出

#### 8.1 行级 JSON

```text
summary_json(row)（output.rs:27-56）:
  { inputTokens, outputTokens, cacheCreationTokens, cacheReadTokens, totalTokens,
    totalCost, modelsUsed, modelBreakdowns } + 可选 date / month / week / project / credits
session_summary_json(row)（output.rs:58-77）:
  { sessionId, ...同上四桶/totalTokens/totalCost, lastActivity, firstActivity,
    modelsUsed, modelBreakdowns, projectPath }（+ credits 若有）
agent_summary_json(row, kind, include_session_metadata)（agent_report.rs:5-48）:
  周期键随 kind：daily→"date" / weekly→"week" / monthly→"month" / session→"sessionId"
  值取 summary_period（date→week→month→session_id 首个存在者，agent_report.rs:59-66）
  include_session_metadata 时追加 lastActivity / firstActivity / projectPath（可为 null）
```

- `modelsUsed` 与 `modelBreakdowns` 直接来自 `UsageSummary`，顺序即 §1.3 的规则（`modelsUsed` 首见序、`modelBreakdowns` 成本降序）。
- `ModelBreakdown` 的 JSON 字段为 camelCase `modelName`/`inputTokens`/`outputTokens`/`cacheCreationTokens`/`cacheReadTokens`/`cost`，`extraTotalTokens` 被 `#[serde(skip_serializing)]` 隐藏，`missingPricing` 仅在为 `true` 时出现（`ccusage-core/src/types.rs:98-112`；测试 `output.rs:736-743`）。
- `UsageSummary` 本身也是 `Serialize`，带 `skip_serializing_if = "Option::is_none"`（`types.rs:147-180`），但报表 JSON 主要走上面三个显式构造函数。
- 顶层包装：
  - daily：`{ "daily": [...], "totals": {...} }`（`commands/mod.rs:52-56`）；`--instances` 时改为 `{ "projects": {name: [...]}, "totals": {...} }`（`commands/mod.rs:45-50`, `group_project_output` `output.rs:147-156`）。
  - monthly/weekly：键分别为 `monthly`/`weekly`（`commands/mod.rs:89-99`, `126-133`）。
  - session：`{ "sessions": [...], "totals": {...} }`（`commands/mod.rs:163-170`）。
  - blocks：`{ "blocks": [...] }`（无 totals，`commands/mod.rs:290-296`）。
  - all（统一）：`{ daily|weekly|monthly|session: [...], "totals": {...} }`（`all/src/report.rs:28-37`, `205-212`）；`--sections` 时按 `OrderedJsonMap` 依 section 顺序输出各键并附 `totals`（`all/src/report.rs:39-61`, `101-131`）。

#### 8.2 `totals_json`（`output.rs:79-121`）

```text
inputTokens / outputTokens / cacheCreationTokens / cacheReadTokens   # 逐行 saturating 求和
totalTokens  = input + output + cache_creation + cache_read + extra   # :105-109（extra 也计入）
totalCost    = Σ row.total_cost（浮点，无饱和）
credits      = Σ 有值项，且仅当 > 0.0 才写入 "credits"                  # :112-115
unpricedModels = 命中 missingPricing 的模型名，BTreeSet 去重排序后写入，
                 为空则不写该键                                       # :116-119, 126-145
```

- **是的，totals 含 cache 分解**：`cacheCreationTokens` 与 `cacheReadTokens` 独立字段，且 `totalTokens` 把两者与 `extra` 都算进去（对 `UsageSummary.total_tokens()` 语义一致，`types.rs:183-190`）。
- 统一 all 报告的 `totals_json`（`all/src/report.rs:174-203`）结构相同，但用 `json_float` 输出成本、且按 `AllRow.total_tokens` 直接求和（该值里 extra 已在源侧吸收，`all/src/types.rs` 的 `AllRow` 无独立 extra 字段，`report.rs:343-367` 反推）。
- blocks 的 JSON 没有 totals 段；块级 `tokenCounts` 只含四桶（无 extra），且 `totalTokens` 由 `TokenCounts::total()` 得出（`blocks.rs:213-219`）。

#### 8.3 `--json` 与 `--jq`（`output.rs:14-16`, `158-187`）

```text
wants_json(shared) = shared.json || shared.jq.is_some()      # :14-16 —— --jq 单独出现也会走 JSON 分支
print_json_or_jq(value, jq, no_cost):
    no_cost → strip_cost_json(递归删除 "totalCost"/"costUSD"/"cost")   # :159-161, 564-581
    jq = Some(f) → 启动外部 `jq f` 子进程，把 JSON 写入其 stdin，
                   stdout 继承；子进程非零退出 → Err("jq failed")       # :162-178
    否则 → serde_json::to_writer_pretty 到 stdout 并补换行               # :179-186
```

- `--jq` 依赖系统 PATH 里的 `jq` 可执行文件，spawn 失败报 `failed to run jq: ...`（`output.rs:168-169`）；筛选逻辑本身不在 ccusage 内实现。
- `--no-cost` 是 JSON 层面递归删字段，不只是隐藏列：只要键名命中 `totalCost` / `costUSD` / `cost` 就会被移除（`output.rs:564-581`）。据此在 blocks JSON 里的实际效果是：`blocks[].projection.totalCost`、`totals.totalCost`、`daily[].totalCost`、`modelBreakdowns[].cost` 等消失；而 `burnRate` **不受影响** —— `BurnRate` 的 serde 字段是 `tokensPerMinute` / `tokensPerMinuteForIndicator` / `costPerHour`（`blocks.rs:37-43`），其中 `costPerHour` 与三个删除键都不同名，会被原样保留（`block_json` 写入点 `blocks.rs:222`）。`output.rs:746-819` 的测试用的是自造键 `burnRate.cost`，只能证明删除函数按名递归，不足以说明真实 blocks JSON 会丢失 burn rate 成本。
- 统一 all 报告的 `--jq` 走 `print_sections_report_json` 的等价实现（`all/src/report.rs:63-99`）。

#### 8.4 `json_float` 对小数位的影响（`output.rs:452-462`）

```rust
pub fn json_float(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && 在 i64 范围内 { json!(value as i64) }
    else { json!(value) }
}
```

- 整数值成本会被写成 JSON **整数**（`156.4` → `156.4`；`0.0` → `0`；`120.0` → `120`），非有限值（NaN/Inf）与超出 i64 的值原样输出为浮点（JSON 序列化会失败/写 null 的行为取决于 serde_json，未实测）。
- 使用范围有限：`block_json` 的 `costUSD`（`blocks.rs:220`）、all 报告 `totalCost`/成本字段（`all/src/report.rs:169`, `196`）、以及 `summary_metadata` 里的 credits（`all/src/loader.rs:813`）；而 `summary_json`/`session_summary_json`/`totals_json` 用**原始 f64**（`output.rs:34`, `66`, `110`），因此同一数值在不同报表里可能一个是 `0`、一个是 `0.0`。`format_currency` 只在文本表格里把成本固定为两位小数（`output.rs:548-550`）。

---

### 9. 表格渲染（概述）

- 入口 `print_usage_table` → `print_usage_table_with_options`（`output.rs:204-232`）；空行集打印 `No usage data found.` 到 stderr 并返回 `Ok`（`output.rs:233-236`）。
- 紧凑模式判定（`output.rs:18-25`）：`shared.compact || (is_stdout_tty && terminal_width < threshold)`。使用表格的阈值是 `USAGE_COMPACT_WIDTH_THRESHOLD = 100`（`ccusage-core/src/lib.rs:61`；调用点 `output.rs:239-244`；测试 `output.rs:592-614`：非 TTY + 宽度 80 **不会**自动紧凑）；blocks 表格用独立的 120（`blocks.rs:20`, `343-348`）；all 报告复用 100（`all/src/report.rs:227-232`）。宽度来源：`COLUMNS` 或终端探测，缺省 120（`ccusage-terminal/src/terminal.rs:3-18`）。
- 列集合 `usage_table_columns`（`output.rs:385-404`）：非紧凑 `[首列, Models, Input, Output, Cache Create, Cache Read, Total Tokens, Cost (USD)]`；紧凑 `[首列, Models, Input, Output, Cost (USD)]`；`--no-cost` 弹掉末列（`output.rs:249-252`）；"Last Activity" 列按需追加（任一 row 有 `last_activity`，`output.rs:245`, `253-256`），值截断为日期（`output.rs:583-585`）。
- 模型列 `format_models_multiline`（`output.rs:522-534`）：每个模型取 `short_model_name` 短名 → **按名字排序** → 去重 → 每行前缀 `- `，用 `\n` 连接；与 JSON 的 `modelsUsed` 顺序不同（后者不排序）。
- 行渲染：首列 label 依次取 `date → month → week → session_id`（`output.rs:277-283`）；`--instances` 插入 `Project: <name>` 分组头与分隔线（`output.rs:261-276`, `464-470`）；`--breakdown` 追加灰显模型明细行（`output.rs:313-322`, `472-520`）；表尾用 `totals_json` 拼 "Total" 行（`output.rs:325-375`），随后按需打印缺失定价告警与紧凑模式提示（`output.rs:377-381`, `410-450`）。
- 日期列做"日期压缩"（`.with_date_compaction(true)`，`output.rs:257-259`；all 报告 `all/src/report.rs:234-236`），具体压缩规则在 `ccusage-terminal/src/table.rs`（本次未逐行核对）。

---

### 10. `--by-agent` / all 报告

#### 10.1 多 agent 合并模型

```text
load_base_rows（all/src/loader.rs:100-396）: 每个内置 agent 一个 AgentLoadSpec{index, agent, load}
    线程作用域内并行加载，结果按 index 排序保证确定性（:422-480）；append_agent_rows 打 detected 标并拼接（:482-492）
finish_rows（:398-420）: Session → 清 metadata_agents，按成本（默认降序）+ period + agent 排序
                        其它 → aggregate_rows(rows, kind) → sort_rows(order)
aggregate_rows（:888-908）: period 归一（Daily 原样 / Monthly [..7] / Weekly week_start(周一) / Session 原样）
    → BTreeMap<归一 period, AllAccumulator> → into_row(period)
```

#### 10.2 `AllAccumulator` / `AllRow`（`all/src/types.rs:7-22`, `64-180`）

- `AllRow`：`period`、`agent`、`models_used`、四桶 token、`total_tokens`、`total_cost`、`metadata`（agent 私有细节）、`metadata_agents`、`agent_breakdowns`（同 period 下按 agent 拆分）、`model_breakdowns`（`types.rs:7-22`）。
- `AllAccumulator.add`：token 全部 `saturating_add`，成本 `+=`；`models` 是 `BTreeSet<String>`（**按名字升序**，与单 agent 路径的"首见序"不同，`types.rs:88`, `119`）；同 agent 的行合并进同一 breakdown（`types.rs:94-105`）。
- `into_row`：`agent_breakdowns` 按 `agent` 名升序（`types.rs:113`），`model_breakdowns` 合并后按成本降序（`types.rs:114-115`, `178`）；行 `agent` 固定为 `"all"`（`types.rs:116-130`）；跨 agent 合并同名模型时四桶相加、`missing_pricing` 取或（`types.rs:152-180`）。

#### 10.3 `--by-agent` 的作用

- CLI：`--by-agent` 只在统一命令（顶层/根）可用，解析为 `AgentCommandArgs.by_agent`（`parser.rs:386-400`）；`session --id` 与之互斥（`parser.rs:452-458`）；各具体 agent 子命令拒绝该 flag（断言 `Unknown codex option '--by-agent'` 位于 `ccusage-cli-parser/src/tests.rs:644-645`，非 `parser.rs`）。
- 行为：`include_agents = args.by_agent`（`all/src/lib.rs:41`）→ JSON 行里追加 `"agents": [ ... ]`，元素为各 agent 的 `agent_json`（`agent`/`modelsUsed`/四桶/totalTokens/totalCost/modelBreakdowns，`all/src/report.rs:133-172`）；默认不输出该键（测试 `all/src/tests.rs:447`, `545`）。
- 表格：`--by-agent` 不改变表格结构；表格**始终**在行下展开各 agent 子行（`AllRow.agent_breakdowns` 存在即渲染，`all/src/report.rs:238-255`），主行的 Agent 列显示 `All`、子行显示 `- Claude`/`- Codex` 等（`report.rs:412-434`, `597-620`）。
- 标题里的 `Detected: ...` 列表来自 `detected_agents` 或行内 agent 集合（`all/src/report.rs:369-410`）。

#### 10.4 `summary_period` 的作用与日期列对齐

- `summary_period(row)` = `date → week → month → session_id` 首个存在的键（`agent_report.rs:59-66`），两种用途：
  1. 各 agent 自带命令（`ccusage codex daily` 之类）的行 key 与排序（`agent_summary_json` 的周期键 `agent_report.rs:10-12`, `68-75`；排序调用见 `adapters/*/src/lib.rs`，例如 `opencode/src/lib.rs:41`、`kimi/src/lib.rs:31`）。
  2. all 报告里 `summary_rows` 用它决定 `AllRow.period`（`all/src/loader.rs:778-784`），从而让不同 agent 的同一日期字符串落到同一 `BTreeMap` 分组（`aggregate_rows`，`loader.rs:888-908`）。
- **日期列对齐机制**：每个 agent 各自用**同一个 `--timezone`**把时间戳转成日期字符串（Claude：`claude/src/lib.rs:524`；Codex：`codex/src/aggregate.rs:415`, `888`；其余适配器同 §2.3），再用纯字符串相等合并 —— 即"同名即同桶"，不存在二次换算或 UTC 归一；任一 agent 的日期口径有偏差（例如按 UTC 生成）会表现为独立的相邻日期行，而不会被自动纠正。周/月在 all 报告里由 `aggregate_rows` 二次归一（周固定周一，`loader.rs:897`），与 `ccusage claude weekly --start-of-week` 的周起点设置无关。
- `AllRow.metadata`（如 Claude 的 `credits`、Codex 的 `lastActivity`/`reasoningOutputTokens`）只在 JSON 的 `metadata` 字段出现（`all/src/loader.rs:810-828`, `878-881`；`all/src/report.rs:138-147`）。

---

### 未验证 / 存疑

> **本节状态（第 8–9 轮实测后）**：§13.5 已实测 `blocks --json` 的字段全集（对应本节第 6 条）与 `claude statusline --help` 的默认值（第 8 条）；第 9 轮全文复核又实测闭环了第 1 条（`blocks --json` 数组**升序、最旧在前**，与代码默认 `SortOrder::Asc` 一致，官方文档的 "newest first" 不成立）。其余条目仍未闭环。

1. **blocks 默认排序与文档冲突**：代码默认 `SortOrder::Asc`（`ccusage-cli/src/types.rs:63-70`），`sort_blocks` 升序后再按需反转（`blocks.rs:177-182`），即默认最旧块在前；但 `docs/guide/blocks-reports.md:147-155` 写"Show newest blocks first (default)"。我没有运行二进制确认实际输出顺序，也未找到任何把 blocks 默认改为 Desc 的赋值点（`run_session` 才显式改 Desc，`commands/mod.rs:153`）。**结论以代码为准，文档疑为过时**。
2. **`TZ` 环境变量语义**：ccusage 自身不读 `TZ`；仅当最终落到 jiff `TimeZone::system()` 时由 jiff 读取（证据为 registry 中 jiff-0.2.38 源码 `src/tz/system/mod.rs:185` 与其文档 `src/tz/timezone.rs:291-311`）。我未运行 ccusage 验证 `TZ=Asia/Tokyo ccusage daily` 的实际日期归属，也未核对 Windows 平台 jiff 的 `system()` 分支实现细节（`src/tz/system/windows/mod.rs` 仅见片段）。
3. **`--active` 单块详情的时区不一致**：`Block Started` 用 `format_utc_second`（`blocks.rs:475`），表格用 `local_parts`（`blocks.rs:292-315`），两者口径不同。未实测；推断为有意（详情对齐 JSON 的 UTC 口径）或历史遗留。
4. **`json_float` 的极端输入与 `format_utc_minute` 调用点**：NaN/Inf 或超 i64 范围时 serde_json 的输出行为未实测（`output.rs:452-462` 只保证分支条件）；`format_utc_minute` 在仓库内仅出现于 `main.rs:231` 的测试断言，生产无调用点（`date_utils.rs:339` 带 `#[doc(hidden)]`），疑为遗留导出。
5. **statusline 缓存失效粒度**：以 `transcript` mtime 判定陈旧（`commands/mod.rs:837`），内容变化而 mtime 不变（或文件系统时间精度不足）时缓存会继续命中；未做实测。
6. **`docs/guide/weekly-reports.md:126-161` 的 JSON 示例**把 `modelBreakdowns` 写成对象映射、字段名与代码的数组结构（`output.rs:36` + `types.rs:100-111`）不符；示例 `totalCost` 等也可能过时。未逐字段核对全部文档示例。
7. **未覆盖的相邻实现**：`ccusage-terminal/src/table.rs`（多行单元格/日期压缩细节、列宽策略）未逐行核对；`blocks` 的 `--token-limit` 交互（`%` 列、剩余行）只有代码路径与阈值证据，未运行验证渲染结果。
8. **`block.extra_total_tokens` 恒为 0**：由 `create_block` 只调用 `add_usage`（`blocks.rs:123-124`）推断；若有适配器把溢出量写进 `entry.extra_total_tokens`（`utils.rs:22-38` 的 fallback 路径），块级 `token_counts.total()` 与 `UsageSummary.total_tokens()` 会不一致。未构造数据验证。
