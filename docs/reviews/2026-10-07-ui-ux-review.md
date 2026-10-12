# UI/UX 审查问题清单（2026-10-07）

> **2026-10-11 历史状态：** 本文保留当时审查发现；后续修复/需求已有变化，当前完成与待办采用 [总账](../plans/README.md) 和 [整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)。不要把原“仍存在/待执行”直接当作当前缺陷。

> **2026-10-07 后续核实（基线 `dd6aaae`）：** 本文以下内容保留为 `1cefac7` 时的历史审查，执行请以 [独立修复计划及逐项核实表](../plans/archive/partial/2026-10-07-ui-ux-review-remediation.md) 为准。F06 导航、F08 真实 tooltip 测试已由后续提交修复；U02 并非三个 radio 完全无名称，U10 首帧闪烁尚未证实，U18 实际隐藏非日维度 0/1 个真实类别，U21 已有进行中与保存反馈。关闭弹窗已有玻璃材质，合理布局尺寸不能一律视为越阶间距。新一轮真实 App 量测另外确认分段控件外高 **36px**、费用浮层外宽 **512px**，见计划的证据与修复约束。不要照搬本文“只监听 by/mode”的图表建议，也不要全局修改 common 小字号。

**基线：`1cefac7`，分支 `docs/product-review-plan`，前端工作树干净。**
规范依据：根目录 `DESIGN.md`（第二版 Apple + Glassmorphism）。范围：`frontend/src/**` 全部非测试文件（4822 行）+ `index.html` + `src-tauri/tauri.conf.json` 窗口配置 + Naive UI / ECharts 依赖默认值。

本文件是**问题清单，不是修复计划**，未改任何代码。修复任务与提交拆分仍按 `CLAUDE.md`「先计划后执行」另立计划。

## 1. 结论

| 等级 | 数量 | 说明 |
| --- | --- | --- |
| Blocker | 2 新增 + 1 已登记 | 新增 U01（费用金额对读屏不可得）、U02（主题控件字符图标且无可访问名称）；导航吸顶即既有 **F06**，本次在 `1cefac7` 复核确认**仍未修复** |
| Major | 10 | 集中在浮层/表格规格未落地、控件族不统一、图表反复重建、错误状态静默 |
| Minor | 10 | 命名不同源、格式化漂移、焦点跟随、文案与动作不一致 |
| Debt | 5 | 根因是三份色值手工同步且无一致性校验 |

一句话判断：**皮肤层（token、卡片、明暗、降级、动效）基本达标，但"契约要求 → 组件真实渲染值"这一段缺校验，导致表头、浮层、控件高度等成批停在设计稿之外；同时费用这个核心数字对辅助技术是不可读的。**

## 2. 与既有计划的关系

| 既有编号 | 位置 | 本次复核结论 |
| --- | --- | --- |
| **F06**（导航滚动容器无高度约束，吸顶失效） | [修复后复核遗留计划](../plans/archive/implemented/2026-10-07-post-remediation-recheck-fixes.md) Task 6，状态"待执行" | **确认仍在**。计算样式实测：`.app-shell` display=block、`.scroll-container` clientHeight=scrollHeight=1484px、`html/body` overflow=visible → 容器自身永不滚动，sticky 的 scrollport 不动，导航随 document 滚出视口。不重复编号，按 Task 6 执行即可 |
| **F08**（NTooltip 测试全局打桩、可见性未验） | 同上 Task 7 | 仍成立；另见本文 **U13**（`.metric-label-help` 缺焦点环类） |
| R07 / R08 / R09 / R10 | 上一轮修复 | 已核实真修复：关窗失败保留弹窗与原因且防重复提交（`App.vue:76-88`）、采集诊断独立呈现（`Dashboard.vue:113-135` 等）、命中率浮层已响应 focus/click/Escape（`SummaryCards.vue:75-99`）、明细行身份用后端游标（`EventTable.vue:215`） |
| `2026-10-06-design-system-visual-qa.md §2.1` | 勾选项"滚动页面：吸顶导航可见模糊 ✅" | **与实现冲突**，随 F06 修复一并回写，不能保留误导后续执行者的通过记录 |

