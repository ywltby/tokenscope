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
2. **来源费用缺失**（`None`）→ 按**可证明适用**的费率估算该汇总（`pricing.estimate_rollup(model, tokens)`——枚举所有可能的时段/上下文条件，同一分项在所有条件下都是同一显式单价才计价；始终生效的覆盖规则照常应用；分段档位/时段价依赖缺失的请求长度/时刻时，该分项真实未定价；不受缺失条件影响的平价分项照常计价）：
   - 估算成功且 `complete` → 金额计入 `cost_usd`（依据标注为「按日汇总估算」），token 不进 unknown；**依据披露**进悬浮详情（不是历史精确费用——不编造请求时间/分档，不默认基础档位、不挑任意时刻）；
   - 估算成功但 `complete=false`（部分分项缺价或条件不确定）→ 已知分项金额计入 cost_usd，**实际未获覆盖的分项 token**（`est.unknown`）进 unknown_tokens；
   - 无候选/估算失败 → 全部 token 进 unknown_tokens（真实未定价），悬浮详情写明缺项（无匹配价格/缺分项），不笼统说「模型没有价格」。
3. **不双重计费**：有来源费用的汇总不再叠加估算（同一份用量只选一次费用依据）。
4. `unknown_pricing` 语义不变：**只在实际存在未获费用覆盖的 token** 时置位——有来源费用的纯汇总组不再置位（unknown_reason 相应不再标 rollup_only 缺价含义；组内任何 token 未获覆盖时才置位并标原因）。
5. 请求数、四桶原始 token、去重、来源选择、导入幂等零改动。
6. **视图缓存失效**：`SNAPSHOT_VERSION` 12 → 13（第三轮）→ 14（第四轮）
   → 15（第五轮：日期可行性/规则择价/`same_as_input` 改变部分汇总读数）
   → 16（第六轮：来源日真实时间片切分 + 逐请求支配择价再次改变部分汇总的
   费用/未定价读数）→ 17（第七轮：逐分项可证明判定保留部分已知分项、DST
   回拨重复小时切分再次改变部分汇总读数）→ 18（第八轮：时区偏移切换点进入
   切分、分项判定改按请求构成）→ 19（第九轮：请求用量比例判定改变部分
   汇总的费用/未定价读数）→ 20（第十轮：跨候选择价与语义完整性改变部分
   汇总的费用/未定价/估算读数）→ 21（第十一轮：日汇总按 0 估算披露改变
   `complete` 与 `cache_price_zero_estimated` 读数）；旧快照的分组费用状态
   不可信，按仓库约定读不出即重建派生结果；不要求清库/重扫/重导。

### 1.2 展示侧：单金额 + 悬浮详情（缺陷 A）

- **费用列主读数 = 对互不重叠贡献的合计**：`display_cost = cost_usd（请求估算 + 可估算汇总）+ source_cost_usd（采用汇总的来源费用）`——未舍入原值相加，`formatMoney` 统一格式化一次。
- `Group` 新增 `total_cost_usd: Option<f64>`（后端合并值，避免前端重复实现加法与舍入顺序；请求估算/来源费用构成进悬浮）。
- CostCell：主读数只显示 `total_cost_usd`（一个金额）；有真实缺价时只保留 `†` 等短标记；**删除**常驻来源胶囊/第二行/长说明；`cost_detail`/`source_cost_state` 不再常驻展示。
- 悬浮详情（新增 `GroupCostBreakdownTooltip`，复用 CostBreakdownTooltip 的浮层配方与 RC06 交互）：分「请求明细估算」「日汇总来源记录费用」「日汇总按价格估算」「缺失部分与依据」四类展示，hover/focus/click 固定三态 + Escape + 外点关闭；键盘可达（触发器 = 金额本身，原生 button 语义沿用）。
- total 行与总览卡同一规则（同一金额、同一悬浮）。SummaryCards 删除常驻来源行/胶囊。
- 不称为实际账单；不加常驻说明。

### 1.3 汇总估算的依据披露

