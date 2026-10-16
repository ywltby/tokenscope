# 审阅意见：来源目录、扫描反馈与 Workspace 统一计划

> 审阅对象：`docs/plans/active/2026-10-14-source-discovery-and-additional-paths.md`（分支 `docs/source-discovery-plan`，提交 `3092736`，基线 main `e34495b` / 0.3.1）。
> 审阅日期：2026-10-15（同日按用户第二轮决策修订：**移除多账户读取**，构建输入识别改用 Cargo 依赖信息文件）。本文是修改该计划的依据；用户决策（第 1 节）优先于计划原文。修改完成后按第 9 节逐条自查，再交复审。

## 0. 结论

计划对现状的描述与代码一致（`SourceConfig` 只有 `enabled`/`dir`、`claude_dir`/`codex_dir`、Codex 默认双根、自定义根禁用 Claude 映射、`COLLECT_SERIAL`、`usage-updated`），细节完整。但当前版本不能直接交给实施方：

- 用户决定**移除多账户读取**，计划中围绕账户枚举、账户映射和权限的设计要整体删除。
- 用户决定拆成 3 个 PR，计划结构要随之拆分。
- 另有 3 处缺口必须补上：识别编译进程序的非代码文件、目录遍历的死循环、嵌套与去重规则。

## 1. 用户决策（2026-10-15，计划必须照此修改）

| # | 决策 | 影响章节 |
| --- | --- | --- |
| D1 | **移除多账户目录读取**：只发现当前用户的标准目录和环境变量指定的目录，其余靠手动追加；不做账户枚举，也不请求管理员权限 | §1、§2、§3、§5.1、§6、S02、S04、S06、S07、§8 |
| D2 | 不新增重复同意流程 | §6 隐私条目、S07 |
| D3 | 拆成 **3 个 PR**：第一个 PR 改目录（workspace 迁移），之后再做具体功能 | 全文结构、执行顺序、版本、交付 |
| D4 | 用 **Cargo 依赖信息文件（`*.d`）** 自动识别像 `docs/privacy.md` 这样被 `include_str!` 编译进程序的文件，提交检查据此选择 | W05a、W05b |
| D5 | 同一应用**直接拒绝**添加已有目录的子目录或上级目录 | §5.1、§6、S01、S03、S05 |
| D6 | `CODEX_HOME` 的语义变化写进**行为变更说明**并配测试 | §3.1、§5、S02、S03、S07 |
| D7 | 读取动画**至少播放 1 秒**，同时用作“读取仍在进行”的可见心跳 | §11.2、F03、F04、DESIGN.md |
| D8 | 文件级去重：**先比对路径**，再处理**硬链接与符号链接**，重点关注性能 | §5.1、S03、S06 |
| D9 | **增加** Claude 环境变量支持 | §3.1、S02 |
| D10 | 测试命令按迁移后的 workspace 同步更新 | S01–S06、F01–F04 |
| D11 | 修复排版问题 | §11.3 |

## 2. 结构：拆成 3 个 PR（D3）

### 2.1 拆分方式

把现有一份计划拆成三份 active 计划，每份可以独立交给实施方，并在 `docs/plans/README.md` 登记：

| PR | 计划文件（建议名） | 范围 | 版本 | 分支 |
| --- | --- | --- | --- | --- |
| PR1 | `2026-10-15-workspace-layout.md` | W01–W07：虚拟 workspace、核心库迁入 `crates/tokenscope-core/`、单一锁、版本工具、产物路径、CI/缓存、暂存区驱动的提交检查（含 D4 的依赖信息文件），以及 c16e023 | patch | 由 `docs/source-discovery-plan` 切出或直接使用该分支 |
| PR2 | `2026-10-15-collection-status.md` | F01–F05：真实扫描状态与读取动画 | minor | PR1 合入后，从最新 main 切出 |
| PR3 | 保留现文件名，只含 S01–S07 | 当前用户默认目录、环境变量目录、追加目录列表、嵌套拒绝与文件级去重 | minor | PR2 合入后，从最新 main 切出 |

- PR2 排在 PR3 之前：F 组规模小，解决的是用户已经反馈的“看不出是否在读取”，也不依赖来源发现；两者都改 `report.rs` 的采集路径，串行合入可以避免互相冲突。
- 每个 PR 在合入前 fetch 最新 main，各自执行一次 `bump_version.py`。计划里不写死版本号（按当前 0.3.1 推算依次是 0.3.2 / 0.4.0 / 0.5.0，但以执行时的 main 为准）。
- 删除这些原文：“统一执行顺序”段、“S07、F05、W07 合并为一次收尾”、§10 开头的“S07 与 W07 合并交付”、§11.5 末尾的“统一交付”、F05 的“与目录发现功能同批发布取 minor”、§1 最后一条里对整体版本的描述。
- PR1 执行期间冻结其他 Rust 分支开发（`git mv` 会移动全部 Rust 源码）。计划须写明：开工时记录其他分支的基线，PR1 合入后其他分支先 rebase，再继续。
- PR2、PR3 的文件路径一律按迁移后的 `crates/tokenscope-core/...` 写（现文已经如此，保留），并写明“前置条件：PR1 已合入 main”。

### 2.2 PR1 中破坏性操作的门槛

W06 第 6–7 步清理旧 `target`，执行前必须向用户报告实际路径、大小、是否为链接、是否被运行中的进程占用，**得到用户确认后**才删除；不得在 PR 门禁脚本中自动删除。

## 3. 移除多账户读取，简化目录发现（D1、D2）

### 3.1 删除的内容

计划中以下内容全部删除，不保留“以后再做”的半成品接口：

- §1：“默认发现 `C:\Users\*` 所代表的本机用户目录”和“目录发现部分只覆盖 Windows 多用户发现”。Goal 改为“默认发现当前用户的标准日志目录与环境变量指定的目录”。
- §2 表格：“多用户发现必须使用该目录所属账户的映射，禁止借用当前账户”一行，改为第 3.2 节中“每个根绑定自己的映射”。
- §3.1 第 1–3 条：读取系统用户配置位置（ProfileList）、配置文件根目录的子目录、排除模板或公共目录、当前用户兜底、发现诊断。另外删除第 4 条中“不读取其他账户的进程环境、不加载注册表 hive”的表述（已无此场景）。
- §3.1 末段中针对其他账户的“无法访问/未确认”和“不得为了遍历其他账户自动请求管理员权限”。
- S02 测试项：两个用户、非 C 盘 profile、模板目录、账户发现部分失败。
- S03 测试项：“缺 cwd 的两个账户同 slug 不串映射”。
- S06：“注入两个合成 profile”“禁止枚举真实 Profiles”。
- §8 清单：“默认可发现两个合成用户的标准日志根”。
- §6 的 `docs/privacy.md` 条目中“读取范围扩展为本机可访问账户”。
- S07 的版本说明“多用户日志自动发现与追加目录管理”，改为“日志目录自动发现与追加目录管理”。
- 计划正文中的“账户”“profile 列表”等措辞，按新范围改写。

### 3.2 保留并简化的内容

1. **自动来源只有三类**：
   - 当前用户的标准根：`~/.claude/projects`、`~/.codex/sessions`、`~/.codex/archived_sessions`。
   - `CODEX_HOME` 指定的目录（见 6.1）。
   - `CLAUDE_CONFIG_DIR` 指定的目录（见 6.2）。

   手动追加目录、启停、移除、旧配置迁移的设计保留。
2. **建议取消“重新发现”按钮和 `source_paths_discover` 接口。**
   - 现在只需检查当前用户的几个固定路径，开销只是几次文件状态查询，可以在每轮采集开始时重新解析来源集合。新装工具后，它的目录会自动出现，用户不需要手动操作。
   - 这样可以一并删除 §3.2 中的发现时机规则、发现修订号、迟到响应丢弃，以及 S04 中“重新发现与定时读取交错”的测试。
   - 保留的约束：每轮采集冻结一份来源集合；采集复用键包含来源集合和映射。
   - 如果执行方认为必须保留按钮，要在计划里写明理由。
3. **每个根绑定自己的映射**：
   - 默认 Claude 根使用 `~/.claude.json`。
   - `CLAUDE_CONFIG_DIR` 根使用核实后的映射文件（见 6.2）。
   - 手动根禁用映射（沿用现有 A03 规则）。

   测试：默认根和环境变量根的同名 slug 不串用映射。
4. **权限拒绝状态保留**：手动目录或网络共享可能因权限被拒绝。“无权限”要如实显示，不能写成“缺失”。不请求提权。
5. **测试隔离保留**：主目录、环境变量和文件系统探测仍然做成可注入的提供者。测试和验收模式下不读真实主目录与真实环境变量（RC10）。
6. **隐私（D2）**：
   - 读取范围仍然只有当前用户自己的数据，不新增同意流程。
   - 如果 `docs/privacy.md` 列举了读取范围，要补上环境变量指定的目录和多个追加目录。
   - 这个文件通过 `include_str!` 编译进壳程序（`src-tauri/src/privacy.rs:21`，测试见同文件 `:1031`），修改它属于壳代码变更，见第 4 节。

