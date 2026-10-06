# Tiered Pricing and Request Cost Breakdown Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将短上下文/长上下文等分段价格抽象成可复用的价格计划，支持多个阈值、每档多类单价和不同的大小条件，并让每条请求在费用悬浮提示中展示实际的价格来源、命中档位和计算明细。

**Architecture:** 在现有 `Pricing` 查找结果中加入通用 `PricePlan`：一个基础价格加零个或多个有界分段。模型匹配保留完整原始标识，但以最后一个 `/` 后的模型名作为候选键：先完整匹配，再按有边界的前缀匹配；所有候选都用本次请求的 token 数计算费用，选择总费用最高的候选作为保守估算。当前落地的官方规则采用“整笔请求切换档位”，输入、输出、缓存写入和缓存读取都使用命中档位中对应的单价，缺失分项沿用同一价格计划的基础价格，基础价格也缺失时才记为 unknown。models.dev、OpenRouter 和外置 TOML 都保留原始模型键、渠道和来源，聚合与明细复用同一个 `CostEstimate` 和 breakdown，避免两套计价逻辑漂移。

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
- Sub2API 的部分代码仍有编译期 fallback 价格；TokenScope 不恢复内置价格，而是对每个候选渠道按本次请求的实际条件计算费用并取最高者，仍无候选即 unknown。
- 当前官方数据表达的是整笔请求切换价格档位。计划实现 `whole_request`，为将来的 `marginal` 留出类型扩展点，但不会在没有来源明确定义时擅自把一笔请求拆成边际区间。

## 价格身份与选择策略

`nano-gpt/qwen/qwen3.8-27b-obliterated:thinking` 这类标识保留原始值，并提取最后一个 `/` 后的模型名作为候选键：

```text
route/channel: nano-gpt/qwen
model leaf: qwen3.8-27b-obliterated
variant: thinking
raw id: nano-gpt/qwen/qwen3.8-27b-obliterated:thinking
```

匹配规则：

1. 从所有价格条目中提取最后一个 `/` 后的模型名，保留原始完整键用于展示；
2. 先尝试包含 variant 的完整匹配；没有结果时再尝试不含 variant 的完整匹配，并在 breakdown 标记 variant fallback；
3. 完整匹配没有结果时，按有边界的前缀匹配，例如 `qwen3.8-27b` 可以匹配 `qwen3.8-27b-instruct`，但不能匹配 `qwen3.8-27b2`；
4. 对每个匹配候选使用本次请求的时间、input/output/cache token 和分段规则，计算该候选当前条件下的总费用；
5. 选择总费用最高的候选。总费用相同时按来源优先级、完整匹配优先、前缀更长优先、原始键字典序作为稳定 tie-breaker；
6. tooltip 显示最终选中的原始模型键、渠道/来源、时间档、输入档、匹配方式、四类单价、分项小计和总价。

“最贵”按本次请求实际时间和 token 条件下的总费用定义，而不是比较某一个 input 单价；否则会出现 input 更贵但 output 更便宜时无法确定的情况。峰谷规则按请求时间判断，分段规则按请求的 prompt token 判断；只比较当前请求可能命中的条件，不把不适用的更高档位强行套到小请求上。这个策略是保守估算，不代表已识别出真实请求渠道；tooltip 必须明确显示“按候选中最高费用估算”。

## 范围外

- 不增加任何编译期模型价格表。
- 统计管线不联网；计算只读取当前已加载的价格索引/离线快照。
- 不根据模型最大上下文窗口推导价格；只有价格来源明确给出的分段规则才参与计价。
- 不改变已有四类 token 和 unknown token 语义；价格来源继续保留三种数据源，但候选选择改为“完整/前缀匹配后取本次请求最高费用”。
- 不在本计划中实现图片像素、音频时长等非 token 计量；数据模型保留可扩展的 `basis` 枚举，但当前只启用 token 依据。

## 必须保持的不变量

