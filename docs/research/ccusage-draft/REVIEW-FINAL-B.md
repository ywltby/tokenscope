# REVIEW-FINAL-B：§9–§13 + 附录 A/B 终审（证伪轮）

- 终审对象：`docs/research/2026-10-14-ccusage-usage-statistics.md` 第 2733–3469 行（§9 起至文末）
- 两侧源码：ccusage HEAD `57afdc2`（v20.0.31）/ ccusage-v19（v19.0.3）/ TokenScope 工作区 `0.3.0`
- 抽查方法：逐条打开源码核对值、行号与语义；附录 A 全量 21 行、附录 B 全量 64 个文件行数逐项比对（口径 = read 工具实际行数）

## 结论（一句话）

抽查 40+ 条中仅 **1 处硬错误（JSON 键名 `byAgent` 应为 `agents`）**、3 处口径/引用问题（悬空 `E2`、XDG 断言与 §5 表冲突、「33 万字符」实为字节口径），其余数值、行号、v19↔v20 对照与文档指控**全部复现为真**；附录 A/B 无一处抄错；主要遗留风险是 §9/§10 的编号装配（1.x–7.x 体系）与四份复核报告编号的交叉引用混乱。

## 必须修正

- **[E1] §9 4.1（文档 2996 行）**：断言「容器键 … `byAgent`（`rust/crates/ccusage-adapter-all/src/report.rs:136-143`）」。
  实际：JSON 中**不存在 `byAgent` 键**；`--by-agent` 追加的键名是 `agents`，插入点在 `report.rs:148-156`（`obj.insert("agents".to_string(), …)`）；`report.rs:136-143` 是 `period` 插入与 `metadata` 兜底（`json!({ "agents": agents })` 只是 metadata 的值）。ccusage 官方文档同样写 `agents` 数组（`docs/guide/all-reports.md:33`、`docs/guide/json-output.md:71`），本文档 §7.10.3（2103 行）也写 `"agents": [ ... ]`。→ 改为「行级追加键 `agents`（`report.rs:148-156`）」，并注意它不是顶层容器键。
  ```rust
  148:     if include_agents
  152:         obj.insert(
  153:             "agents".to_string(),
  ```
- **[E2] §12.2 第 2 条（文档 3334 行）**：断言「daily 实现漏了这条守卫（**§13.2 的 E2**）」。
  实际：§13.2 全节没有 E 编号锚点（条目以「语义级（S1）」「证据错位（S3/S4/R3）」等标签列出）；`E2` 是复核报告 `docs/research/ccusage-draft/REVIEW-R1.md:91` 的编号（且 `REVIEW-R4.md:64` 也有一个 E2，指 `apply_total_token_fallback` 计数，极易误导）。→ 改为「复核报告 R1 的 [E2]，对应本文档 §13.2 第 2 条」。
- **[E3] §9 1.2（文档 2773 行）**：断言「XDG 只出现在 Claude 数据目录发现（`rust/adapters/claude/src/paths.rs:46`）与 HTTP 缓存（`rust/crates/ccusage/src/http.rs:39`）」。
  实际：生产代码里 XDG 变量有三处读取——`XDG_CONFIG_HOME`（`claude/src/paths.rs:46`）、`XDG_CACHE_HOME`（`ccusage/src/http.rs:39`）、**`XDG_DATA_HOME`（`opencode/src/paths.rs:6` 常量、`:29` 读取）**；后者与 §9.5 环境变量表自列的 `XDG_DATA_HOME` 行直接冲突。→ 把「只出现在」改为「配置查找不读 `XDG_CONFIG_HOME`；本仓还读取 `XDG_CACHE_HOME`（HTTP 缓存）与 `XDG_DATA_HOME`（OpenCode 数据根，见 §5 表）」。
- **[E4] §13.1（文档 3362 行）**：断言「6 份分片，合计约 **33 万字符**」。
  实际：`ccusage-draft/S1…S6` 的字符数合计 **250 153**（约 25 万），331 827 是 UTF-8 **字节数**（即 `Get-Item Length` 口径）。→ 改为「约 25 万字符（约 33 万字节）」，或显式标注口径为字节。

