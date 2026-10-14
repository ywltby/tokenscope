# tokscale 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`junhoyeo/tokscale`](https://github.com/junhoyeo/tokscale)，
> main 分支 HEAD `bd62b8c4`（`fix(codex): support reading and caching zstd-compressed transcripts (#1423)`，
> 提交时间 2026-10-11 23:34 +0900，workspace 版本 `4.18.0`）。浅克隆到临时目录后逐文件精读，
> **未运行其二进制、未读取任何真实会话数据、未改动本机 `~/.config/tokscale`**。
>
> 证据分档：**已验证**（源码可证，且行号经交叉复核）／**一致解释**（多条代码证据指向同一结论，
> 无单点直证）／**未验证**（结构推断，未实测；全文统一简写为「未验证」）。
>
> 行号口径：一律为**文件实际行号**（编辑/读取工具口径）。PowerShell `Measure-Object -Line`
> 会忽略空行、给出偏小的计数，本文不采用该口径；两者差异在个别文件上可达 8%（如
> `message_cache.rs` 实际 9222 行 vs 非空 8534 行）。
>
> 本文只记录字段名、表名、路径规则与计算口径，不含任何真实会话内容。

## 目录

1. 范围与方法
2. 总体架构（crate 划分、解析主干与两份实现、统一事件模型）
3. 客户端注册表与覆盖矩阵（58 个数据面）
4. 文件型适配器：逐家读取实现（Claude Code / Codex / Gemini 与共性模式）
5. SQLite 型适配器
6. 缓存 / 同步型数据源（上游无可解析本地存储）
7. 增量缓存与性能
8. 聚合口径与时间（分组、时区、会话化、身份归一）
9. 定价与金额计算（来源、匹配链、计费公式、倍率）
10. 输出与上报
11. 与 TokenScope 的对照：值得抄的、需要避的
12. 未验证项与已知风险（含四轮复核记录）
- 附录 A：关键常量与阈值速查
- 附录 B：模块行号索引（节选）

## 1. 范围与方法

- 仓库体量：Rust workspace 两个 crate（`crates/tokscale-core`、`crates/tokscale-cli`）＋
  Next.js 前端（`packages/frontend`，含 leaderboard 与 submit API）＋ npm 包装包
  （`packages/tokscale`、`packages/cli-<平台>`）。
- 核心规模（实际行号）：`crates/tokscale-core/src/lib.rs` 20319 行、`scanner.rs` 9325 行、
  `message_cache.rs` 9222 行、`pricing/lookup.rs` 9665 行、`pricing/mod.rs` 2965 行、
  `sessions/` 下 62 个文件（`sessions/mod.rs` 2209 行起，最大的 `kiro.rs` 5793 行）。
- 解析手段：符号定位（grep）→ 分段精读原文 → 交叉核对调用点与常量 → 抽取
  `文件:行号` 证据；关键断言由第二组独立复核代理做对抗性验证（见 §12.1 复核说明）。
- 术语：
  **车道（lane）** = `lib.rs` 中按客户端串行执行的一段解析代码；
  **统一事件** = `UnifiedMessage`；
  **源消息缓存** = `<config_dir>/cache/source-message-cache-v2/` 下的分片缓存；
  **定价链** = 模型标识 → 价格条目的匹配与计价路径。

## 2. 总体架构

### 2.1 crate 与职责

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 发现 | `crates/tokscale-core/src/scanner.rs` | walkdir 递归扫描各客户端数据根，产出按客户端分组的文件清单 |
| 解析 | `crates/tokscale-core/src/sessions/*.rs`（62 文件） | 每个客户端一个适配器，把本地日志/SQLite 解析为 `UnifiedMessage` |
| 缓存 | `message_cache.rs` + `fs_atomic.rs` | 解析结果分片落盘，热扫描避免重解析 |
| 聚合 | `aggregator.rs` + `lib.rs` 中的 `aggregate_*` | 按 6 种分组维度折叠、日/月/时汇总、图表日历 |
| 计价 | `pricing/`（10 文件） | 三张动态价格表 + 三张内置表，匹配与分档计价 |
| 输出 | `crates/tokscale-cli`（TUI、report、submit）+ `packages/frontend` | 表格/TUI/JSON、上报 leaderboard |

客户端注册表 `clients.rs` 只描述**扫描面**（根、相对路径、文件名 pattern、是否本地解析、
是否默认提交），不绑定解析函数——解析映射是 `lib.rs` 里手写的顺序车道（§3.1）。

### 2.2 解析主干与两份实现

**已验证**：统计类输出（`models/monthly/hourly/graph/tui/submit`）的唯一解析主干是
`parse_all_messages_streaming`（`crates/tokscale-core/src/lib.rs:1011`）：

| 链路 | 入口 | 关键跳转 |
| --- | --- | --- |
| 报告（`tokscale models/monthly/hourly/graph`） | `get_model_report`（`lib.rs:4247`） | → `parse_all_messages_with_pricing_with_env_strategy`（`lib.rs:777`，默认 `SourceCachePolicy::Persistent`，`lib.rs:790`）→ `parse_all_messages_with_pricing_with_cache_policy`（`lib.rs:974`）→ streaming 主干 |
| TUI 本地数据 | `UsageDataLoader::load`（`crates/tokscale-cli/src/tui/data/mod.rs:629`） | → `parse_local_unified_messages`（`lib.rs:6751`）→ `resolve_local_parse_request`（`lib.rs:5404`）→ `parse_local_unified_messages_resolved`（`lib.rs:5419`）→ 同一主干 |

**修正（对抗复核发现）**：`parse_local_clients`（`lib.rs:5436`）**不是**同一条主干的壳，
而是**第二套独立解析实现**——它内含自己的手写车道段（`lib.rs:5472-6690`，覆盖
opencode/openclaw/augment/junie/kimi/qwen 等自有车道），产出 `ParsedMessage`（`lib.rs:348`）/
`ParsedMessages`（`lib.rs:378`），**不经过** `parse_all_messages_streaming`。
`tokscale report`（`commands/report.rs:43`→`:113`，用于填充 wiki）、`tokscale clients`、
`tokscale wrapped` 走这条实现。因此：

- 「统计链路」只有一份主干（streaming），但**全仓存在两份解析代码**（车道段也有两处：
  `lib.rs:2117-3662` 与 `lib.rs:5472-6690`）；
- 两套语义需要手工对齐，tokscale 自己的测试以成对断言兜底（如
  `crates/tokscale-core/tests/micode_accounting.rs:232-236` 的 `local` vs `unified` 对照）。
  这比「唯一主干」的叙事维护成本更高，是 §11.3 与 §12.4 里「双解析实现」风险的来源。

主干内部顺序（**已验证**）：扫描（`lib.rs:2081` → `scanner.rs:1875` →
`scanner.rs:1929`）→ 打开缓存（`lib.rs:2092-2095`）→ 逐客户端车道
（`lib.rs:2117` 起 OpenCode、`2181` MiMo、`2210` Claude、`2292` Codex、`2430` Gemini、
`2460` Cursor、`2495` Grok、`2625` OpenClaw、`3188` Kimi、`3325` Fx、`3388` Devin、
`3420` Zed、`3507` CodeBuddy、`3595` WorkBuddy、`3639` synthetic；**节选**，
另有 kilo/goose/kiro/crush/trae 等块）→ 出口过滤
（`flush_lane` `lib.rs:930` → `flush_message` `lib.rs:944`，客户端过滤在 `949-958`、
时区重分桶在 `967-969`）→ 缓存落盘（`lib.rs:3655-3657`）→ 历史修复
（`recovery::apply`，`lib.rs:993`；实现 `recovery.rs:331`）→ 报告侧过滤
（`filter_messages_for_report` `lib.rs:5189`）→ 聚合。

日期过滤有两个入口，均**不缩短扫描**（对抗复核补正）：本地解析链在
`filter_unified_messages`（`lib.rs:3737-3757`，调用点 `:5434`）；
报告链在 `message_passes_report_filter`（`lib.rs:5159-5187`，调用点 `:4270`、`:4381`、
`:4510`、`:5004`）。两者都只比较 date 字符串。

### 2.3 统一事件模型

**已验证**（`crates/tokscale-core/src/sessions/mod.rs:81-125`；下列为字段定义，
源码中每字段独立成行并带文档注释，此处压缩排版）：

```rust
pub struct UnifiedMessage {
    pub client: String, pub model_id: String, pub provider_id: String, pub session_id: String,
    pub workspace_key: Option<String>, pub workspace_label: Option<String>,
    pub timestamp: i64, pub date: String,
    pub tokens: TokenBreakdown, pub cost: f64, pub cost_source: CostSource,
    pub service_tier: Option<String>, pub duration_ms: Option<i64>, pub message_count: i32,
    pub agent: Option<String>, pub dedup_key: Option<String>,
    pub session_title: Option<String>, pub parent_session_id: Option<String>,
    pub is_turn_start: bool, pub model_attribution_conflicted: bool,
}
```

token 桶（`lib.rs:252-288`）：`input / output / cache_read / cache_write / cache_write_1h /
reasoning`。要点：

- `cache_write_1h` 是 `cache_write` 的**子集**（Anthropic 1 小时 TTL 档，计费 2× 输入价；
  默认 5 分钟档为 1.25× 输入价），`total()` 不重复累加它（`lib.rs:257-262`、`279-288`）。
- 所有累加走 `add_assign_saturating`（`lib.rs:270-277`），溢出保护而非回绕。
- 成本三态 `CostSource`（`sessions/mod.rs:73-78`）：`Unknown`（默认，未定价）／
  `ProviderReported`（来源自报，权威，`mark_provider_reported_cost` `sessions/mod.rs:455`）／
  `Estimated`（本地价格表推算，`mark_estimated_cost` `sessions/mod.rs:459`）。
  权威成本由 `has_authoritative_cost`（`sessions/mod.rs:463`）守卫，估算不得覆盖它。
- `model_attribution_conflicted`（`sessions/mod.rs:121-124`）标记「观察到互相矛盾的权威模型
  证据」的行，这类行保持未定价，不接受兜底归属。

## 3. 客户端注册表与覆盖矩阵

### 3.1 `define_clients!`：注册表只描述扫描面

**已验证**（`clients.rs:285-401`）：`ClientDef` 字段为
`id / root / relative_path / pattern / headless / parse_local / submit_default`；
宏 `define_clients!`（`clients.rs:320-401`）展开出 `ClientId` 枚举（`:324`）、
`CLIENTS` 常量表（`:374`）与显示名/logo 表（`:389-390`），并用
`assert!($index == i)`（`:392-399`）强制下标连续。

- 根类型 `PathRoot`（`clients.rs:2-18`）：`Home / XdgData / Config / AppData / EnvVar{var,
  fallback_relative} / ReasonixHome`，解析含各平台差异（`clients.rs:98-213`）。
- 注册表规模：**56 个 `ClientId`**（index 0–55），另有注册表外的人工别名
  `synthetic` 与 `9router`（`scanner.rs:1946-1955`、`crates/tokscale-cli/src/main.rs:4277-4280`）——
  两者都不增加 `ClientId`、不产生独立数据面：`9router` 是 CLI 侧筛选别名（指向 Gjc 的
  扫描面），`synthetic` 是解析后的重归属标签（§8.5）。
- 关键点：`ClientDef` **不含解析函数指针**。客户端 → 解析函数是
  `parse_all_messages_streaming`（`lib.rs:1011-3663`）内手写车道里显式书写的
  `sessions::<client>::parse_*` 调用（车道直接调用口径实测 58 处、覆盖 51 个模块；
  若计入 7 处辅助闭包内的解析调用则为 65 处/54 个模块）。
- 默认本地集合 = 所有 `parse_local() == true` 的客户端 + `synthetic`
  （`lib.rs:5441-5448`）。`Cursor` 的 `parse_local: false`（`clients.rs:447`）意味着
  **默认扫描不含 Cursor**，必须显式请求。

### 3.2 覆盖矩阵（58 个数据面）

**已验证**：下表由注册表逐条抽出并与各适配器入口函数核对。解析入口列相对
`crates/tokscale-core/src/`，**保留完整目录前缀**（`sessions/…` 与根目录文件并存）。
平台路径按 Linux/macOS 口径书写：`PathRoot::Config` 的条目（Antigravity/Trae/Warp 等）
在 Windows 下为 `%APPDATA%\tokscale\…`，`PathRoot::Home`（Cursor）为
`%USERPROFILE%\.config\tokscale\…`。

