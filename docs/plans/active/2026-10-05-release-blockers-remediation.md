# 发布阻断问题修复实施计划

> **状态（2026-10-05）：Task 1–8 全部完成**（每任务独立提交，红→绿记录见文末执行记录）；**Task 9 自动化验收通过**（完整门禁绿、真实数据密闭哈希一致、perf 工具冷 6.2s/热 0.15s、tauri build 产出 4.82 MiB）。**D5 真机验收（安装/升级/托盘/DPI 等）按用户决策后延**——计划保持 active，待外部验收批次完成后归档。

**Goal:** 修复会话 `3288887b-f153-4f91-951c-3d24a909d7ac` 审查中确认的 6 个 P1 问题和影响发布可靠性的 P2 问题，使 TokenScope 达到可重复验收、失败可恢复、费用可解释的发布候选状态。

**Architecture:** 保留现有 `source → UsageEvent → cache/dedupe → aggregate → report → Tauri/Vue` 分层。Rust 侧优先修复单飞生命周期、价格快照兼容、价格完整性和分页游标；前端侧把日期筛选改为明确的统计时区日历值。同步、设置和快照继续只写 `~/.tokenscope`，所有失败路径保持旧数据可用。

**Tech Stack:** Rust 2024、Cargo、SQLite、Serde、Jiff、Tauri 2、Vue 3、TypeScript、Vitest、PowerShell。

---

## 目标与非目标

### 目标

- 同参并发采集只运行一次；成功、普通错误和 panic 都能唤醒等待者并释放单飞槽。
- 价格快照升级不丢失已有的非零缓存价格；未知分项不被静默当成免费。
- 内置价格表的四列语义有独立测试，不再让错误的测试期望掩盖表格错误。
- 明细分页在同秒、亚秒和空 `record_id` 情况下不重不漏。
- 只有实际产生 token 的未知价格分项才影响完整性标记。
- 日期筛选始终按用户选择的统计时区解释自然日。
- 设置、同步、外置价格和快照失败时保留旧配置/旧快照，并显示可诊断状态。
- 补齐审查记录中计划声称存在但实际缺失的关键回归测试和文档。

### 非目标

- 不增加新的 Agent 适配器。
- 不恢复 CLI、不替换 Tauri、不迁移 DuckDB、不重写前端架构。
- 不改变已经确认的 Claude Code/Codex token 桶、去重和排除口径，除非新增回归测试证明现有实现违反不变量。
- 不在测试中读取或修改真实 `~/.tokenscope`；所有测试必须注入临时 `cache_dir` 和 `pricing_index`。

## 必须保持的不变量

1. `summary()` 与 `list_events()` 共享同一采集快照；筛选只发生在快照之后。
2. 单飞槽属于具体 `(key, cell)`；旧 leader 不能清理已经被新任务替换的槽位。
3. 采集失败不产生成功快照、不写成功缓存；下一次相同请求必须可以重新执行。
4. `Some(0.0)` 表示明确免费；`None` 表示未知；未知价格不能按 0 计费。
5. `CostEstimate.complete == false` 仅在对应分项 token 数大于 0 且价格未知时成立。
6. 明细分页游标必须包含完整排序键，不能只使用展示用的秒级时间。
7. 日期范围是解析时区下的自然日闭区间，DST 和本机时区差异不能改变日期语义。
8. 设置、价格快照、视图快照和索引写入失败时，旧文件保持可读；损坏设置默认离线且不得覆盖用户已有来源配置。
9. 所有新增测试均使用临时 `cache_dir`、`pricing_index`、价格文件和 fixture。

## 工作分派与依赖

为减少冲突，按文件所有权分派；同一文件不要由多个 agent 同时修改。

| 工作流 | 负责文件 | 主要任务 | 依赖 |
| --- | --- | --- | --- |
| A：采集与分页 | `src/report.rs`、`tests/report*` | 单飞 RAII、错误重试、完整游标 | 无 |
| B：价格语义 | `src/modelsdev.rs`、`src/pricing.rs`、`src/openrouter.rs`、相关 Rust 测试 | 快照兼容、内置价格、完整性、OpenRouter 输入 | 无 |
| C：日期与前端 | `frontend/src/components/DateRangeSelect.vue`、`frontend/src/views/Dashboard.vue`、Vitest | 统计时区日期值、分页 cursor 消费 | A 的 API 设计先确定 |
| D：设置与同步 | `src/settings.rs`、`src-tauri/src/commands.rs`、`src/fsutil.rs`、`src-tauri/src/lib.rs` | 损坏配置、原子写、同步互斥 | 无 |
| E：验收与文档 | `docs/stats-semantics.md`、`docs/plans/`、CI/钩子 | 口径补文档、补缺失测试、D5 验收 | A–D 合并后 |

