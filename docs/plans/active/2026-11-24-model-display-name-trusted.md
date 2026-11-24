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

## 5. 复核补修：官方优先与完整身份（同批 v0.9.8）

复核基线 `f7b75eed6f9a575ca4672e384cbdf561c3129029`。两个新反例先红后绿：

| 反例 | 修复前实测 | 修复后实测 |
| --- | --- | --- |
| 302ai name=`Claude 4.7 Opus`、anthropic name=`Claude Opus 4.7` | 字典序选中 `Claude 4.7 Opus` | 确认基础模型 `anthropic/claudeopus47` → 官方正式名 `Claude Opus 4.7` |
| `author-a/demo-1`=`Demo A One`、`author-b/demo-1`=`Demo B Two` | 查 B 返回 A 的名称 `Demo A One` | 带命名空间按自身身份得 `Demo B Two`；裸 `demo-1` 保留原始 ID（None） |

- **实现**：`display_name_for` 改为分级——(1) 带命名空间用自身完整身份、裸 ID 用确认映射给出的官方完整 ID 精确匹配；(2) 官方缺名时用同版本渠道兜底，正式名核心形态互不包含判为冲突并保留原始 ID；(3) 名称质量分级（正式名 > ID 抄写）与同级稳定字典序保留。改动仅限 `pricing.rs` 展示名解析。
- **边界补测**：新增排在官方前的转发渠道、目录乱序、官方缺名兜底、裸 ID / 带命名空间 / 无目录；`model_display_name_prefers_formal_name_over_id_copy` 用本机目录形状（302ai ID 抄写 + 官方正式名）确认 Opus 4.7 / Sonnet 4.6 / GPT-5.4 三模型继续显示正式名。`confirmed_model_stats` 的精确逐请求断言确认聚合表、CCS 日行同名与下钻原始 ID。
- **精确逐请求断言**：`formal_display_name_shared_by_aggregate_chart_and_rollup` 改为分别断言——原生 Opus 行 `model == "claude-opus-4-7"`；CCS 日行 `label == "Claude Opus 4.7"` 且 `meta.models == [claude-opus-4-7]`（日行 model 允许统计分组键）。
- **测试期望变更**：`model_display_name_is_order_independent` 原断言裸 `opus55` 在 acme/beta 各有正式名时按字典序选 `Acme Name`；按新语义（无法确认的同末段冲突保留原始 ID）改为期望 `None`，并新增带命名空间的 `acme/opus55`→`Acme Name`、`beta/opus55`→`Beta Name` 断言；顺序无关性不变量保留。
- **未改**：版本仍 0.9.8、快照仍 v24，未重复递增；`Group.key`、下钻、CCS 覆盖与逐条计价口径未动。

## 6. 复核补修验收（2026-11-24）

- Rust 定向：`model_identity_contract` 20 项全绿（新增官方优先、命名空间身份、边界三例）；`confirmed_model_stats` 11 项全绿（精确逐请求断言）；`model_alias_rollup_drilldown`（12）、`model_alias_rollup_review`（7）无回归。
- `confirmed_model_stats` 的 `query_expired` 抖动定位为测试架会话槽竞争：产品 `MAX_ACTIVE_QUERIES=8`，该文件多个用例并行各建查询会话，我最初新增的第三个建会话用例使并行会话数越过临界，稳定淘汰既有的 `legacy_model_migration...` 用例所持有的 `empty` 会话。处置：删除该冗余用例（其验证目标已由 `model_identity_contract` 的本机形状测试与 `confirmed_model_stats` 精确断言完整覆盖），恢复后连续 5 轮全绿。未改产品代码、未降任何阈值、未跳过断言。

## 7. 二次复核补修：先定身份、再比名称（同批 v0.9.8 / v24）

复核基线 `27f453a857cba4dfa94e8f0ce1876fbffd3403ac`。三个缺口先红后绿：

| 反例 | 修复前实测 | 修复后实测 |
| --- | --- | --- |
| Sonnet：302ai ID 抄写 + aimlapi 词序变体 `Claude 4.6 Sonnet` + Anthropic `Claude Sonnet 4.6` | `None`（名称核心互不包含被误判冲突，显示原始 ID） | `Claude Sonnet 4.6` |
| author-a `Demo 1`、author-b `Demo 1 Pro`、author-c 无名称 | `author-c/demo-1` → `Demo 1`（借 A）；裸 `demo-1` → `Demo 1` | 两者均 `None`（保留原始 ID）；author-a/b 仍各自解析 |
| Anthropic 官方 name 为 `claude-opus-4-7`、abacus 为 `Claude Opus 4.7` | `claude-opus-4-7`（直接返回官方 ID 抄写） | `Claude Opus 4.7`（回落已确认渠道正式名） |

