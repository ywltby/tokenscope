# TokenScope 产品与技术评估及改进计划

- 创建日期：2026-10-05
- 状态：**执行中**——**A、B、C 阶段已完成**（见文末执行记录），D 阶段待启动
- 评估基线：`b79539a`，并复核评估期间新增的 `6fe5159`。下文代码行号以 `6fe5159` 为主，后续实施按符号重新定位。
- 定位：面向个人开发者的本地、多 Agent 工具用量观察器；不是代理网关、计费系统或云端监控平台。
- 适用范围：Claude Code、Codex；不改变其他 Agent 暂缓排期的既有决定。

## 一、项目经理结论

**方向正确、技术底座可保留，但当前更接近“功能齐备的个人试用版”，尚不能仅凭 M1–M11 已完成就认为具备稳定发布条件。**

主要矛盾不是支持的 Agent 太少，而是：**已有能力之间尚未形成“正确采集 → 正确计算 → 正确筛选 → 正确展示 → 可解释、可恢复”的可靠闭环。** 多一个适配器，会放大现有的身份、去重、缓存和计价问题。

建议按以下顺序推进：

1. 修复首页加载、汇总/明细范围、图表标签和值错配等直接影响使用和判断的问题。
2. 建立统计口径与缓存一致性契约，解决“未知价格被当成零”“漏扫不可见”“同数值不等于同请求”。
3. 补齐数据源管理、刷新/错误恢复、跨 Agent 项目身份，让工具真正能被其他用户使用。
4. 用自动化行为测试和安装包验收定义发布完成，再考虑增加 Agent。

**不建议**重写前端、切换 Tauri、替换 SQLite 为 DuckDB、恢复 CLI、建立动态插件系统，或继续以价格表条数/里程碑数量作为主要进度指标。

### 应保留的成果

- `source → UsageEvent → dedupe/aggregate → report → GUI` 已有合理分层，两个适配器共享主要统计链路。
- 源日志只读，缓存和设置写入 TokenScope 自身目录；真实日志不进入仓库。
- UTC 事件存储、显式展示时区、坏行容错、整模型价格未知保留 token 的基本方向正确。
- 任意模型名和第三方路由兼容是实际需求，不能把 Claude Code 等同于 Anthropic 模型。
- SQLite 文件指纹缓存、后台执行重活、托盘/单实例、价格覆盖和阶段耗时日志均可继续使用。
- `b79539a` 已修复测试污染真实缓存；`6fe5159` 已补充命令失败与关键路径日志。本计划不把这两项重复列为待修复缺陷。

## 二、评估方法与证据边界

阅读了产品说明、里程碑归档、核心模型/解析/去重/价格/缓存/report、Tauri 命令与运行入口、Vue 页面与组件、现有 fixture 和门禁配置；对关键发现复核了当前代码路径。

- **已确认**：代码可直接证明，或已有本轮最小复现；不等同于已在真实日志中量化影响。
- **待验证风险**：有明确反例或缺少契约，但真实发生比例、上游字段语义等仍需进一步验证。
- **产品缺口**：相对于目标用户旅程缺少能力，不把旧的明确取舍伪装成新 bug。
- 本轮未读取真实 Agent 日志、未运行真实数据重建测试、未启动 GUI 或安装包；不声称已经完成真机体验验收。
- 历史 cc-switch 对照是有价值的参照，但不是独立真值。对照应先统一时间、纳入范围、去重和缓存口径，再解释差异。

## 三、问题清单与优先级

P0：先修，阻断可用性或直接产生错误展示。P1：可信试用版发布前必须处理或明确限制。P2：在正确性稳定后完善，不作为重写理由。

### 3.1 已确认的实现问题

