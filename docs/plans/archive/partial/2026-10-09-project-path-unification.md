# 跨工具项目路径统一与会话目录切换实施计划

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| A01 纯路径 / A02 Claude / A03 映射 / A04 Codex / A05 聚合下钻 | 实现与后续边界修复已完成 | — |
| A06 文档/验收 | 自动化已有，当前原生链尚待验 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11) |
| B01 语义证据与根规则 / B02 两个 cwd 字段 | 已完成 | — |
| B03 两侧项目根/切换联动 | 实现和自动化完成，原生链待验 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-09-project-path-unification.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** 同一路径启动的 Claude Code 与 Codex 会话归入同一项目，保留 Codex 已有的目录切换解析，再完善两侧切换目录后的请求归属。

**Architecture:** 路径身份在 source 层统一，聚合与下钻继续消费同一个项目 key。先修复启动路径身份及缓存迁移；再把会话初始路径与请求工作目录分开建模，依据结构化日志证据确定后续归属。展示名称不是项目身份。

**Tech Stack:** Rust、serde、SQLite、Tauri 2、Vue 3、Vitest。

**状态：** 阶段 A（A01–A06）与阶段 B（B01–B03）均已实施并提交；归属规则由用户 2026-10-09 确认（项目根归并，两侧一致）。外部复核已完成三轮：第二轮的三处缺陷（Codex 异常 cwd 破坏会话隔离、`file://` 绕过路径检查、切换后联动验收不足）与**第三轮的六处 P2 + 一项验收缺口**（缓存迁移、映射读取失败缓存、URI 点段绕过、POSIX 尾空格、UNC/URI 主机大小写、编码反斜杠误拒；Codex 与无缓存路径联动）均已修复并补回归，见文末两节「审查修复记录」。唯一未执行的验收项是 GUI 原生实例的目视确认（自动化与隔离数据路径验收均已完成），因此本计划保持 active、暂不归档。

## 1. 依据与范围

已核对源码：

- `src/source/claude.rs`：`ClaudeLine` 未读取 cwd，`project_of` 使用文件父目录相对来源根的路径；标准布局下为 slug。
- `src/source/codex.rs`：`session_meta`、`turn_context` 均能更新 `ScanState.cwd`；用量事件使用当前 cwd。不得为统一初始身份而删除此能力。
- `src/cache.rs`：当前 `SCHEMA_VERSION = "6"`，文件缓存仅按来源及日志文件指纹等信息匹配，外部项目映射变化不会自然使旧事件失效。
- `frontend/src/lib/viewSnapshot.ts`：当前 `SNAPSHOT_VERSION = 6`，界面磁盘快照也需迁移，避免启动短暂恢复旧 slug 分组。
- `docs/stats-semantics.md` §3.4 与 `tests/project_alias_cross_agent.rs` 明确保留 slug/cwd 独立身份，需要随新行为修订。

用户提供的探针报告指出：Claude 顶层 cwd 覆盖完整、同文件有子目录漂移；Codex 在上下文记录中提供 cwd；当前存在同路径 slug/cwd 分裂。报告中的文件数、事件数、覆盖率与 5.5 秒重建耗时是已有观测，不视为本计划重新验收的结果。

### 两阶段交付

| 阶段 | 必须交付 | 不据此宣称完成的内容 |
| --- | --- | --- |
| A：启动路径统一 | 可靠 cwd 归一、跨工具同路径合并、兜底与迁移、下钻一致；保留 Codex 当前路径更新 | Claude 会话中切换目录后的最终归属 |
| B：目录切换适配 | 分离初始路径与请求 cwd，验证两侧更新语义，按明确规则归属切换后的请求 | worktree、符号链接、任意父子目录自动归并 |

阶段 A 的临时差异必须写入口径：Claude 按会话首个有效 cwd 归属；Codex 继续按有效上下文更新归属。只有路径未切换的跨工具会话，才保证按共同启动路径归入同组；不得把阶段 A 描述成“两侧目录切换语义已经一致”。

### 非目标与边界

- 不反向解码 slug，不通过项目名称、路径前缀、工具命令中的 `cd` 猜身份。
- 不改变 sidechain 排除、token 统计、计价、请求去重口径。
- 不修改、清理真实 Claude/Codex 日志；不改变设置页来源目录规则。
- 不调用 `canonicalize` 依赖当前文件系统存在性，不解析符号链接或推断 Git 根；历史已删除目录仍可作为项目身份。
- worktree 归并保留为后续事项；放开 sidechain 前先补正式规则。
- 阶段 A 原则上不改页面布局，但允许修改界面快照版本及相关测试，不能声称“前端零改动”。

