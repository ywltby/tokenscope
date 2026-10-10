# 用量历史库与 CCS 手动导入：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-10-usage-history-and-ccs-import.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| H01 历史库/升级 | 主体实现；v3 针对性修复已提交并复核通过，已污染旧事实未闭合 | [N01](../active/2026-10-11-consolidated-remaining-work.md#n01)；[N07 已完成](../active/2026-10-11-consolidated-remaining-work.md#n07) |
| H02 全部代理/身份 | 全部有效用量纳入；并集身份的弱指纹边界仍需验证 | [N02](../active/2026-10-11-consolidated-remaining-work.md#n02) |
| H03 遗留迁移 | 整批原子主体完成；负数被改零、旧污染恢复尚有缺口 | [N01](../active/2026-10-11-consolidated-remaining-work.md#n01)、[N03](../active/2026-10-11-consolidated-remaining-work.md#n03) |
| H04 采集与退出 | 统一串行采集已实现；退出等待只观察 timer 活动标志 | [N06](../active/2026-10-11-consolidated-remaining-work.md#n06) |
| H05 CCS 只读预览/暂存 | 手动事务与白名单已实现；预览非容量受限流式读取 | [N02](../active/2026-10-11-consolidated-remaining-work.md#n02)、[N04](../active/2026-10-11-consolidated-remaining-work.md#n04) |
| H06 并集/日汇总 | 双向样例/别名转移/时区幂等已提交并复核通过；旧事实与完整身份边界待闭合 | [N01](../active/2026-10-11-consolidated-remaining-work.md#n01)、[N02](../active/2026-10-11-consolidated-remaining-work.md#n02)；[N07 已完成](../active/2026-10-11-consolidated-remaining-work.md#n07) |
| H07 冻结查询/预算 | 冻结事务/批读取已有；保留索引仍 O(n)，百万规模待验 | [N05](../active/2026-10-11-consolidated-remaining-work.md#n05)、[N13](../active/2026-10-11-consolidated-remaining-work.md#n13) |
| H08 设置与验收 | 真实 DTO/明细模式及最终门禁通过；错误恢复、测试隔离/确定性和原生待闭合 | [N04](../active/2026-10-11-consolidated-remaining-work.md#n04)、[N09](../active/2026-10-11-consolidated-remaining-work.md#n09)、[N11](../active/2026-10-11-consolidated-remaining-work.md#n11)；[N07 已完成](../active/2026-10-11-consolidated-remaining-work.md#n07) |

## 原审计更正

- 原“H01–H08 都完成、只剩原生/性能”不真实，存在可复现的旧库、迁移和容量边界缺口。
- 历史库无需按项目分库、不保存完整 JSONL；用量包含全部代理，不增加父子关系字段/界面。CCS 不点击不读取，只手动单次导入。
- `3e645f3` 的 7 项修复已提交，本轮维护中回归和独立探针通过：无歧义 Codex 别名回填、观察时间迁移、双向并集、原生优先、时区重复导入、长 ID 预算及 detail_only 恢复。见[复核记录](../../reviews/2026-10-11-history-seven-fixes-recheck.md)。
- 用原版本生成的合成旧库中，Codex 热扫/重扫仍 2 请求/240 token；Claude 回退事实热扫输出 6、重扫才 21；负数迁移 inserted=1/rejected=0。以上不是用户真实库的扫描结果。
- native-first 旧候选拒绝断言因新保守别名策略变化失败，不直接解释为仍重复入账；弱身份限制另需 N02 验证。
- 日汇总整桶手动替换入口尚未接 GUI，列条件待办：它不能恢复精确请求交集，不优先于用户要求的历史/并集正确性。

## 核对证据与边界

- `src/history.rs`、`src/import/ccs.rs`、`src/query.rs`、`src/report.rs`、壳退出协调与 Settings/Dashboard。
- 本轮仓库外合成 v2/原版本库、双向并集/时区/预算及实际 serde Vue 探针；新计划 N01–N07 的数值反例。

静态核对全部 H 任务；独立探针使用临时合成库和临时来源，未读写用户真实用量数据库/源日志。`3e645f3` 后本轮最终门禁：root 442 pass/9 ignored、壳 41 pass、前端 359 pass，fmt/Clippy/typecheck/format:check/build 全部通过。之前第一版和中途门禁仅保留为历史。

整理基线为 `add8319`；7 项修复复核基线为 `3e645f3`。当前版本状态、最新门禁与仍存在的独立探针反例统一见新计划执行账；N07 已闭合，H01–H08 仍为部分完成。