| ID | 优先级 | 问题、具体场景与影响 | 代码依据 | 建议 |
| --- | --- | --- | --- | --- |
| F01 | P0 | 汇总与明细共用 `runSeq`；两个 immediate watcher 依次启动，后者使前者失效。无快照时汇总不落地、持续转圈；有快照时可能一直停留在旧数据 | `frontend/src/views/Dashboard.vue:37,60-108,146-147` | 分离两条请求代次，或以同一 query generation 协调；独立结束 loading，补用户可见错误和重试 |
| F02 | P0 | 前端已向明细传 `from/to`，后端只应用 `days`。选择历史一周后，汇总属于该周，明细却可能包含其他日期 | `Dashboard.vue:91-99`；`src/report.rs:462-483,523-553` | 汇总和明细复用 `apply_time_filter`，先过滤主范围，再叠加下钻；统一验证非法参数 |
| F03 | P0 | 非日期图表只对分类标签排序，series 保持原顺序。A=10、B=100 时可能显示 B=10、A=100 | `frontend/src/components/TrendChart.vue:61-75` | 生成一份排序后的分组，所有标签、series、tooltip 共用；不用视觉改版掩盖数据错位 |
| F04 | P1 | models.dev 有 cost 对象但某分项缺失时填成 0；该模型仍返回完整费用，非零缓存 token 可能被当成免费 | `src/modelsdev.rs:33-41,69-70,95-101`；`src/pricing.rs:425-462` | 分项保留未知与显式零的区别，表达部分计价；明确同一计价身份下的回退规则 |
| F05 | P1 | 扫描子目录失败被丢弃；文件读取失败产出空结果，仍可能被缓存。读取后才取指纹，也可能把追加前的数据配上追加后的指纹 | `src/source/mod.rs:79-83,123-135`；`src/source/claude.rs:100-107`；`src/source/codex.rs:111-117`；`src/report.rs:290-305` | 发现/解析返回结构化诊断；IO 失败不写成功缓存；读前后指纹一致才提交，不稳定文件稍后重试 |
| F06 | P1 | 查看单个 Agent 时只收集该源的 keep_paths，却对整个数据库 purge。全部→Claude→全部会删除再重建 Codex 缓存 | `src/report.rs:237-248,269-271,339-347`；`src/cache.rs:240-258` | 清理限定到成功完成发现的 source/root；未选中和发现失败的来源不能被清理。这不是已修复的测试污染问题 |
| F07 | P1 | 日期弹层的 `show` 未绑定；取消/确定无法按预期控制弹层。清空起始日后确定按钮禁用，“全部时间”分支不可达；“24h”实际是两个自然日 | `frontend/src/components/DateRangeSelect.vue:15,27-32,47-79,92,124-126` | 修复受控开关、草稿提交和清除入口；自然日与滚动小时窗口不能混称 |
| F08 | P1 | 视图快照没有查询身份，恢复时筛选重置但数据可能属于旧 Agent/维度；两条请求各自存快照也可能混代 | `frontend/src/views/Dashboard.vue:24-37,74-76,103-122,130-147` | 快照携带完整 query key、版本和 generation；只恢复匹配的数据，或连同筛选一起恢复 |
| F09 | P1 | 根 `Cargo.toml` 排除了 `src-tauri`，现有 `cargo --workspace` 门禁不覆盖壳；前端没有行为测试脚本。类型检查通过不能发现 F01–F03 | `Cargo.toml:8-11`；`src-tauri/Cargo.toml:1,34-35`；`.githooks/pre-commit:7-16`；`frontend/package.json:6-26` | 显式增加两个 manifest 的门禁及 mock IPC 组件测试；建立 Windows CI 与安装包验收清单 |
| F10 | P2 | 持久化价格索引有读函数，但生产 `load_cached` 路径未接入；内存命中返回空 warnings。models.dev 来源标签还会落成“内置” | `src/pricing.rs:202-219,492-495,521-547` | 先补重启加载索引测试，再接入读路径；缓存价格及其诊断；修正来源标签，不以索引文件存在作为验收 |

F01 本轮使用仓库已安装的 Vue，在内存中按相同 immediate watcher/异步请求顺序做最小复现，得到：`runSeq=2, loading=true, report=null, events="events"`。这是状态逻辑复现，不是真机 GUI 冒烟。

### 3.2 需要先定口径或专项验证的问题

| ID | 分类/优先级 | 事实与风险 | 处理方向 |
| --- | --- | --- | --- |
| R01 | 算法反例 / P1 | `src/dedupe.rs:40-52` 只按 session 和四类归一化 token 去重；同 session 两个真实请求数值相同，即使模型/日期不同，也会丢一条。实际发生频率未验证 | 用“同响应重播”和“不同请求同用量”双向 fixture 验证；调查原生响应/turn 身份、累计值变化。不能直接关去重，也不能简单加时间戳使重播全部入账 |
| R02 | 字段语义 / P1 | Codex 校验 raw total=input+output，归一化减 cache_read 后又独立加 cache_write；展示总量会比 raw total 多出 cache_write（`src/source/codex.rs:218-246,296-313`） | 先确定各日志版本的 cache_write 是否包含于 raw input，再确定互斥 token 桶；同时核对内置缓存价格列，不在未查证权威来源前断言价格应是多少 |
| R03 | 产品身份 / P1 | Claude 用目录 slug/父目录，Codex 用 cwd basename；同名不同路径可能合并，同项目跨工具又可能分裂（`src/source/claude.rs:71-97`；`src/source/codex.rs:231-255`） | 分离 `project_id` 与 display_name，保留原始路径/slug；优先可靠 cwd，再用人工别名关联。不能通过不可逆 slug 猜出完整路径 |
| R04 | 并发与升级 / P1 | 汇总/明细分别完整采集；缓存文件行与事件分两次读取，清空也非整体事务；解析语义版本不参与命中（`src/cache.rs:91-95,110-137,274-277`） | 用故障注入和线程屏障验证完整旧/新快照；加入解析版本失效；先做事务与单次采集合并，再考虑长期性能优化 |
| R05 | 费用可解释性 / P1 | 前缀命中不等于实际供应商价格；模型可经第三方路由，同名不同 provider 价格可能不同（`src/pricing.rs:425-451`；`src/modelsdev.rs:90-109`） | 费用明确为估算，保留原始模型名，展示精确/别名/前缀匹配及来源。不得从工具名推断供应商；订阅费、折扣、税费不冒充已覆盖 |
| R06 | 同步生命周期 / P2 | 价格/设置直接覆盖写，手动和自动同步无统一互斥；自动到期主要看 models.dev，设置读取失败按默认开启（`src/modelsdev.rs:117-122`；`src/openrouter.rs:104-109`；`src-tauri/src/lib.rs:196-247`） | 原子替换、共用同步协调器、逐源成功/失败状态；关闭自动联网后，设置异常不应悄悄恢复联网 |

