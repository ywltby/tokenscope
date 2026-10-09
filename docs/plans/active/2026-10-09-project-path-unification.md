# 跨工具项目路径统一与会话目录切换实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** 同一路径启动的 Claude Code 与 Codex 会话归入同一项目，保留 Codex 已有的目录切换解析，再完善两侧切换目录后的请求归属。

**Architecture:** 路径身份在 source 层统一，聚合与下钻继续消费同一个项目 key。先修复启动路径身份及缓存迁移；再把会话初始路径与请求工作目录分开建模，依据结构化日志证据确定后续归属。展示名称不是项目身份。

**Tech Stack:** Rust、serde、SQLite、Tauri 2、Vue 3、Vitest。

**状态：** 待实施，所有任务未完成。本轮只编制计划，不修改运行时代码或用户数据。

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

## 3. 任务清单

每项按“写合成失败用例 → 运行确认失败 → 最小实现 → 定向验证 → 自查提交”执行。以下测试名是要新增或修订的验收入口，不代表已经存在或通过。

### A01 公共路径归一化

- [ ] 修改 `src/source/mod.rs`，新增 `src/source/project_path.rs`；测试 `tests/project_path_normalization.rs`；必要时修改 `Cargo.toml` / `Cargo.lock`，优先复用已有 URI 依赖。
- [ ] 定义可失败的纯函数 `normalize_project_path(raw: &str) -> Option<String>`，不得访问用户文件系统。
- [ ] 测试 `windows_equivalent_paths_share_key`、`file_uri_decodes_once`、`roots_and_unc_are_preserved`、`posix_case_and_backslash_are_preserved`、`ambiguous_paths_are_rejected`。
- [ ] 表驱动覆盖 `C:\a\b` / `c:/a/b/` → `C:/a/b`、中文、空格、字面 `%20`、根目录和不支持的 URI。
- [ ] 验证：`cargo test --test project_path_normalization`，预期全部通过；提交 `feat(source): 统一项目路径身份归一化`。

### A02 Claude 会话初始 cwd 与目录兜底

- [ ] 修改 `src/source/claude.rs`；新增 `tests/fixtures/project-path/claude-initial-cwd.jsonl`、`claude-cwd-drift.jsonl`、`claude-multi-session.jsonl`；新增 `tests/project_path_attribution.rs`。
- [ ] 写 `claude_initial_cwd_is_shared_by_session_events`、`claude_subdirectory_drift_does_not_split_phase_a`、`claude_sessions_do_not_share_initial_cwd`、`invalid_cwd_does_not_drop_usage`。
- [ ] 一次读取、一轮解析收集首值与事件，最后按 session 赋值；不要为每个事件重读文件，不改变坏行和 sidechain 计数。
- [ ] 非 assistant 行中的有效 cwd 也可提供上下文；忽略 sidechain 对主会话身份的影响。没有 sessionId 的记录仅在该文件的无 ID 分组内使用首值，不跨文件共享。
- [ ] 未找到 cwd 暂走既有 `project_of`，A03 再接唯一映射兜底。根下文件有 cwd 时优先 cwd；没有时仍为 `(根目录)`。
- [ ] 验证：`cargo test --test project_path_attribution claude_`；提交 `fix(claude): 按会话初始cwd恢复项目身份`。

### A03 唯一正向映射与配置依赖

