# REVIEW-R2：S3 定价与金额分片 对抗式复核

> 复核对象：`docs/research/ccusage-draft/S3-pricing.md`（全文 587 行已读完）
> 复核仓库：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD 已确认为 `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（`git rev-parse HEAD` 实读）
> 复核立场：证伪优先。核验清单 80 条编号行（含互补核对与测试行号抽查），G1–G13 全部高风险项均逐条比对。行号一律以本克隆实际内容为准（`read` 工具报告的总行数与文档口径一致：pricing.rs 6181、cost.rs 664、build.rs 255）。
> 说明：`pricing.rs` 的 `(Get-Content | Measure-Object -Line)` 返回 5759 是**非空行**计数，与文档「6181 行」不冲突。

## 结论

分片整体质量高，可以充当定价主线的可靠底稿：URL/常量/倍率/窗口边界/公式结构等**易错的数字类断言几乎全中**，`is_embedded_model` 17 项白名单、fast/flex 覆盖表数值、DeepSeek V4 三段价与 UTC 半开窗口、`gpt-5.3-codex` 等条目行号、快照字节数（356/474/544 555/14 569 B）均逐字核对通过。

但有 **1 处语义级错误 + 4 处证据错位/错引 + 2 处不完整**，全部集中在「非数字类」的措辞与引用上，须修正后才可合并：

- 语义级：§2/§3.4 断言「内置兜底表只 `or_insert`、绝不覆盖上游」不成立——`put_builtin_pricing` 内 3 个 Codex 键走无条件 `entries.insert`。
- 证据级：§7 把 `UsageEntry.cost_usd` 的 serde 位置标为 `ccusage-cli/src/types.rs:14-15`；§4.5(d) 把内置 `gpt-5.4` 标为 `2431-2450`；§6.2 把日志 tier 拼写映射标为 `codex/speed.rs:14-44`；§6.6 用 `output.rs:704-713` 支撑「missing_pricing 仍可有正 cost」。
- 不完整：§5.3 flex 表以 `gpt-6-*` 概括，漏掉 `gpt-6.1-sol`；§3.4 兜底表清单漏 `gpt-5.6-sol/terra/luna`。

另有 2 处表述需收紧（G8 的 `max_by` tie 方向、§9 的进度条件行号），不改变结论但会被误读。

## 抽样核验清单

| 编号 | 分片位置 | 断言摘要 | 源码位置 | 结果 | 源码摘录 |
| --- | --- | --- | --- | --- | --- |
| 1 | §1.1（G1） | `is_embedded_model` 白名单 17 项，含 `us./eu./global./jp./au.anthropic.`、`o3`/`o3-*`、`o4-mini`/`o4-mini-*`、`zai/`、`openrouter/openai/` | `rust/crates/ccusage-core/build.rs:203-221` | 成立（逐项一致） | `fn is_embedded_model(model: &str) -> bool {` / `\|\| model.starts_with("zai/")` / `\|\| model.starts_with("openrouter/openai/")` |
| 2 | §1.1（G1） | 键改名 `i/o/cc/cr/ia/oa/cca/cra/ctx` + `fast/flex`；仅 `i` 与 `o` 同时存在才保留 | `build.rs:168-198` | 成立；遗漏 `!value.is_null()` 与 fast/flex 的 null 过滤（`:182-184`、`:190-191`） | `("max_input_tokens", "ctx"),` / `if fields.contains_key("i") && fields.contains_key("o") {` |
| 3 | §1.1 | `compact_pricing_json` 155-201、deflate 60-62、panic 路径 75-82、URL 拼接 107-136 | `build.rs:60-62`、`:75-82`、`:107-136`、`:155-201` | 成立 | `miniz_oxide::deflate::compress_to_vec(bytes, 10)` / `fn compact_pricing_json(json: &str) -> Option<String> {` |
| 4 | §1.1 | flake.lock 两 pin 与 rev 值 | `flake.lock:143-145`、`:159-161` | 成立 | `"rev": "d9123f459561754f41fd598a73b405b091d3bf84",` / `"rev": "9cb380733df864b6538c7d6a42111d28897901f5",` |
| 5 | §1.2 | 两份已提交快照尺寸与 catalog-rules 字段 | `src/models-dev-pricing.json`（544 555 B / 34 269 行）、`src/models-dev-catalog-rules.json`（14 569 B）；字段 `pricing.rs:596-617` | 成立 | `owners: FxHashSet<String>,` / `#[serde(rename = "authoredModelIds")]` / `asset_priced_model_ids: FxHashSet<String>,` |
| 6 | §1.3（G2） | `LITELLM_PRICING_URL` / `MODELS_DEV_API_URL` / `MODELS_DEV_FAILURE_RETRY_AFTER` 三常量 | `pricing.rs:55-58` | 成立（字符串与 60s 逐字一致） | `"https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";` / `const MODELS_DEV_API_URL: &str = "https://models.dev/api.json";` / `const MODELS_DEV_FAILURE_RETRY_AFTER: Duration = Duration::from_secs(60);` |
| 7 | §1.3 | ETag 缓存目录/文件名/写入/304 重取/体积上限 | `rust/crates/ccusage/src/http.rs:37-52`、`:165-169`、`:199-229`、`:237-257`、`:88-99`、`:11-12`、`:104-151` | 成立 | `let tmp_path = self.path.with_file_name(format!(` / `match fs::write(&tmp_path, format!("{etag}\n{body}")) {` / `format!("{name}-{hash:016x}")` |
| 8 | §1.3 | 传输错误不复用缓存体（测试 684-695） | `http.rs:683-703` | 成立 | `fn transport_errors_do_not_reuse_cached_body() {` / `"a transport failure must not turn an unbounded-age cache into a response"` |
| 9 | §1.4 | `ConfigPricingOverride` schema 与 camelCase 字段 | `ccusage-config/src/config_schema.rs:593-607`、使用点 `:398`、`:509` | 成立 | `pub struct ConfigPricingOverride {` / `pub cache_read_input_token_cost_above_200k_tokens: Option<f64>,` / `pub pricing_overrides: Option<BTreeMap<String, ConfigPricingOverride>>,` |
| 10 | §1.4 | 配置发现顺序与「第一个可读 JSON 对象」 | `ccusage-config/src/config.rs:246-299` | 成立 | `paths.push(cwd.join(".ccusage").join("ccusage.json"));` / `.find(\|value\| value.as_object().is_some())` |
| 11 | §2（G3） | `load_with_overrides` 五步顺序；`enable_models_dev_fallback = !offline` 位置 | `pricing.rs:1002-1038` | 成立（顺序与行号全对） | `map.fill_long_context_rates_from_models_dev();` / `map.enable_models_dev_fallback = !offline;` / `map.apply_overrides(overrides);` |
| 12 | §2（G3） | `load_embedded` 四步：内嵌 → 兜底 → 补档位 → 开内嵌 models.dev | `pricing.rs:982-1000` | 成立 | `map.put_builtin_pricing(&fast_multiplier_overrides, &flex_multiplier_overrides);` / `map.fill_long_context_rates_from_models_dev();` / `map.enable_embedded_models_dev_fallback = true;` |
| 13 | §2 | 长上下文档位「四个 `*_above_200k` 全空才铺」 | `pricing.rs:1946-1978` | 成立 | `if pricing.input_above_200k.is_some()` / `\|\| pricing.cache_read_above_200k.is_some()` / `continue;` |
| 14 | §2 | 内置兜底表「`entry(...).or_insert(pricing)`，绝不覆盖上游」 | `pricing.rs:1980-1983` vs `:2369-2370`、`:2386-2387`、`:2400-2408` | **不成立**（3 键无条件覆盖） | `self.entries` / `.insert("gpt-5.1-codex".to_string(), gpt_5_1_pricing);` / `self.entries.insert("gpt-5.2".to_string(),` |
| 15 | §2 | 用户 override 缩放判定 `!cache_read_explicit` | `pricing.rs:1838-1845` | 成立；遗漏 `base.input > 0.0` 前提 | `let should_scale = override_value.input_cost_per_token.is_some()` / `&& base.input > 0.0` / `&& !base.cache_read_explicit;` |
| 16 | §3.1 | `Pricing` 全字段与 `DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS = 200_000` | `pricing.rs:196-220` | 成立 | `pub(crate) long_context_threshold: Option<u64>,` / `pub(crate) const DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS: u64 = 200_000;` |
| 17 | §3.2 | `LiteLlmPricing` / `CompactLiteLlmPricing` / models.dev 形状；单价除 1e6 | `pricing.rs:540-573`、`:834-877`、`:909-912` | 成立 | `struct CompactLiteLlmPricing {` / `i: f64,` / `fn per_token(per_million: f64) -> f64 { per_million / 1_000_000.0 }` |
| 18 | §3.3（G4） | `cache_create = input*1.25` / `cache_read = input*0.1` 默认 | `pricing.rs:1085-1088`、models.dev 侧 `:1294-1301` | 成立 | `.unwrap_or(input * 1.25),` / `cache_read: pricing.cache_read_input_token_cost.unwrap_or(input * 0.1),` |
| 19 | §3.4 | 兜底表键清单与上下文上限块 | `pricing.rs:2015-2593`、`:2557-2591` | 部分：漏列 `gpt-5.6-sol/terra/luna`（`:2475-2500`）；`gpt-5.3-codex` 2388-2400 正确 | `("gpt-5.6-sol", 5e-6, 30e-6, 6.25e-6, 0.5e-6),` / `self.context_limits.insert("gpt-5.5".to_string(), 1_050_000);` |
| 20 | §3.4 | GLM 走 `put_builtin_glm`，只补缓存字段、不覆盖显式缓存价 | `pricing.rs:1990-2008` | 成立 | `if !existing.cache_read_explicit {` / `existing.cache_read = pricing.cache_read;` |
| 21 | §4.1（G7） | 查找链：精确段 → fuzzy 判定 → 本表(model/alias) → 在线 models.dev → 内嵌 models.dev | `pricing.rs:1393-1454` | 成立（顺序与行号一致） | `if let Some(pricing) = self` / `.find_entry_or_alias_with_fallback(model, Fuzzy::Denied, None)` / `.or_else(\|\| { embedded_models_dev.and_then(\|pricing\| pricing.find_entry_or_alias(resolved_alias, fuzzy)) })` |
| 22 | §4.1 | 精确段命中即返回（「a 命中后 b、c 不再尝试」） | `pricing.rs:1405-1416` | 成立 | `{` / `return Some(pricing);` / `}` |
| 23 | §4.1 | `find_entry_or_alias_with_fallback` 1559-1574 与 `find_entry_with_fallback` 1586-1615 的 4 步 | `pricing.rs:1559-1615` | 成立 | `.or_else(\|\| { pricing_alias(model).and_then(\|alias\| {` / `if fuzzy == Fuzzy::Denied` / `\|\| self.is_exact_only_lookup_with_fallback(model, embedded_models_dev)` |
| 24 | §4.1（G8） | `max_by` 排序键「键长降序 → 键名字典逆序」 | `pricing.rs:1610-1612` | 表述有歧义：闭包为长度升序比较取最大（最长键胜），同长度时 `right.cmp(left)` 反转 ⇒ **字典序小者胜** | `.max_by(\|(left, _), (right, _)\| {` / `left.len().cmp(&right.len()).then_with(\|\| right.cmp(left))` / `})` |
| 25 | §4.2 | `normalized_pricing_key` 只换 `.`/`@`、不折叠大小写；models.dev 装载侧另用会 lowercase 的版本 | `pricing.rs:2724-2731`、`:750-759`、`:1239` | 成立 | `Cow::Owned(value.replace(['.', '@'], "-"))` / `Cow::Owned(model_id.to_lowercase().replace(['.', '@'], "-"))` |
| 26 | §4.3（G6） | `pricing_key_matches` 四分支与边界判定 | `pricing.rs:2667-2701` | 成立 | `if contains_pricing_key(model, candidate) \|\| contains_pricing_key(candidate, model) {` / `before.is_none_or(is_pricing_key_boundary) && suffix_allows_pricing_key_match(key, suffix)` |
| 27 | §4.3（G6） | `suffix_starts_with_numeric_model_version` 规则与 8 位日期例外；`MODEL_DATE_SUFFIX_DIGITS = 8` | `pricing.rs:2703-2722`、常量 `:59-61` | 成立（唯一使用点 `:2721`） | `!(digit_len == MODEL_DATE_SUFFIX_DIGITS && after_digits.is_none_or(is_pricing_key_boundary))` / `const MODEL_DATE_SUFFIX_DIGITS: usize = 8;` |
| 28 | §4.3 | 版本边界与日期后缀测试 | `pricing.rs:5705-5732`、`:5734-5739` | 成立（逐字一致） | `fn fuzzy_match_does_not_fall_back_across_numeric_model_versions() {` / `assert!(pricing.find("claude-opus-4.8-20260528").is_none());` / `assert!(pricing.find("claude-opus-4-20250514").is_some());` |
| 29 | §4.3 | 全长优先测试 | `pricing.rs:5853-5898` | 成立 | `fn fuzzy_match_prefers_longest_model_key() {` / `pricing.find("claude-sonnet-4-20250514-via-bedrock")` / `assert_eq!(matched.input, 2.0);` |
| 30 | §4.4（G12） | `ExactOnlyKeys` 双索语义与三个查询 | `pricing.rs:443-521` | 成立 | `fn contains_any_spelling(&self, key: &str) -> bool {` / `fn id_spelled_by(&self, key: &str) -> Option<&str> {` |
| 31 | §4.4（G12） | 门禁跨数据源：查询名或静态别名属 exact-only 时禁用 fuzzy | `pricing.rs:1646-1654`、`:1664-1683`、注释 `:1617-1637` | 成立 | `self.exact_only.contains_any_spelling(model)` / `\|\| embedded_models_dev.is_some_and(\|pricing\| pricing.exact_only.contains_any_spelling(model))` / `if requires_exact(model) \|\| (resolved_alias != model && requires_exact(resolved_alias)) {` |
| 32 | §4.4（G12） | 标记来源：内嵌 `exactOnly` 字段 + 在线按生成期规则重推 | `pricing.rs:840-843`、`:674-698`、`:761-768`、`:1223-1226` | 成立 | `#[serde(rename = "exactOnly")]` / `self.is_tier_variant_of_authored_model(source_model_id)` / `\|\| is_unversioned_models_dev_model_id(pricing_key)` |
| 33 | §4.4（G12） | exact-only 测试行号 | `pricing.rs:3342-3368`、`:3371-3453`、`:3480-3536`、`:3648-3704`、`:4702-4728` | 成立（抽查首条） | `fn exact_only_fallback_beats_a_fuzzy_primary_match() {` / `fn live_exact_only_fallback_beats_a_fuzzy_primary_match() {` |
| 34 | §4.5(a) | `claude-sonnet-4-20250514` 等 4 个 id 存在且 input 一致 | `pricing.rs:4931-4955` | 成立 | `fn embedded_pricing_resolves_overlapping_model_keys_exactly() {` / `pricing.find("claude-3-5-haiku-20241022").unwrap().input,` / `0.8e-6,` |
| 35 | §4.5(b) | `claude-opus-4-6-fast` 的 fuzzy 推演与 fast 倍率 6.0 来自 `normalized_prefix` | `fast-multiplier-overrides.json:14-18`、`pricing.rs:944-978`、`:2802-2808`；测试 `:5782-5842` | 成立 | `"claude-opus-4-6": 6.0,` / `matches_model_suffix(&normalized, base).then_some(*multiplier)` / `let suffix = &part[index..];` |
| 36 | §4.5(c) | `gpt-5.3-codex` 内置键 2388-2400、fast=2.0、`gpt-5.2-codex` 保持 1.0 | `pricing.rs:2388-2399`、`:5741-5780`、`:5418-5431` | 成立 | `fn fills_codex_fast_multiplier_when_litellm_pricing_omits_it() {` / `assert_eq!(pricing.find("gpt-5.3-codex").unwrap().fast_multiplier, 2.0);` / `assert_eq!(pricing.find("gpt-5.2-codex").unwrap().fast_multiplier, 1.0);` |
| 37 | §4.5(d) | pi 查找顺序 `override → find_at/find → display find_exact → raw find_at/find` | `rust/adapters/pi/src/parser.rs:685-695`、`:697-708`、`:629-638` | 成立 | `pricing_override_model(raw_model, display_model, Some(pricing))` / `.or_else(\|\| display_model.and_then(\|model\| pricing.find_exact(model)))` / `.or_else(\|\| raw_model.and_then(\|model\| pricing.find_at(model, timestamp)))` |
| 38 | §4.5(d) | 无 override 时 `gpt-5.4` 命中内置条目「`pricing.rs:2431-2450`」 | 内置 `gpt-5.4` 实际在 `pricing.rs:2409-2430`；2431-2450 是 `gpt-5.4-mini` | **行号错** | `self.put_builtin_entry(` / `"gpt-5.4".to_string(),` / `..` 见 `:2409-2410` |
| 39 | §5.1 | `CCUSAGE_MODEL_ALIASES` 解析与 `-fast` 回退 | `ccusage-core/src/model_aliases.rs:8`、`:14-26`、`:39-61` | 成立；遗漏 `alias.is_empty()` 过滤（`:17`、`:21`） | `if let Some(base_model) = model.strip_suffix("-fast")` / `return Cow::Owned(format!("{alias}-fast"));` |
| 40 | §5.2 | `pricing_alias` 三条静态别名 | `pricing.rs:2792-2800` | 成立（逐条一致） | `"gpt-reserve" => Some("gpt-5.6-luna"),` / `"gpt-5.3-spark" => Some("gpt-5.3-codex-spark"),` |
| 41 | §5.3（G5） | fast 表 `exact` 10 条 + `normalized_prefix` 3 条 | `fast-multiplier-overrides.json:2-18` | 成立（值与键逐条一致） | `"gpt-5.6-sol": 2.0,` … `"gpt-5.5": 2.5,` … `"claude-opus-4-8": 2.0` |
| 42 | §5.3（G5） | flex 表全部 0.5、`normalized_prefix` 空；用 `gpt-6-*` 概括 | `flex-multiplier-overrides.json:2-24` | **不完整**：文件为 20 个**精确键**，`gpt-6-*` 概括漏 `gpt-6.1-sol` | `"gpt-6.1-sol": 0.5,` / `"normalized_prefix": {}` |
| 43 | §5.3 | 上游 `provider_specific_entry.fast/flex` 优先于内嵌覆盖表 | `pricing.rs:1072-1079`、`:1309-1314` | 成立 | `.or_else(\|\| fast_multiplier_overrides.multiplier_for(&model))` / `.unwrap_or(1.0);` |
| 44 | §6.1（G9） | 档位 A 整单切换（`context > threshold`）、档位 B 默认 200K 边际 | `cost.rs:143-168`、`:170-198` | 成立 | `let long_context = context_tokens > threshold;` / `let threshold = crate::pricing::DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS;` |
| 45 | §6.1（G9） | `tiered_cost` 边界 `tokens > threshold`（严格大于、边际） | `cost.rs:201-211` | 成立 | `if let Some(above) = above` / `&& tokens > threshold` / `return (threshold as f64 * base) + ((tokens - threshold) as f64 * above);` |
| 46 | §6.1（G9） | 1h 缓存写入 `input × 2`，与 `cache_create` 无关 | `cost.rs:7`、`:128-131` | 成立 | `const CACHE_CREATE_1H_INPUT_MULTIPLIER: f64 = 2.0;` / `let cache_create_1h_cost = pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER;` |
| 47 | §6.1 | 数值自证 5m=10/1h=20/cache_read=30 → 55.5；阈值 100 → 93/94/190e-6；grok 0.352/0.056 | `cost.rs:317-338`、`:358-434`、`:436-478` | 成立（复算一致） | `assert!((cost - 55.5).abs() < f64::EPSILON);` / `assert!((cost - 94e-6).abs() < 1e-12, "boundary cost was {cost}");` / `assert!((cost - 0.352).abs() < 1e-9, "cached-heavy cost was {cost}");` |
| 48 | §6.2 | Codex 双桶公式与缓存读回退 | `adapters/codex/src/report.rs:343-392` | 成立（逐项一致） | `let cache_read = if pricing.cache_read_explicit {` / `pricing.input` / `let long_cache_read = if pricing.cache_read_explicit {` |
| 49 | §6.2 | fast/flex 桶增量公式与 auto 归桶 | `codex/report.rs:234-275` | 成立 | `+ calculate_codex_bucket_cost(&flex_usage, pricing) * (pricing.flex_multiplier - 1.0)` / `CodexSpeedPolicy::Auto(CodexServiceTier::Fast) => (` / `add_codex_usage_buckets(recorded_fast_usage, unclassified_usage),` |
| 50 | §6.2 | long 桶按每条请求切分、阈值来自 models.dev 快照缺省 200K | `codex/aggregate.rs:476-487`、`pricing.rs:2753-2763` | 成立 | `let is_long_context = event.input_tokens > crate::pricing::long_context_split_threshold(model);` / `.unwrap_or(DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS)` |
| 51 | §6.2 | tier 拼写映射证据「`codex/report.rs:250-271`、`codex/speed.rs:14-44`、`codex/src/README.md:35-39`」 | 映射实为 `codex/parser.rs:649-659`；`speed.rs:14-44` 是 `--speed` 与 config.toml | **证据错位**（README 35-39 正确） | `"default" \| "standard" => Some(CodexServiceTier::Standard),` / `"fast" \| "priority" => Some(CodexServiceTier::Fast),` / `"flex" => Some(CodexServiceTier::Flex),` |
| 52 | §6.4 | `apply_total_token_fallback` 两条分支 | `ccusage-core/src/utils.rs:22-38` | 成立（逻辑逐行一致） | `if usage.output_tokens == 0 {` / `usage.output_tokens = missing_tokens;` / `extra_total_tokens = extra_total_tokens.saturating_add(missing_tokens);` |
| 53 | §6.4 | 触发点 Kimi/Pi；展示端计入 totalTokens；字段 skip_serializing | `kimi/src/parser.rs:208`、`pi/src/parser.rs:348`、`types.rs:89-95`、`:106-107`、`:115-121`、`:168-169`、`:183-189` | 成立（行号全部命中） | `let (usage, extra_total_tokens) = apply_total_token_fallback(usage, 0, 0);` / `#[serde(skip_serializing)]` / `.saturating_add(self.extra_total_tokens)` |
| 54 | §6.5 | credits 来源、累加与呈现（裸 `{:.2}`） | `amp/src/parser.rs:212`、`:320`、`codebuff/src/loader.rs:84`、`output.rs:51-52`、`:73-74`、`:112-114`、`common/src/report.rs:45`、`:70`、`:108` | 成立（9 处行号全对） | `credits: (entry.credits > 0.0).then_some(entry.credits),` / `let credits = rows.iter().filter_map(\|row\| row.credits).sum::<f64>();` / `format!("{:.2}", row.credits.unwrap_or_default()),` |
| 55 | §6.6 | 三处缺价披露与两段 WARN 文案 | `output.rs:430-450`、`:126-145`、`types.rs:109-111` | 成立（文案逐字一致） | `"WARN  Missing embedded pricing for {model}; cost excludes this model. Run without --offline or update ccusage after pricing is added."` / `"WARN  Missing pricing for {model}; cost excludes this model. Update pricing or run again after LiteLLM has the model."` |
| 56 | §6.6 | 判定链 `missing_pricing_model_for_usage` → `…_for_token_total` | `cost.rs:48-75` | 成立 | `if mode == CostMode::Display \|\| (mode == CostMode::Auto && cost_usd.is_some()) {` / `if total_tokens == 0 {` |
| 57 | §6.6 | 「`missing_pricing = true` 的 breakdown 仍可能有正 cost（测试 `output.rs:704-713`、注释 `:123-125`）」 | 注释 `output.rs:123-125` 成立；`704-715` 是 helper 且 true 分支 cost 固定 0.0 | **证据不成立** | `/// still show positive cost when other entries carried stored costs.` / `cost: if missing_pricing { 0.0 } else { 0.25 },` |
| 58 | §7（G10） | `CostMode` 三态定义与 `#[default] Auto` | `ccusage-cli/src/types.rs:242-248` | 成立 | `pub enum CostMode {` / `#[default]` / `Display,` |
| 59 | §7（G10） | auto 判定依据 `UsageEntry.cost_usd`（rename `costUSD`）位于 `ccusage-cli/src/types.rs:14-15` | 实为 `ccusage-core/src/types.rs:14-15`；cli 的 14-15 是 `Command` 成员 | **文件错** | `#[serde(rename = "costUSD")]` / `pub cost_usd: Option<f64>,` / （cli 侧对照）`Codex(AgentCommandArgs),` |
| 60 | §7（G10） | Claude 在 Display 模式不加载价格表 | `claude/src/lib.rs:144-152` | 成立 | `let pricing = if shared.mode == CostMode::Display {` / `None` / `Some(PricingMap::load_with_overrides(` |
| 61 | §7 | `with_defaults` 设 `mode: Auto`；CLI 三态文本映射；config `From` | `ccusage-cli/src/types.rs:62-71`、`ccusage-cli-parser/src/parser.rs:1136-1138`、`ccusage-config/src/config.rs:703-711` | 成立 | `mode: CostMode::Auto,` / `"calculate" => Ok(CostMode::Calculate),` / `ConfigCostMode::Display => Self::Display,` |
| 62 | §7 | Pi 任何模式都加载；auto 记录价过滤 + override 强制重算 | `pi/src/lib.rs:21-27`、`pi/src/parser.rs:640-650` | 成立 | `CostMode::Auto if has_override => None,` / `CostMode::Auto => display_cost.filter(\|cost\| cost.is_finite() && *cost >= 0.0),` |
| 63 | §8（G11） | `DEEPSEEK_V4_PRICING_CUTOFF_MS = 1_786_896_000_000`（= 2026-08-16T16:00:00Z） | `pricing.rs:252`；测试 `:2989-3033` | 成立（PowerShell `[DateTimeOffset]::FromUnixTimeMilliseconds` 复算得 `2026-08-16T16:00:00Z`） | `const DEEPSEEK_V4_PRICING_CUTOFF_MS: i64 = 1_786_896_000_000;` / `"2026-08-16T16:00:00Z",` / `[0.22, 0.66, 0.007],` |
| 64 | §8（G11） | 三段价六组数值全部正确 | `pricing.rs:279-331` | 成立（flash 0.14/0.28、0.22/0.66、0.44/1.32；pro 0.435/0.87、0.66/1.98、1.32/3.96） | `input: 1.32e-6,` / `output: 3.96e-6,` / `cache_read: 0.044e-6,`（pro peak，`:314-319`） |
| 65 | §8（G11） | 高峰窗口：UTC、周六周日 off-peak、工作日 `[1,4)` 或 `[6,10)` 半开 | `pricing.rs:333-347`、测试 `:3035-3057` | 成立 | `if !(1..=5).contains(&weekday_from_sunday) {` / `(1..4).contains(&hour) \|\| (6..10).contains(&hour)` |
| 66 | §8（G11） | `has_time_dependent_pricing` 只认归一化后的两个 id（含别名转发） | `pricing.rs:262-277`；测试 `:3114-3136` | 成立（`openrouter/…` 前缀不参与） | `deepseek_v4_model_identity(model)` / `.or_else(\|\| deepseek_v4_model_identity(resolved_model.as_ref()))` / `assert!(!super::has_time_dependent_pricing(model));` |
| 67 | §8 | `apply_deepseek_v4_schedule` 覆盖四价并重铺 4 个 above、置 explicit | `pricing.rs:349-368` | 成立 | `pricing.input_above_200k = Some(rates.input);` / `pricing.cache_create_explicit = true;` |
| 68 | §8 | `find_at` 三步（非 V4 直落 find；调度；user_overrides 三重查找） | `pricing.rs:1461-1482` | 成立 | `else { return self.find(model); };` / `.or_else(\|\| self.user_overrides.get(scheduled_model));` / `apply_explicit_pricing_override(&mut pricing, override_value);` |
| 69 | §9 | `PricingMap` 字段表（FxHashMap、三种 OnceLock） | `pricing.rs:413-441`、`fast.rs:4` | 成立 | `find_cache: OnceLock<Mutex<FxHashMap<String, Option<Pricing>>>>,` / `normalized_keys: OnceLock<FxHashMap<String, Option<String>>>,` |
| 70 | §9 | `clear_find_cache` 6 个调用点 | `pricing.rs:1106`、`:1350`、`:1817`、`:1977`、`:1982`、`:2007` | 成立（grep 恰好 6 处，与文档一致） | `self.normalized_keys.take();` |
| 71 | §9 | 每次装载重新 serde 两份 override JSON；文件 356/474 B | `pricing.rs:1041-1045`、`:1112-1115`；文件尺寸实测 | 成立 | `let fast_multiplier_overrides = SpeedMultiplierOverrides::load(FAST_MULTIPLIER_OVERRIDES_JSON);` |
| 72 | §9 | 进度文案与 WARN 门控：条件「`log && usage_load_output_is_tty()`」归 `progress.rs:295-296` | 定价刷新条件在 `pricing.rs:1009`；`progress.rs:295-296` 属 `track_usage_load` | **行号偏差** | `log && crate::progress::usage_load_output_is_tty(),` / （progress 侧）`&& should_show_usage_load_progress(json, usage_load_output_is_tty());` |
| 73 | §10 | `--no-offline` 合成规则与 statusline 默认离线 | `ccusage/src/commands/mod.rs:380`、`ccusage-cli/src/types.rs:221-239` | 成立 | `offline: args.offline && !args.no_offline,` / `offline: true,` |
| 74 | §10 | HTTP 客户端安装点 `ccusage/src/main.rs:26` | `ccusage/src/main.rs:26` | 成立 | `pricing::set_json_fetcher(http::fetch_json);` |
| 75 | §10 | offline 内嵌 models.dev 仍启用 | `pricing.rs:998`、`:1035`；测试 `:3290-3304`、`:3934-3946` | 成立 | `map.enable_embedded_models_dev_fallback = true;` / `fn keeps_models_dev_fallback_disabled_for_embedded_and_offline_pricing() {` |
| 76 | §11 | `format_currency` / `json_float` / `format_number` / `strip_cost_json` | `output.rs:548-550`、`:452-462`、`:536-546`、`:564-579` | 成立 | `pub fn format_currency(value: f64) -> String { format!("${value:.2}") }` / `json!(value as i64)` |
| 77 | §11 | `json_float` 用于 Codex `costUSD`、adapter-all `totalCost`、Amp credits | `codex/report.rs:179`、`ccusage-adapter-all/src/report.rs:169`、`:196`、`loader.rs:813` | 成立 | `"costUSD": json_float(cost),` / `"totalCost": json_float(row.total_cost),` / `metadata.insert("credits".to_string(), json_float(credits));` |
| 78 | §13（G13，原表未列） | 文档未给出 trust rank 数值与方向 | `pricing.rs:649-651`、`:654-662`、`:1126-1137` | **未断言**（源码 owner=3、platform=2、reseller=1，排序为高信任先） | `const MODELS_DEV_TRUST_OWNER: u8 = 3;` / `const MODELS_DEV_TRUST_RESELLER: u8 = 1;` / `.rank(&right_id).cmp(&rules.rank(&left_id))` |
| 79 | §1.2 | models.dev 装载「只补主表没有的 id」与同拼写竞争 | `pricing.rs:1241-1243`、`:1259-1284` | 成立 | `if claimed.is_none() && self.entries.contains_key(&model_id) {` / `self.entries.remove(&previous.stored_id);` |
| 80 | §2 | `models.dev` 单条装载内部「信任等级 + 详细度」替换 | `pricing.rs:1241-1243`、`:1259-1284`、`:626-633` | 成立（`ModelsDevClaim` 字段顺序即比较顺序） | `trust: u8,` / `has_long_context_tier: bool,` / `has_cache_read: bool,` |

（上表 80 行中多处为同一 G 项的互补核对；核心必查 G1–G13 全部覆盖。）

## 必须修正

1. **§2 表格「内置兜底表 … 绝不覆盖上游」与 §3.4「只在快照缺键时兜底」不成立。** `put_builtin_pricing` 内三处走无条件 `self.entries.insert`：
   `pricing.rs:2369-2370`（`gpt-5.1-codex`）、`:2386-2387`（`gpt-5.2-codex`）、`:2400-2408`（`gpt-5.2`）。
   即：上游快照已给这三个键定价时，兜底值仍会覆盖它，与注释 `:2010-2014` 声明的意图相反。修法：把断言改成「除 `gpt-5.1-codex` / `gpt-5.2-codex` / `gpt-5.2` 三个无条件写入键外，其余走 `entry().or_insert()`」，或直接点名这三处例外并说明理由。
2. **§7 证据行号文件错。** `UsageEntry.cost_usd` 的 `#[serde(rename = "costUSD")]` 在 `rust/crates/ccusage-core/src/types.rs:14-15`；文档写的 `ccusage-cli/src/types.rs:14-15` 是 `Command` 枚举的两行成员（`Codex(...)`、`OpenCode(...)`）。
3. **§4.5(d) 行号错。** 内置 `gpt-5.4` 条目在 `pricing.rs:2409-2430`；文档写的 `2431-2450` 是 `gpt-5.4-mini`。
4. **§6.6 证据不成立。** 「`missing_pricing = true` 的 breakdown 仍可能有正 cost」的注释支撑是 `output.rs:123-125`；但所引 `output.rs:704-713` 是测试 helper `fn breakdown(model, missing_pricing)`，其 `missing_pricing == true` 分支把 cost 固定为 `0.0`，不支持该断言。建议只保留注释引用，或改引能同时出现 `missingPricing: true` 与正 `cost` 的断言点。
5. **§6.2 证据错位。** 日志 `thread_settings.service_tier` 的拼写映射（`default`/`standard`、`fast`/`priority`、`flex`）在 `rust/adapters/codex/src/parser.rs:649-659`；`codex/speed.rs:14-44` 只处理 `--speed` 枚举与各 Codex home 的 `config.toml`。`codex/src/README.md:35-39` 引对了。
6. **§5.3 flex 表不完整。** `flex-multiplier-overrides.json` 是 **20 个精确键**（`gpt-6-astra`、`gpt-6.1-sol`、`gpt-6-luna`、`gpt-6-sol`、`gpt-5.6-sol/terra/luna`、`gpt-5.5`、`gpt-5.5-pro`、`gpt-5.4`、`gpt-5.4-mini/-nano/-pro`、`gpt-5.2`、`gpt-5.1`、`gpt-5`、`gpt-5-mini`、`gpt-5-nano`、`o3`、`o4-mini`），全部 0.5。用 `gpt-6-*` 概括会漏掉 `gpt-6.1-sol`（该键不匹配 `gpt-6-*` 形态）。G5 要求「全部条目逐条一致」，应按 fast 表的写法逐条列出。
7. **§9 条件行号偏差（轻微但要求精确）。** 定价刷新的 TTY 条件在 `pricing.rs:1009`（`log && crate::progress::usage_load_output_is_tty()`）；`progress.rs:295-296` 是 `track_usage_load` 自己的条件，与定价刷新无关。

## 建议补充

- **G8 表述收紧（不改结论）**：闭包是 `left.len().cmp(&right.len()).then_with(|| right.cmp(left))`。`max_by` 在比较为 `Less` 时取后者，因此语义是「最长键优先；同长度时**字典序较小**者胜」。文档「键长降序 → 键名字典逆序」两处短语都可被读成相反方向，建议写成「同长度按字典序反序比较 ⇒ 字典序小者胜」，并补一句 `context_limits` 的同一模式（`pricing.rs:1803-1805`）。
- **G13 补数值**：`MODELS_DEV_TRUST_OWNER = 3`、`PLATFORM = 2`、`RESELLER = 1`（`pricing.rs:649-651`），排序为高信任优先（`:1126-1132`），同级按声明 id 再按 map key。
- **§3.4 兜底表清单**补 `gpt-5.6-sol` / `gpt-5.6-terra` / `gpt-5.6-luna`（`pricing.rs:2475-2500`）。
- **§1.1 键映射**补两个过滤条件：值非 null（`build.rs:182-184`）与 fast/flex 非 null（`:190-191`）；并注明 `provider_specific_entry` 仅在压缩形状里由 `fast.or(flex)` 重建（`pricing.rs:2612-2618`）。
- **§2 用户 override 缩放**补前提 `base.input > 0.0`（`pricing.rs:1839`）；`apply_override` 会把 `cache_read_explicit` / `cache_create_explicit` 与 base 取或（`:1890-1893`）。
- **§4.4** 可点明三分查询语义：`contains`（fuzzy 扫描问候选键）、`contains_any_spelling`（请求名门禁）、`id_spelled_by`（决定哪个条目作答）（`pricing.rs:501-521`）。
- **§6.1** 可补 `tiered_cost` 的 `tokens == 0` 早退（`cost.rs:202-204`）与 `saturating_add` 的溢出语义（`:147-151`）。
- **§8** 可补 `apply_explicit_pricing_override`（`pricing.rs:370-403`）只覆盖显式给出的字段，因此调度结果与用户字段是「字段级」叠加，而非整条替换。
- **§1.2/§9** 可补 models.dev 装载会 `remove` 落败拼写的旧条目（`pricing.rs:1277-1284`）与据此清 `context_limits`（`:1328-1330`）。

## 无法验证

1. 内嵌 LiteLLM 快照（`$OUT_DIR/litellm-pricing.json.deflate`）的真实键集——本次遵守只读约束未构建，`claude-sonnet-4-20250514` 等的存在性只能靠测试断言间接确认（分片 §未验证 1 已披露同一限制）。
2. `models-dev-pricing.json` 34 269 行的全量一致性——本次只抽查 8 处条目行号（3932 / 7341 / 7504 / 7598 / 7634 / 15868 / 15900 / 30696）与 4 个尺寸数字。
3. 两个在线端点（LiteLLM `main`、models.dev `api.json`）的真实响应及其与内嵌快照的差异。
4. `format_currency` 的 `{:.2}` 在 `x.xx5` 上的舍入模式——未跑实验，分片自述同此限制。
5. models.dev 生成器实现（`nix/tools/models-dev-gen`）与快照生成期规则的逐条对齐——只核了运行期 `is_exact_only` 的对应逻辑。
6. `find_cache` 在并发下的「锁内读 → 锁外算 → 锁内写回」压力语义——仅静态审读。

## 我实际打开过的文件

- `rust/crates/ccusage-core/build.rs:17-40`、`:46-62`、`:68-105`、`:107-153`、`:155-255`
- `rust/crates/ccusage-core/src/pricing.rs:1-120`、`:190-389`、`:400-619`、`:620-979`、`:981-1108`、`:1110-1352`、`:1354-1454`、`:1461-1482`、`:1518-1548`、`:1559-1615`、`:1617-1683`、`:1725-2024`、`:2025-2144`、`:2145-2304`、`:2325-2408`、`:2409-2499`、`:2540-2639`、`:2640-2819`、`:2790-2899`、`:2985-3157`、`:3286-3380`、`:3930-3954`、`:4820-4879`、`:4920-4964`、`:5415-5439`、`:5700-5744`、`:5741-5810`、`:5835-5898`、`:6015-6128`
- `rust/crates/ccusage-core/src/cost.rs:1-230`、`:231-510`
- `rust/crates/ccusage-core/src/types.rs:1-200`
- `rust/crates/ccusage-core/src/utils.rs:1-45`
- `rust/crates/ccusage-core/src/output.rs:108-157`、`:430-499`、`:690-809`
- `rust/crates/ccusage-core/src/progress.rs:85-96`、`:285-324`
- `rust/crates/ccusage-core/src/model_aliases.rs:1-150`
- `rust/crates/ccusage-core/src/fast-multiplier-overrides.json`（全文 19 行）、`flex-multiplier-overrides.json`（全文 25 行）
- `rust/crates/ccusage-core/src/models-dev-pricing.json:3932`、`:7341`、`:7504`、`:7598`、`:7634`、`:15868`、`:15900`、`:30696`
- `rust/crates/ccusage/src/http.rs:1-270`、`:668-767`
- `rust/crates/ccusage/src/main.rs:14-48`
- `rust/crates/ccusage/src/commands/mod.rs:370-394`
- `rust/crates/ccusage-cli/src/types.rs:5-115`、`:205-264`
- `rust/crates/ccusage-cli-parser/src/parser.rs:1128-1147`
- `rust/crates/ccusage-config/src/config.rs:244-303`、`:620-679`、`:695-724`
- `rust/crates/ccusage-config/src/config_schema.rs:588-617`
- `rust/crates/ccusage-adapter-all/src/loader.rs:75-99`、`:848-867`
- `rust/crates/ccusage-adapter-all/src/report.rs:160-204`
- `rust/adapters/claude/src/lib.rs:130-179`、`:515-554`
- `rust/adapters/codex/src/report.rs:170-289`、`:325-399`
- `rust/adapters/codex/src/aggregate.rs:465-519`
- `rust/adapters/codex/src/parser.rs:645-664`
- `rust/adapters/codex/src/speed.rs:1-60`
- `rust/adapters/codex/src/README.md:20-65`
- `rust/adapters/pi/src/lib.rs:10-44`
- `rust/adapters/pi/src/parser.rs:220-239`、`:345-364`、`:620-729`
- `rust/adapters/kimi/src/parser.rs:200-219`
- `rust/adapters/common/src/report.rs:40-114`
- `flake.lock:130-169`
- `docs/guide/cost-modes.md:1-110`
- `docs/guide/config-files.md:400-469`

（附：文件尺寸实测 `fast-multiplier-overrides.json` 356 B、`flex-multiplier-overrides.json` 474 B、`models-dev-pricing.json` 544 555 B、`models-dev-catalog-rules.json` 14 569 B；`git rev-parse HEAD` = `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`。）
