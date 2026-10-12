# 整合计划 R01–R05 修复复核

> 以下为 `2252eb3` 的历史复核；后续见[6688466 第三轮](2026-10-11-consolidated-third-recheck.md)。`a58684c` 的[M01 已闭合](2026-10-11-main-merge-readiness.md#m01-closed)，当前状态（A01–A06 已由 `b855021` 修复、循环收敛至 `6fab12f`）见[安装设计实施独立复核](2026-10-11-main-merge-readiness.md#install-design-recheck)。本报告保留当时结果。

- 日期：2026-10-11。
- 被审实现：`2252eb323f3e35e0af72f3ac2d50bc9ea1b4dbbf`，`fix(历史库): 闭合整合计划复核 R01–R05 五项缺口`。
- 依据：[唯一整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)、[上一轮复核](2026-10-11-consolidated-plan-recheck.md)及用户明确要求。本轮只复核和更新文档，不修改产品代码。
- **结论：修复未全部闭合。** R02/N05、R05/N06 可关闭；R01 的原三例已修复，但证据登记仍有两处存储 API 差异；R03 的 Settings 文案没有实际修改；R04 已补集成测试路径，遗漏根库单元测试 helper，普通根库门禁另出现一次峰值测试失败。
- 当前 13 项责任为 **6 项完成、3 项部分完成、4 项本轮验收未执行，共 7 项待办**。完成 N01/N03/N04/N05/N06/N07；部分完成 N02/N08/N09；N10–N13 未执行，保持原有排期。

## 修复核对

| 上轮问题 | 本轮结论 | 证据边界 |
| --- | --- | --- |
| R01 / N02 批量模拟 | 原三例通过，仍部分完成 | 同一事实统一终值、多别名和 Unchanged 补别名已修复并入维护中回归；新增来源冲突与未持久事件键反例仍失败，见下文 |
| R02 / N05 规模与说明 | 完成 | 百万 release 实测保留量、构建/汇总窗口峰值；日期缩窄降低行索引保留量，§3.5 已修正相反说法；不宣称完整 RSS 或百万全部分页已验 |
| R03 / N08 完整候选文案 | 未修复 | `Settings.vue` 1277 行仍写“全部候选……取总费用最高者”，提交和工作区均没有该文件改动 |
| R04 / N09 价格隔离 | 部分完成 | 原 `partial_pricing` 等集成用例已显式注入价格路径；`report.rs::tests::opts` 仍传 `None`，会读取默认外置价；当前普通根库门禁非全绿 |
| R05 / N06 提交与退出验收 | 完成 | 维护中根库/壳侧真实 300 文件提交测试通过；本轮另以临时 SQLite 写锁验证壳侧实际等待、释放后落库及有限超时 |

## 未闭合问题

### R01 / P2：证据登记仍与真实事务不一致

位置：[history.rs](../../src/history.rs) 的 `BatchSim::register`（2466 行），真实来源冲突检查在 1948 行。新的事实模型修复了上轮三例，但模拟登记无条件绑定来源记录和本条 `event_key`，与实际持久化规则有两处差异。

1. **已绑定另一事实的来源记录没有在预览中拒绝。** 临时库已有事实 A（`fact-a`、别名 a、output=10）与事实 B（`fact-b`、别名 b、output=20），B 持有来源记录 `ccs-request / bound-to-b`。对 A 写入相同内容并携带 B 的来源记录，预览为 `Ok([Unchanged])`，提交报“来源记录冲突……已指向事件 2，不能改指事件 1”。实际事务会回滚；预览不能把这种写入描述为可提交。别名已有冲突校验，来源记录缺少对应校验。
2. **模拟把未持久化的替代事件键当成后续身份。** 临时库一事实（规范事件键 `canonical`、别名 a、output=10）。依次写入 `other-source/output=10/别名 a` 和 `other-source/output=20/无别名`，两条均无 origins。预览为 `[Unchanged, Update]`；提交实际 `inserted=1, unchanged=1, updated=0`，最终两事实。真实 Unchanged/Update 不修改事实的规范事件键，也不会凭空保存替代键；预览却登记了 `k|other-source`，使第二条错误命中旧事实。

两例均走公共存储 API。**尚未证明现有 CCS 转换行会构造这些组合，也未证明当前 GUI 导入重复入账或丢失 token。** 其中无 origins 的输入与当前 CCS 行形态不同。问题限定为 `classify_batch` 声明的预览/提交一致性，不能扩大为所有 CCS 导入故障。

上轮三例本轮已通过：实际 CCS 重导入两侧均为 `inserted=0, updated=1, unchanged=0, stale=1`；同事实多别名预览 `[Update, Update]`、提交更新两次且最终一事实；未变化行补别名两侧均两条 Unchanged、最终一事实。这些无需重新实施。

剩余修复应复用真实来源记录的 `(origin_kind, origin_key)` 绑定检查，并只模拟真实事务实际持久化的身份；补上述预览/提交对账，不改变已确认的保守指纹规则。

### R03 / P2：提交说明和计划声称修过的文案仍在原处

位置：[Settings.vue](../../frontend/src/views/Settings.vue) 1277 行。仍写“同一模型的全部候选按本次请求条件计价后取总费用最高者”，遗漏完整候选优先。`b3e5d16..2252eb3` 没有该文件差异，工作区也没有该文件改动；仅修改计划完成声明不能闭合此项。

独立价格探针仍通过：不完整候选 input=10、缺 output，完整候选 input=1/output=1，请求 input=100/output=10；实际选择完整候选费用 `0.00011`，高于它的部分已知费用 `0.001` 不参与这轮选择，`reason=highest_complete_cost`。计价行为正确，剩余修复是说明及对应文案断言。

### R04 / P2：普通单元测试价格路径仍未隔离，根库门禁出现红灯

位置：[report.rs](../../src/report.rs) 的测试 helper `opts`（2023 行），`pricing_path: pricing`（2038 行）、`test_report_pipeline_both_agents`（2072 行）、`pricing_file_path`（178 行）及真实价格加载（945 行）。

helper 在调用者传 `None` 时直接保留 `None`；`test_report_pipeline_both_agents`、agent filter、missing-dir 和 cache-consistency 等普通单元测试实际调用该 helper。`pricing_file_path(None)` 解析默认 `~/.tokenscope/pricing.toml`，与临时 cache/index 无关。因此本机外置价格仍能改变这些测试的价格、警告和来源状态。提交已经补了所列集成测试，但不能据此称“全部普通测试”隔离。本轮没有编辑用户价格来制造反例；直接调用 `Pricing::load(None, ...)` 表示来源未配置，与报告管线的默认路径解析不同，不作为此缺口。

普通 `cargo test --offline --workspace --quiet` 退出码 **101**，在 [query_memory_budget.rs](../../tests/query_memory_budget.rs) 的 `build_peak_is_measured_separately_from_retained_charge`（570 行）失败：保留记账 **34,635 B**、构建峰值增量 **655,745 B**、窗口结束净存活增量 **0 B**，`assert!(live_delta > 0)` 未成立。该二进制失败后 Cargo 中止后续测试，所以已输出的 **387 pass / 1 failed / 8 ignored** 不是全根库总数，也不等于本次全量通过。

该失败随后没有在目标单测、整个目标串行/并行复跑以及 20 次默认并行目标循环中再次出现，不能声称持续失败。独立确定性探针证明该断言的误判条件：先把 1,000 条合成价格留在全局缓存，再创建无价、400 行的新查询，旧价格缓存释放使全进程存活量 **959,504 B → 37,740 B**（净变化 **−921,764 B**，`saturating_sub` 为 0），但新查询实际有 **400 行、保留记账 36,029 B**。全进程净增量不是新查询保留字节，断言可能受窗口前缓存及测试运行时释放影响。此探针证明测量方法问题，**不追认首轮失败时具体释放的是哪一对象，也不证明产品查询丢数据**。

N09 应补单元 helper 的显式临时价格路径，并使峰值测试不依赖无关全局释放；保留首轮红灯证据，不能用后续局部绿灯覆盖成“本轮全部门禁通过”。

## 已闭合验收与测量

**N05 百万合成 release：** 1,000,000 行、保留记账 **88,891,726 B（84.8 MiB）**；构建窗口 Rust 全局分配器峰值 **123,231,716 B（117.5 MiB）**，窗口存活量 **5,806 B → 72,901,225 B**；汇总窗口峰值 **84,272,474 B（80.4 MiB）**；入库 **399,028 ms**、建索引 **43,796 ms**、汇总 **5,637 ms**。退出码 0，总耗时 498.7 s（含 release 编译）。

该分配器只统计经 Rust 全局分配器分配的块，不覆盖 SQLite C 分配、完整 RSS 或系统分配器开销。保留预算与构建峰值仍是不同口径。现有百万测试检查汇总和前两页共 2,000 条；210 条专项覆盖全部分页，4 万条测试覆盖汇总，不能称 4 万/百万明细全部逐页核对。本轮没有新增全量遍历百万分页的要求。同库日期探针仍通过：1,000 行记账 **93,350 B**，一天 500 行 **47,878 B**。

**N06 真实提交和有限等待：** 仓内 `tests/history_collection.rs::real_submission_is_covered_by_shared_observation` 与壳侧 `stop_periodic_collection_waits_for_real_submission` 通过，真实 300 文件采集计数可见，结束后重新打开历史库得到 300 条事实。仓内测试靠足够多文件观察并发，不是可控数据库提交屏障。

本轮在仓库外复制壳源码，仅追加测试及临时构建配置，调用原样的 `stop_periodic_collection`，所有数据/价格均为临时合成文件。用 `BEGIN IMMEDIATE` 持有临时 history.db 写锁：真实 summary 提交已登记，等待 150ms 时退出等待未返回、数据库仍零事实；释放锁后返回，计数归零、完整一事实落库。第二场保持锁、等待上限 60ms，约 **62ms** 返回且活动提交仍可见、数据库仍零事实；释放后提交完整落库。由此补齐可控等待、释放和有限超时证据，关闭 N06。此为辅助探针，不声称维护中套件已有这两个可控锁场景，也不是强制结束进程验收。

新增 v3 缺 root/context/cwd 列的旧缓存迁移回归通过；目标 `history_migration` **8 项通过**。旧污染纠错、负数拒绝及其他已完成任务维持状态，不因新预览或测试方法问题重新实施。

## 验证汇总与交付

| 检查 | 本轮实际结果 |
| --- | --- |
| 根库 fmt / Clippy（`-D warnings`） | 通过 |
| 根库普通全量测试 | 失败；中止前 387 pass / 1 failed / 8 ignored，失败项见 R04 |
| Tauri 独立 manifest 的 fmt / Clippy / 全量测试 | 通过；42 tests |
| 前端 typecheck / format:check / test / build | 通过；33 files、360 tests |
| `review_regressions` / `history_migration` 定向测试 | 26 / 8 通过；含本次新增回归 |
| 上轮独立批量 / 日期与候选探针 | 3 / 2 通过 |
| 新增来源登记反例 | 2 失败；限定为公共存储 API |
| 百万 release / 壳侧可控提交探针 | 各 1 通过 |
| 400 行峰值失败跟进 | 单测、目标串行/并行及 20 次并行目标复跑通过；全局缓存释放探针复现净增量误判条件 |

根/壳 Cargo 使用 `--offline`，普通测试默认并行；新增探针全部使用临时来源、历史和价格路径。普通单元套件仍有 R04 所列默认价格读取，不能笼统称所有测试已密闭。没有运行真实日志忽略测试、CCS 自动同步、当前原生完整矩阵、安装或发布；N10–N13 未产生新增验收产物。

百万命令：`cargo test --offline --release --test history_queries million_row_history_summarizes_and_pages_within_budget -- --ignored --nocapture --test-threads=1`。只显式运行此合成测试。

本机辅助产物：`C:/Users/admin/AppData/Local/Temp/tokenscope-consolidated-rerun-review-7shtur63/`，含 `final-gates/results.json`、`gate-counts.json`、`review-regressions.txt`、`legacy-migration.txt`、`baseline-probes.txt`、`registration-edges.txt`、`allocator-window.txt`、`million.txt`、`controlled-exit.txt` 及相关测试源码；临时产物不是长期交付，本报告保留复现输入和关键结果。

本轮更新复核报告、整合计划、总账和相关审计入口，原执行者“全部完成”声明保留为历史并以本轮核验状态纠正。产品代码和并发研究文件未修改。因根库门禁失败且测试方法缺口未解决，按 `CLAUDE.md` 的提交规则不提交、不推送，未绕过钩子。后续先完成 N02，再闭合 N08/N09；已完成项不重复实施。
