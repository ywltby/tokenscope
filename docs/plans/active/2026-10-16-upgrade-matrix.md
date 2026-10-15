# 升级测试补测：起点矩阵、连续升级链与原生验收重启修复

> 状态：**用户已确认（2026-10-16）**，第 9 节 D1–D4 均按建议选 (a)；同日用户决定把原生验收重启连接修复（UP09，§4.8）并入本计划，不推迟到 PR2。均未实施。
> 交付：独立 PR，patch（测试与发布门禁维护，不改变统计口径和界面）。实施时从最新 main 建 `feat/upgrade-matrix`。
> 顺序：**下一个 PR**，在 [PR2 扫描反馈](2026-10-15-collection-status.md)之前合入。PR2 的 F04 要扩展同一个原生验收脚本，先修好重启连接，PR2 才能在稳定的测试架上取证；也因此早于 [PR3 来源目录](2026-10-14-source-discovery-and-additional-paths.md)，PR3 的设置迁移能直接得到真实 0.3.x 安装包的升级覆盖（见 §8）。
> 承接：[整合计划 N12](2026-10-11-consolidated-remaining-work.md) 的“真实旧包升级”责任；本计划只扩展起点与升级次数，不改变 N12 已验收的 0.1.0 单跳场景。

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
- G4 起点由仓库内登记表决定，可复现；被撤回的版本不作为默认起点；撤回后不会被旧流水线重新发布。
- G5 旧安装包始终是原发布资产，执行前校验写死的 SHA256，不重新构建历史包。

**非目标**

- 降级测试（NSIS 已设 `allowDowngrades: false`，较高结构版本拒绝打开已有单元测试）。
- `pre-*` 中间提交包作为起点（只有 0.1.0 的来源资产例外，沿用现状）。
- 卸载、真实用户数据、本机真实安装。
- 改变统计口径、迁移逻辑或界面。

## 3. 不变量

| 编号 | 内容 |
| --- | --- |
| I1 | 旧安装包只能来自登记表里的原资产 URL，下载后、执行前 SHA256 必须与登记值一致；不一致立即失败，不重试别的来源 |
| I2 | 旧数据只能由旧程序自己采集产生，不合成旧库（沿用 `upgrade_fixture.py` “绝不生成旧缓存”） |
| I3 | 升级后请求事实与该起点、该场景的期望清单逐条一致（含四桶），重启后不增量；`pricing.toml` 字节不变；旧 `cache.db` 只读不变 |
| I4 | 历史库结构发生升级时，`history.db.backup-v{旧版本}` 必须存在、可只读打开、包含旧事实；结构未变时不得新增或覆盖备份，允许保留此前跳的备份（§10.4） |
| I5 | 设置按“起点设置格式 → 当前设置格式”的声明式迁移规则核对：规则内的字段按规则变化，其余字段原值保留 |
| I6 | 起点集合只由已检出提交中的登记表和当前版本号计算，CI 不按 GitHub 列表动态挑选；GitHub 只用于一致性校验 |
| I7 | 被撤回的版本不进入默认起点；发布脚本拒绝重新发布登记为撤回的版本 |
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
| `persistent_format` | `cache_schema`、`history_schema`、`parser_revision`、`identity_revision`、`pricing_index`、`settings_format` |
| `status` | `published`、`withdrawn`、`unpublished`（发布流水线失败、从未公开） |
| `withdrawn` | 撤回日期与原因（`status=withdrawn` 时必填） |

0.2.0–0.3.1 的 `persistent_format` 按 §1.2 表格手工登记，审阅时逐项对照标签源码。

### 4.2 持久格式指纹

- 核心库新增 `SETTINGS_FORMAT_REVISION`（当前为 1）。之后凡是改变设置读取或迁移语义的提交，都必须递增它；PR3 的 `dir` 迁移会把它改为 2。
- 新增一个只用于构建的示例程序 `print_persistent_formats`（不进安装包），输出上面六项常量的 JSON。Release 构建作业运行它，把结果写进 `build-metadata.json`（schema 2，新增 `persistent_format` 字段）。
- 从本 PR 之后发布的版本，登记表里的 `persistent_format` 必须与该版本 `build-metadata.json` 中的值一致，由 `--check` 校验；更早的版本以手工登记为准。

### 4.3 起点选择规则

`scripts/upgrade_baselines.py select --current-version X` 输出 JSON，规则依次为：

1. 0.1.0 固定入选（最老版本，也是唯一的 `legacy-cache` 起点）；若它被撤回或尚未公开，报错要求重新评审基线，不绕过状态约束强行选入。
2. **上一版**：`status=published` 且版本低于 X 的条目中最新的一个。
3. **格式代表**：`status=published` 且版本低于 X 的条目按 `persistent_format` 分组，每组取最新一个。
4. 去重后按版本升序输出。`withdrawn` 与 `unpublished` 条目永远不入选。

