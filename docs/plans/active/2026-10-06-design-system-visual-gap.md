# 第二版视觉差异清单（Apple Glass 落地任务 0）

> 基线：分支 `docs/product-review-plan` @ fd07c6e（工作区 clean）。
> 本文按现行 `DESIGN.md`（第二版，苹果风格 + Glassmorphism）逐项对照当前实现，
> 作为 `2026-10-06-design-system-implementation.md` 各任务的差异依据。
> 不凭旧计划的执行记录判断完成度，全部以当前代码为准。

## 1. 当前实现基线

| 层 | 现状 |
| --- | --- |
| `styles/tokens.css` | v2 Apple Glass 已就位：`--ts-canvas #F5F5F7/#0F0F11`、`--ts-surface(.75)`、`--ts-surface-solid`、`--ts-surface-elevated(.85)`、`--ts-glass(.72)`、`--ts-fill(-hover)`、`--ts-segment-thumb`、separator、accent `#0066CC/#4DA3FF`、状态色、`--ts-chart-*` 四系列色、`--ts-ambient-1/2/3` 柔光、玻璃 stroke/highlight、8/6/14/12/999 圆角、160–240ms 动效、`@supports not backdrop-filter` 95% 降级、`prefers-reduced-motion` 归零；公共类 `.ts-card` / `.ts-card-solid` / `.ts-glass` / `.ts-segmented` / `.ts-pill(-info/-warning/-success)` / `.ts-notice` / `.ts-num` / `.ts-mono` / `.ts-focusable` |
| `styles/naiveTheme.ts` | 与 tokens.css v2 同值同步（body 透明、卡片玻璃底、表格实色、输入 fill 底、圆角 8/14/12、状态色映射） |
| `styles/chartTheme.ts` | `chartTokens(mode)` 固定四系列色/顺序，`echartsThemeObject` 可选；TrendChart 用 `chartTokens` + setOption `color` 数组覆盖默认调色板 |
| `composables/theme.ts` | `light\|dark\|system` 偏好、`tokenscope-theme` 旧值兼容、系统主题监听只影响 system、解析后 `mode`；App.vue `watchEffect` 写 `html[data-theme]` |
| `App.vue` | 52px sticky `.ts-glass` 导航 + 品牌 `◉ TokenScope` + 页面/主题两个 SegmentedControl（radiogroup、方向键、aria-checked）、banner-slot、内容 1200px 居中、≤1100px 边距 20px |
| `SegmentedControl.vue` | 通用分段控件：底槽 `.ts-segmented`、选中块平移 200ms、ArrowLeft/Right/Up/Down、`tabindex` roving |
| 组件层 | Dashboard/SummaryCards/TrendChart/UsageTable/EventTable/Settings/PricingStatusBanner/DateRangeSelect 仍是第一版结构（详见 §2） |

## 2. 与 DESIGN.md v2 的差异表

