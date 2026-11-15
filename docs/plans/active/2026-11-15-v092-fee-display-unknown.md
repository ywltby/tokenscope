# v0.9.2 修复计划：费用列单金额 + 悬浮详情；日汇总按费用覆盖真实计为未定价

> 创建：2026-11-15。分支 `fix/v091-fee-display-unknown`（基于 main `2e6c035`）。
> 授权范围：两项缺陷的红测→最小修复→验证→文档→版本→提交→PR。不自动合并、不发布、不在本机安装。
> 基线：`v0.9.1`（已发布、升级基线已登记）。版本工具分配 patch：`0.9.2`。
> **用户明确更正**：旧口径「日汇总全部保留为未定价 token」是错误约束，本批撤销；旧计划/stats-semantics 若冲突以本提示为准并据实更新。

## 0. 问题与根因

### A. 费用列过度展示来源信息

现状（v0.9.1）：模型行/合计行费用列常驻两行（主金额 + 「CCS 来源记录费用 $X」）、总览卡常驻来源行、partial 时再加警告胶囊与长说明——用户要求**一行只显示一个金额**，来源信息全部收进悬浮详情。

### B. 日汇总被错误计为「未定价 token」

根因：`aggregate.rs::push_rollup_into` 把每条采用汇总的**全部 token 无条件**计入 `unknown_tokens` 并置 `unknown_pricing=true`/`unknown_reason=rollup_only`。即使模型已有价格、CCS 已完整提供费用（含显式 0），仍显示大量未定价。这不是价格同步问题。

## 1. 设计

### 1.1 聚合侧：按费用覆盖逐条判定未定价（缺陷 B）

`push_rollup_into` 逐条判断：

1. **来源费用已提供**（`Some(v)`，含 `Some(0)`）→ 该汇总的 token **不进 unknown_tokens**；费用走来源通道（`source_cost_usd` 累计不变）；组仍可标 `cost_detail`（rollup_only/mixed 语义不变——数据粒度披露，不等同缺价）。
2. **来源费用缺失**（`None`）→ 尝试按模型价格**估算该汇总**（`pricing.estimate(model, tokens, 汇总日)`）：
   - 估算成功且 `complete` → 金额计入 `cost_usd`（依据标注为「按日汇总估算」），token 不进 unknown；**依据披露**进悬浮详情（不是历史精确费用——不编造请求时间/分档，估算时间取汇总归属日，`basis` 如实标注 prompt 总量）；
   - 估算成功但 `complete=false`（部分分项缺价）→ 已知分项金额计入 cost_usd，**实际未获覆盖的分项 token**（`est.unknown`）进 unknown_tokens；
   - 无候选/估算失败 → 全部 token 进 unknown_tokens（真实未定价），悬浮详情写明缺项（无匹配价格/缺分项），不笼统说「模型没有价格」。
3. **不双重计费**：有来源费用的汇总不再叠加估算（同一份用量只选一次费用依据）。
4. `unknown_pricing` 语义不变：**只在实际存在未获费用覆盖的 token** 时置位——有来源费用的纯汇总组不再置位（unknown_reason 相应不再标 rollup_only 缺价含义；组内任何 token 未获覆盖时才置位并标原因）。
5. 请求数、四桶原始 token、去重、来源选择、导入幂等零改动。
6. **视图缓存失效**：`SNAPSHOT_VERSION` 12 → 13（unknown_tokens/unknown_pricing/cost 语义变化，旧快照的分组费用状态不可信，按仓库约定读不出即重建派生结果；不要求清库/重扫/重导）。

### 1.2 展示侧：单金额 + 悬浮详情（缺陷 A）