- 悬浮中显示「只采用可证明适用的费率」与各已知分项小计；注明「始终生效的覆盖规则照常应用；依赖缺失的请求时刻/上下文长度的分项保持未定价、不计入金额；估算不等于历史精确费用；未编造请求时间与分档」。
- 有来源费用时显示「来源记录费用 $X（逐条明细）」。

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
- **绿·B（aggregate.rs，第三轮）**：push_rollup_into 费用覆盖逐条判定（有费用 →
  token 不进 unknown 不叠加估算；缺失 → 第三轮曾用 `estimate_flat` 固定基础价，
  **已被第四轮 `estimate_rollup` 取代**；部分缺价只计实际未获覆盖分项；无候选
  保留真实未定价）；`Group.total_cost_usd`/`cost_partially_unknown`
  合并字段；SNAPSHOT_VERSION 12 → 13（旧快照费用状态不可信，读取时忽略
  重建，不清库/重扫/重导；第四轮再升 14）。既有旧规则断言更新 4 处
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

### 第三轮复核反例修复（2026-11-15，`b7d5b15`）

用户复核确认三个缺陷，按「红测先行」逐一修复并补回归：

1. **零元价格显示「未知†」**：旧 `total_cost_usd` 判定看金额是否非零，
   免费价 × token 估算结果恰为 0 时被当成无依据。修复：`GroupAccum.has_cost_basis`
   （明细估算发生——含结果为 0——或采用汇总带来源费用即置位），分组行与
   合计行按费用依据判定 → `$0.00`。回归
   `zero_priced_estimate_shows_zero_not_unknown`（修复前 None / 修复后
   Some(0.0)）。
2. **日汇总误用 UTC 零点与上下文档位**：旧路径 `estimate(rollup_ts=归属日
   00:00 UTC)` 会命中峰谷时段，并按日 prompt 总量切高档段。第三轮先改为
   `Pricing::estimate_flat`（清空分段、忽略全部时间规则、固定基础价）——
   **该中间版本已被第四轮取代**：它把「不能选档」错误扩大成「基础价一定
   适用」，会低算大上下文日汇总、丢弃始终生效的覆盖规则。回归已改写为
   `rollup_segment_pricing_not_guessed_from_day_total`（input 真实未定价）
   与 `time_limited_rule_does_not_guess_day_time`。
3. **悬浮层内部样式未生效**：`renderContent()` 的 `h()` 节点不带 SFC
   scope 属性，`<style scoped>` 的 `.gcb-section/.gcb-solid` 永不命中。
   修复：样式改全局作用域 + `gcb-` 前缀类名；契约新增
   `v092-tooltip-solid-style-applied`（真实浏览器 computed background/
   padding 断言）。原节点级 scope 断言随架构更新为组件挂载结构验证
   （费用列不再依赖 scoped CSS，render 直建 DOM 问题结构上不可能复发）。

**红取证（控制实验，不只靠“修完刚好通过”）**：

- 反例 2（第三轮控制实验，结论已被第四轮修正）：同一价格表下，旧路径
  `pricing.estimate` 在 UTC 零点对 2M prompt 返回 **$16**（按日总量切
  >1M 高档段）；第三轮 `estimate_flat` 的 **$2** 曾是当时的错误预期。
  第四轮改为：单请求上下文缺失 → input 真实未定价，`total_cost_usd=None`。
- 反例 3：临时把组件 `<style>` 改回 `<style scoped>` 复跑同一契约，
  `v092-tooltip-solid-style-applied` **失败**（background
  `rgba(0, 0, 0, 0)`、radius `0px`、padding `0px`，两个视口）；恢复全局
  样式后断言全过。

**验证与状态（第三轮，后被第四轮扩展）**：Rust `rollup_fee_coverage` 13 项
全过；全 workspace 测试全过；前端 1150 项、typecheck/build=0；契约
`cost_cell_layout_not_concatenated` 明暗 × 1280×820/980×620 全过；完整
e2e runner（11 契约 × 明暗 × 两尺寸）全过。

### 第四轮复核修复（2026-11-16）：费率适用性 + 费用依据判定

用户复核确认两类未闭合问题，批准在 PR #70 分支实施：

