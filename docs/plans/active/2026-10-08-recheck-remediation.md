# 复核遗留缺陷与验收补齐 Implementation Plan

> **执行说明：** 使用 `executing-plans` 技能按任务执行。本文为修复方案，**尚未执行**；本次只编写计划并纠正状态记录。修复前先运行能在当前代码上因目标行为失败的测试，再实现、验证、独立提交；不得仅凭测试名称或总通过数勾选完成。

**目标：** 修复 2026-10-08 复核确认的查询会话隔离、过期恢复、设置竞态、内存预算、可访问性和单价精度缺陷，补齐自动化与原生验收证据。

**架构：** 保留 Rust/Tauri/Vue 分层、不可变查询快照、应用内设置事务和后端统一计价。查询增加跨启动唯一身份与真实保留数据的预算记账；前端以已成功首页的会话身份控制分页，以统一设置读取和写入版本保护状态。浏览器覆盖真实组件与合成 IPC；原生验证使用隔离路径及真实 IPC，分别记录生产 CSP、启动和 DPI。

**技术栈：** Rust stable/edition 2024、Tauri 2、SQLite、Vue 3、Naive UI、ECharts、TypeScript 5、Vitest、Playwright/Chromium、Windows/WebView2。

**基线：** `22c9c26`，分支 `docs/product-review-plan`。承接 [安全、数据一致性与 UI/UX 修复计划](2026-10-07-ui-ux-review-remediation.md) 的 SF04、UX03、UX06、UX07、UX10；SF01 生产 CSP 与 UX09 原生首帧验收仍待闭合。原计划其余已修功能保留，不重新实现。

## 1. 复核依据与范围

本地原始证据位于 [复核报告](../../../qa-artifacts/recheck-2026-10-08/review.md)、[跨进程结果](../../../qa-artifacts/recheck-2026-10-08/query-results.txt)、[浏览器结果](../../../qa-artifacts/recheck-2026-10-08/browser-results.json)。这些是 gitignore 产物，其他检出可能没有；下表及各任务完整给出重建方式，不以本地文件存在作为执行前提。

| 编号 | 优先级/原计划 | 已确认现象 | 复核方式 |
| --- | --- | --- | --- |
| RC01 | P1 / SF04 | 两个进程均生成 q0-g0；第二个接受旧游标，total 从 4 变 5，返回新增事件 | 两个真实 Rust 子进程、合成日志 |
| RC02 | P1 / SF04 | 磁盘恢复的旧游标缺少当前首页成功门槛；过期错误条“重试”继续复用原 beginPromise | 前者源码；过期重试经真实浏览器复现 |
| RC03 | P1 / UX06 | 首载三次 settings_get；读取失败显示 ask；重试不恢复全部配置；保存 C:/saved 后晚到读取覆盖为 C:/old | 真实浏览器、deferred IPC |
| RC04 | P2 / UX06 | 补充源同步失败后只有“可重试”文案；主源已可用时原同步按钮消失 | 源码确认，实施时补失败复现 |
| RC05 | P2 / SF04 | 日期结果零行却保留全部事件；顺序查询各自保留不同事件分配，预算只数 rows | Rust 合成数据，未做内存耗尽测试 |
| RC06 | P2 / UX03 | 四个单价列名称都为缓存命中；Escape 后 Enter/Space 不打开费用浮层；帮助浮层外部点击不关闭 | 真实浏览器 |
| RC07 | P2 / UX07 | unit 场景 1.234567 显示为 $1.23，场景参数被忽略 | 直接运行当前格式化函数 |
| RC08 | P2 / 补充修复 | OpenRouter 价格字段 Option<String> 不接受数字 0 | 较早启动日志及当前源码；未证明最近启动再次触发 |
| RC09 | 验收 / UX10 | 24 场景虽通过，失败恢复、业务状态、键盘/读屏、四边、reduced-motion、zoom 等断言缺失 | 脚本审阅及重新运行 |
| RC10–11 | 验收 / SF01、UX09、UX10 | 缺少隔离原生路径及生产 CSP/原生首帧/DPI 完整证据 | 现有接线及验收存档核对 |

RC08 是本次新纳入的独立兼容修复，不将其追认成原计划遗漏。D5 安装/升级/卸载沿用用户此前后延决定，本计划不自动执行这些操作。

**非目标：** 增加来源、改 Codex 去重、调整四桶或时间口径、改最高完整候选计价、恢复内置价格、强制覆盖价、重写全站 UI、修改真实 agent 日志、清理真实缓存、升级框架、自动安装/发布。现有 CSP 不因测试不便而放宽脚本权限。

## 2. 不变量与方案取舍

1. 查询身份在跨进程重启后不复用；游标必须属于当前不可变快照、主筛选和下钻。时间戳、PID、进程内递增序号单独或简单拼接都不作为跨启动唯一性保证。
2. 首页未成功绑定当前 query 时，旧视图仅供 stale 展示；禁止使用其游标请求新会话。过期恢复由用户动作建立一个新批次，汇总与明细共享一次 query_begin；不自动无限重试，不拼接新旧页。
3. 读取失败是未知状态。设置的关闭动作、自动同步、来源配置由同一次成功读取初始化；失败前已加载值可保留但标旧，首次失败不得变成可操作默认值。
4. 读取晚于保存返回，不等于读取的数据更新。任一相关写入开始即作废先前读取的提交资格；成功不清掉保存期间新编辑的 dirty；失败保留草稿。卸载后请求不得写状态或发通知。
5. 查询预算覆盖保留的完整事件、字符串、价格与索引等对象，不由筛选命中行数代替。预算是查询子系统的保留数据记账上限，**不宣称整个进程 RSS 或采集临时峰值被同一数值严格限制**。淘汰但仍被读取者持有的对象继续占账。
6. 费用、候选、四类费率和完整性仍来自后端。Unknown、Fixed(0)、SameAsInput 保持区分；格式化只改变显示，不能引入第二套计价。
7. 单价说明名称由显式列 key 决定。费用/说明触发器支持真实焦点、原生键盘激活、Escape 和外部点击，描述关联指向存在内容；既有材质、公式实色衬底及金额可读性保留。
8. 测试所有 `SummaryOptions` 显式注入临时来源、cache_dir、pricing_index、价格文件/快照；设置和日志同样隔离。浏览器未知 IPC 显式失败，所有场景包括首帧场景都禁止外联。禁止修改 HOME/USERPROFILE 去赌 Windows 路径解析。
9. 分别报告“实现已验证”“自动验收覆盖完整”“原生验收完成”。截图、正常日志和全量绿灯均不能代替对应故障或交互断言；缺少环境的原生组合保持待验，不签整体完成。

