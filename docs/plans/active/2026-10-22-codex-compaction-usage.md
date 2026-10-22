# Codex 压缩用量补计与线程设置模型归属

> 状态：**用户 2026-10-22 确认计划文本，实施中。** 本计划是一次独立的统计口径缺陷修复，不占整合计划的责任编号；合并并验证发布后归档，并在计划总账登记。来源：对照 cc-switch v4.0.8 → v4.0.18 的 Claude/Codex 会话适配器变化（研究文档 §15.1），只关注 token 用量；首字时间、生成速度不做。

**Goal：** Codex 远程压缩（compaction）那次请求的用量不再被记成 0；Codex 只通过线程设置事件切换模型时，后续用量归到新模型。

**Architecture：** 适配器的两处修改在 `crates/tokenscope-core/src/source/codex.rs` 的逐行状态机内；升级纠正由历史库采集专用事务入口完成：
- 新增「待配对的 `token_usage_record`」状态，由下一条 `token_count` 决定是否单独产出事件；
- `event_msg/thread_settings_applied` 与 `turn_context` 一样更新当前模型。

去重、历史库写入、CCS 导入沿用现有的保守重播身份（`会话 | 原始模型 | 四桶`），不新增身份方案。递增 `PARSE_RULES_REVISION`，让已经登记的旧日志重新解析一次，补回已漏计的历史用量。

用户 2026-10-22 授权的升级例外：采集写入可按 §3.5 的全库唯一规则纠正旧 Codex 事实的模型归属。只增加采集专用的历史库事务入口与纯内存诊断，不改变 CCS 预演/导入内核、日汇总或其他来源，不增加表结构、身份方案或持久计数列。

**Tech Stack：** Rust、serde；`codex.rs` 内的单元测试与合成 fixture；本机只读统计脚本（不入库）。

## 1. 已核实事实（2026-10-22）

### 1.1 上游变化

