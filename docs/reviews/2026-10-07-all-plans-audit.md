# 全部计划实现审核（2026-10-07）

> **2026-10-11 历史状态：** 本文保留当时审查发现；后续修复/需求已有变化，当前完成与待办采用 [总账](../plans/README.md) 和 [整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)。不要把原“仍存在/待执行”直接当作当前缺陷。

审核基线：`a42e843`，分支 `docs/product-review-plan`。本次只审核并编写修复计划，不修改实现。

**结论：主体功能已落地，但不能认定“全部计划已正确实现并验收”。确认 10 类问题，其中 5 类 P1 涉及计费、日期或缓存正确性，5 类 P2 涉及异常数据、交互及诊断。** 既有完整门禁通过，但定向复现暴露了测试覆盖缺口。修复任务见 [独立修复计划](../plans/archive/implemented/2026-10-07-all-plans-audit-remediation.md)。

## 1. 审核范围与覆盖规则

检查 `archive/implemented/` 的 M1–M11、`active/` 的 13 份文档，以及 `DESIGN.md`、`docs/stats-semantics.md` 和 D5 清单；结合当前实现、测试和执行记录判断。下表是需求覆盖账，不表示每项都重新做过真机验收。

有冲突时以用户最新决策和后续计划的明确修订为准；同一天的文档按实际修订内容判断，不能只比较文件日期。

| 计划 | 本次采用的现行要求及审核结果 |
| --- | --- |
| [M1 Claude Code](../plans/archive/implemented/2026-10-03-m1-claude-code-adapter.md) | 保留只读适配器、坏行统计、全局去重；当前主链与 fixture 存在，空结果时诊断展示仍有 R08。 |
| [M2 Codex](../plans/archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) | Codex 四桶归一化与去重以路线图 B1/B2、统计口径文档为准；旧 43 条内置表、旧前缀策略不再验收。 |
| [M3 GUI](../plans/archive/partial/2026-10-03-m3-tauri-vue-gui.md) | GUI 为唯一形态；不要求恢复 CLI；旧关窗缩托盘被三态关闭取代，见 R07。 |
| [M4 缓存/外置价格](../plans/archive/partial/2026-10-03-m4-cache-pricing-settings.md) | 缓存必须不改变结果；R05 违反此要求。价格格式以分段计划及三态价格为准。 |
| [M5 OpenRouter](../plans/archive/partial/2026-10-04-m5-openrouter-pricing-sync.md) | 保留在线同步/离线快照，来源角色改为补充；其数值缓存价应保留，见 R01。 |
| [M6 时区](../plans/archive/implemented/2026-10-04-m6-timezone-resolution.md) | UTC 存储、统计时区解析继续有效；UI 的 local 与 picker 桥接有 R03。 |
| [M7 明细](../plans/archive/partial/2026-10-04-m7-event-drilldown.md) | 共用 report 管线、下钻、四桶/费用同源存在；后端唯一游标未用于前端行身份，见 R10。 |
| [M8 桌面体验](../plans/archive/partial/2026-10-04-m8-desktop-experience.md) | 单实例/托盘/窗口记忆/自启接线存在；真机验收后延，不能据此宣称本次全部通过。 |
| [M9 models.dev](../plans/archive/partial/2026-10-04-m9-modelsdev-source.md) | 主源、离线快照有效；取消内置兜底；非法基础价处理有 R06。 |
| [M10 日期范围](../plans/archive/implemented/2026-10-04-m10-date-range.md) | GUI 自然日字符串契约、区间闭边界继续有效；真实 picker 行为见 R03。 |
| [M11 索引/自动同步](../plans/archive/partial/2026-10-04-m11-pricing-cache-autosync.md) | 持久索引/进程缓存/自动同步存在；修复 R01/R06 必须阻止旧派生索引继续携带错误结果。 |
| [产品路线图](../plans/archive/partial/2026-10-05-product-review-and-roadmap.md) | A/B/C/D1–D4 大部分已实现；C4 快照一致性、B4 诊断和缓存身份仍有 R04/R05/R08；D5 不算已完成。 |
| [发布阻断修复](../plans/archive/partial/2026-10-05-release-blockers-remediation.md) | 单飞 RAII、完整精度游标、v1 非零缓存价、原子写工具等存在；Task 4 内置表要求已废弃。四类落盘原子性仍漏视图缓存 R04。 |
| [定价来源策略](../plans/archive/partial/2026-10-05-pricing-source-policy.md) | 无生产内置价格、首启横幅、models.dev 离线缓存已落地；来源优先级不能覆盖后来“独立候选、按请求估算择价”的要求。 |
| [审阅问题修复](../plans/archive/partial/2026-10-06-review-findings-remediation.md) | 游标补充排序、目录重叠防护、部分同步、组件测试等存在；日期仍需按真实依赖复验 R03。 |
| [二次实现审核修复](../plans/archive/partial/2026-10-06-post-implementation-audit-remediation.md) | 重叠规范化、有效签名旧索引 fixture、Settings 测试存在；UTC 锚往返测试未覆盖真实 picker R03；文档状态仍不一致。 |
| [分段计价/费用明细](../plans/archive/partial/2026-10-06-tiered-pricing-and-request-breakdown.md) | 末段匹配、整请求切档、历史时间、同候选四项单价和 breakdown 已落地；时间档选择有 R02，价格校验有 R06。 |
| [设计系统实施](../plans/archive/partial/2026-10-06-design-system-implementation.md) | 以重写版本及当前 DESIGN.md 第二版为准；主题 token、卡片/表格/费用浮层主体存在；R08/R09 仍违反诊断及键盘要求。 |
| [视觉差异清单](../plans/archive/superseded/2026-10-06-design-system-visual-gap.md) | 属实施前基线，不把其“现状”当当前缺陷；按现代码重新判断。 |
| [视觉 QA](../plans/archive/partial/2026-10-06-design-system-visual-qa.md) | 有 125% dev 走查记录；100%/150% 未验收，截图目录缺失，导航透出效果需复核。 |
| [Apple 视觉刷新](../plans/archive/superseded/2026-10-06-apple-visual-refresh.md) | 早期步骤被当前 DESIGN.md 和重写后的实施计划收敛；不恢复旧 emoji、旧 token 或已被补齐的占位交互。 |
| [Apple 剩余任务](../plans/archive/partial/2026-10-06-apple-refresh-remaining-tasks.md) | 旧“Settings 暂缓”已被后来实现覆盖；不把这条历史暂缓继续当作禁止修改。 |
| [关闭确认/配置文件](../plans/archive/implemented/2026-10-06-close-confirm-and-settings-file.md) | TOML 迁移、三态关闭、记忆、设置页入口主体符合；关闭命令失败路径缺反馈 R07。新增高级配置组不违反旧“四组”要求。 |
| [缓存读取定价解析](../plans/archive/partial/2026-10-06-cache-read-pricing-resolution.md) | 最终口径为 Unknown / Fixed / SameAsInput、显式 model_policy、完整候选优先；R01 与 R02 表明接入和候选内部选择未完成。 |