合并顺序：A/B/D 可并行；C 依赖 A 最终确定的明细行 cursor 字段；E 在全部修复合并后执行。

---

## Task 1：单飞采集改为 RAII 生命周期管理（工作流 A）

**Files:**

- Modify: `src/report.rs` 的 `INFLIGHT`、`FlightCell`、`collect_flighted` 附近实现
- Test: `src/report.rs` 单元测试区；必要时新增 `tests/report_singleflight.rs`

### Step 1：先写失败测试

新增以下测试名，测试必须使用临时目录并注入 `cache_dir`、`pricing_index`：

- `test_failed_flight_is_retryable`
- `test_failed_flight_wakes_all_waiters`
- `test_panicked_flight_wakes_waiter_and_clears_slot`
- 保留并回归 `test_parallel_queries_single_collection`

由于正常 fixture 很难自然触发 `collect_inner` 错误，先抽出一个仅负责执行 leader 工作的内部 helper，使测试可以注入 `|| -> Result<CollectionSnapshot>` 的失败或 panic 闭包；生产路径仍调用真实 `collect_inner`。

### Step 2：运行测试确认修复前失败

```powershell
cargo test test_failed_flight_is_retryable -- --nocapture
cargo test test_panicked_flight_wakes_waiter_and_clears_slot -- --nocapture
```

预期：修复前至少有一个测试失败，表现为第二次请求复用旧失败结果、等待者悬挂或槽位未释放。

### Step 3：实现命名状态和 RAII 守卫

将匿名元组替换为命名结构：

```rust
struct InFlight {
    key: String,
    cell: FlightCell,
}
```

新增 `FlightGuard`：

- `publish_ok(snapshot)`：写入成功结果并 `notify_all`；
- `publish_error(message)`：写入真实错误并 `notify_all`；
- `Drop`：若结果尚未发布，写入“采集线程异常退出”，唤醒等待者；随后只在 `Arc::ptr_eq` 确认仍是本航班时清理 `INFLIGHT`。

`collect_flighted` 使用 `match collect_inner(...)` 显式发布成功/错误；不要再用 `?` 跳过清理。清理锁顺序固定为先 cell、后 `INFLIGHT`，且不在持有 `INFLIGHT` 时等待 cell，避免死锁。

### Step 4：运行测试确认通过

```powershell
cargo test test_failed_flight -- --nocapture
cargo test test_panicked_flight -- --nocapture
cargo test test_parallel_queries_single_collection -- --nocapture
cargo test events_pagination -- --nocapture
```

### Step 5：提交

```powershell
git add src/report.rs
git commit -m "fix(采集): 用 RAII 守卫保证单飞失败可重试"
```

## Task 2：明细分页使用完整不透明游标（工作流 A，Task 1 后）

**Files:**

- Modify: `src/report.rs` 的 `EventRow`、`list_events`、`parse_cursor`
- Modify: `frontend/src/types.ts`、`frontend/src/views/Dashboard.vue`
- Test: `src/report.rs`、`frontend/src/views/Dashboard.test.ts`

### Step 1：定义游标契约

继续按 `(event.ts UTC, record_id)` 排序，但游标必须携带完整精度 UTC 时间戳。展示字段 `ts` 可以保持秒级；新增序列化字段 `cursor` 作为 UI 私有字段，或使用独立的 opaque cursor DTO，不能让前端从展示字符串反解游标。

### Step 2：写失败测试

- `test_events_pagination_same_second_subsecond`
- `test_events_pagination_empty_record_id_tie_break`
- 前端 `events_load_more_uses_opaque_cursor`

fixture 必须包含同一秒内至少两条不同亚秒时间戳，以及多个空 `record_id` 事件。

### Step 3：实现并验证

- `EventRow.cursor` 由完整 UTC 时间戳和稳定 tie-breaker 编码；
- `parse_cursor` 只接受该格式并验证完整精度；
- 前端 `loadEvents(true)` 使用 `last.cursor`，不再拼接 `last.ts|last.record_id`；
- `total` 仍在游标截断前计算。

```powershell
cargo test events_pagination -- --nocapture
pnpm --dir frontend test --run
```

### Step 4：提交

