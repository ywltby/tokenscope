## S3 定价与金额计算

> 研究对象：ccusage 仓库浅克隆（HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31）。第 1–7 轮为静态阅读；第 8 轮已构建并运行（见 §13.5）。
> 主文件：`rust/crates/ccusage-core/src/pricing.rs`（6181 行；1–2894 逻辑、2895–6181 测试）、`build.rs`（255）、`cost.rs`（664）。行号均为该克隆中的实际行号。

### 0. 结论速览

- 价格来自**四处**：编译期内嵌 LiteLLM 快照（白名单裁剪）、编译期内嵌 models.dev 扁平快照、运行期在线 LiteLLM `main` 原始 JSON、运行期在线 `https://models.dev/api.json`；用户侧没有独立价格文件，只有 config 的 `pricingOverrides`（字段级补丁）与 `CCUSAGE_MODEL_ALIASES`（名字改写）。
- 合并**没有深合并**：LiteLLM 按 key 整条替换，models.dev 只补缺失 id，内置兜底表只 `or_insert`，用户 override 是唯一的字段级合并且永远最后执行。
- 匹配链：精确 → 静态别名 → `.`/`@`→`-` 归一化 → 模糊子串（最长键 + 版本边界保护）→ 在线 models.dev → 内嵌 models.dev，外加 `exact_only` 门禁。
- 计价有两种档位语义：**整单切换**（有 per-model `long_context_threshold`）与**边际分段**（LiteLLM `*_above_200k_tokens`，默认 200K）。1 小时缓存写入按 `input × 2`。注意 Codex 侧的阈值判定只看 `input_tokens` 总量（见 §6.2），与 Claude 链的「上下文总量」口径不同。
- 缺价一律回退 `0.0`，同时用 `missingPricing` / `unpricedModels` / stderr 警告三处披露；显式 0 价不算缺价。

---

### 1. 价格数据来源

#### 1.1 编译期内嵌 LiteLLM 快照

```text
build.rs:17-40
  CCUSAGE_PRICING_JSON_PATH 有值 → 读该文件（:22-25，Nix 构建的唯一通道）
  否则 → fetch_pricing_json()，未开 fetch-litellm-pricing feature 直接 panic（:75-82）
       开启时 minreq 下载，超时 10s（:15, :89-105）
  compact_pricing_json(:155-201) → deflate(:60-62) → $OUT_DIR/litellm-pricing.json.deflate
```

- 下载 URL 由 `flake.lock` 的 `nodes.litellm.locked` 拼成（`build.rs:107-136`）：`https://raw.githubusercontent.com/{owner}/{repo}/{rev}/model_prices_and_context_window.json`。
- 运行期以 `include_bytes!(concat!(env!("OUT_DIR"), "/litellm-pricing.json.deflate"))` 内嵌（`pricing.rs:20-21`），**首次使用**时惰性解压一次（`inflate_snapshot` + `OnceLock`，`pricing.rs:33-44`）。
- 压缩是裸 deflate 最高档 `miniz_oxide::deflate::compress_to_vec(bytes, 10)`（`build.rs:60-62`），解压 `miniz_oxide::inflate::decompress_to_vec`（`pricing.rs:35-37`）。
- 压缩键映射（`build.rs:168-195`）：`input_cost_per_token→i`、`output_cost_per_token→o`、`cache_creation_input_token_cost→cc`、`cache_read_input_token_cost→cr`、`*_above_200k_tokens→ia/oa/cca/cra`、`max_input_tokens→ctx`、`provider_specific_entry.fast/flex→fast/flex`；两个过滤条件：源字段**值为 null 时跳过**（`:182-184`）、`fast`/`flex` 为 null 时不写入（`:190-191`）；最后**只有 `i` 与 `o` 同时存在才保留**（`:196-198`）。注意 `provider_specific_entry` 在**压缩形状**里由 `compact.fast.or(compact.flex)` 重建（`pricing.rs:2612-2618`），因此只有 fast 或只有 flex 的条目也能取到倍率。
- 白名单 `is_embedded_model`（`build.rs:203-221`）：`claude-`、`anthropic.`、`anthropic/`、`us./eu./global./jp./au.anthropic.`、`gpt-`、`o3`、`o3-*`、`o4-mini`、`o4-mini-*`、`openai/`、`azure/`、`zai/`、`openrouter/openai/`。测试 `embedded_build_time_pricing_is_compact` 断言内嵌 JSON < 200 000 字节、无 `"source"`、无 `vertex_ai/`、含 `claude-opus-4-6`（`pricing.rs:5844-5851`）。

当前 pin（`flake.lock:143-145` / `:159-161`）：

| 节点 | owner/repo | rev |
| --- | --- | --- |
| `nodes.litellm` | `BerriAI/litellm` | `d9123f459561754f41fd598a73b405b091d3bf84` |
| `nodes.models-dev` | `anomalyco/models.dev` | `9cb380733df864b6538c7d6a42111d28897901f5` |

#### 1.2 编译期内嵌 models.dev 快照

两份**已提交**生成物：`rust/crates/ccusage-core/src/models-dev-pricing.json`（544 555 B，扁平 `model_id → {cost, limit, exactOnly?}`，同一模型含作者目录与各云平台/转售目录拼写，如 `claude-opus-4-6`(7341)、`anthropic.claude-opus-4-6-v1`(3932)、`us.anthropic.claude-opus-4-6-v1`(30696)）与 `models-dev-catalog-rules.json`（14 569 B，字段 `owners`/`platforms`/`authoredModelIds`/`assetPricedModelIds`，结构见 `pricing.rs:596-617`）。生成流程：`justfile` 的 `gen-models-dev-pricing`（nix 从 `models-dev` input 构建后拷贝）、`update-models-dev-pricing`（先 `nix flake update models-dev`）。`build.rs:46-56` 用 `embed_snapshot` 把二者 minify + deflate 内嵌（原因见 `build.rs:42-45`：仓库保持缩进便于 diff，二进制不应带 2K+ 模型缩进），访问器在 `pricing.rs:46-54`。

#### 1.3 运行期在线抓取

| 端点 | URL | 常量 |
| --- | --- | --- |
| LiteLLM | `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` | `pricing.rs:55-56` |
| models.dev | `https://models.dev/api.json` | `pricing.rs:57` |

HTTP 客户端在二进制侧（`rust/crates/ccusage/src/http.rs`），由 `set_json_fetcher` 注入（`pricing.rs:2872-2884`，安装点 `ccusage/src/main.rs:26`），原因是避免 TLS 栈成为 core 的依赖（`pricing.rs:2876-2881`）。

- 只接受 `PricingEndpoint::for_url` 认得的 URL（`http.rs:25-34`、`pricing.rs:72-78`）。
- ETag 磁盘缓存：`$XDG_CACHE_HOME/ccusage/http-cache/`，否则 `~/.cache/ccusage/http-cache/`（`http.rs:37-52`、`:165-169`），文件名 `{sanitize(url)}-{fnv1a64:x}.cache`（`http.rs:237-257`），内容 `etag\nbody`，写入用临时文件 + rename（`http.rs:199-229`）。
- 请求带 `if-none-match`（`http.rs:80-82`）；`304` 仅在缓存体通过端点校验后才复用，否则作废缓存并**重取一次**（不带 validator）（`http.rs:88-99`）；非 200 直接报错（`:100-102`）。
- 超时 `timeout_global(10s)`（`http.rs:11`、`:75-78`）；体积上限 `64 MiB`（`http.rs:12`、`:104-151`）；新响应必须过 `validates_shape`（`http.rs:121-126`、`pricing.rs:88-98`、`:128-194`）。
- **无重试**：除上面 304 那条路径外不二次请求；传输错误不会复用缓存体（测试 `http.rs:684-695`）。

#### 1.4 用户自定义价格

只有两条路径，**没有**「用户价格文件环境变量」：

