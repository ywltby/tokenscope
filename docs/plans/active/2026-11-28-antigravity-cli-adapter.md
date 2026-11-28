# 新增 Antigravity CLI（`antigravity-cli`）用量来源实施计划

> 状态：**已按首轮审查意见修订（2026-11-28），待最终确认后开工（方案文档，尚不分配发行版本）**。
>
> 用户 2026-11-28 已拍板口径与确认项：
> 1. **数据源与历史模型补齐策略（方案 A）**：主流程只读 `brain/<id>/.system_generated/logs/transcript.jsonl`，并严格只读同根目录 `conversation_summaries.db` 补工作区路径 `workspace_uris`；不打开、不解析内部 `conversations/*.db` Protobuf；
> 2. **`CHECKPOINT` 压缩摘要调用**：与 `PLANNER_RESPONSE` 一并计入用量；
> 3. **名称、图标与标识**：展示名 **Antigravity CLI**，图标复用仓库已打包的 `frontend/src/assets/icons/googlegemini.svg`（可访问名称与悬停提示保留 `Antigravity CLI`），内部枚举 `AgentKind::AntigravityCli`（序列化与库内标识 `antigravity-cli`），默认根固定 `~/.gemini/antigravity-cli`；暂不接入桌面版 `~/.gemini/antigravity`、`antigravity-ide`、`antigravity-acp`；
> 4. **首要目标与未知口径**：保证获取到的数据是真实的；缺少工作区路径（pwd）或模型名时如实标为未知，**未知的处理参照项目导入 CCS 的未知处理**（不丢弃有效用量、不伪造项目路径或模型名、不在计价层被外置价格误命中、不静默按 `$0` 计价）；
> 5. **升级矩阵上限**：本轮维持 `MAX_ORIGINS = 6`（`0.10.0` 选点实测 6 个，满足上限；下一版登记 `settings_format = 4` 后另行评审）。
>
> 本计划按用户明确排期解除 `CLAUDE.md`「候选 agent 暂缓排期」中对 Antigravity CLI 的暂缓。

**Goal：** TokenScope 只读采集本机 Antigravity CLI（`~/.gemini/antigravity-cli`）的本地会话与子代理转写用量，作为第五个独立应用纳入统一历史库、汇总、筛选、下钻、计价与来源管理；现有 Claude Code / Codex / WorkBuddy / WorkBuddy AI 的统计结果零变化。

**Architecture：**
- 新增独立适配器 `crates/tokenscope-core/src/source/antigravity_cli.rs`：只发现 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若用户手动指向 `brain/` 目录，也允许直接匹配其下 `<conv_id>/.system_generated/logs/transcript.jsonl`），天然排除 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`conversations/*.db` 与其它非目标文件。
- 工作区路径（`cwd`）通过同根 `<root>/conversation_summaries.db` 以**严格源目录零写入**策略读取（无有效 `-wal` 走 `immutable=1`；存在活跃 `-wal` 时仅用 OS 只读 `fs::read` 采集 `.db` + `.db-wal` 并在纯内存按 SQLite 官方 WAL 帧格式重放到最后已提交事务边界，经 `rusqlite` `serialize` 的 `deserialize_read_only` 加载至 `:memory:`，绝不在源目录调用普通 `mode=ro` 打开 WAL 或创建 `-shm`），且只查询 `(conversation_id, workspace_uris)` 两列。
- 隐私政策修订为共同同意修订 4（如实披露对 `USER_INPUT.content` 尾部 `<USER_SETTINGS_CHANGE>` 块的窄范围内存检查边界），设置格式修订为 v4（新增 `sources.antigravity_cli`，`4->4` 恒等迁移），前端来源筛选与设置分区同步扩展至五个应用；Rust / PowerShell / Node 三处验收入口均在写盘前拦截受保护目录及其子目录/祖先/联接别名。

**Tech Stack：** Rust（`tokenscope` 核心库、`tokenscope-app` 桌面壳，`rusqlite` 启用内置 `serialize` 特性，零新外部 crate）、Vue 3 + Naive UI、SQLite 历史库、PowerShell / Python / Node 原生验收脚本。

---

## 1. 背景与本机只读核验事实（2026-11-28）

研究参考：[cc-switch v4.0.18 的 agy 接入拆解](../../research/2026-10-22-cc-switch-agy-usage.md)、[Magpie 用量统计拆解](../../research/2026-10-28-magpie-usage-statistics.md) §15.8。2026-11-28 对本机 `~/.gemini/antigravity-cli` 在活跃对话期间分三批执行严格只读采样（15:04–15:09，采样期间当前活跃会话继续追加了 5 个新步骤，故不同脚本的分母略有递增，下表逐项标明采样批与分母；只统计结构、字段名、类型与数值，不留存会话正文与敏感内容）：

