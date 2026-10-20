# ccusage 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`ccusage/ccusage`](https://github.com/ccusage/ccusage)，
> main 分支 HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（`chore: release v20.0.31`，
> 提交时间 2026-10-13 22:29 +0100，monorepo 版本 `20.0.31`）。
> 迁移对照对象：`v19.0.3`（HEAD `4aedac0622232f4b8e93650e9b7b54535ef9cf2b`，TypeScript 实现）。
>
> 研究方式：浅克隆到临时目录后逐文件精读（`read` / `grep` / `glob`）；第 8 轮更进一步——
> **真的构建并运行了它**（`cargo build -p ccusage` + `--offline` 运行，数据源为本机 `~/.claude`
> 与 `~/.codex`，全程只读），并把 `build.rs` 与价格匹配算法用脚本离线重放（见 §13.5）。
> 未读取或记录任何会话内容，未改动本机任何数据目录。
> 结论一律落 `文件:行号`；关键断言由独立复核代理做对抗性证伪（见 §13）。
>
> 证据分档：**已验证**（源码可证且行号经复核）／**一致解释**（多条代码证据指向同一结论）／**未验证**（结构推断，未实测）。
>
> 行号口径：一律为**文件实际行号**（read 工具口径）。本仓 `(Get-Content | Measure-Object -Line)`
> 返回的是非空行数，会比实际行号小（如 `pricing.rs` 实际 6181 行 vs 非空 5759 行），本文不采用该口径。
>
> 本文只记录字段名、表名、路径规则与计算口径，不含任何真实会话内容。

## 目录

1. 范围与方法
2. 总体架构（仓库形态、crate 划分、入口分发）
3. 统一数据模型（行级 → 事件级 → 汇总级）
4. 主干与 Claude Code 适配器（发现、并行、解析、去重、边界）
5. Codex 适配器（累计值、压缩回放、速度倍率）
6. 其余 16 个适配器的数据面矩阵
7. 聚合、时间与时区（daily / monthly / weekly / session / blocks / statusline）
8. 定价数据来源、模型名匹配链与金额计算
9. 配置、CLI 解析与输出契约
10. 版本史：v19 TypeScript → v20 Rust 的语义变化
11. 实现与官方文档的差异清单
12. 与 TokenScope 的对照：可抄的与要避的
13. 复核记录与残留未决问题
14. 横向专题：项目名、坏行容错、路径立场与测试基建
- 附录 A：关键常量与阈值速查
- 附录 B：模块与入口索引

**阅读约定**：`## N.` 是本文档章节（1–14 与附录）；各章内的 `### N.` / `#### N.M` 是**该章独立编号**的小节。
正文里形如「§4.6」「§8」的引用沿用其所在分片的章内编号习惯——
即「§4.6」= 第 4 章的第 6 小节（不是文档的第 4.6 章）；跨章引用一律写出章号（如「见 §8 定价章」「§13 复核记录」）。
**裸文件名约定**：未写全路径的文件名（如 `pricing.rs`、`parser.rs`）按其所在章的引言约定解析；
各章引言都列了本章适用的「裸文件名 → 完整路径」对应表。

## 1. 范围与方法

- 仓库形态（**已验证**）：根为 pnpm monorepo（`package.json`，`"packageManager": "pnpm@12.11.2"`），
  workspace 成员为 `apps/*`、`docs`、`packages/*`；Rust 源码不在根 workspace 内，而在
  `rust/`（独立 Cargo workspace，`rust/Cargo.toml` 的 `members = ["adapters/*", "crates/*"]`）。
  分发链：npm 包 `ccusage` → 平台预编译包 `packages/ccusage-{darwin,linux,win32}-{x64,arm64}`。
- 规模（read 工具实际行号）：`rust/crates/ccusage-core/src/pricing.rs` 6181 行、
  `rust/adapters/opencode/src/loader.rs` 2922 行、`rust/adapters/codex/src/loader.rs` 2682 行、
  `rust/adapters/copilot/src/loader.rs` 1990 行、`rust/adapters/codex/src/aggregate.rs` 1853 行、
  `rust/adapters/claude/src/lib.rs` 1582 行、`rust/crates/ccusage-adapter-all/src/loader.rs` 1637 行、
  `rust/crates/ccusage/src/commands/mod.rs` 1411 行、`rust/crates/ccusage-cli-parser/src/parser.rs` 1247 行。
- 方法：先读入口与类型层建立主干，再按「读取 → 归一 → 去重 → 聚合 → 计价 → 渲染」逐段精读；
  符号定位用 grep，关键结论落 `文件:行号`；正文中的结论由 4 组独立复核代理逐条证伪（§13）。
- 术语：
  **事件** = 一条可计量的用量记录（`LoadedEntry`，§3.3）；
  **cost mode** = `--mode` 的 auto / calculate / display 三态（§8.6）。

## 2. 总体架构

### 2.1 crate 划分与职责

| 层 | 位置 | 职责 |
| --- | --- | --- |
| CLI 入口 | `rust/crates/ccusage/src/main.rs` | 命令分发、全局装配（`pricing::set_json_fetcher(http::fetch_json)`） |
| 命令实现 | `rust/crates/ccusage/src/commands/mod.rs` | daily/monthly/weekly/session/blocks/statusline 的具体执行 |
| 参数类型 | `rust/crates/ccusage-cli/src/types.rs` | `SharedArgs`、`CostMode`、`SortOrder`、`WeekDay` 等 |
| 参数解析 | `rust/crates/ccusage-cli-parser/src/{parser,arg_parser,help,help_codegen}.rs` | 自研解析器 + 构建期生成的帮助页 |
| 配置 | `rust/crates/ccusage-config/src/{config,config_schema}.rs` | `ccusage.json` 查找、schema 生成与取值 |
| 核心 | `rust/crates/ccusage-core/src/*` | 类型、日期/时区、聚合、计价、输出渲染、进度 |
| 终端渲染 | `rust/crates/ccusage-terminal/src/*` | 表格、颜色、宽度、标题 |
| 统一报告 | `rust/crates/ccusage-adapter-all/src/*` | 无子命令 / `all` 报告，并行合并 18 家适配器 |
| 适配器 | `rust/adapters/<agent>/src/*` | 每家一个 crate：发现 + 解析 → `LoadedEntry` |
| 适配器公共件 | `rust/adapters/common/src/*` | 文件遍历、并行读取、JSONL 工具、通用报告 |

（**已验证**：`rust/Cargo.toml` 的 workspace 成员与上表一致；`pricing.rs` 等行数见 §1。）

### 2.2 入口分发

`main()`（`rust/crates/ccusage/src/main.rs:25-67`）先注册价格抓取回调，再按解析出的子命令分派：

| 子命令 | 入口 |
| --- | --- |
| 无子命令（默认）/ `all` | `adapter::all::run`（默认 `AgentReportKind::Daily`） |
| `daily` | `commands::run_daily` |
| `monthly` | `commands::run_bucket(shared, BucketKind::Monthly)` |
| `weekly` | `commands::run_weekly` |
| `session` | `commands::run_session` |
| `blocks` | `commands::run_blocks` |
| `statusline` | `commands::run_statusline` |
| `claude daily/monthly/weekly/session/blocks/statusline` | 由 `ccusage-cli-parser/src/parser.rs:557-587` 解析；`blocks`/`statusline` 仅 Claude 支持（`:864-880` 有专门报错文案） |
| `<agent> <report>`（17 个 agent） | `adapter::<agent>::run(args)` |

每个 adapter crate 都导出同名 `run`，`main.rs:90-113` 的测试 `agent_commands_are_exposed_by_independent_crates`
用 17 元函数数组 + `runs.len() == 17` 断言把这层契约钉死——**新增 agent = 新 crate + 一行分发**。

**三个数字的口径**（全文会同时出现，先说明以免混淆）：`rust/crates/ccusage/src/adapter/mod.rs:1-19`
导出的别名是 **19** 个（`all` + 18 家适配器）；`BUILT_IN_AGENT_NAMES`（`ccusage-core/src/lib.rs:63-82`）
列 **18** 家；而 `<agent> <report>` 的分发数组是 **17** 项——因为 `claude` 走独立子命令路径
（`ccusage-cli-parser/src/parser.rs:557-587`），不参与这组分发。

### 2.3 分层纪律

- 适配器只依赖 `ccusage-core`（类型与计价）与 `ccusage-adapter-common`（遍历与 JSONL 工具），
  彼此不互相依赖；`ccusage-adapter-all` 聚合全部适配器。
- 事件模型是唯一跨层契约：适配器产出 `LoadedEntry`（§3.3），聚合与渲染只消费它。
- **计价发生在适配器内**（解析一条即计价一次，`rust/adapters/claude/src/lib.rs:525`），不在渲染层重算。

## 3. 统一数据模型

### 3.1 行级：`UsageEntry`

**已验证**（`rust/crates/ccusage-core/src/types.rs:7-39`）：直接映射 Claude Code JSONL 的一行。

```rust
pub struct UsageEntry {          // serde(rename_all = "camelCase")
    pub session_id: Option<String>,
    pub timestamp: String,       // 原样字符串，稍后 parse_ts_timestamp
    pub version: Option<String>, // CLI 版本，用于 semver 校验
    pub message: UsageMessage,
    #[serde(rename = "costUSD")] pub cost_usd: Option<f64>,
    pub request_id: Option<String>,
    pub is_api_error_message: Option<bool>,
    pub is_sidechain: Option<bool>,
}
pub struct UsageMessage { pub usage: TokenUsageRaw, pub model: Option<String>, pub id: Option<String> }
pub struct TokenUsageRaw {
    pub input_tokens: u64, pub output_tokens: u64,
    pub cache_creation_input_tokens: u64, pub cache_read_input_tokens: u64,
    pub speed: Option<Speed>,                    // "standard" | "fast"
    pub cache_creation: Option<CacheCreationRaw>,// ephemeral_5m_input_tokens / ephemeral_1h_input_tokens
}
```

两个关键细节：

- `cache_creation_token_count()`（`types.rs:42-49`）：`cache_creation` 明细存在时取
  `ephemeral_5m + ephemeral_1h`（**saturating_add**），否则退回扁平的 `cache_creation_input_tokens`。
- 全仓 token 累加一律 `saturating_add`，`total()` 亦然（`types.rs:78-95`）；溢出饱和而非回绕，
  有单测 `saturates_token_count_accumulation_and_total`（`types.rs:196-218`）钉住。

### 3.2 token 四桶

`TokenCounts`（`types.rs:67-96`）与 `ModelBreakdown`（`types.rs:98-122`）都持有四桶
（input / output / cache_creation / cache_read）外加一个 **不序列化** 的 `extra_total_tokens`
（`#[serde(skip_serializing)]`，参与总数校验与 advisor 等附加来源）。
`input_tokens` 的语义是**未缓存的新增输入**：Claude 形态天然如此（`input_tokens` 与两个 cache 桶分开写），
而「含缓存输入」的来源会被拆成这个形状——**拆解发生在输出层**，不在解析归一阶段
（Codex 的解析只做上界钳制，扣减见 §5 第 3 小节的 `input - cached - cache_write`）。
缺失价格时 `ModelBreakdown.missing_pricing`（`types.rs:110-111`）仅在为 true 时序列化。

### 3.3 事件级：`LoadedEntry` / `LoadedFile`

**已验证**（`rust/crates/ccusage-core/src/types.rs:124-145`）：

```rust
pub struct LoadedEntry {
    pub data: UsageEntry,            // 原始行（保真）
    pub timestamp: TimestampMs,      // 毫秒 i64
    pub date: String,                // 按统计时区算出的 YYYY-MM-DD
    pub project: Arc<str>,           // 项目 slug（用于分组）
    pub session_id: Arc<str>,        // 会话标识（Arc 共享，省内存）
    pub project_path: Arc<str>,      // 项目展示路径
    pub cost: f64,                   // 计价结果（解析期即算好）
    pub extra_total_tokens: u64,
    pub credits: Option<f64>,        // Amp 等按 credits 计费的来源
    pub message_count: Option<u64>,
    pub model: Option<String>,       // 归一后的模型名（可能带 -fast 后缀）
    pub usage_limit_reset_time: Option<TimestampMs>,
    pub missing_pricing_model: Option<String>, // 未命中价格时留痕
}
pub struct LoadedFile { pub timestamp: Option<TimestampMs>, pub entries: Vec<LoadedEntry> }
```

`LoadedFile.timestamp` 记「文件内最小时间戳」（`rust/adapters/claude/src/lib.rs:660-666`）。
**注意（复核结论）**：该字段目前是死字段——唯一构造点是 `claude/src/lib.rs:501`，
消费方只读 `.entries`；`DailyLoadedFile.timestamp`（`claude/src/daily.rs:150`）同理。

### 3.4 汇总级：`UsageSummary`

`UsageSummary`（`types.rs:147-190`）是聚合后的行：`date`/`month`/`week`/`session_id`/`project_path`
等维度字段**全部是 `Option` 且按需序列化**（`skip_serializing_if = "Option::is_none"`），
所以同一结构体同时承载日、月、周、会话四种报告的 JSON。持有四桶 + `total_cost` + `credits` +
`models_used`（去重后的模型名列表）+ `model_breakdowns`（每模型一行 token 与费用）。
`extra_total_tokens` 同样 `skip_serializing`：参与 `total_tokens()` 但不进 JSON。

## 4. 主干与 Claude Code 适配器


