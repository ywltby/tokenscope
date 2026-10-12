# 模型名称等价分组与计价：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-10-model-pricing-name-equivalence.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

> 最新核对 `6fab12f`（双审计员挑刺循环收敛；`a58684c` 的 M01 闭合证据见[M01 修复复核](../../reviews/2026-10-11-main-merge-readiness.md#m01-closed)）。下表为当前状态；N08 的跨应用/分页覆盖及计价文案均已完成，剩余原生责任归 N11。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| MP01–MP03、MP05 | 等价键/边界、候选匹配、可信名称、派生索引规则已实现 | — |
| MP04 | 分组/下钻已实现；跨 Claude/Codex、210 条原始模型名与有价费用求和回归已补并通过 | —；证据见 [N08](../active/2026-10-11-consolidated-remaining-work.md#n08) |
| MP06 | Settings 第一处已实际补完整候选优先及回退规则，新增组件断言通过；当前原生目视/价格修订交互待验 | [N11](../active/2026-10-11-consolidated-remaining-work.md#n11)；[N08 已完成](../active/2026-10-11-consolidated-remaining-work.md#n08) |

## 原审计更正（整理时历史结果）

- 上一轮友好下钻标签、按钮 aria 名称和磁盘索引空键已修复，不能重复列为未修缺陷。
- 被称为跨 Claude/Codex 的测试实际仅有 Claude；210 条样例未断言 row.model、费用求和或等价筛选游标。实现存在不代替这些验收。
- MP01 单测位于 model_identity.rs；原报告列的 pricing 行号多为 model_policy 测试。签名实际为 `rules:9`，不是 `rules:v9`。
- 前端快照和解析版本继续由后续规则推进；不把旧 MP 的 view 9 写成当前固定版本。

## 整理时核对证据与边界

- `src/model_identity.rs`、pricing/aggregate/report；`tests/model_name_equivalence.rs`、`tests/model_identity_contract.rs`。
- Dashboard 的 resolvedDrillLabel、UsageTable 的友好 aria label、索引恢复的空键拒绝。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

上述核对基线为 `add8319` 与整理时工作区。`a58684c` 与当前 `6fab12f` 状态以上表与最新复核为准；完整候选优先探针沿用第四轮证据，文案断言在当前前端全量中通过，不再列说明缺口，仅保留当前原生验收。N09 旧数据库产物断言已修，普通与 acceptance 门禁全绿；门禁终态（双审计循环收敛，`6fab12f`）为根库 461 + acceptance、壳库 45、前端 364、python 36、pwsh 契约 22。