1. **`estimate_flat` 强制基础价**：复制候选、删除分段、忽略规则后按 base
   计价，把「不能选档」换成「基础价一定适用」。已复现：两个 250K 请求
   请求路径 $5.00、日汇总 $0.50；始终生效的 $99/M 覆盖规则被丢弃、只算
   $1.00。修复：`Pricing::estimate_rollup` 枚举所有可能的时段/上下文条件，
   同一分项在**所有条件下都是同一显式单价**才计价；分段/时段依赖缺失
   条件的分项保持真实未定价；始终生效的覆盖规则照常应用；平价分项照常
   计价；不默认任何档位/时段。日总量只作为单请求规模的上界：低于阈值时
   可证明不可能命中高档（不是拿日总量选档）。
2. **「有候选」混同「有费用依据」**：`estimate.is_some()` 被当成依据，候选
   只提供未使用分项价格时全部实际 token 在 unknown、却显示 `$0.00†`。
   修复：`CostEstimate::covers_used_tokens` 按实际用量是否获得覆盖判定；
   `Group.has_estimated_cost` / 合计同字段贯通；悬浮详情用它决定是否展示
   估算构成，不再用 `cost_usd !== 0` 猜测。零元费率覆盖实际用量仍是
   `$0.00`；全部实际用量缺价 → `total_cost_usd=None`（未知）；已知零元
   + 缺价混合保留小计与 †；来源显式 0 与缺失 None 严格区分。

**红测（先红，行为断言）**：新增 `rollup_rate_applicability.rs` 12 项；
红取证时 4 项失败在金额/覆盖量/`total_cost_usd`（分段日汇总算出 $0.50、
始终规则只算 $1.00、时段规则算出 $7.00、未使用分项价格显示 $0.00†）；
前端零元悬浮红测失败在「本行没有可适用的费用依据」。修复后全部转绿。
既有「flat 恒基础价」断言按用户更正的语义改写，不保留错误算法。

**验证**：Rust 全 workspace 全过（新增 12 项 + 改写 2 项）；前端全量、
typecheck、build、Prettier 全过；契约新增
`v092-zero-tooltip-explains-basis` 与 `v092-unpriced-used-not-zero`，
`cost_cell_layout_not_concatenated` 21 条断言明暗 × 1280×820/980×620
全过；完整 e2e runner（11 契约 × 明暗 × 两尺寸）全过；真实浏览器复核
零元/未定价金额节点与悬浮内容（截图
`qa-artifacts/v092-fix/cost-cell-r4-2/` 与暗色 980×620 探针）。

### 第五轮复核修复（2026-11-16）：日期可行性、规则择价、same_as_input

用户复核 `d60810a` 发现三个仍被误判为未定价的日汇总（均 1M token）：

1. **周一汇总纳入仅周日生效的规则**：正确 $1.00，当时未定价。修复：
   `estimate_rollup` 接收来源统计日与来源时区，按规则时区检查来源日实际
   覆盖的星期并排除不可能的 period（来源日结束边界——次日零点——不属于
   该日）；日期/时区不可解析时保持保守。
2. **两条始终生效的规则**：现有择价逻辑应选 $3/M，当时判未知。修复：
   每个场景内部先完成请求级规则择价（完整优先 → 最高费用 → 稳定顺序），
   再把同时适用的规则当竞争候选，而不是互斥条件。
3. **`same_as_input` 丢失分支相关性**：缓存读取价全天 $1/M、仅输入价随
   时段变化，当时缓存读取也变未知。修复：`same_as_input` 在每个场景内
   按该规则自己的输入价解析，再做跨场景分项一致性比较；不先把输入价
   折叠成 Unknown 再解析。

回归新增 4 项（周一日/跨日边界、双始终规则、same-as-input、当天可能但
时刻缺失仍未知），`rollup_rate_applicability.rs` 共 17 项。独立 r5 探针
复跑：周一 $1.00、双规则 $3.00、缓存读取 $1.00；此前四项（大上下文、
时段折扣、始终规则、未使用分项）保持修复结果。`SNAPSHOT_VERSION` 14 → 15
（旧快照的汇总费用读数不可信，读取时忽略重建）。