采用的方案：RC01 用进程启动随机命名空间加计数；RC03 统一读取并采用读取代次/写入版本/行编辑版本；RC05 对每个会话完整保留对象保守记账（共享对象允许重复计账），将内存管理改动限制在查询子系统。

执行顺序：**RC01 → RC02 → RC03 → RC04 → RC05 → RC06 → RC07 → RC08 → RC09 → RC10 → RC11**。共享文件依次修改；不假定需要并行工作。RC01/02/05 开始前先在 `docs/stats-semantics.md` 写清相关身份、恢复和预算边界。

每项固定流程：①写具名回归；②运行并保存目标行为的失败输出；③最小实现；④同命令通过并检查相关回归；⑤按完整门禁验证后提交当前任务及记录。0 tests、导入错误、浏览器没启动均不算有效红灯。不能为通过而删掉断言、改成只检查属性存在或放宽 fixture。

## 3. 实施任务

### RC01 · 查询身份跨启动唯一

**文件：** 修改 `src/query.rs`、`docs/stats-semantics.md`；扩展 `tests/query_snapshot_contract.rs`，新增 `tests/query_process_restart.rs`；仅必要时修改根 `Cargo.toml`/`Cargo.lock`。

1. 在集成测试中用 `current_exe()` 启动同一个测试程序的独立子进程，环境只传测试阶段及临时根目录。子进程 A 建第一会话并将完整句柄、游标写到临时 JSON；父进程追加一个合成事件；子进程 B 以完全相同路径/筛选建立第一会话并提交 A 的游标。断言 `query_cursor_from_previous_process_is_rejected`：新旧 ID 不同、旧游标明确拒绝、B 自己的游标正常。不是 clear_registry 模拟重启。
2. 先确认基线失败具体为两次 q0-g0 与重放接受。增加同进程不同会话、错下钻、v1 游标拒绝回归；测试不要固定匹配新 ID 的内部文本形式。
3. 每次启动用系统随机源创建至少 128 位命名空间（例如 UUID v4），与原计数结合，generation 只用于诊断。随机初始化失败明确返回查询错误，不回退为时间/PID；计数溢出明确拒绝复用。采用成熟随机实现，显式声明实际用到的依赖，不依赖传递依赖。
4. 游标仍为不透明身份字符串。若仅 qid 内容变化，保留 v2 结构；旧 q0-g0 命名空间不可与新会话相等。只有线格式变化才升级版本并同时更新 Rust/TS/fixture，避免无必要重建事件缓存。
5. 验证：`cargo test --offline --test query_process_restart --test query_snapshot_contract`。两个子进程状态码和断言均须检查，临时输出不得泄漏真实日志。

**提交：** `fix(查询): 隔离跨进程查询身份与旧游标`。

### RC02 · 旧视图禁止续页，过期重试建立共享新批次

**文件：** `frontend/src/views/Dashboard.vue`、同名 `.test.ts`、`frontend/src/lib/viewSnapshot.ts`/`.test.ts`；必要时 `frontend/src/types.ts`、`src-tauri/src/commands.rs`；`docs/stats-semantics.md`。

1. 写 `restored_view_cannot_page_before_current_first_page_succeeds`：恢复 v6 同筛选旧视图，新 begin 成功但新首页拒绝；触发加载更多不得发出带旧 before 的 query_events。保留旧数据及 stale/失败状态。新首页成功后才能用它的末行游标分页。
2. 写 `expired_retry_starts_one_query_for_summary_and_first_page`：正常首页→分页返回 query_expired→点击错误条重试；严格断言 query_begin **新增一次**，两侧使用同一新 ID、before=null、旧页不追加、成功后错误清除。
3. 写 `failed_begin_can_be_retried`、`late_expired_page_cannot_replace_refreshed_results`：begin 拒绝后重试不能复用已失败 Promise；旧分页/旧 begin 晚到不覆盖新批次。用 deferred 按确定顺序释放响应。
4. 增加成功首页身份记录（query_id、pricing_revision、主/下钻身份、epoch）。磁盘恢复不赋予 live 身份；追加门槛同时校验当前句柄及已显示结果。恢复期间按钮禁用并解释原因，或提供明确“刷新后继续”，不能点击无反馈。
5. 统一错误恢复入口：begin 失败、会话过期/淘汰/外会话游标触发用户发起的新批次；普通当前会话请求失败允许局部重试。过期批次保留旧视图但禁止继续追加，汇总/首页重试协调一次 begin；新批次失败仍可再次重试。沿用现有 seq/epoch/disposed/保存同会话检查。
6. 错误识别集中封装，不散落中文字符串匹配；优先使用明确 code。若调整 Tauri 的 `Result<_, String>` 为结构化错误，完整迁移 TS/fixture 和旧错误展示路径；不把所有失败都判断成 query_expired。
7. 验证：`pnpm --dir frontend test -- src/views/Dashboard.test.ts src/lib/viewSnapshot.test.ts`；若调整错误 DTO，另跑壳测试与 typecheck。

**提交：** `fix(查询): 隔离恢复游标并修复过期重试`。

### RC03 · 设置统一读取、局部恢复与写后防回退

**文件：** `frontend/src/views/Settings.vue`、`Settings.test.ts`；必要时新增 `frontend/src/composables/settingsState.ts`；保持后端 `settings::update` 事务不变。

