# 新增 Antigravity CLI（`antigravity-cli`）用量来源实施计划

> 状态：**待用户确认（方案文档，尚不分配发行版本）**。
>
> 用户 2026-11-28 已拍板口径：
> 1. **数据源与历史模型补齐策略（方案 A）**：主流程只读 `brain/<id>/.system_generated/logs/transcript.jsonl`，并严格只读同根目录 `conversation_summaries.db` 补工作区路径 `workspace_uris`；不打开、不解析内部 `conversations/*.db` Protobuf；
> 2. **`CHECKPOINT` 压缩摘要调用**：与 `PLANNER_RESPONSE` 一并计入用量；
> 3. **名称与标识**：展示名 **Antigravity CLI**，内部枚举 `AgentKind::AntigravityCli`（序列化与库内标识 `antigravity-cli`），默认根固定 `~/.gemini/antigravity-cli`；暂不接入桌面版 `~/.gemini/antigravity`、`antigravity-ide`、`antigravity-acp`；
> 4. **首要目标与未知口径**：保证获取到的数据是真实的；缺少工作区路径（pwd）或模型名时如实标为未知，**未知的处理参照项目导入 CCS 的未知处理**（不丢弃有效用量、不伪造项目路径或模型名、不静默按 `$0` 计价）。
>
> 本计划按用户明确排期解除 `CLAUDE.md`「候选 agent 暂缓排期」中对 Antigravity CLI 的暂缓。

**Goal：** TokenScope 只读采集本机 Antigravity CLI（`~/.gemini/antigravity-cli`）的本地会话与子代理转写用量，作为第五个独立应用纳入统一历史库、汇总、筛选、下钻、计价与来源管理；现有 Claude Code / Codex / WorkBuddy / WorkBuddy AI 的统计结果零变化。

**Architecture：**
- 新增独立适配器 `crates/tokenscope-core/src/source/antigravity_cli.rs`：只发现 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若用户手动指向 `brain/` 目录，也允许直接匹配其下 `<conv_id>/.system_generated/logs/transcript.jsonl`），天然排除 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`conversations/*.db` 与其它非目标文件。
- 工作区路径（`cwd`）通过同根 `<root>/conversation_summaries.db` 以只读模式仅查询 `(conversation_id, workspace_uris)` 两列，经既有 `project_path::normalize_project_path`（含 `file://` 一次百分号解码）与其它工具归入同一项目身份，并接入 `Source::context_revision` 驱动增量刷新；查不到或数据库缺失时按 CCS 口径保持项目未知。
- 隐私政策修订为共同同意修订 4（老用户升级后重新明确同意再读取新增目录），设置格式修订为 v4（新增 `sources.antigravity_cli`），前端来源筛选与设置分区同步扩展至五个应用。

**Tech Stack：** Rust（`tokenscope` 核心库、`tokenscope-app` 桌面壳）、Vue 3 + Naive UI、SQLite（`rusqlite` 只读与统一历史库）、PowerShell / Python / Node 原生验收脚本。

---

## 1. 背景与本机只读核验事实（2026-11-28）

研究参考：[cc-switch v4.0.18 的 agy 接入拆解](../../research/2026-10-22-cc-switch-agy-usage.md)、[Magpie 用量统计拆解](../../research/2026-10-28-magpie-usage-statistics.md) §15.8。2026-11-28 对本机 `~/.gemini/antigravity-cli` 执行严格只读核查（仅统计结构、字段名、类型与数值，不读取/留存会话正文与敏感内容），证实以下一手事实：

