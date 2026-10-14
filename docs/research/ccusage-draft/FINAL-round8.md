### 13.5 第 8 轮：实测复核实录（构建 + 运行 + 离线重放）

前七轮全部是静态阅读。本轮把 §13.3 的残留未决项尽量闭环：**真的构建了 ccusage、真的用本机日志运行了它、并把编译期逻辑用脚本离线重放**，最后由第 8 轮复核员独立复现（复现一致率 7/8，其中一处结论被推翻并已改正，见本节末）。

**环境与方法**（可复现，脚本见 `ccusage-draft/evidence-round8/`）：

- 构建：`cargo build -p ccusage`（Rust 1.99.0）。`rust/crates/ccusage-core/build.rs` 要求
  `CCUSAGE_PRICING_JSON_PATH`，故先按仓库 `flake.lock` 里
  `nodes.litellm.locked.rev = d9123f459561754f41fd598a73b405b091d3bf84` 取
  `model_prices_and_context_window.json`（2 797 533 字节）注入；构建耗时 1 分 17 秒。
  **该 rev 的文件与 LiteLLM 当日 main 的 sha256 相同**，因此内嵌快照与线上现状一致。
- 产物核对（复核员补充的最硬证据）：直接解压构建产出的
  `target/debug/build/…/out/litellm-pricing.json.deflate`，其 **534 键与离线重放的键集
  逐键、逐字段、逐值 0 差异**——「重放等价」由推断升级为实测；内嵌 models.dev 快照
  也与其源文件深度相等。
- 运行：全部加 `--offline`（不联网、不写 HTTP 缓存），数据源为本机 `~/.claude` 与 `~/.codex`；
  本节只记录**结构、字段名与行为**，不含任何会话内容或用量数字明细。
- 离线重放：用 Node 脚本逐行复现 `build.rs:155-221`（裁剪 + 改名）与 `pricing.rs:2667-2808`
  （`pricing_key_matches` / `contains_pricing_key` / `suffix_starts_with_numeric_model_version` / `max_by`）。
- 仓库自测：`cargo test -p ccusage-core --lib` → **188 passed / 0 failed**（文档引用的测试确实存在且通过）。
- 格式化实验：用 `rustc` 编译一段复现 `output.rs:548-550` 的小程序。

**闭环结论（逐条对应 §13.3）**：

| # | 原存疑项 | 实测结果 | 状态 |
| --- | --- | --- | --- |
| 1 | 内嵌 LiteLLM 快照的真实键集 | 4084 条目经白名单裁剪后 **534 键**（筛掉 3550）；构成：`azure/` 260、`gpt-` 108、`claude-` 裸键 **23**、`o3`/`o4-mini` 12、`zai/` 16、anthropic 前缀族 115（区域前缀 86 + `anthropic.` 29）。**`claude-*` 裸键只覆盖 4.5 代及以后** | **已验证**（并与构建产物逐键比对） |
| 2 | 未做运行时实测 | 已运行 `--help`、`daily`、`claude daily`、`codex daily`、`session`、`blocks`（均 `--json --offline`），以及 `--mode` 三态、`--no-cost`、`--by-agent` 两处入口 | **已验证** |
| 3 | `format_currency` 的舍入边界 | Rust `{:.2}` 按 **f64 二进制真值**做 round-half-to-even：`0.005→$0.01`、`0.015→$0.01`、`0.025→$0.03`、`0.125→$0.12`、`1.005→$1.00`、`2.675→$2.67`；复核员另用真中值 `0.125/0.375/0.625/0.875/1.125/2.625/3.375` 全部取偶，独立证明 half-to-even | **已验证** |
| 4 | 在线端点与内嵌快照的差异 | `https://models.dev/api.json`（5 632 590 字节）：229 个 provider、8830 条条目、**4043 个去重 id**；内嵌快照 3049 键中 **3021 键名仍在在线目录**、28 个已消失（长尾）。**边界**：这里只比了**键名集合**，价格/档位/上限/`exactOnly` 未逐项比对 | **部分闭环** |
| 5 | 并发语义（`find_cache` 锁外计算） | 未做压力测试 | **未闭环** |
| 6 | `models-dev-pricing.json` 未全量比对 | 已做键集层面比对（见 4）；内嵌快照与源文件深度相等由复核员确认 | **部分闭环** |
| 7 | `models.dev` 生成器与运行期规则的对齐 | **已读生成器实现**：`nix/tools/models-dev-gen/gen.ts`（240 行）+ `compact.ts`（457 行）。信任分级与运行期一致（`MODELS_DEV_PROVIDER_TRUST` = owner 3 / platform 2 / reseller 1，`compact.ts:22-38` 对 `pricing.rs:649-651`）；候选裁决键为 trust → 长上下文档 → 显式 cache_read → 显式 cache_write → 上下文上限 → provider id / model id（字典序小者胜，`compact.ts:403-430`） | **已验证** |
| 8 | 各 agent 的模型名形态 | 已实测主链路（见下）；其余 adapter 未逐一实测 | **部分闭环** |
| 9 | antigravity 的 protobuf 字段号映射 | 本机无 Antigravity 数据 | **未闭环** |
| 10 | `CODEX_REWRITTEN_BURST_PAUSE_MS = 1000` 的经验依据 | 本机 Codex 日志未识别到「日志被重写」事件 | **未闭环** |

