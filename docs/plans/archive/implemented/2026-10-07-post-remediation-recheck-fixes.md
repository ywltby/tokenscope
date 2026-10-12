# 修复后复核遗留问题 Implementation Plan

> **2026-10-11 归档复核：已实现。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| Task 1–8 / F01–F08 | 最终实现已完成，后续 AP07 补覆盖所有关闭入口 | 无独立待办 |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-07-post-remediation-recheck-fixes.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** 修复上一轮计划执行后仍存在的缓存升级、时间择价、非法价格、视图快照和导航问题，并使回归测试真实、独立、可重复。

**Architecture:** 保留现有计费与展示分层，仅修正各入口的校验和状态边界：先迁移 SQLite 表结构再建索引，先判断时间规则适用性再择价，索引统一归一化后校验，前端以共享刷新批次协调结果。沿用已完成的原子写、模块级保存队列、三态单价和日期字符串契约。

**Tech Stack:** Rust 2024、rusqlite/SQLite、serde/TOML、Tauri 2、Vue 3、Naive UI、Vitest/VTU、真实浏览器布局验收。

**状态：已执行完毕（2026-10-07，基线 `85f3c92` → 终点 `docs/验收` 提交）。** 逐任务红→绿证据见文末执行记录表；D5 安装验收与 100%/150% 系统缩放继续按既有决定后延，未伪造通过。

上游：[全计划审核修复](2026-10-07-all-plans-audit-remediation.md)、[首次审核证据](../../../reviews/2026-10-07-all-plans-audit.md)。本文件覆盖上游“R01–R10 全部完成”的验收结论及下列未闭合部分；已经修好的功能不重做。

## 1. 本轮确认的问题

| 编号 | 等级 | 位置（基线行号） | 复现与正确结果 |
| --- | --- | --- | --- |
| F01 | P1 | `src/cache.rs:97–102` | 真实 v3 的 files 表没有 root 列；打开时先建含 root 的索引，连续报 `no such column: root`，每次退回全量扫描。应自动迁移/重建派生表并恢复暖缓存。 |
| F02 | P1 | `src/pricing.rs:553–555` | A base=10/M、白天 schedule=2/M、独立晚间 schedule=15/M，B固定5/M；12:00/1M输入实际选 A/$10 且标签 peak，应选 B/$5。未命中的空 schedule 把基础价重新带入竞争。 |
| F03 | P1 | `frontend/src/views/Dashboard.vue:180–199,262–267` | 同筛选刷新保存新汇总99+旧明细10；切维度保存 filters.by=model/report.by=day；用户已选 claude，晚到 all 快照又重置筛选。应满足完整身份、共享批次及用户操作优先。 |
| F04 | P2 | `src/pricing.rs:1125–1129`、`src/openrouter.rs:192–194` | 外置时间价 nan 或 OR override NaN 被丢弃后，模型仍按基础价完整计费。任一显式数值非法应拒绝整条候选，其他候选继续参与。 |
| F05 | P2 | `src/pricing.rs:375–377,1405–1409,2030` | 有效签名的当前版索引 plan=null、扁平 input=-5 被接受，实得负费用。应失效并按合法来源重建。 |
| F06 | P2 | `frontend/src/App.vue:165–193` | 父容器取消 flex，子容器仅 flex:1/overflow:auto，实际没有高度约束。1280×820 视口、2200px合成内容时容器高2252px不可内部滚动；document滚400后导航top=-400。应保持导航吸顶、内容从其后滚过。 |
| F07 | P2 | `src/pricing.rs:4910–4916,5259–5263` | 两项新增索引测试单独运行均 exit 101；篡改磁盘索引后未隔离内存缓存。整套通过依赖其他并行测试碰巧替换全局缓存。 |
| F08 | P2/验收缺口 | `frontend/src/components/SummaryCards.test.ts:8–17,137`；`src-tauri/src/commands.rs:214` | 自称真实 NTooltip 的测试仍全局打桩；隐藏窗口失败仍被忽略。应验证实际浮层、透传 hide 错误，不重做已完成的关闭记忆失败反馈。 |