## 4. 用依赖信息文件识别编译进程序的非代码文件（D4）

### 4.1 事实依据

本机现有 Cargo 依赖信息文件（`target/**/deps/*.d`、`src-tauri/target/debug/*.d`）已经列出这些非 Rust 输入：

- `docs/privacy.md`（壳，`include_str!`）
- `src-tauri/capabilities`、`src-tauri/tauri.conf.json`（Tauri 构建）
- `src-tauri/src/acceptance_csp.js`（壳，以 `.js` 结尾，但属于 Rust 构建输入）
- `tests/fixtures/cache/schema-v3.sql`（核心库）
- `tests/fixtures/codex/rate-limit-only.jsonl`（**壳的测试**通过 `../../tests/...` 引用核心库的 fixture，属于跨 crate 引用）

`.github/workflows/install-test.yml` 的 `paths` 中也已经手工列入了 `docs/privacy.md`。这说明这类依赖确实存在，而且目前靠人工维护。

### 4.2 方案：依赖信息文件生成清单，提交钩子读清单，CI 校验清单

依赖信息文件由编译器生成，是“哪些文件被编进了哪个包”的权威来源。但提交钩子不能为了判断该跑哪些检查先编译一遍，所以中间放一份**受版本管理的清单**：

1. **新增脚本** `scripts/build_inputs.py`，有两个模式：
   - `--update`：
     - 先对两个本地包执行 `cargo clean -p tokenscope -p tokenscope-app`，确保依赖信息文件是新生成的；
     - 再运行 `cargo test --locked --workspace --no-run`，以及加 `--features acceptance` 的同一条命令，取两种构建的并集；
     - 解析本次生成的 `*.d` 文件，以及本地包构建脚本输出里的 `cargo:rerun-if-changed=` 行，因为依赖信息文件不包含构建脚本的输入；
     - 滤除 `.rs`、`Cargo.toml`、`env-dep:`、仓库外路径和 `target/` 下的路径；
     - 把结果写成清单（例如 `scripts/build-inputs.json`），每项为“仓库相对路径 → 使用它的包”，按路径排序以保证内容稳定。
   - `--check`：在 CI 中用本次构建产生的依赖信息文件重新生成清单，与已提交的清单比较，不一致就失败，并列出多出和缺少的路径。
2. **CI 防过期**：
   - CI 的 Rust 缓存（rust-cache）不缓存 workspace 成员自身的产物，所以 CI 中两个本地包的依赖信息文件总是新生成的。
   - 脚本仍需防御：只读取本次运行开始之后写入的 `*.d` 文件，避免旧文件混进来。
3. **提交钩子读清单**：
   - 暂存区中出现清单里的文件时，按对应的使用方选择检查。命中壳就跑壳的检查；命中核心库就跑 workspace 检查，因为壳依赖核心库。
   - 清单缺失或无法解析时，退回完整模式。
   - 清单本身被修改时，也走完整模式（它属于门禁入口）。
4. **前端范围**：依赖信息文件不覆盖前端。已核实前端目前没有读取 `frontend/` 之外文件的测试或导入（`App.test.ts` 中的政策文本是手写副本，不读文件），本轮不做前端扫描。清单允许手工登记条目，供以后前端出现跨目录读取时使用。但前端测试会读 `frontend/public/theme-boot.js`（`src/composables/theme.test.ts:150`），所以 W05a 的前端规则要覆盖 `frontend/public/**`，不能只写 `frontend/src/**`。
5. **路径过滤对齐**：`--check` 同时检查各工作流的 `paths` 是否覆盖清单中的全部路径，取代 `install-test.yml` 中手写的 `docs/privacy.md`。
6. **分类表修改**：
   - W05a 的“仅 `.md` 文档”规则改为“未出现在构建输入清单中的 `.md`”。
   - `src-tauri/src/**` 规则改为“`src-tauri/src/**` 下的全部文件”，因为其中有 `.js`。
   - 核心库 `tests/**` 变化继续跑 workspace 测试（原计划已覆盖），因为壳的测试引用了核心库的 fixture。
7. **开发者流程**：新增或改动 `include_str!` 一类引用后，运行 `--update` 并一起提交清单。忘记更新时 CI 的 `--check` 会失败。把这一步写进 CLAUDE.md 的提交流程。
8. **测试**：W05b 增加以下用例：
   - 只暂存 `docs/privacy.md`：必须选中壳检查。
   - 只暂存 `src-tauri/src/acceptance_csp.js`：必须选中壳检查。
   - 只暂存核心库 fixture：必须选中 workspace 测试。
   - 清单缺失或损坏：退回完整模式。
   - 依赖信息文件里出现清单未登记的路径、清单里有已不存在的路径：`--check` 都失败。
   - 旧的依赖信息文件不会被 `--check` 读入。

## 5. 目录嵌套、路径身份与文件级去重（D5、D8）

### 5.1 嵌套规则（D5）

- 用户手动添加时，同一应用中**与任一已有行相同、是其子目录或是其上级目录**都直接拒绝，错误信息写明冲突的那一行。“任一已有行”包括自动发现、环境变量、手动添加三种来源，已停用的行，以及目录当前不存在的行。比较同时使用规范化路径和物理身份（可访问时解析联接和符号链接）。与已有行完全相同的情况保留原规则：返回已有行，提示“目录已在列表中”。跨应用的相同或嵌套继续拒绝（SF09）。
- 删除原文中“同应用父子根保留配置行，子根先认领，父根跳过……”、状态“被子根覆盖”以及“列表注明覆盖关系”。
- **非用户点击产生的嵌套**无法拒绝，要定义确定的处理：
  - 迁移旧 `dir` 时：例如旧 Codex `dir = ~/.codex`，正好是自动根 `sessions`、`archived_sessions` 的上级。
  - 环境变量指向其他根的内部或上级时。
  - 处理要求：迁移和自动结果不阻断。重叠的行标为“与 X 重叠”，提示用户移除其中一行；读取时靠第 5.3 节的文件级去重保证每个文件只解析一次。迁移时出现的重叠，在迁移说明里一次性告知用户。
  - S01、S03 为每种情形各补一个测试。

### 5.2 现有遍历的缺陷（必须修，且属于 PR3）

`src/source/mod.rs::walk_jsonl` 用 `child.is_dir()` 判断子目录。这个调用会**跟随**符号链接和联接，而且没有记录已访问目录。一个指向上级目录的联接会导致无限递归，最终栈溢出。

要求：

- 改用不跟随链接的 `DirEntry::file_type()` 判断类型。
- 遇到目录链接时解析目标，记录已访问目录的物理身份：目标已访问过或在另一个已启用根内，就跳过并在 debug 日志记一条；否则只跟随一次。
- 补一个“联接指向上级目录”的回归测试，修复前应当失败。

### 5.3 去重分层（D8：先比路径，再处理链接）

1. **路径层（每轮必做、开销最低）**：用规范化的绝对路径做键。
   - 处理分隔符、盘符大小写和 Windows 不区分大小写的比较。
   - `std::fs::canonicalize` 在 Windows 上返回 `\\?\C:\...` 形式，要统一成同一种写法，否则同一路径会比不上。
   - 8.3 短文件名（如 `PROGRA~1`）要还原成长文件名。
   - 根目录的规范化与物理身份只在**来源集合变化时**计算并缓存，不在每轮对未变的根重复计算。
2. **符号链接与联接层**：按 5.2 处理目录。文件级的符号链接解析到目标文件，用目标的身份去重。
3. **硬链接层**：用卷序列号加文件 ID 作为身份。
   - 优先取 128 位的 `FILE_ID_INFO`，因为 ReFS 下 64 位文件索引不唯一；取不到时退回 64 位文件索引。
   - FAT/exFAT、网络共享等没有稳定文件 ID 的位置退回路径层，并把身份标为“未确认”。
   - 取文件 ID 需要打开文件句柄。要求**按（路径、大小、修改时间）缓存文件 ID**，只对新增或变化的文件打开句柄，不得每轮对全部文件重取。