| 维度 | 实测结论（历史样本观测，不作验收固定预期） |
| --- | --- |
| 会话结构 | 共 66 个会话 = 24 个主会话（16 非空 + 8 空，`steps = 0`）+ 42 个子代理会话（`self` 40、`research` 2，`nesting_depth = 1`）。子代理落在独立 `brain/<sub_id>`，用量不重复计入父会话，占样本调用约 18%（2,876 / 15,701）、总输出约 28% |
| `transcript.jsonl` vs `transcript_full.jsonl` | 58 个非空会话均有两者；全量 32,527 行比对，二者在 `step_index`、`type`、`source`、`status`、`created_at`、`model` 与全部 token 字段上 **0 差异**；精简版仅截断超长正文/工具输出，体积小约 26%（44.3 MB vs 60.0 MB）。`chunks/transcript/*.jsonl` 与 `chunks/transcript_full/*.jsonl` 是同字节分片副本，不得扫入 |
| 与内部 `.db` 交叉核验 | 与 66 个 `conversations/<id>.db` 的 `steps.metadata`（Protobuf `ModelUsage`）逐条比对，15,695 / 15,695 条的 `input_tokens`、`cache_read_tokens`、`output_tokens` **100% 精确相等（0 差异）**；活跃会话的 `.db-wal` 存在未 checkpoint 滞后（实测 13.6 MB），而 `transcript.jsonl` 已实时落盘到最新步骤 |
| Token 桶语义 | 仅出现 `input_tokens`、`cache_read_tokens`、`output_tokens`（全库 `cache_write_tokens` 出现 0 次）。连续轮次在 Claude 与 Gemini 模型上均满足 `上轮(input + cache_read) ≈ 下轮 cache_read`（且常见 `input_tokens ≪ cache_read_tokens`），证明 `input_tokens` 为 **FRESH 语义（已排除缓存读）**；`output_tokens` 全库满足 `thinking + non_thinking == output`（0 违例），**已包含思考 token** |
| 携带 token 的步骤类型 | 共两类已完成步骤：① `PLANNER_RESPONSE`（`source=MODEL`, `status=DONE`），15,678 条，100% 含 token（含 1 条 `output_tokens = 0` 但输入/缓存非 0 的合法记录）；② `CHECKPOINT`（`source=SYSTEM`, `status=DONE`），2026-11-09 起 18 条全部携带 token（上下文压缩摘要的真实模型调用，单次约 250K cache_read、3K–4K output；早期 38 条无 token 字段）。无 `status != DONE` 携带 token 的行 |
| 模型字段演进 | ① `2026-11-18` 起 `PLANNER_RESPONSE`（含子代理）直接带 `model`（2,092 条，如 `claude-opus-4-6-thinking`、`gemini-3.8-flash-high`、`gemini-4-neon-xhigh`）；② `2026-10-13 ~ 2026-11-10` 主会话无行内 `model`，但在 `USER_INPUT`（`source=USER_EXPLICIT`）带 `The user changed setting \`Model Selection\` from <old> to <new>.`（10,717 条）；③ 同期 40 个子代理无行内 `model` 且无切换文案（2,872 条）；④ 18 条 `CHECKPOINT` 均无行内 `model`，但经对照 18/18 与同会话前一条 `PLANNER_RESPONSE` 同模型 |
| 工作区路径 | `transcript.jsonl` 本身完全不含 cwd/workspace；`conversation_summaries.db` 的 `workspace_uris`（JSON 数组 `["file:///..."]`）覆盖 58/58 非空会话（含全部 42 个子代理），与 `conversations/*.db` 内工作区记录 0 差异 |
| 行序与重复 `step_index` | 单文件内偶发重复 `step_index`（实测 2 次，均为 `GENERIC|RUNNING`）；`created_at` 为 UTC RFC3339 秒级，15,700/15,700 精确等于步骤开始时刻 |

### 1.1 当前代码基线复核（`main` @ `3b9b444`，应用版本 `0.9.10`）

| 项目 | 当前现状与实施影响 |
| --- | --- |
| 应用枚举泛化 | WB01 已完成：`AgentKind::ALL`、`AgentSources::config(AgentKind)`、`acceptance::source_dir_override(AgentKind)` 均已按枚举分发，无需前置泛化阶段 |
| 历史库与 CCS 未知口径 | `usage_events.app` 为 TEXT 无需改表结构；CCS 导入缺 cwd 时写 `project_key = NULL`、`session_initial_cwd = None`、`event_cwd = None`，聚合/查询/下钻统一按 `UNKNOWN_PROJECT`（`"(未知)"`）归组；未知模型存 `model_raw = "unknown"`（嵌入展示名目录与定价目录均不含 `"unknown"`，计价恒返回未知、不按 `$0`、界面保留 `"unknown"`） |
| 持久格式与同意版本 | `SETTINGS_FORMAT_REVISION = 3`、`PRIVACY_POLICY_REVISION = 3`、`HISTORY_SCHEMA_VERSION = 5`、`PARSER_REVISION = 3`、`PARSE_RULES_REVISION = 7`、`INDEX_VERSION = 11` |
| 协议变更门禁 | `scripts/bump_version.py` 强制要求：修改 `docs/privacy.md` 或 `docs/terms.md` 时必须递增 `PRIVACY_POLICY_REVISION`（3 → 4） |
| 升级基线选点 | `python -B scripts/upgrade_baselines.py select --current-version 0.10.0` 当前选中 6 个起点（`0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.10`），恰好等于 `MAX_ORIGINS = 6`；本版（`0.10.0`）不超限。详见 §5 前瞻提醒 |
| 图标资源 | `frontend/src/assets/icons/googlegemini.svg` 已打包并引入 `AgentIcon.vue`（尚未被任一应用选项使用） |

---

## 2. 范围与不做