对本 PR 的下一补丁版本执行选择：0.3.2 发布并登记前为 {0.1.0, 0.3.1}；之后为 {0.1.0, 0.3.2}。固定最老起点与各格式代表去重后计数（上一版通常已是所在格式的代表），不能把 legacy 格式重复计数。超过 6 个时 `select` 报错，要求回到计划评审，不允许静默扩张。

### 4.4 登记表维护

| 时机 | 操作 | 检查 |
| --- | --- | --- |
| 准备合入 main，执行 `bump_version.py` 前后 | `python -B scripts/upgrade_baselines.py add --tag v<上一版>`：用 gh 读取该发布的资产 digest 和 `build-metadata.json`，写入条目 | 上一版还没公开时报“等待 v… 发布完成后再登记”；流水线失败、从未公开的版本登记为 `unpublished` |
| 每个 PR 的 CI | `upgrade_baselines.py --check --base-ref origin/main` | 基础版本（origin/main 的版本）必须已登记；每个 `published` 条目在 GitHub 上必须仍存在且 digest 一致；不一致时提示标为 `withdrawn` |
| 在发布页撤回某个版本后 | 下一个 PR 把它标为 `withdrawn`，填写日期与原因 | `--check` 会发现发布已删除或附件已变，所以撤回必然在下一个 PR 中登记 |
| 发布 | `release_assets.py publish` 校验检出版本的登记表，并读取最新 main 的撤回状态（仅解析 JSON，不执行远程代码） | 当前版本或即将回补的历史版本若被标为 `withdrawn`，拒绝发布；读取失败即阻断。旧流水线不一定包含此检查，不能宣称它们也已被修复，处理见 §10.3 |

按 D2，`--check` 放在 CI `gate`，每个 PR 都运行。

### 4.5 单个起点的场景

每个起点 × {`present`, `deleted`} 各一个作业，沿用现有 `Invoke-UpgradeScenario` 的顺序，按 `profile` 分支：

| 步骤 | `legacy-cache`（0.1.0） | `history`（0.2.0 起） |
| --- | --- | --- |
| 准备 | 现状：写入来源目录设置与自定义价格，不写同意 | 同左，另写 `privacy_policy_accepted = true`（这些版本未同意就不会采集） |
| 旧版就绪 | 现有日志模式 | 同一日志模式（§1.2 已核对 0.2.0、0.3.1）；登记表为每个 `profile` 固定模式 ID，以后格式变化时按起点选用 |
| 旧事实 | `upgrade_fixture.py verify` 读 `cache.db` | `inspect_history.py --verify-manifest` 读旧 `history.db` |
| 记录 | 旧主程序、`cache.db`、`settings.toml`、`pricing.toml` 哈希 | 旧主程序、`settings.toml`、`pricing.toml` 哈希；`history.db` 只读复制一份作证据（新版会写入，不比字节） |
| 升级后 | 现有核对 | 事实按清单核对；按 I4 核对备份；按 I5 核对设置 |
| 同意 | 升级后写入测试同意（现状） | 已存在，不再写入；核对仍为 `true` |

期望清单：登记表条目可指定 `expected_old` 和 `expected_after.{present,deleted}`，缺省时都用 `fixtures/upgrade-0.1.0/expected.json`（当前所有版本解析修订相同，结果一致）。以后解析修订变化时，`present` 按当前解析结果、`deleted` 按旧版已存事实经迁移后的结果分别给出，并由契约测试核对两者的差异只来自声明的修订。

`verify_settings` 改为读取声明式迁移表 `scripts/fixtures/settings_migrations.json`：键为 `从格式→到格式`，值为字段变换规则。1→1 为恒等；PR3 增加 1→2 规则（`dir` → `additional_paths`，`enabled` 保持）。

### 4.6 连续升级链

只在 Release 流水线（以及手动 `workflow_dispatch`）运行，是发布的前置条件。

- 链路：起点集合按版本升序依次覆盖安装，最后装当前包。现在是 0.1.0 → 0.3.1（或 0.3.2）→ 当前。
- 每一跳：装下一个包 → 核对注册版本与主程序哈希 → 启动并等待就绪 → 核对事实、备份与设置 → 正常关闭。最后一跳另做一次重启幂等核对。
- 两条链并行：`present` 和 `deleted`。`deleted` 链在最老版本采集后立即删除日志，后续每一跳都只能依赖已存事实，专门检验多次迁移不丢数据。
- 按 D3，链只经过起点集合（格式代表与上一版），不覆盖全部已发布版本。

### 4.7 工作流

