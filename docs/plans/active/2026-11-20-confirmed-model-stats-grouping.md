# 已确认的同模型渠道与计费档合并展示

- 分支：`fix/confirmed-model-stats-grouping`，自 `origin/main` `f0fc12eb059fcbfb12838bc4505954bdc68cfca5`（v0.9.6、视图快照 v22）。
- 状态：初始修复 `d25993e`、两处复核补修 `12f8579` 与分段控件末帧采样修复 `fa5e657` 均已正常钩子提交；PR [#78](https://github.com/ywltby/tokenscope/pull/78) 最新 HEAD `fa5e657` 的必需检查全绿。**停在可审查状态**：不合并、不发布、不安装。
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

## 6. 独立复核补修：旧模型下钻偏好与目录校验（2026-11-21）

- 起点核对：上一轮正常完整钩子已结束并通过，形成 `d25993e3ad03d8f2baa703bb6a9534162f57a751`；开始补修前工作区、暂存区干净，尚无 PR。沿用 **0.9.7 / 视图快照 v23**，不重复取号。
- **旧下钻偏好**：为 `tokenscope-view-filters` 的模型键增加独立修订标记（`modelKeyVersion=1`）。只有旧偏好的模型下钻，才在首次 `query_events` 显式请求迁移。后端使用同一冻结价格表及只读历史事务中的可见模型关系，按升级前的键找到候选新统计组：唯一则查询新键并返回迁移结果；缺失或歧义则仅清模型下钻，返回原主筛选下的未下钻明细。前端在同批结果落地时更新该条偏好并标记完成，不重复请求、不得被旧响应覆盖。原有来源、维度、时间预设、日汇总模式、时区不变。
- 关系检查跨越当前日期范围，但使用当前查询冻结的历史与价格，不读取查询后新增的数据；因此「键可确认但当前日期无请求」保留有效筛选，不与失效混为一谈。兼容路径不接入日常模型分组/匹配、逐条计价或 CCS 覆盖；新保存的偏好及第二次启动不再进入该路径。
- **目录校验**：`ModelStatsGrouping` 改为消费 `modelsdev::parse_snapshot_text` 已通过校验和旧格式兼容处理的 `Snapshot`，不再另设宽松的原始文本解析器；冷加载和索引恢复均使用同一入口。整份快照无效时仅保留明确映射；可选 `canonical_model_id` 的单条格式错误仍只忽略该字段，合法费率不丢失。
- 红-first：合成 Space Bunny 请求 + 预存 `opencodego/spacebunny`，先证明旧筛选首次打开 0 行、新键 1 行；无效 `v="nonsense"` 先复现有加载警告却仍合并 Gemma。随后覆盖偏好歧义/缺失回退、二次启动、新偏好、空日期范围、冷加载/索引恢复、可选字段错误及旧格式。既有 Hy3 离线、费用、CCS 覆盖、下钻分页和浏览器契约保留。
- 本节结果与 PR/最新 HEAD 门禁在完成后追加；本次仍不合并、不发布、不安装。

### 6.1 红 → 绿结果

| 复核点 | 修复前实测 | 修复后实测 |
| --- | --- | --- |
| 合成 Space Bunny + 旧偏好 `opencodego/spacebunny` | 首次打开 **0 行**；直接新键 `spacebunny` **1 行** | 显式迁移首页 **1 行**，偏好改用新键并保存 `modelKeyVersion=1`，不需重新点表格 |
| 缺失 / 歧义关系 | 缺少旧偏好的兼容回退（本轮补充的边界） | 仅清模型下钻，原主筛选下的明细正常加载；来源/维度/时间/日汇总模式/时区不改 |
| 空日期与二次启动 | 旧键和空日期难以区分 | 关系在同一冻结历史中确认，当前日期无请求仍保留新键；二次启动和新偏好直接严格匹配，不重复兼容 |
| 无效 `v="nonsense"` 冷加载 | 有 models.dev 加载警告，但免费档键仍被折叠到 Gemma 基础键 | 警告保留，普通/免费档各保留结构键，明确 Hy3/Fable 映射照常 |
| 同一无效文件的价格索引恢复（独立进程） | 命中索引、仍错误合并 | 命中既有索引但不采用无效文件目录别名，结构键保持分开 |
| 有效目录 / 单条可选字段错误 / 旧格式 | — | 有效 Gemma 普通/免费档仍合并；单条坏别名不丢费率；v1 缓存零值降级及 v1–v3 名称降级保持 |

- Rust 定向 **33 例通过**：本轮 `confirmed_model_stats` 10 例，加 Fable/Opus、采用贡献下钻与按需计价 23 例。迁移后的首页游标用新键正常续页，旧游标不能请求迁移；新增历史不会改变此前冻结查询的关系。Hy3 离线费用 $1.25778 + 150 缺价 token 的原对账不变。
- 前端偏好/迁移状态 **16 例通过**；另已复跑 Dashboard 与查询编排相关四文件 **134 例通过**。包含二次启动、新偏好空范围、旧响应晚到不覆盖用户新选择。一次构建发现合成夹具 `days=7` 的字面量类型过宽，已修正夹具类型，未改产品逻辑或阈值。
- 类型检查及 build 通过；真实 Chromium 三项相关契约全部通过：`saved_model_drill_migration` **10**、Hy3 分组 **11**、Fable/Opus 日汇总下钻 **19** 条断言。首次打开截图已回看，Space Bunny 原始请求与有效模型筛选同屏；缺失/歧义回退和页面重启均未手工点击模型表格。
- 红/绿与截图证据：`qa-artifacts/confirmed-model-stats/{model-preference-red,snapshot-validation-red,review-migration-green,review-core-regressions,review-browser}.log`、`review-browser/`（忽略入库）。
- **最终完整钩子与 PR 门禁**：
  - 产品提交 `fa5e657` 的正常完整钩子通过：default workspace **1052 passed / 88 目标，0 failed**、acceptance **1071 passed / 88 目标，0 failed**；前端 **82 文件 / 1222 例**；浏览器 17 个具名契约全绿（含 `segmented_cold_start_highlight` 29、`saved_model_drill_migration` 10、`confirmed_model_stats_grouping` 11、`rollup_drilldown_rows` 19）；脚本契约 415 通过。
  - 首次远端 `frontend` 在 `12f8579` 失败：`segmented_cold_start_highlight` 的方向键切换末帧采样停在 `183.2532px`（目标 `183px`，差 **0.2532px** > 0.25px），与本批统计无关。修复只让探针在 320ms 等待后**同步读取真实终态**；0.25px 阈值与中间帧要求未放宽。补修后在四项组合上该契约 29 条断言全通过，正常钩子整链通过，提交 `fa5e657`。
  - PR [#78](https://github.com/ywltby/tokenscope/pull/78)（`fix/confirmed-model-stats-grouping` → `main`）最新 HEAD **`fa5e657f6511deee710a8051d536e8c9b3cb463c`** 的必需检查全绿：[CI](https://github.com/ywltby/tokenscope/actions/runs/45150878718) rust 6m30s / rust-acceptance 4m10s / contracts 8m28s / frontend 9m29s / **gate pass**；[Install test](https://github.com/ywltby/tokenscope/actions/runs/45150878653) install-test 10m24s、12 组单跳升级（含 0.9.6）全 pass、plan-upgrades pass；[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/45150878616) native-ui 11m30s。
  - 本地 `bump_version.py --check --base-ref origin/main` = **0.9.7 / patch / base `f0fc12e` / release true**。
