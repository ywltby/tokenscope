# 全部计划审核问题修复 Implementation Plan

> **2026-10-11 归档复核：已实现。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| R01–R10 | 最终实现已完成；采用后续 F/RC/AP 补齐后的状态 | 无独立待办 |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-07-all-plans-audit-remediation.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** 修复 2026-10-07 全计划审核确认的计费、日期、缓存和交互问题，补上能抓住真实失效场景的回归测试。

**Architecture:** 沿用现有 source → report → pricing/aggregate → DTO → Vue 分层；用统一的价格合法性/选择规则、包含来源上下文的缓存身份、明确的前端查询身份修复根因。保留现有三态单价及完整候选优先策略，不重写定价系统，不改四桶 token 口径。

**Tech Stack:** Rust 2024、SQLite/rusqlite、TOML/serde、Tauri 2、Vue 3、Naive UI、Vitest/VTU。

**状态：已执行（2026-10-07）。** 审核基线 `a42e843`。证据及逐份计划覆盖关系见 [审核意见](../../../reviews/2026-10-07-all-plans-audit.md)。

## 现行约束、范围与不变量

1. 冲突以最新用户决策、缓存读取定价解析计划、当前 DESIGN.md 为准。旧 CLI、编译期内置价格、把缺缓存价自动补为 0 等要求不恢复。
2. Unknown、Fixed(0)、SameAsInput 三者不同；SameAsInput 只对明确声明的缓存读生效，引用当前时间/上下文档最终输入价。缺价本身不能证明“按输入价收费”。
3. 先确定请求历史时刻适用的时间档，再在适用规则中完整优先、费用取最高；跨渠道同样完整优先。完整性只看 token 数大于 0 的分项。
4. 四项单价来自同一候选及其继承链；不拼接渠道价格，不用不适用峰值/长上下文档。候选数按模型条目计，时间规则数量不能冒充渠道数量。
5. 合法单价必须有限且非负。非法数值不得参与费用计算，不得序列化为 null 后伪装免费。
6. 换来源类型/根目录后，缓存命中必须与重新解析一致。缓存是优化，不能把旧解析事件改标为另一 agent。
7. 日期控件对外只提交 YYYY-MM-DD；本机日历用于控件桥接，统计时区用于快捷项和后端解释，二者不可混用。
8. 视图快照只能包含同一刷新批次下相容的查询结果；旧恢复、旧响应、旧保存不能覆盖新状态。这里的批次是前端查询身份，不声称两次 IPC 必然共享后端同一采集瞬间。
9. empty/partial/error/stale 状态都保留必要诊断；发生失败不能继续标注不存在的“后台刷新中”。
10. 每个新增 `SummaryOptions` 测试显式注入临时 `cache_dir`、`pricing_index`，同时隔离价格文件/快照路径；禁止修改真实 agent 日志和真实缓存。无需跑 ignored 真实性能测试。

非目标：增加 agent、重构全部 UI、换存储引擎、自动安装/发布、重新定义模型匹配/四桶统计、强制完成此前已后延的 D5 安装验收。本轮不据缺价自动生成 model_policy。

## 执行顺序

Task 1 → 2 → 6 共用 pricing.rs，串行执行；建议整体顺序为 **1、2、6、5、3、4、7、8、9、10**。每个任务先写回归并确认实际执行且失败，再做最小修复，验证后独立提交。过滤命令返回 0 tests 不算验证。不要顺便实现未确认的新功能。

### Task 1 · P1 · 保留 OpenRouter 的数值缓存价格（R01）

**文件：** `src/pricing.rs`、`src/openrouter.rs`；`src/report.rs` 的费用明细测试；`docs/stats-semantics.md`。