### 3.3 产品设计缺口，不误报为历史实现 bug

1. **“多 Agent 工具”不等于“所有子 Agent 用量”。** Claude sidechain 排除、Codex 某些通道/无模型行跳过、请求数等于纳入的去重用量事件，都是旧的明确口径。应显式说明纳入范围，而不是悄悄改口径或宣传全覆盖。
2. **首次使用缺少纠正路径。** GUI 只使用默认目录；“目录不存在”不能直接推断“未安装”。需要区别未启用、未发现、无日志、无法读取、正常、有遗漏，并允许重新检测和纠正来源目录。
3. **缺少完整刷新与错误恢复。** 需要刷新按钮、最后成功采集时间、缓存/过期状态、失败重试；只切页面才能更新且重置筛选，不是稳定的托盘工具体验。
4. **费用未知的追溯不完整。** 已有 `†` 标记值得保留，但应显示未知 token/模型与部分计价状态；合计行不能把字面“合计”当模型/项目/日期下钻。参考 `SummaryCards.vue:22-59`、`UsageTable.vue:64-108`、`Dashboard.vue:130-137`。
5. **工程完成与用户验收混在一起。** M8/M11 等已归档但仍写“待安装冒烟”；这不代表功能必然有错，却说明当前完成状态不能直接推导“可发布”。历史记录保留，另登记待验收项，不重写历史事实。

## 四、目标用户、成功标准与非目标

### 目标用户旅程

1. 安装启动，知道发现了哪些来源、是否可读、最近何时更新；没有日志也知道下一步。
2. 一分钟内理解最近 7/30 天、不同工具/模型/项目的用量，分清非缓存输入、输出、缓存读写。
3. 点击某一统计项，下钻得到同范围明细；发现异常可以解释“漏扫、排除、去重、未知价格”中的哪一环。
4. 日志持续增长时能刷新；失败可重试且不把旧数据显示成新结果；离线仍可查看本地数据。
5. 升级后缓存安全失效或迁移，不能长期继续显示旧解析规则下的数字。

### 成功指标

- 合成黄金数据集：四类 token、请求/事件数、分组、时间区间与手算期望逐项一致；不是只与上一版输出相同。
- 同一 generation、同一筛选下：总计 = 分组之和 = 全部分页明细之和；未知价格部分单列核对。
- 未变化来源：全部→单来源→全部不重复解析另一来源；缓存/无缓存/重建结果一致。
- 发现失败、解析失败、排除项、价格未知均有可区分状态；不以坏行 0 或 unknown 0 作为“全量正确”的充分证据。
- 首页、日期选择、下钻、重试、快照恢复、托盘恢复均有明确验收记录；所有 P0 关闭后才扩大试用。
- 性能以 release 和固定数据规模记录中位数/P95、峰值内存与重解析文件数，不把“看起来快”作为唯一标准。

### 本轮非目标

- 不新增 Gemini/OpenCode 等适配器；先拿到合成契约样例和本地可验证来源，再由用户另行排期。
- 不接云端账号、不上传日志、不采集提示词/回答正文、不扫描凭证；价格同步不上传使用记录。
- 不做真实账单核算、预算告警、团队云看板、跨设备同步、自动更新发布服务。
- 不新增 CLI，不引入 DuckDB、动态插件、文件监听和字节游标增量解析；除非后续量化证明现有方案不达标。
- 不把“日志已被源工具删除后保留永久历史”隐式加入缓存职责；默认仍反映当前可读日志，历史归档能力需单独立项。

## 五、实施前必须固定的不变量

1. **只读与测试隔离**：源目录仅只读；所有测试显式注入临时来源、缓存、价格索引及相关状态目录。真实数据工具保持显式授权和 ignore，不进入普通门禁。
2. **事件与身份**：Tool/SourceInstance/Session/Event/Project/Model 是不同概念。模型不能代替工具，basename 不能代替项目身份，token 数值不能单独证明请求相同。
3. **去重**：重复回显只计一次；不同真实请求即使用量完全相同也应保留。没有足够身份时输出可解释的保守规则，禁止默默假定精确。
4. **token 桶**：统一展示桶应互斥；reasoning 等子集不能再次相加。缓存写入语义按适配器/版本验证后映射；无法解释时保留诊断，不用截断/补零掩盖冲突。
5. **请求单位与范围**：默认标识为“用量事件/请求（按日志口径）”，不是用户提问数。sidechain、无模型和不支持通道的纳入/排除必须可见；改变规则需单独确认与版本失效。
6. **时间**：事件存 UTC；选择器向业务层传自然日或明确的滚动窗口，不在本机与统计时区之间反复转换。自然日内部可实现为目标时区 `[起日 00:00, 末日次日 00:00)`，不能用固定 24 小时替代 DST 自然日。
7. **同源同查询**：汇总、图表、明细使用同一查询定义、事件 generation、价格版本和一次冻结的 now；数据或查询更新后，旧响应不能覆盖新结果。
8. **费用**：缺价格 ≠ 零价格；金额分为完整估算、部分估算、全未知。只有使用量非零的未知分项影响该事件计价完整性。跨供应商分项不得无条件拼成一张虚假的完整价格表。
9. **缓存**：缓存纯优化；IO 失败/不稳定读取不能成为成功命中；筛选不修改其他来源缓存；发现失败不能触发该来源清理；解析规则升级自动使旧解析结果失效。
10. **安全发布**：缓存读/清理的一致性与文件替换必须有事务/原子边界；关闭自动同步的用户意图不能因设置异常被重置成联网。
11. **部分成功可见**：一个来源失败不拖垮其他来源；“只统计到部分数据”必须随结果持续展示，不能只写一次日志后消失。
12. **接口可扩展**：新增适配器主要修改适配器及来源注册描述，不向聚合层添加工具专属条件；允许注册元数据/图标变更，不承诺新增工具完全零接线。

