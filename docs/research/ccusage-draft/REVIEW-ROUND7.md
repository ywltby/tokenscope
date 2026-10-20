# REVIEW-ROUND7：终稿最终复验（第 7 轮复验员 R7）

> 复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md`（终稿 3768 行）
> 源码样本：ccusage 只读仓库 HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（v20.0.31）；TokenScope 只读仓库（本轮未运行二进制、未修改任何源码与终稿）
> 方法：§14 与 §12 逐条打开源码核对（附摘录）；任务 B 十条改动点逐条 grep + read 复核；结构扫描（标题层级 / 表格列数 / CRLF / BOM）
> 技能检查：本任务无匹配 skill（会话仅 `diagnose-windows-sandbox-acl`，与文档复验无关），按指派直接使用 read/grep/glob + pwsh 只读扫描

## 结论（一句话：可否交付）

**可交付**：第 5–6 轮 10 条改动全部落地（0 未修）；§14 抽查 22 条断言全部与源码一致、§12 TokenScope 侧 8 条引用全部为真；结构终检（LF 3768、无 BOM、标题零跳级、59 张表无断列）通过。仅余 2 处措辞级与 1 处计数级小瑕疵、1 处一行更新建议（均不阻断交付，见「仍需处理」）。

## 任务 A：§14 抽查（22 条）

| # | 位置（终稿行） | 断言要点 | 源码核对（摘录 ≤3 行） | 结果 |
| --- | --- | --- | --- | --- |
| 1 | 3447 §14.1.1 | 别名格式 `k=v`、trim、空丢、后者覆盖 | `project_names.rs:3-17` `split(',')` → `split_once('=')` → 两侧 trim → 空丢 → `collect`（重复 key 后者覆盖） | 一致 |
| 2 | 3449 §14.1.1 | 精确命中原始 key → 否则清洗后再查一次 | `project_names.rs:19-25` `aliases.get(project)` 先返回；否则 `parse_project_name` 后再 `get` | 一致 |
| 3 | 3452 §14.1.1 | 消费者仅 `parse_project_aliases` 与 `output.rs:260`，「grep 命中 17 处」 | 实测严格 pattern `project_aliases` = **16 处**，全部为定义/再导出/传参/构造，无第二消费点 | 结论成立；**计数差 1** |
| 4 | 3461-3462 §14.1.2 | 步 2/3：`Users` 段后 `index+3..` / `index+2..`、`-` 重拼 | `project_names.rs:32-57` 两分支同构：`index+3 < len` → 取后段，`index+2 < len` → 降一档，`join("-")` | 一致 |
| 5 | 3464-3465 §14.1.2 | 步 5/6：≥5 段全 hex → 末两段；首个 `--` 截断 | `:63-73` `split('-').count() >= 5` 且全 `ascii_hexdigit/'-'/'.'` → `parts[len-2..]`；`split_once("--")` 取前半 | 一致 |
| 6 | 3466-3467 §14.1.2 | 步 7/7b：>20 且含 `-`；12 个噪声词；`<6` 字符且 ≥3 段取末三段 | `:74-106` 噪声词逐个对上（dev/development/feat/feature/fix/bug/test/staging/prod/production/main/master/branch）；`last_two.len()>=6` 否则 `>=3` 段取三 | 一致 |
| 7 | 3468-3469 §14.1.2 | 步 8/9：再 trim；清空回退原始 | `:108-113` `trim_matches('/'|'\\'|'-')`；空则 `project.to_string()` | 一致 |
| 8 | 3473-3474 §14.1.2 | 层级压平性质与测试证据行号 | `:151-158` `/Users/alice/ccusage → ccusage`、`/Users/alice/Development/ccusage → ccusage`；步 1 只认小写 `unknown`（`:28-30`） | 一致 |
| 9 | 3479-3487 §14.1.3 | `is_windows_users_path` 判据与 `len>=10` 下限 | `:116-123` 与文中代码块逐字一致；反例 `C:/Users/`、`c:\users\` 不入本分支 | 一致 |
| 10 | 3493-3494 §14.1.4 | 分组头用清洗名、比较用原始 project；daily 传 `instances||project`、其余 `false` | `output.rs:260` `parse_project_aliases`、`:263-276` `current_project != Some(project)` → `format_project_name`；`output.rs:464-467` `Project: {project}`；`commands/mod.rs:35-39`（daily）、`:106`、`:135-142`、`:172-179`（均 `false, None`） | 一致 |
| 11 | 3495-3496 §14.1.4 | `--project-aliases` 仅挂 claude daily 子命令 | `parser.rs:492-499` `-i/--instances`、`-p/--project`、`--project-aliases` 同处解析 | 一致 |
| 12 | 3502-3504 §14.1.5 | JSON 输出原串、`--project` 精确比较原始 slug | `output.rs:147-155` `group_project_output` 用 `row.project.unwrap_or("unknown")`；`claude/paths.rs:65-79` `is_project_path_segment` 判定后收集 | 一致 |
| 13 | 3543 §14.2.3 #5 | `droid`：契约上 D、落地 C（loader 降级） | `droid/parser.rs:33-43` 非 NotFound 读错 → `Err`、JSON 失败 → `cli_error`；`loader.rs:29-40` `unwrap_or_else(|error| { debug_log(...); None })` | 一致 |
| 14 | 3552 §14.2.3 #14 | `antigravity` 唯一真中断：逐库 `?` 冒泡 | `loader.rs:24` `match parse_sqlite_file(&database_path)?`；`parser.rs:146-152`（打开失败）、`:306-309`（page_count 缺失）、`:339-356`、`:381-404` 全 `cli_error` | 一致 |
| 15 | 3539 §14.2.3 #1 | `claude` 四种丢行条件 + 文件打不开仅影响该文件 | `claude/lib.rs:505-507` `let Ok(content) = fs::read(path) else { return loaded_file; }`；`:509-523` marker/反序列化/时间戳/有效性四连 `continue` | 一致 |
| 16 | 3540 §14.2.3 #2 | `codex` 读行失败 `return None`、三连 continue | `codex/parser.rs:139-144` `let Ok(bytes_read) = reader.read_until(...) else { return None; }`；`:145-167` 分类/反序列化/无 token/无时间戳依次 `continue` | 一致 |
| 17 | 3556 §14.2.3 #18 | `zcode` 行级有效性（空 id/session/model、`started_at<=0`、零用量→`None`）+ 四连 debug 降级 | `zcode/parser.rs:34-40` 四个判定 → `return None`；`:59-61` 零用量 → `None`；`loader.rs:90-117` 打开/`busy_timeout`/`query_only`/schema 任一失败 → debug + 空 | 一致 |
| 18 | 3568 §14.2.5 | 全仓无任何坏行计数 | `grep -r 'bad_lines\|badLines\|bad_line'` 全仓 **0 命中**；零件注释 `jsonl.rs:29-30` "silently skipped" | 一致 |
| 19 | 3591 §14.3.2 | `canonicalize` 全仓 9 命中 = 5 生产 + 4 测试 | 生产：`antigravity/paths.rs:68`、`goose/paths.rs:28`、`zcode/paths.rs:32`、`:117`、`adapter-all/loader.rs:544`；测试：`zcode:171/172/211`、`antigravity:136`（测试名） | 一致（恰好 5 处） |
| 20 | 3595-3599 §14.3.2 | 五处回退语义逐一（antigravity `path.clone()`、goose `unwrap_or(path)` + `is_file()`、zcode 两处、`path_key`） | `antigravity:68-70` `seen` 存 canonical/`paths` 存原路径；`goose:28-29` `unwrap_or(path)` 后 `is_file()`；`loader.rs:543-545` `path.to_path_buf()`；`:551-553` `paths_overlap` 相等或前缀 | 一致（zcode `:117` 一处细注见「仍需处理」） |
| 21 | 3582-3587 §14.3.1 | 遍历侧词法判断、拒绝软链；openclaw 显式立场与测试 | `common/lib.rs:18-34` `entry.file_type()`（不跟随）+ `read_dir` 失败静默 `return`；`openclaw/paths.rs:56-58` `if file_type.is_symlink() { continue; }`；`openclaw/sqlite.rs:14-21` 注释立场、`:51-57` `symlink_metadata`、`:354-380` `#[cfg(unix)]` 测试 | 一致 |
| 22 | 3618-3625 / 3644-3650 §14.4 | 准入判据 7 条与行号；`BUILT_IN_AGENT_NAMES` 18 项三向对齐 | `source-support-qa.md`（56 行）：`:3`、`:5`、`:11-16`、`:18`、`:22-26`、`:30-34`、`:37-44`、`:49-55` 逐条命中；`core/lib.rs:63-82` 18 项顺序一致；`adapter-all/loader.rs:119/125/131/308/323/329/345` = `BUILT_IN_AGENT_NAMES[0/1/2/14/15/16/17]`；`config.rs:395-397`、`:218-222`、`:1245` 一致性测试 | 一致 |
| 23 | 3662-3667 §14.5.1 | `ENV_LOCK` 中毒恢复、`EnvVarGuard` RAII、批量逆序、`Fixture`、`fs_fixture!`、zcode fixture | `test-support/lib.rs:15-19` `unwrap_or_else(\|e\| e.into_inner())`；`:21-47` Drop 还原/`remove_var`；`:49-81` `.rev()` 逆序；`:83-127`；`:135-144`；`zcode.rs:20-81`/`:84-113` 两表 3 行、`as_millisecond()` | 一致 |
| 24 | 3675-3678 §14.5.2 | 4 个 e2e 文件行数与套路 | 实测 741 / 137 / 122 / 46 全中；`claude_cli.rs:16-42` `env_clear()` + `HOME/USERPROFILE/XDG_CONFIG_HOME/CLAUDE_CONFIG_DIR` + 三断言；`:44-52` `set_times`；`codex_cli.rs:123-137` 注入组合与 `env_remove("CLAUDE_CONFIG_DIR")` 一致 | 一致 |

