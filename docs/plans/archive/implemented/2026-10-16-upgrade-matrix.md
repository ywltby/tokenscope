# 升级测试补测：起点矩阵、连续升级链与原生验收重启修复

> **2026-10-17 归档复核：已实现。** PR [#10](https://github.com/ywltby/tokenscope/pull/10) 于 2026-10-16 合入 main（`e3eb680`）。§10.7 所列“仍未取得”的证据已在 [Release](https://github.com/ywltby/tokenscope/actions/runs/38916727732) 取得：gate、构建、五场景安装、四个单跳升级（0.1.0 / 0.3.2 × present / deleted）与两条连续升级链 0.1.0 → 0.3.2 → 0.3.3 全部通过，`v0.3.3` 于 15:04:37Z 发布；`build-metadata.json` 为 schema 2，提交与持久格式指纹和登记表一致。UP09 的本地 20 轮重启取证见 §10.6。文件头中“从最新 main 建 `feat/upgrade-matrix`”为原计划写法，实际在 `docs/upgrade-matrix-plan` 分支实施。归档不等于其他计划的待验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。下方原文保留当时的状态与证据，其中“待合入 / 待门禁 / 未实施”等字样以本头部为准。

> 状态：**用户已确认（2026-10-16）**，第 9 节 D1–D5 均已决定；同日用户决定把原生验收重启连接修复（UP09，§4.8）并入本计划，不推迟到 PR2。执行方补充（`fb35e91`）经[审阅第 18 节](../../../reviews/2026-10-15-source-discovery-plan-review.md)复核后已并入正文；按用户要求删去过度防御的设计与过高的验收门槛（第 19 节）。**本地实施已完成（2026-10-16）**，第二轮独立复核的 5 项问题已修复，逐项证据见第 10 节；PR [#10](https://github.com/ywltby/tokenscope/pull/10) 的 CI、Install test（含 0.1.0 与 0.3.2 两个起点的四个作业）与 Native UI E2E 均已通过，Release 的双链与 `v0.3.3` 发布待合入 main。
> 交付：独立 PR，patch（测试与发布门禁维护，不改变统计口径和界面）。实施时从最新 main 建 `feat/upgrade-matrix`。
> 顺序：**下一个 PR**，在 [PR2 扫描反馈](2026-10-15-collection-status.md)之前合入。PR2 的 F04 要扩展同一个原生验收脚本，先修好重启连接，PR2 才能在稳定的测试架上取证；也因此早于 [PR3 来源目录](../partial/2026-10-14-source-discovery-and-additional-paths.md)，PR3 的设置迁移能直接得到真实 0.3.x 安装包的升级覆盖（见 §8）。
> 承接：[整合计划 N12](../../active/2026-10-11-consolidated-remaining-work.md) 的“真实旧包升级”责任；本计划只扩展起点与升级次数，不改变 N12 已验收的 0.1.0 单跳场景。

## 1. 现状（2026-10-16 核实）

### 1.1 现有测试做什么

入口 `scripts/test_upgrade.ps1`，编排在 `scripts/upgrade_test_helpers.ps1` 的 `Invoke-UpgradeScenario`，夹具 `scripts/upgrade_fixture.py`，基线 `scripts/fixtures/upgrade_baseline.json`。

| 项 | 现状 |
| --- | --- |
| 起点 | 只有历史 0.1.0（`pre-8d1d90de…` 原资产，执行前校验写死的 SHA256） |
| 升级次数 | 一次：当前安装包直接覆盖 0.1.0 |
| 场景 | 2 个：`present`（旧日志保留）、`deleted`（旧版采集后删除日志） |
| 触发 | `install-test.yml`（PR，带路径过滤）与 `release.yml`（main，发布前） |
| 环境 | 只在 GitHub-hosted 一次性 Runner；本机只跑契约 |
| 耗时 | 每个场景约 1 分钟（PR #9：59 s、1 m 12 s） |

单轮步骤：确认 Runner 干净 → 装 0.1.0 并核对注册版本 → 生成合成日志，旧版自己采集并核对 `cache.db` → 记录旧主程序与三个数据文件哈希 →（`deleted` 删除日志）→ 覆盖安装当前包，核对注册版本与主程序哈希 → 启动前确认数据文件未变 → 写入测试同意 → 启动新版核对 `history.db` 事实、`cache.db` 只读、价格与设置保留 → 重启再核对一次。

### 1.2 已发布版本的持久格式

| 版本 | 提交 | 数据形态 | 历史库结构 | 解析/身份修订 | 价格索引 | 隐私同意 | 安装包 SHA256（GitHub digest） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0.1.0 | 8d1d90de | 旧 `cache.db`（v6） | 无 | 无 | 8 | 无 | 798e7722…2ece6 |
| 0.2.0 | 8d17a47 | `history.db` | 4 | 1 / 1 | 9 | 有 | 2a84dad6…bb9c |
| 0.2.1 | e75da79 | `history.db` | 4 | 1 / 1 | 9 | 有 | ee231bd0…748f |
| 0.3.0 | 5974e6b | `history.db` | 4 | 1 / 1 | 9 | 有 | bd3faa0d…f3c7 |
| 0.3.1 | e34495b | `history.db` | 4 | 1 / 1 | 9 | 有 | 667d3be1…814f |
| 0.3.2 | 19aae8b | 同上 | 4 | 1 / 1 | 9 | 有 | 发布中，登记时取 |

来源：各标签源码中的 `HISTORY_SCHEMA_VERSION`、`PARSER_REVISION`、`IDENTITY_REVISION`、`INDEX_VERSION` 与 `privacy_policy_accepted`；安装包哈希取自 Release 资产 digest。0.2.0 起运行时不再写 `cache.db`（只在单元测试中出现）。就绪日志“汇总完成（会话 …）：… 请求=N”在 0.2.0 与 0.3.1 源码中格式相同。

### 1.3 缺口

1. **中间版本当起点**：0.2.0–0.3.1 → 当前没有真实安装包测试。当前真实用户最常见的路径正是“上一版 → 新版”。
2. **结构升级后的旧库**：0.2.x/0.3.x 的 `history.db` v4 一旦要迁移到 v5，只有单元测试，没有真实旧包产出的库；0.1.0 走的是 `cache.db` 导入路径，替代不了。
3. **设置格式变化**：PR3 会把旧 `dir` 迁移为追加路径。现有 `verify_settings` 要求每个旧键原值保留，PR3 合入后会误判失败；它也没法表达“按迁移规则变化后的期望”。
4. **连续升级**：中间版本可能写入别名、身份修订、重新解析标记等额外数据，0.1.0 → 0.2.0 → … → 当前的最终状态是否与一次跨越相同，没有验证。

## 2. 目标与非目标

**目标**

- G1 每次发布前都测“上一个公开版本 → 当前”。
- G2 每种出现过的持久格式各保留一个代表起点，格式变化后旧格式仍被测试。
- G3 Release 流水线测一条连续升级链。
- G4 起点由仓库内登记表决定，可复现；被撤回的版本不作为默认起点；含本计划检查的发布流水线拒绝重新发布撤回版本（更早的流水线不受约束，靠撤回流程禁止重跑，§4.4）。
- G5 旧安装包始终是原发布资产，执行前校验写死的 SHA256，不重新构建历史包。

**非目标**

- 降级测试（NSIS 已设 `allowDowngrades: false`，较高结构版本拒绝打开已有单元测试）。
- `pre-*` 中间提交包作为起点（只有 0.1.0 的来源资产例外，沿用现状）。
- 卸载、真实用户数据、本机真实安装。
- 改变统计口径、迁移逻辑或界面。
- 对任意历史流水线强制禁发（需要仓库级发布权限策略，不在本 PR）。

## 3. 不变量

| 编号 | 内容 |
| --- | --- |
| I1 | 旧安装包只能来自登记表里的原资产 URL，下载后、执行前 SHA256 必须与登记值一致；不一致立即失败，不重试别的来源 |
| I2 | 旧数据只能由旧程序自己采集产生，不合成旧库（沿用 `upgrade_fixture.py` “绝不生成旧缓存”） |
| I3 | 升级后请求事实与该起点、该场景的期望清单逐条一致（含四桶），重启后不增量；`pricing.toml` 字节不变；旧 `cache.db` 只读不变 |
| I4 | 某一跳发生历史库结构升级时，这一跳必须新增 `history.db.backup-v{旧版本}`，可只读打开且含旧事实；结构未变的一跳不得新增或覆盖备份，此前跳留下的备份允许保留 |
| I5 | 设置按“起点设置格式 → 当前设置格式”的声明式迁移规则核对：规则内的字段按规则变化，其余字段原值保留 |
| I6 | 起点集合只由已检出提交中的登记表和当前版本号计算，CI 不按 GitHub 列表动态挑选；GitHub 只用于一致性校验 |
| I7 | 被撤回的版本不进入默认起点；含本计划检查的 `publish` 拒绝发布登记为撤回的版本 |
| I8 | 任一起点或任一跳失败，整体失败；不设可选起点、不设跳过分支（CLAUDE.md “不恢复可选旧包变量跳过分支”） |
| I9 | 真实安装、升级只在 GitHub-hosted 一次性 Runner 执行，并沿用 `Assert-HostedInstallRunner` |

## 4. 设计

### 4.1 基线登记表

`scripts/fixtures/upgrade_baselines.json`（schema 2）取代 `upgrade_baseline.json`，0.1.0 条目内容原样迁入。每个条目：

| 字段 | 说明 |
| --- | --- |
| `version`、`tag`、`commit` | 版本身份；`commit` 必须等于标签解引用的提交 |
| `origin_release` | 资产实际所在的发布（0.1.0 为 `pre-8d1d90de…`，其余等于 `tag`） |
| `assets` | 安装包、`SHA256SUMS.txt`、`provenance.sigstore.json`（及存在时的 `build-metadata.json`）的名称、URL、SHA256 |
| `profile` | 起点夹具类型：`legacy-cache`（0.1.0）或 `history`（0.2.0 起） |
| `persistent_format` | `cache_schema`、`history_schema`、`parser_revision`、`identity_revision`、`pricing_index`、`settings_format`；不适用的字段为 `null` |
| `status` | `published`、`withdrawn`、`unpublished`（发布流水线失败、从未公开） |
| `withdrawn` | 撤回日期与原因（`status=withdrawn` 时必填） |

0.2.0–0.3.1 的 `persistent_format` 按 §1.2 表格手工登记，审阅时逐项对照标签源码。

### 4.2 持久格式指纹

- 核心库新增 `SETTINGS_FORMAT_REVISION`（当前为 1）。只有需要声明迁移、或改变旧设置解释语义的提交才递增它，并同步 `settings_migrations.json`；新增普通设置字段不算。PR3 的 `dir` 迁移会把它改为 2。
- 新增只用于构建的示例程序 `crates/tokenscope-core/examples/print_persistent_formats.rs`（不进安装包），从真实常量输出六项 JSON，不复制常量数字。生成 `build-metadata.json` 的构建作业运行 `cargo run --locked --release -p tokenscope --example print_persistent_formats`，把标准输出写进 `build-metadata.json`（schema 2，新增 `persistent_format` 字段）。交付记录写明这一步实际增加的耗时。
- 当前构建只接受 schema 2；历史读取保持兼容已公开的 schema 1 与无元数据的 0.1.0。
- 从本 PR 之后发布的版本，登记表里的 `persistent_format` 必须与该版本 `build-metadata.json` 中的值一致，由 `--check` 校验；更早的版本以手工登记为准。

### 4.3 起点选择规则

`scripts/upgrade_baselines.py select --current-version X` 是离线纯函数：只读检出提交的登记表，不访问网络，标准输出只有 JSON（`{"include": [{"origin": "0.1.0", "source_state": "present"}, …]}`，另给链路用的有序版本列表）。规则依次为：

1. 0.1.0 固定入选（最老版本，也是唯一的 `legacy-cache` 起点）；它若不是 `published`，报错要求重新评审基线。
2. **上一版**：`status=published` 且版本低于 X 的条目中最新的一个。
3. **格式代表**：`status=published` 且版本低于 X 的条目按 `persistent_format` 分组，每组取最新一个。
4. 去重后按版本升序（整数三元组）输出。`withdrawn` 与 `unpublished` 条目永远不入选。

对本 PR 的下一补丁版本执行选择：0.3.2 发布并登记前为 {0.1.0, 0.3.1}；之后为 {0.1.0, 0.3.2}。去重后超过 6 个时 `select` 报错，要求回到计划评审，不允许静默扩张。

### 4.4 登记表维护与撤回

| 时机 | 操作 | 检查 |
| --- | --- | --- |
| 准备合入 main，执行 `bump_version.py` 前后 | `python -B scripts/upgrade_baselines.py add --tag v<上一版>`：用 gh 读取该发布，下载每个资产计算 SHA256，读取 `build-metadata.json`，全部通过后才写入条目 | 上一版还没公开时报“等待 v… 发布完成后再登记”；流水线失败、从未公开的版本登记为 `unpublished`（附提交与失败原因），不能把临时 API 错误当成未发布 |
| 每个 PR 和 main 的 CI `gate` | `upgrade_baselines.py --check --base-ref <事件的 base SHA>` | 基础版本必须已登记；每个 `published` 条目在 GitHub 上必须仍存在、标签解引用到登记提交、API digest 与登记值一致（缺 digest 才下载计算）；`withdrawn` 条目的发布仍存在时只提示“待删除”，不判失败；GitHub API 出错即失败，不跳过 |
| 撤回某个版本 | 依次：取消该版本在途的发布作业 → 提交 PR 把它标为 `withdrawn` 并合入 main → 删除发布页。禁止重跑旧发布作业 | 若先删了发布、后登记，登记合入前每个 PR 的 `--check` 都会失败，作为兜底 |
| 发布 | `release_assets.py publish` 在写 GitHub 前读取最新 main 的登记表（只解析 JSON） | 本次将创建的任一版本（含回补的 0.1.0）被标为 `withdrawn`，拒绝发布；读取失败即阻断 |

按 D2，`--check` 放在 CI `gate`。标签受 `protect-release-tags` 保护，撤回只删发布页，不删标签。

### 4.5 单个起点的场景

每个起点 × {`present`, `deleted`} 各一个作业，沿用现有 `Invoke-UpgradeScenario` 的顺序，按 `profile` 分支：

| 步骤 | `legacy-cache`（0.1.0） | `history`（0.2.0 起） |
| --- | --- | --- |
| 准备 | 现状：写入来源目录设置与自定义价格，不写同意 | 同左，另写 `privacy_policy_accepted = true`（这些版本未同意就不会采集） |
| 旧版就绪 | 现有日志模式 | 同一日志模式（§1.2 已核对 0.2.0、0.3.1） |
| 旧事实 | `upgrade_fixture.py verify` 读 `cache.db` | `inspect_history.py --verify-manifest` 读旧 `history.db` |
| 记录 | 旧主程序、`cache.db`、`settings.toml`、`pricing.toml` 哈希 | 旧主程序、`settings.toml`、`pricing.toml` 哈希；旧程序正常退出后复制一份 `history.db` 作证据（新版会写入，不比字节） |
| 升级后 | 现有核对 | 事实按清单核对；按 I4 核对备份；按 I5 核对设置 |
| 同意 | 升级后写入测试同意（现状） | 已存在，不再写入；核对仍为 `true` |

期望清单：登记表条目可指定 `expected_old` 和 `expected_after.{present,deleted}`，缺省时都用 `fixtures/upgrade-0.1.0/expected.json`（当前所有版本解析修订相同，旧事实核对一步会实际验证这一点）。以后解析修订变化时，再为相应版本显式登记期望。

`verify_settings` 改为读取声明式迁移表 `scripts/fixtures/settings_migrations.json`：键为 `从格式→到格式`，值为字段变换规则，只支持恒等、字段重命名/移动、默认值三种操作。本 PR 只落地 1→1 恒等；PR3 增加 1→2 规则（`dir` → `additional_paths`，`enabled` 保持）。未声明的格式组合失败。

### 4.6 连续升级链

只在 Release 流水线（以及 `install-test.yml` 的手动 `workflow_dispatch`）运行，是发布的前置条件。

- 链路：起点集合按版本升序依次覆盖安装，最后装当前包。现在是 0.1.0 → 0.3.1（或 0.3.2）→ 当前。
- 每一跳：校验并安装下一个包 → 核对注册版本与主程序哈希 → 启动并等待就绪 → 按这一跳目标版本的期望核对事实、按 I4 核对本跳备份、核对设置 → 正常关闭。最后一跳另做一次重启幂等核对。
- 两条链并行：`present` 和 `deleted`。整条链只播种一次；`deleted` 链在最老版本采集后删除日志，后续每一跳都只能依赖已存事实，不得重新播种或补写事实。
- 任一跳失败即中止后续跳并使作业失败；`upgrade-result.json` 记录逐跳结果、失败阶段与日志。
- 按 D3，链只经过起点集合（格式代表与上一版），不覆盖全部已发布版本。

### 4.7 工作流

- `install-test.yml` 和 `release.yml` 各增加 `plan-upgrades` 作业，运行 `upgrade_baselines.py select` 输出矩阵；`upgrade-test` 改为 `matrix: { origin, source_state }`，用 `fromJSON` 读取。`select` 失败或输出空集合时 `plan-upgrades` 失败，不能让空矩阵算通过。
- 各作业下载同一次构建的当前安装包，不各自重建。
- `release.yml` 增加 `upgrade-chain`（两个场景），`publish` 依赖单跳矩阵与双链全部成功。
- 作业名会变成 `upgrade-test (0.3.1, present)` 这种形式。它们不在 main 规则集的必需检查里（`install-test.yml` 有路径过滤），规则集不用改；路径过滤需覆盖登记表、迁移表、升级脚本与工作流。
- 证据上传沿用现有命名，加上起点版本（链路再加跳序号）。

预计耗时（旧场景估算，交付时回填实测）：PR 侧并行 4 个作业，墙钟约 1–2 分钟；Release 侧两条链各 3–4 分钟，与安装测试并行。

### 4.8 原生验收实例生命周期（UP09，2026-10-16 并入）

**起因（事实）：** main `19aae8b` 的 [Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/38768858040) 失败。普通 `ui-system` 14/14 通过；100% 缩放场景首次启动、最小化恢复、设置保存、滚动位置均通过，点击“直接退出”后旧进程退出码 0；重启后连接同一端口 9452 时失败：`browserType.connectOverCDP: socket hang up`（retrieving websocket url）。重启后的设置持久化、托盘检查未完成，后续缩放场景未执行。同一提交在 PR 阶段曾因启动连接链路的 `about:blank` 超时失败，重跑后通过并据此合入。现有工件没有端口所属进程、新实例退出状态等证据，不能区分“旧 WebView2 端点残留”与“新 WebView2 启动异常”。

**代码缺口（`frontend/scripts/native-acceptance.mjs`）：**

- `contracts/native-ui-system.mjs` 的重启调用 `inst.launch()` 沿用原端口；`ui-e2e.yml` 的 4 个 `ui-system` 进程也先后都用 9452。
- `launch` 只要 `/json/version` 返回一次就立即 `connectOverCDP`，不确认旧实例已退出、端点属于新实例；探测成功与连接之间端点关闭时直接失败。
- `stop()` 只结束直接子进程，等 600 ms，不确认是否真的退出。
- 首次启动失败时在写 `evidence.json` 之前就抛出（`:1543`），没有工件。

**要求：**

1. **端口不复用。** 进程内第 n 次启动用 `--port + n`。`ui-e2e.yml` 给各步互不重叠的起始端口：ui-refresh 9451，四个 `ui-system` 分别 9460、9470、9480、9490。启动前端口已有监听者即失败并记录占用进程，不自动换端口。
2. **确认退出。** 每次关闭（重启前和场景结束时）都等宿主及其全部后代进程（启动后按父 PID 递归记录）退出、端口不再监听，上限 20 秒；超时失败并列出残留进程。强制结束只用于失败后的清理，且只针对已记录的 PID。
3. **核实端点归属。** 连接前查询监听该端口的进程，必须是宿主或其已记录的后代；没有监听者就继续等，属于别的进程就失败。
4. **启动预算与重试。** 单次启动从启动进程到拿到应用页面共 65 秒，等于现有两段上限之和（CDP 就绪 45 秒 + 页面 target 20 秒），不收紧也不放宽。期间对连接类瞬断（`socket hang up`、`ECONNRESET`、`ECONNREFUSED`、target 被销毁）每 300 ms 重试，重试前确认宿主仍存活、端口归属不变；宿主一旦退出立即失败并报告退出码。
5. **证据无论成败都写出。** 启动和场景执行包进 try/finally，首次启动失败也写 `evidence.json`。新增 `launches[]`，每项记录：序号、端口、宿主 PID 与退出码、后代 PID、端口所属 PID、每次连接尝试的时间与错误、页面 target URL、各阶段耗时。失败时附宿主日志尾部。
6. **纯逻辑可单测。** 端口计算、归属判定、重试判断抽到 `frontend/scripts/lib/native-instance.mjs`，进程与网络查询以参数传入；`frontend/vitest.config.ts` 纳入 `scripts/lib/**/*.test.mjs`（该测试文件用 node 环境），由 `pnpm --dir frontend test` 运行。进程与监听表查询沿用脚本现有方式（一次 PowerShell 调用 `Get-CimInstance Win32_Process` 与 `Get-NetTCPConnection`），不另建适配器脚本。
7. **本地压力验证。** 新增场景 `ui-relaunch`，参数 `--relaunch-cycles N`（默认 20）：首次启动后每轮“直接退出 → 确认退出 → 重启 → 等待数据 → 核对设置仍在”。**只在本地运行，不进 CI**（D5）；结果写进交付记录。
8. **重跑不能替代原因。** 从本 PR 起，Native UI E2E 失败时先依据证据说明原因再处理：属于测试架缺陷的先修复，属于产品问题的按缺陷处理，不能只靠重跑通过就合并。写进 CLAUDE.md 提交流程。

`contracts/native-ui-system.mjs` 的重启走同一个 `launch`/`stop`；`expire-finish` 的 attach 模式不认领、不结束外部实例。原有检查项名称与数量不减，不改任何场景的断言。

## 5. 任务拆分

实施顺序：UP01/UP02 → UP03 → UP04 → UP05/UP06 → UP07/UP08；UP09 独立，可并行。每项先写能红的反例，再做最小实现。

| 编号 | 任务 | 完成标准 |
| --- | --- | --- |
| UP01 | 登记表 schema 2：迁入 0.1.0，补登 0.2.0–0.3.1（发布完成后再加 0.3.2） | 每个条目的 digest 与 GitHub 资产一致，`commit` 与标签解引用一致；`persistent_format` 与 §1.2 一致 |
| UP02 | `upgrade_baselines.py`：`select`、`add`、`--check` | §4.3 的选择表驱动用例全部通过；§6.1 的反例全部能被检出 |
| UP03 | 持久格式指纹：`SETTINGS_FORMAT_REVISION`、`print_persistent_formats`、`build-metadata.json` schema 2 | Release 构建产物带 `persistent_format`；`release_assets.py verify` 校验它；新增登记项与元数据不一致时 `--check` 失败 |
| UP04 | 夹具与编排按 `profile` 分支；备份规则；声明式设置迁移 | `history` 起点的准备、旧事实、升级后核对都有契约测试；1→1 恒等迁移通过；现有 0.1.0 两场景仍通过 |
| UP05 | 两个工作流改为动态矩阵 | `test_upgrade_workflow.py` 校验两边都使用 `select` 的输出、矩阵绑定起点与场景、空矩阵与失败会传到作业结果 |
| UP06 | Release 连续升级链 | 两条链在 main 的 Release 中通过；`publish` 依赖链 |
| UP07 | 撤回处理：`withdrawn` 状态、发布脚本拒绝发布、撤回流程写进文档 | 契约测试：最新 main 登记为撤回时 `publish` 不发生任何创建调用；撤回后 `select` 不再选它 |
| UP08 | 文档：CLAUDE.md“发布与升级”一节、`docs/plans/README.md`、整合计划 N12 链接 | 写明“合入前先登记上一版”和 §4.4 的撤回顺序 |
| UP09 | 原生验收实例生命周期（§4.8） | §6.2 反例单测全部通过；本地 `ui-relaunch --relaunch-cycles 20` 通过，结果写进交付记录；PR 上 Native UI E2E 通过；CLAUDE.md 写明“重跑不能替代原因” |

## 6. 反例（契约测试必须能检出）

### 6.1 升级矩阵

| 反例 | 期望 |
| --- | --- |
| 登记的 SHA256 与下载文件不一致 | 执行旧安装包前失败 |
| 登记表缺少基础版本 | `--check` 失败并提示运行 `add` |
| 已登记 `published` 的版本在 GitHub 上已删除或附件被替换 | `--check` 失败并提示标为 `withdrawn` |
| 撤回的版本被选为起点 | `select` 不输出它 |
| 最新 main 登记为撤回的版本被重新发布 | `publish` 拒绝 |
| 两个版本持久格式不同却被当成同一组 | 选择用例失败 |
| 起点数超过 6 | `select` 报错 |
| `history` 起点结构未升级却新增了备份，或升级了却没备份 | 夹具核对失败 |
| 设置字段在迁移表之外被改动 | `verify_settings` 失败 |
| 工作流里出现跳过起点的条件分支，或空矩阵被当作通过 | `test_upgrade_workflow.py` 失败 |

### 6.2 原生验收重启（UP09）

| 输入（注入的进程/网络状态） | 必须观察到 |
| --- | --- |
| 旧宿主已退出，其 WebView2 子进程仍监听旧端口 | 新实例使用新端口；重启前等待旧子进程退出，超时则失败并列出残留 PID |
| 新端口上有监听者，但不是新宿主或其后代进程 | 不连接、不结束外来进程；明确失败，证据记录归属不符 |
| `/json/version` 成功后第一次 `connectOverCDP` 报 `socket hang up`，第二次成功 | 在期限内重试成功；`launches[]` 记录两次尝试 |
| 连接重试期间宿主退出（退出码 0 或非 0） | 立即失败并报告退出码，不再重试 |
| 一直连不上直到 65 秒期限 | 按原失败处理，总时限不延长，证据齐全 |
| 页面 target 先是 `about:blank`，随后才挂上 tauri 页面 | 在期限内等到 tauri 页面；超时失败时记录 target 列表 |
| 首次启动失败或场景中途抛错 | `evidence.json` 仍写出 `launches[]` 与宿主日志尾部 |

## 7. 验证与交付门槛

1. 本地：脚本契约全集、两个语法检查器、`cargo test --locked --workspace`（UP03 涉及核心常量与示例程序）全部通过；按 CLAUDE.md 跑完整提交门禁；`ui-relaunch --relaunch-cycles 20` 通过。
2. PR：同一 HEAD 的 CI、Native UI E2E、Install test 全部通过，升级矩阵的所有起点 × 场景通过，每个作业的 `upgrade-result.json` 写明起点版本与提交。Native UI E2E 若失败，按 §4.8 第 8 条先说明原因。
3. 合入后：main 的 Release 中两条升级链通过，`v0.x.y` 才发布；把这次运行链接回填到本计划。
4. 未取得第 2、3 步远程证据前，不能写“升级矩阵已验收”。

## 8. 与其他计划的关系

- **PR3**：本计划先合入时，PR3 的 `dir` → 追加路径迁移会自动得到 0.3.x 真实安装包的升级测试；PR3 需要同时把 `SETTINGS_FORMAT_REVISION` 改为 2，并在 `settings_migrations.json` 增加 1→2 规则。若 PR3 先合入，本计划实施时必须补上这一条迁移规则，否则 0.3.x 起点的设置核对会失败。PR3 计划第 120 行的“隔离升级用例”仍由 PR3 自己负责，本计划只提供真实安装包这一层。
- **PR2**：不改持久格式，升级矩阵不受影响。PR2 的 F04 会扩展 `native-acceptance.mjs`，必须在本计划合入后基于新的实例生命周期实现，不得绕开 `launches[]` 证据或另写一套启动逻辑。
- **整合计划 N12**：0.1.0 单跳场景保留为矩阵中的一个起点，原有验收结论不变。

## 9. 决定（用户 2026-10-16 确认）

| 编号 | 问题 | 选项 | 决定 |
| --- | --- | --- | --- |
| D1 | 起点集合 | (a) 0.1.0 + 上一版 + 每种持久格式的代表；(b) 全部已发布版本 | (a)：覆盖真实风险，起点数只随格式变化增长；(b) 每发一版加一个作业 |
| D2 | `--check` 放在哪里 | (a) CI `gate`，每个 PR 都检查；(b) 只在升级作业和 Release | (a)：保证每个 PR 都登记了上一版；代价是上一版的 Release 完成前，下一个 PR 的 gate 会失败。有编译缓存时 Release 约 18 分钟（0.3.1 为 17 分 53 秒） |
| D3 | 连续升级链 | (a) 只在 Release，经过格式代表；(b) Release 中经过全部已发布版本；(c) PR 中也跑 | (a)：多跳风险集中在格式变化处；(b) 每多一个版本多 1 分钟左右 |
| D4 | 撤回版本还要不要测 | (a) 撤回即删除发布，此后不再测试；(b) 撤回只改说明、保留安装包，并继续作为额外起点测试 | (a)：符合“在发布页撤回”的用法，规则简单；如果某次撤回的版本已有不少人安装，可以对那一个版本临时选 (b) |
| D5 | 20 次重启压力验证在哪里跑 | (a) 只在本地跑一次作为本 PR 取证；(b) 常驻 CI，每次都跑 | (a)：用户决定。常驻会让必需检查每次多数分钟，属于过度防御；CI 中由各场景原有的重启覆盖这条链路 |

## 10. 实施记录（2026-10-16）

本节的结论只覆盖本地实现与脚本契约；§7 第 2、3 步的远程证据仍未取得。

### 10.1 起点登记表与选择器（UP01/UP02/UP07）

- `scripts/fixtures/upgrade_baselines.json`（schema 2）取代 `upgrade_baseline.json`：0.1.0 原样迁入，补登 0.2.0、0.2.1、0.3.0、0.3.1、0.3.2。每条 `commit` 与标签解引用一致；`persistent_format` 按各标签源码常量登记（0.2.0 起 cache 11 / history 4 / parser 1 / identity 1 / pricing 9 / settings 1；0.1.0 为 cache 6 / pricing 8，history 系列与修订为 null）。
- 23 个登记资产的 SHA256 与 GitHub Release 资产 digest 逐条一致。
- `scripts/upgrade_baselines.py`：`select` 是离线纯函数（0.1.0 固定入选 + 上一版 + 每种持久格式的最新代表，去重按版本升序，超过 6 个报错），输出 `origins`、`chain`、`include`；`add` 逐个下载资产核对哈希后才写入（缺 `persistent_format` 的旧元数据要求手工登记，`--unpublished` 需附提交与原因）；`withdraw` 标记撤回；`check` 校验基础版本已登记、标签解引用、远端资产集合与哈希、schema 2 元数据的持久格式一致，GitHub 出错即失败，撤回条目发布仍在只提示“待删除”。
- `release_assets.py`：登记表统一校验（`load_registry`）、`download-baseline --origin <版本>`、`print-persistent-format`、`collect` 必带 `--persistent-format`、`verify` 要求元数据 schema 2、`publish` 先读最新 main 的登记表并拒绝登记为 `withdrawn` 的版本（`assert_publishable`）。

### 10.2 持久格式指纹（UP03）

- 核心库新增 `persistent_format` 模块与 `SETTINGS_FORMAT_REVISION = 1`；六项全部读真实常量（`cache::SCHEMA_VERSION` 改为 `pub`），不复制数字。
- 新增构建用示例 `crates/tokenscope-core/examples/print_persistent_formats.rs`（不进安装包），输出一行紧凑 JSON，键序固定。
- 两个构建作业在 `collect` 前跑 `release_assets.py print-persistent-format`，结果写进 `build-metadata.json` 的 `persistent_format`（schema 2）。发布后登记的新版本由 `--check` 校验登记值与元数据一致。

### 10.3 夹具、编排与备份规则（UP04）

- `upgrade_fixture.py prepare --profile legacy-cache|history`：`history` 起点预置隐私同意，`legacy-cache` 保持不写；新增幂等 `ensure-consent`（0.1.0 升级后与链路每一跳使用）。
- 设置核对改为声明式：`scripts/fixtures/settings_migrations.json`（本 PR 只有 `1->1` 恒等），支持恒等、rename、default 三种操作；未声明的格式组合、迁移表之外的字段改动、迁移后取值不一致都失败。
- 编排按 `profile` 分支：旧事实（`cache.db` 用 `upgrade_fixture.py verify`，`history.db` 用 `inspect_history.py --verify-manifest`）、记录（0.1.0 额外比对 `cache.db` 字节与只读性；0.2.0 起复制旧库作证据、不作字节比对）、升级后核对。
- I4 备份规则：`inspect_history.py --schema-version`（读 `PRAGMA user_version`）给出本跳安装前的结构版本，`Assert-HistoryBackup` 据此判定——结构升级必须有 `history.db.backup-v<旧结构>` 且只读可打开并含旧事实，结构未变不得出现同名备份；0.1.0 起点首次建库不适用。
- 起点、期望清单、设置格式都不再写死：`New-UpgradeCase`/`Set-UpgradeJump` 读登记表条目；`upgrade-result.json`（schema 2）写明起点版本与提交、夹具类型、四桶哈希、旧结构版本、备份名与逐跳结果。

### 10.4 工作流与链路（UP05/UP06）

- 两个工作流各加 `plan-upgrades`（离线 `select`，空集合直接抛错），`upgrade-test` 改为 `matrix.include: ${{ fromJSON(needs.plan-upgrades.outputs.include) }}`，作业名形如 `upgrade-test (0.1.0, present)`；起点与场景只来自矩阵，工作流不写死版本、无跳过分支。
- `release.yml` 新增 `upgrade-chain`（`present`/`deleted` 两条链），`publish` 依赖单跳矩阵与双链全部成功。
- 链路只播种一次；`deleted` 链在第一跳采集后删除来源日志，后续跳只能依赖已存事实。逐跳：下载校验并安装 → 核对注册版本与主程序哈希 → 启动等待就绪 → 核对事实、I4 备份与设置 → 正常关闭；最后一跳再做一次重启幂等核对；任一跳失败即中止并写出逐跳结果。

### 10.5 本地验证（UP01–UP08）

| 项 | 结果 |
| --- | --- |
| `python -B scripts/upgrade_baselines.py select --current-version 0.3.3` | `origins = ["0.1.0", "0.3.2"]`，`include` 四项 |
| `python -B scripts/upgrade_baselines.py check --base-ref origin/main` | 6 个条目、23 个远端资产的标签解引用与哈希全部通过，无警告 |
| `python -B -m unittest discover -s scripts -p "test_*.py"` | 262 项通过 |
| `pwsh -NoProfile -File scripts/test_upgrade_helpers.ps1` | 49 项通过（含 I4 两种反例、链路只播种一次、失败即中止） |
| `pwsh -NoProfile -File scripts/check_powershell_syntax.ps1` | 12 个脚本通过 |
| `python -B scripts/release_assets.py print-persistent-format` | 六项指纹与本计划 §1.2 及登记表一致 |

### 10.6 原生验收实例生命周期（UP09）

- 新增 `frontend/scripts/lib/native-instance.mjs`：端口计算、端口归属判定、连接重试决策、退出确认与证据拼装的纯逻辑；进程与网络查询以参数注入，沿用脚本原有一次 PowerShell 调用（`Get-CimInstance Win32_Process` + `Get-NetTCPConnection`），未另建适配器脚本。新增 `frontend/scripts/lib/native-instance.test.mjs`（27 项，覆盖 §6.2 全部反例，node 环境），`frontend/vitest.config.ts` 纳入 `scripts/lib/**/*.test.mjs`。
- `native-acceptance.mjs` 重写实例生命周期：启动前查端口占用（有监听者即失败并记录占用进程，不自动换端口）；连接前核实端点归属（属于外来进程即失败且不结束它）；连接类瞬断每 300 ms 重试，重试前确认宿主存活与端口归属，宿主退出立即失败并报退出码；单次启动 65 秒预算不收紧不放宽；每次关闭等宿主及其全部后代进程退出且端口不再监听（上限 20 秒，超时列出残留 PID，强制结束只用于失败清理且只针对已记录 PID）。启动与场景执行包进 try/finally，失败也写 `evidence.json`（新增 `launches[]`：序号、端口、宿主 PID 与退出码、后代 PID、端口所属 PID、连接尝试与错误、页面 target、阶段耗时，另附宿主与应用日志尾部）。
- `ui-e2e.yml` 各步起始端口互不重叠（ui-refresh 9451、四个 ui-system 9460/9470/9480/9490），进程内第 n 次启动用 `--port + n`；新增只在本地运行的 `ui-relaunch` 场景（`--relaunch-cycles`，默认 20）。
- **本地取证**：`node frontend/scripts/native-acceptance.mjs --scenario ui-relaunch --events 4 --relaunch-cycles 20 --port 9640 --out qa-artifacts/up09-relaunch-local` 退出码 0——21 次启动端口 9641…9661 无重复，20 轮“直接退出 → 确认退出 → 重启 → 等待数据 → 核对设置”每轮退出码 0 且设置仍在，8/8 检查通过（含新增 `instance_stop_confirmed`），真实用户数据未被触碰；既有 `ui-system` 场景回归 15/15 通过（原 14 项名称与数量不减），attach 模式实测不认领、不结束外部实例。证据与日志留在本机 `qa-artifacts/up09-relaunch-local/`（不入库）。
- 实测纠正了两处与计划假设不符的事实：`Get-NetTCPConnection` 的 `State` 在 JSON 里是数字枚举（只认字符串会把真实监听者判成“无监听者”），undici 的 `fetch failed` 把真实原因放在 `cause`（错误链展开后 `ECONNREFUSED` 等才按可重试处理）。两处都补了单元测试。
- CLAUDE.md 已写明“Native UI E2E 失败先按证据说明原因，重跑不能替代原因”，并记录 20 轮压测只在本地取证。

### 10.7 远程门禁（PR #10）

- PR：[#10](https://github.com/ywltby/tokenscope/pull/10)（base `main`，head `docs/upgrade-matrix-plan`，最终提交 `34518ef`）。
- 同一 HEAD 的远程结果（2026-10-16）：CI `gate` 18m56s 通过（含新增的登记表远端 `check` 步骤）；Install test `install-test` 11m39s 通过，`plan-upgrades` 12s 从登记表选出两个起点，四个单跳作业全部通过——`upgrade-test (0.1.0, present)` 54s、`(0.1.0, deleted)` 1m4s、`(0.3.2, present)` 58s、`(0.3.2, deleted)` 1m4s；`upgrade-chain` 按设计在 PR 上跳过（仅手动触发）；Native UI E2E `native-ui` 11m30s 通过——main `19aae8b` 上失败的同一检查在新的实例生命周期下通过。
- 取消逻辑修复后（HEAD `10364fc`）同一组门禁重跑仍全部通过：`gate` 20m59s、`install-test` 11m34s（`plan-upgrades` 9s，四个单跳作业 57s–1m17s）、`native-ui` 13m4s。
- 仍未取得：合入 main 后 Release 的两条连续升级链、`v0.3.3` 发布与五场景安装的真实签名；这些按 §7 第 3 步在发布批次里取证。

### 10.8 复核修复（2026-10-16 第二轮）

独立复核对第 10.1–10.6 节提出 5 项问题，逐项修复并补了能红的契约：

| 复核项 | 修复 | 新增契约 |
| --- | --- | --- |
| P1：0.3.2 起点的旧期望清单位缺失（`ExpectedOldManifest` 未初始化，真实升级会失败） | `New-UpgradeCase` 从登记表 `expected_old` 初始化该字段 | 真实 Case + 真实 `history.db` 跑真实 `Check-OldUsage`（经 `inspect_history.py --verify-manifest`）通过；`ExpectedOldManifest` 与登记值一致 |
| P1：撤回保护读的是工作区登记表（工作流检出的是发布提交），最新 main 上的撤回看不到 | `publish` 改为 `latest_main_registry()`：先取 main 的固定提交 SHA，再按该 SHA 读登记表并校验结构，任何读取失败都阻断发布 | “本地表允许、最新 main 已撤回”仍拒绝且不发生任何远端写操作；main 读取失败即阻断；`unpublished` 的当前版本同样拒绝 |
| P1：连续升级链实际跳过 history.db 备份验收（判定按起点夹具类型，链路起点永远是 `legacy-cache`） | 备份判定改按**本跳安装前的旧库状态**（`OldHistorySchema`）与升级前备份清单（`BackupsBefore`）：结构升级必须新增且不得覆盖同名备份，结构未变不得新增或改写任何备份；`Check-CurrentUsage` 无条件执行备份核对 | `Profile=legacy-cache` 且已有历史库时非法备份必须失败；结构升级覆盖已有备份失败；结构未变新增/改写备份失败；链路每一跳都带旧库状态并进入核对路径 |
| P2：登记 `unpublished` 版本会让 `entries` 崩溃；`--check` 又强制基础版本必须 `published`，与“发布流水线失败也要登记”的流程冲突 | `resolved_entry` 对无资产条目输出空的安装包字段；`--check` 只要求基础版本**已登记**，非 `published` 改为结果里的显式警告 | `unpublished` 条目仍可解析出版本、状态与期望清单路径；基础版本 `unpublished`/`withdrawn` 通过并带警告，且不作为起点 |
| P2：原生连接预算与退出即时检测没有落到实际异步调用 | 见 10.9 | 见 10.9 |

### 10.9 原生连接预算与退出即时检测（复核 P2）

- 纯逻辑（`frontend/scripts/lib/native-instance.mjs`）：新增 `remainingBudgetMs` / `clampTimeoutToBudget`（自身超时取 `min(max(自身超时, 下限), 剩余预算)`，预算耗尽返回 0）、`HostExitError` / `BudgetExhaustedError`、`createHostExitSignal`（子进程 `exit` 事件与 250 ms 存活轮询取先到者）、`awaitWithHostExit`（目标完成 / 宿主退出 / 预算耗尽三者竞速，被取消时把迟到的成功结果交给清理回调而不回传）、`createProbeFn`（探测自身 30 秒超时被剩余预算截断，预算耗尽不 spawn，宿主退出立刻结束探测）。
- 驱动（`frontend/scripts/native-acceptance.mjs`）：进程探测改为受预算约束的 `createProbeFn`；`/json/version`、`connectOverCDP` 与两次重试间隔、页面 target 轮询全部走 `budgetedConnect` / `waitOrExit`，取消时 `AbortController.abort()` 与 `browser.close()` 关闭底层连接，被取消的尝试在 `launches[]` 里标 `aborted`，成功结果不写入 `this.browser`；`launch` 启动前解除旧监听、启动后装配宿主退出信号并在页面阶段使用本阶段截止时刻；`stop` / `attachToExisting` 结束时解除监听（attach 仍不认领、不结束外部实例）。证据新增 `launch_deadline_ms` / `phase_deadline_ms` / `host_exit_detected` / `budget_ms`。
- 新增 `frontend/scripts/lib/native-instance.async.test.mjs`（16 项真实异步组合：真实子进程退出被事件与轮询检出、退出码 0 与非 0 都立即失败、预算耗尽不再发起第二次连接、探测被预算截断、竞速超时后迟到连接被关闭且不算新尝试、`socket hang up` 后第二次成功且两次尝试入证据）。
- 复核点名之外另有 3 处同类缺陷一并修：`fetchJsonVersion` 的超时被下限抬出预算、连接重试的间隔等待与页面轮询在等待期完全不感知宿主退出。
- 本地实跑：`pnpm --dir frontend test` 42 文件 / 523 项通过（既有 507 项不减），`typecheck` / `format:check` / `build` / `node --check` 通过；`--scenario ui-system --port 9700` 15/15 通过（`real_untouched=true`）；故障注入 `--exe cmd.exe` 在启动后 17 ms 报「宿主在启动阶段退出（退出码=0）」且证据里 `connect_attempts[0].aborted="host-exit"`，`--exe notepad.exe`（不监听端口）在 45006 ms 报「启动预算 45000 ms 用尽」——总时限未被放宽。
- **第二轮复核补充（同一项）**：`awaitWithHostExit` 原先只在**迟到成功**时执行清理回调，超时与宿主退出分支只是 reject，取消动作要等底层自己结束才发生（复核复现：21 ms 时取消回调 0 次）。现拆成两个回调——`onCancel` 在**取消当下**执行（`fetchJsonVersion` 用它 `AbortController.abort()`），`onLateResult` 只收尾迟到的成功结果（关闭已建立的连接）；预算已耗尽时底层任务不启动，但仍然给出取消结论。`budgetedConnect` 上移为核心库的 `connectWithBudget`，把同一份剩余预算作为 `connectOverCDP` 的 `{ timeout }` 传给底层，取消后迟到的连接仍被关闭。新增 6 项断言（含「宿主退出后真实 `fetch` 立刻被 abort、服务端观察到连接关闭」「注入的底层 `connect` 收到与预算相等的 `timeout`」「预算耗尽时任务不启动但仍取消」），并用注入反例确认去掉取消调用后其中 5 项会红。
- 本地复跑：`pnpm --dir frontend test` 42 文件 / **529 项**通过；`--scenario ui-system --port 9710` 15/15 通过（证据含 `host_exit_detected`、`budget_ms`、`stop`），真实用户数据未被触碰。
