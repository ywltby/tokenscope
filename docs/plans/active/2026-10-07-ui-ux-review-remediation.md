# 安全、数据一致性与 UI/UX 审查修复实施计划

> **当前状态（2026-10-08，复核基线 `22c9c26`）：** 已有实现与历史执行记录保留；复核确认 SF04、UX03、UX06、UX07 仍有计划内缺陷，UX10 自动矩阵覆盖不足，不能认定整体完成。后续按 [复核遗留缺陷与验收补齐计划](2026-10-08-recheck-remediation.md) 的 RC01–RC11 执行；本次仅回写状态及新增计划，产品修复尚未开始。
>
> **2026-10-09 再复核承接：** SF09 的**采集层**拒绝重叠已生效，但保存入口漏掉「缺省字段 = 默认启用 + 默认根」，用户仍能保存采集层必然拒绝的配置 → AP01；坏配置下查询/状态回退默认来源 → AP02；重建缓存不走用户来源配置 → AP03；UX06 的异步失败/卸载边界与文案缺口 → AP04/AP05/AP06。上述五项由 [全计划终态复核与遗留修复](2026-10-09-all-plans-final-recheck.md) 承接，本文件对应勾选项在 AP09 归并后才可视为闭合。
>
> **执行说明：** 使用 `executing-plans` 技能逐任务执行。先写能复现问题的测试，再做最小修复；原计划其余有效实现作为回归约束，不重复返工。

**目标：** 优先修复经核实的 HTML 注入、设置丢更新、价格加载与查询一致性问题，再落实界面、键盘交互、错误恢复以及 `DESIGN.md` 第二版玻璃与双主题规范。

**架构：** 保留 Rust + Tauri + SQLite + Vue/Naive/ECharts。后端以设置事务、不可变查询快照和带健康状态的有效价格加载结果明确边界；前端保留请求代次与刷新批次保护。只做这些缺陷需要的局部接口调整，费用仍由后端计算；公共视觉规则集中在 token/adapter。合成数据测试真实展示边界，确定性交错测试并发行为。

**技术栈：** Rust stable、Tauri 2、SQLite、TypeScript 5、Vue 3、Naive UI、ECharts、Vitest、Playwright/Chromium；接口变更仅限查询上下文、价格展示与明确错误状态，并同步 Rust/TS 契约。

**基线：** `dd6aaae`（2026-10-07），分支 `docs/product-review-plan`。原 [UI/UX 审查](../../reviews/2026-10-07-ui-ux-review.md) 基于 `1cefac7`，其中部分结论已过期或需更正。实现时如 HEAD 前进，先重新核对涉及文件。

**合并复核基线：** `3f036bb`（产品代码与 `dd6aaae` 相同）。2026-10-07 按用户补充的技术审查新增 SF01–SF11，沿用本文件与 UX 编号，不另立冲突计划。新增技术问题已核对源码及实际依赖；除下文已有 UI 量测外，不声称已完成攻击、并发、文件锁、真机启动等运行时复现。

---

## 1. 范围、优先级与不变量

本计划承接原审查的 U01–U22、D1–D5，以及第 2.1 节技术审查。SF01 HTML 注入和 SF02 设置丢更新先行，随后修价格失败缓存、分页与日期范围，再处理启动降级、展示三态和异常输入；视觉任务在这些边界稳定后继续。原 UI 报告的“两项新增 Blocker”不能直接当成已证实的阻断级别。

非目标：重写项目或更换技术栈、替换组件库、生成整套主题编译系统、推翻完整候选优先/最高费用估算和 Codex 启发式去重、恢复内置价格、增加“强制用户覆盖价”产品模式、升级依赖、扩大 agent 支持、改用 DuckDB/动态插件、建立笼统统一缓存框架、重新实现已修复的导航与关窗流程。

必须保持：

1. `cost_usd`、候选渠道、单价、档位、未知量均来自后端；前端不重新选价、不重算费用、不把未知当零。仅明确声明 `SameAsInput` 的缓存读使用最终输入价，缺失价格仍为 Unknown；完整候选优先与溢出诊断等最新实现不变。
2. 日期继续用 `formatted-value` 与 `yyyy-MM-dd` 字符串；标签修复不改变时区、闭区间、草稿/取消/确认、跟随今天语义。
3. 保留共享刷新批次、请求代次和卸载保护；SF04 在其上增加后端查询快照，不能把前端 epoch 当成后端 generation。游标协议及视图快照版本仅在新契约需要时升级，旧磁盘视图可显示为 stale，但旧游标不得附着新采集结果。不得为了减少重绘删除数据监听。
4. 双主题同等完整；表格和费用公式保持实色衬底，浮层采用 elevated 材质。不能把所有表面统一透明，也不能靠降低文字 opacity 达成层次。
5. 所有说明浮层有键盘路径；真实可访问名称含当前值。普通 tooltip 不强制使用 `aria-live`；不引入重复朗读。
6. 失败不能伪装成默认值、成功或“主源已可用”；保留已加载数据与用户未保存草稿。重试状态读取不应偷偷触发联网同步。
7. F06 导航布局、F08 真实 tooltip 验证与关窗失败可重试作为回归约束，不重做。缓存重建是派生数据操作，保留现有进行中/完成反馈，不增加强制确认。
8. 所有自动测试使用合成数据；浏览器测试所有 IPC 被 mock，未列出的 command 显式报错；原生 Tauri 契约验收使用注入的临时来源/设置/缓存路径。不得扫描真实 agent 日志或写真实 `~/.tokenscope`；不运行 ignored 真实性能测试。
9. 日志字段属于不可信数据：进入 HTML 时必须由 textContent/安全编码输出，CSP 只作为第二层防护；不把窗口脚本执行夸大成未经证明的系统 RCE。
10. 设置原子替换与业务事务同时保留：全体应用内写者共享同一读改写锁，解析/校验/写入失败不覆盖原文件；用户关闭自动同步的值不得被另一字段的旧副本复原。
11. 外置文件不存在与读取失败不同；失败降级要可见，不得作为同签名健康缓存长期复用。设置页、可用性和估算消费同一组已校验价格，不将 Unknown/SameAsInput 转成 0。
12. 同一查询快照冻结事件、价格对象及版本、解析后的时间范围和时区；翻页不重新采集、不重新编号、不换价格。过期/错查询游标明确拒绝并允许刷新，不静默从新快照续页。
13. 源数据单条异常可诊断地跳过，聚合无法表示的溢出明确报错；不用回绕或饱和截断制造正常结果。预设近 N 天为 `[起始自然日, 今天]`，用户显式未来区间和“全部时间”不被擅自截成今天。

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

