# 定价来源策略调整实施计划

> **状态（2026-10-06）：Task 1–5 全部完成**——内置价格表已移除，三层来源 + 首启同步横幅 + 设置页解释落地；完整门禁与 tauri build 通过；真机验收项（横幅/断网/同步时序）随 D5 外部验收批次执行（用户决策后延）。执行记录见文末。

**Goal:** 移除编译期内置价格表，以 models.dev 为主要定价来源、以本地 models.dev 快照作为离线兜底，并在首次启动没有价格缓存时给用户明确的联网同步提示。

**Architecture:** 生产定价只由三层组成：外置 `pricing.toml`（用户明确覆盖）> models.dev（主源，在线同步或本地快照）> OpenRouter（补充源，在线同步或本地快照）。没有任何可用来源时保留未知价格语义，不猜测、不静默按 0 计费。价格可用性通过结构化状态 DTO 从 Rust 传到 Tauri/Vue，全局横栏只依据该状态显示。

**Tech Stack:** Rust 2024、Serde、SQLite 价格索引、Tauri 2、Vue 3、TypeScript、Vitest、Naive UI。

---

## 产品决策

### 保留的来源

| 层级 | 来源 | 用途 |
| --- | --- | --- |
| 1 | 外置 `~/.tokenscope/pricing.toml` | 用户手工补充或覆盖；最高优先级 |
| 2 | models.dev 在线快照 | 主定价源；模型覆盖面和维护优先级最高 |
| 3 | 本地 models.dev 快照 | 断网时继续使用上次成功同步的主源数据 |
| 4 | OpenRouter 在线/本地快照 | models.dev 未覆盖模型的补充来源 |

### 移除的来源

- 删除 `src/pricing.rs` 中编译期静态 `TABLE` 和 `TIER_BUILTIN`。
- 不再把任意模型落到内置猜测价格。
- 无匹配价格时，`Pricing::lookup` 返回 `None`，汇总和明细显示未知价格及未知 token 数。

### 首次启动行为

当 models.dev 快照不存在、损坏或条目数为 0 时：

- 顶部显示横幅：**“尚未获取定价，需要联网同步价格；当前费用仅能显示为未知。”**
- 提供“立即同步”按钮，调用现有双源同步命令。
- 同步失败时保留横幅，显示失败原因；不得生成伪造或过期的内置价格。
- 如果 OpenRouter 或外置文件仍有可用条目，允许这些条目计价，但横幅仍提示 models.dev 主源尚未就绪。

### 离线行为

- 有有效的本地 models.dev 快照时，断网不显示首次同步横幅。
- 本地快照解析失败时，旧快照不能被覆盖；界面显示“主源不可用”，并允许用户重试同步。
- OpenRouter 只作为补充源，不改变 models.dev 是主源的产品说明和来源标签。

## 必须保持的不变量

1. 不存在编译期价格 fallback；未收录模型始终是 unknown。
2. `Some(0.0)` 仍表示明确免费，`None` 表示未知，未知不能按 0 计费。
3. 外置 > models.dev > OpenRouter 的优先级保持稳定，层内使用最长前缀匹配和变体隔离。
4. 本地快照只读；同步成功后才原子替换快照，失败保留旧文件。
5. 价格可用性由结构化字段表达，前端不得通过解析 warning 文本推断状态。
6. 删除内置来源后，价格索引必须失效重建，旧索引不能重新注入内置条目。
7. 测试不得读写真实 `~/.tokenscope`；所有路径显式注入临时目录。

## 工作分派与依赖

| 工作流 | 文件所有权 | 任务 | 依赖 |
| --- | --- | --- | --- |
| A：价格核心 | `src/pricing.rs`、`src/modelsdev.rs`、相关 Rust 测试 | 删除内置层、快照兼容、索引失效、来源标签 | 无 |
| B：命令与状态 | `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`、壳测试 | 定价状态 DTO/command、同步后状态刷新 | A 的 DTO 契约 |
| C：全局提示 | `frontend/src/App.vue`、新建 `PricingStatusBanner.vue`、Vitest | 首次同步横幅和按钮 | B 的 command 契约 |
| D：设置页和文档 | `frontend/src/views/Settings.vue`、`types.ts`、`docs/stats-semantics.md` | 来源名称、数量、说明和未知语义 | A–C |

A 完成并确认 DTO 后再做 B；B 完成后做 C；D 最后合并，避免多个 agent 同时修改 `PricingView` 或 `Settings.vue`。

## Task 1：建立定价来源和状态契约（工作流 A）

**Files:**

- Modify: `src/pricing.rs`
- Modify: `src/modelsdev.rs`（如需补充快照诊断）
- Test: `src/pricing.rs` 测试模块、必要时新增 `tests/pricing_source_policy.rs`