4. **确定的认领者**：同一物理文件有多个路径时，取规范化路径最小的那一个解析。解析进度按物理身份记录，认领路径变化时不从头重解析。可选优化：Codex 会话从 `sessions` 移到 `archived_sessions`（同一卷）时文件 ID 不变，可借此避免重解析；做与不做都要写明。
5. **性能验收（必须写进 S03/S06）**：
   - `perf_real_data` 实测（release）：热启动仍在现有 0.1–0.2 秒量级，相对改动前不超过 +10%；冷启动（约 5.5 秒）不超过 +10%。改动前后的数字都记进计划。
   - 合成基准：2 个根、约 2 万个文件、约 1 千个硬链接、1 个联接环。记录每轮热采集耗时与句柄打开次数；第二轮未变化时句柄打开次数应接近 0。
   - 每轮重新解析来源集合（见 3.2 第 2 条）的耗时单独记录，应在毫秒级。
   - 新增“来源解析/文件身份”的阶段耗时。自动触发时按 debug 级别输出，手动和启动时按 info，符合 CLAUDE.md 的 U06 日志规约。

## 6. 环境变量（D6、D9）

### 6.1 `CODEX_HOME` 行为变更（D6）

- 现状（`src/source/codex.rs:60-77`）：`CODEX_HOME` 非空时**替代** `~/.codex`，`~/.codex` 不再读取。
- 新行为：`CODEX_HOME` 的 `sessions`、`archived_sessions` **追加**到默认根，`~/.codex` 照常读取。
- **用户可见影响**：升级前设置过 `CODEX_HOME` 的用户，如果 `~/.codex` 里还有旧日志，升级后历史总量可能增加。这一条必须写进 `docs/stats-semantics.md` 的来源一节、PR 描述的“行为变更”一节，以及 `bump_version.py --summary`。
- **测试**（S02/S03）：
  - 已设置且两处都有日志：两行，用量取并集，不重复计。
  - `CODEX_HOME` 与 `~/.codex` 实际相同（大小写不同、带尾分隔符、经联接指向）：合并成一行，来源同时标“自动发现”和“环境变量”。
  - 值为空字符串：忽略（与现状一致）。
  - 相对路径：现状会按当前工作目录解析，新行为改为忽略并给出诊断。这也是一项行为变更，一并写明。
  - 指向不存在的目录：显示为“缺失”，不反复报错。
  - 验收模式：完全忽略环境变量（保持 RC10 隔离优先）。
  - 指向其他根的内部或上级：按 5.1 标为重叠。
  - 升级测试：旧版只读 `CODEX_HOME`，新版两处都读，事件不重复。

### 6.2 Claude 环境变量（D9）

- 增加 `CLAUDE_CONFIG_DIR`：读取 `<CLAUDE_CONFIG_DIR>/projects`，来源标“环境变量”，与默认根合并去重，语义与 `CODEX_HOME` 对称。
- **实施前要核实**：`CLAUDE_CONFIG_DIR` 下项目映射文件的位置（是否为 `<CLAUDE_CONFIG_DIR>/.claude.json`）。以 Claude Code 官方文档或合成实测为准，结论写进计划。核实不了时，该行禁用映射（与手动根相同），不能猜。
- 删除 §3.1 第 5 条“本轮不新增未经项目验证的 Claude 环境变量约定”。
- 测试项与 6.1 对称：并集、相同目录合并、空值、相对路径、不存在、验收模式忽略、重叠。
- 两个环境变量共同的说明：只采用 TokenScope **启动时**看到的值。开机自启的进程只继承注册表里的用户环境变量，看不到只在 shell 配置中设置的变量。修改后需要重启应用。设置页的提示和文档要写明这一点。

## 7. 扫描状态与读取动画（D7）

- §11.2 表格中“采集结束，无新用量：立即停止读取动画”改为：**从开始算起至少显示 1 秒；真实读取超过 1 秒的，到实际结束时停止**。
- 删除 §11.2 中的“不设置最短播放时长；快速扫描允许动画短暂出现”，改为：最短 1 秒只在前端计时，**不得**拖延后端采集、不得占用串行锁、不得推迟下一轮调度、不得推迟统计刷新。有新用量时，汇总和明细照常立即刷新，“刷新中…”状态照常优先显示。
- 显示期内又收到新一轮的开始事件时，动画连续播放，不重新起播，避免抖动。
- 减少动态效果偏好下，改为显示静态读取图标和“正在读取”文字，同样至少 1 秒。窗口隐藏时不播放，恢复后不补播。不朗读开始和结束（原规定保留）。
- 心跳语义写进计划：读取正常时，状态区大约每“间隔 + 1 秒”出现一次至少 1 秒的读取动画，长时间不出现即说明读取停了。**建议**（非必须）：状态数据带上最近一次完成读取的时间，悬停提示显示“最近读取 N 秒前”，读取间隔调长时也能判断。
- **DESIGN.md 同步**：0.3.1 已写入“不用……闪烁或脉动光效”。要在其后加入明确豁免：状态区读取图标旋转（不超过 16px，中性色或强调色，无发光），每次至少 1 秒，只由真实读取触发，不属于装饰性闪烁。否则实施后与设计规范冲突。
- **测试**（F03/F04）：
  - 用可控时钟：50ms 完成的读取，动画显示不少于 1000ms，并在约 1000ms 时结束。
  - 3 秒的慢读取：动画在真实结束时停止。
  - 1 秒内连续两轮：动画不中断。
  - 有新用量：查询不等动画结束就发出。
  - 减少动态效果偏好下：显示静态图标，同样不少于 1 秒。

## 8. 测试命令与排版（D10、D11）

PR1 合入后 workspace 根目录是虚拟 manifest，命令统一改为：

| 位置 | 原命令 | 改为 |
| --- | --- | --- |
| S01 | `cargo test -p tokenscope --test source_paths_settings` | `cargo test --locked -p tokenscope --test source_paths_settings` |
| S02 | `cargo test -p tokenscope --test source_discovery` | `cargo test --locked -p tokenscope --test source_discovery` |
| S03 | `cargo test -p tokenscope --test source_paths_collection` 与 `--test ui_adjustment_collect` | 两条都加 `--locked`，并保留 `-p tokenscope` |
| S04 | `cargo test --manifest-path src-tauri/Cargo.toml` | `cargo test --locked -p tokenscope-app` |
| F01 | `cargo test --locked --test ui_adjustment_collect` | `cargo test --locked -p tokenscope --test ui_adjustment_collect` |
| F02 | `cargo test --locked --manifest-path src-tauri/Cargo.toml` | `cargo test --locked -p tokenscope-app` |
| W05 构建输入清单 | 无 | `python -B scripts/build_inputs.py --check`（CI 在 workspace 构建之后运行）及其单测 |
| 各组收尾 | 零散命令 | `cargo fmt --all -- --check`；`cargo clippy --locked --workspace --all-targets -- -D warnings`；`cargo test --locked --workspace`；再分别加 `--features acceptance` 跑 clippy 和 test；前端 typecheck、format:check、test、build、test:e2e |

- W07 中“`--manifest-path` 本身仍合法”这一句保留，但计划正文不再使用这种写法，与 workspace 门禁保持一致。

排版：

- §11.3 下的 `#### 3.1 真实边界与单飞`、`#### 3.2 Tauri 接线与恢复` 改为 `11.3.1`、`11.3.2`。拆分后，在 PR2 计划内按新章节号重新编号。
- 拆分后检查每份计划的锚点（如 `<a id="collection-feedback">`）和 README 链接都还能打开。

## 9. 修改后自查清单（交复审前逐项勾选）

> 2026-10-15 执行方回填：以下勾选只表示要求已落实到三份计划，不表示功能、CLAUDE.md/DESIGN.md 或测试已实施；原审阅判断保留，仍待独立复审。

- [x] 三份计划各自独立：前置条件、分支、版本等级、门禁、PR 与验收都写清；README 已登记；原文中统一交付和合并收尾的表述已删除。
- [x] 第 3.1 节所列的多账户内容已全部删除，正文中不再出现账户枚举、ProfileList、提权和其他账户映射；Goal 与版本说明已改写。
- [x] 自动来源只剩当前用户标准根和两个环境变量根；“重新发现”按钮已取消，或写明保留的理由；每个根绑定自己的映射；测试隔离保留。
- [x] `docs/privacy.md` 只按实际读取范围补充说明，不新增同意流程，并标明该文件属于壳的构建输入。
- [x] W05a/W05b/W05c 已加入 `scripts/build_inputs.py` 的 `--update`/`--check`、受版本管理的清单、钩子读清单、CI 校验与防过期、`paths` 对齐、分类表修改，以及第 4.2 节第 8 条列出的用例；已安排在实施时将“改动引用后更新清单”写入 CLAUDE.md 提交流程。
- [x] 嵌套规则已改为直接拒绝，覆盖比较已停用和目录不存在的行；迁移和环境变量产生的重叠有确定的处理和测试；“被子根覆盖”等原文已删除。
- [x] `walk_jsonl` 跟随链接导致死循环的问题已列为 PR3 必修，并配先失败的回归测试。
- [x] 去重按路径、链接、硬链接三层实现，并写明文件 ID 缓存、确定的认领者、无稳定 ID 时的退回方式，以及性能预算与合成基准。
- [x] `CODEX_HOME` 行为变更说明的位置、全部测试，以及相对路径语义的变化已写入。
- [x] `CLAUDE_CONFIG_DIR` 已加入，映射文件位置标为“实施前核实”，测试与 `CODEX_HOME` 对称；“只采用启动时的环境变量”的说明已写入。
- [x] §11.2 已改为至少 1 秒，并写明不拖延采集与刷新、连续播放、减少动态效果偏好、测试，以及 DESIGN.md 中的豁免条文。
- [x] 第 8 节的命令全部替换，排版已修。
- [x] 删除旧 `target` 改为执行时报告并经用户确认。