> 只读仓库（第 1–7 轮为静态阅读；第 8 轮已构建并运行，见 §13.5）：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`
> HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31（Rust workspace：`rust/crates/*` + `rust/adapters/*`）。
> **本章裸文件名约定**：未写全路径的 `lib.rs`/`daily.rs`/`paths.rs` 指 `rust/adapters/claude/src/` 下同名文件；`loader.rs`/`report.rs`（统一报表语境）指 `rust/crates/ccusage-adapter-all/src/`；`output.rs`/`cost.rs`/`summary.rs`/`types.rs`/`date_utils.rs`/`utils.rs`/`project_names.rs` 指 `rust/crates/ccusage-core/src/`；`parser.rs` 指 `rust/crates/ccusage-cli-parser/src/parser.rs`；`main.rs`/`commands/mod.rs`/`blocks.rs`/`http.rs` 指 `rust/crates/ccusage/src/`。
> 路径相对仓库根；行号一律为 `read` 实测（任务书给的若干行数来自 `Measure-Object -Line` 口径，偏小：`cost.rs` 实测 664 / `adapter-all/loader.rs` 1637 / `pricing.rs` 6181）。
> 关键文件实测行数：`claude/src/lib.rs` 1582、`claude/src/daily.rs` 1182、`claude/src/paths.rs` 555、`ccusage/src/main.rs` 1063、`ccusage/src/commands/mod.rs` 1411、`adapter-all/src/loader.rs` 1637、`adapter-all/src/types.rs` 208、`core/src/cost.rs` 664、`core/src/types.rs` 219、`core/src/summary.rs` 759。

### 0. 主干坐标

```rust
match cli.command {
    Some(Command::All(args)) => adapter::all::run(args),
    Some(Command::Daily(args)) => commands::run_daily(args),
    Some(Command::Monthly(shared)) => commands::run_bucket(shared, BucketKind::Monthly),
    Some(Command::Weekly(args)) => commands::run_weekly(args),
    Some(Command::Session(args)) => commands::run_session(args),
```

证据：`rust/crates/ccusage/src/main.rs:28-33`；裸 `ccusage` 落到统一日报 `adapter::all::run`（`main.rs:53-65`）；19 个别名导出见 `rust/crates/ccusage/src/adapter/mod.rs:1-19`，统一层再导出 18 个适配器（`adapter-all/src/lib.rs:11-30`）。

### 1. 文件发现（数据根 / 环境变量 / 遍历 / 排序）

**1.1 `claude_paths()` 是唯一的数据根解析点**

```rust
if let Ok(env_paths) = env::var("CLAUDE_CONFIG_DIR") {
    for raw in env_paths.split(',').map(str::trim).filter(|path| !path.is_empty()) {
        let path = normalize_claude_config_path(raw);
        if path.join("projects").is_dir() && seen.insert(path.clone()) { paths.push(path); }
    }
    if !paths.is_empty() { return Ok(paths); }
    return Err(cli_error(format!("No valid Claude data directories found in CLAUDE_CONFIG_DIR. …
```

证据：`rust/adapters/claude/src/paths.rs:23-43`。

- **多根**：`CLAUDE_CONFIG_DIR` 按 `,` 切分、逐段 `trim`、丢空段（`paths.rs:26-31`）；去重靠 `seen`（`paths.rs:25`）。
- **优先级**：该变量一旦设置即权威——任一段有效就返回（`paths.rs:37-39`）；**全部无效直接报错**（`paths.rs:40-42`），不回落 `~/.claude`。
- **每段规范化**：末段名为 `projects` 且是目录时取父目录（`paths.rs:57-63`，`~` 由 `expand_home_path` 展开）；**必须以存在 `<root>/projects/` 为有效条件**（`paths.rs:33`）。端到端测试 `main.rs:382-396`。
- **无该变量时同时尝试两个根**（不是二选一）：

```rust
let home = home::home_dir().ok_or_else(|| cli_error("home directory is not set"))?;
let xdg = env::var("XDG_CONFIG_HOME").map(PathBuf::from)
    .unwrap_or_else(|_| PathBuf::from(&home).join(".config"));
for path in [xdg.join("claude"), home.join(".claude")] { … }
```

证据：`paths.rs:45-53`。

**1.2 文件枚举：递归、无忽略规则、按完整路径字符串排序**

证据：`rust/adapters/common/src/lib.rs:18-34`（`collect_files_with_extension`）。

- 递归无深度限制；`read_dir` 失败静默返回（`common/lib.rs:19-21`）；**无任何忽略规则**（不读 `.gitignore`、不跳隐藏目录）；只收扩展名恰为 `jsonl` 的文件（`common/lib.rs:28`）。
- 排序：`files.sort_by_cached_key(|path| path.to_string_lossy().into_owned());`（`paths.rs:77`）——**去重的输入之一**（谁先谁后决定幸存者）。测试 `main.rs:211-223`：父会话文件排在 `subagents/agent-a.jsonl` 之前。
- 项目过滤只接受单段目录名：`is_project_path_segment` 拒绝空串、`.`、`..` 与含 `/` `\` 的值（`paths.rs:248-254`）；非单段过滤值退化为全量扫描——判定与分支在 `rust/adapters/claude/src/paths.rs:69-75`（`lib.rs:972-982` 是覆盖同一行为的测试）。

**1.3 路径 → 会话/项目身份**

```rust
if relative.len() >= 4 && relative.get(relative.len() - 2) == Some(&"subagents") {
    let session_id = relative[relative.len() - 3].to_string();
    let project_path = relative[..relative.len() - 3].join(std::path::MAIN_SEPARATOR_STR);
```

证据：`paths.rs:303-319`（`extract_session_parts` 整体 `paths.rs:290-331`）。

- `projects/<p>/<session>.jsonl` → `(session, p)`（`paths.rs:303-307`）；`projects/<p>/session-a/subagents/agent-a.jsonl` → `("session-a", "p")`（测试 `lib.rs:1014-1022`）。
- **更深的 workflows 路径不命中 `subagents` 分支**：`projects/p/session-x/subagents/workflows/wf_1/agent-a.jsonl` 落到兜底（`paths.rs:320-330`）→ 会话名取 `wf_1`、项目路径 `p/session-x/subagents/workflows`；该文件归属只能靠行内 `sessionId` 纠正（测试 `paths.rs:517-538`）。
- 项目名取 `projects` 之后的第一段，缺失/空白 → `"unknown"`（`extract_project`，`paths.rs:269-288`）。

**1.4 会话身份的双来源**

```rust
fn loaded_entry_session_id(entry: &LoadedEntry) -> &str {
    entry.data.session_id.as_deref().unwrap_or(entry.session_id.as_ref())
}
```

证据：`rust/adapters/claude/src/lib.rs:405-411`——**行内 `sessionId` 优先，缺失才用路径推导值**；该函数同时被去重键与 sidechain 回放路由消费（见本章「去重规则」）。

### 2. 读取管线（路径 → `LoadedEntry`）

**2.1 调用链**

`load_entries`（`lib.rs:51-55`）→ `load_entries_inner(shared, filter, false)`（`lib.rs:107-188`）；`load_entries_since`（`lib.rs:67-74`）→ 同一内部函数，第三参 `true`。

```rust
let paths = claude_paths()?;
…
let files = usage_files(&paths, project_filter);
let files = if skip_files_before_since {
    split_files_before_since(files, shared, utc_now()).kept
} else { files };
```

证据：`lib.rs:112-130`。随后：价格表（`lib.rs:144-152`）→ 时区（`lib.rs:153`）→ `read_files_parallel`（`lib.rs:155-157`，闭包即 `read_usage_file`）→ 逐条去重（`lib.rs:170-187`，`push_deduped_entry`）。两个入口都用 `progress::track_usage_load(UsageLoadAgent("Claude"), shared.json, …)` 包裹（`lib.rs:52`、`lib.rs:71`）。

**2.2 并发模型：按文件大小贪心均分的线程组**

```rust
let worker_count = if single_thread { 1 } else {
    thread::available_parallelism().map(usize::from).unwrap_or(1).min(files.len())
};
if worker_count <= 1 { return files.iter().map(|file| read(file.as_path())).collect(); }
let chunks = chunk_file_indexes_by_size(files, worker_count);
```

证据：`rust/adapters/common/src/lib.rs:89-101`（`read_files_parallel` 整体 `84-126`）。`chunk_file_indexes_by_size`（`common/lib.rs:49-81`）按 `metadata().len()` 降序（同大小按索引升序，`59-62`）依次投给累计字节最小的块（`66-75`），滤掉空块（`77-80`），`chunk_count.max(1)` 防空（`53`）。结果按原文件顺序回填（`common/lib.rs:113-124`），保证「文件顺序 = 路径排序」在并行下仍成立（测试 `main.rs:176-209`、`common/lib.rs:133-153`）。

> daily 路径**没有复用**该函数，而是复制了一份等价实现：`daily.rs:236-283`（同样的 `available_parallelism().min(files.len())` + 分块 + `thread::scope`）。

**2.3 单文件是「整文件读入 + 字节行切片」，不是流式**

```rust
let Ok(content) = fs::read(path) else { return loaded_file; };
let usage_marker = memmem::Finder::new(br#""usage":{"#);
for line in byte_lines(&content) {
    if usage_marker.find(line).is_none() { continue; }
    let Some(data) = deserialize_usage_line::<UsageEntry>(line) else { continue; };
```

证据：`lib.rs:505-516`（`read_usage_file` 整体 `lib.rs:491-603`）。

- 无 `BufReader`、无流式：一次 `fs::read` 读完整文件；`byte_lines` 只在字节切片上找 `\n`（`core/src/fast.rs:82-103`），无每行 `String` 分配；`memmem` 预过滤在解析前逐行筛（设计说明 `fast.rs:16-70`）。
- **预过滤是精确字节匹配 `"usage":{`**（`lib.rs:509`，daily 同 `daily.rs:302`）。写成 `"usage": {`（带空格）的行会被**整行静默丢弃**。
- **不做时间戳预扫描**：`timestamp_from_line`（`paths.rs:256-267`，memmem 找 `"timestamp":"`）只被测试引用（`main.rs:249-260`），是 TS 兼容的 `#[doc(hidden)]` 导出（`lib.rs:33-34`），不在装载管线上；逐行时间戳在解析后统一处理（`lib.rs:517-519`，失败即丢行）。

**2.4 大文件优化：整文件级 mtime 剪枝 + 首行扫描，没有字节级尾部读**

`const MTIME_PRUNE_MARGIN_MS: i64 = MILLIS_PER_DAY;`（`paths.rs:84`），阈值 = `since_ms - 1 天`（`paths.rs:137`）。规则（`paths.rs:117-172`）：`since` 缺失/不可解析 → 全保留（`122-131`）；窗口起点在将来 → 全保留（`134-136`）；mtime 读不到 → 保留（`156` 的 `is_none_or`）；剪枝**按会话 ID 连通分量整组进行**——用路径会话名 + `recorded_session_id`（读文件直到第一条含 `"sessionId"` 的行，`paths.rs:210-234`，含 `memmem` 预筛 `224`）建并查集（`paths.rs:174-205`），组内任一文件新鲜则整组保留（`139-171`），注释解释了原因：子代理/工作流会回放父消息，单独丢父文件会让回放被双算（`paths.rs:98-106`）。测试：`paths.rs:387-538`（margin / 无 since / 未来窗口 / mtime 不可读 / 父子同组 / 跨配置根与跨目录 `sessionId`）。

statusline 侧另有一处「少读一点」：上下文窗口计算整读 transcript 后**从尾部反向**找最后一条 `type == "assistant"` 且带 `usage` 的行（`commands/mod.rs:719-731`、`738-751`），并用 mtime + 刷新间隔缓存结果（`commands/mod.rs:337-343`、`826-845`；缓存文件 `%TEMP%/ccusage-semaphore/<session>.lock`，`860-864`）。

**`LoadedFile.timestamp` 是死字段（已验证）**：唯一构造点 `claude/src/lib.rs:501-504`（`LoadedFile { timestamp: None, entries: Vec::new() }`），写入点 `lib.rs:660-666`；daily 侧同构（`DailyLoadedFile` 构造 `daily.rs:294-297`、写入 `daily.rs:314-318`）。消费者只读 `.entries`（`lib.rs:158-181` 的 `loaded_entry_count`/去重循环、`daily.rs:100-101`），全仓无任何读取该字段的代码——即两处写入均为死代码。

### 3. 解析与容错

**3.1 JSON 入口与 null 契约**

```rust
pub(crate) fn deserialize_usage_line<T: DeserializeOwned>(line: &[u8]) -> Option<T> {
    if memmem::find(line, b"null").is_none() { return serde_json::from_slice(line).ok(); }
    let root = serde_json::from_slice::<UniqueJsonValue>(line).ok()?.0;
    if has_unsupported_null_field_in_value(&root) { return None; }
    serde_json::from_value(root).ok()
}
```

证据：`lib.rs:727-736`。无 `null` 的行走零拷贝 `from_slice` 快路径（`728-730`）；含 `null` 的行先解析成 `UniqueJsonValue`（`lib.rs:741-830`），它**保留 Serde 对重复成员名的拒绝**（`819-824` 遇重复即报错），防 `serde_json::Value` 的「后者覆盖前者」把畸形行洗白（注释 `738-740`，测试 `lib.rs:1136-1142`）。不可为 `null` 的字段清单：`id | cwd | model | speed | costUSD | version | sessionId | requestId | isApiErrorMessage | cache_read_input_tokens | cache_creation_input_tokens`（`lib.rs:867-886`），命中即丢整行（`732-734`）；唯一豁免是 `usage.iterations[]` 元素的 `model`，范围严格限定在 `usage` 的直接 `iterations` 成员（`lib.rs:832-841`、`849-854`，测试 `lib.rs:1082-1127`）。

**3.2 坏行：静默跳过，无计数无告警**

`lib.rs:514-516`（反序列化失败 `continue`）、`lib.rs:517-519`（时间戳不可解析 `continue`）。共同库明说这是历史行为（`adapters/common/src/jsonl.rs:29-30`）。**TokenScope 的 SF08 要求 bad_lines 计数，此处必须本地补。**

**3.3 `type` / `isApiErrorMessage` / `isSidechain` 都不是过滤条件**

- `UsageEntry` **没有 `type` 字段**（`core/src/types.rs:9-19`），因此不存在 `type == assistant` 白名单；`user`/`summary`/`system` 行只会在「缺 `message`/`usage`」或字段校验处被挡掉。
- `isApiErrorMessage` 只用于识别额度耗尽时间：`lib.rs:533-534` → `usage_limit_reset_time_from_line_bytes`（`lib.rs:918-943`：必须 `Some(true)`，文本含 `Claude AI usage limit reached`，其后 `|` 跟纯数字秒级时间戳且 > 0）。该行**仍作为一条 usage entry 参与统计**，没有跳过分支。
- `isSidechain` 只影响去重优先级与回放路由：`is_sidechain_usage_entry` = `is_sidechain == Some(true)`（`lib.rs:442-444`）。

**3.4 `usage` 缺失 / 字段缺失**

```rust
pub struct TokenUsageRaw {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)] pub cache_creation_input_tokens: u64,
    #[serde(default)] pub cache_read_input_tokens: u64,
    pub speed: Option<Speed>,
    #[serde(default)] pub cache_creation: Option<CacheCreationRaw>,
}
```

证据：`core/src/types.rs:28-39`。

- 缺 `message` 或缺 `message.usage`：因两者都是必填（`types.rs:13`、`types.rs:23`）→ 整行失败（且多数已被 `"usage":{` 预过滤挡掉）。
- `usage` 存在但缺 `input_tokens`/`output_tokens` → serde 缺字段错误 → **整行丢弃**（这两个字段没有 `#[serde(default)]`，`types.rs:30-31`）；缺两个 cache 字段 → 视作 0（`types.rs:32-35`）。
- `speed` 只接受小写 `standard`/`fast`（`types.rs:36` + `types.rs:60-65` 的 `rename_all = "lowercase"`）；其它取值（如 `flex`）会让整行反序列化失败并被静默跳过。

**3.5 行级有效性校验**

`is_valid_usage_entry`（`lib.rs:668-707`）依次拒绝：`version` 存在但不是语义化版本前缀（`is_semver_prefix`，`lib.rs:888-900`）、`sessionId` 空串、`requestId` 空串、`message.id` 空串、`model` 空串；**其余一律接收，包括 total 为 0 的行**——0 token 行不在装载入口过滤，而由**下游两处**各自丢弃：
- 会话报表：四列全 0 的会话被剔除（`commands/mod.rs:155-161`，见 §10.3）。
- 统一报表（adapter-all）：`summary_rows` 丢弃 `total_tokens == 0 && total_cost == 0.0` 的行（`loader.rs:786-789`，见 §9.4）；注意是「且」——0 token 但有 cost 的行（例如只有 credits 的 Copilot 行）会被保留。
- 例外：`claude daily` 直接走 `load_daily_summaries`，不经过 adapter-all 的 `summary_rows`，所以 0 token 行会进入日汇总（贡献 0）。

### 4. 去重规则（重点）

去重是**单遍流式**：按「排序后的文件顺序 → 文件内行序」逐条 `push_deduped_entry`（`lib.rs:170-187`），索引结构 `FxHashMap<u64, SmallVec<[DedupeIndex; 1]>>`（`lib.rs:40-45`、`lib.rs:170`）；同键冲突时不新增，按规则决定是否**原地替换**。

**4.1 精确键 `usage_dedupe_hash`**

```rust
fn usage_dedupe_hash(message_id: &str, request_id: Option<&str>, session_id: &str, timestamp: TimestampMs) -> u64 {
    let mut hasher = FxHasher::default();
    message_id.hash(&mut hasher);
    request_id.hash(&mut hasher);
    if request_id.is_none() {
        session_id.hash(&mut hasher);
        if !is_anthropic_message_id(message_id) { timestamp.hash(&mut hasher); }
    }
    hasher.finish()
}
```

证据：`lib.rs:350-366`。展开：

| 情形 | 去重键 = | 语义 / 测试 |
| --- | --- | --- |
| `requestId` 存在 | `message.id` + `requestId`（**不含会话、不含时间戳**） | 同一 `requestId` 的跨会话副本合并（`lib.rs:1331-1352`） |
| 无 `requestId`，`message.id` 以 `msg_` 开头 | `message.id` + `session_id` | 同会话跨时间戳合并（Claude Code 一条回复按 content block 写多行；`lib.rs:1256-1280`） |
| 无 `requestId`，非 `msg_` 前缀 | `message.id` + `session_id` + `timestamp` | 网关复用同一 `message.id` 的每次调用各自成条（`lib.rs:1224-1254`、`1282-1306`） |

`is_anthropic_message_id` 只是 `starts_with("msg_")` 的启发式（`lib.rs:368-375`：只有 `requestId` 才证明两行属同一响应）。哈希命中后还要过结构比对（`lib.rs:413-425`）：`message.id` 相等 **且** `request_id` 相等 **且**（有 requestId 即通过；否则要求会话相等且（是 `msg_` 前缀或时间戳相等））。

**4.2 重复行保留哪一条（"most complete"）**

```rust
fn should_replace_deduped_entry(candidate: &UsageEntry, existing: &UsageEntry) -> bool {
    let (c, e) = (is_sidechain_usage_entry(candidate), is_sidechain_usage_entry(existing));
    if c != e { return e; }
    let (ct, et) = (usage_token_total(candidate), usage_token_total(existing));
    if ct != et { return ct > et; }
    candidate.message.usage.speed.is_some() && existing.message.usage.speed.is_none()
}
```

证据：`lib.rs:198-212`（本文合并了原行的 `let` 绑定）。`usage_token_total` = `input + output + cache_creation_token_count() + cache_read`（`lib.rs:190-196`）。优先级：① 非 sidechain 胜 sidechain；② token 总数大者胜；③ 平手时「带 `speed`」者胜。**主装载器不比较 cost**（与 daily 不同，见 §4.6）。用户可见效果：同一响应被写两遍时保留完整那次（测试 `main.rs:262-283` 断言留下 `output_tokens: 250` 与 `costUSD 0.01`）。

**4.3 不同 session 复用同一 `message.id` 为什么不合并**

因缺 `requestId` 时键里含 `session_id`（`lib.rs:359-363`），每个会话各自成条。测试 `main.rs:286-309`（两会话、同 id、同时间戳 → 2 条）、`lib.rs:1195-1222`（网关 id `ocgo` 两会话 → 2 条，input 合计 400）。反之，若两文件**行内 `sessionId` 相同**（转写副本）则合并为 1 条（`lib.rs:1308-1329`）——这正是「会话身份取行内、路径兜底」的直接后果（`lib.rs:405-411`）。

**4.4 sidechain 回放：两条额外路由**

```rust
fn sidechain_replay_dedupe_hash(message_id: &str, session_id: &str) -> u64 { … "sidechain-replay" … }
fn sidechain_entry_replay_dedupe_hash(message_id: &str, session_id: &str) -> u64 { … "sidechain-replay-entry" … }
```

证据：`lib.rs:377-392`（两个域分隔字符串）。查找逻辑 `lib.rs:237-266`：候选是 sidechain 走 `sidechain-replay` 桶，否则走**只装 sidechain 条目**的 `sidechain-entry` 桶（注释 `238-241` 解释这是为了不让「网关给每个响应复用一个 message ID」触发全表回扫）。匹配条件（`lib.rs:427-440`）：`message.id` 相等 **且**（双方都无 requestId → 可跨时间戳；否则要求时间戳相等）**且** 至少一边是 sidechain——**daily 侧缺「时间戳相等」这一守卫，见 §4.6 第 4 条**。配套别名维护：跨会话副本可能成为幸存者，需把两个会话的路由都登记到同一条目（`lib.rs:270-286`；`push_deduped_session_alias` 在 `474-489`），替换幸存者时只登记发生变化的键（`287-316`，理由见注释 `289-290`）。测试矩阵：`lib.rs:1354-1396`、`1398-1441`、`1443-1491`、`1493-1535`。

**4.5 子代理文件（`subagents/agent-*.jsonl`）如何避免双算**

机制不是文件名特判，而是「路径身份 + 两条键」共同生效：路径层面子代理文件与父转写同会话（`paths.rs:308-312`），行内层面通常也带父会话 `sessionId`（`lib.rs:405-411`）；于是同 `message.id` + 同 `requestId` 的回放命中精确键（键不含会话，直接合并），无 requestId 的同 `message.id` 回放在同会话内合并。
端到端证据：`rust/crates/ccusage/tests/claude_cli.rs:632-661`——父文件 `msg-parent`（input 10 / cache_read 20）+ `subagents/agent-a.jsonl` 同 id、`isSidechain: true`、cache_read 50000，四个报表总计都必须等于 32（sidechain 那份被合并）。另一形态见 `main.rs:537-567`（父文件里 `type: progress` + `data.message.message` 包装的同一条消息与 subagents 副本合并，模型顺序 `["claude-opus-4-6-fast","claude-haiku-4-5-20251001","claude-opus-4-6"]`），以及跨目录副本 `claude_cli.rs:529`、`paths.rs:517-538`。

**4.6 主装载 vs daily 快捷路径的去重差异（重要）**

| 维度 | `read_usage_file` + `push_deduped_entry` | `read_daily_usage_file` + `push_deduped_daily_entry` |
| --- | --- | --- |
| 键函数 | `usage_dedupe_hash`（`lib.rs:350-366`） | **同一个**（`daily.rs:457-462`、`571-580`） |
| 幸存者选择 | sidechain → token 总数 → speed（`lib.rs:198-212`） | 同前，**但 token 总数平手时再比 cost 大者胜**（`daily.rs:597-599`） |
| 行形态 | 只认顶层 `message.usage`（`UsageEntry`） | 顶层 **或** `data.message.message.usage`（`DailyUsageLine` 无标签联合，`daily.rs:182-204`） |
| `extra_total_tokens` | 聚合时累加（`core/src/summary.rs:54-57`） | 聚合不累加、汇总恒 0（`daily.rs:617-644`、`daily.rs:660`） |
| sidechain 回放路由 | 路由匹配**含 timestamp 守卫**：只有双方都无 requestId 才允许跨时间戳，否则要求 `entry.timestamp == timestamp`（`lib.rs:436-440`） | **缺 timestamp 守卫**，只比对 `message.id` + 会话 + 「至少一边 sidechain」（`daily.rs:493-496`） |

第 4 条是**是否合并**的差异，比上述三条平局裁决差异更实质：对「父条目 + 带**新** requestId、时间戳已被改写」的回放副本，daily 会合并、主装载路径不会。daily 侧有一条测试**显式固化**了这种行为（`daily.rs:731-769`：同 `msg-parent`、新 requestId、时间戳 1_774_000_000_000 vs …001_000 → 合并为 1 条）；主装载侧的对应测试（`lib.rs:1443-1491`）所有条目共用同一时间戳，因此**没有**覆盖反例。

结论：等价只**在测试覆盖的样本形态下**成立，不是结构保证（差异见本章第 8 小节）。

### 5. advisor / iterations / speed

**5.1 `usage.iterations` 里的 advisor 被拆成独立 entry**

```rust
pub(crate) fn advisor_usages_from_line(line: &[u8]) -> Vec<AdvisorUsage> {
    if memmem::find(line, br#""advisor_message""#).is_none() { return Vec::new(); }
    let Ok(envelope) = serde_json::from_slice::<UsageIterationsEnvelope>(line) else { … };
    envelope.message.usage.iterations.into_iter().filter_map(|iteration| {
        (iteration.kind == "advisor_message").then_some(iteration.model).flatten()
            .filter(|model| !model.is_empty())
            .map(|model| AdvisorUsage { model, usage: iteration.usage })
    }).collect()
}
```

证据：`lib.rs:635-658`（信封与元素类型 `lib.rs:605-628`，`UsageIteration` 用 `#[serde(flatten)] usage: TokenUsageRaw`，`lib.rs:621-628`）。要点：先 `memmem` 字节预筛再反序列化（`636-638`）；**只有 `type == "advisor_message"` 被拆条**（`type == "message"` 的迭代不拆，父行 `usage` 已含）；`model` 缺失或为空则跳过该迭代（`649-651`）。拆条实现（`lib.rs:559-600`）：

```rust
let mut advisor_data = entry.data.clone();
advisor_data.message.id = advisor_data.message.id.map(|id| format!("{id}:advisor:{index}"));
advisor_data.message.model = Some(advisor.model.clone());
advisor_data.message.usage = advisor.usage;
advisor_data.cost_usd = None;
```

- `:advisor:{index}` 后缀让 advisor 条目天然不撞父条目去重键（`lib.rs:561-565`）。
- `cost_usd = None`（`568`）→ advisor **永远自算价**（`lib.rs:583-590`），于是 `--mode display` 下 advisor 计价为 `cost_usd.unwrap_or(0.0) = 0.0`（`core/src/cost.rs:40`）——**display 模式会漏掉 advisor 花费**。
- `missing_pricing_model` 独立计算（`569-575`），`usage_limit_reset_time` 继承父条目（`595`）。
- 测试：`main.rs:399-421`（1 行 → 2 条，advisor 保留自己的 159419/7805）、`lib.rs:1163-1193`（advisor 用 advisor 模型计价 26.0，父条目保留 `costUSD 1.23`）、daily 侧 `main.rs:423-447`（日汇总 input 159421 / output 8296）。
- daily 路径同构：`daily.rs:368-398`（id 后缀在 `daily.rs:392-394`）。

**5.2 `speed: fast`**

```rust
let model = data.message.model.as_ref().and_then(|model| {
    if model == "<synthetic>" { None }
    else if matches!(data.message.usage.speed, Some(Speed::Fast)) {
        Some(suffix_string(model, "-fast"))
    } else { Some(model.clone()) }
});
```

证据：`lib.rs:535-543`（daily 镜像 `daily.rs:338-346`；`suffix_string` = 拼接，`core/src/fast.rs:105-110`）。

- `<synthetic>` 归一为 `None`（不显示、不进模型分组）。
- **展示/分组名加 `-fast`，计价不靠后缀**：

```rust
let multiplier = if matches!(usage.speed, Some(Speed::Fast)) { pricing.fast_multiplier } else { 1.0 };
calculate_cost_from_pricing(usage, pricing) * multiplier
```

证据：`core/src/cost.rs:110-115`（`fast_multiplier` 也可被外置覆盖，`ccusage-cli/src/types.rs:295`）。坑：缺价判定用**原始** `data.message.model`（`lib.rs:526-532`），而模型分组消费带后缀的 `entry.model`（`core/src/summary.rs:65-66`），因此同一物理模型会以 `x-fast` 与 `x` 两个名字出现在报表里；`resolve_model_name` 的 `-fast` 分支可用于对齐（`core/src/model_aliases.rs:14-26`）。

### 6. cache token 语义

**6.1 取值优先级：`cache_creation` 对象 > 扁平字段**

```rust
impl TokenUsageRaw {
    pub fn cache_creation_token_count(&self) -> u64 {
        if let Some(b) = &self.cache_creation {
            b.ephemeral_5m_input_tokens.saturating_add(b.ephemeral_1h_input_tokens)
        } else { self.cache_creation_input_tokens }
    }
}
```

证据：`core/src/types.rs:41-50`（`CacheCreationRaw` 在 `types.rs:52-58`，两字段均 `#[serde(default)]`）。所有聚合出口都用它：`TokenCounts::add_usage`（`types.rs:78-87`）、去重用的总数（`lib.rs:190-196`）、daily（`daily.rs:443-448`）。

**6.2 计价里的 5m/1h 分叉与 1h 加倍**

```rust
let (cache_create_5m_tokens, cache_create_1h_tokens) = if let Some(breakdown) = usage.cache_creation {
    (breakdown.ephemeral_5m_input_tokens, breakdown.ephemeral_1h_input_tokens)
} else { (usage.cache_creation_input_tokens, 0) };
let cache_create_1h_cost = pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER;
```

证据：`core/src/cost.rs:118-131`，`CACHE_CREATE_1H_INPUT_MULTIPLIER = 2.0`（`cost.rs:7`）。没有 `cache_creation` 对象时，扁平字段全按 5m 桶（`cache_create` 单价）计，1h 桶为 0。

**6.3 `input_tokens` 是否含 cache —— 不含，四桶相加**

- 桶相加的权威实现：`TokenCounts::add_usage`/`total`（`core/src/types.rs:78-95`）与 `total_usage_tokens`（`core/src/utils.rs:14`）都把 `input + output + cache_creation + cache_read` 当**独立桶**。
- 直接文字依据：`core/src/cost.rs:139-142` 注释「`input_tokens` here is the uncached remainder - adapters normalize usage into the Claude shape」。
- 反向印证：其它适配器的原始字段含 cache，需显式扣减（Copilot：`docs/guide/copilot/index.md:31`；Codex：本章 §9.2）。Claude 侧 `input_tokens` 直接落列，不做扣减（`lib.rs:524-525`、`544-558`）。

### 7. 成本取值

**7.1 `--mode` 默认 `auto`**

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CostMode { #[default] Auto, Calculate, Display }
```

证据：`rust/crates/ccusage-cli/src/types.rs:242-248`；`SharedArgs::with_defaults()` 再次显式 `mode: CostMode::Auto`（`types.rs:62-71`）；CLI 只接受 `auto|calculate|display`（`ccusage-cli-parser/src/parser.rs:1134-1141`，赋值 `parser.rs:753`）。

**7.2 三分支本体**

```rust
match mode {
    CostMode::Display => cost_usd.unwrap_or(0.0),
    CostMode::Auto => cost_usd.unwrap_or_else(|| calculate_cost_from_tokens(model, usage, timestamp, pricing)),
    CostMode::Calculate => calculate_cost_from_tokens(model, usage, timestamp, pricing),
}
```

证据：`core/src/cost.rs:39-46`（`calculate_cost_for_usage_at`，`cost.rs:31-46`；`calculate_cost` 只是拆包转发 + `parse_ts_timestamp`，`cost.rs:9-18`）。

- **display**：只信 `costUSD`，无值即 0，从不查价、无兜底；且装载层在 display 下**根本不加载价格表**（`lib.rs:144-152`，daily 镜像 `daily.rs:45-53`）。
- **auto**：`costUSD` 有值一律采用，缺席才按 token 自算。
- **calculate**：永远自算，忽略 `costUSD`。缺价标记 `missing_pricing_model_for_usage`（`cost.rs:48-59`）在 display 或「auto 且有 costUSD」时返回 `None`，否则查价失败返回别名归一后的模型名（`cost.rs:61-75`）。

**7.3 Claude 链路调用点**

| 路径 | 调用 | 行号 |
| --- | --- | --- |
| 主装载主行 | `calculate_cost(&data, mode, pricing)` | `lib.rs:525` |
| 主装载 advisor | `calculate_cost_for_usage_at(Some(model), usage, None, Some(ts), …)` | `lib.rs:583-590` |
| daily 主行 | `calculate_cost_for_usage_at(model, usage, cost_usd, Some(ts), …)` | `daily.rs:323-330` |
| daily advisor | 同上，`cost_usd = None` | `daily.rs:382-389` |
| statusline 会话花费 | `load_entries` + 按会话过滤 + `sum(entry.cost)` | `commands/mod.rs:673-682` |

时间戳进入计价只为按事件时间选价（`cost.rs:102-106`，`find_at` / `find`）。

**7.4 相关开关**：`--offline` 只影响价格表是否联网刷新（`core/src/pricing.rs:1002-1038`：先 `load_embedded()`，`!offline` 才 `fetch_pricing_json`，失败仅告警回退内嵌；`enable_models_dev_fallback = !offline`）；`--no-cost` **不参与装载**，只在输出层剥离 cost（`core/src/output.rs:158-161` 调 `strip_cost_json`（`output.rs:564`），表格分支 `249/304/369/512`）；统一装载层自己再装一份价格表（`adapter-all/src/loader.rs:83-89`）。

### 8. 日汇总快捷路径

**8.1 确有独立实现**

`load_daily_summaries`（`lib.rs:78-84`）→ `load_daily_summaries_with_detection`（`lib.rs:97-105`）→ `daily::load_daily_summaries_inner`（`daily.rs:30-76`）。它**不调用** `load_entries`，而是自建「读文件 → 行级去重 → 按天分桶累加」链路：`daily.rs:35-62`（`claude_paths` → `usage_files` → `split_files_before_since` → `summarize_daily_files`）。累加器 `DailyAccumulator`（`daily.rs:607-670`）、按天 `BTreeMap` 分组（`daily.rs:131-146`）、`group_by_project` 时按 `(date, project)` 分组（`daily.rs:112-129`）。它额外返回 `detected`：即使文件全被 `--since` 剪掉，只要被剪掉的文件里确有 entry 也算「检测到 Claude 用量」（`daily.rs:63-75`）。

**8.2 两个实现共用的部件**

共用：`claude_paths`/`usage_files`/`split_files_before_since`（导入 `daily.rs:22-28`，调用 `daily.rs:35-37`）、`deserialize_usage_line`（`daily.rs:307` vs `lib.rs:514`）、`advisor_usages_from_line`（`daily.rs:368` vs `lib.rs:560`）、`usage_dedupe_hash`/`is_anthropic_message_id`/两条 sidechain 哈希（`daily.rs:457-462`、`472`、`482-484`）、`extract_project`/`extract_session_parts`（`daily.rs:291-292`）、去重索引助手（`daily.rs:22-28`）。
未共用（各一份副本）：`is_valid_usage_entry`（`daily.rs:403-442`）、`should_replace_*` 与 `*_usage_token_total`（`lib.rs:190-212` vs `daily.rs:443-448`、`582-601`）、聚合累加器（`core/src/summary.rs:39-123` vs `daily.rs:607-670`）。

**8.3 源码里有等价性测试（3 个单元 + 1 组 CLI 级）**

```rust
let entries = load_entries(&shared, None).unwrap();
let daily = load_daily_summaries(&shared, None, false).unwrap();
let expected_daily = summarize_by_key(&entries, |entry| entry.date.clone(), |key| (key.to_string(), None)).unwrap();
assert_eq!(daily.iter().map(summary_json).collect::<Vec<_>>(),
           expected_daily.iter().map(summary_json).collect::<Vec<_>>());
```

证据：`main.rs:465-478`（`daily_summaries_match_entry_aggregation_for_requestless_rewrites`）。同型断言：`main.rs:483-535`（普通 + `group_by_project` 分组日汇总，`503`、`516-534`）、`main.rs:570-595`（progress 包装行与 subagents 副本并存仍一致）。CLI 级：`tests/claude_cli.rs:603-629` 的 `claude_report_totals` 对 daily/weekly/monthly/session 四报表取 `totals.totalTokens` 比较，被 `claude_cli.rs:632/664/688/718` 复用。

**8.4 等价性的边界（TokenScope 必读）**

- 结构差异 1：daily 能解析 `type: progress` 包装行（`DailyUsageLine::AgentProgress`，`daily.rs:182-204`），主装载器只认顶层 `message`（`UsageEntry`，`core/src/types.rs:9-19`）。在「progress 行有 subagents 文件副本」的形态下两者收敛（`main.rs:570-595` 断言相等）；**若某条 progress 行没有任何副本，两条路径会分叉（按代码推断，未实测）**。
- 结构差异 2：daily 在 token 总数平手时用 cost 决胜（`daily.rs:597-599`），主装载器不比 cost（`lib.rs:198-212`）。
- 结构差异 3：daily 聚合不累加 `extra_total_tokens`（`daily.rs:660`），主路径累加（`core/src/summary.rs:54-57`、`113`）；Claude 侧该字段恒为 0（`lib.rs:552`、`591`），当前无影响。
- **结构差异 4（改变是否合并，最实质）**：daily 的 sidechain 回放路由**缺 timestamp 守卫**——`daily.rs:493-496` 只比对 `message.id` + 会话 + 「至少一边是 sidechain」，而主路径 `lib.rs:436-440` 还要求「双方都无 requestId 或时间戳相等」。`lib.rs:434-435` 的注释本身就承认了这个差异（「Requestless replays match across timestamps, as in the daily loader. Replays that carry a new request ID still need the parent's timestamp **in this loader**」）。后果：同一样本经 `load_daily_summaries` 与 `load_entries + summarize_by_key` 会得到不同总量（daily 合并、主路径不合并）；daily 有测试固化该行为（`daily.rs:731-769`），主路径侧无覆盖反例（`lib.rs:1443-1491` 共用同一时间戳）。
- 结论：等价只在**测试覆盖的样本形态下**成立，不是结构保证；改任一侧的去重/解析都必须重跑 `main.rs:449-595` 与 `tests/claude_cli.rs:603-741`，并针对「新 requestId + 改写时间戳的 sidechain 回放」补一条跨实现对照用例。

### 9. 统一报告（adapter-all）

**9.1 顶层装载器与并发**

`load_rows`（`loader.rs:29-37`）→ `load_pricing`（`83-89`）→ `load_kind_for_report`（`91-98`，weekly/monthly 归并为按日装载）→ `load_base_rows`（`100-396`）→ `finish_rows`（`398-420`）；`load_sections`（`39-81`）为多段报表复用同一批 base rows。18 个 agent 各一条 `AgentLoadSpec`（装配 `116-358`，agent 名取 `BUILT_IN_AGENT_NAMES[i]`，`119`），随后追加用户自定义 `pi.stores`（`359-380`）。汇总标识表 18 项：

```rust
pub const BUILT_IN_AGENT_NAMES: &[&str] = &[
    "claude", "codex", "opencode", "amp", "droid", "codebuff", "hermes", "pi", "goose",
    "openclaw", "kilo", "copilot", "gemini", "antigravity", "kimi", "qwen", "grok", "zcode",
];
```

证据：`core/src/lib.rs:63-82`（本文压缩换行）。**并行模型：一个 agent 一个 OS 线程**（`thread::scope` + `mpsc`），全仓无 rayon 依赖：

```rust
thread::scope(|scope| {
    let (sender, receiver) = mpsc::channel();
    for spec in specs {
        let sender = sender.clone();
        handles.push((spec.index, spec.progress_agent, scope.spawn(move || {
            let result = (spec.load)();
            let _ = sender.send((spec.index, spec.agent, spec.progress_agent, result));
        })));
    }
```

证据：`loader.rs:430-443`。错误按 `index` 排序后**只抛第一个**（`472-475`），成功结果按 `index` 回填保证顺序稳定（`477`），线程 panic 也转成错误（`465-470`）。子装载器收到 `json: true` 的克隆以抑制各自进度输出（`loader.rs:112-115`），进度条由外层统一驱动（`105-111`、`426-428`、`450-465`）。

**9.2 token 列对齐：统一列就是 `AllRow`，只有 Codex 做扣减**

统一计量列：`input_tokens / output_tokens / cache_creation_tokens / cache_read_tokens / total_tokens`（`adapter-all/src/types.rs:12-16`）。多数 agent 只透传（`loader.rs:795-799`；`total_tokens` 取自 `summary.total_tokens()`，`loader.rs:785`，定义含 `extra_total_tokens`，`core/src/types.rs:183-189`）。Codex 是唯一「从 input 扣缓存」的：

```rust
let input = codex::non_cached_input_tokens(
    usage.input_tokens, usage.cached_input_tokens, usage.cache_creation_tokens,
);
… cache_creation_tokens: usage.cache_creation_tokens,
  cache_read_tokens: usage.cached_input_tokens,
```

证据：`loader.rs:845-855`（模型级）、`loader.rs:868-875`（组级）；扣减语义 `adapters/codex/src/report.rs:128`：`input_tokens.saturating_sub(cached_input_tokens.saturating_add(cache_creation_tokens))`。即 **`cached_input_tokens → cache_read_tokens` 列，且 `cache_read + cache_creation` 一并从 `input_tokens` 扣除**。Reasoning token 不在列里，只进元数据：`loader.rs:878-881`（`"reasoningOutputTokens"`），`extra_total_tokens: 0`（`856`）。其它 agent 的 extra 由各自适配器填入（`hermes/src/parser.rs:171`、`droid/src/loader.rs:83`、`goose/src/parser.rs:92`、`kimi/src/parser.rs:362`、`copilot/src/loader.rs:513`、`codebuff/src/loader.rs:83`、`openclaw/src/parser.rs:405`），合并用饱和加（`types.rs:174`、`types.rs:202`）。

**9.3 agent 名与 project 字段**

- 行级 agent 名：`loader.rs:793`、`loader.rs:866`（`"codex"`）、`types.rs:118`（跨 agent 聚合行 `"all"`）、动态 pi store 名用 `Box::leak` 造 `&'static str`（`494-496`，调用 `361`）。展示名映射在 report 层（`"pi" => "pi-agent"` 等，`report.rs:597-620`），JSON 输出标识本身（`report.rs:162`）。
- **`[pi] gpt-5.4` 这类前缀既不在 loader 也不在 report，而在适配器 parser**：

```rust
let model = raw_model.as_ref().map(|model| format!("[{}] {model}", context.store_name()));
```

证据：`adapters/pi/src/parser.rs:354-356`（默认 store 名 `"pi"`，`adapters/pi/src/loader.rs:74-78`）；另一例 `adapters/openclaw/src/parser.rs:317` `format!("[openclaw] {model}")`。全仓 `format!("[` 仅这两处 + CLI 帮助文本（`ccusage-cli-parser/src/help_codegen.rs:235/253`）；report 层不剥离该前缀（模型名只经 `short_model_name` 处理 `claude-` 前缀，`core/src/project_names.rs:125`）。
- project：`AllRow` **没有 project 列**，只在元数据里带 `projectPath`，且**仅 pi 系**允许带（`include_project_path=true`，`loader.rs:589`、`628`；其它 agent 传 `false`）：`if include_project_path && let Some(project_path) = summary.project_path.as_ref() { metadata.insert("projectPath".to_string(), json!(project_path)); }`（`loader.rs:819-821`）。缺失不写占位串。
- 会话身份归组用 `(project_path, session_id)`（`loader.rs:735-741`）；Claude 会话报表走 `load_entries` 而非 daily：`loader.rs:633-643`。

**9.4 合并与二次聚合**

各 agent 行先拼接（`append_agent_rows`，`loader.rs:482-492`，同时收集 `detected`），再在 loader 层按报表粒度二次聚合：daily/weekly/monthly → `aggregate_rows`（`loader.rs:417`）；session 只排序不聚合（`399-414`）。`aggregate_rows`（`888-908`）按 period 分桶（weekly 周一起算 `897-898`，monthly 取前 7 字符 `893-896`），`AllAccumulator` 累加五列 + cost + 模型集合并按 agent 保留 `agent_breakdowns`（`types.rs:78-130`），聚合行 `agent: "all"`（`types.rs:116-129`）。零值行被丢弃：`total_tokens == 0 && total_cost == 0.0`（`loader.rs:786-789`，注释：只有 cost 的 Copilot credits 行仍要计费）。

**9.5 各开关在装载层的影响点**

| 开关 | 影响点 |
| --- | --- |
| `--since/--until` | 条目级 `filter_loaded_entries_by_date`（`adapters/common/src/lib.rs:36-47`；调用 `adapter-all/src/loader.rs:564/582/585/625/726`）；Claude 日汇总级按 `date` 字符串比较（`adapter-all/src/loader.rs:656-665`）；会话级按 `last_activity`（`adapter-all/src/loader.rs:756-768`）；Codex 无窗口时走 `load_groups` 快路径（`adapter-all/src/loader.rs:672-683`） |
| `--json` | 只影响进度抑制与输出分流：`loader.rs:108-115` 强制子装载 `json: true`；`wants_json = json ‖ jq.is_some()`（`core/src/output.rs:14-16`），分流在 `adapter-all/src/lib.rs:45/65` |
| `--no-cost` | 装载层零引用，只删输出（`output.rs:158-159` 入参、`:249/304/369/512` 调用点，剥离函数 `output.rs:564-581`） |
| `--offline` | 唯一入口 `loader.rs:83-89` 的 `PricingMap::load_with_overrides(shared.offline, …)` |
| `--single-thread` | 装载层不读，经 `SharedArgs` 透传到文件级并行（`adapters/common/src/lib.rs:89-96`） |
| `project_filter` | 统一装载层**不传**（`loader.rs:635`/`648` 传 `None`）；`SharedArgs` 无 project 字段（`ccusage-cli/src/types.rs:33-60`） |

### 10. 命令层

**10.1 `run_daily`：走日汇总快捷路径**

```rust
let mut rows = load_daily_summaries(&shared, args.project.as_deref(), args.instances || args.project.is_some())?;
filter_and_sort_summaries(&mut rows, &shared, |row| row.date.as_deref().unwrap_or_default());
```

证据：`commands/mod.rs:33-42`；JSON 分流 `44-57`；`--instances/--project` 会打开按项目分组（`38`）。

**10.2 `run_bucket` / `run_weekly`：entries → 按天聚合 → 再分桶**

```rust
let entries = load_entries_since(&shared, None)?;
let mut daily = summarize_by_key(&entries, |entry| entry.date.clone(), |key| (key.to_string(), None))?;
filter_and_sort_summaries(&mut daily, &shared, …);
let mut buckets = summarize_summaries_by_bucket(&daily, kind, WeekDay::Sunday);
```

证据：`commands/mod.rs:72-87`（weekly 在 `110-124`，差异是 `args.start_of_week`）；分桶实现 `core/src/summary.rs:178-206`，跨桶累加 `208-289`。

**10.3 `run_session`**

- 列表（`commands/mod.rs:146-181`）：`load_session_rows`（`183-206`）内部 `load_entries_since` + `filter_loaded_entries_by_date`，按 `(project_path, session_id)` 归入 `SessionAccumulator`；随后丢掉四列全 0 的会话并按 `total_cost` 排序：`rows.retain(|row| row.input_tokens + row.output_tokens + row.cache_creation_tokens + row.cache_read_tokens > 0)`（`155-161`）。
- 单会话（`208-260`）：按 `entry.data.session_id` 或路径会话名匹配，按时间戳排序（`211-217`）。**JSON 明细里的 cache creation 用扁平字段**（`243` `entry.data.message.usage.cache_creation_input_tokens`），与聚合口径 `cache_creation_token_count()` 可能不一致（有 `cache_creation` 对象时会写成 0）。

**10.4 `run_blocks` / `run_statusline`**

- `run_blocks`（`262-307`）用**无界** `load_entries`（`267`），因为 5 小时计费块要向前链接（`blocks.rs:61` 起按时间排序切块）；`--recent` 只保留最近 3 天或活动块（`272-277`，`DEFAULT_RECENT_DAYS = 3`，`core/src/lib.rs:60`）。
- `run_statusline`（`310-376`）：从 stdin 读 hook JSON（`318-325`）；成本来源四态 `cc/ccusage/auto/both`（`420-435`）；`statusline_shared` 默认 `offline = args.offline && !args.no_offline`（`378-384`，`StatuslineArgs::default` 里 `offline: true`，`ccusage-cli/src/types.rs:221-240`）。
- 活动块装载用渐进窗口避免全量读：依次 1/7/30 天（`557-559` 常量 `STATUSLINE_BLOCK_LOOKBACK_DAYS`），达不到锚点就放宽，最后回退全量 `load_entries`（`568-597`）；「今日」窗口由 `statusline_today_shared` 生成（`541-555`：`since = until = 本地今天的 YYYYMMDD`）。
- 结果缓存以「未过期 + transcript mtime 未变」为命中条件，正在更新时保留旧输出（`826-845`）。

**10.5 日期过滤、排序、分页**

- 日期比较是**字符串比较**：`date_within_range` 把 `YYYY-MM-DD` 去掉 `-` 后与 `YYYYMMDD` 边界做字典序比较，两端闭区间（`core/src/date_utils.rs:196-203`）；因此 `2026`、`202609` 这类部分边界**可工作**（注释 `190-195`）。窗到瞬间的换算 `date_range_bounds_ms` 返回 `[since 00:00, until 次日 00:00)`（`205-225`），只用于 mtime 剪枝与 statusline。
- 排序：`filter_and_sort_summaries`（`core/src/summary.rs:291-305`）先过滤再 `sort_summaries`（`307-315`，按日期键）；统一报表会话形态按 cost（`loader.rs:403-413`，显式 `--order asc` 才反转）。
- `--last N` 在解析阶段折算成 `--since`：`rust/crates/ccusage/src/cli/last_window.rs:11-24`（按报表自身周期：daily=今天、weekly/monthly 对应周/月），各 agent 报表同规则（`26-74`），周起点固定周一（`60-62`）。
- **没有分页/limit/offset 参数**：`ccusage-cli-parser/src/parser.rs` 中检索 `--limit`/`--page`/`--offset` 无命中（唯一带 limit 的是 blocks 的 token 预算列，`commands/mod.rs:283-306`）。报表是「一次装完 → 排序 → 输出」。

### 11. 给 TokenScope 的可复用清单（含坑）

1. **多根 + 显式根优先**：`CLAUDE_CONFIG_DIR` 逗号分隔，设了就必须有效否则硬失败——TokenScope 若要更友好，应显式声明「无效环境变量 → 告警 + 可选回退」的策略差异。
2. **路径排序是结果的一部分**：`usage_files` 的 `sort_by_cached_key(path_string)` 决定去重「谁先谁后」，跨实现对比需复刻或改用与顺序无关的合并规则。
3. **身份优先行内、路径兜底**：`subagents/agent-*.jsonl` 不双算靠的是「同会话 + 同 requestId/messageId」的键而非文件特判；`subagents/workflows/<wf>/agent-*.jsonl` 的路径兜底会话名是工作流目录名，只有行内 `sessionId` 能纠正。
4. **紧凑 JSON 是硬约束**：`"usage":{`/`"timestamp":"` 都要求无空格；TokenScope 用 serde 解析整行更健壮，但「同一样本两工具结果不同」是预期差异，需在文档里披露。
5. **`requestId` 缺失是常态**：必须准备三档键（§4.1），否则要么重复计费（网关复用 id），要么漏计（一条回复多行）。
6. **sidechain 是优先级输入，不是过滤条件**。
7. **advisor 迭代要拆条并自算价**：拆条后 `cost_usd = None`，display 模式会算成 0——三种计价口径都要显式处理，否则静默少算。
8. **cache 口径**：`cache_creation` 对象优先于扁平字段；`input_tokens` 是未缓存余量，四桶相加才是 total；Codex 必须 `input - cached - cache_write`。
9. **日汇总有独立实现**：若 TokenScope 只保留一条主路径，需要把四条差异点（progress 行、cost 决胜、extra token、sidechain 时间戳守卫）写成显式不变量并补测试——第 4 条会让两实现给出**不同总量**，不能只靠现有等价性测试背书。
10. **`--offline`/`--no-cost`/`--json` 都不改变用量口径**（分别是价格刷新、输出剥离、进度与格式）。

### 未验证/存疑

> **本节状态（第 8 轮）**：§13.5 的实测覆盖的是模型名匹配与运行时契约，与本节多数条目无关；
> 本节各条仍是「静态阅读下的未决项」，其中「progress 包装行两路径是否分叉」「`--single-thread`
> 对统一报表并行无效」等仍需构造样本或运行验证。

- **progress 包装行的双路径分叉**：`read_usage_file` 用严格 `UsageEntry`（`lib.rs:514`，`message` 必填 `core/src/types.rs:13/23`），daily 用无标签联合接受 `data.message.message`（`daily.rs:182-204`）。「progress 行有 subagents 副本」时两者收敛（`main.rs:570-595` 断言相等），但**没有**测试覆盖「progress 行无任何副本」的情形——按代码推断两条路径 token 总量会不同，未实测。
- **`Speed` 枚举严格性**：`speed` 只接受小写 `standard`/`fast`（`core/src/types.rs:36/60-65`），其它取值会让整行被静默丢弃；仓库内无该取值 fixture，属推断。
- **6 位微秒时间戳**：`parse_ts_timestamp` 只接受整秒（长度 20/25）或 3 位毫秒（24/29）（`core/src/date_utils.rs:113-117`），其它长度直接 `None` → 整行丢弃；无相应 fixture，未实测。
- **daily 与 entries 的 cost 决胜差异**（`daily.rs:597-599` vs `lib.rs:198-212`）在真实数据上是否会分叉：需要「token 总数相同、cost 不同、且无其它决胜维度」的样本，现有测试只覆盖「subagent 副本带 costUSD」一种（`main.rs:570-595`）。
- **`--single-thread` 对统一报表的 agent 级并行无效**：`load_agent_rows_parallel` 无该分支（`loader.rs:422-480`），属代码阅读结论，未运行验证。
- **Codex `group.total_tokens` 与四列之和是否恒等**：`loader.rs:876` 直接透传，未深挖 `rust/adapters/codex` 内部是否已含 reasoning（不影响 Claude 主线）。
- **行数口径**：任务书给出的若干行数（`cost.rs` 618、`pricing.rs` 5759、`adapter-all/loader.rs` 1532）与实测（664、6181、1637）不符；本文一律采用实测行号。

## 5. Codex 适配器


- 仓库：`ccusage`（只读样本）HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31
- 证据约定：所有 `path:line` 相对 ccusage 仓库根（`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`），行号取实际文件行号（`read` 工具核对；`Measure-Object -Line` 会少算末行，不采用）
- 方法：静态阅读（`read` / `grep` / `glob`）为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5），未修改任何样本
- 覆盖：`rust/adapters/codex/src/{loader,parser,aggregate,types,replay,report,speed,paths,lib,compaction_tests}.rs`、`rust/adapters/codex/{README.md,src/README.md}`、`rust/adapters/common/src/lib.rs`、`rust/crates/ccusage-core/src/{pricing,model_aliases}.rs`、`rust/crates/ccusage-adapter-all/src/{loader,report}.rs`、`rust/crates/ccusage-cli*/src/*`、`docs/guide/codex/index.md`
- **本章裸文件名约定**：未写全路径的 `parser.rs`/`loader.rs`/`aggregate.rs`/`types.rs`/`replay.rs`/`report.rs`/`speed.rs`/`paths.rs`/`lib.rs` 指 `rust/adapters/codex/src/` 下同名文件；`cost.rs`/`utils.rs` 指 `rust/crates/ccusage-core/src/`；`lib.rs`（统一报表语境）指 `rust/crates/ccusage-adapter-all/src/`。

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

> **本节状态（第 8 轮实测后）**：§13.5 已实测 `codex daily --json` 的字段与 `--speed` 三态（`auto` 与 `standard` 数值不同、`fast` 更高），并确认 `.jsonl.zst` 不被支持；本节其余条目（compaction 归因、1000 ms 阈值的经验依据、未分类 token 的档位归属）仍需真实数据或构造样本，仍未闭环。

1. **`src/README.md:47-53` 描述的 MultiAgent V2 边界在代码里不存在**：文档说用 `event_msg.task_started` 作为「重放前缀结束」、用 `inter_agent_communication_metadata`（旧名 `inter_agent_communication`）的 `trigger_turn === true` 作为「子回合开始」来切分。全仓 grep（`task_started` / `trigger_turn` / `inter_agent_communication`）**只命中该 README**，解析器没有任何相关字符串或状态。实际实现是 §5.1 的「父前缀逐条相等 + 重写突发」。两者结论方向一致但机制不同，属于文档与代码不一致；因浅克隆（`git log` 仅 1 个提交）无法判定谁先谁后。
2. **`docs/guide/codex/index.md:50` 与 `:116` 说「缺模型元数据的会话被跳过」，与代码冲突**：`parser.rs:813-818` 在无模型时回退 `gpt-5` 并标记 fallback（`rust/adapters/codex/src/README.md:63` 与 `docs/guide/codex/index.md:54` 也是这么说的）。`aggregate.rs:328` 的 `model == None → skip` 分支在解析链上几乎不可达，目前只有手工构造事件的快照用例会命中（`lib.rs:1055-1068`）。
3. **replay 前缀匹配的精确性未在真实日志上验证**：`parser.rs:236` 要求 6 个字段完全相等；父日志被压缩/改写后完全走启发式。阈值常量本身与两处使用点已核实（定义 `parser.rs:122`，使用点 `parser.rs:171-173` 与 `parser.rs:256-261`），但注释给出的实测区间（10–40 ms vs 5.8–15.3 s）只来自源码注释（`parser.rs:114-118`），我未独立复现。
4. **`detect_rewritten_burst` 会二次打开并重扫文件头**（`parser.rs:129-177`），与主扫描重复 IO；触发条件与频率、对超大 rollout 的影响未测。
5. **session 身份是路径派生**（`parser.rs:892-904`），不是 Codex 的会话 UUID；同一会话被重命名/移动后，跨文件去重键（`aggregate.rs:29-42`，非 Session 分组时不含 session）与 `sessionId` 稳定性未验证。
6. **compaction 归属在并行下的确定性**依赖 `file_index`（`aggregate.rs:711-751`），我读了测试（`aggregate.rs:938-965`）但未实跑 `cargo test`；「文件顺序 = `files.sort()` 的字典序」这一前提未进一步验证。
7. **`--mode display` 在 Codex 下不改变金额**是从函数签名（无 mode 参数）推出的结论，仓库内没有针对 Codex 的 display 成本断言测试（只有抑制缺失定价元数据的测试 `report.rs:828-834`）。
8. 官方文档 `docs/guide/codex/index.md` 为 Beta 声明（`:5`），字段与语义随 Codex CLI 版本演进；本次精读只覆盖 v20.0.31 的单一快照。

已复核确认（原存疑项已撤销）：`.jsonl.zst` 无支持（全仓 grep `zstd` 零命中，`flate2` 仅 `ureq` 传递依赖）；速度旗标确为 `--speed`；`CODEX_REWRITTEN_BURST_PAUSE_MS = 1_000` 与两处使用点一致（`parser.rs:122`, `:171-173`, `:256-261`）；Codex 的增量语义与输出层 cached 扣减（`report.rs:123-129`）不矛盾——内部保留总 input、仅在渲染时做 `input - cached - cache_creation`。

## 6. 其余 16 个适配器的数据面矩阵


- 仓库：`ccusage`（只读样本）HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31
- 证据约定：所有 `path:line` 均相对仓库根；行号取实际文件行号（用 `read` 逐文件核对）
- 方法：静态阅读（`read` / `grep` / `glob`）为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5），未修改任何样本
- 覆盖：`rust/adapters/{opencode,copilot,antigravity,pi,grok,kimi,openclaw,codebuff,hermes,zcode,qwen,droid,kilo,goose,gemini,amp}`、`rust/adapters/common/src/{jsonl.rs,lib.rs,report.rs}`、`rust/crates/ccusage-adapter-all/src/{loader.rs,report.rs,types.rs}`，并以 `ccusage-core` 的 `types/cost/utils/home/date_utils/progress` 作为公共零件参照
- 边界：Claude（`rust/adapters/claude/`）与 Codex（`rust/adapters/codex/`）另见 §4 与 §5，本章只在跨适配器对比处引用其行号，不重复展开
- **本章裸文件名约定**：各小节标题里的 agent 名即该节裸文件名的宿主——如 `grok/src/parser.rs:170` 是简写，完整路径为 `rust/adapters/grok/src/parser.rs`；`common/...` 指 `rust/adapters/common/src/`；统一报表语境的 `loader.rs`/`report.rs`/`types.rs` 指 `rust/crates/ccusage-adapter-all/src/`；`pricing.rs`/`cost.rs`/`types.rs`（core 语境）指 `rust/crates/ccusage-core/src/`。

### 0. 结论速览

| 问题 | 结论 | 关键证据 |
| --- | --- | --- |
| 有没有统一的「发现→解析→LoadedEntry」模板函数 | **没有**。`common/` 只提供零件（文件收集 / 并行读 / 日期过滤 / JSONL 迭代器），每家 adapter 自己写 `load_entries` + `load_entries_inner` | `common/src/lib.rs:14`, `common/src/lib.rs:18`, `common/src/lib.rs:84`, `common/src/jsonl.rs:47`，对照 `amp/src/loader.rs:8` |
| 16 家共有的包装 | 全部 16 家的入口都包在 `ccusage_core::progress::track_usage_load` 里（进度条 + 计时） | `ccusage-core/src/progress.rs:290`；`amp/src/loader.rs:9`, `antigravity/src/loader.rs:12`, `zcode/src/loader.rs:51` |
| 并行读文件 | 15 家用 `common::read_files_parallel`（按文件大小分块、结果按原文件序重组）；**antigravity 是唯一顺序 for 循环的** | `common/src/lib.rs:84`, `common/src/lib.rs:49`；`antigravity/src/loader.rs:23` |
| SQLite 适配器 | 7 家：`opencode`、`openclaw`、`zcode`、`kilo`、`goose`、`hermes`、`antigravity`；**全部用 `sqlite` crate（无 rusqlite）**，全部 `with_read_only()` | `rust/Cargo.toml:65`；`zcode/src/loader.rs:91`, `goose/src/loader.rs:75`, `hermes/src/loader.rs:43`, `kilo/src/loader.rs:52`, `openclaw/src/sqlite.rs:87`, `opencode/src/loader.rs:288`, `antigravity/src/parser.rs:146` |
| token 四桶归一到哪里 | `LoadedEntry.data.message.usage: TokenUsageRaw`（input/output/cache_creation_input/cache_read_input），汇总时经 `TokenCounts::add_usage` 落到 `input/output/cache_creation/cache_read` 四桶 | `ccusage-core/src/types.rs:29`, `ccusage-core/src/types.rs:69`, `ccusage-core/src/types.rs:78` |
| 把 cached 从 input 里减掉 | **本章 16 家范围内**明确做的有四家：`grok`（`split_input_tokens`）、`copilot`（input − cache_read − cache_creation）、`zcode`（input − cache_read − cache_creation）、`gemini`（条件性 `subtract_cached_overlap_tokens`）；Claude 侧不在此列（它本来就分桶写，见 §4） | `grok/src/parser.rs:170`, `copilot/src/parser.rs:445`, `zcode/src/parser.rs:47`, `gemini/src/parser.rs:400` |
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
| — | `claude`（对照） | `$XDG_CONFIG_HOME/claude` 与 `~/.claude` 两处的 `projects/**/*.jsonl`（`CLAUDE_CONFIG_DIR` 可多根覆盖；见 §4.1） | JSONL | 见 `claude/src/lib.rs:510` 起字节级迭代 | 否 | 是 `costUSD`：记录内厂商成本映射到 `UsageEntry.cost_usd`（`ccusage-core/src/types.rs:14-15`），`Display` 模式直接采用（`claude/src/lib.rs:525-532`） |
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

> **本节状态（第 8 轮实测后）**：§13.5 的实测面向 Claude / Codex 与价格链，**未覆盖本章各适配器的内部细节**（protobuf 映射、credits 归因、SQL 细节、replay 剔除点等）；本节各条仍为静态阅读下的未决项，需样本或额外数据才能闭环。

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

## 7. 聚合、时间与报告形态


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

## 8. 定价数据来源、匹配链与金额计算


> 研究对象：ccusage 仓库浅克隆（HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31）。第 1–7 轮为静态阅读；第 8 轮已构建并运行（见 §13.5）。
> 主文件：`rust/crates/ccusage-core/src/pricing.rs`（6181 行；1–2894 逻辑、2895–6181 测试）、`build.rs`（255）、`cost.rs`（664）。行号均为该克隆中的实际行号。

### 0. 结论速览

- 价格来自**四处**：编译期内嵌 LiteLLM 快照（白名单裁剪）、编译期内嵌 models.dev 扁平快照、运行期在线 LiteLLM `main` 原始 JSON、运行期在线 `https://models.dev/api.json`；用户侧没有独立价格文件，只有 config 的 `pricingOverrides`（字段级补丁）与 `CCUSAGE_MODEL_ALIASES`（名字改写）。
- 合并**没有深合并**：LiteLLM 按 key 整条替换，models.dev 只补缺失 id，内置兜底表只 `or_insert`，用户 override 是唯一的字段级合并且永远最后执行。
- 匹配链：精确 → 静态别名 → `.`/`@`→`-` 归一化 → 模糊子串（最长键 + 版本边界保护）→ 在线 models.dev → 内嵌 models.dev，外加 `exact_only` 门禁。
- 计价有两种档位语义：**整单切换**（有 per-model `long_context_threshold`）与**边际分段**（LiteLLM `*_above_200k_tokens`，默认 200K）。1 小时缓存写入按 `input × 2`。注意 Codex 侧的阈值判定只看 `input_tokens` 总量（见 §6.2），与 Claude 链的「上下文总量」口径不同。
- 缺价一律回退 `0.0`，同时用 `missingPricing` / `unpricedModels` / stderr 警告三处披露；显式 0 价不算缺价。

