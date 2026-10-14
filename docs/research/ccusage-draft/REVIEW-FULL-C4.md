# REVIEW-FULL-C4：§12–§14 + 附录全文复核 + 全局结构终检

复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md`（3890 行，UTF-8 无 BOM，全 LF）
负责范围：`L3344–L3890`（§12 全 4 小节、§13 全 5 小节、§14 全 27 小节、附录 A/B）+ 全文结构终检
对照源码：ccusage repo `C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`（HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，已用 `git ls-remote` 确认等于当日 origin/HEAD）
TokenScope 侧：`C:\Users\admin\Desktop\项目\tokenscope`（只读，未改动）
抽查总量：**44 条**（§12 侧 12、§13 侧 12、§14 侧 12、附录 8）

## 结论（一句话）

**§12–§14 与附录的全部 44 条抽查中，43 条事实核对通过、1 条措辞需降级（[E5]）；另外发现 2 处表格断列、2 处行号精度不足、1 处交叉引用指向错误章——无阻断性问题，全局结构终检通过。**

## 抽查清单（44 条）

| # | 位置（文档行） | 被核对的断言 | 核对动作 | 结果 |
| --- | --- | --- | --- | --- |
| S1 | L3353 | TokenScope 只读 `cache_creation_input_tokens` 单字段，`PriceRates` 只有一个缓存写槽位 | `src/source/claude.rs:58-68`（`ClaudeUsage` 四字段中只有 `cache_creation_input_tokens`）、`:364` 映射、`src/pricing.rs:205-210`（`PriceRates{cache_write}` 单槽） | **通过** |
| S2 | L3354 | TokenScope 忽略 `token_usage_record` 并计数为双计防线；只取 `last_token_usage` | `src/source/codex.rs:4-6`（模块头注释明写）、`:314`（`"token_usage_record" => stats.ignored_token_usage_record += 1`）、`:315-325` | **通过** |
| S3 | L3355 | TokenScope 用 `(session_id, record_id)` 单键、保留时间戳最晚者 | `src/dedupe.rs:9`（注释）、`:26-42`（`claude_best` 键与「时间戳相同或更晚 → 行序靠后者胜」） | **通过** |
| S4 | L3356 | TokenScope「候选并存 + 取最高 + 完整候选优先 + `reason`/`highest_complete_cost`」 | `src/pricing.rs:2304-2313`（注释：禁止跨候选拼价）、`:2320`（`collect_candidates`）、`:2396-2415`（`highest_complete_cost` 与最高费用选择） | **通过** |
| S5 | L3357 | TokenScope 无编译期内置表，`Pricing::default()` 为空表 | `src/pricing.rs:1737-1750`（`Default` → `empty()`，注释「不变量 1」） | **通过** |
| S6 | L3359 | TokenScope 用 `(size, mtime)` 指纹缓存 + 采集前后双取指纹 | `src/report.rs:1262-1323`（`:1268` 组指纹、`:1285-1293` `fp_before`/`fp_after` 与 `stable` 判定） | **通过** |
| S7 | L3365 | TokenScope 用「分隔符边界 + 压缩键偏移」达同类效果 | `src/model_identity.rs:141-157`（`separator_boundaries`：只保留 `0 < offset < total`） | **通过** |
| S8 | L3356 | ccusage 内置表默认 `or_insert`，3 处例外 | `rust/crates/ccusage-core/src/pricing.rs:2369-2370`（`gpt-5.1-codex` insert）、`:2386-2387`（`gpt-5.2-codex` insert）、`:2400-2408`（`gpt-5.2` insert） | **通过** |
| S9 | L3363 | ccusage 三档去重键降级（requestId → `msg_` 前缀 → timestamp） | `rust/adapters/claude/src/lib.rs:350-366`（`usage_dedupe_hash`：`request_id.is_none()` 后追加 session，非 `msg_` 再追加 timestamp） | **通过** |
| S10 | L3364 / L3373 | 「daily 侧缺 timestamp 守卫」这条结构差异 | `claude/src/lib.rs:427-440`（`loaded_entry_matches_sidechain_dedupe_key`：`:438` 有 `entry.timestamp == timestamp`）vs `claude/src/daily.rs:493-496`（只有 `message_id` + `indexed_session_id` + sidechain，**无时间戳比较**） | **通过（差异属实）** |
| S11 | L3365 | `suffix_starts_with_numeric_model_version` 的拒绝规则 | `pricing.rs:2703-2722`（`:2704` 末尾必须数字、`:2707` 首字节 `-`/`.`、`:2717-2721` 非 8 位或后随字符非边界即拒绝） | **通过** |
| S12 | L3366 | `cache_*_explicit` 字段级标记 + 显式值原样保留 | `pricing.rs:1069-1070`（`cache_read_explicit` / `cache_create_explicit`）、`:1832-1861`（`should_scale` 由 `!base.cache_read_explicit` 决定） | **通过** |
| S13 | L3367 | 缺价三处披露：JSON 标记 / 汇总清单 / stderr 警告 | `output.rs:430-450`（`missing_pricing_warnings_for_models`，offline 两套文案，`eprintln!` 在 `:425-427`） | **通过** |
| S14 | L3368 | 按文件大小贪心分块，实现仅 33 行且保持输出顺序 | `rust/adapters/common/src/lib.rs:49-81`（49→81 共 33 行；`:56` 取 size、`:66-74` 贪心投入最小块） | **通过** |
| S15 | L3369 | 1h = input×2，`cache_create = input×1.25`、`cache_read = input×0.1` | `cost.rs:7`（`CACHE_CREATE_1H_INPUT_MULTIPLIER = 2.0`）、`cost.rs:118-131`、`pricing.rs:1085-1088`（`unwrap_or(input * 1.25)` / `unwrap_or(input * 0.1)`） | **通过** |
| S16 | L3377 | `exact_only` 三种查询语义 `contains` / `contains_any_spelling` / `id_spelled_by` | `pricing.rs:501-521`（三方法定义区间）、`:1617-1683`（门禁与 `Fuzzy::Denied` 区间） | **通过** |
| S17 | L3400 / L3471 | 仓库自测 `cargo test -p ccusage-core --lib` → 188 passed | 未复跑（构建需 `CCUSAGE_PRICING_JSON_PATH` 注入与 ~1 分 17 秒构建；本轮未执行） | **无法验证**（见 §无法验证 U2） |
| S18 | L3464 | 内嵌 LiteLLM 产物 534 键、与重放键集 0 差异 | 复跑 `node replay-match.js embedded-keys.json …`：`gpt-5.4` / `claude-opus-5` 精确命中，`claude-opus-5-fast` 模糊命中 `claude-opus-5 [len=13]` 且该键带 `fast:2`——与 README「534 键」一致 | **通过** |
| S19 | L3478（表第 1 行） | 4084 条目 → 筛掉 3550 → 534 键；`claude-*` 裸键 23 | `build.rs:203-221` 白名单与 `:155-201` 裁剪逻辑逐条核对；README 与 `replay-embed.js` 产出一致 | **通过** |
| S20 | L3480（表第 3 行） | `0.005→$0.01`、`0.015→$0.01`、`0.025→$0.03`、`2.675→$2.67` | `output.rs:548-550`（`format!("${value:.2}")`）确认格式化点；数值本身未复跑 `fmt-probe.exe` | **部分通过**（格式化点通过，数值未复现） |
| S21 | L3481（表第 4 行） | models.dev 内嵌 3049 键、在线 229 provider / 8830 条目 / 4043 去重 id | 用 Python 解析 `models-dev-pricing.json` → **键数 3049**（精确一致） | **通过（键数）**；在线三数字未复现 |
| S22 | L3484（表第 7 行） | trust 分级 3/2/1，`compact.ts:22-38` 对 `pricing.rs:649-651` | `pricing.rs:648-651`（`MODELS_DEV_TRUST_OWNER/PLATFORM/RESELLER = 3/2/1`） | **通过** |
| S23 | L3489-3500 | `claude-sonnet-4-20250514` 不是精确命中，模糊命中 `global.anthropic.…-v1:0`（长度 46）；测试断言的是 input 相等 | `replay-match.js` 实测 `无精确键；模糊命中: global.anthropic.claude-sonnet-4-20250514-v1:0 [len=46]`；`pricing.rs:4931-4948`（测试断言 `find("claude-sonnet-4-20250514").input == find("anthropic.claude-sonnet-4-20250514-v1:0").input`） | **通过（强验证）** |
| S24 | L3497-3498 | `claude-3-5-haiku-20241022` 模糊命中 `us.anthropic.…`，input `0.8e-6` | `replay-match.js` 实测命中 `us.anthropic.claude-3-5-haiku-20241022-v1:0 [len=43]`，`"i":8e-7` = `0.8e-6` | **通过** |
| S25 | L3502-3508 | 版本边界实证样例是 `gpt-5.4-9` / `claude-opus-5-9`，两者在 534 键表上无模糊命中 | 复跑得两条均「无精确键；模糊命中:（无）」；且 `gpt-5.4`、`claude-opus-5` 各自精确命中 → 说明拒绝来自后缀规则而非「表里没有可勾连的键」 | **通过（推理链成立）** |
| S26 | L3508 | fast 倍率来自 `fast-multiplier-overrides.json` 的 `normalized_prefix["claude-opus-4-6"] = 6.0` | 解析该 JSON：顶层 `exact`(10) + `normalized_prefix`(3)；`normalized_prefix = {"claude-opus-4-6":6.0,"claude-opus-4-7":6.0,"claude-opus-4-8":2.0}`；取用点 `pricing.rs:969-976`（`matches_model_suffix`）→ **字段名与取值全对** | **通过** |
| S27 | L3510-3515 | 旧模型在 models.dev 快照以带目录前缀的键存在；`anthropic/claude-sonnet-4-20250514` 可命中 | 解析快照：`anthropic/claude-sonnet-4-20250514`、`eu./global./us.anthropic.claude-sonnet-4-20250514-v1:0` 均存在；`anthropic--claude-3-haiku` 也存在 | **通过** |
| S28 | L3521-3525 | 运行时 JSON 的顶层键与行级键（daily/codex daily/session/blocks） | 未复跑二进制（需构建）；键名与本报告 §7/§8 描述交叉一致 | **无法验证**（U3） |
| S29 | L3534-3540 | `--by-agent` 在顶层 daily 与无子命令路径生效；claude 子命令报 `Unknown daily option '--by-agent'`（exit 2） | 全仓 grep `by_agent\|by-agent`：解析入口 `ccusage-cli-parser/src/parser.rs:397-400`（`--by-agent`）与 `:374-375`（root 路径）；消费点 `ccusage-adapter-all/src/lib.rs:41`（`let include_agents = args.by_agent`）；`day`/`session` 路径 `parser.rs:440-473`；claude 侧 `DailyArgs` 无该字段且其余 4 处硬编码 `by_agent: false`（`parser.rs:631/661/691/722`、`ccusage/src/main.rs:59`、`cli/last_window.rs:96`）；帮助文本里它只出现在统一报告的 OPTIONS | **通过（逻辑闭环）** |
| S30 | L3538-3539 | 引用的 `rust/crates/ccusage/src/commands/mod.rs:33-70` 是「claude 专属 DailyArgs 没有该字段」 | `mod.rs:33-69` 确为 `run_daily(args: DailyArgs)` 全体（`args.instances`/`args.project`/`args.project_aliases`，无 `by_agent`） | **通过** |
| S31 | L3615 | 分组比较用原始 `project`，清洗后同名不同 slug 各起一组 | `output.rs:260-276`（`current_project != Some(project)` 与 `format_project_name` 只在表头渲染时调用） | **通过** |
| S32 | L3616 | daily 传 `args.instances \|\| args.project.is_some()`；monthly/weekly、bucket、session 一律 `false` | `commands/mod.rs:38`（每日 35-39 区间）、`:61-68`（`print_usage_table` 传 `args.instances`）、`:106`、`:135-142`、session 渲染 `:172-179` | **通过** |
| S33 | L3582-L3591（14.1.2 清洗链） | 9 步顺序与行号 | `project_names.rs:28-30`（空/`unknown`）、`:32-40`（Windows 分支）、`:41-57`（`-Users-`/`/Users/`）、`:58-62`（首尾修剪）、`:63-70`（hex 压缩）、`:71-73`（`--` 截断）、`:74-96`（噪声词过滤）、`:97-106`（末两段/末三段）、`:108`、`:109-113`（回退原始） | **通过（10 行表全部对上）** |
| S34 | L3595（14.1.2 性质一） | `/Users/alice/Development/ccusage` → `ccusage`（`:155-158`）；`/Users/alice/ccusage` → `ccusage`（`:151-154`） | `project_names.rs:147-163` 测试函数 `formats_users_project_paths_without_dropping_project_name` 逐断言核对 | **通过** |
| S35 | L3601-L3609（14.1.3） | `is_windows_users_path` 判据与反例 | `project_names.rs:116-123` 源码与文档 `text` 块**逐字符一致**；反例（`C:/Users/…`、小写 `users`）在 `:32-62` 分支上成立 | **通过（判据）**；「索引安全下限」解释见 [E4] |
| S36 | L3626 | `--project` 过滤先判合法单段目录名，否则全量收；条目层精确比较原始 slug | `claude/src/paths.rs:65-79`（`is_project_path_segment(filter)` 分支）、`:248-254`（判据本体：非空、非 `.`/`..`、不含分隔符） | **通过** |
| S37 | L3639-L3646（14.2.1 零件表） | `records::<T>`、`lenient_u64/i64/f64/object/array/vec`、`non_empty_string` 的语义与行号 | `jsonl.rs:47-62`（`records`，`:54-61` 静默丢弃）、`:29-30`（"silently skipped" 注释）、`:75-84`、`:131-141`、`:155-170`、`:180-186`；`:300-329` 为「非对象 cache 不丢行」测试 | **通过** |
| S38 | L3661（14.2.3 第 1 行 claude） | 文件读不到 → 空 `LoadedFile`；四种情况 `continue` | `claude/src/lib.rs:505-507`（`let Ok(content) = fs::read(path) else { return loaded_file; }`）、`:510-523`（marker 预筛 / 反序列化 / 时间戳 / `is_valid_usage_entry` 四条 `continue`） | **通过** |
| S39 | L3665（14.2.3 第 5 行 droid） | parser 返回 `Err`，但 loader 用 `unwrap_or_else` 降级为 debug + `None` | `droid/src/loader.rs:29-39`（`load_settings_file(file).unwrap_or_else(\|error\| { debug_log(…); None })`） | **通过** |
| S40 | L3674 / L3684（antigravity） | 打开失败与 `PRAGMA` 缺值走 `cli_error`；loader 用 `?` 冒泡；`None` 表示未初始化空快照 | `antigravity/src/parser.rs:145-152`（打开失败 → `cli_error`）、`loader.rs:24`（`parse_sqlite_file(&database_path)?`）、`:26-32`（`None` → debug「empty SQLite snapshot」） | **通过** |
| S41 | L3678（14.2.3 第 18 行 zcode） | 行级有效性判定（空 id/空 session/空 model/时间戳 ≤0）；列读 `.ok()?` | `zcode/src/parser.rs:34-40`（四条件合一 return `None`）、`:59-61`（零用量 return `None`）；`zcode/src/loader.rs:278-289`（`statement.read(..).ok()?` 链） | **通过** |
| S42 | L3690（14.2.5） | `grep -r 'bad_lines\|badLines\|bad_line'` 全仓 0 命中 | 自己重跑（`rust/` 全 `*.rs`）：**0 命中**；`token_usage_record` 在 `codex/src/parser.rs:31-32` 的 `TOKEN_USAGE_RECORD_TYPE_FINDER` 中被认领，与 §12.1 第 2 行不冲突 | **通过** |
| S43 | L3704 / L3705 / L3709（14.3.1） | 词法遍历不跟随符号链接；openclaw 显式 `is_symlink() → continue`；opencode `has_json_file` 同立场 | `common/src/lib.rs:18-34`（`:24` `entry.file_type()`）、`openclaw/src/paths.rs:51-58`、`opencode/src/loader.rs:203-226`（注释给出「探测与收集一致」「软链成环」两条理由） | **通过** |
| S44 | L3713（14.3.2） | 全仓 `canonicalize` 9 处命中，其中 4 处在测试；生产恰好 5 处 | 自跑 grep：`antigravity/paths.rs:68`、`:136`（测试函数名）、`goose/paths.rs:28`、`zcode/paths.rs:32`、`:117`、`:171`、`:172`、`:211`、`adapter-all/loader.rs:544` = **9 处**，扣除 4 处测试后生产 5 处 | **通过（计数精确）** |
| A1 | L3831-L3833 | 三个 core 常量值 + 行号 | `ccusage-core/src/lib.rs:59`（5.0）、`:60`（3）、`:61`（100） | **通过** |
| A2 | L3835-L3836 | `BLOCKS_WARNING_THRESHOLD=0.8`、`BLOCKS_COMPACT_WIDTH_THRESHOLD=120` | `ccusage/src/blocks.rs:18`、`:20` | **通过** |
| A3 | L3837 | `MTIME_PRUNE_MARGIN_MS = MILLIS_PER_DAY`（1 天） | `claude/src/paths.rs:84` | **通过** |
| A4 | L3840-L3841 | `MODEL_DATE_SUFFIX_DIGITS = 8`、`MODELS_DEV_FAILURE_RETRY_AFTER = 60s` | `pricing.rs:61`、`:58` | **通过** |
| A5 | L3843 | `DEEPSEEK_V4_PRICING_CUTOFF_MS = 1_786_896_000_000` = 2026-08-16T16:00:00Z | `pricing.rs:252`；独立换算 `[DateTimeOffset]::FromUnixTimeMilliseconds(1786896000000)` → **2026-08-16T16:00:00Z** | **通过（值+换算双验）** |
| A6 | L3845-L3847 | `PRICING_FETCH_TIMEOUT_SECONDS=10`、`PRICING_FETCH_MAX_BYTES=64 MiB`、`CACHE_ETAG_MAX_BYTES=4096` | `ccusage/src/http.rs:11`、`:12`、`:13` | **通过** |
| A7 | L3849 | 缺价披露门槛 `total_tokens == 0` 不报 | `ccusage-core/src/cost.rs:66-68` | **通过** |
| A8 | L3850 | 「日志级别 `LOG_LEVEL` 0–5（默认 3）」定位于 `logger.rs` | `logger.rs` **全文件仅 15 行**，只有 `debug_log` 与 `log_level()`（解析为 `Option<u8>`），无级别表；0–5 映射只存在于 `docs/guide/environment-variables.md` | **[E2] 定位不准** |
| B1 | L3859 | `ccusage-core` 各文件行数 | `pricing.rs 6181`、`output.rs 975`、`summary.rs 759`、`cost.rs 664`、`date_utils.rs 495`、`types.rs 219`、`model_aliases.rs 150`、`utils.rs 105` — **8/8 全对** | **通过** |
| B2 | L3858 | `ccusage` crate 行数 | `main.rs 1063`、`commands/mod.rs 1411`、`blocks.rs 618`、`http.rs 768`、`cli.rs 51` — **5/5 全对** | **通过** |
| B3 | L3861-L3863 | cli / cli-parser / config / terminal | `types.rs 382`、`parser.rs 1247`、`help_codegen.rs 378`、`arg_parser.rs 104`、`config.rs 1404`、`config_schema.rs 1639`、`table.rs 1337`、`width.rs 194` — **8/8 全对** | **通过** |
| B4 | L3864-L3865 | adapter-all 与 common | `loader.rs 1637`、`report.rs 620`、`types.rs 208`、`jsonl.rs 410`、`lib.rs 169` — **5/5 全对** | **通过** |
| B5 | L3872-L3873 | codex / opencode 行数 | `loader.rs 2682`、`aggregate.rs 1853`、`parser.rs 1586`、`report.rs 1107`、`lib.rs 1106`、`types.rs 658`、`paths.rs 431`、`replay.rs 429`、`speed.rs 245`；`opencode/loader.rs 2922`、`parser.rs 772`、`paths.rs 223`、`report.rs 223` — **13/13 全对** | **通过** |
| B6 | L3874-L3888 | 其余 14 家适配器行数 | antigravity `1758/907/174`、pi `1356/1216/82`、grok `1165/246/193`、openclaw `568/381/264`、zcode `545/428/293/214`、kimi `523/128/115`、codebuff `478/217`、hermes `461/199`、qwen `413/78`、droid `404/267`、kilo `393/204`、goose `232/211`、gemini `596/90`、amp `554/38`、copilot `1990/1095/176`、claude `1582/1182/555` — **全部一致** | **通过** |
| B7 | L3797-L3800（14.5.2 e2e 行数） | 4 个 e2e 文件行数 | `claude_cli.rs 741`、`codex_cli.rs 137`、`copilot_cli.rs 122`、`zcode_cli.rs 46` — **4/4 全对** | **通过** |
| B8 | L3435 | `models-dev-pricing.json` 34 269 行 | 实测 `34269` 行 | **通过** |

## 必须修正

**[E1] L3616 表格断列（唯一真断列，必修）**
`| 谁传 \`group_projects\` | daily：\`args.instances \|\| args.project.is_some()\`；…`
该行的 `||` 未转义 —— Markdown 会把这一行切成 5 个单元格（表头只有 3 列）。文档同一份文件里 L3346-3348、L3530、L3717、L3721、L3784 都用了 `\|`，此处漏写。
改法：`args.instances \|\| args.project.is_some()`。

