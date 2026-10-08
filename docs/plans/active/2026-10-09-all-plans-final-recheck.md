# 全计划终态复核与遗留修复 Implementation Plan

> **执行说明：** 使用 `executing-plans` 技能逐项执行。本文是复核结果及修复计划。
>
> **执行状态（2026-10-09 实施完毕，收尾账见 §7）：** AP01–AP07 已逐项实现并独立提交（`6a445d6`、`ed10ed9`、`f538d35`、`a2a4d11`、`c31bba2`、`d68bbb2`），每个回归先红后绿；AP08 的**自动化部分**已完成——浏览器契约矩阵扩充为 60 场景 / 17 条必需契约 / 243 条断言、零失败，**原生与系统部分**（query 过期原生轮、125%/150% 缩放、系统深色首帧、CSP 脚本哨兵）与 D5 后延项保持待验；AP09 完成状态归并。因此本计划**仍未归档**：未验项的唯一活跃入口是 [D5 验收清单](../d5-acceptance-checklist.md) 与 [原生验收记录](2026-10-08-native-recheck-qa.md)。

**Goal:** 修复全计划复核发现的来源配置旁路、设置异步状态与错误展示缺口，补齐仍缺失的验收证据，统一计划完成度。

**Architecture:** 保留 source → report/query → Tauri → Vue、不可变查询快照及后端统一计价。让查询、来源状态、重建和配置保存共用有效来源语义；前端所有异步结果采用一致的版本及卸载守卫。原生验收与实现修复分别记录。

**Tech Stack:** Rust stable、Tauri 2、SQLite、Vue 3、Naive UI、Vitest、Playwright/Chromium、Windows/WebView2。

**复核基线：** `5ec826cb0510de1a5072de04a458aea4e7f9abef`，分支 `docs/product-review-plan`。覆盖当时 `docs/plans/` 全部 **31 份 Markdown**（11 份归档里程碑、18 份 active 计划/差异/QA 文档、索引及 D5 清单）。CLI、编译期内置价格、先扫描者认领重叠文件等已被明确替代的历史设计，不作为缺失功能重新实现。

## 1. 结论与证据边界

**不能认定全部计划闭环。** 主要功能及历次修复确已落地，现有完整门禁、浏览器矩阵通过；但新定向探针复现了六个错误现象，另有两处错误路径经源码确认。RC11 和 D5 的未验项目仍保留在现行文档中，并非本次新增产品需求。

本次进行了逐份目标/任务/后续替代关系梳理，以及解析、去重、缓存、计价、查询、设置、桌面生命周期和前端关键路径复核。表中“已有实现”表示查到实现和相应测试证据，不等于穷尽所有输入、所有平台或历史真机操作。

### 实际重跑

| 验证 | 本次结果 | 能证明的范围 |
| --- | --- | --- |
| 仓库 `.githooks/pre-commit` 完整门禁 | 通过：根库 fmt/clippy/全部非 ignored 测试，壳 fmt/clippy/17 测试，前端 typecheck/format/24 文件 280 测试 | 已有自动测试覆盖范围；根库单元测试 178 项，另有集成测试 |
| `pnpm --dir frontend build` | 通过；存在 Vite 大 chunk 提示 | 前端生产构建成功，不等价于原生安装包验收 |
| 生产前端 `check-ui-contracts.mjs --phase verify` | **28 场景、12 条具名契约、182 条断言，零失败**；Chromium 148.0.7778.96 | 真实 App/组件、合成 IPC；不替代真实后端和系统 DPI |
| 新增临时前端探针 | 3 项均按预期失败 | 晚到拒绝、卸载后通知、已有价格却声称全未知 |
| 新增临时 Rust 隔离探针 | 1 项失败，记录 3 个独立错误结果 | 默认来源重叠获准、停用后重建仍采集、损坏配置查询仍获准 |

本次**未重新启动原生应用、未改系统 DPI/主题、未安装/升级/卸载、未运行真实数据缓存重建或 ignored 性能测试**。Rust 探针通过 `acceptance` feature 冻结唯一临时根；原始日志与真实缓存未作为复现输入。复核于 2026-10-08 开始，计划于 2026-10-09 整理完成。

