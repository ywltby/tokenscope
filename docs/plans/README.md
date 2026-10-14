# TokenScope 计划与完成状态总账

> 2026-10-11 整理。**用户明确要求 > 较新计划 > 较旧计划 > 未确认的设计建议。**
> 当前待办入口：[剩余任务整合与验收计划](active/2026-10-11-consolidated-remaining-work.md)。[UI 调整计划](active/2026-10-13-ui-adjustment.md)本轮功能及价格/窗口隐藏复核补修已完成、本地验收通过，最新补修仍需自身 HEAD 的远程门禁，PR #7 未合并；真实跨午夜、长时间休眠、跨显示器已按用户决定移至[独立系统验收待办](system-acceptance-todo.md)，本轮不执行。逐份事实核对：[审计索引](audits/README.md)。
> 最新进展：PR #5 最终 19c634b 的 CI、五场景、两项升级及工件已验，合并为 main 8d17a47。main 同样全部通过，历史 [v0.1.0](https://github.com/ywltby/tokenscope/releases/tag/v0.1.0) 与 [v0.2.0](https://github.com/ywltby/tokenscope/releases/tag/v0.2.0) 已公开；新建草稿读取延迟曾阻断上传，确认既有草稿后分阶段恢复了同一产物。防复发的 0.2.1 补修已改为固定创建响应 Release ID，当前门禁仍按新 HEAD 和 [Release](https://github.com/ywltby/tokenscope/actions/workflows/release.yml)判定；其余系统签字及 N10/N11/N13 不关闭。

## 现在执行什么

- **2026-10-15 按 1eaede7 审阅意见拆分，三份设计均待复审、未实施**：
  - **PR1 / patch**：[Workspace 目录迁移与提交检查](active/2026-10-15-workspace-layout.md)，W01–W07，使用 docs/source-discovery-plan，含 c16e023；迁移期间协调冻结其他 Rust 开发，合入后先 rebase。
  - **PR2 / minor**：[真实扫描反馈](active/2026-10-15-collection-status.md)，F01–F05；PR1 合入后从最新 main 建 feat/collection-status，读取图标至少 1 秒，查询不等待动画。
  - **PR3 / minor**：[当前用户来源目录与追加路径](active/2026-10-14-source-discovery-and-additional-paths.md)，S01–S07；PR2 合入后从最新 main 建 feat/source-paths，只发现当前用户默认和 CODEX_HOME / CLAUDE_CONFIG_DIR，取消重新发现按钮。

三 PR 各自递增一次版本、各自验证最终 HEAD，不再统一交付。此次只在现有计划分支修改文档；独立系统验收 todo 不变。

原 active 的 25 份计划全部归档；M1–M11 重新核对自身验收条件。历史计划共 **36 份：8 份已实现、26 份部分完成、2 份已被取代**。部分完成通常是“实现已完成、验收有尾项”；用量历史计划仍有原生及真实性能验收责任。除上述三份新计划外，`active/` 保留上述整合计划、2026-10-13 的 UI 调整计划与[控件材质与浮层可读性计划](active/2026-10-13-control-material.md)（用户已确认：边界 ≥ 3:1、克制受光与焦点/选中柔光、操作型浮层实色；已实施且视觉验收通过；UI 独立分支准备发布，待 PR 门禁），不再分别追逐旧计划的尾项。

- **PR #4 已合并**：4cde2b33 的最终两套检查成功，main b1f8b635 的 CI/Release 与五个安装场景成功。A01–A06、M01、N09 保持关闭。
- **P2：N12 版本与真实旧包升级**，历史 0.1.0 与功能轮次 0.2.0 已发布；版本工具、不可变发布和日志保留/删除两场景升级已实跑通过。当前 0.2.1 修复新建草稿身份读取，仍需自己的 PR/main 发布门禁。
- **P2：N10–N11**，当前隐私强取证和原生统一验收；N01–N09 不重复实施。
- **P3：N12 其余系统验收、N13**，随实际发布做 D5/系统签字，正确性闭合后复测性能。