## 六、建议的最小架构调整

保留现有 crate 和技术栈，只补三个缺失的边界，不做一次性大重构。

### 6.1 来源契约

- `SourceConfig`：稳定 ID、工具类型、enabled、显式目录；先支持现有两个工具的一源一根，必要时扩展多根。
- `DiscoveryResult` / `ParseOutcome`：成功文件、失败/跳过计数、原因、读取完整性；不把 IO 失败包装成正常空解析。
- 统一来源注册表同时服务发现、设置页、缓存清理范围和 UI 元数据。
- 同源重叠目录按规范化文件路径去重；独立来源中的原生 ID 冲突需要命名空间。复制日志是否合并必须显式定义，不能仅靠目录 ID 得到伪精确答案。

### 6.2 采集快照与查询契约

- 在 `report` 周边引入轻量采集协调器：合并同一次并发采集，发布不可变 `CollectionSnapshot`，包含 generation、采集时间、来源状态、事件和诊断。
- 定义 `QuerySpec`，集中表达 agent、时间、时区、模型、项目和下钻；先让现有汇总/明细共享过滤函数，再复用快照，降低改动风险。
- F01 先用独立请求序号修复，不必等待后端快照重构；快照机制不得成为阻挡简单正确性修复的依赖。
- 明细以后支持稳定游标 `(timestamp, event_id)` 或等效分页；`total` 是过滤后的全部数，200 条只是展示上限，不可用前 200 条去核对全量总计。
- 视图缓存携带查询和 schema 版本，原子保存同代结果；缓存恢复失败回正常查询，查询失败保留有明确过期标记的旧数据。

### 6.3 价格结果契约

- `PriceMatch` 保留原始模型、匹配条目、匹配方式、来源、同步时间和价格版本。
- `CostEstimate` 表达已知小计、未计价分项/token 和完整性；不得仅用一个 `Option<f64>` 表达所有情况。
- 精确匹配优先，显式别名可覆盖；前缀推断需有边界和标识，保留合法自定义后缀，避免 `gpt-50` 自动继承 `gpt-5`。
- 先完成“不把未知当零”，再做完整的来源/匹配解释 UI；不扩大为供应商账单系统。

## 七、分阶段执行计划

以下为建议阶段，不自动占用新的 M 编号。确认后逐阶段实施；每个缺陷先写失败回归，再写实现。表中**新增测试名及 `pnpm test` 脚本都是计划交付，不代表当前已存在**。

### A：恢复基本展示闭环及建立回归入口（建议 2–4 人日）

目标：先让“打开→筛选→看图→看明细”正确工作，解决 F01–F03、F09 的最低保障。

| 任务 | 代码位置 | 拟新增测试/验收 | 验证命令 |
| --- | --- | --- | --- |
| A1 建立前端 Vitest + Vue Test Utils/mock IPC 测试入口 | `frontend/package.json`、测试配置、`frontend/src/**/*.test.ts` | `dashboard_first_load`、`ipc_error_visible`；证明新测试先在现有逻辑失败 | `pnpm --dir frontend test --run`（新增脚本） |
| A2 分离/协调请求代次，独立加载与错误状态 | `frontend/src/views/Dashboard.vue` | `dashboard_parallel_requests_settle`、`dashboard_latest_query_wins`、`dashboard_retry_after_failure` | `pnpm --dir frontend test --run` |
| A3 明细复用主时间过滤，合计行不生成字面过滤 | `src/report.rs`、`Dashboard.vue`、`UsageTable.vue` | `test_events_range_matches_summary`、`test_events_invalid_range_rejected`、`total_row_clears_drill` | `cargo test events_range`；前端行为测试 |
| A4 标签和 series 使用同一排序结果 | `frontend/src/components/TrendChart.vue` | `chart_labels_match_series`，名称排序和用量排序故意相反，逐桶断言 | 前端行为测试 + 最小窗口图表冒烟 |
| A5 门禁显式覆盖核心库和 Tauri 壳 | `.githooks/pre-commit`、新增 CI 配置、开发说明 | `quality_gate_covers_both_manifests`：在壳引入临时错误验证 CI 必红，然后撤销 | 第九节完整门禁；不跳过 hooks |

