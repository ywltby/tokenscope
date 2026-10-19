# SQLite 并发打开失败的根因调查

> 状态：**待用户确认**（2026-10-19，由启动卡死恢复计划复核意见提出：撤回测试层容错后另案调查）。
> 分支与版本归属：随下一个合入 main 的批次交付结论；调查本身不改产品代码。

**Goal：** 确认「两个连接同时打开同一 SQLite 库偶发 SQLITE_READONLY / BUSY」的根因，决定处置方案——环境排除、产品级打开容错，或其他。

## 1. 背景与已核实事实

- 本机（Windows 11，`C:\Users\admin\AppData\Local\Temp` 与项目 `target/` 均可复现）在全量测试的并行负载下，六个并发打开同一库的用例（`test_parallel_queries_single_collection`、`test_cache_concurrent_v3_open`、`concurrent_queries_share_one_collection_without_duplicating_usage`、`concurrent_writers_do_not_duplicate_events`、review_regressions 并发多参查询、`collection_status_follower_shares_leader_round`）会偶发乃至稳定失败；错误统一为 `attempt to write a readonly database: Error code 8`，个别为 `database is locked`。
- 已撤回的测试层重试（32a53e5，复核意见要求撤回）曾掩盖该现象；撤回后全量 `cargo test --workspace` 在本机失败（复核当天实测：`history_collection` 的并发用例命中 readonly）。
- CI windows-latest 从未失败（几十次完整 workspace 测试全绿）；单连接使用（Tauri 应用）无此现象。
- 最小复现与产品代码无关：Python 系统 SQLite 模拟两线程并发建库+WAL（30/30 失败）；预建库后并发完整 open+migrate 序列仍 1/10 失败；预建 WAL 后的并发写稳定通过。
- 复核意见指出：Python 复现同类错误**不能确定**原因是杀软，也不能排除产品需要处理的 SQLite 并发问题——本调查以此为起点。

## 2. 调查步骤（按序执行，每步留证据）

1. **取扩展错误码**：本机复现时以 `RUST_BACKTRACE` 与 rusqlite 错误链取 SQLITE_READONLY 的扩展码（`SQLITE_READONLY_RECOVERY` / `SQLITE_READONLY_CANTINIT` / `SQLITE_READONLY_ROLLBACK` 指向 -shm/-wal 恢复路径；纯 `SQLITE_READONLY` 指向文件只读）。扩展码直接缩小根因范围。
2. **对照实验矩阵**（每个组合跑 ≥20 轮并发用例记录失败率）：
   - Temp 目录 vs 项目 target vs 新用户目录 `%USERPROFILE%\ts-sqlite-probe`；
   - Windows Defender 实时防护关闭（需用户操作或临时排除目录）vs 开启；
   - 干净 Windows 环境（沙盒/VM/另一台机器）对照；
   - rusqlite bundled SQLite 版本与系统 sqlite3.dll 版本对照（`sqlite3_version()`）。
3. **产品行为核对**：审 `HistoryDb::open`（WAL 切换重试只认 DatabaseBusy）与 `Cache::open`（2 秒降级策略）对 readonly 扩展码的覆盖面；若实验证明 CI 同环境不复现、根因确在本机环境，产品无需改动；若发现产品在合法并发时序下也会触发（例如用户同时开两个 TokenScope 实例），按缺陷修产品并补先红后绿回归。
4. **处置**：
   - 环境根因：把测试临时目录纳入排除建议写入 CLAUDE.md 本机环境注意；CI 与产品不变。
   - 产品缺陷：产品级修复（打开时对 shm 初始化失败的降级/重试），改动走独立计划评审；并发用例恢复原断言后自然全绿。

## 3. 不做

- 不恢复测试层环境噪声重试（复核意见明确否决）。
- 不在调查结论前修改产品打开路径。
- 不为绕过噪声调大等待、改 CI 并发或钩子选择逻辑。

## 4. 验收

- 扩展错误码与对照矩阵的原始数据留档（qa-artifacts 或调查记录）；
- 结论写入本文档 §5，并回填整合计划执行账；
- 处置方案经用户确认后执行；若产品修复，回归测试先红后绿。
