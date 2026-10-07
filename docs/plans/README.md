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
