## 11. 实现与官方文档的差异清单

研究过程中发现的「文档写的 ≠ 代码做的」条目。**以代码为准**；列在这里是因为它们会直接影响使用者对数字的解读，也是复现差异时的第一现场。

| # | 位置 | 文档说法 | 代码事实 | 影响 |
| --- | --- | --- | --- | --- |
| 1 | `docs/guide/blocks-reports.md:150-151` | 「`--order desc` 显示最新块（默认）」 | `SortOrder` 的 `#[default]` 是 `Asc`（`rust/crates/ccusage-cli/src/types.rs`），`blocks` 默认最旧在前 | 直接照文档复现会得到相反顺序 |
| 2 | `docs/guide/weekly-reports.md:138-149` | `modelBreakdowns` 是**对象映射**（模型名 → 明细） | 实际是**数组**（`UsageSummary.model_breakdowns: Vec<ModelBreakdown>`，`types.rs:175`） | 按文档写的解析器会崩 |
| 3 | `docs/guide/codex/index.md:50` 与 `:116` | 「缺模型元数据的会话被跳过」 | 同文 `:54` 说「早期日志仍计入、按 `gpt-5` 回退」，与代码一致（`rust/adapters/codex/src/parser.rs:813` 附近的回退） | 文档**自身**前后矛盾 |
| 4 | `rust/adapters/codex/src/README.md:47-53` | 描述 `task_started` / `inter_agent_communication_metadata` / `trigger_turn` 的边界处理 | 全仓 grep 这三个符号只命中该 README，代码未实现；真实实现是父前缀 usage 全等匹配 + 「重写突发」启发式（`codex/src/parser.rs:236`、`:122`） | 会把未实现的能力当成已实现 |
| 5 | `docs/guide/environment-variables.md:124/129/209` | `CCUSAGE_OFFLINE` 强制离线 | 两版源码（v19/v20）**均无**该变量的读取 | 设了不生效 |
| 6 | `docs/guide/configuration.md:38` | `CCUSAGE_TIMEZONE` | 同上，两版均无实现；真实入口是 `--timezone` 与配置项 | 设了不生效 |
| 7 | `docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24/51` | `CCUSAGE_GROK_PATH` | 设计稿本身要求移除；v20 实际用 `GROK_HOME`（`rust/adapters/grok/src/paths.rs:9`） | 历史设计遗留 |
| 8 | `--help` 输出 | — | `--jq` 可被解析并生效，但**未出现在帮助文本**（`cli-help.json` 无该条目） | 隐藏选项 |
| 9 | `docs/guide/cost-modes.md:198-205` | 成本公式（含 1 小时缓存 = input × 2） | 与 `cost.rs` 一致（`rust/crates/ccusage-core/src/cost.rs:7`、`:128-131`） | **一致**（正面例子） |
| 10 | `docs/guide/codex/index.md:55` | 缓存读价缺失时回退 input 价 | 与 `rust/adapters/codex/src/report.rs:344-348` 一致 | **一致** |

推论（**一致解释**）：ccusage 的文档随 PR 演进，但 `docs/` 与 `rust/` 之间没有强制一致性的门禁——
只有 `config-schema.json` 与 CLI 帮助是构建期生成的（§9），其余指南文本靠人维护。
对照代码时**先看测试与源码**，再看文档。

## 13. 复核记录与残留未决问题

### 13.1 复核流程

| 轮次 | 执行者 | 动作 | 结果 |
| --- | --- | --- | --- |
| 第 1 轮 | 6 个独立研究代理（S1–S6） | 按「主干/Claude、Codex、定价与金额、聚合与报告、其余 16 适配器、配置与 CLI 与版本史」六路精读，各自产出带 `文件:行号` 的分片 | 6 份分片，合计约 25 万字符（约 33 万字节 UTF-8） |
| 第 2 轮 | 4 个对抗式复核代理（R1–R4） | **不复用研究结论**，重新打开源码逐条证伪：R1 复核 S1+S2（71 条抽样）、R2 复核 S3（80 条）、R3 复核 S4（58 条）、R4 复核 S5+S6（43 条） | 发现 25 处必须修正（含 3 处语义级、多处证据错位） |
| 第 3 轮 | 6 个原作代理 | 按 R1–R4 的「断言 / 实际 + `文件:行号`」逐条就地修正分片 | 6 份分片修正完毕（含 3 处语义级问题的改写） |
| 第 4 轮 | 2 个终审代理（A/B） | 对**装配后的完整文档**分段复核：A 查 §1–§8（61 条行号抽查）、B 查 §9–§13 与附录（73 条抽查 + 附录全量比对） | A：2 处必改 + 6 处装配问题；B：1 处硬错误（JSON 键名）+ 3 处口径问题 |
| 第 5 轮 | 主研究员修正 + 2 个验证代理 | 逐条修复终审清单并重新装配；R5 验证「必改项是否落地」，R6 审查「覆盖度与结构性缺口」 | R5：13 条必改项全部已修、0 未修，判定达到可交付标准；R6：3 个真缺口（项目名清洗链、行级容错横向表、符号链接立场）+ 2 处高价值漏项（选源判据、测试基建） |
| 第 6 轮 | 1 个补充研究代理（S7） | 按 R6 清单补齐缺口与漏项，写成 §14 横向专题 | 见 §14 |
| 第 7 轮 | 1 个复验代理（R7） | 复验 §14（22 条抽查）、第 5–6 轮改动（10 条逐条）、§12 的 TokenScope 侧引用（8 条）与全文结构 | 未落地项 0；4 处低危细节（阅读约定章号、§14.6 步号、zcode 分支描述、grep 计数）已修；判定**可交付** |