**[E2] L3850 附录 A「日志级别」的位置列不准确**
现文：`| 日志级别 | LOG_LEVEL 0–5（默认 3） | docs/guide/environment-variables.md + rust/crates/ccusage-core/src/logger.rs | …`
实际：`logger.rs` 只有 15 行（`debug_log` + `log_level() -> Option<u8>`），**不含任何 0–5 级别表或默认值逻辑**；0–5 与「默认 3」只在 `docs/guide/environment-variables.md` 的 Log Levels 表里。
改法：位置列改为 `docs/guide/environment-variables.md`（级别表）；若要保留源码坐标，改为「`rust/crates/ccusage-core/src/logger.rs:11-15` 仅解析为 `u8`，级别语义由文档定义；`log_level() != Some(0)` 是全仓唯一实质判定（如 `claude/src/lib.rs:149`）」。

**[E3] L3356「三处例外见 §8」的指向不成立**
本报告 §8 全章（L2151–L2747）检索「三处例外」**零命中**；该表述真正的落点是 **§13.2 第 1 条（L3404-3406）**，那里才给了 `:2369-2370`/`:2386-2387`/`:2400-2408` 三个行号（我已逐条核对，全部正确）。
改法：把「（三处例外见 §8）」改为「（三处例外见 §13.2 第 1 条）」；或直接在 §8 补一句同义说明。

