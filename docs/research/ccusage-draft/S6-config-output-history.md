# S6 配置 / CLI / 输出 / 版本史

研究用只读样本（**禁止修改**）：

- 主仓（Rust，v20.0.31）：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（`chore: release v20.0.31`）。
- 对照仓（TypeScript，v19.0.3）：`%TEMP%\ccusage-research\ccusage-v19`，HEAD `4aedac0622232f4b8e93650e9b7b54535ef9cf2b`，tag `v19.0.3`。

行号约定：Rust 侧证据写成 `path:line`（相对主仓根）；v19 侧证据写成 `ccusage-v19/path:line`。下文所有行号均为实际行号（read/grep 实测）。
**本章裸文件名约定**：未写全路径的 `parser.rs`/`help.rs`/`help_codegen.rs`/`arg_parser.rs` 指 `rust/crates/ccusage-cli-parser/src/`；`config.rs`/`config_schema.rs` 指 `rust/crates/ccusage-config/src/`；`output.rs`/`types.rs`（core 语境）/`agent_report.rs` 指 `rust/crates/ccusage-core/src/`；`http.rs`/`cli.rs`/`commands/mod.rs` 指 `rust/crates/ccusage/src/`；`types.rs`（CLI 语境）指 `rust/crates/ccusage-cli/src/types.rs`。

---

## 1. 配置文件体系

### 1.1 交付物与命名

v20 的配置文件仍叫 `ccusage.json`，但**由 Rust 生成 JSON Schema**，且 TS 侧在 v20 只剩下发 npm 包的分发壳。

- Schema 文件：`apps/ccusage/config-schema.json`（14415 行，仓库内提交）。
- 分发副本：`docs/public/config-schema.json`（文档站构建前从上面那份拷贝）。

证据：`docs/package.json:8`（`cp ../apps/ccusage/config-schema.json public/config-schema.json`）；`docs/README.md:13-14`；`nix/treefmt.nix:57-61`（treefmt 会把生成结果同时刷到两处）；一致性由 `nix/checks.nix:90-111` 强制（生成结果与提交文件 diff 不一致即失败，报 "apps/ccusage/config-schema.json is out of sync with the Rust schema source."）。

示例配置：`ccusage.example.json:1-39`，顶层是 `$schema` / `defaults` / `commands`，schema 指向 `./apps/ccusage/config-schema.json`（`ccusage.example.json:2`）。`nix/checks.nix:177` 用 `check-jsonschema` 校验该示例。

### 1.2 查找路径与优先级

查找路径由 `discover_config_paths()` 生成，顺序即优先级，**第一个能被读取且能解析成 JSON object 的文件获胜**（不做多文件合并）。

```rust
// rust/crates/ccusage-config/src/config.rs:258-269
fn discover_config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        paths.push(cwd.join(".ccusage").join("ccusage.json"));
    }
    paths.extend(claude_config_dirs().into_iter().map(|dir| dir.join("ccusage.json")));
    paths
}
```

用户级目录来自 `claude_config_dirs()`：有 `CLAUDE_CONFIG_DIR` 时按逗号切分（每个都 trim、丢空项），否则退回 `~/.config/claude` 与 `~/.claude` 两个目录。

- 证据：`rust/crates/ccusage-config/src/config.rs:271-283`。
- 因此完整顺序是：`$CWD/.ccusage/ccusage.json` → (`$CLAUDE_CONFIG_DIR/*/ccusage.json` 或 `~/.config/claude/ccusage.json` → `~/.claude/ccusage.json`)。
- 取首个可用者：`rust/crates/ccusage-config/src/config.rs:246-256`（`read_to_string` + `serde_json::from_str` 双双失败的会被静默跳过，最后 `.find(|value| value.as_object().is_some())`）。

显式路径：`--config <path>` / `--config=<path>`。它在参数扫描阶段被取出，不参与普通选项解析：

- 证据：`rust/crates/ccusage-config/src/config.rs:285-299`（`scan_config_path`，同时支持 `=` 与分离写法，空值不认）。

**没有 `CCUSAGE_CONFIG` 环境变量**。源码层面（`env::var` / `env::var_os` 读取）`CCUSAGE_*` 只有三个：`CCUSAGE_VERSION`、`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_MODEL_ALIASES`。文档层面另有一批**两版源码均无实现**的变量名：`CCUSAGE_OFFLINE`（`docs/guide/environment-variables.md:124`、`:129`、`:209`）、`CCUSAGE_TIMEZONE`（`docs/guide/configuration.md:38`）、`CCUSAGE_GROK_PATH`（`docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24`、`:51`）——最后这个只是设计文档里被点名要移除的旧路径来源，同一文档明确要求只保留 `GROK_HOME` 与 `~/.grok`（该文 `:35`、`:51-52`、`:59-60`、`:83`），v20 实际实现即 `rust/adapters/grok/src/paths.rs:9` 的 `GROK_HOME`。也没有 XDG 参与**配置查找**：`config.rs` 不读 `XDG_CONFIG_HOME`。XDG 变量在别处出现三处：Claude 数据目录发现（`rust/adapters/claude/src/paths.rs:46`，`XDG_CONFIG_HOME`）、HTTP 缓存根（`rust/crates/ccusage/src/http.rs:39`，`XDG_CACHE_HOME`）与 OpenCode 数据根（`rust/adapters/opencode/src/paths.rs:6`、`:29`，`XDG_DATA_HOME`）。

文档侧口径与实现一致但有冗余描述（把「用户配置/遗留配置」拆成两条优先级）：`docs/guide/configuration.md:16-24`。

### 1.3 配置结构（有哪些设置）

`CcusageConfig` 顶层字段：`$schema`、`defaults`、`commands`，外加每个 agent 一个命名空间（`claude`、`codex`、`opencode`、`amp`、`droid`、`codebuff`、`hermes`、`pi`、`goose`、`openclaw`、`kilo`、`copilot`、`gemini`、`antigravity`、`kimi`、`qwen`、`grok`、`zcode`），全部 `Option`，全部 camelCase。

证据：`rust/crates/ccusage-config/src/config_schema.rs:13-59`（`#[serde(rename_all = "camelCase")]` 在第 14 行）；每个 agent 的 `{ defaults, commands }` 形状见 `config_schema.rs:72-359`；根 `commands` 只允许 `daily/weekly/monthly/session/blocks/statusline`（`config_schema.rs:61-70`）。

各类设置的落点：

| 类别 | 配置键 | 作用对象 | 证据 |
| --- | --- | --- | --- |
| 日期与输出通用项 | `since` `until` `json` `mode` `order` `breakdown` `offline` `noOffline` `color` `noColor` `timezone` `jq` `compact` `singleThread` `noCost` `debug` `debugSamples` `all` `pricingOverrides` | 所有报告 | `config_schema.rs:358-399` |
| 日报专有 | `instances` `project` `projectAliases` | `daily` / `claude daily` | `config_schema.rs:425-434` |
| 周报专有 | `startOfWeek` | `weekly` | `config_schema.rs:445-450` |
| blocks 专有 | `active` `recent` `tokenLimit` `sessionLength` | `blocks` | `config_schema.rs:461-472` |
| statusline 专有 | `offline` `noOffline` `visualBurnRate` `costSource` `cache` `noCache` `refreshInterval` `contextLowThreshold` `contextMediumThreshold` `timezone` `debug` `modelLabelAliases` `pricingOverrides` | `statusline` | `config_schema.rs:481-510` |
| Codex 专有 | `speed` | `codex` | `config_schema.rs:512-519` |
| pi 专有 | `piPath`、`pi.stores[]`（`name`+`path`） | `pi` | `config_schema.rs:521-528`、`config_schema.rs:183-209` |
| OpenClaw 专有 | `openClawPath` | `openclaw` | `config_schema.rs:530-537` |
| 自定义价格 | `pricingOverrides.<model>.*`（11 个 cost 字段） | 全局 / statusline | `config_schema.rs:593-607` |
| 枚举取值 | `mode`=auto/calculate/display；`order`=desc/asc；`startOfWeek`=sunday..saturday；`speed`=auto/standard/fast/flex；`visualBurnRate`=off/emoji/text/emoji-text（kebab-case）；`costSource`=auto/ccusage/cc/both | — | `config_schema.rs:539-591` |

没有独立的「默认 agent 选择」键：agent 由命令位置决定（`ccusage codex daily`），配置只提供各 agent 段的默认值。

### 1.4 配置应用顺序（覆盖规则）

`option_maps()` 按固定顺序收集「要应用的 option map」，后收集的覆盖先收集的：

```
根 defaults
→ commands.<原始命令>（如 commands.daily / commands."codex daily"）
→ commands.<report>（仅当命令带 agent，如 commands.daily）
→ commands."<agent>:<report>"（如 commands."codex:daily"）
→ <agent>.defaults
→ <agent>.commands.<report>
```

证据：`rust/crates/ccusage-config/src/config.rs:92-132`；命令名拆解（`codex:daily` 与 `codex daily` 两种写法归一）见 `config.rs:301-334`，其中 `is_agent_command`（`config.rs:395-397`）与 `is_report_command`（`config.rs:399-404`）。

应用进具体参数结构：`apply_config_to_shared`（`config.rs:406-413`）、`apply_config_to_daily_args`（`config.rs:415-428`）、`weekly`（`config.rs:430-436`）、`blocks`（`config.rs:438-454`）、`statusline`（`config.rs:456-505`）、agent 级（`config.rs:507-529`，目前只处理 `speed` / `piPath` / `openClawPath`）。对外接口是 `cli` 侧定义的 trait：

- `rust/crates/ccusage-cli/src/types.rs:299-321`（`CliConfig` 六个钩子 + `config_error` 默认 `None`）；`NoConfig` 空实现 `types.rs:323-325`。