| client id | 展示名 | 数据来源（路径 / 环境变量） | 数据面 | 解析入口 |
| --- | --- | --- | --- | --- |
| opencode | OpenCode | `$XDG_DATA_HOME/opencode/opencode*.db`；旧版 `opencode/storage/message/*/*.json` | 混合（SQLite 优先） | sessions/opencode.rs:27 / :51 |
| claude | Claude Code | `$CLAUDE_CONFIG_DIR/projects/**/*.jsonl`（默认 `~/.claude`）＋同目录 `transcripts/`＋`~/.cc-mirror/<variant>/config/projects` | JSONL | sessions/claudecode.rs:407 |
| codex | Codex CLI | `$CODEX_HOME/sessions/**/*.jsonl*`（含 `.jsonl.zst`）＋`archived_sessions/`＋headless 根 | JSONL（可压缩） | sessions/codex.rs:1415 / :1454 |
| gemini | Gemini CLI | `$GEMINI_CLI_HOME/tmp/*/chats/*.json`（+ `session-*.json`/`.jsonl`） | 混合 | sessions/gemini.rs:100 |
| cursor | Cursor IDE | `<home>/.config/tokscale/cursor-cache/usage*.json` / `usage*.csv`（需 `tokscale cursor sync`） | 自建缓存 | sessions/cursor.rs:308 |
| amp | Amp | `$XDG_DATA_HOME/amp/threads/T-*.json` | JSON 快照 | sessions/amp.rs:275 |
| droid | Droid | `~/.factory/sessions/**/*.settings.json` | JSON 快照 | sessions/droid.rs:419 |
| openclaw | OpenClaw | `~/.openclaw/agents/<id>/agent/openclaw-agent.sqlite` + `<id>/sessions/*.jsonl*`（legacy `.clawdbot`/`.moltbot`/`.moldbot`） | 混合 | sessions/openclaw.rs:603 / :437 |
| pi | Pi | `~/.pi/agent/sessions/**/*.jsonl` | JSONL（Pi 格式族） | sessions/pi.rs:398 |
| kimi | Kimi CLI | `~/.kimi/sessions/**/wire.jsonl`；`$KIMI_CODE_HOME/sessions/**/wire.jsonl`；Kimi Work desktop 根 | JSONL | sessions/kimi.rs:447 / :339 |
| qwen | Qwen CLI | `~/.qwen/projects/**/*.jsonl` | JSONL | sessions/qwen.rs:75 |
| roocode | Roo Code | VS Code globalStorage `rooveterinaryinc.roo-cline/tasks/**/ui_messages.json`（含 vscode-server） | JSON 快照 | sessions/roocode.rs:51 / :55 |
| kilocode | Kilo Code | VS Code globalStorage `kilocode.kilo-code/tasks/**/ui_messages.json` | JSON 快照 | sessions/kilocode.rs:9 |
| mux | Mux | `~/.mux/sessions/*/session-usage.json` | JSON 快照（会话聚合） | sessions/mux.rs:47 |
| kilo | Kilo CLI | `$XDG_DATA_HOME/kilo/kilo.db` | SQLite | sessions/kilo.rs:16 |
| crush | Crush | `$XDG_DATA_HOME/crush/projects.json` → 各项目 `crush.db` | SQLite（仅成本） | sessions/crush.rs:47 |
| hermes | Hermes Agent | `$HERMES_HOME/state.db` + `$HERMES_HOME/profiles/*/state.db` | SQLite | sessions/hermes.rs:194 |
| copilot | Copilot CLI | `~/.copilot/session-store.db`、`~/.copilot/data.db`、`~/.copilot/otel/*.jsonl`、VS Code `chatSessions/*.jsonl` | 混合（4 源） | sessions/copilot.rs:15 / sessions/copilot_session_store.rs:15 / sessions/copilot_desktop.rs:241 / sessions/copilot_vscode.rs:9 |
| goose | Goose | `$GOOSE_PATH_ROOT/data/sessions/sessions.db`；`~/.local/share/goose/...`；macOS Library | SQLite | sessions/goose.rs:48 |
| codebuff | Codebuff | `$CODEBUFF_DATA_DIR/projects/**/chat-messages.json`（默认三通道 manicode[-dev/-staging]） | JSON 快照 | sessions/codebuff.rs:40 |
| freebuff | Freebuff | 与 Codebuff 同树；`$FREEBUFF_DATA_DIR` → `$CODEBUFF_DATA_DIR` | JSON 快照（估算） | sessions/freebuff.rs:118 |
| antigravity | Antigravity | `~/.config/tokscale/antigravity-cache/sessions/*.jsonl`（需 `antigravity sync`） | 自建缓存 | sessions/antigravity.rs:6 |
| zed | Zed Agent | `~/.local/share/zed/threads/threads.db`（macOS/Windows 各自路径） | SQLite（仅 zed.dev） | sessions/zed.rs:36 |
| kiro | Kiro | `~/.kiro/sessions/cli/*.json`（+同 stem `.jsonl`）+ `~/.local/share/kiro-cli/data.sqlite3` + IDE globalStorage | 混合 | sessions/kiro.rs:186 / :1507 |
| trae | Trae | `~/.config/tokscale/trae-cache/sessions/*.json`（需 `trae sync`） | 自建缓存 | sessions/trae.rs:113 |
| warp | Warp | `~/.config/tokscale/warp-cache/usage*.json`（需 `warp sync`；仅聚合） | 自建缓存 | sessions/warp.rs:32 |
| cline | Cline | VS Code globalStorage tasks `ui_messages.json` + CLI `$CLINE_SESSION_DATA_DIR` 等四级回退 | 混合 | sessions/cline.rs:15 / :136 |
| gjc | Gajae-Code | `$GJC_CODING_AGENT_DIR/sessions/**/*.jsonl`（默认 `~/.gjc/agent`） | JSONL | sessions/gjc.rs:96 |
| grok | Grok Build | `$GROK_HOME/sessions/*/*/updates.jsonl` + `$GROK_HOME/logs/unified.jsonl`（双源） | JSONL | sessions/grok.rs:828 / :341 / :528 |
| jcode | Jcode | `$JCODE_HOME/sessions/session_*.json` + 同 stem `.journal.jsonl` | 混合 | sessions/jcode.rs:294 |
| commandcode | Command Code | `~/.commandcode/projects/**/*.jsonl`（v3 带自带 usage/cost） | JSONL | sessions/commandcode.rs:191 |
| micode | MiMo Code | `$XDG_DATA_HOME/mimocode/mimocode*.db` + orca hooks 沙箱目录 | SQLite | sessions/micode.rs:519 / :475 |
| micode-desktop | Xiaomi MiMo AI | 同一 store，按 `session.version` 前缀 `desktop-` 分流 | SQLite | sessions/micode.rs:475 / :519 |
| antigravity-cli | Antigravity CLI | `$GEMINI_CLI_HOME/antigravity-cli/conversations/*.db` | SQLite | sessions/antigravity_cli.rs:74 |
| junie | Junie | `~/.junie/sessions/*/events.jsonl` | JSONL | sessions/junie.rs:21 |
| zcode | ZCode | `~/.zcode/projects/**/*.jsonl`（legacy）+ `~/.zcode/cli/db/db.sqlite`（v2） | 混合 | sessions/zcode.rs:107 / :281 |
| opencodereview | OpenCodeReview | `~/.opencodereview/sessions/**/*.jsonl` | JSONL | sessions/opencodereview.rs:15 |
| codebuddy | CodeBuddy | `~/.codebuddy/projects/**/*.jsonl` + 扩展文本日志（`%LOCALAPPDATA%`/`%APPDATA%`） | 混合 | sessions/codebuddy.rs:12 → sessions/tencent_buddy.rs:197 / :309 |
| workbuddy | WorkBuddy | `~/.workbuddy/projects/**/*.jsonl`、`~/.workbuddy-ai/...`（5.5+）+ 各自 `workbuddy.db` | 混合 | sessions/workbuddy.rs:14 / :43 |
| devin-cli | Devin CLI | `$XDG_DATA_HOME/devin/cli/sessions.db` | SQLite | sessions/devin.rs:153 |
| devin-desktop | Devin Desktop | `<config>/Devin/User/acp-events/*.ndjson`（各平台路径不同） | NDJSON | sessions/devin.rs:424 / :110 |
| senpi | Senpi (OmO Native) | `$SENPI_CODING_AGENT_DIR/sessions/**/*.jsonl`、`~/.omo/agent/sessions` 等 | JSONL | sessions/senpi.rs:24 |
| augment | Augment Code | `~/.augment/sessions/*.json` | JSON 快照 | sessions/augment.rs:154 |
| kimchi | Kimchi | `$KIMCHI_CODING_AGENT_DIR/sessions/**/*.jsonl` | JSONL（Pi 格式） | sessions/kimchi.rs:11 |
| reasonix | Reasonix | `$REASONIX_STATE_HOME` / `$REASONIX_HOME/stats/*.jsonl` | JSONL | sessions/reasonix.rs:81 |
| prime-agent | Prime Agent | `$PRIME_AGENT_CODING_AGENT_DIR/sessions/**/*.jsonl` + `session-artifacts/`（RLM 子会话） | JSONL | sessions/prime_agent.rs:112 |
| cherrystudio | Cherry Studio | `%APPDATA%\CherryStudio\Data\Agents\.claude\projects\*.jsonl`（V2）+ V1 根 + `cherrystudio.sqlite` | 混合 | sessions/cherrystudio.rs:259 / :638 |
| dsh | DeepSeek Harness | `$DSH_HOME/sessions/**/session.v<N>.jsonl[.zstd]` | JSONL（zstd 嗅探） | sessions/dsh.rs:196 |
| mcode | MiniMax Code | `<config>/tokscale/headless/mcode/*.jsonl`（`tokscale headless mcode` 捕获） | headless 捕获 | sessions/mcode.rs:29 |
| fx | Fx | `~/.fx/sessions/<id>/usage-v2.json` + `session.json` + `index.json` | JSON 快照（会话聚合） | sessions/fx.rs:263 |
| omp | Oh My Pi | `~/.omp/agent/sessions/**/*.jsonl`（固定根，不继承 Pi 环境变量） | JSONL | sessions/omp.rs:27 |
| lmstudio | LM Studio | `$LM_STUDIO_HOME/server-logs/**/*.log` | 文本日志（仅 usage 块） | sessions/lmstudio.rs:335 |
| unsloth | Unsloth Studio | `$UNSLOTH_STUDIO_HOME/studio.db` | SQLite | sessions/unsloth.rs:264 |
| hindsight | Hindsight | `$HINDSIGHT_HOME/usage/*.jsonl`（需 `hindsight sync`） | 自建缓存 | sessions/hindsight.rs:77 |
| muse | Muse Code | `$XDG_DATA_HOME/muse/sessions/**/session.jsonl`（含 `subagent/`） | JSONL | sessions/muse.rs:44 |
| antigravity-extension | Antigravity IDE 扩展 | `~/.gemini/antigravity/conversations/*.db` | SQLite | sessions/antigravity_cli.rs:79 |
| sakana（无独立 ClientId） | Sakana Fugu | 经 Codex 转录 `model_provider: sakana` 重归属 | 重归属 | provider_identity.rs:268（fugu 分支 :340-342） |
| synthetic（无独立 ClientId） | Synthetic | `hf:` 模型前缀 / `synthetic`·`glhf`·`octofriend` provider 重归属；+ Octofriend `sqlite.db` | 重归属 | sessions/synthetic.rs:99 / :112 |

### 3.3 读取方式分类统计

| 类别 | 数量 | 共同点 |
| --- | --- | --- |
| JSONL 转录 | 17 | 递归发现固定文件名（`*.jsonl`、`wire.jsonl`、`updates.jsonl`、`events.jsonl`、`session.jsonl[.zstd]`），逐行解析带 usage 的事件；Pi 格式族覆盖 pi/kimchi/senpi/omp/prime-agent（模块头注释 `sessions/pi.rs:18-26`、共享入口 `:414`、共享基版本常量 `:49`；各族的 `use` 点见 kimchi.rs:7、omp.rs:22、senpi.rs:19） |
| JSON 快照 | 9 | 一文件 = 一次会话/任务完整对象，整体反序列化后按 turn 提取；roocode/kilocode/cline 共用 `parse_roo_kilo_file`，codebuff/freebuff 共用 chat-messages 布局 |
| SQLite | 11 | 统一只读连接（`sessions/utils.rs:301`）执行固定 SQL；kilo/micode 共用 `opencode_schema` 驱动（`opencode_schema.rs:1239`）；crush 先经 `projects.json` 注册表定位库文件 |
| 混合（≥2 数据面） | 11 | 迁移期新旧存储叠放，靠稳定 dedup key 去重防双计；含 jcode（`session_*.json` + `.journal.jsonl` 边车） |
| tokscale 自建缓存（需 sync/headless） | 6 | 数据先由 CLI 命令抓到 `~/.config/tokscale/`（Windows `%APPDATA%\tokscale`），再当普通文件读；上游无本地用量存储。6 = cursor/antigravity/trae/warp/hindsight + 1 个 headless 捕获（mcode） |
| 文本日志 | 1 | lmstudio 只抓 `usage` 块与响应 id，不留正文 |
| NDJSON 事件流 | 1 | devin-desktop 的 ACP 事件流 |
| 重归属 | 2 | sakana / synthetic：无独立扫描根，解析后按模型/provider 特征改归属 |

### 3.4 README 与代码的差异（抽 6 个客户端核对）

1. **Claude 第三来源未记录**：README 只写 `projects/` 与 `transcripts/`，代码另有 cc-mirror
   变体根（`scanner.rs:794-798` → `cc_mirror.rs:22`，`variant.json` 的 `configDir` 可重定向，
   默认回退 `<variant>/config`，`cc_mirror.rs:116`）。
2. **Kiro 的 `*.jsonl` 描述有误导**：CLI 路径扫描 pattern 是 `*.json`（`scanner.rs:2726-2732`），
   `.jsonl` 由解析器按同 stem 关联读取（`kiro.rs:570-577`），不被独立扫描。
3. **Cursor 缓存格式过时**：README 只提 `usage*.csv`；代码 pattern 是
   `usage*.json|usage*.csv`（`clients.rs:445`），JSON 为当前格式、CSV 为 legacy，并存时
   JSON 胜出（`scanner.rs:3022`、`:3029`）。
4. **`--client` 取值清单漏 10 个 id**：README 的取值清单列 46 个 ClientId（清单另含
   非 ClientId 的 `synthetic`，故合计 47 项），注册表实有 56 个，缺 senpi、
   freebuff、dsh、mcode、fx、omp、workbuddy、devin-cli、devin-desktop、reasonix——它们都可被
   `ClientId::from_str`（`clients.rs:369`）解析。
5. **MiMo 的 orca 第二来源未记录**：代码无条件追加
   `~/Library/Application Support/orca/mimocode-hooks/shared/data` 并与 XDG 目录求并集
   （`scanner.rs:2251-2276`）。
6. **Cursor 平台根与 Warp/Trae 不同**：Cursor 用 `PathRoot::Home`（`clients.rs:443`）→
   Windows `%USERPROFILE%\.config\tokscale\cursor-cache`；Antigravity/Trae/Warp 用
   `PathRoot::Config`（`clients.rs:622/652/662`，按此顺序）→ `%APPDATA%\tokscale\...`
   （`clients.rs:154-157`）。

**一致解释**：README 表格 58 行 ≈ 56 个 `ClientId` + 2 个重归属来源，数量自洽。

## 4. 文件型适配器：逐家读取实现

### 4.1 Claude Code（`sessions/claudecode.rs`）

扫描面（`clients.rs:414-426` + `scanner.rs:779-799`）：`projects/**/*.jsonl` 为主，
额外扫 `projects` 同级的 `transcripts/` 与 cc-mirror 变体根；后缀规则 `*.jsonl*`
会把备份/隔离副本一并收进来（`scanner.rs:513-535`），靠去重折算。

**usage → 桶映射是直译**（`claudecode.rs:696-709`；代码块中 `// 暂不 clamp` 为本文注，
源码对应说明在 `:702-706`）：

```rust
let cache_write = usage.cache_creation_input_tokens.unwrap_or(0).max(0);
TokenBreakdown {
    input: usage.input_tokens.unwrap_or(0).max(0),
    output: usage.output_tokens.unwrap_or(0).max(0),
    cache_read: usage.cache_read_input_tokens.unwrap_or(0).max(0),
    cache_write,
    cache_write_1h: usage.cache_write_1h_raw(),  // 暂不 clamp
    reasoning: 0,
}
```