持久化证据见 [量测 JSON](../../../qa-artifacts/2026-10-07-ui-ux/measurements.json)、[浅色日期弹层](../../../qa-artifacts/2026-10-07-ui-ux/light-date.png)、[深色费用浮层](../../../qa-artifacts/2026-10-07-ui-ux/dark-cost.png)。这些是**修复前证据**，不代表所有浮层或操作系统缩放已验证。金额/类别等数据均为合成，不能用截图核对产品统计结果。

> **产物位置（2026-10-07 调整）：** 浏览器验收产物（截图、量测 JSON、控制台日志）统一落在
> **仓库根 `qa-artifacts/`**，并已加入 `.gitignore`——体积大且每次运行都会重写，属可再生的
> 生成物，不作为源码提交；上列链接指向本机该目录。

### 2.1 新增技术审查的核实、纠偏与覆盖关系

| 用户审查项 | 源码核实结论（`3f036bb`） | 任务与范围 |
| --- | --- | --- |
| 1：图表存储型 XSS 路径 | 确认 `TrendChart.vue` formatter 将 `fullLabels()` 原文拼入 `<br/>` HTML；安装的 `echarts/lib/component/tooltip/TooltipHTMLContent.js` 将字符串写到 innerHTML；Tauri csp=null。需要可控模型/项目等被显示字段与 tooltip 触发，不是任意对话文本即可触发；未证明系统 RCE | **SF01，首修**；与 UX01 材质、UX05 生命周期共同维护安全 tooltip |
| 2：并发设置丢更新 | 确认关闭动作、记忆关闭、自动同步、来源配置均独立 load→save；ensure_toml 首建/迁移也是写者。原子写不能阻止旧副本覆盖其他字段 | **SF02，首修**；UX06 busy/草稿保护仅处理前端体验，不能替代事务 |
| 3：外置价格失败缓存 | 确认 `Pricing::load` 对 read_to_string 的所有 Err 直接返回；load_cached 将降级产物写内存/索引，签名仅路径/size/mtime。需同时处理故障恢复和既有健康缓存命中时的来源不可读 | SF03；与 SF10 共用加载健康状态，不能只增加 warning |
| 4：活跃日志翻页重复/漏行 | 确认每次 list_events 重新采集，丢弃 generation，按 `(ts,rid)` 当前组内位置分配 seq。静态数据测试不覆盖组内插入/删除；不同时间戳新增不必然触发 | SF04 冻结查询快照；不重写前端已有批次保护 |
| 5：今天/近 N 天含未来 | 确认后端 `filter_days` 只有下界且内部读取 now；**当前 GUI 快捷项传 from/to 双边区间，不能泛称 GUI 全部有此 bug**。GUI 构造范围/标签时仍应每次操作只读取一次 today | SF05 + UX04；只修相应时间解析路径 |
| 6：日志不可写阻断启动 | 确认 logging::init 在窗口创建前调用 rolling::daily；依赖 tracing-appender 0.2.5 的 new 对初始化结果 expect；subscriber.init 也应消除可恢复初始化失败的 panic | SF06；不与 UX09 主题首帧混为一谈 |
| 7：价格展示三态丢失 | 确认 OpenRouter 对照 DTO 用 f64/unwrap_or(0)，主表 resolve_direct 将 SameAsInput 压成 null；TS 分段视图同样只声明 number/null，需一并对齐 | SF07 + UX07/UX03；不改变实际估算算法 |
| 8：异常 token 算术 | 确认 Codex 零值/守恒判断为普通 u64 加法；model 桶累计与聚合也有未检查加法。是异常输入容错缺口，不描述为正常高频故障 | SF08；边界 fixture 覆盖 debug/release |
| 架构 1：查询上下文 | 现有 CollectionSnapshot 已含 generation/pricing，但只用于并发单飞；不是持久分页会话，也未冻结 today | 合并 SF04/SF05，复用现有管线，不独立造缓存平台 |
| 架构 2：有效价格入口 | 确认 pricing_status 先按原始 entries.len 判主/补充源可用，再另外 load 校验；全被拒绝仍可能报告有价 | SF10，派生自有效候选；原始条数只作诊断 |
| 架构 3/4：事务、边界测试 | 建议成立，但不单列泛化重构；设置交错、日志变动、暂时不可读、真实 tooltip、未来时间、DTO 契约分别落到 SF01–SF10 测试 | 每任务先红→绿，既有全绿不能代替定向复现 |
| 目录重叠按先扫认领 | 确认 dedup_source_overlap 在解析前给 Claude 优先文件归属，既有测试只覆盖共享 Claude 文件。配置写入已有重叠校验，但手工/旧配置和发现路径仍可绕过 | SF09 明确拒绝不安全重叠，**覆盖旧计划“先扫描者认领”的处理方式**；保留防重复目标 |
| Codex 启发式去重 | 是文档已接受的限制，不作为新 bug，不简单关闭。稳定分页身份和“请求去重身份”是不同概念 | SF11 同步用户说明；SF04 不改 dedupe key |
| 最高候选价与外置优先 | 当前最高完整候选估算规则保留；外置低价没有无条件覆盖高价不是本轮 bug；缺价候选排除后也不能宣称严格全渠道上界 | SF11 修正文档和展示说明，不实现账单或强制覆盖模式 |
| README 过时 | 确认功能段仍写内置四层、统计段另一套三层与 CLI 参数；CLAUDE/口径文档部分开头也有容易误读的层级/旧游标说明 | SF11 统一当前权威说明，归档历史不全盘改写 |

## 3. 实施任务

总顺序：**SF01 → SF02 → SF03 → SF05 → SF04 → SF06 → SF07 → SF08 → SF09 → SF10 → SF11**，然后继续 **UX00 → UX01 → UX02 → UX04 → UX05 → UX06 → UX07 → UX03 → UX08 → UX09 → UX10**。SF01 可先建仅覆盖注入的真实浏览器入口，UX00 后续扩充，不等待完整视觉测试平台。SF03 先确立加载健康契约，SF10 复用；SF07 先修价格语义，UX07 只统一格式；SF05 的冻结时间解析供 SF04 使用。每任务按“失败测试 → 最小实现 → 定向测试 → 自查 → 独立提交”推进，不跳过 hook。

SF04/SF05/SF07/SF08/SF09/SF10 涉及查询或数据契约时，实施该任务前先在 `docs/stats-semantics.md` 对应章节写清新不变量，再修改实现与必要缓存版本；SF11 负责最后统一文档，不代表可以把口径说明全部延迟到最后。

