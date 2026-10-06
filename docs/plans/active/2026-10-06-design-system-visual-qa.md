# 设计系统视觉 QA 记录（2026-10-06）

> 对应计划：`docs/plans/active/2026-10-06-design-system-implementation.md` Task 8。
> 自动化验收已全部通过；本文件同时列出**必须在真机完成的视觉走查项**及其步骤。
> 自动化测试不能替代视觉验收（DESIGN.md §7）。

## 1. 自动化验收结果（2026-10-06）

| 门禁 | 结果 |
| --- | --- |
| `pnpm --dir frontend typecheck` | ✅ 通过 |
| `pnpm --dir frontend format:check` | ✅ 通过 |
| `pnpm --dir frontend test` | ✅ 14 文件 / 107 用例全部通过 |
| `pnpm --dir frontend build` | ✅ 通过 |
| `cargo fmt --check` / `cargo clippy --all-targets -- -D warnings` | ✅ 通过（后端零改动回归确认） |
| `cargo test` + `cargo test --manifest-path src-tauri/Cargo.toml` | ✅ 全部通过 |

覆盖关键状态的自动化用例：

- 主题偏好 light/dark/system 解析与持久化、data-theme 驱动（theme.test.ts、App.test.ts）
- Naive UI / ECharts 两套主题适配完整性（theme.test.ts）
- 统一指标条：零值 0、未知 † 不伪装 0、极小金额、命中率公式（SummaryCards.test.ts）
- 图表：固定语义色（禁默认调色板）、高度封顶 + dataZoom、文字摘要等价、主题切换重建（TrendChart.test.ts、chartData.test.ts）
- 表格：数字列右对齐 tabular、估算表头、未知警告色非 opacity、Enter/Space 下钻（UsageTable.test.ts、EventTable.test.ts）
- 费用明细：事实→公式→结果→来源、无法估算、暂无数据（costBreakdown.test.ts）
- 无障碍关键点：tablist/tab、aria-label、role=img、aria-expanded（accessibility.test.ts）

## 2. 真机视觉走查清单（待执行）

### 2.1 尺寸与缩放

- [ ] 1280×820：首屏层级 = 导航 → 标题 → 指标条 → 分项行；筛选不换行错位
- [ ] 980×620：内容左右边距 16px；指标条 2×2 折行；筛选行自动换行可读
- [ ] 系统 100% / 125% / 150% 缩放：核心金额与刷新按钮不截断

### 2.2 主题矩阵（浅色 / 深色各一遍）

- [ ] 正常数据 / 无数据（空状态卡）/ 加载中（局部 NSpin + 刷新中标签）/ 旧缓存（缓存数据标签）/ 部分价格（† 警告色）/ 未知价格（未知†）
- [ ] 玻璃导航在深浅两主题下的对比度与内描边；不支持 backdrop-filter 时的实色降级（可用开发工具禁用 backdrop-filter 模拟）
- [ ] 图例、轴标签、tooltip 在两主题下可读（chartTokens 派生色）

### 2.3 键盘路径

- [ ] Tab 顺序：品牌 → 汇总 tab → 设置 tab → 主题选择 → 内容区
- [ ] 费用 tooltip：Tab 聚焦触发器展开（focus）、Enter 展开（click）、Escape 关闭、点击外部关闭（onClickoutside）
- [ ] 聚合行聚焦后 Enter/Space 下钻到明细；图表"数据摘要"按钮 Enter 展开并出现 region
- [ ] 设置页来源行开关/目录/保存全程键盘可用

### 2.4 数据语义抽查

- [ ] 费用 tooltip 在窗口右缘时向左翻转（placement="left"）
- [ ] 长模型名/长渠道名：轴上省略、tooltip 显示完整原始键
- [ ] 明细费用列与 tooltip"估算合计"数值一致（同源，前端不重算）

## 3. 已知差异与限制

- 汇总页筛选行新增了"统计时区"下拉（DESIGN.md §4 筛选顺序要求），设置页的时区选择仍保留，两者共享同一偏好。
- ECharts tooltip formatter 使用完整原始模型键（轴上仍为末段展示名）；如后续需要显示渠道前缀，需后端 breakdown 增加字段。
- 视觉 token 的浅/深两组值在 tokens.css 与 naiveTheme.ts/chartTheme.ts 各维护一份（CSS 变量无法在渲染前同步读取），修改 DESIGN.md 色表时必须同步三处。
