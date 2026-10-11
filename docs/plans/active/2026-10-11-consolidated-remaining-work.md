# TokenScope 剩余任务整合与验收计划

> 执行时使用 executing-plans，逐项记录实现、定向验证和交付结果。归档不表示未验项目已经通过。

**Goal:** 先完成用户要求的用量长期保存、全部代理纳入和 CCS 手动导入并集，再完成必要的当前版本验收；不重复执行已完成或已被新需求取代的旧任务。

**Architecture:** 沿用一个 `~/.tokenscope/history.db`，只保存 token 和 cwd 等必要元数据。复用现有采集、导入、查询和隔离验收入口；旧计划保留实施历史，所有剩余责任只在本计划登记。

**Tech Stack:** Rust / SQLite / Tauri 2 / Vue 3，以及现有 Rust、Vitest、浏览器和原生 acceptance 测试工具。

- 整理日期：2026-10-11。
- 状态：**最新 `83d06c0`，本地另有第二轮挑刺审计修复待提交：9 项完成、1 项部分完成、3 项本轮验收未执行，共 4 项待办。** N01–N09 已完成，M10 保持闭合。M06 严格事实校验与 fixture 契约 unittest 26 项通过；M05 生命周期契约 22 项 pwsh 7.6.6 实跑全绿——五个日志反例已接存活替身且断言钉住错误文本，"错因通过"与热路径 DBG 调试输出两项缺口由两轮挑刺审计闭合；不能认定五步已全部实施，N12 待 PR CI 实跑取证。N12 部分完成，当前暂不建议合并 main。详见[设计实施独立复核](../../reviews/2026-10-11-main-merge-readiness.md#install-design-recheck)，此前各轮结果保留为历史。
- 核对范围：原 active 的 25 份计划、对应 audits、M1–M11 归档、D5 清单、当前源码与测试内容、可用的历史 QA 产物和实际 CI/发布记录。
- 整理核对起点：`add8319`；后续修复提交 `3e645f3` 的 7 项已完成独立复核与最终门禁，见[复核记录](../../reviews/2026-10-11-history-seven-fixes-recheck.md)。此前工作区结果保留为历史，不再作为当前交付结论。
- 最近独立复核实现：`a64d3400fa1874659be6351a2a4d7a380a39ef3a`，复核日期 2026-10-12。M01 的完整本地门禁基线为 `a58684c`；本轮只跑新的两类安装契约及相关定向回归，没有重复完整 Rust/前端门禁。下方早期反例与执行账保留发生时的结果，当前状态以任务表及最新复核为准。执行者声明不能代替独立验证。
- 修复设计：[N12 安装验收修复设计](#n12-install-design)已完成，M06 校验器与生成器通过；M05 集成和第五步门禁（构建前契约命令）已闭合，PR CI 实跑取证待完成。不新增第二份 active 计划。
- 总账：[计划目录](../README.md)；逐份结论：[审计索引](../audits/README.md)；发布附属清单：[D5](../d5-acceptance-checklist.md)。

## 1. 优先级与状态规则

1. **用户明确要求高于任何计划和设计建议。** 同一事项以较新的明确要求为准。
2. **本整合计划是唯一当前待办入口，优先于归档旧计划。** 合并冲突时采用较新的已确认专项要求，不恢复旧实现来满足旧勾选。
3. 未被新要求取代的已确认口径继续有效。本次整理提出的优化建议不自动成为必须实施的产品需求。
4. `已实现`、`本轮定向验证通过`、`历史产物已验证`、`待验`分别记账。文件、函数或测试存在不等于行为通过；过去的产物不能证明当前安装包通过。
5. 修复、测试和手工验收完成一项，更新本表和对应审计即可，不重新打开一批旧计划。

必须保持的用户要求：一个数据库；精简用量和 cwd 元数据；全部代理的有效用量都纳入；不建立主/子代理关系产品；日志消失或来源停用仍可查历史；CCS 只在设置页手动点击后执行一次，未点击不读取、不导入；导入与已有历史求并集，重复导入不增加用量。

## 2. 已完成与本轮已验证的部分

| 范围 | 状态与证据边界 |
| --- | --- |
| 历史库主体、全部代理用量、Codex 归档发现、设置页手动 CCS 导入 | 实现存在，Rust/前端已有定向回归；已核验边界见 N01–N09，当前原生/性能仍归 N11/N13，不写成 H01–H08 全部验收通过 |
| 导入 DTO、4096 条明细加日汇总的查询死锁、日汇总日期筛选、分页全量事件驻留 | 此前修复已完成定向回归与日期/死锁隔离探针；当前普通回归继续通过 |
| v2 无歧义 Codex 别名回填、旧观察时间迁移、新原生值优先于 CCS | `3e645f3` 相关回归通过；`85edad2` 又修复旧污染事实，独立旧库探针热扫/重扫恢复正确值，N01 闭合；无来源时仍不猜造终值 |
| CCS-first Codex 并集、日汇总改变来源时区后重复导入 | `3e645f3` 两顺序回归通过；时区重选保持 10 请求/1 行汇总；不能由样例推导所有身份均可精确合并，见 N02 |
| 长 `record_id` 的查询保留预算、百万合成规模 | 长 ID 受保留预算拒绝；`2252eb3` 百万保留与 Rust 构建峰值实测通过，N05 完成；`6688466` 移除净存活误判断言，当前普通门禁全绿，真实性能归 N13 |
| 真实历史提交与退出等待 | 维护中根库/壳侧各 300 文件测试通过；独立临时 SQLite 写锁验证等待、释放后完整落库及 60ms 有限等待，N06 完成 |
| `detail_only` 保存/恢复及实际 Rust serde 的 Settings 载荷 | 此前两项仓库外 Vue 探针通过，维护中的保存/恢复/v10 失效回归当前继续通过；视图版本 11；原生验收仍由 N11 承接 |
| 项目路径与模型等价主体 | source 归一化、项目根推进、分组/下钻、可信显示名、边界计价均有实现和回归；跨应用、有价分页及完整候选文案通过，N08 完成，当前原生验收见 N11 |
| SF/UX、RC/AP 既有修复 | 后续修复闭环已核对。历史浏览器 60 场景/17 契约/243 断言有产物；历史原生 TTL、CSP、无强制参数的当前显示器 150% 有直接证据，不重复列为“从未验过” |
| CI / main 预发布 | 真实 PR CI、main CI、NSIS/校验和/attestation/publish 有成功运行记录；正式版本 tag、当前产物与安装验收另见 N12 |

## 3. 唯一任务状态表

保留 13 个稳定责任编号，其中 N01–N09 已完成，N12 部分完成，N10/N11/N13 本轮验收未执行，共 4 项仍有待办。版本、合并前 PR 入口及 M10 过滤保持关闭，M06 严格校验器与 fixture 已通过；M05 契约、构建前契约命令与卸载后事实验证已闭合（两轮挑刺审计后全绿），当前只剩当前产物的 PR CI 实跑取证（安装工作流与普通 CI 各自记账）；其他系统/性能验收仍按原排期。

| ID | 优先级 | 任务 | 当前状态 | 是否仍有必要 / 来源 |
| --- | --- | --- | --- | --- |
| N01 | P1 | 已污染旧历史库的安全纠错与升级闭环 | 已完成；v4 合并并补别名，解析修订触发一次重解析；旧版本污染库独立探针热扫/重扫均恢复正确值，备份和回滚验证通过 | 必须；H01/H03/H06、历史库复审 |
| N02 | P1 | 请求并集双向一致与独立请求保护 | 已完成；`16beea1` 来源索引补 origin_kind，第四轮两个同键不同种类对账及原五个存储反例通过；保守请求指纹限制保留 | 必须；用户并集要求、H02/H05/H06 |
| N03 | P2 | 遗留缓存非法值受检迁移 | 已完成；负数读取期拒绝并计 rejected；最新 8 项迁移回归通过，独立负数/回滚历史证据保留 | 必须；H03、SF08 |
| N04 | P2 | 导入暂存容量限制与手动错误恢复 | 已完成；请求/汇总读取过程中限制行数；提交失败清除已消费计划，恢复由用户重新预览；后端及组件回归通过 | 必须完成容量保证；错误恢复与本项合并；H05/H08 |
| N05 | P2 | 查询规模验收与准确内存说明 | 已完成；百万 release 保留 88,891,726 B、Rust 分配器构建峰值 123,231,716 B；日期缩窄减少索引保留量，§3.5 已修正；只测前两页，不等于完整 RSS 或真实性能 | 必须验证，测量后再决定优化；H07、RC05 |
| N06 | P2 | 退出等待覆盖所有已开始的采集提交 | 已完成；根库/壳侧真实 300 文件回归通过；本轮独立可控 SQLite 写锁验证实际等待、释放后完整落库、有限超时及活动状态，详见最新复核 | 必须核实并补齐原任务边界；H04 |
| N07 | P2 | 日汇总身份、明细模式及 GUI 修复交付 | 已完成；既有交付证据保留，当前功能回归与普通全量门禁通过 | H01/H06/H08；原生操作由 N11 承接 |
| N08 | P2 | 当前计价说明、模型跨应用与分页验收 | 已完成；Settings 1277 行实际补完整候选优先及回退规则，新增组件文案断言通过；跨应用、原始模型名、有价分页断言维持通过 | 必须；MP04/MP06、较新分段计价规则 |
| N09 | P2 | 测试确定性、路径隔离与当前门禁 | 已完成；`a58684c` 闭合 M01，history.db 路径清单、落盘产物及有效 SQLite 七表校验实测通过；两套 manifest 的普通/acceptance 和前端门禁全绿 | 必须；review-findings Task6、来源策略测试、当前合并门禁 |
| N10 | P2 | 隐私原生取证工具与当前完整浏览器矩阵 | P01–P05 已实现；P06 的强验收未闭合 | 必须；最新隐私计划 P06 |
| N11 | P2 | 最新项目/模型/CCS 原生统一验收 | 分别存在待验，合并一次执行 | 必须；A06/B03、MP06、H08、设计 Task8 |
| N12 | P2（A01–A06 已修，待 PR CI 实跑取证；其余系统验收 P3） | 当前产物的 D5 系统、安装及发布验收 | A01–A06 全部修复：A01 启动函数改名 Launch-AppOnce 并补 -LiveLogsDir；A02 等待读 DataDir/logs（应用实际写入处）与证据目录分离，关闭后 live-logs 复制回证据随 always() 上传；A03 GUI 替身换 notepad（WM_CLOSE → 退出码 0，控制台宿主 0xC000013A 不能当优雅退出）；A04 五个日志反例接存活替身并钉住错误文本（超时/请求数/提前退出的真实路径覆盖）；A05 卸载后 manifest 逐条复核保留事实；A06 install-test.yml 构建前补两条契约命令（M06 unittest 26 项 + M05 生命周期 22 项 pwsh 7.6.6 实跑全绿）。待 PR CI 实跑取证 | 系统验收随发布批次；D5、RC11/AP08 |
| 2026-10-12 | 双审计员循环第一轮（执行） | 完成 | 审计员 B（核心）：P2-1 CCS Codex 捐献按 (scheme,value) 判重修复 + 回归；P2-2 内容等值原生写升级 rank（classify_against）+ 回归；P3 query.rs 额度文案/v3 utc 同步/文档修正。审计员 A（门禁）：P2-1 失败路径复制 live-logs；P3 卸载 30s 轮询、file URI 编码、非 db 退出 2、make_legacy_cache 幂等；.gitignore 补 Python/日志模式。两个审计员均确认无 P1 |
| 2026-10-12 | 双审计员循环第二轮（执行） | 完成 | 审计员 C（热路径+契约）：P2 热路径 7 处 DBG eprintln 删除；P2 M05 反例"错因通过"修复——四个反例接 New-StandInProcess、全部 Test-Throws 钉住 -ExpectedText、反例 1 重构为"归档旧日志→等待超时"并沿用生产证据根命名。审计员 D（集成）：确证同一 P2 并补 P3——test_install.ps1 失败模型注释、安装失败写证据说明、ExpectedRequests 从 manifest 事件数推导、非 CI 运行守卫（Reset-UserHome 保护）、证据动作不掩盖原始异常；v2 迁移 observed_at_utc 回填改 RFC3339（strftime）+ 回归；inspect_history 模式查询统一 DatabaseError 守卫与 NULL/空串区分显示（26 项 unittest）。两审计员均确认无 P1 |
| N13 | P3 | 历史库实际冷/热启动性能基准 | 新历史库真实规模复测未执行 | 有必要，排在正确性之后；H07、旧 D4/性能建议 |

## 4. 各任务实施与验收

<a id="n01"></a>

### N01：已污染旧历史库的安全纠错与升级闭环

- 旧任务内容：升级不能丢事实；补齐持久身份；旧 Claude 采集时间不能永久压住真实终值；修复后无需靠用户手动清库恢复。
- 文件：`src/history.rs`、`src/report.rs`；测试 `tests/history_migration.rs`、`tests/review_regressions.rs`。
- 整理时反例（现已修复）：用原版本生成的合成旧库中，Codex 热扫与重扫均仍为 **2 请求/240 token**；曾回退的 Claude 热扫输出仍为 **6**，重扫才恢复为 **21**。这是隔离复现，不是对用户真实库的扫描结果。
- `85edad2` 复核：旧版本生成的合成污染库经 v4 升级后，Codex 热扫/重扫均为 **1 请求/120 token**，Claude 热扫/重扫均恢复输出 **21**；升级前备份含旧 schema 和事实，迁移失败全批回滚。来源消失时不能还原的终值仍不猜造。
- 步骤：先保留这些合成旧库为版本迁移 fixture；断言升级备份可恢复、迁移原子、无歧义别名正确；再修复已有重复证据/错误终值和必要的重解析触发。不能以新建数据库测试代替升级测试。
- 来源已消失或身份确实不够时保留事实并明确限制，不能猜造终值、直接清库或任意删掉同用量请求。
- 验收：`cargo test --offline --test history_migration`、`cargo test --offline --test review_regressions`；升级、重启、归档移动、重复升级后唯一请求四桶保持正确，旧备份不被覆盖。

<a id="n02"></a>

### N02：请求并集双向一致与独立请求保护

- 旧任务内容：`A={r1,r2}`、`B={r2,r3}` 得到 `{r1,r2,r3}`；native→CCS 与 CCS→native 相同；重复导入、改选备份、源删除后重启都幂等；同时间同用量的独立请求不能被当作可证实交集。
- 文件：`src/model.rs`、`src/history.rs`、`src/import/ccs.rs`、`src/report.rs`；测试 `tests/history_overlap.rs`、`tests/ccs_import.rs`、`tests/review_regressions.rs`。
- `3e645f3` 给 CCS Codex 写入 `session|原始模型|四桶` 保守别名，修好了样例反向重复，维护中两顺序及 proxy/session 别名转移回归通过，但改变了原计划“跨来源身份不足列为候选”的处理范围。该指纹有已披露的独立请求合并限制，不能称为强请求身份。
- `85edad2` 已补不同会话独立保留、保守指纹合并及多来源证据测试；其批量预览/提交分叉、同事实多别名及 Unchanged 漏别名三例已由 `2252eb3` 统一事实模型修复并入回归，本轮独立复跑通过。保留已披露的保守指纹选择，不重复修这三例。
- `2252eb3` 的来源记录改绑与未持久替代事件键两例已由 `6688466` 修复，本轮独立复跑通过，五个原反例全部闭合，不重复实施。
- 第三轮 T01 的批内来源索引漏种类已由 `16beea1` 修复。第四轮独立复跑：不同种类同键插入时预览/提交均两条 Insert；既有事实补证据均两条 Unchanged、两条来源证据；原五个存储反例继续通过，N02 闭合。反例限定存储 API，保守请求指纹和此前证据边界不变。
- 步骤：先补两种顺序、强身份/身份不足、不同来源请求标识但相同四桶、不同时间相同四桶、proxy/session 别名转移的断言；再按已确认并集口径处理差异。复用最小身份元数据，不扩展成代理关系或完整会话存储。
- 验收：上述三个测试文件；交集只算一次，独立事实不因新增来源被错误合并；保守限制和未解决候选如实可见。原生值优先于 CCS，不受两侧时间偏差改变。

<a id="n03"></a>

### N03：遗留缓存非法值受检迁移

- 文件：`src/cache.rs` 的 legacy 读取、`src/history.rs` 的迁移；测试 `tests/history_migration.rs`。
- 整理时反例（现已修复）：负输入被 `.max(0)` 改为 0，迁移结果 `inserted=1/rejected=0`。
- `85edad2` 复核：维护中迁移测试 7 项通过；独立负输入旧库得到 `inserted=0/rejected=1`，零用量写入。合法同行、受检溢出和失败回滚仍有回归保护。
- 步骤：先加负数/溢出/矛盾桶的合成旧库断言，再以受检转换拒绝坏记录并保留诊断；合法同行继续按已确认容错规则处理，整批致命失败回滚且允许重试。
- 验收：`cargo test --offline --test history_migration`；来源库只读，负值不伪装成合法零值，合法历史不丢。

<a id="n04"></a>

### N04：导入暂存容量与手动错误恢复

- 文件：`src/import/ccs.rs` 的 `read_requests/read_rollups/preview` 与暂存计划；`frontend/src/views/Settings.vue`；测试 `tests/ccs_import.rs`、`frontend/src/views/Settings.ccsImport.test.ts`。
- 旧任务内容：一致只读事务、仅白名单字段、有容量限制的流式暂存、一计划一批次、取消/失败不提交、不自动重试；提交失败后允许用户手动重新预览。
- 整理时缺口（现已修复）：读取先生成完整 `Vec` 后才检查行数；失败后已消费计划仍留在界面。
- `85edad2` 复核：`read_requests/read_rollups` 逐行施加上限，超限不写历史；Settings 提交失败后清除旧预览，允许重新手动预览，无自动重试。后端容量测试与失败后恢复组件测试通过。
- 步骤：加超过暂存上限、取消、过期/代次变化、提交失败与手动恢复断言；将限制放在读取/暂存过程中，复用现有只读事务和提交事务。
- 验收：`cargo test --offline --test ccs_import`、`pnpm --dir frontend test`；超限明确拒绝且零用量写入，任何恢复均由新一次手动点击触发，不监听来源、不自动同步。

<a id="n05"></a>

### N05：查询规模验收与准确内存说明

- 文件：`src/query.rs`、`src/report.rs`、`docs/stats-semantics.md`；测试 `tests/history_queries.rs`、`tests/query_memory_budget.rs`。
- 旧任务内容：百万合成历史流式汇总、稳定分页、TTL/预算有效、旧查询冻结。当前事件本体按批读取，但 `SnapshotRow` 全量索引及字符串仍按命中请求数增长，不能称整个查询内存与事件数无关。
- `85edad2` 新增百万合成测试，覆盖汇总、保留记账及前两页 2,000 条，不等于全部百万明细已逐页对账；现有 210 条分页有全量断言，4 万条测试验证汇总而非全量分页。原有 400 条分配器峰值测试不能替代本项要求的百万构建峰值测量。
- 历史 `85edad2` release 复跑为 88,891,388 B，当时未量百万峰值。上一轮 `2252eb3` 独立复跑：1,000,000 行保留 **88,891,726 B（84.8 MiB）**，构建窗口 Rust 分配器峰值 **123,231,716 B（117.5 MiB）**，汇总窗口峰值 **84,272,474 B**；入库 **399,028 ms**、建索引 **43,796 ms**、汇总 **5,637 ms**；命令见第二轮复核。本轮未重跑百万，测量代码保留，只移除预算估算量与分配器实测量的大小断言。
- 本项完成：§3.5 已修正日期筛选说法；第二轮同库 1,000 行保留 **93,350 B**，一天 500 行 **47,878 B**，第三轮日期探针继续通过。Rust 全局分配器测量不覆盖 SQLite C 分配和完整 RSS，不宣称峰值受保留预算限制；百万只检查前两页，4 万条测试验证汇总。400 行净存活误判断言已修且普通门禁通过，真实基准仍归 N13。
- 步骤：保留长标识超预算断言；增加百万条合成记录的汇总、跨页、元数据保留/构建峰值测量；明确预算覆盖范围与失败动作；再按实际瓶颈判断是否需要索引/分页下推。
- 验收：`cargo test --offline --test history_queries`、`cargo test --offline --test query_memory_budget`；无遗漏/重复/预算绕过，不为本任务换 DuckDB，不将瞬时峰值写成已受保留预算限制。

<a id="n06"></a>

### N06：退出等待覆盖全部采集提交

- 文件：`src/report.rs` 统一采集协调器及 `active_collection_submissions`、`src-tauri/src/lib.rs::stop_periodic_collection`、相关 commands；测试 `tests/history_collection.rs` 和壳侧退出测试。
- 旧任务内容：启动、手动、定时共用协调器，退出收尾已开始的提交。整理时活动标志只在定时线程设定。
- `85edad2` 实现已将 `SubmissionGuard` 放在共享领队边界；当时仅有模拟元数据计数验收。`2252eb3` 新增根库/壳侧真实 300 文件提交回归，本轮通过，提交后重开库可见全部 300 条事实。
- 本项完成：第二轮仓库外壳副本仅追加测试/临时构建配置，用 SQLite 写锁验证原样的 `stop_periodic_collection`；等待、释放后落库与 60ms 上限约 62ms 返回均通过，详见第二轮复核。本轮实际提交路径未改，仓内根库/壳侧真实 300 文件回归继续通过，未重复运行外部锁探针；不冒充已入库的可控锁测试或强制终止进程验收。
- 步骤：以可控制的提交屏障验证手动/定时/退出并发；把必要的收尾观察放到共享采集边界；保留有限等待、SQLite 回滚和未同意不打开历史库的约束。
- 验收：`cargo test --offline --test history_collection`、`cargo test --offline --manifest-path src-tauri/Cargo.toml`；真实提交与有限等待已有证据，已提交事实保留，不声称已复现生产数据丢失。

<a id="n07"></a>

### N07：日汇总身份、明细模式及 GUI 修复交付

- **状态：已完成（2026-10-11）。** 修复提交 `3e645f3`；维护中回归、独立探针、口径说明和最终门禁均通过，见[7 项复核](../../reviews/2026-10-11-history-seven-fixes-recheck.md)。保留编号供旧计划承接；原生验收仍在 N11。
- 文件：`src/history.rs`、`frontend/src/views/Dashboard.vue`、`frontend/src/lib/viewSnapshot.ts` 及对应测试；`docs/stats-semantics.md`。
- 旧任务内容：日汇总按来源完整主键幂等；改变时区假设不复制用量；保存/恢复只看请求明细；旧快照失效；覆盖未知不得直接相加或制造请求。
- 完成证据：维护中回归覆盖时区重复导入、v2 仅时区不同的重复汇总迁移和 `detail_only` 保存/恢复/v10 失效；快照版本 11、口径 §6.5.1 和全部门禁已同步通过。
- 验收：`cargo test --offline --test review_regressions`、前端 test/typecheck/format:check/build；同源时区重选仍为 10 次，旧视图不恢复错误模式，日粒度费用/项目未知原因准确。
- 日汇总整桶手动替换入口原计划有描述，当前 `PreferRollup` 未接到 GUI。它无法恢复精确请求并集，**列入第 5 节条件待办**，不以新增高级选项阻塞上述幂等与模式修复。

<a id="n08"></a>

### N08：计价说明、模型跨应用与分页验收

- 文件：`frontend/src/views/Settings.vue`、`src/pricing.rs`、`src/aggregate.rs`、`src/report.rs`；测试 `tests/model_name_equivalence.rs`、`tests/model_identity_contract.rs`、`tests/pricing_source_policy.rs`；`README.md`、`CLAUDE.md`、`docs/stats-semantics.md`。
- 旧任务内容：同一模型等价写法跨 Claude/Codex 合并；分页保留原始模型名，210 条请求的逐请求费用求和与汇总一致；名称和下钻随价格修订更新。
- 整理时覆盖不足（现已补齐）：跨应用样例只启用 Claude；210 条分页缺原始模型名与有价费用总和断言。`85edad2` 的双应用、原始模型名与逐请求费用求和测试本轮通过。
- 本项完成：`2252eb3` 未落地的文案在 `6688466` 的 Settings 1277 行实际修改，明确完整候选优先、无完整候选才回退部分已知费用；新增组件断言通过，独立计价仍正确选择完整候选 `0.00011` 而非部分已知费用 `0.001`。跨应用和有价分页回归维持通过，原生验收仅归 N11。
- 步骤：补上述有区分力的合成断言，统一来源说明；保留等价规则、版本/变体边界、分段/峰谷逐请求计价与未知三态。
- 验收：先按第 6 节查询真实测试目标再运行；费用求和、原始模型和跨应用身份均有明确断言；文案与当前 `estimate` 一致。

<a id="n09"></a>

### N09：测试确定性、路径隔离与当前门禁

- 文件：`src/report.rs` 单飞 panic/follower 测试、`tests/pricing_source_policy.rs`、`tests/partial_pricing.rs`、`tests/native_acceptance.rs` 及其他价格/查询测试。
- 旧任务内容：Task6 用确认信号消除调度竞态，普通测试不依赖真实 `~/.tokenscope`。
- 整理时单飞 `sleep(150ms)` 竞态已用真实航班等待者握手替代，历史 100 次回归通过，不重复实施。`2252eb3` 补了 `partial_pricing` 等集成测试的显式价格路径。
- `6688466` 已修公共 `opts` helper 的缺省价格路径，移除 400 行峰值测试的净存活误判断言，当前普通根库门禁全绿；上一轮红灯保留为历史，不重复列为当前失败。
- 第三轮 T02 已由 `16beea1` 给 9 处直接构造/fixture 显式注入临时 pricing_path 闭合。隔离默认价下修复选项 8 行全部未知，省略选项的对照组 2 行有价；复用当前筛选测试原函数体也通过。不编辑用户真实价格，不把纯时间过滤的无 I/O 选项算入。
- M01 已闭合：`a58684c` 将子进程路径清单、父进程量测标签和落盘产物断言更新为 history.db，保留用户目录指纹与来源隔离检查，并只读验证七张历史表。仓库实际 native_acceptance 三项父测试通过，两个子进程阶段仍按设计忽略；根库全量 acceptance 通过。N10 强取证继续独立待验。
- 步骤：先写/复现调度屏障边界，用真实握手替代时窗；核对路径注入；同步正在推进的版本/DTO 回归，按两个 manifest 与前端门禁完成交付。
- 验收：目标单飞循环和普通价格隔离证据已保留。最新普通根库 456 pass / 10 ignored、壳 42、前端 361 全绿；acceptance 根库 456 pass / 10 ignored、壳 47 通过。双 manifest fmt、普通/acceptance Clippy（`-D warnings`）、前端 typecheck/format:check/build 与 Release metadata 七项通过；Cargo 使用 `--locked`。远端已包含修复，但尚无打开的 PR 或该提交的 PR CI 记录，不能写成远端 CI 已通过。

<a id="n10"></a>

### N10：隐私强验收与完整浏览器矩阵

- 文件：`frontend/scripts/native-acceptance.mjs`、`frontend/scripts/interaction-contracts.mjs`、现有 root/壳 acceptance 入口、`src-tauri/src/privacy.rs`；测试仍只使用隔离合成数据。
- 原 P06 内容保留：未同意不读取业务数据或联网；同意原子保存先于第一次业务 I/O；真实受保护 IPC 拒绝；拒绝/关闭/重启/损坏设置；完整浏览器矩阵与政策产物。
- 工具缺口：`businessFiles` 只列 `cache.db`、漏 `history.db/WAL/SHM`；未同意只等 1500ms，未观察超过 120 秒；缺已有业务文件读取与 Rust 网络计数；Ready 后读到 `true` 只证明终值，不证明落盘顺序；“denied IPC”浏览器场景未真实调用全部后端命令。
- 步骤：补取证白名单与计数/顺序记录；复用隔离 native 两个隐私场景；真实等待跨过门槛并逐项调用当前 27 个业务 IPC；运行当前完整 required contracts/fixture 矩阵，不能以 JSON 的 required 列表代替实际结果。
- 验收：保存版本/参数/产物散列、每条实际断言及失败；既有隐私 JSON 的 1 场景/3 契约/41 断言只作为历史局部证据。验收缺口不等于已证明产品闸门失效。

<a id="n11"></a>

### N11：最新项目、模型与 CCS 的原生统一验收

- 旧 A06/B03、MP06、H08、设计 Task8 的原生待验合并到一个隔离批次。
- 文件/入口：`scripts/prepare-native-acceptance.ps1`、`frontend/scripts/native-acceptance.mjs`、现有 acceptance feature；对应项目/模型/导入 fixture。
- 覆盖：同项目跨工具显示/下钻；子目录归根与越界切换；友好模型名/原始明细名；按日/模型/项目行点击进入明细并清除；设置页仅手动 CCS 预览/确认/取消/失败恢复；重复导入、来源时区、请求明细模式及日级未知；设置页全部分段/峰谷展开。
- 合并 M4/M5/M7/M9 的自身待验：来源与历史状态；重扫/重建派生索引不删历史；打开/创建外置价格文件；价格双源同步成功及单源失败后的条数、时间和全局横幅；首启提示→手动同步→离线重启；`SameAsInput` 与排除不完整候选的实际费用浮层。各行为分别记结果，不用一次“页面打开”代替。
- D5 4.9–4.11 已有开发实例历史记录，不写成从未执行；4.12 的列表渲染与组件展开测试不能冒充全部档位原生展开通过。
- 验收：当前 release+acceptance 的实际 DTO、交互、日志和截图同一产物；来源库只读、写入隔离 TokenScope 目录；无需重建已经不存在的 2026-10-06 截图目录。

<a id="n12"></a>

### N12：D5 系统、安装与实际发布批次

- 从旧 roadmap D5、多个 remediation 真机尾项、RC11/AP08、M3/M8/M11 的自身安装或运行项、CI 方案6 合并，不分别做重复签字；M4/M5/M7/M9 的界面项在 N11。
- 附属规范：[D5 清单](../d5-acceptance-checklist.md)。当前计划持有任务状态，清单记录步骤和签字。
- 最新安装门禁：`a64d340` 的 M06 逐事实校验器及生成器契约实跑 22 项通过，旧的单条 NULL/互换/字面 NULL 反例已能检出；M10、版本、PR 入口及 Release 依赖保持闭合。M05 集成和契约仍有三个 P1 阻断，详见[本轮独立复核](../../reviews/2026-10-11-main-merge-readiness.md#install-design-recheck)。
- 仍需修复：A01 主脚本调用不存在的 Launch-AppOnce；A02 等待 evidence/logs/logs 而应用写 DataDir/logs，且没有上传当前日志；A03 PS7 正例仍以 0xC000013A 退出。A04 五个日志反例缺 Process，须提供存活句柄并限定失败原因；A05 PR 安装构建前缺两条契约命令；A06 卸载后缺 manifest 校验。全部归本项，不恢复旧计划。当前分支无打开 PR，当前 SHA 全事件 Actions runs=0；本轮未执行真实安装或修改用户数据。
- 仍需：新产物安装/保留数据升级/卸载；单实例、关窗/托盘、窗口记忆、自启、休眠恢复；真实 125%/跨监视器、系统深色的新隐私引导首帧和 Ready 后偏好恢复；隐私政策在 100%/125%/150% 可读。
- 已有旧版当前显示器 150%、CSP/TTL 证据保留；强制 WebView device scale 不能签 Windows 系统 DPI，旧偏好首帧不能签当前隐私引导。
- M11 运行验收与隐私批次复用：Ready 后跨过 120 秒观察价格检查/同步，关闭后不再请求，重新开启恢复；按实际快照新鲜度设置合成条件，不把“线程已接线”当作计时通过。休眠恢复与 D5 3.8 同批。
- 真正决定发布时才检查当前 SHA 的 CI、NSIS/校验和/来源证明，以及首次正式 `v*` tag 路线。已有 main 预发布成功，正式 tag 无实跑不等于功能未实现；本轮不制造 tag 或公开 Release 补证据。
- 验收：产物 SHA/版本、日期、场景结果、接受限制及签字齐全；升级不按缓存规则清历史，来源停用不隐藏已存用量；按用户后延决定排期。

<a id="n12-install-design"></a>

#### M05/M06 安装验收修复设计与实施步骤

设计日期：2026-10-11；来源：用户要求设计两个剩余问题的解决方法。状态：**设计完成并实施完毕；2026-10-12 复核发现的 M05 集成缺口与两轮挑刺审计发现的"错因通过"/DBG 缺口已修复，M05 契约 22 项与 M06 unittest 26 项本地实跑全绿；PR CI 取证待完成**。执行时使用 executing-plans，按以下步骤落实；此节继续归 N12，不增加责任编号，也不把建议、源码存在或脚本语法通过记成安装验收完成。

**Goal:** 证明当前安装包启动的那个进程成功查询了保留历史，并证明每个请求的 token/cwd 元数据完整、正确且未重复。

**Architecture:** 验收脚本在每次启动前归档旧日志，使用同一进程句柄贯穿查询、观察与关闭；复用生产代码已有的“汇总完成”日志。历史事实使用独立 JSON 期望文件，按稳定请求身份与只读 SQLite 快照逐条比较。正常发布包沿用现有业务路径，所有验收判定集中在脚本。

**Tech Stack:** PowerShell 7、Python 标准库 unittest/json/sqlite3、现有 Windows Actions 与 NSIS 发布包。

##### 方案选择与源码约束

| 方案 | 代价及取舍 |
| --- | --- |
| **推荐：启动前归档旧日志，再等待本次成功汇总** | Runner 数据可控，归档发生在没有应用进程时；当前日志目录只包含本次输出。复用现有日志，实现范围集中在验收脚本，归档产物还能排障 |
| 启动前记录每个文件的字节偏移，只读新增部分 | 同样可以解决旧日志问题，但必须处理启动时序、文件替换/截断、滚动及不完整 UTF-8；本轮没有保留旧日志在原目录的需求，复杂度更高 |
| 给产品增加带 run_id/PID 的结构化查询回执 | 进程归属最明确，但需要修改正常发布包的观测协议及其测试，当前两个脚本缺口不需要扩大到这一步 |

源码依据：Dashboard 初始 range=null、days=null，query_opts 原样传递，因此首次默认查询覆盖全部历史；可以继续使用 7 月的固定时间戳。report.rs 的“汇总完成（会话 ...）”在历史快照读取与聚合成功后输出，且包含请求数。“历史库写入”只说明采集提交完成，不能用作查询完成的判定。

设计期间看到的 4eb634d 仍在 Start-Process 与 WAL 等待之后记录日志偏移，快速启动时可能已经越过成功日志；其逐条模式还用文本 NULL 代替缺失值。实施必须按下面的启动顺序及 JSON 类型约定处理，不能只照着这个中间版本继续换关键词。

##### 必须同时成立的通过条件

1. **同一实例**：每个场景只启动一次。启动前确认没有 TokenScope 进程；启动后保留 Start-Process -PassThru 返回的 Process 对象，记录 PID、可执行文件路径、版本、场景与 run_id。等待函数不再启动第二个实例。
2. **本次证据**：旧 logs 在启动前移入 Runner 临时证据目录。只解析新 logs 下 tokenscope.log* 的完整 UTF-8 日志行；启动完成前发生的异常、归档失败或来源不明的额外 TokenScope 实例都使场景失败。
3. **真实历史查询完成**：等待当前日志中的首条成功“汇总完成”记录，保存 query_id，并要求请求数精确为期望文件的 3。WAL 存在、时间变化及“历史库写入”仅作诊断，不参与成功判定。
4. **保持存活**：从启动起，到显式请求关闭前，每轮等待及 8 秒观察期都检查同一 Process 对象。提前退出一律失败，包括退出码 0；退出码 0 只有在显式请求优雅关闭之后才有成功意义。
5. **关闭成功**：CloseMainWindow 必须返回 true；在 30 秒内 WaitForExit 返回 true，随后 ExitCode 必须为 0。超时后的强制结束只用于清理失败场景，不能补成通过。
6. **事实逐条一致**：查询完成后及正常关闭后，分别执行一次精确事实校验。实际记录与期望身份一一对应，字段逐个相等，NULL/空字符串/字面字符串保持区别。
7. **成功才记账**：所有条件满足后才写场景 PASS。失败保存阶段、PID、已观察到的 query_id、退出码及字段差异；清理过程不能覆盖失败结果。

关闭语义以 Microsoft 的 [CloseMainWindow](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.closemainwindow?view=net-10.0) 与 [WaitForExit(Int32)](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.waitforexit?view=net-10.0) 为准：前者返回是否成功发送关闭消息，后者返回进程是否在期限内退出。等待与观察使用 Stopwatch，避免以文件时间或系统时钟变化判断健康。

##### M05 的启动、查询及关闭流程

每次启动的证据目录为 `$env:RUNNER_TEMP/tokenscope-install-evidence/<scenario>/<run_id>/`。准备阶段确认没有存活实例，核对归档源确为测试 DataDir/logs、目的地确在本次证据根内，再以 Move-Item -LiteralPath 移动旧目录。移动发生在启动前，失败就中止。历史库、旧缓存、设置、窗口/视图快照继续保留；归档只处理 TokenScope 自身日志。

Write-SeedData 显式配置两个不同的临时空来源目录、隐私已同意、close_action="quit"、关闭价格自动同步，保证实际查询仅命中已知历史事实。只在启动子进程期间设置 RUST_LOG=info，并在 finally 恢复调用者环境。首次启动使用全新视图；重新安装保留此前同一测试生成的全部历史视图，不伪造汇总快照。

以下是实施必须保持的控制流；函数名可据此落在 install_test_helpers.ps1，日志/进程判定不得另起一套旁路：

```text
context = Prepare-LaunchEvidence(dataDir, evidenceRoot, scenario)
try:
    context.Process = Launch-AppOnce(exe)
    Require-ProcessAlive(context)
    summary = Wait-CurrentSummary(context, timeout=180s)
      每轮先检查进程仍活着
      只读取当前 logs 的完整日志行，支持跨日产生多个新日志文件
      没有汇总完成记录就继续等待；首条成功汇总请求数不是 3 就失败
      读到记录后再次检查进程仍活着，返回 query_id 和 requests
    Verify-HistoryManifest(historyDb, expectedJson)
    Observe-ProcessAlive(context, duration=8s, poll=200ms)
    Require-ProcessAlive(context)
    Require(context.Process.CloseMainWindow() == true)
    Require(context.Process.WaitForExit(30000) == true)
    Require(context.Process.ExitCode == 0)
    Verify-HistoryManifest(historyDb, expectedJson)
    Save-ScenarioResult(context, PASS)
catch:
    Save-ScenarioResult(context, FAIL, stage, error)
    清理本场景持有的进程，最终仍返回失败
finally:
    收集本次日志及校验输出，释放 Process 对象
```

将原 Wait-AppMigration 的再次 Start-Process 删除，场景 2、场景 5、配置了固定旧包时的场景 6 共用此流程。场景 3/4 的数据保留也使用同一事实校验器。现有安装、卸载、版本、快捷方式和包哈希断言继续使用原检查。

##### M06 的数据及类型契约

新建独立、手工维护的 `scripts/fixtures/install_history_expected.json`，schema_version=1、events 数组。生成器继续合成 v10 旧缓存，校验器读取这个期望文件；校验器不从实际库生成期望，也不直接把生成器的 EVENTS 当作期望。新增生成器契约测试，直接读回旧缓存核对输入确实符合这份固定规格。

三条事件保持 **3 请求 / 180 输入 / 35 输出 / 2 项目**。为同时验证有值与缺失值，调整为两个会话/两个旧文件，并让 m2 带一个真实子目录 event_cwd：

| session_id | record_id | project_key | session_initial_cwd | event_cwd | input/output | cache_write/cache_read |
| --- | --- | --- | --- | --- | --- | --- |
| s-alpha | m1 | C:/work/alpha | C:/work/alpha | JSON null | 100/20 | 0/0 |
| s-alpha | m2 | C:/work/alpha | C:/work/alpha | C:/work/alpha/src | 50/10 | 0/0 |
| s-beta | m3 | C:/work/beta | C:/work/beta | JSON null | 30/5 | 0/0 |

每条期望还包含 app="claude-code"、model_raw="claude-sonnet-4-5"。时间戳继续使用原三个固定时间。业务身份为 `(app, session_id, record_id)`；按身份比对，不依赖 SQLite 行号或行排序。

在 inspect_history.py 增加严格模式：

```powershell
python -B scripts/inspect_history.py <history.db> --verify-manifest scripts/fixtures/install_history_expected.json
# 0 = 完整通过；1 = 事实差异；2 = 打开/查询/参数/期望文件错误
# PowerShell 在每次调用后立即检查 LASTEXITCODE，非 0 必须 throw。
```

严格模式只读打开库，在一次只读事务内查询 usage_events 的**全部**记录，不加 record_id 非空过滤，不做 DISTINCT，不先转字典覆盖重复身份。查询列为 app、session_id、record_id、model_raw、project_key、session_initial_cwd、event_cwd 与四个 token 字段。缺库、缺表、缺列、空身份、重复身份、额外/缺失记录都失败。SQL NULL 保持 Python None/JSON null，空字符串保持空字符串；所有 cwd 值做原值比较。持久 token 的十进制 TEXT 转受检整数，四桶全部核对，不依赖 SQLite SUM 的浮点/聚合结果。

字段比较核心可以直接按下面的结构实现；读取阶段负责验证字段类型、token 非负/u64 范围及期望文件版本：

```python
KEY = ("app", "session_id", "record_id")
FIELDS = ("model_raw", "project_key", "session_initial_cwd", "event_cwd",
          "input_tokens", "output_tokens", "cache_write_tokens", "cache_read_tokens")

def index_events(rows, label):
    result = {}
    for row in rows:
        key = tuple(row[name] for name in KEY)
        if any(type(value) is not str or not value for value in key):
            raise ValueError(f"{label}: invalid identity {key!r}")
        if key in result:
            raise ValueError(f"{label}: duplicate identity {key!r}")
        result[key] = row
    return result

def compare_events(actual_rows, expected_rows):
    actual = index_events(actual_rows, "actual")
    expected = index_events(expected_rows, "expected")
    problems = []
    for key in sorted(expected.keys() - actual.keys()):
        problems.append({"kind": "missing", "identity": key})
    for key in sorted(actual.keys() - expected.keys()):
        problems.append({"kind": "unexpected", "identity": key})
    for key in sorted(expected.keys() & actual.keys()):
        for field in FIELDS:
            if actual[key][field] != expected[key][field]:
                problems.append({"kind": "field", "identity": key, "field": field,
                                 "expected": expected[key][field], "actual": actual[key][field]})
    return problems
```

输出为有版本的 JSON 校验结果及明确退出码，差异标到请求身份和字段，例如 `s-alpha/m1.session_initial_cwd: expected="C:/work/alpha", actual=null`。PowerShell 只负责调用及传播失败，不能再用 `|` 拼接整行或把 NULL 改成同名字符串来比较。聚合计数作为摘要展示，从已校验记录计算。

##### 必须先写出的反例与正例

| 范围 | 用例 | 预期 |
| --- | --- | --- |
| M05 | 旧日志已有成功汇总，新实例只有 WAL/写库记录或没有新日志 | 失败；归档中的任何关键词均不能满足当前判定 |
| M05 | 新实例刚启动就写出完整汇总，随后一直存活 | 通过等待；证明准备工作位于启动之前，不漏掉快速成功 |
| M05 | 新实例仅写“历史库写入”，没有“汇总完成” | 超时失败 |
| M05 | 本次汇总请求数为 0 或 2，库里仍有三条正确旧事实 | 失败；外部 SQL 读数不能替代本次查询 |
| M05 | 启动等待或观察期提前退出，分别用退出码 0 和 23 | 全部失败 |
| M05 | 新日志跨日、记录分多次写完、UTF-8 字符位于写入边界 | 完整成功记录才通过，部分行继续等待 |
| M05 | CloseMainWindow=false、关闭超时、正常请求关闭后退出非 0 | 全部失败；强制清理后不能改成通过 |
| M05 | 一个新实例、正确汇总、存活观察、请求关闭成功、退出 0 | 通过，并记录同一 PID 与本次 query_id |
| M06 | 单条初始 cwd 改 NULL；两条 cwd 互换；项目改错 | 每种都失败，并定位具体身份和字段 |
| M06 | 期望 null 改成空字符串或字面字符串 "NULL"；非空 event_cwd 丢失 | 每种都失败；正确混合 null/非空值通过 |
| M06 | app/session_id/record_id 改错、空身份、重复身份、额外/缺失记录 | 每种都失败；不能丢弃坏身份后继续核对 |
| M06 | 任一 token 桶改变，尤其只改 cache_write/cache_read | 失败；完整四桶保持独立 |
| M06 | 行顺序或 SQLite id 改变，所有业务事实不变 | 通过 |
| M06 | 库缺失、schema 缺列、期望文件损坏、Python 退出非 0 | 明确失败，不能得到空集合或默认零值后继续通过 |

##### 可直接执行的实施顺序

1. **先固化 M06 契约。** 新建期望 JSON 和 `scripts/test_inspect_history.py`；修改 `scripts/inspect_history.py`。先用当前脚本跑 NULL、互换、字面 NULL、空身份、重复身份等测试并确认红灯，再实现严格读取、类型检查及逐字段比较。使用 stdlib unittest 和临时 SQLite，全程只用合成数据。
2. **增强真实旧缓存 fixture。** 修改 `scripts/make_legacy_cache.py`，按上表造两个文件/两个会话，含 null 与非空 event_cwd；把生成器读回及 3/180/35/2 断言加入同一个 Python 测试文件。输入生成与输出期望保持可独立核对。
3. **抽出可测的 M05 生命周期。** 新建 `scripts/install_test_helpers.ps1` 与 `scripts/test_install_helpers.ps1`。前者只定义函数，不含安装、注册表操作、默认用户目录写入或顶层执行；后者只使用临时目录和受控进程替身。先复现旧日志、快启动、仅写库、早退 0/23、关闭失败，再落实上述单次启动控制流。测试须调用脚本实际使用的同一函数，不能另写一份正确逻辑代替。
4. **接回真实安装场景。** 修改 `scripts/test_install.ps1`，场景 2/5/6 调用同一启动与关闭流程，场景 2/3/4/5/6 使用同一事实校验器。删除等待函数内的第二次启动、将已自行退出 0 当健康的分支，以及数据错误后的默认成功回退；仅在完整成功后生成 PASS 摘要。
5. **接入门禁并签验。** 修改 `.github/actions/checks/action.yml` 加入下面两条定向命令；`.github/workflows/install-test.yml` 在生产构建之前也运行这两条，以免单独手动运行绕过单测。Release 已依赖 gate，因此沿用共享质量门禁与同一安装脚本；在两个安装 job 所在的 install-test.yml/release.yml 中增加 always() 的证据上传步骤，复用仓库已有钉住 SHA 的 upload-artifact 动作。现有 Python/PowerShell 环境配置与依赖锁保持一致，PS1 统一 UTF-8 且 RequiresVersion=7。

```powershell
python -B -m unittest discover -s scripts -p "test_inspect_history.py" -v
# 必须立即检查 LASTEXITCODE，非 0 退出该门禁步骤
pwsh -NoProfile -File scripts/test_install_helpers.ps1
# 必须立即检查 LASTEXITCODE，非 0 退出该门禁步骤
```

每步分别记录红灯、修复和绿灯；测试发现问题时暂停后续集成，不用旧全量绿灯覆盖新的失败。最终运行现有普通/acceptance/前端门禁，在 PR 的 Install test 上验证实际 NSIS 包。记录被构建 SHA、安装包 SHA256、版本、场景、PID、query_id、事实校验结果及退出码，失败日志以 Actions 产物保留；固定旧包未配置时继续明确记为未执行。

**闭合标准：** 以上全部负例确实失败、全部正例通过；当前 PR 产物的安装/首次迁移/重复安装/卸载/重新安装实跑成功，配置固定旧包后其升级场景也成功。设计和单测通过只分别证明设计与脚本契约，M05/M06 的完成需独立验证，N12 的其余系统/发布签字仍保持原责任。

<a id="n13"></a>

### N13：真实规模历史库性能基准

- 文件：`tests/perf_real_data.rs` 与现有隔离验收工具；必要时新增合成规模测量，不新增数据库引擎。
- 旧内容：历史库冷采集/热查询、真实约 1.2 GB 日志、查询/明细响应。旧缓存的 0.12s 数字不是新历史库验收结果。
- 步骤：先完成 N01–N09；使用只读源数据，将输出与价格索引放到临时目录，记录环境、事件数、索引/聚合阶段、冷热耗时与峰值内存。
- 验收：前后用量完全一致，记录可复查结果。已有热启动 1 秒目标可作参考；优化依据实际瓶颈，不为补旧 D4 的 20 次/P95 建一个额外工程。

## 5. 已取代、暂缓与不再单独执行

| 旧任务/建议 | 处理 | 理由与恢复条件 |
| --- | --- | --- |
| 排除 sidechain、区分主/子代理关系；源删/停用/改目录就清用量 | 已取代 | 用户明确要全部用量与长期历史；不恢复 |
| 每项目一个库、完整 JSONL 历史库、CCS 自动同步 | 不实施 | 与用户已确定范围冲突 |
| 内置价格、严格按外置→models.dev→OR 覆盖估算；无边界最长前缀 | 已取代 | 较新候选最高完整费用与模型等价计划优先；修说明，不回退业务 |
| CLI、旧 slug 项目身份、旧游标/缓存/视图具体版本号 | 已取代 | GUI 唯一产品形态、跨工具路径、冻结查询和后续语义版本已经承接 |
| 单行文字筛选、数据摘要、数字右对齐、静态柔光等旧视觉要求 | 已取代 | 最新用户修改与 DESIGN 优先，不为旧截图返工 |
| Apple Task6 的设置/通知 | 已完成承接 | 后续设计 Task7/UX08/RC/AP 已实现，不再列暂缓 |
| TTL、CSP、当前显示器150%原生组合 | 历史验收已完成 | 当前新功能回归只并入 N10–N12，不再单独恢复原任务 |
| 补造旧截图目录、逐份重跑所有旧提交门禁、重复签 D5 | 不再单独做 | 保留真实历史证据与限制，当前批次一次覆盖 |
| 日汇总整桶手动替换高级入口 | 条件待办，暂缓 | 当前保守明细优先和未知覆盖说明先保证正确；只有日汇总不能证明请求交集。实际需要对照汇总口径时再明确排期，不删除原请求事实 |
| DuckDB、自动更新、Gemini/OpenCode/Qoder/ZCode/WorkBuddy 新适配器 | 未排期 | 用户当前重点是已有两来源与历史并集；研究文档不是实施授权 |
| 多设备 CCS 独立数据集、精确还原已消失的原始会话 | 非本期 | 单逻辑来源与精简事实无法恢复缺失原文；有真实需求再单独确认 |

## 6. 实施顺序与验证命令

N01–N09 已完成，不重复实施；M06 严格校验器及 fixture 保持通过，M10 不重复实施。N12 的 A01–A06 已修复且契约全绿（两轮挑刺审计后）；随后取得当前产物的 PR 安装与普通 CI 成功结果，两种验证各自记账。后续做 N10/N11；N12 其余系统与正式签字随发布批次，N13 随性能验收排期。

新增测试名先在实际目标文件中登记，再执行；本计划提出的断言不是已经存在或已经通过的测试。N08 的现有目标为 `model_name_equivalence`、`model_identity_contract` 和 `pricing_source_policy`。

```powershell
# 按任务运行目标测试；测试中的数据/历史/全部价格路径均为临时目录
cargo test --offline --test history_migration -- --test-threads=1
cargo test --offline --test history_overlap -- --test-threads=1
cargo test --offline --test ccs_import -- --test-threads=1
cargo test --offline --test review_regressions -- --test-threads=1
cargo test --offline --test history_queries -- --test-threads=1
cargo test --offline --test query_memory_budget -- --test-threads=1
cargo test --offline --test model_name_equivalence -- --test-threads=1
cargo test --offline --test model_identity_contract -- --test-threads=1
cargo test --offline --test pricing_source_policy -- --test-threads=1

# 最终门禁：根库与壳分别执行
cargo fmt --all -- --check
cargo clippy --offline --workspace --all-targets -- -D warnings
cargo test --offline --workspace
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --offline --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --offline --manifest-path src-tauri/Cargo.toml
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build

# 合并门禁还包括 CI 实际要求的 acceptance 构建和发布元数据测试
cargo clippy --offline --locked --workspace --all-targets --features acceptance -- -D warnings
cargo test --offline --locked --workspace --features acceptance
cargo clippy --offline --locked --manifest-path src-tauri/Cargo.toml --all-targets --features acceptance -- -D warnings
cargo test --offline --locked --manifest-path src-tauri/Cargo.toml --features acceptance
python -B -m unittest discover -s scripts -p "test_release_metadata.py" -v
```

原生/浏览器入口的参数从当前脚本 `--help`/现有准备脚本取得，明确 acceptance 隔离根；不能把普通实例连到真实用户数据当合成验收。未具备安装/系统操作或发布条件时保留 N12 待验，不伪造通过。

## 7. 执行账

| 日期 | ID / 检查 | 结果 | 证据与边界 |
| --- | --- | --- | --- |
| 2026-10-11 | 旧计划与审计整理 | 文档任务完成 | 25 份旧 active 与 M1–M11 标明状态/承接；审计结论更正，链接按归档位置更新 |
| 2026-10-11 | 首轮根库 / 壳回归 | 436 通过、9 忽略 / 41 通过 | 隔离 TEMP/TMP，串行；此后工作区继续修改，不能冒充最终新版本门禁 |
| 2026-10-11 | 历史库/GUI独立探针 | 上表所列子项通过；旧污染事实仍失败 | 全为合成数据/临时库；原 Codex 候选策略断言因新保守别名处理改变而失败，归 N02 核对，不伪称旧契约也已通过 |
| 2026-10-11 | 整理后门禁复跑 | 根库 442 通过/9 忽略；壳 41 通过；前端 358 通过/1 失败 | 根/壳 fmt、壳 Clippy、前端 format:check 通过；根 Clippy 的 `src/query.rs:433` 为 `ptr_arg`，前端 `Dashboard.rollups.test.ts:153/184/221` 类型错误导致 typecheck/build 失败；`viewSnapshot.test.ts:84` 仍期望 10。N09 待闭合，未提交推送 |
| 2026-10-11 | 文档完整性与链接 | 通过 | 36 份历史正文保留（只更新链接），455 个本地链接/138 个任务锚点有效；active 仅 1 份，25 份审计均有当前状态与承接 |
| 2026-10-11 | `3e645f3` 的 7 项修复复核 | 7 项通过，N07 闭合 | 维护中回归与重新运行的独立探针一致；原生优先额外覆盖值完全相等、无 cwd 的两个顺序；旧污染事实和负数迁移仍归 N01/N03 |
| 2026-10-11 | 修复后最终门禁 | 全部通过 | 根库 442 通过/9 忽略、壳 41 通过、前端 359 通过；根/壳 fmt/Clippy、前端 typecheck/format:check/build 通过。取代上方旧失败的当前状态 |
| 2026-10-11 | 复核后文档完整性与链接 | 通过 | 36 份历史正文保留；467 个本地链接/138 个任务锚点有效；active 仅 1 份、13 个编号中 1 完成/12 待办；25 份审计均有状态与承接 |
| 2026-10-11 | 复核 R01–R05 修复（执行） | 完成 | R01 批模拟事实模型（身份→事实 ID、统一终值、全类别证据登记）+ 三反例入回归；R02 时间范围说明改为与实现一致 + 计数分配器量测百万构建峰值（release：构建窗口峰值 ≈123 B/行、汇总窗口 84 MB）；R03 Settings 补完整候选优先；R04 全部普通测试 SummaryOptions 扫查并注入 pricing_path；R05 真实历史提交与退出并发屏障验收（根库/壳侧各一）；附带 read_legacy_cache 列探测兜底（v3 缺列旧结构按"仅迁合法字段"迁移）与旧 agent 别名 claude 兼容，修复偶发 no-such-column 失败面 |
| 2026-10-11 | 第二轮复核 R01/R03/R04 修复（执行） | 完成 | R01 来源记录已绑定其他事实时预览同口径拒绝、非 Insert 不登记替代事件键（两反例入回归，review_regressions 28 项）；R03 Settings 文案实际落地（上轮替换因 prettier 换行不匹配静默未生效，本轮加文案断言测试防回归）；R04 report.rs 单元 helper pricing=None 改临时路径、峰值测试去 live_delta 断言（全局缓存释放使净增量可为负，窗口峰值必为正是唯一稳健断言）、百万测试同类断言同步放宽；并行全量 8/8 稳定（455/10） |
| 2026-10-11 | 第三轮复核 T01/T02 修复（执行） | 完成 | T01 批内来源身份补 origin_kind（resolve 与 register 共用 origin_claim_key，与 UNIQUE(origin_kind, origin_key) 同域）+ 两场景预览/提交对账回归；T02 report.rs 九处直构选项注入 pricing_path（含两个游标 fixture 与 append/no-reparse 稳定性测试），apply_time_filter 纯过滤构造与 acceptance/perf 默认路径用例按口径豁免；并行全量 6/6 稳定（456/10，含新回归） |
| 2026-10-11 | 第二轮复核 M02–M07 修复（执行者声明） | 声明完成；独立复核仅四项闭合 | M02 install-test needs 补 [gate, build]（outputs 直依规则）；M03 workflow 调用路径的实际制表符（	 转义事故）清除并全文扫零；M04 make_legacy_cache 创建数据父目录；M05 场景 5 改 -wal 出现证明新实例打开既有库 + 观察期后无重复断言；M06 cwd 断言从计数升级为路径级（inspect_history --projects）；M07 旧包 SHA 必填且不匹配在安装前中止；新增 install-test.yml（workflow_dispatch）支持合并前对当前产物实跑安装验收；utf8NoBOM 兼容性声明纠正为 #Requires -Version 7（pwsh 默认 shell） |
| 2026-10-11 | 第三轮复核 M05/M06/M08/M09 修复（执行者声明） | 声明完成；独立复核部分通过 | M05 Wait-AppDbUse 以 -wal LastWriteTime 晚于启动为准（遗留 WAL 一直存在不算数）；M06 --projects 模式 + 严格集合相等断言（本地验证 3/180/35/2 与两条路径）；M08 install-test.yml 版本改读 tauri.conf.json（分支名不是版本）；M09 增加 pull_request 触发 + paths 过滤（工作流随 PR 落入 main，M09 的默认分支规则随之满足，PR CI 即合并前安装实跑）；install-test.yml 触发器/步骤经 YAML 解析验证 |
| 2026-10-11 | 第三轮复核 M05/M06/M10 修复（执行者声明） | 声明完成；独立复核部分通过 | M05 Start-AppHealthy 三健康信号（-wal 更新、日志"历史库写入"、优雅关闭退出码 0）+ Close-AppGracefully，写库后崩溃不再误通过；M06 inspect_history --cwd 模式 + 三字段断言（project 严格集合、session_initial_cwd 严格集合、event_cwd 全空）；M10 install-test.yml PR paths 补 src/**、Cargo.toml、Cargo.lock、.github/actions/**（当前 PR 改动命中过滤，PR CI 即含安装实跑） |
| 2026-10-11 | 第四轮复核 M05/M06 修复（执行者声明） | 声明完成；完整独立验收待执行 | M05 Start-AppHealthy 启动前记录日志文件偏移、只扫描**新增字节**找"历史库写入"信号（借用旧日志的隔离进程不再通过），wal 更新与优雅关闭退出码 0 保留；M06 inspect_history --records 按 record_id 逐行输出 project/session_initial_cwd/event_cwd/input/output（缺值 NULL），断言与已知三行**严格集合相等**——单行初始 cwd 置 NULL、两条互换、event_cwd 改错均不再漏检（本地以当前 schema 构造库验证 --records 输出与预期逐字相等）。本轮源码核对发现偏移实际在启动/WAL 等待后记录，完整通过条件以新增设计为准 |
| 2026-10-11 | N12 安装门禁接入（执行） | 完成（安装部分） | Release workflow 新增 install-test job（build 与 publish 之间）：下载同一 artifact、校验 SHA256、按场景验收（全新安装/首启迁移/重复安装/卸载/重新安装 + 可选固定旧包升级）；迁移断言查请求数、token、cwd 而非文件存在；scripts/make_legacy_cache.py 合成 v10 旧数据、scripts/inspect_history.py 提供验收读数；PS1 带 BOM+CRLF（PS 5.1 中文系统兼容）。真实 DPI/跨显示器/休眠/自启保留本机或自托管；N12 不因此整项标完成 |
| 2026-10-11 | 第四轮 M01 修复（执行） | 完成 | native_acceptance 的默认数据库路径检查、量测标签与落盘产物断言从 cache.db 更新为 history.db，并核对有效 SQLite 历史库（usage_events/event_aliases/event_origins/source_files/ccs_daily_usage/import_runs/history_meta 七表）；workspace+acceptance 门禁通过（456/10） |
| 2026-10-11 | 文档整理与复核交付范围 | 仅本批文档 | 本次不改产品代码；同步当前审计与整合计划，保留其他正在推进的研究改动 |
| 2026-10-11 | N01/N02/N03（执行） | 完成 | v4 迁移（Codex 重复行合并 + 别名补齐）、解析规则修订触发重解析、污染 fixture（2 请求/240 → 1/120；终值 6 → 热扫恢复 21）、负数受检拒绝；`history_migration` 7 项通过 |
| 2026-10-11 | N04/N06/N09（执行） | 完成 | 暂存上限读取期生效（超限零写入）、提交失败清除已消费计划、提交观察移共享边界（握手测试）、单飞真实握手循环 100 次、unknown-only 价格路径隔离；`ccs_import`、`pricing_source_policy` 通过 |
| 2026-10-11 | N02 批感知预览（执行） | 完成 | `classify_batch` 按提交顺序分类：批内重复按未变化呈现、净新增不虚增；身份矩阵（独立保留/保守合并/多来源一条事件）通过 |
| 2026-10-11 | N05 规模测量（执行） | 完成 | 百万级合成：≈88 B/行、100 万行保留 ≈85 MiB（256 MiB 预算内）、行索引构建 77s、流式汇总 21s（debug 构建）；测量测试 `--ignored` 手工运行，默认套件由 4 万条流式测试守护；§3.5 补记线性增长与失败动作 |
| 2026-10-11 | N08（执行） | 完成 | Settings 三处计价文案对齐候选语义；跨应用等价合并、210 条分页原始模型名 + 逐请求费用求和断言通过 |
| 2026-10-11 | N01–N09 后门禁 | 见提交 | 根/壳 fmt、clippy、test 与前端四项门禁；N07 维持完成；N10–N13 未执行（原生/发布/真实规模批次，见第 6 节顺序） |
| 2026-10-11 | `85edad2` 实施后独立复核 | 4 完成 / 5 部分完成 / 4 本轮验收未执行 | N01/N03/N04/N07 完成；N02 批量预览、N05 峰值/说明、N06 退出并发验收、N08 完整候选文案、N09 价格隔离未闭合；N10–N13 的未执行状态与实施账一致。详情见实施后复核 |
| 2026-10-11 | 实施后普通门禁 | 全部通过 | 根库 448 通过/10 忽略、壳 41 通过、前端 360 通过；根/壳 fmt/Clippy、前端 typecheck/format:check/build 通过；根/壳使用默认并行测试 |
| 2026-10-11 | 实施后隔离反例 | 3 个批量分类反例失败、2 个口径探针通过 | 真实 CCS 合成库的预览/提交不一致；同事实多别名和未变化行登记反例走直接存储 API，不冒充均由现有 CCS 行形态触发。保留量缩小与完整候选优先探针通过；均为临时库/价格 |
| 2026-10-11 | N05 百万合成独立复跑 | 现有测试通过，责任仍部分完成 | release：1,000,000 行、保留 88,891,388 B（84.8 MiB），入库 401,484 ms / 索引 44,713 ms / 汇总 5,422 ms；测前两页，不测构建峰值，不冒充真实历史基准 |
| 2026-10-11 | 实施后文档核对 | 通过 | active 仍仅 1 份，13 个编号/4 完成/9 待办一致；36 份归档历史正文保留，25 份审计入口与本地链接/任务锚点有效 |
| 2026-10-11 | `2252eb3` R01–R05 修复独立复核 | 6 完成 / 3 部分完成 / 4 本轮验收未执行 | R02/N05、R05/N06 闭合；原三个 R01 反例通过，另两个存储 API 登记反例失败；R03 文案实际未修改，R04 单元 helper 隔离遗漏。上方执行者“完成”声明按最新复核纠正 |
| 2026-10-11 | `2252eb3` 普通门禁 | 根库失败，壳/前端通过 | 根库中止前 387 pass / 1 failed / 8 ignored，峰值净存活断言失败；壳 42、前端 360 通过，根/壳 fmt/Clippy、前端 typecheck/format/build 通过；保持当时失败状态，不提交推送 |
| 2026-10-11 | `2252eb3` 目标回归与峰值跟进 | 目标回归通过，误判条件确证 | review_regressions 26、history_migration 8 通过；峰值单测、整个目标串行/并行及 20 次默认并行目标复跑通过。独立价格缓存释放使净存活 −921,764 B，但新查询 400 行/保留 36,029 B；不追认首次失败的具体释放对象 |
| 2026-10-11 | `2252eb3` N05 百万 release | 通过，N05 完成 | 1,000,000 行，保留 88,891,726 B；Rust 构建峰值 123,231,716 B、汇总峰值 84,272,474 B；入库 399,028ms / 索引 43,796ms / 汇总 5,637ms；只测前两页，非完整 RSS/真实性能 |
| 2026-10-11 | `2252eb3` N06 可控提交/超时 | 通过，N06 完成 | 仓内真实 300 文件测试通过；仓库外壳副本以临时 SQLite 写锁验证原退出等待：锁持有时等待、释放后完整落库，60ms 上限约 62ms 返回且活动提交保持可见 |
| 2026-10-11 | 第二轮复核文档完整性 | 通过 | 68 份计划/审计及关联复核文档 UTF-8 可读；454 个本地链接、138 个任务锚点有效；active 仅 1 份、13 编号/6 完成/7 待办一致，36 份归档文件与当时 HEAD 完全一致 |
| 2026-10-11 | `6688466` 第三轮独立复核 | 7 完成 / 2 部分完成 / 4 本轮验收未执行 | 原五个存储反例全部通过，Settings 文案实际修复，N08 闭合；N02 批内来源键漏 origin_kind，N09 直接构造选项漏价格路径，新增三个探针失败，详见第三轮复核 |
| 2026-10-11 | 第三轮普通门禁 | 全部通过 | 根库 455 pass / 10 ignored、壳 42、前端 361；根/壳 fmt/Clippy、前端 typecheck/format:check/build 全绿；当前门禁取代上一轮红灯，新增反例结果另记 |
| 2026-10-11 | 第三轮独立存储/价格验证 | 原五例与两口径探针通过；三个新增断言失败 | 混合种类同来源键：空库预览 Insert/Update、实际插入两条；既有事实补证据预览误冲突、实际两条 Unchanged/两证据。隔离默认价使 8 行中 2 行有价；不冒充已证明现有 CCS 行触发混合种类 |
| 2026-10-11 | 第三轮文档完整性 | 通过 | 69 份计划/审计及关联复核文档 UTF-8 可读；462 个本地链接、138 个任务锚点有效；active 仅 1 份、13 编号/7 完成/6 待办一致，36 份归档文件与 HEAD 完全一致 |
| 2026-10-11 | `16beea1` 第四轮修复复核 | T01/T02 闭合，N02 完成；N09 门禁仍待修 | 原五个存储反例、同键不同种类两例、口径与价格隔离共 11 项仓外验证通过；当前 8 完成 / 1 部分完成 / 4 本轮验收未执行 |
| 2026-10-11 | 第四轮完整合并门禁 | 普通通过，acceptance 根库失败 | 普通根库 456/壳 42/前端 361；acceptance 根库 455 pass / 1 failed / 10 ignored、壳 47 pass；两套 Clippy 和发布元数据 7 项通过，唯一失败为旧 cache.db 断言 M01 |
| 2026-10-11 | 第四轮 main 可行性 | 可快进；暂不建议合并 | fetch 后 main 为 HEAD 祖先，0 behind / 50 ahead；远端功能分支落后 3 个修复提交，未发现打开 PR。实际隔离子进程成功生成 history.db；只更新断言的仓外副本 3 pass / 2 ignored，仅用于确认根因；未修产品或执行合并 |
| 2026-10-11 | 第四轮文档完整性 | 通过 | 70 份计划/审计及关联复核文档 UTF-8 可读；471 个本地链接、138 个任务锚点有效；active 仅 1 份、13 编号/8 完成/5 待办一致，36 份归档文件与 HEAD 完全一致 |
| 2026-10-11 | `a58684c` M01 独立复核 | 通过；9 完成 / 0 部分完成 / 4 本轮验收未执行 | 实际 native_acceptance 三项父测试通过，路径清单、history.db 落盘及七表只读校验生效；旧失败仅保留为历史，N09 闭合 |
| 2026-10-11 | M01 后完整合并门禁 | 全部通过 | Cargo 均带 `--locked`；普通根库 456/10 忽略、壳 42、前端 361；acceptance 根库 456/10 忽略、壳 47；双 manifest fmt/普通与 acceptance Clippy、前端 typecheck/format/build、Release metadata 七项通过 |
| 2026-10-11 | M01 后远端与合并条件 | 修复已推送，可快进；PR CI 尚未运行 | 被审实现与远端功能分支均为 a58684c，main 为 8d1d90d；0 behind / 51 ahead。只读查询没有打开的 PR，也无该提交的 PR CI；本机 gh 缺失不等于不能使用 GitHub 连接器创建 PR。本轮未创建 PR、合并 main 或触发发布 |
| 2026-10-11 | `b16437c` 安装门禁补充核对 | M02–M04 未闭合，N12 部分完成 | YAML 解析查出 install-test 三处读取非直接依赖 gate；PowerShell AST 将调用拆成 .\scripts 与 est_install.ps1；实际合成脚本在缺父目录时退出 1（unable to open database file）。未运行真实安装，不把静态验证当 Runner 实跑；当前暂不建议合并，9 完成 / 1 部分完成 / 3 本轮验收未执行 |
| 2026-10-11 | 两份执行答复统一复核 | M01 再次通过；M02–M07 未闭合 | M01 3 pass/2 ignored、metadata 7 pass；M02–M04 复现。提取原 PS1 函数：模拟应用退出码 23、旧库不变、两项目路径均错误，等待返回 3/180/35/2 且四断言通过；提取原旧包块：缺 SHA 零失败/安装两次，错 SHA 一个失败/仍安装两次。仅 mock 外部安装/网络/重置动作，未操作真实用户数据；BOM/CRLF 属实，PS5.1 的 utf8NoBOM 参数不兼容另记边界 |
| 2026-10-11 | `cc7b7d3` 安装修复独立复核 | M02/M03/M04/M07 闭合；N12 部分完成 | 两份 workflow needs/路径/无制表符通过；缺父目录造 v10 库为 3/180/35/2；旧包缺/错 SHA 安装调用均 0，正确 SHA 为 2/零失败；版本声明 7.0。M05 遗留 WAL+新实例退出 23 仍六断言通过，M06 错误路径后缀仍六断言通过；M08 分支名版本断言失败，M09 默认 main 无手动工作流入口。实际 acceptance 3/2、review_regressions 29、metadata 7 通过；全事件 Actions 当前 SHA 运行 0、无打开 PR。9 完成/1 部分完成/3 待验；未运行真实安装、不提交推送 |
| 2026-10-11 | `e99a4f3` 第三轮安装独立复核 | M08/M09 闭合，M05/M06 部分修复；N12 部分完成 | 实际版本步骤输出 0.1.0，当前 PR 入口有效；旧 WAL 不变超时，错误项目路径失败 1。写 WAL 后退出 23、真实等待 8 秒仍五断言通过；初始/事件 cwd 全改错也五断言通过。新增 M10：根 src/Cargo/setup 单独修改不匹配安装 paths。acceptance 3/2、review_regressions 29、metadata 7 通过；无 PR/Actions，0 behind/54 ahead；9 完成/1 部分完成/3 待验；未运行真实安装，不提交推送 |
| 2026-10-11 | `b875815` 第四轮安装独立复核 | M10 闭合，M05/M06 部分修复；N12 部分完成 | 退出 23 失败 1，整批 cwd 损坏失败 2，八条输入路径均匹配；旧日志加只写辅助表后退出 0 仍十断言通过，单条初始 cwd 丢失或两条互换各七断言通过。迁移 8、acceptance 3/2、review_regressions 29、metadata 7 通过；无 PR/Actions，0 behind/55 ahead；9 完成/1 部分完成/3 待验；未运行真实安装，不提交推送 |
| 2026-10-11 | 用户要求的 M05/M06 完整修复设计 | 设计完成；实现及 Actions 待验 | 归入 N12：启动前归档旧日志，单个 Process 对象等待成功汇总/观察/关闭；独立 JSON 规格按 app/session_id/record_id 核对全部 cwd/四桶，区分 null/空字符串/字面 NULL；包含具体文件、红绿测试顺序及失败证据。16 组仓外设计算法/文件系统实验通过，不代表实际 PS7/NSIS 验收通过；当前仍有未闭合项，本批文档不提交推送 |
| 2026-10-11 | N12 修复设计实施（M06 契约 / M05 生命周期 / 场景接回 / 门禁接入，执行者声明） | 声明完成；独立复核未通过 | 执行者报告期望 JSON + --verify-manifest 22 项 unittest、PS7 生命周期 21 项断言全绿，场景共用生命周期、checks 与安装构建前契约接入。原声明保留为历史；2026-10-12 核对只有 M06 的 22 项通过，M05 测试退出 1，实际启动/目录及门禁尚有缺口，以后一条独立复核为准 |
| 2026-10-12 | a64d340 N12 设计实施独立复核 | M06 契约通过，M05 集成及门禁未完成 | Python 22/metadata 7、Rust 迁移 8/acceptance 3（2 忽略）/回归 29 通过；实际 PS7 7.6.6 契约退出 1、完整生命周期正例退出 0xC000013A。真实 Run-AppLifecycle 抽取复现启动函数不存在；当前日志写入临时 DataDir/logs 仍等待超时；五个日志反例都因缺 Process 抛错。PR 安装 workflow 无两条前置契约，卸载后无 manifest。无打开 PR/当前 SHA 全事件 Actions 0，main 0 behind/57 ahead；9 完成/1 部分/3 待验。详见设计实施复核 A01–A06；本批不提交推送 |