### 13.2 第 2 轮发现的典型问题（保留以示证据强度）

- **语义级（S3）**：分片称「内置兜底表只 `or_insert`、绝不覆盖上游」，实际 `put_builtin_pricing` 有
  3 个键走无条件 `insert`（`rust/crates/ccusage-core/src/pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`），
  与源码注释声明的意图相反。
- **语义级（S1）**：分片漏掉「daily 侧的 sidechain 回放路由没有 timestamp 守卫」这条**结构差异**
  （`claude/src/daily.rs:493-496` vs `claude/src/lib.rs:436-440`）——它会改变「是否合并」，
  比已列的三条平局裁决规则更实质；修正后该分片明确写出「等价只在测试覆盖的样本形态下成立」。
- **证据错位（S3/S4/R3）**：多处行号指到了相邻函数（`gpt-5.4` 与 `gpt-5.4-mini`、
  `find_help_page` 与 `help_text_for_tokens`、`codex/speed.rs` 与 `codex/parser.rs`），
  以及把测试 helper 当作生产证据。
- **计数错误（S5）**：`apply_total_token_fallback` 的调用被写成「14 家」，实测是 12 家 / 14 处。
- **文档指控复核（R3）**：分片指控 `weekly-reports.md` 的 `modelBreakdowns` 结构过时——复核确认属实（§11 第 2 条）。
- **代理分歧裁定（主研究员）**：R4 称 `prepare_session_message_query` 在 `loader.rs:754`，
  S5 称在 `:752`；直接读源（`rust/adapters/opencode/src/loader.rs:752-755`）确认为 `fn` 行 **752**，
  754 是第二个参数行——裁定 S5 正确，R4 该条作废。

### 13.3 残留未决问题（未验证项，按影响排序）

1. **内嵌 LiteLLM 快照的真实键集未直接检视**：`$OUT_DIR/litellm-pricing.json.deflate` 是编译产物，
   本次遵守「只读、不构建」约束未生成它；「某模型在不在内嵌表里」的结论来自测试断言与匹配算法推演。
   复现方式：设 `CCUSAGE_PRICING_JSON_PATH` 指向一份 LiteLLM 快照后构建，或运行 `--json` 看 `modelBreakdowns`。
2. **未做运行时实测**：本报告全部结论来自静态阅读与仓库内测试断言，
   **未运行 ccusage 二进制、未与本机真实日志比对**；凡涉及「真实数据上两实现是否分叉」的判断均为推断。
3. **`format_currency` 的舍入边界**（`{:.2}` 对 `x.xx5`）未做实验。
4. **在线端点与内嵌快照的差异**：`https://models.dev/api.json` 与 LiteLLM `main` 的实时内容未抓取比对。
5. **并发语义未压力验证**：`PricingMap.find_cache` 的「锁内读 → 锁外算 → 锁内写回」只在单线程测试中覆盖。
6. **`models-dev-pricing.json`（34 269 行）未全量比对**：只抽查了 8 处条目与 4 个尺寸数字。
7. **`models.dev` 生成器**（`nix/tools/models-dev-gen`）与运行期规则的逐条对齐未核。
8. **各 agent 的模型名形态差异**：仅 Claude / Codex / Pi 三条路径逐行核对；
   其余适配器只确认它们调用同一套价格加载与计价函数，`model` 字段是否已带 `-fast` 或 provider 前缀未逐一验证。
9. **antigravity 的 protobuf 字段号映射**是代码事实，但与真实 Antigravity 版本 schema 的对应关系无法在仓库内验证。
10. **`CODEX_REWRITTEN_BURST_PAUSE_MS = 1000` 的经验依据**只在注释里，未独立复现。

### 13.4 未决问题的性质

上述 10 条的共同点是：**它们都要求「运行」或「外部数据」才能闭环**，
而本次研究被限定为「只读源码」。凡不满足这条限定就无法验证的，都已明确标注，
未以「推断」冒充「已验证」。

七轮复核（约 300 条抽查与 30 余处修正）后，**没有发现阻断性问题**；
本报告的行号均以 HEAD `57afdc2` 为基准，源码一旦推进即需重新核对。
