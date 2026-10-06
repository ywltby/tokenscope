# 设计系统视觉 QA 记录（2026-10-06，第二版 Apple Glass）

> 对应计划：`docs/plans/active/2026-10-06-design-system-implementation.md` 任务 8。
> 规范：根目录 `DESIGN.md`（第二版，苹果风格 + Glassmorphism）。
> 自动化验收已全部通过；本文件同时列出**必须在真机完成的视觉走查项**及其步骤。
> 自动化测试不能替代视觉验收（DESIGN.md §7）。

## 1. 自动化验收结果（2026-10-06，Apple Glass 任务 0–7 完成后）

| 门禁 | 结果 |
| --- | --- |
| `pnpm --dir frontend typecheck` | ✅ 通过 |
| `pnpm --dir frontend format:check` | ✅ 通过 |
| `pnpm --dir frontend test` | ✅ 14 文件 / 133 用例全部通过 |
| `pnpm --dir frontend build` | ✅ 通过 |
| `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 通过 |
| `cargo test --workspace`（根库） | ✅ 全部通过（lib + 集成测试） |
| `cargo fmt/clippy/test --manifest-path src-tauri/Cargo.toml` | ✅ 通过（壳 10 单测） |

覆盖关键契约的自动化用例：

- 主题偏好 light/dark/system 解析与持久化、data-theme 驱动、主题控件可访问名称（theme.test.ts、App.test.ts、accessibility.test.ts）
- token 层：两套主题完整、无 blur 降级、reduced-motion、状态胶囊（theme.test.ts）
- 分段控件：radiogroup 语义、图标 + 文字来源项、方向键切换（App.test.ts、Dashboard.test.ts、SegmentedControl）
- 页头：大标题 + 时区/日期摘要 + 状态胶囊（缓存数据/刷新中/刷新失败/已更新）、筛选顺序 来源→维度→日期→时区（Dashboard.test.ts）
- 内联通知：来源四态、多条合并可展开、汇总/明细错误可重试、去设置动作（Dashboard.test.ts）
- 指标卡：单卡左右两区、零值 0、未知† 警告胶囊不伪装 0、极小金额、命中率公式收进 tooltip、分项比例条占比/空槽（SummaryCards.test.ts）
- 图表：固定语义色禁默认调色板、绘图区透明 + 实色衬底容器、堆叠柱仅顶段圆角、HTML 圆点图例、高度封顶 + dataZoom、文字摘要等价、主题切换重建（TrendChart.test.ts、chartData.test.ts）
- 表格：数字列右对齐 tabular、无外框无竖线行发丝线、› 指示、合计行粗线、Enter/Space 下钻、可关闭筛选标签（UsageTable.test.ts、EventTable.test.ts）
- 费用浮层：hover/focus/click 打开、Escape/外部点击关闭、aria-expanded、480px 上限、16px 玻璃模糊、公式区实色衬底、事实→公式→结果→来源、无法估算/暂无数据（EventTable.test.ts、costBreakdown.test.ts）
- 设置页：四组卡片、组标题在卡外、左标签右控件行、警告 ts-notice、技术详情折叠、来源身份中性色（Settings.test.ts）

## 2. 真机视觉走查清单（待执行）

执行方式：`pnpm --dir frontend tauri dev`（或按 `apple-refresh-remaining-tasks.md` 配置代理后 `.\frontend\node_modules\.bin\tauri dev`）。

### 2.1 尺寸、滚动与玻璃层次

- [ ] 1280×820：内容最大宽度 1200px 居中；导航 52px 吸顶；筛选栏一行不换行
- [ ] 980×620（最小窗口）：筛选整组换行不压缩；表格无页面横向滚动；指标卡主/次读数纵向堆叠
- [ ] 滚动页面：内容从吸顶导航下方经过时导航可见模糊（Win10 不支持时退化为 95% 实色，布局不变）
- [ ] 卡片：14px 圆角、20px 内边距、顶部高光描边、区块间距 20px

### 2.2 主题与状态矩阵（浅色 / 深色 / 自动 各过一遍）

- [ ] 浅色：画布 #F5F5F7 + 三团柔光静止（不动画、不随滚动）；深色：#0F0F11 + 深色卡片描边
- [ ] 正常数据 / 无数据（空状态一句结论 + 下一步建议）/ 首次无缓存（定价横幅 .ts-notice）/ 旧缓存（标题行警告胶囊）/ 部分价格（费用旁「含未计价 token」胶囊）/ 未知模型（未知†，不伪装 0）/ 极小非零金额（不显示 $0.00）
- [ ] 表格行、图表绘图区、费用公式区保持实色可读（玻璃不发虚）
- [ ] 长模型名 / 长项目名 / 长渠道名：单行省略，hover/focus 可见完整值

### 2.3 键盘与缩放（100% / 125% / 150%）

- [ ] Tab 顺序符合视觉顺序；所有交互元素 2px 焦点环可见
- [ ] 分段控件（页面/主题/来源/维度）左右方向键切换；Tab 进出整组
- [ ] 日期选择：打开 → 快捷/草稿 → 确定 / 取消 / 清除；Escape 关闭浮层
- [ ] 费用明细浮层：focus 打开 → Escape / 点击外部关闭；靠近窗口边缘自动翻转
- [ ] 聚合表行 Enter/Space 下钻；明细筛选标签可键盘关闭；设置页保存
- [ ] 各缩放下核心金额与操作不截断

### 2.4 图表与费用证据一致性

- [ ] 图例（卡片头部圆点）、数据摘要、聚合表、请求明细四者口径一致
- [ ] 费用浮层展示后端 breakdown：匹配模型、单价、来源、候选渠道、分段/峰谷条件；区分保守估算候选与服务器实际路由
- [ ] 零值显示 0、未知显示未知†、N/A 仅用于无基数命中率

### 2.5 截图存档

截图存到 `docs/plans/active/screenshots/2026-10-06/`：
浅色 1280×820 首屏、深色 1280×820 首屏、980×620 筛选换行、指标卡特写、
来源异常通知、费用明细浮层（浅/深各一）。

## 3. 已知限制与实现说明

- 玻璃配方（导航/卡片/通知/浮层）的模糊与饱和度集中在 `tokens.css`
  （`--ts-glass-blur` 20px、`--ts-glass-blur-popover` 16px）；不支持
  `backdrop-filter` 时按 DESIGN.md 退化到 95% 实色。
- Naive UI 主题覆盖（`naiveTheme.ts`）与 ECharts 适配（`chartTheme.ts`）
  的色值是 `tokens.css` 的人工同步副本——**三处改色必须一起改**。
- ECharts 绘图区容器使用 `.ts-card-solid` 实色衬底（计划硬约束），
  ECharts 自身 backgroundColor 保持 transparent。
- 自动主题（system）下 Naive 主题与 data-theme 由同一解析值驱动，无首帧闪烁；
  系统主题变化监听依赖 matchMedia，Tauri WebView 支持正常。
- 非日维度趋势图 >14 类别时启用图内滚动（dataZoom），日维度 >60 天启用缩放；
  类别不会被静默丢弃。

## 4. 结论

任务 0–8 自动化部分完成；**计划保持 active，待 §2 真机走查全部勾选后归档**
（完成定义第 6 条：真实视觉验收不能以"编译通过"代替）。