## 3. Blocker

### U01 · 明细费用列把金额从可访问名称里吞掉

- **位置**：`frontend/src/components/EventTable.vue:135-141`
- **观察**：`h("span", { role: "button", "aria-label": "费用计算明细", class: "ts-focusable" }, fmtPrice(c))`。`aria-label` 覆盖可见文本，读屏在本行只播"费用计算明细"，**丢掉金额本身**；浮层本体无 `id`，全仓无 `aria-describedby` / `aria-live`，"事实→公式→结果→来源"对辅助技术完全不可达。
- **期望**：WCAG 4.1.2（名称/角色/值）、2.5.3（标签在名称中）；`DESIGN.md §6`"浮层可通过 focus/click 读取""文本、状态和图表不能只靠颜色区分"。
- **影响**：估算是本产品的核心结论，费用列在唯一的逐请求证据表上对读屏用户退化成一个无值按钮。R09/`244d6cc` 修好了"能打开"，没修"读得到"。
- **修法**：移除 `aria-label`，或写成含值的名称（`费用 $0.0126，查看计算明细`）；给浮层内容 `id` 并挂 `aria-describedby`；保留现有 hover/focus/click/Escape 行为。
- **验证**：真实 NTooltip/FPopup 挂载测试断言可访问名含格式化后的金额，且 `aria-describedby` 指向实际渲染出的浮层节点（不得再用透传 stub）。

### U02 · 主题切换用字符当图标，三个 radio 无可访问名称

- **位置**：`frontend/src/App.vue:46-50`（`label: "☀"` / `"☾"`）→ `frontend/src/components/SegmentedControl.vue:88`（原样渲染文本）
- **观察**：主题段的三个 radio 可访问名分别是"☀""☾""自动"，读屏念作无名符号；视觉上是字符而非线性 SVG。同一条导航栏的品牌位已经用了 SVG（`App.vue:112-126`），自相矛盾。
- **期望**：`DESIGN.md §1`「不用 emoji 当图标；用现有图标组件或 SF Symbols 风格的线性 SVG」、`§3`「图标旁有文字或可访问名称」、`§5`「主题切换用三段分段控件（图标 + 可访问名称）」。
- **连带**：`frontend/src/App.test.ts:141` 断言 `["☀","☾","自动"]`，**把违规钉成了契约**，改实现时必须同步改断言，否则门禁会反向锁死问题。
- **修法**：新增浅色/深色两个 16px 线性 SVG（线宽 1.5、圆角端点），用 `SegmentedControl` 已有的 `icon` 槽（`:85-88`）渲染；"自动"保留文字，三项各自补明确 `aria-label`。
- **验证**：`pnpm --dir frontend test src/App.test.ts src/accessibility.test.ts`，断言每项可访问名为完整中文词且不含符号字符。

## 4. Major

### U03 · 浮层不是 85% elevated 玻璃，且注释与实现不符