> 上表数字是**复核当时**的口径（矩阵 28 场景 / 12 契约 / 182 断言），用于界定"哪些反例当时被复现"；实施完成后的最新门禁与矩阵数字见 §7 执行账，不把旧计数当当前证据。

本地产物（gitignore，其他检出可能不存在）：

- `qa-artifacts/all-plans-recheck-2026-10-08/browser/measurements.json` 及截图：包含提交、浏览器版本、逐场景及逐契约结果。
- 同目录 `PlanAudit.test.ts`、`plan_audit.rs`：手写合成探针。临时测试入口已从生产测试目录移除，避免把预期红灯混入门禁；下文给出重建场景，不以这些本地文件存在为执行前提。
- Rust 输出：`default_source_overlap_accepted=true, disabled_sources_rebuild_events=2, corrupt_settings_query_accepted=true`。
- 前端输出：保存 `false` 后旧读取拒绝使值成为 `null`；卸载后仍调用成功通知一次；`hasAnyPricing=true` 时横幅仍含“当前费用仅能显示为未知”。

## 2. 全部文档的覆盖与当前状态

> **覆盖关系索引（AP09）**：下表“本次判定与承接”列是全部 31 份文档的**唯一权威覆盖关系索引**——某份历史文档里未勾选的任务，若已由后续计划实现，其状态以本列为准并由该列链接到具体承接项/修复提交；仍有缺陷的链接 AP 编号。历史文本不整段重写，归档目录的旧勾选也不用于推导“全体验已验”。

路径前缀分别为 `archive/implemented/` 与 `active/`；下表文件名全部相对 `docs/plans/`。历史执行记录保留，不把旧测试总数当当前证据。

