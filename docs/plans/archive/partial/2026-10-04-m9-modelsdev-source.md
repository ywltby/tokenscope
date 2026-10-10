# M9：models.dev 主源 + OpenRouter 备份（四层价格合并）

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 双源同步与快照实现 | 已完成；旧四层/内置表/丢同名候选不再有效 | — |
| 任务 4 / 验收 5 | 成功/部分失败后的数量时间与横幅待验，与 M5 合并 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11) |

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 状态：**已完成（2026-10-04），GUI 双源同步冒烟待用户确认**
- 创建：2026-10-04

## 目标

价格来源从三层扩为**四层**，优先级从高到低：

1. **外置** `pricing.toml`（本地覆盖，不变）；
2. **models.dev**（新主源，默认）：`~/.tokenscope/pricing-modelsdev.json` 快照；
3. **OpenRouter**（降为备份）：`~/.tokenscope/pricing-openrouter.json`（不变）；
4. **内置**：静态兜底（不变）。

同步入口合并为一个：GUI「同步价格」按钮与 CLI `tokenscope pricing sync` 一次同步**两个源**（各自独立快照、独立报错，一个失败不影响另一个）。

## 数据事实（2026-10-04 实测 models.dev/api.json）

- 226 个 provider、8,392 个模型；`cost` 四类字段（input/output/cache_read/cache_write）**单位已是 USD/百万 token**，无需换算（`claude-sonnet-4-5` = 3/15/0.3/3.75 与内置表一致）。
- **覆盖互补实测**：OpenRouter 缺的 doubao 全系在 `volcengine` provider 下（含日期后缀形态 `doubao-seed-2-0-pro-260215`）；glm-5-3-flash 亦在。
- 模型键 = `provider` + `/` + `model_id`（如 `volcengine/doubao-seed-2-0-pro-260215`），经 `normalize_model_id` 归一化后剥 vendor、点转横线，与日志模型串（`doubao-seed-2-0-pro-260215`）汇合。
- 部分模型无 `cost`（免费/非 LLM）→ 跳过不入快照，避免 0 价误报；有 cost 全零者保留（免费语义）。
- 跨 provider 同名归一化后可能同键（如多家转发同一模型）→ 快照按 id 排序去重，保留其一（价格通常一致；如有差异以排序靠前者为准，可被外置裁决）。

## 设计决策

- `Pricing::load(external, modelsdev_snapshot, openrouter_snapshot)`——层序即优先级；查找 `(u8::MAX - tier, prefix_len)` 取最大，变体隔离不变。
- 同步命令一次拉双源：`modelsdev::sync` + `openrouter::sync`，分别捕获错误汇总报告；单源失败不影响另一源与本地已有快照。
- 快照瘦身：只存 `{id, name, prompt, completion, cache_read, cache_write}`（name 取 models.dev 的模型显示名；OpenRouter 快照结构不变）。
- 设置页价格卡展示两个在线源的同步时间与条数；来源标签四值：外置 / models.dev / openrouter / 内置。
- **统计管线永不联网**（沿 M5）：summary/list_events 只读快照。

## 非目标

- 自动定时同步；双源价格冲突的自动仲裁 UI（可被外置覆盖，悬浮对照可见差异）
- 逐 provider 的快照拆分

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | `src/modelsdev.rs`：解析 + 快照 | `src/modelsdev.rs` | `test_modelsdev_parse_` / `test_modelsdev_snapshot_` | `cargo test modelsdev` |
| 2 | pricing 四层 | `src/pricing.rs` | `test_pricing_tier_`（四层裁决）/ `test_pricing_modelsdev_` | `cargo test pricing` |
| 3 | report/cli/tauri 双源同步 + 路径注入 | `src/report.rs`、`src/cli.rs`、`src-tauri/src/commands.rs` | `test_report_` 回归 + 同步冒烟 | `cargo run -- pricing sync` |
| 4 | GUI 同步双源 + 来源四值 | `frontend/src/views/Settings.vue` | vue-tsc | `pnpm build` |

## 验收

1. 全量门禁全绿（fmt / clippy / test / vue-tsc / pnpm build）。
2. 真实同步：双源各写快照（models.dev ~8k 条、OpenRouter ~466 条）；同步后真实日志 **doubao 系 4.7M unknown token 转正**（unknown 降至接近 0）。
3. 四层裁决 fixture：外置 > models.dev > openrouter > 内置；doubao 走 models.dev 层、grok 走 openrouter 层、无源模型走内置（单测固化）。
4. 单源失败降级：仅 models.dev 快照损坏 → openrouter 层与内置仍生效（单测固化）。
5. GUI 同步按钮冒烟留用户确认。

## 风险

- models.dev 无成本字段模型较多 → 跳过策略已在设计固化；快照体积（~1-2MB）可接受。
- 归一化同键跨 provider 价格不一致 → 排序取一 + 外置可裁决，文档注明。

## 验收记录（2026-10-04）

1. 全量门禁：fmt / clippy（0 警告）/ `cargo test --workspace`（71 测试：根 crate 64 + e2e 6 + tauri commands…）/ vue-tsc / pnpm build 全绿。
2. 真实双源同步：`tokenscope pricing sync` 一次同步 **models.dev 7,957 条 + OpenRouter 466 条**，各自独立快照与时间戳。
3. **unknown 完全清零**：同步后真实日志 unknown input 4,725,136 → 0（doubao 系经 models.dev 层转正；hy3:free 被 models.dev 免费条目精确命中，变体隔离保持）。合计费用 1,650.77 → 1,156.64（models.dev 对主力模型给出更低牌价，层级设计如此；价差悬浮可见，可被外置覆盖）。
4. 四层裁决 fixture 固化：外置 > models.dev > openrouter > 内置；单源快照损坏/缺失不影响其余层（modelsdev/openrouter 各有坏快照单测）。
5. `tauri build` NSIS 产物正常；GUI 双源状态展示与同步按钮冒烟留用户确认。

## 实现要点

- `modelsdev.rs` 与 openrouter.rs 同构：api.json 解析（BTreeMap provider→models，跳过无 cost 条目防 0 价误报；同归一化键去重保留排序靠前者）→ 瘦身快照。
- `Pricing::load` 签名扩为三快照（external/modelsdev/openrouter），tier 常量扩展为四层；查找仍 `(u8::MAX - tier, prefix_len)` 取最大。
- models.dev cost 单位与项目口径一致（USD/Mtok），无需换算——比 OpenRouter 层更直接。
- tauri sync 命令返回统一 `SyncOutcome` 数组（source/count/path/synced_at），前端不再区分源类型。