## 任务 B：改动落地（逐条）

1. §3.2「拆解发生在输出层」——**已修**（L149「**拆解发生在输出层**，不在解析归一阶段」；配套 L150 指向 §5 §3）。
2. §4 §9.5 表三处裸行号——**已修**（L677 `adapters/common/src/lib.rs:36-47`、`adapter-all/src/loader.rs:656-665` / `:756-768` / `:672-683` / `:564/582/585/625/726` 全部带路径；L678 接住原 `lib.rs:45/65`。同表仍有少量同文件简写如 `loader.rs:108-115`、`output.rs:158-159`、`loader.rs:83-89`，属可接受风格，非误导）。
3. §5 §3「不预先扣减」与 §3.2 不再互斥——**已修**（L859「cached 不从 `input_tokens` 里扣…只在输出层做减法」与 L149-150 同口径；`report.rs:123-129` 佐证）。
4. §6 的「见本章 4.1」引用——**已修**（L1152「共 15 次（见本章 4.1）」→ §6.4.1 L1399「15 家用 `read_files_parallel`（只有 `antigravity` 不用）」，目标存在且支持该数）。
5. §7 §3.1「两处生产调用 + 三处内联复制」——**已修**（L1728：两处调用 `adapters/common/src/lib.rs:41`、`core/src/summary.rs:297`；L1729-1731 列全三处内联 `blocks.rs:170-174`、`loader.rs:660-664`、`:758-766`）。
6. §9 1.2 XDG 三处 + §9 4.1 `agents` 键名——**已修**（L2791 三处枚举：`claude/src/paths.rs:46`、`http.rs:39`、`opencode/src/paths.rs:6`/`:29`；L3014 `agents`（`report.rs:148-156`）；与 L3074-3079 环境变量表一致）。
7. §2.2「19/18/17 三个数字的口径」——**已修**（L98-101：别名 19 / `BUILT_IN_AGENT_NAMES` 18 / 分发数组 17）。
8. §1 术语表无「漏斗 / 计价链」——**已修**（全文 grep 两词 0 命中）。
9. §13.1 已是 7 轮复核表——**已修**（L3380-3388 七行，第 7 轮为 R7 本行）。
10. §6 总表最后两行 cost 列——**已修**（L1120 claude「是 `costUSD`…」、L1121 codex「否（会话日志内无成本字段）；成本按 pricing 计算…」；「未核对（其他成员负责）」0 命中）。