现行定价规则特别说明：先按末段完整/有边界前缀匹配，按**请求的历史时间和实际 token**计算可适用档位；先在完整候选中取最高总费用，没有完整候选才取已知部分最高。不能把不适用的峰值档强加给请求；不能拼接不同渠道单价；缺失缓存价不能自动当 0 或输入价。以上明确要求覆盖旧计划中不加完整性筛选的“取最高”表述。

## 2. 已确认的问题

### R01 · P1 · OpenRouter 有缓存价格，导入后仍成了未知

- 位置：`src/pricing.rs:1513–1514`；上游 `src/openrouter.rs:224–225` 已保存数值。
- 复现：OpenRouter v2 条目 input=`0.000002` USD/token、cache_read=`0.0000005` USD/token；请求输入和缓存读各 1M。
- 实际：`$2`，`complete=false`，1M 缓存读未计价。应为 `$2.5` 且完整。
- 原因：基础 cache_read/cache_write 被硬编码为 Unknown。最新缓存读计划 Task 3 要求来源数值转 Fixed，旧“OpenRouter 缓存未知”规则已被覆盖。
- 修复还需失效旧索引；只改导入代码，已有索引仍可能保留旧 Unknown。

### R02 · P1 · 时间档选择破坏谷价覆盖和完整性优先

