# UI/UX 审查核实与修复实施计划

> **执行说明：** 使用 `executing-plans` 技能逐任务执行；本文只登记方案，尚未实施。先写能复现问题的测试，再做最小修复。不得根据原审查中的旧结论重复返工已完成任务。

**目标：** 修复经核实的界面、键盘交互和错误恢复问题，落实 `DESIGN.md` 第二版玻璃与双主题规范，并用真实组件量测防止再次出现“代码写了规格、实际渲染不符合规格”。

**架构：** 延续 Vue 3 + Naive UI + ECharts；公共视觉规则落到语义 token 与组件适配器，交互状态留在所属组件。费用只格式化后端结果，统计与定价算法不变。使用合成 IPC 数据启动真实前端进行视觉验收，单元测试验证状态机与边界。

**技术栈：** TypeScript 5、Vue 3、Naive UI、ECharts、Vitest、Playwright/Chromium；Rust/Tauri 保持现有接口。

**基线：** `dd6aaae`（2026-10-07），分支 `docs/product-review-plan`。原 [UI/UX 审查](../../reviews/2026-10-07-ui-ux-review.md) 基于 `1cefac7`，其中部分结论已过期或需更正。实现时如 HEAD 前进，先重新核对涉及文件。

---

## 1. 范围、优先级与不变量

本计划承接原审查的 U01–U22、D1–D5；具体取舍以第 2 节为准。优先修复信息不可正确读取、错误无恢复路径、日期误显示和图表重复重建，再落实材质、字号和控件尺寸。原报告的“两项新增 Blocker”不能直接当成已证实的阻断级别。

非目标：重做页面架构、替换组件库、生成整套主题编译系统、调整定价/分段/峰谷/缓存口径、恢复内置价格、升级依赖、扩大 agent 支持范围、重新实现已修复的导航与关窗流程。

必须保持：

1. `cost_usd`、候选渠道、单价、档位、未知量均来自后端；前端不重新选价、不重算费用、不把未知当零。仅明确声明 `SameAsInput` 的缓存读使用最终输入价，缺失价格仍为 Unknown；完整候选优先与溢出诊断等最新实现不变。
2. 日期继续用 `formatted-value` 与 `yyyy-MM-dd` 字符串；标签修复不改变时区、闭区间、草稿/取消/确认、跟随今天语义。
3. 视图快照 v5、共享刷新批次、请求代次、游标分页与卸载保护不变；不得为了减少重绘删除必要的数据更新监听。
4. 双主题同等完整；表格和费用公式保持实色衬底，浮层采用 elevated 材质。不能把所有表面统一透明，也不能靠降低文字 opacity 达成层次。
5. 所有说明浮层有键盘路径；真实可访问名称含当前值。普通 tooltip 不强制使用 `aria-live`；不引入重复朗读。
6. 失败不能伪装成默认值、成功或“主源已可用”；保留已加载数据与用户未保存草稿。重试状态读取不应偷偷触发联网同步。
7. F06 导航布局、F08 真实 tooltip 验证与关窗失败可重试作为回归约束，不重做。缓存重建是派生数据操作，保留现有进行中/完成反馈，不增加强制确认。
8. 所有自动测试使用合成数据；所有 IPC 被 mock，未列出的 command 显式报错。不得扫描真实 agent 日志或写真实 `~/.tokenscope`；不运行 ignored 真实性能测试。

## 2. 审查逐项核实与处理

“代码确认”不等于视觉量测通过；启动闪烁、系统缩放等未测部分明确保留待验。

