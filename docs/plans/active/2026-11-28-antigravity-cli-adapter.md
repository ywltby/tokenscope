# 新增 Antigravity CLI（`antigravity-cli`）用量来源实施计划

> 状态：**已按三轮审查意见修订（2026-11-28），待最终确认后开工（方案文档，尚不分配发行版本）**。
>
> 用户 2026-11-28 已拍板口径与确认项：
> 1. **数据源与历史模型补齐策略（方案 A）**：主流程只读 `brain/<id>/.system_generated/logs/transcript.jsonl`，并严格只读同根目录 `conversation_summaries.db` 补工作区路径 `workspace_uris`；不打开、不解析内部 `conversations/*.db` Protobuf；
> 2. **`CHECKPOINT` 压缩摘要调用**：与 `PLANNER_RESPONSE` 一并计入用量；
> 3. **名称、图标与标识**：展示名 **Antigravity CLI**，图标复用仓库已打包的 `frontend/src/assets/icons/googlegemini.svg`（可访问名称与悬停提示保留 `Antigravity CLI`），内部枚举 `AgentKind::AntigravityCli`（序列化与库内标识 `antigravity-cli`），默认根固定 `~/.gemini/antigravity-cli`；暂不接入桌面版 `~/.gemini/antigravity`、`antigravity-ide`、`antigravity-acp`；
> 4. **首要目标与未知口径**：保证获取到的数据是真实的；缺少工作区路径（pwd）或模型名时如实标为未知，**未知的处理参照项目导入 CCS 的未知处理**（不丢弃有效用量、不伪造项目路径或模型名、不在计价层被外置价格误命中、不静默按 `$0` 计价）；
> 5. **升级矩阵上限**：本轮维持 `MAX_ORIGINS = 6`（`0.10.0` 选点实测 6 个，满足上限；下一版登记新持久格式后另行评审）。
>
> 本计划按用户明确排期解除 `CLAUDE.md`「候选 agent 暂缓排期」中对 Antigravity CLI 的暂缓。

**Goal：** TokenScope 只读采集本机 Antigravity CLI（`~/.gemini/antigravity-cli`）的本地会话与子代理转写用量，作为第五个独立应用纳入统一历史库、汇总、筛选、下钻、计价与来源管理；现有 Claude Code / Codex / WorkBuddy / WorkBuddy AI 的统计结果零变化。

**Architecture：**
- 新增独立适配器 `crates/tokenscope-core/src/source/antigravity_cli.rs`：只发现 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若用户手动指向 `brain/` 目录，也允许直接匹配其下 `<conv_id>/.system_generated/logs/transcript.jsonl`），天然排除 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`conversations/*.db` 与其它非目标文件；对可原地改写的 `transcript.jsonl` 在文件指纹中纳入内容摘要，确保等长同 `mtime` 改写在 `refresh = false` 正常路径下仍可靠重解析。
- 工作区路径（`cwd`）通过同根 `<root>/conversation_summaries.db` 在**读取期间持有操作系统写入/删除保护**获取一致视图：Windows 上以 `OpenOptions::new().read(true).share_mode(FILE_SHARE_READ)`（拒绝共享写与删除）短暂持柄读取主库、检查 `-journal` 并读取可选 `-wal`（已在本机实测：活跃运行的 `agy.exe` 不长期占用写句柄，`FILE_SHARE_READ` 可正常打开；若写者正持柄或存在非空 `-journal`，立即失败返回 `Unusable`，绝不读入提交前写回主库的脏页，也绝不在源目录创建 `-shm`），释柄后在纯内存执行官方 WAL 三分类校验重放并由 `rusqlite 0.40.2` 内置 `serialize` 的 `deserialize_read_exact(MAIN_DB, image.as_slice(), image.len(), true)` 载入 `:memory:`，仅查询 `(conversation_id, workspace_uris)` 两列。单轮采集的 `collection_key`、解析器与 `remember_collection` 共用同一份冻结映射实例。
- 历史库 `history.db`（v5 → v6，纯增量新表 `event_file_observations`）为 `AntigravityCli` 按强身份记录每个源文件的最新观察，再在所有当前文件最新观察上应用统一跨文件代表规则与「当前观察优先、全部缺失才回退库内已存值」的项目防降级规则，确保任意多轮跨根/同文件改写均与冷库全扫严格收敛一致。
- 隐私政策修订为共同同意修订 4（如实披露 `USER_INPUT.content` 尾部 `<USER_SETTINGS_CHANGE>` 块与 `conversation_summaries.db` 原始页短暂内存载入边界），设置格式修订为 v4（新增 `sources.antigravity_cli`，`4->4` 恒等迁移）；Rust / PowerShell / Node 三处验收入口在首笔写盘前统一完成根、已有树（含联接与硬链接 `nlink > 1`）与拟写目标全路径预检。

**Tech Stack：** Rust（`tokenscope` 核心库、`tokenscope-app` 桌面壳，`rusqlite 0.40.2` 启用内置 `serialize` 特性，零新外部 crate）、Vue 3 + Naive UI、SQLite 历史库、PowerShell / Python / Node 原生验收脚本。

---

## 1. 背景与本机只读核验事实（2026-11-28）

研究参考：[cc-switch v4.0.18 的 agy 接入拆解](../../research/2026-10-22-cc-switch-agy-usage.md)、[Magpie 用量统计拆解](../../research/2026-10-28-magpie-usage-statistics.md) §15.8。2026-11-28 对本机 `~/.gemini/antigravity-cli` 在活跃对话期间分批执行严格只读采样（15:04–17:03，采样期间当前活跃会话继续追加了 5 个新步骤，故不同脚本的分母略有递增，下表逐项标明采样批与分母；只统计结构、字段名、类型与数值，不留存会话正文与敏感内容）：