**做：**
- AG01：应用枚举、历史库身份、去重规则、壳层映射与历史核验工具白名单。
- AG02：`source/antigravity_cli.rs` 适配器（精准文件发现、只读 `conversation_summaries.db` 映射、transcript 解析、模型提取与未知兜底、受检算术）与合成 fixture 契约测试。
- AG03：来源发现、设置格式 v4（`sources.antigravity_cli` 与迁移规则）、验收根隔离与全仓测试来源密闭。
- AG04：隐私政策修订 4（`docs/privacy.md` 与 `PRIVACY_POLICY_REVISION = 4`）及升级/同意契约同步。
- AG05：前端五应用适配（类型、筛选、来源图标、设置页第五来源分区与说明、中英文文案）与 `DESIGN.md` 同步。
- AG06：原生验收合成数据与 `ui-identity` / `ui-source-paths` / 隐私重新同意场景覆盖。
- AG07：本机真实数据只读对账（独立脚本双边核验，写入隔离目录）。
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
2. **数据真实优先与「未知」处理（参照 CCS 口径）**：
   - **项目未知（缺 pwd / workspace）**：当 `conversation_summaries.db` 缺失、无该会话记录、`workspace_uris` 为空/非合法 JSON/全部 URI 无法经 `normalize_project_path` 规范化、或只读降级时，`UsageEvent.project = UNKNOWN_PROJECT`（`"(未知)"`），`session_initial_cwd = None`，`event_cwd = None`；写入 `history.db` 时 `project_key = NULL`（与 `import/ccs.rs` 完全一致）。聚合按项目分组与下钻均归入 `"(未知)"` 组，绝不以会话 ID 或目录名伪造项目。
   - **模型未知（缺 model）**：行内无 `model`、同会话也无前置有效模型或 `Model Selection` 切换记录时，**绝不丢弃有效用量事件**，`model` 记为常量 `UNKNOWN_MODEL = "unknown"`（与 CCS 未知模型写法一致），`skipped_no_model` 不吞有效用量。计价层对 `"unknown"` 必无候选命中 → `cost_usd = None`、`unknown_pricing = true`、`unknown_tokens` 如实累计，界面以警告色披露「未知」且绝不静默按 `$0`；展示名查询无身份证据，界面原样显示 `"unknown"`，绝不从 `settings.json` 反推历史模型。
3. **桶互斥（FRESH 语义）与受检算术（SF08）**：
   - `input_tokens = raw.input_tokens`（**不减** `cache_read_tokens`）；
   - `cache_read_tokens = raw.cache_read_tokens`（缺失按 0）；
   - `cache_write_tokens = raw.cache_write_tokens`（缺失按 0，`cache_write_split_known = false`，`cache_write_5m = 0`，`cache_write_1h = 0`）；
   - `output_tokens = raw.output_tokens`（已包含思考 token，不另行叠加）。
   - 任何已出现的 token 字段为负数、浮点、布尔、字符串或超出 `u64`，或 `UsageEvent::validate_buckets()` 检查总和/prompt 溢出失败 → 计 `bad_lines` 跳过，不饱和、不回绕；四桶全为 0 → 计 `skipped_zero_usage` 跳过（允许 `output_tokens == 0` 但输入/缓存读非 0 的合法完成行）。
4. **事件准入与精准去重**：
   - 仅接纳两类已完成步骤：① `type == "PLANNER_RESPONSE" && source == "MODEL" && status == "DONE"`；② 至少携带一个 token 字段的 `type == "CHECKPOINT" && source == "SYSTEM" && status == "DONE"`（早期无 token 字段的 `CHECKPOINT` 静默跳过，不计坏行）。未完成步骤（`status != "DONE"`）不产出事件、不占去重身份。
   - 文件内按 `step_index` 收敛（后写覆盖前写，应对重写与重复行）；缺失/非非负整数 `step_index` 的合资格用量行计 `bad_lines` 跳过。
   - 强身份方案 **`antigravity-cli-step`** = `"{session_id}|{step_index}"`（`record_id = step_index.to_string()`），在 `dedupe.rs` 与 `history.db` 中受 `app = "antigravity-cli"` 隔离，不与任何其他应用合并。