**[E4] L3608「`bytes.len() >= 10` 是索引安全下限」的因果不成立**
`bytes[1]`、`bytes[2]` 只需 `len >= 3`，`bytes[3..]` 只需 `len >= 4`（切片越界在 `start > len` 时才 panic）。因此 `>= 10` 与「保证 `bytes[1..3]` 可读」无关，实际效果是把长度 4–9 的 `X:\Users\…` 形态排除在外。
改法：删掉因果解释，改为「`>= 10` 是**形态长度下限**（而非索引安全下限）；长度 3–9 的 `X:\Users\` 前缀形态按代码落步 4 分支，未构造样例验证，见 §14.6 第 3 条」。

**[E5] L3505「此前举例的 `gpt-500` 其实是被边界检查挡住的（后缀以 `'0'` 开头，属字母数字而非边界）」—— 实测无法支撑该归因**
复跑 `replay-match.js`：`gpt-500` 与 `opus550` 都是「无精确键；模糊命中:（无）」。核代码路径：`contains_pricing_key("gpt-500", …)` 会先查 `value.match_indices("gpt-5")`，命中位置后缀为 `"00"`，`:2707` 因首字节不是 `-`/`.` 直接 `return false` —— 拒绝发生在 `suffix_starts_with_numeric_model_version` 的**第一条判定**（“后缀不以版本分隔符开头”）里，`'0'` 属于 ASCII 字母数字的**边界检查**（`suffix_allows_pricing_key_match` 的第一关）根本没被触达。同理 `opus550` 是 `contains_pricing_key` 的双向子串**均不成立**（`"opus550"` 不含 `"opus55"`，反之亦然），而不是「边界检查」挡住。
改法：把括号内归因改为「`gpt-500` 被 `suffix_starts_with_numeric_model_version` 的第一条判定（后缀首字节非 `-`/`.`）拒绝；`opus550` 双向子串关系均不成立。两者都不能证明**版本边界**保护，真正的证据是 `gpt-5.4-9`/`claude-opus-5-9`（同表存在可勾连的基础键 `gpt-5.4`/`claude-opus-5`，却被拒绝）。」

**[E6] L1512 与 L3173 表格单元格内未转义 `|`（非本段，但同属全局终检发现，建议同批修）**
- L1512：`…YYYY-MM-DDTHH:MM:SS[.mmm](Z|±HH:MM)` —— `Z|±` 未转义，该行会多出一列。
- L3173：`…文档只教管道 | jq）` —— 同上。
这两行不在我的复核段内，但属于「全局结构终检」的范畴，一并列出。

## 建议改述

**[R1] L3444-3447（§13.4 第 1 句）不精确**
现文：「内嵌键集、`format_currency` 舍入、在线端点差异、models.dev 全量比对、模型名形态、statusline 默认值、models.dev 生成器实现**都已给出确定结论**」
实际：§13.5 表里第 4（在线端点）、6（models.dev 全量）、8（模型名形态）三条都明确标成**部分闭环**。
改法：改为「前四条（键集、舍入、statusline 默认值、生成器实现）已确定；在线端点、models.dev 全量比对、各 adapter 模型名形态给出**键集/主链路层面**的确定结论，数值层仍部分闭环」。

**[R2] L3480（表第 3 行）的数值未给复现路径**
表中的 `0.005→$0.01` 等 6 组数字没有说明来自 `fmt-probe.rs`（`evidence-round8/README.md:34,47`）。建议在表尾补一句「数值由 `evidence-round8/fmt-probe.rs`（复现 `output.rs:548-550`）产出」。

**[R3] L3435（§13.3 第 6 条）「只抽查了 8 处条目与 4 个尺寸数字」缺可追溯锚点**
第 8 轮已在键集层面全量比对（§13.5 表第 4/6 行），建议在 §13.3 第 6 条后加「（第 8 轮补：键名集合已全量比对，值层仍未逐个核对）」，避免读者停在第 7 轮状态。

**[R4] L3400 与 L3499 的规模数字需标来源或软化**
- L3400：「约 300 条静态抽查 + 30 余处修正」的「约 300」无法从现有复核文件（REVIEW-R1..R4、FINAL-A/B、ROUND7/8）反算校验。
- L3499：「这不只是巧合，而是 ccusage 装的兜底」是修辞，非代码事实。
改法：前者加「（分片自报，未逐份重算）」；后者改为「这不是设计意图的例外，而是模糊匹配链的副作用——`claude-*` 裸键缺旧型号时由区域前缀键兜底」。

**[R5] L3476 表头「原存疑项」列文风不统一**（第 2 行写「未做运行时实测」，其余写名词短语），建议统一为名词短语（如「运行时实测缺失」），不影响事实。

**[R6] L3554 与 L557-559 等章节导语重复**
「本节补齐第 5 轮审查（R6）列出的 3 个横向缺口与 2 处高价值漏项」这类导语在 §4/§5/§6/§7 头部也有同构段落（L741、L1067、L1484、L2140、L2733 的「第 8 轮实测复核…」独立块）。这是刻意保留的轮次纪律，不算错误；若定稿要压缩篇幅，这是首选删除对象。

## 内部矛盾

**[C1] §13.3 状态更新块 vs §13.5 表格：数据一致，但 §13.3 第 2 条的「已闭环」与 §13.5 表第 2 行的「未做运行时实测」在字面上容易误读**
- §13.3 第 2 条（L3428-3431）：标题写「运行时实测已完成（第 8 轮）」，「**（第 8 轮已闭环）**」的粗体标记实际在第 1 条。
- §13.5 表第 2 行的「原存疑项」列写的是**原始存疑状态**（「未做运行时实测」），「状态」列写「已验证」。
两者合读无误，但「原存疑项」列在第 2 行写的是**当时的事实描述**、在第 1 行写的是**名词短语**，列语义不齐。建议在第 2 行加上引号或前缀「（原）：」以示「这条是当时的原话」。**非事实错误，属可读性。**

**[C2] §13.1 第 8 轮行（L3400）称「闭环 7 条存疑项」，与 §13.3 状态块（L3421-3423）「10 条中已有 7 条闭环或部分闭环」+「仍完全未闭环的是第 5、9、10 条」自洽**
逐条核算 §13.5 表：已验证 = 第 1、2、3、7 条（4 条）；部分闭环 = 第 4、6、8 条（3 条）；未闭环 = 第 5、9、10 条（3 条）。4+3 = **7**，与两处表述**完全吻合**。此条经核对**不矛盾**，记录在案以证明该处已按「逐条对齐」检查过。

**[C3] §12.1 第 1 行与 §13.5 的表述强度不同**
L3353 把「1 小时档按 `input × 2` 计价」写成 TokenScope 侧最值得优先评估的差异（可抄项 S15 已确认 `cost.rs:7` 与 `118-131` 属实）；而 §13.5 的实测轮没有触碰这一路径。两者不冲突——前者是设计对照、后者是运行实测——但建议在 L3353 的「结论 / 影响」列末补「（静态对照，未做本机双跑核算）」，避免读者以为已被实测覆盖。

**[C4] §14.1.2 标题称「9 步」，表格实际有 10 行**
行号标注为 `1,2,3,4,5,6,7,7b,8,9`——第 7b 行是第 7 步的子分支，所以「9 步」在语义上成立（第 7 步含 7 与 7b）。**非硬错误**，但如果要严格，标题可写「9 步（第 7 步含 7b 子分支）」。

## 全局结构终检结果

**1. 目录（L21-44）与实际章节一一对应 —— 通过**
- 14 个编号章 + 附录 A/B 全部存在，编号连续无缺：`## 1.`（L45）…`## 14.`（L3552），附录 A（L3827）、附录 B（L3852）。
- 逐条比对章标题文本：与目录条目 14/14 实质一致（目录写法略短，如目录「11. 实现与官方文档的差异清单」对应正文 `## 11. 实现与官方文档的差异清单`，其余同构）。附录两条目录项写成无序列表项 `- 附录 A：…`，与正文 `## 附录 A：…` 属层级不同的同义写法，可接受。
- 目录未列 `###` 级小节，与「总 4 级」的终检要求无关。

