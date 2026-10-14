## 8. 定价数据的来源、加载与匹配链

### 8.1 三份价格数据

| 来源 | 载体 | 生效时机 | 备注 |
| --- | --- | --- | --- |
| LiteLLM 快照（主表） | 编译期内嵌 `litellm-pricing.json.deflate` | 永远 | 由 `build.rs` 生成，**经过裁剪与改名**（见下） |
| models.dev 目录 | 编译期内嵌 `models-dev-pricing.json`（544 555 字节）+ `models-dev-catalog-rules.json`（14 569 字节） | 内嵌版离线可用；在线版需联网 | 用于补 LiteLLM 缺失的模型、长上下文档位、`exactOnly` 裁决 |
| 用户覆盖 | `PricingOverride`（配置文件/CLI） | 最后生效 | 见 8.4 |

在线刷新地址（**已验证**，`pricing.rs:55-58`）：

```rust
const LITELLM_PRICING_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";
const MODELS_DEV_FAILURE_RETRY_AFTER: Duration = Duration::from_secs(60);
```

### 8.2 编译期内嵌：裁剪 + 改名 + deflate

`build.rs`（`rust/crates/ccusage-core/build.rs`）做三件事：

1. **取源**：优先读环境变量 `CCUSAGE_PRICING_JSON_PATH` 指向的
   `model_prices_and_context_window.json`（Nix 构建即如此注入，`build.rs:13,22-25`）；
   否则在 `fetch-litellm-pricing` feature 下按 `flake.lock` 里 `nodes.litellm.locked`
   的 `owner/repo/rev` 拼 raw URL 下载（`build.rs:107-136`，10 秒超时，`build.rs:15`）。
   该 feature **默认关闭**——注释明确说 rustls 是全 workspace 最贵的构建依赖（`build.rs:68-74`）。
2. **裁剪**：`compact_pricing_json`（`build.rs:155-201`）只保留 `is_embedded_model`
   命中的键并对字段改名，映射表（`build.rs:168-178`）：

   | LiteLLM 字段 | 内嵌键 |
   | --- | --- |
   | `input_cost_per_token` | `i` |
   | `output_cost_per_token` | `o` |
   | `cache_creation_input_token_cost` | `cc` |
   | `cache_read_input_token_cost` | `cr` |
   | `*_above_200k_tokens`（4 个） | `ia` / `oa` / `cca` / `cra` |
   | `max_input_tokens` | `ctx` |
   | `provider_specific_entry.fast` / `.flex` | `fast` / `flex` |

   `is_embedded_model` 的白名单（`build.rs:203-221`）：`claude-`、`anthropic.`、
   `anthropic/`、`us./eu./global./jp./au.anthropic.`、`gpt-`、`o3`、`o3-`、`o4-mini`、
   `o4-mini-`、`openai/`、`azure/`、`zai/`、`openrouter/openai/`。
   **没有 `i` 或没有 `o` 的条目直接丢弃**（`build.rs:196-198`）——
   所以内嵌表的覆盖范围就是「Anthropic + OpenAI 家族」；其余厂商要么命中内嵌 models.dev 快照，
   要么必须联网。
3. **压缩**：`miniz_oxide::deflate::compress_to_vec(bytes, 10)`（最高压缩级别，仅构建期付代价），
   运行期首次使用时 `decompress_to_vec` 解压一次并缓存在 `OnceLock<String>`（`pricing.rs:31-54`）。

`parse_litellm_pricing`（`pricing.rs:2596-2626`）反过来兼容两种输入：先试短键
（对象含 `i` 与 `o` 时走 `CompactLiteLlmPricing`），失败再试原生长键结构；
两条路径都要求 input/output **同时存在**。

### 8.3 加载顺序与覆盖优先级

`PricingMap::load_with_overrides(offline, log, overrides)`（`pricing.rs:1002-1038`）：

```
1. load_embedded()                       // 内嵌裁剪表
   ├ load_json_with_overrides(裁剪表)     // 逐条解析，写入 entries（pricing.rs:1048-1108）
   ├ put_builtin_pricing(...)            // 少量内置硬编码条目（pricing.rs:2015+）
   ├ fill_long_context_rates_from_models_dev()   // 用内嵌 models.dev 的 cost.tiers 补档位阈值
   └ enable_embedded_models_dev_fallback = true  // 离线也能回落 models.dev 快照
2. 若 !offline：fetch_pricing_json → load_json(在线 LiteLLM 全量)
   └ 失败/解析不出条目 → stderr WARN，继续用内嵌（pricing.rs:1014-1028）
3. fill_long_context_rates_from_models_dev()   // 在线刷新会整条替换条目，需重补
4. enable_models_dev_fallback = !offline       // 在线目录仅在联网时启用（pricing.rs:1035）
5. apply_overrides(overrides)                  // 用户覆盖最后落地（pricing.rs:1036）
```