| 维度 | 采样批 / 分母 | 实测结论（历史样本观测，不作验收固定预期） |
| --- | --- | --- |
| 会话结构与子代理占比 | 第 3 批（15:08，全量 66 会话 / 15,701 条用量步骤） | 66 会话 = 24 主会话（16 非空 + 8 空，`steps = 0`）+ 42 子代理（`self` 40、`research` 2，`nesting_depth = 1`）。子代理落在独立 `brain/<sub_id>`，用量不重复计入父会话：主会话 12,825 条、子代理 2,876 条（约 18.3% 调用、约 28.4% 输出） |
| `transcript.jsonl` vs `transcript_full.jsonl` | 第 1 批（15:04，58 非空会话 / 32,527 行 / 15,696 条用量步骤） | 二者在 `step_index`、`type`、`source`、`status`、`created_at`、`model` 与全部 token 字段上 **0 差异**；精简版仅截断超长正文/工具输出，体积小约 26%（44.3 MB vs 60.0 MB）。`chunks/transcript/*.jsonl` 与 `chunks/transcript_full/*.jsonl` 是同字节分片副本，不得扫入 |
| 与内部 `.db` 交叉核验 | 第 2 批（15:07，transcript 15,700 条 vs `immutable=1` 主库 15,695 条） | 重叠的 **15,695 / 15,695 条**在 `input_tokens`、`cache_read_tokens`、`output_tokens` 上 **100% 精确相等（0 差异）**；差值 5 条全部来自活跃会话尚未 checkpoint 的 `.db-wal`（13.6 MB），而 `transcript.jsonl` 已实时落盘 |
| Token 桶语义 | 第 1/2 批（15,695–15,696 条） | 仅出现 `input_tokens`、`cache_read_tokens`、`output_tokens`（全库 `cache_write_tokens` 0 次）。连续轮次在 Claude 与 Gemini 上均满足 `上轮(input + cache_read) ≈ 下轮 cache_read`（且常见 `input_tokens ≪ cache_read_tokens`），证明 `input_tokens` 为 **FRESH 语义（已排除缓存读）**；`output_tokens` 满足 `thinking + non_thinking == output`（0 违例），**已包含思考 token** |
| 携带 token 的步骤类型 | 第 1 批（15:04，15,696 = 15,678 + 18） | ① `PLANNER_RESPONSE`（`source=MODEL`, `status=DONE`），15,678 条，100% 含 token（含 1 条 `output_tokens = 0` 但输入/缓存非 0 的合法记录）；② `CHECKPOINT`（`source=SYSTEM`, `status=DONE`），2026-11-09 起 18 条全部携带 token（上下文压缩摘要真实模型调用，单次约 250K cache_read、3K–4K output；早期 38 条无 token 字段）。无 `status != DONE` 携带 token 的行 |
| 模型字段与 `USER_SETTINGS_CHANGE` 结构 | 第 2 批（15:07）+ 第 4 批专项核验（16:13，267 条 `USER_INPUT`） | ① `2026-11-18` 起 `PLANNER_RESPONSE`（含子代理）直接带 `model`（2,092 条）；② `2026-10-13 ~ 2026-11-10` 主会话无行内 `model`（10,717 条），但全部 267/267 条 `USER_INPUT` 均以 `</USER_REQUEST>` 封闭用户提问，全部 17/17 次切换通知均落在 `</USER_REQUEST>` 之后的 `<USER_SETTINGS_CHANGE>\nThe user changed setting \`Model Selection\` from <old> to <new>. ...\n</USER_SETTINGS_CHANGE>` 系统注入块内（即使 11 条超长用户输入在 `transcript.jsonl` 被截断，尾部系统块仍 11/11 完整保留）；③ 同期 40 个子代理无 `model` 与切换块（2,872 条）；④ 18 条 `CHECKPOINT` 均无行内 `model`，18/18 与同会话前一条 `PLANNER_RESPONSE` 同模型 |
| 工作区路径 | 第 3 批（15:08，58 非空会话） | `transcript.jsonl` 不含 cwd；`conversation_summaries.db` 的 `workspace_uris`（JSON 数组 `["file:///..."]`）覆盖 58/58 非空会话（含全部 42 个子代理），与 `conversations/*.db` 0 差异 |
| 行序与重复 `step_index` | 第 1/3 批 | 单文件内偶发重复 `step_index`（2 次，均为 `GENERIC|RUNNING`）；`created_at` 为 UTC RFC3339 秒级，15,700/15,700 精确等于步骤开始时刻 |

### 1.1 当前代码基线复核（`main` @ `3b9b444`，应用版本 `0.9.10`）

| 项目 | 当前现状与实施影响 |
| --- | --- |
| 应用枚举泛化 | WB01 已完成：`AgentKind::ALL`、`AgentSources::config(AgentKind)`、`acceptance::source_dir_override(AgentKind)` 均已按枚举分发，无需前置泛化阶段 |
| 历史库与同刻更新 | `usage_events.app` 为 TEXT 无需改表结构；但现有 `history.rs:3069` 的 `same_instant_challenger_wins` 对同刻候选只保留总量大者，无法处理 Antigravity CLI 同一文件把某步从 100 更正为 80 的场景 → 须在 AG01 区分「同一源文件更正」与「另一根旧副本」 |
| 计价层与 `"unknown"` | `pricing.rs:2887` 的 `collect_candidates` 按末段等价键匹配，若外置 `pricing.toml` 声明了 `unknown` 或 `provider/unknown`，缺模型的 `"unknown"` 事件会被误计价 → 须在汇总与明细共用的计价入口显式拦截 Antigravity CLI 的未知模型哨兵 |
| 采集复用键与失效 | `report.rs:707` 的 `collection_key` 目前只拼入 `ClaudeCode` 映射修订；必须把已启用 Antigravity CLI 根的工作区映射修订同步拼入 `collection_key`，否则仅补齐摘要库后新查询会复用旧采集结果 |
| 验收保护三处入口 | `crates/tokenscope-core/src/acceptance.rs:316`（缺 `.gemini/antigravity-cli` 且未拦受保护目录的子目录）、`scripts/prepare-native-acceptance.ps1:93`（先写盘、缺 WorkBuddy 与 `.gemini/antigravity-cli`、未解析联接/子目录）、`frontend/scripts/lib/native-single-instance.mjs:116`（缺 `.gemini/antigravity-cli`）→ 三处均须在 AG03/AG06 同步补齐并验证拒绝后零新增文件 |
| 持久格式与协议门禁 | `SETTINGS_FORMAT_REVISION = 3`、`PRIVACY_POLICY_REVISION = 3`、`HISTORY_SCHEMA_VERSION = 5`、`PARSER_REVISION = 3`、`PARSE_RULES_REVISION = 7`、`INDEX_VERSION = 11`；`bump_version.py` 强制修改 `docs/privacy.md` 时递增 `PRIVACY_POLICY_REVISION` |
| 升级基线选点 | `upgrade_baselines.py select --current-version 0.10.0` 选中 6 个起点（`0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.10`），不超 `MAX_ORIGINS = 6`；维持现状 |

