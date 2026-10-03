# 计划目录

- `active/` 进行中的计划；`backlog/` 排队未启动；`archive/implemented/` 已完成归档。
- **滚动状态以本文件总账为准，CLAUDE.md 不追写。**
- 计划必须包含：目标 / 非目标 / 不变量 / 任务清单（每项对应代码位置、测试名、验证命令）/ 验收标准。没有测试名或验证命令的任务不得标记完成。
- 完成验收后把文件移入 `archive/implemented/`，并在下表登记完成时间与验收要点。

| 计划 | 状态 | 备注 |
| --- | --- | --- |
| [active/2026-10-03-m4-cache-pricing-settings.md](active/2026-10-03-m4-cache-pricing-settings.md) | 动工中 | M4：缓存落盘 + 价格外置 + GUI 设置页（用户指示继续推进） |
| [archive/implemented/2026-10-03-m3-tauri-vue-gui.md](archive/implemented/2026-10-03-m3-tauri-vue-gui.md) | ✅ 完成 2026-10-03 | M3：Tauri 2 + Vue 3 GUI（Naive UI/双主题/托盘常驻）；NSIS 产物已出，GUI 交互冒烟待用户确认 |
| [archive/implemented/2026-10-03-m2-codex-adapter-pricing.md](archive/implemented/2026-10-03-m2-codex-adapter-pricing.md) | ✅ 完成 2026-10-03 | M2：Codex 适配器 + 多 agent 汇总 + 价格表扩充；验收期发现同请求重发并修正口径，cc 对照 38/39 一致 |
| [archive/implemented/2026-10-03-m1-claude-code-adapter.md](archive/implemented/2026-10-03-m1-claude-code-adapter.md) | ✅ 完成 2026-10-03 | M1：Cargo 骨架 + Claude Code 统计闭环；与 cc-switch 对照 15 天中 10 天全指标一致 |
