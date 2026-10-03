# M2：Codex 适配器 + 多 agent 汇总 + 价格表扩充

- 状态：**已完成（2026-10-03）**，验收记录见文末
- 创建：2026-10-03

## 目标

1. 接入第二个 agent——**Codex**（`~/.codex/sessions/**/*.jsonl`），走 M1 已验证的适配器架构，聚合与渲染层零改动（仅新增 agent 维度）。
2. CLI 升级为**多 agent**：默认扫描全部已注册源并合并统计；新增 `--agent` 过滤与 `--by agent` 分组；新增 `--codex-dir`。
3. **价格表扩充 + 最长前缀匹配**：内置表从 7 条扩到约 40 条（快照自 cc-switch `model_pricing`，覆盖本机实际出现的全部模型），费用列从全 unknown 变为基本可用。

## 非目标

- 价格表外置配置文件 / models.dev 自动导入（M3）
- `token_usage_record` 通道解析（见数据事实 1，双计风险，明确不用）
- reasoning token 单列统计（`reasoning_output_tokens` 是 output 子集，M2 仅在 JSON 的 stats 里留计数，不做维度）
- per-session 时区（`turn_context.timezone` 本机全为 Asia/Shanghai，字段存在但沿用仓库统一时区）

## 数据事实（2026-10-03 本机实测：47 文件全量扫描）

1. **token 两路记录，只取 `event_msg`/`token_count`**：24,246 条；`token_usage_record` 仅 126 条且只出现在 2 个文件中，与 token_count 并存（tur-only 文件 0 个），payload 是 `usage`/`turn_token_usage`/`thread_token_usage` 累计回显——两路同取必然双计。M2 忽略 `token_usage_record`（计入 ignored 计数），验收时抽查该 2 文件确认无缺口。
2. **语义**：`total_tokens = input_tokens + output_tokens` 且 `cached_input_tokens ⊆ input_tokens`（23,948/24,246 成立）；`cache_write_input_tokens`、`reasoning_output_tokens`（⊆ output）为独立字段。归一化映射：`input = input_tokens - cached_input_tokens`、`cache_read = cached_input_tokens`、`cache_write = cache_write_input_tokens`、`output = output_tokens`。
3. **278 条零分量行**：`(0,0,0,total>0)`（如 `(0,0,0,19457)`），只升 total 不升分量的占位事件——跳过并计数（`skipped_zero_usage`）。分量非零但 `total ≠ input+output` 的行实测为 0；若有则按坏行处理，不静默入账。
4. **同请求重发（修正 2026-10-03 验收期实测）**：`(timestamp, last_token_usage)` 确实 0 重复，但同一请求的 `last_token_usage` 会被**原样重发**（时间戳不同、数值逐字相同）：`(session, ltu 五元组)` 重复 2,897 组、多余 3,040 行，多数 ×2（流式结束/回合结束各发一次），行距 1~10+ 行不等、`info` 无可区分字段；全部重复在文件内部，无跨文件重复。初版"无重复"结论仅按 `(ts, ltu)` 查重，系探查键选错，作废。
5. **模型归属**：token_count 不带模型；`turn_context`（789 条，11 个模型：gpt-5.4/5.5/5.6-sol/5.6-luna/5.3-codex-spark/6-astra、kimi-k2.5、doubao、mimo、deepseek）带 `model`+`cwd`，按时间序生效；实测 0 条 token_count 出现在文件首个 turn_context 之前。
6. **多 session 同文件**：52 条 `session_meta` / 47 文件；session_id 取 `session_meta.id`，遇新 session_meta 重置当前模型。
7. 所有记录都有顶层 `timestamp`（0 缺失）；项目 = `cwd` basename。

## 不变量（先于代码）

