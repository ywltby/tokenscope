## 14. 横向专题：项目名、坏行容错、路径立场与测试基建

本节补齐第 5 轮审查（R6）列出的 3 个横向缺口与 2 处高价值漏项，作为 §1–§13 的横向补充：项目名的推导与清洗链（§14.1）、行级坏数据策略对照（§14.2）、符号链接与路径规范化立场（§14.3）、选源准入判据（§14.4）、测试基建（§14.5）。

- 样本：`ccusage` 只读仓库 HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（v20.0.31）；本节所有行号均为本次实测（`read` / `grep` 逐文件核对），与各分片既有编号独立。
- 方法：静态阅读为主；第 8 轮已构建并运行 ccusage（`--offline`，见 §13.5）。未修改任何样本或源码。
- 口径：涉 18 家适配器时，Claude / Codex 与 §4、§5 互相引用，不重复展开；凡本节未逐行读到的分支，在表中标注核实方式并在 §14.6 列明。

### 14.1 项目名的推导、清洗与别名

**结论**：ccusage 的**项目身份**是适配器抽出的原始 `project` 字符串（Claude 侧是 `~/.claude/projects/<slug>` 的那一段目录名，`rust/adapters/claude/src/paths.rs:269-288`），分组、过滤、JSON 输出全用它；`format_project_name` 只在**表格分组头**做一次展示用清洗，别名表同样只影响展示。**展示名不是身份。**

#### 14.1.1 别名表：`parse_project_aliases`

| 项 | 事实 | 证据 |
| --- | --- | --- |
| 入口 | 只有两个：CLI `--project-aliases <RAW>`、配置 `commands.daily.projectAliases`（camelCase） | `rust/crates/ccusage-cli-parser/src/parser.rs:495-497`、`rust/crates/ccusage-config/src/config_schema.rs:433`、`:640` |
| 格式 | 逗号分隔 `key=value`；逐个 `split_once('=')`，两侧 trim，任一为空即丢该对，重复 key 后者覆盖（`collect` 语义） | `rust/crates/ccusage-core/src/project_names.rs:3-17` |
| 官方文档 | 与实现一致（`"uuid-project=Frontend App,long-name=Backend API"`） | `docs/guide/daily-reports.md:276`、`:282`、`docs/guide/configuration.md:224` |
| 优先级 | 精确命中原始 key → 直接返回；否则拿**清洗后**的名字再查一次别名 | `project_names.rs:19-25` |
| 生效范围 | 唯一调用点是表格渲染，且仅在 `group_projects` 为真时使用 | `rust/crates/ccusage-core/src/output.rs:260`、`:270-274` |

**注释与实现不一致（新发现）**：`config_schema.rs:433` 的 doc 注释写「JSON object or path mapping project paths to display aliases」，实现里没有 JSON 对象或文件路径分支——全仓 `project_aliases` 的消费者只有 `parse_project_aliases` 与 `output.rs:260`（grep 命中 16 处，均为传参/构造）。以代码与官方文档为准。

#### 14.1.2 清洗链：`parse_project_name` 的判断顺序

逐字对应 `project_names.rs:27-114`，**顺序不可交换**：