### SF01：安全输出图表 tooltip，并增加 CSP 防线

**文件：** `frontend/src/components/TrendChart.vue`；新增 `frontend/src/lib/{chartTooltip.ts,chartTooltip.test.ts}`、`frontend/scripts/check-chart-tooltip-security.mjs`；`src-tauri/tauri.conf.json`；仅必要时调整 `src-tauri/capabilities/default.json`（不得扩大权限）。

1. 先写纯函数/DOM 测试 `chart_tooltip_treats_labels_as_text`，涵盖模型/项目全名、fallback name、系列名、`<>&"'`、中文、长字符串。首选 formatter 返回由 `document.createElement` + `textContent` 构建的 HTMLElement，换行/布局用固定节点；安装的 ECharts 支持 DOM 分支。原始字段不参与 innerHTML、属性名、URL 或 CSS 拼接。不要用自制黑名单正则“消毒”。
2. 在真实 ECharts 上显示包含无害测试哨兵的 `<img ... onerror=...>` / SVG 类 payload 标签，用 dispatchAction 或真实悬停触发 showTip。测试页仅允许本地 fixture，IPC stub 记录调用、外部请求拦截；断言恶意标签完整作为文本可读，没有注入元素、哨兵未变、没有攻击诱发 IPC/外联。仅 mock formatter 返回值不算完成。
3. 对 HTML 输出修复的浏览器回归须在无 CSP 的隔离测试页也通过，证明安全来自输出边界，而非 CSP 掩盖漏洞。增加双主题/项目与模型维度测试，UX05 后续修改必须保留此回归。
4. Tauri 生产 CSP 启用最小来源策略：default-src self；script 限制 self/Tauri 注入所需 hash/nonce，禁止 unsafe-eval 与 script unsafe-inline；object-src none、base-uri none，connect-src 只保留验证过的 Tauri IPC 来源。图像/字体限制为本地来源及实际必要的数据 URI，不能只限制 fetch 却放行任意外部图片。前端无需直接访问价格源，联网留 Rust。Naive/ECharts 的动态内联样式需明确 style 策略，不能为禁 inline 样式破坏界面，也不能因样式需要就放开脚本。
5. devCsp 单独配置 Vite HMR 的确切 localhost/WebSocket 来源；生产不能照搬开发白名单。不猜 IPC scheme：按当前 Tauri/WebView2 实际运行验证，并保持权限边界。测试生产构建中脚本 payload 被阻止，同时 IPC、Naive、ECharts、主题和日期组件正常。普通 Vite preview 不代表 Tauri 注入后的 CSP 已通过；未测原生时记录待验。

**测试/命令：** `pnpm --dir frontend test -- src/lib/chartTooltip.test.ts src/components/TrendChart.test.ts`；本地 Vite 运行后 `node frontend/scripts/check-chart-tooltip-security.mjs --url http://127.0.0.1:1437`（新增入口，具名检查 `real_echarts_does_not_interpret_untrusted_html`）；`pnpm --dir frontend build`；Tauri 生产 CSP 验收纳入 UX10，不连接真实数据源。

### SF02：设置统一事务更新，覆盖所有写入口

**文件：** `src/settings.rs`、`src-tauri/src/commands.rs`；测试同文件与新增 `tests/settings_transactions.rs`。

1. 根库提供 `update(path, mutate)`：持有同一进程级设置写锁 → load 最新文件/遗留值 → mutate 与校验 → 原子 save → 返回提交后的值。应用单实例，先用覆盖所有设置文件更新的单锁即可，不引入锁注册表；只在 spawn_blocking 内等待，锁不跨 await。底层 save_unlocked 私有，避免调用方绕过事务或递归加锁。
2. 自动同步开关、关闭动作、close_resolve 的 remember、来源配置、ensure_toml 首建/迁移均收敛到该入口/同一锁。来源重叠校验放在锁内、基于最新其他来源值。读取错误/非法配置拒绝更新，绝不用默认 Settings 覆盖损坏文件；保留原子替换、遗留备份及关闭失败可重试。
3. 确定性交错测试：让“修改关闭动作”进入读改写临界区并通过 channel 暂停，发起“关闭自动同步”，恢复前者，等待两者完成；最后 close_action 正确且 auto_sync=false。测试第二个 writer 已请求进入而不是在锁内等双方 barrier，以免测试本身死锁；不用 sleep 猜顺序。
4. 增加来源配置与 ensure_toml/迁移的并发测试，覆盖写入/校验失败保留原字节。测试用临时路径及可注入同步点，不改变全局 HOME。前端 busy 仍有用，但不作为此任务修复证明。
5. 本事务保证应用内写者互不丢更新；任意外部编辑器并发写入不受进程锁约束，要在配置说明明确限制。不得宣称跨进程可串行化，也不顺手改变用户手工编辑能力。关闭开关不等于取消此前已经发出的网络请求，本任务保证后续读取不会被旧设置副本重新开启。

**测试/命令：** `settings_transactions_preserve_unrelated_fields`、`ensure_toml_cannot_overwrite_concurrent_update`、`failed_update_preserves_original_bytes`；`cargo test --offline --test settings_transactions`；`cargo test --manifest-path src-tauri/Cargo.toml --offline settings`、`cargo test --manifest-path src-tauri/Cargo.toml --offline close`。

### SF03：价格读取失败显式降级，恢复后可重新加载

**文件：** `src/pricing.rs`、`src/report.rs` 的价格读取接线；新增 `tests/pricing_read_recovery.rs`，扩展 `tests/pricing_index_restart.rs`。

1. 引入内部 `PricingLoadOutcome`（名称可按工程习惯调整）：已校验 Pricing、按源健康状态/诊断、revision/signature、能否缓存。NotFound 表示可选文件不存在；PermissionDenied、共享占用、InvalidData/非法 UTF-8 等读取失败必须携带路径和原因告警，不能与不存在同分支。
2. 外置文件通常很小：在复用价格缓存之前读取一次字节并验证 UTF-8，签名纳入其内容摘要，再把同一份读取结果交给 parser，避免检查后再读不同内容。这样不仅“失败后恢复”能重试，“旧成功缓存已在内存而文件随后不可读”也不会被直接命中遮蔽。只比较 mtime/大小或 File::exists 不足够。
3. 任一来源暂时读取失败，本次可以返回其余来源的降级结果和告警，但不得写为可复用成功内存项/磁盘索引。收敛正常重建与非法索引回退两条缓存写入路径，共用 cacheability 判断，不根据 warning 文本判断是否失败。有效候选被拒绝的业务诊断与临时 I/O 失败分开。
4. 旧索引可能已永久记住无告警的失败产物：递增当前 INDEX_VERSION 并重建，保留原候选合法性校验。缓存命中也保留健康诊断；故障恢复后重新读、解析、应用 model_policy，再发布成功 revision，不要求用户改文件长度/时间戳。
5. 用临时文件与注入 reader 做确定性“同内容、同 size/mtime、第一次 PermissionDenied、第二次成功”测试；Windows 另用禁止共享读取的临时句柄复现共享锁。记录“索引未发布失败结果、内存未当作成功命中、恢复后外置价/model_policy 生效”；冷启动子进程覆盖磁盘恢复，不污染全局 PRICE_CACHE 或用户目录。

