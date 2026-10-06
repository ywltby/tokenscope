# TokenScope 视觉设计系统落地实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 按根目录 `DESIGN.md` 将 TokenScope Vue/Naive UI 界面统一为可读、可访问的冷色玻璃仪表盘，并在浅色、深色和不同窗口尺寸下保持一致的费用与用量信息层级。

**Architecture:** 先建立集中式 CSS 语义 token、Naive UI 主题覆盖和 ECharts 主题适配器，再按“应用壳 → 汇总指标 → 图表 → 表格/费用明细 → 设置与状态 → 验收”的顺序迁移现有组件。视觉改造只改变布局、样式和交互呈现；统计公式、价格选择、后端 breakdown、缓存和数据加载流程保持不变。

**Tech Stack:** Vue 3、TypeScript 5、Naive UI、ECharts 6、Vitest、Vite/Tauri 2；本地字体和 CSS `backdrop-filter`，不增加在线字体或新的 UI 框架。

---

## 设计基线与不可变约束

- 实现前必须通读 `DESIGN.md`；本文是执行顺序，冲突时以用户明确需求和业务数据口径为准。
- 只使用语义 token，组件中不新增散落的颜色、阴影、圆角和间距常量。
- 玻璃只用于导航、工具栏、状态横幅、菜单和 tooltip；表格主体、图表绘图区、费用公式区使用不透明或高不透明度表面。
- 不引入 emoji 图标、紫色渐变、霓虹发光、整页透明或大面积阴影。
- 不修改 `src/`、`src-tauri/`、价格匹配、分段计费、统计公式、日期时区和缓存逻辑。
- 不把“未知价格”变成 0，不隐藏首次无缓存、旧缓存、部分同步或请求级计价来源。
- 现有并行 agent 的改动不得被覆盖；开始每个任务前先确认 `git status`，只编辑任务列出的文件。

## 执行顺序

### Task 1: 建立视觉 token、全局样式和主题适配层

**Files:**

- Create: `frontend/src/styles/tokens.css`
- Create: `frontend/src/styles/naiveTheme.ts`
- Create: `frontend/src/styles/chartTheme.ts`
- Modify: `frontend/src/main.ts`
- Modify: `frontend/src/App.vue`
- Modify: `frontend/src/composables/theme.ts`
- Test: `frontend/src/composables/theme.test.ts`（若文件不存在则创建）

**Step 1: 写主题和 token 的失败测试。**

覆盖以下行为：默认跟随系统；显式浅色/深色持久化；系统主题变化只影响 `system` 偏好；解析后的主题值只可能是 `light` 或 `dark`；token 样式表能被应用入口加载。

**Step 2: 运行定向测试确认失败。**

运行：`pnpm --dir frontend vitest run src/composables/theme.test.ts`

预期：新增的 system 偏好或 token 导入断言失败，不能因为测试没有执行而跳过。

**Step 3: 实现 token 文件。**

在 `tokens.css` 定义 `--ts-canvas`、`--ts-surface`、`--ts-surface-solid`、`--ts-surface-elevated`、边框、正文、辅助文字、青色强调、成功/警告/错误/信息色，以及 4/8/12/16/24/32 间距、6/8/12 圆角、focus ring、玻璃模糊和过渡时长。浅色与深色值必须以 `DESIGN.md` 表格为准；为不支持 `backdrop-filter` 的系统提供实色降级。

**Step 4: 实现 Naive UI 和 ECharts 适配。**

在 `naiveTheme.ts` 集中设置 body、card、border、text、primary、button、input、table、tooltip、popover、alert、tabs 的颜色和圆角。`chartTheme.ts` 固定 token 系列顺序、图表背景、网格、坐标轴、文字和 tooltip 颜色，禁止直接使用 ECharts 默认调色板。

**Step 5: 改造主题 composable 和应用入口。**

将主题偏好扩展为 `light | dark | system`，保留现有 `tokenscope-theme` 存储键的兼容性。`useTheme` 暴露 `preference`、解析后的 `mode`、切换/设置方法；`App.vue` 只消费解析后的 mode，并把主题覆盖和全局 CSS 传入 `NConfigProvider`。主题切换不能产生首帧明显闪烁。

**Step 6: 运行测试与类型检查。**