| 编号 | 当前结论与纠正 | 落点 |
| --- | --- | --- |
| U01 | 确认：金额按钮名称只有“费用计算明细”，真实 DOM 无 `aria-describedby`。浮层能通过 focus 打开，不能称完全不可达；缺少 live 区域本身不是错误 | UX03 |
| U02 | 部分成立：radio 当前有“☀ / ☾ / 自动”名称，前两项语义不明确、字符图标不合规范。现有 `icon` 只支持 AgentIcon，不是通用图标插槽 | UX02 |
| U03 | 确认 Naive/ECharts 浮层材质偏差；关闭确认弹窗已有 85% 背景和 16px blur，不重做。费用浮层外框实测 512px，另需修盒模型 | UX01 |
| U04 | 确认表格字号偏差。表头主文字色、正文次要色，并非二者同色；必须局部改 DataTable，不能改 `common.fontSizeSmall` 影响全站控件 | UX01 |
| U05 | 确认合计行 400。死选择器实际是 `.total-row strong { font-weight: 700 }`；按规范将真实合计单元格设为 600 | UX01 |
| U06 | 实测“近14天 / 近30天”被约束，文本宽约 38.08px、内容区 33px；不是所有非“当天”项目都裁剪 | UX04 |
| U07 | 比报告更广：真实分段控件外高 36px、日期 32px、时区 28px。分段容器 `height:32px` 外加上下 padding，缺少 border-box | UX02、UX04 |
| U08 | 确认父组件模板新建数组会触发图表销毁重建；并非 watch 返回数组就每次渲染执行，子组件自身摘要切换也不是该因果链。只监听 by/mode 会漏新数据，禁止照搬 | UX05 |
| U09 | 确认首次加载与部分刷新拒绝未处理，串行加载会阻断后续区块；设置读取失败还可能显示可操作的伪默认值 | UX06 |
| U10 | 启动风险待取证：App 中 watchEffect 初次在 setup 同步执行，不是 mounted 后；当前未证明必闪白 | UX09 |
| U11 | 层次问题成立，但 DESIGN 同时规定通知条与卡片同配方。新增卡内轻提示变体，并明确规范适用范围；不删全局横幅玻璃 | UX08 |
| U12 | 确认设置页说明浮层默认 hover，部分说明用 opacity。仅给 span 加 tabindex 不足以让 Naive hover tooltip 响应 focus | UX03 |
| U13 | 统一品牌焦点环缺失；原生焦点和已修复的真实命中率 tooltip 不能说不存在 | UX02、UX03 |
| U14 | 显示词不一致确认。按当前 DESIGN 统一为“缓存命中”，技术键 `cache_read` 与“缓存命中率”保留 | UX07 |
| U15 | 确认：请求 0.0126 显示四位，公式结果显示 0.01；固定六位也会将 1e-8 显示为零。统一入口，但区分汇总金额、请求金额、每百万 token 单价 | UX07 |
| U16 | 确认方向键只 emit，选中与 tabindex 更新而实际焦点不移动 | UX02 |
| U17 | 确认未观察实际尺寸/选项变化；不能仅凭代码宣布所有系统缩放必错位 | UX02、UX10 |
| U18 | 原阈值判断错误：groups 含“合计”，当前非日维度只隐藏 0/1 个真实类别，2 类别已显示 | UX05 |
| U19 | 确认同步失败提示无动作，同时状态读取失败会被 `v-else-if` 隐藏；读取重试也缺自己的 pending 管理 | UX06 |
| U20 | 确认年份丢失；更严重的是 2024-01-01 到 2025-01-01 因短标签相同被折叠为“1/1” | UX04 |
| U21 | 大部分不成立：重建已有 loading/成功失败反馈，关闭动作已有“已保存”。只补必要操作说明与重复触发回归；不承诺机器特定秒数 | UX08 |
| U22 | 确认缺稳定目录名称和可查看的完整值；placeholder 可能提供兜底 accessible name，不能绝对称完全无名。`s.dir` 是当前生效目录，不必然是默认目录 | UX08 |
| D1 | 软状态背景值重复，补 success/warning/error-soft 两主题 token | UX01 |
| D2 | 现有主题测试未做跨 CSS/adapter 的明确语义值对照；增加映射一致性测试，不机械比较所有色值字符串 | UX01 |
| D3 | 明确未调用的 ECharts 导出、未使用的 Card 覆盖可清理，低优先级；不为“复用”引入 NCard | UX08 |
| D4 | 收敛重复浮层/表格视觉规则；动态尺寸、数据驱动 style、必要的 scoped `:deep()` 可以保留 | UX01、UX08 |
| D5 | 不把所有像素值当越阶间距：表高 380/420、最小宽 200、图表高度算法都有用途。修实际字体漂移（如 15px/650）即可 | UX08 |
| F06 | 已由 `a1c8e90` 修复，已有合成长页真实浏览器量测与截图；无需再执行旧修复 | UX10 回归 |
| F08 | 已由 `fcba1cf` 补真实命中率浮层测试；“测试全部打桩”已过时 | UX03、UX10 回归 |