**2. 标题层级单调性 —— 通过（无跳级、无 h5+）**
- 全文标题级别集合 = `{1, 2, 3, 4}`，**没有 `#####`/`######`**。
- `h1` 仅 1 个（L1 标题）；`h2` 逐处为章；`h3` 为章内小节；`h4` 仅在 §5.5、§6.2/§6.3/§6.4、§7.1–§7.10、§8.1–§8.11、§9.1–§9.6、§10.1–§10.8、§14.1.1–§14.5.4、B.1/B.2 出现。逐处检查未发现 `h2 → h4` 式跳级。
- 一处**风格例外（非错误）**：第 4 章的 11 个 `h3` 用 `### 0.`–`### 11.` 编号（L198–L739），与其余章 `### N.M` 形态不同；但 L40-43 的阅读约定已显式声明此约定（「§4.6」= 第 4 章第 6 点），且 §1.1 起其余章采用 `N.M`。**该约定与实际全文一致**，可通过。

**3. 交叉引用连续性（抽查 10 处）—— 9 处通过、1 处指向错误（[E3]）**
| 引用 | 所在行 | 目标 | 结论 |
| --- | --- | --- | --- |
| §13.5 | L10/193/756/1085/3444/3557/3825 等 27 处 | `## 13.5`（L3452） | 通过 |
| §8.6 | L60（术语定义） | `#### 6.1`–`#### 6.6` 段，`§8.6` = §8 章 cost mode 段 | 通过 |
| §4.6 第 4 条 | L433 | §4 章 `### 6. cache token 语义`（L509）下的第 4 点 | 通过 |
| §10.3 | L375 | §7 章 `### 10. --by-agent / all 报告` 的 `#### 10.3` | 通过 |
| §9.4 | L376 | §6 章 `#### 4.4 SQLite 适配器细节对照`（adapter-all 装配段） | 通过（按章内编号语义） |
| §5.3 | L2551 | §8 章 `#### 5.3 服务档倍率表` | 通过 |
| §7.6 | L3302 | §8 章 `### 7. cost mode 语义` 段（价格相关，原编号） | 通过 |
| §2.4 | L3039 | §9 章 `#### 2.4 分层帮助与 help_codegen` | 通过 |
| §1.1 | L2702 | §1 章小节 | 通过 |
| §8（三处例外） | L3356 | 无对应文本 | **[E3] 指向错误** |