异常配置边界一并纳入 F04/F05：有限 input 单价 `1e308`、1M input 的中间乘法会溢出成 `inf`，尽管数学结果 `1e308` 有限。此例不代表常规模型价格受影响，但不能作为完整正常费用返回。

## 2. 不变量与范围

1. 不恢复内置价格，不猜缓存折扣；Unknown、Fixed(0)、SameAsInput 保持不同；只按历史请求实际条件择价，完整候选优先且不跨渠道拼价。
2. 未命中的时间窗不得凭空成为另一套基础价。规则级**明确声明**的默认价格/适用分段仍保留，不能一刀切删除所有未命中 period 的 schedule。
3. 显式非法数值与缺字段不同：不得把非法值变成 Unknown 后继承，也不得只删非法覆盖而恢复基础价。结构性规则错误沿既有明确契约处理，数值非法优先拒绝整候选。
4. 旧 SQLite 结构必须先迁移再使用新列；升级只改 TokenScope 自有派生表，不删数据库文件、WAL文件或原始日志。健康 v4 缓存不因本次修复重复清空。
5. 快照保存同时验证完整查询身份和批次；同筛选手动刷新也产生新批次。用户操作后，晚到启动缓存不可接管页面。
6. 测试不修改真实 `~/.tokenscope` 或 agent 目录。所有 SummaryOptions 显式注入临时 cache_dir、pricing_index、pricing_path、modelsdev_path、openrouter_path 和来源目录；不用 `Z:/no-such` 等机器相关路径代替隔离。
7. 真正依赖旧表、冷启动、组件浮层、浏览器布局的测试，必须使用对应真实契约；不得用“改版本号”“命中内存”“检查 aria 属性”“CSS 文本含 sticky”冒充验证。

非目标：新增来源或计费语法、单时间窗跨午夜、新收费模式、修改四桶统计、重做 R01/R03/R08/R10、改数据库引擎、自动安装/发布。D5 安装和100%/150%系统缩放继续按既有决定后延。

## 3. 执行顺序

按 **Task 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8** 执行。先恢复索引测试可靠性，再修核心行为。pricing.rs 相关任务串行，Dashboard 相关任务串行；每项先确认失败、最小修复、确认通过、单独提交。测试过滤命中0项不算通过。

### Task 1：让索引回归真正覆盖冷启动（F07）

**修改：** `src/pricing.rs` 的两项新增索引测试。
**新增：** `tests/pricing_index_restart.rs`，使用独立子进程执行实际 `Pricing::load_cached`。

1. 记录基线红灯（两条命令均应执行1项并失败）：

   ```powershell
   cargo test --lib test_pricing_index_rebuilds_stale_cache_rates --offline -- --nocapture
   cargo test --lib test_invalid_price_cannot_survive_index_load --offline -- --nocapture
   ```

2. 将这两项磁盘索引回归迁入新的集成测试，保留名字，移除原失真的重复用例。优先使用集成测试可执行文件的隔离子进程模式：父进程提供临时 fixture 路径和阶段，子进程调用真实加载链并断言；环境只传测试路径/阶段，禁止回落默认数据目录。
3. 明确阶段：合法来源→seed进程生成有效签名索引→父进程通过serde篡改该索引→新进程cold-load验证重建→再一个新进程验证合法索引可读取。每个子进程必须检查退出码，不能运行失败后继续当通过。另单测同进程正常重复加载仍可命中内存，不修改生产缓存策略来迁就测试。
4. 新测试不借助其他测试清 PRICE_CACHE。若保留任何同进程全局状态用例，复用已有 CACHE_TEST_LOCK 并正确清理，但**不能认为只锁两个新测试就能隔离其他 report 测试的间接调用**；需要冷启动的断言全部放子进程。
5. 验证新的实际入口，单独、串行和默认并行均须通过：

   ```powershell
   cargo test --test pricing_index_restart test_pricing_index_rebuilds_stale_cache_rates
   cargo test --test pricing_index_restart test_invalid_price_cannot_survive_index_load
   cargo test --test pricing_index_restart -- --test-threads=1
   cargo test --test pricing_index_restart
   ```

6. 更新计划记录中的测试位置/命令，避免再运行旧 `--lib` 过滤获得0项成功。提交：`test(定价): 在隔离进程中验证价格索引恢复`。