要点：**在线刷新是「整条替换」而不是字段合并**（注释 `pricing.rs:1031-1033` 明说），
所以第 3 步必须重补 models.dev 的档位；用户覆盖则始终拥有最终解释权（第 5 步）。
离线模式（`--offline`）只影响 2/4 两步，内嵌表照常工作。

### 8.4 默认费率推导与「显式」标记

LiteLLM 条目缺缓存价格时按固定比例推导（`pricing.rs:1085-1088`）：

```rust
cache_create: pricing.cache_creation_input_token_cost.unwrap_or(input * 1.25),
cache_read:   pricing.cache_read_input_token_cost.unwrap_or(input * 0.1),
```

同时用 `cache_create_explicit` / `cache_read_explicit` 记住「该值是发布数据还是推导值」
（`pricing.rs:1069-1070`）。这个标记只在用户覆盖时起作用：覆盖了 input 价而未覆盖缓存价、
且基值本来就是推导值时，按 `new_input / old_input` **等比缩放**缓存价格；
显式值则原样保留（`pricing.rs:1832-1883`，注释把规则写得很清楚）。

### 8.5 service tier 倍率（fast / flex）

倍率的取值优先级（`pricing.rs:1071-1079`）：

```
provider_specific_entry.fast  →  fast-multiplier-overrides.json  →  1.0
provider_specific_entry.flex  →  flex-multiplier-overrides.json  →  1.0
```

两个覆盖文件是内嵌的 `include_str!` 常量（`pricing.rs:28-29`），内容如下（**已验证**，
读取自 `rust/crates/ccusage-core/src/fast-multiplier-overrides.json` 与 `flex-multiplier-overrides.json`）：

| 类型 | 条目 |
| --- | --- |
| fast（精确） | `gpt-5.6-sol` 2.0、`gpt-5.6-terra` 2.0、`gpt-5.6-luna` 2.0、`gpt-5.5` 2.5、`gpt-5.4` 2.0、`gpt-5.3-codex` 2.0、`gpt-6-astra` 2.0、`gpt-6-sol` 2.0、`gpt-6.1-sol` 2.0、`gpt-6-luna` 2.0 |
| fast（归一化前缀） | `claude-opus-4-6` 6.0、`claude-opus-4-7` 6.0、`claude-opus-4-8` 2.0 |
| flex | `gpt-5.x` / `gpt-6.x` / `o3` / `o4-mini` 等 17 条 0.5 |

倍率的**应用点因适配器而异**（这是容易看错的地方）：

- Claude 链：`cost.rs:110-114` 在总额上乘 ——`speed == fast` 时 `cost = 基价 * fast_multiplier`；
- Codex 链：`adapters/codex/src/report.rs:272-274` 按「请求内被标记为该 tier 的 token 桶」加价：

```rust
standard_cost
    + calculate_codex_bucket_cost(&flex_usage, pricing) * (pricing.flex_multiplier - 1.0)
    + calculate_codex_bucket_cost(&fast_usage, pricing) * (pricing.fast_multiplier - 1.0)
```

即 `总价 = 标准价 + fast 桶×(倍率-1) + flex 桶×(倍率-1)`。`flex_multiplier`
在 Claude 侧**没有任何计算用途**（全仓仅 Codex 使用，见 §9.3）。

### 8.6 模型名 → 价格条目的匹配链

入口 `PricingMap::find(model)`（`pricing.rs:1354-1391`）——带**结果缓存**
（`find_cache: OnceLock<Mutex<FxHashMap<String, Option<Pricing>>>>`，
连 `None` 也缓存，避免反复走模糊匹配）。

真正的匹配在 `find_with_fallbacks`（`pricing.rs:1393-1454`），顺序固定：

```
0. 别名解析：resolve_model_name(model)（用户配置 CCUSAGE_MODEL_ALIASES，见 8.7）
1. 精确段（Fuzzy::Denied，不允许模糊）：
     原文 → 别名，二者都要求在下列表中**精确取键**
2. 计算本次查询是否允许模糊（allows_fuzzy_lookup_with_fallbacks, pricing.rs:1664-1683）
3. 模糊段（按优先级）：
     a. 主表（内嵌/在线 LiteLLM）：原文 → 别名
     b. 在线 models.dev 目录（enable_models_dev_fallback）
     c. 内嵌 models.dev 快照（enable_embedded_models_dev_fallback）
   ——a 命中后 b、c 不再尝试
```

