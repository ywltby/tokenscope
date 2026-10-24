# 测试临时目录清理实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
>
> 状态：**B1 已合并（2026-10-23，合并 `fde7762`）；B2 已实施待复核（同日，分支 `fix/rust-lockwait-cleanup`：TL03 提交 `338db58`、TL04 版本 `0.5.13` `release: true`）。** 前置 PR #42 经用户批准合并（`4fc46c6`，合并前同步最新 main、修复 Runner 临时目录短名断言两处并通过全部必需检查）。计划分支 `docs/test-temp-cleanup-plan` 已随 PR #44 进入 main。

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
| tests/history_collection.rs | 辅助 :19 | tokenscope-history-collection- | 7 处 `.ok()` | 局部 / 全局查询快照（`summary()` 后） | 忽略 | 仅进程退出 | 后续候选 |
| tests/history_overlap.rs | 辅助 :22 | tokenscope-history-overlap- | 11 处 `.ok()`；:659 `.unwrap()`（codex_compaction_ccs_union_is_independent_of_import_order，严格样例） | 局部 / 全局查询表 | 大多忽略 | 进程退出为主 | 后续候选 |
| tests/history_queries.rs | 辅助 :62 | tokenscope-history-queries- | 7 处 `.ok()` | 局部 / 全局查询表 | 忽略 | 仅进程退出 | 后续候选 |
| tests/history_migration.rs | 辅助 :19 | tokenscope-history-migration- | 2 处 `.ok()` | 局部 | 忽略 | 仅进程退出 | 后续候选 |
| tests/history_storage.rs | 辅助 :23 | tokenscope-history-storage- | 无任何清理 | 局部 | 不适用 | 仅进程退出 | 后续候选 |
| src/report.rs 单元测试 | tmp_dir :2784、:4385 | tokenscope-m4- / tokenscope-b4- | 27 处 `.ok()` | 局部 / 全局查询快照（`summary()` 后） | 忽略 | 仅进程退出 | 后续候选 |
| tests/e2e_claude.rs | hermetic_dir :33 | tokenscope-e2e-claude- | 无任何清理 | 局部 | 不适用 | 仅进程退出 | 后续候选 |
| tests/e2e_codex.rs | hermetic_dir :33 | tokenscope-e2e-codex- | 无任何清理 | 局部 | 不适用 | 仅进程退出 | 后续候选 |

**隔离复现**（各自全新私有根，只跑指定用例，外层等进程退出后读目录清单）：

| 用例 | 命令要点 | 测试自身结果 | 外层核实到的残留 |
| --- | --- | --- | --- |
| 前端「父进程设上限」 | `TMP`/`TEMP`/`TMPDIR` 指向 `tmp-frontend`；`pnpm --dir frontend exec vitest --run scripts/lib/native-scenario.test.mjs -t "父进程设上限"` | 退出码 0，1 通过 / 30 跳过，墙钟 2s | `native-scenario-test-iBXP53/`：evidence.json 40 B + run-scenario.mjs 559 B，共 599 B；node-compile-cache 为 Node 自身产物，归外层所有 |
| Rust 锁等待 | 同法指向 `tmp-rust`；`cargo test --locked -p tokenscope --lib history::tests::migration_lock_wait_uses_resident_timeout -- --exact` | 退出码 0，1 通过，测试体 5.59s | `tokenscope-lockwait-12136-*/`：history.db 102400 B + history.db.backup-v3 94208 B，共 196608 B |

两例都呈现同一区别：**测试自身通过、外层发现残留**——被测行为的断言全绿，泄漏只能由进程退出后的外层文件系统核实，这正是 TL02 外层契约要固化的观察方式。本轮只记录文件名、大小、退出码、耗时与目录边界，未收集真实会话内容，也未写真实 `~/.tokenscope` 或 agent 来源。

### 6.3 TL02 前端补漏与 CI 接线（2026-10-23）

