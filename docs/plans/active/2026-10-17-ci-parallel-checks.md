# CI 拆成并行作业与原生 E2E 精简

> 状态：**已合入，待最后一项取证**。PR [#12](https://github.com/ywltby/tokenscope/pull/12) 于 2026-10-17 05:50Z 合入 main（`1223ef0`），main 的 CI、Native UI E2E 与 Release 全部通过，`v0.3.5` 已发布，四组 Rust 缓存已在 main 上保存（§5 第 3 条）。剩余：用 PR2 的第一次 CI 记录热缓存效果（§5 第 3 条后半），之后归档。
> 交付：独立小 PR，patch（只改工作流、复合动作、测试脚本与文档，不改程序）。**在分支 `ci/parallel-checks` 上实施**：它从 `feat/collection-status` 的 `ec8603b` 建立，已带发布去重计划的回填与归档（`4170869`）和 PR2 计划的修改（`ec8603b`）；从 main 另建分支会丢掉这两个提交。排在 [PR2 扫描反馈](2026-10-15-collection-status.md)之前；合入后 `feat/collection-status` 用 `git merge --ff-only origin/main` 跟上即可。

## 1. 现状（2026-10-17 核实）

PR 的等待时间取决于两项必需检查 `gate`（ci.yml）与 `native-ui`（ui-e2e.yml）中较慢的那个。main 上最近一次（`0ee50f4`）：CI [38952158892](https://github.com/ywltby/tokenscope/actions/runs/38952158892) 11m43s，Native UI E2E [38952158936](https://github.com/ywltby/tokenscope/actions/runs/38952158936) 10m17s；PR #11 上 CI `gate` 为 16–17 分钟。

**CI**：全部检查在 `gate` 一个作业里依次执行，互不依赖：

| 内容 | 耗时 |
| --- | --- |
| checkout + `./.github/actions/setup`（Rust、pnpm、Node、Python、两份缓存、前端依赖） | 1m11s |
| 版本递增检查、升级登记表检查、Python/PowerShell 契约测试 | 约 1m25s |
| 普通模式 fmt、clippy、test | 1m53s |
| 前端 typecheck、format:check、test、build | 44s |
| 浏览器 E2E | 2m15s |
| acceptance 模式 clippy、test | 2m22s |
| 构建输入清单校验（`build_inputs.py --check`，先 `cargo clean -p` 两个本地包再重建两种模式） | 1m43s |

**Native UI E2E**：

| 内容 | 耗时 |
| --- | --- |
| checkout + setup | 1m47s |
| 原生窗口选择契约 | 13s |
| 构建 release + acceptance 程序 | 2m29s |
| ui-refresh 场景（含固定 120 秒真实空闲观察） | 2m51s |
| ui-system 场景 4 轮：系统 DPI、强制缩放 1 / 1.5 / 2，每轮约 40 秒 | 2m46s |

本次运行的 `evidence.json` 显示 Runner 的系统 DPI 轮 `dpr=1`、视口 980×620，与“强制缩放 1”那一轮相同。

构建安装包不动：v0.3.3 那次 11 分钟是 PR1 改了 rust-cache key 后的第一次构建（日志 `No cache found`），v0.3.4 命中缓存后为 4m30s；它也不在 PR 等待路径上。

## 2. 改什么

### C00 升级登记表测试与真实登记表脱钩（从 PR2 的 F00 移入）

本 patch 要登记 v0.3.4，而 `RegistryCase` 直接读真实登记表，登记一次就要改测试（PR #11 登记 0.3.3 时改了 68 行）。用户 2026-10-17 决定做的 F00 正是为了消除这件事，所以把它移到本 patch 第一个提交，内容不变：

- 把 main `0ee50f4` 上的真实登记表（0.1.0、0.2.0–0.3.3）原样复制为 `scripts/fixtures/upgrade_baselines_test.json`，`scripts/test_upgrade_baselines.py` 的 `RegistryCase.setUp` 改为读这份固定表。
- `RepositoryCase` 与 `test_release_assets.py` 只核对仓库名和 0.1.0 这类固定事实，继续读真实登记表；`upgrade_baselines.py` 本身不改。
- 验收：之后执行 `upgrade_baselines.py add --tag v0.3.4`，测试文件不需要任何改动，`test_upgrade_baselines.py` 仍通过。

### C01 CI 拆成 4 个并行作业加 1 个汇总作业

`.github/actions/checks/action.yml` 增加必填输入 `group`，每一步加 `if: inputs.group == '<组>'`（上传浏览器证据那步为 `always() && inputs.group == 'frontend'`）；第一步校验 `group` 必须是下表 4 个值之一，否则失败，防止拼错后所有步骤被静默跳过。步骤本身的命令与顺序不改。

| 组 / 作业名 | 包含的现有步骤 | 预计 |
| --- | --- | --- |
| `rust` | cargo fmt --check、workspace clippy、workspace test | 约 3 分钟 |
| `rust-acceptance` | acceptance clippy、acceptance test | 约 3.5 分钟 |
| `contracts` | 版本递增检查、升级登记表检查（这两步从 ci.yml 移入本作业，需要 `fetch-depth: 0` 与 `GH_TOKEN`）、全部 Python/PowerShell 契约测试（含三步安装验收契约与 Release metadata tests）、构建输入清单校验 | 约 4.5 分钟 |
| `frontend` | typecheck、format:check、test、build、浏览器 E2E、上传浏览器证据 | 约 4 分钟 |

`ci.yml`：

- 4 个作业都在 `windows-latest`，各自 checkout + `./.github/actions/setup` + `./.github/actions/checks`（传对应 `group`）。只有 `contracts` 需要 `fetch-depth: 0`。
- 新的 `gate` 作业：`needs` 上述 4 个作业，`if: always()`，在 `ubuntu-latest` 上只做一件事——任一依赖结果不是 `success`（含 failure、cancelled、skipped）就失败。作业名保持 `gate`，规则集 `protect-main` 的必需检查不用改。
- `concurrency`、触发条件、`permissions` 不变。
- `setup` 动作不改：rust-cache 的 key 是 `${{ github.job }}-workspace-layout`，每个作业自动各用一份缓存。

### C02 契约测试

`scripts/test_build_paths.py` 的 `CiLayoutCase`：

- 新增：checks 动作里每一步（除第一步的组校验外）恰好属于 4 个组之一；`ci.yml` 中 4 个组各被调用恰好一次；`gate` 作业 `needs` 这 4 个作业且自身不调用 checks。目的是保证拆分后“不减”：没有步骤掉出所有组，也没有组没人跑。
- 改写 `test_release_gate_keeps_identity_checks_without_full_quality_gate` 最后一句：不再要求 `ci.yml` 的 `gate` 作业块里含 `./.github/actions/checks`，改为要求 `ci.yml` 使用该动作（由上一条覆盖细节）。Release gate 的断言不变。
- `test_checks_action_runs_workspace_rounds_once` 保留不改。

### C03 原生 E2E 精简

1. `frontend/scripts/contracts/native-ui-refresh.mjs`：空闲观察从 120 秒改为 30 秒，检查名 `idle_120s_no_info_lines` 改为 `idle_30s_no_info_lines`，注释同步。自动读取最短间隔 5 秒，30 秒约覆盖 6 轮，足以判断空闲时是否写 info 日志（U06 约束）。120 秒来自已归档的 UI 调整计划，属可收紧的取证时长；本地 20 轮重启压测等其他场景不受影响。
2. `.github/workflows/ui-e2e.yml`：缩放循环从 `'1', '1.5', '2'` 改为 `'1.5', '2'`，删掉端口表里 `'1'` 一项。理由见 §1：Runner 上系统 DPI 轮已是 `dpr=1`。本地手动取证仍可传 `--scale 1`，脚本不改。
3. 连带改动（实施时发现，不在原允许清单内）：`frontend/scripts/lib/native-instance.test.mjs` 从 `ui-e2e.yml` 提取全部 9xxx 端口并断言集合等于 `<9451, 9460, 9470, 9480, 9490>`，删掉 9470 后必须同步该期望值，否则前端测试必红。

### C04 文档

- `CLAUDE.md`「常用命令」里关于 CI 的那句：补充“分成 rust、rust-acceptance、contracts、frontend 四个并行作业，由汇总作业 `gate` 作为必需检查”，“不减的完整门禁”措辞保留。
- `docs/plans/README.md`「现在执行什么」把本计划改为“实施中”。

## 3. 不做的事

- 不减少任何检查，不合并普通与 acceptance 两套 Rust 测试。
- 不动构建安装包、install-test、Release 工作流与 setup 动作。
- 不去掉 main 上的 CI 与 Native UI E2E：rust-cache 只在 main 上保存，PR 靠它命中缓存。
- 不把原生 E2E 拆成多个作业共享构建产物：改动大，收益约 2 分钟。
- 不为新作业预热缓存。

## 4. 接受的代价

- 4 个新作业在 main 上第一次运行前没有自己的 Rust 缓存：本 PR 上的 `rust`、`rust-acceptance`、`contracts` 会从头编译依赖，可能比现在更慢；合入后 main 的第一次 CI 保存缓存，之后的 PR 才能体现提速。
- 旧的 `gate-workspace-layout` 缓存不再使用，7 天无访问后由 GitHub 自动清理；仓库缓存总量仍远低于 10 GB 上限。
- 每个作业各付一次约 1 分钟环境准备，总 Runner 分钟数增加；公开仓库不计费。

## 5. 验收

1. 本地：C00 后 `test_upgrade_baselines.py` 通过，登记 v0.3.4 后不改测试仍通过；C02 新测试在未拆分的 ci.yml 上失败、拆分后通过；提交钩子（改了 `.github/**`，自动完整模式）通过。
2. PR：最终 HEAD 的 `gate`（含 4 个子作业）与 `native-ui` 通过；本 PR 改了 `scripts/**` 与 `.github/workflows/**`，install-test 也须通过。如实记录各作业用时，冷缓存导致的慢不算失败。
3. 合入后：main 的 CI 与 Native UI E2E 通过；Release 发布 v0.3.5。用 PR2 的第一次 CI 记录热缓存下的 `gate`、`native-ui` 用时作为效果，不设硬门槛（预期 CI 约 5 分钟、原生 E2E 约 8 分钟）。
4. 回填：PR 链接、各运行链接与用时写到本节，然后归档本计划。

### 实施记录（2026-10-17，分支 `ci/parallel-checks`）

- C00：`test_upgrade_baselines.py` 23 项通过；随后登记 v0.3.4 未改动任何测试文件，仍 23 项通过（此前登记 0.3.3 改了 68 行）。
- C02：两条新测试在未拆分的工作流上必红，拆分后 `test_build_paths.py` 19 项全绿。
- C03：改动后 `pnpm --dir frontend test` 529 项通过（含同步后的端口集合契约）。
- contracts 组不加前端构建：把 `frontend/dist` 移走后 `python -B scripts/build_inputs.py --check` 通过（`missing`、`stale`、`changed`、`workflow_uncovered` 全空），§6 第 5 步的备选方案不启用。
- 版本：v0.3.4 已登记进升级起点矩阵，patch 递增到 0.3.5。

**PR [#12](https://github.com/ywltby/tokenscope/pull/12) 门禁（四个新作业的冷缓存首跑，2026-10-17）**

| 检查 | 运行 | 用时 |
| --- | --- | --- |
| CI `gate`（汇总，结果为四组中最慢者） | [39006263490](https://github.com/ywltby/tokenscope/actions/runs/39006263490) | 8m52s |
| — `rust` | 同上 | 7m58s |
| — `rust-acceptance` | 同上 | 8m44s |
| — `contracts` | 同上 | 6m48s |
| — `frontend` | 同上 | 4m45s |
| — `gate` 汇总作业本身 | 同上 | 3s |
| `native-ui` | [39006263499](https://github.com/ywltby/tokenscope/actions/runs/39006263499) | 7m04s |
| Install test（含 4 个升级场景） | [39006263447](https://github.com/ywltby/tokenscope/actions/runs/39006263447) | 12m37s |

PR 等待 = max(`gate`, `native-ui`) ≈ 8m52s。对照拆分前：main `0ee50f4` 的 CI 11m43s、PR #11 的 `gate` 16–17 分钟；本次是四个新作业没有自己缓存的首跑，已达成预期。`upgrade-chain` 在 PR 上按设计跳过（`if: github.event_name == 'workflow_dispatch'`）。

**合入后 main `1223ef0`（2026-10-17，§5 第 3 条前半）**

| 运行 | 作业 | 用时 | 结果 |
| --- | --- | --- | --- |
| [CI 39007741994](https://github.com/ywltby/tokenscope/actions/runs/39007741994) | `rust` / `rust-acceptance` / `contracts` / `frontend` | 8m55s / 8m31s / 8m29s / 4m20s | 通过（四组仍是冷缓存首跑） |
| 同上 | `gate` 汇总 | 2s；工作流合计 9m03s | 通过 |
| [Native UI E2E 39007741999](https://github.com/ywltby/tokenscope/actions/runs/39007741999) | `native-ui` | 6m42s（拆分前 10m17s） | 通过；工件 56 项检查全过，含 `idle_30s_no_info_lines` 与 DPR 1 / 1.5 / 2 |
| [Release 39007741954](https://github.com/ywltby/tokenscope/actions/runs/39007741954) | gate 27s、build 4m45s、install-test、升级矩阵（0.1.0 / 0.3.4 × present / deleted）、两条升级链、publish | 合计 6m51s | 通过；`v0.3.5` 预发布于 05:57:31Z，四个资产齐全 |

GitHub 缓存列表已有 main 上的 `rust-workspace-layout`（632 MB）、`rust-acceptance-workspace-layout`（632 MB）、`contracts-workspace-layout`（496 MB）、`frontend-workspace-layout`（113 MB）四份，PR 之后可以命中。旧 `gate-workspace-layout` 缓存不再使用，按 §4 等 GitHub 自动清理。

**待取证（§5 第 3 条后半）**：PR2 第一次 CI 的 `gate`（及四组）与 `native-ui` 用时，填在这里后归档本计划。

## 6. 执行步骤

1. **准备分支**：在 `ci/parallel-checks` 上 `git fetch origin` 并 rebase 到最新 `origin/main`。
2. **C00**（第一个提交）：按 §2 C00 复制固定登记表并改 `RegistryCase.setUp`，运行 `python -B -m unittest discover -s scripts -p "test_upgrade_baselines.py" -v`。
3. **登记上一版并递增版本**：`python -B scripts/upgrade_baselines.py add --tag v0.3.4`（main 前进时改为当时的上一版），再 `python -B scripts/bump_version.py --bump patch --summary "CI 拆成并行作业，精简原生 E2E 等待"`，用 `--check --base-ref origin/main` 自查。确认测试文件没有因登记而改动。
4. **C02 先写测试**：按 §2 C02 写测试，在未改的工作流上运行 `python -B -m unittest discover -s scripts -p "test_build_paths.py" -v`，确认失败。
5. **C01 改工作流与动作**：按 §2 C01 改 `checks/action.yml` 与 `ci.yml`，再跑第 4 步命令确认通过。`contracts` 作业里构建输入清单校验原本排在前端 build 之后；该校验不登记 `frontend/dist`，且只做 debug 构建，预计不依赖前端产物。若 PR 上确实失败，在 `contracts` 组里该步之前加 `pnpm --dir frontend build`，并在 PR 说明里写明原因。
6. **C03**：按 §2 C03 改两个文件。
7. **C04 文档**。
8. **提交与 PR**：按文件名暂存，正常提交，让钩子跑完整检查；推送后创建 PR，说明中写明 §4 的冷缓存代价。检查全绿后按用户授权合并。
9. **合入后**：按 §5 第 3、4 条核对并回填。

允许改动的文件：`.github/actions/checks/action.yml`、`.github/workflows/ci.yml`、`.github/workflows/ui-e2e.yml`、`frontend/scripts/contracts/native-ui-refresh.mjs`、`frontend/scripts/lib/native-instance.test.mjs`（C03 连带）、`scripts/test_build_paths.py`、`scripts/test_upgrade_baselines.py`、`scripts/fixtures/upgrade_baselines_test.json`（新增）、`CLAUDE.md`、本计划、`docs/plans/README.md`、`docs/plans/active/2026-10-15-collection-status.md`，以及登记与版本工具自动更新的文件。
