# 发布流程去掉重复的完整测试

> 状态：**待用户确认**（2026-10-17）。
> 交付：独立小 PR，patch（只改发布工作流与文档，不改程序）。确认后从最新 main 建 `ci/release-gate-dedup`，排在 [PR2 扫描反馈](2026-10-15-collection-status.md)之前。
> 合入前照例：`python -B scripts/upgrade_baselines.py add --tag v0.3.3` 登记上一版，再 `python -B scripts/bump_version.py --bump patch --summary "..."`。

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

以前 main 上 Release 的完整测试失败会阻止发布；改后 main CI 与 Release 并行，CI 失败时预发布可能已经发出。相同代码树不保证不同运行的结果完全相同：环境、外部依赖、构建身份、时序以及尚未被测试稳定检出的产品缺陷都可能产生差异，不能直接归为环境问题。处理方式：先按失败原因查清；确属产品问题的，按 CLAUDE.md 已有的撤回顺序处理（取消相关在途发布 → `upgrade_baselines.py withdraw` → 合入 → 删除发布页）。发布为预发布不代表可以忽略失败。若分支保护被放宽或引入绕过者，必须重新评审本方案前提。

## 5. 验收

1. 本地：提交钩子（改 `.github/**` 会自动选完整模式）通过，新增契约测试先红后绿。
2. PR：CI `gate` 与 `native-ui` 通过（install-test 按路径过滤自动触发时也须通过）。
3. 合入后：Release 的 gate 在 5 分钟内完成，build、install-test、升级矩阵、两条升级链与 publish 全部成功，`v0.3.4` 发布。把 run 链接和各作业耗时回填到本节，计划随后归档。

## 6. 详细执行步骤（2026-10-17 补充，尚未实施）

本节只细化 R01–R03，不新增工作流、不改运行程序、不降低 PR 和安装升级门槛。本轮文档补充留在 `docs/release-gate-dedup-plan`；用户确认实施后再建立实现分支。每步检查退出码，失败即停止依赖它的后续步骤。

### 6.1 开工核对与版本登记

1. 运行 `git status --short --branch`、`git fetch origin`，确认没有其他 agent 的未提交改动；不得用 reset/restore 清理他人的工作。
2. 用 `gh api repos/ywltby/tokenscope/rulesets` 找到 `protect-main`，再读取其详情。必须仍为 active、无 bypass actor、必需检查含 `gate` 与 `native-ui`、strict 为 true、合并方法仅 merge。2026-10-17 本轮复核的规则集 ID 为 `25173983`，执行时按名称查找，不把 ID 写死进程序。若前提改变，报告差异，不自动修改规则集或继续删除门禁。
3. 从最新 `origin/main` 创建 `ci/release-gate-dedup`，将本计划文件的已审版本带入该分支；若 main 已有同版本计划则不重复搬运。用 `git diff origin/main --stat` 确认没有夹带其他功能。
4. 读取当前 main 的应用版本。先用 `gh release view v0.3.3` 核实上一版已公开及资产齐全，再运行 `python -B scripts/upgrade_baselines.py add --tag v0.3.3`。如果已登记则直接 check，不删条目重加；版本已前进时改为登记真实上一版，不能机械照抄 0.3.3。
5. 运行 `python -B scripts/upgrade_baselines.py check --base-ref origin/main`，确认资产与登记一致。再运行 `python -B scripts/bump_version.py --bump patch --summary "发布身份校验与完整质量门禁分离，保留安装升级验收"`，最后 `python -B scripts/bump_version.py --check --base-ref origin/main`。同一个 PR 的补修不重复加号。预期版本为 0.3.4；若 main 已前进，以工具算出的下一 patch 为准，更新本文的预期发布号。

允许的实现文件：`.github/workflows/release.yml`、`scripts/test_build_paths.py`、`CLAUDE.md`、本计划与 `docs/plans/README.md`，以及登记/版本工具正常更新的登记表、四份版本配置、根 Cargo.lock、release-change.json。不更新第三方依赖，不修改共享 setup/checks 动作。

### 6.2 R02：先写契约，证明现状会失败