**统计：已修 10 / 未修 0。**

## 任务 C：TokenScope 侧核对（8 条 + 1 条附加）

| # | 文档引用（§12） | TokenScope 源码事实（摘录） | 结果 |
| --- | --- | --- | --- |
| 1 | `src/source/claude.rs:58-68,364`（cache_write 单桶） | `:58-68` `ClaudeUsage` 只有 `cache_creation_input_tokens` 一个缓存写字段；`:364` `cache_write_tokens: usage.cache_creation_input_tokens` | 相符 |
| 2 | `src/pricing.rs:200-210`（PriceRates 四字段） | `:203-210` `input/output/cache_write/cache_read: RateSpec`，注释「每项三态…不静默为 0」 | 相符 |
| 3 | `src/source/codex.rs:4-6,314-325`（忽略 `token_usage_record`） | `:4-6`「只取 `info.last_token_usage`；`token_usage_record` 是累计回显，一律忽略并计数」；`:314` `"token_usage_record" => stats.ignored_token_usage_record += 1` | 相符 |
| 4 | `src/dedupe.rs:9,26-42`（Claude 去重键） | `:9`「同 `(session, message.id)` 保留时间戳最晚的一条」；`:27` `key = (session_id, record_id)`，`:29` 更早丢弃、`:33-38` 相同/更晚则行序靠后者胜 | 相符 |
| 5 | `src/pricing.rs:2304-2415`（estimate 取最高） | `:2391-2400` `highest_complete_cost` / `highest_partial_cost` 两阶段池；`:2402-2415` 池内取费用最高、同价按 tie-rank；注释「禁止跨候选拼价」 | 相符 |
| 6 | `src/report.rs:1262-1323`（指纹缓存） | `:1266-1268` `(size, mtime)` 指纹；`:1287/:1292` `fp_before`/`fp_after` 双取；`:1294-1299` 不稳定则不登记（警告） | 相符 |
| 7 | `src/model_identity.rs:141-157`（边界偏移） | `:141-157` `separator_boundaries`：只保留 `0 < offset < 全长` 并去重（首尾符号不产生边界） | 相符 |
| 8 | `src/pricing.rs:1737-1750`（无内置表） | `:1737-1742` `Default for Pricing` → `Self::empty()`，注释「没有显式来源时不会偷偷给价（不变量 1）」 | 相符 |
| 9（附加） | §12.1 行 6「全 `src/` 无 block 语义」 | grep `src/` 下 `block` 仅命中普通词义（代码块/邻接块），无 5 小时计费窗口实现 | 相符 |

