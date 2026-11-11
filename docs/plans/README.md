# TokenScope 计划与完成状态总账

> 更新日期：2026-11-10。**用户明确要求 > 较新计划 > 较旧计划 > 未确认的设计建议。**
> 唯一当前待办入口：[剩余任务整合与验收计划](active/2026-10-11-consolidated-remaining-work.md)。真实跨午夜、长时间休眠、跨显示器在[独立系统验收待办](system-acceptance-todo.md)维护，均未排期、未标通过。[审计索引](audits/README.md)保留历史时点结论。

## 现在执行什么

**2026-11-10 当前优先：筛选等待、图表动画与重复纠错备份（已实施 0.7.5，PR #61 检查全绿）**：[修复计划](active/2026-11-10-filter-query-chart-performance.md)。F01 收窄纠错备份触发，F02 合并尺寸/数据更新以保留柱形动画，F03 减少筛选查询的重复扫描与等待。定向回归与真实 ECharts 中间帧已通过；同规模合成基准暖态 p50：全部时间按日 1079→485 ms、按项目 1159→469 ms、按应用 1060→477 ms、按模型 1114→489 ms、7d 645→144 ms、24h 635→36 ms（计划 §9）。保持统计/计价/CCS/冻结分页语义，不清理真实库或旧备份；停在可审查 PR，尚未自动合并或发布。

**2026-11-10 原生启动空白取证（已随 v0.7.4 发布，根因未闭合）**：[启动诊断计划](active/2026-11-10-native-startup-diagnostics.md)。PR #60 已合并（`46f0dc7`），Release 构建、安装、12 组单跳与双链通过；补验收宿主阶段标记、独立 HTTP/CDP 状态与失败前最小线程转储，保持 45/20/65 秒启动预算及原失败判定。尚未取得本项目同次故障栈，不把运行成功记为根因已修复；不改统计口径。

**2026-11-09 交付遗漏修正（已随 v0.7.3 发布）**：[CCS 历史补充与来源选择交付修复](active/2026-11-09-ccs-history-delivery-repair.md)。PR #56 已合并（merge commit `58487a4`）并发布 **v0.7.3**：build、install-test、12 个单跳升级矩阵、present/deleted 连续升级链与 publish 全绿；标签/资产/SHA256 逐项核验一致（计划 §6）。真实一致副本对账：61 个无明细桶（24,100 请求 / 4,337,748,027 token）全部计入，三个重叠桶全部 A 类不再误报，冻结 CCS 副本重复导入第二次零增量（整体净增 71 请求 / 19,345,530 token；6 条新汇总涉及的 5 桶计入 249 请求，不直接相加），65 条已导入日汇总与来源四桶 0 差异（计划 §5）。升级后已导入 CCS 历史按新规则统计，无需清库重导；用户安装状态独立，不得提前宣称。

**2026-11-08 专项（已实施并随 v0.7.2 发布）**：[Codex 正常请求身份与漏计修复](active/2026-11-08-codex-request-identity.md) K01–K05 交付。新格式 `token_usage_record` 的 `(thread_id, response_id)` 建立强响应身份：普通会话/工具循环/回退/legacy fork 中不同响应同用量分别计入，同一响应副本只计一次，record/count 两观察合计一条事实；旧库误合并的弱事实经镜像来源键在纠错备份+事务内原行升级，无法映射的保持原值并披露。Codex `/btw` 用量获取按用户决定只登记为 **TODO / 暂缓**（见专项 §4），不实现遥测接入、不作为本批交付阻断；与既有 N01–N18 责任编号的历史计数分开记录。**2026-11-09 追加修正**：新 turn 边界后的同用量 count-only 不再经镜像来源键误合并（2200/2），旧库同形态升级为 A+B 两条；无新 turn 边界且无其他身份证据的歧义仍保留限制（见专项 §7）。**同日合并并发布**：PR #57 merge commit `39f4478`，Release `v0.7.2`；12 个单跳升级矩阵与 present/deleted 两条连续升级链全绿（专项 §8）。