### 本次真实页面取证

在 `dd6aaae` 启动 Vite，Playwright 加载真实 App/Vue/Naive 组件，所有 Tauri IPC 替换为合成响应。980×620 CSS 像素、浅色/深色，等待浮层过渡结束再量测；未运行原生 Tauri 窗口，未改变 Windows DPI。

| 量测 | 两主题结果 |
| --- | --- |
| 聚合表表头 / 正文 | 均 14px，行高 22.4px，单行外高约 43.39px；表头 500，正文 400 |
| 聚合表表头 / 正文颜色 | 浅色 `#1D1D1F / #515154`；深色 `#F5F5F7 / #AEAEB2` |
| 合计行 | 单元格 400，未达到 600 |
| 筛选栏 | 来源、聚合分段外高 36px；日期 32px；时区 28px |
| 费用浮层 | 背景分别 `rgb(255,255,255)` / `rgb(44,44,46)`，alpha=1；已有 blur(16px)，外框 512px |
| 费用触发器 | accessibility snapshot 为 `button "费用计算明细"`，文本 0.0126；无描述关联 |
| 日期弹层 | 外层实色且 blur=none；内层 85% + blur(20px)，玻璃叠在实色上，未采用 16px 浮层配方 |
| 日期快捷项 | 五项均 34×34px；近14天/近30天文本约 38.08px，被 33px 内容区限制；其余三项本次未超宽 |

持久化证据见 [量测 JSON](../../reviews/qa-artifacts/2026-10-07-ui-ux/measurements.json)、[浅色日期弹层](../../reviews/qa-artifacts/2026-10-07-ui-ux/light-date.png)、[深色费用浮层](../../reviews/qa-artifacts/2026-10-07-ui-ux/dark-cost.png)。这些是**修复前证据**，不代表所有浮层或操作系统缩放已验证。金额/类别等数据均为合成，不能用截图核对产品统计结果。

## 3. 实施任务

任务顺序：UX00 → UX01 → UX02 → UX04 → UX05 → UX06 → UX07 → UX03 → UX08 → UX09 → UX10。UX03 使用 UX07 的金额展示接口，避免两次修改触发器名称。每任务按“失败测试 → 最小实现 → 定向测试 → 自查 → 独立提交”推进；提交遵守项目门禁，不跳过 hook。

### UX00：建立真实组件量测入口

**文件：** 新增 `frontend/scripts/check-ui-contracts.mjs`、`frontend/scripts/fixtures/ui-contracts.mjs`；保留 `frontend/scripts/check-app-scroll.mjs` 原有职责。

1. Playwright 通过 URL 加载实际前端；`addInitScript` 注入合成 `__TAURI_INTERNALS__`，所有读取/写入/事件接口显式 mock。提供正常、空数据、未知价、部分计价、长模型/路径、多类别、各设置读取失败 fixture；未知命令直接失败。
2. 支持 `--url`、`--phase baseline|verify`、`--output`。baseline 只记录当前违例，verify 按本计划断言；非预期浏览器/JS/IPC 错误、未处理拒绝、未知 command、资源加载错误在两个模式均以非零退出。故障 fixture 主动返回的 IPC reject 属预期输入，应验证提示与恢复行为，不将其直接判为脚本失败。使用独立 browser context，不加载用户 profile，不访问外部服务。
3. 截图与 JSON 记录 commit、浏览器版本、viewport、deviceScaleFactor、主题、fixture、可访问性快照与量测值；等待字体就绪和 CSS 过渡完成，禁止把动画缩放中间值当最终尺寸。
4. 使用 DOM Range/scrollWidth 判断实际文本是否溢出，不能仅量被裁剪后的 span。材质读真实承载背景节点，包含外框 padding/border，不能只断言 themeOverrides 或 CSS 字符串。

**验收项名称：** `real_app_fixture_boots_without_ipc_leak`、`baseline_records_contract_violations`。UX01–UX09 的浏览器验收加入同一脚本，每项具名输出失败原因；不得注入“修正后 CSS”伪造通过。

**命令：** 独立终端运行 `pnpm --dir frontend exec vite --host 127.0.0.1 --port 1437`；随后运行 `node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 --phase baseline --output docs/plans/qa-artifacts/ui-ux-remediation/before`。完成后只停止本任务启动的进程。