退出条件：三个 P0 均有可复现的红→绿记录；无缓存首次加载可成功/可失败重试；根库、壳、前端均进入质量门禁。

### B：统计可信度与缓存正确性（建议 5–8 人日，关键路径）

目标：处理 F04–F06、R01–R04；先给出可验证契约，避免基于猜测修改历史数字。

| 任务 | 代码位置 | 拟新增测试/验收 | 验证命令 |
| --- | --- | --- | --- |
| B1 固定各适配器字段映射、请求单位、排除项 | `src/model.rs`、`src/source/{claude,codex}.rs`、合成 fixture、口径说明 | `test_token_bucket_contract`、`test_codex_cache_write_semantics`；独立手算预期、注明适用日志版本 | `cargo test token_bucket`；`cargo test codex_cache_write` |
| B2 验证并修正去重身份，保留重播防线 | `src/dedupe.rs`、`src/source/codex.rs`、`src/model.rs`、`src/cache.rs` | `test_codex_distinct_requests_equal_usage`、`test_codex_replay_deduped`、`test_dedupe_cross_day` | `cargo test dedupe`；`cargo test codex_distinct` |
| B3 分项价格未知与完整性，保留显式零 | `src/modelsdev.rs`、`src/openrouter.rs`、`src/pricing.rs`、`src/aggregate.rs`、前端类型 | `test_pricing_missing_component_not_free`、`test_pricing_explicit_zero`、`test_partial_cost_totals` | `cargo test pricing`；`cargo test partial_cost`；前端行为测试 |
| B4 发现/读取失败可见，读取不稳定不缓存 | `src/source/mod.rs`、两适配器、`src/report.rs`、`src/cache.rs` | `test_discovery_error_visible`、`test_read_failure_not_cached`、`test_append_during_parse_not_cached_as_complete` | `cargo test discovery_error`；`cargo test not_cached` |
| B5 清理限定来源，缓存读取/清空一致，增加解析版本 | `src/cache.rs`、`src/report.rs` | `test_agent_filter_preserves_other_cache`、`test_cache_read_clear_snapshot`、`test_parser_version_invalidates_cache` | `cargo test cache`；`cargo test agent_filter` |
| B6 黄金数据集对账，归因而非强行贴齐第三方 | `tests/e2e_claude.rs`、`tests/e2e_codex.rs`、新增合成 fixture | `test_golden_summary_events_parity`，无缓存/命中/重建及双源/单源全矩阵 | `cargo test golden`；`cargo test` |

退出条件：不同请求同用量与重复回显同时正确；未知不被当零；不完整采集可见；筛选不破坏另一来源缓存；旧缓存不会阻止新解析规则生效。R02 若缺少可信语义证据，必须明确不支持的字段组合，不凭空宣布已解决。

### C：多来源产品闭环与可解释展示（建议 4–6 人日）

目标：用户能在 GUI 内找到数据、正确筛选、理解统计并从错误恢复。

| 任务 | 代码位置 | 拟新增测试/验收 | 验证命令 |
| --- | --- | --- | --- |
| C1 现有两个工具的目录配置、启用/停用、重新检测 | `src/settings.rs`、`src/report.rs`、`src-tauri/src/commands.rs`、`Settings.vue` | `test_source_config_roundtrip`、`test_source_overlap_once`、`source_empty_error_ready_states` | `cargo test source_config`；前端行为测试 |
| C2 稳定项目身份和展示名；同名不误合并，跨工具关联可解释 | `src/model.rs`、两适配器、`aggregate.rs`、缓存迁移 | `test_project_same_basename_distinct`、`test_project_alias_cross_agent`、`test_claude_nested_project_identity` | `cargo test project` |
| C3 修日期控制、清除、取消和统计时区语义 | `DateRangeSelect.vue`、`Dashboard.vue`、时区 composable、`report.rs` | `date_range_cancel_reopen`、`date_range_clear_all`、`today_uses_selected_timezone`、`test_time_range_dst` | 前端行为测试；`cargo test time_range` |
| C4 查询快照匹配、刷新、最后成功时间、失败重试与分页 | `Dashboard.vue`、`App.vue`、`EventTable.vue`、`report.rs`、相关 commands | `view_cache_query_mismatch`、`refresh_preserves_filters`、`test_events_pagination_parity` | 前端行为测试；`cargo test events_pagination` |
| C5 完整/部分/未知费用、来源与范围说明、合计可追溯 | `SummaryCards.vue`、`UsageTable.vue`、`EventTable.vue`、`types.ts`、价格匹配 DTO | `cost_unknown_partial_complete`、`pricing_match_source_visible`、`excluded_scope_visible` | 前端行为测试；`cargo test pricing` |

退出条件：不读教程也能纠正数据目录；标签与实际查询一致；用户知道何时更新、遗漏什么、费用为何只是估算；主筛选/明细/快照不会各自表达不同口径。

### D：运行生命周期和可发布验收（建议 3–5 人日）

目标：处理 F10、R04/R06 的运行保障，补齐“工程完成→安装可用”的证据。