CLI 侧只对「有该字段的键」赋值，所以配置文件永远不能覆盖默认值以外的语义冲突：例如 `order` 一旦来自配置就会把 `order_explicit` 置真（`config.rs:588-591`），这与 CLI 的同名行为一致（`rust/crates/ccusage-cli-parser/src/parser.rs:761-764`）。

`pricingOverrides` 是**逐字段合并**而非整条替换：`merge_pricing_overrides`（`config.rs:627-635`）与 `merge_override_fields`（`config.rs:637-681`）只覆盖 source 里 `Some` 的字段；`ConfigPricingOverride → PricingOverride` 的字段全量映射在 `config.rs:683-701`。

### 1.5 配置文件里的取值校验

配置绕过了 CLI 解析器，因此校验被单独实现一次：

- 日期：`ConfigContext::detect_date_bound_error`（`config.rs:69-90`），非字符串报 `since must be a string. Expected YYYY-MM-DD or YYYYMMDD`，非法日期报 `since 'x' is not a valid date. Expected YYYY-MM-DD or YYYYMMDD`；两者都被 `config_error()` 包成 `Invalid ccusage config: ...`（`config.rs:198-200`），并在解析早期直接 `return Err`（`parser.rs:90-92`）。
- 时区：配置里的 `timezone` 与 `--timezone` 走同一条校验（见本章第 3 节，`rust/crates/ccusage/src/cli/timezone.rs:130-150` 有专门测试「配置时区被合法 flag 覆盖后放行」）。
- `pi.stores`：数组形状、`name`/`path` 必填、`path` 非空、name 匹配 `^[a-z][a-z0-9_-]{0,31}$`（`config_schema.rs:11`）、不得与内置 agent 名冲突、不得重名；错误以 `Invalid ccusage config: pi.stores[i] ...` 形式暴露且**只对会读 pi stores 的命令生效**（`daily/weekly/monthly/session`，`config.rs:134-141`、`config.rs:143-196`、`config.rs:57-63`）。

### 1.6 JSON Schema 的生成方式

Rust 侧用 `schemars` 从结构体派生，再手工增强：

- 派生入口：`CcusageConfig` 上的 `#[derive(JsonSchema)]`（`config_schema.rs:13`），其余结构同（例如 `config_schema.rs:358-360`）。
- 生成函数：`generate_config_schema_json()`（`config_schema.rs:711-788`），期间调用 `enrich_schema`（`config_schema.rs:856-878`）、`add_schema_defaults`（`config_schema.rs:880-920`，含 `set_definition_defaults` `config_schema.rs:922-939`）、`inline_schema_references`（`config_schema.rs:941-1020`，`inline_schema_value`/`inline_ref`/`inline_all_of`/`merge_schema_object`）、`wrap_root_schema`（`config_schema.rs:1021-1047`）。
- 缩进用 **tab**：`tab_indent_json`（`config_schema.rs:789-804`），与仓库 `.editorconfig`/示例文件一致。
- 生成可执行体：`rust/crates/ccusage-config/src/bin/generate_config_schema.rs:3-15`（无参写 stdout，带参写文件，失败 `exit(1)`）。
- 提交产物的刷新命令：`apps/ccusage/justfile:25-27`（`cargo run -p ccusage-config --bin generate-config-schema -- config-schema.json` → `oxfmt --write` → 拷到 `docs/public/`）。
- Cargo 目标声明：`rust/crates/ccusage-config/Cargo.toml:11`；库再导出：`rust/crates/ccusage-config/src/lib.rs:10`。
- Nix 侧有 `nix run .#generate-schema` 与 treefmt 钩子（`nix/treefmt.nix:57-61`、`nix/treefmt.nix:221`）。

配置文件字段的运行时读取**不**依赖 schema：`SharedOptions::from_map` 等一组手写取值器按 camelCase 键名逐字段取（`config_schema.rs:609-634`、`635-644`、`645-652`、`653-663`、`664-683`、`684-692`、`693-701`、`702-710`），底层用 `string_option` / `bool_option` / `usize_option` / `u64_option` / `f64_option` / `enum_option` / `pricing_override_map_option` / `hashmap_option`（`config_schema.rs:805-855`）。也就是说：**schema 只是给编辑器/CI 用，运行时不做严格类型拒绝**，非法类型会被静默忽略（例如 `instances: "yes"` 等于没写）。

### 1.7 内置默认值一览

配置字段几乎都是 `Option`，真正的默认值定义在 CLI 侧：

| 参数 | 默认值 | 证据 |
| --- | --- | --- |
| `mode` | `auto` | `rust/crates/ccusage-cli/src/types.rs:62-71` |
| `debug_samples` | `5` | 同上 |
| `order` | `asc` | 同上 |
| `since/until/last/json/breakdown/offline/color/jq/compact/single_thread/no_cost/timezone` | 全部空/false/None | `types.rs:34-60` |
| `blocks.session_length` | `5.0` 小时 | `rust/crates/ccusage-core/src/lib.rs:59`（`DEFAULT_SESSION_DURATION_HOURS`），注入点 `rust/crates/ccusage/src/cli.rs:19-24` |
| `blocks.recent` 的 lookback | `3` 天 | `rust/crates/ccusage-core/src/lib.rs:60`（`DEFAULT_RECENT_DAYS`），用于 `rust/crates/ccusage/src/commands/mod.rs:573`、`659` |
| 紧凑布局阈值 | 100 列 | `rust/crates/ccusage-core/src/lib.rs:61`（`USAGE_COMPACT_WIDTH_THRESHOLD`） |
| `statusline.offline` | `true` | `rust/crates/ccusage-cli/src/types.rs:221-240` |
| `statusline.cache` | `true` | 同上 |
| `statusline.refresh_interval` | `1`（秒） | 同上 |
| `statusline.context_low_threshold` / `medium` | `50` / `80` | 同上 |
| `statusline.visual_burn_rate` | `off` | 同上 |
| `statusline.cost_source` | `auto` | 同上 |
| `weekly.start_of_week` | `sunday`（`ccusage claude weekly`） | `rust/crates/ccusage-cli-parser/src/parser.rs:520-524` |
| 统一周报（非 `claude weekly`）的周起点 | `monday` | `rust/crates/ccusage/src/cli/last_window.rs:60-62`（`UNIFIED_WEEK_START`）、`rust/crates/ccusage-adapter-all/src/loader.rs:897` |

---

## 2. CLI 解析

### 2.1 结论：自研 parser，不用 clap

`rust/crates/ccusage-cli-parser/src/` 是手写解析器：`parser.rs`（1247 行，命令/选项语义）+ `arg_parser.rs`（104 行，词法切分）。整个 crate 不依赖 clap/argh 之类框架（`Cargo.toml` 里没有相关依赖；解析全程只做 `Vec<String>` 索引游走）。

- 入口：`Cli::parse_from_with_config(args, config, default_session_duration_hours, version)`（`parser.rs:65-133`），`args.into_iter().skip(1)` 丢掉程序名（`parser.rs:74`）。
- 词法层：`ArgParser`（`arg_parser.rs:3-7`）提供 `peek` / `peek_name` / `next` / `next_flag` / `value_for`。
- `--flag=value` 与 `--flag value` 都支持；`--flag=`（空值）报 `Missing value for <flag>`；值以 `-` 开头同样报 `Missing value for <flag>`（`arg_parser.rs:59-73`）。`daily=bad` 这种「位置参数带等号」报 `Expected option, got 'daily=bad'`（`arg_parser.rs:40-57`，测试 `arg_parser.rs:95-103`）。
- 不支持短选项合并（`-jO` 会被当成未知 flag），也不支持 `--` 之后的裸参数透传。
- 非 UTF-8 参数：`Arguments must be valid UTF-8`（`arg_parser.rs:10-23`，`rust/crates/ccusage/src/cli.rs:41-51` 也做了一次同样校验）。

### 2.2 选项命名规则与清单

命名规则：长选项 kebab-case（`--project-aliases`、`--start-of-week`、`--debug-samples`、`--no-cost`）；短选项**单字符且不冲突**；`--no-*` 系列（`--no-offline`、`--no-color`、`--no-cost`、statusline 的 `--no-cache`）显式存在，而不是靠否定前缀隐式实现。

共享选项（`is_shared_flag` 白名单，全部命令可用）：

| 短 | 长 | 取值 | 证据 |
| --- | --- | --- | --- |
| `-s` | `--since` | 日期 | `parser.rs:745-747` |
| `-u` | `--until` | 日期 | `parser.rs:748-750` |
| — | `--last` | 正整数 | `parser.rs:751` |
| `-j` | `--json` | 开关 | `parser.rs:752` |
| `-m` | `--mode` | auto/calculate/display | `parser.rs:753` |
| `-d` | `--debug` | 开关 | `parser.rs:754` |
| — | `--debug-samples` | u64 | `parser.rs:755-760` |
| `-o` | `--order` | desc/asc | `parser.rs:761-764` |
| `-b` | `--breakdown` | 开关 | `parser.rs:765` |
| `-O` | `--offline` / `--no-offline` | 开关 | `parser.rs:766-767` |
| — | `--color` / `--no-color` | 开关 | `parser.rs:768-769` |
| `-z` | `--timezone` | 字符串 | `parser.rs:770` |
| `-q` | `--jq` | 字符串 | `parser.rs:771` |
| — | `--config` | 路径 | `parser.rs:772` |
| — | `--compact` | 开关 | `parser.rs:773` |
| — | `--single-thread` | 开关 | `parser.rs:774` |
| — | `--no-cost` | 开关 | `parser.rs:775` |

命令专有选项：

