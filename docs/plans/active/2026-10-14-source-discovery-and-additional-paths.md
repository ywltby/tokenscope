# 来源目录、扫描状态反馈与 Cargo Workspace 统一实施计划

> 状态：**设计草案，待用户审阅；本轮不实施。**
> 2026-10-15 整合：目录发现 S01–S07、扫描反馈 F01–F05、构建整理 W01–W07 统一在本计划维护，全部未实施。原扫描反馈独立草稿已并入第 11 节，不再保留第二份待审计划。
> 执行时使用 executing-plans 技能逐项落实；用户明确要求优先于本计划。分支：`docs/source-discovery-plan`，基线为 main `e34495b`（0.3.1）。

**Goal：** 默认发现本机各用户可访问的 Claude Code / Codex 标准日志目录，设置页输入用于追加路径，以列表管理全部来源，重复目录和重复日志不重复计量。

同时让扫描开始/结束在界面上有真实状态反馈，并统一 Cargo workspace、锁文件、构建产物与门禁，清除旧 CLI 同名规避配置。

**统一执行顺序：** 用户批准后先 W01–W05 完成构建基础迁移，W06 验证基础产物；再 S01–S04 接通多根来源，F01–F02 接通真实采集状态；随后 S05 与 F03 完成界面；最后 S06 与 F04 联合 E2E。S07、F05、W07 合并为一次文档/版本/交付收尾，W06 的安装升级与当前 HEAD 取证在最终提交重验。三个任务组分别登记证据，不重复提版本、不重复建立同类门禁。

**Architecture：** 增加独立目录发现与解析层，将自动发现、手动追加、停用规则解析成带身份及诊断的来源集合。采集、状态展示、查询缓存使用同一份不可变解析结果；历史库的事件身份和保留语义不变。

**Tech Stack：** Rust、Windows 用户配置文件发现、Tauri 2 IPC、Vue 3 / Naive UI、TOML、现有 SQLite 历史库、Vitest / Playwright 合成验收。

## 1. 已确认需求与范围

- 默认发现 `C:\Users\*` 所代表的本机用户目录下的标准日志位置；不递归搜索整个用户目录，更不扫描整盘。
- 当前单目录输入改为“追加读取目录”，添加不会替换默认来源，全部路径以列表显示。
- 无论主代理、子代理、归档会话，只要是已有解析器支持的有效用量均纳入。
- 保留统一 `~/.tokenscope/history.db`，不按账户或来源目录拆库，不保存完整会话正文。
- 停用、删除配置或源日志消失只影响后续采集，已经保存的历史用量不删除。
- 目录发现部分只覆盖 Windows 多用户发现。其他平台继续发现当前用户的标准目录，手动追加多目录可用；不顺带开发 WSL、容器、SSH 或远程共享的自动发现。
- 本计划待确认，不分配发行版本；后续功能实现准备合入 main 时按当时最新基线递增 minor，同一 PR 不重复递增。

## 2. 现状与必须一起改的入口

| 当前实现 | 本轮要求 |
| --- | --- |
| `src/settings.rs::SourceConfig` 只有 `enabled`、`dir` | 改为自动发现开关、追加项、自动项停用清单，并兼容旧设置 |
| `src/report.rs::SummaryOptions` 传入单个 `claude_dir` / `codex_dir` | 生产入口统一传递解析后的多根来源集合 |
| `make_source` 按应用构造单一来源 | Claude 每根绑定自己的映射；Codex 多根扫描 |
| `claude_project_mapping` 默认使用当前账户 `.claude.json` | 多用户发现必须使用该目录所属账户的映射，禁止借用当前账户 |
| Codex 默认根包括 `sessions` 与 `archived_sessions` | 两者都保留并分别可见；不退回只读 sessions |
| `source_config_set` 保存一个覆盖目录 | 追加/启停/移除需在现有 settings 事务内完成 |
| `Settings.vue` 显示“当前生效目录” | 改为多根列表，状态与采集来源使用同一集合 |
| 最近采集复用键包含来源配置 | 键包含来源集合、目录启停、映射与发现修订，防止复用旧结果 |

必须检查手动刷新、后台采集、启动读取、source_status、诊断导出和 native 验收的所有调用点，不能只改设置页。

## 3. 自动发现规则

### 3.1 用户目录候选

1. Windows 生产实现从系统用户配置位置取得配置文件根目录与已登记的账户主目录，补充配置文件根目录的直接子目录（覆盖未登记但保留下来的用户目录）。不得硬编码 `C:`，也不得由显示用户名拼接路径。
2. 当前用户主目录始终作为候选补充；ProfileList 不可读时降级为可访问的配置文件根目录和当前用户，并给出一次发现诊断。系统根同样无法定位时只检查当前用户，不猜盘符、不扩展全盘。
3. 对候选只探测下表的固定相对路径。排除系统明确标识的模板/公共配置目录；目录联接或符号链接做物理身份去重，不递归枚举账户根下的任意子树。
4. 不读取其他账户的进程环境，也不加载其他账户注册表 hive。读取当前进程显式提供的 `CODEX_HOME`，追加其标准子目录，不替换其他已发现默认根。
5. 本轮不新增未经项目验证的 Claude 环境变量约定；非标准 Claude 日志通过手动追加解决。