| 任务 | 代码位置 | 拟新增测试/验收 | 验证命令 |
| --- | --- | --- | --- |
| D1 合并并发采集，发布同代事件与价格快照 | `src/report.rs`、`src-tauri/src/commands.rs`、必要的轻量状态模块 | `test_parallel_queries_single_collection`、`test_snapshot_generation_consistent` | `cargo test snapshot`；壳 manifest 测试 |
| D2 价格索引真正重启命中，诊断随缓存保持 | `src/pricing.rs` | `test_pricing_index_restart_hit`、`test_pricing_warning_survives_cache_hit`、`test_pricing_version_invalidates_index` | `cargo test pricing_index`；`cargo test pricing_warning` |
| D3 原子保存、同步互斥、逐源重试、关闭联网安全 | `modelsdev.rs`、`openrouter.rs`、`settings.rs`、`src-tauri/src/lib.rs` | `test_atomic_snapshot_failure_keeps_old`、`test_sync_disabled_on_invalid_settings`、`test_sync_provider_retry_independent` | 对应 Rust 测试 + mock 网络；不请求真实服务 |
| D4 主线程残余文件 IO 后台化；验证性能而非盲目换引擎 | `src-tauri/src/lib.rs`、`commands.rs`、`window_state.rs`、性能用例 | `slow_io_window_responsive`、`synthetic_cold_warm_benchmark`、`filter_switch_no_reparse` | release 合成性能工具；Windows 真机冒烟 |
| D5 安装包、升级、托盘、主题、离线与隐私验收 | Tauri 配置、CI、发布说明、验收记录 | `windows_release_smoke`、`upgrade_cache_invalidation`、`source_tree_readonly` | 完整门禁；`tauri build`；签字式验收清单 |

退出条件：无开放 P0；P1 均关闭或有用户接受且界面明示的限制；安装包实际通过验收。产物构建成功、测试通过、实际验收通过分别记录，不能互相代替。

### 资源、依赖与控范围

- 按 1 名熟悉 Rust/Vue 的开发者估算，A–D 合计 **14–23 人日**；这是工作量范围，不是已承诺日历排期。日志语义调查、跨机验证和发布签名等待另计。
- 用户/产品负责人：拍板统计范围、请求称呼、项目关联、费用含义，并验收实际使用路径。
- 实施负责人：每个缺陷交付失败测试、最小修复、升级/缓存影响说明。
- 验收可由同一人承担，但黄金数据预期应独立手算，不能从实现自动生成“答案”。
- 关键依赖：A 先建立回归保障；B 的事件/价格契约先于 C 的相关展示。D 的缓存事务基础在 B 完成，采集快照重用可以后置。
- 若只投入第一周：只做 A + B4/B5 的明确缺陷，并启动 B1/B2 语义验证；不同时开展新 Agent、UI 改版与数据库迁移。

## 八、发布验收矩阵

| 维度 | 必须覆盖的场景 | 通过标准 |
| --- | --- | --- |
| 解析/去重 | 重播、同用量不同请求、缺模型、坏行、截断尾行、嵌套目录、session 切换 | 不崩溃；计入/排除/重复均可解释；手算总量一致 |
| 时间/查询 | UTC、上海、local、一个 DST 时区；区间端点；跨午夜；日期与项目/模型下钻 | 统一自然日语义；汇总、图表、全部明细严格同口径 |
| 缓存 | 无缓存、命中、重建；切 Agent；发现失败；读取中追加；并发清理；解析升级 | 缓存不改变数字，不污染其他来源，不永久保存失败结果 |
| 价格 | 全未知、部分未知、显式免费、同名不同源、合法后缀、误前缀、快照损坏、离线 | 未知不为零；匹配可解释；旧可用快照保留，降级状态持续可见 |
| 交互 | 首启无快照、失败重试、乱序响应、日期取消/清除、旧快照不匹配、设置往返 | 无永久 loading；筛选不误导；过期状态明确；路径可恢复 |
| Windows 桌面 | 安装/升级/卸载、双开唤醒、关窗缩托盘、退出、自启、休眠恢复、最小窗口、125%/150% DPI、明暗主题 | 每项记录产物版本、步骤、结果；真实系统操作按授权执行 |
| 隐私 | 来源树内容不变；普通测试不触碰真实缓存；自动同步关闭后设置损坏 | 不写来源，不泄露日志/项目数据，不意外恢复联网 |

性能验收建议：固定机器、release、固定合成数据集至少重复 20 次；记录原始测量条件。以热查询 P95 ≤1s 为初始目标，冷采集以可靠基线无明显回退为目标，超标先定位阶段耗时。历史记录的本机约 1.2GB 冷 5.5s/热 0.12s 仅作参考，**不是本轮复测结论**；GUI 可交互时间与后端查询时间分开记录。百万事件、持续达不到响应目标或确需 SQL 聚合下推时，再重新评估存储方案。

## 九、本轮验证记录与后续门禁

### 本轮已执行