- `daily`（统一与 `claude daily`）：`-i/--instances`、`-p/--project`、`--project-aliases`（`parser.rs:492-497`）。
- `weekly`（`claude weekly`）：`-w/--start-of-week`（`parser.rs:530-532`）。
- `session`：`-i/--id`（`parser.rs:449`、`parser.rs:550`）。
- `blocks`：`-a/--active`、`-r/--recent`、`-t/--token-limit`、`-n/--session-length`（`parser.rs:202-215`；默认 `session_length` 由 `DEFAULT_SESSION_DURATION_HOURS` 注入，`parser.rs:190-196`）。
- `statusline`：`-O/--offline`、`--no-offline`、`-B/--visual-burn-rate`、`--cost-source`、`--cache`、`--no-cache`、`--refresh-interval`、`--context-low-threshold`、`--context-medium-threshold`、`-z/--timezone`、`--config`、`--debug`（`parser.rs:224-260`）。注意 statusline 走**独立分支**、不消费共享表（它只从根选项继承 `timezone`，见 `parser.rs:220-222`），因此 `--json` 之类对它无效并会报 `Unknown statusline option '--json'`。
- `codex`：`--speed`、`-i/--id`（仅 session 报告，`parser.rs:618-624`）。
- `pi`：`--pi-path`（`parser.rs:652`）；`openclaw`：`--open-claw-path`（`parser.rs:682`）。
- 根级「统一报告」选项（只在命令之前、或 daily/monthly/weekly/session 之后有效）：`--all`、`--sections <d,w,m,s>`、`--by-agent`（`parser.rs:383-403`）。

### 2.3 全局选项与子命令的继承

解析分两段：先解析「命令之前」的 root 选项（`parser.rs:96-107`），再进入 `parse_command`，把已解析的 `shared.clone()` 交给命令分支（`parser.rs:114-121`）。命令内部再解析共享选项时是**就地覆盖**（`parse_shared_arg_for_command` → `parse_shared_arg`，`parser.rs:729-741`）。

- 关键后果：`ccusage --timezone UTC daily --timezone Asia/Tokyo` 以命令内的为准；`--timezone` 校验只检查「最终生效值」（`rust/crates/ccusage/src/cli/timezone.rs:118-127` 的测试断言）。
- 根级专用选项在不适用的命令上会报错：`accepts_root_all_options` 只认 `daily|monthly|weekly|session`（`parser.rs:366-368`），否则报 `Unknown option '--sections'`（以首个使用到的 flag 名回显，`parser.rs:27-38`、`parser.rs:160-165`）。
- 旧式写法被显式迁移并报错：`claude:daily` → 自动改写为 `claude daily`（`normalize_legacy_agent_command_args`，`parser.rs:811-822`）；而 `--daily`/`--weekly` 等报告 flag 报 `Report flags like --daily are not supported. Use "ccusage daily" instead.`（`parser.rs:828-839`）；`--agent`/`-a` 过滤器报 `Agent filters like --agent are not supported. Use "ccusage <agent> <report>"...`（`parser.rs:841-855`），其中 `blocks -a`（active）被豁免（`parser.rs:842`、`parser.rs:857-866`）。
- 命令集合：`daily|monthly|weekly|session|blocks|statusline` + 18 个 agent（`claude`、`codex`、`opencode`、`amp`、`droid`、`codebuff`、`hermes`、`pi`、`goose`、`openclaw`、`kilo`、`copilot`、`gemini`、`antigravity`、`kimi`、`qwen`、`grok`、`zcode`），见 `parser.rs:781-809` 与 `parser.rs:166-363`。
- 每个 agent 支持的 report 集合不同：`claude` 全 6 种，`codex` 只有 daily/monthly/session，`opencode` 有 daily/weekly/monthly/session，其余 15 个 agent 是 daily/monthly/session（`parser.rs:973-987`）。

### 2.4 分层帮助与 `help_codegen`

`ccusage daily --help` 这类分层帮助是**真实现**，且帮助文本是构建期从 JSON 生成的：

- `help.rs` 用 `include!(concat!(env!("OUT_DIR"), "/cli-help.rs"))` 引入生成代码（`help.rs:13`）。
- `--help`/`-h` 在任何语义校验之前拦截（`control_arg`，`parser.rs:136-150`；测试 `parser.rs:1232-1246` 断言 `--help` 优先于 `--daily` 之类的错误，`--version` 又优先于 `--help`）。
- 帮助页按命令 token 序列**精确匹配**：`find_help_page` 比对 `page.path` 与 `command_tokens(args)`（`help.rs:59-68`；上一层 `help_text_for_tokens` 在 `help.rs:55-57`，`help_text_for_args` 在 `help.rs:30-34`）；token 提取会跳过选项及其取值（`parser.rs:890-906`、`option_takes_value` `parser.rs:908-947`）。命中则渲染该页（含 `COMMANDS`/`OPTIONS`），未命中的回退到根帮助（`help.rs:30-34`、`help.rs:70-86`、`help.rs:88-114`）。
- 程序名（`ccusage` / `ccusage.exe`）会被剥掉，兼容 `ccusage daily --help` 与 `ccusage daily` 两种入口（`help.rs:36-53`）。
- `--version` 直接打印 `ccusage {version}` 并 `exit(0)`（`help.rs:15-18`）；版本串是编译期 `env!("CCUSAGE_VERSION")`（`rust/crates/ccusage/src/cli.rs:23`，见本章第 6 节）。
- `help_codegen.rs` 的职责：读 `src/cli-help.json`（选项集合：`flags`/`description`/`default`/`choices`）与 `src/cli-commands.json`（`root.usage`、`root.commands`、`pages[]`、`combinedOptions`），生成 `const ROOT_USAGE / ROOT_COMMANDS / HELP_PAGES` 与每个选项集的 `fn xxx() -> &'static str`。
  - 证据：`help_codegen.rs:5-17`（入口）、`19-52`（选项函数与组合选项）、`54-107`（`OPTIONS:` 段渲染，对齐宽度最小 36，`default:`/`choices:` 括号后缀）、`109-163`（命令规格）、`165-178`（组合选项去表头拼接）、`180-260`（写函数/取值工具）。
  - 构建钩子：`rust/crates/ccusage-cli-parser/build.rs:5-29`（`#[path = "src/help_codegen.rs"] mod help_codegen;` 复用同一份源码，`rerun-if-changed` 两个 JSON，产物写入 `OUT_DIR/cli-help.rs`）。
  - 选项说明的实际文案样例见 `rust/crates/ccusage-cli-parser/src/cli-help.json:2-45`（`-j, --json`、`-O, --offline`、`--no-offline` 标注 "Negatable of -O, --offline"）。
- 注意：**`--jq` 不在帮助文本里**。`cli-help.json` 全文件无 `jq` 命中（grep `jq` 在 `ccusage-cli-parser/src` 只命中 `parser.rs:771`、`parser.rs:923`、`parser.rs:1039`、`tests.rs:133/971` 与快照），即 `-q/--jq` 是可解析但未列进 `--help` 的选项。

### 2.5 错误提示格式与退出码

统一出口在二进制层：解析失败 → stderr 打印单条 message，再打印 `Run 'ccusage --help' for usage.`，`process::exit(2)`。

证据：`rust/crates/ccusage/src/cli.rs:13-33`（`unwrap_or_else(|message| exit_with_usage(&message))`）、`cli.rs:35-39`（文案与退出码）。解析器内部只返回 `Result<_, String>`，不自行打印。

---

## 3. 参数校验（互斥/冲突、非法值、未知命令）

**先纠正一个常见假设：`--json` 与 `--jq` 不互斥。** `wants_json = shared.json || shared.jq.is_some()`（`rust/crates/ccusage-core/src/output.rs:14-16`），两者同时给出时 jq 分支优先（`output.rs:158-187`）。同理 `--offline` 与 `--no-offline` 也不互斥，后者只是把开关翻回去（`parser.rs:766-767`，语义由各 loader 自行判定，见 `docs/guide/environment-variables.md:205-213` 的对照说明）。

真正存在冲突检测的项：

| 规则 | 文案 | 证据 |
| --- | --- | --- |
| `--last` 只能用于按日历分组的报告（daily/weekly/monthly） | `The --last option is only available for the daily, weekly, and monthly reports.` | `parser.rs:1100-1110` |
| `--last` 不能与 `--since`/`--until` 并用 | `The --last option cannot be combined with --since or --until.` | `parser.rs:1111-1113` |
| `--last` 不能与 `--sections` 并用 | `The --last option cannot be used with --sections.` | `parser.rs:1114-1116` |
| `--since` 晚于 `--until` | `The --since date 'x' is later than the --until date 'y'.` | `parser.rs:1120-1132`（statusline 豁免，`parser.rs:1124-1126`） |
| `session --id` 与 `--sections`/`--by-agent` 并用 | `The --sections and --by-agent options cannot be used with session --id.` | `parser.rs:454-460` |
| 报告 flag 别名 | `Report flags like --daily are not supported. Use "ccusage daily" instead.` | `parser.rs:828-839` |
| agent 过滤器 | `Agent filters like --agent are not supported. Use "ccusage <agent> <report>", for example "ccusage codex daily".` | `parser.rs:841-855` |
| agent 不支持的 report | `The "blocks" report is only available for Claude Code usage.` / `The "weekly" report is not available for Codex usage.`（另附 `Use "ccusage <agent> daily" ...`） | `parser.rs:868-888`（分支在 `878-886`） |

非法取值文案（全部带 `Invalid ...` 前缀，退出码 2）：