1. 在实际快照 → Pricing::load/load_cached → estimate 链路写失败测试 `test_openrouter_base_cache_rates_preserved`：v2 快照 prompt=0.000002、completion=0.000003、cache_read=0.0000005、cache_write=0，input/read 各 1M，期望费用 2.5、complete=true、读价 0.5/M、写价 Fixed(0)。再测缺失为 Unknown、显式 0 为免费、分段未声明缓存价继承同条目基础值。
2. 最小修复：保留快照中的 Option 数值并换算 USD/token → USD/M；使用现有 RateSpec 转换，不能 `unwrap_or(0)`。检查 override 及基础值均遵守三态，不新增猜测兜底。
3. 索引做一次明确语义失效：基线 INDEX_VERSION=5，本批提升到 6；**拒绝 v4/v5**，不能保留当前 `INDEX_VERSION-1..=INDEX_VERSION` 而继续接受有错误缓存价的 v5。Task 6 沿用此版本，最终提交前若版本被其他工作占用则选择新版本并更新 fixture。
4. `test_pricing_index_rebuilds_stale_cache_rates` 用有效 source_sig 构造 v4/v5 索引，先证 fixture 可以按旧入口读到，再证新 load_cached 必须从来源重建且输出 2.5；新索引重启读取仍一致。
5. 验证：`cargo test openrouter --lib`；`cargo test pricing_index --lib`；`cargo test cost_breakdown --lib`。记录真实命中的测试数，再提交 `fix(计价): 保留 OpenRouter 缓存单价并失效旧索引`。

### Task 2 · P1 · 修正时间覆盖及规则内部完整性选择（R02）

**文件：** `src/pricing.rs`、`src/report.rs` 的估算/费用明细测试；必要时 `frontend/src/types.ts`、`frontend/src/lib/costBreakdown.ts` 及其测试；`docs/stats-semantics.md`。

1. 写失败测试：
   - `test_valley_rate_overrides_base_before_channel_selection`：A base=10，UTC 08:00–20:00 input=2；B 固定5；12:00/1M input 选 B/$5，21:00 选 A/$10。同测开始含入、结束排除。
   - `test_complete_schedule_precedes_higher_partial_schedule`：A 同时适用两个 schedule，partial input=10/read未知、complete input=2/read=1；input/read 各1M，选 complete/$3。
   - `test_schedule_selection_all_partial_and_zero_token_unknown`：没有完整档才比较已知小计；缺价分项为零 token 不影响完整性。
2. 将候选内部选择提炼成与外层一致的比较规则。参考流程：

   ```text
   rules = 当前时刻适用的完整规则组合
   rules 非空：逐规则计算，base 仅供缺字段继承，不额外参与竞争
   rules 为空：用默认价格计划（包含默认上下文分段）
   inner = complete 优先 → cost 降序 → 固定稳定次序
   outer = 各条目的 inner → complete 优先 → cost 降序 → 现有 tie-break
   ```

3. 保留“一个最终公式来自一个渠道/时间档”的约束。多时间规则重叠合法时依旧比较，不静默改成首条命中。检验星期、时区、长上下文和 SameAsInput 同时存在时的继承顺序；单窗口跨午夜仍按当前同日窗口限制拒绝并诊断，不顺带新增跨午夜语法。
4. 候选内排除的不完整时间规则要能披露：可增加独立的规则级诊断字段；不要改变 `candidate_count`、完整/不完整候选计数的单位，也不要把已被时间条件排除的基础档称为“缺价候选”。DTO 和 tooltip 随必要字段同步补测。
5. 验证：`cargo test schedule --lib`；`cargo test valley --lib`；`cargo test cost_breakdown --lib`；涉及 UI 时 `pnpm --dir frontend test src/lib/costBreakdown.test.ts src/components/EventTable.test.ts`。
6. 提交 `fix(计价): 按有效时间规则执行完整优先择价`。

### Task 3 · P1 · 使用真实日期控件契约并支持 local（R03）

**文件：** `frontend/src/components/DateRangeSelect.vue`、同目录 `DateRangeSelect.test.ts`；`frontend/src/lib/dates.ts`，新增 `frontend/src/lib/dates.test.ts`；`frontend/src/composables/timezone.ts`；`frontend/src/views/Dashboard.test.ts`。