### UX01：统一浮层材质、表格排版与语义色映射

**文件：** 修改 `frontend/src/styles/{tokens.css,naiveTheme.ts,chartTheme.ts}`、`frontend/src/components/{EventTable,UsageTable,DateRangeSelect,TrendChart}.vue`、`frontend/src/views/Settings.vue`；新增 `frontend/src/styles/themeContract.test.ts`。

1. 写 `adapters_match_semantic_tokens_in_both_themes`：按显式字段映射比较 CSS 的浅/深 token 与 Naive/ECharts 消费值，规范化 hex/rgba；区分 surface-solid、elevated、separator 等语义。解析 scope 后再比较，不用全文件首次 regex 命中代替主题解析。
2. 写 `table_typography_is_scoped_to_datatable`：DataTable small 正文 13px/1.45，表头 12px/500/1.4/secondary，正文 text；其他 small 控件字号与高度保持原契约。**不改 common.fontSizeSmall。** ECharts 网格线映射 separator，软状态背景改为两主题 token。
3. 浮层以一个真实外壳消费 85% elevated + 16px blur + 12px 圆角及规范阴影；为 Tooltip、Popover、日期面板、Select 菜单与 ECharts HTML tooltip 确认各自实际节点和 adapter API。无 backdrop-filter 时使用 95% 配方。处理箭头背景，避免内外两层玻璃或实色外层遮蔽。
4. 抽公共浮层/表格样式，保留公式实色区。费用浮层在真实承载节点采用 `box-sizing:border-box` 和 `max-width:min(480px, calc(100vw - 32px))` 等等价边界，长文本换行；不能仅给内部内容 max-width。关闭弹窗已有正确材质，除重复状态色替换外不改行为。
5. 合计行规则命中真实 td，600 字重，上分隔线保留；普通单行保持约 40px（±1px），复杂多行自然长高。调整 padding/line-height 必须验证设置价格表虚拟滚动，不能硬裁内容换取固定高度。

**浏览器验收：** `floating_material_matches_elevated_contract`、`cost_popover_outer_width_is_bounded`、`three_tables_use_contract_typography`、`total_row_is_semibold`。覆盖聚合、明细、设置三表；tooltip 在四边翻转、长内容、双主题均可读；无 blur 回退需实际模拟不支持环境并记录方法，不能只把 blur 设为 none 就称回退通过。

**命令：** `pnpm --dir frontend test -- src/styles/themeContract.test.ts src/components/UsageTable.test.ts`；浏览器入口按 UX10 运行，当前任务相关项应通过。

### UX02：分段控件名称、焦点与真实尺寸

**文件：** 修改 `frontend/src/components/SegmentedControl.vue`、`frontend/src/App.vue`、`frontend/src/styles/tokens.css`；新增 `frontend/src/components/SegmentedControl.test.ts`；按需更新 `frontend/src/App.test.ts`。

1. 写 `arrow_selection_moves_dom_focus`、`tab_enters_selected_radio_and_leaves_group`、`theme_radios_have_semantic_names`：真正发送方向键，检查 activeElement/aria-checked/tabindex，不只检查 emit。
2. 为选项增加独立 `ariaLabel`，为主题提供通用 SVG 渲染插槽；保留已有 AgentIcon 来源图标兼容。三项名称为“浅色模式 / 深色模式 / 跟随系统”，装饰 SVG 隐藏于读屏。不要用未经扩展的 AgentIcon 去解析 sun/moon。
3. 方向键更新选中值后 `nextTick` 聚焦相应按钮；props 被外部改变不主动抢焦点。加入公共 focus-visible 2px accent/offset 2px，确保底槽不裁剪焦点环。
4. 分段容器用局部 border-box 达到**外高**32px，内选中块与 padding 相容；不要顺手全局重置所有组件盒模型。观察容器与选项实际尺寸、选项变化，批量 nextTick 更新 thumb，卸载 disconnect，空 options 安全无动作。
5. 写 `resizing_selected_option_repositions_thumb`、`external_selection_does_not_steal_focus`；浏览器 `segmented_outer_height_is_32`、`thumb_tracks_option_bounds` 验证尺寸变化但 modelValue 未变的情况；motion-reduce 下不依赖动画结束才能可用。