### Step 1：写失败测试

- `test_no_builtin_entries_after_source_policy_change`
- `test_unknown_model_without_sources_is_unknown`
- `test_external_overrides_modelsdev`
- `test_modelsdev_overrides_openrouter`
- `test_source_labels_distinguish_modelsdev_and_openrouter`

测试必须用临时外置 TOML、models.dev JSON、OpenRouter JSON 和 `pricing_index`。

### Step 2：实现核心层

- 删除 `TABLE`、`TIER_BUILTIN`、`Pricing::builtin()`；提供空价格表构造方法。
- `Pricing::default()` 改为空表，保证没有显式来源时不会偷偷给价。
- `Pricing::load()` 从空表开始加载三层；没有快照只返回 warning，不添加伪价格。
- `PricingEntry.source` 明确返回 `外置`、`models.dev`、`OpenRouter`。
- 更新模块注释、价格层级说明和外置模板注释。

### Step 3：处理索引和缓存迁移

- 递增 `INDEX_VERSION`，使旧的含内置条目的 `pricing-index.json` 自动失效。
- 为 source policy 增加签名版本；即使源文件指纹不变，旧索引也必须重建。
- 增加测试确认从旧索引恢复后不再出现 `TIER_BUILTIN` 条目。

### Step 4：验证并提交

```powershell
cargo test pricing_source_policy -- --nocapture
cargo test pricing -- --nocapture
cargo test --test partial_pricing -- --nocapture
cargo fmt --check
```

```powershell
git add src/pricing.rs src/modelsdev.rs tests/pricing_source_policy.rs
git commit -m "feat(计价): 移除编译期内置价格来源"
```

## Task 2：保留 models.dev v1/v2 快照兼容并暴露可用性（工作流 A/B）

**Files:**

- Modify: `src/modelsdev.rs`
- Modify: `src-tauri/src/commands.rs`
- Test: `src/modelsdev.rs`、`src-tauri` 测试

### Step 1：写失败测试

- `test_modelsdev_v1_preserves_nonzero_cache_prices`
- `test_modelsdev_v1_zero_cache_prices_become_unknown`
- `test_modelsdev_v2_keeps_explicit_zero_prices`
- `test_pricing_status_missing_modelsdev_snapshot`
- `test_pricing_status_uses_cached_modelsdev_snapshot`
- `test_pricing_status_invalid_snapshot_is_degraded`

### Step 2：实现状态 DTO

新增结构化 `PricingStatus`，至少包含：

```rust
pub struct PricingStatus {
    pub modelsdev_available: bool,
    pub modelsdev_count: usize,
    pub modelsdev_synced_at: Option<String>,
    pub openrouter_available: bool,
    pub external_count: usize,
    pub has_any_pricing: bool,
    pub needs_sync: bool,
    pub warnings: Vec<String>,
}
```

新增 `pricing_status` Tauri command。状态计算必须复用与 `pricing_entries` 相同的路径解析和快照解析规则，不能出现设置页显示可用而汇总页认为不可用的分叉。

### Step 3：验证

```powershell
cargo test modelsdev -- --nocapture
cargo test pricing_status --manifest-path src-tauri/Cargo.toml -- --nocapture
cargo test --manifest-path src-tauri/Cargo.toml
```

## Task 3：全局首次同步横幅（工作流 C）

**Files:**

- Create: `frontend/src/components/PricingStatusBanner.vue`
- Modify: `frontend/src/App.vue`
- Modify: `frontend/src/types.ts`
- Test: `frontend/src/components/PricingStatusBanner.test.ts`、`frontend/src/App.test.ts`

### Step 1：写失败测试

- `pricing_banner_visible_without_modelsdev_cache`
- `pricing_banner_hidden_with_cached_modelsdev`
- `pricing_banner_sync_success_refreshes_status`
- `pricing_banner_sync_failure_keeps_warning`

### Step 2：实现

- `App.vue` 启动时调用 `pricing_status`；状态为 `needs_sync` 时在 header 下方显示横栏。
- 横栏提供“立即同步”按钮，调用 `sync_pricing_openrouter`；成功后重新调用 `pricing_status`。
- 同步过程中禁用按钮并显示加载状态；失败显示错误但不吞掉原有横幅。
- 横栏使用 `NAlert`，不阻塞 Dashboard；用户仍可查看日志和未知费用。

### Step 3：验证

```powershell
pnpm --dir frontend test --run
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
```

## Task 4：设置页来源展示和费用解释（工作流 D）

**Files:**

- Modify: `frontend/src/views/Settings.vue`
- Modify: `frontend/src/types.ts`
- Modify: `docs/stats-semantics.md`
- Test: `frontend/src/views/Settings.test.ts`（如当前不存在则新建）