---

## 2. 范围与不做

**做：**
- AG01：应用枚举、历史库身份、去重规则、同源文件更正语义、按步骤强身份的项目防降级、Antigravity 未知模型共用计价拦截、壳层映射与历史核验工具白名单。
- AG02：`source/antigravity_cli.rs` 适配器（精准文件发现、严格源端零写入的 `conversation_summaries.db` 只读加载含活跃 WAL 内存重放、按规范化 DB 路径隔离的进程缓存与映射修订、transcript 解析、`<USER_SETTINGS_CHANGE>` 窄匹配与未知兜底、受检算术）与合成 fixture 契约测试。
- AG03：来源发现、`collection_key` 接线、设置格式 v4（`sources.antigravity_cli` 与完整 `1/2/3/4->4` 迁移规则，其中 `4->4` 为恒等迁移）、Rust/PowerShell/Node 三处验收保护入口同步与全仓测试来源密闭。
- AG04：隐私政策修订 4（`docs/privacy.md` 诚实披露 `USER_INPUT.content` 尾部 `<USER_SETTINGS_CHANGE>` 的内存匹配边界、摘要库两列读取与排除清单，`PRIVACY_POLICY_REVISION = 4`）及升级/同意契约同步。
- AG05：前端五应用适配（复用 `googlegemini.svg`、类型、筛选、来源控件、设置页第五来源分区与说明、中英文文案）与 `DESIGN.md` 同步。
- AG06：原生验收合成数据与 `ui-identity` / `ui-source-paths` / 隐私重新同意场景覆盖。
- AG07：本机真实数据只读对账（固定可比较输入截点，采集前后核验文件指纹未变，写入隔离目录）。
- AG08：`docs/stats-semantics.md`、`CLAUDE.md`、`docs/plans/README.md`、`scripts/build-inputs.json`、版本递增与交付验收。

**不做：**
- 不接入桌面版 GUI（`~/.gemini/antigravity`）、`~/.gemini/antigravity-ide`、`~/.gemini/antigravity-acp`；后续有真实样本与明确排期再另立项。
- 不打开 `conversations/*.db`（不解析私有 Protobuf，不承担内部字段号漂移风险）。
- 不读取 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`history.jsonl`、`settings.json`、`annotations/`、`implicit/`、`log/`、`cache/`、`brain/<id>/scratch/`、`steps/*/output.txt`、`~/.gemini/config`。
- `conversation_summaries.db` 严格只读 `conversation_id` 与 `workspace_uris` 两列，不读取 `title`、`preview`、`conversations_fts` 或 `raw_summary`。
- 不在 CCS 导入链路中新增 `agy` 映射（TokenScope 的 CCS 导入仍仅补充 Claude Code 历史）。
- 不增加环境变量派生根（使用 `--gemini_dir` 的私有目录通过设置页「手动追加目录」纳入）。
- 不增加主/子代理层级展示产品（子代理有效用量按既有口径一并纳入）。
- 不改动 Claude Code、Codex、WorkBuddy、WorkBuddy AI 的既有解析、身份、去重与计价口径。

---

## 3. 必须保持的不变量

1. **现有四应用统计零变化**：同一份 Claude Code / Codex / WorkBuddy / WorkBuddy AI 输入，改动前后的事件集合、四桶、去重、项目归属与费用逐项一致。
2. **数据真实优先与「未知」处理（参照 CCS 口径，且杜绝外置价格误命中）**：
   - **项目未知（缺 pwd / workspace）**：当 `conversation_summaries.db` 缺失、无该会话记录、`workspace_uris` 为空/非合法 JSON/全部 URI 无法经 `normalize_project_path` 规范化、或活跃 WAL 无法取得一致视图时，`UsageEvent.project = UNKNOWN_PROJECT`（`"(未知)"`），`session_initial_cwd = None`，`event_cwd = None`；写入 `history.db` 时 `project_key = NULL`（与 `import/ccs.rs` 逐列一致）。聚合按项目分组与下钻均归入 `"(未知)"` 组，绝不以会话 ID 或目录名伪造项目。
   - **模型未知（缺 model）**：行内无 `model`、同会话也无前置有效模型或 `<USER_SETTINGS_CHANGE>` 切换通知时，**绝不丢弃有效用量事件**，`model` 记为常量 `UNKNOWN_MODEL = "unknown"`（与 CCS 未知模型写法一致），`skipped_no_model` 不吞有效用量。
   - **未知模型强制不计价（汇总与明细共源）**：在 `aggregate.rs` 与 `report.rs` 共用的计价入口上，对 `AgentKind::AntigravityCli` 且 `model.trim().eq_ignore_ascii_case(UNKNOWN_MODEL)` 的事件**无条件返回 `None`（未定价）**——即使外置 `pricing.toml` 或快照刻意配置了 `unknown` 或 `provider/unknown` 的单价，也绝不命中、费用必须为 `None`、token 必须全量计入 `unknown_tokens` 并置 `unknown_pricing = true`；展示名查询同样对 `AntigravityCli` 的 `UNKNOWN_MODEL` 直接返回未解析，界面原样显示 `"unknown"`，绝不编造名称或从 `settings.json` 反推。
