# S1 主干与 Claude Code 适配器（ccusage v20.0.31 精读）

> 只读仓库（未运行其二进制、未改动任何文件）：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`
> HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`，v20.0.31（Rust workspace：`rust/crates/*` + `rust/adapters/*`）。
> 路径相对仓库根；行号一律为 `read` 实测（任务书给的若干行数来自 `Measure-Object -Line` 口径，偏小：`cost.rs` 实测 664 / `adapter-all/loader.rs` 1637 / `pricing.rs` 6181）。
> 关键文件实测行数：`claude/src/lib.rs` 1582、`claude/src/daily.rs` 1182、`claude/src/paths.rs` 555、`ccusage/src/main.rs` 1063、`ccusage/src/commands/mod.rs` 1411、`adapter-all/src/loader.rs` 1637、`adapter-all/src/types.rs` 208、`core/src/cost.rs` 664、`core/src/types.rs` 219、`core/src/summary.rs` 759。

## S1 主干与 Claude Code 适配器

### 0. 主干坐标

```rust
match cli.command {
    Some(Command::All(args)) => adapter::all::run(args),
    Some(Command::Daily(args)) => commands::run_daily(args),
    Some(Command::Monthly(shared)) => commands::run_bucket(shared, BucketKind::Monthly),
    Some(Command::Weekly(args)) => commands::run_weekly(args),
    Some(Command::Session(args)) => commands::run_session(args),
```

证据：`rust/crates/ccusage/src/main.rs:28-33`；裸 `ccusage` 落到统一日报 `adapter::all::run`（`main.rs:53-65`）；19 个别名导出见 `rust/crates/ccusage/src/adapter/mod.rs:1-19`，统一层再导出 18 个适配器（`adapter-all/src/lib.rs:11-30`）。

### 1. 文件发现（数据根 / 环境变量 / 遍历 / 排序）

**1.1 `claude_paths()` 是唯一的数据根解析点**

```rust
if let Ok(env_paths) = env::var("CLAUDE_CONFIG_DIR") {
    for raw in env_paths.split(',').map(str::trim).filter(|path| !path.is_empty()) {
        let path = normalize_claude_config_path(raw);
        if path.join("projects").is_dir() && seen.insert(path.clone()) { paths.push(path); }
    }
    if !paths.is_empty() { return Ok(paths); }
    return Err(cli_error(format!("No valid Claude data directories found in CLAUDE_CONFIG_DIR. …
```

证据：`rust/adapters/claude/src/paths.rs:23-43`。

- **多根**：`CLAUDE_CONFIG_DIR` 按 `,` 切分、逐段 `trim`、丢空段（`paths.rs:26-31`）；去重靠 `seen`（`paths.rs:25`）。
- **优先级**：该变量一旦设置即权威——任一段有效就返回（`paths.rs:37-39`）；**全部无效直接报错**（`paths.rs:40-42`），不回落 `~/.claude`。
- **每段规范化**：末段名为 `projects` 且是目录时取父目录（`paths.rs:57-63`，`~` 由 `expand_home_path` 展开）；**必须以存在 `<root>/projects/` 为有效条件**（`paths.rs:33`）。端到端测试 `main.rs:382-396`。
- **无该变量时同时尝试两个根**（不是二选一）：

```rust
let home = home::home_dir().ok_or_else(|| cli_error("home directory is not set"))?;
let xdg = env::var("XDG_CONFIG_HOME").map(PathBuf::from)
    .unwrap_or_else(|_| PathBuf::from(&home).join(".config"));
for path in [xdg.join("claude"), home.join(".claude")] { … }
```

证据：`paths.rs:45-53`。

**1.2 文件枚举：递归、无忽略规则、按完整路径字符串排序**

证据：`rust/adapters/common/src/lib.rs:18-34`（`collect_files_with_extension`）。