**由此修正的一处实质错误（§8 §4.5(a)）**：原文称 `claude-sonnet-4-20250514`
「阶段 A 精确命中即可」——**错**。实测：内嵌表没有该裸键，真实路径是**模糊匹配**
命中区域前缀的 Bedrock 键（实测命中 `global.anthropic.claude-sonnet-4-20250514-v1:0`，长度 46 为最长；
候选另含 `anthropic.` / `us.` / `eu.` 三个前缀的变体——**`apac.` 只存在于未裁剪的完整 LiteLLM，
不在内嵌表**，白名单不含该前缀）。仓库测试
`embedded_pricing_resolves_overlapping_model_keys_exactly`（`pricing.rs:4931-4955`）
本身就在证明这一点——它断言的是 `find("claude-sonnet-4-20250514")` 与
`find("anthropic.claude-sonnet-4-20250514-v1:0")` 的 **input 相等**（即前者落到了后者的价上），
而不是断言裸键存在。`claude-3-5-haiku-20241022` 属于另一条路径：它是**内嵌兜底表的精确命中**（`pricing.rs:2191-2192`
用 `put_builtin_entry` 写入该 id），不进模糊扫描；三个同价的 Bedrock 变体（`anthropic.` / `eu.` / `us.`，
input 均 `8e-7`）因此不会被选中——这也是 `pricing.rs:4951-4954` 的断言无法区分路径的原因。
**脚本边界**：`replay-match.js` 只加载 LiteLLM 裁剪表，**不含内置兜底表**，所以它会把这个名字误报成
模糊命中（脚本的 tie-break 曾因方向写反取到 `us.`，修正后与真实语义一致）；正确结论以本段为准。
**推论**：`claude-*` 裸键缺旧型号时，ccusage 仍能给出价格，但给的是**区域/Bedrock 渠道价**——
这解释了两件事：为什么旧模型不会报 `unpricedModels`，以及为什么其单价可能与 Anthropic 直连账单不同。

**版本边界保护的实证（归因修正）**：真正的样例是 `gpt-5.4-9` 与 `claude-opus-5-9`——
表内有 `gpt-5.4` / `claude-opus-5`，后缀 `-9` 命中
`suffix_starts_with_numeric_model_version` 的拒绝分支，两者在 534 键表上均**无模糊命中**。
此前举例的 `gpt-500` 其实是被**边界检查**挡住的（后缀以 `'0'` 开头，属字母数字而非边界），
`opus550` 则连双向子串关系都不成立——两者都不能证明版本保护。另：`claude-opus-4-6-fast`
无精确键，模糊命中基础键 `claude-opus-4-6`，fast 倍率由 `fast-multiplier-overrides.json` 的
`normalized_prefix["claude-opus-4-6"] = 6.0` 提供（复现 `matches_model_suffix` 的 `rfind` + `-` 判定）。

**旧模型的两道防线（实测补充）**：内嵌 models.dev 快照里，旧模型以**带目录前缀**的键存在——
实测查到 `anthropic/claude-sonnet-4-20250514`、`eu./global./us.anthropic.claude-sonnet-4-20250514-v1:0`。
因此即使主表（LiteLLM）失手，模糊匹配仍能在第二道防线命中 `anthropic/claude-sonnet-4-20250514`
（`/` 是合法边界）。生成器注释也解释了这类保留策略：reseller 的价是「可信目录不再发布时的最后手段」，
并明确举了"退役的 Claude 3 发布版只有 reseller 还在列"（`compact.ts:31-37`）。
注意归一化后的拼写（`anthropic--claude-3-haiku` 这类）也出现在快照里，说明同一目录内存在多种拼写形态。