- [ ] 新增 `src/source/claude_projects.rs`、`tests/claude_project_mapping.rs`、合成 `tests/fixtures/project-path/claude-projects.json`；修改 `src/source/claude.rs`、`src/report.rs`、`src/cache.rs`。
- [ ] 映射由显式配置路径注入，默认来源才解析对应账户的 `~/.claude.json`；自定义来源无明确配置关联时不读取本机映射。测试构造器始终注入临时文件或禁用映射。
- [ ] 按确认的正向 slug 编码生成候选，使用编码前原始路径匹配 slug，匹配成功后再归一化身份。不同候选归一后仍不同则视为冲突；不选择第一个。
- [ ] 测试 `unique_forward_mapping_resolves_legacy_slug`、`colliding_slugs_remain_unmerged`、`custom_root_does_not_use_home_mapping`、`missing_or_bad_config_keeps_usage`。
- [ ] 每个采集批次只加载一次映射，形成确定性 revision；有效映射、缺失、损坏等状态变化均纳入 revision。不得每行读配置或只在启动时永久缓存。
- [ ] 给依赖映射的解析缓存增加上下文修订匹配，并让 `src/report.rs` 的内存复用键感知该修订；不改日志指纹也能在映射变化后重新解析。已建立查询会话继续使用旧冻结快照。
- [ ] 测试 `mapping_change_invalidates_disk_and_memory_results`：日志不变、配置改动，下一次查询归属更新；原查询不变。验证：`cargo test --test claude_project_mapping`；提交 `fix(claude): 安全解析旧项目映射并纳入缓存失效`。

### A04 Codex 归一化与切换回归保护

- [ ] 修改 `src/source/codex.rs`；在 `tests/project_path_attribution.rs` 增加 Codex 用例与 `tests/fixtures/project-path/codex-cwd-switch.jsonl`。
- [ ] 测试 `codex_initial_path_matches_claude`、`codex_turn_context_changes_only_later_events`、`codex_a_b_a_switch_is_preserved`、`codex_new_session_does_not_inherit_cwd`。
- [ ] `session_meta` 重置目录上下文；有效 `turn_context.cwd` 更新后续事件。不把 Codex 项目冻结到文件首值；空/无效字段不清掉同 session 已知目录。
- [ ] 无有效 cwd 使用现有未知项目兜底；初始上下文缺失时使用首个有效 turn_context，但不跨 session 回填。
- [ ] 验证：`cargo test --test project_path_attribution codex_`、`cargo test source::codex`；提交 `fix(codex): 归一项目路径并保留上下文切换`。

### A05 跨工具聚合、展示与下钻闭环

- [ ] 修订 `tests/project_alias_cross_agent.rs`，新增 `tests/project_path_drilldown.rs`；核对 `src/aggregate.rs`、`src/report.rs`、`src/query.rs`，只在测试发现不兼容时修改。
- [ ] 旧用例拆为两种：有相同可靠 cwd 则合并；仅 basename 相同或 Claude 只有 slug 则不推断合并。
- [ ] 测试 `same_path_merges_agents`、`same_basename_different_paths_stay_separate`、`merged_project_drilldown_returns_both_agents`、`project_merge_preserves_totals`。
- [ ] 通过真实 `query_begin` / 汇总 / 下钻事件管线断言 key 精确回传；超过一页时续页仍属该项目，来源筛选可分别取出两侧事件。
- [ ] 逐字段比较变更前后的总请求、四类 token、费用及未知价格状态；不能只对比项目行数。
- [ ] 验证：`cargo test --test project_alias_cross_agent`、`cargo test --test project_path_drilldown`；提交 `test(project): 覆盖跨工具路径合并与下钻`。

### A06 迁移、口径与阶段 A 验收

- [ ] 修改 `src/cache.rs`：解析版本 6 → 7；若实施时版本已变化则递增实际版本，禁止覆盖别人的迁移。测试 `project_identity_version_reparses_legacy_rows`。
- [ ] 修改 `frontend/src/lib/viewSnapshot.ts`、对应 `.test.ts`；递增快照版本，拒绝旧分组/旧下钻 key。测试 `old_project_identity_snapshot_is_ignored`；核对 `frontend/src/views/Dashboard.vue` 的恢复路径。
- [ ] 更新 `docs/stats-semantics.md` §3.4、必要的 `CLAUDE.md` 项目身份说明、本计划和 `docs/plans/README.md`；不将 B 写为已完成。
- [ ] 用临时目录中的 v6 缓存验证：首次升级重解析、第二次热命中，两次结果一致；模拟配置映射更新后再验证缓存失效。
- [ ] 验证：`cargo test project_identity_version`、`pnpm --dir frontend test -- src/lib/viewSnapshot.test.ts`，再执行 §4 门禁。
- [ ] 原生验收用隔离来源、缓存、配置和价格路径；同项目一行、展示名正常、下钻含两侧事件、启动无旧 slug 分裂闪现。真实缓存冷重建另列执行记录，不以旧报告的 5.5 秒作性能保证。
- [ ] 提交 `fix(cache): 迁移统一项目身份并更新统计口径`；填写阶段 A 独立验收记录。