```powershell
git add src/report.rs frontend/src/types.ts frontend/src/views/Dashboard.vue frontend/src/views/Dashboard.test.ts
git commit -m "fix(明细): 使用完整精度游标避免分页漏行"
```

## Task 3：修复 models.dev v1 快照降级（工作流 B）

**Files:**

- Modify: `src/modelsdev.rs`
- Test: `src/modelsdev.rs` 测试模块、必要时新增 `tests/pricing_snapshot_compat.rs`

### Step 1：明确兼容规则

v1 没有办法区分“真实免费 0”和“旧代码把缺失值填成 0”。因此：

- 非零 `cache_read` / `cache_write` 保留；
- v1 中为 0 的缓存分项降级为 `None`；
- v2 继续严格区分 `Some(0.0)` 与 `None`；
- 加载时记录一次 warn，提示需要重新同步，但不能把所有非零缓存价格抹掉。

### Step 2：测试红灯

- `test_modelsdev_v1_preserves_nonzero_cache_prices`
- `test_modelsdev_v1_zero_cache_prices_become_unknown`
- `test_modelsdev_v2_keeps_explicit_zero_prices`

### Step 3：实现并验证

只在 `snapshot.v < 2` 的兼容分支转换缓存字段；不要修改 v2 加载路径。

```powershell
cargo test modelsdev_v1 -- --nocapture
cargo test pricing -- --nocapture
```

### Step 4：提交

```powershell
git add src/modelsdev.rs
git commit -m "fix(计价): 保留 v1 快照中的非零缓存价格"
```

## Task 4：校正内置价格表并建立字段顺序防线（工作流 B）

**Files:**

- Modify: `src/pricing.rs`
- Modify: `tests/e2e_codex.rs`、`tests/partial_pricing.rs`（仅修正独立手算期望）

### Step 1：先确认权威数据

逐个核对受影响模型的 input/output/cache_write/cache_read 顺序和单位。没有权威证据的模型保持未知，不凭猜测交换两列。

### Step 2：写独立字段测试

- `test_builtin_price_columns_are_not_swapped`
- `test_builtin_gpt_cache_read_write_values`
- `test_builtin_lookup_longest_prefix_boundary`

测试直接断言 `ModelPrice` 四个字段，不通过最终费用反推，避免“错表配错期望值”。

### Step 3：实现并验证

修正常量表和所有受影响的独立期望；检查 `normalize_model_id` 和最长前缀匹配不能让 `gpt-50` 命中 `gpt-5`。

```powershell
cargo test builtin_price -- --nocapture
cargo test pricing -- --nocapture
cargo test e2e_codex -- --nocapture
```

### Step 4：提交

```powershell
git add src/pricing.rs tests/e2e_codex.rs tests/partial_pricing.rs
git commit -m "fix(计价): 校正内置缓存价格并锁定字段语义"
```

## Task 5：修复价格完整性判定（工作流 B）

**Files:**

- Modify: `src/pricing.rs` 的 `estimate`
- Test: `tests/partial_pricing.rs`、`tests/golden_reconciliation.rs`

### Step 1：写失败测试

- `test_zero_token_unknown_price_is_complete`
- `test_positive_token_unknown_price_is_partial`
- `test_partial_unknown_tokens_are_reported_without_zero_mark`

### Step 2：实现

价格分项未知时，只有 `tokens > 0` 才设置 `complete = false` 并累加 `unknown_tokens`。已知价格为 0 仍保持完整。

### Step 3：验证

```powershell
cargo test partial_pricing -- --nocapture
cargo test golden_reconciliation -- --nocapture
```

### Step 4：提交

```powershell
git add src/pricing.rs tests/partial_pricing.rs tests/golden_reconciliation.rs
git commit -m "fix(计价): 仅对实际未知用量标记不完整"
```

## Task 6：日期范围改用统计时区日历值（工作流 C）

**Files:**

- Modify: `frontend/src/components/DateRangeSelect.vue`
- Modify: `frontend/src/views/Dashboard.vue`、`frontend/src/types.ts`
- Test: `frontend/src/components/DateRangeSelect.test.ts`、`frontend/src/views/Dashboard.test.ts`
- Test: `src/report.rs` 时间范围测试

### Step 1：确定数据契约

日期控件对外传递 `YYYY-MM-DD` 字符串，而不是本机零点毫秒。组件接收当前统计时区，快捷项按该时区计算自然日；Dashboard 直接把字符串传给 `SummaryOptions.from/to`。

### Step 2：写失败测试

- `today_uses_selected_timezone`
- `date_range_cancel_reopen`
- `date_range_clear_all`
- `date_range_dst_boundary`