1. 写失败测试 `picker_emits_selected_calendar_date`、`picker_displays_existing_calendar_date`，用真实 NDatePicker 或真实本机日历事件值覆盖 Asia/Shanghai 与 America/Los_Angeles。上海点 2026-10-06 应提交 2026-10-06；洛杉矶外部传此日应显示此日。现有喂 Date.UTC 的用例必须纠正，不能继续证明错误假设。
2. 首选 `v-model:formatted-value` + `value-format="yyyy-MM-dd"` 直接桥接字符串；先核实当前安装版 API、清空、键盘输入、无效输入行为。若保留毫秒，则严格使用本机 `new Date(y,m-1,d)` 和本机年月日提取，不能用 UTC 或统计时区重解释。纯日历 addDays 仍可用 UTC，不要无差别替换所有日期工具。
3. `todayInTz("local")` 复用已有 tzDate 或省略 Intl 的 timeZone。合法 IANA 路径保留。当前 timezone.ts 直接读取 localStorage，没有有效性校验；补小型校验函数：local 特判、其余用 Intl 验证，非法持久化值回退现有默认 Asia/Shanghai，并测试不会因旧偏好导致首屏异常。
4. 补 `local_timezone_renders_and_survives_reload`、`shortcuts_use_statistics_timezone`，以及 DST 起止日、跨年、清除/取消/重开测试。跨时区测试用独立进程/项目环境，避免并行测试全局改 TZ 串扰；打印 Intl 实际时区以证明测试环境生效。
5. 验证：`pnpm --dir frontend test src/components/DateRangeSelect.test.ts src/lib/dates.test.ts src/views/Dashboard.test.ts`；至少一条实际控件点击/输入 → emit 断言，不能全用 stub。
6. 提交 `fix(日期): 按控件日历语义选择日期并支持本机时区`。

### Task 4 · P1 · 消除视图快照的恢复、保存竞态（R04）

**文件：** `frontend/src/views/Dashboard.vue`、`Dashboard.test.ts`；`src-tauri/src/commands.rs`；复用 `src/fsutil.rs`；必要时抽取 `frontend/src/lib/viewSnapshot.ts` 及测试。

1. 用 deferred promises 写失败测试：
   - `snapshot_waits_for_matching_summary_and_events`：all 汇总10 → 切 claude → 先返回明细；不得以 claude 筛选保存旧汇总10。
   - `late_snapshot_cannot_replace_fresh_report`：新汇总99先到，旧缓存10后到；最终仍99。
   - `failed_refresh_does_not_save_mixed_snapshot`、`stale_failure_shows_retry_state`。
   - `latest_snapshot_wins_out_of_order_saves`：两次保存倒序完成，最终磁盘保留更新结果。
   - `unmounted_dashboard_cannot_overwrite_new_snapshot`：切设置再回汇总后，旧实例的查询/保存晚到也不能覆盖新实例。
2. 每个请求在发起时捕获不可变 key 和批次，成功时连同结果保存。summary key 包括 by/agent/range/tz/refreshKey；events key 包括 agent/range/tz/drill/refreshKey 及分页身份。使用完整快照筛选身份协调两者；不拿响应返回时的 currentFilters 冒充其请求身份。
3. 明确初始化状态机：可以先完成缓存恢复再启动首轮查询，也可保留并发但加入“任何新查询结果/用户操作后缓存不再接管”的守卫。恢复本身不能反复触发混乱的 immediate watcher；成功恢复后保证确有后台更新。
4. 同筛选手动刷新也要防止新汇总+旧明细拼接。保存门槛使用查询批次与相容 key，不仅比较筛选字符串。分页追加仅作用于所属明细查询。
5. 将视图快照升级 v4，忽略可能含混代数据的旧 v3。保存捕获不可变 payload，采用应用/模块级顺序队列（允许合并待保存最新项），跨 Dashboard 挂载周期保持顺序；卸载时使旧实例的未完成查询失效。队列不能只放在组件实例内，因为 App 使用 v-if 切换页面。不得并发 fire-and-forget 后让旧写覆盖新写；不要只加 atomic_write 就宣称顺序问题解决。
6. 壳端抽出可注入临时路径的 view cache 保存函数，调用 atomic_write；`test_view_cache_atomic_failure_keeps_previous` 注入失败验证旧内容完好，`test_view_cache_round_trip` 验证完整 payload。IPC 失败应处理，不制造未处理 Promise rejection；缓存写失败不阻止新鲜数据显示。
7. 状态按实际 loading/error/stale 组合呈现：stale + error = 旧数据/刷新失败，stale + idle 不能写刷新中。
8. 验证：`pnpm --dir frontend test src/views/Dashboard.test.ts`；`cargo test --manifest-path src-tauri/Cargo.toml view_cache`。提交 `fix(缓存): 协调视图查询身份与快照读写顺序`。

