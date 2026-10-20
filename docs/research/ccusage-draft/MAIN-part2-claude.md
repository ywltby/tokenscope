## 4. 读取链：Claude Code 适配器

Claude 是 ccusage 的默认与最主要数据面（无子命令时即走它）。

### 4.1 数据根发现

**已验证**（`rust/adapters/claude/src/paths.rs:23-63`）：

| 情形 | 行为 |
| --- | --- |
| `CLAUDE_CONFIG_DIR` 已设置 | 按 `,` 拆分、`trim`、丢空项，逐项 `expand_home_path`；仅保留「其下确有 `projects/` 目录」的路径（`paths.rs:33`）；去重；全部无效时**报错退出**并给出期望结构（`paths.rs:40-42`） |
| 未设置 | 依次取 `$XDG_CONFIG_HOME/claude`（缺省 `~/.config/claude`）与 `~/.claude`（`paths.rs:45-53`） |
| 传入的就是 `projects` 目录 | `normalize_claude_config_path` 自动上提一级（`paths.rs:57-63`） |

两个根**同时可用时都扫**（XDG 与传统 home 并存），这是「多根」设计的最小实现。
本机实测 `~/.claude/projects/` 下有 8 个一级项目目录、会话文件为 `<uuid>.jsonl`（旁证，未读内容）。

### 4.2 文件清单

`usage_files()`（`paths.rs:65-79`）：`<root>/projects` 递归收集 `.jsonl`
（`ccusage_adapter_common::collect_usage_files` → `collect_files_with_extension`，
`rust/adapters/common/src/lib.rs:14-34`：手写递归、只按扩展名过滤、对目录不可读**静默跳过**）。
`--project` 过滤走 `is_project_path_segment` 判定后再拼 `projects/<filter>`
（`paths.rs:69-76`），避免路径穿越（有测试 `rejects_dot_segments_as_project_path_segments`）。
最终 `files.sort_by_cached_key(path.to_string_lossy())`——**按路径字符串排序**，
以逼近 TypeScript 版行为（`main.rs:212-223` 的测试钉住「父文件在子目录 subagents 文件之前」）。

### 4.3 `--since` 的文件级剪枝（省 I/O 的关键）

`split_files_before_since()`（`paths.rs:117-172`）**不改解析结果，只缩小文件清单**：

1. `since` 缺省/不可解析/晚于当前时间 → 全部保留（`paths.rs:127-136`）。
2. 阈值 = `since_ms - MTIME_PRUNE_MARGIN_MS`，余量常量 `MTIME_PRUNE_MARGIN_MS = MILLIS_PER_DAY`（一天，`paths.rs:84`），用于吸收时区换算误差与「会话在所属条目之后很久才落盘」。
3. 用 **union-find**（`SessionGroups`，`paths.rs:174-180`）把「路径推出的 session id」与「文件内记录到的 session id」连成组——**整组保留或整组丢弃**（`paths.rs:139-170`）。
   原因写在注释里（`paths.rs:98-106`）：sidechain 回放的去重是按 session id 跨文件匹配的，
   只丢父文件会让它的回放被重复计入。
4. 组内只要有任一文件 `mtime >= 阈值`，整组保留；`mtime` 读不到的文件视为存活。

### 4.4 单文件解析管线

**已验证**（`claude/src/lib.rs:491-603`）——逐行漏斗，任何一环不满足即静默跳过：