运行：`pnpm --dir frontend vitest run src/composables/theme.test.ts`、`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`。

预期：全部通过；若并行 agent 的未完成文件导致 typecheck 失败，记录错误并停止提交，不要绕过门禁。

**Step 7: 提交。**

`git add frontend/src/styles frontend/src/main.ts frontend/src/App.vue frontend/src/composables/theme.ts frontend/src/composables/theme.test.ts`；提交信息：`feat(界面): 建立统一主题与视觉 token`。

### Task 2: 重做应用壳、顶部导航、横幅和汇总页工具栏

**Files:**

- Modify: `frontend/src/App.vue`
- Modify: `frontend/src/views/Dashboard.vue`
- Modify: `frontend/src/components/PricingStatusBanner.vue`
- Modify: `frontend/src/components/DateRangeSelect.vue`
- Test: `frontend/src/App.test.ts`
- Test: `frontend/src/components/PricingStatusBanner.test.ts`
- Test: `frontend/src/components/DateRangeSelect.test.ts`

**Step 1: 为布局和状态写失败测试。**

断言：页面有可识别的汇总/设置 tab；主题控件能读出当前浅色、深色或跟随系统；价格横幅在无缓存、部分同步、旧缓存和同步中仍有清晰文字；日期选择仍保留草稿、取消和确认语义；横幅不遮挡内容。

**Step 2: 实现应用壳。**

使用 56px 左右的玻璃导航栏、简洁 TokenScope 字标、带选中底色的 tab、带文字的主题选择器。移除 inline style 和 emoji 日历图标，替换为现有线性图标或文字按钮。全局内容左右边距 24px，窗口接近 980px 时降为 16px。

**Step 3: 实现汇总页标题和筛选行。**

在 Dashboard 顶部增加页面标题、当前日期/时区摘要和刷新动作；筛选顺序固定为来源 → 维度 → 日期 → 时区 → 刷新，空间不足时换行。保留现有请求参数、刷新 key、错误信息和数据保留行为。

**Step 4: 迁移横幅和日期选择器样式。**

状态横幅使用玻璃表面、语义状态色和明确行动；同步按钮不可用时仍说明原因。日期选择器使用统一控件高度、圆角和 focus ring，弹窗使用 elevated surface；不改日期字符串和时区转换逻辑。

**Step 5: 运行定向测试和格式检查。**

运行：`pnpm --dir frontend vitest run src/App.test.ts src/components/PricingStatusBanner.test.ts src/components/DateRangeSelect.test.ts`、`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`。

**Step 6: 提交。**

提交信息：`feat(界面): 重构应用壳与筛选工具栏`。

### Task 3: 重做 SummaryCards 的指标层级

**Files:**

- Modify: `frontend/src/components/SummaryCards.vue`
- Modify: `frontend/src/types.ts`（仅在现有字段不足以表达显示状态时）
- Test: `frontend/src/components/SummaryCards.test.ts`（创建）

**Step 1: 写失败测试。**

覆盖：估算费用、总 token、请求数、缓存命中率位于统一读数条；零值显示 0；未知价格显示未知标记且不显示为 0；极小非零金额不被格式化为 `$0.00`；缓存命中率继续使用现有 `cache_read / (input + cache_read)` 公式。

**Step 2: 重排模板。**

将当前“巨大英雄卡 + 四张彩色卡 + 独立命中率卡”改为一个统一指标 strip，再接一个轻量 token 分项行。费用旁显示“估算”，单位独立对齐；输入、输出、缓存写入、缓存命中使用低饱和语义色和文字，不使用 emoji 或彩虹卡片。

**Step 3: 应用数字和状态样式。**

所有数字启用 tabular lining nums，核心读数 30–36px，辅助信息不低于 12px。未知、加载中、无数据和零值使用不同的文本状态；不要通过整体 opacity 隐藏未知信息。

**Step 4: 运行测试。**

运行：`pnpm --dir frontend vitest run src/components/SummaryCards.test.ts src/views/Dashboard.test.ts`。

预期：既有统计断言继续通过，新增显示状态断言通过。

**Step 5: 提交。**

提交信息：`feat(界面): 重构用量指标层级`。

### Task 4: 迁移趋势图到统一图表主题并补充可访问摘要