5. **最小读取面与零写入保证**：
   - 仅收集 `<root>/brain/<conv_id>/.system_generated/logs/transcript.jsonl`（若 `<root>` 本身是 `brain/` 形态则收集 `<root>/<conv_id>/.system_generated/logs/transcript.jsonl`），通过 `DirVisitLedger` 防环与多根认领，明确跳过 `transcript_full.jsonl` 与 `chunks/`；不遍历根外路径，软链/联接越界或失效跳过并诊断。
   - 只读打开 `<root>/conversation_summaries.db`：当同目录 `-wal` 不存在或大小 ≤ 32 字节（空 WAL 头）时，强制使用 `SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_URI` + `file:...?mode=ro&immutable=1`，保证 **零 `-shm`/`-wal` 创建、零目录/时间戳改写**；当 `-wal` > 32 字节（活跃写入中）时使用 `SQLITE_OPEN_READ_ONLY` + `busy_timeout(2000ms)` + 只读事务，权限受限时回落 `immutable=1`。读取失败（锁/损坏）不进进程内缓存，不阻断用量采集，输出可读诊断。
   - 已保存事件的已知项目归属保护：若某条 `antigravity-cli` 事实在 `history.db` 已有非空 `project_key`，后续仅因 `conversation_summaries.db` 缺失或暂时不可读导致同 `event_key` 重解析出 `project_key = None` 时，保留库内已确认的 `project_key` 不做降级覆盖；反之，从 `None` 补齐为有效规范化路径时正常更新。
6. **`Model Selection` 严格提取防误伤**：
   - 仅解析 `type == "USER_INPUT" && source == "USER_EXPLICIT"` 的 `content`，严格匹配 `The user changed setting \`Model Selection\` from <old> to <new>.` 模板（句号后接空白/换行/行尾或反引号截止，保留版本号内小数点）；`<new>` 为空或 `None` 不生效；显示名归一与 `agy` 原生 slug 一致（如 `Gemini 3.8 Flash (High)` → `gemini-3.8-flash-high`，`Claude Opus 4.6 (Thinking)` → `claude-opus-4.6-thinking`；已是无空格无括号的 ID 原样保留）。绝不扫描 `PLANNER_RESPONSE` / `GENERIC` 正文。
   - 模型解析优先级：行内显式 `model`（同时推进同会话 `session_model`）> 同会话前序 `session_model`（含 `Model Selection` 与前一条 `PLANNER_RESPONSE` 的模型，覆盖 `CHECKPOINT`）> `"unknown"`。
7. **项目身份与跨工具统一**：`workspace_uris` 中首个通过 `project_path::normalize_project_path` 的 URI 作为该会话的项目身份，与 Claude Code / Codex / WorkBuddy 的同一目录归一到同一项目 key。
8. **时间口径**：`created_at` 按 UTC RFC3339 原样存储（步骤开始时刻），缺失或格式非法计 `bad_lines` 跳过；展示沿用既有单次时区转换链路。
9. **同意先于读取**：`PRIVACY_POLICY_REVISION` 升至 4，旧同意（修订 1/2/3）升级后进入 `StaleRevision`，零业务读取；重新同意原子落盘后才读取五个应用。
10. **重复扫描幂等与历史持久**：重扫、文件追加、重启、停用或删除源目录均不重复计数且不删除 `history.db` 已保存用量。

---

## 4. 分步任务清单

新增行为一律先写失败测试（解析类带手工合成最小 fixture，严禁复制真实日志），确认红灯后再实现；每个提交只解决一类问题；所有构造 `SummaryOptions` / `DiscoveryContext` 的测试必须注入隔离临时目录，严禁触碰真实 `~/.tokenscope` 与真实 `~/.gemini/antigravity-cli`；Rust 测试统一经 `python -B scripts/run_rust_tests.py -- cargo test ...` 执行。

| 任务 | 主要文件 |
| --- | --- |
| AG01 | `crates/tokenscope-core/src/{model,dedupe,history,report}.rs`、`crates/tokenscope-core/src/import/ccs.rs`、`src-tauri/src/commands.rs`、`scripts/inspect_history.py`、`scripts/test_inspect_history.py` |
| AG02 | 新增 `crates/tokenscope-core/src/source/antigravity_cli.rs`、`crates/tokenscope-core/tests/antigravity_cli_source.rs`、`crates/tokenscope-core/tests/fixtures/antigravity_cli/`；接入 `src/source/mod.rs`、`src/history.rs`（同键已知项目防降级保护） |
| AG03 | `crates/tokenscope-core/src/{settings,acceptance,report}.rs`、`crates/tokenscope-core/src/source/discovery.rs`、`crates/tokenscope-core/tests/{source_paths_settings,source_paths_collection,acceptance_closure}.rs`、`scripts/fixtures/settings_migrations.json`、全仓测试密闭审计 |
| AG04 | `crates/tokenscope-core/src/settings.rs`、`docs/privacy.md`、`scripts/{upgrade_fixture.py,test_upgrade_fixture.py,test_upgrade_helpers.ps1,prepare-native-acceptance.ps1}`、`frontend/` 隐私 mock 与原生同意测试 |
| AG05 | `frontend/src/{types.ts,lib/viewFilters.ts,components/AgentIcon.vue,views/Dashboard.vue,views/Settings.vue,lib/locales/zh-CN.ts,lib/locales/en-US.ts}`、相关单测与 UI 契约、`DESIGN.md` |
| AG06 | `scripts/prepare-native-acceptance.ps1`、`frontend/scripts/contracts/native-identity.mjs`、`frontend/scripts/lib/native-unified.mjs`、`frontend/scripts/native-acceptance.mjs` 及纯函数契约测试 |
| AG07 | 隔离临时目录只读对账脚本（不入库）与对账摘要回填本计划 |
| AG08 | `docs/stats-semantics.md`、`CLAUDE.md`、`docs/plans/README.md`、本计划交付记录、`scripts/build-inputs.json`、版本递增与基线检查 |