至少覆盖本机 Asia/Shanghai、选择 UTC、跨 DST 的 IANA 时区，以及从午夜附近打开日期弹层的情况。

### Step 3：实现并验证

- 移除 `new Date().setHours(0, 0, 0, 0)` 作为统计语义来源；
- 日期标签只格式化日历字符串；
- 后端继续按 `resolve_tz` 解释闭区间；
- 取消只丢弃草稿，清除后确定提交 `null`。

```powershell
pnpm --dir frontend test --run
cargo test time_range -- --nocapture
```

### Step 4：提交

```powershell
git add frontend/src/components/DateRangeSelect.vue frontend/src/views/Dashboard.vue frontend/src/types.ts frontend/src/components/DateRangeSelect.test.ts frontend/src/views/Dashboard.test.ts src/report.rs
git commit -m "fix(日期): 按统计时区传递自然日范围"
```

## Task 7：设置、快照和同步的失败安全（工作流 D）

**Files:**

- Modify: `src-tauri/src/commands.rs`
- Modify: `src/settings.rs`、`src/fsutil.rs`、`src/openrouter.rs`、`src-tauri/src/lib.rs`
- Test: 对应 Rust 单元测试和壳测试

### 子任务 7.1：损坏设置不可覆盖

- `source_config_set` 读取失败时直接返回错误，不得 `unwrap_or_default` 后保存；
- 新增 `test_source_config_set_invalid_settings_keeps_file`；
- 自动同步继续遵守 `auto_sync_allowed(Err) == false`。

### 子任务 7.2：所有关键文件统一原子写

- 设置、视图快照、窗口状态、价格索引统一使用 `fsutil::atomic_write`；
- 临时文件名加入随机/唯一后缀，不能只依赖进程号；
- 新增 `test_atomic_write_concurrent_targets_do_not_truncate` 和真实 rename 失败测试。

### 子任务 7.3：同步互斥和逐源失败隔离

- models.dev 与 OpenRouter 同一 provider 只能有一个同步任务；
- 两个 provider 互不阻塞，单源失败不覆盖另一源旧快照；
- 新增 `test_sync_provider_retry_independent`；网络使用 mock，不请求真实服务。

### 子任务 7.4：OpenRouter 输入校验

- 缺价字段保留为 `None`，不能静默变成 0；
- 负价条目拒绝或转为未知并记录 warning；
- 新增 `test_openrouter_missing_price_is_unknown`、`test_openrouter_negative_price_is_rejected`。

```powershell
cargo test settings -- --nocapture
cargo test fsutil -- --nocapture
cargo test sync_provider -- --nocapture
cargo test openrouter -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml
```

### 提交

```powershell
git add src-tauri/src/commands.rs src/settings.rs src/fsutil.rs src/openrouter.rs src-tauri/src/lib.rs
git commit -m "fix(运行): 强化设置与价格同步的失败安全"
```

## Task 8：补齐测试、口径文档和质量门禁（工作流 E）

**Files:**

- Modify: `docs/stats-semantics.md`
- Modify: `docs/plans/active/2026-10-05-product-review-and-roadmap.md`
- Modify: `.githooks/pre-commit`
- Add/Modify: 缺失的 Rust/Vitest 测试

### 必补测试

- `test_token_bucket_contract`
- `test_project_alias_cross_agent`
- `test_claude_nested_project_identity`
- `test_atomic_snapshot_failure_keeps_old`
- `test_sync_provider_retry_independent`
- `today_uses_selected_timezone`
- `pricing_match_source_visible`
- `excluded_scope_visible`
- `source_empty_error_ready_states`

每个测试必须真实执行断言；不能只让 `cargo test <filter>` 空匹配后返回成功。

### 文档和门禁

