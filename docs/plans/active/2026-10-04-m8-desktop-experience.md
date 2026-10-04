# M8：桌面体验补全——单实例互斥、窗口状态记忆、开机自启

- 状态：**已确认（用户 2026-10-04 指示"继续"；取候选清单"桌面体验补全"项）**
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