**命令：** `pnpm --dir frontend test -- src/components/SegmentedControl.test.ts src/App.test.ts`。

### UX03：费用及说明浮层可访问性

**文件：** 修改 `frontend/src/components/{EventTable,SummaryCards}.vue`、`frontend/src/views/Settings.vue`；必要时新增 `frontend/src/components/HelpTooltip.vue` 复用同一交互；修改 `frontend/src/components/{EventTable.test.ts,SummaryCards.tooltip.test.ts}`，新增 `frontend/src/views/Settings.tooltip.test.ts`。

1. 先写真实 NTooltip 测试 `cost_trigger_exposes_amount_and_description`、`settings_help_opens_on_focus_and_click`、`tooltip_escape_closes_without_losing_trigger`。禁止全局打桩 tooltip；如复用 HelpTooltip，把其行为也用真实浮层覆盖。
2. 费用触发器名称使用 UX07 请求金额格式（例如“估算费用 $0.0126，查看计算明细”）；给内容稳定且唯一的 ID，显示时 `aria-describedby` 指向存在的节点。未知/部分计价名称仍明确状态，不强转 null 成数字。
3. hover、focus、click、Enter/Space 可读取，Escape/外部点击关闭。建议原生 button 并去掉多余外观；鼠标离开但仍有键盘焦点时不意外关闭。沿用 SummaryCards 已修复的 hover/focus 区分策略，避免 Escape 后焦点未移走却立即重开。
4. 设置页 priceCell/prefixCell 等说明接入同样可访问交互；说明文字用语义色，去掉低 opacity。命中率说明补公共焦点环，保留既有真实 tooltip 测试。
5. 浏览器检查 accessible name/description 实际内容与可见浮层，Tab/Shift+Tab、Escape、窗口边缘、长文案；不以存在 aria 属性作为充分证据，不增加 live 区域强制朗读整张价格表。

**命令：** `pnpm --dir frontend test -- src/components/EventTable.test.ts src/components/SummaryCards.tooltip.test.ts src/views/Settings.tooltip.test.ts`。

### UX04：日期快捷项、跨年标签与筛选栏

**文件：** 修改 `frontend/src/components/DateRangeSelect.vue`、`frontend/src/views/Dashboard.vue`；修改 `frontend/src/components/DateRangeSelect.test.ts`，必要时将纯标签函数放 `frontend/src/lib/dates.ts` 并测试。

1. 写 `same_month_day_across_years_is_not_single_day`：2024-01-01..2025-01-01 必须含两个年份；`historical_range_retains_year`、`follow_today_label_retains_start_year`；同日按完整 ISO 日期比较，不能比较 M/D 标签。
2. 当任一端点年份不是统计时区当前年，或区间跨年时展示足够年份；同年当年可保留简短标签。只改 label，保留日期草稿与 formatted-value 接口。
3. 日期快捷按钮宽度由文案+水平内边距决定，外高32px；组宽不足整组换行，不挤压、裁字。时区选择局部由 small 改 medium，不扩大所有 small 控件。
4. 浏览器 `date_shortcuts_do_not_clip_text`、`filter_controls_have_equal_outer_height`：980/1280窗口，四组外高32px（±1px），快捷项有完整文本与可点击区；长日期可读且不造成页面横向滚动。

**命令：** `pnpm --dir frontend test -- src/components/DateRangeSelect.test.ts src/composables/timezone.test.ts src/views/Dashboard.test.ts`。原 R03 的日期/时区回归必须继续通过。

### UX05：图表实例生命周期与类别展示

**文件：** 修改 `frontend/src/views/Dashboard.vue`、`frontend/src/components/TrendChart.vue`、`frontend/src/lib/chartData.ts`；修改 `frontend/src/{views/Dashboard,components/TrendChart,lib/chartData}.test.ts`。