- cc-switch [PR #8479](https://github.com/farion1231/cc-switch/pull/8479)（`8cfe5644`，v4.0.17）：Codex 0.153+ 每次模型响应都写一条顶层 `token_usage_record`（`payload.usage` 的四个字段与 `token_count` 的 `last_token_usage` 同名，另有 `payload.response_id`）。远程压缩时，这条记录有真实用量，而紧随其后的 `token_count` 的 `last_token_usage` 四个分量全为 0（`total_token_usage` 仍非零）。只认 `token_count` 的导入器会把压缩请求记成 0。cc-switch 的规则：
  1. 记录先挂起，等下一条 `token_count`；
  2. `last_token_usage` 与记录完全相同 → 只保留 `token_count` 那一行；
  3. `last_token_usage` 存在且全为 0 → 把记录单独作为一条请求导入；
  4. 两者都非零但不相同 → 记录只作计时参考，不计费（否则会重复计数，它有对应的反例 fixture）；
  5. 文件末尾仍未配对的记录不计费（实时会话常先写记录、后写 `token_count`，提前计费会在下一轮与 `token_count` 重复）；
  6. 后一条记录会顶替尚未配对的前一条。

  它还单独列出一个尚未解决的情况：分叉子线程重放父线程前缀时，压缩记录也会一起被复制（上游 issue 7917 第 1 项）。
- cc-switch [PR #8176](https://github.com/farion1231/cc-switch/pull/8176)（`68122bc4`）：Codex 可能只写 `event_msg/thread_settings_applied`（`payload.thread_settings.model`）来切换模型，而不写新的 `turn_context`；只认 `turn_context` 的导入器会把后续用量归给旧模型，若此前没有模型上下文则归为 unknown。规则：读取非空字符串模型，忽略缺失、非字符串与空白值；之后出现的 `turn_context` 照常覆盖。

### 1.2 本项目现状

- `source/codex.rs:5-6` 与 `docs/stats-semantics.md` §2.2 把 `token_usage_record` 认定为「线程级累计回显」，一律忽略并计数（`codex.rs:323`，`ignored_token_usage_record`）。
- 全 0 的 `last_token_usage` 按「零分量占位行」跳过（`codex.rs` `ingest_token_count`，`skipped_zero_usage`；头注释记录当时本机 278 条）。
- 模型只从 `turn_context` 取（`codex.rs:317-322`），`session_meta` 重置；没有模型时计 `skipped_no_model` 跳过。
- 未变化文件的跳过判定（`report.rs:1407-1423`）由「大小:mtime 指纹 + `PARSE_RULES_REVISION`（`report.rs:602`，当前 2）+ 来源上下文版本」组成；递增 `PARSE_RULES_REVISION`，已登记的文件会在下一轮重新解析一次。`history.rs:39` 的 `PARSER_REVISION`（当前 1）随事件和来源记录一起存储，并进入持久格式指纹（`persistent_format.rs`）。
- 历史库来源键 = `native|应用|路径|行序|内容指纹`（`report.rs:2205-2216`）：重新解析时，已有事件的来源键不变，写入幂等；新产出的压缩事件是新键。
- CCS 导入对带 `session_id` 的 Codex 行登记与原生同构的保守重播身份（`import/ccs.rs:855`、`:993`）。cc-switch v4.0.17+ 把压缩用量写成单独的明细行（`codex_thread:{root}:usage:{response_id}`，`data_source = codex_session`），只覆盖它升级后新读到的行。

### 1.3 本机实测（只读，只输出计数与合计，脚本不入库）

本机 `~/.codex` 共 68 个 rollout 文件，`session_meta` 中的 CLI 版本跨 0.116.0–0.162.1。

| 项 | 数量 |
| --- | --- |
| `token_usage_record` | 5555 条，分布在 21 个文件 |
| 与随后 `token_count` 的 `last_token_usage` 四分量完全相同 | 5511 |
| 非零记录后紧跟全 0 的 `token_count`（压缩） | **44** |
| 都非零但不相同 / 被后一条顶替 / 文件末尾未配对 | 0 / 0 / 0 |
| 全 0 的 `token_count` 总数 | 337 |

- 44 次压缩请求被跳过，**漏计输入 10,712,650（其中缓存命中 9,532,672）、输出 205,483**；占本机 Codex 全部 `token_count` 合计的输入 0.22%、缓存命中 0.20%、输出 1.18%。
- 5511 条完全一致说明 `payload.usage` 是单次用量，「累计回显」的旧结论不成立；之前没有出问题，是因为这些用量正好已被 `token_count` 覆盖。累计值在 `payload.thread_token_usage` / `turn_token_usage`，本计划不读取。
- 分叉：21 个带父线程（`forked_from_id` 或 `thread_spawn.parent_thread_id`）的子线程文件都包含父线程的 `session_meta`，且所有会话 id 相同（子线程的 `payload.session_id` 是根会话 id）。按现有键去重合并了 3489 条重放，未发生重复计数。因此不需要 cc-switch 那套「定位父线程文件、扣除重放前缀」的逻辑。
- 线程设置：564 条 `thread_settings_applied`，都带模型，其中 7 条改变了模型；但每次改变之后、下一条非零 `token_count` 之前都已有新的 `turn_context`，本机**没有**用量被归错或因无模型被跳过。这是潜在风险，现有日志不会因此改变统计。

## 2. 范围与不做

做：
- CP01 先写失败测试（压缩配对与线程设置模型）。
- CP02 实现压缩记录配对。
- CP03 实现线程设置模型归属。
- CP04 递增解析规则版本，让旧文件重新解析一次。
- CP05 文档、口径与真实数据复核，交付。

不做：
- 首字时间、生成速度、请求耗时（用户 2026-10-22 明确不做）。
- 不用 `response_id` 建立新的强身份，不改 `(会话, 原始模型, 四桶)` 去重键，不新增历史库身份方案或表结构。
- 不实现 cc-switch 的父线程定位与重放前缀扣除（§1.3：现有键已覆盖）。
- 不读取 `thread_token_usage` / `turn_token_usage`，不做累计差分。
- Claude 适配器不改：v4.0.8 → v4.0.18 的 Claude 改动只涉及首字时间与计价缓存，token 口径没有变化。
- 不改 CCS 导入逻辑，只补回归测试证明并集不重复。

## 3. 必须保持的不变量

1. **配对规则与上游一致**：记录只在「下一条 `token_count` 的 `last_token_usage` 存在且四分量全 0」时单独产出事件；完全相同时只保留 `token_count`；都非零但不同时不产出；文件末尾、`session_meta` 边界前仍未配对的记录不产出；后一条记录顶替前一条。
2. **不重复计数**：
   - 同一文件内同一次压缩只产出一条事件；
   - 分叉子线程重放的压缩记录与父文件中的同一记录按现有键合并为一条；
   - CCS 导入的压缩明细行与原生压缩事件命中同一保守身份，导入顺序不影响并集结果；
   - 对已有 5511 条一致配对，统计与修复前逐字段相同。
3. **事件属性**：压缩事件的时间戳取记录自身的 `timestamp`（缺失或非法计坏行，不产出），模型、项目根、会话 id 取记录所在位置的当前状态；四桶按现有 R02 归一化（`input = raw - cached - cache_write`），并做同样的受检算术与 `cached + cache_write ≤ input` 校验，违反即计坏行。
4. **模型归属顺序**：`turn_context` 与 `thread_settings_applied` 按行序，后出现者生效；`session_meta` 重置；缺失、非字符串或空白的模型值不改变当前模型。
5. **历史保留与模型纠正例外（用户 2026-10-22 授权）**：重新解析补新事件，不删除旧用量。仅以下条件全部满足时允许原地纠正模型：
   - 新事件为 Codex；候选是库内 parser_revision 早于当前版本的原生 Codex 事实；会话、精确事件时间、四桶相同，只有原始模型不同。不以来源路径或行号作为匹配条件，归档移动后同样适用。
   - 候选在全库唯一。零候选走原有新增/合并路径；多候选不猜测、不纠正，也不把该输入新增成第三份用量，保留原总量并记录本轮歧义诊断。
   - 仅更新原行 model_raw、派生 model_identity 与 parser_revision；事件 id、event_key、时间、四桶、项目/其他事实属性、原来源证据全部保留。纠正动作不新增来源证据；后续正常写入仍沿用既有来源登记规则。
   - 旧 codex-usage 别名保留，并给同一事件追加新模型对应的同方案别名；旧模型 CCS 再导入不增加用量。经旧别名命中已纠正事实的模型单项重播不能把模型改回去。
   - 当前 parser_revision 的事实不参与这条候选纠正路径。其他来源、CCS 日汇总及一般导入写入行为不变。
   - 以上例外之外，已有事件与来源证据不改写、不删除；已删除来源日志的历史保持原样，漏计无法补回。
6. **诊断如实**：`skipped_zero_usage` 不再包含已转为压缩事件的那些行；被忽略的记录仍计入 `ignored_token_usage_record`。采集专用结果增加纯内存 models_corrected / ambiguous_models 计数，纠正也计入本轮 updated、只推进一次 generation；歧义跳过且无其他写入时不推进 generation。本轮纠正/歧义计数并入提交后的 info 摘要，歧义另在既有警告通道披露条数，不新增查询字段或持久计数列。
7. **普通构建与其他来源零变化**：Claude 与 CCS 的解析结果、价格计算、查询接口不变。

## 4. 任务

### CP01 失败测试

在 `codex.rs` 测试模块用合成 fixture 先写下列用例，实现前确认红灯（失败原因必须是断言本身）：

- `compaction_record_followed_by_zero_last_is_counted`：记录（如 26705/0/0/491）+ 全 0 `token_count` → 1 条事件，四桶与时间戳、模型正确。
- `record_matching_following_token_count_is_not_double_counted`：一致配对 → 只有 `token_count` 一条。
- `record_mismatching_nonzero_token_count_is_ignored`：都非零但不同 → 只有 `token_count`。
- `unpaired_record_at_eof_or_session_boundary_is_not_counted`：文件末尾、`session_meta` 之前未配对 → 0 条。
- `later_record_replaces_pending_record`：连续两条记录后接全 0 `token_count` → 只产出后一条。
- `zero_or_malformed_record_is_ignored`：全 0 / 字段非法 / 时间戳缺失的记录不产出，非法者计坏行。
- `compaction_record_subset_violation_is_bad_line`：`cached + cache_write > input` 计坏行。
- `forked_child_replay_of_compaction_is_deduped`：父、子两个文件各含同一压缩记录（同根会话 id）→ 经全局去重后 1 条。
- `thread_settings_model_applies_to_following_usage`：先 `turn_context` 为 A，后 `thread_settings_applied` 为 B，再 `token_count` → 归 B；之后 `turn_context` 为 C → 归 C。
- `thread_settings_model_without_prior_turn_context`：没有 `turn_context`，只有线程设置 → 归该模型，不再计 `skipped_no_model`。
- `thread_settings_invalid_model_is_ignored`：缺失、非字符串、空白不改变当前模型。
- CCS 并集回归（`tests/` 下现有 CCS 导入测试旁）：先导 CCS 压缩明细行再原生采集、或反过来，总用量相同且不重复。

### CP02 压缩记录配对

`ScanState` 增加待配对记录（四桶、时间戳、行号、当时的模型 / 会话 / 项目状态）。`token_usage_record` 分支解析 `payload.usage` 并挂起；`token_count` 分支在现有校验前完成配对判定；`session_meta` 清空挂起；文件结束丢弃挂起。更新 `codex.rs` 头注释中「累计回显」的错误结论。

### CP03 线程设置模型

`event_msg` 增加 `thread_settings_applied` 分支，读取 `payload.thread_settings.model`，有效时更新 `state.model`。`RolloutPayload` 只增加所需字段，不放宽其他字段的类型。

### CP04 让旧文件重新解析

递增 `report.rs` 的 `PARSE_RULES_REVISION`（2 → 3），并更新其注释说明 v3 的原因。核实 `history.rs` 的 `PARSER_REVISION` 的读取方：按其注释「适配器输出语义变化时递增」应递增 1 → 2；递增前确认它不会触发历史迁移或拒绝打开，并确认持久格式指纹变化对 `scripts/upgrade_baselines.py select` 起点矩阵的影响（新增一种持久格式代表属正常，超过 6 个起点需重新评审）。两者分别记录结论。

补一条采集层测试：已登记指纹、内容未变的 Codex 文件，在规则版本递增后的下一轮会被重新解析并补入压缩事件，原有事件不重复、不改写。

依 §3.5 增补旧模型升级回归：单条原生旧事实纠正后仍为一行且四桶不变；移到 archived_sessions 后仍能匹配；纠正后再导入旧模型 CCS 不增用量；多候选不纠正/不新增并有诊断、原总量不变；当前 parser 的事实不进入纠正；旧模型别名原生重播不回退已纠正模型。每条功能回归先确认断言红灯，再实现。采集专用事务复用原写入内核处理非纠正项，整批溢出检查/回滚和 generation 语义保持；不修改 CCS 的分类预演。

必须停止：别名约束不允许同事件保留两个 codex-usage 值；需要表结构/新身份方案/持久计数列；纠正会影响 Claude 或 CCS 日汇总。

### CP05 文档、复核与交付

- `docs/stats-semantics.md` §2.2、§3.2：改正 `token_usage_record` 的语义描述，写明配对规则、线程设置模型与本机实测；§3.2 的「替代方案（未立项）」保留并注明本次没有采用 `response_id`。
- 真实数据复核（只读来源、隔离输出，沿用 `perf_real_data` 的隔离方式或一次性脚本，不写入真实 `~/.tokenscope`）：修复后的解析结果比修复前多出的事件数应为 44，增量应与 §1.3 的合计一致；不一致先归因。
- 版本：统计口径修复，按 patch 递增。合并前 fetch 最新 main 再取号；若 B2（PR #31）先合入，本 PR 顺延取号。准备合入前按 CLAUDE.md 登记上一发布基线。
- 发布说明要提示：升级后 Codex 历史总量会小幅增加（补回压缩用量）；来源日志已删除的部分无法补回。

## 5. 验收

- CP01 全部用例先红后绿；`cargo test --locked --workspace` 两套（普通与 acceptance）通过，clippy、fmt 干净。
- 真实数据复核结果与 §1.3 一致，并写入执行记录（只记计数与合计）。
- 必需检查（`gate`、`native-ui`）与 Install test 全绿后交用户复核，**合并由用户决定**。

## 6. 执行记录

### 起步与 CP01–CP03（2026-10-22）

- 在指定 worktree 从 `docs/codex-compaction-plan` 新建 `fix/codex-compaction-usage`；以 `aed4150` 合入 main `c9de40e`（0.5.9），计划状态提交 `5908f28`。原研究提交截至 `424b30d` 已在祖先链，Claude 的 §15.1 保持不变；不在本分支另写研究内容。B2 结果回填由独立 worktree 继续，B3 的待修订停点不变。
- 清洁基线：原有 Codex 单元测试 12/12。CP01 新增的 11 项在旧实现全部明确断言失败（0 通过 / 11 失败），CCS 并集回归也在原生请求数实际 0、期望 1 处失败；没有编译错误或探针缺失冒充红灯。一致配对、非零不一致和孤立记录的旧守卫与有效压缩串联，核对状态消费/边界后仍能产出下一条压缩；不把旧守卫本来就通过的条件宣称为新红灯。
- CP02 用记录位置捕获的事件挂起到下一条 `token_count`，复用既有 R02/受检算术；仅存在且全零的 last 配对入账。匹配、不一致、顶替、EOF/session 边界的被忽略记录照实计数，已转为压缩的零行不再计 `skipped_zero_usage`。CP03 按行序处理线程设置与 turn_context，忽略无效或空白模型。
- 首轮边界复核追加两例并先红：非法下一条 token_count 不能越过配对、空 payload 的 session_meta 也必须清空挂起记录和上下文。保留用量字段原有严格类型，在解码失败时只识别边界并丢弃挂起；不放宽类型。独立审查另发现缺失/null payload 的新记录必须顶替旧挂起记录；追加第三个边界回归，确认事件数非空的断言红灯后补齐诊断。局部处理后 Codex 26/26（12 旧 + 11 计划 + 3 边界）通过，CCS 两种导入顺序回归及完整并集套件 12/12 通过。其他旧测试和断言未改。
- 本机语料继续写入导致初查为 49 条候选而非历史基准 44 条。只读归因：按事件时间前 44 条仍精确为输入 10,712,650 / 缓存命中 9,532,672 / 缓存写入 0 / 输出 205,483；之后新增 5 条为输入 1,802,736 / 缓存命中 1,777,664 / 缓存写入 0 / 输出 31,263。原预期没有调整；这些是记录配对普查，CP05 仍须用新旧实际解析器复核事件差异。
- 证据位于本 worktree 的 `qa-artifacts/codex-compaction/`：`baseline-codex.log`、`cp01-red-all.log`（11 个具名失败及断言）、`cp01-red-ccs.log`、`cp02-edge-red.log`、`cp02-cp03-green-final.log`、`cp02-green-ccs.log`、`census-before.json`、`census-attribution.json`。只输出真实语料的计数与合计，不写真实 `~/.tokenscope`。

### CP04：旧文件重解析与版本指纹

- `report::PARSE_RULES_REVISION` 2 → 3，进入文件命中键 `规则版本|来源上下文`；这是未变化文件重解析的实际控制方。采集回归先以 v2 登记真实大小/mtime、规范化根/路径和旧正常请求；未改常量时在“实际 1 条、期望 2 条、解析调用数 0”处断言红灯。改为 3 后真实解析一次、补入一条压缩，下一轮解析调用数仍为 1、事件数与 generation 不再增加。
- `history::PARSER_REVISION` 1 → 2。读取方只有新库元数据默认值、实际写入的事件/来源、CCS 新来源和发行指纹；打开/迁移仅看 `HISTORY_SCHEMA_VERSION` 与 `PRAGMA user_version`，没有按 parser 标记迁移、过滤或拒绝旧库。完整旧库的元数据/旧事件/旧来源保留 1，新事件/来源写 2。回归逐字段比较旧事件完整 SQL 行、别名、来源记录、ID 和版本均不变，来源字节/长度/mtime 不变，结构版本不变。
- 实际常量输出的持久指纹为 cache=11 / history=4 / parser=2 / identity=1 / pricing=9 / settings=2。离线选择器核查：登记 v0.5.9 后，本次目标 0.5.10 的三个起点为 0.1.0、0.4.0、0.5.9；parser2 版本发布登记后，后续目标才增加该新代表，预计四个起点、present/deleted 展开八项，不超过六个起点上限。未改选择器、起点上限或既有基线。
- 证据：`cp04-red.log` / `cp04-green.log`、`cp04-ccs-suite.log`、`persistent-formats.json`，以及 `qa-artifacts/codex-compaction-cp04-readonly/selection-projection.json`（离线内存预测，不是假发行资产）。

### CP05：实际解析器只读对照

- 一次性 Rust 程序同时链接修复前普通核心库与修复后 acceptance 核心库，逐文件调用两个实际 `CodexSource::parse_file`；该方法只调用 `read_text`，不走默认根解析、历史库或查询入口。每对解析前后核对同一文件长度/mtime 稳定，没有将真实日志拷贝到磁盘或输出模型、会话、路径及正文，只输出计数与合计。
- 受测基线为 `5908f28` 的产品实现；修复后适配器源码 SHA256 `76b6de887caf813fe8207747dc6ceed5c91ad9453ff10a6acf67b8c99d6df71a`。旧核心库 SHA256 `13250ee21bbbcfb449d28d7429ee0aeb3def6032cd4ff13222517dc1be4a0540`，新核心库 SHA256 `0a91a3c47f32590cbc43875e54876c36f27d8b58ec133f3285ace29a08d2490e`，取证程序 SHA256 `bc04153543c30a4fd6ac56f3fb7e00433b47f56f7d771971be7c7cee55ae8f05`。
- 实测 69 个文件、30,484 条原事件逐字段不变，改写/缺失/采样期间变化文件均为 0；原事件包括一致配对的 token_count，未重复计数。新增原始与全局去重后的增量均为 49 条，去重后总数 26,952 → 27,001；增量输入 12,515,386 / 缓存命中 11,310,336 / 缓存写入 0 / 输出 236,746。其中时间顺序前 44 条与计划原预期逐桶完全一致，另 5 条的合计与上述普查归因一致，未改预期凑数。
- 命令与输出：`rustc --edition=2024 qa-artifacts/codex-compaction/parser-census.rs --extern before=qa-artifacts/codex-compaction/libbefore.rlib --extern after=target/debug/deps/libtokenscope-5437635bfac58fa1.rlib -L dependency=target/debug/deps -o qa-artifacts/codex-compaction/parser-census.exe`；运行该程序得到 `parser-census.json`，退出 0，stderr 为空。程序和产物仅留在忽略的 QA 目录，不入库；未写真实 `~/.tokenscope`。
- `stats-semantics.md` §2.2/§3.2 和适配器头注释已修正单次用量/线程模型规则，保留未立项的 response_id 强身份方案；发行说明须继续提示旧日志仍存在时才能补计。
- 取号前再次 fetch，main 为 `c9de40e` / 0.5.9；本分支经 `upgrade_baselines.py add --tag v0.5.9` 下载核对上一版四项原资产并登记，未改旧项。用官方脚本递增一次 patch 到 0.5.10，`--check --base-ref origin/main` 返回 previous=0.5.9、kind=patch、release=true；发布说明已写“历史总量会小幅增加，已删除来源日志无法补回”。实际离线 `select --current-version 0.5.10` 的起点正是 0.1.0、0.4.0、0.5.9，展开六组，证据 `add-v0.5.9.log`、`bump-version.log`、`version-check.log`、`select-0.5.10.json`。版本仍在本地未提交变更中，最终提交门禁阻断未解决。

### 提交门禁阻断（待用户决定的范围外测试装配）

- 首次 CP01–CP03 普通提交钩子在既有 `collection_status::tests::failure_keeps_latest_generation_and_reports_error` 失败：271 通过、1 失败，位置 `collection_status.rs:481`，比较两个 `last_completed_at` 字符串的 `>=`，未生成提交。原日志 `parser-commit-hook.log` 保留，没有重跑该测试掩盖原因。
- 固定时间探针证明该断言有字符串排序缺陷：jiff 格式化的 `2026-10-22T10:00:00.1Z` → `2026-10-22T10:00:00.100001Z` 是更晚时刻，但字符串 `>=` 为 false、Timestamp `>=` 为 true。原失败没有输出两值，不能据此排除时钟回拨；不宣称已还原原失败的精确时间值。
- 已向用户请求例外：仅把比较两侧解析为 Timestamp、保留 `>=` 语义并输出失败时两值，按测试装配单独提交；超出本计划仅改 Codex/必要常量的范围，批准前不修改该既有断言。探针与拟议补丁留在 `timestamp-order-probe.rs` / `.log` 和 `timestamp-order-proposed.patch`。

### 升级模型归属的阻断（待用户确定旧历史策略）

- 独立复核发现 §3 不变量 2/5 的边界冲突，追加采集层合成回归 `reparsing_thread_settings_model_change_must_not_duplicate_old_usage`。日志为 `turn_context(model-a) → thread_settings(model-b) → token_count`，旧规则已保存 model-a 的一条事实并登记 v2 未变文件指纹；规则 3 重解析后，实际事件数/数据库行数为 2（期望 1），非缓存输入 80 → 160、输出 10 → 20、缓存命中 20 → 40，来源文件字节/长度/mtime 不变。证据 `cp04-model-upgrade-red.log`，失败是防重断言本身，不是编译、缺探针或外部访问。
- 原因：`model.rs` 的 `content_fingerprint` 与 `codex-usage` 别名均包含原始模型；`report.rs` 的原生来源键又包含内容指纹。A 改为 B 后，新键/别名不能命中旧 A；按历史只增不删规则新增 B 并保留 A。此前 CP04 绿色回归与本机 30,484 条不变事实只证明“模型未变”的路径，不能证明这个升级边界通过。
- 已向用户请求确定策略：修订计划，允许在保持四桶和总请求量的前提下纠正旧模型并同步既有来源/别名；或保留旧模型归属、仅新入库请求应用新规则。尚未选择或实现任一方案，没有新增身份方案/表结构或修改历史写入逻辑，没有删除/放宽这个红灯断言。
- Codex 实施、提交和开 PR 停在当前状态，等待范围/策略决定；已推送的最后提交仍为计划状态 `5908f28`，实现、解析常量、回归、口径和 0.5.10 取号均为本地未提交变更。B2 结果回填在独立分支继续，不受此阻断影响。此计划仍在 active，不归档、不宣称完成。

### 用户授权与恢复实施（2026-10-22）

- 用户批准时间断言测试修复，也批准按 §3.5 唯一候选规则纠正旧模型；这是上两项停点的后续决定，撤销此前等待状态。先修订本计划及口径，再补回归、验红和实现；不扩大到新的身份或表结构。
- 已合入 B2 结果回填主线 `c9d6c89`，保留本专项与 §15.1。时间断言独立提交 `7142131`（fix(test)）并推送；普通/acceptance 两套 workspace fmt、Clippy、测试全部通过，其他断言/窗口不变。归因为 jiff 去掉小数秒末尾零后的字符串排序误判；提交正文引用 `timestamp-order-probe.log`，完整复验保存 `timestamp-fix-hook.log`，此前失败日志不覆盖。
- schema 只约束 `(app, identity_scheme, identity_value)`，允许同事件保留旧/新同方案别名；现有 app/time 索引可支持精确候选查询，不需新索引或表结构。采用独立采集事务入口，CCS classify/write/日汇总路径保持原样；新增诊断仅为本轮内存值。
- 真实数据复核须补证：模型纠正预期 0；先前新增五条所在文件的修改时间必须晚于计划核查时点，只输出计数和时间，不输出路径或内容。最终结果与运行结论待后续填写，0.5.10 仍只取号一次，最终 PR 合并由用户决定。
