# M3：Tauri 2 + Vue 3 桌面 GUI

- 状态：**已确认，动工中**（2026-10-03 用户拍板：Naive UI、明暗双模式、CLI 保留、托盘常驻）
- 创建：2026-10-03

## 背景与目标

用户明确产品形态：**Rust 桌面 GUI + Vue 3 前端**（对标 cc-switch 的 Usage Statistics 面板），CLI 不是目标形态。M1/M2 沉淀的核心层（source 适配器 / model / aggregate / pricing，均已 `Serialize`）整体复用为 GUI 的 Rust 后端；CLI 保留为薄壳辅助工具（脚本化与回归测试用），**如用户不要可随时移除，不影响架构**。

M3 目标：搭起 Tauri 2 应用骨架并交付第一个可用 GUI——**用量汇总 Dashboard**（维度切换、agent 过滤、时间范围、趋势图、明细表）。

## 本机环境事实（2026-10-03 实测）

- Node v24.14.0、npm 11.12.0、pnpm 11.22.0（用 pnpm）。
- WebView2 Runtime 154.0.4258.48 已装（Tauri 前置条件满足）。
- Rust 1.99.0 MSVC（满足）；Tauri CLI 未装——以 `@tauri-apps/cli` devDependency 引入，`pnpm tauri <cmd>` 调用，不全局安装。

## 架构与目录（最小重构）

```
tokenscope/                 # 仍是核心 lib + CLI bin（Cargo workspace root）
  src/…                     # M1/M2 核心层原样保留
  src/report.rs             # 新增：SummaryOptions→SummaryReport 管线（从 cli.rs 抽出，CLI/GUI 共用）
  src-tauri/                # 新增：Tauri 2 应用壳（依赖根 crate path 依赖）
    src/{main.rs,lib.rs,commands.rs}
    tauri.conf.json
  frontend/                 # 新增：Vue 3 + Vite + TS（pnpm）
    src/{App.vue, views/Dashboard.vue, components/…}
```

- 根 `Cargo.toml` 加 `[workspace] members = ["src-tauri"]`（根包自动为成员）；**不搬移现有文件**，git 历史与 M1/M2 测试路径零改动。
- Tauri commands 只做参数转换并调用 report 管线，**不含任何解析/聚合逻辑**（层间契约沿 M1）。
- `frontend/` 与 Rust 构建的接线走 Tauri 约定：`beforeDevCommand`/`beforeBuildCommand` 指向 `pnpm --dir frontend build`；`.gitignore` 补 `frontend/node_modules`、`frontend/dist`、`src-tauri/target`（并入现有 target 规则）、`src-tauri/gen`。

## 技术选型

- Tauri 2（`tauri = "2"`、`@tauri-apps/api` v2、`@tauri-apps/cli` v2 devDep）；应用标识 `io.github.ywltby.tokenscope`，窗口标题 `TokenScope`。
- Vue 3.5 + Vite + TypeScript（strict）；组件库 **Naive UI**（Vue3 原生、TS 优先、内置暗色主题）；图表 **ECharts + vue-echarts**。
- 用户已拍板 Naive UI；**明暗双模式**：NConfigProvider + darkTheme，顶栏切换并持久化到 localStorage，默认跟随系统。
- **托盘常驻（用户要求）**：`tauri` features `tray-icon` + `image-png`；应用启动即创建托盘图标；主窗口关闭 → 隐藏到托盘（prevent_close）；托盘菜单「显示主窗口 / 退出」，单击托盘图标恢复窗口。

## CLI / GUI 共用管线（新增 report.rs）

```rust
pub struct SummaryOptions { pub by: GroupBy, pub agent: Option<AgentKind>,
                            pub days: Option<u32>, pub claude_dir: Option<PathBuf>, pub codex_dir: Option<PathBuf> }
pub struct SummaryReport { pub by: &'static str, pub groups: Vec<Group>, pub totals: Group,
                           pub sources: Vec<(AgentKind, CollectStats)>, pub warnings: Vec<String>,
                           pub generated_at: String }
pub fn summary(opts: &SummaryOptions) -> Result<SummaryReport>
```

- `cli.rs` 的 `run()` 改为：组装 options → 调 `summary()` → 表格/JSON 渲染（行为与 M2 输出完全一致，回归验证）。
- Tauri `summarize` command：同样调 `summary()`，`SummaryReport` 直接 serde 序列化返回。