---

### 1. 价格数据来源

#### 1.1 编译期内嵌 LiteLLM 快照

```text
build.rs:17-40
  CCUSAGE_PRICING_JSON_PATH 有值 → 读该文件（:22-25，Nix 构建的唯一通道）
  否则 → fetch_pricing_json()，未开 fetch-litellm-pricing feature 直接 panic（:75-82）
       开启时 minreq 下载，超时 10s（:15, :89-105）
  compact_pricing_json(:155-201) → deflate(:60-62) → $OUT_DIR/litellm-pricing.json.deflate
```

- 下载 URL 由 `flake.lock` 的 `nodes.litellm.locked` 拼成（`build.rs:107-136`）：`https://raw.githubusercontent.com/{owner}/{repo}/{rev}/model_prices_and_context_window.json`。
- 运行期以 `include_bytes!(concat!(env!("OUT_DIR"), "/litellm-pricing.json.deflate"))` 内嵌（`pricing.rs:20-21`），**首次使用**时惰性解压一次（`inflate_snapshot` + `OnceLock`，`pricing.rs:33-44`）。
- 压缩是裸 deflate 最高档 `miniz_oxide::deflate::compress_to_vec(bytes, 10)`（`build.rs:60-62`），解压 `miniz_oxide::inflate::decompress_to_vec`（`pricing.rs:35-37`）。
- 压缩键映射（`build.rs:168-195`）：`input_cost_per_token→i`、`output_cost_per_token→o`、`cache_creation_input_token_cost→cc`、`cache_read_input_token_cost→cr`、`*_above_200k_tokens→ia/oa/cca/cra`、`max_input_tokens→ctx`、`provider_specific_entry.fast/flex→fast/flex`；两个过滤条件：源字段**值为 null 时跳过**（`:182-184`）、`fast`/`flex` 为 null 时不写入（`:190-191`）；最后**只有 `i` 与 `o` 同时存在才保留**（`:196-198`）。注意 `provider_specific_entry` 在**压缩形状**里由 `compact.fast.or(compact.flex)` 重建（`pricing.rs:2612-2618`），因此只有 fast 或只有 flex 的条目也能取到倍率。
- 白名单 `is_embedded_model`（`build.rs:203-221`）：`claude-`、`anthropic.`、`anthropic/`、`us./eu./global./jp./au.anthropic.`、`gpt-`、`o3`、`o3-*`、`o4-mini`、`o4-mini-*`、`openai/`、`azure/`、`zai/`、`openrouter/openai/`。测试 `embedded_build_time_pricing_is_compact` 断言内嵌 JSON < 200 000 字节、无 `"source"`、无 `vertex_ai/`、含 `claude-opus-4-6`（`pricing.rs:5844-5851`）。

当前 pin（`flake.lock:143-145` / `:159-161`）：

| 节点 | owner/repo | rev |
| --- | --- | --- |
| `nodes.litellm` | `BerriAI/litellm` | `d9123f459561754f41fd598a73b405b091d3bf84` |
| `nodes.models-dev` | `anomalyco/models.dev` | `9cb380733df864b6538c7d6a42111d28897901f5` |

#### 1.2 编译期内嵌 models.dev 快照

两份**已提交**生成物：`rust/crates/ccusage-core/src/models-dev-pricing.json`（544 555 B，扁平 `model_id → {cost, limit, exactOnly?}`，同一模型含作者目录与各云平台/转售目录拼写，如 `claude-opus-4-6`(7341)、`anthropic.claude-opus-4-6-v1`(3932)、`us.anthropic.claude-opus-4-6-v1`(30696)）与 `models-dev-catalog-rules.json`（14 569 B，字段 `owners`/`platforms`/`authoredModelIds`/`assetPricedModelIds`，结构见 `pricing.rs:596-617`）。生成流程：`justfile` 的 `gen-models-dev-pricing`（nix 从 `models-dev` input 构建后拷贝）、`update-models-dev-pricing`（先 `nix flake update models-dev`）。`build.rs:46-56` 用 `embed_snapshot` 把二者 minify + deflate 内嵌（原因见 `build.rs:42-45`：仓库保持缩进便于 diff，二进制不应带 2K+ 模型缩进），访问器在 `pricing.rs:46-54`。

#### 1.3 运行期在线抓取

| 端点 | URL | 常量 |
| --- | --- | --- |
| LiteLLM | `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` | `pricing.rs:55-56` |
| models.dev | `https://models.dev/api.json` | `pricing.rs:57` |

HTTP 客户端在二进制侧（`rust/crates/ccusage/src/http.rs`），由 `set_json_fetcher` 注入（`pricing.rs:2872-2884`，安装点 `ccusage/src/main.rs:26`），原因是避免 TLS 栈成为 core 的依赖（`pricing.rs:2876-2881`）。

- 只接受 `PricingEndpoint::for_url` 认得的 URL（`http.rs:25-34`、`pricing.rs:72-78`）。
- ETag 磁盘缓存：`$XDG_CACHE_HOME/ccusage/http-cache/`，否则 `~/.cache/ccusage/http-cache/`（`http.rs:37-52`、`:165-169`），文件名 `{sanitize(url)}-{fnv1a64:x}.cache`（`http.rs:237-257`），内容 `etag\nbody`，写入用临时文件 + rename（`http.rs:199-229`）。
- 请求带 `if-none-match`（`http.rs:80-82`）；`304` 仅在缓存体通过端点校验后才复用，否则作废缓存并**重取一次**（不带 validator）（`http.rs:88-99`）；非 200 直接报错（`:100-102`）。
- 超时 `timeout_global(10s)`（`http.rs:11`、`:75-78`）；体积上限 `64 MiB`（`http.rs:12`、`:104-151`）；新响应必须过 `validates_shape`（`http.rs:121-126`、`pricing.rs:88-98`、`:128-194`）。
- **无重试**：除上面 304 那条路径外不二次请求；传输错误不会复用缓存体（测试 `http.rs:684-695`）。

#### 1.4 用户自定义价格

只有两条路径，**没有**「用户价格文件环境变量」：

1. config 的 `pricingOverrides`：schema `ConfigPricingOverride`（`ccusage-config/src/config_schema.rs:593-607`），使用点 `config_schema.rs:397-398`、`:508-509`；字段（camelCase）`inputCostPerToken`、`outputCostPerToken`、`cacheCreationInputTokenCost`、`cacheReadInputTokenCost`、四个 `…Above200kTokens`、`maxInputTokens`、`fastMultiplier`、`flexMultiplier`。
2. `CCUSAGE_MODEL_ALIASES`（`model_aliases.rs:8`）：把请求名改写到另一价格键（§5.1）。

config 文件发现顺序（`ccusage-config/src/config.rs:246-299`）：`--config <path>` → `<cwd>/.ccusage/ccusage.json` → 各 `CLAUDE_CONFIG_DIR`（逗号分隔）下的 `ccusage.json` → `~/.config/claude/ccusage.json`、`~/.claude/ccusage.json`；取第一个可读且为 JSON 对象的文件（`:246-256`）。合并进 `SharedArgs.pricing_overrides` 是字段级（`:627-681`，测试 `:1049-1086`）。

**证据**：`build.rs:13`、`:17-40`、`:60-62`、`:75-82`、`:107-136`、`:155-221`、`pricing.rs:20-54`、`:55-61`、`:2872-2893`、`http.rs:11-12`、`:25-151`、`:165-257`、`config_schema.rs:593-607`、`config.rs:246-299`、`:627-681`、`model_aliases.rs:8`、文档 `docs/guide/config-files.md:407-464`.

---

### 2. 加载顺序与覆盖规则

两个入口：`load_embedded()`（`pricing.rs:982-1000`，纯离线基线）与 `load_with_overrides(offline, log, overrides)`（`pricing.rs:1002-1038`，生产路径；调用例 `claude/src/lib.rs:144-152`、`pi/src/lib.rs:22-27`、`ccusage-adapter-all/src/loader.rs:83-89`）。

```text
load_embedded():                                  # :982-1000
  1) entries ← 内嵌 LiteLLM（load_json_with_overrides）
  2) put_builtin_pricing()                        # :1980-1983，entry().or_insert()，只补缺键
  3) fill_long_context_rates_from_models_dev()    # :1946-1978
  4) enable_embedded_models_dev_fallback = true   # :998

load_with_overrides(!offline):                    # :1002-1038
  1) load_embedded()
  2) if !offline: 在线 LiteLLM → load_json()      # :1007-1028，键级整条替换
  3) fill_long_context_rates_from_models_dev()    # :1034（在线刷新会整条替换，必须重铺）
  4) enable_models_dev_fallback = !offline        # :1035
  5) apply_overrides(user)                        # :1036-1037，字段级，最后执行
```

| 层 | 合并语义 | 证据 |
| --- | --- | --- |
| 内嵌/在线 LiteLLM → `entries` | 逐键 `insert`，同键整条替换；刷新**只增不删** | `pricing.rs:1048-1108`（`:1080-1100`）、`:1014-1020` |
| 内置兜底表 | 默认 `entry(...).or_insert(pricing)`（只补缺键）；**3 处例外走无条件 `self.entries.insert`**：`gpt-5.1-codex`(`:2369-2370`)、`gpt-5.2-codex`(`:2386-2387`)、`gpt-5.2`(`:2400-2408`)——上游快照已有价时也会被覆盖，与注释 `:2010-2014` 声明的意图相反 | `pricing.rs:1980-1983`、`:2369-2370`、`:2386-2387`、`:2400-2408` |
| models.dev（内嵌/在线） | 只补主表没有的 id；装载内部按信任等级与详细度整条替换，替换时**删除落败拼写的旧条目**并解绑其 context limit | `pricing.rs:1241-1243`、`:1259-1284`（删除 `:1277-1284`）、`:1328-1330` |
| 长上下文档位 | 仅当四个 `*_above_200k` 全空才铺；上游已有任一项则完全不动 | `pricing.rs:1946-1978` |
| 用户 override | 字段级；`input` 改动且 **`base.input > 0.0`**、cache 是派生值（`!cache_read_explicit`）时按 `new/old` 同比缩放；`cache_*_explicit` 与 base 取或后写回 | `pricing.rs:1820-1915`（缩放前提 `:1838-1845`、取或 `:1890-1893`） |

用户 override 细节：

- 结果写回 `entries`（`pricing.rs:1911`），同时保留一份在 `user_overrides`（`:1821-1822`）供时间调度后重新叠加（`:1468-1481`）。
- 已有条目的缓存价是派生值时按比例缩放（`pricing.rs:1838-1883`；缩放前提是 `override.input_cost_per_token.is_some() && base.input > 0.0 && !base.cache_read_explicit`，`:1838-1840`；写回时 `cache_read_explicit`/`cache_create_explicit` 与 base 取或，`:1890-1893`；测试 `pricing.rs:6069-6179` 三条）；新模型则新建条目，未给字段为 0.0（测试 `pricing.rs:6021-6033`）。
- 别名键的 override 以别名目标条目为底（`pricing.rs:1823-1828`）。

失败降级：无 HTTP 客户端 → 等价离线（`pricing.rs:2886-2893`）；LiteLLM 抓取或解析失败（0 条）→ 仅当 `log_level() >= 4` 时 WARN，继续用已有表（`:1016-1027`、`:2810-2812`）；models.dev 失败 → WARN + **60 秒退避**（`:58`、`:814-818`、`:2839-2849`）；`--offline` → 全部网络步骤跳过且 `enable_models_dev_fallback=false`（`:1007`、`:1035`，测试 `:3934-3946`）。

**证据**：`pricing.rs:982-1038`、`:1040-1108`、`:1820-1915`、`:1946-1978`、`:1980-1983`、`:2810-2812`、`:2835-2861`.

---

### 3. 价格数据结构

#### 3.1 计算条目 `Pricing`（完整字段，含原注释）

```rust
// pricing.rs:196-220
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub(crate) cache_create: f64,
    pub cache_read: f64,
    pub cache_read_explicit: bool,
    /// Whether `cache_create` came from published data rather than the derived
    /// `input * 1.25` default, so provider-fact patches know what they may fix.
    cache_create_explicit: bool,
    pub input_above_200k: Option<f64>,
    pub output_above_200k: Option<f64>,
    pub(crate) cache_create_above_200k: Option<f64>,
    pub cache_read_above_200k: Option<f64>,
    // Token count above which the `*_above_200k` rates apply. The field names
    // keep the LiteLLM `_above_200k_tokens` suffix for JSON compatibility, but
    // some providers switch tiers at a different point (OpenAI long-context
    // pricing starts above 272K input tokens), so the threshold is per model.
    pub(crate) long_context_threshold: Option<u64>,
    pub fast_multiplier: f64,
    pub flex_multiplier: f64,
}
/// Default tier boundary for LiteLLM `*_above_200k_tokens` pricing fields.
pub(crate) const DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS: u64 = 200_000;   // :219-220
```

`Pricing::empty()` 全 0、倍率 1.0（`pricing.rs:233-249`）。

| 字段 | 参与金额 | 说明 |
| --- | --- | --- |
| `input` / `output` | 是 | §6 |
| `cache_create` | 是 | 5 分钟缓存写入单价 |
| `cache_read` | 是（例外） | Codex 在 `!cache_read_explicit` 时改用 `input`（`codex/report.rs:344-348`） |
| `cache_read_explicit` / `cache_create_explicit` | 间接 | 决定 override 缩放与 provider 补丁是否生效（`:1838-1845`、`:1990-2008`） |
| 四个 `*_above_200k` | 是 | 超阈值档位；任一非空即视为上游已带档位 |
| `long_context_threshold` | 是 | 有值 → 整单切换；无值 → 200K 边际（§6.1） |
| `fast_multiplier` / `flex_multiplier` | 是 | 服务档倍率（§6.3） |

#### 3.2 上游输入类型

```rust
struct LiteLlmPricing {                         // :540-552（完整形状）
    input_cost_per_token, output_cost_per_token,
    cache_creation_input_token_cost, cache_read_input_token_cost,
    input/output/cache_creation/cache_read_cost_per_token_above_200k_tokens,
    max_input_tokens, provider_specific_entry: Option<{ fast, flex }>,   // :554-558
}
struct CompactLiteLlmPricing {                  // :560-573（内嵌压缩形状）
    i, o, cc, cr, ia, oa, cca, cra, ctx, fast, flex,
}
struct ModelsDevModel { id, cost, limit{context}, modalities{input,output}, exactOnly }  // :834-877
struct ModelsDevCost  { input, output, cache_read, cache_write, tiers[] }
struct ModelsDevCostTier { input, output, cache_read, cache_write, tier{type,size} }
```

`parse_litellm_pricing`（`:2596-2626`）先试压缩形状（要求含 `i` 与 `o`），否则按完整形状解析并要求 input/output 同时存在。models.dev 单价是**每百万 token**，装载时除 1e6（`:909-912`、`:1285-1301`）。

#### 3.3 缺省派生（关键语义）

```rust
cache_create = cache_creation_input_token_cost.unwrap_or(input * 1.25)   // :1085-1087
cache_read   = cache_read_input_token_cost.unwrap_or(input * 0.1)        // :1088
```
（models.dev 侧同规则 `:1294-1301`。）`*_explicit` 正是用来区分「上游给的」与「派生的」，这也是改 `input` 时派生缓存价会同比缩放的原因。

#### 3.4 内置兜底表与补丁

`put_builtin_pricing`（`:2015-2593`）**默认**只在快照缺键时兜底（`entry().or_insert()`，注释 `:2010-2014`：上游按小时刷新，用冻结数字覆盖会把过期价引回来），**但有 3 处走无条件 `self.entries.insert`**，上游已有价也会被兜底值覆盖：`gpt-5.1-codex`(`:2369-2370`，未套 flex 倍率)、`gpt-5.2-codex`(`:2386-2387`)、`gpt-5.2`(`:2400-2408`，套 flex 倍率)。覆盖 `claude-opus-4-5/4-6/4-7/4-8`(5e-6/25e-6，缓存 6.25e-6/0.5e-6)、`claude-haiku-4-5`、`claude-opus-4`、`claude-sonnet-4-6`、`claude-sonnet-4`(带 200K 档位 `:2158-2175`)、`claude-3-5-haiku`(+`-20241022`)、`claude-3-opus/-sonnet/-haiku`、`gpt-5`、`gpt-5.5`、`grok-4.3`、`gpt-5.1-codex`、`gpt-5.2-codex`、`gpt-5.3-codex`(`:2388-2400`)、`gpt-5.2`、`gpt-5.4`、`gpt-5.4-mini/-nano`、`gpt-5.6-sol/-terra/-luna`(`:2475-2502`，5e-6/30e-6、2.5e-6/15e-6、1e-6/6e-6)、`glm-5/-turbo/5.1`；GLM 走 `put_builtin_glm`（`:1990-2008`，只补缓存字段、不覆盖上游显式缓存价，测试 `:3779-3830`）；上下文上限写在 `:2557-2591`（如 `gpt-5.5 → 1_050_000`）。

**证据**：`pricing.rs:196-250`、`:540-573`、`:834-912`、`:1080-1108`、`:1285-1315`、`:1990-2593`、`codex/report.rs:344-348`.

---

### 4. 模型名匹配算法（重点）

#### 4.1 匹配链

```text
find(model)                                                 # :1354-1391（含 find_cache，miss 也缓存）
└─ find_with_fallbacks(model, models_dev, embedded_models_dev)          # :1393-1454
   1) alias = model_aliases::resolve_model_name(model)                  # :1399
   2) 精确阶段（Fuzzy::Denied）：
        find_entry_or_alias_with_fallback(model, Denied, None)          # :1405-1406
        ?: (alias != model) 时对 alias 再做一次                          # :1407-1413
        命中即返回                                                       # :1414-1416
   3) fuzzy = allows_fuzzy_lookup_with_fallbacks(model, alias, …)       # :1420-1425
   4) 模糊阶段：本表(model) → 本表(alias) → 在线 models.dev(alias) → 内嵌 models.dev(alias)  # :1426-1453

find_entry_or_alias_with_fallback(model, fuzzy, embedded)               # :1559-1574
   1) entries.get(model)                       # 精确 :1565-1567
   2) pricing_alias(model) → find_entry_with_fallback(alias)            # :1568-1572
   3) find_entry_with_fallback(model)                                   # :1573

find_entry_with_fallback(model, fuzzy, embedded)                        # :1586-1615
   1) entries.get(model)                                                # :1592
   2) exact_only.id_spelled_by(model) → entries[id]                     # :1595-1597（见 :517-521）
   3) fuzzy == Denied 或 is_exact_only_lookup_with_fallback(model) → None  # :1598-1602
   4) 扫描全表取最长匹配：filter(!exact_only) + filter(pricing_key_matches)
      max_by(|l, r| l.len().cmp(&r.len()).then_with(|| r.cmp(l)))   # :1604-1613
```

第 4 步 tie-break 的准确语义：**最长键优先；长度相同时字典序较小者胜**（`max_by` 收到 `r.cmp(l)`，故右侧更大时不替换当前最大者）。`context_limit` 的模糊扫描使用同一比较器（`pricing.rs:1803-1805`），并同样受 `exact_only` 过滤（`:1799`）。

另有一条只做「精确 + 分隔符归一化」的路径 `find_exact_normalized`（`:1518-1533`），由 `find_exact_with_fallback`（`:1501-1516`）与 pi 的 `find_exact` 使用，依赖预建索引 `normalized_keys()`（`:1535-1548`，同拼写多键时记 `None`）。上下文上限查询 `context_limit` 走**结构并行的同一套链**（`:1685-1808`）。

#### 4.2 归一化

```rust
// pricing.rs:2724-2731
fn normalized_pricing_key(value: &str) -> Cow<'_, str> {
    if value.contains(['.', '@']) { Cow::Owned(value.replace(['.', '@'], "-")) }
    else { Cow::Borrowed(value) }
}
```

只把 `.` 与 `@` 换成 `-`，**不折叠大小写**；`claude-opus-4.7-20260416` ≡ `claude-opus-4-7-20260416`，`claude-opus-5@eu` ≡ `claude-opus-5-eu`（测试 `:3508-3535`）。models.dev **装载侧**另用 `normalized_models_dev_model_id`（`:750-759`，会 `to_lowercase()`）做拼写归并（`:1239`、`:1274-1284`），两者不可混用。日期后缀剥离 `model_without_date_suffix`（`:2767-2790`，支持 `-YYYY-MM-DD` 与 `-YYYYMMDD`）只用于档位继承（`:1959-1964`）、阈值查询（`:2755-2760`）与倍率查找（`:961`）。

#### 4.3 模糊匹配的边界规则

```rust
// pricing.rs:2667-2722
fn pricing_key_matches(candidate, model, normalized_model) -> bool {
    contains_pricing_key(model, candidate)                     // 候选是查询子串
        || contains_pricing_key(candidate, model)              // 查询是候选子串
        || contains_pricing_key(normalized_model, &normalized_pricing_key(candidate))
        || contains_pricing_key(&normalized_pricing_key(candidate), normalized_model)
}
fn contains_pricing_key(value: &str, key: &str) -> bool {
    value.match_indices(key).any(|(index, _)| {
        let before = index.checked_sub(1).and_then(|i| value.as_bytes().get(i)).copied();
        let suffix = &value[index + key.len()..];
        before.is_none_or(is_pricing_key_boundary) && suffix_allows_pricing_key_match(key, suffix)
    })
}
fn is_pricing_key_boundary(byte: u8) -> bool { !byte.is_ascii_alphanumeric() }        // :2689-2691
fn suffix_allows_pricing_key_match(key, suffix) -> bool {
    let Some(sep) = suffix.as_bytes().first().copied() else { return true };
    if !is_pricing_key_boundary(sep) { return false }          // 后接字母/数字 → 拒绝
    !suffix_starts_with_numeric_model_version(key, suffix)     // :2703-2722：数字版本边界保护
}
// suffix_starts_with_numeric_model_version：key 末位是数字 + 后缀以 '-'/'.' 开头 + 其后是数字
// → 拒绝（视作不同版本）；例外：数字正好 8 位（MODEL_DATE_SUFFIX_DIGITS=8，:59-61）且后面是尾/边界
```

后果与测试：

- 双向子串都必须落在非字母数字边界上；版本升级不会被误降级：只有 `claude-opus-4` 时 `claude-opus-4.8-20260528`/`-4-9`/`-5`/`-4.70` 全 miss，`claude-opus-4-20250514` 命中（`pricing.rs:5705-5732`）。
- 日期后缀放行（`:5734-5739`）；边界必须存在（`:5662-5703`）。
- 全长优先：`claude-sonnet-4-20250514-via-bedrock` 命中更长键 `claude-sonnet-4-20250514`(2.0) 而非 `claude-sonnet-4`(1.0)（`:5853-5898`）。

#### 4.4 `exact_only` 门禁

动机见结构体注释（`pricing.rs:419-424`）：若 `kimi-k2.7-code-highspeed` 这类单独定价的高档位留在模糊扫描里，基础模型会因「最长匹配」抢到它。

- `ExactOnlyKeys`（`:443-473`）：原始拼写集合 `raw` + 归一化拼写索引 `normalized`（同拼写多 id 记 `None`）。三个查询方法对应三种语义：`contains`（`:501-503`）问「这个候选键本身是否 exact-only」，供 fuzzy 扫描过滤候选；`contains_any_spelling`（`:507-512`）问「这个请求名是否属于 exact-only 的任一拼写」，供门禁决定能否模糊；`id_spelled_by`（`:517-521`）问「该拼写唯一指认哪个 exact-only 条目」，决定由哪个条目作答。
- 查询侧：`is_exact_only_lookup_with_fallback`（`:1646-1654`）检查本表与内嵌 models.dev；`allows_fuzzy_lookup_with_fallbacks`（`:1664-1683`）在查询名或其静态别名属于 exact-only 时返回 `Fuzzy::Denied`（`:1676-1682`）。
- 标记来源：内嵌快照直接带 `"exactOnly": true`（`ModelsDevModel.exact_only`，`:840-843`；实例 `models-dev-pricing.json:7634-7645` 的 `claude-opus-5-fast`）；在线响应无此字段，用生成期同一规则重推 `ModelsDevCatalogRules::is_exact_only`（`:674-698`）= 是作者模型的 tier 变体（归一化后以作者 id 开头且紧跟 `-`，且不含 `/`）或 id 不含任何数字（`:761-768`）。
- 门禁跨数据源生效：主表能 fuzzy 命中基础模型也不行，必须让内嵌 models.dev 的档位条目接手（注释 `:1617-1637`）。测试：`:3342-3368`、`:3371-3453`、`:3480-3536`、`:3648-3704`、`:4702-4728`。

#### 4.5 四个给定名字的命中推演

**(a) `claude-sonnet-4-20250514`**：**实测修正（第 8 轮）——不是精确命中，而是模糊命中**。内嵌表（534 键，见 §13.5）里**没有**该裸键（`claude-*` 裸键只覆盖 4.5 代及以后）；真实路径是双向子串落到区域前缀的 Bedrock 键——实测命中最长键 `global.anthropic.claude-sonnet-4-20250514-v1:0`（长度 46），候选另含 `anthropic.` / `us.` / `eu.` 三个前缀的变体（**`apac.` 不在内嵌表**——裁剪白名单不含该前缀，它只存在于未裁剪的完整 LiteLLM）。仓库测试 `embedded_pricing_resolves_overlapping_model_keys_exactly`（`pricing.rs:4931-4955`）正是这项行为的断言：它比较 `find("claude-sonnet-4-20250514")` 与 `find("anthropic.claude-sonnet-4-20250514-v1:0")` 的 **input 相等**，而非断言裸键存在。8 位日期后缀放行规则（`suffix_starts_with_numeric_model_version`，`:2703-2722`）与「取最长键」（`:5853-5898`）仍如原述；`claude-3-5-haiku-20241022` 则**不需要模糊匹配**：它是**内嵌兜底表的精确命中**——`pricing.rs:2191-2192` 用 `put_builtin_entry` 把 `claude-3-5-haiku` 与 `claude-3-5-haiku-20241022` 直接写入 `entries`，精确阶段的 `entries.get(model)`（`:1565-1567`）就作答了，三个同价的 Bedrock 变体（`anthropic.` / `eu.` / `us.`，input 均为 `8e-7`）不会被选中。这也解释了为什么 `pricing.rs:4951-4954` 的断言（input `0.8e-6`）**无法区分命中路径**——三条候选价同。（若它真的走模糊扫描，会按「同长度取字典序较小者」在并列候选里选中 `eu.`：实测该组长度为 `eu.` 43、`us.` 43、`anthropic.` 40。）

**(b) `claude-opus-4-6-fast`**：models.dev 快照中**没有**该键（只有 `claude-opus-4-8-fast`:7504、`claude-opus-5-5-fast`:7598、`claude-opus-5-fast`:7634，三者均带 `"exactOnly": true`，见 `models-dev-pricing.json:7504-7515`、`:7598-7609`、`:7634-7645`）。精确 miss 后 fuzzy：`contains_pricing_key("claude-opus-4-6-fast", "claude-opus-4-6")` 成立（前边界为空、后缀 `-fast` 首字节是边界、key 末位非数字）→ 命中基础条目 `claude-opus-4-6`（`:2667-2722`）。前提是该拼写不是 exact-only，否则 `allows_fuzzy_lookup_with_fallbacks` 拒绝 fuzzy 并转内嵌 models.dev（`:1664-1683`、`:1450-1453`）。fast 倍率由 `fast-multiplier-overrides.json` 的 `normalized_prefix["claude-opus-4-6"]=6.0` 提供（`SpeedMultiplierOverrides::multiplier_for` `:944-978`、`matches_model_suffix` `:2802-2808`），测试 `:5782-5842`（`claude-opus-4.7-20260416` → 6.0，无 tier 关系的 `claude-opus-4-70` → 1.0）。现实入口是 Claude adapter 在 `speed: fast` 时把**展示名**写成 `{model}-fast`（`claude/src/lib.rs:535-543`），金额仍按原始模型名 + `Speed::Fast`（`:525` → `cost.rs:110-115`）。

**(c) `gpt-5.3-codex`**：内置兜底表有精确键（`:2388-2400`），快照也有（`models-dev-pricing.json:15868`、`:5691` 的 `azure/…`、`:24474` 的 `openai/…`）→ 精确命中。fast 倍率 2.0 来自 `fast-multiplier-overrides.json` 的 `exact`，测试 `:5418-5431`、`:5741-5780`（缺上游倍率仍为 2.0，而 `gpt-5.2-codex` 保持 1.0）；计价走 Codex 专属公式（§6.2）。

**(d) `[pi] gpt-5.4`**：名字由 pi adapter 渲染为 `[<store>] <model>`（`pi/src/parser.rs:352-356`，默认 store 名 `pi`，`:225-231`）。查找顺序 `store_pricing_at`（`:697-708`）/`store_pricing`（`:685-695`）：`override_model → find_at/find` → `display_model → find_exact` → `raw_model → find_at/find`。`pricing_override_model`（`:629-638`）先问 `has_user_override(display_model)`（`pricing.rs:1489-1494`，含别名解析）再问 raw。因此：有 `pricingOverrides["[pi] gpt-5.4"]` → 命中 `apply_override` 写回 `entries` 的条目（`pricing.rs:1911`），且 Auto 模式被强制改为 token 重算（`source_cost_for_mode` `parser.rs:640-650`）；无 override → `find_exact("[pi] gpt-5.4")` 必 miss，退到 `raw_model="gpt-5.4"` 命中内置条目（`pricing.rs:2409-2430`）或快照（`models-dev-pricing.json:15900`）。测试：`pi/src/parser.rs:774-790`、`:950-984`、`:863-880`、`:915-948`。

**证据**：`pricing.rs:1354-1454`、`:1461-1482`、`:1518-1548`、`:1559-1615`、`:1646-1683`、`:2666-2808`、`:443-522`、`:674-698`、`:761-768`、`pi/src/parser.rs:225-231`、`:352-356`、`:629-708`、`claude/src/lib.rs:525-543`.

---

### 5. 别名表

#### 5.1 `CCUSAGE_MODEL_ALIASES`（运行期用户别名）

- 常量 `MODEL_ALIASES_ENV = "CCUSAGE_MODEL_ALIASES"`（`model_aliases.rs:8`）；无内置默认，未设即空表（`:32-37`）。
- 解析 `parse_model_aliases`（`:39-61`）：内容以 `{` 开头且可解析为 `BTreeMap<String,String>` → 按 JSON；否则剥外层花括号后用 `,`/`;`/换行切分、每段 `from=to`（`:52-60`）。结果存 `OnceLock<RwLock<BTreeMap<String,String>>>`（`:10`、`:28-30`）。
- 查询 `resolve_model_name`（`:14-26`）：精确命中返回目标名；否则若名字以 `-fast` 结尾且基名命中，返回 `{alias}-fast`（`:20-24`）；否则返回原名借用。
- 测试：`:103-115`、`:117-125`、`:127-140`、`:142-149`（含 `private-alpha-fast → gpt-5.5-fast`）。