- 日期：`Invalid value for --since 'x'. Expected YYYY-MM-DD or YYYYMMDD.`（`parser.rs:1047-1051`）。合法格式只有两种：8 位纯数字，或 `YYYY-MM-DD`；会校验真实日历（含闰年），`2026-02-30`、`2026-13-01`、`2026-7-10`、`2026/07/10`、带时间戳的串全部拒绝。证据：`rust/crates/ccusage-cli/src/types.rs:73-110`、`112-124`，测试 `types.rs:331-381`。
- `--last`：`Invalid value for --last 'x'. Expected a whole number of periods, 1 or greater.`（`0` 与解析失败同罪，`parser.rs:1053-1060`）。
- 枚举：`Invalid cost mode 'x'`（`parser.rs:1134-1141`）、`Invalid sort order 'x'`（`1143-1149`）、`Invalid --sections value 'x'. Expected one or more of: daily, weekly, monthly, session.`（`1151-1179`，去重且空串报错）、`Invalid week day 'x'`（`1181-1192`）、`Invalid speed option 'x'`（`1194-1202`）、`Invalid visual burn rate 'x'`（`1204-1212`）、`Invalid cost source 'x'`（`1214-1222`）。
- 数值型：`Invalid value for --session-length`（`parser.rs:208-213`）、`--refresh-interval`（`parser.rs:236-241`）、`--context-low-threshold`（`242-247`）、`--context-medium-threshold`（`248-255`）、`--debug-samples`（`755-760`）。
- 时区：`Invalid value for --timezone 'x'. Expected an IANA timezone name such as UTC or America/New_York, or 'local' for the system timezone.`（`rust/crates/ccusage/src/cli/timezone.rs:10-17`）。该校验在配置合并**之后**跑一次，只查命令实际会用的那个值（`timezone.rs:19-27`、测试 `118-150`）。
- 配置侧同规则：`config.rs:69-90`（见 §1.5）。

未知子命令/未知选项：

- 顶层非选项 token 不是已知命令：`Unknown command 'x'`（`parser.rs:100-102`），`parse_command` 的兜底同样报 `Unknown command 'x'`（`parser.rs:362`）。
- `claude` 下的未知子命令：`Unknown claude command 'x'`（`parser.rs:569-571`）；agent 报告位置上的未知 token 是 `Unknown <agent> command 'x'`（`parser.rs:710-712`）。
- 各命令的未知选项：`Unknown daily option 'x'`（`parser.rs:498`）、`Unknown weekly option 'x'`（`533`）、`Unknown session option 'x'`（`450`/`551`）、`Unknown blocks option 'x'`（`214`）、`Unknown statusline option 'x'`（`259`）、`Unknown codex option 'x'`（`623`）、`Unknown pi option 'x'`（`653`）、`Unknown openclaw option 'x'`（`683`）；共享表不认的 flag 报 `Unknown option 'x'`（`parser.rs:776`）。
- 多余位置参数：`Unexpected argument 'x'`（`parser.rs:123-125`）。
- 参数用尽仍要取值：`Expected option but reached end of arguments`（`arg_parser.rs:40-44`）。

---

## 4. 输出渲染

### 4.1 JSON 序列化约定

键名一律 **camelCase**，且都是手写 `json!` 构造（没有 `#[derive(Serialize)]` 的 snake_case 泄漏）：

- 每日/每周/每月/通用汇总行：`summary_json`（`rust/crates/ccusage-core/src/output.rs:27-56`）→ `inputTokens`/`outputTokens`/`cacheCreationTokens`/`cacheReadTokens`/`totalTokens`/`totalCost`/`modelsUsed`/`modelBreakdowns`。
- 会话行：`session_summary_json`（`output.rs:58-77`）→ 额外 `sessionId`/`lastActivity`/`firstActivity`/`projectPath`。
- 汇总行：`totals_json`（`output.rs:79-121`）。
- agent 报告行：`agent_summary_json`（`rust/crates/ccusage-core/src/agent_report.rs:5-19`）；容器键 `daily|weekly|monthly|session`（`rust/crates/ccusage-adapter-all/src/report.rs:207-210`）、`totals`（`report.rs:175`）、`agents`（`report.rs:148-156`；`report.rs:136-143` 是 period/metadata 的插入点）。
- 周期字段的键名由 `period_key` 决定（`agent_report.rs:68`），首列表头由 `first_column` 决定（`agent_report.rs:50`），摘要取周期值用 `summary_period`（`agent_report.rs:59`）。

**空值省略规则**（这是脚本兼容性的关键）：

- 行级可选键按「有才插」：`date`/`month`/`week`/`project`/`credits`（`output.rs:38-54`）；会话行的 `credits` 同样条件插入（`output.rs:73-76`）。
- `totals.credits` 只在总和 > 0 时出现（`output.rs:112-115`）。
- `totals.unpricedModels` 只在存在缺价模型时出现，且排序去重（`output.rs:126-145`）；每个受影响的 `modelBreakdowns` 条目带 `"missingPricing": true`，有价的条目**省略该键**（`docs/guide/json-output.md:32-39`）。
- 浮点整数化：`json_float` 把有限且无小数部分的 `f64` 写成整数（`output.rs:452-462`）。
- 项目分组输出把 key 归到 `unknown`（`group_project_output`，`output.rs:147-156`）。

### 4.2 `print_json_or_jq` 如何调用 jq

**外部二进制，不是内嵌库**：`std::process::Command::new("jq")`，把 JSON 写进子进程 stdin，stdout 继承，非零退出报 `jq failed`。

证据：`rust/crates/ccusage-core/src/output.rs:158-187`（spawn 在 `163-168`，写 stdin 在 `169-174`，等待并检查状态在 `175-178`；无 jq 时走 `serde_json::to_writer_pretty` + `\n`，`180-185`）。`--no-cost` 在序列化前先剥离成本字段（`output.rs:159-161` + `strip_cost_json` `output.rs:564-581`，递归删 `totalCost`/`costUSD`/`cost`）。

jq 不可用时（未安装）：spawn 失败会得到 `failed to run jq: <io error>`（`output.rs:165-168`）。文档只教「管道给 jq」（`docs/guide/json-output.md:580-604`），`--jq` 本身未进帮助文本（见 §2.4）。

### 4.3 `sanitize_terminal_text`

把字符串里的控制字符转成可见转义（`escape_default`），防止模型名/项目名注入 ANSI 控制序列污染终端输出：

- 定义：`rust/crates/ccusage-core/src/output.rs:552-562`。
- 再导出：`rust/crates/ccusage-core/src/lib.rs:41`。
- 实际使用点（示例）：Codex 报告里对标签做净化（`rust/adapters/codex/src/report.rs:11` 引入、`rust/adapters/codex/src/report.rs:503` 调用）。
- 测试：`output.rs:905-910`（`sanitizes_terminal_control_characters_as_visible_escapes`，`"future\nclient\t\u{1b}[31m"`）。

### 4.4 进度条与 TTY 检测

- 开关判定：`should_show_usage_load_progress(json, output_is_tty) = !json && output_is_tty`（`rust/crates/ccusage-core/src/progress.rs:35-37`）；TTY 判定是 `io::stdout().is_terminal()`（`progress.rs:94-96`）。
- 实际调用再加一层：非 `LOG_LEVEL=0` 才显示（`progress.rs:290-306`，条件在 `295-296`）。也就是说 `--json` 或管道输出、或 `LOG_LEVEL=0` 时都没有 spinner。
- 渲染细节：10 帧盲文 spinner、80 ms 间隔（`progress.rs:11-12`），写 stderr 并隐藏/恢复光标（`progress.rs:220`、`progress.rs:281-286`），单行宽度自适应（`fit_status_to_width` `progress.rs:87-92`，宽度取 `ccusage_terminal::terminal_width()`）。
- 单例：`thread_local` 的 `ACTIVE_PROGRESS`，已有会话时复用不重复起线程（`progress.rs:17-19`、`progress.rs:132-141`）；停止时清屏并清空状态（`progress.rs:198-229`）。
- 文案形如 `Loading usage logs (1/3) :: Codex, OpenCode`（`progress.rs:43-78`，测试 `326-337`）；价格刷新会追加前缀 `Refreshing model pricing from LiteLLM...`（测试 `340-353`）。

### 4.5 表格与紧凑模式

- 紧凑布局条件：显式 `--compact`，或 **stdout 是 TTY 且终端宽度 < 100 列**（`output.rs:18-25` + `rust/crates/ccusage-core/src/lib.rs:61`）。
- 非 TTY 且窄**不会**自动紧凑（测试 `output.rs:593-597`）。
- 列集合随 `compact` 与 `show_cache_creation` 变化（`usage_table_columns`，`output.rs:385-404`），`--no-cost` 会裁掉 Cost 列（`output.rs:249-252`、`304-306`、`369-371`）。
- 空结果走 stderr：`No usage data found.`（`output.rs:233-236` + `output.rs:406-408`）。
- 缺价告警走 stderr，且区分离线：`WARN Missing embedded pricing for {model}; ... Run without --offline ...` vs `WARN Missing pricing for {model}; ...`（`output.rs:410-450`）。
- 表格实现与宽度计算在 `ccusage-terminal` crate（`rust/crates/ccusage-terminal/src/{table.rs,width.rs,terminal.rs}`）；`COLUMNS` 环境变量只影响它（`terminal.rs:6`）。

---

## 5. 环境变量清单（源码穷举）

以 `rg 'env::var'`（含 `var_os`）对 `rust/` 全树穷举：**21 个 `_ENV` 常量 + 13 个直接字面量**，去重后共 33 个用户可配置变量名，另加 1 个 Cargo 注入项（`OUT_DIR`），合计 34 个读取目标（下表逐一列出）：

