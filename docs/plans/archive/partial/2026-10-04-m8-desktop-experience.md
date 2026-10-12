# M8：桌面体验补全——单实例互斥、窗口状态记忆、开机自启

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 任务 1–3 实现 | 单实例、窗口记忆、自启均已接线，后续 AP 补最终保存 | — |
| 自身验收 2 | 重开几何恢复、双开唤起、自启注册/移除尚待系统运行验收 | [N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 状态：**已完成（2026-10-04），三项能力待用户安装后冒烟确认**
- 创建：2026-10-04

## 背景

用户正在 dev 模式冒烟 GUI（本会话已按反馈修复：主线程阻塞、转圈居中、筛选两行、agent 图标分段控件、暗色图标隐形等，均以独立 fix 提交）。托盘常驻形态下三个体验缺口浮出：重复启动出现多实例多托盘、窗口尺寸/位置不记忆、无开机自启。

## 目标

1. **单实例互斥**：第二次启动不新建实例，而是唤起已有窗口（官方 `tauri-plugin-single-instance`，注册为首个插件）。
2. **窗口状态记忆**：主窗口尺寸/位置持久化到 `~/.tokenscope/window-state.json`，启动恢复；Rust 侧实现（免前端 window 权限），Resized/Moved 事件更新内存态、后台线程节流落盘（1s）。
3. **开机自启**：官方 `tauri-plugin-autostart`；设置页新增「桌面体验」卡，开关实时生效。

## 非目标

- 多窗口/托盘气泡、自动更新（自动更新涉及签名与网络发布通道，单独立项）
- 窗口状态的手工重置入口（删文件即可）

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | 单实例插件（二次启动唤起窗口） | `src-tauri/src/lib.rs`、`src-tauri/Cargo.toml` | —（手工冒烟） | `cargo build -p tokenscope-tauri` + 双开 exe |
| 2 | 窗口状态持久化/恢复 | `src-tauri/src/window_state.rs` | `test_window_state_`（序列化往返/坏文件容忍） | `cargo test -p tokenscope-tauri` |
| 3 | 自启插件 + 设置页开关 | `src-tauri/src/lib.rs`、`capabilities/default.json`、`frontend/src/views/Settings.vue` | —（手工冒烟） | `vue-tsc` + `pnpm build` |
| 4 | 门禁 + 产物 + 文档 | — | 全量回归 | `cargo test --workspace` + `tauri build` |

## 验收

1. 全量门禁全绿（fmt / clippy / test / vue-tsc / format:check / pnpm build）。
2. 手工冒烟（用户或本会话 dev 实例）：① 调整窗口大小/位置 → 退出重开 → 恢复；② 二次启动 exe → 不出新实例，已有窗口前置；③ 设置页自启开关切换后，系统自启动项出现/消失（任务管理器 → 启动应用核对）。
3. 状态文件坏 JSON → 忽略并用 conf.json 默认尺寸（单测固化容忍行为）。

## 风险

- 自启插件写注册表属系统级变更 → 仅在用户点击开关时触发，默认关闭。
- 窗口状态用物理像素存储 → 跨 DPI 迁移场景极少，同机恢复正确即可。

## 验收记录（2026-10-04）

1. 全量门禁：fmt / clippy（0 警告）/ `cargo test --workspace`（73 测试：根 crate 60 + e2e 6 + tauri 2 + 窗口状态 5）/ vue-tsc / pnpm build 全绿。
2. `tauri build` NSIS 产物 4.38 MiB。
3. 手工冒烟（待用户）：① 调整窗口大小/位置 → 退出重开恢复；② 二次启动 exe 不出新实例、已有窗口前置；③ 设置页自启开关 ↔ 任务管理器启动应用项。

## 实现要点

- 窗口状态：内存态由 Resized/Moved 事件更新（最大化期间不更新矩形），后台线程每秒检查脏标记节流落盘，关窗时机强制落盘；物理像素存储，坏 JSON 告警降级默认尺寸（单测固化）。
- 单实例插件必须最先注册；回调里复用 show_main 唤起窗口。
- 自启走 Rust commands（ManagerExt）而非前端 JS 插件——保持 capabilities 最小（core:default），业务逻辑全部在 Rust 侧的既定模式。
- 时区选择自 Dashboard 迁入设置页（用户决策）：共享 composable `useTimezone`（localStorage 持久化），两页共享同一 ref，设置页修改后回汇总页自动按新时区刷新。