1. 写 `settings_initial_load_reads_once`：进入设置页只有一次 settings_get，其结果填充来源、自动同步、关闭动作。分别拒绝该读取、source_status、cache_stats、pricing_entries、autostart_status，其他成功区块可用，无 unhandled rejection。
2. 写 `settings_retry_restores_all_dependent_controls`：首次 settings_get 失败；关闭动作显示“读取失败/未知”且不能保存，来源保存与自动同步相应禁用；点击设置错误条重试，返回 quit/true/明确来源目录，**三处状态都恢复**，错误清除。不以“错误条消失”作为充分断言。
3. 替换现有伪晚到测试。`late_settings_response_does_not_overwrite_saved_value` 必须按“重试读取挂起 → 编辑 → 保存成功 → 释放旧读取”执行；最终仍为保存值。分别覆盖来源、关闭动作、自动同步；记录提交参数和界面值，不能让 mock 保存后自动返回新值绕过旧响应。
4. 增加 `edit_during_save_remains_dirty`：提交 A 后用户编辑 B，保存 A 返回不得清掉 B 的 dirty；后续读取不得覆盖 B。`late_read_after_unmount_is_ignored` 验证卸载守卫。失败保存保留草稿和错误；跨行保存不混用一个全局 savingSource 导致提前解锁。
5. 统一 `loadSettings`，删除三处分散 settings_get；每次读取捕获 readSeq 和 mutationEpoch，只有两者仍匹配且未卸载才提交。任一写操作开始即推进 mutationEpoch；来源每行 editRevision，成功仅清除提交时版本一致的 dirty。写失败也不得让旧读取获准覆盖用户草稿。
6. 各独立区块持有 loading/error/读代次，局部重试防重复但不丢必要刷新。保存成功后的 source_status 失败保留“已保存，状态刷新失败”及原因；读取重试不触发网络同步。保留已有数据与未保存草稿，未知不替换为 ask/false/default-enabled。
7. 验证：`pnpm --dir frontend test -- src/views/Settings.test.ts`、`pnpm --dir frontend typecheck`。测试必须实际覆盖三个相关控件的成功、失败、竞态，而不只是输入框一项。

**提交：** `fix(设置): 统一读取恢复并阻止写后旧响应回退`。

### RC04 · 同步错误有真实重试动作，状态刷新不丢失

**文件：** `frontend/src/components/PricingStatusBanner.vue`/`.test.ts`、`frontend/src/views/Settings.vue`/`.test.ts`。

1. 写 `partial_sync_failure_keeps_sync_retry_action`：主源成功、补充源失败，status.needsSync=false 且有可用价格；仍显示具体失败及同步重试按钮。真实点击按钮后再次调用同步，成功才清理对应错误。
2. 写 `sync_failure_without_status_does_not_claim_partial_success`、`status_retry_never_starts_sync`、`repeated_sync_activation_is_single_flight`：状态未知时不能统一说“部分成功”；两类错误/两种按钮 pending 分离，重复触发只发一次请求。
3. 写 `status_change_during_pending_read_is_not_lost`：状态读取挂起时同步完成或触发 pricing-status-changed，释放旧读取后最终必须采用同步后的状态。当前防重复直接 return 不能丢刷新意图；采用 pending 标记结束后补读一次或代次替换，不允许旧结果盖新。
4. 在错误条添加可操作“重试同步”，复用 syncNow；状态失败的重试仅 refreshStatus。设置页 syncPricing 同样加函数级防重复，列表刷新失败保留旧价格并归类为读取失败；卸载时使在途请求失效并移除监听。
5. 验证：`pnpm --dir frontend test -- src/components/PricingStatusBanner.test.ts src/views/Settings.test.ts`。保留 SF10 的有效候选状态语义，available 不从原始条数推断。

**提交：** `fix(同步): 补齐错误重试与状态刷新协调`。

### RC05 · 查询按完整保留数据记账与回收

**文件：** `src/query.rs`、`src/report.rs`、`src/pricing.rs`、必要时 `src/model.rs`；新增 `tests/query_memory_budget.rs`；扩展 `tests/query_snapshot_contract.rs`、`docs/stats-semantics.md`。

1. 先建立可注入小额预算的注册表测试，避免分配数百 MB。`empty_range_charges_retained_collection`：零筛选行仍有完整事件/价格开销；`long_strings_increase_snapshot_charge`：事件数相同但长模型/项目字符串占更多预算；`pricing_and_sort_indices_are_accounted`：价格规则和行索引也计入。
2. `sequential_queries_cannot_bypass_byte_budget`：依次建多会话触发 LRU 淘汰，旧 ID 明确失效；单个快照超预算返回可读错误且不截断事件。再测 checked 算术溢出拒绝、失败构建不泄漏额度。
3. 按 ownership 和 capacity 建保守 `retained_bytes`：事件 Vec 分配、事件内四个 String、SourceReport/诊断字符串、Pricing 的候选/嵌套规则/索引、SnapshotRow Vec、身份/时区元数据；计算全用 checked。`size_of::<UsageEvent>() * rows.len()`、序列化 JSON 长度和固定“每行约 200 字节”均不合格。记录容器/分配开销的保守系数及不覆盖的进程级开销，不再声称 300 MB RSS 上界。
4. 使用具名 `MAX_RETAINED_QUERY_BYTES`，初值 256 MiB；保留 8 会话和空闲 TTL。测试注入更小阈值。每会话计完整可达保留图，共享价格/collection 允许重复记账，换取简单且保守的上限；不要因 Arc 就漏账。若需优化重复记账，另有所有权证明和测试后再做。
5. 额度由随 QuerySnapshot 最后一个 Arc 释放的 reservation 持有，注册表锁内原子准入。淘汰但读取仍在进行时不得提前退还；`evicted_but_borrowed_snapshot_stays_charged`、`concurrent_admission_respects_budget` 用 channel 暂停读取确定性交错，不靠 sleep。无法腾出额度时明确失败，不能超预算接受。
6. 修正单飞结果的所有权：若仍 `Arc::new(collection.clone())` 深拷贝事件，各实际保留副本都必须计账；优先让 FlightCell 直接共享 `Arc<CollectionSnapshot>`，消除无必要深拷贝。保持错误/panic 的 RAII 释放及跟随者可重试测试。
7. 建表/解析期间临时峰值单独量测记录，不伪装已由保留预算覆盖；不在此任务建立通用 allocator 或改整个采集架构。准入失败前不得破坏当前已显示查询；错误给用户缩小范围/稍后重试的真实建议，若缩范围不减少保留量则不能这样提示。
8. 验证：`cargo test --offline --test query_memory_budget --test query_snapshot_contract`、`cargo test --offline collect --lib`。记录测试中预算、实际记账、淘汰后/借用释放后的值，禁止只断言会话数 <=8。

