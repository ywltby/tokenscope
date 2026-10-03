# TokenScope

本地 AI agent 使用量统计工具。Rust 编写，只读扫描各 AI 编程工具落在本地的会话日志，统计 token 用量与请求数，按 agent / 模型 / 项目 / 时间段聚合查看。对标 [cc-switch](https://github.com/farion1231/cc-switch) 的 Usage Statistics，作为独立工具覆盖更多 agent。

## 当前支持

| Agent | 状态 | 数据来源 |
| --- | --- | --- |
| Claude Code | ✅ | `~/.claude/projects/**/*.jsonl` |
| Codex | ✅ | `~/.codex/sessions/**/*.jsonl` |
| Gemini CLI / OpenCode 等 | 规划中 | — |

## 使用

```powershell
tokenscope summary                      # 合并全部已装 agent，按日汇总（Asia/Shanghai 落日界）
tokenscope summary --agent codex        # 只统计指定 agent（claude|codex）
tokenscope summary --by agent           # 按 agent 分组（--by day|model|project|agent）
tokenscope summary --days 7             # 最近 7 个自然日
tokenscope summary --json               # 机器可读输出（逐源统计，含坏行/去重计数）
tokenscope summary --claude-dir <path> --codex-dir <path>   # 覆盖扫描目录
```

统计口径：Claude Code 会话日志约 70% 的行是同一消息的流式重写，按 `(sessionId, message.id)` 去重保留最后一条；Codex 会把同一请求的用量原样重发，按 `(session, 用量五元组)` 去重保留首条；Codex 的 `input_tokens` 含缓存，归一化为剔除缓存口径与 Claude 对齐；子代理（sidechain）、`<synthetic>` 行、零分量占位行与旧版缺字段的坏行一律跳过并计数，不静默入账。费用按内置价格表估算（USD / 百万 token，快照自 cc-switch `model_pricing` 2026-10-03，最长前缀匹配），无价格模型的用量单独列为 unknown，不按 0 吞掉。

## 开发

```powershell
cargo build
cargo fmt
cargo clippy --all-targets
cargo test
```

工程约定见 [CLAUDE.md](CLAUDE.md)，计划与状态总账见 [docs/plans/README.md](docs/plans/README.md)。对 `~/.claude` 等agent 目录严格只读。

## License

[MIT](LICENSE)