1. 父级用 computed 缓存排除合计后的真实 groups，停止在模板每次 filter。以真实类别数判断：≥1 展示图表；0 使用明确空状态，不显示无内容图，也不把合计当类别。
2. 写 `unrelated_parent_update_does_not_touch_chart`、`new_groups_update_existing_instance`、`dimension_switch_removes_obsolete_axes`、`same_dimension_refresh_preserves_zoom`。断言无关变化 init/dispose/setOption 都不增加，新数据必须更新 series/轴与摘要。
3. 初始化、数据更新、主题变化、卸载分离。数据更新调用 setOption；稳定 series id，以 merge 更新同维度数据并保留适用 zoom，维度改变完整替换 option/明确清理旧轴、series、dataZoom。必要时显式保存/恢复 zoom，不允许旧配置残留。
4. 为降低主题适配风险，主题切换允许一次 dispose/init，恢复同维度适用的 zoom；resize 仅 resize，卸载释放实例和观察器。不得只监听 by/mode，也不在每次数据更新时重建实例。
5. 写 `zero_one_two_categories_have_explicit_rendering`，覆盖 day/model/project/agent、合计存在/空数组。真实浏览器追加高类别数滚动、切维度、切主题后完整类别和摘要一致性。

**命令：** `pnpm --dir frontend test -- src/components/TrendChart.test.ts src/views/Dashboard.test.ts src/lib/chartData.test.ts`。

### UX06：设置首载与定价横幅错误恢复

**文件：** 修改 `frontend/src/views/Settings.vue`、`frontend/src/components/PricingStatusBanner.vue`；修改同名 `.test.ts`。

1. 写 `settings_initial_failures_are_independent_and_retryable`：分别拒绝 source_status/cache_stats/pricing_entries/settings_get/autostart_status；其他成功区块仍显示，错误区块显示原因与局部重试，无 unhandled rejection。
2. 一次读取 settings_get 初始化依赖它的配置区块；与其他独立读取并发，但各自处理失败，不用一个共享 loading/error 覆盖全部结果。没读到配置前禁用相应写入控件并说明原因，不能把“每次询问”等默认值当读取成功。
3. 写 `retry_preserves_dirty_source_drafts`、`late_settings_response_does_not_overwrite_saved_value`：重试/刷新只更新未编辑草稿；为相关请求加代次/卸载守卫，未返回的旧请求不覆盖新编辑或保存值。保持现有后端接口，不为 UI 状态引入数据库改造。
4. syncPricing 后的 pricing_entries 刷新失败单独归类为“价格列表读取失败”，保留旧数据、保证 finally 退出 busy 并通知横幅。来源保存成功后 source_status 读取失败也应区分“已保存，状态刷新失败”，避免误导用户重复保存。
5. 写 `sync_and_status_failures_remain_visible`、`status_retry_does_not_sync_network`、`partial_sync_preserves_usable_pricing`、`retry_ignores_duplicate_activation`。横幅同步重试与状态读取重试各有 pending/防重复；两类错误均可读可操作，可合并一条通知并展开详情。
6. 状态未知时只说同步/读取失败；只有返回证据支持才说部分可用。成功清理对应错误，旧请求不得覆盖新结果；初次无价的联网提示保留。

**命令：** `pnpm --dir frontend test -- src/views/Settings.test.ts src/components/PricingStatusBanner.test.ts`。浏览器 fixture 用按钮真实触发失败→重试→恢复，不仅调用组件方法。

### UX07：金额与 token 显示词统一

**文件：** 新增 `frontend/src/lib/{formatMoney.ts,formatMoney.test.ts,tokenDisplay.ts}`；修改 `frontend/src/types.ts`、`frontend/src/lib/{costBreakdown.ts,tieredPrice.ts,chartData.ts}`、`frontend/src/styles/chartTheme.ts`、相关四类 token 展示组件及 `Settings.vue`；保留现有业务字段。

1. 写 `request_amount_matches_breakdown_result`（0.0126）、`tiny_nonzero_is_never_displayed_as_zero`（1e-8）、`zero_and_unknown_are_distinct`、`unit_price_keeps_its_precision_and_unit`。建立单一格式化入口，显式区分 summary/request/unit 场景，不把单价与费用视为相同单位。
2. 请求金额与公式结果用同一规则：0 明确为 $0.00，0<金额<1 至少保留现有四位精度，小值按需六位；若仍会舍入为零，改为可读科学记数法等明确非零表达。汇总可保留两位习惯但同样保护微小非零值。单价保留其有效精度并注明 USD/1M token；不改 numeric 值，不以格式化字符串参与计算。
3. 新入口明确是否包含 `$`（建议返回完整 USD 文本），一次迁移所有调用方或保留有测试的兼容 wrapper，避免标题/模板重复添加货币符号。未知状态由类型/调用点明确传递，不用 0 代替。
4. tokenDisplay 仅保存 key/显示名/顺序，依次输入、输出、缓存写、缓存命中；颜色仍由主题适配器提供。表头、图例、摘要、说明和公式消费同源元数据，缓存命中率及后端 key 不机械改名。
5. 写 `all_token_views_share_labels_and_order`；保留显式 SameAsInput 缓存读使用输入价、缺失价格仍未知、部分价格提示、候选排除原因等原有 costBreakdown 测试，不为了通过快照删掉说明。