1. 价格选择先按完整/前缀匹配产生候选，再按本次请求的时间、输入大小和 token 总费用选择最高候选；同一个最终命中条目的基础价格、时间档和分段价格来自同一来源，禁止把不同候选的分项价格拼成一张卡。
2. `prompt_tokens` 的规范值为 `input + cache_write + cache_read`；`output` 不参与当前上下文档位选择。
3. 规范区间为左闭右开 `[min_tokens, max_tokens)`；`max_tokens = None` 表示无上限；同一价格计划的区间不得重叠或出现空洞后被错误猜测。
4. models.dev 的 `tier.type = context` 且 `size = S` 表示短档覆盖 `prompt_tokens <= S`，高档从 `S + 1` 开始；OpenRouter 的 `min_prompt_tokens = S` 按上游字段语义从 `S`（包含）开始。两个来源的相同数字不能共用未经转换的边界。
5. 分段缺少某个价格字段时，先回退到同一条目的基础价格；基础价格也缺失时，该分项 token 进入 `unknown`，不得按 0 计费。显式 0 仍表示免费。
6. 无匹配分段且没有合法基础价格时，整条请求的缺价原因必须可见；不得自动选最近档位。
7. 请求级 breakdown 与聚合总价必须调用同一个估算结果；前端不能自行重算费用。
8. 所有价格都在 Rust 内部统一为 USD/百万 token；OpenRouter 的 USD/token 在快照导入时转换，悬浮提示明确显示原始模型键、渠道/来源、时间/输入档和单位，并说明采用了候选最高费用。
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
    schedules: Vec<PriceSchedule>,
}

PriceSegment {
    label: Option<String>,
    min_tokens: u64,
    max_tokens: Option<u64>,
    prices: PriceRates, // 每项 Option<f64>，None 表示沿用 base
}

PriceSchedule {
    label: Option<String>,
    timezone: Option<String>,
    periods: Vec<TimePeriod>,
    prices: PriceRates,
    segments: Vec<PriceSegment>,
}
```

当前来源统一使用 `basis = PromptTokens` 和 `application = WholeRequest`。`InputTokens` 等枚举值用于后续复用，若外置文件声明当前版本不支持的 basis/application，加载时给出诊断并跳过该规则，不能静默改变计价含义。

对一条请求：

```text
prompt_tokens = input + cache_write + cache_read
segment = 唯一满足 min_tokens <= prompt_tokens < max_tokens 的分段
schedule = 当前请求时间命中的时间档；无时间规则时使用默认档
rate(component) = segment.prices.component ?? schedule.prices.component ?? plan.base.component
subtotal(component) = component_tokens * rate(component) / 1_000_000
cost = 所有有单价分项 subtotal 之和
unknown = 所有没有最终单价的分项 token
```

`CostEstimate` 增加 `basis_value`、`basis`、`source`、原始模型键、命中时间档/分段标签、分项明细和 unknown 原因。由于 breakdown 含 `Vec`，`CostEstimate` 从 `Copy` 改为 `Clone`，并同步所有调用点。

## 执行顺序

先完成 Task 1 的模型和边界不变量，再完成 Task 2A 的候选匹配与最高费用选择，随后按 Task 2–5 接入来源和计算器；Task 6–8 接入 report/UI；Task 9 做迁移、文档和完整验收。每个 Task 独立提交，定向测试通过后再进入下一项。

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

### Task 2A：按模型末段匹配并选择最高费用候选

**Files:**
- Modify: `src/model.rs`（如需新增模型末段/variant helper，保留原始 `model` 字符串）
- Modify: `src/pricing.rs`（候选索引、完整/前缀匹配、候选费用比较和来源 DTO）
- Modify: `src/modelsdev.rs`（快照保留原始 `provider/model` 键）
- Modify: `src/openrouter.rs`（快照保留完整渠道模型键）
- Test: `src/pricing.rs`、`src/modelsdev.rs`、`src/openrouter.rs`

**Step 1: 写失败测试**

加入以下 fixture：

```text
nano-gpt/qwen/qwen3.8-27b-obliterated:thinking
other-channel/qwen/qwen3.8-27b-obliterated:thinking
```

验证：

- 两个渠道的末段模型名完整相同，候选都能被找到；
- 完整模型名优先于前缀模型名；
- 没有完整匹配时，只接受有边界的前缀匹配；
- 每个候选按请求时间、输入大小和四类 token 计算总价，最终选择最高费用候选；
- `:thinking` 先尝试 variant 完整匹配，再按明确的 variant fallback 规则处理；
- 选中的原始模型键、来源、渠道、时间档、分段和单价可序列化。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing::tests::test_pricing_candidate_selection -- --nocapture
```

预期：当前实现没有候选列表和原始模型键，无法按末段匹配并选择最高费用候选。

**Step 3: 实现候选查价**

保留 `UsageEvent.model` 的原始值，在 pricing 层建立按末段模型名索引的候选列表：

