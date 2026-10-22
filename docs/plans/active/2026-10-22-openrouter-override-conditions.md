# OpenRouter 价格同步修复：时间条件字段改为整数后整份解析失败

> 状态：**用户 2026-10-22 确认，含 OR03**。用户 2026-10-22 要求单独写计划、单独开 PR（不并入 TLS 信任修复，即分支 `fix/emudhra-tls-trust` 上的计划 `2026-10-22-emudhra-tls-trust-fix.md`）。本计划是一次独立的缺陷修复，不占整合计划的责任编号；合并并验证发布后归档到 `archive/implemented/`，并在计划总账登记。

**Goal：** OpenRouter 价格同步恢复可用。上游在条件价格（override）里新增或改变时间条件的字段形态时，只跳过带时间条件的那一档价格，不再让整份响应解析失败。

**Architecture：** 修复点集中在 `crates/tokenscope-core/src/openrouter.rs` 的响应结构 `ApiOverride`：时间条件字段只用来判断“存在即跳过”，不参与计价，因此改为只检测是否存在、不约束取值类型，并把 `utc_days` 纳入时间条件。可选的 OR03 把解析失败的影响范围从“整份响应”收窄到“单个模型”。

**Tech Stack：** Rust、serde；`openrouter.rs` 内的单元测试；`tests/pricing_live_sync.rs` 的真实联网用例（随 TLS 修复 PR 引入）。

## 1. 已核实事实（2026-10-22）

- **故障现象**：在 TLS 修复的真实联网验证中，`openrouter::sync` 下载成功，但解析失败：`解析 OpenRouter 响应失败: json: invalid type: integer 0, expected a string`。
- **已影响用户**：本机 `~/.tokenscope/logs` 在 2026-10-12 有一条 ERROR“OpenRouter 自动同步失败”，原因相同；本机 OpenRouter 快照停留在 2026-10-06（457 条）。也就是说，正在使用的版本（v0.5.6 及更早）已经无法同步 OpenRouter 价格。
- **自动同步失败只写日志**：`src-tauri/src/lib.rs:1029-1034` 调用 `log_error`，界面上没有提示，所以这个问题持续了 10 天没有被发现。
- **上游当前的数据形态**：2026-10-22 实拉 `https://openrouter.ai/api/v1/models`，共 468 个模型，其中 78 个带 `overrides`。override 的字段组合如下：

  | 组合 | 条数 |
  | --- | --- |
  | `utc_days`（星期数组）+ `utc_start` / `utc_end`（整数，如 `0`、`100`）+ 价格 | 10 |
  | `utc_start` / `utc_end`（整数）+ 价格 | 4 |
  | 只有 `utc_days` + 价格（没有 `min_prompt_tokens`） | 2 |
  | `min_prompt_tokens`（整数）+ 价格（部分另带 `input_cache_write_1h`、`audio`、`input_audio_cache`） | 76 |

  合计 92 个 override，其中带时间条件的 16 个。

  基础价格字段全部是字符串。
- **代码现状**：
  - `ApiOverride.utc_start` / `utc_end` 定义为 `Option<String>`（`openrouter.rs:74-76`），整数就会让整份 `ApiResponse` 反序列化失败。
  - 这两个字段只在 `sync_with` 中用于判断“存在即跳过并记 warning”（`openrouter.rs:249-251`），不参与计价。
  - 模块文档（`openrouter.rs:28`）和测试 `non_price_fields_and_root_structure_stay_strict`（`openrouter.rs:609` 起）都明确要求“`utc_start` 必须保持字符串语义、整数必须失败”。这条断言与上游的现状冲突，正是本次故障的根源。
  - `utc_days` 不在结构体中，被 serde 忽略。只带 `utc_days` 的 override 目前因缺 `min_prompt_tokens` 而被跳过，但跳过原因写的是“min_prompt_tokens 非法或缺失”。如果以后出现 `utc_days` 加 `min_prompt_tokens` 的组合，会被**误当作无时间条件的长上下文档位**应用，这是一个潜在的计价错误。
  - 现有的模型级隔离只覆盖“单价数值非法”（F04：整个模型被拒绝，其余模型照常）；serde 结构层面的失败仍然会让整份响应失败。
- **models.dev 不受影响**：同次实测成功，共 8778 条。

## 2. 范围与不做