- `install-test.yml` 和 `release.yml` 各增加 `plan-upgrades` 作业，运行 `upgrade_baselines.py select` 输出矩阵；`upgrade-test` 改为 `matrix: { origin, source_state }`，用 `fromJSON` 读取。
- `release.yml` 增加 `upgrade-chain`（两个场景），`publish` 依赖它。
- 作业名会变成 `upgrade-test (0.3.1, present)` 这种形式。它们不在 main 规则集的必需检查里（`install-test.yml` 有路径过滤），规则集不用改。
- 证据上传沿用现有命名，加上起点版本。

预计耗时：PR 侧并行 4 个作业，墙钟时间仍约 1–2 分钟；Release 侧加两条链，每条 3–4 分钟，并与安装测试并行。公开仓库有 20 个并发作业的额度，足够。

### 4.8 原生验收实例生命周期（UP09，2026-10-16 并入）

**起因（事实）：** main `19aae8b` 的 [Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/38768858040) 失败。普通 `ui-system` 14/14 通过；100% 缩放场景首次启动、最小化恢复、设置保存、滚动位置均通过，点击“直接退出”后旧进程退出码 0；重启后连接同一端口 9452 时失败：`browserType.connectOverCDP: socket hang up`（retrieving websocket url）。重启后的设置持久化、托盘检查未完成，后续缩放场景未执行。同一提交在 PR 阶段曾因启动连接链路的 `about:blank` 超时失败，重跑后通过并据此合入。现有工件没有端口所属进程、新实例退出状态等证据，不能区分“旧 WebView2 端点残留”与“新 WebView2 启动异常”。

**代码缺口（`frontend/scripts/native-acceptance.mjs` 的 `launch`）：** `contracts/native-ui-system.mjs` 的重启调用 `inst.launch()` 沿用原端口；`/json/version` 只要返回一次就立即 `connectOverCDP`，不确认旧实例的 WebView2 进程已退出、端口已释放，也不确认端点属于新实例；探测成功与连接之间端点关闭时直接失败，不在启动期限内处理。

**要求：**

1. **每次启动独立端口。** 每次 `launch` 分配本场景内未用过的端口（基准端口 + 启动序号）；启动前确认该端口没有监听者，否则换下一个，不复用旧端口。实际端口写入证据。
2. **旧实例完全退出后才重启。** 启动时记录宿主 PID 及其 WebView2 子进程（`msedgewebview2.exe`）；重启前在有上限的时间内（建议 20 秒）等待宿主与这些子进程全部退出、旧端口不再监听。超时即失败，并记录残留进程。
3. **核实端点归属。** 连接前通过系统查询监听该端口的进程，确认它是本次宿主本身或已验证的后代进程；没有监听者时可等待，出现外来 owner 或无法核实归属时失败，不能连到旧实例或别的进程（§11.2）。
4. **启动期限内处理瞬断。** 在同一个启动期限内（沿用 45 秒），对 `connectOverCDP` 与页面 target 等待中的连接类瞬断（`socket hang up`、`ECONNRESET`、`ECONNREFUSED`、target 被销毁）有限次重试；每次重试前重新确认宿主存活和端点归属。宿主一旦退出立即失败并报告退出码，不重试；超过期限按原失败处理，不放宽总时限。
5. **证据无论成败都保存。** `evidence.json` 新增 `launches[]`，每次启动记录：序号、端口、启动前端口状态、宿主 PID 与退出码、WebView2 子进程、端口所属进程（PID、映像名、父 PID）、`/json/version` 的 Browser 字符串、每次连接尝试的时间与错误、页面 target 列表、各阶段耗时。失败时另外保存宿主日志尾部。
6. **纯逻辑可单测。** 端口分配、归属判定、重试决策抽到 `frontend/scripts/lib/native-instance.mjs`，进程与网络查询以参数注入；在 `frontend/vitest.config.ts` 中纳入 `scripts/lib/**/*.test.mjs`，由 `pnpm --dir frontend test` 运行。
7. **压力模式。** `native-acceptance.mjs` 新增 `--relaunch-cycles N`：同一作业内连续 N 次“直接退出 → 重启 → 连接 → 读取设置”，每次使用新端口，用于专门验证重启链路。
8. **重跑不能替代原因。** 从本 PR 起，Native UI E2E 失败不得只靠重跑通过就合并：先依据证据说明原因；属于测试架缺陷的先修复，属于产品问题的按缺陷处理。写进 CLAUDE.md 提交流程。

原有检查项名称与数量不减；只改实例生命周期，不改任何场景的断言。

## 5. 任务拆分

