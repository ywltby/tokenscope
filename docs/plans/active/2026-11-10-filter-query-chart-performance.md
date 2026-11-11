# 筛选等待、图表动画与重复纠错备份修复计划

> 执行模型：使用 `executing-plans`、`systematic-debugging`、`test-driven-development` 和 `verification-before-completion`，逐项先红后绿。
> 状态：**已发布（v0.7.5 预发布，2026-11-11）**。F01/F02/F03 已随 PR #61（合并提交 `aa134a1`）合入 main；验收期与独立复核的四处调度补修一并交付。Release 全部 19 个作业通过：gate、plan-upgrades、build、install-test、12 组单跳升级矩阵（6 个起点 × present/deleted）、`upgrade-chain` present/deleted 两条连续链、publish；资产身份与哈希已核对（安装包 SHA256 `b8be9c1c…648b9`，标签/提交/元数据一致）。未在本机安装；AC10 部分验证与 about:blank 根因仍未闭合，见下文与整合计划。

**Goal:** 消除无必要的整库纠错备份与图表动画截断，减少时间/应用/分组切换的查询等待，同时保持现有统计、计价与冻结分页语义。

**Architecture:** 保留现有历史库、查询会话与 ECharts 实例。备份预判与事务内纠错使用相同的身份判定；图表在布局稳定后合并处理尺寸与数据；筛选走已提交历史的读取路径，优化实际重复扫描和匹配开销，不建立另一套统计系统。

**Tech Stack:** Rust / SQLite / Tauri 2 / Vue 3 / ECharts 6.1 / Vitest / Playwright。

## 1. 基线、证据与边界

- 基线：v0.7.4，main `46f0dc758b1a882ce4d42d7e7e5479c56529c60e`。执行前重新 fetch；若 main 前进，先审查差异，不覆盖其他会话内容。
- 本机安装版 0.7.4，2026-11-10 15:26 启动。15:27 四次分组切换，从价格加载日志到汇总完成约 **1,326–1,409 ms**；采集 **69–88 ms**、汇总聚合 **833–893 ms**。差额含快照构建等阶段，尚无逐阶段实测，不能全部归为数据库锁或 UI 主线程阻塞。
- `useUsageQuery.ts` 等汇总与明细同批次齐备才发布，期间保留旧图。这个一致性约束必须保留，等待反馈需要改善。
- `TrendChart.vue` 的高度变化触发 ResizeObserver，再调用默认动画时长为 0 的 `chart.resize()`。同一真实组件的隔离浏览器探针已复现：数据动画刚开始即跳到终态；同高度更新对照可正常动画。证据：gitignored `qa-artifacts/chart-filter-analysis/{server.mjs,probe.py,animation-observations.json}`。这些研究文件不作为正式回归的运行依赖。
- `history.rs::count_pending_corrections` 把 Codex 普通 native 来源命中已有事件也算成纠错，导致 `VACUUM INTO`。15:37 元数据观察：本次启动已产生 **58 份 / 4,301,222,912 字节**纠错备份。日志中的“本轮改写 N 组”是当前预判计数，不能直接当作实际改写数。
- 本轮只读日志/文件元数据与源码，浏览器测试使用合成数据。没有为分析重扫真实来源、导入 CCS、修改真实 history.db 或删除备份。
- v0.7.4 的启动取证已发布；about:blank 根因仍开放。该问题、`/btw`、同步服务超时、旧人工验收不并入本批。

## 2. 不变量与方案选择

1. 四桶、请求数、费用、未知/估算标志、Claude/Codex 去重、CCS A/B/C 来源选择逐项不变。来源已删除的已保存用量必须保留。
2. 真实弱身份升级、重复事实合并、镜像来源转移及弱别名接管，仍须先有可恢复备份，事务内失败完整回滚。不得通过关备份、吞错误或跳过纠错来提速。
3. 正常新增、普通终值更新和幂等重放不新增特殊纠错备份。一次确需纠错的批次最多生成一份，仍保留既有恢复能力。
4. 新筛选建立新查询会话；同屏汇总/明细绑定同一个 query_id、价格修订和冻结边界。旧分页不变、新查询可见已提交更新；过期游标明确拒绝。
5. 不仅以 generation 作为复用键：历史库身份、来源配置、应用范围、价格修订、时区、冻结时间范围与 CCS 模式均不能串用。应用范围扩展或配置失配必须正确退回采集。
6. 图表保留同维度 zoom、维度切换轴向/zoom 重置、主题切换、空数据和 reduced-motion 约束；不靠关闭动画、隐藏旧图或截图延时掩盖问题。
7. 不将全部事件或费用明细常驻内存；既有 8 会话/256 MiB 配额与最后一个 Arc 释放规则继续有效。不新增历史库 schema、查询游标版本或全局无界缓存。
8. 不关闭用户实例、不改其采集间隔或系统设置；不清库、不重新导入、不删除既有备份。人工清理仅送回收站，失败保留现场。

