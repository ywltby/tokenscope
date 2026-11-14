# v0.9.1 修复计划：隐藏页图表重建缩成 100px、费用列样式失配、来源费用与未知混排

> 创建：2026-11-14。分支 `fix/v090-chart-cost-fee`（基于 main `e9f5c33`）。
> 授权范围：本批三个缺陷的红测→最小修复→测试→文档→版本→提交→PR。不自动合并、不发布、不在本机安装正式包。
> 基线：`v0.9.0`（已发布、升级基线已在 PR #67 登记——`scripts/fixtures/upgrade_baselines.json` 含 v0.9.0 条目）。版本用工具分配 patch：`0.9.1`。

## 6. 交付状态

- **红测（先红）**：TrendChart 5 例（T-A1 隐藏期 dispose、T-A2 隐藏期 init 3 次、
  T-A4 缩放丢失）、CostCell 组件缺失（加载失败）、Rust `cost_detail` 字段
  编译期 9 处 E0609——全部在旧实现下失败取证后才动手修复。
- **绿·缺陷 A（TrendChart.vue）**：`containerMeasurable()`（clientWidth /
  offsetParent 判据）+ `pending` 合并（rebuild > data/dimension > layout，
  rebuild 快照缩放范围）+ `maybeRestore()`（恢复时按**当下真实容器尺寸**
  init/重建，init 显式传 width/height；无 pending 时核对 ECharts 实际输出
  `getWidth/getHeight` 失配即 resize 校正、不发 setOption 不打断动画）+
  `onActivated`（KeepAlive 回页，rAF 两帧等布局）与 ResizeObserver 回调守卫。
  隐藏期间不再 dispose/init——主题+语言连续变更只记一次 pending，恢复时一次
  重建。旧断言更新 2 处：init 现在显式传尺寸（`{renderer:"svg"}` → 逐字段）、
  happy-dom 无布局下 width 为 undefined（真实容器宽度由浏览器契约断言）。
  **TrendChart 32 项全过（含新增 5 项）**。
- **绿·缺陷 B（CostCell.vue 新组件）**：费用列从 UsageTable render 回调抽成
  独立 SFC——组件节点天然带 scope 标记，scoped CSS 命中（真实浏览器 computed
  display 从 inline 变为 (inline-)flex column，主金额与来源行上下排列）。
  UsageTable 费用列 render 改为 `h(CostCell, { group })`；旧组件内的
  .cost-cell 死样式删除；UsageTable 测试 3 处 vnode 解析 helper 改走真实
  挂载渲染。**CostCell 10 项 + UsageTable 31 项全过**。
- **绿·缺陷 C（aggregate.rs cost_detail）**：`Group.cost_detail`
  （rollup_only / mixed / 纯明细缺省不序列化）；喂入时序为明细先、汇总后——
  明细占位 `"detail"`（finish 时清回 None，JSON 逐字节兼容），汇总到来时
  `None → rollup_only`、`"detail" → mixed`；totals 按"任一 mixed → mixed"
  聚合。前端 CostCell / SummaryCards 按字段分流（规则 1-4）；locale 新键
  `sourceCostTag`（来源记录 / Source recorded）。**Rust
  ccs_rollup_source_cost 16 项全过（新增 5 项）+ SummaryCards 24 项全过**。
  喂入顺序 bug 一次：首版 rollup 侧覆盖把 mixed 覆盖回 rollup_only（B 类桶
  红了 T-C2），改为按既有值升级后转绿——红测先行的价值。
