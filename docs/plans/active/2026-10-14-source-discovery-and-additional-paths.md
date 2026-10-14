# PR3：当前用户日志目录发现与追加路径管理

> 状态：按 [1eaede7 审阅意见](../../reviews/2026-10-15-source-discovery-plan-review.md)修订，待复审；S01–S07 均未实施。
> 前置条件：[PR1 目录迁移](2026-10-15-workspace-layout.md)和 [PR2 扫描反馈](2026-10-15-collection-status.md)依次合入 main。届时从最新 main 创建 `feat/source-paths`，独立 minor 版本、独立 PR 和验收。当前设计修订仍提交到 `docs/source-discovery-plan`。

**Goal：** 默认发现当前用户的标准日志目录与环境变量指定的目录，以追加列表管理来源；来源重叠不重复解析或计量，遍历链接不会死循环。

**Architecture：** 每轮采集探测固定候选并冻结来源集合；路径、链接和文件物理身份去重后交给既有适配器。历史事实、查询会话和 PR2 扫描生命周期保持既有语义。

**Tech Stack：** Rust workspace、Tauri 2、Vue 3 / Naive UI、TOML、SQLite、Vitest、隔离原生 E2E。

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
| CLAUDE_CONFIG_DIR | `<CLAUDE_CONFIG_DIR>/projects` | 实施前核实 |
| 手动追加 | 用户明确指定的绝对日志根 | 禁用映射，沿用 A03 |

CLAUDE_CONFIG_DIR 映射文件位置必须在 S02 实施前通过官方文档或隔离合成实测核实，将证据回填本节；目前不假定为 `<CLAUDE_CONFIG_DIR>/.claude.json`。无法核实则禁用该根映射，不借用默认映射。默认和环境根同名 slug 必须测试不串映射；相同物理根合并时优先已核实默认绑定，冲突不猜测、需诊断。

环境变量只使用 TokenScope 启动时继承的值：空串忽略，相对路径忽略并诊断，不存在的绝对根显示缺失、不每轮报错。修改后需要重启；自启进程未必继承交互 shell 中设置的变量，设置帮助和文档必须说明。测试和 acceptance 注入主目录、环境及探测器，完全忽略真实主目录和环境变量。

隐私 Ready 后，每轮开始探测固定路径并冻结集合及映射。来源状态查询使用同一解析逻辑但不扫描用量。默认根不存在可不显示；环境/手动缺失根保留行，权限失败显示无权限。采集中配置变化只影响下一轮，始终串行。根规范化和物理身份按集合缓存；存在性/链接元数据变化、配置变化、别名目标变化需失效，不能只按路径字符串永久缓存。复用键包含实际集合、启停、映射和上下文签名。

**行为变更：** 旧 CODEX_HOME 替代默认 `.codex`，新行为追加到默认根；升级后默认根旧日志可能进入统计使总量增加。相对 CODEX_HOME 原先按 cwd 解释，新行为忽略并诊断。CLAUDE_CONFIG_DIR 是新增支持，采用同样追加语义。写入 stats-semantics 来源章节、设置帮助、PR3“行为变更”及版本摘要。

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

旧根等于自动根则合并来源标签；旧 Codex dir 为 `~/.codex`、包含自动 sessions/archive 子根时仍迁移成功，标为“与 X 重叠”，在一次性迁移说明中提示移除/停用。总开关原本关闭时不开始采集。说明“原自定义目录已保留为追加目录；自动发现已开启”，提供关闭自动发现入口。

## 4. 嵌套、遍历与三层去重

### 4.1 嵌套规则

同应用手动添加与全部已有行比较（默认、环境、手动、停用、缺失均包含）：同根返回已有行并提示“目录已在列表中”，不新增；上级或子目录直接拒绝，注明冲突行。可访问时同时比较物理目标，防止联接别名绕过；不可访问时执行逻辑路径边界比较并显示身份未确认。

迁移和环境变化产生的同应用嵌套不阻断，标重叠、提示移除手动行或停用自动行，文件层确保只解析一次；不采用子根覆盖父根。跨应用相同/嵌套继续拒绝手动事务；自动出现时隔离冲突集合，其余正常读取，不用“先到者获胜”。

### 4.2 遍历环必修

现有 walk_jsonl 以 child.is_dir() 跟随目录链接且无访问集合，祖先联接会无限递归。改用 DirEntry::file_type() 不跟随分型，显式解析链接目标；维护整轮已访问目录物理身份。已访问或归另一个已启用根负责的目标跳过，其他目标只跟随一次；跳过需确保目标确实由其他根遍历，不漏读。无稳定身份时用解析目标路径防环，无法解析链接则跳过并诊断，不能无界递归。