## 10. 执行方修订说明（待复审）

- [PR1](../plans/archive/implemented/2026-10-15-workspace-layout.md)：独立 patch，W01–W07 和 c16e023；保留目录迁移、新旧版本基线、全部门禁，新增 W05c 依赖信息清单。旧 target 必须报告路径、大小、链接、占用并取得用户确认后删除。
- [PR2](../plans/active/2026-10-15-collection-status.md)：PR1 合入后独立 minor，F01–F05；至少 1 秒显示、CSS 图标、查询不等动画、连续轮次及减少动态效果均有用例，安排 DESIGN.md 豁免。
- [PR3](../plans/active/2026-10-14-source-discovery-and-additional-paths.md)：PR2 合入后独立 minor，S01–S07；只剩当前用户标准/环境/追加根，取消重新发现，保留 settings 事务、防环、三层去重、性能预算及环境变更说明。
- 技术细化 1：编译后的 `--check` 不能用自己的启动时间筛 `.d`；增加构建前 `--begin` run 边界和每模式产物记录。明确包清理/隔离构建与完整性检查，不依赖缓存动作的内部实现假设；旧依赖信息不能混入，缺证据不得空集合通过。
- 技术细化 2：实际心跳周期是配置间隔＋真实采集耗时，最短 1 秒只影响显示，不改调度；缺少可见动画本身不足以证明后端停止。失败/状态不可用立即显示，正常短采集满足 1 秒。
- CLAUDE_CONFIG_DIR 映射位置仍为实施前核实项；未把猜测写成事实。真实性能数字仍是预算/历史参考，尚无本次实现基准。

## 11. 复审意见（针对 4ef146e）

> 复审日期：2026-10-15。对象：[PR1](../plans/archive/implemented/2026-10-15-workspace-layout.md)、[PR2](../plans/active/2026-10-15-collection-status.md)、[PR3](../plans/active/2026-10-14-source-discovery-and-additional-paths.md)、`docs/plans/README.md`、UI 调整计划顶部说明。

**结论：** 第 9 节各项均已落实到三份计划，多账户相关内容已删净（全文检索 ProfileList、提权、账户映射、`source_paths_discover` 均无残留），旧锚点 `#collection-feedback` 已无引用。执行方新增的两条技术细化（构建前 `--begin` 划定证据边界；心跳周期 = 间隔 + 真实耗时，1 秒只影响显示）判断正确，予以采纳。改完下面 11.1、11.2 两处即可开工 PR1；11.3 在 PR3 开工前改好即可。

### 11.1 清单的消费方要区分普通与 acceptance 构建（PR1 W05a/W05c，必改）

- 事实：`src-tauri/src/acceptance_csp.js` 由 `src-tauri/src/acceptance.rs:104` 的 `include_str!` 引入，整个模块受 `#[cfg(feature = "acceptance")]` 控制。本机现有依赖信息文件中，壳的普通构建只列出 `docs/privacy.md`，带 acceptance 的构建才出现 `acceptance_csp.js`。
- 问题：W05c 的清单只记“消费方集合”（包名）。若 `acceptance_csp.js` 只登记为 `tokenscope-app`，钩子会跑壳的普通 clippy/test，而普通构建根本不编译这个文件，改动等于没验证。
- 要求：
  1. 清单的消费方改为“包 + 构建模式”（例如 `tokenscope-app@acceptance`），钩子按模式选择对应命令。
  2. W05a 表中“涉及验收模块时加 acceptance”判断条件不明确，改成确定的规则。建议最简单的写法：`crates/tokenscope-core/**` 和 `src-tauri/src/**` 的 Rust 改动，普通和 acceptance 两种模式都跑 clippy/test。W05b 第 6 步实测耗时；如果太慢，再写出明确的缩减规则并配测试。
  3. 必要契约中加入用例：`acceptance_csp.js` 单独改动必须选中壳的 acceptance 检查。

### 11.2 清单只收受版本管理的文件（PR1 W05c，必改）

- 事实：本机 release（custom-protocol）构建的壳依赖信息文件，列出了 6 个 `OUT_DIR/tauri-codegen-assets/<哈希>.js|css|html`，以及 `frontend/dist/assets/*-<哈希>.js|css`。`frontend/dist` 在仓库内、不在 target 下，现有排除规则（仓库外、target/build-dir 内）挡不住；它的文件名带哈希，每次前端构建都会变。
- 问题：`--update`/`--check` 目前只用 `cargo test`（不带 custom-protocol），暂时不会读到这些文件。但只要以后某个模式或 CI 步骤带上 custom-protocol，清单就会每次变动，`--check` 结果也会不稳定。
- 要求：
  1. 清单条目只保留 Git 跟踪的文件，或含有被跟踪文件的目录（用 `git ls-files` 判断）。未跟踪和被忽略的路径排除，写入证据目录并列出。
  2. 计划写明清单覆盖范围：`cargo test` 的普通和 acceptance 两种模式、不带 custom-protocol。`frontend/dist` 由前端规则覆盖。
  3. 依赖信息里的路径形如 `src-tauri\src\../../docs/privacy.md`。规范化时要先按字面折叠 `..`，再转成仓库相对路径。用例覆盖这种写法，以及一份 release 风格的 `.d` fixture，断言生成产物被排除。

### 11.3 旧 `dir` 迁移后开启自动发现也是行为变更（PR3 §2、§3、S07，PR3 开工前改）

- 事实：现在 `make_source`（`src/report.rs:241`）的逻辑是：设置了自定义 `dir`，就只读这个目录，默认根不读。按 PR3 §3，旧的非空 `dir` 迁移为追加项后，自动发现会开启，默认根也开始读取，统计总量可能增加。这与 `CODEX_HOME` 从“替代”改为“追加”属于同一类变化。
- 要求：把这条和 `CODEX_HOME` 一起写进 §2 的“行为变更”、`docs/stats-semantics.md` 来源章节和 S07 的版本摘要；S01 增加“旧配置有自定义 dir 时，升级后默认根的用量被并入”的升级用例。本机 `settings.toml` 没有设置 `dir`，不受影响，但计划要覆盖其他升级用户。

### 11.4 建议（不阻塞）

- PR1 实施要改目录、构建和门禁，不再是纯文档改动。建议从当前分支头新建 `feat/workspace-layout`（或 `build/workspace-layout`），c16e023 和三份计划随之带入，之前的提交不用改写。这样避免再次出现“实现落在 docs 分支”的情况（控件材质那次已经发生过）。README 和 PR1 页首的分支名同步修改。
- PR3 §4.3 已选择实现“sessions → archived_sessions 移动后复用解析进度”。这是第 5.3 节中的可选项，可以保留；如果实施时发现它会改动历史库按路径记录进度的结构，可以拆出本轮，但计划里要写明“不做”。

## 12. 第 11 节修订回填（仅计划，待复审）

- [x] 11.1：[PR1](../plans/archive/implemented/2026-10-15-workspace-layout.md) W05a/W05c 的消费方改为包＋构建模式，普通与 acceptance 分别保留、选命令；核心与壳 Rust 改动固定跑两模式。W05b 增加分项耗时，契约明确 acceptance_csp.js 单独改动不能只跑普通壳测试。
- [x] 11.2：清单限定 Git 跟踪文件及含跟踪文件的目录，未跟踪/生成路径及排除原因进入 QA 证据。明确两种 cargo test 模式均不带 custom-protocol；字面折叠路径后再判断仓库边界，补 Windows/UNC/越界、release 风格生成资源及 Git 跟踪边界契约。update/check 使用同一规则。
- [x] 11.3：[PR3](../plans/active/2026-10-14-source-discovery-and-additional-paths.md) §2/§3/S07 写明旧自定义 dir 由替代变追加、默认根新增用量可能提高总量；安排同步 stats-semantics 与升级提示。S01 增加升级并集、重复请求和停用用例，完整采集断言在 S03 接通后验证。
- [x] 11.4：采纳实施分支建议，PR1 与 README 改为开工时从当前文档分支最新提交创建 feat/workspace-layout。本轮不切分支，不改写提交。归档移动复用继续保留在 PR3 本轮范围与验收中，未静默延期。

以上只表示设计已修改；未实施脚本、业务代码或测试，也未把各任务标为完成。