- **契约（真实浏览器）**：`source-cost` fixture（四组：rollup+7.25 /
  rollup+0 / mixed / 纯明细缺价）+ 两条新契约注册进 REQUIRED_CONTRACTS 与
  CI e2e runner 白名单：
  - `cost_cell_layout_not_concatenated`（9 断言 × 明暗 × 两尺寸）PASS：
    scope 标记全命中、computed (inline-)flex column、规则 1-4 逐组断言
    （含 mixed 组 mainTop/sourceTop 几何分离 > 4px）、费用列不撑坏表格；
  - `summary_chart_survives_hidden_rebuild`（10 断言 × 明暗 × 两尺寸）PASS：
    汇总→设置→切深色主题→回汇总后 SVG 宽 ≥ 容器 × 0.95（修复前 = 100px），
    远离 100px 缺陷特征。
  截图逐张查看：`qa-artifacts/v091-fix/{cost-cell,cost-cell-2,chart-rebuild}/`。
  契约调试修正 3 处：日维度 key 重映射 → 契约切按模型维度；fixture model
  维度保留 synthetic-* 身份键；display 断言放宽为 (inline-)flex（语义相同）。
- **全量门禁**：typecheck=0、format:check=0、前端全量 **1177 项**（78 文件）
  全过、build=0、e2e runner（11 契约 × 明暗 × 两尺寸）全过；
  `cargo clippy --locked --workspace --all-targets` 干净；Rust 全 workspace
  测试（`CARGO_BUILD_JOBS=2` 串行，规避本机并行编译内存不足 os error 1455）
  全部 `test result: ok`、0 failed；`build_inputs --check` 与版本检查
  `bump_version.py --check --base-ref origin/main` 通过（0.9.1，patch，
  release: true）。
- **版本**：`0.9.1`（patch，工具分配；基线 0.9.0 = 已发布版，升级基线已在
  PR #67 登记）。`docs/stats-semantics.md` §5.1 增补 `cost_detail` 口径。
- **未做**（按授权边界）：不合并、不发布、不本机安装；不改真实
  history.db/价格/设置；视图快照版本不递增（字段向后兼容）；AC10 ENOENT、
  /btw 等独立项不动。

## 0. 问题与根因（已由用户与 qa-artifacts/v090-installed-diagnosis 独立确认）

### A. 隐藏页面重建导致图表缩成 100px

- 汇总页与设置页都由 `MainApp.vue:303` 的 `<KeepAlive>` 保留；汇总页隐藏时 `TrendChart` 组件不卸载、DOM 仍在文档外层但不可见。
- 在设置页切主题/语言触发隐藏图表的 `rebuildForTheme()`（`TrendChart.vue:338-348` / `385` / `388`）：`echarts.init` 在**不可测量**的容器上初始化，实测宽高为 0 → ECharts 回退 100px 默认。
- `applyHeight()`（256-265）返回 `el.clientWidth || appliedWidth`——把**旧宽度**当本次尺寸记入 `appliedWidth/Height`；回到汇总页后 ResizeObserver 比较「旧宽 == 旧宽」直接跳过 resize（`353-359`），错误画布永久保留。
- 证据（findings.md）：宽 1030 容器 SVG=100px；窄 754 同样；语言切换同路径复现；控制组（隐藏期 resize，不重建）正常。
- 修复方向（用户已定）：
  1. 隐藏容器不 init/不重建；隐藏期间只记录待应用的最新状态（pending）。
  2. 页面重新可见、布局完成后按**真实容器尺寸**恢复，并核对 ECharts 实际输出尺寸（SVG），不能只信缓存。
  3. 保留 dataZoom 范围、最新筛选、主题、语言；避免重复 resize 打断动画（沿用 UX05 单批 flush 纪律）。
  4. 不用 CSS 拉伸掩盖；不改 token 数据。

### B. 费用列文字粘连（scoped 样式未命中 render 节点）

- `UsageTable.vue` 的费用列 render 回调用 `h("span", { class: "cost-cell" })` 创建节点；NDataTable 的 render 输出**不带本组件的 scope 属性**，`<style scoped>` 的 `.cost-cell` 选择器（316-337）永不命中。
- 真实 computed display=inline → 主金额与「CCS 来源记录费用 $9.00」排成一行（"$123.45†CCS 来源记录费用 $9.00"）。
- 修复方向：费用列抽成**独立子组件**（CostCell.vue，SFC scoped 天然生效），或 render 节点带内联样式（不依赖 scope）。选独立组件：标签/金额/说明结构清晰、可单测 computed style。窄窗口不得撑坏表格（980×620 验证）。