### Task 2：按真实旧表结构迁移事件缓存（F01）

**修改：** `src/cache.rs`、`tests/cache_source_identity.rs`。
**新增：** `tests/fixtures/cache/schema-v3.sql`（从 `3109678:src/cache.rs` 的实际DDL提取，UTF-8合成fixture，不含真实数据）。

1. `test_cache_migrates_real_v3_schema`：用该SQL建立无root列、path仍独占唯一的v3表，写入meta=3和合成事件；调用当前 Cache::open。修复前必须报 no such column，而不是先建v4再改meta。
2. 把初始化流程收敛为同一事务：

   ```text
   打开连接并设置busy timeout/WAL等
   开始写事务（并发打开须在事务内重新判断版本）
   建/读meta，读取真实schema版本
   旧版本或无版本的旧结构：先drop events，再drop files（索引随表删除）
   建当前files/events及索引，最后更新schema_version
   commit；任一步失败rollback，返回现有可见降级错误
   ```

   只重建明确属于缓存的表，不删除整个文件，不用 DELETE 假装结构迁移。当前健康v4沿用，不必为了改初始化顺序升级v5。损坏meta读取异常不再无条件吞成“首次启动”；保持诊断可见。
3. 断言新表确有root、复合唯一键生效：同path不同root/agent可以共存；旧事件已失效；写入后重新打开能命中。补 `test_cache_migration_is_atomic`（在schema helper内注入失败，版本和表结构一起回滚）、`test_cache_concurrent_v3_open`（两个连接不能交错半迁移）。失败注入只在测试提供，不增加生产配置。
4. 修正两个弱断言：换根测试必须**先验证换根暖查询，再跑refresh作对照**，不得refresh先清库；暖命中测试用CountingSource证明parse次数不增长，不能只比较两个相同结果。补齐这些测试遗漏的临时pricing_path。
5. 验证：`cargo test cache_migrates_real_v3_schema --lib`、`cargo test cache_migration --lib`、`cargo test cache_concurrent_v3_open --lib`、`cargo test --test cache_source_identity`、`cargo test --test source_overlap`、`cargo test --test golden_reconciliation`。
6. 提交：`fix(缓存): 在使用新列前完成旧表结构迁移`。

### Task 3：排除不适用的空时间规则（F02）

**修改：** `src/pricing.rs` 的 matching_time_rules/estimate 及单测；`docs/stats-semantics.md`。

1. `test_unmatched_empty_schedule_does_not_reintroduce_base`：A base10；valley period UTC08:00–20:00=2；另一peak period20:00–23:00=15；B固定5。12:00的1M input必须选B/$5，20:00选A/$15，23:00才恢复A/$10。不能把peak标签贴到中午请求。
2. 区分“命中period”“明确的schedule默认覆盖”“没有可适用覆盖”：
   - 有period命中：返回命中的整规则组合。
   - 无period命中，但schedule.prices存在Fixed（包括0）/SameAsInput，或当前prompt命中的schedule分段含显式覆盖：保留这一明确默认规则。
   - 无命中且无上述显式覆盖：跳过该schedule；不能因缺字段继承base就算它适用。
   - 所有schedule均不适用：默认plan（含默认上下文分段）参与一次。
3. 若判断schedule分段需要prompt大小，将已有basis值传给匹配/适用性判定；不要用不适用分段的存在代替实际命中。保留有效规则内完整优先、跨候选完整优先、稳定tie-break、SameAsInput与候选/规则诊断计数。
4. 补 `test_explicit_schedule_default_remains_applicable`、`test_schedule_default_segment_requires_matching_basis`；显式0/引用输入价都算声明，空规则和仅标签/时区不算声明。覆盖星期不匹配和窗边界，不增加跨午夜语法。
5. 验证：`cargo test schedule --lib`、`cargo test valley --lib`、`cargo test cost_breakdown --lib`。提交：`fix(计价): 跳过没有适用价格覆盖的时间规则`。

### Task 4：封住非法覆盖和扁平索引入口（F04/F05）

