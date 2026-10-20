# REVIEW-COVERAGE：ccusage 研究报告的覆盖度审查（第 6 轮）

- 审查对象：`docs/research/2026-10-14-ccusage-usage-statistics.md`（3486 行 / 368 911 B / SHA-256 `5291FD18…3DEE1C`）
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（v20.0.31，只读，未运行任何二进制）
- **本轮定位：覆盖度审查**，不重复 R1–R4 / FINAL-A/B 已完成的「行号与断言是否属实」证伪（那 6 份已抽验 200+ 条）。本轮问的是另一个问题：**用户想摸清的东西，是否真的被回答了**。
- 方法：① 提取全文 215 个标题并逐章核对目录；② 对 5 个原始问题逐项定位章节与子项；③ 用「仓库文件清单 ∩ 报告引用」求差集找漏项；④ 对候选漏项逐个打开源码确认存在性与语义；⑤ 用关键词计数法校验横向覆盖（`symlink`/`zst`/`project-aliases`/`instances`/`LOG_LEVEL`/`log_level` 等）。
- 判定口径：**已完整回答** = 原始问题列出的每个子项都能在文档里找到成节的答案；**部分回答** = 主体在、但有子项被跳过；**未回答** = 找不到成节内容。

## 结论（一句话）

覆盖度明显高于同类拆解文档：5 个原始问题中 **3 个已完整回答、2 个部分回答**，**无任何被遗漏的 agent 或本地数据源**（18 家与 `BUILT_IN_AGENT_NAMES` 18 项逐一对应），主链条（发现→解析→去重→聚合→计价→渲染）**没有整段缺失**；真正的缺口集中在两类——「横向汇总层」（容错策略只在 Claude/Codex 与文件级有表，16 家的行级坏数据策略未成表）与「与 TokenScope 直接相关、但报告未当作机制处理的旁枝」（项目名规范化与别名、选源准入判据、符号链接立场、测试隔离基建）。

## 五个原始问题的覆盖判定（表格）

| # | 原始问题 | 判定 | 对应章节 | 缺口说明 |
| --- | --- | --- | --- | --- |
| 1 | **怎么读取**（文件在哪 / 怎么发现 / 怎么解析 / 怎么容错 / 怎么去重） | **部分回答** | §4.1–§4.4（Claude）、§5.1–§5.5（Codex）、§6.1–§6.4（16 家）、§6.4.4（SQLite 7 家） | 「位置 / 发现 / 解析 / 去重」4 项完整（含 18 家默认根+环境变量总表、遍历与排序规则、`entry.file_type()` 语义、三档去重键与回放路由）。**「容错」只做到文件级**：§6.4.5 的总表覆盖「目录读不到 / home 解析失败 / 显式配置非法 / 文件级读取失败」，但**「读到了、某行是坏的」这一级没有横向汇总**——16 家分片里只有 `droid`（JSON 解析失败 = 硬错误，L1223）与 `openclaw`（`cost` 容忍非对象，L1290）两处是明确的异常数据策略陈述（另有 `kimi` 的「必须丢弃 `session` 级记录」L1276，属防双算过滤，不是坏数据） |
| 2 | **怎么聚合**（按什么维度 / 时间怎么算 / 会话与项目怎么归并） | **部分回答** | §7.1（聚合原语）、§7.2（时区）、§7.3（日期过滤）、§7.4（周月）、§7.5（会话）、§7.6（blocks）、§7.9（输出）、§7.10（by-agent） | 维度、时间口径、会话归并、排序规则都完整。**项目维度只讲了一半**：`--instances` / `-p` 的分组渲染、JSON 容器改名（`projects`）、`(project_path, session_id)` 归组都有（散见 §4.9 L663、§4.10 L680-689、§7.0 结论速览 L1509、§7.8.1 L2033、§7.9 L2089），但**「项目名从哪来、怎么清洗、别名怎么映射」整段空缺**——`core/src/project_names.rs`（141 行生产代码）在全文只被当作 `short_model_name` 的引用出处出现 1 次（§4.5，L661） |
| 3 | **怎么计算金额**（价格从哪来 / 模型名怎么匹配 / 公式 / 缺价怎么办） | **已完整回答** | §8.1–§8.11 | 4 类价格来源、加载与覆盖顺序、`Pricing` 全字段、匹配链（归一化 → 精确 → 边界回退 → `exact_only` 门禁）、4 个给定名字的命中推演、别名三层、服务档倍率、通用与 Codex 两条公式、`extra_total_tokens`/`credits` 归属、缺价三处披露、cost mode 三态、峰谷时间定价、缓存与离线、数字格式化——闭环完整，无子项遗漏 |
| 4 | **具体代码实现**（函数 / 行号 / 关键常量） | **已完整回答** | 全文 + 附录 A + 附录 B + §13 | 全文含 1340 个按「文件名 + 起始行」去重后的 `文件:行号` 锚点（我的实测口径见「无法判定」第 4 条）；附录 A 21 行常量与阈值、附录 B 64 个模块与入口索引；§13 记录 4 轮复核（R1–R4 共 252 条抽样）与 10 条残留未决项，且逐条标注「已验证 / 一致解释 / 未验证」 |
| 5 | **尽可能覆盖 ccusage 支持的全部 agent** | **已完整回答** | §6.1 总表（18 行）、§6.3 逐家分片（16 家，每家 10–17 行） | 仓库 `rust/adapters/` 共 19 个目录（含 `common`），agent 适配器 18 个；`BUILT_IN_AGENT_NAMES`（`core/src/lib.rs:63-82`）同样 18 项。报告 §6.1 的 18 行与之一一对应，**无遗漏**（claude/codex 由 §4/§5 独立成章） |

