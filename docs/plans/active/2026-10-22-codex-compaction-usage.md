# Codex 压缩用量补计与线程设置模型归属

> 状态：**用户 2026-10-22 确认计划文本，实施中。** 本计划是一次独立的统计口径缺陷修复，不占整合计划的责任编号；合并并验证发布后归档，并在计划总账登记。来源：对照 cc-switch v4.0.8 → v4.0.18 的 Claude/Codex 会话适配器变化（研究文档 §15.1），只关注 token 用量；首字时间、生成速度不做。

**Goal：** Codex 远程压缩（compaction）那次请求的用量不再被记成 0；Codex 只通过线程设置事件切换模型时，后续用量归到新模型。

**Architecture：** 两处修改都在 `crates/tokenscope-core/src/source/codex.rs` 的逐行状态机内：
- 新增「待配对的 `token_usage_record`」状态，由下一条 `token_count` 决定是否单独产出事件；
- `event_msg/thread_settings_applied` 与 `turn_context` 一样更新当前模型。

去重、历史库写入、CCS 导入沿用现有的保守重播身份（`会话 | 原始模型 | 四桶`），不新增身份方案。递增 `PARSE_RULES_REVISION`，让已经登记的旧日志重新解析一次，补回已漏计的历史用量。

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
5. **历史只增不删**：重新解析只补新事件；已有事件来源键不变、不被改写或删除；来源文件已删除的旧用量保持原样（这部分漏计无法补回，如实说明）。
6. **诊断如实**：`skipped_zero_usage` 不再包含已转为压缩事件的那些行；被忽略的记录仍计入 `ignored_token_usage_record`。不新增持久计数列；如确需新诊断字段，先在计划里补写再做。
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

（实施时填写。）