- 新增 `scripts/test_native_scenario_temp_cleanup.py`：外层契约用独立 `TemporaryDirectory` 私有根 + 同级哨兵，向真实 Vitest 子进程只注入 `TMP`/`TEMP`/`TMPDIR`，临时配置（普通对象导出、显式只限定原测试文件）+ JSON 报告核实「确实只执行了该用例」，每次外层调用 30 秒上限；pnpm 可执行文件解析沿用 `pre_commit_checks.py` 的 `shutil.which` + Windows `.cmd` 经 `cmd /c` 方式。三条路径的子进程 stdout/stderr 全部落盘。
- 红灯（未修代码，`qa-artifacts/test-temp-cleanup/b1/contract-red.log`）：3 条全部失败且各归其位——正常路径与中途失败路径红在「进程退出后仍残留 `native-scenario-test-*`」；清理失败路径红在「Vitest 以 0 退出」（旧代码根本没调用删除，未假通过）。中途失败路径先通过注入标记与 JSON 报告核对、最后才红在残留，证明注入与单用例核实机制有效。
- 修复：原用例按计划结构最小包裹 `try { … } finally { fs.rmSync(dir, { recursive: true, force: true }); expect(fs.existsSync(dir)).toBe(false); }`，原步骤与断言逐字保留，未吞错、未追加末尾删除。
- 绿灯：契约 3 条全过（5.8s，`contract-green.log`）；整个 `native-scenario.test.mjs` 31 用例全过（2.9s，`native-scenario-after.log`），系统 Temp 定向前缀残留 0。
- CI：contracts 组新增「原生场景契约临时目录清理回归」步骤（沿用逐条检查退出码写法，保留既有调用）；提交钩子 `SCRIPTS_CHECK` 按 `test_*.py` 全量发现自动纳入本契约，未改选择器。契约本地新增耗时约 6 秒（三次外层 Vitest 调用，单次 30 秒上限内）。
- 快速检查：`check_node_syntax.py` 48 个脚本通过、选择器契约 55 用例通过、`format:check` 与 `typecheck` 干净；完整提交钩子（含 `.github/**` 触发的完整门禁）通过后提交。
- §3 不变量自审：删除只发生在契约自身私有根（哨兵字节核对通过）；原验收语义逐字保留（退出码 7、10000ms 父上限、9000ms 断言、200ms 子看门狗、最小证据内容）；`dir` 创建后全部步骤在 `try` 内；清理失败使测试非零退出且现场保留在外层私有根；不涉及 Rust、全局查询表与查询寿命；红绿均以真实目录消失/存在核实，不以通过数代替。

### 6.4 PR #44 复审补修（2026-10-23）

用户复审确认前端 try/finally 修复成立、远端全绿，但指出两处外层测试架缺陷，复现脚本与结果在忽略目录 `qa-artifacts/test-temp-cleanup/review-44/review_probes.{py,json}`（head `153f564`，可直接运行）：

1. **失败证据被删除**：证据与私有根同目录、清理无条件注册；注入外层断言失败后 stdout/stderr/JSON 报告全部随 teardown 消失（探针实测 `root_exists_after_teardown=false`）。
2. **超时不覆盖后代进程**：`subprocess.run(timeout=…)` 只杀直接进程；后代持有输出管道时无界排空——预算 0.5s 实测 2.102s 才返回，且超时输出未落盘（探针实测 `evidence_files_after_timeout=[]`）。

**补修内容**（对 `scripts/test_native_scenario_temp_cleanup.py` 与 `.github/actions/checks/action.yml`，三条真实路径语义与 30 秒预算不变）：

- 证据保留：新增 `_keep_evidence_on_failure` 装饰器——外层真实失败时先把证据目录整体移入独立目录 `qa-artifacts/test-temp-cleanup/contract-evidence/<时间戳>-<用例>/`（附私有根残留清单，含文件名与字节），再上抛；成功时正常清理。CI contracts 组新增「Upload temp-cleanup contract evidence」失败产物上传步骤（`failure()` 条件、沿用 v7 pin）。
- 超时收尾：外层调用改为 `Popen` + 预算内 `communicate`；超时先整树强杀（Windows `taskkill /F /T`，POSIX 杀进程组，杀失败记录入证据），再有界排空（10 秒宽限），仍超时才直关管道并改用超时异常里的部分输出——输出始终落盘。
- 新增第四条短时反例（合成子进程，预算 1s）：父进程先产出一个继承输出管道的长寿后代（3s）并写 PID 文件再驻留；断言超时路径触发、耗时有界、已捕获输出保留（`parent-ready`）、后代长寿于预算时不得等到其输出、后代 PID 确实死亡、哨兵不变。

**红绿复核**：