| 检查 | 结果 |
| --- | --- |
| 根工程 `cargo fmt --all -- --check` | 通过（`6fe5159`） |
| 根工程 `cargo clippy --workspace --all-targets --quiet` | 通过 |
| 根工程 `cargo test --workspace --quiet` | 71 个通过；1 个真实数据性能测试按设计忽略 |
| 前端 `typecheck` / `format:check` / `build` | 通过；构建仍有大 chunk 提示，约 2.60MB 未 gzip，优先级低于正确性 |
| 两个 manifest 的 `cargo metadata --no-deps` | 确认是独立 workspace，根命令不覆盖桌面壳 |
| 壳 `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --quiet` | 通过 |
| 壳 `cargo test --manifest-path src-tauri/Cargo.toml --quiet` | 5 个通过；未启动 GUI |
| 壳 `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | **未通过**：`commands.rs:197/248`、`lib.rs:51/236` 附近格式差异；本轮不修改业务文件 |
| F01 Vue 最小状态复现 | 复现成功，汇总被明细请求作废；未作为仓库新测试写入 |
| 真实日志、缓存重建、安装包/GUI 冒烟 | 未执行，不以静态审查冒充通过 |

评估中曾遇到另一批改动尚未完成时的临时编译失败；最新提交重验已通过，故不把该中间状态列为产品问题。当前仍开放的壳格式检查单独记录。

### 实施后的完整门禁（严格串行）

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
$env:HTTP_PROXY = "http://127.0.0.1:7897"
$env:HTTPS_PROXY = "http://127.0.0.1:7897"

cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test --run   # A1 新增后才可执行
pnpm --dir frontend build
.\frontend\node_modules\.bin\tauri build   # 发布验收时执行
```

CI/脚本应在每步失败时停止，不仅顺序执行命令；任何一步失败都不能标记“全量门禁通过”。普通门禁不运行需要真实用户数据的 ignore 测试。

## 十、需要确认的产品决策与启动建议

以下给出推荐值，**本轮未替用户批准新口径**：

1. **先做 A，再做 B，而不是先接新 Agent。** 优先把当前两个来源做准、做稳。
2. **费用始终叫“估算费用”，请求明确按日志事件口径。** 不承诺等于供应商账单或用户提问次数。
3. **暂不直接改变 sidechain 纳入规则。** 先展示当前排除范围；若要“应用全部用量”，再验证父子链是否重复并建立独立 fixture。无模型但用量有效的记录，建议后续进入未知模型桶，而非永久丢弃，须随 B1 确认。
4. **先支持两个既有工具的可配置目录，保留扩展点。** 多根/新工具按真实需求分批启用；同一项目跨工具关联采用可解释的路径/别名策略。
5. **保持本地优先、Windows 先验收、SQLite 不换。** 价格默认自动同步的既有选择保留，但补齐关闭、错误和同步状态的安全语义。

确认本计划只表示同意后续方向；每阶段实施以任务、测试和退出条件为边界，不自动授权读取真实日志、重建真实缓存、安装系统级自启或对外发布安装包。

## 十一、执行记录

### A 阶段：恢复基本展示闭环及建立回归入口 ✅（2026-10-05，分支 `docs/product-review-plan`）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| A1 前端 Vitest + VTU + mock IPC 测试入口 | vitest 5 / @vue/test-utils / happy-dom；`pnpm --dir frontend test`；Dashboard 五用例 | `c85ee5b` |
| A2 F01 请求代次分离 + 独立加载/错误态 + 重试 | 红→绿：修复前 5/5 红（report 恒 null、summaryError undefined、合计行字面 drill 均复现）；修复后 5/5 绿 | `c85ee5b` |
| A3 F02 明细复用主时间过滤 | 红→绿：新增 `tests/e2e_events_range.rs` 修复前 2/2 红（区间 07-17..17 明细 total=7 ≠ 汇总 2；非法区间不报错）；修复后全绿 | `4572ad5` |
| A4 F03 图表标签与 series 同源排序 | 抽取 `frontend/src/lib/chartData.ts` 纯函数（ECharts 无法在测试环境渲染，纯函数同时解决可测性）；chart_labels_match_series 三用例绿 | `d7d4fc7` |
| A5 门禁覆盖双 manifest + CI | 壳 fmt 欠账修复；钩子新增 壳 fmt/clippy/test + 前端 vitest；`.github/workflows/ci.yml`（windows-latest，同第九节门禁）。**必红验证**：壳注入临时格式错误 → 钩子在「壳 cargo fmt --check」步 exit 1；撤销后全绿。钩子首跑另自查出 `pnpm test --run` 参数重复并修正 | `cfc8a40` |

阶段退出条件核对：三个 P0（F01–F03）均有可复现的红→绿记录 ✅；无快照首次加载可成功、失败可重试 ✅；根库、壳、前端均进入提交钩子与 CI 门禁 ✅。

附注：A3 的合计行守卫（`total_row_clears_drill`）随 A2 提交（同在 Dashboard.vue，测试同文件）；A4 的红基线以源码装配逻辑坐实（标签重排而 series 原序），无法在无 canvas 环境对旧组件直接跑红。