1 小时档来自 `cache_creation.ephemeral_1h_input_tokens`（`sessions/utils.rs:738-744`）；
`cache_write_1h` 故意先不 clamp，等整文件组装完在 `claudecode.rs:816-821` 统一收口成
`min(cache_write)`。

**去重键**（`claudecode.rs:632-636`）：`{message.id}:{requestId}`；无 requestId 时退化为
`message:{id}`。同键的流式快照按 `merge_claude_duplicate`（`claudecode.rs:988-1045`）逐字段取
max 合并，但 `input` 例外——当一方带缓存键、另一方不带时，**带缓存键的一方整体胜出**
（`claudecode.rs:1006-1020`），因为 Anthropic 的 `input_tokens` 是不含缓存的余量，两种语义不可比。
跨文件重放走 `merge_message_completeness`（`claudecode.rs:1054-1116`），并在 `1112-1115`
让权威成本覆盖估算成本。

**subagent 与 workflows**：首行 `isSidechain` 触发 sidechain 处理，并用父 `sessionId`
覆盖文件 stem 以免会话数虚高（`claudecode.rs:526-542`）；名字按「`.meta.json` 的 `agentType`
→ 父会话里对应 `tool_use` → 兜底」三层解析（`88-128`）；父文件定位锚定 `subagents` 祖先目录，
因此 `subagents/workflows/<wf>/agent-X.jsonl` 也能找回父会话（`156-183`）；
`subagents/**/journal.jsonl` 被显式拒收（`140-146`）。

**advisor 用量（提交 `c8fc84b` "fix(claude): count advisor model usage"）**：`usage.iterations[]` 中 `type == "advisor_message"` 的条目
单独出账，模型取 iteration 自己的 `model`，去重键是父键加 `:advisor:{index}`，`message_count`
置 0（`claudecode.rs:829-871`）；主循环在父消息装配完成后才处理 advisor（`735-772`）。
即：服务端 advisor 模型的额外用量从「被忽略」改为「独立入账」。

**其余细节**：时间戳 RFC3339 → 毫秒（`claudecode.rs:977-981`），缺失回退文件 mtime（`459`）；
轮次耗时 = 上一条 user 行时间 → 本 assistant 行时间，只取正值（`563-566`、`983-986`）；
project/cwd 由 `projects/<key>` 的 dash 编码 slug 经 `decode_claude_project_slug`
（`sessions/mod.rs:868`）做**回溯式文件系统探测**还原，带探测预算（`883-895`、`960-1045`）。

### 4.2 Codex CLI（`sessions/codex.rs`）

扫描面（`clients.rs:427-439` + `scanner.rs:2317-2352`）：`sessions/**/*.jsonl*`、
`archived_sessions/`、headless 根；pattern `*.jsonl*` 与后缀规则共同覆盖 `.jsonl.zst`。

**取数矩阵**（`codex.rs:933-973`，**节选**：源码 7 条分支注释与
`(Some(total), None, Some(previous))` 分支的 953-961 行实现从略）：

```rust
let (tokens, next_totals) = match (total_usage, last_usage, state.previous_totals) {
    (Some(total), Some(last), Some(previous)) => {
        if total == previous { continue; }
        if total.delta_from(previous).is_none()
            && total.looks_like_stale_regression(previous, last) { continue; }
        (last.into_tokens(), Some(total))
    }
    (Some(total), Some(last), None) => (last.into_tokens(), Some(total)),
    (Some(total), None, Some(previous)) => { /* 差分 */ }
    (Some(total), None, None) => (total.into_tokens(), Some(total)),
    (None, Some(last), Some(previous)) => (last.into_tokens(), Some(previous.saturating_add(last))),
    (None, Some(last), None) => (last.into_tokens(), None),
    (None, None, _) => continue,
};
```

即：**计费增量取 per-turn 的 `last_token_usage`**，`total_token_usage` 用于识别重复、跳过回归、
维护水位（`codex.rs:192-207`、`232-247`）；空桶快照不推进水位（`975-983`）。

**修正（对抗复核发现）**：`total` **并非只用于去重**——在 `last` 缺失的降级分支里，
`total.delta_from(previous)` 会直接作为增量（`codex.rs:952-962`），更极端的情况下
（无 last、无 previous）整条 `total` 被当作一次请求（`:964`）。源码注释
（`codex.rs:893-896`）声称 total「never as a direct delta source」，与代码实际行为存在出入；
引用时应以代码为准。

**桶拆分**（`codex.rs:249-275`）：先取 `cached = max(cached_input_tokens,
cache_read_input_tokens)`（`:183-187`），再以 `min(cached, input)` 从 input 中扣除、余数作为
`cache_read`（`:252`）；`reasoning_output_tokens` 从 output 中切出归一为
`reasoning`——因为它是 `output_tokens` 的子集，直接并计会双计（`253-265` 注释）。

**model 来源**：`turn_context.payload.model` 为主（`810-825`，同时重置 turn 锚点），辅以
`info.model/model_info.slug`（`1554-1570`）；token_count 行若尚无模型则暂存，等模型出现再补
（`1080-1087`、`1376-1412`）。

**zstd（提交 `bd62b8c4` "fix(codex): support reading and caching zstd-compressed transcripts"）**：增量续读不支持压缩文件（`start_offset != 0` 直接返回空并置
`parse_succeeded=false`，`codex.rs:1462-1472`）；解压用流式 `zstd::stream::read::Decoder` +
`window_log_max(26)` + `take(MAX_ARCHIVE_BYTES + 1)`（256 MB 上限，`1489-1522`，常量
`sessions/utils.rs:545`），超限或失败即清空消息并标记解析失败。

**replay 去重**：`session_meta` 标记的 fork 子会话会重放父历史，解析器用
`forked_child_waiting_for_turn_context` 门控跳过父阶段的行（`620-690`、`1102-1107`、
`1434-1452`）；token_count 的 dedup key 用**上游累计四桶**＋fork 父标识作用域
（`1056-1066`、`1324-1358`），保证 sibling 重放共享一键、互不相关会话不合并。

### 4.3 Gemini CLI（`sessions/gemini.rs`）

结构：`~/.gemini/tmp/<project-hash>/chats/session-*.json`（会话 JSON）与 `.jsonl` 录制
（`gemini.rs:3-5`、`156-191`）。消息级字段 `id/timestamp/type/tokens/model`（`20-44`）。

- token 字段做了很宽的别名兼容（`51-81`）：input 取 `input|prompt|input_tokens|prompt_tokens|
  promptTokenCount`，缓存取 `cached|cached_tokens|cachedContentTokenCount`，思维链取
  `thoughts|reasoning|thoughts_tokens`；`tool` 桶被并进 input（`274-283`）。
- cached 与 input 重叠时按 `subtract_cached_overlap` / `normalize_*` 相减，并区分「input 已含
  cached」与「净 input」两种语义（`479-483`、`504-552`）。
- **JSONL 去重是「同 id 整条替换」**（`391-397`，后写覆盖先写），不是取最大值。
- 坏行只把 `cacheable` 置 false，避免把截断文件写进缓存（`356-359`、`419-422`）。

### 4.4 文件型适配器的共性模式

**两种取数范式并存**（**已验证**）：

| 范式 | 代表 | 位置 |
| --- | --- | --- |
| 单次值（每行/每请求自带 usage） | Claude Code、Gemini、Cline、Junie、Command Code | `claudecode.rs:696`、`gemini.rs:248`、`cline.rs:238`、`junie.rs:132`、`commandcode.rs:421` |
| 累计快照差分 | Codex、Copilot Desktop、Devin ACP、Grok | `codex.rs:934-985`、`copilot_desktop.rs:115-164`、`devin.rs:418-424`（doc 注释 + 实现起点）、`grok.rs:506-519` |

**坏行容错**普遍是「静默跳过、不 panic」：Claude 逐行 `Err(_) => continue`
（`claudecode.rs:511-515`）；Codex 解析失败忽略该行，但 I/O 失败会置 `parse_succeeded=false`
（`codex.rs:530`、`536-543`）。工具层 `lossy_lines` / `for_each_json_line`
（`sessions/utils.rs:15-33`、`103-123`）用**按行 lossy 解码**替代 `BufRead::lines()`，
避免一个坏字节截断整个转录（注释记录 #1031 的实测损失）。

**自带成本**一律经 `mark_provider_reported_cost`（`sessions/mod.rs:455-465`）标为权威，压过估算价：
Command Code 用每请求 `costUsd`（`commandcode.rs:450-452`）、Cline（`cline.rs:259-261`）、
Junie（`junie.rs:144-146`）、gjc/openclaw 走 `CamelUsage.cost.total`（`sessions/utils.rs:747-779`）。
Freebuff 相反：没有自带用量时按「本回合新增上下文字符 ÷ 4」估算（`freebuff.rs:159-186`）。

**客户端名字段**没有中心映射表：多数是构造函数首参硬编码字面量（`amp.rs:95`、`droid.rs:516`、
`junie.rs:132` 等），少数用模块常量（`commandcode.rs:60`、`grok.rs:24`、`fx.rs:42`），
Roo/Kilo 系通过参数化复用（`pi.rs:795` 的 `client` 参数、`roocode.rs:55` 的 `source`）。

`crates/tokscale-core/src/sessions/utils.rs` 中被广泛复用的工具（**已验证**）：
`lossy_lines`(`:27`)、`for_each_json_line`(`:103`，带物理行号回调)、`extract_i64`(`:191`)、
`parse_timestamp_value`/`parse_timestamp_str`(`:217`/`:237`)、`file_modified_timestamp_ms`(`:292`)、
`parse_json_line`(`:506`)、`estimate_tokens`(`:540`，`chars.div_ceil(4)`)、`is_zst_path`(`:548`)、
`for_each_archive_line`(`:555`)、`session_id_from_path`(`:615`)、`workspace_key_from_path`(`:628`)。

## 5. SQLite 型适配器

### 5.1 只读打开与 schema 探测

**已验证**：全仓唯一「只读打开客户端源库」的打开器是 `sessions/utils.rs:301-306`（`wiki.rs:94`
是 tokscale 自有 wiki 库的读写，不属客户端源库）：

```rust
pub(crate) fn open_readonly_sqlite(path: &Path) -> rusqlite::Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
}
```

- `NO_MUTEX` 的前提是每个 parser 单线程独占连接（注释 `utils.rs:296-300`）。
- **全仓无 `immutable=1`**；唯一例外是 CLI 读 Cursor 桌面 `state.vscdb` 时用
  `file:…?mode=ro` URI（`crates/tokscale-cli/src/cursor.rs:421-425`）。
- PRAGMA 只用于**探测**（`PRAGMA table_info`，`zed.rs:96`、`micode.rs:62`、
  `cherrystudio.rs:428`）；没有 `journal_mode` / `query_only` 设置。
- 忙等只有 OpenClaw 一处：`busy_timeout = 1500ms`（`openclaw.rs:185`、`:624`）。
  其余适配器打开失败即 warn + 返回空集，靠缓存层兜底（`utils.rs:483-495`）。
- 行驱动两层：`sqlite_for_each_row_on_with_params`（`utils.rs:396`）与
  `sqlite_for_each_row`（`:477`）；结果枚举 `SqliteScan`（`:322`）区分
  NotOpened/NotPrepared/NotExecuted/Incomplete/Ran，增量路径用 `completed()`（`:352`）
  拒绝在「只读了前缀」时写高水位。
- 源库被占用时**不做** WAL 特殊处理：不写 sidecar、不 checkpoint，扫描阶段忽略 `-wal`/`-shm`
  （`scanner.rs:676-679`，测试断言 `scanner.rs:3480-3486`）。

多版本 schema 的适配机制 = **候选 SQL 数组顺序 `prepare`，第一个 `prepared()` 的胜出，且不回落旧查询**
（`opencode_schema.rs:1484-1505`）：

```rust
for (index, query) in group.iter().enumerate() {
    let scan = collect_rows(db_path, &conn, query, &mut |row| { /* … */ });
    if scan.prepared() { chosen = Some(index); read_every_row = scan.completed(); break; }
}
```

### 5.2 关键表与列映射（抽样）

| 客户端 | 表 / 形态 | token 列 → 桶 | 备注 |
| --- | --- | --- | --- |
| opencode | V2 `session_message`+`session_v2`/`session`（5 变体）；V1 `message`+`session`（role 在 JSON 内） | 两布局都跑、按指纹去重合并 | `opencode_schema.rs:351-391`、`396-420`、`422-425` |
| kilo / micode | 复用 opencode schema 驱动 | 同上 | `opencode_schema.rs:603`、`610`、`585`；`micode.rs:54-91` 处理缺列旧库 |
| copilot CLI | `assistant_usage_events` | input/output/cache_read/cache_write/reasoning_tokens，input 扣缓存 | `copilot_session_store.rs:16-30`、`89-100` |
| copilot Desktop | `sessions` | `total_input/output/cached/reasoning_tokens` | 按 `session.shutdown` sidecar 差分，避免把生命周期总量压到 created_at（`copilot_desktop.rs:283-297`、`345-353`） |
| goose | `sessions` | `total_tokens/input_tokens/output_tokens`，优先 `accumulated_*` | reasoning 为「总量减 in/out」的推断值（`goose.rs:118-126`） |
| zed | `threads(id, updated_at, …, data)` | 只收 `provider == zed.dev` | data 支持明文/zstd（`zed.rs:178-204`） |
| kiro | `conversations_v2(key, conversation_id, value)` | `history[*].request_metadata` 里的 token_usage（扁平+嵌套双通道） | 别名表 `kiro.rs:1805-1838` |
| devin-cli | `message_nodes.chat_message` | `$.metadata.metrics` | SQL 内 `created_at * 1000`（`devin.rs:171`） |
| workbuddy | `session_usage` 表 | `used` → input | `workbuddy.rs:44-57`、`:93` |
| hermes | 探 `session_model_usage` 存在性 | 逐模型 or 会话总计 | `hermes.rs:177`、`214-218` |
| cherrystudio | `ai_usage_record` + JSONL | 动态拼 SQL（`pragma_table_info` 判列） | `cherrystudio.rs:428`、`444`、`464-596` |
| antigravity-cli / extension | `gen_metadata`、`trajectory_metadata_blob`、`steps(step_type=15)` | protobuf blob 自带 reader | `antigravity_cli.rs:107`、`424`、`482` |