- 末段提取只影响匹配键，原始完整键、来源和渠道元数据必须保留；
- 完整匹配分为“含 variant”和“无 variant”两次尝试；
- 前缀匹配必须检查 token 边界，不能让 `gpt-5` 命中 `gpt-50`；
- 每个候选都调用同一个 `estimate_with_entry`，使用当前请求时间、token 和分段规则得到总费用；
- 选择费用最高者，按“来源优先级 > 完整匹配 > 前缀长度 > 原始键”稳定打破并列；
- 不从不同候选逐项拼价，所有四类价格必须来自最终同一个候选；
- 无候选时保持 unknown。

索引版本必须递增以包含原始键和候选元数据；旧索引走重建，不从旧的单一归一化键推断渠道。`PricingEntry`、日志和 breakdown 必须标记实际匹配键及“最高候选”选择原因。

**Step 4: 运行测试确认通过**

```powershell
cargo test pricing::tests::test_pricing_candidate_selection -- --nocapture
cargo test pricing -- --nocapture
cargo test modelsdev -- --nocapture
cargo test openrouter -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/model.rs src/pricing.rs src/modelsdev.rs src/openrouter.rs
git commit -m "fix(定价): 按条件选择最高费用候选"
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

保留 OpenRouter override 的完整原始键和渠道元数据；它与 models.dev 条目作为独立候选进入估算，不能在加载阶段互相覆盖或拼接。最终由请求条件下的候选总费用裁决。

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

新增 `[[model.segment]]`、`[[model.schedule]]` 和 `[[model.schedule.period]]` 反序列化结构和模板注释，四类价格字段保持可选以区分“未填写”和显式 0。时间规则至少包含 `timezone`、`start_time`、`end_time`、可选 weekday 限制和价格覆盖；旧写法自动转换为无分段、无时间表的 `PricePlan`。对无效条目保留现有“外置文件错误不静默回退”的诊断语义，避免用户以为自定义价格已生效。

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

### Task 4A：支持峰谷时间规则并纳入最高费用选择

**Files:**
- Modify: `src/pricing.rs`（`PriceSchedule`、时间区间匹配、候选最坏/当前条件费用）
- Modify: `src/model.rs`（如需提供请求时间的统一 helper）
- Modify: `src/report.rs`（向 estimate 传入事件时间；不得使用当前墙上时钟替代历史事件时间）
- Test: `src/pricing.rs`、`src/report.rs`

**Step 1: 写失败测试**

覆盖以下候选：

- 渠道 A 无时间规则，固定价格；
- 渠道 B 工作日高峰/非高峰两个价格；
- 同一请求时间处于渠道 B 高峰时，B 的总费用高于 A，选择 B；
- 同一请求时间处于非高峰时，A 更贵则选择 A；
- 同一渠道时间段重叠时，选择该渠道当前条件下更贵的完整规则；
- 历史事件使用事件时间和声明时区计算，不能读取运行时当前时间。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing::tests::test_pricing_time_condition_candidate -- --nocapture
```

预期：当前价格计划没有时间条件，也没有按请求时间比较候选的能力。

**Step 3: 实现时间条件**

为价格计划增加可选时间表：时区、星期限制、左闭右开时间段、价格覆盖和可选上下文分段。对每个候选：

- 用事件 timestamp 转换到规则时区；
- 找到当前请求可适用的时间规则；
- 若多个规则重叠，分别计算并取该候选的最高总价；
- 没有时间规则时使用基础价格；
- 再把候选总价交给全局最高费用选择器。

时间规则解析失败、时区非法或区间重叠无法确定时，记录 warning 并跳过该规则，不猜测价格。breakdown 必须记录时间档标签、时区和实际使用的事件时间。

**Step 4: 运行测试确认通过**

```powershell
cargo test pricing::tests::test_pricing_time_condition_candidate -- --nocapture
cargo test report::tests::test_event_pricing_uses_event_timestamp -- --nocapture
```

**Step 5: 提交**

```powershell
git add src/pricing.rs src/model.rs src/report.rs
git commit -m "feat(定价): 按请求时间选择峰谷最高费用"
```

### Task 5：实现统一估算器和请求级 breakdown

**Files:**
- Modify: `src/pricing.rs`（`estimate`、分项计算 DTO）
- Modify: `src/aggregate.rs`（复用估算结果）
- Test: `src/pricing.rs`、`src/aggregate.rs`

**Step 1: 写失败测试**

使用多个渠道候选、一条基础价 + `272001` 高档和峰谷时间表的 fixture，覆盖：