做：
- OR01 先写失败测试。
- OR02 放宽时间条件字段的类型，并把 `utc_days` 纳入时间条件。
- OR03 单个模型结构异常只跳过该模型（用户 2026-10-22 确认纳入）。
- OR04 真实联网验证与交付。

不做：
- 不支持按时间生效的 OpenRouter 价格。带时间条件的档位继续跳过，费用按基础价或其他档位估算，与现在的设计一致。
- 不改快照格式、计价规则和 models.dev 同步。
- 不在界面上提示自动同步失败。这是本次故障没被发现的原因之一，但属于界面功能，建议另行排期。
- 不放宽 `id`、`name` 以及价格以外字段的类型约束（OR03 只改变失败的影响范围，不放宽类型）。

## 3. 必须保持的不变量

1. **时间条件一律跳过**：override 中只要出现 `utc_start`、`utc_end`、`utc_days` 中的任何一个（取值不为 null，类型不限），这一档就跳过并记 warning，原因写明“含时间条件”。它绝不会被当作无条件档位或长上下文档位应用。
2. **其余结果不变**：对不含时间条件的模型和 override，修复前后生成的快照条目与价格逐字段相同（用现有测试的 fixture 证明）。
3. **单价校验不放宽**：价格字段的数值校验（NaN、无穷、负数、非法类型导致整个模型被拒绝）保持原样。
4. **失败不覆盖快照**：同步失败时保留旧快照（现有测试 `failed_sync_preserves_previous_snapshot` 继续通过）。
5. **（OR03）失败面收窄但不静默**：单个模型结构无法解析时，只跳过该模型并记 warning（含模型 id 与原因）。根结构损坏，或者没有任何一个模型能解析时，整次同步失败并保留旧快照，不写出空快照。

## 4. 任务

每项先写失败测试，在未改的代码上确认红，再实现。一个提交只解决一类问题。fixture 手工合成，只模仿字段形态，不复制上游的完整响应。

### OR01 失败测试

在 `openrouter.rs` 的测试模块中追加用例 `time_conditioned_overrides_in_current_upstream_shape_are_skipped`，fixture 含三个模型：
- 模型 A：一个 `utc_days` 加整数 `utc_start` / `utc_end` 的 override，以及一个 `min_prompt_tokens` 长上下文 override；
- 模型 B：只有 `utc_days` 加 `min_prompt_tokens` 的 override（模拟潜在的误用场景）；
- 模型 C：没有 override。

断言：
- 同步成功，快照含 A、B、C 三个模型；
- A 只保留长上下文档位，时间档被跳过并有“含时间条件”warning；
- B 没有任何档位，warning 原因是时间条件；
- C 不受影响。

未改代码时应该在解析阶段整体失败（红）。

### OR02 放宽时间条件字段

- `ApiOverride` 中，`utc_start` / `utc_end` 改为只检测是否存在、接受任意类型（例如 `Option<serde::de::IgnoredAny>`），并新增同样处理的 `utc_days`。时间条件判断改为三个字段任一存在即跳过。
- 更新模块文档第 28 行的边界说明：时间条件字段只检测存在，不约束类型。
- 修改现有断言：`non_price_fields_and_root_structure_stay_strict` 中“整数 `utc_start` 必须解析失败”的断言改为“整数 `utc_start` 可以解析，该档按时间条件跳过”。这是上游事实变化导致的口径修订，在提交说明中写明原因；同一测试中的其余断言（数字 `id` 非法、根结构损坏失败）一律保留。

### OR03 单个模型的解析隔离（用户已确认纳入）

- 响应先解析为 `{"data": [任意 JSON]}`，再逐个把模型解析为 `ApiModel`。某个模型失败时跳过该模型，记 warning（模型 id 取自原始 JSON 的 `id` 字段，取不到就用序号），并计入 `rejected`。
- 没有任何一个模型能解析时，整次同步失败。
- 测试：
  - 一个模型的 `id` 是数字、其他模型正常：同步成功，只少这一个模型并有 warning；
  - 全部模型都非法：同步失败，旧快照保留；
  - 根结构损坏（没有 `data`、`data` 不是数组）：仍然失败。
- 现有断言“数字 `id` 让 `ApiResponse` 解析失败”所在的层级会变化。改为断言“数字 `id` 的模型被拒绝且有 warning”，类型约束本身不放宽。
- **取舍**：这一项能让以后上游任何一个模型的字段变化都只影响那一个模型，而不是让全部价格停止更新；代价是多一层解析，并要改动一条现有断言的层级。不做也能修好本次故障。