3. **桶互斥（FRESH 语义）与受检算术（SF08）**：
   - `input_tokens = raw.input_tokens`（**不减** `cache_read_tokens`）；
   - `cache_read_tokens = raw.cache_read_tokens`（缺失按 0）；
   - `cache_write_tokens = raw.cache_write_tokens`（缺失按 0，`cache_write_split_known = false`，`cache_write_5m = 0`，`cache_write_1h = 0`）；
   - `output_tokens = raw.output_tokens`（已包含思考 token，不另行叠加）。
   - 任何已出现的 token 字段为负数、浮点、布尔、字符串或超出 `u64`，或 `UsageEvent::validate_buckets()` 检查总和/prompt 溢出失败 → 计 `bad_lines` 跳过，不饱和、不回绕；四桶全为 0 → 计 `skipped_zero_usage` 跳过（允许 `output_tokens == 0` 但输入/缓存读非 0 的合法完成行）。
4. **事件准入、强身份去重、同源文件更正与按强身份防降级**：
   - 仅接纳两类已完成步骤：① `type == "PLANNER_RESPONSE" && source == "MODEL" && status == "DONE"`；② 至少携带一个 token 字段的 `type == "CHECKPOINT" && source == "SYSTEM" && status == "DONE"`（早期无 token 字段的 `CHECKPOINT` 静默跳过，不计坏行）。未完成步骤（`status != "DONE"`）不产出事件、不占去重身份。
   - 文件内按 `step_index` 收敛（后写覆盖前写）；缺失/非非负整数 `step_index` 的合资格用量行计 `bad_lines` 跳过。
   - 强身份方案 **`antigravity-cli-step`** = `"{session_id}|{step_index}"`（`record_id = step_index.to_string()`），在 `dedupe.rs` 与 `history.db` 中受 `app = "antigravity-cli"` 隔离，不与任何其他应用合并。
   - **同源文件更正（增量与冷库严格一致）**：对于 `AntigravityCli`，若入库写入 `w` 经强身份命中已存事实 `existing`（同 `NativeLog` 优先级），且 `existing` 在 `event_origins` 中的全部 `native_file` 来源均来自**同一个规范化源文件路径 `P`**，而本次 `w` 也来自该文件 `P`（即同一源文件发生步骤改写，如 step 7 在同 `created_at` 下由 100 token 改写为 80 token），允许 `w` 覆盖更正（同步更新 `usage_events.event_key = w.event_key`），使增量采集与冷库全扫收敛到同一数值；若 `w` 来自**另一根/另一文件路径 `Q != P`**（例如旧备份根副本），不触发同文件更正，沿用既有确定性代表规则，避免旧副本倒退主根终值。
   - **按步骤强身份的项目防降级与补齐**：`AntigravityCli` 的项目保护与补齐基于强身份 `antigravity-cli-step`（不依赖 `w.event_key == existing.event_key`——文件搬迁、行号重排或步骤更正都会改变 `event_key`）：
     - 若库内 `existing.project_key` 为 `Some(path)` 而新写入 `w.project_key` 为 `None`（摘要库缺失、损坏或活跃 WAL 暂无法取得一致视图），无论 `event_key` 是否变化，均保留库内已确认的 `project_key`、`session_initial_cwd`、`event_cwd` 不做降级覆盖；
     - 若库内 `existing.project_key` 为 `None` 而新写入 `w.project_key` 为 `Some(path)`（摘要库后续补齐）且核心事实（`ts`、`model`、`tokens`）一致或属于同源文件更正，允许按强身份更新补齐项目归属。
5. **严格源目录零写入与最小读取面**：
   - 仅收集 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若 `<root>` 本身是 `brain/` 形态则收集 `<root>/<conv_id>/.system_generated/logs/transcript.jsonl`），通过 `DirVisitLedger` 防环与多根认领，明确跳过 `transcript_full.jsonl` 与 `chunks/`；软链/联接越界或失效跳过并诊断。
   - **`conversation_summaries.db` 零写入一致视图**：
     - 绝不在源目录上以普通 `SQLITE_OPEN_READ_ONLY` 打开 WAL 库（SQLite 会在缺少 `-shm` 时于源目录创建 `-shm`，违反只读不变量）；
     - **路径 A（无活跃 WAL：`db-wal` 不存在或大小 ≤ 32 字节）**：使用 `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI` + `file:...?mode=ro&immutable=1` 直接只读打开，零 `-shm`/`-wal` 创建；
     - **路径 B（活跃 WAL：`db-wal` 存在且大小 > 32 字节）**：先检查总大小上限（默认 64 MiB，防异常大文件 OOM），以纯 OS 只读 `std::fs::read` 依次读取主库与 `-wal` 字节，并在读取前后比对 `(db_size, db_mtime, wal_size, wal_mtime)`（变化则最多重试 1 次）。快照一致后，在纯内存中按 SQLite 官方 WAL 文件格式校验 32 字节头（magic `0x377f0682`/`0x377f0683` 与头校验和）及逐帧 `salt1/salt2` 与累计校验和，遇首个 salt 或校验和失配立即停止（兼容 checkpoint 后遗留的旧代帧），**仅重放至此前最后一个有效提交帧（`db_size_after_commit > 0`；无有效提交帧时直接保留主库映像）**，截断/扩展内存映像至该页数并在内存中把头字节 18–19（WAL 版本号 2）改写为 1，经 `rusqlite` 内置 `serialize` 特性 `deserialize_read_only` 载入 `:memory:` 连接执行只读查询 `SELECT conversation_id, workspace_uris FROM conversation_summaries`；**全程源目录零写入、零 `-shm` 创建、零磁盘临时文件**。
     - 若两次尝试期间文件持续变化、超大小上限或 WAL 校验/解码失败，视为本轮无法取得一致视图（`MappingState::Unusable`）：不回退源目录普通只读打开、不缓存失败态、输出可读诊断；已入库事件按不变量 4 保留原有归属，新事件暂记未知项目，待下一轮补齐。