**4. 换行与编码 —— 通过**
- 文件 417 284 字节；首 3 字节 `23 20 63`（`# c`）→ **无 BOM**；`CRLF` 计数 **0**、`LF` 计数 3890、行数 3891（含末行空）→ **全 LF、UTF-8 无 BOM**。

**5. 表格完整性（59 个表格块全检 + 断列定位）—— 2 处待修**
- 用脚本把全文按围栏外、以 `|` 起止的连续行聚合成 **59 个表格块**，逐块校验「第二行是分隔行」与「各行列数一致」。
- 结果：**57 块完全干净**；2 块报告列数不齐，定位后 1 块为真断列、1 块为误报：
  - **真断列**：L3613 起的 §14.1.4 表，第 3616 行 `||` 未转义 → [E1]。
  - **误报**：L3528 起的 §13.5 运行时行为表报「3 列 vs 6 列」，逐字确认是 `\|` 已转义（`--speed [auto\|standard\|flex\|fast]`），脚本未排除反斜杠转义所致，**非缺陷**。
- 另发现两处未转义 `|`（L1512、L3173）→ [E6]（不在本复核段，属全局发现）。

**6. 其余结构项**
- 全文 6 处「第 8 轮实测复核…」提示块（L741、L1067、L1484、L2140、L2733、L3310）措辞一致，均正确指向 §13.5 或声明「本节保留原始清单」。
- §14.6（L3814-3825）10 条未验证项编号连续、无重复；§13.3 的 10 条与 §13.5 表 10 行的编号一一对位（已逐条比对）。
- `附录 A` 22 行常量表：列数 4、分隔行正常；`附录 B` B.1（10 行）/B.2（20 行）列数 3、分隔行正常。

