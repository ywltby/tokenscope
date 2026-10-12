# 整合计划修复复核与 main 合并可行性

> 最新结论（2026-10-12，已实跑运行时基线 `fa911dd`）：**PR #4 两套检查全部成功，五个必需安装场景、六次严格 manifest 与成功工件均已核验，main 尚未合并。** 详见[Actions 成功取证](#pr4-fa911dd-green)。后续同步文档并修正一处 cfg(test) 并行断言，运行时不变；正式合并核对最新 HEAD 两套检查。N12 安装部分已验，整体仍部分完成，固定旧包/系统签字及 N10/N11/N13 继续待验。

## `16beea1` 第四轮历史复核

- 日期：2026-10-11。
- 被审实现：`16beea1cc94c24892cc3b36b2de3994be6a56db9`，`fix(历史库): 闭合第三轮复核 T01/T02 两项缺口`。
- 依据：[唯一整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)及用户定稿；上一轮为[6688466 第三轮复核](2026-10-11-consolidated-third-recheck.md)。
- **结论：T01、T02 均已闭合，N02 完成；当前暂不建议合入 main。** 普通门禁通过，但 CI 必跑的根库 acceptance 测试确定性失败，见 M01。这是旧数据库产物断言未随历史库迁移更新，未证明产品历史库或路径隔离失效。
- 当前责任状态：**8 项完成、1 项部分完成、4 项本轮验收未执行，共 5 项待办。** N01–N08 完成，N09 只剩本轮 M01 门禁阻断；N10–N13 维持原有验收责任与排期。

## 上轮修复核验

| 上轮问题 | 本轮结论 | 独立验证 |
| --- | --- | --- |
| T01：批内来源索引漏 origin_kind | 已修复 | resolve/register 共用包含种类与键的来源索引。不同种类同键的新写入：预览两条 Insert，实际插入 2、更新 0、最终 2 事实；既有事实补证据：预览两条 Unchanged，实际 unchanged=2、origins_added=2、最终 2 事实 |
| T02：直接构造测试选项漏 pricing_path | 已修复 | 上轮所列 9 处构造/fixture 均显式指定临时缺失价格文件；纯时间过滤和隔离验收的默认路径用例按原口径豁免。隔离默认价格有 gpt-5.6-sol 时，修复选项的 8 行全部未知；省略 pricing_path 的对照组选项仍使 2 行有价 |

进一步复用当前 `test_list_events_filters` 的原函数体，在仓库外用 acceptance 根内的合成默认价格运行，原筛选、排序、项目/日期、分页与全部未知断言通过。测试专用明细 shim 通过公开 query API 调用相同查询管线，原选项与断言未改；未编辑用户真实价格文件。

原五个存储反例继续通过：实际 CCS 两行重导入的预览/提交均 inserted=0、updated=1、unchanged=0、stale=1，最终一事实；同事实多别名两条 Update；Unchanged 补别名两条 Unchanged；同种类来源改绑两侧均拒绝；未持久替代事件键两侧均 Unchanged/Insert，最终两事实。日期缩窄减少保留量、完整候选优先于更贵的部分候选两项也继续通过。共 11 项仓外存储/口径/价格验证通过；来源反例仍限定公共存储 API，不扩大为已证明所有 GUI 导入场景。

## M01 / P1 / N09：acceptance 仍断言生成旧 cache.db，阻断 CI 与发布 gate

位置：[tests/native_acceptance.rs](../../tests/native_acceptance.rs) 第 363–365 行。`native_acceptance_paths_are_hermetic` 在隔离采集结束后要求 `tokenscope/cache.db` 存在，报错“缓存必须落在隔离根内”。同文件第 143、343 行的路径检查仍只列旧 cache.db，没有列当前 history.db。

当前运行时使用 history.db；旧 cache.db 仅是可选的只读迁移来源。新建隔离根没有旧缓存，正常采集不会创建它。以下实际仓库命令稳定失败，退出码 101；普通构建中的该测试提前跳过，所以普通门禁不会发现这处问题。

```powershell
cargo test --offline --locked --workspace --features acceptance
cargo test --offline --locked --features acceptance --test native_acceptance -- --nocapture --test-threads=1
```

定位验证：直接运行实际 acceptance 测试二进制的 `acceptance_child_hermetic_paths` 子进程，隔离根全新、只含合成/空来源。子进程退出 0，输出 `ACCEPTANCE_HERMETIC_OK`。实际产物为 history.db、history.db-wal、history.db-shm、settings.toml、pricing-index.json 和日志；没有 cache.db。只读打开这份临时 history.db，可见 usage_events、event_aliases、event_origins、source_files、ccs_daily_usage、import_runs、history_meta 七张表。仓外测试副本仅把默认数据库路径及文件断言更新为 history.db，三个父测试均通过、两个子进程阶段仍按设计忽略。**这些诊断副本不代表仓库代码已经修复。**

影响：[checks action](../../.github/actions/checks/action.yml) 第 48–50 行明确运行根库 acceptance test；CI 与 Release gate 都调用它。当前断言会使 PR/main CI 失败，并使 Release 的后续 NSIS 构建和发布因 gate 失败而跳过。

闭合方式：更新这个验收用例的默认数据库路径检查与实际产物断言，核对有效 SQLite 历史库，保留现有用户目录指纹与来源隔离检查；随后跑两套 manifest 的普通/acceptance 门禁。该任务并入 N09，不另建与当前整合计划竞争的计划；N10 的强隐私取证仍独立保留。

## 本轮门禁

| 检查 | 实际结果 |
| --- | --- |
| 根库普通 fmt / Clippy / test | 通过；456 pass / 10 ignored |
| 壳普通 fmt / Clippy / test | 通过；42 pass |
| 前端 typecheck / format:check / Vitest / build | 通过；33 文件、361 pass |
| 根库 acceptance Clippy | 通过，`-D warnings` |
| 根库 acceptance test | 失败；首次全量在 native_acceptance 中止；`--no-fail-fast` 跟进为 455 pass / 1 failed / 10 ignored，唯一失败为 M01 |
| 壳 acceptance Clippy / test | 通过；47 pass |
| Release metadata 单元测试 | 7 pass |
| 仓外独立存储/口径/价格验证 | 11 pass；当前原函数体价格隔离验证包含在内 |
| 实际验收子进程 / 仅更新断言的仓外副本 | 子进程退出 0；副本 3 pass / 2 ignored，用于确认根因 |

根/壳 Cargo 均离线，普通与 acceptance 全量使用默认并行。所有探针写入临时历史/价格/验收目录；验收用例自身仅读取用户数据目录的文件元信息以比较前后指纹。未运行真实日志忽略测试、当前完整浏览器矩阵、原生 GUI 统一交互、安装/系统或真实历史性能验收；未触发 CCS 自动导入或发布。

## 分支与合并条件

本轮成功 fetch origin 后，远端 main 为 `8d1d90de29aa00d10b9a7e4b8f906d909726446b`，是被审 HEAD 的祖先；`origin/main...HEAD` 为 0 个 main 独有提交、50 个分支独有提交。Git 可以快进，当前不存在需要解决的分支冲突。审查针对这次修复及合并门禁，未把 50 个提交、199 个变化文件逐行重新审计。

远端功能分支仍为 `b3e5d166339081f170239c880030022704340fb8`，本地领先 3 个修复提交。GitHub 只读查询未发现该 head 的打开 PR；当前未推送实现不能声称已通过对应远端 CI。修复 M01 后应推送当前功能分支，再通过实际 PR CI 确认合并版本。

main push 会触发 [Release workflow](../../.github/workflows/release.yml) 并在 gate 成功后生成预发布包。N10 隐私强取证、N11 当前原生交互、N12 安装/系统/发布签字、N13 真实历史性能仍未闭合；合并门禁通过也不能将它们写成当前正式版本验收完成。

本轮只更新复核与任务状态文档。因必选门禁失败，按 CLAUDE.md 的默认提交流程例外不提交、不推送，不执行 main 合并；并发研究文件和既有暂存区保持原状。

<a id="m01-closed"></a>

## `a58684c` 的 M01 修复独立复核

- 日期：2026-10-11。
- 被审实现：`a58684c7a1fdf06a154a4c569ab7050e8039d9fd`，`fix(测试): acceptance 断言更新为 history.db 并核对有效历史库`。
- 结论：M01 已修复，N09 闭合；当前 13 个责任编号为 **9 完成、0 部分完成、4 本轮验收未执行**。本地合并门禁全绿，尚未取得对应远端 PR CI 的通过结果。

逐项核对提交差异：子进程使用 `history_file_path(&Default::default())` 并检查路径落在隔离根内；父进程要求 `history.db=` 量测标签与实际 history.db 产物；只读打开数据库，并实际查询七张表。用户数据目录前后指纹与来源隔离检查保留。本提交只修改测试及执行账，没有变更产品采集、存储或计价代码。N09 原新增的“临时目录纳秒化”归因已删除：该写法在修复前已存在，不属于 a58684c 的改动。

在当前仓库直接执行定向命令，实际结果为 **3 pass / 2 ignored，退出 0**，不再使用仓外诊断副本证明修复。两个被忽略的入口是父测试实际调用的子进程阶段。

```powershell
cargo test --offline --locked --features acceptance --test native_acceptance -- --nocapture --test-threads=1
```

