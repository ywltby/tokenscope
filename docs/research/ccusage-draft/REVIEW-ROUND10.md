# REVIEW-ROUND10：终检员 D1 —— 第 9 轮修正的落地验证

- 对象：`docs/research/2026-10-14-ccusage-usage-statistics.md`（3949 行，UTF-8，2026-10-14 13:24:34 写入）
- 样本：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`（HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31）、v19 对照 `ccusage-v19`（`4aedac0`）
- 方法：正文逐段读取 + 源码逐行核对 + 解压构建产物 `litellm-pricing.json.deflate` 取内嵌主表真身 + 重跑 `check-linebounds.ps1` / `check-refs.ps1`（以 UTF-8 读入后 `Invoke-Expression` 执行，原脚本无 BOM，直接跑会被 pwsh 5.1 误读成 ANSI）
- 范围：只读取，未修改任何样本或源码；本报告只记结构、键名、行号与统计口径

## 结论（一句话）

**第 9 轮的 6 处修正全部正确落地**——每处的源码依据经独立复核成立，且 §10.4 与 §10.8-4 不再互相矛盾、§10.8 的 4 个交叉引用目标小节确实讲对应内容；但同批编辑留下 **2 处断行残片（§5、§9 引言）**，另有 **3 处数字/措辞需要刷新或收紧**（§13 的「2113 条」「52 处」、§13.5 的脚本 tie-break 说法、§4.5(a) 的「等长 43 的三条键」），**5 章「裸文件名约定」中 §4/§6/§7 干净、§5/§9 带残片**。

## 任务 A：6 处修正验证

### A1. `claude-3-5-haiku-20241022` 归属（§4.5(a) 尾段 L2437；§13.5 L3510-3514，任务书写的「L3540 附近」实为 L3510-3514）

- **核对方式**：读 `pricing.rs:2176-2196`（`claude_3_5_haiku` 定义 + 两条 `put_builtin_entry`）、`pricing.rs:1980-1983`（`put_builtin_entry` 实现）、`pricing.rs:1559-1574`（`find_entry_or_alias_with_fallback`）；解压 `target/debug/build/ccusage-core-30b61b361638d3b0/out/litellm-pricing.json.deflate`（4080 B → 41 839 B JSON、534 键）取出全部 haiku 键与 `i` 字段；全文 grep `us.anthropic`、`haiku`、`模糊命中`。
- **结论：正确落地。**
  - `:2191-2192` 确实把 `claude-3-5-haiku` 与 `claude-3-5-haiku-20241022` 写进 `entries`；`:1565-1567` 的 `self.entries.get(model).copied()` 是 fallback 链的第一步（精确阶段），因此该名字**不进模糊扫描**的结论成立。
  - 三个 Bedrock 变体在主表内确实存在且同价：`anthropic.claude-3-5-haiku-20241022-v1:0`（40 字符，`i:8e-7`）、`eu.…`（43，`8e-7`）、`us.…`（43，`8e-7`）；`pricing.rs:4951-4954` 的 `0.8e-6` 断言与三条候选同值，故「无法区分命中路径」的判断成立。
  - 全文已无「模糊命中 `us.anthropic…`」旧表述：`us.anthropic` 仅 2 处（L2201 快照拼写示例、L3527 旧模型两道防线），`模糊命中` 6 处均为 sonnet-4/其他模型/脚本误判语境（L3566 明写「真实运行时由兜底表精确键作答」）。
- 注意两处**未修的措辞**（非本处修正目标，但落在同一句里）：见任务 C 的 C2、C4。

### A2. §10.4 / §10.8-4 的 v19↔v20 门控（L3247、L3305——任务书写「L3308 附近」，实际在 L3305）

- **核对方式**：读 v19 `apps/ccusage/src/adapter/codex/parser.ts:193-236`、v20 `rust/adapters/codex/src/parser.rs:465-505`，并对照文档 §5.3 的代码块（L849-859）。
- **结论：措辞精确、修正到位。**
  - v19（`:205-212`）：`let raw = parsed.lastUsage; if (raw == null && parsed.totalUsage != null) { raw = subtractRawUsage(...) }`——**无条件**优先 `lastUsage`，`previousTotals` 只在 `totalUsage != null` 时前移。文档「v19 无此门控、`lastUsage` 存在即无条件采用」成立。
  - v20（`:483-494`）：`cumulative_advanced = total_usage.as_ref().is_none_or(|t| previous_totals.as_ref() != Some(t))`；`last_token_usage.filter(|_| cumulative_advanced).or_else(|| total - previous)`。文档「顺序相同，但多一道『累计值必须前进』门控，未前进时丢弃 `last_token_usage` 改走累计差分」**准确**。
  - 唯一可补注的一点：`total_token_usage` 为 `None` 时 `is_none_or` 返回 `true`，门控**视为通过**（仍按 `last_token_usage` 取值，与 v19 一致）。文档未显式点明这一情形；因 §5.3 已给出完整代码块（含 `is_none_or`），口径可自证，建议顺手加半句以免读者把「None」误当「未前进」。
  - §10.4 与 §10.8-4 现文字互不冲突，且都与 §5.3 的 L863-864 一致。

### A3. §10.8 的交叉引用（第 3/4/5/9 条）

- **核对方式**：按文首「阅读约定」（L40-44：`§4.6` = 第 4 章第 6 小节）核每个目标小节的实际标题与内容。
- **结论：4 个目标全部对得上。**
  - `§4.4` → 第 4 章第 4 节「去重规则（重点）」（`### 4.`，L381）= Claude 去重。✓
  - `§5.3` → 第 5 章第 3 节「token 拆分与增量语义（重点）」（`### 3.`，L845），L864 正是「累计未前进的重复快照被丢弃」。✓
  - `§9.3` → 第 9 章第 3 节「参数校验（互斥/冲突、非法值、未知命令）」（`### 3.`，L2985），L3004 含「双格式 + 真实日历校验（含闰年）」。✓
  - `§8` → 第 8 章定价（L2158 起）。✓
  - 顺带核了同批显式化的 `§9.2.4`（L2962「分层帮助与 `help_codegen`」）与 `§9.1.5`（L2857「配置文件里的取值校验」）也各自对位。✓

