# 开发计划

- **滚动状态以本文件为准，CLAUDE.md 不追写。**
- `active/` 进行中的计划；`archive/implemented/` 已完成归档（含完整目标 / 非目标 / 不变量 / 任务清单 / 验收记录）。
- 计划必须包含：目标 / 非目标 / 不变量 / 任务清单（每项对应代码位置、测试名、验证命令）/ 验收标准。没有测试名或验证命令的任务不得标记完成。
- 完成验收后把文件移入 `archive/implemented/`，并在文末总账登记完成时间与验收要点。

## 里程碑总览

项目当前形态：**Tauri 2 + Vue 3 桌面 GUI（唯一产品形态，CLI 已于 2026-10-04 移除）**，多 agent 用量统计，SQLite 缓存 + 外置价格表。

| 里程碑 | 交付 | 验收要点 | 归档 |
| --- | --- | --- | --- |
| M1 | Cargo 骨架 + Claude Code 统计闭环（source→aggregate→render 分层，去重保末条） | Python 独立复算逐组一致；与 cc-switch 对照 15 个共有日期中 10 天五项指标完全一致 | [归档](archive/implemented/2026-10-03-m1-claude-code-adapter.md) |
| M2 | Codex 适配器 + 多 agent 汇总 + 价格表扩至 43 条（最长前缀） | 验收期发现 Codex 同请求原样重发，修正口径后 cc 对照 38/39 日期全指标一致；M1 回归不变 | [归档](archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) |
| M3 | Tauri 2 + Vue 3 GUI（Naive UI、明暗双模式、托盘常驻、关窗缩托盘） | NSIS 安装包产出；CLI 与 GUI 共用 report 管线，回归逐项一致 | [归档](archive/implemented/2026-10-03-m3-tauri-vue-gui.md) |
| M4 | SQLite 缓存（指纹失效、故障降级）+ 价格外置 TOML + GUI 设置页 | 无缓存/命中/--refresh 三路径数字逐字段一致；更正 M2 归档事件数口径笔误（最终 20,935） | [归档](archive/implemented/2026-10-03-m4-cache-pricing-settings.md) |
| M5 | OpenRouter 价格同步 + 变体隔离（当时为四层合并含内置，**已被 2026-10-06 三层策略取代**） | 真实同步 466 条、价格与官方一致；unknown 大幅下降；NSIS 4.27 MiB | [归档](archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md) |
| M6 | UTC 存储不变量 + 时区解析链（--tz local/IANA，默认上海）+ JSON timezone 字段 | 本机==上海逐字段一致；UTC 日界生效；非法时区报错；NSIS 产物 | [归档](archive/implemented/2026-10-04-m6-timezone-resolution.md) |
| M7 | 逐请求明细（CLI events + GUI 行点击下钻）+ collect_all 共用路径 | 真实数据总额核对一致（3,741=3,741；日明细 token 求和一致） | [归档](archive/implemented/2026-10-04-m7-event-drilldown.md) |
| M8 | 桌面体验：单实例互斥、窗口状态记忆、开机自启（设置页开关） | NSIS 4.38 MiB；三项能力待用户安装冒烟 | [归档](archive/implemented/2026-10-04-m8-desktop-experience.md) |
| M9 | models.dev 主源 + OpenRouter 备份（当时为四层价格合并含内置，**已被 2026-10-06 三层策略取代**） | 双源真实同步 7,957+466 条；unknown 清零；doubao/hy3:free 转正 | [归档](archive/implemented/2026-10-04-m9-modelsdev-source.md) |
| M10 | 时间区间选择（GUI daterange 快捷项 + CLI --from/--to，按解析时区闭区间） | 区间覆盖全量/单日等价/端点含入/互斥报错全过 | [归档](archive/implemented/2026-10-04-m10-date-range.md) |
| M11 | 价格索引持久化（签名失效）+ 进程内缓存 + 24h 自动同步双源 + 设置页开关 | 索引 8,467 条命中后 0.86s；NSIS 4.77 MiB | [归档](archive/implemented/2026-10-04-m11-pricing-cache-autosync.md) |

## 当前状态

