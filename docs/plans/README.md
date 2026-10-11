# TokenScope 计划与完成状态总账

> 2026-10-11 整理。**用户明确要求 > 较新计划 > 较旧计划 > 未确认的设计建议。**
> 当前唯一待办入口：[剩余任务整合与验收计划](active/2026-10-11-consolidated-remaining-work.md)；逐份事实核对：[审计索引](audits/README.md)。
> 最新实施后复核：[85edad2 复核记录](../reviews/2026-10-11-consolidated-plan-recheck.md)。

## 现在执行什么

原 active 的 25 份计划全部归档；M1–M11 重新核对自身验收条件。历史计划共 **36 份：8 份已实现、26 份部分完成、2 份已被取代**。部分完成通常是“实现已完成、验收有尾项”，用量历史计划则仍有实现缺口。`active/` 只保留上述整合计划，不再分别追逐旧计划的尾项。

- **P1：N02**，先修批量预览与提交的请求并集处置差异；N01 的旧污染纠错已通过。
- **P2：N05、N06、N08、N09**，补查询峰值/准确说明、采集退出并发验收、完整候选文案与价格路径隔离；**N10–N11**，当前隐私和原生统一验收。
- **P3：N12–N13**，随实际发布做 D5/安装/系统验收，正确性闭合后复测性能。

保留 13 个责任编号；`85edad2` 实施后复核为 **4 项完成、5 项部分完成、4 项本轮验收未执行**，共 9 项待办。N01/N03/N04/N07 已完成；N02/N05/N06/N08/N09 仍有实现或验收缺口；N10–N13 未在实施批次执行。每项已有旧任务来源、必要性、文件、验证方法与通过标准；暂缓/被取代的内容单列在新计划第 5 节。

必须保持用户定稿：一个 `~/.tokenscope/history.db`，只保存精简 token/cwd 元数据；全部代理有效用量纳入，不做主子关系产品；源日志删除/停用仍查得到已保存历史；CCS 设置页只手动单次导入；已有事实与导入求并集，重复导入不增加用量。较新历史库规则优先于旧“源日志是唯一事实/sidechain 排除”规则。

## 已完成能力与证据边界

| 范围 | 当前结论 |
| --- | --- |
| Claude/Codex 解析、四桶归一化、时间/项目/模型聚合、冻结查询与分页 | 主体与多轮修复已经实现；历史并集/升级/规模边界见 N01–N07 |
| GUI、设置 TOML/事务、关闭三态、导航/日期控件/费用浮层、窗口/托盘/自启接线 | 既有功能实现完成；自身未闭合的原生操作/系统验收合并至 N10–N12 |
| 无内置价、三来源独立候选、完整优先/最高请求费用、分段峰谷/缓存三态 | 已实现；跨应用与有价分页断言通过；完整候选文案与普通测试价格隔离仍需 N08/N09 |
| 项目路径统一、子目录归根/越界切换、模型等价/可信名称 | 核心与多轮边界修复已经实现；跨应用和原始模型分页已验证，当前原生链见 N11 |
| 隐私同意闸门 | P01–P05 已实现；P06 强取证与当前完整矩阵仍未闭合，不能写成全部通过 |
| 历史库与 CCS 手动预览/确认、全部代理、Codex 归档发现 | 旧污染纠错、非法迁移、读取期容量限制通过；批量预览、百万构建峰值及退出并发验收仍有责任，H01–H08 不能全部关闭 |
| 既有浏览器/原生 QA | 历史 60 场景/17 契约/243 断言、CSP、真实 TTL、无强制当前显示器 150% 有产物；不证明最新功能/安装包全验 |
| CI / main 预发布 | 实际远程 PR/main 成功及 exe/校验和/attestation 发布有证据；当前 SHA、正式 v*、安装和验签未冒充已验 |

最新复跑：根库 **448 通过/10 忽略**，壳 **41 通过**，前端 **360 通过**；根/壳 fmt/Clippy、前端 typecheck/format:check/build 全部通过，根/壳使用默认并行测试。详情见[实施后复核记录](../reviews/2026-10-11-consolidated-plan-recheck.md)。此前旧污染事实与负数迁移反例现已通过；新增批量预览/提交反例失败，百万峰值、说明、价格隔离和当前原生验收仍未闭合，不能写“代码无未完成项/只剩真机”。[前次 7 项复核](../reviews/2026-10-11-history-seven-fixes-recheck.md)保留发生时的验证结果。

## 原 active 计划：逐份归档

| 历史计划 | 状态 | 当前剩余责任 |
| --- | --- | --- |
| [定价来源策略](archive/partial/2026-10-05-pricing-source-policy.md) | 部分完成 | N08、N09、N11 |
| [产品审查与路线图](archive/partial/2026-10-05-product-review-and-roadmap.md) | 部分完成 | N12、N13 |
| [发布阻断问题修复](archive/partial/2026-10-05-release-blockers-remediation.md) | 部分完成 | N09、N12 |
| [Apple 视觉刷新剩余任务](archive/partial/2026-10-06-apple-refresh-remaining-tasks.md) | 部分完成 | N11、N12 |
| [Apple 视觉刷新早期方案](archive/superseded/2026-10-06-apple-visual-refresh.md) | 已被取代 | N11、N12 |
| [缓存读取价格语义](archive/partial/2026-10-06-cache-read-pricing-resolution.md) | 部分完成 | N11 |
| [关闭确认与设置文件](archive/implemented/2026-10-06-close-confirm-and-settings-file.md) | 已实现 | 无独立待办 |
| [设计系统实施](archive/partial/2026-10-06-design-system-implementation.md) | 部分完成 | N11、N12 |
| [设计系统视觉差异基线](archive/superseded/2026-10-06-design-system-visual-gap.md) | 已被取代 | N12 |
| [设计系统视觉 QA 历史记录](archive/partial/2026-10-06-design-system-visual-qa.md) | 部分完成 | N11、N12 |
| [实现后审计修复](archive/partial/2026-10-06-post-implementation-audit-remediation.md) | 部分完成 | N12 |
| [审查发现问题修复](archive/partial/2026-10-06-review-findings-remediation.md) | 部分完成 | N09、N12 |
| [分段计价与请求费用明细](archive/partial/2026-10-06-tiered-pricing-and-request-breakdown.md) | 部分完成 | N08、N11、N12 |
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
| [模型名称等价分组与计价](archive/partial/2026-10-10-model-pricing-name-equivalence.md) | 部分完成 | N08、N11 |
| [用量历史库与 CCS 手动导入](archive/partial/2026-10-10-usage-history-and-ccs-import.md) | 部分完成 | N01、N02、N03、N04、N05、N06、N09、N11、N13；N07 已完成 |

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