**测试/命令：** `external_not_found_is_distinct_from_read_failure`、`transient_read_failure_is_not_cached`、`healthy_cache_does_not_hide_new_read_failure`、`same_metadata_recovery_restores_external_policy`、`restart_rejects_legacy_degraded_index`；`cargo test --offline --test pricing_read_recovery`；`cargo test --offline --test pricing_index_restart`。

### SF04：查询上下文冻结事件、时间与价格，分页绑定快照

**文件：** `src/report.rs`，新增 `src/query.rs` 并在 `src/lib.rs` 导出必要接口；`src-tauri/src/{commands,lib}.rs`；`frontend/src/{types.ts,views/Dashboard.vue,lib/viewSnapshot.ts}`；新增 `tests/query_snapshot_contract.rs`，更新 `tests/e2e_events_range.rs`、`frontend/src/{views/Dashboard,lib/viewSnapshot}.test.ts`。

1. 采用**短生命周期不可变快照**，不在活跃日志上重算 seq，也不为分页修改 Codex dedupe key。新增 begin_query 等价入口返回 query_id/context 元数据；后台一次采集得到 Arc 事件集、Arc Pricing、generation、pricing_revision、SF05 冻结的 as_of/tz/范围、有效来源配置身份。summary 与首页/后续页显式使用此 query_id。
2. 注册表只管理本进程查询会话，复用现有采集/单飞与 SQLite 路径；限制活跃数量与总内存预算、空闲 TTL，使用可注入时钟测试回收。实现时将默认限制写为具名常量并记录选值与量测，优先淘汰闲置旧会话；容量不足明确报错，不截断事件集。避免每页克隆整批事件；排序索引/游标位置只在同一快照内计算一次。不得仅把 generation 写进游标却仍重新采集。
3. 新游标版本绑定 query_id、主查询指纹、下钻过滤指纹、快照内唯一行位置（seq 在固定序列内可用），校验边界/版本/归属；模型/项目/日期切换后的旧游标不可续用。过期/被淘汰/重启失效时返回结构化 `query_expired` 或等价错误，非法/错查询返回对应错误，禁止默默换成第一页或新采集结果。
4. 同一快照 total、排序、候选与金额恒定；日志追加/删除/重排、价格同步、跨午夜只影响新 query。手动刷新创建新 query 并切换批次；来源/时间主筛选变化也新建，下钻在同一快照应用固定过滤。新快照建立失败保留旧视图标 stale，不能伪装已更新。
5. Dashboard 每个现有 refreshEpoch 共享一次 begin_query Promise，然后汇总/明细并发读取；保留 seq/disposed/epoch 守卫，不允许旧 begin_query 晚到覆盖新批次。保存磁盘视图时校验同一后端 query_id/price revision；按实际 DTO 变化升级前端视图快照版本，旧视图只作 stale 展示，重新启动后不能拿旧游标继续追加。
6. 冻结时间需在查询开始解析一次，generated_at/查询元数据与之可解释；价格变化不在旧分页中偷偷重算。有效候选估算逻辑不变。测试显式修改合成日志与价格文件，验证旧 query 完整遍历无重漏，新 query 才反映变化；不同筛选的并发 query 互不污染。

**测试/命令：** `paging_same_timestamp_insert_delete_keeps_snapshot_rows`、`paging_keeps_price_revision_until_refresh`、`summary_and_events_share_query_context`、`expired_or_foreign_cursor_requires_refresh`、`old_disk_view_cannot_resume_live_cursor`、`query_registry_evicts_with_explicit_expiry`；`cargo test --offline --test query_snapshot_contract`；`cargo test --offline --test e2e_events_range`；`pnpm --dir frontend test -- src/views/Dashboard.test.ts src/lib/viewSnapshot.test.ts`。UX00 fixture 同步新增 query command/元数据。

### SF05：统一预设时间范围的双边界和时间基准

**文件：** `src/aggregate.rs`、`src/report.rs`；`frontend/src/components/DateRangeSelect.vue`；`tests/e2e_events_range.rs` 和对应日期组件测试。

1. 提供以注入 as_of/today 解析时间范围的纯函数，近 N 天为起始日期到今天的闭区间，事件只做一次统计时区落日转换。summary/list_events/SF04 共用解析结果，不各自取 now。
2. 写 `preset_days_excludes_future_dates`：固定上海今天，覆盖前一日、起点、今天23:59:59、未来一天，以及 UTC 与统计时区跨日。支持 days=0 当前归一为1的既有约定；超大 days 导致日期减法超界返回可读错误，不能 expect panic。
3. 写 `one_query_uses_one_today_across_midnight`；前端快捷项/确认/标签每次操作只读取一次 today，再计算两端，避免同一操作跨午夜取到两个“今天”。保留当前已正确的 formatted-value/from-to 路径，不把自定义显式未来区间或全部时间限制为今天。
4. 与 UX04 合并日期测试文件：本任务管真实范围，UX04 管年份标签与控件布局，避免反复修改时回退任一修复。

**命令：** `cargo test --offline preset_days`；`cargo test --offline --test e2e_events_range`；`pnpm --dir frontend test -- src/components/DateRangeSelect.test.ts src/composables/timezone.test.ts`。

### SF06：日志初始化可失败，GUI 启动可继续

**文件：** `src/logging.rs`、`src-tauri/src/{lib,commands}.rs`、`frontend/src/{types.ts,App.vue}`；新增 `tests/logging_startup.rs`，按新增启动诊断接口扩展 `frontend/src/App.test.ts`。