**8/8 条与源码相符，0 条不符。**

## 任务 D：结构终检

- **换行与编码**：`LF = 3768`、`CRLF = 0`、**无 BOM**（首字节非 `EF BB BF`），UTF-8 有效。
- **标题层级**：`h1=1`（L1）、`h2=17`（目录 + 14 章 + 附录 A/B）、`h3=94`、`h4=131`；**无 `#####`/`######`；相邻标题零跳级**（R5 记录的「§10 h2→h4」已修为 `### 10.1…10.8`）。
- **表格**：全文 59 张表，逐行按未转义 `|` 切分后**列数全部一致，0 处断列**（含表格内转义竖线与代码样式单元格）。
- **目录一致性**：目录列 1–14 章 + 附录 A/B，与正文 `##` 章标题逐一对应；§14 位于 3430–3704 行，与任务书范围一致。**唯一不一致**：L38「阅读约定」仍写「本文档章节（**1–13** 与附录）」，未随第 6 轮新增 §14 更新（见下）。

## 仍需处理（如有）

均为低危、不阻断交付：

1. **[建议交付前顺手改，一行]** L38 阅读约定：「`## N.` 是本文档章节（**1–13** 与附录）」→ 应为「**1–14** 与附录」（第 6 轮新增 §14 后漏更新；目录与正文均已是 14 章，此处是唯一残留）。
2. **[措辞，低危]** 3543 行区（§14.6 第 3 条）称 `C:/Users/...` 与 `c:\users\...`「按代码会落**步 3/步 4** 分支」；实测两者都落**步 4**（`is_windows_users_path` 对正斜杠与小写均返回 false，且不满足 `/Users/`、`-Users-` 前缀）。表述可读作「步 3 或步 4」，但不精确。
3. **[措辞，低危]** §14.3.2 zcode `paths.rs:117` 行「失败 → debug 日志 + 丢弃该根」：日志受 `if configured` 约束（`:120`），默认根失败是静默丢弃；goose / antigravity 才是无条件回退。建议注明「仅配置根记日志」。
4. **[计数，低危]** §14.1.1「grep 命中 17 处」实测为 **16 处**（严格 pattern `project_aliases`：定义 1 + 再导出 1 + output.rs 5 + config 2 + cli-parser 2 + cli types 1 + commands 1 + tests 1 + config_schema 2）。「唯一消费者为 `output.rs:260`、其余均为传参/构造」的结论不受影响；建议改为「16 处」或注明口径。

以上 4 条均**不影响已核实的任何结论**；第 1 条为唯一「文档自查一致性问题」，其余为措辞/计数精度。除此之外，本轮未发现任何新的断言错误、证据错位或结构缺陷。