1. config 的 `pricingOverrides`：schema `ConfigPricingOverride`（`ccusage-config/src/config_schema.rs:593-607`），使用点 `config_schema.rs:397-398`、`:508-509`；字段（camelCase）`inputCostPerToken`、`outputCostPerToken`、`cacheCreationInputTokenCost`、`cacheReadInputTokenCost`、四个 `…Above200kTokens`、`maxInputTokens`、`fastMultiplier`、`flexMultiplier`。
2. `CCUSAGE_MODEL_ALIASES`（`model_aliases.rs:8`）：把请求名改写到另一价格键（§5.1）。

config 文件发现顺序（`ccusage-config/src/config.rs:246-299`）：`--config <path>` → `<cwd>/.ccusage/ccusage.json` → 各 `CLAUDE_CONFIG_DIR`（逗号分隔）下的 `ccusage.json` → `~/.config/claude/ccusage.json`、`~/.claude/ccusage.json`；取第一个可读且为 JSON 对象的文件（`:246-256`）。合并进 `SharedArgs.pricing_overrides` 是字段级（`:627-681`，测试 `:1049-1086`）。

**证据**：`build.rs:13`、`:17-40`、`:60-62`、`:75-82`、`:107-136`、`:155-221`、`pricing.rs:20-54`、`:55-61`、`:2872-2893`、`http.rs:11-12`、`:25-151`、`:165-257`、`config_schema.rs:593-607`、`config.rs:246-299`、`:627-681`、`model_aliases.rs:8`、文档 `docs/guide/config-files.md:407-464`.

---

### 2. 加载顺序与覆盖规则

两个入口：`load_embedded()`（`pricing.rs:982-1000`，纯离线基线）与 `load_with_overrides(offline, log, overrides)`（`pricing.rs:1002-1038`，生产路径；调用例 `claude/src/lib.rs:144-152`、`pi/src/lib.rs:22-27`、`ccusage-adapter-all/src/loader.rs:83-89`）。

```text
load_embedded():                                  # :982-1000
  1) entries ← 内嵌 LiteLLM（load_json_with_overrides）
  2) put_builtin_pricing()                        # :1980-1983，entry().or_insert()，只补缺键
  3) fill_long_context_rates_from_models_dev()    # :1946-1978
  4) enable_embedded_models_dev_fallback = true   # :998

load_with_overrides(!offline):                    # :1002-1038
  1) load_embedded()
  2) if !offline: 在线 LiteLLM → load_json()      # :1007-1028，键级整条替换
  3) fill_long_context_rates_from_models_dev()    # :1034（在线刷新会整条替换，必须重铺）
  4) enable_models_dev_fallback = !offline        # :1035
  5) apply_overrides(user)                        # :1036-1037，字段级，最后执行
```

| 层 | 合并语义 | 证据 |
| --- | --- | --- |
| 内嵌/在线 LiteLLM → `entries` | 逐键 `insert`，同键整条替换；刷新**只增不删** | `pricing.rs:1048-1108`（`:1080-1100`）、`:1014-1020` |
| 内置兜底表 | 默认 `entry(...).or_insert(pricing)`（只补缺键）；**3 处例外走无条件 `self.entries.insert`**：`gpt-5.1-codex`(`:2369-2370`)、`gpt-5.2-codex`(`:2386-2387`)、`gpt-5.2`(`:2400-2408`)——上游快照已有价时也会被覆盖，与注释 `:2010-2014` 声明的意图相反 | `pricing.rs:1980-1983`、`:2369-2370`、`:2386-2387`、`:2400-2408` |
| models.dev（内嵌/在线） | 只补主表没有的 id；装载内部按信任等级与详细度整条替换，替换时**删除落败拼写的旧条目**并解绑其 context limit | `pricing.rs:1241-1243`、`:1259-1284`（删除 `:1277-1284`）、`:1328-1330` |
| 长上下文档位 | 仅当四个 `*_above_200k` 全空才铺；上游已有任一项则完全不动 | `pricing.rs:1946-1978` |
| 用户 override | 字段级；`input` 改动且 **`base.input > 0.0`**、cache 是派生值（`!cache_read_explicit`）时按 `new/old` 同比缩放；`cache_*_explicit` 与 base 取或后写回 | `pricing.rs:1820-1915`（缩放前提 `:1838-1845`、取或 `:1890-1893`） |

用户 override 细节：

- 结果写回 `entries`（`pricing.rs:1911`），同时保留一份在 `user_overrides`（`:1821-1822`）供时间调度后重新叠加（`:1468-1481`）。
- 已有条目的缓存价是派生值时按比例缩放（`pricing.rs:1838-1883`；缩放前提是 `override.input_cost_per_token.is_some() && base.input > 0.0 && !base.cache_read_explicit`，`:1838-1840`；写回时 `cache_read_explicit`/`cache_create_explicit` 与 base 取或，`:1890-1893`；测试 `pricing.rs:6069-6179` 三条）；新模型则新建条目，未给字段为 0.0（测试 `pricing.rs:6021-6033`）。
- 别名键的 override 以别名目标条目为底（`pricing.rs:1823-1828`）。

失败降级：无 HTTP 客户端 → 等价离线（`pricing.rs:2886-2893`）；LiteLLM 抓取或解析失败（0 条）→ 仅当 `log_level() >= 4` 时 WARN，继续用已有表（`:1016-1027`、`:2810-2812`）；models.dev 失败 → WARN + **60 秒退避**（`:58`、`:814-818`、`:2839-2849`）；`--offline` → 全部网络步骤跳过且 `enable_models_dev_fallback=false`（`:1007`、`:1035`，测试 `:3934-3946`）。

**证据**：`pricing.rs:982-1038`、`:1040-1108`、`:1820-1915`、`:1946-1978`、`:1980-1983`、`:2810-2812`、`:2835-2861`.

---

### 3. 价格数据结构

#### 3.1 计算条目 `Pricing`（完整字段，含原注释）

```rust
// pricing.rs:196-220
#[derive(Debug, Clone, Copy)]
pub struct Pricing {
    pub input: f64,
    pub output: f64,
    pub(crate) cache_create: f64,
    pub cache_read: f64,
    pub cache_read_explicit: bool,
    /// Whether `cache_create` came from published data rather than the derived
    /// `input * 1.25` default, so provider-fact patches know what they may fix.
    cache_create_explicit: bool,
    pub input_above_200k: Option<f64>,
    pub output_above_200k: Option<f64>,
    pub(crate) cache_create_above_200k: Option<f64>,
    pub cache_read_above_200k: Option<f64>,
    // Token count above which the `*_above_200k` rates apply. The field names
    // keep the LiteLLM `_above_200k_tokens` suffix for JSON compatibility, but
    // some providers switch tiers at a different point (OpenAI long-context
    // pricing starts above 272K input tokens), so the threshold is per model.
    pub(crate) long_context_threshold: Option<u64>,
    pub fast_multiplier: f64,
    pub flex_multiplier: f64,
}
/// Default tier boundary for LiteLLM `*_above_200k_tokens` pricing fields.
pub(crate) const DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS: u64 = 200_000;   // :219-220
```

`Pricing::empty()` 全 0、倍率 1.0（`pricing.rs:233-249`）。

| 字段 | 参与金额 | 说明 |
| --- | --- | --- |
| `input` / `output` | 是 | §6 |
| `cache_create` | 是 | 5 分钟缓存写入单价 |
| `cache_read` | 是（例外） | Codex 在 `!cache_read_explicit` 时改用 `input`（`codex/report.rs:344-348`） |
| `cache_read_explicit` / `cache_create_explicit` | 间接 | 决定 override 缩放与 provider 补丁是否生效（`:1838-1845`、`:1990-2008`） |
| 四个 `*_above_200k` | 是 | 超阈值档位；任一非空即视为上游已带档位 |
| `long_context_threshold` | 是 | 有值 → 整单切换；无值 → 200K 边际（§6.1） |
| `fast_multiplier` / `flex_multiplier` | 是 | 服务档倍率（§6.3） |

#### 3.2 上游输入类型