**Files:**

- Modify: `frontend/src/components/TrendChart.vue`
- Modify: `frontend/src/lib/chartData.ts`（只补显示元数据，不改变聚合结果）
- Test: `frontend/src/lib/chartData.test.ts`
- Test: `frontend/src/components/TrendChart.test.ts`（创建）

**Step 1: 写失败测试。**

断言：四类 token 始终按固定顺序和固定语义色输出；长模型名有完整 tooltip；空数据、单类别、类别很多时都有明确状态；图表数据能生成等价的文字摘要或表格入口。

**Step 2: 接入 `chartTheme.ts`。**

初始化 ECharts 时注入主题配置，关闭默认 palette。网格线、坐标轴、tooltip 和图例从 CSS/主题 token 派生；浅色/深色切换时销毁并重建或更新实例，不能残留旧主题。

**Step 3: 控制图表高度和类别溢出。**

保留足够的类别，不用 `max(320, groups * 34 + 70)` 无限撑高页面；类别多时采用滚动、缩放或分页，并在界面上说明当前范围。长标签省略但 hover/focus 可见完整值。

**Step 4: 增加可访问摘要。**

在图表标题区域提供“查看数据表/摘要”入口；键盘用户可以获得日期、模型、四类 token 和总量文本。图表颜色必须有图例和文字，不得只靠颜色。