- 低于阈值、恰好等于阈值、刚超过阈值；
- input、output、cache_write、cache_read 同时存在；
- 分段只覆盖部分价格字段；
- 同一模型不同渠道的固定价、峰价和分段价比较；
- 当前事件时间落在不同峰谷档时，最高候选随条件变化；
- 完全缺价和部分缺价；
- 同一请求被估算两次时 breakdown 与总价完全相同。

测试断言每一项的 token 数、单位单价、USD 小计、总价、来源、basis 值、命中档位和 unknown token 均正确。示例公式使用 `tokens * usd_per_million / 1_000_000`，不要用展示层四舍五入值参与计算。

**Step 2: 运行测试确认失败**

```powershell
cargo test pricing::tests::test_pricing_cost_breakdown -- --nocapture
```

**Step 3: 实现统一计算**

让 `Pricing::estimate(model, counts, request_at)`：

1. 提取末段模型名并生成完整/前缀候选；
2. 对每个候选使用 `request_at`、`basis_value = counts.input + counts.cache_write + counts.cache_read` 和其时间/分段规则得到 effective rates；
3. 为每个候选的四类 token 生成 `CostLine { kind, tokens, unit_price, subtotal, priced }`；
4. 分别累加候选总价，选择当前条件下总费用最高的完整候选；
5. 记录最终候选的 unknown token 和缺价原因；
6. 返回 `CostEstimate`，保留现有 `cost/complete/unknown` 字段兼容调用方。

聚合层继续按事件估算再累加，并传入事件 timestamp；如同一事件需要明细和聚合同时使用，使用同一 `CostEstimate`，不要在 aggregate 和 report 各写一套公式。

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
- 原始完整模型键、渠道、匹配方式和“候选中最高费用”说明；
- 请求时间、时间档及时区；
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

断言设置页的一个模型条目能显示原始模型键、渠道/来源、基础价、时间档、计价依据和所有分段的范围及四类单价；没有分段或时间规则的普通模型仍保持现有表格布局；缺失分项显示“未知”，显式 0 显示 `$0`。

**Step 2: 实现 DTO 和 UI**

在 `PricingEntry` 中增加原始键、渠道/来源、`basis`、`segments`、`schedules`、`has_tiered_pricing` 等只读字段。设置页使用现有 `NTooltip` 展开档位和峰谷规则，不改变价格编辑文件的打开/保存行为。来源标签继续显示外置、models.dev、OpenRouter，不新增内置来源。

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

### Task 9：索引/快照迁移、候选选择和文档验收

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

- models.dev、OpenRouter 和外置条目都进入候选池；最终按请求时间、输入大小和 token 条件下的最高总费用选择；
- 计价依据是请求实际 prompt token，不是模型允许的最大上下文；时间规则使用事件 timestamp 和规则时区；
- 费用 tooltip 显示原始模型键、渠道、时间档、分段、单价和“候选中最高费用估算”过程；
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

使用合成 fixture 验证：models.dev 单档、多档和旧字段；OpenRouter 多 override；外置覆盖；低于/等于/超过阈值；峰谷时间；不同渠道同模型候选；缓存写入/读取；部分缺价；未知模型；来源冲突；旧索引；请求 tooltip。测试结果必须同时核对聚合总价与逐请求明细求和。

**Step 4: 记录限制**

若真实在线数据中出现未识别的 tier type、边界语义冲突或新的价格字段，记录为 warning/unknown 并在计划执行记录中列明，不通过猜测价格来“修复”验收。

## 验收标准

- [ ] 一个模型可以保存基础价和至少三段上下文价格，匹配边界无重叠、无 off-by-one。
- [ ] models.dev `tiers`、`context_over_200k` 和 OpenRouter `overrides` 能保存到本地快照并在离线模式下参与计价。
- [ ] 外置 TOML 兼容旧 flat 写法，并支持多个分段和四类分项单价。
- [ ] 同一模型的不同渠道价格保留为独立候选；按请求时间、输入大小和 token 条件计算后选择最高总费用，无价格时仍为 unknown。
- [ ] 聚合金额、请求 `cost_usd` 和 breakdown 总价使用同一计算结果。
- [ ] 每条请求费用悬浮提示能解释原始模型键、渠道、时间档、basis、命中分段、token 数、单价和分项小计；部分缺价明确显示未计价 token，并标注候选最高费用估算。
- [ ] 设置页能查看模型的基础价格、峰谷规则和全部分段价格。
- [ ] 旧快照/索引迁移、Rust/Tauri/前端门禁和定向边界测试全部通过。
- [ ] 没有引入编译期内置价格，也没有在计算期间联网。