### Task 5 · P1 · 将来源上下文纳入事件缓存身份（R05）

**文件：** `src/cache.rs`、`src/report.rs`；新增 `tests/cache_source_identity.rs`；`docs/stats-semantics.md`。保持 source 解析职责不变。

1. 写端到端失败 fixture：`logs/a/b/s.jsonl` 为手工 Claude assistant 日志。所有路径临时隔离。
   - `cache_source_identity_root_change_matches_refresh`：根 logs → logs/a；暖缓存项目必须从 a/b 变 b，与 refresh 一致。
   - `cache_source_identity_agent_change_reparses`：同路径 Claude → Codex；应按 Codex 真解析为0事件，不能返回改标 agent 的旧事件。
   - `cache_source_identity_warm_hit_skips_parse`：上下文不变仍命中，CountingSource 证明不是用全量重解析掩盖问题。
2. 显式引入 CacheSourceIdentity，至少包含 agent + 规范化根目录 + 解析语义版本。文件键为该上下文与规范化文件路径；size/mtime 仍为内容指纹。路径规范化复用现有能力，Windows 大小写/`..` 规则一致，不新增另一套不兼容规范。
3. lookup SQL 真正过滤上下文；store、唯一键、purge 和并发事务一起更新。purge 不能删除别的 agent/上下文；不在命中后把别的来源产物强行 relabel。无需在每次 lookup 后重算 Claude project 来绕过错误缓存身份。
4. SQLite schema 由基线 v3 升 v4（若已被占用则再增）；旧派生缓存自动清理重建，不修改原日志。补 `test_cache_schema_invalidates_contextless_rows`，以及目录重叠既有回归。
5. 验证：`cargo test --test cache_source_identity`；`cargo test cache --lib`；`cargo test overlap`；`cargo test --test golden_reconciliation`。提交 `fix(缓存): 按来源类型和根目录隔离解析产物`。

### Task 6 · P2 · 统一有限非负单价校验（R06）

**文件：** `src/pricing.rs`、`src/modelsdev.rs`、`src/openrouter.rs` 及各自测试；`docs/stats-semantics.md`。

1. 写失败测试 `test_nonfinite_price_rejected`（TOML nan/inf/-inf、OR 字符串 NaN/Inf）；`test_invalid_base_not_restored_by_degradation`（models.dev 快照 input=-2）；合法0必须保留。
2. 统一 Fixed 合法条件 `value.is_finite() && value >= 0.0`。单位换算后再校验，避免巨大 USD/token 乘1M后溢出。覆盖 base/segment/schedule/period、外置、在线解析、本地快照与当前版索引恢复。
3. 明确拒绝策略：任一显式数值单价非法时，拒绝该候选条目并产生定位到来源/模型/档位/分项的 warning，其他合法候选继续参与。不得把非法显式覆盖改成 Unknown 后继续继承：当前 Unknown 也表达缺字段，这会悄悄恢复基础价并变成 complete=true。非法 base 绝不原样保留，也不新增“无效”费率状态来扩大此次模型改造。
4. 所有来源使用同一数值校验策略；当前版索引发现非法条目时从来源重建，不能静默接受。结构性分段错误的既有降级只在基础价数值合法且已有明确契约时保留；显式非法数值优先按本任务拒绝整条候选。验收：无非法数值传播、0合法、无合法替代候选时保持未计价、有合法候选时正常择价；在 stats-semantics 写明这一边界。
5. `test_invalid_price_cannot_survive_index_load` 用有效签名索引验证恢复路径。沿用 Task 1 的新索引版本，不能为兼容旧错误值恢复旧格式读取窗口。
6. 验证：`cargo test nonfinite`；`cargo test invalid_base`；`cargo test invalid_price`；`cargo test pricing --lib`。断言输出费用有限且非负、warning存在、complete/unknown符合实际。
7. 提交 `fix(计价): 拒绝非有限及负单价进入估算`。

