# Post-Implementation Audit Remediation Plan

> **状态（2026-10-06）：Task 1–5 全部完成**（红→绿，提交号见文末执行记录）；**Task 6 自动化部分完成**——tauri build 连续两次产出 4.83 MiB 安装包（5,065,571 / 5,065,415 字节，差异为 zip 时间戳），真机场景按用户决策后延至外部验收批次。计划保持 active 待真机验收。

**Goal:** 修复最近一轮实现审阅中仍存在的日期选择器风险、来源重叠验证缺口、旧索引测试假阳性、Settings 集成测试缺失和 active 文档不一致，并完成真实 D5 验收记录。

**Architecture:** 在现有代码结构上做小范围修正，不重新设计采集、定价或分页架构。日期选择器把“UI picker 毫秒值”和“统计时区日历字符串”分开处理；来源重叠使用统一的规范化键和可见 warning；测试直接验证行为而不是仅验证替代路径；文档只更新当前 active 状态，历史归档保留原始事实。

**Tech Stack:** Vue 3、TypeScript、Naive UI、Vitest、Rust 2024、SQLite、Tauri 2、Cargo test/clippy/rustfmt、Windows D5 验收。

---

## 不变量

1. 用户在日期选择器中点选的日历日期，确认后必须原样成为 `YYYY-MM-DD`，不受本机时区和统计时区的组合影响。
2. 目录存在或不存在、带 `..`、带大小写别名时，两个启用来源的等价目录都必须被识别为重叠。
3. 旧价格索引测试必须在签名有效、仅版本过期的条件下证明重建，不能依赖签名不匹配触发重建。
4. Settings 的同步、来源配置错误和全局横幅刷新必须有组件级行为测试。
5. 重叠来源、日期转换和索引迁移失败时，用户或测试输出必须能区分真实错误与降级行为。
6. D5 未执行的项目不能被记录为通过；自动化通过不能替代安装包真机验收。

## 执行顺序

Task 1、Task 2、Task 3、Task 4 可并行；Task 5 在代码和测试完成后执行；Task 6 最后执行。每个任务独立提交，禁止把文档修正和行为修复混进同一个代码提交。

### Task 1：修复日期选择器真实 round-trip

**Files:**
- Modify: `frontend/src/components/DateRangeSelect.vue:95-108`
- Modify: `frontend/src/lib/dates.ts`
- Modify: `frontend/src/components/DateRangeSelect.test.ts`

**Step 1: 写能证明当前缺陷的失败测试**

把 `NDatePicker` 替换为最小 stub，明确发出 `update:value`，覆盖：

1. 选择一个日期后，组件 setter 得到 picker 的毫秒值；
2. 本机为负偏移时区时，选择的日期不会变成前一天；
3. 统计时区为 UTC、上海、纽约时，picker round-trip 结果都保持同一 `YYYY-MM-DD`；
4. 快捷项仍按统计时区计算“今天”，不受本次 UI bridge 修复影响。

当前测试必须先失败，不能继续使用快捷按钮代替 picker setter。

**Step 2: 运行失败测试**

```powershell
pnpm --dir frontend test -- DateRangeSelect
```

预期：当前 `tzDate(ms, props.tz)` 在 UTC 零点和负偏移统计时区组合下产生前一天。

**Step 3: 实现最小修复**

把 picker 毫秒值转换为“UI 所使用的日历日期”，不要把 picker 的显示值重新解释为统计时区日期。统计时区只继续用于快捷项和传给后端的日期字符串。

如果 Naive UI 的 date picker 使用本机日历语义，则用本机日历转换函数完成 getter/setter；如果使用 UTC 锚定语义，则使用明确的 UTC 日历转换函数。不要让同一个 `tzDate` 同时承担两种语义。

**Step 4: 运行通过测试**

```powershell
pnpm --dir frontend test -- DateRangeSelect
pnpm --dir frontend typecheck
```

**Step 5: 提交**

```powershell
git add frontend/src/components/DateRangeSelect.vue frontend/src/lib/dates.ts frontend/src/components/DateRangeSelect.test.ts
git commit -m "fix(gui): 修复日期选择器跨时区回传"
```

### Task 2：加强来源重叠规范化、告警和回归测试

**Files:**
- Modify: `src/report.rs:387-455`
- Modify: `src/settings.rs:88-116`
- Modify: `src-tauri/src/commands.rs:169-210`
- Modify: `tests/source_overlap.rs`
- Add or modify: report/cache 相关测试

**Step 1: 写失败测试**

新增并明确验证以下场景：

1. 不存在目录的 `a\b\..\shared` 与 `a\shared` 被配置校验识别为同一路径；
2. Windows 大小写别名被识别为同一路径；
3. 使用 mock source 让同一文件对两个 agent 都产出事件，旧实现会产生两份，修复后只能保留一份；
4. 直接查询 SQLite，确认缓存的 path、agent 和 events 归属没有被后扫描 agent 覆盖；
5. 旧配置触发采集层去重时，报告 warnings 或前端可见状态中包含“来源目录重叠”诊断。

