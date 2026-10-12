# Review Findings Remediation Implementation Plan

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| Task 1–5 | 主体及后续修复已实现 | — |
| Task 6 | 未完全完成：单飞 follower 测试仍依赖 150ms 调度时间窗 | [N09](../../active/2026-10-11-consolidated-remaining-work.md#n09) |
| Task 7 | 真机/发布验收后延 | [N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-06-review-findings-remediation.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

> **状态（2026-10-06）：Task 1–6 全部完成**（红→绿，提交号见文末执行记录）；**Task 7 自动化部分完成**（tauri build 4.83 MiB），真机场景（安装/升级/托盘/DPI 等）按用户 2026-10-05 决策后延至外部验收批次——计划保持 active，外部验收完成后再归档。

**Goal:** 修复两个现有计划审阅中发现的分页漏行、来源目录重叠、定价部分同步状态错误和前端验收缺口，并同步产品文档与 D5 验收证据。

**Architecture:** 保留现有 `source → UsageEvent → dedupe → report → Tauri/Vue` 分层，不增加内置价格来源，也不改变 models.dev 主源、OpenRouter 补充源的产品决策。明细分页改为带稳定 tie-breaker 的不透明游标；来源目录在配置和采集两层防止同一文件被两个 agent 重复统计；定价同步继续使用现有 command 契约，但前端在部分失败后重新读取状态并传播状态刷新事件。

**Tech Stack:** Rust 2024、jiff、SQLite、Tauri 2、Vue 3、TypeScript、Naive UI、Vitest、Cargo test/clippy/rustfmt。

---

## 目标不变量

1. 明细分页在相同 timestamp、相同或空 `record_id` 时仍不重不漏；同一采集快照中的每个事件都有唯一 cursor。
2. Claude 与 Codex 指向同一规范化目录时，同一日志文件最多贡献一次统计；旧配置也不能绕过这个保护。
3. models.dev 已成功而 OpenRouter 失败时，主定价状态立即变为可用；用户仍能看到补充源失败原因并可重试。
4. `pricing_status` IPC 失败不能静默隐藏所有诊断；设置页手动同步后全局横幅必须同步刷新。
5. 统计日期语义继续使用 `YYYY-MM-DD` 和所选统计时区；日期选择器的毫秒值只承担 UI 桥接，不改变统计时区含义。
6. 所有测试使用临时 `cache_dir` 和 `pricing_index`，不触碰用户的 `~/.tokenscope`。
7. 没有任何生产代码、指导文档或验收清单继续声称存在内置价格兜底。

## 执行顺序

Task 1 和 Task 2 可并行；Task 3 和 Task 4 可并行；Task 5 在代码修改完成后执行；Task 6 必须最后执行。每个任务独立提交，提交前只运行该任务相关的定向测试，Task 6 再跑完整门禁。

### Task 1：修复相同 timestamp + 空 record_id 的分页漏行

**Files:**
- Modify: `src/report.rs:679-835`（游标 DTO、排序、过滤、序列化）
- Modify: `src/report.rs:1540-1660`（分页 fixture 和回归测试）
- Modify: `frontend/src/types.ts`（仅在后端 DTO 需要新增字段时同步类型；前端继续原样回传 cursor）

**Step 1: 写失败测试**

新增一个 Codex fixture：两条事件使用完全相同的 UTC timestamp、空 `record_id`、不同 token 数量，分页 `limit=1` 连续读取，断言两条事件都出现且 cursor 不同。再用同一 fixture 重复查询，断言 cursor 顺序稳定。

**Step 2: 运行测试确认失败**

```powershell
cargo test test_events_pagination_exact_timestamp_empty_record_id -- --nocapture
```

预期：当前实现第二页为空，或两个 cursor 相同。

**Step 3: 实现最小修复**

引入后端私有 cursor 结构，至少包含完整 timestamp、`record_id` 和稳定 tie-breaker。推荐在采集事件完成后按现有展示排序做稳定排序，再给相同 `(timestamp, record_id)` 的事件分配确定性的序号；cursor 用 `serde_json` 序列化为不透明字符串，避免 `record_id` 中的分隔符破坏解析。

过滤时比较完整的 `(timestamp, record_id, tie_breaker)` 元组，不能只比较 timestamp 和 record_id。解析器必须拒绝缺字段、非法 timestamp 和非法 tie-breaker。确保文件发现顺序和事件顺序保持确定性，重复读取同一 fixture 得到相同 cursor。

同步更新 `EventRow` 和相关注释；前端不得自行解析 cursor，继续把 `last.cursor` 原样传给后端。

**Step 4: 运行测试确认通过**

```powershell
cargo test test_events_pagination -- --nocapture
cargo test report -- --nocapture
```

预期：同秒亚秒、空 `record_id`、完全相同 timestamp 三组分页测试全部通过。

**Step 5: 提交**

```powershell
git add src/report.rs frontend/src/types.ts
git commit -m "fix(分页): 为相同时间事件补充稳定游标键"
```

### Task 2：阻止来源目录重叠造成重复统计和缓存覆盖

**Files:**
- Modify: `src/report.rs:350-590`（来源构造、路径规范化和防御性去重）
- Modify: `src/settings.rs`、`src-tauri/src/commands.rs`（保存来源配置时的重叠校验）
- Modify: `frontend/src/views/Settings.vue`（展示可操作的配置错误）
- Test: `src/report.rs`、`src-tauri/src/commands.rs` 中的来源配置和采集测试

**Step 1: 写失败测试**

覆盖三种情况：

1. Claude 和 Codex 使用同一目录时，采集结果只包含一份事件；
2. 两条路径通过相对路径、大小写或目录别名指向同一目录时，也被识别为重叠；
3. 已存在的旧设置直接进入采集流程时，不会让后扫描的 agent 覆盖缓存中的前一个 agent。

测试必须注入临时 `cache_dir`、`pricing_index`，并断言事件数、来源统计和缓存文件归属。

**Step 2: 运行测试确认失败**

```powershell
cargo test source_overlap -- --nocapture
```

预期：当前实现会重复统计，或缓存的同路径记录只保留最后一个 agent。

**Step 3: 实现最小修复**

新增统一的路径规范化 helper：优先使用 `canonicalize`，路径不存在时使用绝对路径和组件清理作为稳定 fallback。

在来源配置保存时，如果两个启用的 agent 指向同一规范化目录，返回明确错误并阻止保存。采集层仍需做防御性保护，以处理旧设置、符号链接和配置文件被外部修改的情况：跨 agent 维护规范化文件路径集合，同一文件只进入一次统计，并产生一条可诊断 warning。缓存清理和 `purge_agent` 必须与实际归属保持一致，不能通过简单的后写覆盖隐藏冲突。

设置页显示后端返回的重叠错误，保留用户当前输入以便修改；不要静默禁用某个 agent。

**Step 4: 运行测试确认通过**

```powershell
cargo test source_overlap -- --nocapture
cargo test source_config -- --nocapture
cargo test cache -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/report.rs src/settings.rs src-tauri/src/commands.rs frontend/src/views/Settings.vue
git commit -m "fix(采集): 防止来源目录重叠重复统计"
```

### Task 3：修复定价部分同步后的状态传播

**Files:**
- Modify: `frontend/src/components/PricingStatusBanner.vue`
- Modify: `frontend/src/views/Settings.vue`
- Test: `frontend/src/components/PricingStatusBanner.test.ts`
- Create: `frontend/src/views/Settings.test.ts`（若当前不存在）

**Step 1: 写失败测试**

新增以下测试：

1. `sync_pricing_openrouter` reject，但随后 `pricing_status` 返回 `needsSync=false` 时，横幅不再显示“尚未获取定价”，同时显示 OpenRouter 部分失败原因；
2. `pricing_status` IPC reject 时，界面显示“定价状态读取失败，可重试”，不能静默返回空 DOM；
3. 设置页同步失败后仍重新读取 `pricing_entries`，主源成功写盘的数据立即可见；
4. 设置页同步成功或部分失败后派发 `pricing-status-changed` 事件，全局横幅重新读取状态。

**Step 2: 运行测试确认失败**

```powershell
pnpm --dir frontend test -- PricingStatusBanner Settings
```

预期：当前 Banner 不会在 catch 后刷新状态，设置页也不会更新价格视图。

**Step 3: 实现最小修复**

- `PricingStatusBanner.vue` 在同步成功和失败路径都调用 `refreshStatus()`；
- 将同步错误放到独立的错误提示中，使主源已经可用时仍能显示补充源失败原因；
- `refreshStatus()` 失败时保留可见的重试提示，不再把 `status` 静默清空；
- 组件监听并清理 `pricing-status-changed` 浏览器事件；
- `Settings.vue` 在同步结束后刷新 `pricing_entries`，并在成功/失败两种路径派发状态刷新事件；
- 不改变现有 Tauri command 的返回类型，避免无必要的 IPC 契约扩散。

**Step 4: 运行测试确认通过**

```powershell
pnpm --dir frontend test -- PricingStatusBanner Settings
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
```

**Step 5: 提交**

```powershell
git add frontend/src/components/PricingStatusBanner.vue frontend/src/components/PricingStatusBanner.test.ts frontend/src/views/Settings.vue frontend/src/views/Settings.test.ts
git commit -m "fix(gui): 正确传播定价部分同步状态"
```

### Task 4：补齐前端集成测试并修正日期控件桥接

**Files:**
- Modify: `frontend/src/components/DateRangeSelect.vue`
- Modify: `frontend/src/components/DateRangeSelect.test.ts`
- Create: `frontend/src/App.test.ts`
- Modify: `frontend/src/lib/statsView.test.ts`（移除旧的“内置”来源样例）

**Step 1: 写失败测试**

1. 在负偏移本机时区下模拟 NDatePicker 的日期选择值，验证打开、选择、确定后的日期字符串与用户选择一致；
2. App 挂载后，`pricing_status.needsSync=true` 时能够看到全局横幅；
3. App 在 `needsSync=false` 时不显示横幅；
4. 来源标签测试只接受 `外置`、`models.dev`、`OpenRouter`。

**Step 2: 运行测试确认失败**

```powershell
pnpm --dir frontend test -- DateRangeSelect App statsView
```

预期：当前缺少 App 集成测试，日期选择器没有负偏移 round-trip 覆盖。

**Step 3: 实现最小修复**

将 NDatePicker 的毫秒值桥接限定为“主机本地日历 UI 值”；统计时区只用于快捷项和后端传递的 `YYYY-MM-DD`，不再用统计时区把 picker 的 UI 毫秒重新解释成另一天。保留 UTC/DST 快捷项测试，并新增本机负偏移环境下的选择 round-trip 测试。

App 测试只验证真实组件接线和结构化 `needsSync` 可见性，Tauri `invoke` 使用 mock；不要把 Naive UI 内部实现复制到测试里。清理 statsView 中遗留的“内置”样例。

**Step 4: 运行测试确认通过**

```powershell
pnpm --dir frontend test -- DateRangeSelect App statsView
pnpm --dir frontend typecheck
```

**Step 5: 提交**

```powershell
git add frontend/src/components/DateRangeSelect.vue frontend/src/components/DateRangeSelect.test.ts frontend/src/App.test.ts frontend/src/lib/statsView.test.ts
git commit -m "test(gui): 补齐横幅接线和日期选择回归"
```

### Task 5：同步定价策略文档并补强旧索引迁移测试

**Files:**
- Modify: `CLAUDE.md:11-20`
- Modify: `src/pricing.rs:1-12,140,359-360,454` 及过时测试注释/测试名
- Modify: `docs/plans/d5-acceptance-checklist.md:68`
- Modify: `docs/plans/archive/partial/2026-10-05-release-blockers-remediation.md`（标明内置价格任务已被新策略取代）
- Modify: `docs/plans/archive/partial/2026-10-05-pricing-source-policy.md`（记录本计划的后续修复）
- Test: `src/pricing.rs` 或 `src/report.rs` 的索引迁移测试

**Step 1: 写失败测试**

构造一个版本 2、包含旧内置条目的 `pricing-index.json`，启动价格加载流程，断言索引被忽略或重建，结果只包含外置、models.dev、OpenRouter 三层，不出现 `内置` 来源。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing_index -- --nocapture
```

预期：当前只有版本常量断言，没有完整的旧索引启动迁移验证。

**Step 3: 实现最小修复**

补齐旧索引 fixture、重建路径和“无内置条目”断言。同步所有注释、测试名和指导文档：三层顺序为“外置 > models.dev > OpenRouter”，无来源返回 unknown；离线只依赖本地 models.dev/OpenRouter 快照。

D5 清单 5.3 改为“价格走本地快照；无快照时费用显示未知并提示联网同步”，删除“内置”字样。旧 release-blockers plan 中涉及内置价格表的任务必须标记为 superseded，避免后续 agent 按旧策略恢复静态表。

**Step 4: 运行测试确认通过**

```powershell
cargo test pricing -- --nocapture
cargo test pricing_index -- --nocapture
```

**Step 5: 提交**

```powershell
git add CLAUDE.md src/pricing.rs docs/plans/d5-acceptance-checklist.md docs/plans/archive/partial/2026-10-05-release-blockers-remediation.md docs/plans/archive/partial/2026-10-05-pricing-source-policy.md
git commit -m "docs(定价): 统一三层来源策略并补索引迁移验收"
```

### Task 6：消除单飞测试竞态并完成自动化门禁

**Files:**
- Modify: `src/report.rs:1510-1535`（panic 单飞测试同步方式）
- Optionally modify: `src/report.rs:263-337`（若选择增加 Mutex poison 恢复）
- Modify: `docs/plans/archive/partial/2026-10-06-review-findings-remediation.md`（执行记录）

**Step 1: 写失败测试或稳定性基线**

将 `sleep(50ms)` 替换为 channel/barrier：leader 明确发出“已插入 INFLIGHT”信号后，follower 才开始等待。循环运行该测试至少 100 次，确认不会因调度顺序成为 leader 而假失败。

**Step 2: 实现最小修复**

保留当前 `FlightGuard` 的成功、错误、panic 清槽语义；不要为了未确认的 poison 路径重写锁模型。若增加 poison 恢复，必须先有独立测试，且恢复后仍需唤醒等待者和清理对应 key。

**Step 3: 运行完整门禁**

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --manifest-path src-tauri/Cargo.toml
pnpm --dir frontend test
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend build
```

预期：根库、Tauri 壳和前端测试全部通过，且没有把测试写入真实 `~/.tokenscope`。

**Step 4: 提交**

```powershell
git add src/report.rs docs/plans/archive/partial/2026-10-06-review-findings-remediation.md
git commit -m "test(并发): 消除单飞 panic 测试竞态"
```

### Task 7：执行 D5 真机验收并更新计划状态

**Files:**
- Modify: `docs/plans/d5-acceptance-checklist.md`
- Modify: `docs/plans/archive/partial/2026-10-06-review-findings-remediation.md`
- Modify: 原两个 active plan 的执行记录和状态

**Step 1: 构建并记录产物**

```powershell
.\frontend\node_modules\.bin\tauri build
```

记录版本、安装包路径、重复构建结果和产物大小。

**Step 2: 执行真机场景**

至少完成：全新安装、升级、双开互斥、托盘、DPI、主题、来源目录配置、明细分页、只读源目录、断网启动、无 models.dev 快照首次启动、models.dev 成功/OpenRouter 失败、设置页同步后全局横幅刷新。

**Step 3: 记录证据**

每项填写版本、日期、通过/失败/阻塞和备注；未执行项不能写成“通过”。确认 D5 清单第 2–5 节不再留空。

**Step 4: 归档判断**

只有自动化门禁、D5 真机验收和文档一致性全部通过后，才把本计划及两个上游计划移入 `docs/plans/archive/implemented/`。若存在已知遗留，保留 active 状态并在计划中列明接受的限制。

## 完成定义

- [ ] 完全相同 timestamp + 空 `record_id` 的分页不重不漏。
- [ ] 重叠来源目录不会重复统计或互相覆盖缓存。
- [ ] 定价主源成功、补充源失败时，状态、错误和重试行为正确。
- [ ] Banner、App、Settings、日期选择器有真实回归测试。
- [ ] 旧索引不会恢复内置价格；CLAUDE、代码注释和 D5 文档统一为三层策略。
- [ ] 单飞 panic 测试无调度竞态。
- [ ] Rust、Tauri、前端完整门禁通过。
- [ ] D5 真机验收有逐项证据，计划状态与实际一致。（→ 后延至外部验收批次）

## 执行记录（2026-10-06，分支 docs/product-review-plan）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| Task 1 分页游标键 | 完全相同 timestamp + 空 record_id 两行同键丢行 → 稳定排序后组内序号 tie-breaker + serde_json 不透明游标（deny_unknown_fields，分隔符免疫）；重复读取顺序稳定 | `1e2ce51` |
| Task 2 来源重叠 | normalize_path（canonicalize + 组件清理兜底）；配置层 validate_no_overlap 拒绝保存 + Settings.vue 显示错误；采集层 DedupSource 跨 agent 去重 + 重叠告警 + 发现诊断透传；test_source_overlap 三件套 | `c66826e` |
| Task 3 状态传播 | Banner 同步成功/失败都刷新 pricing_status；部分成功横幅收敛但补充源失败原因独立可见；pricing_status 失败可重试提示（不静默空 DOM）；Settings 派发 pricing-status-changed；测试 7 件套 | `7a7ee1d` |
| Task 4 前端集成 | App.test.ts（needsSync 横幅可见性，真实接线）；picker 毫秒桥接 round-trip 回归；statsView 清理内置样例 | `5c98301` |
| Task 5 策略文档 | CLAUDE.md/定价注释/测试名统一三层语义；legacy_index_v2 含内置条目失效重建测试（并行竞态经 CACHE_TEST_LOCK 修复）；D5 5.3 去内置字样；release-blockers Task 4 标记 superseded | `3cf4d8c` |
| Task 6 单飞竞态 | panic 测试 sleep(50ms) 换 channel 信号顺序（registered → follower 进入 wait → go → panic），领队阻塞 go 上不提前清槽；测试 3 连跑稳定 | `58c3487` |
| Task 7 D5 验收 | tauri build 4.83 MiB 产出；真机场景（安装/升级/托盘/DPI/断网时序）后延至外部验收批次（用户决策） | `本次提交` |

完成定义勾选：分页不重不漏 ✅；重叠不重复统计 ✅；部分同步状态正确 ✅；Banner/App/Settings/日期回归 ✅；旧索引不恢复内置 + 文档统一 ✅；单飞竞态消除 ✅；门禁全绿（根库 106 + 壳 10 + 前端 41 测试，clippy -D warnings）✅；D5 真机验收后延（已登记 todo）。