在 `scripts/test_build_paths.py` 的 `CiLayoutCase` 新增 `test_release_gate_only_validates_identity_while_ci_keeps_full_checks`，必要时拆成同一组内的数个短测试。

1. 添加小型 `workflow_job_block(text, name)` 辅助函数：只提取 `jobs:` 下指定两空格缩进的作业，从该行读到下一个同级作业。要求 jobs 与目标作业恰好命中一次，缺失/重复/空块抛 `AssertionError`；不要用全文件字符串包含判断，也不要为这个测试引入 PyYAML 或编写通用 YAML 解析器。
2. Release gate 检查：runner 仍为 Windows、timeout 为 10；checkout 固定 SHA、fetch-depth 0、persist-credentials false；Python 3.12 的 setup 存在；不再调用 `./.github/actions/setup` 和 `./.github/actions/checks`。
3. 从 gate 的步骤范围检查 `bump_version.py --check --event-file` 和 `release_metadata.py` 各出现一次；metadata 的 `id: metadata` 保留。版本检查的非零退出必须传播；不能给 gate 或两个校验步骤增加 `continue-on-error`，不能增加跳过身份校验的 `if`。
4. 检查 outputs 恰好保留 `tag/sha/version/title/prerelease/latest`，且仍来自 `steps.metadata.outputs.<同名字段>`。CI 的 gate 必须继续调用共享 checks；Release build 必须继续调用共享 setup，避免误删整个工作流的构建依赖。
5. 运行 `python -B -m unittest discover -s scripts -p "test_build_paths.py" -v`，在当前未改的工作流上应因 Release gate 仍含完整动作或 timeout=45 而失败。保存失败日志到 `qa-artifacts/release-gate-dedup/`，核对是真实断言失败，不能拿导入失败/命令不存在当红灯证据。

补充轻量反例：在内存副本中分别删除版本检查、删除 metadata、将 checks 放回 Release gate、删除 CI checks、让目标 job 不存在，验证检查器拒绝。测试不得改写真实工作流；正常 build 中仍有 setup 的副本必须通过 gate 范围检查。若封装验证函数，就让正常文件与反例调用同一函数，避免只测替身。

### 6.3 R01：精确修改 Release gate

1. 只在 `.github/workflows/release.yml` 的 `jobs.gate` 删除两个本地复合动作，其他 jobs 保持原样。
2. 原 setup 位置替换为下列固定 action（与仓库当前使用值一致，实施时确认未被其他 PR 更新）：

```yaml
      - uses: actions/setup-python@ece7cb06caefa5fff74198d8649806c4678c61a1 # v6
        with:
          python-version: '3.12'
```

3. gate 的 `timeout-minutes` 改为 10，保持作业 ID `gate`。在作业上方注释说明其只负责发布版本与身份，完整检查由受保护 PR 保证。
4. 步骤顺序必须是 checkout → setup-python → 版本递增检查 → metadata；checkout 的 ref/fetch-depth/persist-credentials、metadata id、六项 outputs 与所有下游 needs 不变。Windows 默认 pwsh 沿用，不更换 runner 或额外安装 Rust/Node/pnpm。
5. 再运行 §6.2 的命令，全部通过；对比 `git diff -- .github/workflows/release.yml`，确认 build、安装、矩阵、双链、publish 的内容没有变化。

### 6.4 R03：同步文档与本地验证

1. `CLAUDE.md` 的「发布与升级」区分两个同名 gate：PR/CI gate 仍做完整检查，Release gate 只做发布身份校验。明确 main CI 与 Native UI E2E 并行执行、不成为 Release 的等待依赖；安装升级与撤回保护保持原门槛。
2. `docs/plans/README.md` 将本任务从待确认改为实施中，保持在 PR2 之前。只有功能实际落地后才在本计划登记 R01–R03 的实现提交；远程证据未出时不能写已验收或已归档。
3. 逐条运行下列定向检查，均须 exit 0：

```powershell
python -B -m unittest discover -s scripts -p "test_build_paths.py" -v
python -B -m unittest discover -s scripts -p "test_release_metadata.py" -v
python -B -m unittest discover -s scripts -p "test_bump_version.py" -v
python -B -m unittest discover -s scripts -p "test_install_workflow.py" -v
python -B -m unittest discover -s scripts -p "test_upgrade_workflow.py" -v
git diff --check
```