所有测试都必须注入临时 `cache_dir` 和 `pricing_index`。

**Step 2: 运行失败测试**

```powershell
cargo test source_overlap -- --nocapture
```

预期：当前 `normalize_path` fallback 不处理 `..`，现有测试也无法证明两个 adapter 同时扫描同一文件时不会重复。

**Step 3: 实现最小修复**

- fallback 规范化必须折叠 `.`、`..` 和空组件；
- Windows 路径比较键使用不区分大小写的规范化表示，原始路径仍用于文件 IO；
- 采集层去重 warning 加入 `Collected.warnings`，使 Dashboard/明细的 warnings 能看到，而不是只写日志；
- 保留配置保存时拒绝重叠的行为，采集层继续防御旧配置和符号链接；
- 测试使用 mock source 或等价 fixture，不能依赖某一个 adapter 恰好无法解析另一种日志格式。

**Step 4: 运行通过测试**

```powershell
cargo test source_overlap -- --nocapture
cargo test cache -- --nocapture
cargo test source_config --manifest-path src-tauri/Cargo.toml -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/report.rs src/settings.rs src-tauri/src/commands.rs tests/source_overlap.rs
git commit -m "fix(采集): 完善来源重叠规范化和诊断"
```

### Task 3：修正旧索引迁移测试的假阳性

**Files:**
- Modify: `src/pricing.rs:1038-1070`
- Add or modify: pricing index migration tests

**Step 1: 写失败测试**

构造 v2 索引，其中：

- `sig` 与当前 `load_cached(None, None, None, index_path)` 的来源签名完全一致；
- `v = 2`；
- entries 中包含一个旧内置条目。

在测试中先证明若版本检查被移除，旧条目会被命中；再断言当前实现因版本不匹配而重建，旧条目不存在。

**Step 2: 运行测试确认当前测试不足**

```powershell
cargo test test_legacy_index_v2_with_builtin_entries_is_invalidated -- --nocapture
```

同时检查测试 fixture，确保不再使用必然导致签名不匹配的 `stale-sig`。

**Step 3: 实现测试修正**

复用生产代码使用的签名计算方式，确保测试只改变索引版本，不改变来源签名。保持 `CACHE_TEST_LOCK`，避免全局 `PRICE_CACHE` 污染并行测试。

**Step 4: 运行通过测试**

```powershell
cargo test pricing_index -- --nocapture
cargo test pricing -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/pricing.rs
git commit -m "test(定价): 让旧索引迁移测试验证真实版本失效"
```

### Task 4：补齐 Settings 组件级集成测试

**Files:**
- Create: `frontend/src/views/Settings.test.ts`
- Modify: `frontend/src/views/Settings.vue`（仅为可测试性或错误展示所需的最小修改）
- Modify: `frontend/src/components/PricingStatusBanner.test.ts`（必要时抽取公共 mock）

**Step 1: 写失败测试**

覆盖：

1. `sync_pricing_openrouter` 部分失败时，Settings 仍重新调用 `pricing_entries`；
2. 同步结束后派发 `pricing-status-changed`；
3. `source_config_set` 返回目录重叠错误时，错误可见且当前输入仍保留；
4. 重新加载来源列表不会清除用户尚未提交的另一行草稿。

**Step 2: 运行失败测试**

```powershell
pnpm --dir frontend test -- Settings
```

预期：当前没有 Settings.test.ts，无法证明这些行为。

**Step 3: 实现最小测试和修正**

使用 mock IPC 和最小 Naive UI stub，只验证 Settings 的行为与事件，不复制组件库内部实现。若发现 `pricing_entries` 失败会造成未处理 Promise，补充用户可见错误并确保 `syncing` 必然复位。

**Step 4: 运行通过测试**

```powershell
pnpm --dir frontend test -- Settings PricingStatusBanner App
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
```

**Step 5: 提交**

```powershell
git add frontend/src/views/Settings.test.ts frontend/src/views/Settings.vue frontend/src/components/PricingStatusBanner.test.ts
git commit -m "test(gui): 补齐设置页同步和来源配置测试"
```

### Task 5：清理 active 计划和当前代码注释的不一致

**Files:**
- Modify: `src/pricing.rs:141`（四层索引注释）
- Modify: `docs/plans/active/2026-10-05-product-review-and-roadmap.md`（给 F01–F10、C/D 阶段补 resolved/superseded 状态）
- Modify: `docs/plans/active/2026-10-05-release-blockers-remediation.md`（保留 superseded 说明，避免旧 Task 4 看起来仍待执行）
- Modify: `docs/plans/active/2026-10-05-pricing-source-policy.md`
- Modify: `docs/plans/d5-acceptance-checklist.md`
- Optionally modify: `docs/plans/README.md`（明确历史 M5/M9 为旧策略，当前策略为三层）

**Step 1: 建立文档核对清单**

搜索 active 计划、当前代码注释和 D5 文档中的“四层”“内置兜底”“load_cached 未接入”等旧状态，逐项标注为当前事实、历史记录或已 superseded。