- 在统计口径文档明确 `isApiErrorMessage` 是否排除；若不排除，删掉与代码不一致的表述；
- 补写项目身份、未知价格、缓存失败和分页 cursor 口径；
- 更新计划中的 C 阶段状态和 D5 提交号；
- 本地 pre-commit 的 clippy 增加 `-D warnings`，与 CI 一致。

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --manifest-path src-tauri/Cargo.toml
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test --run
pnpm --dir frontend build
```

## Task 9：发布前验收与真实数据验证

### 自动化验收

必须全部通过：

- 根库、壳、前端完整门禁；
- 测试前后 `~/.tokenscope` 逐字节一致；
- `cargo test --release --test perf_real_data -- --ignored --nocapture` 只在显式 `TOKENSCOPE_REAL_PERF=1` 下执行；
- `tauri build` 产出安装包。

### 真机验收顺序

1. 先手动同步一次 models.dev 和 OpenRouter 价格；
2. 首次启动测冷扫描，第二次启动测热缓存；
3. 验证同参并发、错误后重试、明细加载更多、同秒记录、日期时区切换；
4. 验证关闭自动同步、损坏设置、断网、单源失败时旧数据仍可查看；
5. 验证来源目录只读、测试不改真实缓存、安装升级不丢索引；
6. 将结果写入 `docs/plans/d5-acceptance-checklist.md`，全部通过后再把本计划移入 `archive/implemented/`。

## 完成定义

- 6 个 P1 均有“修复前失败 → 修复后通过”的回归记录；
- 关键 P2 的失败安全测试通过，未引入新的默认联网或数据丢失行为；
- 所有测试保持密闭，未读写真实用户缓存；
- Rust、Tauri 壳、前端门禁全绿；
- D5 真机验收完成并记录证据；
- 每个工作流一个主题一个 commit，最后由主 agent 做一次集成审查和最终提交/推送。

## 执行记录（2026-10-05，分支 docs/product-review-plan）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| Task 1 单飞 RAII | 修复 `?` 早退导致失败航班占据槽位不可重试；FlightGuard（publish_ok/publish_error/Drop 兜底 panic）+ 单飞槽改 HashMap 按 key 管理（不同参数可并发、同参共享，修复跨 key 顶替竞态）；leader 工作可注入。test_failed_flight_is_retryable / wakes_all_waiters / panicked_wakes_waiter_and_clears_slot 红→绿 | `bacb56f` |
| Task 2 完整精度游标 | EventRow.cursor（完整精度 UTC + record_id 不透明串），前端原样回传；同秒亚秒不丢行、空 record_id 靠亚秒决序。test_events_pagination_same_second_subsecond / empty_record_id_tie_break 红→绿 | `66e5576` |
| Task 3 v1 快照兼容 | v1 非零缓存价保留（此前一律抹成未知），0 → 未知；v2 严格区分不变。三个命名测试红→绿 | `19e25d1` |
| Task 4 内置表校正 | ⚠️ **已被 2026-10-06 定价来源策略 superseded**：内置价格表已整体移除（见 pricing-source-policy 计划 Task 1），逐行校正与费率结构防线转由测试 fixture 承载；lookup 词元边界（gpt-50 不命中 gpt-5）保留在生产 lookup 中 | `2975c3e` → superseded |
| Task 5 完整性判定 | 未知分项仅 tokens>0 才置 complete=false（零 token 未知不打 †）；partial_cost_totals 的 d2 断言随新语义翻转 | `1850d18` |
| Task 6 统计时区日期 | 控件契约改日历字符串 + tz prop；快捷项按所选时区解释"今天"（Intl 实际偏移，DST 安全）；Dashboard 直传字符串；快照 v3。today_uses_selected_timezone / dst_boundary 红→绿 | `273b625` |
| Task 7 失败安全 | 7.1 损坏设置报错不覆盖（keeps_file）；7.2 四类文件统一原子写 + 唯一临时名（并发不串档 + rename 失败清理）；7.3 双 provider 独立 SYNC_LOCK + sync_with 可注入 mock（单源失败不覆盖另一源）；7.4 OpenRouter 缺价 None / 负价拒绝 | `18efcda` |
| Task 8 测试与文档 | 补 project_alias_cross_agent / claude_nested_project_identity / empty 态 / statsView 纯函数两测试；口径文档修正 isApiErrorMessage 表述（代码不读取）并补项目身份/游标/部分计价/缓存失败口径；钩子 clippy -D warnings（双 manifest） | `8c40fb5` |
| Task 9 自动化验收 | 完整门禁绿（根库 104 + 壳 6 + 前端 30 测试，clippy -D warnings 双 manifest 干净）；正常套件前后 ~/.tokenscope 哈希逐字节一致；TOKENSCOPE_REAL_PERF=1 显式运行冷 6.2s / 热 0.15s（21,055 请求 $1312.70）；tauri build 4.82 MiB。真机验收项后延（用户决策） | `本次提交` |

完成定义核对：6 个 P1 全部有红→绿回归记录 ✅；关键 P2 失败安全测试通过、无新增默认联网/数据丢失 ✅；测试密闭 ✅；三门禁全绿 ✅；D5 真机验收**后延待用户**（非阻塞代码项）。