| 编号 | 任务 | 完成标准 |
| --- | --- | --- |
| UP01 | 登记表 schema 2：迁入 0.1.0，补登 0.2.0–0.3.1（发布完成后再加 0.3.2） | 每个条目的 digest 与 GitHub 资产一致，`commit` 与标签解引用一致；`persistent_format` 与 §1.2 一致 |
| UP02 | `upgrade_baselines.py`：`select`、`add`、`--check` | §4.3 的选择表驱动用例全部通过；§6 的反例全部能被检出 |
| UP03 | 持久格式指纹：`SETTINGS_FORMAT_REVISION`、`print_persistent_formats`、`build-metadata.json` schema 2 | Release 构建产物带 `persistent_format`；`release_assets.py verify` 校验它；新增登记项与元数据不一致时 `--check` 失败 |
| UP04 | 夹具与编排按 `profile` 分支；备份规则；声明式设置迁移 | `history` 起点的准备、旧事实、升级后核对都有契约测试；1→1 恒等迁移通过 |
| UP05 | 两个工作流改为动态矩阵 | `test_upgrade_workflow.py` 校验两边都使用 `select` 的输出、矩阵绑定起点与场景、失败会传到作业结果 |
| UP06 | Release 连续升级链 | 两条链在 main 的 Release 中通过；`publish` 依赖链 |
| UP07 | 撤回处理：`withdrawn` 状态、发布脚本拒绝重发、撤回操作写进文档 | 契约测试：登记为撤回的版本 `publish` 失败；撤回后 `select` 不再选它 |
| UP08 | 文档：CLAUDE.md“发布与升级”一节、`docs/plans/README.md`、整合计划 N12 链接 | 写明“合入前先登记上一版”和撤回流程 |
| UP09 | 原生验收实例生命周期（§4.8、§11） | §6.2 与 §11 反例单测全部通过；本地与 PR 远程各实跑 20 次重启，均一次通过；独立压力场景工件含 21 条 `launches[]`（首次启动加 20 次重启），20 条循环结果；CLAUDE.md 写明“重跑不能替代原因” |

## 6. 反例（契约测试必须能检出）

### 6.1 升级矩阵

| 反例 | 期望 |
| --- | --- |
| 登记的 SHA256 与下载文件不一致 | 执行旧安装包前失败 |
| 登记表缺少基础版本（origin/main 的版本） | `--check` 失败并提示运行 `add` |
| 已登记 `published` 的版本在 GitHub 上已删除或附件被替换 | `--check` 失败并提示标为 `withdrawn` |
| 撤回的版本被选为起点 | `select` 不输出它 |
| 重跑撤回版本的发布 | `publish` 拒绝 |
| 两个版本持久格式不同却被当成同一组 | 选择用例失败 |
| 起点数超过 6 | `select` 报错 |
| `history` 起点结构未升级却生成了备份，或升级了却没备份 | 夹具核对失败 |
| 设置字段在迁移表之外被改动 | `verify_settings` 失败 |
| 工作流里出现跳过起点的条件分支 | `test_upgrade_workflow.py` 失败 |

### 6.2 原生验收重启（UP09）

| 输入（注入的进程/网络状态） | 必须观察到 |
| --- | --- |
| 旧宿主已退出，其 WebView2 子进程仍监听旧端口 | 新实例使用新端口；重启前等待旧子进程退出，超时则失败并列出残留 PID |
| 新端口上有监听者，但不是新宿主或其后代进程 | 不连接、不终止外来进程；明确失败，证据记录归属不符 |
| `/json/version` 成功后第一次 `connectOverCDP` 报 `socket hang up`，第二次成功 | 在期限内重试成功；`launches[]` 记录两次尝试 |
| 连接重试期间宿主退出（退出码 0 或非 0） | 立即失败并报告退出码，不再重试 |
| 一直连不上直到 45 秒期限 | 按原失败处理，总时限不延长，证据齐全 |
| 页面 target 先是 `about:blank`，随后才挂上 tauri 页面 | 在期限内等到 tauri 页面；超时失败时记录 target 列表 |
| 启动失败或场景中途抛错 | `evidence.json` 仍写出 `launches[]` 与宿主日志尾部 |

## 7. 验证与交付门槛

1. 本地：脚本契约全集、两个语法检查器、`cargo test --locked --workspace`（UP03 涉及核心常量与示例程序）全部通过；按 CLAUDE.md 跑完整提交门禁。
2. PR：同一 HEAD 的 CI、Native UI E2E（一次通过，不借助重跑）、Install test 全部通过，UP09 的 20 次重启压力实跑通过，其中升级矩阵的所有起点 × 场景通过，证据工件齐全（每个作业的 `upgrade-result.json` 写明起点版本与提交）。
3. 合入后：main 的 Release 中两条升级链通过，`v0.x.y` 才发布；把这次运行链接回填到本计划。
4. 未取得第 2、3 步远程证据前，不能写“升级矩阵已验收”。

## 8. 与其他计划的关系