## 13. 第二次复审（针对 9a8b0c9、9d8942a）

**结论：通过，可以开工 PR1。** 第 11 节四项均已落实：

- 消费方改为“包 + 模式”，`acceptance_csp.js` 单独改动必须选中壳 acceptance 检查。
- 清单只收 Git 跟踪文件，先折叠 `..` 再判断是否越出仓库，并增加 release 风格 fixture。
- 旧 `dir` 迁移的行为变更已写入 §2、§3、S01、S07。
- 实施分支改为 `feat/workspace-layout`。

新增的 PR1 第 4–5 节、PR2 第 6 节、PR3 第 8 节逐项核对，结论如下：

- 引用的 `AGENTS.md`、`scripts/test_install_helpers.ps1` 存在，`qa-artifacts/` 已在 `.gitignore` 中。
- 本地门禁命令 `python -B -m unittest discover -s scripts -p "test_*.py"` 覆盖了 CI 分别执行的全部 7 个脚本测试文件，本机实跑 74 项通过（35.6 秒），都是临时目录契约测试，没有真实安装。
- README 中的四个跨文件锚点与目标标题一致。
- PR2 的状态机和计时规则与现有 `collect_flighted_with`、`COLLECT_SERIAL`、`Trigger` 的结构相符：start 在取锁后发出，终态在释锁前写入，同一 runId 不延长显示期。
- PR3 的事实矩阵覆盖了第 5 节全部要求。

**唯一的小矛盾（不阻塞，执行 W05a 时一并改）：** 核心 fixture 中只有 `tests/fixtures/cache/schema-v3.sql` 是编译期输入（`src/cache.rs:742`、`tests/history_migration.rs:544` 中的 `include_str!`），会进入清单。其余 fixture（包括壳测试跨 crate 读取的 `codex/rate-limit-only.jsonl`）都是运行时读取，不在清单里。

按新分类表“非 Rust 文件未登记者完整兜底”，这些 fixture 改动会走完整模式，与“必要契约”中“核心 fixture 变更选 workspace”的说法不一致。请在分类表加一条明确规则：`crates/tokenscope-core/tests/fixtures/**` → workspace 两种模式的 clippy/test（workspace 已包含壳，跨 crate 引用随之覆盖），并让契约与之一致。保持现在的完整兜底也安全，只是更慢，二选一写明即可。

## 14. 第 13 节执行方回填（仅计划）

选择 fixtures 专用目录规则：`crates/tokenscope-core/tests/fixtures/**` 固定运行 workspace 普通与 acceptance 两种模式 clippy/test，覆盖核心及壳。W05a 分类表、优先级和 §5.3 契约同步；目录内 Markdown 不享受纯文档豁免，新旧路径均参与重命名分类。清单损坏/门禁入口仍完整兜底，多类影响取并集。

W02 与 W05c 同步区分编译期嵌入和运行时读取：schema-v3.sql 按实际依赖进入清单，运行时 JSONL 不伪造登记，靠目录规则覆盖。三份计划及 README 状态同步为复审通过、实现未开始。本轮未创建实施分支、未执行 W01。

## 15. PR1 实现复核（feat/workspace-layout，HEAD 8a9652b）

> 复核日期：2026-10-15。对象：`e190686..8a9652b` 共 21 个提交。复核方在本机按计划第 4.2 节命令表逐条重跑，并独立核对迁移、锁文件、钩子选择和清单证据。

**结论：** 结构迁移、单锁、版本工具、产物路径、CI 精简和按暂存区选择检查都已按设计落地，质量好。但本机门禁有 1 条失败（15.2 第 1 项），且同一个检查脚本在仓库上实际没有检查任何文件（15.2 第 2 项）。修好这两项后可以创建 PR。远程 CI、安装升级和 Native UI E2E 仍要等 PR 创建后才有证据，目前不能称“合并就绪”。

### 15.1 已核实通过

| 项目 | 复核方法 | 结果 |
| --- | --- | --- |
| 源码与测试迁移 | `git diff -M e190686 HEAD -- src tests crates` | 全部文件 100% 原样重命名，唯一新增是核心 `Cargo.toml`，没有夹带业务改动 |
| 锁文件 | 用 tomllib 比较旧壳锁、旧根锁和新根锁的 (name, version, source, checksum, deps) | 第三方包零差异；旧根锁的包都包含在内；只有两个本地包从 0.3.1 变为 0.3.2 |
| workspace | `cargo metadata --offline --locked` | 两个成员，两个默认成员，target 为根目录 `target/`；只有一个受版本管理的 Cargo.lock |
| 清单内容 | 对照本机两种模式的依赖信息文件 | 5 项与实际编译输入一致：`docs/privacy.md`、`acceptance_csp.js`（仅 acceptance）、`schema-v3.sql`、`tauri.conf.json`、`capabilities/` |
| 钩子选择 | 用临时 `GIT_INDEX_FILE` 暂存 5 类改动，运行 `--explain` | 纯文档只跑文档检查；`privacy.md` 跑壳两种模式加文档检查；`acceptance_csp.js` 只跑壳 acceptance；fixture 跑 workspace 两种模式；`theme-boot.js` 跑前端四项。未暂存差异拦截行为正确 |
| 清单反例 | 读取 `W05/build-inputs-check-negative2.txt` | 漏登记和 `install-test.yml` paths 未覆盖都被检出，退出码非零 |
| 测试环境隔离 | 检查会建临时 Git 仓库的 3 个测试模块和本仓 `.git/config` | 3 个模块都在加载时清除 `GIT_*`；`.git/config` 已无 `bare`，也无被覆盖的身份 |
| 本地门禁 | 按第 4.2 节命令表逐条执行 | fmt、两种模式的 clippy/test、`test_install_helpers.ps1`、前端 typecheck/format:check/test/build/test:e2e、`bump_version.py --check` 全部 exit 0；**脚本契约 216 项中 2 项失败**，见 15.2 |

### 15.2 必改（创建 PR 前）

1. **PowerShell 语法检查的测试依赖控制台编码。**
   - 复现：`test_script_checks.PowerShellSyntaxCase` 的两个用例用 UTF-8 解码 pwsh 的输出，而 `check_powershell_syntax.ps1` 按控制台编码输出中文。复核方的 PowerShell 会话编码是 utf-8，用例通过；从 Git Bash 发起时 pwsh 的 `[Console]::OutputEncoding` 是 gb2312，两个用例失败。
   - 影响：提交钩子经 `sh → python → pwsh` 调用这组测试。从 Git Bash、IDE 或其他控制台编码不同的环境提交时会误红。GitHub Windows Runner 的默认编码也未验证。
   - 要求：脚本开头固定 `[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)`，测试保持 UTF-8 解码。补一个用例：在 gb2312 环境（例如先执行 `chcp 936`，或在子进程中显式设置控制台编码）下运行仍然通过。

2. **零个脚本也算通过，仓库范围的用例实际检查的是空目录。**
   - `test_repository_scripts_are_syntactically_valid` 的注释写“默认以仓库根为范围”，但它调用的 `run_check()` 总是传 `-Root self.root`（空临时目录），输出是“通过：0 个脚本”。
   - `check_powershell_syntax.ps1` 和 `check_node_syntax.py` 都允许 0 个文件时 exit 0。目录改名或路径解析失败时，检查会静默变成空检查。
   - 要求：两个检查器都在检查数为 0 时失败。仓库范围的用例不传 `-Root`，并断言检查数等于仓库实际脚本数（当前 PowerShell 11 个，`frontend/scripts` 下 node 脚本 14 个）。

### 15.3 建议（不阻塞，可在本 PR 一并处理）

1. **库目标本身的依赖信息文件没有被读到。** `dep_files_from_messages` 用 `with_suffix(".d")`，把 `libtokenscope-<hash>.rlib` 映射成了 `libtokenscope-<hash>.d`，但 Cargo 实际写的是 `deps/tokenscope-<hash>.d`。复核方读取 `pr1-negative2` 证据确认：两个本地库的 rlib/rmeta 都映射到不存在的文件。当前清单没有出错，因为库的单元测试二进制编译同一份源码，它的 `.d` 兜住了。但出现 `#[cfg(not(test))]` 下的 `include_str!` 或 `test = false` 时就会漏登记。建议映射时去掉 `lib` 前缀，补一条只有 rlib 产物的用例，同时修正模块文档字符串中的错误示例。
2. **人工登记项会被 `--check` 判为过期。** `check()` 拿 `generated + manual` 和本次生成结果比较，任何人工项都会进入 `stale`，人工分区实际不能用。目前人工项为空，不影响结果；建议人工项单独校验（路径存在、受跟踪、消费方合法），不参与生成项比较。
3. **未跟踪文件不在部分暂存保护范围内。** `unstaged_paths` 只读 `git diff`。例如暂存的 `lib.rs` 新增了 `mod foo;`，而 `foo.rs` 还没有 `git add`，本地编译能通过，提交里却缺文件，要到 CI 才失败。建议把 `git ls-files --others --exclude-standard` 中落在所选检查范围内的路径也纳入拦截。
4. 计划 §6.7 仍写“钩子显式 unset `GIT_*`”，§6.8 已改为由测试侧隔离，需同步。`--check` 失败时直接抛出 Python 堆栈，建议改为输出清晰的差异信息并以非零退出。
5. CI 的清单校验没有采用 `--begin` 边界后复用两次构建的方案，而是自己 `clean -p` 后重新编译两次。正确性没问题，代价是 CI 多两轮本地包编译。可以接受，请在计划中写明这一偏差。

