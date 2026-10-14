# 多用户日志目录自动发现与追加路径管理实施计划

> 状态：**设计草案，待用户审阅；本轮不实施。**
> 执行时使用 executing-plans 技能逐项落实；用户明确要求优先于本计划。分支：`docs/source-discovery-plan`，基线为 main `e34495b`（0.3.1）。

**Goal：** 默认发现本机各用户可访问的 Claude Code / Codex 标准日志目录，设置页输入用于追加路径，以列表管理全部来源，重复目录和重复日志不重复计量。

**Architecture：** 增加独立目录发现与解析层，将自动发现、手动追加、停用规则解析成带身份及诊断的来源集合。采集、状态展示、查询缓存使用同一份不可变解析结果；历史库的事件身份和保留语义不变。

**Tech Stack：** Rust、Windows 用户配置文件发现、Tauri 2 IPC、Vue 3 / Naive UI、TOML、现有 SQLite 历史库、Vitest / Playwright 合成验收。

## 1. 已确认需求与范围

- 默认发现 `C:\Users\*` 所代表的本机用户目录下的标准日志位置；不递归搜索整个用户目录，更不扫描整盘。
- 当前单目录输入改为“追加读取目录”，添加不会替换默认来源，全部路径以列表显示。
- 无论主代理、子代理、归档会话，只要是已有解析器支持的有效用量均纳入。
- 保留统一 `~/.tokenscope/history.db`，不按账户或来源目录拆库，不保存完整会话正文。
- 停用、删除配置或源日志消失只影响后续采集，已经保存的历史用量不删除。
- 本轮只覆盖 Windows 多用户发现。其他平台继续发现当前用户的标准目录，手动追加多目录可用；不顺带开发 WSL、容器、SSH 或远程共享的自动发现。
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
- 全量门禁：根库/壳 fmt、clippy `-D warnings`、test；前端 typecheck、format:check、test、build、test:e2e；acceptance 与安装升级契约按 CI 同口径运行。
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