**修改：** `src/pricing.rs`、`src/openrouter.rs`；`tests/pricing_index_restart.rs`；`docs/stats-semantics.md`。数值溢出诊断按需同步 `src/report.rs`、`frontend/src/types.ts`、`frontend/src/lib/costBreakdown.ts` 及其测试。

1. 写失败回归：
   - `test_invalid_schedule_rate_rejects_candidate`：base=2、period input=nan，不能返回base/$2；同测schedule自身、schedule.segment、负数、正负无穷。
   - `test_invalid_openrouter_override_rejects_model_during_sync`：通过真实sync_with注入合成API，base prompt=0.000002、override min=100000/prompt="NaN"；整个模型不入快照，合法兄弟模型保留，告警包含模型和分项。
   - `test_invalid_flat_rate_in_current_index_forces_rebuild`：有效签名当前索引plan=null/扁平input=-5；新进程必须重建至来源input=2，并保留显式0合法用例。
2. 外置和OR在丢弃任何结构性规则前，对**原始候选所有显式数值**做预检查；非法数值将整个模型标为拒绝，不能在内部循环 `continue` 后仍写基础条目。可以使用明确错误类型或独立数值预检，不通过匹配错误字符串决定是否拒绝。同步诊断沿已有告警路径报告，不把整个来源中其他合法模型一起丢弃。
3. 单位转换后的结果也校验有限非负。索引先将plan/旧扁平字段统一归一成完整PricePlan，再校验、再add_entry；load_cached与from_index不得存在另一条未校验恢复通道。当前版索引有非法条目时整份索引按来源重建；不要仅判断 `Some(plan)`。
4. 新索引语义由v6升v7，仅接受新版本，拒绝旧v6派生错误；更新 Task 1 旧版fixture、当前版fixture及冷启动断言。无需更改正常来源快照格式。**已在旧OR快照中被丢弃的override无法靠重建索引找回**：验证使用修复后的注入同步链；不删除用户离线快照，不自动联网，真实数据恢复待下次正常同步。
5. 补 `test_finite_rate_does_not_overflow_intermediate_cost`：1M tokens × 1e308/M应得有限1e308；用 `tokens as f64 / 1_000_000.0 * rate` 等避免不必要的中间溢出，并检查分项及总和。真正超出有限表示范围时不能夹成0/最大值或complete=true：该项不计金额、保留未计价token并给“金额超出可表示范围”的结构化原因，经breakdown解释；其他正常候选仍按完整性规则竞争。仅在此场景补必要DTO字段，不改变正常公式。
6. 验证：`cargo test invalid_schedule --lib`、`cargo test invalid_openrouter_override --lib`、`cargo test --test pricing_index_restart`、`cargo test finite_rate --lib`、`cargo test pricing --lib`；涉及DTO时补 `src/report.rs` 与 `frontend/src/lib/costBreakdown.test.ts`，验证原因可见。
7. 提交：`fix(计价): 在所有恢复入口拒绝非法候选`；若数值溢出防护跨DTO，另一个同主题小提交记录其回归，不把它藏在校验提交中。

### Task 5：给视图快照真正的批次和恢复所有权（F03）

**修改：** `frontend/src/views/Dashboard.vue`、`Dashboard.test.ts`、`frontend/src/lib/viewSnapshot.ts`、`viewSnapshot.test.ts`。保留壳端原子写与现有模块级保存队列。