### A4. §11 第 2 行的 `rust/crates/ccusage-core/src/types.rs:175`

- **核对方式**：读 `ccusage-core/src/types.rs:168-180`；grep 全文 `types\.rs:1[0-9][0-9]`。
- **结论：正确落地、无裸引用残留。** `types.rs:175` = `pub model_breakdowns: Vec<ModelBreakdown>` ✓。全文出现该串的只有 L3341（已带全路径）与 L3597（修正记录元文本）；其余 §11/§3/§7 处的 `types.rs` 引用语境自洽。

### A5. 附录 A 的「日志级别」行（L3909）

- **核对方式**：读 `ccusage-core/src/logger.rs` 全文（实测 15 行）；读 `docs/guide/environment-variables.md:61-90`；`git grep LOG_LEVEL`（`rust/`、`packages/`、`apps/`）。
- **结论：正确落地。** `logger.rs` 全文 15 行，只有 `debug_log(&SharedArgs, …)` 与 `log_level() -> Option<u8>`（读 env 后 `parse::<u8>()`），**不含任何 0–5 级别表** ✓；级别语义表确实在 `environment-variables.md:65-74`（0 Silent/Errors only、1 Warn、2 Log、3 Info default、4 Debug、5 Trace），与附录 A 括号内描述一致，「默认 3」也对。
- 提示（P3）：`docs/guide/{amp,claude,codebuff,codex,copilot,droid,gemini,grok,hermes,kilo,openclaw,opencode}/index.md` 的环境变量表里也各有一行「0 silent … 5 trace」简述。若「位置」列意在做**唯一出处**声明，宜写「完整映射表见 `docs/guide/environment-variables.md`（各 agent 文档另有同一行简述）」。

### A6. §14.1.3 的 `bytes.len() >= 10`（L3660-3667）