全部使用点：`pricing.rs:1399`（`find`）、`:1462`（`find_at`）、`:1706`（上下文上限）、`:273`（`has_time_dependent_pricing`）、`:1493`（`has_user_override`）、`cost.rs:74`/`:90`（缺价上报用的是解析后的名字）、`pi/src/parser.rs:682`。交叉测试 `pricing.rs:5916-5931`。

#### 5.2 `pricing_alias`（内置静态别名，共三条）

```rust
// pricing.rs:2792-2800
"gpt-reserve"   => Some("gpt-5.6-luna"),
"gpt-5.6"       => Some("gpt-5.6-sol"),
"gpt-5.3-spark" => Some("gpt-5.3-codex-spark"),
```

使用点：`pricing.rs:1569`（精确 miss 后、模糊之前）、`:1677`（别名目标 exact-only 时禁模糊）、`:1827`（override 取 base）、`:1960`、`:2756`（档位/阈值继承）、`:950`/`:967`（倍率继承）、`:1772`（上下文上限）。测试：`:5110-5166`、`:5286-5298`、`:5317-5376`、`:5544-5579`。

#### 5.3 服务档倍率表

上游 `provider_specific_entry.fast/flex` 优先，缺失时用内嵌覆盖表（`pricing.rs:1071-1079`、`:1309-1314`）：

`fast-multiplier-overrides.json` → `exact`: `gpt-5.6-sol/terra/luna`=2.0、`gpt-5.5`=2.5、`gpt-5.4`=2.0、`gpt-5.3-codex`=2.0、`gpt-6-astra/sol`/`gpt-6.1-sol`/`gpt-6-luna`=2.0；`normalized_prefix`: `claude-opus-4-6`=6.0、`claude-opus-4-7`=6.0、`claude-opus-4-8`=2.0。
`flex-multiplier-overrides.json` → `exact`（**20 个键，全部 0.5**）：`gpt-6-astra`、`gpt-6.1-sol`、`gpt-6-luna`、`gpt-6-sol`、`gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`、`gpt-5.5`、`gpt-5.5-pro`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.4-nano`、`gpt-5.4-pro`、`gpt-5.2`、`gpt-5.1`、`gpt-5`、`gpt-5-mini`、`gpt-5-nano`、`o3`、`o4-mini`；`normalized_prefix` 为空。

查找算法 `SpeedMultiplierOverrides::multiplier_for`（`pricing.rs:944-978`）：`exact.get(model)` → `pricing_alias(model)` → 按 `/`、`:` 切段并对每段枚举「点号之后的子串」 → 逐候选 `exact.get(part)` → `exact.get(model_without_date_suffix(part))` → `pricing_alias(base)` → `normalized_prefix` 前缀匹配（`.`/`@`→`-` 后用 `matches_model_suffix`）。测试：`:4826-4879`、`:5418-5461`、`:5462-5535`、`:5580-5605`。

**证据**：`model_aliases.rs:8-150`、`pricing.rs:2792-2808`、`:944-978`、`fast-multiplier-overrides.json`、`flex-multiplier-overrides.json`、`pricing.rs:1040-1079`、`:1285-1315`.

---

### 6. 金额公式（重点）

#### 6.1 通用路径（`cost.rs`）

```rust
// :31-46 模式分支
Display   => cost_usd.unwrap_or(0.0)
Auto      => cost_usd.unwrap_or_else(|| calculate_cost_from_tokens(..))
Calculate => calculate_cost_from_tokens(..)

// :93-116 取价 + 服务档倍率
let Some(model) = model else { return 0.0 };
let Some(pricing) = (有 ts ? pricing.find_at(model, ts) : pricing.find(model)) else { return 0.0 };
let multiplier = if usage.speed == Some(Speed::Fast) { pricing.fast_multiplier } else { 1.0 };
calculate_cost_from_pricing(usage, pricing) * multiplier

// :118-199 分项
(c5m, c1h) = usage.cache_creation ? (ephemeral_5m_input_tokens, ephemeral_1h_input_tokens)
                                  : (cache_creation_input_tokens, 0)          // :119-127
c1h_rate       = pricing.input            * 2.0                                // :7, :128（常量 CACHE_CREATE_1H_INPUT_MULTIPLIER）
c1h_rate_above = pricing.input_above_200k * 2.0                                // :129-131

// 档位 A（有 per-model 阈值）：整单切换，:143-168
context = input + cache_read + c5m + c1h          // :147-151，全部 saturating_add
long    = context > threshold                     // 严格大于，:152
rate(base, above) = long ? (above ?? base) : base
cost = input*rate(input) + output*rate(output) + c5m*rate(cache_create)
     + c1h*rate(c1h_rate) + cache_read*rate(cache_read)

// 档位 B（无阈值，默认 200_000）：边际分段，:170-199
cost = Σ tiered_cost(tokens, base, above, 200_000)，桶为 input / output / c5m / c1h / cache_read
// tiered_cost（:201-211）：
tokens == 0                      -> 0.0                          # :202-204 早退
above 存在 && tokens > threshold -> threshold*base + (tokens-threshold)*above
否则                             -> tokens*base
```

| 语义点 | 说明 | 证据 |
| --- | --- | --- |
| 整单 vs 边际 | 有 `long_context_threshold` 时全请求所有桶走高档位价；只有 LiteLLM `*_above_200k_tokens`（无 per-model 阈值）时才是边际 | `cost.rs:133-142`、`:170-172`、`:201-211` |
| 阈值判定用的 context | 含 `input + cache_read + c5m + c1h`（缓存命中也算上下文） | `cost.rs:139-151`、测试 `:436-478` |
| 计数溢出 | 四桶求和全部用 `saturating_add`：counters 来自宽松 JSONL 解析，环绕会把合计压到阈值以下、溢出检查会直接中止（源码注释 `cost.rs:144-146`） | `cost.rs:147-151` |
| 1h 缓存写入 | `input × 2`，与 `cache_create` 单价无关 | `cost.rs:7`、`:128-131` |
| 有 `cache_creation` 明细 | `cache_creation_input_tokens` 不单独计价，避免双计 | `cost.rs:119-127` |

数值自证：`cost.rs:317-338`（5m=10、1h=20、cache_read=30 → 55.5）；`:358-434`（阈值 100，input 59/60/61 → 93e-6 / 94e-6 / 190e-6，证明整单切换 + 严格大于）；`:436-478`（真实 `grok-4.5` 价，缓存重读 500K → 0.352 vs 短上下文 0.056）。

#### 6.2 Codex 专属路径

```text
// codex/report.rs:343-392  bucket 计价（short/long 双桶）
cache_read = cache_read_explicit ? pricing.cache_read : pricing.input            // :344-348
long_input_rate = input_above_200k ?? input ; long_output_rate = output_above_200k ?? output
long_cache_read = cache_read_explicit ? (cache_read_above_200k ?? cache_read) : long_input_rate
long_cache_creation_rate = cache_creation_above_200k ?? cache_creation
cost = short_non_cached*input + short_cached*cache_read + short_cache_creation*cache_creation
     + (output - long_output)*output
     + long_non_cached*long_input_rate + long_cached*long_cache_read
     + long_cache_creation*long_cache_creation_rate + long_output*long_output_rate

// codex/report.rs:234-275  服务档按桶增量
cost = bucket_cost(total, pricing)
     + bucket_cost(flex_usage, pricing) * (flex_multiplier - 1.0)
     + bucket_cost(fast_usage, pricing) * (fast_multiplier - 1.0)
```

- long 桶在聚合阶段按**每条请求**切分：`is_long_context = event.input_tokens > long_context_split_threshold(model)`（`codex/aggregate.rs:476-487`），阈值来自 models.dev 快照，缺省 200 000（`pricing.rs:2753-2763`）。
- flex/fast 桶由 `--speed` 强制值或日志 `thread_settings.service_tier` 决定；**拼写映射在 `codex/parser.rs:649-659`**（`default`|`standard` → Standard、`fast`|`priority` → Fast、`flex` → Flex，其它值映射为 `None` 即清除已记录档位，语义见 `codex/parser.rs:465-475`，测试 `:1325-1337`）；`--speed` 枚举与各 Codex home 的 `config.toml` 解析在 `codex/speed.rs:14-44`。未分类 token 按 auto 策略归入当前档（`codex/report.rs:250-271`、`codex/src/README.md:35-39`）。
- 时间敏感模型（DeepSeek V4）先按时间戳分桶（`codex/aggregate.rs:488-509`），计价时逐桶 `find_at`（`codex/report.rs:190-204`）。

#### 6.3 fast/flex 的适用范围

- 通用路径只在 `usage.speed == Some(Speed::Fast)` 时乘 `fast_multiplier`（`cost.rs:110-115`）；`flex_multiplier` 在通用路径不参与（来源无 flex 概念）。
- Codex 用 §6.2 的桶增量形式同时支持 fast/flex。
- 倍率来源顺序见 §5.3（上游 `provider_specific_entry` 优先于内嵌覆盖表）。

#### 6.4 `extra_total_tokens` 是否参与计价

**本身不参与**，但产生它的路径可能改变金额：

```rust
// utils.rs:22-38
known = total_usage_tokens(usage) + extra_total_tokens
missing = total_tokens - known                                    // saturating
if missing == 0 → 原样返回
if usage.output_tokens == 0 → usage.output_tokens = missing       // ← 这部分会被计价
else → extra_total_tokens += missing                              // ← 不计价
```

触发点只有「日志给出 `total_tokens` 但桶之和不匹配」（Kimi、Pi 等：`kimi/src/parser.rs:208`、`pi/src/parser.rs:348`）。展示端一律计入 `totalTokens`（`types.rs:89-95`、`:115-121`、`:183-189`；`output.rs:96-109`），字段本身被 `#[serde(skip_serializing)]` 隐藏（`types.rs:106-107`、`:168-169`）。

#### 6.5 `credits`（Amp 等）如何呈现

解析：Amp 从 `usageLedger.events[]` / `messages[].usage` 读 `credits`（`amp/src/parser.rs:212`、`:320`）；Codebuff 只在 >0 时保留（`codebuff/src/loader.rs:84`）。汇总累加（`summary.rs:56-60`、`:242-247`）。呈现：summary/session JSON 有值即输出（`output.rs:51-53`、`:73-75`），Totals 只在合计 >0 时输出（`output.rs:112-115`），表格列 `Credits` 用裸 `{:.2}`（`common/src/report.rs:45`、`:70`、`:108`）。**不进入任何金额公式**。

#### 6.6 缺失价格的行为

金额直接 `0.0`（`cost.rs:99-109`），不 panic；「显式 0 价」不算缺价（测试 `cost.rs:217-237`）。

| 披露通道 | 内容 | 证据 |
| --- | --- | --- |
| `ModelBreakdown.missing_pricing` | 仅 true 时序列化 `"missingPricing": true` | `types.rs:109-111`、测试 `output.rs:737-742`、`:744-800` |
| `totals.unpricedModels` | 去重排序的模型名数组，全有价时该键缺席 | `output.rs:126-145` |
| stderr WARN | 离线：`WARN  Missing embedded pricing for {model}; cost excludes this model. Run without --offline or update ccusage after pricing is added.`；在线：`WARN  Missing pricing for {model}; cost excludes this model. Update pricing or run again after LiteLLM has the model.` | `output.rs:430-450` |
| 判定 | `missing_pricing_model_for_usage`（Display 或 Auto+有 costUSD → 不报）→ `…_for_token_total`（token=0 不报；`find` 为 None 才报） | `cost.rs:48-75` |
| Codex 变体 | `missing_pricing: mode != Display && codex_model_missing_pricing(...)` | `ccusage-adapter-all/src/loader.rs:856-859` |

注意 `missing_pricing = true` 的 breakdown 仍可能有正 cost：`summary.rs:92`（`breakdown.cost += entry.cost`）与 `summary.rs:93-95`（仅在 `entry.missing_pricing_model.is_some()` 时置标记）是两条互相独立的语句，且 `output.rs:123-125` 的文档注释明确写明 "A model can still show positive cost when other entries carried stored costs"；同一模型部分事件带 `costUSD`（走 display 分支）时会同时满足两者。

**证据**：`cost.rs:7`、`:31-211`、`:217-237`、`:317-478`、`codex/report.rs:183-392`、`codex/aggregate.rs:476-509`、`utils.rs:22-38`、`types.rs:89-121`、`:168-189`、`output.rs:51-145`、`:410-450`、`amp/src/parser.rs:212`、`:320`、`codebuff/src/loader.rs:84`、`common/src/report.rs:45-108`、`ccusage-adapter-all/src/loader.rs:856-859`、`pricing.rs:2753-2763`.

---

### 7. cost mode 语义

```rust
// ccusage-cli/src/types.rs:242-248
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CostMode { #[default] Auto, Calculate, Display }
```

- CLI 文本映射 `"auto" | "calculate" | "display"`（`ccusage-cli-parser/src/parser.rs:1136-1138`）；`SharedArgs::with_defaults()` 设 `mode: Auto`（`ccusage-cli/src/types.rs:62-71`）；config 同名枚举与 `From`（`ccusage-config/src/config.rs:703-711`）。
- 分支（`cost.rs:39-46`）：`Display` → `cost_usd.unwrap_or(0.0)`；`Auto` → `cost_usd.unwrap_or_else(自算)`；`Calculate` → 总是自算。
- **auto 的判定依据只是 `costUSD` 是否存在**：`UsageEntry.cost_usd: Option<f64>`（serde `rename = "costUSD"`，`ccusage-core/src/types.rs:14-15`），`Some(0.0)` 也走 display，不回退到 token 计算。测试 `cost.rs:239-290`、`:563-612`（含 Display/Auto/Calculate 三态同数据对照）。
- adapter 差异：Claude 在 Display 模式**不加载价格表**（`claude/src/lib.rs:144-152`）；Pi 任何模式都加载（`pi/src/lib.rs:21-27`），auto 对记录价做 `is_finite && >= 0.0` 过滤、且有 override 时强制重算（`pi/src/parser.rs:640-650`）；Codex 的 mode 只决定是否用 `costUSD`，档位由 `--speed` + 记录 tier 决定（§6.2）。
- 文档交叉验证：`docs/guide/cost-modes.md:15-29`（auto）、`:50-65`（calculate）、`:85-99`（display 缺价 `$0.00`），与源码一致。

**证据**：`ccusage-core/src/types.rs:14-15`、`ccusage-cli/src/types.rs:62-71`、`:242-248`、`ccusage-cli-parser/src/parser.rs:1136-1138`、`ccusage-config/src/config.rs:703-711`、`cost.rs:31-46`、`:239-290`、`:563-612`、`claude/src/lib.rs:144-152`、`pi/src/lib.rs:21-27`、`pi/src/parser.rs:618-650`、`docs/guide/cost-modes.md:15-99`.

---

### 8. 时间相关定价

只有 DeepSeek V4 两个模型有时间维度定价，且是硬编码调度，不是通用「生效区间表」。

```rust
// pricing.rs:252
const DEEPSEEK_V4_PRICING_CUTOFF_MS: i64 = 1_786_896_000_000;   // = 2026-08-16T16:00:00Z
// pricing.rs:271-277
pub fn has_time_dependent_pricing(model: &str) -> bool {
    let resolved = model_aliases::resolve_model_name(model);
    deepseek_v4_model_identity(model).or_else(|| deepseek_v4_model_identity(&resolved)).is_some()
}
// deepseek_v4_model_identity（:262-269）只认归一化后的 "deepseek-v4-flash" / "deepseek-v4-pro"
```

| 时段 | 判定 | flash in/out | pro in/out |
| --- | --- | --- | --- |
| `old` | `ts < CUTOFF` | 0.14e-6 / 0.28e-6 | 0.435e-6 / 0.87e-6 |
| `off_peak` | `ts >= CUTOFF` 且非高峰 | 0.22e-6 / 0.66e-6 | 0.66e-6 / 1.98e-6 |
| `peak` | `ts >= CUTOFF` 且命中高峰窗口 | 0.44e-6 / 1.32e-6 | 1.32e-6 / 3.96e-6 |

（`pricing.rs:279-331`）高峰窗口（`pricing.rs:333-347`）：按 **UTC**（注释强调用 epoch 而非展示时区，`:334-335`），周六周日恒 off-peak，工作日小时落在 `[1,4)` 或 `[6,10)`（半开区间，结束整点不含）为 peak。

应用与叠加（`pricing.rs:349-368`、`:1461-1482`）：

```text
find_at(model, ts):
 1) 非 DeepSeek V4 → 直接 find(model)
 2) find(model) 取静态价 → apply_deepseek_v4_schedule 覆盖 input/output/cache_create/cache_read，
    并把四个 *_above_200k 也设为同价、cache_*_explicit 置真（防止 override 缩放误伤）
 3) 命中 user_overrides（原名 / 别名 / 调度后名字）→ apply_explicit_pricing_override（pricing.rs:370-403）
    只逐字段覆盖 override 中**显式给出**的项，其余保留调度结果 ⇒ 调度价与用户字段是字段级叠加，而非整条替换
```

调用点：`cost.rs:102-106`（有事件时间戳用 `find_at`）、`cost.rs:9-18`（`calculate_cost` 从 `data.timestamp` 解析）、`codex/aggregate.rs:488-509`、`:568`（按时间戳分桶）、`codex/report.rs:190-204`（逐桶计价）。

测试：`pricing.rs:2989-3033`、`:3035-3057`（UTC 工作日半开窗口与偏移无关）、`:3059-3085`、`:3087-3100`、`:3114-3136`（带 provider 前缀的第三方路由名不参与调度）、`:3138-3164`、`:3166-3180`、`:3182-3227`（用户显式 override 仍最高优先）；金额侧 `cost.rs:523-561`、`:563-612`、`:614-663`（无时间戳的旧 API 保静态价）。

**证据**：`pricing.rs:252-368`、`:1461-1482`、`cost.rs:9-18`、`:102-106`、`:523-663`、`codex/aggregate.rs:488-509`、`codex/report.rs:190-204`.

---

### 9. 缓存与性能

```rust
// pricing.rs:413-441
pub struct PricingMap {
    entries: FxHashMap<String, Pricing>,               // rustc_hash::FxHashMap（fast.rs:4）
    user_overrides: FxHashMap<String, PricingOverride>,
    exact_only: ExactOnlyKeys,                         // raw: FxHashSet + normalized: FxHashMap
    context_limits: FxHashMap<String, u64>,
    builtin_context_limits: FxHashMap<String, u64>,
    enable_models_dev_fallback: bool,
    enable_embedded_models_dev_fallback: bool,
    find_cache: OnceLock<Mutex<FxHashMap<String, Option<Pricing>>>>,
    normalized_keys: OnceLock<FxHashMap<String, Option<String>>>,
}
```

- 不是 `Rc`/`Arc`；`Pricing` 是 `Copy`（`:196`），adapter 以 `&PricingMap` 借用。

| 数据 | 生命周期 | 证据 |
| --- | --- | --- |
| 三份内嵌快照 inflate | `OnceLock<String>`，首次使用才解压，永久 | `pricing.rs:33-54` |
| 内嵌 models.dev → `PricingMap` | `OnceLock<PricingMap>` | `pricing.rs:2825-2833` |
| 在线 models.dev → `PricingMap` | `ModelsDevPricingCache`：成功永久缓存；失败记 `Instant` 并退避 60s | `pricing.rs:792-832`、`:58` |
| 在线 LiteLLM → 内存 | **无跨调用缓存**：每次非 offline 加载都会请求（可命中 HTTP 304 磁盘缓存） | `pricing.rs:1007-1028`、`http.rs:69-99` |
| 单次查询结果 | `find_cache`（含 miss），装载/补档位/应用 override 后被清 | `pricing.rs:1354-1391`、`:1917-1923` |

`clear_find_cache`（`:1917-1923`）同时清 `find_cache` 与 `normalized_keys`；调用点 `:1106`、`:1350`、`:1817`、`:1977`、`:1982`、`:2007`。`find` 的并发形状是「锁内读 → 锁外算 → 锁内写回」（`:1354-1391`），因此并发下同一名字可能被算两次，但结果幂等。

加载成本与日志：每次装载都会重新 serde 解析两份 speed override JSON（`:1041-1045`、`:1112-1115`，文件仅 356 / 474 B）；models.dev 装载按信任等级（`OWNER=3`/`PLATFORM=2`/`RESELLER=1`，`:649-651`）→ 声明 id → map key 排序 provider（`:1126-1132`），并按 key 排序 catalog 内模型（`:1208-1209`）以保证可复现；替换落败拼写时 `remove` 旧条目并清其上下文上限（`:1277-1284`、`:1328-1330`）。**没有价格加载耗时/条数的 info 日志**，唯一反馈是 TTY 进度文案 `Refreshing model pricing from LiteLLM...`，其开关就在调用点 `pricing.rs:1009`（`log && crate::progress::usage_load_output_is_tty()`），渲染与测试见 `progress.rs:308-319`；刷新类 WARN 亦受 `should_log_pricing_refresh_details() = log_level() >= 4` 门控（`:2810-2812`）。

**证据**：`pricing.rs:33-58`、`:413-441`、`:649-651`、`:792-832`、`:1002-1038`、`:1041-1045`、`:1106`、`:1126-1132`、`:1277-1284`、`:1328-1330`、`:1350`、`:1354-1391`、`:1917-1923`、`:2825-2833`、`fast.rs:4`、`progress.rs:308-319`.

---

### 10. 离线 / 在线

- 开关：`-O`/`--offline` → `shared.offline = true`（`ccusage-cli-parser/src/parser.rs:766`）；`--no-offline` → `shared.no_offline = true`（`:767`）；合成规则 `offline: args.offline && !args.no_offline`（`ccusage/src/commands/mod.rs:380`），因此 `--no-offline` 可覆盖配置里的 `"offline": true`。statusline 默认离线（`StatuslineArgs::default()` 中 `offline: true`，`ccusage-cli/src/types.rs:221-239`；文档 `docs/guide/statusline.md:59`）。
- 断网效果（`pricing.rs:1002-1038`）：跳过 LiteLLM 抓取（`:1007`）；`enable_models_dev_fallback = false`（`:1035`）；内嵌 models.dev 仍启用（`:998`，测试 `:3290-3304`、`:4914-4929`）；用户 override 照常应用（`:1036`）。文档口径一致：`--offline` 只控制数据源，`pricingOverrides` 控制具体条目（`docs/guide/config-files.md:455-462`）。
- 失败/超时/重试：

| 环节 | 行为 | 证据 |
| --- | --- | --- |
| LiteLLM 传输/状态错误 | `Err` → WARN（log≥4）→ 用已有表，不重试 | `pricing.rs:1021-1027`、`http.rs:100-102` |
| 响应形状不合法 | `validates_shape` 失败 → `Err` | `http.rs:121-126`、`pricing.rs:88-98` |
| 体积 > 64 MiB | `Err` | `http.rs:12`、`:104-151` |
| 全局超时 | 10 秒 | `http.rs:11`、`:75-78` |
| 304 但缓存体不合法 | 作废缓存并**重取一次** | `http.rs:88-99` |
| models.dev 失败 | WARN + 60 秒退避 | `pricing.rs:58`、`:814-818`、`:2839-2849` |
| 未安装 HTTP 客户端 | `no HTTP client installed for pricing refresh`，等价离线 | `pricing.rs:2886-2893` |
| 未知端点 URL | `unsupported pricing URL` | `http.rs:26-31` |

- 版本/日期戳：两套内嵌快照 JSON **没有任何版本或生成日期字段**（本次在 `models-dev-pricing.json` 34 269 行与 `models-dev-catalog-rules.json` 中检索 `generated|generatedAt|version|date|updated|_meta` 零命中）；运行期也无「快照年龄」校验。可追溯性只靠 git 提交与 `flake.lock` 的两个 pin（§1.1）。在线端点同样无版本标识（LiteLLM 抓 `main`，models.dev 抓 `api.json`）。

**证据**：`ccusage-cli-parser/src/parser.rs:766-767`、`ccusage/src/commands/mod.rs:380`、`ccusage-cli/src/types.rs:221-239`、`pricing.rs:998`、`:1002-1038`、`:2814-2861`、`http.rs:11-151`、`flake.lock:143-145`、`:159-161`.

---

### 11. 数字格式化

```rust
// output.rs:548-550
pub fn format_currency(value: f64) -> String { format!("${value:.2}") }
// output.rs:452-462
pub fn json_float(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0
        && value >= i64::MIN as f64 && value <= i64::MAX as f64
    { json!(value as i64) } else { json!(value) }
}
```

- `format_currency`：固定两位小数 + `$` 前缀；**没有** `< 0.01` 的特殊处理（0.004 → `$0.00`），无千分位，负数会得到 `$-1.00`；舍入由 Rust 标准库浮点格式化完成。
- `format_number`（`:536-546`）：整数千分位，用于 token 数。
- `credits` 列用裸 `{:.2}`（`adapters/common/src/report.rs:108`）。
- `json_float` 让整数金额在 JSON 中保持整数形态（`42` 而非 `42.0`），用于 Codex `costUSD`（`codex/report.rs:179`）、adapter-all `totalCost`（`ccusage-adapter-all/src/report.rs:169`、`:196`）、Amp credits（`ccusage-adapter-all/src/loader.rs:813`）。
- `--no-cost` 走 `strip_cost_json` 递归删除 `totalCost` / `costUSD` / `cost`（`output.rs:564-579`）。

**证据**：`output.rs:452-462`、`:536-550`、`:564-579`、`codex/report.rs:179`、`ccusage-adapter-all/src/report.rs:169`、`:196`、`ccusage-adapter-all/src/loader.rs:813`、`common/src/report.rs:108`.

---

### 未验证 / 存疑

> **本节状态（第 8 轮实测后）**：§13.5 的对照表已逐条标出本节哪些条目被闭环（内嵌键集、models.dev 生成器、在线目录对比、格式化舍入等）与被部分闭环的边界；第 5 条（并发压力）与第 11、12 条（兜底表写入意图、Codex 未分类 token）仍开放。

1. **内嵌 LiteLLM 快照的键集未直接检视**：`$OUT_DIR/litellm-pricing.json.deflate` 是编译产物，当时（第 1–7 轮）遵守「只读、不构建、不运行」约束未生成它；`claude-sonnet-4-20250514`、`claude-opus-4-6-fast` 等在该快照中的存在性结论来自仓库内测试断言（如 `pricing.rs:4931-4955`）与算法推演。**（第 8 轮已闭环：见 §13.5——构建产物被解压后与重放键集逐键逐值 0 差异，534 键。）**
2. **`format_currency` 舍入模式未实验**：`{:.2}` 对 `x.xx5` 的边界行为未在测试中断言，本文只描述为「Rust 标准库浮点格式化」。
3. **models.dev 在线结果与内嵌快照的一致性**依赖生成期规则逐条对齐；本次核对了规则代码（`pricing.rs:653-744`）与若干快照条目，未做 34K 行全量比对，也未审阅 `nix/tools/models-dev-gen` 生成器的实现细节（仅确认其测试文件存在）。
4. **并发语义未压力验证**：`find_cache` 的「锁外计算 + 锁内写回」与 `clear_find_cache` 的交互只在单线程测试覆盖。
5. **1 小时缓存写入 `input × 2`** 取自常量与注释（`cost.rs:7`、`:128-131`）及文档 `docs/guide/cost-modes.md:198-205`，未与厂商定价页交叉验证。
6. **DeepSeek V4 三段价与 UTC 高峰窗口**为硬编码（`pricing.rs:279-347`），本文只核对代码与测试期望值；`CUTOFF` 的 UTC 时刻由本机 PowerShell `[DateTimeOffset]::FromUnixTimeMilliseconds(1786896000000)` 换算为 `2026-08-16T16:00:00Z`。
7. **刷新与删除语义**：LiteLLM 刷新路径只有 `insert`（`pricing.rs:1080-1100`），未见 `remove`，被上游移除的模型可能继续以旧价出现；models.dev 装载内部确实会 `remove` 落败拼写的旧条目（`:1277-1284`），但那是同一份源内部的替换，不是「上游下线」的清理。结论基于全文 grep，未做运行时验证。
8. **statusline 路径的 offline 默认值与 `--no-offline` 合成规则**未逐行追踪（该路径另有按需加载的 `find_statusline_hook_context`，`ccusage/src/commands/mod.rs:716-765`）。
9. **各 agent 的模型名字形态差异**（是否已带 `-fast`、是否带 provider 前缀）会显著改变命中结果；本文只对 Claude / Codex / Pi 三条路径逐行核对，其余 adapter 仅确认调用同一套 `load_with_overrides` + `calculate_cost*`。
10. **`credits` 是否应计入「总额」**在任何地方都不成立（代码中只做展示与聚合），但各 adapter 的 credits 语义（Amp 与 Copilot 的定义是否一致）未逐一核对。
11. **兜底表的三处无条件写入**（`gpt-5.1-codex`、`gpt-5.2-codex`、`gpt-5.2`，`pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`）与 `:2010-2014` 的注释意图相反，源码未给出解释，无法判定是刻意锁定这三项还是遗漏；且 `gpt-5.1-codex` 这一处未套 flex 倍率，而 `gpt-5.2` 套了。
12. **Codex 服务档的「未分类 token」归属**：`codex/report.rs:250-271` 的 auto 分支把未分类用量并入当前档，其行为依赖各 Codex home 的 `config.toml` 探测（`codex/speed.rs:33-44`）；本机无 Codex 环境，未做端到端验证。

## 9. 配置、CLI 解析与输出契约


研究用只读样本（**禁止修改**）：

- 主仓（Rust，v20.0.31）：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（`chore: release v20.0.31`）。
- 对照仓（TypeScript，v19.0.3）：`%TEMP%\ccusage-research\ccusage-v19`，HEAD `4aedac0622232f4b8e93650e9b7b54535ef9cf2b`，tag `v19.0.3`。

行号约定：Rust 侧证据写成 `path:line`（相对主仓根）；v19 侧证据写成 `ccusage-v19/path:line`。下文所有行号均为实际行号（read/grep 实测）。
**本章裸文件名约定**：未写全路径的 `parser.rs`/`help.rs`/`help_codegen.rs`/`arg_parser.rs` 指 `rust/crates/ccusage-cli-parser/src/`；`config.rs`/`config_schema.rs` 指 `rust/crates/ccusage-config/src/`；`output.rs`/`types.rs`（core 语境）/`agent_report.rs` 指 `rust/crates/ccusage-core/src/`；`http.rs`/`cli.rs`/`commands/mod.rs` 指 `rust/crates/ccusage/src/`；`types.rs`（CLI 语境）指 `rust/crates/ccusage-cli/src/types.rs`。

---

### 1. 配置文件体系

#### 1.1 交付物与命名

v20 的配置文件仍叫 `ccusage.json`，但**由 Rust 生成 JSON Schema**，且 TS 侧在 v20 只剩下发 npm 包的分发壳。

- Schema 文件：`apps/ccusage/config-schema.json`（14415 行，仓库内提交）。
- 分发副本：`docs/public/config-schema.json`（文档站构建前从上面那份拷贝）。

证据：`docs/package.json:8`（`cp ../apps/ccusage/config-schema.json public/config-schema.json`）；`docs/README.md:13-14`；`nix/treefmt.nix:57-61`（treefmt 会把生成结果同时刷到两处）；一致性由 `nix/checks.nix:90-111` 强制（生成结果与提交文件 diff 不一致即失败，报 "apps/ccusage/config-schema.json is out of sync with the Rust schema source."）。

示例配置：`ccusage.example.json:1-39`，顶层是 `$schema` / `defaults` / `commands`，schema 指向 `./apps/ccusage/config-schema.json`（`ccusage.example.json:2`）。`nix/checks.nix:177` 用 `check-jsonschema` 校验该示例。

#### 1.2 查找路径与优先级

查找路径由 `discover_config_paths()` 生成，顺序即优先级，**第一个能被读取且能解析成 JSON object 的文件获胜**（不做多文件合并）。

```rust
// rust/crates/ccusage-config/src/config.rs:258-269
fn discover_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        paths.push(cwd.join(".ccusage").join("ccusage.json"));
    }
    paths.extend(claude_config_dirs().into_iter().map(|dir| dir.join("ccusage.json")));
    paths
}
```

用户级目录来自 `claude_config_dirs()`：有 `CLAUDE_CONFIG_DIR` 时按逗号切分（每个都 trim、丢空项），否则退回 `~/.config/claude` 与 `~/.claude` 两个目录。

- 证据：`rust/crates/ccusage-config/src/config.rs:271-283`。
- 因此完整顺序是：`$CWD/.ccusage/ccusage.json` → (`$CLAUDE_CONFIG_DIR/*/ccusage.json` 或 `~/.config/claude/ccusage.json` → `~/.claude/ccusage.json`)。
- 取首个可用者：`rust/crates/ccusage-config/src/config.rs:246-256`（`read_to_string` + `serde_json::from_str` 双双失败的会被静默跳过，最后 `.find(|value| value.as_object().is_some())`）。

显式路径：`--config <path>` / `--config=<path>`。它在参数扫描阶段被取出，不参与普通选项解析：

- 证据：`rust/crates/ccusage-config/src/config.rs:285-299`（`scan_config_path`，同时支持 `=` 与分离写法，空值不认）。

**没有 `CCUSAGE_CONFIG` 环境变量**。源码层面（`env::var` / `env::var_os` 读取）`CCUSAGE_*` 只有三个：`CCUSAGE_VERSION`、`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_MODEL_ALIASES`。文档层面另有一批**两版源码均无实现**的变量名：`CCUSAGE_OFFLINE`（`docs/guide/environment-variables.md:124`、`:129`、`:209`）、`CCUSAGE_TIMEZONE`（`docs/guide/configuration.md:38`）、`CCUSAGE_GROK_PATH`（`docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24`、`:51`）——最后这个只是设计文档里被点名要移除的旧路径来源，同一文档明确要求只保留 `GROK_HOME` 与 `~/.grok`（该文 `:35`、`:51-52`、`:59-60`、`:83`），v20 实际实现即 `rust/adapters/grok/src/paths.rs:9` 的 `GROK_HOME`。也没有 XDG 参与**配置查找**：`config.rs` 不读 `XDG_CONFIG_HOME`。XDG 变量在别处出现三处：Claude 数据目录发现（`rust/adapters/claude/src/paths.rs:46`，`XDG_CONFIG_HOME`）、HTTP 缓存根（`rust/crates/ccusage/src/http.rs:39`，`XDG_CACHE_HOME`）与 OpenCode 数据根（`rust/adapters/opencode/src/paths.rs:6`、`:29`，`XDG_DATA_HOME`）。

文档侧口径与实现一致但有冗余描述（把「用户配置/遗留配置」拆成两条优先级）：`docs/guide/configuration.md:16-24`。

#### 1.3 配置结构（有哪些设置）

`CcusageConfig` 顶层字段：`$schema`、`defaults`、`commands`，外加每个 agent 一个命名空间（`claude`、`codex`、`opencode`、`amp`、`droid`、`codebuff`、`hermes`、`pi`、`goose`、`openclaw`、`kilo`、`copilot`、`gemini`、`antigravity`、`kimi`、`qwen`、`grok`、`zcode`），全部 `Option`，全部 camelCase。

证据：`rust/crates/ccusage-config/src/config_schema.rs:13-59`（`#[serde(rename_all = "camelCase")]` 在第 14 行）；每个 agent 的 `{ defaults, commands }` 形状见 `config_schema.rs:72-359`；根 `commands` 只允许 `daily/weekly/monthly/session/blocks/statusline`（`config_schema.rs:61-70`）。

各类设置的落点：

| 类别 | 配置键 | 作用对象 | 证据 |
| --- | --- | --- | --- |
| 日期与输出通用项 | `since` `until` `json` `mode` `order` `breakdown` `offline` `noOffline` `color` `noColor` `timezone` `jq` `compact` `singleThread` `noCost` `debug` `debugSamples` `all` `pricingOverrides` | 所有报告 | `config_schema.rs:358-399` |
| 日报专有 | `instances` `project` `projectAliases` | `daily` / `claude daily` | `config_schema.rs:425-434` |
| 周报专有 | `startOfWeek` | `weekly` | `config_schema.rs:445-450` |
| blocks 专有 | `active` `recent` `tokenLimit` `sessionLength` | `blocks` | `config_schema.rs:461-472` |
| statusline 专有 | `offline` `noOffline` `visualBurnRate` `costSource` `cache` `noCache` `refreshInterval` `contextLowThreshold` `contextMediumThreshold` `timezone` `debug` `modelLabelAliases` `pricingOverrides` | `statusline` | `config_schema.rs:481-510` |
| Codex 专有 | `speed` | `codex` | `config_schema.rs:512-519` |
| pi 专有 | `piPath`、`pi.stores[]`（`name`+`path`） | `pi` | `config_schema.rs:521-528`、`config_schema.rs:183-209` |
| OpenClaw 专有 | `openClawPath` | `openclaw` | `config_schema.rs:530-537` |
| 自定义价格 | `pricingOverrides.<model>.*`（11 个 cost 字段） | 全局 / statusline | `config_schema.rs:593-607` |
| 枚举取值 | `mode`=auto/calculate/display；`order`=desc/asc；`startOfWeek`=sunday..saturday；`speed`=auto/standard/fast/flex；`visualBurnRate`=off/emoji/text/emoji-text（kebab-case）；`costSource`=auto/ccusage/cc/both | — | `config_schema.rs:539-591` |

