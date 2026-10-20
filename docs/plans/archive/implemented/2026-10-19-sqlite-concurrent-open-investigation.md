# SQLite 并发打开失败的根因调查

> **2026-10-20 锁等待复核补充：** 本文历史实现中“2 秒总预算兼作 SQLite 锁等待”的语义已由[整合计划 N15](../../active/2026-10-11-consolidated-remaining-work.md#n15历史库打开锁等待回归)登记的专项修复纠正（`ff1bda0`，随 [PR #19](https://github.com/ywltby/tokenscope/pull/19) 的 0.5.5 交付；合入与发布状态以 N15 为准）。重试 deadline 只限制失败后的下一次尝试；历史库/缓存恢复固定 5000/2000ms，成功备份不因耗时中止，完整最新历史库免写事务打开。并发初始化修复与类型化错误继续保留；下文正文保留原时点记录，不作为新的锁等待验收标准。

> **2026-10-20 归档复核：已完成并发布。** 本计划随 [PR #16](https://github.com/ywltby/tokenscope/pull/16) 合入 main `0ffab19`，并由 [Release 39353860618](https://github.com/ywltby/tokenscope/actions/runs/39353860618) 发布 [v0.5.2](https://github.com/ywltby/tokenscope/releases/tag/v0.5.2)。main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39353860584)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39353860657) 均成功；发布的安装、六个单跳升级和两条连续升级链全部通过。原草稿交付限制及“待复核/未合并”描述仅为下文历史状态。
>
> 启动恢复与 SQLite 并发打开修复均已通过复核，无独立实施待办。旧 dark 会话超时的具体根因未被追溯证实；取证与恢复交付完成不等于证实该根因。真实系统验收仍见[独立待办](../../system-acceptance-todo.md)，其他剩余任务只在[整合计划](../../active/2026-10-11-consolidated-remaining-work.md)维护。

## 归档前原文

> 状态：**调查完成（2026-10-19）：HistoryDb 与 Cache 两条打开序列均已覆盖、修复并验证**（由启动卡死恢复计划复核意见提出：撤回测试层容错后另案调查）。
> 分支与版本归属：随 `fix/boot-recovery` 交付至草稿 PR #16（版本 0.5.2）；前置的 v0.5.1 已发布并登记基线。
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
7. **真实持锁用例**（HistoryDb 与 Cache 各一条）：另一连接持有 `BEGIN IMMEDIATE` 写事务时，`open` 的总等待受外层绝对 deadline 约束（2s 附近）并以类型化瞬态错误失败。断言留调度容差（下界预算 −0.5s、上界预算 +1s）——「锁等待预算」可精确断言，操作系统 I/O 与线程调度不可严格限制，只断言不会出现「多个完整预算叠加」的量级。
8. **迁移路径的预算传递与确定性回归**：`migrate` 接收同一个 deadline/clock，在读版本、备份前、**备份后**、建立迁移事务前各按剩余预算设置 `busy_timeout`——备份（`VACUUM INTO`）消耗的时间会反映到事务前预算，耗尽则不建立迁移事务；WAL 的 `apply_busy_budget` 位于**每次尝试内部**。回归覆盖生产接线（`open → open_once → migrate`，步进假时钟模拟检查点之间的真实 I/O 消耗）：备份耗尽全部预算 → 备份后检查点失败、trace 无 `tx_busy_ms`、库结构保持 v3；备份消耗部分预算 → 迁移事务的 `busy_timeout` 小于完整预算且不超过备份后剩余，升级正常完成。两处变异验证（删备份后检查、事务改用完整预算）均使对应测试转红。

## 3. 结论与处置

**已证实（隔离对照，证据见 §2）**：失败与「并发首次初始化」绑定——同一进程、同一目录、同一环境下 B/C/D 三组 30/30 通过，只有 A 组失败（HistoryDb 11/30、Cache 2/100）；错误为纯 `SQLITE_READONLY`（extended=8）、立即返回而非锁等待；同一连接内重试无效、重新打开连接恢复。

**推断（未逐层证明）**：具体机理是「本连接在 WAL 切换/迁移窗口内，连接级 journal 状态与库头不一致」。探针证据（`mode=wal` 但写事务立即 readonly、stage_ms≈0、重开连接恢复）支持这一解释，但没有逐层跟踪 SQLite 内部状态，故不作为已证实结论。

**修复**：
- 新增 `crate::retry`：外层**一次创建**绝对 deadline（2s 预算）+ 20ms 间隔；`is_concurrent_init_transient` **只按 `rusqlite::Error` 类型**（ReadOnly/Busy）判定，不匹配消息文本。时钟与 sleep 可注入（`Clock`/`RealClock`）。
- `HistoryDb::open` 与 `Cache::open` 的打开序列、WAL 重试与迁移/初始化事务等待**共用同一个绝对 deadline**（内层只用剩余预算，不得重新 `now + 2s`）——注意：「使用同一预算常量」不等于「共享同一 deadline」，旧实现正是外层与内层各自按 `now + 预算` 计时，每次进入内层都能重新获得完整预算，总时长不受单一上限约束（不是某个固定秒数的叠加）。`migrate` 接收同一个 deadline/clock：读版本前、备份前、**备份后建立迁移事务前**各按剩余预算设 `busy_timeout`；备份耗时不得让后续阶段误用完整预算。
- 每个可能锁等待的 SQLite 阶段（WAL 切换的**每次尝试内部**、读版本、备份前后、迁移/初始化事务前）按**剩余预算**重新设置 `busy_timeout`，不是只在连接建立时设一次完整预算；预算耗尽则该阶段直接失败。
- 睡眠结束后重新检查 deadline，预算耗尽不再启动下一次重试，返回最后一个类型化错误。
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
- ✅ 确定性重试策略测试覆盖类型判定、重试/不重试、预算上限与最后错误；
- ✅ 迁移路径预算传递有覆盖生产接线的确定性回归（备份耗尽/部分消耗预算），并有变异验证。