1. **只读**（沿 M1）：对 `~/.codex` 只读；全程不写任何文件。
2. **健壮**（沿 M1）：坏行跳过计数，不 panic、不中断。
3. **Codex 归一化**：仅当分量非零且 `total == input+output` 时入账；零分量行跳过计数；不符按坏行计数。归一化后 `input_noncached = input - cached ≥ 0`。
4. **双计防线**：`token_usage_record` 一律不入账；`reasoning_output_tokens` 不并入 output 之外的任何列；同 `(session, ltu 五元组)` 重发去重**保留首条**（数值逐字相同，首条即请求完成时刻），丢弃计数入 `duplicates_dropped`。
5. **模型归属**：按最后一个 `turn_context` 归属；session 边界重置；无模型可归属的 token_count 计入 `skipped_no_model`，不虚构模型名。
6. **多源容错**：任一 agent 目录缺失 → 该源警告 + 空结果，其余源正常统计；全部缺失 → 空结果退出 0。
7. **时间口径**（沿 M1）：Asia/Shanghai 自然日落日。
8. **计价**：最长前缀匹配（如 `gpt-5.6-luna` 必须先于 `gpt-5.6` 命中）；未收录仍走 unknown 可见（沿 M1 不变量 5）。

## 价格表（pricing.rs 重构）

- 查找算法从"手工顺序线性匹配"改为**最长前缀优先**（`TABLE` 前缀按长度降序或查找时 `max_by_key(prefix.len())`）。
- 新增条目（快照自 cc-switch `model_pricing` 2026-10-03，USD/百万 token，四类）：
  - claude 修订：`claude-opus-4-5` 及以后 `5/25/6.25/0.5`（现表 opus-4 `15/75` 仅覆盖 4 与 4-1）
  - gpt：`gpt-5.x` 家族（5.4 `2.5/15/0.25`、5.5 `5/30/0.5`、5.6 `4/20/0.4/5`、5.6-luna `0.2/1.2/0.02/0.25`、5.6-terra、5.3-codex* `1.75/14/0.175`、5.2 `1.75/14`、5.1/5 `1.25/10/0.125`、mini/nano）、`gpt-6-astra` `10/50/1/12.5`
  - grok：`grok-4.5*` `2/6/0.3`、`grok-4.6*`/`4.7` `2/6/0.5`、`grok-4` `3/15/0.75`、code-fast `1/2/0.2`
  - deepseek：`v4-flash*` `0.3/1.2/0.006`、`v4-pro` `1.32/3.96/0.044`、`v3.2` `0.28/0.42/0.028`、`v3.1` `0.55/1.67/0.055`
  - 其他本机出现：`kimi-k2.5` `0.6/3/0.1`、`doubao-seed-2-*`、`mimo-v2.5*`
- 测试固化：luna 与 5.6 分层、opus-4-5 与 opus-4 分层、deepseek-v4-flash-0731 命中、unknown 仍返回 None。

## CLI（M2 面）

```
tokenscope summary                          # 合并全部已注册源（claude + codex）
tokenscope summary --agent claude           # 只统计指定 agent（claude|codex）
tokenscope summary --by agent|day|model|project
tokenscope summary --days N --json          # 沿 M1
tokenscope summary --claude-dir <p> --codex-dir <p>   # 覆盖发现根目录
```

JSON 报告调整：`agent` 字段改为 `sources: [{agent, stats}]`（逐源采集统计），groups 增加可选 `agents` 字段（该分组涉及到的 agent 列表）。表格脚注按源拼接统计。

## 任务清单（代码位置 / 测试名 / 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | 最长前缀查找重构 + 价格表扩充 | `src/pricing.rs` | `test_pricing_`（新增 luna/opus 分层、v4-flash 命中） | `cargo test pricing` |
| 2 | `AgentKind::Codex` 与 model 层扩展 | `src/model.rs` | `test_usage_event_` 回归 | `cargo test model` |
| 3 | Codex 适配器：发现（递归 YYYY/MM/DD）、session 边界、模型归属、归一化、零分量跳过、坏行 | `src/source/codex.rs` | `test_codex_discovery_` / `test_codex_parse_typical` / `test_codex_multi_session` / `test_codex_model_attribution` / `test_codex_zero_usage` / `test_codex_badline` / `test_codex_missing_dir` | `cargo test codex` |
| 4 | 多源 CLI：默认合并、`--agent`、`--by agent`、`--codex-dir`、sources 统计 | `src/cli.rs`、`src/aggregate.rs` | `test_summary_flags_`（扩展）、`test_aggregate_by_agent` | `cargo test cli aggregate` |
| 5 | 渲染：sources 统计数组与表格脚注 | `src/render/` | `test_render_` 回归 + sources 断言 | `cargo test render` |
| 6 | e2e：codex fixture + 双 agent 合并 | `tests/e2e_codex.rs`、`tests/e2e_claude.rs` | `test_e2e_codex_` / `test_e2e_multi_agent_` | `cargo test` |