## 内部矛盾

- **[M1] `agents` vs `byAgent`（同 E1）**：§7.10.3（2103）写 `"agents"`、§9.4.1（2996）写 `byAgent`，同一 JSON 键两个名字。以 §7.10.3 与源码为准。
- **[M2] §12.3 第 4 条（3346 行）**：「§11 列了 10 条文档与实现不符」。实际 §11 表 10 行中第 9、10 行自标「**一致**（正面例子）」（`docs/guide/cost-modes.md:198-205`、`docs/guide/codex/index.md:55`），不符项为 8 条。→ 改为「§11 中 8 条不符、2 条一致」。
- **[M3] §9 1.2 vs §9 5 表（同 E3）**：XDG 覆盖范围两处不一致。
- **[M4] §12.2 第 6 条（3338 行）**：「实现只有 30 行」。实际 `rust/adapters/common/src/lib.rs:49-81` = **33 行**（含注释与空行）。→ 改「约 30 行」或「33 行」。
- **[M5] §12.1 第 5 条（3327 行）**：「白名单只含 Anthropic/OpenAI 家族」。实际 `rust/crates/ccusage-core/build.rs:203-221` 还含 `zai/`（非该两家）与 `openrouter/openai/`（属 OpenAI 家族）、`azure/`。→ 改为「白名单以 Anthropic/OpenAI 家族为主，另有 `zai/`」。
- 已核**无冲突**的交叉项：§8「缺价披露三处」（2140 行）与 §12.2 第 5 条（3337 行）三处一致（`missingPricing`/`unpricedModels`/stderr）；兜底表「三处无条件 insert」在 §8(2720)、§12.3-2(3344)、§13.2 三处行号完全相同（`pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408`），且我已在源码复现。

## 装配问题

- **[A1] 编号断裂（§9）**：2736 行直接以 `### 1.1` 起，1.1–1.7 七个小节**缺 `### 1.` 父标题**；而 4/5/6 组均有父标题（`### 4. 输出渲染` 等）。
- **[A2] 编号体系错位（§10）**：§10 的子节沿用分片旧编号 `7.1`–`7.8`（3128–3266 行），与章节号 10 不符；同一问题使 §9 内的「见 §3」（2828 行）、「见 §6」（2936 行）与全文 §3（统一数据模型）、§6（其余 16 个适配器）撞号，读者按编号跳转会落到无关章节。
- **[A3] 标题前缺空行（Markdown 粘连）**：3292→3293（§10 末项紧贴 `## 11.`）、3355→3356（§12.4 紧贴 `## 13.`）、3405→3406（§13.4 紧贴 `## 附录 A`）；另 §9 标题下多出连续空行（2734–2735）。
- **[A4] 分片拼接遗留**：§10 末尾的 `### 未验证/存疑`（3282 行）混装 §9/§11 范围的未决项（第 1 条 `--jq` 帮助文本、第 2 条 `docs/guide/config-files.md`），不属于「版本史」。
- **[A5] 交叉引用样式**：§13.2 被 §12.2 以「(§13.2 的 E2)」引用，但该节无编号锚点；建议给 §13.2 各条补 `(E1…E6)` 编号（或统一改引 `REVIEW-R*.md`）。
- 未发现半截段落、表格列数错位或引用指向不存在文件的其它情况；`docs/public/config-schema.json` 在仓库中不存在属正常（构建期由 `docs/package.json:8` 拷贝生成）。

## 抽查清单