## 2. 不变量和待决规则

1. 同一规范化绝对路径跨工具得到同一 key；同名不同路径必须保持独立。
2. 排序、汇总、事件明细与下钻都使用身份 key，标签仅用于展示。
3. 变更项目归属后，事件总数、四类 token 总量、费用、未知价格状态不变；仅项目分组重新分配。
4. Codex 的 A → B → A 上下文更新仍对后续请求生效；新 session 缺 cwd 不得继承上个 session 的目录。
5. Claude 阶段 A 取每个 session 的首个有效 cwd，标准一文件一 session 情况等价于文件级冻结。不得因后续行变为子目录而重写全部历史归属；一个文件含多个 sessionId 时不得共用首值。
6. 首个有效 cwd 只是“可恢复的会话初始路径”：若日志前缀缺失，不能承诺它必然是进程真实启动目录。阶段 A 同 session 较晚出现首值时可用于该 session 已解析事件，但不能跨 session 补值。
7. JSON 行的 cwd 缺失、空值、类型异常或无效路径，不应让原本合法的 usage 事件新增 bad_lines；路径提取容错与 usage 校验分开。
8. 配置兜底只接受唯一正向映射。冲突、读取失败或映射不存在，保留旧相对目录/slug 身份及 `(根目录)` 兜底，不猜测合并。
9. 会话快照建立后身份与映射冻结，刷新获得新身份修订；旧游标不能续到新快照。

### 规范化规则（阶段 A 定稿）

- Windows 绝对路径：反斜杠统一为 `/`、盘符大写、非根目录去尾随分隔符；不全路径转小写。
- 根路径保留语义：`C:/` 不能变成 `C:`，POSIX `/` 保持 `/`，UNC 保留主机与共享根。
- `file://` 使用 URI 解析器，仅对 URI 做一次百分号解码；普通路径里的 `%20` 必须保留字面值。
- POSIX 路径保留大小写和合法反斜杠字符；不能把 Windows 规则套到所有字符串上。
- 相对路径、`C:foo`、不支持的 URI、非法编码或含无法安全解释的组件返回“不可用”，进入兜底；不相对 TokenScope 的 cwd 补全。
- 不自动折叠 symlink/junction、worktree 或目录别名；有歧义时宁可保持独立。

### 阶段 B 决策门

先完成 B01 证据核验，再定稿：

- 结构化 cwd 表示请求工作目录，还是会话级记录副本？Claude 的 user/assistant 行分别何时更新？
- A → A/sub 是否仍为项目 A，A → B 何时算切换项目？仅靠父子路径关系不足以判定。
- 候选基线为“按有效事件 cwd 精确分组”，不做父子目录自动归并；若要保留仓库级项目，需要可靠根身份或显式映射规则。

上述业务归属未明确前，可完成证据和数据模型设计，不上线自动重新分组。阶段 A 不等待 B 的最终规则。

**B01 核验结果（2026-10-09，详见 `docs/research/2026-10-09-session-cwd-semantics.md`）**：

- Codex `turn_context.cwd` = **轮级工作目录**（本机 905/905 带 cwd，66 个 session 中 1 个出现过切换，形态为完全换目录）；
- Claude 顶层 `cwd` = **行级状态快照**（每行都带；20 个 session 中 16 个恒定，唯一非 sidechain 漂移是「短暂进入子目录再返回」，另 3 个多值 session 全部由 sidechain 行构成）；
- 两侧更新语义不同，不能共用同一个「会话目录」解释；
- 实例 1–4 的产品选择见研究文档 §4；确认之前不上线任何自动重新分组。

**定稿（2026-10-09 用户确认）**：归属按**项目根**归并、两侧一致——从 `/test`
起步时，进入 `/test/123` 或更深目录仍归 `/test`；工作目录一旦**越出当前根**
（`/test` → `/bee`），该目录即成为新项目，其子目录（`/bee/123`）同属新项目。
判定用归一化路径的分量边界前缀，不做文件系统访问、不推断仓库根、不折叠
大小写；缺上下文不回溯、不借用未来切换（实例 4 保持"记 `(未知)` 后按新值"）。

## 3. 任务清单

每项按“写合成失败用例 → 运行确认失败 → 最小实现 → 定向验证 → 自查提交”执行。以下测试名是要新增或修订的验收入口，不代表已经存在或通过。

### A01 公共路径归一化