**验证**：Rust 全 workspace、clippy -D warnings 全过；前端全量/typecheck/
build/Prettier 全过（快照版本 14 → 15）；本轮不改金额/悬浮渲染逻辑，
沿用第四轮已验收实现。PR #70 的必需检查以最新 HEAD 为准；停在可审查
状态，不合并、不发布、不安装。

### 第六轮复核修复（2026-11-16，基线 `64325c1`）：区间划分 + 逐请求主导择价

独立复核 `qa-artifacts/pr70-r6-review/` 用合成数据调用真实核心库 `aggregate_with_rollups`
给出三个已复现反例；红测先行（`rollup_applicability_round6.rs`）三项失败在
金额/覆盖断言（部分重叠未知 0≠1M、交叉优势 `unknown_pricing=false`、跨时区
`total=None≠$1`），再修实现：

1. **部分重叠被当成整段同时生效**（`time_worlds` 旧逻辑取"第一个重叠
   period"）：A 默认 $3/08-12 $2、B 默认 $1/10-14 $3，请求级 09/11/15 时
   分别 $2/$3/$3，日汇总却固定 $3。修复：来源日按**每条规则每个时区的真实
   绝对时间边界**切分为半开区间，每区间取代表时刻，直接调用请求级
   `matching_time_rules` 求该时刻真正同时适用的规则集——不构造"存在重叠"
   的伪组合。未在 08-10 区间出现 A 时段价 + B 默认价，费率 2/3 不一致 →
   input 真实未定价。星期、半开边界、跨时区、DST 由 jiff 统一处理，不假定
   自然日恒 24 小时。
2. **日总量配比替代单请求配比择价**（旧 `scenario_estimate` 用整桶 cost 挑
   胜出规则）：A(input3/output1) 与 B(input1/output3) 始终生效，日总量
   input=1M/output=1M 的两种合法构成分别 $6/$4，日汇总固定 $4。修复：择价
   改用**支配判据**——请求级 cost 是单请求 token 的线性函数，仅当存在一条
   规则在实际使用桶上逐项单价 ≥ 其它每条规则时，胜出规则才与配比无关、
   可证明计价；交叉优势无支配者 → 全部使用桶未定价。禁止逐字段拼价，禁止
   把日汇总当一个请求；input-only 下 $3/$2 的全局优势规则仍得 $3。
3. **跨时区只检查星期不检查窗口交集**（旧 `period_feasible`）：来源日 UTC
   周一对应上海周一08:00至周二08:00，周一00:00-01:00 折扣窗口已过、周二
   凌晨不满足 weekday，整段只能 $1/M，旧实现误报未定价。区间切分后该
   period 的真实绝对窗口与来源日无交，代表时刻 `matching_time_rules` 不命中
   → 可证明 $1。合法可发生（UTC 周日来源日含上海周一折扣窗）仍保持未定价，
   不过度清零。

**对照**：完全重叠且费率一致 → 计价；互不重叠费率不同 → 未定价；跨时区
可发生/不可发生两路；全局优势规则 → $3；DST 切换日不假定 24h。上一轮三修复
（周一排除周日、双始终规则 $3、`same_as_input` $1）与大上下文/时段差异/平价/
零元/来源费用/请求数四桶不变量全部保持回归。

**验证**：`rollup_applicability_round6.rs` 8 项新测全过；`rollup_rate_applicability`
17 项、`rollup_fee_coverage` 13 项、`review_regressions` 33 项全过。
`SNAPSHOT_VERSION` 15 → 16（部分汇总费用/未定价读数改变，旧快照读取时忽略重建）。
本轮不改金额/悬浮渲染逻辑，沿用第四轮已验收实现。停在可审查 PR，不合并、
不发布、不安装。

### 第七轮复核修复（2026-11-16，基线 `55bbaac`）：分项保留 + DST 回拨重复小时

独立复核 `qa-artifacts/pr70-r7-review/` 用合成数据调用真实核心库给出三个反例；
红测先行（`rollup_applicability_round7.rs`）三项失败在金额/覆盖断言（单规则
`total=None≠$1`、交叉规则 `total=None≠$1`、回拨日 `unknown_pricing=false`），
再修实现：