- **2026-10-09 跨工具项目路径统一（分支 `feat/project-path-unification`）：阶段 A 已实施（A01–A06）并推送；阶段 B 进行中（B01 证据核验完成，归属规则待产品确认）。** [跨工具项目路径统一与会话目录切换](active/2026-10-09-project-path-unification.md)。阶段 A 把项目身份改为 source 层统一的规范化绝对路径：Claude 取会话初始 cwd（无 cwd 时按 `~/.claude.json` 的 projects 正向编码唯一命中，冲突/缺失保持 slug），Codex 取当前上下文 cwd（A → B → A 保留、新会话不继承）；同一路径跨工具合并为一行，仅同名不合并。缓存解析版本 6 → 7（映射修订）→ 8（身份口径），前端快照 6 → 7，映射内容/状态变化即让磁盘缓存与采集复用键失效而旧查询会话仍冻结。B01 只读核验结论：Codex `turn_context.cwd` 是轮级工作目录，Claude 顶层 `cwd` 是行级状态快照且 sidechain 常态带不同目录——[核验记录](../research/2026-10-09-session-cwd-semantics.md)；四个归属实例待确认后再做 B02/B03。

- **2026-10-09 新计划（阶段 A 之前的状态记录）：** 同一计划原为「待实施」；阶段 A 见上一行，阶段 B 仍待产品确认。

- **2026-10-09 后续补齐（优先于下方历史状态）：** 复核 `9db0bbb` 后由 `9222aff` 补齐弹窗/托盘退出最终保存与保存串行化；`6332f4f` 补原生验收入口与驱动。CSP 哨兵、关闭保存、已记忆最小化失败恢复、当前显示器未强制缩放的 150% 组合验收，以及真实空闲 630.684 秒后的查询过期/新会话/下一页恢复均通过。浏览器矩阵复跑 60 场景 / 17 契约 / 243 断言零失败。证据、失败尝试与环境边界见 [补齐验收记录](active/2026-10-09-ap08-completion-qa.md)。Windows 125% / 跨显示器、系统深色首帧与 D5 发布验收仍待验；CSP 图片噪声按每轮实际计数，含重载轮不再固定写成 2 条。

- **2026-10-09 全计划终态复核与遗留修复：AP01–AP07 已实现，AP08 自动化部分完成、原生部分待验（基线 `5ec826c` → `d68bbb2`）。** [全计划终态复核与遗留修复计划](active/2026-10-09-all-plans-final-recheck.md) 覆盖原有 31 份计划/QA/索引文档，其 AP01–AP07 已逐项红→绿并独立提交：默认启用来源纳入保存校验（`6a445d6`）、坏配置不再回退默认来源（`ed10ed9`）、重建缓存按生效来源配置（`f538d35`）、设置异步失败与卸载守卫及关闭动作串行化（`a2a4d11`）、定价横幅区分"主源待同步"与"完全无价"（`a2a4d11`）、来源检测失败可见可重试（`c31bba2`）、已记忆关窗失败上报且磁盘 IO 移出事件回调（`d68bbb2`）。AP08 自动化：浏览器矩阵从 28 场景 / 12 契约 / 173 断言扩到 **60 场景 / 17 条必需契约 / 243 条断言，零失败**（Chromium 148.0.7778.96，提交 `d68bbb2`，产物 `qa-artifacts/all-plans-final-recheck/after/measurements.json`，gitignore），新增 5 条反例契约（来源保存被拒按行归属、关闭动作写入失败回退、晚到失败读取不降级已保存值、来源检测失败可恢复且不丢汇总、定价横幅按 DTO 区分四态）。**仍未验（不写成通过）**：AP08 的原生/系统部分——query 会话过期的原生轮（需 >600 s 真实空闲）、Windows 每监视器 125%/150% 缩放、系统深色下"跟随系统"首帧、CSP 脚本哨兵（缺仅验收构建可用的受控入口）——与 D5 安装/升级/卸载、自启注册、休眠及真机隐私时序一并后延，唯一活跃入口是 [D5 验收清单](d5-acceptance-checklist.md) 与 [原生验收记录](active/2026-10-08-native-recheck-qa.md)。生产 CSP 下 naive-ui 的 data-URI 预热图固定产生 2 条 `img-src` 违规，保持明确计数与原因、不放宽 CSP，也不声称绝对零违规。