```rust
struct LiteLlmPricing {                         // :540-552（完整形状）
    input_cost_per_token, output_cost_per_token,
    cache_creation_input_token_cost, cache_read_input_token_cost,
    input/output/cache_creation/cache_read_cost_per_token_above_200k_tokens,
    max_input_tokens, provider_specific_entry: Option<{ fast, flex }>,   // :554-558
}
struct CompactLiteLlmPricing {                  // :560-573（内嵌压缩形状）
    i, o, cc, cr, ia, oa, cca, cra, ctx, fast, flex,
}
struct ModelsDevModel { id, cost, limit{context}, modalities{input,output}, exactOnly }  // :834-877
struct ModelsDevCost  { input, output, cache_read, cache_write, tiers[] }
struct ModelsDevCostTier { input, output, cache_read, cache_write, tier{type,size} }
```

`parse_litellm_pricing`（`:2596-2626`）先试压缩形状（要求含 `i` 与 `o`），否则按完整形状解析并要求 input/output 同时存在。models.dev 单价是**每百万 token**，装载时除 1e6（`:909-912`、`:1285-1301`）。

#### 3.3 缺省派生（关键语义）

```rust
cache_create = cache_creation_input_token_cost.unwrap_or(input * 1.25)   // :1085-1087
cache_read   = cache_read_input_token_cost.unwrap_or(input * 0.1)        // :1088
```
（models.dev 侧同规则 `:1294-1301`。）`*_explicit` 正是用来区分「上游给的」与「派生的」，这也是改 `input` 时派生缓存价会同比缩放的原因。

#### 3.4 内置兜底表与补丁

`put_builtin_pricing`（`:2015-2593`）**默认**只在快照缺键时兜底（`entry().or_insert()`，注释 `:2010-2014`：上游按小时刷新，用冻结数字覆盖会把过期价引回来），**但有 3 处走无条件 `self.entries.insert`**，上游已有价也会被兜底值覆盖：`gpt-5.1-codex`(`:2369-2370`，未套 flex 倍率)、`gpt-5.2-codex`(`:2386-2387`)、`gpt-5.2`(`:2400-2408`，套 flex 倍率)。覆盖 `claude-opus-4-5/4-6/4-7/4-8`(5e-6/25e-6，缓存 6.25e-6/0.5e-6)、`claude-haiku-4-5`、`claude-opus-4`、`claude-sonnet-4-6`、`claude-sonnet-4`(带 200K 档位 `:2158-2175`)、`claude-3-5-haiku`(+`-20241022`)、`claude-3-opus/-sonnet/-haiku`、`gpt-5`、`gpt-5.5`、`grok-4.3`、`gpt-5.1-codex`、`gpt-5.2-codex`、`gpt-5.3-codex`(`:2388-2400`)、`gpt-5.2`、`gpt-5.4`、`gpt-5.4-mini/-nano`、`gpt-5.6-sol/-terra/-luna`(`:2475-2502`，5e-6/30e-6、2.5e-6/15e-6、1e-6/6e-6)、`glm-5/-turbo/5.1`；GLM 走 `put_builtin_glm`（`:1990-2008`，只补缓存字段、不覆盖上游显式缓存价，测试 `:3779-3830`）；上下文上限写在 `:2557-2591`（如 `gpt-5.5 → 1_050_000`）。

**证据**：`pricing.rs:196-250`、`:540-573`、`:834-912`、`:1080-1108`、`:1285-1315`、`:1990-2593`、`codex/report.rs:344-348`.

---

### 4. 模型名匹配算法（重点）

#### 4.1 匹配链

```text
find(model)                                                 # :1354-1391（含 find_cache，miss 也缓存）
└─ find_with_fallbacks(model, models_dev, embedded_models_dev)          # :1393-1454
   1) alias = model_aliases::resolve_model_name(model)                  # :1399
   2) 精确阶段（Fuzzy::Denied）：
        find_entry_or_alias_with_fallback(model, Denied, None)          # :1405-1406
        ?: (alias != model) 时对 alias 再做一次                          # :1407-1413
        命中即返回                                                       # :1414-1416
   3) fuzzy = allows_fuzzy_lookup_with_fallbacks(model, alias, …)       # :1420-1425
   4) 模糊阶段：本表(model) → 本表(alias) → 在线 models.dev(alias) → 内嵌 models.dev(alias)  # :1426-1453

find_entry_or_alias_with_fallback(model, fuzzy, embedded)               # :1559-1574
   1) entries.get(model)                       # 精确 :1565-1567
   2) pricing_alias(model) → find_entry_with_fallback(alias)            # :1568-1572
   3) find_entry_with_fallback(model)                                   # :1573

find_entry_with_fallback(model, fuzzy, embedded)                        # :1586-1615
   1) entries.get(model)                                                # :1592
   2) exact_only.id_spelled_by(model) → entries[id]                     # :1595-1597（见 :517-521）
   3) fuzzy == Denied 或 is_exact_only_lookup_with_fallback(model) → None  # :1598-1602
   4) 扫描全表取最长匹配：filter(!exact_only) + filter(pricing_key_matches)
      max_by(|l, r| l.len().cmp(&r.len()).then_with(|| r.cmp(l)))   # :1604-1613
```

第 4 步 tie-break 的准确语义：**最长键优先；长度相同时字典序较小者胜**（`max_by` 收到 `r.cmp(l)`，故右侧更大时不替换当前最大者）。`context_limit` 的模糊扫描使用同一比较器（`pricing.rs:1803-1805`），并同样受 `exact_only` 过滤（`:1799`）。

另有一条只做「精确 + 分隔符归一化」的路径 `find_exact_normalized`（`:1518-1533`），由 `find_exact_with_fallback`（`:1501-1516`）与 pi 的 `find_exact` 使用，依赖预建索引 `normalized_keys()`（`:1535-1548`，同拼写多键时记 `None`）。上下文上限查询 `context_limit` 走**结构并行的同一套链**（`:1685-1808`）。

#### 4.2 归一化

```rust
// pricing.rs:2724-2731
fn normalized_pricing_key(value: &str) -> Cow<'_, str> {
    if value.contains(['.', '@']) { Cow::Owned(value.replace(['.', '@'], "-")) }
    else { Cow::Borrowed(value) }
}
```

只把 `.` 与 `@` 换成 `-`，**不折叠大小写**；`claude-opus-4.7-20260416` ≡ `claude-opus-4-7-20260416`，`claude-opus-5@eu` ≡ `claude-opus-5-eu`（测试 `:3508-3535`）。models.dev **装载侧**另用 `normalized_models_dev_model_id`（`:750-759`，会 `to_lowercase()`）做拼写归并（`:1239`、`:1274-1284`），两者不可混用。日期后缀剥离 `model_without_date_suffix`（`:2767-2790`，支持 `-YYYY-MM-DD` 与 `-YYYYMMDD`）只用于档位继承（`:1959-1964`）、阈值查询（`:2755-2760`）与倍率查找（`:961`）。

#### 4.3 模糊匹配的边界规则

```rust
// pricing.rs:2667-2722
fn pricing_key_matches(candidate, model, normalized_model) -> bool {
    contains_pricing_key(model, candidate)                     // 候选是查询子串
        || contains_pricing_key(candidate, model)              // 查询是候选子串
        || contains_pricing_key(normalized_model, &normalized_pricing_key(candidate))
        || contains_pricing_key(&normalized_pricing_key(candidate), normalized_model)
}
fn contains_pricing_key(value: &str, key: &str) -> bool {
    value.match_indices(key).any(|(index, _)| {
        let before = index.checked_sub(1).and_then(|i| value.as_bytes().get(i)).copied();
        let suffix = &value[index + key.len()..];
        before.is_none_or(is_pricing_key_boundary) && suffix_allows_pricing_key_match(key, suffix)
    })
}
fn is_pricing_key_boundary(byte: u8) -> bool { !byte.is_ascii_alphanumeric() }        // :2689-2691
fn suffix_allows_pricing_key_match(key, suffix) -> bool {
    let Some(sep) = suffix.as_bytes().first().copied() else { return true };
    if !is_pricing_key_boundary(sep) { return false }          // 后接字母/数字 → 拒绝
    !suffix_starts_with_numeric_model_version(key, suffix)     // :2703-2722：数字版本边界保护
}
// suffix_starts_with_numeric_model_version：key 末位是数字 + 后缀以 '-'/'.' 开头 + 其后是数字
// → 拒绝（视作不同版本）；例外：数字正好 8 位（MODEL_DATE_SUFFIX_DIGITS=8，:59-61）且后面是尾/边界
```