**命令：** `pnpm --dir frontend test -- src/lib/formatMoney.test.ts src/lib/costBreakdown.test.ts src/lib/chartData.test.ts src/components/SummaryCards.test.ts src/components/EventTable.test.ts`。

### UX08：设置页层次、目录信息与有限清理

**文件：** 修改 `frontend/src/views/{Settings,Dashboard}.vue`、`frontend/src/styles/{tokens.css,naiveTheme.ts,chartTheme.ts}`、`frontend/src/views/Settings.test.ts`、`DESIGN.md`。

1. 为通知条添加卡内变体，取消卡内独立大阴影/外框，保留图标、状态文本、详情与重试；卡外通知继续玻璃配方。在 DESIGN 明确该适用范围，避免规范自相矛盾。
2. 给来源目录实际 `<input>` 提供稳定名称（Naive `inputProps`/label 关联），如“Claude Code 日志目录”；技术详情显示完整“当前生效目录”，可选择复制、长路径换行。保留空输入恢复默认的行为，不能把有效覆盖目录叫“默认目录”。
3. 重建缓存旁加简短预期“重新扫描日志，可能需要一段时间”；保留 loading 和结果消息，只在缺少函数级防重复时补 guard。关闭动作保留已有已保存反馈，不加确认弹窗。
4. 写 `source_directory_input_has_stable_name`、`effective_directory_is_available_in_full`、`rebuild_keeps_progress_and_prevents_repeat`。断言保存前后已有错误状态不丢失。
5. 将空状态标题 15px/650 对齐既有合适字阶（卡片级结论用17px/600）；仅删除明确无引用的 Card 覆盖与 echartsThemeObject。保留有用途的表高、图表算法尺寸、动态 style 与局部布局值，不为通过审查修改整个间距标度。

**命令：** `pnpm --dir frontend test -- src/views/Settings.test.ts src/views/Dashboard.test.ts src/composables/theme.test.ts`；`pnpm --dir frontend typecheck`。目录名称与长路径另经真实 browser textbox/布局验收。

### UX09：启动主题先取证，再决定最小修复

**文件：** 检查 `frontend/index.html`、`frontend/src/composables/theme.ts`、`frontend/src/App.vue`、`src-tauri/tauri.conf.json`；只有确需浏览器预启动修复时新增 `frontend/src/lib/themePreference.ts` / 对应测试，或等价共享无 Vue 依赖 resolver；更新 QA 记录。

1. 浏览器入口增加 `prepaint_theme_matches_preference`：组合 localStorage light/dark/system/缺失/非法值与系统明暗；延迟 App 主模块加载，记录 CSS 首次可绘制时主题、后续切换与背景。先确认浏览器白屏/浅色画布还是原生窗口背景问题，不把 DOMContentLoaded 后截图称“首帧”。
2. 若首次可绘制画布不符合选择，在 head 的最早可执行阶段解析偏好并设置 data-theme；共享存储键/解析规则，处理 localStorage 异常，避免提前 import Vue 大模块。生产构建也必须验证脚本顺序、CSP 与错误降级。
3. 若 browser 已符合要求则记录无须修改；原生 Tauri 冷启动另行记录证据。若原生窗口仍闪烁，只在确认窗口主题 API 能随解析结果设置且双主题均验证后修复；不硬编码近黑色“至少解决深色”。
4. 写/保留 `preference_resolution_is_shared`、`invalid_or_unavailable_storage_falls_back_to_system`、`system_changes_only_affect_system_preference`。本任务可“已取证、无需代码修改”结束，但必须有对应证据；未跑原生启动则保留待验，不写已修复。