| 文档 | 现行实现/证据映射 | 本次判定与承接 |
| --- | --- | --- |
| archive/implemented/2026-10-03-m1-claude-code-adapter.md | `source/claude.rs`、`model.rs`、`aggregate.rs`、`e2e_claude`、黄金对账 | 解析/统计已有实现；CLI/render/内置价按后续决策替代 |
| archive/implemented/2026-10-03-m2-codex-adapter-pricing.md | `source/codex.rs`、`dedupe.rs`、`e2e_codex`、`token_bucket_contract` | 已有实现；20 行额度通知误报已由 `5ec826c` 修复；同模型同用量启发式去重是已披露限制 |
| archive/implemented/2026-10-03-m3-tauri-vue-gui.md | `App.vue`、Dashboard/图表/表格、commands、浏览器矩阵 | GUI 已有；来源错误路径见 AP02/AP06，原生完成度见 AP08 |
| archive/implemented/2026-10-03-m4-cache-pricing-settings.md | `cache.rs`、`cache_behavior`、Settings | 缓存/外置价已有；重建未使用用户来源配置，AP03 |
| archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md | `openrouter.rs`、`pricing.rs`、数字价格/原子保存测试 | 已有；旧来源优先规则被候选最高完整价策略替代；横幅 AP05 |
| archive/implemented/2026-10-04-m6-timezone-resolution.md | `resolve_tz`、日期字符串契约、UTC 缓存测试 | 已有；CLI 参数是历史面，当前从 GUI/IPC 进入 |
| archive/implemented/2026-10-04-m7-event-drilldown.md | query/report、EventTable、分页/黄金对账 | 已有；当前固定会话分页替代逐页重新采集 |
| archive/implemented/2026-10-04-m8-desktop-experience.md | 壳单实例/托盘/window_state/自启 | 实现已有；默认最小化失败路径 AP07，安装/系统验收 AP08 |
| archive/implemented/2026-10-04-m9-modelsdev-source.md | `modelsdev.rs`、v1/v2/tiers、有效候选状态测试 | 已有；旧四层策略不再适用；状态展示 AP05 |
| archive/implemented/2026-10-04-m10-date-range.md | DateRangeSelect、dates、time_filter_indices | 已有；真实 picker、local/DST/跨年边界测试与浏览器回归通过 |
| archive/implemented/2026-10-04-m11-pricing-cache-autosync.md | 价格索引冷启动子进程测试、设置开关、自动线程 | 实现已有；开关异步状态 AP04；休眠/自启仍属 AP08 |
| active/2026-10-05-product-review-and-roadmap.md | A/B/C/D 主线实现及后续 R/F/SF/RC 修复 | C1/C4、D4 仍有入口/错误路径缺口，AP01–04/AP06–07；D5 后延 |
| active/2026-10-05-release-blockers-remediation.md | 单飞 RAII、原子写、三态单价、UTC 游标 | 主要修复已被更强后续实现覆盖；AP02/AP07/AP08 承接剩余边界 |
| active/2026-10-05-pricing-source-policy.md | 无编译期内置价、有效来源 DTO、全局横幅 | 后端已有；“缺主源”不能表达为“所有费用未知”，AP05 |
| active/2026-10-06-apple-visual-refresh.md | tokens、导航、卡片、筛选、通知 | 已由后续设计落地/UX/RC 计划覆盖；浏览器回归通过；AP08 保留系统验收 |
| active/2026-10-06-apple-refresh-remaining-tasks.md | 同上 Task 3–7 | Task 6 原暂缓部分已由后续 Settings/通知实现覆盖；旧任务状态需 AP09 归并 |
| active/2026-10-06-design-system-implementation.md | styles/主题适配、真实组件矩阵 | 已有；错误语义 AP04–06，原生组合 AP08 |
| active/2026-10-06-design-system-visual-gap.md | 第二版历史基线差异 | 基线记录，不能作为当前未实现清单；AP09 标明覆盖关系 |
| active/2026-10-06-design-system-visual-qa.md | 历史视觉记录，后续修正了吸顶等结论 | 当前浏览器证据有效；系统缩放/首帧以最新 RC11 未验项为准 |
| active/2026-10-06-close-confirm-and-settings-file.md | settings TOML/迁移、CloseConfirmDialog、close_resolve | 已有；默认动作保存竞态 AP04、已记忆路径隐藏失败 AP07 |
| active/2026-10-06-tiered-pricing-and-request-breakdown.md | PricePlan/segments/schedules、四桶 breakdown、tieredPrice | 已有；候选同源、完整优先、峰谷与非法价回归保留；原生范围见 AP08 |
| active/2026-10-06-cache-read-pricing-resolution.md | RateSpec、SameAsInput、候选两阶段、明细来源 | 已有；不重开已修精度/三态问题，不恢复无条件外置覆盖 |
| active/2026-10-06-review-findings-remediation.md | 游标、来源冲突、同步状态、真实组件 | 采集层拒绝重叠有效，但保存入口漏默认来源，AP01；状态 AP04–06 |
| active/2026-10-06-post-implementation-audit-remediation.md | 日期双向绑定、归属缓存、索引有效签名、Settings 测试 | 既有修复保留；来源/设置剩余边界 AP01–04，文档 AP09 |
| active/2026-10-07-all-plans-audit-remediation.md | R01–R10 计价、缓存、快照、关闭与 tooltip | 主要修复已有；不是全部关闭路径均可恢复，AP07 |
| active/2026-10-07-post-remediation-recheck-fixes.md | 真 v3 迁移、非法候选拒绝、视图同批次、导航和真实 tooltip | 既有 F01–F08 修复保留；close_resolve 的 hide 透传不覆盖默认关窗分支，AP07 |
| active/2026-10-07-ui-ux-review-remediation.md | SF01–11/UX00–10 + RC 承接 | 查询/价格/算术/布局已落地；SF09 保存漏口、UX06 异步失败、SF10 展示仍需 AP01/AP04/AP05；UX10/SF01 原生 AP08 |
| active/2026-10-08-recheck-remediation.md | RC01–10 及部分 RC11 | RC01/02/05–08 已有修复；RC03/04 仍有异步结果边界；RC09 本次矩阵通过但未覆盖新反例；RC11 未闭合 |
| active/2026-10-08-native-recheck-qa.md | release+acceptance 原生存档与逐项限制 | 历史局部验收证据；明确未验项 AP08，横幅发现 AP05；不把 WebView 缩放当系统 DPI |
| d5-acceptance-checklist.md | 发布签字清单 | 安装/升级/卸载、部分桌面与隐私时序仍后延；AP08 单列，不自动操作系统 |
| README.md | 滚动索引及历史总账 | 已诚实保留 RC11/D5 未验，但旧 active 清单/完成勾选未统一；AP09 |

## 3. 已确认问题与优先级

以下源码行号对应复核基线；后续实施以函数名定位。