后果与测试：

- 双向子串都必须落在非字母数字边界上；版本升级不会被误降级：只有 `claude-opus-4` 时 `claude-opus-4.8-20260528`/`-4-9`/`-5`/`-4.70` 全 miss，`claude-opus-4-20250514` 命中（`pricing.rs:5705-5732`）。
- 日期后缀放行（`:5734-5739`）；边界必须存在（`:5662-5703`）。
- 全长优先：`claude-sonnet-4-20250514-via-bedrock` 命中更长键 `claude-sonnet-4-20250514`(2.0) 而非 `claude-sonnet-4`(1.0)（`:5853-5898`）。

#### 4.4 `exact_only` 门禁

动机见结构体注释（`pricing.rs:419-424`）：若 `kimi-k2.7-code-highspeed` 这类单独定价的高档位留在模糊扫描里，基础模型会因「最长匹配」抢到它。

- `ExactOnlyKeys`（`:443-473`）：原始拼写集合 `raw` + 归一化拼写索引 `normalized`（同拼写多 id 记 `None`）。三个查询方法对应三种语义：`contains`（`:501-503`）问「这个候选键本身是否 exact-only」，供 fuzzy 扫描过滤候选；`contains_any_spelling`（`:507-512`）问「这个请求名是否属于 exact-only 的任一拼写」，供门禁决定能否模糊；`id_spelled_by`（`:517-521`）问「该拼写唯一指认哪个 exact-only 条目」，决定由哪个条目作答。
- 查询侧：`is_exact_only_lookup_with_fallback`（`:1646-1654`）检查本表与内嵌 models.dev；`allows_fuzzy_lookup_with_fallbacks`（`:1664-1683`）在查询名或其静态别名属于 exact-only 时返回 `Fuzzy::Denied`（`:1676-1682`）。
- 标记来源：内嵌快照直接带 `"exactOnly": true`（`ModelsDevModel.exact_only`，`:840-843`；实例 `models-dev-pricing.json:7634-7645` 的 `claude-opus-5-fast`）；在线响应无此字段，用生成期同一规则重推 `ModelsDevCatalogRules::is_exact_only`（`:674-698`）= 是作者模型的 tier 变体（归一化后以作者 id 开头且紧跟 `-`，且不含 `/`）或 id 不含任何数字（`:761-768`）。
- 门禁跨数据源生效：主表能 fuzzy 命中基础模型也不行，必须让内嵌 models.dev 的档位条目接手（注释 `:1617-1637`）。测试：`:3342-3368`、`:3371-3453`、`:3480-3536`、`:3648-3704`、`:4702-4728`。

#### 4.5 四个给定名字的命中推演

**(a) `claude-sonnet-4-20250514`**：**实测修正（第 8 轮）——不是精确命中，而是模糊命中**。内嵌表（534 键，见 §13.5）里**没有**该裸键（`claude-*` 裸键只覆盖 4.5 代及以后）；真实路径是双向子串落到区域前缀的 Bedrock 键——实测命中最长键 `global.anthropic.claude-sonnet-4-20250514-v1:0`（长度 46），候选另含 `anthropic.` / `us.` / `eu.` 三个前缀的变体（**`apac.` 不在内嵌表**——裁剪白名单不含该前缀，它只存在于未裁剪的完整 LiteLLM）。仓库测试 `embedded_pricing_resolves_overlapping_model_keys_exactly`（`pricing.rs:4931-4955`）正是这项行为的断言：它比较 `find("claude-sonnet-4-20250514")` 与 `find("anthropic.claude-sonnet-4-20250514-v1:0")` 的 **input 相等**，而非断言裸键存在。8 位日期后缀放行规则（`suffix_starts_with_numeric_model_version`，`:2703-2722`）与「取最长键」（`:5853-5898`）仍如原述；`claude-3-5-haiku-20241022` 则**不需要模糊匹配**：它是**内嵌兜底表的精确命中**——`pricing.rs:2191-2192` 用 `put_builtin_entry` 把 `claude-3-5-haiku` 与 `claude-3-5-haiku-20241022` 直接写入 `entries`，精确阶段的 `entries.get(model)`（`:1565-1567`）就作答了，三个同价的 Bedrock 变体（`anthropic.` / `eu.` / `us.`，input 均为 `8e-7`）不会被选中。这也解释了为什么 `pricing.rs:4951-4954` 的断言（input `0.8e-6`）**无法区分命中路径**——三条候选价同。（若它真的走模糊扫描，会按「同长度取字典序较小者」在并列候选里选中 `eu.`：实测该组长度为 `eu.` 43、`us.` 43、`anthropic.` 40。）

**(b) `claude-opus-4-6-fast`**：models.dev 快照中**没有**该键（只有 `claude-opus-4-8-fast`:7504、`claude-opus-5-5-fast`:7598、`claude-opus-5-fast`:7634，三者均带 `"exactOnly": true`，见 `models-dev-pricing.json:7504-7515`、`:7598-7609`、`:7634-7645`）。精确 miss 后 fuzzy：`contains_pricing_key("claude-opus-4-6-fast", "claude-opus-4-6")` 成立（前边界为空、后缀 `-fast` 首字节是边界、key 末位非数字）→ 命中基础条目 `claude-opus-4-6`（`:2667-2722`）。前提是该拼写不是 exact-only，否则 `allows_fuzzy_lookup_with_fallbacks` 拒绝 fuzzy 并转内嵌 models.dev（`:1664-1683`、`:1450-1453`）。fast 倍率由 `fast-multiplier-overrides.json` 的 `normalized_prefix["claude-opus-4-6"]=6.0` 提供（`SpeedMultiplierOverrides::multiplier_for` `:944-978`、`matches_model_suffix` `:2802-2808`），测试 `:5782-5842`（`claude-opus-4.7-20260416` → 6.0，无 tier 关系的 `claude-opus-4-70` → 1.0）。现实入口是 Claude adapter 在 `speed: fast` 时把**展示名**写成 `{model}-fast`（`claude/src/lib.rs:535-543`），金额仍按原始模型名 + `Speed::Fast`（`:525` → `cost.rs:110-115`）。

**(c) `gpt-5.3-codex`**：内置兜底表有精确键（`:2388-2400`），快照也有（`models-dev-pricing.json:15868`、`:5691` 的 `azure/…`、`:24474` 的 `openai/…`）→ 精确命中。fast 倍率 2.0 来自 `fast-multiplier-overrides.json` 的 `exact`，测试 `:5418-5431`、`:5741-5780`（缺上游倍率仍为 2.0，而 `gpt-5.2-codex` 保持 1.0）；计价走 Codex 专属公式（§6.2）。

**(d) `[pi] gpt-5.4`**：名字由 pi adapter 渲染为 `[<store>] <model>`（`pi/src/parser.rs:352-356`，默认 store 名 `pi`，`:225-231`）。查找顺序 `store_pricing_at`（`:697-708`）/`store_pricing`（`:685-695`）：`override_model → find_at/find` → `display_model → find_exact` → `raw_model → find_at/find`。`pricing_override_model`（`:629-638`）先问 `has_user_override(display_model)`（`pricing.rs:1489-1494`，含别名解析）再问 raw。因此：有 `pricingOverrides["[pi] gpt-5.4"]` → 命中 `apply_override` 写回 `entries` 的条目（`pricing.rs:1911`），且 Auto 模式被强制改为 token 重算（`source_cost_for_mode` `parser.rs:640-650`）；无 override → `find_exact("[pi] gpt-5.4")` 必 miss，退到 `raw_model="gpt-5.4"` 命中内置条目（`pricing.rs:2409-2430`）或快照（`models-dev-pricing.json:15900`）。测试：`pi/src/parser.rs:774-790`、`:950-984`、`:863-880`、`:915-948`。