1. **单条部分定价规则丢已知费用**（`scenario_estimate` 旧逻辑要求胜出规则
   覆盖全部使用桶，否则全部 force_unpriced）：input=$1/M 有价、output 无价，
   日汇总 1M+1M 本应保留 input $1、只清 output。修复：改用 Pareto 前沿筛出
   "可能胜出规则"，再**逐分项**判定——所有可能胜出规则在该分项都有价且费率
   一致才计价，否则只清该分项。单规则下 input 可证明 $1、output 未定价。
2. **交叉优势规则丢公共不变分项**：A(input3/output1)、B(input1/output3)，
   cache_read 都 $1/M，日汇总 input/output/cache_read 各 1M。input/output
   胜出规则随配比变化（前沿费率 3/1 与 1/3 不一致 → 未定价），cache_read
   两侧恒 $1 → 可证明并保留。禁止逐字段拼价、禁止编造组合价格。
3. **DST 回拨第二次窗口被漏**（`day_instants` 对每个本地端点只调一次
   `to_zoned`）：来源日 UTC 2026-11-01（危地马拉 = [06:00Z, 次日06:00Z)），
   纽约周日 01:30-01:45 折扣 $10；回拨后该本地窗出现两次，第一次 05:30-05:45Z
   在来源日前，第二次 06:30-06:45Z 在来源日内。修复：对回拨 fold 端点取
   `to_ambiguous_zoned().earlier()/.later()` 两个真实映射都作切分边界；前拨
   gap 的不存在时刻只取请求级 compatible 映射。请求级 06:00/06:35/12:00Z
   分别 $1/$10/$1，日汇总费率真实变化 → input 1M 未定价、total=None。

**对照**：半开边界（06:30Z 命中 $10、06:45Z 不命中 $1）；前拨 gap 不制造
伪窗口、全日可证明 $1。上一轮三反例（部分重叠、交叉 $4、跨时区 $1）与
平价/优势规则/已知零元/真实缺价/`same_as_input`/线上缺缓存价/来源费用/
请求数四桶等全部保持。

**验证**：`rollup_applicability_round7.rs` 4 项新测全过；`rollup_applicability_round6`
8 项、`rollup_rate_applicability` 17 项、`rollup_fee_coverage` 13 项、
`review_regressions` 33 项全过。`SNAPSHOT_VERSION` 16 → 17（分项保留与回拨
切分改变部分汇总读数，旧快照读取时忽略重建）。版本沿用 0.9.2（同一 PR 补修
不重复递增）。停在可审查 PR，不合并、不发布、不安装。

### 第八轮复核修复（2026-11-17，基线 `49bdc30`）：偏移切换点 + 按请求构成判定 + 显式回收

**R8-1 前拨日的真实价格窗口**：`day_instants` 已有 fold 端点的 earlier/later
映射，但没有把**时区偏移切换点**当作切分边界。来源日与规则时区同为
America/New_York、周日 01:45–02:30 折扣 $10，2026-03-08 的 02:30 本地时刻
不存在，compatible 映射到 03:30，真实窗口 [01:45, 02:00) 落不进任何切片，
整日被误判成恒定 $1。修复：按 15 分钟粒度采样偏移、发现变化后二分到秒，
把切换点加入 cuts。请求级 06:30/06:50/07:10Z 分别 $1/$10/$1；日汇总
input 1M 未定价、total=None。

**R8-2 完整优先必须按分项实际被使用的请求判定**：全局 Pareto 前沿会把
「只在不含缓存读取的请求里才可能胜出」的规则算进可能胜出集合。规则 A
（output $1、cache_read $1）与 B（output $3、cache_read 缺价）都常驻，
日汇总 output 1M + cache_read 1M 时，任何含缓存读取的请求都由完整的 A
胜出，故缓存读取可证明 $1；输出仍取决于请求构成。修复：枚举使用中分项
的子集作为可行请求构成，逐条按「完整优先 → 费用最高」判断哪些规则可能
胜出；缺该分项的规则直接出局；基准改为对已证明分项覆盖最多的候选。
结果：保留 cache_read $1、`unknown.cache_read=0`、`unknown.output=1M`、
`total_cost_usd=Some(1.0)`。