| 变量 | 用途 | 默认/回退 | 证据 |
| --- | --- | --- | --- |
| `HOME` | 主目录 | 无则看 `USERPROFILE` | `rust/crates/ccusage-core/src/home.rs:5` |
| `USERPROFILE` | Windows 主目录 | 无则看 `HOMEDRIVE`+`HOMEPATH` | `home.rs:6`、`home.rs:17-31` |
| `HOMEDRIVE` / `HOMEPATH` | 拼出主目录 | — | `home.rs:7-8`、`home.rs:24-30` |
| `LOG_LEVEL` | 日志级别（`0` 静默…`5` trace） | 未设 = 默认级别 | `rust/crates/ccusage-core/src/logger.rs:11-15`；文档表 `docs/guide/environment-variables.md:61-90` |
| `NO_COLOR` | 关色（任何非空值） | 无 | `rust/crates/ccusage-terminal/src/style.rs:39` |
| `FORCE_COLOR` | 强制开色 | 无（否则看 TTY） | `style.rs:42` |
| `COLUMNS` | 终端宽度覆盖 | 无则探测终端 | `rust/crates/ccusage-terminal/src/terminal.rs:6` |
| `XDG_CACHE_HOME` | HTTP 缓存根（必须是绝对路径） | `~/.cache` | `rust/crates/ccusage/src/http.rs:39-52` |
| `CLAUDE_CONFIG_DIR` | Claude 数据目录（含逗号列表）与配置查找目录 | `~/.config/claude`、`~/.claude` | `rust/adapters/claude/src/paths.rs:26`、`rust/crates/ccusage-config/src/config.rs:272` |
| `XDG_CONFIG_HOME` | Claude 配置目录回退 | `~/.config` | `rust/adapters/claude/src/paths.rs:46` |
| `CODEX_HOME` | Codex 数据目录（可列表） | `~/.codex` | `rust/adapters/codex/src/paths.rs:227` |
| `XDG_DATA_HOME` | OpenCode 数据根 | `~/.local/share` | `rust/adapters/opencode/src/paths.rs:6`、`paths.rs:29` |
| `OPENCODE_DATA_DIR` | OpenCode 目录覆盖 | XDG 推导 | `opencode/src/paths.rs:5` |
| `AMP_DATA_DIR` | Amp | `~/.local/share/amp` | `rust/adapters/amp/src/paths.rs:5` |
| `DROID_SESSIONS_DIR` | Droid | `~/.factory/sessions` | `rust/adapters/droid/src/paths.rs:5` |
| `CODEBUFF_DATA_DIR` | Codebuff | `~/.config/manicode` | `rust/adapters/codebuff/src/paths.rs:5` |
| `HERMES_HOME` | Hermes | `~/.hermes` | `rust/adapters/hermes/src/paths.rs:5` |
| `PI_AGENT_DIR` | pi-agent | `~/.pi/agent/sessions` | `rust/adapters/pi/src/paths.rs:5` |
| `GOOSE_PATH_ROOT` | Goose | 标准 Goose 根 | `rust/adapters/goose/src/paths.rs:5` |
| `OPENCLAW_DIR` | OpenClaw | `~/.openclaw` | `rust/adapters/openclaw/src/paths.rs:9` |
| `KILO_DATA_DIR` | Kilo | `~/.local/share/kilo` | `rust/adapters/kilo/src/paths.rs:9` |
| `KIMI_DATA_DIR` | Kimi | `~/.kimi`、`~/.kimi-code` | `rust/adapters/kimi/src/paths.rs:9` |
| `QWEN_DATA_DIR` | Qwen | `~/.qwen` | `rust/adapters/qwen/src/paths.rs:9` |
| `COPILOT_HOME` / `COPILOT_OTEL_FILE_EXPORTER_PATH` | Copilot CLI 目录 / 单个 OTEL JSONL | `~/.copilot` | `rust/adapters/copilot/src/paths.rs:5-6`、`paths.rs:56`、`paths.rs:76` |
| `GEMINI_DATA_DIR` | Gemini CLI | `~/.gemini/tmp` | `rust/adapters/gemini/src/paths.rs:5` |
| `ANTIGRAVITY_DATA_DIR` | Antigravity | `~/.gemini/antigravity*`、`~/.config/antigravity` | `rust/adapters/antigravity/src/paths.rs:7` |
| `GROK_HOME` | Grok Build CLI | `~/.grok` | `rust/adapters/grok/src/paths.rs:9` |
| `ZCODE_HOME` | ZCode（非空但全非法时不回退默认） | `~/.zcode` | `rust/adapters/zcode/src/paths.rs:9`、文档 `docs/guide/environment-variables.md:55` |
| `CCUSAGE_MODEL_ALIASES` | 模型名别名（JSON 对象或 `a=b,c=d` 形式；支持 `-fast` 变体） | 无 | `rust/crates/ccusage-core/src/model_aliases.rs:8`、`model_aliases.rs:13-60` |
| `CCUSAGE_PRICING_JSON_PATH` | **构建期**：LiteLLM 快照输入文件 | 无则按 feature 下载/报错 | `rust/crates/ccusage-core/build.rs:13`、`build.rs:22-29`、`build.rs:75-82` |
| `CCUSAGE_VERSION` | **构建期**：版本串注入 | 无则读根 `package.json` 的 `version` | `rust/crates/ccusage/build.rs:6-18` |
| `OUT_DIR` | 构建脚本产物目录（Cargo 注入） | — | `rust/crates/ccusage-cli-parser/build.rs:18`、`rust/crates/ccusage-core/build.rs:65` |

**文档与实现不一致（S6 发现，建议记入交叉核验清单）**：下表的「实现」列指 `env::var` / `env::var_os` 读取点；以下变量在两版源码中都**没有**对应读取，只存在于文档。

| 变量名 | 文档位置 | 说明 |
| --- | --- | --- |
| `CCUSAGE_OFFLINE` | `docs/guide/environment-variables.md:124-131`（"Force offline mode by default"）、`:209`；v19 同文 `ccusage-v19/docs/guide/environment-variables.md:96-110` | 跨版本遗留：离线只能靠 `--offline` / 配置 `offline`（`parser.rs:766`、`config_schema.rs:378`） |
| `CCUSAGE_TIMEZONE` | `docs/guide/configuration.md:38` 与 `ccusage-v19/docs/guide/configuration.md:38` | 时区只能来自 `--timezone` 或配置 `timezone`（`parser.rs:770`、`timezone.rs:10-17`） |
| `CCUSAGE_GROK_PATH` | `docs/superpowers/specs/2026-07-28-grok-env-only-path-design.md:24`、`:51` | 该设计文档本身要求**移除**它，只保留 `GROK_HOME` 与 `~/.grok`（该文 `:35`、`:51-52`、`:83`）；v20 实现为 `rust/adapters/grok/src/paths.rs:9` 的 `GROK_HOME` |

即：源码 `env::var` 读取的 `CCUSAGE_*` 只有 `CCUSAGE_VERSION`、`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_MODEL_ALIASES` 三个（见上表），其余 `CCUSAGE_*` 名称只在文档里流通。

---

## 6. HTTP 与版本

### 6.1 `http.rs` 抓什么：只有价格表，且只认两个 URL

- 目标 URL 常量：LiteLLM `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`（`rust/crates/ccusage-core/src/pricing.rs:55-56`）与 models.dev `https://models.dev/api.json`（`pricing.rs:57`）。
- URL 白名单：`PricingEndpoint::for_url` 只接受这两个（`pricing.rs:70-78`），其它 URL 直接 `unsupported pricing URL: {url}`（`rust/crates/ccusage/src/http.rs:25-34`）。
- 响应校验：拿到 body 后按 endpoint 做「有可用条目」校验（`pricing.rs:80-126`）与轮廓校验（`pricing.rs:88-98`），失败报 `pricing response failed <endpoint> validation`（`http.rs:121-126`）。
- 没有任何「更新检查」请求：全仓 HTTP 调用点只有 `fetch_pricing_json` / `fetch_models_dev_json`（`pricing.rs:2863-2893`）与构建期下载（`rust/crates/ccusage-core/build.rs:89-105`）。

### 6.2 超时、缓存与并发安全

- 超时：全局 10 秒（`http.rs:11` 常量，`http.rs:75-78` 装配 `ureq::Agent`）；响应体上限 64 MiB（`http.rs:12`、`http.rs:104-120`、`read_bounded_body` `http.rs:139-151`）。
- 缓存键：URL 派生文件名 + FNV-1a 哈希（`cache_file_stem`，`http.rs:236-257`），落在 `$XDG_CACHE_HOME/ccusage/http-cache/` 或 `~/.cache/ccusage/http-cache/`（`http.rs:37-52`）。
- 条件请求：带 `if-none-match`（`http.rs:80-82`）；304 时只接受**校验通过**的缓存体，否则先失效再无条件重取一次（`http.rs:88-99`，`revalidate=false` 走 `fetch_json_with_cache_dir_inner` 递归）。
- 原子写：etag+body 写临时文件后 `rename`，注释明确「一次 rename 保证并发读者不会看到混搭的一对」（`http.rs:153-157`、`http.rs:199-229`）。
- 缓存文件上限与 etag 合法性：`CACHE_FILE_MAX_BYTES`（`http.rs:14`）、etag 必须 ASCII 且无控制字符（`http.rs:186-192`、`http.rs:200-206`）。
- 客户端装在二进制里而不是 core：`set_json_fetcher`（`pricing.rs:2871-2893`，注释解释为「避免把 ureq/TLS 拉进每个 adapter 的依赖」），装配点 `rust/crates/ccusage/src/main.rs:26`。
- models.dev 失败节流：失败后 60s 内不再重试（`pricing.rs:58`、`pricing.rs:795-799`）。
- 离线开关：`--offline`/`--no-offline`（`parser.rs:766-767`）与配置 `offline`/`noOffline`（`config_schema.rs:377-380`），statusline 默认离线（`rust/crates/ccusage-cli/src/types.rs:224`）。

### 6.3 `CCUSAGE_VERSION` 注入方式

