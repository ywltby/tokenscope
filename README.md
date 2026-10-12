# TokenScope

本地 AI agent 使用量统计的桌面工具：只读扫描各 AI 编程工具的会话日志，将精简用量长期保存在自己的数据库，统计 token、请求数与估算费用。Rust（Tauri 2）+ Vue 3 前端。对标 [cc-switch](https://github.com/farion1231/cc-switch) 的 Usage Statistics。

当前实现与验收状态见 [计划总账](docs/plans/README.md)；剩余任务统一在 [整合计划](docs/plans/active/2026-10-11-consolidated-remaining-work.md)，包括历史升级与并集边界、当前原生和发布验收。

## 功能

- **多 agent 统计**：默认合并全部已装 agent，也可单看某个（当前 Claude Code + Codex，更多适配器规划中）
- **多维度聚合**：按日 / 模型 / 项目 / 应用分组；时间支持预设（近 N 天，统计时区下 `[起始自然日, 今天]` 闭区间）与**自定义区间**（日期区间选择器）
- **逐请求明细**：点击汇总行即下钻到去重后的请求级明细（时间 / 模型 / 项目 / 四类 token / 费用）
- **Dashboard**：概览卡片、按日堆叠趋势图（ECharts）、明细表、来源采集统计
- **明暗双模式**：默认跟随系统，可手动切换并记忆
- **托盘常驻**：关闭窗口三态——未配置默认动作时弹窗询问「最小化到托盘 / 直接退出」并记忆选择（设置页可改回每次询问）；托盘菜单恢复或退出
- **桌面体验**：单实例互斥（二次启动唤起已有窗口）、窗口尺寸/位置记忆、开机自启（设置页开关）
- **统一用量历史库**：`~/.tokenscope/history.db` 保存精简 token、cwd、请求身份和来源元数据，不保存完整 JSONL；日志删除或来源停用后，已采集用量仍可查。旧 `cache.db` 只读迁移，重扫日志不清历史
- **CCS 手动导入**：设置页手动预览、确认后导入一次；与已有历史去重求并集，不自动读取或同步 CCS。日汇总保留日粒度，不伪造请求明细
- **三来源计价（无内置表）**：外置 `pricing.toml`、models.dev 主源与 OpenRouter 补充源保留独立候选，优先完整候选并按请求选最高估算费用；未收录模型显示「未知」。设置页可同步双源，离线使用本地快照
- **时区可配**：存储一律 UTC，展示按解析链（显式指定 > 本机 > 默认 Asia/Shanghai）一次转换

## 当前支持

| Agent | 状态 | 数据来源 |
| --- | --- | --- |
| Claude Code | ✅ | `~/.claude/projects/**/*.jsonl`，含子代理日志 |
| Codex | ✅ | `~/.codex/sessions/**/*.jsonl` 与 `~/.codex/archived_sessions/**/*.jsonl` |
| Gemini CLI / OpenCode 等 | 规划中 | — |

## 使用

### 桌面应用

```powershell
pnpm --dir frontend install        # 首次安装前端依赖
.\frontend\node_modules\.bin\tauri dev    # 开发运行（必须在仓库根执行）
.\frontend\node_modules\.bin\tauri build  # 生产构建，产出 NSIS 安装包
                                    # target/release/bundle/nsis/TokenScope_*_x64-setup.exe
```

## 统计口径

- **去重**：Claude 按 `(sessionId, message.id)` 更新有效终值；Codex 使用 `(session, 原始模型, 四桶)` 的保守重播身份，文件副本/归档移动不改变身份。因此**同会话同模型同桶数值的不同请求可能被合并**，不能称为强请求身份。历史迁移与跨来源边界的未闭合项见整合计划 N01–N03。
- **归一化**：Codex 的 `input_tokens` 含缓存（`total = input + output`、`cached ⊆ input`），入账时拆为剔除缓存 input + cache_read，与 Claude 口径对齐。
- **时间**：缓存与存储一律 UTC（RFC3339）；聚合与展示按解析出的单一时区一次性转换——默认 Asia/Shanghai，GUI 下拉可选本机或任意 IANA 时区，避免多次转换。一次查询冻结一个「今天」，预设近 N 天为统计时区下 `[起始自然日, 今天]` 闭区间，不含未来日期。
- **全部代理用量**：有效主代理/子代理请求均纳入，不建立代理关系分类。`<synthetic>`、零分量占位与坏行跳过并计数；来源缺失不删除已保存历史。
- **计价**：无编译期内置表；模型末段按等价规则匹配，完整匹配优先、前缀回退只在分隔符边界，版本与 `:free` 等变体隔离。同名多渠道保持独立，完整候选优先、逐请求取**最高费用**，四桶单价来自同一候选；来源等级只在同价时决胜。估算不等于供应商账单。单价三态：数字 / 显式 0 / 未知；外置 `model_policy` 可明确声明缓存读价「沿用输入价」。
- **来源目录**：两个启用的来源（含默认目录）不得相同或嵌套——保存与采集时都会拒绝并提示恢复方法；停用其一即可恢复。
- **设置**：`~/.tokenscope/settings.toml` 全部应用内写入走同一读改写事务（并发不丢字段）；外部编辑器不受此约束，请勿与 GUI 同时写入。

## 开发

Rust workspace（核心库 + Tauri 壳）加 Vue 3 前端（Vite / Naive UI / ECharts / pnpm）：

```
src/          核心库：source 适配器 → history → query/aggregate → pricing；report 管线为 GUI 数据入口
src-tauri/    Tauri 2 壳：窗口/托盘 + commands（参数转换，零业务逻辑）
frontend/     Vue 3 应用：Dashboard 与设置页
tests/        e2e 与合成 fixture（真实日志永不入库）
```

```powershell
# Rust
cargo build                        # 构建 workspace
cargo fmt                          # 格式化（提交前 --check 必须干净）
cargo clippy --all-targets         # 静态检查（提交前必须干净）
cargo test                         # 全部测试

# 前端（TypeScript 钉 5.x，勿升 7——vue-tsc 尚不兼容 TS 7）
pnpm --dir frontend typecheck      # vue-tsc 类型检查
pnpm --dir frontend format         # Prettier 格式化（format:check 为检查）
pnpm --dir frontend build          # 生产构建

# Tauri CLI 在仓库根调用（CLI 只向下搜索 src-tauri，在 frontend 目录执行会找不到配置）
.\frontend\node_modules\.bin\tauri dev      # 桌面应用开发运行
.\frontend\node_modules\.bin\tauri build    # 生产构建（NSIS 安装包）
```

提交钩子：`git config core.hooksPath .githooks` 启用后，分别运行根库与壳的 fmt/clippy/test，以及前端 typecheck/format:check/test。

## License

[MIT](LICENSE)