| 步 | 条件 | 动作 | 行号 |
| --- | --- | --- | --- |
| 1 | 空串或字面量 `unknown` | 返回 `Unknown Project`（唯一兜底字面量） | `:28-30` |
| 2 | `is_windows_users_path` 为真 | 按 `\` 切分，定位第一个 `Users` 段：其后 ≥3 段取 `index+3..`，否则 ≥2 段取 `index+2..`，用 `-` 重新拼接 | `:32-40` |
| 3 | 否则以 `-Users-` 或 `/Users/` 开头 | 按 `-` 或 `/` 切分（丢弃空段），同样定位 `Users` 段后取 `index+3..` / `index+2..` | `:41-57` |
| 4 | 其他形态 | 只做首尾 `/\-` 修剪，不改内容 | `:58-62` |
| 5 | 段数 ≥5 **且**全部字符属于 `[0-9a-fA-F.-]` | 只保留**最后两段**（hex/哈希形态压缩） | `:63-70` |
| 6 | 含 `--` | 在**第一个** `--` 处截断，只留前半 | `:71-73` |
| 7 | 含 `-` 且长度 >20 | 分段过滤：长度 ≤2 的段丢弃；小写后等于噪声词的段丢弃（`dev`/`development`/`feat`/`feature`/`fix`/`bug`/`test`/`staging`/`prod`/`production`/`main`/`master`/`branch`） | `:74-96` |
| 7b | 过滤后 ≥2 段 | 取最后两段拼接；结果 <6 字符且过滤后 ≥3 段则改取**最后三段**；否则保持原串不缩 | `:97-106` |
| 8 | — | 再修一次首尾 `/\-` | `:108` |
| 9 | 若已被清空 | 回退**原始未清洗**的 `project`（保证永不为空串） | `:109-113` |

两条易被误读的性质：

- 步 2/3 的「隔段取尾」是把 `Users/<用户>/<...>` 的层级压平：`/Users/alice/Development/ccusage` 经 `Users` 分段后取 `index+3..` → `ccusage`（测试 `project_names.rs:155-158`），而 `/Users/alice/ccusage` 只有 3 段（`index=0`），走 `index+2..` → `ccusage`（`:151-154`）——即用户目录下一级与多级都得到末段名。
- 步 1 只认小写 `unknown`，步 9 的兜底回退发生在所有裁剪之后，因此清洗链**不会**产生空展示名（除非原始即为 `unknown`）。

#### 14.1.3 `is_windows_users_path` 判据

```text
(bytes.len() >= 10 && bytes[1] == b':' && bytes[2] == b'\\' && bytes[3..].starts_with(b"Users\\"))
|| project.starts_with("\\Users\\")
```

证据：`project_names.rs:116-123`。判据的性质与边界：

- 只认两类形态：`X:\Users\...`（盘符 + 反斜杠 + `Users\`）与 `\Users\...`（无主机前缀的形态）。
- `bytes.len() >= 10` 是一个**保守下界**：要匹配 `Users\` 只需长度 ≥9（前缀 3 字节 + 6 字节字面量），代码取 10 是留余量，并非某处索引的最小安全值。
- **不走本分支**的反例：`C:/Users/...`（正斜杠）、`/Users/...`（落步 3）、`c:\users\...`（小写 `users`）。测试只覆盖 `C:\Users\phaedrus\Development\ccusage`、`\Users\phaedrus\Development\ccusage`（`rust/crates/ccusage/src/main.rs:149-161`）与 `/Users/alice/*`、`-Users-alice-ccusage`（`project_names.rs:147-163`），大小写与正斜杠形态**无测试**。

#### 14.1.4 调用点

| 调用点 | 作用 | 证据 |
| --- | --- | --- |
| 表格分组头 | `group_projects` 为真且 `project` 变化时插一行 `Project: <清洗名>`；分组比较用的是**原始** `project`（`current_project != Some(project)`），所以清洗后同名的不同 slug 仍各起一组 | `output.rs:260-276`；`project_header_row` `:464-467` |
| 谁传 `group_projects` | daily：`args.instances \|\| args.project.is_some()`；monthly/weekly、`bucket`、`session` 一律传 `false` | `rust/crates/ccusage/src/commands/mod.rs:35-39`、`:61-68`、`:106`、`:135-142`、session 渲染 `:172-179` |
| CLI 选项 | `--project-aliases` 仅挂在 `claude daily` 子命令上（`-p/--project` 同处） | `ccusage-cli-parser/src/parser.rs:492-499` |
| 配置键 | 结构体字段、`from_map` 读取、JSON Schema 生成三处 | `ccusage-config/src/config_schema.rs:428-434`、`:640`、`:1085` |

#### 14.1.5 展示名 vs 身份（key）

| 维度 | 身份（key） | 展示名 |
| --- | --- | --- |
| 分组 | `UsageSummary.project` = 适配器抽出的原始串 | 仅在分组头渲染时套 `format_project_name` |
| JSON | `summary_json` 直接输出 `row.project`（未清洗）；`group_project_output` 用 `row.project.unwrap_or("unknown")` 作 `BTreeMap` key | `output.rs:48-50`、`:147-155` |
| `--project` 过滤 | 先判「是否为合法单段目录名」，是则只收该目录（`projects/<filter>`），否则全量收；随后条目层 `entry.project.as_ref() != filter` 精确比较——比较对象是**原始 slug**，不是展示名 | `claude/src/paths.rs:65-79`、`:248-254`；`claude/src/daily.rs:103-106` |
| `--instances` 输出 | key 与 `detected` 判定都用原始 project | `ccusage/src/commands/mod.rs:45-50`、`claude/src/daily.rs:65-71` |

推论：`-Users-alice-foo` 与 `/Users/alice/foo` 可能都显示为 `foo`，但它们是两个组、两个 JSON key、两种 `--project` 取值；别名也不改变分组，只改变表头文字。此立场与 TokenScope 的「展示名（路径末段）不是身份、仅同名不合并」一致（`CLAUDE.md`「项目身份」条）。

### 14.2 行级坏数据策略对照

**结论**：18 家适配器共享同一批宽松反序列化零件（`rust/adapters/common/src/jsonl.rs`），但**没有统一的坏行策略**——绝大多数按「丢行 / 丢字段 / 丢文件」三档静默降级，只有 `antigravity` 会把单行/单库的坏数据升级成对用户可见的错误。**全仓没有任何坏行计数**。

#### 14.2.1 统一零件

| 零件 | 语义 | 行号 |
| --- | --- | --- |
| `records::<T>` | `byte_lines` 切行 → 可选 `LinePrefilter`（memmem 预筛）→ `from_slice::<T>(line).ok()`；**失败行静默丢弃**，不计数、不打日志 | `jsonl.rs:47-62`；注释明写 "silently skipped" `:29-30`、`:23-27` |
| `lenient_u64` | 非 u64（浮点/字符串/null/负数/缺失）一律 0 | `:75-84` |
| `lenient_i64` | 整数才 `Some`，浮点被拒 | `:95-101` |
| `lenient_f64` | 任意 JSON 数字都 `Some` | `:112-118` |
| `lenient_object` | 非对象或嵌套对象反序列化失败 → `None`（**不丢整行**） | `:131-141` |
| `lenient_array` / `lenient_vec` | 数组内坏元素丢弃；前者区分「空数组 vs 非数组」，后者折叠成空 `Vec` | `:155-170` / `:180-186` |
| `non_empty_string` | 非字符串、trim 后为空 → `None` | `:196-202` |
| 预筛语义 | `LinePrefilter` 的标记必须出现在「适配器会接受的每一行」上，否则该行在任何 JSON 解析前就被跳过 | `:23-27`、`:54-59` |

零件自带的回归测试明确了「宽松到什么程度」：浮点 token 变 0、字符串 token 变 0（`:259-271`）、非对象 cache 字段不丢行（`:305-349`）、数组内坏元素跳过（`:352-395`）。

#### 14.2.2 四档定义

- **A 丢行**：单行解析失败或被判定不适用 → 静默跳过，继续处理文件内其余行。
- **B 丢字段**：单字段类型/形态意外 → 取 `None`/`0`/空集合，保留该行其余可用信息（`lenient_*` 家族）。
- **C 丢文件**：文件或库级失败（读不到、打开失败、整体反序列化失败、schema 不符）→ `debug_log` + 空结果，报告照常渲染。
- **D 升级为错误**：返回 `Err` 并冒泡，**中断该 agent 的整份报告**。

#### 14.2.3 18 家分档表

| # | agent | 档位 | 关键证据（行号） | 本节核实方式 |
| --- | --- | --- | --- | --- |
| 1 | `claude` | C + A | 文件读不到 → 空 `LoadedFile`（`claude/src/lib.rs:505-507`）；逐行 marker 预筛、反序列化失败、时间戳失败、`is_valid_usage_entry` 为假 **四种** 情况均 `continue`（`:510-523`） | 逐行读 |
| 2 | `codex` | A（+ C，见 §5） | 读行失败即 `return None`（`codex/src/parser.rs:139-144`）；行分类不符、反序列化失败、无 token、无时间戳依次 `continue`（`:145-167`）；`replay.rs:404` 反序列化失败走 else 分支 | 逐行读 |
| 3 | `opencode` | A + B | 消息 JSON `from_slice(...).ok()?`（`opencode/src/parser.rs:275`、`loader.rs:542`）；字段级 `lenient_*`（`parser.rs:17-114`）；loader 内多处 `continue`（`loader.rs:335-445`、`:585-651`） | grep 定位 + 邻段 |
| 4 | `amp` | C + A | 整文件反序列化失败 → 直接返回空 `Vec`（`amp/src/parser.rs:96-101`）；事件缺时间戳/模型/tokens 逐个 `continue`（`:131-142`）；loader 把 `Err` 降级为 debug + 空（`loader.rs:22-30`） | 逐行读 |
| 5 | `droid` | **契约上 D，落地 C** | parser 对非 `NotFound` 读错与 JSON 解析失败返回 `Err`（`droid/src/parser.rs:33-43`），但 loader 用 `unwrap_or_else` 转成 debug 日志 + `None`（`loader.rs:29-40`） | 逐行读 |
| 6 | `codebuff` | C + A | 文件 `fs::read(path)?` 失败上抛、整体 JSON 失败返回空（`codebuff/src/parser.rs:41-44`）；行级 `continue`（`:49-59`、`:174-180`）；loader 降级（`loader.rs:30-39`） | 逐行读 |
| 7 | `hermes` | A + B | SQLite 列读失败即 `None`（`hermes/src/parser.rs:24-27`）；空桶且零成本的行丢弃（`:41-49`）；库打开失败 debug + 空（`loader.rs:41-53`） | grep 定位 + 邻段 |
| 8 | `pi` | A + B | 行级多重 `continue`（`pi/src/parser.rs:321-350`、`:508`）；字段级 `lenient_*`（`:22-92`） | grep 定位 + 邻段 |
| 9 | `goose` | A + C | 列读 `.ok()?`（`goose/src/parser.rs:25-27`、`:107-116`）；库打开/查询失败 debug + 空（`loader.rs:75-89`、`:101`） | grep 定位 + 邻段 |
| 10 | `openclaw` | A + B | 行级 `continue`（`openclaw/src/parser.rs:192-201`）；SQLite 行级 `continue`（`sqlite.rs:124-163`）；字段级 `lenient_*`（`parser.rs:25-155`） | grep 定位 + 邻段 |
| 11 | `kilo` | A + C | 库打开失败 / prepare 失败 debug + 空（`kilo/src/loader.rs:51-66`）；每列读失败 `continue`（`:71-79`） | 逐行读 |
| 12 | `copilot` | A + B | 整文件 `records::<CopilotRecord>` 收集（`copilot/src/parser.rs:131`）；会话状态行级 `continue`（`:254-303`）；属性级宽松解析（`:167-224`） | grep 定位 + 邻段 |
| 13 | `gemini` | C + A + B | 单文件 `fs::read_to_string(path)?` 只在该文件内上抛，loader 统一降级（`gemini/src/parser.rs:117`、`loader.rs:25-37`）；`records` 传 `None` 预筛（`parser.rs:176`）；行级 `continue`（`:190-202`） | 逐行读 |
| 14 | `antigravity` | **D（唯一真中断）** | 打开失败 → `cli_error`（`antigravity/src/parser.rs:146-152`）；`PRAGMA page_count` 缺值也 `Err`（`:306-309`）；行读失败、blob 解析失败逐条 `cli_error`（`:339-356`、`:381-404`）；loader 用 `?` 直接冒泡（`loader.rs:24`） | 逐行读 |
| 15 | `kimi` | A + B | 行级过滤 + `filter_map`（`kimi/src/parser.rs:127-128`）；字段级 `lenient_*`（`:24-102`） | grep 定位 + 邻段 |
| 16 | `qwen` | A + B | `records::<QwenLine>` + 预筛（`qwen/src/parser.rs:111`）；字段级 `lenient_*`（`:32-55`） | grep 定位 + 邻段 |
| 17 | `grok` | C + A + B | 整会话文件 `Err` 降级为 debug + 空（`grok/src/loader.rs:33-42`）；行级 `continue`（`parser.rs:219-230`、`:255-275`）；字段级 `lenient_*`（`:34-129`） | 逐行读 |
| 18 | `zcode` | A + C | 行级有效性判定（空 id/空 session/空 model/时间戳 ≤0/零用量 → `None`，`zcode/src/parser.rs:34-40`、`:59-61`）；列读 `.ok()?`（`loader.rs:278-289`）；打开、`PRAGMA busy_timeout`、`PRAGMA query_only`、schema 任一失败 → debug + 空（`loader.rs:90-117`） | 逐行读 |

上表已覆盖全部 18 家；标注「grep 定位 + 邻段」的 8 家（见 §14.6 第 9 条）只在证据粒度上弱于逐行读，其完整上下文链见 S5 分片 §4.4/§4.5，本节不重复。

#### 14.2.4 特殊者

- **`antigravity` 是唯一的 D 档**，且是**跨文件、跨行**的：`loader.rs:23-34` 对每个库用 `?`，因此**单个损坏或结构不符的 `conversations/*.db` 会让整个 antigravity 报告失败**（`CliError` 冒泡到命令层），其余库与其余 agent 不受影响。注释解释了「空库才跳过」（`:26-32`：`None` 表示未初始化的空快照），但没有任何「单库失败降级」分支。
- **`droid` 的「硬错误」只停在函数契约层**：`parser.rs:32-43` 的 `Err` 被 `loader.rs:29-40` 吸收为 debug 日志，净效果与 C 档相同。把它读成「droid 会因一个坏 `*.settings.json` 崩掉」是错的；反之把它读成「droid 与别家完全一样」也不准确——它的 `Err` 是**语义信息**（文件被跳过这件事在 `RUST_LOG=debug` 下可见）。
- **`claude` 是丢行条件最多的一家**：一个「坏行」只要满足「无 `"usage":{` 标记 / 反序列化失败 / 时间戳不可解析 / 未通过 `is_valid_usage_entry`」中的任一条就被丢（`claude/src/lib.rs:509-523`），而**文件打不开只影响该文件**（`:505-507`）。

#### 14.2.5 坏行计数结论

`grep -r 'bad_lines|badLines|bad_line'` 在整仓（含 `rust/`、`apps/`、`packages/`、`docs/`）**0 命中**。结合零件的注释与实现：

- 坏行**没有任何计数、没有输出、没有告警**；用户从报告里无法得知「有日志被丢弃」（`jsonl.rs:29-30`、`:47-62`）。
- 唯一可见的降级痕迹是**文件级** `debug_log`（例：`amp/src/loader.rs:24-29`、`grok/src/loader.rs:34-41`、`gemini/src/loader.rs:31-35`、`kilo/src/loader.rs:54-58`、`zcode/src/loader.rs:93-97`），且默认 `info` 级别下不可见。
- 对照 TokenScope 的 SF08（异常行计 `bad_lines` 跳过、累计溢出则明确报错）——**这是 ccusage 明确缺失、而 TokenScope 已具备的能力**；若要与 ccusage 行为对齐，需要显式决定「不计数」是否可接受，而不是默默继承。

### 14.3 符号链接与路径规范化立场

**结论**：ccusage 并存两种取向——**遍历/发现侧坚持词法（lexical）判断，拒绝跟随符号链接**；**去重/重叠判定侧使用 `canonicalize` 求真实同一性**，且 5 处生产调用全部带失败回退。二者不是矛盾，而是服务于不同问题；但边界处存在一处语义接缝（见本节末）。

#### 14.3.1 遍历侧：不跟随符号链接

| 位置 | 事实 | 证据 |
| --- | --- | --- |
| 通用递归收集 | 用 `entry.file_type()`（即 `symlink_metadata` 语义，**不跟随**）判类型：既不算文件也不递归目录；`fs::read_dir` 失败静默 `return`（目录不存在 = 空结果） | `rust/adapters/common/src/lib.rs:18-34`（`:24-32` 的判断、`:19-21` 的静默返回） |
| openclaw JSONL | 显式 `if file_type.is_symlink() { continue; }` | `openclaw/src/paths.rs:51-58` |
| openclaw SQLite | `is_directory_without_symlink` / `is_file_without_symlink` 用 `symlink_metadata` 判定 | `openclaw/src/sqlite.rs:51-57`，使用点 `:25`、`:39`、`:43` |
| openclaw 不变量 | 模块注释写死立场：「Directory traversal stays lexical (no symlink following, no canonicalization)：discovery must agree with the JSONL walk in `paths.rs`, which also refuses symlinks」 | `openclaw/src/sqlite.rs:14-21` |
| openclaw 测试 | `ignores_symlinked_database_paths`（`#[cfg(unix)]`）：文件软链 + 目录软链两种形态都断言「一个都不收」 | `openclaw/src/sqlite.rs:354-380` |
| opencode 探测 | `has_json_file` 同样以 `file_type()` 判，注释给出两条理由：跟随会让「探测说有、收集却拒读」自相矛盾，且软链成环会递归到栈溢出 | `opencode/src/loader.rs:206-209`、`:210-224` |

#### 14.3.2 canonicalize 派：全仓 5 处生产调用

全仓 `canonicalize` 共 9 处命中，其中 4 处落在测试代码（`zcode/src/paths.rs:171-172`、`:211` 三处调用，`antigravity/src/paths.rs:136` 命中的是测试函数名 `deduplicates_canonicalized_override_aliases`）；**生产调用恰好 5 处**：

| 位置 | 用途 | 失败回退 |
| --- | --- | --- |
| `antigravity/src/paths.rs:68` | 跨根去重 `conversations/**.db`（同库被多个根覆盖只留一份）；`seen` 存 canonical，`paths` 仍存**原路径** | `unwrap_or_else(\|_\| path.clone())` |
| `goose/src/paths.rs:28` | 三条候选（`~/.local/share/goose`、`~/Library/Application Support/goose`、`~/.local/share/Block/goose`）解析为同一文件时去重 | `unwrap_or(path)`，随后仍要 `is_file()`（`:29`） |
| `zcode/src/paths.rs:32` | 库路径解析：canonicalize **之后**再判 `is_file()` | 失败 → debug 日志 + `continue`（`:41-50`） |
| `zcode/src/paths.rs:117` | 每个根的 canonicalize（根去重） | 失败 → 丢弃该根；**仅配置根**打 debug 日志（`configured` 分支 `:120-128`），默认根静默丢弃 |
| `ccusage-adapter-all/src/loader.rs:544` | `path_key()`：来源重叠检测的键（`paths_overlap` 判相等或互为前缀） | `unwrap_or_else(\|_\| path.to_path_buf())`（`:543-545`） |

重叠检测的语义在 `loader.rs:546-553`：**相等或嵌套都算重叠**，因为会话文件是递归收集的——一个 store 根若是另一个 store（或默认 pi store）的祖先，同一批文件会以不同 dedupe 身份被吃进两次；命中即返回 `cli_error`（`:527-533`）。

#### 14.3.3 并存原因与风险

- **遍历求稳定与防循环**：发现阶段的输入是用户目录（含用户自建软链），跟随会带来三类问题——重复计数（软链与目标各扫一遍）、无限递归（成环）、以及「探测与收集不一致」（`opencode/src/loader.rs:206-209` 明确点名）。因此所有 `collect_*` 一律用 `file_type()` 词法判断，且 **JSONL 遍历与 SQLite 发现必须同立场**（`openclaw/src/sqlite.rs:18-20`）。
- **重叠检测求真实同一性**：`paths_overlap` 要回答「这两个配置项是不是在指同一堆文件」，若不做 canonicalize，`~/.pi/x` 与「指向它的软链」会被判为不重叠 → 同一份会话被两条来源读两次。所以这 5 处只出现在**去重、根解析、重叠检测**上，不进入收集循环。
- **风险 1（canonicalize 侧）**：失败回退到原路径/原行为，意味着「软链解析失败」时去重可能失效（把同一文件当两条）——静默、无告警；反之解析成功时又会把软链与目标合二为一，与「遍历拒读软链」形成接缝：**同一个软链，收集侧看不到它，重叠检测却认为两者相同**。goose 与 zcode 都用「canonicalize 后再 `is_file()`」给这条接缝兜底（`goose/src/paths.rs:28-31`、`zcode/src/paths.rs:32-40`），antigravity 与 `path_key` 没有这层二次校验。
- **风险 2（遍历侧）**：词法遍历的代价是**目录项级别的软链一律不可见**——用户若把某个会话目录做成软链（常见于「数据目录搬到大盘」的用法），该目录不会被递归。至于「根路径本身是符号链接」的情形（`fs::read_dir` 跟随根），本节未在仓库测试中找到覆盖，见 §14.6。

### 14.4 选源准入判据与已否决的源

**结论**：ccusage 的准入判据是「本地能读到可核验的用量记录」，核心底线是**具备时间戳 + 会话身份 + 模型身份 + token 计数（或可映射到 token 的成本）**，且**明确拒绝从文本长度估算 token、拒绝抓私有云服务**。准入判据是叙事层文档，`BUILT_IN_AGENT_NAMES` 是硬编码的实现白名单，两者不需要数量一致。

#### 14.4.1 准入判据（`docs/guide/source-support-qa.md:1-56` 全文提炼）

| 判据 | 原文要点 | 行号 |
| --- | --- | --- |
| 最低信息集 | 时间戳、会话身份、模型身份、token 计数或可映射为 token 的已记录成本 | `:3` |
| 逐消息/逐轮计数 | 有分桶更好（输入输出、缓存、推理） | `:11-12` |
| 可定价性 | 需要模型标识；需要时间戳做筛选与分组；需要会话/对话标识 | `:13-15` |
| 格式稳定性 | JSONL / SQLite 表 / 结构化遥测导出 | `:16` |
| **显式否决 1** | 只有本地 transcript 文本不够：看不到 tokenizer 行为、隐藏上下文、缓存输入、工具调用开销、服务端记账 | `:18` |
| **显式否决 2** | **不从文本长度估算 token**——否则日报/月报/会话/成本会「看起来精确，实则猜的」 | `:5` |
| **显式否决 3** | 不抓私有云、不依赖未公开的鉴权 API 取用户用量历史 | `:25` |
| 新增源需要的材料 | 脱敏样例、默认数据目录、字段对照（哪些字段是 input/output/cache/reasoning/model/timestamp/session）、成本是记录值还是待算 | `:49-55` |

#### 14.4.2 已否决 / 暂缓的源

| 源 | 状态 | 理由 | 证据 |
| --- | --- | --- | --- |
| Devin CLI | 否决（可重审） | 用量信息在 Devin 云端，本地数据无法给出历史 token/成本；若 Devin 提供含时间戳/会话/模型/计数的本地导出，可重开 | `source-support-qa.md:22-26` |

#### 14.4.3 「曾经不支持 → 现已支持」的案例

| 源 | 早期状况 | 现状 | 证据 |
| --- | --- | --- | --- |
| Antigravity | 无 | 独立源：读本地 `gen_metadata` SQLite，**与 Gemini CLI 分离**，映射 input / cache-read / visible-output / thinking 四个桶到标准报告形状 | `source-support-qa.md:30-34`；实现见 `antigravity/src/parser.rs:689-703`、`loader.rs:19-41` |
| Grok Build CLI | 早期只看到「不暴露可用 token 记账」的本地数据（如无逐轮 usage 的 SQLite） | 现读 `${GROK_HOME:-~/.grok}/sessions/**/updates.jsonl` 的 `sessionUpdate == "turn_completed"`（`usage` / `modelUsage`）；成本取 Grok 自己的 `costUsdTicks`，LiteLLM 估算兜底；**进行中的 turn 不计** | `source-support-qa.md:37-44`；实现见 `grok/src/parser.rs:219-230`、`:468-480` |

两个案例共同说明准入线怎么移动：**不是**放宽到「估算」，而是等到本地出现逐轮 token 字段（Grok 的 `turn_completed`、Antigravity 的 `gen_metadata` protobuf）。

#### 14.4.4 准入清单与实现清单的关系

`BUILT_IN_AGENT_NAMES` 18 项（`rust/crates/ccusage-core/src/lib.rs:63-82`：claude、codex、opencode、amp、droid、codebuff、hermes、pi、goose、openclaw、kilo、copilot、gemini、antigravity、kimi、qwen、grok、zcode）与 `rust/adapters/` 下 18 个适配器目录一一对应（`adapters/` 共 19 个目录，含零件库 `common`）。三向对齐证据：

| 方向 | 事实 | 证据 |
| --- | --- | --- |
| 常量 → 配置层 | agent 名即合法命令名（`is_agent_command`）；同时 18 名 + `"all"` 构成「命名 pi store 保留名」集合 `reserved_named_pi_store_names` | `ccusage-config/src/config.rs:395-397`、`:218-222`；一致性由测试锁定 `:1244-1254` |
| 常量 → 报告层 | `AgentLoadSpec` 逐个取 `BUILT_IN_AGENT_NAMES[i]` 作为 `agent` 字段，共 18 个 spec（`index` 0..17）；index 0/1 是 Claude/Codex 专用装配 | `ccusage-adapter-all/src/loader.rs:116-358`（抽样 `:119`、`:125`、`:131`、`:308`、`:323`、`:329`、`:345`） |
| 常量 → 顺序语义 | 数组顺序 = spec 顺序 = 报告中的 agent 顺序 | `loader.rs:116-358` 与 `lib.rs:63-82` 顺序一致 |

关系论断：`source-support-qa.md` 回答「什么样的源**可以**做」，`BUILT_IN_AGENT_NAMES` 回答「现在**确实**接了哪些」。判据里没有「必须官方支持」「必须有 API」这类门槛，唯一的硬底线是本地有可核验的 token 或成本、**禁止估算**；而实现层加一个源的成本是「新增 adapter crate + 数组项 + `AgentLoadSpec`」，聚合与渲染零改动（§5/§6 已确认的架构主张）。文档明确保留扩源入口（`source-support-qa.md:47-55`），所以「准入清单 ⊃ 实现清单」是设计意图，不是遗漏。

### 14.5 测试基建（可抄的隔离模式）

**结论**：`ccusage-test-support` 用「全局环境变量锁 + RAII 还原 + 临时目录 fixture」三件套，把「测试与真实用户数据/宿主环境」彻底隔开；e2e 测试再用 `env_clear()` 从空环境起跑。这套模式解决的是**环境变量污染、临时目录管理、跨测试串扰**三类问题，与 TokenScope 的「测试密闭性」同源，但前提不同（ccusage 无状态）。

#### 14.5.1 `ccusage-test-support` 零件

| 零件 | 语义 | 行号 |
| --- | --- | --- |
| `static ENV_LOCK: Mutex<()>` | 进程内全局互斥；`env_lock()` 用 `unwrap_or_else(\|e\| e.into_inner())` 恢复中毒锁——一个测试 panic 不会连锁毒死其余需要改环境的测试 | `rust/crates/ccusage-test-support/src/lib.rs:15-19` |
| `EnvVarGuard::set(key, value)` | 先取锁 → 记录旧值 → `unsafe set_var`；`Drop` 时还原旧值，旧值为 `None` 则 `remove_var` | `:21-47` |
| `EnvVarsGuard::set_many(iter)` | 一次持锁批量设置 `(key, Option<value>)`；`Drop` 时**逆序**还原，避免同一 key 被重复设置时还原错序 | `:49-81` |
| `Fixture` | `assert_fs::TempDir` 包装（临时目录随对象释放清空）；`root()` / `path()` / `write_file()`（自动建父目录）/ `create_dir_all()` | `:83-127` |
| `fs_fixture!` 宏 | 内联目录树字面量 → 逐文件写入，fixture 与断言同屏可读 | `:135-144`，自测 `:146-171` |
| `zcode::create_fixture(path)` | 建 `model_usage` + `session` 两表并插 3 行（`session-a` 两行、`session-b` 一行；模型 `GLM-5.2`/`GLM-5.3`；时间戳 2099-01-02 / 01-15 / 02-01），`started_at` 由 `jiff::Timestamp` 解析后落毫秒整数 | `rust/crates/ccusage-test-support/src/zcode.rs:20-81`，插行 `:84-113` |

依赖面：`grep` 在 `rust/**/Cargo.toml` 命中 26 个 manifest——19 个 `rust/adapters/*` 目录（含零件库 `common`）+ 其余 5 个 crate（`ccusage`、`ccusage-adapter-all`、`ccusage-cli-parser`、`ccusage-config`、`ccusage-core`）+ `ccusage-test-support` 自身 + workspace 根 `rust/Cargo.toml`，即**每家适配器都按该模式写单元测试**（未逐个区分 `[dependencies]` / `[dev-dependencies]`，见 §14.6）。

#### 14.5.2 端到端测试形态

| 文件 | 行数（`read` 实测，含空行） | 体裁 | 套路 |
| --- | --- | --- | --- |
| `rust/crates/ccusage/tests/claude_cli.rs` | 741 | 行为断言为主 + 少量快照 | `Fixture` 写字面 JSONL（含 sidechain、无 requestId、跨日界等构造），`Command::new(env!("CARGO_BIN_EXE_ccusage"))` + `env_clear()` + 注入 `HOME`/`USERPROFILE`/`XDG_CONFIG_HOME`/`CLAUDE_CONFIG_DIR`，断言 JSON 的 `totalCost`/`totalTokens`/`entries.len()`（`:16-42`）；用 `set_times` 伪造 mtime 测 `--since` 剪枝（`:44-52`）；`claude_report_totals` 把同一 fixture 跑遍全部报告命令做交叉一致性断言（`:712-714`） |
| `rust/crates/ccusage/tests/codex_cli.rs` | 137 | 行为断言 | 统一 `run_cli_output` 注入 `CODEX_HOME` + 三个空 home 变量 + `env_remove("CLAUDE_CONFIG_DIR")` + `LOG_LEVEL=0`/`NO_COLOR`/`COLUMNS=240`（`:123-137`）；断言 session id 的多种形态选择（`:16-35`） |
| `rust/crates/ccusage/tests/copilot_cli.rs` | 122 | 快照（`insta`） | 对 daily/monthly/session 的 JSON 与表格分别 `insta::assert_snapshot!`（`:12-23`、`:26-45`） |
| `rust/crates/ccusage/tests/zcode_cli.rs` | 46 | 快照（`insta`） | 数据源换成 SQLite fixture（`create_fixture`），并固定 `--since/--until/--mode/--offline/--timezone UTC`，对三种报告的 stdout+stderr 一起快照（`:6-44`） |

#### 14.5.3 这套模式解决什么问题

- **环境变量污染**：所有适配器都从环境（`HOME`、`USERPROFILE`、`XDG_*`、各 `*_DATA_DIR`）解析数据根，`cargo test` 默认多线程同进程，直接 `set_var` 会互相踩；`ENV_LOCK` 把「改环境」串行化，RAII 保证异常路径也还原（`lib.rs:21-47`）。
- **临时目录**：`Fixture`/`fs_fixture!` 让 fixture 路径全部落在 `TempDir` 内，配合显式注入的 `HOME`，**默认根不可达真实用户数据**——这是 TokenScope 那次「测试把真实缓存 purge 成 fixture」事故的同类问题，ccusage 用「从空环境起跑」而非「注入 cache_dir 参数」来堵。
- **跨测试串扰 / 顺序依赖**：e2e 一律 `env_clear()`（`claude_cli.rs:17`、`zcode_cli.rs:11`）或显式清掉可能干扰的变量（`codex_cli.rs:134`），使开发者本机遗留变量无法造成假绿；并行读的正确性则用「单线程 vs 多线程结果一致」的对拍测试锁定（`rust/adapters/common/src/lib.rs:134-`）。

#### 14.5.4 与「测试密闭性」的关系

- 可借鉴且与 TokenScope 直接同向的：`ENV_LOCK` + RAII 还原、`fs_fixture!` 式内联 fixture、SQLite fixture 生成器（`create_fixture`）、`insta` 快照 e2e、以及「同一 fixture 跑遍全部报告命令」的交叉一致性断言。
- 不能直接照搬的：ccusage 是**无状态 CLI**（每次运行重新解析，不写用户缓存），因此不存在「测试注错缓存目录」的隐患，也就**没有** TokenScope 那种 `cache_dir` / `pricing_index` 注入参数。TokenScope 的密闭性要求（任何测试必须注入临时目录、禁止落回 `~/.tokenscope`）不能靠「env_clear + 空 HOME」单独满足——它保护的是**写入路径**，ccusage 模式只覆盖**读取路径**。
- 一个共同点值得强调：两家都避免把真实会话日志入库（ccusage 全程合成 fixture，`claude_cli.rs:6-14` 是手写字符串数组），与 TokenScope 的「真实 agent 日志永不入库」一致。

### 14.6 未验证 / 存疑

1. **`LinePrefilter` 与「可接受行」的等价性未逐一验证**。零件注释要求标记必须出现在适配器会接受的每一行（`rust/adapters/common/src/jsonl.rs:23-27`），本节只核实了各家是否传预筛与标记来源（`grok/src/parser.rs:219`、`qwen/src/parser.rs:111`、`kimi/src/parser.rs:127`、`openclaw/src/parser.rs:192`、`pi/src/parser.rs:319`、`copilot/src/parser.rs:131`、`gemini/src/parser.rs:176` 传 `None`），**未逐个证明标记集合无漏筛**（漏筛的表现是静默少算，无告警）。
2. **`antigravity` 的 protobuf 字段号与真实 schema 的对应关系无法在仓库内验证**（S5 分片「未验证」末条已声明的同一存疑），本节只补充其行级后果：字段缺失时有 `unwrap_or(0)` 兜底（`antigravity/src/parser.rs:689-703`），但结构性错误（字段号 0、非法 wire type、varint 溢出、截断）一律 `Err` → 整库失败。
3. **大小写与正斜杠形态的 `is_windows_users_path` 行为未构造样例**：`C:/Users/...` 与 `c:\users\...` 按代码均落步 4 分支（`project_names.rs:32-62`），实际展示差异未验证（纯静态判读）。
4. **步 5 的 hex 压缩分支（`project_names.rs:63-70`）的设计意图无注释依据**，本节只陈述行为（段数 ≥5 且全为 hex/`-`/`.` → 取最后两段），未找到来源说明，也未与 TS 版实现对照。
5. **§14.1 的别名解析是否曾在历史版本支持 JSON/路径形态未查证**：现状是「注释说 JSON object 或路径、实现只解析 `k=v`」（`config_schema.rs:433` vs `project_names.rs:3-17`）。
6. **符号链接：根路径本身是软链时的行为未验证**。本节确认的是「目录项级别的软链不跟随」（`common/src/lib.rs:24-32`）与「openclaw 显式拒绝」（`openclaw/src/paths.rs:56-58`）；至于 `fs::read_dir` 对「根本身是软链」是否跟随，本节按标准库语义推断为「跟随」，**仓库内无对应测试**（唯一符号链接测试是 `openclaw/src/sqlite.rs:354-380`，且 `#[cfg(unix)]`，Windows 上不运行）。
7. **`is_project_path_segment` 之外的分支**：`--project` 过滤若传入含分隔符或 `..` 的值会落回全量扫描（`claude/src/paths.rs:65-79`、`:248-254`），该路径下的 `entry.project == filter` 精确比较必然全不命中——这一「用户输错即得空报告」的行为**未实测**。
8. **26 个 manifest 命中数的构成未逐项区分**：`ccusage-test-support` 的引用落在 19 个 `rust/adapters/*` 目录 + 5 个 crate + 自身 + workspace 根，但未逐个打开确认是否都写在 `[dev-dependencies]`（`grep` 命中计数，非逐文件核对）。
9. **§14.2 中标注「grep 定位 + 邻段」的 8 家**（opencode、copilot、pi、qwen、kimi、openclaw、hermes、goose）未逐行读全部分支：其丢字段判定「哪些字段缺失会产生语义影响（而非仅取值 0）」未逐条枚举；本节只确认了它们的档位与证据行号。
10. **运行时行为**：第 8 轮已运行验证 CLI 与聚合路径（见 §13.5）；但「坏行被静默丢弃后报告数值如何变化」仍缺少用合成坏行样本跑出来的运行验证。
