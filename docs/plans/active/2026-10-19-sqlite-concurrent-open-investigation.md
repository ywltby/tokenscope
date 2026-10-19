# SQLite 并发打开失败的根因调查

> 状态：**调查完成（2026-10-19）：HistoryDb 与 Cache 两条打开序列均已覆盖、修复并验证**（由启动卡死恢复计划复核意见提出：撤回测试层容错后另案调查）。
> 分支与版本归属：随 `fix/boot-recovery` 批次交付。
> 根因表述分级见 §3：「失败与并发首次初始化绑定」是隔离对照**已证实**的证据；「连接级 journal 状态与库头不一致」是**推断**，未逐层证明。

**Goal：** 确认「两个连接同时打开同一 SQLite 库偶发 SQLITE_READONLY / BUSY」的根因，决定处置方案——环境排除、产品级打开容错，或其他。

## 1. 背景与已核实事实

- 本机全量测试的并行负载下，六个并发打开同一库的用例（`test_parallel_queries_single_collection`、`test_cache_concurrent_v3_open`、`concurrent_queries_share_one_collection_without_duplicating_usage`、`concurrent_writers_do_not_duplicate_events`、review_regressions 并发多参查询、`collection_status_follower_shares_leader_round`）会偶发乃至稳定失败；错误统一为 `attempt to write a readonly database: Error code 8`，个别为 `database is locked`。
- 已撤回的测试层重试（32a53e5，复核意见要求撤回）曾掩盖该现象；撤回后全量 `cargo test --workspace` 在本机失败（复核当天实测：`history_collection` 的并发用例命中 readonly）。
- **对照背景（不构成根因结论）**：CI windows-latest 的历史完整 workspace 测试未见该失败；Tauri 应用单连接使用未见同类报告；复核环境（Codex 同一工作区）本轮实跑原并发用例、237 项核心单测与完整 workspace 测试通过——触发条件由本文档 §2 的隔离对照给出。
- Python 对照：python 3.13 内置 sqlite3 模块（`sqlite3.sqlite_version` = 3.53.1）模拟两线程并发建库 + WAL 曾 30/30 失败；预建 WAL 后的并发写稳定通过。产品路径使用 rusqlite 0.40.2（bundled SQLite 3.53.2）——两者实现不同，Python 结果只作背景对照，不指认任何系统库来源。
- **Cache 同类事实**：`test_cache_concurrent_v3_open`（预建 v3 库 → 两连接并发打开 → 迁移）在当前代码下 100 次运行 2 次失败（2%），错误为「开始缓存初始化事务失败: attempt to write a readonly database: Error code 8」——失败发生在 `BEGIN IMMEDIATE` 建立初始化事务这一步，与 HistoryDb 同一竞态、同一 extended=8。
- 复核意见指出：Python 复现同类错误**不能确定**原因是杀软，也不能排除产品需要处理的 SQLite 并发问题。

## 2. 调查方法与结果（2026-10-19）

1. **连接生命周期阶段定位**：临时探针 example（不提交）复制 `HistoryDb::open` 的 12 个阶段——open_with_flags → busy_timeout → journal_mode=WAL（100×20ms 只重试 DatabaseBusy，同产品）→ foreign_keys → synchronous → 读 user_version → BEGIN IMMEDIATE → DDL + user_version → INSERT meta → COMMIT → read_snapshot 模拟（READ_ONLY + BEGIN + SELECT）→ 关闭；每阶段记录线程、连接序号、单调时间戳、`SqliteFailure` 主码 / extended_code / message，并保留同轮两个连接的完整时间线。四组隔离目录单变量对照（每组 30 轮，每轮独立目录）：
   - **A 并发新建**：30 轮 **11 轮失败**（9 次 `8-ddl`、2 次 `3-wal`），全部 `code=ReadOnly extended=8`（纯 SQLITE_READONLY，无子码），`stage_ms≈0`（立即失败，非锁等待超时）；
   - **B 预建后关闭再并发打开**：30/30 通过；
   - **C 保持一个连接存活再并发打开**：30/30 通过；
   - **D 串行打开对照**：30/30 通过。
   - 证据：`qa-artifacts/sqlite-probe/run1-30rounds.log`、`run2-30rounds-timelines.log`。
