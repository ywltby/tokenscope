## 12. 与 TokenScope 的对照：可抄的与要避的

对照基准：本仓 `Cargo.toml` 版本 `0.3.0` 时的源码（对照事实逐条见同目录 `TS-FACTS.md`，
每条带 `文件:行号`）。本节只列**两侧都有代码证据**的差异；括号内的 `→` 左侧是 ccusage、右侧是 TokenScope。

### 12.1 结构性差异（会直接导致两边数字不同）

| # | 维度 | ccusage | TokenScope | 结论 / 影响 |
| --- | --- | --- | --- | --- |
| 1 | **Claude 缓存写入的时长档** | 区分 `cache_creation.ephemeral_5m/1h`，1 小时档按 `input × 2` 计价（`cost.rs:118-131`） | 只读 `cache_creation_input_tokens` 单一字段映射为 `cache_write`，`PriceRates` 只有一个缓存写槽位（`src/source/claude.rs:58-68,364`、`src/pricing.rs:200-210`） | **TokenScope 会低估 1 小时缓存写入的成本**：同一个 token 量在 ccusage 里可能按 2 倍 input 计价。这是最值得优先评估的一条 |
| 2 | **Codex 的 compaction / 回放增量** | 计入 `token_usage_record`（须被 `compacted` 记录认领、按 `response_id` 跨文件去重），并对父会话回放做前缀扣减（`codex/src/parser.rs:236,510-530`） | 明确忽略 `token_usage_record`（计 `ignored_token_usage_record`，作为双计防线），只取 `last_token_usage`（`src/source/codex.rs:4-6,314-325`） | ccusage **覆盖更广**（可能多算 replay/compaction），TokenScope **更保守**（可能少算）。两者不是谁对谁错，但口径差异要在文档里写明 |
| 3 | **Claude 去重键** | 三档键 + sidechain 回放路由 + 三条决胜规则（`claude/src/lib.rs:350-366`、`:198-212`） | `(session_id, record_id)` 单键，保留时间戳最晚者（`src/dedupe.rs:9,26-42`） | ccusage 会把「同 requestId 跨会话的复制」合并；TokenScope 不合并。网关/复制 transcript 场景下 ccusage 更小 |
| 4 | **价格表模型** | **单表 + 覆盖**：在线刷新按**键级整条替换**（`pricing.rs:1080-1100`），用户 override 字段级且最后执行（`:1036`），内置表默认 `or_insert`（三处例外见 §8） | **候选并存 + 取最高**：三来源各成候选、禁止跨候选拼价、完整候选优先再取最高费用（`src/pricing.rs:2304-2415`） | 两种设计目标不同：ccusage 求「一个确定的价」，TokenScope 求「多渠道下不给低估」。TokenScope 的 `reason`/`highest_complete_cost` 是 ccusage 没有的披露 |
| 5 | **内嵌价格表** | 有：编译期裁剪 LiteLLM + models.dev 快照（`build.rs:155-221`），离线永远可用 | 无：`Pricing::default()` 为空表，未收录模型恒为「未知」（`src/pricing.rs:1737-1750`） | ccusage 的「离线可用」以**覆盖范围窄**（裁剪白名单只含 `claude-`/`anthropic.*`/`gpt-`/`o3`/`o4-mini`/`openai/`/`azure/`/`zai/`/`openrouter/openai/` 前缀）为代价；TokenScope 以「首次启动需同步」为代价 |
| 6 | **5 小时计费窗口（blocks）** | 完整实现：UTC 整点锚 + 严格 `>5h` 断块 + gap 块 + burn rate/projection（`blocks.rs:53-160`、`:567-618`） | **未实现**（全 `src/` 无 block 语义） | 若要做：ccusage 的锚点/断块/gap 三规则可直接照抄，注意它按 **UTC** 对齐（§7） |
| 7 | **增量策略** | 每次全量重扫；省的是**读取**（`--since` 时按 mtime + 会话组并查集剪枝文件，`claude/src/paths.rs:117-172`）+ 行级 `memmem` 预过滤 + 按大小分块的并行读 | 每次串行全量解析；省的是**重解析**（`(size, mtime)` 指纹缓存 + 采集前后双取指纹，`src/report.rs:1262-1323`） | 两者互补：TokenScope 若引入 mtime 剪枝，必须**按会话组整体判断**（ccusage 的并查集就是为 sidechain 跨文件去重准备的） |

### 12.2 可直接借用的做法（附 ccusage 证据）

