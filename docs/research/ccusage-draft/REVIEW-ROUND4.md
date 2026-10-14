# REVIEW-ROUND4：修正落地复核（第 4 轮复核员 R5）

> 复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md`（终稿 **3486 行**，修正装配后版本）
> 基准报告：`REVIEW-FINAL-A.md`（§1–§8，必改 2 + 装配 6）、`REVIEW-FINAL-B.md`（§9–§13 + 附录，必改 4 + 内部矛盾 4 + 装配 5）
> 源码样本：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（只读；本轮未运行、未修改样本）
> 方法：逐条 grep 定位前轮必改项 → `read` 复读终稿原文 → 打开源码核验（15 条抽查）→ PowerShell 脚本扫描标题层级 / 表格列数 / 疑似半截段落 → 五处章界逐一回读
> 技能检查：本任务无匹配 skill（session 目录仅 `diagnose-windows-sandbox-acl`，与文档复核无关），按指派直接使用 read/grep/glob 与 pwsh 只读扫描

## 结论（一句话）

前两轮 **13 条必改项 + 装配项全部落地（未修项 0）**，修正未引入任何阻断性新错误；仅余 3 处**非前轮要求**的装配瑕疵（§3.2 的 `§9.2` 裸章内引用歧义、§10 标题 h2→h4 跳级、六章标题后双空行），**达到可交付标准**。

## 任务 A：前轮必改项验证（逐条）

| 来源 | 编号 | 前轮要求 | 结果 | 终稿行号与摘录 |
| --- | --- | --- | --- | --- |
| A | [E1] | §3.2 不得再称 Codex 在「归一阶段」拆缓存 | **已修** | 144–146：`input_tokens` 的语义是**未缓存的新增输入**…`而「含缓存输入」的来源会被拆成这个形状——**拆解发生在输出层**，不在解析归一阶段` / `（Codex 的解析只做上界钳制，扣减见 §5 §3 与 §9.2 的 input - cached - cache_write）` |
| A | [E2] | §7 §3.1 改为「两处生产调用 + 三处内联复制」 | **已修** | 1724：`它是"权威窗口判定"的**单一定义点**，生产调用有两处（adapters/common/src/lib.rs:41…；core/src/summary.rs:297…）。其余报表路径是**内联复制的等价逻辑**，不是调用`；1725–1727 列全三处（`blocks.rs:170-174`、`loader.rs:660-664`、`:758-766`） |
| A | [A1] | 两处被压行的列表要拆开 | **已修** | 494/495 分为两行（`<synthetic>` 归一、`-fast` 展示名）；560/561/562 分为 `**display**` / `**auto**` / `**calculate**` 三项 |
| A | [A2] | 「§13.2 的 E2」断链引用 | **已修** | 3349：`daily 实现漏了这条守卫（§13.2 第 2 条）`；§13.2 第 2 条（3388–3390）确为 daily sidechain 守卫 |
| A | [A3] | 「见 §4.1」等章内引用不再撞全局章号 | **已修** | 1148：`共 15 次（见本章 4.1）`；266：`（见本章「去重规则」）`；447：`（差异见本章第 8 小节）`；另 37–40 行新增全局「阅读约定」界定章内编号 |
| A | [A4] | §4 §9.5 表格补文件名 | **已修** | 673：`adapters/common/src/lib.rs:36-47`、`adapter-all/src/loader.rs:656-665` / `:756-768` / `:672-683` / `:564/582/585/625/726` 全部带全路径；原裸引用 `lib.rs:51/67` 改由 674 行的 `adapter-all/src/lib.rs:45/65` 承担 |
| A | [A6] | 「33 万字符」改口径 | **已修** | 3378：`6 份分片，合计约 25 万字符（约 33 万字节 UTF-8）` |
| B | [E1] | §9 4.1 的键名 `byAgent` → `agents` | **已修** | 3010：`agents`（`rust/crates/ccusage-adapter-all/src/report.rs:148-156`；`report.rs:136-143` 是 period/metadata 的插入点）；§7 §10.3（2116）同写 `"agents": [ ... ]`，两章不再矛盾 |
| B | [E2] | 悬空引用 `E2` | **已修** | 同 A-[A2]（3349 改引 `§13.2 第 2 条`） |
| B | [E3] | §9 1.2 XDG 表述与 §9.5 表冲突 | **已修** | 2787：`也没有 XDG 参与**配置查找**…XDG 变量在别处出现三处`（`claude/src/paths.rs:46`、`http.rs:39`、`opencode/src/paths.rs:6`/`:29`）；与 3070–3075 环境变量表逐行一致 |
| B | [E4] | 「33 万字符」口径 | **已修** | 同 A-[A6] |
| B | [M2] | §12 对 §11 条数的描述 | **已修** | 3361：`§11 列了 8 条「文档与实现不符」（另 2 条是文档与代码一致的正面例子）`；§11 表 3314–3323 实为 8 条不符 + 第 9/10 条标 **一致** |
| B | [M4] | 「只有 30 行」→ 33 行 | **已修** | 3353：`（common/src/lib.rs:49-81）：实现只有 33 行` |
| B | [M5] | 白名单须含 `zai/` | **已修** | 3342 枚举含 `/zai/`；2173、3134 同含 `zai/`——三处一致，与 `core/build.rs:203-221` 的 13 条前缀规则对齐 |
| B | [装配] | §9 父节标题 + §10 编号 + 标题前空行 + §10 未验证节说明 | **已修** | 2748 `### 1. 配置文件体系`，下辖 2750–2860 的 `#### 1.1…1.7`；3142–3294 为 `#### 10.1…10.8`；标题前空行 3307 / 3371 / 3422 均已补；3296 `### 未验证/存疑（本节同时覆盖配置/CLI/输出章与版本史章的未决项）` |