- **PR3**：本计划先合入时，PR3 的 `dir` → 追加路径迁移会自动得到 0.3.x 真实安装包的升级测试；PR3 需要同时把 `SETTINGS_FORMAT_REVISION` 改为 2，并在 `settings_migrations.json` 增加 1→2 规则。若 PR3 先合入，本计划实施时必须补上这一条迁移规则，否则 0.3.x 起点的设置核对会失败。PR3 计划第 120 行的“隔离升级用例”仍由 PR3 自己负责，本计划只提供真实安装包这一层。
- **PR2**：不改持久格式，升级矩阵不受影响。PR2 的 F04 会扩展 `native-acceptance.mjs`，必须在本计划合入后基于新的实例生命周期实现，不得绕开 `launches[]` 证据或另写一套启动逻辑。
- **整合计划 N12**：0.1.0 单跳场景保留为矩阵中的一个起点，原有验收结论不变。

## 9. 决定（用户 2026-10-16 确认：全部按建议）

| 编号 | 问题 | 选项 | 建议（已采纳） |
| --- | --- | --- | --- |
| D1 | 起点集合 | (a) 0.1.0 + 上一版 + 每种持久格式的代表；(b) 全部已发布版本 | (a)：覆盖真实风险，起点数只随格式变化增长；(b) 每发一版加一个作业 |
| D2 | `--check` 放在哪里 | (a) CI `gate`，每个 PR 都检查；(b) 只在升级作业和 Release | (a)：保证每个 PR 都登记了上一版；代价是上一版的 Release 完成前，下一个 PR 的 gate 会失败。有编译缓存时 Release 约 18 分钟（0.3.1 为 17 分 53 秒） |
| D3 | 连续升级链 | (a) 只在 Release，经过格式代表；(b) Release 中经过全部已发布版本；(c) PR 中也跑 | (a)：多跳风险集中在格式变化处；(b) 每多一个版本多 1 分钟左右 |
| D4 | 撤回版本还要不要测 | (a) 撤回即删除发布，此后不再测试；(b) 撤回只改说明、保留安装包，并继续作为额外起点测试（让装过它的用户能升级出去） | (a)：符合“在发布页撤回”的用法，规则简单；如果某次撤回的版本已有不少人安装，可以对那一个版本临时选 (b) |

## 10. 实施细则：升级矩阵（2026-10-16 审阅补充）

本节和 §11 消除前文的实现歧义，冲突时以这两节为准。保留 D1–D4 和 UP01–UP09 的产品范围；尚未执行，不把方案审阅当作验收。实现顺序：UP01/UP02 → UP03 → UP04 → UP05/UP06 → UP07/UP08；UP09 可先完成其独立契约，最终在同一个 PR 取证。

### 10.1 文件边界与逐步交付

| 批次 | 修改/新增文件 | 先写的失败契约与完成条件 |
| --- | --- | --- |
| UP01/02 | `scripts/fixtures/upgrade_baselines.json`、`scripts/upgrade_baselines.py`、`scripts/test_upgrade_baselines.py`；迁移 `release_assets.py` 的 `load_baseline` / `download_baseline` 调用者 | 固定历史条目逐字节保留资产身份；版本按整数三元组排序；重复版本、非法状态、缺失格式字段、无有效起点、超过 6 个均失败；相同输入输出顺序固定 |
| UP03 | 核心 `settings.rs` 与常量所属模块；新增 `crates/tokenscope-core/examples/print_persistent_formats.rs`；`release_assets.py`、`test_release_assets.py`；两套构建工作流 | 当前产物缺失/伪造格式字段失败；历史 schema 1 元数据仍能按历史规则读取；示例从真实常量取值，禁止复制常量数字 |
| UP04 | `upgrade_fixture.py`、`upgrade_test_helpers.ps1`、`test_upgrade.ps1`、`scripts/fixtures/settings_migrations.json`；`test_upgrade_helpers.ps1` 及 Python fixture 测试 | history 起点不读取 cache；退出前不做文件快照；错误事实、意外设置变更、缺备份均失败；现有 0.1.0 两场景仍通过 |
| UP05/06 | `.github/workflows/install-test.yml`、`.github/workflows/release.yml`、`scripts/test_upgrade_workflow.py`；共享的升级编排函数 | 非空矩阵、同一当前包、失败传播、每跳验证、双链证据、发布依赖可检出；不复制一套单跳函数到链路 |
| UP07/08 | `release_assets.py`、相应契约、`CLAUDE.md`、计划索引及 N12 状态文档 | 最新撤回状态阻断所有本次将创建的发布；登记历史身份不变；写清旧工作流限制 |

每批先运行对应新反例并保留失败输出，再做最小实现、运行测试和提交。新增文件的名字是实施约定，不应在尚未创建时写成有效 Markdown 文件链接。

### 10.2 登记、元数据和版本检查