| 维度 | 采样批 / 分母 | 实测结论（历史样本观测，不作验收固定预期） |
| --- | --- | --- |
| 会话结构与子代理占比 | 第 3 批（15:08，全量 66 会话 / 15,701 条用量步骤） | 66 会话 = 24 主会话（16 非空 + 8 空，`steps = 0`）+ 42 子代理（`self` 40、`research` 2，`nesting_depth = 1`）。子代理落在独立 `brain/<sub_id>`，用量不重复计入父会话：主会话 12,825 条、子代理 2,876 条（约 18.3% 调用、约 28.4% 输出） |
| `transcript.jsonl` vs `transcript_full.jsonl` | 第 1 批（15:04，58 非空会话 / 32,527 行 / 15,696 条用量步骤） | 二者在 `step_index`、`type`、`source`、`status`、`created_at`、`model` 与全部 token 字段上 **0 差异**；精简版仅截断超长正文/工具输出，体积小约 26%（44.3 MB vs 60.0 MB）。`chunks/transcript/*.jsonl` 与 `chunks/transcript_full/*.jsonl` 是同字节分片副本，不得扫入 |
| 与内部 `.db` 交叉核验 | 第 2 批（15:07，transcript 15,700 条 vs `immutable=1` 主库 15,695 条） | 重叠的 **15,695 / 15,695 条**在 `input_tokens`、`cache_read_tokens`、`output_tokens` 上 **100% 精确相等（0 差异）**；差值 5 条全部来自活跃会话尚未 checkpoint 的 `.db-wal`（13.6 MB），而 `transcript.jsonl` 已实时落盘 |
| Token 桶语义 | 第 1/2 批（15,695–15,696 条） | 仅出现 `input_tokens`、`cache_read_tokens`、`output_tokens`（全库 `cache_write_tokens` 0 次）。连续轮次在 Claude 与 Gemini 上均满足 `上轮(input + cache_read) ≈ 下轮 cache_read`（且常见 `input_tokens ≪ cache_read_tokens`），证明 `input_tokens` 为 **FRESH 语义（已排除缓存读）**；`output_tokens` 满足 `thinking + non_thinking == output`（0 违例），**已包含思考 token** |
| 携带 token 的步骤类型 | 第 1 批（15:04，15,696 = 15,678 + 18） | ① `PLANNER_RESPONSE`（`source=MODEL`, `status=DONE`），15,678 条，100% 含 token（含 1 条 `output_tokens = 0` 但输入/缓存非 0 的合法记录）；② `CHECKPOINT`（`source=SYSTEM`, `status=DONE`），2026-11-09 起 18 条全部携带 token（上下文压缩摘要真实模型调用，单次约 250K cache_read、3K–4K output；早期 38 条无 token 字段）。无 `status != DONE` 携带 token 的行 |
| 模型字段与 `USER_SETTINGS_CHANGE` 结构 | 第 2 批（15:07）+ 第 4 批专项核验（16:13，267 条 `USER_INPUT`） | ① `2026-11-18` 起 `PLANNER_RESPONSE`（含子代理）直接带 `model`（2,092 条）；② `2026-10-13 ~ 2026-11-10` 主会话无行内 `model`（10,717 条），但全部 267/267 条 `USER_INPUT` 均以 `</USER_REQUEST>` 封闭用户提问，全部 17/17 次切换通知均落在 `</USER_REQUEST>` 之后的 `<USER_SETTINGS_CHANGE>\nThe user changed setting \`Model Selection\` from <old> to <new>. ...\n</USER_SETTINGS_CHANGE>` 系统注入块内（即使 11 条超长用户输入在 `transcript.jsonl` 被截断，尾部系统块仍 11/11 完整保留）；③ 同期 40 个子代理无 `model` 与切换块（2,872 条）；④ 18 条 `CHECKPOINT` 均无行内 `model`，18/18 与同会话前一条 `PLANNER_RESPONSE` 同模型 |
| 工作区路径与句柄占用 | 第 3 批（15:08，58 非空会话）+ 第 5 批实测（17:03） | `transcript.jsonl` 不含 cwd；`conversation_summaries.db` 的 `workspace_uris`（JSON 数组 `["file:///..."]`）覆盖 58/58 非空会话（含全部 42 个子代理），与 `conversations/*.db` 0 差异；在 `agy.exe` 活跃运行期间实测对 `conversation_summaries.db` 与 `-wal` 以 `FILE_SHARE_READ` 打开均成功（无常驻写句柄） |
| 行序与重复 `step_index` | 第 1/3 批 | 单文件内偶发重复 `step_index`（2 次，均为 `GENERIC|RUNNING`）；`created_at` 为 UTC RFC3339 秒级，15,700/15,700 精确等于步骤开始时刻 |

### 1.1 当前代码基线复核（`main` @ `3b9b444`，应用版本 `0.9.10`）

| 项目 | 当前现状与实施影响 |
| --- | --- |
| 应用枚举泛化 | WB01 已完成：`AgentKind::ALL`、`AgentSources::config(AgentKind)`、`acceptance::source_dir_override(AgentKind)` 均已按枚举分发，无需前置泛化阶段 |
| 历史库 v5 → v6 与多文件观察 | `HISTORY_SCHEMA_VERSION = 5`（`tests/history_migration.rs:537` 等处断言 `== 5` 需同步为常量或 6）；`event_origins` 会登记败者来源（`history.rs:2862`），故需在 v6 增设 `event_file_observations` 表（纯增量迁移，与本版 `SETTINGS_FORMAT_REVISION = 4` 共用同一新持久格式组，不额外增加升级基线数）并让 `dedupe.rs` 透传跨文件副观察，同时在 AG01 补真实 v5 升级回归 |
| 文件指纹与等长改写 | `report.rs:1488` 当前只按 `size:mtime_ms` 判文件缓存命中；对可原地等长改写的 `AntigravityCli` `transcript.jsonl`，必须把内容摘要纳入文件指纹，才能让 `80 → 60` 同长度同 `mtime` 改写在 `refresh = false` 正常入口下可靠更新 |
| 计价层与展示名约定 | `pricing.rs:1510` 外置模型条目必填键为 `prefix`；`collect_candidates`（`pricing.rs:2887`）按末段等价键匹配 → 须在汇总与明细共用的计价入口拦截 `AntigravityCli` 的 `UNKNOWN_MODEL`。按 `docs/stats-semantics.md:699`，名称目录无匹配时最终模型分组 `Group.label` 取原始模型 ID 代表（即 `Some("unknown")`），而非 `None` |
| `rusqlite 0.40.2` 内存反序列化 | 已核对本地缓存 `rusqlite-0.40.2/src/serialize.rs`：`serialize = []` 为零额外依赖的内置特性，提供 `Connection::deserialize_read_exact(MAIN_DB, image.as_slice(), image.len(), true)` |
| 采集键与冻结映射 | `report.rs:707`/`978`/`1065` 的单飞采集须让 `collection_key`、解析器与 `remember_collection` 消费同一份冻结工作区映射，避免键与内容失配 |
| 验收保护三处入口 | `acceptance.rs:316`、`scripts/prepare-native-acceptance.ps1:93/:107`（旧根复用需同步拦截内部联接与硬链接 `nlink > 1`，对齐 `native-single-instance.mjs:681`）、`frontend/scripts/lib/native-single-instance.mjs:116` 三处均须补齐 `.gemini/antigravity-cli` 与写前全路径预检 |
| 持久格式与协议门禁 | `SETTINGS_FORMAT_REVISION = 3 → 4`、`HISTORY_SCHEMA_VERSION = 5 → 6`、`PRIVACY_POLICY_REVISION = 3 → 4`、`PARSER_REVISION = 3`、`PARSE_RULES_REVISION = 7`、`INDEX_VERSION = 11`；`upgrade_baselines.py select --current-version 0.10.0` 当前选中 6 起点，不超 `MAX_ORIGINS = 6` |

---

## 2. 范围与不做

**做：**
- AG01：应用枚举、历史库 v6（`event_file_observations` 与真实 v5 升级回归）、去重与跨文件观察透传、跨文件代表重算、先当前观察后库内兜底的项目防降级、Antigravity 未知模型共用计价拦截、壳层映射与历史核验工具白名单。
- AG02：`source/antigravity_cli.rs` 适配器（精准文件发现、基于 Windows `FILE_SHARE_READ` 写保护的零写入 `conversation_summaries.db` 快照与官方 WAL 帧三分类、按规范化 DB 路径 + 内容摘要隔离的进程缓存与映射修订、transcript 解析与内容指纹、`</USER_REQUEST>` 后 `<USER_SETTINGS_CHANGE>` 窄匹配与未知兜底、受检算术）与合成 fixture 契约测试。
- AG03：来源发现、单轮冻结映射贯穿 `collection_key` / 解析器 / `remember_collection`、设置格式 v4（`sources.antigravity_cli` 与完整 `1/2/3/4->4` 迁移规则，`4->4` 恒等迁移）、Rust/PowerShell（含旧根内部联接 + 硬链接 `nlink > 1` + 拟写目标预检）/Node 三处验收保护入口同步与全仓测试密闭。
- AG04：隐私政策修订 4（`docs/privacy.md` 诚实披露 `USER_INPUT.content` 尾部 `<USER_SETTINGS_CHANGE>` 匹配边界、`conversation_summaries.db` 与 `-wal` 原始页短暂内存载入及两列 SQL 限定、排除清单，`PRIVACY_POLICY_REVISION = 4`）及升级/同意契约同步。
- AG05：前端五应用适配（复用 `googlegemini.svg`、类型、筛选、来源控件、设置页第五来源分区与说明、中英文文案）与 `DESIGN.md` 同步。
- AG06：原生验收合成数据与 `ui-identity` / `ui-source-paths` / 隐私重新同意场景覆盖。
- AG07：本机真实数据只读对账（双边共读同一冻结输入树，或各自按「规范化相对路径 + 存在状态 + 长度 + 每文件 SHA-256」编码生成输入清单摘要比对，写入隔离目录）。
- AG08：`docs/stats-semantics.md`、`CLAUDE.md`、`docs/plans/README.md`、`scripts/build-inputs.json`、版本递增与交付验收。