- [x] 修改 `src/source/mod.rs`，新增 `src/source/project_path.rs`；测试 `tests/project_path_normalization.rs`；必要时修改 `Cargo.toml` / `Cargo.lock`，优先复用已有 URI 依赖。（`url` 已锁定在依赖图中，提为直接依赖）
- [x] 定义可失败的纯函数 `normalize_project_path(raw: &str) -> Option<String>`，不得访问用户文件系统。
- [x] 测试 `windows_equivalent_paths_share_key`、`file_uri_decodes_once`、`roots_and_unc_are_preserved`、`posix_case_and_backslash_are_preserved`、`ambiguous_paths_are_rejected`。
- [x] 表驱动覆盖 `C:\a\b` / `c:/a/b/` → `C:/a/b`、中文、空格、字面 `%20`、根目录和不支持的 URI。**审查修订（2026-10-10）**：URI 路径同样受「拒绝 `.`/`..` 组件、解码后不含 NUL」约束——检查必须在 `url::Url::parse` **之前**完成（解析器会静默折叠点段）；合法空格（`%20`）正常解码且与普通路径同一身份，普通路径里的 `%20` 仍按字面。新增 `file_uri_rejects_dot_segments_and_nul`、`file_uri_and_plain_path_share_identity_for_spaces`。
- [x] 验证：`cargo test --test project_path_normalization`，5 项全过；提交 `feat(source): 统一项目路径身份归一化`（`6413b85`）。

### A02 Claude 会话初始 cwd 与目录兜底

- [x] 修改 `src/source/claude.rs`；新增 `tests/fixtures/project-path/claude-initial-cwd.jsonl`、`claude-cwd-drift.jsonl`、`claude-multi-session.jsonl`；新增 `tests/project_path_attribution.rs`（另加 `claude-invalid-cwd.jsonl` 覆盖无效 cwd 容错）。
- [x] 写 `claude_initial_cwd_is_shared_by_session_events`、`claude_subdirectory_drift_does_not_split_phase_a`、`claude_sessions_do_not_share_initial_cwd`、`invalid_cwd_does_not_drop_usage`。
- [x] 一次读取、一轮解析收集首值与事件，最后按 session 赋值；不要为每个事件重读文件，不改变坏行和 sidechain 计数。
- [x] 非 assistant 行中的有效 cwd 也可提供上下文；忽略 sidechain 对主会话身份的影响。没有 sessionId 的记录仅在该文件的无 ID 分组内使用首值，不跨文件共享。
- [x] 未找到 cwd 暂走既有 `project_of`，A03 再接唯一映射兜底。根下文件有 cwd 时优先 cwd；没有时仍为 `(根目录)`。
- [x] 验证：`cargo test --test project_path_attribution claude_`，4 项全过；提交 `fix(claude): 按会话初始cwd恢复项目身份`（`3068ac9`，壳锁文件同步 `7ddde7d`）。

### A03 唯一正向映射与配置依赖

- [x] 新增 `src/source/claude_projects.rs`、`tests/claude_project_mapping.rs`、合成 `tests/fixtures/project-path/claude-projects.json`；修改 `src/source/claude.rs`、`src/report.rs`、`src/cache.rs`。
- [x] 映射由显式配置路径注入（新选项 `claude_projects_path`），默认来源才解析对应账户的 `~/.claude.json`；自定义来源无明确配置关联时不读取本机映射。测试构造器始终注入临时文件或禁用映射。
- [x] 按确认的正向 slug 编码生成候选，使用编码前原始路径匹配 slug，匹配成功后再归一化身份。不同候选归一后仍不同则视为冲突；不选择第一个。
- [x] 测试 `unique_forward_mapping_resolves_legacy_slug`、`colliding_slugs_remain_unmerged`、`custom_root_does_not_use_home_mapping`、`missing_or_bad_config_keeps_usage`。
- [x] 每个采集批次只加载一次映射，形成确定性 revision；有效映射、缺失、损坏等状态变化均纳入 revision。不得每行读配置或只在启动时永久缓存（进程内按 size+mtime 纳秒指纹缓存）。
- [x] 给依赖映射的解析缓存增加上下文修订匹配（`files.context_rev`，解析版本 6 → 7），并让 `src/report.rs` 的内存复用键感知该修订（`collection_key` 含映射修订；`DedupSource` 显式转发 `context_revision`）；不改日志指纹也能在映射变化后重新解析。已建立查询会话继续使用旧冻结快照。
- [x] 测试 `mapping_change_invalidates_disk_and_memory_results`：日志不变、配置改动，下一次查询归属更新；原查询不变。验证：`cargo test --test claude_project_mapping`，5 项全过；提交 `fix(claude): 安全解析旧项目映射并纳入缓存失效`（`3aee5aa`）。