1. 先写三个失败测试：`same_filter_refresh_failure_does_not_save_mixed_snapshot`（旧10/10，新summary99、events reject，不得新增99/10保存）；`dimension_switch_waits_for_matching_report`（day→model，events先到不能保存model/day）；`user_filter_change_prevents_late_cache_restore`（首载未返回，用户选claude，all缓存晚到不能接管）。
2. 用一个主刷新协调入口代替两条独立watcher各自制造“批次”的做法。按主筛选by/agent/range/tz、refreshKey变化或手动刷新，同步生成单一refreshEpoch，给两条查询传入同一epoch与不可变key。两侧各自的requestSeq继续用于丢弃旧响应，不能用它们碰巧相等来证明属于同一批次。
3. 保存门槛：summary与events均成功、属于当前epoch，主key完全一致；summary.report.by与key.by一致，events另带drill及分页所属身份。发起新epoch即撤销旧结果的保存资格，界面可继续显示旧结果。某侧失败时保留上一份一致快照，不得由另一侧成功写新旧拼接。
4. drill切换不必重扫汇总：作为当前主epoch下的子查询，携带新的drill版本；summary同主epoch可复用。追加页仅允许在所属明细首批完成后执行，锚点来自同key/epoch/drill版本；不把旧表的cursor用于新筛选。增加 `append_cannot_invalidate_new_filter_first_page` 回归。
5. 启动恢复增加用户操作版本：读取缓存前捕获interactionEpoch，任何筛选、下钻、手动刷新操作都立即递增并撤销恢复资格；即使新查询尚未返回也生效。恢复自身不要被当作用户操作反复触发watcher；可用明确initializing/restoring状态，恢复后恰好启动一批刷新。卸载拒收、fresh-result守卫保留。
6. 快照v4可能已有混代内容，升级为v5并忽略旧v4；epoch不跨进程当事实使用，恢复后为新实例分配身份。保存前捕获独立payload，模块级队列继续跨挂载串行；旧实例晚到不能入队。完善现有跨挂载、倒序响应、相同筛选重复刷新、失败重试测试。
7. 验证：`pnpm --dir frontend test src/views/Dashboard.test.ts src/lib/viewSnapshot.test.ts`，断言实际dispatch payload和次数，不只检查页面文字。提交：`fix(视图缓存): 按共享刷新批次保存相容结果`。

### Task 6：恢复有边界的导航滚动容器（F06）

**修改：** `frontend/src/App.vue`、`App.test.ts`；`docs/plans/archive/partial/2026-10-06-design-system-visual-qa.md`。
**新增：** `frontend/scripts/check-app-scroll.mjs`（可复用的真实浏览器验收脚本，运行环境依赖写明）。
**按需修改：** `frontend/package.json`、`frontend/pnpm-lock.yaml`（仅在需要落地浏览器测试依赖时）。

1. 脚本从当前App.vue读取实际style，并构建不含用户数据的2200px合成长页面，不能复制一套“修正后的CSS”给测试自己通过。使用可用Playwright/Chromium；若环境缺依赖先按仓库方式安装测试依赖，不能只用happy-dom声称布局通过。
2. 修复前记录1280×820：container.clientHeight=scrollHeight=2252、设置scrollTop400仍0，document滚400后header.top=-400。
3. 最小修复可恢复 `.app-shell { height:100vh; display:flex; flex-direction:column }`，给 `.scroll-container { flex:1; min-height:0; overflow-y:auto }`；header继续放在该容器内。不得把header移回兄弟节点来“恢复吸顶”，那会恢复旧玻璃问题。
4. 自动断言容器高度受视口限制、scrollHeight>clientHeight、设置scrollTop后确实内部滚动、header.top保持容器顶边（允许1px误差）、内容坐标随滚动变化。按1280×820与980×620、浅/深模式执行；确认浮层和通知可见，body不会意外产生第二条主滚动条。再做一次实际应用/开发窗口走查，脚本合成布局不冒充完整WebView验收。
5. 验证：`node frontend/scripts/check-app-scroll.mjs`、`pnpm --dir frontend test src/App.test.ts src/accessibility.test.ts`。保存脱敏截图和量测输出，回写QA；100%/150%系统缩放仍标后延。提交：`fix(布局): 约束主滚动容器并验证导航吸顶`。

### Task 7：补真实 tooltip 验收和隐藏失败反馈（F08）

**修改：** `frontend/src/components/SummaryCards.vue`、`SummaryCards.test.ts`；新增 `frontend/src/components/SummaryCards.tooltip.test.ts`；`src-tauri/src/commands.rs`、`frontend/src/App.test.ts`。

