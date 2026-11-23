# 模型展示名：正式名优先于 ID 抄写

- 分支：`fix/model-display-name-trusted`，自 `origin/main` `61032b2107b2507e02c0b98a6fbb492b51f4ef83`（v0.9.7）。
- 状态：方案记录，先写红测试再改实现。用户明确停在可审查状态：合并、发布、本机安装另行授权。
- 发行版本：仓库工具取号 **patch 0.9.8**（`--check --base-ref origin/main` = 0.9.8 / 0.9.7 / release true）。视图快照 23 → 24 自动重建。

## 1. 问题与证据

本机 `~/.tokenscope/pricing-modelsdev.json`（v4、9578 条）核对：

| 末段 | 目录条目 | name |
| --- | --- | --- |
| `claude-opus-4-7` | `302ai/claude-opus-4-7` | `claude-opus-4-7`（ID 原样） |
| `claude-opus-4-7` | `abacus/`、`aihubmix/`、`anthropic/` 等 | `Claude Opus 4.7` |
| `claude-sonnet-4-6` | `302ai/...`、`sap-ai-core/...` | ID 原样 |
| `claude-sonnet-4-6` | `abacus/`、`anthropic/` 等 | `Claude Sonnet 4.6` |
| `gpt-5.4` | `302ai/gpt-5.4`、`sap-ai-core/gpt-5.4` | ID 原样 |
| `gpt-5.4` | `abacus/`、`openai/` 等 | `GPT-5.4` |
| `gpt-6-astra-cc-format` / `grok-4.6-build` / `glm-5.2-x` | 无该完整 ID 条目 | 无名称记录 |

根因：`Pricing::display_name_for` 只按完整等价 ID 的末段组取候选，过滤 models.dev 层后按 `entry_display_order`（`(display, name, tier)` 字典序）择一。`302ai` 字典序最小且其 name 是 ID 原样，于是盖掉同组正式名。`o3` 的目录 name 本身就是 `o3`（合法短名），`Hy3`、`GPT-5.4` 等短名标点必须保留。

## 2. 不变量与方案

1. **分组与计价零改动**：`Group.key`、`model_stats_key`、`canonical_identity_key`、模型下钻、确认归并、CCS 覆盖、逐条计价、费用 breakdown 全部保持原样。展示名只在 `Group.label` 与图表取标签处消费，不参与任何键、身份或价格判定。
2. **身份匹配不变**：仍按完整等价 ID 的末段组、models.dev 模型级 name 取值；无快照、仅渠道别名、空白名、仅前缀命中仍返回 None，聚合退回原始代表写法（保留现有契约测试）。
3. **名称质量分级**：在候选组内按 name 与条目 ID 的关系分级，而非仅按渠道字典序：
   - 已确认别名/基础键的明确映射（Hy3）最高，沿用现有 `model_label` 固定分支；
   - OAuth/官方完整 ID 或目录内**非 ID 抄写**的正式模型名（含大小写/空格/标点，如 `Claude Opus 4.7`、`GPT-5.4`、`o3`）；
   - **ID 抄写**（name 归一化后等于该条目 ID 末段或完整 ID，如 `302ai/claude-opus-4-7` 的 name）降为兜底，不得盖掉同组正式名。
   - 同一质量级内仍按稳定的 `(display, name)` 字典序择一，目录乱序、新增代理渠道不改变结果。
4. **不凭空取名**：没有可信名称的自定义 ID（`gpt-6-astra-cc-format` 等）保留原始 ID；Preview、日期版本、变体不借用相近模型名称；不引入编译期静态名称表。
5. **展示名三处同源**：聚合表、图表、CCS 日汇总行共用 `Pricing::display_name_for` → `aggregate::model_label`，请求明细与 CCS 原始 ID 详情仍展示原 ID。
6. **升级零迁移**：`SNAPSHOT_VERSION` 23 → 24，旧快照忽略并由首次成功查询重建；筛选偏好、时区、历史库、已导入 CCS、价格快照与索引不改写，无需清库/重导。