- 位置：`src/pricing.rs:1761–1775`。
- 复现 A：渠道 A 基础 `$10/M`，08:00–20:00 谷价 `$2/M`；渠道 B 固定 `$5/M`。12:00 的 1M 输入实际选 A 基础 `$10`，应按 A 当前 `$2` 与 B `$5` 比较，选择 B。
- 复现 B：同渠道两个适用 schedule：不完整规则 input=`10`、cache_read 缺失；完整规则 input=`2`、cache_read=`1`。输入与缓存读各 1M，实际选 `$10` 部分价，应选完整 `$3`。
- 原因：候选内先把基础档当永久竞争者，再只比较已知小计；在全局“完整优先”前已丢掉正确时间档。
- 依据：分段计划 Task 4A 的时间覆盖规则 + 最新缓存读计划 Task 4 的完整性优先。两者都必须生效。

### R03 · P1 · 手选日期仍会偏一天，“本机时区”会抛异常

- 位置：`frontend/src/components/DateRangeSelect.vue:98–113`；`frontend/src/lib/dates.ts:11–33`。
- 用项目实际 Naive UI 依赖的 date-fns 复现：本机上海手选 2026-10-06，UTC setter 得到 2026-10-05；本机洛杉矶回显 UTC 锚 2026-10-06，picker 显示 2026-10-05。
- `todayInTz("local")` 直接传 `Intl.DateTimeFormat`，实际抛 `RangeError: Invalid time zone specified: local`。Dashboard 标题及日期快捷项都调用它。
- 原因：Naive UI 使用本机日历毫秒，没有配置成 UTC；现有测试向 stub 喂 `Date.UTC`，只证明两段自写转换互逆。不得把这类测试当成实际控件验收。

### R04 · P1 · 视图快照会混搭筛选数据，晚到缓存可覆盖新结果

- 位置：`frontend/src/views/Dashboard.vue:140–161`、`:207–230`、`:257–260`；`src-tauri/src/commands.rs:423`。
- 真实 Vue 响应式代码 + deferred IPC 复现 A：已有 all 汇总 input=10，切 claude 后先返回明细、挂起新汇总，实际保存 `filters.agent="claude"` + 旧 all 汇总 input=10。
- 复现 B：新汇总 input=99 已完成，同筛选旧快照 input=10 晚到，最终退回 10；`stale=true`、`loading=false`，却持续显示“后台刷新中”。
- 原因：保存只检查 eventsKey，没有汇总查询身份；恢复与 immediate 查询并发且没有新旧保护；状态判断又把 stale 放在失败/实际加载之前。
- 另外保存仍直接覆盖写文件，未使用现成 `fsutil::atomic_write`。原子写和查询一致性必须一起修，前者无法解决后者。

### R05 · P1 · 事件缓存身份缺少解析来源和根目录

- 位置：`src/cache.rs:126–190`、`src/report.rs:593–601`；项目名依赖 `src/source/claude.rs:75` 的相对根路径。
- 隔离 fixture 复现：日志在 `logs/a/b/s.jsonl`；先以 `logs` 为根得到项目 `a\b`，再改根为 `logs/a`。缓存结果仍为 `a\b`，强制重解析为 `b`。
- 同目录先用 ClaudeSource、再用 CodexSource：缓存路径返回 1 个标为 Codex 的旧 Claude 事件，重解析返回 0 个事件。
- 原因：SQL 查找只比较 path/size/mtime，忽略已存 agent，更没有 root 上下文；命中后直接给旧事件赋传入 agent。
- 当前单次采集目录去重不能解决跨次配置变化，违反“缓存仅优化，结果必须与无缓存一致”。

### R06 · P2 · 非法价格能产生 NaN 或负费用

- 位置：`src/pricing.rs:350–362`、`:1435–1450`；`src/openrouter.rs:112–124`。
- 外置 `input=nan` 实得 `cost=NaN, complete=true`，无告警；检查只有 `<0`，未排除非有限数。
- models.dev 快照基础 input=`-2` 被校验告警后，错误分支仍“仅保留基础价”，实际返回 `$-2, complete=true`。
- 需要区分坏基础价和坏分段，统一有限非负校验；显式 0 仍合法。加载失败不能把非法数值保留成完整计费结果。

### R07 · P2 · 记忆关闭动作失败后，弹窗消失且无反馈

- 位置：`frontend/src/App.vue:71–73`；`src-tauri/src/commands.rs:204–209`。
- 可达路径：损坏 settings.toml 或写入失败，选择“记忆 + 退出/最小化”。前端先关弹窗且不捕获 reject，后端持久化失败提前返回，窗口保持但没有错误或重试入口。
- 后端拒绝覆盖坏设置是正确行为；应补前端异步状态与可操作错误，而不是吞掉失败继续覆盖配置。