- 红（旧机制）：用户探针即提交 `153f564` 上的红灯记录（见 review_probes.json）；本仓复核探针 `qa-artifacts/test-temp-cleanup/b1/review_fix_probes.py` 把新反例跑在旧 `_execute_case` 上——预算 1.0s 实测 3.11s，红在超时处理（「外层调用超过 1.0s 上限」），日志 `review_fix_probes.json` 的 `red_on_old_mechanism`。
- 绿（新机制）：契约 4 条全过（7.0s，`contract-green-v3.log`）；证据保留链复核——注入外层断言失败后证据目录存活且 report/stdout/stderr 三文件齐全（同探针 `evidence_retention`）。
- 期间自纠两处实现错误（反例父程序漏 `import time` 导致父进程秒退、vitest 命令漏 `_argv` 解析导致启动失败），均由本契约自身断言/报错暴露后修正。

**文档勘正与回填**：TL01 登记提交号为 `2540290`；TL02 交付 PR #44，contracts 作业总时长 5m18s，其中「原生场景契约临时目录清理回归」步骤实测 4981ms；审计表 history_collection 与 report 的资源持有者补记全局查询快照（`summary()` 后）。

**复审第二轮（同日，提交 30b9708 仍需补修、暂不建议合并）**：用户确认失败证据保留与上传接线通过复核，但指出三项问题，复现脚本 `qa-artifacts/test-temp-cleanup/review-44/review_probes_round2.{py,json}`：

1. **父进程先退出时超时仍在**：收尾依赖父 PID，taskkill 报父 PID 不存在，后代自然跑完（预算 0.3s + 宽限 0.2s 实测 2.101s 才返回），保存 stdout 为空；先前 `import time` 意外触发的孤儿边界实为必须覆盖项，不能靠让父进程存活回避。
2. **收尾宽限串联扩大预算**：预算耗尽后可串联多次 10 秒收尾，1 秒预算最坏运行到 21 秒，违反「每次调用不超过预算、不增加等待补偿」。
3. **fetch 失败根因未定位**：`awaitWithHostExit` 先置 settled 再调取消回调，取消回调抛 TypeError 时最终仍返回 HostExitError（用户 node 探针证实），此前「abort 落在连接建立阶段」的归因不成立；缺 `error.cause` 与服务端到达证据，隔离复跑通过不构成归因证据。**撤回「根因清楚」的结论，根因记为未定位**；原始失败日志摘录存 `qa-artifacts/test-temp-cleanup/b1/native-instance-flake-original.log`。该用例补齐证据字段属后续候选，不在本批范围。

**第二轮补修**（先红后绿，新增回归 `test_early_parent_exit_orphan_is_bounded_killed_and_recorded`）：

- 统一截止时间：等待阶段 `deadline = 启动 + 预算`；超时后收尾（终止树 + 排空）合计固定上限 `TEARDOWN_CAP_S = 2s`，不随预算缩放、不再串联。排空仍超时则直接采用超时异常里的部分输出，且不再触碰管道对象——`BufferedReader.close()` 会阻塞在幸存读取线程持有的锁上，正是 0.2s 宽限实测拖到 2.101s 的机制。
- 父已退出：Windows 改用 `KILL_ON_JOB_CLOSE` 作业对象（Popen 返回后立即挂入，后代自动入作业），终止作业即连带终止孤儿，收尾不依赖父 PID；POSIX 维持进程组终止；作业不可用时回退 taskkill /T /F。正常完成时关闭作业句柄，顺带终止作业内残留后代。
- 红绿：新回归在 30b9708 实现上红——预算 0.5s、后代 5s 实测 5.09s，超出「预算 + 收尾上限」3.5s 界（`contract-red-round2.log`）；新机制上契约五条路径全绿（6.8s，`contract-green-round3.log`），孤儿场景实测 elapsed 0.504s、stdout 保留 `parent-ready`、孤儿 PID 确认死亡。原「父存活」反例的耗时段言同步收紧为「预算 + 收尾上限 + 1s」。

**复审第三轮（同日，提交 94d6ed4 仍需补修）**：用户确认五条契约本地通过、远端全绿、fetch 根因撤回正确，但指出两项阻断，复现脚本 `qa-artifacts/test-temp-cleanup/review-44/review_probes_round3.{py,json}`：

