# 用量历史库与 CCS 手动导入：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-10-usage-history-and-ccs-import.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

> 最新核对 `6fab12f`（双审计员挑刺循环收敛；`a58684c` 的 M01 闭合证据见[M01 修复复核与合并评估](../../reviews/2026-10-11-main-merge-readiness.md#m01-closed)）。下表为当前状态；后文整理时反例保留为历史，不代表这些缺陷仍存在。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

2026-10-12 [最新独立门禁复核](../../reviews/2026-10-11-main-merge-readiness.md#current-merge-readiness)：代码 231f667、后续仅文档 e31d1b6；普通/acceptance 根库各 462/10 忽略、壳 46/51、前端 364、Python 36、PS7 22 全部通过。N01–N09 保持完成，原生/真实性能仍归 N11/N13，H01–H08 不全部关闭；当前 PR 安装证据仍待验。后文门禁数量保留为原时点记录。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| H01 历史库/升级 | v4 旧污染纠错、备份与回滚通过 | [N01 已完成](../active/2026-10-11-consolidated-remaining-work.md#n01)、[N07 已完成](../active/2026-10-11-consolidated-remaining-work.md#n07) |
| H02 全部代理/身份 | 全部有效用量纳入；保守身份限制已披露；原五反例及不同来源种类同键两例对账通过 | [N02 已完成](../active/2026-10-11-consolidated-remaining-work.md#n02) |
| H03 遗留迁移 | 负数拒绝、整批回滚与旧污染恢复通过；新增 v3 缺列回归通过 | [N01 已完成](../active/2026-10-11-consolidated-remaining-work.md#n01)、[N03 已完成](../active/2026-10-11-consolidated-remaining-work.md#n03) |
| H04 采集与退出 | 共享提交观察、真实 300 文件回归、可控写锁等待与有限超时通过 | [N06 已完成](../active/2026-10-11-consolidated-remaining-work.md#n06) |
| H05 CCS 只读预览/暂存 | 读取期行数限制、失败后手动恢复及预览登记边界回归通过 | [N02 已完成](../active/2026-10-11-consolidated-remaining-work.md#n02)；[N04 已完成](../active/2026-10-11-consolidated-remaining-work.md#n04) |
| H06 并集/日汇总 | 双向样例、别名转移、时区幂等与旧污染纠错通过；原五反例及混合来源种类对账闭合，保守请求指纹边界保留 | [N02 已完成](../active/2026-10-11-consolidated-remaining-work.md#n02)；[N01 已完成](../active/2026-10-11-consolidated-remaining-work.md#n01)、[N07 已完成](../active/2026-10-11-consolidated-remaining-work.md#n07) |
| H07 冻结查询/预算 | 保留索引仍 O(n)；百万保留与 Rust 构建峰值实测通过，非完整 RSS/真实性能 | [N05 已完成](../active/2026-10-11-consolidated-remaining-work.md#n05)；[N13](../active/2026-10-11-consolidated-remaining-work.md#n13) |
| H08 设置与验收 | DTO、模式、失败恢复、价格隔离及普通/acceptance 门禁通过；history.db 路径、产物和七表校验通过；当前原生待验 | [N11](../active/2026-10-11-consolidated-remaining-work.md#n11)；[N04 已完成](../active/2026-10-11-consolidated-remaining-work.md#n04)、[N07 已完成](../active/2026-10-11-consolidated-remaining-work.md#n07)、[N09 已完成](../active/2026-10-11-consolidated-remaining-work.md#n09) |

## 原审计更正（整理时历史结果）

- 原“H01–H08 都完成、只剩原生/性能”不真实，存在可复现的旧库、迁移和容量边界缺口。
- 历史库无需按项目分库、不保存完整 JSONL；用量包含全部代理，不增加父子关系字段/界面。CCS 不点击不读取，只手动单次导入。
- `3e645f3` 的 7 项修复已提交，本轮维护中回归和独立探针通过：无歧义 Codex 别名回填、观察时间迁移、双向并集、原生优先、时区重复导入、长 ID 预算及 detail_only 恢复。见[复核记录](../../reviews/2026-10-11-history-seven-fixes-recheck.md)。
- 用原版本生成的合成旧库中，Codex 热扫/重扫仍 2 请求/240 token；Claude 回退事实热扫输出 6、重扫才 21；负数迁移 inserted=1/rejected=0。以上不是用户真实库的扫描结果。
- native-first 旧候选拒绝断言因新保守别名策略变化失败，不直接解释为仍重复入账；弱身份限制另需 N02 验证。
- 日汇总整桶手动替换入口尚未接 GUI，列条件待办：它不能恢复精确请求交集，不优先于用户要求的历史/并集正确性。

## 整理时核对证据与边界

- `src/history.rs`、`src/import/ccs.rs`、`src/query.rs`、`src/report.rs`、壳退出协调与 Settings/Dashboard。
- 本轮仓库外合成 v2/原版本库、双向并集/时区/预算及实际 serde Vue 探针；新计划 N01–N07 的数值反例。

静态核对全部 H 任务；独立探针使用临时合成库和临时来源，未读写用户真实用量数据库/源日志。`3e645f3` 后本轮最终门禁：root 442 pass/9 ignored、壳 41 pass、前端 359 pass，fmt/Clippy/typecheck/format:check/build 全部通过。之前第一版和中途门禁仅保留为历史。

整理基线为 `add8319`，7 项修复基线为 `3e645f3`；以上数字保留为当时结果。当前状态以上表、整合计划执行账及双审计循环收敛终态（`6fab12f`）为准，H01–H08 总体仍部分完成。`a58684c` 时点普通门禁根 456/壳 42/前端 361 全绿，来源身份与价格隔离反例已闭合，acceptance 根库 456 pass / 10 ignored、壳 47 pass，M01 旧产物断言已修；门禁终态为根库 461 + acceptance、壳库 45、前端 364、python 36、pwsh 契约 22。原生/性能验收仍待 N11/N13，不把旧反例重复登记。
