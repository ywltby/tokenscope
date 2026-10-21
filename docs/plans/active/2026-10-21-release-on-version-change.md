# 只在版本变化时发布：合入 main 与发布解耦

> 状态：**用户 2026-10-21 确认方向并要求写成计划**（不开预发布分支，改为“版本号没变就不发布”）。承接[整合计划 N16](2026-10-11-consolidated-remaining-work.md#n16合入-main-与发布解耦)。
> 修订：本计划修改用户 2026-10-12 定稿的“main 每批必递增版本”规则，以用户本次确认为准，并同步 `CLAUDE.md`。
> 分支 `ci/release-on-version-change`；本批不含产品改动，**按新规则不递增版本**。

**Goal：** 只有会进入安装包的改动才要求递增版本并触发发布；文档、计划、CI、脚本、测试类改动可以合入 main，但不产生新版本、不构建安装包。

**Architecture：**
- `bump_version.py` 的检查从“必须递增一次”改为：先按改动路径判断这次合入是否涉及产品输入，涉及时必须递增一次，不涉及时允许版本不变。
- Release 的 gate 用同一个函数输出 `release`，版本没变时，构建、安装、升级、发布各作业全部跳过。
- 产品输入的判断复用 `scripts/build-inputs.json`（编译进程序的非 Rust 输入），不另起一套口径。
- 必需检查（`gate`、`native-ui`）、升级基线登记、标签推送发布，都不改。

**Tech Stack：** Python 3.12 脚本与 unittest；GitHub Actions（`release.yml`、`.github/actions/checks`）。

## 1. 背景与已核实事实

- **Release 的触发方式**：`release.yml` 在 `push: branches: [main]` 和 `tags: ['v*']` 时触发。gate 调 `bump_version.py --check --event-file`，main 推送时以 `before` 为基线，要求版本按 `release-change.json` 恰好递增一次，随后一律构建并发布。
- **PR 检查**：`.github/actions/checks` 对每个目标为 main 的 PR 跑同一个检查，所以不递增就过不了 gate。
- **结果**：只收录研究资料的 PR #17 发了 v0.5.3；只做归档的 PR #20 也要发 v0.5.6，安装包内容与上一版相同。
- **不能只按目录判断**：`docs/privacy.md` 登记在 `scripts/build-inputs.json`，消费方是 `tokenscope-app`（default 与 acceptance 两种构建模式），会编译进程序。
- **起点选择**：`upgrade_baselines.py select` 只选**严格小于**当前版本的已发布条目。版本不变的 PR 照样能选出起点，安装测试和升级测试不受影响。
- **基线检查**：CI 要求 PR 的基础版本已经登记。这条规则保留：版本不变的 PR 在上一版发布之后，同样要先登记它。
- **PR #20 的处理**：它包含归档文档（`592455c`）和“登记 v0.5.5 并递增到 0.5.6”（`b5dc1c0`）两个提交。本分支已拣入归档提交（`6574d91`）；登记 v0.5.5 在本批完成，**不递增 0.5.6**；PR #20 由本批关闭。

## 2. 范围与不做

做：
- 产品输入判定。
- 调整 `bump_version.py` 的检查语义。
- Release 在版本未变时跳过后续作业。
- 测试与工作流契约。
- 更新 `CLAUDE.md` 和文档。
- 合并后用真实的 main 运行验收一次。

不做：
- 不开预发布 / develop 分支。
- 不改必需检查和 ruleset，不改升级基线的登记与 `check` 规则。
- 不改标签推送发布、发布资产与签名流程，不改 `bump` 子命令的递增算法。
- 不为版本未变的合入单独设计其他工作流。CI 和 Native UI E2E 照常运行。
- 不追溯撤回 v0.5.3。

## 3. 必须保持的不变量

1. **产品改动必须递增**：只要基线到 HEAD 的改动里有任何产品输入，版本就必须按 `release-change.json` 恰好递增一次，规则与现在完全一样。
2. **默认按产品处理**：只有命中 §4 RV01 列出的“非产品路径”，而且没有登记在 `build-inputs.json`、没有被 default 构建模式消费的路径，才算非产品。未知路径和新增的根目录文件一律按产品处理。
3. **版本不变要成对**：版本不变时，`release-change.json` 必须与基线逐字节一致，不留下一份没有对应版本的声明。
4. **可以主动递增**：非产品改动也可以主动递增一次（例如要发布流水线修复），这时按递增规则检查并正常发布。
5. **发布判定只有一个来源**：PR 检查和 main 推送的 gate 调用同一个函数。main 推送时再按 `before..sha` 复核一次，版本不变却含产品改动就失败，不跳过。
6. **跳过不是失败**：版本未变时 Release 运行结论为成功，gate 之后的作业标记为 skipped，不创建标签，不改动已有发布。
7. **其余发布行为不变**：标签推送发布、“新 main 提交不取消另一个提交的发布”、失败后重跑同一提交，这些行为都不变。

## 4. 任务

每项先写会失败的测试，再实现。一个提交只解决一类问题。

### RV01 产品输入判定

- 文件：`scripts/bump_version.py`（或抽出一个小模块，供它调用）。
- 新增 `release_relevant(paths, manifest) -> list[str]`，返回需要发布的路径。没有这样的路径时返回空列表。
- **非产品路径**：
  - `docs/**`，以及不在 `build-inputs.json` 中的 `*.md`。
  - `.github/**`、`.githooks/**`、`scripts/**`。
  - `frontend/scripts/**`、`frontend/**/*.test.ts`、`frontend/**/*.test.mjs`。
  - `crates/*/tests/**`、`crates/*/examples/**`、`src-tauri/tests/**`。
  - `.gitignore`、`.gitattributes`、`.editorconfig`、`release-change.json`。
- 上述路径中，凡是 `build-inputs.json` 登记了 default 模式消费方的，仍按产品处理。例如 `docs/privacy.md`；核心库测试 fixture 若有 default 消费方也一样。
- 其余路径（`crates/*/src/**`、`src-tauri/**` 非测试部分、`frontend/` 产品源码、根 `Cargo.toml`/`Cargo.lock`、工具链与 `.cargo/**`、未知路径）都按产品处理。
- **测试**：
  - 纯文档、纯 CI/脚本、纯测试改动返回空。
  - `docs/privacy.md` 返回它本身。
  - 测试 fixture 按 `build-inputs.json` 的 default 消费方判定。
  - 未知根目录文件、`frontend/src/*.vue`、`Cargo.lock` 都返回对应路径。
  - 重命名时，新旧路径都要参与判断。

### RV02 版本检查语义

- 文件：`scripts/bump_version.py`、`scripts/test_bump_version.py`。
- `check` / `check_event` 先用 `git diff --name-status --no-renames <base>..HEAD` 取改动路径，再调用 RV01：
  - 有产品路径：沿用现有的恰好递增一次检查。版本不变时，报错信息里列出前几个产品路径。
  - 没有产品路径、版本不变：要求 `release-change.json` 与基线一致，返回 `release: false`。
  - 没有产品路径、版本有递增：按递增规则检查，返回 `release: true`。
- 返回值增加 `release` 字段，命令行 JSON 输出带上它。标签事件保持原样，返回 `release: true`。
- **测试**（追加，不删现有用例）：
  - 文档改动、版本不变：通过，`release: false`。
  - 产品改动、版本不变：失败，信息里含路径。
  - `docs/privacy.md` 改动、版本不变：失败。
  - 非产品改动、版本合法递增：通过，`release: true`。
  - 版本不变但改了 `release-change.json`：失败。
  - 未知路径、版本不变：失败。
  - main 推送事件和 PR 事件各覆盖一次“跳过”和一次“拒绝”。

### RV03 Release 跳过后续作业

- 文件：`.github/workflows/release.yml`，以及解析工作流的契约测试（`scripts/test_upgrade_workflow.py`、`scripts/test_install_workflow.py` 等，以实际检索结果为准）。
- **gate**：
  - 版本检查步骤输出 `release`。
  - `release_metadata.py` 只在 `release == 'true'` 时运行。如果它对版本未变的情况会报错，就按条件跳过，**不改它的发布身份校验**。
  - gate 的 outputs 增加 `release`。
- **下游作业**：`build`、`install-test`、`plan-upgrades`、`upgrade-test`、`upgrade-chain`、`publish` 全部加 `if: needs.gate.outputs.release == 'true'`。已有 `if:` 的作业与之合并，不得覆盖原条件。
- **契约测试**：
  - 断言每个下游作业都受 `release` 条件约束。
  - 断言 gate 输出了 `release`。
  - 断言版本检查仍然是 gate 的第一个校验步骤。

### RV04 文档与规则同步

- **`CLAUDE.md`「提交流程」**：
  - 改写“main 版本递增”条目：注明用户 2026-10-21 修订，只有产品输入改动才递增；纯文档、CI、脚本、测试改动不递增、不发布；可以主动递增。
  - 删掉“准备合入 main 的文档 PR 也须 patch”。
  - 「发布与升级」条目补一句“版本未变的 main 推送，Release 只跑 gate”。
  - 「升级起点矩阵」里“准备合入 main 前必须先登记上一版”保持不变。
- **计划文档**：本计划回填执行记录。整合计划的 N16 行、§7 执行账，以及 `docs/plans/README.md` 同步。

### RV05 交付与真实验收

- **准备**：
  - 合并最新 `origin/main`。
  - `python -B scripts/upgrade_baselines.py add --tag v0.5.5`，登记上一版。
  - **不运行 `bump_version.py --bump`**。用新检查确认：`--check --base-ref origin/main` 输出 `release: false`。
- **开 PR**：本批改动全部是非产品路径，开普通（非草稿）PR。
  - 正文写明：关闭 PR #20 的原因（归档内容已并入本批，不再发 0.5.6），并在 PR #20 留言链接本 PR 后关闭。**关闭前先向用户确认。**
  - 必需检查和 Install test 全部通过后，交给用户复核，**合并由用户决定**。
- **合并后真实验收**：
  - 记录 main 上该提交的 Release 运行：gate 成功，下游作业全部 skipped，没有出现 `v0.5.6` 标签或发布。
  - CI 和 Native UI E2E 照常成功。
  - 下一个产品 PR 合入时，确认 Release 照常构建并发布。这一项记为待验，不提前标通过。

## 5. 验收

- RV01–RV03 的新测试在修改前的代码上确认红（记录失败输出），修改后变绿。
- `python -B -m unittest discover -s scripts -p "test_*.py"` 全部通过；完整提交钩子通过。
- PR 的 CI（gate）、Native UI E2E、Install test 全部成功。
- 合并后 main 的 Release 运行满足 RV05 的“跳过”验收。产品 PR 的正常发布记为待验。

## 6. 执行记录

2026-10-21 在分支 `ci/release-on-version-change` 实施，RV01–RV03 一项一个提交：

| 任务 | 提交 | 红 → 绿 |
| --- | --- | --- |
| RV01 产品输入判定 | `02a680f` | 先追加 9 条 `ReleaseRelevanceCase`；未改实现时 12 处 `AttributeError: module 'bump_version' has no attribute 'release_relevant'`（Ran 25 tests，errors=12），实现后 25 条全过 |
| RV02 版本检查语义 | `2427f5a` | 先追加 `ReleaseOnChangeCase`；未改实现时 4 处 `KeyError: 'release'`、2 处断言失败（报错未列产品路径、声明不一致的信息缺失），Ran 33 tests，failures=2 / errors=4；实现后 33 条全过 |
| RV03 Release 跳过下游 | `7849f0c` | 先加 `ReleaseSkipCase` 并调整 if 计数断言；未改工作流时 9 处失败（6 个下游作业无发布条件、gate 无 `release` 输出、身份校验无 if、`upgrade-test` 的 if 计数 1≠2），Ran 8 tests，failures=9；改后 8 条全过 |
| RV04 文档与规则同步 | 本提交 | 只改文档，`check_staged_docs.py` 通过 |
| RV05 交付与验收 | 待回填 | 合并 main、登记 v0.5.5、`--check --base-ref origin/main` 输出 `release: false`、开 PR 取检查结论 |

与计划假设不同、经用户确认的处理：

1. **`release_metadata.py` 对版本未变不报错**。它只按 ref 与当前版本产出 tag（`scripts/release_metadata.py:26-38`），版本未变时会输出 `tag=v0.5.5`；而 `release_assets.py` 的 publish 在标签已指向别的提交时直接拒绝（`scripts/release_assets.py:249-252`）。因此仍按 `release == 'true'` 条件跳过它，理由记为「不报错，但产出会复用已有标签」。
2. **`test_upgrade_workflow.py` 原有断言禁止 `upgrade-test` 带作业级 `if`**（作业内 `if:` 计数恒为 1）。按用户批准改为：`release.yml` 中该作业 `if:` 恰为 2 条（作业级发布判定 + 证据上传），`install-test.yml` 保持 1 条；`continue-on-error`、`exclude:`、`if: false` 仍然禁止。
3. **现有用例 `test_version_rollback_skip_and_new_main_baseline_are_rejected` 的最后一段**用「基线指向 HEAD」断言必须失败。新语义下这是「没有改动的合入」，合法返回 `release: false`。按用户批准，把该段改为「自比不发布」，并追加「已递增基线上改产品文件、版本不变仍必须失败」；前 5 个回滚/跳号断言不动。