- **核对方式**：读 `project_names.rs:116-123`；按 `bytes[3..].starts_with(b"Users\\")` 的语义算长度下界。
- **结论：修正正确，「≥9」这个数字对。** `b"Users\\"` 是 6 字节，`bytes[3..]` 要装下它需 `len ≥ 9`；`bytes[1]`/`bytes[2]` 的索引存在性只需 `len ≥ 3`（被 ≥9 覆盖）。9 字节的反例 `X:\Users\a` 恰好命中，故 9 就是**匹配下界**，10 是留余量——文档现述（「保守下界，匹配 `Users\` 只需 ≥9」）与 C4 报告对 `>= 10` 与索引安全无关的判断同向，不冲突。

## 任务 B：新增内容验证

### B7. 五章「本章裸文件名约定」抽样（5 条，逐条解析 → 验证存在 → 验证行号内容）

| 章 | 抽的引用 | 按约定解析为 | 实测 | 结论 |
| --- | --- | --- | --- | --- |
| §4 | `paths.rs:303-319`（及 `:290-331`，L259-262） | `rust/adapters/claude/src/paths.rs`（555 行） | L290-331 = `extract_session_parts` 全函数，L303-307 = 两段分支、L308-319 = `subagents` 分支，与正文描述逐项吻合 | ✓ |
| §5 | `parser.rs:483-494`（L3247） | `rust/adapters/codex/src/parser.rs`（1586 行） | L483-494 = `cumulative_advanced` + `filter` + `or_else(差分)`，正是 Codex 累计门控 | ✓ |
| §6 | `grok/src/parser.rs:170`（L1104） | `rust/adapters/grok/src/parser.rs`（1165 行） | L170 = `fn split_input_tokens(...)`，与总表「grok 减 cached」一项相符 | ✓ |
| §7 | `blocks.rs:53`、`:75`（L1525） | `rust/crates/ccusage/src/blocks.rs`（618 行） | L53 = `identify_session_blocks`；L75 = `since_start > session_duration \|\| since_last > session_duration`（严格大于断开） | ✓ |
| §9 | `config.rs:258-269`（L2786） | `rust/crates/ccusage-config/src/config.rs`（1404 行） | L258-269 = `discover_config_paths`（cwd 的 `.ccusage/ccusage.json` → 各 Claude 目录，无合并） | ✓ |

- 附加校验：§4 内裸 `parser.rs:N` 的最大行号为 753 < `ccusage-cli-parser/src/parser.rs` 的 1247 行，裸 `paths.rs:N` 最大 517 ≤ 555 行——约定与引用规模自洽。
- **但 §5、§9 的约定行本体带断行残片**（§4/§6/§7 干净），见 C1。

### B8. §13.6「第 9 轮」小节的数字核对

- 抽样数：`60+59+63+44 = 226` ✓；逐路结果与四份报告逐条对上——C1（60 条、59 一致，报告 L9）、C2（59 条全部一致 + 独立解压 534 键，报告 L6）、C3（63 条、62 一致 + 实测 `--by-agent`/`--jq`，报告 L10）、C4（44 条、43 一致 + 全局结构终检通过，报告 L7/L11）。✓
- 行号越界扫描：**「2113 条」与终稿不符**。以 `check-linebounds.ps1` 重跑（对象＝当前 3949 行终稿、主仓索引 342 文件）得「已解析并检查 **2125** 条；越界引用：无」。且「全部 `文件:行号` 引用」措辞过强：全文共 2284 处匹配，其中 ~159 条因简写路径（`codex/report.rs`、`core/types.rs`…）无法解析而**未入扫描**，**33 条 v19 引用（`ccusage-v19/…`）也不在索引里**（脚本只索引主仓）。
- 引用完整性：**「14 个章号、52 处」与终稿不符**。重跑 `check-refs.ps1`：章级引用共 **73 处**、涉及 **13 个章号**（§1、§3–§14；`§2` 未被任何引用使用），全部指向存在章节；「14」显然是把「文档有 1–14 章」当成了被引章号数。
- 结论：「0 越界」「全部指向存在章节」这两个**安全性结论成立**；两个**计数需刷新**（§13.1 表 L3413 与 §13.6 L3582-3583 各一处）。

## 任务 C：新问题

### C1（P1，须修）§5、§9 引言的约定行带断行残片——原「覆盖/行号约定」行被半行覆盖

- **§5 L762**（= 分片 `S2-codex.md:7`）：行尾为 `…指 rust/crates/ccusage-adapter-all/src/。EADME.md}\`、\`rust/adapters/common/src/lib.rs\`、\`rust/crates/ccusage-core/src/{pricing,model_aliases}.rs\`、\`rust/crates/ccusage-adapter-all/src/{loader,report}.rs\`、\`rust/crates/ccusage-cli*/src/*\`、\`docs/guide/codex/index.md\``。这与上一行 L761「- 覆盖：`rust/adapters/codex/src/{…}.rs`、`rust/adapters/codex/{README.md,src/README.md}`」连看即知：**原「覆盖」行的后半段被吞**（`README.md` 的 `R` 也丢了），属于插入约定行时的半行覆盖事故。修复建议：把 L761 恢复为完整一行（codex 文件集 + `codex/{README.md,src/README.md}` + `common/src/lib.rs` + `core/{pricing,model_aliases}.rs` + `adapter-all/{loader,report}.rs` + `cli*/src/*` + `docs/guide/codex/index.md`），L762 只保留约定句。
- **§9 L2764**（= 分片 `S6-config-output-history.md:9`）：行尾为 `…指 rust/crates/ccusage-cli/src/types.rs。号（read/grep 实测），未做估算。`；而上一行 L2763 恰好缺了尾巴（结尾为「（read/grep 实测）。」）。修复建议：L2763 补回「，未做估算。」，删掉 L2764 尾部残片「号（read/grep 实测），未做估算。」。
- 判定：这两处**不是第 9 轮新引入**（同形残片已在 S2/S6 分片里），但**属于第 9 轮编辑后仍未收口**的可读性缺陷，且正处在新增内容上，必须修。