6. **工作区缓存根隔离、查询失效与失效粒度**：
   - 进程内工作区映射缓存以**规范化 DB 路径**（Windows 统一小写）为主键，值内再比对 `(db_size, db_mtime, wal_size, wal_mtime)`——不同手动根即使摘要库大小与修改时间相同也完全隔离、互不串用。
   - 映射修订值 `revision` 仅对 `(state, 排序后的 conversation_id -> normalized_project 映射表)` 计算确定性摘要（不含 `title`、`preview`、`last_modified_time` 或 WAL 字节指纹）：
     - 同会话继续聊天更新了摘要库 `mtime`/WAL 但 `(conversation_id -> workspace)` 映射集合未变时，`revision` **保持不变**，不触发整根 transcript 重解析；
     - 仅当新会话出现有效工作区、已有会话工作区变化或 DB 状态迁移（`missing ↔ loaded ↔ unusable`）时，`revision` 才变化，此时按现有 `Source::context_revision` 机制使该根下文件缓存失效并整根重解析一次（与 Claude 项目映射同粒度）。
   - `report.rs` 的 `collection_key`（计划模式与单根模式两条分支）必须纳入所有已启用 Antigravity CLI 根的 `norm_db_path@revision`（停用时固定 `"disabled"` 且不读库），保证仅补齐 `conversation_summaries.db` 后，新查询不复用旧采集结果。
7. **`USER_INPUT.content` 窄匹配与隐私披露边界**：
   - 仅在 `type == "USER_INPUT" && source == "USER_EXPLICIT"` 中，先定位最后一个 `</USER_REQUEST>` 关闭标签，**只检查该标签之后**的系统注入块 `<USER_SETTINGS_CHANGE>...</USER_SETTINGS_CHANGE>`（`<USER_REQUEST>` 内的用户提问正文完全跳过），在块内严格匹配 `The user changed setting \`Model Selection\` from <old> to <new>.`（句号后接空白/换行/行尾/`<` 或反引号截止，保留版本号内小数点）；`<new>` 为空或 `None` 忽略；显示名归一与 `agy` 原生 slug 一致（如 `Gemini 3.8 Flash (High)` → `gemini-3.8-flash-high`，`Claude Opus 4.6 (Thinking)` → `claude-opus-4.6-thinking`；已是无空格无括号的 ID 原样保留）。绝不扫描 `PLANNER_RESPONSE` / `GENERIC` 正文，也不把普通用户消息（即使原文引用了切换模板甚至嵌套了 `<USER_SETTINGS_CHANGE>` 标签）当作模型切换。
   - 仅在内存中提取并保留归一化后的模型名，**`USER_INPUT.content` 原文绝不落库、不进诊断日志、不展示、不外传**，并在 `docs/privacy.md` 修订 4 中对这一窄范围处理作明确书面披露。
   - 模型解析优先级：行内显式 `model`（同时推进同会话 `session_model`）> 同会话前序 `session_model`（含 `<USER_SETTINGS_CHANGE>` 与前一条 `PLANNER_RESPONSE` 模型，覆盖 `CHECKPOINT`）> `"unknown"`。
8. **项目身份与时间口径**：`workspace_uris` 首个经 `project_path::normalize_project_path` 成功的 URI 作为会话项目身份，跨五应用同一目录归同一 key；`created_at` 按 UTC RFC3339 原样存储（步骤开始时刻），缺失或非法计 `bad_lines` 跳过，展示沿用既有单次时区转换。
9. **同意先于读取**：`PRIVACY_POLICY_REVISION` 升至 4，旧同意（修订 1/2/3）升级后进入 `StaleRevision`，零业务读取；重新同意原子落盘后才读取五个应用。
10. **验收根三端写前拦截与测试密闭**：Rust（`acceptance.rs`）、PowerShell（`prepare-native-acceptance.ps1`）、Node（`native-single-instance.mjs`）三处入口在**首次创建目录或写盘之前**，均须按规范化真实路径（解析最长存在祖先与符号链接/Windows 联接目标）拒绝真实受保护目录（含 `~/.gemini/antigravity-cli`）的**相等路径、祖先路径、任意新/旧子目录与联接别名**；拒绝后磁盘零新增文件/目录。全仓构造 `SummaryOptions` / `DiscoveryContext` 的测试必须密闭注入隔离根。

---

## 4. 分步任务清单

新增行为一律先写失败测试（解析类带手工合成最小 fixture，严禁复制真实日志），确认红灯后再实现；每个提交只解决一类问题；所有构造 `SummaryOptions` / `DiscoveryContext` 的测试必须注入隔离临时目录，严禁触碰真实 `~/.tokenscope` 与真实 `~/.gemini/antigravity-cli`；Rust 测试统一经 `python -B scripts/run_rust_tests.py -- cargo test ...` 执行。