### AG01 应用枚举、历史库身份与去重

- `AgentKind` 新增 `AntigravityCli`（serde 与 `as_str()` / `parse()` 为 `"antigravity-cli"`，纳入 `AgentKind::ALL`；显示名 `"Antigravity CLI"`）。
- `model.rs` 新增常量 `pub const UNKNOWN_MODEL: &str = "unknown";`（与 `UNKNOWN_PROJECT` 并列，供解析层与测试共用）。
- `UsageEvent::native_identity()` 与 `same_source_identity()` 为 `AgentKind::AntigravityCli` 返回 `Some(("antigravity-cli-step", format!("{session_id}|{record_id}")))`（要求二者非空）。
- `dedupe.rs`：将 `AgentKind::AntigravityCli` 纳入强身份组（按 `(agent, session_id, record_id)` 去重，保留确定性终值，不与其他应用合并），并在 `dropped` 排序表补齐顺序 4。
- `report.rs`：`event_write_of` 对 `AntigravityCli` 的 `UNKNOWN_PROJECT` 写 `project_key: None`（与 CCS `import/ccs.rs:940` 保持 SQL `NULL` 一致；查询读取侧 `usage_event_of` 与聚合侧 `aggregate.rs` 均自动映射回 `UNKNOWN_PROJECT`）。
- `import/ccs.rs`：维持 CCS 仅支持 Claude Code，`AntigravityCli` 映射为不支持、无跨来源别名。
- `commands.rs`：补齐 `"antigravity-cli"` 解析、写入与状态查询分支。
- `scripts/inspect_history.py` 与 `scripts/test_inspect_history.py`：白名单追加 `"antigravity-cli"`，保留未知 app 拒绝断言。
- 测试：新 app 写入/读出往返；`project_key: None` 与 `model = "unknown"` 聚合/下钻/计价闭环；与现有四应用同 `session_id|record_id` 互不合并；旧版本历史库升级可读。

### AG02 共享解析器 `source/antigravity_cli.rs` 与只读工作区映射

- **默认根与自定义根**：
  - 默认根：`~/.gemini/antigravity-cli`（验收模式走 `acceptance::source_dir_override(AgentKind::AntigravityCli)` → `<root>/sources/antigravity-cli`）。
  - 发现规则（`discover_with_ledger`）：
    - 登记 `acceptance::record_path("source_root_read", ...)` 并通过 `DirVisitLedger` 进入根与 `brain/` 目录；
    - 枚举会话目录 `<conv_id>`（校验合法目录名，符号链接/Windows 联接解析目标并经台账防环，目标逃逸或断链跳过并诊断）；
    - 仅收集 `<conv_dir>/.system_generated/logs/transcript.jsonl`；严格忽略 `transcript_full.jsonl`、`chunks/`、空会话目录、非目标 `.jsonl`、`conversations/*.db`；
    - 手动根若直接指向 `brain/`（不含 `brain/` 子目录但直接含 `<conv_id>/.system_generated/logs/transcript.jsonl`），同样只收集该固定相对路径，不向上越界。
- **只读 `conversation_summaries.db` 映射（参考 `claude_projects.rs` 结构）**：
  - 仅定位 `<root>/conversation_summaries.db`（绝不向父目录越界）；
  - 按不变量 5 打开（无有效 `-wal` 强制 `immutable=1`，活跃 `-wal` 走 `SQLITE_OPEN_READ_ONLY` + `busy_timeout`，权限受限降级 `immutable=1`），仅查 `SELECT conversation_id, workspace_uris FROM conversation_summaries`；
  - 解析 JSON 数组，取首个经 `project_path::normalize_project_path` 成功的路径；进程内按 `(db_size, db_mtime, wal_size, wal_mtime)` 缓存，失败态 `Unusable` 不进缓存且通过诊断提示；
  - 生成确定性 `context_revision`（哈希状态与排序后的 `conv_id -> normalized_path` 表），使工作区补齐后受影响文件自动重解析。