1. `select` 是离线纯函数：只读检出提交的登记表，不访问 GitHub、不修改文件。JSON 输出形态固定为 `{ "include": [{ "origin": "0.1.0", "source_state": "present" }, ...] }`；链路另输出有序版本列表。标准输出只有 JSON，诊断写标准错误。
2. `add` 的网络请求通过可注入客户端；测试使用响应 fixture，不执行真实发布。先验证所有资产、完整提交 SHA、标签解引用与格式数据，再原子替换登记表；失败不得留下半条记录。`published` 必须有完整资产；`unpublished` 允许无资产但必须有版本、提交及失败原因，不能把暂时的 API 错误推断成未发布。
3. `--check --base-ref` 使用事件固定的基线提交：PR 使用 base SHA，不能因为运行途中 main 前进而换版本；main push 检查该提交之前的 main 版本，不能要求刚待发布的当前版本已公开。工作流明确 fetch 所需 ref。同一发布失败后补修的版本可将前版显式登记为 unpublished，仍按已公开版本选择起点。
4. 已登记资产必须验证下载内容 SHA256，API digest 只是第一层；缺 digest 时下载算哈希，不能当通过。标签必须解析到提交，不能只信 `target_commitish`。0.1.0 的别名 tag 与实际 `origin_release` 分开核对。
5. 当前构建只接受 metadata schema 2，历史读取器兼容已公开的 schema 1/无 metadata 的既有 0.1.0。schema 2 缺字段不能退回手工登记。示例输出字段严格类型校验；无 cache 的版本用 `cache_schema: null`，各格式字段的 null/数值含义固定，分组不依赖 JSON 键顺序。
6. 两套生产构建都运行 `cargo run --locked -p tokenscope --example print_persistent_formats`，捕获 stdout JSON 到临时文件并交给 metadata 工具校验；stderr 编译日志不得混进 JSON。只在构建阶段运行，不在安装 Runner 为获取常量重编译。Rust 示例加入后检查构建输入清单是否需要更新。
7. 不把“新增普通设置字段”自动等同于格式破坏；`SETTINGS_FORMAT_REVISION` 递增条件是需要声明迁移或改变旧设置解释语义，必须与迁移表同步。若实现确需扩大此定义，先在本节说明，不能无意造成每版新增一种格式。

### 10.3 撤回机制的真实边界

旧提交只看自己的文件，不可能知道未来发生的撤回。新增发布逻辑在写 GitHub 发布前获取最新 main SHA，再读取该 SHA 的登记表，仅用于撤回阻断；起点选择仍严格使用本次 checkout，保持 I6。记录读取的 SHA 和检查时间，认证/网络/JSON 错误均失败，不能降级放行。

当前 `publish` 还会回补 0.1.0，因此撤回检查要覆盖所有准备创建的版本，而不只当前版本。添加契约：本地旧表为 published、最新 main 表为 withdrawn 时，任何创建 tag/release/upload 调用都没有发生。

**保证范围：** 这只能约束包含新检查的发布代码。更老的工作流不受新代码控制，删除发布也不能阻止已运行到写入步骤的旧作业。撤回操作文档必须写明：先取消对应在途发布、登记撤回状态并使其进入 main，再删除发布；禁止重跑旧发布作业。需要对任意历史工作流实施不可绕过的强制禁发时，应另做仓库级发布权限/外部策略，不能在本 PR 声称已实现。删除发布到登记生效之间不宣称原子安全。

### 10.4 单跳、多跳的事实核对

1. history 起点由旧程序采集，必须正常退出并确认关闭后再复制数据库；否则使用 SQLite backup API，禁止只复制仍有 WAL 的主文件。快照属于证据，不成为新版事实来源。每跳记录起止版本、格式、安装包及主程序哈希、期望清单哈希。
2. I4 按“本跳新增备份”判断：升级前保存已有备份路径/哈希清单。无结构变更允许保留先前跳产生的备份，但不允许本跳新增或覆盖；发生迁移则验证本跳要求的备份事实与版本。不要因连续链上已有旧备份误报。
3. 设置规则只实现有界的声明式操作（恒等、明确字段重命名/移动和默认值），禁止 eval、任意脚本或通配删除。本 PR 仅落地 1→1；PR3 再实现 1→2。未声明的格式组合失败，不猜测；完整比较已播种设置，隐私同意等测试注入字段列为明确例外并记录注入时刻。
4. 单跳以起点和目标修订选择清单；链路必须为每一个目标版本选择对应期望，不能拿最终版本清单验证所有中间跳。默认复用 0.1.0 清单前须逐版本证明解析事实相同；遇到修订差异必须显式登记期望，不能只比较总量。
5. 两条链同样受 `Assert-HostedInstallRunner` 约束，使用独立 Runner。整个链只播种一次；deleted 在首次旧版采集成功后删除合成来源，后续不得重播种、恢复日志、清空库或补写事实。每跳安装前重新校验该包哈希。
6. 所有失败路径写 `upgrade-result.json`：`run_id`、checkout SHA、当前包 SHA、起点、source_state、逐跳结果、失败阶段和日志。失败中止后续跳，顶层非零退出；`fail-fast: false` 只为收集其他作业证据，不掩盖失败。

