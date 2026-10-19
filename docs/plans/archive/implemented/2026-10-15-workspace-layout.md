# PR1：虚拟 Cargo Workspace、目录迁移与提交检查

> **2026-10-17 归档复核：已实现。** PR [#9](https://github.com/ywltby/tokenscope/pull/9) 于 2026-10-15 合入 main（`19aae8b`），[Release](https://github.com/ywltby/tokenscope/actions/runs/38768858069) 的 gate、构建、五场景安装与固定 0.1.0 升级通过并发布 `v0.3.2`。归档不等于其他计划的待验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。下方原文保留当时的状态与证据，其中“待合入 / 待门禁 / 未实施”等字样以本头部为准。
>
> | 第 3 节未勾项 | 当前状态 |
> | --- | --- |
> | 固定旧版升级实跑 | 已由 `v0.3.2` 的 Release 在一次性 Runner 上完成 |
> | 旧 `src-tauri/target` 清理 | 2026-10-17 用户要求清理时核实该目录已不存在（回收站中也没有），根 `target/` 完好；本仓会话记录中未找到删除命令，删除时间与操作者未知 |
>
> 合入后 main 上的 Native UI E2E 曾因重启后连接调试端口的时序缺口失败（[run 38768858040](https://github.com/ywltby/tokenscope/actions/runs/38768858040)），属测试架问题，已由[升级测试补测计划](2026-10-16-upgrade-matrix.md) UP09 修复。

> 状态：[审阅第 13 节](../../../reviews/2026-10-15-source-discovery-plan-review.md)复审通过，PR1 可开工；运行时 fixture 分类已按意见明确。W01–W07 未实施。设计基线 main e34495b / 0.3.1。
> 本轮计划修订留在 `docs/source-discovery-plan`；PR1 开工时从该分支最新提交创建 `feat/workspace-layout`，带入既有 mjs 维护提交 c16e023 和三份计划，不改写历史。仅本 PR 改目录/构建/门禁，独立 patch；合入后才从最新 main 分别启动 [PR2](2026-10-15-collection-status.md)、[PR3](../partial/2026-10-14-source-discovery-and-additional-paths.md)，不在本 PR 实施功能。

**Goal：** 核心库迁入 crates/tokenscope-core，统一 workspace/锁/target，并根据实际构建输入选择本地检查。

**Architecture：** 根虚拟 workspace 组织核心和壳；编译依赖信息生成受版本管理的输入清单，hook 只读清单，CI 重建校验。

**执行约束：** 开工前记录其他 Rust 分支及基线并协调冻结开发；本 PR 迁移全部 Rust 源码，合入后其他分支先 rebase 最新 main 再继续。当前仅修改计划，不自动中断其他 agent。每项先写失败契约再最小实现，使用 executing-plans 技能。

## 1. 已核实的现状与目标

- 根 crate `tokenscope` 是库，根 `src/main.rs` 已不存在；桌面 package 为 `tokenscope-app`，唯一应用二进制为 `tokenscope.exe`。保留二进制名称和应用标识，不影响安装升级。
- 根 manifest 仍以旧 CLI 同名为理由 `exclude = ["src-tauri"]`，壳 manifest 仍声明独立 `[workspace]`，相关注释已过时。
- 根锁文件中的全部包身份（name/version/source）均已存在于当前壳锁文件；实施时须重新核实，以最新内容为准。可优先使用包含完整依赖图的壳锁文件作为统一锁候选，再由 Cargo 验证，不能手工拼接锁文件。
- 目标为一个 workspace、一个根 `Cargo.lock`、默认一个根 `target/`；同一依赖在相同构建条件下复用。不同 profile、features、目标平台以及 host/build 依赖仍可能分别构建，不承诺全部消除重复编译或节省固定比例空间。
- “两个 target 共约 76GB”尚未实测。清理前记录真实大小、路径和链接关系，区分实际目录、junction 与共享 worktree 缓存，不能把逻辑路径大小直接相加。

## 2. 目标配置与不变量

2026-10-15 用户确认完整目录整理，取代此前“不搬迁源码”的方案：根目录改为虚拟 workspace，核心库迁入 `crates/tokenscope-core/`，前端和桌面壳继续独立。目录名称明确职责，本轮保留核心 package 名 `tokenscope`，避免无收益的全仓 import 改名；壳 package 仍为 `tokenscope-app`，exe 仍为 `tokenscope.exe`。

```text
tokenscope/
├── Cargo.toml                    # 虚拟 workspace、共享依赖
├── Cargo.lock                    # 唯一锁文件
├── crates/tokenscope-core/
│   ├── Cargo.toml                # 核心 package，name = "tokenscope"
│   ├── src/                      # 原根 src/，不依赖 Tauri
│   └── tests/                    # 原根 tests/，含完整 fixtures
├── frontend/                     # Vue 与前端 package/lock
├── src-tauri/                    # Tauri package、配置、图标、权限
│   └── src/                      # IPC、窗口、托盘、生命周期
├── scripts/
├── docs/
└── target/                       # 默认共享输出
```

根 manifest 不再含 `[package]`、包级 dependencies/dev-dependencies/features；这些随核心库迁移。根配置：

```toml
[workspace]
members = ["crates/tokenscope-core", "src-tauri"]
default-members = ["crates/tokenscope-core", "src-tauri"]
resolver = "3"
```

删除根 exclude 和壳独立 workspace。`default-members` 明确根目录普通 build/test 覆盖两个 package；只查核心库使用 `cargo test -p tokenscope`，只构建 GUI 使用 `cargo build -p tokenscope-app --bin tokenscope`。CI 仍显式使用 `--workspace`，不依赖工作目录的默认选择。

把共同使用的 `anyhow`、`serde`、`serde_json`、`log`、`jiff` 等依赖声明集中到 `[workspace.dependencies]`，成员以 `workspace = true` 继承。版本约束选择满足现有两侧要求且不改变锁定版本的交集；各成员需要的 features 继续显式保留。根库 dev-dependency 与普通 dependency 的共享项也检查一致性。仅壳使用的 Tauri/plugin 依赖留在壳中，不为追求统一机械上移全部依赖。

本轮保留两包各自显式 package version，不顺带引入 `workspace.package` 版本继承。四份版本配置为 `crates/tokenscope-core/Cargo.toml`、`src-tauri/Cargo.toml`、`frontend/package.json`、`src-tauri/tauri.conf.json`，根虚拟 manifest 没有应用版本。统一锁必须恰好包含两个无 source 的本地包、版本与四配置一致。第三方版本、checksum、source 不应漂移；必要差异必须逐项解释，不运行无范围的 `cargo update`。

生产构建仍默认不含 acceptance；`tokenscope-app/acceptance` 继续透传根库 feature。验证全 workspace acceptance 覆盖两个 package，正常发布构建显式不带验收 feature，不能用验收包作为发布包。

### W01：基线、失败契约与路径清单

1. 读取当前 Git 状态、锁文件、Cargo/Tauri 配置、环境 `CARGO_TARGET_DIR`、用户/项目 Cargo target-dir 配置；仅检查相关键，不输出无关凭证。记录当前 metadata 的 workspace_members/default_members/target_directory 与依赖版本集合。
2. 全仓搜索 `src-tauri/target`、反斜杠形式、`path.join(..., "src-tauri", "target", ...)`、双 Cargo.lock、独立 workspace、旧 CLI 同名等引用；区分运行入口与历史证据，历史报告不伪改成新路径。
3. 更新 `scripts/test_bump_version.py` 的合成仓库 fixture：一个根锁包含两个本地包，删掉子锁 fixture。先运行并记录旧工具因要求第二锁而失败。
4. 补充有意义的失败用例：统一锁缺壳包、任一本地包版本错误/重复、第三方同名带 source 的包不误改；保留原子回滚、重复 bump 幂等、历史 main 基线读取测试。
5. 检查现有工作流/发布契约覆盖位置。为真实路径接线补测试，不用单纯断言某段字符串代替实际 metadata、构建与产物验证。
6. 新增核心库目录迁移清单：`CARGO_MANIFEST_DIR`、`include_str!`/`include_bytes!`、测试 fixture 相对路径、脚本工作目录、工作流 paths、文档源码链接。先设计“虚拟 workspace 新布局＋旧布局历史基线”的版本工具 fixture，证明旧工具不能正确处理新布局。

### W02：合并 workspace、依赖和锁文件

1. 用 `git mv` 将根 `src/`、`tests/` 整体迁入 `crates/tokenscope-core/`，包含 fixture；从原根 manifest 拆出核心 package manifest，根改虚拟 workspace。壳核心依赖改为 `tokenscope = { path = "../crates/tokenscope-core" }`，按 2 配置共享依赖，删除旧 CLI 同名规避注释。不保留旧路径转发副本、junction 或重复测试目录。
2. 保存两份旧锁的比较基线，将完整壳锁内容作为根锁候选，删除受版本管理的 `src-tauri/Cargo.lock`；不删除 target。
3. 先运行 `cargo metadata --offline --locked --format-version 1` 和 `cargo check --offline --locked --workspace`。若 Cargo 要求刷新锁，单独解析并审阅差异后重跑 locked 检查；网络确有需要时遵守项目代理约定。
4. 核对 metadata 恰有两个成员、默认选择包含两者、target_directory 为预期根目录；使用 `cargo tree --workspace -d` 分析剩余重复版本，不强制消灭合法的多版本依赖。
5. 对比最终锁与迁移前完整壳锁的第三方包版本/source/checksum，解释任何差异。运行根库和壳定向测试，确认无循环依赖。
6. 核心源码和 tests 同级关系保持，逐项验证 fixture 实际加载内容；不要把所有 `CARGO_MANIFEST_DIR` 盲目改成仓库根。壳测试跨 crate 读取的 `tests/fixtures/codex/rate-limit-only.jsonl` 路径必须随迁移更新，并由 W05a 的 fixtures 目录规则选择 workspace 两种模式检查；运行时读取不会被 W05c 编译输入清单捕获，不能仅检查核心自己的 fixture。确需仓库根的工具入口显式解析 workspace root，核心测试仍以 crate 根为基准。从根目录和核心 crate 目录分别运行代表性测试，确认不依赖启动 cwd。
7. 检查依赖方向：核心库不引入 Tauri，壳依赖核心库，前端仍通过 IPC；本轮目录迁移不混入业务逻辑重构，也不把 source/history/pricing 拆成更多 crate。

### W03：版本工具迁移为单锁契约

1. 修改 `scripts/bump_version.py` 和 `scripts/release_metadata.py`：当前四配置读取新核心 manifest，LOCKS 仅根锁；`validate_locks` 不再按锁索引决定包集合，必须验证两个本地包；禁止对根虚拟 manifest 写 version。
2. 同步 `scripts/test_bump_version.py`、`scripts/test_release_metadata.py`：四配置＋一锁＋声明原子更新。历史版本解析须读取指定 Git ref 自身的文件布局：旧提交从根 `[package]` 取版本，新提交从核心 manifest 取版本；当前布局不能决定历史布局。缺文件与 Git 执行错误区分处理，新布局文件损坏不得静默回退。分别测试迁移前 main→新布局分支、新布局 main→后续 bump、两布局版本不一致/损坏的拒绝路径。
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
| 工作流 paths、脚本 fixture/源码引用、当前开发文档 | 根 `src/**`、`tests/**` 改为 `crates/tokenscope-core/**`；构建配置保留独立触发。检查删除/重命名也能触发 CI；旧历史报告不改写事实 |

不要假设所有构建都带三元组：不带 `--target` 的原生验收路径与显式 target 的发布路径分别取证。若用户显式设置 CARGO_TARGET_DIR，支持现有 exe 参数覆盖，不静默忽略用户配置；标准 CI 验证无覆盖时根 target 行为。

### W05：按变更选择本地检查，去掉重复门禁和缓存映射

1. `.githooks/pre-commit` 按下述暂存区规则选择检查；完整模式合并为一次 workspace fmt/clippy/test，保留前端门禁；Cargo 检查使用 `--locked`，不漏掉壳测试。
2. `.github/actions/checks/action.yml` 保留普通、acceptance 两套 workspace clippy/test，删除重复壳轮次；fmt 只执行一次 `--all`。保留全部 Python、PowerShell、前端与 E2E 门禁。
3. `.github/actions/setup/action.yml` Rust cache 只配置 `. -> target`；迁移时增加缓存键版本以免恢复旧布局。保留各 job 区分，不能把不同构建场景视作同一完整缓存。
4. 检查 `ci.yml`、`install-test.yml`、`ui-e2e.yml` 的 paths 过滤是否覆盖改动入口，避免“改了构建配置却跳过验收”。
5. 本地验证命令：`cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --locked --workspace`；再分别以 `--features acceptance` 跑 clippy/test。
6. 验证根目录普通 `cargo test --locked` 确实选择两个默认成员；不再反复跑第二套独立 manifest 门禁。

#### W05a：暂存区驱动的检查选择（2026-10-15 用户确认加入）

目标：不属于构建输入的纯文档提交不跑 Rust/前端测试；编译进程序的文档必须选择消费方检查。代码按影响范围运行已有检查，未知类别完整兜底。PR CI 保留完整门禁，不做函数级测试推断或源码静态引用扫描，也不缓存“上次通过”来免检。

实现文件：`.githooks/pre-commit` 保留轻量入口；新增 `scripts/pre_commit_checks.py` 集中分类和命令编排，新增 `scripts/test_pre_commit_checks.py` 覆盖临时 Git 仓库契约；文档校验可置于独立 `scripts/check_staged_docs.py` 并有对应测试。使用现有 Python 标准库，先核对开发环境可用；缺解释器或分类执行异常必须明确失败，不能静默放行。

| 暂存区文件类别 | 最小检查集合 |
| --- | --- |
| 未命中构建输入清单的 `.md` 文档 | staged whitespace、UTF-8、文档格式与新增/变更本地链接；不跑 Rust/前端测试 |
| 命中输入清单的文件（含 `.md`、`.js`、fixture） | 按全部“包＋构建模式”消费方并集选择 clippy/test；壳用壳检查，核心用 workspace 检查；default 不加 feature，acceptance 加 `--features acceptance`；文档仍做文档检查 |
| `crates/tokenscope-core/**` 的 Rust 代码 | Rust 格式、普通和 acceptance 两种模式的 workspace clippy/test，覆盖壳依赖核心库的影响；迁移提交中旧根路径的删除/重命名也按核心库分类 |
| `src-tauri/src/**` 的 Rust 代码 | Rust 格式、普通和 acceptance 两种模式的 `tokenscope-app` clippy/test |
| `crates/tokenscope-core/tests/fixtures/**` | 固定选择 workspace 普通与 acceptance 两种模式 clippy/test，包含壳测试；无论编译期嵌入或运行时读取均适用，不要求先命中输入清单 |
| 上述目录中未命中 fixtures 专用规则的非 Rust 文件 | 已登记输入按清单的包和模式选择；未登记者完整兜底。`acceptance_csp.js` 单独改动必须选中壳 acceptance 检查 |
| `frontend/src/**`、`frontend/public/**`、前端配置及依赖清单/锁 | 前端 typecheck、format:check、test、build；包含测试读取的 public/theme-boot.js；E2E 编排并入脚本规则 |
| `scripts/**` 的 Python/PowerShell、`frontend/scripts/**` | 明确映射的脚本语法和契约测试；共享 helper 取全部消费者契约并集，未登记脚本走完整模式，不只测试被改文件本身 |
| Cargo manifest/锁、`build.rs`、`.cargo/**`、Rust 工具链、Tauri 配置/capabilities、`.github/**`、`.githooks/**`、检查选择器及其测试、发布声明等构建/门禁入口 | 完整模式，包含普通与 acceptance Rust 检查、前端门禁和现有相关脚本契约 |
| 其他未明确分类的文件 | 完整模式兜底，输出未识别路径及原因 |

分类用有顺序的显式规则，构建/门禁入口优先于宽泛目录规则；多类改动取并集、同一检查只执行一次。普通代码最小检查不代表可免除最终完整门禁；完整模式命令与 W05 的 CI 门禁维护同一语义，不复制旧双 workspace 冗余命令。本地完整模式禁止运行真实安装/卸载/休眠，只运行安全契约与隔离验收；安装升级仍由一次性 Actions Runner 承担。

fixtures 专用规则先于文档豁免和非 Rust 未登记兜底，目录内 Markdown 也不能按纯文档免检。该规则与清单消费方取并集，不短路其他影响；清单损坏或同时修改门禁配置时仍走完整模式。迁移提交中的旧 `tests/fixtures/**` 删除/重命名按同一规则处理。运行时 fixture 不伪造为编译输入，也不为了清单而改成 include_str!。

构建输入匹配先于文档豁免，清单缺失/损坏/无法匹配格式时完整兜底。清单及生成器/测试变更属于门禁入口，走完整检查。对部分暂存读取 index 的清单；同时用 HEAD 清单与暂存清单的并集防止本次删登记导致输入漏检。

读取 `git diff --cached --name-status -z --find-renames` 的 NUL 分隔结果，支持新增、修改、删除、重命名及中文/空格路径。重命名旧路径和新路径都参与分类，防止 Rust 文件改名成 Markdown 后逃过检查。首次提交使用空树基线；Git 查询失败不能当作空提交。空暂存区只报告无文件检查，不擅自操作 Git index。

默认 `auto`；提供 `TOKENSCOPE_PRECOMMIT_MODE=full` 强制完整检查，非法值报错，不提供关闭全部检查的模式。提供只打印选中检查的 `--explain` 入口用于审阅，hook 正常路径必须实际执行。输出选中项、触发文件/规则、跳过原因和执行结果；任一检查非零立即阻止提交。

分类依据暂存区，但多数工具读取工作区：必须处理部分暂存，不能把“测了未提交修复”声称为“已验证暂存代码”。实施初版选择简单可靠规则：运行代码检查时，如被选检查会读取的受版本管理输入存在未暂存差异，明确阻止并列出需处理文件；不得自动 stash、reset、补暂存或覆盖用户修改。纯文档检查直接读取 index 内容，不因无关未暂存代码阻止文档提交。检测范围应覆盖共享配置和跨 crate 依赖，使用临时仓库测试其边界；若未来改成隔离 index 快照执行，另行设计缓存/环境成本。

文档检查必须检查暂存版本与暂存树中的链接目标：识别代码围栏，支持相对路径、目录、图片、URL 编码及可解析的标题锚点；网络链接不在 hook 联网验证。历史文档已有失效链接不突然全仓阻断，只校验此次新增/变更链接及本次删除/重命名明确影响到的本地引用。不能用正则盲扫所有括号；对不支持的语法明确说明检查边界，不假报通过。只检查，不自动格式化未暂存内容。

#### W05b：选择器和实际 hook 的验收

1. 先写临时 Git 仓库失败测试，覆盖纯文档、核心库、壳、前端、脚本共享依赖、混合改动、构建配置、未知文件、删除、跨类别重命名、中文/空格路径、首次提交、空 index 与 Git 查询错误。
2. 覆盖暂存文档与工作区不同、暂存代码与工作区不同、无关未暂存文件、强制 full、非法模式；证明不会 stash/reset、更改 index 或写用户目录。
3. 对命令执行器使用记录参数的替身，验证选择的命令及去重、失败退出码传播、未选择的命令未运行。模拟完整检查失败时必须阻止真实临时仓库 `git commit`；纯文档提交不得启动 Cargo/pnpm。
4. 文档校验测试包含有效/失效相对链接、删除目标、中文路径、围栏代码、历史失效链接及暂存树与工作区目标不一致，拒绝假通过和大量误报。
5. 运行 `python -B -m unittest discover -s scripts -p "test_pre_commit_checks.py" -v` 及文档校验测试；加入 CI 完整门禁，防止选择器修改只靠自己放行。
6. 在真实仓库分别记录文档、核心 Rust、壳 Rust 和强制完整模式的命令清单与耗时，明确两种 Rust 构建模式各自成本。核心/壳 Rust 变更均运行普通和 acceptance clippy/test；将来若缩减须先写确定的分类规则并配契约，不能按“涉及验收模块”猜测。首次切换 hook 的提交必须实际跑完整模式验证，禁止以修改后的分类器自动减少自身验证。
7. 更新 `CLAUDE.md` 提交流程：本地按暂存区选择，未知变更/full 保守检查；PR CI 完整门禁不减。同步修改现有“任何修改提交前必须全部跑一遍”的规定，否则仅智能化 hook 仍无法减少重复工作。保留禁止 `--no-verify`、默认验证后提交推送及失败不得提交等约束。

W05a/W05b 当前均未实施。完成标准：非构建输入的文档提交只运行文档检查，编译输入按消费者检查，核心库变更仍覆盖壳，混合变更不漏检，未知路径/失败/部分暂存不能静默绕过，当前 PR CI 仍执行完整门禁。

#### W05c：Cargo 依赖信息生成构建输入清单

新增 `scripts/build_inputs.py`、`scripts/test_build_inputs.py` 和受版本管理的 `scripts/build-inputs.json`。不做源码静态扫描；hook 不为分类先编译，仅读取清单。清单 schema 包含排序后的仓库相对路径、file/directory 类型、消费方集合；Rust 消费方明确记录包和构建模式（default / acceptance），如 `tokenscope-app@acceptance`。同一文件被两模式消费时保留两项，hook 分别选择普通和带 `--features acceptance` 的命令，不能合并成只有包名。自动生成项与人工登记项分区，后者为未来前端跨目录输入保留，不能悄悄覆盖生成结果。

**生成模式 `--update`：**

1. 明确提示会重新编译两个本地 package；先 `cargo clean -p tokenscope -p tokenscope-app`（非整仓 clean），检查无并发构建或运行实例占用；若不安全则使用明确的隔离 target，不能强杀实例或清理用户目标。
2. 在首次构建前记录此次生成 run_id、start、HEAD/输入状态及实际 target/build-dir；执行 `cargo test --locked --workspace --no-run --message-format=json`，再执行加 `--features acceptance` 的同一命令，两种模式均不带 custom-protocol。每种模式结束立即按“包＋模式”收集产物清单和输入，取并集，不能被后续模式覆盖掉普通模式证据。
3. 根据 Cargo metadata 和本次 JSON compiler-artifact/package_id 归属，只收集两个本地包的 `.d`，包含测试目标；消费方按编译包而非路径所属目录判断。解析 Makefile 转义、续行、Windows 盘符、空格/中文，不能按空格/冒号简单 split；不能混入第三方或旧构建残留。
4. 补读本地包 build-script-executed 所关联的构建脚本 output，支持 `cargo:rerun-if-changed=` 和 `cargo::rerun-if-changed=`，相对路径按该 package 根解析；目录输入保留目录类型，用于匹配未来新增子文件。构建脚本未声明输入时不能假定零输入，应保守登记包目录或明确拒绝缺失证据。
5. 解码依赖路径后，按其路径基准拼接，先按字面折叠 `.` / `..`，再判断仓库边界并转为仓库相对 POSIX 路径；例如 `src-tauri\src\../../docs/privacy.md` 归为 `docs/privacy.md`。正确处理 Windows 分隔符、盘符和 UNC，越出仓库的路径不收录。排除 `.rs`、Cargo.toml（固定规则已覆盖）、env-dep 和实际 target/build-dir 生成物；目录输入不得因扩展名规则误过滤。
6. 以 `git ls-files -z` 为依据，只保留 Git 跟踪文件或含跟踪文件的目录；未跟踪文件（含被忽略产物）、不含跟踪文件的目录均排除。已跟踪文件不能仅因命中 ignore 模式被漏掉。目录规则匹配暂存区受版本管理的后代，包含新加、删除和重命名文件。`OUT_DIR/tauri-codegen-assets`、`frontend/dist/assets` 等生成物不进入清单，前端源码到 dist 的变化由前端检查规则覆盖，不登记带哈希的输出。
7. 消费方和记录稳定排序，清单不含本机路径/mtime/run_id，不按扩展名豁免被编译的 Markdown/JS/SQL/JSONL。本次生成证据及排除路径、排除原因放 ignored QA 目录，清单原子写入仓库文件。映射位置改变、include_str!/include_bytes!/构建脚本输入新增删除后须运行 update 并提交清单。

**CI 模式 `--check`：**

- 在 workspace 构建前由 `--begin` 建立本次证据边界，清理两个本地包或确保明确的干净构建区；两种模式均记录 Cargo JSON，构建后 `--check` 基于该边界重新生成并比较。这是对审阅文字的必要细化：不能用编译后才启动的 check 进程时间过滤 `.d`，否则本次输出也会被全部滤掉。
- 不依赖 rust-cache 永远不缓存本地包的假设。只接受本次产物与其 run 边界之后更新的依赖证据；时间精度冲突由记录的产物身份/内容及清理边界验证。缺少普通/acceptance 任一模式或本地包、只有旧文件、Cargo 命令失败均失败，不能拿空集合通过。旧 mtime、不同 checkout、无本次记录的 `.d` 不参与。
- 不一致列出新增/缺失路径与消费包、构建模式差异；已不存在或不再受版本管理的登记文件失败。生成与检查使用同一 Git 跟踪过滤及路径规范化规则，人工项也须满足跟踪范围。人工项单独保留并校验路径/消费方，不能用人工项掩盖生成项过期。
- 同时校验 `.github/workflows` 中相关 CI/安装/Native UI paths 覆盖清单全部路径（包含目录后代、否定模式和顺序），替代零散维护 docs/privacy.md 的做法。实现可采用不设路径过滤的保守覆盖；校验步骤必须处于不会被这些文件改动跳过的 PR 工作流，不能形成“过滤遗漏，所以校验也不运行”的闭环。
- 清单只覆盖当前 Windows `cargo test` 普通＋acceptance 两种构建，均不带 custom-protocol；不声称涵盖生产 custom-protocol 或其他未构建的平台/feature，发布和 Native UI 构建仍由完整 CI 验证。build-dir 内部布局变化时应明确失败并升级解析器，不静默漏输入。

**必要契约：** docs/privacy.md 单独变更按登记模式选壳；src-tauri/src/acceptance_csp.js 单独变更必须选中壳 acceptance clippy/test，不能只运行普通模式；核心/壳 Rust 改动分别命中 workspace/壳的两模式 clippy/test；核心 fixture 变更选 workspace，包括壳测试跨 crate 引用；frontend/public/theme-boot.js 选前端；缺失/损坏清单 full；新增 include 输入未登记/登记目标已删/消费方或模式改动/缺模式/旧依赖信息全部失败。路径转义、Windows 分隔符/盘符/UNC、字面 `..` 折叠、越出仓库、目录新子文件、两模式消费集合不丢失、构建脚本两种前缀、跨 crate 消费归属均测试。增加 release 风格 `.d` fixture，包含 OUT_DIR 和 frontend/dist 的带哈希资源、合法跟踪输入，断言仅保留合法输入并记录排除原因；另测未跟踪输入、空目录排除和已跟踪但命中 ignore 模式的文件不漏登记。

运行 `python -B -m unittest discover -s scripts -p "test_build_inputs.py" -v`，CI 两种构建后运行 `python -B scripts/build_inputs.py --check`；hook 分类测试纳入 W05b 和 CI。同步 CLAUDE.md 开发流程，不新增静态扫描引用的一层。

依据：[Cargo dep-info](https://doc.rust-lang.org/cargo/reference/build-cache.html#dep-info-files) 与 [构建脚本输出](https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script)。清单记录迁移后的实际编译输入，包括嵌入的 schema-v3.sql；运行时 fixture 由 W05a 专用目录规则覆盖，不要求出现在清单。旧路径不能遗留为隐式豁免。

### W06：构建与缓存隔离验收，最后处理旧 target

1. 在没有旧编译产物的隔离 checkout 或一次性 Actions Runner 上运行 metadata、普通/acceptance 门禁，证明结果不依赖旧 `src-tauri/target`。
2. 生产 `tauri build` 不带 acceptance，生成 NSIS；执行 release_assets 收集/哈希契约，确认实际 exe 与安装包路径均正确。
3. 另行构建 acceptance 包并跑 Native UI E2E，证明隔离根仍生效且不会读取/污染用户真实数据；生产与验收产物按不同 job/工件分开记录。
4. Actions 跑五项安装验收和两项固定旧版升级；本机禁止运行真实安装/卸载脚本。检查上传工件非空、当前 HEAD 与版本匹配，而不只看 job 成功。
5. 记录从根和 src-tauri 发起 Cargo/Tauri 命令后都未新建子 target/子锁。更换构建入口不能回到旧布局。
6. 新构建验证通过后，向用户报告旧 target 的实际绝对路径、大小、链接关系及进程/worktree 占用，得到明确确认后才删除。不能把本计划批准当作具体目录删除确认，不能在 PR 门禁中自动清理；当前运行实例可能来自旧 debug exe。
7. 不复制拼接两套 target，不先清空根 target。确认后再验证路径在预期仓库内，使用单一 PowerShell 文件操作；仍占用则登记待清理、不强杀实例。记录实际释放空间。W05c 仅本地包重编译准备不等于此处整目录清理授权。

### W07：规约、版本与 PR1 交付

- 更新 `CLAUDE.md` 的 workspace、命令、提交钩子、单锁版本规约；检查 README、开发/发布说明中的当前构建步骤。历史审核证据中的旧产物路径保留并注明历史上下文，不重写旧成功记录。
- `--manifest-path` 本身仍合法，不误当旧补丁删除；本计划执行命令统一使用 workspace 或 `-p`。
- PR1 只交付 W01–W07 与 c16e023，不夹带 F/S 功能。后续计划的文件路径按新布局，PR1 合入后其他 Rust 分支先 rebase 再开发。
- fetch 最新 main，迁移后的工具独立 bump patch；不写死版本号，不顺带更新第三方依赖。PR2、PR3 各自基于其开始时最新 main 再递增 minor。
- 全部普通/acceptance 门禁、前端构建、版本/发布契约、本次 HEAD 的 CI/安装升级/Native UI E2E 通过后才报告合并就绪；不自行合并。

## 3. 进度与完成标准

| 任务 | 状态 | 证据 |
| --- | --- | --- |
| W01 基线与失败契约 | 已完成 | `qa-artifacts/workspace-layout/94e5c52/W01/`（两份 metadata、两份旧锁、`red-bump-version.txt` 18 项 KeyError 红灯） |
| W02 单 workspace 与锁 | 已完成 | `a26961f`、`c0497ca`；`W01/metadata-unified.json`（两成员/默认两成员/根 target）、`W02/cargo-check-exit.txt`、`W02/cargo-test-summary.txt`（552 通过）、`W01/lock-diff-shell-vs-unified.json`（467 包零差异） |
| W03 版本工具 | 已完成 | `abc4ef5`；`W03/green-bump-version.txt`（16 OK）、`W03/python-all.txt`（82 OK） |
| W04 路径接线 | 已完成 | `6f6f4dc`；残留检索 `W01/grep-old-target.txt`、`W01/grep-shell-manifest.txt` |
| W05 门禁与缓存 | 已完成（本地） | `6f6f4dc`；`W05/test-pre-commit.txt`（30 OK）、`W05/test-check-staged-docs.txt`（15 OK）；真实提交 `a26961f`/`c0497ca` 触发完整门禁并通过 |
| W05c 构建输入清单 | 已完成（本地） | `abc4ef5`；`W05/build-inputs-update.txt`、`W05/build-inputs-check-final.txt`（零差异）、`W05/build-inputs-check-negative2.txt`（漏登记+paths 未覆盖双重检出） |
| W06 构建验收与旧目录清理 | 本机部分完成 | `W06/isolated-checkout.txt`（隔离 checkout 冷编译两模式、旧入口不新建子 target/子锁）、`W06/build-and-acceptance.txt`（生产 NSIS + 资产收集 + 验收包 + 原生 E2E）；旧 target 按用户决策登记待清理（见 6.1） |
| W07 文档与交付 | 部分完成 | `fda4a6c` 起 HEAD：`CLAUDE.md`/`README.md`/`stats-semantics.md` 已更新、版本 0.3.2、`--check` 通过、分支已推送；远程门禁待创建 PR |

- [x] metadata 两个成员、正确默认成员与统一 target_directory。
- [x] 根是虚拟 workspace，核心源码/测试/fixture 完整迁移，无旧路径副本；跨 cwd 测试、旧/新 Git 基线版本读取均通过。
- [x] 仅一个受版本管理的 Cargo.lock，两个本地包版本正确，第三方依赖无未解释漂移。
- [x] 旧 CLI 同名规避配置和当前文档说明全部清除，共享依赖 features 保持正确。
- [x] 钩子、CI、缓存、NSIS 收集与原生验收统一新布局；普通/acceptance 覆盖无损（本地门禁逐条 exit 0，生产与验收包本机实跑）。
- [ ] 生产包与验收包已取得当前提交的实跑证据（6.6）；**固定旧版升级仍未执行**——属一次性 Actions Runner，待用户创建 PR 后由 Actions 提供。
- [ ] 旧 target 已安全清理，或明确记录被占用/共享而暂缓（已测量，用户 2026-10-15 决定暂缓并登记为待清理）。

## 4. 三份计划共同的执行与交付约束

本节同时适用于 PR2、PR3。任务表中的“未开始”不能因为计划写好了而改为完成。以下是执行步骤，不是本轮已经取得的证据。

### 4.1 开工与逐项执行

1. 读取 AGENTS.md、CLAUDE.md、当前计划；涉及界面再读 DESIGN.md。执行 `git status --short`、`git branch --show-current`、`git log -1 --oneline`，记录基线。存在他人改动时不暂存、不覆盖、不重置。
2. PR1 在用户确认实施后按页首创建实施分支；PR2 必须确认 PR1 已合入 main，PR3 必须确认 PR2 已合入 main，再按各自页首建分支。不得并行实施三个 PR，也不得把后一个 PR 的功能提前塞入前一个。
3. 按任务编号执行，每项拆成“补测试 → 单独运行确认红灯 → 实现 → 定向验证 → 检查差异 → 提交”。前置任务未通过时只允许修复该前置任务，不继续堆积后续功能。
4. 测试红灯须命中目标断言或预期缺失接口；语法错误、缺工具、网络失败不算有效红灯。先保存失败输出，再改实现；不允许修改期望值、放宽阈值或跳过用例来获得绿灯。
5. 原有统计不变量继续成立：事实求并集、四 token 桶、模型/项目身份、价格冻结、查询分页、CCS 手动导入均不得顺带改动。新增状态或路径元数据不能成为请求去重键。
6. 自动测试使用合成来源和临时 history/settings/pricing 目录；注入全部真实来源入口，失败时也不能回落用户目录。真实数据性能测试单列 opt-in；本机不运行真实安装/卸载，不触发真实系统休眠。
7. 新测试计数器、延迟、故障注入只在测试或 acceptance 构建中存在。发布构建不含 acceptance；不要通过生产环境变量开启测试后门。
8. 任务提交只暂存本项文件。禁止跳过 hook、擅自关闭 CI、降低验收阈值、删除旧失败证据、强推改写历史。未知现状先核实；无法验证的项目写“待验＋原因”，不推断通过。

### 4.2 证据格式与四级门槛

每项在本计划的执行记录中增加：`任务编号 | 实现 commit | 验证 HEAD | 命令 | 预期断言 | 实际结果/退出码 | 证据位置 | 未闭合项`。原始输出存 `qa-artifacts/<pr-name>/<head>/<task>/`（ignored）；文档仅登记摘要，不提交真实日志。执行模型必须回填实际命令和路径，不能留下占位符冒充证据。

| 阶段 | 必须满足 | 不能替代的证据 |
| --- | --- | --- |
| 单项实现通过 | 目标反例先失败、修复后通过；相关回归通过；无越界改动 | 全量测试绿不能代替目标用例 |
| 本地交付通过 | 下列完整门禁逐条 exit 0；所有本 PR 必需项闭合；版本声明通过 | 单测不能代替生产构建或原生 E2E |
| 合并就绪 | 同一最终来源 HEAD 的 CI、安装升级、Native UI E2E 全绿；工件可下载且属于本次构建，关键断言确实执行 | queued、skipped、旧 HEAD 绿、上传空目录均不算通过 |
| 发布完成 | 合并后的 main SHA 自身 gate/build/install/publish 成功；版本标签、资产及校验和对应本次产物 | PR 绿或已合并不等于发布成功 |

即使合并就绪也不自行合并，须遵守用户当时授权。真实跨午夜/长时间休眠/跨显示器只引用独立 todo；旧 target 清理若未获授权可单独待办，不因此声称空间已释放。

PR1 完成迁移后，三个 PR 收尾都逐条执行以下命令（PowerShell 每条执行后检查 `$LASTEXITCODE`；非零立即停，不让后续成功覆盖失败）：

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets --features acceptance -- -D warnings
cargo test --locked --workspace --features acceptance
python -B -m unittest discover -s scripts -p "test_*.py"
pwsh -NoProfile -File scripts/test_install_helpers.ps1
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
pnpm --dir frontend test:e2e
python -B scripts/bump_version.py --check --base-ref origin/main
```

构建输入清单另按 W05c 从 `--begin` 到两种构建再 `--check` 完整执行，不能对旧 target 直接运行 check 当作本轮验证。CI 当前已有的其他脚本契约不得因本命令表未逐个列出而删除。命令因迁移需要改变时，同步脚本、CI 和本计划，记录原因。

## 5. PR1 具体落地顺序与阻断断言

### 5.1 W01–W04：先迁移结构，再改调用方

1. W01 保存两份 `cargo metadata --no-deps --format-version 1`、两份锁的包身份及版本工具测试基线；为版本工具加入旧布局/新布局临时 Git 仓库。旧实现对新布局必须失败，旧布局历史查询必须保持通过。
2. W02 先 `git mv` 源码及 fixtures，再改三个 manifest 和根锁。立即核对根 manifest 没有 package、成员恰为 tokenscope 与 tokenscope-app，且核心不依赖壳/Tauri。失败先修依赖图，不继续批量改脚本。
3. 修改源码相对路径时逐个定位引用方：核心测试原有同级 fixtures 通常不变；跨 crate 引用和仓库根资源需要重算。禁止全仓替换 `../`。以真实加载 fixture 的内容断言验证，不只检查路径字符串存在。
4. W03 统一当前版本读取入口与历史 ref 布局探测；分别测试缺文件、损坏 TOML、Git 查询失败。只有“该 ref 确实没有新布局文件”才允许旧布局读取，其他错误不得回退。
5. W04 逐个更新 workflow/mjs/release 调用点；表中每项记录“已改＋测试”或“无需改＋依据”。检索残留 `src-tauri/target`、子 Cargo.lock 与旧源码路径，分类为有效入口/历史材料；有效入口必须零残留，历史材料不改写。
6. W06 在干净 runner 验证不带 target 的验收 exe 与带三元组的生产 NSIS 两条路径。实际文件存在、hash 清单匹配、安装/升级执行通过，才允许关闭产物路径任务。

### 5.2 W05c：先实现生成器，再让 hook 使用清单

实现顺序固定为 `test_build_inputs.py → build_inputs.py → build-inputs.json → test_pre_commit_checks.py → pre_commit_checks.py → hook/CI`。不要先跳过文档测试，再补输入识别。

建议最小 schema 如下；模式字段是必需契约，字段名称可随实现统一调整并有 schema 校验：

```json
{
  "schema_version": 1,
  "generated": [
    {
      "path": "src-tauri/src/acceptance_csp.js",
      "kind": "file",
      "consumers": [{ "package": "tokenscope-app", "mode": "acceptance" }]
    }
  ],
  "manual": []
}
```

1. 把 `.d` 解析、build-script output 解析、路径规范化、Git 跟踪过滤、记录合并做成纯函数；用合成 fixture 验证，不在这些单测中编译真实工程。Makefile 多目标/转义/续行不能按空格简单拆。
2. 编排层解析 workspace metadata，再记录本次普通/acceptance compiler-artifact 与 build-script-executed。两次编译各自立即采集证据；只归属本地 package_id，不根据 `.d` 文件名猜包名。不把两模式产物最后留下的一个文件当作两份证据。
3. 同一输入的 consumer 按 `(package, mode)` 去重排序；路径/类型相同才合并。未知模式、损坏 schema、重复冲突记录明确拒绝；生成器失败不得写一份空清单覆盖旧清单。
4. `--update` 只在成功收齐两模式、过滤和校验后原子替换清单。`--check` 只比较，不修改受跟踪文件；输出差异路径及包/模式，退出码非零。缺证据、构建失败、只有旧缓存与确实零输入必须可区分。
5. 防止 CI 绿而清单没验证：在临时 fixture 新加一个 include 输入但不更新清单，`--check` 必须失败；更新后通过；删除输入、改变消费模式再次失败。保存三次实际输出。

### 5.3 W05a/W05b：分类器不能成为免检漏洞

推荐流水线：`读取 staged diff/HEAD 与 index 清单 → 规则并集 → 展开检查命令 → 按检查身份去重 → 部分暂存检查 → 执行`。分类层只返回选择结果和原因，不执行 Cargo；执行层接收 argv 数组，禁止字符串拼接命令。

| 临时仓库变更 | 必须选中 | 必须排除/失败行为 |
| --- | --- | --- |
| 普通计划 Markdown | 暂存文档检查 | 不启动 Cargo/pnpm |
| 仅 acceptance_csp.js | 壳 acceptance clippy/test | 只跑 default 算失败 |
| 核心 Rust | workspace 两模式 clippy/test＋fmt | 只测核心而漏壳算失败 |
| 核心 fixtures 的运行时 JSONL 或嵌入 SQL 单独改动 | workspace 两模式 clippy/test；测试清单有/无该路径两种情况 | 正常清单下不因运行时文件未登记而多跑前端；不得只测核心漏壳 |
| fixtures 下 Markdown、新增/删除/跨目录重命名 | fixtures 规则先于文档豁免；新旧路径影响取并集 | 不得以扩展名或旧路径逃过检查 |
| 壳 Rust | 壳两模式 clippy/test＋fmt | 不凭文件名猜 acceptance |
| public/theme-boot.js | 前端检查 | 不落入纯静态资源豁免 |
| 清单/选择器/hook/Cargo 配置 | 完整模式 | 禁止由自身变更降低验证 |
| 清单丢失、坏 JSON、未知 schema/类别 | 完整模式并解释 | 不能当作文档提交通过 |
| 旧路径重命名为文档，或删除登记 | 新旧路径、HEAD/index 清单并集 | 不能只检查新路径 |
| 被选检查输入有未暂存修改 | 明确阻止并列文件 | 禁止自动 stash/reset/add |

临时 Git 仓库必须真实调用 hook；失败替身必须阻止 commit，成功替身记录 argv 与调用次数。只测试分类函数、不测试 hook 接线，不满足 W05。首次替换真实 hook 的提交强制 full 并保留日志。

## 6. PR1 执行记录（2026-10-15）

实现分支 `feat/workspace-layout`，起点 `94e5c52`（`docs/source-discovery-plan` 最新提交，含 c16e023）。本机环境：cargo/rustc 1.99.0、Python 3.14.3、pnpm 11；Rust 门禁全部以 `--offline --locked` 执行。原始输出保存在 ignored 的 `qa-artifacts/workspace-layout/94e5c52/`，本节只登记摘要。

| 任务 | 实现 commit | 验证 HEAD | 命令 | 预期断言 | 实际结果 | 证据位置 | 未闭合项 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| W01 | abc4ef5 | 94e5c52 | `python -B -m unittest discover -s scripts -p "test_bump_version.py"` | 新布局下版本工具必须按核心 manifest 与唯一锁工作 | 18 项 `KeyError: 'package'`（旧实现读不到根虚拟 manifest 的版本），命中预期缺失接口 | `W01/red-bump-version.txt` | 无 |
| W02 | a26961f、c0497ca | 94e5c52 | `cargo metadata --offline --locked --format-version 1`；`cargo check --offline --locked --workspace`；`cargo test --locked --workspace` | 恰好两个成员、默认成员含两者、target 为根 target；check 与 test 通过 | 成员与默认成员均为 `tokenscope@0.3.1` + `tokenscope-app@0.3.1`，`target_directory` 为仓库根；check exit 0（3m29s）；test 552 项通过 | `W01/metadata-unified.json`、`W02/cargo-check-exit.txt`、`W02/cargo-test-summary.txt` | 无 |
| W02 锁与依赖 | a26961f | 94e5c52 | 自写比较脚本比较三份锁的 name/source/version/checksum | 第三方不得漂移 | 统一锁与原壳锁 467 个包身份零差异；原根锁 156 个包全部包含其中 | `W01/lock-diff-shell-vs-unified.json` | 无 |
| W02 依赖方向 | a26961f | a26961f | `cargo tree --workspace -d` | 只保留合法的多版本依赖 | 50 行重复均为第三方内部多版本（如 base64 0.22/0.23），无环、无核心→壳依赖 | `W02/cargo-tree-duplicates.txt` | 无 |
| W02 跨 cwd | a26961f | a26961f | 根目录与 `crates/tokenscope-core` 各跑 `--test project_path_attribution` | 不依赖启动工作目录 | 两处均 16 项通过 | `W02/cwd-root.txt`、`W02/cwd-crate.txt` | 无 |
| W03 | abc4ef5 | abc4ef5 | `test_bump_version.py`、`test_release_metadata.py`、`python -B -m unittest discover -s scripts -p "test_*.py"` | 单锁契约与历史布局探测 | 16 项通过；全部脚本契约 82 项通过 | `W03/green-bump-version.txt`、`W03/python-all.txt` | 无 |
| W04 | 6f6f4dc | 6f6f4dc | `git grep -n "src-tauri[/\\\\]target" -- ':!docs'` | 有效入口零残留 | 非 docs 命中为 0；`src-tauri/Cargo.lock` 引用只余历史文档 | `W01/grep-old-target.txt` | 无 |
| W05a/W05b | 6f6f4dc | 6f6f4dc | `test_pre_commit_checks.py`、`test_check_staged_docs.py`；真实提交触发 hook | 分类正确、缺工具明确失败、临时仓库与调用者仓库隔离 | 选择器 30 项、文档检查 15 项通过；首次切换 hook 的提交（a26961f）实际跑完整门禁并通过 | `W05/test-pre-commit.txt`、`W05/test-check-staged-docs.txt` | 无 |
| W05c | abc4ef5 | abc4ef5 | `build_inputs.py --update` / `--check`；`test_build_inputs.py` | 生成后与本次构建零差异；漏登记必须失败 | 清单 5 项；纯函数 31 项通过；正向 check 零差异；新增真实 include 输入后 check 非零退出并同时报出「清单漏登记」与「install-test.yml paths 未覆盖」 | `W05/build-inputs-update.txt`、`W05/build-inputs-check-final.txt`、`W05/build-inputs-check-negative2.txt` | 无 |
| W06 | — | — | 只读测量（见 6.1） | 删除前必须取得用户确认 | 未执行删除；本轮按用户决策登记为待清理 | `W06/`（本机测量另见 6.1） | 隔离 Runner 验收、生产包与旧版升级实跑待 CI；旧 target 待后续授权清理 |
| W07 | 4488b56 | 4488b56 | `python -B scripts/bump_version.py --bump patch --summary …`；`--check --base-ref origin/main` | 四配置＋一锁＋声明原子更新，版本 0.3.2 | bump exit 0；check 通过（previous 0.3.1 / base e34495b）；分支已推送 | 本提交 | 远程门禁与 PR 未创建（用户决策） |

### 6.1 旧 target 现状（只读测量，等待确认后清理）

| 目录 | 大小 | 实际类型 | 最近写入 | 仓库配置引用 | 进程占用 |
| --- | --- | --- | --- | --- | --- |
| `<仓库>/src-tauri/target` | 38.85 GB（32326 文件） | 真实目录（非 junction/符号链接） | 2026-10-08 | 零（`src-tauri/Cargo.toml` 已无独立 workspace，代码与 workflow 已指向根 target） | 无（测量时仅本次迁移产生的 cargo/rustc 在根 target 工作） |
| `<仓库>/target` | 37.71 GB（49524 文件） | 真实目录 | 迁移后持续写入 | 默认共享输出，保留 | 当前构建使用中 |

两目录相加约 76.5 GB，与计划开头的估算一致，但**清理只涉及 `src-tauri/target`**：根 `target/` 是统一后的默认输出，不能清理。另需注意仓库存在第二个 worktree（`~/.config/superpowers/worktrees/tokenscope/source-discovery-plan`，detached HEAD `3ffea3e`），它有自己的构建目录，不在本次清理范围。

**用户 2026-10-15 决策：暂缓清理。** 本轮不删除 `src-tauri/target`，登记为待清理项，也不声称空间已释放；后续若要清理，仍按 W06.7 的核对流程执行（确认路径在预期仓库内、无进程占用、单一 PowerShell 文件操作、记录实际释放空间、仍占用则登记不强杀实例）。清理脚本已就绪并演练过 dry-run（`qa-artifacts/workspace-layout/tools/cleanup_legacy_target.ps1`，默认 dry-run、只有 `-Execute` 才删除）：2026-10-15 演练输出 `exists=true / isInside=true / linkType=Directory / files=32326 / gib=38.85 / occupancy=[] / action=dry-run：未删除`，未触碰任何文件；授权后一条命令即可执行并记录实际释放空间。

**用户 2026-10-15 决策：暂不创建 PR。** 分支已推送到 `origin/feat/workspace-layout`（HEAD `4488b56`），本轮不产生远程 CI、安装升级与 Native UI E2E 证据，因此不报告“合并就绪”；这些门槛的实跑证据待用户确认后由 Actions 提供。

### 6.3 W04 逐项结论（§5.1.5 要求每项记录「已改＋测试」或「无需改＋依据」）

| W04 表格项 | 结论 | 依据 / 验证 |
| --- | --- | --- |
| `frontend/scripts/native-acceptance.mjs` | 已改 | `EXE_DEFAULT` 改为根 `target/release/tokenscope.exe`；c16e023 的输出路径与日志复制修复保留。验收包构建后 exe 落在根 target，原生 E2E 的 `evidence.json` 记录的正是该路径（见 6.6） |
| `.github/workflows/ui-e2e.yml` | 已改 | 构建命令改为 `cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope`，显式 GUI package、release 与两 feature，产物位置与脚本默认路径一致 |
| `.github/workflows/release.yml` | 已改 | NSIS 与 exe 收集改为 `target/x86_64-pc-windows-msvc/release/...`，保留显式目标三元组；本机 production 构建 + `release_assets.py collect` 以同一路径实测通过（见 6.6） |
| `.github/workflows/install-test.yml` | 已改 | 与 release.yml 使用同一产物路径规则，仍收集并校验同一个待发布安装包；paths 过滤从 `src/**` 改为 `crates/tokenscope-core/**` |
| `scripts/release_assets.py` 及测试 | 无需改 | 收集接口以 `--bundle`/`--executable` 参数接收路径，本身不含仓库布局假设；唯一直接读取的 `src-tauri/tauri.conf.json` 未迁移，仍是正确路径。`test_release_assets.py` 全绿 |
| `.gitignore` | 已改 | `target` 规范为 `target/`，保持所有层级目录忽略（含历史子目录遗留），未缩成仅根 `/target` |
| 工作流 paths、脚本引用、当前开发文档 | 已改 | `install-test.yml` paths 已更新；`CLAUDE.md`、`README.md`、`docs/stats-semantics.md` 的源码/测试路径改指 `crates/tokenscope-core`；历史 audit/review 记录不改写事实 |
| `src-tauri/target` 残留检索（§5.1.5） | 已核对 | 非 docs 命中为 0；仅历史报告保留旧路径作为当时证据 |

### 6.4 本地交付门槛（计划 §4.2 命令表，逐条执行，非零即停）

`qa-artifacts/workspace-layout/pr1-local-gate.txt`（脚本 `qa-artifacts/workspace-layout/tools/local_gate.ps1`）。全部 14 条 exit 0，未见“后续成功覆盖失败”的情形：

| 命令 | 结果 | 耗时 |
| --- | --- | --- |
| `cargo fmt --all -- --check` | exit 0 | 1.1s |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | exit 0 | 1.3s |
| `cargo test --locked --workspace` | exit 0，548 通过 / 10 忽略 | 74.6s |
| `cargo clippy --locked --workspace --all-targets --features acceptance -- -D warnings` | exit 0 | 1.0s |
| `cargo test --locked --workspace --features acceptance` | exit 0，553 通过 / 10 忽略 | 71.4s |
| `python -B -m unittest discover -s scripts -p "test_*.py"` | exit 0，159 通过 | 63.7s |
| `pwsh -NoProfile -File scripts/test_install_helpers.ps1` | exit 0 | 17.2s |
| `pwsh -NoProfile -File scripts/test_native_window_action.ps1` | exit 0 | 2.5s |
| `pnpm --dir frontend typecheck` | exit 0 | 12.2s |
| `pnpm --dir frontend format:check` | exit 0 | 5.7s |
| `pnpm --dir frontend test` | exit 0，480 通过（40 文件） | 40.9s |
| `pnpm --dir frontend build` | exit 0 | 16.1s |
| `pnpm --dir frontend test:e2e` | exit 0 | 172.6s |
| `python -B scripts/bump_version.py --check --base-ref origin/main` | exit 0（0.3.1 → 0.3.2） | 0.3s |

两种 Rust 构建模式的成本（本机热缓存）：普通与 acceptance 的 clippy 各约 1s、test 各约 71–75s；冷缓存下的隔离 checkout 见 6.5。

本轮另补两组契约测试（不属于 §4.2 命令表，但对应 §5.1.5 与 §5.2 的必要契约）：

- `scripts/test_build_paths.py`（10 项）：发布与安装的 `release_assets collect` 参数必须一致且指向 `target/x86_64-pc-windows-msvc/release/...`；`release_assets.py` 自身不含仓库布局假设；`native-acceptance.mjs` 的 `EXE_DEFAULT` 为根 `target/release/tokenscope.exe` 且 `--exe` 覆盖仍可用；`ui-e2e.yml` 用 `-p tokenscope-app` 构建且不带 `--target`；rust-cache 只映射 `. -> target`；checks action 的 fmt 与 workspace 轮次各只出现一次、含清单校验与新增契约；install-test paths 覆盖核心 crate、不含旧根路径；`.gitignore` 为 `target/`；有效入口零 `src-tauri/target` 残留；受版本管理的锁只有根一个。该测试先跑出红灯（CI 未接线），接入 Selector and build-input contracts 步骤后转绿。
- `test_build_inputs.py` 新增 3 项 release 风格 dep-info 组合契约：只保留合法跟踪输入，被排除项按「固定规则覆盖或构建生成物」记录原因；仓库外路径与空目录输入同样被丢弃。
- `test_pre_commit_checks.py` 新增 1 项钩子接线契约：临时仓库把 `core.hooksPath` 指向受版本管理的 `.githooks`，用替身 `scripts/pre_commit_checks.py` 控制退出码——替身返回 7 时真实 `git commit` 被阻止且不产生提交，返回 0 时提交成功。钩子脚本本身不替换。
- 符合性复核（2026-10-15 第 4 轮）补齐 W05a 表格中尚未实现的两项：其一，`check_staged_docs.py` 增加「只对本次新增/修改行报告行尾空白与 CR」的空白检查（历史行不追溯），`test_check_staged_docs.py` 由 15 项增至 18 项；其二，新增 `scripts/check_powershell_syntax.ps1`（解析 `scripts/` 与 `frontend/scripts/` 下全部 `.ps1`，当前 11 个脚本通过）与 `test_script_checks.py`（5 项：坏脚本报错并给出位置、好脚本放行、仓库脚本全部通过、脚本规则同时覆盖 Python 契约/PowerShell 契约/语法检查、命令保持 argv 数组），并把 `scripts-contracts` 的选中集合从 1 条扩展为 5 条（Python 契约、语法检查、`test_install_helpers.ps1`、`test_upgrade_helpers.ps1`、`test_native_window_action.ps1`），CI 的 Selector and build-input contracts 步骤同步接入这两个检查。
- 脚本契约总数 159 → 189 项，全部通过。
- 第 5 轮逐条复核再补 8 项：`test_pre_commit_checks.py` 的「无 HEAD 的首次提交按空树基线读取并正常分类」（实测 `git diff --cached --name-status -z --find-renames` 在无 HEAD 时自动与空树比较）与「被选检查不写用户数据目录 `~/.tokenscope`」；`test_build_paths.py` 的 6 项仓库不变量——根 manifest 是虚拟 workspace（无 package/dependencies/exclude/version 键）、壳 manifest 无独立 `[workspace]` 且 `tokenscope` 依赖指向 `../crates/tokenscope-core`、共享依赖集中在根并由成员以 `workspace = true` 继承且各自保留 features、核心库三个依赖段均无 Tauri 依赖、根锁恰好两个无 source 本地包且版本与四份版本配置一致、构建输入清单记录全为仓库相对路径且不含 run_id/mtime/盘符。另核对 `--manifest-path` 在有效入口零残留（仅剩测试内的反向断言）。
- CI/Release 的版本门禁事件路径按真实事件结构在本机模拟（`qa-artifacts/workspace-layout/W07/version-gates.txt`）：PR 事件（`ci.yml` gate）与 main push 事件（`release.yml` gate）各跑 `bump_version.py --check --event-file`，均 exit 0（version 0.3.2、previous 0.3.1、kind patch、base e34495b）；`release_metadata.py` 以 `GITHUB_REF=refs/heads/main` 实跑输出 `tag=v0.3.2 / prerelease=true / latest=false`。事件文件需为无 BOM 的 UTF-8（GitHub 提供的事件文件如此）；本机用 `Out-File -Encoding utf8` 生成的带 BOM 副本会被明确拒绝并报 `Unexpected UTF-8 BOM`，属预期失败而非静默通过。

### 6.5 隔离验收与旧入口核对（W06.1/W06.5）

`qa-artifacts/workspace-layout/W06/isolated-checkout.txt`（脚本 `tools/w06_isolated_checkout.ps1`）。隔离目录为 `%TEMP%\tokenscope-pr1-isolated`，从本分支 `git clone --local` 得到，克隆后仓库内 target 目录数为 0：

| 步骤 | 结果 | 耗时（冷缓存） |
| --- | --- | --- |
| `cargo metadata --offline --locked --no-deps` | exit 0 | 0.1s |
| `cargo clippy --locked --offline --workspace --all-targets -- -D warnings` | exit 0 | 113.5s |
| `cargo test --locked --offline --workspace --features acceptance --no-run` | exit 0 | 199.9s |
| 从 `src-tauri` 目录发起 `cargo check -p tokenscope-app`（旧入口） | exit 0 | 8.1s |

布局核对：`childTargetCreated = false`、`childLockCreated = false`、`rootTargetCreated = true`、受版本管理的锁恰为 `Cargo.lock`。即在没有任何旧编译产物的 checkout 里，从根目录与 `src-tauri` 目录发起构建都只写根 `target/`，不新建子 target 或子锁。

### 6.6 生产构建、资产收集与原生 E2E（W06.2/W06.3）

`qa-artifacts/workspace-layout/W06/build-and-acceptance.txt`（脚本 `tools/w06_build_acceptance.ps1`），四步全部 exit 0：

| 步骤 | 结果 | 耗时 |
| --- | --- | --- |
| 生产 `tauri build`（不带 acceptance，NSIS，显式 `x86_64-pc-windows-msvc`） | exit 0；产物 `target/x86_64-pc-windows-msvc/release/bundle/nsis/TokenScope_0.3.2_x64-setup.exe`（5.31 MiB） | 334.6s |
| `release_assets.py collect` 与哈希契约 | exit 0；产出 `TokenScope_v0.3.2_windows-x64-setup.exe`（5565434 字节）、`SHA256SUMS.txt`、`build-metadata.json`（含 exe 与解包后 exe 的 sha256） | 0.9s |
| 验收包构建（`cargo build -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope`，不带 `--target`） | exit 0 | 229.3s |
| 原生 E2E `ui-refresh`（真实 WebView2 + 隔离根） | exit 0，10 项断言通过；`evidence.json` 记录的 exe 是根 `target/release/tokenscope.exe` | 143.3s |

写入位置核对：本次构建后根 `target/release/tokenscope.exe` 大小为 20030464、写入时间 2026-10-15 14:26；旧 `src-tauri/target/release/tokenscope.exe` 仍是 10-13 的 20029440，未被本次构建触碰——更换构建入口后没有回到旧布局。

本机不运行真实安装/卸载脚本：W06.4 的五项安装验收与两项固定旧版升级属于一次性 Actions Runner，且本轮按用户决策未创建 PR，因此该部分不构成本轮证据。

### 6.8 复核反例修复（2026-10-15 第二轮复核）

外部复核在 `4a54074` 上提出 5 项反例（2×P1、3×P2）。逐项复现确认后修复，并补 8 项回归测试，脚本契约 189 → 197 项：

| 反例 | 根因 | 修复 | 回归测试 |
| --- | --- | --- | --- |
| P1 `git commit --only <path>` 时检查器看不到本次提交的内容 | 钩子 `unset GIT_INDEX_FILE`，检查器又剥离 `GIT_*`，于是读默认 index 而不是 Git 为 `--only` 准备的临时 index，输出「暂存区没有文件」后放行 | 钩子保留 Git 传入的环境（`--only` 依赖它）；实现层不再剥离；隔离改到测试侧（`setUpModule` 删除 `GIT_*`），使自建临时仓库与调用者互不影响 | `test_only_commit_exposes_the_paths_git_prepared_for_that_commit`（真实钩子 + 回显替身，断言 `STAGED=[Cargo.toml|]`） |
| P1 部分暂存保护不全，full 模式直接跳过 | 保护范围只按检查前缀（`src-tauri/src/`），漏掉共享配置与清单登记的编译输入；`main` 的 full 分支把 `blocked` 置空 | 新增 `RUST_INPUT_SCOPES`（`crates/`、`src-tauri/`、`Cargo.toml`、`Cargo.lock`）并并入清单登记路径；`classify` 增加 `force_full`，full 与其他模式共用同一套保护 | `test_unstaged_shared_configuration_and_compiled_inputs_block_rust_checks`、`test_forced_full_mode_uses_the_same_unstaged_guard` |
| P2 清单过滤把目录前缀当成成员资格 | `filter_tracked` 对「位于含跟踪文件的目录下」的文件也放行，只跟踪 `docs/privacy.md` 时 `docs/untracked.md`、`docs/empty/` 都被保留 | 改为精确成员资格：文件须在 `tracked_files`，目录须在 `tracked_dirs` | `test_directory_prefix_is_not_membership_for_file_inputs` |
| P2 前端验收脚本改动没有选到脚本检查 | `frontend/scripts/` 走的是前端四项，未按 §W05a「E2E 编排并入脚本规则」处理 | 新增 `scripts/check_node_syntax.py`（`node --check` 校验 `frontend/scripts/**`，当前 14 个脚本）与 `node-syntax` 检查项；`frontend/scripts/` 改动选前端四项 + `node-syntax`，CI 同步接入 | `test_frontend_acceptance_script_selects_script_syntax_check` |
| P2 删除被引用文件时未检查未修改文档 | `main` 只看暂存的 Markdown 变更；目标被删而引用方未改时直接返回「没有 Markdown 变更」 | 新增 `removed_paths`（D 路径与 R 旧路径）与 `documents_referencing`（先按基本名粗筛再解析链接，覆盖未修改文档）；`main` 在没有 Markdown 变更但存在移除路径时同样执行 | `test_removing_a_linked_target_is_reported_from_unchanged_documents`、`test_renaming_a_target_is_reported_from_unchanged_documents`、`test_removed_target_without_references_reports_nothing` |

复现与验证：修复前在本机用真实钩子复现为 `STAGED=[]`（提交却包含 `Cargo.toml`），修复后同一场景为 `STAGED=[Cargo.toml|]`；坏 `.mjs` 脚本可被 `check_node_syntax.py` 抓出（exit 1）。

按同样的复核方式自查，补 5 项契约（脚本契约 197 → 202 项）：

- **`--only` 端到端**：临时仓库里放仓库本体的 `pre_commit_checks.py` / `build_inputs.py` / `check_staged_docs.py`（不放替身），挂上受版本管理的 `.githooks`，对「已跟踪文件在工作区被改坏但没有 `git add`」执行 `git commit --only <path>`。坏链接文档被查出（输出含 `nope.md`）并阻止提交，正常改动放行——这条链路同时证明「选中了检查」与「检查确实执行」，比只回显暂存区的替身更强。
- **强制完整模式的前端未暂存差异**同样阻止（完整模式会跑前端检查）。
- **清单已登记的 fixture**（`schema-v3.sql`）与运行时 fixture 走同一套 workspace 规则，且不牵连前端检查。
- **fixture 重命名进 `docs/`** 时新旧路径规则取并集（fixtures 规则 + 文档检查）。

第三轮复核另提三项（同样逐项复现后修复，脚本契约 204 → 211 项）：

| 反例 | 根因 | 修复 |
| --- | --- | --- |
| 未暂存保护的范围不对：工具链配置文件漏配、清单登记的目录按精确路径匹配、纯文档提交反被编译输入误阻 | 输入范围硬编码且不区分 file/directory；清单输入不按消费方筛选 | 新增 `RUST_SHARED_INPUTS`（`crates/`、`src-tauri/`、`Cargo.toml`、`Cargo.lock`、`rustfmt.toml`、`rust-toolchain.toml`、`.cargo/`）；`_manifest_inputs` 按消费方筛选并保留 kind，目录项按子树匹配；文档检查只读 index，不再被编译输入的未暂存改动阻止 |
| 删除/改名没有触发文档检查，引用检查形同闲置 | 分类器只按路径分类，D/R 状态不会选中 docs | `classify` 对 D/R 状态额外加入文档检查并记录 `deleted-or-renamed` 触发原因 |
| 前端脚本只做语法检查，未满足「E2E 编排并入脚本规则」 | `frontend/scripts/` 统一映射为前端四项 + node-syntax | 按消费者映射：浏览器验收脚本（`check-*`、`run-ui-refresh-e2e.mjs`）→ 前端四项 + node-syntax + `browser-e2e`（`pnpm test:e2e`）；`native-acceptance.mjs`、`contracts/`、`lib/`、`fixtures/` 与未登记脚本无本地等价契约 → 完整模式兜底；`browser-e2e` 同时纳入完整模式 |

链路测试按要求覆盖「分类器 → 实际检查器」：端到端用例用仓库本体的选择器与检查器，在临时仓库里删除一个被引用的文件，提交被真实文档检查阻止（输出含引用方文件名），无人引用时放行。

第四轮复核提出执行顺序问题：`expand_commands` 按检查名称展开，`browser-e2e` 排在 `frontend-build` 之前，而 E2E 入口用 Vite preview 直接读取已有 dist、自己不构建——可能验证旧产物后放行新改动，干净环境还会因缺 dist 失败；选中模式与完整模式都有该问题。

修复：新增 `CHECK_ORDER` 与 `CHECK_DEPENDENCIES`（`browser-e2e` 依赖 `frontend-build`），`_ordered_checks` 按依赖拓扑展开、补齐被选中检查缺失的依赖并检测依赖成环；`run()` 仍在首个非零退出时停止，因此构建失败时 E2E 不会执行。补 5 项顺序契约（脚本契约 211 → 216 项）：selected 与 full 两模式都要求 build 先于 E2E、单独选 `browser-e2e` 时自动带上构建、构建失败后 E2E 不执行、共享依赖对多个消费者只展开一次。

### 6.7 过程事件（已在当轮修复）

测试用的临时仓库操作一度污染了真实仓库 `.git/config`（`bare = true` 与被 fixture 身份覆盖的 `[user]` 段），原因是当时为验证钩子环境而手动注入了绝对路径的 `GIT_DIR`/`GIT_INDEX_FILE`。执行方当时记录分支、HEAD、reflog、index 与工作区内容未受影响，配置已按备份复原（身份回退全局 `~/.gitconfig`）。最初清理钩子环境的方案后来被 §6.8 的 `--only` 反例推翻；当前钩子与提交检查器保留本次提交的 Git 环境，由自建临时仓库的测试隔离 `GIT_*`，不能再次统一 unset。原始事件记录见 `qa-artifacts/workspace-layout/incident-2026-10-15/README.md`（ignored），其中初版修复描述属于历史记录。

### 6.9 第 15 节审阅修复

- PowerShell 语法检查器显式输出无 BOM UTF-8；测试保持严格 UTF-8 解码，覆盖主动设置 GBK 的子进程成功输出及失败 stderr。仓库范围测试不再传空临时根；PowerShell 与 Node 两种检查器遇到 0 个脚本均失败，测试按真实脚本集合核对数量（本轮为 11/14）。
- 库 `.rlib`/`.rmeta` 产物去掉 Cargo 的 `lib` 文件前缀再定位 `.d`，不会删除同名二进制自身的 `lib` 前缀；只有库产物、没有单测二进制的用例也能收集输入。
- 人工登记独立校验规范相对路径、文件/目录类型、Git 跟踪资格及消费方；生成项仍单独比较，人工项不能掩盖漏登记。工作流覆盖检查同时包含人工项。CLI 与编排层共享异常类型，缺清单时返回清晰非零错误而非堆栈。
- 未跟踪且未被忽略的文件也参与检查输入保护；只有命中实际选中范围才阻止，纯文档提交不受无关新源码影响，ignored target/QA 不误阻。
- CI 实现偏差明确接受：清单 `--check` 自己 `clean -p` 两个本地包后重新构建普通和 acceptance 两轮，而非复用前面门禁的产物。代价是额外两轮本地包编译；不能宣称实现了构建证据复用。此次保留现有正确性策略，不再改调度。
- 本轮新增回归在 `test_script_checks.py`、`test_review15_regressions.py`；后者接入完整 CI。失败反例先于实现运行，复现了 GBK 解码失败、零文件放行、库依赖遗漏、人工项误报及未跟踪输入遗漏；实际验证结果在完成后回填。

验证：修复提交 `0cbbc8b` 的完整 hook exit 0；脚本契约最终 228 项通过，普通/acceptance workspace fmt、clippy、test 全绿，前端 480 项、typecheck/format/build 全绿，浏览器 E2E 4 场景零违例、5 契约共 178 断言通过，安装/升级/窗口 helper 契约通过。Git Bash 下语法契约 10 项通过，仓库检查器实际检查 PowerShell 11 个、Node 14 个。真实 `build_inputs.py --check` 重编译两模式后返回 generated=5，missing/stale/changed/absent 均空、workflow_uncovered 为空；日志与产物记录在 `qa-artifacts/workspace-layout/review15/manifest-check.txt` 和 `manifest/`。版本检查仍为 0.3.1 → 0.3.2，本 PR 不重复加号。

创建 PR、远程 CI/安装升级/Native UI E2E、旧 target 清理均保持原有待办与授权边界。