| 应用 | 每个用户目录探测的日志根 | 关联元数据 |
| --- | --- | --- |
| Claude Code | `<profile>/.claude/projects` | 仅 `<profile>/.claude.json` 的既有项目映射 |
| Codex | `<profile>/.codex/sessions` | 无额外账户映射 |
| Codex | `<profile>/.codex/archived_sessions` | 无额外账户映射 |
| Codex 当前环境位置 | `<CODEX_HOME>/sessions`、`archived_sessions` | 来源标记“环境变量”；与标准位置合并去重 |

不存在的标准路径默认不生成空列表行；遇到权限拒绝时不能假称不存在，应显示“无法访问/未确认”。发现任务在后台执行，单个目录失败不阻塞其他来源。不得为了遍历其他账户自动请求管理员权限。

### 3.2 发现时机

- 仅在现有隐私同意闸门 Ready 后：启动时发现一次；设置页“重新发现”按钮显式重新发现。
- 定时采集只使用当前已解析来源集合，不随 5 秒等采集间隔重做账户枚举。
- 手动追加/移除/启停只更新来源解析，不隐式扫描所有账户；手动刷新用量沿用已发现集合。
- 新建账户或新装工具的目录通过“重新发现”或重启出现。发现中按钮显示加载态；完成时显示发现时间和部分失败提示。
- 单次采集冻结来源集合。配置变更发生在采集中时，当前批次完整结束，下一批使用新配置；不同时启动第二批采集。
- 测试和 acceptance 模式必须注入合成 profile 列表、环境与文件系统探测，覆盖全部真实发现入口，绝不回退本机用户目录或注册表。

## 4. 配置模型与迁移

建议 TOML 结构（字段名实施时按此固定，序列化测试覆盖）：

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
excluded_auto_paths = ["C:/Users/work/.codex/archived_sessions"]

