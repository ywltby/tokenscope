# 原生复核 QA 记录：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-08-native-recheck-qa.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| RC10/RC11 既有原生批次 | 隔离入口和已完成场景有记录；失败尝试保留 | — |
| TTL / CSP / 当前显示器真实 150% | 已由后续 AP08 补齐，不再是“从未执行” | — |
| 125% / 跨显示器、系统深色首帧及当前版本批次 | 仍未验 | [N10](../active/2026-10-11-consolidated-remaining-work.md#n10)、[N11](../active/2026-10-11-consolidated-remaining-work.md#n11)、[N12](../active/2026-10-11-consolidated-remaining-work.md#n12) |

## 原审计更正

- 原“四项待验”未纳入较新 AP08：TTL、CSP、未强制的当前显示器 150% 已有直接证据。
- 应用消失/CDP 关闭只能证明验收中断，不能断言由其他实例终止；后续记录区分了未知终止者与 240s 驱动超时。
- 强制 device-scale-factor 150% 是工具轮，不能冒充系统设置 150% 的轮次。

## 核对证据与边界

- `frontend/scripts/native-acceptance.mjs`、`scripts/prepare-native-acceptance.ps1`。
- 较新 AP08 原生 measurements、两次真实空闲过期和无强制参数组合轮。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