```
fs::read(整个文件到内存)                     // lib.rs:505，不是流式
 └ byte_lines(content)                        // 逐行切片
    └ memmem 预过滤：行内必须含 `"usage":{`    // lib.rs:509-513（字节级，最快的一层）
       └ deserialize_usage_line::<UsageEntry>  // lib.rs:727-736
          ├ 行内无 "null" → 直接 serde_json::from_slice
          └ 含 "null" → 先解析成 Value，跑 null 契约校验（见下）
       └ parse_ts_timestamp(&data.timestamp)   // 失败即丢
       └ update_loaded_file_timestamp          // 记录文件最小时间戳（min）
       └ is_valid_usage_entry(&data)           // lib.rs:668-707
          ├ version 存在必须 `is_semver_prefix`（防 "not-semver" 污染）
          └ session_id / request_id / message.id / message.model
            这四者**只要存在就不允许为空串**
       └ format_date_tz(timestamp, tz)         // 按统计时区算 YYYY-MM-DD
       └ calculate_cost(...) / missing_pricing_model_for_usage(...)
       └ usage_limit_reset_time_from_line      // 仅 isApiErrorMessage 行带 "usage limit reached|epoch"
       └ 模型名归一：`<synthetic>` → None；speed=fast → 追加 `-fast`（lib.rs:535-543）
       └ advisor 拆分（见 4.5）
```

`deserialize_usage_line` 的 null 契约（`lib.rs:709-736`）值得单独记：
Claude Code 会写出**已知非空字段为 `null`** 的行，TypeScript 版把它们跳过，
Rust 版为此在反序列化前做结构检查（`is_unsupported_nullable_field`，`lib.rs:867-901`），
唯一豁免是 `usage.iterations[].model`（Claude Code 常写 `null`，`lib.rs:712`）；
同时用 `UniqueJsonValue`（`lib.rs:741-830`）保留「重复 JSON 成员即非法」的严格性——
经 `serde_json::Value` 中转默认会静默去重，反而把畸形行洗成合法行。

### 4.5 advisor 与 iterations 拆条

**已验证**（`lib.rs:635-658`、`read_usage_file` 的 559-600 行）：

- 先做便宜的字节预筛：行内必须出现 `"advisor_message"`（`lib.rs:636`），再做完整解析。
- `message.usage.iterations[]` 中 `type == "advisor_message"` 的项**各自成为一条独立事件**，
  模型取 iteration 自带 `model`，token 用该 iteration 的四个桶。
- 拆出的条目：`message.id` 变为 `"{原id}:advisor:{index}"`（`lib.rs:562-565`），
  `cost_usd` 置 `None`（advisor 行不写 cost），因此计价必须按 advisor 模型自算——
  有测试 `calculates_advisor_cost_with_the_advisor_model`（`lib.rs:1164`）与
  端到端测试 `loads_advisor_usage_as_a_separate_model_entry`（`main.rs:399-421`）。
- 日汇总实现里有完全对应的逻辑（`daily.rs:368-398`），保持两条链路一致。

### 4.6 去重（本次研究中最精细的一段）

去重发生在**跨文件合并阶段**（`load_entries_inner` 的 170-187 行），用 `FxHashMap<u64, DedupeIndexVec>`
索引已收录条目，主键由 `usage_dedupe_hash`（`lib.rs:350-366`）生成：

```text
key = hash(message.id, request_id)                        // 基本情况
      + 当 request_id 为 None 时追加 session_id
      + 再当 message.id 不是 "msg_" 开头时追加 timestamp
```

设计意图（源码注释 `lib.rs:343-349` 与 368-375）：

| 场景 | 结果 |
| --- | --- |
| 同一 response 的多行（同 message.id + 同 requestId） | 合并为 1 条 |
| 网关对每个响应复用同一个 message.id（无 requestId、非 `msg_` 前缀） | 因带 session+timestamp 而不误合 |
| 真 Anthropic `msg_` 前缀且无 requestId | 只按 session 折叠，**跨 timestamp**——Claude Code 会为同一 message 的每个 content block 各写一行 |
| 不同 session 复用了同 message.id | 不合并（session 参与键） |

**sidechain 回放**单独有一条匹配路线（`lib.rs:237-266`）：
`/btw` 侧链日志会用**新的 requestId** 回放父消息，精确键匹配不到，
于是再查 `sidechain_replay_dedupe_hash`（`message_id + session_id`，`lib.rs:377-383`）
与 `sidechain_entry_replay_dedupe_hash`（`lib.rs:386-392`，父侧候选只查侧链条目，减少重扫）。
匹配条件放宽为「requestless 时可跨时间戳」（`loaded_entry_matches_sidechain_dedupe_key`，`lib.rs:427-440`），
且要求**双方至少一侧是 sidechain**（`lib.rs:442-444` + `is_sidechain_usage_entry`）。
跨会话补登别名（`lib.rs:270-286`）是为了让「留存者换了会话」之后，后续回放仍能命中。

保留哪一条由 `should_replace_deduped_entry`（`lib.rs:198-212`）决定，优先级依次为：

1. **非 sidechain 优于 sidechain**（父消息胜过侧链回放）；
2. token 总数大者（`usage_token_total` = 四桶之和，`lib.rs:190-196`）；
3. 带 `speed` 字段者胜（信息更全，且会决定 `-fast` 后缀）。

评分基准是**原样的 JSONL 版本**（`Message.id`/`usage`/`speed`），不是归一后的 `LoadedEntry`；
替换时只增量更新变化了的索引键（`lib.rs:287-316`），避免同一时间戳反复重写导致重扫大桶——
注释明确说是为网关场景兜性能。这一整套行为由 9 个定向测试钉住
（`lib.rs:1196-1537`：跨会话同 message.id、同 session 无 requestId、同时间戳重复写、
复制 transcript、跨会话同 requestId、sidechain 回放、父优先、索引刷新……）。

### 4.7 并行度

`read_files_parallel`（`rust/adapters/common/src/lib.rs:84-126`）：

- 线程数 = `available_parallelism()`，上限为文件数；`--single-thread` 时退化为顺序（`common/lib.rs:89-99`）。
- 分块用 `chunk_file_indexes_by_size`（`common/lib.rs:49-81`）：按文件大小**降序**做贪心装箱
  （每次投给当前累计最小的块），从而让大文件分散开；
- `thread::scope` 起后**按索引回填结果**，保证输出顺序与输入一致（有测试 `preserves_file_order_and_matches_single_thread`，`common/lib.rs:133-153`）。

### 4.8 日汇总的独立实现

`load_daily_summaries`（`lib.rs:78-105`）走 `daily.rs` 的**另一套解析代码**
（`read_daily_usage_file`，`daily.rs:285-401`），与主链路差异：

- 反序列化类型是 `DailyUsageLine`（`daily.rs:171-235`），额外容纳 AgentProgress 包裹
  （`type: "progress"` 内嵌 `data.message`，`daily.rs:207-229`）——即**嵌套在 progress 行里的模型调用也被计入**；
- 保留 `message_id`/`request_id`（`DailyLoadedEntry`，`daily.rs:155-170`）以便跨文件去重；
- 但它**只做日粒度累加**（`DailyAccumulator`，`daily.rs:608`），不产出逐条 `LoadedEntry`。

两条链路的等价性被写成了测试契约：`daily_summaries_match_entry_aggregation_for_requestless_rewrites`
（`main.rs:449-480`）、`loads_daily_summaries_like_loaded_entry_aggregation`（`main.rs:482-535`）、
`keeps_nested_agent_progress_in_daily_model_order`（`main.rs:537-567`）等——
**这是「同语义两份实现」的维护成本，也是 ccusage 自认必须用测试兜住的地方**（对照 §12）。