**不做：**
- 不接入桌面版 GUI（`~/.gemini/antigravity`）、`~/.gemini/antigravity-ide`、`~/.gemini/antigravity-acp`；后续有真实样本与明确排期再另立项。
- 不打开 `conversations/*.db`（不解析私有 Protobuf，不承担内部字段号漂移风险）。
- 不读取 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`history.jsonl`、`settings.json`、`annotations/`、`implicit/`、`log/`、`cache/`、`brain/<id>/scratch/`、`steps/*/output.txt`、`~/.gemini/config`。
- `conversation_summaries.db` 的 SQL 查询严格限定仅读 `conversation_id` 与 `workspace_uris` 两列，不查询 `title`、`preview`、`conversations_fts` 或 `raw_summary`。
- 不在 CCS 导入链路中新增 `agy` 映射（TokenScope 的 CCS 导入仍仅补充 Claude Code 历史）。
- 不增加环境变量派生根（使用 `--gemini_dir` 的私有目录通过设置页「手动追加目录」纳入）。
- 不增加主/子代理层级展示产品（子代理有效用量按既有口径一并纳入）。
- 不改动 Claude Code、Codex、WorkBuddy、WorkBuddy AI 的既有解析、身份、去重与计价口径。

---

## 3. 必须保持的不变量

1. **现有四应用统计零变化**：同一份 Claude Code / Codex / WorkBuddy / WorkBuddy AI 输入，改动前后的事件集合、四桶、去重、项目归属与费用逐项一致。
2. **数据真实优先与「未知」处理（参照 CCS 口径，且杜绝外置价格误命中）**：
   - **项目未知（缺 pwd / workspace）**：当 `conversation_summaries.db` 缺失、无该会话记录、`workspace_uris` 为空/非合法 JSON/全部 URI 无法经 `normalize_project_path` 规范化、或读取期间无法取得写入保护/一致视图时，`UsageEvent.project = UNKNOWN_PROJECT`（`"(未知)"`），`session_initial_cwd = None`，`event_cwd = None`；写入 `history.db` 时 `project_key = NULL`（与 `import/ccs.rs` 逐列一致）。聚合按项目分组与下钻均归入 `"(未知)"` 组，绝不以会话 ID 或目录名伪造项目。
   - **模型未知（缺 model）**：行内无 `model`、同会话也无前置有效模型或 `<USER_SETTINGS_CHANGE>` 切换通知时，**绝不丢弃有效用量事件**，`model` 记为常量 `UNKNOWN_MODEL = "unknown"`（与 CCS 未知模型写法一致），`skipped_no_model` 不吞有效用量。
   - **未知模型强制不计价（汇总与明细共源）**：在 `aggregate.rs` 与 `report.rs` 共用的计价入口上，对 `AgentKind::AntigravityCli` 且 `model.trim().eq_ignore_ascii_case(UNKNOWN_MODEL)` 的事件**无条件返回 `None`（未定价）**——即使外置 `pricing.toml`（合法 `[[model]]` 的 `prefix = "unknown"` 或 `prefix = "provider/unknown"`）或线上快照配置了单价，也绝不命中：
     - 纯未知模型分组满足 `total_cost_usd == None`、`unknown_pricing == true`、`unknown_tokens` 等于事件全量，且按 `docs/stats-semantics.md:699` 名称目录查询未解析、最终模型分组展示名 `label.as_deref() == Some("unknown")`；
     - 请求明细行满足 `model == "unknown"`、`cost_usd == None`、`cost_breakdown == None`；绝不编造名称或从 `settings.json` 反推。
3. **桶互斥（FRESH 语义）与受检算术（SF08）**：
   - `input_tokens = raw.input_tokens`（**不减** `cache_read_tokens`）；
   - `cache_read_tokens = raw.cache_read_tokens`（缺失按 0）；
   - `cache_write_tokens = raw.cache_write_tokens`（缺失按 0，`cache_write_split_known = false`，`cache_write_5m = 0`，`cache_write_1h = 0`）；
   - `output_tokens = raw.output_tokens`（已包含思考 token，不另行叠加）。
   - 任何已出现的 token 字段为负数、浮点、布尔、字符串或超出 `u64`，或 `UsageEvent::validate_buckets()` 检查总和/prompt 溢出失败 → 计 `bad_lines` 跳过，不饱和、不回绕；四桶全为 0 → 计 `skipped_zero_usage` 跳过（允许 `output_tokens == 0` 但输入/缓存读非 0 的合法完成行）。
4. **事件准入、强身份逐文件最新观察、内容指纹与跨文件代表/项目收敛**：
   - 仅接纳两类已完成步骤：① `type == "PLANNER_RESPONSE" && source == "MODEL" && status == "DONE"`；② 至少携带一个 token 字段的 `type == "CHECKPOINT" && source == "SYSTEM" && status == "DONE"`（早期无 token 字段的 `CHECKPOINT` 静默跳过，不计坏行）。未完成步骤（`status != "DONE"`）不产出事件、不占去重身份。
   - 文件内按 `step_index` 收敛（后写覆盖前写）；缺失/非非负整数 `step_index` 的合资格用量行计 `bad_lines` 跳过。
   - 强身份方案 **`antigravity-cli-step`** = `"{session_id}|{step_index}"`（`record_id = step_index.to_string()`），在 `dedupe.rs` 与 `history.db` 中受 `app = "antigravity-cli"` 隔离，不与任何其他应用合并。
   - **Antigravity `transcript.jsonl` 内容指纹（识别等长同 `mtime` 改写）**：在 `report.rs` 的逐文件缓存命中判断中，对 `AgentKind::AntigravityCli` 的文件指纹采用 `size:mtime_ms:sha256`（仅针对 Antigravity CLI，不影响现有四应用快路径），确保即便 `80 → 60` 等长改写且 `mtime_ms` 完全相同，正常 `refresh = false` 入口仍可靠识别内容变化并重解析。
   - **按强身份维护每文件最新观察 + 跨文件代表重算（增量与冷库严格等价）**：
     - `history.db` v6 新增表 `event_file_observations(event_id, app, source_file, event_key, ts_seconds, ts_nanos, model_raw, model_identity, project_key, session_initial_cwd, event_cwd, input_tokens, output_tokens, cache_write_tokens, cache_read_tokens, cache_write_5m, cache_write_1h, parser_revision, PRIMARY KEY (event_id, source_file))`（仅用于 `AgentKind::AntigravityCli`，不改既有四应用表/写入语义）；
     - 每次采集（含 `dedupe.rs` 同轮落败的跨文件副观察，透传至 `EventWrite`）按 `(event_id, norm_source_file)` **UPSERT** 该文件的最新观察（同一源文件改写时自动替换其旧观察，另一文件的观察不受影响）；
     - 随后在 `event_file_observations WHERE event_id = ?` 全部当前文件最新观察上，应用与 `dedupe.rs` 相同的确定性跨文件代表规则（事件时间最晚 → 四桶 `total_u128()` 更大 → `event_key` 更大）选出胜出观察 `winner`，重新物化 `usage_events`（含同步 `usage_events.event_key`）：
       - 例：`P=100` → `Q=70`（`{P:100, Q:70}` → `100`）→ `P` 改写为 `80`（`{P:80, Q:70}` → **`80`**）→ `P` 等长同 `mtime` 改写为 `60`（`{P:60, Q:70}` → **`70`**），每步均与冷库全扫完全一致；
       - 源目录停用或文件删除不删除历史观察与 `usage_events`，已保存用量永久保留。
   - **项目归属确定性优先级与防降级（先当前观察，后库内兜底）**：无论 `event_key` 是否因文件搬迁、行号重排或步骤改写而改变，物化 `usage_events` 时按严格三层优先级决定 `(project_key, session_initial_cwd, event_cwd)`：
     1. 若胜出观察 `winner` 有 `project_key.is_some()`，直接采用 `winner` 的三列项目字段；
     2. 否则，在 `event_file_observations WHERE event_id = ?` 的**全部当前文件观察**中筛选 `project_key.is_some()` 者，按同一确定性跨文件代表序（最新 `ts` → 更大 `total_u128()` → 更大 `event_key`）取最佳观察，并**从这同一条观察取齐 `project_key`、`session_initial_cwd`、`event_cwd` 三列**（保证 `Q` 把项目从 `A` 更正为 `B` 时，增量库立即更新为 `B`，与冷库扫描完全一致）；
     3. **仅当全部当前文件观察的 `project_key` 均为 `None`**（例如摘要库缺失、损坏或写者持柄暂返 `Unusable`）时，才沿用 `existing` 上次确认的三列项目字段，防止已确认项目被降级回未知。