- **单文件解析**：
  - 从路径提取 `session_id = <conv_id>`；
  - 逐行 serde 宽松解码，畸形 JSON 计 `bad_lines`；按 `step_index` 存入 `BTreeMap<u64, ParsedStep>`（后写覆盖前写，记录最终行号）；
  - 按 `step_index` 升序遍历：维护 `session_model`，应用 `USER_INPUT|USER_EXPLICIT` 的 `Model Selection` 严格提取与显示名归一；对准入的 `PLANNER_RESPONSE|MODEL|DONE` 与带 token 的 `CHECKPOINT|SYSTEM|DONE` 做受检算术、`validate_buckets()`、时间戳校验、模型回退 `"unknown"`、工作区查表回退 `UNKNOWN_PROJECT`。
- **历史库已知项目防降级保护**：
  - 核查 `history.rs` 同 `event_key` 更新分支：对于 `AntigravityCli`（且仅在 `existing.project_key.is_some()` 且 `w.project_key.is_none()` 时），避免因 `conversation_summaries.db` 临时不可读或移除而把已确认项目路径覆盖回未知；补定向单测验证「先未知后补齐 → 更新成功」「先已知后 DB 缺失/不可读 → 保留已知项目」。
- **合成 fixture 测试（`tests/antigravity_cli_source.rs`）**：
  - 发现：根不存在；空会话目录；忽略 `transcript_full.jsonl`、`chunks/**/*.jsonl`、`conversations/*.db`、`history.jsonl`、`scratch/*.jsonl`；联接环/失效链接安全终止；直接指向 `brain/` 根。
  - 口径：FRESH 语义（`input < cache_read` 绝不扣减）；`output` 含 thinking 不重复叠加；可选 `cache_write_tokens` 兼容与缺省 0；`output_tokens == 0` 且输入非 0 的已完成调用保留；全 0 计 `skipped_zero_usage`。
  - 步骤准入：`PLANNER_RESPONSE|MODEL|DONE` 纳入；`PLANNER_RESPONSE|RUNNING/ERROR` 忽略；带 token 的 `CHECKPOINT|SYSTEM|DONE` 纳入且继承前序模型；无 token 的早期 `CHECKPOINT` 静默跳过不计坏行；同 `step_index` 后写覆盖前写。
  - 模型与未知：行内 `model` 优先；`USER_INPUT|USER_EXPLICIT` `Model Selection` 正确提取 `Gemini 3.8 Flash (High)`、`Gemini 4 Neon (XHigh)`、`Claude Opus 4.6 (Thinking)` 及中途切换；`PLANNER_RESPONSE`/`GENERIC` 正文出现 `Model Selection` 文本**不误匹配**；`from X to None` 不覆盖；旧子代理无模型与无切换文案 → 保留事件、`model == "unknown"`、`cost_usd == None`、`unknown_pricing == true`、`display_name == "unknown"`。
  - 工作区与未知：`conversation_summaries.db` 中 `file:///C:/...` 含百分号编码中文路径与 Claude 大写盘符/反斜杠归一到同一项目 key；DB 缺失 / 会话缺行 / 空数组 / 非法 URI → `project == "(未知)"`、`project_key == NULL`、下钻 `"(未知)"` 命中且与 CCS 未知项目同组；只读打开无 `-wal` 库前后目录文件清单与 mtime 逐字节不变。
  - 畸形容错：坏 JSON、负数/浮点/字符串/布尔 token、`u64` 溢出、非法 `created_at`、缺失 `step_index` 均计 `bad_lines` 跳过不 panic；累计溢出走既有 `BigUint` / 查询校验。
  - 时间边界：UTC 秒级 `created_at` 落在跨日界附近按 `Asia/Shanghai` 归日正确。

### AG03 来源发现、设置格式 v4 与验收隔离

- `settings.rs`：
  - `AgentSources` 新增 `pub antigravity_cli: Option<SourceConfig>`（缺省 = 启用 + 自动发现）；更新 `config` / `config_mut` 与 `SETTINGS_TEMPLATE` 注释；
  - `SETTINGS_FORMAT_REVISION` 由 `3` 升至 `4`；
  - `scripts/fixtures/settings_migrations.json` 新增 `1->4`、`2->4`、`3->4`、`4->4`（对 `sources.antigravity_cli.enabled` 声明 `default: true, allow_missing: true`）。
- `source/discovery.rs` 与 `report.rs`：
  - 固定候选集追加 `AgentKind::AntigravityCli` 默认根 `~/.gemini/antigravity-cli`；
  - 跨应用根冲突（例如把 `~/.gemini/antigravity-cli` 追加给 Claude 或 Codex）与同应用嵌套认领复用既有 `resolve_sources` 规则；
  - `SummaryOptions` 增加 `antigravity_cli_dir` 与 `antigravity_cli_enabled`，并审计全仓所有构造 `SummaryOptions` / `DiscoveryContext` 的测试，为新增来源显式密闭注入临时缺失根或禁用，确保绝不读到真实 `~/.gemini/antigravity-cli`。