- 构建脚本：`rust/crates/ccusage/build.rs:5-18`。优先 `env::var("CCUSAGE_VERSION")`；未设则读仓库根 `../../../package.json` 的 `version`；随后 `println!("cargo:rustc-env=CCUSAGE_VERSION={version}")`，并声明 `rerun-if-env-changed` / `rerun-if-changed`。
- 运行期读取：`env!("CCUSAGE_VERSION")`（`rust/crates/ccusage/src/cli.rs:23` 传给解析器，最终 `--version` 打印 `ccusage {version}`，`help.rs:15-18`）。
- 一致性测试：`compiled_version_matches_release_package`（`rust/crates/ccusage/src/main.rs:125-135`）断言编译期版本 == 根 `package.json` 的 `version`。
- Nix 打包显式传这两个环境变量：`package.nix:46-47`（`CCUSAGE_PRICING_JSON_PATH`、`CCUSAGE_VERSION`），并在 `nix/cargo-artifacts.nix:98` 列为影响产物哈希的变量。
- 价格快照构建：`rust/crates/ccusage-core/build.rs:17-40`（LiteLLM 压缩快照 + models.dev 两个快照），压缩级别 10（`build.rs:58-62`），嵌入白名单（`build.rs:203-221`：`claude-*`、`anthropic*`、`gpt-*`、`o3*`、`o4-mini*`、`openai/*`、`azure/*`、`zai/*`、`openrouter/openai/*`），运行期首次使用时 inflate（`pricing.rs:40-54`）。

---

## 7. TS → Rust 迁移对比（v19.0.3）

### 7.1 v19 的源码结构

v19 是 TS monorepo，业务逻辑在 `apps/ccusage/src`：

- 入口与运行时：`cli.ts`（234 行）只是 Bun/Node 运行时选择器（优先找 `bun`，否则要求 Node ≥ 22，`cli.ts:9`、`cli.ts:95-128`、`cli.ts:130-167`），真实命令定义在 `main.ts`/`main.bun.ts`/`main.node.ts`，顶层 `index.ts` 7 行。
- 命令层：`commands/{index,daily,weekly,monthly,session,blocks,statusline,all,agent,codex,session_id,loading-progress}.ts`。
- 适配层：`adapter/{claude,codex,opencode,amp,pi}` + `adapter/shared.ts`（735 行共享 loader 框架）；Claude 解析器单文件 `adapter/claude/data-loader.ts` 达 7004 行。
- 共享包：`packages/internal/src/{jsonl,fs,pricing,workers,array,colors,format,sort,sqlite,logger,json-file-state,pricing-fetch-utils}.ts`；表格在 `packages/terminal/src/table.ts`（1713 行）。
- 证据：`ccusage-v19/apps/ccusage/src/` 与 `ccusage-v19/packages/` 目录清单；`ccusage-v19/apps/ccusage/src/cli.ts:91-128`；`ccusage-v19/packages/terminal/src/table.ts` 行数。

v20 的对应物：Rust workspace `rust/crates/{ccusage,ccusage-cli,ccusage-cli-parser,ccusage-config,ccusage-core,ccusage-terminal,ccusage-adapter-all,ccusage-test-support}` + `rust/adapters/*`（18 个 adapter，每个 `src/{lib,loader,parser,paths,report}.rs`）。`apps/ccusage` 在 v20 只保留 npm 分发壳：`apps/ccusage/src/cli.js:27-58`（平台包名映射）、`cli.js:93-109`（解析 native 二进制）、`cli.js:233-258`（spawn 并转发信号），行为等价于 v19 `cli.ts` 的一部分但目标从 JS bundle 换成了 native binary。

### 7.2 CLI 与配置对照

| 维度 | v19（TS） | v20（Rust） |
| --- | --- | --- |
| 解析框架 | gunshi + valibot（`ccusage-v19/apps/ccusage/src/shared-args.ts:1-4`、`:106`、`:111-114`） | 自研 `parser.rs` + `arg_parser.rs`（`rust/crates/ccusage-cli-parser/src/parser.rs:65-133`） |
| 日期格式 | 只接受 `YYYYMMDD`（`ccusage-v19/apps/ccusage/src/shared-args.ts:19-30`，`filterDateSchema`） | `YYYY-MM-DD` 或 `YYYYMMDD`，且校验真实日历（`rust/crates/ccusage-cli/src/types.rs:73-110`） |
| `--last` | 不存在（v19 源码全仓无 `--last`；只有 `weekly` 的 `startOfWeek`，`ccusage-v19/apps/ccusage/src/commands/weekly.ts:26`） | 新增（`parser.rs:751`、`rust/crates/ccusage/src/cli/last_window.rs:11-24`） |
| `--jq` | 不存在（v19 全仓无 `--jq` 选项，文档只教管道 `\| jq`） | 新增（`parser.rs:771`；实现为外部 jq，`output.rs:158-187`） |
| `--sections` / `--by-agent` / `--all` | 不存在 | 新增统一报告选项（`parser.rs:383-403`、`parser.rs:366-368`） |
| `--no-cost` | `sharedArgs` 中无（`ccusage-v19/apps/ccusage/src/shared-args.ts:18-106`） | 新增（`parser.rs:775`、`strip_cost_json` `output.rs:564-581`） |
| 短选项字母 | `-O` 代表 `--offline` 且 `negatable`（`shared-args.ts:69-75`） | `-O` 同为 `--offline`，否定写作独立 flag `--no-offline`（`parser.rs:766-767`） |
| 配置文件 | `ccusage.json`，搜索 `$CWD/.ccusage/ccusage.json` + 各 Claude 目录下的 `ccusage.json`（`ccusage-v19/apps/ccusage/src/config-loader-tokens.ts:71-82`，文件名常量 `ccusage-v19/apps/ccusage/src/consts.ts:94`） | 同顺序（`rust/crates/ccusage-config/src/config.rs:258-283`） |
| 配置加载语义 | 第一个可解析文件生效，`$schema`/`defaults`/`commands` + 5 个 agent 段（`config-loader-tokens.ts:87-129`、`:235-293`）；有原型污染防护（`config-loader-tokens.ts:45-49`）；合并顺序 CLI > 命令配置 > 默认配置（`:296-341`） | 第一个「可读且是 JSON object」的文件生效（`config.rs:246-256`）；18 个 agent 段；应用顺序见 §1.4（`config.rs:92-132`） |
| Schema 生成 | 从 gunshi 参数定义生成（`ccusage-v19/apps/ccusage/scripts/generate-json-schema.ts`，511 行；`package.json` 的 `generate:schema`：`ccusage-v19/apps/ccusage/package.json:47`） | 从 Rust 结构体用 schemars 生成（`rust/crates/ccusage-config/src/config_schema.rs:711`；命令 `apps/ccusage/justfile:25-27`） |
| 版本来源 | `package.json` 的 `version` 直接 import（`ccusage-v19/apps/ccusage/src/commands/index.ts:4`，`package.json:4`） | 编译期 `CCUSAGE_VERSION`（`rust/crates/ccusage/build.rs:5-18`），并有测试对齐 `package.json`（`rust/crates/ccusage/src/main.rs:125-135`） |
| 分层帮助 | gunshi 自带 | 构建期 codegen：`cli-help.json` + `cli-commands.json` → `OUT_DIR/cli-help.rs`（`rust/crates/ccusage-cli-parser/build.rs:5-29`、`help_codegen.rs:5-17`） |
| 每命令报告支持差异报错 | `getReportFlagAliasError` / `getAgentFilterOptionError` / `getUnsupportedAgentReportError`（`ccusage-v19/apps/ccusage/src/commands/index.ts:198-227`） | 等价实现 `report_flag_alias_error` / `agent_filter_option_error` / `unsupported_agent_report_error`（`parser.rs:828-888`）——**文案与分支是逐条对齐移植的** |

### 7.3 Claude 去重键

v19：唯一键由 message id 与 request id 拼接，作用域是「单个 JSONL 文件内」的 `processedEntries`：

```ts
// ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1194-1202
export function createUniqueHash(data: UsageData): string | null {
	const messageId = data.message.id;
	if (messageId == null) return null;
	const requestId = data.requestId;
	return requestId == null ? messageId : `${messageId}:${requestId}`;
}
```

- 替换规则：候选条目的 token 合计更大，或合计相同但带 `speed` 字段，才替换已存在条目（`ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1629-1638`）。
- 应用点（每个报告各一份，逻辑相同）：`data-loader.ts:2075-2123`（daily：`existingEntryIndex = processedEntries[uniqueHash]`，不满足替换条件直接 `return`）、`:2136-2183`、`:2199-2264`、`:2322-2334`、`:2457-2469`；blocks 用带索引的变体 `:2894-2913`。

v20：把键升级为「哈希 + 会话感知 + sidechain 变体」，并且是**跨文件全局**去重：

- 精确键：`usage_dedupe_hash(message_id, request_id, session_id, timestamp)`（`rust/adapters/claude/src/lib.rs:350-371`）；当没有 request id 时，只有 `msg_` 前缀的 Anthropic message id 才允许跨时间戳合并，否则要求时间戳相同（`lib.rs:359-362` + `is_anthropic_message_id` `lib.rs:373-375` + `loaded_entry_matches_dedupe_key` `lib.rs:413-425`）。
- 额外两把键用于 sidechain 重放：`sidechain_replay_dedupe_hash`（`lib.rs:377-384`）与 `sidechain_entry_replay_dedupe_hash`（`lib.rs:386-393`），并在替换后重建索引（`lib.rs:295-311`）。
- 主入口：`push_deduped_entry`（`lib.rs:214-340`，查找/替换/插入三条路径分别在 `219-268`、`270-318`、`320-340`），入口循环在 `lib.rs:170-187`；daily 走镜像实现 `push_deduped_daily_entry`（`rust/adapters/claude/src/daily.rs:99-132`）。
- 替换规则对齐 TS：`should_replace_deduped_entry`（`lib.rs:198-213`）。
- 测试名体现了「比 TS 更细」的语义：`keeps_gateway_usage_from_distinct_sessions_with_reused_message_id`（`lib.rs:1196`）、`dedupes_anthropic_message_id_from_same_session_at_distinct_timestamps`（`lib.rs:1257`）、`dedupes_repeated_requestless_writes_at_the_same_timestamp`（`lib.rs:1283`）、`dedupes_sidechain_replay_after_an_equal_copy_from_another_session`（`lib.rs:1355`）；集成侧对应 `keeps_reused_message_id_from_distinct_sessions_at_same_timestamp`（`rust/crates/ccusage/src/main.rs:286-307`）。