- **位置**：`frontend/src/styles/naiveTheme.ts:37,58`（`elevated: "#FFFFFF"` / `"#2C2C2E"` 实色）→ 用于 `:88 modalColor`、`:89 popoverColor`、`:171 Tooltip.color`；对比 `frontend/src/styles/tokens.css:14`（`--ts-surface-elevated: rgba(255,255,255,.85)`）；`frontend/src/styles/chartTheme.ts:66`（`tooltipBg` 同样实色）；`frontend/src/components/EventTable.vue:126-129`
- **观察**：Naive 侧浮层底色是不透明实色，`EventTable` 手工挂的 `backdrop-filter: var(--ts-glass-blur-popover)` 在实色之上**完全无效**；`:127` 注释却写"背景色来自 --ts-surface-elevated 85%"。`EventTable.test.ts:302` 只断言 style 字符串存在，属弱证据。
- **期望**：`DESIGN.md §2` 浮层 = 不透明度 85% + 模糊 16px + `0 12px 48px` 阴影；`§5 图表/费用明细` 同要求。
- **影响**：菜单、tooltip、弹窗、日期面板四类浮层实际全部脱离玻璃语言，与卡片的层次关系变成"实色块压在玻璃上"；注释与测试一起掩盖了这点。
- **修法**：给 Naive 传带 alpha 的值（其主题接受 rgba 字符串），或统一给浮层加 `.ts-popover` 公共类承载 85% + 模糊 + elevated 阴影，并删除误导注释。
- **验证**：真实浏览器读取 popover 计算样式，`background-color` alpha < 1 且 `backdrop-filter != none`（与 F06 Task 6 的浏览器脚本共用一套环境）。

### U04 · 表头与表格正文的字号/颜色规格整体没落地

- **位置**：`frontend/src/styles/naiveTheme.ts:156-169`（DataTable 只覆盖 `borderColor/borderRadius/thColor/tdColor/tdColorHover/padding/thFontWeight`，**未设** `thTextColor`、`fontSizeSmall`）
- **依赖默认值**：`node_modules/naive-ui/es/_styles/common/_common.mjs:15` → `fontSizeSmall: "14px"`；`node_modules/naive-ui/es/data-table/styles/light.mjs:54` → `thTextColor: textColor1`
- **观察**：两张表都用 `size="small"`（`UsageTable.vue:147`、`EventTable.vue:238`），于是表头是 **14px + 主文字色 `#1D1D1F`**，正文 14px；设置页价格表同错。行高约 42px 而非 40px。
- **期望**：`DESIGN.md §3` 表头/标签 12px/500 `--ts-text-secondary`，表格正文 13px/400/行高 1.45；`§5 表格` 重申同值。
- **影响**：这是全站面积最大的一类元素，"数字清晰、层级分明"的观感主要来自这里；表头与正文同色同字号直接导致 §7 用户反馈过的"没有层次"。
- **修法**：`thTextColor: c.textSecondary` + `common.fontSizeSmall: "13px"`；表头 12px 需要 Naive 无对应项，收进 `tokens.css` 的公共类（如 `.ts-table .n-data-table-th { font-size: 12px }`），**不要**在页面里写 `:deep()`。
- **验证**：浏览器读取 `.n-data-table-th` 计算 `font-size`/`color`，以及 `td` 的 `font-size` 与行高。

### U05 · 合计行字重 600 是死选择器

- **位置**：`frontend/src/components/UsageTable.vue:192`（`.total-row strong { font-weight: 600 }`）
- **观察**：`:107` 只给行加 `total-row` 类，而 `:31-85` 各列渲染的是 `span` 或纯文本，DOM 里根本没有 `strong`；同文件 `:159` 的 `<strong>` 在空状态模板里，与合计行无关。合计行上方的 `--ts-separator-strong`（`:189`）生效，字重不生效。
- **期望**：`DESIGN.md §5 表格`「合计行字重 600，上方一条 `--ts-separator-strong`」。
- **影响**：整张表最需要跳出来的那一行与普通行同样轻重。
- **修法**：把 `total-row` 的单元格文本包进 `strong`，或直接 `:deep(.total-row td) { font-weight: 600 }` 收进公共类。
- **验证**：真实挂载后读取合计行单元格计算 `font-weight`，断言为 600（不得只检查选择器字符串存在）。

### U06 · 日期快捷项固定 34px 宽，文案被裁