**证据**：`pricing.rs:1354-1454`、`:1461-1482`、`:1518-1548`、`:1559-1615`、`:1646-1683`、`:2666-2808`、`:443-522`、`:674-698`、`:761-768`、`pi/src/parser.rs:225-231`、`:352-356`、`:629-708`、`claude/src/lib.rs:525-543`.

---

### 5. 别名表

#### 5.1 `CCUSAGE_MODEL_ALIASES`（运行期用户别名）

- 常量 `MODEL_ALIASES_ENV = "CCUSAGE_MODEL_ALIASES"`（`model_aliases.rs:8`）；无内置默认，未设即空表（`:32-37`）。
- 解析 `parse_model_aliases`（`:39-61`）：内容以 `{` 开头且可解析为 `BTreeMap<String,String>` → 按 JSON；否则剥外层花括号后用 `,`/`;`/换行切分、每段 `from=to`（`:52-60`）。结果存 `OnceLock<RwLock<BTreeMap<String,String>>>`（`:10`、`:28-30`）。
- 查询 `resolve_model_name`（`:14-26`）：精确命中返回目标名；否则若名字以 `-fast` 结尾且基名命中，返回 `{alias}-fast`（`:20-24`）；否则返回原名借用。
- 测试：`:103-115`、`:117-125`、`:127-140`、`:142-149`（含 `private-alpha-fast → gpt-5.5-fast`）。

全部使用点：`pricing.rs:1399`（`find`）、`:1462`（`find_at`）、`:1706`（上下文上限）、`:273`（`has_time_dependent_pricing`）、`:1493`（`has_user_override`）、`cost.rs:74`/`:90`（缺价上报用的是解析后的名字）、`pi/src/parser.rs:682`。交叉测试 `pricing.rs:5916-5931`。

#### 5.2 `pricing_alias`（内置静态别名，共三条）

```rust
// pricing.rs:2792-2800
"gpt-reserve"   => Some("gpt-5.6-luna"),
"gpt-5.6"       => Some("gpt-5.6-sol"),
"gpt-5.3-spark" => Some("gpt-5.3-codex-spark"),
```

使用点：`pricing.rs:1569`（精确 miss 后、模糊之前）、`:1677`（别名目标 exact-only 时禁模糊）、`:1827`（override 取 base）、`:1960`、`:2756`（档位/阈值继承）、`:950`/`:967`（倍率继承）、`:1772`（上下文上限）。测试：`:5110-5166`、`:5286-5298`、`:5317-5376`、`:5544-5579`。

#### 5.3 服务档倍率表

上游 `provider_specific_entry.fast/flex` 优先，缺失时用内嵌覆盖表（`pricing.rs:1071-1079`、`:1309-1314`）：

`fast-multiplier-overrides.json` → `exact`: `gpt-5.6-sol/terra/luna`=2.0、`gpt-5.5`=2.5、`gpt-5.4`=2.0、`gpt-5.3-codex`=2.0、`gpt-6-astra/sol`/`gpt-6.1-sol`/`gpt-6-luna`=2.0；`normalized_prefix`: `claude-opus-4-6`=6.0、`claude-opus-4-7`=6.0、`claude-opus-4-8`=2.0。
`flex-multiplier-overrides.json` → `exact`（**20 个键，全部 0.5**）：`gpt-6-astra`、`gpt-6.1-sol`、`gpt-6-luna`、`gpt-6-sol`、`gpt-5.6-sol`、`gpt-5.6-terra`、`gpt-5.6-luna`、`gpt-5.5`、`gpt-5.5-pro`、`gpt-5.4`、`gpt-5.4-mini`、`gpt-5.4-nano`、`gpt-5.4-pro`、`gpt-5.2`、`gpt-5.1`、`gpt-5`、`gpt-5-mini`、`gpt-5-nano`、`o3`、`o4-mini`；`normalized_prefix` 为空。

查找算法 `SpeedMultiplierOverrides::multiplier_for`（`pricing.rs:944-978`）：`exact.get(model)` → `pricing_alias(model)` → 按 `/`、`:` 切段并对每段枚举「点号之后的子串」 → 逐候选 `exact.get(part)` → `exact.get(model_without_date_suffix(part))` → `pricing_alias(base)` → `normalized_prefix` 前缀匹配（`.`/`@`→`-` 后用 `matches_model_suffix`）。测试：`:4826-4879`、`:5418-5461`、`:5462-5535`、`:5580-5605`。

**证据**：`model_aliases.rs:8-150`、`pricing.rs:2792-2808`、`:944-978`、`fast-multiplier-overrides.json`、`flex-multiplier-overrides.json`、`pricing.rs:1040-1079`、`:1285-1315`.

---

### 6. 金额公式（重点）

#### 6.1 通用路径（`cost.rs`）

```rust
// :31-46 模式分支
Display   => cost_usd.unwrap_or(0.0)
Auto      => cost_usd.unwrap_or_else(|| calculate_cost_from_tokens(..))
Calculate => calculate_cost_from_tokens(..)

// :93-116 取价 + 服务档倍率
let Some(model) = model else { return 0.0 };
let Some(pricing) = (有 ts ? pricing.find_at(model, ts) : pricing.find(model)) else { return 0.0 };
let multiplier = if usage.speed == Some(Speed::Fast) { pricing.fast_multiplier } else { 1.0 };
calculate_cost_from_pricing(usage, pricing) * multiplier

// :118-199 分项
(c5m, c1h) = usage.cache_creation ? (ephemeral_5m_input_tokens, ephemeral_1h_input_tokens)
                                  : (cache_creation_input_tokens, 0)          // :119-127
c1h_rate       = pricing.input            * 2.0                                // :7, :128（常量 CACHE_CREATE_1H_INPUT_MULTIPLIER）
c1h_rate_above = pricing.input_above_200k * 2.0                                // :129-131

// 档位 A（有 per-model 阈值）：整单切换，:143-168
context = input + cache_read + c5m + c1h          // :147-151，全部 saturating_add
long    = context > threshold                     // 严格大于，:152
rate(base, above) = long ? (above ?? base) : base
cost = input*rate(input) + output*rate(output) + c5m*rate(cache_create)
     + c1h*rate(c1h_rate) + cache_read*rate(cache_read)

// 档位 B（无阈值，默认 200_000）：边际分段，:170-199
cost = Σ tiered_cost(tokens, base, above, 200_000)，桶为 input / output / c5m / c1h / cache_read
// tiered_cost（:201-211）：
tokens == 0                      -> 0.0                          # :202-204 早退
above 存在 && tokens > threshold -> threshold*base + (tokens-threshold)*above
否则                             -> tokens*base
```

| 语义点 | 说明 | 证据 |
| --- | --- | --- |
| 整单 vs 边际 | 有 `long_context_threshold` 时全请求所有桶走高档位价；只有 LiteLLM `*_above_200k_tokens`（无 per-model 阈值）时才是边际 | `cost.rs:133-142`、`:170-172`、`:201-211` |
| 阈值判定用的 context | 含 `input + cache_read + c5m + c1h`（缓存命中也算上下文） | `cost.rs:139-151`、测试 `:436-478` |
| 计数溢出 | 四桶求和全部用 `saturating_add`：counters 来自宽松 JSONL 解析，环绕会把合计压到阈值以下、溢出检查会直接中止（源码注释 `cost.rs:144-146`） | `cost.rs:147-151` |
| 1h 缓存写入 | `input × 2`，与 `cache_create` 单价无关 | `cost.rs:7`、`:128-131` |
| 有 `cache_creation` 明细 | `cache_creation_input_tokens` 不单独计价，避免双计 | `cost.rs:119-127` |