按用户要求补齐的 [M05/M06 安装验收修复设计与实施步骤](active/2026-10-11-consolidated-remaining-work.md#n12-install-design)已完整落地：逐身份 JSON 事实、生成器契约、实际启动、日志路由、PS7 正反例及前置门禁均已验证，`a64d340` 的 A01–A06 和后续 Actions 暴露的编码/调用/目录/活动日志读取缺口均闭合。fa911dd 的 CI、真实五场景和工件已验，不再将必需安装实跑列为未完成。

保留 13 个责任编号；当前为 **9 项完成、1 项部分完成、3 项本轮验收未执行**，共 4 项待办。N01–N09 已完成，N12 必需安装、版本及真实升级已有实跑证据；当前发布补修待新门禁，其余系统/发布签字仍待验；N10/N11/N13 保留未执行状态。历史“合并待验”记录不再代表 PR #4 当前状态。

必须保持用户定稿：一个 `~/.tokenscope/history.db`，只保存精简 token/cwd 元数据；全部代理有效用量纳入，不做主子关系产品；源日志删除/停用仍查得到已保存历史；CCS 设置页只手动单次导入；已有事实与导入求并集，重复导入不增加用量。较新历史库规则优先于旧“源日志是唯一事实/sidechain 排除”规则。

## 已完成能力与证据边界

| 范围 | 当前结论 |
| --- | --- |
| Claude/Codex 解析、四桶归一化、时间/项目/模型聚合、冻结查询与分页 | 主体与多轮修复已经实现；历史并集/升级/规模边界见 N01–N07 |
| GUI、设置 TOML/事务、关闭三态、导航/日期控件/费用浮层、窗口/托盘/自启接线 | 既有功能实现完成；自身未闭合的原生操作/系统验收合并至 N10–N12 |
| 无内置价、三来源独立候选、完整优先/最高请求费用、分段峰谷/缓存三态 | 已实现；跨应用、有价分页、完整候选文案及普通价格隔离通过，N08 完成；当前原生验收由 N11 承接 |
| 项目路径统一、子目录归根/越界切换、模型等价/可信名称 | 核心与多轮边界修复已经实现；跨应用和原始模型分页已验证，当前原生链见 N11 |
| 隐私同意闸门 | P01–P05 已实现；P06 强取证与当前完整矩阵仍未闭合，不能写成全部通过 |
| 历史库与 CCS 手动预览/确认、全部代理、Codex 归档发现 | 旧污染纠错、非法迁移、容量、百万 Rust 峰值、退出、证据登记、价格隔离及普通/acceptance 门禁已验证；原生/性能仍有责任，H01–H08 不能全部关闭 |
| 既有浏览器/原生 QA | 历史 60 场景/17 契约/243 断言、CSP、真实 TTL、无强制当前显示器 150% 有产物；不证明最新功能/安装包全验 |
| CI / main 预发布 | fa911dd 的 PR 两套检查和真实五个安装场景已验；已有 main 预发布/校验和/attestation 是历史证据。固定旧包、正式 v* 与当前发布签字仍待验 |

`a58684c` 普通与 acceptance 完整门禁保留为时点基线：普通根库 **456 通过/10 忽略**、壳 **42 通过**、前端 **361 通过**；acceptance 根库 **456 通过/10 忽略**、壳 **47 通过**。双 manifest fmt、普通/acceptance Clippy（`-D warnings`）、前端 typecheck/format:check/build 和发布元数据 **7 项**通过，Cargo 均带 `--locked`，详见[M01 修复复核](../reviews/2026-10-11-main-merge-readiness.md#m01-closed)。随后安装提交未改产品源码。该轮实际迁移 **8 通过**、native_acceptance **3 通过/2 忽略**、review_regressions **29 通过**、metadata **7 通过**；未重复全套。main 可快进；安装接线不能代替验收实跑。门禁终态（双审计循环收敛，`6fab12f`）：根库 **461 + acceptance**、壳库 **45**、前端 **364**、python **36**、pwsh 契约 **22** 全绿，此前数字保留为时点基线。本批索引随 A01–A06 修复入库，当时的“验证失败例外”已消失。

最新独立核验（2026-10-12）：普通与 acceptance 根库各 **462 通过/10 忽略**，壳 **46/51**、前端 **364**、Python **36**、PS7 **22** 和全部格式/Clippy/typecheck/build 通过；实际安装编排的仓外探针通过当前日志/单实例/关闭前后事实/证据/环境恢复。外部启动和关闭被替换，未实跑 NSIS。A01–A06 已独立确认关闭；231f667 及后续仅文档 e31d1b6 均无 PR/Actions 结果，main 仍祖先。代码与本地门禁就绪，但正式合并须当前 PR 的 CI/gate 和 Install test/install-test 绿灯。N12 保持部分完成；一条 P3 设置页内部审计文案原归 N11 清理，2026-10-13 起改由 UI 调整计划 U05 承担，不成为新合并阻断。详见[最新复核](../reviews/2026-10-11-main-merge-readiness.md#current-merge-readiness)。

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
