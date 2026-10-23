# 测试临时目录清理实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 状态：**待用户复核，尚未实施。** 用户 2026-10-23 授权开分支编写计划；该授权不代表确认本计划的实施范围或合并 PR #42。计划分支：`docs/test-temp-cleanup-plan`。

**Goal:** 修复已证实的 Vitest 临时目录遗漏和 Rust 锁等待用例句柄释放顺序问题，增加能发现“测试通过但目录残留”的回归，并明确 Rust 进程级收尾的适用边界。

**Architecture:** 前端在创建临时目录的用例内用 `try/finally` 收尾，由隔离外层契约观察真实 Vitest 子进程退出后的文件系统。Rust 先释放本用例的连接，再严格核实删除；保留 PR #42 的进程退出后收尾作为另一层保障。其余 Rust 夹具逐文件审计登记，本轮不统一引入守卫或改查询生命周期 API。

**Tech Stack:** Node.js `fs`、Vitest、Python `unittest`、Rust / rusqlite、现有 GitHub Actions 门禁。沿用现有依赖。

## 1. 已核实事实与证据边界

核查基线为 main `fafe1d6`（2026-10-23），应用版本 `0.5.12`。执行前必须重新 fetch，复核下表；路径可因新增提交移行，以函数名定位。

| 项目 | 已核实事实 | 证据 |
| --- | --- | --- |
| 前端遗漏 | `frontend/scripts/lib/native-scenario.test.mjs:403` 创建 `native-scenario-test-*`，该用例没有清理目录 | 隔离运行指定用例：1 通过、30 跳过；退出后留下 `evidence.json` 40 字节和 `run-scenario.mjs` 559 字节 |
| 子进程退出 | 子进程在 200ms 看门狗回调写最小证据后主动 `process.exit(7)`；父进程 10000ms / SIGKILL 是兜底 | 同一用例断言 `child.error` 为空、退出码 7、耗时小于 9000ms，均通过；不能描述成“杀进程后再写证据” |
| Rust 成功也残留 | `history.rs::tests::migration_lock_wait_uses_resident_timeout` 执行 ROLLBACK 后，`holder` 仍存活便删除目录，删除错误被忽略 | 隔离运行 1 项通过，约 5.60s；进程退出后仍留 `history.db` 与 `history.db.backup-v3` |
| 守卫的局限 | `TempDir` 的析构忽略删除错误；文件/目录句柄需要先释放，可用 `close()` 检查删除结果 | 已核对本机 tempfile 3.27.0 源码及[官方说明](https://docs.rs/tempfile/3.27.0/tempfile/struct.TempDir.html#resource-leaking)；全局查询表中的快照不由局部目录守卫负责释放 |
| 已有正确样例 | `history_overlap.rs::codex_compaction_ccs_union_is_independent_of_import_order` 先 drop 本地库、仅回收自身查询，再用 `remove_dir_all(...).unwrap()` 核实清理 | 可作为审计对照；不能据此修改生产查询 API 或清空全局查询表 |
| CI 前端依赖 | `.github/actions/setup/action.yml` 对各组统一安装锁定的前端依赖 | contracts 可直接运行本仓 Vitest，无需增加下载步骤或修改 setup |
| PR #42 | `fix/rust-test-temp-cleanup` / `7c24193` 提供 Rust 独立临时根、子进程退出后清理、失败现场保留及钩子/CI 接线；编写本计划时 PR 仍 open | [PR #42](https://github.com/ywltby/tokenscope/pull/42)。它只包装 Cargo 测试，不覆盖 Vitest；不能借用其结果宣布全仓零残留 |

本机核查日志位于忽略目录 `qa-artifacts/temp-leak-review/native-scenario-before.log`、`rust-passing-test-before.log`，未入库。复现的两个私有根也在该证据目录内；自动审批曾拒绝删除它们，现场保留，本计划不授权绕过该拒绝。未来执行必须建立新隔离根，不依赖这些本机文件才能运行回归。

此前报告的 181 个前端目录与数万个 Rust 目录是历史观察。本次系统 Temp 中该前端前缀为 0；可以证实代码仍会泄漏，不能由历史目录数推导准确测试次数、失败次数或把全部残留归因于 panic。

## 2. 范围、批次与依赖

| 批次 | 任务 | 文件范围 | 版本与前置项 |
| --- | --- | --- | --- |
| B1 前端补漏 | TL01、TL02 | 本计划执行记录；`frontend/scripts/lib/native-scenario.test.mjs`；新增 `scripts/test_native_scenario_temp_cleanup.py`；`.github/actions/checks/action.yml` 的 contracts 接线 | 从最新 main 单独开实现分支；不依赖 PR #42 合并；`release: false` |
| B2 Rust 局部修复 | TL03、TL04 | `crates/tokenscope-core/src/history.rs` 的上述测试；版本工具生成的版本/锁/声明文件，必要的上一版基线登记 | PR #42 已经用户决定合并并同步进最新 main 后执行。源码路径中的测试改动按当前规则属于产品输入，单独 PR 递增 patch，`release: true` |
| 收口 | TL05 | 本计划、`docs/plans/README.md`、整合计划中的本专项入口 | 如单开回填 PR，仅文档则 `release: false`；不改变 N01–N16 状态 |

本计划的实施交付是两个已复现漏点及其回归、PR #42 收尾接线的复核、审计登记。`history_collection.rs`、`history_overlap.rs`、`report.rs`、`e2e_claude.rs`、`e2e_codex.rs` 中其他夹具的批量改造不在本轮实施范围；审计发现记为后续候选，不能在收口时写成已治理。

不新增 `tempfile` dev-dependency，不批量替换 `PathBuf`，不改生产查询表、查询 TTL、统计口径、持久格式、价格或网络行为。历史 Temp、旧 worktree、真实 `~/.tokenscope` 和各 agent 数据目录均不属于清理目标。

## 3. 必须保持的不变量

1. **删除范围由创建者拥有。** 回归只删除自身新建的私有根；不按系统 Temp 的公共前缀批量删除，不进入外部目录联接；外层契约必须放置同级哨兵并确认其字节不变。
2. **原验收语义保留。** 前端的退出码 7、最小证据内容、10000ms 父上限、9000ms 断言和 200ms 子看门狗均不变；Rust 原错误分类与 5000ms 预算及上下界断言不变。只增加清理行为和清理结果断言。
3. **正常和异常路径都进入前端收尾。** `dir` 创建后，脚本生成、子进程调用、证据读取和全部原断言都包含在 `try` 中；任何一步抛错仍进入 `finally`。创建失败时不得删除不属于自己的路径。
4. **清理失败不能伪装通过。** 删除错误必须使测试非零退出并能定位目录；删除后核对不存在。不得用 catch 空体、`.ok()`、忽略错误或延长等待把残留掩盖掉。
5. **先释放资源再删。** Rust 仅释放当前用例自己的句柄；本轮不清空全局注册表，不通过改变查询寿命或让其他并行测试过期来获得删除成功。
6. **进程收尾与用例收尾边界清楚。** 保留 PR #42 的失败/中断留现场策略；局部清理会减少局部现场，不要求它在异常时同时“删除并保留”同一目录。断言失败的 stdout/stderr 必须由外层证据保存。
7. **证据与断言真实。** 先确认新增回归在未修实现上因断言而红，再实现转绿；注入失败只在合成子进程/临时配置中生效，测试通过数不能代替目录消失的核实。

## 4. 逐项实施

### TL01 复核事实并登记已知夹具

**修改：** 本计划 §6，不改业务或测试实现。

1. Fetch 并记录 main、PR #42 的实际 head/合并状态。读取 CLAUDE.md、此计划全文及目标函数；事实变更时先报告差异，不沿用过时行号或重复实现 PR #42。
2. 用 `rg` 查找上述 Rust 文件及前端 `mkdtempSync`、`remove_dir_all`、`rmSync`、`std::env::temp_dir`。登记表列：文件/函数、创建位置、清理位置、资源持有者、是否忽略错误、当前用例/进程层保障、本轮处理或后续候选。
3. 在新私有 TMP/TEMP/TMPDIR 中复现两个已知用例，外层等待进程结束后读取目录清单，日志与计数写入新的 `qa-artifacts/test-temp-cleanup/<批次>/`。只运行指定用例，不扫描真实来源、不跑 ignored 真实性能测试。
4. 将“测试自身通过、外层发现残留”的区别写进 §6。只记录文件名、大小、退出码、耗时和目录边界，不收集真实会话内容。
5. 单独提交审计记录，提交钩子通过后 push；不得借该提交声明实现完成。

### TL02 前端先红后绿与 CI 接线（B1）

**新增：** `scripts/test_native_scenario_temp_cleanup.py`。

**修改：** `frontend/scripts/lib/native-scenario.test.mjs`、`.github/actions/checks/action.yml`。

1. 先写 Python 外层契约：每个子用例用独立 `TemporaryDirectory`，只向实际 Vitest 子进程注入三个临时环境变量，保留父环境和同级哨兵。复用本仓的 pnpm 可执行文件解析方式；每次外层同步调用有 30 秒上限，超时判失败，不能增加等待补偿或跳过缺失工具。调用本仓锁定的 Vitest，只选原用例，不在子进程中执行外层契约自身：

   ```powershell
   pnpm --dir frontend exec vitest --run scripts/lib/native-scenario.test.mjs -t "父进程设上限"
   ```

   外层调用时增加 JSON reporter，把报告写入其私有证据目录。必须核对原用例名称确实出现且只有该用例执行：正常路径为 passed，注入失败路径为 failed；全跳过、报告缺失、配置加载失败均使契约失败。不能只依据退出码和空目录把“实际没跑用例”判成清理成功。

2. 最少覆盖三条实际执行路径，子进程 stdout/stderr 都保存：

   | 路径 | 注入方式 | 外层断言 |
   | --- | --- | --- |
   | 正常通过 | 不注入错误，真实生成并运行原测试的子脚本 | Vitest 退出 0，私有根无 `native-scenario-test-*`，同级哨兵不变 |
   | 测试中途失败 | 在临时 Vitest setup 中，仅对该 `evidence.json` 的 `fs.readFileSync` 注入带固定标记的抛错；其他读取透传 | Vitest 非零、日志含该标记、目录仍被清理；不能把配置/依赖失败当成有效反例 |
   | 清理失败 | setup 仅对当前测试目录的 `fs.rmSync` 注入带固定标记的删除错误 | Vitest 非零且输出删除错误/目录；残留只在外层私有根，父契约待子进程退出后负责收尾 |

   临时 config/setup 均写入外层私有根；配置显式限定原测试文件，使用项目已安装的 Vitest，不下载依赖。不得修改仓库原断言或把生成文件留在源码目录。

3. 在尚未加 finally 的原代码上运行：

   ```powershell
   python -B -m unittest discover -s scripts -p "test_native_scenario_temp_cleanup.py" -v
   ```

   预期正常路径和中途失败路径因目录残留而断言红；清理失败路径也不能因旧代码根本没调用删除而假通过。红灯必须指向上述行为，非命令不存在、模块导入失败或网络错误。

4. 对原用例做最小包裹，原步骤与断言原样保留。`dir` 创建之后立即进入以下结构：

   ```js
   const dir = fs.mkdtempSync(path.join(os.tmpdir(), "native-scenario-test-"));
   try {
     // 原来的脚本生成、spawnSync、证据读取和全部断言放在这里。
   } finally {
     fs.rmSync(dir, { recursive: true, force: true });
     expect(fs.existsSync(dir)).toBe(false);
   }
   ```

   若为删除错误补充上下文，只能增加目录和 cause，不得吞错。不得在测试末尾单独追加删除来替代 finally。

5. 外层契约转绿，再跑整个原文件：

   ```powershell
   pnpm --dir frontend exec vitest --run scripts/lib/native-scenario.test.mjs
   ```

   所有原用例通过且定向前缀不残留；不要求隔离根完全空，因为 Node 编译缓存等由外层拥有。

6. 在 CI 的 contracts 组按现有逐条检查退出码的写法追加新 Python 文件调用；保留其他调用。该组已有锁定的前端依赖，直接复用，不修改 setup。
7. 运行新契约及选择器相关契约，记录新增 CI 耗时；完整提交钩子通过后，用 `fix(test): 清理原生场景契约的临时目录` 提交（中文正文附归因、红绿日志），串行 push。B1 只含本节文件与执行记录，`--check` 必须为 `release: false`。

### TL03 释放 Rust 锁等待用例的连接（B2）

**修改：** `crates/tokenscope-core/src/history.rs` 中 `migration_lock_wait_uses_resident_timeout`，其他函数不改。

1. B1 交付后，从最新 main 新开分支，确认 PR #42 已由用户决定合并，独立临时根入口与钩子/CI 接线都已存在。若未合并，停止 B2，记录前置项；不能借本计划自行合并或复制一套入口。
2. 先把该用例末尾忽略错误的删除改为严格核对，**先不增加 `drop(holder)`**：

   ```rust
   let cleanup = std::fs::remove_dir_all(&dir);
   assert!(cleanup.is_ok(), "夹具清理失败 {}: {cleanup:?}", dir.display());
   assert!(!dir.exists(), "夹具目录仍存在: {}", dir.display());
   ```

   这是追加清理结果断言；原来的错误分类、elapsed 上下界及预算断言全部保留。
3. 在 Windows 下经 PR #42 入口运行，确认因活连接导致新清理断言红，并记录保留现场：

   ```powershell
   python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --lib history::tests::migration_lock_wait_uses_resident_timeout -- --exact
   ```

   若此环境删除仍可成功，不能伪报红；需用独立外层文件系统断言重新定位或报告事实差异，不改平台行为和等待窗口凑结果。
4. 在 `holder.execute_batch("ROLLBACK").unwrap()` 后、严格删除前加入 `drop(holder)`，按同一命令复验转绿。无需 `TempDir` 或新依赖。
5. 以外层私有根包裹直接 Cargo 定向运行，再次确认**用例内**已无 `tokenscope-lockwait-*`；同时核对外层入口成功后的临时根消失。两层验证分别记录，不能只看外层把旧残留一起删掉。
6. 保存失败和复验日志，完整钩子通过后单独提交，中文正文注明“ROLLBACK 释放事务锁但连接仍持有文件，先 drop 再核实删除”。不得修改生产打开/迁移、忙等待、重试或查询实现。

### TL04 B2 取号与门禁

**修改：** 版本工具生成的四份版本配置、根 `Cargo.lock`、`release-change.json`；必要时登记上一版升级基线。

1. Fetch 最新 main。执行时若基线不是 0.5.12，按实际最新版本计算；本计划不预占号。同一个产品 PR 只递增一次 patch。
2. 如上一发布尚未登记，先用 `upgrade_baselines.py add --tag v<上一版>` 登记并核对原资产；已登记则保留现有记录。
3. 用 `bump_version.py --bump patch --summary "修复 Rust 锁等待测试的临时目录收尾"` 生成版本。若实施时仍从 0.5.12 起步，预期为 0.5.13；以工具和最新 main 为准，不手改单个版本字段。
4. `python -B scripts/bump_version.py --check --base-ref origin/main` 必须返回 `release: true`。不能为了保持不发布而放宽路径分类。
5. 版本单独提交，正文说明源码路径内测试仍触发当前产品输入规则；禁止混入依赖更新。完成本节前不把 B2 PR 标为可合并。

### TL05 批次交付与收口

1. 每个批次开普通 PR，正文列出 §3 七条不变量自审、红绿证据、修改范围、版本判定和仍未治理的夹具范围。**合并均由用户决定。**
2. 等待 gate、native-ui 和实际触发的 Install test 全部成功；不以重跑代替失败归因。原生失败先读取日志与 `evidence.json`，包括 `launches[]`，区分装配与产品问题。
3. 获得合并决定后使用普通 merge commit；确认 main 的 CI、Native UI E2E、Release。B1/文档应只跑 Release gate，六下游 skipped；B2 应完成对应新版本发布及现行安装/升级矩阵，不写死历史矩阵组数。
4. 回填 §6 的提交、PR、运行链接、命令、耗时与证据位置；中文文档回读。计划自身范围全部完成后按 CLAUDE.md 归档，README 和整合入口同步；尚未实施的审计候选明确保留其状态，不宣称全仓零泄漏，不重开或关闭已有 N 编号。

## 5. 验收与停止条件

- B1 的正常、中途失败、删除失败三条外层契约满足预期；整个 `native-scenario.test.mjs` 原验收仍通过。新回归已接入 CI，并记录实际耗时。
- B2 在原资源顺序上有真实红灯，加 `drop(holder)` 后转绿；原时间窗口与错误类型断言都保留，局部清理与进程清理分别有文件系统证据。
- 两批均完成当前必需检查和实际触发的安装检查；合并和 main 验收按用户决定完成后才能标为交付。
- 发现隔离根之外的写入/删除、真实用户目录变化，立即停止相关运行并保留证据。
- 若需修改生产生命周期/查询 API、引入依赖、放宽已有断言或等待、清空全局查询表、改变必需检查或版本分类，停止并先修订计划。不能为通过清理断言改业务语义。
- PR #42 前置项未满足仅阻止 B2，不阻止 B1；历史证据目录的删除拒绝不通过新脚本绕过，也不作为实现上述修复的前置项。

## 6. 执行记录

### 6.1 编写计划时的核查（2026-10-23）

- 基线 main `fafe1d6`，工作区干净；PR #42 仍 open，head `7c24193`，尚未合并进 main。
- 前端隔离复现：指定 Vitest 用例 1 通过 / 30 跳过，残留两个文件共 599 字节；日志 `qa-artifacts/temp-leak-review/native-scenario-before.log`。
- Rust 隔离复现：锁等待用例 1 通过，约 5.60s，残留主库与备份；日志 `qa-artifacts/temp-leak-review/rust-passing-test-before.log`。
- 两次均未写真实 `~/.tokenscope` 或 agent 来源；本机复现现场留在忽略的 QA 目录，不入库。自动审批拒绝删除本次两个私有根，仅返回 `blocked by policy`，未提供更具体原因；未尝试其他删除途径。
- 以上只证明漏点，不是新增回归的红绿记录；TL01–TL05 尚未执行。

### 6.2 TL01 审计登记与隔离复现（2026-10-23，B1 实现分支）

- 分支 `fix/frontend-test-temp-cleanup`（基于 d7c307f）。复核：main 仍为 `fafe1d6`，PR #42 仍 open（head `7c24193`，mergeable）；§1 两处漏点行号未变（`native-scenario.test.mjs:403`、`history.rs:3248`），无事实漂移。
- 复现日志与私有根在忽略目录 `qa-artifacts/test-temp-cleanup/b1/`（`native-scenario-before.log`、`rust-passing-test-before.log`；私有根 `tmp-frontend/`、`tmp-rust/` 就地保留），不入库。

**夹具审计登记**（rg 扫描 `std::env::temp_dir` / `mkdtempSync` / `rmSync` / `remove_dir_all`；「忽略」指删除错误被丢弃、失败不可见）：

| 文件 | 创建 | 前缀 | 清理 | 资源持有者 | 错误处理 | 现有保障 | 处置 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| frontend/scripts/lib/native-scenario.test.mjs（父进程设上限用例） | :403 | native-scenario-test- | 无 | 无句柄残留；根本没有清理调用 | 不适用 | 仅进程退出 | **B1 修复（TL02）** |
| 同文件（超时证据字段用例） | :137 | native-scenario-tail-*.json | :151 finally | 局部 | rmSync force（ENOENT 属预期） | 用例 finally | 已治理样例，不动 |
| frontend/scripts/lib/acceptance-io.test.mjs | :35 / :109 / :124 | tokenscope-ac02- | 各用例 finally | 局部 | 抛错可见 | 用例 finally | 已治理样例，不动 |
| frontend/scripts/lib/native-pricing-observation.test.mjs | :56 | tokenscope-pricing-path- | finally unlink+rmdir | 局部 | 抛错可见 | 用例 finally | 已治理样例，不动 |
| src/history.rs::migration_lock_wait_uses_resident_timeout | :3249 | tokenscope-lockwait- | :3257 预清理、:3268 收尾 | `holder` Connection（ROLLBACK 后仍持文件句柄） | `let _ =` 丢弃 | 仅进程退出 | **B2 修复（TL03，前置 PR #42）** |
| src/history.rs::concurrent_first_initialization_both_succeed | :3293 | tokenscope-first-init- | :3297 / :3317 | 局部连接 | `let _ =` | 仅进程退出 | 后续候选 |
| src/history.rs 其余单元测试 | unique_temp_dir :3354 | tokenscope-{tag}- | :3378 / :3395 / :3418 / :3457 为 `.unwrap()`；:3508 / :3541 为 `let _ =` | 局部连接 / 全局查询表 | 严格与忽略并存 | 严格处用例层，忽略处进程退出 | 后续候选 |
| tests/history_collection.rs | 辅助 :19 | tokenscope-history-collection- | 7 处 `.ok()` | 局部 | 忽略 | 仅进程退出 | 后续候选 |
| tests/history_overlap.rs | 辅助 :22 | tokenscope-history-overlap- | 11 处 `.ok()`；:659 `.unwrap()`（codex_compaction_ccs_union_is_independent_of_import_order，严格样例） | 局部 / 全局查询表 | 大多忽略 | 进程退出为主 | 后续候选 |
| tests/history_queries.rs | 辅助 :62 | tokenscope-history-queries- | 7 处 `.ok()` | 局部 / 全局查询表 | 忽略 | 仅进程退出 | 后续候选 |
| tests/history_migration.rs | 辅助 :19 | tokenscope-history-migration- | 2 处 `.ok()` | 局部 | 忽略 | 仅进程退出 | 后续候选 |
| tests/history_storage.rs | 辅助 :23 | tokenscope-history-storage- | 无任何清理 | 局部 | 不适用 | 仅进程退出 | 后续候选 |
| src/report.rs 单元测试 | tmp_dir :2784、:4385 | tokenscope-m4- / tokenscope-b4- | 27 处 `.ok()` | 局部 | 忽略 | 仅进程退出 | 后续候选 |
| tests/e2e_claude.rs | hermetic_dir :33 | tokenscope-e2e-claude- | 无任何清理 | 局部 | 不适用 | 仅进程退出 | 后续候选 |
| tests/e2e_codex.rs | hermetic_dir :33 | tokenscope-e2e-codex- | 无任何清理 | 局部 | 不适用 | 仅进程退出 | 后续候选 |

**隔离复现**（各自全新私有根，只跑指定用例，外层等进程退出后读目录清单）：

| 用例 | 命令要点 | 测试自身结果 | 外层核实到的残留 |
| --- | --- | --- | --- |
| 前端「父进程设上限」 | `TMP`/`TEMP`/`TMPDIR` 指向 `tmp-frontend`；`pnpm --dir frontend exec vitest --run scripts/lib/native-scenario.test.mjs -t "父进程设上限"` | 退出码 0，1 通过 / 30 跳过，墙钟 2s | `native-scenario-test-iBXP53/`：evidence.json 40 B + run-scenario.mjs 559 B，共 599 B；node-compile-cache 为 Node 自身产物，归外层所有 |
| Rust 锁等待 | 同法指向 `tmp-rust`；`cargo test --locked -p tokenscope --lib history::tests::migration_lock_wait_uses_resident_timeout -- --exact` | 退出码 0，1 通过，测试体 5.59s | `tokenscope-lockwait-12136-*/`：history.db 102400 B + history.db.backup-v3 94208 B，共 196608 B |

两例都呈现同一区别：**测试自身通过、外层发现残留**——被测行为的断言全绿，泄漏只能由进程退出后的外层文件系统核实，这正是 TL02 外层契约要固化的观察方式。本轮只记录文件名、大小、退出码、耗时与目录边界，未收集真实会话内容，也未写真实 `~/.tokenscope` 或 agent 来源。

### 6.3 实施后填写

| 任务/批次 | 受测提交及作用 | 红灯断言与绿灯 | 命令/耗时/证据位置 | PR 与 main 运行 | 不变量自审/偏离/剩余范围 |
| --- | --- | --- | --- | --- | --- |
| TL01 | d7c307f（审计登记，无业务/测试代码改动） | 不适用；复现结论为「通过但有残留」（见 §6.2） | rg 扫描；前端定向 2s；Rust 定向 6s（测试体 5.59s）；日志 `qa-artifacts/test-temp-cleanup/b1/` | 随 B1 PR 提交 | 只登记未治理；其余夹具为后续候选，不在本轮宣称已治理 |
| TL02 / B1 | 未执行 | 未执行 | 未执行 | 未建 | 待用户确认计划 |
| TL03 / B2 | 未执行 | 未执行 | 未执行 | 未建 | PR #42 前置项待决定 |
| TL04 / B2 | 未执行 | 未执行 | 未执行 | 未建 | 不预占版本 |
| TL05 | 未执行 | 不适用 | 未执行 | 未建 | 不改变 N01–N16 状态 |