数值自证：`cost.rs:317-338`（5m=10、1h=20、cache_read=30 → 55.5）；`:358-434`（阈值 100，input 59/60/61 → 93e-6 / 94e-6 / 190e-6，证明整单切换 + 严格大于）；`:436-478`（真实 `grok-4.5` 价，缓存重读 500K → 0.352 vs 短上下文 0.056）。

#### 6.2 Codex 专属路径

```text
// codex/report.rs:343-392  bucket 计价（short/long 双桶）
cache_read = cache_read_explicit ? pricing.cache_read : pricing.input            // :344-348
long_input_rate = input_above_200k ?? input ; long_output_rate = output_above_200k ?? output
long_cache_read = cache_read_explicit ? (cache_read_above_200k ?? cache_read) : long_input_rate
long_cache_creation_rate = cache_creation_above_200k ?? cache_creation
cost = short_non_cached*input + short_cached*cache_read + short_cache_creation*cache_creation
     + (output - long_output)*output
     + long_non_cached*long_input_rate + long_cached*long_cache_read
     + long_cache_creation*long_cache_creation_rate + long_output*long_output_rate

// codex/report.rs:234-275  服务档按桶增量
cost = bucket_cost(total, pricing)
     + bucket_cost(flex_usage, pricing) * (flex_multiplier - 1.0)
     + bucket_cost(fast_usage, pricing) * (fast_multiplier - 1.0)
```

- long 桶在聚合阶段按**每条请求**切分：`is_long_context = event.input_tokens > long_context_split_threshold(model)`（`codex/aggregate.rs:476-487`），阈值来自 models.dev 快照，缺省 200 000（`pricing.rs:2753-2763`）。
- flex/fast 桶由 `--speed` 强制值或日志 `thread_settings.service_tier` 决定；**拼写映射在 `codex/parser.rs:649-659`**（`default`|`standard` → Standard、`fast`|`priority` → Fast、`flex` → Flex，其它值映射为 `None` 即清除已记录档位，语义见 `codex/parser.rs:465-475`，测试 `:1325-1337`）；`--speed` 枚举与各 Codex home 的 `config.toml` 解析在 `codex/speed.rs:14-44`。未分类 token 按 auto 策略归入当前档（`codex/report.rs:250-271`、`codex/src/README.md:35-39`）。
- 时间敏感模型（DeepSeek V4）先按时间戳分桶（`codex/aggregate.rs:488-509`），计价时逐桶 `find_at`（`codex/report.rs:190-204`）。

#### 6.3 fast/flex 的适用范围

- 通用路径只在 `usage.speed == Some(Speed::Fast)` 时乘 `fast_multiplier`（`cost.rs:110-115`）；`flex_multiplier` 在通用路径不参与（来源无 flex 概念）。
- Codex 用 §6.2 的桶增量形式同时支持 fast/flex。
- 倍率来源顺序见 §5.3（上游 `provider_specific_entry` 优先于内嵌覆盖表）。

#### 6.4 `extra_total_tokens` 是否参与计价

**本身不参与**，但产生它的路径可能改变金额：

```rust
// utils.rs:22-38
known = total_usage_tokens(usage) + extra_total_tokens
missing = total_tokens - known                                    // saturating
if missing == 0 → 原样返回
if usage.output_tokens == 0 → usage.output_tokens = missing       // ← 这部分会被计价
else → extra_total_tokens += missing                              // ← 不计价
```

触发点只有「日志给出 `total_tokens` 但桶之和不匹配」（Kimi、Pi 等：`kimi/src/parser.rs:208`、`pi/src/parser.rs:348`）。展示端一律计入 `totalTokens`（`types.rs:89-95`、`:115-121`、`:183-189`；`output.rs:96-109`），字段本身被 `#[serde(skip_serializing)]` 隐藏（`types.rs:106-107`、`:168-169`）。

#### 6.5 `credits`（Amp 等）如何呈现

解析：Amp 从 `usageLedger.events[]` / `messages[].usage` 读 `credits`（`amp/src/parser.rs:212`、`:320`）；Codebuff 只在 >0 时保留（`codebuff/src/loader.rs:84`）。汇总累加（`summary.rs:56-60`、`:242-247`）。呈现：summary/session JSON 有值即输出（`output.rs:51-53`、`:73-75`），Totals 只在合计 >0 时输出（`output.rs:112-115`），表格列 `Credits` 用裸 `{:.2}`（`common/src/report.rs:45`、`:70`、`:108`）。**不进入任何金额公式**。

#### 6.6 缺失价格的行为

金额直接 `0.0`（`cost.rs:99-109`），不 panic；「显式 0 价」不算缺价（测试 `cost.rs:217-237`）。

| 披露通道 | 内容 | 证据 |
| --- | --- | --- |
| `ModelBreakdown.missing_pricing` | 仅 true 时序列化 `"missingPricing": true` | `types.rs:109-111`、测试 `output.rs:737-742`、`:744-800` |
| `totals.unpricedModels` | 去重排序的模型名数组，全有价时该键缺席 | `output.rs:126-145` |
| stderr WARN | 离线：`WARN  Missing embedded pricing for {model}; cost excludes this model. Run without --offline or update ccusage after pricing is added.`；在线：`WARN  Missing pricing for {model}; cost excludes this model. Update pricing or run again after LiteLLM has the model.` | `output.rs:430-450` |
| 判定 | `missing_pricing_model_for_usage`（Display 或 Auto+有 costUSD → 不报）→ `…_for_token_total`（token=0 不报；`find` 为 None 才报） | `cost.rs:48-75` |
| Codex 变体 | `missing_pricing: mode != Display && codex_model_missing_pricing(...)` | `ccusage-adapter-all/src/loader.rs:856-859` |

注意 `missing_pricing = true` 的 breakdown 仍可能有正 cost：`summary.rs:92`（`breakdown.cost += entry.cost`）与 `summary.rs:93-95`（仅在 `entry.missing_pricing_model.is_some()` 时置标记）是两条互相独立的语句，且 `output.rs:123-125` 的文档注释明确写明 "A model can still show positive cost when other entries carried stored costs"；同一模型部分事件带 `costUSD`（走 display 分支）时会同时满足两者。

**证据**：`cost.rs:7`、`:31-211`、`:217-237`、`:317-478`、`codex/report.rs:183-392`、`codex/aggregate.rs:476-509`、`utils.rs:22-38`、`types.rs:89-121`、`:168-189`、`output.rs:51-145`、`:410-450`、`amp/src/parser.rs:212`、`:320`、`codebuff/src/loader.rs:84`、`common/src/report.rs:45-108`、`ccusage-adapter-all/src/loader.rs:856-859`、`pricing.rs:2753-2763`.

---

### 7. cost mode 语义

```rust
// ccusage-cli/src/types.rs:242-248
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CostMode { #[default] Auto, Calculate, Display }
```

- CLI 文本映射 `"auto" | "calculate" | "display"`（`ccusage-cli-parser/src/parser.rs:1136-1138`）；`SharedArgs::with_defaults()` 设 `mode: Auto`（`ccusage-cli/src/types.rs:62-71`）；config 同名枚举与 `From`（`ccusage-config/src/config.rs:703-711`）。
- 分支（`cost.rs:39-46`）：`Display` → `cost_usd.unwrap_or(0.0)`；`Auto` → `cost_usd.unwrap_or_else(自算)`；`Calculate` → 总是自算。
- **auto 的判定依据只是 `costUSD` 是否存在**：`UsageEntry.cost_usd: Option<f64>`（serde `rename = "costUSD"`，`ccusage-core/src/types.rs:14-15`），`Some(0.0)` 也走 display，不回退到 token 计算。测试 `cost.rs:239-290`、`:563-612`（含 Display/Auto/Calculate 三态同数据对照）。
- adapter 差异：Claude 在 Display 模式**不加载价格表**（`claude/src/lib.rs:144-152`）；Pi 任何模式都加载（`pi/src/lib.rs:21-27`），auto 对记录价做 `is_finite && >= 0.0` 过滤、且有 override 时强制重算（`pi/src/parser.rs:640-650`）；Codex 的 mode 只决定是否用 `costUSD`，档位由 `--speed` + 记录 tier 决定（§6.2）。
- 文档交叉验证：`docs/guide/cost-modes.md:15-29`（auto）、`:50-65`（calculate）、`:85-99`（display 缺价 `$0.00`），与源码一致。

