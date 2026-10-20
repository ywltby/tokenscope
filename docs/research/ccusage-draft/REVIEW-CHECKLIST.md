# ccusage 研究复核清单（供对抗式复核 agent 使用）

仓库：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`（HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31）
旧版：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage-v19`（v19.0.3，TypeScript）

## 复核方法

对每一条断言：
1. 打开指明的源码位置，逐字确认行号与内容；
2. 若内容与断言不符 → 记为 **错误**，给出「断言 / 实际」两行对照；
3. 若源码里存在与断言相反或更复杂的证据（例如另有分支、另有覆盖顺序）→ 记为 **不完整**；
4. 只做「能否证伪」的工作，不做风格评价。

## A. 我（主研究）已亲自核对的锚点 —— 请抽查 8 条以上

| 编号 | 断言 | 位置 |
| --- | --- | --- |
| A1 | Rust workspace 成员是 `adapters/*` 与 `crates/*`，版本统一继承 `version.workspace = true` | `rust/Cargo.toml` |
| A2 | 内嵌 LiteLLM 快照编译期裁剪：白名单前缀 `claude-`/`anthropic.`/`gpt-`/`o3`/`o4-mini`/`openai/`/`azure/`/`zai/`/`openrouter/openai/`，字段改短名 `i/o/cc/cr/ia/oa/cca/cra/ctx`，缺 `i` 或 `o` 的条目被丢弃 | `rust/crates/ccusage-core/build.rs:155-221` |
| A3 | `MODEL_DATE_SUFFIX_DIGITS = 8`，用于「8 位数字后缀视为日期（可前缀匹配），其它数字后缀视为不同版本（拒绝匹配）」 | `rust/crates/ccusage-core/src/pricing.rs:61`、`2703-2722` |
| A4 | `CostMode` 默认 `Auto`；`Display` 模式在 Claude 链上不加载价格表 | `rust/crates/ccusage-cli/src/types.rs`（`#[default] Auto`）、`rust/adapters/claude/src/lib.rs:144-152` |
| A5 | blocks 的窗口锚点 `floor_to_hour` 是 **UTC** 整点（`div_euclid(MILLIS_PER_HOUR)`），不是本地时区 | `rust/crates/ccusage-core/src/date_utils.rs:58-60`、`rust/crates/ccusage/src/blocks.rs:89,92,105-107` |
| A6 | Codex 桶成本里 `cache_read` 未显式给价时用 **input 价**（不是 `input*0.1`） | `rust/adapters/codex/src/report.rs:343-348` |
| A7 | `flex_multiplier` 只在 Codex 计价里使用；Claude 链只用 `fast_multiplier`（乘总额） | `rust/adapters/codex/src/report.rs:273`、`rust/crates/ccusage-core/src/cost.rs:110-115` |
| A8 | `format_currency` = `format!("${value:.2}")` | `rust/crates/ccusage-core/src/output.rs:548-550` |
| A9 | Claude 去重键：`hash(message.id, request_id)`；无 request_id 时追加 session_id；`message.id` 非 `msg_` 前缀时再追加 timestamp | `rust/adapters/claude/src/lib.rs:350-366` |
| A10 | 重复条目保留规则：非 sidechain 优先 → token 总数大者优先 → 有 `speed` 者优先 | `rust/adapters/claude/src/lib.rs:198-212` |
| A11 | `CCUSAGE_MODEL_ALIASES` 是**用户环境变量**（非内建别名表），支持 JSON 与 `k=v` 两种写法，`-fast` 变体自动跟随 | `rust/crates/ccusage-core/src/model_aliases.rs:8,39-61` |
| A12 | 内建别名仅 3 条：`gpt-reserve→gpt-5.6-luna`、`gpt-5.6→gpt-5.6-sol`、`gpt-5.3-spark→gpt-5.3-codex-spark` | `rust/crates/ccusage-core/src/pricing.rs:2793-2800` |
| A13 | 缓存创建 1 小时档单价 = input 价 × 2.0 | `rust/crates/ccusage-core/src/cost.rs:7,128-131` |
| A14 | 缺失价格时：token 总数为 0 不报、Display 不报、Auto 且有 costUSD 不报 | `rust/crates/ccusage-core/src/cost.rs:48-59` |
| A15 | `claude` 子命令支持 daily/monthly/weekly/session/blocks/statusline；blocks/statusline 仅 Claude 支持并有专门报错文案 | `rust/crates/ccusage-cli-parser/src/parser.rs:557-587,864-880` |
| A16 | 文件并行读取按「大小降序贪心装箱」分块，结果按原顺序回填 | `rust/adapters/common/src/lib.rs:49-126` |

## B. 需要重点证伪的「易错点」（来自 subagent 分片，初判）

- B1 Codex `total_token_usage` 的差分口径：是否有「取相邻差」逻辑？压缩/回放如何扣除？`codex_line_usage_kind`/`subtract_codex_raw_usage`/`CodexReplayState` 的真实语义？
- B2 `detect_rewritten_burst`（`parser.rs:129`）与 `CODEX_REWRITTEN_BURST_PAUSE_MS = 1_000` 解决什么问题、阈值如何用。
- B3 各 adapter 环境变量清单是否与 `docs/guide/environment-variables.md` 一致（逐个 grep `env::var`）。
- B4 是否所有 adapter 都产出「四桶 + cost」的统一契据；哪些 adapter 有自带 cost 字段（Amp credits、Pi total cost）。
- B5 `load_daily_summaries`（Claude）与 `load_entries` 是否真的等价（有没有已知差异，如 agent progress 行的处理）。
- B6 `--since` 剪枝对**结果**无影响这一断言（只看 mtime；但若文件 mtime 被外部修改会怎样）。
- B7 zstd/压缩：Codex 是否支持 `.jsonl.zst`（源码里有没有 zstd 依赖）。
- B8 时区：默认时区是什么（系统时区还是 UTC）；`--timezone` 的解析与 `jiff` 用法。
- B9 价格匹配的 exact-only 门控是否会「藏住」某些模型的价（如 claude-opus-5-fast）。
- B10 `models.dev` 目录在离线时是否仍参与（`enable_embedded_models_dev_fallback`）。
- B11 会话分组 key（session reports）到底用什么。
- B12 statusline 的缓存与并发（`process_is_alive`、锁文件）。
- B13 数字格式化：`json_float` 是否做四舍五入。
- B14 v19（TS）与 v20（Rust）在去重/计价/block 上的实质差异。

## C. 输出格式（复核 agent 写文件）

写到 `docs/research/ccusage-draft/REVIEW-<轮次>.md`：

```
## 结论
（一句话：分片 X 是否可用）

## 错误（必须修）
- [E1] 分片 §x.y：断言「…」/ 实际「…」（file:line）

## 不完整（建议补）
- [I1] …

## 无法验证
- [U1] …

## 复核到的行号抽样（证明真的打开过）
- file:line → 内容摘录
```
