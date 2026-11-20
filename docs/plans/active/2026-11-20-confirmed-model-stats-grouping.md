# 已确认的同模型渠道与计费档合并展示

- 分支：`fix/confirmed-model-stats-grouping`，自 `origin/main` `f0fc12eb059fcbfb12838bc4505954bdc68cfca5`（v0.9.6、视图快照 v22）。
- 状态：本地实施及完整验收完成，准备正常钩子提交与 PR 审查；不合并、不发布、不安装。
- 发行版本：仓库工具取号 **0.9.7**；已核对并登记 v0.9.6 原发布资产作为上一版升级基线。基线选择为 0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 / 0.6.0 / 0.9.6（present/deleted 共 12 组）。

## 1. 问题与不变量

用户确认的两种来源：WorkBuddy 原生 `hy3`；Claude Code 已导入的 CCS 日汇总 `tencent/hy3:free`。目标是让它们与 `tencent/hy3` 在模型表格及图表共用一行一柱 **Hy3**，但明细仍可追溯各自 ID。

1. 分组只改变统计展示，不意味着请求、计价候选或 CCS 覆盖证据等价。历史库、导入和去重键、覆盖规则与时间边界、逐条价格及来源费用的适用顺序不变。尤其不能用普通档的请求去覆盖免费档日汇总。
2. 聚合表与图表共用后端 `Group.key` 与 `Group.label`；模型下钻、采用的 CCS 日行分组、游标模型分量共用冻结的统计分组键。被覆盖请求按旧规则标记；日行仍只从三向选择后的采用集构建，合并后保留原始 ID 列表。
3. 每条请求与每条采用的日汇总先按原始 ID、适用费率或 CCS 来源费用计价，再累加。已知零价、已知非零费用与真实缺价分别披露；未定价 token 不因分组而消失。不用新分组键查价，不用合并后的 token 重新估算。
4. 不以目录候选数量、展示名、近似字串、价格相同或后缀本身当证据。未确认同名异模、Preview、日期版本、有独立模型语义的变体不并入基础版。查询不联网；确认过的 Hy3、Fable 无快照仍归并。

## 2. 调用方与方案

| 调用链 | 模型键 |
| --- | --- |
| `collect_detail_buckets` 的汇总/历史身份/明细三处、`select_rollup_contributions`、`AggregateState` 覆盖跳过、`build_event_row` 覆盖标记 | 原 `canonical_identity_key`，保留 v0.9.4 以来的桶证据（包括既有 Fable 别名）；不引入新目录或计费档规则 |
| `AggregateState` 的模型维度（原生、CCS 采用贡献）、`build_rollup_drill_index`、`query_events_from` 的模型过滤、`drill_fingerprint` | 独立 `model_stats_key` |
| `chartData.ts`、表格、点行下钻 | 直接消费后端分组，不在前端另算身份，也不按标签去重 |