**命令：** `pnpm --dir frontend test -- src/composables/theme.test.ts`（新增 resolver 测试同时运行）；生产 preview 下运行浏览器入口，原生验收采用 UX10 的手工矩阵。

### UX10：集中验收与文档回写

**文件：** 完成 `frontend/scripts/check-ui-contracts.mjs`；新增 `docs/plans/qa-artifacts/ui-ux-remediation/` 脱敏截图/JSON；修改 `docs/plans/README.md`、`docs/plans/active/2026-10-06-design-system-visual-qa.md`、本计划；原始审查保留历史并链接终态。

1. 执行以下自动矩阵：浅/深 × 1280×820/980×620；正常/空/未知价/部分失败；设置重试、键盘 Tab/方向键/Enter/Space/Escape、长日期/模型/路径、多类别图表、浮层四边、reduced-motion。具名断言全部通过后截图，不用截图代替交互断言。
2. 在**实际 App**长页滚动时检查内部 scroller 的 scrollTop、导航顶边与通知条可见性；保留既有 `check-app-scroll.mjs` 结构回归。当前合成 CSS 长页已证明修复，不等于真实产品视觉 QA 全部完成。
3. 用真实 Windows/Tauri 运行浅/深主题的100%/125%/150%系统缩放、冷启动和原生窗口背景验收；记录系统缩放、窗口逻辑/物理尺寸和截图。浏览器 deviceScaleFactor 只改变栅格密度，不能替代系统缩放通过记录。环境无法覆盖的组合留“待验”。
4. 对合成明细核对已知/部分/未知显示，单价、候选渠道、档位等字段没有因格式化或换肤丢失；只检查展示与 DTO 一致，不新增第二套定价算法。
5. 每项记录修改文件、测试名称、红→绿证据、截图、剩余限制与提交；完成后按仓库约定归档并修正链接。旧 D5 安装验收不与本文的设计债 D5 混同，未实际执行不得代签。

**自动命令：**

```powershell
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
# 独立终端启动生产预览；完成后只停止自己启动的进程
pnpm --dir frontend exec vite preview --host 127.0.0.1 --port 1437
# 在另一终端执行
node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 --phase verify --output docs/plans/qa-artifacts/ui-ux-remediation/after
node frontend/scripts/check-app-scroll.mjs
```

提交前另跑项目完整 Rust 门禁：

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --offline -- -D warnings
cargo test --workspace --offline --quiet
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --offline -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --offline --quiet
```

每条命令均需检查退出码；根 manifest 不替代壳 manifest。任何验证失败先排查，不跳过 pre-commit；严格依次验证→暂存明确文件→commit→push 当前分支。

## 4. 完成定义与执行记录

- [ ] UX00–UX08 各任务具名测试通过，真实组件量测证明材质、字号、行高、外框尺寸与焦点符合规范。
- [ ] 图表无关更新不重建，新数据照常更新；日期跨年显示正确，统计参数不变。
- [ ] 费用名称包含金额、描述可读取；未知与极小非零不混成零，公式结果与请求金额展示一致。
- [ ] 设置失败可局部重试、不丢草稿；同步与状态读取错误均可恢复，没有未处理拒绝。
- [ ] UX09 首次绘制与原生启动证据分别记录；没有把未复现问题写成确定故障或把未测试项写成通过。
- [ ] UX10 自动矩阵通过；系统缩放/安装等无法覆盖项明确待验。必要真机项未验时只标“实现完成、验收未闭合”，不整体宣告完成。
- [ ] 原 F06/F08、关窗失败恢复、日期接口、缓存快照、定价/统计回归均保留；文档归档与索引同步。

| 项目 | 状态 | 证据 |
| --- | --- | --- |
| 本次核实 | 已完成 | `dd6aaae` 代码对照、真实 App 合成 IPC 浅深主题量测、上文持久化证据 |
| 基线门禁 | 已通过 | 根库/壳 fmt、clippy、test；前端 typecheck、format、177 个测试、build；未运行 ignored 真实性能测试；build 有现存 chunk 体积提示 |
| UX00–UX10 实现 | 待执行 | 本次仅新增核实结果与计划，未修改产品代码 |
| 系统缩放/原生首帧 | 待验 | 本次未运行原生窗口或改变 Windows DPI |