**提交：** `fix(查询): 按完整保留对象约束会话预算`。

### RC06 · 费用与说明浮层的语义及完整键盘交互

**文件：** `frontend/src/components/CostBreakdownTooltip.vue`、`HelpTooltip.vue`、`EventTable.vue`/`.test.ts`、`frontend/src/views/Settings.vue`/`Settings.tooltip.test.ts`；必要时新增 `HelpTooltip.test.ts`。

1. 真实组件测试 `each_price_column_exposes_its_own_label`：输入、输出、缓存写、缓存命中四列分别核对完整名称。priceCell 改为接收 `keyof` 中明确的四桶 key，以共享 tokenDisplay 取标签和取值，删除函数对象比较。
2. `cost_tooltip_reopens_with_enter_and_space_after_escape`：实际 focus 打开、Escape 关闭、保持焦点、Enter 打开、再关闭、Space 打开。`help_tooltip_closes_on_outside_click`：点击固定→移开指针→点击外部→关闭，描述关联清除、触发器仍可再次激活。
3. 优先使用 `button type=button`，局部重置外观保留字号/数值对齐/焦点环；利用原生 Enter/Space 产生一次 click，删除会重复 toggle 的手写键盘处理。mouse/focus/pinned 继续分别维护，Escape 后不因保留焦点立即重开；外部点击清除相关状态。不得把 hover 支持当成键盘支持。
4. 使用真实 NTooltip，不打桩内容；校验 aria-describedby 指向当前存在的节点、描述实际含公式/来源，多个行 ID 不重复；部分计价名称含状态，未知仍明确未知。费用触发器与设置帮助各自覆盖，不以其中一个组件代替全部。
5. Vitest 中焦点/点击与原生浏览器行为不完全等价，RC09 必须再次用 page.keyboard/page.mouse 验证。原生无障碍点击持久性在 RC11 独立记录，不根据此前失败推断所有鼠标点击都坏。
6. 验证：`pnpm --dir frontend test -- src/components/EventTable.test.ts src/components/SummaryCards.tooltip.test.ts src/views/Settings.tooltip.test.ts`；若新增 HelpTooltip 测试同时运行。

**提交：** `fix(无障碍): 修正单价名称与浮层激活关闭行为`。

### RC07 · 单价格式保留有效精度与单位

**文件：** `frontend/src/lib/formatMoney.ts`/`.test.ts`、`tieredPrice.ts`/`.test.ts`、`costBreakdown.ts`/`.test.ts`、`tokenDisplay.ts`；相关设置/明细调用点；`DESIGN.md` 金额说明。

1. 写 table-driven `unit_rates_keep_significant_precision`：1.234567→$1.234567、12.3456789→$12.3456789、0.123456789→$0.123456789、1e-8 明确非零、0→$0.00、null→未知；NaN/Infinity 不显示合法价格。
2. 单价规则固定为保留 JS number 的最短可往返十进制有效表示（以 String(value) 为基础），不额外强制两/四/六位舍入；极小/极大值允许清晰科学记数法。0 特殊格式保留，美元符号只添加一次。request/summary 保留现有展示策略；公式结果与请求总额仍共用 request。
3. `scenario` 必须进入实际分支。所有单价调用用 unit，解释量纲的列头/说明/公式明确 `USD / 1M token` 或等价的“每百万 token”；单位不能仅存在于注释或测试名。
4. `unit_precision_survives_settings_and_breakdown` 验证真实设置单价与公式中的费率均展示完整 1.234567；金额结果仍来自 DTO，不用显示字符串复算。SameAsInput 在单价页保留语义；具体请求的解析价由 breakdown 提供。
5. 同步修复仍散落的“缓存读”产品显示词，统一读取 tokenDisplay；技术 key、原始 API 字段不机械替换。测试对实际四列/公式输出逐项验证，不只比较元数据常量。
6. 验证：`pnpm --dir frontend test -- src/lib/formatMoney.test.ts src/lib/tieredPrice.test.ts src/lib/costBreakdown.test.ts src/components/EventTable.test.ts src/views/Settings.tooltip.test.ts`。

**提交：** `fix(展示): 保留单价精度并明确单位与分项名称`。

### RC08 · OpenRouter 数字价格兼容（独立补充修复）

**文件：** `src/openrouter.rs`；新增合成 `tests/fixtures/openrouter/numeric-prices.json`；必要时扩展 `tests/pricing_view_contract.rs`。不保存真实 API 响应。

1. `openrouter_accepts_numeric_and_string_prices` 经真实 serde `ApiResponse` → `sync_with` → 临时快照链路，混合四类基础价及 overrides 的字符串、整数0、正小数，原代码必须在反序列化处失败。
2. 只对价格字段引入局部 serde 输入类型/反序列化器：number|string|null/缺失。数字与数字字符串复用同一有限非负校验；缺失/null/现有空串语义仍未知，显式0仍免费。不把 id/name/时间字符串宽松转换成任意 JSON，也不对整份文本替换 0。
3. 保留整模型拒绝非法显式单价的现有策略，包括 overrides；负数、字符串 NaN/Infinity、坏数字串必须带模型/分项诊断，不静默改0。布尔/数组/对象作为非法价格报告，不能因宽松匹配变成未知合法值；根响应结构损坏依旧明确失败。
4. `numeric_zero_is_preserved_in_snapshot_and_pricing_view` 对基础与 override 的0逐字段验证；`failed_sync_preserves_previous_snapshot` 验证拉取/响应级解析失败不覆盖已有文件。同步仍原子写、同 provider 互斥；离线统计不联网。保持现有快照数字格式，无语义需要不升级索引/事件缓存。
5. 验证：`cargo test --offline openrouter --lib`、`cargo test --offline --test pricing_view_contract`。全部用注入 fetch，真实联网同步不作为自动测试前提。