### C. 「有来源费用却仍显示未知」的费用状态混排

- `aggregate.rs push_rollup_into`（500-565）无条件：四桶计入 `unknown_tokens`（540）、`unknown_pricing=true`（544）、`unknown_reason=rollup_only`（545）。
- 结果：14 个带来源费用的模型组全部显示「未知†」——把「无请求级估算」「来源已提供费用」「模型真缺价」混成一个语义。
- 修复方向（用户已定，展示规则 1-7）：
  1. 纯日汇总 + 有来源费用 → 行费用读数 = 来源金额 + 「来源记录」标识，**不再并列主读数「未知†」**。
  2. 来源显式 0 → 保留 $0.00 + 来源标识；不宣称免费、不改写缺失。
  3. 请求估算 + 来源费用并存（B 类桶混合组）→ 两部分分别标明，不静默混合。
  4. 真缺价部分继续如实披露；「来源未提供费用」≠「模型没有价格」。
  5. **不把 unknown_pricing 全部清零**：新增最小状态字段区分各部分（见 §3）。
  6. 不为日汇总编造请求时间/上下文/分档；不拿今天单价冒充历史费用。
  7. 来源选择、去重、请求数、四桶、导入幂等**全部不变**；来源费用只累计最终采用的汇总。

## 1. 设计

### 1.1 TrendChart 可见性感知（缺陷 A）

新增可见性判定 + pending 机制，**不引入对 KeepAlive 的耦合**（组件只看自身 DOM 是否可测量）：

```
function containerMeasurable(): boolean
  = el.value && el.value.offsetParent !== null（隐藏/分离 → null）
```

- `initChart()` / `rebuildForTheme()` / `flushChart()` 在 `!containerMeasurable()` 时：
  - 不 dispose、不 init、不 setOption；把本次意图记入 `pending`（theme/locale → `rebuild`；数据/维度 → `data`/`dimension`；resize → `layout`；最新一次覆盖旧 pending——同一批内合并）。
  - `rebuild` 的 pending 同时**快照**当前 zoom（captureZoom 只能在实例存活时取；隐藏期实例保留不销毁）。
- 恢复时机：`document.visibilitychange` 不可靠（KeepAlive 页面切换不触发），用 **ResizeObserver 对父容器的观察 + `onActivated`**。组件在 KeepAlive 下 `onActivated` 会在回页时触发；再叠加一层 `requestAnimationFrame` 等布局完成。
  - `onActivated(() => { pending 应用 })`——先 rAF 两帧（容器已重新布局），读 `el.clientWidth/clientHeight` 真实尺寸；
  - 若有 pending：按 pending 类型执行 init/rebuild/flush（用真实尺寸，`echarts.init(el, null, { renderer:"svg", width, height })` **显式传尺寸**）；
  - 无 pending 但发现 ECharts 实际输出尺寸 ≠ 容器（防御：读 `.chart-canvas svg` 的 getBoundingClientRect 或 chart.getWidth/getHeight）→ `resize({width, height})` 校正（不 setOption，不打断动画）。
- `applyHeight` 保留，但**只在容器可测量时**返回实测值；不可测量路径不再被调用（前置守卫）。`appliedWidth/appliedHeight` 只记**真正下发到实例**的尺寸。
- ResizeObserver 回调同样加可测量守卫（隐藏期 observer 不触发，但防御性保留）。
- 显式传尺寸的场景：init（含恢复路径）。运行中 resize 仍走 `chart.resize({width,height})` 显式值。

### 1.2 费用列独立组件（缺陷 B）

- 新建 `frontend/src/components/CostCell.vue`：props = group（Group）；模板结构：
  ```
  <span class="cost-cell">
    <span class="cost-main">…估算或未知读数…（含 N18 分档 pill）</span>
    <span v-if="source != null" class="source-cost-cell">…标签+金额…</span>
  </span>
  ```