**关于 §2.2 的「17 个 agent」**：经核实**不是矛盾**——`main.rs:92-112` 的测试数组恰为 17 个 `run`（列表含 `codex`，**不含 `claude`**），而 `claude` 走独立子命令分支；`adapter/mod.rs:1-19` 的 19 个别名 = `all` + 18 家。即 17 = 除 Claude 外的 agent 子命令数、18 = 适配器数、19 = 别名导出数。三处数字各有定义，但需读者自行拼合（见「可读性」第 3 条）。

## 结构性缺口

### A. 有 ccusage 支持、但报告完全没提的 agent 或数据源？

**没有。** 逐一核对结果：适配器目录 18 家（amp / antigravity / claude / codebuff / codex / copilot / droid / gemini / goose / grok / hermes / kilo / kimi / openclaw / opencode / pi / qwen / zcode）在报告 §6.1 总表与 §6.3 分片中全部出现，且每家的「默认根 + 环境变量 + 存储格式 + 关键字段 + 是否 SQLite + 自有 cost 字段」都有条目。

数据源形态也齐（按 §6.1「格式」列逐行数）：JSONL 7 家（copilot / pi / grok / kimi / openclaw / qwen / gemini）、SQLite 7 家（opencode / openclaw / antigravity / hermes / zcode / kilo / goose，§6.4.4 有专门对照表）、单 JSON 对象或数组 4 家（amp / codebuff / droid / gemini）、OTel 导出（copilot）、protobuf blob（antigravity）；openclaw 与 gemini 是双通道，故上述分类有重叠。

次级发现（不是「遗漏 agent」，而是「漏掉一份关键情报」）：仓库有 `docs/guide/source-support-qa.md`（56 行），内容是 ccusage 的**选源准入判据**与**探过但拒绝的源**——

- 准入底线：「本地时间戳 + 会话身份 + 模型身份 + token 计数或可映射的记账成本」，四者缺一即不支持；
- 明确的拒绝立场：「不从文本长度估算 token」，因为那会让日报/月报/会话/成本报表**看起来精确、实际是猜的**；
- 已否决的源：Devin CLI（用量只在云侧，没有本地用量日志）；标注「曾经不支持、现已支持」的源：Antigravity、Grok Build CLI。

报告 §6 只回答了「这 18 家怎么读」，没回答「**为什么是这 18 家 / 别的工具为什么不支持**」。这份判据对 TokenScope 的候选 agent 排期决策是直接可用的（见建议清单第 3 条）。

### B. 「读取 → 聚合 → 计价」链条上被跳过的环节

先排除四个常见怀疑项——**它们都不是缺口**，报告都有成节内容：