## 无法验证

- **U1 运行时 JSON 键名（§13.5 表第 2 行 / L3521-3531）**：需构建二进制（`CCUSAGE_PRICING_JSON_PATH` 注入 + ~1 分 17 秒编译）并用本机 `~/.claude`/`~/.codex` 运行，本轮只做只读静态复核，未复跑。相关键名与本报告 §7/§8 描述交叉一致，但「实测」二字的举证责任仍在此前的第 8 轮记录。
- **U2 仓库自测 188 passed（L3400、L3471）**：未复跑 `cargo test -p ccusage-core --lib`。
- **U3 `format_currency` 的具体舍入数值（L3480 表第 3 行）**：未复跑 `fmt-probe.exe`；只确认了格式化点 `output.rs:548-550` 是 `format!("${value:.2}")`。
- **U4 在线端点三个数字（L3481：229 provider / 8830 条目 / 4043 去重 id）与「28 个键已消失」**：需联网抓 `https://models.dev/api.json`；本轮未联网抓取。仅用本地快照独立验证了「内嵌 3049 键」这一半（精确一致）。
- **U5 构建产物 `out/litellm-pricing.json.deflate` 的 534 键逐键逐值比对（L3464）**：该产物在另一个工作副本的 `target/` 下，本轮未解压比对；只复跑了重放脚本（`embedded-keys.json` 侧的键集与命中行为一致）。
- **U6 `droid` 与 `kimi` 的 loader 全部分支**：只核对了文档给出的关键行号区间（二者均在抽查中命中），未逐行读完 267/128 行。
- **U7 §14.2.3 中标注「grep 定位 + 邻段」的 8 家**（opencode、copilot、pi、qwen、kimi、openclaw、hermes、goose）：文档自身已声明证据粒度弱于逐行读（§14.6 第 9 条），我抽样覆盖了其中 openclaw（`paths.rs:51-58`）、opencode（`loader.rs:203-226`）、goose（`parser.rs:25-33`、`loader.rs:72-89`），其余 5 家未逐个复读。
- **U8 `CODEX_REWRITTEN_BURST_PAUSE_MS = 1000` 的经验依据（§13.3 第 10 条 / §13.5 表第 10 行）**：文档自认未闭环，我确认常量位置（`codex/src/parser.rs:122`）与注释存在，但注释里的经验依据无法在仓库内被证伪或证实。

