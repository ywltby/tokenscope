# M1：Cargo 骨架 + Claude Code 使用统计闭环

> **2026-10-11 归档复核：已实现。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 任务 1–8 / 验收 1–4 | 原里程碑实现和自身验收已有记录 | 无独立待办 |
| 旧 CLI / 内置价格 / sidechain 排除 / slug 身份 | 被后续用户需求和计划取代 | 不恢复 |

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 状态：**已完成（2026-10-03）**，验收记录见文末
- 创建：2026-10-03

## 目标

建立 TokenScope 的 Cargo 工程（单 crate `tokenscope`，edition 2024），并打通第一个 agent——Claude Code——的完整统计闭环：

```
扫描 ~/.claude/projects/**/*.jsonl → 解析为 UsageEvent → 聚合（日/模型/项目）→ 终端表格 + JSON
```

本里程碑**不落盘**：每次运行全量扫描、实时聚合，输出仅走 stdout/stderr。

## 非目标（明确不做）

- Codex 及其他 agent 适配器（M2；Codex 日志形态已初探：`~/.codex/sessions/**/rollout-*.jsonl`，含 `session_meta` / `response_item` / `token_count` / `token_usage_record` 记录类型）
- 价格表外置配置、models.dev 导入（后续里程碑；cc-switch 的定价走 models.dev 同步，非简单映射，不宜照抄）
- TUI、趋势图、TokenScope 自有缓存数据库
- 跨设备/远程数据

## 数据事实（2026-10-03 本机实测，作为口径依据）

对本机 9 个项目目录、19 个 jsonl 文件、12385 条 assistant 记录的抽样结论：

1. assistant 记录 `message.usage` 必有四个规范字段：`input_tokens` / `output_tokens` / `cache_creation_input_tokens` / `cache_read_input_tokens`；另有 `cache_creation`（对象）、`server_tool_use`、`service_tier` 等新字段，M1 忽略。
2. **重复严重**：3388 个重复 `message.id`、共 8620 条多余行（约 70% 冗余）；99.7% 的重复行 usage 逐字节相同（时间戳相差毫秒，流式重写），11 个 id 的 usage 随重写增长。→ 去重必须做，保留**最后一条**（按时间戳，相同则按行序）。
3. `isSidechain=true` 本机当前为 0 条，但字段存在；子代理链路默认不计。
4. 模型串任意：本机经 cc-switch 路由，实际为 `grok-4.5-build` / `gpt-5.6-luna` / `deepseek-v4-flash-0731` 等，另有 `<synthetic>`（usage 全零的合成行）。
5. 时间戳为 UTC ISO-8601 带毫秒与 `Z` 后缀（如 `2026-07-17T08:44:46.746Z`）。
6. 项目标识：`projects/<slug>/` 目录名（路径特殊字符替换为 `-`）；行内另有 `cwd` / `gitBranch` 可后续利用，M1 用目录名。

## 不变量（先于代码）

1. **只读**：对 `~/.claude` 下一切文件只以只读方式打开；不创建/修改/删除 agent 目录内任何内容；M1 全程不写任何文件。
2. **健壮**：任意单行解析失败跳过并计数，不得 panic、不得中断所在文件或整体扫描；坏行计数在 `--json` 输出与表格脚注可见。
3. **去重口径**：同一 `(sessionId, message.id)` 只计一次，保留时间戳最后一条；`isSidechain=true` 不计；`model == "<synthetic>"` 跳过并计数。三条均在 fixture 固化。
4. **时间口径**：记录按行内 UTC 时间戳解析，聚合按 Asia/Shanghai（UTC+8，无夏令时）自然日落日。
5. **费用口径**：input / output / cache_write / cache_read 四类分别计价；模型不在内置价格表时费用为 `unknown`，**不得按 0 静默吞掉**，且 unknown 的 token 量单独可见。
6. **层间契约**：`source` 层输出 `UsageEvent` 后不得再暴露 Claude 特有字段；`aggregate` / `render` 不 import `source` 内部类型；新增 agent 时聚合与渲染层零改动。
7. **目录缺失**：projects 目录不存在时输出警告并以空结果正常退出（退出码 0）——机器上没装该 agent 是常态。

## 模块与依赖（单 crate，不过早拆库）

```
src/
  main.rs            # clap 入口、错误出口
  model.rs           # UsageEvent、聚合结构（agent 用枚举 + 字符串标识）
  source/mod.rs      # Source trait（discover+collect → Vec<UsageEvent> + 采集统计）
  source/claude.rs   # Claude Code 适配器
  aggregate.rs       # UTC→本地日界、按日/模型/项目聚合
  pricing.rs         # 内置静态价格表（USD / 百万 token，四类单价；仅收录公开牌价的常见模型）
  render/mod.rs      # 表格（comfy-table）
  render/json.rs     # JSON 输出（serde_json）
tests/fixtures/claude/   # 合成 jsonl（含典型/重复/坏行/sidechain/synthetic/空目录）
tests/e2e_claude.rs
```