1. 在独立测试文件挂载真实NTooltip，禁止mock naive-ui；等待真实Teleport/触发时序，断言公式在初始隐藏、focus/hover后可见、Escape后关闭。另测从触发器移入内容不会立即消失，以及focus仍在时鼠标离开不应取消键盘展示。不能只看aria-expanded，也不能靠已有全局透传stub检测可见性。
2. 若现有单布尔开关不能满足上述行为，最小拆分hover/focus/点击固定状态并统一show计算，补Escape关闭逻辑；不重写整套浮层。旧测试去掉“真实组件”误导性描述。
3. `close_resolve` 用 `w.hide().map_err(...) ?` 返回实际错误，只有成功后记成功日志；主窗口不存在同样返回可操作错误。复用现有前端await/catch/pending，不改成功的记忆保存顺序、三态行为和托盘退出旁路。
4. `test_close_hide_error_is_propagated` 用可注入窗口动作验证返回Err；`close_hide_failure_remains_retryable` mock IPC hide失败，弹窗/原因仍可见，重试成功后关闭。保留已有记忆写盘失败用例，不把“无错误”当隐藏成功。
5. 验证：`pnpm --dir frontend test src/components/SummaryCards.test.ts src/components/SummaryCards.tooltip.test.ts src/App.test.ts`；`cargo test --manifest-path src-tauri/Cargo.toml close`。
6. 分别提交 `test(无障碍): 用真实浮层验证命中率说明`（若修交互则用fix）和 `fix(关闭): 透传隐藏窗口失败并允许重试`。

### Task 8：集成验证与状态回写

**修改：** 本计划、`docs/plans/README.md`、`docs/stats-semantics.md`、上一轮修复计划的复核附注、视觉QA。

1. 每项记录修复前失败、修复后通过的精确命令、命中数量与提交号。不要覆盖历史执行记录；追加“后续复核发现/本次修正”，将旧“全部完成”注明其适用基线。
2. 完整门禁串行执行，任何失败先停下定位：

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

3. 门禁不能替代Task 1独立进程与Task 6真实布局验证；两者各通过一次并留证后，不做无理由循环跑全套。正常套件不运行 ignored 真实性能测试，不触碰真实用户缓存。
4. 回读UTF-8、检查文档链接及git diff；按具体文件提交推送，不纳入他人修改，不使用 `--no-verify`。存在验证失败时按CLAUDE.md暂不提交，明确记录阻断，不能写“门禁全绿”。
5. 技术修复与D5后延验收分列，未获得真机证据不勾选。最终文档提交：`docs(验收): 记录二次复核修复及真实验证结果`。

## 4. 完成定义

- [x] F01：真实v3升级成功、迁移失败原子回滚、并发打开安全，健康v4暖命中保留。
- [x] F02：空的未命中规则不重引基础价，明确schedule默认覆盖保留。
- [x] F03：三个已复现快照竞态全部回归通过，分页/下钻/跨挂载仍正确。
- [x] F04/F05：原始非法覆盖整候选拒绝，扁平/plan索引统一校验，旧索引失效，费用不传播非有限值。
- [x] F06：真实浏览器量测吸顶有效，有脱敏证据，未完成系统缩放仍如实后延。
- [x] F07：两项索引测试独立及整套均通过，冷启动确实经过磁盘。
- [x] F08：真实tooltip出现/关闭可验证，hide失败返回前端可重试。
- [x] 全门禁与文档核对完成，历史与当前状态分明。