方案取舍：采用上述局部修复。拒绝“停用备份/动画”这种掩盖方式；本批不引入全历史预聚合、多层跨查询缓存或新的存储引擎。查询热路径以阶段测量决定改动，不能先假定计价或 SQL 是主因。

## 3. F01：纠错备份只由实际纠错意图触发（优先止住增长）

**文件**：
- 修改 `crates/tokenscope-core/src/history.rs`：`count_pending_corrections`、`lookup_hits`、`write_events_in_tx` 相关判断；保持 `backup_before_correction` 的安全约束。
- 新增 `crates/tokenscope-core/tests/correction_backup_policy.rs`；复用/补充 `codex_request_identity_migration.rs` 与 `claude_cross_session_migration.rs`。

**步骤**：
1. 用合成库和真实采集入口构造“已是强身份的 Codex 请求被反复重解析”。首轮正常写入后，重放/追加新请求再重放；断言备份数不再增长、正常新增只增一条、四桶正确、稳定重放 generation 不变。旧实现必须在备份数量断言处红。
2. 补必要纠错正例：弱事实升强、两/三来源拆分、compaction 镜像、跨会话重复合并、异响应镜像转移、弱 count-only 接管旧弱别名。断言真正纠错前有备份，同批最多一份，重扫/重启后不继续备份或改数。
3. 让预判与事务内使用同一套“小型纠错意图判断”（名字由实现确定）：区分普通命中、真正的旧表示升级、合并、来源/别名转移。不能简单用 `record_id.is_some()` 或“有任意 origin 命中”决定是否备份，也不能忽略无 response_id 的弱记录接管路径。
4. 审查批内依赖：前一条写入可能改变后一条的命中。预判可保守，但事务内若发现未取得备份的实际纠错，必须拒绝并回滚，不可放行。若预判/事务之间历史代次变了，应在串行采集/事务边界重新验证，不能拿过时判断当许可。
5. 备份失败回归：旧事实、别名、来源和 generation 均不改变。日志区分“计划纠错/已完成”，不再把预判数说成实际改写数；无变更的自动轮次不刷 info。

**定向验证**：

```powershell
python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test correction_backup_policy
python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test codex_request_identity_migration
python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test claude_cross_session_migration
```

不要读取真实会话或对真实库运行修复探针。既有多余备份的清理不在本批授权内。

## 4. F02：布局与数据统一更新，保住柱形动画

**文件**：
- 修改 `frontend/src/components/TrendChart.vue`、`TrendChart.test.ts`。
- 新增 `frontend/scripts/contracts/ui-chart-transition-contract.mjs`，接入 `interaction-contracts.mjs` 和 `run-ui-refresh-e2e.mjs` 的常规浏览器门禁；必要时补合成 fixture。

**步骤**：
1. 单测主动触发 ResizeObserver 回调，不能沿用完全空桩。覆盖“watch 数据 → 改高度 → setOption → observer 回调”的旧顺序，证明存在第二次无动画 resize。
2. 正式浏览器契约使用真实组件/ECharts，构造模型 20→4、模型→按日、按日 7→14、空→有值、连续切换和真实容器 resize。普通动画模式下至少取两个中间帧：柱形几何应继续变化、不能在首帧直接等于终态；不能只断言 `setOption` 被调用或全局 animation 仍在运行（可能只是轴标签动画）。保存失败现场与帧数据。
3. 改为一个布局后更新调度：先应用目标高度，读取实际尺寸；确实变尺寸时先 resize，再下发新数据。记录已应用宽/高，ResizeObserver 对相同尺寸不再重复 resize；由数据改高度产生的回调不再打断刚开始的动画。同轮数据/维度/尺寸变化合并，卸载取消待处理任务。
4. 轴标签宽度更新并入该批次，不在 observer 里再发无必要的独立 setOption。同维度沿用实例和 dataZoom；维度切换清理旧轴/系列并重置 zoom；主题按既有规则重建。reduced-motion 下允许不动画，正确数值与交互仍须通过。
5. 浏览器 A/B 验证：旧版在至少一个高度变化场景红，新版所有规定场景绿。沿用合成来源和 IPC 替身，不启动用户安装版。