**提交：** `fix(价格同步): 接受 OpenRouter 数字与字符串单价`。

### RC09 · 浏览器矩阵补齐可失败的业务与交互断言

**文件：** `frontend/scripts/check-ui-contracts.mjs`、`frontend/scripts/fixtures/ui-contracts.mjs`、`frontend/scripts/check-chart-tooltip-security.mjs`、必要时 `frontend/scripts/check-app-scroll.mjs`；产物 `qa-artifacts/recheck-remediation/`。

1. fixture 改为真正维护多 query_id 与各自冻结参数的会话表，游标/过期/页数可控；不使用一个常量 ID 和 sessionBy 冒充并发会话。所有新旧 command 显式处理；未列 command 失败。unknown/partial/empty 同时设置汇总与明细 DTO，不能只有分组变化而明细固定正常。
2. 用以下具名契约作为覆盖清单，缺少场景或关键断言就使 verify 失败；场景数本身不作为完成标准。

| 契约 | 必须执行的动作/断言 |
| --- | --- |
| `empty_unknown_partial_have_correct_content` | 断言空状态、未知≠0、部分费用/未知桶与 fixture 一致；保留已有错误诊断 |
| `settings_failure_retry_restores_all_controls` | 默认矩阵包含失败 fixture；真实按钮触发失败→重试→恢复来源/关闭动作/自动同步 |
| `saved_settings_survive_delayed_read` | 浏览器内重演 RC03 deferred 保存竞态，提交参数及最终输入均正确 |
| `partial_sync_exposes_actionable_retry` | 主源可用后仍能点击补充失败重试；状态重试不发网络同步 IPC |
| `expired_query_recovers_as_one_new_batch` | 真实加载更多/重试按钮；一次新 begin、汇总/首页同 ID、没有旧 before |
| `keyboard_triggers_expose_value_and_description` | Tab/Shift+Tab、方向键、Enter、Space、Escape；可访问名称逐列正确，description 有实际内容，焦点位置正确 |
| `popover_stays_visible_at_four_viewport_edges` | 双主题/两窗口尺寸，在四边触发真实浮层；量测外框完整在视口、内容可读/可滚动、关闭恢复正确 |
| `date_outer_surface_matches_contract` | 同时量 `.range-panel` 与 Naive 实际外壳祖先；一层有效 elevated 材质，防双层玻璃/实色外壳掩盖 |
| `reduced_motion_preserves_controls_and_layout` | emulateMedia(reducedMotion=reduce)，实际主题/分段/浮层操作；计算动画/过渡符合规范且焦点/选中/尺寸不失效 |
| `chart_scroll_and_zoom_survive_supported_updates` | 多类别实际滚动可见完整类别；用真实图表交互改变 zoom、同维刷新保持、切维清理，摘要数值与数据一致 |
| `real_app_scroll_keeps_navigation_visible` | 真实 App 内部 scroller 的 scrollTop 增大、导航位置稳定、相关通知可达；合成 CSS 脚本仅辅助 |
| `prepaint_theme_has_correct_canvas` | 主模块明确未执行时，CSS已可绘制的 data-theme、实际背景/画布颜色符合偏好；此时截图，释放模块后另存终态 |

3. 交互用 Playwright locator/keyboard/mouse，不用 evaluate 调组件方法绕过事件；evaluate 只量测、设置确定性环境或读真实 ECharts 状态。浏览器无障碍语义与原生读屏是不同证据，分别记录。
4. 首帧循环也注入完整 IPC 和外联拦截，处理 storage 不可用；截图不能只在释放主模块后拍再命名 prepaint。每场景保存实际断言列表、输入 fixture、提交 hash、浏览器版本、结果及截图路径。
5. 如果外层日期/缩放等新增断言先红，先确认是否真产品偏差；只修对应局部样式并记录，不能将断言移回内层使其“通过”。保留 SF01 无 CSP 页面上的真实 ECharts 恶意标签测试，不仅检查静态 formatter。
6. 验证命令见第4节。至少通过既有浅/深×1280×820/980×620基础组合及新增失败、延迟、边界交互场景。关键路径为 baseline 漏洞添加红→绿证据；不能以空数组 `.every()` 或没有命中目标元素算通过。

**提交：** `test(验收): 覆盖真实错误恢复与交互边界`；若发现额外产品修复，单独 fix 提交后再收敛矩阵。

### RC10 · 为原生验收提供确定的隔离入口

**文件：** 根/壳 `Cargo.toml`，`src/report.rs`、`src/settings.rs`、`src/logging.rs`、`src-tauri/src/lib.rs`/`commands.rs`；新增 `src-tauri/src/acceptance.rs`、`scripts/prepare-native-acceptance.ps1`、`docs/plans/active/2026-10-08-native-recheck-qa.md`。