### A04 Codex 归一化与切换回归保护

- [x] 修改 `src/source/codex.rs`；在 `tests/project_path_attribution.rs` 增加 Codex 用例与 `tests/fixtures/project-path/codex-cwd-switch.jsonl`。
- [x] 测试 `codex_initial_path_matches_claude`、`codex_turn_context_changes_only_later_events`、`codex_a_b_a_switch_is_preserved`、`codex_new_session_does_not_inherit_cwd`。
- [x] `session_meta` 重置目录上下文；有效 `turn_context.cwd` 更新后续事件。不把 Codex 项目冻结到文件首值；空/无效字段不清掉同 session 已知目录。**审查修订（2026-10-10）**：`payload` 的上下文元数据（id / session_id / cwd / model）改为宽容类型——类型异常只表示"没有该值"，**不得让整条记录解析失败**（此前 `"cwd": 12345` 会让 `session_meta` 整行失败，会话边界重置被跳过，新会话请求被算到旧会话与旧项目）。用量字段 `info` 保持严格（类型异常按坏行计数）。新增 fixture `codex-invalid-cwd.jsonl` 与用例 `codex_invalid_cwd_does_not_skip_session_boundary`。
- [x] 无有效 cwd 使用现有未知项目兜底；初始上下文缺失时使用首个有效 turn_context，但不跨 session 回填。
- [x] 验证：`cargo test --test project_path_attribution codex_`（8 项全过）、`cargo test --lib codex`（15 项全过）；提交 `fix(codex): 归一项目路径并保留上下文切换`（`4eff289`）。

### A05 跨工具聚合、展示与下钻闭环

- [x] 修订 `tests/project_alias_cross_agent.rs`，新增 `tests/project_path_drilldown.rs`；核对 `src/aggregate.rs`、`src/report.rs`、`src/query.rs`，只在测试发现不兼容时修改（本轮无需改动管线代码）。
- [x] 旧用例拆为两种：有相同可靠 cwd 则合并；仅 basename 相同或 Claude 只有 slug 则不推断合并。
- [x] 测试 `same_path_merges_agents`、`same_basename_different_paths_stay_separate`、`merged_project_drilldown_returns_both_agents`、`project_merge_preserves_totals`。
- [x] 通过真实 `query_begin` / 汇总 / 下钻事件管线断言 key 精确回传；超过一页时续页仍属该项目（limit=2 翻三页），来源筛选可分别取出两侧事件。
- [x] 逐字段比较变更前后的总请求、四类 token、费用及未知价格状态；不能只对比项目行数。
- [x] 验证：`cargo test --test project_alias_cross_agent`（2 项）、`cargo test --test project_path_drilldown`（2 项）全过；提交 `test(project): 覆盖跨工具路径合并与下钻`（`1ffee42`）。

### A06 迁移、口径与阶段 A 验收

- [x] 修改 `src/cache.rs`：解析版本 6 → 7；若实施时版本已变化则递增实际版本，禁止覆盖别人的迁移（A03 已升到 7，本项递增到 **8**）；测试 `project_identity_version_reparses_legacy_rows`。
- [x] 修改 `frontend/src/lib/viewSnapshot.ts`、对应 `.test.ts`；递增快照版本（6 → **7**），拒绝旧分组/旧下钻 key。测试 `old_project_identity_snapshot_is_ignored`；核对 `frontend/src/views/Dashboard.vue` 的恢复路径（`cached.v !== SNAPSHOT_VERSION` 即忽略）。
- [x] 更新 `docs/stats-semantics.md` §3.4、`CLAUDE.md` 项目身份说明、本计划和 `docs/plans/README.md`；不将 B 写为已完成。
- [x] 用临时目录中的旧版本缓存验证：首次升级重解析、第二次热命中，两次结果一致；模拟配置映射更新后再验证缓存失效（`tests/project_identity_acceptance.rs` + `tests/claude_project_mapping.rs::mapping_change_invalidates_disk_and_memory_results`）。
- [x] 验证：`cargo test project_identity_version`、`pnpm --dir frontend test -- src/lib/viewSnapshot.test.ts src/views/Dashboard.test.ts`，再执行 §4 门禁。
- [x] 隔离数据路径验收：`tests/project_identity_acceptance.rs` 用隔离来源/缓存/配置/价格（临时目录）核对同项目一行、展示名正常、下钻含两侧事件；"启动不闪现旧分组"由前端旧快照忽略用例等价覆盖。**边界**：GUI 原生实例的目视确认（真实窗口启动观感）仍待发布验收执行；真实缓存冷重建未在本轮执行，不以旧报告的 5.5 秒作性能保证。
- [x] 提交 `fix(cache): 迁移统一项目身份并更新统计口径`；填写阶段 A 独立验收记录（见文末「实施记录」）。

