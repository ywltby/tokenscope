# TokenScope

本地 AI agent 使用量统计的桌面工具：只读扫描各 AI 编程工具落在本地的会话日志，统计 token 用量、请求数与估算费用。Rust（Tauri 2）+ Vue 3 前端，另附与 GUI 数字同源的 CLI。对标 [cc-switch](https://github.com/farion1231/cc-switch) 的 Usage Statistics，作为独立工具覆盖更多 agent。

## 功能

- **多 agent 统计**：默认合并全部已装 agent，也可单看某个（当前 Claude Code + Codex，更多适配器规划中）
- **多维度聚合**：按日 / 模型 / 项目 / agent 分组，时间范围过滤（全部 / 近 7 / 30 / 90 天）
- **Dashboard**：概览卡片、按日堆叠趋势图（ECharts）、明细表、来源采集统计
- **明暗双模式**：默认跟随系统，可手动切换并记忆
- **托盘常驻**：关闭窗口只是缩到托盘，托盘菜单或左键单击恢复，退出走托盘菜单
- **SQLite 缓存**：`~/.tokenscope/cache.db` 按文件指纹增量失效，缓存故障自动退回全量扫描（缓存是纯优化，日志才是事实源）
- **价格表三层合并**：内置表兜底，一键同步 OpenRouter 466+ 模型价格（含显示名），外置 `~/.tokenscope/pricing.toml` 补充覆盖（本地优先），GUI 设置页可视化管理
- **同源 CLI**：`tokenscope` 命令与 GUI 走同一条 Rust 数据管线，脚本化与核对两用

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
pnpm --dir frontend tauri dev      # 开发运行
pnpm --dir frontend tauri build    # 生产构建，产出 NSIS 安装包
                                    # target/release/bundle/nsis/TokenScope_*_x64-setup.exe
```

### CLI

```powershell
tokenscope summary                      # 合并全部已装 agent，按日汇总（Asia/Shanghai 落日界）
tokenscope summary --agent codex        # 只统计指定 agent（claude|codex）
tokenscope summary --by agent           # 按 agent 分组（--by day|model|project|agent）
tokenscope summary --days 7             # 最近 7 个自然日
tokenscope summary --json               # 机器可读输出（逐源统计，含坏行/去重计数）
tokenscope summary --claude-dir <path> --codex-dir <path>   # 覆盖扫描目录
tokenscope summary --refresh            # 强制全量重解析并重建缓存
tokenscope pricing sync                 # 同步 OpenRouter 价格快照（466+ 模型）
```

## 统计口径

- **去重**：Claude Code 会话日志约 70% 的行是同一消息的流式重写，按 `(sessionId, message.id)` 去重保留最后一条；Codex 会把同一请求的用量原样重发，按 `(session, 用量五元组)` 去重保留首条。去重在全局统一执行，无缓存 / 缓存命中 / `--refresh` 三条路径数字一致。
- **归一化**：Codex 的 `input_tokens` 含缓存（`total = input + output`、`cached ⊆ input`），入账时拆为剔除缓存 input + cache_read，与 Claude 口径对齐。
- **时间**：日志内 UTC 时间戳统一转 Asia/Shanghai 后按自然日落日。
- **健壮性**：子代理（sidechain）、`<synthetic>` 行、零分量占位行与缺字段的坏行一律跳过并计数，不静默入账；agent 目录缺失只警告不报错。
- **计价**：三层来源按优先级合并——本地外置 `pricing.toml` > OpenRouter 同步快照 > 内置表，层内最长前缀匹配（模型名自动归一化，兼容 `vendor/` 前缀与点/横线版本号写法）；`:free` 等变体与基名隔离计价；无价格模型的用量单独列为 unknown，不按 0 吞掉。

## 开发

Rust workspace（核心库 + CLI + Tauri 壳）加 Vue 3 前端（Vite / Naive UI / ECharts / pnpm）：

```
src/          核心库：source 适配器 → dedupe → cache → aggregate → pricing → render；report 管线为 CLI/GUI 共用入口
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
pnpm --dir frontend build          # 生产构建
pnpm --dir frontend tauri dev      # 桌面应用开发运行
```

## License

[MIT](LICENSE)
