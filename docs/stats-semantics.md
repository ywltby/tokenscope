# 统计口径说明（stats semantics）

> 本文件是 TokenScope 统计口径的**权威说明**（计划 B1/B2 交付物，2026-10-05 建立）。
> 改动解析、去重或计价语义时必须先更新本文件并同步失效缓存（`cache.rs` 的
> `SCHEMA_VERSION`），再改实现。每个结论注明证据来源与适用日志版本。

## 1. 请求单位

「请求数」= 纳入统计的去重用量事件数（按日志口径），**不等于**用户提问数，
也不等于底层 API 请求次数（适配器可能在一次提问中产生多条用量记录，也可能
合并）。UI 中展示为「请求」；各适配器的纳入/排除规则见下文，排除项均有
独立计数（`bad_lines` / `skipped_*` / `duplicates_dropped`），在来源采集统计中可见。

## 2. Token 桶（互斥四桶）

展示统一为四个互斥桶：**非缓存输入 / 输出 / 缓存写 / 缓存读**。展示总量
= 四桶之和。契约测试：`tests/token_bucket_contract.rs`。

### 2.1 Claude Code（`~/.claude/projects/**/*.jsonl`）

Anthropic Messages API 的 usage 四字段本身就是互斥桶，逐一映射：

| 日志字段 | 桶 | 说明 |
| --- | --- | --- |
| `usage.input_tokens` | 非缓存输入 | Anthropic 语义：不含 cache_read/cache_write |
| `usage.output_tokens` | 输出 | 含 reasoning（不单列） |
| `usage.cache_creation_input_tokens` | 缓存写 | 计 1.25× 价格 |
| `usage.cache_read_input_tokens` | 缓存读 | 计 0.1× 价格 |

只取 `type=assistant` 且含 `message.usage` 的行；sidechain（子 agent）
与 `isApiErrorMessage`/合成行按既有口径排除并计数。去重身份 = 
`(session_id, message.id)`，保时间戳最晚一条（流式重发取终值）。

### 2.2 Codex（`~/.codex/sessions/**/*.jsonl`，rollout 格式）

只取 `event_msg/token_count` 的 `info.last_token_usage`；`token_usage_record`
是线程级**累计回显**（含 `response_id`/`turn_id` 但值非单请求口径），一律
忽略并计数（双计防线）。

