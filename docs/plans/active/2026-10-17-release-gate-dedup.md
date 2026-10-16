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

其余作业（build、install-test、plan-upgrades、upgrade-test、upgrade-chain、publish）不动。它们测的是真正要发布的安装包，PR 上测不到。

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

以前 main 上 Release 的完整测试失败会阻止发布；改后若 main 上的 CI 失败，预发布已经发出。由于代码树与 PR 相同，这种失败只可能来自环境或不稳定的测试。处理方式：先按失败原因查清；确属产品问题的，按 CLAUDE.md 已有的撤回顺序处理（`upgrade_baselines.py withdraw` → 合入 → 删除发布页）。发布本来就是预发布，不是正式版。

## 5. 验收

1. 本地：提交钩子（改 `.github/**` 会自动选完整模式）通过，新增契约测试先红后绿。
2. PR：CI `gate` 与 `native-ui` 通过（install-test 按路径过滤自动触发时也须通过）。
3. 合入后：Release 的 gate 在 5 分钟内完成，build、install-test、升级矩阵、两条升级链与 publish 全部成功，`v0.3.4` 发布。把 run 链接和各作业耗时回填到本节，计划随后归档。