- **时区**：§7.2.3（解析与优先级、`local`/`UTC`/IANA、`TZ` 不由 ccusage 读取而由 jiff 读、未知时区静默回退 system 并由 CLI 入口显式校验）；
- **日期边界**：§7.2.4（跨日边界 = 目标时区下的日历日，测试给出 UTC 23:30 在 `Asia/Tokyo` 落次日）+ §7.3（`--since/--until` 闭区间字符串比较、`--last` 周期语义、非法组合表）；
- **排序**：§7.1.6（`filter_and_sort_summaries` / `sort_summaries`）+ §7.5.4（会话排序）+ §1.1（文件枚举按完整路径字符串排序，直接影响去重幸存者）；
- **输出契约**：§9.4（JSON 序列化约定、`print_json_or_jq`、`sanitize_terminal_text`、TTY 与紧凑模式、表格渲染）+ §8.1（行级 JSON 与 `totals_json`）。

真正薄的有三处，按影响排序：

1. **行级坏数据策略（横向）**。现有素材足以支撑一张表但没成表：坏行走 `serde_json::from_slice(...).ok()` 静默丢弃（`common/src/jsonl.rs:47` 的 `records` 是统一零件）；Claude 明确写了「静默跳过，无计数无告警」（§4.3.2）；`droid` 把 JSON 解析失败**升级为 `cli_error`**（`droid/src/parser.rs:32-43`）；`antigravity` 的 SQLite 打开失败**返回 Err 并打断该库解析**（`antigravity/src/parser.rs:146-152`）；`lenient_*` 系列则让「字段级坏值」不丢行（§6.2.1 已列全 8 个函数）。**缺的正是「归谁升级、归谁丢弃」这一列**。
2. **项目名的推导、清洗与别名**（`core/src/project_names.rs:1-141`）。完整规则链一次都没被写下来：`parse_project_aliases` 解析 `k=v` 逗号列表 → `format_project_name` 先查原始 key 再查清洗后 key → `parse_project_name` 依次做 Windows `Users\` 分段截取、`-Users-`/`/Users/` slug 前缀处理、5+ 段 hex-ish 截断取末两段、`--` 后缀截断、>20 字符时滤掉 `dev/dev…/feat/fix/test/main/master` 等噪声词取末两段、空值与 `unknown` 归一为 `Unknown Project`，失败回退原串。调用点只有一处：`output.rs:255-276` 的 `--instances` 分组头。
3. **符号链接与 canonicalization 立场（未成不变量，且全仓并不统一）**。报告已覆盖两处点状事实（L1141 的「`entry.file_type()` 判定，不跟随符号链接」、L1288 的 openclaw「跳过符号链接」），但**没提炼成不变量，也没写出「谁在什么场合做了相反的事」**。实测全仓**两种取向并存**，且分界不在适配器之间，而在「遍历」与「判定」两类动作之间。

**目录遍历一律 lexical**：`common/src/lib.rs:18-34` 用 `entry.file_type()`（不跟随）；`openclaw/src/sqlite.rs:14-21` 的注释把理由写成设计约束——「目录遍历保持 lexical（不跟随 symlink、不做 canonicalization），因为**发现必须与实际读取的遍历一致**」；`sqlite.rs:51-56` 用 `symlink_metadata` 实现 `is_directory_without_symlink`／`is_file_without_symlink`，并有测试 `ignores_symlinked_database_paths`（`sqlite.rs:356`）；`opencode/src/loader.rs:206-209` 给出第二个理由——**symlink 循环会递归到栈溢出**，且「跟随会让检测认领了收集阶段拒绝读的文件」。

**而「单文件解析目标」与「来源重叠检测」会 canonicalize**，共 5 处：`antigravity/src/paths.rs:68`、`goose/src/paths.rs:28`、`zcode/src/paths.rs:32`、`:117`，以及 `ccusage-adapter-all/src/loader.rs:544`——最后一处正是 `paths_overlap` 的重叠判定基础，也就是 TokenScope SF09（两个启用来源不得相同或嵌套）在 ccusage 侧的对应实现。

报告 §6.4.5 只把「配置目录重叠 → 报错」写成表格里的一行（L1473），既没说明它靠 canonicalize 判定，也没说明它为何与目录遍历的 lexical 取向不冲突（前者判「来源是否同一」，后者判「目录项是否可读」）。

另有一处轻量问题：`--instances` 的**输出契约**分散在四处（§4.10 命令层、§7.8.1 JSON 容器、§7.9 渲染与 `Project:` 分组头、§9.2.2 选项清单），没有单一权威小节；对「按项目聚合」这个 TokenScope 核心需求，读者需要拼四处。

### C. 对 TokenScope 有价值、但报告完全没涉及的机制

1. **项目展示名的清洗与用户别名**（同上 B-2）。TokenScope 现有 `src/source/project_path.rs`、`src/source/claude_projects.rs`，而对外的项目展示名规则是「路径末段」且明确「展示名不是身份」。ccusage 给出的是一条**可选的更丰富展示策略**（路径清洗 + 用户别名映射），且解决了 TokenScope 目前没解决的问题：Windows 路径与 Claude slug 编码（`-Users-alice-ccusage`）在 UI 里怎么变成可读名字。
2. **选源准入判据与拒绝理由清单**（同上 A）。TokenScope 的候选 agent 排期（Gemini CLI / OpenCode 暂缓）需要的就是这套判据。
3. **测试隔离基建**（`rust/crates/ccusage-test-support/`）。`lib.rs:15-81` 提供全局 `ENV_LOCK` 互斥 + RAII 恢复的 `EnvVarGuard`/`EnvVarsGuard`（改环境变量后自动还原，避免并行测试互相污染），`Fixture` 包装 `assert_fs::TempDir`（`:83-87`），`zcode.rs:1-113` 是**直接建 SQLite schema + 插入代表行的合成 fixture 生成器**（`create_fixture`）。对照 TokenScope 的历史事故（测试未注入临时 `cache_dir`/`pricing_index`，把真实缓存 purge 成 fixture）与「真实日志永不入库 + 手工合成最小 fixture」约定，这是现成可抄的范式。
4. **多来源目录重叠的判定算法**。§6.4.5 已收录 `pi.stores` 重叠报错（`all/src/loader.rs:503-553`，`paths_overlap`），但只有一行，没写它**怎么判**——`loader.rs:544` 用 `canonicalize`（失败回退原路径）把候选路径规范化后再比前缀。这是 TokenScope SF09（两个启用来源不得相同或嵌套）在 ccusage 侧的对应实现，两者的判定口径（规范化 vs 分量前缀）与失败回退策略可以直接对照，值得在 §12 单列一条。

## 目录与术语一致性

**目录 ↔ 实际章节：一致。** `## 目录`（L19-35）列 13 章 + 附录 A/B，正文实际标题为 `## 1.`…`## 13.` + `## 附录 A：` + `## 附录 B：`，无缺号、无多号、标题与目录措辞吻合（仅目录用简称，如目录「4. 主干与 Claude Code 适配器（发现、并行、解析、去重、边界）」对应正文「## 4. 主干与 Claude Code 适配器」）。