- SFC scoped 样式天然命中自己模板的节点；UsageTable render 回调改为 `h(CostCell, { group })`——组件实例有 scope id，样式生效。
- 展示逻辑（含 §2 C 类规则）全部收进 CostCell，UsageTable 不再持有费用排版。

### 1.3 费用状态字段（缺陷 C）

**后端（aggregate.rs）**：
- `push_rollup_into` 保留 unknown_tokens/unknown_pricing/unknown_reason 语义（真缺价披露不变），新增**来源覆盖状态字段**：

```rust
/// 2026-11-14：本组「有请求级估算的 token 部分」与「只有日汇总的部分」的构成。
/// `rollup_only` = 组内全部用量来自最终采用的日汇总（没有任何请求级明细贡献）；
/// `mixed` = 请求明细与日汇总并存；`detail` = 只有明细（不输出该字段）。
#[serde(skip_serializing_if = "Option::is_none")]
pub cost_detail: Option<&'static str>,   // "rollup_only" | "mixed"
```

- 判定：明细事件与汇总贡献**都会**写 `cost_detail`：push_event_into 写 `detail`（内部值，不序列化）/ push_rollup_into 写 `rollup_only`；合并规则 `detail+rollup → mixed`。序列化时 `detail` 省略（向后兼容：纯明细组输出与现状逐字节一致）。
- 不改 unknown_pricing/unknown_tokens/unknown_reason 的现有语义（合计行照旧）——**展示层**据此分流。

**前端（CostCell + locales）**：
- 组行费用列：
  - `cost_detail === "rollup_only"` 且 `source_cost_usd != null` → 主读数 = `formatMoney(source)` + 「来源记录」小标签（tooltip = sourceCostHint）；**不显示未知†**。
  - `cost_detail === "rollup_only"` 且 `source_cost_usd == null` → 维持现状「未知†」（来源未提供费用）。
  - `cost_detail === "mixed"` → 主估算读数照旧 + 来源费用独立行（现状 T2 样式），各自归属清晰。
  - 其他（纯明细/缺价）→ 现状逻辑不动。
- `unknown_tokens` 列与「含未计价 token」胶囊语义不变（日汇总 token 仍计入 unknown_tokens；rollup_only 行的胶囊/未知列如实显示——「这部分费用未按请求估算」为真）。
- SummaryCards（总览卡）：totals 侧同规则——`totals.cost_detail === "rollup_only"`（全部组都是纯汇总）且 totals.source_cost_usd != null 时主读数显示来源金额+标识；mixed/其他维持现状。合计行（UsageTable total row）同组行规则。
- locale 新键：`sourceCostTag`（"来源记录"/"Source recorded"）。en-US 同步。

### 1.4 兼容性

- `Group` serde 新字段 `cost_detail`（skip_if none）→ 旧 view-cache 快照（SNAPSHOT_VERSION 12）读入后该字段为 None → 前端回退现状逻辑，显示不崩。**快照版本不递增**：字段缺失只是回到旧展示，不是数据损坏（与 cache_write_split_estimated 引入时同构）。
- fixture/contracts（`ui-contracts.mjs`）新增组数据时按需带 `cost_detail`，旧数据不填 → 走现状路径，既有契约不受影响。

## 2. 红测（先写，全部在旧实现下失败）

### 2.1 TrendChart（`TrendChart.test.ts`，happy-dom + echarts mock）

