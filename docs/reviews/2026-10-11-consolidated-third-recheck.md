# 整合计划第三轮修复复核

> 以下为 `6688466` 的历史复核；T01/T02 已由 `16beea1` 修复并通过独立验证，`a58684c` 的[M01 已闭合](2026-10-11-main-merge-readiness.md#m01-closed)。当前 `a64d340` 状态见[安装设计实施独立复核](2026-10-11-main-merge-readiness.md#install-design-recheck)，保留本轮发生时的结果。

- 日期：2026-10-11。
- 被审实现：`6688466e75f3344868ba2432b45745e7fcdd6233`，`fix(历史库): 闭合第二轮复核 R01/R03/R04 三项缺口`。
- 依据：[唯一整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)、[第二轮复核](2026-10-11-consolidated-five-fixes-recheck.md)及用户要求。本轮仅审查和同步文档，不改产品代码。
- **结论：上轮具体反例已经修复，N08 完成；N02、N09 尚不能整体关闭。** 新来源冲突校验的批内键遗漏 `origin_kind`，仍与真实事务不一致；部分直接构造选项的普通单元测试绕过已修 helper，价格路径仍不密闭。两项均有隔离复现。
- 普通门禁全部通过：根库 **455 pass / 10 ignored**、壳 **42 pass**、前端 **361 pass**，双 manifest fmt/Clippy 及前端 typecheck/format:check/build 通过。普通门禁全绿与新增反例失败分别记账。
- 当前 13 个编号：**7 完成、2 部分完成、4 本轮验收未执行，共 6 项待办**。完成 N01/N03/N04/N05/N06/N07/N08；N02/N09 部分完成，N10–N13 维持原排期。

## 已确认修复

| 上轮问题 | 本轮核对结果 |
| --- | --- |
| R01：来源证据已绑定另一事实 | 原反例通过：相同 `(origin_kind, origin_key)` 改绑，预览和真实提交都拒绝；新增维护中回归存在并通过 |
| R01：未持久替代事件键被模拟登记 | 原反例通过：预览 `[Unchanged, Insert]`；提交 `inserted=1, unchanged=1, updated=0`、最终两事实，非 Insert 不再登记替代键 |
| R03：Settings 漏完整候选优先 | 1277 行文案实际修改，新增组件断言验证完整优先、无完整候选才回退部分候选，并排除旧说法；N08 闭合 |
| R04：公共 `opts` helper 价格缺省 | `report.rs::tests::opts` 将 None 转为临时 cache 下 `no-pricing.toml`，该 helper 的调用者已不回落用户外置价；剩余直接构造见下文 |
| R04：400 行净存活断言红灯 | 删除 `live_delta > 0` 及相关比较；当前普通根库全量通过。百万测试去除分配器实测量和预算估算量的大小断言，仍保留测量输出与预算断言，不重新宣称两口径等价 |

原第一轮的实际 CCS 重导入、同事实多别名及 Unchanged 补别名三例继续通过：实际 CCS 两侧均 `inserted=0, updated=1, unchanged=0, stale=1`，最终一事实；多别名两侧两条 Update、最终一事实；补别名两侧两条 Unchanged、最终一事实。本轮独立重跑全部五个存储反例通过，不能再把已修问题列为当前缺陷。

## 剩余发现

### T01 / P2 / N02：批内来源身份遗漏种类，合法写入误分类或误拒绝

位置：[history.rs](../../src/history.rs) 的 `BatchSim::resolve`（2402 行）和新冲突校验 `BatchSim::register`（2485 行）。实际 schema 145 行为 `UNIQUE(origin_kind, origin_key)`，真实 lookup 和提交也都用这个二元身份；模拟索引仍用 `format!("o|{}", origin_key)`，没有 `origin_kind`。

使用 `ORIGIN_KIND_NATIVE_FILE` 与 `ORIGIN_KIND_CCS_REQUEST`、相同来源键 `synthetic-shared-key`、不同规范事件键和 output=10/20，在临时库验证两种合法场景：

| 场景 | 预览实际结果 | 真实提交结果 | 应有预览 |
| --- | --- | --- | --- |
| 空库插入 A/native_file 与 B/ccs_request | `[Insert, Update]` | `inserted=2, updated=0`，最终两事实 | `[Insert, Insert]` |
| 已有 A/B 两事实，各补上述来源证据 | 来源冲突错误，称整批将失败 | `unchanged=2, origins_added=2`，最终两事实 | `[Unchanged, Unchanged]` |

第一例的 resolve 把不同来源种类的相同键当成一个事实；第二例在规范事件键已分别命中 A/B 后，新 register 冲突分支错误拒绝合法证据。数据库与真实事务可同时保存两种来源的同值键，模拟不能缩窄其身份域。

这是对上轮已明确要求的 `(origin_kind, origin_key)` 绑定规则继续核对，未新增身份产品需求。**两例限定为公共存储 API。当前 CCS 转换行统一使用 `ccs_request`，尚未证明 GUI 导入会构造混合来源种类的这两例，也未证明已有历史丢数。** 上轮相同种类的冲突修复保持有效；只需让模拟的查找与登记共用完整二元身份，并补预览/提交对账，保守请求指纹不变。

### T02 / P2 / N09：直接构造的单元测试仍读取默认外置价

位置：[report.rs](../../src/report.rs) 的 `test_list_events_filters`（2369 行，base 构造 2371 行），`test_report_tz_resolution`（2325 行）等。它们直接构造 `SummaryOptions`，没有使用已修 `opts` helper，仍省略 `pricing_path`；同文件还有以下路径：

- `test_list_events_dedupe_consistent_with_summary`（2448 行）。
- `test_events_pagination_parity`（2574 行）。
- `subsecond_fixture`（3139 行）与 `identical_ts_fixture`（3248 行），被分页测试实际使用。
- `test_disabled_source_skipped`（3365 行）。
- `collect_stability_tests` 的 `test_append_during_parse_not_cached_as_complete`（3457 行）与 `test_filter_switch_no_reparse`（3541 行），实际进入 `collect_all_with_sources`。

`pricing_file_path(None)` 仍与临时 cache/index 无关，解析默认 `~/.tokenscope/pricing.toml`，collect 945 行随后读取该文件。纯 `apply_time_filter` 构造的选项没有业务 I/O，不算隔离失败；已显式注入价格的 `test_report_modelsdev_layer` 等也不重复列入。

独立 probe 启用现有 acceptance feature，在临时目录 `bootstrap_with` 冻结数据根，确认默认价格路径在隔离根内；不改 HOME，不编辑用户真实价格。采用 `test_list_events_filters` 同形的选项和仓内 basic fixtures，省略 `pricing_path`，其他价格源显式指向临时缺失文件。在隔离默认外置价仅写合成 `gpt-5.6-sol` 的四桶单价各 1：查询实际 **8 行，其中 2 行有价**，与该普通测试末尾“无来源时全部未知”的 `priced=0` 断言矛盾。辅助验收因此失败，证明路径遗漏会影响结果，而不是仅静态猜测。

当前普通全量在本机通过不证明这些测试隔离。公共 helper 的修复是真实且有效的；剩余应核对所有实际调用采集/查询的直接构造和独立 helper，显式注入临时全部价格路径，不修改用户数据造反例。此项为测试污染/环境依赖，未证明产品费用计算错误。

## 验证与边界

| 检查 | 本轮结果 |
| --- | --- |
| 根库 fmt / Clippy（`-D warnings`）/ 普通全量测试 | 通过；455 pass / 10 ignored，包含 `review_regressions` 28 项 |
| Tauri 独立 manifest 的 fmt / Clippy / 普通全量测试 | 通过；42 pass |
| 前端 typecheck / format:check / test / build | 通过；33 files、361 tests，含新增文案断言 |
| 仓库外原五个批量存储反例 | 5 通过；上述已修问题与真实提交一致 |
| 日期范围 / 完整价格候选探针 | 2 通过；1,000 行记账 92,745 B、一天 500 行 47,273 B；完整候选 `0.00011` 胜过部分已知费用 `0.001` |
| 来源二元身份对账探针 | 2 失败，见 T01；与普通全量分开记账 |
| 隔离默认价格探针 | 1 失败，见 T02；8 行中 2 行有价，不符合普通测试的全部未知声明 |

根/壳均离线验证，普通套件采用默认并行。新增来源 probe 使用合成临时库；默认价格 probe 只通过现有 acceptance 根重定向默认路径，未碰用户真实用量或价格。普通根库套件仍有 T02 的默认价格读取，不能称全套密闭。没有运行真实日志忽略测试、CCS 自动同步、当前浏览器/原生完整矩阵、安装或发布。

N05/N06 维持第二轮已关闭状态：本次没有重新运行百万 release 或外部可控提交锁；相关产品路径未改变，百万改动仅移除不正确的跨口径大小断言，测量代码保留。普通历史查询和真实 300 文件提交回归本轮通过。前轮百万测量与退出等待结果保留其当时版本和限制，不冒充本轮新产物。N10–N13 未产生新增验收证据。

本机辅助产物：`C:/Users/admin/AppData/Local/Temp/tokenscope-consolidated-third-review-4qvkgla6/`。`final-gates/results.json` 与 `gate-counts.json` 保存全部普通门禁；`batch-probe/tests/registration_edges.rs` 和 `batch.rs` 是原五反例，`origin_pair.rs` 是二元身份反例，`default_prices.rs` 是隔离价格反例，`independent-results.txt` 保存关键结果。本报告保留复现输入，临时目录不是长期交付。

本轮更新整合计划、总账、三份关联审计和复核索引；完成与执行者声明分开，不另建竞争计划，不重开已完成项。产品代码、36 份归档文件及并发研究改动未修改。普通门禁虽通过，新增对账/隔离验证仍失败，按 `CLAUDE.md` 的验证失败例外，本批文档不提交推送，不绕过钩子。