| ID | 优先级 | 触发、结果与证据 | 原计划 |
| --- | --- | --- | --- |
| AP01 | P1 | 无来源配置时，将 Claude 目录设为默认 Codex 根；`source_config_set_impl` 对另一来源 `None` 返回“不参与校验”，实际 `None` 表示默认启用。保存成功，之后采集层又因冲突拒绝，用户被允许保存不可用配置。`commands.rs:395`；隔离实测 accepted=true | C1、SF09 |
| AP02 | P1 | `settings.toml` 损坏/不可读时 `load_settings_or_default` 吞错，query/source_status 重新启用默认来源，可能显示错误数据范围或采集用户已停用的来源。`commands.rs:189,205,216`；损坏配置后 query_begin 实测成功 | C1、来源状态/失败语义 |
| AP03 | P1 | 设置中停用双源后点重建，`refresh_cache`→`rebuild_cache(None)`→`SummaryOptions::default()`，跳过配置；实测仍缓存 2 条合成事件。自定义目录同样未传入。`commands.rs:432`、`report.rs:217` | M4、C1、D5 4.2/4.4 |
| AP04 | P2 | loadSettings 成功分支检查 mutationEpoch，catch 不检查；旧请求拒绝会将已保存 false 变 null、关闭动作标未知。写操作 await 后普遍无 disposed 检查；卸载后仍发成功通知已复现。关闭动作还无 busy/写入代次，快速选择存在乱序落地路径（源码证据，未独立运行此反例）。`Settings.vue:86,302,343,386,440` | UX06、RC03/04 |
| AP05 | P2 | `needsSync=true` 只表示 models.dev 无有效候选，外置或 OR 可以有价；Banner 固定声称全部费用未知。`pricing.rs:1465`、`PricingStatusBanner.vue:visible/bannerText`；真实组件已复现，历史原生 QA 也记录同现象 | 定价策略、SF10、RC04 |
| AP06 | P2 | Dashboard 的 `void loadSources()` 没 catch，source_status 拒绝产生未处理 Promise；没有错误/重试状态，也没有随刷新重新检测，旧 missing/empty 状态可能保留。`Dashboard.vue:307,537`；源码确认，本次未另跑失败探针 | C1/C4、UX06、RC09 |
| AP07 | P2 | 记忆 minimize 后关窗走 `let _ = window.hide(); prevent_close()`，隐藏失败被吞，没有弹窗/重试；仅 close_resolve 路径已修。关窗事件还同步写窗口文件及读设置，未满足 D4 残余 IO 后台化目标。`src-tauri/src/lib.rs:100–106,205`；源码确认，未注入原生 hide/慢磁盘故障 | D4、关闭三态、R07/F08 |
| AP08 | 验收 | RC11 的原生过期恢复、Windows 每监视器 125%/150%、系统深色下跟随系统首帧、CSP 脚本哨兵仍缺证据；D5 后延 | RC11、UX10、D5 |
| AP09 | 文档 | 多份 active 文档任务勾选与执行记录、后续替代关系未统一；归档里程碑仍混有“待安装确认”。不能用归档目录推导全体验已验 | 多轮收尾任务 |

## 4. 不变量与实施边界

1. `AgentSources` 字段缺失 = 默认启用及默认根；只有显式 enabled=false 才停用。保存、普通查询、重建、状态页必须一致。
2. 配置文件不存在可用产品默认值；已存在但解析/读取失败必须明确报错。不得把失败当“用户重新启用了默认来源”。关闭决策可安全退回询问，但不能复用该退路决定采集范围。
3. 配置验证失败不写盘；设置读取失败先于清库/扫描。所有重建测试注入来源、cache_dir、pricing_index、价格文件/双快照；不为测试运行真实重建。
4. 异步成功和失败遵守同一 readSeq/mutationEpoch/disposed 守卫；卸载后允许后端已授权操作结束，但前端不得再改已卸载实例或弹通知。全局必要状态刷新应由仍存活的协调者处理。
5. 查询随机命名空间、live 首页分页守卫、完整保留数据预算、三态价格及候选选择继续保留；不为这些新问题重写已验证模块。
6. 主源待同步与完全无定价是不同状态；不保证“有任意价格就所有模型均能计价”。同步重试和状态重试继续分开。
7. 先失败回归、再最小实现、再门禁；每任务独立提交。下述用例名是**拟新增**，不可把尚不存在的测试命令当已通过证据。
8. 本计划不自动授权发布、安装/卸载、自启注册或改变系统 DPI/主题；D5 继承既有后延决定。