**章内编号存在三套体系并存**（这是本报告最明显的结构瑕疵）：

- §4–§9：章内独立编号，`### 1. 文件发现` … `### 11. 给 TokenScope 的可复用清单`，子项 `#### 2.1`。§4 的第 0 节叫「主干坐标」，§5–§8 叫「结论速览」，§9 则**没有第 0 节**（直接从 `### 1. 配置文件体系` 开始）；
- §10：**跳级**——`## 10. 版本史` 下直接是 `#### 10.1 v19 的源码结构`，没有 `###` 层级父标题（渲染层级与 §4–§9 的子节不同）；
- §12 / §13：改用「章号.序号」——`### 12.1 结构性差异`、`### 13.1 复核流程`，与 §4–§9 的章内编号规则不一致。

L37-40 的「阅读约定」只解释了前两套（「`### N.` / `#### N.M` 是该章独立编号」→ 「§4.6 = 第 4 章的 4.6 小节」），**没有覆盖 §12/§13 的 `12.x` 形式**，按该约定反推会误读。另需注意 §7 章内第 7 节、§8 章内第 8 节与「§7」「§8」的章号同形，读者极易混（报告在 L38-40 已提示「跨章引用一律写出章号」，算是缓解）。

**术语：核心术语一致，术语表有 2 项失效。**

- 一致：`LoadedEntry`（20 次）、`LoadedFile`（5 次）、`CostMode` / cost mode（19 次）、`TokenUsageRaw`/`TokenCounts`、`entry / session / block` 三者的用法与其定义（行级/会话级/5 小时窗口）一致，未发现同一概念两种叫法的实质混用。
- 失效：§1 术语表定义的两个词**在全文各只出现 1 次（即定义处）**——「**漏斗**」（L57）与「**计价链**」（L58）此后从未被复用；正文改用「入口管线」「匹配链」等同义表达。属术语表冗余。
- 轻微：术语表把「事件」定义为 `LoadedEntry`，但「事件」在 §8 的 pricing 语境里也指「上游输入类型」（§8.3.2），局部有歧义，未造成误读。