- **根因**：上一版用名称核心形态的包含关系推断「是否同一模型」，`Claude 4.6 Sonnet` 与 `Claude Sonnet 4.6`、`Demo 1` 与 `Demo 1 Pro` 的包含关系被误用为身份证据；且带命名空间自身无合格名时错误落入同末段兜底；官方 quality 1 命中后未继续找渠道正式名。
- **实现**：`display_name_for` 改为「先定身份、再比质量」——(1) 身份由命名层已核实映射、统计层固定别名/目录 `base_model` 关系、自身完整 ID 或裸 ID 的唯一 models.dev 身份决定，名称不参与；(2) 已确认基础模型时同末段渠道同池取词，仅自身身份时只含自身条目；(3) 官方正式名 → 渠道正式名 → ID 抄写兜底，同级稳定序。
- **命名层映射**：新增 `DISPLAY_NAME_ALIASES`（`claudesonnet46` → `anthropic/claudesonnet46`、`gpt54` → `openai/gpt54`），仅用于展示名候选集合，不进入 `model_stats_key` / `canonical_identity_key` / 计价 / 下钻 / CCS 覆盖，`Group.key` 与历史库不变。两条均按本机 v4 目录逐条核对官方模型级名称。
- **删除**：`display_name_core` 与 `formal_names_same_model`（用名称包含判身份的错误做法）。
- **边界补测**：目录乱序（Sonnet 正/反序）、缺官方名称（沿用既有用例）、同末段异模（author-a/b/c）、Preview 与日期版本（各按自身完整 ID 解析，日期版本无目录条目保留原文）。

### 7.1 验收

- Rust 定向：`model_identity_contract` 24 项全绿（新增词序变体、无名称命名空间、官方 ID 抄写回落、Preview/日期四例）；`confirmed_model_stats` 11、`model_alias_rollup_drilldown` 12、`model_alias_rollup_review` 7 无回归。
- **本机目录只读复核**（一次性临时测试，跑后送回收站，未写任何用户数据）：`claude-opus-4-7` → `Claude Opus 4.7`、`claude-sonnet-4-6` → `Claude Sonnet 4.6`、`gpt-5.4` → `GPT-5.4`，裸 ID 与官方完整 ID 结果一致。
- 未改：版本仍 0.9.8、快照仍 v24；`Group.key`、下钻匹配、CCS 覆盖、请求数、四桶与逐条计价口径未动。

### 7.2 全量与门禁

- Rust：`cargo fmt --all --check` 与 `git diff --check` 干净；clippy 在 default 与 `--features acceptance` 下均 0 warning；`test default` 88 个测试目标全绿（0 失败），`test acceptance` 88 个目标、1082 个用例全绿（0 失败）。全程无 `LNK1104`、无 `query_expired`、无 panic。本机为 7 CU APU，acceptance 需重新链接全部测试二进制，整轮约 2 小时（`CARGO_BUILD_JOBS=2` 串行）。
- 前端：`typecheck`、`format:check`、`build` 通过；`test` 1221/1222。唯一 1 例 `Settings.test.ts > initial_value_comes_from_settings_get` 在 5s 预算处超时——当时与 rustc 全量编译并行（82 个 worker 各约 1.9s 启动），该文件单独重跑用例耗时 3088ms 并 61 例全绿，判定为资源竞争下的时序抖动；未改超时阈值、未跳过断言。
- 浏览器契约：`test:e2e` 的 17 个具名契约、1193 条断言全部通过，含本批直接相关的 `confirmed_model_stats_grouping`（11 条）与 `model_chart_readable_rows`（121 条）。
- 提交 `14b8c0a` 经正常钩子（19 项全量检查）通过；PR #80 最新 HEAD 的必需检查：`gate`、`rust`、`rust-acceptance`、`contracts`、`frontend`、`install-test`、`native-ui`、`plan-upgrades` 均成功；`upgrade-test` 的 7 个基线（0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.7，各含 `deleted` 与 `present`）14 组全成功；`upgrade-chain` 按设计跳过。合计 21 项检查 20 成功 1 跳过，PR `mergeStateStatus=CLEAN`。
- 顺带修掉一处工具链坑：Git Bash（MSYS）会把命令行参数里的 Windows 反斜杠路径转成混合斜杠，`SHCreateItemFromParsingName` 因此返回 `0x80070057`（E_INVALIDARG）。临时文件送回收站前必须经 `os.path.abspath()` 规范化，并复用 `scripts/run_rust_tests.py` 的 `_send_to_recycle_bin` / `_recycle_bin_holds` 复核 `$I` 条目。

## 8. 后续

- 已推送到 PR #80，最新 HEAD `14b8c0ad6cf7609ef1d79965ef62c1b9aed387e4` 的必需 CI 全绿。停在可审查状态：合并、发布与本机安装另行授权。