4. 精确暂存本批文件后运行 `python -B scripts/pre_commit_checks.py --explain`。因为改了 `.github/**`，应选完整检查；正常 commit 让钩子执行，失败先修复，不使用 `--no-verify`。不必在提交前人工重复跑一遍相同完整门禁。提交标题、正文用中文 Conventional Commits，不包含原始日志或工件。
5. 本地不模拟安装、不在真实用户目录运行安装脚本、不创建测试版本标签。main/tag 两种身份分支由现有脚本契约覆盖，真实产物留给 Actions。

### 6.5 PR 与合入前证据

1. 推送实现分支并通过 `gh pr create` 创建到 main 的 PR，说明只移除 Release 的重复完整测试，列出分支保护前提、实际产物仍验收以及 main CI 不再阻断发布的风险。保存 PR 号码和来源 HEAD。
2. 用 `gh pr checks <编号>` 与 `gh pr view <编号> --json headRefOid,statusCheckRollup` 核实同一最终 HEAD 的 CI gate、Native UI E2E 全绿；本批修改 release.yml/脚本应核实是否命中 install-test 的 paths，触发的安装及四项单跳升级也必须通过。不能因为 ruleset 没把安装列为 required 就忽略红灯。
3. GitHub PR 默认测试合并 ref，工件 SHA 可与分支 HEAD 不同；记录实际 checkout SHA，并核对父提交包含最终 HEAD 与当前 main。禁止用前一轮 HEAD 的绿色结果代替当前证据。复核分支保护仍满足 §6.1，然后按用户授权和项目流程合并，不用 bypass。
4. Release 只在 main/tag push 时运行，因此 PR 绿色只能证明本地契约和 PR 检查通过，不能宣称已测得新 Release gate 耗时。不要临时加触发器或推测试标签取得“绿色”。

### 6.6 合入后验收、耗时与归档

1. 用 `gh run list --branch main --workflow release.yml` 找到本次 merge SHA 的运行。检查 Release gate 日志只有 checkout、Python 和两个身份校验，没有 Rust 工具链安装、pnpm install 或完整 checks；六项输出非空，下游能读到正确版本/SHA。
2. 同一 Release 中 build、五场景安装、起点矩阵、present/deleted 双链、publish 全部成功。下载安装/升级证据，核对当前安装包身份、各起点和每跳结果；不能仅凭 job 绿色就将链路标完成。
3. 读取目标 Release 和标签，确认 tag 解引用为本次 merge commit、版本一致、安装包/校验和/metadata/证明文件齐全；不得删除或覆盖既有标签和资产。main CI 与 native-ui 的结果同时记录，若失败按 §4 调查，不以发布成功掩盖失败。
4. gate 的 5 分钟目标按 job `startedAt → completedAt` 计，排队时间单列；工作流总耗时也单列，不能把取消、跳过或没有实际执行当提速。总发布约 14 分钟只是预估，不是承诺。若 gate 超过 5 分钟，分析具体步骤：setup 下载/Runner 波动与残留完整测试分开记录，不重跑碰绿、不静默改目标。
5. 在 §5 回填以下表格并附运行/工件链接；上述证据取得后才将计划移入 archive，同步索引及所有指向旧 active 路径的有效链接。若新 Release 失败，保持 active 并记录失败阶段；需要恢复原 gate 时走正常修复 PR，不削弱发布门禁。

| 证据 | 待回填内容 |
| --- | --- |
| 实现身份 | PR、最终 HEAD、PR 测试 checkout SHA、main merge SHA |
| 本地验证 | 契约红灯原因、绿灯结果、完整提交门禁结果 |
| PR 门禁 | CI / Native UI / 安装与单跳升级运行链接及结论 |
| Release | run 链接、gate/build/安装/矩阵/双链/publish 时长与结论 |
| 发布身份 | 实际版本、tag commit、metadata SHA、安装包哈希 |
| 运行耗时 | gate 执行时间、排队时间、工作流总耗时、与旧约 31 分钟比较的限制 |