时间列统一归一到 epoch 毫秒：goose 三级格式回退（`goose.rs:31-46`）后秒转毫秒（`:111`）；
micode `normalize_time` 判 `>1e12`（`micode.rs:46-52`）；copilot Desktop 兼容 SQLite
空格分隔 datetime 与秒/毫秒（`copilot_desktop.rs:607-617`）。**时区归一把在适配器外完成**（§8.3）。

### 5.3 直接采用来源金额的适配器

**已验证 + 复核修正**：以下三家的金额直接取自源库，换算口径各不相同——copilot 与 kiro
需要单位换算，cherrystudio 直接采用（且按 §3.2 分类属「混合」数据面）。
全仓同类的自带成本远不止这三家，见下文清单：

| 客户端 | 来源字段 | 金额处理 |
| --- | --- | --- |
| copilot | `assistant_usage_events.total_nano_aiu` | `/ 1e11`（nano AIU → USD），`copilot_session_store.rs:13`、`:126-149` |
| cherrystudio | `ai_usage_record.cost` | 直接采用（`cherrystudio.rs:592`、`:775`），非 USD 币种一律交回 token 计价（`cherrystudio.rs:746-777`） |
| kiro | credit 数 | `credit × 0.04`（`kiro.rs:41`、`:471`、`:1686`） |

同类的"自带成本"**远不止三家**（对抗复核列举 14+ 处，均标 `ProviderReported`）：
`cline.rs:259-261`、`fx.rs:326/369`、`gjc.rs:171+209`、`unsloth.rs:193/256`、
`opencode.rs:131` + `opencode_schema.rs:940-957/1041`（SQLite `cost` 列）、
`cursor.rs:420/547`、`micode.rs:417-419`、`commandcode.rs:451`、`junie.rs:145`、
`lmstudio.rs:556`、`sessions/copilot_session_store.rs:126-149`。其余适配器传 `0.0`
由 tokscale 计价（goose `:128`、zed `:162`、copilot Desktop `:334`）。

### 5.4 粗粒度与本地推理的显式建模

- **Warp 只有聚合，但带金额**（复核修正）：每条记录是账户/工作区聚合，模型名写死
  `aggregate-requests`、token 全 0、请求数进 `message_count`，**金额 = `cents_to_dollars(spend_cents)`**
  （`sessions/warp.rs:74-84`、`:100-117`）。提交路径会按 `is_aggregate_only_warp_row` 把这些
  tokenless 聚合行整行剔除（`crates/tokscale-cli/src/main.rs:6085-6098`、`:6113`）——
  即「本地可见、不参与上报」。
- **本地推理成本为 0 被当作权威观测**：unsloth 对 `providerType == "local"` 返回
  `(local, true)` 并 `mark_provider_reported_cost()`（`unsloth.rs:44-46`、`:192-193`）；
  LM Studio 每条消息 cost 0.0 并同样标记（`lmstudio.rs:553`、`:556`）。
- **不可定价 ≠ 0**：unsloth 对未知/订阅制路由改用 `unpriced:<type>` 命名空间（`unsloth.rs:59-64`），
  保持「未定价」语义而不是误报免费。

## 6. 缓存 / 同步型数据源（上游无可解析本地存储）

这 6 个客户端的数据由 tokscale 自己的子命令抓取并落缓存，扫描器再把缓存当普通文件读：

| 客户端 | 命令入口（CLI） | 缓存落点 | core 解析 |
| --- | --- | --- | --- |
| cursor | `run_cursor_command`（`main.rs:6891`）→ `run_cursor_sync`（`cursor.rs:2001`） | `<home>/.config/tokscale/cursor-cache/usage.json`（旧 CSV，`cursor.rs:103-117`、`:153`） | `sessions/cursor.rs:308`；成本 `totalCents` 优先、退回 `chargedCents`，cents→USD 并标 provider-reported（`:192-201`、`:395-401`；`mark_provider_reported_cost` 在 `:419-421`） |
| antigravity | `main.rs:6919` → `antigravity.rs:215` | `antigravity-cache/{manifest.json, sessions/*.jsonl}`（`antigravity.rs:98`、`:106`、`:774`） | `sessions/antigravity.rs:6` |
| trae | `main.rs:6947` → `trae.rs:1379` | `<config>/trae-cache/manifest.json` + `sessions/usage-<batch_ts>.json`（`trae.rs:1011`、`:1307`） | `sessions/trae.rs:113` |
| warp | `main.rs:7070` → `warp.rs` | `<config>/warp-cache/usage.json`（`warp.rs:84-85`、`:137`） | `sessions/warp.rs:32` |
| hindsight | `main.rs:7079` → `hindsight.rs` | **不在** tokscale 目录：`$HINDSIGHT_HOME/usage/<YYYY-MM>.jsonl`（`hindsight.rs:11-12`、`:141`） | `sessions/hindsight.rs:77` |
| mcode | `tokscale headless mcode` | `<config>/tokscale/headless/mcode/*.jsonl`（`clients.rs:968`） | `sessions/mcode.rs:29` |

证据链细节（**已验证**）：

- **Trae 不是纯账户级**：抓的是 `POST /trae/api/v1/pay/query_user_usage_group_by_session`
  （`trae.rs:1067`），返回**按会话聚合**的行，每行含 `dollar_float` 与
  `extra_info.{input_token,output_token,cache_read_token,cache_write_token}`
  （core `sessions/trae.rs:79-85`）；凭据是账户级，链路为「缓存 token → `ExchangeToken` →
  解密桌面 `storage.json` 的 `iCubeAuthInfo`」（`trae.rs:37-44`、`:270`；safeStorage 解密
  `:660-765`，魔数 `tc\x05\x10` 在 `:702`）。
- **Antigravity 认证不涉云端凭据**：走本地语言服务器进程的 `--csrf_token`（`antigravity.rs:1319`）
  + 端口发现（`:1350`、`:1381`）+ heartbeat 探活（`:1480`）。
- **Warp 只有 GraphQL 聚合**：`GetRequestLimitInfo` / `GetWorkspacesMetadataForUser`
  （`warp.rs:13-14`），凭据支持 Bearer 或 Cookie 头（`:141-158`、`:345-347`）。
- **Hindsight 主动剥离内容**：只落 token/耗时/模型/bank 标识，不含 prompt/completion
  （`hindsight.rs:20`，测试 `:781`）。
- 认证/凭据来源仅记录机制与文件位置；本文不展开、不含任何真实凭据。

## 7. 增量缓存与性能

### 7.1 分片缓存布局与两条版本轴

**已验证**：

- 位置：`<config_dir>/cache/source-message-cache-v2/<client>/shard-<hex>.bin`
  （`paths.rs:120` → `message_cache.rs:94` → `shard_path` `:3069-3071`），
  同级锁文件 `source-message-cache.lock`（`:98`）；**256 个分片**（`CACHE_SHARD_COUNT`，`:72`），
  单分片上限 256 MiB（`MAX_CACHE_SHARD_BYTES`，`:73`）。
- 每片是一个 bincode 信封（手写头 + payload，`bincode 1.3`）。下划线右侧的说明为本文所加；
  源码此处另有 `#[derive(Debug, Serialize, Deserialize)]`：

```rust
struct CachedShardEnvelope {          // message_cache.rs:2526-2531
    format_version: u32,              // wire 布局版本（本文注）
    parser_namespace: String,         // 客户端命名空间（本文注）
    parser_version: u32,              // 该客户端解析器版本（本文注）
    payload: Vec<u8>,                 // CachedSourceEntry 的 bincode（本文注）
}
```

- 两条版本轴**刻意分离**（注释 `message_cache.rs:15-48`）：
  `CACHE_FORMAT_VERSION = 10`（`:49`，wire 布局/`UnifiedMessage` 变化才动），
  按客户端分派的 `parser_version()`（`:1281-1608`；Codex 14 `:1353`、Claude 2 `:1394`）。
  **「独立」有边界（复核修正）**：共享解析器族的成员不独立——族名册
  `SHARED_PARSER_FAMILIES`（`:1135-1259`，如 CodeBuddy/WorkBuddy、Codebuff/Freebuff，
  `:1241-1258`）走 `shared_family_version`（`:1265-1279`），以「族 base + offset」共用同一版本，
  驱动代码一变整族一起失效。
  效果：**一个客户端（或一个族）的解析器升级不会淘汰其他命名空间的缓存**——信封先比
  `namespace + parser_version`（比较点 `:3130-3134`；`:2525-2531` 只是信封结构体定义），
  不符即返回 `Stale`（`:3133`）并登记重写（`:2674-2678`；`:3301` 是 format_version
  未知时的兜底分支）。
- `parser_generation()`（`message_cache.rs:1074-1093`，导出 `lib.rs:28`）**不进缓存 key**，
  只服务 TUI 聚合缓存：把全部 `parser_version` + Claude 保留代次折叠成 FNV-1a u64，
  不一致即 `CacheResult::Miss`（`crates/tokscale-cli/src/tui/cache.rs:812`）。
- 存储粒度是**解析后的 `UnifiedMessage` 列表**（不是原始字节）。
  `CachedSourceEntry`（`:1669-1705`）含 `parser_namespace / parser_version / path /
  fingerprint / messages / fallback_timestamp_indices`，外加 per-client 边车：
  `codex_incremental`（`:867-873`）、`prime_accounting`（`:1691-1694`）、
  `opencode_incremental`（`:1695-1700`）、`micode_metadata`（`:1701-1704`；
  MiMo 用独立信封 `MICODE_CACHE_FORMAT_VERSION = 11`，`:53`、`:3136-3163`）。
- **Claude 是唯一「缓存即唯一副本」的命名空间**：压缩后的历史行只留在缓存里，
  `parser_version` bump 对它是有损而非仅仅变冷——`message_cache.rs` 的 `ClientId::Claude`
  分支带显式 WARNING 注释（`:1384-1393`："bumping this discards data that is not recoverable
  by re-parsing … the cold rebuild that follows sees only the compacted file"），
  `CachedSourceEntry.messages` 的文档同旨（`:1675-1680`），
  相关迁移逻辑见 `lib.rs:1257-1308` 的 `needs_retention_provenance_migration`。

### 7.2 文件指纹与失效判定

**已验证**：`SourceFingerprint`（`message_cache.rs:231-238`）= `size + modified_ns（纳秒）+
sample_hashes + content_hash + related_files`，**没有 inode**。

- 采样：`sample_offsets`（`:3431-3456`）取 `0, max/4, max/2, 3max/4, max` 五点、每点读
  `min(size, 4096)` 字节做 FNV-1a；`max_offset = size - sample_len` 随 append 整体移动，
  **任何 append 都会改变最后一个采样点**。
- 热判定 `primary_fingerprint_matches`（`:809-815`）只比 size + mtime + 采样，
  文档写明「热扫描每文件最多读 20 KiB」（`:251-260`）。
- `ContentHashMode`（`:3477-3504`）：通用客户端用 `SamplesOnly` 并存 `[0u8;32]` 哨兵，
  只有 Codex 与 Prime 用全文 SHA-256。
- 失效：指纹变 → `FingerprintStatus::Changed` → 整文件重解析；
  解析器升级 → 信封 `Stale` → 重写该片；损坏 → `Invalid` → 重写（`:2674-2685`、
  `:3115-3121`、`:3123-3133`）。**源消息缓存没有 TTL**（全仓搜索未见过期字段，
  但此项非穷尽验证；全仓其他 TTL 均不属本层：TUI 聚合缓存
  `CACHE_STALE_THRESHOLD_MS = 5min`（`tui/cache.rs:23`）、订阅用量缓存 300s
  （`commands/usage/mod.rs:344-347`）、TUI remote 缓存 3600s（`tui/remote.rs:23`）、
  定价快照 3600s（`pricing/cache.rs:6`））。

### 7.3 增量续读：文件 append 只有 Codex

**已验证（措辞经复核限定）**：热命中要求 `consumed_offset == fingerprint.size &&
prefix_hash == fingerprint.content_hash` 且 `ends_with_newline`（三个合取项，
`message_cache.rs:3609-3620`）；
size 变大时走 `parse_codex_file_incremental`（`lib.rs:2021-2025`），前置条件是
`codex_prefix_matches`（`:3598-3607`，重算前缀 SHA-256）与 `file_ends_with_newline`
（`:3581-3596`，检查偏移前一个字节是 `\n`）；不满足就 `reparse_from_start`（`lib.rs:1971-1976`）。

- **限定**：这是唯一的**文件级 append 偏移续读**；OpenCode 另有一套 SQLite 变更行 mark
  （实现入口 `lib.rs:1832`，`:1825-1831` 为设计注释；状态字段
  `message_cache.rs:1695-1700`），不属于同一种机制。
- 其余所有客户端走 `load_or_parse_source_with_fingerprint_and_policy`（`lib.rs:1240-1377`），
  指纹变即整文件重解析——**没有通用的 tail/partial-line 续读**，半行问题靠
  `ends_with_newline` 闸口规避。
- `.jsonl.zst` 永远不续读：写缓存时把压缩文件记为
  `consumed_offset = fingerprint.size`（整文件消费，`lib.rs:1147-1153`），读缓存时用
  `if !is_zst_path(path)` 排除尾巴续读（`lib.rs:2016`）。

### 7.4 并发、原子写与多进程

**已验证**：

- 锁用 fs2：读分片取**共享**锁（`message_cache.rs:2642`），写分片取**独占**锁（`:2929`）。
  进程内状态在一个 `Mutex<CacheState>` 后面（`:2548-2576`）。
- 原子写 `write_shard_with_limit`（`:3323-3393`）：写临时文件 → `sync_all` →
  `fs_atomic::replace_file` → 再 `sync_all`；不变式注释写死「绝不在替换完成前删除原分片」。
- `fs_atomic::replace_file`（`fs_atomic.rs:4-14`）：Unix 用 `rename`，Windows 用
  `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`，并对
  `ERROR_ACCESS_DENIED(5)` / `ERROR_SHARING_VIOLATION(32)` 重试 5 次、退避 `10ms × attempt`
  （`:17-76`）——因为杀毒/索引/云同步会短暂持有刚写完的文件。
- 多进程（TUI 与 `tokscale submit` 同跑）：save 是「先读盘上的片、再叠加本进程脏条目」，
  同 key 合并而非覆盖（`absorb_retained_history` `:2452-2506`，调用点 `:3010-3025`）；
  删除也要比对指纹，避免删掉别的进程刚刷新的条目（`:2997-3009`）。
