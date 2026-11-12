# 设置页「关于」与当前安装版本实施计划

> 状态：**已实施，PR #65 待审查**（2026-11-12）。产品版本已按新功能规则从 `0.7.5` 升至 `0.8.0`；尚未合并、发布或在本机安装。
> 执行模型：使用 `executing-plans`、`vue-best-practices`、`test-driven-development` 和 `verification-before-completion`，按下列步骤实施。
>
> **实施记录（2026-11-12）**：
> - 入口与面板：`Settings.vue` 末尾新增「关于 / About」；首次进入才挂载 `AboutPanel.vue`，之后保留。版本来自 `@tauri-apps/api/app` 的只读 `getVersion()`，成功结果在组件生命周期内保留。
> - 状态：加载 `role="status"`、失败 `role="alert"` 显示本地化原因并提供重试；同一时刻最多一条读取，卸载/更新一轮后的迟到结果被丢弃；空白返回值按不可用处理。
> - 语言与布局：zh-CN / en-US 键补齐（`locales.test` 守卫）；宽窗口侧栏与 980×620 顶部分段控件均可进入关于。
> - 副作用：关于只调用 `plugin:app|version`；浏览器契约审计进入关于后新增 IPC，无设置写入、采集、导入或查询。
> - 验证：前端全量 **1140 项通过**；typecheck / format:check / build 通过；浏览器契约 `settings_about_contract` **12 项断言通过**，覆盖明暗主题 × 宽窄窗口、英文切换、失败原因与重试；证据截图在 gitignored `qa-artifacts/ui-adjustment/ci-e2e/settings_about_contract--*.png`。
> - 真实宿主：`native-ui-system` 新增 `about_runtime_version_matches_exe`，把界面显示的 `getVersion()` 与本次受测 EXE 的 `VersionInfo.FileVersion` 对照（fixture 模拟值 `9.8.7` 仅用于浏览器契约，不能替代真实 IPC）。PR #65 最新 HEAD `f30144f` 的 Native UI E2E 三个 ui-system 场景均输出 `ui=0.8.0; exe=0.8.0`。
> - CI：PR #65 最新 HEAD `f30144f` 的 20 项检查成功、1 项按设计跳过（`upgrade-chain`），`CLEAN`；CI 日志确认前端 77 个测试文件通过、`settings_about_contract` 12 项断言通过；Install test 成功。
> - 版本：`bump_version.py --bump minor` 输出 `0.8.0` / `release: true`；升级起点含上一版 `0.7.5`；本机未安装、未发布。

**Goal:** 用户打开「设置 → 关于」即可看到正在运行的 TokenScope 的真实版本，不必查安装包文件名或发布页。

**Architecture:** 沿用现有设置分区导航，新增末尾的「关于 / About」。版本读取与状态展示放入独立 `AboutPanel.vue`，首次进入时挂载，通过 Tauri 自带的只读 `getVersion()` 取得运行时版本；成功结果只在组件生命周期内保留。

**Tech Stack:** Vue 3 / TypeScript / Tauri 2 app API / vue-i18n / Naive UI / Vitest / 现有浏览器界面契约。

## 1. 已核对的基线

- 规划基线：main `fbff40f261415f54987e26589e25e9ab84d4d984`，版本 `0.7.5`。执行时重新 fetch 并核对最新 main，不硬套旧提交号。
- `frontend/src/views/Settings.vue` 当前有 general、sources、usage、pricing、appearance、advanced 六个分区，没有关于入口或版本显示。
- 设置使用分区保留挂载的方式保存草稿；窗口宽度不超过 1100px 时使用顶部导航。新增分区必须兼容这两种布局。
- 当前安装的 `@tauri-apps/api/app` 提供 `getVersion()`，调用只读 `plugin:app|version`；现有 capability 为 `core:default`。优先复用现有权限，不新增自定义 Rust command 或扩大权限。
- 中英文资源分别位于 `frontend/src/lib/locales/zh-CN.ts`、`frontend/src/lib/locales/en-US.ts`。
- 本计划不分配版本、不修改发布声明，也不代表功能已经进入已发布安装包。