5. **读取期间持有写入保护的零写入一致视图与最小读取面**：
   - 仅收集 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若 `<root>` 本身是 `brain/` 形态则收集 `<root>/<conv_id>/.system_generated/logs/transcript.jsonl`），通过 `DirVisitLedger` 防环与多根认领，明确跳过 `transcript_full.jsonl` 与 `chunks/`；软链/联接越界或失效跳过并诊断。
   - **`conversation_summaries.db` 受写入保护的零写入快照（彻底杜绝未提交脏页与混页）**：
     - 绝不在源目录上以 SQLite 直接打开库文件（防建 `-shm`），也绝不以无写保护的共享读读取可能存在脏页写回或并发提交的库；
     - **受保护读取协议（Windows `FILE_SHARE_READ` 内核拒绝共享写与删除）**：
       1. 以 `OpenOptions::new().read(true).share_mode(FILE_SHARE_READ)`（仅允许其它只读共享，**拒绝 `FILE_SHARE_WRITE` 与 `FILE_SHARE_DELETE`**）打开主库 `conversation_summaries.db`：
          - `NotFound` → 返回 `MappingState::Missing`；
          - 共享冲突（`ERROR_SHARING_VIOLATION`：写者正在持柄，含回滚日志提交前脏页写回或活跃 WAL 写连接）或其它打开错误 → 立即返回 `MappingState::Unusable`；
       2. **在持有主库 `FILE_SHARE_READ` 句柄期间**：
          - 检查 `conversation_summaries.db-journal`：若存在则同样以 `share_mode(FILE_SHARE_READ)` 打开，若拒绝共享或字节长度 > 0（活跃回滚事务或崩溃遗留热日志）→ 立即返回 `MappingState::Unusable`；
          - 尝试以 `share_mode(FILE_SHARE_READ)` 打开 `conversation_summaries.db-wal`：若 `NotFound` 则记无 WAL（并复查 `-journal` 仍不存在/为空）；若共享冲突或其它错误 → 立即返回 `MappingState::Unusable`；
       3. **仍在持有全部只读保护句柄期间**（此时操作系统内核保证零进程持有写句柄、且任何新写者/删除操作均被阻断，持续时间仅约 1–2 ms），用带上限的只读流（`Read::take`）从已持句柄读出 `db_bytes` 与可选 `wal_bytes`（**实际读入总字节 ≤ 64 MiB**），读完立即释放句柄；
       4. （跨平台兜底：非 Windows 平台无强制 `FILE_SHARE_READ` 时，未取得强制写入保护前不放行无保护共享读，默认保守返回 `Unusable`）；
     - **WAL 帧严格三分类与双重 64 MiB 上限**（释柄后在纯内存中执行）：
       - 无 `-wal` 或 `-wal` ≤ 32 字节（空 WAL 头/零帧）：校验主库页大小合法且长度为页大小正整数倍；
       - 存在有效 `-wal`（> 32 字节）：校验 32 字节 WAL 头（magic `0x377f0682`/`0x377f0683`、版本 `3007000`、头校验和、`wal.page_size == db.page_size`），顺序扫描每帧（`24 + page_size` 字节）并严格区分三种情况：
         - **旧代残留**（完整帧 `(salt1, salt2) != (header.salt1, header.salt2)`）：合法 checkpoint 后旧代分界，立即停止扫描，仅重放此前最后一个有效提交帧（无前置提交帧则直接保留主库映像）；
         - **未完整写入 / 未提交尾帧**（同代完整帧累计校验和正确但 `db_size_after_commit == 0` 且后续无提交帧；或位于最后一个有效提交帧之后的 EOF 不足一帧残尾）：忽略未提交尾部，仅重放至此前最后一个有效提交帧；
         - **损坏 / 校验失败**（WAL 头非法、`page_size` 不符、同代帧 `pgno == 0`、或**同代完整帧累计校验和失败**）：立即返回 `MappingState::Unusable`（绝不静默回退旧提交）；
       - **双重 64 MiB 上限**：校验「实际读入 `db_bytes.len() + wal_bytes.len() ≤ 64 MiB`」**且**「按 `db_size_after_commit.checked_mul(page_size)` 扩展后的最终内存映像 ≤ 64 MiB」；超限返回 `Unusable`；
     - **内存只读载入与两列限定查询**：在内存映像副本上将头字节 18–19（WAL 格式号 2）改写为 1，调用 `rusqlite 0.40.2` 的 `conn.deserialize_read_exact(rusqlite::MAIN_DB, image.as_slice(), image.len(), true)` 载入 `:memory:` 连接，仅执行 `SELECT conversation_id, workspace_uris FROM conversation_summaries`，查询后立即释放内存映像。**全程源目录零写入、零 `-shm` 创建、零磁盘临时文件**；
     - 任何一步无法取得写入保护或一致视图，一律返回 `MappingState::Unusable`（不回退无保护读或磁盘打开、不缓存失败态、输出可读诊断；已入库事件按不变量 4 保留既有项目归属，新事件记未知项目）。
6. **工作区缓存根隔离、内容摘要刷新、单轮冻结映射与失效粒度**：
   - 进程内工作区映射缓存以**规范化 DB 路径**（Windows 统一小写）为主键，以受保护持柄读出的 `(db_bytes, wal_bytes)` **SHA-256 内容摘要**判命中（彻底不依赖 Windows `mtime`，且避免同字节快照重复反序列化 SQLite）；不同手动根即使摘要库大小与修改时间相同也完全隔离、互不串用。
   - **单轮冻结一致**：每次采集入口在进入 `collect_flighted_with_on` 前（单飞键构建前）为所有已启用 Antigravity CLI 根一次性冻结 `BTreeMap<PathBuf, Arc<WorkspaceMapping>>`，该冻结映射同时供 `collection_key`（计划模式与单根模式双分支，停用固定 `"disabled"` 且不读库）、`AntigravityCliSource` 解析器与 `remember_collection` 消费，杜绝「以修订 A 的采集键发布按修订 B 解析的结果」。
   - **失效粒度**：`revision` 仅对 `(state, 排序后的 conversation_id -> normalized_project 映射表)` 计算确定性摘要（不含 `title`、`preview`、`last_modified_time` 或 WAL 字节摘要）：
     - 同会话继续聊天更新摘要库但 `(conversation_id -> workspace)` 映射集合未变时，`revision` 保持不变，不触发整根 transcript 重解析；
     - 仅当新会话出现有效工作区、已有会话工作区变化或 DB 状态迁移（`missing ↔ loaded ↔ unusable`）时，`revision` 才变化，按现有 `Source::context_revision` 机制使该根下文件缓存失效并整根重解析一次（与 Claude 项目映射同粒度）。