## Tauri commands（M3 面）

| command | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `summarize` | `by, days?, agent?` | `SummaryReport` | 汇总数据（维度/过滤/时间范围） |
| `source_status` | — | `Vec<{agent, dir, exists, files}>` | 首页来源状态提示（目录缺失给出警告） |

## GUI（Dashboard 单页，M3 范围）

- 顶栏：agent 过滤（全部/Claude Code/Codex）、时间范围（全部 / 近 7 / 30 / 90 天）、维度切换（日/模型/项目/Agent）、暗色切换。
- 概览卡片行：请求数、四类 token 合计、估算费用（unknown 显示 `部分未知`）。
- 趋势图：ECharts 按日堆叠柱状（输入/输出/缓存写/缓存读），随维度与过滤联动。
- 明细表：与当前 CLI 表格同列（首列随维度、请求、四类 token、合计、费用$），合计行置底，unknown 加 `†` 脚注；来源采集统计（坏行/去重/跳过）放可折叠区域。
- 目录缺失的 agent 在来源状态里黄条提示，不阻塞其他来源（沿不变量 6）。

## 非目标（后续里程碑）

- 价格表外置配置 / models.dev 导入（M4，GUI 设置页一并做）
- 开机自启、自动更新、多窗口、逐请求明细视图、单实例互斥
- frontend 单测框架（M3 以 `vue-tsc` 类型检查 + `pnpm build` + 手工冒烟为准，vitest 留给 M4 评估）

## 任务清单（代码位置 / 测试名 / 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | report 管线抽取 + CLI 改薄壳 | `src/report.rs`、`src/cli.rs` | `test_report_pipeline_`（fixture 目录驱动） | `cargo test report` + CLI 表格/JSON 与 M2 输出一致 |
| 2 | workspace + src-tauri 骨架 + frontend 骨架 | `Cargo.toml`、`src-tauri/*`、`frontend/*` | `test_tauri_commands_`（command 层单测，mock options） | `pnpm --dir frontend install` → `pnpm --dir frontend tauri dev` 可开窗 |
| 3 | `summarize` / `source_status` commands | `src-tauri/src/commands.rs` | `test_tauri_commands_` | `cargo test -p tokenscope-tauri` |
| 4 | Dashboard：过滤/维度/卡片/明细表 | `frontend/src/views/Dashboard.vue` 等 | 类型检查 | `pnpm --dir frontend typecheck && pnpm --dir frontend build` |
| 5 | ECharts 趋势图联动 | `frontend/src/components/TrendChart.vue` | 类型检查 | 同上 |
| 5b | 托盘常驻 + 关窗缩托盘 | `src-tauri/src/lib.rs` | `test_tray_setup_`（编译期配置 + 手工冒烟） | `cargo build -p tokenscope-tauri` + 冒烟清单 |
| 6 | 生产构建产物 | `src-tauri/tauri.conf.json` | — | `pnpm --dir frontend tauri build` 产出 exe/msi，安装启动冒烟 |
| 7 | 文档回写（CLAUDE.md 设计方向改为「GUI 主形态 + CLI 辅助」、README、.gitignore） | — | — | 人工核对 |

## 验收

1. `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test` 全绿；`vue-tsc` 0 错误；`pnpm build` 通过。
2. CLI 回归：`summary`/`summary --json`/`--by agent` 输出与 M2 一致（对照 M1/M2 验收统计数字）。
3. GUI 冒烟清单：默认开窗显示合并汇总；切换 agent/维度/天数数据联动；趋势图渲染；暗色主题切换；无 agent 目录时黄条提示且不崩溃。
4. `pnpm --dir frontend tauri build` 产出可安装/可运行产物，真实数据下 `--days 30` 与 CLI 数字一致。
5. Rust 侧对 `~/.claude`、`~/.codex` 仍严格只读（GUI 不引入写路径）。

## 风险

- `tauri build` 首次编译依赖多、NSIS 首次下载工具链，耗时长（本机可接受，仅影响首次）。
- Tauri/ECharts 版本迭代快 → 锁定 lockfile，升级单独 plan。
- WebView2 版本差异 → 本机 154 已满足 Tauri 2 最低要求，无需分发 bootstrap。