### B01 目录切换证据与归属规则定稿

- [x] 新增 `docs/research/2026-10-09-session-cwd-semantics.md`，使用合成/受控会话记录验证 Claude 和 Codex 的 A → B → A、A → A/sub、恢复会话及多 session 边界（先在本机真实日志上只读统计，再用合成样例表达结论）。
- [x] 仅保留最小结构化字段样例（type、sessionId、cwd、时间与合成 usage）；不提交真实提示词、工具输出或用户日志。
- [x] 区分 `session_meta`、`turn_context`、Claude user/assistant 顶层 cwd，以及仅 shell 子进程执行 `cd` 的反例（后者作为「未验证推论」明示）。
- [x] 明确记录哪种变化足以代表当前请求工作目录；不能仅凭“存在 cwd 字段”认定更新语义相同（Codex 轮级快照 vs Claude 行级快照）。
- [x] 定稿 §2 决策门：事件 cwd 与项目归属的关系、父子目录处理、缺上下文处理；需要产品选择的规则列成具体实例供确认，不默认归并仓库根。**→ 已由用户 2026-10-09 确认（项目根归并：子目录归并、越界切换；缺上下文不回溯）。**
- [x] 验证：将观测转换为 B03 的具名 fixture（研究文档 §5 给出用例 → fixture 映射）；核对每个结论都有样例与局限。提交 `docs(project): 核验会话目录变化语义并列出归属待决项`（本次）。

### B02 分离初始项目路径与请求工作目录

- [x] 按 B01 已定规则修改 `src/model.rs`、`src/source/claude.rs`、`src/source/codex.rs`、`src/cache.rs`；若暴露到 IPC，再同步 `src/report.rs` 与 `frontend/src/types.ts`，不强制新增界面控件（`EventRow` 增两个可选字段，界面未新增控件）。
- [x] 数据模型分别表达 `session_initial_cwd` 与 `event_cwd`（均为 `Option<String>`），已有 `project` 保存已决归属 key；字段名称可按仓库惯例定稿，但语义不可复用成一个字段。
- [x] 新字段必须贯通序列化和缓存读写；若两阶段分批发布，B 再递增解析版本，并按兼容性判断界面快照版本，不复用已发布的版本 7（解析版本 8 → **9**，界面快照 7 → **8**）。
- [x] 更新 `src/report.rs` 保留内存预算估算与 `src/query.rs` 相关测试；新增字符串也计入预算，不能只修改模型而漏掉保留内存（`collection_retained_bytes` 计入两个 `Option<String>`）。
- [x] 测试 `initial_cwd_is_stable_while_event_cwd_changes`、`cwd_context_survives_cache_roundtrip`、`new_cwd_fields_are_in_memory_budget`。
- [x] 验证：`cargo test cwd_context`、`cargo test initial_cwd`、`cargo test memory_budget` 均命中并通过；提交与 B03 合并为 `feat(project): 支持结构化会话目录切换归属`（字段与归属规则耦合，拆开会留下不可编译的中间状态——已在提交信息中说明）。

### B03 双侧切换归属与最终验收

- [x] 修改两侧适配器与 `tests/project_path_attribution.rs`、`tests/project_path_drilldown.rs`，新增 `tests/fixtures/project-path/session-switch/` 合成用例。
- [x] 测试 `claude_structured_cwd_switch`、`codex_structured_cwd_switch`、`switch_does_not_reassign_previous_events`、`shell_cd_text_does_not_change_project`、`both_agents_follow_decided_subdirectory_policy`。
- [x] 未知上下文不借用下一 session 或未来切换的 cwd；归属变化不回溯修改已发生请求。
- [x] 验证切换后 A/B 各组的请求与 token，A → B → A 可正确返回 A；下钻、分页、热缓存和无缓存结果一致。**收窄（2026-10-10 审查）**：本轮补齐 `switch_groups_drilldown_pages_cache_and_cost_are_consistent`（`tests/project_path_drilldown.rs`）——同一会话切换 fixture 上串成一组断言：A/B 两组请求数与费用、冷/热缓存逐字段一致、两组费用之和 = 合计且各自 = 明细逐条之和、A 组 `limit=2` 翻页 2+1 不重不漏、B 组明细保留切换后的原始工作目录。此前只验证了"切换解析"与"阶段 A 的分页"，不构成完整联动验收。
- [x] 执行 §4 门禁及隔离原生验收（隔离数据路径部分），更新统计口径与阶段状态。只有 B01–B03 和 A 均验收完成才归档整份计划——**GUI 原生目视项未执行，故本计划保持 active、不归档**。
- [x] 提交 `feat(project): 支持结构化会话目录切换归属`（含 B02 数据模型，见 B02 说明）。