1. **作业分配仍有启动竞态**：先启动进程再分配作业——作业只自动接纳加入后创建的后代，不追溯收编已有后代。受控时序下（后代先于分配创建）：分配成功、终止未报错、调用返回后后代仍存活、stdout 为空，实测 2.321s。
2. **总预算仍未统一**：截止时间只约束第一次等待，之后仍追加 2 秒；回退路径 taskkill 与排空各再用一次 2 秒。应在原总预算内部预留收尾时间，所有等待共用同一剩余时间；若需扩大预算须先修订计划。
3. （建议）stdout/stderr 直接写入私有日志文件，超时后读取已有内容，不再依赖管道 EOF。

**第三轮补修**（先红后绿，新增确定性回归 `test_job_enrollment_precedes_descendant_creation`）：

- 消除启动竞态：Windows 改为挂起创建（`CREATE_SUSPENDED`）→ 挂入 KILL_ON_JOB_CLOSE 作业 → `ResumeThread` 恢复——作业先于任何目标代码就位，后代必然入作业；主线程句柄经工具帮助快照枚举取得（挂起进程只有主线程一个线程，出现多个线程即失败，不误恢复）。作业创建/分配/恢复任一步失败即**显式失败，不降级**为无法处理孤儿的 taskkill；失败路径先终止作业再清理挂起进程。
- 原预算内统一截止：新增 `TEARDOWN_RESERVE_S = 0.5s`（`min(0.5, 预算/4)`）在首次等待前从预算中预留；超时后终止树 + 等待退出共用同一截止时间（启动 + 预算）的剩余部分；树终止仍失败时不追加任何等待。删除 `TEARDOWN_CAP_S` 与 taskkill 回退。
- 输出直写文件：stdout/stderr 由子进程直接写入私有日志文件（句柄继承），不再创建管道——不依赖 EOF，超时后读取已有内容即可；这也移除了「管道被后代持有时排空无界」与「BufferedReader.close() 锁阻塞」两类问题的载体。
- 回归改写：受控时序回归断言「分配时 PID 文件必然未出现（挂起创建）」且逃逸后代被作业关闭收割；父先退出回归改为正常完成路径（父退出后调用应在预算内完成、孤儿被 KILL_ON_JOB_CLOSE 收割）；挂起超时回归保留（预算内父进程不退出）。三条反例的耗时段言统一为「原预算 + 1s 容差」。
- 红绿：受控时序回归在 94d6ed4 实现上确定性红——分配等待 0.3s 内 PID 文件已出现（旧实现子进程先行运行），「分配先于目标代码」断言失败（`contract-red-round3b.log`）；新机制六条路径全绿（8.3s，`contract-green-round4.log`）。实测：孤儿场景 completed 0.106s（预算 0.5s、孤儿确认死亡）、挂起超时场景 timeout 0.755s（预算 1.0s、stdout 保留 parent-ready、后代确认死亡）——所有等待均在原预算内收敛。

### 6.5 实施后填写（收尾补记，2026-10-23 main 直接提交）

- PR #44 合并后 main 三条运行全成功：[CI](https://github.com/ywltby/tokenscope/actions/runs/39989795279)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39989795088)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39989795109)（版本不变，仅 gate，六下游 skipped，未建标签）。PR 最终轮 CI 16 项检查全过。
- 已合并分支 `fix/frontend-test-temp-cleanup`、计划分支 `docs/test-temp-cleanup-plan` 删除（内容已全部进入 main）。
- B2 仍未实施：等 PR #42 的用户决定与同步；本计划因此保持 active，不归档。
- PR #42 同步准备（2026-10-23，用户授权合并前准备、不构成合并批准）：分支 `fix/rust-test-temp-cleanup` 合并 origin/main `c443ffc`（无冲突；`checks/action.yml` 两边接线并存——Rust 包装与 B1 契约步骤都在），随本记录提交以 full 模式通过完整提交钩子后推送；必需检查结果见 PR 页。
- B2 勘误（2026-10-23 用户复核）：PR #46 首轮 CI 的 native-ui 失败（`ui-source-paths`）与 TL04 门禁超限，此前分别记为「环境级启动失败（CDP 全程 ECONNREFUSED）」与「既有用例的调度敏感」，**两处归因均撤回**：(a) native-ui 首轮证据为 CDP 端点约 5.3s 起可用、此后 20s 等待期内页面 target 始终仅 `about:blank`；`launches[]` 中宿主退出码 -1 出现在失败清理期间（测试架终止），不构成应用自行崩溃的证据——记为「页面 target 未出现，根因未定位」，第二次运行通过；此前「全程 ECONNREFUSED」只对应启动初期前几条连接尝试，与完整日志不符。(b) 1.199s 超限为事实，但无调度或负载证据支持「并行负载」归因，保留为假设；重跑通过仅证明后续运行成功。两次重跑均未放宽任何断言，代码范围未扩大；失败证据存 `qa-artifacts/test-temp-cleanup/b2/native-ui-failure/`。
- §3 不变量自审补正（同上复核）：第 2 条（原验收语义保留）对 B2 成立——原错误分类断言（`is_concurrent_init_transient`）、elapsed 上下界与 5000ms 预算断言逐字保留，仅增加 `drop` 与清理结果断言（对照 `338db58`）；第 4 条（清理失败不能伪装通过）对 B2 成立——忽略错误的删除改为 `assert!(cleanup.is_ok())` 与存在性核对，红灯轮（code 32）证明删除失败会使用例非零退出并定位目录；第 3 条为前端专属，本批无前端改动。