使用显式栈避免深目录耗尽调用栈；read_dir 子项错误需保留，不能 entries.flatten 静默吞错。祖先联接回归放在有超时的受控子进程，修复前失败不得拖死测试 runner，修复后断言终止、文件齐全且只一次。

### 4.3 文件身份与缓存

1. **先路径**：规范化绝对路径，处理分隔符、盘符、Windows 大小写、尾分隔符；可访问时统一 canonicalize 的 `\\?\` 前缀/UNC、8.3 短名。非 Windows 不折叠大小写；Windows 敏感目录必要时用身份确认，不能误合并独立文件。
2. **再链接**：目录链接按 4.2 防环；文件符号链接用目标身份。路径去重先于昂贵句柄操作。
3. **最后硬链接**：Windows 卷序列号＋128 位 FILE_ID_INFO 优先，取不到才诊断并退 64 位文件索引；FAT/exFAT/共享等无稳定 ID 时退路径、标“身份未确认”，不按内容猜同文件。其他平台使用对应物理身份。

文件 ID 按（路径、大小、修改时间）缓存，仅新增/变化文件打开句柄；消失/重现、链接目标和上下文变化使缓存失效。不得宣称三元组能识别所有保持大小/时间的替换，记录限制并测试可观察失效情况。

同物理文件以规范化路径最小者认领，解析指纹/进度按物理身份＋规则及映射上下文维护；路径改变但上下文不变不重解析，上下文改变必须重解析以保归属。Codex 同卷 sessions→archived_sessions 身份保持的复用纳入本轮。不能先重复解析再靠历史库掩盖开销。

每根映射冻结，手动根不猜映射。物理文件身份不是请求身份：Claude message 身份、Codex 保守重播指纹不变；复制为不同文件仍由事实层求并集。停用和删除不删除历史。

### 4.4 性能预算

- 同机同数据 release 改前/改后各测多次中位数，记录数据规模与文件缓存条件。热目标仍约 0.1–0.2 秒量级，冷历史参考约 5.5 秒；两者相较本次实测基线不超过 +10%，不能用历史单次数字冒充本轮基线。
- 合成：2 根、约 2 万文件、约 1 千硬链接、1 个联接环。记录热轮耗时和身份句柄次数，第二轮未变文件的身份句柄次数应为 0，固定根探测单列，整体接近 0。
- 固定来源解析应为毫秒级，来源解析/文件身份阶段各自计时；auto debug、手动/启动 info，不逐文件刷屏。
- perf_real_data 仍须显式 opt-in；执行前说明真实来源读取和历史库写入影响，优先只读来源＋隔离数据库对比。其他自动化测试禁止访问真实数据。真实基准未跑就标待验，不用合成测试替代。

## 5. 设置页、接口与隐私

每应用总开关、自动开关；追加输入/目录选择/添加；下方列表，没有重新发现按钮。目录完整可复制换行；来源为默认自动/环境/手动多标签；状态为待读取、可读取、缺失、无权限、部分失败、停用、冲突、与 X 重叠。仅手动来源可移除，合并行移除只删手动标签；自动行可停用。

拒绝空值、相对路径、明显整盘/整个用户主目录、同应用嵌套和跨应用冲突；合法缺失绝对根可保存。选择取消不保存，输入不静默改写；错误保留草稿、成功清空、防重复提交。

IPC 保留 source_paths_list、source_path_add、source_path_remove、source_path_set_enabled，总开关走 settings 事务。写接口对最新 settings 单项修改，前端串行提交、完成后重读，用普通请求序号避免旧列表覆盖；不建立独立发现修订。配置成功失效采集复用键，串行请求新采集，沿用 PR2 状态和冻结查询。

解析行包含 id/agent/path/canonical_identity/origins/mapping_path/enabled/diagnostic，id 仅为配置展示身份。单应用过滤来源警告、全局警告保留，权限状态不伪报缺失，不请求额外权限，不重复刷 info。

docs/privacy.md 只补充当前用户默认、环境和多个手动路径，不新增同意流程。该文件通过 include_str! 编译进壳，必须命中 PR1 的构建输入清单和壳检查。

## 6. S01–S07 实施任务

每项使用 executing-plans 技能：失败测试→确认原因→最小实现→定向与必要门禁→独立提交。路径按 PR1 迁移后布局。

### S01 配置集合与迁移

- 修改 `crates/tokenscope-core/src/settings.rs`，新增 `crates/tokenscope-core/tests/source_paths_settings.rs`。
- 覆盖旧空/非空/停用、新旧并存、空数组、幂等、原子失败、并发事务、父根迁移重叠保留和说明、手动父/子拒绝（含停用和缺失行）。
- `cargo test --locked -p tokenscope --test source_paths_settings`。

### S02 固定候选与环境发现

- 新增 `crates/tokenscope-core/src/source/discovery.rs`、`crates/tokenscope-core/tests/source_discovery.rs`；修改 source/mod.rs、acceptance.rs。
- 注入主目录、启动环境、探测器；第 2 节环境/映射全部用例、新目录下一轮出现、权限/缺失区分、根身份缓存失效、毫秒级解析。CLAUDE_CONFIG_DIR 映射证据回填第 2 节。
- `cargo test --locked -p tokenscope --test source_discovery`。

### S03 防环、多根与去重

- 修改 `crates/tokenscope-core/src/report.rs`、`source/{mod,claude,codex,claude_projects}.rs`；新增 `crates/tokenscope-core/tests/source_paths_collection.rs`。
- 环/深目录/断链/读错、重复路径/短名/前缀/符号链接/硬链接、稳定认领者、归档移动、上下文改变、默认环境同 slug 不串映射、无 ID 降级均测试。
- 覆盖迁移/环境重叠仅解析一次、手动嵌套拒绝、跨应用冲突隔离、复制求并集、独立事件不丢、停用保留、CODEX_HOME 升级。
- `cargo test --locked -p tokenscope --test source_paths_collection` 与 `cargo test --locked -p tokenscope --test ui_adjustment_collect`；执行第 4.4 节预算。

### S04 壳事务与调度

- 修改 `src-tauri/src/{commands,lib,privacy}.rs`，测 Ready 前拒绝、每轮冻结、配置与采集交错、事务失败不扩范围、退出后不采集、PR2 状态一致。
- `cargo test --locked -p tokenscope-app`。

### S05 来源列表

- 修改 `frontend/src/types.ts`、Settings.vue/Settings.test.ts、useUsageQuery.ts 及测试。
- 多来源标签、追加不覆盖、父/子拒绝、同根提示、非手动重叠、启停/移除、错误保草稿、保存重读、长路径和可访问名称、无发现按钮、环境重启提示。
- `pnpm --dir frontend test`、`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`。

### S06 浏览器、原生与性能

- 扩展 `frontend/scripts/fixtures/ui-contracts.mjs`、`contracts/interaction-contracts.mjs`、`run-ui-refresh-e2e.mjs`、`native-acceptance.mjs`，接入 Native UI E2E。
- 合成当前主目录、两个环境根和手动根，逐请求 manifest/token/cwd/映射检查，归档及子代理、重复刷新/重启不增量；不回退真实主目录/环境。明暗主题、1280×820 / 980×620、键盘与长路径。
- 两万文件/硬链接/环、热轮句柄次数及真实 opt-in 基准按 4.4 执行，失败不降低预算；实跑前后结果回填。
- `pnpm --dir frontend build`、`pnpm --dir frontend test:e2e`，原生隔离工件单独记录。

### S07 PR3 文档、版本与交付

- 更新 CLAUDE.md、DESIGN.md、docs/stats-semantics.md、docs/privacy.md、计划和总账；新增引用按 PR1 更新构建输入清单。
- fetch 最新 main，独立 minor bump，摘要明确“日志目录自动发现与追加目录管理；CODEX_HOME 追加默认根，相对环境路径不再读取”；不预定版本号。
- 门禁：`cargo fmt --all -- --check`；`cargo clippy --locked --workspace --all-targets -- -D warnings`；`cargo test --locked --workspace`；clippy/test 再加 `--features acceptance`；前端 typecheck/format:check/test/build/test:e2e；脚本契约和清单检查。
- 同一 HEAD 的 CI、安装升级、Native UI E2E 与有效工件通过才就绪，不自行合并。真实跨午夜/长时间休眠/跨显示器仍在独立系统 todo，不混入本 PR。

## 7. 状态与验收

S01–S07 全部未开始，版本未分配。实施后逐项记 commit、命令、失败→通过及远程 run/artifact，计划审阅不代表实现。

- [ ] 固定候选、两种环境变量、映射核实和隔离全部验证。
- [ ] 手动嵌套拒绝；迁移/环境重叠不阻断且只解析一次。
- [ ] 联接环闭合；三层去重不丢独立事件。
- [ ] 来源集合、列表、诊断一致；停用历史保留。
- [ ] 性能预算、合成句柄次数、真实基准分别有证据或待验标记。
- [ ] CODEX_HOME 行为变更、重启说明、隐私范围及清单齐全。
- [ ] PR3 自身版本、完整门禁、安装升级与原生工件验证。