7. **`USER_INPUT.content` 窄匹配与隐私披露边界**：
   - 仅在 `type == "USER_INPUT" && source == "USER_EXPLICIT"` 中，先定位最后一个 `</USER_REQUEST>` 关闭标签，**只检查该标签之后**的系统注入块 `<USER_SETTINGS_CHANGE>...</USER_SETTINGS_CHANGE>`（`<USER_REQUEST>` 内的用户提问正文完全跳过），在块内严格匹配 `The user changed setting \`Model Selection\` from <old> to <new>.`（句号后接空白/换行/行尾/`<` 或反引号截止，保留版本号内小数点）；`<new>` 为空或 `None` 忽略；显示名归一与 `agy` 原生 slug 一致（如 `Gemini 3.8 Flash (High)` → `gemini-3.8-flash-high`，`Claude Opus 4.6 (Thinking)` → `claude-opus-4.6-thinking`；已是无空格无括号的 ID 原样保留）。绝不扫描 `PLANNER_RESPONSE` / `GENERIC` 正文，也不把普通用户消息（即使原文引用切换模板甚至嵌套 `<USER_SETTINGS_CHANGE>` 标签）当作模型切换。
   - 仅在内存中提取并保留归一化后的模型名，**`USER_INPUT.content` 原文绝不落库、不进诊断日志、不展示、不外传**，并在 `docs/privacy.md` 修订 4 中明确书面披露。
   - 模型解析优先级：行内显式 `model`（同时推进同会话 `session_model`）> 同会话前序 `session_model`（含 `<USER_SETTINGS_CHANGE>` 与前一条 `PLANNER_RESPONSE` 模型，覆盖 `CHECKPOINT`）> `"unknown"`。
8. **项目身份与时间口径**：`workspace_uris` 首个经 `project_path::normalize_project_path` 成功的 URI 作为会话项目身份，跨五应用同一目录归同一 key；`created_at` 按 UTC RFC3339 原样存储（步骤开始时刻），缺失或非法计 `bad_lines` 跳过，展示沿用既有单次时区转换。
9. **同意先于读取**：`PRIVACY_POLICY_REVISION` 升至 4，旧同意（修订 1/2/3）升级后进入 `StaleRevision`，零业务读取；重新同意原子落盘后才读取五个应用。
10. **验收根三端写前全路径拦截（含联接与硬链接 `nlink > 1`）与测试密闭**：
    - Rust（`acceptance.rs`）、PowerShell（`prepare-native-acceptance.ps1`）、Node（`native-single-instance.mjs`）三处入口在**首次创建目录或写盘之前**，均须按规范化真实路径（解析最长存在祖先与符号链接/Windows 联接目标）拒绝真实受保护目录（`.tokenscope`、`.claude`、`.codex`、`.cc-switch`、`.workbuddy`、`.workbuddy-ai`、`.gemini/antigravity-cli`）的**相等路径、祖先路径、任意新/旧子目录与联接别名**（主目录本身拒相等与祖先）；
    - PowerShell 复用带 `manifest.json` 的已有旧根时，还必须在首笔写入前**不跟随链接遍历已有整树，拒绝任何内部联接/符号链接/重解析点，并沿用 Node 已有规则（`native-single-instance.mjs:681`）拒绝任何硬链接数 `nlink > 1` 的已有普通文件（避免 `WriteAllText` 原位覆盖根外文件）；同时逐一核对全部拟写目录与目标文件的最长存在祖先均落在规范化 `-Root` 内且在受保护目录之外**；拒绝后磁盘两端零新增/零改动。全仓构造 `SummaryOptions` / `DiscoveryContext` 的测试必须密闭注入隔离根。

---

## 4. 分步任务清单

新增行为一律先写失败测试（解析类带手工合成最小 fixture，严禁复制真实日志），确认红灯后再实现；每个提交只解决一类问题；所有构造 `SummaryOptions` / `DiscoveryContext` 的测试必须注入隔离临时目录，严禁触碰真实 `~/.tokenscope` 与真实 `~/.gemini/antigravity-cli`；Rust 测试统一经 `python -B scripts/run_rust_tests.py -- cargo test ...` 执行。

| 任务 | 主要文件 |
| --- | --- |
| AG01 | `crates/tokenscope-core/src/{model,dedupe,history,pricing,model_display,aggregate,report}.rs`、`crates/tokenscope-core/src/import/ccs.rs`、`crates/tokenscope-core/tests/history_migration.rs`、`src-tauri/src/commands.rs`、`scripts/inspect_history.py`、`scripts/test_inspect_history.py` |
| AG02 | `crates/tokenscope-core/Cargo.toml`（`rusqlite` 启用内置 `serialize`）、新增 `crates/tokenscope-core/src/source/antigravity_cli.rs`、`crates/tokenscope-core/tests/antigravity_cli_source.rs`、`crates/tokenscope-core/tests/fixtures/antigravity_cli/`；接入 `src/source/mod.rs`、`src/report.rs`（`AntigravityCli` 内容指纹） |
| AG03 | `crates/tokenscope-core/src/{settings,acceptance,report}.rs`、`crates/tokenscope-core/src/source/discovery.rs`、`scripts/prepare-native-acceptance.ps1`、`frontend/scripts/lib/native-single-instance.mjs`、`scripts/fixtures/settings_migrations.json`、`scripts/test_upgrade_fixture.py`、相关 Rust/PS/Node 测试与全仓测试密闭审计 |
| AG04 | `crates/tokenscope-core/src/settings.rs`、`docs/privacy.md`、`scripts/{upgrade_fixture.py,test_upgrade_fixture.py,test_upgrade_helpers.ps1,prepare-native-acceptance.ps1}`、`frontend/` 隐私 mock 与原生同意测试 |
| AG05 | `frontend/src/{types.ts,lib/viewFilters.ts,components/AgentIcon.vue,views/Dashboard.vue,views/Settings.vue,lib/locales/zh-CN.ts,lib/locales/en-US.ts}`、相关单测与 UI 契约、`DESIGN.md` |
| AG06 | `scripts/prepare-native-acceptance.ps1`、`frontend/scripts/contracts/native-identity.mjs`、`frontend/scripts/lib/native-unified.mjs`、`frontend/scripts/native-acceptance.mjs` 及纯函数契约测试 |
| AG07 | 隔离临时目录只读对账脚本（不入库，共读冻结树或编码「规范化相对路径 + 存在状态 + 长度 + 每文件 SHA-256」清单摘要）与对账摘要回填本计划 |
| AG08 | `docs/stats-semantics.md`、`CLAUDE.md`、`docs/plans/README.md`、本计划交付记录、`scripts/build-inputs.json`、版本递增与基线检查 |

### AG01 应用枚举、历史库 v6 逐文件观察、项目优先级与未知模型计价拦截

- `AgentKind` 新增 `AntigravityCli`（serde 与 `as_str()` / `parse()` 为 `"antigravity-cli"`，纳入 `AgentKind::ALL`；显示名 `"Antigravity CLI"`）；`model.rs` 新增 `pub const UNKNOWN_MODEL: &str = "unknown";`。
- `UsageEvent::native_identity()` 与 `same_source_identity()` 为 `AntigravityCli` 返回 `Some(("antigravity-cli-step", format!("{session_id}|{record_id}")))`。
- `dedupe.rs`：将 `AntigravityCli` 纳入强身份组（按 `(agent, session_id, record_id)` 去重），并将同轮落败的 `AntigravityCli` 跨文件观察透传给调用方写入 `event_file_observations`；`dropped` 排序表补齐顺序 4。
- `history.rs`（`HISTORY_SCHEMA_VERSION: 5 → 6`）与 `report.rs`：
  - v6 幂等迁移新增 `event_file_observations` 表（不改既有表行，保留升级前备份）；同步 `tests/history_migration.rs` 等处硬编码 `== 5` 的版本断言；
  - `event_write_of` 对 `AntigravityCli` 的 `UNKNOWN_PROJECT` 写 `project_key: None`（与 CCS `import/ccs.rs:940` 一致），并携带各源文件的规范化观察；
  - `AntigravityCli` 写入按 `(event_id, norm_source_file)` UPSERT 最新文件观察，再在 `event_file_observations WHERE event_id = ?` 全集合上按统一跨文件代表规则重算胜出观察 `winner` 并更新 `usage_events`（含同步 `usage_events.event_key`）；项目归属严格执行不变量 4 的三层优先级（① `winner` 有项目取 `winner` → ② 否则从当前所有带项目的观察按确定性代表序选一条取齐 `project_key`/`session_initial_cwd`/`event_cwd` → ③ 仅当全部当前观察均缺项目才沿用 `existing` 上次确认值）。