- **位置**：`frontend/src/components/DateRangeSelect.vue:207-217`（`.shortcut-btn { width: 34px; height: 34px; font-size: 12px; border-radius: 0 }`）+ `:200-206`（`.shortcut-row { overflow: hidden }`）+ `:29-36`（标签含"近14天""近30天"）
- **观察**：12px 下 4 个字符约 48px、3 个字符约 36px，都超过 34px 定宽，Naive 按钮文本 `white-space: nowrap`，父行 `overflow: hidden` → **文字被截断**。仅"当天"能容下。
- **期望**：`DESIGN.md §5 筛选栏`「宽度不足时整组换行，不压缩到不可读」；`§3` 辅助信息不得低于 12px；`§6` 缩放不得截断核心操作。
- **连带**：高度 34px 与圆角 0 既不在 32px 控件族也不在 8px 半径族，与同面板其它控件不同族。
- **修法**：去掉固定宽度改用水平 padding，或复用现成的 `SegmentedControl`（多选一场景本就要求统一）。
- **验证**：980×620 与 150% 缩放下读取每个按钮 `scrollWidth <= clientWidth`，文本完整。

### U07 · 筛选栏控件不同族（28px 与 32px 混排）

- **位置**：`frontend/src/views/Dashboard.vue:341-351`（时区 `NSelect size="small"`）→ `frontend/src/styles/naiveTheme.ts:152-153` `heightSmall: "28px"`；同一行日期触发器 32px（`DateRangeSelect.vue:117` + `:176-183`）、分段控件 32px（`tokens.css:241`）
- **期望**：`DESIGN.md §5 筛选栏`「所有控件高度 32px、底色 `--ts-fill`、无描边、圆角 8px，视觉上是同一族」。
- **影响**：一行里出现两种高度，正是"筛选栏乱"的残留形态。
- **修法**：时区改 `size="medium"`（已是 32px）；并把"筛选栏控件一律 32px"写成可断言的不变量（同 U06 一起收进公共类或组件 props 校验）。
- **验证**：浏览器读取来源段、维度段、日期触发器、时区选择器的 `getBoundingClientRect().height` 全等于 32。

### U08 · 趋势图在父级每次渲染时被销毁重建

- **位置**：`frontend/src/views/Dashboard.vue:499`（模板内 `report.groups.filter(...)`，每次渲染生成新数组）→ `frontend/src/components/TrendChart.vue:155`（`watch(() => [props.groups, props.by, mode.value], render)`，getter 返回新数组，恒判定为变化）→ `:42-52`（`render()` 内 `chart.dispose(); echarts.init()`）
- **观察**：状态胶囊"刷新中↔已更新"切换、点"数据摘要"、下钻、任何 Dashboard 局部重渲染都会重建整个 ECharts 实例。
- **期望**：`DESIGN.md §6` 动效只用于颜色/透明度/选中块平移与高度变化；隐含着不应有整块重绘闪白。
- **影响**：图表闪白、dataZoom 与 tooltip 状态丢失、大区间下重复初始化的开销直接落在主线程 UI 上；用户"滚回去看同一个柱子"的连续操作被打断。
- **修法**：把过滤结果提到 `computed` 以保持引用稳定；`watch` 只监听 `by` 与 `mode`；数据变化用 `chart.setOption(..., { notMerge: false })` 增量更新，`dispose` 只留给卸载与主题切换。
- **验证**：测试注入一次状态胶囊文本变化，断言 `echarts.init` 调用次数不增加、`setOption` 增加；`onBeforeUnmount` 仍恰好 dispose 一次。

### U09 · 设置页首屏加载失败静默，禁用控件无原因无重试

- **位置**：`frontend/src/views/Settings.vue:77-87`（`loadAll` 只有 `try/finally`，无 `catch`）+ `:221` 调用点 `void loadAll()`；`:188` `autostart_status` 同型；失败后果见 `:366` `:disabled="autostart == null"`
- **观察**：任一 `invoke` 失败 → `sources/cache/pricing` 停在 null、草稿不加载 → 页面按"未加载"渲染（`—`），自启开关永久禁用，**没有任何错误说明或重试入口**。
- **期望**：`DESIGN.md §6` 六种状态含 error；`§5 状态提示`「数据源异常…必须可见」+ 内联通知条 + 右侧文字操作。
- **影响**：用户看到的是"设置页好像坏了但不知道为什么"，而且这是唯一能修数据源配置的入口。
- **修法**：`loadAll` 分来源捕获，写 `settingsError`；页顶用 `.ts-notice` 展示原因 + "重试"文字动作；禁用控件必须解释为何禁用。
- **验证**：mock IPC reject，断言出现可聚焦的重试动作且开关区域给出原因文本（不是只断言 DOM 存在）。