**两处装配残留（读者可见的内部痕迹）：**

1. §6.1 总表最后两行（L1116-1117）的「自有 cost 字段」列写着「**未核对（其他成员负责）**」——这是分片装配的痕迹，且与文档实际状态冲突：Claude 的成本取值在 §4.7 成节、Codex 在 §5.8 成节。读者据此无法判断这两行到底完整不完整。
2. §12.2 第 2 条（L3349）引「§13.2 第 2 条」时附带「daily 实现漏了这条守卫」——FINAL-B 已指出该锚点在 §13.2 不存在 E 编号；本次复核确认 §13.2 用「语义级（S1）」等标签而非 E 编号，引用仍不精确。

**重复叙述：抽查未发现有害重复。** 我先按字面计数筛出高重复度主题（`--instances` 4 次、`input × 2`（1 小时缓存倍率）6 次、`mtime` 24 次、`isApiErrorMessage` 3 次、`saturating_add`（溢出语义）15 次），再逐一打开这些命中点确认是否属于无信息复述：结论是**都属于有意分层**——§12 作为「与 TokenScope 的对照」章，每条都重新带上行号并从 TokenScope 视角复述结论（例如 1 小时缓存倍率在 §8.6.1 讲公式、§12.1 第 1 条讲两边差异、§12.2 第 7 条讲可借用点；`saturating_add` 则分别出现在数据模型、各家归一、统一报表三处各自的上下文里），不是同一句话的拷贝。§13.2 的「保留以示证据强度」也已在文中自我说明用途。

## 建议补充清单（按价值排序）