- 损坏片只影响自己：`test_corrupt_shard_does_not_hide_entries_from_other_shards`
  （`:7785-7814`）；无全局容量上限、无 LRU/淘汰；运行时唯一清理入口是
  `remove_legacy_monolith_cache_once`（Once 包装 `:130-133` + 函数体 `:116-125`，
  运行时调用点 `:2624`，删 v1 单体文件；测试专用的 `prune_missing_files`
  `:2869-2870` 带 `#[cfg(test)]`，不属运行时入口）。

### 7.5 性能证据与取舍

- 仓库**没有基准测试**（无 `benches/`、无 criterion）；量化证据是测试级计数器
  `FULL_HASH_CALLS`（`message_cache.rs:78-81`、`:3512-3514`）：热命中**零次**全文 SHA-256
  （`:4043-4056`）、通用客户端冷热都不算全文哈希（`:4091-4130`）、Codex 增量复用指纹不重复哈希
  （`:4020-4040`）。
- 一处内存事故的修复留痕：旧实现把整批消息长期驻留，`-c droid` 单命令峰值 1.16 GB vs
  缓存 358 MB（`:2561-2569`），因此 `take()` 设计成「payload 交给唯一消费者、缓存只留壳」
  （`:2757-2793`）；按命名空间懒加载（`lib.rs:2088-2095`）。
- 另一处热路径复杂度修复：旧代码为每个分片重算所有 key 的分片号（O(shards × keys)），
  冷启动时主导耗时，改为先按分片分桶（`:2934-2939`）。
- 扫描并行度上限 `SCAN_WORKER_CEILING = 4`（`scanner.rs:1989`），理由注释明确：
  「目录遍历阻塞在文件系统而非 CPU，超过少量 worker 只会互相争抢」；扫描用局部
  `rayon::ThreadPoolBuilder` 线程池，构建失败回落全局池（`scanner.rs:2915-2926`）。
- 解析车道普遍 `par_iter()`（`lib.rs` 中实测 86 处调用，如 `2153`、`2292`），
  内存按车道经 `MessageSink`（`lib.rs:832`）逐段释放（设计说明 `lib.rs:825-831`）。

## 8. 聚合口径与时间

### 8.1 分组维度与折叠键

**已验证**：`GroupBy`（`lib.rs:206-249`）6 个取值，键拼装在
`aggregate_model_usage_entries_with_rollup`（`lib.rs:4080`）内（`lib.rs:4107-4118`）：

| GroupBy | 折叠键 | 说明 |
| --- | --- | --- |
| `Model` | 归一模型名 | 跨客户端合并 |
| `ClientModel`（默认） | `client:model` | |
| `ClientProviderModel` | `client:provider:model` | 最细 |
| `WorkspaceModel` | `workspace_group_key:model` | |
| `Session` | `session_id:model` | |
| `ClientSession` | `client:session_id:model` | |

累加点（`lib.rs:4175-4184`）：五个 token 桶 `saturating_add`、`message_count += max(0)`、
`cost += msg.cost`、`performance.record_message`。月/时聚合结构同构（`4330-4336`、`4442-4451`）；
日维度走 `DayAccumulator::add_message`（`aggregator.rs:284-338`），会话维度走
`SessionAccumulator::add_message`（`aggregator.rs:451-506`，顺带把毫秒时间戳归一为秒）。

排序：models 按 cost 降序、NaN 垫底（`lib.rs:4205-4213`）；monthly/hourly 升序（`4354`、`4479`）；
daily 按日期升序（`aggregator.rs:78`）；session 按 last_seen 降序（`aggregator.rs:120-125`）。

**关键设计**：**去重与重放保护全部前移到解析期**，聚合层只按 date 过滤、不再去重
（`lib.rs:5159-5187` 只有日期谓词）。因此重复计费与否完全取决于解析期的 `dedup_key` 纪律
（§4.1/§4.2 的各家规则）。这是明写的信任边界，本文未验证是否存在实际双计实例。

### 8.2 报告结构

- `ModelReport`（`lib.rs:640`）/ `ModelUsage`（`:586`，含 `performance: ModelPerformance` `:298`）；
  `MonthlyReport(V2)`（`:655`/`:667`）、`HourlyReport`（`:726`）、`TimeMetricsReport`（`:4977`）；
  图与日历 `GraphResult`（`:537`）+ `DataSummary`（`:516`）+ `DailyContribution`（`:478`）+
  `YearSummary`（`:507`）；会话 `SessionContribution`（`:492`）。
- **没有 `DailyReport` 类型**：日维度由 `GraphResult.contributions` 承载。
- 时间指标 `TimeMetrics`（`sessionize.rs:235` 起，注入点 `lib.rs:4753-4767`）。

### 8.3 时区分桶：`BucketTimezone`

**已验证**（`bucket_tz.rs:36-45`；源码含 `#[derive]` 与逐变体注释，此处压缩为一行）：

```rust
pub enum BucketTimezone { Local, Pinned(chrono_tz::Tz) }
```

- 唯一权威来源是 settings.json 的 `scanner.bucketTimezone`（`scanner.rs:80-81`）；
  首跑由 `pin_bucket_timezone_if_unset`（`crates/tokscale-cli/src/tui/settings.rs:496-526`）探测并写入。
- 探测：`TZ` 环境变量（**仅 unix**，`bucket_tz.rs:224-229`；Windows 恒返回 `None`，`:250-253`）
  → `iana_time_zone::get_timezone()`（`:215-217`）；候选必须与 `chrono::Local` 在
  `[epoch, now + 10 年]` 上每 30 分钟采样一次的偏移**完全一致**才接受（`:267-287`、`:347-377`），
  否则不 pin。空串/缺失静默降级 `Local`（`:55-57`），无法解析的名称才 warn（`:62-66`）。
- 日键 `day_key` = `%Y-%m-%d`（`:108-113`），时键 `hour_key` = `%Y-%m-%d %H:00`（`:122-127`）；
  无法表示的瞬间返回空串（`:137-146`），调用方保留原日期而不是塞进假桶
  （`sessions/mod.rs:441-447`）。
- 时间链路：解析时先按 `chrono::Local` 派生 date（`sessions/mod.rs:1093-1103`），
  再在 flush 时用 pinned 时区整体 re-key（`lib.rs:944-969` → `rebucket_date`
  `sessions/mod.rs:429-448`；ParsedMessage 车道 `rebucket_parsed_days` `lib.rs:6694-6714`；
  recovery 也对齐 `recovery.rs:322-329`）。
- DST 兜底：`local_day_start` 对 DST 空缺的本地午夜返回 `None`，跳出该 interval 的剩余分配
  （`sessionize.rs:419-432`、`469-479`），歧义时段取 earliest（`:476`）。

**口径差异（代码可见；是否有意为之未验证）**：core 的小时桶用 pinned 时区
（`lib.rs:4427-4430`），而 TUI 的 hourly/minutely 桶从 timestamp 经**本机 `Local`** 重算
（`crates/tokscale-cli/src/tui/data/mod.rs:1185`、`1273`，底层 `1709-1769`）；pin 了非本机时区时
两者的小时归属会不同。日/月桶用已 re-key 的 `msg.date`（`tui/data/mod.rs:953`），不受影响。

### 8.4 会话化与活跃时长

- `DEFAULT_IDLE_GAP_MS = 3 * 60 * 1000`（`sessionize.rs:8`）：按 `(client, session_id)` 分组后，
  相邻消息间隔超过 3 分钟即切段；段内 wall 时长计入活跃时长。
- 每日活跃时长按 wall-clock 占比**分配到各自然日**（`sessionize.rs:389-453`），
  跨日会话两端各取本日份额。
- 子会话归并：`UnifiedMessage.parent_session_id`（`sessions/mod.rs:110-116`）允许把 subagent
  会话滚进父会话（如 OMP 的嵌套目录），`is_turn_start`（`:117-120`）用于区分
  「用户交互轮次」与「API 请求数」。

### 8.5 身份归一

**模型身份双轨**（`lib.rs:77-104`）：

```rust
pub fn canonical_model_id(model_id: &str) -> String { normalize_syntactic(model_id) }
pub fn normalize_model_for_grouping(model_id: &str) -> String {
    model_alias::global().apply(normalize_syntactic(model_id))
}
```

- `normalize_syntactic`（`lib.rs:131-169`）：小写、剥 `(reasoning-tier)` 后缀、剥尾部
  `-YYYYMMDD`、Claude 版本号 `.`→`-`、折叠 `anthropic/claude-…` 前缀。
- 用户别名 `ModelAliasMap`（`model_alias.rs:39-42`）来自 settings.json 的 `modelAliases`
  （载入 `tui/settings.rs:531-533`），键值两侧都过 `normalize_syntactic`，丢空/自映射，
  上限 4096（`:30`、`89-91`），`apply` 单跳不递归（`:114-119`）。
- **分离的理由**（注释 `model_alias.rs:12-21`、`lib.rs:84-88`）：别名是机器本地配置，
  一旦进入上传身份，服务端历史会按设备分裂；因此**提交/上传/导出/持久化路径只用 canonical**
  （`aggregator.rs:298-311`、`:465-468`）。
- provider 归一：`canonicalize_provider_segment`（`provider_identity.rs:1-60`）做小写、
  `-`→`_`、`<…>` 占位返回 `None`、别名折叠（vertex→anthropic、gemini→google、
  openai_codex→openai、azure→azure_ai 等，`:11-57`）；`provider_tags`（`:73-93`）按 `/` 与 `.`
  切段；模型名推断 provider 在 `inferred_provider_from_model`（`:260-367`）。
- **Synthetic 重归属**发生在解析期：模型前缀 `hf:` / `accounts/fireworks/` /
  `accounts/together/`，或 provider ∈ {synthetic, glhf, synthetic.new, octofriend}，
  且**仅当 provider 为空或 unknown 时**才改写为 `synthetic`
  （`sessions/synthetic.rs:19-96`；调用点 `lib.rs:960-965`、`:6664-6666`）；
  客户端字段不变。Sakana 的重归属同理（`provider_identity.rs:268` 的
  `inferred_provider_from_model_inner`，fugu 分支 `:340-342`）。

## 9. 定价与金额计算

这一层是「金额怎么算出来的」的全部答案：**六类上游来源（+1 张用户表）→ 匹配链 → 分档累进公式 → 倍率修正**。

### 9.1 价格来源（3 动态 + 3 内置 + 用户表）

**已验证**：

| 来源 | 载体 | 获取方式 | 位置 |
| --- | --- | --- | --- |
| LiteLLM（主） | `model_prices_and_context_window.json` | 运行时抓 `raw.githubusercontent.com/BerriAI/litellm/main/...` | `pricing/litellm.rs:6-7` |
| models.dev | `https://models.dev/api.json` | 运行时抓，结构 `provider → models → cost{input,output,cache_read,cache_write}` | `pricing/models_dev.rs:7`、`:10-28`、`:79-114` |
| OpenRouter | REST `/models` + `/models/{id}/endpoints` | 先拿列表与兜底价，再对可识别作者前缀并发（10）取端点价 | `pricing/openrouter.rs:22`、`:23`、`:113-156`、`:216-282` |
| 内置 Cursor / Sakana / Tokscale Archive | 编译期常量表 | 无网络 | `pricing/mod.rs:123-226`、`:227-245`、`:285-415` |
| 用户自定义 | `custom-pricing.json` | 本地文件 | `pricing/custom.rs:11`、`:37-73` |

- 三张动态表**并行抓取、各自独立降级**（`mod.rs:416-432`、`:435-457`、`:472-497`）；
  磁盘缓存 TTL = **3600 秒**（`pricing/cache.rs:6`），路径 `<config_dir>/cache/`
  （`cache.rs:12-14` → `paths.rs:120-121`），文件为 `pricing-litellm.json` /
  `pricing-models-dev.json` / `pricing-openrouter.json`；写入走原子替换（`cache.rs:22-103`）。
- config 目录解析顺序：`TOKSCALE_CONFIG_DIR` → macOS `~/.config/tokscale` →
  `dirs::config_dir()/tokscale`（`paths.rs:94-111`）。
- HTTP：30s 超时、3 次重试、200ms 指数退避，仅 5xx/429 重试（`pricing/fetch.rs:7-46`）。
- 进程内 `OnceCell` 只初始化一次（`mod.rs:21`、`:538-543`）；`TOKSCALE_PRICING_CACHE_ONLY`
  可强制只用磁盘缓存（`lib.rs:5388-5393`）。
- 手动入口：`tokscale pricing <model_id> [--provider custom|litellm|openrouter|models.dev]
  [--json]`（`crates/tokscale-cli/src/main.rs:195-208`，分发 `:797`）与 `pricing list-overrides`
  （`main.rs:3740-3753`、`:3960-4063`）。

**费率结构**（`pricing/litellm.rs:10-28`）：`ModelPricing` 17 个 `Option<f64>` 字段，
单位统一为**每 token 美元**（`input_cost_per_token`、`output_cost_per_token`、
`cache_creation_input_token_cost`(±above_200k / above_1hr / above_1hr_above_200k)、
`cache_read_input_token_cost`(±above_200k / above_272k)、以及 input/output 的
128k/200k/256k/272k 长上下文分档）。`custom.rs` 允许 `*_per_million_tokens` 写法并在装载时 `/1e6`
（`custom.rs:351-378`，上限校验 `:367-373`）。**金额单位只有美元，全仓没有汇率换算代码**
（三源快照以美元计这一条属惯例推断，标**未验证**）。

### 9.2 匹配链：模型标识 → 价格条目

主入口 `PricingService::lookup_with_source[_and_provider]`（`pricing/mod.rs:545-585`）：
**custom 最高优先**（`:551-558`、`:571-580`），随后进入 `pricing/lookup.rs` 的
`resolve_with_source_and_provider`（`:619-815`）→ `lookup_auto`（`:821-1156`）。
顺序（**已验证**）：

1. **硬拦截**：`provider_id` 以 `unpriced` 开头 → `None`（`lookup.rs:631-633`、`:3165-3168`）；
   self-hosted（llama.cpp / llama.cpp_modal）→ 全 0 价内置行（`lookup.rs:636-639`、
   `self_hosted.rs:7-21`、`provider_identity.rs:66-71`）；`auto` 之类路由标签 → `None`
   （`lookup.rs:644-646`、`:3160-3163`）。