已关闭任务不重做；CCS 来源选择修复已随 v0.7.3 发布（见上）。2026-11-04 复核：N13 按 AC09 原验收口径判定关闭、N18 随 [v0.6.1](https://github.com/ywltby/tokenscope/releases/tag/v0.6.1) 预发布关闭；用户协议重新同意专项（PR #52/#53，v0.6.1）归档 implemented。当前责任状态如下，N14 已随 B1 关闭：

| 责任 | 当前状态与下一步 |
| --- | --- |
| N10 隐私强验收 | P01–P05 已实现；P06 未闭合。2026-10-25 分批实施：AC06 + AC08 已实施（提交 `0191f78`/`17fb37e`/`af784bf`，0.6.1，已随 PR #52/#53 合并），AC07 整项暂缓（进程内恢复保留待验），N10 不关闭 |
| N11 原生统一验收 | B2 / PR #31 自动化通过，合并 `c9de40e` / v0.5.9，PR/main 门禁及发布验收全成功。安装后手动同步 OpenRouter 的人工尾项仍待验（最少操作清单见整合计划 §4 末尾），N11 不关闭 |
| N12 系统与发布签字 | 最新 [v0.6.1](https://github.com/ywltby/tokenscope/releases/tag/v0.6.1) 的安装五场景、八组单跳（含 0.6.0→0.6.1 history v5 迁移）、两条连续链与发布成功（[Release run 41651288318](https://github.com/ywltby/tokenscope/actions/runs/41651288318)）；AC10 已接入 CI 并在 main 通过（`ui-single-instance` 19/19 + 3 项托盘展示层 pending，检查器「部分验证（放行）」，[run 41651288477](https://github.com/ywltby/tokenscope/actions/runs/41651288477)）；其余系统验收与人工签字待验（操作清单见整合计划 §4 末尾），N12 不关闭 |
| N13 真实性能 | **已关闭（2026-11-04 按 AC09 原验收口径判定）**：真实测量（只读 `~/.claude`/`~/.codex`，写入全部隔离临时目录）111 文件 / 33240–33409 请求，冷/热四桶一致、重叠根不重复计数（第 1、2 轮来源未变化干净成立）、热查询 230–289 ms 远低于 1 秒参考值（不触发登记优化项）；三轮汇总数字均记录于计划，脱敏证据文件仅第三轮留存 `qa-artifacts/n13-real-perf/`（该轮重叠段受来源变化影响已标注，前两轮只有汇总数字）；测量边界如实保留为限度说明，不是未满足的验收条件；未设新门槛、未补测 |
| N14 来源目录遗留 | 已完成；第 4 项共享冲突读取失败 fixture 随 PR #27 合并 `4198051`，PR/main 门禁通过，四项全部关闭 |
| N17 WorkBuddy 双来源 | 开放（仅剩人工验证）：已合并发布（2026-10-25），PR #49 合并 `44c026d`，[v0.6.0](https://github.com/ywltby/tokenscope/releases/tag/v0.6.0) 预发布，main CI/原生/Release 与八组单跳+两条链全过；用户手动安装验证待执行（对账数字按当次样本基线比对；最少操作清单见整合计划 §4 末尾） |
| N18 Claude 缓存写分档计价 | **已关闭（2026-11-04），随 [v0.6.1](https://github.com/ywltby/tokenscope/releases/tag/v0.6.1) 预发布交付**：PR #53 合并 `3d11fa5`，main CI/原生/Release 全成功；约定验收全部满足——定向测试、隔离端到端三用例（`n18_e2e_tiers.rs`）、真实界面验收（`ui-cache-tiers` 17/17 含汇总「分档估算」pill 截图）、旧库迁移由真实升级路径（0.6.0→0.6.1 单跳 + 两条连续链）覆盖；分档计价另由 pricing.rs 单测与 ui-pricing 场景覆盖。原缺陷（1h 写费低估 37.5%）已修复 |

N01–N09、N14–N16、N13、N18 已关闭；18 个责任编号中为 **14 项完成、4 项开放（N10–N12、N17）**：N10 的 AC07 继续暂缓，N11 剩安装后手动同步 OpenRouter 尾项，N12 剩其余系统验收与人工签字（AC10 自动化部分已接 CI 通过），N17 已合并发布 v0.6.0 但仅剩用户手动安装验证。

## 2026-11-04 用户协议重新同意专项归档（随 v0.6.1 交付）

[用户协议更新后的重新同意实施计划](archive/implemented/2026-10-28-consent-updates.md) 四个任务全部交付：共同同意修订 3（隐私政策 + 用户许可协议精确匹配、协议同意字段）、协议变更版本守卫、升级编排按目标版本能力装配与原生验收（含 `privacy-revision2-consent` 场景）随 [PR #52](https://github.com/ywltby/tokenscope/pull/52) 与 [PR #53](https://github.com/ywltby/tokenscope/pull/53) 合并 `3d11fa5`，并经 main Release 实跑发布 [v0.6.1](https://github.com/ywltby/tokenscope/releases/tag/v0.6.1)（预发布，2026-11-03，安装五场景、八组单跳、两条连续升级链、publish 全过）。AC07 暂缓、N10 开放与真实自启 D5 3.4 人工签字等边界由整合计划与收口计划承接。

## 2026-11-04 界面双语（i18n）与 AC10 豁免范围修正（PR #54）

[界面双语实施计划](active/2026-11-04-i18n.md) 的 I01–I08 已交付：`vue-i18n` 集中资源 + `lib/translate.ts`（纯函数库安全复用）、`settings.toml` 新增 `language` 与受闸门保护的 `settings_set_language`、界面/无障碍名称/Naive 组件语言/`html[lang]` 全量接入、托盘菜单双语且保存后重建。默认简体中文；同意前只允许内存切换（零持久化 IO，既有 `consent_screen_*` 断言不变），同意成功后先写后读权威值；语言与统计时区严格分离。隐私政策与用户许可协议正文仍是简体中文准据版本，英文界面显式标注，不声称已完成法律文本翻译。安装器改为**自动跟随系统语言**（`SimpChinese` + `English`，不弹选择框；静默 `/S` 的生效语言仍未取证）。前端 1079 例、Rust workspace、clippy 双模式与 PR 必需检查（`gate`/`contracts`/`frontend`/`rust`/`rust-acceptance`/`native-ui`/`install-test`/`upgrade-test` 矩阵）通过；第二轮定向修复（重新同意保留弹窗最后选择、语言保存串行顺序、政策更新提示/费用浮层单位与无障碍名称英文化）已验证，前端全量 1079 例；`upgrade-chain` 按设计跳过。一次 `upgrade-test (0.4.0, present)` 因历史库 `VACUUM INTO` 备份目标已存在而失败、重跑通过，根因未定位，如实记录为竞态观察。

AC10 验收脚本的 SQLite 瞬时文件豁免同时收窄：只在**受控历史库**（`<隔离根>/tokenscope/history.db`）的 `-journal` / `-wal` / `-shm` 且 `ENOENT` 时豁免，两个检查阶段行为一致；其他路径、其他错误、存在时的链接/硬链接仍按原规则检查，并由定向回归区分范围（见 `frontend/scripts/lib/native-single-instance.test.mjs`）。

## 2026-10-24/25 WorkBuddy 双来源（N17 已合并发布 v0.6.0）

[WorkBuddy 国内版与 WorkBuddy AI 国际版接入计划](active/2026-10-21-workbuddy-adapters.md) 经用户 2026-10-24 确认修订并解除验收收口的串行依赖后，已在分支 `feat/workbuddy-adapters` 完成 WB01–WB09 实施（提交表、红绿摘要、十条不变量自审与对账结果见计划 §6.3）：现有来源等价泛化以冻结测试与变异证明行为不变；两版 WorkBuddy 以独立身份接入（设置格式 v3、隐私政策修订 2 与重新同意）；WB08 真实对账 Rust 与独立 Python 逐应用逐字段一致（国内版 2,881 条 / 国际版 656 条、坏行 0）；`ui-identity`、`ui-source-paths` 与新增 `privacy-policy-update` 原生场景本地通过。取号 minor 0.6.0。2026-10-25 用户经三轮外部评审后授权合并：**PR #49 普通合并 `44c026d`，main 的 CI / Native UI E2E / Release（构建、八组单跳、两条连续升级链、publish）全部实际成功，[v0.6.0](https://github.com/ywltby/tokenscope/releases/tag/v0.6.0) 已发布（预发布，4 资产）；运行链接见专项计划 §6.4。用户手动安装验证保留待验。** 原验收收口 B3 待修订、B4 / B5 未执行的状态保留，不改变 N01–N16 的历史结论。

## 2026-10-23 测试临时目录清理（已完成归档）

[测试临时目录清理实施计划](archive/implemented/2026-10-23-test-temp-cleanup.md) 两个批次均已交付并经用户逐轮复核：B1（Vitest 用例遗漏修复 + 六条外层契约回归 + CI 接线）随 [PR #44](https://github.com/ywltby/tokenscope/pull/44) 合并 `fde7762`（三轮复审补修：失败证据保留、超时进程树收尾、挂起创建消除作业竞态、原预算内统一截止、日志直写文件）；前置 [PR #42](https://github.com/ywltby/tokenscope/pull/42)（Rust 独立临时根与进程级收尾）经用户批准合并 `4fc46c6`；B2（锁等待用例先 `drop(holder)` 再严格核实删除）随 [PR #46](https://github.com/ywltby/tokenscope/pull/46) 合并 `58d0d15`，发布 [v0.5.13](https://github.com/ywltby/tokenscope/releases/tag/v0.5.13)（构建、五场景安装、八组单跳（0.1.0 / 0.4.0 / 0.5.9 / 0.5.12 × present/deleted）、两条连续升级链全过）。两类历史失败（native-ui 页面 target 未出现、`latest_history_opens_without_writer_lock` 超限）根因未定位、仅记录；其余夹具的治理为后续候选，见归档计划 §6.2。本专项不占 N 编号，不改变 N09 等已完成项的历史状态。

## 2026-11-07 Claude 跨会话复制去重与旧库纠错（0.7.2）

[专项计划](active/2026-11-07-claude-cross-session-dedupe.md) 的 C01–C05 已交付：复制/分支会话把同一次模型调用抄进另一会话时，副本的 `message.id`、`requestId`、`uuid`、事件时间、模型与四桶完全一致，旧身份 `(session_id, message.id)` 让副本各成一条事实。现追加 `claude-copy` 证据身份作为第二个别名（不删除会话内身份、不按时间或 token 猜测），内存去重与历史写入共用同一判定；`PARSE_RULES_REVISION` 5 → 6 让未变化的文件重解析补证据，旧库中证据充分的重复事实在写入事务内按固定代表规则合并（先迁全部别名与来源，再删重复行），合并前生成 `history.db.correction-backup-<UTC>` 可恢复备份，无备份整批回滚、证据不足的旧行保留。定向测试 12 例（`claude_cross_session_dedupe.rs` 4 + `claude_cross_session_migration.rs` 8）加 `dedupe.rs` 三条单测；口径见 `docs/stats-semantics.md` §3.1、§3.3.1、§6、§6.5.4。**边界**：只抄到流式中间观察的副本、来源已删除的一侧不合并；不涉及 `/btw`、Codex fork 或 CCS 日汇总规则；代码修复不等于安装版已更新。

## 已交付与证据边界

- PR #7–#13 已分别合入并发布 v0.3.0–v0.4.0，计划见下方第二批归档。
- PR #14 来源目录功能已合入；v0.5.0 因发布测试架失败未公开，其功能由 PR #15 的 v0.5.1 首次公开。PR #15 合并 `9ea0c584`，[Release 39291272967](https://github.com/ywltby/tokenscope/actions/runs/39291272967) 的两条连续升级链成功，N14 第 1 项关闭。
- PR #16 启动取证与恢复、SQLite 并发打开修复合并 `0ffab19`，v0.5.2 已发布；main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39353860584)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39353860657)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39353860618) 全成功。N14 第 3 项按原验收条件关闭，旧超时根因仍未证实。
- PR #17（`89aec76`）仅收录研究资料及版本/基线文件，不改变产品行为，也不自动增加开发任务。本次计划收尾以已核验的 v0.5.2 发布证据为基准，不借用其他版本结果补记未执行验收。
- PR #19 打开锁等待回归修复合并 `1156385`，v0.5.5 已发布；main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39452749833)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39452749860)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39452749887) 全成功，N15 关闭。
- PR #21 / #22 已验证版本不变时 Release 只跑 gate、六个下游全部 skipped；PR #23 产品修复合并 `d064f98` 并发布 v0.5.6，main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39568790001)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39568789964)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39568789984) 全成功（安装、六个单跳、两条连续链）。N14 第 2 项及 N16 关闭，版本与发布解耦两条路径均有实跑证据。
- B1 / AC01 的 `69ce2f4` 随 [PR #27](https://github.com/ywltby/tokenscope/pull/27) 普通合并 `4198051`；两环境共享冲突探针、两个变异、普通/acceptance 采集各 17/17 与完整钩子通过，PR 的 CI/原生/安装/六组升级全成功。main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39633188944)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39633188991)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39633189036) 全成功，Release 六个下游 skipped，最新发布仍为 v0.5.6；N14 第 4 项及 N14 全部关闭。
- B2 / AC02–AC05 随 [PR #31](https://github.com/ywltby/tokenscope/pull/31) 普通合并 `c9de40e`，[v0.5.9](https://github.com/ywltby/tokenscope/releases/tag/v0.5.9) 已公开并下载核对四项原资产后登记基线。三个场景本地各三连跑与 29 条具名业务断言先红后绿，最终 PR 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39756585501)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39756585449)、[Install test](https://github.com/ywltby/tokenscope/actions/runs/39756585577) 全成功。main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39758431511)、[原生](https://github.com/ywltby/tokenscope/actions/runs/39758431422)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39758431613) 全成功（安装五场景、六组单跳、两条连续链）；新增 CI 为 PR 116 秒 / main 68 秒。N11 人工尾项保留，B3 待修订，B4、B5 未执行。

`active/` 保留整合计划与已确认的[剩余验收收口计划](active/2026-10-21-acceptance-closure.md)（AC01–AC12，B1 已关闭 N14；B2 自动化已合并，N11 人工尾项待验；B3 修订稿待用户确认（§4b），B4、B5 未执行），以及上述已确认、独立推进的 WorkBuddy 双来源计划（N17）；[合入与发布解耦](archive/implemented/2026-10-21-release-on-version-change.md)（N16）与测试临时目录清理专项（B1/B2 均交付）已完成并归档。归档共 **54 份：24 份已实现、28 份部分完成、2 份已被取代**。部分完成通常表示实现存在但自身验收有尾项；归档表示不再独立执行，不表示所有验收通过。

必须保持用户定稿：一个 `~/.tokenscope/history.db`，只保存精简 token/cwd 元数据；全部代理有效用量纳入；源日志删除或停用仍查得到历史；CCS 设置页只手动单次导入，导入与已有事实求并集。较新规则优先于旧“现存日志是唯一事实”等历史设计。

## 2026-10-22 Codex 用量修复归档

| 计划 | 交付 | 当前状态 |
| --- | --- | --- |
| [Codex 压缩用量补计与线程设置模型归属](archive/implemented/2026-10-22-codex-compaction-usage.md) | PR #35 / v0.5.10，合并 `a31540f` | 对照 cc-switch v4.0.18 发现；CP01–CP05 与 §5 验收闭合，main CI、原生、Release 全成功；v0.5.10 基线随归档登记；不占整合计划责任编号 |

## 2026-10-22 价格同步修复归档

| 计划 | 交付 | 当前状态 |
| --- | --- | --- |
| [价格同步 TLS 信任修复](archive/implemented/2026-10-22-emudhra-tls-trust-fix.md) | PR #29 / v0.5.7；组合复验在 PR #30 合并后完成 | TF01–TF03 自身验收闭合；包含 TLS 更新与解析修复的 main `cb35f83` 上两价格源完整同步通过 |
| [OpenRouter 时间条件解析修复](archive/partial/2026-10-22-openrouter-override-conditions.md) | PR #30 / v0.5.8，合并 `cb35f83` | main CI、原生、Release 全成功，六组单跳与两条连续链通过；安装后设置页手动同步按用户决定保留待验，转 N11 |
| [价格同步告警汇总](archive/implemented/2026-10-22-sync-log-aggregation.md) | PR #37 / v0.5.11，合并 `0fabf24` | LA01–LA04 与 §5 验收闭合；隔离程序取证 models.dev WARN 0 / 明细 415、OpenRouter WARN 2 / 明细 23；汇总行措辞随 v0.5.12 修正；不占整合计划责任编号 |

原生计时误报由独立 [PR #32](https://github.com/ywltby/tokenscope/pull/32) 修复（合并 `1f00c9e`）：两条 2300ms 阈值保留，精确状态切换与真实回调取代两端 DOM 轮询判定；main 的 CI、原生成功，Release 只跑 gate、六个下游 skipped。v0.5.7、v0.5.8 均已按原资产核对并登记升级基线，v0.5.8 登记随本次归档交付；不提前关闭 N11 或其他开放编号。

## 2026-10-20 第三批归档

| 计划 | 交付 | 当前状态 |
| --- | --- | --- |
| [启动卡死取证补强与引导期恢复](archive/implemented/2026-10-19-boot-stall-recovery.md) | PR #16 / v0.5.2 | BS01–BS06 完成，已复核、合并、发布；N14 第 3 项关闭 |
| [SQLite 并发打开失败调查](archive/implemented/2026-10-19-sqlite-concurrent-open-investigation.md) | 随 PR #16 / v0.5.2 | HistoryDb/Cache 修复、类型化错误、绝对 deadline 与定向回归完成；推断与实证分开保留；锁等待语义由下一行修正 |
| [SQLite 打开锁等待回归修复](archive/implemented/2026-10-20-sqlite-open-lock-wait.md)（2026-10-21 补入） | PR #19 / v0.5.5 | LW01–LW05 及复核补修完成，已合并、发布；N15 关闭 |
| [合入 main 与发布解耦](archive/implemented/2026-10-21-release-on-version-change.md)（2026-10-21 补入） | PR #21 / #22 / #23 | RV01–RV05 完成，不发布与产品正常发布两条路径均通过，N16 关闭 |

历史执行命令、测试数量和“等待 PR/发布”等语句仅代表当时状态。当前执行以整合计划及仓库现行工具为准；准备合入 main 的批次仍按 CLAUDE.md 登记上一发布基线，只有产品输入改动才递增一次版本，纯文档、CI、脚本与测试的合入不递增、不发布。

## 原 active 计划：逐份归档

| 历史计划 | 状态 | 当前剩余责任 |
| --- | --- | --- |
| [定价来源策略](archive/partial/2026-10-05-pricing-source-policy.md) | 部分完成 | N11；N08/N09 已完成 |
| [产品审查与路线图](archive/partial/2026-10-05-product-review-and-roadmap.md) | 部分完成 | N12、N13 |
| [发布阻断问题修复](archive/partial/2026-10-05-release-blockers-remediation.md) | 部分完成 | N12；N09 已完成 |
| [Apple 视觉刷新剩余任务](archive/partial/2026-10-06-apple-refresh-remaining-tasks.md) | 部分完成 | N11、N12 |
| [Apple 视觉刷新早期方案](archive/superseded/2026-10-06-apple-visual-refresh.md) | 已被取代 | N11、N12 |
| [缓存读取价格语义](archive/partial/2026-10-06-cache-read-pricing-resolution.md) | 部分完成 | N11 |
| [关闭确认与设置文件](archive/implemented/2026-10-06-close-confirm-and-settings-file.md) | 已实现 | 无独立待办 |
| [设计系统实施](archive/partial/2026-10-06-design-system-implementation.md) | 部分完成 | N11、N12 |
| [设计系统视觉差异基线](archive/superseded/2026-10-06-design-system-visual-gap.md) | 已被取代 | N12 |
| [设计系统视觉 QA 历史记录](archive/partial/2026-10-06-design-system-visual-qa.md) | 部分完成 | N11、N12 |
| [实现后审计修复](archive/partial/2026-10-06-post-implementation-audit-remediation.md) | 部分完成 | N12 |
| [审查发现问题修复](archive/partial/2026-10-06-review-findings-remediation.md) | 部分完成 | N12；N09 已完成 |
| [分段计价与请求费用明细](archive/partial/2026-10-06-tiered-pricing-and-request-breakdown.md) | 部分完成 | N11、N12；N08 已完成 |
| [全计划审计修复 R01–R10](archive/implemented/2026-10-07-all-plans-audit-remediation.md) | 已实现 | 无独立待办 |
| [修复后复核 F01–F08](archive/implemented/2026-10-07-post-remediation-recheck-fixes.md) | 已实现 | 无独立待办 |
| [UI/UX 与统计基础修复](archive/partial/2026-10-07-ui-ux-review-remediation.md) | 部分完成 | N10、N11、N12 |
| [原生复核 QA 记录](archive/partial/2026-10-08-native-recheck-qa.md) | 部分完成 | N10、N11、N12 |
| [RC01–RC11 复核修复](archive/partial/2026-10-08-recheck-remediation.md) | 部分完成 | N10、N11、N12 |
| [AP01–AP09 全计划终态复核](archive/partial/2026-10-09-all-plans-final-recheck.md) | 部分完成 | N10、N11、N12 |
| [AP08 补齐验收记录](archive/partial/2026-10-09-ap08-completion-qa.md) | 部分完成 | N10、N11、N12 |
| [CI 与发布自动化](archive/implemented/2026-10-09-ci-release-automation.md) | 已实现 | N12 |
| [首次启动隐私同意](archive/partial/2026-10-09-first-launch-privacy-consent.md) | 部分完成 | N10、N12 |
| [跨工具项目路径统一](archive/partial/2026-10-09-project-path-unification.md) | 部分完成 | N11 |
| [模型名称等价分组与计价](archive/partial/2026-10-10-model-pricing-name-equivalence.md) | 部分完成 | N11；N08 已完成 |
| [用量历史库与 CCS 手动导入](archive/partial/2026-10-10-usage-history-and-ccs-import.md) | 部分完成 | N11、N13；N01–N09 已完成 |

## 2026-10-17 第二批归档

以下 8 份计划均已合入 main。前 7 份已发布并归档到 archive/implemented（CI 并行化与 PR2 于 2026-10-17/18 补入）；PR3 于 2026-10-19 归档到 archive/partial，其改动随 0.5.1 首次公开；各文件头部的“2026-10-17 归档复核”说明原文中过时状态的实际结果。

| 历史计划 | PR / 版本 | 状态 | 当前剩余责任 |
| --- | --- | --- | --- |
| [UI 调整：时间筛选、自动读取、汇总页状态与设置页](archive/implemented/2026-10-13-ui-adjustment.md) | #7 / v0.3.0 | 已实现 | U06 常驻更新时间由 PR2 取代；系统验收见独立待办 |
| [控件材质与浮层可读性](archive/implemented/2026-10-13-control-material.md) | #8 / v0.3.1 | 已实现 | 真实跨显示器见独立待办 |
| [PR1：虚拟 Cargo Workspace、目录迁移与提交检查](archive/implemented/2026-10-15-workspace-layout.md) | #9 / v0.3.2 | 已实现 | 无；旧 `src-tauri/target` 核实已不存在 |
| [升级测试补测：起点矩阵、连续升级链与原生验收重启修复](archive/implemented/2026-10-16-upgrade-matrix.md) | #10 / v0.3.3 | 已实现 | 无 |
| [发布流程去掉重复的完整测试](archive/implemented/2026-10-17-release-gate-dedup.md) | #11 / v0.3.4 | 已实现 | 无；发布从约 31 分钟降到 6 分 44 秒 |
| [CI 拆成并行作业与原生 E2E 精简](archive/implemented/2026-10-17-ci-parallel-checks.md) | #12 / v0.3.5 | 已实现 | 无；热缓存下 CI 约 5m53s、原生 E2E 约 7m39s |
| [PR2：真实扫描状态与读取动画](archive/implemented/2026-10-15-collection-status.md) | #13 / v0.4.0 | 已实现 | main 上原生 E2E 一次卡死：PR3 S00 加固后 main 首次运行通过、未复现；系统验收见独立待办 |
| [PR3：当前用户日志目录发现与追加路径管理](archive/partial/2026-10-14-source-discovery-and-additional-paths.md) | #14 / v0.5.0（未发布，由 0.5.1 公开） | 部分完成 | N13（真实性能）；N14 四项均已关闭 |

## M1–M11：自身验收复核

| 里程碑 | 状态 | 当前剩余责任 |
| --- | --- | --- |
| [M1 Claude Code 适配器](archive/implemented/2026-10-03-m1-claude-code-adapter.md) | 已实现 | 无独立待办 |
| [M2 Codex 适配器与计价](archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) | 已实现 | 无独立待办 |
| [M3 Tauri + Vue GUI](archive/partial/2026-10-03-m3-tauri-vue-gui.md) | 部分完成 | N12 |
| [M4 缓存、外置价格与设置](archive/partial/2026-10-03-m4-cache-pricing-settings.md) | 部分完成 | N11 |
| [M5 OpenRouter 价格同步](archive/partial/2026-10-04-m5-openrouter-pricing-sync.md) | 部分完成 | N11 |
| [M6 时区解析](archive/implemented/2026-10-04-m6-timezone-resolution.md) | 已实现 | N11 |
| [M7 请求明细下钻](archive/partial/2026-10-04-m7-event-drilldown.md) | 部分完成 | N11 |
| [M8 桌面体验](archive/partial/2026-10-04-m8-desktop-experience.md) | 部分完成 | N12 |
| [M9 models.dev 价格主源](archive/partial/2026-10-04-m9-modelsdev-source.md) | 部分完成 | N11 |
| [M10 日期区间](archive/implemented/2026-10-04-m10-date-range.md) | 已实现 | N11 |
| [M11 价格索引与自动同步](archive/partial/2026-10-04-m11-pricing-cache-autosync.md) | 部分完成 | N10、N12、N13 |

M1/M2/M6/M10 不因全产品 D5 未签字反向变成未实现。M3/M4/M5/M7/M8/M9/M11 的 partial 来自原计划自己的安装/操作/运行条件，不是泛泛要求每份计划重跑 D5；已合并成安装托盘、Settings 同步、下钻点击、窗口/单实例/自启、自动同步五组。

## 归档规则与维护

- [archive/implemented](archive/README.md)：原工程及自身验收已闭合，公共产品验收另记。
- [archive/partial](archive/README.md)：实现或验收有未闭合项，旧记录保留，责任转入唯一新计划。
- [archive/superseded](archive/README.md)：被较新用户需求/方案取代，不按旧设计重新实现。
- [D5 清单](d5-acceptance-checklist.md) 是 N12 的发布附属清单，保留旧产物结果，不是另一份 active 计划。
- 后续只更新新计划的任务与证据，并回填对应历史头部。不得把历史“待验/active/完成”直接当当前状态。

其他工具适配器、自动更新、DuckDB、多设备 CCS 等没有本期实施排期；研究文档不自动变成开发任务。旧截图补造、旧 CLI/内置价、代理关系、项目拆库和自动 CCS 同步不再执行。
