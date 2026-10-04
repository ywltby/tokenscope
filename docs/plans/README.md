# 开发计划

- **滚动状态以本文件为准，CLAUDE.md 不追写。**
- `active/` 进行中的计划；`archive/implemented/` 已完成归档（含完整目标 / 非目标 / 不变量 / 任务清单 / 验收记录）。
- 计划必须包含：目标 / 非目标 / 不变量 / 任务清单（每项对应代码位置、测试名、验证命令）/ 验收标准。没有测试名或验证命令的任务不得标记完成。
- 完成验收后把文件移入 `archive/implemented/`，并在文末总账登记完成时间与验收要点。

## 里程碑总览

项目当前形态：**Tauri 2 + Vue 3 桌面 GUI（主）+ 同源 CLI（辅）**，多 agent 用量统计，SQLite 缓存 + 外置价格表。

| 里程碑 | 交付 | 验收要点 | 归档 |
| --- | --- | --- | --- |
| M1 | Cargo 骨架 + Claude Code 统计闭环（source→aggregate→render 分层，去重保末条） | Python 独立复算逐组一致；与 cc-switch 对照 15 个共有日期中 10 天五项指标完全一致 | [归档](archive/implemented/2026-10-03-m1-claude-code-adapter.md) |
| M2 | Codex 适配器 + 多 agent 汇总 + 价格表扩至 43 条（最长前缀） | 验收期发现 Codex 同请求原样重发，修正口径后 cc 对照 38/39 日期全指标一致；M1 回归不变 | [归档](archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) |
| M3 | Tauri 2 + Vue 3 GUI（Naive UI、明暗双模式、托盘常驻、关窗缩托盘） | NSIS 安装包产出；CLI 与 GUI 共用 report 管线，回归逐项一致 | [归档](archive/implemented/2026-10-03-m3-tauri-vue-gui.md) |
| M4 | SQLite 缓存（指纹失效、故障降级）+ 价格外置 TOML + GUI 设置页 | 无缓存/命中/--refresh 三路径数字逐字段一致；更正 M2 归档事件数口径笔误（最终 20,935） | [归档](archive/implemented/2026-10-03-m4-cache-pricing-settings.md) |
| M5 | OpenRouter 价格同步（主源）+ 三层合并（外置 > openrouter > 内置）+ 变体隔离 | 真实同步 466 条、价格与官方一致；unknown 大幅下降；NSIS 4.27 MiB | [归档](archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md) |
| M6 | UTC 存储不变量 + 时区解析链（--tz local/IANA，默认上海）+ JSON timezone 字段 | 本机==上海逐字段一致；UTC 日界生效；非法时区报错；NSIS 产物 | [归档](archive/implemented/2026-10-04-m6-timezone-resolution.md) |
| M7 | 逐请求明细（CLI events + GUI 行点击下钻）+ collect_all 共用路径 | 真实数据总额核对一致（3,741=3,741；日明细 token 求和一致） | [归档](archive/implemented/2026-10-04-m7-event-drilldown.md) |

## 当前状态

- **进行中：[M8 桌面体验补全](active/2026-10-04-m8-desktop-experience.md)**（单实例互斥 / 窗口状态记忆 / 开机自启）。
- GUI 冒烟进行中（用户 dev 模式实测反馈，会话内已修复：主线程阻塞、转圈居中、筛选两行、agent 图标分段控件、暗色图标隐形等）。
- 待办尾巴：M3/M4 的 GUI 交互冒烟留用户安装确认（数据正确性已由 CLI/测试覆盖）。

## 后续候选方向（未立项；择项后在 `active/` 成文，经确认再动工）

> **排期决策（用户 2026-10-04）**：当前专注 **Claude Code 与 Codex** 两个适配器；新增 agent 适配器（下表 2/3）暂缓排期，待安装使用或拿到样例日志、且用户明确排期后再立项。

1. ~~价格在线同步~~ → ✅ 已完成（M5 OpenRouter 价格同步，466 条实测）。
2. **Gemini CLI 适配器**：`~/.gemini` 用量日志。**暂缓前提**：本机未安装（无真实日志可实测验收）；待安装使用或拿到样例日志后立项。
3. **OpenCode 适配器**：同上，暂缓前提同 Gemini（本机无数据源）。
4. **逐请求明细视图**：按会话/日期下钻请求级列表，数据已在缓存 `events` 表，主要是 GUI 工作。
5. **桌面体验补全**：开机自启、单实例互斥、自动更新、窗口尺寸记忆。

## 总账

| 计划 | 状态 | 备注 |
| --- | --- | --- |
| [archive/implemented/2026-10-04-m7-event-drilldown.md](archive/implemented/2026-10-04-m7-event-drilldown.md) | ✅ 完成 2026-10-04 | M7：逐请求明细视图（CLI events + GUI 下钻）；真实数据总额核对一致 |
| [archive/implemented/2026-10-04-m6-timezone-resolution.md](archive/implemented/2026-10-04-m6-timezone-resolution.md) | ✅ 完成 2026-10-04 | M6：UTC 存储固化 + 时区解析链（--tz local/IANA，默认上海）；JSON 增 timezone 字段 |
| [archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md](archive/implemented/2026-10-04-m5-openrouter-pricing-sync.md) | ✅ 完成 2026-10-04 | M5：OpenRouter 价格同步（主源）+ 本地三层合并；466 条实测，变体隔离修正误定价 |
| [archive/implemented/2026-10-03-m4-cache-pricing-settings.md](archive/implemented/2026-10-03-m4-cache-pricing-settings.md) | ✅ 完成 2026-10-03 | M4：SQLite 缓存 + 价格外置 + 设置页；三路径数字一致，附带更正 M2 事件数口径笔误 |
| [archive/implemented/2026-10-03-m3-tauri-vue-gui.md](archive/implemented/2026-10-03-m3-tauri-vue-gui.md) | ✅ 完成 2026-10-03 | M3：Tauri 2 + Vue 3 GUI（Naive UI/双主题/托盘常驻）；NSIS 产物已出，GUI 交互冒烟待用户确认 |
| [archive/implemented/2026-10-03-m2-codex-adapter-pricing.md](archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) | ✅ 完成 2026-10-03 | M2：Codex 适配器 + 多 agent 汇总 + 价格表扩充；验收期发现同请求重发并修正口径，cc 对照 38/39 一致 |
| [archive/implemented/2026-10-03-m1-claude-code-adapter.md](archive/implemented/2026-10-03-m1-claude-code-adapter.md) | ✅ 完成 2026-10-03 | M1：Cargo 骨架 + Claude Code 统计闭环；与 cc-switch 对照 15 天中 10 天全指标一致 |