| 任务 | 主要文件 |
| --- | --- |
| AG01 | `crates/tokenscope-core/src/{model,dedupe,history,pricing,model_display,aggregate,report}.rs`、`crates/tokenscope-core/src/import/ccs.rs`、`src-tauri/src/commands.rs`、`scripts/inspect_history.py`、`scripts/test_inspect_history.py` |
| AG02 | `crates/tokenscope-core/Cargo.toml`（`rusqlite` 启用内置 `serialize`）、新增 `crates/tokenscope-core/src/source/antigravity_cli.rs`、`crates/tokenscope-core/tests/antigravity_cli_source.rs`、`crates/tokenscope-core/tests/fixtures/antigravity_cli/`；接入 `src/source/mod.rs` |
| AG03 | `crates/tokenscope-core/src/{settings,acceptance,report}.rs`、`crates/tokenscope-core/src/source/discovery.rs`、`scripts/prepare-native-acceptance.ps1`、`frontend/scripts/lib/native-single-instance.mjs`、`scripts/fixtures/settings_migrations.json`、`scripts/test_upgrade_fixture.py`、相关 Rust/PS/Node 测试与全仓测试密闭审计 |
| AG04 | `crates/tokenscope-core/src/settings.rs`、`docs/privacy.md`、`scripts/{upgrade_fixture.py,test_upgrade_fixture.py,test_upgrade_helpers.ps1,prepare-native-acceptance.ps1}`、`frontend/` 隐私 mock 与原生同意测试 |
| AG05 | `frontend/src/{types.ts,lib/viewFilters.ts,components/AgentIcon.vue,views/Dashboard.vue,views/Settings.vue,lib/locales/zh-CN.ts,lib/locales/en-US.ts}`、相关单测与 UI 契约、`DESIGN.md` |
| AG06 | `scripts/prepare-native-acceptance.ps1`、`frontend/scripts/contracts/native-identity.mjs`、`frontend/scripts/lib/native-unified.mjs`、`frontend/scripts/native-acceptance.mjs` 及纯函数契约测试 |
| AG07 | 隔离临时目录只读对账脚本（不入库，带固定截点指纹校验）与对账摘要回填本计划 |
| AG08 | `docs/stats-semantics.md`、`CLAUDE.md`、`docs/plans/README.md`、本计划交付记录、`scripts/build-inputs.json`、版本递增与基线检查 |

### AG01 应用枚举、历史库身份、同文件更正与未知模型计价拦截

- `AgentKind` 新增 `AntigravityCli`（serde 与 `as_str()` / `parse()` 为 `"antigravity-cli"`，纳入 `AgentKind::ALL`；显示名 `"Antigravity CLI"`）；`model.rs` 新增 `pub const UNKNOWN_MODEL: &str = "unknown";`。
- `UsageEvent::native_identity()` 与 `same_source_identity()` 为 `AntigravityCli` 返回 `Some(("antigravity-cli-step", format!("{session_id}|{record_id}")))`。
- `dedupe.rs`：将 `AntigravityCli` 纳入强身份组（按 `(agent, session_id, record_id)` 去重），并在 `dropped` 排序表补齐顺序 4。
- `history.rs`（`merge_event` / `classify_against`）与 `report.rs`（`event_write_of`）：
  - `event_write_of` 对 `AntigravityCli` 的 `UNKNOWN_PROJECT` 写 `project_key: None`（与 CCS `import/ccs.rs:940` 一致）；
  - 实现不变量 4 的**同源文件更正**（`AntigravityCli` 命中已存强身份、且已存事实在 `event_origins` 中全部 `native_file` 来源均来自与本次 `w` 相同的规范化源文件路径时，允许同刻更正 token/模型并更新 `usage_events.event_key`；来自不同文件路径的旧副本不降级主根终值）；
  - 实现不变量 4 的**按步骤强身份项目防降级与补齐**（命中同一 `antigravity-cli-step` 时，即使 `event_key` 因文件搬迁/行重排/token 改写而改变，`existing.project_key.is_some() && w.project_key.is_none()` 仍保留 `existing` 的三项项目字段；反之 `existing.project_key.is_none() && w.project_key.is_some()` 允许补齐）。
- `pricing.rs`、`model_display.rs`、`aggregate.rs`、`report.rs`：
  - 在汇总与明细共用的计价入口新增应用感知判断（`estimate_for_agent`）：对 `AntigravityCli` 且 `model.trim().eq_ignore_ascii_case(UNKNOWN_MODEL)` 无条件返回 `None`（不进入候选匹配）；
  - 展示名查询同样对 `AntigravityCli` 的 `UNKNOWN_MODEL` 直接返回未解析，保留 `"unknown"`。
- `import/ccs.rs`、`src-tauri/src/commands.rs`、`scripts/inspect_history.py` 与 `scripts/test_inspect_history.py`：补齐 `"antigravity-cli"` 映射/白名单，CCS 保持不支持，保留未知 app 拒绝断言。
- 必测回归：
  - **同文件更正 vs 冷库一致**：先采 step 7 = 100 token → 同文件在同 `created_at` 改写 step 7 = 80 → 增量采集后查库总量 == 80，并与冷库全扫逐项一致；
  - **跨根旧副本不降级**：主根 100 入库后，另一根旧副本 80 不覆盖 100；
  - **强身份项目防降级与补齐**：已知项目在文件搬迁/行号重排/token 改写且摘要库缺失时保持原项目；未知项目在摘要库补齐后（即使行号变化）成功更新为真实项目；
  - **恶意 `pricing.toml` 未知模型拦截**：显式配置 `id = "unknown"` 与 `id = "provider/unknown"` 的高单价，验证 Antigravity CLI 缺模型事件在汇总（`SummaryReport`）与明细（`EventList`）中 `cost_usd == None`、`unknown_pricing == true`、`unknown_tokens == 事件全量`、`label == None`，而同报告内已知模型正常计价。

### AG02 解析器 `source/antigravity_cli.rs` 与零写入工作区映射

- `crates/tokenscope-core/Cargo.toml`：`rusqlite` 的 features 增加内置 `"serialize"`（不引入新 crate，验证 `--locked` 构建通过且 `Cargo.lock` 无非预期变动）。
- **发现规则（`discover_with_ledger`）**：
  - 默认根 `~/.gemini/antigravity-cli`（验收模式走 `<root>/sources/antigravity-cli`）；
  - 登记 `acceptance::record_path("source_root_read", ...)`，经 `DirVisitLedger` 防环与多根认领；
  - 仅收集 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若 `<root>` 本身是 `brain/` 形态则收集 `<root>/<conv_id>/.system_generated/logs/transcript.jsonl`），忽略 `transcript_full.jsonl`、`chunks/`、空目录、`conversations/*.db`、`history.jsonl`、`scratch/`；软链/联接越界或失效跳过并诊断。