| 编号 | 章节 | 断言 | 源码位置 | 结果 | 摘录 |
| --- | --- | --- | --- | --- | --- |
| 1 | 附录 A | `DEFAULT_SESSION_DURATION_HOURS`=5.0 / `DEFAULT_RECENT_DAYS`=3 / `USAGE_COMPACT_WIDTH_THRESHOLD`=100 | `core/src/lib.rs:59-61` | 通过 | `pub const DEFAULT_SESSION_DURATION_HOURS: f64 = 5.0;` |
| 2 | 附录 A | `BUILT_IN_AGENT_NAMES` 18 项且顺序与文档一致 | `core/src/lib.rs:63-82` | 通过 | `"claude","codex","opencode","amp"…"grok","zcode"` 共 18 |
| 3 | 附录 A | `BLOCKS_WARNING_THRESHOLD`=0.8 / `BLOCKS_COMPACT_WIDTH_THRESHOLD`=120 | `ccusage/src/blocks.rs:18,20` | 通过 | `const BLOCKS_WARNING_THRESHOLD: f64 = 0.8;` |
| 4 | 附录 A | `MTIME_PRUNE_MARGIN_MS` = 1 天 | `adapters/claude/src/paths.rs:84` | 通过 | `const MTIME_PRUNE_MARGIN_MS: i64 = MILLIS_PER_DAY;` |
| 5 | 附录 A | `CACHE_CREATE_1H_INPUT_MULTIPLIER`=2.0 | `core/src/cost.rs:7` | 通过 | `const CACHE_CREATE_1H_INPUT_MULTIPLIER: f64 = 2.0;` |
| 6 | 附录 A | `DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS`=200 000 | `core/src/pricing.rs:220` | 通过 | `const DEFAULT_LONG_CONTEXT_THRESHOLD_TOKENS: u64 = 200_000;` |
| 7 | 附录 A | `MODEL_DATE_SUFFIX_DIGITS`=8 / `MODELS_DEV_FAILURE_RETRY_AFTER`=60s | `pricing.rs:61,58` | 通过 | `const MODEL_DATE_SUFFIX_DIGITS: usize = 8;` |
| 8 | 附录 A | `MODELS_DEV_TRUST_*` = 3/2/1 | `pricing.rs:649-651` | 通过 | `TRUST_OWNER: u8 = 3;` … `RESELLER: u8 = 1;` |
| 9 | 附录 A | `DEEPSEEK_V4_PRICING_CUTOFF_MS` = 1 786 896 000 000 = 2026-08-16T16:00Z | `pricing.rs:252` | 通过 | `const DEEPSEEK_V4_PRICING_CUTOFF_MS: i64 = 1_786_896_000_000;`（换算吻合） |
| 10 | 附录 A | `CODEX_REWRITTEN_BURST_PAUSE_MS`=1 000 | `codex/src/parser.rs:122` | 通过 | `const CODEX_REWRITTEN_BURST_PAUSE_MS: i64 = 1_000;` |
| 11 | 附录 A | 超时 10s / 上限 64 MiB / ETag 4096 | `ccusage/src/http.rs:11-13` | 通过 | `PRICING_FETCH_TIMEOUT_SECONDS: u64 = 10;` / `64 * 1024 * 1024` / `4096` |
| 12 | 附录 A | 缓存价推导 `×1.25` / `×0.1` | `pricing.rs:1085-1088` | 通过 | `.unwrap_or(input * 1.25)` / `unwrap_or(input * 0.1)` |
| 13 | 附录 A | 缺价门槛 `total_tokens == 0` 不报 | `core/src/cost.rs:66-68` | 通过 | `if total_tokens == 0 { return None; }` |
| 14 | 附录 A | `LOG_LEVEL` 0–5「默认 3」 | `core/src/logger.rs:11-15` + `docs/guide/environment-variables.md:72` | 注意 | 源码仅 `Option<u8>`，无默认级别常量；「默认 3」只在文档表格（行 72 标 default）。建议在「位置」列注明默认值仅文档声明 |
| 15 | 附录 B | `ccusage` crate 5 文件行数（1063/1411/618/768/51） | `rust/crates/ccusage/src/*` | 通过 | 逐项与 `wc`（read 口径）完全一致 |
| 16 | 附录 B | `ccusage-core` 8 文件行数（6181/975/759/664/495/219/150/105） | `rust/crates/ccusage-core/src/*` | 通过 | 全部一致 |
| 17 | 附录 B | `cli-parser`（1247/378/104）、`config`（1404/1639）、`terminal`（1337/194）、`adapter-all`（1637/620/208）、`common`（410/169/384） | 对应路径 | 通过 | 15 项全部一致 |
| 18 | 附录 B | 18 家 adapter 共 40 个文件行数 | `rust/adapters/<agent>/src/*.rs` | 通过 | 40 项全部一致（含 codex 9 文件、opencode 2922 等） |
| 19 | §9 1.2 | 查找顺序 = `$CWD/.ccusage/ccusage.json` → `CLAUDE_CONFIG_DIR*` 或 `~/.config/claude` → `~/.claude`，取首个可用 | `config.rs:258-269`、`:271-283`、`:246-256` | 通过 | `paths.push(cwd.join(".ccusage").join("ccusage.json"));` … `.find(\|value\| value.as_object().is_some())` |
| 20 | §9 1.2 | 无 `CCUSAGE_CONFIG`；源码读取的 `CCUSAGE_*` 仅 3 个 | 全树 grep | 通过 | 21 个 `_ENV` 常量 + 13 个字面量，去重后 33 名 + `OUT_DIR` |
| 21 | §9 1.3 | `SharedOptions` 19 键与表列一致（`config_schema.rs:358-399`） | `config_schema.rs:360-399` | 通过 | `since/until/json/mode/debug/debug_samples/order/breakdown/offline/no_offline/…/pricing_overrides` |
| 22 | §9 1.3 | `pi.stores[]`（name+path）@ `config_schema.rs:183-209`；`PiStoreConfig` 正则 | `config_schema.rs:183-198` | 通过 | `pub stores: Option<Vec<PiStoreConfig>>` + `regex(pattern = "^[a-z][a-z0-9_-]{0,31}$")` |
| 23 | §9 1.4 | `option_maps` 覆盖顺序 6 级 | `config.rs:92-132` | 通过 | `defaults → commands.<raw> → commands.<report> → commands."<agent>:<report>" → <agent>.defaults → <agent>.commands.<report>` |
| 24 | §9 1.5 | 配置日期错误文案与 `Invalid ccusage config:` 包装 | `config.rs:69-90`、`:198-200` | 通过 | `"{key} must be a string. Expected {DATE_BOUND_FORMATS}"` |
| 25 | §9 1.6 | 生成入口 `generate_config_schema_json`（711-788）用 tab 缩进 | `config_schema.rs:711`、`apps/ccusage/config-schema.json:2` | 通过 | 生成物 14415 行、第 2 行以 tab 起始 |
| 26 | §9 1.7 | statusline 默认值（offline/cache/1s/50/80/off/auto） | `ccusage-cli/src/types.rs:221-240` | 通过 | `offline: true, cache: true, refresh_interval: 1, context_low_threshold: 50` |
| 27 | §9 2.2 | 共享选项表 19 项逐行号 | `parser.rs:745-776` | 通过 | `"-j" \| "--json" => shared.json = true,` 等全部命中 |
| 28 | §9 2.2 | agent 报告集合差异（claude 6/opencode 4/其余 15 家 3） | `parser.rs:973-987` | 通过 | `"codex" => matches!(report, "daily" \| "monthly" \| "session")` |
| 29 | §9 2.4 | `find_help_page` @ `help.rs:59-68`；`--jq` 不在帮助文本 | `help.rs:59-68`；grep `jq` | 通过 | `HELP_PAGES.iter().find(…page.path.len() == tokens.len()…)`；`cli-help.json` 零命中 |
| 30 | §9 3 | `--json` 与 `--jq` 不互斥、jq 优先 | `core/src/output.rs:14-16`、`:158-187` | 通过 | `shared.json \|\| shared.jq.is_some()`；`if let Some(filter) = jq` 分支在前 |
| 31 | §9 3 | `--last`/`session --id` 冲突文案与行号 | `parser.rs:1107-1115`、`:454-460` | 通过 | `"The --last option cannot be combined with --since or --until."` |
| 32 | §9 4.1 | `summary_json`/`session_summary_json`/`totals_json` 键名与条件插入 | `output.rs:27-56`、`:58-77`、`:79-121` | 通过 | `obj.insert("credits"…)` 仅 `row.credits` 有值时 |
| 33 | §9 4.1 | `totals.unpricedModels` 排序去重、有价省略键 | `output.rs:126-145`、`docs/guide/json-output.md:36-37` | 通过 | `BTreeSet` 去重；`Each affected entry … carries "missingPricing": true` |
| 34 | §9 4.1 | **`byAgent` 容器键** | `adapter-all/src/report.rs:136-143,148-156` | 错误 | 实际键名 `"agents"`，插入点 148-156；136-143 为 `period`/`metadata`（见 [E1]） |
| 35 | §9 4.2 | jq 为外部二进制、失败文案、`--no-cost` 先剥离 | `output.rs:159-187`、`:564-581` | 通过 | `Command::new("jq")`；`failed to run jq: {error}`；`strip_cost_json` 在 spawn 之前 |
| 36 | §9 4.4 | spinner 10 帧/80ms；`LOG_LEVEL=0` 抑制；单例复用 | `progress.rs:11-12`、`:295-296`、`:132-141` | 通过 | `let enabled = crate::log_level() != Some(0) && should_show…` |
| 37 | §9 4.5 | 紧凑条件、空结果 stderr、缺价告警区分离线 | `output.rs:18-25`、`:233-236`、`:430-450` | 通过 | `WARN  Missing embedded pricing for {model}; … Run without --offline …` |
| 38 | §9 6.1 | 只认两个 URL；`for_url` 白名单；形状校验 | `pricing.rs:55-57`、`:70-78`、`:80-126` | 通过 | `LITELLM_PRICING_URL`/`MODELS_DEV_API_URL`；`_ => None` |
| 39 | §9 6.2 | 缓存键 FNV-1a；etag ASCII+无控制字符；原子 rename | `ccusage/src/http.rs:236-257`、`:186-206`、`:199-229` | 通过 | `hash = hash.wrapping_mul(0x0100_0000_01b3)`；`One rename so concurrent readers never observe a mixed pair` |
| 40 | §9 6.3 | 版本注入与一致性测试；Nix 变量；内嵌白名单 | `ccusage/build.rs:5-18`、`main.rs:125-135`、`package.nix:46-47`、`cargo-artifacts.nix:98`、`core/build.rs:203-221` | 通过 | `CCUSAGE_VERSION = version;`；白名单含 `zai/`、`openrouter/openai/` |
| 41 | §10 7.2 | v19 无 `--last`/`--jq`/`--no-cost`；日期仅 YYYYMMDD | v19 `apps/**/*.ts`（grep）+ `shared-args.ts:19-30` | 通过 | grep 零命中；`description: 'Filter from date (YYYYMMDD format)'` |
| 42 | §10 7.3 | v19 `createUniqueHash`；替换规则；应用点 2075-2123 | v19 `adapter/claude/data-loader.ts:1194-1202`、`:1629-1638`、`:2085-2097` | 通过 | `return requestId == null ? messageId : \`${messageId}:${requestId}\`` |
| 43 | §10 7.3 | v20 三档键 + `msg_` 判定 + 守卫 | `claude/src/lib.rs:350-375`、`:413-440` | 通过 | `if request_id.is_none() { session_id.hash(…); if !is_anthropic_message_id(…) { timestamp.hash(…) } }` |
| 44 | §10 7.4 | v19 优先 `lastUsage`、累计差分回退；v20 同主路径 | v19 `codex/parser.ts:205-212`；`codex/src/parser.rs:483-496` | 通过 | `let raw = parsed.lastUsage; if (raw == null && parsed.totalUsage != null) …` |
| 45 | §10 7.4 | v20 回退减法含 cache_creation 档；replay 状态机；去重/扣减测试 | `codex/src/parser.rs:1284-1308`、`:254-257`、`loader.rs:240/552/562/615/943` | 通过 | `cache_creation_tokens: current.…saturating_sub(previous…)` |
| 46 | §10 7.5 | v19 blocks 常量/整点对齐/双条件/gap/活跃判定；测试常量 373 | v19 `session-blocks.ts:7,135,146,157-162,202,373` | 通过 | `if (timeSinceBlockStart > sessionDurationMs \|\| timeSinceLastEntry > sessionDurationMs)` |
| 47 | §10 7.5 | v20 同结构 + `isGap`；调用点命令层固定 5h | `ccusage/src/blocks.rs:53-107`、`:211`、`commands/mod.rs:573` | 通过 | `floor_to_hour` → `div_euclid(MILLIS_PER_HOUR)`（UTC 整点） |
| 48 | §10 7.6 | v19 前缀候选 + 双向 includes 最短长度差 | v19 `packages/internal/src/pricing.ts:245-288` | 通过 | `if (!comparison.includes(lower) && !lower.includes(comparison)) continue;` |
| 49 | §10 7.6 | v20 `find`/`find_exact`/`find_exact_with_fallback` + 4 条 fuzzy 测试 | `pricing.rs:1354`、`:1484`、`:1501`、`:5663/5706/5735/5854` | 通过 | 测试名与行号逐一命中 |
| 50 | §10 7.7 | v19 三态语义与立即成本；v20 match 与缺价标记 | v19 `data-loader.ts:1284-1336`；`core/src/cost.rs:31-59` | 通过 | `CostMode::Display => cost_usd.unwrap_or(0.0)` |
| 51 | §10 7.8 | 保留的 TS 兼容测试名与行号（5 条） | `main.rs:150/212/483`、`claude/src/lib.rs:1025/1041` | 通过 | `fn formats_windows_user_project_paths_like_typescript()` |
| 52 | §11-1 | blocks 文档称 desc 为默认 | `docs/guide/blocks-reports.md:150`；`ccusage-cli/src/types.rs:251-254` | 通过 | 文档 `# Show newest blocks first (default)`；源码 `pub enum SortOrder { Desc, #[default] Asc }` |
| 53 | §11-2 | weekly 文档 `modelBreakdowns` 为对象 | `docs/guide/weekly-reports.md:138-149`；`core/src/types.rs:175` | 通过 | `"modelBreakdowns": { "claude-opus-4-1-…": {…} }` vs `Vec<ModelBreakdown>` |
| 54 | §11-3 | codex 文档自身矛盾（:50 skipped vs :54 计入） | `docs/guide/codex/index.md:50,54,116`；`codex/src/parser.rs:813-818` | 通过 | `*current_model = Some("gpt-5".to_string());` |
| 55 | §11-4 | codex README 三个符号未实现 | rust 全树 grep + `codex/src/parser.rs:236,122` | 通过 | 三符号只命中 `README.md:50-51`；实现为 `if prefix.get(index) == Some(&usage)` |
| 56 | §11-5/6/7 | `CCUSAGE_OFFLINE`/`CCUSAGE_TIMEZONE`/`CCUSAGE_GROK_PATH` 无实现 | `docs/guide/environment-variables.md:124/129/209`、`configuration.md:38`、grok 设计文档 | 通过 | 环境变量文 :124 `### CCUSAGE_OFFLINE`；配置文 :38 `export CCUSAGE_TIMEZONE="Asia/Tokyo"` |
| 57 | §11-9/10 | cost-modes 公式与 codex 缓存读价回退「一致」 | `docs/guide/cost-modes.md:203`；`core/src/cost.rs:128-131`；`codex/src/report.rs:344-348` | 通过 | `cacheCreate1hTokens * inputPrice * 2`；`if pricing.cache_read_explicit { … } else { pricing.input }` |
| 58 | §12.1-1 | TokenScope 单缓存写槽位 | `src/source/claude.rs:58-68,364`、`src/pricing.rs:200-210` | 通过 | `struct ClaudeUsage { … cache_creation_input_tokens: u64 … }`；`pub struct PriceRates { input, output, cache_write, cache_read }` |
| 59 | §12.1-2 | TokenScope 忽略 `token_usage_record` | `src/source/codex.rs:4-6,314` | 通过 | `"token_usage_record" => stats.ignored_token_usage_record += 1,` |
| 60 | §12.1-3 | TokenScope 单键 `(session_id, record_id)` 保留最晚 | `src/dedupe.rs:9,26-42` | 通过 | `let key = (e.session_id.clone(), e.record_id.clone());` |
| 61 | §12.1-4 | TokenScope 候选取最高 | `src/pricing.rs:2304-2415` | 通过 | `禁止跨候选拼价：四类单价全部来自最终选中的同一个候选。` |
| 62 | §12.1-5 | `Pricing::default()` 为空表 | `src/pricing.rs:1737-1750` | 通过 | `Task 1：默认空表——没有显式来源时不会偷偷给价（不变量 1）。` |
| 63 | §12.1-6/7 | blocks 未实现；指纹缓存 + 双取指纹 | `src/report.rs:1262-1323` | 通过 | `let fingerprint = size.zip(mt).map(\|(s, m)\| format!("{s}:{m}"));` |
| 64 | §12.2-3 | 数字版本后缀保护；TokenScope 边界等价 | `pricing.rs:2703-2722`；`src/model_identity.rs:141-157` | 通过 | `!(digit_len == MODEL_DATE_SUFFIX_DIGITS && after_digits.is_none_or(is_pricing_key_boundary))` |
| 65 | §12.2-6 | 大小贪心分块「30 行」 | `adapters/common/src/lib.rs:49-81` | 注意 | 实为 33 行（见 [M4]） |
| 66 | §12.3-1 | daily 缺 timestamp 守卫 | `claude/src/daily.rs:493-496` vs `lib.rs:436-440` | 通过 | daily：`(existing.message_id … == Some(message_id) && indexed_session_id == … && (candidate_is_sidechain \|\| is_sidechain_daily_entry(existing)))`；主路径：`(requestless \|\| entry.timestamp == timestamp)` |
| 67 | §12.3-2 | 兜底表 3 处无条件 `insert` | `pricing.rs:2369-2370`、`:2386-2387`、`:2400-2408` | 通过 | `self.entries.insert("gpt-5.1-codex".to_string(), gpt_5_1_pricing);` |
| 68 | §13.1 | 四轮复核抽样数 71/80/58/43 与「25 处必须修正」 | `ccusage-draft/REVIEW-R{1..4}.md` | 通过 | 表格数据行 71/80/58/43；必须修正条目 6+7+6+6 = 25 |
| 69 | §13.1 | 「6 份分片合计约 33 万字符」 | `ccusage-draft/S{1..6}*.md` | 错误 | 字符 250 153、字节 331 827（见 [E4]） |
| 70 | §13.2 | R4/S5 分歧裁定 `prepare_session_message_query` 在 752 | `opencode/src/loader.rs:752`；`REVIEW-R4.md:67` | 通过 | `fn prepare_session_message_query(` 恰在 752；文档裁定正确、R4 的 E5 作废 |
| 71 | §13.2 | `apply_total_token_fallback` 12 家/14 处 | 全树 grep | 通过 | 14 处调用、12 家（含 amp/kimi 各 2 处） |
| 72 | §13.3-6 | `models-dev-pricing.json` 34 269 行 | `core/src/models-dev-pricing.json` | 通过 | 34 269 |
| 73 | 附录 A（补充） | 白名单含 `openrouter/openai/` | `core/build.rs:220` | 通过 | `\|\| model.starts_with("openrouter/openai/")` |

