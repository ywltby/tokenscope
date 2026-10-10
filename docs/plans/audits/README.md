# 计划审计事实复核

> 2026-10-11 重新核对。整理基线：`add8319` 与当日工作区；后续修复 `3e645f3` 的 7 项已完成独立复核和最终门禁，见[复核记录](../../reviews/2026-10-11-history-seven-fixes-recheck.md)。
> [唯一剩余计划](../active/2026-10-11-consolidated-remaining-work.md) ｜ [完成/待办总账](../README.md) ｜ [归档说明](../archive/README.md)

## 修正后的结论

原审计“代码层面没有未完成项，唯一缺口是真机”**不成立**。原 25 份报告大多能对应到真实实现，但把函数/测试存在、旧执行记录或整个 QA 总数当作当前行为通过；另有任务编号、提交归因与版本字符串错误。本次逐份更正，不保留原错误结论作为当前事实。

本次实际区分：已实现、历史验证、本轮定向通过、实现/验证未完成、被新需求取代。原 active 25 份中 **4 份已实现、19 份部分完成、2 份被取代**，全部归档；M1–M11 另外 4 份已实现/7 份部分完成。当前仅保留整合计划，13 个责任编号中 N07 已完成、12 项仍有待办。

- 历史库主体存在，但旧污染事实、负数迁移、容量受限暂存、退出覆盖、完整并集/规模/交付仍有 N01–N09，不能写 H01–H08 全部完成。
- review-findings Task6 的 150ms follower 竞态未消除；价格测试仍有真实路径读取；计价 UI 说明与最新候选规则不一致。
- MP04 的跨应用和有价分页验收不足；项目 A/B 与隐私 P03–P05 的原审计职责错配已修正。`rules:9` 才是实际签名。
- Apple Task6 的 Settings/通知后来已完成；分段价格计划已有执行账。旧视觉基线不是当前缺陷清单。
- 较新 AP08 已完成 CSP、真实 TTL 和当前显示器未强制的 150%；不能继续抄更早“全部待验”。125%/跨屏/系统深色首帧与安装仍未全验。
- 隐私 P01–P05 已实现；P06 工具遗漏 history.db 与真实 I/O/顺序/120s 取证，局部矩阵不能冒充全矩阵。缺取证不等于已证明生产泄漏。
- CI/main 预发布有实际远程成功记录；正式 tag/当前 SHA/安装/验签没有冒充已验。

## 逐份修订报告

| 审计 | 归档结论 / 原计划 | 当前承接 |
| --- | --- | --- |
| [定价来源策略](2026-10-05-pricing-source-policy.md) | [部分完成](../archive/partial/2026-10-05-pricing-source-policy.md) | N08、N09、N11 |
| [产品审查与路线图](2026-10-05-product-review-and-roadmap.md) | [部分完成](../archive/partial/2026-10-05-product-review-and-roadmap.md) | N12、N13 |
| [发布阻断问题修复](2026-10-05-release-blockers-remediation.md) | [部分完成](../archive/partial/2026-10-05-release-blockers-remediation.md) | N09、N12 |
| [Apple 视觉刷新剩余任务](2026-10-06-apple-refresh-remaining-tasks.md) | [部分完成](../archive/partial/2026-10-06-apple-refresh-remaining-tasks.md) | N11、N12 |
| [Apple 视觉刷新早期方案](2026-10-06-apple-visual-refresh.md) | [已被取代](../archive/superseded/2026-10-06-apple-visual-refresh.md) | N11、N12 |
| [缓存读取价格语义](2026-10-06-cache-read-pricing-resolution.md) | [部分完成](../archive/partial/2026-10-06-cache-read-pricing-resolution.md) | N11 |
| [关闭确认与设置文件](2026-10-06-close-confirm-and-settings-file.md) | [已实现](../archive/implemented/2026-10-06-close-confirm-and-settings-file.md) | 无独立待办 |
| [设计系统实施](2026-10-06-design-system-implementation.md) | [部分完成](../archive/partial/2026-10-06-design-system-implementation.md) | N11、N12 |
| [设计系统视觉差异基线](2026-10-06-design-system-visual-gap.md) | [已被取代](../archive/superseded/2026-10-06-design-system-visual-gap.md) | N12 |
| [设计系统视觉 QA 历史记录](2026-10-06-design-system-visual-qa.md) | [部分完成](../archive/partial/2026-10-06-design-system-visual-qa.md) | N11、N12 |
| [实现后审计修复](2026-10-06-post-implementation-audit-remediation.md) | [部分完成](../archive/partial/2026-10-06-post-implementation-audit-remediation.md) | N12 |
| [审查发现问题修复](2026-10-06-review-findings-remediation.md) | [部分完成](../archive/partial/2026-10-06-review-findings-remediation.md) | N09、N12 |
| [分段计价与请求费用明细](2026-10-06-tiered-pricing-and-request-breakdown.md) | [部分完成](../archive/partial/2026-10-06-tiered-pricing-and-request-breakdown.md) | N08、N11、N12 |
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
| [模型名称等价分组与计价](2026-10-10-model-pricing-name-equivalence.md) | [部分完成](../archive/partial/2026-10-10-model-pricing-name-equivalence.md) | N08、N11 |
| [用量历史库与 CCS 手动导入](2026-10-10-usage-history-and-ccs-import.md) | [部分完成](../archive/partial/2026-10-10-usage-history-and-ccs-import.md) | N01、N02、N03、N04、N05、N06、N09、N11、N13；N07 已完成 |

## 本轮证据与限制

1. 通读计划/任务表及后续计划，核对当前源码、测试断言、真实历史提交；原计划正文保留并新增当前状态表。
2. 首轮整理曾出现根 Clippy、前端类型及快照版本断言失败，保留为历史记录。修复 `3e645f3` 后本轮重新运行：根库 **442 pass / 9 ignored**、壳 **41 pass**、前端 **359 pass**；根/壳 fmt/Clippy、前端 typecheck/format:check/build 全部通过。当前结果以[7 项复核](../../reviews/2026-10-11-history-seven-fixes-recheck.md)及新计划最新执行账为准。
3. 仓库外合成探针：旧版/v2 迁移、归档移动、native↔CCS、日汇总时区重选、冻结查询死锁/日期、长 ID 预算、实际 Rust serde 的 Settings 与模式恢复。新计划记录成功子项和仍失败的反例；全部使用临时库/来源，不读写真实用量库/日志。
4. 读取本机可用的历史 QA JSON/日志，核对实际断言与参数；缺失旧截图/只记数值的历史记录明确标证据边界，不补造图片或重跑旧版来“填满”。
5. 只读核对 GitHub 主仓库实际 run/release API；[CI/发布报告](2026-10-09-ci-release-automation.md) 给出直接链接。Python release metadata 7 项通过，没有触发新发布。
6. 原生安装/系统更改、当前完整矩阵、全部复杂弱身份、百万规模及新历史性能未在本次整理/复核完成；7 项修复通过不代替这些责任。最终状态以新计划后续执行账为准。

用户明确需求优先于全部旧记录；较新计划优先于较旧计划。被否定或被取代的设计不因曾写在计划里而复活，暂缓建议不擅自升级为必须实施。
