# 设计系统视觉 QA 记录（2026-10-06，第二版 Apple Glass）

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 既有视觉项 / F06 | 历史结果保留；F06 旧通过被复核否定后由后续滚动容器修复 | — |
| 当前界面与系统尾项 | 当前原生组合、系统缩放/首帧尚待验 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11)、[N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-06-design-system-visual-qa.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

> 对应计划：`docs/plans/archive/partial/2026-10-06-design-system-implementation.md` 任务 8。
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

## 2. 真机视觉走查清单

> **2026-10-07 真机走查已执行**（tauri dev，Windows 10，系统缩放 125%），结果标注如下。

执行方式：`pnpm --dir frontend tauri dev`（或按 `apple-refresh-remaining-tasks.md` 配置代理后 `.\frontend\node_modules\.bin\tauri dev`）。

### 2.1 尺寸、滚动与玻璃层次

- [x] 1280×820：内容最大宽度 1200px 居中；导航 52px 吸顶；筛选栏一行不换行（1470×930 下同验证）
- [x] 980×620（最小窗口）：筛选整组换行不压缩（来源+维度一行、日期+时区第二行）；表格无页面横向滚动（scroll-x 内部滚动）；指标卡主/次读数纵向堆叠
- [x] 滚动页面：吸顶导航可见模糊，内容从导航下方经过透出（深浅两主题均验证）
      （2026-10-07 修正：原实现 header 在滚动容器外，内容不会从其后经过；
      已改为 header 置于 .scroll-container 内同一滚动上下文，见修复提交 2fedca7）
- [x] （2026-10-07 F06 二次复核）2fedca7 的"header 入容器"不完整——
      .app-shell 缺 flex 约束，容器仍随内容生长（合成页实测 2274px），
      内部滚动失效、document 成第二条滚动条。修复：`.app-shell` 加
      `display:flex; flex-direction:column`，`.scroll-container` 加
      `min-height:0`。证据：`node frontend/scripts/check-app-scroll.mjs`
      真实 Chromium 量测 1280×820 / 980×620 × 浅/深四场景全过
      （修复前红灯 `qa-artifacts/app-scroll/prefix-red.txt`，修复后
      `measurements.json` + 截图）；release 应用实例滚动 10 页后导航
      吸顶、内容从其后经过（`real-window-scrolled.png`）。100%/150%
      系统缩放继续后延。
- [x] 卡片：14px 圆角、20px 内边距、顶部高光描边、区块间距 20px

### 2.2 主题与状态矩阵（浅色 / 深色 / 自动 各过一遍）

- [x] 浅色：画布 #F5F5F7 + 三团柔光静止；深色：#0F0F11 + 深色卡片描边（深色重启后持久化保持）
- [x] 正常数据（已更新胶囊）/ 零数据区间（零显示 0、命中率 N/A、比例条空槽）/ 极小非零金额（明细浮层 $0.000388 非 $0.00）
- [x] 表格行、图表绘图区、费用公式区实色可读
- [x] 长模型名 / 长项目名：单行省略 + hover tooltip 完整值（claude-opus-5-5 明细、C--Users-admin-Desktop----tokenscope 项目 tooltip 均验证）
- 注：首次无缓存横幅/未知† 由既有组件测试覆盖（本机已有缓存无法复现）

### 2.3 键盘与缩放（100% / 125% / 150%）

- [x] Tab 顺序符合视觉顺序；交互元素 2px 焦点环可见（日期触发器实测）
- [x] 主题分段控件方向键切换（Left/Right 实测）；分段 radiogroup 语义
- [x] 日期选择：键盘 Enter 打开 → 输入区间 → 确定 / 清除（2020 区间与清除还原均实测）
- [x] 费用明细浮层：click 打开 → Escape 关闭（触发器焦点环可见）
- [x] 聚合表行点击下钻（claude-opus-5-5/gpt-5.3-codex-spark 实测）；明细筛选标签可见
- ⚠ 100/150% 系统缩放未自动化（涉及修改系统显示设置）；125% 下全部走查通过，布局断点已在 980/1470 两个逻辑宽度验证

### 2.4 图表与费用证据一致性

- [x] 图例（卡片头部圆点）、数据摘要、聚合表、请求明细口径一致（合计 $2961.69 与 CLI 双源核对一致）
- [x] 费用浮层：匹配模型/渠道/来源/匹配方式/候选选择（候选 39 条，完整 36/不完整 3，在完整候选中取最高费用）/估算范围排除提示（3 个缺价候选未参与主估算）——缓存读取定价解析计划全部要素真机可见
- [x] 零值 0 / 未知未知† / N/A 仅无基数命中率

### 2.5 截图存档

截图存到 `docs/plans/active/screenshots/2026-10-06/`：
浅色 1280×820 首屏、深色 1280×820 首屏、980×620 筛选换行、指标卡特写、
来源异常通知、费用明细浮层（浅/深各一）。
> **注（Task 10 复核）**：screenshots 目录未建，以上为**历史文字记录，
图片当前不可核验**；补图需使用合成/脱敏展示数据。100%/150% 缩放与
D5 安装验收仍未执行，明确待验、不填通过。
> **注（2026-10-08 RC11 原生取证）**：首帧与主题/缩放矩阵的**可核验证据**已另行产出——
release+`acceptance` 原生实例的连续帧（深色首帧 799 ms 亮度 27.5、全程无浅帧插入；
浅色首帧 698 ms 亮度 255；跟随系统 898 ms）与六组"主题 × WebView 设备缩放"
（dpr 1/1.25/1.5）真实键盘/浮层四边/日期外壳/长路径/图表/导航量测，
见 [原生验收记录](2026-10-08-native-recheck-qa.md) §2–§3。
口径限制不变：**Windows 每监视器缩放与系统深色首帧仍未验**（不改系统设置），
§2.2 的"自动"行只覆盖系统=浅色一侧。

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
2026-10-08 RC11 已补齐 §2.2/§2.3 大部分原生证据（见上注与
[原生验收记录](2026-10-08-native-recheck-qa.md)），但 §2.5 的历史截图存档仍不存在、
系统缩放与系统深色两项仍待验，因此本清单继续 active，不因部分补证而整体宣告完成。
