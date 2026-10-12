# M11：价格索引预计算 + 定时自动同步

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 持久索引/进程缓存/自动同步实现 | 已完成，后来有真实磁盘索引和冷启动子进程验证 | — |
| 任务 3 / 验收 3 | Ready 后跨 120s 观察、关闭后停止与再开启未有完整运行证据 | [N10](../../active/2026-10-11-consolidated-remaining-work.md#n10)、[N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |
| 设置页性能 | 历史 0.86s 是 CLI，不证明设置页 <100ms | [N13](../../active/2026-10-11-consolidated-remaining-work.md#n13)（共用测量） |

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 状态：**已完成（2026-10-04），GUI 自动同步冒烟待用户确认**
- 创建：2026-10-04

## 背景（用户反馈）

- 切到设置页要等加载：`pricing_entries` 每次重新解析双快照（models.dev 7,957 条 + OpenRouter 466 条）；
- 程序常驻时价格**不会**自动更新（同步目前仅为显式动作）；
- 用户要求：解析在后台算好 + 定时自动同步。

## 目标

1. **价格索引持久化**：`~/.tokenscope/pricing-index.json`（归一化前缀 + 层级 + 价格的扁平结构，同步后/首次构建后写入）。启动与每次使用优先加载索引（扁平 JSON，毫秒级），仅当三源文件 mtime 签名变化时才从原始快照/内置表重建。
2. **进程内缓存**：`Pricing` 按"三源签名"缓存（`Arc<Pricing>`），签名不变直接复用——summary/list_events 每次调用零重复解析。
3. **定时自动同步**（GUI 常驻线程）：默认开启、每 24h 检查一次，距上次成功同步 ≥24h 则后台同步双源并重建索引；启动后 2 分钟做首次检查。同步结果写日志，不弹窗不阻塞。
4. **设置持久化**：`~/.tokenscope/settings.json`（`price_auto_sync`，默认 true）；设置页「桌面体验」卡新增开关，实时生效。

## 设计决策

- **联网边界修订**：统计管线仍然永不联网；价格同步从"仅显式"改为"默认每 24h 自动（可关）"——用户 2026-10-04 明确要求，CLAUDE.md 原则同步修订。
- **签名失效**：sig = 三源文件路径 + (size, mtime_ms)；签名一致 → 索引/进程缓存直接复用。外置 pricing.toml 编辑后 mtime 变化 → 自动重建（无需重启）。
- **索引内容**：含全部四层条目（tier/display/name/四价），即合并结果——加载索引即得到最终查找表，运行期零合并成本。
- 同步线程失败只写日志（log::warn/error 带堆栈），不影响 GUI；CLI 保持显式同步、不启动常驻线程。

## 非目标

- 同步间隔可配置（固定 24h，后续按需）
- 双源冲突仲裁 UI（价差悬浮可见 + 外置可覆盖）

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | settings.rs（load/save/默认值容忍） | `src/settings.rs` | `test_settings_`（往返/缺失/坏文件） | `cargo test settings` |
| 2 | pricing 索引持久化 + 进程内缓存（签名失效） | `src/pricing.rs` | `test_pricing_index_roundtrip` / `test_pricing_sig_rebuild` | `cargo test pricing` |
| 3 | GUI 自动同步线程（24h，首查 2min） | `src-tauri/src/lib.rs` | —（日志冒烟） | dev 日志观察 |
| 4 | 设置页开关 + commands | `src-tauri/src/commands.rs`、`frontend/src/views/Settings.vue` | `test_parse_` 回归 | `vue-tsc` |

## 验收

1. 全量门禁全绿。
2. 设置页打开不再重解析双快照（进程内缓存/索引命中，耗时 <100ms 量级）；`summary` 二次调用不再重读快照。
3. 自动同步：dev 启动 2 分钟后日志出现双源同步记录；关闭开关后不再自动同步（settings.json 持久化）。
4. `pricing sync`（CLI）后索引文件同步更新。
5. 统计管线仍零联网（代码路径不变）。

## 风险

- 索引文件损坏 → 告警并从快照重建（与快照降级策略一致）。
- 系统休眠导致定时器漂移 → 每小时轮询检查"距上次同步"而非精确定时。

## 验收记录（2026-10-04）

1. 全量门禁：fmt / clippy（0 警告）/ `cargo test --workspace`（73 测试：根 crate 69 + e2e 6... 含 settings 4 个新单测）/ vue-tsc / pnpm build 全绿。
2. 索引持久化验收：CLI `summary` 一次运行后 `~/.tokenscope/pricing-index.json` 生成（8,467 条，层级分布 models.dev 7,957 / openrouter 466 / 内置 43 / 外置 1）；二次运行索引命中 0.86s。
3. 进程内缓存：三源签名一致时 `Pricing` 以 Arc 复用，summary/list_events 零重复解析。
4. 自动同步线程：GUI 启动 2 分钟首查、每小时轮询、距上次同步 ≥24h 触发双源同步并重建索引（日志留痕）；settings.json 持久化开关（默认开）。
5. 设置页：价格卡新增自动同步开关（settings_get / settings_set_price_auto_sync）。
6. `tauri build` NSIS 4.77 MiB；GUI 自动同步冒烟留用户确认。

## 实现要点

- 索引键为归一化前缀字节串（避免多字节切片 panic）；索引内容即四层合并结果，加载即得最终查找表。
- `Pricing::load_cached` 返回 Arc<Pricing> + 命中标记；进程内缓存用 `static Mutex<Option<(sig, Arc<Pricing>)>>`。
- 自动同步轮询（每小时）而非精确定时（休眠漂移）；同步失败仅日志，不影响 GUI。
- 排障记录：本轮两处 python replace 静默失败（fmt 重排锚点不匹配）导致接线未生效，改为 Edit 精确修改后修复——**代码接线类改动停止使用 python 字符串替换**。