```powershell
pnpm --dir frontend exec vitest run src/components/TrendChart.test.ts
pnpm --dir frontend test:e2e
```

## 5. F03：减少筛选查询等待，不用“只省几十毫秒”冒充完成

**主要文件**：
- `frontend/src/composables/useUsageQuery.ts` / `.test.ts`、`frontend/src/views/Dashboard.vue` / `.test.ts`。
- `crates/tokenscope-core/src/query.rs`、`report.rs`；按实测热点最小修改 `history.rs`、`aggregate.rs` 或 `pricing.rs`。
- 新增 `crates/tokenscope-core/tests/filter_query_performance.rs`（合成基准与确定性工作量回归），沿用 `history_queries.rs`、`query_snapshot_contract.rs`、`query_memory_budget.rs`、`query_time_range.rs`。

### F03a：先冻结基准与正确结果

1. 构造约 4–7 万请求、四应用、约 9,000 条价格条目、CCS 日汇总与请求明细重叠的合成数据。业务输出全部落 hermetic 临时根，禁止默认用户 cache_dir/pricing_index。
2. 启动与首次装载另算；同一版本/构建 profile/机器对暖态筛选序列跑 5 轮：全部↔单应用、按日↔模型↔应用↔项目、全部时间↔7日↔24h，包含快速连点。保存阶段耗时和 p50/p95，不把不同数据范围或旧工具口径互相比。
3. 分开测采集、行索引/排序、CCS 覆盖桶、汇总读取/聚合、明细首页、前端发布和柱形首帧。只有确有需要才加低频 debug/测试计数，不能逐事件打日志或持有全历史做 profiling。
4. 保留基线结果，后续逐项比较请求数、四桶、费用、未知/估算标志、A/B/C 选择、分组及分页身份。费用比较沿用现有精度约定，不靠放大误差吞差异。

### F03b：分离筛选与采集，控制过时请求

1. 把筛选变化定义为独立批次来源（例如 `filter`），正常使用 `collect:false`，复用现有 `recent_collection` 和只读快照路径；启动、显式刷新、强制重扫维持采集。来源/价格配置变化或复用不命中必须退回正常采集。不能把手动筛选伪装成自动刷新而丢失状态反馈。
2. 补红测：筛选不多跑采集；全应用→单应用可投影，单应用→全部或不同单应用未覆盖时必须补采；价格/来源配置变化不拿旧上下文；仍发新 query_id，读最新已提交 generation。
3. 对快速连续选择仅保留最新筛选意图：沿用并补强批次代次守卫，防止旧响应发布/保存；若做在途合并，旧批次结束后最多补发一批最新意图，不为每次点击堆积完整后台查询。显式刷新和后台更新通知不能被错误吞掉。
4. 控件选择与“正在更新”反馈及时变化；图表/表格保留已提交的一致旧批次直到新批次齐备。不要先套新分组标题/筛选身份再显示旧数字，也不独立发布汇总与明细。

### F03c：处理已知重复工作，并以测量决定下一处热点

1. **覆盖桶扫描**：当前 `collect_detail_buckets` 对每个来源时区扫描所选应用的全部事件，连没有对应日汇总的应用/模型/日期也建桶。只为已有汇总涉及的应用与桶构建覆盖统计；必要时在 ReadSnapshot 读取侧收窄应用，保留分批读取。无汇总不扫描；`detail_only`/时刻范围可以跳过不需要的覆盖统计，但必须保留原有时区/范围诊断。
2. 回归明确比较 CCS 选择输出而非仅总 token：A/B/C、旧批次、多批次、来源时区、快照后新增、无明细历史、非 Claude 事实都不变。不能用当前展示日期范围裁掉覆盖判定需要的同桶明细。
3. 若主要耗时仍在汇总，以 F03a 结果决定最小优化：优先消除重复模型归一化/候选收集、宽字段反复物化等工作。候选匹配结果只在同一冻结 Pricing 上复用，金额仍按每条请求四桶、分档和事件时间计算；不能只按模型缓存最终费用。若采用查询内候选 memo，须有条目/字节上限，超限走原计算，不截断数据，不在查询结束后无界保留。
4. 不先上跨查询结果缓存。确需共享行索引/聚合结果时，先在计划追加完整键、所有权、失效和预算设计并验证，不能以不完整 key 换取漂亮耗时。不得为本批性能改动顺手迁移 schema/游标或重做计价。
5. 必须报告优化前后每阶段数字。仅把 collect:true 改 false、仅改善重复访问命中，或仅加加载动画，不能判定查询性能项完成。

