# TokenScope 计划与完成状态总账

> 更新日期：2026-10-21。**用户明确要求 > 较新计划 > 较旧计划 > 未确认的设计建议。**
> 唯一当前待办入口：[剩余任务整合与验收计划](active/2026-10-11-consolidated-remaining-work.md)。真实跨午夜、长时间休眠、跨显示器在[独立系统验收待办](system-acceptance-todo.md)维护，均未排期、未标通过。[审计索引](audits/README.md)保留历史时点结论。

## 现在执行什么

功能专项均已交付，不再执行旧分支或重做已关闭任务。剩余责任如下：

| 责任 | 当前状态与下一步 |
| --- | --- |
| N10 隐私强验收 | P01–P05 已实现；补齐 P06 原生取证与完整矩阵，尚未关闭 |
| N11 原生统一验收 | 项目、模型、CCS、价格同步等组合验收仍待执行，不能用一般 E2E 绿灯替代 |
| N12 系统与发布签字 | 安装、版本递增、真实升级及连续链已有成功证据；其余系统验收和签字仍待验 |
| N13 真实性能 | 历史库冷/热查询、多根真实数据性能复测未完成，按只读来源与隔离输出执行 |
| N14 来源目录遗留 | 第 1、2、3 项已关闭；预设切换重复查询及托盘隐藏通知随 PR #23 / v0.5.6 交付，当前仅第 4 项权限拒绝 fixture 待做 |

N01–N09、N15、N16 已完成；16 个责任编号中为 **11 项完成、2 项部分完成（N12/N14）、3 项专项验收未闭合（N10/N11/N13）**，共 5 个仍开放的责任编号，不等于 5 个单独操作。

## 已交付与证据边界

- PR #7–#13 已分别合入并发布 v0.3.0–v0.4.0，计划见下方第二批归档。
- PR #14 来源目录功能已合入；v0.5.0 因发布测试架失败未公开，其功能由 PR #15 的 v0.5.1 首次公开。PR #15 合并 `9ea0c584`，[Release 39291272967](https://github.com/ywltby/tokenscope/actions/runs/39291272967) 的两条连续升级链成功，N14 第 1 项关闭。
- PR #16 启动取证与恢复、SQLite 并发打开修复合并 `0ffab19`，v0.5.2 已发布；main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39353860584)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39353860657)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39353860618) 全成功。N14 第 3 项按原验收条件关闭，旧超时根因仍未证实。
- PR #17（`89aec76`）仅收录研究资料及版本/基线文件，不改变产品行为，也不自动增加开发任务。本次计划收尾以已核验的 v0.5.2 发布证据为基准，不借用其他版本结果补记未执行验收。
- PR #19 打开锁等待回归修复合并 `1156385`，v0.5.5 已发布；main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39452749833)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39452749860)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39452749887) 全成功，N15 关闭。
- PR #21 / #22 已验证版本不变时 Release 只跑 gate、六个下游全部 skipped；PR #23 产品修复合并 `d064f98` 并发布 v0.5.6，main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39568790001)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39568789964)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39568789984) 全成功（安装、六个单跳、两条连续链）。N14 第 2 项及 N16 关闭，版本与发布解耦两条路径均有实跑证据。

`active/` 保留整合计划与待确认的[剩余验收收口计划](active/2026-10-21-acceptance-closure.md)（AC01–AC12，承接全部 5 个开放编号）；[合入与发布解耦](archive/implemented/2026-10-21-release-on-version-change.md)（N16）已完成并归档。归档共 **48 份：19 份已实现、27 份部分完成、2 份已被取代**。部分完成通常表示实现存在但自身验收有尾项；归档表示不再独立执行，不表示所有验收通过。

必须保持用户定稿：一个 `~/.tokenscope/history.db`，只保存精简 token/cwd 元数据；全部代理有效用量纳入；源日志删除或停用仍查得到历史；CCS 设置页只手动单次导入，导入与已有事实求并集。较新规则优先于旧“现存日志是唯一事实”等历史设计。

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
| [PR3：当前用户日志目录发现与追加路径管理](archive/partial/2026-10-14-source-discovery-and-additional-paths.md) | #14 / v0.5.0（未发布，由 0.5.1 公开） | 部分完成 | N14（预设切换重复批次、权限拒绝 fixture；连续升级链与启动取证恢复已关闭）；N13（真实性能） |

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