**字段语义（R02 已查证，2026-10-05）**：OpenAI Responses API 的
`input_tokens` 是**总量桶**，`input_tokens_details.cached_tokens` 与
`cache_write_tokens` 均为其子集。官方文档示例：
`input_tokens=15000 = cached_tokens 12000 + cache_write_tokens 3000 + 0 未缓存`
（[prompt-caching 指南](https://developers.openai.com/api/docs/guides/prompt-caching)）。

归一化（Codex CLI **≥ 0.145.0** 透传 `cache_write_input_tokens`）：

| 日志字段 | 桶 |
| --- | --- |
| `input_tokens − cached_input_tokens − cache_write_input_tokens` | 非缓存输入 |
| `output_tokens` | 输出 |
| `cache_write_input_tokens` | 缓存写（GPT-5.6+ 计 1.25× 价格） |
| `cached_input_tokens` | 缓存读（计 0.1× 价格） |

守恒式：`非缓存输入+输出+缓存写+缓存读 == input_tokens+output_tokens == total_tokens`。

- **旧版日志（< 0.145.0）**：无 cache_write 字段（serde 缺省 0），退化为
  `input = raw − cached`，与 M2 以来的历史行为一致。
- **订阅（ChatGPT）流**：服务端当前恒返 `cache_write_input_tokens = 0`
 （[issue #32479](https://github.com/openai/codex/issues/32479) 评论区，
  OpenAI 成员确认 client 侧已修、服务端置零是独立问题）→ 订阅用户的
  缓存写桶恒为 0 属**数据源现状**，不是本工具丢数。
- **未知字段组合**：`cached + cache_write > input` 的行无法用已查证语义
  解释（例如第三方 provider 的自定义映射），按**坏行计数**不入账，不猜测。

校验式：`total == input + output`（本机 47 文件全量成立，M2 实测）。

## 3. 去重（dedupe）

全局去重（M4 起上移，`src/dedupe.rs`），在缓存合并之后、聚合之前执行，
保证无缓存/缓存命中/重建三路径数字一致。

### 3.1 Claude Code

同 `(session_id, message.id)` 保留时间戳最晚一条（流式增量重发，终值即
最终 usage）；时间戳相同保行序靠后。**无已知误伤**：`message.id` 唯一
标识一次响应。

### 3.2 Codex

同 `(session_id, model, 非缓存输入, 输出, 缓存写, 缓存读)` 保留**首条**
（归一化字段可重建 raw 五元组；B2 起键含 model）。

**为什么要去重**：同一请求的 `last_token_usage` 会被原样重发（M2 实测
2,897 组 / 多余 3,012 行，多数 ×2：流式结束与回合结束各发一次；数值逐字
相同、时间戳不同、行距 1~10+ 行、`info` 无可区分字段、全部在文件内部）。
不去重会把每个请求计成约 2 倍（初版实现即因此与 cc-switch 呈精确 2 倍差）。

**已知限制（R01，保守规则的代价）**：`token_count` 事件本身**没有**请求
身份（`info` 无 id 字段，M2 逐字段核对）；同一会话内**同模型**且四桶数值
完全相同的两个真实请求，会被保守合并为一条。当前证据下无法在不误放重播
的前提下区分这两者（时间戳不能入键——重播的时间戳本就不同）。B2 已把
model 纳入键：**跨模型**的同用量请求不再误合并；重播必然同模型（同一
`turn_context`），防线不变。同模型同用量的合并误伤面（数值恰好相等的
探活级请求）远小于重播误伤面（不去重则整体 ×2），故保留保守规则。
**替代方案（未立项）**：改用 `token_usage_record` 的 `response_id` +
线程累计值差分推导单请求用量，可获得精确身份，但需重写采集口径并重新
对账，且依赖 `token_usage_record` 在各日志版本中的可靠性——待有真实
多版本日志可对照时另行验证。

### 3.3 契约测试

`src/dedupe.rs`：重播保首条、跨批次/跨日仍去重、agent 间隔离。
`tests/e2e_events_range.rs` 等端到端测试保证去重与汇总/明细同源。

## 4. 费用（估算）

- 费用为**估算**：模型前缀匹配价格表（外置 pricing.toml > models.dev >
  OpenRouter > 内置），按四桶分别计价后求和。前缀命中不等于实际供应商
  账单（第三方路由、折扣、订阅额度、税费均未覆盖）；UI 标注「估算」。
- **缺价格 ≠ 零价格**：完全无价格的模型，事件费用不计入且 tokens 进入
  `unknown_tokens`，`unknown_pricing = true`（表格 `†` 标记）。部分分项
  缺价格的模型处理见计划 B3（分项未知不按 0 计）。
- 价格快照与外置文件的来源、条数、同步时间在设置页可见。

## 5. 时间与时区

- 事件时间戳一律**存 UTC**（`jiff::Timestamp`），缓存与导出均为 UTC。
- 展示/聚合按解析链解析时区：显式指定 > 本机时区 > 默认 Asia/Shanghai。
- 「近 N 天」与 from/to 闭区间按**解析时区的自然日**解释（非滚动 24h 窗口）。

## 6. 缓存

- 缓存是**纯优化不是事实源**：任何故障降级为全量内存扫描并告警，数字不变。
- 失效粒度 = 文件级 `(path, size, mtime_ms)` 指纹；解析语义变更时递增
  `cache.rs::SCHEMA_VERSION` 强制全量重建（版本不符自动清库）。
- 测试密闭性：所有测试注入临时 cache/索引目录，禁止触碰真实 `~/.tokenscope`。

## 7. 证据与版本索引

| 结论 | 证据 | 适用版本 |
| --- | --- | --- |
| Codex `total = input + output`、`cached ⊆ input` | 本机 47 文件 151,750 行全量实测（M2） | M2 时点（2026-10-03） |
| `cache_write ⊆ input` | [OpenAI prompt-caching 官方示例](https://developers.openai.com/api/docs/guides/prompt-caching)（15000 = 12000 + 3000） | Responses API 现行文档 |
| `cache_write_input_tokens` 字段存在性与订阅流置零 | [openai/codex#32479](https://github.com/openai/codex/issues/32479)（0.145.0 修复 + OpenAI 成员回复） | codex-cli ≥ 0.145.0 |
| 重播特征（同值不同时刻、无身份字段） | M2 实测 2,897 组（本机 47 文件） | M2 时点 |
| Claude 四字段互斥 | [Anthropic Messages API usage 文档](https://docs.claude.com/en/docs/build-with-claude/prompt-caching)（官方字段定义） | 现行 API |