- **2026-10-08 复核修复计划：RC01–RC10 已执行，RC11 部分闭合（优先于下方历史声明）**：[复核遗留缺陷与验收补齐计划](active/2026-10-08-recheck-remediation.md) 的 RC01–RC10 已逐项红→绿并提交（`3d3ce2c`…`74d8f93`）；RC11 用 release+`acceptance` 原生实例取证 22 轮（含作废/重跑），[原生验收记录](active/2026-10-08-native-recheck-qa.md) §1–§6 大部分已验证。仍**待验且不写成通过**：会话过期的原生轮（隔离实例两次在 600 s 空闲窗口内被外部进程终止）、Windows 每监视器 125%/150% 缩放（本轮是 `--force-device-scale-factor` 注入的 WebView 设备缩放，实测 dpr 1/1.25/1.5）、系统深色下的“跟随系统”首帧（不改系统主题）、CSP 内联/外源脚本哨兵（无合法注入通道，调试器求值不算证据；已验的是实际响应头策略与全功能可用）。**口径纠正**：下文“24 场景=完整矩阵”“浅色截图”等说法不成立——浏览器矩阵现为 28 场景量测 + 12 条具名契约 173 条断言，截图存档为历史文字记录、原生首帧证据现位于 `qa-artifacts/native-recheck-2026-10-08/`（gitignore）。（**2026-10-09 更新**：矩阵已由 AP08 扩为 60 场景 / 17 条具名契约 / 243 条断言，见上方最新条目；此处 28/12/173 为当时口径，不再作为当前数字。）本轮另登记两处状态口径问题：仅外置价格可用时定价横幅仍称“尚未获取定价/费用仅能显示为未知”（与同屏费用矛盾），以及 naive-ui 的 data-URI 预热图在生产 CSP 下固定产生 2 条 `img-src` 违规（未为此放宽 CSP）。D5 安装/升级/卸载继续后延；本计划保持 active。

> 下方保留历史执行记录；其中“已全部实现”“仅剩 D5”等为当时声明，已由上方复核修正，不作为当前完成度判断。

- **2026-10-08 安全/数据一致性与 UI/UX 修复计划实现完毕（基线 `9f86fd4` → `698a17f`）**：[安全、数据一致性与 UI/UX 修复计划](active/2026-10-07-ui-ux-review-remediation.md) 的 **SF01–SF11 与 UX00–UX09 已全部实现**（SF 侧：tooltip 安全输出 + 最小生产 CSP、设置读改写事务、价格读取失败显式降级、查询快照冻结事件/时间/价格、预设日期双边界、日志初始化可失败、价格三态、受检算术、重叠目录拒绝、有效候选一致、文档口径统一；UX 侧：真实组件量测入口、浮层材质/表格排版/语义色映射、分段控件语义名称与真实焦点、日期跨年标签、图表实例生命周期、设置首载错误恢复、金额与 token 单一入口、浮层可访问性、设置页层次与目录信息、首帧主题）。自动矩阵 `check-ui-contracts.mjs --phase verify` 24 场景 0 契约违例 + `prepaint_theme_matches_preference` 6 场景；前端 245 测试、typecheck、format、build 与 Rust 双 crate 门禁全绿。**仍待验**：100%/125%/150% 系统缩放、原生 Tauri 冷启动与窗口背景、D5 安装验收——不伪造通过。验收产物移至仓库根 `qa-artifacts/`（已 gitignore，本地产物不入库）。

- **2026-10-07 技术审查合并入同一修复计划（基线 `3f036bb`）**：[安全、数据一致性与 UI/UX 修复计划](active/2026-10-07-ui-ux-review-remediation.md) 新增 SF01–SF11，先处理 tooltip HTML 注入与设置丢更新，再修外置价失败缓存、冻结查询/分页、预设日期上界、日志启动降级、价格三态、数值溢出、重叠目录与有效价格状态，最后同步当前文档。保留原 UX00–UX10 并明确依赖；不推翻最高候选估算/Codex 去重，不重写技术栈。新增问题已核对源码/依赖，运行时故障复现按任务执行；**本次仍仅修改计划及索引，产品修复待执行**。