### Task 7 · P2 · 关闭命令失败保持可重试（R07）

**文件：** `frontend/src/App.vue`、`App.test.ts`；`frontend/src/components/CloseConfirmDialog.vue`、`CloseConfirmDialog.test.ts`；必要时 `src-tauri/src/commands.rs`。

1. `close_failure_remains_visible_and_retryable`：mock close_resolve reject，弹窗仍可见，错误可读；取消记忆后重试可成功。`close_submission_prevents_duplicate_invokes`、取消不 invoke、重开记忆框复位回归同时保留。
2. onCloseResolve 改 async，pending 时禁用提交，成功后关闭/由窗口动作结束；失败显示错误并保留选择。不要无条件 finally 关闭弹窗，也不要因写盘失败自动覆盖坏 TOML。
3. 后端 hide 失败如仍被忽略，改为可返回错误，前端沿同一路径处理。托盘退出旁路、已有记忆三态不变。
4. 验证：`pnpm --dir frontend test src/App.test.ts src/components/CloseConfirmDialog.test.ts`；后端变更时 `cargo test --manifest-path src-tauri/Cargo.toml`。
5. 提交 `fix(关闭): 展示失败原因并支持重新提交`。

### Task 8 · P2 · 空结果和部分结果保留采集诊断（R08）

**文件：** `frontend/src/views/Dashboard.vue`、`Dashboard.test.ts`；`frontend/src/components/UsageTable.vue`、`UsageTable.test.ts`；可抽出 `frontend/src/components/CollectionDiagnostics.vue`。

1. `empty_report_keeps_collection_errors_visible`：groups=[]、source_status=ready、sources[0].stats.io_errors>0/warnings非空；页面必须显示采集失败原因。`partial_report_has_visible_notice`：部分数据仍展示，并可读到缺失说明。`clean_empty_report_is_not_error`：正常无匹配区间不要误报。
2. 将诊断移到不依赖表格是否挂载的位置，用 DESIGN.md 内联通知条和可展开详情；详细统计可以折叠，但存在不完整采集的事实不可默认藏起。
3. 对 warning/坏行/IO错误/目录停用分开命名，不声称所有 warning 都表示丢数据。避免同一报告诊断在页面和表格重复出现；错误/partial 状态与 Task 4 的状态机共用规则。
4. 验证：`pnpm --dir frontend test src/views/Dashboard.test.ts src/components/UsageTable.test.ts`。提交 `fix(诊断): 在空结果和部分结果中保留采集告警`。

### Task 9 · P2 · 补齐真实 tooltip 行为与唯一行身份（R09/R10）

**文件：** `frontend/src/components/SummaryCards.vue`、`SummaryCards.test.ts`、`EventTable.vue`、`EventTable.test.ts`；必要时 `frontend/src/accessibility.test.ts`。