**证据**：`ccusage-core/src/types.rs:14-15`、`ccusage-cli/src/types.rs:62-71`、`:242-248`、`ccusage-cli-parser/src/parser.rs:1136-1138`、`ccusage-config/src/config.rs:703-711`、`cost.rs:31-46`、`:239-290`、`:563-612`、`claude/src/lib.rs:144-152`、`pi/src/lib.rs:21-27`、`pi/src/parser.rs:618-650`、`docs/guide/cost-modes.md:15-99`.

---

### 8. 时间相关定价

只有 DeepSeek V4 两个模型有时间维度定价，且是硬编码调度，不是通用「生效区间表」。

```rust
// pricing.rs:252
const DEEPSEEK_V4_PRICING_CUTOFF_MS: i64 = 1_786_896_000_000;   // = 2026-08-16T16:00:00Z
// pricing.rs:271-277
pub fn has_time_dependent_pricing(model: &str) -> bool {
    let resolved = model_aliases::resolve_model_name(model);
    deepseek_v4_model_identity(model).or_else(|| deepseek_v4_model_identity(&resolved)).is_some()
}
// deepseek_v4_model_identity（:262-269）只认归一化后的 "deepseek-v4-flash" / "deepseek-v4-pro"
```

| 时段 | 判定 | flash in/out | pro in/out |
| --- | --- | --- | --- |
| `old` | `ts < CUTOFF` | 0.14e-6 / 0.28e-6 | 0.435e-6 / 0.87e-6 |
| `off_peak` | `ts >= CUTOFF` 且非高峰 | 0.22e-6 / 0.66e-6 | 0.66e-6 / 1.98e-6 |
| `peak` | `ts >= CUTOFF` 且命中高峰窗口 | 0.44e-6 / 1.32e-6 | 1.32e-6 / 3.96e-6 |

（`pricing.rs:279-331`）高峰窗口（`pricing.rs:333-347`）：按 **UTC**（注释强调用 epoch 而非展示时区，`:334-335`），周六周日恒 off-peak，工作日小时落在 `[1,4)` 或 `[6,10)`（半开区间，结束整点不含）为 peak。

应用与叠加（`pricing.rs:349-368`、`:1461-1482`）：

```text
find_at(model, ts):
 1) 非 DeepSeek V4 → 直接 find(model)
 2) find(model) 取静态价 → apply_deepseek_v4_schedule 覆盖 input/output/cache_create/cache_read，
    并把四个 *_above_200k 也设为同价、cache_*_explicit 置真（防止 override 缩放误伤）
 3) 命中 user_overrides（原名 / 别名 / 调度后名字）→ apply_explicit_pricing_override（pricing.rs:370-403）
    只逐字段覆盖 override 中**显式给出**的项，其余保留调度结果 ⇒ 调度价与用户字段是字段级叠加，而非整条替换
```

调用点：`cost.rs:102-106`（有事件时间戳用 `find_at`）、`cost.rs:9-18`（`calculate_cost` 从 `data.timestamp` 解析）、`codex/aggregate.rs:488-509`、`:568`（按时间戳分桶）、`codex/report.rs:190-204`（逐桶计价）。

测试：`pricing.rs:2989-3033`、`:3035-3057`（UTC 工作日半开窗口与偏移无关）、`:3059-3085`、`:3087-3100`、`:3114-3136`（带 provider 前缀的第三方路由名不参与调度）、`:3138-3164`、`:3166-3180`、`:3182-3227`（用户显式 override 仍最高优先）；金额侧 `cost.rs:523-561`、`:563-612`、`:614-663`（无时间戳的旧 API 保静态价）。

**证据**：`pricing.rs:252-368`、`:1461-1482`、`cost.rs:9-18`、`:102-106`、`:523-663`、`codex/aggregate.rs:488-509`、`codex/report.rs:190-204`.

---

### 9. 缓存与性能

```rust
// pricing.rs:413-441
pub struct PricingMap {
    entries: FxHashMap<String, Pricing>,               // rustc_hash::FxHashMap（fast.rs:4）
    user_overrides: FxHashMap<String, PricingOverride>,
    exact_only: ExactOnlyKeys,                         // raw: FxHashSet + normalized: FxHashMap
    context_limits: FxHashMap<String, u64>,
    builtin_context_limits: FxHashMap<String, u64>,
    enable_models_dev_fallback: bool,
    enable_embedded_models_dev_fallback: bool,
    find_cache: OnceLock<Mutex<FxHashMap<String, Option<Pricing>>>>,
    normalized_keys: OnceLock<FxHashMap<String, Option<String>>>,
}
```

- 不是 `Rc`/`Arc`；`Pricing` 是 `Copy`（`:196`），adapter 以 `&PricingMap` 借用。

| 数据 | 生命周期 | 证据 |
| --- | --- | --- |
| 三份内嵌快照 inflate | `OnceLock<String>`，首次使用才解压，永久 | `pricing.rs:33-54` |
| 内嵌 models.dev → `PricingMap` | `OnceLock<PricingMap>` | `pricing.rs:2825-2833` |
| 在线 models.dev → `PricingMap` | `ModelsDevPricingCache`：成功永久缓存；失败记 `Instant` 并退避 60s | `pricing.rs:792-832`、`:58` |
| 在线 LiteLLM → 内存 | **无跨调用缓存**：每次非 offline 加载都会请求（可命中 HTTP 304 磁盘缓存） | `pricing.rs:1007-1028`、`http.rs:69-99` |
| 单次查询结果 | `find_cache`（含 miss），装载/补档位/应用 override 后被清 | `pricing.rs:1354-1391`、`:1917-1923` |

`clear_find_cache`（`:1917-1923`）同时清 `find_cache` 与 `normalized_keys`；调用点 `:1106`、`:1350`、`:1817`、`:1977`、`:1982`、`:2007`。`find` 的并发形状是「锁内读 → 锁外算 → 锁内写回」（`:1354-1391`），因此并发下同一名字可能被算两次，但结果幂等。

加载成本与日志：每次装载都会重新 serde 解析两份 speed override JSON（`:1041-1045`、`:1112-1115`，文件仅 356 / 474 B）；models.dev 装载按信任等级（`OWNER=3`/`PLATFORM=2`/`RESELLER=1`，`:649-651`）→ 声明 id → map key 排序 provider（`:1126-1132`），并按 key 排序 catalog 内模型（`:1208-1209`）以保证可复现；替换落败拼写时 `remove` 旧条目并清其上下文上限（`:1277-1284`、`:1328-1330`）。**没有价格加载耗时/条数的 info 日志**，唯一反馈是 TTY 进度文案 `Refreshing model pricing from LiteLLM...`，其开关就在调用点 `pricing.rs:1009`（`log && crate::progress::usage_load_output_is_tty()`），渲染与测试见 `progress.rs:308-319`；刷新类 WARN 亦受 `should_log_pricing_refresh_details() = log_level() >= 4` 门控（`:2810-2812`）。

**证据**：`pricing.rs:33-58`、`:413-441`、`:649-651`、`:792-832`、`:1002-1038`、`:1041-1045`、`:1106`、`:1126-1132`、`:1277-1284`、`:1328-1330`、`:1350`、`:1354-1391`、`:1917-1923`、`:2825-2833`、`fast.rs:4`、`progress.rs:308-319`.

---

### 10. 离线 / 在线