没有独立的「默认 agent 选择」键：agent 由命令位置决定（`ccusage codex daily`），配置只提供各 agent 段的默认值。

#### 1.4 配置应用顺序（覆盖规则）

`option_maps()` 按固定顺序收集「要应用的 option map」，后收集的覆盖先收集的：

```
根 defaults
→ commands.<原始命令>（如 commands.daily / commands."codex daily"）
→ commands.<report>（仅当命令带 agent，如 commands.daily）
→ commands."<agent>:<report>"（如 commands."codex:daily"）
→ <agent>.defaults
→ <agent>.commands.<report>
```

证据：`rust/crates/ccusage-config/src/config.rs:92-132`；命令名拆解（`codex:daily` 与 `codex daily` 两种写法归一）见 `config.rs:301-334`，其中 `is_agent_command`（`config.rs:395-397`）与 `is_report_command`（`config.rs:399-404`）。

应用进具体参数结构：`apply_config_to_shared`（`config.rs:406-413`）、`apply_config_to_daily_args`（`config.rs:415-428`）、`weekly`（`config.rs:430-436`）、`blocks`（`config.rs:438-454`）、`statusline`（`config.rs:456-505`）、agent 级（`config.rs:507-529`，目前只处理 `speed` / `piPath` / `openClawPath`）。对外接口是 `cli` 侧定义的 trait：

- `rust/crates/ccusage-cli/src/types.rs:299-321`（`CliConfig` 六个钩子 + `config_error` 默认 `None`）；`NoConfig` 空实现 `types.rs:323-325`。

CLI 侧只对「有该字段的键」赋值，所以配置文件永远不能覆盖默认值以外的语义冲突：例如 `order` 一旦来自配置就会把 `order_explicit` 置真（`config.rs:588-591`），这与 CLI 的同名行为一致（`rust/crates/ccusage-cli-parser/src/parser.rs:761-764`）。

`pricingOverrides` 是**逐字段合并**而非整条替换：`merge_pricing_overrides`（`config.rs:627-635`）与 `merge_override_fields`（`config.rs:637-681`）只覆盖 source 里 `Some` 的字段；`ConfigPricingOverride → PricingOverride` 的字段全量映射在 `config.rs:683-701`。

#### 1.5 配置文件里的取值校验

配置绕过了 CLI 解析器，因此校验被单独实现一次：

- 日期：`ConfigContext::detect_date_bound_error`（`config.rs:69-90`），非字符串报 `since must be a string. Expected YYYY-MM-DD or YYYYMMDD`，非法日期报 `since 'x' is not a valid date. Expected YYYY-MM-DD or YYYYMMDD`；两者都被 `config_error()` 包成 `Invalid ccusage config: ...`（`config.rs:198-200`），并在解析早期直接 `return Err`（`parser.rs:90-92`）。
- 时区：配置里的 `timezone` 与 `--timezone` 走同一条校验（见本章第 3 节，`rust/crates/ccusage/src/cli/timezone.rs:130-150` 有专门测试「配置时区被合法 flag 覆盖后放行」）。
- `pi.stores`：数组形状、`name`/`path` 必填、`path` 非空、name 匹配 `^[a-z][a-z0-9_-]{0,31}$`（`config_schema.rs:11`）、不得与内置 agent 名冲突、不得重名；错误以 `Invalid ccusage config: pi.stores[i] ...` 形式暴露且**只对会读 pi stores 的命令生效**（`daily/weekly/monthly/session`，`config.rs:134-141`、`config.rs:143-196`、`config.rs:57-63`）。

#### 1.6 JSON Schema 的生成方式

Rust 侧用 `schemars` 从结构体派生，再手工增强：

- 派生入口：`CcusageConfig` 上的 `#[derive(JsonSchema)]`（`config_schema.rs:13`），其余结构同（例如 `config_schema.rs:358-360`）。
- 生成函数：`generate_config_schema_json()`（`config_schema.rs:711-788`），期间调用 `enrich_schema`（`config_schema.rs:856-878`）、`add_schema_defaults`（`config_schema.rs:880-920`，含 `set_definition_defaults` `config_schema.rs:922-939`）、`inline_schema_references`（`config_schema.rs:941-1020`，`inline_schema_value`/`inline_ref`/`inline_all_of`/`merge_schema_object`）、`wrap_root_schema`（`config_schema.rs:1021-1047`）。
- 缩进用 **tab**：`tab_indent_json`（`config_schema.rs:789-804`），与仓库 `.editorconfig`/示例文件一致。
- 生成可执行体：`rust/crates/ccusage-config/src/bin/generate_config_schema.rs:3-15`（无参写 stdout，带参写文件，失败 `exit(1)`）。
- 提交产物的刷新命令：`apps/ccusage/justfile:25-27`（`cargo run -p ccusage-config --bin generate-config-schema -- config-schema.json` → `oxfmt --write` → 拷到 `docs/public/`）。
- Cargo 目标声明：`rust/crates/ccusage-config/Cargo.toml:11`；库再导出：`rust/crates/ccusage-config/src/lib.rs:10`。
- Nix 侧有 `nix run .#generate-schema` 与 treefmt 钩子（`nix/treefmt.nix:57-61`、`nix/treefmt.nix:221`）。

配置文件字段的运行时读取**不**依赖 schema：`SharedOptions::from_map` 等一组手写取值器按 camelCase 键名逐字段取（`config_schema.rs:609-634`、`635-644`、`645-652`、`653-663`、`664-683`、`684-692`、`693-701`、`702-710`），底层用 `string_option` / `bool_option` / `usize_option` / `u64_option` / `f64_option` / `enum_option` / `pricing_override_map_option` / `hashmap_option`（`config_schema.rs:805-855`）。也就是说：**schema 只是给编辑器/CI 用，运行时不做严格类型拒绝**，非法类型会被静默忽略（例如 `instances: "yes"` 等于没写）。

#### 1.7 内置默认值一览

配置字段几乎都是 `Option`，真正的默认值定义在 CLI 侧：

| 参数 | 默认值 | 证据 |
| --- | --- | --- |
| `mode` | `auto` | `rust/crates/ccusage-cli/src/types.rs:62-71` |
| `debug_samples` | `5` | 同上 |
| `order` | `asc` | 同上 |
| `since/until/last/json/breakdown/offline/color/jq/compact/single_thread/no_cost/timezone` | 全部空/false/None | `types.rs:34-60` |
| `blocks.session_length` | `5.0` 小时 | `rust/crates/ccusage-core/src/lib.rs:59`（`DEFAULT_SESSION_DURATION_HOURS`），注入点 `rust/crates/ccusage/src/cli.rs:19-24` |
| `blocks.recent` 的 lookback | `3` 天 | `rust/crates/ccusage-core/src/lib.rs:60`（`DEFAULT_RECENT_DAYS`），用于 `rust/crates/ccusage/src/commands/mod.rs:573`、`659` |
| 紧凑布局阈值 | 100 列 | `rust/crates/ccusage-core/src/lib.rs:61`（`USAGE_COMPACT_WIDTH_THRESHOLD`） |
| `statusline.offline` | `true` | `rust/crates/ccusage-cli/src/types.rs:221-240` |
| `statusline.cache` | `true` | 同上 |
| `statusline.refresh_interval` | `1`（秒） | 同上 |
| `statusline.context_low_threshold` / `medium` | `50` / `80` | 同上 |
| `statusline.visual_burn_rate` | `off` | 同上 |
| `statusline.cost_source` | `auto` | 同上 |
| `weekly.start_of_week` | `sunday`（`ccusage claude weekly`） | `rust/crates/ccusage-cli-parser/src/parser.rs:520-524` |
| 统一周报（非 `claude weekly`）的周起点 | `monday` | `rust/crates/ccusage/src/cli/last_window.rs:60-62`（`UNIFIED_WEEK_START`）、`rust/crates/ccusage-adapter-all/src/loader.rs:897` |

---

### 2. CLI 解析

#### 2.1 结论：自研 parser，不用 clap

`rust/crates/ccusage-cli-parser/src/` 是手写解析器：`parser.rs`（1247 行，命令/选项语义）+ `arg_parser.rs`（104 行，词法切分）。整个 crate 不依赖 clap/argh 之类框架（`Cargo.toml` 里没有相关依赖；解析全程只做 `Vec<String>` 索引游走）。

- 入口：`Cli::parse_from_with_config(args, config, default_session_duration_hours, version)`（`parser.rs:65-133`），`args.into_iter().skip(1)` 丢掉程序名（`parser.rs:74`）。
- 词法层：`ArgParser`（`arg_parser.rs:3-7`）提供 `peek` / `peek_name` / `next` / `next_flag` / `value_for`。
- `--flag=value` 与 `--flag value` 都支持；`--flag=`（空值）报 `Missing value for <flag>`；值以 `-` 开头同样报 `Missing value for <flag>`（`arg_parser.rs:59-73`）。`daily=bad` 这种「位置参数带等号」报 `Expected option, got 'daily=bad'`（`arg_parser.rs:40-57`，测试 `arg_parser.rs:95-103`）。
- 不支持短选项合并（`-jO` 会被当成未知 flag），也不支持 `--` 之后的裸参数透传。
- 非 UTF-8 参数：`Arguments must be valid UTF-8`（`arg_parser.rs:10-23`，`rust/crates/ccusage/src/cli.rs:41-51` 也做了一次同样校验）。

#### 2.2 选项命名规则与清单

命名规则：长选项 kebab-case（`--project-aliases`、`--start-of-week`、`--debug-samples`、`--no-cost`）；短选项**单字符且不冲突**；`--no-*` 系列（`--no-offline`、`--no-color`、`--no-cost`、statusline 的 `--no-cache`）显式存在，而不是靠否定前缀隐式实现。

共享选项（`is_shared_flag` 白名单，全部命令可用）：

| 短 | 长 | 取值 | 证据 |
| --- | --- | --- | --- |
| `-s` | `--since` | 日期 | `parser.rs:745-747` |
| `-u` | `--until` | 日期 | `parser.rs:748-750` |
| — | `--last` | 正整数 | `parser.rs:751` |
| `-j` | `--json` | 开关 | `parser.rs:752` |
| `-m` | `--mode` | auto/calculate/display | `parser.rs:753` |
| `-d` | `--debug` | 开关 | `parser.rs:754` |
| — | `--debug-samples` | u64 | `parser.rs:755-760` |
| `-o` | `--order` | desc/asc | `parser.rs:761-764` |
| `-b` | `--breakdown` | 开关 | `parser.rs:765` |
| `-O` | `--offline` / `--no-offline` | 开关 | `parser.rs:766-767` |
| — | `--color` / `--no-color` | 开关 | `parser.rs:768-769` |
| `-z` | `--timezone` | 字符串 | `parser.rs:770` |
| `-q` | `--jq` | 字符串 | `parser.rs:771` |
| — | `--config` | 路径 | `parser.rs:772` |
| — | `--compact` | 开关 | `parser.rs:773` |
| — | `--single-thread` | 开关 | `parser.rs:774` |
| — | `--no-cost` | 开关 | `parser.rs:775` |

命令专有选项：

- `daily`（统一与 `claude daily`）：`-i/--instances`、`-p/--project`、`--project-aliases`（`parser.rs:492-497`）。
- `weekly`（`claude weekly`）：`-w/--start-of-week`（`parser.rs:530-532`）。
- `session`：`-i/--id`（`parser.rs:449`、`parser.rs:550`）。
- `blocks`：`-a/--active`、`-r/--recent`、`-t/--token-limit`、`-n/--session-length`（`parser.rs:202-215`；默认 `session_length` 由 `DEFAULT_SESSION_DURATION_HOURS` 注入，`parser.rs:190-196`）。
- `statusline`：`-O/--offline`、`--no-offline`、`-B/--visual-burn-rate`、`--cost-source`、`--cache`、`--no-cache`、`--refresh-interval`、`--context-low-threshold`、`--context-medium-threshold`、`-z/--timezone`、`--config`、`--debug`（`parser.rs:224-260`）。注意 statusline 走**独立分支**、不消费共享表（它只从根选项继承 `timezone`，见 `parser.rs:220-222`），因此 `--json` 之类对它无效并会报 `Unknown statusline option '--json'`。
- `codex`：`--speed`、`-i/--id`（仅 session 报告，`parser.rs:618-624`）。
- `pi`：`--pi-path`（`parser.rs:652`）；`openclaw`：`--open-claw-path`（`parser.rs:682`）。
- 根级「统一报告」选项（只在命令之前、或 daily/monthly/weekly/session 之后有效）：`--all`、`--sections <d,w,m,s>`、`--by-agent`（`parser.rs:383-403`）。

#### 2.3 全局选项与子命令的继承

解析分两段：先解析「命令之前」的 root 选项（`parser.rs:96-107`），再进入 `parse_command`，把已解析的 `shared.clone()` 交给命令分支（`parser.rs:114-121`）。命令内部再解析共享选项时是**就地覆盖**（`parse_shared_arg_for_command` → `parse_shared_arg`，`parser.rs:729-741`）。

- 关键后果：`ccusage --timezone UTC daily --timezone Asia/Tokyo` 以命令内的为准；`--timezone` 校验只检查「最终生效值」（`rust/crates/ccusage/src/cli/timezone.rs:118-127` 的测试断言）。
- 根级专用选项在不适用的命令上会报错：`accepts_root_all_options` 只认 `daily|monthly|weekly|session`（`parser.rs:366-368`），否则报 `Unknown option '--sections'`（以首个使用到的 flag 名回显，`parser.rs:27-38`、`parser.rs:160-165`）。
- 旧式写法被显式迁移并报错：`claude:daily` → 自动改写为 `claude daily`（`normalize_legacy_agent_command_args`，`parser.rs:811-822`）；而 `--daily`/`--weekly` 等报告 flag 报 `Report flags like --daily are not supported. Use "ccusage daily" instead.`（`parser.rs:828-839`）；`--agent`/`-a` 过滤器报 `Agent filters like --agent are not supported. Use "ccusage <agent> <report>"...`（`parser.rs:841-855`），其中 `blocks -a`（active）被豁免（`parser.rs:842`、`parser.rs:857-866`）。
- 命令集合：`daily|monthly|weekly|session|blocks|statusline` + 18 个 agent（`claude`、`codex`、`opencode`、`amp`、`droid`、`codebuff`、`hermes`、`pi`、`goose`、`openclaw`、`kilo`、`copilot`、`gemini`、`antigravity`、`kimi`、`qwen`、`grok`、`zcode`），见 `parser.rs:781-809` 与 `parser.rs:166-363`。
- 每个 agent 支持的 report 集合不同：`claude` 全 6 种，`codex` 只有 daily/monthly/session，`opencode` 有 daily/weekly/monthly/session，其余 15 个 agent 是 daily/monthly/session（`parser.rs:973-987`）。

#### 2.4 分层帮助与 `help_codegen`

`ccusage daily --help` 这类分层帮助是**真实现**，且帮助文本是构建期从 JSON 生成的：

- `help.rs` 用 `include!(concat!(env!("OUT_DIR"), "/cli-help.rs"))` 引入生成代码（`help.rs:13`）。
- `--help`/`-h` 在任何语义校验之前拦截（`control_arg`，`parser.rs:136-150`；测试 `parser.rs:1232-1246` 断言 `--help` 优先于 `--daily` 之类的错误，`--version` 又优先于 `--help`）。
- 帮助页按命令 token 序列**精确匹配**：`find_help_page` 比对 `page.path` 与 `command_tokens(args)`（`help.rs:59-68`；上一层 `help_text_for_tokens` 在 `help.rs:55-57`，`help_text_for_args` 在 `help.rs:30-34`）；token 提取会跳过选项及其取值（`parser.rs:890-906`、`option_takes_value` `parser.rs:908-947`）。命中则渲染该页（含 `COMMANDS`/`OPTIONS`），未命中的回退到根帮助（`help.rs:30-34`、`help.rs:70-86`、`help.rs:88-114`）。
- 程序名（`ccusage` / `ccusage.exe`）会被剥掉，兼容 `ccusage daily --help` 与 `ccusage daily` 两种入口（`help.rs:36-53`）。
- `--version` 直接打印 `ccusage {version}` 并 `exit(0)`（`help.rs:15-18`）；版本串是编译期 `env!("CCUSAGE_VERSION")`（`rust/crates/ccusage/src/cli.rs:23`，见本章第 6 节）。
- `help_codegen.rs` 的职责：读 `src/cli-help.json`（选项集合：`flags`/`description`/`default`/`choices`）与 `src/cli-commands.json`（`root.usage`、`root.commands`、`pages[]`、`combinedOptions`），生成 `const ROOT_USAGE / ROOT_COMMANDS / HELP_PAGES` 与每个选项集的 `fn xxx() -> &'static str`。
  - 证据：`help_codegen.rs:5-17`（入口）、`19-52`（选项函数与组合选项）、`54-107`（`OPTIONS:` 段渲染，对齐宽度最小 36，`default:`/`choices:` 括号后缀）、`109-163`（命令规格）、`165-178`（组合选项去表头拼接）、`180-260`（写函数/取值工具）。
  - 构建钩子：`rust/crates/ccusage-cli-parser/build.rs:5-29`（`#[path = "src/help_codegen.rs"] mod help_codegen;` 复用同一份源码，`rerun-if-changed` 两个 JSON，产物写入 `OUT_DIR/cli-help.rs`）。
  - 选项说明的实际文案样例见 `rust/crates/ccusage-cli-parser/src/cli-help.json:2-45`（`-j, --json`、`-O, --offline`、`--no-offline` 标注 "Negatable of -O, --offline"）。
- 注意：**`--jq` 不在帮助文本里**。`cli-help.json` 全文件无 `jq` 命中（grep `jq` 在 `ccusage-cli-parser/src` 只命中 `parser.rs:771`、`parser.rs:923`、`parser.rs:1039`、`tests.rs:133/971` 与快照），即 `-q/--jq` 是可解析但未列进 `--help` 的选项。

#### 2.5 错误提示格式与退出码

统一出口在二进制层：解析失败 → stderr 打印单条 message，再打印 `Run 'ccusage --help' for usage.`，`process::exit(2)`。

证据：`rust/crates/ccusage/src/cli.rs:13-33`（`unwrap_or_else(|message| exit_with_usage(&message))`）、`cli.rs:35-39`（文案与退出码）。解析器内部只返回 `Result<_, String>`，不自行打印。

---

### 3. 参数校验（互斥/冲突、非法值、未知命令）

**先纠正一个常见假设：`--json` 与 `--jq` 不互斥。** `wants_json = shared.json || shared.jq.is_some()`（`rust/crates/ccusage-core/src/output.rs:14-16`），两者同时给出时 jq 分支优先（`output.rs:158-187`）。同理 `--offline` 与 `--no-offline` 也不互斥，后者只是把开关翻回去（`parser.rs:766-767`，语义由各 loader 自行判定，见 `docs/guide/environment-variables.md:205-213` 的对照说明）。

真正存在冲突检测的项：

| 规则 | 文案 | 证据 |
| --- | --- | --- |
| `--last` 只能用于按日历分组的报告（daily/weekly/monthly） | `The --last option is only available for the daily, weekly, and monthly reports.` | `parser.rs:1100-1110` |
| `--last` 不能与 `--since`/`--until` 并用 | `The --last option cannot be combined with --since or --until.` | `parser.rs:1111-1113` |
| `--last` 不能与 `--sections` 并用 | `The --last option cannot be used with --sections.` | `parser.rs:1114-1116` |
| `--since` 晚于 `--until` | `The --since date 'x' is later than the --until date 'y'.` | `parser.rs:1120-1132`（statusline 豁免，`parser.rs:1124-1126`） |
| `session --id` 与 `--sections`/`--by-agent` 并用 | `The --sections and --by-agent options cannot be used with session --id.` | `parser.rs:454-460` |
| 报告 flag 别名 | `Report flags like --daily are not supported. Use "ccusage daily" instead.` | `parser.rs:828-839` |
| agent 过滤器 | `Agent filters like --agent are not supported. Use "ccusage <agent> <report>", for example "ccusage codex daily".` | `parser.rs:841-855` |
| agent 不支持的 report | `The "blocks" report is only available for Claude Code usage.` / `The "weekly" report is not available for Codex usage.`（另附 `Use "ccusage <agent> daily" ...`） | `parser.rs:868-888`（分支在 `878-886`） |

非法取值文案（全部带 `Invalid ...` 前缀，退出码 2）：

- 日期：`Invalid value for --since 'x'. Expected YYYY-MM-DD or YYYYMMDD.`（`parser.rs:1047-1051`）。合法格式只有两种：8 位纯数字，或 `YYYY-MM-DD`；会校验真实日历（含闰年），`2026-02-30`、`2026-13-01`、`2026-7-10`、`2026/07/10`、带时间戳的串全部拒绝。证据：`rust/crates/ccusage-cli/src/types.rs:73-110`、`112-124`，测试 `types.rs:331-381`。
- `--last`：`Invalid value for --last 'x'. Expected a whole number of periods, 1 or greater.`（`0` 与解析失败同罪，`parser.rs:1053-1060`）。
- 枚举：`Invalid cost mode 'x'`（`parser.rs:1134-1141`）、`Invalid sort order 'x'`（`1143-1149`）、`Invalid --sections value 'x'. Expected one or more of: daily, weekly, monthly, session.`（`1151-1179`，去重且空串报错）、`Invalid week day 'x'`（`1181-1192`）、`Invalid speed option 'x'`（`1194-1202`）、`Invalid visual burn rate 'x'`（`1204-1212`）、`Invalid cost source 'x'`（`1214-1222`）。
- 数值型：`Invalid value for --session-length`（`parser.rs:208-213`）、`--refresh-interval`（`parser.rs:236-241`）、`--context-low-threshold`（`242-247`）、`--context-medium-threshold`（`248-255`）、`--debug-samples`（`755-760`）。
- 时区：`Invalid value for --timezone 'x'. Expected an IANA timezone name such as UTC or America/New_York, or 'local' for the system timezone.`（`rust/crates/ccusage/src/cli/timezone.rs:10-17`）。该校验在配置合并**之后**跑一次，只查命令实际会用的那个值（`timezone.rs:19-27`、测试 `118-150`）。
- 配置侧同规则：`config.rs:69-90`（见 §1.5）。

未知子命令/未知选项：

- 顶层非选项 token 不是已知命令：`Unknown command 'x'`（`parser.rs:100-102`），`parse_command` 的兜底同样报 `Unknown command 'x'`（`parser.rs:362`）。
- `claude` 下的未知子命令：`Unknown claude command 'x'`（`parser.rs:569-571`）；agent 报告位置上的未知 token 是 `Unknown <agent> command 'x'`（`parser.rs:710-712`）。
- 各命令的未知选项：`Unknown daily option 'x'`（`parser.rs:498`）、`Unknown weekly option 'x'`（`533`）、`Unknown session option 'x'`（`450`/`551`）、`Unknown blocks option 'x'`（`214`）、`Unknown statusline option 'x'`（`259`）、`Unknown codex option 'x'`（`623`）、`Unknown pi option 'x'`（`653`）、`Unknown openclaw option 'x'`（`683`）；共享表不认的 flag 报 `Unknown option 'x'`（`parser.rs:776`）。
- 多余位置参数：`Unexpected argument 'x'`（`parser.rs:123-125`）。
- 参数用尽仍要取值：`Expected option but reached end of arguments`（`arg_parser.rs:40-44`）。

---

### 4. 输出渲染

#### 4.1 JSON 序列化约定

键名一律 **camelCase**，且都是手写 `json!` 构造（没有 `#[derive(Serialize)]` 的 snake_case 泄漏）：

- 每日/每周/每月/通用汇总行：`summary_json`（`rust/crates/ccusage-core/src/output.rs:27-56`）→ `inputTokens`/`outputTokens`/`cacheCreationTokens`/`cacheReadTokens`/`totalTokens`/`totalCost`/`modelsUsed`/`modelBreakdowns`。
- 会话行：`session_summary_json`（`output.rs:58-77`）→ 额外 `sessionId`/`lastActivity`/`firstActivity`/`projectPath`。
- 汇总行：`totals_json`（`output.rs:79-121`）。
- agent 报告行：`agent_summary_json`（`rust/crates/ccusage-core/src/agent_report.rs:5-19`）；容器键 `daily|weekly|monthly|session`（`rust/crates/ccusage-adapter-all/src/report.rs:207-210`）、`totals`（`report.rs:175`）、`agents`（`report.rs:148-156`；`report.rs:136-143` 是 period/metadata 的插入点）。
- 周期字段的键名由 `period_key` 决定（`agent_report.rs:68`），首列表头由 `first_column` 决定（`agent_report.rs:50`），摘要取周期值用 `summary_period`（`agent_report.rs:59`）。

**空值省略规则**（这是脚本兼容性的关键）：

- 行级可选键按「有才插」：`date`/`month`/`week`/`project`/`credits`（`output.rs:38-54`）；会话行的 `credits` 同样条件插入（`output.rs:73-76`）。
- `totals.credits` 只在总和 > 0 时出现（`output.rs:112-115`）。
- `totals.unpricedModels` 只在存在缺价模型时出现，且排序去重（`output.rs:126-145`）；每个受影响的 `modelBreakdowns` 条目带 `"missingPricing": true`，有价的条目**省略该键**（`docs/guide/json-output.md:32-39`）。
- 浮点整数化：`json_float` 把有限且无小数部分的 `f64` 写成整数（`output.rs:452-462`）。
- 项目分组输出把 key 归到 `unknown`（`group_project_output`，`output.rs:147-156`）。

#### 4.2 `print_json_or_jq` 如何调用 jq

**外部二进制，不是内嵌库**：`std::process::Command::new("jq")`，把 JSON 写进子进程 stdin，stdout 继承，非零退出报 `jq failed`。

证据：`rust/crates/ccusage-core/src/output.rs:158-187`（spawn 在 `163-168`，写 stdin 在 `169-174`，等待并检查状态在 `175-178`；无 jq 时走 `serde_json::to_writer_pretty` + `\n`，`180-185`）。`--no-cost` 在序列化前先剥离成本字段（`output.rs:159-161` + `strip_cost_json` `output.rs:564-581`，递归删 `totalCost`/`costUSD`/`cost`）。

jq 不可用时（未安装）：spawn 失败会得到 `failed to run jq: <io error>`（`output.rs:165-168`）。文档只教「管道给 jq」（`docs/guide/json-output.md:580-604`），`--jq` 本身未进帮助文本（见 §2.4）。

#### 4.3 `sanitize_terminal_text`

把字符串里的控制字符转成可见转义（`escape_default`），防止模型名/项目名注入 ANSI 控制序列污染终端输出：

- 定义：`rust/crates/ccusage-core/src/output.rs:552-562`。
- 再导出：`rust/crates/ccusage-core/src/lib.rs:41`。
- 实际使用点（示例）：Codex 报告里对标签做净化（`rust/adapters/codex/src/report.rs:11` 引入、`rust/adapters/codex/src/report.rs:503` 调用）。
- 测试：`output.rs:905-910`（`sanitizes_terminal_control_characters_as_visible_escapes`，`"future\nclient\t\u{1b}[31m"`）。

#### 4.4 进度条与 TTY 检测

- 开关判定：`should_show_usage_load_progress(json, output_is_tty) = !json && output_is_tty`（`rust/crates/ccusage-core/src/progress.rs:35-37`）；TTY 判定是 `io::stdout().is_terminal()`（`progress.rs:94-96`）。
- 实际调用再加一层：非 `LOG_LEVEL=0` 才显示（`progress.rs:290-306`，条件在 `295-296`）。也就是说 `--json` 或管道输出、或 `LOG_LEVEL=0` 时都没有 spinner。
- 渲染细节：10 帧盲文 spinner、80 ms 间隔（`progress.rs:11-12`），写 stderr 并隐藏/恢复光标（`progress.rs:220`、`progress.rs:281-286`），单行宽度自适应（`fit_status_to_width` `progress.rs:87-92`，宽度取 `ccusage_terminal::terminal_width()`）。
- 单例：`thread_local` 的 `ACTIVE_PROGRESS`，已有会话时复用不重复起线程（`progress.rs:17-19`、`progress.rs:132-141`）；停止时清屏并清空状态（`progress.rs:198-229`）。
- 文案形如 `Loading usage logs (1/3) :: Codex, OpenCode`（`progress.rs:43-78`，测试 `326-337`）；价格刷新会追加前缀 `Refreshing model pricing from LiteLLM...`（测试 `340-353`）。

#### 4.5 表格与紧凑模式

- 紧凑布局条件：显式 `--compact`，或 **stdout 是 TTY 且终端宽度 < 100 列**（`output.rs:18-25` + `rust/crates/ccusage-core/src/lib.rs:61`）。
- 非 TTY 且窄**不会**自动紧凑（测试 `output.rs:593-597`）。
- 列集合随 `compact` 与 `show_cache_creation` 变化（`usage_table_columns`，`output.rs:385-404`），`--no-cost` 会裁掉 Cost 列（`output.rs:249-252`、`304-306`、`369-371`）。
- 空结果走 stderr：`No usage data found.`（`output.rs:233-236` + `output.rs:406-408`）。
- 缺价告警走 stderr，且区分离线：`WARN Missing embedded pricing for {model}; ... Run without --offline ...` vs `WARN Missing pricing for {model}; ...`（`output.rs:410-450`）。
- 表格实现与宽度计算在 `ccusage-terminal` crate（`rust/crates/ccusage-terminal/src/{table.rs,width.rs,terminal.rs}`）；`COLUMNS` 环境变量只影响它（`terminal.rs:6`）。

---

### 5. 环境变量清单（源码穷举）

以 `rg 'env::var'`（含 `var_os`）对 `rust/` 全树穷举：**21 个 `_ENV` 常量 + 13 个直接字面量**，去重后共 33 个用户可配置变量名，另加 1 个 Cargo 注入项（`OUT_DIR`），合计 34 个读取目标（下表逐一列出）：

| 变量 | 用途 | 默认/回退 | 证据 |
| --- | --- | --- | --- |
| `HOME` | 主目录 | 无则看 `USERPROFILE` | `rust/crates/ccusage-core/src/home.rs:5` |
| `USERPROFILE` | Windows 主目录 | 无则看 `HOMEDRIVE`+`HOMEPATH` | `home.rs:6`、`home.rs:17-31` |
| `HOMEDRIVE` / `HOMEPATH` | 拼出主目录 | — | `home.rs:7-8`、`home.rs:24-30` |
| `LOG_LEVEL` | 日志级别（`0` 静默…`5` trace） | 未设 = 默认级别 | `rust/crates/ccusage-core/src/logger.rs:11-15`；文档表 `docs/guide/environment-variables.md:61-90` |
| `NO_COLOR` | 关色（任何非空值） | 无 | `rust/crates/ccusage-terminal/src/style.rs:39` |
| `FORCE_COLOR` | 强制开色 | 无（否则看 TTY） | `style.rs:42` |
| `COLUMNS` | 终端宽度覆盖 | 无则探测终端 | `rust/crates/ccusage-terminal/src/terminal.rs:6` |
| `XDG_CACHE_HOME` | HTTP 缓存根（必须是绝对路径） | `~/.cache` | `rust/crates/ccusage/src/http.rs:39-52` |
| `CLAUDE_CONFIG_DIR` | Claude 数据目录（含逗号列表）与配置查找目录 | `~/.config/claude`、`~/.claude` | `rust/adapters/claude/src/paths.rs:26`、`rust/crates/ccusage-config/src/config.rs:272` |
| `XDG_CONFIG_HOME` | Claude 配置目录回退 | `~/.config` | `rust/adapters/claude/src/paths.rs:46` |
| `CODEX_HOME` | Codex 数据目录（可列表） | `~/.codex` | `rust/adapters/codex/src/paths.rs:227` |
| `XDG_DATA_HOME` | OpenCode 数据根 | `~/.local/share` | `rust/adapters/opencode/src/paths.rs:6`、`paths.rs:29` |
| `OPENCODE_DATA_DIR` | OpenCode 目录覆盖 | XDG 推导 | `opencode/src/paths.rs:5` |
| `AMP_DATA_DIR` | Amp | `~/.local/share/amp` | `rust/adapters/amp/src/paths.rs:5` |
| `DROID_SESSIONS_DIR` | Droid | `~/.factory/sessions` | `rust/adapters/droid/src/paths.rs:5` |
| `CODEBUFF_DATA_DIR` | Codebuff | `~/.config/manicode` | `rust/adapters/codebuff/src/paths.rs:5` |
| `HERMES_HOME` | Hermes | `~/.hermes` | `rust/adapters/hermes/src/paths.rs:5` |
| `PI_AGENT_DIR` | pi-agent | `~/.pi/agent/sessions` | `rust/adapters/pi/src/paths.rs:5` |
| `GOOSE_PATH_ROOT` | Goose | 标准 Goose 根 | `rust/adapters/goose/src/paths.rs:5` |
| `OPENCLAW_DIR` | OpenClaw | `~/.openclaw` | `rust/adapters/openclaw/src/paths.rs:9` |
| `KILO_DATA_DIR` | Kilo | `~/.local/share/kilo` | `rust/adapters/kilo/src/paths.rs:9` |
| `KIMI_DATA_DIR` | Kimi | `~/.kimi`、`~/.kimi-code` | `rust/adapters/kimi/src/paths.rs:9` |
| `QWEN_DATA_DIR` | Qwen | `~/.qwen` | `rust/adapters/qwen/src/paths.rs:9` |
| `COPILOT_HOME` / `COPILOT_OTEL_FILE_EXPORTER_PATH` | Copilot CLI 目录 / 单个 OTEL JSONL | `~/.copilot` | `rust/adapters/copilot/src/paths.rs:5-6`、`paths.rs:56`、`paths.rs:76` |
| `GEMINI_DATA_DIR` | Gemini CLI | `~/.gemini/tmp` | `rust/adapters/gemini/src/paths.rs:5` |
| `ANTIGRAVITY_DATA_DIR` | Antigravity | `~/.gemini/antigravity*`、`~/.config/antigravity` | `rust/adapters/antigravity/src/paths.rs:7` |
| `GROK_HOME` | Grok Build CLI | `~/.grok` | `rust/adapters/grok/src/paths.rs:9` |
| `ZCODE_HOME` | ZCode（非空但全非法时不回退默认） | `~/.zcode` | `rust/adapters/zcode/src/paths.rs:9`、文档 `docs/guide/environment-variables.md:55` |
| `CCUSAGE_MODEL_ALIASES` | 模型名别名（JSON 对象或 `a=b,c=d` 形式；支持 `-fast` 变体） | 无 | `rust/crates/ccusage-core/src/model_aliases.rs:8`、`model_aliases.rs:13-60` |
| `CCUSAGE_PRICING_JSON_PATH` | **构建期**：LiteLLM 快照输入文件 | 无则按 feature 下载/报错 | `rust/crates/ccusage-core/build.rs:13`、`build.rs:22-29`、`build.rs:75-82` |
| `CCUSAGE_VERSION` | **构建期**：版本串注入 | 无则读根 `package.json` 的 `version` | `rust/crates/ccusage/build.rs:6-18` |
| `OUT_DIR` | 构建脚本产物目录（Cargo 注入） | — | `rust/crates/ccusage-cli-parser/build.rs:18`、`rust/crates/ccusage-core/build.rs:65` |

**文档与实现不一致（S6 发现，建议记入交叉核验清单）**：下表的「实现」列指 `env::var` / `env::var_os` 读取点；以下变量在两版源码中都**没有**对应读取，只存在于文档。

| 变量名 | 文档位置 | 说明 |
| --- | --- | --- |
| `CCUSAGE_OFFLINE` | `docs/guide/environment-variables.md:124-131`（"Force offline mode by default"）、`:209`；v19 同文 `ccusage-v19/docs/guide/environment-variables.md:96-110` | 跨版本遗留：离线只能靠 `--offline` / 配置 `offline`（`parser.rs:766`、`config_schema.rs:378`） |
| `CCUSAGE_TIMEZONE` | `docs/guide/configuration.md:38` 与 `ccusage-v19/docs/guide/configuration.md:38` | 时区只能来自 `--timezone` 或配置 `timezone`（`parser.rs:770`、`timezone.rs:10-17`） |
| `CCUSAGE_GROK_PATH` | `docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24`、`:51` | 该设计文档本身要求**移除**它，只保留 `GROK_HOME` 与 `~/.grok`（该文 `:35`、`:51-52`、`:83`）；v20 实现为 `rust/adapters/grok/src/paths.rs:9` 的 `GROK_HOME` |