[[sources.codex.additional_paths]]
path = "D:/history/codex/sessions"
enabled = true
```

- 新安装默认两个应用启用、自动发现开启、追加列表为空。应用总开关只控制采集，不排除历史库中的旧用量。
- 自动项停用存入 `excluded_auto_paths`，以规范化路径比较；重新发现、重启不能自动重新启用。关闭自动发现后可仅使用手动项。
- 自动结果本身不写入 settings；只持久化用户选择。运行期保留上一轮发现记录，以显示本轮消失项；重启后未再发现的项可以不显示，但停用清单保留。
- 旧 `dir` 非空：按原有“递归日志根”语义迁入一个追加项，保留 `enabled`；自动发现开启。旧应用总开关为 false 时仍禁止任何实际采集。
- 旧 `dir` 为空：迁为默认发现，无手动项。旧路径与自动项重合只显示/扫描一份，并标记“自动发现、手动添加”。
- 使用可区分“旧字段缺失”和“新数组为空”的反序列化模型；新配置已存在时以新配置为准，不能每次加载又从旧 `dir` 导入。
- 迁移通过现有 `settings::update` 原子事务保存，成功后不再输出旧字段；失败时不启动扩大范围的采集、不覆盖坏配置、不丢弃其他已支持的设置。连续迁移不新增重复项。
- 迁移后设置页显示一次说明：“原自定义目录已保留为追加目录；已启用自动发现”，提供自动发现开关，不要求再次手工填写目录。

## 5. 路径、事件与账户映射规则

### 5.1 路径身份和重复扫描

- 展示路径保留完整可复制文本；逻辑比较采用项目既有 Windows 规范化规则（分隔符、盘符、大小写、尾分隔符）。存在且可访问的目录再解析物理身份，用于联接/符号链接别名消重。
- 文件系统身份解析失败时不把不同目录冒认成同一目录；保留显式诊断，不因 canonicalize 失败将全轮判空。
- 同应用相同根合并 origins；同应用父子根保留配置行，采集分配为互斥子树：子根先认领，父根跳过所有更具体的已启用子根。列表注明覆盖关系，文件最多解析一次。
- 同一物理文件通过别名出现时按物理身份消重。不要先扫描两遍再仅靠事件库去重掩盖性能问题。
- 跨应用相同或嵌套根继续拒绝。手动保存时事务失败并明确冲突项；自动重新发现产生冲突时隔离冲突集合、显示错误，其他无冲突根继续采集，不执行“先到者获胜”。
- 同一采集批次对每个来源根冻结对应映射，不跨账户借用 Claude 项目表。手动日志根没有显式可信关联时沿用“映射禁用”的旧规则，不猜其所属账户。

### 5.2 历史事实与费用

- 不将用户目录、账户名、来源 ID 加进原生事件身份。Claude 原有 message 身份、Codex 原有保守重播指纹保持不变；不声称 Codex 获得了独立请求强身份。
- 同一日志复制到默认目录和追加目录，事件取并集；来源溯源仍可记录多个位置。不得改变 CCS 手动导入、日汇总或费用冻结口径。
- 源停用/删除配置后，历史用量仍可查询。设置文案统一为“停止后续读取”，修掉旧的“完全不参与统计”等误导语句。
- 不新增“按账户统计”或账户筛选；profile 信息仅用于来源解释和元数据关联。

## 6. 设置页与后端接口

每个应用一组：总开关、自动发现开关、“重新发现”按钮；其下“追加读取目录”输入/目录选择按钮与“添加”按钮；再下方是统一路径列表。

| 列 | 内容 |
| --- | --- |
| 目录 | 完整路径、支持换行复制，不以省略号遮住差异 |
| 来源 | 自动发现 / 环境变量 / 手动添加，可多标记 |
| 状态 | 待读取、可读取、缺失、无权限、部分失败、已停用、冲突、被子根覆盖；不能把“目录存在”写成“读取成功” |
| 操作 | 启停；手动项可移除，自动项不可删除；自动与手动重合时移除只移除手动来源 |

- 手动输入指向**日志根**，不是工作项目 cwd。提示 Claude 通常选择 `projects`，Codex 选择 `sessions` 或 `archived_sessions`，两者可分别添加；旧自定义任意根仍保持原递归语义。
- 保存前拒绝空值、相对路径、明显整盘/整个用户总目录及跨应用冲突；不存在的绝对日志路径允许保存，列表标为缺失。输入不会被静默改写成另一个目录。
- 不引入任意 glob 输入。目录选择可用系统选择器，取消不保存；前端草稿不直接决定后端扫描范围。
- 添加重复项返回已有行并提示“目录已在列表中”，不产生新行。失败保留输入，成功清空；保存请求期间防重复点击。
- 增加 `source_paths_list`、`source_paths_discover`、`source_path_add`、`source_path_remove`、`source_path_set_enabled` 业务 IPC；应用开关和自动发现开关走既有 settings 原子更新。
- 解析结果建议为 `ResolvedSourceRoot { id, agent, path, canonical_identity, origins, mapping_path, enabled, diagnostic }`；来源 ID 是配置/展示身份，绝不当请求事实身份。
- 每个写接口在后端对最新 settings 做单项事务操作，禁止前端整组旧快照覆盖。以来源修订号丢弃迟到的发现/状态响应；配置修改后失效最近采集键，串行请求新一轮采集，沿用 usage-updated 通知与汇总/明细冻结协议。
- 单应用视图只显示对应应用来源警告；全局配置/价格警告保留。重复无变化的权限错误不在每次采集刷 info。
- `docs/privacy.md` 明确读取范围扩展为本机可访问账户的标准日志目录；继续遵守现有同意闸门，不新增本计划外的强制重复同意流程。

## 7. 实施任务（每项先红后绿，再提交）

各任务都按：补失败测试 → 运行确认失败原因 → 最小实现 → 对应测试通过 → 必需门禁 → 独立提交。不得以改测试门槛来绕过重复计量或真实目录隔离。

### S01 配置集合与迁移

- 修改：`src/settings.rs`；新增：`tests/source_paths_settings.rs`。
- 测试旧空值/非空值/停用、新数组为空、新旧并存、重复加载、保存失败及两个并发单项更新不互相覆盖。
- 命令：`cargo test --test source_paths_settings`。通过标准：追加项只迁移一次，失败不扩大采集范围，其他设置逐字段保留。
- 提交：`feat(来源): 支持追加目录配置及旧覆盖目录迁移`。

### S02 可注入的多用户发现器

- 新增：`src/source/discovery.rs`、`tests/source_discovery.rs`；修改：`src/source/mod.rs`、`src/acceptance.rs`。
- 生产 profile 枚举和文件系统探测封装为可替换提供者；调用方不散落注册表或环境读取。
- 测试两个用户、非 C 盘、当前环境根、Codex 双根、权限拒绝、无标准目录、模板目录、别名、发现部分失败；用计数提供者证明定时采集不重复枚举。
- 命令：`cargo test --test source_discovery`。测试一律合成目录，提供者未注入时测试模式拒绝真实发现。
- 提交：`feat(来源): 自动发现多用户标准日志目录`。

### S03 多根解析、采集与历史并集

- 修改：`src/report.rs`、`src/source/claude.rs`、`src/source/codex.rs`、`src/source/claude_projects.rs`；新增：`tests/source_paths_collection.rs`。
- 根集合排序及指纹稳定；同根/别名/嵌套只解析一次；多用户 Claude 各自绑定 `.claude.json`；冲突隔离与错误来源归属明确。
- 测试复制同一事件、迁移前后同一事件、父子根、别名、独立真实事件不误合并、缺 cwd 的两个账户同 slug 不串映射、单根失败其他根仍入库、停用后历史保留、映射变更令采集缓存失效。
- 命令：`cargo test --test source_paths_collection` 与 `cargo test --test ui_adjustment_collect`。冻结查询仍一致，不改既有 Codex 身份语义。
- 提交：`feat(采集): 汇合多根来源并防止重复扫描`。

### S04 后端事务接口与调度

- 修改：`src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`（命令注册及后台读取）、`src-tauri/src/privacy.rs`（闸门测试范围）。
- 测试未同意时不能发现或读取真实目录；重新发现/追加/移除与定时读取交错、旧修订响应迟到、应用关闭后不触发采集、配置成功后正确失效缓存。
- 命令：`cargo test --manifest-path src-tauri/Cargo.toml`。同一时刻最多一个采集批次，所有路径状态与实际解析集合一致。
- 提交：`feat(设置): 接入多目录事务和来源发现命令`。

### S05 设置页列表与行为测试

- 修改：`frontend/src/types.ts`、`frontend/src/views/Settings.vue`、`Settings.test.ts`、`frontend/src/composables/useUsageQuery.ts` 及对应测试。
- 测试自动与手动标签、添加不覆盖、重复项、移除只删手动来源、自动项停用持久化、读取失败保留草稿、并发操作不回滚新状态、完整路径可访问名称及长路径换行。
- 命令：`pnpm --dir frontend test`、`pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`。
- 提交：`feat(设置): 用来源列表管理自动与追加目录`。

### S06 浏览器与原生隔离验收

- 修改：`frontend/scripts/fixtures/ui-contracts.mjs`、`frontend/scripts/contracts/interaction-contracts.mjs`、`frontend/scripts/run-ui-refresh-e2e.mjs`、`frontend/scripts/native-acceptance.mjs`；新增来源列表具名契约与必要的隔离 fixture。
- 浏览器：明暗主题、1280×820 / 980×620；新增/重复/停用/移除/权限失败/迟到响应；不得只断言文件存在或空列表。
- 原生：注入两个合成 profile、一个追加根；预期事件清单逐请求核对；复制同一日志后请求数/token 不变；新增独立事件后增加；停用再重启历史仍在；Codex archive 与 Claude 子代理均纳入。
- 真机与 Actions 均禁止枚举真实 Profiles；用户目录指纹不变。复用安装/升级 manifest 校验及现有验收隔离入口，不能绕过闸门。
- 命令：`pnpm --dir frontend build`、`pnpm --dir frontend test:e2e`；原生用新增具名场景接入现有 Native UI E2E workflow，分别记录浏览器和原生结果。
- 提交：`test(来源): 覆盖多用户发现与追加目录生命周期`。

### S07 文档、版本与交付

- 修改：`CLAUDE.md`、`DESIGN.md`、`docs/stats-semantics.md`、`docs/privacy.md`、本计划、`docs/plans/README.md`；检查隐私页面是否有对应测试需同步。
- 全量门禁：W01–W05 完成后统一 workspace fmt、clippy `-D warnings`、test；前端 typecheck、format:check、test、build、test:e2e；acceptance 与安装升级契约按 CI 同口径运行。
- 用户确认实施后才在功能分支执行 `python -B scripts/bump_version.py --bump minor --summary "多用户日志自动发现与追加目录管理"`，随后 `--check --base-ref origin/main`。
- PR 的 CI、安装/升级、Native UI E2E 必须针对同一 HEAD 成功；真实跨显示器等既有待办不被此计划顺带勾选。
- 提交：`docs(来源): 同步多目录统计语义与验收记录`。

## 8. 验收与审阅清单

- [ ] 用户批准本设计，才开始 S01–S07。
- [ ] 默认可发现两个合成用户的标准日志根，且未读取测试边界外的任何目录。
- [ ] 自动发现和手动追加同时生效；旧配置迁移不丢目录、不重复迁移。
- [ ] 来源列表、采集集合、诊断归属一致；权限失败不拖垮其他根。
- [ ] 同文件不重复解析、同事件不重复入库、独立事件不丢失；账户映射不串用。
- [ ] 移除/停用后历史仍可查，重复刷新/重启/导入不增加既有用量。
- [ ] 发现只在启动/显式重新发现发生，短间隔采集不做全账户扫描。
- [ ] 两种主题、窄窗口、键盘操作、全部回归及当前 HEAD 远程门禁通过。

## 9. 同分支的独立维护提交

用户要求将主工作区已有的 `frontend/scripts/native-acceptance.mjs` 未提交改动带到本分支，**单独 commit**：输出路径绝对化、验收日志目录使用显式递归拷贝。该提交是已有验收脚本维护，不代表本计划任何功能已实施；本计划文档另行提交。原工作区文件保留，避免覆盖其他工作。

## 10. 追加：单一 Cargo Workspace 与旧 CLI 构建补丁清理

> 2026-10-15 用户要求：在 `docs/source-discovery-plan` 分支、同一计划内设计全部迁移步骤，不另开分支。W01–W07 仅规划，尚未实施。建议先完成 W01–W06，再执行 S01–S06；S07 与 W07 合并交付。若来源功能已开工，先记录实际基线，禁止覆盖其他工作。

### 10.1 已核实的现状与目标

- 根 crate `tokenscope` 是库，根 `src/main.rs` 已不存在；桌面 package 为 `tokenscope-app`，唯一应用二进制为 `tokenscope.exe`。保留二进制名称和应用标识，不影响安装升级。
- 根 manifest 仍以旧 CLI 同名为理由 `exclude = ["src-tauri"]`，壳 manifest 仍声明独立 `[workspace]`，相关注释已过时。
- 根锁文件中的全部包身份（name/version/source）均已存在于当前壳锁文件；实施时须重新核实，以最新内容为准。可优先使用包含完整依赖图的壳锁文件作为统一锁候选，再由 Cargo 验证，不能手工拼接锁文件。
- 目标为一个 workspace、一个根 `Cargo.lock`、默认一个根 `target/`；同一依赖在相同构建条件下复用。不同 profile、features、目标平台以及 host/build 依赖仍可能分别构建，不承诺全部消除重复编译或节省固定比例空间。
- “两个 target 共约 76GB”尚未实测。清理前记录真实大小、路径和链接关系，区分实际目录、junction 与共享 worktree 缓存，不能把逻辑路径大小直接相加。

### 10.2 目标配置与不变量

根目录继续保留 `[package]`，不搬迁源码、不改包名，添加：

```toml
[workspace]
members = [".", "src-tauri"]
default-members = [".", "src-tauri"]
resolver = "3"
```

删除根 exclude 和壳独立 workspace。`default-members` 明确根目录普通 build/test 覆盖两个 package；只查核心库使用 `cargo test -p tokenscope`，只构建 GUI 使用 `cargo build -p tokenscope-app --bin tokenscope`。CI 仍显式使用 `--workspace`，不依赖工作目录的默认选择。

把共同使用的 `anyhow`、`serde`、`serde_json`、`log`、`jiff` 等依赖声明集中到 `[workspace.dependencies]`，成员以 `workspace = true` 继承。版本约束选择满足现有两侧要求且不改变锁定版本的交集；各成员需要的 features 继续显式保留。根库 dev-dependency 与普通 dependency 的共享项也检查一致性。仅壳使用的 Tauri/plugin 依赖留在壳中，不为追求统一机械上移全部依赖。

本轮保留两包各自显式 package version，以兼容既有版本工具与历史提交读取；不顺带引入 `workspace.package` 版本继承。统一锁必须恰好包含两个无 source 的本地包、版本与四份应用配置一致。第三方版本、checksum、source 不应漂移；必要差异必须逐项解释，不运行无范围的 `cargo update`。

生产构建仍默认不含 acceptance；`tokenscope-app/acceptance` 继续透传根库 feature。验证全 workspace acceptance 覆盖两个 package，正常发布构建显式不带验收 feature，不能用验收包作为发布包。

### W01：基线、失败契约与路径清单

1. 读取当前 Git 状态、锁文件、Cargo/Tauri 配置、环境 `CARGO_TARGET_DIR`、用户/项目 Cargo target-dir 配置；仅检查相关键，不输出无关凭证。记录当前 metadata 的 workspace_members/default_members/target_directory 与依赖版本集合。
2. 全仓搜索 `src-tauri/target`、反斜杠形式、`path.join(..., "src-tauri", "target", ...)`、双 Cargo.lock、独立 workspace、旧 CLI 同名等引用；区分运行入口与历史证据，历史报告不伪改成新路径。
3. 更新 `scripts/test_bump_version.py` 的合成仓库 fixture：一个根锁包含两个本地包，删掉子锁 fixture。先运行并记录旧工具因要求第二锁而失败。
4. 补充有意义的失败用例：统一锁缺壳包、任一本地包版本错误/重复、第三方同名带 source 的包不误改；保留原子回滚、重复 bump 幂等、历史 main 基线读取测试。
5. 检查现有工作流/发布契约覆盖位置。为真实路径接线补测试，不用单纯断言某段字符串代替实际 metadata、构建与产物验证。

### W02：合并 workspace、依赖和锁文件

1. 修改 `Cargo.toml`、`src-tauri/Cargo.toml`，按 10.2 配置共享依赖，删除全部旧 CLI 同名规避注释。
2. 保存两份旧锁的比较基线，将完整壳锁内容作为根锁候选，删除受版本管理的 `src-tauri/Cargo.lock`；不删除 target。
3. 先运行 `cargo metadata --offline --locked --format-version 1` 和 `cargo check --offline --locked --workspace`。若 Cargo 要求刷新锁，单独解析并审阅差异后重跑 locked 检查；网络确有需要时遵守项目代理约定。
4. 核对 metadata 恰有两个成员、默认选择包含两者、target_directory 为预期根目录；使用 `cargo tree --workspace -d` 分析剩余重复版本，不强制消灭合法的多版本依赖。
5. 对比最终锁与迁移前完整壳锁的第三方包版本/source/checksum，解释任何差异。运行根库和壳定向测试，确认无循环依赖。

### W03：版本工具迁移为单锁契约

1. 修改 `scripts/bump_version.py`：LOCKS 仅根锁；`validate_locks` 不再按锁索引决定包集合，必须验证两个本地包。
2. 同步 `scripts/test_bump_version.py`：四配置＋一锁＋声明原子更新；测试命名不再声称六个版本文件。保留历史双工程提交作为 base-ref 的读取兼容，因为读取的是四配置而非历史锁数量。
3. 在临时 Git fixture 运行 bump/check/失败回滚，证明只改本地包版本、无子锁生成、第三方依赖不漂移。
4. 运行 `python -B -m unittest discover -s scripts -p "test_bump_version.py" -v` 与 `test_release_metadata.py`，要求全部通过。草稿阶段不执行产品版本递增。

### W04：产物路径与构建命令统一

逐项修改并验证：

| 文件 | 处理 |
| --- | --- |
| `frontend/scripts/native-acceptance.mjs` | EXE_DEFAULT 改根 `target/release/tokenscope.exe`；保留 c16e023 的输出路径及日志复制修复 |
| `.github/workflows/ui-e2e.yml` | 明确 GUI package、release、acceptance/custom-protocol；构建位置与脚本默认路径一致 |
| `.github/workflows/release.yml` | NSIS 与 exe 收集改根 `target/x86_64-pc-windows-msvc/release/...`，保留显式目标三元组 |
| `.github/workflows/install-test.yml` | 与 Release 使用同一产物路径规则，仍校验将发布的同一安装包 |
| `scripts/release_assets.py` 及测试 | 核对是否已有路径无关接口；无需改的实现不改，只同步确有旧路径的调用和 fixture |
| `.gitignore` | 保留所有层级 target 忽略，可规范为 `target/`；不缩成仅根 `/target` |

不要假设所有构建都带三元组：不带 `--target` 的原生验收路径与显式 target 的发布路径分别取证。若用户显式设置 CARGO_TARGET_DIR，支持现有 exe 参数覆盖，不静默忽略用户配置；标准 CI 验证无覆盖时根 target 行为。

### W05：去掉重复门禁和缓存映射

1. `.githooks/pre-commit` 合并为一次 workspace fmt/clippy/test，保留前端门禁；Cargo 检查使用 `--locked`，不漏掉壳测试。
2. `.github/actions/checks/action.yml` 保留普通、acceptance 两套 workspace clippy/test，删除重复壳轮次；fmt 只执行一次 `--all`。保留全部 Python、PowerShell、前端与 E2E 门禁。
3. `.github/actions/setup/action.yml` Rust cache 只配置 `. -> target`；迁移时增加缓存键版本以免恢复旧布局。保留各 job 区分，不能把不同构建场景视作同一完整缓存。
4. 检查 `ci.yml`、`install-test.yml`、`ui-e2e.yml` 的 paths 过滤是否覆盖改动入口，避免“改了构建配置却跳过验收”。
5. 本地验证命令：`cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --locked --workspace`；再分别以 `--features acceptance` 跑 clippy/test。
6. 验证根目录普通 `cargo test --locked` 确实选择两个默认成员；不再反复跑第二套独立 manifest 门禁。

### W06：构建与缓存隔离验收，最后处理旧 target

1. 在没有旧编译产物的隔离 checkout 或一次性 Actions Runner 上运行 metadata、普通/acceptance 门禁，证明结果不依赖旧 `src-tauri/target`。
2. 生产 `tauri build` 不带 acceptance，生成 NSIS；执行 release_assets 收集/哈希契约，确认实际 exe 与安装包路径均正确。
3. 另行构建 acceptance 包并跑 Native UI E2E，证明隔离根仍生效且不会读取/污染用户真实数据；生产与验收产物按不同 job/工件分开记录。
4. Actions 跑五项安装验收和两项固定旧版升级；本机禁止运行真实安装/卸载脚本。检查上传工件非空、当前 HEAD 与版本匹配，而不只看 job 成功。
5. 记录从根和 src-tauri 发起 Cargo/Tauri 命令后都未新建子 target/子锁。更换构建入口不能回到旧布局。
6. 新构建验证通过后再清理旧 target：先核实绝对路径处于仓库内、是否为链接、是否被运行中的 TokenScope 或其他构建/worktree 引用。当前运行实例可能来自旧 debug exe，不能直接删除其目录。
7. 不把两套 target 文件复制拼接；不为迁移先清空根 target。对可安全清理的旧目录执行单一 PowerShell 文件操作；仍被使用则登记待清理路径与原因，不强杀用户实例。清理后记录实际回收空间。

### W07：规约、版本与同分支交付

- 更新 `CLAUDE.md` 的 workspace、命令、提交钩子、单锁版本规约；检查 README、开发/发布说明中的当前构建步骤。历史审核证据中的旧产物路径保留并注明历史上下文，不重写旧成功记录。
- 本计划 S04/S07 和第 11 节统一遵循 workspace 门禁；`--manifest-path` 本身仍合法，不把正常包选择命令误当补丁清除。
- 目录发现、扫描反馈和构建整理分别记录完成状态与提交，均留在用户指定分支。未实施的计划不因构建整理完成而勾选。
- 本批准备合入 main 时 fetch 最新 main，按实际范围递增一次版本：若包含来源发现功能选 minor，若仅构建维护选 patch。使用迁移后的版本工具，不手改版本、不为同一个 PR 多次递增。
- 全部普通/acceptance 门禁、前端构建、版本/发布契约、本次 HEAD 的 CI/安装升级/Native UI E2E 通过后才报告合并就绪；不自行合并。

### 10.3 进度与完成标准

| 任务 | 状态 | 证据 |
| --- | --- | --- |
| W01 基线与失败契约 | 未开始（已做只读初步配置核查） | 尚无迁移测试 |
| W02 单 workspace 与锁 | 未开始 | — |
| W03 版本工具 | 未开始 | — |
| W04 路径接线 | 未开始 | — |
| W05 门禁与缓存 | 未开始 | — |
| W06 构建验收与旧目录清理 | 未开始 | — |
| W07 文档与交付 | 未开始 | — |

- [ ] metadata 两个成员、正确默认成员与统一 target_directory。
- [ ] 仅一个受版本管理的 Cargo.lock，两个本地包版本正确，第三方依赖无未解释漂移。
- [ ] 旧 CLI 同名规避配置和当前文档说明全部清除，共享依赖 features 保持正确。
- [ ] 钩子、CI、缓存、NSIS 收集与原生验收统一新布局；普通/acceptance 覆盖无损。
- [ ] 生产包、验收包、固定旧版升级均取得当前提交的实跑证据。
- [ ] 旧 target 已安全清理，或明确记录被占用/共享而暂缓；不把待清理写成已释放空间。

<a id="collection-feedback"></a>

## 11. 扫描状态反馈与自动刷新

本节取代 2026-10-13 UI 调整计划 U06 中常驻“更新于 HH:MM:SS”的要求；其余时间口径、分页保护和查询一致性继续有效。F01–F05 未开始，交互方向已确认，具体实施设计随整份计划审阅。

### 11.1 原因与范围

“每 5 秒自动读取”指上一轮采集结束后等待 5 秒，再扫描来源；不是每 5 秒必然产生新的 token 用量。现有状态时间来自统计查询的 `as_of`，不代表最近扫描时间。没有新事实时，后台不发 `usage-updated`，界面时间保持不变，用户无法据此判断采集是否正常工作。

2026-10-15 本机只读排查：源日志用量记录时间 00:16:01.265，对应入库 00:16:05.041；截图 00:16:27 显示“更新于 00:16:05”；下一笔源用量时间 00:16:46.565，入库 00:16:52.082。该片段没有证实 UI 漏刷。源记录时间不等同于操作系统实际落盘时间，现有 info 日志也不能证明无变化期间每一轮扫描执行时刻。原始会话内容、身份和路径不纳入文档或 fixture。

F 组任务只改扫描反馈，不改变用量解析、去重、价格、自动读取间隔或 CCS 手动导入规则。不改成定时强刷统计，不新增常驻时间字段，不增加每轮 info 日志。

### 11.2 界面契约

| 实际状态 | 状态区 | 统计数据 |
| --- | --- | --- |
| 初始状态尚未获取 | 正在确认状态，静态 | 保留已有视图，不假称扫描中 |
| 等待下轮，已有有效统计，无错误 | 静态绿点“已更新” | 不变 |
| 后台实际开始采集 | 旋转图标“正在读取” | 保留已有视图 |
| 采集结束，无新用量 | 立即停止读取动画，恢复静态状态 | 不触发新查询 |
| 采集结束，有新用量 | 读取动画停止；统计查询在途时显示“刷新中…”及其加载状态 | 汇总与明细同批成功后一起替换 |
| 有新用量，但明细已加载更多 | 静态“有新数据 · 刷新” | 保留已加载页，用户点击后刷新 |
| 采集失败 | 动画停止，“读取失败”，提供可操作错误提示 | 保留旧统计；不能显示正常“已更新” |
| 采集成功但存在部分来源/坏行问题 | 动画停止，沿用采集诊断提示，状态体现部分异常 | 有效用量仍可更新 |
| 查询失败 | 查询加载停止，“刷新失败 · 显示上次数据” | 不覆盖旧统计 |

状态优先规则：活动阶段显示当前工作；错误持续保留在独立提示中，不能被另一路成功清除。后台下一次成功只能清除采集错误，成功查询才能清除查询错误。没有首批有效统计时不能显示“已更新”。

悬停说明：“每 5 秒自动读取；无新用量时保持统计不变”（间隔使用已保存的有效配置）。取消常驻具体更新时间；内部 `as_of`、快照时间及日志不删除。设置页帮助文案同步解释自动保存、扫描频率及有变化才刷新。

动画只在真实活动阶段播放，不用读取间隔定时模拟，不设置最短播放时长；快速扫描允许动画短暂出现。减少动态效果偏好下采用静态读取图标及文字。图标不影响布局、不抢焦点；避免屏幕阅读器每 5 秒朗读扫描开始/结束，错误和用户操作结果才使用适当通知。

窗口隐藏时不执行统计刷新、不播放动画；后端照常采集。恢复时查询当前状态，按代次补齐变化，并保留分页保护；不得补播隐藏期间已经完成的扫描。

### 11.3 后端状态与事件

#### 3.1 真实边界与单飞

修改 `src/report.rs` 的 `collect_flighted_with`：仅领队取得 `COLLECT_SERIAL` 串行锁后进入 running，结束采集工作后进入终态；单飞跟随者不创建轮次，等待串行锁不算正在读取。启动、手动刷新、定时采集共用这一边界，避免定时入口动画遗漏手动采集或把排队时间算成扫描。

根库提供可注入的状态观察接口及内存快照，不依赖 Tauri。状态更新后释放状态锁，再通知观察者；观察者失败不能改变采集结果。终态覆盖成功、返回错误与 unwind，RAII 收尾不能在 panic 时伪报成功；进程直接终止不承诺发结束事件。无业务权限时跳过定时轮次，不制造成功扫描。退出中不启动新轮次。

建议统一状态 DTO：

```typescript
type CollectionStatus = {
  revision: number; // 进程内每次状态转换单调递增，IPC 限安全整数
  runId: number; // 仅真正执行的领队递增
  phase: "idle" | "running" | "succeeded" | "failed";
  trigger: "startup" | "manual" | "auto" | null;
  latestGeneration: number | null; // 最近成功采集的历史代次，失败不清空
  changed: boolean; // 本轮是否有事实变动，不以文件 mtime 代替
  hasWarnings: boolean;
  error: string | null; // 精简可展示错误，完整链仅写现有日志
};
```

初始 idle 不意味着统计已经更新。保存最近成功代次，使最后一轮无变化时仍能修复前端漏掉更早通知的情况。普通状态不带文件路径、模型列表或用量事实；诊断内容沿用既有查询渠道。

#### 3.2 Tauri 接线与恢复

`src-tauri/src/lib.rs` 在业务 Ready 后注册一次观察者并发送 `collection-status` 事件；状态终态发出不等于统计查询已经完成。保留 `usage-updated`，不重复增加第二条无条件刷新链路。

`src-tauri/src/commands.rs` 新增只读 `collection_status`，仅返回内存快照；在命令注册和 `src-tauri/src/privacy.rs` 中接入 Ready 权限检查。查询状态不能扫描目录、打开历史库或导入 CCS。

前端先订阅，再调用状态查询；只接收较新 revision，避免查询晚到覆盖刚收到的事件。监听失败不得静默显示正常状态，显示“读取状态不可用”，允许重试。窗口恢复、组件重新挂载时重查；可见期间每 30 秒仅查询一次内存状态以纠正漏失结束事件，失败时停止假定动画并提示状态不可用，不把超时假判为采集失败。隐藏时停止该校验计时器，卸载时清理订阅和计时器。

状态恢复得到比当前统计快照更高的 latestGeneration 时走现有自动刷新合并入口；与 `usage-updated` 交叉到达时，同一已覆盖代次不再重复建查询。查询在途、分页、隐藏保护继续有效。状态 revision 和历史 generation 各自独立，不能混用。

### 11.4 实施步骤（先红后绿）

每项先添加能失败的定向用例，保存失败断言，再最小实现、运行相关测试并单独提交。执行时使用 executing-plans 技能；本设计未批准前不实施。

#### F01：根库真实采集生命周期

- 修改 `src/report.rs`；必要时将状态类型单独放入 `src/collection_status.rs` 并在 `src/lib.rs` 导出。
- 扩展 `tests/ui_adjustment_collect.rs` 的串行隔离测试：真实领队 start/end 一对、无变化也有 end、有变化代次、失败/unwind 收尾、跟随者不重复、排队不提前 start。
- 观察接口使用隔离实例；若必须接进程单例，所有相关测试共享已有锁，不新增并行全局状态污染。
- 运行 `cargo test --locked --test ui_adjustment_collect`，预期全部通过且无用户目录读写。

#### F02：宿主事件与当前状态查询

- 修改 `src-tauri/src/lib.rs`、`src-tauri/src/commands.rs`、`src-tauri/src/privacy.rs`。
- 测试 Ready 前拒绝、状态查询不触发采集、观察者只注册一次、发事件失败不影响用量提交、失败后下一轮成功恢复。
- 运行 `cargo test --locked --manifest-path src-tauri/Cargo.toml`。

#### F03：前端状态与展示

- 新增 `frontend/src/composables/collectionStatus.ts` 及对应测试，集中订阅、版本判定、重同步及卸载清理。
- 修改 `frontend/src/composables/useUsageQuery.ts` 和测试、`frontend/src/views/Dashboard.vue`、`frontend/src/views/Settings.vue`；样式遵循 `DESIGN.md`，实施时明确实际共享样式落点。
- 测试 start→无变化 end 不发 query_begin、不改统计；start→有变化仅一个刷新批次；读取结束且查询未完仍显示刷新中；两类错误互不覆盖；事件乱序、订阅与查询竞态、漏通知恢复、分页保护、窗口隐藏恢复、计时器清理。
- 测试状态文案与 tooltip 无常驻时间、动态效果偏好、无首批结果时不得显示已更新。
- 运行 `pnpm --dir frontend test`、`typecheck`、`format:check`、`build`。

#### F04：端到端验收

- 扩展 `frontend/scripts/native-acceptance.mjs` 的隔离刷新验收及现有浏览器 E2E；只用合成 JSONL、隔离数据根，不改真实日志或数据库。
- 无变化：观察真实 start/end，动画停下，统计和查询身份不变；至少两轮，不能只模拟前端事件证明后台接线。
- 有变化：追加一条合法合成用量，断言 start/end、代次增加、统计增加一次、汇总与明细同 query_id；不手动点击刷新。
- 短扫描：状态生命周期可完整取证，不要求截图必然抓到瞬时动画；用受控慢采集验证动画实际可见及结束后停止，延迟注入不得进入生产构建。
- 失败恢复、运行时切换 5 秒间隔、分页后新数据提示、隐藏恢复分别验收。采集 revision/runId/generation、查询身份及断言结果进入工件，不保存真实会话内容。
- 现有 mjs 维护提交 c16e023 是本计划前置已有改动，不记作 F04 已完成。

#### F05：文档、门禁与交付

- 更新 `DESIGN.md` 状态反馈规范、`docs/stats-semantics.md` 扫描与查询时间区别及相关设置说明；旧 UI 计划标注被本计划替代的要求，不回写旧验收结果。
- 完成统一 workspace fmt/clippy/test、前端全门禁和相关 E2E。保留定向失败→通过证据；当前 HEAD 的远程 CI、安装和 Native UI 检查另行取证。
- 本次设计草稿不分配版本；实施准备合入 main 时按批次执行版本工具，若与目录发现功能同批发布取 minor，同一个 PR 不重复加号。未经授权不合并。

### 11.5 任务状态

| 任务 | 状态 | 完成证据 |
| --- | --- | --- |
| F01 生命周期 | 未开始 | — |
| F02 宿主事件与查询 | 未开始 | — |
| F03 状态与动画 | 未开始 | — |
| F04 E2E | 未开始 | — |
| F05 文档与交付 | 未开始 | — |

来源发现、扫描反馈与构建整理分别验收，统一交付。真实跨午夜、长时间休眠和跨显示器继续留在独立系统验收 todo，不因本计划关闭或重新引入本轮。