## 执行记录

实现完成后按 Task 逐项填写提交号、定向测试和完整门禁结果；在真实数据验收未完成前保持本计划位于 `docs/plans/active/`，不得提前归档。

| Task | 内容 | 提交 | 定向测试 |
| --- | --- | --- | --- |
| 1 | 通用 PricePlan/PriceSegment/PriceRates + `[min,max)` 边界匹配与校验（重叠/反向/空洞/无上限段后置拒绝；无基础价且未命中分段 → unknown 不按 0） | fe1e0e1 | `pricing::tests::test_pricing_segment*`（5 项）+ 全库 107→112 通过 |
| 2A | 末段模型名候选匹配（完整 → variant 回退 → 有边界前缀 → 前缀回退）+ 按请求条件取最高费用候选；`MatchedCandidate` 可序列化；索引 v4 | 3d98248 | `test_pricing_candidate_selection` + pricing/modelsdev/openrouter 过滤全绿 |
| 2 | models.dev `tiers`（context）与 `context_over_200k` → 规范分段（size S → min=S+1）；快照 v3；重复/冲突/非法 size 跳过并 warning | ee451d7 | `test_modelsdev_tier` + `test_modelsdev_tier_conversion_rules` + `test_pricing_modelsdev_tier` |
| 3 | OpenRouter `pricing.overrides`（`min_prompt_tokens` inclusive）→ 分段 ×1e6；快照 v2；乱序升序/同阈值保留首个/负价与时间条件跳过 | 675a181 | `test_openrouter_override` + `test_pricing_openrouter_override` |
| 4 | 外置 TOML `[[model.segment]]` 与可选 `basis`/`application`（非法值显式拒绝）；四类价格 Option 化（缺键=未知 ≠ 显式 0）；模板更新 | b17f004 | `test_pricing_external_segment` + external 过滤全绿 |
| 4A | `PriceSchedule`/`SchedulePeriod` 峰谷时间规则（时区/HH:MM 窗口/星期限制/规则级价格）；`estimate(model, t, at)` 按事件时间裁决；同候选多规则取最贵完整规则；`request_at`/时间档入 breakdown | 95031fd | `test_pricing_time_condition_candidate` + `test_event_pricing_uses_event_timestamp` |
| 5 | 统一估算器输出 `CostLine` 四行明细 + `basis`/`basis_value`/`segment_label`；聚合/明细同一估算结果；纯函数可重复（两次估算相等） | c1fc91a | `test_pricing_cost_breakdown` + aggregate/pricing 过滤全绿 |
| 6 | `EventRow.cost_breakdown`（复用 pricing DTO，不泄露内部类型）；`list_events` 每事件一次 estimate 同产 `cost_usd` 与明细；分页/过滤不变 | fd8ce3e | `test_event_cost_breakdown` + report 过滤 26 项全绿 |
| 7 | 前端 `EventRow`/breakdown DTO；`formatCostBreakdown` 纯函数（来源/渠道/匹配方式/候选最高费用说明、prompt 度量式、档位、分项、unknown 行）；费用列 NTooltip | 018cf6e | EventTable.test.ts 10 项 + 前端 54 项/typecheck/format 全绿 |
| 8 | `PricingEntry` 携带 channel/basis/segments/schedules/has_tiered_pricing，四价 Option 化；设置页"分段"标记 + 档位悬浮（lib/tieredPrice 纯函数） | 76f2735 | `test_pricing_entries_segments` + Settings.test.ts 8 项 + 前端 58 项/build 全绿 |
| 9 | 索引迁移（v3 扁平索引按版本失效重建）、旧快照离线基础价、新索引保留分段、跨来源不拼价；CLAUDE.md / stats-semantics §4 / D5 清单 4.9–4.12 | （本提交） | `test_pricing_index_migration` + pricing_index/modelsdev/openrouter/report 过滤全绿 |

### 已知限制（如实记录，不猜测价格）

- OpenRouter 带 `utc_start`/`utc_end` 的时间条件 override 未接入时间引擎：同步时跳过并记 warning（Task 4A 范围为外置时间规则）。
- 计价依据当前仅启用 `prompt_tokens` 与 `whole_request`；外置文件声明其他 basis/application 时显式拒绝该条目（枚举为将来扩展保留）。
- 真实在线数据的验收（4.9–4.12 与 D5 外部安装批次）尚未执行，计划保持 active；发现未识别 tier type / 新价格字段时按 warning + unknown 处理，不猜测。