| # | 补在哪 | 为什么值得补 | 需要读的文件 / 行范围 |
| --- | --- | --- | --- |
| 1 | §6 章内新增 `### 4.6 项目名的推导、清洗与别名`（插在 `### 5. 未验证 / 存疑`（L1475）之前），并在 §7.9 / §9.2.2 的 `--instances` 处加交叉引用 | 原问题 2「项目怎么归并」的缺口所在；TokenScope 有项目身份（`src/source/project_path.rs`）与「展示名不是身份」的口径，但缺一套把 Windows 路径与 Claude slug 编码变成可读名的规则，ccusage 这套已带测试可直接对照 | `rust/crates/ccusage-core/src/project_names.rs:1-141`（生产部分：`parse_project_aliases` 3-17、`format_project_name` 19-25、`parse_project_name` 27-114、`is_windows_users_path` 116-123；测试 143-164）；调用点 `rust/crates/ccusage-core/src/output.rs:255-276`；CLI `rust/crates/ccusage-cli-parser/src/parser.rs:492-497`；配置键 `rust/crates/ccusage-config/src/config_schema.rs:428-434`、`:640`、`:1085` |
| 2 | §6 章内新增 `### 4.7 行级坏数据策略对照`（紧随建议 1 的 4.6，与既有 4.5「缺失路径 / 空目录的分支」凑成三张同构表） | 原问题 1「怎么容错」的横向缺口；TokenScope 的 SF08 要求 `bad_lines` 计数与受检算术，必须先知道每家在「丢行 / 丢字段 / 丢文件 / 整命令失败」四档里落在哪一档——`droid` 与 `antigravity` 会升级为错误，直接影响采集可信度评估 | `rust/adapters/common/src/jsonl.rs:29-30`（模块注释明说「反序列化失败的行静默跳过，与历史行为一致」）、`:47`（`records` 本体）、`:75-196`（8 个 `lenient_*`）；`rust/adapters/droid/src/parser.rs:32-43`（`cli_error` 硬错误）；`rust/adapters/antigravity/src/parser.rs:146-152`（打开失败即 `cli_error`）；`rust/adapters/claude/src/lib.rs:505-519`；`rust/adapters/codex/src/parser.rs`（`serde_json` 落点，行号由 §5 已给出） |
| 3 | §6 章内新增一小节（如 `### 5. 选源准入判据与已否决的源`），或在 §11 后单列 | 回答「为什么是这 18 家」，并把 ccusage 的拒绝立场（「不从文本长度估算 token，否则报表看起来精确实际是猜」）与 TokenScope 的「未收录模型始终未知、不得按 0 静默吞掉」并置——这是两个项目共享的设计伦理，对 TokenScope 的候选 agent 排期（Gemini CLI / OpenCode 暂缓）是直接可用的判据 | `docs/guide/source-support-qa.md:1-56`（全文，含 Devin CLI 否决理由、Antigravity 与 Grok Build CLI 的「曾经不支持→现已支持」）；`rust/crates/ccusage-core/src/lib.rs:63-82`（`BUILT_IN_AGENT_NAMES` 18 项，作为「当前支持面」的权威清单） |
| 4 | §6.2（公共零件）或 §6.4.5 旁，单列「符号链接与路径规范化立场」 | 从点状事实升级为不变量，并写出「两种取向并存」的真实格局；它与 TokenScope 的取向**部分相反**（TokenScope 用规范化绝对路径做项目 key 并禁止来源嵌套；ccusage 在**目录遍历**上故意 lexical、拒绝 symlink，却在**来源重叠检测**上用 canonicalize），这个对照是 §12 目前完全缺失的部分 | `rust/adapters/common/src/lib.rs:18-34`（`entry.file_type()` 不跟随）；`rust/adapters/openclaw/src/sqlite.rs:14-21`（lexical 不变量及其理由）、`:51-56`（`symlink_metadata` 守卫）、`:356-380`（`ignores_symlinked_database_paths` 测试）；`rust/adapters/opencode/src/loader.rs:206-209`（循环炸栈与「检测须与收集一致」）；反向证据（5 处 canonicalize）：`rust/adapters/antigravity/src/paths.rs:68`、`rust/adapters/goose/src/paths.rs:28`、`rust/adapters/zcode/src/paths.rs:32`、`:117`、`rust/crates/ccusage-adapter-all/src/loader.rs:544` |
| 5 | §9 章内新增一小节（如 `### 7. 测试与 fixture 形态`），或作为附录 C | TokenScope 有「真实日志永不入库 + 手工合成最小 fixture + 解析类改动带 fixture 用例」的硬约定，还有测试未注入临时目录导致真实缓存被 purge 的事故史；ccusage 的 `EnvVarGuard`（全局互斥 + RAII 还原）与 SQLite `create_fixture` 是现成可抄的隔离模式 | `rust/crates/ccusage-test-support/src/lib.rs:1-40`（`EnvVarGuard` 与 `ENV_LOCK`）、`:40-87`（`EnvVarsGuard`/`Drop`/`Fixture`）；`rust/crates/ccusage-test-support/src/zcode.rs:1-113`（`create_fixture` 建表插行）；端到端形态 `rust/crates/ccusage/tests/{claude_cli,codex_cli,copilot_cli,zcode_cli}.rs`（报告已引 `claude_cli.rs:632-661`） |
| 6 | §7 章内新增 `### 11. --instances / -p 的项目分组输出契约`（把现散在 §4.10 / §7.8.1 / §7.9 的四处合并成一处权威描述） | TokenScope 的核心功能就是「按项目聚合查看」；目前读者要拼四处才能得到完整契约（分组键、JSON 容器改名、渲染分组头、`-p` 的单段目录名限制、过滤值非单段时退化为全量扫描） | `rust/crates/ccusage/src/commands/mod.rs:33-57`（`--instances/--project` 打开分组、JSON 分流）；`rust/crates/ccusage-core/src/output.rs:147-156`（`group_project_output`）、`:261-276`（`Project:` 分组头）；`rust/adapters/claude/src/daily.rs`（`group_by_project` 分组日汇总，报告 §4.8 L599 已定位测试 `main.rs:483-535`）；`rust/adapters/claude/src/paths.rs:248-254`（`is_project_path_segment`） |
| 7 | §6.1 总表最后两行（L1116-1117） | 装配残留（「未核对（其他成员负责）」）与文档现状冲突：Claude 成本在 §4.7、Codex 在 §5.8 都已成节。要么补齐这两行的「自有 cost 字段」列（Claude 有 `costUSD`、Codex 有 `total_token_usage`/`last_token_usage` 路径），要么改注「见 §4.7 / §5.8」，不要留内部分工痕迹 | 报告 §4.7（L537-577）、§5.8（L990-1007）；源码 `rust/crates/ccusage-core/src/types.rs:14-15`（`costUSD` 的 serde 位置）、`rust/adapters/codex/src/report.rs`（成本段，§5.8 已给行号） |
| 8 | §10 与 §12 / §13 的标题层级 | 统一编号体系，消除「跳级」与「三套并存」：§10 把 `#### 10.1` 提升为 `### 10.1`（或整章改为章内编号以贴合 L37-40 的阅读约定）；§12 / §13 的二层标题在阅读约定里补一句 `### 12.x` 形式，或同样改章内编号 | 报告 L3139-3296（§10 全部子节）、L3334-3370（§12）、L3374-3421（§13）、L37-40（阅读约定） |
| 9 | §1 术语表（L55-59） | 「漏斗」「计价链」定义后全文零复用（各仅 1 次，即定义处），属术语冗余；要么在 §4.2 / §8.4 正文改用这两个词（它们确实精确对应那两段内容），要么删除 | 报告 L55-59；对照 §4.2（入口管线）、§8.4（匹配链） |
| 10 | §2.2 的「17 个 agent」（L94-97）加一句口径说明 | 全文并存 17（dispatch 的 agent 子命令）/ 18（适配器与 `BUILT_IN_AGENT_NAMES`）/ 19（`adapter/mod.rs` 别名导出）三个数字，各自都对但需读者自己拼合；一句话即可消除歧义，也顺带说明「Claude 为何不带 `<agent> <report>` 形式」 | `rust/crates/ccusage/src/main.rs:90-113`（数组恰 17 项、不含 `claude`、`assert_eq!(runs.len(), 17)`）；`rust/crates/ccusage/src/adapter/mod.rs:1-19`（19 个 `use` 别名 = `all` + 18 家）；`rust/crates/ccusage/src/main.rs:25-67`（claude 走独立子命令分支） |