### R08 · P2 · 空结果把采集故障一并隐藏

- 位置：`frontend/src/views/Dashboard.vue:396–427`；`frontend/src/components/UsageTable.vue:155–165`。
- 代码分支可确认：`report.groups=[]` 时只渲染“暂无数据”；承载 `report.sources`/`report.warnings` 的 UsageTable 不挂载。
- 当目录有日志但全是坏行或读取失败时，source_status 可能仍为 ready，页面无法解释为什么没有数据。非空部分结果也缺少醒目的 partial 提示。
- 应独立呈现报告诊断；正常空范围与不完整采集必须可区分。不要把所有 warning 都等同于坏行，应按实际诊断类别给文案。

### R09 · P2 · 命中率说明有 tabindex，但 focus 读不到公式

- 位置：`frontend/src/components/SummaryCards.vue:71–73`。
- 真实 Naive UI + happy-dom 复现：focus 不显示公式，hover 显示。默认 NTooltip 仍按 hover 触发。
- 现有测试透传 tooltip 内容，仅断言 tabindex，未验证交互；不符合 DESIGN.md 明确要求的 hover/focus 等价。

### R10 · P2 · 明细表仍用秒级展示字段拼行键

- 位置：`frontend/src/components/EventTable.vue:212–214`。
- 合法合成场景：相同秒、agent、model、session 的两条不同请求，现 rowKey 完全相同；后端 cursor 已携带稳定的唯一排序身份。
- 这会给表格重复身份，存在行复用/展开状态错误风险。当前抽查未观察到真实日志碰撞，**不声称已发生生产丢行**；但该实现不能满足已有同秒分页不重不漏的端到端要求。

## 3. 验收与文档问题（不混作已复现运行故障）

1. **导航内容透出需真机复核。** `App.vue:91/120/174` 中导航与 `overflow-y:auto` 的滚动容器互为兄弟，内容受容器裁剪；这与 DESIGN.md §2“内容在导航背后滚过”的要求有结构性冲突。QA 文档却写已通过。本次未启动 WebView 验证，先登记为证据冲突，修复计划要求用截图/布局测量判定，不直接宣称视觉回归已复现。
2. **截图证据缺失。** QA 指向 `docs/plans/active/screenshots/2026-10-06/`，当前工作区该目录不存在；不推断历史走查没做，但当前审核无法核验图片。
3. **D5 仍未全部完成。** 安装/升级/卸载等按已有用户决策后延；桌面、隐私离线仍有未填项，100%/150% 缩放尚无完整证据。dev 模式 125% 走查和自动化不能替代这些项目。
4. **状态账陈旧。** README 仍强调“仅剩 D5”，缺少后续计划总账；执行记录中有旧测试数量、未勾选清单、`本次提交` 占位。历史记录保留，但应标明“当时完成/被覆盖/现存回归”，不能继续统一宣称当前全部完成。

## 4. 本次验证与限制

实际重新执行并通过：

- 根库 `cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets --offline -- -D warnings`、`cargo test --workspace --offline --quiet`：143 单测 + 27 集成测试，合计 170；真实数据性能测试 1 项保持 ignored。
- Tauri 独立 manifest 的 fmt/clippy/test：14 项测试通过。
- 前端 typecheck、format:check、Vitest（15 文件 / 150 项）、build 通过；构建仍有大 chunk 提示，未将其当作本轮正确性缺陷。
- 定价外部 Rust harness 复现 R01/R02/R06；缓存外部 Rust harness 复现 R05；真实 date-fns/Intl 复现 R03；协作审阅用实际 Vue 响应式与 mock IPC 复现 R04，用真实 NTooltip 复现 R09。父审阅复查相关实现并重跑定价、缓存和日期复现。

临时 Rust harness 位于系统 TEMP：`tokenscope-pricing-audit-e7f3ef4eb0d04263ad3d98dbf735bece/`、`tokenscope-cache-audit-oct7.rs`。复现数据和预期已写进修复计划，执行者不依赖这些临时文件。新增复现不写真实 agent 日志或 `~/.tokenscope`；未运行真实性能测试、安装包验收、系统自启变更或在线同步。未做本次套件前后真实缓存哈希对比，因此不把历史密闭性记录充当本次证明。

“门禁通过”仅说明现有用例通过；上述问题应以最小失败回归补齐，再进行修复。完整视觉、桌面发布验收仍按独立证据清单推进。