1. 使用 RollingFileAppender::builder().build 返回 Result，去掉可恢复初始化路径上的 expect/init panic。显式 `try_init` 处理 subscriber 已存在或初始化失败；目录无法解析、创建或写入都进入可观测降级路径。
2. 文件失败时尝试 stderr subscriber，并用简短诊断说明文件日志不可用；将初始化状态存入 Tauri managed state，由 App 挂载后通过 `startup_diagnostics` 等价只读接口取一次并显示非阻断通知，避免窗口监听建立前发送事件导致丢消息。提示失败不再次依赖同一个文件 logger，不递归报错，也不阻塞窗口创建等待用户确认。
3. 保留成功路径 WorkerGuard 生命周期；失败时不返回假 guard。全局 subscriber 测试放隔离子进程，或拆纯 writer 构建测试，避免多测试线程抢全局 subscriber。
4. 临时目录中让 logs 路径成为普通文件，稳定触发初始化失败；验证进程不 panic 且继续执行启动后标记。另测有效目录与重复初始化。子进程标记不等于原生窗口已验证；Tauri 窗口验收单独记录且使用隔离应用数据目录。

**测试/命令：** `unwritable_log_target_does_not_abort_startup`、`existing_subscriber_is_not_a_panic`、`successful_logger_keeps_guard`；`cargo test --offline --test logging_startup`；`cargo test --manifest-path src-tauri/Cargo.toml --offline`。

### SF07：价格列表与对照价保留三态及规则结构

**文件：** `src/pricing.rs` 的 PricingEntry/OpenRouterPrice/entries；`frontend/src/types.ts`、`frontend/src/lib/tieredPrice.ts`、`frontend/src/views/Settings.vue`；新增 `tests/pricing_view_contract.rs`、`frontend/src/lib/tieredPrice.test.ts`，更新设置测试。

1. 基础价与 OpenRouter 对照统一保留 `RateSpec`，采用现有 serde 线格式 `number | "same_as_input" | null`，前端用对应联合类型。删除 unwrap_or(0) 和压平 SameAsInput 的导出路径；没有对应模型与有模型但某项未知是两个状态。
2. 同步基础价、上下文分段、schedule、period、schedule 内 segments 的 TS/DTO 与展示。缺少覆盖值代表继承上层，不直接标成免费；完整 PricePlan 作为权威结构，别让设置页再次用另一套数字-only view 推导。
3. 主表明确显示“同输入价（随分段/时间规则）”，若展示解析后的具体金额，必须由后端在明确上下文下提供数值与 rate_kind；前端不自己计算候选最大值，也不把基础输入价冒充所有条件的最终输入价。
4. 明确 incomplete 的作用域：无请求上下文时只能描述基础费率可解析性/规则是否存在缺项，不能等同“任意请求都会完整”。SameAsInput 的基础输入价未知时仍要提示依赖未定；所有请求是否完整仍以实际 breakdown 为准。必要时以明确命名的 base_incomplete 替代含糊字段并迁移调用方。
5. Rust 序列化 fixture → TS 展示契约覆盖 Unknown/Fixed(0)/Fixed(正数)/SameAsInput、输入未知、分段输入变价、时间档输入变价。测试对应 estimate 的 rate_kind/unit_price 与说明一致，**保留原估算结果不变**。UX07 的 money formatter 只接收已解析数字，不能给 "same_as_input" 做 Number/toFixed。

**测试/命令：** `pricing_view_preserves_unknown_zero_and_same_as_input`、`comparison_price_does_not_turn_missing_into_free`、`same_as_input_tracks_resolved_tier_input_in_breakdown`、`nested_schedule_views_keep_rate_specs`；`cargo test --offline --test pricing_view_contract`；`pnpm --dir frontend test -- src/lib/tieredPrice.test.ts src/views/Settings.test.ts`。

### SF08：异常 token 使用受检算术，溢出不伪装正常数据

**文件：** `src/source/{codex,claude}.rs`、`src/model.rs`、`src/aggregate.rs`、`src/report.rs`，按受影响调用点修改 `src/pricing.rs`/`src/cache.rs`；新增 `tests/token_overflow_contract.rs`，扩展 `tests/token_bucket_contract.rs`。

1. Codex 零值判断改四字段分别为0，不做加法；守恒用 checked_add，cached+cache_write 受检后与 input 比较，确认子集再减。失败计 bad_lines，继续后续行；不要用 saturating_add 让异常组合看似合法。
2. source→model 公共校验保证单事件四桶总数/prompt 可表示，两适配器与从旧缓存恢复的事件都经过相同边界；旧缓存不合法事件不得绕过新检查。按解析语义变化递增解析/缓存版本并重建派生数据，不删除源日志。
3. model 桶累计、分组请求数/未知桶/总计改用 checked；一次更新先在临时值上全部成功再提交，避免某桶已改而后面失败。多个合法事件累计溢出时返回明确查询错误，保留 UI 旧数据/重试，不任意跳过“最后一个”制造不确定统计。
4. 检查金额累计为非有限数的相关出口并维持现有请求级溢出诊断，不能让聚合结果序列化成正常空值。已有定价溢出修复保留；变更 Result 签名时完整更新调用者而非 unwrap。范围仅限事件/桶/相关聚合算术，不做无关全仓重构。
5. fixture 包含单字段接近 u64::MAX 的合法 JSON、多字段溢出、cached 子集溢出、后续正常行、两个单独合法但累计溢出的事件；热缓存/冷扫描得到一致诊断。正常黄金 fixture 数字保持不变。

**测试/命令：** `overflowing_codex_line_is_skipped_and_next_line_survives`、`aggregate_overflow_returns_error_without_partial_result`、`legacy_cached_event_cannot_bypass_validation`；`cargo test --offline --test token_overflow_contract`；`cargo test --release --offline --test token_overflow_contract`；`cargo test --offline --test token_bucket_contract --test golden_reconciliation`。release 命令只运行合成测试，绝不加 `--ignored`。

### SF09：拒绝无法安全处理的跨适配器来源重叠

**文件：** `src/settings.rs`、`src/report.rs`、`src-tauri/src/commands.rs`、`tests/source_overlap.rs`、`tests/cache_source_identity.rs`；必要时在 `frontend/src/views/Settings.vue` 展示结构化目录冲突。

