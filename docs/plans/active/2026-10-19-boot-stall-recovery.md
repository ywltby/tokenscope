# 启动卡死取证补强与引导期恢复

> 状态：**待用户确认**（2026-10-19 起草）。承接[整合计划 N14](2026-10-11-consolidated-remaining-work.md#n14来源目录遗留待验与竞态缺陷) 第 3 项。
> 前置：[PR #15](https://github.com/ywltby/tokenscope/pull/15)（0.5.1，含失败取证 `b141fcc`）合入 main 且 v0.5.1 发布完成。PR #15 尚未合入时可以先从 `origin/fix/upgrade-chain-format` 建分支开工；它合入后先合并最新 main，再开 PR。
> 分支 `fix/boot-recovery`；修复类批次，递增 patch；一个 PR、一次版本。

**Goal：** 下次再出现“页面导航完成、主界面不出现”时，失败取证能直接判断卡在哪一步；产品在引导期的三种卡死都有可见、可操作的出路，不再无提示白屏。

**Architecture：** 产品只改前端引导壳：隐私检查加“耗时较长”提示与代次保护，主界面加载失败显示错误与“重新加载”，并在引导壳根节点上用 `data-boot-state` 标出当前阶段。测试架在浏览器 IPC 桥两侧和网络请求上补时间线，失败取证据此自动给出判断。新增一条浏览器契约，用真实故障做端到端验证。后端、原生 E2E 不改。

**Tech Stack：** Vue 3 / Naive UI、Vitest、Playwright（`playwright-core` 1.63，已有契约在用 `page.clock`）。

## 1. 背景与已知事实

- main `599c89e` 的 [CI 39146151362](https://github.com/ywltby/tokenscope/actions/runs/39146151362) 中，浏览器契约 `ui_control_material` 失败：light 主题全部通过后，dark 主题新会话 `goto(networkidle)` 正常返回，随后 10 秒内 `#app .app-shell` 未出现。现场没有证据，原因未明。
- 已核实（详见 [PR3 归档的合入与发布记录](../archive/partial/2026-10-14-source-discovery-and-additional-paths.md#合入与发布记录2026-10-19)与 N14）：
  - 最近 60 次失败过的 CI 运行里，这是第一次出现等待 `.app-shell` 超时。
  - 那次机器不慢。工件压缩包的原始时间显示，“日期矩阵末张截图 → measurements.json”在通过的运行里是 40 秒，失败那次是 34 秒，其中还含 10 秒空等。
  - 导航按网络空闲返回，说明当时没有挂起的请求，可以排除“代码块下载慢”。
  - 本地按 CI 会话序列启动 200 次全部成功（壳层出现耗时中位数 762 ms、最长 819 ms）。
- 本地故障注入，三种卡死都复现了与 CI 相同的现象：
  - `privacy_bootstrap` 永不返回：页面停在“正在检查隐私设置…”。
  - MainApp 的 JS 请求失败：空白，报 `Failed to fetch dynamically imported module`。
  - MainApp 的 CSS 请求失败：空白，报 `Unable to preload CSS`。
- 产品现状：`privacyGate.bootstrap()` 没有耗时提示，也不区分新旧响应；`App.vue` 用 `void enterMainApp()` 动态导入主界面，失败后不重试、不提示。
- 后端约束：已同意时，`privacy_bootstrap` 会在同一次调用里同步执行业务初始化（日志、窗口、托盘、后台线程、来源目录迁移）；另一次检查在途时最多等 60 秒。因此耗时长可能是合理的，**不能把“慢”当失败**。后端检查串行且幂等，重复调用安全。

## 2. 范围与不做

做：引导期“检查耗时较长”提示与代次保护；主界面加载失败界面；`data-boot-state`；取证补强与自动判断；一条浏览器契约；文档与交付。

不做：
- 不调大任何等待、不给契约加重试。
- 不改后端，不改原生 E2E。
- 不给“同意并继续”加耗时提示。
- 不做自动整页重新加载（避免循环）。
- 不在 CI 跑压测（用户 2026-10-16 决定）。

## 3. 必须保持的不变量

1. 引导期边界（P04/P05）不变：未就绪前不读持久化偏好、不发业务 IPC。新增的“重新检查”只调用 `privacy_bootstrap`。
2. 只采用**最近一次**发出的 `privacy_bootstrap` 的结果，成功和失败都一样。`accept()` 开始时，在途的 bootstrap 一律作废。迟到的响应不得把界面从“已就绪 / 保存中”退回去，也不得写入错误。
3. 检查耗时较长**不是错误**：不写 `error`、不取消原请求；原请求的响应如果仍是最新一次，到达后照常应用。
4. 主界面加载失败必须可见、可操作，不得白屏。导入期间若已进入退出流程，不复活主应用（保留现有检查）。
5. 失败取证只读、有界，不影响会话关闭；只增加证据，不改变任何已有断言的通过标准。

## 4. 任务

每项先写会失败的测试，再实现；一个提交只解决一类问题。

### BS01 隐私检查：耗时提示与代次保护

- 文件：`frontend/src/lib/privacyGate.ts`、`frontend/src/lib/privacyGate.test.ts`。
- 导出 `BOOTSTRAP_SLOW_MS = 10_000`；gate 增加只读状态 `slow`。
- `bootstrap()`：每次调用递增代次，并为本次调用开一个 `BOOTSTRAP_SLOW_MS` 计时器，到点且本次仍是最新一次时置 `slow = true`。响应（成功或失败）只在“仍是最新一次”时应用，否则直接丢弃；最新一次结束时清除计时器与 `slow`。
- `accept()` 开始时递增代次（作废在途 bootstrap），并清除计时器与 `slow`。`retry()` 仍然调用 `bootstrap()`，自然成为新的一次。
- 单测（`vi.useFakeTimers()`），以下五例在实现前必须失败：
  1. 挂起超过 10 秒：`slow` 为 true，`error` 为 null，阶段仍为 checking。
  2. 超时提示后原请求返回 ready：`slow` 清除，阶段为 ready。
  3. 第一次挂起 → `retry()` 返回 ready → 第一次迟到返回 needs_consent：阶段保持 ready。
  4. 同上，但第一次以 reject 迟到：`error` 保持 null。
  5. blocked_error 时 `retry()`（挂起）→ `accept()` 返回 ready → 迟到的 bootstrap 返回 needs_consent：阶段保持 ready。
- 已有用例全部保持通过。

### BS02 主界面加载失败界面与阶段标记

- 新增 `frontend/src/lib/mainAppLoader.ts`：`createMainAppLoader({ load, isReady })`，返回 `visible`、`error`、`enter()`。
  - `enter()` 单飞（并发调用共用同一次加载）。
  - 加载失败时记录 `error` 并 `console.error`；加载成功但 `isReady()` 已为 false 时不置可见。
- `frontend/src/App.vue`：
  - `enterMainApp()` 保持“先 `applyResolvedThemeAttribute()` 再导入”的顺序，改用加载器，传入 `() => import("./MainApp.vue")`。
  - 检查块：未出错且 `gate.slow` 为 true 时，在“正在检查隐私设置…”下方显示“检查耗时较长”的说明与「重新检查」按钮（`role="status"`，不是错误）。
  - 加载失败：在 `.privacy-shell` 内显示 `role="alert"` 面板：“主界面加载失败：<原因>”与「重新加载」按钮（`window.location.reload()`）。复用现有 `.privacy-center` / `.privacy-blocked` 样式。
  - 失败必须整页重新加载：同一文档里失败的模块请求会被模块映射缓存，再次 `import()` 不会恢复。
  - `.privacy-shell` 上加 `data-boot-state`，取值固定为：`checking`、`checking-slow`、`blocked`（检查出错或 blocked_error）、`consent`（needs_consent / saving_consent / starting）、`exiting`、`loading-main`（已就绪、主界面未显示、无错误）、`main-load-failed`、`main`。只用于诊断与测试，不作样式钩子。
- 单测：
  - 新增 `mainAppLoader.test.ts`：加载失败、就绪丢失、成功、并发单飞四例。
  - `App.test.ts` 增一例：bootstrap 挂起超过 `BOOTSTRAP_SLOW_MS` 后，阶段标记从 `checking` 变为 `checking-slow`，提示与「重新检查」可见，点击后发出第二次 `privacy_bootstrap`。
  - 已有用例全部保持通过。

### BS03 失败取证补强与自动判断

只改测试架。

- `frontend/scripts/check-ui-contracts.mjs`：
  - 会话记录创建时刻。
  - Node 侧 IPC 记录追加 `at`（相对会话创建的毫秒数）；结束时写 `ms` 与 `outcome`（`ok` / `unknown` / `reject` / `error`），挂起的调用没有 `ms`。
  - 新增同源请求时间线 `requests`（`path`、`at`，结束时补 `ms`、`outcome`，保留最近 50 条）。
  - `INIT_SCRIPT` 的页面侧 IPC 记录追加 `at: Math.round(performance.now())`。
  - 已有用法只按 `cmd` / `args` 读取或计数，加字段不影响它们，执行时复核一遍。
- `frontend/scripts/lib/contract-failure.mjs`：
  - `domSnapshotInPage()` 增加 `boot_state`、`page_ipc`（页面侧最近 15 条命令名）与 `page_ipc_count`。
  - `captureContractFailure()` 每个会话增加 `ipc_tail`、`ipc_pending`、`requests_tail`、`requests_pending`，壳层不可见时再加 `suspected`；摘要行追加“判断=…”。
- 新增纯函数 `classifyBootStall(item)`，按以下顺序给出判断：
  1. DOM 不可读：返回“DOM 不可读”。
  2. `main-load-failed`，或页面错误匹配 `dynamically imported module` / `preload CSS`：返回“主界面资源加载失败”，附失败请求。
  3. `checking` / `checking-slow`：
     - 页面侧有 `privacy_bootstrap`、Node 侧没有：“IPC 未到达 Node”；
     - Node 侧挂起：“fixture 未返回”；
     - Node 侧已返回：“响应未回到页面”；
     - 页面侧也没有：“页面未发出检查”。
  4. `loading-main`：返回“主界面导入未结束”。
  5. `main` 但壳层缺失或不可见：返回“主界面挂载异常”。
  6. 其余：返回“未分类”。
- 单测：每一类至少一例，前三类按本地故障注入的实际形态构造；摘要包含“判断=”。

### BS04 浏览器契约 `boot_stall_recovery`

- `frontend/scripts/fixtures/ui-contracts.mjs`：
  - 新场景 `privacy-bootstrap-deferred`：第 1 次 `privacy_bootstrap` 返回 `defer("privacy_bootstrap#1")`，之后返回 ready。
  - fixture 暴露 `staleBootstrap()`，返回 needs_consent 快照。
- 新契约（独立文件，登记进 `REQUIRED_CONTRACTS` 与 `IMPL`，并加入 `frontend/scripts/run-ui-refresh-e2e.mjs` 的契约列表，让 CI 运行）：
  1. **检查挂起**：
     - 在 `goto` 前执行 `page.clock.install()`，导航后断言 `data-boot-state="checking"`；
     - `page.clock.runFor(BOOTSTRAP_SLOW_MS + 500)` 后断言 `checking-slow` 与「重新检查」可见；
     - 点击后 `.app-shell` 可见、阶段为 `main`；
     - `settleDeferred("privacy_bootstrap#1", staleBootstrap())` 后再推进少量时钟，断言仍为 `main`、政策弹窗未出现；
     - 会话照常 `finishSession`。
  2. **主界面加载失败**：
     - 普通 fixture，`context.route(/MainApp-[\w-]+\.js/, abort)` 后导航，断言 `main-load-failed`、“主界面加载失败”与「重新加载」可见；
     - 断言页面错误只有注入失败的资源记录，且没有 `unhandledrejection`。这个会话不调用 `finishSession`，因为它的页面错误检查会被预期内的资源错误触发，改为显式记录这条断言；
     - `unroute` 后点击「重新加载」，`.app-shell` 可见、阶段为 `main`。
- 本地反证：临时撤回 BS01 / BS02 的产品改动，契约必须失败；在执行记录里写明失败的是哪条断言，然后恢复。

### BS05 文档

- `DESIGN.md`：
  - “首屏与隐私同意引导期”一节补“检查耗时较长（重新检查）”与“主界面加载失败（重新加载）”两种状态，以及 `data-boot-state` 取值表（标明只用于诊断）；
  - 文件清单里 `App.vue` 的“三态”描述同步。
- `docs/privacy.md` 不改：读取范围不变，“重新检查”只是重复同一次同意字段读取。
- 整合计划 N14 第 3 项与执行账、`docs/plans/README.md` 按实际结果回填。本计划 §6 写执行记录。

### BS06 版本与交付

- `git fetch`；确认 PR #15 已合入、v0.5.1 已发布。
- 先 `python -B scripts/upgrade_baselines.py add --tag v0.5.1`，再 `python -B scripts/bump_version.py --bump patch --summary "<本轮说明>"`，最后 `--check --base-ref origin/main`。
- 本地完整门禁通过后推送，开**草稿** PR 面向 main。等 CI、Native UI E2E、Install test 在最终 HEAD 上全部通过；任何失败先按证据说明原因，不以重跑代替。
- 停在草稿 PR：不转正式、不合并。

## 5. 验证命令

```powershell
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
node frontend/scripts/run-ui-refresh-e2e.mjs   # 依赖最新 build
python -B scripts/bump_version.py --check --base-ref origin/main
```

提交钩子按暂存区选择检查；交付前用 `TOKENSCOPE_PRECOMMIT_MODE=full` 跑一次完整模式。

## 6. 执行记录

（实施后逐项记录提交、命令、先红后绿的现象与远程运行编号。）