## 5. 实施任务

### AP01：统一默认来源的重叠校验

**文件：** `src-tauri/src/commands.rs`、`src/settings.rs`、`src/report.rs`；`tests/source_overlap.rs`、壳 command 测试。

1. 先新增 `default_enabled_source_participates_in_overlap_validation`：隔离根下另一来源保持 None；保存到该默认根或父/子目录必须 Err，设置字节不变。正反两个 agent 都测。
2. 增加显式停用允许保存、无配置首次保存、显式目录和默认目录一致的边界。不要只测试两个来源均为 Some 的情况。
3. 统一有效来源解析：以 `Settings::source_config()` 得到配置，再处理 enabled 和默认根；在 settings::update 锁内检查将要保存的完整配置。采集层仍保留第二道拒绝。
4. 运行 `cargo test --offline --test source_overlap` 和 `cargo test --offline --manifest-path src-tauri/Cargo.toml default_enabled_source`，先观察目标断言红、实现后绿。
5. 提交：`fix(来源): 将默认启用来源纳入保存校验`。

### AP02：设置读取错误不得切换采集范围

**文件：** `src-tauri/src/commands.rs`、`src/settings.rs`；壳 settings/source/query 测试。

1. 写 `corrupt_settings_rejects_query_and_source_status`、`unreadable_settings_does_not_enable_default_sources`：先保存显式停用/自定义根，随后损坏或 Windows 独占锁住配置；查询与状态均报含原因的错误，采集探针计数为 0。
2. 写 `missing_settings_keeps_documented_defaults` 保留真正首次启动；关闭错误安全退回 Ask 单独测试。
3. 删除采集/状态入口对 `load_settings_or_default` 的依赖，以严格 `Result<Settings>` 读取；共享有效配置解析。关闭行为的 Ask 退路单独命名并记录警告，不改变用户明确退出/取消语义。
4. 验证 `cargo test --offline --manifest-path src-tauri/Cargo.toml settings` 和 `cargo test --offline --test settings_transactions`；接 AP06 验证界面能恢复。
5. 提交：`fix(设置): 配置读取失败时拒绝默认来源回退`。

### AP03：重建缓存使用当前有效来源

**文件：** `src/report.rs`、`src-tauri/src/commands.rs`；新增 `tests/rebuild_source_contract.rs`，扩展壳测试。

1. 写 `rebuild_respects_disabled_sources`：隔离默认目录有 2 条事件，但设置停用双源，重建不得解析这些事件。写 `rebuild_uses_custom_source_roots`：默认与自定义根放不同 ID/数量，重建只含自定义数据。
2. 写 `invalid_settings_does_not_clear_cache_on_rebuild`：故障前已有有效缓存，故障重建报错，缓存保留。再比较普通查询/无缓存/重建后的四桶、请求、来源身份一致。
3. 将重建 API 改为接收明确的采集选项，壳用与 query 相同的已成功读取配置构造；库不隐式读用户设置。先配置/冲突验证，再清理重建。明确“重建派生数据”的提示，不能声称配置外来源也参与。
4. 验证 `cargo test --offline --test rebuild_source_contract --test cache_behavior --test golden_reconciliation`，以及壳 `refresh_cache` 相关测试。
5. 提交：`fix(缓存): 按生效来源配置重建事件缓存`。

### AP04：补齐设置失败结果及写操作生命周期守卫

**文件：** `frontend/src/views/Settings.vue`、`Settings.test.ts`；必要时共享局部异步协调函数；`PricingStatusBanner.vue` 同类卸载分支一并核对。

