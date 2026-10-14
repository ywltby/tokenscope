# PR1：虚拟 Cargo Workspace、目录迁移与提交检查

> 状态：已按[审阅意见第 11 节](../../reviews/2026-10-15-source-discovery-plan-review.md)补齐设计，待复审；W01–W07 未实施。设计基线 main e34495b / 0.3.1。
> 本轮计划修订留在 `docs/source-discovery-plan`；PR1 开工时从该分支最新提交创建 `feat/workspace-layout`，带入既有 mjs 维护提交 c16e023 和三份计划，不改写历史。仅本 PR 改目录/构建/门禁，独立 patch；合入后才从最新 main 分别启动 [PR2](2026-10-15-collection-status.md)、[PR3](2026-10-14-source-discovery-and-additional-paths.md)，不在本 PR 实施功能。

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
6. 核心源码和 tests 同级关系保持，逐项验证 fixture 实际加载内容；不要把所有 `CARGO_MANIFEST_DIR` 盲目改成仓库根。壳测试通过 `../../tests/fixtures/codex/rate-limit-only.jsonl` 跨 crate 引用的路径必须随迁移更新并被 W05c 清单捕获，不能仅检查核心自己的 fixture。确需仓库根的工具入口显式解析 workspace root，核心测试仍以 crate 根为基准。从根目录和核心 crate 目录分别运行代表性测试，确认不依赖启动 cwd。
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
| 上述目录中的非 Rust 文件/fixture | 已登记输入按清单的包和模式选择；未登记者完整兜底。`acceptance_csp.js` 单独改动必须选中壳 acceptance 检查 |
| `frontend/src/**`、`frontend/public/**`、前端配置及依赖清单/锁 | 前端 typecheck、format:check、test、build；包含测试读取的 public/theme-boot.js；E2E 编排并入脚本规则 |
| `scripts/**` 的 Python/PowerShell、`frontend/scripts/**` | 明确映射的脚本语法和契约测试；共享 helper 取全部消费者契约并集，未登记脚本走完整模式，不只测试被改文件本身 |
| Cargo manifest/锁、`build.rs`、`.cargo/**`、Rust 工具链、Tauri 配置/capabilities、`.github/**`、`.githooks/**`、检查选择器及其测试、发布声明等构建/门禁入口 | 完整模式，包含普通与 acceptance Rust 检查、前端门禁和现有相关脚本契约 |
| 其他未明确分类的文件 | 完整模式兜底，输出未识别路径及原因 |

分类用有顺序的显式规则，构建/门禁入口优先于宽泛目录规则；多类改动取并集、同一检查只执行一次。普通代码最小检查不代表可免除最终完整门禁；完整模式命令与 W05 的 CI 门禁维护同一语义，不复制旧双 workspace 冗余命令。本地完整模式禁止运行真实安装/卸载/休眠，只运行安全契约与隔离验收；安装升级仍由一次性 Actions Runner 承担。

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

依据：[Cargo dep-info](https://doc.rust-lang.org/cargo/reference/build-cache.html#dep-info-files) 与 [构建脚本输出](https://doc.rust-lang.org/cargo/reference/build-scripts.html#outputs-of-the-build-script)。生成清单时必须覆盖 source/fixture 迁移后的路径，旧路径不能遗留为隐式豁免。

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
| W01 基线与失败契约 | 未开始（已做只读初步配置核查） | 尚无迁移测试 |
| W02 单 workspace 与锁 | 未开始 | — |
| W03 版本工具 | 未开始 | — |
| W04 路径接线 | 未开始 | — |
| W05 门禁与缓存 | 未开始 | — |
| W06 构建验收与旧目录清理 | 未开始 | — |
| W07 文档与交付 | 未开始 | — |

- [ ] metadata 两个成员、正确默认成员与统一 target_directory。
- [ ] 根是虚拟 workspace，核心源码/测试/fixture 完整迁移，无旧路径副本；跨 cwd 测试、旧/新 Git 基线版本读取均通过。
- [ ] 仅一个受版本管理的 Cargo.lock，两个本地包版本正确，第三方依赖无未解释漂移。
- [ ] 旧 CLI 同名规避配置和当前文档说明全部清除，共享依赖 features 保持正确。
- [ ] 钩子、CI、缓存、NSIS 收集与原生验收统一新布局；普通/acceptance 覆盖无损。
- [ ] 生产包、验收包、固定旧版升级均取得当前提交的实跑证据。
- [ ] 旧 target 已安全清理，或明确记录被占用/共享而暂缓；不把待清理写成已释放空间。