## 3. 实施顺序

1. 新增 Rust 红测试：`display_name_for` 对 302ai 前置的合成目录返回正式名；乱序/缺官方/离线兜底；正式短名保留；无可信名保留原 ID；Fable/Hy3 无回归；版本与同名异模不改分组。
2. 修改 `pricing.rs`：引入名称质量比较并在 `display_name_for` 按质量优先择一，保持 `(display, name)` 稳定序。
3. 前端快照 23 → 24，同步测试断言与 `docs/stats-semantics.md`。
4. Rust 全量、前端全链、真实浏览器契约（聚合表/图表/CCS 日行名称一致、下钻命中正确明细、费用用量不变）。
5. 仓库工具取号/基线、正常钩子提交、PR、最新 HEAD 必需 CI 全绿。

## 4. 验收清单与实测（2026-11-24）

- `302ai` 在前时 Opus 4.7 / Sonnet 4.6 / GPT-5.4 显示正式名；目录乱序、缺官方条目与离线时结果确定——`model_display_name_prefers_formal_name_over_id_copy`（先红后绿）、`model_display_name_is_order_independent`、`model_display_name_id_copy_is_last_resort`、`model_display_name_untrusted_id_returns_none` 通过。
- 裸 ID 与已确认渠道别名名称一致；`o3`、`Hy3`、`GPT-5.4`、Fable 无回归；自定义 ID 保留原 ID；Preview/日期/变体不借名——`model_display_name_requires_full_id_match`、`model_identity_contract` 全套与 `confirmed_model_stats` 的 Gemma/Fable/Hy3 用例通过。
- 聚合、图表、CCS 日行名称一致；下钻命中正确明细，费用/用量原值不变——新增 `formal_display_name_shared_by_aggregate_chart_and_rollup` 断言聚合与 CCS 日行同名、明细不写展示名、`reconcile` 守恒通过。
- Rust：`cargo fmt --all --check` 干净；clippy 两种 feature 模式 `-D warnings` 通过；`run_rust_tests.py` 的 default 与 acceptance 两轮全部通过。
  - 本机链接内存限制：首次两轮测试并发链接报 `LNK1104`（与既有实施计划记录同类），按仓库已知处置将构建并发设为 2 后**串行重跑两轮全绿**，未降低任何测试数或阈值；失败轮次的临时根按约定保留现场。
  - 正常提交钩子的 full 轮首次在 `latest_history_opens_without_writer_lock` 失败：硬阈值 1s、实测 1.0196s（超 19ms），该用例与本批改动无关（history.rs 未触碰）；单独连续重跑 3 次均 0.06–0.07s 通过，判定为完整钩子高负载下的偶发时序抖动。按仓库约定不降低阈值，原样重跑完整钩子。
- 前端：`typecheck`、`format:check`、`test`（82 文件）、`build`、`test:e2e`（17 个具名浏览器契约全绿，含 `confirmed_model_stats_grouping` 与 `rollup_drilldown_rows`）全部通过。
  - 首次前端 `test` 与 Rust 全量并行时 2 个真实子进程用例因 forks worker 启动超时失败（测试架资源竞争，非断言/产品缺陷），单独串行重跑全绿；未修改超时或阈值。
- 脚本契约：`python -B -m unittest discover -s scripts -p test_*.py` **415 项通过**。
- `build_inputs.py --check` 本机因脚本清理本地包后访问 registry 卡住（本机代理问题，非产物变更），已停止；本批只改 `.rs`/`.ts`/`.md`，未触及 `include_str!`/构建脚本输入，清单无需更新，CI 全量门禁会在有网环境复核。

## 5. 后续

- 经正常钩子提交、推送 PR，等最新 HEAD 必需 CI 全绿；停在可审查状态，合并/发布/本机安装另行授权。