- 开关：`-O`/`--offline` → `shared.offline = true`（`ccusage-cli-parser/src/parser.rs:766`）；`--no-offline` → `shared.no_offline = true`（`:767`）；合成规则 `offline: args.offline && !args.no_offline`（`ccusage/src/commands/mod.rs:380`），因此 `--no-offline` 可覆盖配置里的 `"offline": true`。statusline 默认离线（`StatuslineArgs::default()` 中 `offline: true`，`ccusage-cli/src/types.rs:221-239`；文档 `docs/guide/statusline.md:59`）。
- 断网效果（`pricing.rs:1002-1038`）：跳过 LiteLLM 抓取（`:1007`）；`enable_models_dev_fallback = false`（`:1035`）；内嵌 models.dev 仍启用（`:998`，测试 `:3290-3304`、`:4914-4929`）；用户 override 照常应用（`:1036`）。文档口径一致：`--offline` 只控制数据源，`pricingOverrides` 控制具体条目（`docs/guide/config-files.md:455-462`）。
- 失败/超时/重试：

| 环节 | 行为 | 证据 |
| --- | --- | --- |
| LiteLLM 传输/状态错误 | `Err` → WARN（log≥4）→ 用已有表，不重试 | `pricing.rs:1021-1027`、`http.rs:100-102` |
| 响应形状不合法 | `validates_shape` 失败 → `Err` | `http.rs:121-126`、`pricing.rs:88-98` |
| 体积 > 64 MiB | `Err` | `http.rs:12`、`:104-151` |
| 全局超时 | 10 秒 | `http.rs:11`、`:75-78` |
| 304 但缓存体不合法 | 作废缓存并**重取一次** | `http.rs:88-99` |
| models.dev 失败 | WARN + 60 秒退避 | `pricing.rs:58`、`:814-818`、`:2839-2849` |
| 未安装 HTTP 客户端 | `no HTTP client installed for pricing refresh`，等价离线 | `pricing.rs:2886-2893` |
| 未知端点 URL | `unsupported pricing URL` | `http.rs:26-31` |

- 版本/日期戳：两套内嵌快照 JSON **没有任何版本或生成日期字段**（本次在 `models-dev-pricing.json` 34 269 行与 `models-dev-catalog-rules.json` 中检索 `generated|generatedAt|version|date|updated|_meta` 零命中）；运行期也无「快照年龄」校验。可追溯性只靠 git 提交与 `flake.lock` 的两个 pin（§1.1）。在线端点同样无版本标识（LiteLLM 抓 `main`，models.dev 抓 `api.json`）。

**证据**：`ccusage-cli-parser/src/parser.rs:766-767`、`ccusage/src/commands/mod.rs:380`、`ccusage-cli/src/types.rs:221-239`、`pricing.rs:998`、`:1002-1038`、`:2814-2861`、`http.rs:11-151`、`flake.lock:143-145`、`:159-161`.

---

### 11. 数字格式化

```rust
// output.rs:548-550
pub fn format_currency(value: f64) -> String { format!("${value:.2}") }
// output.rs:452-462
pub fn json_float(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0
        && value >= i64::MIN as f64 && value <= i64::MAX as f64
    { json!(value as i64) } else { json!(value) }
}
```

- `format_currency`：固定两位小数 + `$` 前缀；**没有** `< 0.01` 的特殊处理（0.004 → `$0.00`），无千分位，负数会得到 `$-1.00`；舍入由 Rust 标准库浮点格式化完成。
- `format_number`（`:536-546`）：整数千分位，用于 token 数。
- `credits` 列用裸 `{:.2}`（`adapters/common/src/report.rs:108`）。
- `json_float` 让整数金额在 JSON 中保持整数形态（`42` 而非 `42.0`），用于 Codex `costUSD`（`codex/report.rs:179`）、adapter-all `totalCost`（`ccusage-adapter-all/src/report.rs:169`、`:196`）、Amp credits（`ccusage-adapter-all/src/loader.rs:813`）。
- `--no-cost` 走 `strip_cost_json` 递归删除 `totalCost` / `costUSD` / `cost`（`output.rs:564-579`）。

**证据**：`output.rs:452-462`、`:536-550`、`:564-579`、`codex/report.rs:179`、`ccusage-adapter-all/src/report.rs:169`、`:196`、`ccusage-adapter-all/src/loader.rs:813`、`common/src/report.rs:108`.

---

### 未验证 / 存疑

> **本节状态（第 8 轮实测后）**：§13.5 的对照表已逐条标出本节哪些条目被闭环（内嵌键集、models.dev 生成器、在线目录对比、格式化舍入等）与被部分闭环的边界；第 5 条（并发压力）与第 11、12 条（兜底表写入意图、Codex 未分类 token）仍开放。

1. **内嵌 LiteLLM 快照的键集未直接检视**：`$OUT_DIR/litellm-pricing.json.deflate` 是编译产物，当时（第 1–7 轮）遵守「只读、不构建、不运行」约束未生成它；`claude-sonnet-4-20250514`、`claude-opus-4-6-fast` 等在该快照中的存在性结论来自仓库内测试断言（如 `pricing.rs:4931-4955`）与算法推演。**（第 8 轮已闭环：见 §13.5——构建产物被解压后与重放键集逐键逐值 0 差异，534 键。）**
2. **`format_currency` 舍入模式未实验**：`{:.2}` 对 `x.xx5` 的边界行为未在测试中断言，本文只描述为「Rust 标准库浮点格式化」。
3. **models.dev 在线结果与内嵌快照的一致性**依赖生成期规则逐条对齐；本次核对了规则代码（`pricing.rs:653-744`）与若干快照条目，未做 34K 行全量比对，也未审阅 `nix/tools/models-dev-gen` 生成器的实现细节（仅确认其测试文件存在）。
4. **并发语义未压力验证**：`find_cache` 的「锁外计算 + 锁内写回」与 `clear_find_cache` 的交互只在单线程测试覆盖。
5. **1 小时缓存写入 `input × 2`** 取自常量与注释（`cost.rs:7`、`:128-131`）及文档 `docs/guide/cost-modes.md:198-205`，未与厂商定价页交叉验证。
6. **DeepSeek V4 三段价与 UTC 高峰窗口**为硬编码（`pricing.rs:279-347`），本文只核对代码与测试期望值；`CUTOFF` 的 UTC 时刻由本机 PowerShell `[DateTimeOffset]::FromUnixTimeMilliseconds(1786896000000)` 换算为 `2026-08-16T16:00:00Z`。
7. **刷新与删除语义**：LiteLLM 刷新路径只有 `insert`（`pricing.rs:1080-1100`），未见 `remove`，被上游移除的模型可能继续以旧价出现；models.dev 装载内部确实会 `remove` 落败拼写的旧条目（`:1277-1284`），但那是同一份源内部的替换，不是「上游下线」的清理。结论基于全文 grep，未做运行时验证。
8. **statusline 路径的 offline 默认值与 `--no-offline` 合成规则**未逐行追踪（该路径另有按需加载的 `find_statusline_hook_context`，`ccusage/src/commands/mod.rs:716-765`）。
9. **各 agent 的模型名字形态差异**（是否已带 `-fast`、是否带 provider 前缀）会显著改变命中结果；本文只对 Claude / Codex / Pi 三条路径逐行核对，其余 adapter 仅确认调用同一套 `load_with_overrides` + `calculate_cost*`。
10. **`credits` 是否应计入「总额」**在任何地方都不成立（代码中只做展示与聚合），但各 adapter 的 credits 语义（Amp 与 Copilot 的定义是否一致）未逐一核对。
11. **兜底表的三处无条件写入**（`gpt-5.1-codex`、`gpt-5.2-codex`、`gpt-5.2`，`pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`）与 `:2010-2014` 的注释意图相反，源码未给出解释，无法判定是刻意锁定这三项还是遗漏；且 `gpt-5.1-codex` 这一处未套 flex 倍率，而 `gpt-5.2` 套了。
12. **Codex 服务档的「未分类 token」归属**：`codex/report.rs:250-271` 的 auto 分支把未分类用量并入当前档，其行为依赖各 Codex home 的 `config.toml` 探测（`codex/speed.rs:33-44`）；本机无 Codex 环境，未做端到端验证。