> 附录 A 的 21 行已全量核对（第 1–14 条覆盖，其中第 14 条为口径保留项）；附录 B 的 64 个文件行数已全量比对（第 15–18 条），无一处偏差。

## 无法验证

1. **`--jq` 是否「故意隐藏」**：能证实 `cli-help.json` 无该条目且选项可解析（`parser.rs:771`），但设计意图无法从源码判定（§10 未验证第 1 条自陈相同）。
2. **内嵌 LiteLLM 快照的真实键集**：`$OUT_DIR/litellm-pricing.json.deflate` 为编译产物，本轮遵守「只读、不构建」未生成。
3. **运行期行为**：`format_currency` 舍入边界、`models.dev` 实时响应与内嵌快照差异、`find_cache` 并发语义、`CODEX_REWRITTEN_BURST_PAUSE_MS=1000` 的实测依据（注释称 burst 10–40 ms、后继停顿 5.8–15.3 s）——均需运行或外部数据。
4. **`docs/guide/config-files.md`（614 行）的「多文件回退」表述**：该文未逐行读完，只确认 §9.1.2 的实现事实（取首个可用文件）。
5. **v19「全仓无 `--last`/`--jq`」的「全仓」边界**：本轮覆盖 `apps/` 与 `packages/` 的 `.ts`；未覆盖 `docs/`、`scripts/` 与 vitest 内联快照文本。
6. **§13.2 第 3 条「证据错位」三例的具体错位对象**（`gpt-5.4` vs `gpt-5.4-mini` 等）：属 S3/S4 分片历史问题的复述，本轮未回到当时的错误文本比对。

