# ccusage 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`ccusage/ccusage`](https://github.com/ccusage/ccusage)，
> main 分支 HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（`chore: release v20.0.31`，
> 提交时间 2026-10-13 22:29 +0100，monorepo 版本 `20.0.31`）。浅克隆到临时目录后逐文件精读，
> **未运行其二进制、未读取任何真实会话内容、未改动本机 `~/.claude` 与 `~/.codex`**。
>
> 证据分档：**已验证**（源码可证且行号经交叉复核）／**一致解释**（多条代码证据指向同一结论，
> 无单点直证）／**未验证**（结构推断，未实测）。
>
> 行号口径：一律为**文件实际行号**（read 工具口径）。
>
> 本文只记录字段名、路径规则与计算口径，不含任何真实会话内容。

## 目录

1. 范围与方法
2. 总体架构（仓库形态、crate 划分、入口分发）
3. 统一数据模型（行级 → 事件级 → 汇总级）
4. 读取链：Claude Code 适配器（发现、并行、解析、去重、边界）
5. 读取链：Codex 适配器（累计值、压缩回放、速度倍率）
6. 读取链：其余 16 个适配器的数据面矩阵
7. 聚合、时间与时区（daily / monthly / weekly / session / blocks / statusline）
8. 定价数据来源与模型名匹配链
9. 金额计算公式与 cost mode
10. CLI、配置与输出契约
11. 版本史：v19 TypeScript → v20 Rust 的语义变化
12. 与 TokenScope 的对照：可抄的与要避的
13. 复核记录与未决问题

## 1. 范围与方法

- 仓库形态（**已验证**）：根为 pnpm monorepo（`package.json`，`"packageManager": "pnpm@12.11.2"`），
  workspace 成员为 `apps/*`、`docs`、`packages/*`；Rust 源码不在根 workspace 内，而在
  `rust/`（独立 Cargo workspace）。分发链：npm 包 `ccusage` → 平台预编译包
  `packages/ccusage-{darwin,linux,win32}-{x64,arm64}`。
- 规模：`rust/` 下约 5 万行 Rust（`rust/crates` + `rust/adapters`），其中
  `crates/ccusage-core/src/pricing.rs` 6181 行、`adapters/codex/src/loader.rs` 2682 行、
  `adapters/opencode/src/loader.rs` 2922 行、`adapters/claude/src/lib.rs` 1582 行。
- 方法：先读入口与类型层建立主干，再按「读取 → 归一 → 聚合 → 计价 → 渲染」逐段精读；
  符号定位用 grep，结论落 `文件:行号`；关键断言由独立复核代理做对抗性验证（见 §13）。
- 术语：
  **事件** = 一条可计量的用量记录（`LoadedEntry`）；
  **漏斗** = 「发现文件 → 逐行过滤 → 校验 → 去重」这条入口管线；
  **计价链** = 模型标识 → 价格条目的匹配与取值路径；
  **cost mode** = `--mode` 的 auto / calculate / display 三态。

## 2. 总体架构

### 2.1 crate 划分与职责

| 层 | 位置 | 职责 |
| --- | --- | --- |
| CLI 入口 | `rust/crates/ccusage/src/main.rs` | 命令分发、全局装配（`pricing::set_json_fetcher(http::fetch_json)`） |
| 命令实现 | `rust/crates/ccusage/src/commands/mod.rs` | daily/monthly/weekly/session/blocks/statusline 的具体执行 |
| 参数类型 | `rust/crates/ccusage-cli/src/{types,lib}.rs` | `SharedArgs`、`CostMode`、`SortOrder`、`WeekDay` 等 |
| 参数解析 | `rust/crates/ccusage-cli-parser/src/*` | 自研解析器 + 分层帮助生成 |
| 配置 | `rust/crates/ccusage-config/src/*` | `ccusage.json` 读取、schema 校验、默认值 |
| 核心 | `rust/crates/ccusage-core/src/*` | 类型、日期/时区、聚合、计价、输出渲染 |
| 终端渲染 | `rust/crates/ccusage-terminal/src/*` | 表格、颜色、宽度、标题 |
| 统一报告 | `rust/crates/ccusage-adapter-all/src/*` | `ccusage`（无子命令）/ `all` 报告，合并 18 家适配器 |
| 适配器 | `rust/adapters/<agent>/src/*` | 每家一个 crate：发现 + 解析 → `LoadedEntry` |
| 适配器公共件 | `rust/adapters/common/src/*` | 文件遍历、并行读取、JSONL 工具、通用报告 |