| # | DESIGN.md 要求 | 现状 | 差异 | 归属任务 |
| --- | --- | --- | --- | --- |
| 1 | 画布三团静态柔光 | `body` 已挂 `--ts-canvas-aurora`（fixed、不动画） | 无差异 | — |
| 2 | 52px 吸顶玻璃导航 + 发丝线 | `App.vue` `.ts-glass` sticky 52px | 无差异（验收时确认滚动模糊） | 8 |
| 3 | 页面/主题切换 = 分段控件（浅/深/自动三项） | 两个 SegmentedControl，主题三项 ☀/☾/自动 | 基本达标；主题项用字符图标，验收确认可读性 | 2/8 |
| 4 | 来源筛选 = 图标 + 文字分段控件，命中区 ≥32px | Dashboard `.agent-row` 是 40×40 纯图标 `NButton` 方块（无文字） | 差异：改为 SegmentedControl + AgentIcon + 文字 | 2 |
| 5 | 聚合维度 = 分段控件 | `NRadioGroup + NRadioButton`（Naive 默认外观） | 差异：换 SegmentedControl 统一外观 | 3 |
| 6 | 页面大标题 28px/700 + 时区/日期副标题 + 标题行右侧状态胶囊与刷新 | 标题 22px；无状态胶囊（"刷新中/缓存数据"是数据区上方 `NTag`） | 差异：标题规格、状态胶囊上移标题行 | 3 |
| 7 | 筛选栏统一 32px、`--ts-fill` 底、无描边、8px 圆角；日期控件无 emoji | DateRangeSelect 触发器含 `📅` emoji + scoped `rgba(128,128,128,.18)` 硬编码；NSelect/NRadioGroup 为 Naive 默认皮 | 差异：控件族统一、去 emoji、去硬编码 | 3 |
| 8 | 异常提示 = `.ts-notice` 内联通知（图标 + 结论 + 文字操作，多条可合并） | Dashboard 用 `NAlert`（汇总/明细错误、来源四态）；Settings 用 `NAlert`；PricingStatusBanner 是 `.ts-glass` + 3px 左边框自绘 | 差异：三处统一改 `.ts-notice` | 3/7 |
| 9 | 指标卡 = 一张 `.ts-card`：左费用 44px + `USD · 估算值，非账单`，右三读数 26px + 发丝线，未计价警告胶囊，命中率公式进 tooltip，底部 6px 分项比例条 + 图例 | `.metric-strip` 四列 grid（32px 读数），费用染 accent 蓝，公式直铺 unit 行，未知标记是文字 `†`，无比例条，无卡片 | 差异：整卡重构 | 4 |
| 10 | 趋势图入卡：绘图区透明、网格 separator、无轴线、12px muted 轴字、柱顶圆角 4px、圆点图例、类别多不静默丢弃、"数据摘要"保留 | 轴线已去、轴字 12px、dataZoom/摘要已有；但无卡片外壳、legend 非圆点、柱无圆角、摘要/按钮引用已删除变量 | 差异：卡化 + 圆角 + 图例 | 5 |
| 11 | 表格入卡：无竖线无外框、separator 行线、表头 12px/500 无底色、行高 40px、hover 仅 `--ts-fill-hover`、可点击行 `›`、合计行上 strong 线、键盘 focus 2px | UsageTable `bordered=true` + `single-line=false`（重网格）、行仅 cursor:pointer（无键盘/›/粗线）；EventTable `bordered=false` 但模型列外的键盘路径不全；两表行高非 40px | 差异：表格皮统一 + 行交互 | 5 |
| 12 | 费用明细 tooltip：hover/focus/click 打开、Escape/点外关闭、elevated 玻璃 85%+16px blur、公式区 `.ts-card-solid`、事实→公式→结果→来源、≤480px | 交互路径已齐（manual NTooltip + openKey + 键盘）；但浮层是 Naive 默认皮、公式区无实色衬底、样式引用已删除变量 | 差异：浮层视觉 | 6 |
| 13 | 设置页四组（应用/数据源/缓存/价格）每组一张卡、组标题在卡外 13px/600、行=左标签+右控件、行间发丝线、技术详情折叠、来源标签中性色 | NCard 标题在卡内 + NGrid 两列拼贴；来源身份中性色已做；警告用 NAlert；`#f0a020` 硬编码（未知价） | 差异：结构重排 + 去 NAlert/去硬编码 | 7 |
| 14 | 引用已删除 token（`--ts-border`、`--ts-radius`、`--ts-radius-lg`）的文件 | Dashboard、SummaryCards、TrendChart、EventTable、DateRangeSelect、PricingStatusBanner 六处（第一版变量名，v2 已更名 separator/radius-control 等） | 差异：随各任务迁移时替换 | 3–7 |
| 15 | 状态六态（loading/stale/empty/partial/error/no-price）可见且局部化 | loading 首载 NSpin、之后保数据；stale/empty 已有；错误 NAlert；no-price 全局横幅 | 部分达标，样式统一到胶囊/通知条 | 3/7 |
| 16 | 1280×820 / 980×620、100/125/150% 缩放、键盘全路径 | 布局容器达标；真机走查未做 | 差异：验收执行 | 8 |

## 3. 基线门禁（任务 0 Step 3）

- `pnpm --dir frontend typecheck`：✅ 通过
- `pnpm --dir frontend format:check`：✅ 通过
- `pnpm --dir frontend test`：✅ 14 文件 107 用例全绿

与本计划无关的并行改动：无（工作区 clean）。

## 4. 结论

任务 1（token/适配层）与任务 2（应用壳/分段控件）的主体已由 a8788aa、5cd362c、b76809c
落地且与现行 DESIGN.md 一致；剩余差异集中在组件层（§2 表 #4–#14），
按实施计划任务 2→3→4→5→6→7 顺序逐项消除，任务 8 做真机验收。
