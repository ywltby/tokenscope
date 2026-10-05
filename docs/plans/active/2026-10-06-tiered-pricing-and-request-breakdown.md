# Tiered Pricing and Request Cost Breakdown Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将短上下文/长上下文等分段价格抽象成可复用的价格计划，支持多个阈值、每档多类单价和不同的大小条件，并让每条请求在费用悬浮提示中展示实际的价格来源、命中档位和计算明细。

**Architecture:** 在现有 `Pricing` 查找结果中加入通用 `PricePlan`：一个基础价格加零个或多个有界分段。每条请求先按声明的计价依据计算一个上下文度量值，再选择唯一分段；当前落地的官方规则采用“整笔请求切换档位”，输入、输出、缓存写入和缓存读取都使用命中档位中对应的单价，缺失分项沿用该模型的基础价格，基础价格也缺失时才记为 unknown。models.dev、OpenRouter 和外置 TOML 都先转换为同一规范模型，聚合与明细复用同一个 `CostEstimate` 和 breakdown，避免两套计价逻辑漂移。

**Tech Stack:** Rust 2024、serde/serde_json、TOML、Tauri 2、Vue 3、TypeScript、Naive UI、Vitest、Cargo test/clippy/rustfmt。

---

## Sub2API 参考结论

参考项目：[Wei-Shaw/sub2api](https://github.com/Wei-Shaw/sub2api)。重点参考文件为 `backend/internal/service/channel.go`、`model_pricing_resolver.go`、`billing_service.go` 和 `billing_context_schedule.go`。

可复用的做法：

1. 用 `PricingInterval` 表示 `min/max`、输入/输出/缓存等多类价格，而不是增加一个仅适用于 OpenAI 的 `long_context_price` 字段。
2. 先按上下文 token 数找到一个区间，再把区间中显式填写的价格覆盖基础价格；空字段不应被当成免费。
3. 用统一的 `CostBreakdown` 记录输入、输出、缓存和总价，所有入口都调用同一个计算函数。
4. 上下文度量值包含 `input + cache_write + cache_read`，这与本项目的 `UsageEvent` 结构兼容。

需要调整的地方：

- Sub2API 的 `FindMatchingInterval` 使用隐含的 `(min, max]` 语义；TokenScope 改用明确的 `[min, max)` 规范，并由各来源适配器转换上游阈值，避免 `272000` 边界被不同来源误解。
- Sub2API 的部分代码仍有编译期 fallback 价格；TokenScope 继续执行“外置 > models.dev > OpenRouter、无来源即 unknown”，不恢复内置价格。
- 当前官方数据表达的是整笔请求切换价格档位。计划实现 `whole_request`，为将来的 `marginal` 留出类型扩展点，但不会在没有来源明确定义时擅自把一笔请求拆成边际区间。

## 范围外

- 不增加任何编译期模型价格表。
- 统计管线不联网；计算只读取当前已加载的价格索引/离线快照。
- 不根据模型最大上下文窗口推导价格；只有价格来源明确给出的分段规则才参与计价。
- 不改变已有四类 token、unknown token 和三层价格来源的产品语义。
- 不在本计划中实现图片像素、音频时长等非 token 计量；数据模型保留可扩展的 `basis` 枚举，但当前只启用 token 依据。

## 必须保持的不变量

1. 价格选择仍按来源优先级和最长模型前缀裁决；同一个最终命中条目的基础价格与分段价格来自同一来源，禁止把 models.dev 的基础价和 OpenRouter 的高档价拼成一张卡。
2. `prompt_tokens` 的规范值为 `input + cache_write + cache_read`；`output` 不参与当前上下文档位选择。
3. 规范区间为左闭右开 `[min_tokens, max_tokens)`；`max_tokens = None` 表示无上限；同一价格计划的区间不得重叠或出现空洞后被错误猜测。
4. models.dev 的 `tier.type = context` 且 `size = S` 表示短档覆盖 `prompt_tokens <= S`，高档从 `S + 1` 开始；OpenRouter 的 `min_prompt_tokens = S` 按上游字段语义从 `S`（包含）开始。两个来源的相同数字不能共用未经转换的边界。
5. 分段缺少某个价格字段时，先回退到同一条目的基础价格；基础价格也缺失时，该分项 token 进入 `unknown`，不得按 0 计费。显式 0 仍表示免费。
6. 无匹配分段且没有合法基础价格时，整条请求的缺价原因必须可见；不得自动选最近档位。
7. 请求级 breakdown 与聚合总价必须调用同一个估算结果；前端不能自行重算费用。
8. 所有价格都在 Rust 内部统一为 USD/百万 token；OpenRouter 的 USD/token 在快照导入时转换，悬浮提示明确显示单位。
9. 价格规则缺失、快照过期或旧索引被丢弃时，现有 unknown 和首次联网提示语义保持不变。
10. 新增测试一律注入临时 `cache_dir` 和 `pricing_index`，不得触碰真实 `~/.tokenscope`。

## 规范数据模型与计算公式

在 `src/pricing.rs` 中引入以下概念（具体 Rust 命名可按现有风格调整）：

```text
PricePlan {
    basis: PromptTokens | InputTokens | OutputTokens | TotalTokens,
    application: WholeRequest,
    base: PriceRates,
    segments: Vec<PriceSegment>,
}

PriceSegment {
    label: Option<String>,
    min_tokens: u64,
    max_tokens: Option<u64>,
    prices: PriceRates, // 每项 Option<f64>，None 表示沿用 base
}
```

当前来源统一使用 `basis = PromptTokens` 和 `application = WholeRequest`。`InputTokens` 等枚举值用于后续复用，若外置文件声明当前版本不支持的 basis/application，加载时给出诊断并跳过该规则，不能静默改变计价含义。

对一条请求：

```text
prompt_tokens = input + cache_write + cache_read
segment = 唯一满足 min_tokens <= prompt_tokens < max_tokens 的分段
rate(component) = segment.prices.component ?? plan.base.component
subtotal(component) = component_tokens * rate(component) / 1_000_000
cost = 所有有单价分项 subtotal 之和
unknown = 所有没有最终单价的分项 token
```

`CostEstimate` 增加 `basis_value`、`basis`、`source`、命中分段标签/范围、分项明细和 unknown 原因。由于 breakdown 含 `Vec`，`CostEstimate` 从 `Copy` 改为 `Clone`，并同步所有调用点。

## 执行顺序

先完成 Task 1 的模型和边界不变量，再按 Task 2–5 接入来源和计算器；Task 6–8 接入 report/UI；Task 9 做迁移、文档和完整验收。每个 Task 独立提交，定向测试通过后再进入下一项。

### Task 1：建立通用价格计划和边界匹配器

**Files:**
- Modify: `src/pricing.rs`（`Entry`、`ModelPrice`、`CostEstimate`、索引 DTO、价格查找）
- Modify: `src/model.rs`（如需新增 `TokenCounts::prompt_tokens()`，保持现有序列化字段不变）
- Test: `src/pricing.rs` 单元测试

**Step 1: 写失败测试**

新增以下测试：

- 空分段时沿用基础四类价格；
- 三段 `[0, 100000)`、`[100000, 272001)`、`[272001, None)` 的下界、上界和最后一档选择；
- 相邻分段边界 `100000` 只命中后一段，`272000` 与 `272001` 分别命中不同段；
- 重叠、反向、非单调和“无上限但后面还有分段”被拒绝；
- 分段只覆盖 input 时 output/cache 回退基础价，显式 0 不回退；
- 没有基础价和没有命中分段时返回 unknown，不返回 0。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing::tests::test_pricing_segment -- --nocapture
```

预期：当前没有 `PricePlan`、区间校验和分段选择能力，测试无法编译或失败。

**Step 3: 实现最小模型**

把现有四个扁平价格字段迁移到 `PriceRates`，在 `Entry` 和 `IndexEntry` 中保存 `PricePlan`。实现：

- `validate_price_plan`：校验非负价格、区间边界、无重叠和唯一匹配；
- `select_segment(plan, basis_value)`：只按 `[min, max)` 选择，不做最近档位猜测；
- `effective_rates(plan, selected)`：逐字段执行“分段显式值 > 基础值”；
- 保留旧平价条目的反序列化兼容，读取旧索引时把四个扁平价格包装成无分段计划。

不要在此 Task 改变来源加载优先级或 UI。

**Step 4: 运行测试确认通过**

```powershell
cargo test pricing::tests::test_pricing_segment -- --nocapture
cargo test pricing -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/pricing.rs src/model.rs
git commit -m "feat(定价): 建立通用分段价格计划"
```

### Task 2：接入 models.dev 的 tiers 和 context_over_200k

**Files:**
- Modify: `src/modelsdev.rs`（`ApiCost`、tier DTO、`SnapshotEntry`、同步和加载）
- Modify: `src/pricing.rs`（models.dev 条目转 `PricePlan`）
- Test: `src/modelsdev.rs`、`src/pricing.rs` 的同步/快照 fixture

**Step 1: 写失败测试**

用真实响应形状构造 fixture：

```json
{
  "input": 4,
  "output": 20,
  "cache_read": 0.4,
  "cache_write": 5,
  "tiers": [{
    "input": 8,
    "output": 30,
    "cache_read": 0.8,
    "cache_write": 10,
    "tier": {"type": "context", "size": 272000}
  }],
  "context_over_200k": {"input": 8, "output": 30, "cache_read": 0.8, "cache_write": 10}
}
```

断言同步快照保留分段，`272000` 仍命中基础档，`272001` 命中高档；没有 `tiers` 时 `context_over_200k` 仍能生成高档；非 context 类型的 tier 被忽略并留下 warning；`tiers` 与旧字段冲突时只使用确定的优先级，不重复计价。

**Step 2: 运行测试确认失败**

```powershell
cargo test modelsdev::tests::test_modelsdev_tier -- --nocapture
```

预期：当前 serde 会丢弃 `tiers` 和 `context_over_200k`。

**Step 3: 实现来源转换**

新增可选字段并写入 models.dev 快照。规则为：

- `tiers` 只接受 `tier.type = "context"`；按 `size` 排序，转换成规范的上下文区间；
- 多个 `size` 按升序形成连续档位：第一个阈值之前使用基础价，某一 tier 的价格从该 tier 的 `size + 1` 开始，直到下一个 tier 的 `size`；最后一档为无上限。若来源同时给出无法组成连续档位的记录，跳过冲突记录并留下 warning；
- `context_over_200k` 仅作为没有等价 `tiers` 时的兼容输入；等价时去重，冲突时优先 `tiers` 并记录 warning；
- 快照版本递增，旧 v1/v2 快照仍可作为“只有基础价格”的离线数据读取，不伪造分段；新同步写入新版本；
- 保留缺失价格字段为 `None`，显式 `0` 保持为免费。

**Step 4: 运行测试确认通过**

```powershell
cargo test modelsdev -- --nocapture
cargo test pricing_modelsdev -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/modelsdev.rs src/pricing.rs
git commit -m "feat(modelsdev): 保存上下文分段价格"
```

### Task 3：接入 OpenRouter overrides

**Files:**
- Modify: `src/openrouter.rs`（`ApiPricing.overrides`、`SnapshotEntry`、版本字段）
- Modify: `src/pricing.rs`（OpenRouter 条目转换）
- Test: `src/openrouter.rs`、`src/pricing.rs` 的 overrides fixture

**Step 1: 写失败测试**

构造含多个 override 的响应：基础 `prompt/completion` 一档，`min_prompt_tokens=272000` 和 `min_prompt_tokens=1000000` 两档。断言快照保存全部档位，并验证精确下界按 OpenRouter 的 inclusive 语义命中 override。

同时覆盖 cache read/write 缺失、空字符串、负价和多个 override 顺序乱序的情况。

**Step 2: 运行测试确认失败**

```powershell
cargo test openrouter::tests::test_openrouter_override -- --nocapture
```

预期：当前 `overrides` 未解析，只有扁平四价。

**Step 3: 实现来源转换**

解析 `min_prompt_tokens` 和每个 override 的四类 USD/token 价格，在导入快照时乘以 `1_000_000` 统一为 USD/百万 token。按 `min_prompt_tokens` 升序建立 `[min, next_min)` 分段，基础价覆盖最低阈值以下；同一阈值重复项保留确定性的一项并写 warning。快照增加版本号，旧快照按无分段基础价兼容读取。

禁止把 OpenRouter override 合并到 models.dev 条目；来源优先级仍在 `Pricing::load` 的条目级别裁决。

**Step 4: 运行测试确认通过**

```powershell
cargo test openrouter -- --nocapture
cargo test pricing::tests::test_pricing_openrouter_override -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/openrouter.rs src/pricing.rs
git commit -m "feat(openrouter): 接入多档 prompt 价格"
```

### Task 4：扩展外置 pricing.toml 语法并保持兼容

**Files:**
- Modify: `src/pricing.rs`（`ExternalModel`、模板、加载校验）
- Modify: `src/modelsdev.rs`/`src/openrouter.rs`（如需共享价格 DTO，不复制解析逻辑）
- Test: `src/pricing.rs` 外置 TOML 测试

**Step 1: 写失败测试**

加入如下外置 fixture：

```toml
[[model]]
prefix = "gpt-5.6"
basis = "prompt_tokens"
application = "whole_request"
input = 4.0
output = 20.0
cache_write = 5.0
cache_read = 0.4

[[model.segment]]
label = ">272K"
min_tokens = 272001
input = 8.0
output = 30.0
cache_write = 10.0
cache_read = 0.8
```

断言旧 flat TOML 仍能加载；新 TOML 产生分段；非法 basis/application、负价格、重叠区间和未封尾区间被拒绝且不影响其他来源的诊断；外置条目仍最高优先。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing::tests::test_pricing_external_segment -- --nocapture
```

**Step 3: 实现兼容解析**

新增 `[[model.segment]]` 反序列化结构和模板注释，四类价格字段保持可选以区分“未填写”和显式 0。旧写法自动转换为无分段 `PricePlan`。对无效条目保留现有“外置文件错误不静默回退”的诊断语义，避免用户以为自定义价格已生效。

**Step 4: 运行测试确认通过**

```powershell
cargo test pricing::tests::test_pricing_external -- --nocapture
cargo test pricing -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/pricing.rs
git commit -m "feat(定价): 支持外置分段价格配置"
```

### Task 5：实现统一估算器和请求级 breakdown

**Files:**
- Modify: `src/pricing.rs`（`estimate`、分项计算 DTO）
- Modify: `src/aggregate.rs`（复用估算结果）
- Test: `src/pricing.rs`、`src/aggregate.rs`

**Step 1: 写失败测试**

使用一条基础价 + `272001` 高档的 fixture，覆盖：

- 低于阈值、恰好等于阈值、刚超过阈值；
- input、output、cache_write、cache_read 同时存在；
- 分段只覆盖部分价格字段；
- 完全缺价和部分缺价；
- 同一请求被估算两次时 breakdown 与总价完全相同。

测试断言每一项的 token 数、单位单价、USD 小计、总价、来源、basis 值、命中档位和 unknown token 均正确。示例公式使用 `tokens * usd_per_million / 1_000_000`，不要用展示层四舍五入值参与计算。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing::tests::test_pricing_cost_breakdown -- --nocapture
```

**Step 3: 实现统一计算**

让 `Pricing::estimate(model, counts)`：

1. 查找最终 `Entry`；
2. 计算 `basis_value = counts.input + counts.cache_write + counts.cache_read`；
3. 选择分段并得到 effective rates；
4. 为四类 token 生成 `CostLine { kind, tokens, unit_price, subtotal, priced }`；
5. 累加已知小计，记录 unknown token 和缺价原因；
6. 返回 `CostEstimate`，保留现有 `cost/complete/unknown` 字段兼容调用方。

聚合层继续按事件估算再累加；如同一事件需要明细和聚合同时使用，使用同一 `CostEstimate`，不要在 aggregate 和 report 各写一套公式。

**Step 4: 运行测试确认通过**

```powershell
cargo test pricing::tests::test_pricing_cost_breakdown -- --nocapture
cargo test aggregate -- --nocapture
cargo test pricing -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/pricing.rs src/aggregate.rs
git commit -m "feat(计费): 统一分段价格计算明细"
```

### Task 6：让 report 输出请求级费用明细

**Files:**
- Modify: `src/report.rs`（`EventRow`、事件列表映射）
- Modify: `src/model.rs`（如需为 breakdown DTO 提供稳定枚举序列化）
- Test: `src/report.rs` 明细测试

**Step 1: 写失败测试**

新增请求明细 fixture，断言 `EventRow.cost_breakdown` 包含：模型、来源、计价依据、basis token、命中档位、四个分项的 token/单价/小计、总价和 unknown 信息；低档与高档请求各一条，并断言 breakdown 总价等于 `cost_usd`。

**Step 2: 运行测试确认失败**

```powershell
cargo test report::tests::test_event_cost_breakdown -- --nocapture
```

**Step 3: 实现 DTO 接线**

新增可序列化的 `EventCostBreakdown`/`CostLine` DTO，优先复用 pricing 层的结构，不把 Rust 内部 `Entry` 或路径泄露给前端。`list_events` 对每条事件只调用一次 estimate，同时填充 `cost_usd` 和 `cost_breakdown`。未知价格也返回可解释的 breakdown（例如“缓存读缺少单价”），而不是只给空费用。

保持分页 cursor、排序、过滤和现有 `cost_usd = null` 兼容；不要把 breakdown 写入 SQLite 事件缓存，价格变化后重新估算即可。

**Step 4: 运行测试确认通过**

```powershell
cargo test report::tests::test_event_cost_breakdown -- --nocapture
cargo test report -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/report.rs src/model.rs
git commit -m "feat(明细): 输出请求级费用计算过程"
```

### Task 7：前端费用悬浮提示

**Files:**
- Modify: `frontend/src/types.ts`（`EventRow`、breakdown DTO）
- Modify: `frontend/src/components/EventTable.vue`（费用列 tooltip）
- Test: `frontend/src/components/EventTable.test.ts`

**Step 1: 写失败测试**

用一个低档、一个高档和一个部分 unknown 的 `EventList` fixture，挂载真实 `EventTable`，触发费用单元格的 hover/tooltip，断言可见：

- 来源和模型；
- `prompt tokens = input + cache_write + cache_read` 的度量式；
- 命中档位及范围；
- 每个分项的 token、USD/百万 token 单价和小计；
- 总价或 unknown 原因。

断言 `cost_usd` 仍按现有格式显示，tooltip 不使用二进制浮点长尾，也不在前端重新计算金额。

**Step 2: 运行测试确认失败**

```powershell
pnpm --dir frontend test -- EventTable
```

**Step 3: 实现最小 UI**

复用当前已使用的 `NTooltip`。费用数字作为 trigger，`cost_breakdown` 存在时显示结构化多行内容；缺少 breakdown 的旧/异常响应显示“暂无计算明细”而不是崩溃。unknown 分项使用明确颜色和文案，显示已计价小计与未计价 token。

抽出纯函数格式化逻辑（例如 `formatCostBreakdown`）并单测，保持 tooltip 宽度、暗色主题和窄窗口可读性。不要把价格选择逻辑复制到 Vue。

**Step 4: 运行测试确认通过**

```powershell
pnpm --dir frontend test -- EventTable
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
```

**Step 5: 提交**

```powershell
git add frontend/src/types.ts frontend/src/components/EventTable.vue frontend/src/components/EventTable.test.ts
git commit -m "feat(gui): 悬浮展示请求费用计算明细"
```

### Task 8：设置页展示分段价格

**Files:**
- Modify: `src/pricing.rs`（`PricingEntry` 序列化计划和 segments）
- Modify: `src-tauri/src/commands.rs`（若 command DTO 需要同步；保持 command 名称不变）
- Modify: `frontend/src/types.ts`
- Modify: `frontend/src/views/Settings.vue`（价格表 tooltip/档位展示）
- Test: `src/pricing.rs`、`frontend/src/views/Settings.test.ts`

**Step 1: 写失败测试**

断言设置页的一个模型条目能显示基础价、计价依据和所有分段的范围及四类单价；没有分段的普通模型仍保持现有表格布局；缺失分项显示“未知”，显式 0 显示 `$0`。

**Step 2: 实现 DTO 和 UI**

在 `PricingEntry` 中增加 `basis`、`segments`、`has_tiered_pricing` 等只读字段。设置页使用现有 `NTooltip` 展开档位，不改变价格编辑文件的打开/保存行为。来源标签继续显示外置、models.dev、OpenRouter，不新增内置来源。

**Step 3: 运行测试确认通过**

```powershell
cargo test pricing::tests::test_pricing_entries_segments -- --nocapture
pnpm --dir frontend test -- Settings
pnpm --dir frontend typecheck
```

**Step 4: 提交**

```powershell
git add src/pricing.rs src-tauri/src/commands.rs frontend/src/types.ts frontend/src/views/Settings.vue frontend/src/views/Settings.test.ts
git commit -m "feat(gui): 展示模型分段价格"
```

### Task 9：索引/快照迁移、来源优先级和文档验收

**Files:**
- Modify: `src/pricing.rs`（`INDEX_VERSION`、旧索引兼容/失效）
- Modify: `src/modelsdev.rs`、`src/openrouter.rs`（快照版本迁移）
- Modify: `CLAUDE.md`（费用语义简述，如有过时描述）
- Modify: `docs/stats-semantics.md`（新增分段计价与 breakdown 口径）
- Modify: `docs/plans/d5-acceptance-checklist.md`
- Modify: this plan（执行记录和验收结果）

**Step 1: 写失败测试**

构造旧 v1/v2/v3 扁平索引、旧 models.dev/OpenRouter 快照和新分段索引，断言：旧索引在需要新字段时失效重建；旧快照可以作为无分段基础价离线使用；新索引恢复后仍保留 segments；来源冲突不会出现跨 provider 拼价。

**Step 2: 实现迁移**

递增索引版本并让旧索引走已有的原子重建流程。快照读取保持向后兼容，但新同步写入新版本。更新模板和统计语义文档，明确：

- models.dev 是主源，OpenRouter 是补充源，外置最高优先；
- 计价依据是请求实际 prompt token，不是模型允许的最大上下文；
- 费用 tooltip 显示的是当前价格快照下的估算过程；
- unknown 分项不会被当成免费。

**Step 3: 运行测试确认通过**

```powershell
cargo test pricing_index -- --nocapture
cargo test modelsdev -- --nocapture
cargo test openrouter -- --nocapture
cargo test report -- --nocapture
```

**Step 4: 提交**

```powershell
git add src/pricing.rs src/modelsdev.rs src/openrouter.rs CLAUDE.md docs/stats-semantics.md docs/plans/d5-acceptance-checklist.md docs/plans/active/2026-10-06-tiered-pricing-and-request-breakdown.md
git commit -m "docs(定价): 记录分段计价和请求明细口径"
```

### Task 10：完整门禁和真实 fixture 验收

**Files:**
- Modify: this plan（记录命令、结果、遗留限制）
- Test: `src/modelsdev.rs`、`src/openrouter.rs`、`src/pricing.rs`、`src/report.rs`、`frontend/src/components/EventTable.test.ts`、`frontend/src/views/Settings.test.ts`

**Step 1: 运行 Rust 门禁**

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo test --manifest-path src-tauri/Cargo.toml
```

**Step 2: 运行前端门禁**

```powershell
pnpm --dir frontend test
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend build
```

**Step 3: 执行定向场景**

使用合成 fixture 验证：models.dev 单档、多档和旧字段；OpenRouter 多 override；外置覆盖；低于/等于/超过阈值；缓存写入/读取；部分缺价；未知模型；来源冲突；旧索引；请求 tooltip。测试结果必须同时核对聚合总价与逐请求明细求和。

**Step 4: 记录限制**

若真实在线数据中出现未识别的 tier type、边界语义冲突或新的价格字段，记录为 warning/unknown 并在计划执行记录中列明，不通过猜测价格来“修复”验收。

## 验收标准

- [ ] 一个模型可以保存基础价和至少三段上下文价格，匹配边界无重叠、无 off-by-one。
- [ ] models.dev `tiers`、`context_over_200k` 和 OpenRouter `overrides` 能保存到本地快照并在离线模式下参与计价。
- [ ] 外置 TOML 兼容旧 flat 写法，并支持多个分段和四类分项单价。
- [ ] 价格来源、模型前缀和分段规则不会跨 provider 混用；无价格时仍为 unknown。
- [ ] 聚合金额、请求 `cost_usd` 和 breakdown 总价使用同一计算结果。
- [ ] 每条请求费用悬浮提示能解释来源、basis、命中档位、token 数、单价和分项小计；部分缺价明确显示未计价 token。
- [ ] 设置页能查看模型的基础价格与全部分段价格。
- [ ] 旧快照/索引迁移、Rust/Tauri/前端门禁和定向边界测试全部通过。
- [ ] 没有引入编译期内置价格，也没有在计算期间联网。

## 执行记录

实现完成后按 Task 逐项填写提交号、定向测试和完整门禁结果；在真实数据验收未完成前保持本计划位于 `docs/plans/active/`，不得提前归档。