### 本专项性能验收目标

- 以上合成规模、同一参考机器，暖态常用筛选端到端 p50 **目标 ≤500 ms，且相对同机基线下降至少 40%**；首次读取新范围与返回已访问范围分别报告，不能只报缓存命中。p95、前端长任务和临时/保留内存一并记录。
- 控件/加载反馈目标在 **100 ms 内**出现；柱形更新保留真实中间帧。这些是本专项参考机验收目标，不直接加入易抖动的 CI 绝对耗时断言；CI 验证确定性的扫描/采集次数、最新意图和正确性。
- 未达到目标就继续定位尚占主导的阶段；若最小范围内仍未完成，明确保持 F03 开放，不能以“有提示/测试全绿”称三项全修复，也不得自行扩展成存储架构重做。

## 6. F04：集成验收与防回归

| 场景 | 必须观察到的结果 |
| --- | --- |
| 连续多轮相同 Codex 强记录、追加新请求 | 无无意义备份；请求与四桶仅按真实新请求增加 |
| 真弱事实升级 / 跨会话合并 / 镜像及弱别名转移 | 先备份再纠错，同批一份；备份失败不改旧事实 |
| 高度改变的数据更新 / 维度切换 | 有柱形中间帧，不被 observer resize 立即截断 |
| 同高度 / zoom / 主题 / 空数据 / reduced-motion | 既有行为正确；无重复 init/dispose、卸载无残留任务 |
| 快速筛选 A→B→C、响应乱序 / 一侧失败 | 仅最新一致批次发布，旧响应不落快照，不混 query_id |
| 采集、CCS 导入或价格更新期间切换 | 新会话见新事实/价格，旧分页仍冻结；覆盖策略不漂移 |
| 来源缺失、配置变化、应用范围扩展 | 不丢已保存历史；该补采时补采，不伪造空结果 |
| 预算、TTL、超长 ID、算术溢出 | 原有拒绝/释放/受检算术不被优化绕过 |

定向回归包括 `correction_backup_policy`、Codex/Claude 迁移族、`ccs_rollup_source_selection`、`ccs_rollup_coverage`、查询快照/时间/预算、`n18_cache_tier_pricing`、TrendChart、useUsageQuery、Dashboard 及真实浏览器动画契约。不要增加与实现逐行对应的重复测试。

## 7. F05：版本、检查与交付

1. 分主题提交 F01 / F02 / F03，保留红测与前后测量证据。先完成受影响定向验证；按仓库选择器运行正常提交钩子，不 `--no-verify`，不降低预算、跳过断言或改系统权限。
2. 本轮计划不分配版本、不改 release-change。实施产品修改时 fetch 最新 main，用 `bump_version.py --bump patch` 分配本批版本（当前基线预期 0.7.5，以工具实际结果为准）；同一 PR 补修不重复递增。
3. 建 PR 前按仓库规则登记上一版（当前为 **v0.7.4**）：`upgrade_baselines.py add --tag v0.7.4` 下载并验证真实资产，再检查；若执行时已有更新的发布，以最新 main 对应的上一版为准。不要把缺基线留成一轮可避免的 CI 红灯。
4. 完整检查：Rust fmt、普通/acceptance 两套 workspace clippy/test；前端 typecheck/format/test/build/E2E；Python/PowerShell 契约；build_inputs、版本与升级基线检查。Rust 使用 `scripts/run_rust_tests.py` 入口，测试写入全隔离。
5. PR 最新 HEAD 的 gate、native-ui、install-test、plan-upgrades 与全部升级矩阵必须通过。失败先读日志/artifact，修复已定位问题；外部瞬断最多一次有依据的失败作业重跑并如实记账，不拿重跑当根因修复。
6. 本机受控 Native 仅在隔离根、合成数据上做必要定向验收，不关闭/干扰用户实例，端口/PID/路径冲突即保留现场。真实用户数据性能复测不在默认授权内；需要时另行取得授权，不默读真实会话内容。
7. 更新本计划、总账与必要语义/设计说明。没有语义变化不要改旧统计口径。交付报告逐项列 F01/F02/F03 的证明、前后数字、PR/HEAD/CI 与未验证项，停在可审查 PR；**不自动合并、不发布、不在本机安装**。