**Step 5: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/lib/chartData.test.ts src/components/TrendChart.test.ts`、`pnpm --dir frontend typecheck`。

提交信息：`feat(界面): 统一趋势图主题与可访问摘要`。

### Task 5: 重做聚合表和请求明细表

**Files:**

- Modify: `frontend/src/components/UsageTable.vue`
- Modify: `frontend/src/components/EventTable.vue`
- Test: `frontend/src/components/UsageTable.test.ts`
- Test: `frontend/src/components/EventTable.test.ts`

**Step 1: 写失败测试。**

覆盖：数字列右对齐和 tabular nums；长模型/项目名可通过 focus/click 获取完整值；行详情可用 Enter/Space 展开；表格主体没有重网格；虚拟滚动和当前行 key 行为不变。

**Step 2: 迁移表格表面。**

关闭重边框网格，使用 `--ts-surface-solid`、轻行分隔和 hover 背景；保留 Naive UI 的排序、分页/虚拟滚动能力。表头简短且有明确单位，金额列使用估算语义。

**Step 3: 补键盘和复制路径。**

模型、项目、渠道和路径列使用省略显示，tooltip 同时支持 hover/focus；可展开行必须有按钮语义、aria-expanded 和明确 focus ring。不能以 title 属性作为唯一辅助方式。

**Step 4: 运行定向测试并提交。**

运行：`pnpm --dir frontend vitest run src/components/UsageTable.test.ts src/components/EventTable.test.ts`。

提交信息：`feat(界面): 优化聚合与请求明细表格`。

### Task 6: 落实请求级费用明细 tooltip

**Files:**

- Modify: `frontend/src/components/EventTable.vue`
- Modify: `frontend/src/lib/costBreakdown.ts`
- Modify: `frontend/src/types.ts`（仅补充已有后端 breakdown 的展示类型）
- Test: `frontend/src/lib/costBreakdown.test.ts`（创建或补充）
- Test: `frontend/src/components/EventTable.test.ts`

**Step 1: 先写失败测试。**

覆盖：输入/输出/缓存各项显示 token × 单价 ÷ 1M 和小计；显示匹配模型、价格来源、候选渠道、匹配层级、命中的分段/峰谷条件；完全未知时显示无法估算；tooltip 触发器支持 hover、focus、click、Escape 和点击外部关闭。

**Step 2: 只消费后端 breakdown。**

检查并统一 `costBreakdown.ts` 的展示模型，让前端不重新选择价格、不重复计算峰谷或分段价格。缺失字段使用“暂无数据”，不能猜测服务器实际渠道；明确区分服务器响应模型/路由与保守估算候选。

**Step 3: 实现 elevated tooltip。**

最大宽度 480px，按“事实 → 公式 → 结果 → 来源”排列；公式区使用不透明背景，等宽数字右对齐。小额非零费用不能被显示为 0；tooltip 靠近窗口边缘时自动翻转，键盘和鼠标均可关闭。

**Step 4: 运行费用相关测试。**

运行：`pnpm --dir frontend vitest run src/lib/costBreakdown.test.ts src/components/EventTable.test.ts`。

同时检查 Rust 已输出 breakdown 的字段名和序列化兼容性；若发现字段不足，创建单独后端计划，不在本计划中修改价格逻辑。

**Step 5: 提交。**

提交信息：`feat(界面): 完善请求费用明细展示`。

### Task 7: 重做设置页和所有数据状态

**Files:**

- Modify: `frontend/src/views/Settings.vue`
- Modify: `frontend/src/components/PricingStatusBanner.vue`
- Modify: `frontend/src/views/Dashboard.vue`
- Test: `frontend/src/views/Settings.test.ts`
- Test: `frontend/src/components/PricingStatusBanner.test.ts`

**Step 1: 写失败测试。**

覆盖设置页分组结构、来源身份与同步状态分离、首次无缓存横幅、部分同步错误、旧缓存提示、无数据和加载中的局部状态。同步成功后横幅与设置页状态必须一致。

**Step 2: 重排设置页。**

按“应用、数据源、缓存、价格”分组；移除不必要的大号 `NStatistic`；技术路径、同步时间和错误详情放到可展开区域。外置/models.dev/OpenRouter 标签使用中性色，成功/警告/错误只表达状态。

**Step 3: 统一 Dashboard 状态。**

为 loading、stale、empty、partial、error、no-price 建立一致的文案、图标和色彩。刷新时保留已有数据，仅局部显示加载；不增加全屏遮罩。保留现有错误和定价同步动作。

**Step 4: 运行测试并提交。**

运行：`pnpm --dir frontend vitest run src/views/Settings.test.ts src/components/PricingStatusBanner.test.ts src/views/Dashboard.test.ts`。

提交信息：`feat(界面): 统一设置页与数据状态`。

### Task 8: 响应式、无障碍和视觉回归验收

**Files:**

- Modify: 受影响的 Vue 组件和 `frontend/src/styles/tokens.css`
- Create: `docs/plans/active/2026-10-06-design-system-visual-qa.md`（记录截图和已知差异；若项目已有视觉 QA 文档则合并）
- Test: 相关 Vitest 测试；必要时新增 `frontend/src/accessibility.test.ts`

**Step 1: 运行完整前端门禁。**

运行：`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`、`pnpm --dir frontend test`、`pnpm --dir frontend build`。

预期：全部通过；不得使用 `--no-verify`。

**Step 2: 做桌面尺寸检查。**

分别检查 1280×820 和 980×620，确认首屏层级、筛选换行、表格横向溢出、tooltip 翻转和刷新按钮可见。再检查 100%、125%、150% 缩放。

**Step 3: 做主题和状态矩阵检查。**

浅色/深色各检查：正常数据、无数据、加载中、旧缓存、部分价格、未知价格、极小非零金额、长模型名和长渠道名。确认文字对比度、focus ring、图例和数据摘要可读。

**Step 4: 做键盘路径检查。**

只用键盘完成页面切换、筛选、日期确认/取消、刷新、费用 tooltip 展开/关闭、表格详情展开和设置保存。检查 Escape、Tab 顺序和 aria-expanded/aria-label。

**Step 5: 运行 Rust 回归检查。**

因为本计划不应改变后端，至少运行 `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 和 `cargo test --manifest-path src-tauri/Cargo.toml`，确认视觉迁移没有误改数据层。

**Step 6: 分任务提交后做最终提交。**

先确认 `git diff` 只包含设计系统相关文件，再由维护者执行完整 pre-commit。提交信息：`feat(界面): 落地 TokenScope 玻璃仪表盘设计`。

## 完成定义

计划完成必须同时满足：

1. 所有界面颜色、间距、圆角和阴影来自集中 token；没有新增散落硬编码视觉值。
2. 浅色、深色和系统主题在应用壳、Naive UI、ECharts、tooltip、表格和状态横幅中一致。
3. 汇总页首屏先呈现估算费用、总 token、请求数、缓存命中率，再呈现分项、趋势和明细。
4. 费用明细可通过 hover、focus、click 访问，并展示匹配模型、单价、来源、候选渠道及分段/峰谷条件；未知价格不伪装成 0。
5. 统计公式、价格计算、缓存、日期及时区、后端 breakdown 没有改变。
6. 1280×820、980×620、三种缩放比例、两种主题和所有关键状态通过视觉与自动化验收。
7. `pnpm --dir frontend typecheck`、`format:check`、`test`、`build`，以及根库和 Tauri 壳的 Rust 门禁全部通过。