- **2026-10-07 UI/UX 审查已核实，新增待执行计划（基线 `dd6aaae`）**：[UI/UX 审查核实与修复实施计划](active/2026-10-07-ui-ux-review-remediation.md) 逐项处理 U01–U22 / D1–D5，并纠正下述旧审查中过时或扩大化的结论。真实 App 合成数据量测确认浮层实色、费用浮层外宽 512px、表格 14px/合计 400、筛选栏 36/32/28px 混排及两个日期快捷项裁字。F06/F08 已修复，仅回归；启动闪烁/系统缩放保留待验。**本次仅核实并写计划，未实施 UI 修复**；完整基线门禁通过（前端 177 个测试）。

- **2026-10-07 修复后复核遗留计划执行完毕（基线 `85f3c92`，优先于以下完成记录）**：八项复核遗漏已逐项修复——真实 v3 缓存迁移（5821d74）、未命中空时间规则跳过（001dee3）、非法候选拒绝+索引 v7（d543c6d）、溢出防护经明细解释（1cefac7）、视图快照共享批次 v5（c21cb03）、主滚动容器约束+真实浏览器吸顶验证（a1c8e90）、真实 tooltip 浮层（fcba1cf）、hide 失败透传（a44ac5a），索引冷启动回归迁入隔离子进程（7fc1e12）。逐任务红→绿证据见 [修复后复核遗留计划](active/2026-10-07-post-remediation-recheck-fixes.md) 执行记录；终态全量门禁（根库/壳 fmt+clippy+test、前端 typecheck/format/test 177/build）全绿。**下述 UI/UX 审查中"既有 F06（导航吸顶失效）"已由 a1c8e90 修复**（其余清单项另行处理）。**仍待验**：D5 安装验收、100%/150% 缩放——继续后延，不伪造通过。
- **2026-10-07 UI/UX 审查（基线 `1cefac7`，只列问题不改代码）**：按 `DESIGN.md` 第二版逐条复核前端，新增 [UI/UX 审查问题清单](../reviews/2026-10-07-ui-ux-review.md)——2 项新增 Blocker（费用列可访问名吞掉金额、主题控件用字符图标且 radio 无可访问名称）+ 10 项 Major + 10 项 Minor + 5 项设计债；既有 **F06**（导航吸顶失效）经计算样式复核确认仍未修复，视觉 QA §2.1 的通过勾选与实现冲突。根因集中在"三份色值手工同步且无一致性校验"和"契约值未落到 Naive/ECharts 真实渲染值"。
- **2026-10-07 全计划复审更新（优先于以下历史状态）**：已有实现完整门禁通过，但定向复现确认计价、日期和缓存等仍有缺陷，不能再按“仅剩 D5”判断完成度。逐份计划覆盖关系与证据见 [审核意见](../reviews/2026-10-07-all-plans-audit.md)。
- **2026-10-07 审核修复完成（R01–R10）**：[独立修复计划](active/2026-10-07-all-plans-audit-remediation.md) 已按序执行完毕——R01 OR 缓存价+索引 v6（fcf1780）、R02 时间档完整优先（6e33596）、R06 非法单价拒绝（57de3c1）、R05 缓存身份 schema v4（b8fc81a）、R03 日期控件契约（48fa3fb）、R04 快照查询身份 v4（2006f4b）、R07 关闭失败可重试（8d1ea8f）、R08 采集诊断通知（4c0ae8b）、R09/R10 键盘 tooltip+游标行身份（244d6cc）、导航滚动上下文修正（2fedca7）。每个任务红→绿记录见该计划执行记录表；最终全量门禁（根库/壳 fmt+clippy+test、前端四项）全绿。**仍待验**：D5 安装验收、100%/150% 缩放、QA 截图存档——不伪造通过。
- **执行中计划**：[产品与技术评估及改进路线](active/2026-10-05-product-review-and-roadmap.md)（2026-10-05，分支 `docs/product-review-plan`）。**四个阶段的代码与自动化部分全部完成（A/B/C/D1–D4）**：P0 修复、门禁与 CI、统计口径 `docs/stats-semantics.md`、部分计价、缓存正确性、黄金对账、日期控件与快照查询身份、来源目录配置、项目身份、费用可追溯、采集单飞、游标分页、原子快照与离线安全——详见计划文末执行记录。**剩余仅 D5 真机发布验收**：[签字式清单](d5-acceptance-checklist.md) 已就绪、安装包构建已验证（4.82 MiB），逐项验收待用户执行。
- GUI 冒烟进行中（用户 dev 模式实测反馈；会话内已修复：主线程阻塞、转圈居中、筛选两行、agent 图标分段控件、暗色图标隐形、分段控件宽度、图表全条目显示等，均独立 fix 提交）。
- **启动慢根因修复（2026-10-05）**：测试套件此前把真实 `~/.tokenscope/cache.db` purge 成 fixture（cache_dir/pricing_index 未密闭注入），GUI 每次启动被迫全量冷扫描 1.2 GB 日志。已修复：测试全部注入临时目录（两轮哈希校验密闭）、`collect_all` 分阶段耗时落日志、新增 `#[ignore]` 真实数据冷/热计时工具（release 实测冷 5.5 s / 热 0.12 s，数字一致）。
- 待办尾巴：M3/M4 的 GUI 交互冒烟留用户安装确认（数据正确性已由 CLI/测试覆盖）。