**R8-3 显式回收站清理**：`_cleanup` 不再依赖宿主对 Temp 的放行策略，改为
显式 `SHFileOperationW(FOF_ALLOWUNDO)`，并用回收站 `$I` 元数据核对条目
（原路径消失不算回收成功）；回收失败、无法核对条目、或临时根内含超
240 字符路径时一律保留现场并让门禁失败——超长路径在调用前拒绝，避免
`SHFileOperationW` 删一半再失败。不回退永久删除、不用改名搬运代替回收。
`_path_too_long` 与其「WinError 145 = MAX_PATH」的错误归类一并移除。

**验证**：`rollup_applicability_round8.rs` 2 项新测（先红后绿）；
`scripts` 契约 17 项全过（含「原路径消失但回收站无条目必须失败」「读不到
回收站不能算成功」）；`SNAPSHOT_VERSION` 17 → 18。版本沿用 0.9.2。

### 第九轮复核修复（2026-11-17，基线 `f8f4a69`）：请求用量比例 + 回收意图前置

独立复核 `qa-artifacts/pr70-r9-review/` 给出两项反例，均用真实核心库 / 注入
模拟复现，红测先行（`rollup_applicability_round9.rs` 2 项失败在金额/unknown
数字断言；`test_run_rust_tests.py` 长普通文件路径预检失败在行为断言），再修实现：

**R9-1 择价必须考虑任意正配比，不能只测等权向量**。`scenario_estimate` 旧逻辑
枚举使用分项子集后把各分项单价等权相加判断胜出，等价于只测试 v=(1,1)。
请求级费用是配比的线性函数 `Σ v_k·p_k`：A(input3/output1) 与 B(input2/output1.5)
常驻时，balanced（两次各 500k+500k）输入费 $3、skewed（900k+0 / 100k+1M）
输入费 $2.90，两者日总量相同——输入单价本身不可由日总量确定。修复：对固定
支持集取完整候选，按 Motzkin 传递定理用**线性可行性 / 凸包支配**判定每条候选
是否可能在某个正配比下胜出（存在其它完整候选价格的非负组合逐项主导它则排除）；
实现为小规模 LP 顶点枚举（n≤4），非等权采样、非经验配比、非逐字段取价。
对照全过：交换规则声明顺序结果不变、逐项支配的全局优势规则仍计价（$4）、
竞争规则共有的恒定 cache_read 仍保留（$1）、无完整候选时部分规则的已知 input
保留（$1）。`unknown.input=1M`/`unknown.output=1M`/`total=None`。

**R9-2 回收必须前置拒绝永久删除，不能事后才发现**。旧 `_send_to_recycle_bin`
用 `SHFileOperationW(FOF_ALLOWUNDO)`，微软文档明确该标志只是 "if possible"、
回收站禁用时静默永久删除，`_cleanup` 事后查 `$I` 只能发现、无法保住现场。
修复：改用 `IFileOperation::SetOperationFlags(FOFX_RECYCLEONDELETE |
FOFX_EARLYFAILURE | FOF_NOERRORUI | FOF_SILENT)`——`FOFX_RECYCLEONDELETE`
明确要求回收站、不回退永久删除，`FOFX_EARLYFAILURE` 令任一子操作出错即整体
停止。COM 初始化/CoCreateInstance/IShellItem/SetOperationFlags/DeleteItem/
PerformOperations/GetAnyOperationsAborted 任一步失败、取消、S_FALSE 都返回
非零并保留源内容；`$I` 核验降为附加证据。不回退 rmtree/DeleteFile，不用改名
搬运，不改系统策略。后端经 `_file_operation` 注入模拟反例。长路径预检
`_has_overlong_entry` 改为对每个条目（含普通文件）检查完整路径、不跟随链接、
枚举失败按命中处理——修复「短目录 + 254 字符合法文件名」被漏检的漏洞。

**验证**：`rollup_applicability_round9.rs` 6 项全过（含请求构成对照与全部
对照）；`scripts` 契约 26 项全过（新增真实回收合成树入站核验、取消/接口失败/
不可回收反例、长文件与枚举失败预检）；既有 round6/7/8、rate、fee、
review_regressions 全绿。`SNAPSHOT_VERSION` 18 → 19。版本沿用 0.9.2
（同一 PR 补修不重复递增）。停在可审查 PR，不合并、不发布、不安装。