- **费用列主读数 = 对互不重叠贡献的合计**：`display_cost = cost_usd（请求估算 + 可估算汇总）+ source_cost_usd（采用汇总的来源费用）`——未舍入原值相加，`formatMoney` 统一格式化一次。
- `Group` 新增 `total_cost_usd: Option<f64>`（后端合并值，避免前端重复实现加法与舍入顺序；请求估算/来源费用构成进悬浮）。
- CostCell：主读数只显示 `total_cost_usd`（一个金额）；有真实缺价时只保留 `†` 等短标记；**删除**常驻来源胶囊/第二行/长说明；`cost_detail`/`source_cost_state` 不再常驻展示。
- 悬浮详情（新增 `GroupCostBreakdownTooltip`，复用 CostBreakdownTooltip 的浮层配方与 RC06 交互）：分「请求明细估算」「日汇总来源记录费用」「日汇总按价格估算」「缺失部分与依据」四类展示，hover/focus/click 固定三态 + Escape + 外点关闭；键盘可达（触发器 = 金额本身，原生 button 语义沿用）。
- total 行与总览卡同一规则（同一金额、同一悬浮）。SummaryCards 删除常驻来源行/胶囊。
- 不称为实际账单；不加常驻说明。

### 1.3 汇总估算的依据披露

- 悬浮中显示「按日汇总估算（YYYY-MM-DD，prompt N tokens）」与四分项小计；注明「日汇总无请求时间/缓存分档，估算不等于历史精确费用；未编造分档」。
- 有来源费用时显示「来源记录费用 $X（YYYY-MM-DD 明细逐条）」。

## 2. 红测（先红，全部行为断言）

### 2.1 Rust（aggregate）

- T-B1「有来源费用的汇总不再计未定价」：单条 rollup `Some(7.25)`（模型无价格候选）→ 该组 unknown_tokens=0、unknown_pricing=false；来源金额照常。
- T-B2「显式 0 同样覆盖」：`Some(0)` → unknown_tokens=0（零费用 ≠ 缺失）。
- T-B3「部分提供逐条判定」：两条汇总 A=Some(7.25)、B=None（模型无价格）→ unknown_tokens 只含 B 的 token；unknown_pricing=true。
- T-B4「缺失时按可适用价格估算」：模型有完整价格、rollup None → cost_usd=估算值、unknown_tokens=0；估算依据（basis=日 prompt 总量）可查。
- T-B5「部分分项缺价 → 实际未获覆盖分项进 unknown」：价格缺 cache_read、rollup None 且有 cache_read token → cost=已知分项、unknown_tokens.cache_read=该量。
- T-B6「真正无可用价格保留未定价」：无候选 → unknown=全部、悬浮依据可说明。
- T-B7「不双重计费」：有来源费用 + 模型有价 → cost_usd 不追加估算。
- T-B8「A/B/C 选择与请求数/四桶不变」；T-B9「混合明细+汇总（mixed）时：明细缺价 token 照旧 + 已覆盖汇总不算未定价」。
- T-B10「跨分组合计与覆盖状态一致」（承接上轮 cost_detail 语义：unknown 只按实际覆盖）。

### 2.2 前端组件

- T-A1「单金额」：mixed totals（est 1.0 + source 7.25）→ CostCell 主读数 `$8.25`（未舍入相加后统一格式化），**无**常驻来源行/胶囊；纯汇总组同理。
- T-A2「短标记保留」：真实缺价 → 只显示 `未知†`/`†`；无来源长文。
- T-A3「悬浮详情」：hover/focus/pinned 打开浮层，内容含「请求明细估算」「日汇总来源记录费用」分区与依据；Escape/外点关闭。
- T-A4「总览卡同一规则」：SummaryCards 主读数 = 同一 total 金额；无常驻来源行；悬浮同构。
- T-A5「键盘聚焦可打开详情」。

### 2.3 浏览器契约

- 更新/新增契约：默认状态费用列/总览卡只有单金额（无「CCS 来源记录费用」常驻文本、无来源胶囊）；hover/键盘聚焦打开详情且内容分区正确；明暗 × 两尺寸；100px 图表修复与表格滚动不回归。

## 3. 实施顺序