- `pricing.rs`、`model_display.rs`、`aggregate.rs`、`report.rs`：
  - 在汇总与明细共用的计价入口新增应用感知判断（`estimate_for_agent`）：对 `AntigravityCli` 且 `model.trim().eq_ignore_ascii_case(UNKNOWN_MODEL)` 无条件返回 `None`；
  - 展示名目录查询对 `AntigravityCli` 的 `UNKNOWN_MODEL` 返回未解析，聚合层按 `docs/stats-semantics.md:699` 以原始模型 ID 填充最终模型分组 `label`（`Some("unknown")`）。
- `import/ccs.rs`、`src-tauri/src/commands.rs`、`scripts/inspect_history.py` 与 `scripts/test_inspect_history.py`：补齐 `"antigravity-cli"` 映射/白名单，CCS 保持不支持，保留未知 app 拒绝断言。
- 必测回归：
  - **真实 v5 → v6 历史库升级测试（必测）**：构造真实 v5 库结构（含既有事件、别名、来源、`cache_write_5m`/`cache_write_1h` 与 `user_version = 5`），经 `HistoryDb::open` 升级至 v6，断言备份生成、新表 `event_file_observations` 就绪、原事实与别名无损，以及第二次打开幂等不重复备份；
  - **三轮/四轮跨文件更正与等长同 `mtime` 回归（必测）**：Round 1 `P = 100` → Round 2 `Q = 70`（得 100）→ Round 3 `P` 同刻改写为 `80`（得 **`80`**）→ Round 4 `P` **保持文件字节长度与 `mtime` 不变**改写为 `60` 并以 **`refresh = false`** 走正常入口（靠内容指纹触发重解析，增量库收敛到 **`70`**），每步均与冷库全扫逐项一致；另测同轮冷扫 `{P=80, Q=70}` 后下轮仅改 `P → 60` 仍得 `70`；
  - **项目更正优先级与防降级（必测）**：① `P` 为用量胜出者但缺项目、`Q` 初始提供项目 `A` 入库 → `Q` 将项目更正为 `B` 后增量库更新为 **`B`**（且 `session_initial_cwd`/`event_cwd` 同源取自 `Q`），与冷库一致；② 全部当前观察均缺项目（如摘要库缺失/不可读，含文件搬迁/行号重排/token 改写）时保留库内已确认项目不降级；
  - **有效外置 `pricing.toml` 未知模型拦截（必测）**：使用合法 `[[model]]` 配置 `prefix = "unknown"` 与 `prefix = "provider/unknown"` 的非零单价；**先正例断言**普通 `pricing.estimate("unknown", ...)` 确实命中外置价格，**再反例断言**：名称目录查询返回未解析；汇总纯未知模型分组满足 `total_cost_usd == None`、`unknown_pricing == true`、`unknown_tokens == 事件全量`、`label.as_deref() == Some("unknown")`；请求明细行满足 `model == "unknown"`、`cost_usd == None`、`cost_breakdown == None`；同报告已知模型正常计价。

### AG02 解析器 `source/antigravity_cli.rs` 与写保护零写入工作区映射

- `crates/tokenscope-core/Cargo.toml`：`rusqlite` 的 features 增加内置 `"serialize"`（零新 crate，验证 `--locked` 构建通过）。
- **发现规则（`discover_with_ledger`）**：
  - 默认根 `~/.gemini/antigravity-cli`（验收模式走 `<root>/sources/antigravity-cli`）；
  - 登记 `acceptance::record_path("source_root_read", ...)`，经 `DirVisitLedger` 防环与多根认领；
  - 仅收集 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若 `<root>` 本身是 `brain/` 形态则收集 `<root>/<conv_id>/.system_generated/logs/transcript.jsonl`），忽略 `transcript_full.jsonl`、`chunks/`、空目录、`conversations/*.db`、`history.jsonl`、`scratch/`；软链/联接越界或失效跳过并诊断。
- **受写入保护的零写入 `conversation_summaries.db` 加载与缓存**：
  - 仅定位 `<root>/conversation_summaries.db`，绝不向上越界；
  - 按不变量 5 在 Windows 上以 `share_mode(FILE_SHARE_READ)` 短暂持柄读取 `db`、校验 `-journal`、读取可选 `-wal`（读入总字节 ≤ 64 MiB）后立刻释柄；在纯内存按官方 WAL 三分类（旧代残留 / 未提交尾帧 / 同代完整帧校验失败）与扩展后映像 ≤ 64 MiB 重放，调用 `conn.deserialize_read_exact(rusqlite::MAIN_DB, image.as_slice(), image.len(), true)` 载入 `:memory:`，仅执行 `SELECT conversation_id, workspace_uris FROM conversation_summaries`；
  - 进程内缓存键为**规范化 DB 路径**（Windows 小写），值以持柄读出快照的 `(db_bytes, wal_bytes)` **SHA-256 内容摘要**判命中（不依赖 Windows `mtime`）；失败态 `Unusable` 不进缓存、输出诊断；`revision` 仅对 `(state, 排序后的 conv_id -> normalized_project)` 计算摘要。
- **单文件解析与内容指纹**：
  - `report.rs` 对 `AgentKind::AntigravityCli` 的文件缓存指纹采用 `size:mtime_ms:sha256`（仅作用于 Antigravity CLI，不影响现有四应用）；
  - `session_id = <conv_id>`；逐行 serde 宽松解码，畸形 JSON 计 `bad_lines`；按 `step_index` 收敛到 `BTreeMap<u64, ParsedStep>`（后写覆盖前写，记录最终行号）；
  - 按 `step_index` 升序遍历，仅对 `USER_INPUT|USER_EXPLICIT` 在最后一个 `</USER_REQUEST>` 之后的 `<USER_SETTINGS_CHANGE>` 块内提取 `Model Selection`；对准入的 `PLANNER_RESPONSE|MODEL|DONE` 与带 token 的 `CHECKPOINT|SYSTEM|DONE` 执行 FRESH 桶映射、受检算术、`validate_buckets()`、UTC 时间校验、模型回退 `UNKNOWN_MODEL`、工作区回退 `UNKNOWN_PROJECT`。