## 8. 当前状态

| 项 | 状态 |
| --- | --- |
| F01 纠错备份触发 | **已实施**：普通强重放/追加无备份；弱升强、合并、镜像/来源/别名转移仍先备份，同批最多一份 |
| F02 图表更新时序 | **已实施**：布局与数据同批更新；真实 ECharts 中间帧契约覆盖 20→4、模型→按日、7→14、空→有值、连续切换与容器 resize |
| F03 筛选查询性能 | **已实施**：筛选走 `collect:false`，行索引按时间窗口收窄，覆盖桶按应用/模型收窄，汇总窄投影，明细首页只读当前页 |
| F04/F05 集成与交付 | PR #61 已合并（`aa134a1`）、v0.7.5 预发布已发布；Release 全作业（含两条连续升级链）实跑通过，资产身份与哈希核对一致 |

## 9. 执行记录（2026-11-10）

- **F01（`fix(历史)`）**：预判改为按写入索引记录真实纠错意图，事务内只对获准写入开放纠错通道；批内强升级会影响后续弱 count-only 的归属，按升级后的强事实保守判定。新增 `correction_backup_policy` 5 项：普通重放/追加 0 备份、弱升强 1 份、同批双升级 1 份、弱 count-only 接管 1 份、备份失败旧事实/别名/来源/generation 全不变。旧实现重放 4 次生成 4 份备份，现已为 0。
- **F02（`fix(图表)`）**：一次布局后更新，先应用高度并量测，确实变尺寸时先 resize，再与数据/维度/轴标签一起下发；相同尺寸的 ResizeObserver 回调不再 resize/setOption。新增浏览器契约用真实 ECharts 采样 SVG 柱形几何，要求至少两个中间帧、首变帧不是终态。旧组件在模型 20→4、模型→按日、空→有值三个场景失败；新组件 15 条断言全通过。同高度/zoom/主题/空数据/reduced-motion 语义不变。
- **F03（`perf(查询)`）**：筛选变化以 `filter` 批次 `collect:false` 读取已提交历史，启动/显式刷新/程序刷新仍采集；最近采集键不命中时后端退回采集。快速连点仍建新 query_id，旧响应由 epoch/seq 守卫丢弃，汇总与明细只在同一会话齐备后发布。后端：冻结时间范围先做 SQL 秒级窗口，覆盖桶只为已有日汇总的应用/等价模型读窄投影（无汇总/仅明细/时刻范围直接跳过），汇总复用存储等价模型键与窄事件，无下钻明细首页 total 取快照行数、只读当前页。**价格/来源变化**：采集键纳入三份价格输入实际路径的长度与 mtime（含 GUI 未显式注入时的默认路径），价格同步后旧快照立即失效；MainApp 同时广播文档级 `data-changed`，隐藏中的汇总页也记下“下一批先采集”，不依赖 KeepAlive 停用期间是否收到 prop 更新。
- **同规模基准（release，50k 请求 / 四应用 / 9,000 价格条目 / 30 天 CCS 汇总，暖态 5 轮 p50）**：

  | 场景 | 改前（建快照+汇总+明细） | 改后 | 降幅 |
  | --- | --- | --- | --- |
  | 全部时间·按日 | 535+323+221=1079 ms | 236+247+2=485 ms | 55% |
  | 全部时间·按项目 | 592+343+224=1159 ms | 227+240+2=469 ms | 60% |
  | 全部时间·按应用 | 539+312+209=1060 ms | 237+238+2=477 ms | 55% |
  | 全部时间·按模型 | 554+346+214=1114 ms | 232+255+2=489 ms | 56% |
  | 近 7 天·按日 | 530+69+46=645 ms | 90+52+2=144 ms | 78% |
  | 近 24 小时·按日 | 615+11+9=635 ms | 26+8+2=36 ms | 94% |
  | 单应用·按模型 | 216+85+52=353 ms | 119+61+2=182 ms | 48% |

  启动采集（空合成来源）约 127–197 ms，与筛选路径分开报告；24h/单应用首次范围也因 SQL 窗口和窄投影下降。明细首页从 209–224 ms 降到约 2 ms。前端筛选选择后同 tick 进入「刷新中…」，保留旧的一致批次直到新批次齐备。