### B 阶段：统计可信度与缓存正确性 ✅（2026-10-05，分支 `docs/product-review-plan`）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| B1+R02 字段映射/请求单位/排除项 | R02 权威证据链闭合：OpenAI 官方文档示例 `input_tokens=15000=cached 12000+cache_write 3000`（cache_write ⊆ input）+ openai/codex#32479（0.145.0 透传、订阅流恒 0——真实日志从未暴露的原因）。归一化改 `input=raw−cached−cw`、守恒式展示总量==raw total；未知字段组合按坏行暴露；缓存 v2 + 版本不符自动清库 | `cd7eb29` |
| B2+R01 去重身份 | 双向 fixture：重播跨批次/跨日仍去重（防线不变）；键加入 model（重播必然同模型），跨模型同用量不再误合并；同模型同用量保守合并显式钉住（事件无请求身份，response_id+差分替代方案待真实多版本日志验证） | `cd7eb29` |
| B3 分项价格未知≠零 | 快照 v2 分项可空 + v1 保守降级；Pricing 分项 Option 化（外置缺键=显式 0、OpenRouter 缓存分项按未知）；CostEstimate{cost,unknown,complete} 贯穿聚合/明细/合计；**顺带接通价格索引读取**（F10/D2 前半，重启命中） | `919e4db` |
| B4 失败可见/不缓存 | discover_with_errors（子目录不可读不再静默）；io_errors 与坏行分列（前端新增"读取失败"行）；解析前后指纹复核，追加期/读取失败文件不写成功缓存（独占句柄红验证 + AppendMock 管线注入） | `6987e93` |
| B5 清理限定/事务 | purge_agent（F06：agent 过滤不再清空他源缓存，红→绿验证 files 2≠4→4）；lookup 读事务 + clear 事务化（200 轮并发压测钉住全或无）；解析版本失效 | `6987e93` |
| B6 黄金对账 | tests/golden_reconciliation.rs：独立手算总账（5 请求/in1690/out455/cw6000/cr17020/cost 0.044268）一处钉住四维分组、区间、明细同源、单双源、缓存三路径 | `3613736` |

**口径说明**：新增 `docs/stats-semantics.md`（请求单位/四桶映射与版本/去重规则与已知限制/费用估算/时区/缓存失效 + 证据链接），后续语义改动须先更新该文件并递增缓存版本。

### C 阶段：多来源产品闭环与可解释展示（进行中，2026-10-05）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| C3 日期控制/清除/取消/时区语义 | F07 四缺陷红→绿：弹层受控化（show 未绑定致草稿永不同步、取消后重开提交残留草稿）；显式「清除」入口（原"全部时间"分支不可达）；快捷条按自然日更名（近2天/7天/14天/30天，替换"24h"混称）；tzDate 用 Intl 按所选时区实际偏移（替换硬编码 +8，UTC/DST 正确） | `dc80cdf` |
| C4 查询快照匹配/刷新 | F08 修复：快照 v2 携带 filters（by/agent/range/drill/tz），eventsKey 记录明细筛选上下文，与汇总一致才落盘（杜绝混代）；恢复连同筛选一起；旧格式整体忽略；「刷新」按钮以当前筛选重跑（筛选不动）。分页游标随 D 阶段采集快照一并交付 | `cfd91a7` |
| C1 来源目录配置/启停/重新检测 | settings 增加 AgentSources/SourceConfig（旧 settings.json 向前兼容）；summary/list_events 注入目录与启停；source_status 四态（ready/missing/empty/disabled）；source_config_set 命令；Settings.vue 数据来源卡（启停+目录+按行保存）；Dashboard 告警按四态呈现 | `f072fd5` |
| C2 稳定项目身份 | Codex 项目身份改完整 cwd（test_project_same_basename_distinct：C:/work/alpha 与 D:/other/alpha 不再合并）；Claude 项目身份改相对根路径（真实 slug 布局行为不变、嵌套可分）；Group.label 展示名（表格首列/图表分类显末段，完整身份保留下钻匹配）；缓存 v3 自动失效 | `c6e57fb` |
| C5 费用解释 UI | UsageTable 出现"未知†"列（按行未计价 token，全计价时隐藏）；脚注覆盖无价格模型与部分计价两种来源；设置页价格表来源行标注"不完整"（B3 incomplete）；排除项/读取失败已随 B4 来源统计可见 | `216686b` |

**C 阶段退出条件核对**：不读教程也能纠正数据目录（C1 四态 + 配置）✅；标签与实际查询一致（C3/C4）✅；用户知道何时更新、遗漏什么（B4 统计行 + C1 四态 + 来源采集统计折叠面板）✅；费用为何只是估算、未知在哪（C5 未知†列 + 脚注 + 设置页不完整标注）✅；主筛选/明细/快照不各自表达不同口径（C4 一致性检查）✅。剩余遗留：C4 的明细分页游标随 D1 采集快照一并交付（已在 D 阶段任务中）。

B 阶段退出条件核对：不同请求同用量与重复回显同时正确（B2 双向 fixture）✅；未知不被当零（B3 三层语义 + 测试）✅；不完整采集可见（B4 发现/读取/统计三层）✅；筛选不破坏另一来源缓存（B5 红→绿）✅；旧缓存不阻止新解析规则生效（v2 版本失效机制）✅。R02 按计划要求未凭空宣布：Anthropic-via-Codex 等未查证组合按坏行计数暴露，明确记录于口径说明。