| 任务 | 状态 | 修复提交 | 红→绿证据 | 剩余限制 |
| --- | --- | --- | --- | --- |
| 1 | 完成 | `7fc1e12` test(定价): 在隔离进程中验证价格索引恢复 | 红：两条 `--lib` 索引测试单独运行 exit 101（`v5 旧语义索引必须失效重建` / `含非法数值的索引必须触发重建`——进程内 PRICE_CACHE 吞掉磁盘篡改）；绿：迁入 `tests/pricing_index_restart.rs` 子进程三阶段（seed→篡改→冷加载→可读）+ 同进程重复加载命中，单测/串行/并行均过（3 passed） | D5 安装、100%/150% 缩放沿用后延 |
| 2 | 完成 | `5821d74` fix(缓存): 在使用新列前完成旧表结构迁移 | 红：`test_cache_migrates_real_v3_schema` 等 3 条以真实 v3 DDL fixture（`tests/fixtures/cache/schema-v3.sql`）报 `no such column: root`；绿：open 收敛为 IMMEDIATE 写事务（读版本→drop events/files→建当前表→写版本→commit，注入失败整体回滚），并发打开加 WAL 重试；lib 157 并行全过 | 迁移仅重建 TokenScope 自有派生表 |
| 3 | 完成 | `001dee3` fix(计价): 跳过没有适用价格覆盖的时间规则 | 红：`test_unmatched_empty_schedule_does_not_reintroduce_base`（12:00 选 A/$10 带 peak 标签）、分段/星期两条（未命中仍贴时间档标签）；绿：matching_time_rules 携带 basis、未命中且无显式声明（Fixed/0/SameAsInput 或命中段覆盖）的规则整体跳过；schedule/valley/cost_breakdown 全过 | 命中规则缺字段继承基础价语义不变 |
| 4 | 完成 | `d543c6d` fix(计价): 在所有恢复入口拒绝非法候选；`1cefac7` fix(计价): 防止单位换算与求和的中间溢出并经明细解释 | 红：时间价 nan 整候选未拒（回基础 $2）、OR override NaN 模型仍入快照（count 2）、扁平 input=-5 冷加载命中（未重建）、1M×1e308 得 inf；绿：外置/OR 数值预检先于结构丢弃（整模型拒绝+定位告警随 SyncReport.warnings）、索引 v7 仅接受新版（plan 与扁平字段归一后统一校验，from_index 兜底跳过）、先除后乘+求和防溢出（2M×1e308 → overflow 行不计金额、token 转未计价、明细显示"金额超出可表示范围"）；显式 0 合法保留。旧 OR 快照中已被丢弃的 override 无法靠重建找回，待下次正常同步 | 用户离线快照不删除、不自动联网 |
| 5 | 完成 | `c21cb03` fix(视图缓存): 按共享刷新批次保存相容结果 | 红：同筛选刷新 99/旧明细落盘、切维度保存 filters.by=model+report.by=day、用户选 claude 后晚到 all 缓存重置筛选、新筛选首批在途时追加不被拒绝；绿：refreshEpoch 共享批次 + summaryBatchEpoch/eventsBatchEpoch 保存门槛（均属当前批次、主键含 by、report.by 一致）、append 门（首批完成+同身份）、interactionEpoch 撤销恢复资格、快照 v5 忽略 v4；Dashboard 30 + viewSnapshot 4 全过 | epoch 不跨进程、不声称两次 IPC 共享采集瞬间 |
| 6 | 完成 | `a1c8e90` fix(布局): 约束主滚动容器并验证导航吸顶 | 红：`node frontend/scripts/check-app-scroll.mjs` 修复前 4 场景全败（容器 2274px 随内容生长、scrollTop=400 无效、document 第二条滚动）；绿：`.app-shell` flex 纵向 + `.scroll-container` min-height:0，1280×820/980×620 × 浅/深四场景全过（截图+量测见 `qa-artifacts/app-scroll/`），release 应用实例滚动 10 页导航吸顶（`real-window-scrolled.png`） | 100%/150% 系统缩放继续后延 |
| 7 | 完成 | `fcba1cf` fix(无障碍): 拆分命中率浮层触发态并用真实浮层验收；`a44ac5a` fix(关闭): 透传隐藏窗口失败并允许重试 | 红：真实 NTooltip 测试（不打桩）——Escape/mouseleave/blur 后浮层仍显示（单布尔无法表达键盘与鼠标分离）；绿：hover/focus/pinned 三态拆分 + 统一 show，浮层可见性沿祖先链检测 display，移入内容不消失、focus 保持鼠标离开不取消；`close_minimize_with` 注入 hide 失败透传（Rust `test_close_hide_error_is_propagated`）、前端 `close_hide_failure_remains_retryable` 弹窗保持可重试 | — |
| 8 | 完成 | 本提交 `docs(验收): 记录二次复核修复及真实验证结果` | 全门禁串行执行通过：根库 fmt/clippy/test（15 套件全 ok、0 FAILED）、壳 fmt/clippy/test（17 passed）、前端 typecheck/format:check/test（18 文件 177 tests）/build 全绿；独立进程索引回归与真实浏览器布局验收各通过一次，不做无理由循环 | 历史记录未覆盖改写，仅追加 |