即：源码 `env::var` 读取的 `CCUSAGE_*` 只有 `CCUSAGE_VERSION`、`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_MODEL_ALIASES` 三个（见上表），其余 `CCUSAGE_*` 名称只在文档里流通。

---

### 6. HTTP 与版本

#### 6.1 `http.rs` 抓什么：只有价格表，且只认两个 URL

- 目标 URL 常量：LiteLLM `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`（`rust/crates/ccusage-core/src/pricing.rs:55-56`）与 models.dev `https://models.dev/api.json`（`pricing.rs:57`）。
- URL 白名单：`PricingEndpoint::for_url` 只接受这两个（`pricing.rs:70-78`），其它 URL 直接 `unsupported pricing URL: {url}`（`rust/crates/ccusage/src/http.rs:25-34`）。
- 响应校验：拿到 body 后按 endpoint 做「有可用条目」校验（`pricing.rs:80-126`）与轮廓校验（`pricing.rs:88-98`），失败报 `pricing response failed <endpoint> validation`（`http.rs:121-126`）。
- 没有任何「更新检查」请求：全仓 HTTP 调用点只有 `fetch_pricing_json` / `fetch_models_dev_json`（`pricing.rs:2863-2893`）与构建期下载（`rust/crates/ccusage-core/build.rs:89-105`）。

#### 6.2 超时、缓存与并发安全

- 超时：全局 10 秒（`http.rs:11` 常量，`http.rs:75-78` 装配 `ureq::Agent`）；响应体上限 64 MiB（`http.rs:12`、`http.rs:104-120`、`read_bounded_body` `http.rs:139-151`）。
- 缓存键：URL 派生文件名 + FNV-1a 哈希（`cache_file_stem`，`http.rs:236-257`），落在 `$XDG_CACHE_HOME/ccusage/http-cache/` 或 `~/.cache/ccusage/http-cache/`（`http.rs:37-52`）。
- 条件请求：带 `if-none-match`（`http.rs:80-82`）；304 时只接受**校验通过**的缓存体，否则先失效再无条件重取一次（`http.rs:88-99`，`revalidate=false` 走 `fetch_json_with_cache_dir_inner` 递归）。
- 原子写：etag+body 写临时文件后 `rename`，注释明确「一次 rename 保证并发读者不会看到混搭的一对」（`http.rs:153-157`、`http.rs:199-229`）。
- 缓存文件上限与 etag 合法性：`CACHE_FILE_MAX_BYTES`（`http.rs:14`）、etag 必须 ASCII 且无控制字符（`http.rs:186-192`、`http.rs:200-206`）。
- 客户端装在二进制里而不是 core：`set_json_fetcher`（`pricing.rs:2871-2893`，注释解释为「避免把 ureq/TLS 拉进每个 adapter 的依赖」），装配点 `rust/crates/ccusage/src/main.rs:26`。
- models.dev 失败节流：失败后 60s 内不再重试（`pricing.rs:58`、`pricing.rs:795-799`）。
- 离线开关：`--offline`/`--no-offline`（`parser.rs:766-767`）与配置 `offline`/`noOffline`（`config_schema.rs:377-380`），statusline 默认离线（`rust/crates/ccusage-cli/src/types.rs:224`）。

#### 6.3 `CCUSAGE_VERSION` 注入方式

- 构建脚本：`rust/crates/ccusage/build.rs:5-18`。优先 `env::var("CCUSAGE_VERSION")`；未设则读仓库根 `../../../package.json` 的 `version`；随后 `println!("cargo:rustc-env=CCUSAGE_VERSION={version}")`，并声明 `rerun-if-env-changed` / `rerun-if-changed`。
- 运行期读取：`env!("CCUSAGE_VERSION")`（`rust/crates/ccusage/src/cli.rs:23` 传给解析器，最终 `--version` 打印 `ccusage {version}`，`help.rs:15-18`）。
- 一致性测试：`compiled_version_matches_release_package`（`rust/crates/ccusage/src/main.rs:125-135`）断言编译期版本 == 根 `package.json` 的 `version`。
- Nix 打包显式传这两个环境变量：`package.nix:46-47`（`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_VERSION`），并在 `nix/cargo-artifacts.nix:98` 列为影响产物哈希的变量。
- 价格快照构建：`rust/crates/ccusage-core/build.rs:17-40`（LiteLLM 压缩快照 + models.dev 两个快照），压缩级别 10（`build.rs:58-62`），嵌入白名单（`build.rs:203-221`：`claude-*`、`anthropic*`、`gpt-*`、`o3*`、`o4-mini*`、`openai/*`、`azure/*`、`zai/*`、`openrouter/openai/*`），运行期首次使用时 inflate（`pricing.rs:40-54`）。

---


## 10. 版本史：v19 TypeScript → v20 Rust 的语义变化


### 10.1 v19 的源码结构

v19 是 TS monorepo，业务逻辑在 `apps/ccusage/src`：

- 入口与运行时：`cli.ts`（234 行）只是 Bun/Node 运行时选择器（优先找 `bun`，否则要求 Node ≥ 22，`cli.ts:9`、`cli.ts:95-128`、`cli.ts:130-167`），真实命令定义在 `main.ts`/`main.bun.ts`/`main.node.ts`，顶层 `index.ts` 7 行。
- 命令层：`commands/{index,daily,weekly,monthly,session,blocks,statusline,all,agent,codex,session_id,loading-progress}.ts`。
- 适配层：`adapter/{claude,codex,opencode,amp,pi}` + `adapter/shared.ts`（735 行共享 loader 框架）；Claude 解析器单文件 `adapter/claude/data-loader.ts` 达 7004 行。
- 共享包：`packages/internal/src/{jsonl,fs,pricing,workers,array,colors,format,sort,sqlite,logger,json-file-state,pricing-fetch-utils}.ts`；表格在 `packages/terminal/src/table.ts`（1713 行）。
- 证据：`ccusage-v19/apps/ccusage/src/` 与 `ccusage-v19/packages/` 目录清单；`ccusage-v19/apps/ccusage/src/cli.ts:91-128`；`ccusage-v19/packages/terminal/src/table.ts` 行数。

v20 的对应物：Rust workspace `rust/crates/{ccusage,ccusage-cli,ccusage-cli-parser,ccusage-config,ccusage-core,ccusage-terminal,ccusage-adapter-all,ccusage-test-support}` + `rust/adapters/*`（18 个 adapter，每个 `src/{lib,loader,parser,paths,report}.rs`）。`apps/ccusage` 在 v20 只保留 npm 分发壳：`apps/ccusage/src/cli.js:27-58`（平台包名映射）、`cli.js:93-109`（解析 native 二进制）、`cli.js:233-258`（spawn 并转发信号），行为等价于 v19 `cli.ts` 的一部分但目标从 JS bundle 换成了 native binary。

### 10.2 CLI 与配置对照

| 维度 | v19（TS） | v20（Rust） |
| --- | --- | --- |
| 解析框架 | gunshi + valibot（`ccusage-v19/apps/ccusage/src/shared-args.ts:1-4`、`:106`、`:111-114`） | 自研 `parser.rs` + `arg_parser.rs`（`rust/crates/ccusage-cli-parser/src/parser.rs:65-133`） |
| 日期格式 | 只接受 `YYYYMMDD`（`ccusage-v19/apps/ccusage/src/shared-args.ts:19-30`，`filterDateSchema`） | `YYYY-MM-DD` 或 `YYYYMMDD`，且校验真实日历（`rust/crates/ccusage-cli/src/types.rs:73-110`） |
| `--last` | 不存在（v19 源码全仓无 `--last`；只有 `weekly` 的 `startOfWeek`，`ccusage-v19/apps/ccusage/src/commands/weekly.ts:26`） | 新增（`parser.rs:751`、`rust/crates/ccusage/src/cli/last_window.rs:11-24`） |
| `--jq` | 不存在（v19 全仓无 `--jq` 选项，文档只教管道 `\| jq`） | 新增（`parser.rs:771`；实现为外部 jq，`output.rs:158-187`） |
| `--sections` / `--by-agent` / `--all` | 不存在 | 新增统一报告选项（`parser.rs:383-403`、`parser.rs:366-368`） |
| `--no-cost` | `sharedArgs` 中无（`ccusage-v19/apps/ccusage/src/shared-args.ts:18-106`） | 新增（`parser.rs:775`、`strip_cost_json` `output.rs:564-581`） |
| 短选项字母 | `-O` 代表 `--offline` 且 `negatable`（`shared-args.ts:69-75`） | `-O` 同为 `--offline`，否定写作独立 flag `--no-offline`（`parser.rs:766-767`） |
| 配置文件 | `ccusage.json`，搜索 `$CWD/.ccusage/ccusage.json` + 各 Claude 目录下的 `ccusage.json`（`ccusage-v19/apps/ccusage/src/config-loader-tokens.ts:71-82`，文件名常量 `ccusage-v19/apps/ccusage/src/consts.ts:94`） | 同顺序（`rust/crates/ccusage-config/src/config.rs:258-283`） |
| 配置加载语义 | 第一个可解析文件生效，`$schema`/`defaults`/`commands` + 5 个 agent 段（`config-loader-tokens.ts:87-129`、`:235-293`）；有原型污染防护（`config-loader-tokens.ts:45-49`）；合并顺序 CLI > 命令配置 > 默认配置（`:296-341`） | 第一个「可读且是 JSON object」的文件生效（`config.rs:246-256`）；18 个 agent 段；应用顺序见 §1.4（`config.rs:92-132`） |
| Schema 生成 | 从 gunshi 参数定义生成（`ccusage-v19/apps/ccusage/scripts/generate-json-schema.ts`，511 行；`package.json` 的 `generate:schema`：`ccusage-v19/apps/ccusage/package.json:47`） | 从 Rust 结构体用 schemars 生成（`rust/crates/ccusage-config/src/config_schema.rs:711`；命令 `apps/ccusage/justfile:25-27`） |
| 版本来源 | `package.json` 的 `version` 直接 import（`ccusage-v19/apps/ccusage/src/commands/index.ts:4`，`package.json:4`） | 编译期 `CCUSAGE_VERSION`（`rust/crates/ccusage/build.rs:5-18`），并有测试对齐 `package.json`（`rust/crates/ccusage/src/main.rs:125-135`） |
| 分层帮助 | gunshi 自带 | 构建期 codegen：`cli-help.json` + `cli-commands.json` → `OUT_DIR/cli-help.rs`（`rust/crates/ccusage-cli-parser/build.rs:5-29`、`help_codegen.rs:5-17`） |
| 每命令报告支持差异报错 | `getReportFlagAliasError` / `getAgentFilterOptionError` / `getUnsupportedAgentReportError`（`ccusage-v19/apps/ccusage/src/commands/index.ts:198-227`） | 等价实现 `report_flag_alias_error` / `agent_filter_option_error` / `unsupported_agent_report_error`（`parser.rs:828-888`）——**文案与分支是逐条对齐移植的** |

### 10.3 Claude 去重键

v19：唯一键由 message id 与 request id 拼接，作用域是「单个 JSONL 文件内」的 `processedEntries`：

```ts
// ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1194-1202
export function createUniqueHash(data: UsageData): string | null {
	const messageId = data.message.id;
	if (messageId == null) return null;
	const requestId = data.requestId;
	return requestId == null ? messageId : `${messageId}:${requestId}`;
}
```

- 替换规则：候选条目的 token 合计更大，或合计相同但带 `speed` 字段，才替换已存在条目（`ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1629-1638`）。
- 应用点（每个报告各一份，逻辑相同）：`data-loader.ts:2075-2123`（daily：`existingEntryIndex = processedEntries[uniqueHash]`，不满足替换条件直接 `return`）、`:2136-2183`、`:2199-2264`、`:2322-2334`、`:2457-2469`；blocks 用带索引的变体 `:2894-2913`。

v20：把键升级为「哈希 + 会话感知 + sidechain 变体」，并且是**跨文件全局**去重：

- 精确键：`usage_dedupe_hash(message_id, request_id, session_id, timestamp)`（`rust/adapters/claude/src/lib.rs:350-371`）；当没有 request id 时，只有 `msg_` 前缀的 Anthropic message id 才允许跨时间戳合并，否则要求时间戳相同（`lib.rs:359-362` + `is_anthropic_message_id` `lib.rs:373-375` + `loaded_entry_matches_dedupe_key` `lib.rs:413-425`）。
- 额外两把键用于 sidechain 重放：`sidechain_replay_dedupe_hash`（`lib.rs:377-384`）与 `sidechain_entry_replay_dedupe_hash`（`lib.rs:386-393`），并在替换后重建索引（`lib.rs:295-311`）。
- 主入口：`push_deduped_entry`（`lib.rs:214-340`，查找/替换/插入三条路径分别在 `219-268`、`270-318`、`320-340`），入口循环在 `lib.rs:170-187`；daily 走镜像实现 `push_deduped_daily_entry`（`rust/adapters/claude/src/daily.rs:99-132`）。
- 替换规则对齐 TS：`should_replace_deduped_entry`（`lib.rs:198-213`）。
- 测试名体现了「比 TS 更细」的语义：`keeps_gateway_usage_from_distinct_sessions_with_reused_message_id`（`lib.rs:1196`）、`dedupes_anthropic_message_id_from_same_session_at_distinct_timestamps`（`lib.rs:1257`）、`dedupes_repeated_requestless_writes_at_the_same_timestamp`（`lib.rs:1283`）、`dedupes_sidechain_replay_after_an_equal_copy_from_another_session`（`lib.rs:1355`）；集成侧对应 `keeps_reused_message_id_from_distinct_sessions_at_same_timestamp`（`rust/crates/ccusage/src/main.rs:286-307`）。

**语义变化**：v19 中「同 message id + 同 request id」在不同会话/文件也会合并（键里没有 session），只有 gateway 前缀 id 因 requestless 才可能存活；v20 显式区分 `msg_` 与 `gateway_` 两类 id，并按会话隔离 requestless 的跨时间戳合并（`lib.rs:359-362`、测试 `main.rs:310-357`）。这是最容易导致两版数字不一致的点。

### 10.4 Codex 累计值处理

v19：**优先**使用 `last_token_usage`（`ParsedTokenCountLine.lastUsage`，`ccusage-v19/apps/ccusage/src/adapter/codex/types.ts:12-17`），只有它缺失时才回退到「累计值差分」（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:205-212`）；去重用「事件序列化键」：

```ts
// ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:90-104
function subtractRawUsage(current: RawUsage, previous: RawUsage | null): RawUsage {
	return {
		input_tokens: Math.max(current.input_tokens - (previous?.input_tokens ?? 0), 0),
		cached_input_tokens: Math.max(current.cached_input_tokens - (previous?.cached_input_tokens ?? 0), 0),
		output_tokens: ... /* 同上，另含 reasoning_output_tokens / total_tokens */
	};
}
```

- 取值优先级（关键）：`let raw = parsed.lastUsage; if (raw == null && parsed.totalUsage != null) { raw = subtractRawUsage(parsed.totalUsage, previousTotals); }`（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:205-209`）——`last_token_usage` 是主路径，累计差分是回退路径。
- 状态推进：`previousTotals` 只在 `parsed.totalUsage != null` 时前移（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:201`、`:210-212`）。
- `cached` 被夹到 `min(cached_input_tokens, input_tokens)`（`parser.ts:106-124`，第 109 行）。
- 去重键 = 时间戳+模型+五个 token 字段拼接（`parser.ts:126-129`），线性 `Set` 去重（`parser.ts:131-143`）。

v20：

- v19 的回退减法在 v20 里对应 `subtract_codex_raw_usage`（`rust/adapters/codex/src/parser.rs:1284-1308`，用 `saturating_sub` 且多了 `cache_creation_tokens` 一档），v20 同样以 `last_token_usage` 为主路径（`parser.rs:483-496` 先取 `total_token_usage` 做前移与 `last_token_usage` 缺失判断）。
- 状态机升级：`previous_totals` 字段（`parser.rs:89`）在 `parser.rs:483-496` 推进；新增 replay 判定分支 `CodexReplayState::SkippingRewrittenBurst`（`parser.rs:254-257`，用时间差判断重写突发）。
- 事件去重：`dedupe_codex_events`（`rust/adapters/codex/src/loader.rs:240-…`），测试 `dedupes_matching_codex_usage_events_from_distinct_sessions`（`loader.rs:552`）、`dedupes_compaction_usage_by_response_id`（`loader.rs:562`）、`dedupes_copied_branch_history_across_session_files`（`loader.rs:615`）。
- compaction/replay 是 v20 新增的**覆盖范围**（不是新增 `last_token_usage` 支持，两版都优先它）：`rust/adapters/codex/src/compaction_tests.rs:141-143` 等用例覆盖 `last_token_usage` 与 `total_token_usage` 并存的情况；`subtracts_cache_write_usage_from_cumulative_token_totals`（`rust/adapters/codex/src/loader.rs:943`）说明 v20 会从累计值里扣掉 cache write。
- 迁移对齐测试：`prices_codex_versioned_models_like_typescript_adapter`（`rust/crates/ccusage/src/main.rs:694`）、`prices_codex_gpt_5_4_like_typescript_adapter`（`main.rs:773`）。

**语义变化**：取值**顺序相同，但 v20 多一道门控**——两版都先看 `last_token_usage`；但 v20 会先用 `cumulative_advanced`（即 `total_token_usage` 是否与上一条不同；**该字段缺失时门控视为通过**）判定，**未前进时丢弃 `last_token_usage` 并转走累计差分**（`rust/adapters/codex/src/parser.rs:483-494`），而 v19 无此门控、`lastUsage` 存在即无条件采用（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:205-212`）。因此「累计值未前进但 `last_token_usage` 存在」这类样本两版会给出不同结果——口径详见 §5.3。其余差异是 v20 增加了 replay/rewrite 状态机（跳过被重写的突发，`parser.rs:254-257`）、从累计值中扣减 cache write（`rust/adapters/codex/src/loader.rs:943`）以及 response id 层面的 compaction 去重（`loader.rs:562`）。跨会话相同事件两版都会去重（v19 的键不含 session，`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:126-129`；v20 走 `dedupe_codex_events`，`loader.rs:240`）。

### 10.5 5 小时 block 算法

v19（`ccusage-v19/apps/ccusage/src/session-blocks.ts`）：

- 常量 `DEFAULT_SESSION_DURATION_HOURS = 5`（`session-blocks.ts:7`）。
- 首个 block 起点 = 首条消息时间**向下取整到整点**（`floorToHourMs`，`session-blocks.ts:45-47`；调用在 `:135` 与 `:165`）。
- 切块条件：`timeSinceBlockStart > 5h` **或** `timeSinceLastEntry > 5h`（`session-blocks.ts:146`）；后者还会插入 gap block（`:157-162`，`createGapBlock` 在 `:247-…`）。
- 活跃判定：`now - actualEnd < 5h && now < start + 5h`（`session-blocks.ts:202`）；`actualEndTime` 取块内最后一条消息时间（`:199-201`）。
- 汇总：块内 4 类 token 求和 + `costUSD` 求和 + 模型集合（`:204-237`）；burn rate / 预测在 `:284-357`。
- 测试常量另有一处硬编码 `SESSION_DURATION_MS = 5 * 60 * 60 * 1000`（`:373`）。

v20（`rust/crates/ccusage/src/blocks.rs` + `ccusage-core`）：

- 常量同值：`DEFAULT_SESSION_DURATION_HOURS: f64 = 5.0`（`rust/crates/ccusage-core/src/lib.rs:59`），`DEFAULT_RECENT_DAYS: i64 = 3`（`:60`）。
- `identify_session_blocks(entries, session_duration_hours, ...)`（`rust/crates/ccusage/src/blocks.rs:53-104`），`floor_to_hour`（`blocks.rs:105-107`），`create_block`（`blocks.rs:109-147`），`create_gap_block`（`blocks.rs:149-…`），gap 在 JSON 里以 `"isGap"` 暴露（`blocks.rs:211`）。
- 调用点固定用 5 小时常量：`rust/crates/ccusage/src/commands/mod.rs:573`、`:600`、`:659`（另外 `blocks --session-length` 可覆盖，`parser.rs:208-213`）。

**结论：算法结构一致**（整点对齐、双条件切块、gap 块、活跃判定），差异在实现细节与可配置项（v20 支持 `--session-length`、`--token-limit`，v19 的 blocks 命令另有自己的 token limit 解析 `ccusage-v19/apps/ccusage/src/commands/blocks.ts:106-114` 与默认最近天数常量 `ccusage-v19/apps/ccusage/src/consts.ts:9`）。

### 10.6 价格表来源与匹配

| 维度 | v19 | v20 |
| --- | --- | --- |
| 来源 | 仅 LiteLLM 原始文件（`ccusage-v19/packages/internal/src/pricing.ts:4-5`） | LiteLLM + models.dev 双源（`rust/crates/ccusage-core/src/pricing.rs:55-57`） |
| 离线数据 | 构建期 macro 预取，按 agent 过滤后内联（`ccusage-v19/apps/ccusage/src/pricing-fetcher.ts:14`、`ccusage-v19/apps/ccusage/src/adapter/codex/pricing-macro.ts:10-17`、`:19-31`） | 构建期压缩内嵌（deflate level 10，`rust/crates/ccusage-core/build.rs:58-62`；白名单 `:203-221`），运行期 inflate |
| 抓取 | `fetch(url)` + `response.json()`（`ccusage-v19/packages/internal/src/pricing.ts:180-225`） | `ureq` + 10s 超时 + ETag/304 + 64MiB 上限 + 形状校验（`rust/crates/ccusage/src/http.rs:11-14`、`:75-137`） |
| 缓存策略 | 进程内 `cachedPricing` + 单飞 promise（`pricing.ts:227-243`）；失败回落到本地离线数据（`:161-178`） | 磁盘 HTTP 缓存（条件请求）+ models.dev 60s 失败退避（`pricing.rs:58`） |
| 模型匹配 | 直接命中候选前缀（`pricing.ts:245-254`、Claude 前缀表 `ccusage-v19/apps/ccusage/src/pricing-fetcher.ts:6-12`）→ 否则全表小写 `includes` 双向匹配，取长度差最小者（`pricing.ts:272-288`） | `PricingMap::find`（`rust/crates/ccusage-core/src/pricing.rs:1354-1460`）、`find_exact`（`:1484`）、`find_exact_with_fallback`（`:1501`）；模糊扫描受「exact-only id」门控（注释 `pricing.rs:405-446`、`:1619-1665`），并要求模型键边界（测试 `fuzzy_match_requires_model_key_boundaries` `pricing.rs:5663`、`fuzzy_match_prefers_longest_model_key` `pricing.rs:5854`、`fuzzy_match_does_not_fall_back_across_numeric_model_versions` `pricing.rs:5706`、`fuzzy_match_allows_date_like_suffixes_for_known_numeric_model_versions` `pricing.rs:5735`） |
| 别名 | 无 | `model_without_date_suffix`（`pricing.rs:2767-…`）、别名表（`pricing.rs:2792-2824`）、`CCUSAGE_MODEL_ALIASES` 环境变量（`rust/crates/ccusage-core/src/model_aliases.rs:8-60`） |
| 分层计价 | `calculateTieredCost`（`ccusage-v19/packages/internal/src/pricing.ts:16-54`）+ `calculateCostFromPricing`（`pricing.ts:318-…`，200k 阈值字段） | `tiered_cost`（`rust/crates/ccusage-core/src/cost.rs:201-…`）、`calculate_cost_from_pricing`（`cost.rs:118-…`）、配置化 `*_above_200k_tokens`（`config_schema.rs:600-603`） |

**语义变化**：匹配从「双向 includes + 最短长度差」（容易把 `claude-opus-5` 与 `claude-opus-5-eu`、`-turbo` 等变体互相吞掉）收紧为「精确（含归一化/日期后缀/别名）优先，模糊需键边界且受 exact-only 门控」；同时新增第二价格源与磁盘级条件请求缓存。`rust/crates/ccusage/src/main.rs:694/773` 的 `..._like_typescript_adapter` 测试说明 Codex 侧仍以复刻 TS 结果为目标。

### 10.7 cost mode

v19（`ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1284-1336`）：

- `display`：永远用 `costUSD`，缺失记 0（`:1291-1294`）。
- `calculate`：永远按 token 重算；没有 model 名记 0（`:1296-1305`）。
- `auto`：有 `costUSD` 用它，否则重算，再否则 0（`:1307-1321`）。
- 另有「立即成本」快路径 `getImmediateCostForEntry`（`:1326-1336`），只对 display / 带 costUSD 的 auto 生效。
- 聚合口径 `calculateTotals` / `createTotalsObject`（`ccusage-v19/apps/ccusage/src/calculate-cost.ts:53-87`）。

v20（`rust/crates/ccusage-core/src/cost.rs`）：

- `calculate_cost_for_usage_at` 的 match（`cost.rs:31-46`）：`Display => cost_usd.unwrap_or(0.0)`（`:40`）、`Auto => cost_usd 优先否则计算`（`:41-43`）、`Calculate => calculate_cost_from_tokens`（`:44`）。
- 缺价标记：`missing_pricing_model_for_usage` 在 display、或 auto 且已有 costUSD 时返回 `None`（`cost.rs:48-59`），与 `docs/guide/json-output.md:59` 的说明一致。
- 新增倍率：cache creation 1h 输入倍率 2.0（`cost.rs:7`）、`fast`/`flex` 服务层级乘数（`rust/crates/ccusage-core/build.rs:186-195` 抽取 `provider_specific_entry.fast/flex`；测试 `rust/crates/ccusage/src/main.rs:723-770`、`:772-809`）。

**语义变化**：三态语义保持不变；v20 增加「未计价模型」显式信号（`unpricedModels` / `missingPricing`）与服务层级乘数，且 cost 来源可再细分（`--cost-source` 仅 statusline：`auto|ccusage|cc|both`，`parser.rs:1214-1222`）。

### 10.8 v20 相比 v19 的语义变化清单（汇总）

1. **运行时与分发**：Bun/Node 脚本 → 原生二进制；npm 包降级为 wrapper（`apps/ccusage/src/cli.js:93-109`、`:233-258`）。v19 的 `CCUSAGE_BUN_AUTO_RUN` 开关（`ccusage-v19/apps/ccusage/src/env.ts:1-2`）随 Bun 路径一起消失。
2. **并发模型**：v19 用 worker_threads + `CCUSAGE_JSONL_WORKER_THREADS`（`ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1978` 等 4 处）与 `CCUSAGE_JSONL_READ_CONCURRENCY`（`data-loader.ts:208`）→ v20 用线程池 + `--single-thread`（`rust/crates/ccusage-cli/src/types.rs:56`）与按体积分块（测试 `balances_file_chunks_by_size`，`rust/crates/ccusage/src/main.rs:176-209`）。
3. **Claude 去重**：单键 `messageId[:requestId]` → 会话感知 + `msg_`/gateway 区分 + sidechain 重放键（§4.4）。
4. **Codex**：取值顺序不变（都先看 `last_token_usage`），但 v20 加了「累计值必须前进」的门控，未前进时改走累计差分 → 另新增 replay/rewrite 状态机、compaction 去重与 cache-write 扣减（§5.3）。
5. **日期输入**：仅 `YYYYMMDD` → 双格式 + 日历校验（§9.3）。
6. **CLI 能力**：新增 `--jq`、`--last`、`--sections`、`--by-agent`、`--no-cost`、`--all`；错误出口统一为 exit 2 + `Run 'ccusage --help' for usage.`（`rust/crates/ccusage/src/cli.rs:35-39`）。
7. **帮助系统**：gunshi 自动生成 → 构建期从两个 JSON 生成静态表（§9.2.4）。
8. **配置**：字段扩展到 18 个 agent 段 + 统一 `commands` + pricingOverrides + pi.stores；校验错误统一 `Invalid ccusage config: ...`（§9.1.5）。
9. **价格**：新增 models.dev 源、磁盘 ETag 缓存、别名与精确优先匹配、缺价信号（§8）。
10. **版本**：`package.json` 直接 import → 编译期 `CCUSAGE_VERSION` + 一致性测试（`rust/crates/ccusage/build.rs:5-18`、`main.rs:125-135`）。
11. **保留的 TS 兼容断言**（跨版本回归锚点）：`formats_windows_user_project_paths_like_typescript`（`rust/crates/ccusage/src/main.rs:150`）、`sorts_usage_files_like_typescript_path_strings`（`main.rs:212`）、`loads_daily_summaries_like_loaded_entry_aggregation`（`main.rs:483`）、`rejects_null_schema_fields_like_typescript_loader`（`rust/adapters/claude/src/lib.rs:1025`）、`allows_null_content_like_typescript_loader`（`lib.rs:1041`）。

---

### 未验证/存疑（本节同时覆盖配置/CLI/输出章与版本史章的未决项）

> 第 8 轮实测（§13.5）已覆盖本节第 1 条的一部分（`--jq` 不在帮助里，实测确认）与第 9 条（运行期状态）；
> 第 6 条（`blocks` JSON 字段全集）也已被 §13.5 的 `blocks --json` 实测补全。其余各条与 §13.5 无关，
> 仍未闭环。

> 第 8 轮实测复核（§13.5）已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目；此处保留原始清单与当时的判断，逐条状态以 §13.5 为准。

1. **`--jq` 是否应在帮助里出现**：`cli-help.json` 无 jq 条目（grep `jq` 只命中 parser/tests/snapshots），但选项确实可解析。无法从源码判断这是「故意隐藏」还是「漏写」，需要看 v20 的 release note 或 changelog（本仓未查）。
2. **`docs/guide/config-files.md`（614 行）未逐行精读**：只核对了它的 `$schema` 示例与 `jq` 校验说明（`:13`、`:593`），其中关于「多文件回退」的表述与实现（只取第一个可用文件）可能有出入，建议 S7/父代理核对。
3. **v19 → v20 的 entry 级 cost 计算路径**没有逐行对齐：v20 的 `cost.rs` 只读了结构段（`31-59`、`118`、`201`），`calculate_cost_from_tokens` 与 `find_at`（按事件时间选价，`pricing.rs:1461`）的内部规则未展开；若需要「某一具体模型两版钞票差异」级别的结论，需补读 `cost.rs:93-200`。
4. **v20 价格来源优先级**（外置 override / 内嵌 LiteLLM / models.dev / 运行期刷新）的**合并顺序**只读到零散注释（`pricing.rs:405-446`、`:1489-1500`、`:1631-1665`），未形成完整链路结论；`PricingMap::load_with_overrides`（`pricing.rs:1002`）本体未读。
5. **v19 的 `packages/internal/src/pricing-fetch-utils.ts`（47 行）未读**，构建期预取的过滤细节只有 `pricing-macro.ts` 的调用面。
6. **`ccusage blocks` 的 JSON 字段全集**未穷举：只确认 `isGap`（`blocks.rs:211`）与 `block_json`（`rust/crates/ccusage/src/main.rs:11` 导出），burn rate / 预测字段的键名未逐项核对。
7. **v20 `--last` 与配置 `since` 的交互**：`--last` 解析发生在 CLI 之后（`rust/crates/ccusage/src/cli.rs:29-31`，`last_window.rs:15-23`），会覆盖配置里已应用的 `since`；如果配置只给了 `until`，行为是「since 由 --last 推出 + until 保留」，此组合没有专门测试覆盖（`last_window.rs` 测试只覆盖了 `--last` 单独使用）。
8. **`--jq` 与 `--no-cost` 的组合**：`strip_cost_json` 在 jq 之前执行（`output.rs:159-161`），因此 jq 过滤器看不到被删字段——这是实现事实，但未在文档中声明。
9. **运行期验证状态**：第 1–7 轮为纯源码阅读；**第 8 轮已构建并运行 ccusage**（`--offline`，见 §13.5），本节与运行期相关的结论（CLI 解析、帮助文本、`--jq` 隐藏性）均已实测。行号基于 HEAD `57afdc2`（v20.0.31）与 `4aedac0`（v19.0.3），若仓库前进需重新核对。

## 11. 实现与官方文档的差异清单

研究过程中发现的「文档写的 ≠ 代码做的」条目。**以代码为准**；列在这里是因为它们会直接影响使用者对数字的解读，也是复现差异时的第一现场。

| # | 位置 | 文档说法 | 代码事实 | 影响 |
| --- | --- | --- | --- | --- |
| 1 | `docs/guide/blocks-reports.md:150-151` | 「`--order desc` 显示最新块（默认）」 | `SortOrder` 的 `#[default]` 是 `Asc`（`rust/crates/ccusage-cli/src/types.rs`），`blocks` 默认最旧在前 | 直接照文档复现会得到相反顺序 |
| 2 | `docs/guide/weekly-reports.md:138-149` | `modelBreakdowns` 是**对象映射**（模型名 → 明细） | 实际是**数组**（`UsageSummary.model_breakdowns: Vec<ModelBreakdown>`，`rust/crates/ccusage-core/src/types.rs:175`） | 按文档写的解析器会崩 |
| 3 | `docs/guide/codex/index.md:50` 与 `:116` | 「缺模型元数据的会话被跳过」 | 同文 `:54` 说「早期日志仍计入、按 `gpt-5` 回退」，与代码一致（`rust/adapters/codex/src/parser.rs:813` 附近的回退） | 文档**自身**前后矛盾 |
| 4 | `rust/adapters/codex/src/README.md:47-53` | 描述 `task_started` / `inter_agent_communication_metadata` / `trigger_turn` 的边界处理 | 全仓 grep 这三个符号只命中该 README，代码未实现；真实实现是父前缀 usage 全等匹配 + 「重写突发」启发式（`codex/src/parser.rs:236`、`:122`） | 会把未实现的能力当成已实现 |
| 5 | `docs/guide/environment-variables.md:124/129/209` | `CCUSAGE_OFFLINE` 强制离线 | 两版源码（v19/v20）**均无**该变量的读取 | 设了不生效 |
| 6 | `docs/guide/configuration.md:38` | `CCUSAGE_TIMEZONE` | 同上，两版均无实现；真实入口是 `--timezone` 与配置项 | 设了不生效 |
| 7 | `docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24/51` | `CCUSAGE_GROK_PATH` | 设计稿本身要求移除；v20 实际用 `GROK_HOME`（`rust/adapters/grok/src/paths.rs:9`） | 历史设计遗留 |
| 8 | `--help` 输出 | — | `--jq` 可被解析并生效，但**未出现在帮助文本**（`cli-help.json` 无该条目） | 隐藏选项 |
| 9 | `docs/guide/cost-modes.md:198-205` | 成本公式（含 1 小时缓存 = input × 2） | 与 `cost.rs` 一致（`rust/crates/ccusage-core/src/cost.rs:7`、`:128-131`） | **一致**（正面例子） |
| 10 | `docs/guide/codex/index.md:55` | 缓存读价缺失时回退 input 价 | 与 `rust/adapters/codex/src/report.rs:344-348` 一致 | **一致** |
| 11 | `ccusage daily --help` | 列出 `--by-agent` | 顶层 `daily` 下**生效**（行级 `agents` 数组；实测不带该标志时无此键），但 `ccusage claude daily --by-agent` 直接报 `Unknown daily option '--by-agent'`（exit 2）——Claude 专属 daily 走 `DailyArgs`，无该字段（`rust/crates/ccusage/src/commands/mod.rs:33-70`） | 同一选项在两个入口能力不同：**顶层可用、`claude` 子命令不可用** |

推论（**一致解释**）：ccusage 的文档随 PR 演进，但 `docs/` 与 `rust/` 之间没有强制一致性的门禁——
只有 `config-schema.json` 与 CLI 帮助是构建期生成的（§9），其余指南文本靠人维护。
对照代码时**先看测试与源码**，再看文档。

## 12. 与 TokenScope 的对照：可抄的与要避的

对照基准：本仓 `Cargo.toml` 版本 `0.3.0` 时的源码（对照事实逐条见同目录 `TS-FACTS.md`，
每条带 `文件:行号`）。本节只列**两侧都有代码证据**的差异；括号内的 `→` 左侧是 ccusage、右侧是 TokenScope。

### 12.1 结构性差异（会直接导致两边数字不同）