1. 本轮选最小确定方案：对所有**启用**来源解析实际目录（包括默认目录），规范化后检查相同/嵌套冲突；保存时在 SF02 锁内拒绝，采集时也校验手改/旧配置。禁用来源不阻止用户恢复配置。不要基于字符串公共前缀误判 `foo` 与 `foobar`；存在的 symlink/junction 依据解析后的实际路径，IO 检查失败可见。
2. 在 agent 筛选和缓存写入前检查有效来源配置，让“全部/仅某来源”对同一无效启用配置一致报错；错误列出冲突目录、影响来源及修改/停用其一的恢复方法。source_status/设置读取仍可返回配置与诊断，让用户能修正，不因拒绝采集导致设置页也进不去。
3. 发现阶段若仍遇到同一规范化文件被不同 adapter 认领，明确返回归属冲突；移除“先发现者得”的跨 adapter 默认。不要以“第一个 parser 返回零事件就换另一个”作为格式检测，也不新造启发式格式路由。
4. 将旧 `test_source_overlap_same_dir_single_count` 等测试改为明确拒绝预期，并增加共享 Codex rollout、嵌套目录、默认目录与显式覆盖冲突、大小写/规范化别名、禁用来源恢复。失败前无成功缓存写入；分离目录冷/热缓存统计不受影响。
5. 这是对旧重叠计划的局部覆盖：保留“不能重复计数/不能污染另一 adapter 缓存”的不变量，替换“按扫描顺序选择 parser”。不改变独立来源正常解析和去重规则。

**测试/命令：** `ambiguous_overlap_is_rejected_before_adapter_selection`、`overlap_validation_uses_effective_default_paths`、`disabled_source_allows_overlap_recovery`；`cargo test --offline --test source_overlap --test cache_source_identity`；`cargo test --manifest-path src-tauri/Cargo.toml --offline source_config`。

### SF10：价格可用性、列表和估算使用同一有效候选集合

**文件：** `src/pricing.rs`、`src/report.rs`、`src-tauri/src/commands.rs`、`frontend/src/components/PricingStatusBanner.vue`；`tests/pricing_source_policy.rs`、`tests/pricing_index_restart.rs` 与横幅测试。

1. 复用 SF03 的加载结果：来源数据→归一/合法性校验/显式 model_policy→有效候选及诊断→估算、列表、status、index。pricing_status 不再自己读取原始 entries.len 当可用性；索引恢复须产生相同集合和状态，不能成为校验旁路。
2. 区分原始条目数、校验通过候选数和有可解析费率路径的候选数。source available/hasAnyPricing 基于后者（明确0也算有价格；全部 Unknown 或未解析 SameAsInput 不算）。部分费率可用仍可参与现有部分估算，并显式保留未知；是否对某请求完整只能由 estimate 判断，不能从 source available 推导。
3. “存在可解析费率路径”的判断复用后端费率继承/解析语义，覆盖基础、分段、schedule/period 及 SameAsInput；不在 status 复制第二套价算逻辑。`needsSync` 保留主源未有效可用时提示联网的产品策略，即使外置/补充源有部分可用价格也说明当前状态。
4. 原始计数可保留作技术诊断，但字段名/UI 文案区分已读取与有效可用。全部被拒绝、全部 Unknown、单个明确0、部分可用、索引命中/重启、外置读取失败与恢复均测；warning 传到设置与横幅可展开，不把“有 N 条原始记录”显示成“已有有效定价”。

**测试/命令：** `all_rejected_candidates_do_not_mark_source_available`、`all_unknown_candidates_are_not_pricing_available`、`explicit_zero_is_valid_available_pricing`、`source_status_matches_validated_entries_after_restart`；`cargo test --offline --test pricing_source_policy --test pricing_index_restart`；`pnpm --dir frontend test -- src/components/PricingStatusBanner.test.ts`。与 UX06 合并状态/重试 UI，不创建另一套横幅。

### SF11：统一当前文档、估价说明与已接受限制

**文件：** `README.md`、`CLAUDE.md` 的当前技术规则、`docs/stats-semantics.md`、`docs/plans/README.md`；必要时 `frontend/src/lib/{costBreakdown,statsView}.ts` 的说明及对应测试；本计划。

1. README 清除内置价格兜底、两套矛盾层级、产品 CLI 参数、固定缩托盘等过时当前态描述。保留开发用 Tauri CLI/Cargo 命令，不把开发命令误删成“产品 CLI”；当前关闭行为说明三态及记忆设置。
2. 清楚写明三个来源及离线快照角色、末段完整匹配优先再前缀、现有变体保护、完整候选优先后取最高费用；来源优先/tie-break 以当前实现列清，不把外置低价承诺为全局强制覆盖。`model_policy` 是显式补充缺失语义，不等于用户账单覆盖模式。
3. 区分保守候选估算、尚未实现的全局用户强制覆盖概念和实际供应商账单；已排除不完整候选时，现值不是所有未知渠道的严格上界。费用公式始终说明“计价候选渠道，不是实际路由”。不借文档修复推翻既定峰谷/分段选择规则。
4. Codex 启发式去重继续保留，在用户统计口径说明中写清“同会话同模型同桶数值的不同请求可能合并”；没有多版本证据不改用原生 ID/累计差分。SF04 快照行位置解决分页稳定性，不证明 API 请求身份精确。
5. 更新权威口径中旧 `(ts,rid)` 游标说明、范围冻结、价格三态/有效可用状态、重叠拒绝、异常 token 容错及设置事务/外部编辑限制。以 SF 实现后的真实协议写终态，历史归档仅加覆盖链接，不把历史验收数字改成新结论。

**验证：** `rg -n '内置|最高优先|--from|--to|--tz|--refresh|游标|覆盖|去重' README.md CLAUDE.md docs/stats-semantics.md` 后逐条人工判定上下文；“无内置”、开发命令、历史说明不要求零匹配。若改 UI 说明运行 `pnpm --dir frontend test -- src/lib/costBreakdown.test.ts src/lib/statsView.test.ts`。核对每项产品承诺能映射到本轮具名测试，不写仅靠文档成立的新能力。

### UX00：建立真实组件量测入口

**文件：** 新增 `frontend/scripts/check-ui-contracts.mjs`、`frontend/scripts/fixtures/ui-contracts.mjs`；保留 `frontend/scripts/check-app-scroll.mjs` 原有职责。

1. Playwright 通过 URL 加载实际前端；`addInitScript` 注入合成 `__TAURI_INTERNALS__`，所有读取/写入/事件接口显式 mock。提供正常、空数据、未知价、部分计价、长模型/路径、多类别、各设置读取失败 fixture；未知命令直接失败。
   合并 SF 后同时 mock query_id/generation/price revision、过期分页、价格三态和启动诊断；复用 SF01 真实 ECharts 安全入口的 fixture/浏览器解析方式，不维护两份相互矛盾的 mock 协议。