### 10.5 工作流接线验收

- `plan-upgrades` 输出空集合、格式不对或获取失败，必须使门禁失败，不能让空 matrix 作为通过。作业的 `needs` 必须直接包含读取 outputs 的生产者。
- PR 和 Release 的单跳矩阵均消费同一 `select` 实现；下载同一次构建、同 SHA 的当前安装包，不能各作业重新构建。上传工件命名包含 run/attempt、起点、场景；链路工件再包含各跳编号。
- PR 的只读 token 足以核对发布资产；不得为测试增加发布权限。单元测试覆盖 403/404/限流/网络失败，均不是跳过理由。
- 连续链在 Release 强制执行；手动验证通过 `install-test.yml` 的 `workflow_dispatch` 开启链路，以便需要时合并前取证。Release 的 publish 直接依赖双链及单跳成功；取消/失败/跳过都不能发布。对升级脚本、登记表、fixture 和工作流的 PR 路径过滤全部覆盖。
- “PR 1–2 分钟、链 3–4 分钟”仅是旧场景估计，不是验收事实；构建和排队另计，回填真实时长，不依赖固定并发额度保证正确性。

## 11. 实施细则：原生生命周期 UP09

### 11.1 模块职责和状态机

新增 `frontend/scripts/lib/native-instance.mjs` 与同目录 `native-instance.test.mjs`。测试文件使用 `// @vitest-environment node`，避免沿用 happy-dom。`frontend/vitest.config.ts` 增加该测试匹配，同时保留全部既有测试。

状态：`idle → allocating → spawned → endpoint-owned → connecting → page-ready → running → stopping → stopped`；任一异常进入 failed，写证据后只清理确认属于本次启动的进程。禁止在失败分支自动再启动一个应用。模块注入 `clock/sleep/spawn/snapshotProcesses/listeners/probeVersion/connect/writeEvidence`，纯测试不访问真实进程和网络。

Windows 适配器新增 `scripts/native-instance-state.ps1`：UTF-8 JSON stdout、诊断 stderr；用 CIM 获取 PID、父 PID、创建时间、映像路径，用 TCP 监听表获取端口 owner；查询失败与“没有进程/监听者”必须是不同结果。Node 用异步 spawn 调用，设超时，不在轮询中反复用无上限 spawnSync 阻塞事件循环。增加 `scripts/test_native_instance_state.ps1` 契约并接入本地脚本检查与 `.github/workflows/ui-e2e.yml`。

原有 `Instance.launch/stop` 委托此模块；`contracts/native-ui-system.mjs` 的显式退出重启也必须经过同一生命周期，不另造旁路。盘点 native-acceptance 内全部 launch、stop、直接 connectOverCDP 调用，保留 attach 观察模式的只读边界：attach 不得认领或终止外部实例。

### 11.2 端口和进程身份

1. `--port` 保留为候选起点。范围 1024–65535，端口递增且本次驱动进程用过的端口不再使用；每次分配最多检查 128 个候选，耗尽明确失败，不回绕。IPv4/IPv6 的通配监听和独占绑定失败均算占用，探测 socket 用完立即关闭。
2. 端口空闲检查与子进程真正绑定之间存在竞争，不能宣称预检查已锁定端口。若启动后发现 foreign owner，本次启动失败并记录 owner，不连接、不杀该 owner；下次人工运行可以重新分配。不要在归属不符时无限等待或自动换端口重启来掩盖竞争。
3. 进程身份采用 `(pid, creationTime)`，避免 PID 复用。宿主记录 exe 路径；WebView2 记录完整祖先链，递归后代不能只查直接子进程。启动到运行期间持续累积已验证后代，正常退出请求之前再快照一次；父进程消失后仍保留已确认的后代身份。仅凭映像名相同或隔离目录相同不足以认领。
4. 端口 owner 必须是新宿主本身或可验证的本次后代。连接前、连接成功后各核一次；宿主在任一阶段退出，无论 code 0/非零/信号都失败。查询权限不足、创建时间不可读、祖先链无法证明时失败并取证，不能猜归属。
5. 正常重启：请求 GUI 直接退出，最多 20 秒等宿主退出码 0、已记录后代退出、旧端口释放。这 20 秒是退出预算，与下一次启动预算分开。不通过就停止整个场景；强杀只用于失败后清理，不能把强杀算正常退出成功。清理前重查身份，绝不按名字终止所有 WebView2。

### 11.3 45 秒预算与重试规则