### 实现要求

- 来源标签显示 `外置`、`models.dev`、`OpenRouter`，不再出现“内置”。
- 价格表说明改为“外置 > models.dev > OpenRouter”；明确本地 models.dev 快照是离线缓存。
- models.dev 缺失或损坏时显示状态、路径和“需要同步”提示。
- OpenRouter 缓存价格不覆盖 models.dev 同前缀价格，只作为补充。
- 未知模型和未知分项继续显示 `†` 与未知 token，不按 0 静默展示为完整费用。
- 删除“同步前仅内置 + 外置生效”等旧文案。

### 验证

```powershell
pnpm --dir frontend test --run
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
```

## Task 5：迁移、真实数据和发布验收

### 自动化验收

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

额外断言：

- 删除/改名旧价格索引后，首次启动会重建且不出现 builtin 条目；
- 使用本地 models.dev 快照时断网仍能计价；
- 删除 models.dev 快照后，首次启动横幅出现；
- 同步失败不会删除上一份可用快照；
- 没有任何来源时所有模型均为 unknown，费用不会被静默记为 0。

### 真机验收顺序

1. 备份并移走 `~/.tokenscope/pricing-modelsdev.json` 与 `pricing-index.json`；启动应用，确认顶部横幅出现。
2. 点击“立即同步”，确认 models.dev 和 OpenRouter 成功后横幅消失，设置页显示两个来源的数量。
3. 断网重启，确认本地 models.dev 快照仍能计价且不显示首次同步横幅。
4. 删除外置 `pricing.toml`，确认没有内置价格悄悄出现。
5. 使用未知模型 fixture，确认 UI 显示未知 token 和 `†`。

### 完成定义

- 代码中没有生产 `TIER_BUILTIN`、`Pricing::builtin()` 或静态价格 fallback；
- models.dev 来源可在设置页和日志中单独识别；
- 首次无缓存时横幅可见、可同步、失败可重试；
- 离线 models.dev 快照可继续工作；
- 索引版本迁移、Rust、壳和前端门禁全部通过；
- 真机验收证据写入 `docs/plans/d5-acceptance-checklist.md`。

## 执行记录（2026-10-06，分支 docs/product-review-plan）

| 任务 | 结果 | 提交 |
| --- | --- | --- |
| Task 1 移除内置层 | TABLE/TIER_BUILTIN/builtin() 删除；default=空表；load 三层叠加；来源标签 外置/models.dev/OpenRouter；INDEX_VERSION→3（含内置旧索引失效，test_source_labels 断言编译期版本）；策略五件套测试 + 受影响测试迁移（pricing 单测 fixture_pricing、e2e 外置 TOML、partial_pricing d2 无兜底重算、aggregate 空表语义、external_broken 无兜底断言） | `7732625` |
| Task 2 PricingStatus | 结构化 DTO + pricing_status 命令（与 pricing_entries 同路径解析）；missing/cached/invalid_degraded 三测试。v1/v2 快照兼容测试此前已交付 | `d1c963b` |
| Task 3 全局横幅 | PricingStatusBanner（needsSync 结构化字段驱动）+ App 接入；立即同步 → 刷新状态；失败保留横幅 + 原因。红基线：computed 误写 snake_case needs_sync（可见性测试暴露）→ 四测试绿 | `bce36e3` |
| Task 4 设置页与文档 | 来源标签映射（models.dev/OpenRouter/外置，无内置）；优先级文案改三层 + 离线快照说明；主源缺失 NAlert 提示；priceSourceLine 三态测试；口径文档费用章节更新 | `229b0c2` |
| Task 5 验收 | 完整门禁绿（根库 104 + 壳 9 + 前端 35 测试、clippy -D warnings 双 manifest、前端 build）；tauri build 4.82 MiB。额外断言映射：旧索引失效（INDEX_VERSION 3 + 重启命中测试）、离线快照计价（pricing_status cached + modelsdev_layer）、无快照横幅（banner visible）、同步失败保旧快照（sync_provider_retry_independent）、无来源全 unknown（unknown_model_without_sources）。真机时序验收随 D5 批次执行 | `本次提交` |

**审阅后续（2026-10-06 review-findings-remediation）**：旧索引迁移补测（v2 含内置条目启动即失效重建，无内置条目恢复）——见该计划 Task 5。

完成定义核对：无生产 TIER_BUILTIN/builtin()/静态 fallback ✅；models.dev 来源可单独识别（标签 + 状态 DTO + 日志）✅；首启横幅可见/可同步/失败可重试 ✅；离线快照可用 ✅；索引迁移与三门禁通过 ✅；真机证据随 D5 批次（后延，用户决策）。
