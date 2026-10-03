# M4：缓存落盘（SQLite + 增量失效）+ 价格表外置 + GUI 设置页

- 状态：**已确认（用户 2026-10-03 指示"继续推进 M4"，方向即 M3 收尾对齐项）**
- 创建：2026-10-03

## 目标

1. **缓存落盘**：TokenScope 自有数据目录 `~/.tokenscope/` 建 SQLite 缓存（`cache.db`），按文件指纹增量失效，启动不再每次全量解析 JSONL；CLI `--refresh` 强制全量重建。
2. **价格表外置**：`~/.tokenscope/pricing.toml`（TOML，手写友好），外置条目与内置表合并、同前缀外置覆盖内置；坏文件警告降级，不崩溃。
3. **GUI 设置页**：数据来源状态、缓存状态、价格表条目（内置/外置标识）、打开价格文件、重建缓存按钮。

## 架构决策（先于代码）

- **缓存是纯优化，不是事实源**：任何缓存故障（打不开/损坏/被锁/查询失败）→ stderr 警告 + 全量内存扫描，结果必须与无缓存完全一致，退出码不受影响。日志目录本身永远是事实源。
- **失效粒度 = 文件级**：指纹 `(size, mtime_ms)`，变化即整文件重解析（单文件解析毫秒级，不做字节偏移追加——cc-switch 的字节级游标为常驻后台同步设计，我们的模型是"启动时刷新"，文件级简单且无部分行问题）。消失的文件从缓存清除。
- **去重上移**（本次重构核心）：M1/M2 的去重目前实现在 source 适配器内（Claude 全局、Codex 全局），缓存按文件存事件后必须由**全局步骤**统一去重才能保持口径。重构为：source 只产出「未去重事件 + 文件内跳过计数」，report 管线新增 `dedupe` 模块按 agent 规则去重（Claude `(session, message.id)` 保末条、Codex `(session, 用量五元组)` 保首条），`duplicates_dropped` 由该步骤统计并回填 SourceReport。**三条路径（无缓存/命中/--refresh）的 SummaryReport 必须逐字段一致**（e2e 固定），fixture 数字回归不变。
- **SQLite**：rusqlite（bundled，免系统依赖）；WAL + busy_timeout；事件表按文件外键级联删除。路径 `~/.tokenscope/cache.db`。
- **价格合并规则**：外置与内置合并为一张表，相同前缀**外置覆盖内置**；查找仍是最长前缀。文件不存在 = 纯内置（静默）；解析失败 = 警告 + 纯内置。
- **SummaryOptions 扩展**：`cache_dir: Option<PathBuf>`、`pricing_path: Option<PathBuf>`（测试注入；None = `~/.tokenscope/` 默认）、`refresh: bool`。

## 非目标（M5+）

- models.dev 在线导入（GUI 设置页预留说明，不做网络请求）
- 字节级增量游标、常驻后台同步、文件监听
- 价格表在线更新

## 数据库 schema

```sql
PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- schema_version
CREATE TABLE IF NOT EXISTS files (
  id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL, agent TEXT NOT NULL,
  size INTEGER NOT NULL, mtime_ms INTEGER NOT NULL,
  lines_seen INTEGER NOT NULL, bad_lines INTEGER NOT NULL,
  skipped_sidechain INTEGER NOT NULL DEFAULT 0, skipped_synthetic INTEGER NOT NULL DEFAULT 0,
  skipped_zero_usage INTEGER NOT NULL DEFAULT 0, skipped_no_model INTEGER NOT NULL DEFAULT 0,
  ignored_token_usage_record INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS events (
  file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
  ts TEXT NOT NULL, model TEXT NOT NULL, session_id TEXT NOT NULL, project TEXT NOT NULL,
  input INTEGER NOT NULL, output INTEGER NOT NULL,
  cache_write INTEGER NOT NULL, cache_read INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_file ON events(file_id);
```

## CLI / GUI 面

- CLI：`summary --refresh`（重建缓存）；其余不变，输出与 M3 逐字节一致。
- GUI 设置页（顶栏导航 汇总/设置）：来源状态卡（agent、目录、文件数、缺失黄条）、缓存卡（缓存文件数/事件数、「重建缓存」按钮）、价格卡（条目表含来源列 内置/外置、外置文件路径、「打开价格文件」按钮、无外置文件时提供「创建模板」）。
- 新 commands：`cache_stats`、`refresh_cache`、`pricing_entries`、`open_pricing_file`。

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | 去重上移：dedupe 模块 + source 适配器去重移除 + 回填 stats | `src/dedupe.rs`、`src/source/{claude,codex}.rs`、`src/report.rs` | `test_dedupe_claude_` / `test_dedupe_codex_`（含跨文件场景，自原 fixture 单测迁移） | `cargo test dedupe` + 全量 fixture 回归 |
| 2 | cache 模块：schema/指纹/增删/降级 | `src/cache.rs` | `test_cache_roundtrip` / `test_cache_invalidate_` / `test_cache_stale_purge` / `test_cache_corrupt_fallback` | `cargo test cache` |
| 3 | pricing 外置：加载合并/覆盖/坏文件 | `src/pricing.rs` | `test_pricing_external_`（合并/覆盖/缺失/坏文件） | `cargo test pricing` |
| 4 | 管线接入 + CLI `--refresh` + 三路径一致 e2e | `src/report.rs`、`src/cli.rs` | `test_report_cache_consistency`（无缓存/命中/refresh 三跑逐字段相等） | `cargo test report` |
| 5 | GUI 设置页 + 新 commands + opener 插件 | `src-tauri/src/commands.rs`、`frontend/src/views/Settings.vue` | `test_parse_` 回归 + `test_tauri_pricing_` | `cargo test -p tokenscope-tauri` + `vue-tsc` |
| 6 | 文档回写 + 归档 | CLAUDE.md/README | — | 人工核对 |

## 验收

1. 全量门禁：`cargo fmt --check`、`cargo clippy --workspace --all-targets`、`cargo test --workspace`、`vue-tsc`、`pnpm build` 全绿。
2. **三路径一致性**：fixture 上无缓存 / 缓存命中 / `--refresh` 三次运行的 SummaryReport 序列化结果完全相等（测试固化）；CLI 对真实 `~/.claude`+`~/.codex` 的统计与 M3 记录一致（19+47 文件、3,741+23,947 事件、去重 8,620+3,012）。
3. 故障注入：把 cache.db 换成垃圾字节 → 警告 + 结果仍正确；pricing.toml 写坏 → 警告 + 内置价格生效。
4. 外置价格覆盖生效：写一条同前缀外置价 → 对应模型费用变化（fixture 固化）。
5. `tauri build` 产物可构建；设置页冒烟（缓存状态显示、重建按钮、打开价格文件）留用户安装确认。
6. 只读不变量：对 agent 目录仍零写入；TokenScope 只写 `~/.tokenscope/`。

## 风险

- 去重上移动到口径实现位置 → 靠 fixture 数字回归兜底（duplicates_dropped 总量在两种实现下必相等：occurrences−1）。
- rusqlite bundled 首次编译增量编译时间 → 一次性成本，可接受。
- tauri-plugin-opener 权限/版本摩擦 → 失败则退化为 `std::process::Command` 打开默认编辑器（Windows 先行）。
- CLI 与 GUI 并发访问 cache.db → WAL + busy_timeout + 锁冲突降级全量扫描。
