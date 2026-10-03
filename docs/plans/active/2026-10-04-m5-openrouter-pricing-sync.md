# M5：OpenRouter 价格同步（主源）+ 本地补充（本地优先）

- 状态：**已确认（用户 2026-10-04 拍板：主用 openrouter.ai/api/v1/models，配合本地价格补充，本地价格优先）**
- 创建：2026-10-04

## 目标

模型价格改为三层来源，自动获取为主、本地兜底：

1. **OpenRouter 同步快照**（主源）：`~/.tokenscope/pricing-openrouter.json`，由 `https://openrouter.ai/api/v1/models` 同步生成（含模型显示名）；
2. **本地外置** `pricing.toml`（补充/覆盖）：同前缀**永远压过** OpenRouter 与内置；
3. **内置表**：静态快照，作 OpenRouter 未收录模型（如实测中的 doubao）的兜底。

同步入口：GUI 设置页「同步 OpenRouter 价格」按钮 + CLI `tokenscope pricing sync`。

## 数据事实（2026-10-04 实测 openrouter.ai/api/v1/models）

- 公开免鉴权，466 个模型；`pricing` 四类字段齐全：`prompt` / `completion` / `input_cache_read` / `input_cache_write`，单位为**每 token USD 字符串**，×1e6 即本项目口径。
- 抽样与内置表逐项一致（官方牌价）：`anthropic/claude-sonnet-4.5` = 3/15/0.3/3.75、`x-ai/grok-4.5` = 2/6/0.3/0、`openai/gpt-5.2` = 1.75/14/0.175。
- 覆盖：内置 20 个前缀命中 19（doubao 无）；另有 `name` 显示名字段（如 "Anthropic: Claude Sonnet 4.5"）。
- 变体后缀：`:free`（0 价，独立条目）、`:batch`、`:thinking` 等；部分模型有 `overrides` 阶梯价（>200k 涨价）与 `input_cache_write_1h`。

## 设计决策（先于代码）

- **优先级：层级优先，层内最长前缀**。同一模型前缀命中多层时按 `外置 > openrouter > 内置` 取值；层内仍按最长前缀（跨层不比长度——"本地价格优先"按用户语义落地）。归一化后前缀可能跨层重叠（如外置 `claude-sonnet-4` 与 openrouter `claude-sonnet-4-5` 同时命中 `claude-sonnet-4-5-20250929`，取外置短前缀）。
- **命名归一化**（键与查询同一函数 `normalize_model_id`）：lowercase；剥 `vendor/` 前缀；`.` → `-`（claude-sonnet-4.5 ↔ claude-sonnet-4-5 汇合）。变体后缀（`:free` 等）**保留**参与匹配——最长前缀天然让 `:free` 变体优先于基名命中，`tencent/hy3:free` 按同 id 精确对上 0 价。
- **同步是显式动作，统计永不联网**：`summary()` 只读快照文件；同步失败/无快照 → 沿用内置表并告警（GUI 按钮报错，CLI 退出码 1）。快照含 `synced_at` 与条目数，损坏 → 警告 + 忽略。
- **快照内容瘦身**：只存 `{id, name, prompt, completion, cache_read, cache_write}`（约 466 条，~200KB），丢弃描述/架构等。
- **HTTP**：`ureq`（rustls，无系统依赖，阻塞式——同步是显式动作可阻塞）。统计管线零新增网络依赖。
- **`overrides` 阶梯价与 1h 缓存档**：v1 忽略，取默认档；文档与 GUI 说明注明。

## 非目标

- 自动定时同步、启动时联网检查
- 阶梯价（overrides）/ 1h 缓存档
- models.dev 作为第二在线源

## CLI / GUI 面

- CLI：`tokenscope pricing sync`（同步并写快照，打印条目数与路径）。
- GUI 设置页价格卡新增：「同步 OpenRouter 价格」按钮（成功显示条目数与时间）、表格加「显示名」列、来源标签三值（内置 / openrouter / 外置）、快照时间展示。

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | pricing 三层合并 + normalize + name 字段 | `src/pricing.rs` | `test_pricing_normalize_` / `test_pricing_tier_`（外置>openrouter>内置、层内最长前缀、变体命中） | `cargo test pricing` |
| 2 | openrouter 同步与快照 | `src/openrouter.rs` | `test_openrouter_parse_`（fixture JSON→条目）/ `test_openrouter_snapshot_`（读写往返、坏快照忽略） | `cargo test openrouter` |
| 3 | report 管线接入三层 + SummaryOptions 扩展 | `src/report.rs` | `test_report_pricing_tiers_` | `cargo test report` |
| 4 | CLI `pricing sync` | `src/cli.rs` | `test_pricing_cmd_` | `cargo run -- pricing sync` |
| 5 | GUI 同步按钮 + 名字列 + 来源标签 | `src-tauri/src/commands.rs`、`frontend/src/views/Settings.vue` | `test_tauri_pricing_` 回归 | `vue-tsc` + `cargo test -p tokenscope-tauri` |
| 6 | 真实同步验收 + 文档 | — | — | `pricing sync` 实跑 + spot-check 费用 |

## 验收

1. 全量门禁：fmt / clippy / test --workspace / vue-tsc / pnpm build 全绿。
2. 真实同步：CLI `pricing sync` 拉取 466 条写快照；同步后对真实日志跑 `summary`，抽查费用：`claude-sonnet-4-5-20250929` 与内置值一致、`tencent/hy3:free` 从 unknown 变为 $0、`doubao-seed-2-0-pro-260215` 仍走内置兜底（openrouter 无收录）。
3. 优先级 fixture：外置同前缀覆盖 openrouter；openrouter 覆盖内置；层内最长前缀不变（单测固化）。
4. 离线安全：断网/坏快照时 summary 正常出数（单测 + 降级警告）。
5. GUI 冒烟留用户确认。

## 风险

- OpenRouter 字段演进（pricing 新键/类型变化）→ serde 宽松解析 + 同步失败显式报错，不影响统计。
- 归一化把不同官方命名汇合（点/横线）→ 以"同模型不同写法"为设计意图；如遇真实冲突，以外置表裁决。
- ureq + rustls 首次编译增量时间 → 一次性成本。
