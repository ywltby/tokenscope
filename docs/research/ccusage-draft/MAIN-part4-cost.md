## 9. 金额计算：公式、模式与缺失价格

### 9.1 三种 cost mode

`CostMode`（`rust/crates/ccusage-cli/src/types.rs`）在 `calculate_cost_for_usage_at`
（`rust/crates/ccusage-core/src/cost.rs:31-46`）里只有三行逻辑：

```rust
match mode {
    CostMode::Display   => cost_usd.unwrap_or(0.0),   // 只用日志自带的 costUSD
    CostMode::Auto      => cost_usd.unwrap_or_else(|| calculate_cost_from_tokens(...)),
    CostMode::Calculate => calculate_cost_from_tokens(...), // 永远自算，忽略 costUSD
}
```

注意 `Display` 模式**不加载价格表**：Claude 链在装载时就直接跳过
`PricingMap::load_with_overrides`（`claude/src/lib.rs:144-152`），
省掉解析开销也省掉网络。`Auto` 是默认值，语义是「日志给了就用，没给就算」。
`calculate_cost_from_tokens`（`cost.rs:93-116`）在模型名为 `None` 或缺价格时返回 `0.0`，
并按 `speed == Fast` 决定是否乘 `fast_multiplier`。

### 9.2 Claude 链公式

`calculate_cost_from_pricing`（`cost.rs:118-199`）先把缓存创建拆成 5 分钟 / 1 小时两档：

```rust
let (cache_create_5m_tokens, cache_create_1h_tokens) = if let Some(breakdown) = usage.cache_creation {
    (breakdown.ephemeral_5m_input_tokens, breakdown.ephemeral_1h_input_tokens)
} else {
    (usage.cache_creation_input_tokens, 0)     // 无明细 → 全按 5 分钟档
};
let cache_create_1h_cost = pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER; // = 2.0
```

然后分两条互斥路径：

**路径 A：模型带 `long_context_threshold`（来自 models.dev 的 `cost.tiers`，如 OpenAI 272K）**
——阈值判定用**整个请求的上下文总量**，且一旦判为长上下文，**所有桶整体切到高档价**（不是边际）：

```rust
let context_tokens = input + cache_read + cache_create_5m + cache_create_1h; // 全饱和加
let long_context = context_tokens > threshold;
// 每个桶：long_context ? above_rate.unwrap_or(base) : base
```

**路径 B：普通模型**——用 `tiered_cost`（`cost.rs:201-211`）的**边际语义**，阈值
`DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS = 200_000`（`pricing.rs:220`）：

```rust
pub fn tiered_cost(tokens: u64, base: f64, above: Option<f64>, threshold: u64) -> f64 {
    if tokens == 0 { return 0.0; }
    if let Some(above) = above && tokens > threshold {
        return (threshold as f64 * base) + ((tokens - threshold) as f64 * above);
    }
    tokens as f64 * base
}
```

总式（两路共同的分项结构，五桶相加）：

```
cost = f(input) + f(output) + f(cache_create_5m) + f(cache_create_1h) + f(cache_read)
```

其中 1 小时缓存创建的基价是 `input` 价的 2 倍（`CACHE_CREATE_1H_INPUT_MULTIPLIER = 2.0`，`cost.rs:7`），
高档价同样乘 2（`cost.rs:128-131`）。Fast 倍率在**总额**上相乘（`cost.rs:110-115`）。
两条路径有单测钉住：`prices_cache_creation_breakdown_by_duration`（55.5 = 10×1+20×2+30×0.1+…，`cost.rs:317-338`）、
`prices_two_stage_model_as_whole_request_at_long_context_rates`（`cost.rs:358+`）。

### 9.3 Codex 链公式（与 Claude 不同）

Codex 的 token 计数来自累计事件，且**同一请求内可能混有不同 service tier 与长上下文桶**，
所以计价在聚合层完成（`adapters/codex/src/report.rs`）：

- 输入拆分（`report.rs:123-129`）：`non_cached_input = input - cached - cache_creation`（全饱和减），
  即 JSON 里的 `inputTokens` 是**不含缓存**的净输入；
- 桶成本 `calculate_codex_bucket_cost`（`report.rs:343-392`）按「短上下文桶 + 长上下文桶」分别定价：

```rust
let cache_read = if pricing.cache_read_explicit { pricing.cache_read } else { pricing.input };
// 长上下文：OpenAI 对超阈值请求的**每个 token** 都按 long-context 价计，
// 因此把聚合用量拆成两个独立桶
let long_input = usage.long_context_input_tokens.min(usage.input_tokens);
...（各长桶都用 min 链做上界保护，避免坏数据把桶加爆）
cost = short_non_cached*input + short_cached*cache_read + short_cache_creation*cache_creation
     + (output - long_output)*output_price
     + long_non_cached*long_input_rate + long_cached*long_cache_read
     + long_cache_creation*long_cache_creation_rate + long_output*long_output_rate
```

两个必须记住的差异点：

1. **cache_read 的兜底价不同**：Claude 链在缺省时用 `input * 0.1`
   （加载期推导，`pricing.rs:1088`）；Codex 链在 `cache_read_explicit == false` 时
   **直接用 `input` 价**（`report.rs:344-348`）——即「价格表没写明缓存折扣就别猜」。
2. **长上下文的计价语义不同**：Claude 链（带 threshold 的模型）是「整请求切换」，
   Codex 链是「按桶拆开、各自计价」。同一份价格数据，两条链路解释不同。

tier 加价按 §8.5 的桶加权（`report.rs:234-275`），`CodexSpeedPolicy::Forced`
直接指定整个请求的 tier，`Auto` 则用「记录里标记过的 tier 桶 + 未分类桶」补齐。

### 9.4 缺失价格：留痕而不是静默算 0

缺失判定 `missing_pricing_model_for_usage`（`cost.rs:48-59`）：

```rust
if mode == CostMode::Display || (mode == CostMode::Auto && cost_usd.is_some()) { return None; }
missing_pricing_model_for_token_total(model, total_usage_tokens(usage), pricing)
```

即：**token 总数为 0 不报**、**Display 模式不报**、**Auto 且已有 costUSD 不报**；
其余情况查表失败就把模型名（经 `resolve_model_name` 归一）记进
`LoadedEntry.missing_pricing_model`（`types.rs:138`），最终由
`print_missing_pricing_warnings`（`ccusage-core/src/output.rs`）汇总打印。
`ModelBreakdown.missing_pricing`（`types.rs:110-111`）则是 JSON 里的对应标记，
只在为 true 时序列化。有测试 `zero_rate_pricing_is_not_missing_pricing`
（`cost.rs:217-237`）确认**价格为 0 的模型不算缺失**——「未知」与「免费」被刻意区分开。

### 9.5 金额的输出形态

- `format_currency`（`ccusage-core/src/output.rs`）：终端表格用，规则见 §10；
- `json_float(value)`：JSON 输出统一走它，保证浮点序列化稳定；
- 每个模型一行 `ModelBreakdown`（`types.rs:100-122`）同时带四桶 token 与 `cost`；
  日/月/周/会话行再对模型明细求和得到 `total_cost`。