1. **三档去重键**：`requestId` 缺失是常态，键要按「有无 requestId → 是否 `msg_` 前缀 → 是否带 timestamp」降级（`claude/src/lib.rs:350-366`）。TokenScope 目前单键 `(session_id, record_id)`，遇到「一条回复写多行」的形态时行为不同，值得补一条不变量测试。
2. **sidechain 回放的独立路由**：回放会用**新 requestId** 重写历史，普通键匹配不到；ccusage 用「`message.id` + session + 至少一侧是 sidechain」的次键兜住（`lib.rs:237-266`、`:427-440`）。注意它**有 timestamp 守卫**，而 daily 实现漏了这条守卫（§13.2 第 2 条）——抄的时候不要抄漏。
3. **数字版本后缀保护**：候选键以数字结尾时，后缀「`-`/`.` + 数字」若不是 8 位日期就判为不同版本，拒绝匹配（`pricing.rs:2703-2722`）。TokenScope 用「分隔符边界 + 压缩键偏移」达到同类效果（`src/model_identity.rs:141-157`），两套规则可互相印证。
4. **字段级 override + 「值来源」标记**：`cache_*_explicit` 让「派生值可按比例缩放、显式值必须原样保留」成为可实现规则（`pricing.rs:1069-1070`、`:1832-1861`）。TokenScope 的 `RateSpec` 三态解决的是「缺失 vs 显式 0」，两者可以叠加。
5. **缺价三处披露**：JSON 标记（`missingPricing`）、汇总清单（`unpricedModels`）、stderr 警告（`output.rs:430-450`）。TokenScope 已有「未知」三态与横幅，可对照补齐「汇总清单」这一层。
6. **按文件大小贪心分块的并行读**（`common/src/lib.rs:49-81`）：实现只有 33 行，且天然保持输出顺序；若 TokenScope 将来要并行化采集，这是现成的模式（注意 ccusage 的并行是**文件级**，TokenScope 现在是来源串行，两者不冲突）。
7. **缓存写入的时长口径**（1h = `input × 2`）与**默认缓存价推导**（`cache_create = input×1.25`、`cache_read = input×0.1`）：可直接作为 TokenScope 扩展单一 `cache_write` 槽位时的取值依据（`cost.rs:7,118-131`、`pricing.rs:1085-1088`）。

### 12.3 建议规避的做法（附证据）

1. **同一语义两份实现**：Claude 的 `load_entries`（`claude/src/lib.rs`）与 `load_daily_summaries`（`claude/src/daily.rs`）是两套解析代码，靠 7 个等价性测试维持；但复核发现两者在 sidechain 回放路由上**确有结构差异**（daily 缺 timestamp 守卫，`daily.rs:493-496` vs `lib.rs:436-440`）——测试没覆盖的样本形态会给出不同总量。TokenScope 用单一管线 + 不变量测试是更稳的取向。
2. **「兜底表」语义与注释不一致**：`put_builtin_pricing` 声明只补缺键，实际有 3 处无条件覆盖（`pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`）。若抄这套结构，要么保持纯粹 `or_insert`，要么显式标注例外。
3. **厂商特例硬编码的规模**：`codex-auto-review` 的日期回退表、DeepSeek V4 三段价与 UTC 峰谷窗口（`pricing.rs:252-347`）、`pricing_alias` 三条别名——这些会随时间腐化。TokenScope 的「外置规则 + 数据文件」方向更好，但需要同样的测试覆盖才能保证不腐化。
4. **文档与代码无门禁**：§11 列了 8 条「文档与实现不符」（另 2 条是文档与代码一致的正面例子）。TokenScope 的 `config-schema.json` 式「构建期生成 + CI 校验」正是 ccusage 只在 schema 与帮助文本上做到、而指南文本没做到的事。
5. **`exact_only` 门禁的复杂度**：为了防止 `claude-opus-5-fast` 被基础模型价抢走，ccusage 引入跨数据源的门禁集合与三种查询语义（`contains` / `contains_any_spelling` / `id_spelled_by`，`pricing.rs:501-521`、`:1617-1683`）。TokenScope 的候选模型天然避免了这个问题（不覆盖、取最高），**不要**再引入单表覆盖语义。

### 12.4 一句话总结

ccusage 的强项是**口径的工程化**：把「同一条日志可能被写多行 / 被回放 / 被复制」这些脏现实，
用去重键、回放路由、压缩回放状态机一层层兜住，并用测试把这些规则钉死；
弱项是**结构膨胀**：双实现、跨源门禁、厂商硬编码与文档漂移。
TokenScope 的强项是**候选式定价与非破坏性历史库**；若要对齐，
优先补的三件事是：缓存写入时长档、Codex 增量口径的显式决策、以及 5 小时窗口的取舍。