2. 支持 `--url`、`--phase baseline|verify`、`--output`。baseline 只记录当前违例，verify 按本计划断言；非预期浏览器/JS/IPC 错误、未处理拒绝、未知 command、资源加载错误在两个模式均以非零退出。故障 fixture 主动返回的 IPC reject 属预期输入，应验证提示与恢复行为，不将其直接判为脚本失败。使用独立 browser context，不加载用户 profile，不访问外部服务。
3. 截图与 JSON 记录 commit、浏览器版本、viewport、deviceScaleFactor、主题、fixture、可访问性快照与量测值；等待字体就绪和 CSS 过渡完成，禁止把动画缩放中间值当最终尺寸。
4. 使用 DOM Range/scrollWidth 判断实际文本是否溢出，不能仅量被裁剪后的 span。材质读真实承载背景节点，包含外框 padding/border，不能只断言 themeOverrides 或 CSS 字符串。

**验收项名称：** `real_app_fixture_boots_without_ipc_leak`、`baseline_records_contract_violations`。UX01–UX09 的浏览器验收加入同一脚本，每项具名输出失败原因；不得注入“修正后 CSS”伪造通过。

**命令：** 独立终端运行 `pnpm --dir frontend exec vite --host 127.0.0.1 --port 1437`；随后运行 `node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 --phase baseline --output qa-artifacts/ui-ux-remediation/before`。完成后只停止本任务启动的进程。

### UX01：统一浮层材质、表格排版与语义色映射

**文件：** 修改 `frontend/src/styles/{tokens.css,naiveTheme.ts,chartTheme.ts}`、`frontend/src/components/{EventTable,UsageTable,DateRangeSelect,TrendChart}.vue`、`frontend/src/views/Settings.vue`；新增 `frontend/src/styles/themeContract.test.ts`。

1. 写 `adapters_match_semantic_tokens_in_both_themes`：按显式字段映射比较 CSS 的浅/深 token 与 Naive/ECharts 消费值，规范化 hex/rgba；区分 surface-solid、elevated、separator 等语义。解析 scope 后再比较，不用全文件首次 regex 命中代替主题解析。
2. 写 `table_typography_is_scoped_to_datatable`：DataTable small 正文 13px/1.45，表头 12px/500/1.4/secondary，正文 text；其他 small 控件字号与高度保持原契约。**不改 common.fontSizeSmall。** ECharts 网格线映射 separator，软状态背景改为两主题 token。
3. 浮层以一个真实外壳消费 85% elevated + 16px blur + 12px 圆角及规范阴影；为 Tooltip、Popover、日期面板、Select 菜单与 ECharts HTML tooltip 确认各自实际节点和 adapter API。无 backdrop-filter 时使用 95% 配方。处理箭头背景，避免内外两层玻璃或实色外层遮蔽。
   ECharts 保持 SF01 的 DOM/textContent 输出；不能为了统一玻璃样式改回原始标签拼 HTML。CSP 样式白名单与 UI 配方共同验证。
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
   所有 setOption/主题重建路径继续使用 SF01 安全 formatter；回归包括恶意标签，不能只检查初始化次数。

**命令：** `pnpm --dir frontend test -- src/components/TrendChart.test.ts src/views/Dashboard.test.ts src/lib/chartData.test.ts`。

### UX06：设置首载与定价横幅错误恢复

**文件：** 修改 `frontend/src/views/Settings.vue`、`frontend/src/components/PricingStatusBanner.vue`；修改同名 `.test.ts`。

1. 写 `settings_initial_failures_are_independent_and_retryable`：分别拒绝 source_status/cache_stats/pricing_entries/settings_get/autostart_status；其他成功区块仍显示，错误区块显示原因与局部重试，无 unhandled rejection。
2. 一次读取 settings_get 初始化依赖它的配置区块；与其他独立读取并发，但各自处理失败，不用一个共享 loading/error 覆盖全部结果。没读到配置前禁用相应写入控件并说明原因，不能把“每次询问”等默认值当读取成功。
3. 写 `retry_preserves_dirty_source_drafts`、`late_settings_response_does_not_overwrite_saved_value`：重试/刷新只更新未编辑草稿；为相关请求加代次/卸载守卫，未返回的旧请求不覆盖新编辑或保存值。保持现有后端接口，不为 UI 状态引入数据库改造。
4. syncPricing 后的 pricing_entries 刷新失败单独归类为“价格列表读取失败”，保留旧数据、保证 finally 退出 busy 并通知横幅。来源保存成功后 source_status 读取失败也应区分“已保存，状态刷新失败”，避免误导用户重复保存。
5. 写 `sync_and_status_failures_remain_visible`、`status_retry_does_not_sync_network`、`partial_sync_preserves_usable_pricing`、`retry_ignores_duplicate_activation`。横幅同步重试与状态读取重试各有 pending/防重复；两类错误均可读可操作，可合并一条通知并展开详情。
6. 状态未知时只说同步/读取失败；只有返回证据支持才说部分可用。成功清理对应错误，旧请求不得覆盖新结果；初次无价的联网提示保留。
   本任务接入 SF02 事务 command、SF10 的有效可用状态；前端 pending 不替代后端互斥，原始快照条目数不能作为恢复成功依据。

**命令：** `pnpm --dir frontend test -- src/views/Settings.test.ts src/components/PricingStatusBanner.test.ts`。浏览器 fixture 用按钮真实触发失败→重试→恢复，不仅调用组件方法。

### UX07：金额与 token 显示词统一

**文件：** 新增 `frontend/src/lib/{formatMoney.ts,formatMoney.test.ts,tokenDisplay.ts}`；修改 `frontend/src/types.ts`、`frontend/src/lib/{costBreakdown.ts,tieredPrice.ts,chartData.ts}`、`frontend/src/styles/chartTheme.ts`、相关四类 token 展示组件及 `Settings.vue`；保留现有业务字段。

1. 写 `request_amount_matches_breakdown_result`（0.0126）、`tiny_nonzero_is_never_displayed_as_zero`（1e-8）、`zero_and_unknown_are_distinct`、`unit_price_keeps_its_precision_and_unit`。建立单一格式化入口，显式区分 summary/request/unit 场景，不把单价与费用视为相同单位。
   SF07 的 `RateSpec` 显示分支先区分 Unknown/SameAsInput/Fixed，再仅将 Fixed 或后端已解析金额传入数字 formatter；不得把联合类型再次压回 number/null。
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

**文件：** 完成 `frontend/scripts/check-ui-contracts.mjs`；新增 `qa-artifacts/ui-ux-remediation/` 脱敏截图/JSON；修改 `docs/plans/README.md`、`docs/plans/active/2026-10-06-design-system-visual-qa.md`、本计划；原始审查保留历史并链接终态。