- T-A1「隐藏容器不 init」：mount 后把组件根 `el.offsetParent` 置 null（happy-dom 可用 `Object.defineProperty` 桩 offsetParent 或把容器 `display:none`——用 display:none 真实路径）；改 `mode` → `init` 次数不变、无 dispose、`pendingTheme` 置位（经内部状态或下次行为断言）。
- T-A2「恢复时按真实尺寸 init 且显式传尺寸」：隐藏期改主题+语言（pending 合并为一次 rebuild）；容器恢复可见 + `clientWidth=1030`；`onActivated` 或观察器触发恢复 → `echarts.init` 第 2 次调用的第 3 参含 `width: 1030`；`appliedWidth`（经 resize 断言）= 1030 而非旧值。
- T-A3「可见时无 pending 但 ECharts 尺寸失配 → resize 校正」：init 后容器宽从 1030 变 800（模拟隐藏期间容器变宽）→ 恢复可见 → `resize({width:800})` 被调、`setOption` 未被多余调用。
- T-A4「缩放范围保留」：隐藏期 rebuild 后 setOption 的 dataZoom start/end 与隐藏前一致（既有 captureZoom 逻辑，补隐藏路径断言）。
- T-A5「重复激活不重复 resize」：恢复后连发两次激活事件 → 第二次无 resize/setOption。
- KeepAlive 真实路径：`onActivated` 需要 KeepAlive 上下文——单测里用 KeepAlive 包裹组件并 toggle `v-if`（Vue test-utils 支持），验证 activated 钩子被调。**替代**：把恢复逻辑收敛成 `maybeRestore()` 暴露方法 + 在 onActivated/observer 里调；单测直接测 `maybeRestore` + 真实浏览器契约覆盖 KeepAlive 路径。

### 2.2 CostCell（新 `CostCell.test.ts`）

- T-B1「scope 命中」：mount CostCell → `.cost-cell` 元素**带 data-v scope 属性**（组件渲染即有）；computed display（jsdom 里 getComputedStyle）为 flex 列。
- T-B2「窄窗口不撑坏」：容器 200px 时 `.source-cost-cell` 白空间 nowrap 但整体在 cost-cell 内换行，无横向溢出（结构断言：cost-cell 是列布局）。
- UsageTable 集成（`UsageTable.test.ts`）：费用列 render 返回 CostCell vnode → 行内 `.cost-cell` 带组件 scope 属性（旧实现下该断言失败——节点无 scope）。

### 2.3 aggregate（`ccs_rollup_source_cost.rs` 扩展 + 前端组件测试）

- T-C1「rollup_only + 来源费用 → cost_detail=rollup_only」：纯汇总组断言 `cost_detail == Some("rollup_only")`（旧实现无字段 → 编译期/运行期红）。
- T-C2「明细+汇总混合 → mixed」：A 类桶写 detail + B 类补汇总 → `Some("mixed")`。
- T-C3「纯明细 → 不输出字段」：serde JSON 里无 `cost_detail` 键（skip_serializing_if 生效）。
- T-C4「现有语义不变」：unknown_pricing/unknown_tokens/unknown_reason/source_cost_usd 断言全部保留（改断言 = 禁止）。
- T-C5「totals 聚合」：全 rollup_only 组 → totals.cost_detail=rollup_only；混合组 → mixed。
- 前端 CostCell 测试覆盖展示规则 1/2/3/4：rollup_only+费用、rollup_only+0、rollup_only+缺失、mixed、纯明细缺价。

### 2.4 浏览器契约（interaction-contracts.mjs）

- 新契约 `summary_chart_survives_hidden_rebuild`：normal fixture → 汇总页按模型维度 → 进设置 → 切主题 → 切语言 → 回汇总 → 断言 `.chart-canvas svg` getBoundingClientRect().width ≈ 容器宽（>300px；旧实现 SVG=100 → 红）+ 截图。1280×820 与 980×620 × 明暗四组合由 runner 覆盖（与 pricing_scroll 契约同法）。
- 新契约 `cost_cell_layout_not_concatenated`：新 fixture `source-cost`（三个组：rollup_only+7.25、rollup_only+0、mixed+估算0.20+来源9.00、纯明细缺价组）→ 断言：主金额与来源标签的**纵向坐标不同**（getBoundingClientRect top 差 > 4px）或 cost-cell computed display 为 flex/column；每个来源费用金额与标签同排（同一行内）；窄窗 980×620 无横向溢出；截图。
- `REQUIRED_CONTRACTS` 注册两条（runner 用 `only` 参数跑必需清单——CI frontend 作业跑全量）。先确认 runner 的全量/子集机制（run-ui-refresh-e2e.mjs / check-ui-contracts.mjs 的 only 参数）再落位。

