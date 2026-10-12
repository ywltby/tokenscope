# M5：OpenRouter 价格同步（主源）+ 本地补充（本地优先）

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 同步实现与离线快照 | 已完成；旧单源主源/内置兜底由较新来源规则取代 | — |
| 任务 5 / 验收 5 | 当前同步成功后的 GUI 反馈待验，与 M9 合并 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11) |

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 状态：**已完成（2026-10-04），GUI 同步按钮冒烟待用户安装确认**
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

## 验收记录（2026-10-04）

1. 全量门禁：`cargo fmt --check`、`cargo clippy --workspace --all-targets`（0 警告）、`cargo test --workspace`（65 测试：根 crate 57 + e2e 6 + tauri 2）、`vue-tsc`、`pnpm build` 全绿。
2. 真实同步：`tokenscope pricing sync` 拉取 **466 条**写快照；`anthropic/claude-sonnet-4.5`（3/15/0.3/3.75）与 `x-ai/grok-4.5`（2/6/0.3/0）×1e6 后与官方牌价逐项一致。
3. 同步后真实日志全部模型入价：unknown input 从 17,381,581 降至 4,725,136（仅剩 `nvidia/nemotron-…:free` 等快照外模型）。
4. **变体隔离**（实现期发现的边界）：免费 `tencent/hy3:free` 曾经最长前缀误套付费基名 `tencent/hy3`（0.0825/Mtok）——修正为带 `:变体` 的查询只匹配同变体条目，基名价格不外溢，宁 unknown 不误价；免费变体本身若在快照中有条目则精确命中 0 价。
5. 层级优先级 fixture 固化：外置 > openrouter > 内置（同前缀层级裁决，跨层不比前缀长度）；层内最长前缀不变；`gpt-5.2-20260101` 经 openrouter（7.0）压过内置（1.75）。
6. 离线安全：快照缺失静默、损坏警告并忽略该层；e2e 助手统一钉住不存在的快照路径，测试不再依赖真实 `~/.tokenscope` 状态。
7. `tauri build` 产出 NSIS 4.27 MiB；GUI「同步 OpenRouter 价格」按钮冒烟留用户确认。

## 后续增量（2026-10-04，用户追加）

设置页价格表单价悬浮显示对照信息：来源 + 同前缀 OpenRouter 对照价（无对应模型标注"未知价格"）。后端在 `PricingEntry.openrouter` 挂同前缀 openrouter 层价格；前端 `NTooltip` 包四个单价列，并修复同前缀跨来源行的 rowKey 冲突。实测例：内置 `deepseek-v4-flash`（0.3）与 OpenRouter（0.028）差异悬浮可见——实际计价按层级走 OpenRouter，需要内置价时写入 `pricing.toml` 即可（本地优先）。

## 实现要点

- `normalize_model_id`：lowercase + 剥 `vendor/` + `.`→`-`，键与查询同函数，点/横线命名汇合（`claude-sonnet-4.5` ↔ `claude-sonnet-4-5-20250929`）。
- `Pricing` 从两层（内置+外置）变为三层合并表，`Entry.tier` 参与查找排序：`(u8::MAX - tier, prefix_len)` 取最大——注意 `Reverse(tier)` 方向会写反（实测踩过）。
- OpenRouter pricing 字段实测存在 `null` 与空串（免费/残缺条目），serde 全 `Option<String>` + 解析回退 0。
- 聚合层每事件一次查找（~500 条 × 2.7 万事件线性扫描）实测无感知延迟，暂不优化。