**语义变化**：v19 中「同 message id + 同 request id」在不同会话/文件也会合并（键里没有 session），只有 gateway 前缀 id 因 requestless 才可能存活；v20 显式区分 `msg_` 与 `gateway_` 两类 id，并按会话隔离 requestless 的跨时间戳合并（`lib.rs:359-362`、测试 `main.rs:310-357`）。这是最容易导致两版数字不一致的点。

### 7.4 Codex 累计值处理

v19：**优先**使用 `last_token_usage`（`ParsedTokenCountLine.lastUsage`，`ccusage-v19/apps/ccusage/src/adapter/codex/types.ts:12-17`），只有它缺失时才回退到「累计值差分」（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:205-212`）；去重用「事件序列化键」：

```ts
// ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:90-104
function subtractRawUsage(current: RawUsage, previous: RawUsage | null): RawUsage {
	return {
		input_tokens: Math.max(current.input_tokens - (previous?.input_tokens ?? 0), 0),
		cached_input_tokens: Math.max(current.cached_input_tokens - (previous?.cached_input_tokens ?? 0), 0),
		output_tokens: ... /* 同上，另含 reasoning_output_tokens / total_tokens */
	};
}
```

- 取值优先级（关键）：`let raw = parsed.lastUsage; if (raw == null && parsed.totalUsage != null) { raw = subtractRawUsage(parsed.totalUsage, previousTotals); }`（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:205-209`）——`last_token_usage` 是主路径，累计差分是回退路径。
- 状态推进：`previousTotals` 只在 `parsed.totalUsage != null` 时前移（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:201`、`:210-212`）。
- `cached` 被夹到 `min(cached_input_tokens, input_tokens)`（`parser.ts:106-124`，第 109 行）。
- 去重键 = 时间戳+模型+五个 token 字段拼接（`parser.ts:126-129`），线性 `Set` 去重（`parser.ts:131-143`）。

v20：

- v19 的回退减法在 v20 里对应 `subtract_codex_raw_usage`（`rust/adapters/codex/src/parser.rs:1284-1308`，用 `saturating_sub` 且多了 `cache_creation_tokens` 一档），v20 同样以 `last_token_usage` 为主路径（`parser.rs:483-496` 先取 `total_token_usage` 做前移与 `last_token_usage` 缺失判断）。
- 状态机升级：`previous_totals` 字段（`parser.rs:89`）在 `parser.rs:483-496` 推进；新增 replay 判定分支 `CodexReplayState::SkippingRewrittenBurst`（`parser.rs:254-257`，用时间差判断重写突发）。
- 事件去重：`dedupe_codex_events`（`rust/adapters/codex/src/loader.rs:240-…`），测试 `dedupes_matching_codex_usage_events_from_distinct_sessions`（`loader.rs:552`）、`dedupes_compaction_usage_by_response_id`（`loader.rs:562`）、`dedupes_copied_branch_history_across_session_files`（`loader.rs:615`）。
- compaction/replay 是 v20 新增的**覆盖范围**（不是新增 `last_token_usage` 支持，两版都优先它）：`rust/adapters/codex/src/compaction_tests.rs:141-143` 等用例覆盖 `last_token_usage` 与 `total_token_usage` 并存的情况；`subtracts_cache_write_usage_from_cumulative_token_totals`（`rust/adapters/codex/src/loader.rs:943`）说明 v20 会从累计值里扣掉 cache write。
- 迁移对齐测试：`prices_codex_versioned_models_like_typescript_adapter`（`rust/crates/ccusage/src/main.rs:694`）、`prices_codex_gpt_5_4_like_typescript_adapter`（`main.rs:773`）。

**语义变化**：取值**顺序相同，但 v20 多一道门控**——两版都先看 `last_token_usage`；但 v20 会先用 `cumulative_advanced`（即 `total_token_usage` 是否与上一条不同；**该字段缺失时门控视为通过**）判定，**未前进时丢弃 `last_token_usage` 并转走累计差分**（`rust/adapters/codex/src/parser.rs:483-494`），而 v19 无此门控、`lastUsage` 存在即无条件采用（`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:205-212`）。因此「累计值未前进但 `last_token_usage` 存在」这类样本两版会给出不同结果——口径详见 §5.3。其余差异是 v20 增加了 replay/rewrite 状态机（跳过被重写的突发，`parser.rs:254-257`）、从累计值中扣减 cache write（`rust/adapters/codex/src/loader.rs:943`）以及 response id 层面的 compaction 去重（`loader.rs:562`）。跨会话相同事件两版都会去重（v19 的键不含 session，`ccusage-v19/apps/ccusage/src/adapter/codex/parser.ts:126-129`；v20 走 `dedupe_codex_events`，`loader.rs:240`）。

### 7.5 5 小时 block 算法

v19（`ccusage-v19/apps/ccusage/src/session-blocks.ts`）：

- 常量 `DEFAULT_SESSION_DURATION_HOURS = 5`（`session-blocks.ts:7`）。
- 首个 block 起点 = 首条消息时间**向下取整到整点**（`floorToHourMs`，`session-blocks.ts:45-47`；调用在 `:135` 与 `:165`）。
- 切块条件：`timeSinceBlockStart > 5h` **或** `timeSinceLastEntry > 5h`（`session-blocks.ts:146`）；后者还会插入 gap block（`:157-162`，`createGapBlock` 在 `:247-…`）。
- 活跃判定：`now - actualEnd < 5h && now < start + 5h`（`session-blocks.ts:202`）；`actualEndTime` 取块内最后一条消息时间（`:199-201`）。
- 汇总：块内 4 类 token 求和 + `costUSD` 求和 + 模型集合（`:204-237`）；burn rate / 预测在 `:284-357`。
- 测试常量另有一处硬编码 `SESSION_DURATION_MS = 5 * 60 * 60 * 1000`（`:373`）。

v20（`rust/crates/ccusage/src/blocks.rs` + `ccusage-core`）：

- 常量同值：`DEFAULT_SESSION_DURATION_HOURS: f64 = 5.0`（`rust/crates/ccusage-core/src/lib.rs:59`），`DEFAULT_RECENT_DAYS: i64 = 3`（`:60`）。
- `identify_session_blocks(entries, session_duration_hours, ...)`（`rust/crates/ccusage/src/blocks.rs:53-104`），`floor_to_hour`（`blocks.rs:105-107`），`create_block`（`blocks.rs:109-147`），`create_gap_block`（`blocks.rs:149-…`），gap 在 JSON 里以 `"isGap"` 暴露（`blocks.rs:211`）。
- 调用点固定用 5 小时常量：`rust/crates/ccusage/src/commands/mod.rs:573`、`:600`、`:659`（另外 `blocks --session-length` 可覆盖，`parser.rs:208-213`）。

**结论：算法结构一致**（整点对齐、双条件切块、gap 块、活跃判定），差异在实现细节与可配置项（v20 支持 `--session-length`、`--token-limit`，v19 的 blocks 命令另有自己的 token limit 解析 `ccusage-v19/apps/ccusage/src/commands/blocks.ts:106-114` 与默认最近天数常量 `ccusage-v19/apps/ccusage/src/consts.ts:9`）。

### 7.6 价格表来源与匹配

| 维度 | v19 | v20 |
| --- | --- | --- |
| 来源 | 仅 LiteLLM 原始文件（`ccusage-v19/packages/internal/src/pricing.ts:4-5`） | LiteLLM + models.dev 双源（`rust/crates/ccusage-core/src/pricing.rs:55-57`） |
| 离线数据 | 构建期 macro 预取，按 agent 过滤后内联（`ccusage-v19/apps/ccusage/src/pricing-fetcher.ts:14`、`ccusage-v19/apps/ccusage/src/adapter/codex/pricing-macro.ts:10-17`、`:19-31`） | 构建期压缩内嵌（deflate level 10，`rust/crates/ccusage-core/build.rs:58-62`；白名单 `:203-221`），运行期 inflate |
| 抓取 | `fetch(url)` + `response.json()`（`ccusage-v19/packages/internal/src/pricing.ts:180-225`） | `ureq` + 10s 超时 + ETag/304 + 64MiB 上限 + 形状校验（`rust/crates/ccusage/src/http.rs:11-14`、`:75-137`） |
| 缓存策略 | 进程内 `cachedPricing` + 单飞 promise（`pricing.ts:227-243`）；失败回落到本地离线数据（`:161-178`） | 磁盘 HTTP 缓存（条件请求）+ models.dev 60s 失败退避（`pricing.rs:58`） |
| 模型匹配 | 直接命中候选前缀（`pricing.ts:245-254`、Claude 前缀表 `ccusage-v19/apps/ccusage/src/pricing-fetcher.ts:6-12`）→ 否则全表小写 `includes` 双向匹配，取长度差最小者（`pricing.ts:272-288`） | `PricingMap::find`（`rust/crates/ccusage-core/src/pricing.rs:1354-1460`）、`find_exact`（`:1484`）、`find_exact_with_fallback`（`:1501`）；模糊扫描受「exact-only id」门控（注释 `pricing.rs:405-446`、`:1619-1665`），并要求模型键边界（测试 `fuzzy_match_requires_model_key_boundaries` `pricing.rs:5663`、`fuzzy_match_prefers_longest_model_key` `pricing.rs:5854`、`fuzzy_match_does_not_fall_back_across_numeric_model_versions` `pricing.rs:5706`、`fuzzy_match_allows_date_like_suffixes_for_known_numeric_model_versions` `pricing.rs:5735`） |
| 别名 | 无 | `model_without_date_suffix`（`pricing.rs:2767-…`）、别名表（`pricing.rs:2792-2824`）、`CCUSAGE_MODEL_ALIASES` 环境变量（`rust/crates/ccusage-core/src/model_aliases.rs:8-60`） |
| 分层计价 | `calculateTieredCost`（`ccusage-v19/packages/internal/src/pricing.ts:16-54`）+ `calculateCostFromPricing`（`pricing.ts:318-…`，200k 阈值字段） | `tiered_cost`（`rust/crates/ccusage-core/src/cost.rs:201-…`）、`calculate_cost_from_pricing`（`cost.rs:118-…`）、配置化 `*_above_200k_tokens`（`config_schema.rs:600-603`） |

**语义变化**：匹配从「双向 includes + 最短长度差」（容易把 `claude-opus-5` 与 `claude-opus-5-eu`、`-turbo` 等变体互相吞掉）收紧为「精确（含归一化/日期后缀/别名）优先，模糊需键边界且受 exact-only 门控」；同时新增第二价格源与磁盘级条件请求缓存。`rust/crates/ccusage/src/main.rs:694/773` 的 `..._like_typescript_adapter` 测试说明 Codex 侧仍以复刻 TS 结果为目标。

### 7.7 cost mode

v19（`ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1284-1336`）：

- `display`：永远用 `costUSD`，缺失记 0（`:1291-1294`）。
- `calculate`：永远按 token 重算；没有 model 名记 0（`:1296-1305`）。
- `auto`：有 `costUSD` 用它，否则重算，再否则 0（`:1307-1321`）。
- 另有「立即成本」快路径 `getImmediateCostForEntry`（`:1326-1336`），只对 display / 带 costUSD 的 auto 生效。
- 聚合口径 `calculateTotals` / `createTotalsObject`（`ccusage-v19/apps/ccusage/src/calculate-cost.ts:53-87`）。

v20（`rust/crates/ccusage-core/src/cost.rs`）：

- `calculate_cost_for_usage_at` 的 match（`cost.rs:31-46`）：`Display => cost_usd.unwrap_or(0.0)`（`:40`）、`Auto => cost_usd 优先否则计算`（`:41-43`）、`Calculate => calculate_cost_from_tokens`（`:44`）。
- 缺价标记：`missing_pricing_model_for_usage` 在 display、或 auto 且已有 costUSD 时返回 `None`（`cost.rs:48-59`），与 `docs/guide/json-output.md:59` 的说明一致。
- 新增倍率：cache creation 1h 输入倍率 2.0（`cost.rs:7`）、`fast`/`flex` 服务层级乘数（`rust/crates/ccusage-core/build.rs:186-195` 抽取 `provider_specific_entry.fast/flex`；测试 `rust/crates/ccusage/src/main.rs:723-770`、`:772-809`）。

**语义变化**：三态语义保持不变；v20 增加「未计价模型」显式信号（`unpricedModels` / `missingPricing`）与服务层级乘数，且 cost 来源可再细分（`--cost-source` 仅 statusline：`auto|ccusage|cc|both`，`parser.rs:1214-1222`）。

### 7.8 v20 相比 v19 的语义变化清单（汇总）

1. **运行时与分发**：Bun/Node 脚本 → 原生二进制；npm 包降级为 wrapper（`apps/ccusage/src/cli.js:93-109`、`:233-258`）。v19 的 `CCUSAGE_BUN_AUTO_RUN` 开关（`ccusage-v19/apps/ccusage/src/env.ts:1-2`）随 Bun 路径一起消失。
2. **并发模型**：v19 用 worker_threads + `CCUSAGE_JSONL_WORKER_THREADS`（`ccusage-v19/apps/ccusage/src/adapter/claude/data-loader.ts:1978` 等 4 处）与 `CCUSAGE_JSONL_READ_CONCURRENCY`（`data-loader.ts:208`）→ v20 用线程池 + `--single-thread`（`rust/crates/ccusage-cli/src/types.rs:56`）与按体积分块（测试 `balances_file_chunks_by_size`，`rust/crates/ccusage/src/main.rs:176-209`）。
3. **Claude 去重**：单键 `messageId[:requestId]` → 会话感知 + `msg_`/gateway 区分 + sidechain 重放键（§4.4）。
4. **Codex**：取值顺序不变（都先看 `last_token_usage`），但 v20 加了「累计值必须前进」的门控，未前进时改走累计差分 → 另新增 replay/rewrite 状态机、compaction 去重与 cache-write 扣减（§5.3）。
5. **日期输入**：仅 `YYYYMMDD` → 双格式 + 日历校验（§9.3）。
6. **CLI 能力**：新增 `--jq`、`--last`、`--sections`、`--by-agent`、`--no-cost`、`--all`；错误出口统一为 exit 2 + `Run 'ccusage --help' for usage.`（`rust/crates/ccusage/src/cli.rs:35-39`）。
7. **帮助系统**：gunshi 自动生成 → 构建期从两个 JSON 生成静态表（§9.2.4）。
8. **配置**：字段扩展到 18 个 agent 段 + 统一 `commands` + pricingOverrides + pi.stores；校验错误统一 `Invalid ccusage config: ...`（§9.1.5）。
9. **价格**：新增 models.dev 源、磁盘 ETag 缓存、别名与精确优先匹配、缺价信号（§8）。
10. **版本**：`package.json` 直接 import → 编译期 `CCUSAGE_VERSION` + 一致性测试（`rust/crates/ccusage/build.rs:5-18`、`main.rs:125-135`）。
11. **保留的 TS 兼容断言**（跨版本回归锚点）：`formats_windows_user_project_paths_like_typescript`（`rust/crates/ccusage/src/main.rs:150`）、`sorts_usage_files_like_typescript_path_strings`（`main.rs:212`）、`loads_daily_summaries_like_loaded_entry_aggregation`（`main.rs:483`）、`rejects_null_schema_fields_like_typescript_loader`（`rust/adapters/claude/src/lib.rs:1025`）、`allows_null_content_like_typescript_loader`（`lib.rs:1041`）。

---

## 未验证/存疑（本节同时覆盖配置/CLI/输出章与版本史章的未决项）

> 第 8 轮实测（§13.5）已覆盖本节第 1 条的一部分（`--jq` 不在帮助里，实测确认）与第 9 条（运行期状态）；
> 第 6 条（`blocks` JSON 字段全集）也已被 §13.5 的 `blocks --json` 实测补全。其余各条与 §13.5 无关，
> 仍未闭环。

> 第 8 轮实测复核（§13.5）已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目；此处保留原始清单与当时的判断，逐条状态以 §13.5 为准。

1. **`--jq` 是否应在帮助里出现**：`cli-help.json` 无 jq 条目（grep `jq` 只命中 parser/tests/snapshots），但选项确实可解析。无法从源码判断这是「故意隐藏」还是「漏写」，需要看 v20 的 release note 或 changelog（本仓未查）。
2. **`docs/guide/config-files.md`（614 行）未逐行精读**：只核对了它的 `$schema` 示例与 `jq` 校验说明（`:13`、`:593`），其中关于「多文件回退」的表述与实现（只取第一个可用文件）可能有出入，建议 S7/父代理核对。
3. **v19 → v20 的 entry 级 cost 计算路径**没有逐行对齐：v20 的 `cost.rs` 只读了结构段（`31-59`、`118`、`201`），`calculate_cost_from_tokens` 与 `find_at`（按事件时间选价，`pricing.rs:1461`）的内部规则未展开；若需要「某一具体模型两版钞票差异」级别的结论，需补读 `cost.rs:93-200`。
4. **v20 价格来源优先级**（外置 override / 内嵌 LiteLLM / models.dev / 运行期刷新）的**合并顺序**只读到零散注释（`pricing.rs:405-446`、`:1489-1500`、`:1631-1665`），未形成完整链路结论；`PricingMap::load_with_overrides`（`pricing.rs:1002`）本体未读。
5. **v19 的 `packages/internal/src/pricing-fetch-utils.ts`（47 行）未读**，构建期预取的过滤细节只有 `pricing-macro.ts` 的调用面。
6. **`ccusage blocks` 的 JSON 字段全集**未穷举：只确认 `isGap`（`blocks.rs:211`）与 `block_json`（`rust/crates/ccusage/src/main.rs:11` 导出），burn rate / 预测字段的键名未逐项核对。
7. **v20 `--last` 与配置 `since` 的交互**：`--last` 解析发生在 CLI 之后（`rust/crates/ccusage/src/cli.rs:29-31`，`last_window.rs:15-23`），会覆盖配置里已应用的 `since`；如果配置只给了 `until`，行为是「since 由 --last 推出 + until 保留」，此组合没有专门测试覆盖（`last_window.rs` 测试只覆盖了 `--last` 单独使用）。
8. **`--jq` 与 `--no-cost` 的组合**：`strip_cost_json` 在 jq 之前执行（`output.rs:159-161`），因此 jq 过滤器看不到被删字段——这是实现事实，但未在文档中声明。
9. **运行期验证状态**：第 1–7 轮为纯源码阅读；**第 8 轮已构建并运行 ccusage**（`--offline`，见 §13.5），本节与运行期相关的结论（CLI 解析、帮助文本、`--jq` 隐藏性）均已实测。行号基于 HEAD `57afdc2`（v20.0.31）与 `4aedac0`（v19.0.3），若仓库前进需重新核对。