**运行时行为验证（与文档既有描述逐项对照）**：

| 观察点 | 实测 | 对应文档 |
| --- | --- | --- |
| `claude daily --json` 顶层键 | `daily`、`totals` | §7 §8.1 一致 |
| 日行的键 | `cacheCreationTokens, cacheReadTokens, date, inputTokens, modelBreakdowns, modelsUsed, outputTokens, totalCost, totalTokens` | §7 §8.1 一致 |
| `codex daily --json` 日行的键 | 额外含 `costUSD`、`models`、`reasoningOutputTokens` | §5 §10 一致 |
| `session --json` 顶层与行键 | `session` / `totals`；行含 `agent`、`metadata`、`period` | §7 §10 一致 |
| `blocks --json` 块键 | `actualEndTime, burnRate, costUSD, endTime, entries, id, isActive, isGap, models, projection, startTime, tokenCounts, totalTokens` | §7 §6 一致（含 `isGap`、`burnRate`、`projection`） |
| `--no-cost` | 输出中 `totalCost`/`costUSD`/`cost` 全部消失 | §7 §8.3 一致 |
| `--mode` 三态 | `display` → `totals.totalCost = 0.0`；`auto` 与 `calculate` 数值完全相同。**边界**：本机日志没有 `costUSD` 字段，所以「display 给 0 是因为它只信 `costUSD`」是**一致解释**而非直接实测 | §8 §6 一致 |
| `--jq` | **不在** `--help` 文本里（但选项可解析） | §11 第 8 条一致 |
| `claude statusline --help` | `--offline` 默认 **true**、`--cache` 默认 true、`--refresh-interval` 默认 1 | 闭环 §8「未验证 / 存疑」第 8 条的疑问 |
| `codex daily --help` | `--speed [auto\|standard\|flex\|fast]`，默认 auto | §5 §6 一致 |
| `codex daily --speed` 三态 | `auto` 与 `standard` 数值**不同**、`fast` 高于 `standard` ——说明 `auto` 确实读取并采用了本机日志记录的 service tier（若日志无 tier 记录，auto 会退化成 standard），`--speed` 的强制覆盖也确实改变金额 | §5 §6 一致 |

**`--by-agent` 的适用范围（本条曾被本轮实测误判，已按复核结论改正）**：
该选项在**顶层 `daily` 与无子命令路径**下**生效**——`ccusage daily --json --by-agent --offline`
的行级键包含 `agents`（数组，实测元素数 ≥1），不带该标志时同一位置只有 `agent`；
`ccusage --json --by-agent --offline` 同理。它**不被 `claude` 子命令接受**：
`ccusage claude daily --json --by-agent --offline` 直接报
`Unknown daily option '--by-agent'` 并以 exit 2 退出（Claude 专属的 `DailyArgs` 没有该字段，
见 `rust/crates/ccusage/src/commands/mod.rs:33-70`；统一报告侧由
`ccusage-cli-parser/src/parser.rs:375`、`:440-473` 解析，`ccusage-adapter-all/src/lib.rs:41` 消费）。
**教训记录**：本轮初次实测把检查点放在**顶层键**（顶层只有 `daily`/`totals`），因此误得
「选项被接受但静默无效」的结论；复核员用**行级键**复现才暴露问题——`agents` 是行级键。

**仍未闭环的 3 条**（保留在 §13.3）：并发压力、antigravity 的 protobuf 映射、
Codex 重写突发阈值的经验依据——它们都需要真实触发条件或额外数据，本次环境不具备。

**重放脚本的适用边界**（写入证据目录，避免误用）：
`replay-match.js` 实现了 `pricing_key_matches` 与最长键裁决，但**未实现 `exact_only` 门禁**，
也**不加载内置兜底表**（`put_builtin_pricing`）——因此它会把 `claude-opus-5-fast` 判成命中
`claude-opus-5`、把 `claude-3-5-haiku-20241022` 判成模糊命中区域键，而真实运行时分别由
models.dev 档位条目与兜底表精确键作答。用它下结论时必须叠加这两层。

### 13.6 第 9 轮：全文逐节复核

第 8 轮之后又做了一次**面向全文**的复核（4 路并行，每路逐节读完自己负责的章节并按章抽查）：