1. 红测（Rust 10 + 前端 5）→ 全部行为断言红取证。
2. 绿·B：aggregate push_rollup_into 覆盖判定 + estimate 接线 + unknown_reason 语义 + SNAPSHOT_VERSION 13。
3. 绿·A：Group.total_cost_usd 后端合并 → CostCell 单金额重构 → GroupCostBreakdownTooltip 新组件 → SummaryCards/UsageTable 接线 → locale 增删。
4. 契约更新 + 浏览器验证（明暗 × 两尺寸截图逐张）。
5. 文档：stats-semantics §4/§5.1 撤销旧规则；DESIGN.md §5 费用明细补悬浮详情；本批计划回填。
6. 版本 0.9.2（patch，工具分配）→ 全量门禁 → 提交推送 → PR 跟踪。

## 4. 不做

- 不改真实 history.db/价格/设置/日志；不清库、不重扫、不重导（视图缓存按版本自动失效重建）。
- 不把日汇总 token 当单请求上下文；不编造请求时间/分档/历史价格。
- 不动 A/C 类排除逻辑、请求数、四桶、去重、导入幂等。
- 不动线上缺失缓存价按 0 的现有规则；请求明细缺价照旧。
- 不扩修 AC10、/btw 等独立项。

## 5. 交付状态（2026-11-15）

- **红测（行为断言红取证）**：Rust 10 项（rollup_fee_coverage.rs）9 项失败在
  unknown_tokens/unknown_pricing/cost 断言（如 2,000,000 ≠ 0）；T-B5 初版
  fixture 用 modelsdev 路径触发 T3 assumed_zero（探针证实 rate_kind=
  AssumedZero 是现有产品规则的正确行为），改用外置 pricing.toml（未声明键
  = 未知）后行为红可复现。前端旧断言面（SummaryCards/UsageTable 42 项）
  按新规则重写。
- **绿·B（aggregate.rs）**：push_rollup_into 费用覆盖逐条判定（有费用 →
  token 不进 unknown 不叠加估算；缺失 → 按价格估算该汇总，时间 = 归属日
  00:00 UTC、basis 标注日 prompt 总量；部分缺价只计实际未获覆盖分项；
  无候选保留真实未定价）；`Group.total_cost_usd`/`cost_partially_unknown`
  合并字段；SNAPSHOT_VERSION 12 → 13（旧快照费用状态不可信，读取时忽略
  重建，不清库/重扫/重导）。既有旧规则断言更新 4 处
  （ccs_rollup_coverage 1 + ccs_rollup_source_cost 2 + history_overlap 2，
  全部是「日汇总全部未定价」撤销后的语义修正）。
- **绿·A（前端）**：CostCell 单金额重构（主读数 = total_cost_usd，† 由
  GroupCostBreakdownTooltip 统一拼接，修复双 †）；新组件
  GroupCostBreakdownTooltip（构成分区：请求明细估算/日汇总来源记录费用/
  按日汇总估算依据/缺失分项；RC06 三态 + Escape + 外点关闭；内容 vnode
  用函数式组件包裹——直接模板插值会被 toDisplayString 压崩）；SummaryCards
  常驻来源行/胶囊/未计价胶囊全部移除；UsageTable 表注撤掉来源长文。
- **契约**：cost_cell 契约更新为单金额规则（cells 查询 .gcb-trigger、
  display 断言改单金额存在性、悬浮 hover 断言、总览卡单金额），fixture
  更新 total_cost_usd/cost_partially_unknown 与覆盖后的 unknown 数据。
  明暗 × 两尺寸全过；完整 e2e runner（11 契约 × 明暗 × 两尺寸）全过
  （100px 图表修复与表格滚动无回归）。
- **全量门禁**：Rust 全 workspace 测试全过（含既有语义修正）、clippy
  -D warnings 干净、前端 1150 项全过、typecheck/format/build=0、
  build_inputs --check 干净、版本 0.9.2（patch，release: true）。
- **文档**：stats-semantics §5.1 撤销旧规则并写入费用覆盖判定与单金额
  展示规则；DESIGN.md §5 费用明细补分组费用详情；本计划回填。