### C2（P2）§13.5 L3514「（并因等长 tie-break 曾取 `us.`）」与实现/别处结论冲突

`replay-match.js:59-72` 的实现是「长度降序、等长取**字典序较小**者」，README（`evidence-round8/README.md:68`）也如此声明；按该实现等长命中会是 `eu.`——L2437 也是这么推的（「会按同长度取字典序较小者选中 `eu.` 而不是 `us.`」）。L3514 的「曾取 `us.`」缺少与现存证据的对应，建议删除或改写为「会把该名字误报进模糊扫描（并漏掉 `exact_only` 门禁）」。

### C3（P2）证据目录 README 与正文结论不一致（附带发现）

`evidence-round8/README.md:54-55` 仍写「`claude-3-5-haiku-20241022` **无裸键，靠模糊匹配落到** `us.anthropic.…-v1:0`」，与同文件 L72-73（正确结论）及终稿 §13.5 冲突。终稿 L3513 明确把读者导向该 README 的「使用边界」，读者若从文件头读起会先看到旧结论。建议同步删除 L54-55 中的 haiku 半句（保留 sonnet-4 半句，它本身正确）。

### C4（P3）§4.5(a) L2437 的「等长 43 的三条键」措辞

实测三条候选长度是 40（`anthropic.`）、43（`eu.`）、43（`us.`）——「等长 43」只适用其中两条。主句结论（会选中 `eu.` 而非 `us.`）不受影响，建议改成「同长的 `eu.`/`us.` 两条…」。

### C5（P3）§5/§13.5/§13.6 的行数与范围标注

- §13.1 表与 §13.6 都写「逐节读完**全文 3890 行**」、C4 范围写「L3344–L3890」，而终稿是 3949 行（§13.6 自身、§14 补充段等都在 3890 行之后）。作为历史记录可保留，建议加一句「复核时 3890 行，编辑后终稿 3949 行，行号范围以复核时为准」。
- §13.5 内有两处讲同一件事（L3513-3514 与 L3563-3567 的「重放脚本的适用边界」），详略不同、无矛盾；若求紧凑可合并，非必须。

### C6 其他结构检查（均通过）

- 表格断列：对全文所有 Markdown 表格做列数一致性扫描（表头/分隔行/数据行竖线数比对），**0 处异常**（比「抽查 5 张」更强的全量检查）——含 §13.6 新增表、附录 A/B。
- 编号衔接：§13.1→13.2→13.3→13.4→13.5→13.6 连续，随后 `## 14.` 正常开头；§14.1.1–14.6、附录 A→B 顺序正常；无孤儿小节。
- 无重复段落（§13.6 与 §13.1 的第 9 轮行属「一处详述、一处索引」，非重复）。
- §13.6 表格格式、6 条修正列表的换行与缩进均正常（无被 edit 打乱的缩进）。

## 仍需处理

1. **P1** 修 §5 L762 与 §9 L2764 的断行残片（含分片 S2/S6 同源残片），恢复 L761/L2763 的不完整句。
2. **P1** 刷新 §13.1 L3413 与 §13.6 L3582 的两组数字：越界扫描 → 「2125 条已解析、0 越界（另 ~159 条简写路径 + 33 条 v19 引用未纳入机械扫描）」；引用完整性 → 「73 处章级引用、13 个章号，全部存在」。
3. **P2** 处理 §13.5 L3514 的「曾取 `us.`」与 C3 的 README 旧结论（两处均可 1 行改掉）。
4. **P3**（可选润色）L2437「等长 43 的三条键」、§10.4 补注「`total_token_usage` 缺失时门控视为通过」、§13.1/§13.6 的行数时间线说明、附录 A「位置」列的唯一性表述。
5. 本报告只做验证，未改动终稿；`REVIEW-ROUND10.md` 未提交，交由父代理统一处理提交批次。