2. **别名表**（`aliases.rs:268-283`，含 `kimi-code/` 前缀剥离重查；表体 `:4-266`）。
   典型：`kimi-for-coding`→`kimi-k2.7-code`、`minimax-m3`→`minimax/MiniMax-M3`、
   `ox-alpha`→`opencode-go/ox-alpha-free`、`MODEL_PLACEHOLDER_M26`→`claude-opus-4-6`、
   `claude-opus-5.5-high`→`claude-opus-5-5`、`cursor-grok-4.6-*`→`grok-4.6`。
3. **规范化**：剥离 `(minimal|low|medium|high|xhigh|auto|none)` 括号后缀
   （`lib.rs:59-75`，调用点 `lookup.rs:658`）；非白名单的 `xxx(...)` 直接拒绝
   （`lookup.rs:676-683`）；版本分隔符 `.`↔`-` 归一（`normalize_version_separator`，`:2749`）；
   Claude 家族名归一（`normalize_model_name`，`:2551-2606`）；已知/通用 provider 前缀剥离
   （`:2783`、`:2806`）。
4. **候选集择优**：`provider-scoped → 已知前缀剥离 → exact（三源择优）→ 分隔符/名称归一 exact
   → prefix（`:1052-1087`）→ 内置 Cursor/Sakana/Archive（`:1089-1120`）→ fuzzy（`:1122-1155`）`。
   三源之间不是「等级覆盖」，而是 `choose_best_source_result`（`:3637-3691`）按
   「provider 提示匹配 → key 根匹配 → 原始厂商 → 非转售商」比较，最后才回落 LiteLLM；
   models.dev 仅在主选属跨厂商别名时反超（`:3696-3715`）。
5. **剥壳回退**：后缀逐段剥离 ≤4 段（`try_strip_unknown_suffix`，`:2968-2999`）、
   前缀逐段剥离 ≤2 段（`try_strip_unknown_prefix`，`:3060+`）。
6. **fuzzy**：长度 ≥5 且不在黑名单（`auto/mini/chat/base/claude/anthropic/gemini/model/router/
   default`，`:72-83`、`:2946-2951`）；先家族过滤（`extract_model_family` `:2464`）再词边界包含
   （`:2539`），最后 `select_best_match` 排序（`:3292-3630`）。
7. **失败即 `None`**（不猜价、也不假装免费）；结果进程内缓存 512 条（`lookup.rs:85`、`:540-585`）。

### 9.3 计费公式（`compute_cost`，`pricing/lookup.rs:2300-2462`）

**已验证**，分档累进核心：

```rust
let tiered_cost = |tokens: f64, base: Option<f64>, tiers: &[(f64, Option<f64>)]| {
    let base_price = safe_price(base);
    let mut cost = 0.0; let mut lower_bound = 0.0; let mut active_price = base_price;
    for (threshold, tier_price) in tiers {
        let Some(tier_price) = tier_price.filter(|v| is_valid_price_value(*v)) else { continue };
        if !threshold.is_finite() || *threshold <= lower_bound { continue; }
        if tokens <= *threshold { return cost + (tokens - lower_bound).max(0.0) * active_price; }
        cost += (*threshold - lower_bound) * active_price;
        lower_bound = *threshold; active_price = tier_price;
    }
    cost + (tokens - lower_bound).max(0.0) * active_price
};
```

桶映射与收口（`:2337-2342`、`:2461`）：

- `output 桶 = output + reasoning`（`:2338`，reasoning 并入输出）；input 独立；
  `cache_write` 拆 `cache_write_1h`（上限 `min(cache_write)`）与 5 分钟余量（`:2341-2342`）。
- 阈值常量：128k / 200k / 256k / 272k（`lookup.rs:86-89`）。
  input/output 走四档（`:2344-2387`）；cache_read 只有 200k/272k（`:2398-2411`，注释说明上游未给
  128k/256k 缓存价）；cache_write 5 分钟档用 200k（`:2432-2439`），1 小时档用
  `cache_creation_input_token_cost_above_1hr` 系列（`:2419-2447`）；**没有任何 1 小时价时整块按
  5 分钟档的单一 200k 边界计**（`:2449-2459`）。
- **边界语义**：`tokens <= threshold` 全部按低档价（`:2325-2327`；测试
  `test_compute_cost_tiered_boundary_at_200k_uses_base_rates` `:7295-7310` 用 200_000 断言基础价）
  ——即有**跨档时按段累进**，不是整单跳档。
- 合计 = `input_cost + output_cost + cache_read_cost + cache_write_5m_cost + cache_write_1h_cost`
  （`:2461`）。全链路 f64，不四舍五入；展示层 `format!("${:.2}")`（`main.rs:4118-4119`、
  `commands/report.rs:1194`），聚合侧用 `cost.max(0.0)` 消负零（`aggregator.rs:401`、`:410`）。

**例外：整单换价**（`compute_cost_for_lookup`，`lookup.rs:2204-2298`）——与累进分段不同：

- OpenAI 名单（`is_openai_full_request_272k_model` `:2021-2042`）且 provider 为 openai
  且 LiteLLM 行完整（完整性判定 `:2060-2071`）时，总输入 `input + cache_read + cache_write > 272k`
  就把各级率**整体替换**为 above 价、并按 input 倍数缩放 cache_write（`:2241-2297`）。
- xAI grok 200k 同构，边界为 `>=` 且清空高段字段避免二次分档（`:2104-2202`）。

### 9.4 倍率与守卫

| 机制 | 规则 | 位置 |
| --- | --- | --- |
| Zed hosted | 该客户端视角下 `provider_id == "zed.dev"` 时 **×1.1**（官方文档口径：list price + 10%） | `sessions/zed.rs:22`、`lib.rs:5227-5235`（注释明确提醒未来 LiteLLM 若收录已含加价的 zed 行会双重计费） |
| OpenAI fast/priority | `service_tier` 为 `priority`/`fast`，或模型名带 fast 后缀且命中归一化费率时 **×2.0** | `lib.rs:5276-5298`；入口 `calculate_cost_with_service_tier` `:5303-5312` |
| 权威成本守卫 | `has_authoritative_cost()` 为真直接跳过计价 | `lib.rs:5314-5336`（`:5318-5320`） |
| 写回条件 | 仅当 `calculated_cost > 0.0` 才覆盖 cost 并标 `Estimated` | `lib.rs:5332-5335` |

**计价时机（对抗复核修正）**：「只在解析期计价一次」只对 `UnifiedMessage` 链路成立，
全仓存在两个额外计价点，引用时必须区分：

- `ParsedMessage` 链路明确**不计价**（`lib.rs:368-372` 注释 "This lane applies no pricing"），
  由消费者在报告期计算：`tokscale report` 的 `report.rs:167` →
  `compute_msg_cost`（`report.rs:1572-1593`；`ProviderReported` 行直接返回原值 `:1573-1575`，
  其余按 token 重算）。它复用同一个 `PricingService`，因此同一天可能被重算多次。
- **提交闸** `prepare_submission_pricing`（`lib.rs:4865-4973`）会按「提交口径」重写成本：
  无完整定价覆盖的非权威行被清零（`:4951-4952`）并登记 `incomplete_cost_dates`（`:4946`）。
  即**本地估算口径 ≠ 上报口径**，同一份日志在两处的金额可能不同。

### 9.5 特例与陷阱

- **未知模型**：lookup `None` → cost 保持 0.0 且 `cost_source = Unknown`（写回条件排除了 0.0）；
  提交路径显式把不覆盖的用量计 0 并写入 `incomplete_cost_dates`（`lib.rs:4939-4953`，
  与 `TsDailyTotals.cost_is_complete` 对应，`main.rs:4972-4975`）。
- **显式 0 价是真实价格**：`has_any_usable_base_rate`（`litellm.rs:289`）、
  `has_any_usable_pricing`（`lookup.rs:2822`）、`quotes_zero_for_every_published_rate`
  （`litellm.rs:124-136`）；`covers_usage`（`litellm.rs:141-166`）严格区分「缺率」与「零率」。
  代码直言 0 价行混合了「真免费」与「订阅已含」两类，数据本身分不开（`litellm.rs:82-110`）。
- **订阅陷阱的两处硬防护**：LiteLLM 的 `github_copilot/` 前缀整段剔除
  （`mod.rs:27`、`:105-117`）；models.dev 的 `kimi_for_coding` 命名空间行标为不可作
  submission-safe 依据、但仍可用于估计（`lookup.rs:3222`、`:3253-3262`）。
- **跨行借率**：provider 提示命中的行若缺缓存率，可从 canonical 行借**缺失桶**，
  借前要求基础价一致（`resolve_for_usage` `lookup.rs:1921-1969`、`quote_same_base_rates`
  `:1981-2012`）。
- **币种**：Cherry Studio 解析是唯一做币种判断的地方——非 USD 的非零自报金额不被当美元，
  交回 token 计价（`sessions/cherrystudio.rs:746-777`）。

### 9.6 官方口径测试（抽样）

`lookup.rs:7295`（200k 边界用基础价）、`:7313/7332/7354`（分档累进与阈值顺序）、
`:7999`（1 小时缓存写按 1 小时价）、`:8013`（无 1 小时价时只切一次）、`:8041-8059`
（负/Inf/NaN 高段价回退基础价）、`:8605`（显式 0 价可用）；服务层 `mod.rs:1623`
（self-hosted 按 0 计价且覆盖）、`:1890`（既有来源胜过 models.dev 兜底）；
`crates/tokscale-core/tests/anthropic_catalog.rs:132` 镜像 `filter_litellm_data`；
CLI 端到端 `crates/tokscale-cli/tests/cli_tests.rs:1703`、`:4880`、`:4972`。

## 10. 输出与上报

### 10.1 CLI 形态

`main.rs:108` 的 `Commands` 枚举含 `Models / Monthly / Hourly / Pricing / Clients / Tui /
Submit / Autosubmit / Headless / Wrapped / Usage / Report / ...` 等子命令；
`--group-by` 在 `main.rs:89-95`（全局）与 `:121-127`（models）定义，默认 `client,model`。

- `tokscale report`（`commands/report.rs:38` 的 `run_report`）是**任务归属报告**：先解析会话
  （走 §2.2 的独立实现 `parse_local_clients`，并在报告期逐条重算成本，§9.4），
  再用 LLM summarizer 给会话生成标题/任务类别/复杂度，写入本地 wiki 库
  （`WikiDb`，`wiki.rs:1-3` 注释「FM-generated summaries of coding sessions」），
  然后按 workspace/client 过滤汇总（`report.rs:44-115`）。这是 tokscale 里唯一
  需要外部模型参与的功能，与 token 统计主链路相互独立。
- `tokscale import`（`commands/import.rs:98`）支持导入两种第三方导出：
  `clawdboard`（`:190`）与 `ccusage`（`:654`），并带对账容差常量
  （`RECONCILE_RELATIVE_TOLERANCE = 0.01`、`RECONCILE_TOKEN_ABS_TOLERANCE = 2`，
  `import.rs:1005-1007`）。
- TUI（`crates/tokscale-cli/src/tui/`）自建聚合层：`UsageData`（`tui/data/mod.rs:432-440`）
  含 daily/hourly/minutely/monthly/session/project 视图；TUI 侧有两处**独立于 core 的实现**：
  minutely 桶（core 无此粒度，`tui/data/mod.rs:146`、`:1273-1322`）与
  TUI 聚合缓存（`tui/cache.rs:23` 的 5 分钟 TTL，key 含 `parser_generation()`）。

### 10.2 上报 leaderboard

`TsTokenContributionData`（`main.rs:4904-4919`）是上报载荷：`meta / device / scan_scope /
summary / years / contributions[] / time_metrics / mcp_servers / provenance`。
要点：

- 每条日贡献含 `totals{tokens,cost,messages,cost_is_complete}`、`intensity`、
  `token_breakdown{input,output,cache_read,cache_write,reasoning}`、
  `clients[]{client,model_id,provider_id,tokens,cost,messages}`、`active_time_ms`
  （`main.rs:4963-5009`）。
- `scan_scope.parser_versions`（`main.rs:4886-4889`、`5031-5044`）声明每个客户端的解析器版本；
  绝大多数客户端是 `SUBMISSION_PARSER_VERSION = 1`，Copilot 为 2，MiCode 家族为 2
  （`main.rs:4877-4882`、`5036-5039`）。服务端据此决定是否接受该设备的增量。
- survey 类字段（模型名）走 `canonical_model_id`（§8.5），保证别名不污染服务端身份。
- 服务端在 `packages/frontend/src/app/api/submit/route.ts`（1623 行）做校验、合并与
  去重（`validateSubmission`、`mergeClientBreakdownsWithRegressionGuard`、日粒度
  `daily_breakdown` 表、`parserHighWater` 高水位等）；该文件以注释记录新旧客户端别名折叠
  （`LEGACY_CLIENT_ALIASES`：`kilocode → kilo`）。
- 前端 `packages/frontend` 是 Next.js 全栈应用：leaderboard、用户资料、groups、badge/embed SVG、
  设备与上报数据管理 API 都在其中；统计口径与 CLI 无关，本文不展开。

## 11. 与 TokenScope 的对照：值得抄的、需要避的

TokenScope 与 tokscale 是同代问题（多 agent 本地用量统计）的两种解法。可对照的差异点如下
（左侧事实均以本文前述源码证据为准；TokenScope 侧口径见仓库 `CLAUDE.md` 与
`docs/stats-semantics.md`）。

### 11.1 结构差异一览

| 维度 | tokscale（本仓 ref） | TokenScope（现状） |
| --- | --- | --- |
| 客户端注册表 | `clients.rs` 宏生成 56 个 `ClientId`，只描述扫描面；解析映射是手写车道 | 每个 source 适配器自治（发现+解析），无全局注册表 |
| 客户端数量 | 56（+2 重归属），各家格式差异大 | 双 agent（Claude Code、Codex），聚焦深度 |
| 事实存储 | **无历史库**：解析结果在分片缓存里，宁可重建 | `~/.tokenscope/history.db` SQLite 持久事实源，来源文件删除不删用量 |
| 缓存 | 分片 256 × bincode，双版本轴（wire 10 / per-client parser_version） | **解析缓存已退役**：`cache.db` 仅首次升级时只读迁移一次，解析指纹由 `history.db` 的 `source_files` 承接（本仓 `src/report.rs:278-281` 的 `open_cache` 已 `#[allow(dead_code)]`，注释「生产采集路径一律走历史库」） |
| 增量续读 | 仅 Codex 有文件级 append 续读（前缀哈希 + 换行边界）；OpenCode 另有 SQLite 变更行 mark | 文件级 (size, mtime) 指纹跳过；未命中即整文件重解析，无 append 续读 |
| 价格来源 | 6 个上游来源（3 动态 + 3 内置）+ 1 张用户自定义表，候选择优 | 3 个（`pricing.toml` / models.dev / OpenRouter），候选独立、**无编译期内置表** |
| 计价公式 | 分档累进 + 两处整单换价例外 + 两处倍率 | 上下文分段 + 峰谷时间规则 + 多渠道候选取最高费用 |
| 去重位置 | 全部在解析期，聚合层不去重 | 解析期身份 + 历史库求并集 |
| 时区 | 首跑 pin IANA zone 写 settings.json，日键稳定 | settings.toml `timezone` 为唯一权威 |
| 金额权威性 | `ProviderReported` 一经确认不再重估 | **无来源自带成本优先**：事件费用一律本地估价；CCS 导入的 `source_cost_usd` 仅作来源证据保存，不进入主估算（本仓 `src/aggregate.rs:106-110`） |

