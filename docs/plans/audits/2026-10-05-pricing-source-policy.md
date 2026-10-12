# 定价来源策略：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-05-pricing-source-policy.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| Task 1–3 | 已实现：无生产内置表、三来源候选、离线快照与状态 | — |
| Task 4 | 主体已实现；来源说明仍与较新计价规则不一致 | [N08](../active/2026-10-11-consolidated-remaining-work.md#n08) |
| Task 5 | 自动化已有；首次横幅→同步→离线的当前原生链待验，测试路径未完全隔离 | [N09](../active/2026-10-11-consolidated-remaining-work.md#n09)、[N11](../active/2026-10-11-consolidated-remaining-work.md#n11) |

## 原审计更正

- 原“5 项完成、只剩时序”过宽：Settings 仍把来源写成严格覆盖，unknown-only 测试也未注入外置价格路径。
- 较新分段/候选规则优先：完整候选优先，再按请求条件取最高费用；来源等级用于同价决胜，不能恢复无条件本地覆盖。

## 核对证据与边界

- `src/pricing.rs::estimate` 的完整池/最高费用选择、`PriceTable::default`；`tests/pricing_source_policy.rs`。
- `frontend/src/views/Settings.vue` 的来源说明、`tests/pricing_source_policy.rs` unknown-only 的 `SummaryOptions`。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