1. 新增 `late_rejected_settings_read_does_not_reset_saved_value`：初读成功→旧刷新挂起→保存 false/quit 成功→旧刷新 reject；保持保存值及 known 状态，不出现旧错误。对来源、自动同步、关闭动作分别覆盖。
2. 新增 `settings_mutations_do_not_notify_after_unmount`，覆盖 saveSource/setAutoSync/setCloseAction/syncPricing/rebuild 的成功和失败响应；卸载后通知 spy 不增加，局部状态不再写入。
3. 新增 `close_action_rapid_changes_cannot_reorder`：两次选择通过 deferred 逆序完成；采用单飞禁用或串行写入，保证 UI 与最后实际持久化值一致。仅在响应端忽略旧结果不能阻止后端旧写落盘。
4. 成功/catch 使用相同 current 判断；写操作 await 后检查 disposed；关闭动作增加函数级 busy 及 UI loading/disabled。局部已保存状态不能由旧失败降级为未知。
5. 运行 `pnpm --dir frontend test -- src/views/Settings.test.ts src/components/PricingStatusBanner.test.ts`。浏览器 fixture 增加“保存后旧读取拒绝”契约，不能仅复用旧读取 resolve 的场景。
6. 提交：`fix(设置): 阻止晚到错误和卸载后写响应污染界面`。

### AP05：横幅区分无主源与无任何价格

**文件：** `frontend/src/components/PricingStatusBanner.vue`、同名测试、`frontend/src/views/Settings.vue`、浏览器 fixtures/contracts；后端 DTO 已有 `has_any_pricing`，不新增重复推断。

1. 写 `external_only_pricing_does_not_claim_all_costs_unknown`、OR-only 对应用例，给出 needsSync=true/hasAnyPricing=true 及至少一项有效候选。
2. 四态表驱动：完全无价→当前无可用价格；只有外置/OR→主源待同步，已有价格仍用于估算；主源可用→首同步条收敛；状态读取失败→未知并可重试。
3. 用 DTO 选择文案，保留“立即同步”及部分同步失败重试；不将原始条目数当有效价格，也不承诺所有请求完整计价。同步设置页同类说明。
4. 运行 `pnpm --dir frontend test -- src/components/PricingStatusBanner.test.ts src/App.test.ts`；浏览器增加外置-only 与 OR-only 的费用+文案联合断言。
5. 提交：`fix(定价): 正确说明主源缺失时的可用价格状态`。

### AP06：Dashboard 来源检测错误可见、可重试、可更新

**文件：** `frontend/src/views/Dashboard.vue`、`Dashboard.test.ts`、浏览器失败 fixture。

1. 写 `source_status_failure_is_visible_and_retryable`：查询成功而 source_status 拒绝；不产生 unhandled rejection，错误含原因，点击重试可恢复来源列表。
2. 写 `manual_refresh_rechecks_source_status`：目录从 missing/empty 变 ready，刷新后旧通知消失；读取失败不能把旧状态标为最新。
3. 增加独立 loading/error/读取代次及卸载守卫；合并必要刷新，避免连点并发。不得用 source_status 错误清空成功的汇总；AP02 的设置错误要能经此路径显示。
4. 运行 `pnpm --dir frontend test -- src/views/Dashboard.test.ts`；真实浏览器覆盖 status 拒绝→恢复及目录变化，不以只覆盖 query_summary 拒绝代替。
5. 提交：`fix(汇总): 展示来源检测失败并支持刷新恢复`。

### AP07：已记忆的关闭动作同样处理失败，移出事件回调 IO

**文件：** `src-tauri/src/lib.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/window_state.rs`、必要的 App/关闭弹窗接线及测试。

1. 写 `remembered_minimize_failure_is_reported_and_retryable`：设置 minimize，注入 hide Err；关闭被拦截以防丢窗口，但必须向仍可见窗口展示原因及重试/取消；与未记忆的 close_resolve 共用结果处理。
2. 写 `close_handler_does_not_wait_for_settings_or_state_io`：用 barrier 暂停配置读取/状态写入，事件入口仍及时返回、后续结果单次处理；重复关闭不得并发执行/提前退出。窗口 API 仍在 Tauri 允许的线程执行。
3. 将磁盘读取/持久化交给后台任务或已有状态保存协调者；协调退出前的最终保存，不简单 fire-and-forget 后立即 exit。保留取消不写 close_action、托盘直接退出语义。
4. 运行 `cargo test --offline --manifest-path src-tauri/Cargo.toml close`、`cargo test --offline --manifest-path src-tauri/Cargo.toml window_state` 和 `pnpm --dir frontend test -- src/App.test.ts src/components/CloseConfirmDialog.test.ts`；隔离原生实例验证记忆动作及失败恢复。未具备注入环境时不能只勾单元测试就声称原生通过。
5. 提交：`fix(窗口): 统一关闭失败恢复并后台处理持久化`。