另按 [checks action](../../.github/actions/checks/action.yml) 逐项运行十五项本地检查，所有退出码为 0：

| 检查 | 实际结果 |
| --- | --- |
| 根库普通 fmt / Clippy / workspace test | 通过；456 pass / 10 ignored |
| 壳普通 fmt / Clippy / test | 通过；42 pass |
| 前端 typecheck / format:check / Vitest / build | 通过；33 文件、361 pass |
| 根库 acceptance Clippy / workspace test | 通过；456 pass / 10 ignored |
| 壳 acceptance Clippy / test | 通过；47 pass |
| Release metadata 单元测试 | 7 pass |

Cargo 全部使用锁文件约束，Clippy 均有 `-D warnings`，测试使用默认并行；额外 `--offline` 仅避免更新本机依赖。前述第四轮的 11 项独立探针、百万规模及可控退出验证沿用原证据，本轮没有重复执行。未运行真实数据忽略测试、隐私强取证、原生统一交互、安装/系统或真实历史性能验收。

fetch 后，远端功能分支与被审实现均为 a58684c，main 为 `8d1d90de29aa00d10b9a7e4b8f906d909726446b`，`origin/main...a58684c` 为 **0 behind / 51 ahead**，可以快进。GitHub 只读查询没有该功能分支的打开 PR，也没有 a58684c 的 PR workflow run；[CI](../../.github/workflows/ci.yml) 由 PR 或 main push 触发，功能分支 push 不触发该 workflow，所以当前不能宣称远端 CI 已通过。本轮审查修复及门禁，没有重新逐行审计全部 51 个提交。

执行者答复有两处需要更正：a58684c 只更新了 N09 表行和执行账，总账顶部、N09 详细验收、审计索引仍写旧失败状态，本批文档已同步；本机没有 gh 并不意味着只能由用户在网页创建 PR，当前 GitHub 连接器也提供创建 PR 接口，调用是否成功还须实际验证。本轮仅复核，没有创建 PR、合并 main 或触发发布。

被审 a58684c 的 main Release workflow 会按 gate/build/publish 依赖生成预发布包；这不等于 N12 安装验收通过。a58684c 未包含 Actions 安装测试；准备提交复核文档期间，另一个安装批次提交并推送了 b16437c，同时带入整合表的 M01 状态同步。新增部分补充核对如下，不将原 M01 复核结果自动推广到后来提交。

<a id="install-wiring-blockers"></a>

## `b16437c` 安装门禁补充核对：M02–M04

被审提交：`b16437c51bdef7ea1bddec955dafe41d6c27b59f`。只增加 Release workflow、三份安装辅助脚本和执行账，没有修改产品源码。当前状态为 **9 完成、1 部分完成、3 本轮验收未执行，共 4 项待办**；新增安装门禁属于 N12，不反向重开 N09。

### M02 / P1 / N12：install-test 不能读取间接依赖 gate 的输出