### B01 目录切换证据与归属规则定稿

- [ ] 新增 `docs/research/2026-10-09-session-cwd-semantics.md`，使用合成/受控会话记录验证 Claude 和 Codex 的 A → B → A、A → A/sub、恢复会话及多 session 边界。
- [ ] 仅保留最小结构化字段样例（type、sessionId、cwd、时间与合成 usage）；不提交真实提示词、工具输出或用户日志。
- [ ] 区分 `session_meta`、`turn_context`、Claude user/assistant 顶层 cwd，以及仅 shell 子进程执行 `cd` 的反例。
- [ ] 明确记录哪种变化足以代表当前请求工作目录；不能仅凭“存在 cwd 字段”认定更新语义相同。
- [ ] 定稿 §2 决策门：事件 cwd 与项目归属的关系、父子目录处理、缺上下文处理；需要产品选择的规则列成具体实例供确认，不默认归并仓库根。
- [ ] 验证：将观测转换为 B03 的具名 fixture；核对每个结论都有样例与局限。提交 `docs(project): 明确会话目录变化的归属规则`。

### B02 分离初始项目路径与请求工作目录

- [ ] 按 B01 已定规则修改 `src/model.rs`、`src/source/claude.rs`、`src/source/codex.rs`、`src/cache.rs`；若暴露到 IPC，再同步 `src/report.rs` 与 `frontend/src/types.ts`，不强制新增界面控件。
- [ ] 数据模型分别表达 `session_initial_cwd` 与 `event_cwd`（可选），已有 `project` 保存已决归属 key；字段名称可按仓库惯例定稿，但语义不可复用成一个字段。
- [ ] 新字段必须贯通序列化和缓存读写；若两阶段分批发布，B 再递增解析版本，并按兼容性判断界面快照版本，不复用已发布的版本 7。
- [ ] 更新 `src/report.rs` 保留内存预算估算与 `src/query.rs` 相关测试；新增字符串也计入预算，不能只修改模型而漏掉保留内存。
- [ ] 测试 `initial_cwd_is_stable_while_event_cwd_changes`、`cwd_context_survives_cache_roundtrip`、`new_cwd_fields_are_in_memory_budget`。
- [ ] 验证：`cargo test cwd_context`、`cargo test initial_cwd`、`cargo test memory_budget`；提交 `feat(project): 分离会话初始路径和请求工作目录`。

### B03 双侧切换归属与最终验收

- [ ] 修改两侧适配器与 `tests/project_path_attribution.rs`、`tests/project_path_drilldown.rs`，新增 `tests/fixtures/project-path/session-switch/` 合成用例。
- [ ] 测试 `claude_structured_cwd_switch`、`codex_structured_cwd_switch`、`switch_does_not_reassign_previous_events`、`shell_cd_text_does_not_change_project`、`both_agents_follow_decided_subdirectory_policy`。
- [ ] 未知上下文不借用下一 session 或未来切换的 cwd；归属变化不回溯修改已发生请求。
- [ ] 验证切换后 A/B 各组的请求与 token，A → B → A 可正确返回 A；下钻、分页、热缓存和无缓存结果一致。
- [ ] 执行 §4 门禁及隔离原生验收，更新统计口径与阶段状态。只有 B01–B03 和 A 均验收完成才归档整份计划。
- [ ] 提交 `feat(project): 支持结构化会话目录切换归属`。

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
- 本轮编制计划不启动实施，后续按任务顺序执行；不要求并行代理。

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

实施记录：尚无。阶段 A 未开始；阶段 B 未开始。探针脚本 `ts_probe*.py` 只作可选排查辅助，不作为仓库测试依赖，也不保证临时文件仍存在。