单表内的查找 `find_entry_with_fallback`（`pricing.rs:1586-1615`）：

1. `entries.get(model)` 精确命中；
2. `exact_only.id_spelled_by(model)`：若该 id 被标记为「只能精确匹配」，
   则它的**分隔符变体**（`gpt-5.3→gpt-5@3` 之类）仍直接用它（`pricing.rs:1592-1597`）；
3. 若本次查询被判定为 `Fuzzy::Denied` **或** 命中了 exact-only 门控 → 直接返回 `None`
   （即**不允许**用别的模型的价去糊弄，`pricing.rs:1598-1602`）；
4. 否则做全表扫描：`pricing_key_matches(candidate, model, normalized_model)` 过滤，
   再用 `max_by(键长度, 字典序)` 取**最长键**（`pricing.rs:1603-1613`）。

模糊匹配的相等规则 `pricing_key_matches`（`pricing.rs:2667-2695`）：

```rust
fn pricing_key_matches(candidate: &str, model: &str, normalized_model: &str) -> bool {
    if contains_pricing_key(model, candidate) || contains_pricing_key(candidate, model) { return true; }
    let normalized_candidate = normalized_pricing_key(candidate);
    contains_pricing_key(normalized_model, normalized_candidate.as_ref())
        || contains_pricing_key(normalized_candidate.as_ref(), normalized_model)
}
```

- **双向包含**：`claude-3-5-sonnet` 能命中 `claude-3-5-sonnet-20241022`，反之亦然；
- 归一化只替换 `.` 与 `@` 为 `-`（`normalized_pricing_key`，`pricing.rs:2725-2731`），
  大小写**不**归一；
- 边界检查 `is_pricing_key_boundary`：命中位置前后一个字节必须非字母数字（`pricing.rs:2689-2691`），
  避免 `gpt-5` 命中 `xgpt-5y`；
- **版本保护** `suffix_starts_with_numeric_model_version`（`pricing.rs:2703-2722`）：
  候选键以数字结尾、且后缀以 `-` 或 `.` 接数字时，这个后缀若**不是** 8 位日期
  （`MODEL_DATE_SUFFIX_DIGITS = 8`，`pricing.rs:61`）就算作「不同版本」，拒绝该匹配。
  效果：`opus55-20261010` 可回落 `opus55`，但 `opus550` 不会命中 `opus55`。

### 8.7 别名与「只能精确匹配」的门控

- 用户别名：环境变量 `CCUSAGE_MODEL_ALIASES`（`model_aliases.rs:8`），
  支持 JSON 或 `k=v,k2=v2` 文本（`model_aliases.rs:39-61`），
  且 `xxx-fast` 会解析成 `别名映射值 + "-fast"`（`model_aliases.rs:20-24`）。
- 内建别名（硬编码 3 条，`pricing.rs:2793-2800`）：

  | 请求名 | 归到 |
  | --- | --- |
  | `gpt-reserve` | `gpt-5.6-luna` |
  | `gpt-5.6` | `gpt-5.6-sol` |
  | `gpt-5.3-spark` | `gpt-5.3-codex-spark` |

- `exact_only` 集合：由 models.dev 目录规则（`ModelsDevCatalogRules`，`pricing.rs:597-712`）
  决定的「只能精确匹配」id。它的存在意义写在注释里（`pricing.rs:1617-1637`）：
  `claude-opus-5-fast` 若被模糊匹配到 LiteLLM 的 `claude-opus-5`，
  就永远到不了快照里真正的 `-fast` 档位条目，于是要用门控把模糊匹配**挡掉**，
  逼它继续往后查找。
- 目录可信度排序 `rank`（`pricing.rs:649-673`）：`owner`(3) > `platform`(2) > `reseller`(1)，
  同级按 id 字典序，再按 provider key——保证多 catalog 抢同一个 id 时结果**确定性**。

### 8.8 时间相关定价（唯一一处）

`has_time_dependent_pricing(model)`（`pricing.rs:272-277`）+ `deepseek_v4_*` 系列
（`pricing.rs:252-368`）：DeepSeek V4 有峰谷价，按 **UTC 工作日半开窗口** 判定峰时
（`deepseek_v4_peak`，`pricing.rs:333-347`），分界常量
`DEEPSEEK_V4_PRICING_CUTOFF_MS = 1_786_896_000_000`。`find_at(model, timestamp)`
（`pricing.rs:1461-1482`）先做普通查找再套用该时间表，最后再套用户覆盖。
其余模型一律走静态表——即**「价格随时间变化」在全仓只有 DeepSeek V4 一家**。
