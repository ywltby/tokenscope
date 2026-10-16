# 发布流程去掉重复的完整测试

> 状态：**待用户确认**（2026-10-17）。执行方补充（`3c10bd9`）经[审阅第 21 节](../../reviews/2026-10-15-source-discovery-plan-review.md#21-发布去重计划复核执行方补充-3c10bd9)复核：接受风险表述与版本登记的修正，删去过度防御的步骤，§6 重写。
> 交付：独立小 PR，patch（只改发布工作流、一条契约测试与文档，不改程序）。**在当前分支 `docs/release-gate-dedup-plan` 上实施**：它已带本计划与第二批计划归档（`fcc3578`），从 main 另建分支会丢掉归档提交。排在 [PR2 扫描反馈](2026-10-15-collection-status.md)之前。

## 1. 现状（2026-10-17 核实）

`release.yml` 的 `gate` 作业做三件事：

| 步骤 | 依赖 | 是否与 PR 重复 |
| --- | --- | --- |
| `bump_version.py --check --event-file`：本次合入恰好递增一次版本 | git、Python | 否：校验的是 push 事件，PR 上校验不到合并后的这一跳 |
| `release_metadata.py`：版本一致、标签与提交身份，产出 `tag`/`sha`/`version` 等输出 | git、Python | 否：后续 6 个作业都读它的输出 |
| `./.github/actions/setup` + `./.github/actions/checks`：完整质量门禁 | Rust、pnpm、Node | **是**：与 `ci.yml` 的 `gate` 是同一个复合动作 |

完整门禁在 PR 上已经必须通过，main 上的代码不会与 PR 测过的不同：

- 规则集 `protect-main`：必须走 PR；必需检查 `gate`、`native-ui`；`strict_required_status_checks_policy: true`（分支必须与最新 main 同步）；只允许普通合并；`bypass_actors` 为空。
- 因此合并提交的代码树就是 PR 上 `gate` 测过的那棵树。
- 标签触发（`refs/tags/v*`）时，`release_metadata.py` 要求该提交是 `origin/main` 的祖先，同样经过了 PR 门禁。

代价（v0.3.3，Release run 38916727732）：gate 18 分钟，构建 11 分钟，安装与升级约 1.5 分钟，发布约 0.5 分钟，总计约 31 分钟；gate 占 6 成，而且整段都在发布的关键路径上。

## 2. 改什么

### R01 Release 的 gate 只保留身份校验

`.github/workflows/release.yml` 的 `gate` 作业：

- 删除 `./.github/actions/setup` 与 `./.github/actions/checks` 两步，改为 `actions/setup-python`（与其他作业相同的钉住版本，3.12）。
- 保留 checkout（`fetch-depth: 0`，两个校验脚本都要用历史）、`bump_version.py --check`、`release_metadata.py` 与全部 `outputs`。
- `timeout-minutes` 45 → 10。
- 作业名保持 `gate`：6 个下游作业和两份契约测试都用 `needs.gate.outputs.*`，改名只增加改动面。在作业上方加一行注释：完整测试由 PR 必需检查保证，这里只校验发布身份。

其余作业（build、install-test、plan-upgrades、upgrade-test、upgrade-chain、publish）不动。PR 已经做安装与单跳升级测试，但 Release 重新构建的安装包仍须独立验收，且连续升级链是 Release 的发布前门禁；不能用 PR 工件代替本次发布产物的证据。

### R02 一条契约测试

在 `scripts/test_build_paths.py` 的 `CiLayoutCase` 加一条：Release 的 `gate` 作业含 `bump_version.py --check` 与 `release_metadata.py`，且不含 `actions/checks`；`ci.yml` 的 `gate` 仍使用 `./.github/actions/checks`。目的是防止日后删测试时连身份校验一起删掉，或者把 CI 的完整门禁误删。

### R03 文档

- `CLAUDE.md`「发布与升级」：写明 Release 的 gate 只做版本与身份校验，完整测试由 PR 必需检查（`gate`、`native-ui`，要求与 main 同步）保证；main 上的 CI 与 Native UI E2E 照常运行，与发布并行。
- `docs/plans/README.md`「现在执行什么」加本计划一行。

## 3. 不做的事

- 不让 publish 等待 main 上 CI 的结果：那会把刚省下的时间加回来。
- 不改 `ci.yml`、`ui-e2e.yml`、`install-test.yml`：main 上的 CI 还负责保存编译缓存（`save-if` 只在 main 上为真），PR 靠它命中缓存。
- 不把 gate 换到 ubuntu，也不改名。
- 不改构建作业的缓存策略。

## 4. 接受的风险

以前 main 上 Release 的完整测试失败会阻止发布；改后 main CI 与 Release 并行，CI 失败时预发布可能已经发出。代码树相同也不保证结果相同（环境、时序、偶发的产品缺陷都可能导致差异），所以失败不能直接归为环境问题：先按失败原因查清；确属产品问题的，按 CLAUDE.md 已有的撤回顺序处理（取消在途发布 → `upgrade_baselines.py withdraw` → 合入 → 删除发布页）。若分支保护被放宽或出现绕过名单，要重新评审本方案。

## 5. 验收

1. 本地：新增契约测试先红后绿；提交钩子（改了 `.github/**`，自动选完整模式）通过。
2. PR：最终 HEAD 的 CI `gate`、`native-ui` 通过；本 PR 改了 `scripts/**` 与 `.github/workflows/**`，会触发 install-test，它也须通过。
3. 合入后：Release 的 gate 日志里只有 checkout、Python 与两个身份校验；build、install-test、升级矩阵、两条升级链与 publish 全部成功，新版本发布。gate 与整个工作流的耗时如实记录，不设硬门槛。
4. 回填：PR 链接、Release run 链接与各作业耗时写到本节，然后归档本计划。

## 6. 执行步骤

1. **准备分支**：在 `docs/release-gate-dedup-plan` 上 `git fetch origin` 并 rebase 到最新 `origin/main`。
2. **登记上一版并递增版本**：`python -B scripts/upgrade_baselines.py add --tag v<上一版>`（main 未前进时为 `v0.3.3`；已登记则跳过），再 `python -B scripts/bump_version.py --bump patch --summary "发布流程只做身份校验，完整测试由 PR 门禁保证"`，用 `--check --base-ref origin/main` 自查。
3. **R02 先写测试**：在 `scripts/test_build_paths.py` 的 `CiLayoutCase` 加一条测试。用几行正则截出 `release.yml` 与 `ci.yml` 中的 `gate:` 作业块（写法同 `scripts/test_upgrade_workflow.py` 的 `job_block`），断言：
   - Release gate 含 `bump_version.py --check`、`release_metadata.py` 与 `id: metadata`；
   - Release gate 不含 `./.github/actions/setup` 和 `./.github/actions/checks`；
   - CI gate 仍含 `./.github/actions/checks`。

   先在未改的工作流上运行 `python -B -m unittest discover -s scripts -p "test_build_paths.py" -v`，确认因 Release gate 含 checks 而失败。
4. **R01 改工作流**：只改 `release.yml` 的 `gate` 作业。删除 setup 与 checks 两步，在 checkout 后加：

   ```yaml
         - uses: actions/setup-python@ece7cb06caefa5fff74198d8649806c4678c61a1 # v6
           with:
             python-version: '3.12'
   ```

   `timeout-minutes` 改为 10，作业上方加注释。checkout、两个校验步骤、`outputs` 与其他作业不动。再跑第 3 步的命令，确认通过。
5. **R03 文档**：按 §2 R03 改 `CLAUDE.md`；`docs/plans/README.md` 把本计划改为“实施中”。
6. **提交与 PR**：按文件名暂存，正常提交，让钩子跑完整检查；推送后创建 PR，说明中写明 §4 的风险。检查全绿后按用户授权合并。
7. **合入后**：按 §5 第 3、4 步核对并回填。Release 失败时保留计划在 active，记录失败的作业与原因。

允许改动的文件：`.github/workflows/release.yml`、`scripts/test_build_paths.py`、`CLAUDE.md`、本计划、`docs/plans/README.md`，以及登记与版本工具自动更新的文件。