## 后续候选方向（未立项；择项后在 `active/` 成文，经确认再动工）

> **排期决策（用户 2026-10-04）**：当前专注 **Claude Code 与 Codex** 两个适配器；新增 agent 适配器（下表 2/3）暂缓排期，待安装使用或拿到样例日志、且用户明确排期后再立项。

1. ~~价格在线同步~~ → ✅ 已完成（M5 OpenRouter 价格同步，466 条实测）。
2. **Gemini CLI 适配器**：`~/.gemini` 用量日志。**暂缓前提**：本机未安装（无真实日志可实测验收）；待安装使用或拿到样例日志后立项。
3. **OpenCode 适配器**：同上，暂缓前提同 Gemini（本机无数据源）。
4. **逐请求明细视图**：按会话/日期下钻请求级列表，数据已在缓存 `events` 表，主要是 GUI 工作。
5. ~~桌面体验补全~~ → ✅ 已完成（M8：单实例互斥、窗口状态记忆、开机自启）；剩余自动更新单独立项。
6. **存储引擎换 DuckDB**（已评估，暂缓——2026-10-05）：当前聚合全部在内存完成，SQLite 行式存储够用（价格索引命中后真实汇总约 0.9s），DuckDB 的收益在分析型扫描与聚合下推，而 bundled C++ 依赖会明显加重构建。**立项触发条件**：事件量达百万行级，或需要把 GROUP BY 聚合下推进 SQL；交接面收敛在 `src/cache.rs` 单文件，届时替换成本可控。

## 总账