## 2. 产品范围与不变量

1. 关于放在设置导航末尾，展示现有 TokenScope 品牌标识、应用名称和「当前版本 / Installed version」。版本文本可以正常选中复制。
2. 版本值来自正在运行的程序；不得写死 `0.7.5`，不得用前端 `package.json`、环境变量或远端最新 Release 的版本冒充。
3. 首次进入关于前不读取版本；进入后显示加载状态，成功后切换分区再返回不重复读取。中英文切换只更新标签，不重新请求版本。
4. 失败展示本地化错误提示和可读原因，提供重试；不显示 `0.0.0`、当前仓库版本或上次安装版本作为回退。浏览器预览中接口不可用也不能伪造版本。
5. 同一时刻最多一条版本读取；重试期间禁用重复触发。卸载后的迟到结果不更新状态，不留下未处理 rejection。
6. 关于只读程序元数据，不写 settings/localStorage，不触发采集、重扫、CCS 导入、价格同步或 `data-changed`。
7. 沿用 DESIGN.md 的卡片、语义色、焦点和字体；品牌图形复用现有标记，不重新设计图标。加载用 status、错误用 alert，错误不能只靠颜色区分。
8. 本批只补关于与版本信息，不加入自动更新、更新检查、联网请求、遥测、构建提交号、系统信息或新的顶级页面。

## 3. A01：先写设置入口的行为红测

**文件：** 修改 `frontend/src/views/Settings.test.ts`。

1. 在现有设置测试装配中 mock Tauri app API，不连接真实宿主。
2. 新增首个行为用例：可以从导航进入关于，并看到运行时返回的版本。模拟值使用与仓库版本不同的值，如 `9.8.7`。
3. 运行该用例，确认旧实现因为没有关于入口而失败，而非导入不存在的组件或测试装配错误。
4. 补入口边界：进入关于前不调用版本 API；返回原分区不丢既有未保存草稿；关于不派发 `data-changed`。

命令：`pnpm --dir frontend exec vitest run src/views/Settings.test.ts`。

## 4. A02：实现关于面板及读取状态

**文件：** 新增 `frontend/src/components/AboutPanel.vue`、`frontend/src/components/AboutPanel.test.ts`；修改 `frontend/src/views/Settings.vue` 和两份语言资源。

1. 在 `SECTION_IDS` / `SECTION_LABEL_KEY` 增加 about，沿用现有 `selectSection`。用首次访问标记延迟挂载 AboutPanel，之后保留挂载；不要改变其他分区生命周期。
2. AboutPanel 管理 loading、version、error 与读取在途状态。首次挂载读取一次；成功保存结果；失败显示重试；只在用户重试时再次读取。
3. 版本空值或无有效文本按不可用处理，不把空白行展示成成功。
4. 加入最小的有效生命周期保护，避免卸载后提交迟到结果。不要引入全局缓存、共享刷新调度器或新依赖。
5. 补齐中英文导航、字段、加载、失败和重试文案，产品文本不写死在模板中；原始错误只作纯文本展示。
6. 使用现有品牌标记和设置卡片样式。无需为这块只读内容重构 MainApp 或整套设置布局。

面板回归必须覆盖：

- 延迟响应时先显示加载状态，成功后显示 `9.8.7`；与仓库版本无关。
- 拒绝后错误可见，重试成功；连续触发重试不并发读取。
- 切换分区、切换语言后仍保留成功结果，不增加请求。
- 中文与英文标签完整；卸载后返回结果不产生未处理错误。
- 只有只读 app API 调用，没有设置写入、采集或导入副作用。

命令：`pnpm --dir frontend exec vitest run src/components/AboutPanel.test.ts src/views/Settings.test.ts src/views/Settings.language.test.ts src/lib/locales.test.ts`。