1. 执行以下自动矩阵：浅/深 × 1280×820/980×620；正常/空/未知价/部分失败；设置重试、键盘 Tab/方向键/Enter/Space/Escape、长日期/模型/路径、多类别图表、浮层四边、reduced-motion。具名断言全部通过后截图，不用截图代替交互断言。
2. 在**实际 App**长页滚动时检查内部 scroller 的 scrollTop、导航顶边与通知条可见性；保留既有 `check-app-scroll.mjs` 结构回归。当前合成 CSS 长页已证明修复，不等于真实产品视觉 QA 全部完成。
3. 用真实 Windows/Tauri 运行浅/深主题的100%/125%/150%系统缩放、冷启动和原生窗口背景验收；记录系统缩放、窗口逻辑/物理尺寸和截图。浏览器 deviceScaleFactor 只改变栅格密度，不能替代系统缩放通过记录。环境无法覆盖的组合留“待验”。
4. 对合成明细核对已知/部分/未知显示，单价、候选渠道、档位等字段没有因格式化或换肤丢失；只检查展示与 DTO 一致，不新增第二套定价算法。
5. 每项记录修改文件、测试名称、红→绿证据、截图、剩余限制与提交；完成后按仓库约定归档并修正链接。旧 D5 安装验收不与本文的设计债 D5 混同，未实际执行不得代签。
6. 合并验收增加 SF01–SF11 的真实 tooltip/CSP、设置确定性交错、价格临时读取失败后恢复、活跃日志分页、冻结价格/时间、日志启动降级、三态 DTO、数值溢出、重叠拒绝及有效可用状态。前端新增 query/启动诊断等 command 必须在浏览器 fixtures 中显式覆盖，Rust 临时路径必须包含 cache_dir/pricing_index/settings/logging；全量门禁绿不替代这些具名边界测试。

**自动命令：**

```powershell
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
# 独立终端启动生产预览；完成后只停止自己启动的进程
pnpm --dir frontend exec vite preview --host 127.0.0.1 --port 1437
# 在另一终端执行
node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 --phase verify --output qa-artifacts/ui-ux-remediation/after
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

- [ ] SF01–SF11 全部边界闭合。**复核重新打开：** SF04 跨启动身份、旧游标、过期重试与保留内存预算由 RC01/RC02/RC05 修复；SF01 生产 CSP 原生证据由 RC10/RC11 补齐。原 HTML 输出安全及设置事务实现保留。
- [ ] 同查询汇总/明细/分页的事件、时间、价格版本一致；旧/过期游标显式拒绝且可恢复。**现有 TTL 拒绝不证明跨启动隔离，恢复旧视图后的分页仍需 RC01/RC02。**
- [x] 外置读取失败可见且可恢复，三态展示和有效候选状态一致；未知、免费与沿用输入价可区分。
- [x] 辅助日志失败不终止启动，异常 token 不 panic/回绕，跨适配器重叠不按扫描先后静默认领。
- [x] README/CLAUDE/统计口径与终态一致，Codex 去重限制和最高候选估算范围明确；未引入强制价格覆盖或新技术栈。
- [ ] UX00–UX08 各任务具名边界真正覆盖并通过。**复核重新打开：** UX03/UX06/UX07 的实现与测试缺口由 RC03/RC04/RC06/RC07/RC09 承接；已有材质/字号等量测记录保留，不推导未覆盖交互通过。
- [x] 图表无关更新不重建，新数据照常更新；日期跨年显示正确，统计参数不变。
- [ ] 费用/单价名称、描述及键盘行为正确，单价保留有效精度。**名称含金额已验证，但列名称、Enter/Space、外部关闭和 unit 精度仍需 RC06/RC07。**
- [ ] 设置失败可局部重试、不丢草稿；同步与状态读取错误均可恢复。**RC03/RC04 待修统一读取、真正晚到响应保护和可操作同步重试。**
- [ ] UX09 首次绘制与原生启动证据分别记录；没有把未复现问题写成确定故障或把未测试项写成通过。
      ——**部分**：浏览器首帧证据已记录（`prepaint_theme_matches_preference` 六种偏好/系统组合，
      dev 与生产 preview 均通过，且断言主模块尚未执行）；**原生 Tauri 冷启动/窗口背景未运行**，保持待验。
- [ ] UX10 自动矩阵通过；系统缩放/安装等无法覆盖项明确待验。必要真机项未验时只标"实现完成、验收未闭合"，不整体宣告完成。
      ——**部分**：既有自动矩阵通过（24 场景 0 违例 + 首帧 6 场景），但缺少失败恢复、业务状态与完整交互断言，RC09 待补；**100%/125%/150% 系统缩放、原生窗口背景、
      安装验收未运行**，保持待验。
- [x] 原 F06/F08、关窗失败恢复、日期接口、定价/统计既有回归保留；索引已链接复核计划。本文仍在 active，未归档；查询快照新增缺口按上方未完成项处理。

| 项目 | 状态 | 证据 |
| --- | --- | --- |
| 本次核实 | 已完成 | `dd6aaae` 代码对照、真实 App 合成 IPC 浅深主题量测、上文持久化证据 |
| 技术审查合并核实 | 已完成 | `3f036bb` 源码、ECharts HTML 输出分支、tracing-appender 初始化依赖核对；SF01–SF11 的运行时边界复现留实施阶段，不伪造已验 |
| 基线门禁 | 已通过（技术审查合并后重跑） | 根库/壳 fmt、clippy、test；前端 typecheck、format、177 个测试、build；未运行 ignored 真实性能测试；build 有现存 chunk 体积提示；不代表尚未实现的 SF 边界已修复 |
| SF01–SF11 实现 | 复核后部分重新打开 | `e4f0d49`…`648d0a3` 的已有测试全绿记录有效；SF04 的跨启动/恢复/预算边界未被覆盖，待 RC01/RC02/RC05；生产 CSP 另待原生验收 |
| UX00–UX09 实现 | 复核后部分重新打开 | UX00/01/02/04/05/09：`3597328`…`58afebd`；UX03/06/07：`e59e51b`、`6af5f47`、`9f86fd4`；UX08：`698a17f`。前端245项通过不能证明遗漏路径正确；UX03/UX06/UX07 待 RC03/RC04/RC06/RC07 |
| UX10 自动矩阵 | 部分覆盖，待补齐 | 既有24场景零违例+首帧6场景可保留；完整失败/恢复/键盘/边界等具名断言按 RC09 补齐；不据场景总数标整体完成 |
| 系统缩放/原生首帧 | 待验 | 未运行原生窗口或改变 Windows DPI；仅记录浏览器 deviceScaleFactor 之外的空白 |
| 产物位置 | 已调整 | 浏览器验收产物移至仓库根 `qa-artifacts/`（`.gitignore`，本地产物不入库）；脚本默认输出与文档引用已同步（`ea15249`） |