**R9-3 补齐 R9-1 的相邻误报（同日晚间自查发现）**。「支持集上没有完整候选」
这一支原先直接跳过、不加约束，而更小的支持集已把分项登记为可证明——于是两条
各只定价一个分项的规则（A 只定 input $1、B 只定 output $2）会被错误地同时
判为可证明，日汇总给出 input $1 + output $2 = $3。但请求级在该支持集上按
`highest_partial_cost` 回退：合并请求 A 已知 $1、B 已知 $2，选 B，input 整个
落空；大 input 小 output 的请求反过来选 A，output 落空。选中哪条由配比决定，
两个分项都不可由日总量确定。修复：该分支先用同一套线性可行性判定筛出**可能
胜出的部分候选**，再登记——某个分项只要存在一条可能胜出的候选不给它定价，就
标记为不可证明。不能简单「登记全部候选」：只定 cache_read 的基础层在已知费用
比较里永远赢不了，那样会把可证明的 input 一并拖成未知（新增对照
`non_competing_base_layer_keeps_provable_input` 锁定 input 仍为 $1）。单条部分
规则的已知分项不受影响（唯一候选给它定了价，仍保留 $1）。红测
`competing_partial_rules_leave_both_components_unknown` 先失败在
`unknown.input=0`（应为 1M），修复后 `unknown.input=1M`/`unknown.output=1M`/
`total=None`；round6/7/8/9、rate、fee、ccs_rollup_*、partial_pricing、
review_regressions 共 148 项全绿。

### 第十轮复核修复（2026-11-18，基线 `76dfcea`）：跨候选择价 + 语义完整性 + COM 配对

独立复核 `qa-artifacts/pr70-current-review/` 确认三项，均先写红测
（`rollup_applicability_round10.rs` 6 项失败在金额/unknown/完整性状态；
`test_run_rust_tests.py::ComApartmentBalanceCase` 的 S_FALSE 用例失败在配对次数 0）。

**R10-1 请求构成必须跨候选判定**。`estimate_rollup` 外层仍按日总量给每个候选算
一遍再取最高日费用，而请求级是逐请求选最高后相加。A(input3/output1) 与
B(input1/output3) 下 balanced（两次各 0.5M+0.5M）合计 $4、separated（1M 纯输入
+ 1M 纯输出）合计 $6，日总量相同却给不出任一侧费率——旧实现返回完整 $4。
修复：把「价格候选 × 该时刻适用规则」作为统一择价单元，所有候选放进同一个池
（`judge_scenario`），复用同一套 `could_win`；`day_instants` 改为跨候选求切分
边界；日期/时区不可解析时按「候选 × 世界」组合枚举并封顶 64，超限即全部分项
不可证明（不构造不可能同时成立的规则组合）。`could_win` 增加 rival 去重、单条
支配快判与变量数上限 8——超限返回「可能胜出」，只损失排除精度。

**R10-2 完整性不能只看单价是否存在**。分档估算候选 A（声明 5m/1h、按合并价 $3
估算、complete=false）被当成完整竞争者，压过真正完整的 B（$1）；单候选
（base cache_write $3 + 5m $3 + 1h $6、分档未知）扁平化后恢复成 complete=true
并丢掉 `cache_write_split_estimated`。修复：按支持集实际使用的分项计算语义
完整性（缺价 / 线上按 0 估算 / 写入费按合并价估算都不完整），扁平化后保留
估算标记与不完整状态。

**R10-3 CoInitializeEx(S_FALSE) 未配对**。微软文档：每次成功调用（含 S_FALSE）
都要配对一次 `CoUninitialize`，S_FALSE 只说明此前已初始化、不代表本次不需要
释放。旧代码只对 S_OK 配对。修复后 S_OK / S_FALSE 各恰好一次、初始化失败零次；
错误注释同步更正。

**验证**：round10 10 项全过（含候选换序、逐项支配候选、共有恒定分项、单一
候选、无完整候选对照）；既有计价/日汇总 198 项全绿；scripts 契约 29 项全过
（含真实回收合成树入站核验）。`SNAPSHOT_VERSION` 19 → 20：费用快照是独立轴，
读数变化必须失效旧快照，不能用「同一 PR 不加发行号」代替。版本沿用 0.9.2。