- **严格零写入 `conversation_summaries.db` 加载与缓存**：
  - 仅定位 `<root>/conversation_summaries.db`，绝不向上越界；
  - 无有效 `-wal`（缺失或 ≤ 32 字节）走 `file:...?mode=ro&immutable=1`；活跃 `-wal`（> 32 字节）走不变量 5 的纯内存双次指纹比对 + 官方 WAL 帧校验重放（只到最后提交帧 `db_size_after_commit > 0`）+ `deserialize_read_only` 加载至 `:memory:`；只查 `SELECT conversation_id, workspace_uris FROM conversation_summaries`；无法取得一致视图时返回 `Unusable`（不回退源目录普通 `mode=ro`，不缓存失败态，输出诊断）；
  - 进程内缓存键为**规范化 DB 路径**（Windows 小写），值携带 `(db_size, db_mtime, wal_size, wal_mtime)`；`revision` 仅对 `(state, 排序后的 conv_id -> normalized_project)` 计算摘要。
- **单文件解析**：
  - `session_id = <conv_id>`；逐行 serde 宽松解码，畸形 JSON 计 `bad_lines`；按 `step_index` 收敛到 `BTreeMap<u64, ParsedStep>`（后写覆盖前写，记录最终行号）；
  - 按 `step_index` 升序遍历，仅对 `USER_INPUT|USER_EXPLICIT` 在最后一个 `</USER_REQUEST>` 之后的 `<USER_SETTINGS_CHANGE>` 块内提取 `Model Selection`；对准入的 `PLANNER_RESPONSE|MODEL|DONE` 与带 token 的 `CHECKPOINT|SYSTEM|DONE` 执行 FRESH 桶映射、受检算术、`validate_buckets()`、UTC 时间校验、模型回退 `UNKNOWN_MODEL`、工作区回退 `UNKNOWN_PROJECT`。
- **合成 fixture 契约测试（`tests/antigravity_cli_source.rs`）**：
  - 发现：根不存在；空会话目录；忽略 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`conversations/*.db`、`history.jsonl`、`scratch/*.jsonl`；联接环/断链；直接指向 `brain/` 根。
  - **活跃 WAL 零写入专项（必测）**：构造 WAL 模式合成 `conversation_summaries.db`，将新会话的最新工作区仅提交到 `-wal`（不 checkpoint），并删除 `-shm`；调用加载后断言：① 读到了 WAL 中最新工作区路径；② 源目录**未生成 `-shm`**、文件清单与全部文件 mtime/字节数零变化。另测未提交尾帧被安全忽略、损坏 WAL 校验和返回 `Unusable` 且源目录零写入（老事件保原项目、新事件记未知）。
  - **双根缓存隔离与失效粒度专项（必测）**：两个不同手动根下放置大小与修改时间完全一致但内容不同的 `conversation_summaries.db`，验证各自命中正确项目不串用；修改已有会话的 `title`/`last_modified_time`（不改 `workspace_uris`）验证 `revision` 不变、不触发重解析；仅向摘要库新增工作区记录（不改 `transcript.jsonl`）验证 `revision` 变化、`collection_key` 变化且下一次查询（`refresh = false`）立即补齐项目路径。
  - **普通用户消息引用切换模板反例（必测）**：`<USER_REQUEST>` 内用户消息原文包含 `The user changed setting \`Model Selection\` from None to Fake Model (High).`，甚至包含伪造的 `<USER_SETTINGS_CHANGE>...</USER_SETTINGS_CHANGE>`，均**不得**改变 `session_model`；`PLANNER_RESPONSE` / `GENERIC` 正文同理不误匹配；只有 `</USER_REQUEST>` 之后真实系统块 `<USER_SETTINGS_CHANGE>` 才生效，且原文绝不出现在任何事件字段或诊断文本中。
  - 口径、步骤准入、畸形容错与跨日时间边界测试同前（FRESH、`CHECKPOINT`、`output == 0` 保留、全 0 跳、坏行计数、`file://` 百分号编码中文路径与 Claude 同 key、UTC 跨日）。

### AG03 来源发现、`collection_key`、设置格式 v4 与三端验收写前保护

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
  - `collection_key`（`Some(plans)` 与 `None` 双分支）纳入已启用 Antigravity CLI 根的 `norm_db_path@revision`（停用固定 `"disabled"`）；
  - `SummaryOptions` 增加 `antigravity_cli_dir` 与 `antigravity_cli_enabled`，审计全仓所有测试构造点补齐密闭注入。
- **三处验收根写前保护入口同步加固**：
  1. `crates/tokenscope-core/src/acceptance.rs`：`source_dir_override(AntigravityCli)` → `<root>/sources/antigravity-cli`；受保护清单追加 `home_canon.join(".gemini").join("antigravity-cli")`；**所有受保护应用目录（`.tokenscope`、`.claude`、`.codex`、`.workbuddy`、`.workbuddy-ai`、`.gemini/antigravity-cli`）均同时拒绝相等、祖先、任意子目录（`canon.starts_with(&protected)`）与联接别名**（用户主目录 `home_canon` 仍只拒相等与祖先）；
  2. `scripts/prepare-native-acceptance.ps1`：在任何 `New-Item` / 写盘之前，先通过最长存在祖先真实路径（解析符号链接/Windows 联接）规范化 `-Root` 与全部受保护目录（补充 `.workbuddy`、`.workbuddy-ai`、`.gemini\antigravity-cli`），按路径分量边界拦截相等、祖先、子目录（含尚不存在的新子目录如 `~/.gemini/antigravity-cli/new-sub`）与联接别名，拒绝后断言零新增文件/目录；
  3. `frontend/scripts/lib/native-single-instance.mjs`：`getUserProtectedDirs` 追加 `path.join(dir, ".gemini", "antigravity-cli")`，补充覆盖真实根、祖先 `~/.gemini`、新子目录 `~/.gemini/antigravity-cli/new-sub` 与联接别名的契约测试，断言拒绝后零新增文件/目录。