**统计**：必改项 13 条（A 8 条 + B 9 条，其中字节口径与断链引用两处重叠）= **已修 13 / 未修 0 / 修得不彻底 0**。
唯一「按建议原文照做但仍有歧义」的一处见任务 C 的 N1。

## 任务 B：抽查清单（15 条，含修正段落）

| # | 抽查位置（终稿行） | 断言 | 源码位置 | 结果 | 源码摘录（≤3 行） |
| --- | --- | --- | --- | --- | --- |
| 1 | §3.2（144–146）**修正段** | 解析只做上界钳制，扣减在输出层/落列处 | `codex/src/parser.rs:1310-1316`、`codex/src/report.rs:123-129`、`adapter-all/src/loader.rs:845-855` | 正确 | `usage.cached_input_tokens = usage.cached_input_tokens.min(usage.input_tokens);` / `input_tokens.saturating_sub(cached_input_tokens.saturating_add(cache_creation_tokens))` |
| 2 | §4 §9.5 表（673）**修正段** | 三个裸行号 + 五个调用点均为 `adapter-all/src/loader.rs` | `loader.rs:656-665/672-683/756-768`、调用点 `:564/582/585/625/726` | 正确 | `fn filter_daily_summaries_by_date(…)`（656）/ `fn filter_session_summaries(…)`（756）/ grep 五个调用点全部命中 |
| 3 | §5 §3（836–873）**修正段** | 增量语义、全 0 跳过、输出层减法 | `codex/src/parser.rs:479-508`、`:1284-1308`、`:1310-1316`、`report.rs:123-129` | 正确 | `.filter(\|_\| cumulative_advanced)`（489）/ `if raw_usage.input_tokens == 0 && …`（501-508） |
| 4 | §6 §4.1 引用（1148、1394–1399）**修正段** | 「本章 4.1」目标正确；零件与重复度量化成立 | `common/src/lib.rs:18/36/49/84`、`:113-124`、`qwen/src/loader.rs`（全 9 行） | 正确 | `results[index] = Some(value);`（119）/ `qwen/src/loader.rs` 实测 9 行、`load_entries` 在 `:5` |
| 5 | §7 §3.1（1715–1729）**修正段** | 两处调用 + 三处内联，全部行号 | `date_utils.rs:190-203`、`common/src/lib.rs:41`、`core/src/summary.rs:297`、`blocks.rs:170-174`、`loader.rs:660-664/758-766` | 正确 | `crate::date_within_range(date_fn(row), …)`（297-301）/ `date_within_range(&entry.date, …)`（41-45）/ 三处内联逐行命中 |
| 6 | §9 §1.2 + §9 §4.1（2787、3010）**修正段** | XDG 三处；`agents` 键与插入点 | `claude/src/paths.rs:46`、`ccusage/src/http.rs:39`、`opencode/src/paths.rs:6/29`；`all/src/report.rs:136-156` | 正确 | `env::var("XDG_CONFIG_HOME")` / `env::var_os("XDG_CACHE_HOME")` / `const XDG_DATA_HOME_ENV` / `obj.insert("agents".to_string(), …)`（152-155） |
| 7 | §6 §4.4 `hermes` 行（1457） | SQL 静态内联 `:54-72`、只读 `:42` | `hermes/src/loader.rs:42-72` | 正确 | `sqlite::Connection::open_with_flags(db_path, sqlite::OpenFlags::new().with_read_only())`（42-43）/ `FROM sessions WHERE model IS NOT NULL AND TRIM(model) != ''`（69-71） |
| 8 | §7 §6.2（1855–1879） | 整点锚、严格大于、gap 块、`div_euclid` | `blocks.rs:53-103`、`:105-107`、`:149-164`、`date_utils.rs:58-60` | 正确 | `if since_start > session_duration \|\| since_last > session_duration {`（75）/ `Self(self.0.div_euclid(MILLIS_PER_HOUR) * MILLIS_PER_HOUR)`（59）/ gap 块 `id: format!("gap-{}", …)`（152） |
| 9 | §7 §8.2（2039–2052） | totals 含 extra、credits 仅 >0、块级四桶 | `output.rs:79-121`、`blocks.rs:213-219`、`all/src/report.rs:174-203` | 正确 | `.saturating_add(extra)`（109）/ `if credits > 0.0 { value["credits"] = json!(credits); }`（113-115）/ 容器键 `rows_key`（205-211） |
| 10 | §8 §4.2（2364–2374） | 归一化只换 `.`/`@`、不折叠大小写 | `pricing.rs:2724-2731` | 正确 | `if value.contains(['.', '@']) { Cow::Owned(value.replace(['.', '@'], "-")) }`（2726-2727） |
| 11 | §8 §6.2（2514–2535） | Codex 桶计价与长上下文切分阈值 | `codex/report.rs:343-348`、`codex/aggregate.rs:476-487`、`pricing.rs:2753-2763` | 正确 | `let cache_read = if pricing.cache_read_explicit { pricing.cache_read } else { pricing.input };`（344-348）/ `let is_long_context = event.input_tokens > crate::pricing::long_context_split_threshold(model);`（481） |
| 12 | §10 §10.7（3262–3278） | v20 三态 match 与缺价条件（**仅复验 v20 侧**） | `core/src/cost.rs:31-46`、`:48-59` | 正确 | `CostMode::Display => cost_usd.unwrap_or(0.0)`（40）/ `CostMode::Calculate => calculate_cost_from_tokens(…)`（44）/ `if mode == CostMode::Display \|\| (mode == CostMode::Auto && cost_usd.is_some())`（55） |
| 13 | 附录 A（随机 3 条之一） | `MTIME_PRUNE_MARGIN_MS` = 1 天 | `claude/src/paths.rs:84` | 正确 | `const MTIME_PRUNE_MARGIN_MS: i64 = MILLIS_PER_DAY;` |
| 14 | 附录 A（随机 3 条之二） | `CACHE_CREATE_1H_INPUT_MULTIPLIER` = 2.0 | `core/src/cost.rs:7` | 正确 | `const CACHE_CREATE_1H_INPUT_MULTIPLIER: f64 = 2.0;` |
| 15 | 附录 A（随机 3 条之三） | `DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS` = 200 000 | `core/src/pricing.rs:220` | 正确 | `pub(crate) const DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS: u64 = 200_000;` |