1. 先列出原生路径清单并测试：两来源根、cache.db、pricing.toml、双源价格快照、pricing-index、settings.toml/迁移备份、view cache、日志、窗口状态、WebView 用户数据。不能只改 cache_dir 而其他项仍落真实目录。
2. 新增非默认 cargo feature `acceptance`，壳显式透传根库同名 feature；仅该构建识别进程级 `TOKENSCOPE_ACCEPTANCE_ROOT`。启动最早阶段解析并验证它；缺失/非法路径拒绝验收启动，绝不回退真实 HOME。普通构建行为不变；测试钩子不通过前端 IPC 暴露。
3. 复用现有 data_dir/显式 path 注入，设置/日志/commands/自动同步等全部消费同一冻结根。来源设置由准备脚本写成临时根内两个明确目录，即使目录不存在也不能 fallback 默认真实来源；读取设置失败的验收场景仍使用冻结的合成来源配置。WebView/profile/窗口状态单独指定到该根，避免真实 localStorage 干扰首帧与恢复。
4. 准备脚本用 UTF-8 生成合成日志、有效价格及 settings（price_auto_sync=false），记录绝对路径清单与初始散列；脚本不覆盖非本次新建目录，不改系统 HOME，不触发真实自启/安装。参数限制在本次临时工作目录；任何必要清理先核验解析后的路径。
5. `native_acceptance_paths_are_hermetic`、`acceptance_mode_never_falls_back_to_real_sources`、`acceptance_without_root_fails_before_collection`：隔离子进程验证所有读写目标；基础启动再实际检查产物只在隔离根。根库普通单测仍必须显式传 paths，不把全局环境变量当测试密闭性的替代品。
6. 该构建使用 release 与原生产前端/CSP 配置，不能使用 devUrl/devCsp；记录普通 build 与 acceptance build 的前端 assets/CSP 一致性。它只能证明此配置的原生契约，不冒充已验安装包或发布签字。若隔离入口尚未验证，就保持 RC11 待验，不能退回真实 1.2GB 日志完成签字。
7. 验证：`cargo test --offline --features acceptance acceptance`；`cargo test --manifest-path src-tauri/Cargo.toml --offline --features acceptance acceptance`；构建/启动命令见第4节。新增 QA 文档逐项留实际命令和产物字段，不预填通过。

**提交：** `test(原生验收): 增加隔离路径与合成数据启动入口`。

### RC11 · 原生生产验收、证据纠正与状态回写

**文件：** RC10 的 native QA 文档、本计划、原修复计划、`docs/plans/README.md`、`docs/plans/active/2026-10-06-design-system-visual-qa.md`；D5 清单仅追加相关状态说明，不代签后延项目。

1. 每轮记录 commit、feature、产物 SHA256、WebView2/Windows 版本、系统 DPI、逻辑/物理窗口尺寸、主题偏好、隔离根及原生 IPC 日志。100%/125%/150% 是 Windows 实际缩放；deviceScaleFactor 不算替代。
2. 浅/深各三档 DPI：真实组件键盘/无障碍名称、费用内容打开与四边、日期外壳、长路径、图表、通知/重试可操作、导航滚动。费用 button 与同名 row 分开计数；无障碍工具输出截断时只记录工具限制，不称 WebView2 固定399上限。
3. 原生冷启动首帧单独捕获到首个窗口画面（连续帧/录像可用），结合实际主题与背景；常规加载完成截图不能证明首帧。浏览器预绘制通过记录独立保留。
4. 生产 CSP：确认实际加载的策略，正常脚本、IPC、Naive/ECharts、主题/日期可用；在页面实际文档中插入无害 script 哨兵，验证禁止内联脚本/外源脚本执行，记录 securitypolicyviolation/原生日志。调试工具自身的脚本求值不作为页面 CSP 是否生效的证明。无 CSP 的图表安全测试与此项分别通过；不放宽策略使测试通过。
5. 故障原生验证：临时 logs 路径为普通文件，应用继续显示窗口且有启动降级通知；临时 pricing.toml 共享锁→降级提示→释放后恢复；合成日志/价格变化时旧 query 稳定、新 query 更新；过期错误可见并通过其按钮恢复。正常日志初始化/价格加载耗时不能代替故障验收。
6. 关窗三态最小回归：显示弹窗→Escape→弹窗消失且进程存活，分别保存前后截图及临时 settings 字节；取消不落 close_action。记忆/最小化/退出只在隔离实例验证，截图必须对应动作之后。
7. 环境缺少的 DPI/首帧/CSP 组合明确“待验+缺失证据”，不要转写“全部通过”。D5 安装/升级/卸载保持此前后延，不自动安装；原生合成验收不等于真实数据性能或安装包验收。
8. 回写原计划：关联 RC01–11 的提交、测试名/数量、有效红→绿、产物位置与剩余项；纠正“28触发器”“浅色截图”“24场景=完整矩阵”等扩大化结论，历史文件不删。全部本轮必要验收完成才归档本计划并修正链接；若实现/自动化已结束而原生未闭合，保留 active 和未勾选项，最终状态明确写此限制。

**验证：** 第4节完整自动门禁及 QA 的 `native_production_csp_enforced`、`native_cold_start_matches_theme`、`native_dpi_layout_and_keyboard`、`native_fault_recovery_is_visible`、`native_close_cancel_preserves_settings` 五类具名验收记录。

**提交：** `docs(验收): 回写复核修复证据与剩余限制`。

## 4. 验证命令与证据规则