## 验收

1. `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 全绿。
2. 真实 `~/.codex`：Python 独立复算（同口径）逐组核对按日/按模型数字；坏行 0 或逐条可解释；278 条零分量行全部计入 skipped。
3. **cc-switch 对照**：codex 平面重叠日期与 `usage_daily_rollups` 对比（注意 cc 的 input 列为排除缓存口径，与本工具归一化后 input 同口径），差异归因写回归档记录；含 `token_usage_record` 的 2 个文件抽查 session 级覆盖无缺口。
4. 只读不变量：`~/.codex` 冻结副本运行前后快照一致。
5. `--agent claude` 输出与 M1 完全一致（回归）；单源缺失时另一源正常。

## 风险

- Codex CLI 版本演进字段变化 → serde 宽松解析 + 坏行计数（沿 M1）。
- cc-switch codex 口径若实为"含缓存 input"（与其 `input_token_semantics` 编码有关），对照差异会成比例放大 → 验收时先用单日反推验证口径再全量对比，结论写回归档。
- 价格快照滞后于牌价调整 → 表内注明快照日期，外置配置在 M3 解决。

## 验收记录（2026-10-03）

1. `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 全绿（31 lib + 2 claude e2e + 4 codex e2e）。
2. 真实 `~/.codex`：47 文件 / 151,750 行 → 23,947 事件（同请求重发去重丢 3,012、坏行 20=旧版 CLI 写的缺 `info` 的 token_count 行、零分量跳过 279、无模型 0、忽略 usage_record 126）；Python 独立复算按日 45 组、按模型 8 组逐项一致。
3. **cc-switch 对照（本里程碑关键修正）**：初版实现按计划"无重复"口径，与 `usage_daily_rollups` 仅 6/39 日期一致且差异呈精确 2 倍；实测发现 Codex 会**原样重发**同一请求的 `last_token_usage`（`(session, ltu)` 重复 2,897 组、值逐字相同、时间戳不同），计划数据事实 4 与不变量 4 当场修正并实现去重保首条；修复后 **38/39 日期五项指标（请求/输入/输出/缓存写/缓存读）完全一致**，口径反推日 2026-03-14 三项 token 精确相等，确认 cc 的 input 列即剔缓存口径。唯一残差 2026-06-09（in 差 4.23%），归因同 M1：cc 保留了其后被轮转/改写的源文件历史。本工具数据比 cc 多 6 个日期（cc 未再同步）。
4. `token_usage_record` 覆盖抽查：126 条全部与 token_count 共存，无 tur-only session，忽略它不丢数据。
5. M1 回归：`--agent claude` 采集统计逐项等于 M1 记录（19 文件/30,743 行/3,741 事件/去重 8,620/坏行 0），单 agent 报告不出现 `agents` 字段。
6. 只读不变量（`~/.codex` 冻结副本快照一致）、缺失目录警告+退出码 0、多源合计=单源之和，全部通过。

## 遗留与后续

- 价格表为静态快照（2026-10-03，取自 cc-switch `model_pricing`），牌价调整需手动更新；外置配置与 models.dev 导入留给 M3。
- 请求计数口径与 cc-switch 不同（本工具 = token_count 事件数，cc = 其自身请求定义），对照以 token 四类总和为准。
- Codex `reasoning_output_tokens` 仅随 output 计入，未单列；`turn_context.timezone` 本机恒为 Asia/Shanghai，多时区支持留待需要时立项。