1. `hit_rate_tooltip_opens_on_focus` 必须挂载真实 NTooltip 并等待其触发时序；先证明当前 focus 失败、hover成功。实现 hover/focus/click 受控显示及 Escape 关闭，移动到内容区域不应立即消失，焦点样式可见。不能只断言 tabindex 或把浮层默认内容直接渲染出来。
2. 验证 `pnpm --dir frontend test src/components/SummaryCards.test.ts src/accessibility.test.ts`，提交 `fix(无障碍): 让命中率公式支持键盘读取`。
3. `same_second_requests_have_distinct_row_keys` 构造同秒/同agent/model/session、不同 cursor 两行；从实际传给 NDataTable 的 row-key 断言不同。rowKey 直接使用后端 cursor，不重新拼展示时间。
4. 再测追加页和两个费用浮层的关联，不因行 identity 冲突共用状态；保留后端完整精度/同 timestamp 空 record_id 的游标回归。
5. 验证 `pnpm --dir frontend test src/components/EventTable.test.ts`；`cargo test events_pagination`（包含同秒、空 record_id 和完全相同时间戳用例）。提交 `fix(明细): 使用后端游标作为表格行身份`。

### Task 10 · 收敛证据、完整门禁和计划状态

**文件：** `docs/plans/README.md`、本计划、`docs/stats-semantics.md`、`docs/plans/archive/partial/2026-10-06-design-system-visual-qa.md`、`docs/plans/d5-acceptance-checklist.md`；导航若确认缺陷才改 `frontend/src/App.vue` 并补对应测试/截图。

1. 先核实导航证据冲突：长页面浅/深主题滚动，用实际 WebView 截图/元素边界证明内容是否从导航后经过。若被兄弟容器裁剪，让 sticky header 与内容处于同一滚动上下文，保持52px高度、窗口最小尺寸、横幅/浮层不被遮挡；修改前后截图与滚动位置可复核。若实测符合，则只登记证据，不为重构而改代码。
2. 核实 QA 的截图存档；缺少原图时注明“历史文字记录，图片当前不可核验”，补图需使用合成/脱敏展示数据。100%/150% 与 D5 安装等仍未执行时明确待验，不填通过。此前用户后延的事项继续后延，不因本计划自动触发安装/自启/发布。
3. 同步 README 各计划状态和覆盖关系；已修 R01–R10 链接本计划提交证据，保留历史审核原文；不把新通过记录倒填为当时已通过。只在完成定义满足后归档，允许技术修复完成与发布验收待办并存。
4. 严格串行执行完整门禁（不使用 PowerShell `&&`）：

   ```powershell
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path src-tauri/Cargo.toml
   pnpm --dir frontend typecheck
   pnpm --dir frontend format:check
   pnpm --dir frontend test
   pnpm --dir frontend build
   ```

5. 门禁通过后停止无理由重复测试；回读 UTF-8 文档，确认精确文件 diff，按具体文件 add/commit/push。不得提交他人改动或 `--no-verify`。每个修复 commit 都记录定向失败→通过；最终文档提交 `docs(验收): 回写全计划审核修复结果与待验事项`。

## 完成定义与执行记录

- [x] R01：OR 数值/零/缺失缓存价正确，旧索引强制重建。（fcf1780）
- [x] R02：谷价可覆盖基础价，候选内/跨候选完整优先，规则诊断不冒充渠道计数。（6e33596）
- [x] R03：真实 picker 双向日期正确，local/跨时区/DST 回归通过。（48fa3fb）
- [x] R04：快照不混代、不覆盖新状态、按序原子保存，失败状态可解释。（2006f4b）
- [x] R05：换 root/agent 与 refresh 结果一致；正常暖缓存仍跳过解析。（b8fc81a）
- [x] R06：非法单价不进入费用；所有入口、降级与索引恢复一致。（57de3c1）
- [x] R07/R08：关闭失败可重试，empty/partial 均能看到诊断。（8d1ea8f / 4c0ae8b）
- [x] R09/R10：真实键盘 tooltip 生效，同秒请求行身份唯一。（244d6cc）
- [x] 全门禁通过；新增测试隔离真实缓存；文档覆盖关系准确。
- [x] 导航证据冲突有明确判定（2fedca7）；D5 与未完成视觉矩阵单列，不伪造验收。

## 执行记录（2026-10-07，审核基线 a42e843）

