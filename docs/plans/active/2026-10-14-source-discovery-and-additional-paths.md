# PR3：当前用户日志目录发现与追加路径管理

> 执行方式：使用 executing-plans 技能，按 S00 → S07 逐项实施；本文件 §9 是逐步执行清单，不另起竞争计划。

> 状态：[审阅第 13 节](../../reviews/2026-10-15-source-discovery-plan-review.md)复审通过；**2026-10-18 精简修订与新增 S00 已经用户确认**（见 [§0](#0-2026-10-18-精简修订)），可以开工；S00–S07 均未实施。
> 前置条件：PR1、PR2 均已合入（PR2 为 PR #13，main `6393bfc`，v0.4.0）。分支 `feat/source-paths` 已从 main `6393bfc` 建立，带 PR2 计划归档提交；在该分支上实施，独立 minor、独立 PR 和验收。交付时登记 `v0.4.0`。

**Goal：** 默认发现当前用户的标准日志目录与环境变量指定的目录，以追加列表管理来源；来源重叠不重复解析或计量，遍历链接不会死循环。

**Architecture：** 每轮采集探测固定候选并冻结来源集合；规范化路径去重、目录链接防环后交给既有适配器。不读取硬链接文件 ID。历史事实、查询会话和 PR2 扫描生命周期保持既有语义。

**Tech Stack：** Rust workspace、Tauri 2、Vue 3 / Naive UI、TOML、SQLite、Vitest、隔离原生 E2E。

## 0. 2026-10-18 精简修订

按“避免过度防御”的协作要求，开工前删掉收益低、实现和验收成本高的部分。下文各节已按此改写，与本节冲突时以本节为准。

1. **去掉第三层“硬链接物理身份”**：不读 Windows 卷序列号 / FILE_ID_INFO / 64 位文件索引，不建（路径、大小、修改时间）→ 文件 ID 缓存，不统计身份句柄次数。去重只做两层：规范化路径（可访问时 `std::fs::canonicalize`，它已解析联接、符号链接、8.3 短名和大小写）＋目录链接防环。理由：日志目录里出现硬链接极少；万一出现，同一文件被解析两次，事实层按原生身份求并集，统计不会重复，只多一次解析。
2. **不做 sessions→archived_sessions 移动的进度复用**：移动后的文件按新路径重新解析，事实层去重，与现状一致。
3. **根身份不缓存**：固定候选只有几个，每轮重新规范化即可，不设缓存与失效规则。
4. **性能只取证不设门槛**：改前、改后各在本机 release 下跑热轮 3 次取中位数，如实记录；明显变慢时说明原因。删除 2 万文件合成压测和“+10%”硬预算。真实数据基准仍是显式 opt-in、只在本地跑。
5. **测试断言按用例目的取舍**：不要求每个用例都断言四桶、身份集合、cwd、诊断与解析次数。统计类用例断言总量与请求身份集合；去重类用例再断言解析次数；诊断类用例断言诊断。
6. **原生 E2E 只做一条主线**：从设置页添加一个手动目录 → 追加合成日志 → 下一轮读取后统计可见 → 重启后仍在。明暗主题、两种尺寸、多环境根的组合由浏览器 E2E 与核心测试覆盖，不在原生 E2E 里重复。
7. **防环回归不强制子进程**：修复后的测试在临时目录建祖先联接，断言有限时间结束、文件齐全且只出现一次。红态可在本地手动演示并记录现象（旧实现会无界递归），不要求把红态做成不拖死 runner 的自动化测试。

8. **新增 S00（第一个提交）**：PR2 合入后 main 的 [Native UI E2E 39050690161](https://github.com/ywltby/tokenscope/actions/runs/39050690161) 在 `ui-system` 缩放 2 卡死至 45 分钟超时，原因无法从现有证据定位。PR3 先让这类卡死能定位，再做 S01；本 PR 的原生 E2E 也依赖它。

不变：固定候选与环境变量语义、追加列表配置与迁移、同应用嵌套拒绝 / 迁移重叠不阻断、跨应用冲突隔离、显式栈与 `DirEntry::file_type()` 防环、`read_dir` 子项错误不吞、设置页与 IPC、隐私文档与构建输入清单、行为变更披露。

## 1. 范围与入口

- 自动候选仅为当前用户标准根、启动时的 CODEX_HOME、启动时的 CLAUDE_CONFIG_DIR。其余通过手动追加；不扩展自动搜索范围，不做整盘、远程工具或 glob 发现。
- 取消“重新发现”按钮和 source_paths_discover 接口。每轮只探测少量固定路径，新装工具的目录下一轮出现；没有独立发现任务、发现修订或发现迟到响应状态机。
- 启停和移除只影响后续读取，历史库不删数据；所有有效代理用量均纳入。不新增同意流程，不改变 CCS 手动导入、日汇总或费用冻结。
- PR1 后修改入口：`crates/tokenscope-core/src/settings.rs`、`report.rs`、`source/{mod,claude,codex,claude_projects}.rs`；壳 commands/lib/privacy 和前端 Settings/useUsageQuery。必须覆盖启动、手动、定时、来源状态、诊断与验收调用点。

## 2. 固定候选、环境与映射

| 来源 | 日志根 | Claude 映射 |
| --- | --- | --- |
| 默认 Claude | `~/.claude/projects` | `~/.claude.json` |
| 默认 Codex | `~/.codex/sessions`、`~/.codex/archived_sessions` | 无 |
| CODEX_HOME | `<CODEX_HOME>/sessions`、`<CODEX_HOME>/archived_sessions` | 无 |
| CLAUDE_CONFIG_DIR | `<CLAUDE_CONFIG_DIR>/projects` | 禁用映射（未核实，见下） |
| 手动追加 | 用户明确指定的绝对日志根 | 禁用映射，沿用 A03 |

CLAUDE_CONFIG_DIR 映射文件位置必须在 S02 实施前通过官方文档或隔离合成实测核实，将证据回填本节；目前不假定为 `<CLAUDE_CONFIG_DIR>/.claude.json`。无法核实则禁用该根映射，不借用默认映射。默认和环境根同名 slug 必须测试不串映射；相同物理根合并时优先已核实默认绑定，冲突不猜测、需诊断。

**CLAUDE_CONFIG_DIR 映射核实结论（S02 实施前，2026-10-18）**：官方文档 [code.claude.com/docs/en/claude-directory](https://code.claude.com/docs/en/claude-directory) 原文 *"If you set CLAUDE_CONFIG_DIR, every `~/.claude` path on this page lives under that directory instead"* ——确认 `<CLAUDE_CONFIG_DIR>/projects` 是日志根；但 `.claude.json` 位于 `~/.claude.json`（不在 `~/.claude` 之内），官方 settings / env-vars / claude-directory 三页均未说明它是否随 `CLAUDE_CONFIG_DIR` 迁移（env-vars 页截断未含该条目全文）。按本节兜底规则：**该环境根禁用映射**（slug 身份 + 明确诊断），不借用默认映射；后续若取得官方证据或隔离实测证据再启用。

环境变量只使用 TokenScope 启动时继承的值：空串忽略，相对路径忽略并诊断，不存在的绝对根显示缺失、不每轮报错。修改后需要重启；自启进程未必继承交互 shell 中设置的变量，设置帮助和文档必须说明。测试和 acceptance 注入主目录、环境及探测器，完全忽略真实主目录和环境变量。

隐私 Ready 后，每轮开始探测固定路径并冻结集合及映射。来源状态查询使用同一解析逻辑但不扫描用量。默认根不存在可不显示；环境/手动缺失根保留行，权限失败显示无权限。采集中配置变化只影响下一轮，始终串行。根规范化每轮重新计算，不缓存（§0 第 3 条）。采集复用键包含实际集合、启停、映射和上下文签名。

**行为变更：** 旧 CODEX_HOME 替代默认 `.codex`，新行为追加到默认根。旧非空自定义 `dir` 同样曾替代默认根，迁为追加项并开启自动发现后，已启用来源将同时读取默认根与自定义目录；两类升级都可能因默认根旧日志被纳入而增加统计总量，不是重复计量。原已停用来源保持停用。相对 CODEX_HOME 原先按 cwd 解释，新行为忽略并诊断。CLAUDE_CONFIG_DIR 是新增支持，采用同样追加语义。以上全部写入 docs/stats-semantics.md 来源章节、设置帮助、PR3“行为变更”及版本摘要。

两种环境变量均测试：两处有日志取并集；大小写/尾分隔符/联接同根合并一行且保留来源标签；空值、相对值、缺失、重叠；acceptance 忽略真实环境。Codex 另验证旧版只读环境根、新版默认与环境都读的升级用量并集。

## 3. 配置与迁移

```toml
[sources.claude]
enabled = true
auto_discover = true
excluded_auto_paths = []

[[sources.claude.additional_paths]]
path = "D:/history/claude/projects"
enabled = true

[sources.codex]
enabled = true
auto_discover = true
excluded_auto_paths = []

[[sources.codex.additional_paths]]
path = "D:/history/codex/sessions"
enabled = true
```

自动开关管理默认和环境候选，停用项持久化于 excluded_auto_paths，探测/重启不能重启用。自动结果不存为配置，仅保存用户选择。应用总开关优先。

旧非空 dir 按原递归语义迁成一个追加项、保留 enabled、自动发现开启；旧空值无追加项。新字段优先，空数组不当成旧配置。settings::update 原子迁移成功后不再输出 dir；失败不扩大读取范围、不覆盖坏配置、不丢其他字段，重复加载不再迁移。

旧根等于自动根则合并来源标签；旧 Codex dir 为 `~/.codex`、包含自动 sessions/archive 子根时仍迁移成功，标为“与 X 重叠”，在一次性迁移说明中提示移除/停用。总开关原本关闭时不开始采集。说明“原自定义目录已保留为追加目录；自动发现已开启，来源启用时默认目录的历史用量也会纳入，统计总量可能增加”，提供关闭自动发现入口。这是读取范围的行为变更，不能仅描述为字段改名或无影响迁移。

## 4. 嵌套、遍历与两层去重

### 4.1 嵌套规则

同应用手动添加与全部已有行比较（默认、环境、手动、停用、缺失均包含）：同根返回已有行并提示“目录已在列表中”，不新增；上级或子目录直接拒绝，注明冲突行。可访问时同时比较物理目标，防止联接别名绕过；不可访问时执行逻辑路径边界比较并显示身份未确认。

迁移和环境变化产生的同应用嵌套不阻断，标重叠、提示移除手动行或停用自动行，文件层确保只解析一次；不采用子根覆盖父根。跨应用相同/嵌套继续拒绝手动事务；自动出现时隔离冲突集合，其余正常读取，不用“先到者获胜”。

### 4.2 遍历环必修

现有 walk_jsonl 以 child.is_dir() 跟随目录链接且无访问集合，祖先联接会无限递归。改用 DirEntry::file_type() 不跟随分型，显式解析链接目标；维护整轮已访问目录的规范化目标路径集合。已访问或归另一个已启用根负责的目标跳过，其他目标只跟随一次；跳过需确保目标确实由其他根遍历，不漏读。无法解析链接则跳过并诊断，不能无界递归，不引入文件 ID。

使用显式栈避免深目录耗尽调用栈；read_dir 子项错误需保留，不能 entries.flatten 静默吞错。祖先联接回归按 §0 第 7 条执行：不强制子进程，旧实现的无界递归不在自动化 runner 上直接运行；修复后断言终止、文件齐全且只一次。

### 4.3 文件去重（两层）

1. **路径**：规范化绝对路径，处理分隔符、盘符、Windows 大小写、尾分隔符；可访问时用 `std::fs::canonicalize` 统一 `\\?\` 前缀/UNC、8.3 短名、联接与符号链接目标。非 Windows 不折叠大小写。不可访问时只做逻辑路径规范化，并标“身份未确认”。
2. **目录链接**：按 4.2 防环。

同一规范化路径在本轮只认领一次，归规范化后最小的根；解析进度沿用现有 `source_files`（按 app、root、path）。不做硬链接物理身份和移动复用（§0 第 1、2 条）：这两种情况下同一内容可能被解析两次，由事实层按原生身份求并集，统计不重复。

每根映射冻结，手动根不猜映射。物理文件身份不是请求身份：Claude message 身份、Codex 保守重播指纹不变；复制为不同文件仍由事实层求并集。停用和删除不删除历史。

### 4.4 性能取证（不设硬门槛）

- 改前、改后各在本机 release 下跑热轮 3 次取中位数，如实记录数据规模；明显变慢时定位到来源解析、遍历或解析阶段并说明原因。参考量级：热轮约 0.1–0.2 秒。
- 来源解析单独计时，预期毫秒级；auto 轮次按 debug、手动/启动按 info，不逐文件刷屏。
- perf_real_data 仍须显式 opt-in，只在本地跑；执行前说明会读取真实来源并写历史库，优先用只读来源＋隔离数据库。其他自动化测试禁止访问真实数据。没跑就标待验。

## 5. 设置页、接口与隐私

每应用总开关、自动开关；追加输入/目录选择/添加；下方列表，没有重新发现按钮。目录完整可复制换行；来源为默认自动/环境/手动多标签；状态为待读取、可读取、缺失、无权限、部分失败、停用、冲突、与 X 重叠。仅手动来源可移除，合并行移除只删手动标签；自动行可停用。

拒绝空值、相对路径、明显整盘/整个用户主目录、同应用嵌套和跨应用冲突；合法缺失绝对根可保存。选择取消不保存，输入不静默改写；错误保留草稿、成功清空、防重复提交。

IPC 保留 source_paths_list、source_path_add、source_path_remove、source_path_set_enabled，总开关走 settings 事务。写接口对最新 settings 单项修改，前端串行提交、完成后重读，用普通请求序号避免旧列表覆盖；不建立独立发现修订。配置成功失效采集复用键，串行请求新采集，沿用 PR2 状态和冻结查询。

解析行包含 id/agent/path/canonical_identity/origins/mapping_path/enabled/diagnostic，id 仅为配置展示身份。单应用过滤来源警告、全局警告保留，权限状态不伪报缺失，不请求额外权限，不重复刷 info。

docs/privacy.md 只补充当前用户默认、环境和多个手动路径，不新增同意流程。该文件通过 include_str! 编译进壳，必须命中 PR1 的构建输入清单和壳检查。

## 6. S00–S07 实施任务

每项使用 executing-plans 技能：失败测试→确认原因→最小实现→定向与必要门禁→独立提交。路径按 PR1 迁移后布局。

### S00 原生验收卡死可定位（第一个提交）

**现象**（main `6393bfc`，run 39050690161）：`ui-system --scale 2` 第 1 次启动，`system-restored.png` 已写出，`timezone-restarted.png` 与 `launch #2` 日志都没有；WebView2 于 16:50:20 退出（process.log 末行），之后 39 分钟无输出，作业 45 分钟超时被取消，`evidence.json` 未写出。同一 HEAD 在 PR #13 上五次通过。

**卡点所在段**（`frontend/scripts/contracts/native-ui-system.mjs` 第 150–172 行）：`spawnSync` 调 `native-window-action.ps1 -Action close`（无 timeout）→ 点击“直接退出”（Playwright 默认超时）→ 15 秒内等宿主退出 → `inst.stop()`（其中 `browser.close()` 无超时）→ `inst.launch()`。无法区分测试架卡死与程序点击“直接退出”后未真正退出。

**改什么**：

1. 契约目录与 `native-acceptance.mjs` 中所有 `spawnSync` 加 `timeout`（30 秒），超时按失败抛出并写明动作名。
2. `stop()` 里的 `browser.close()` 加 10 秒上限，超时记入停止证据后继续走既有的退出确认与终止流程。
3. 场景级看门狗：普通场景整体设上限（5 分钟）；沿用既有入口实现，不另套一层无人收尾的 Promise.race。`expire-cycle`、`ui-relaunch` 等有明确长时目的的场景保留显式独立预算，不用 5 分钟截断合法工作。到点先写 `evidence.json`（沿用 `failed` 数组，加入 `scenario_timeout`，并记录最后阶段名、宿主退出状态与退出码），然后在有界收尾中复制应用日志、按既有流程终止已记录进程，以非零退出。具体步骤见 §9.1。
4. 阶段日志：`native-ui-system.mjs`（以及 `native-collection-status.mjs`、`native-ui-refresh.mjs` 中的关闭/重启段）在关闭、点击退出、等待退出、停止、重启前各打一行 `阶段 <名称>`，并记入证据的最后阶段字段。
5. 单测：`frontend/scripts/lib/native-instance.test.mjs` 增加看门狗用例（用可控时钟或极短上限，断言写出证据、记录最后阶段、非零退出）。

**验收**：`pnpm --dir frontend test` 通过；本 PR 的 Native UI E2E 通过。不要求复现原卡死。最后阶段和宿主状态用于缩小范围，不能仅凭宿主未退出就归因程序：还要确认退出动作是否成功送达；动作/连接未送达属测试架，明确送达后应用未退出才进一步查产品。未知根因如实保留，不用重跑通过代替解释。

### S01 配置集合与迁移

- 修改 `crates/tokenscope-core/src/settings.rs`，新增 `crates/tokenscope-core/tests/source_paths_settings.rs`。
- 覆盖旧空/非空/停用、新旧并存、空数组、幂等、原子失败、并发事务、父根迁移重叠保留和说明、手动父/子拒绝（含停用和缺失行）。
- 新增隔离升级用例：旧非空自定义 dir 与默认根各有独立用量，并含重复请求；旧配置只读自定义根，迁移后默认根独立用量并入、重复请求不增量，重复迁移/采集结果不变。旧来源 enabled=false 时两处均不读取。S01 固化配置与预期，S03 接通多根采集后执行完整用量断言，未通过不能关闭该升级用例。
- `cargo test --locked -p tokenscope --test source_paths_settings`。

### S02 固定候选与环境发现

- 新增 `crates/tokenscope-core/src/source/discovery.rs`、`crates/tokenscope-core/tests/source_discovery.rs`；修改 source/mod.rs、acceptance.rs。
- 注入主目录、启动环境、探测器；第 2 节环境/映射全部用例、新目录下一轮出现、权限/缺失区分、每轮重新规范化和毫秒级解析。CLAUDE_CONFIG_DIR 映射证据回填第 2 节。
- `cargo test --locked -p tokenscope --test source_discovery`。

### S03 防环、多根与去重

- 修改 `crates/tokenscope-core/src/report.rs`、`source/{mod,claude,codex,claude_projects}.rs`；新增 `crates/tokenscope-core/tests/source_paths_collection.rs`。
- 环/深目录/断链/读错、重复路径/短名/前缀/符号链接、稳定认领者、上下文改变、默认环境同 slug 不串映射均测试。
- 覆盖迁移/环境重叠仅解析一次、手动嵌套拒绝、跨应用冲突隔离、复制求并集、独立事件不丢、停用保留、CODEX_HOME 升级。
- `cargo test --locked -p tokenscope --test source_paths_collection` 与 `cargo test --locked -p tokenscope --test ui_adjustment_collect`；按第 4.4 节记录性能。

### S04 壳事务与调度

- 修改 `src-tauri/src/{commands,lib,privacy}.rs`，测 Ready 前拒绝、每轮冻结、配置与采集交错、事务失败不扩范围、退出后不采集、PR2 状态一致。
- `cargo test --locked -p tokenscope-app`。

### S05 来源列表

- 修改 `frontend/src/types.ts`、Settings.vue/Settings.test.ts、useUsageQuery.ts 及测试。
- 多来源标签、追加不覆盖、父/子拒绝、同根提示、非手动重叠、启停/移除、错误保草稿、保存重读、长路径和可访问名称、无发现按钮、环境重启提示。
- `pnpm --dir frontend test`、`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`。

### S06 浏览器、原生与性能

- 浏览器 E2E：扩展 `frontend/scripts/fixtures/ui-contracts.mjs`、`contracts/interaction-contracts.mjs`、`run-ui-refresh-e2e.mjs`，覆盖来源列表的键盘添加、取消、同根提示、父/子拒绝、合并标签、停用持久化和长路径；沿用现有明暗主题与两种尺寸。
- 原生 E2E：`native-acceptance.mjs` 加一个场景并接入 Native UI E2E，按 §0 第 6 条只做一条主线；不回退真实主目录和环境变量。
- 性能按 4.4 记录改前改后。
- `pnpm --dir frontend build`、`pnpm --dir frontend test:e2e`，原生隔离工件单独记录。

### S07 PR3 文档、版本与交付

- 更新 CLAUDE.md、DESIGN.md、docs/stats-semantics.md、docs/privacy.md、计划和总账；新增引用按 PR1 更新构建输入清单。
- fetch 最新 main，独立 minor bump，摘要明确“日志目录自动发现与追加目录管理；旧自定义 dir 迁为追加项并开启自动发现，已启用来源的默认目录用量也会纳入、总量可能增加；CODEX_HOME 追加默认根，相对环境路径不再读取”；不预定版本号。
- 门禁：`cargo fmt --all -- --check`；`cargo clippy --locked --workspace --all-targets -- -D warnings`；`cargo test --locked --workspace`；clippy/test 再加 `--features acceptance`；前端 typecheck/format:check/test/build/test:e2e；脚本契约和清单检查。
- 同一 HEAD 的 CI、安装升级、Native UI E2E 与有效工件通过才就绪，不自行合并。真实跨午夜/长时间休眠/跨显示器仍在独立系统 todo，不混入本 PR。

## 7. 状态与验收

S01–S07 未开始，版本未分配。实施后逐项记 commit、命令、失败→通过及远程 run/artifact，计划审阅不代表实现。

- [x] S00：原生验收卡死有超时、看门狗证据与阶段日志。（2026-10-18 实施完毕，记录见下）
- [x] S01：配置集合与迁移（设置格式 v2）。（2026-10-18 实施完毕，记录见下）
- [x] S02：固定候选与环境发现、映射核实与隔离解析。（2026-10-18 实施完毕，记录见下）
- [x] S03：防环、多根与去重。（2026-10-18 实施完毕，记录见下）
- [x] S04：壳事务与调度。（2026-10-18 实施完毕，记录见下）
- [x] S05：来源列表前端。（2026-10-18 实施完毕，记录见下）
- [x] S06：浏览器、原生与性能。（2026-10-18 实施完毕，记录见下）
- [x] 固定候选、两种环境变量、映射核实和隔离全部验证。（S02/source_discovery 14 例，记录见 S02）
- [x] 手动嵌套拒绝；迁移/环境重叠不阻断且只解析一次。（S01/S03/S04 测试，记录见各项）
- [x] 联接环闭合；两层去重不丢独立事件。（S03/source_paths_collection 14 例，记录见 S03）
- [x] 来源集合、列表、诊断一致；停用历史保留。（S04 壳接线 + S05 列表 + 采集隔离语义，记录见各项）
- [x] 改前改后性能有记录；真实基准有证据或待验标记。（S06：合成对比已记录，真实基准待验，记录见下）
- [x] 旧自定义 dir 与 CODEX_HOME 行为变更、升级用量并集、停用保留、重启说明、隐私范围及清单齐全。（S07 文档四处 + 构建输入清单校验，记录见下）
- [x] PR3 自身版本、完整门禁、安装升级与原生工件验证。（2026-10-18 完成，记录见 S07）
- [x] S07：文档、版本与交付。（2026-10-18 实施完毕，草稿 PR #14 等待复核，记录见下）

### S00 执行记录（2026-10-18）

- **提交**：S00 代码与测试为 commit `7e31a19`（pre-commit 完整模式全绿）；本记录为紧随其后的文档提交。
- **落点**：新增 `frontend/scripts/lib/native-scenario.mjs`（30 秒有界 `spawnSyncBounded`、10 秒 `closeBrowserBounded`、`withTimeout` 有界取证、`createStageTracker` 阶段记录、`runScenarioBounded` 场景看门狗、`finalizeScenarioTimeout` 有界收尾、`ScenarioCanceledError`）；新增 `frontend/scripts/lib/native-scenario.test.mjs`；`native-acceptance.mjs` 全部 spawnSync 改有界、`Instance.stop/launch` 接取消自查与阶段标记、失败取证加 8 秒上限、场景执行改走看门狗入口；`contracts/native-{ui-system,ui-refresh,collection-status}.mjs` 的关闭/重启段加 `close-command`/`click-quit`/`wait-host-exit` 阶段标记与有界命令。
- **§9.1 第 1 条清单（改前现状，均已消除）**：无界 spawnSync 10 处（主入口 ps/psCommand/nativeGeometry/git/cmd 四类 + ui-system 的 restore/close/托盘关闭/ui-relaunch windowAction + ui-refresh 与 collection-status 的 closeForRestart）；无界 `browser.close()` 4 处（Instance.stop、cleanupFailedLaunch、expire-start 分支、两个契约 closeForRestart）；失败截图/DOM 查询在场景竞速保护外；evidence.json 只在末尾一次性写出。现有场景名称与检查项全部保留，仅新增 `scenario_timeout` 检查。
- **关键测试**：`scripts/lib/native-scenario.test.mjs` 24 例——同步命令超时（error=ETIMEDOUT/signal=SIGTERM/status=null 全部进错误信息）、CDP close 永不返回、场景永不返回、onTimeout 恰一次、迟到分支不重复触发且不重启（源级契约钉住 Instance 接线）、失败取证永不返回、正常完成解除计时器、attach 不终止外部进程、Node 子进程端到端（父测 10 秒上限、断言磁盘证据与非零退出 7）。
- **执行结果**：`pnpm --dir frontend test` 44 文件/566 用例全绿；`python -B scripts/check_node_syntax.py` 20 个脚本通过；本机冒烟（`--exe` 指向不存在路径跑 `ui-system`）退出码 2，evidence.json 落盘含 `failure_stage=setup`、`last_stage`（stop 已完成 done_at 置位）、`stages`（launch 失败未完成）、`stop.browser_close`、真实数据指纹不变。
- **边界如实记录**：防卡死机制已验证；run 39050690161 的原卡死根因仍未知（S00 只保证此类卡死能定位，不宣称已修复）；场景预算非精确墙钟强杀点（同步阻塞最多推迟一个 30 秒有界调用）；远程 Native UI E2E 待本 PR CI，属交付条件。未完成项：无（本项范围内）。

### S01 执行记录（2026-10-18）

- **提交**：S01 代码与测试为 commit `4d541f1`（pre-commit 完整模式全绿）；本记录为紧随其后的文档提交。
- **落点**：`settings.rs`——`SourceConfig` 增 `auto_discover`/`excluded_auto_paths`/`additional_paths`（`AdditionalSourcePath{path,enabled}`）并自定义反序列化保留"新字段是否出现"信息；`migrate_source_paths(_with)` 事务锁内 v1 dir→追加项（空 dir 仅归一、相对 dir 按 cwd 转绝对、失败可操作且字节不动、幂等、重叠给一次性提示、其余字段逐字段保留）；`check_additional_path` 与 `validate_additional_path_shape` 供 S04 接线；`SETTINGS_FORMAT_REVISION` 1→2，模板同步。升级契约：`settings_migrations.json` 补 `source_dir_to_additional_path` 操作与 `1->2`/`2->2`（保留 `1->1`），`upgrade_fixture.py` 校验实现与 `test_upgrade_fixture.py` 契约测试同步。
- **关键测试**：`source_paths_settings.rs` 17 例（旧空/非空/停用 dir、新旧并存忽略 dir、显式空列表、幂等、共享锁原子失败、迁移×update 并发合并、相对 dir 存在/缺失、父根重叠说明、遗留 JSON、字段保留、追加路径形态与同/嵌套拒绝、升级配置固化、修订号=2）；`test_upgrade_fixture.py` 新增 8 例（dir→追加项各坏形态拒绝、空 dir 无空条目、停用继承、无 dir 免迁移、2->2 恒等、规则表完备性）。
- **执行结果**：`cargo test --locked -p tokenscope --test source_paths_settings` 17/17；`cargo test --locked --workspace` 49 个测试目标全绿；`cargo clippy --locked --workspace --all-targets` 与 `cargo fmt --all -- --check` 干净；`python -B -m unittest test_upgrade_fixture` 17/17。
- **边界如实记录**：升级用量并集断言（旧自定义 dir 与默认根各有独立用量、重复请求不增量）按 §9.2 第 7 条**待 S03** 在 `source_paths_collection.rs` 以相同场景执行，本项只固化配置与预期；`source_config_set` 等壳入口仍按旧接口编译兼容（S04 替换）。未完成项：无（本项范围内）。

### S02 执行记录（2026-10-18）

- **提交**：S02 代码与测试为 commit `7d3955c`（pre-commit 完整模式全绿）；本记录为紧随其后的文档提交。
- **落点**：新增 `source/discovery.rs`（`DiscoveryContext` 注入锚点/启动环境/探测器、`StartupEnv` 快照、`filesystem_probe` 四态分类、`resolve_sources` 候选构造与同根合并、行操作 `set_row_enabled`/`remove_manual_from_row`/`row_exists`、FNV-1a 上下文签名）；`source/mod.rs` 挂模块；`settings.rs` 的 `normalized_key` 发布为公有（排除项匹配用同一规范化函数）；新增 `tests/source_discovery.rs`。§2 已回填 CLAUDE_CONFIG_DIR 映射核实证据（官方 claude-directory 页确认 `<CLAUDE_CONFIG_DIR>/projects`；`.claude.json` 位置无官方文字 → 该根禁用映射降级）。
- **关键测试**：`source_discovery.rs` 14 例——默认三根的 origin/映射/启用、Missing/NoPermission/NotADirectory 区分、环境空串/相对/缺失、CODEX_HOME 追加语义、大小写+尾分隔符+别名同根合并三标签、legacy 语义不扩大读取（dir 取代默认、Codex 环境根替代默认）、映射四类绑定（默认/环境禁用/手动禁用/合并保默认）、总开关×行开关×排除项组合、行禁用→重解析→移除→开启循环、未知 id 拒绝、行 id 跨轮稳定、新目录下一轮出现、签名随有效变化而变。
- **执行结果**：`cargo test --locked -p tokenscope --test source_discovery` 14/14；`cargo test --locked --workspace` 50 个测试目标全绿；`cargo clippy --locked --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 干净。
- **边界如实记录**：解析耗时未设硬门槛（§0 第 4 条），毫秒级取证随 S03/S06 采集计时输出；`acceptance.rs` 本项未改——`DiscoveryContext::production()` 已复用 `source_dir_override` 落隔离根，§9.3 第 2 条要求的"acceptance 空环境覆盖 + 场景显式注入"在 S04 壳启动路径接线时落实并在此回填；联接别名/断链的真实文件系统 fixture 属 S03（`source_paths_collection`）。未完成项：无（本项范围内）。

### S03 执行记录（2026-10-18）

- **提交**：S03 代码与测试为 commit `cdb6fd6`（pre-commit 完整模式全绿）；本记录为紧随其后的文档提交。
- **落点**：`source/mod.rs` 新增 `walk_jsonl_guarded`（显式栈 + `file_type()` 分型 + 链接显式解析 + reparse point 检测）与 `DirVisitLedger`（canonical 目录台账，跨应用进入暴露）；Claude/Codex 实现 `discover_with_ledger` 共享台账、独立调用自带台账；`report.rs` 新增 `SourcePlan`（进入 `SummaryOptions.source_plans`，None 保持旧单根兼容语义）、`make_sources` 每根一实例、`discover_round` 排序共享遍历 + 文件身份认领（最小规范化根优先、跨应用同文件整组隔离并双侧诊断、同应用重复认领一次）、`SourceReport` 按 agent 聚合、`collection_key` 计划模式含实际根集合与映射修订；移除采集期硬校验（重叠为合法状态，硬拒绝移到保存事务）。
- **关键测试**：`source_paths_collection` 14 例对应 §8.3——`ancestor_junction_loops_terminate_with_complete_single_parse`（真实 junction 经 mklink /J，环终止文件只解析一次）、`deep_tree_broken_junction_and_stray_files_terminate_with_diagnostics`（120 层深目录 + 断链诊断）、`same_agent_nested_and_duplicate_roots_parse_once`（父/子/大小写/尾分隔符，且交换输入顺序结果相同）、`copied_files_parse_but_native_identity_dedupes_without_loss`、`hard_link_paths_each_parse_once_and_union_in_history`（按 §0 第 1 条不做物理身份去重）、`mapping_context_change_reparses_file`、`same_slug_across_roots_do_not_cross_mappings`、`cross_app_same_root_isolated_and_other_roots_continue`、`upgrade_old_dir_plus_default_root_union`（**关闭 S01 遗留**：A∪B、副本不增量、热轮 generation 不推进、停用不读取且历史保留、恢复后纳入）、`codex_home_upgrade_reads_default_and_env_roots`、`truncated_file_reparses_with_fresh_fingerprint`、`root_set_change_between_rounds_changes_reading_scope`、`empty_plan_set_collects_nothing_and_keeps_history`、`plan_mapping_binding_loads_and_resolves`；`source_overlap` 10 例按隔离语义重写（同根/别名/嵌套/大小写隔离 + 诊断、跨文件隔离在解析前、无冲突照常采集、保存校验函数级核对）；`rebuild_source_contract` 重叠隔离语义。
- **执行结果**：`cargo test --locked --workspace` 51 个测试目标全绿；`cargo clippy --locked --workspace --all-targets -- -D warnings` 干净；`cargo fmt --all -- --check` 干净；acceptance 特性 clippy/test 48 个全绿。
- **边界如实记录**：junction 用 `cmd /c mklink /J` 创建（临时根内，清理先解除链接再删树，本机与 runner 均支持，未以 skip 换通过）；权限拒绝路径未在采集层 fixture 注入（Windows ACL 注入易残留，`NoPermission` 分类由发现层与探测注入测试覆盖），如实记待验项并入 S06 原生验收观察；热轮"零重解析"断言受全局采集复用槽位影响不确定，改以 generation 不推进 + 统计不增量断言；性能取证（§4.4 改前/改后热轮中位数）在 S06 执行。未完成项：性能取证待 S06、权限拒绝采集 fixture 待验（如实披露，不关闭）。

### S04 执行记录（2026-10-18）

- **提交**：S04 代码与测试为 commit `3c7ae1d`（pre-commit 完整模式全绿）；本记录为紧随其后的文档提交。
- **落点**：`commands.rs`——`resolve_source_set(_in)` 返回 `ResolvedSourceSet`（整轮 `SourcePlan` + 应用总开关），`query_opts`/`list_events`/`collection_opts_from`/`rebuild_opts` 逐处替换；启动环境快照（`capture_startup_environment`，lib setup 最早阶段一次）；`source_config_set` 收窄为总开关（旧 dir 参数忽略并告警、新列表逐字段保留）；新增 `source_paths_list`/`source_path_add`/`source_path_remove`/`source_path_set_enabled`（隐私闸门 + 串行采集唤醒 + impl 密闭读取传入路径）。`lib.rs`：setup 最早阶段捕获环境；`RuntimeStep::SourcePathMigration` 一次性 v1 dir 迁移（成功写"总量可能增加"说明与重叠提示进启动诊断；失败保留旧文件按 v1 语义读取并降级诊断）。`discovery.rs`：行操作改用显式 `RowConfigPaths`（消除 canonical/逻辑键两种字符串形态的匹配错位），`canonical_identity` 公共助手。
- **关键测试**：shell——`test_source_config_set_sets_only_app_switch`（旧 dir 忽略 + 追加项/排除项/自动开关逐字段保留）、`source_path_add_rejects_cross_app_and_nested_conflicts`（撞缺省 Codex 默认根、嵌套、同应用已存在、相对路径、整个主目录、合法追加）、`source_path_remove_and_set_enabled_follow_row_semantics`（禁用默认行写排除项不动手动项、禁用手动行、移除只删手动、纯自动行不可移除、开启清空、未知 id 拒绝）、`collection_opts_follow_current_settings_per_round`（每轮计划跟随设置：新目录下一轮出现、停用后计划消失、损坏设置拒绝不扩范围）、`missing_settings_keeps_documented_defaults`（注入主目录展开默认三根）、损坏/不可读设置拒绝；core——行循环测试改 RowConfigPaths。privacy 白名单新增四命令由 `privacy_all_business_commands_reject_before_ready` 自动覆盖 Ready 前拒绝。
- **执行结果**：`cargo test --locked -p tokenscope-app` 57 用例全绿；`cargo test --locked --workspace` 51 个测试目标全绿；`cargo clippy --locked --workspace --all-targets -- -D warnings` 与 fmt 干净。
- **边界如实记录**：`source_status` 保持应用级兼容面未改（设置页根行走新 IPC，S05 接前端）；设置迁移结果的一次性提示经启动诊断返回（S05 不重复展示）；`_in` 注入变体仅测试使用。未完成项：无（本项范围内）。

### S05 执行记录（2026-10-18）

- **提交**：S05 代码与测试为 commit `08e93b4`（pre-commit 完整模式全绿）；本记录为紧随其后的文档提交。
- **落点**：`Settings.vue`「数据源」分区重写——source_paths_list 行列表（多标签、完整路径可复制换行、状态文案含"已停用 · X"）、追加目录为明确动作（草稿失败保留/成功清空/防重复提交）、总开关与自动发现开关即改即存（失败回滚 + mutationEpoch 防晚到覆盖）、行级启停/移除（移除只对手动行）、同键串行队列、"已保存，列表刷新失败"区分提示、环境重启提示、无重新发现按钮；新增壳命令 `source_auto_set`（S04 遗漏的自动开关入口，随本项补齐，含 handler/白名单）；`types.ts` 增 `SourcePathRow`；settingsPreload 预读清单加 source_paths_list；`useUsageQuery.ts` 不变（汇总页 source_status 兼容面保留）。
- **关键测试**：Settings.test.ts 新 describe「来源目录管理（PR3）」6 例 + 改写 RC03/AP04/UX06/UX08 中依赖旧草稿模型的用例（retry 保留追加草稿、晚到读取不覆盖开关、追加失败不丢其他错误、追加串行、切分区草稿保留、UX08 稳定命名与换行契约）；ccsImport/preload/tooltip mock 同步（预读 6→7 命令）。
- **执行结果**：`pnpm --dir frontend test` 44 文件 561 用例全绿；`pnpm --dir frontend typecheck`、`format:check`、`build` 干净。
- **边界如实记录**：原生 E2E 的界面接线验证在 S06（`ui-source-paths` 场景）；浏览器 E2E 契约扩展也在 S06。未完成项：无（本项范围内，E2E 项归 S06）。

### S06 执行记录（2026-10-18）

- **提交**：S06 代码与测试为 commit `9861019`（pre-commit 完整模式全绿，含端口分配契约同步）；本记录为紧随其后的文档提交。
- **落点**：浏览器 E2E——`ui-contracts.mjs` 白名单 +6 来源命令与可变行状态（list/add/remove/set_enabled/auto_set/config_set 联动）；`interaction-contracts.mjs` 新增 `source_paths_management_contract`（合并标签、Enter 添加、取消不调用、父/子拒绝保草稿、停用跨重载持久、移除按钮语义）并改写 4 条依赖旧保存模型的契约；`ui-refresh-contract.mjs` 的保存步骤改为点击来源总开关等 `source_config_set`；`Settings.vue` 开关保存成功补发 `data-changed`（与旧保存通知链同源）。原生 E2E——新增 `contracts/native-source-paths.mjs` 场景 `ui-source-paths` 并接入驱动器（应用日志/失败取证收集、`native-instance.test.mjs` 端口分配契约）与 `.github/workflows/ui-e2e.yml`（端口 9455，进程内 2 次启动用 9455–9456）。性能取证——新增 `perf_synthetic_rounds.rs`（合成树 + 隔离目录，改前 worktree 复用同一热轮用例）。
- **关键测试**：`source_paths_management_contract` 10 断言；`ui_refresh_lifecycle` 8 断言（含新增"来源保存通知保留的汇总页"）；`ui-source-paths` 场景 12 检查（默认行在列、UI 追加手动行、追加后默认行不变、合成请求自动可见不点刷新、汇总/明细同一查询、重启后手动行与默认行保留、15 秒稳定窗口计数不重复、真实数据指纹不变）；`perf_synthetic_rounds` 2 例。
- **执行结果**：
  - 浏览器 E2E `pnpm --dir frontend test:e2e`：6 条契约全绿（`ui_refresh_lifecycle` 8、`ui_refresh_clock` 7、`ui_adjustment_shapes` 13、`ui_date_keyboard_matrix` 121、`ui_control_material` 29、`source_paths_management_contract` 10），基线场景 0 契约违例。
  - 原生场景本机实测（`--scenario ui-source-paths --events 4 --port 9455`）：12/12 检查通过；合成请求写入后 **4850 ms** 自动可见（requests 6→7，无手动刷新）；重启后计数仍 7；`real_untouched` 指纹不变；2 次启动退出确认通过（中间重启为干净退出码 0）。
  - 性能（§4.4，本机 release，合成 40 文件/12 000 请求/5 天，单 Claude 根，隔离目录）：改前（基线 worktree `20b0c4c`）热轮 3 次中位数 **112 ms**（108/112/116）、冷轮 1715 ms；改后热轮中位数 **117 ms**（107/117/127）、冷轮 1752 ms——差异在运行间噪声带内，无"明显变慢"定位触发；来源解析单独计时中位数 **507 µs**（274–713 µs，毫秒级达标）。
- **调查记录（不用重跑碰绿）**：`ui_refresh_lifecycle` 首跑超时的根因是 `run-ui-refresh-e2e.mjs` 以 `vite preview` 服务**既有 dist**，而 `data-changed` 补发是未提交的源码改动、不在旧包内——重建后原样通过，未放宽任何断言或超时。
- **边界如实记录**：真实数据性能基准（`perf_real_data` opt-in）未运行，按 §4.4 标**待验**；远程 Native UI E2E（含新场景）待本 PR CI 属交付条件；S03 遗留的"权限拒绝采集 fixture"未在本场景注入（Windows ACL 注入易残留），`NoPermission` 分类仍由发现层测试覆盖，保持待验披露。未完成项：无（本项范围内）。

### S07 执行记录（2026-10-18）

- **提交**：文档 `802923b`（stats-semantics §1/§2.2/§3.4、privacy、DESIGN、CLAUDE、总账；构建输入清单 `--check` 通过、无新增 include 输入）→ 版本 `d501bab`（minor 0.4.0→0.5.0，四份配置 + 根锁 + `release-change.json`，摘要按 §6 措辞；`bump_version --check --base-ref origin/main` 通过）→ 基线 `ac6eb65`（`upgrade_baselines.py add --tag v0.4.0`，资产逐个下载核对后写入）。**最终 HEAD = `138f06f`**（含 CI 取证的四个修复，见下），fetch 时 main 仍在 `6393bfc`（无分叉）。
- **本地完整门禁**：`cargo fmt --all -- --check`；普通 + acceptance 两套 `cargo clippy --locked --workspace --all-targets -- -D warnings`；普通 + acceptance 两套 `cargo test --locked --workspace`（52 个测试目标，无失败）；前端 typecheck / format:check / 562 用例 / build / 浏览器 E2E 六契约全绿；`python -B -m unittest discover -s scripts -p "test_*.py"` 277 例、node 语法 21、pwsh 语法 12、升级契约 59 项、安装契约 26 项；`bump_version --check` 与构建输入清单 `--check` 通过。
- **CI 取证的四个修复（先定位后修，不重跑碰绿）**：
  1. **产品缺陷（`7a99978`）**：`settings::update` 对未迁移 v1 文件的派生序列化带出 `auto_discover=true`，把文件混合化——重载时旧 dir 被静默忽略，迁移失败窗口内任何无关设置修改都悄悄改变读取范围（并发测试 update 先取得写锁时必现）。修复：`SourceConfig` 手动 Serialize 保留 v1 形态；新增确定性回归 + 并发用例两序殊途同归；18 例 9 连跑。
  2. **产品缺陷（`ce14a4f`）**：每根一实例后，缺失的默认 codex `archived_sessions` 根每轮产生"目录不存在"警告，状态胶囊常驻"已更新 · 部分异常"（ui-system 证据与截图定位）——违反 §2"缺失不每轮报错"。修复：缺失/非目录的纯自动根不进采集计划（列表行照常返回；目录出现下一轮自然回计划）；手动缺失根保留。新增如实探测的回归测试。
  3. **测试架（`67c512a`）**：runner 上宿主 pid 752 干净退出后被系统进程复用，`refreshDescendants` 从复用者出发把 126 个系统进程记成后代，退出确认 20 秒超时且强杀清单险些覆盖系统进程。修复：宿主退出后不再从 hostPid 走树 + 存活/强杀清单按进程名身份校验（`descendantsAliveByName` + 单测）。
  4. **测试架（`ce14a4f` + `138f06f`）**：runner 的 TEMP 含 8.3 短名（RUNNER~1），两处按字面比较 canonical 化路径的断言必然失配（本机 admin 无短名故 9 连跑通过）。修复：改按 `canonicalize`/`realpathSync.native` 身份比较。
- **远程验证（同一 HEAD 的 PR 合并 SHA `46bec84`，内容 = HEAD + 未变 main）**：
  - CI run [39120833826](https://github.com/ywltby/tokenscope/actions/runs/39120833826) ✅（rust / rust-acceptance / contracts / frontend 四组 + gate）。
  - Native UI E2E run [39120833933](https://github.com/ywltby/tokenscope/actions/runs/39120833933) ✅：6 场景工件核对——`ui-refresh` 11、`ui-system` ×3 各 15（含 1.5/2 缩放）、`ui-collection-status` 20、**`ui-source-paths` 12（新场景非空全过）**；`failed=[]`、无场景超时、`real_untouched` 全部为真（真实 `~/.tokenscope` 指纹前后一致）。
  - Install test run [39120833790](https://github.com/ywltby/tokenscope/actions/runs/39120833790) ✅：install-test + 升级矩阵 4 起点（0.1.0/0.4.0 × present/deleted）全绿（含本 PR 新登记的 v0.4.0）；升级证据核对 `settings_format` 指纹 = **2**（新值）、历史库 schema 4→4 不变；`upgrade-chain` 在 PR 上按设计跳过（main 发布阶段另取证，不计为通过）。
- **PR**：草稿 [PR #14](https://github.com/ywltby/tokenscope/pull/14) 面向 main，等待复核。按约定停在草稿 PR：不自行转正式、不合并、不清理分支。
- **边界如实记录（剩余待验，不随本 PR 关闭）**：真实数据性能基准（`perf_real_data`）待验（§4.4）；采集层权限拒绝 fixture 待验（S03 遗留）；`upgrade-chain` 连续升级链待 main 发布阶段取证；真实跨午夜、长时间休眠、跨显示器在[独立系统待办](../system-acceptance-todo.md)，不纳入本 PR。

### 审阅补修记录（2026-10-18，审阅 §23 / PR #14）

- **结论与范围**：复核判定"补修一项后再合并"——跨应用冲突的实际行为与 §4.1/§8.5、stats-semantics §2.2、CLAUDE.md SF09 不符：共用遍历台账 + Claude 恒排 Codex 前，跨应用同根时后到应用被台账挡下空手而归、其文件由先到应用的解析器读取（"先到者获胜"），嵌套反向时甚至无诊断；原测试把错误行为写成预期且残留 DBG 输出。同一 PR 补修，版本不递增（仍 0.5.0）。
- **补修 1（发现层集中判定）**：`ResolvedRoot` 增 `conflict_with`（对方显示路径）；`resolve_sources` 在有效启用集合上两两比较规范化身份键（`normalized_key`，canonical 优先），不同应用相同或按路径分量互为父子 → **双方**都标记冲突并加诊断"与 <应用> 的 <路径> 冲突（相同或互为父子）：本轮双方都不读取"。同应用嵌套属合法重叠：被认领方（子根）行加"与 <父根> 重叠，文件由 <父根> 读取"诊断（父根较小先认领，数据不丢不重）。
- **补修 2（壳侧）**：`source_plans_from_with` 排除 `conflict_with` 非空的行；`source_paths_list` 冲突行 state = `conflicted`（诊断照常下发）。`discover_round` 不再承担跨应用隔离，台账 `Already(其他应用)` 分支保留诊断作兜底；文件级认领隔离仅服务遗留注入路径（测试/基准直接构造单根来源）。
- **补修 3（测试改写）**：删除 `cross_app_same_root_isolated_and_other_roots_continue` 的"先到者获胜"预期与两行 DBG；新增 `cross_app_same_root_both_sides_conflicted_and_excluded`、`cross_app_nested_claude_parent_codex_child_both_excluded`、`cross_app_nested_codex_parent_claude_child_both_excluded`（走 `resolve_sources` → 壳侧同规则过滤 → 真实采集，断言冲突根双方都不读取、仅独立根 1 条请求计入、无跨应用兜底诊断）；`source_discovery.rs` 增 4 例（同根双方标记、嵌套双方标记、停用侧不冲突、同应用子根重叠诊断）；壳侧增 `cross_app_conflicted_roots_excluded_from_plans`（手写 TOML 构造——设置页 add 已拒绝跨应用，断言计划只剩独立根、列表双方行 state=conflicted 且诊断可见）。
- **补修 4（界面）**：`SourcePathRow.state` 增 `conflicted`；设置页冲突行显示「冲突」，诊断写明对方路径；同应用重叠行显示"与 X 重叠，文件由 X 读取"。前端 563 用例全绿（含冲突行渲染用例）。
- **补修 5（文档）**：stats-semantics §2.2 改写为"跨应用根冲突 → 发现层整组隔离"（含先到者获胜的机理说明与台账兜底定位）、同应用嵌套补读取去向诊断；CLAUDE.md SF09 与 DESIGN.md 状态清单同步。
- **测试架加固（`2205827`，补修后 CI 取证）**：ui-system 的 `all_time_restore_does_not_query` / `page_switch_preserves_scroll_and_query` 一次失败，排查结论：预设切换存在**迟到的重复批次**（失败运行在切换后 1.6 s 出现第二个采集轮；通过的运行同样有查询 begin 无汇总完成的批次）——既有竞态，非补修引入（补修不触及查询路径，夹具数据写入与计划逐项一致）；失败运行的证据另记录 runner 窗口最小化被 OS 接受但页面 1.2 s 后仍报告可见（`minimizedVisibility=visible`），窗口时序放大了竞态。修复：恢复检查前先等查询身份稳定（2 秒无变化）再取基线，检查只对"恢复本身不触发查询"负责；切页检查保持对照切页前稳定基线的完整强度。补修二进制本地 ui-system 15/15。
- **验证**：`source_discovery` 18/18、`source_paths_collection` 16/16、壳 57 用例、前端 563 用例；fmt/双 feature clippy/两套 workspace 测试全绿。版本不递增；`upgrade-chain`、真实性能基准等待验项维持 §23.3 的既有披露（同应用父根为手动时子根映射丢失，按审阅意见本 PR 不处理）。预设切换的重复批次竞态如实披露，留作后续观察，不在本 PR 扩大范围。

## 8. 执行细则、算法边界与验收门槛

共同执行、证据和合并门槛见 [PR1 第 4 节](../archive/implemented/2026-10-15-workspace-layout.md#41-开工与逐项执行)。PR2 合入前不开始 S01。先保持所有旧查询/计价/导入回归通过；路径管理不能改变事件业务身份或重写已有历史事实。

### 8.1 S01/S02：先解析配置，再接实际读取

1. settings.rs 先新增兼容反序列化和原子迁移测试，保留旧 dir 输入支持但新写出不带 dir。只通过 settings::update 修改来源字段，其他设置必须逐字段保持。坏配置直接报告，不用默认配置覆盖原文件。
2. 为 discovery.rs 定义注入上下文（当前用户主目录、启动环境、文件系统探测器、映射绑定），返回排序稳定的来源行与诊断。测试不调用真实 home/env；生产启动只捕获一次环境，不能在每轮偷偷重读改变范围。
3. 候选构造固定为默认三根、两个环境变量派生根、手动列表；按配置应用总开关/自动开关/排除项/追加项开关。仅探测这些候选，不枚举 C:/Users，不扫描主目录找工具，不提权。
4. 先做逻辑路径规范化，再对可访问根探测实际身份；同根合并标签。不要把“不存在”“无权限”“映射未知”都合成缺失。环境相对路径忽略并诊断，不能拼 cwd。
5. S02 核实 CLAUDE_CONFIG_DIR 映射位置并记录证据；未核实采用该根禁用映射的明确降级，不借用默认映射、不阻断其他合法根。手动路径始终不猜映射。
6. 配置迁移只负责字段与启停；S03 完成后再通过真实采集入口验证升级用量。S01 可以记“配置契约通过，升级集成待 S03”，不能提前勾完升级断言。

### 8.2 S03：按整轮集合遍历，不逐根重复扫描

建议分为 `resolve_roots → freeze_context → walk_unique_files → parse → commit_history` 五步。整轮共享目录访问集合与文件身份集合；不要为每个根创建独立 visited 再拼接结果。

1. 采集开始冻结根、映射及设置修订。跨应用冲突先分组隔离，再遍历其他合法根；结果不能随根输入顺序改变。同应用嵌套遍历保留路径覆盖，整轮每个可解析的规范化目录/文件路径最多处理一次；硬链接例外按 §0 处理。
2. 使用显式栈及 DirEntry::file_type；链接先解析再访问，加入目录身份集合后才递归。遇到断链/权限/子项错误累计结构化诊断；同轮去重，不每文件打 info。
3. 文件按规范化路径去重。候选先排序或在解析前完成归属选择，确保最小路径认领；禁止边遇到边解析后再改认领者，造成输入顺序相关的重复解析。
4. 物理不同的复制文件各自解析，历史事实靠现有去重求并集；两个独立合法请求不得因为 token 相同或文件名相同被删掉。
5. 解析进度沿用现有 `source_files` 与内容变化检测；映射上下文改变必须重解析。不改历史库表结构。
6. 活跃文件仍由现有增量解析处理。
7. 所有最终写入继续经过既有串行采集与历史事务；配置保存不能直接旁路启动第二条扫描链。复用结果键包含本轮来源上下文，不让旧来源结果用于新设置。

新增 Windows junction 测试只在临时根创建环，按 §0 第 7 条执行，不强制子进程；测试清理先解除临时链接再删除临时树，不能沿链接清理目标。环境不支持链接时本地记待验，在支持的 Windows runner 上补验，不能把 skip 算通过；不增加文件 ID 验收。

### 8.3 S01–S04 必需事实矩阵

断言按 §0 第 5 条取舍：统计类断言总量与请求身份集合，去重类再断言解析次数，诊断类断言诊断；只断言文件数不够。重复记录使用相同原生身份，独立记录使用不同原生身份，不能用 Codex 弱指纹无法区分的样例冒充独立请求。

| 合成场景 | 必须结果 |
| --- | --- |
| 旧 dir 有 A，默认根有 B 和 A 的副本 | 升级后恰为 A∪B；重复迁移/采集不增量；停用来源时均不读取 |
| 默认与环境指向同一根或联接别名 | 合并来源标签，每文件解析一次，不重复报缺失 |
| 手动新增已有根的父/子（包括停用、缺失行） | 保存失败、原配置字节不变、错误指出冲突；同根只提示已有行 |
| 迁移/环境产生嵌套 | 保留配置并标重叠、有效文件不漏且仅解析一次；不采用子根覆盖 |
| 两个 adapter 认领同一根/文件 | 诊断归属冲突并隔离，不按扫描顺序选择；其他根继续 |
| 祖先环、深目录、断链和权限失败 | 受控时间内终止；可读用量完整；错误分类正确 |
| 物理独立复制一份 | 两份都解析，事实层去重，统计不增量；独立原生身份不丢 |
| 文件截断、消失重现、链接改目标 | 不沿用错误游标；诊断和有效事实正确 |
| 默认与环境同 slug、各自映射不同 | 项目归属分别正确；手动根不借任一映射 |
| 采集中保存设置、随后触发新轮次 | 本轮仍用冻结集合；下一轮使用新集合；无并发写库 |
| 停用/删除来源后重启 | 不再读取该根；历史事实、已保存统计仍在 |

在 `source_paths_settings`、`source_discovery`、`source_paths_collection` 三个测试文件中给上述用例可搜索名称，执行记录逐项映射测试名，不能仅写“相关测试均通过”。

### 8.4 S05/S06：UI、性能和实际接线

1. 列表展示后端返回的行，不在 Vue 重算路径身份/重叠。新增、移除、启停请求串行；保存成功后重读，不以乐观删除掩盖后端拒绝；失败保留输入草稿。
2. 浏览器 E2E 覆盖键盘添加、取消、同根、父/子拒绝、合并标签、停用持久化和长路径。原生 E2E 按 §0 第 6 条从设置界面操作后追加合成日志，等待真实下一轮读取验证统计；不能只 mock IPC 得出接线完成结论。
3. 性能按第 4.4 节记录。真实数据基准没跑就单列待验。
4. S07 在配置、算法、UI、原生隔离验收各自通过后进入交付检查。剩余真实性能待验须明确披露，不把它写成全部验收完成；三个既有系统 todo 不再纳入本 PR 工作。

## 9. 按当前代码展开的执行清单（2026-10-18）

本节补充实现路径，不恢复 §0 已删除的功能或性能门槛。生产实现与测试分开提交可行，但每个 S 项收尾必须有可运行的验证。未完成项留待办，不以“已写代码”代替通过。当前只细化计划，以下均未实施。

### 9.0 开工检查与共享契约

1. 留在 `feat/source-paths`，确认工作区和远端状态；保留已有 PR2 归档及本计划提交。读 CLAUDE.md、DESIGN.md、本计划 §0 和最新审阅意见。不再新建分支，不实施 PR3 以外的产品功能。
2. 保存改前性能证据或明确登记“未取得基线”。只用已有 opt-in 方法与隔离数据目录；不能为了性能对比动真实来源或清空真实历史库。记录数据规模、构建模式、来源集合以及三次热轮耗时，改后用相同条件比较。
3. 一轮采集的数据流固定为：读取最新设置 → 解析固定候选 → 冻结根和映射 → 单飞/串行采集 → 整轮发现及去重 → 既有解析/历史事务。实际 running 仍在获得采集串行锁后才发出，冻结过程不能制造第二套 start/end。
4. `source_paths_list` 只探测少量根，不遍历日志、不触发查询、不写历史。需要文件系统访问的 IPC 一律经 `run_blocking`，不在 Tauri 同步 command 主线程做 I/O。
5. 普通核心测试显式注入空或合成的来源上下文、`cache_dir`、`pricing_index`；`Some(explicit_root)` 的测试兼容入口只读注入根，不能因新默认值偷偷追加真实 home。生产 GUI 使用新的集合入口，不能继续借此兼容入口表示“追加”。

### 9.1 S00：先补原生测试架有界收尾

**落点：** `frontend/scripts/native-acceptance.mjs`，`frontend/scripts/contracts/native-{ui-system,ui-refresh,collection-status}.mjs`，`frontend/scripts/lib/native-instance*.mjs` 及对应测试。必要时新增小型 `native-scenario.mjs`/`.test.mjs`，只放共用阶段与收尾逻辑，不重写实例管理。

1. 先列出所有同步子进程、`browser.close()`、失败截图/DOM 查询及最终落盘点。当前主入口已经有场景 `Promise.race`，但失败截图和 `stop()` 在其保护外；仅改 race 的数字不能解决卡死。保存现有场景名称/检查项清单，后续不得删断言换绿。
2. 给同步命令统一增加 30 秒 `timeout`；同时检查 `result.error`、`status`、`signal`，不要仅检查 stdout。超时错误至少含动作、阶段、命令退出情况，不输出凭据。同步阻塞会推迟 JS 定时器，因此 5 分钟不是精确墙钟强杀点；预算最多还需容纳当前这一次有界同步调用。
3. 抽出共用阶段记录 `markStage(name)`，调用前写阶段、启动序号及时间，调用后记录完成。阶段至少覆盖 `close-command`、`click-quit`、`wait-host-exit`、`stop`、`relaunch`。保留 `failure_stage` 的现有大类，新增 `last_stage`，不要在 finally 用“cleanup”覆盖真正失败阶段。
4. 场景超时先设置取消标志，追加失败检查并同步写最小证据，然后执行有界收尾。最小证据不依赖 CDP：场景、最后阶段、已记录 PID、`exitCode`（未知为 null）、`launches` 和 `failed`。后续增强日志/截图失败不能阻止这份证据落盘。
5. 所有断开 CDP 的路径（含契约直接调用与 `expire-start`）复用 10 秒有界关闭。超时记录原因为 close timeout，继续既有进程退出确认；不要把该次超时悄悄算作通过。失败截图/DOM 取证也必须有短上限，无法完成就跳过并记原因。异常收尾后明确非零退出，不能因悬挂 Promise/连接留在事件循环中。
6. `Promise.race` 不会取消另一分支。场景在每次关闭/重启和产生新副作用前检查取消标志；超时后不得再次 launch。收尾不重入，清除看门狗 timer；只处理本次自己启动的 PID，attach 模式不终止外部实例。保留现有 20 秒停止确认预算，不新增无界轮询。
7. 增加可控替身测试：同步命令超时、CDP close 永不返回、场景永不返回、失败截图永不返回、正常路径解除 timer、超时后的迟到分支不重启、attach 不杀外部进程。至少一条通过 Node 子进程运行共用收尾入口，父测试设上限，断言磁盘证据与非零退出；只断言 mock 回调被调用不能证明进程能结束。
8. 运行 `pnpm --dir frontend test` 和 `python -B scripts/check_node_syntax.py`；提交 S00 后才开始 S01。远程 Native UI E2E 仍是交付条件；不能在没有新证据时声称 main 原卡死原因已修复。

### 9.2 S01：区分缺字段与显式空值，落盘后才扩大读取

**落点：** `crates/tokenscope-core/src/settings.rs`、`crates/tokenscope-core/tests/source_paths_settings.rs`；升级联动 `scripts/fixtures/settings_migrations.json`、`scripts/test_upgrade_fixture.py` 与其调用的迁移校验实现。

1. 先写旧 TOML/JSON 的最小 fixture：无 sources、`dir` 为空/非空、enabled=false、已有新字段、显式 `additional_paths=[]`、其他设置非默认值。新格式只输出 enabled/auto_discover/excluded_auto_paths/additional_paths。
2. 使用输入 DTO 或自定义反序列化保留“新字段是否出现”的信息；不要先 `#[serde(default)]` 成空数组再猜新旧格式。任一新格式字段出现时按新配置解释，旧 dir 不再补入。字段缺省的默认值明确为 auto=true、列表为空。
3. 迁移在设置事务锁内读最新文件、转换并原子保存，保存成功才向采集发布新设置。不能把迁移写入塞进 `update()` 调用的同一个加锁 `load()` 而形成递归锁。迁移保存失败返回明确错误，原文件字节不变，不回退成“全部默认启用”。
4. 旧相对 dir 如确实存在，先按旧入口的解释转为绝对追加路径再保存；若无法保持原语义，返回可操作诊断并保留旧文件，不能把它丢掉后默默扫描默认根。空旧 dir 不产生空路径行。
5. 原有字段必须逐字段保留：隐私同意、价格自动同步、关闭行为、时区、读取间隔及另一应用配置。迁移提示由本次迁移结果返回；不要每轮把不存在的 dir 当作“又迁了一次”。
6. 本次确实改变旧设置解释语义，按 `SETTINGS_FORMAT_REVISION` 注释递增设置格式（当前 1 → 2），同步持久格式输出与升级规则；不是只 bump 应用版本。当前声明式规则仅支持 identity/rename/default，无法表达 dir→追加对象数组：增加一个有限、专用的来源迁移操作及契约，不能用恒等规则或忽略 sources 字段蒙混升级断言。保留旧 `1->1`，补 `1->2` 和 `2->2`；变换的预期独立于被测应用。
7. 测试原子失败、重复迁移、并发改时区/来源不丢字段；完成配置测试后记录“升级用量待 S03”，不提前关闭集成验收。

### 9.3 S02：统一解析模型与行操作含义

**落点：** 新增 `source/discovery.rs`，复用 `source/project_path.rs`/`report.rs` 的规范化能力，扩展 `source/claude_projects.rs` 的映射注入；不要再写第三套字符串前缀判重。

1. 建议类型职责：`DiscoveryContext` 持有注入 home/启动环境/探测器；`ResolvedRoot` 持有 agent、原始显示路径、规范化比较键、origin 列表、映射绑定、有效启停、诊断；`ResolvedSources` 持有稳定排序的根集合和上下文签名。名字可随代码习惯调整，职责不可合并成全局可变列表。
2. 生产只在启动时捕获环境，测试直接构造上下文，禁止并行测试修改进程全局环境。acceptance 强制使用隔离 home 与空环境覆盖，再由场景显式注入环境根，不能“未提供就读本机”。
3. 先保留每个 origin 的配置路径与开关，再合并同一规范化根；行有效 enabled = 应用总开关且至少一个 origin 有效。自动 origin 同时受 auto_discover/excluded_auto_paths 控制，手动 origin 受自己的 enabled 控制。两个总开关不改写各行用户选择，重新开启时恢复原选择。
4. 行启停作用于该行全部 origin：禁用时把自动 origin 的路径加入排除项、把手动项设 false；开启做逆操作。移除仅删除手动项，自动标签仍在且保持原启停。明确测试“默认+手动合并行”的禁用→重启→移除→开启，防止移除手动后悄悄恢复自动读取。
5. 排除项保存稳定的绝对配置路径，匹配时用同一规范化函数；不保存探测结果列表。显示行 id 不用数组下标，写接口收到 id 后按最新解析结果定位；行已消失就返回“来源已变化，请重试”，不能操作相邻行。重启后自动禁用选择仍有效，缺失后重现不复活。
6. 同根标签合并、嵌套比较按路径分量，不把 `C:/a` 与 `C:/ab` 判成嵌套。可访问根 canonicalize；不可访问保留逻辑键与身份未确认诊断。默认缺失可隐藏，但仍参与手动新增校验；列表隐藏不等于删除候选。
7. 通过官方资料或隔离实测核实环境 Claude 映射并回填 §2（来源、日期、结论）；无证据明确禁用。对同根绑定冲突保留已核实默认绑定并诊断，不把另一根映射混入。规范化后的映射路径及内容修订参与上下文签名。
8. 用探测器注入 NotFound/PermissionDenied/普通文件/失效链接分类；真实文件系统 fixture 验证下一轮新目录可见及链接目标改变。S02 输出只负责解析，不偷偷打开历史库。

### 9.4 S03：冻结发现清单后再解析

1. 保留当前适配器逐文件 `parse_file` 和既有历史事务；增加整轮发现清单，让 report 按根实例与文件条目消费。Claude 每根使用独立 `ClaudeSource::with_mapping`，不能把所有根放进一个带默认映射的 ClaudeSource。Codex 可复用多根能力，但不能为每根重新创建独立 visited。
2. 文件条目区分 `read_path`、规范化去重键、所属 root。去重键只用于判重，不能直接拿 canonical 文件路径替换解析访问路径：Claude 的 slug 提取依赖文件相对其 root 的路径。根与访问路径必须成对，联接/别名下也能正确 strip_prefix。
3. 对排序后的根做显式栈遍历，目录访问键用已解析目标路径；普通目录也在进入前加入 visited。文件先收集再确定最小规范化根认领者，然后解析。同根多标签不多扫描；父根/子根同应用重叠不提前删除子树。遇到跨应用同文件认领时，在解析前把该冲突文件排除并给两侧诊断，其余文件继续。
4. 跨应用根同/嵌套先隔离冲突根集合；停用根不参与实际采集隔离，不能导致正常根无故停读。手动新增按 §4.1 对全部已配置行校验；停用/移除是解除冲突的操作，不能被旧重叠校验反过来挡住。
5. 显式处理每个 read_dir Result，不用 flatten；链接目标无法解析则跳过并诊断。Windows junction 可能表现为 reparse point，不能只假设 `file_type().is_symlink()` 覆盖全部联接；实际 junction fixture 必须证明祖先环终止。这里仅解析目标路径，不恢复文件 ID 方案。
6. `source_files` 的路径及 root 规则尽量保留；上下文修订包含根集合、映射、启停与探测结果的有关部分。不要使用每轮随机值/当前时间做签名导致热轮全失效。无新增事实的扫描不增加 history generation，不因列表变化而伪造用量更新。
7. 反例在 §8.3 基础上再钉住：交换根输入顺序结果相同；新默认目录在下一轮被发现；相同日志复制/移动仅事实去重；联接换目标不复用旧映射；同根别名不重复解析。统计用相同原生身份 A 与不同原生身份 B，不能用不同文件名充当请求身份。
8. 跑三个新增集成文件和 `ui_adjustment_collect`；补跑已有历史、映射、查询测试（完整 workspace 门禁覆盖）。把 S01 留下的旧自定义目录升级用量用例在此关闭。

### 9.5 S04/S05：替换单根入口，接好保存与刷新

1. `commands.rs` 当前 `source_settings_impl` 返回两个可选 dir 和 enabled，至少被 query_opts、collect_opts、list_events、rebuild_opts 使用。逐处换为共用的集合解析入口，并核对 source_status、启动采集、定时采集、手动刷新、缓存重建；不能只改设置页调用。
2. 保留汇总消费的应用级 `source_status` 统计兼容面；设置页改用 `source_paths_list` 的根行。`source_config_set` 若保留兼容入口，只允许总开关等明确操作；旧 dir 写入不得再覆盖整份新列表。同步 `generate_handler!` 注册、隐私白名单和测试命令清单。
3. IPC 最小契约：list 返回 roots+诊断；add(agent,path) 返回 added/already_exists 与行 id；remove(agent,id) 只移除手动项；set_enabled(agent,id,enabled) 按 §9.3 操作全部标签。总开关/auto 开关走设置单项事务。新增与启用时后端重新校验，不信任前端传入 canonical_identity/mapping_path。
4. 保存成功后才失效复用结果并请求既有串行采集；多次快速保存可合并请求，但最后配置必须被读取。保存失败不触发新采集。正在执行的一轮继续用已冻结来源；自动查询继续只查已保存历史，不因列表改造退回扫描。
5. Settings.vue 删除原单目录编辑/保存语义，输入框只作追加草稿；开关改完即保存，添加是明确动作。操作串行队列必须在失败后可继续，不能一个 rejected Promise 毒死后续保存。成功保存但重读失败提示“已保存，列表刷新失败”，不得提示保存失败后让用户重复添加。
6. 展示稳定 id、完整路径、多标签、有效状态和操作；列表以内容换行避免横向撑破，读取间隔仍沿用原自动保存逻辑。总开关关闭时行可编辑但不采集；显示原因而非把每项持久开关改 false。使用 Naive UI/既有 DESIGN 控件，不自造主题颜色。
7. Settings 测试覆盖新增/取消、重复提交、失败保草稿、队列继续、旧列表响应晚到、多标签行启停/移除、缺失路径保存、当前应用警告过滤；useUsageQuery 保留分页与隐藏保护，前端不重算路径归属。

### 9.6 S06/S07：逐项证据与交付判定

1. 浏览器测试扩展现有 mock IPC，同步新增命令与 DTO；主题/尺寸复用现有矩阵，不另造测试框架。原生新场景建议 `ui-source-paths`，沿用 S00 生命周期与阶段记录，端口加入现有分配/契约；只用隔离根。
2. 原生主线：从 UI 添加**自动候选之外**的合成目录 → 确认列表追加且默认来源仍在 → 写入一条新的 Claude 原生请求 → 下一轮统计 +1 且汇总/明细同一查询 → 正常退出/重启 → 追加项仍在且同一请求不再次计数。期间不点击刷新来掩盖自动接线问题；失败保留日志和 evidence.json。
3. 实际运行命令以 `native-acceptance.mjs --help`/当前参数解析为准，将完整参数回填执行记录；构建用根 target 的 acceptance 二进制，不能误用生产包或上轮旧 exe。将新场景接入 `.github/workflows/ui-e2e.yml` 的 always 工件上传范围。
4. 升级验收除现有安装矩阵，还要核对设置迁移新格式：旧非空 dir、旧禁用来源和显式新空列表的独立契约必须执行；设置格式指纹为新值，历史库格式不变。不得编辑历史版本登记表里的格式指纹来适配新版。
5. S07 前读取最新 origin/main，登记实际上一发布版本（已登记不重复写），按规约分配一次 minor。当前计划阶段不 bump。更新 stats-semantics、privacy、DESIGN、CLAUDE、计划总账；构建输入清单路径若未变可保持内容不变，但 `--check` 必须通过。
6. 本地完整门禁按 §6 S07 与当前 `.github/actions/checks/action.yml` 执行；脚本契约可用 `python -B -m unittest discover -s scripts -p "test_*.py"`，PowerShell 安装/升级契约用 checks 中实际命令。浏览器 E2E 前必须完成 frontend build。不要只运行新增测试就报告完整门禁。
7. 用同一最终 HEAD 的 CI、Native UI E2E、安装与选出的全部升级矩阵结果判断就绪；原生下载并检查新场景非空、checks 全通过、失败字段为空、真实数据未触碰。PR 上按设计跳过的连续升级链不写成通过；main 发布阶段另取证。不自行合并。
8. 每个 S 项在 §7 下登记：提交、关键测试名、实际执行命令与结果、未完成项。S00 单列“防卡死机制已验证 / 原事故根因仍未知”；性能未运行保留待验；系统 todo 不加入本 PR。出现失败先定位再修，不靠延长时间或重复跑绿销项。