- **合成 fixture 契约测试（`tests/antigravity_cli_source.rs`）**：
  - 发现：根不存在；空会话目录；忽略 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`conversations/*.db`、`history.jsonl`、`scratch/*.jsonl`；联接环/断链；直接指向 `brain/` 根。
  - **写保护零写入快照专项（必测两类状态 + WAL 三分类）**：
    - **状态 A（写者已退出、留有已提交 WAL 且无 `-shm`）**：构造 WAL 合成库将最新工作区仅提交到 `-wal`（不 checkpoint），关闭写连接并删除 `-shm` → 持 `FILE_SHARE_READ` 读出最新路径，且源目录未创建 `-shm`、文件清单与全部字节/mtime 零变化；
    - **状态 B（写者正在持柄，含提交前脏页写回主库）**：① 回滚日志模式下开启未提交事务写入 1,200 行迫使脏页刷入主库且未提交（持柄中）→ 加载器被内核拒绝共享写，返回 `MappingState::Unusable`，绝不读出未提交脏页；② WAL 模式下写连接持柄未关闭 → 同样返回 `MappingState::Unusable`；两种情形下均断言源目录零写入、老事件保原项目、新事件记未知；另测崩溃遗留非空 `-journal` 返回 `Unusable`；
    - **WAL 三分类与双重 64 MiB 上限**：① checkpoint 后旧代残留帧（不同 salt）→ 正常截断并应用此前提交；② 未提交尾帧（同代合法校验和但 `db_size == 0`）与最后提交后的 EOF 不足一帧残尾 → 忽略尾部、应用此前提交；③ WAL 头损坏或**同代完整帧翻字节致校验失败** → 必须返回 `Unusable`（绝不静默回退旧提交）；④ 实际读入字节超 64 MiB 与 `db_size_after_commit * page_size` 扩展映像超 64 MiB 均返回 `Unusable`。
  - **双根缓存隔离、单轮冻结映射与失效粒度专项（必测）**：两个不同手动根下放置大小与修改时间相同但内容不同的 `conversation_summaries.db`，各自命中正确项目不串用；修改已有会话 `title`/`last_modified_time`（不改 `workspace_uris`）验证 `revision` 不变、不触发重解析；仅向摘要库新增工作区记录（即便保持相同 `mtime`）验证 `revision` 与 `collection_key` 同步变化且下次查询（`refresh = false`）立即补齐项目路径；验证单轮 `collection_key`、解析器与 `remember_collection` 消费同一冻结映射。
  - **普通用户消息引用切换模板反例（必测）**：`<USER_REQUEST>` 内用户消息原文包含 `The user changed setting \`Model Selection\` from None to Fake Model (High).` 及伪造 `<USER_SETTINGS_CHANGE>...</USER_SETTINGS_CHANGE>`，均**不得**改变 `session_model`；`PLANNER_RESPONSE` / `GENERIC` 正文同理不误匹配；只有 `</USER_REQUEST>` 之后真实系统块 `<USER_SETTINGS_CHANGE>` 才生效，且原文绝不出现在任何事件字段或诊断文本中。
  - 口径、步骤准入、畸形容错与跨日时间边界测试同前（FRESH、`CHECKPOINT`、`output == 0` 保留、全 0 跳、坏行计数、`file://` 百分号编码中文路径与 Claude 同 key、UTC 跨日）。

### AG03 来源发现、冻结映射 `collection_key`、设置格式 v4 与三端验收写前保护

- `settings.rs`：
  - `AgentSources` 新增 `pub antigravity_cli: Option<SourceConfig>`（缺失 = 启用 + 自动发现），更新 `config` / `config_mut` 与 `SETTINGS_TEMPLATE`；
  - `SETTINGS_FORMAT_REVISION` 由 `3` 升至 `4`。
- `scripts/fixtures/settings_migrations.json` 与 `scripts/test_upgrade_fixture.py`：
  - `"1->4"`：继承 `claude`/`codex` 的 `source_dir_to_additional_path` + `sources.workbuddy.enabled` / `sources.workbuddy_ai.enabled` / `sources.antigravity_cli.enabled` 的 `default(true, allow_missing=true)`；
  - `"2->4"`：`sources.workbuddy.enabled` / `sources.workbuddy_ai.enabled` / `sources.antigravity_cli.enabled` 的 `default(true, allow_missing=true)`；
  - `"3->4"`：仅 `sources.antigravity_cli.enabled` 的 `default(true, allow_missing=true)`（保留 v3 用户对 WorkBuddy 的显式配置）；
  - `"4->4"`：**恒等迁移（`"operations": []`）**，新增单测验证 `sources.antigravity_cli.enabled = false` 在 `4->4` 升级校验下通过且保持停用。
- `source/discovery.rs` 与 `report.rs`：
  - 固定候选集追加 `AntigravityCli` 默认根 `~/.gemini/antigravity-cli`，跨应用冲突与同应用重叠沿用 `resolve_sources`；
  - 在采集入口为本轮一次性冻结已启用 Antigravity CLI 根的 `WorkspaceMapping`，由 `collection_key`（`Some(plans)` 与 `None` 双分支，停用固定 `"disabled"`）、`AntigravityCliSource` 解析器与 `remember_collection` 共用同一冻结映射；
  - `SummaryOptions` 增加 `antigravity_cli_dir` 与 `antigravity_cli_enabled`，审计全仓所有测试构造点补齐密闭注入。
- **三处验收根写前全路径保护入口同步加固**：
  1. `crates/tokenscope-core/src/acceptance.rs`：`source_dir_override(AntigravityCli)` → `<root>/sources/antigravity-cli`；受保护清单追加 `home_canon.join(".gemini").join("antigravity-cli")`；所有受保护应用目录（`.tokenscope`、`.claude`、`.codex`、`.workbuddy`、`.workbuddy-ai`、`.gemini/antigravity-cli`）均同时拒绝相等、祖先、任意子目录（`canon.starts_with(&protected)`）与联接别名（用户主目录 `home_canon` 仍只拒相等与祖先）；
  2. `scripts/prepare-native-acceptance.ps1`：在任何 `New-Item` / 写盘之前完成两层预检：① 解析 `-Root` 与全部受保护目录（补充 `.workbuddy`、`.workbuddy-ai`、`.gemini\antigravity-cli`、`.cc-switch`）的最长存在祖先真实路径，按路径分量边界拦截相等、祖先、新/旧子目录与联接别名；② 复用已有带 `manifest.json` 的旧根时，**不跟随链接遍历已有整树拒绝任何内部联接/符号链接/重解析点，并沿用 Node 已有规则（`native-single-instance.mjs:681`）经 `GetFileInformationByHandle` 检查拒绝任何 `nlink > 1` 的已有硬链接普通文件，同时逐一校验所有拟写目录与目标文件的最长存在祖先均落在规范化 `-Root` 内且在受保护目录之外**；单测用临时目录分别构造「合法带 `manifest.json` 旧根 + `sources\antigravity-cli` 内部联接指向外部目录」与「合法带 `manifest.json` 旧根 + 拟写目标为指向外部文件的硬链接（`nlink = 2`）」，直接调用 PowerShell 脚本，断言两种均拒绝退出且两端零新增/零改动；
  3. `frontend/scripts/lib/native-single-instance.mjs`：`getUserProtectedDirs` 追加 `path.join(dir, ".gemini", "antigravity-cli")`，补充覆盖真实根、祖先 `~/.gemini`、新子目录 `~/.gemini/antigravity-cli/new-sub` 与联接别名的契约测试，断言拒绝后零新增文件/目录。

### AG04 隐私政策修订 4 与重新同意

