# REVIEW-FULL-C3 —— §9–§11（L2748–L3343，596 行）全文复核

复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md` 第 9 章（配置/CLI/输出契约，26 小节）、第 10 章（版本史，9 小节）、第 11 章（实现与官方文档差异清单，11 行）。
样本：主仓 `%TEMP%\ccusage-research\ccusage`，v19 对照仓 `%TEMP%\ccusage-research\ccusage-v19`，二进制 `ccusage\rust\target\debug\ccusage.exe`（运行一律加 `--offline`）。
方法：逐节读完 596 行；对源码/文档逐条定位（read/grep）；对 §11 第 8、11 行与帮助文本做运行时实测；对 v19 侧打开原文件核对；跨章一致性对照 §5、§6、§7、§13.5。
本报告只记录结构、键名、行号与常量，不含任何会话内容或用量数值。

## 结论（一句话）

§9–§11 的源码定位准确度很高：抽查 63 条，62 条与源码/文档一致，唯一实质错误是 §10.4「取值主路径两版一致」（与 §5.3 及代码事实相反）；第 8 轮争议点（`--by-agent` 适用范围、`--jq` 隐藏性）本次实测双双通过；另需处理 3 处引用层问题（§10.8 小节交叉引用错位、§9/§10 存疑节顶部指引与 §13.5 不匹配、§11 第 2 行 `types.rs` 引用缺 crate 限定）。

## 抽查清单（63 条）

| # | 位置 | 文档断言 | 证据（≤3 行源码/文档摘录） | 判定 |
| --- | --- | --- | --- | --- |
| 1 | §9 首（L2753-2754） | 主仓 HEAD `57afdc2`=v20.0.31；v19 HEAD `4aedac0`=tag v19.0.3 | `git rev-parse HEAD` → `57afdc25…`；`describe --tags --exact-match` → `v20.0.31`；v19 → `4aedac06…` / `v19.0.3` | 成立 |
| 2 | §9.1.1 | `apps/ccusage/config-schema.json` 14415 行 | 实测 `(Get-Content …).Count = 14415` | 成立 |
| 3 | §9.1.1 | `docs/package.json:8` 拷贝 schema 后再构建 | `"build": "cp ../apps/ccusage/config-schema.json public/config-schema.json && vitepress build"` | 成立 |
| 4 | §9.1.1 | 一致性由 `nix/checks.nix:90-111` 强制，含指定报错文案 | `Only the tracked apps/ccusage/config-schema.json is checked; docs/public/config-schema.json is a gitignored build copy.`（`:90-91`）；`ERROR: apps/ccusage/config-schema.json is out of sync with the Rust schema source.`（`:111`） | 成立（且解释了工作区里 `docs/public/config-schema.json` 不存在属预期） |
| 5 | §9.1.2 | `discover_config_paths` 顺序 = `$CWD/.ccusage/ccusage.json` → 各 Claude 目录 | `config.rs:258-269`：`cwd.join(".ccusage").join("ccusage.json")` 后 `paths.extend(claude_config_dirs()…)` | 成立 |
| 6 | §9.1.2 | `CLAUDE_CONFIG_DIR` 逗号切分 / 否则 `~/.config/claude` 与 `~/.claude` | `config.rs:271-283`：`.split(',').map(str::trim).filter(\|p\| !p.is_empty())`；否则 `vec![home/.config/claude, home/.claude]` | 成立 |
| 7 | §9.1.2 | 只取第一个「可读且是 JSON object」的文件，失败静默跳过 | `config.rs:246-256`：`.filter_map(read_to_string.ok).filter_map(from_str.ok).find(\|v\| v.as_object().is_some())` | 成立 |
| 8 | §9.1.2 | `scan_config_path` 支持 `=` 与分离写法，空值不认 | `config.rs:285-299`：`if flag == "--config" && !value.is_empty()` / `else if arg == "--config" { args.get(index+1) }` | 成立 |
| 9 | §9.1.2 | 没有 `CCUSAGE_CONFIG` 环境变量 | 全树 `env::var(_os)?(` 穷举 38 处，`CCUSAGE_*` 仅 3 个（VERSION / PRICING_JSON_PATH / MODEL_ALIASES） | 成立 |
| 10 | §9.1.3 | 顶层含 `$schema`/`defaults`/`commands` + 18 个 agent 段，全 camelCase | `config_schema.rs:14` `#[serde(rename_all = "camelCase")]`；`:24-58` 列 claude…zcode 共 18 段 | 成立 |
| 11 | §9.1.3 | 根 `commands` 只允许 6 项 | `config_schema.rs:61-70`：`daily/weekly/monthly/session/blocks/statusline` | 成立 |
| 12 | §9.1.4 | 覆盖顺序 6 级（根 defaults → …→ `<agent>.commands.<report>`） | `config.rs:92-132`：maps.push 顺序与文档逐字对应 | 成立 |
| 13 | §9.1.4 | 配置给的 `order` 会置 `order_explicit` | `config.rs:588-591`：`shared.order = order.into(); shared.order_explicit = true;` | 成立 |
| 14 | §9.1.4 | `pricingOverrides` 逐字段合并 | `config.rs:637-681`：`if source.input_cost_per_token.is_some() { target.input_cost_per_token = … }`（11 字段同构） | 成立 |
| 15 | §9.1.5 / §11-5 | 配置日期校验文案与 `Invalid ccusage config:` 包装 | `config.rs:69-90`：`"{key} must be a string. Expected {DATE_BOUND_FORMATS}"`；`:198-200` `format!("Invalid ccusage config: {}", …)` | 成立 |
| 16 | §9.1.5 | `pi.stores` 校验（数组/必填/非空/name 正则/冲突/重名） | `config_schema.rs:11` `"^[a-z][a-z0-9_-]{0,31}$"`；`config.rs:180-188` `must match`、`collides with a built-in agent`、`duplicate pi.stores name` | 成立 |
| 17 | §9.1.6 | schemars 派生 + 手工增强；缩进用 tab | `config_schema.rs:711` `pub fn generate_config_schema_json()`；`:797` `let mut formatted = "\t".repeat(spaces / 2);` | 成立 |
| 18 | §9.1.6 | 提交产物刷新命令在 `apps/ccusage/justfile:25-27` | `cargo run … --bin generate-config-schema -- config-schema.json` / `oxfmt --write` / `cp … ../../docs/public/config-schema.json` | 成立 |
| 19 | §9.1.7 | `mode=auto`、`debug_samples=5`、`order=asc` | `cli/src/types.rs:65-67`：`mode: CostMode::Auto, debug_samples: 5, order: SortOrder::Asc` | 成立 |
| 20 | §9.1.7 | statusline 默认 offline/cache/interval/阈值 | `cli/src/types.rs:224-232`：`offline: true, cache: true, refresh_interval: 1, 50, 80` | 成立 |
| 21 | §9.1.7 | 统一周报周起点 = monday | `cli/last_window.rs:60-62`：`const UNIFIED_WEEK_START: WeekDay = WeekDay::Monday;` | 成立 |
| 22 | §9.2.1 | 自研 parser；`parser.rs` 1247 行、`arg_parser.rs` 104 行 | 实测行数 1247 / 104；`arg_parser.rs:3-7` 为结构定义 | 成立 |
| 23 | §9.2.2 | 共享选项短/长名清单 | `parser.rs:745-776` 逐条比对：`-s -u --last -j -m -d --debug-samples -o -b -O/--no-offline --color/--no-color -z -q --config --compact --single-thread --no-cost` | 成立 |
| 24 | §9.2.2 | blocks 专有 `-a/-r/-t/-n` | `parser.rs:203-213`：`"-a"\|"--active"`、`"-r"\|"--recent"`、`"-t"\|"--token-limit"`、`"-n"\|"--session-length"` | 成立 |
| 25 | §9.2.3 | `accepts_root_all_options` 只认 4 个报告 | `parser.rs:366-368`：`matches!(command, "daily"\|"monthly"\|"weekly"\|"session")` | 成立 |
| 26 | §9.2.4 | help 由 build 期 codegen；`--version` 打印 `ccusage {version}` | `help.rs:13` `include!(concat!(env!("OUT_DIR"), "/cli-help.rs"));`；`:16` `println!("ccusage {version}")`；`build.rs:5-6` 复用 `#[path = "src/help_codegen.rs"]` | 成立 |
| 27 | §9.2.4 / §11-8 | `-q/--jq` 可解析但**不在**帮助文本 | 实测 `ccusage daily --help`、`ccusage --help` 均无 `--jq`；`grep jq` 在 `ccusage-cli-parser/src` 只命中 `parser.rs:771/923/1039`、`tests.rs:133/971`、snapshots（cli-help.json 无命中） | 成立 |
| 28 | §9.2.5 | 解析失败 → stderr 两行 + `exit(2)` | `ccusage/src/cli.rs:35-39`：`eprintln!("Run 'ccusage --help' for usage."); process::exit(2);` | 成立 |
| 29 | §9.3 / §11-8 侧 | `--json` 与 `--jq` 不互斥 | `output.rs:14-16`：`shared.json \|\| shared.jq.is_some()` | 成立 |
| 30 | §9.3 | 真冲突清单（`--last` 三条 + 日期窗口 + session --id） | `parser.rs:1102-1116`、`1123-1132`、`454-460` 文案逐字一致 | 成立 |
| 31 | §9.4.1 | 行级 JSON 手写 `json!`、camelCase；`agents` 是报告行键 | `agent_report.rs:5-19`；`report.rs:148-156` 插 `"agents"`；实测 `daily --by-agent` 行级含 `agents`，不带则无（`agent` 键两种情况都在） | 成立 |
| 32 | §9.4.2 | jq 是外部二进制，stdin 传 JSON，非零退出报 `jq failed` | `output.rs:163-178`：`Command::new("jq").arg(filter)…`；`return Err(cli_error("jq failed"))`；`no_cost` 在 jq 之前剥离（`:159-161`） | 成立 |
| 33 | §9.4.4 | 10 帧 braille spinner、80ms、非 json 且 TTY 才显示 | `progress.rs:11-12`：`SPINNER_FRAMES: [&str; 10]`；`SPINNER_INTERVAL = from_millis(80)`；`:35-37` `!json && output_is_tty` | 成立 |
| 34 | §9.5 | 环境变量穷举：21 个 `_ENV` 常量 + 13 个直接字面量 = 33 个变量 + 1 个 Cargo 注入项 | 按 grep 结果逐一归类：`_ENV` 常量 21 个（DROID/CODEBUFF/GEMINI/GOOSE/PRICING_JSON_PATH/AMP/MODEL_ALIASES/KIMI/ANTIGRAVITY/COPILOT_HOME/COPILOT_OTEL/ZCODE/KILO/HERMES/GROK/OPENCODE/XDG_DATA_HOME/PI/QWEN/VERSION/OPENCLAW），字面量 13 个（HOME/USERPROFILE/HOMEDRIVE/HOMEPATH/LOG_LEVEL/CLAUDE_CONFIG_DIR/XDG_CONFIG_HOME/CODEX_HOME/XDG_CACHE_HOME/COLUMNS/NO_COLOR/FORCE_COLOR/OUT_DIR） | 成立（附一处口径补充，见「建议改述」2） |

### 抽查（§10）

| # | 位置 | 文档断言 | 证据 | 判定 |
| --- | --- | --- | --- | --- |
| 35 | §10.1 | v19 文件规模（cli.ts 234、index.ts 7、shared.ts 735、data-loader.ts 7004、table.ts 1713） | 实测行数全部一致 | 成立 |
| 36 | §10.1 | `cli.ts:9` 要求 Node ≥ 22 | `const MIN_NODE_MAJOR_VERSION = 22;` | 成立 |
| 37 | §10.2 | v19 只接受 `YYYYMMDD` | `shared-args.ts:19-30`：`description: 'Filter from date (YYYYMMDD format)'` + `parse: parseDateArg`；`:11-13` → `v.parse(filterDateSchema, value)` | 成立 |
| 38 | §10.2 | v19 `-O` 为 offline 且 `negatable` | `shared-args.ts:69-75`：`offline: { type: 'boolean', negatable: true, short: 'O' }` | 成立 |
| 39 | §10.2 | v19 配置搜索路径与常量 | `config-loader-tokens.ts:79-82`：`const dirs = [join(process.cwd(), '.ccusage'), ...toArray(getClaudePaths())];`；`consts.ts:94` `CONFIG_FILE_NAME = 'ccusage.json'` | 成立 |
| 40 | §10.2 | v19 三个错误函数在 `commands/index.ts:198-227` | `:198` `getReportFlagAliasError`、`:206` `getAgentFilterOptionError`、`:219` `getUnsupportedAgentReportError`（第三个函数体延伸到 `:234` 之后） | 成立（行号区间偏短，见「建议改述」4） |
| 41 | §10.3 | v19 去重键 `messageId[:requestId]` | `data-loader.ts:1194-1202`：`return requestId == null ? messageId : \`${messageId}:${requestId}\`;` | 成立 |
| 42 | §10.3 | v19 替换规则：token 总数大者胜，平手看 `speed` | `data-loader.ts:1633-1637`：`if (candidate.tokenTotal !== existing.tokenTotal) return candidate.tokenTotal > existing.tokenTotal; return candidate.hasSpeed && !existing.hasSpeed;` | 成立 |
| 43 | §10.3 | v20 键升级：`msg_` 前缀才允许无 requestId 跨时间戳 | `claude/src/lib.rs:359-363`：`if request_id.is_none() { session_id.hash(...); if !is_anthropic_message_id(message_id) { timestamp.hash(...) } }`；`:373-375` `starts_with("msg_")` | 成立 |
| 44 | §10.4 | v19 无条件优先 `lastUsage`，缺失才回退差分 | `codex/parser.ts:206-209`：`let raw = parsed.lastUsage; if (raw == null && parsed.totalUsage != null) { raw = subtractRawUsage(parsed.totalUsage, previousTotals); }` | 成立 |
| 45 | §10.4 | v20 `subtract_codex_raw_usage` 用 `saturating_sub` 且多 `cache_creation_tokens` | `codex/src/parser.rs:1288-1297`：`input_tokens: current.input_tokens.saturating_sub(previous.map_or(0, …)), cache_creation_tokens: …` | 成立 |
| 46 | §10.4 | 四个 v20 测试名与行号 | grep 命中：`loader.rs:552` `dedupes_matching_codex_usage_events_from_distinct_sessions`、`:562` `dedupes_compaction_usage_by_response_id`、`:615` `dedupes_copied_branch_history_across_session_files`、`:943` `subtracts_cache_write_usage_from_cumulative_token_totals` | 成立 |
| 47 | §10.5 | v19 5h block：常量/整点对齐/双条件切块/gap/活跃判定 | `session-blocks.ts:7` `= 5`；`:135`/`:165` `floorToHourMs(entryTimeMs)`；`:146` `if (timeSinceBlockStart > sessionDurationMs \|\| timeSinceLastEntry > sessionDurationMs)`；`:157-162` gap；`:202` `const isActive = nowMs - actualEndTimeMs < sessionDurationMs && …` | 成立 |
| 48 | §10.5 | v20 结构一致，gap 暴露为 `isGap` | `blocks.rs:75` `if since_start > session_duration \|\| since_last > session_duration {`；`:89` `floor_to_hour(entry.timestamp)`；`:211` `"isGap": block.is_gap` | 成立 |
| 49 | §10.6 | v19 匹配：候选前缀直查 → 否则双向 `includes` 取长度差最小 | `packages/internal/src/pricing.ts:249-251` `candidates.add(\`${prefix}${modelName}\`)`；`:276` `if (!comparison.includes(lower) && !lower.includes(comparison)) continue;` | 成立 |
| 50 | §10.7 | v19 三态语义 | `data-loader.ts:1291-1294` `if (mode === 'display') { return data.costUSD ?? 0; }`；`:1307-1321` auto/costUSD 优先；`:1326-1336` `getImmediateCostForEntry` | 成立 |
| 51 | §10.7 | v20 三态 + 缺价信号 | `cost.rs:39-45`：`Display => cost_usd.unwrap_or(0.0)` / `Auto => cost_usd.unwrap_or_else(...)` / `Calculate => calculate_cost_from_tokens(...)`；`:55` `if mode == CostMode::Display \|\| (mode == CostMode::Auto && cost_usd.is_some()) { return None; }` | 成立 |
| 52 | §10.4 末段 / §10.8-4 | 「取值主路径两版一致」 | v19 无累计前进判定；v20 `parser.rs:484-489` 有 `cumulative_advanced` 过滤：`let raw_usage = info.and_then(|i| i.last_token_usage…).filter(\|_\| cumulative_advanced).or_else(\|\| …subtract…)` | **不成立**（见「必须修正」2 / 「内部矛盾」1） |

### 抽查（§11 逐行）

| # | 行 | 文档说法 vs 代码事实 | 证据 | 判定 |
| --- | --- | --- | --- | --- |
| 53 | 1 | `blocks-reports.md:150-151` 写「newest first (default)」；代码默认 `Asc` | 文档：`# Show newest blocks first (default)` / `ccusage blocks --order desc`；代码 `cli/src/types.rs:250-255`：`Desc,` 后 `#[default]` 落在 `Asc` | 成立 |
| 54 | 2 | `weekly-reports.md:138-149` 是对象映射；代码是数组 | 文档：`"modelBreakdowns": { "claude-opus-4-1-20250805": { … } }`；代码 `ccusage-core/src/types.rs:175` `pub model_breakdowns: Vec<ModelBreakdown>` | 成立（引用缺 crate 限定，见「必须修正」4） |
| 55 | 3 | `codex/index.md:50`/`:116` 说跳过，`:54` 说仍计入 | `:50` `Sessions lacking model metadata (seen in early September 2025 builds) are skipped.`；`:54` `Early September 2025 logs that never recorded turn_context metadata are still included; the CLI assumes gpt-5 …`；`:116` `Those entries are ignored to avoid mispriced reports.` | 成立（文档自相矛盾成立） |
| 56 | 4 | 三个 marker 符号代码未实现；真实实现是 usage 全等 + 重写突发 | 全仓 grep `task_started\|inter_agent_communication\|trigger_turn` **只命中** `rust/adapters/codex/src/README.md:50-51`；实现 `codex/src/parser.rs:236` `if prefix.get(index) == Some(&usage)`、`:122` `const CODEX_REWRITTEN_BURST_PAUSE_MS: i64 = 1_000;` | 成立（本轮最硬的一条） |
| 57 | 5 | `environment-variables.md:124/129/209` 出现 `CCUSAGE_OFFLINE`，两版均无实现 | 文档 `:124` `### CCUSAGE_OFFLINE`、`:129` `export CCUSAGE_OFFLINE=1`、`:209` 同；源码穷举无该变量 | 成立 |
| 58 | 6 | `configuration.md:38` 出现 `CCUSAGE_TIMEZONE` | `export CCUSAGE_TIMEZONE="Asia/Tokyo"`；源码穷举无该变量 | 成立 |
| 59 | 7 | 设计稿点名 `CCUSAGE_GROK_PATH` 并自行要求移除 | `:23-24` `There will be no … \`CCUSAGE_GROK_PATH\` variable`；`:51` `Remove any remaining \`CCUSAGE_GROK_PATH\` handling`；`:52` `Keep only \`GROK_HOME\` and \`~/.grok\``；实现 `grok/src/paths.rs:9` `GROK_HOME_ENV` | 成立 |
| 60 | 8 | `--jq` 可解析生效但未出现在帮助 | 实测两处 help 无 `--jq`；`parser.rs:771` 可解析 | 成立 |
| 61 | 9 | 成本公式（1h 缓存 = input × 2）与代码一致 | 文档 `cost-modes.md:203` `cacheCreate1hTokens * inputPrice * 2`；代码 `cost.rs:7` `CACHE_CREATE_1H_INPUT_MULTIPLIER: f64 = 2.0;`、`:128` `pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER` | 成立 |
| 62 | 10 | 缓存读价缺失回退 input 价，文档与代码一致 | 文档 `codex/index.md:55` `cached input uses the cache-read price (falling back to the input price when missing)`；代码 `codex/src/report.rs:344-348` `let cache_read = if pricing.cache_read_explicit { pricing.cache_read } else { pricing.input };` | 成立 |
| 63 | 11 | 顶层 `daily --by-agent` 生效、`claude daily --by-agent` 报错 | 实测：`daily --json --by-agent --offline` 行级键含 `agents`（不带则无，两次 `agent` 键都有）；`claude daily --json --by-agent --offline` → `Unknown daily option '--by-agent'`，exit 2；解析器 `parser.rs:492-498` + `commands/mod.rs:33-70` 走 `DailyArgs` | 成立 |

## 必须修正

1. **§10.8 第 3/4/5/9 条的括号交叉引用在终稿编号下指向错误小节**（L3296、L3297、L3298、L3302）。
   终稿 §7 是「聚合、时间与报告形态」，其小节为 `7.1 聚合原语`、`7.2 时间与时区`、`7.3 日期过滤`、`7.4 周报/月报`、`7.6 5 小时 blocks`——与四条所指内容（Claude 去重 / Codex 累计 / 日期输入 / 价格）无一对应。
   建议改为：第 3 条 → §4.4（`rust/adapters/claude/src/lib.rs:350-375`）、第 4 条 → §5.3（`codex/src/parser.rs:478-497`）、第 5 条 → §9.2.2 与 §9.3、第 9 条 → §8。
   （同章 L3304 的 `（§2.4）`、L3301 的 `（§1.5）` 实指 §9.2.4 与 §9.1.5，属同一批「跨章引用省略了章号」的写法，建议一并显式化。）

2. **§10.4 末段与 §10.8 第 4 条的核心结论「取值主路径两版一致」不成立**（L3239、L3297）。
   v19 `parser.ts:206` 是 `let raw = parsed.lastUsage;`——**无条件**优先；v20 `parser.rs:487-494` 先算 `cumulative_advanced`，再 `.filter(|_| cumulative_advanced)` 把 `last_token_usage` 丢掉并转走差分。这是 v20 新增的判据，会改变「累计值未前进但 `last_token_usage` 存在」这类样本的结果。
   建议：把该结论改为「取值顺序相同，但 v20 多一道『累计值必须前进』的门控」，并与 §5.3（L859 已正确写明该门控）保持同一措辞。

3. **§9/§10 的「未验证/存疑」节顶部指引与 §13.5 不匹配**（L3310）。
   指引称「已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目」，但本节 9 条存疑中**没有**「内嵌表键集」「格式化」类条目（这两类属 §8）；实际被 §13.5 触及的只有第 9 条（运行期验证）与第 1 条的一半（`--jq` 实测），而第 1 条的原始疑问「故意隐藏还是漏写」在 §13.5 中仍无结论。此外第 6 条（`blocks` JSON 字段全集）其实已被 §13.5 的实测行列全（L3525 列出 13 个块键），但本节未回填状态。
   建议：改为本节专属表述，例如「§13.5 已实测 CLI 解析与帮助文本（对应本节第 1、9 条与 §11 第 8 条）；第 2–5、7、8 条与 §13.5 无关，仍未闭环；第 6 条已由 §13.5 的 `blocks --json` 实测覆盖。」

4. **§11 第 2 行（及 §10.7 同处引用）的 `types.rs:175` 缺 crate 限定，会指错文件**（L3329、L3286）。
   表内第 1 行写全了 `rust/crates/ccusage-cli/src/types.rs`；第 2 行只写 `types.rs:175`——但 `ccusage-cli/src/types.rs:175` 实际是 `AgentCommandArgs` 的 `pub shared: SharedArgs`。真实位置是 `rust/crates/ccusage-core/src/types.rs:175`（`pub model_breakdowns: Vec<ModelBreakdown>`）。
   建议：改写为 `rust/crates/ccusage-core/src/types.rs:175`。

## 建议改述

1. **§9.5 表两行的默认根被简化成单值**：`OPENCLAW_DIR` 只写 `~/.openclaw`（实现是四选一：`~/.openclaw`、`~/.clawdbot`、`~/.moltbot`、`~/.moldbot`，`openclaw/src/paths.rs:23-28`）；`CODEBUFF_DATA_DIR` 只写 `~/.config/manicode`（实现是 `~/.config/{manicode,manicode-dev,manicode-staging}` 三个频道目录，`codebuff/src/paths.rs:6`）。与 §6 总表（L1113、L1114）对照时会产生「两处不一致」的错觉。建议加「等」或与 §6 对齐。

2. **§9.5 的穷举口径可补一句边界**：`rg 'env::var'` 另有 2 处未计入——`rust/crates/ccusage-test-support/src/lib.rs:30`、`:59` 的泛型 `std::env::var_os(key)`（测试环境锁，key 由调用方传入）。它不构成用户可配置变量，但会让人复算时对不上 38 这个数。建议加脚注说明。

3. **§9.2.4 与 §9.10-1 的状态标注建议合并**：§9.2.4 只陈述「可解析但未进帮助」，读者容易当成已解释的现象；「故意隐藏还是漏写」目前只在 §9 存疑第 1 条与 §11 第 8 行出现。建议在 §9.2.4 直接加半句「原因未知，见 §9 存疑第 1 条 / §11 第 8 行」。

4. **§10.2 表 v19 侧 `commands/index.ts:198-227` 区间偏短**：第三个函数 `getUnsupportedAgentReportError` 自 `:219` 起、函数体延伸到 `:234` 之后（blocks/statusline 分支在 `:231-233`）。建议改为 `:198-236` 或写 `:198 起`。

5. **§10.6 表「抓取」行可补两个函数名**：v20 的 64 MiB 上限是**两处独立判定**——`content-length` 预检（`http.rs:104-114`）与 `read_bounded_body`（`:139-151`），后者是唯一对 chunked/无 content-length 响应生效的那道。补上可避免读者以为只有一处上限。

6. **§11 表第 1 行可补精确行号**：`SortOrder` 定义与默认值在 `rust/crates/ccusage-cli/src/types.rs:250-255`（`#[default]` 在 `:253`，`Asc` 在 `:254`），便于后续复核直接定位。

## 内部矛盾

1. **§10.4 与 §5.3 直接冲突（两处措辞不能同时为真）**：§5.3（L859）已写明 v20 会在 `cumulative_advanced == false` 时 `.filter()` 掉 `last_token_usage`、随后被全 0 判定跳过，并引了回归测试 `loader.rs:2244`（`skips_repeated_last_usage_when_cumulative_total_is_unchanged`）；而 §10.4 末段（L3239）与 §10.8 第 4 条（L3297）却下结论「取值主路径两版一致」「取值优先级不变」。同一份文档对同一机制给出两种不相容的描述。建议以 §5.3 为准改写 §10.4/§10.8。

2. **§9.5 环境变量表 vs §6 适配器变量：核查结论是「无冲突」**。逐一对照 18 家的环境变量名、设置后是否回落默认、逗号列表语义，两处无一条相互否证；差异只在**默认根的列举精度**（openclaw 四值 vs 一值、codebuff 三频道 vs 一频道、goose 三条候选 vs 「标准 Goose 根」）。归入「建议改述」1，不构成矛盾。

3. **§10 的 v19 描述 vs §4/§5 的 v20 描述：除上面第 1 条外，抽查到的点无矛盾**。已逐项对齐：Claude 去重（§4.4 的 `usage_dedupe_hash` 展开与 §10.3 的 `lib.rs:350-375` 引用同源）、Codex 事件去重（§5 的 `aggregate.rs` 键维度与 §10.4 的 `loader.rs:240` 各说各层，互补而非冲突）、5h block（§7.6 的 `blocks.rs:53-107` 与 §10.5 一致）、价格匹配（§8 与 §10.6 一致）。

4. **§10.4 与 §5.5 的「README 可信度」口径需协调（措辞级）**：§5.5（L920）以 `src/README.md:47-53` 作为「MultiAgent V2 subagent replay」的**命名与问题定义**来源，读起来像该 README 描述已被实现；§11 第 4 行则指出 README 里那套 marker 判定在代码中**不存在**。建议在 §5.5 补一句限定（「README 只用于命名问题；以下机制是代码实际实现」），避免读者带着 README 的心智模型去读 §5.1/§5.2。

## 无法验证

1. `ccusage-v19/packages/internal/src/pricing-fetch-utils.ts`（47 行）未读——§10.6「离线数据」行提到的构建期预取过滤细节只到 `pricing-macro.ts` 调用面（文档 §9 存疑第 5 条自陈，本次亦未补）。
2. `docs/guide/config-files.md`（614 行）只核实了存在性与行数，未逐行读；其「多文件回退」表述是否与「只取第一个可解析文件」的实现有出入，本次未定（文档 §9 存疑第 2 条）。
3. §10.2 表 v19 侧以下引用未逐行打开：`cli.ts:95-128`、`cli.ts:130-167`（运行时分派）、`commands/index.ts:4`（版本 import）、`commands/weekly.ts:26`（`startOfWeek`）。已核的相邻行（`cli.ts:9`、`index.ts:198-234`）一致，故判为低风险未验。
4. §10.5 v19 侧的 `session-blocks.ts:204-237`（块内聚合）、`:284-357`（burn rate / 预测）、`:373`（测试常量 `SESSION_DURATION_MS`）未读；本次只核了常量、整点对齐、切块、gap、活跃判定五处。
5. §9.4.3 的三条断言（`sanitize_terminal_text` 定义体 `output.rs:552-562`、再导出 `core/lib.rs:41`、测试 `output.rs:905-910`）未打开函数体核对。
6. §9.6.2 的 `cache_file_stem` FNV-1a（`http.rs:236-257`）与 models.dev 60s 失败节流（`pricing.rs:58`、`:795-799`）未读原文；本次只核了 10s 超时、64 MiB、ETag/304、缓存目录、原子写五处。
7. §9.1.7 的 `blocks.recent` lookback=3 天的**两个使用点**（`commands/mod.rs:573`、`:659`）只核到 `:573`（与 `DEFAULT_SESSION_DURATION_HOURS` 同用），`:659` 未读。

## 我实际打开过的文件（file:line）

主仓 `%TEMP%\ccusage-research\ccusage`：

- `rust/crates/ccusage-config/src/config.rs:55-199`、`:236-345`、`:404-433`、`:580-701`
- `rust/crates/ccusage-config/src/config_schema.rs:10-89`、`:352-479`、`:588-617`、`:705-744`、`:786-810`、`:852-883`
- `rust/crates/ccusage-cli-parser/src/parser.rs:198-217`、`:218-262`、`:364-413`、`:444-465`、`:486-535`、`:740-799`、`:795-844`、`:866-890`、`:1095-1139`
- `rust/crates/ccusage-cli-parser/src/arg_parser.rs:1-80`
- `rust/crates/ccusage-cli-parser/src/help.rs:10-69`
- `rust/crates/ccusage-cli-parser/src/help_codegen.rs:1-60`
- `rust/crates/ccusage-cli-parser/src/cli-help.json:1-50`
- `rust/crates/ccusage-cli-parser/build.rs:1-29`
- `rust/crates/ccusage-cli/src/types.rs:30-109`、`:160-189`、`:215-259`
- `rust/crates/ccusage-core/src/output.rs:10-39`、`:38-152`、`:155-199`、`:448-472`
- `rust/crates/ccusage-core/src/agent_report.rs:1-70`
- `rust/crates/ccusage-core/src/cost.rs:1-65`、`:120-139`
- `rust/crates/ccusage-core/src/lib.rs:55-66`
- `rust/crates/ccusage-core/src/types.rs:145-186`
- `rust/crates/ccusage-core/src/progress.rs:8-37`、`:86-95`
- `rust/crates/ccusage-core/src/model_aliases.rs:1-40`
- `rust/crates/ccusage-adapter-all/src/report.rs:130-214`
- `rust/crates/ccusage/src/http.rs:5-64`、`:75-159`、`:180-234`
- `rust/crates/ccusage/src/cli.rs:1-55`
- `rust/crates/ccusage/src/cli/last_window.rs:1-30`、`:52-69`
- `rust/crates/ccusage/src/blocks.rs:50-114`、`:204-217`
- `rust/crates/ccusage/src/commands/mod.rs:28-77`、`:565-589`
- `rust/crates/ccusage/src/main.rs:118-139`
- `rust/crates/ccusage/build.rs:1-19`
- `rust/adapters/claude/src/lib.rs:195-224`、`:348-389`
- `rust/adapters/codex/src/parser.rs:110-149`、`:228-272`、`:478-505`、`:800-829`、`:1282-1311`
- `rust/adapters/codex/src/loader.rs:236-249`、`:938-949`
- `rust/adapters/codex/src/report.rs:340-353`
- `rust/adapters/codex/src/README.md:40-59`
- `rust/adapters/*/src/paths.rs`（grok/kimi/qwen/kilo/openclaw/zcode/opencode/gemini/copilot 头部）
- `docs/guide/blocks-reports.md:143-156`
- `docs/guide/weekly-reports.md:132-151`
- `docs/guide/codex/index.md:40-117`
- `docs/guide/environment-variables.md:115-164`、`:196-220`
- `docs/guide/configuration.md:10-39`
- `docs/guide/cost-modes.md:194-213`
- `docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:20-64`
- `docs/package.json`、`docs/README.md:1-18`
- `nix/checks.nix:85-114`、`:168-187`、`nix/treefmt.nix:52-65`
- `apps/ccusage/justfile:20-33`、`apps/ccusage/src/cli.js:25-64`

v19 对照仓 `%TEMP%\ccusage-research\ccusage-v19`：

- `apps/ccusage/src/adapter/claude/data-loader.ts:1192-1209`、`:1282-1341`、`:1626-1641`
- `apps/ccusage/src/adapter/codex/parser.ts:85-164`、`:196-220`
- `apps/ccusage/src/session-blocks.ts:1-30`、`:128-207`
- `apps/ccusage/src/shared-args.ts:1-114`
- `apps/ccusage/src/pricing-fetcher.ts:1-25`
- `apps/ccusage/src/config-loader-tokens.ts:65-134`
- `apps/ccusage/src/cli.ts:1-20`
- `apps/ccusage/src/commands/index.ts:195-234`
- `apps/ccusage/src/consts.ts:88-101`
- `packages/internal/src/pricing.ts:1-60`、`:155-239`、`:240-299`、`:305-344`

运行时实测（`rust/target/debug/ccusage.exe`，全部 `--offline`）：

- `--version` → `ccusage 20.0.31`
- `daily --json --by-agent`（顶层键 `daily,totals`；行级键含 `agents`）
- `daily --json`（同一行级位置无 `agents`）
- `claude daily --json --by-agent` → `Unknown daily option '--by-agent'`，exit 2
- `daily --help` / `--help`（均无 `--jq`；`daily --help` 有 `--by-agent`）
