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
即「§4.6」= 第 4 章的 4.6 小节（去重规则下的第 6 点），不是文档的第 4.6 章；
跨章引用一律写出章号（如「见 §8 定价章」「§13 复核记录」）。

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