- **目录**：models.dev [API 文档](https://github.com/anomalyco/models.dev#api-reference) 明确由 `base_model` 导出 `canonical_model_id`，指向提供方无关的 `lab/model`。普通同步保留该可选元数据；既有快照及价格索引不要求重建，读到的目录在查询价格修订中冻结。率值、候选、价格索引版本不变。只有完整目录 ID → 明示的基础 ID 才能确认；对 models.dev 的 OpenRouter 条目，同时认其确切的 OpenRouter 路由 ID（`openrouter/` 是目录包装）。版本保护：目录条目与目标须保留完整型号末段等价键，仅目录明示的 `:free` 允许作为计费档差异，不能丢掉 Preview、日期或其他限定；缺失、无效或冲突证据不推断。基础键仍可作为下钻参数，不因目录乱序而变化。
- **明确映射**：核实的 `hy3` / `tencent/hy3` / `tencent/hy3:free` → `tencent/hy3`（显示 **Hy3**）；Fable 延用 `claude-fable-5-2` / `anthropic/claude-fable-5.2`。保留已核实 Opus 4.7 的裸 ID 与 Anthropic ID 的下钻回归；不会把未知渠道的同名 ID 并入。仅完整结构键命中，禁止通用前缀或变体回退。
- **另一组真实目录证据**：models.dev `openrouter` 下 `google/gemma-4-26b-a4b-it` 与 `google/gemma-4-26b-a4b-it:free` 的 `canonical_model_id` 均为 `google/gemma-4-26b-a4b-it`；OpenRouter 当日两条目录项的 `canonical_slug` 同为 `google/gemma-4-26b-a4b-it-20260403`。测试冻结最小条目，不依赖联网；后者不是用来归并日期版本的规则。
- **升级**：只递增 `SNAPSHOT_VERSION` 一次（22 → 23）。旧视图快照忽略，首次成功查询重建；筛选偏好、时区、已导入 CCS、历史库、价格输入均不因升级重写。取号与上一版基线走仓库工具。

## 3. 验收顺序

1. 先在隔离目录以合成 WorkBuddy 日志和已导入的 CCS 日汇总复现同名两行；记录修复前结果后再改实现。
2. 在同一冻结查询核对合并后的下钻请求数、四桶、费用及原始 ID；增加有价、显式零价、真实缺价、来源费用和同应用不同计费档覆盖边界对照。应用/日期筛选、同日多贡献与原生/CCS 跨页续用的游标均验证。
3. 以 Gemma 的实证字段验证可复用性，另测多渠道乱序、无元数据或无快照、同名无证据、Preview、日期和语义变体。复跑 Fable 两向覆盖、Opus CCS 两日下钻。
4. 前端旧快照回归和真实 Chromium：表格一行、ECharts 一柱、两侧可下钻、费用及可访问的来源 ID；通过格式化、Rust 全量、前端及真实浏览器门禁。
5. 正常 hooks 提交、推送独立分支、开 PR、核对最新 HEAD 的必需 CI；回填实测数字与证据，不进行合并、发布或安装。

## 4. 实施与阶段证据（2026-11-20 → 2026-11-21）

- 红-first：`confirmed_model_stats.rs` 在主线实跑为 **2 组**：`tencent/hy3:free`（3 请求）与 `tencenttokenhub/hy3`（1 请求），标签均为 Hy3；期望 1 组失败。修复后 **1 组 Hy3**，4 请求、四桶 4000 / 800 / 90 / 300，费用 0.0031（请求估算）+ 1.25（来源）= **1.2531**；下钻同时返回原始 `hy3` WorkBuddy 请求及 `models=[tencent/hy3:free]` CCS 日行。
- 复杂离线对照先逐记录计价：普通档请求 0.00294、WorkBuddy 请求 0.0031、显式免费请求 0、实际缓存写全缺价请求 150 token 仍未知；当日来源费用 1.25，前一日普通/免费两条采用贡献先各计价再合成日行 0.00174。组总计 **11 请求，四桶 6000 / 1330 / 255 / 420，已知小计 $1.25778 + 缺价 150 缓存写 token**。7 行下钻（5 请求 + 2 日行）扣除被覆盖的一条后逐项一致；同应用普通档没有被免费档日汇总覆盖，普通档未采用的 $99 日汇总不出现在下钻。
- 7 页 `limit=1` 轮换 `hy3` / `tencent/hy3` / `tencent/hy3:free`，原生及日行游标连续无重漏，日期或未确认模型换筛选时拒绝旧游标。WorkBuddy 当日、Claude 当日及 Claude 前一日筛选分别对账通过。
- Gemma 第二组：正常 models.dev 同步保留真实 `canonical_model_id` 形状，普通、免费、DeepInfra 三条请求共一组，逐请求费用 **0.003 / 0 / 0.003**（费率为合成对照）。目录乱序不影响结果；同名无关系的渠道及裸末段不合并，无快照时这一组保守保持结构身份。Preview/日期/`:thinking`/空变体/无效字段/冲突与链式关系负例通过。
- 前端红-first：v22 的拆分 Hy3 快照仍恢复成首帧（失败），实际全缺价请求的金额触发器显示 `$0.00`（失败）；新回归先红后改为 v23 拒旧、真实缺价显示「未知」、显式免费保持 `$0.00`。原有混合费用明细/可访问交互保留。
- 真实浏览器首轮的表格、实际 SVG 四段几何、分页和费用状态均通过，但 CCS 模型提示在真实键盘聚焦后未打开（原 `NTooltip` 只接默认 hover）。据此局部补齐日行模型提示的 hover/focus 与 Escape，并让可访问名称始终包含原始 ID 和来源时区；不改变表格列或常驻名称。
- 编译与执行阻碍如实保留：全量首次编译发现一处测试构造缺新增可选字段，补齐；下一轮 Windows 报 `os error 1455` / `LNK1102`（分页文件/链接内存不足），只将本轮构建并发降至 2，不改产品输入、测试数或阈值。阶段定向 Rust 4 例、前端 147 例通过；最终全量及新增冷启动索引回归以以下交付记录为准。
- 证据目录：`qa-artifacts/confirmed-model-stats/`（忽略入库）；测试失败临时根按仓库规则保留，成功轮次由统一入口核实进入回收站。
- 额外编排问题：独立调用 `build_inputs.py --check` 会先清本地包，和正在进行的 Rust 构建并行时引发指纹目录缺失。已中止该检查并停止并行 Cargo 构建；后续检查串行、构建并发 2。前端全量首轮另有两条 Settings 用例 5 秒超时（同时存在高并发构建），尚不能记为通过；最终串行完整钩子结果单独记录。未降低超时或几何阈值。
- 主线基线核实：`f0fc12e` 的 Release 及 Native UI 成功，但 main CI 的两条分段动画末帧验收失败，维持历史披露，不当作本轮全绿证据。

## 5. 本地全量验收

- `cargo fmt --all --check` 干净；workspace default clippy `-D warnings` 通过。
- `scripts/run_rust_tests.py -- cargo test --locked --workspace`：**1047 passed / 88 个成功测试目标，22 ignored，0 failed**；临时根 `tokenscope-test-s603a5h1` 已由统一入口送入回收站并核实。包含本轮 5 个统计分组用例、新进程价格索引恢复、Fable 两向覆盖、Opus CCS 两日下钻与按需计价回归。
- 全量过程中调整了三类已过时的断言：Hy3 免费档现在使用基础展示组；没有明确目录关系的裸名不再借唯一候选改写为 `acme/…` 等渠道键；旧“放入任意唯一快照即合并”夹具改为提供明确的基础模型关系。原有请求数、四桶和费用断言均保留。
- Chromium **148.0.7778.96** 定向契约 `confirmed_model_stats_grouping`：**11 条断言通过**，包含 light/1280×820、dark/980×620 的一行一柱实际 SVG 四段几何、跨页原生/日行、四种费用状态与键盘追溯 CCS 原始免费档 ID；截图人工回看无溢出。完整前端链及正常提交钩子结果待下节回填。
- 前端完整链 `typecheck → format:check → test → build → test:e2e` **EXIT=0**：81 文件 / **1212 测试通过**；normal 明暗 × 1280×820 / 980×620 四场景无违例；所选 **16 个具名契约全通过**，包括既有 Fable/Opus 下钻与本轮 Hy3 契约。未修改分段控件或它的动画验收阈值，不以历史 main 的失败结果充当通过。
- 脚本契约 `python -B -m unittest discover -s scripts -p test_*.py`：**415 测试，OK**。其负例会输出有意的阻断诊断，原日志保留；不据此声称人工系统验收已完成。