### 11.2 值得借鉴的实现手法

1. **「注册表只描述扫描面、解析映射独立」的解耦**：`ClientDef` 不含函数指针
   （`clients.rs:285-293`），使 56 个客户端可以共享一套扫描调度，且新增客户端只动
   注册表条目 + 一条车道（车道之间顺序执行、车道内部 `par_iter` 并行）。代价是
   `lib.rs` 变成 2 万行巨物——TokenScope 若客户端数量扩张，
   需在「表驱动」与「文件自治」之间早做选择。
2. **缓存双版本轴**（`message_cache.rs:15-53`）：wire 布局与 per-client 解析器版本分开，
   避免一个客户端升级淘汰全部缓存。TokenScope 现有 `history.db` 的 schema 版本演进可借鉴
   同样的「分级失效」思路。
3. **「缓存即唯一副本」的显式风险标注**：Claude 压缩历史只存在缓存里，
   `parser_version` bump 对它是**有损**而非仅仅变冷（`message_cache.rs:1675-1680` 与
   `:1384-1393` 的 WARNING 注释）；README 也单独警告删缓存会让压缩会话总量变低。
   TokenScope 因有持久历史库，天然规避这类损失——这正是把「缓存」与「事实源」分开的价值。
4. **去重键的作用域纪律**（`claudecode.rs:1139-1159`）：内容派生的 dedup key 全局稳定、
   含会话 id 的 key 只在本文件有效；`dedup_key_is_globally_stable` 决定该行历史可否跨重启保留。
   TokenScope 的原生身份（`(session_id, message.id)` 等）可对照此纪律审查「哪些 key 能跨设备/跨重扫复用」。
5. **`CostSource` 三态与「0 价 vs 未知」的区分**：tokscale 至少三处谓词专门区分「缺率」与
   「零率」（`litellm.rs:124-166`、`lookup.rs:2822`），并在提交层显式输出
   `cost_is_complete: false`。TokenScope 已有「未知不按 0 静默吞掉」的口径，可补上
   *显式 0（真免费）/ 订阅已含 / 未知* 三态在界面上的区分标记。
6. **来源之间「候选择优」而非「等级覆盖」**（`lookup.rs:3637-3715`）：同价条目按
   provider 提示匹配度排序，而不是「优先级高者无条件覆盖」。TokenScope 现行
   「平等候选、完整匹配优先、取最高费用」与此同构，可继续沿用。
7. **指纹不含 inode**（`message_cache.rs:231-238`）：只用 size + mtime + 采样点哈希，
   换来跨设备/跨文件系统可用；采样点含文件末尾（随 append 移动），使「追加」必被检出。
   TokenScope 若做文件级缓存，可直接采用这套指纹设计。
8. **原子写 + 平台重试**（`fs_atomic.rs:17-76`）：Windows 下 `MoveFileExW(WRITE_THROUGH)`
   并对 `ERROR_ACCESS_DENIED`/`ERROR_SHARING_VIOLATION` 退避重试——被杀毒/索引/云同步干扰
   在 Windows 上非常常见，TokenScope 写 `history.db`/settings 的原子替换可复用此策略。
9. **声明式解析器版本**（`main.rs:4877-4882`、`:5031-5044`）：上报载荷携带
   `scan_scope.parser_versions`，服务端据此判断增量可信区间。TokenScope 已在本机侧
   维护解析修订（本仓 `src/history.rs:140` 的 `parser_revision`），若做跨设备/上报合并，
   需把它提升为「随数据一起流动」的声明式元数据。
10. **`unpriced:<type>` 命名空间**（`unsloth.rs:59-64`）：把「不可定价」编码进 provider 字段，
    而不是塞进价格 0——这是比「标记一个 bool」更不易被下游误用的小技巧。

### 11.3 需要警惕或明确否定的部分

1. **聚合层不去重**（`lib.rs:5159-5187` 只按 date 过滤）：整套重复计费防护押在解析期
   `dedup_key` 上。tokscale 自己在多处用「首见即留」「key 必须全局稳定」来兜底，
   但只要某家适配器的 key 粒度错误，聚合层没有任何第二道闸门。TokenScope 若沿用
   「历史库求并集」的路线，应继续保持解析期与入库期两道独立防线。
2. **「一个客户端一个手写车道」的副作用**：`lib.rs` 车道段 2117-3662 约 1500 行、
   大量 `par_iter` + 特例注释；56 客户端的维护成本已体现在 README 与代码的多处不一致
   （§3.4 的 6 条）。客户端数量不是竞争力本身，口径一致性才是。
3. **编译期内置价格表**（`pricing/mod.rs:123-415`）：tokscale 保留了三张内置表作为兜底。
   这与 TokenScope「无编译期内置价格、未收录模型始终未知」的既定决策相反。两种取舍各有
   理由（tokscale 需要覆盖退役模型的历史提交；TokenScope 强调不猜测）。**不建议**照搬，
   但可以借鉴它的表达方式：内置表条目带 `"Tokscale Archive"` 来源标签，让「兜底价」与
   「实时价」在证据层可区分。
4. **小时桶的两套时区口径**（core 用 pinned、TUI 用 `Local`，§8.3）：同一工具内出现
   口径分叉，且代码未说明是否有意为之。TokenScope 的「一次查询只在入口解析一次时间并冻结」
   纪律更稳，应继续坚持「后端冻结、前端不重算」。
5. **订阅/免费额度被记成 0**：tokscale 明确承认无法区分「真免费」与「订阅已含」
   （`litellm.rs:82-110`）。TokenScope 若展示成本，需要在 UI 上避免让这类 0 与
   「本地推理的 0」混同——后者的 0 是权威观测（`unsloth.rs:44-46`），前者是数据缺口的伪装。

## 12. 未验证项与已知风险

### 12.1 复核说明

本文的证据链按三层组织：

1. **切片深读**：7 个独立子代理分别精读「总架构 / 文件型适配器 / SQLite 与缓存型 /
   增量缓存 / 聚合 / 定价 / 覆盖矩阵」，产出带行号的结论（合计引用数百处 `文件:行号`）。
2. **本人抽查**：对跨切片的关键断言逐一打开源码核对——覆盖本文引用的全部核心公式、
   常量与结构体定义（20+ 处，含 `compute_cost` 全式、`CostSource`/`UnifiedMessage`/
   `TokenBreakdown`/`ClientDef` 定义、缓存版本常量、行数口径差异的验证）。抽查全部通过；
   行数口径差异（`Measure-Object -Line` 忽略空行）即在此阶段发现。
3. **对抗性复核**：2 个独立复核代理按「逐条验证 + 主动找反例」模式复查读取链路与
   聚合/计价/缓存共 24 条断言（结论见 §12.2），并对复核新引入的证据行号再次抽查（4/4 通过）。

**证据等级**：本文结论若标注**已验证**，表示源码可证且经复核；**一致解释**表示多条间接证据
一致、无单点直证；**未验证**表示结构推断或环境相关行为，未实测。

### 12.2 复核结论（对抗性验证）

两个独立复核代理按「逐条验证 + 主动找反例」模式复查了 24 条关键断言
（读取链路 12 条、聚合/计价/缓存 12 条）。结果：**完全通过 14 条**，**部分正确 8 条**，
**不成立 2 条**（14 + 8 + 2 = 24）。全部偏差已在正文中修正，主要如下：

- **不成立 1：「唯一解析主干」**——实际存在第二套独立实现 `parse_local_clients`
  （§2.2 已改，并补出车道段两处的范围）。
- **不成立 2：「金额直接来自源库只有三家」**——全仓 14+ 处；三家属 SQLite 类带换算的特例
  （§5.3 已改，附反例清单）。
- **部分正确 1：「每客户端独立 parser_version」**——共享解析器族共用版本
  （§7.1 已改，补族名册与比较点行号）。
- **部分正确 2：「Codex 的 total 只用于去重」**——降级分支里 total 直接作为增量源，
  源码注释与代码自相矛盾（§4.2 已改）。
- **部分正确 3：「只在解析期计价一次」**——跨链路不成立：`ParsedMessage` 车道不计价、
  由 `report` 在报告期重算，另有提交闸重写（§9.4 已补）。
- **部分正确 4：「日期过滤只有一个入口」**——有两个（§2.2 已补）。
- **额外修正**：warp 聚合行**携带金额**且提交时被剔除（§5.4 已改）；
  「只有 Codex 有增量」需限定为文件级 append 续读，OpenCode 另有行级增量（§7.3 已限定）；
  忙等/`immutable`/`NO_MUTEX` 等「唯一」类断言经全仓搜索未见反例，维持原判。
- **复核补充的同层级机制**（已并入正文或在下方列出）：
  - 双解析实现需要手工对齐语义，测试用 `local` vs `unified` 成对断言兜底；
  - 真正按客户端索引的表在缓存层：`CacheIdentity::for_client`（`message_cache.rs:1029`），
    决定各车道的缓存命名空间与指纹策略；
  - `workbuddy` 的跨文件合并按 session 而非日期（`lib.rs:3705-3735`）；
  - recovery overlay 只在 `GraphPricingRequirement::Lenient` 下应用，提交路径永不含 overlay
    （`lib.rs:4575-4581`），可用 `TOKSCALE_RECOVERY_DISABLE` 关闭；
  - 提交闸 `prepare_submission_pricing`（`lib.rs:4865-4973`）是「本地口径 ≠ 上报口径」的
    唯一实现点（已并入 §9.4）。

**第 3 轮（分章独立复核）**：4 个复核代理按章节切分（§3 覆盖矩阵 / §4–§5 适配器 /
§6–§7–§10 缓存与上报 / §8–§9 与附录）各自独立验证，全部引用「带文件名的行号」逐条打开源码核对，
并对矩阵 79 处解析入口引用做全量定位（58 行数据面中「一行多入口」展开后的引用数）。结果：

- 表格与结论主体成立（矩阵 58 行 id 集合与注册表双向差集为空、路径列 20/20 一致、
  附录 A 19/21 行逐值正确、§9.3 计费公式片段与源码逐 token 一致）。
- 但**行号精度与表述**共修正 30+ 处，典型：§3.3 两处分类计数（JSON 快照 10→9、混合 10→11）、
  `scanner.rs` WAL 处理（671-672→676-679）、`lib.rs:1384-1393`（实为泛型签名，
  正确出处是 `message_cache.rs:1384-1393` 的 WARNING 注释）、附录 A 的 zstd 位置
  （`codex.rs:1489`→`:1503`）、`incomplete_cost_dates` 登记行（`lib.rs:4946`）、
  路由文件行数（1529→1623）、`devin.rs` 引用改为 doc 注释起点、`spendCents`→`spend_cents`
  （源码标识符）、§3.4 的 `PathRoot::Config` 条目顺序（Antigravity/Trae/Warp）等。
- **一条复核建议被证伪**：复核建议补充「清理入口还有 `prune_missing_files`（`message_cache.rs:2870`）」，
  但该函数带 `#[cfg(test)]`（`:2869`），仅测试专用；运行时唯一入口仍是
  `remove_legacy_monolith_cache_once`（调用点 `:2624`）。文档按证伪后的结论书写——
  复核本身也需要被验证。

**第 4 轮（收敛复核）**：3 个复核代理分别做「验证第 3 轮修正的落实」「全量简写行号抽查」
「读者视角通读」。结果：

- **修正落实**：20 条抽验中 18 条完全落实；不精确 2 条（§4.2 分支数 8→7；
  §5.3 的「均为 SQLite 数据面」限定语与矩阵分类冲突）。
- **简写行号**：提取全文 247 处简写引用（首行号 ≥300 的 178 处）逐一归属并核对，
  **命中 319 处、偏差 1 处、无法定位 0 处**；唯一实质偏差是 §2.2 的时区重分桶行号
  （`960-971` → 实为 `967-969`，`960-965` 是 synthetic 字段归一）。
- **最严重的发现来自 §11（本仓对照章节）**：两处对 TokenScope 现状的描述与事实相反——
  ①「现有缓存机制与 `history.db` 并存」不成立（解析缓存已退役，`cache.db` 仅只读迁移一次，
  `open_cache` 带 `#[allow(dead_code)]`，本仓 `src/report.rs:278-281`）；
  ②「同源（自带成本优先）」不成立（本仓无来源自带成本优先，事件费用一律本地估价，
  CCS 的 `source_cost_usd` 仅作来源证据、不进主估算，本仓 `src/aggregate.rs:106-110`）。
  另有一处判断失真：TokenScope 已有 `parser_revision`（本仓 `history.rs:140`），
  不是「尚缺解析器代次」。三处均已按本仓源码改正。
- 其余一并修正：§9.1/§11.1 的来源计数口径（上游 6 个 + 用户表）、§7.2「无 TTL」与
  §9.1「只有美元」的证据档位措辞、附录 B 的索引补录与区间统一（`988-1045`、`3323-3393`）、
  §3.4 标题计数（7→6）、README 清单口径（46 个 ClientId，清单另含 synthetic）等。

**收敛判定**：第 4 轮后残留问题为 0 个实质错误（仅剩风格级建议，如增大目录、术语微调），
判定收敛；后续若再发现新证据，按同一「修正 → 再复核」流程处理。

**第 5 轮（终局复核）**：2 个复核代理分别做「第 4 轮修正落实验证」与「随机 35 处终局抽查」。