### AP08：补验收证据，保持发布后延边界

**文件：** `frontend/scripts/contracts/interaction-contracts.mjs`、`frontend/scripts/fixtures/ui-contracts.mjs`、`frontend/scripts/native-acceptance.mjs`、`scripts/prepare-native-acceptance.ps1`、原生 QA 和 D5 文档。

1. AP01–07 实现后扩充相应失败 fixture 和联合断言，重新执行完整浏览器矩阵。需要真实 Rust 配置/缓存的案例用隔离后端，不能用合成 IPC 证明后端正确。
2. 原生 query 过期轮：独占本次记录的验收 PID，空闲超过真实 TTL 后加载更多，存错误、重试后新会话和恢复分页证据。记录进程消失的事实；未查到终止者时不推断其他实例导致终止。
3. Windows 每监视器 125%/150% 与系统深色跟随首帧：在具备对应系统环境时取证；没有环境则保持待验。不得以 `--force-device-scale-factor` 或手动深色替代。
4. CSP 哨兵：增加仅验收构建可用的受控入口，证明普通页面脚本插入未授权内联/外源 script 被策略拒绝，正常 IPC/主题/图表仍可用；调试器直接求值成功/失败不作 CSP 证据。检查普通构建不携带该入口。
5. 已记录的 Naive UI data-URI 预热图违规保持明确计数和原因；优先查依赖可用修复或去掉死代码。没有功能影响的噪声不是本次 P1 阻断，不为了“零日志”放宽生产 CSP，也不能声称当前绝对零违规。
6. D5 安装/升级/卸载、自启注册、系统休眠及真机隐私时序继续在独立发布批次执行，逐项记录版本/结果/证据；本计划不把后延项打勾。若用户另行决定缩减验收范围，记录决策而不是伪造通过。

### AP09：统一计划索引、覆盖关系与完成定义

**文件：** `docs/plans/README.md`、本计划、被承接 active 计划及 QA、`d5-acceptance-checklist.md`。

1. 为第 2 节每份文档核对“历史/已替代/实现完成/仍待验”状态。旧 unchecked 任务若已由后续计划实现，链接具体承接项；仍有缺陷的项目链接 AP 编号，不把历史文本整段重写成当前结论。
2. 将可归档的实现计划与其验收责任明确关联后再归档；未验项必须有唯一活跃入口，不能通过移动文件丢失任务。
3. 更新 RC03/04、SF09、C1/D4 等相关“全部完成”声明及最新矩阵数量；只记录实际执行结果。原生/系统/发布验收分列。
4. 文档 UTF-8 回读、链接检查、`git diff --check`，完整门禁后独立提交。

## 6. 验证命令与执行顺序

顺序：**AP01 → AP02 → AP03 → AP04 → AP05 → AP06 → AP07 → AP08 → AP09**。共享代码顺序修改；不为本计划默认启动并行代理。

仓库根 PowerShell，cargo 不在 PATH 时用 `C:/Users/admin/.cargo/bin/cargo.exe`。联网依赖操作沿用本机代理 `http://127.0.0.1:7897`。

```powershell
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
```

独立终端启动本次生产前端预览（不启动原生应用）：

```powershell
pnpm --dir frontend exec vite preview --host 127.0.0.1 --port 1441 --strictPort
node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1441 --phase verify --output qa-artifacts/all-plans-final-recheck/after
```

需要验收 feature 的测试须在独立子进程先 bootstrap 临时根，再调用 command；普通构建和验收构建都要验证。不得通过修改 HOME/USERPROFILE 获得伪隔离。

## 7. 完成定义与执行账