### U10 · 深色主题首帧闪烁

- **位置**：`frontend/index.html`（无预涂 `data-theme` 的内联脚本，`tokens.css` 经 `main.ts:2` 注入）→ `frontend/src/App.vue:30-32`（挂载后的 `watchEffect` 才写 `html.dataset.theme`）；`src-tauri/tauri.conf.json` 窗口项只有 `width/height/minWidth/minHeight/center`，无 `backgroundColor`/`theme`
- **观察**：`:root` 默认浅色（`tokens.css:7-95`），`color-scheme: light`；深色用户在 Vue 挂载前先画一帧浅色画布，WebView 无背景色兜底。
- **期望**：`DESIGN.md §2`「浅色和深色是两套同等完整的主题」；视觉 QA 记录 §3 也写了"无首帧闪烁"，该结论仅覆盖 Naive 主题，未覆盖 CSS token 首帧。
- **修法**：`index.html` 内联脚本读 `localStorage['tokenscope-theme']`（含 `system` → `matchMedia`）预涂 `data-theme` 与 `color-scheme`；Tauri 窗口配 `backgroundColor`（浅/深两值按当前主题，或至少近黑），必要时设 `theme`。
- **验证**：深色系统 + 冷启动录屏/逐帧检查首帧无浅色闪；`tauri dev` 启动观察窗口底色。

### U11 · 卡片里再套带边框卡片

- **位置**：`frontend/src/styles/tokens.css:325-340`（`.ts-notice` 自带 `border` + inset 高光 + `--ts-shadow-card`）被放进 `.ts-card` 内：`frontend/src/views/Settings.vue:438`、`:515-519`、`:540`
- **期望**：`DESIGN.md §5 卡片`「禁止在卡片里再套带边框的卡片；需要分区时用 `--ts-separator` 发丝线」。
- **影响**：玻璃套玻璃、双层描边双层阴影，通知条在卡片内显得比卡片本身更"重"，视线被打断。
- **修法**：加 `.ts-notice--inline` 变体（无阴影、无外描边，仅左侧状态色图标 + 发丝线或 `--ts-accent-soft` 底），卡内一律用它；卡外（横幅、页面顶）保留现有配方。
- **验证**：视觉快照确认卡内通知无外阴影；`Settings.test.ts` 断言卡内 notice 带 inline 类。

### U12 · 设置页说明信息只能 hover 取得，并用 opacity 压低

- **位置**：`frontend/src/views/Settings.vue:242-256`（单价/分段 tooltip 的触发 `span` 无 `tabindex`、无 `ts-focusable`，对照 `EventTable.vue:56` 有 → 说明团队已认可该规范）、`:253`（`style="opacity: 0.7"` 作用在 12px 文本上）、`:250`/`:279` 模板内联 style
- **期望**：`DESIGN.md §1`「不依赖 hover 才能完成操作；hover 信息都要有 focus/click/键盘等价路径」「不用 `opacity` 压低重要信息」；`§6` 键盘与对比度。
- **影响**：单价与分段是判断"这个价对不对"的关键说明，键盘用户完全读不到；12px 文本再压 70% 透明度，`--ts-text-secondary` 的对比度余量被吃掉。
- **修法**：补 `tabindex="0" class="ts-focusable"` 与与 `EventTable` 一致的 focus/click 处理；`opacity: .7` 换成 `--ts-text-secondary`/`--ts-text-muted` 语义色。
- **验证**：键盘 Tab 到该说明并断言浮层可见；文本色来自语义 token（无 opacity 修饰）。