- `acceptance.rs`：
  - `source_dir_override(AgentKind::AntigravityCli)` 返回 `<root>/sources/antigravity-cli`；
  - 真实保护目录列表追加 `home_canon.join(".gemini").join("antigravity-cli")`（覆盖相等、父目录 `~/.gemini`、规范化别名与联接）。
- 测试：发现层三件套（存在/缺失/冲突）；设置读写往返与 `1/2/3->4` 迁移契约；`source_paths_*` 与 `acceptance_closure` 覆盖第五应用；`persistent_format` 常量与 JSON 契约同步。

### AG04 隐私政策修订 4 与重新同意

- `crates/tokenscope-core/src/settings.rs`：`PRIVACY_POLICY_REVISION` 由 `3` 升至 `4`。
- `docs/privacy.md`：
  - 更新日期与适用范围（共同同意修订号：4）；
  - §2.1 支持工具列表新增 **Antigravity CLI**（默认根目录 `~/.gemini/antigravity-cli`）；
  - 明确列举读取边界（仅 `brain/<id>/.system_generated/logs/transcript.jsonl` 的结构化用量元数据，以及 `conversation_summaries.db` 的 `conversation_id` / `workspace_uris` 两列）与明确排除项（`conversations/*.db`、`transcript_full.jsonl`、`chunks/`、`history.jsonl`、`settings.json`、`annotations/`、`implicit/`、`log/`、`cache/`、`steps/*/output.txt`、`~/.gemini/config` 及其他 Antigravity 端）；
  - §9.2 修订历史新增「共同同意修订 4」。
- 同步测试与升级夹具：
  - `scripts/prepare-native-acceptance.ps1`、前端 `privacySnapshot` mock 与 `frontend/scripts/` 同意契约同步修订 4；
  - `scripts/upgrade_fixture.py` / `scripts/test_upgrade_fixture.py` / `scripts/test_upgrade_helpers.ps1` 核验升级链路：历史基线（修订 1/2/3）升级到本版后先验证旧同意被拒、零业务读取，再授予修订 4 同意并进入业务；
  - 本地跑通全部 `scripts/test_*.py` 与 `test_upgrade_helpers.ps1` 契约。

### AG05 前端五应用适配与设计规范同步

- 类型与筛选：
  - `frontend/src/types.ts`：`AgentFilter`、`SourcePathRow.agent`、`AGENT_LABEL` 增加 `"antigravity-cli": "Antigravity CLI"`；`sourceIdOf` 原样透传 `"antigravity-cli"`；
  - `frontend/src/lib/viewFilters.ts`：`AGENTS` 增加 `"antigravity-cli"`（旧持久化筛选偏好完全兼容）。
- 图标与汇总页来源控件：
  - `frontend/src/components/AgentIcon.vue`：映射 `"antigravity-cli"` 使用已随包打包的 `googlegemini.svg`（如用户后续提供专用 Antigravity SVG logo 可直接替换）；
  - `frontend/src/views/Dashboard.vue`：`agentOptions` 扩为 6 项纯图标选项（全部 + 5 应用），保留可访问名称 `"Antigravity CLI"` 与悬停提示；验证 980×620 与 1280×820 视口不换行不溢出。
- 设置页与双语文案：
  - `frontend/src/views/Settings.vue`：`AGENT_IDS` 扩为 5 项，Antigravity CLI 分区附一行固定说明（双语同步 `zh-CN.ts` / `en-US.ts`）：说明仅统计 Antigravity CLI 本地会话（不含桌面版/IDE），缺少工作区或模型记录的历史会话将如实显示为未知；
  - CCS 导入说明文案同步补充 Antigravity CLI 仍以 TokenScope 自身采集与原生历史为准。
- `DESIGN.md`：同步汇总页来源控件 6 选项与设置页 5 来源分区约束。
- 测试：更新/新增 `Dashboard.test.ts`、`Settings.test.ts`、`viewFilters.test.ts`、`locales.test.ts` 与浏览器契约（明暗双主题、980×620 与 1280×820）。

### AG06 原生验收接线与场景覆盖

- `scripts/prepare-native-acceptance.ps1`：在 `<root>/sources/antigravity-cli` 下生成合成数据：
  - 主会话 `transcript.jsonl`（含 `USER_INPUT` `Model Selection`、显式 `model` 的 `PLANNER_RESPONSE`、带 token 的 `CHECKPOINT`、一条未完成行、一条坏行）；
  - 无模型子代理会话 `transcript.jsonl`（验证 `"unknown"` 模型事件保留且费用为未知）；
  - 无工作区会话 `transcript.jsonl`（不在 `conversation_summaries.db` 中，验证归入 `"(未知)"` 项目并可下钻）；
  - `conversation_summaries.db`（含与 Claude 合成日志同路径的 `file://` URI，验证跨 5 应用合并为同一项目）。