## 我实际打开过的文件

ccusage（HEAD 57afdc2）：

- `rust/crates/ccusage-core/src/lib.rs`、`cost.rs`、`pricing.rs`、`output.rs`、`logger.rs`、`progress.rs`、`date_utils.rs`、`types.rs`、`utils.rs`、`agent_report.rs`、`build.rs`
- `rust/crates/ccusage/src/blocks.rs`、`http.rs`、`cli.rs`、`main.rs`
- `rust/crates/ccusage-cli/src/types.rs`
- `rust/crates/ccusage-cli-parser/src/parser.rs`、`help.rs`、`help_codegen.rs`、`cli-help.json`
- `rust/crates/ccusage-config/src/config.rs`、`config_schema.rs`、`lib.rs`、`bin/generate_config_schema.rs`、`Cargo.toml`
- `rust/crates/ccusage-adapter-all/src/report.rs`
- `rust/adapters/claude/src/lib.rs`、`daily.rs`、`paths.rs`
- `rust/adapters/codex/src/parser.rs`、`report.rs`、`README.md`
- `rust/adapters/opencode/src/loader.rs`、`rust/adapters/common/src/lib.rs`、`rust/adapters/grok/src/paths.rs`
- `docs/guide/{environment-variables,configuration,blocks-reports,weekly-reports,cost-modes,json-output}.md`、`docs/guide/codex/index.md`
- `ccusage.example.json`、`apps/ccusage/config-schema.json`、`apps/ccusage/justfile`、`docs/package.json`、`package.nix`、`nix/{cargo-artifacts,checks,treefmt}.nix`

ccusage-v19（4aedac0）：

- `apps/ccusage/src/adapter/claude/data-loader.ts`、`apps/ccusage/src/adapter/codex/parser.ts`、`apps/ccusage/src/adapter/codex/types.ts`
- `apps/ccusage/src/session-blocks.ts`、`shared-args.ts`、`config-loader-tokens.ts`、`consts.ts`、`calculate-cost.ts`
- `packages/internal/src/pricing.ts`

TokenScope：

- `src/source/claude.rs`、`src/source/codex.rs`、`src/dedupe.rs`、`src/pricing.rs`、`src/model_identity.rs`、`src/report.rs`、`Cargo.toml`

报告与分片（核对装配一致性）：

- `docs/research/ccusage-draft/REVIEW-R1.md`、`REVIEW-R2.md`、`REVIEW-R3.md`、`REVIEW-R4.md`
- `docs/research/ccusage-draft/S1-main-and-claude.md` … `S6-config-output-history.md`（体积统计）