| # | 维度 | ccusage | TokenScope | 结论 / 影响 |
| --- | --- | --- | --- | --- |
| 1 | **Claude 缓存写入的时长档** | 区分 `cache_creation.ephemeral_5m/1h`，1 小时档按 `input × 2` 计价（`cost.rs:118-131`） | 只读 `cache_creation_input_tokens` 单一字段映射为 `cache_write`，`PriceRates` 只有一个缓存写槽位（`src/source/claude.rs:58-68,364`、`src/pricing.rs:200-210`） | **TokenScope 会低估 1 小时缓存写入的成本**：同一个 token 量在 ccusage 里可能按 2 倍 input 计价。这是最值得优先评估的一条 |
| 2 | **Codex 的 compaction / 回放增量** | 计入 `token_usage_record`（须被 `compacted` 记录认领、按 `response_id` 跨文件去重），并对父会话回放做前缀扣减（`codex/src/parser.rs:236,510-530`） | 明确忽略 `token_usage_record`（计 `ignored_token_usage_record`，作为双计防线），只取 `last_token_usage`（`src/source/codex.rs:4-6,314-325`） | ccusage **覆盖更广**（可能多算 replay/compaction），TokenScope **更保守**（可能少算）。两者不是谁对谁错，但口径差异要在文档里写明 |
| 3 | **Claude 去重键** | 三档键 + sidechain 回放路由 + 三条决胜规则（`claude/src/lib.rs:350-366`、`:198-212`） | `(session_id, record_id)` 单键，保留时间戳最晚者（`src/dedupe.rs:9,26-42`） | ccusage 会把「同 requestId 跨会话的复制」合并；TokenScope 不合并。网关/复制 transcript 场景下 ccusage 更小 |
| 4 | **价格表模型** | **单表 + 覆盖**：在线刷新按**键级整条替换**（`pricing.rs:1080-1100`），用户 override 字段级且最后执行（`:1036`），内置表默认 `or_insert`（三处无条件写入的例外见 §13.2 第 1 条） | **候选并存 + 取最高**：三来源各成候选、禁止跨候选拼价、完整候选优先再取最高费用（`src/pricing.rs:2304-2415`） | 两种设计目标不同：ccusage 求「一个确定的价」，TokenScope 求「多渠道下不给低估」。TokenScope 的 `reason`/`highest_complete_cost` 是 ccusage 没有的披露 |
| 5 | **内嵌价格表** | 有：编译期裁剪 LiteLLM + models.dev 快照（`build.rs:155-221`），离线永远可用 | 无：`Pricing::default()` 为空表，未收录模型恒为「未知」（`src/pricing.rs:1737-1750`） | ccusage 的「离线可用」以**覆盖范围窄**（裁剪白名单只含 `claude-`/`anthropic.*`/`gpt-`/`o3`/`o4-mini`/`openai/`/`azure/`/`zai/`/`openrouter/openai/` 前缀）为代价；TokenScope 以「首次启动需同步」为代价 |
| 6 | **5 小时计费窗口（blocks）** | 完整实现：UTC 整点锚 + 严格 `>5h` 断块 + gap 块 + burn rate/projection（`blocks.rs:53-160`、`:567-618`） | **未实现**（全 `src/` 无 block 语义） | 若要做：ccusage 的锚点/断块/gap 三规则可直接照抄，注意它按 **UTC** 对齐（§7） |
| 7 | **增量策略** | 每次全量重扫；省的是**读取**（`--since` 时按 mtime + 会话组并查集剪枝文件，`claude/src/paths.rs:117-172`）+ 行级 `memmem` 预过滤 + 按大小分块的并行读 | 每次串行全量解析；省的是**重解析**（`(size, mtime)` 指纹缓存 + 采集前后双取指纹，`src/report.rs:1262-1323`） | 两者互补：TokenScope 若引入 mtime 剪枝，必须**按会话组整体判断**（ccusage 的并查集就是为 sidechain 跨文件去重准备的） |

### 12.2 可直接借用的做法（附 ccusage 证据）

1. **三档去重键**：`requestId` 缺失是常态，键要按「有无 requestId → 是否 `msg_` 前缀 → 是否带 timestamp」降级（`claude/src/lib.rs:350-366`）。TokenScope 目前单键 `(session_id, record_id)`，遇到「一条回复写多行」的形态时行为不同，值得补一条不变量测试。
2. **sidechain 回放的独立路由**：回放会用**新 requestId** 重写历史，普通键匹配不到；ccusage 用「`message.id` + session + 至少一侧是 sidechain」的次键兜住（`lib.rs:237-266`、`:427-440`）。注意它**有 timestamp 守卫**，而 daily 实现漏了这条守卫（§13.2 第 2 条）——抄的时候不要抄漏。
3. **数字版本后缀保护**：候选键以数字结尾时，后缀「`-`/`.` + 数字」若不是 8 位日期就判为不同版本，拒绝匹配（`pricing.rs:2703-2722`）。TokenScope 用「分隔符边界 + 压缩键偏移」达到同类效果（`src/model_identity.rs:141-157`），两套规则可互相印证。
4. **字段级 override + 「值来源」标记**：`cache_*_explicit` 让「派生值可按比例缩放、显式值必须原样保留」成为可实现规则（`pricing.rs:1069-1070`、`:1832-1861`）。TokenScope 的 `RateSpec` 三态解决的是「缺失 vs 显式 0」，两者可以叠加。
5. **缺价三处披露**：JSON 标记（`missingPricing`）、汇总清单（`unpricedModels`）、stderr 警告（`output.rs:430-450`）。TokenScope 已有「未知」三态与横幅，可对照补齐「汇总清单」这一层。
6. **按文件大小贪心分块的并行读**（`common/src/lib.rs:49-81`）：实现只有 33 行，且天然保持输出顺序；若 TokenScope 将来要并行化采集，这是现成的模式（注意 ccusage 的并行是**文件级**，TokenScope 现在是来源串行，两者不冲突）。
7. **缓存写入的时长口径**（1h = `input × 2`）与**默认缓存价推导**（`cache_create = input×1.25`、`cache_read = input×0.1`）：可直接作为 TokenScope 扩展单一 `cache_write` 槽位时的取值依据（`cost.rs:7,118-131`、`pricing.rs:1085-1088`）。

### 12.3 建议规避的做法（附证据）

1. **同一语义两份实现**：Claude 的 `load_entries`（`claude/src/lib.rs`）与 `load_daily_summaries`（`claude/src/daily.rs`）是两套解析代码，靠 7 个等价性测试维持；但复核发现两者在 sidechain 回放路由上**确有结构差异**（daily 缺 timestamp 守卫，`daily.rs:493-496` vs `lib.rs:436-440`）——测试没覆盖的样本形态会给出不同总量。TokenScope 用单一管线 + 不变量测试是更稳的取向。
2. **「兜底表」语义与注释不一致**：`put_builtin_pricing` 声明只补缺键，实际有 3 处无条件覆盖（`pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`）。若抄这套结构，要么保持纯粹 `or_insert`，要么显式标注例外。
3. **厂商特例硬编码的规模**：`codex-auto-review` 的日期回退表、DeepSeek V4 三段价与 UTC 峰谷窗口（`pricing.rs:252-347`）、`pricing_alias` 三条别名——这些会随时间腐化。TokenScope 的「外置规则 + 数据文件」方向更好，但需要同样的测试覆盖才能保证不腐化。
4. **文档与代码无门禁**：§11 列了 9 条「文档与实现不符」（另 2 条是文档与代码一致的正面例子）。TokenScope 的 `config-schema.json` 式「构建期生成 + CI 校验」正是 ccusage 只在 schema 与帮助文本上做到、而指南文本没做到的事。
5. **`exact_only` 门禁的复杂度**：为了防止 `claude-opus-5-fast` 被基础模型价抢走，ccusage 引入跨数据源的门禁集合与三种查询语义（`contains` / `contains_any_spelling` / `id_spelled_by`，`pricing.rs:501-521`、`:1617-1683`）。TokenScope 的候选模型天然避免了这个问题（不覆盖、取最高），**不要**再引入单表覆盖语义。

### 12.4 一句话总结

ccusage 的强项是**口径的工程化**：把「同一条日志可能被写多行 / 被回放 / 被复制」这些脏现实，
用去重键、回放路由、压缩回放状态机一层层兜住，并用测试把这些规则钉死；
弱项是**结构膨胀**：双实现、跨源门禁、厂商硬编码与文档漂移。
TokenScope 的强项是**候选式定价与非破坏性历史库**；若要对齐，
优先补的三件事是：缓存写入时长档、Codex 增量口径的显式决策、以及 5 小时窗口的取舍。

## 13. 复核记录与残留未决问题

### 13.1 复核流程

| 轮次 | 执行者 | 动作 | 结果 |
| --- | --- | --- | --- |
| 第 1 轮 | 6 个独立研究代理（S1–S6） | 按「主干/Claude、Codex、定价与金额、聚合与报告、其余 16 适配器、配置与 CLI 与版本史」六路精读，各自产出带 `文件:行号` 的分片 | 6 份分片，合计约 25 万字符（约 33 万字节 UTF-8） |
| 第 2 轮 | 4 个对抗式复核代理（R1–R4） | **不复用研究结论**，重新打开源码逐条证伪：R1 复核 S1+S2（71 条抽样）、R2 复核 S3（80 条）、R3 复核 S4（58 条）、R4 复核 S5+S6（43 条） | 发现 25 处必须修正（含 3 处语义级、多处证据错位） |
| 第 3 轮 | 6 个原作代理 | 按 R1–R4 的「断言 / 实际 + `文件:行号`」逐条就地修正分片 | 6 份分片修正完毕（含 3 处语义级问题的改写） |
| 第 4 轮 | 2 个终审代理（A/B） | 对**装配后的完整文档**分段复核：A 查 §1–§8（61 条行号抽查）、B 查 §9–§13 与附录（73 条抽查 + 附录全量比对） | A：2 处必改 + 6 处装配问题；B：1 处硬错误（JSON 键名）+ 3 处口径问题 |
| 第 5 轮 | 主研究员修正 + 2 个验证代理 | 逐条修复终审清单并重新装配；R5 验证「必改项是否落地」，R6 审查「覆盖度与结构性缺口」 | R5：13 条必改项全部已修、0 未修，判定达到可交付标准；R6：3 个真缺口（项目名清洗链、行级容错横向表、符号链接立场）+ 2 处高价值漏项（选源判据、测试基建） |
| 第 6 轮 | 1 个补充研究代理（S7） | 按 R6 清单补齐缺口与漏项，写成 §14 横向专题 | 见 §14 |
| 第 7 轮 | 1 个复验代理（R7） | 复验 §14（22 条抽查）、第 5–6 轮改动（10 条逐条）、§12 的 TokenScope 侧引用（8 条）与全文结构 | 未落地项 0；4 处低危细节（阅读约定章号、§14.6 步号、zcode 分支描述、grep 计数）已修；判定**可交付** |
| 第 8 轮 | 主研究员（实测轮）+ 1 个复核员 | **构建 ccusage 并用本机日志真实运行**（`--help` / `daily` / `claude daily` / `codex daily` / `session` / `blocks` / `--mode` 三态 / `--no-cost` / `--by-agent` 两处入口），用脚本离线重放 `build.rs` 的裁剪逻辑与 `pricing.rs` 的匹配算法、用 `rustc` 复现金额格式化，并跑仓库自测（188 passed） | 闭环 7 条存疑项、修正 §8 一处实质错误；复核员独立复现（含解压真实构建产物逐键比对），推翻并改正了本轮一处关于 `--by-agent` 的错误结论；详见 §13.5 |
| 第 9 轮 | 4 个全文复核员（C1–C4） | **逐节读完**全文并按章抽查（C1 60 条 / C2 59 条 / C3 63 条 / C4 44 条，共 226 条），另做机械检查：全部 `文件:行号` 引用的越界扫描（2125 条、0 越界）与引用完整性检查 | 一致率约 98%；本轮修正 6 处实质/引用问题（`claude-3-5-haiku-20241022` 的命中路径、v19↔v20 的取值门控、§10.8 交叉引用错位、`types.rs` 缺 crate 限定、附录 A 位置列、`bytes.len` 因果），并为各章补「裸文件名约定」；详见 §13.6 |

### 13.2 第 2 轮发现的典型问题（保留以示证据强度）

- **语义级（S3）**：分片称「内置兜底表只 `or_insert`、绝不覆盖上游」，实际 `put_builtin_pricing` 有
  3 个键走无条件 `insert`（`rust/crates/ccusage-core/src/pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`），
  与源码注释声明的意图相反。
- **语义级（S1）**：分片漏掉「daily 侧的 sidechain 回放路由没有 timestamp 守卫」这条**结构差异**
  （`claude/src/daily.rs:493-496` vs `claude/src/lib.rs:436-440`）——它会改变「是否合并」，
  比已列的三条平局裁决规则更实质；修正后该分片明确写出「等价只在测试覆盖的样本形态下成立」。
- **证据错位（S3/S4/R3）**：多处行号指到了相邻函数（`gpt-5.4` 与 `gpt-5.4-mini`、
  `find_help_page` 与 `help_text_for_tokens`、`codex/speed.rs` 与 `codex/parser.rs`），
  以及把测试 helper 当作生产证据。
- **计数错误（S5）**：`apply_total_token_fallback` 的调用被写成「14 家」，实测是 12 家 / 14 处。
- **文档指控复核（R3）**：分片指控 `weekly-reports.md` 的 `modelBreakdowns` 结构过时——复核确认属实（§11 第 2 条）。
- **代理分歧裁定（主研究员）**：R4 称 `prepare_session_message_query` 在 `loader.rs:754`，
  S5 称在 `:752`；直接读源（`rust/adapters/opencode/src/loader.rs:752-755`）确认为 `fn` 行 **752**，
  754 是第二个参数行——裁定 S5 正确，R4 该条作废。

### 13.3 残留未决问题（未验证项，按影响排序）

> **状态更新（第 8 轮）**：下列 10 条中已有 7 条经实测闭环或部分闭环，
> 逐条状态与实测证据见 §13.5 的对照表；仍完全未闭环的是第 5、9、10 条（并发压力、
> antigravity 的 protobuf 映射、Codex 重写突发阈值的经验依据）。

1. **内嵌 LiteLLM 快照的真实键集**：`$OUT_DIR/litellm-pricing.json.deflate` 是编译产物。
   **（第 8 轮已闭环）** 该产物已被解压并与离线重放的键集**逐键逐值比对（534 键、0 差异）**，
   详见 §13.5；原「来自测试断言与推演」的限制已解除。
2. **运行时实测已完成（第 8 轮）**：本报告结论原为静态阅读 + 仓库内测试断言；
   第 8 轮已构建并运行 ccusage（`--offline`，本机 `~/.claude` / `~/.codex`），
   并跑了仓库自测（`cargo test -p ccusage-core --lib`：188 passed / 0 failed）。
   仍属推断的只剩「两套实现（entries / daily）在真实数据上是否分叉」这类需要跨实现对照样本的判断。
3. **`format_currency` 的舍入边界**（`{:.2}` 对 `x.xx5`）未做实验。
4. **在线端点与内嵌快照的差异**：`https://models.dev/api.json` 与 LiteLLM `main` 的实时内容未抓取比对。
5. **并发语义未压力验证**：`PricingMap.find_cache` 的「锁内读 → 锁外算 → 锁内写回」只在单线程测试中覆盖。
6. **`models-dev-pricing.json`（34 269 行）未全量比对**：只抽查了 8 处条目与 4 个尺寸数字。
7. **`models.dev` 生成器**（`nix/tools/models-dev-gen`）与运行期规则的逐条对齐未核。
8. **各 agent 的模型名形态差异**：仅 Claude / Codex / Pi 三条路径逐行核对；
   其余适配器只确认它们调用同一套价格加载与计价函数，`model` 字段是否已带 `-fast` 或 provider 前缀未逐一验证。
9. **antigravity 的 protobuf 字段号映射**是代码事实，但与真实 Antigravity 版本 schema 的对应关系无法在仓库内验证。
10. **`CODEX_REWRITTEN_BURST_PAUSE_MS = 1000` 的经验依据**只在注释里，未独立复现。

### 13.4 未决问题的性质

第 8 轮把「要求运行」的那部分逐条实测（§13.5）：内嵌键集、`format_currency` 舍入、
在线端点差异、models.dev 全量比对、模型名形态、statusline 默认值、models.dev 生成器实现
都已给出**确定结论**；余下 3 条（并发压力、antigravity 的 protobuf 映射、
Codex 重写突发阈值的经验依据）需要真实触发条件或额外数据，本次环境不具备，仍按未验证标注。

八轮复核（约 300 条静态抽查 + 30 余处修正 + 一轮构建/运行实测）后，**没有发现阻断性问题**；
本报告的行号均以 HEAD `57afdc2` 为基准，源码一旦推进即需重新核对。

### 13.5 第 8 轮：实测复核实录（构建 + 运行 + 离线重放）

前七轮全部是静态阅读。本轮把 §13.3 的残留未决项尽量闭环：**真的构建了 ccusage、真的用本机日志运行了它、并把编译期逻辑用脚本离线重放**，最后由第 8 轮复核员独立复现（复现一致率 7/8，其中一处结论被推翻并已改正，见本节末）。

**环境与方法**（可复现，脚本见 `ccusage-draft/evidence-round8/`）：

- 构建：`cargo build -p ccusage`（Rust 1.99.0）。`rust/crates/ccusage-core/build.rs` 要求
  `CCUSAGE_PRICING_JSON_PATH`，故先按仓库 `flake.lock` 里
  `nodes.litellm.locked.rev = d9123f459561754f41fd598a73b405b091d3bf84` 取
  `model_prices_and_context_window.json`（2 797 533 字节）注入；构建耗时 1 分 17 秒。
  **该 rev 的文件与 LiteLLM 当日 main 的 sha256 相同**，因此内嵌快照与线上现状一致。
- 产物核对（复核员补充的最硬证据）：直接解压构建产出的
  `target/debug/build/…/out/litellm-pricing.json.deflate`，其 **534 键与离线重放的键集
  逐键、逐字段、逐值 0 差异**——「重放等价」由推断升级为实测；内嵌 models.dev 快照
  也与其源文件深度相等。
- 运行：全部加 `--offline`（不联网、不写 HTTP 缓存），数据源为本机 `~/.claude` 与 `~/.codex`；
  本节只记录**结构、字段名与行为**，不含任何会话内容或用量数字明细。
- 离线重放：用 Node 脚本逐行复现 `build.rs:155-221`（裁剪 + 改名）与 `pricing.rs:2667-2808`
  （`pricing_key_matches` / `contains_pricing_key` / `suffix_starts_with_numeric_model_version` / `max_by`）。
- 仓库自测：`cargo test -p ccusage-core --lib` → **188 passed / 0 failed**（文档引用的测试确实存在且通过）。
- 格式化实验：用 `rustc` 编译一段复现 `output.rs:548-550` 的小程序。

**闭环结论（逐条对应 §13.3）**：

| # | 原存疑项 | 实测结果 | 状态 |
| --- | --- | --- | --- |
| 1 | 内嵌 LiteLLM 快照的真实键集 | 4084 条目经白名单裁剪后 **534 键**（筛掉 3550）；构成：`azure/` 260、`gpt-` 108、`claude-` 裸键 **23**、`o3`/`o4-mini` 12、`zai/` 16、anthropic 前缀族 115（区域前缀 86 + `anthropic.` 29）。**`claude-*` 裸键只覆盖 4.5 代及以后** | **已验证**（并与构建产物逐键比对） |
| 2 | 未做运行时实测 | 已运行 `--help`、`daily`、`claude daily`、`codex daily`、`session`、`blocks`（均 `--json --offline`），以及 `--mode` 三态、`--no-cost`、`--by-agent` 两处入口 | **已验证** |
| 3 | `format_currency` 的舍入边界 | Rust `{:.2}` 按 **f64 二进制真值**做 round-half-to-even：`0.005→$0.01`、`0.015→$0.01`、`0.025→$0.03`、`0.125→$0.12`、`1.005→$1.00`、`2.675→$2.67`；复核员另用真中值 `0.125/0.375/0.625/0.875/1.125/2.625/3.375` 全部取偶，独立证明 half-to-even | **已验证** |
| 4 | 在线端点与内嵌快照的差异 | `https://models.dev/api.json`（5 632 590 字节）：229 个 provider、8830 条条目、**4043 个去重 id**；内嵌快照 3049 键中 **3021 键名仍在在线目录**、28 个已消失（长尾）。**边界**：这里只比了**键名集合**，价格/档位/上限/`exactOnly` 未逐项比对 | **部分闭环** |
| 5 | 并发语义（`find_cache` 锁外计算） | 未做压力测试 | **未闭环** |
| 6 | `models-dev-pricing.json` 未全量比对 | 已做键集层面比对（见 4）；内嵌快照与源文件深度相等由复核员确认 | **部分闭环** |
| 7 | `models.dev` 生成器与运行期规则的对齐 | **已读生成器实现**：`nix/tools/models-dev-gen/gen.ts`（240 行）+ `compact.ts`（457 行）。信任分级与运行期一致（`MODELS_DEV_PROVIDER_TRUST` = owner 3 / platform 2 / reseller 1，`compact.ts:22-38` 对 `pricing.rs:649-651`）；候选裁决键为 trust → 长上下文档 → 显式 cache_read → 显式 cache_write → 上下文上限 → provider id / model id（字典序小者胜，`compact.ts:403-430`） | **已验证** |
| 8 | 各 agent 的模型名形态 | 已实测主链路（见下）；其余 adapter 未逐一实测 | **部分闭环** |
| 9 | antigravity 的 protobuf 字段号映射 | 本机无 Antigravity 数据 | **未闭环** |
| 10 | `CODEX_REWRITTEN_BURST_PAUSE_MS = 1000` 的经验依据 | 本机 Codex 日志未识别到「日志被重写」事件 | **未闭环** |

**由此修正的一处实质错误（§8 §4.5(a)）**：原文称 `claude-sonnet-4-20250514`
「阶段 A 精确命中即可」——**错**。实测：内嵌表没有该裸键，真实路径是**模糊匹配**
命中区域前缀的 Bedrock 键（实测命中 `global.anthropic.claude-sonnet-4-20250514-v1:0`，长度 46 为最长；
候选另含 `anthropic.` / `us.` / `eu.` 三个前缀的变体——**`apac.` 只存在于未裁剪的完整 LiteLLM，
不在内嵌表**，白名单不含该前缀）。仓库测试
`embedded_pricing_resolves_overlapping_model_keys_exactly`（`pricing.rs:4931-4955`）
本身就在证明这一点——它断言的是 `find("claude-sonnet-4-20250514")` 与
`find("anthropic.claude-sonnet-4-20250514-v1:0")` 的 **input 相等**（即前者落到了后者的价上），
而不是断言裸键存在。`claude-3-5-haiku-20241022` 属于另一条路径：它是**内嵌兜底表的精确命中**（`pricing.rs:2191-2192`
用 `put_builtin_entry` 写入该 id），不进模糊扫描；三个同价的 Bedrock 变体（`anthropic.` / `eu.` / `us.`，
input 均 `8e-7`）因此不会被选中——这也是 `pricing.rs:4951-4954` 的断言无法区分路径的原因。
**脚本边界**：`replay-match.js` 只加载 LiteLLM 裁剪表，**不含内置兜底表**，所以它会把这个名字误报成
模糊命中（脚本的 tie-break 曾因方向写反取到 `us.`，修正后与真实语义一致）；正确结论以本段为准。
**推论**：`claude-*` 裸键缺旧型号时，ccusage 仍能给出价格，但给的是**区域/Bedrock 渠道价**——
这解释了两件事：为什么旧模型不会报 `unpricedModels`，以及为什么其单价可能与 Anthropic 直连账单不同。

**版本边界保护的实证（归因修正）**：真正的样例是 `gpt-5.4-9` 与 `claude-opus-5-9`——
表内有 `gpt-5.4` / `claude-opus-5`，后缀 `-9` 命中
`suffix_starts_with_numeric_model_version` 的拒绝分支，两者在 534 键表上均**无模糊命中**。
此前举例的 `gpt-500` 其实是被**边界检查**挡住的（后缀以 `'0'` 开头，属字母数字而非边界），
`opus550` 则连双向子串关系都不成立——两者都不能证明版本保护。另：`claude-opus-4-6-fast`
无精确键，模糊命中基础键 `claude-opus-4-6`，fast 倍率由 `fast-multiplier-overrides.json` 的
`normalized_prefix["claude-opus-4-6"] = 6.0` 提供（复现 `matches_model_suffix` 的 `rfind` + `-` 判定）。

**旧模型的两道防线（实测补充）**：内嵌 models.dev 快照里，旧模型以**带目录前缀**的键存在——
实测查到 `anthropic/claude-sonnet-4-20250514`、`eu./global./us.anthropic.claude-sonnet-4-20250514-v1:0`。
因此即使主表（LiteLLM）失手，模糊匹配仍能在第二道防线命中 `anthropic/claude-sonnet-4-20250514`
（`/` 是合法边界）。生成器注释也解释了这类保留策略：reseller 的价是「可信目录不再发布时的最后手段」，
并明确举了"退役的 Claude 3 发布版只有 reseller 还在列"（`compact.ts:31-37`）。
注意归一化后的拼写（`anthropic--claude-3-haiku` 这类）也出现在快照里，说明同一目录内存在多种拼写形态。

**运行时行为验证（与文档既有描述逐项对照）**：

| 观察点 | 实测 | 对应文档 |
| --- | --- | --- |
| `claude daily --json` 顶层键 | `daily`、`totals` | §7 §8.1 一致 |
| 日行的键 | `cacheCreationTokens, cacheReadTokens, date, inputTokens, modelBreakdowns, modelsUsed, outputTokens, totalCost, totalTokens` | §7 §8.1 一致 |
| `codex daily --json` 日行的键 | 额外含 `costUSD`、`models`、`reasoningOutputTokens` | §5 §10 一致 |
| `session --json` 顶层与行键 | `session` / `totals`；行含 `agent`、`metadata`、`period` | §7 §10 一致 |
| `blocks --json` 块键 | `actualEndTime, burnRate, costUSD, endTime, entries, id, isActive, isGap, models, projection, startTime, tokenCounts, totalTokens` | §7 §6 一致（含 `isGap`、`burnRate`、`projection`） |
| `--no-cost` | 输出中 `totalCost`/`costUSD`/`cost` 全部消失 | §7 §8.3 一致 |
| `--mode` 三态 | `display` → `totals.totalCost = 0.0`；`auto` 与 `calculate` 数值完全相同。**边界**：本机日志没有 `costUSD` 字段，所以「display 给 0 是因为它只信 `costUSD`」是**一致解释**而非直接实测 | §8 §6 一致 |
| `--jq` | **不在** `--help` 文本里（但选项可解析） | §11 第 8 条一致 |
| `claude statusline --help` | `--offline` 默认 **true**、`--cache` 默认 true、`--refresh-interval` 默认 1 | 闭环 §8「未验证 / 存疑」第 8 条的疑问 |
| `codex daily --help` | `--speed [auto\|standard\|flex\|fast]`，默认 auto | §5 §6 一致 |
| `codex daily --speed` 三态 | `auto` 与 `standard` 数值**不同**、`fast` 高于 `standard` ——说明 `auto` 确实读取并采用了本机日志记录的 service tier（若日志无 tier 记录，auto 会退化成 standard），`--speed` 的强制覆盖也确实改变金额 | §5 §6 一致 |

**`--by-agent` 的适用范围（本条曾被本轮实测误判，已按复核结论改正）**：
该选项在**顶层 `daily` 与无子命令路径**下**生效**——`ccusage daily --json --by-agent --offline`
的行级键包含 `agents`（数组，实测元素数 ≥1），不带该标志时同一位置只有 `agent`；
`ccusage --json --by-agent --offline` 同理。它**不被 `claude` 子命令接受**：
`ccusage claude daily --json --by-agent --offline` 直接报
`Unknown daily option '--by-agent'` 并以 exit 2 退出（Claude 专属的 `DailyArgs` 没有该字段，
见 `rust/crates/ccusage/src/commands/mod.rs:33-70`；统一报告侧由
`ccusage-cli-parser/src/parser.rs:375`、`:440-473` 解析，`ccusage-adapter-all/src/lib.rs:41` 消费）。
**教训记录**：本轮初次实测把检查点放在**顶层键**（顶层只有 `daily`/`totals`），因此误得
「选项被接受但静默无效」的结论；复核员用**行级键**复现才暴露问题——`agents` 是行级键。

**仍未闭环的 3 条**（保留在 §13.3）：并发压力、antigravity 的 protobuf 映射、
Codex 重写突发阈值的经验依据——它们都需要真实触发条件或额外数据，本次环境不具备。

**重放脚本的适用边界**（写入证据目录，避免误用）：
`replay-match.js` 实现了 `pricing_key_matches` 与最长键裁决，但**未实现 `exact_only` 门禁**，
也**不加载内置兜底表**（`put_builtin_pricing`）——因此它会把 `claude-opus-5-fast` 判成命中
`claude-opus-5`、把 `claude-3-5-haiku-20241022` 判成模糊命中区域键，而真实运行时分别由
models.dev 档位条目与兜底表精确键作答。用它下结论时必须叠加这两层。

### 13.6 第 9 轮：全文逐节复核

第 8 轮之后又做了一次**面向全文**的复核（4 路并行，每路逐节读完自己负责的章节并按章抽查）：

| 路 | 范围 | 抽样 | 结果 |
| --- | --- | --- | --- |
| C1 | §1–§5（L45–L1079） | 60 条 | 59 条一致；数字/阈值/优先级类零偏差 |
| C2 | §6–§8（L1080–L2747） | 59 条 | 全部一致（含独立解压构建产物复核 534 键构成） |
| C3 | §9–§11（L2748–L3343） | 63 条 | 62 条一致；并实测复验 `--by-agent` 与 `--jq` 两个争议点 |
| C4 | §12–§14 + 附录（L3344–L3890） | 44 条 | 43 条一致；全局结构终检通过 |

另做了两项机械检查（覆盖全文，非抽样）：

- **行号越界扫描**：用脚本提取全部 `文件:行号` 引用并按每个文件的实际行数比对——**可解析的 2125 条 0 越界**；同名文件的歧义引用改用「同名文件最大行数」做宽松上界，同样 0 越界。**扫描边界**：另有约 159 条以简写路径（`loader.rs:123` 之类）出现且未匹配到唯一文件的引用，与 33 条 v19 引用，不在该扫描范围内。
- **引用完整性**：`§N` 章级引用共 73 处、覆盖 13 个章号，全部指向存在的章节；章内小节引用的读法已写进开头的「阅读约定」。

**本轮修正的问题（6 处）**：

1. **§8 §4.5(a) 与 §13.5 的 `claude-3-5-haiku-20241022` 归属**（实质错误）：它不是模糊命中，
   而是**内置兜底表的精确键**（`pricing.rs:2191-2192`）——精确阶段直接作答，不进模糊扫描。
   该误判的来源是重放脚本缺兜底表 + 三条 Bedrock 候选同价（测试无法区分路径）；已改正，
   并把脚本缺口写进证据目录的「使用边界」。
2. **§10.4 与 §10.8-4 的 v19↔v20 结论**（实质错误）：原文称「取值主路径两版一致」——错。
   v19 是**无条件**优先 `lastUsage`（`parser.ts:205-212`），v20 多了「累计值必须前进」的门控，
   未前进时丢弃 `last_token_usage` 改走累计差分（`codex/src/parser.rs:483-494`）。
   已按 §5.3 的口径改写，两处不再矛盾。
3. **§10.8 的交叉引用错位**：第 3/4/5/9 条的 `（§7.x）` 实为「第 7.x 小节」的旧编号，
   在终稿编号下指向无关内容；已改为 §4.4 / §5.3 / §9.3 / §8（并把 §2.4、§1.5 显式化为 §9.2.4、§9.1.5）。
4. **§11 第 2 行的 `types.rs:175` 缺 crate 限定**：裸写会指到 `ccusage-cli/src/types.rs` 的
   `AgentCommandArgs.shared`；已补为 `rust/crates/ccusage-core/src/types.rs:175`。
5. **附录 A 的「日志级别」位置列**：`logger.rs` 只有 15 行、不含 0–5 级别表；映射实际写在
   `docs/guide/environment-variables.md`，已改正并注明封装文件的真实规模。