- 修正落实：20 条中 18 条完全正确；§5.3 首句的矛盾已按复核给出的精确改法改正；
  另 4 处措辞/排版小项（§11.1 价格来源口径、§6 cursor 的 `mark_provider_reported_cost`
  位置补 `:419-421`、§12.2 段落空行、目录条目措辞）一并落实。
- 终局抽查：35 处删选覆盖 §2–§12，**33 处逐字命中、未发现新的实质错误**；
  2 处轻微偏差已修（§7.3 热命中条件补第三合取项 `ends_with_newline`；
  §2.2 车道清单 Devin 起点 `3391`→`3388-3394`，并标注清单为节选）。
- 数字类断言由复核代理独立重建验证：56 个 `ClientId`、README 清单 47 项
  （46 + synthetic）、`par_iter` 86 处、Codex parser_version 14 / Claude 2、
  codex 取数矩阵 7 臂、zstd 的 1489/1503/1515、fs_atomic 重试 5 次 × 10ms、
  阈值 128k/200k/256k/272k、lookup 缓存 512、TTL 3600、import 容差 1%/2/$0.01。

**最终收敛声明**：五轮复核（1 轮深读 7 代理 → 2 轮对抗复核 2 代理 → 3 轮分章复核 4 代理
→ 4 轮修正验证与通读 3 代理 → 5 轮终局抽查 2 代理）后，未再发现实质错误；
剩余条目仅为风格建议（目录/术语微调），不影响任何结论。文档定稿。

### 12.2.1 跨解释器复核（pwsh 7.6.6，定稿追加）

本会话的 `pwsh` 工具实际运行 Windows PowerShell 5.1（默认按 ANSI 解码），而系统上另有
PowerShell 7.6.6；中文文本在 5.1 默认编码下会被吞掉行尾换行，可能影响"以行计数为口径"
的统计。为排除该风险，全部行号级结论在 **pwsh 7.6.6（Core）** 下重新执行了一遍：

- 文档完整性：1273 行（`ReadAllLines` / `Get-Content` 默认 / 显式 UTF-8 三种口径一致）、
  18 个代码围栏、14 个表格块无列数错配。
- 源文件：tokscale 仓库 **189 个 `.rs` 文件**在默认编码与显式 UTF-8 下的行数完全一致
  （即源码行号不受该编码差异影响）；本文引用的 **94 个文件 0 处行号越界**。
- 行号抽查：68 处（20 处核心 + 48 处扩展，覆盖 §2–§12 与关键常量）在 pwsh 7.6.6 下
  逐一打开源码核对，**全部命中**（11 处初判不符经逐行查看均为抽查脚本的期望串选择问题，
  文档本身正确，如 `:2674` 是 Stale 分支入口、`estimate_tokens` 的 `:538` 是文档注释行）。
- 口径声明复核：`message_cache.rs` 实际 9222 行 vs `Measure-Object -Line` 的非空计数 8534 行，
  在 pwsh 7.6.6 下同样成立（文档头部口径说明无误）。

结论：跨解释器复核未发现任何新错误；行号级结论与解释器无关。

### 12.3 各切片的未验证清单（不得外推）

**读取与架构**

- `recovery.rs` 的完整语义（用量账本恢复覆盖层）只按模块注释与两处调用点采信，
  未逐行跟踪 submit 调用链。
- `/orca/mimocode-hooks` 第二来源在所有平台无条件附加，未验证 Windows/Linux 上是否真实存在，
  也未找到对应测试。
- `~/.claude/transcripts/` 由哪个 Claude Code 版本写入、与 `projects/` 是否内容重叠，未深挖。
- Claude 的 cc-mirror 变体扫描在多账号场景下是否与主根重复计费，未验证。
- Gemini 是否给消息设置 `dedup_key`：模块内未见赋值，跨文件重放是否依赖上层缓存合并**未验证**。
- Codex 的 UUID v7 前缀 replay 判定（`codex.rs:1195-1280`）只读片段，未逐分支验证。
- Codex headless 路径的 `cached_input_tokens` 处理（`codex.rs:1711-1730`）只读片段。
- openclaw `transcript_events` 的完整列语义、kiro 的 credit 分摊细节（`kiro.rs:455-471`）、
  trae 会话行完整字段集，均只读片段。
- TUI 实时区时区（`Local`）与 core pinned 时区的差异是否为有意设计，注释未说明。

**缓存与性能**

- 全仓未发现缓存全局容量上限、LRU、TTL 或内建清理命令；未穷尽 TUI 启动期清理逻辑。
- 注释中的性能数字（热扫描 ≤20 KiB/文件、1.16 GB 峰值）无法在本环境复现（无基准测试）。
- `.jsonl.zst` 每次变化整文件解压重解析，其追加频率与代价未实测。
- `SourceCachePolicy::InMemory` 的实际调用方未穷尽（只见定义 `lib.rs:772-790`）。

**计价与聚合**

- models.dev 与 OpenRouter 数据以美元计价这一点，仓库内没有明文断言（惯例推断）。
- `select_best_match` 的排序尾段（`lookup.rs:3412-3630`）与 `select_endpoint_pricing`
  （`openrouter.rs:216-282`）未逐行核对。
- 内置三张价格表条目未穷举统计（只抽查若干条与注释）。
- 聚合层不做跨车道全局去重是否会导致实际双计，未验证（设计上的信任边界）。

### 12.4 对 TokenScope 的迁移风险提示

- tokscale 的**「缓存即事实源」**模式对「重扫代价」很不友好：删缓存即重解析全部历史。
  TokenScope 的 `history.db` 模式在这一点上更安全，但也带来相反的代价——**旧的错误解析会
  长期留在库里**。tokscale 的 `recovery.rs` 覆盖层正是为「账本级修复」而设，提示
  TokenScope 需要等价的「历史重算/修复通道」而不是「删库重扫」。
- tokscale 的 56 客户端覆盖含 6 个「自建缓存型」来源（cursor/trae/warp/hindsight/antigravity/mcode），
  它们依赖 CLI 主动 `sync`。若 TokenScope 未来扩展这类来源，需要明确「缓存新鲜度」
  在界面上的表达能力（tokscale 只有 manifest 与批时间戳，无 UI 提示）。
- **双解析实现的教训**：`parse_local_clients` 与 `parse_all_messages_streaming` 是两份
  独立代码（§2.2），语义靠手工对齐与成对测试维持。TokenScope 目前只有一条管线；
  若未来为「任务归属报告」之类消费方引入第二条，必须像 tokscale 一样为每个适配器准备
  `local` vs `unified` 的成对断言，否则口径漂移不会被发现。
- **本地口径与上报口径必须分开定义**：tokscale 的 `prepare_submission_pricing`
  （`lib.rs:4865-4973`）会在提交时把「定价覆盖不完整」的用量清零并标记
  `incomplete_cost_dates`。这是「本地看到 $X、上报记 $0」的合法差异——如果 TokenScope
  将来有跨设备/共享面板，必须提前定义哪一侧是权威、以及如何在 UI 上解释差异。

## 附录 A：关键常量与阈值速查

| 常量 | 值 | 位置 |
| --- | --- | --- |
| 会话切分空闲阈值 `DEFAULT_IDLE_GAP_MS` | 3 × 60 × 1000 ms | `sessionize.rs:8` |
| 缓存分片数 `CACHE_SHARD_COUNT` | 256 | `message_cache.rs:72` |
| 单分片上限 `MAX_CACHE_SHARD_BYTES` | 256 MiB | `message_cache.rs:73` |
| 缓存 wire 版本 `CACHE_FORMAT_VERSION` | 10（MiCode 独立 11） | `message_cache.rs:49`、`:53` |
| 解析器版本（示例） | Codex 14；Claude 2 | `message_cache.rs:1353`、`:1394` |
| 指纹采样 | 5 点 × 4096 字节 | `message_cache.rs:74-75` |
| 热扫描读量上限（注释） | ≤ 20 KiB/文件 | `message_cache.rs:251-260` |
| 定价缓存 TTL | 3600 s | `pricing/cache.rs:6` |
| 分档阈值 | 128k / 200k / 256k / 272k tokens | `pricing/lookup.rs:86-89` |
| 计价模糊匹配最小长度 | 5 | `pricing/lookup.rs:91` |
| 扫描并行上限 `SCAN_WORKER_CEILING` | 4 | `scanner.rs:1989` |
| zstd 解压上限 `MAX_ARCHIVE_BYTES` | 256 MiB（`window_log_max(26)`） | `sessions/utils.rs:545`、`codex.rs:1503`（Decoder 建于 1489，`take(上限)` 在 1515） |
| 上报解析器版本 | 通用 1 / Copilot 2 / MiCode 2 | `main.rs:4877-4882` |
| Zed hosted 倍率 | ×1.1 | `lib.rs:5227-5235` |
| OpenAI fast/priority 倍率 | ×2.0 | `lib.rs:5276-5298` |
| Kiro credit 单价 | $0.04 / credit | `kiro.rs:41`、`:1686` |
| Copilot nano AIU 换算 | ÷ 1e11 | `copilot_session_store.rs:13` |
| TUI 聚合缓存 TTL | 5 min | `tui/cache.rs:23` |
| 订阅额度缓存 TTL | 300 s | `commands/usage/mod.rs:344-347` |
| 来源缺用量元数据时的 token 估算 | `chars.div_ceil(4)`（与价格表无关） | `sessions/utils.rs:538-541` |
| 导入对账容差 | 1% / 2 tokens / $0.01（`RECONCILE_RELATIVE_TOLERANCE` / `..._TOKEN_ABS_TOLERANCE` / `..._COST_ABS_TOLERANCE`） | `commands/import.rs:1005-1007` |

## 附录 B：模块行号索引（节选：正文引用到的模块与函数）

| 文件 | 关键内容 | 行号 |
| --- | --- | --- |
| `crates/tokscale-core/src/lib.rs` | 模型名归一 / GroupBy / TokenBreakdown / 报告类型 | `89`、`102`、`131`、`206`、`252`、`537-730` |
| | 解析主干与两份实现 | `1011`、`4247`、`5436`、`6751` |
| | 缓存交接（读取/写回） | `1240`、`2092`、`3655` |
| | 出口过滤与时区重分桶 | `930`、`944`、`3737`、`5159-5187` |
| | 聚合与计价入口 | `4080`、`4107-4118`、`5314-5336` |
| `sessions/mod.rs` | `CostSource` / `UnifiedMessage` / 成本标记 | `73-78`、`81-125`、`455-465` |
| | workspace slug 解码 | `868`、`883-895`、`960-1045` |
| `clients.rs` | `PathRoot` / `ClientDef` / `define_clients!` | `2-18`、`285-318`、`320-401` |
| `scanner.rs` | 扫描入口 / 并行 / 后缀规则 / 并行上限 | `1929`、`2915-2926`、`513-535`、`1989` |
| `message_cache.rs` | 版本常量 / 信封 / 分片路径 / 锁 / 原子写 | `49-53`、`2525-2531`、`3069-3071`、`2642`、`2929`、`3323-3393` |
| | 指纹与采样 / Codex 续读判定 | `231-260`、`3431-3456`、`3581-3620` |
| `pricing/lookup.rs` | 匹配主链 / 计费公式 / 整单换价 / 阈值 | `619-1156`、`2300-2462`、`2021-2298`、`86-89` |
| `pricing/mod.rs` | 服务装配 / 降级 / custom 前置 / 内置表 | `416-497`、`545-627`、`123-415` |
| `pricing/litellm.rs` | URL / 费率结构 / 零价与覆盖判定 | `6-7`、`10-28`、`124-166`、`289` |
| `pricing/aliases.rs` | 别名表 / 解析入口 | `4-266`、`268-283` |
| `pricing/cache.rs` | 定价缓存 TTL / 原子写 | `6`、`22-103` |
| `pricing/fetch.rs` | HTTP 超时与重试 | `7-46` |
| `pricing/openrouter.rs` | API 根 / 端点价择优 | `22`、`113-156`、`216-282` |
| `pricing/models_dev.rs` | URL / 单位换算 | `7`、`79-114` |
| `pricing/custom.rs` | 用户价格表装载 | `11`、`351-378` |
| `sessions/claudecode.rs` | 去重键 / 合并 / subagent / advisor | `632-636`、`988-1045`、`88-183`、`829-871` |
| | dedup key 全局稳定性判定 | `1139-1159` |
| `sessions/codex.rs` | 取数矩阵 / 桶拆分 / zstd / replay | `934-985`、`249-275`、`1462-1522`、`1324-1358` |
| `sessions/gemini.rs` | token 别名 / 重叠相减 / 同 id 替换 | `51-81`、`504-552`、`391-397` |
| `sessions/opencode_schema.rs` | 多版本 schema 探测 | `351-425`、`1484-1505` |
| `sessions/utils.rs` | 只读 SQLite / 逐行读 / 时间与估算工具 | `301-310`、`103-123`、`191-292`、`540-555` |
| `bucket_tz.rs` | Local/Pinned / 探测校验 / 日键 | `36-45`、`267-287`、`108-146` |
| `sessionize.rs` | 空闲阈值 / 活跃时长分配 | `8`、`389-453` |
| `model_alias.rs` | 别名装载与折叠 | `39-42`、`86-149` |
| `provider_identity.rs` | provider 归一与推断 | `1-93`、`260-367` |
| `sessions/synthetic.rs` | 重归属规则 | `19-96` |
| `crates/tokscale-cli/src/main.rs` | 命令枚举 / 上报载荷 / 解析器版本声明 | `108`、`4904-5026`、`4877-4882`、`5031-5044` |
| `crates/tokscale-cli/src/commands/report.rs` | 任务归属报告 | `38-115`、`1572-1593` |
| `crates/tokscale-cli/src/tui/data/mod.rs` | TUI 数据层（含 minutely） | `629`、`146`、`1273-1322` |
| `crates/tokscale-cli/src/{cursor,trae,warp,antigravity,hindsight}.rs` | §6 的 sync 客户端（命令实现） | 见 §6 表 |
| `crates/tokscale-core/src/fs_atomic.rs` | 原子替换与 Windows 重试 | `4-14`、`17-76` |
| `crates/tokscale-core/src/aggregator.rs` | 日/会话/汇总/图聚合 | `14`、`93`、`131`、`231` |
| `packages/frontend/src/app/api/submit/route.ts` | 上报服务端校验与合并 | 全文（1623 行） |

---

**文档版本**：2026-10-14 首版。研究对象固定于 `bd62b8c4`；tokscale 迭代很快，引用行号
在后续提交中可能漂移，复核时以符号名（函数/常量名）为准二次定位。
