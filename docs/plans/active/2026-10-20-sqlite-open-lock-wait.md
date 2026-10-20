# SQLite 打开流程的锁等待回归修复

> 状态：**用户 2026-10-20 要求写成计划交执行模型实施。** 承接[整合计划 N15](2026-10-11-consolidated-remaining-work.md#n15历史库打开锁等待回归)。
> 来源：PR #16（v0.5.2）合入后的复核。[SQLite 并发打开调查](../archive/implemented/2026-10-19-sqlite-concurrent-open-investigation.md)的并发首次初始化修复本身成立，本计划只纠正它顺带改变的锁等待语义。
> 分支 `fix/sqlite-open-lock-wait`；修复类批次，递增 patch；一个 PR、一次版本。

**Goal：** 打开历史库与缓存时，「重试多久」和「每次等锁多久」分开计算。慢 I/O、升级前备份不再因为总预算耗尽而打开失败；打开之后，连接的锁等待恢复为 PR #16 之前的常驻值；已是最新版本的历史库，打开时不再拿写锁。

**Architecture：** 只改 `crates/tokenscope-core` 的 `retry.rs`、`history.rs`、`cache.rs` 及其单测。2 秒绝对 deadline 保留，但只用来决定「瞬态错误之后还要不要重开连接再试」。每个可能等锁的阶段改用固定的 busy_timeout（历史库 5 秒、缓存 2 秒，均为 PR #16 之前的值）。`migrate` 在版本已是最新且元信息齐全时直接返回，不建立写事务。采集、查询、导入的调用方不改。

**Tech Stack：** Rust / rusqlite 0.40（bundled SQLite 3.53.2）；`PRAGMA busy_timeout` 可读回当前值，用于确定性断言。

## 1. 背景与已核实事实

- **修改前（`9ea0c58`）**：`HistoryDb::open` 设置 `busy_timeout = 5000ms`，此后不再改动，打开流程没有总时长上限。`Cache::open` 是固定 2 秒。
- **修改后（PR #16 至今）**：
  - `HistoryDb::open` 与 `Cache::open` 的全过程共用一个 2 秒绝对 deadline，覆盖建文件、切换 WAL、读版本、升级前备份和迁移事务。
  - 每个阶段的 busy_timeout 都取「剩余预算」，耗尽就以字符串错误失败。这种错误不是类型化的瞬态错误，所以也不会重试。
  - open 返回后不再重设 busy_timeout，连接一直沿用最后一次设置的剩余值。
- **探针实测（2026-10-20，临时 worktree，未提交）**：
  - 新建库打开后，另一个连接持有 `BEGIN IMMEDIATE`，这时在已打开的连接上 `set_meta`：约 **2.2 秒**报 `database is locked`，修改前会等 5 秒。
  - 同样持锁的情况下再执行一次 `HistoryDb::open`：约 **2.2 秒**后失败。
- **CI 证据**：main `89aec76` 的 [CI 39361520195](https://github.com/ywltby/tokenscope/actions/runs/39361520195)（rust-acceptance）中，`history::tests::open_lock_wait_is_bounded_by_retry_budget` 在**无任何竞争的第一次打开**（`history.rs:2977`）就失败了，报「读取历史库版本前预算不足：并发初始化重试预算已耗尽」。CI 负载下，新建库加切换 WAL 就超过了 2 秒。
- **调用面**：
  - 每次查询都会经过 `query_begin` → `report::open_read_snapshot` → `HistoryDb::open`。而 `migrate` 每次都要 `BEGIN IMMEDIATE` 补元信息，所以**每次查询都要拿写锁**。
  - 采集把一轮事件放在一个写事务里提交（`report.rs` 的 `write_batch`），串行锁是 `COLLECT_SERIAL`。
  - CCS 导入（`commands.rs` 的 `open_history_for_import`）**不在**这个串行锁内，会和采集争用写锁。
- **大库升级的后果**：升级前备份（`VACUUM INTO`）超过 2 秒时，打开失败并报「备份完成，但重试预算已耗尽」，要到第二次启动才能成功（那时备份已存在会被跳过）。`backup_exhausting_budget_stops_before_migration_transaction` 把这个行为当作预期固化了下来。当前结构版本是 v4，下次升级结构时就会触发。
- **未实测**：真实冷扫描时写事务的时长。这一点不作为修复前提：固定等待和「最新版本不拿写锁」本身就消除了查询与采集之间的竞争。

## 2. 范围与不做

做：

- 把重试 deadline 与锁等待拆开。
- 打开结束时重设常驻 busy_timeout。
- 慢 I/O 和备份不受总预算约束。
- 版本已是最新时打开不拿写锁。
- 改写相应测试，更新文档并交付。

不做：

- 不改 2 秒重试 deadline、20ms 间隔，不改瞬态错误的类型判定（ReadOnly/Busy）和「每次重试新建连接」。
- 不加测试层重试，不改 CI 并发或钩子选择。
- 不调大 PR #16 之前的等待值（历史库 5 秒、缓存 2 秒原样恢复）。
- 不改采集的分批方式、查询快照设计和导入流程，不把 CCS 导入并入采集串行锁。
- 不在 CI 跑压测。
- #18 取证判定偏严、引导期「重新检查」文案两项复核意见不在本计划内。

## 3. 必须保持的不变量

1. **重试规则不变**：只有类型化瞬态错误（`rusqlite` ReadOnly/Busy）才会重试；每次重试都新建连接；只有 deadline 未到期才启动下一次尝试。到期后返回最后一个类型化错误。
2. **锁等待固定**：每个可能等锁的阶段，busy_timeout 都是固定常驻值（历史库 5000ms、缓存 2000ms），**不取剩余预算**。open 成功返回时，连接的 `PRAGMA busy_timeout` 等于常驻值。
3. **不等锁的步骤不计时**：建目录和文件、切换 WAL 的 I/O、读版本、`VACUUM INTO` 备份、DDL，都不因已经过的时间而失败。
4. **升级语义不变**：先备份，再进入一个迁移事务，失败整体回滚、原库保留；备份已存在就跳过；未知更高版本拒绝打开。
5. **最新版本只读打开**：版本等于 `HISTORY_SCHEMA_VERSION`、四个元信息键都已存在时，`migrate` 不建立写事务，文件内容不变，generation 等计数不变。缺任何一个键时照旧在事务里补齐，已有值一律保留。
6. **并发首次初始化不退化**：`concurrent_first_initialization_both_succeed`（50 轮）和 Cache v3 并发打开回归保持原强度，零测试层重试，照常通过。
7. **单次尝试有上界**：一次尝试最多等待「等锁阶段数 × 常驻值」再加 I/O 时间；deadline 只决定是否再开下一次尝试，不存在多个完整等待无限叠加的情况。这个上界写进 `retry.rs` 的模块注释。

## 4. 任务

每项先写会失败的测试，在当前 main 上确认红，再实现。一个提交只解决一类问题。

### LW01 拆开重试 deadline 与锁等待

- 文件：`retry.rs`、`history.rs`、`cache.rs`。
- **测试先行**：
  - **新建库**：用 `StepClock` 让每个检查点推进远超 2 秒（例如 3 秒），无竞争地新建并打开历史库和缓存，必须成功。这是 CI 39361520195 的确定性复现，当前代码会红。
  - **常驻值**：打开后 `PRAGMA busy_timeout` 读回值，历史库等于 5000，缓存等于 2000。分别在真实时钟和步进时钟下各测一次。当前代码会红：真实时钟下略小于 2000，步进时钟下远小于常驻值。
- **实现**：
  - `retry.rs` 新增两个常量：历史库与缓存各自的常驻锁等待。
  - 每个等锁阶段（WAL 每次尝试内部、读版本前、迁移/初始化事务前）改为设置固定常驻值。
  - 删除或改名 `apply_busy_budget` 中「预算耗尽就 bail」的语义。`Deadline::busy_budget` 如果不再被使用就删掉，不留死代码。
  - `open_once` 成功返回前，重设一次常驻值。
- **更新模块注释**：说明 deadline 只决定是否重试，以及不变量 7 的上界。

### LW02 升级前备份不受预算约束

- 文件：`history.rs`。
- **测试改写**：
  - `backup_exhausting_budget_stops_before_migration_transaction` 改为：备份期间步进时钟越过 deadline，升级仍完成，`user_version` = 4，备份文件存在。
  - `migration_transaction_gets_only_remaining_budget_after_backup` 改为断言：迁移事务前的 busy_timeout 等于常驻值。
  - 保留 trace 检查点，只改断言。
- **实现**：删除备份前后的预算检查（「备份完成，但重试预算已耗尽」这条路径整体移除）。

### LW03 最新版本打开不拿写锁

- 文件：`history.rs`。
- **测试先行**：
  - 建好一个最新版本的历史库。另一个连接持有 `BEGIN IMMEDIATE` 期间，`HistoryDb::open` 加 `read_snapshot` 能在 1 秒内成功，读到的 generation 与持锁前一致。当前代码会等满约 2 秒后失败。
  - 删掉一个元信息键后再打开：照旧补齐，其余键保持原值。
- **实现**：`migrate` 读版本后，如果等于当前版本，就用一条只读查询确认四个元信息键都存在；齐全则直接返回。否则走原有的事务路径。

### LW04 改写真实持锁用例

- 文件：`history.rs`、`cache.rs`。
- **历史库**：`open_lock_wait_is_bounded_by_retry_budget` 按新语义改名，例如 `open_lock_wait_uses_fixed_busy_timeout`。
  - 持锁对象改为**需要迁移**的库（用 `seed_v3_history`）。最新版本的库在 LW03 之后已经不需要锁。
  - 断言：错误是类型化瞬态错误；耗时 ≥ 常驻值 − 0.5 秒。上界放宽到「常驻值 × 2 + 2 秒」，只用来排除无限叠加。
- **缓存**：同名用例同样改写，缓存每次打开都会建立初始化事务，持锁对象不用变。
- 用例里的第一次打开（持锁方自己）不得依赖「2 秒内完成」。

### LW05 文档与交付

- **计划与归档文档**：
  - 本计划回填执行记录。
  - [调查归档](../archive/implemented/2026-10-19-sqlite-concurrent-open-investigation.md)顶部加一条复核说明：2 秒总预算兼作锁等待的语义已由本计划修正，正文保留不改。
  - 整合计划的 N15 行与 §7 执行账回填结果，`docs/plans/README.md` 同步。
- **交付步骤**：
  1. 从最新 `origin/main` 合并。
  2. `python -B scripts/upgrade_baselines.py add --tag v0.5.4`，登记上一版。
  3. `python -B scripts/bump_version.py --bump patch --summary "<本轮说明>"`，然后 `--check --base-ref origin/main`。
  4. 开草稿 PR。PR CI（`gate`）、Native UI E2E、Install test 全绿后交复核，**合并由用户决定**。

## 5. 验收

- LW01–LW04 的新测试或改写测试，都在修改前的代码上确认红（记录失败输出），修改后变绿。
- 两项变异验证：把常驻值改回「剩余预算」、在最新版本路径恢复 `BEGIN IMMEDIATE`，对应测试都会转红（只在本地做，不提交）。
- `cargo test --locked -p tokenscope` 在本机连续两次全绿；`cargo clippy --locked --workspace --all-targets`、`cargo fmt --all --check` 干净。
- 草稿 PR 的 CI、Native UI E2E、Install test 全部成功；原生失败按 `evidence.json` 说明原因，不靠重跑通过。

## 6. 执行记录

（执行模型回填：提交、红绿日志、变异验证结果、PR 与 CI 运行链接。）