## 4. 验证命令与执行纪律

PowerShell 中先补 Cargo 路径，命令逐条运行并检查退出码：

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
```

- 所有测试的 `SummaryOptions.cache_dir`、`pricing_index`、来源目录及映射配置均显式注入临时路径，不触碰真实 `~/.tokenscope`、`~/.claude.json` 或来源日志。
- 不设置真实性能测试授权环境变量，不顺带执行真实缓存重建。需要真实验收时单列范围与结果，优先只读日志及隔离派生目录。
- 每项记录测试命令、结果、commit 和证据位置；阶段完成不能仅凭全套测试绿色判断。
- 不跳过提交钩子；按项目默认流程提交和推送。远端更新先检查再整合，不强推。
- 阶段 A/B 已按任务顺序执行完毕；探针脚本只在临时目录运行，不作为仓库依赖。

## 5. 完成标准与记录

| 项目 | 阶段 A | 阶段 B |
| --- | --- | --- |
| 同路径跨工具合并、同名不同路径不合并 | 必须通过 | 回归通过 |
| 四类 token、费用和请求总数守恒 | 必须通过 | 必须通过 |
| 映射冲突与配置变化失效 | 必须通过 | 回归通过 |
| Codex A → B → A 保留 | 必须通过 | 两侧统一规则通过 |
| Claude cwd 漂移 | 按首值归属并注明阶段限制 | 按定稿规则分配后续请求 |
| 磁盘/内存缓存、启动视图与下钻一致 | 必须通过 | 必须通过 |
| worktree / sidechain 归并 | 不实施 | 不实施 |

## 6. 实施记录（阶段 A + 阶段 B）

任务按顺序实现，每项先写失败用例再实现；提交均为独立 commit，pre-commit 钩子（根库与壳的 fmt/clippy/test、前端 typecheck/format/test）每轮通过。

| 任务 | 提交 | 定向验证 |
| --- | --- | --- |
| A01 公共路径归一化 | `6413b85` | `cargo test --test project_path_normalization`（5 项） |
| A02 Claude 会话初始 cwd（另含壳锁文件同步 `7ddde7d`） | `3068ac9` | `cargo test --test project_path_attribution`（Claude 侧 4 项） |
| A03 唯一正向映射与缓存失效 | `3aee5aa` | `cargo test --test claude_project_mapping`（5 项）+ 映射/缓存单测 |
| A04 Codex 归一化与切换保护 | `4eff289` | `cargo test --test project_path_attribution`（8 项）、`cargo test --lib codex`（15 项） |
| A05 跨工具聚合与下钻 | `1ffee42` | `cargo test --test project_alias_cross_agent`（2 项）、`--test project_path_drilldown`（2 项） |
| A06 迁移、口径与验收 | `72541d6` | `cargo test --lib project_identity_version`、`cargo test --test project_identity_acceptance`、前端快照用例 |
| B01 目录切换证据核验 | `419a965` | 只读统计 + `docs/research/2026-10-09-session-cwd-semantics.md`（四个待决实例） |
| B02 数据模型分离 | 与 B03 合并 `eb1c9c9` | `cargo test cwd_context`、`cargo test initial_cwd`、`cargo test memory_budget` |
| B03 归属规则与验收 | `eb1c9c9` | `cargo test --test project_path_attribution`（15 项 + 阶段 A 回归） |

阶段 A 验收结论（逐项对应 §5 完成标准）：

- 同路径跨工具合并、同名不同路径不合并：`same_path_merges_agents` / `same_basename_different_paths_stay_separate` / `codex_initial_path_matches_claude` 通过。
- 四类 token、费用与请求总数守恒：`project_merge_preserves_totals`（含身份分裂对照）与 A06 隔离验收逐字段通过。
- 映射冲突与配置变化失效：`colliding_slugs_remain_unmerged`、`mapping_change_invalidates_disk_and_memory_results` 通过（已建立的查询会话继续使用旧冻结快照）。
- Codex A → B → A 保留：`codex_a_b_a_switch_is_preserved`、`codex_new_session_does_not_inherit_cwd` 通过。
- Claude cwd 漂移：阶段 A 按会话初始路径归属（`claude_subdirectory_drift_does_not_split_phase_a`），阶段限制当时已写入口径。
- 磁盘/内存缓存、启动视图与下钻一致：缓存解析版本 6 → 7（映射修订）→ 8（身份口径），前端快照版本 6 → 7；`old_project_identity_snapshot_is_ignored` 与 A06 隔离验收通过。
- worktree / sidechain 归并：未实施（不在范围内）。

阶段 B 验收结论（同上逐项对应）：

- 两侧统一规则：`both_agents_follow_decided_subdirectory_policy`（子目录归并）与 `claude_structured_cwd_switch` / `codex_structured_cwd_switch`（越界切换）通过。
- 不回溯：`switch_does_not_reassign_previous_events` 通过（切换只影响其后事件）。
- 反例：`shell_cd_text_does_not_change_project` 通过（工具文本里的 `cd` 不改归属）。
- 数据模型与缓存：`initial_cwd_is_stable_while_event_cwd_changes`、`cwd_context_survives_cache_roundtrip`、`new_cwd_fields_are_in_memory_budget` 通过；解析版本 9、界面快照版本 8。
- 回归：阶段 A 全部用例（合并/守恒/映射失效/下钻分页）与 §4 门禁（根库与壳 fmt/clippy/test、前端 typecheck/format/test/build）全绿。
- worktree / sidechain 归并：仍不实施（用户定稿规则不需要仓库根推断）。

口径修订汇总（已同步 `docs/stats-semantics.md` §3.4 与 `CLAUDE.md`）：Claude 身份由「文件父目录相对路径（slug）」→「项目根归并（会话 cwd 推进）」，Codex 由「原始 cwd / 轮级精确」→「项目根归并」；新增 `session_initial_cwd` 与 `event_cwd` 两个事件字段。

未完成与边界：GUI 原生实例的目视验收（真实窗口启动观感、隔离实例截图）未执行——本计划因此保持 active，不归档；未执行真实缓存冷重建（不以旧报告的 5.5 秒作性能保证）。探针脚本 `ts_probe*.py` 只作可选排查辅助，不作为仓库测试依赖，也不保证临时文件仍存在。

### 审查修复记录（2026-10-10）

外部审查（证据 `qa-artifacts/project-path-review-probe.log`，3 项边界探针全部复现）提出三类问题，本轮逐项修复并补回归：

| 问题 | 复现 | 修复 | 回归用例 |
| --- | --- | --- | --- |
| Codex 异常 cwd 破坏会话隔离 | 新 `session_meta` 的 `"cwd": 12345` → 整行解析失败、会话边界被跳过（新会话请求被标成旧会话 `old` 并继承旧项目） | `RolloutPayload` 的 id / session_id / cwd / model 改为宽容类型 + 字符串提取；`info` 保持严格 | `codex_invalid_cwd_does_not_skip_session_boundary`（新增 fixture `codex-invalid-cwd.jsonl`） |
| `file://` 绕过异常路径检查 | `file:///C:/a/../b` 被 `url::Url` 静默折叠为 `C:/b`；`file:///C:/a%00b` 解码后含 NUL 仍被接受 | 在 `Url::parse` **之前**对原始 URI 路径做一次解码并检查 `.`/`..` 组件与 NUL；解析后再查一遍；空格（`%20`）与普通路径字面 `%20` 的语义不变 | `file_uri_rejects_dot_segments_and_nul`、`file_uri_and_plain_path_share_identity_for_spaces` |
| 切换后的完整联动验收不足 | —（验收缺口，非运行时缺陷） | 新增联动断言：A/B 分组请求数与费用、冷/热缓存逐字段一致、费用守恒（两组之和 = 合计 = 明细之和）、`limit=2` 翻页不重不漏、明细保留切换后的原始目录 | `switch_groups_drilldown_pages_cache_and_cost_are_consistent` |