| 路 | 范围 | 抽样 | 结果 |
| --- | --- | --- | --- |
| C1 | §1–§5（L45–L1079） | 60 条 | 59 条一致；数字/阈值/优先级类零偏差 |
| C2 | §6–§8（L1080–L2747） | 59 条 | 全部一致（含独立解压构建产物复核 534 键构成） |
| C3 | §9–§11（L2748–L3343） | 63 条 | 62 条一致；并实测复验 `--by-agent` 与 `--jq` 两个争议点 |
| C4 | §12–§14 + 附录（L3344–L3890） | 44 条 | 43 条一致；全局结构终检通过 |

另做了两项机械检查（覆盖全文，非抽样）：

- **行号越界扫描**：用脚本提取全部 `文件:行号` 引用并按每个文件的实际行数比对——**可解析的 2125 条 0 越界**；同名文件的歧义引用改用「同名文件最大行数」做宽松上界，同样 0 越界。**扫描边界**：另有约 159 条以简写路径（`loader.rs:123` 之类）出现且未匹配到唯一文件的引用，与 33 条 v19 引用，不在该扫描范围内。
- **引用完整性**：`§N` 章级引用共 73 处、覆盖 13 个章号，全部指向存在的章节；章内小节引用的读法已写进开头的「阅读约定」。

**本轮修正的问题（6 处）**：

1. **§8 §4.5(a) 与 §13.5 的 `claude-3-5-haiku-20241022` 归属**（实质错误）：它不是模糊命中，
   而是**内置兜底表的精确键**（`pricing.rs:2191-2192`）——精确阶段直接作答，不进模糊扫描。
   该误判的来源是重放脚本缺兜底表 + 三条 Bedrock 候选同价（测试无法区分路径）；已改正，
   并把脚本缺口写进证据目录的「使用边界」。
2. **§10.4 与 §10.8-4 的 v19↔v20 结论**（实质错误）：原文称「取值主路径两版一致」——错。
   v19 是**无条件**优先 `lastUsage`（`parser.ts:205-212`），v20 多了「累计值必须前进」的门控，
   未前进时丢弃 `last_token_usage` 改走累计差分（`codex/src/parser.rs:483-494`）。
   已按 §5.3 的口径改写，两处不再矛盾。
3. **§10.8 的交叉引用错位**：第 3/4/5/9 条的 `（§7.x）` 实为「第 7.x 小节」的旧编号，
   在终稿编号下指向无关内容；已改为 §4.4 / §5.3 / §9.3 / §8（并把 §2.4、§1.5 显式化为 §9.2.4、§9.1.5）。
4. **§11 第 2 行的 `types.rs:175` 缺 crate 限定**：裸写会指到 `ccusage-cli/src/types.rs` 的
   `AgentCommandArgs.shared`；已补为 `rust/crates/ccusage-core/src/types.rs:175`。
5. **附录 A 的「日志级别」位置列**：`logger.rs` 只有 15 行、不含 0–5 级别表；映射实际写在
   `docs/guide/environment-variables.md`，已改正并注明封装文件的真实规模。
6. **§14.1.3 的 `bytes.len() >= 10` 因果**：它不是「索引安全下限」（匹配 `Users\` 只需 ≥9），
   已改述为「作者留余量的保守下界」。

此外给 §4/§5/§6/§7/§9 五章的引言补了**「本章裸文件名约定」**（裸 `parser.rs` / `loader.rs` 等
分别指哪个 crate），从源头消除同名文件的行号歧义；并在开头的阅读约定里补了对应说明。

**本轮仍判定未闭环的项**（与 §13.3 一致，加复核员列出的补充）：并发压力、antigravity 的
protobuf 映射、Codex 重写突发阈值的经验依据、`copilot` 的 credits 归因、`opencode` 的 SQL 细节、
`pi` 的 replay 剔除点、`json_float` 的极端输入、`--single-thread` 与 `blocks`/`statusline` 的未运行入口等。

**代理分歧与裁定（1 处）**：C4 曾把 `gpt-500` 无命中的原因归给「版本后缀规则的第一条」，
与源码顺序不符——`suffix_allows_pricing_key_match`（`pricing.rs:2693-2701`）**先**做边界检查，
`'0'`（字母数字）在此即返回 `false`，执行流根本到不了 `suffix_starts_with_numeric_model_version`。
文档保留「被边界检查挡住」的归因（版本保护的实证样例早已换成 `gpt-5.4-9` 与 `claude-opus-5-9`）。
