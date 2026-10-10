# 历史用量第二轮 7 项修复复核

- 日期：2026-10-11。
- 修复提交：`3e645f3`；本轮检查期间后续提交仅修改研究文档，产品代码与该修复一致。
- 范围：[统计口径 §6.5.1](../stats-semantics.md#651-复审第二轮修订2026-10-11隔离探针发现的遗漏) 的 7 项，以及对应源码、回归和实际采集/导入路径。
- **结论：7 项针对性修复均通过本轮复核。** 本轮没有发现这 7 项的新回归；历史库整体仍按[整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)跟踪其他缺口。

## 逐项结论

| 问题 | 结果 | 核对与实测证据 | 限制 |
| --- | --- | --- | --- |
| 1. v2 原生 Codex 缺重播别名，热扫后归档移动重复计量 | 通过 | schema v3 从已有事实回填 `codex-usage`；合成 v2 库升级后热扫已有 1 个别名，文件移动仍为 1 请求/120 token；复制、移动、遗留迁移重扫也保持 1/120 | 仅回填能唯一对应事实的身份；原库已经重复的事实不能由此自动安全合并，见 N01 |
| 2. 旧库采集时间压住原生真实终值 | 通过 | 迁移将用于新旧比较的观察秒/纳秒改为事件时间；合成 v2 Claude 库收到新终值后输出为 20，重扫仍为 20，不再被旧采集时刻拒绝 | 已经被旧逻辑回退的值不由迁移猜造，仍需另行恢复 |
| 3. CCS-first 与 native-first 的 Codex 并集不一致 | 通过 | 两个顺序均命中同一事实；维护中的回归覆盖 proxy/session 去重后转移别名，保留后续原生匹配入口 | `session|原始模型|四桶` 是保守重播指纹，不能证明同用量独立请求的强身份；缺 session 的重叠候选仍默认拒绝 |
| 4. CCS 时间较晚时覆盖更可信原生值 | 通过 | 来源优先级先于观察时间；两顺序实际导入/采集探针均在原生输出 30 后拒绝晚到的 CCS 50；即使第一份 CCS 的值与原生完全一致、日志没有 cwd，也保持 `NativeLog` 优先级 | 同优先级内仍按观察时间比较；本轮边界探针未发现优先级未提升的实际路径 |
| 5. 改选来源时区再次导入日汇总重复计数 | 通过 | v3 主键不含 `source_tz`；v2 仅时区不同的重复行迁移折叠；实际预览/导入先 UTC 后 Asia/Shanghai，第二次新增 0，最终 10 请求/1 行汇总 | 来源时区只是导入假设；日汇总仍不能还原逐请求时间、项目或精确交集 |
| 6. 长 `record_id` 绕过查询保留内存预算 | 通过 | 行预算加入字符串堆占用；维护中的 `row_budget_counts_record_id_heap` 通过；约 4,004,108 字节索引在 65,536 字节额度下明确拒绝 | 此预算约束保留数据，不能代替构建峰值/百万规模验收，见 N05 |
| 7. `detail_only` 不随视图保存/恢复 | 通过 | 快照 v11 保存并恢复 `rollups_mode`；前端回归覆盖保存、恢复和忽略 v10；独立 Vue 探针同时通过实际 Rust serde 的 Settings 载荷与模式恢复 | 原生桌面交互验收仍合并到 N11，组件测试不替代安装包验收 |

维护中的定向回归：

- [review_regressions.rs](../../tests/review_regressions.rs)：`codex_import_and_collect_yield_the_same_union`、`deduped_codex_session_row_donates_replay_identity_to_proxy_row`、`upgraded_v2_history_backfills_identity_time_and_rollup_key`、`changing_source_timezone_does_not_duplicate_rollups`、`native_precedence_beats_observed_time`。
- [query.rs](../../src/query.rs)：`row_budget_counts_record_id_heap`。
- [Dashboard.rollups.test.ts](../../frontend/src/views/Dashboard.rollups.test.ts) 与 [viewSnapshot.test.ts](../../frontend/src/lib/viewSnapshot.test.ts)：模式保存/恢复、旧版本失效与版本号一致。

## 本轮验证

| 检查 | 结果 |
| --- | --- |
| 根库 fmt / Clippy（`-D warnings`）/ 全量测试 | 全部通过；442 通过、9 忽略 |
| Tauri 独立 manifest 的 fmt / Clippy（`-D warnings`）/ 全量测试 | 全部通过；41 通过 |
| 前端 typecheck / format:check / test / build | 全部通过；33 个测试文件、359 个测试通过 |
| 仓库外原生优先级边界测试 | 1 个测试通过，包含两种导入/采集顺序 |
| 仓库外 Vue 探针 | 2 个文件、2 个测试通过 |
| 仓库外迁移/归档、时区重导入、长 ID、日期筛选、4096 行查询探针 | 上述修复及原已修查询场景通过；其他历史反例继续登记在下节 |

Cargo 全部使用 `--offline`；根/壳测试本轮使用 `--test-threads=1`，TEMP/TMP 指向新临时目录。独立探针只使用合成日志、合成来源库、临时历史库和临时价格路径；不读写用户真实用量数据库/来源日志，未运行忽略的真实数据测试。

可复查的本机日志目录：`C:/Users/admin/AppData/Local/Temp/tokenscope-seven-recheck-ul38kmgb/`。`final-gates/results.json` 记录 10 条门禁命令及退出码，`storage-current.txt`、`priority-edge.txt`、`gui-tz-reimport.txt`、`gui-row-memory.txt`、`vue-independent.txt` 记录独立探针。临时目录不属于长期仓库产物；本报告保留关键结果与维护中的回归入口。

## 仍未闭合的既有事项

本轮复核的是上述 7 项修复，不是将 H01–H08 全部验收关闭。以下旧反例再次复现，均来自旧版本生成的合成库：

- **N01：已污染旧事实。** Codex 既有重复仍为 2 请求/240 token，热扫和重扫都不自动去重；Claude 已回退事实热扫输出仍为 6，重扫后恢复为 21。来源消失时不能据此承诺能恢复真实终值。
- **N03：遗留非法值迁移。** 负输入仍被改为 0，结果为 `inserted=1/rejected=0`，不是受检拒绝。
- **N02/N04/N05/N06 等其他责任**继续按整合计划处理；全量测试通过不能替代独立身份、容量、规模、退出等待及原生验收。

整合计划 N07 的自动化修复、维护中回归、口径说明与门禁本轮闭合；原生部分由 N11 承接。此前整理时的 Clippy、前端类型和快照版本门禁失败已修复，本轮结果取代其当前状态，旧失败记录保留为历史。