6. **§14.1.3 的 `bytes.len() >= 10` 因果**：它不是「索引安全下限」（匹配 `Users\` 只需 ≥9），
   已改述为「作者留余量的保守下界」。

此外给 §4/§5/§6/§7/§9 五章的引言补了**「本章裸文件名约定」**（裸 `parser.rs` / `loader.rs` 等
分别指哪个 crate），从源头消除同名文件的行号歧义；并在开头的阅读约定里补了对应说明。

**本轮仍判定未闭环的项**（与 §13.3 一致，加复核员列出的补充）：并发压力、antigravity 的
protobuf 映射、Codex 重写突发阈值的经验依据、`copilot` 的 credits 归因、`opencode` 的 SQL 细节、
`pi` 的 replay 剔除点、`json_float` 的极端输入、`--single-thread` 与 `blocks`/`statusline` 的未运行入口等。

**代理分歧与裁定（1 处）**：C4 曾把 `gpt-500` 无命中的原因归给「版本后缀规则的第一条」，
与源码顺序不符——`suffix_allows_pricing_key_match`（`pricing.rs:2693-2701`）**先**做边界检查，
`'0'`（字母数字）在此即返回 `false`，执行流根本到不了 `suffix_starts_with_numeric_model_version`。
文档保留「被边界检查挡住」的归因（版本保护的实证样例早已换成 `gpt-5.4-9` 与 `claude-opus-5-9`）。

## 14. 横向专题：项目名、坏行容错、路径立场与测试基建

本节补齐第 5 轮审查（R6）列出的 3 个横向缺口与 2 处高价值漏项，作为 §1–§13 的横向补充：项目名的推导与清洗链（§14.1）、行级坏数据策略对照（§14.2）、符号链接与路径规范化立场（§14.3）、选源准入判据（§14.4）、测试基建（§14.5）。

- 样本：`ccusage` 只读仓库 HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（v20.0.31）；本节所有行号均为本次实测（`read` / `grep` 逐文件核对），与各分片既有编号独立。
- 方法：静态阅读为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5）。未修改任何样本或源码。
- 口径：涉 18 家适配器时，Claude / Codex 与 §4、§5 互相引用，不重复展开；凡本节未逐行读到的分支，在表中标注核实方式并在 §14.6 列明。

### 14.1 项目名的推导、清洗与别名

**结论**：ccusage 的**项目身份**是适配器抽出的原始 `project` 字符串（Claude 侧是 `~/.claude/projects/<slug>` 的那一段目录名，`rust/adapters/claude/src/paths.rs:269-288`），分组、过滤、JSON 输出全用它；`format_project_name` 只在**表格分组头**做一次展示用清洗，别名表同样只影响展示。**展示名不是身份。**

#### 14.1.1 别名表：`parse_project_aliases`

| 项 | 事实 | 证据 |
| --- | --- | --- |
| 入口 | 只有两个：CLI `--project-aliases <RAW>`、配置 `commands.daily.projectAliases`（camelCase） | `rust/crates/ccusage-cli-parser/src/parser.rs:495-497`、`rust/crates/ccusage-config/src/config_schema.rs:433`、`:640` |
| 格式 | 逗号分隔 `key=value`；逐个 `split_once('=')`，两侧 trim，任一为空即丢该对，重复 key 后者覆盖（`collect` 语义） | `rust/crates/ccusage-core/src/project_names.rs:3-17` |
| 官方文档 | 与实现一致（`"uuid-project=Frontend App,long-name=Backend API"`） | `docs/guide/daily-reports.md:276`、`:282`、`docs/guide/configuration.md:224` |
| 优先级 | 精确命中原始 key → 直接返回；否则拿**清洗后**的名字再查一次别名 | `project_names.rs:19-25` |
| 生效范围 | 唯一调用点是表格渲染，且仅在 `group_projects` 为真时使用 | `rust/crates/ccusage-core/src/output.rs:260`、`:270-274` |

**注释与实现不一致（新发现）**：`config_schema.rs:433` 的 doc 注释写「JSON object or path mapping project paths to display aliases」，实现里没有 JSON 对象或文件路径分支——全仓 `project_aliases` 的消费者只有 `parse_project_aliases` 与 `output.rs:260`（grep 命中 16 处，均为传参/构造）。以代码与官方文档为准。

#### 14.1.2 清洗链：`parse_project_name` 的判断顺序

逐字对应 `project_names.rs:27-114`，**顺序不可交换**：

| 步 | 条件 | 动作 | 行号 |
| --- | --- | --- | --- |
| 1 | 空串或字面量 `unknown` | 返回 `Unknown Project`（唯一兜底字面量） | `:28-30` |
| 2 | `is_windows_users_path` 为真 | 按 `\` 切分，定位第一个 `Users` 段：其后 ≥3 段取 `index+3..`，否则 ≥2 段取 `index+2..`，用 `-` 重新拼接 | `:32-40` |
| 3 | 否则以 `-Users-` 或 `/Users/` 开头 | 按 `-` 或 `/` 切分（丢弃空段），同样定位 `Users` 段后取 `index+3..` / `index+2..` | `:41-57` |
| 4 | 其他形态 | 只做首尾 `/\-` 修剪，不改内容 | `:58-62` |
| 5 | 段数 ≥5 **且**全部字符属于 `[0-9a-fA-F.-]` | 只保留**最后两段**（hex/哈希形态压缩） | `:63-70` |
| 6 | 含 `--` | 在**第一个** `--` 处截断，只留前半 | `:71-73` |
| 7 | 含 `-` 且长度 >20 | 分段过滤：长度 ≤2 的段丢弃；小写后等于噪声词的段丢弃（`dev`/`development`/`feat`/`feature`/`fix`/`bug`/`test`/`staging`/`prod`/`production`/`main`/`master`/`branch`） | `:74-96` |
| 7b | 过滤后 ≥2 段 | 取最后两段拼接；结果 <6 字符且过滤后 ≥3 段则改取**最后三段**；否则保持原串不缩 | `:97-106` |
| 8 | — | 再修一次首尾 `/\-` | `:108` |
| 9 | 若已被清空 | 回退**原始未清洗**的 `project`（保证永不为空串） | `:109-113` |

两条易被误读的性质：

- 步 2/3 的「隔段取尾」是把 `Users/<用户>/<...>` 的层级压平：`/Users/alice/Development/ccusage` 经 `Users` 分段后取 `index+3..` → `ccusage`（测试 `project_names.rs:155-158`），而 `/Users/alice/ccusage` 只有 3 段（`index=0`），走 `index+2..` → `ccusage`（`:151-154`）——即用户目录下一级与多级都得到末段名。
- 步 1 只认小写 `unknown`，步 9 的兜底回退发生在所有裁剪之后，因此清洗链**不会**产生空展示名（除非原始即为 `unknown`）。

#### 14.1.3 `is_windows_users_path` 判据

```text
(bytes.len() >= 10 && bytes[1] == b':' && bytes[2] == b'\\' && bytes[3..].starts_with(b"Users\\"))
|| project.starts_with("\\Users\\")
```

证据：`project_names.rs:116-123`。判据的性质与边界：

- 只认两类形态：`X:\Users\...`（盘符 + 反斜杠 + `Users\`）与 `\Users\...`（无主机前缀的形态）。
- `bytes.len() >= 10` 是一个**保守下界**：要匹配 `Users\` 只需长度 ≥9（前缀 3 字节 + 6 字节字面量），代码取 10 是留余量，并非某处索引的最小安全值。
- **不走本分支**的反例：`C:/Users/...`（正斜杠）、`/Users/...`（落步 3）、`c:\users\...`（小写 `users`）。测试只覆盖 `C:\Users\phaedrus\Development\ccusage`、`\Users\phaedrus\Development\ccusage`（`rust/crates/ccusage/src/main.rs:149-161`）与 `/Users/alice/*`、`-Users-alice-ccusage`（`project_names.rs:147-163`），大小写与正斜杠形态**无测试**。

#### 14.1.4 调用点

| 调用点 | 作用 | 证据 |
| --- | --- | --- |
| 表格分组头 | `group_projects` 为真且 `project` 变化时插一行 `Project: <清洗名>`；分组比较用的是**原始** `project`（`current_project != Some(project)`），所以清洗后同名的不同 slug 仍各起一组 | `output.rs:260-276`；`project_header_row` `:464-467` |
| 谁传 `group_projects` | daily：`args.instances \|\| args.project.is_some()`；monthly/weekly、`bucket`、`session` 一律传 `false` | `rust/crates/ccusage/src/commands/mod.rs:35-39`、`:61-68`、`:106`、`:135-142`、session 渲染 `:172-179` |
| CLI 选项 | `--project-aliases` 仅挂在 `claude daily` 子命令上（`-p/--project` 同处） | `ccusage-cli-parser/src/parser.rs:492-499` |
| 配置键 | 结构体字段、`from_map` 读取、JSON Schema 生成三处 | `ccusage-config/src/config_schema.rs:428-434`、`:640`、`:1085` |

#### 14.1.5 展示名 vs 身份（key）

| 维度 | 身份（key） | 展示名 |
| --- | --- | --- |
| 分组 | `UsageSummary.project` = 适配器抽出的原始串 | 仅在分组头渲染时套 `format_project_name` |
| JSON | `summary_json` 直接输出 `row.project`（未清洗）；`group_project_output` 用 `row.project.unwrap_or("unknown")` 作 `BTreeMap` key | `output.rs:48-50`、`:147-155` |
| `--project` 过滤 | 先判「是否为合法单段目录名」，是则只收该目录（`projects/<filter>`），否则全量收；随后条目层 `entry.project.as_ref() != filter` 精确比较——比较对象是**原始 slug**，不是展示名 | `claude/src/paths.rs:65-79`、`:248-254`；`claude/src/daily.rs:103-106` |
| `--instances` 输出 | key 与 `detected` 判定都用原始 project | `ccusage/src/commands/mod.rs:45-50`、`claude/src/daily.rs:65-71` |

推论：`-Users-alice-foo` 与 `/Users/alice/foo` 可能都显示为 `foo`，但它们是两个组、两个 JSON key、两种 `--project` 取值；别名也不改变分组，只改变表头文字。此立场与 TokenScope 的「展示名（路径末段）不是身份、仅同名不合并」一致（`CLAUDE.md`「项目身份」条）。

### 14.2 行级坏数据策略对照

**结论**：18 家适配器共享同一批宽松反序列化零件（`rust/adapters/common/src/jsonl.rs`），但**没有统一的坏行策略**——绝大多数按「丢行 / 丢字段 / 丢文件」三档静默降级，只有 `antigravity` 会把单行/单库的坏数据升级成对用户可见的错误。**全仓没有任何坏行计数**。

#### 14.2.1 统一零件

| 零件 | 语义 | 行号 |
| --- | --- | --- |
| `records::<T>` | `byte_lines` 切行 → 可选 `LinePrefilter`（memmem 预筛）→ `from_slice::<T>(line).ok()`；**失败行静默丢弃**，不计数、不打日志 | `jsonl.rs:47-62`；注释明写 "silently skipped" `:29-30`、`:23-27` |
| `lenient_u64` | 非 u64（浮点/字符串/null/负数/缺失）一律 0 | `:75-84` |
| `lenient_i64` | 整数才 `Some`，浮点被拒 | `:95-101` |
| `lenient_f64` | 任意 JSON 数字都 `Some` | `:112-118` |
| `lenient_object` | 非对象或嵌套对象反序列化失败 → `None`（**不丢整行**） | `:131-141` |
| `lenient_array` / `lenient_vec` | 数组内坏元素丢弃；前者区分「空数组 vs 非数组」，后者折叠成空 `Vec` | `:155-170` / `:180-186` |
| `non_empty_string` | 非字符串、trim 后为空 → `None` | `:196-202` |
| 预筛语义 | `LinePrefilter` 的标记必须出现在「适配器会接受的每一行」上，否则该行在任何 JSON 解析前就被跳过 | `:23-27`、`:54-59` |

零件自带的回归测试明确了「宽松到什么程度」：浮点 token 变 0、字符串 token 变 0（`:259-271`）、非对象 cache 字段不丢行（`:305-349`）、数组内坏元素跳过（`:352-395`）。

#### 14.2.2 四档定义

- **A 丢行**：单行解析失败或被判定不适用 → 静默跳过，继续处理文件内其余行。
- **B 丢字段**：单字段类型/形态意外 → 取 `None`/`0`/空集合，保留该行其余可用信息（`lenient_*` 家族）。
- **C 丢文件**：文件或库级失败（读不到、打开失败、整体反序列化失败、schema 不符）→ `debug_log` + 空结果，报告照常渲染。
- **D 升级为错误**：返回 `Err` 并冒泡，**中断该 agent 的整份报告**。

#### 14.2.3 18 家分档表

| # | agent | 档位 | 关键证据（行号） | 本节核实方式 |
| --- | --- | --- | --- | --- |
| 1 | `claude` | C + A | 文件读不到 → 空 `LoadedFile`（`claude/src/lib.rs:505-507`）；逐行 marker 预筛、反序列化失败、时间戳失败、`is_valid_usage_entry` 为假 **四种** 情况均 `continue`（`:510-523`） | 逐行读 |
| 2 | `codex` | A（+ C，见 §5） | 读行失败即 `return None`（`codex/src/parser.rs:139-144`）；行分类不符、反序列化失败、无 token、无时间戳依次 `continue`（`:145-167`）；`replay.rs:404` 反序列化失败走 else 分支 | 逐行读 |
| 3 | `opencode` | A + B | 消息 JSON `from_slice(...).ok()?`（`opencode/src/parser.rs:275`、`loader.rs:542`）；字段级 `lenient_*`（`parser.rs:17-114`）；loader 内多处 `continue`（`loader.rs:335-445`、`:585-651`） | grep 定位 + 邻段 |
| 4 | `amp` | C + A | 整文件反序列化失败 → 直接返回空 `Vec`（`amp/src/parser.rs:96-101`）；事件缺时间戳/模型/tokens 逐个 `continue`（`:131-142`）；loader 把 `Err` 降级为 debug + 空（`loader.rs:22-30`） | 逐行读 |
| 5 | `droid` | **契约上 D，落地 C** | parser 对非 `NotFound` 读错与 JSON 解析失败返回 `Err`（`droid/src/parser.rs:33-43`），但 loader 用 `unwrap_or_else` 转成 debug 日志 + `None`（`loader.rs:29-40`） | 逐行读 |
| 6 | `codebuff` | C + A | 文件 `fs::read(path)?` 失败上抛、整体 JSON 失败返回空（`codebuff/src/parser.rs:41-44`）；行级 `continue`（`:49-59`、`:174-180`）；loader 降级（`loader.rs:30-39`） | 逐行读 |
| 7 | `hermes` | A + B | SQLite 列读失败即 `None`（`hermes/src/parser.rs:24-27`）；空桶且零成本的行丢弃（`:41-49`）；库打开失败 debug + 空（`loader.rs:41-53`） | grep 定位 + 邻段 |
| 8 | `pi` | A + B | 行级多重 `continue`（`pi/src/parser.rs:321-350`、`:508`）；字段级 `lenient_*`（`:22-92`） | grep 定位 + 邻段 |
| 9 | `goose` | A + C | 列读 `.ok()?`（`goose/src/parser.rs:25-27`、`:107-116`）；库打开/查询失败 debug + 空（`loader.rs:75-89`、`:101`） | grep 定位 + 邻段 |
| 10 | `openclaw` | A + B | 行级 `continue`（`openclaw/src/parser.rs:192-201`）；SQLite 行级 `continue`（`sqlite.rs:124-163`）；字段级 `lenient_*`（`parser.rs:25-155`） | grep 定位 + 邻段 |
| 11 | `kilo` | A + C | 库打开失败 / prepare 失败 debug + 空（`kilo/src/loader.rs:51-66`）；每列读失败 `continue`（`:71-79`） | 逐行读 |
| 12 | `copilot` | A + B | 整文件 `records::<CopilotRecord>` 收集（`copilot/src/parser.rs:131`）；会话状态行级 `continue`（`:254-303`）；属性级宽松解析（`:167-224`） | grep 定位 + 邻段 |
| 13 | `gemini` | C + A + B | 单文件 `fs::read_to_string(path)?` 只在该文件内上抛，loader 统一降级（`gemini/src/parser.rs:117`、`loader.rs:25-37`）；`records` 传 `None` 预筛（`parser.rs:176`）；行级 `continue`（`:190-202`） | 逐行读 |
| 14 | `antigravity` | **D（唯一真中断）** | 打开失败 → `cli_error`（`antigravity/src/parser.rs:146-152`）；`PRAGMA page_count` 缺值也 `Err`（`:306-309`）；行读失败、blob 解析失败逐条 `cli_error`（`:339-356`、`:381-404`）；loader 用 `?` 直接冒泡（`loader.rs:24`） | 逐行读 |
| 15 | `kimi` | A + B | 行级过滤 + `filter_map`（`kimi/src/parser.rs:127-128`）；字段级 `lenient_*`（`:24-102`） | grep 定位 + 邻段 |
| 16 | `qwen` | A + B | `records::<QwenLine>` + 预筛（`qwen/src/parser.rs:111`）；字段级 `lenient_*`（`:32-55`） | grep 定位 + 邻段 |
| 17 | `grok` | C + A + B | 整会话文件 `Err` 降级为 debug + 空（`grok/src/loader.rs:33-42`）；行级 `continue`（`parser.rs:219-230`、`:255-275`）；字段级 `lenient_*`（`:34-129`） | 逐行读 |
| 18 | `zcode` | A + C | 行级有效性判定（空 id/空 session/空 model/时间戳 ≤0/零用量 → `None`，`zcode/src/parser.rs:34-40`、`:59-61`）；列读 `.ok()?`（`loader.rs:278-289`）；打开、`PRAGMA busy_timeout`、`PRAGMA query_only`、schema 任一失败 → debug + 空（`loader.rs:90-117`） | 逐行读 |

上表已覆盖全部 18 家；标注「grep 定位 + 邻段」的 8 家（见 §14.6 第 9 条）只在证据粒度上弱于逐行读，其完整上下文链见 S5 分片 §4.4/§4.5，本节不重复。

#### 14.2.4 特殊者

- **`antigravity` 是唯一的 D 档**，且是**跨文件、跨行**的：`loader.rs:23-34` 对每个库用 `?`，因此**单个损坏或结构不符的 `conversations/*.db` 会让整个 antigravity 报告失败**（`CliError` 冒泡到命令层），其余库与其余 agent 不受影响。注释解释了「空库才跳过」（`:26-32`：`None` 表示未初始化的空快照），但没有任何「单库失败降级」分支。
- **`droid` 的「硬错误」只停在函数契约层**：`parser.rs:32-43` 的 `Err` 被 `loader.rs:29-40` 吸收为 debug 日志，净效果与 C 档相同。把它读成「droid 会因一个坏 `*.settings.json` 崩掉」是错的；反之把它读成「droid 与别家完全一样」也不准确——它的 `Err` 是**语义信息**（文件被跳过这件事在 `RUST_LOG=debug` 下可见）。
- **`claude` 是丢行条件最多的一家**：一个「坏行」只要满足「无 `"usage":{` 标记 / 反序列化失败 / 时间戳不可解析 / 未通过 `is_valid_usage_entry`」中的任一条就被丢（`claude/src/lib.rs:509-523`），而**文件打不开只影响该文件**（`:505-507`）。

#### 14.2.5 坏行计数结论

`grep -r 'bad_lines|badLines|bad_line'` 在整仓（含 `rust/`、`apps/`、`packages/`、`docs/`）**0 命中**。结合零件的注释与实现：

- 坏行**没有任何计数、没有输出、没有告警**；用户从报告里无法得知「有日志被丢弃」（`jsonl.rs:29-30`、`:47-62`）。
- 唯一可见的降级痕迹是**文件级** `debug_log`（例：`amp/src/loader.rs:24-29`、`grok/src/loader.rs:34-41`、`gemini/src/loader.rs:31-35`、`kilo/src/loader.rs:54-58`、`zcode/src/loader.rs:93-97`），且默认 `info` 级别下不可见。
- 对照 TokenScope 的 SF08（异常行计 `bad_lines` 跳过、累计溢出则明确报错）——**这是 ccusage 明确缺失、而 TokenScope 已具备的能力**；若要与 ccusage 行为对齐，需要显式决定「不计数」是否可接受，而不是默默继承。

### 14.3 符号链接与路径规范化立场

**结论**：ccusage 并存两种取向——**遍历/发现侧坚持词法（lexical）判断，拒绝跟随符号链接**；**去重/重叠判定侧使用 `canonicalize` 求真实同一性**，且 5 处生产调用全部带失败回退。二者不是矛盾，而是服务于不同问题；但边界处存在一处语义接缝（见本节末）。

#### 14.3.1 遍历侧：不跟随符号链接

| 位置 | 事实 | 证据 |
| --- | --- | --- |
| 通用递归收集 | 用 `entry.file_type()`（即 `symlink_metadata` 语义，**不跟随**）判类型：既不算文件也不递归目录；`fs::read_dir` 失败静默 `return`（目录不存在 = 空结果） | `rust/adapters/common/src/lib.rs:18-34`（`:24-32` 的判断、`:19-21` 的静默返回） |
| openclaw JSONL | 显式 `if file_type.is_symlink() { continue; }` | `openclaw/src/paths.rs:51-58` |
| openclaw SQLite | `is_directory_without_symlink` / `is_file_without_symlink` 用 `symlink_metadata` 判定 | `openclaw/src/sqlite.rs:51-57`，使用点 `:25`、`:39`、`:43` |
| openclaw 不变量 | 模块注释写死立场：「Directory traversal stays lexical (no symlink following, no canonicalization)：discovery must agree with the JSONL walk in `paths.rs`, which also refuses symlinks」 | `openclaw/src/sqlite.rs:14-21` |
| openclaw 测试 | `ignores_symlinked_database_paths`（`#[cfg(unix)]`）：文件软链 + 目录软链两种形态都断言「一个都不收」 | `openclaw/src/sqlite.rs:354-380` |
| opencode 探测 | `has_json_file` 同样以 `file_type()` 判，注释给出两条理由：跟随会让「探测说有、收集却拒读」自相矛盾，且软链成环会递归到栈溢出 | `opencode/src/loader.rs:206-209`、`:210-224` |

#### 14.3.2 canonicalize 派：全仓 5 处生产调用

全仓 `canonicalize` 共 9 处命中，其中 4 处落在测试代码（`zcode/src/paths.rs:171-172`、`:211` 三处调用，`antigravity/src/paths.rs:136` 命中的是测试函数名 `deduplicates_canonicalized_override_aliases`）；**生产调用恰好 5 处**：

| 位置 | 用途 | 失败回退 |
| --- | --- | --- |
| `antigravity/src/paths.rs:68` | 跨根去重 `conversations/**.db`（同库被多个根覆盖只留一份）；`seen` 存 canonical，`paths` 仍存**原路径** | `unwrap_or_else(\|_\| path.clone())` |
| `goose/src/paths.rs:28` | 三条候选（`~/.local/share/goose`、`~/Library/Application Support/goose`、`~/.local/share/Block/goose`）解析为同一文件时去重 | `unwrap_or(path)`，随后仍要 `is_file()`（`:29`） |
| `zcode/src/paths.rs:32` | 库路径解析：canonicalize **之后**再判 `is_file()` | 失败 → debug 日志 + `continue`（`:41-50`） |
| `zcode/src/paths.rs:117` | 每个根的 canonicalize（根去重） | 失败 → 丢弃该根；**仅配置根**打 debug 日志（`configured` 分支 `:120-128`），默认根静默丢弃 |
| `ccusage-adapter-all/src/loader.rs:544` | `path_key()`：来源重叠检测的键（`paths_overlap` 判相等或互为前缀） | `unwrap_or_else(\|_\| path.to_path_buf())`（`:543-545`） |

重叠检测的语义在 `loader.rs:546-553`：**相等或嵌套都算重叠**，因为会话文件是递归收集的——一个 store 根若是另一个 store（或默认 pi store）的祖先，同一批文件会以不同 dedupe 身份被吃进两次；命中即返回 `cli_error`（`:527-533`）。

#### 14.3.3 并存原因与风险

- **遍历求稳定与防循环**：发现阶段的输入是用户目录（含用户自建软链），跟随会带来三类问题——重复计数（软链与目标各扫一遍）、无限递归（成环）、以及「探测与收集不一致」（`opencode/src/loader.rs:206-209` 明确点名）。因此所有 `collect_*` 一律用 `file_type()` 词法判断，且 **JSONL 遍历与 SQLite 发现必须同立场**（`openclaw/src/sqlite.rs:18-20`）。
- **重叠检测求真实同一性**：`paths_overlap` 要回答「这两个配置项是不是在指同一堆文件」，若不做 canonicalize，`~/.pi/x` 与「指向它的软链」会被判为不重叠 → 同一份会话被两条来源读两次。所以这 5 处只出现在**去重、根解析、重叠检测**上，不进入收集循环。
- **风险 1（canonicalize 侧）**：失败回退到原路径/原行为，意味着「软链解析失败」时去重可能失效（把同一文件当两条）——静默、无告警；反之解析成功时又会把软链与目标合二为一，与「遍历拒读软链」形成接缝：**同一个软链，收集侧看不到它，重叠检测却认为两者相同**。goose 与 zcode 都用「canonicalize 后再 `is_file()`」给这条接缝兜底（`goose/src/paths.rs:28-31`、`zcode/src/paths.rs:32-40`），antigravity 与 `path_key` 没有这层二次校验。
- **风险 2（遍历侧）**：词法遍历的代价是**目录项级别的软链一律不可见**——用户若把某个会话目录做成软链（常见于「数据目录搬到大盘」的用法），该目录不会被递归。至于「根路径本身是符号链接」的情形（`fs::read_dir` 跟随根），本节未在仓库测试中找到覆盖，见 §14.6。

### 14.4 选源准入判据与已否决的源

**结论**：ccusage 的准入判据是「本地能读到可核验的用量记录」，核心底线是**具备时间戳 + 会话身份 + 模型身份 + token 计数（或可映射到 token 的成本）**，且**明确拒绝从文本长度估算 token、拒绝抓私有云服务**。准入判据是叙事层文档，`BUILT_IN_AGENT_NAMES` 是硬编码的实现白名单，两者不需要数量一致。

#### 14.4.1 准入判据（`docs/guide/source-support-qa.md:1-56` 全文提炼）

| 判据 | 原文要点 | 行号 |
| --- | --- | --- |
| 最低信息集 | 时间戳、会话身份、模型身份、token 计数或可映射为 token 的已记录成本 | `:3` |
| 逐消息/逐轮计数 | 有分桶更好（输入输出、缓存、推理） | `:11-12` |
| 可定价性 | 需要模型标识；需要时间戳做筛选与分组；需要会话/对话标识 | `:13-15` |
| 格式稳定性 | JSONL / SQLite 表 / 结构化遥测导出 | `:16` |
| **显式否决 1** | 只有本地 transcript 文本不够：看不到 tokenizer 行为、隐藏上下文、缓存输入、工具调用开销、服务端记账 | `:18` |
| **显式否决 2** | **不从文本长度估算 token**——否则日报/月报/会话/成本会「看起来精确，实则猜的」 | `:5` |
| **显式否决 3** | 不抓私有云、不依赖未公开的鉴权 API 取用户用量历史 | `:25` |
| 新增源需要的材料 | 脱敏样例、默认数据目录、字段对照（哪些字段是 input/output/cache/reasoning/model/timestamp/session）、成本是记录值还是待算 | `:49-55` |

#### 14.4.2 已否决 / 暂缓的源

| 源 | 状态 | 理由 | 证据 |
| --- | --- | --- | --- |
| Devin CLI | 否决（可重审） | 用量信息在 Devin 云端，本地数据无法给出历史 token/成本；若 Devin 提供含时间戳/会话/模型/计数的本地导出，可重开 | `source-support-qa.md:22-26` |

#### 14.4.3 「曾经不支持 → 现已支持」的案例

| 源 | 早期状况 | 现状 | 证据 |
| --- | --- | --- | --- |
| Antigravity | 无 | 独立源：读本地 `gen_metadata` SQLite，**与 Gemini CLI 分离**，映射 input / cache-read / visible-output / thinking 四个桶到标准报告形状 | `source-support-qa.md:30-34`；实现见 `antigravity/src/parser.rs:689-703`、`loader.rs:19-41` |
| Grok Build CLI | 早期只看到「不暴露可用 token 记账」的本地数据（如无逐轮 usage 的 SQLite） | 现读 `${GROK_HOME:-~/.grok}/sessions/**/updates.jsonl` 的 `sessionUpdate == "turn_completed"`（`usage` / `modelUsage`）；成本取 Grok 自己的 `costUsdTicks`，LiteLLM 估算兜底；**进行中的 turn 不计** | `source-support-qa.md:37-44`；实现见 `grok/src/parser.rs:219-230`、`:468-480` |

两个案例共同说明准入线怎么移动：**不是**放宽到「估算」，而是等到本地出现逐轮 token 字段（Grok 的 `turn_completed`、Antigravity 的 `gen_metadata` protobuf）。

#### 14.4.4 准入清单与实现清单的关系

`BUILT_IN_AGENT_NAMES` 18 项（`rust/crates/ccusage-core/src/lib.rs:63-82`：claude、codex、opencode、amp、droid、codebuff、hermes、pi、goose、openclaw、kilo、copilot、gemini、antigravity、kimi、qwen、grok、zcode）与 `rust/adapters/` 下 18 个适配器目录一一对应（`adapters/` 共 19 个目录，含零件库 `common`）。三向对齐证据：

| 方向 | 事实 | 证据 |
| --- | --- | --- |
| 常量 → 配置层 | agent 名即合法命令名（`is_agent_command`）；同时 18 名 + `"all"` 构成「命名 pi store 保留名」集合 `reserved_named_pi_store_names` | `ccusage-config/src/config.rs:395-397`、`:218-222`；一致性由测试锁定 `:1244-1254` |
| 常量 → 报告层 | `AgentLoadSpec` 逐个取 `BUILT_IN_AGENT_NAMES[i]` 作为 `agent` 字段，共 18 个 spec（`index` 0..17）；index 0/1 是 Claude/Codex 专用装配 | `ccusage-adapter-all/src/loader.rs:116-358`（抽样 `:119`、`:125`、`:131`、`:308`、`:323`、`:329`、`:345`） |
| 常量 → 顺序语义 | 数组顺序 = spec 顺序 = 报告中的 agent 顺序 | `loader.rs:116-358` 与 `lib.rs:63-82` 顺序一致 |

关系论断：`source-support-qa.md` 回答「什么样的源**可以**做」，`BUILT_IN_AGENT_NAMES` 回答「现在**确实**接了哪些」。判据里没有「必须官方支持」「必须有 API」这类门槛，唯一的硬底线是本地有可核验的 token 或成本、**禁止估算**；而实现层加一个源的成本是「新增 adapter crate + 数组项 + `AgentLoadSpec`」，聚合与渲染零改动（§5/§6 已确认的架构主张）。文档明确保留扩源入口（`source-support-qa.md:47-55`），所以「准入清单 ⊃ 实现清单」是设计意图，不是遗漏。

### 14.5 测试基建（可抄的隔离模式）

**结论**：`ccusage-test-support` 用「全局环境变量锁 + RAII 还原 + 临时目录 fixture」三件套，把「测试与真实用户数据/宿主环境」彻底隔开；e2e 测试再用 `env_clear()` 从空环境起跑。这套模式解决的是**环境变量污染、临时目录管理、跨测试串扰**三类问题，与 TokenScope 的「测试密闭性」同源，但前提不同（ccusage 无状态）。

#### 14.5.1 `ccusage-test-support` 零件

| 零件 | 语义 | 行号 |
| --- | --- | --- |
| `static ENV_LOCK: Mutex<()>` | 进程内全局互斥；`env_lock()` 用 `unwrap_or_else(\|e\| e.into_inner())` 恢复中毒锁——一个测试 panic 不会连锁毒死其余需要改环境的测试 | `rust/crates/ccusage-test-support/src/lib.rs:15-19` |
| `EnvVarGuard::set(key, value)` | 先取锁 → 记录旧值 → `unsafe set_var`；`Drop` 时还原旧值，旧值为 `None` 则 `remove_var` | `:21-47` |
| `EnvVarsGuard::set_many(iter)` | 一次持锁批量设置 `(key, Option<value>)`；`Drop` 时**逆序**还原，避免同一 key 被重复设置时还原错序 | `:49-81` |
| `Fixture` | `assert_fs::TempDir` 包装（临时目录随对象释放清空）；`root()` / `path()` / `write_file()`（自动建父目录）/ `create_dir_all()` | `:83-127` |
| `fs_fixture!` 宏 | 内联目录树字面量 → 逐文件写入，fixture 与断言同屏可读 | `:135-144`，自测 `:146-171` |
| `zcode::create_fixture(path)` | 建 `model_usage` + `session` 两表并插 3 行（`session-a` 两行、`session-b` 一行；模型 `GLM-5.2`/`GLM-5.3`；时间戳 2099-01-02 / 01-15 / 02-01），`started_at` 由 `jiff::Timestamp` 解析后落毫秒整数 | `rust/crates/ccusage-test-support/src/zcode.rs:20-81`，插行 `:84-113` |

依赖面：`grep` 在 `rust/**/Cargo.toml` 命中 26 个 manifest——19 个 `rust/adapters/*` 目录（含零件库 `common`）+ 其余 5 个 crate（`ccusage`、`ccusage-adapter-all`、`ccusage-cli-parser`、`ccusage-config`、`ccusage-core`）+ `ccusage-test-support` 自身 + workspace 根 `rust/Cargo.toml`，即**每家适配器都按该模式写单元测试**（未逐个区分 `[dependencies]` / `[dev-dependencies]`，见 §14.6）。

#### 14.5.2 端到端测试形态

| 文件 | 行数（`read` 实测，含空行） | 体裁 | 套路 |
| --- | --- | --- | --- |
| `rust/crates/ccusage/tests/claude_cli.rs` | 741 | 行为断言为主 + 少量快照 | `Fixture` 写字面 JSONL（含 sidechain、无 requestId、跨日界等构造），`Command::new(env!("CARGO_BIN_EXE_ccusage"))` + `env_clear()` + 注入 `HOME`/`USERPROFILE`/`XDG_CONFIG_HOME`/`CLAUDE_CONFIG_DIR`，断言 JSON 的 `totalCost`/`totalTokens`/`entries.len()`（`:16-42`）；用 `set_times` 伪造 mtime 测 `--since` 剪枝（`:44-52`）；`claude_report_totals` 把同一 fixture 跑遍全部报告命令做交叉一致性断言（`:712-714`） |
| `rust/crates/ccusage/tests/codex_cli.rs` | 137 | 行为断言 | 统一 `run_cli_output` 注入 `CODEX_HOME` + 三个空 home 变量 + `env_remove("CLAUDE_CONFIG_DIR")` + `LOG_LEVEL=0`/`NO_COLOR`/`COLUMNS=240`（`:123-137`）；断言 session id 的多种形态选择（`:16-35`） |
| `rust/crates/ccusage/tests/copilot_cli.rs` | 122 | 快照（`insta`） | 对 daily/monthly/session 的 JSON 与表格分别 `insta::assert_snapshot!`（`:12-23`、`:26-45`） |
| `rust/crates/ccusage/tests/zcode_cli.rs` | 46 | 快照（`insta`） | 数据源换成 SQLite fixture（`create_fixture`），并固定 `--since/--until/--mode/--offline/--timezone UTC`，对三种报告的 stdout+stderr 一起快照（`:6-44`） |

#### 14.5.3 这套模式解决什么问题

- **环境变量污染**：所有适配器都从环境（`HOME`、`USERPROFILE`、`XDG_*`、各 `*_DATA_DIR`）解析数据根，`cargo test` 默认多线程同进程，直接 `set_var` 会互相踩；`ENV_LOCK` 把「改环境」串行化，RAII 保证异常路径也还原（`lib.rs:21-47`）。
- **临时目录**：`Fixture`/`fs_fixture!` 让 fixture 路径全部落在 `TempDir` 内，配合显式注入的 `HOME`，**默认根不可达真实用户数据**——这是 TokenScope 那次「测试把真实缓存 purge 成 fixture」事故的同类问题，ccusage 用「从空环境起跑」而非「注入 cache_dir 参数」来堵。
- **跨测试串扰 / 顺序依赖**：e2e 一律 `env_clear()`（`claude_cli.rs:17`、`zcode_cli.rs:11`）或显式清掉可能干扰的变量（`codex_cli.rs:134`），使开发者本机遗留变量无法造成假绿；并行读的正确性则用「单线程 vs 多线程结果一致」的对拍测试锁定（`rust/adapters/common/src/lib.rs:134-`）。

#### 14.5.4 与「测试密闭性」的关系

- 可借鉴且与 TokenScope 直接同向的：`ENV_LOCK` + RAII 还原、`fs_fixture!` 式内联 fixture、SQLite fixture 生成器（`create_fixture`）、`insta` 快照 e2e、以及「同一 fixture 跑遍全部报告命令」的交叉一致性断言。
- 不能直接照搬的：ccusage 是**无状态 CLI**（每次运行重新解析，不写用户缓存），因此不存在「测试注错缓存目录」的隐患，也就**没有** TokenScope 那种 `cache_dir` / `pricing_index` 注入参数。TokenScope 的密闭性要求（任何测试必须注入临时目录、禁止落回 `~/.tokenscope`）不能靠「env_clear + 空 HOME」单独满足——它保护的是**写入路径**，ccusage 模式只覆盖**读取路径**。
- 一个共同点值得强调：两家都避免把真实会话日志入库（ccusage 全程合成 fixture，`claude_cli.rs:6-14` 是手写字符串数组），与 TokenScope 的「真实 agent 日志永不入库」一致。

### 14.6 未验证 / 存疑

1. **`LinePrefilter` 与「可接受行」的等价性未逐一验证**。零件注释要求标记必须出现在适配器会接受的每一行（`rust/adapters/common/src/jsonl.rs:23-27`），本节只核实了各家是否传预筛与标记来源（`grok/src/parser.rs:219`、`qwen/src/parser.rs:111`、`kimi/src/parser.rs:127`、`openclaw/src/parser.rs:192`、`pi/src/parser.rs:319`、`copilot/src/parser.rs:131`、`gemini/src/parser.rs:176` 传 `None`），**未逐个证明标记集合无漏筛**（漏筛的表现是静默少算，无告警）。
2. **`antigravity` 的 protobuf 字段号与真实 schema 的对应关系无法在仓库内验证**（S5 分片「未验证」末条已声明的同一存疑），本节只补充其行级后果：字段缺失时有 `unwrap_or(0)` 兜底（`antigravity/src/parser.rs:689-703`），但结构性错误（字段号 0、非法 wire type、varint 溢出、截断）一律 `Err` → 整库失败。
3. **大小写与正斜杠形态的 `is_windows_users_path` 行为未构造样例**：`C:/Users/...` 与 `c:\users\...` 按代码均落步 4 分支（`project_names.rs:32-62`），实际展示差异未验证（纯静态判读）。
4. **步 5 的 hex 压缩分支（`project_names.rs:63-70`）的设计意图无注释依据**，本节只陈述行为（段数 ≥5 且全为 hex/`-`/`.` → 取最后两段），未找到来源说明，也未与 TS 版实现对照。
5. **§14.1 的别名解析是否曾在历史版本支持 JSON/路径形态未查证**：现状是「注释说 JSON object 或路径、实现只解析 `k=v`」（`config_schema.rs:433` vs `project_names.rs:3-17`）。
6. **符号链接：根路径本身是软链时的行为未验证**。本节确认的是「目录项级别的软链不跟随」（`common/src/lib.rs:24-32`）与「openclaw 显式拒绝」（`openclaw/src/paths.rs:56-58`）；至于 `fs::read_dir` 对「根本身是软链」是否跟随，本节按标准库语义推断为「跟随」，**仓库内无对应测试**（唯一符号链接测试是 `openclaw/src/sqlite.rs:354-380`，且 `#[cfg(unix)]`，Windows 上不运行）。
7. **`is_project_path_segment` 之外的分支**：`--project` 过滤若传入含分隔符或 `..` 的值会落回全量扫描（`claude/src/paths.rs:65-79`、`:248-254`），该路径下的 `entry.project == filter` 精确比较必然全不命中——这一「用户输错即得空报告」的行为**未实测**。
8. **26 个 manifest 命中数的构成未逐项区分**：`ccusage-test-support` 的引用落在 19 个 `rust/adapters/*` 目录 + 5 个 crate + 自身 + workspace 根，但未逐个打开确认是否都写在 `[dev-dependencies]`（`grep` 命中计数，非逐文件核对）。
9. **§14.2 中标注「grep 定位 + 邻段」的 8 家**（opencode、copilot、pi、qwen、kimi、openclaw、hermes、goose）未逐行读全部分支：其丢字段判定「哪些字段缺失会产生语义影响（而非仅取值 0）」未逐条枚举；本节只确认了它们的档位与证据行号。
10. **运行时行为**：第 8 轮已运行验证 CLI 与聚合路径（见 §13.5）；但「坏行被静默丢弃后报告数值如何变化」仍缺少用合成坏行样本跑出来的运行验证。

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
| 日志级别 | `LOG_LEVEL` 0–5（默认 3） | `docs/guide/environment-variables.md`（0 静默、1 警告、2 日志、3 info、4 debug、5 trace）；`rust/crates/ccusage-core/src/logger.rs` 只是轻量封装（15 行） | 只影响日志输出，不影响统计口径 |

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