## 5. Minor

| 编号 | 问题 | 位置 | 期望与修法 |
| --- | --- | --- | --- |
| U13 | 焦点环覆盖不一致：契约要 2px `--ts-accent`，实现只有 `.ts-focusable:focus-visible`，而四组分段控件与命中率说明都缺该类（页面样式表实测无对应 `:focus` 规则，仅剩浏览器默认环） | `tokens.css:362`；`SegmentedControl.vue:80`；`SummaryCards.vue:78` | `DESIGN.md §6`；给 `.ts-segmented-item` 与 `.metric-label-help` 加 `ts-focusable`，或在 tokens 内为二者写焦点规则 |
| U14 | 系列命名不同源："缓存读" vs "缓存命中" | `chartTheme.ts:31`、`chartData.ts` 对 `SummaryCards.vue:38` | `DESIGN.md §92` 图例/分项条/表头色点同源；统一为一个词（建议"缓存读"，与后端 `cache_read` 对齐） |
| U15 | 金额格式化三套，同名列精度不一致 | `EventTable.vue:21-26`、`types.ts:284 fmtCost`、`costBreakdown.ts:49` | 收敛为单一 `fmtCost`，各处只调用它 |
| U16 | 方向键切换后焦点不跟随选中（roving tabindex 漂移） | `SegmentedControl.vue:52-62` + `:82` | 切换时把焦点移到新选中项，或 tabindex 依焦点项而非选中项 |
| U17 | 选中块只在挂载/切换时测量，无 ResizeObserver | `SegmentedControl.vue:27-46` | `DESIGN.md §7` 要 100/125/150% 缩放验收；缩放、筛选栏换行、字体加载后会错位 |
| U18 | ≤2 类别时趋势图整块消失；`chartStateText(0)`（"暂无数据"文案）在当前渲染路径下几乎不可达（空组已由上层空状态接管，仅"只剩合计行"这一极端情形能命中） | `Dashboard.vue:498`、`chartData.ts:71-72` | 改为类别过少仍出图，或明确"类别过少不绘图"的说明文案；顺带决定空文案去留 |
| U19 | 横幅文案写"可重试"却没有重试动作；`syncError` 与 `statusFailed` 同时成立时"读取失败"提示被"部分失败（主源已可用）"分支吞掉 | `PricingStatusBanner.vue:98-103`（`:85 v-if` 与 `:103 v-else-if` 互斥） | 补重试按钮；两个条件独立成块或按优先级合并为一条可展开通知（`§5 状态提示`） |
| U20 | 日期触发器标签丢年份，跨年显示"12/28 ~ 1/3" | `DateRangeSelect.vue:45-48` | 跨年或非同一年时补年份；区间口径是统计正确性的一部分 |
| U21 | 危险/长耗时操作缺预期：重建缓存（本机 1.2 GB ≈ 5.5 s）无耗时说明与确认；改关窗动作即时写盘 | `Settings.vue:493`、`:46-51` | 行内说明耗时量级 + 进行中态；即时写盘的项给"已保存"反馈 |
| U22 | 目录输入框无可访问名称（用 `placeholder` 兼作），长默认路径截断后无处看全值 | `Settings.vue:432-437`、`:481-484` | `aria-label` 独立于 placeholder；技术详情区补该目录全值 |

## 6. 设计债