### OR04 验证与交付

- 带代理运行 `TOKENSCOPE_LIVE_PRICING=1 cargo test -p tokenscope --test pricing_live_sync -- --ignored --nocapture`，确认两个用例都通过，并记录 OpenRouter 的条目数与 warning 中时间条件跳过的条数。该用例随 TLS 修复 PR 引入，若本 PR 先合并，则在本 PR 中同样加入。
- 版本：递增 patch。若 TLS 修复 PR（0.5.7）先合并，本 PR 更新到最新 main 后递增到 0.5.8；反之本 PR 为 0.5.7，TLS PR 递增到 0.5.8。
- 本地门禁：fmt、两套 clippy 与测试（普通与 acceptance）、`python -B -m unittest discover -s scripts -p "test_*.py"`。
- 开普通 PR，等 `gate`、`native-ui`、Install test 全部成功。**合并由用户决定。**
- 合并后：
  - 核对 main 的 Release 与 v0.5.x 发布；
  - 新版本安装后，在设置页手动同步一次 OpenRouter，确认成功、条目数更新、同步时间刷新；
  - 本计划归档。

## 5. 执行记录

2026-10-22 在分支 `fix/openrouter-override-conditions` 实施：

| 任务 | 提交 | 红 → 绿 |
| --- | --- | --- |
| OR01 + OR02 | `479282f` | 新增 `time_conditioned_overrides_in_current_upstream_shape_are_skipped`，未改代码时失败，报错与生产环境相同：`invalid type: integer 0, expected a string`。改为只检测时间字段是否存在、并纳入 `utc_days` 后通过。变异验证：去掉 `utc_days` 的判断，断言“只带 utc_days 的档位不得被当作长上下文档位”失败。`non_price_fields_and_root_structure_stay_strict` 中“整数 `utc_start` 必须失败”改为“可解析且仍判为时间条件” |
| OR03 | `1deef48` | 新增 `one_malformed_model_is_skipped_without_failing_sync`，未改代码时整体失败（`invalid type: integer 123, expected a string`）；新增守卫用例 `all_models_malformed_fails_and_keeps_previous_snapshot`（修复前后都应通过，用来防止回归）。变异验证：去掉“全部异常即失败”的判断，守卫用例失败。缺失 `id`、数字 `id` 两条断言改为“该元素判为非法”，根结构损坏的断言保留 |
| OR04 联网用例 | `7deaa08` | 加入与 TLS 修复 PR 逐字节相同的 `pricing_live_sync.rs`。带代理实测：OpenRouter 同步成功 461 条；在 468 个模型中，7 个按原有的非法单价规则被拒绝，16 个时间条件档位被跳过（与 §1 统计一致），没有模型因结构异常被跳过。models.dev 8780 条 |
| OR04 版本 | `9080c28` | 合并最新 main 后递增到 0.5.7，`release: true`，基线校验无警告；完整提交检查通过 |
| OR04 改号 | 合并提交 | PR #29（0.5.7）先合入为 `999424d`；本分支合入该 main，`release-change.json` 冲突取 main 一侧后用 `bump_version.py --bump patch` 递增到 0.5.8（`previous: 0.5.7`、`release: true`），锁文件随之带入 #29 的 rustls-webpki 0.103.16 / webpki-roots 1.0.10，联网用例即同时覆盖 TLS 修复；登记 v0.5.7 升级基线后推送 |

偏离与说明：
- 守卫用例的初版 fixture 用了 `"pricing":[]`，实现后它仍然被判为合法模型：serde 允许用数组反序列化结构体，所有字段又都有默认值，于是解析成一个空价格表。这是 fixture 写错了，不是实现的问题，已改为 `"pricing":"not-an-object"`。这一既有行为保持不变。
- `"data":[]`（空数组）维持原行为（写出空快照），不在本次范围内。“全部异常即失败”只在至少有一个模型结构异常时才生效。
- 本分支基于 main，TLS 依赖仍是旧版本，所以联网实测只证明了解析修复；TLS 的验证在 PR #29 中进行。两个 PR 都是 0.5.7，后合并的那个更新到最新 main 后改为 0.5.8。

合并后待验：main 的 Release 与对应版本的发布；安装新版本后，在设置页手动同步一次 OpenRouter，确认成功、条目数与同步时间都刷新；随后归档本计划。
