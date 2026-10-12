# 计划审计事实复核

> 2026-10-11 重新核对。整理基线：`add8319` 与当日工作区；后续修复 `3e645f3` 的 7 项已完成独立复核和最终门禁，见[复核记录](../../reviews/2026-10-11-history-seven-fixes-recheck.md)。
> 最新独立复核：[当前 main 合并门槛](../../reviews/2026-10-11-main-merge-readiness.md#current-merge-readiness)（2026-10-12，代码 231f667，后续 e31d1b6 仅改文档）：A01–A06 已闭合，完整本地普通/acceptance、前端及安装契约通过，代码与本地门禁就绪；当前无 PR/Actions，正式合并仍须 CI/gate 和 Install test/install-test 绿灯。N12 安装待实跑，其他系统及 N10/N11/N13 不关闭；历史结果和执行者声明保留原时点。
> [唯一剩余计划](../active/2026-10-11-consolidated-remaining-work.md) ｜ [完成/待办总账](../README.md) ｜ [归档说明](../archive/README.md)

## 修正后的结论

原审计“代码层面没有未完成项，唯一缺口是真机”**不成立**。原 25 份报告大多能对应到真实实现，但把函数/测试存在、旧执行记录或整个 QA 总数当作当前行为通过；另有任务编号、提交归因与版本字符串错误。本次逐份更正，不保留原错误结论作为当前事实。

本次实际区分：已实现、历史验证、本轮定向通过、实现/验证未完成、被新需求取代。原 active 25 份中 **4 份已实现、19 份部分完成、2 份被取代**，全部归档；M1–M11 另外 4 份已实现/7 份部分完成。当前仅保留整合计划，13 个责任编号中 **9 完成、1 部分完成、3 本轮验收未执行**，共 4 项待办。N12 的 A01–A06 缺口已修、双审计循环已收敛，但尚未取得当前产物的 CI 实跑结果，整体部分完成；历史计划中的承接编号保留，完成与否查当前整合表。

- 旧污染事实、负数迁移、容量、手动恢复、百万 Rust 峰值及退出等待已通过；批内来源身份、原五个存储反例、直接构造的价格隔离与普通/acceptance 门禁也已闭合。历史库仍有 N11/N13 原生/性能验收，不能写 H01–H08 全部完成。
- review-findings Task6 握手和历史循环已完成；公共 helper、峰值净存活断言、Settings 文案及 9 处直接价格路径均已修复。`a58684c` 将旧数据库验收更新为 history.db 并只读核对七表，实际仓库测试及完整门禁通过，N09 闭合。
- MP04 的跨应用和有价分页断言已补并通过，原生验收仍待 N11；项目 A/B 与隐私 P03–P05 的原审计职责错配已修正。`rules:9` 才是实际签名。
- Apple Task6 的 Settings/通知后来已完成；分段价格计划已有执行账。旧视觉基线不是当前缺陷清单。
- 较新 AP08 已完成 CSP、真实 TTL 和当前显示器未强制的 150%；不能继续抄更早“全部待验”。125%/跨屏/系统深色首帧与安装仍未全验。
- 隐私 P01–P05 已实现；P06 工具遗漏 history.db 与真实 I/O/顺序/120s 取证，局部矩阵不能冒充全矩阵。缺取证不等于已证明生产泄漏。
- CI/main 预发布有实际远程成功记录；正式 tag/当前 SHA/安装/验签没有冒充已验。

## 逐份修订报告

| 审计 | 归档结论 / 原计划 | 当前承接 |
| --- | --- | --- |
| [定价来源策略](2026-10-05-pricing-source-policy.md) | [部分完成](../archive/partial/2026-10-05-pricing-source-policy.md) | N11；N08/N09 已完成 |
| [产品审查与路线图](2026-10-05-product-review-and-roadmap.md) | [部分完成](../archive/partial/2026-10-05-product-review-and-roadmap.md) | N12、N13 |
| [发布阻断问题修复](2026-10-05-release-blockers-remediation.md) | [部分完成](../archive/partial/2026-10-05-release-blockers-remediation.md) | N12；N09 已完成 |
| [Apple 视觉刷新剩余任务](2026-10-06-apple-refresh-remaining-tasks.md) | [部分完成](../archive/partial/2026-10-06-apple-refresh-remaining-tasks.md) | N11、N12 |
| [Apple 视觉刷新早期方案](2026-10-06-apple-visual-refresh.md) | [已被取代](../archive/superseded/2026-10-06-apple-visual-refresh.md) | N11、N12 |
| [缓存读取价格语义](2026-10-06-cache-read-pricing-resolution.md) | [部分完成](../archive/partial/2026-10-06-cache-read-pricing-resolution.md) | N11 |
| [关闭确认与设置文件](2026-10-06-close-confirm-and-settings-file.md) | [已实现](../archive/implemented/2026-10-06-close-confirm-and-settings-file.md) | 无独立待办 |
| [设计系统实施](2026-10-06-design-system-implementation.md) | [部分完成](../archive/partial/2026-10-06-design-system-implementation.md) | N11、N12 |
| [设计系统视觉差异基线](2026-10-06-design-system-visual-gap.md) | [已被取代](../archive/superseded/2026-10-06-design-system-visual-gap.md) | N12 |
| [设计系统视觉 QA 历史记录](2026-10-06-design-system-visual-qa.md) | [部分完成](../archive/partial/2026-10-06-design-system-visual-qa.md) | N11、N12 |
| [实现后审计修复](2026-10-06-post-implementation-audit-remediation.md) | [部分完成](../archive/partial/2026-10-06-post-implementation-audit-remediation.md) | N12 |
| [审查发现问题修复](2026-10-06-review-findings-remediation.md) | [部分完成](../archive/partial/2026-10-06-review-findings-remediation.md) | N12；N09 已完成 |
| [分段计价与请求费用明细](2026-10-06-tiered-pricing-and-request-breakdown.md) | [部分完成](../archive/partial/2026-10-06-tiered-pricing-and-request-breakdown.md) | N11、N12；N08 已完成 |
| [全计划审计修复 R01–R10](2026-10-07-all-plans-audit-remediation.md) | [已实现](../archive/implemented/2026-10-07-all-plans-audit-remediation.md) | 无独立待办 |
| [修复后复核 F01–F08](2026-10-07-post-remediation-recheck-fixes.md) | [已实现](../archive/implemented/2026-10-07-post-remediation-recheck-fixes.md) | 无独立待办 |
| [UI/UX 与统计基础修复](2026-10-07-ui-ux-review-remediation.md) | [部分完成](../archive/partial/2026-10-07-ui-ux-review-remediation.md) | N10、N11、N12 |
| [原生复核 QA 记录](2026-10-08-native-recheck-qa.md) | [部分完成](../archive/partial/2026-10-08-native-recheck-qa.md) | N10、N11、N12 |
| [RC01–RC11 复核修复](2026-10-08-recheck-remediation.md) | [部分完成](../archive/partial/2026-10-08-recheck-remediation.md) | N10、N11、N12 |
| [AP01–AP09 全计划终态复核](2026-10-09-all-plans-final-recheck.md) | [部分完成](../archive/partial/2026-10-09-all-plans-final-recheck.md) | N10、N11、N12 |
| [AP08 补齐验收记录](2026-10-09-ap08-completion-qa.md) | [部分完成](../archive/partial/2026-10-09-ap08-completion-qa.md) | N10、N11、N12 |
| [CI 与发布自动化](2026-10-09-ci-release-automation.md) | [已实现](../archive/implemented/2026-10-09-ci-release-automation.md) | N12 |
| [首次启动隐私同意](2026-10-09-first-launch-privacy-consent.md) | [部分完成](../archive/partial/2026-10-09-first-launch-privacy-consent.md) | N10、N12 |
| [跨工具项目路径统一](2026-10-09-project-path-unification.md) | [部分完成](../archive/partial/2026-10-09-project-path-unification.md) | N11 |
| [模型名称等价分组与计价](2026-10-10-model-pricing-name-equivalence.md) | [部分完成](../archive/partial/2026-10-10-model-pricing-name-equivalence.md) | N11；N08 已完成 |
| [用量历史库与 CCS 手动导入](2026-10-10-usage-history-and-ccs-import.md) | [部分完成](../archive/partial/2026-10-10-usage-history-and-ccs-import.md) | N11、N13；N01–N09 已完成 |

## 本轮证据与限制

1. 通读计划/任务表及后续计划，核对当前源码、测试断言、真实历史提交；原计划正文保留并新增当前状态表。
2. 早期通过/失败结果保留为历史；`a58684c` 时点普通根库 **456 pass / 10 ignored**、壳 **42 pass**、前端 **361 pass**；acceptance 根库 **456 pass / 10 ignored**、壳 **47 pass**，双 manifest fmt、普通/acceptance Clippy 与前端 typecheck/format:check/build 全绿。Cargo 使用 `--locked`；该轮十五项 CI 对应本地检查全部退出 0。双审计循环收敛后终态（`6fab12f`）：根库 **461 + acceptance**、壳库 **45**、前端 **364**、python **36**、pwsh 契约 **22** 全绿。
3. `16beea1` 的原五个存储反例、来源二元身份两例、日期/候选口径及价格隔离共 11 项仓外验证通过，上轮 T01/T02 不再列为当前缺陷。`a58684c` 的仓库实际 native_acceptance 三项父测试通过，history.db 路径、产物及七表校验生效，M01 闭合。百万、可控退出及 11 项独立探针沿用此前证据，本轮未重复执行。
4. 读取本机可用的历史 QA JSON/日志，核对实际断言与参数；缺失旧截图/只记数值的历史记录明确标证据边界，不补造图片或重跑旧版来“填满”。
5. 历史 run/release 的直接链接见[CI/发布报告](2026-10-09-ci-release-automation.md)。`a58684c` 时点 fetch 验证 main 可快进，修复已在远端功能分支；只读查询未发现打开的 PR，也无该提交的 PR CI 记录，不声称远端已绿。GitHub 连接器提供创建 PR 接口，本机缺 gh 不构成唯一障碍；本轮只复核，未创建 PR。Python release metadata 7 项通过，没有合并 main 或触发新发布。当前截至 `231f667` 为 **0 behind / 71 ahead**，仍无安装实跑证据。
6. N05 百万测量、N06 可控等待已闭合；Rust 分配器峰值不等于完整 RSS，百万只验证现有汇总/前两页断言。N10–N13 的当前完整矩阵、原生/安装/系统和真实历史性能未执行，不以旧产物或普通 build 宣称通过。
7. 复核期间另提交并推送 b16437c，增加安装 workflow/脚本及整合执行账，没有改产品源码。新增部分查出 M02 缺直接 gate 依赖、M03 脚本调用路径含制表符、M04 合成旧库前缺数据目录创建；YAML/PowerShell AST/临时目录实测有直接证据。Release metadata 七项再次通过，但不覆盖这些缺口；M02–M04 此后由 `cc7b7d3` 等提交闭合。该批复核文档当时按未解决阻断例外未提交，例外已随 A01–A06 修复消失（见第 12 条）。
8. 用户提供两份执行答复后统一复核：M01 再次 3 pass/2 ignored、metadata 7 pass，M02–M04 仍存在；M05/M06 的受控原函数反例与 M07 原旧包块两分支有直接结果。新应用退出、cwd 路径错误仍通过计数断言，缺/错 SHA 仍执行模拟安装。PS1 BOM 与 CRLF 属实，PS5.1 的 utf8NoBOM 参数绑定不兼容另记边界。所有探针仅读写命名临时目录，外部安装/网络/重置均模拟；没有运行真实安装，PR CI 不执行 Release 安装 job，不能签 N12。
9. `cc7b7d3` 再复核：M02/M03/M04/M07 闭合，RequiresVersion 7.0 正确；M05 遗留 WAL+新应用退出 23、M06 路径后缀损坏均六断言通过，未全闭合。新增 M08 把分支名当安装版本、M09 默认 main 没有手动工作流入口。实际 native_acceptance 3/2、review_regressions 29、metadata 7 通过；只读 GitHub 查询当前 SHA 全事件 Actions runs=0、无打开 PR，main 可快进 0/53。9 完成/1 部分完成/3 待验；N12 当时部分完成（见第 12 条）。
10. `e99a4f3` 第三轮安装复核：M08 版本实际输出 0.1.0，M09 当前 PR 路线有效；旧 WAL 不变超时、错误项目路径集合失败 1。真实 SQLite 写入后退出 23、观察 8 秒，五断言仍通过；初始/事件 cwd 全改错也五断言通过。新增 M10 的 src/Cargo/setup 路径不匹配。acceptance 3/2、review_regressions 29、metadata 7 通过；无打开 PR/当前 SHA 全事件 runs=0、main 可快进 0/54。N09 保持完成、N12 当时部分完成（见第 12 条）。
11. `a64d340` 设计实施复核：M06 22 项 unittest、metadata 7 和 Rust 定向 40 pass/2 ignored 通过；PS7 7.6.6 生命周期实跑退出 1，正例仍为 0xC000013A，五个日志反例缺句柄提前失败。隔离抽取真实编排函数复现 Launch-AppOnce 未定义；当前日志与等待目录不一致、当前日志未上传。PR 安装构建前缺两条契约，卸载后缺 manifest。执行者“全部完成”改记为声明，N12 当时部分完成；该时点无 PR/Actions，0 behind/57 ahead。所有探针仅操作临时目录，未运行真实安装。
12. `b855021`–`6fab12f` 收敛：第 11 条的 A01–A06 六项已全部修复并由 `788905a` 记账——A01 启动函数改名 `Launch-AppOnce` 并补 `-LiveLogsDir`；A02 等待改读 `DataDir\logs`、与证据目录分离、关闭后当前日志随 `always()` 上传；A03 GUI 替身换 `notepad`（`WM_CLOSE` → 退出码 0）；A04 五个日志反例接存活替身；A05 `install-test.yml` 构建前补两条契约命令；A06 卸载后按 manifest 逐条复核保留事实。双审计员挑刺循环六轮 12 名审计员、累计 15 个修复提交（`d7ee94b`…`6fab12f`），P1 始终为 0。门禁终态：根库 461 + acceptance、壳库 45、前端 364、python 36、pwsh 契约 22。第 7–11 条“不提交推送”的验证失败例外随之失效，本索引与复核文档入库；N12 唯一待办为 PR CI 实跑取证。
13. `231f667` / 仅文档 `e31d1b6` 独立合并复核：完整普通/acceptance 根库各 462 通过/10 忽略、壳 46/51、前端 364、Python 36、PS7 22，双 manifest 格式/Clippy（-D warnings）及前端四项全部通过。实际安装编排隔离探针通过；A01–A06 已关闭，未发现新的 P1/P2 代码阻断。该两 SHA 全事件 Actions 均 0、无打开 PR；main 仍祖先。代码与本地门禁就绪，正式合并仍缺当前 PR CI 和安装绿灯；N12 系统/发布及 N10/N11/N13 不关闭。前条数字保留为执行者当时记录，本次实测数字以上述结果为准。

用户明确需求优先于全部旧记录；较新计划优先于较旧计划。被否定或被取代的设计不因曾写在计划里而复活，暂缓建议不擅自升级为必须实施。