## 我实际打开过的文件（≥15 个 file:line）

**TokenScope 侧（8 处）**
1. `C:\Users\admin\Desktop\项目\tokenscope\src\source\claude.rs:50-74` → `ClaudeUsage` 四字段，`cache_creation_input_tokens` 单字段
2. `...\src\source\claude.rs:355-372` → `cache_write_tokens: usage.cache_creation_input_tokens`
3. `...\src\pricing.rs:195-216` → `PriceRates { input, output, cache_write, cache_read }`
4. `...\src\pricing.rs:2304-2333` → 候选计价与「禁止跨候选拼价」
5. `...\src\pricing.rs:2396-2415` → `highest_complete_cost` 与最高费用选择
6. `...\src\pricing.rs:1737-1754` → `Pricing::default()` → `empty()`
7. `...\src\source\codex.rs:1-10` 与 `:310-329` → 忽略 `token_usage_record` + 计数
8. `...\src\dedupe.rs:1-46` → `(session_id, record_id)` 单键、最晚时间戳胜
9. `...\src\model_identity.rs:138-159` → `separator_boundaries`
10. `...\src\report.rs:1262-1323` → `(size, mtime)` 指纹与 `fp_before/fp_after`

**ccusage 侧（30+ 处）**
11. `rust/crates/ccusage-core/src/lib.rs:50-89` → `DEFAULT_SESSION_DURATION_HOURS`/`BUILT_IN_AGENT_NAMES`（18 项）
12. `rust/crates/ccusage-core/src/pricing.rs:54-65` → `MODELS_DEV_FAILURE_RETRY_AFTER`、`MODEL_DATE_SUFFIX_DIGITS`
13. `rust/crates/ccusage-core/src/pricing.rs:216-220`、`:249-260` → 长上下文阈值、DeepSeek V4 cutoff
14. `rust/crates/ccusage-core/src/pricing.rs:645-656` → trust 3/2/1
15. `rust/crates/ccusage-core/src/pricing.rs:900-980` → `SpeedMultiplierOverrides`（`exact` + `normalized_prefix`）
16. `rust/crates/ccusage-core/src/pricing.rs:1030-1107` → override 最后执行、`input*1.25` / `input*0.1`
17. `rust/crates/ccusage-core/src/pricing.rs:1830-1864` → `should_scale` 与显式值保留
18. `rust/crates/ccusage-core/src/pricing.rs:2362-2411` → 三处无条件 insert
19. `rust/crates/ccusage-core/src/pricing.rs:2655-2722` → `pricing_key_matches` / `contains_pricing_key` / 版本后缀规则
20. `rust/crates/ccusage-core/src/pricing.rs:2795-2808` → 三条内置别名、`matches_model_suffix`
21. `rust/crates/ccusage-core/src/pricing.rs:4931-4948` → `embedded_pricing_resolves_overlapping_model_keys_exactly` 断言本体
22. `rust/crates/ccusage-core/src/cost.rs:1-14`、`:60-73`、`:112-133` → 1h×2、`total_tokens==0` 门槛
23. `rust/crates/ccusage-core/src/output.rs:44-55`、`:144-157`、`:256-277`、`:425-473`、`:544-553` → JSON 键、`group_project_output`、项目头渲染、缺价警告、`format_currency`
24. `rust/crates/ccusage-core/src/logger.rs:1-15`（全文件）→ 无 0–5 级别表
25. `rust/crates/ccusage-core/src/project_names.rs:1-135`、`:136-164` → 清洗链 9 步 + 测试断言
26. `rust/crates/ccusage-core/src/build.rs:150-224` → 裁剪白名单前缀全清单
27. `rust/crates/ccusage-core/src/models-dev-pricing.json`（Python 解析）→ 3049 键；`anthropic/claude-sonnet-4-20250514`、`claude-opus-5-fast`（`exactOnly: true`）等存在性
28. `rust/crates/ccusage-core/src/fast-multiplier-overrides.json:1-15`（Python 解析）→ `normalized_prefix["claude-opus-4-6"] = 6.0`
29. `rust/crates/ccusage/src/blocks.rs:12-23` → `BLOCKS_WARNING_THRESHOLD` / `BLOCKS_COMPACT_WIDTH_THRESHOLD`
30. `rust/crates/ccusage/src/http.rs:8-15` → 三个 HTTP 常量
31. `rust/crates/ccusage/src/commands/mod.rs:30-74` → `run_daily`（无 `by_agent`）
32. `rust/crates/ccusage-cli-parser/src/parser.rs:369-382`、`:437-476` → `--by-agent` 两条解析路径
33. `rust/crates/ccusage-adapter-all/src/lib.rs:35-48` → `let include_agents = args.by_agent`
34. `rust/crates/ccusage-adapter-all/src/loader.rs:537-550` → `path_key` / 重叠检测
35. `rust/crates/ccusage-config/src/config.rs:388-405`、`:1240-1254` → `is_agent_command`、保留名一致性测试
36. `rust/crates/ccusage-config/src/config_schema.rs:426-439` → `project_aliases` 的注释与实现不符
37. `rust/crates/ccusage-test-support/src/lib.rs:13-62`、`:128-171` → `ENV_LOCK`、两个 Guard、`fs_fixture!`
38. `rust/crates/ccusage-test-support/src/zcode.rs:78-113` → SQLite fixture 插行
39. `rust/adapters/common/src/lib.rs:14-98` → `collect_files_with_extension`、`chunk_file_indexes_by_size`
40. `rust/adapters/common/src/jsonl.rs:20-89`、`:131-190`、`:300-329` → 零件语义与回归测试
41. `rust/adapters/claude/src/lib.rs:142-155`、`:232-268`、`:420-449`、`:500-525` → Display 不加载价格表、sidechain 路由、时间戳守卫、四种丢行
42. `rust/adapters/claude/src/daily.rs:485-509` → daily 侧缺时间戳守卫
43. `rust/adapters/claude/src/paths.rs:62-89`、`:244-254`、`:265-288` → 过滤分支、`is_project_path_segment`、`extract_project`
44. `rust/adapters/codex/src/parser.rs:100-127`、`:136-169`、`:505-534`、`:587-601` → 读行失败、四种 `continue`、compaction 认领、headless 反序列化
45. `rust/adapters/droid/src/loader.rs:20-39`、`rust/adapters/antigravity/src/parser.rs:143-160`、`loader.rs:17-40`、`rust/adapters/codebuff/src/parser.rs:38-61,170-183`、`rust/adapters/goose/src/parser.rs:22-33`、`loader.rs:72-93`、`rust/adapters/zcode/src/parser.rs:30-65`、`loader.rs:272-307`、`rust/adapters/grok/src/parser.rs:216-231,465-482`、`rust/adapters/gemini/src/loader.rs:22-41`、`rust/adapters/amp/src/loader.rs:20-38`、`rust/adapters/opencode/src/loader.rs:203-226`、`rust/adapters/openclaw/src/paths.rs:48-61`、`rust/adapters/goose/src/paths.rs:20-37`、`rust/adapters/antigravity/src/paths.rs:58-81` → §14.2.3/§14.3 分档与路径立场

**证据脚本与元数据**
46. `docs/research/ccusage-draft/evidence-round8/README.md:1-73`（复现步骤与使用边界）
47. `docs/research/ccusage-draft/evidence-round8/replay-match.js:1-77`（含 `suffixStartsWithNumericModelVersion`）
48. 复跑命令：`node replay-match.js embedded-keys.json gpt-5.4-9 claude-opus-5-9 gpt-500 opus550 claude-opus-5-fast gpt-5.4 claude-opus-5 claude-sonnet-4-20250514 claude-3-5-haiku-20241022`
49. `docs/research/ccusage-draft/REVIEW-CHECKLIST.md:1-71`（格式基准）
50. 全文编码/行数核查：417 284 字节、无 BOM（`23 20 63`）、CRLF=0、LF=3890

**只读声明**：本轮未修改任何 ccusage 源码、未改动 TokenScope 源码、未写入 `evidence-round8/`（期间误建的临时脚本已删除，改写到 `%TEMP%` 并在核对后保留在系统临时目录）。所有核对命令均为读取或重放 `evidence-round8/` 既有只读输入。
