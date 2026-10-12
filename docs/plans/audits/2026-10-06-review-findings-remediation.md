# 审查发现问题修复：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-06-review-findings-remediation.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

> 最新核对 `6fab12f`（双审计员挑刺循环收敛；`a58684c` 的 M01 闭合证据见[M01 修复复核](../../reviews/2026-10-11-main-merge-readiness.md#m01-closed)）。Task 6 的握手和循环、后续价格路径隔离及普通/acceptance 门禁均已完成，N09 闭合；Task 7 的系统与发布责任仍待 N12 的 PR CI 实跑取证。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

2026-10-12 [最新独立门禁复核](../../reviews/2026-10-11-main-merge-readiness.md#current-merge-readiness)：代码 231f667、后续仅文档 e31d1b6；普通/acceptance 根库各 462/10 忽略、壳 46/51、前端 364、Python 36、PS7 22 全部通过。N09 保持完成；当前仍缺 PR CI 与安装实跑，Task 7 的系统/发布责任不关闭。后文早期门禁数量保留为原时点记录。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| Task 1–5 | 主体及后续修复已实现 | — |
| Task 6 | 已完成：真实等待者握手替代 150ms 时间窗，100 次循环证据保留 | —；相关 [N09 已完成](../active/2026-10-11-consolidated-remaining-work.md#n09) |
| Task 7 | 真机/发布验收后延 | [N12](../active/2026-10-11-consolidated-remaining-work.md#n12) |

## 原审计更正（整理时历史结果）

- 原“Task 1–6 已实现”不成立于 Task 6 的确定性目标：测试没有 follower 已等待的握手。
- 生产单飞 RAII 与 panic 释放实现存在；此处是测试确定性缺口，不称为已复现生产故障。
- 原要求的 100 次验证不能用历史 3 次运行或加长 sleep 代替。

## 整理时核对证据与边界

- `src/report.rs` 的 singleflight panic/follower 测试及 `sleep(150ms)` 释放逻辑。
- 游标、目录重叠、同步状态、组件测试的最终实现。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

上述核对基线为 `add8319` 与整理时工作区。当前握手、峰值净存活断言、公共 helper 与直接选项价格隔离已修。`a58684c` 更新过期产物断言，实际 history.db 路径、落盘及七表校验通过；普通/acceptance 门禁全绿，N09 闭合。门禁终态（双审计循环收敛，`6fab12f`）：根库 461 + acceptance、壳库 45、前端 364、python 36、pwsh 契约 22。Task 7 的发布/系统排期维持；N12 的 A01–A06 安装门禁缺口已由 `b855021` 修复，只剩 PR CI 实跑取证。