预期：A01 红测与上述回归全部通过，既有设置、语言及资源结构测试不退化。

## 5. A03：界面与原生接线验收

**文件：** 按现有契约结构最小修改 `frontend/scripts/contracts/interaction-contracts.mjs`、`frontend/scripts/fixtures/ui-contracts.mjs`；必要时在既有设置原生场景补断言，不新增独立工作流。

1. 浏览器 fixture 支持只读 `plugin:app|version`，模拟值与仓库版本不同。只在关于被访问时计数，不给正常浏览器预览虚构安装版本。
2. 验证宽窗口侧栏和窄窗口顶部导航都能进入关于；明暗主题、中英文下版本可读，七个导航项不重叠、不被裁掉。
3. 关于正常显示与读取失败重试各有最小界面断言。优先扩展既有设置契约，避免为此建立另一套测试服务。
4. 若在现有 Native 场景补关于检查，直接对照该运行验收 EXE 的版本来源，证明真实 IPC 可用；不得读取本机用户数据或安装正式包。
5. 本地浏览器契约与 Runner Native 的证明范围分别记录。没有真实宿主验证时，不写成已经证明真实 IPC 成功。

命令：`pnpm --dir frontend test:e2e`。Native 验证复用后续 PR 的现有流程。

## 6. A04：版本、门禁与可审查交付

1. 确认收到实施授权后再修改产品。保留其他会话的文件与暂存内容，不自动 stash、不覆盖、不新建 worktree。
2. 新增关于属于新功能，执行时按 CLAUDE.md 使用版本工具分配 minor；当前基线预期为 `0.8.0`，以最新 main 和工具实际结果为准。同一 PR 补修不重复递增。
3. 使用 `python -B scripts/bump_version.py --bump minor --summary "设置新增关于分区，显示当前运行版本"` 同步版本与发布声明。规划阶段不执行该命令。
4. 核对上一已发布版本的升级基线，已登记且验证通过时复用，不重复造记录。
5. 先做定向验证，再运行 `pnpm --dir frontend typecheck`、`format:check`、`test`、`build`、`test:e2e`，以及仓库选择器和正常提交钩子要求的检查。不得绕过钩子或放宽断言。
6. 运行 `python -B scripts/build_inputs.py --check` 和 `python -B scripts/bump_version.py --check --base-ref origin/main`。若涉及 Rust 测试，使用 `scripts/run_rust_tests.py` 入口并保持临时输出隔离。
7. 更新本计划实施记录、plans README 与必要的界面说明。只写本功能的实际状态，不顺手重算无关历史责任项或扩修其他功能。
8. 正常提交推送，创建可审查 PR，等待最新 HEAD 的必要 CI、Native UI、安装与升级矩阵通过。失败先看日志和 evidence，修复已定位问题，不盲目重跑。
9. 交付停在 PR，不合并、不发布、不在本机安装。当前 v0.7.5 安装版不会因计划或 PR 创建而出现新入口，必须随后续发布升级交付。
10. 人工文件与目录清理一律送入回收站；失败保留现场，不直接永久删除。

## 7. 完成判据（2026-11-12 实施状态）

- [x] 设置存在中英文关于入口，两种导航布局均可达（单测 + 浏览器契约）。
- [x] 显示运行程序的真实版本，加载、失败、重试与无接口场景表现明确（单测、浏览器 fixture，以及 Native `ui=0.8.0; exe=0.8.0` 真实 IPC 对照）。
- [x] 首次访问前零版本请求，成功后保留结果，无写设置或采集副作用（单测 + 浏览器 IPC 审计）。
- [x] 相关单测、浏览器契约与真实 IPC 验证范围已分别记录（本节）。
- [x] 最新 HEAD 必要检查成功，版本与发布声明匹配（20 项成功、1 项按设计跳过，`CLEAN`）。
- [x] 计划和 PR 记录一致，未提前宣称已发布或用户已安装。