| 任务 | 修复提交 | 修复前失败证据 | 修复后验证 | 遗留/后延 |
| --- | --- | --- | --- | --- |
| 1 R01 | fcf1780 | test_openrouter_base_cache_rates_preserved 红（\$2/不完整，应 \$2.5/完整） | openrouter 11 / pricing_index 3 / cost_breakdown 2 绿 | — |
| 2 R02 | 6e33596 | test_valley_rate_overrides… 红（12:00 选基础 \$10，应按谷价 \$2 与 B \$5 比较选 B） | schedule 3 / valley 1 / cost_breakdown 2 绿；excluded_incomplete_schedules 诊断不冒充渠道计数 | — |
| 3 R03 | 48fa3fb | picker_emits_selected_calendar_date 红（HEAD 组件下 4 用例失败；手选 10-06 提交 10-05） | 真实 NDatePicker 键入→emit 断言绿；local 时区/校验 4 用例绿；DST/跨年纯日历绿 | 洛杉矶等负偏移由字符串往返不变量覆盖（Windows Node 忽略 TZ，无法进程内切时区） |
| 4 R04 | 2006f4b | snapshot_waits_for_matching… 红（以 claude 筛选保存旧 all 汇总） | 队列单测 4 / Dashboard 场景 6（含 v3 忽略、卸载不落盘、失败态胶囊）/ 壳 round-trip+原子失败 2 绿 | — |
| 5 R05 | b8fc81a | cache_source_identity 三用例红（换根项目 a 不变 b；换 agent 改标旧事件） | 端到端 3 绿 + cache 21（含 schema v4 失效/身份隔离单测） | — |
| 6 R06 | 57de3c1 | test_nonfinite_price_rejected_toml 等 4 红（nan/inf 进费用、降级恢复非法 base） | nonfinite 2 / invalid_base 1 / invalid_price 1 / pricing 59 绿 | — |
| 7 R07 | 8d1ea8f | close_failure… 红（失败静默关弹窗丢勾选） | App 集成 2（失败可见+重试成功、pending 防重复）+ 组件 2 绿 | — |
| 8 R08 | 4c0ae8b | empty_report_keeps_collection_errors_visible 红（空结果诊断不可见） | 3 场景绿（空+异常可见/部分可见/干净不误报）；表格重复展示移除 | — |
| 9 R09/R10 | 244d6cc | hit_rate_tooltip_opens_on_focus 红（focus 不开）；same_second… 红（行身份冲突） | 真实触发时序绿（focus/hover/Escape/Enter）；同秒行身份=cursor 绿；events_pagination 4 绿 | — |
| 10 | 2fedca7 | QA 文档导航证据与旧实现不符（header 在滚动容器外，内容不会从其后经过） | header 移入 .scroll-container（同滚动上下文吸顶），App/accessibility 15 用例绿；截图存档仍缺 → QA 注明"历史文字记录，图片当前不可核验" | 100/150% 缩放与 D5 安装验收继续后延（不伪造通过） |

门禁（最终，严格串行）：根库 fmt/clippy/test ✅ · 壳 fmt/clippy/test ✅ ·
前端 typecheck/format:check/test(167)/build ✅

## 后续复核附注（2026-10-07，修复后再次复核）

上表"R01–R10 完成"及门禁结论适用于本计划基线 `85f3c92`。其后的二次复核
（[修复后复核遗留计划](2026-10-07-post-remediation-recheck-fixes.md)）确认仍存在八项遗漏：
真实 v3 缓存结构未迁移（先建含 root 的索引连续报错退回全量扫描）、未命中的空时间规则把
基础价重新带入竞争、视图快照同筛选刷新混代/切维度拼接/晚到缓存重置用户筛选、非法覆盖
与扁平索引入口未整候选拒绝、两项索引测试依赖并行污染、主滚动容器无高度约束致吸顶失效、
tooltip/隐藏窗口失败的真实验收缺口。已在上述计划中逐项修复（提交 `7fc1e12`…`a44ac5a`，
含索引 v7、快照 v5、真实浏览器布局量测与 release 应用实例走查），D5 安装验收与
100%/150% 系统缩放继续后延。