- `crates/tokenscope-core/src/settings.rs`：`PRIVACY_POLICY_REVISION` 由 `3` 升至 `4`。
- `docs/privacy.md`：
  - 更新日期与适用范围（共同同意修订号：4）；
  - §2.1 支持工具列表新增 **Antigravity CLI**（默认根目录 `~/.gemini/antigravity-cli`）；
  - §2.1「数据处理豁免与红线条款」**明确、如实披露** Antigravity CLI 的全部读取与内存处理边界：
    - 结构化用量元数据：只读 `brain/<id>/.system_generated/logs/transcript.jsonl` 的时间戳、模型标识、步骤编号、步骤类型/状态与 token 计数；
    - **`USER_INPUT.content` 窄范围内存检查披露**：为兼容尚未在响应行直接记录模型名的历史会话，本应用会在本地内存中检查 `USER_INPUT`（`source = "USER_EXPLICIT"`）记录的 `content` 字段中**位于 `</USER_REQUEST>` 标签之后**的系统注入块 `<USER_SETTINGS_CHANGE>`，仅匹配固定前缀 `The user changed setting \`Model Selection\` from ... to ...` 以提取并仅保留归一化后的模型名；不解析 `<USER_REQUEST>` 内的用户提问正文，不落库、不进诊断日志、不展示、不外传任何 `USER_INPUT.content` 原文；
    - **工作区索引内存零写入处理披露**：为在源目录零写入且避免读取未提交脏页的前提下取得一致项目路径，本应用会短暂持有只读句柄将同根 `conversation_summaries.db` 及其配套 `-wal` 的原始数据库页载入本地进程内存供只读解析，**SQL 查询严格限定仅读取 `conversation_id` 与 `workspace_uris` 两列，仅保留归一化后的项目路径映射**；文件内其它页面与列（如 `title`、`preview`、`conversations_fts`、`raw_summary`）不被 SQL 读取、不落库、不进诊断日志、不展示、不外传，内存映像在查询完成后立即释放；
    - 明确排除清单：`conversations/*.db`、`transcript_full.jsonl`、`chunks/`、`history.jsonl`、`settings.json`、`annotations/`、`implicit/`、`log/`、`cache/`、`steps/*/output.txt`、`~/.gemini/config` 及桌面版/IDE/ACP 目录；
  - §9.2 修订历史新增「共同同意修订 4」。
- 同步升级与同意契约：`scripts/prepare-native-acceptance.ps1`、前端 `privacySnapshot` mock、`scripts/upgrade_fixture.py` / `scripts/test_upgrade_fixture.py` / `scripts/test_upgrade_helpers.ps1`、原生同意契约测试，核验旧同意（修订 1/2/3）先被拒且零业务读取，授予修订 4 后进入业务。

### AG05 前端五应用适配与设计规范同步

- `frontend/src/types.ts` 与 `frontend/src/lib/viewFilters.ts`：`AgentFilter`、`SourcePathRow.agent`、`AGENT_LABEL`、`AGENTS` 增加 `"antigravity-cli"`（标签 `"Antigravity CLI"`）。
- `frontend/src/components/AgentIcon.vue`：`"antigravity-cli"` 映射到已随包打包的 `googlegeminiSvg`（按钮保持 `aria-label="Antigravity CLI"` 与悬停提示）。
- `frontend/src/views/Dashboard.vue`：`agentOptions` 扩为 6 项纯图标选项（全部 + 5 应用），验证 980×620 与 1280×820 双主题不换行不溢出。
- `frontend/src/views/Settings.vue` 与 `zh-CN.ts` / `en-US.ts`：`AGENT_IDS` 扩为 5 项，Antigravity CLI 分区附固定说明（仅统计 Antigravity CLI 本地会话，不含桌面版/IDE；缺失工作区或模型记录的历史会话将如实显示为未知）；同步 CCS 导入说明文案。
- `DESIGN.md`：同步 6 图标来源控件（明确 Antigravity CLI 复用随包 `googlegemini.svg`）与设置页 5 来源分区约束。
- 单测与 UI 契约：更新/新增 `Dashboard.test.ts`、`Settings.test.ts`、`viewFilters.test.ts`、`locales.test.ts` 与浏览器契约。

### AG06 原生验收接线与场景覆盖

- `scripts/prepare-native-acceptance.ps1`：在 `<root>/sources/antigravity-cli` 下生成合成数据：
  - 主会话 `transcript.jsonl`（含 `<USER_REQUEST>...</USER_REQUEST>` 内的假切换反例 + `</USER_REQUEST>` 后的真实 `<USER_SETTINGS_CHANGE>`、显式 `model` 的 `PLANNER_RESPONSE`、带 token 的 `CHECKPOINT`、未完成行、坏行）；
  - 无模型子代理会话 `transcript.jsonl`（验证 `"unknown"` 保留、分组展示名 `"unknown"` 且费用未知）；
  - 无工作区会话 `transcript.jsonl`（验证归入 `"(未知)"` 并可下钻）；
  - `conversation_summaries.db`（含与 Claude 同路径的 `file://` URI）。
- 扩展 `ui-identity`、`ui-source-paths` 与隐私重新同意（3 → 4）场景；核验 `isolation_runtime_guard` 零越界、零未授权写入。

### AG07 本机真实数据只读对账（带路径/长度/内容清单摘要）

- 全部输出目录（`history.db`、缓存、价格索引）指向隔离临时目录。
- **可证明同输入双边对账（防元数据滞后与同字节换路径移花接木）**：
  1. 在不改动 `~/.gemini/antigravity-cli` 的前提下，采用「两端共读同一份冻结目录树，或两端各自对**实际读入的全部目标条目（每个 `brain/<conv_id>/.system_generated/logs/transcript.jsonl` 以及 `conversation_summaries.db` / `-wal` / `-journal`）按规范化相对路径升序编码 `(rel_path, exists, byte_len, file_sha256)` 构成清单再计算整体 SHA-256 摘要**」：
     - 仅当 `rust_manifest_sha256 == python_manifest_sha256` 时接受本轮对账（同字节文件从 `brain/A` 移到 `brain/B` 会改变 `rel_path` 从而改变清单摘要）；若活跃会话在双边之间发生写入导致摘要不一致，丢弃并重试直到摘要一致；
  2. 在同清单摘要截点上逐项核对总请求数（`PLANNER_RESPONSE` + 带 token `CHECKPOINT` + 全部子代理）、四桶精确合计、已知/未知模型分布、已知/未知项目分布实现 **0 偏差**，坏行数为 0 或逐行说明原因；同时验证源目录无新增 `-shm`、无新增/变更文件，真实 `~/.tokenscope` 零变化；
  3. 仅将脱敏汇总计数、采样时刻与输入清单摘要前 12 位回填本计划，严禁留存任何会话正文或敏感路径。

### AG08 文档、构建清单与交付

- `docs/stats-semantics.md`：新增 §2.4「Antigravity CLI（`~/.gemini/antigravity-cli`）」与 §3 去重/逐文件观察重算说明。
- `CLAUDE.md`：更新「项目目标」为五应用统计，更新「候选 agent」与 Antigravity CLI 口径要点。
- `scripts/build-inputs.json`：运行 `python -B scripts/build_inputs.py --update` 并 `--check`。
- 版本与基线（仅在用户确认并完成实现后执行）：
  - `python -B scripts/bump_version.py --bump minor --summary "新增 Antigravity CLI 用量来源支持"`（`0.9.10 → 0.10.0`），核验 `v0.9.10` 已在 `upgrade_baselines.json` 登记；
  - 跑通全量本地门禁（`TOKENSCOPE_PRECOMMIT_MODE=full`、Rust 普通/acceptance 双模式 clippy+test、前端 typecheck/format/test/build/E2E、PowerShell/Python 契约与原生验收场景），将实测证据回填本计划与 `docs/plans/README.md`。

---

## 5. 风险与既有确认记录

1. **升级起点矩阵 `MAX_ORIGINS = 6`**：本版 `0.10.0` 选点 6 个（`0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.10`），不超上限，本轮维持 `MAX_ORIGINS = 6`；本版 `settings_format = 4` 与 `history_schema = 6` 合并在同一版发布，不额外拆分基线组；下一版（`0.10.1` / `0.11.0`）登记 `0.10.0` 后将触发第 7 个起点守卫并按规范另行评审。
2. **图标方案（已确认）**：复用 `frontend/src/assets/icons/googlegemini.svg`，可访问名称保留 `Antigravity CLI`。
3. **预览模型与 `unknown` 模型的费用显示**：2026-11-18 前旧子代理的 `"unknown"` 模型（由共享计价路径强制返回未定价）及未入公开价格目录的预览模型，在未配置外置价格时均如实显示费用「未知」（橙色），不按 `$0` 估算。
4. **单 PR 交付节奏**：AG01–AG08 在 `feat/antigravity-cli-adapter` 分支内按任务逐提交推进，作为一个完整产品 PR 交付。