### 15.4 未闭合项（如实保留）

- 远程 CI、安装升级和 Native UI E2E 尚无证据（按计划 §6.1 记载的用户决定，暂不创建 PR）。
- 旧 `src-tauri/target`（38.85 GB）按用户决定暂缓清理，空间尚未释放。

## 16. 第 15 节修复回填

- 15.2 编码：PowerShell 检查器开头固定 UTF-8；新增显式 GBK 子进程的成功 stdout、失败 stderr 回归，Python 严格 UTF-8 解码。已在 Git Bash 路径复验，未改用容错解码隐藏问题。
- 15.2 空检查：两个检查器零文件均非零退出；仓库测试不传临时 Root，按仓库真实文件集合核对数量，不再只匹配一个数字。
- 15.3.1：rlib/rmeta 映射移除库产物 `lib` 前缀；只有库产物的回归通过，普通二进制名称不误改。
- 15.3.2：生成项单独比较，人工项单独校验路径、类型、跟踪范围与消费方，并参与工作流覆盖检查；人工项不能掩盖生成项缺失。
- 15.3.3：未跟踪非忽略文件进入输入保护，按所选检查范围拦截；纯文档与 ignored 输出仍不误阻。
- 15.3.4：修正计划 §6.7 的历史加固描述；CLI 重复模块加载导致异常类型不一致的问题已修复，缺清单报错不再输出堆栈。
- 15.3.5：计划 §6.9 明确记录 CI 清单校验会额外重编译两轮的代价，保留现有正确性策略。

修复提交 `0cbbc8b`：完整提交门禁通过，脚本契约 228 项、前端 480 项、浏览器 E2E 178 断言通过；真实清单两模式重编译后无差异。Git Bash 下语法契约 10 项通过，真实检查 PowerShell 11 个、Node 14 个。详细结果见计划 §6.9。

本轮不创建 PR、不删除旧 target；修复代码不等于远程安装升级已验收，仍待独立复审。

## 17. 第 16 节修复复核（HEAD 518fef7，修复提交 0cbbc8b）

结论：第 15 节 2 项必改、5 项建议全部闭合，本地实现层面可以创建 PR。远程 CI、安装升级与 Native UI E2E 仍无证据，合并就绪要等 PR 门禁结果。

### 17.1 逐项核实

| 项 | 复核方式 | 结果 |
|---|---|---|
| 15.2 编码 | 在原失败环境（Git Bash 调用 pwsh）复跑 `python -B -m unittest discover -s scripts -p "test_*.py"` | 228 项全部通过，原 2 项失败消失 |
| 15.2 空检查 | 对空临时目录分别运行两个检查器 | PowerShell 与 Node 检查器均输出「拒绝空检查」并 exit 1；仓库范围实测 11 个 / 14 个，exit 0 |
| 15.3.1 库 `.d` 映射 | 用 `review15/manifest/cargo-default.json` 的真实 Cargo 消息调用 `dep_files_from_messages` | 已收集库目标 `tokenscope-990f6a2dc04b6829.d` 与壳库 `tokenscope_tauri_lib-*.d`；磁盘上不存在带 `lib` 前缀的 `.d`，映射方向正确 |
| 15.3.2 人工登记 | 读 `check`/`update`/`validate_manual` 与回归用例 | 生成项单独比较，人工项单独校验，不能掩盖漏登记；工作流覆盖检查包含人工项 |
| 15.3.3 未跟踪文件 | 读 `unstaged_paths`/`_blocked_inputs` | 只有命中所选检查范围才拦截，忽略目录不参与；当前工作区无未跟踪文件，不会误阻 |
| 15.3.4 堆栈与 §6.7 | 用不存在的清单路径运行 `build_inputs.py --check` | 只输出一行「构建输入清单失败：…」，exit 1，无 Traceback；§6.7 已改为历史描述 |
| 15.3.5 CI 重编译 | 读计划 §6.9 | 已明确记录额外两轮编译的代价，接受 |

补充：仓库范围用例的期望数量与检查器使用同一套 glob 推导，不是独立事实源；但有 `assertGreater(expected, 0)`，目录改名或路径解析出错导致的空检查都会失败，满足 15.2 的目的，不要求改成硬编码数字。

### 17.2 新发现：分支混入与 PR1 无关的研究文档（需用户决定，不由执行方处理）

`f7ce9d3` 之后有 6 个 `docs(研究)` 提交（`0646468`…`518fef7`），新增 `docs/research/2026-10-15-codex-token-pulse-usage-statistics.md`（1728 行）与 `docs/research/2026-10-15-tokenarena-usage-statistics.md`（1134 行）。它们不属于 W01–W07，也不在 PR1 计划范围内。只改文档，不影响门禁与运行时，但会把约 2900 行无关内容带进「只改目录」的 PR1，评审和回滚粒度都变差。

两种处理方式：

- 移出（推荐）：以当前 HEAD 建 `docs/research` 分支保留这 6 个提交，把 `feat/workspace-layout` 重置到 `f7ce9d3` 并强制推送。这一步会改写已推送的历史，需要用户明确授权。研究文档以后单独走文档 PR，合入 main 前需递增 patch 版本。
- 保留：PR 描述中单列这两份研究文档，说明与目录迁移无关；版本不重复递增。

### 17.3 仍未闭合

- 远程 CI、安装升级、Native UI E2E：创建 PR 后才有证据。
- 旧 `src-tauri/target`（38.85 GB）：按用户决定暂缓清理。

## 18. 升级测试补测计划复核（docs/upgrade-matrix-plan，执行方补充 fb35e91）

审阅对象：`docs/plans/active/2026-10-16-upgrade-matrix.md` 新增的 §10、§11 和前文的 7 处改动。以下判断都对照了当前代码和远程记录：`frontend/scripts/native-acceptance.mjs`、`contracts/native-ui-system.mjs`、`.github/workflows/ui-e2e.yml`、`frontend/vitest.config.ts`、`scripts/release_assets.py`、`scripts/pre_commit_checks.py`，以及 Native UI E2E 运行 38580807609 的步骤耗时与工件。

### 18.1 接受的补充

| 位置 | 结论 |
| --- | --- |
| §10.2.3 `--check` 固定使用事件的 base SHA | 接受。避免运行途中 main 前进导致基础版本变化 |
| §10.3 撤回机制的真实边界 | 接受，而且纠正了原计划的过度承诺：旧提交的流水线不可能知道之后发生的撤回。`publish` 会回补 0.1.0（`release_assets.py:350`），所以检查覆盖所有将创建的版本，这一点也是对的 |
| §10.4.2 I4 按“本跳新增备份”判断 | 接受。连续链上保留前一跳的备份是正常现象，原 I4 会误报 |
| §10.4.4 链路每跳使用各自目标版本的期望清单 | 接受 |
| §11.2.3 进程身份用 `(pid, creationTime)`，并递归记录后代 | 接受 |
| §11.4 先建证据骨架，再启动 | 接受，问题已核实：首次 `launch` 的 catch（`native-acceptance.mjs:1543`）在写 `evidence.json`（约第 1680 行）之前直接抛出，所以首次启动失败时没有工件 |
| §11.5 次数口径：21 次 launch，20 次循环 | 接受 |
| §4.3 0.1.0 被撤回时报错 | 接受 |

### 18.2 必须修改（改完才能交给执行）

**A. 启动预算被收紧了 20 秒。**

现有代码分两段计时：CDP 就绪最多 45 秒（`native-acceptance.mjs:236`），之后等页面 target 最多 20 秒（`:256`），上限合计约 65 秒。§11.3 改成“从分配端口到选定页面总共 45 秒，不能再加 20 秒”，实际是把上限压到 45 秒。PR #9 在 PR 阶段的失败正好发生在 `about:blank` 这一段，收紧它会引入新的不稳定，也违背“只改生命周期、不改验收条件”。

起因是我原文 §4.8 第 4 条写的“沿用 45 秒”有歧义。

改为：单次启动总预算 65 秒，等于现有两段之和；每一阶段的耗时写进 `launches[]`。是否收紧，等有了实测分布再另行评审。§4.8 第 4 条和 §6.2 的“45 秒”一并修改。