审查同时确认：正常路径上的跨工具合并、子目录归并、越界切换、映射失效、新字段缓存与内存预算均已落地。修复后复跑定向用例与全套门禁（根库/壳 fmt+clippy+test、前端 typecheck/format/test/build）全绿。

### 审查修复记录（第三轮，2026-10-10）

外部复核（隔离探针复现）提出六处 P2 与一项验收缺口，本轮逐项修复：

| 问题 | 复现 | 修复 | 回归用例 |
| --- | --- | --- | --- |
| 修复未迁移旧缓存 | 隔离升级后 2 次请求仅统计为 1 次，强制刷新才恢复（修复前后的解析产物不同，但版本号都是 9） | 缓存解析版本 9 → **10**（Codex 会话边界、file URI 点段、UNC/URI 主机大小写、POSIX 尾空格修复后的内容变化）；前端快照 9 → **10** | `repaired_semantics_require_cache_migration` + 快照版本用例 |
| 映射读取失败被持续缓存 | 配置短暂被独占锁后，恢复可读仍沿用失败映射、保留 slug，直到指纹变化或重启 | `load_cached` 只在状态 **非 `Unusable`** 时写进程内缓存（读取失败可重试） | `locked_config_is_retried_after_recovery`（Windows 独占打开，确定性复现） |
| URI 点段检查仍可绕过 | `file://server\share\a\..\b` 被接受为 `//server/share/b`（authority 后取不到 `/` 时返回空路径） | authority 判定同时接受 `\` 终止（与解析器对 file URI 的反斜杠归一一致） | `uri_backslash_handling_matches_path_semantics` |
| POSIX 尾空格被删除 | `/tmp/alpha ` 与 `/tmp/alpha` 得到同一 key（误合并两个目录） | 去掉整体 `trim`（仅"全空白"判不可用；尾/首空格是合法文件名字符） | `posix_trailing_space_is_preserved` |
| UNC 与 URI 身份不一致 | `\\Server\Share\Dir`（保留大写）与 `file://Server/Share/Dir`（URL 小写）拆成两个身份 | `normalize_unc` 主机段小写化，与 URI 主机解析同口径 | `unc_and_file_uri_agree_on_host_case` |
| 合法 POSIX 反斜杠被误拒绝 | 普通 `/tmp/a\..\b` 接受，但对应 URI `file:///tmp/a%5C..%5Cb` 返回不可用 | 点段检查改为"按**未编码**分隔符切分组件 + 逐组件解码"：`%5C` 是字面字符，`%2e%2e` 仍是点段 | 同上 |
| 验收缺口：缺 Codex 与无缓存路径 | — | 新增 Codex 切换联动断言与 `refresh = true`（无缓存路径）一致性比较 | `codex_switch_groups_and_no_cache_path_are_consistent` |

