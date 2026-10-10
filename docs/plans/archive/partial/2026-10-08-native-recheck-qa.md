# 原生生产验收记录（RC10 入口 / RC11 取证）

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| RC10/RC11 既有原生批次 | 隔离入口和已完成场景有记录；失败尝试保留 | — |
| TTL / CSP / 当前显示器真实 150% | 已由后续 AP08 补齐，不再是“从未执行” | — |
| 125% / 跨显示器、系统深色首帧及当前版本批次 | 仍未验 | [N10](../../active/2026-10-11-consolidated-remaining-work.md#n10)、[N11](../../active/2026-10-11-consolidated-remaining-work.md#n11)、[N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-08-native-recheck-qa.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

**2026-10-09 后续补齐：** 最新证据见 [AP07 / AP08 补齐验收记录](2026-10-09-ap08-completion-qa.md)，其中 CSP 哨兵、关闭保存/失败恢复和当前显示器原生 150% 已验证。下文保留前一轮历史结果；未完成的系统与发布验收继续由全计划 AP08 / D5 承接。

本文件记录 **release + `acceptance` feature** 构建下、跑在隔离根里的原生
WebView2 验收。它只证明"这套配置下的原生契约"，不证明安装包、不证明真实
1.2 GB 数据的性能，也不代替浏览器自动化结论。

- 关联计划：[复核遗留缺陷与验收补齐](2026-10-08-recheck-remediation.md) RC10 / RC11
- 状态口径：**已验证 / 待验 / 环境缺失**三态，缺证据就写待验，不写"全部通过"
- 产物目录：`qa-artifacts/native-recheck-2026-10-08/`（gitignore，本机；
  每轮一个子目录，内含 `evidence.json` + 截图/帧序列 + 进程日志）
- **2026-10-09 归并：** 本文件的待验项（§5-5 会话过期原生轮、Windows 每监视器 125%/150%、
  系统深色首帧、CSP 脚本哨兵、安装/升级/卸载与自启注册）仍是唯一活跃待验入口，由
  [全计划终态复核与遗留修复](2026-10-09-all-plans-final-recheck.md) AP08 承接并在
  [D5 验收清单](../../d5-acceptance-checklist.md) 内登记；本轮复核修复 AP01–AP07 未触碰这些原生动作。

## 0. 复现步骤（本次实际使用的命令序列）

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
# 1) 构建（一次，全部轮次共用同一产物）
$env:TOKENSCOPE_ACCEPTANCE_ROOT = $acceptanceRoot   # 仅用于满足入口校验；构建本身不读它
.\frontend\node_modules\.bin\tauri build --features acceptance --no-bundle
#   → src-tauri\target\release\tokenscope.exe

# 2) 取证驱动（每轮一条；驱动器自己准备隔离根、设环境变量、启动 exe、
#    用 CDP 连上真实 WebView2 后做真实点击/键盘量测，最后只杀本轮 PID）
node frontend/scripts/native-acceptance.mjs --scenario <场景> `
  --theme <light|dark|system> --scale <1|1.25|1.5> --expect-theme <light|dark> `
  --port <端口> --events <合成事件数> [--root <复用隔离根>] [--frames] `
  --out qa-artifacts/native-recheck-2026-10-08/<轮次>
```

三条通道口径（**不修改系统显示缩放、不改系统主题、不放宽 CSP**）：

- **缩放通道**：`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--force-device-scale-factor=N`，
  实测 `window.devicePixelRatio` 分别为 1 / 1.25 / 1.5。这证明的是 WebView 设备缩放
  下的布局与交互；**不等于** Windows 每监视器缩放（本机系统缩放保持 100%，
  未改注册表/显示设置）。
- **主题通道**：应用自己的偏好开关（真实点击 → 写入隔离 WebView2 profile）。
  `--force-dark-mode` 实测**不改变** `prefers-color-scheme`，用它标"深色"会静默
  跑成浅色（第一轮就踩了这个坑，见 R0），因此所有轮次都改用应用内开关并显式
  核对 `documentElement[data-theme]`。
- **单实例约束**：acceptance 构建的 identifier 固定为 `{原 id}.acceptance`，
  同一时刻只能有一个验收实例——第二个实例会走单实例交接并立即以 0 退出。
  取证必须串行；需要 600 s 空闲的会话过期轮期间不能启动其他实例。

每轮 `evidence.json` 必含：commit、feature、exe SHA256、隔离根绝对路径、
manifest 文件数与散列、Windows / WebView2 版本、PID、devicePixelRatio、
窗口逻辑/物理尺寸、主题通道、真实 `~/.tokenscope` 前后指纹、全部断言明细。

**本轮公共环境**：commit `74d8f93`；`tokenscope.exe` SHA256 前缀
`581072f0131af15f`；Windows 10.0.19045（x64）；WebView2 运行时 `Edg/154.0.4258.62`；
系统显示缩放 100%（屏幕 2560×1600）；构建日志显示 `beforeBuildCommand` =
`vue-tsc --noEmit && vite build`，`frontendDist=../frontend/dist`。

| 轮次 | 场景（产物子目录） | 缩放通道 / 实测 dpr | 主题 | PID | 结果 |
| --- | --- | --- | --- | --- | --- |
| R0 | combo（`dark-100/`，早期） | 100% / 1 | 标记 dark，实测 `prefers-color-scheme:dark=false` → 实为浅色 | 16560 | **作废**（主题通道不可信，已由 C1–C6 重做） |
| R1 | seed-theme + first-frame（`ff-dark/`） | 100% / 1 | 深色（持久化偏好） | 3576 → 23392 | 已验证（§2） |
| R2 | seed-theme + first-frame（`ff-light/`） | 100% / 1 | 浅色（持久化偏好） | 8544 → 23512 | 已验证（§2） |
| R3 | first-frame（`ff-system/`，全新根） | 100% / 1 | 跟随系统 | 24916 | 已验证（系统当前为浅色，见 §2 限制） |
| R4 | csp（`csp/`） | 100% / 1 | 深色（真实切换） | 30192 | 已验证（§4） |
| R5 | fault-logs-file（`fault-logs/`） | 100% / 1 | 跟随系统 | 20920 | 已验证（§5-1） |
| R6 | fault-pricing-locked（`light-150-fault/`） | 150% / 1.5 | 浅色 | 见 evidence | 已验证（§5-2） |
| R7 | query-stability（`dark-150-fault/`） | 150% / 1.5 | 跟随系统 | 29372 | 已验证（§5-3） |
| R8 | sync-failure（`dark-125-fault/`） | 125% / 1.25 | 跟随系统 | 16980 | 已验证（§5-4） |
| R9 | expire-start ×3 + expire-finish（`expire/`） | 125% / 1.25 | 浅色 | 14928 / 18080（均被外部进程终止） | **待验**（§5-5，环境受限） |
| R10 | close-cancel（`close-cancel/`） | 100% / 1 | 跟随系统 | 18452 | 已验证（§6 取消） |
| R11 | close-tray（`close-tray/`） | 100% / 1 | 跟随系统 | 14988 | 已验证（§6 最小化） |
| R12 | close-remember（`close-remember/`） | 100% / 1 | 跟随系统 | 16580 / 24160 | 已验证（§6 记忆+退出） |
| C1–C6 | combo（`combo-light-1`、`combo-light-125`、`combo-light-15`、`combo-dark-1`、`combo-dark-125`、`combo-dark-15`） | 100/125/150% × 浅/深，实测 dpr 1/1.25/1.5 | 与标记一致（`data-theme` 已核对） | 13264 / 12828 / 16656 / 6656 / 1260 / 23224 | 已验证（§3，每轮 17/17 断言） |

## 1. 入口与隔离（RC10）

| 项 | 断言 | 状态 | 证据 |
| --- | --- | --- | --- |
| `native_acceptance_paths_are_hermetic` | 数据目录、cache.db、settings.toml、双源快照、pricing-index、view-cache、日志、两个来源根全部落在隔离根内；真实 `~/.tokenscope` 前后指纹完全一致 | 已验证（子进程 + 原生实跑） | `cargo test --offline --features acceptance --test native_acceptance` → 3 passed；原生 22 轮 `evidence.json`（含作废与重跑轮，逐轮列在 §0） 的 `root_files_after` 均含 `tokenscope/{cache.db,logs/…,pricing-index.json,settings.toml,view-cache.json}` 与 `webview/EBWebView/…`，且 `real_untouched=true`（每轮本次运行前后指纹相同） |
| `acceptance_mode_never_falls_back_to_real_sources` | 未显式指定来源目录时用隔离根下默认根；目录缺失只报 `missing`；空隔离根采集 requests=0 | 已验证（子进程） | 同上 |
| `acceptance_without_root_fails_before_collection` | 缺失 / 空 / 相对 / 不存在 / 等于真实数据目录的验收根一律在采集前拒绝，并点名 `TOKENSCOPE_ACCEPTANCE_ROOT` | 已验证（子进程，退出码 3） | 同上 + 壳侧 `start_rejects_missing_relative_and_missing_dir` |
| 普通构建不受影响 | 无 feature 时不读该环境变量，路径行为与验收前一致 | 已验证（编译期分支 + 默认门禁全绿） | `cargo test --workspace --offline`、`cargo test --manifest-path src-tauri/Cargo.toml --offline` |
| 准备脚本产物可解析 | 合成 Claude 日志 files=2 events=4 bad_lines=1；Codex files=1 events=2 bad_lines=0；`totals.requests=6`、`totals.cost_usd=0.0358`、未知 token 2,100、缓存命中率 58.3% | 已验证（原生 release 实例首帧画面） | `ff-dark/first-frame/frames/000.png`（汇总卡"请求数 6 / 估算费用 $0.0358 / 含未计价 token"、聚合表合计行、以及"本轮采集存在部分问题（1 行解析失败（已跳过））"告警条）；分页轮 `--Events 240` 另证大样本可解析 |
| acceptance 构建仍用生产前端与 CSP | release 构建 `frontendDist=../frontend/dist`、生产 `csp` 生效；不使用 `devUrl`/`devCsp` | 已验证 | 构建日志（同一次 `vite build` 产出同一 `dist`，acceptance feature 只改 Rust 侧目录/identifier）；§4 实测文档响应头 `content-security-policy` 为配置里的生产策略（Tauri 另加两个 inline 脚本哈希），且页面 origin 是 `http://tauri.localhost/` 而非 `localhost:1420` |
| 单实例交接 | 同一时刻第二个验收实例不干扰第一个：立即以 0 退出（identifier 带 `.acceptance` 后缀，故与运行中的普通实例互不影响） | 已验证（观察到的行为） | 一次并发启动记录 `进程提前退出 code=0`；据此把取证改为串行。属既有单实例设计，非缺陷 |

## 2. 原生首帧与背景（RC11 / 原 UX09）

取证方式：连接 CDP 后**立即**连续抓真实 WebView 帧（每帧记录相对进程启动的
毫秒数与像素亮度，亮度由最小 PNG 解码算出），并核对页面自报的
`data-theme` / `localStorage` / paint 时间戳。"加载完成"截图不作为首帧证据。

| 场景 | 期望 | 状态 | 产物与实测值 |
| --- | --- | --- | --- |
| 深色偏好冷启动首帧 | 第一帧即深色画布，无先浅后深闪烁 | 已验证 | `ff-dark/first-frame/frames/`（21 帧全部可解码）：首个可观测帧 = 进程启动后 **799 ms**，亮度 **27.5**；帧序列亮度 min 27.5 / max **31**（全程深色，无浅帧插入）；`data-theme=dark`、`localStorage tokenscope-theme=dark`；`first-paint=648 ms`、`FCP=648 ms` |
| 浅色偏好冷启动首帧 | 第一帧即浅色 | 已验证 | `ff-light/first-frame/frames/`（22 帧）：首帧 **698 ms**、亮度 **255**，序列 min **242.3** / max 255（无深帧插入）；`data-theme=light`；`first-paint=292 ms`、`FCP=660 ms` |
| 跟随系统 | 第一帧等于系统主题 | 已验证（系统=浅色这一半）；**系统深色未验** | `ff-system/first-frame/`：全新根、无持久化偏好 → 实测 `prefers-color-scheme:dark=false`、`data-theme=light`、首帧 898 ms 亮度 242.3；`first-paint=224 ms`、`FCP=860 ms`。要覆盖"系统深色"需改 Windows 应用主题设置，本计划不改系统配置 → 记为环境缺失 |
| WebView profile 隔离生效 | 首启无真实 localStorage 残留；偏好只来自隔离 profile | 已验证 | 全新根首启：`tokenscope-theme` 只可能是应用自写的 `system`；播种轮（真实点击深色/浅色 + 应用内"直接退出"）后同根冷启动读到 `dark`/`light`；`<root>/webview/EBWebView/…` 由本轮新建（`root_files_after` 可见）。附带实测结论：**强杀进程会丢未落盘的 localStorage**（一次播种用 Stop-Process 后偏好消失），因此取证统一走应用自身的退出通道 |

浏览器侧的预绘制取证（`prepaint_theme_has_correct_canvas`，21 条断言，含
storage 不可用场景）是另一条独立证据；两者口径不同，互不替代。

## 3. 六组主题 × DPI 的键盘与布局

每格都是原生实例上的真实交互/量测值（`combo-*/combo/evidence.json`，
每轮 17 条断言全绿）。`缩放`列同时给出注入通道值与实测 `devicePixelRatio`；
窗口尺寸列为「逻辑 DIP / 物理 px」。

| 组合 | 键盘/可访问名称 | 浮层四边 | 日期外壳 | 长路径 | 图表 | 通知/重试 | 导航滚动 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 浅色 100%（dpr 1；1920×1230 DIP / 1920×1230 px） | Tab 15 次到达费用触发器；名称"估算费用 $0.000325，查看计算明细"；焦点环 box-shadow 0 0 0 2px+4px | 480×382.25 @(933,748.875) 四边均在视口内；含"单价量纲：USD / 1M token（每百万 token）"；Enter 开、Escape 关 | 外壳 rgba(255,255,255,0.85)+blur(16px) saturate(1.5)+12px，内层透明/无 blur（单层） | 620×820 DIP 窗口下生效目录 2 行换行、无横向溢出（scroll=client=620）、未截断 | canvas 1070×320 已绘制；切"按模型"后仍绘制 | 正常态 2 条通知（定价横幅 + 采集告警）；真实重试可操作性见 §5-2/§5-4 同缩放轮 | sticky 保持、scrollTop=400、blur(20px) saturate(1.8) |
| 浅色 125%（dpr 1.25；1536×985 / 1920×1231） | 同上（outlineWidth 2.4px） | 480×382.4 @(741,452.6) 四边在内 | 同上 | 2 行、620/620、未截断 | 1070×320 已绘制 | 同上 | 同上 |
| 浅色 150%（dpr 1.5；1281×820 / 1922×1230） | 同上（outlineWidth 2.66667px） | 480×382.3 @(614,369.67) 四边在内 | 同上 | 2 行、620/620、未截断 | 1071×320 已绘制 | 同上 | 同上 |
| 深色 100%（dpr 1） | 同上；焦点环配色随主题切换为 rgb(77,163,255) | 同上 | 外壳 rgba(44,44,46,0.85)+blur(16px)+12px，内层透明 | 同上 | 同上 | 同上 | 同上，导航底色 rgba(28,28,30,0.72) |
| 深色 125%（dpr 1.25） | 同上 | 480×382.4 @(741,452.6) | 同上 | 同上 | 同上 | 同上 | 同上 |
| 深色 150%（dpr 1.5） | 同上 | 480×382.3 @(614,369.67) | 同上 | 同上 | 同上 | 同上 | 同上 |

可访问名称口径：汇总页 38 个、设置页 28 个可聚焦控件**全部**有真实名称
（`unnamedCount=0`）；费用触发器是 `button` 且与同名表格行分开计数。
无障碍工具输出被截断时只记录"工具输出截断"，不得写成 WebView2 的固定上限。

限制：这 6 组覆盖的是 **WebView 设备缩放**；Windows 每监视器缩放（真机
125%/150% 下 WebView2 的 DPI 协商路径）在本机未验，因为不改显示设置。

## 4. 生产 CSP（原生）

`native_production_csp_enforced` 实测（R4 / `csp/csp/evidence.json`，9/9 断言）：

| 项 | 状态 | 证据 |
| --- | --- | --- |
| 实际生效的 CSP 文本 | 已验证 | 重新加载真实文档，CDP `Network.responseReceived` 抓到 `http://tauri.localhost/` 200 响应头 `content-security-policy`：`connect-src 'self' ipc: http://ipc.localhost; img-src 'self'; style-src 'self' 'unsafe-inline'; object-src 'none'; default-src 'self' ipc: http://ipc.localhost; script-src 'self' 'sha256-LfuJG+FMw5hbhOn+hMh77Vu0Optfq5j8cQL58JdELFg=' 'sha256-w2+n69XwQgMuilhA9U8HwOA5JuqWvSZa399WhrzErwE='; base-uri 'none'; font-src 'self'`——即 `tauri.conf.json` 的生产策略（Tauri 为两个内联脚本追加 hash），非 `devCsp` |
| IPC/Naive/ECharts/主题/日期在 CSP 下正常 | 已验证 | 同轮断言：汇总卡片 4 张（真实 IPC 数据）、canvas 1070×320 已绘制、日期弹层外壳材质量测通过、深浅色真实切换（`data-theme` 与卡片底色同时变化）、全程无未预期 CSP 违规 |
| 内联脚本哨兵被拒 | **待验（环境缺失）** | 缺合法注入通道：唯一可用的是 CDP/调试器求值，而 plan 明确"DevTools/调试器的脚本求值不算证据"。实测旁证（非哨兵）：响应头含 `script-src 'self' + 两个 hash`，且 `dist/index.html` 无未授权内联脚本 |
| 外源脚本哨兵被拒 | **待验（环境缺失）** | 同上；另记：`default-src 'self' ipc:` 与 `script-src 'self'` 在配置层面无外源放行 |
| 已知噪声（不放宽策略） | 已定位并计数 | 每轮固定 2 条 `img-src 'self'` 违规：naive-ui 2.45.3 `es/tree/src/utils.mjs` 模块顶层 `new Image()` 预热一张 1×1 data-URI gif，包内无任何读取点（上游死代码）。功能无影响（表格/树筛选、图表、浮层均正常）。**处理方式：保持 CSP 不放宽，取证按"已知噪声/未预期"分开计数**，未预期必须为 0 |

无 CSP 的图表安全测试（`check-chart-tooltip-security.mjs`，4 场景真实 ECharts）
是另一条独立证据，两者都要通过（RC09 记录）。

## 5. 故障恢复（原生，仅作用于隔离根）

| 场景 | 期望 | 状态 | 证据（实测） |
| --- | --- | --- | --- |
| 临时 `logs` 路径写成普通文件 | 窗口仍出现，且有启动降级通知（SF06） | 已验证（100%） | R5：`<root>/tokenscope/logs` 预置为普通文件 → 导航照常渲染、`.banner-slot .ts-notice[role=status]` 显示"文件日志不可用，已退回备用输出：…"、图表仍绘制（降级不阻断）；`logs` 保持为文件未被静默改写 |
| 临时 `pricing.toml` 被共享锁占用 | 显示降级提示；释放后恢复读取 | 已验证（150%） | R6：启动**前**以 `FileShare=None` 独占该文件（并先确认锁真的持有）→ 设置页价格组出现"外置价格文件读取失败，该层本次不可用（恢复可读后将自动重新加载；原因: …）"；等待 41 s 锁释放后切走/切回触发真实重读 → 该告警消失。踩坑记录：中途才加锁无效（启动时已读成功），必须启动前持有并确认 |
| 合成日志/价格变化 | 旧 query 稳定不变，新 query 反映变化（SF04） | 已验证（150%） | R7：载入后 3 s 内摘要文本逐字不变 → 向隔离根追加一条合成事件（+1 请求）→ **未刷新时视图仍等于旧值**（旧会话不被原地改写）→ 点"刷新"后新会话数值变化 |
| 会话过期 | 错误条可见，点其按钮恢复（RC02） | **待验（环境受限）** | 唯一合法通道是真实空闲超过 `QUERY_IDLE_TTL=600 s` 后点"加载更多"，由后端 `with_snapshot` 的 `gc_expired` 判过期（RC02 的首页身份守卫使旧游标无法被复用，因此不能用刷新/切维度伪造）。两次尝试均在空闲窗口内被**外部进程**终止：`expire/expire-start/`（PID 14928、18080）的应用日志止于自身最后一条查询（12:26:59 采集 242 事件、明细 200 行/共 242），无优雅关闭记录，随后 CDP 端口 ECONNREFUSED。本机同期另有非验收实例在启停（父进程 `cargo`，见 §8 附注），不打算为抢占窗口去干扰其他工作。缺失证据：`expire-finish` 的 4 条断言（错误条文案含 `query_expired`、恢复按钮存在、点击后错误消失、分页恢复）。旁证（不等价）：浏览器侧契约 `query-expired-on-page2` 已在真实浏览器 + 真实点击下覆盖同一 UI 路径（RC09），Rust 侧过期分支由 `cargo test --offline --features acceptance --test query_snapshot_contract` 覆盖 |
| 补充源同步失败 | 错误条有可点击的重试同步（RC04） | 已验证（125%） | R8：只给该进程设 `HTTPS_PROXY/HTTP_PROXY=http://127.0.0.1:9` → 真实同步两源均连接被拒（os error 10061），错误条文案保留原因并给出"重试同步"；点击后确实再次尝试且失败原因仍在（不伪装成功、不清空原因） |

正常路径的日志初始化/价格加载耗时**不能**代替以上故障验收。

## 6. 关窗三态最小回归

`native_close_cancel_preserves_settings` 及另外两态，全部在隔离实例上、
用真实 `WM_CLOSE`（`PostMessage`，等价于点标题栏关闭）触发：

| 动作 | 弹窗 | 进程存活 | settings 字节 | 状态 |
| --- | --- | --- | --- | --- |
| Escape 取消（R10） | 出现"关闭 TokenScope / 要最小化到托盘继续统计，还是直接退出程序？"；Escape 后计数 0 | 是（exitCode=null） | SHA256 `3772679abb50…` → 同一值（未写入） | 已验证（7/7，截图 before-close / dialog-open / after-cancel） |
| 最小化到托盘（R11） | 出现；点击后弹窗计数 0 | 是 | SHA256 前后相同（`c15b227918ec…`，未写入 close_action） | 已验证（6/6）。实测：隐藏主窗口后该 PID 仍有 22×22 顶层宿主窗口，故判定按"最大可见顶层窗口 < 100×100" |
| 记忆 + 直接退出（R12） | 首次出现；勾选"记住我的选择"（`n-checkbox--checked`）后点"直接退出" → 进程自行结束 exitCode=0 | 否（预期退出） | SHA256 `08f1e2ad670b… → 5a4de9badf55…`，文件新增 `close_action` | 已验证（8/8）。重启同一隔离根：静息无弹窗，再次 WM_CLOSE 直接退出（无询问） |

截图均对应动作之后；记忆/最小化只在隔离实例验证，未触碰真实
`~/.tokenscope/settings.toml`。

## 7. 明确后延（不计入本轮完成）

- D5 安装 / 升级 / 卸载与自启注册（用户此前决定后延，本计划不自动执行）；
- 真实 1.2 GB 数据的性能与缓存预热（`#[ignore]` 测试，需显式环境变量）；
- 安装包签名与发布。

## 8. 结论口径与本轮发现

- 只有 §1–§6 全部落到"已验证 + 产物路径"后，才允许把 RC11 标为完成；
  任一项缺环境或缺证据，就在计划 §5 执行账里保持"待验 + 缺失证据"。
- 本轮明确**未验**的项：Windows 每监视器缩放（125%/150%）、系统深色主题下的
  "跟随系统"首帧、CSP 内联/外源脚本哨兵（无合法注入通道）。
- 本轮新发现（不改代码、按证据登记后另行处理）：
  1. **定价横幅文案与状态不符**——隔离根只有外置 `pricing.toml`（5 条有效价）时
     `pricing_status.needs_sync=true`（判据是"models.dev 无有效条目"），横幅于是
     显示"尚未获取定价……当前费用仅能显示为未知"，而同一界面里费用**正常显示**
     （$0.0358、明细浮层齐全）。6 组 combo + CSP 轮均稳定复现。属 RC04 同族的
     "状态口径不诚实"，建议作为新条目修复（横幅需区分"主源未就绪"与"完全无定价"）。
  2. naive-ui 2.45.3 的 data-URI 预热图在生产 CSP 下固定产生 2 条 `img-src` 违规
     （见 §4），不影响功能；不为它放宽 CSP。
- 取证工具自身的两条口径教训（已写进驱动器注释，避免下次误判）：
  明细表是虚拟滚动，DOM 行数不能证明分页；`--force-dark-mode` 不改变
  `prefers-color-scheme`，主题必须走应用内开关并核对 `data-theme`。

### 8.1 附注：取证期间的并发实例（需要有人处理）

- 取证期间本机另有一个 **非验收实例** 在运行（`tokenscope.exe`，父进程为
  `cargo`，PID 20208 起于 18:12 前后），并持续写真实 `~/.tokenscope`
  （`cache.db`、`view-cache.json`、`window-state.json`、`logs/` 的 mtime 随其变化）。
  本计划口径要求真实数据目录不被取证触碰：每轮的 `real_untouched` 比较的是
  **本次运行前后**的指纹，因此仍为 true；但跨轮次的指纹值会因该实例而漂移，
  这一点如实记录，不解释成"真实目录整体无人写入"。
- 该并发实例还会在空闲窗口内导致验收实例消失（§5-5 两次失败），并且它自身
  就是仓库规则要避免的行为（真实日志/真实缓存不应被非隔离运行触碰）。
  建议：并发工作统一走 `TOKENSCOPE_ACCEPTANCE_ROOT` 隔离根，或测试显式注入
  `cache_dir` / `pricing_index` 到临时目录。