| 计划 | 状态 | 备注 |
| --- | --- | --- |
| [active/2026-10-09-project-path-unification.md](active/2026-10-09-project-path-unification.md) | 阶段 A 完成（A01–A06）；阶段 B 进行中（B01 核验完成，规则待确认） | 跨工具项目路径统一：source 层统一身份、Claude 会话初始 cwd、项目映射正向解析与缓存失效、Codex 归一化并保留上下文切换；B01 只读核验已产出，四个归属实例待产品确认后再做 B02/B03 |
| [active/2026-10-09-first-launch-privacy-consent.md](active/2026-10-09-first-launch-privacy-consent.md) | 计划已入库，待确认实施 | 首次启动隐私同意：前后端闸门、先保存后解锁、拒绝退出及失败保护；仅计划，未改产品实现 |
| [active/2026-10-09-all-plans-final-recheck.md](active/2026-10-09-all-plans-final-recheck.md) | 实现与自动化完成；原生部分通过，系统项待验 | 全计划终态复核与遗留修复；AP07 退出漏口已补，AP08 分项状态见执行账；发布验收仍在 D5 |
| [active/2026-10-09-ap08-completion-qa.md](active/2026-10-09-ap08-completion-qa.md) | 补齐验收记录 | AP07 / AP08 后续原生证据、失败尝试、发布构建入口隔离与未验边界；不新增发布签字入口 |
| [active/2026-10-08-recheck-remediation.md](active/2026-10-08-recheck-remediation.md) | 执行中（RC01–RC10 完成，RC11 部分待验） | RC01–RC11：修复复核遗留、补自动化和隔离原生验收；原生会话过期/系统缩放/系统深色首帧/CSP 哨兵仍待验，故不归档 |
| [active/2026-10-07-ui-ux-review-remediation.md](active/2026-10-07-ui-ux-review-remediation.md) | 复核后重新打开 | SF04/UX03/UX06/UX07 存在缺陷，UX10 覆盖不足；后续由 RC01–RC11 承接 |
| [archive/implemented/2026-10-04-m11-pricing-cache-autosync.md](archive/implemented/2026-10-04-m11-pricing-cache-autosync.md) | ✅ 完成 2026-10-04 | M11：价格索引持久化 + 进程内缓存 + 24h 自动同步；索引 8,467 条命中后 0.86s |
| [archive/implemented/2026-10-04-m10-date-range.md](archive/implemented/2026-10-04-m10-date-range.md) | ✅ 完成 2026-10-04 | M10：时间区间选择（GUI daterange + CLI --from/--to）；真实数据闭区间/互斥/端点验收通过 |
| [archive/implemented/2026-10-04-m9-modelsdev-source.md](archive/implemented/2026-10-04-m9-modelsdev-source.md) | ✅ 完成 2026-10-04 | M9：models.dev 主源 + OpenRouter 备份（当时为四层合并含内置）；真实同步 7,957+466 条，unknown 清零 |
| [archive/implemented/2026-10-04-m8-desktop-experience.md](archive/implemented/2026-10-04-m8-desktop-experience.md) | ✅ 完成 2026-10-04 | M8：单实例互斥 + 窗口状态记忆 + 开机自启；时区选择迁入设置页 |
| [archive/implemented/2026-10-04-m7-event-drilldown.md](archive/implemented/2026-10-04-m7-event-drilldown.md) | ✅ 完成 2026-10-04 | M7：逐请求明细视图（CLI events + GUI 下钻）；真实数据总额核对一致 |
| [archive/implemented/2026-10-04-m6-timezone-resolution.md](archive/implemented/2026-10-04-m6-timezone-resolution.md) | ✅ 完成 2026-10-04 | M6：UTC 存储固化 + 时区解析链（--tz local/IANA，默认上海）；JSON 增 timezone 字段 |
| [archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md](archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md) | ✅ 完成 2026-10-04 | M5：OpenRouter 价格同步（主源）+ 本地三层合并；466 条实测，变体隔离修正误定价 |
| [archive/implemented/2026-10-03-m4-cache-pricing-settings.md](archive/implemented/2026-10-03-m4-cache-pricing-settings.md) | ✅ 完成 2026-10-03 | M4：SQLite 缓存 + 价格外置 + 设置页；三路径数字一致，附带更正 M2 事件数口径笔误 |
| [archive/implemented/2026-10-03-m3-tauri-vue-gui.md](archive/implemented/2026-10-03-m3-tauri-vue-gui.md) | ✅ 完成 2026-10-03 | M3：Tauri 2 + Vue 3 GUI（Naive UI/双主题/托盘常驻）；NSIS 产物已出，GUI 交互冒烟待用户确认 |
| [archive/implemented/2026-10-03-m2-codex-adapter-pricing.md](archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) | ✅ 完成 2026-10-03 | M2：Codex 适配器 + 多 agent 汇总 + 价格表扩充；验收期发现同请求重发并修正口径，cc 对照 38/39 一致 |
| [archive/implemented/2026-10-03-m1-claude-code-adapter.md](archive/implemented/2026-10-03-m1-claude-code-adapter.md) | ✅ 完成 2026-10-03 | M1：Cargo 骨架 + Claude Code 统计闭环；与 cc-switch 对照 15 天中 10 天全指标一致 |