依赖：`serde` / `serde_json` / `clap`(derive) / `jiff`（时区）/ `comfy-table` / `anyhow` / `dirs`（home 解析）。

## CLI（M1 面）

```
tokenscope summary                      # 默认按日聚合，终端表格
tokenscope summary --by model|project|day
tokenscope summary --json               # 机器可读，含坏行/跳过计数
tokenscope summary --days N             # 最近 N 个自然日（本地时区），默认全部
tokenscope summary --claude-dir <path>  # 覆盖发现根目录（测试/e2e 用）
tokenscope --version
```

## 任务清单（代码位置 / 测试名 / 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | Cargo 骨架 + clap + `--version` | `Cargo.toml`、`src/main.rs` | `test_version_flag` | `/c/Users/admin/.cargo/bin/cargo.exe run -- --version` |
| 2 | `UsageEvent` 与采集统计结构 | `src/model.rs` | `test_usage_event_` | `cargo test model` |
| 3 | 发现：`projects/**/*.jsonl` 递归；缺失目录 → 警告+空 | `src/source/claude.rs` | `test_discovery_`（tmp 目录 fixture） | `cargo test discovery` |
| 4 | 解析：assistant 行 usage → 事件；去重保末条；排除 sidechain / synthetic；坏行跳过计数 | `src/source/claude.rs` | `test_parse_typical` / `test_parse_badline` / `test_dedupe_keeps_last` / `test_sidechain_excluded` / `test_synthetic_skipped` | `cargo test claude` |
| 5 | UTC→Asia/Shanghai 落日 + 按日/模型/项目聚合 | `src/aggregate.rs` | `test_daily_boundary`（UTC 15:59/16:00 分属两日）/ `test_aggregate_` | `cargo test aggregate` |
| 6 | 内置价格表 + 四类计价；unknown 可见 | `src/pricing.rs` | `test_pricing_unknown_model` / `test_pricing_` | `cargo test pricing` |
| 7 | 表格 / JSON 渲染 | `src/render/` | `test_render_table` / `test_render_json` | `cargo test render` |
| 8 | 端到端：fixture 目录 → summary | `tests/e2e_claude.rs` | `test_e2e_summary_table` / `test_e2e_summary_json` | `cargo test` |

## 验收

1. `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 全绿。
2. 对本机真实 `~/.claude` 运行 `summary --json`：坏行计数为 0 或逐条可解释；抽样一天的总 token 与 cc-switch 使用统计面板目测同量级，差异写回本文档。
3. 只读不变量：运行前后 `~/.claude` 目录树与文件 mtime 无变化。
4. `--claude-dir` 指向不存在目录：stderr 警告、退出码 0、空结果。

## 风险

- Claude Code 字段随版本演进 → serde 宽松解析（未知字段忽略、缺失字段按默认），坏行计数暴露异常而非崩溃。
- 去重口径与 cc-switch 不完全一致 → 验收第 2 条抽样对照，差异原因记录进本文档后关闭。
- `<synthetic>` / sidechain 之外未来可能出现新的零成本行类型 → 以坏行/跳过计数观察，出现再定口径。

## 验收记录（2026-10-03）

1. `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 全绿；21 个单测 + 2 个 e2e（fixture 断言含手算费用精确值）。
2. 真实 `~/.claude`：19 文件 / 30,743 行 → 3,741 事件（去重丢 8,620、坏行 0、跳过 sidechain 0 / synthetic 24），与开发期 Python 探查数字一致。
3. **cc-switch 对照**（读取其 `cc-switch.db` 的 `usage_daily_rollups`，比目测面板更精确）：claude 平面 15 个共有日期中 10 天五项指标（请求数/输入/输出/缓存读/缓存写）完全一致；5 天差异 ≤0.6%，成对分布：
   - `07-31`（cc 多 2 请求 / +20,557 in）、`08-06`（cc 多 1 请求 / +45,979 in）：cc-switch 的 rollup 保留了其后被轮转/重写的源文件历史，本工具只反映磁盘现状；
   - `08-05`（cc +18,534 in / +44 out，请求数同）：同一 message 的流式重写在文件重写后快照时点不同；
   - `08-04`（本工具多 1 请求）、`08-13`（本工具多 3 请求，token 完全一致）：去重计数口径差（cc-switch 另有 `session_usage_dedup`/proxy 侧 request_id 合并）。
   - 结论：口径对齐成立，差异均已归因，M1 关闭。后续里程碑不追求逐字节对齐，以本工具口径为准。
4. 只读不变量：`~/.claude/projects` 冻结副本运行前后 SHA-256 快照一致；Python 独立复算 3,741 事件、按日/按模型全部 23 组数字与工具输出逐一吻合（脚本输出 ALL-PASS）。
5. 缺失目录：stderr 警告 + 退出码 0 + 空结果。
6. 附带发现：本机真实数据全部经 cc-switch 路由到非 Anthropic 模型，费用列全部为 `0.00†`（unknown 可见）——不变量 5 按设计生效；cc-switch 对这些模型有 206 条 `model_pricing` 记录，后续价格表里程碑可直接参照其条目。