- **定向回归**：`correction_backup_policy`、`codex_request_identity_migration`、`claude_cross_session_migration`、`filter_query_performance`、`ccs_rollup_coverage`、`ccs_rollup_source_selection`、`query_snapshot_contract`、`query_time_range`、`query_memory_budget`、`history_queries`、`history_storage` 全绿；前端 `useUsageQuery`、`Dashboard`、`TrendChart` 与浏览器图表契约通过。
- **版本**：`bump_version.py --bump patch` 分配 **0.7.5**；上一版 **v0.7.4** 已在 PR 前登记为升级基线（30 个版本全量 check、44 项单测通过）。
- **PR #61**：F01–F03 代码 HEAD `2f9d061` 的 `gate`、`rust`、`rust-acceptance`、`contracts`、`frontend`、`native-ui`、`install-test`、`plan-upgrades` 与 12 组升级矩阵全部成功；PR 侧 `upgrade-chain` 按设计 skipped。此后仅本计划文档更新，仍停在可审查 PR。
- **验收期补修（2026-11-11，HEAD `589d2e9`）**：两处调度缺陷——①快速切换堆积后台查询：`startRefreshBatch` 在途期间也立即开新批（后端工作不取消），改为一条在途、待执行只保留最后一次选择，批次结算后执行；`auto` 仍经 `requestAutoRefresh` 重入，翻页保护不被绕过。②同一次设置写入刷两次：`refreshKey` prop 与文档事件各排一批，改为同一 token（递增后的 refreshKey 值随事件 detail），先到者调度、后到者见相同 token 只同步基线；prop-only / event-only 仍各自调度。Dashboard 两个断言旧时序的用例改写为新时序（不变量保留：最新结果胜出、两侧齐备才落盘）；前端 1112 项 test、typecheck、format、build 与浏览器 E2E 契约全绿，PR #61 HEAD `589d2e9` 全部检查成功、`CLEAN`。
- **独立复核补修（2026-11-11，HEAD `64b86a0`）**：两处调度反例——①在途排队的手动采集意图被后续筛选覆盖：`manualRefresh` 在批次在途时递增 `collectRequestSeq` 独立记账"仍需采集"，待执行来源照常被最新筛选覆盖，`query_begin` 按 `collectSeq > consumedCollectSeq` 判定，意图随 begin 成功才消费、失败保留给重试。②同 token 回声未同步 `lastRefreshKey` 基线：事件先到 + prop 后到合并后，下一次普通 begin 把同一次变更再当成未处理程序刷新，普通筛选错发 `collect:true`；回声分支现同步基线，事件先到与 prop 先到两个方向均只处理一次。回归 4 项（在途→手动→再筛选、意图消费一次、失败保留、两个方向回声后普通筛选 `collect:false`）先红后绿；前端 1116 项 test、typecheck、format、build 与浏览器 E2E（含图表动画契约）全绿，PR #61 HEAD `64b86a0` 全部检查成功、`CLEAN`。
- **未自动合并、未发布、未在本机安装**；最终交付停在可审查 PR。
- **合并与发布（2026-11-11）**：PR #61 以 merge commit 合入 main（合并提交 `aa134a16d9a65d838bef9bef038e9ca8966bcb4a`，合并前核对 HEAD `3ec1e90`、20 项检查全成功、`bump --check` release:true、v0.7.4 基线已登记）。main 触发的 [CI](https://github.com/ywltby/tokenscope/actions/runs/42930342835)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/42930342826)、[Release](https://github.com/ywltby/tokenscope/actions/runs/42930342874) 全部 success；Release 内部全部 19 个作业成功：gate、plan-upgrades、build、install-test、12 组单跳升级矩阵（0.1.0/0.4.0/0.5.9/0.5.13/0.6.0/0.7.4 × present/deleted）、`upgrade-chain` present/deleted 两条连续链与 publish 全部实跑通过（PR 侧 skipped 的 upgrade-chain 以 Release 实跑为准）。
- **发布身份与资产**：[v0.7.5 预发布](https://github.com/ywltby/tokenscope/releases/tag/v0.7.5) 标签解引用 `aa134a1`（与 build-metadata.json 的 commit/version 一致）；资产 `TokenScope_v0.7.5_windows-x64-setup.exe`、`SHA256SUMS.txt`、`build-metadata.json`、`provenance.sigstore.json` 齐全。下载后重算安装包 SHA256 `b8be9c1c0fb64c780920bf5ff6acdbda20bbc586ce5a950cd827d76eb7e648b9`，与校验清单及 GitHub digest 三方一致。未在本机安装、未建立本地 release 目录（验证文件在系统临时目录）。
