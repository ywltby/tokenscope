# TokenScope

本地 AI agent 使用量统计的桌面工具：只读扫描各 AI 编程工具落在本地的会话日志，统计 token 用量、请求数与估算费用。Rust（Tauri 2）+ Vue 3 前端。对标 [cc-switch](https://github.com/farion1231/cc-switch) 的 Usage Statistics，作为独立工具覆盖更多 agent。

## 功能

- **多 agent 统计**：默认合并全部已装 agent，也可单看某个（当前 Claude Code + Codex，更多适配器规划中）
- **多维度聚合**：按日 / 模型 / 项目 / 应用分组；时间支持预设与**自定义区间**（日期区间选择器 / `--from`+`--to`）
- **逐请求明细**：点击汇总行即下钻到去重后的请求级明细（时间 / 模型 / 项目 / 四类 token / 费用）
- **Dashboard**：概览卡片、按日堆叠趋势图（ECharts）、明细表、来源采集统计
- **明暗双模式**：默认跟随系统，可手动切换并记忆
- **托盘常驻**：关闭窗口只是缩到托盘，托盘菜单或左键单击恢复，退出走托盘菜单
- **桌面体验**：单实例互斥（二次启动唤起已有窗口）、窗口尺寸/位置记忆、开机自启（设置页开关）
- **SQLite 缓存**：`~/.tokenscope/cache.db` 按文件指纹增量失效，缓存故障自动退回全量扫描（缓存是纯优化，日志才是事实源）
- **价格表四层合并**：本地外置 `pricing.toml`（最高优先）> models.dev 同步（默认主源，7900+ 模型）> OpenRouter 同步（备份，460+ 模型）> 内置表兜底；GUI 设置页一键同步双源并可视化管理
- **时区可配**：存储一律 UTC，展示按解析链（显式指定 > 本机 > 默认 Asia/Shanghai）一次转换

## 当前支持

| Agent | 状态 | 数据来源 |
| --- | --- | --- |
| Claude Code | ✅ | `~/.claude/projects/**/*.jsonl` |
| Codex | ✅ | `~/.codex/sessions/**/*.jsonl` |
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

- **去重**：Claude Code 会话日志约 70% 的行是同一消息的流式重写，按 `(sessionId, message.id)` 去重保留最后一条；Codex 会把同一请求的用量原样重发，按 `(session, 用量五元组)` 去重保留首条。去重在全局统一执行，无缓存 / 缓存命中 / `--refresh` 三条路径数字一致。
- **归一化**：Codex 的 `input_tokens` 含缓存（`total = input + output`、`cached ⊆ input`），入账时拆为剔除缓存 input + cache_read，与 Claude 口径对齐。
- **时间**：缓存与存储一律 UTC（RFC3339）；聚合与展示按解析出的单一时区一次性转换——默认 Asia/Shanghai，可显式指定本机时区（`--tz local` / GUI 下拉）或任意 IANA 时区（`--tz UTC` 等），避免多次转换。
- **健壮性**：子代理（sidechain）、`<synthetic>` 行、零分量占位行与缺字段的坏行一律跳过并计数，不静默入账；agent 目录缺失只警告不报错。
- **计价**：三层来源按优先级合并——本地外置 `pricing.toml` > OpenRouter 同步快照 > 内置表，层内最长前缀匹配（模型名自动归一化，兼容 `vendor/` 前缀与点/横线版本号写法）；`:free` 等变体与基名隔离计价；无价格模型的用量单独列为 unknown，不按 0 吞掉。

## 开发

Rust workspace（核心库 + Tauri 壳）加 Vue 3 前端（Vite / Naive UI / ECharts / pnpm）：

```
src/          核心库：source 适配器 → dedupe → cache → aggregate → pricing；report 管线为 GUI 数据入口
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

提交钩子：`git config core.hooksPath .githooks` 启用后，每次 commit 自动跑 Rust fmt/clippy 与前端 typecheck/format:check。

## License

[MIT](LICENSE)