| 任务/批次 | 受测提交及作用 | 红灯断言与绿灯 | 命令/耗时/证据位置 | PR 与 main 运行 | 不变量自审/偏离/剩余范围 |
| --- | --- | --- | --- | --- | --- |
| TL01 | 2540290（审计登记，无业务/测试代码改动） | 不适用；复现结论为「通过但有残留」（见 §6.2） | rg 扫描；前端定向 2s；Rust 定向 6s（测试体 5.59s）；日志 `qa-artifacts/test-temp-cleanup/b1/` | 随 B1 PR 提交 | 只登记未治理；其余夹具为后续候选，不在本轮宣称已治理 |
| TL02 / B1 | 红：d7c307f（未修）；绿：153f564 及三轮复审补修（e0d276c 终态） | 红：3 路径各因残留/假通过而红；复审各轮新回归在当轮实现上红（孤儿超界、分配晚于目标代码）；绿：契约 6 过（含超时、父先退出、受控时序反例）+ 原文件 31 过；用户复核本地 7.65s 通过并授权合并 | 契约约 8.3s；原文件 2.9s；日志 `qa-artifacts/test-temp-cleanup/b1/` | [PR #44](https://github.com/ywltby/tokenscope/pull/44) 普通合并 `fde7762`（含注释勘正 6e3ddeb）；main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39989795279)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39989795088)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39989795109) 全成功；版本不变，Release 仅 gate（23s），六下游 skipped | 自审见 §6.3；三轮复审补修见 §6.4；审计候选未动 |
| TL03 / B2 | 红：338db58 前一提交（严格断言、无 drop）；绿：338db58 | 红：经 run_rust_tests 入口，`夹具清理失败 code 32`（活连接持文件），wrapper 保留现场；绿：drop(holder) 后 1 passed（5.64s）；原错误分类/上下界/预算断言保留 | 入口命令每轮约 6s；双层验证：wrapper 层「清理已核实，临时根不存在」＋直接 Cargo 私有根 `tokenscope-lockwait-*` 残留 0；日志 `qa-artifacts/test-temp-cleanup/b2/` | 随 B2 PR | 前置 PR #42 已由用户批准合并（`4fc46c6`，含同步 main 与 Runner 短名断言修复）；只释放本用例自己的连接，未动全局查询表与生产代码；不变量 2/4 成立（依据见 §6.5 勘误） |
| TL04 / B2 | c02ce60 | 不适用 | bump patch → 0.5.13；`--check` = `release: true`；v0.5.12 已在基线登记表（本机 check 的网络异常不构成缺失证据，CI 权威校验） | 随 B2 PR | 版本单独提交；门禁一次超限**已证实但原因未定位**（`latest_history_opens_without_writer_lock` 实测 1.199s > 1s 阈值；「并行负载/调度抖动」仅为假设，无调度或负载直接证据；重跑通过仅证明后续运行成功，未放宽断言；c02ce60 提交正文中的「已归因/调度敏感」表述以本勘误为准） |
| TL05 | B1 部分完成：PR #44 获用户合并决定后普通合并；main 的 CI、Native UI E2E、Release 全成功（Release 仅 gate） | 不适用 | 见 TL02/B1 行的 main 运行链接 | [PR #44](https://github.com/ywltby/tokenscope/pull/44) 已合并 | B1 交付闭合；B2/收口未完成——计划保持 active；不改变 N01–N16 状态 |
