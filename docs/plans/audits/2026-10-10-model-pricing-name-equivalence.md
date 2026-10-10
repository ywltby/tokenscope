# 模型名称等价分组与计价：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-10-model-pricing-name-equivalence.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| MP01–MP03、MP05 | 等价键/边界、候选匹配、可信名称、派生索引规则已实现 | — |
| MP04 | 分组/下钻实现已完成；跨应用/原模型名/有价分页覆盖不足 | [N08](../active/2026-10-11-consolidated-remaining-work.md#n08) |
| MP06 | 文档与定向验收已有；当前原生目视/价格修订交互待验 | [N08](../active/2026-10-11-consolidated-remaining-work.md#n08)、[N11](../active/2026-10-11-consolidated-remaining-work.md#n11) |

## 原审计更正

- 上一轮友好下钻标签、按钮 aria 名称和磁盘索引空键已修复，不能重复列为未修缺陷。
- 被称为跨 Claude/Codex 的测试实际仅有 Claude；210 条样例未断言 row.model、费用求和或等价筛选游标。实现存在不代替这些验收。
- MP01 单测位于 model_identity.rs；原报告列的 pricing 行号多为 model_policy 测试。签名实际为 `rules:9`，不是 `rules:v9`。
- 前端快照和解析版本继续由后续规则推进；不把旧 MP 的 view 9 写成当前固定版本。

## 核对证据与边界

- `src/model_identity.rs`、pricing/aggregate/report；`tests/model_name_equivalence.rs`、`tests/model_identity_contract.rs`。
- Dashboard 的 resolvedDrillLabel、UsageTable 的友好 aria label、索引恢复的空键拒绝。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