| 编号 | 债 | 位置 | 建议 |
| --- | --- | --- | --- |
| D1 | 缺 `--ts-warning-soft` / `--ts-success-soft` / `--ts-error-soft`，胶囊底色写成 rgba 字面量并需额外 `[data-theme]` 分支；组件内同类硬编码 | `tokens.css:299,303,306,310`；`CloseConfirmDialog.vue:126,132` | 补三个 soft 语义 token，两主题各一值，删除分支选择器与组件内字面量 |
| D2 | **三份色值手工同步（`tokens.css` / `naiveTheme.ts` / `chartTheme.ts`）却无任何一致性校验** —— U03、U04 就是这个缺口产出的 | `naiveTheme.ts:1-75`、`chartTheme.ts:36-69` 的自述注释 | 加一条 vitest：以 `tokens.css` 为唯一来源解析出语义色表，逐键比对两个 adapter；不一致直接红。这是本清单里性价比最高的一项 |
| D3 | 死配置/死码：Naive `Card` 覆盖块无对应使用（全仓 `NCard` 使用数为 0）；`echartsThemeObject` 无引用 | `naiveTheme.ts:127-131`；`chartTheme.ts:73` | 删除，或改为真实使用点（若保留 NCard 路径则需补玻璃模糊，否则它与 `.ts-card` 会长出两种样子） |
| D4 | 9 处模板内联 `style="…"` 与页面级 `:deep()` 覆盖，浮层/表格样式散在业务组件里 | `EventTable.vue:128-129`、`UsageTable.vue:189`、`Settings.vue:250,253,279`、`Dashboard.vue:487` 等 | 收进 `tokens.css` 公共类（`.ts-popover`、`.ts-table`），让 U03/U04 的修正在一处生效 |
| D5 | 越阶数值不在 `4/8/12/16/20/24/32` 与 `§3` 字阶上 | `Dashboard.vue:487`(380px)、`:617`(160px)、`:529`(15px/650)、`TrendChart.vue:39`(320/34/70 魔数)、`SummaryCards.vue:161`(48px)、`:143`(200px)、`Settings.vue:272`(6px)、`:339`(380px)、`:580`(表高 420) | 归到标度或写进 tokens；15px/650 若确需，先在 `DESIGN.md §3` 增字阶再消费 |

## 7. 已核实合规（不必重复排查）

- `.vue` 组件内**零硬编码十六进制**（实测 grep 无命中）；色值集中在三个适配层。
- 卡片体系未混用：全站 `.ts-card` 18 处，`NCard` 0 处 → `§5`「两者不得混用出两种样子」目前成立。
- `@supports not backdrop-filter` 退化 95% 实色齐备（`tokens.css:143-154`）；`prefers-reduced-motion` 归零动效（`:156-162`）。
- 图表显式传 `color` 数组，未用 ECharts 默认调色板（`TrendChart.vue:79-82`）；绘图区容器 `.ts-card-solid` 实色衬底在位（`:182` 起）。
- 前端不重算价格：`costBreakdown.ts:94` 直接用后端 `subtotal/cost_usd`；全缺价 →"无法估算"；分段命中档位、峰谷时间档、保守估算说明均在（`:68`、`:71-73`、`:130`）。
- 未知 ≠ 0、极小金额给 6 位小数（`tieredPrice.ts:7-13`、`costBreakdown.ts:51`）。
- 指标卡 44px/-0.03em、26px/-0.02em、费用不染强调色、发丝线分隔、比例条 6px/3px + 空槽 + 图例（`SummaryCards.vue:185-225`）。
- 筛选栏顺序来源→维度→日期→时区成立（`Dashboard.vue:341-351`），分段控件 radiogroup 语义 + 方向键环绕成立（`SegmentedControl.vue:70-83`）。
- 设置页分组与 macOS 结构成立：组标题 13px/600 在卡外（`Settings.vue:610-616`）、行式左标签右控件 + 发丝线（`:619-626`）、路径与同步时间收进 `NCollapse`（`:479-491`）、来源身份标签用中性色。
- 窗口尺寸符合 `§4`：`tauri.conf.json` 1280×820 / min 980×620。

## 8. 验收缺口（本次也无法闭合）