六处修复的定向测试与全量门禁（根库/壳 fmt+clippy+test、前端 typecheck/format/test 323 项/build）全部退出码 0。

### 隔离数据路径验收执行记录（2026-10-09）

1. 准备隔离根（合成来源 + 隔离缓存/配置/价格）：

   ```powershell
   $root = Join-Path $env:TEMP ('tokenscope-native-' + [guid]::NewGuid().ToString('N'))
   $src = Get-Content -Raw -Encoding UTF8 .\scripts\prepare-native-acceptance.ps1
   & ([scriptblock]::Create($src)) -Root $root   # 5.1 宿主需显式 UTF-8 读取（见脚本 .NOTES）
   $env:TOKENSCOPE_ACCEPTANCE_ROOT = $root
   ```

   合成日志已按阶段 A/B 的验收需要补 `cwd`：Claude 前两条位于
   `C:\acceptance\workspace\tokenscope`（其中一条是其子目录 `src`）与 Codex
   起始目录相同；后两条越出该根到 `C:\acceptance\workspace\bee`（子目录
   `bee\123`）。事件总数仍由 `-Events` 决定。

2. 归属前哨（非 GUI 路径，`tests/native_acceptance_attribution.rs`，`#[ignore]` +
   `TOKENSCOPE_ACCEPTANCE_ROOT` 显式授权）：

   `cargo test --test native_acceptance_attribution -- --ignored` → 通过：
   合并行含两侧 agent、展示名 `tokenscope`、切换后新项目行 `bee` 只含
   claude-code、`src` 与 `bee/123` 不独立成行、无 slug 残留、下钻含两侧事件。

3. RC10 隔离密闭性回归：`cargo test --features acceptance --test native_acceptance`
   → 3 项通过（另 2 项为父用例驱动的子进程用例）。

4. **未执行**：GUI 窗口目视（迁移后可执行：`$env:TOKENSCOPE_ACCEPTANCE_ROOT = $root`
   后启动 acceptance 构建，观察"同项目一行、下钻含两侧事件、启动不闪现旧分组"）。
   该步骤需要人工观察，故计划不归档。