**B. 不同进程之间仍然复用同一个端口。**

`ui-e2e.yml` 连续启动 4 个 `ui-system` 进程（默认缩放和 1、1.5、2 三档），全部使用 `--port 9452`。现有 `stop()`（`:351`）只结束直接子进程，等 600 毫秒，不核实是否真的退出。§11.2.1 的“本次驱动进程用过的端口不再使用”只约束同一进程内部，后一个进程依然从 9452 开始。

失败的 run 38768858040 出在第二个 9452 进程（layout-1）。它的直接失败点是进程内重启，但跨进程复用是同一类隐患。要求三项：

1. 每个场景结束时的 stop，同样执行 §11.2.5 的退出确认：所有后代进程退出、端口释放；确认不通过，场景判失败。
2. 工作流每一步使用互不重叠的端口段，段宽不小于该步的最多启动次数。例如 ui-refresh 9451，四个 ui-system 分别 9460、9470、9480、9490，压力场景 9600。
3. `test_upgrade_workflow.py` 或 UP09 的契约测试校验各步端口段不重叠。

**C. 计划内部前后矛盾，不能靠“冲突时以 §10、§11 为准”兜底。**

执行方会按条文实现，条文互相矛盾就会实现错。应当直接修改前文，并删掉这句兜底。目前的矛盾有：

| 位置 | 现有写法 | 应与之一致的条文 |
| --- | --- | --- |
| §2 G4 | “撤回后不会被旧流水线重新发布” | §10.3：只能约束含新检查的发布代码 |
| §3 I7 | 没有说明适用边界 | 同上 |
| §4.4 撤回一行 | 先在发布页撤回，再由下一个 PR 登记 | §10.3：先取消在途发布、登记撤回并进入 main，再删除发布 |
| §4.8 第 1 条 | 基准端口 + 启动序号 | §11.2.1：递增探测，最多 128 个候选，不回绕 |
| §4.8 第 4 条、§6.2 | 45 秒 | 见 A：65 秒 |
| §4.8 第 7 条 | `--relaunch-cycles` 可用于任意场景，并读取设置 | §11.5：只用于独立的 `ui-relaunch` 场景，其他场景拒绝这个参数 |

**D. 撤回过程中的中间状态，`--check` 没有定义。**

按 §10.3 的顺序，登记撤回的那个 PR 运行 CI 时，该版本的发布页仍然存在。§4.4 只规定了“`published` 条目必须存在”，没有规定 `withdrawn` 条目遇到发布仍存在时如何处理。

建议规则：

- `withdrawn` 条目不要求它在 GitHub 上的状态。发布还在时，`--check` 输出“待删除”提示，但不判失败。
- 如果有人先删了发布、后登记，那么在登记合入之前，每个 PR 的 `--check` 都会失败。这正好是 D2 的兜底，保留。

### 18.3 需要用户决定：20 次重启压力测试是否常驻

原计划只要求 20 次重启在本地和 PR 远程各实跑一次，作为本 PR 的取证。§11.6 改成在 `ui-e2e.yml` 中常驻，PR、main、手动触发每次都跑 20 次重启。

成本估算如下，是推算，未实测：

- 运行 38580807609 中，4 个 `ui-system` 进程（每个含一次重启）合计 2 分 40 秒。
- 按此推算，20 次重启约增加 4–6 分钟。
- native-ui 目前每次 9–13 分钟，而且是必需检查。

建议：

- 常驻部分改为 `--relaunch-cycles 3`，保留对重启链路的回归保护。
- 20 次改由 `workflow_dispatch` 输入触发。`ui-e2e.yml` 在 main 上已经有 `workflow_dispatch`，可以在 PR 分支上手动触发。
- 本 PR 的验收不变：PR HEAD 上 20 次重启实跑一次，并且一次通过。

### 18.4 应当修改

1. **`--check` 是否下载资产要写清楚。** §10.2.4 现在的写法可以理解为每次 `--check` 都下载全部已登记的安装包并计算哈希。这样每个 PR 的下载量会随版本数线性增长，必需检查也会更依赖网络。建议分工：
   - `add` 下载每个资产并逐个计算 SHA256。
   - `--check` 比对 API digest 与登记值，只有缺 digest 时才下载。
   - 安装作业在执行前按 I1 下载并校验。
2. **允许的页面 origin 要有出处。** 代码目前用 `/tauri/i` 匹配，注释写的是 `tauri://localhost`。现有 `evidence.json` 不记录页面 URL（查了 38580807609 的工件，没有这个字段）。要求：先在 `launches[]` 中记录 target URL，从一次成功运行中取得实际 origin，再写成常量，并在契约测试中固定，不凭记忆填写。
3. **`print_persistent_formats` 的编译开销。** `cargo run -p tokenscope --example` 默认使用 dev profile，在 Release 作业里会把核心库多编译一遍。要求：
   - 写明使用哪个 profile、哪些 feature，并实测增加的耗时。
   - 写明“两套生产构建工作流”具体指哪两个文件。
4. **PowerShell 适配器的耗时。** 每次查询都要启动 pwsh 并执行 CIM 查询，这部分耗时会计入启动预算。每次连接重试前后各核对一次归属，次数一多，开销可能不小（推断）。要求：在 `launches[]` 中记录每次查询的耗时；如果单次超过 2 秒，改为一次查询同时返回进程树和监听表，或者使用常驻进程。

### 18.5 结论

方向正确，§10、§11 补上了多处真实的实现缺口。但还有 4 项必须先改：A 启动预算、B 跨进程端口、C 前后矛盾、D 撤回的中间状态。18.3 需要用户决定。这几项处理完之前，计划不交给执行。

## 19. 用户决定与计划精简（2026-10-16）

用户决定：20 次重启压力验证只在本地跑，不进 CI（计划 D5）；计划中过度防御的设计和过高的验收门槛要删除。据此重写计划，§10、§11 不再单列，有用的内容并回正文，第 18 节的 A–D 一并处理：

| 第 18 节 | 处理 |
| --- | --- |
| A 启动预算 | 单次启动共 65 秒，等于现有两段上限之和 |
| B 跨进程端口 | 进程内 `--port + n`；工作流各步起始端口互不重叠；每次关闭都确认后代进程退出、端口释放 |
| C 前后矛盾 | G4、I4、I7、§4.4 撤回顺序、端口、预算、`--relaunch-cycles` 归属已统一，删去“冲突以 §10、§11 为准” |
| D 撤回中间状态 | `withdrawn` 条目的发布仍在时只提示“待删除” |
| 18.3 压力测试常驻 | 按 D5 只在本地跑；UP09 不再要求 PR 远程实跑 20 次 |
| 18.4 第 1 项 | `add` 下载计算哈希；`--check` 比 API digest，缺 digest 才下载 |
| 18.4 第 2、4 项 | 随删除项一起取消（不再要求精确 origin，不另建 PowerShell 适配器） |
| 18.4 第 3 项 | 写明 `--release` 运行示例程序，交付记录回填增加的耗时 |

删除的过度设计：

- 九状态的生命周期状态机，以及注入 8 个依赖的接口。
- 独立的 PowerShell 适配器脚本及其契约测试。
- 128 个候选端口的递增探测，以及 IPv4/IPv6 双栈判断。端口被占用直接失败。
- 用 `(pid, creationTime)` 防 PID 复用、持续累积祖先链。20 秒窗口内 PID 复用的概率可以忽略，第 18.1 节对它的接受相应撤回。
- 5 次重试加退避序列、单次 5 秒取消，以及优先 websocket URL。改为在总预算内每 300 ms 重试。
- 精确 origin 匹配。保留现有 `/tauri/i`，只在证据中记录 target URL。
- §11.5 新增的 14 组反例。保留 §6.2 的 7 组。
- 证据骨架逐阶段原子更新。改为 try/finally 保证写出。
- 发布前撤回检查的 SHA 与时间留证。
- 403/404/限流的单元测试矩阵。保留“API 出错即失败”。
- 连续链的 SQLite backup API 备选方案。旧程序正常退出后复制即可。

第 18.1 节接受的其余补充保留：固定 base SHA、撤回的真实边界、逐跳备份、逐跳期望、首次启动失败也写证据。

## 20. 升级测试补测实现复核（PR #10，HEAD c64685d）

### 20.1 核对结果