## 无法判定的事项

1. **未运行 ccusage 二进制**（与报告同一约束）。凡涉及「运行时行为」的判断（例如符号链接在真实 Windows 上被 `DirEntry::file_type()` 报告为哪种类型、`--instances` 分组在真实数据上的渲染）本轮都只做静态推断，未做运行期闭环。
2. **未逐字通读 3486 行**。本轮读完的是：215 个标题全部、§1 全文、§4.1–§4.3 与 §4.6 全文、§6 主干与 §6.1/§6.2/§6.3 抽样与 §6.4 全文、§7.2/§7.3 全文、§8.4 段与 §8.6 抽样、§12/§13 全文、附录 A 抽样，以及前序 6 份复核报告。**未读**：§4.7–§4.11 正文、§5 全文、§7.4–§7.10 正文、§8.1–§8.3/§8.5/§8.7–§8.11 正文、§9 全文、§10 全文、附录 B 全表。若这些段落存在覆盖问题，本轮可能漏判。
3. **未独立复现 §13.3 的 10 条残留未决项**（内嵌 LiteLLM 快照真实键集、`format_currency` 舍入边界、在线端点与内嵌快照差异、并发压力、`models-dev-pricing.json` 全量比对等）。它们都要求「运行」或「外部数据」，本轮不具备条件，故仅确认其被明确标注，不确认其结论。
4. **行号级全局校验未完成**。我尝试对全部 1340 个 `文件:行号` 锚点做越界批量检测，但因报告多用「基名 + 行号」形式（如 `lib.rs:107-188`），而 `lib.rs` 在多个 crate 中同名，文件名级索引产生大量假阳性（假阳性样本：指向 `claude/src/lib.rs` 的行号被拿去比对了行数仅 171 的其它 `lib.rs`），该检测结论不可用，已放弃。行号真实性的可靠结论以 R1–R4 / FINAL-A/B 的分片级抽查（200+ 条）为准，本轮不重复。
5. **§12 中「TokenScope 一侧」的断言未核对**（如 `src/pricing.rs:2304-2415`、`src/dedupe.rs:9,26-42`、`src/model_identity.rs:141-157` 等）。本轮只读 ccusage 仓库与 TokenScope 的文件清单/CLAUDE.md 口径，未打开 TokenScope 源码逐条核对；若需保证对照章两侧都准，应另派 TokenScope 侧复核者。
6. **`pricing.rs`（6181 行）、`opencode/loader.rs`（2922 行）等超大文件未通读**，只读了被报告引用的段落与本次新增结论所需的函数。
7. **本轮判定「没有遗漏 agent」的依据是仓库文件清单 + `BUILT_IN_AGENT_NAMES` + `adapters/` 目录三者的交叉核对**；若存在「未在 `BUILT_IN_AGENT_NAMES` 登记却仍可被统一报表读取」的源（例如仅通过 `pi.stores` 自定义配置接入的第三方源），本轮未穷举验证。