- 递归无深度限制；`read_dir` 失败静默返回（`common/lib.rs:19-21`）；**无任何忽略规则**（不读 `.gitignore`、不跳隐藏目录）；只收扩展名恰为 `jsonl` 的文件（`common/lib.rs:28`）。
- 排序：`files.sort_by_cached_key(|path| path.to_string_lossy().into_owned());`（`paths.rs:77`）——**去重的输入之一**（谁先谁后决定幸存者）。测试 `main.rs:211-223`：父会话文件排在 `subagents/agent-a.jsonl` 之前。
- 项目过滤只接受单段目录名：`is_project_path_segment` 拒绝空串、`.`、`..` 与含 `/` `\` 的值（`paths.rs:248-254`）；非单段过滤值退化为全量扫描（`lib.rs:972-982`）。

**1.3 路径 → 会话/项目身份**

```rust
if relative.len() >= 4 && relative.get(relative.len() - 2) == Some(&"subagents") {
    let session_id = relative[relative.len() - 3].to_string();
    let project_path = relative[..relative.len() - 3].join(std::path::MAIN_SEPARATOR_STR);
```

证据：`paths.rs:303-319`（`extract_session_parts` 整体 `paths.rs:290-331`）。

- `projects/<p>/<session>.jsonl` → `(session, p)`（`paths.rs:303-307`）；`projects/<p>/session-a/subagents/agent-a.jsonl` → `("session-a", "p")`（测试 `lib.rs:1014-1022`）。
- **更深的 workflows 路径不命中 `subagents` 分支**：`projects/p/session-x/subagents/workflows/wf_1/agent-a.jsonl` 落到兜底（`paths.rs:320-330`）→ 会话名取 `wf_1`、项目路径 `p/session-x/subagents/workflows`；该文件归属只能靠行内 `sessionId` 纠正（测试 `paths.rs:517-538`）。
- 项目名取 `projects` 之后的第一段，缺失/空白 → `"unknown"`（`extract_project`，`paths.rs:269-288`）。

**1.4 会话身份的双来源**

```rust
fn loaded_entry_session_id(entry: &LoadedEntry) -> &str {
    entry.data.session_id.as_deref().unwrap_or(entry.session_id.as_ref())
}
```

证据：`rust/adapters/claude/src/lib.rs:405-411`——**行内 `sessionId` 优先，缺失才用路径推导值**；该函数同时被去重键与 sidechain 回放路由消费（见本章「去重规则」）。

### 2. 读取管线（路径 → `LoadedEntry`）

**2.1 调用链**

`load_entries`（`lib.rs:51-55`）→ `load_entries_inner(shared, filter, false)`（`lib.rs:107-188`）；`load_entries_since`（`lib.rs:67-74`）→ 同一内部函数，第三参 `true`。

```rust
let paths = claude_paths()?;
…
let files = usage_files(&paths, project_filter);
let files = if skip_files_before_since {
    split_files_before_since(files, shared, utc_now()).kept
} else { files };
```

证据：`lib.rs:112-130`。随后：价格表（`lib.rs:144-152`）→ 时区（`lib.rs:153`）→ `read_files_parallel`（`lib.rs:155-157`，闭包即 `read_usage_file`）→ 逐条去重（`lib.rs:170-187`，`push_deduped_entry`）。两个入口都用 `progress::track_usage_load(UsageLoadAgent("Claude"), shared.json, …)` 包裹（`lib.rs:52`、`lib.rs:71`）。

**2.2 并发模型：按文件大小贪心均分的线程组**

```rust
let worker_count = if single_thread { 1 } else {
    thread::available_parallelism().map(usize::from).unwrap_or(1).min(files.len())
};
if worker_count <= 1 { return files.iter().map(|file| read(file.as_path())).collect(); }
let chunks = chunk_file_indexes_by_size(files, worker_count);
```

证据：`rust/adapters/common/src/lib.rs:89-101`（`read_files_parallel` 整体 `84-126`）。`chunk_file_indexes_by_size`（`common/lib.rs:49-81`）按 `metadata().len()` 降序（同大小按索引升序，`59-62`）依次投给累计字节最小的块（`66-75`），滤掉空块（`77-80`），`chunk_count.max(1)` 防空（`53`）。结果按原文件顺序回填（`common/lib.rs:113-124`），保证「文件顺序 = 路径排序」在并行下仍成立（测试 `main.rs:176-209`、`common/lib.rs:133-153`）。

> daily 路径**没有复用**该函数，而是复制了一份等价实现：`daily.rs:236-283`（同样的 `available_parallelism().min(files.len())` + 分块 + `thread::scope`）。

**2.3 单文件是「整文件读入 + 字节行切片」，不是流式**

```rust
let Ok(content) = fs::read(path) else { return loaded_file; };
let usage_marker = memmem::Finder::new(br#""usage":{"#);
for line in byte_lines(&content) {
    if usage_marker.find(line).is_none() { continue; }
    let Some(data) = deserialize_usage_line::<UsageEntry>(line) else { continue; };
```

证据：`lib.rs:505-516`（`read_usage_file` 整体 `lib.rs:491-603`）。

- 无 `BufReader`、无流式：一次 `fs::read` 读完整文件；`byte_lines` 只在字节切片上找 `\n`（`core/src/fast.rs:82-103`），无每行 `String` 分配；`memmem` 预过滤在解析前逐行筛（设计说明 `fast.rs:16-70`）。
- **预过滤是精确字节匹配 `"usage":{`**（`lib.rs:509`，daily 同 `daily.rs:302`）。写成 `"usage": {`（带空格）的行会被**整行静默丢弃**。
- **不做时间戳预扫描**：`timestamp_from_line`（`paths.rs:256-267`，memmem 找 `"timestamp":"`）只被测试引用（`main.rs:249-260`），是 TS 兼容的 `#[doc(hidden)]` 导出（`lib.rs:33-34`），不在装载管线上；逐行时间戳在解析后统一处理（`lib.rs:517-519`，失败即丢行）。

**2.4 大文件优化：整文件级 mtime 剪枝 + 首行扫描，没有字节级尾部读**

`const MTIME_PRUNE_MARGIN_MS: i64 = MILLIS_PER_DAY;`（`paths.rs:84`），阈值 = `since_ms - 1 天`（`paths.rs:137`）。规则（`paths.rs:117-172`）：`since` 缺失/不可解析 → 全保留（`122-131`）；窗口起点在将来 → 全保留（`134-136`）；mtime 读不到 → 保留（`156` 的 `is_none_or`）；剪枝**按会话 ID 连通分量整组进行**——用路径会话名 + `recorded_session_id`（读文件直到第一条含 `"sessionId"` 的行，`paths.rs:210-234`，含 `memmem` 预筛 `224`）建并查集（`paths.rs:174-205`），组内任一文件新鲜则整组保留（`139-171`），注释解释了原因：子代理/工作流会回放父消息，单独丢父文件会让回放被双算（`paths.rs:98-106`）。测试：`paths.rs:387-538`（margin / 无 since / 未来窗口 / mtime 不可读 / 父子同组 / 跨配置根与跨目录 `sessionId`）。

statusline 侧另有一处「少读一点」：上下文窗口计算整读 transcript 后**从尾部反向**找最后一条 `type == "assistant"` 且带 `usage` 的行（`commands/mod.rs:719-731`、`738-751`），并用 mtime + 刷新间隔缓存结果（`commands/mod.rs:337-343`、`826-845`；缓存文件 `%TEMP%/ccusage-semaphore/<session>.lock`，`860-864`）。

**`LoadedFile.timestamp` 是死字段（已验证）**：唯一构造点 `claude/src/lib.rs:501-504`（`LoadedFile { timestamp: None, entries: Vec::new() }`），写入点 `lib.rs:660-666`；daily 侧同构（`DailyLoadedFile` 构造 `daily.rs:294-297`、写入 `daily.rs:314-318`）。消费者只读 `.entries`（`lib.rs:158-181` 的 `loaded_entry_count`/去重循环、`daily.rs:100-101`），全仓无任何读取该字段的代码——即两处写入均为死代码。

### 3. 解析与容错

**3.1 JSON 入口与 null 契约**

```rust
pub(crate) fn deserialize_usage_line<T: DeserializeOwned>(line: &[u8]) -> Option<T> {
    if memmem::find(line, b"null").is_none() { return serde_json::from_slice(line).ok(); }
    let root = serde_json::from_slice::<UniqueJsonValue>(line).ok()?.0;
    if has_unsupported_null_field_in_value(&root) { return None; }
    serde_json::from_value(root).ok()
}
```

证据：`lib.rs:727-736`。无 `null` 的行走零拷贝 `from_slice` 快路径（`728-730`）；含 `null` 的行先解析成 `UniqueJsonValue`（`lib.rs:741-830`），它**保留 Serde 对重复成员名的拒绝**（`819-824` 遇重复即报错），防 `serde_json::Value` 的「后者覆盖前者」把畸形行洗白（注释 `738-740`，测试 `lib.rs:1136-1142`）。不可为 `null` 的字段清单：`id | cwd | model | speed | costUSD | version | sessionId | requestId | isApiErrorMessage | cache_read_input_tokens | cache_creation_input_tokens`（`lib.rs:867-886`），命中即丢整行（`732-734`）；唯一豁免是 `usage.iterations[]` 元素的 `model`，范围严格限定在 `usage` 的直接 `iterations` 成员（`lib.rs:832-841`、`849-854`，测试 `lib.rs:1082-1127`）。

**3.2 坏行：静默跳过，无计数无告警**

`lib.rs:514-516`（反序列化失败 `continue`）、`lib.rs:517-519`（时间戳不可解析 `continue`）。共同库明说这是历史行为（`adapters/common/src/jsonl.rs:29-30`）。**TokenScope 的 SF08 要求 bad_lines 计数，此处必须本地补。**

**3.3 `type` / `isApiErrorMessage` / `isSidechain` 都不是过滤条件**

- `UsageEntry` **没有 `type` 字段**（`core/src/types.rs:9-19`），因此不存在 `type == assistant` 白名单；`user`/`summary`/`system` 行只会在「缺 `message`/`usage`」或字段校验处被挡掉。
- `isApiErrorMessage` 只用于识别额度耗尽时间：`lib.rs:533-534` → `usage_limit_reset_time_from_line_bytes`（`lib.rs:918-943`：必须 `Some(true)`，文本含 `Claude AI usage limit reached`，其后 `|` 跟纯数字秒级时间戳且 > 0）。该行**仍作为一条 usage entry 参与统计**，没有跳过分支。
- `isSidechain` 只影响去重优先级与回放路由：`is_sidechain_usage_entry` = `is_sidechain == Some(true)`（`lib.rs:442-444`）。

**3.4 `usage` 缺失 / 字段缺失**

```rust
pub struct TokenUsageRaw {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)] pub cache_creation_input_tokens: u64,
    #[serde(default)] pub cache_read_input_tokens: u64,
    pub speed: Option<Speed>,
    #[serde(default)] pub cache_creation: Option<CacheCreationRaw>,
}
```

证据：`core/src/types.rs:28-39`。

- 缺 `message` 或缺 `message.usage`：因两者都是必填（`types.rs:13`、`types.rs:23`）→ 整行失败（且多数已被 `"usage":{` 预过滤挡掉）。
- `usage` 存在但缺 `input_tokens`/`output_tokens` → serde 缺字段错误 → **整行丢弃**（这两个字段没有 `#[serde(default)]`，`types.rs:30-31`）；缺两个 cache 字段 → 视作 0（`types.rs:32-35`）。
- `speed` 只接受小写 `standard`/`fast`（`types.rs:36` + `types.rs:60-65` 的 `rename_all = "lowercase"`）；其它取值（如 `flex`）会让整行反序列化失败并被静默跳过。

**3.5 行级有效性校验**

`is_valid_usage_entry`（`lib.rs:668-707`）依次拒绝：`version` 存在但不是语义化版本前缀（`is_semver_prefix`，`lib.rs:888-900`）、`sessionId` 空串、`requestId` 空串、`message.id` 空串、`model` 空串；**其余一律接收，包括 total 为 0 的行**——0 token 行不在装载入口过滤，而由**下游两处**各自丢弃：
- 会话报表：四列全 0 的会话被剔除（`commands/mod.rs:155-161`，见 §10.3）。
- 统一报表（adapter-all）：`summary_rows` 丢弃 `total_tokens == 0 && total_cost == 0.0` 的行（`loader.rs:786-789`，见 §9.4）；注意是「且」——0 token 但有 cost 的行（例如只有 credits 的 Copilot 行）会被保留。
- 例外：`claude daily` 直接走 `load_daily_summaries`，不经过 adapter-all 的 `summary_rows`，所以 0 token 行会进入日汇总（贡献 0）。

### 4. 去重规则（重点）

去重是**单遍流式**：按「排序后的文件顺序 → 文件内行序」逐条 `push_deduped_entry`（`lib.rs:170-187`），索引结构 `FxHashMap<u64, SmallVec<[DedupeIndex; 1]>>`（`lib.rs:40-45`、`lib.rs:170`）；同键冲突时不新增，按规则决定是否**原地替换**。

**4.1 精确键 `usage_dedupe_hash`**

```rust
fn usage_dedupe_hash(message_id: &str, request_id: Option<&str>, session_id: &str, timestamp: TimestampMs) -> u64 {
    let mut hasher = FxHasher::default();
    message_id.hash(&mut hasher);
    request_id.hash(&mut hasher);
    if request_id.is_none() {
        session_id.hash(&mut hasher);
        if !is_anthropic_message_id(message_id) { timestamp.hash(&mut hasher); }
    }
    hasher.finish()
}
```

证据：`lib.rs:350-366`。展开：

| 情形 | 去重键 = | 语义 / 测试 |
| --- | --- | --- |
| `requestId` 存在 | `message.id` + `requestId`（**不含会话、不含时间戳**） | 同一 `requestId` 的跨会话副本合并（`lib.rs:1331-1352`） |
| 无 `requestId`，`message.id` 以 `msg_` 开头 | `message.id` + `session_id` | 同会话跨时间戳合并（Claude Code 一条回复按 content block 写多行；`lib.rs:1256-1280`） |
| 无 `requestId`，非 `msg_` 前缀 | `message.id` + `session_id` + `timestamp` | 网关复用同一 `message.id` 的每次调用各自成条（`lib.rs:1224-1254`、`1282-1306`） |

`is_anthropic_message_id` 只是 `starts_with("msg_")` 的启发式（`lib.rs:368-375`：只有 `requestId` 才证明两行属同一响应）。哈希命中后还要过结构比对（`lib.rs:413-425`）：`message.id` 相等 **且** `request_id` 相等 **且**（有 requestId 即通过；否则要求会话相等且（是 `msg_` 前缀或时间戳相等））。

**4.2 重复行保留哪一条（"most complete"）**

```rust
fn should_replace_deduped_entry(candidate: &UsageEntry, existing: &UsageEntry) -> bool {
    let (c, e) = (is_sidechain_usage_entry(candidate), is_sidechain_usage_entry(existing));
    if c != e { return e; }
    let (ct, et) = (usage_token_total(candidate), usage_token_total(existing));
    if ct != et { return ct > et; }
    candidate.message.usage.speed.is_some() && existing.message.usage.speed.is_none()
}
```

证据：`lib.rs:198-212`（本文合并了原行的 `let` 绑定）。`usage_token_total` = `input + output + cache_creation_token_count() + cache_read`（`lib.rs:190-196`）。优先级：① 非 sidechain 胜 sidechain；② token 总数大者胜；③ 平手时「带 `speed`」者胜。**主装载器不比较 cost**（与 daily 不同，见 §4.6）。用户可见效果：同一响应被写两遍时保留完整那次（测试 `main.rs:262-283` 断言留下 `output_tokens: 250` 与 `costUSD 0.01`）。

**4.3 不同 session 复用同一 `message.id` 为什么不合并**

因缺 `requestId` 时键里含 `session_id`（`lib.rs:359-363`），每个会话各自成条。测试 `main.rs:286-309`（两会话、同 id、同时间戳 → 2 条）、`lib.rs:1195-1222`（网关 id `ocgo` 两会话 → 2 条，input 合计 400）。反之，若两文件**行内 `sessionId` 相同**（转写副本）则合并为 1 条（`lib.rs:1308-1329`）——这正是「会话身份取行内、路径兜底」的直接后果（`lib.rs:405-411`）。

**4.4 sidechain 回放：两条额外路由**

```rust
fn sidechain_replay_dedupe_hash(message_id: &str, session_id: &str) -> u64 { … "sidechain-replay" … }
fn sidechain_entry_replay_dedupe_hash(message_id: &str, session_id: &str) -> u64 { … "sidechain-replay-entry" … }
```

证据：`lib.rs:377-392`（两个域分隔字符串）。查找逻辑 `lib.rs:237-266`：候选是 sidechain 走 `sidechain-replay` 桶，否则走**只装 sidechain 条目**的 `sidechain-entry` 桶（注释 `238-241` 解释这是为了不让「网关给每个响应复用一个 message ID」触发全表回扫）。匹配条件（`lib.rs:427-440`）：`message.id` 相等 **且**（双方都无 requestId → 可跨时间戳；否则要求时间戳相等）**且** 至少一边是 sidechain——**daily 侧缺「时间戳相等」这一守卫，见 §4.6 第 4 条**。配套别名维护：跨会话副本可能成为幸存者，需把两个会话的路由都登记到同一条目（`lib.rs:270-286`；`push_deduped_session_alias` 在 `474-489`），替换幸存者时只登记发生变化的键（`287-316`，理由见注释 `289-290`）。测试矩阵：`lib.rs:1354-1396`、`1398-1441`、`1443-1491`、`1493-1535`。

**4.5 子代理文件（`subagents/agent-*.jsonl`）如何避免双算**

机制不是文件名特判，而是「路径身份 + 两条键」共同生效：路径层面子代理文件与父转写同会话（`paths.rs:308-312`），行内层面通常也带父会话 `sessionId`（`lib.rs:405-411`）；于是同 `message.id` + 同 `requestId` 的回放命中精确键（键不含会话，直接合并），无 requestId 的同 `message.id` 回放在同会话内合并。
端到端证据：`rust/crates/ccusage/tests/claude_cli.rs:632-661`——父文件 `msg-parent`（input 10 / cache_read 20）+ `subagents/agent-a.jsonl` 同 id、`isSidechain: true`、cache_read 50000，四个报表总计都必须等于 32（sidechain 那份被合并）。另一形态见 `main.rs:537-567`（父文件里 `type: progress` + `data.message.message` 包装的同一条消息与 subagents 副本合并，模型顺序 `["claude-opus-4-6-fast","claude-haiku-4-5-20251001","claude-opus-4-6"]`），以及跨目录副本 `claude_cli.rs:529`、`paths.rs:517-538`。

**4.6 主装载 vs daily 快捷路径的去重差异（重要）**

| 维度 | `read_usage_file` + `push_deduped_entry` | `read_daily_usage_file` + `push_deduped_daily_entry` |
| --- | --- | --- |
| 键函数 | `usage_dedupe_hash`（`lib.rs:350-366`） | **同一个**（`daily.rs:457-462`、`571-580`） |
| 幸存者选择 | sidechain → token 总数 → speed（`lib.rs:198-212`） | 同前，**但 token 总数平手时再比 cost 大者胜**（`daily.rs:597-599`） |
| 行形态 | 只认顶层 `message.usage`（`UsageEntry`） | 顶层 **或** `data.message.message.usage`（`DailyUsageLine` 无标签联合，`daily.rs:182-204`） |
| `extra_total_tokens` | 聚合时累加（`core/src/summary.rs:54-57`） | 聚合不累加、汇总恒 0（`daily.rs:617-644`、`daily.rs:660`） |
| sidechain 回放路由 | 路由匹配**含 timestamp 守卫**：只有双方都无 requestId 才允许跨时间戳，否则要求 `entry.timestamp == timestamp`（`lib.rs:436-440`） | **缺 timestamp 守卫**，只比对 `message.id` + 会话 + 「至少一边 sidechain」（`daily.rs:493-496`） |

第 4 条是**是否合并**的差异，比上述三条平局裁决差异更实质：对「父条目 + 带**新** requestId、时间戳已被改写」的回放副本，daily 会合并、主装载路径不会。daily 侧有一条测试**显式固化**了这种行为（`daily.rs:731-769`：同 `msg-parent`、新 requestId、时间戳 1_774_000_000_000 vs …001_000 → 合并为 1 条）；主装载侧的对应测试（`lib.rs:1443-1491`）所有条目共用同一时间戳，因此**没有**覆盖反例。

结论：等价只**在测试覆盖的样本形态下**成立，不是结构保证（差异见本章第 8 小节）。

### 5. advisor / iterations / speed

**5.1 `usage.iterations` 里的 advisor 被拆成独立 entry**

```rust
pub(crate) fn advisor_usages_from_line(line: &[u8]) -> Vec<AdvisorUsage> {
    if memmem::find(line, br#""advisor_message""#).is_none() { return Vec::new(); }
    let Ok(envelope) = serde_json::from_slice::<UsageIterationsEnvelope>(line) else { … };
    envelope.message.usage.iterations.into_iter().filter_map(|iteration| {
        (iteration.kind == "advisor_message").then_some(iteration.model).flatten()
            .filter(|model| !model.is_empty())
            .map(|model| AdvisorUsage { model, usage: iteration.usage })
    }).collect()
}
```

证据：`lib.rs:635-658`（信封与元素类型 `lib.rs:605-628`，`UsageIteration` 用 `#[serde(flatten)] usage: TokenUsageRaw`，`lib.rs:621-628`）。要点：先 `memmem` 字节预筛再反序列化（`636-638`）；**只有 `type == "advisor_message"` 被拆条**（`type == "message"` 的迭代不拆，父行 `usage` 已含）；`model` 缺失或为空则跳过该迭代（`649-651`）。拆条实现（`lib.rs:559-600`）：

```rust
let mut advisor_data = entry.data.clone();
advisor_data.message.id = advisor_data.message.id.map(|id| format!("{id}:advisor:{index}"));
advisor_data.message.model = Some(advisor.model.clone());
advisor_data.message.usage = advisor.usage;
advisor_data.cost_usd = None;
```

- `:advisor:{index}` 后缀让 advisor 条目天然不撞父条目去重键（`lib.rs:561-565`）。
- `cost_usd = None`（`568`）→ advisor **永远自算价**（`lib.rs:583-590`），于是 `--mode display` 下 advisor 计价为 `cost_usd.unwrap_or(0.0) = 0.0`（`core/src/cost.rs:40`）——**display 模式会漏掉 advisor 花费**。
- `missing_pricing_model` 独立计算（`569-575`），`usage_limit_reset_time` 继承父条目（`595`）。
- 测试：`main.rs:399-421`（1 行 → 2 条，advisor 保留自己的 159419/7805）、`lib.rs:1163-1193`（advisor 用 advisor 模型计价 26.0，父条目保留 `costUSD 1.23`）、daily 侧 `main.rs:423-447`（日汇总 input 159421 / output 8296）。
- daily 路径同构：`daily.rs:368-398`（id 后缀在 `daily.rs:392-394`）。

**5.2 `speed: fast`**

```rust
let model = data.message.model.as_ref().and_then(|model| {
    if model == "<synthetic>" { None }
    else if matches!(data.message.usage.speed, Some(Speed::Fast)) {
        Some(suffix_string(model, "-fast"))
    } else { Some(model.clone()) }
});
```

证据：`lib.rs:535-543`（daily 镜像 `daily.rs:338-346`；`suffix_string` = 拼接，`core/src/fast.rs:105-110`）。

- `<synthetic>` 归一为 `None`（不显示、不进模型分组）。
- **展示/分组名加 `-fast`，计价不靠后缀**：

```rust
let multiplier = if matches!(usage.speed, Some(Speed::Fast)) { pricing.fast_multiplier } else { 1.0 };
calculate_cost_from_pricing(usage, pricing) * multiplier
```

证据：`core/src/cost.rs:110-115`（`fast_multiplier` 也可被外置覆盖，`ccusage-cli/src/types.rs:295`）。坑：缺价判定用**原始** `data.message.model`（`lib.rs:526-532`），而模型分组消费带后缀的 `entry.model`（`core/src/summary.rs:65-66`），因此同一物理模型会以 `x-fast` 与 `x` 两个名字出现在报表里；`resolve_model_name` 的 `-fast` 分支可用于对齐（`core/src/model_aliases.rs:14-26`）。

### 6. cache token 语义

**6.1 取值优先级：`cache_creation` 对象 > 扁平字段**

```rust
impl TokenUsageRaw {
    pub fn cache_creation_token_count(&self) -> u64 {
        if let Some(b) = &self.cache_creation {
            b.ephemeral_5m_input_tokens.saturating_add(b.ephemeral_1h_input_tokens)
        } else { self.cache_creation_input_tokens }
    }
}
```

证据：`core/src/types.rs:41-50`（`CacheCreationRaw` 在 `types.rs:52-58`，两字段均 `#[serde(default)]`）。所有聚合出口都用它：`TokenCounts::add_usage`（`types.rs:78-87`）、去重用的总数（`lib.rs:190-196`）、daily（`daily.rs:443-448`）。

**6.2 计价里的 5m/1h 分叉与 1h 加倍**

```rust
let (cache_create_5m_tokens, cache_create_1h_tokens) = if let Some(breakdown) = usage.cache_creation {
    (breakdown.ephemeral_5m_input_tokens, breakdown.ephemeral_1h_input_tokens)
} else { (usage.cache_creation_input_tokens, 0) };
let cache_create_1h_cost = pricing.input * CACHE_CREATE_1H_INPUT_MULTIPLIER;
```

证据：`core/src/cost.rs:118-131`，`CACHE_CREATE_1H_INPUT_MULTIPLIER = 2.0`（`cost.rs:7`）。没有 `cache_creation` 对象时，扁平字段全按 5m 桶（`cache_create` 单价）计，1h 桶为 0。

**6.3 `input_tokens` 是否含 cache —— 不含，四桶相加**

- 桶相加的权威实现：`TokenCounts::add_usage`/`total`（`core/src/types.rs:78-95`）与 `total_usage_tokens`（`core/src/utils.rs:14`）都把 `input + output + cache_creation + cache_read` 当**独立桶**。
- 直接文字依据：`core/src/cost.rs:139-142` 注释「`input_tokens` here is the uncached remainder - adapters normalize usage into the Claude shape」。
- 反向印证：其它适配器的原始字段含 cache，需显式扣减（Copilot：`docs/guide/copilot/index.md:31`；Codex：本章 §9.2）。Claude 侧 `input_tokens` 直接落列，不做扣减（`lib.rs:524-525`、`544-558`）。

### 7. 成本取值

**7.1 `--mode` 默认 `auto`**

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CostMode { #[default] Auto, Calculate, Display }
```

证据：`rust/crates/ccusage-cli/src/types.rs:242-248`；`SharedArgs::with_defaults()` 再次显式 `mode: CostMode::Auto`（`types.rs:62-71`）；CLI 只接受 `auto|calculate|display`（`ccusage-cli-parser/src/parser.rs:1134-1141`，赋值 `parser.rs:753`）。

**7.2 三分支本体**

```rust
match mode {
    CostMode::Display => cost_usd.unwrap_or(0.0),
    CostMode::Auto => cost_usd.unwrap_or_else(|| calculate_cost_from_tokens(model, usage, timestamp, pricing)),
    CostMode::Calculate => calculate_cost_from_tokens(model, usage, timestamp, pricing),
}
```

证据：`core/src/cost.rs:39-46`（`calculate_cost_for_usage_at`，`cost.rs:31-46`；`calculate_cost` 只是拆包转发 + `parse_ts_timestamp`，`cost.rs:9-18`）。

- **display**：只信 `costUSD`，无值即 0，从不查价、无兜底；且装载层在 display 下**根本不加载价格表**（`lib.rs:144-152`，daily 镜像 `daily.rs:45-53`）。
- **auto**：`costUSD` 有值一律采用，缺席才按 token 自算。
- **calculate**：永远自算，忽略 `costUSD`。缺价标记 `missing_pricing_model_for_usage`（`cost.rs:48-59`）在 display 或「auto 且有 costUSD」时返回 `None`，否则查价失败返回别名归一后的模型名（`cost.rs:61-75`）。

**7.3 Claude 链路调用点**

| 路径 | 调用 | 行号 |
| --- | --- | --- |
| 主装载主行 | `calculate_cost(&data, mode, pricing)` | `lib.rs:525` |
| 主装载 advisor | `calculate_cost_for_usage_at(Some(model), usage, None, Some(ts), …)` | `lib.rs:583-590` |
| daily 主行 | `calculate_cost_for_usage_at(model, usage, cost_usd, Some(ts), …)` | `daily.rs:323-330` |
| daily advisor | 同上，`cost_usd = None` | `daily.rs:382-389` |
| statusline 会话花费 | `load_entries` + 按会话过滤 + `sum(entry.cost)` | `commands/mod.rs:673-682` |

时间戳进入计价只为按事件时间选价（`cost.rs:102-106`，`find_at` / `find`）。

**7.4 相关开关**：`--offline` 只影响价格表是否联网刷新（`core/src/pricing.rs:1002-1038`：先 `load_embedded()`，`!offline` 才 `fetch_pricing_json`，失败仅告警回退内嵌；`enable_models_dev_fallback = !offline`）；`--no-cost` **不参与装载**，只在输出层剥离 cost（`core/src/output.rs:158-161` 调 `strip_cost_json`（`output.rs:564`），表格分支 `249/304/369/512`）；统一装载层自己再装一份价格表（`adapter-all/src/loader.rs:83-89`）。

### 8. 日汇总快捷路径

**8.1 确有独立实现**

`load_daily_summaries`（`lib.rs:78-84`）→ `load_daily_summaries_with_detection`（`lib.rs:97-105`）→ `daily::load_daily_summaries_inner`（`daily.rs:30-76`）。它**不调用** `load_entries`，而是自建「读文件 → 行级去重 → 按天分桶累加」链路：`daily.rs:35-62`（`claude_paths` → `usage_files` → `split_files_before_since` → `summarize_daily_files`）。累加器 `DailyAccumulator`（`daily.rs:607-670`）、按天 `BTreeMap` 分组（`daily.rs:131-146`）、`group_by_project` 时按 `(date, project)` 分组（`daily.rs:112-129`）。它额外返回 `detected`：即使文件全被 `--since` 剪掉，只要被剪掉的文件里确有 entry 也算「检测到 Claude 用量」（`daily.rs:63-75`）。

**8.2 两个实现共用的部件**

共用：`claude_paths`/`usage_files`/`split_files_before_since`（导入 `daily.rs:22-28`，调用 `daily.rs:35-37`）、`deserialize_usage_line`（`daily.rs:307` vs `lib.rs:514`）、`advisor_usages_from_line`（`daily.rs:368` vs `lib.rs:560`）、`usage_dedupe_hash`/`is_anthropic_message_id`/两条 sidechain 哈希（`daily.rs:457-462`、`472`、`482-484`）、`extract_project`/`extract_session_parts`（`daily.rs:291-292`）、去重索引助手（`daily.rs:22-28`）。
未共用（各一份副本）：`is_valid_usage_entry`（`daily.rs:403-442`）、`should_replace_*` 与 `*_usage_token_total`（`lib.rs:190-212` vs `daily.rs:443-448`、`582-601`）、聚合累加器（`core/src/summary.rs:39-123` vs `daily.rs:607-670`）。

**8.3 源码里有等价性测试（3 个单元 + 1 组 CLI 级）**

```rust
let entries = load_entries(&shared, None).unwrap();
let daily = load_daily_summaries(&shared, None, false).unwrap();
let expected_daily = summarize_by_key(&entries, |entry| entry.date.clone(), |key| (key.to_string(), None)).unwrap();
assert_eq!(daily.iter().map(summary_json).collect::<Vec<_>>(),
           expected_daily.iter().map(summary_json).collect::<Vec<_>>());
```

证据：`main.rs:465-478`（`daily_summaries_match_entry_aggregation_for_requestless_rewrites`）。同型断言：`main.rs:483-535`（普通 + `group_by_project` 分组日汇总，`503`、`516-534`）、`main.rs:570-595`（progress 包装行与 subagents 副本并存仍一致）。CLI 级：`tests/claude_cli.rs:603-629` 的 `claude_report_totals` 对 daily/weekly/monthly/session 四报表取 `totals.totalTokens` 比较，被 `claude_cli.rs:632/664/688/718` 复用。

**8.4 等价性的边界（TokenScope 必读）**

- 结构差异 1：daily 能解析 `type: progress` 包装行（`DailyUsageLine::AgentProgress`，`daily.rs:182-204`），主装载器只认顶层 `message`（`UsageEntry`，`core/src/types.rs:9-19`）。在「progress 行有 subagents 文件副本」的形态下两者收敛（`main.rs:570-595` 断言相等）；**若某条 progress 行没有任何副本，两条路径会分叉（按代码推断，未实测）**。
- 结构差异 2：daily 在 token 总数平手时用 cost 决胜（`daily.rs:597-599`），主装载器不比 cost（`lib.rs:198-212`）。
- 结构差异 3：daily 聚合不累加 `extra_total_tokens`（`daily.rs:660`），主路径累加（`core/src/summary.rs:54-57`、`113`）；Claude 侧该字段恒为 0（`lib.rs:552`、`591`），当前无影响。
- **结构差异 4（改变是否合并，最实质）**：daily 的 sidechain 回放路由**缺 timestamp 守卫**——`daily.rs:493-496` 只比对 `message.id` + 会话 + 「至少一边是 sidechain」，而主路径 `lib.rs:436-440` 还要求「双方都无 requestId 或时间戳相等」。`lib.rs:434-435` 的注释本身就承认了这个差异（「Requestless replays match across timestamps, as in the daily loader. Replays that carry a new request ID still need the parent's timestamp **in this loader**」）。后果：同一样本经 `load_daily_summaries` 与 `load_entries + summarize_by_key` 会得到不同总量（daily 合并、主路径不合并）；daily 有测试固化该行为（`daily.rs:731-769`），主路径侧无覆盖反例（`lib.rs:1443-1491` 共用同一时间戳）。
- 结论：等价只在**测试覆盖的样本形态下**成立，不是结构保证；改任一侧的去重/解析都必须重跑 `main.rs:449-595` 与 `tests/claude_cli.rs:603-741`，并针对「新 requestId + 改写时间戳的 sidechain 回放」补一条跨实现对照用例。

### 9. 统一报告（adapter-all）

**9.1 顶层装载器与并发**

`load_rows`（`loader.rs:29-37`）→ `load_pricing`（`83-89`）→ `load_kind_for_report`（`91-98`，weekly/monthly 归并为按日装载）→ `load_base_rows`（`100-396`）→ `finish_rows`（`398-420`）；`load_sections`（`39-81`）为多段报表复用同一批 base rows。18 个 agent 各一条 `AgentLoadSpec`（装配 `116-358`，agent 名取 `BUILT_IN_AGENT_NAMES[i]`，`119`），随后追加用户自定义 `pi.stores`（`359-380`）。汇总标识表 18 项：

```rust
pub const BUILT_IN_AGENT_NAMES: &[&str] = &[
    "claude", "codex", "opencode", "amp", "droid", "codebuff", "hermes", "pi", "goose",
    "openclaw", "kilo", "copilot", "gemini", "antigravity", "kimi", "qwen", "grok", "zcode",
];
```

证据：`core/src/lib.rs:63-82`（本文压缩换行）。**并行模型：一个 agent 一个 OS 线程**（`thread::scope` + `mpsc`），全仓无 rayon 依赖：

```rust
thread::scope(|scope| {
    let (sender, receiver) = mpsc::channel();
    for spec in specs {
        let sender = sender.clone();
        handles.push((spec.index, spec.progress_agent, scope.spawn(move || {
            let result = (spec.load)();
            let _ = sender.send((spec.index, spec.agent, spec.progress_agent, result));
        })));
    }
```

证据：`loader.rs:430-443`。错误按 `index` 排序后**只抛第一个**（`472-475`），成功结果按 `index` 回填保证顺序稳定（`477`），线程 panic 也转成错误（`465-470`）。子装载器收到 `json: true` 的克隆以抑制各自进度输出（`loader.rs:112-115`），进度条由外层统一驱动（`105-111`、`426-428`、`450-465`）。

**9.2 token 列对齐：统一列就是 `AllRow`，只有 Codex 做扣减**

统一计量列：`input_tokens / output_tokens / cache_creation_tokens / cache_read_tokens / total_tokens`（`adapter-all/src/types.rs:12-16`）。多数 agent 只透传（`loader.rs:795-799`；`total_tokens` 取自 `summary.total_tokens()`，`loader.rs:785`，定义含 `extra_total_tokens`，`core/src/types.rs:183-189`）。Codex 是唯一「从 input 扣缓存」的：

```rust
let input = codex::non_cached_input_tokens(
    usage.input_tokens, usage.cached_input_tokens, usage.cache_creation_tokens,
);
… cache_creation_tokens: usage.cache_creation_tokens,
  cache_read_tokens: usage.cached_input_tokens,
```

证据：`loader.rs:845-855`（模型级）、`loader.rs:868-875`（组级）；扣减语义 `adapters/codex/src/report.rs:128`：`input_tokens.saturating_sub(cached_input_tokens.saturating_add(cache_creation_tokens))`。即 **`cached_input_tokens → cache_read_tokens` 列，且 `cache_read + cache_creation` 一并从 `input_tokens` 扣除**。Reasoning token 不在列里，只进元数据：`loader.rs:878-881`（`"reasoningOutputTokens"`），`extra_total_tokens: 0`（`856`）。其它 agent 的 extra 由各自适配器填入（`hermes/src/parser.rs:171`、`droid/src/loader.rs:83`、`goose/src/parser.rs:92`、`kimi/src/parser.rs:362`、`copilot/src/loader.rs:513`、`codebuff/src/loader.rs:83`、`openclaw/src/parser.rs:405`），合并用饱和加（`types.rs:174`、`types.rs:202`）。

**9.3 agent 名与 project 字段**

- 行级 agent 名：`loader.rs:793`、`loader.rs:866`（`"codex"`）、`types.rs:118`（跨 agent 聚合行 `"all"`）、动态 pi store 名用 `Box::leak` 造 `&'static str`（`494-496`，调用 `361`）。展示名映射在 report 层（`"pi" => "pi-agent"` 等，`report.rs:597-620`），JSON 输出标识本身（`report.rs:162`）。
- **`[pi] gpt-5.4` 这类前缀既不在 loader 也不在 report，而在适配器 parser**：

```rust
let model = raw_model.as_ref().map(|model| format!("[{}] {model}", context.store_name()));
```

证据：`adapters/pi/src/parser.rs:354-356`（默认 store 名 `"pi"`，`adapters/pi/src/loader.rs:74-78`）；另一例 `adapters/openclaw/src/parser.rs:317` `format!("[openclaw] {model}")`。全仓 `format!("[` 仅这两处 + CLI 帮助文本（`ccusage-cli-parser/src/help_codegen.rs:235/253`）；report 层不剥离该前缀（模型名只经 `short_model_name` 处理 `claude-` 前缀，`core/src/project_names.rs:125`）。
- project：`AllRow` **没有 project 列**，只在元数据里带 `projectPath`，且**仅 pi 系**允许带（`include_project_path=true`，`loader.rs:589`、`628`；其它 agent 传 `false`）：`if include_project_path && let Some(project_path) = summary.project_path.as_ref() { metadata.insert("projectPath".to_string(), json!(project_path)); }`（`loader.rs:819-821`）。缺失不写占位串。
- 会话身份归组用 `(project_path, session_id)`（`loader.rs:735-741`）；Claude 会话报表走 `load_entries` 而非 daily：`loader.rs:633-643`。

**9.4 合并与二次聚合**

各 agent 行先拼接（`append_agent_rows`，`loader.rs:482-492`，同时收集 `detected`），再在 loader 层按报表粒度二次聚合：daily/weekly/monthly → `aggregate_rows`（`loader.rs:417`）；session 只排序不聚合（`399-414`）。`aggregate_rows`（`888-908`）按 period 分桶（weekly 周一起算 `897-898`，monthly 取前 7 字符 `893-896`），`AllAccumulator` 累加五列 + cost + 模型集合并按 agent 保留 `agent_breakdowns`（`types.rs:78-130`），聚合行 `agent: "all"`（`types.rs:116-129`）。零值行被丢弃：`total_tokens == 0 && total_cost == 0.0`（`loader.rs:786-789`，注释：只有 cost 的 Copilot credits 行仍要计费）。

**9.5 各开关在装载层的影响点**

| 开关 | 影响点 |
| --- | --- |
| `--since/--until` | 条目级 `filter_loaded_entries_by_date`（`adapters/common/src/lib.rs:36-47`；调用 `adapter-all/src/loader.rs:564/582/585/625/726`）；Claude 日汇总级按 `date` 字符串比较（`adapter-all/src/loader.rs:656-665`）；会话级按 `last_activity`（`adapter-all/src/loader.rs:756-768`）；Codex 无窗口时走 `load_groups` 快路径（`adapter-all/src/loader.rs:672-683`） |
| `--json` | 只影响进度抑制与输出分流：`loader.rs:108-115` 强制子装载 `json: true`；`wants_json = json ‖ jq.is_some()`（`core/src/output.rs:14-16`），分流在 `adapter-all/src/lib.rs:45/65` |
| `--no-cost` | 装载层零引用，只删输出（`output.rs:158-159` 入参、`:249/304/369/512` 调用点，剥离函数 `output.rs:564-581`） |
| `--offline` | 唯一入口 `loader.rs:83-89` 的 `PricingMap::load_with_overrides(shared.offline, …)` |
| `--single-thread` | 装载层不读，经 `SharedArgs` 透传到文件级并行（`adapters/common/src/lib.rs:89-96`） |
| `project_filter` | 统一装载层**不传**（`loader.rs:635`/`648` 传 `None`）；`SharedArgs` 无 project 字段（`ccusage-cli/src/types.rs:33-60`） |

### 10. 命令层

**10.1 `run_daily`：走日汇总快捷路径**

```rust
let mut rows = load_daily_summaries(&shared, args.project.as_deref(), args.instances || args.project.is_some())?;
filter_and_sort_summaries(&mut rows, &shared, |row| row.date.as_deref().unwrap_or_default());
```

证据：`commands/mod.rs:33-42`；JSON 分流 `44-57`；`--instances/--project` 会打开按项目分组（`38`）。

**10.2 `run_bucket` / `run_weekly`：entries → 按天聚合 → 再分桶**

```rust
let entries = load_entries_since(&shared, None)?;
let mut daily = summarize_by_key(&entries, |entry| entry.date.clone(), |key| (key.to_string(), None))?;
filter_and_sort_summaries(&mut daily, &shared, …);
let mut buckets = summarize_summaries_by_bucket(&daily, kind, WeekDay::Sunday);
```

证据：`commands/mod.rs:72-87`（weekly 在 `110-124`，差异是 `args.start_of_week`）；分桶实现 `core/src/summary.rs:178-206`，跨桶累加 `208-289`。

**10.3 `run_session`**

- 列表（`commands/mod.rs:146-181`）：`load_session_rows`（`183-206`）内部 `load_entries_since` + `filter_loaded_entries_by_date`，按 `(project_path, session_id)` 归入 `SessionAccumulator`；随后丢掉四列全 0 的会话并按 `total_cost` 排序：`rows.retain(|row| row.input_tokens + row.output_tokens + row.cache_creation_tokens + row.cache_read_tokens > 0)`（`155-161`）。
- 单会话（`208-260`）：按 `entry.data.session_id` 或路径会话名匹配，按时间戳排序（`211-217`）。**JSON 明细里的 cache creation 用扁平字段**（`243` `entry.data.message.usage.cache_creation_input_tokens`），与聚合口径 `cache_creation_token_count()` 可能不一致（有 `cache_creation` 对象时会写成 0）。

**10.4 `run_blocks` / `run_statusline`**

- `run_blocks`（`262-307`）用**无界** `load_entries`（`267`），因为 5 小时计费块要向前链接（`blocks.rs:61` 起按时间排序切块）；`--recent` 只保留最近 3 天或活动块（`272-277`，`DEFAULT_RECENT_DAYS = 3`，`core/src/lib.rs:60`）。
- `run_statusline`（`310-376`）：从 stdin 读 hook JSON（`318-325`）；成本来源四态 `cc/ccusage/auto/both`（`420-435`）；`statusline_shared` 默认 `offline = args.offline && !args.no_offline`（`378-384`，`StatuslineArgs::default` 里 `offline: true`，`ccusage-cli/src/types.rs:221-240`）。
- 活动块装载用渐进窗口避免全量读：依次 1/7/30 天（`557-559` 常量 `STATUSLINE_BLOCK_LOOKBACK_DAYS`），达不到锚点就放宽，最后回退全量 `load_entries`（`568-597`）；「今日」窗口由 `statusline_today_shared` 生成（`541-555`：`since = until = 本地今天的 YYYYMMDD`）。
- 结果缓存以「未过期 + transcript mtime 未变」为命中条件，正在更新时保留旧输出（`826-845`）。

**10.5 日期过滤、排序、分页**

- 日期比较是**字符串比较**：`date_within_range` 把 `YYYY-MM-DD` 去掉 `-` 后与 `YYYYMMDD` 边界做字典序比较，两端闭区间（`core/src/date_utils.rs:196-203`）；因此 `2026`、`202609` 这类部分边界**可工作**（注释 `190-195`）。窗到瞬间的换算 `date_range_bounds_ms` 返回 `[since 00:00, until 次日 00:00)`（`205-225`），只用于 mtime 剪枝与 statusline。
- 排序：`filter_and_sort_summaries`（`core/src/summary.rs:291-305`）先过滤再 `sort_summaries`（`307-315`，按日期键）；统一报表会话形态按 cost（`loader.rs:403-413`，显式 `--order asc` 才反转）。
- `--last N` 在解析阶段折算成 `--since`：`rust/crates/ccusage/src/cli/last_window.rs:11-24`（按报表自身周期：daily=今天、weekly/monthly 对应周/月），各 agent 报表同规则（`26-74`），周起点固定周一（`60-62`）。
- **没有分页/limit/offset 参数**：`ccusage-cli-parser/src/parser.rs` 中检索 `--limit`/`--page`/`--offset` 无命中（唯一带 limit 的是 blocks 的 token 预算列，`commands/mod.rs:283-306`）。报表是「一次装完 → 排序 → 输出」。

### 11. 给 TokenScope 的可复用清单（含坑）

1. **多根 + 显式根优先**：`CLAUDE_CONFIG_DIR` 逗号分隔，设了就必须有效否则硬失败——TokenScope 若要更友好，应显式声明「无效环境变量 → 告警 + 可选回退」的策略差异。
2. **路径排序是结果的一部分**：`usage_files` 的 `sort_by_cached_key(path_string)` 决定去重「谁先谁后」，跨实现对比需复刻或改用与顺序无关的合并规则。
3. **身份优先行内、路径兜底**：`subagents/agent-*.jsonl` 不双算靠的是「同会话 + 同 requestId/messageId」的键而非文件特判；`subagents/workflows/<wf>/agent-*.jsonl` 的路径兜底会话名是工作流目录名，只有行内 `sessionId` 能纠正。
4. **紧凑 JSON 是硬约束**：`"usage":{`/`"timestamp":"` 都要求无空格；TokenScope 用 serde 解析整行更健壮，但「同一样本两工具结果不同」是预期差异，需在文档里披露。
5. **`requestId` 缺失是常态**：必须准备三档键（§4.1），否则要么重复计费（网关复用 id），要么漏计（一条回复多行）。
6. **sidechain 是优先级输入，不是过滤条件**。
7. **advisor 迭代要拆条并自算价**：拆条后 `cost_usd = None`，display 模式会算成 0——三种计价口径都要显式处理，否则静默少算。
8. **cache 口径**：`cache_creation` 对象优先于扁平字段；`input_tokens` 是未缓存余量，四桶相加才是 total；Codex 必须 `input - cached - cache_write`。
9. **日汇总有独立实现**：若 TokenScope 只保留一条主路径，需要把四条差异点（progress 行、cost 决胜、extra token、sidechain 时间戳守卫）写成显式不变量并补测试——第 4 条会让两实现给出**不同总量**，不能只靠现有等价性测试背书。
10. **`--offline`/`--no-cost`/`--json` 都不改变用量口径**（分别是价格刷新、输出剥离、进度与格式）。

## 未验证/存疑

- **progress 包装行的双路径分叉**：`read_usage_file` 用严格 `UsageEntry`（`lib.rs:514`，`message` 必填 `core/src/types.rs:13/23`），daily 用无标签联合接受 `data.message.message`（`daily.rs:182-204`）。「progress 行有 subagents 副本」时两者收敛（`main.rs:570-595` 断言相等），但**没有**测试覆盖「progress 行无任何副本」的情形——按代码推断两条路径 token 总量会不同，未实测。
- **`Speed` 枚举严格性**：`speed` 只接受小写 `standard`/`fast`（`core/src/types.rs:36/60-65`），其它取值会让整行被静默丢弃；仓库内无该取值 fixture，属推断。
- **6 位微秒时间戳**：`parse_ts_timestamp` 只接受整秒（长度 20/25）或 3 位毫秒（24/29）（`core/src/date_utils.rs:113-117`），其它长度直接 `None` → 整行丢弃；无相应 fixture，未实测。
- **daily 与 entries 的 cost 决胜差异**（`daily.rs:597-599` vs `lib.rs:198-212`）在真实数据上是否会分叉：需要「token 总数相同、cost 不同、且无其它决胜维度」的样本，现有测试只覆盖「subagent 副本带 costUSD」一种（`main.rs:570-595`）。
- **`--single-thread` 对统一报表的 agent 级并行无效**：`load_agent_rows_parallel` 无该分支（`loader.rs:422-480`），属代码阅读结论，未运行验证。
- **Codex `group.total_tokens` 与四列之和是否恒等**：`loader.rs:876` 直接透传，未深挖 `rust/adapters/codex` 内部是否已含 reasoning（不影响 Claude 主线）。
- **行数口径**：任务书给出的若干行数（`cost.rs` 618、`pricing.rs` 5759、`adapter-all/loader.rs` 1532）与实测（664、6181、1637）不符；本文一律采用实测行号。