| 项 | 结论 | 依据 |
| --- | --- | --- |
| 起点选择 | 符合 §4.3 | `select --current-version 0.3.3` 输出起点 {0.1.0, 0.3.2}、链 0.1.0 → 0.3.2、`include` 四项；0.2.0–0.3.2 的持久格式相同，归为一组，代表正好是上一版 |
| 登记表 | 符合 §4.1 | 6 个条目、`commit` 均为标签解引用提交；本地 `check` 与 PR gate 均通过 |
| 工作流接线 | 符合 §4.7 | 两边的 `plan-upgrades` 都由 `select` 生成矩阵，空集合抛错；release 的 `publish` 依赖 `upgrade-test` 与 `upgrade-chain`；gate 用 `pull_request.base.sha`，push 时用 `event.before` |
| 撤回保护 | 符合 §4.4 | `publish` 先取 main 的固定 SHA，再读该 SHA 的登记表，读取失败即阻断；覆盖当前版本与回补的 0.1.0 |
| 启动预算 | 符合 §4.8 第 4 条 | CDP 45 s + 页面 20 s，合计 65 s，与原上限一致 |
| 端口 | 符合 §4.8 第 1 条 | `ui-e2e.yml` 各步改用 9451/9460/9470/9480/9490 |
| CLAUDE.md | 符合 UP08 与 §4.8 第 8 条 | 写明登记上一版、撤回顺序、“重跑不能代替原因”、压测只在本地 |
| 远程证据 | 通过 | PR 的 gate、install-test（四个单跳）、native-ui 均通过。链路在 PR 上默认跳过，我在 PR 分支手动触发 run [38912723002](https://github.com/ywltby/tokenscope/actions/runs/38912723002)：`upgrade-chain (present)` 1m07s、`(deleted)` 1m51s 通过，日志为“连续升级链 0.1.0 -> 0.3.2 -> 0.3.3：逐跳事实、备份、设置与重启幂等全部通过”；同一次运行的四个单跳作业也通过 |

### 20.2 UP09 的实现超出了第 19 节精简后的范围（不阻断合入）

UP09 一共约 2900 行：`native-instance.mjs` 867 行，两份测试共约 1000 行（48 项），`native-acceptance.mjs` 净增约 980 行。

其中有几项正是第 19 节明确删除的“单次调用取消 / 预算截断”一类设计，由执行方之后的第二轮复核（P2）重新加了回来：

- `awaitWithHostExit` 的 `onCancel` / `onLateResult` 双回调与 AbortController。
- `createHostExitSignal`：退出事件与 250 ms 轮询竞速。
- `clampTimeoutToBudget`，以及按剩余预算截断探测和连接。
- `native-instance.async.test.mjs` 的 21 项异步组合测试。

判断：

- 这些代码已经实现、测试全绿、远程通过，现在拆掉要再走一轮完整门禁，还有引入回归的风险，收益不大。所以**不返工**。
- 约束一：PR2 的 F04 只复用现有的 `launch` / `stop` 与 `launches[]`，不再扩展生命周期逻辑。
- 约束二：之后的复核不得推翻用户已确认的精简决定。要改，先回到计划，由用户决定。

### 20.3 小问题（不需要单独修）

- 计划文件头仍写“从最新 main 建 `feat/upgrade-matrix`”，实际 PR 的 head 是 `docs/upgrade-matrix-plan`。§10 开头“远程证据仍未取得”也已过时。合入后归档时一并修正。
- 0.2.0 起 `cache_schema` 登记的是运行时不再写入的 `cache.db` 常量。以后只改这个常量，会多出一个起点作业。影响很小，暂不处理。

### 20.4 结论

可以合入。合入后需要在 main 的 Release 里确认两条链再次通过、`v0.3.3` 已发布，然后用 `upgrade_baselines.py add --tag v0.3.3` 在下一个 PR 登记。

## 21. 发布去重计划复核（执行方补充 3c10bd9）

对象：[发布流程去掉重复的完整测试](../plans/active/2026-10-17-release-gate-dedup.md)，执行方新增 §6（85 行）。按用户“避免过度防御的计划”的要求精简，§6 重写为 7 步，计划从 145 行减到 87 行。

### 21.1 接受

| 补充 | 处理 |
| --- | --- |
| §4：相同代码树不保证结果相同，失败不能直接归为环境问题 | 接受，压成一句；撤回顺序补上“取消在途发布” |
| §4：分支保护放宽时需重新评审 | 接受 |
| R01：PR 上也有安装与单跳升级，但 Release 重新构建的包须独立验收 | 接受，保留原文 |
| §6.1：上一版按 main 实际版本登记，不机械照抄 0.3.3；已登记则跳过 | 接受 |
| §6.3：setup-python 的固定写法与步骤顺序 | 接受 |
| install-test 会被触发 | 改为明确写出：本 PR 改了 `scripts/**` 与 `.github/workflows/**`，install-test 必然触发，须通过 |

### 21.2 纠正

- **§6.1 第 3 步“从最新 main 建 `ci/release-gate-dedup`”会丢提交**：当前分支 `docs/release-gate-dedup-plan` 已带第二批计划归档 `fcc3578`，另建分支只“带入本计划文件”，归档提交就不会进 main。改为在当前分支 rebase 到最新 main 后实施。

### 21.3 删除的过度设计

- 开工时重新读取规则集并逐项比对。前提已在 2026-10-17 核实，写进了 §1；放宽时重新评审已由 §4 约束。
- 契约测试断言 runner、timeout、checkout 的三项参数、六项 outputs 全集、build 仍用 setup。只保留真正要防的三条：身份校验在、完整动作不在 Release gate、CI gate 仍跑完整检查。
- 用内存副本做 5 种变异反例，并要求正常文件与反例走同一函数。
- 保存红灯日志到 `qa-artifacts/` 并核对失败类型。
- 提交前手工逐条跑 5 个单测命令和 `git diff --check`。改 `.github/**` 时钩子本来就选完整模式。
- 记录 PR 合并 ref 的 checkout SHA 并核对父提交。规则集要求与 main 同步，GitHub 已保证。
- 合入后下载安装与升级证据逐跳核对。本 PR 不改这些作业，作业通过即可。
- 核对 tag 解引用、metadata SHA、安装包哈希。`publish` 本来就做这些校验。
- gate 5 分钟硬门槛，以及排队时间与执行时间分开统计。改为如实记录耗时，不设硬门槛。
- 回填 6 行证据表。改为回填 PR 链接、Release run 链接和各作业耗时。

## 22. 发布去重实现复核（PR #11，HEAD 6f495f4）

结论：**可以合入。** 改动范围与精简后的计划一致，PR 首次运行全部通过。

### 22.1 逐项核对

| 项 | 核对结果 |
| --- | --- |
| R01 `release.yml` | 只改 `gate`：删除 setup 与 checks，加 setup-python 3.12（固定 SHA 与其他作业相同），timeout 45 → 10，加一行注释；checkout、两个校验步骤、`id: metadata`、六项 outputs 与其余六个作业的 diff 为空 |
| R02 契约测试 | 三条断言与计划一致。复核时把 `release.yml` 换回 main 版本运行，测试失败（gate 含 `./.github/actions/setup`）；恢复后通过 |
| R03 文档 | `CLAUDE.md`「发布与升级」写明 Release gate 只做身份校验、完整测试由 PR 必需检查保证、main 的 CI 与 Native UI E2E 并行不阻断 |
| 登记 v0.3.3 | 提交 `e3eb680`、持久格式指纹与 `build-metadata.json` 一致；四个资产的 sha256 与 GitHub digest 逐一相同 |
| 版本 | 0.3.3 → 0.3.4，patch，`release-change.json` 摘要已更新 |
| PR 检查 | 同一 HEAD：CI `gate`、`native-ui`、`install-test`、`plan-upgrades` 与四项单跳升级均为首次运行通过；`upgrade-chain` 在 PR 上按设计跳过；`mergeStateStatus` 为 CLEAN |

### 22.2 计划外的改动

- **`cf73995` 原生实例退出用例的偶发红灯**：Windows 上轮询可能先于父进程回收子进程发现进程已退出，此时 `child.exitCode` 仍为 null。修复只放宽轮询路径（允许 3 或 null），exit 事件路径仍严格要求 3；与同文件另两个真实子进程用例对 unknown 退出码的处理一致。提交说明写明了原因，不是重跑碰绿，属于 CLAUDE.md 允许直接修复的测试红灯。接受。
- **`scripts/test_upgrade_baselines.py` 改了 68 行**：原因是 `RegistryCase.setUp` 直接读取真实登记表 `scripts/fixtures/upgrade_baselines.json`，登记 0.3.3 后，写死的版本号与选择结果都要跟着改。本次修改正确，但问题会在每次登记上一版时重复出现（PR2 登记 0.3.4 时还要再改一遍）。建议：测试改为读取一份冻结的测试登记表，与真实登记表脱钩；真实登记表的正确性已由 CI gate 的 `upgrade_baselines.py check` 保证。不阻断本 PR。

### 22.3 文档

执行方指出的计划页头“待用户确认”已改为“已实施，待合入后的 Release 取证”，§5 回填本地与 PR 证据；计划总账同步。合入后的 Release 取证（gate 日志、双链、发布与耗时）完成前不归档。