补充（未单列成行，同一轮打开）：`core/src/lib.rs:59-61`（5.0 / 3 / 100 三常量）与 `:63-82`（`BUILT_IN_AGENT_NAMES` 恰 18 项、顺序与附录 A 一致）、`blocks.rs:18/20`（0.8 / 120）、`rust/Cargo.toml:65`（`sqlite = { version = "0.37.4", … ["bundled","linkage"] }`）——全部与终稿一致。
`--no-cost` 行（675）称「装载层零引用」：`adapter-all/src/loader.rs` 全文无 `no_cost`，仅 `shared.offline`（`:85`），与 `adapter-all/src/lib.rs:51/67` 的透传事实一致。

## 任务 C：新发现的问题

**结论：修正过程未引入阻断性新错误。** 全文重扫 + 行号/符号 grep 交叉校验通过，仅 3 处非前轮要求的装配瑕疵与 2 处扫描假阳性。

- **N1（低危，A-[E1] 修正的遗留）**：146 行写 `扣减见 §5 §3 与 §9.2 的 input - cached - cache_write`。`§9.2` 目标实为 **§4 的 9.2 小节**（638 行 `**9.2 token 列对齐…**`），但 §9 章实有 `### 2. CLI 解析`；按 37–40 行「阅读约定」（章内引用沿用所在章编号）也无法解析（§3 无 9.2）。建议补写为「§4 §9.2」。此措辞沿自 A 报告 [E1] 的建议原文，属可接受残留。
- **N2（低危，装配遗留）**：脚本扫出全文**唯一**标题跳级——3142 行 `#### 10.1` 挂在 3139 行 `## 10.` 之下（h2→h4），§10 缺 `###` 父节；而 §9 有 `### 1.` 同款父节。前轮只要求编号改为 `#### 10.1…10.8`（已满足），层级本身未要求调整。
- **N3（极低危，渲染无影响）**：章标题后连续两个空行共 6 处——§5（751–752）、§6（1080–1081）、§7（1496–1497）、§8（2142–2143）、§9（2737–2738）、§10（3140–3141）；B 报告只点名 §9，实为分片拼接的系统性产物（§1–§4 无此现象）。
- **扫描假阳性（无需处理）**：表格列数校验仅报 2 处（1504、3161），回读确认是单元格内的**转义竖线** `\|`（`YYYY-MM-DDTHH:MM:SS[.mmm](Z\|±HH:MM)`、`管道 \| jq`），Markdown 写法正确，非断列。
- **半截段落：未发现。** 脚本按「非终止行紧邻标题」扫描零命中；并逐一回读五处章界——§8/§9（2719–2748）、§9/§10（3134–3142）、§10/§11（3305–3308）、§11/§12（3325–3331）、§13/附录 A（3419–3425）——句子与列表均完整收尾。
- **自引用行号无错位**：grep `行 \d{3,4}`、`3469|3486` 等模式，全文无「正文行 N」类自引用（唯一命中 3216 行的 `第 109 行` 指向 v19 源码，非本文）。
- **修正未造成新矛盾**：§3.2 ↔ §5 §3 ↔ §4 §9.2 三处扣减口径现已互洽；白名单三处枚举（2173 / 3134 / 3342）与 `build.rs:203-221` 一致；§12.3「8 条不符」与 §11 表格、§13.2 引用一致。

## 仍未解决 / 无法验证

1. **无「修正前」副本**：临时目录未留装配中间态，本轮无法做逐字符 diff；「未引入新错误」由全文重扫 + 行号/符号交叉校验 + 关键段落源码复读支撑（15/15 命中），不等于形式化 diff 证明。
2. **继承前轮自标的未验证项**（本轮未推翻、亦未闭环）：内嵌 LiteLLM 快照键集（编译产物不存在）、运行期行为（`format_currency` 舍入、statusline 分叉、`find_cache` 并发）、`nix/tools/models-dev-gen` 生成器、`docs/guide/config-files.md` 多文件回退表述。
3. **§10.7 的 v19 侧未复验**：本轮只打开 v20 侧 `core/src/cost.rs:31-59`；`ccusage-v19/apps/**/data-loader.ts:1284-1336` 沿用 B 报告结论。
4. **本轮未运行任何二进制、未改动样本仓库**；所有结论均为静态核对。