### 2.5 现有测试回归面（预期改动点）

- `UsageTable.test.ts`：`source_cost_is_shown_per_model_row_and_total` 现断言「估算未知† + 来源费用并列」——rollup_only 行为改变后：该测试的 fixture 需带 `cost_detail`（旧场景= mixed 或 unknown 原因非 rollup_only 的等价场景），**新增** rollup_only 行断言（主读数=来源金额、无未知†）。逐条核对 49 项组件测试。
- `SummaryCards.test.ts`：T2 两个用例按 totals.cost_detail 分流更新；新增 rollup_only totals 用例。
- `TrendChart.test.ts`：主题切换 dispose/init 现有断言在可见容器下不变；新增隐藏路径用例。
- Rust：ccs_rollup_source_cost 11 例断言补 cost_detail；其他 unknown_pricing 断言的测试不受影响（字段向后兼容）。

## 3. 实施顺序（红→绿，每步可独立验证）

1. **红**：写 T-A1~A5（TrendChart.test.ts）、T-B1/B2 + UsageTable scope 断言（CostCell/UsageTable.test.ts）、T-C1~C5（Rust + CostCell.test.ts 展示规则）。全部跑红取证。
2. **绿·A**：TrendChart 加 `containerMeasurable`/`pending`/`maybeRestore` + `onActivated`/ResizeObserver 守卫 + init 显式尺寸 + 恢复后实际尺寸核对。跑绿 T-A*。
3. **绿·B**：新建 CostCell.vue（含 C 类展示规则）；UsageTable 费用列换组件。跑绿 T-B*。
4. **绿·C**：aggregate.rs 加 `cost_detail`（明细/汇总/合并/finish 聚合 + serde skip）；CostCell 按字段分流；SummaryCards totals 分流；locale 新键。跑绿 T-C* + 全量组件测试。
5. **契约**：ui-contracts.mjs 加 `source-cost` fixture 与组数据；interaction-contracts.mjs 加两条契约并注册。本地跑新契约取证（含截图逐张查看）。
6. **全量门禁**：typecheck / format / 前端全量测试 / build / e2e 全契约；Rust 两套 clippy -D warnings / 全量测试；build_inputs --check；版本 bump。
7. **版本**：`python -B scripts/bump_version.py --bump patch --summary "…"`（基线 0.9.0 → 0.9.1；先 fetch main 确认基线）；`--check --base-ref origin/main` 自查；`release-change.json` 随工具生成。
8. **提交**：红测与修复按缺陷分组提交（A/B/C 三个 commit + docs/版本 commit）；推送建 PR；跟踪最新 HEAD 必需检查至完成。
9. **文档**：`docs/stats-semantics.md` §4/§5.1 增补 cost_detail 口径与展示规则；本计划回填交付状态；`docs/plans/README.md` 登记本批。

## 4. 不做

- 不改真实 history.db / 价格文件 / 设置 / 会话日志；测试全部临时目录 + 合成数据。
- 不修 /btw、AC10、privacy-denied-ipc 或其他独立项。
- 不把来源费用写成请求级估算；不为日汇总补造请求字段。
- 不用全局 span 样式修 B；不用 CSS 拉伸修 A。
- 视图快照版本不递增（字段向后兼容）；价格索引版本不涉及。
- 人工清理一律送回收站；不关闭用户实例。

## 5. 验收口径

- 三个缺陷各有旧实现下失败的回归，修复后转绿；无断言降级。
- 浏览器截图逐张查看：1280×820 / 980×620 × 明暗（图表恢复 + 费用列布局）。
- 准确记录 CSS 视口与 DPR；缩放验证以 CSS 像素为准，不冒充系统缩放实测。
- 保留计数/四桶/来源选择/导入幂等不变量（Rust 既有断言全部保留并新增 cost_detail 断言）。
- PR 必需检查（gate/contracts/frontend/rust/rust-acceptance/native-ui/install-test/upgrade 矩阵）最新 HEAD 全绿。