1. **截图证据仍为零。** `docs/plans/active/screenshots/2026-10-06/` 目录不存在；本次尝试渲染时，可用的浏览器工具没有可见视口（viewport 0×0、`visibilityState=hidden`），`tauri dev` 的 WebView2 也无法被截图工具接管。因此**所有"像素级观感"结论仍只有代码与计算样式支撑**。
2. `§7` 要求的 100% / 150% 系统缩放、`D5` 安装/升级/卸载验收仍未执行（按既有决策后延，本清单不冒充完成）。
3. 需要真实渲染才能判定的条目：U03（浮层 alpha）、U04（表头计算字号色）、U05（合计字重）、U06（文本截断）、U07（控件同高）、U17（缩放后选中块错位）。这些恰好都是 `frontend/scripts/check-app-scroll.mjs`（F06 Task 6）可以顺带覆盖的量测——**建议把那个脚本扩成"契约量测脚本"，一次解决 U03–U07 与 F06 的取证。**

## 9. 建议执行顺序

| 批次 | 内容 | 为什么在这个位置 |
| --- | --- | --- |
| 1 | **F06**（导航吸顶，走既有 Task 6）+ **D2**（token 一致性测试）+ **U08**（图表重建） | 前者是玻璃语言成立的前提；D2 是防止 U03/U04 类偏差再生的机制；U08 影响每一次交互的手感 |
| 2 | **U01 + U02 + U13**（可访问名称与焦点环，含 `App.test.ts:141` 反向断言） | 辅助技术路径 + 把"钉住违规的测试"纠正过来 |
| 3 | **U04 + U05 + U03 + U07 + U06**（表格/浮层/控件族的真实渲染值） | 一批"契约写了但没落地"的规格，改完视觉层次立刻不同 |
| 4 | **U09 + U10 + U12 + U19**（错误状态、首帧、hover-only、文案与动作一致） | 状态完整性与信任感 |
| 5 | **U11 + Minor 批 + Debt 批** | 收口与防再生 |

## 10. 完成定义（待对应修复计划消费）

- [ ] F06：真实浏览器量测导航吸顶有效（`header.top` 恒为容器顶边、容器 `scrollTop` 可 > 0），并回写视觉 QA §2.1 的错误通过记录
- [ ] U01/U02/U13：费用列可访问名含金额且浮层可被引用；主题控件为 SVG + 中文可访问名；分段与说明有 2px 焦点环
- [ ] U03–U07：浮层 alpha < 1 且有模糊；表头 12px/`--ts-text-secondary`、正文 13px、合计行 600；日期快捷项文本不被裁；筛选栏四项同高 32px
- [ ] U08：状态胶囊变化时 `echarts.init` 不再被调用
- [ ] U09/U10/U12/U19：加载失败可见可重试；深色首帧无闪；说明信息可键盘读取且无 opacity 压低；横幅文案与动作一致
- [ ] D2 落地为测试（不一致即红），D1/D3/D4/D5 收敛到 tokens
- [ ] 补齐脱敏截图存档；100%/150% 缩放与 D5 若仍后延，明确写"待验"，不勾选

---

### 取证方法（便于复核）

1. 通读 `DESIGN.md` 与 `frontend/src/**` 全部非测试文件，再读对应 `.test.ts` 判定证据强度（透传 stub、只断言属性或 style 字符串存在、向 stub 喂自身输出再验互逆的，均记为弱证据并在对应条目注明）。
2. 与 Naive UI 默认值核对：直接读 `node_modules/naive-ui/es/**` 的 `_common.mjs` 与组件 `styles/light.mjs`，确认未被 `themeOverrides` 覆盖的项实际取什么值（U04 的依据）。
3. 运行时实测（vite dev 1420 + 计算样式）：`.app-shell` display、`.scroll-container` clientHeight/scrollHeight、`html/body` overflow、body 氛围光斑与 `background-attachment`、页面样式表中针对 `.ts-segmented-item` / `.metric-label-help` 的 `:focus` 规则存在性。取证后已关闭 dev 服务，仓库未因此改动。
4. 统计类事实（`NCard` 使用数 0、`.vue` 内硬编码 hex 0 处、模板内联 style 9 处、`echartsThemeObject` 引用 0）以 grep 计数为准。