### AG04 隐私政策修订 4 与重新同意

- `crates/tokenscope-core/src/settings.rs`：`PRIVACY_POLICY_REVISION` 由 `3` 升至 `4`。
- `docs/privacy.md`：
  - 更新日期与适用范围（共同同意修订号：4）；
  - §2.1 支持工具列表新增 **Antigravity CLI**（默认根目录 `~/.gemini/antigravity-cli`）；
  - §2.1「数据处理豁免与红线条款」**明确、如实披露** Antigravity CLI 的全部读取边界：
    - 结构化用量元数据：只读 `brain/<id>/.system_generated/logs/transcript.jsonl` 的时间戳、模型标识、步骤编号、步骤类型/状态与 token 计数；
    - **`USER_INPUT.content` 窄范围内存检查披露**：为兼容尚未在响应行直接记录模型名的历史会话，本应用会在本地内存中检查 `USER_INPUT`（`source = "USER_EXPLICIT"`）记录的 `content` 字段中**位于 `</USER_REQUEST>` 标签之后**的系统注入块 `<USER_SETTINGS_CHANGE>`，仅匹配固定前缀 `The user changed setting \`Model Selection\` from ... to ...` 以提取并仅保留归一化后的模型名；不解析 `<USER_REQUEST>` 内的用户提问正文，不落库、不进诊断日志、不展示、不外传任何 `USER_INPUT.content` 原文；
    - 工作区索引：只读同根 `conversation_summaries.db` 的 `conversation_id` 与 `workspace_uris` 两列（以源目录零写入方式读取，不读取 `title`、`preview`、`conversations_fts` 或 `raw_summary`）；
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
  - 无模型子代理会话 `transcript.jsonl`（验证 `"unknown"` 保留且费用未知）；
  - 无工作区会话 `transcript.jsonl`（验证归入 `"(未知)"` 并可下钻）；
  - `conversation_summaries.db`（含与 Claude 同路径的 `file://` URI）。
- 扩展 `ui-identity`、`ui-source-paths` 与隐私重新同意（3 → 4）场景；核验 `isolation_runtime_guard` 零越界、零未授权写入。

### AG07 本机真实数据只读对账（固定输入截点）

- 全部输出目录（`history.db`、缓存、价格索引）指向隔离临时目录。
- **固定可比较输入截点**：
  1. 对账开始前记录 `~/.gemini/antigravity-cli` 下全部 `transcript.jsonl` 与 `conversation_summaries.db`（含 `-wal`/`-shm` 存在状态）的 `(path, size, mtime_ns)` 清单 `S0`；
  2. 立即串行运行 Rust `AntigravityCliSource` 全量解析与独立 Python 按 §3 口径的只读脚本；
  3. 结束后再次采集清单 `S1` 并与 `S0` 逐项比对：若 `S1 != S0`（采集中存在活跃写入），丢弃本次结果并重取一批，直到 `S0 == S1` 的静态截点成立；
  4. 在静态截点上逐项核对总请求数（`PLANNER_RESPONSE` + 带 token `CHECKPOINT` + 全部子代理）、四桶精确合计、已知/未知模型分布、已知/未知项目分布实现 **0 偏差**，坏行数为 0 或逐行说明原因；同时验证源目录无新增 `-shm`、无 mtime 变化，真实 `~/.tokenscope` 零变化；
  5. 仅将脱敏汇总计数与截点时刻回填本计划，严禁留存任何会话正文或敏感路径。

### AG08 文档、构建清单与交付

- `docs/stats-semantics.md`：新增 §2.4「Antigravity CLI（`~/.gemini/antigravity-cli`）」与 §3 去重/同文件更正说明。
- `CLAUDE.md`：更新「项目目标」为五应用统计，更新「候选 agent」与 Antigravity CLI 口径要点。
- `scripts/build-inputs.json`：运行 `python -B scripts/build_inputs.py --update` 并 `--check`。
- 版本与基线（仅在用户确认并完成实现后执行）：
  - `python -B scripts/bump_version.py --bump minor --summary "新增 Antigravity CLI 用量来源支持"`（`0.9.10 → 0.10.0`），核验 `v0.9.10` 已在 `upgrade_baselines.json` 登记；
  - 跑通全量本地门禁（`TOKENSCOPE_PRECOMMIT_MODE=full`、Rust 普通/acceptance 双模式 clippy+test、前端 typecheck/format/test/build/E2E、PowerShell/Python 契约与原生验收场景），将实测证据回填本计划与 `docs/plans/README.md`。

---

## 5. 风险与既有确认记录

1. **升级起点矩阵 `MAX_ORIGINS = 6`**：本版 `0.10.0` 选点 6 个（`0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.10`），不超上限，本轮维持 `MAX_ORIGINS = 6`；本版发布登记 `settings_format = 4` 后，下一版（`0.10.1` / `0.11.0`）将触发第 7 个起点守卫并按规范另行评审。
2. **图标方案（已确认）**：复用 `frontend/src/assets/icons/googlegemini.svg`，可访问名称保留 `Antigravity CLI`。
3. **预览模型与 `unknown` 模型的费用显示**：2026-11-18 前旧子代理的 `"unknown"` 模型（由共享计价路径强制返回未定价）及未入公开价格目录的预览模型，在未配置外置价格时均如实显示费用「未知」（橙色），不按 `$0` 估算。
4. **单 PR 交付节奏**：AG01–AG08 在 `feat/antigravity-cli-adapter` 分支内按任务逐提交推进，作为一个完整产品 PR 交付。