2. **失败时序**：失败样本里同轮另一连接已完成 `12-done`（迁移提交完成）；失败连接停在 `7-begin` 之后的第一条 DDL 或 WAL 切换；WAL 切换后查询 `PRAGMA journal_mode` 返回 `wal`（库级模式已切换）。
3. **同连接重试无效**：同一连接内重试 `migrate` 20 次（400ms 预算耗尽）仍失败；**重新打开连接**后成功——指向连接级 journal 状态与库头不一致，而非锁等待。
4. **版本与打开 flags**：产品 SQLite 3.53.2（rusqlite bundled，`rusqlite::version()`）；`HistoryDb::open` 用 `READ_WRITE | CREATE | URI | NO_MUTEX`，`read_snapshot` 用 `READ_ONLY | NO_MUTEX`。
5. **失败 / 通过会话环境记录**：失败会话为 WorkBuddy Git Bash（存在 `BASH_FUNC_rm` 包装与 `CODEBUDDY_SAFE_DELETE_*`、沙箱相关变量），cargo/rustc 1.99.0（`C:\Users\admin\.cargo\bin`），TEMP/TMP = `C:\Users\admin\AppData\Local\Temp`，提交时 node 进程 5 个（WorkBuddy 自身）。通过会话（Codex）的工具链与 TEMP 未记录。注意：cargo 构建写 `target/` 被会话沙箱拒绝访问是独立事件（构建缓存写入），不作为 SQLite readonly 的根因。
6. **Cache 覆盖**：修复前 100 次 2 失败、修复后同口径 100 次 0 失败；`Cache::open` 与 `HistoryDb::open` 共用同一重试预算与类型判定（`crate::retry`）。

## 3. 结论与处置

**已证实（隔离对照，证据见 §2）**：失败与「并发首次初始化」绑定——同一进程、同一目录、同一环境下 B/C/D 三组 30/30 通过，只有 A 组失败（HistoryDb 11/30、Cache 2/100）；错误为纯 `SQLITE_READONLY`（extended=8）、立即返回而非锁等待；同一连接内重试无效、重新打开连接恢复。

**推断（未逐层证明）**：具体机理是「本连接在 WAL 切换/迁移窗口内，连接级 journal 状态与库头不一致」。探针证据（`mode=wal` 但写事务立即 readonly、stage_ms≈0、重开连接恢复）支持这一解释，但没有逐层跟踪 SQLite 内部状态，故不作为已证实结论。

**修复**：
- 新增 `crate::retry` 统一预算与判定：2s 共享 deadline、20ms 间隔；`is_concurrent_init_transient` **只按 `rusqlite::Error` 类型**（ReadOnly/Busy）判定，不匹配消息文本；预算耗尽返回最后一个错误。
- `HistoryDb::open` 与 `Cache::open` 的 WAL 切换与整个打开序列共用该预算（原来 WAL 100×20ms 与打开 20×20ms 各自计时、可叠加）；每次重试新建连接。
- 类型化错误保留：迁移失败用 `MigrateFailure` 包装——`Display` 仍带完整原因与「历史库文件保留」位置（未知版本 / SQL 失败不会被压掉），`source` 链保留 `rusqlite::Error`；`anyhow!("{:#}")` 字符串化与消息兜底判定移除。

**回归**：
- 确定性重试测试 5 例（`retry.rs`）：类型判定（含「字符串化后不再算瞬态」的约束，变异验证会转红）、瞬态重试至成功、非瞬态立即失败、零预算不重试、预算上限与最后错误；另加 `MigrateFailure` 的 Display/类型双约束单测。
- 真实并发测试保持原强度、零测试层重试：HistoryDb 50 轮并发首次初始化（先红后绿）；Cache v3 并发打开修复后 100/100；全量 `cargo test --locked --workspace` 连续两次 0 失败套件。

## 4. 不做

- 不恢复测试层环境噪声重试（复核意见明确否决）。
- 不为绕过失败调大等待、改 CI 并发或钩子选择逻辑；不同时保留测试层容错。
- 不把 Python 对照或 CI 历史记录当作根因结论；不指认未查明的系统库来源。

## 5. 验收

- ✅ 扩展错误码与四组对照的原始数据留档（`qa-artifacts/sqlite-probe/`）；
- ✅ 结论写入本文档 §3（根因表述分级：已证实的对照结论 + 未证明的机理推断），回填整合计划执行账；
- ✅ HistoryDb 与 Cache 两条打开序列均修复，带保持原并发强度的回归测试，先红后绿；
- ✅ 确定性重试策略测试覆盖类型判定、重试/不重试、预算上限与最后错误。