（**已验证**：`rust/Cargo.toml` 的 `members = ["adapters/*", "crates/*"]`；各 crate 源码见上表行数统计。）

### 2.2 入口分发

`main()`（`rust/crates/ccusage/src/main.rs:25-67`）先注册价格抓取回调，再按解析出的子命令分派：

| 子命令 | 入口 |
| --- | --- |
| 无子命令（默认） | `adapter::all::run`（`AgentReportKind::Daily`） |
| `all` | `adapter::all::run` |
| `daily` | `commands::run_daily` |
| `monthly` | `commands::run_bucket(shared, BucketKind::Monthly)` |
| `weekly` | `commands::run_weekly` |
| `session` | `commands::run_session` |
| `blocks` | `commands::run_blocks` |
| `statusline` | `commands::run_statusline` |
| `claude` | 默认 daily 走同一 `commands::run_daily` 路径（见 §4） |
| `codex`/`opencode`/`amp`/`droid`/… （17 个） | `adapter::<agent>::run(args)` |

每个 adapter crate 都导出同名 `run`，`main.rs:90-113` 的测试 `agent_commands_are_exposed_by_independent_crates`
以 17 元函数数组 + `runs.len() == 17` 断言把这层契约钉死——**新增 agent 只加 crate 与一行分发**。

### 2.3 分层纪律

- 适配器只依赖 `ccusage-core`（类型与计价）与 `ccusage-adapter-common`（遍历与 JSONL 工具），
  彼此不互相依赖；`ccusage-adapter-all` 聚合全部适配器。
- 事件模型是唯一的跨层契约：适配器产出 `LoadedEntry`（§3.3），聚合与渲染只消费它。
  计价发生在**适配器内**（解析一条即计价一次，`claude/lib.rs:525`），而不是在渲染层。

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
    pub cache_creation: Option<CacheCreationRaw>,// ephemeral_5m/1h_input_tokens
}
```

两个关键细节：

- `cache_creation_token_count()`（`types.rs:42-49`）：`cache_creation` 明细存在时用
  `ephemeral_5m + ephemeral_1h`（**saturating_add**），否则退回扁平的 `cache_creation_input_tokens`。
- 全仓 token 累加一律 `saturating_add`，`total()` 也是（`types.rs:78-95`）；溢出饱和而非回绕，
  并有单测 `saturates_token_count_accumulation_and_total`（`types.rs:196-218`）钉住。

### 3.2 token 四桶

`TokenCounts`（`types.rs:67-96`）与 `ModelBreakdown`（`types.rs:98-122`）都持有四桶
（input / output / cache_creation / cache_read）外加一个 **不序列化** 的 `extra_total_tokens`
（`#[serde(skip_serializing)]`，供总数校验与 advisor 之类附加来源使用）。
`u64` 语义：**input 是不含 cache 的新增输入**——适配器在归一阶段把厂商的「含 cache 输入」拆开
（例如 Codex 的 `input_tokens - cached_input_tokens`，见 §5）。

### 3.3 事件级：`LoadedEntry` / `LoadedFile`

**已验证**（`types.rs:124-145`）：

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
    pub credits: Option<f64>,        // Amp 专用
    pub message_count: Option<u64>,
    pub model: Option<String>,       // 归一后的模型名（可能带 -fast 后缀）
    pub usage_limit_reset_time: Option<TimestampMs>,
    pub missing_pricing_model: Option<String>, // 未命中价格时留痕
}
pub struct LoadedFile { pub timestamp: Option<TimestampMs>, pub entries: Vec<LoadedEntry> }
```

`LoadedFile.timestamp` 是文件内**最小**时间戳（`claude/lib.rs:660-666`），供 `--since` 窗口的文件级剪枝。

### 3.4 汇总级：`UsageSummary`

`UsageSummary`（`types.rs:147-190`）是聚合后的行：`date`/`month`/`week`/`session_id`/`project_path`
五个维度字段**全部是 `Option` 且按需序列化**（`skip_serializing_if = "Option::is_none"`），
所以同一个结构体能同时承载日、月、周、会话四种报告的 JSON。四桶 + `total_cost` + `credits` +
`models_used`（去重后的模型名列表）+ `model_breakdowns`（每个模型一行的 token 与费用）。
`extra_total_tokens` 同样 `skip_serializing`：它参与 `total_tokens()` 但不进入 JSON。