### 第十一轮复核修复（2026-11-18，基线 `feff517`）：日汇总保留「缓存缺价按 0 估算」标记

独立复核 `qa-artifacts/pr70-r11-review/` 给出反例：同一模型两个候选，
`external` 显式 `cache_read=$0`、`provider`（线上来源）缺缓存读价 → 按 0 估算；
日总量 1M 缓存读 + 1M 缓存写有两种真实构成，合计都是 $2：
分两次（1M 只读 + 1M 只写，两侧都完整）与两次各 0.5M 读 + 0.5M 写（无完整候选，
回退到 provider，缓存读按 0 估算、complete=false）。旧实现金额正确（$2）却返回
`cache_price_zero_estimated=false`、`complete=true`。

**根因**：`Ledger::only_assumed_zero` 在任一可能胜出结果提供**显式**零价时变成
false，于是重建费用时把「另一条路径使用估算」的事实一起丢掉。

**修复**：把「金额是否确定」与「依据是否含假设」分开判断——`possible` 一致仍
决定费率可证明；新增 `assumed_zero_used`，只要**任一可能胜出路径**按 0 估算过，
就保留金额与 token（`unknown` 不变），同时置 `cache_price_zero_estimated=true`
并保持 `complete=false`。只有所有可能胜出路径都用显式零价才保持完整计价；被排除
的候选、零用量分项、最终未获覆盖的分项都不产生标记（前者不登记，中者不在 used
里，后者 `proved` 为 `None`）。N18 分档估算标记与既有择价语义不变。

**验证**：`rollup_applicability_round11.rs` 4 项——核心反例先失败在
`cache_price_zero_estimated` 断言，修复后 `cost=2.0`、`unknown.cache_read=0`、
`unknown.cache_write=0`、`complete=false`、`cache_price_zero_estimated=true`，
`Group.total_cost_usd=Some(2.0)` 且标记与合计同步；显式零价对照保持完整计价，
全按 0 估算对照保持不完整 + 标记。既有计价/日汇总 207 项全绿。
`SNAPSHOT_VERSION` 20 → 21（`complete` 与估算标记读数改变，费用快照独立轴）。
版本沿用 0.9.2。

### 第十二轮补齐（2026-11-18，基线 `3ae04d9`）：汇总费用浮层展示「缓存缺价按零估算」

后端在第十一轮已正确置 `Group.cache_price_zero_estimated=true`（合计 $2.00、
未定价 token 全 0、`complete=false`），但共享浮层
`frontend/src/components/GroupCostBreakdownTooltip.vue` 不读该字段，总览卡 /
模型行 / 合计行的费用详情只显示通用估算说明。

补齐：浮层构成区新增两行——`cost.cacheZeroAssumed`（缺失缓存价按 0 估算）与
`cost.cacheZeroAssumedDetail`（线上价格来源未提供该缓存分项单价、按 $0 参与估算、
不是来源明确给出的免费价）。只在 `cache_price_zero_estimated === true` 时显示；
显式零价（标记 false）不显示。金额区域仍只有单一金额，说明只进浮层；悬浮、
键盘聚焦、Escape 与外点关闭交互未改。总览卡、模型行与合计行都经 `CostCell`
复用同一组件，一处修复全覆盖。

**验证**：新增 `GroupCostBreakdownTooltip.test.ts` 4 项——同一份 $2.00、
unknown 全 0 的数据切换估算标记，分别断言浮层有无说明；金额区只有 $2.00；
点击切换 / Escape / 聚焦上报不变。先红（标记 true 时无说明）后绿。浏览器验收
契约 `cost_cell_layout_not_concatenated` 增加 `synthetic-cache-zero` fixture 与
两条断言（浮层披露存在、金额区单金额），并补「显式零价无该说明」对照。前端
79 文件 1155 项 + typecheck/format/build 全过。后端择价、金额、unknown 与
`SNAPSHOT_VERSION`（21）均未改，版本仍 0.9.2。