## 风险与回滚

- **Naive UI 默认主题覆盖不完整：** 先以 CSS token 覆盖关键表面，再补 `themeOverrides`；若某组件版本不支持透明背景，使用实色降级，不升级 Naive UI。
- **ECharts 实例残留旧主题：** 监听解析后的主题值，先 `dispose` 再按保留的业务数据重建；不改变 `chartData` 聚合。
- **玻璃效果导致低对比度或性能下降：** 只在少数壳层使用 blur，表格和图表保持实色；在不支持 blur 时自动降级。
- **前端类型错误来自并行工作：** 记录具体文件和错误，等待责任 agent 修复后再跑门禁；禁止修改无关文件或使用 `--no-verify`。
- **视觉改造误伤数据语义：** 每个任务都先补显示层测试，所有统计/价格输入输出保持原有快照和测试结果。

## 执行记录（2026-10-06）

| Task | 内容 | 提交 | 结果 |
| --- | --- | --- | --- |
| 1 | tokens.css 语义 token（浅/深两套 + @supports 实色降级 + reduced-motion）、naiveTheme.ts / chartTheme.ts 适配器、主题偏好 light\|dark\|system（兼容旧存储键）、App 接线 data-theme | 61ec0ac | theme.test.ts 7 项 + 全量前端测试通过 |
| 2 | 56px 玻璃导航 + tab 页切换 + 带标签主题选择器；Dashboard 页面标题/时区摘要/筛选行（来源→维度→日期→时区→刷新）；横幅玻璃化 + alert 角色；日期选择器去 emoji、elevated 弹层 | 124fc6a | App/Banner/DateRangeSelect 28 项通过 |
| 3 | SummaryCards 统一指标读数条 + 轻量分项行；未知† 不伪装 0；极小金额保留；命中率公式不变 | df68b19 | SummaryCards.test.ts 6 项 + Dashboard 回归通过 |
| 4 | TrendChart 接入 chartTokens（禁默认调色板）、高度封顶 560 + dataZoom、完整键 tooltip、可访问文字摘要；chartData 增 fullLabels/摘要/状态（不改变聚合） | 5a85189 | chartData/TrendChart 10 项通过 |
| 5 | 聚合表/明细表：轻行分隔（去重网格）、数字列 tabular、费用列"估算"表头、未知列警告色（去 opacity）、行 Enter/Space 键盘下钻、模型/项目列可聚焦取完整值 | 9aafb51 | UsageTable/EventTable 语义测试通过 |
| 6 | 费用 tooltip 重构为"事实→公式→结果→来源"行模型；manual trigger 支持 hover/focus/click + Escape + 点击外部关闭；完全未知显示"无法估算"；缺字段"暂无数据" | aa498ea | costBreakdown.test.ts 8 项 + EventTable 更新通过 |
| 7 | 设置页按 应用/数据源/缓存/价格 分组；NStatistic 换紧凑行；技术路径/同步时间收进"技术详情"折叠；来源身份标签中性色；Dashboard 空状态文案 | 4c50e7d | Settings/Dashboard/Banner 33 项通过 |
| 8 | accessibility.test.ts（tablist/aria-label/role=img/aria-expanded）；完整门禁；视觉 QA 记录文档 | c84bc4e | 见下 |

### Task 8 门禁结果

- 前端：typecheck ✅ / format:check ✅ / vitest 14 文件 107 用例 ✅ / build ✅
- Rust 回归（确认后端零改动）：fmt ✅ / clippy -D warnings ✅ / test 152 ✅ / src-tauri 10 ✅

### 待办（保持计划 active 的原因）

- 真机视觉走查未执行：1280×820 / 980×620 / 三档缩放 / 双主题矩阵 / 键盘全路径，
  清单见 `docs/plans/active/2026-10-06-design-system-visual-qa.md` §2。完成后本计划方可归档。