- [x] AP01：默认来源参与保存校验，失败不写文件。`6a445d6`；库/壳双层回归（默认来源撞默认根、嵌套、反向、停用恢复、首次保存、显式目录等于本来源默认根）。
- [x] AP02：坏配置不启用默认来源，首次启动与关闭询问退路仍正确。`ed10ed9`；查询/状态/重建共用严格读取内核，关闭行为走单独命名的 Ask 退路（`close_decision_now`），采集路径不复用该退路。
- [x] AP03：重建与查询使用同一有效来源，停用/自定义/读取失败回归通过。`f538d35`；`rebuild_cache` 接收采集选项、清库前先校验来源，设置读取失败不触碰缓存（库 + 壳双层断言）。
- [x] AP04：晚到成功及失败均受保护，卸载后无局部通知，关闭动作不乱序。`a2a4d11`；关闭动作改为写入串行化（在途只记最后一次意图，失败回退到已落盘值），写操作落地前检查 `disposed`。
- [x] AP05：外置-only、OR-only、无价、主源可用及状态失败文案均符合 DTO。`a2a4d11`；组件单测 + 浏览器契约 `pricing_banner_distinguishes_dto_states`（19 条断言）。
- [x] AP06：来源检测错误有恢复动作，刷新与晚到响应行为正确。`c31bba2`；错误条可重试、失败标注“可能已过期”、手动刷新重查，且不清空已成功的汇总。
- [x] AP07：已记忆关闭动作失败可恢复，事件回调无阻塞文件 IO。`d68bbb2`；关窗回调只做内存更新与协调启动（单飞 + 后台 IO），退出前完成最终落盘，失败经 `close-action-failed` 复用关闭对话框重试。
- [x] AP08 自动化：新反例已纳入真实边界断言，完整矩阵通过。**60 场景（15 fixture × 2 主题 × 2 视口）/ 17 条必需契约 / 243 条契约断言（矩阵断言 304）、零失败**（Chromium 148.0.7778.96，提交 `d68bbb2`，产物 `qa-artifacts/all-plans-final-recheck/after/measurements.json`，gitignore）。新增契约：`source_save_rejected_error_is_row_scoped`、`close_action_write_failure_rolls_back_to_confirmed`、`late_failed_settings_read_does_not_downgrade_saved`、`source_status_failure_recovers_without_losing_summary`、`pricing_banner_distinguishes_dto_states`。需要真实 Rust 配置/缓存的案例由隔离 Rust 测试证明（`tests/rebuild_source_contract.rs` 与壳侧 settings/source/rebuild 内核测试），未用合成 IPC 冒充后端正确性。
- [ ] AP08 原生：**待验**。query 会话过期的原生轮（需 >600 s 真实空闲与独占 PID）、Windows 每监视器 125%/150% 缩放、系统深色下“跟随系统”首帧、CSP 脚本哨兵（缺仅验收构建可用的受控入口；未为此放宽生产 CSP）本轮均未取证，环境缺失不算通过。Naive UI data-URI 预热图违规保持 **2 条 `img-src`** 的明确计数与原因，不声称绝对零违规。
- [x] AP09：全计划状态与实现/验收证据一致，替代关系可追溯。本文件 §2 的“本次判定与承接”列为唯一权威覆盖关系索引；README 当前状态与总账、被承接 active 文档（RC03/RC04、SF09/UX06、C1/D4）已加承接指针；原生/系统/发布验收与代码修复分列。

**已知范围限制（浏览器层，不升级为产品缺陷）：** ①“已确认值被晚到失败读取降级”的强形态在真实浏览器只有“首读失败 → 重试挂起 → 保存成功 → 晚到 reject”这一条可构造路径（`refreshKey` 仅单测可推进，生产中切页会卸载组件），因此该契约覆盖的是同族反例，强形态由组件单测覆盖；②关闭动作写入失败的原因依赖 Naive message（约 3 s 自动消失），浏览器断言只验证“可重试成功”，错误常驻需要另加行内错误条（属新需求）；③所有失败注入都在合成 IPC 层，真实后端判定由 Rust 测试与原生验收覆盖。

**独立后延：** D5 发布验收未完成前不得标为“可发布”，不纳入“代码修复已完成”的表述。

| 日期/基线 | 工作 | 状态 |
| --- | --- | --- |
| 2026-10-08～09 / 5ec826c | 31 份文档覆盖关系、关键源码、完整门禁、浏览器矩阵及隔离反例复核 | 已完成复核 |
| 2026-10-09 / 5ec826c → d68bbb2 | AP01–AP07 逐项红→绿实现并独立提交；AP08 浏览器矩阵扩充重跑（60 场景 / 17 契约 / 243 断言，零失败）；AP09 状态归并与文档回写 | 代码与自动化已完成；AP08 原生/系统与 D5 待验，本计划保持 active |