位置：[release.yml](../../.github/workflows/release.yml) 第 90、96、100、120 行。install-test 只有 `needs: build`，却三次读取 `needs.gate.outputs.sha/version`。GitHub [needs 上下文规则](https://docs.github.com/en/actions/reference/workflows-and-actions/contexts#needs-context)明确只包含直接依赖，不包含依赖的依赖；缺失属性解引用为空字符串。因此下载名变成 `release-`，与 build 上传的 `release-<sha>` 不匹配；AppVersion 也为空。publish 强依赖 install-test，发布会被阻断。此结论依据实际 YAML 与官方语义，未冒充远端运行结果。

本轮用真实 YAML 解析出 job 图，查出该 job 三处非直接依赖输出引用；其余已引用 gate 的 job 均有直接依赖。修复应直接声明 `needs: [gate, build]`，或由 build 明确转发并改用对应输出。

### M03 / P1 / N12：安装脚本调用路径含实际制表符

位置：[release.yml](../../.github/workflows/release.yml) 第 128 行。路径在 `scripts` 后是 U+0009 制表符，缺少预期的反斜杠与 `t`，不是合法的 `.\scripts\test_install.ps1`。对 YAML 解码后的 run 脚本执行 PowerShell AST 解析，命令元素实际为 `.\scripts`、`est_install.ps1`、`@args`；它会尝试调用 scripts 目录，无法执行安装脚本。语法解析无报错不能证明目标路径有效。修复为完整路径或 `Join-Path` 调用，再核对 AST 命令目标。

### M04 / P1 / N12：写入合成旧库前没有创建数据目录

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 84–99、166–181 行；[make_legacy_cache.py](../../scripts/make_legacy_cache.py) 第 73 行。Reset-UserHome 删除 `.tokenscope` 后，安装参数只有 `/S`，Write-SeedData 在第一次显式启动应用之前执行，代码没有创建 DataDir。当前配置没有自定义安装 hook；Tauri [默认 NSIS 模板](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi)的静默启动分支需要 `/R`，据此推断当前全新安装不会替脚本准备 `.tokenscope`。

直接用仓库原 make_legacy_cache.py 在临时目录下写入一个父目录不存在的 cache.db，退出 1，实际错误为 `sqlite3.OperationalError: unable to open database file`。固定旧包场景也先重置目录再写旧库，存在相同缺口。应在造数据之前明确创建 DataDir，再验证冷环境场景；本轮没有在用户机器运行安装或删除真实目录。

Release metadata 七项在 b16437c 再次通过，它们不验证 job 依赖、PowerShell 命令目标或数据目录准备。本轮保留 M01 十五项完整门禁的原始通过证据；新增 YAML/AST/缺父目录验证失败另记，不能用普通绿灯覆盖。尚无该分支的打开 PR，也没有 b16437c 的 PR workflow run；Release workflow 只在 main/v* push 触发，PR 普通 CI 即使通过，也不能证明此安装 job 实跑通过。

当前 main 仍可快进，但暂不建议合并最新分支。N10 隐私强取证、N11 原生交互、N12 的实际安装/系统/发布签字、N13 真实历史性能仍待验。本轮只同步复核与整合计划，未改产品、workflow 或安装脚本；因存在未解决发布阻断，按 `CLAUDE.md` 的默认提交流程例外不提交、不推送，不创建 PR、合并 main 或触发发布；并发研究文件及其暂存内容保留。

<a id="combined-install-review"></a>

## 两份执行答复合并后的统一复核结论

本轮再次读取 HEAD 和远端功能分支，均为 `b16437c51bdef7ea1bddec955dafe41d6c27b59f`；相对于 a58684c，产品源码、Cargo 清单/锁文件和 checks action 没有变化。M01 定向复跑实际为 **3 pass / 2 ignored**，Release metadata 再次 **7 pass**；上一轮十五项完整本地门禁的 456/42/47/361 计数保留，不冒充本轮又跑了一次全套。GitHub 查询仍无打开的 PR、无 b16437c 的 PR workflow run。

执行答复的真实性分开判断：M01 修复、两个提交已推送、install-test/三份 helper 存在、publish 依赖 install-test、PS1 带 UTF-8 BOM 和 270 行 CRLF 均属实；“workflow 接线正确”和“PR CI 绿后即可合并”不能成立。N12 的脚本提交不等于安装/升级/卸载已通过。M02–M04 本轮再次复现，另补三项 P2 验收缺口，全部归 N12，不重开 N09。

| ID | 优先级 | 当前发现 | 修复/验收要求 |
| --- | --- | --- | --- |
| M02 | P1 | install-test 缺直接 gate 依赖，SHA/版本为空，下载产物名错误 | 明确依赖 gate 和 build，再核对全部 needs 输出及 artifact 名 |
| M03 | P1 | 实际制表符拆断脚本调用路径，命令目标成为目录 | 修复完整路径，并解析真实 run 脚本核对目标 |
| M04 | P1 | 造旧库前未创建 DataDir，缺父目录时 SQLite 打开失败 | 明确创建目录，在全新及固定旧包场景验证 |
| M05 | P2 | 已有库达到三条即返回，不能证明本次启动的应用成功读取 | 等待本次启动的进程/Ready/实际读库完成证据，检出异常退出 |
| M06 | P2 | 只数两个 project_key，不核对项目路径或 cwd 原值 | 逐记录比对预期项目、session_initial_cwd/event_cwd，补路径损坏反例 |
| M07 | P2 | 缺旧包 SHA 仍安装；SHA 错误只登记失败后继续安装 | URL/SHA 成对校验，校验失败在安装前终止；未配置则明确跳过 |

### M05：重新安装场景可由旧库造成误通过

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 136–149、209–212 行。Wait-AppMigration 启动进程后只检查 history.db 是否已有至少三条记录；不读取进程句柄或本次启动的成功信号。重新安装时库原本就有三条记录，所以条件不依赖新应用是否成功打开它。

本轮从真实 PS1 的 AST 提取原 Get-HistoryFacts、Wait-AppMigration、Assert-MigratedFacts 和 Assert-True，复用实际 inspect_history.py；仅替换启动、休眠和注册项查询，模拟应用启动后退出码 23。临时库保持原散列不变，原等待函数仍返回 3/180/35/2，四项断言全部通过。**这是控制流反例，不是已经实跑安装包或证明当前应用崩溃。** 新应用的异常退出必须使该场景失败；已有库计数只能证明数据仍在。

### M06：项目数量相同不能证明 cwd 保留

位置：[inspect_history.py](../../scripts/inspect_history.py) 第 38–42 行；[test_install.ps1](../../scripts/test_install.ps1) 第 152–157 行。helper 只返回 COUNT(DISTINCT project_key)，不读取项目/cwd 原值；PS1 只比较 Projects=2。

同一受控探针将三个记录的路径都改成 C:/wrong/one、C:/wrong/two，同时保持输入/输出与项目数量；实际 helper 和原四项断言继续通过。正常 fixture 的两项目应为 C:/work/alpha、C:/work/beta，数量相同的路径替换未被检出。应比对逐记录身份与路径值，必要的 cwd 为 NULL 时也应显式验证；不以“两个项目”签 cwd 不丢失。

### M07：固定旧包的哈希约束没有强制执行

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 222–234 行。进入旧包场景只要求 URL 非空，SHA 为空时直接跳过校验；SHA 不一致时 Assert-True 仅登记失败，仍继续执行旧包和新版安装。最终会失败不等于安装前已阻断。

本轮提取原固定旧包 if 块及原断言函数，替换下载/安装/用户目录重置等外部动作进行受控验证：仅 URL 时安装调用两次、零失败；配置错误 SHA 时记录一个失败，安装调用仍两次。未下载或执行真实安装器，所有文件只在命名临时目录。固定旧包应同时要求 URL 和 SHA，并在哈希不匹配时先终止，不进入安装步骤。

### 编码与运行边界

PS1 的 BOM 与 CRLF 声明属实，但它的 `#Requires -Version 5.1` 不能代表运行兼容。第 53、110、112 行使用 utf8NoBOM；本机 Windows PowerShell 5.1.19041.6456 实测 Set-Content 参数绑定拒绝该值。此处与原中文解析问题不同。Windows Actions [默认使用 PowerShell Core](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idstepsshell)，当前 pwsh 路线不受这个 5.1 参数问题影响；应明确运行版本/修正兼容性声明，或使用支持 5.1 的 UTF-8 写法，不把 BOM 当作完整运行验证。

PR CI 不执行 Release 的 install-test。修完这些缺口后，还需取得当前产物的 Actions 安装实跑结果；可以复用安装逻辑到 PR 或独立的不发布验证入口，不能以 PR 普通绿灯代替。固定旧版本升级未配置时仍待验。真实 DPI、跨屏、休眠、自启与发布签字保持 N12 原排期；N10/N11/N13 保持待验。

本轮最终状态仍是 **9 完成、1 部分完成、3 本轮验收未执行，共 4 项待办**。只更新同一整合计划、总账和复核报告，未新增竞争计划，未改产品或安装实现；因 P1 阻断尚未闭合，本批复核文档继续按 CLAUDE.md 例外不提交推送。

<a id="install-fix-recheck"></a>

## `cc7b7d3` 安装门禁修复独立复核

- 日期：2026-10-11；被审实现：`cc7b7d3292392d4a13b4cd30de8618c9d493072e`。
- 结论：六项中的四项已闭合，M05/M06 部分修复但未满足通过条件；新增手动工作流存在版本传参和触发入口两个 P1 问题。**当前不能据此合并 main。** 上文 b16437c 的旧接线、目录及哈希问题不再作为当前缺陷。
- 范围：读取实际提交差异、解析两份 workflow 与实际 PowerShell AST、在命名临时目录构造 SQLite 数据并执行原函数体。未在本机安装/卸载应用、清理用户数据、创建 PR、合并或发布。

### 已闭合项

| 项目 | 当前证据与结论 |
| --- | --- |
| M02 | Release install-test 直接依赖 `[gate, build]`；解析全部 needs 输出引用，没有非直接依赖读取；gate 的版本输出存在 |
| M03 | 两份安装 workflow 无实际 U+0009；实际 run 块的 PowerShell AST 均解析成完整 `.\scripts\test_install.ps1` 命令目标 |
| M04 | 实际 make_legacy_cache.py 在父目录不存在时退出 0；读回 schema_version=10、3 请求/180 输入/35 输出/2 项目 |
| M07 | 提取原旧包 if 块，模拟外部动作：缺 SHA 时下载 0/安装 0/失败 1，错误 SHA 时下载 1/安装 0/失败 1，正确 SHA 时下载 1/安装 2/失败 0。哈希失败在旧包安装前阻断 |
| 运行版本声明 | AST 的 RequiredPSVersion 为 7.0，语法错误 0；声明与 utf8NoBOM/pwsh 路线一致。不是已在本机完整执行 PS7 脚本的证明 |

### M08 / P1：手动工作流传入分支名，正常安装版本必然不匹配

位置：[install-test.yml](../../.github/workflows/install-test.yml) 第 37–38 行；[test_install.ps1](../../scripts/test_install.ps1) 第 192 行。

新入口把 `${{ github.ref_name }}` 传给 AppVersion。当前 ref_name 为 `feat/project-path-unification`，应用配置版本是 `0.1.0`，NSIS 的 DisplayVersion 使用应用版本；分支名不是安装版本。即使成功安装，原断言仍登记失败，脚本最终退出 1，不能取得该分支的成功安装记录。运行 tag 时 ref_name 通常带 `v`，同样不能直接作为安装版本。

本轮提取实际 Assert-True 与原版本断言，分别注入当前分支名和实际配置版本：失败 1，消息为 `DisplayVersion = feat/project-path-unification（实际 0.1.0）`。应从本次 checkout 的应用配置读取并核对版本，使用该值传参；现有发布身份脚本只允许 main/v*，不能直接在功能分支调用其发布入口。

### M09 / P1：仅功能分支新增 workflow_dispatch，所述合并前 UI 入口尚不存在

位置：[install-test.yml](../../.github/workflows/install-test.yml) 第 9–10 行。

GitHub 只读仓库查询确认默认分支为 main；实时 main 为 `8d1d90d`，其树中仅有 ci.yml/release.yml，没有 install-test.yml。GitHub [手动运行规则](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow)要求 workflow_dispatch 工作流先存在于默认分支，之后才能在 Run workflow 中选择其他分支。本轮新增文件只在功能分支；“现在去 Actions → Install test → Run workflow 选择当前分支”不能作为现状下可用的取证步骤。

需要给合并前验收提供可触发入口，例如可信 PR 的不发布安装工作流，或先独立接入默认分支的手动入口，再选择功能分支。先合并全部实现才能测试，会失去用户要求的合并前取证条件。本轮仅审查，未更改触发规则或默认分支。

### M05 / P2：遗留 WAL 仍会让新应用失败误判为读库成功

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 136–150、234–238 行；Stop-App 第 90–92 行。

Wait-AppDbOpen 不记录启动前状态，也不持有新进程句柄，只查 `history.db-wal` 是否存在。前面的 Stop-App 使用强制结束；SQLite [WAL 文档](https://sqlite.org/wal.html)明确说明异常退出可留下 WAL。实际 inspect_history.py 以只读方式访问 WAL 数据库，也能产生并留下该文件。因此该文件既不能归属到新实例，也不能证明应用成功查询保留数据。

本轮用独立 SQLite 子进程打开合成库后直接退出 23，留下真实 WAL；再执行原 Wait-AppDbOpen 与场景 5 的等待/读数/断言，模拟新应用也退出 23。结果等待立即成功、数据库散列不变、3/180/35/2 正确、六项断言通过、失败 0。不是当前安装包已崩溃，而是脚本无法检出该反例。固定等待 8 秒也不证明迁移或查询完成。

通过条件仍是本次启动的可归属证据：进程持续存活，并成功完成预期历史查询/初始化，异常退出明确失败；查询结果与已知事实核对。WAL 可辅助诊断，不能单独签“当前程序读取成功”。不能直接删 WAL 制造信号，WAL 可能保存尚未 checkpoint 的已提交数据。

### M06 / P2：路径断言是子串包含，没有保证恰好两条预期路径

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 174–178 行；[inspect_history.py](../../scripts/inspect_history.py) 第 43–48 行。

helper 的 --projects 已正确返回原 project_key，但原断言用未锚定的 `-match` 搜索整个输出。保留两项目计数，把路径改成 `C:/work/alpha-incorrect`、`C:/work/beta-incorrect` 后，六项原断言全部通过、失败 0；helper 实际输出的确是这两条错误路径。任意 C:/wrong/one、two 的旧反例现在失败 2，说明改动有效，但“恰好 alpha/beta”这一声明仍不成立。

应按行解析后与预期路径集合严格相等，并检查 helper 退出码。cwd 保留的原验收范围还包括 session_initial_cwd/event_cwd，当前 helper 只读 project_key，不能以项目身份路径代替逐记录 cwd 元数据的验证。

### 本轮验证、远端状态与计划边界

- 实际 native_acceptance：**3 pass / 2 ignored**，三个父测试通过；M01/N09 保持闭合。
- 实际 review_regressions：**29 pass**；Release metadata：**7 pass**。更早 a58684c 的十五项完整普通/acceptance 门禁保留为历史基线，不冒充本轮又执行全套。
- 缺父目录造库、两份 workflow 的 needs/路径/无制表符、原函数 M05/M06 反例、原旧包块三分支与原版本断言均取得新证据。原函数抽取不执行脚本顶层 Reset-UserHome 或真实安装；本机为 PS5.1，仅执行不含 PS7 专有写文件参数的原函数子集，不据此声称完整 PS7/Runner 验收通过。
- HEAD 与实时远端功能分支均为 cc7b7d3；main 是祖先，**0 behind / 53 ahead**。只读 GitHub 查询没有打开的 PR；不限事件的 `/actions/runs?head_sha=cc7b7d3292392d4a13b4cd30de8618c9d493072e` 返回 total_count=0，当前产物没有 Actions 实跑记录。此前直接匿名 API 查询触发限流，随后通过已授权连接器完成查询；未用“仅 PR runs”的 wrapper 推断手动运行不存在。
- N01–N09 已完成，N12 部分完成，N10/N11/N13 维持待验：**9 完成 / 1 部分完成 / 3 本轮验收未执行，共 4 项待办**。M05/M06/M08/M09 全部由 N12 承接；实际旧包升级、DPI/跨屏/休眠/自启及发布签字没有因此关闭。

先修 M08/M09 的确定性阻断和 M05/M06 的误通过，再取得当前 SHA 对应的 Actions 安装结果及 PR CI。未配置固定旧包时仍明确保留升级待验。仅修改复核与状态文档；按 [CLAUDE.md](../../CLAUDE.md)“存在验证失败/未解决阻断”例外，本批不提交推送。

<a id="install-third-recheck"></a>

## `e99a4f3` 第三轮安装修复独立复核

- 日期：2026-10-11；被审实现：`e99a4f39f61eac9c255da2b9d692b6b7f9c9321e`。
- 本轮仅改 install-test.yml、test_install.ps1 与计划执行账；产品 Rust/前端及 Release 安装门禁源码未变。
- 结论：M08/M09 已解决当前 PR 的版本及触发问题；M05/M06 有效修复了上一轮的直接反例，但原通过条件尚未全部闭合；新增 M10 路径过滤范围缺口。**暂不建议合并 main**。旧 M02/M03/M04/M07 保持闭合，没有恢复为当前缺陷。

### 修复有效的部分

| 项目 | 独立验证 |
| --- | --- |
| M08 | 从实际 workflow 提取版本步骤并运行，写出 `version=0.1.0`；安装步骤引用同一 app_version 输出，不再传 ref_name |
| M09 | 实际 YAML 包含 `pull_request`、目标 main，当前工作流变更命中 paths；可通过 PR 在合并前取证，无需先把整份实现合入 main。workflow_dispatch 的默认分支限制本身没有改变 |
| M05 的旧 WAL 反例 | WAL 修改时间不变化、新进程模拟退出 23 时，原 Wait-AppDbUse 超时抛错，旧文件直接误通过问题已修 |
| M06 的路径后缀反例 | 正确 alpha/beta 通过五项断言；alpha-incorrect/beta-incorrect 触发一项严格集合断言失败，子串漏洞已修 |

GitHub [pull_request 规则](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#pull_request)默认在 opened/synchronize/reopened 时运行，checkout 默认读取 PR merge ref。新增 PR 路线用于当前合并前测试可行；取得结果时应同时记录 PR head SHA 与实际 merge/ref SHA，不把提交头和构建产物身份混为一谈。此处没有真实运行记录，不写成安装通过。

### M05 / P2：WAL 更新后异常退出仍会签成成功读取

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 136–153、240–244 行。Wait-AppDbUse 仍不持有新进程句柄，只要求 WAL 修改时间晚于启动；之后固定等待 8 秒，未检查存活或本次初始化/查询完成。部分启动成功产生数据库写入，之后异常退出，也满足该条件。

本轮提取实际原函数，用 SQLite 子进程提交一个仅含启动标记的辅助表后直接退出 23；该子进程从未查询 usage_events，旧三条用量不变。原等待函数观察到 WAL 时间由 `14:05:22.8923605Z` 更新到 `14:05:24.3799268Z`，正常返回；真实等待 8 秒后，实际 inspect_history.py 与五项断言全部通过、失败 0。进程已经退出，主数据库散列仍与此前一致，用量仍是 3/180/35/2。

这证明脚本不能检出“写库后退出”，不是已经证明当前安装包崩溃。应在启动时取得进程句柄，在等待及观察期明确检测退出，并等待可归属到本次实例的初始化/历史查询成功证据，核对实际查询结果。WAL 时间可辅助观察，不能替代成功完成；不直接删除 WAL。

### M06 / P2：项目身份已严格相等，两个 cwd 字段仍未验证

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 172–185 行；[inspect_history.py](../../scripts/inspect_history.py) 第 43–48 行。

项目路径集合已修复，应关闭上一轮子串问题。但验收仍只读取 project_key，没有读取 session_initial_cwd/event_cwd；这是最初 M06 的逐记录元数据范围，不是本轮新加要求。保留 alpha/beta 项目及 3/180/35/2，将三条记录的初始 cwd 全部替换为 `C:/wrong/initial`、事件 cwd 全部替换为 `C:/wrong/sub`，原五项断言仍全部通过、失败 0。

应按请求身份核对每条记录的初始 cwd 与事件 cwd，包括原 fixture 中 event_cwd=NULL 的语义；项目身份路径不能代替 cwd 原值。该发现不证明迁移实现实际损坏元数据，而是当前安装验收不能检出损坏。

### M10 / P2：PR 安装过滤漏掉根 Rust 库、依赖及 setup action

位置：[install-test.yml](../../.github/workflows/install-test.yml) 第 17–21 行。paths 只包含 src-tauri、frontend、scripts 和 .github/workflows；核心解析、历史库与聚合在根 src/，而 [壳 Cargo.toml](../../src-tauri/Cargo.toml) 的 tokenscope 依赖为 `path = ".."`。单独修改 src/history.rs、根 Cargo.toml 或 .github/actions/setup/action.yml 时，当前过滤均不命中，安装包内容或构建环境可能已改变，安装工作流却跳过。

本轮解析实际 patterns，对这些具体路径求匹配，结果全为 false；当前 workflow 文件为 true，所以这一项不会阻止本次大 PR 的触发。GitHub [路径过滤规则](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#onpull_requestpull_request_targetpathspaths-ignore)规定至少一个路径匹配才运行。应覆盖所有安装产物输入，或去掉工作流级过滤。Release 的 main push 安装门禁不受此 PR 过滤影响；本项属于新增 PR 门禁的覆盖遗漏，不另开责任计划。

### 验证与当前交付状态

- 实际 native_acceptance **3 pass / 2 ignored**，review_regressions **29 pass**，Release metadata **7 pass**；脚本 AST 语法错误 0、RequiresVersion=7.0。此前完整普通/acceptance 门禁保留为历史基线，本轮未重复全套。
- 两份 workflow 无制表符且全部 needs 输出属于直接依赖；实际版本步骤写出 0.1.0。旧 WAL 超时、写入后崩溃误通过、正确路径、错误路径、错误 cwd 元数据五个探针分别取得证据；抽取原函数体不运行脚本顶层的真实安装/卸载/Reset-UserHome。
- 当前本机为 PS5.1；只运行不含 PS7 专有参数的原函数子集与版本步骤，不冒充整份 PS7/Actions 安装验证。SQLite、所有探针和步骤输出只落命名临时目录，没有接触真实用户数据；未新增仓内诊断源码。
- HEAD 与实时远端功能分支均为 e99a4f3，main 为 8d1d90d，**0 behind / 54 ahead**，可快进。GitHub 只读查询无打开 PR；当前 SHA 不限事件的 Actions runs 返回 **total_count=0**。PR 触发接线完成不等于 PR 已创建或检查已运行。
- N01–N09 已完成，N12 部分完成，N10/N11/N13 保持待验：**9 完成 / 1 部分完成 / 3 本轮验收未执行，共 4 项待办**。M05/M06/M10 全部并入 N12；固定旧包升级和系统/发布签字继续保留原验收边界。

本批只同步复核与计划状态，保留旧失败及执行者声明为历史。先补剩余验收缺口和 PR 过滤，再取得对应产物的 PR 安装及普通 CI 成功结果；按 [CLAUDE.md](../../CLAUDE.md)验证失败例外，不提交推送本批文档。

<a id="install-fourth-recheck"></a>

## `b875815` 第四轮安装修复独立复核

- 日期：2026-10-11；被审实现：`b87581597b49906d5b5e5fc25023199171e1e063`，本机 HEAD 与实时远端功能分支一致。
- 本轮只修改 install-test.yml、test_install.ps1、inspect_history.py 和整合计划；Rust、前端、Release workflow、旧库生成器均未变。
- **结论：M10 已闭合；M05/M06 部分修复，仍各有一个验收缺口。暂不建议合并 main。** 上轮“退出码 23”和“整批 cwd 改错”反例已能检出，不能继续作为当前失败。以下反例说明验收不能检出某些错误，不表示已证明产品发生这些错误。

### 修复有效的部分

| 项目 | 本轮独立验证 |
| --- | --- |
| M05 非零退出 | 提取实际 Start-AppHealthy/Close-AppGracefully 原函数；真实 SQLite 子进程写入辅助启动表后退出 23，WAL 时间更新，退出码断言失败 1，不能再被签成成功 |
| M06 整批字段损坏 | 正确 fixture 的七项事实断言通过；保留项目路径与 3/180/35/2，把初始/事件 cwd 全改错，现触发两项断言失败；event_cwd=NULL 的全空检查已接入 |
| M10 PR 路径过滤 | 实际 YAML 已含 src/**、Cargo.toml、Cargo.lock、.github/actions/**；上轮四个漏项以及壳/前端/脚本/workflow 共八个候选输入路径均匹配。目标仍为 main，当前分支的工作流改动也匹配 |
| 原接线与造库 | Release install-test 仍直接依赖 gate/build，publish 仍依赖三者；两份 workflow 无制表符。不存在父目录时，实际生成器建出 v10 库，四项读数为 3/180/35/2 |

### M05 / P2：历史日志仍可代替本次初始化或查询成功

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 168–169 行。日志检查扫描 logs 下全部文件，没有建立启动前的文件偏移基线，也未限定日志时间或本次查询标识。场景 5 保留场景 2 的整个数据目录，后者的“历史库写入”日志天然能够命中。

本轮用实际原函数和临时 SQLite 数据验证：先放一条前一天的日志，再启动真实 Python 子进程，仅提交 launch_probe 辅助表、从不查询 usage_events、不输出新日志，然后退出 0。WAL 修改时间更新；旧日志文件散列不变。真实等待 8 秒后，两个启动断言、退出码断言、七项事实断言全部通过：**10 通过 / 0 失败**。退出 23 的对照组则失败 1，证明新增退出码保护有效，而当前日志归属仍有缺口。

另外，[src/report.rs](../../src/report.rs) 第 1197–1207 行的“历史库写入”只标记 write_batch 完成。来源指纹登记、历史计数查询及之后的查询快照/汇总还未完成；该信息不能直接称为“历史查询成功”。第 1629–1637 行已有带查询会话标识的“汇总完成”日志，可作为更靠后的成功证据。

修复应记录启动前各日志文件长度/本次运行标识，只接受本次新增且能归属本次实例的成功信号，并检查历史查询确实完成。继续保留进程退出码检查；不能只把旧关键词换成另一个同样扫描全历史的关键词。

### M06 / P2：去重路径集合仍丢失逐记录关系及缺失值

位置：[inspect_history.py](../../scripts/inspect_history.py) 第 50–56 行；[test_install.ps1](../../scripts/test_install.ps1) 第 237–241 行。SELECT DISTINCT 加上 IS NOT NULL 会丢掉每条事实的身份、路径对应关系及缺失值。三个字段都读到了，不等于三条事实的元数据都正确。

本轮分别构造两个受控反例：把 m1 的初始 cwd 从 alpha 改成 NULL，m2 仍为 alpha、m3 仍为 beta；或者互换 m1/m3 的初始 cwd，项目键保持原值。两份库的去重初始 cwd 仍为 alpha/beta，四项用量读数仍为 3/180/35/2，event_cwd 仍全 NULL。实际 inspect_history.py 与原七项断言均得到 **7 通过 / 0 失败**。

应按稳定请求身份比对 fixture 的每条事实，至少核对 project_key、session_initial_cwd、event_cwd 的对应值和 NULL 语义；不能靠多个彼此独立的 DISTINCT 集合证明记录保留。此要求延续最初 M06 的逐记录验收边界，不另开新任务。

### 本轮验证与交付状态

- fresh `cargo test --offline --locked --features acceptance --test native_acceptance --test review_regressions --test history_migration -- --test-threads=1`：迁移 **8 pass**、native acceptance **3 pass / 2 ignored**、review_regressions **29 pass**，共 **40 pass / 2 ignored**；发布元数据 unittest **7 pass**。
- 两份 workflow 的 YAML、无制表符、needs 与八条路径覆盖检查通过；实际 PS1 AST 语法错误 0；两个 Python helper 编译通过，实际旧库生成器通过。本轮未重复全套普通/前端门禁，先前完整结果仅作历史基线。
- 六组隔离函数探针取得新证据；不执行安装脚本顶层，不执行真实安装、卸载或 Reset-UserHome。本机是 PS5.1、没有 pwsh，只运行可兼容的原函数子集，不冒充完整 PS7/Windows Runner 验收。所有数据库、日志和探针产物只在命名临时目录。
- GitHub 只读核验：当前功能分支无打开 PR；当前 SHA 不限事件的 Actions runs 返回 **total_count=0**。没有本次安装实跑绿灯；PR 路径接线完成不能写成 PR 检查已运行。main 为 8d1d90d，功能分支 **0 behind / 55 ahead**，Git 可快进不代表验收完成。
- 当前总账维持 **9 完成 / 1 部分完成 / 3 本轮验收未执行，共 4 项待办**。N09 保持完成；剩余 M05/M06 与安装实跑均由 N12 承接，固定旧包升级、系统/发布签字仍待验，N10/N11/N13 不变。

本轮同步复核与整合计划，保留此前各轮和执行者声明为历史。先闭合两个验收缺口，再取得对应产物的 PR 安装与普通 CI 成功证据；按 [CLAUDE.md](../../CLAUDE.md)验证失败例外，本批文档不提交推送。

<a id="install-design-recheck"></a>

## `a64d340` N12 修复设计实施独立复核

- 日期：2026-10-12；被审实现：`a64d3400fa1874659be6351a2a4d7a380a39ef3a`，本机 HEAD 与实时远端功能分支一致。
- 依据：[N12 的 M05/M06 修复设计](../plans/active/2026-10-11-consolidated-remaining-work.md#n12-install-design)，逐项核对五步实现及执行者报告；未修改产品或验收脚本。
- **结论（`a64d340` 时点）：M06 校验器和生成器契约可确认完成；M05 实际场景集成及门禁验证未完成。三个 P1 阻断、三个 P2 覆盖缺口仍需修复，不能仅等待 Actions 就签实施完成。**
- **修复结论（2026-10-12 追加，`b855021` 提交、`788905a` 记账）：A01–A06 六项已全部闭合。** A01 启动函数改名 `Launch-AppOnce` 并补 `-LiveLogsDir`；A02 等待改读 `DataDir\logs`、与证据目录分离，关闭后当前日志复制回证据并随 `always()` 上传；A03 GUI 替身换 `notepad`（`WM_CLOSE` → 退出码 0，控制台宿主 0xC000013A 不再当作优雅退出）；A04 五个日志反例接存活替身，失败原因被真实约束；A05 `install-test.yml` 构建前补两条契约命令（M06 严格事实校验、M05 生命周期，pwsh 7.6.6 实跑）；A06 卸载后按 manifest 逐条复核保留事实。门禁终态（循环收敛至 `6fab12f`）：根库 461 + acceptance、壳库 45、前端 364、python 36、pwsh 契约 22。下列 A01–A06 各节保留当时的复现证据。

### 已确认有效的部分

`python -B -m unittest discover -s scripts -p test_inspect_history.py -v` 实跑 **22 通过**。校验器在只读事务内读取全部事实，使用 `(app, session_id, record_id)` 核对身份及字段；能拒绝单条初始 cwd 丢失、cwd 互换、项目错误、非空 event_cwd 丢失、空串/字面 NULL 与 SQL NULL 混淆、四桶改动及额外/缺失/重复记录。缺库、缺表、缺列和坏期望文件返回 2；行序/rowid 改变不影响结果。旧库生成器读回与独立 JSON 规格逐条一致，两会话、非空 event_cwd 及 3/180/35/2 均确认。这里确认的是事实校验与合成输入契约，真实 NSIS 迁移仍需安装实跑。

生命周期等待函数已改认完整行的“汇总完成”，检查请求数及同一进程存活，关闭要求 ExitCode=0；临时空来源目录、子进程期间 RUST_LOG=info 和正常退出后的二次事实校验也已接入。Release 的 `install-test.needs=[gate, build]`、`publish.needs=[gate, build, install-test]` 及两个安装 job 的 `always()` 上传配置正确，旧的 M02/M03/M10 不重新列为缺陷。

### A01 / P1：实际场景调用不存在的启动函数

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 154 行；[install_test_helpers.ps1](../../scripts/install_test_helpers.ps1) 第 63 行。主脚本调用 `Launch-AppOnce`，库只定义 `New-AppContext`，没有别名或其他定义。PowerShell AST 语法通过不会检查该运行期调用。

本轮只抽取真实 `Run-AppLifecycle` 函数，将 Stop-App、卸载注册项读取替换为临时测试桩，未执行安装脚本顶层。结果直接报“Launch-AppOnce 不会被识别为 cmdlet、函数、脚本文件或可执行程序”。场景 2/5/6 均走这个函数，场景 2 在启动应用之前就终止；调用还位于 try 块之外，因此没有场景 result.json。统一函数名，并增加调用实际编排函数的契约，不能只测手工拼装的 Context。

### A02 / P1：等待目录不是应用的当前日志目录，上传也遗漏当前日志

位置：[install_test_helpers.ps1](../../scripts/install_test_helpers.ps1) 第 60、145 行；[test_install.ps1](../../scripts/test_install.ps1) 第 150–155 行。Prepare 返回 `evidence/<scenario>/<run_id>/logs`，该路径成为 Context.EvidenceDir；Wait 再拼一次 logs，实际扫描 `evidence/<scenario>/<run_id>/logs/logs`。生产日志仍写到 DataDir/logs（[logging.rs](../../src/logging.rs) 第 54–71 行），主脚本虽然计算了该目录，却没有传给等待函数。

隔离复现使用存活的当前 PowerShell 进程句柄，在临时 DataDir/logs 写入完整的 `q-current` 汇总 3 请求；等待仍超时。即使修复 A01，该问题也会阻断首启。当前脚本还没有将本次 DataDir/logs 复制到证据目录，always 上传只能拿到归档的旧日志，不能证明本次实例行为。

应让 Prepare 返回具有独立 `LogsDir`、`EvidenceDir`、`PreviousLogsDir` 的上下文：等待只读 DataDir/logs，旧日志移入 evidence/previous-logs，本次日志在场景 finally 中复制到 evidence/current-logs；成功和失败都保存结果。对目录字段及日志归属补正反例，避免靠修改一处 Join-Path 维持隐含约定。

### A03 / P1：本机 PowerShell 7 契约测试仍失败，未取得“21 项全绿”

位置：[test_install_helpers.ps1](../../scripts/test_install_helpers.ps1) 第 77–90、245–267 行。本轮以 **PowerShell 7.6.6** 实跑 `pwsh -NoProfile -File scripts/test_install_helpers.ps1`，最终退出 **1**：完整生命周期正例在 Close-AppGracefully 报“优雅关闭退出码 -1073741510 != 0”，即 `0xC000013A`。此前 18 条显示 PASS，最后三条断言未执行，不是 21 条全部通过。

替身仍通过 Start-Process 启动带控制台的 pwsh，再在其内部运行 WinForms。这没有消除被关闭的主窗口属于控制台或 GUI 尚未准备好的问题。质量门禁已经调用此脚本，因此当前不能称本地 CI 同口径全绿；没有远程运行证据时也不推断 Runner 已通过或必然产生同一退出码。替身应使用确定无控制台的 GUI 进程，并在 GUI 窗口就绪后请求关闭；保留真实发布应用退出码必须为 0 的要求。

### A04 / P2：五个反例因缺进程句柄抛错，未验证相应日志行为

位置：[test_install_helpers.ps1](../../scripts/test_install_helpers.ps1) 第 114–144、194–200 行。旧日志、仅写库、请求数 0/2、半行五种场景创建 Context 后没有赋值 Process。Wait 在进入日志检查之前就抛“上下文缺少进程句柄”；Test-Throws 没有限定异常原因，因此全部显示 PASS。

本轮对这五种 Context 独立调用，五个异常均为缺进程句柄。旧日志用例还没有执行 Prepare/归档，所以不能证明旧日志与本次证据隔离。每个日志反例应先提供实际存活的受控句柄，分别约束超时、请求数不符等原因；旧日志用例必须调用同一个 Prepare 和场景编排。加上实际启动/准备的契约，才能拦住 A01/A02。

### A05 / P2：PR 安装工作流没有构建前的两条契约命令

位置：[install-test.yml](../../.github/workflows/install-test.yml) 第 52 行。实际步骤是 setup → 读版本 → production build → 安装 → 上传；没有 test_inspect_history.py 或 test_install_helpers.ps1，也没有调用 checks action。YAML 解析确认安装 job 两条命令均缺失，与“checks 与 install-test.yml 在生产构建前运行两条契约”的答复不符。

checks action 已包含两条命令，Release gate 也使用它，这两处接线成立；它们不能代替独立 PR 安装 workflow 内的前置执行。按设计在 PR 安装构建前显式运行两条契约并检查退出码，或引用只包含这两条命令的公共 action。

### A06 / P2：卸载场景只检查数据库存在，没有校验保留事实

位置：[test_install.ps1](../../scripts/test_install.ps1) 第 206–221 行。场景 3 执行 manifest 校验，但场景 4 卸载后只 Test-Path settings/pricing/history.db，没有 Verify-HistoryManifest；“场景 3/4 都走 manifest”的答复不成立。

卸载若清空事实而保留空库文件，这一步仍会显示 PASS；后续重新安装还可能从保留的 cache.db 再迁移恢复，从而掩盖卸载时发生的数据损坏。卸载完成、进入重新安装之前立即执行同一只读 manifest 校验，才能验证设计要求的每阶段数据保留。

### 验证、交付及计划状态

- Python M06 契约 **22 pass**；发布元数据 **7 pass**。实际 Rust 定向迁移 **8 pass**、native_acceptance **3 pass / 2 ignored**、review_regressions **29 pass**，共 **40 pass / 2 ignored**。
- PS7 生命周期契约退出 **1**；仓外探针复现不存在的真实场景调用、当前成功日志读不到及五个无效反例。未执行真实 NSIS 安装、卸载、用户目录清理或产品启动；探针和合成事实仅在命名临时目录。
- 三份 workflow/action 的 YAML 与无制表符检查通过；PR 安装 workflow 缺少两条前置契约命令，语法通过不代表调用或流程正确。此前完整 Rust/前端门禁保留为历史基线，本轮不重复全套，也不据其声称新的 PS7 门禁全绿。
- GitHub 只读查询：当前功能分支没有打开的 PR，当前 SHA 全事件 Actions runs **total_count=0**。本机和 origin 功能分支为 a64d340，main 为 8d1d90d，**0 behind / 57 ahead**；Git 可快进不代表允许签合并验收。
- 总账保持 **9 完成 / 1 部分完成 / 3 本轮验收未执行，共 4 项待办**。N09 的原 M01 继续关闭；本轮新安装脚本/契约缺口归 N12。N10/N11/N13、固定旧包升级、系统/发布签字继续待验；不重新恢复旧计划。

A01–A06 已由 `b855021` 修复并经 `788905a` 记账（见本节修复结论），当时的修复优先级与“不提交推送”决定属历史状态；该验证失败例外已消失，本报告已入库。下一步是取得当前安装产物的 PR 安装与普通 CI 实跑证据后评估合并。执行者原声明保留为历史。

<a id="current-merge-readiness"></a>

## 2026-10-12 当前 main 合并门槛独立复核

代码与测试基线为 `231f667099aff035ee2ef240dd6d356a843e8de7`；复核期间新增 `e31d1b67c0323cb1ed5466bff61ecb1819cd99d6`，差异仅为十份状态文档，没有改变被测源码、脚本、依赖或打包输入。本轮重跑完整普通与 acceptance 本地门禁，并核对从 a64d340 起的核心、前端、打包及安装脚本修改；没有重新执行此前已完成的百万性能或用户真实数据测试。

**结论：未发现新的 P1/P2 代码阻断，A01–A06 均闭合，代码与本地门禁已就绪。正式合并前仍必须取得当前 PR 的 CI/gate 与 Install test/install-test 成功结果；当前两者都没有运行记录，因此不能写成已满足全部合并门槛。**

| 验证 | 本轮实际结果 |
| --- | --- |
| 根库普通 workspace test | 462 通过 / 10 忽略 / 0 失败 |
| 根库 acceptance workspace test | 462 通过 / 10 忽略 / 0 失败；原 M01 路径验收维持通过 |
| 壳普通 / acceptance test | 46 / 51 通过，0 失败 |
| 双 manifest fmt 与普通/acceptance Clippy | 全部通过；Clippy 均含 -D warnings |
| 前端 typecheck、format:check、test、build | 全部通过，33 文件 / 364 测试 |
| Python 契约 | 36 通过：历史事实/生成器 29、发布元数据 7 |
| PowerShell 7.6.6 生命周期契约 | 22 通过，0 失败，退出码 0 |
| 安装脚本实际编排隔离探针 | 通过；使用原 Run-AppLifecycle、Launch-AppOnce 和事实校验函数，替换外部启动/关闭，缩短观察时间，只操作临时目录 |
| PR / Release workflow | 两条契约在 PR 构建前执行且检查退出码；Release needs 与发布依赖正确；两处 always 上传、YAML、无制表符检查通过 |

Cargo 命令与 checks action 对齐，额外使用 --offline 防止验证时改动依赖，均带 --locked。两份 Cargo manifest 分别执行，没有用根命令代替壳门禁。忽略项保留原定义，不将用户真实数据、百万性能或原生实跑的忽略项计为通过。本机 Cargo 1.99.0、Node 24.14.0、pnpm 11.22.0、Python 3.14.3；CI setup 使用 Node 22、Python 3.12 与 stable Rust，所以本地通过仍须由 Runner 验证。

### 上轮六项缺口的关闭证据

- **A01**：真实调用与库均为 Launch-AppOnce，参数 LiveLogsDir 明确传入。仓外抽取真实 Run-AppLifecycle，原函数已贯通准备、一次启动、汇总、两次 manifest、观察、关闭与结果保存，不再报缺函数。
- **A02**：Wait 读取 DataDir/logs，证据目录独立。隔离探针在旧日志归档后立即写 q-current-pipeline，实际汇总读取该 query_id/3 请求，关闭后的当前日志副本与 result.json 均存在；同一 PID 贯穿流程，父进程 RUST_LOG 恢复。该探针只证明编排和事实校验，未启动 TokenScope 或运行 NSIS。
- **A03**：正例已使用 GUI 替身，实际 PS7 测试正常关闭并以 0 退出，最后三条断言已执行；不再出现此前 0xC000013A。
- **A04**：旧日志、仅写库、0/2 请求及半行反例均提供存活句柄，并约束“超时”或请求数不符的异常文本；旧日志反例真实执行 Prepare 归档，不能因缺句柄冒充覆盖。
- **A05**：install-test.yml 的两条契约均位于 production build 之前，并显式检查 LASTEXITCODE；Release gate 继续使用包含这两条命令的 checks action。
- **A06**：卸载完成后、重新安装之前调用 Verify-HistoryManifest；数据库存在不再是保留事实的唯一断言。场景 2/5/6 共用生命周期，重复安装也校验同一规格。

### 远程证据与合并边界

只读查询 GitHub 全事件 Actions API：231f667 与后续 e31d1b6 的 total_count 均为 0；功能分支没有打开的 PR。实时 origin/main 为 `8d1d90de29aa00d10b9a7e4b8f906d909726446b`，仍是功能分支祖先；231f667 时点为 0 behind / 71 ahead，后续文档提交不引入合并冲突。没有创建 PR、合并 main、制造 tag 或发布安装包。

在 feat/project-path-unification → main 的 PR 中取得 **CI/gate 与 Install test/install-test 同一被合并版本的成功结果**，并检查安装证据中首启迁移、重复安装、卸载、重新安装的结果和当前日志，再签合并。Release 的发布前安装门禁继续独立生效，不能用合并后的 Release 实跑倒推合并前已通过。固定旧版本包升级仍按已确认方案为可选：配置 URL/SHA 后必须实际通过，未配置则记录跳过，不冒充该项完成。

发现一项非阻断的 P3 文案残留：[Settings.vue](../../frontend/src/views/Settings.vue) 第 1194 行把 I-P3-2 审计编号及“逐行闭合”说明写入设置页文本。它不改变导入或计量行为，可在 N11 的文案验收中清理；本次没有改产品源码，也不将其扩大为新的合并阻断。

总账保持 9 完成 / 1 部分完成 / 3 本轮验收未执行，共四项待办。N12 **安装部分**的剩余合并条件是当前 PR Actions 实跑；N12 其余 DPI/系统/发布签字、N10 隐私强取证、N11 原生统一验收、N13 真实历史性能继续按原排期，不因本地全绿关闭。当前没有本地验证失败，复核文档按 CLAUDE.md 的默认提交流程提交推送。

<a id="pr4-utf8-ci-fix"></a>

## PR #4 首次 Actions 失败与 UTF-8 输出修复

日期：2026-10-12；失败源 HEAD 为 `804230f43ecef68b26902c04fef86998bba715ca`。[PR #4](https://github.com/ywltby/tokenscope/pull/4) 为正式 PR，base=main、head=feat/project-path-unification，未合并。[CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38195571861/job/114637322957) 与 [Install test/install-test](https://github.com/ywltby/tokenscope/actions/runs/38195571882/job/114637323311) 的实际日志都显示同样两项失败：

- `test_mode_query_on_corrupted_db_is_exit_2`：错误文字应包含“无法按 SQLite 打开”，重定向 stderr 却得到字面量 Unicode 转义。
- `test_non_numeric_token_text_is_exit_2`：受检 token 错误应返回 2，实际返回 1。

安装 workflow 在生产构建前的 M06 契约步骤终止；M05 契约、NSIS 构建、五个必需安装场景未执行，always 上传未找到证据。旧包升级未执行，不能标为“已确认的可选跳过”。先前本地通过和源码复核不替代这次失败，804230f 不满足合并门槛。

### 根因及可重复验证

校验器采用 ensure_ascii=False 输出 JSON，中文错误也直接写 stderr，但没有指定标准流编码。Windows Python 3.12 的重定向管道可能沿用系统编码：stderr 的 backslashreplace 会把中文转义；stdout 的严格编码会抛 UnicodeEncodeError，程序来不及返回原定的 2，进程以未处理异常的 1 结束。原本地验证使用 Python 3.14 与 UTF-8 环境，遗漏了这一环境条件。

本轮以本机 Python 3.12.13，设置 PYTHONIOENCODING=cp1252、PYTHONUTF8=0，先复现原两项失败。中文仓库路径还使一项 JSON 事实差异输出为空，这是同源问题。新增 Utf8OutputCase 的四项回归在旧实现中有 12 个组合失败，均因诊断转义或 UnicodeEncodeError，修复后全部通过。

### 修复及本地门禁

[inspect_history.py](../../scripts/inspect_history.py) 在 CLI 入口、任何输出前将 stdout/stderr 的编码固定为 UTF-8，保留 JSON/逐行格式和 0/1/2 退出码语义。[test_inspect_history.py](../../scripts/test_inspect_history.py) 用真实子进程强制 CP1252，按原始字节严格解码 UTF-8，覆盖中文路径/模型的成功 JSON、四桶非法值的退出码 2、四种坏库查询的中文 stderr，以及三种逐行模式。原有断言没有放宽，未通过设置全局 CI 编码掩盖问题。

- Python 3.12.13 + CP1252/UTF8=0：事实契约 33 项通过；连同 metadata 共 40 项通过。
- Python 3.14 + UTF-8：全部 Python 40 项通过；新增回归自身仍强制子进程使用 CP1252。
- 根库普通/acceptance 各 462 通过/10 忽略；壳 46/51；前端 364；PS7 22；双 manifest fmt/Clippy（-D warnings）、前端 typecheck/format/build 全部通过。

修复推送来源分支后，由 PR synchronize 自动触发新的两套运行，原失败记录保留。合并须检查修复提交所对应的最新 CI/gate 与 Install test/install-test，以及真实安装场景和工件；此处只记录失败与修复时的本地验证，后续远程结果以 PR Checks 为准。未手工重跑原失败、修改检查规则或执行合并；N12 保持部分完成，其他系统/隐私/原生/性能任务不关闭。

<a id="pr4-workflow-binding-fix"></a>

## PR #4 第二次安装失败与 workflow 参数绑定修复

编码修复提交为 `dceee11c50daacf0f8134143d020d8905844a125`。[CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38198245109/job/114645639181) 全部成功；[Install test/install-test](https://github.com/ywltby/tokenscope/actions/runs/38198244797/job/114645637716) 的事实契约 33 项、生命周期 22 项与生产 NSIS 构建通过，随后场景 1 在真正执行安装器之前失败：Start-Process 收到的程序路径是字面量 `-InstallerPath`。断言 0 通过/1 失败，后续场景和旧包升级未执行，上传仍无工件。

原因是 PowerShell 的数组展开按位置绑定：`@('-InstallerPath', $file, '-AppVersion', '0.1.0')` 不能作为脚本的名称参数展开，数组中的参数名会成为普通字符串值。PR 安装和 Release 安装步骤都有这段写法。它属于真实 workflow 调用缺陷，已通过生产参数声明与实际 run 块在临时工作区复现，不重跑碰绿。

两处调用改为 `$installParameters` 哈希表，InstallerPath/AppVersion 及可选 OldInstallerUrl/OldInstallerSha256 按名称绑定，子脚本退出码仍直接传给 job。未修改安装器、事实和生命周期判定，也未放宽 CI。

新增 [test_install_workflow.py](../../scripts/test_install_workflow.py)：提取两份 workflow 的真实安装 run 块，并提取生产 test_install.ps1 的参数声明；在中文及空格路径的临时影子工作区中，用只记录参数的脚本替身执行。覆盖两份 workflow × 四种旧包变量配置 × 成功/失败退出码，共 16 个组合；确认安装器路径、版本、可选值及无多余参数均正确，退出码 37 也原样传递。旧实现 16 组合失败，修复后全部通过。整个测试不执行 NSIS、不改注册表、不清空真实用户数据。

该契约已接入共享 checks 和 PR 安装工作流的生产构建之前，Release gate 同时使用共享 checks。Python 3.12.13 强制 CP1252/UTF8=0 的全部 Python **42 项**通过（事实 33 + 调用 2 + metadata 7），三份修改过的 YAML 解析与 diff --check 通过；Rust/壳/前端实现未改变，上轮完整本地门禁及 dceee11 的远程 CI 成功证据保留。

本条记录对应修复时的现场与定向验证；推送后最新远程状态以 PR Checks 为准。dceee11 的安装失败不能签合并，须最新版本的 CI 与 Install test 同时成功并核对实际场景和工件。main 未合并，N12/其余待验项不关闭。

### 849c9be 实跑补充：调用契约的 8.3 路径误判

[第三次 Install test](https://github.com/ywltby/tokenscope/actions/runs/38199434645/job/114649314560) 与 [CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38199434648/job/114649314598) 在新增调用契约失败：安装器已按名称正确绑定，版本/可选值/退出码均正确，但 Python 临时目录保留 `C:/Users/RUNNER~1/...`，PowerShell 的 Get-ChildItem.FullName 返回 `C:/Users/runneradmin/...`。测试要求路径字符串相等，将同一个实际文件的两个名称误判为不同安装器；这是本轮新增测试的缺陷，未执行本次 NSIS 构建或安装，仍无工件。

本地用 GetShortPathNameW 得到真实临时根短名（TO68CE~1），将 TEMP/TMP 指向它，先重现 16 个组合全红。修复只调整路径验证：要求绝对路径、真实文件存在，并以 Path.samefile 确认文件身份；版本、旧包 URL/SHA、多余参数仍严格相等，0/37 退出码继续核对。错误参数名或另一安装产物仍不能通过。相同短名 TEMP 加 Python 3.12.13、CP1252/UTF8=0 的全部 **42 项**通过，不依赖修改 Runner 的临时目录来掩盖问题。

第三次运行按原失败保留，修复提交触发新运行。此补充仍只确认调用契约修复，最新 CI 与实际安装及工件须重新取得，不关闭 N12。

<a id="pr4-nsis-directory-fix"></a>

## 56592a5 实跑与 NSIS 注册目录修复

[CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38200032886/job/114651173150) 已全部成功；[Install test](https://github.com/ywltby/tokenscope/actions/runs/38200032829/job/114651172940) 的调用、事实、生命周期三条契约及生产 NSIS 打包通过。安装器实际执行，HKCU 注册项与 DisplayVersion=0.1.0 两项通过，随后的 Join-Path 报 `Cannot find drive. A drive with the name '"C' does not exist.`；场景 1 中断、后续及旧包升级未执行、无工件。

本机已生成的 `src-tauri/target/release/nsis/x64/installer.nsi` 第 676 行确认 Tauri NSIS 给 InstallLocation 写入了带外层双引号的 INSTDIR，这是合法注册值格式；原测试直接把它作为目录拼接。[test_install.ps1](../../scripts/test_install.ps1) 的 Get-UninstallEntry 现仅在读取结果中去掉成对外层引号，安装/启动/卸载所有消费者共用正确路径，注册表和 UninstallString 保持原值。

[调用契约测试](../../scripts/test_install_workflow.py) 新增实际 Get-UninstallEntry 函数回归：只替代注册表读取，输入同时含其他应用与 TokenScope 注册项；验证带引号/普通目录都能拼接到预期真实文件、版本及卸载字符串保留。旧实现的引号用例复现相同 `"C` 错误，修复后通过。真实短名 TEMP + Python 3.12.13 CP1252/UTF8=0 下全部 **43 项**通过（事实 33 + 调用/注册目录 3 + metadata 7），完整 PS1 的 UTF-8 语法解析通过；未执行本机 NSIS 或改用户注册表/历史数据。

原失败运行保留，最新修复提交须重新取得 CI 和真实五场景及工件。此处记录修复时的现场，后续远程状态以 PR Checks 为准；N12 及其他待验项不关闭。

<a id="pr4-live-log-sharing-fix"></a>

## a440dec 实跑与活动日志共享读取修复

[CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38201212010/job/114654827990) 全绿；[Install test](https://github.com/ywltby/tokenscope/actions/runs/38201212017/job/114654827326) 三条契约/生产打包通过，实际全新安装的注册项、版本、主程序及快捷方式四项通过。场景 2 启动后存活，却在等本次汇总 180 秒后超时；后续安装场景及旧包升级未执行，本轮 manifest 与正常退出未获验证。

[失败工件](https://github.com/ywltby/tokenscope/actions/runs/38201212017/artifacts/11725353819) 已实际下载读取，ZIP SHA256=`54d816dc0842f2167f0e50675e603b5156308de50d26e4c818e695ab3f2e3f40` 与 GitHub 工件元数据一致。内含本次 live-logs 与 result.json：应用日志确认旧库迁移 3 条、两次成功“汇总完成”，请求均为 3；结果却是 stage=launched、query_id/requests=null、pass=false。不是应用未查询，真实汇总行与原正则也匹配。

问题在 [Get-CompleteLogLines](../../scripts/install_test_helpers.ps1)：ReadAllText 默认 FileShare.Read，Windows 的新读取句柄不允许既有 writer 的写访问，抛 IOException 后被循环跳过。以本次工件的真实日志和持续持有的 FileShare.ReadWrite/Delete 写句柄本地复现：writer 打开时 helper 读 0 行，关闭后读 27 行。之前合成日志测试都在写完后关闭句柄，遗漏了运行中的 logger。

修复使用 FileAccess.Read 的 FileStream，并显式 FileShare.ReadWrite/Delete，再用 UTF-8 StreamReader 读取；所有句柄在 finally 释放。完整行过滤、归档旧日志、预期请求数、同一进程存活与退出码要求全部保留；文件轮转等 IOException 仍下一轮重读。[生命周期契约](../../scripts/test_install_helpers.ps1) 新增持续打开真实 writer 的四项断言，半行超时、写入换行后读取/本次 query_id/请求数 3 均验证；控制台替身隐藏运行。

旧实现的新正例失败，修复后 PS7 契约 **26 项**全绿；真实工件重放在 writer 打开/关闭时均读 **27 行**，两份 PS1 的 UTF-8 语法解析通过。Python 43 项及 a440dec 的普通 CI 证据保留，Python/Rust/前端实现未改变。此处只确认读取修复，推送后最新远程状态以 PR Checks 为准；仍须最新两套检查、真实五场景与工件，不关闭 N12。

<a id="pr4-fa911dd-green"></a>

## fa911dd 的 Actions 成功取证与合并结论

来源 HEAD=`fa911dd6445cee2c55279ea43008dcd60d936515`。[CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38203057083/job/114660447156) 与 [Install test/install-test](https://github.com/ywltby/tokenscope/actions/runs/38203057088/job/114660447331) 均 completed/success，来源 SHA 一致；原失败按原 SHA 保留，未手动重跑碰绿。

安装 job 实际完成生产 NSIS 打包及全新安装、首次启动迁移、重复安装、卸载、重新安装五个必需场景，**11 项断言通过/0 失败**。首次启动和重新安装各在关闭前后严格校验全部事实，重复安装、卸载后也校验，共六次 manifest 全部 ok=true/problems=[]，读数稳定为 **3 请求 / 180 输入 / 35 输出 / 2 项目**，各记录身份/原始模型/三 cwd/四桶及 NULL 语义均按规格核对。

[成功工件](https://github.com/ywltby/tokenscope/actions/runs/38203057088/artifacts/11726496236) 已下载读取，ZIP SHA256=`016c28b2fc4832713bd816051f33a960d7a7f48fc896dbab652d7cb185b51630` 与 GitHub 元数据一致。五个文件包含两次启动的 live-logs、两份 result.json 及归档旧日志；两个结果均 stage=closed、pass=true、requests=3。fresh-migrate PID=5488，reinstall PID=6968，query_id 不同且分别与各自本次日志的成功汇总吻合；正常关闭已在脚本中通过退出码 0 守卫，未用强制结束补成功。

普通 CI 包含两套 manifest 的普通/acceptance 与前端完整门禁，调用/注册目录 3 项、事实 33 项、metadata 7 项及 PS7 生命周期 **26 项**全部成功。PR #4 仍 open、merged=false、mergeable=true/clean；远端 main=`8d1d90d`，0 个 main 独有提交/78 个分支独有提交，main 仍是来源 HEAD 的祖先，工作区干净。

**结论：fa911dd 的运行时实现和已约定的合并验收通过。** 后续同步状态文档并修正单元测试的并行断言，安装脚本/运行时基线不变；合并时仍核对 PR 最新 HEAD 的两套检查。固定旧包升级因 OLD_INSTALLER_URL/SHA256 未配置而实际告警跳过，保持待验；DPI/跨显示器/休眠/自启及发布签字、N10/N11/N13 均按原排期，N12 整项不关闭。

### 状态同步时捕获的并行断言与修复

文档提交钩子捕获 submission_counter_covers_leader_work 的归零断言失败（1 != 0）。COLLECT_SUBMISSIONS_ACTIVE 是进程级计数；本航班结束后其它并行单元测试可立即进入采集，原“等待全局 0 后无锁断言”存在观察窗口。生产 SubmissionGuard 在释放 COLLECT_SERIAL 及发布结果前已析构，没有发现计数泄漏。

修复仅位于 src/report.rs 的 cfg(test) 模块：起始通道握手后直接严格断言 1；本航班 join 后取得相同 COLLECT_SERIAL 锁，阻止其它领队在归零断言期间进入，再严格断言 0。保留原握手超时，未清零计数或改成宽松范围，运行时未改。默认并行根库单元测试五轮各 208 通过；恢复原 5s 握手超时后的完整 workspace acceptance 462/10 忽略及 acceptance Clippy 通过。此提交仍需最新两套 PR 检查，不用前一 SHA 的绿灯代替。

### bb6b795 实跑与 GUI 替身窗口握手

[Install test](https://github.com/ywltby/tokenscope/actions/runs/38205913148/job/114669249575) 已成功；[CI/gate](https://github.com/ywltby/tokenscope/actions/runs/38205913143/job/114669249543) 的 Rust 普通测试通过，生命周期正例却在 CloseMainWindow 返回 false 处失败，后续 acceptance 跳过。GUI 替身原本 Start-Process 后立即返回，固定一秒观察没有确认记事本已创建窗口。

New-GuiStandInProcess 现等待同一个 Process 的非零 MainWindowHandle，每次 Refresh 并检查提前退出；30 秒内未就绪明确失败且仅清理自己创建的进程。正式安装脚本和关闭失败守卫未改变。替身就绪后再执行完整生命周期，不重跑原失败碰绿。修改后 PS7 契约连续三轮各 26 通过，完整脚本 UTF-8 语法解析通过；最新修复仍需两套 PR 检查。