- 扩展 `ui-identity` 与 `ui-source-paths` 场景：验证 Antigravity CLI 独立筛选、跨应用同路径项目合并、`CHECKPOINT` 计入、未知模型保留与未知费用披露、未知项目归组、停用来源后历史保留；验证旧修订（3）→ 修订（4）重新同意闸门。
- 验证 `isolation_runtime_guard` 零越界、零未授权写入、`evidence.json` 无违例。

### AG07 本机真实数据只读对账（本地隔离运行）

- 全部输出目录（`history.db`、缓存、价格索引）指向临时隔离目录，只读运行 Rust `AntigravityCliSource` 采集真实 `~/.gemini/antigravity-cli`。
- 独立 Python 只读脚本按 §3 口径重算同一批 `transcript.jsonl` 与 `conversation_summaries.db`：
  - 逐项比对总请求数（含 `PLANNER_RESPONSE` + 带 token `CHECKPOINT` + 全部子代理）、四桶精确合计、已知/未知模型分布、已知/未知项目分布，要求 0 偏差；坏行数为 0 或逐行说明原因；
  - 核验运行前后 `~/.gemini/antigravity-cli`（含 `conversation_summaries.db` 与目录条目）与真实 `~/.tokenscope` 未发生任何修改；
  - 仅把脱敏汇总计数回填本计划，严禁写入任何会话文本、路径或敏感信息。

### AG08 文档、构建清单与交付

- `docs/stats-semantics.md`：新增 §2.4「Antigravity CLI（`~/.gemini/antigravity-cli`）」与 §3 去重说明（覆盖 FRESH 语义、`CHECKPOINT`、方案 A 模型提取、CCS 同口径未知项目/未知模型、`conversation_summaries.db` 只读策略）。
- `CLAUDE.md`：更新「项目目标」为五应用统计，更新「候选 agent」与适配器口径要点。
- `scripts/build-inputs.json`：运行 `python -B scripts/build_inputs.py --update` 并 `--check`。
- 版本与基线（仅在用户确认并进入正式交付阶段后执行）：
  - 新功能 + 设置格式 v4 + 隐私修订 4 → 使用 `python -B scripts/bump_version.py --bump minor --summary "新增 Antigravity CLI 用量来源支持"`（`0.9.10 → 0.10.0`），并确认 `v0.9.10` 已在 `upgrade_baselines.json` 登记（已由 PR #84 完成）；
  - 执行完整本地门禁（`TOKENSCOPE_PRECOMMIT_MODE=full`、Rust 普通与 acceptance 双模式 clippy/test、前端 typecheck/format/test/build/E2E、原生验收场景）并回填证据到本计划与 `docs/plans/README.md`。

---

## 5. 风险与前置确认提醒

1. **升级起点矩阵 `MAX_ORIGINS = 6` 的后一版影响（提前披露）**：
   - 本版 `0.10.0` 的 `upgrade_baselines.py select` 结果为 6 个起点（`0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.10`），**本版完全满足上限**；
   - 本版发布并登记 `settings_format = 4` 后，**下一版（如 `0.10.1` 或 `0.11.0`）**将因新增第 7 个格式代表而触发 `MAX_ORIGINS = 6` 守卫，届时需另行评审起点策略（不静默扩张上限）。
2. **图标方案**：
   - 默认复用仓库内已有打包资产 `frontend/src/assets/icons/googlegemini.svg`（可访问名称与 tooltip 为 `Antigravity CLI`）。如果你希望使用特定的 Antigravity 品牌 SVG 图标，请在确认时提供或说明。
3. **预览模型与 `unknown` 模型的费用显示**：
   - 2026-11-18 前旧子代理的 `"unknown"` 模型，以及尚未进入 models.dev / OpenRouter 公开价格表的预览模型（如 `gemini-3.8-flash-high`、`gemini-4-neon-xhigh`），在未手动配置 `~/.tokenscope/pricing.toml` 时均会如实显示为费用「未知」（橙色），不按 `$0` 伪造估算；`claude-opus-4-6-thinking` 等公开目录可命中模型正常计价。
4. **单 PR 交付节奏**：
   - AG01–AG08 在 `feat/antigravity-cli-adapter` 分支内按任务逐提交推进，作为一个完整产品 PR 交付（避免出现「已扩充来源但尚无隐私修订或界面」的半成品版本）。
