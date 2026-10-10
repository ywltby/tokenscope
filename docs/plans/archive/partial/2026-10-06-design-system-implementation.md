# TokenScope Apple Glass 视觉设计落地实施计划

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| Task 0–7 | 实现及后续修复已有；Task 6 为费用浮层，Task 7 为设置/状态 | — |
| Task 8 | 历史自动化通过；当前原生组合与系统走查未全部闭合 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11)、[N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-06-design-system-implementation.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按第二版 `DESIGN.md` 将 TokenScope 重构为 Apple Big Sur / Monterey 风格的玻璃拟态桌面界面，在浅色、深色和自动主题下保持清晰的信息层级与费用可追溯性。

**Architecture:** 先建立唯一的 CSS 语义 token、公共 `.ts-card` / `.ts-segmented` / `.ts-notice` 样式、Naive UI 主题覆盖和 ECharts 主题适配器，再按应用壳、汇总布局、指标卡、图表、表格、费用浮层、设置页和视觉验收逐层迁移。前端只消费现有后端数据和费用 breakdown，不重新计算价格、不改变统计公式、不改变缓存和日期逻辑。

**Tech Stack:** Vue 3、TypeScript 5、Naive UI、ECharts 6、Vitest、Vite/Tauri 2；本机系统字体、CSS `backdrop-filter`、CSS media query；不引入在线字体和新的 UI 框架。

---

## 执行前的硬约束

- 实现前必须阅读根目录 `DESIGN.md`，本文只负责把第二版规范拆成可执行任务。
- 第二版视觉方向是 Apple Glass：`#F5F5F7/#0F0F11` 画布、系统蓝、静态三团柔光、半透明玻璃和实色数据区。不得沿用第一版的石墨灰/青色方案。
- 玻璃透明度不得低于 70%，模糊不得超过 20px；不支持 `backdrop-filter` 时退化到 95% 不透明表面。
- 所有卡片统一为 `.ts-card`：14px 圆角、20px 内边距、统一描边/顶部高光/阴影；禁止在卡片内再套另一种带边框卡片。
- 表格、图表绘图区、费用公式区使用 `--ts-surface-solid` 或等价高不透明表面，不能依赖透明背景保证可读性。
- 不使用 emoji、紫色渐变、彩虹配色、移动渐变、装饰背景图案或发光边缘。
- 未知价格、部分数据、旧缓存和错误必须可见；不能把未知费用显示为 0。
- 不修改 Rust 价格匹配、分段/峰谷计算、统计公式、缓存、日期时区和后端 breakdown。
- 每个任务开始前检查 `git status`，不得覆盖其他 agent 的未提交文件；提交时只暂存任务列出的路径。

## 任务 0：建立第二版规范差异清单

**Files:**

- Read: `DESIGN.md`
- Read: `frontend/src/App.vue`
- Read: `frontend/src/views/Dashboard.vue`
- Read: `frontend/src/views/Settings.vue`
- Read: `frontend/src/components/*.vue`
- Read: `frontend/src/composables/theme.ts`
- Create: `docs/plans/archive/superseded/2026-10-06-design-system-visual-gap.md`

**Step 1: 记录当前实现基线。**

列出当前已存在的 token、inline style、Naive UI 默认主题、ECharts 初始化方式、主题偏好存储和组件状态，不凭旧计划中的执行记录判断已完成内容。

**Step 2: 按 DESIGN.md 建立差异表。**

至少覆盖：三团柔光、吸顶导航、三段主题控件、统一卡片、分段控件、标题行状态胶囊、指标比例条、表格无竖线、内联通知、设置页分组、tooltip 键盘路径和 1280×820/980×620 布局。

**Step 3: 运行现有前端基线。**

运行：`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`、`pnpm --dir frontend test`。

记录失败项和与本计划无关的并行改动，不修改这些失败项来制造“基线通过”。

**Step 4: 提交差异清单。**

提交信息：`docs(界面): 记录第二版视觉差异`。

## 任务 1：实现 Apple Glass 基础 token、柔光画布和主题适配

**Files:**

- Create or replace: `frontend/src/styles/tokens.css`
- Create or replace: `frontend/src/styles/naiveTheme.ts`
- Create or replace: `frontend/src/styles/chartTheme.ts`
- Modify: `frontend/src/main.ts`
- Modify: `frontend/src/App.vue`
- Modify: `frontend/src/composables/theme.ts`
- Test: `frontend/src/composables/theme.test.ts`

**Step 1: 写失败测试。**

覆盖 `light | dark | system` 偏好、旧 `tokenscope-theme` 值兼容、系统主题变更、解析后的 `data-theme`、主题控件的可访问名称和 reduced-motion 媒体查询。

**Step 2: 写第二版 token。**

严格录入 `DESIGN.md` 的 `--ts-canvas`、`--ts-surface`、`--ts-surface-solid`、`--ts-surface-elevated`、`--ts-glass`、`--ts-fill`、`--ts-segment-thumb`、separator、text、accent、状态色和四类图表色。补充 `--ts-ambient-1/2/3`、`--ts-glass-stroke`、`--ts-glass-highlight`、4/8/12/16/20/24/32 间距、8/6/14/12/999 圆角和 160–240ms 动效 token。

**Step 3: 实现画布柔光。**

在全局 body 或应用根节点加入左上蓝、右上青、底部浅绿三团固定静态椭圆柔光；单团不透明度不超过 20%，不动画、不随滚动、不使用紫色。内容滚动时必须能让吸顶玻璃看到背后的画布和内容。

**Step 4: 实现卡片和降级样式。**

提供 `.ts-card`、`.ts-card-solid`、`.ts-glass`、`.ts-segmented`、`.ts-notice`、`.ts-status-pill`、focus ring 和 `prefers-reduced-motion` 规则。卡片使用 14px 圆角、20px 内边距、顶部 inset 高光、统一描边和阴影；`@supports not (backdrop-filter: blur(1px))` 时切换到 95% 实色。

**Step 5: 接入 Naive UI 和 ECharts。**

`naiveTheme.ts` 只负责把同一套 token 映射到 NCard、NButton、NInput、NSelect、NDataTable、NTooltip、NPopover、NAlert 等组件；`chartTheme.ts` 只负责图表颜色、文字、分隔线和 tooltip。禁止复制另一套近似颜色。

**Step 6: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/composables/theme.test.ts`、`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`。

提交信息：`feat(界面): 建立 Apple Glass 视觉基础层`。

## 任务 2：重做 52px 吸顶导航和统一分段控件

**Files:**

- Modify: `frontend/src/App.vue`
- Modify: `frontend/src/components/AgentIcon.vue`
- Modify: `frontend/src/views/Dashboard.vue`
- Test: `frontend/src/App.test.ts`

**Step 1: 写失败测试。**

断言导航有 sticky 定位、品牌图标和 TokenScope 文本；页面切换与主题切换均为 `role="radiogroup"`；主题有浅色/深色/自动三项；来源项为图标 + 文字而非纯图标方块；左右方向键可以在分段组内切换。

**Step 2: 实现导航。**

导航高度约 52px，采用 `.ts-glass`，底边为发丝线；左侧品牌 15px/600，中部汇总/设置分段控件，右侧浅色/深色/自动三段控件。不得使用原生 select 或只有“暗色”文字加 switch 的旧布局。

**Step 3: 实现通用 segmented。**

底槽使用 `--ts-fill`、8px 圆角、2px 内边距、高度 32px；选中块使用 `--ts-segment-thumb` 和 6px 圆角，200ms 平移。来源选项必须保留 Claude Code/Codex 的图标和文字，命中区至少 32px。

**Step 4: 运行 App 测试并提交。**

运行：`pnpm --dir frontend vitest run src/App.test.ts`。

提交信息：`feat(界面): 重做吸顶导航与分段控件`。

## 任务 3：按第二版线框重排 Dashboard 标题、筛选和通知

**Files:**

- Modify: `frontend/src/views/Dashboard.vue`
- Modify: `frontend/src/components/DateRangeSelect.vue`
- Modify: `frontend/src/components/PricingStatusBanner.vue`
- Test: `frontend/src/views/Dashboard.test.ts`
- Test: `frontend/src/components/DateRangeSelect.test.ts`
- Test: `frontend/src/components/PricingStatusBanner.test.ts`

**Step 1: 写失败测试。**

覆盖页面大标题、时区/日期摘要、标题行右侧刷新和状态胶囊；筛选顺序固定为来源 → 聚合维度 → 日期 → 时区；数据源异常为内联通知；日期确认/取消和时区口径不变。

**Step 2: 实现页面头部。**

页面大标题 28px/700，副标题显示 `Asia/Shanghai · 今天 YYYY-MM-DD`；刷新按钮和“已更新/刷新中/缓存数据”胶囊在标题行右侧，不另起一行。内容最大宽度 1200px 居中，宽度 ≤1100px 时边距降为 20px。

**Step 3: 实现筛选栏。**

所有控件高度 32px、`--ts-fill` 底色、无描边、8px 圆角，视觉上属于同一族；宽度不足时整组换行。日期控件去掉 emoji，使用线性图标或文字。保留现有刷新、错误和数据保留逻辑。

**Step 4: 实现内联通知。**

价格未同步、数据源目录不存在、部分同步、旧缓存和未知模型使用 `.ts-notice`：玻璃底、12px 圆角、左侧状态图标、一行结论、右侧文字操作；多条信息合并为一条并可展开。不得使用整块高饱和 Alert 背景。

**Step 5: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/views/Dashboard.test.ts src/components/DateRangeSelect.test.ts src/components/PricingStatusBanner.test.ts`。

提交信息：`feat(界面): 重排汇总页头部与通知状态`。

## 任务 4：重构单张指标卡和分项比例条

**Files:**

- Modify: `frontend/src/components/SummaryCards.vue`
- Test: `frontend/src/components/SummaryCards.test.ts`

**Step 1: 写失败测试。**

覆盖主费用、三个次读数、比例条分段、全部为零、无数据、未知价格、极小金额和命中率 tooltip。明确断言命中率继续使用 `cache_read / (input + cache_read)`，不把公式重新铺在卡片正文。

**Step 2: 实现统一 `.ts-card`。**

卡片标题使用 17px/600；左侧估算费用 44px/600，下方显示 `USD · 估算值，非账单`；右侧总 token、请求数、缓存命中率 26px/600，中间使用 separator 发丝线。费用数字使用主文字色，不染蓝色。

**Step 3: 实现警告胶囊和比例条。**

含未计价 token 时在费用旁显示“含未计价 token”警告胶囊。底部加入高度 6px、圆角 3px 的输入/输出/缓存写/缓存命中比例条和一行色点图例；四类颜色只来自 chart tokens。全部为零时显示空槽，无数据显示 `—`，零、未知、加载中分开表达。

**Step 4: 迁移公式说明到 tooltip。**

缓存命中率说明通过 hover 和 focus 打开 tooltip，正文只保留结论；tooltip 关闭仍不影响读数。

**Step 5: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/components/SummaryCards.test.ts src/views/Dashboard.test.ts`。

提交信息：`feat(界面): 重构汇总指标卡与比例条`。

## 任务 5：把趋势图、聚合表和请求表放入统一卡片

**Files:**

- Modify: `frontend/src/components/TrendChart.vue`
- Modify: `frontend/src/components/UsageTable.vue`
- Modify: `frontend/src/components/EventTable.vue`
- Modify: `frontend/src/lib/chartData.ts`
- Test: `frontend/src/components/TrendChart.test.ts`
- Test: `frontend/src/components/UsageTable.test.ts`
- Test: `frontend/src/components/EventTable.test.ts`

**Step 1: 写失败测试。**

覆盖统一卡片头部“标题 + 可选副标题 + 右侧操作”；图表绘图区透明；四类系列颜色和顺序固定；表格无竖线、无外框、40px 行高；数字列右对齐；长内容可 focus 查看；行 Enter/Space 展开；图表类别多时不静默丢失。

**Step 2: 重构 TrendChart。**

图表放入 `.ts-card`，绘图区背景透明；网格线使用 separator，坐标轴不画轴线，坐标文字 12px muted，柱子顶部圆角 4px，堆叠柱只让最上段带圆角。图例放卡片头部右侧，使用圆点样式。主题切换时更新/重建实例，不能残留旧主题。

**Step 3: 保留可访问摘要和类别完整性。**

继续提供“数据摘要”入口，输出日期、模型和四类 token 的文字等价物；类别过多时滚动或缩放，不能静默截断。长模型名仅视觉省略，tooltip 展示全名。

**Step 4: 重构表格。**

表格与卡片之间不加第二层边框；移除竖线和外框，只保留 separator 发丝线；表头 12px/500，无底色；行高 40px；hover 只改变 `--ts-fill-hover`；可点击行显示 `›`；合计行上方使用 stronger separator。保留虚拟滚动、排序、行 key 和现有数据行为。

**Step 5: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/components/TrendChart.test.ts src/components/UsageTable.test.ts src/components/EventTable.test.ts src/lib/chartData.test.ts`。

提交信息：`feat(界面): 统一趋势图与数据表卡片`。

## 任务 6：实现 Apple Glass 费用明细浮层

**Files:**

- Modify: `frontend/src/components/EventTable.vue`
- Modify: `frontend/src/lib/costBreakdown.ts`
- Modify: `frontend/src/types.ts`（仅补展示类型）
- Test: `frontend/src/lib/costBreakdown.test.ts`
- Test: `frontend/src/components/EventTable.test.ts`

**Step 1: 写失败测试。**

覆盖 hover/focus/click 三种打开方式、Escape/外部点击关闭、边缘自动翻转、最大宽度 480px，以及输入/输出/缓存的 token × 单价 ÷ 1M 公式行和来源行。

**Step 2: 只消费后端 breakdown。**

显示匹配模型、价格来源、候选渠道、完整/前缀匹配、命中的分段阈值和峰谷条件。区分服务器实际响应模型/路由与保守估算候选；字段缺失显示“暂无数据”，完全未知显示“无法估算”，不得由前端猜单价。

**Step 3: 实现浮层视觉。**

浮层使用 85% elevated glass + 16px blur；公式区套 `.ts-card-solid`；按“事实 → 公式 → 结果 → 来源”排列，数字使用等宽 tabular 对齐，小额非零金额不能显示为 `$0.00`。

**Step 4: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/lib/costBreakdown.test.ts src/components/EventTable.test.ts`。

提交信息：`feat(界面): 重做费用明细玻璃浮层`。

## 任务 7：重做 macOS 风格设置页和状态系统

**Files:**

- Modify: `frontend/src/views/Settings.vue`
- Modify: `frontend/src/components/PricingStatusBanner.vue`
- Modify: `frontend/src/views/Dashboard.vue`
- Test: `frontend/src/views/Settings.test.ts`
- Test: `frontend/src/components/PricingStatusBanner.test.ts`

**Step 1: 写失败测试。**

覆盖应用/数据源/缓存/价格四组、每组单卡片、组标题在卡片外、设置项“左标签 + 右控件”、行间 separator、技术详情折叠、来源身份中性色和同步状态语义色。

**Step 2: 实现设置页。**

按 macOS 系统设置组织卡片；取消不必要的大号 NStatistic；技术路径、同步时间和错误详情放可展开的“技术详情”。所有设置卡片使用同一 `.ts-card`，不混用另一套 NCard 外观。

**Step 3: 收敛所有状态。**

loading、stale、empty、partial、error、no-price 统一使用标题行状态胶囊或 `.ts-notice`。加载保留旧数据，不加全屏 spinner；空状态居中显示一句结论和一句下一步建议；多条通知合并并可展开。

**Step 4: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/views/Settings.test.ts src/components/PricingStatusBanner.test.ts src/views/Dashboard.test.ts`。

提交信息：`feat(界面): 统一设置页与状态提示`。

## 任务 8：视觉回归、响应式和无障碍验收

**Files:**

- Modify: 受影响的 Vue/CSS 文件
- Create or update: `docs/plans/archive/partial/2026-10-06-design-system-visual-qa.md`
- Test: 相关 Vitest 测试；必要时新增 `frontend/src/accessibility.test.ts`

**Step 1: 运行完整门禁。**

运行：`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`、`pnpm --dir frontend test`、`pnpm --dir frontend build`。

预期：全部通过；不得使用 `--no-verify`。

**Step 2: 验收窗口、滚动和玻璃层次。**

在 1280×820 和 980×620 检查：内容最大宽度 1200px、导航 52px 吸顶、滚动内容从导航下方经过时能看到模糊层、卡片 14px 圆角/20px 内边距、筛选整组换行、表格不产生页面横向滚动。

**Step 3: 验收主题和状态矩阵。**

浅色、深色、自动三种主题偏好下检查正常数据、无缓存、旧缓存、部分价格、未知模型、极小非零金额、长模型名和长渠道名。确认画布柔光静止，玻璃不低于 70% 不透明，表格/图表/公式区保持实色可读。

**Step 4: 验收键盘和缩放。**

在 100%、125%、150% 缩放下，只用键盘完成页面切换、来源/维度/主题分段切换、日期确认/取消、刷新、tooltip 打开/关闭、表格详情展开和设置保存。确认 2px focus ring、Tab 顺序、左右方向键、Escape 和 aria 属性。

**Step 5: 验收图表与费用证据。**

逐项比对图例、数据摘要、聚合表和请求明细；确认费用浮层显示后端 breakdown 的模型、单价、来源、候选渠道、分段/峰谷条件，未知价格不伪装为 0。

**Step 6: 做 Rust 回归并记录截图。**

运行 `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 和 `cargo test --manifest-path src-tauri/Cargo.toml`。将每个尺寸/主题/关键状态的截图或可复现步骤记录到视觉 QA 文档。

**Step 7: 最终提交。**

确认 `git diff` 只包含本设计系统范围，运行完整 pre-commit 后提交：`feat(界面): 落地 Apple Glass 设计规范`。

## 完成定义

1. `DESIGN.md` 第二版中的 Apple Glass 规则全部有对应实现：三团柔光、52px sticky glass nav、统一 `.ts-card`、`.ts-segmented`、`.ts-notice`、三段主题控件和实色数据区。
2. 浅色、深色、自动主题的 token、Naive UI、ECharts、tooltip、表格和状态横幅一致，且支持无 blur 降级。
3. 汇总首屏按线框显示标题/状态、筛选、单张指标卡（费用 + 三项读数 + 比例条）、趋势、聚合和请求明细。
4. 费用 tooltip 通过 hover、focus、click 可访问，展示后端 breakdown 的事实、公式、结果和来源。
5. 统计公式、计价规则、缓存、日期时区和后端字段未改变。
6. 1280×820、980×620、100/125/150% 缩放、浅/深/自动主题和 loading/stale/empty/partial/error/no-price 状态完成真实视觉验收。
7. 前端 `typecheck`、`format:check`、`test`、`build`，根库与 Tauri 壳 Rust 门禁全部通过。

## 风险与回滚

- **玻璃过度发灰：** 检查导航是否 sticky、背景是否有三团柔光、玻璃不透明度是否达到 70%；必要时先提高不透明度，不改变数据区实色规则。
- **Naive UI 与公共卡片出现两套外观：** 优先让 NCard 通过主题覆盖复用 `.ts-card` token；无法覆盖时统一改用公共 class，禁止局部自定义第三种卡片。
- **ECharts 主题切换残留：** 监听解析后的主题并 dispose/rebuild，业务 `chartData` 不变。
- **透明度影响对比度：** 表格、图表绘图区、公式区立即退回 `--ts-surface-solid`；不通过降低文字 opacity 补救。
- **并行 agent 造成类型或门禁失败：** 记录具体文件和错误，等待责任 agent 修复；禁止修改无关文件和使用 `--no-verify`。

## 执行记录（2026-10-06，Apple Glass 第二版）

| 任务 | 内容 | 提交 |
| --- | --- | --- |
| 任务 0 | 第二版视觉差异清单（`2026-10-06-design-system-visual-gap.md`）+ 基线门禁（typecheck/format/107 用例） | cbf114e |
| 任务 1 | Apple Glass 基础层核对收尾：`.ts-status-pill` 公共类、reduced-motion/状态胶囊断言（主体由 a8788aa、b76809c 先行落地） | d547293 |
| 任务 2 | 来源筛选改图标+文字分段控件；品牌线性 SVG 标记；分段组方向键测试 | 149c750 |
| 任务 3 | 页头 28px 大标题 + 标题行状态胶囊；维度分段控件；NAlert → `.ts-notice` 内联通知（去设置/重试动作）；日期触发按钮入控件族；横幅 `.ts-notice` 化 | 851308d |
| 任务 4 | 单张指标卡（费用 44px + 三次读数 26px 发丝线分隔）、警告胶囊、命中率公式入 tooltip、6px 分项比例条 + 色点图例 | 437cf3c |
| 任务 5 | 趋势/聚合/明细统一 `.ts-card`（图例移卡头、堆叠柱顶段圆角）；表格无竖线行发丝线、› 指示、合计行粗线、可关闭筛选标签；区块间距 20px | aafe57a |
| 任务 6 | 费用浮层 elevated 玻璃（`--ts-glass-blur-popover` 16px）+ 公式区 `.ts-card-solid` 衬底 + 交互断言（click/hover/focus/Escape/aria-expanded） | 83c0994 |
| 任务 7 | 设置页 macOS 四组卡片（组标题在卡外、左标签右控件行）；全部警告收敛 `.ts-notice`；Dashboard 多来源异常合并可展开通知 | 5d2f882 |
| 任务 8 | 图表绘图区实色衬底容器（硬约束）；全量前端门禁（typecheck/format/133 用例/build）+ Rust 双 manifest 回归全绿；QA 文档重写为第二版 | 见最终提交 |

门禁结果与真机走查清单见 `2026-10-06-design-system-visual-qa.md`。
**计划保持 active：完成定义第 6 条（1280×820 / 980×620 / 三种缩放 / 浅深自动主题 /
六态状态的真实视觉验收）待走查通过后归档。**