自动测试只运行合成数据；**不运行 ignored 真实性能测试**。每条命令都检查退出码；PowerShell 多命令不会自动因前一条非零停止，执行者必须逐条检查或显式 fail-fast。

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline --quiet
cargo test --release --offline --test token_overflow_contract
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --offline --quiet
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
```

RC10 新增 feature 后另跑对应 feature 的 fmt/clippy/test；只执行普通构建门禁不能证明隔离分支正确。新增依赖需联网时遵循仓库代理约定；不将 --offline 失败当作测试已通过。

浏览器自动验收：预览服务与安全测试 dev 服务使用空闲端口，记录本次 PID，只停止自己启动的进程；端口占用换端口并同步 URL。

```powershell
# 独立终端：生产前端（先 build）
pnpm --dir frontend exec vite preview --host 127.0.0.1 --port 1437 --strictPort
# 验证终端
node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 --phase verify --output qa-artifacts/recheck-remediation/after
node frontend/scripts/check-app-scroll.mjs
# 另一个独立终端：安全测试页不在生产构建中，需 dev 服务
pnpm --dir frontend exec vite --host 127.0.0.1 --port 1438 --strictPort
# 验证终端：无 CSP 的真实 ECharts 输出边界
node frontend/scripts/check-chart-tooltip-security.mjs --url http://127.0.0.1:1438
```

RC10 要交付的原生入口命令契约（这些命令依赖该任务实现，当前不可声称已经可用）：

```powershell
# 根目录；新脚本创建唯一临时目录，输出路径清单
$acceptanceRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('tokenscope-native-' + [guid]::NewGuid().ToString('N'))
.\scripts\prepare-native-acceptance.ps1 -Root $acceptanceRoot
$env:TOKENSCOPE_ACCEPTANCE_ROOT = $acceptanceRoot
.\frontend\node_modules\.bin\tauri build --features acceptance --no-bundle
# 构建成功后启动这个 release 实例，记录 PID；不调用 tauri dev
Start-Process -FilePath '.\src-tauri\target\release\tokenscope.exe' -PassThru
# 验收结束后移除本进程设置；仅关闭本次记录的应用进程
Remove-Item Env:TOKENSCOPE_ACCEPTANCE_ROOT
```

可视原生应用需要显示窗口；脚本里的后台服务/helpers 必须隐藏窗口。原生 QA 的故障设置仅作用于隔离根；安装/自启等系统动作不包含在上述命令中。

每条执行记录必须包含：任务号、实际提交 hash、失败原因与红灯用例、绿灯命令/数量、当前代码及构建版本、证据路径、尚未运行项。新增测试至少选择代表性目标修复做一次受控撤回验证，确认它会再次因目标行为失败；不要修改其他未提交工作。

提交遵循 CLAUDE.md：验证→暂存具体文件→commit→push 当前分支，严格串行，不跳 hook。为减少验收产物噪声只提交源码、fixture 与文本记录；截图/JSON 放 qa-artifacts 并维持 gitignore，计划正文保留足够复现步骤。

## 5. 完成定义与执行账

- [x] RC01 两个真实子进程旧游标拒绝，新会话自身分页正常。
- [x] RC02 旧视图无 live 分页资格，过期/失败重试可恢复且只建一个共享批次。
- [x] RC03 一次 settings_get 恢复所有依赖项；真实 deferred 写后回退/保存期间编辑/卸载均受保护。
- [x] RC04 同步与状态错误各有有效重试；pending 刷新事件不丢失。
- [x] RC05 完整保留对象计账、并发准入/借用/回收边界测试通过，内存声明与保证一致。
- [ ] RC06 真实组件及浏览器完整键盘、列名称、描述、外部关闭通过。
- [x] RC07 单价精度和单位在真实调用点保留，request/summary/三态计价回归不变。
- [x] RC08 OpenRouter 基础/override 的数字与字符串兼容，非法值策略及原子快照保留。
- [ ] RC09 所有必需具名契约有实际断言并通过，失败恢复及首帧场景无 IPC/网络漏口。
- [ ] RC10 release 原生隔离入口已验证，所有读写和 WebView 状态在临时根。
- [ ] RC11 生产 CSP、原生首帧/背景、六个主题×DPI组合及故障恢复有足够证据；未验项明确留下。
- [ ] 原计划和索引状态与真实完成度一致；本轮必要项未验不归档、不整体宣告完成；D5 后延单列。

| 任务 | 状态 | 红灯证据 | 绿灯命令/数量 | 提交 | 剩余限制 |
| --- | --- | --- | --- | --- | --- |
| RC01 | 已完成（实现+自动化）；本轮复核确认逐项达标 | `qa-artifacts/recheck-remediation/rc01-red.txt`：两个真实子进程都生成 `q0-g0`，B 接受 A 的旧游标（`子进程 B 失败：status=Some(3)`） | `cargo test --offline --test query_process_restart --test query_snapshot_contract` → 3 passed（+1 ignored 子进程入口）+ 6 passed | `3d3ce2c`、`32ff2ba` | 偏差：同进程不同会话/错下钻/v1 游标回归落在 `tests/query_process_restart.rs`，plan 点名的 `query_snapshot_contract.rs` 未另扩（同一断言集已覆盖，不重复搬移）；子进程继承宿主环境但剥离 `TOKENSCOPE_*` 并注入临时根（`env_clear` 在 Windows 上破坏路径解析，见 `32ff2ba`） |
| RC02 | 已完成（实现+自动化）；本轮复核确认逐项达标 | `qa-artifacts/recheck-remediation/rc02-red.txt`：恢复视图仍用旧 `before` 发起 `query_events`（`Dashboard.vue:318 sameMainIdentity(live.filters…)` 处崩溃/续页） | `pnpm --dir frontend test -- src/views/Dashboard.test.ts src/lib/viewSnapshot.test.ts` → Dashboard 37 passed、viewSnapshot 4 passed | `e5c9285` | `LiveFirstPage` 的 `pricingRevision/epoch` 仅作诊断记录，分页门槛按 plan 要求校验“当前句柄 + 已显示结果 + 主/下钻身份”；错误 DTO 未改结构化，故 `types.ts`/`commands.rs` 无需迁移 |
| RC03 | 已完成（实现+自动化）；本轮补齐复核缺口 | `qa-artifacts/recheck-remediation/rc03-red.txt` + `rc03-gap-red.txt`：写入不作废在途读取时晚到读取把已保存的关闭动作改回旧值；丢弃补读意图时刷新不落地；探针自检证明只挂 `window` 的未处理拒绝断言是空断言 | `pnpm --dir frontend test -- src/views/Settings.test.ts` → 43 passed（本轮新增/改写 7 个具名用例）；`pnpm --dir frontend typecheck`、`format:check` 全绿；全量 273 passed | `d2a3a8d` + 本提交 | 补齐内容：五个区块统一 `runBlockRead`（读代次+在途标记+补读+卸载守卫）、来源保存写入门槛改为“未知且无在途重试”、刷新失败保留真实原因、deferred 竞态按来源/关闭动作/自动同步分别覆盖并断言提交参数与界面值、跨行保存独立解锁、重试不触发同步 |
| RC04 | 已完成（实现+自动化）；本轮补齐复核缺口 | `qa-artifacts/recheck-remediation/rc04-red.txt` + `rc04-gap-red.txt`：卸载不失效在途请求/不移除监听时事件仍触发读取 | `pnpm --dir frontend test -- src/components/PricingStatusBanner.test.ts src/views/Settings.test.ts` → 17 passed + 43 passed | `bbeefe9` + 本提交 | 补齐内容：恢复 plan 具名 `status_retry_never_starts_sync`、`sync_failure_without_status_does_not_claim_partial_success`，新增两类按钮 pending 分离与卸载失效/移除监听用例，同步后列表刷新失败断言旧价格保留 |
| RC05 | 已完成（实现+自动化）；本轮补齐复核缺口 | `qa-artifacts/recheck-remediation/rc05-red.txt`（旧实现按 `rows.len()` 记账）+ `rc05-gap-red.txt`：受控撤回[A]超预算建议改回"请缩小时间范围" → `budget_errors_give_only_effective_advice` 红；撤回[B]叶子助手改回 saturating → `collect_retained_leaf_helpers_reject_overflow_not_saturate` 红 | `cargo test --offline --test query_memory_budget --test query_snapshot_contract` → 10 passed + 7 passed；`cargo test --offline collect --lib` → 7 passed；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --offline` → 全绿 | `767b5ed` + 本提交 | 补齐内容：准入错误改为只给真正有效的建议（减少并发会话/等 TTL/停用来源目录）并说明缩小范围无效、字节量按 B/KiB/MiB 精确显示；叶子字节助手全部受检乘加（夹取会把超大保留量报成预算放得下的假小值）；补计 `pricing_revision` 克隆与 `TimeZone`；借用/并发改用 channel 确定性交错并新增 `admission_failure_keeps_displayed_session_usable`、`budget_errors_give_only_effective_advice`、`build_peak_is_measured_separately_from_retained_charge`；`query_snapshot_contract.rs` 补 `shared_collection_snapshots_charge_every_session`。临时峰值已实测记录（400 事件：记账 102,390 B，峰值 +230,651 B，仍存活 +77,382 B）。**仍不宣称**全进程 RSS 或采集临时峰值受 256 MiB 限制；准入在额度锁内原子完成（注册表锁只管身份/容量，淘汰在锁外 Drop） |
| RC06 | 已完成（实现+自动化）；本轮补齐复核缺口；浏览器/原生复验归 RC09、RC11 | `qa-artifacts/recheck-remediation/rc06-red.txt`（四列同名、Escape 后不重开、外部点击不关闭）+ `rc06-gap-red.txt`：撤回费用浮层的文档点击监听 → `cost_tooltip_closes_on_outside_click` 红；SummaryCards 触发器退回 `div role=button` + 手写 keydown → 原生 button 断言与“单独 keydown 不得 toggle”同时红（3 个用例） | `pnpm --dir frontend test -- src/components/EventTable.test.ts src/components/SummaryCards.tooltip.test.ts src/views/Settings.tooltip.test.ts` → 22 + 5 + 7 passed；`typecheck`、`format:check` 全绿；全量 278 passed | `a711f95` + 本提交 | 补齐内容：费用浮层补上固定态外部点击关闭（原来只依赖 `trigger="manual"` 下不可靠的 `clickoutside`）；SummaryCards 命中率触发器由 `div role=button`+手写 Enter/Space 改为原生 `button`（消除重复 toggle）并支持外部关闭；四类分项显示词改由 `tokenDisplay` 单一来源；新增多行同挂时 `aria-describedby` 互不相同且内容不串用、部分计价状态可见的断言。浏览器侧 `page.keyboard`/`page.mouse` 复验与原生读屏分别在 RC09/RC11 |
| RC07 | 已完成（实现+自动化）；本轮补齐复核缺口 | `qa-artifacts/recheck-remediation/rc07-red.txt`（`unit 1.234567: expected '$1.23' to be '$1.234567'`）+ 本轮 `rc06-gap-red.txt` 同源用例 | `pnpm --dir frontend test -- src/lib/formatMoney.test.ts src/lib/tieredPrice.test.ts src/lib/costBreakdown.test.ts src/components/EventTable.test.ts src/views/Settings.tooltip.test.ts` → 7 + 3 + 14 + 22 + 7 passed | `c1ba8fc` + 本提交 | 补齐内容：公式里的量纲后缀改自 `tokenDisplay` 共享常量（不再硬编码 `/M`）；费用浮层自身写出「单价量纲：USD / 1M token（每百万 token）」；新增真实渲染级精度断言（`unit_precision_survives_settings_and_breakdown` 直接读单元格文本核对 `$1.234567`/`$12.3456789`/`$0.123456789` 与 SameAsInput 语义），以及分项名/量纲来自单一来源的断言；DESIGN.md 金额说明同步 |
| RC08 | 已完成（实现+自动化；原生验收归 RC11） | `qa-artifacts/recheck-remediation/rc08-red.txt`：受控撤回 `parse_price` 的 Number 分支后 `openrouter_accepts_numeric_and_string_prices` 与 `numeric_zero_is_preserved_in_snapshot_and_pricing_view` 双双变红（0 条导入/取不到条目）。修复前真实失败点在 serde 层，由常驻回归 `test_rc08_old_string_only_schema_rejects_numeric_fixture` 在同一 fixture 上永久断言旧 `Option<String>` schema 反序列化失败 | `cargo test --offline openrouter --lib` → 19 passed；`cargo test --offline --test pricing_view_contract` → 5 passed；`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --offline -- -D warnings`、`cargo test --workspace --offline` → 全绿（exit 0） | 本提交（`fix(价格同步): 接受 OpenRouter 数字与字符串单价`） | 只用合成 fixture，未联网复验真实 API；`id`/`name`/`utc_start` 与根结构仍严格失败；快照数字格式与索引/事件缓存版本未变 |
| RC09 | 待执行 | — | — | — | 不能复用24场景总数作覆盖证明 |
| RC10 | 待执行 | — | — | — | 原生前置路径隔离 |
| RC11 | 待执行 | — | — | — | DPI/首帧/CSP待验，D5安装后延 |