**Step 2: 更新文档**

当前策略统一写为：外置 > models.dev > OpenRouter；无来源返回 unknown；离线只依赖本地快照。历史归档计划可以保留当时的设计，但 active 计划不得继续把已修复问题列为未处理。

**Step 3: 运行文档一致性检查**

```powershell
rg -n "四层|内置兜底|load_cached.*未接入|来源标签.*内置" CLAUDE.md src docs/plans/active frontend
```

预期：只剩明确标注为历史、superseded 或测试旧索引迁移场景的文本。

**Step 4: 提交**

```powershell
git add src/pricing.rs docs/plans/active docs/plans/d5-acceptance-checklist.md docs/plans/README.md
git commit -m "docs(计划): 清理已完成任务的旧状态"
```

### Task 6：完成 D5 真机验收和证据登记

**Files:**
- Modify: `docs/plans/d5-acceptance-checklist.md`
- Modify: `docs/plans/active/2026-10-06-post-implementation-audit-remediation.md`
- Modify: 相关 active plan 状态

**Step 1: 重新构建并记录产物**

```powershell
.\frontend\node_modules\.bin\tauri build
```

记录版本、安装包路径、文件大小和第二次构建结果；不能只引用旧的 4.82 MiB 或未附证据的 4.83 MiB 文字。

**Step 2: 完成真机场景**

至少执行并记录：

- 全新安装、升级、卸载后的用户数据保留；
- 双开互斥、关窗缩托盘、托盘退出、自启、窗口记忆；
- 125%/150% DPI、浅色/深色主题、休眠恢复；
- 来源目录配置、停用/启用、目录重叠错误；
- 明细分页、日期选择器、断网启动、无 models.dev 快照首次启动；
- models.dev 成功而 OpenRouter 失败时的横幅和设置页状态；
- 源目录只读、测试不修改真实缓存。

**Step 3: 填写证据**

每项填写产物版本、日期、结果（通过/失败/阻塞）、备注和验收人。未执行项保持空白或标记阻塞，不得写成自动化通过。

**Step 4: 更新计划状态**

只有 Task 1–5 的回归测试通过且 D5 第 2–5 节有真实证据后，才把相关 active plan 移入 `docs/plans/archive/implemented/`。否则保持 active，并明确剩余阻塞。

## 完成定义

- [ ] 日期 picker setter 在负偏移时区 round-trip 正确，并有真实组件测试。
- [ ] 来源重叠测试能在两个 source 都产出事件时抓住重复统计，`..`/大小写/不存在路径均有覆盖。
- [ ] 重叠 warning 能到达报告或 UI；缓存归属有直接断言。
- [ ] 旧索引迁移测试使用有效签名，只验证版本失效。
- [ ] `Settings.test.ts` 覆盖同步、来源错误和全局状态事件。
- [ ] active 计划与当前三层定价实现一致，旧状态已明确标记历史或 superseded。
- [ ] D5 真机验收逐项有证据，安装包构建可重复性有记录。
- [x] Rust、Tauri、前端完整门禁通过。

## 执行记录（2026-10-06，分支 docs/product-review-plan）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| Task 1 picker round-trip | setter 由 tzDate(ms, 统计时区) 改为 msToUtcCalendar（UTC 零点锚逆运算）——负偏移时区下点选 10-05 不再变 10-04；统计时区仅用于快捷项与后端传参。红→绿（纽约/上海 round-trip） | `418ccde` |
| Task 2 重叠规范化 | normalize_path fallback 折叠 `..`；重叠键大小写不敏感；重叠诊断进 Collected.warnings；DedupSource 透传发现诊断；dedup_source_overlap 独立函数 + 集成测试入口；mock 双 agent 同文件测试 + SQLite 归属直查 + validate 补 `..`/大小写/尾斜杠 | `831ad5c` |
| Task 3 旧索引假阳性 | fixture sig 改用生产 source_sig——先证 from_index 会命中旧内置条目（fixture 有效），再证 load_cached 仅因版本失效重建 | `f93cda6` |
| Task 4 Settings 测试 | Settings.test.ts 新建：部分失败刷新 pricing_entries、派发 pricing-status-changed、重叠错误可见输入保留、保存后重载来源状态 | `10b59aa` |
| Task 5 文档一致性 | pricing.rs:141 索引注释改三层；product-review 问题清单标历史 + F10 行内补注；README M5/M9 标注旧策略被取代；一致性扫描（四层/内置兜底/load_cached 未接入）仅剩历史与 superseded 文本 | `9f6151c` |
| Task 6 D5 产物 | tauri build 连续两次 4.83 MiB（可重复性确认）；真机场景后延至外部验收批次（用户决策） | `本次提交` |

完成定义勾选：picker round-trip ✅；重叠覆盖（.. /大小写/双 agent mock/SQLite 归属）✅；warning 到达 report ✅；旧索引用有效签名验证版本失效 ✅；Settings.test.ts ✅；active 文档一致 ✅；门禁全绿 ✅；D5 真机证据后延（todo 登记）。