以单调时钟记录 `launchStarted`，分配端口到选定 tauri 页面总共最多 45 秒；不能再外加原来的 20 秒 target 等待。每个网络、系统查询和 Playwright 调用都受剩余预算约束（单次上限可设 5 秒），超时取消底层调用，不能只用 Promise.race 留下后台连接。

- HTTP `/json/version` 必须成功状态码、合法 JSON、有效 Browser/调试端点；连接优先使用这次探测返回的 websocket URL，避免 `connectOverCDP(http)` 再探测一次换了端点。只接受当前本地端口的 URL，不能按返回值连接外部地址。
- CDP 连接最多 5 次，间隔 150/300/600/1000 ms，每次都重新核实宿主和端口归属。只有明确连接瞬断进入重试；协议错误、归属不符、场景断言失败立即失败。耗尽次数或总期限任一先到就停止。
- 页面为 about:blank 时在同一连接上等待，不为它反复重连；只匹配应用允许的确切 origin，不用 `/tauri/i` 模糊匹配。连接断开/target 销毁才进入上述重试；期间始终检查宿主状态。连接失败的客户端释放后才能下一次尝试，不得关闭应用或残留事件监听。
- 页面 ready 只是 launch 的边界。原有 `waitForData` 和场景业务断言继续执行，失败不能触发 launch 重试。不得用压力模式降低原有断言阈值。

### 11.4 先建证据，再启动

`native-acceptance.mjs` 目前首次 launch 的 catch 位于完整证据逻辑之前，必须把整个启动和场景执行纳入统一 try/catch/finally。分配端口前创建 evidence 骨架，每次阶段变化原子更新；无论失败在 spawn、CDP、页面或场景，都产生非零结果和 evidence。

`launches[]` 每项含 launch_id、单调耗时/UTC 时刻、状态、候选端口及拒绝理由、宿主身份、后代快照、端口 owner、探测/连接尝试、target URL、退出结果和清理结果。顶层标明 commit、exe SHA256、run/attempt、隔离根及当前 launch_id；重启后不得沿用首次 PID 冒充当前 PID。失败保存隔离 process.log 与应用日志末尾，标记日志缺失而非空成功。证据序列化错误不能覆盖原异常，至少保留 stderr 诊断。

### 11.5 压力场景与必测反例

增加独立 `--scenario ui-relaunch --relaunch-cycles 20`（参数必须为整数 1–100，其他场景拒绝该参数），新增 `frontend/scripts/contracts/native-ui-relaunch.mjs`。首次启动后通过 UI 写 UTC 并核实 TOML；随后每轮真实 GUI 退出、等待完全退出、重启、等待数据、确认 UTC 的 UI 与 TOML、历史事实不增量。20 次重启 = **21 次成功 launch、20 次循环验证**；最后也正常关闭并确认无残留。不重复执行整个 ui-system 20 次，既有 ui-system 的 14 项仍原样独立执行。

在 §6.2 的七组之外，必须增加：PID 复用不认领；孙进程残留；端口预检后被抢占；进程查询失败不能当空；HTTP 成功但无效 JSON；连接调用挂住不超过总预算；第五次仍失败停止；连接成功后 owner 变化；精确 origin 拒绝含 tauri 的外部 URL；首次启动失败仍有工件；重启证据 PID 更新；非法 cycles；20 次循环中第 k 次失败不执行 k+1；清理失败保留首个原因。使用假时钟测试期限，不真实等待 45 秒。

### 11.6 执行命令与远程接线

先运行 `pnpm --dir frontend test`（原有测试与新增 Node 契约全绿），以及 `pwsh -NoProfile -File scripts/test_native_instance_state.ps1`。再执行：

```powershell
pnpm --dir frontend build
cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope
node frontend/scripts/native-acceptance.mjs --scenario ui-relaunch --events 4 --port 9600 --relaunch-cycles 20 --out qa-artifacts/upgrade-matrix/native-relaunch
```

逐条检查退出码，上一条失败不得继续；本机只启动隔离 acceptance 可执行文件，不安装。证据应为 21 次 launch/20 次循环全部成功且真实数据指纹不变。

`.github/workflows/ui-e2e.yml` 在原有场景外加同一压力命令，PR/main/dispatch 都执行，不依赖新工作流在默认分支出现后才能手动触发；原有场景失败时保持失败，压力测试不能覆盖它。新增输出路径纳入 always 上传，启动已执行却缺 evidence 时门禁失败。确认 45 分钟 job 预算能容纳正常构建和 20 次重启，不能靠扩大单次启动预算过关。

交付记录同时写来源 HEAD 与实际测试 checkout SHA（PR 可能是 merge SHA）、run/attempt、exe SHA 和工件。第一次失败必须保留；调查、修复后新 HEAD 的首次运行通过才算闭合，不把“点击 rerun 后绿色”当修复。若遇独立平台故障也须附明确证据并如实保留失败，不伪造一次通过结论。
