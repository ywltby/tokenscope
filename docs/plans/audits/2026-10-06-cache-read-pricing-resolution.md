# 缓存读取价格语义：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-06-cache-read-pricing-resolution.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| Task 1–6 | Unknown / Fixed / SameAsInput、显式 model_policy、完整候选优先已实现 | — |
| Task 7 | 历史自动化/后端只读核对有记录；实际 SameAsInput 与排除候选浮层待验 | [N11](../active/2026-10-11-consolidated-remaining-work.md#n11) |

## 原审计更正

- 原“7/7 全部通过”混合了后端记录与 GUI 验收。2384 请求/$136.26 是历史执行记录；本轮不重复读取真实源日志，也未独立复现其精确数值。
- 索引 v4/v5 是历史版本，已由后续失效规则替代；不为旧索引退版本或恢复严格本地覆盖。

## 核对证据与边界

- `src/pricing.rs` 的 `RateSpec`、policy 及完整候选池；`tests/pricing_read_recovery.rs`。
- `frontend/src/components/CostBreakdownTooltip.vue` 与历史执行账。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
