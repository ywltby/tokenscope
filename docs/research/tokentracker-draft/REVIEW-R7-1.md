# 全文复核 R7-1：元信息 + §1–§4

- 复核对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md`（4448 行）中 **元信息头部（L1–L20）＋ §1–§4 全文（L69–L871，§5 起为 L872）**
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（`git log -1` = `b19d27b chore(release): v1.3.14 (#792)`，`git describe --tags --always` = `v1.3.14`）
- 行号口径：`[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` 与 Node `fs.readFileSync().split("\n")`（两者在本轮抽验中逐点一致）；**全程未使用裸 `Get-Content`**
- 复核职责：**证伪**。未修改样本仓库（复核前后 `git status --porcelain` 均为空）、未修改主文档
- 机械脚本：`%TEMP%\tt-r71\refs2.mjs`（引用抽取）、`verify1.mjs`（规模与 GBK 漂移）、`verify41c.mjs`（45 个解析器 key 集合重建）

## 结论（可交付 / 需修订）

**判定：需修订（8 条，均为引用级／计数级／表述级；无机制性错误、无方向性错误）**

- 范围内 **177 个显式 `路径:行号` 引用簇 / 357 个行号点**，**文件存在 177/177、行号越界 0**；仅 1 处省略写法（`gnome-extension/.../extension.js`）与 2 处短形式（`status.js:44`、`wsl-probe.js:149`），均可从上下文唯一确定，**不算错**。
- 抽验 **154 条实质性断言**：**✅ 136 条 / ⚠️ 8 条 / ❌ 2 条**（2 条 ❌ 为「三条断言」计数错与「`~/.gemini` 207 个文件」口径缺失，见必须修 #1、#3；8 条 ⚠️ 为引用区间、唯一性措辞与 §13 台账缺行，见必须修 #2、#4–#8）。§3.3 锁机制、§3.4 游标两代与提交语义、§4 队列行 schema 与写入幂等机制 **全部逐字/逐值命中**，包括 §4.1 的「45 = 5 薄壳 + 40 自持、11 种键集合、计数键 31/6/3」——本轮用独立脚本重建后与文档**完全一致**。
- 元信息头部四段声明与事实相符（含 GBK 漂移「单文件最多 15 行／全 `src/` 累计 52 行」的 PowerShell 5.1 实测复现）；§1 时间线的 R1/R3/R4/OPEN-Q(R6) 数字与 §13 台账一致，R2 的 E=15 可复现而 I/U 缺 §13 对应合计行，R5 未在 §1 列出。
- 内部一致性：33 个代码围栏全部闭合；16 张表列数全部正确（脚本报的 1 处「3 列」是 `\|` 转义管道符的假阳性）；未发现同一事实两处写法冲突；未发现结论与源码方向相反。

## A. 全量引用存在性（统计表 + 问题清单）

### A1. 统计表

| 项 | 数 |
| --- | --- |
| 显式 `路径:行号` 引用簇（L1–L871） | **177** |
| 其中唯一解析到样本仓真实文件 | **177** |
| 文件不存在 | **0** |
| 行号点（单点 63 + 区间端点 294） | **357** |
| 行号越界点 | **0** |
| 省略形式引用（路径含 `...` 且带行号） | **0** |
| 省略形式提及（含 `...`、无行号） | **1** |
| 短形式引用（仅 basename + 行号） | **2** |
| 短形式提及（仅 basename、无行号） | **118** |
| 裸行号引用（`:NNN`，需按上下文归属） | **282** |
| 代码围栏开合对 | **33（全部闭合，无未闭合）** |
| 表格结构问题 | **真问题 0**（脚本报 1 处，经查为 `\|` 转义的假阳性） |

### A2. 省略形式的可判定性（单独列出，如实统计）

| 位置 | 省略写法 | 省略部分 | 能否从上下文唯一确定 |
| --- | --- | --- | --- |
| L138（§2.1 表「Linux Tauri 壳」行） | `gnome-extension/.../extension.js` | `TokenTrackerLinux/` + `tokentracker@tokentracker.cc/` | **能**。同表该行第二列为 `` `TokenTrackerLinux/` ``，且仓库内 `TokenTrackerLinux/gnome-extension/**/extension.js` 唯一命中（全仓 `extension.js` 在 `TokenTrackerLinux/` 下仅 1 个）；同行的 `src-tauri/src/server.rs:` 与前一行的 `TokenTrackerLinux/src-tauri/src/server.rs:700` 互相印证 |

补充（非 `...` 形式但同样省略前缀，不算错）：L138 的 `src-tauri/src/server.rs`（无 `TokenTrackerLinux/` 前缀）——仓库根不存在 `src-tauri/`，该路径只在 `TokenTrackerLinux/` 下成立，属同表的相对写法，可唯一确定。

### A3. 短形式引用（2 处，均可唯一解析）

| 位置 | 写法 | 解析结果 |
| --- | --- | --- |
| L316 | `status.js:44` | `src/commands/status.js`（全仓 basename 唯一）——`status.js:44` 逐字为 `const { detectPassiveProviders, isPassiveModeActive } = require("../lib/passive-mode");` ✅ |
| L478 | `wsl-probe.js:149` | `src/lib/wsl-probe.js`（全仓 basename 唯一）——`:149` 逐字为 `if (platform !== "win32") return { native: null, wsl: null };` ✅ |

### A4. 问题清单

- **越界引用：0**
- **指向不存在文件：0**
- **断裂引用（有行号无文件、或有 `\`\`` 不成对）：0**
- **无法唯一解析的省略/短形式：0**
- 唯一「非仓库路径的完整路径提及」经逐条人工核对后全部是 **regex 假阳性或运行时数据路径**（如 `~/.claude/settings.json`、`~/.tokentracker/app/bin/tracker.js`、`~/.codex/config.toml`、`~/.grok/hooks/99-tokentracker-usage.json`）——脚本因交替分支 `js` 先于 `json` 把 `.json` 截成 `.js`，**不是文档问题**。

## B. 结论验证（逐条：断言 / 判定 / 依据）

### B-1 元信息头部（L3–L19）

| # | 断言 | 判定 | 依据（亲手打开） |
| --- | --- | --- | --- |
| 1 | 研究对象 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c` | ✅ | `git rev-parse HEAD` 逐字相同 |
| 2 | 版本 v1.3.14，提交时间 2026-10-13 21:49:05 +0800 | ✅ | `git show -s --format="%ci"` = `2026-10-13 21:49:05 +0800`；`package.json.version` = `1.3.14` |
| 3 | npm 包名 `tokentracker-cli` | ✅ | `package.json` 的 `name` 字段逐字相同 |
| 4 | 行号口径「文件实际行号（按 `\n` 分行、UTF-8 解码）」 | ✅ | `src/lib/rollout.js` 三种 UTF-8 口径同为 **24932**；Node `split("\n")` 亦为 24932 |
| 5 | GBK 漂移「单文件最多 15 行；全 `src/` 累计受影响 52 行」 | ✅ | PowerShell **5.1** 实测：`(Get-Content $p).Count` = **24917**、`-Encoding UTF8` = **24932**；全 `src/` 逐文件比对 `TOTAL_AFFECTED_LINES=52`、有漂移文件 **20** 个，最大 15（`rollout.js`）、6（`local-api.js`）、5（`sync.js`）、4（`session-analytics.js`），其余 16 个各 1–3 —— **与文档逐项吻合** |
| 6 | 同上：`src/lib/rollout.js` 差 15 / `local-api.js` 差 6 / `sync.js` 差 5 / `session-analytics.js` 差 4 | ✅ | 实测 15/6/5/4，逐项一致 |
| 7 | 「只落统计数字、不含任何会话内容」的边界声明 | ✅（文本层） | 范围内未出现任何真实会话文本、代码片段、模型串或项目名；正文只出现字段名/路径规则/常量 |
| 8 | 证据分档三档「已验证／一致解释／未验证」，且不跨档升级 | ✅ | 范围内一致执行；`【未能确认】` 均附判定条件（§3.5 `multiInstallParse:49-54`、§4.4 `groupQueued` 分支、§4.7 `setMaxTotal` 调用点） |
| 9 | 数据来源两阶段（源码只读 + 2026-10-14 只读补证） | ⚠️ | 计数类声明可复核（见 #10–#12）；「未运行其程序／未安装依赖／未改动 `~/.tokentracker`」属**当时行为**，本轮无法回溯证伪——但本机 `~/.tokentracker` 目录**根本不存在**（`Test-Path` = False），与「全程未被改动」不冲突 |

### B-2 §1.3 规模与口径（L94–L117）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 10 | `read` / `ReadAllLines(UTF8)` / `Get-Content -Encoding UTF8` 三法同为 24932 | ✅ | 三种口径实测均 24932 |
| 11 | 默认编码 `Get-Content` = 24917（错） | ✅ | PowerShell 5.1 实测 24917；`[Text.Encoding]::Default.WebName` = `gb2312` |
| 12 | `Measure-Object -Line` = 23260（错，且忽略空行） | ✅ | PowerShell 5.1 实测 23260；UTF-8 口径非空行数为 23275（差异来自被吞掉的换行） |
| 13 | 规模纠偏表 6 行（`sync.js` 7251 行/288,151 字节、`rollout.js` 24932/942,266、`status.js` 1729/76,611、`cli.js` 107/4,060、`init.js` 82,654 字节/2427 行、`bin/tracker.js` 60 行） | ✅ | 逐文件实测，**6/6 全中**（见证据抽样） |
| 14 | §1.1：`git ls-files src` = 112 且 `Get-ChildItem -Recurse -File -Force src` = 112 | ✅ | 实测两者均 112 |
| 15 | §1.1：核心模块体积 `rollout.js` 942,266 B / `local-api.js` 156 KB / `session-analytics.js` 138 KB / `cursor-store.js` 62 KB / `fs.js` 15 KB | ✅ | 实测 942,266 / 155,946 / 137,522 / 62,975 / 15,033 字节 |
| 16 | §1.1：交付形态 = Node CLI（`src/`）+ `dashboard/` + `TokenTrackerBar/` + `TokenTrackerWin/` + `TokenTrackerLinux/` | ✅ | 仓库根目录逐项存在（另有 `.github`/`bin`/`docs`/`migrations`/`scripts`/`test`/`TokenTrackerWin.Tests`，与「五种交付形态」不冲突） |
| 17 | §1.1：`src/lib/pricing/` 含 `index.js`、`matcher.js`、`litellm-fetcher.js`、`seed-snapshot.json`、`curated-overrides.json` | ✅ | 5/5 存在 |
| 18 | §1.2：「全库 `bad_lines\|badLines\|bad_line` 在 `src/lib/` 下零命中」 | ✅ | 对 `src/` 全目录检索该正则：**No matches found** |
| 19 | §1.2：0.26.3 数据丢失事故由「注释原文记录 + 事后守卫代码」双类证据支持（`sync.js:234-248`、`:258-268`） | ✅ | `:234-237` 逐字「v6 was bumped in 0.26.3 … SHIPPING 0.26.3 caused catastrophic data loss …」；`:239-241` 逐字「wiped 2.17B claude tokens … The upload-offset reset also propagated the damage to the cloud」；`:242-247` 逐字「0.26.4 HALTS back at v4 …」；`:258-262` 为 #187 重扫修复注释 |
| 20 | §1.4：Claude 22 个 `*.jsonl` / 4609 行 | ✅ | 实测 22 文件 / 4609 行（**逐位吻合**） |
| 21 | §1.4：Codex 64 个 `rollout-*.jsonl` / 179776 行 | ✅ | 实测 64 文件 / 179776 行（**逐位吻合**；用共享读打开被占用的 7 个文件后取得） |
| 22 | §1.4：「`~/.gemini` 下 207 个文件」 | ❌ | 该目录**实际 965 个文件**（`antigravity-cli` 917 / `antigravity` 25 / `antigravity-ide` 19 / `config` 4）。**207 是正确的范围计数但不是全目录计数**：`*.jsonl`(89) + `*.json`(118) = **207**（且全部 ≤8 MB），与源证据 `OPENQ-W3-corpus-stats.md:8`「解析范围 207 个 `.jsonl`+`.json`（≤8 MB）」一致——**是文档漏写范围限定，不是数字错**。见必须修 #3 |

### B-3 §1 时间线 vs §13 台账（L21–L49）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 23 | R1：6 个复核 agent，抽验合计 772 条，E=50／I=43／U=29 | ✅ | §13.1 合计行逐数字相同（772 / 50 / 43 / 29），且 139+46+141+188+157+101 = 772 内部自洽 |
| 24 | R1 的三条结论级推翻举例（KiroCli 字符估算、三族签名、OpenCode 拒绝覆盖） | ✅ | §13.1 R1-C/R1-B/R1-A 行与 §3.2 幂等范式第 4 条的最终表述一致（OpenCode「没有『无标记则拒绝写入』的分支」） |
| 25 | R2：合计检查 306 项 | ✅ | §13.2 三行工作量相加：(40+40+13)+(40+54+14)+(49+46+10) = 93+108+105 = **306** |
| 26 | R2：E=15／I=24／U=14 | ⚠️ | **E=15 可复现**（R2-1「必须修（E）共 3 条」+ R2-2「必须修的是 E1–E10」+ R2-3「新增判定 E2」= 15）；**I=24／U=14 在 §13.2 与本轮抽读的 R2 报告中均无对应合计行**，无法独立复算 → 记为「§13 缺合计行」 |
| 27 | R3：四单元合计 478 条/点，命中率 88.9%–99.5% | ✅ | §13.5：182+72+112+112 = **478**；四单元命中率 99.5% / 88.9% / 92.0% / 94.6%，区间端点吻合 |
| 28 | R4：修正点验收 29/29 与 31/32，抽样命中率 94.0% 与 97.7%，只剩 7 处引用级校准 | ✅ | §13.7 逐项相同（29/29、50 条 94.0%、32 条 31 成立、97.7%、7 处） |
| 29 | OPEN-Q：A 类 18→12、B 类 10→4、1 条迁入缺陷清单、新增 §12.E、R6 见 §13.10 | ✅ | §13.9 与 §13.10 逐项相同（A16 迁入 C 类第 25 条；§12.E 8 项） |
| 30 | §1 未提及 R5（最终验证） | ⚠️ | §13 标题为「复核记录（R1–R6）」且 §13.8 完整记载 R5；§1 的第 6–8 条从 R3 直接跳到 R4 → OPEN-Q/R6。属**时间线不完整**，非矛盾 |
| 31 | §1 第 1 条「8 份分片（`S0…S6`）」 | ⚠️ | `docs/research/tokentracker-draft/` 内 `S*` 文件实为 **9 个**（S0/S1/S1b/S2/S3a/S3b/S4/S5/S6）。若不计 S1b 则恰 8，但文档未说明 → 计数口径需一句话澄清 |

### B-4 §2 总体架构与数据流（L127–L198）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 32 | `bin/tracker.js` 60 行；`:5,12-13` 剥离 `--debug` 成 `TOKENTRACKER_DEBUG`；`:42-59` 统一错误出口 | ✅ | `:5` `stripDebugFlag` require、`:12-13` `const { argv, debug } = stripDebugFlag(...)` + `if (debug) process.env.TOKENTRACKER_DEBUG = '1'`；`:43-59` 打印 stack/Status/Code/Original error/Next actions，`:59` `process.exitCode = 1` |
| 33 | `bin/tracker.js:15-40` 代理自举，`unprotected === true` 时硬失败 | ✅ | `:35-40` 逐字：`if (applyResult && applyResult.unprotected === true) { … process.exit(1); }`，含中英双行提示 |
| 34 | `src/cli.js` 107 行、`:12-66` 无语法的静态 switch、无 commander/yargs、无参即 serve | ✅ | `:12` `async function run(argv) {`、`:66` 闭括号；`:15-18` 无参 `await cmdServe(argv)`；`:63-64` `default: throw new Error(\`Unknown command: ${command}\`)` |
| 35 | `src/cli.js:68-105` `--help` 文本含三行采集相关说明（Every Code / auto sync / Codex 命名约定） | ✅ | `:93` `"  - Every Code notify installs when ~/.code/config.toml exists."`、`:95` `"  - auto sync waits for a device token."`、`:97` `"  - sync parses ~/.codex/sessions/**/rollout-*.jsonl and ~/.code/sessions/**/rollout-*.jsonl, then uploads token deltas."` 三行**逐字相同** |
| 36 | `src/commands/serve.js:14-22` 端口 7680 / WSL 7681 / 最多 20 次；`:15-20` 注释点明 Windows Delivery Optimization 与 issue #267 | ✅ | `:14` `DEFAULT_PORT = 7680`、`:21` `WSL_DEFAULT_PORT = 7681`、`:22` `DEFAULT_MAX_PORT_ATTEMPTS = 20`；`:15-20` 注释逐字提到「accepts the TCP connection and drops the HTTP request (#267)」 |
| 37 | 显式 `--port` 时 `allowFallback: !opts.portExplicit`、挂 `ensurePortFree`，被占即 `buildPortInUseHint` + `exitCode = 1` | ✅ | `serve.js:209-224` 逐字 `allowFallback: !opts.portExplicit` / `ensurePortFreeFn: opts.portExplicit ? ensurePortFree : null` / `process.stderr.write(buildPortInUseHint(opts.port))` / `process.exitCode = 1`；`listenOnAvailablePort` 定义于 `:679`，`attempts = allowFallback ? maxAttempts : 1`（`:690`） |
| 38 | §2.1 表依据：`bin/tracker.js:5-59`、`src/cli.js:12-66`、`serve.js:69,80-90,257-260`、`local-api.js:202-211` | ✅ | `serve.js:69` `async function cmdServe(argv)`；`:80-90` 首次启动 `cmdInit(["--yes"])`；`:257` `startNativeBackgroundSync({`；`local-api.js:202-211` `readQueueData = createAsyncQueueReader({...})` |
| 39 | macOS 壳：只注入 `TOKENTRACKER_APP_SHELL=macos`；`ServerManager.swift:203` 是注入点、`:233` 是登录 shell 兜底分支；serve 侧 60s 分支要求 `appShell === "windows"`（`serve.js:304`） | ✅ | `ServerManager.swift:203` `env["TOKENTRACKER_APP_SHELL"] = "macos"`；`:233` `fallbackEnv["TOKENTRACKER_APP_SHELL"] = "macos"`（在 `launchServer` 的 `/bin/zsh -lc` 分支内）；`serve.js:304` `if (normalizedShell !== "windows") return null;` |
| 40 | Windows 壳：5 分钟计时器 `TrayApplicationContext.cs:84`、Tick 绑 `:240`、就绪即启动并立即触发一次 `:965-971`、`Failed` 停止 `:972-974`；双环境变量注入 `ServerManager.cs:694,700` | ✅ | `:84` `_syncTimer = new() { Interval = 5 * 60 * 1000 }`；`:240` `_syncTimer.Tick += (_, _) => TriggerBackgroundSync();`；`:965-971` Running → `_poller.Start(); _poller.RefreshNow(); _syncTimer.Start(); TriggerBackgroundSync();`；`:972-974` Failed → `_syncTimer.Stop();`；`ServerManager.cs:694` `TOKENTRACKER_APP_SHELL = "windows"`、`:700` `TOKENTRACKER_NATIVE_SYNC_OWNER = "windows-host"` |
| 41 | Linux 壳：`server.rs:587-604` 日志路径轮转、`:116` server 记录目录、`:700` 仅注入 `TOKENTRACKER_APP_SHELL=linux` | ✅ | `:589-606` `server_log_paths` 三段（`$XDG_STATE_HOME/tokentracker/server.log` → `$HOME/.local/state/tokentracker/server.log` → `/tmp/tokentracker-server.log`）；`:116` `dirs.push(state_home.join("tokentracker").join("servers"))`；`:700` `command.env("TOKENTRACKER_APP_SHELL", "linux")` |
| 42 | Windows 壳日志 `%LOCALAPPDATA%\TokenTracker\windows-host.log`（`Diag.cs:11-13`） | ✅ | `:11-13` `LogPath = Path.Combine(GetFolderPath(LocalApplicationData), "TokenTracker", "windows-host.log")` |
| 43 | §2.2「计价输入是五列之和，`total_tokens` 不参与计费（`src/lib/pricing/index.js:248-256`）」 | ⚠️ | 结论**成立**：`:257-264` `baseCost = (input*pricing.input + output*pricing.output + cached_input*cache_read + cache_creation*cache_write + reasoningCost)/1e6`，`total_tokens` 全程未出现；但**引用区间 `:248-256` 只覆盖 reasoning 门控（`reasoningIncludedInOutput` 白名单 + `reasoningCost`），不含五列算式** → 引用偏差约 9 行。见必须修 #2 |
| 44 | §2.3：hook 触发的 sync 是单源、手动 `sync` 才是全源（`resolveAutoSourceScope`，`sync.js:3747-3753`） | ✅ | `:3747-3753` 逐字：`if (!opts?.auto) return null; … if (!opts.fromNotify) return null; return normalizeSyncSource(opts.source);` |
| 45 | §2.3：队列写入在游标提交之前（`rollout.js:3483` 先 append，`sync.js:3444-3446` 后 commit） | ✅ | `rollout.js:3483` `await fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8");`；`sync.js:3444-3446` `if (!skipNoOpCursorCommit) { … await cursorStore.commit(cursors); }` |
| 46 | §2.3：`pendingBytes` 只按主队列算（`sync.js:3654-3659`） | ✅ | `:3654-3659` 逐字含注释「Only the main queue is uploaded by drainQueueToCloud …」+ `const pendingBytes = Math.max(0, queueSize - Number(afterState.offset \|\| 0));` |
| 47 | §2.4：目录布局由 `tracker-paths.js:4-12` 决定；队列/游标/锁都在 `~/.tokentracker/tracker/` 下 | ✅ | `:4-12` 逐字返回 `rootDir`/`trackerDir`(=`rootDir/tracker`)/`binDir`/`cacheDir`；`sync.js:568-573` 显式 `path.join(trackerDir, "cursors.json"/"queue.jsonl"/…)` |
| 48 | §2.4：`~/.tokentracker/logs/` 以仓库为准不存在（`tracker-paths.js:5-11` 只产四个目录；`path.join(<trackerDir>,"logs")` 类写入零命中） | ✅ | 解析器 `:5-11` 只建 4 个目录；对 `src/` 检索 `"logs"` 共 12 处，**全部指向 provider 自有目录**（`qoder-limits.js:631-636`、`rollout.js:10065-10080/20604`、`zcode-limits.js:919`），**无一处以 `trackerDir`/`~/.tokentracker` 为父目录** |

### B-5 §3.2 hook 安装（L250–L316）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 49 | 三层解耦：`notify-command.js`（38 行）× 各 provider config × `notify.cjs`；平台差异在 `:13-19`；`powershellLiteral` `:3-5`、`quotePosix` `:7-11` | ✅ | 文件实为 **38 行**；`:13-19` `buildNotifyCommand` 的 win32 分支 `& ${powershellLiteral(execPath)} ${powershellLiteral(notifyPath)} --source=${source}`、POSIX 分支 `/usr/bin/env node ${quotePosix(notifyPath)} --source=${source}`；`:3-5` `replace(/'/g, "''")` |
| 50 | Claude hook 项附 `"shell": "powershell"`（`claude-config.js:176-179`） | ✅ | `:176-179` `hookShell` 以 `command.trimStart().startsWith("& '")` 判定并返回 `{ shell: "powershell" }` |
| 51 | `notify.cjs` 模板区间 `init.js:1453-2231`、宿主 `buildNotifyHandler` `:1445-2232`、落盘 `:425-435` | ✅ | `:1445` 函数定义、`:1453` 反引号起、`:2231` 反引号止、`:2232` 闭括号；`:425-435` `writeNotifyHandler` 调 `buildNotifyHandler` 并 `writeFileAtomic` |
| 52 | 行为 ①解析 `--source`（`:1461-1476`）②落 signal（`:1448,1496-1498`）③20s 节流（`:1519-1536`）④链式回放（`:1538-1559`，`:1540-1547` 显式不链式分支）⑤无条件 `exit(0)`（`:1561`）⑥调试日志上限 1 MB（`:1488-1517`） | ✅ | 六处逐字命中：`:1464-1476` for 循环收 `payloadArgs`；`:1448` `queueSignalPath`、`:1497` `writeFileSync(signalPath, …)`；`:1519-1526` 注释「at most once per 20 seconds」+ `if (now - last > 20_000)`；`:1540-1547` `source === 'claude' \|\| 'opencode' \|\| 'gemini' \|\| 'codebuddy' \|\| 'workbuddy' \|\| 'workbuddy-ai' ? null : codexOriginalPath`；`:1561` `process.exit(0);`；`:1491-1493` `: 1_000_000` |
| 53 | `trackerBinPath`（`:1483`）与 `depsMarkerPath`（`:1484`）字面不同但求值相同 → 判据退化为一次 `existsSync` | ✅ | `:1483` `const trackerBinPath = ${JSON.stringify(trackerBinPath)};`（值 = `<trackerDir>/app/bin/tracker.js`）、`:1484` `const depsMarkerPath = path.join(trackerDir, 'app', 'bin', 'tracker.js');` → 求值相同 |
| 54 | 同 quirk 见于 `openclaw-session-plugin.js:358,361` | ✅ | `:358` `const trackerBinPath = ${JSON.stringify(trackerBinPath)};`、`:361` `const depsMarkerPath = path.join(trackerDir, 'app', 'bin', 'tracker.js');` |
| 55 | `spawnDetached` 用 `detached + unref() + stdio:'ignore'`（`:1563-1573`） | ✅ | `:1565-1571` 逐字含 `detached: true`、`stdio: 'ignore'`、`child.unref();` |
| 56 | `SUPPORTED_PROVIDERS` 45 项（`init.js:119-165`） | ✅ | `:120-164` 逐行枚举 = **45 项** |
| 57 | `45 = 写配置 12 + 仅探测 1 + 纯被动 17 + 仅欢迎名单 15`（四类互斥、并集恰 45） | ✅ | 逐项回源：写配置 12 = codex `:686-702`、acode `:704-720`、claude `:722-731`、gemini `:733-742`、opencode `:744-756`、omp `:789-828`、grok `:901-916`、codebuddy `:1037-1046`、workbuddy `:1050-1059`、workbuddy-ai `:1062-1071`、openclaw `:1073-1118`、every-code `:1130-1146`；仅探测 1 = Cursor `:758-776`（注释逐字 `// Cursor (API-based, no hooks needed)`）；纯被动 4 例行号逐项命中（MiniMax Code `:851-856`、Command Code `:860-865`、Craft Agents `:870-875`、Reasonix `:878-883`）；12+1+17+15 = 45 ✅ 与 `renderWelcome` 的 `SUPPORTED_PROVIDERS.length`（`:274-276`）闭合 |
| 58 | 幂等四类 + Grok 例外：Claude `hasHook`（`:143-156`）/已存在不写盘（`:39`）/`normalizeEntriesForCommand`（`:181-230`）；Codex 等值即 `changed:false`（`:76,107`）+ `isManagedNotifyCmd`（`:40-58`）+ 手工行编辑（`:245-269`、`:271-309`）；omp 哨兵（`:17`、`:388-395`，消费 `init.js:813`）；OpenCode 内容等值短路（`:73-75`）+ 无条件备份覆盖（`:80-85`）+ `unexpected-content`（`:94`）；Grok 恒 `changed:true`（`:85-87`、`:307-317`）且无备份 | ✅ | 逐条逐字命中（另见 #66 对 OpenCode「标记检测」措辞的限定） |
| 59 | 备份策略：claude/gemini `.bak.<ISO>`、codex 系列先存 `*_notify_original.json`、omp 写入前后 `pathIdentity` + `sameFileIdentity`、OpenClaw 靠 `plugins.entries` 策略、卸载用 `isManagedNotifyCmd` 把关 | ✅ | `claude-config.js:248-262`、`gemini-config.js:265-279` 的 `.bak.${new Date().toISOString().replace(/[:.]/g, "-")}`；`codex-config.js:87-104` `hasForeignNotify` + `writeJson(notifyOriginalPath, {notify, capturedAt})`；`omp-hook.js:227-239`（写前）与 `:265-279`（写后）两次 `pathIdentity` + `sameFileIdentity`；`openclaw-session-plugin.js:180` `ensureOpenclawSessionPluginPolicy`、`:154-178` 五布尔探测；`uninstall.js:76-137` 还原块；`codex-config.js:110-133` `current-not-managed` |
| 60 | passive mode：判定三条件（`passive-mode.js:189-204`）、`dirHasFile` 至多一层且吞 readdir 失败（`:49-68`）、失败原因只分 ENOENT/EACCES/EROFS（`:70-81`）、覆盖 8 家（`:96-185`）、模块头「hook 缺失永不导致 sync 失败」（`:22-25`）、`init.js` 无调用关系 | ✅ | `:191` `const passive = hookExpected && !hookInstalled && logsPresent;`；`:49-68` 单层 `dirHasFile` 递归 + `catch → return false`；`:76-79` 三种 code；`:97/109/122/133/145/156/166/177` 恰 8 个 `buildEntry`（claude/gemini/codex/acode/every_code/codebuddy/workbuddy/workbuddy-ai），无 grok/opencode/openclaw；`:23-25` 注释逐字；`src/` 内 require `passive-mode` 只在 `status.js:44` |
| 61 | 「全仓引用它的只有 `status.js`（`:44` require、`:1021` 调用、`:1248` 输出、`:1448,1688` 呈现）」 | ⚠️ | `src/` 内**5 处全部逐字命中**；但「全仓」不成立：`test/passive-mode.test.js:24`、`test/workbuddy-ai.test.js:17` 亦 import。结论方向不受影响。见必须修 #5 |
| 62 | OpenCode「**没有**『无标记则拒绝写入』的分支；标记检测只出现在卸载路径（`:94`）」 | ⚠️ | 前半句 ✅（`upsertOpencodePlugin` 全函数无 marker 检查）；后半句**过窄**：`hasPluginMarker` 亦被 `isOpencodePluginInstalled`（`:104`）调用，即探测路径也做标记检测。见必须修 #6 |

### B-6 §3.3 锁机制与 sync 编排（L318–L392，任务指定必查）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 63 | `cmdSync` 体 `:516`–`:3709`（3194 行）；`module.exports` 在 `:3821-3857`；其后 `:3859-7251` 是 repair/helper | ✅ | `:3709` 为闭括号、`:3711` 起为 `parseArgs`；7251−516+1 = **3194**；`^module.exports` 唯一命中 `:3821` |
| 64 | 锁常量：`NOTIFY_LOCK_WAIT_MS = 130_000`、`NOTIFY_LOCK_POLL_MS = 5_000`、`PRIORITY_LOCK_WAIT_MS = 90_000`、`PRIORITY_LOCK_POLL_MS = 250`（`sync.js:295-300`） | ✅ | `:295-300` 逐字，含 `:297-298` 注释「120-second child-process budget … Reserve at least 30 seconds …」 |
| 65 | 三档等待：`waitForLock/drain/publishAccount` → 90s/250ms 超时抛 `SYNC_BUSY`；`fromNotify` → `${lockPath}.notify-wait` 低频合并闸最多 130s；其它（含 `--auto`）→ 立即 `null` | ✅ | `:438` `const waitsForPriority = Boolean(opts.waitForLock \|\| opts.drain \|\| opts.publishAccount);`；`:452-456` `SYNC_BUSY`；`:461` `if (!opts.fromNotify \|\| notifyWaitMs <= 0) return null;`；`:466` `${lockPath}.notify-wait`；`:463-465` 注释逐字 |
| 66 | `fs.js:69-74` 常量：`LOCK_STALE_MS = 5 min`、`LOCK_HEARTBEAT_MS = 30 s`、`MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64`；`:244-254` 过渡闸 5ms × 1000 ≈ 5 s | ✅ | `:69-74` 逐字（`5 * 60 * 1000` / `30 * 1000` / `4` / `64` / `TRANSITION_GUARD_RETRY_MS = 5` / `TRANSITION_GUARD_ATTEMPTS = 1000`）；`:244-254` 循环 + `:260` `Timed out serializing lock release` |
| 67 | 加锁是 `fs.open(lockPath,"wx")` → 写 `{pid, token, createdAt}` → 以同样 `wx` 建 `${lockPath}.heartbeat.${sha256(token)}` 并写入 token 本身（`:296-307`）；心跳 30s 且 `utimes` 两个文件 + `timer.unref()`（`:206-224`） | ✅ | `:296` `await fs.open(lockPath, "wx")`；`:301-304` JSON + `"\n"`；`:305-306` `fs.open(heartbeatPath, "wx")` + `writeFile(\`${token}\n\`)`；`heartbeatPathFor`（`:88-91`）= `crypto.createHash("sha256").update(token,"utf8").digest("hex")` → 文件名确为 sha256(token) ✅；`:210-213` `Promise.all([handle.utimes(now,now), heartbeatHandle.utimes(now,now)])`；`:219-222` `setInterval(…, LOCK_HEARTBEAT_MS)` + `timer.unref?.()` |
| 68 | 陈旧判定三分支：①owner 进程不存在（`process.kill(pid,0)`，只有 `ESRCH` 算死、`EPERM` 保守当活）②owner 存活 → **只看该 token 专属心跳 mtime** 是否超 `LOCK_STALE_MS`，心跳读不到即判活（`:117-134`）③无 owner 记录 → 退回锁文件 mtime（`:136`） | ✅ | `:118` `if (!isProcessAlive(owner.pid)) return true;`；`:93-103` `isProcessAlive` 逐字 `if (e?.code === "ESRCH") return false;` + 注释 `EPERM means the process exists but belongs to another user` + `return true;`；`:127-131` `heartbeatPathFor(lockPath, owner.token)` → `Date.now() - heartbeat.mtimeMs > LOCK_STALE_MS`，catch → `return false`（判活）；`:136` `return Date.now() - stat.mtimeMs > LOCK_STALE_MS;` |
| 69 | 回收用 `.reclaim` 次级锁串行化、持闸后**重新校验一次**（TOCTOU）、`rename` 到 `.stale.<pid>.<uuid>`；释放时同样持闸且**只有 token 匹配才 unlink** | ✅ | `:342-346` `.reclaim` guard；`:368` `if (!(await existingLockCanBeReclaimed(lockPath))) return null;`（持闸复核）；`:370` `quarantinePath = \`${lockPath}.stale.${process.pid}.${crypto.randomUUID()}\``；`:372` `fs.rename`；`:268-269` `const owner = parseLockOwner(...); if (owner?.token === token) {` → `:273-274` unlink 锁与心跳 |
| 70 | `.reclaim` 嵌套闸会永久堵死（issue #431），有一次性迭代清扫，从深到浅只删「owner 已死或心跳超期」那级（`:178-204`） | ✅ | `:178-185` 注释逐字含 `issue #431`；`:189-197` 逐级 `.reclaim` 收集；`:200-201` `for (let index = chain.length - 1; index >= 0; index -= 1)`（深→浅）；`:352-359` 每次 acquisition 只扫一次 |
| 71 | 「该 token 的心跳还新鲜吗」把 PID 复用与长时健康运行解耦 | ✅ | 注释 `:120-124` 与 `:392-393` 逐字支持该解释 |
| 72 | `recordSyncSkip`（`:484-506`）：`lock_busy` 正常争用、`lock_debris` 持有者已死；成功拿锁后清除 marker（`:565`）；`status` 输出 `last_sync_skipped` | ✅ | `:484` `SYNC_SKIP_MARKER`、`:491-505` 逐字含两种 reason 与 `Sync skipped: ${reason} — ${detail}`；`:554-558` 写 stderr 后 `return`（不抛错）；`:565` `await clearSyncSkip(trackerDir);`；`status.js:285` 读、`:351-355` 组行、`:1058` `last_sync_skipped: syncSkip?.at ? syncSkip : null` |
| 73 | `skipNoOpCursorCommit`（`:3424-3437`）**13 个**条件项 | ✅ | 逐项点数：`opts.auto`、`!isFullSourceScan`、`mode==="v2"`、`requiresCommit !== true`、`totalParsed === (…)`、`totalBuckets === 0`、两个 project 桶、`!schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok` = **13** |
| 74 | 跳过但审计到期 → 计数延迟累加到 `deferred-codex-audit.json`（`:3456-3460`） | ✅ | `:3456-3459` `} else if (codexAuditRecorded) { await cursorStore.writeDeferredCodexAuditSyncs(deferredCodexAuditSyncs + 1); }` |
| 75 | 去重五层（根 / 目录 / 文件 / 事件 / 同 session 多路径）与全部行号 | ✅ | ①`scan-roots.js:116-146`(`identityKey`)+`:204-224`(`appendUniqueDirs` 只对附加根去重，注释逐字「Base entries are never collapsed against each other: the native + WSL pair is listed on purpose」) ②`sync.js:864-870` `uniqueSources` 按 `sessionsDir` 去重 ③`rollout.js:937` `claudeFileIdentityHash`（注释逐字「sha256 of the first 64KB plus the file size」，`CLAUDE_FILE_ID_PREFIX_BYTES = 64 * 1024`）+ `:1056-1105` 逻辑（`:1073` `} else if (!prev && primary !== filePath) {`、`:1080` `offset: st.size`、`:1084` `duplicateOf: primary`）④`rollout.js:292-295` 注释 `sessionUUID:eventTimestamp`、`:4914-4919` `claudeMessageDedupKey` 返回 `${msgId}:${reqId}` 或 `msgId`、`:1233-1237` `> 100_000 ? slice(-100_000)` ⑤`rollout.js:428-461`（`:436` `if (rebuildingBaseline) return true;` 第一优先；`:458-460` 稳态四条件 → `false`，否则 `prev?.lastTotal` → `true`，新文件 → `false`） |
| 76 | 服务端 ingest 按桶键整行 upsert（`sync.js:249-256` 注释） | ✅ | `:249-256` 注释逐字含「the ingest's whole-row upsert overwrites every historical bucket with the correct conversation counts … last emission per key wins」 |
| 77 | §3.3 编排表 23 行（`:516-545`…`:3654-3708`）抽样 | ✅ | 抽验 6 行：`:567-575` 八个路径常量（config/cursors/queue/queue.state/project queue/project queue.state/upload.throttle/grok signal）逐项一致；`:584-634` legacy baseUrl 迁移把 `queue.state.offset` 置 0 + 删 `upload.throttle.json`；`:713-757` 三档模式与 `backgroundCodexUsageRepair`；`:759-896` sources 组装与 `union` 收两套；`:898-991` 六个 repair + 冷扫描过滤 + `deferredCodexAuditSyncs`；`:3654-3708` 见 #46 |
| 78 | 参数全集与 `--auto`/`--background` 的三处放大（`:719-735`、`:375-378`、`:367-373`） | ✅ | `:3711-3745` `parseArgs`；`:719-735` `sourceAllowed` 三档；`:375-377` `if (opts?.auto) return;`；`:367-373` `BACKGROUND_AUTO_SYNC_SOURCES = {"acode","codex","every-code","reasonix"}` = 四个源 |

### B-7 §3.4 游标两代（L394–L450，任务指定必查）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 79 | `openCursorStore`（`:25-107`）按体积决定代际；`DEFAULT_ACTIVATION_BYTES = 16 MB`（`:21`）；判定在 `:77-86` | ✅ | `:21` `const DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024;`；`:77-82` `legacySize` + `shouldActivate = forceV2 \|\| (Number.isFinite(activationBytes) && activationBytes >= 0 && legacySize >= activationBytes)`；`:83-85` 否则回 `LegacyCursorStore` |
| 80 | 激活条件「≥16 MB，或已存在 manifest，或 `forceV2`」 | ✅ | `:44` `if (isManifest(manifest)) { … }` 先于体积判定（manifest 存在即走 v2）；`:78` 含 `forceV2` |
| 81 | Legacy 提交 = 全量重写（`:199-201`）；其他能力全部空实现（`:177-197`）；`codexEventStore = null` | ✅ | `:199-201` `async commit(cursors) { await writeJson(this.cursorsPath, cursors); }`；`:177-197` 六个空方法（`loadCodexFilesForPaths` / `materializeAllCodexState` / `loadOpencodeMessagesForBatch` / `materializeAllOpencodeState` / `canSkipCodexDay`→false / `readDeferredCodexAuditSyncs`→0 / `writeDeferredCodexAuditSyncs` / `clearDeferredCodexAuditSyncs`） |
| 82 | 裸状态 `{ version: 1, files: {}, updatedAt: null }`（`defaultCursorState`，`:1816-1818`） | ✅ | `:1816-1818` 逐字 |
| 83 | v2 目录布局（常量 `:13-21`）：`manifest.json` / `deferred-codex-audit.json` / `generations/<gen-…>/{generation.json, core-v3.json, codex-files/, codex-events/, opencode-messages/, opencode-fingerprints/}` | ✅ | `:13-21` 五常量 + `STORE_DIRNAME = "cursor-store-v2"`、`SHARDED_CORE_FILENAME = "core-v3.json"`、`DEFERRED_CODEX_AUDIT_FILENAME`；`commit` 内 `ensureDir` 四个子目录（`:860-863`）；代际目录名 `gen-${Date.now()}-${uuid8}`（`:1820-1822`） |
| 84 | `V2CursorStore.commit`（`:855-1016`）九步：①新代 ②`cloneGenerationFiles` 克隆（`:877-881,1349-1362`，`COPYFILE_FICLONE`，与硬链接无关）③只重写脏分片（`:899-921`）＋空分片 `delete` ④事件集重建/追加（`:923-945`）⑤写 `core-v3.json` ⑥写 `generation.json` 与 counts ⑦**最后**原子替换 `manifest.json`（`:987`）⑧只留三代（`:1005-1009`）⑨未 publish 前失败删半成品（`:1010-1015`） | ✅ | 九步逐字命中：`:877-881` `await cloneGenerationFiles({fromDirectory, toDirectory, metadata})`；`:1361` `fssync.constants.COPYFILE_FICLONE`（确为 reflink/CoW，无 `link(` 调用）；`:899-921` `dirtyFileShards` + `delete nextMetadata.codexFiles[shardKey]`；`:923-945` `rebuildEventShards` 或逐 shard 追加；`:959-963` 写 `nextMetadata.coreFile`；`:965-970` 写 `generation.json`；`:972` `maybeInjectFailure(…, "beforeManifestSwap")`；`:987` `await writeJson(path.join(this.storeRoot, MANIFEST_FILENAME), nextManifest)`；`:1005-1009` `cleanupGenerations(…, new Set([current, rollback, previous]))`；`:1010-1015` `if (!published) await fs.rm(generationDir, {recursive:true, force:true})` |
| 85 | 分片规则：`isCodexSessionCursorPathForRoots`（`:1771-1779`，`**/.codex/sessions/**`、`**/.codex/archived_sessions/**` 或位于本进程 `codexRoots` 下）；文件分片键正则 `/(\d{4})/(\d{2})/(\d{2})/` → `YYYY-MM-DD` 否则 `misc`（`:1788-1793`）；事件分片键取冒号后 10 字符（`:1800-1805`）；`safeShardFilename` 只允许 `^\d{4}-\d{2}-\d{2}$` 否则 `misc`（`:1807-1809`） | ✅ | 四处逐字；`:1774` `/\/\.codex\/(?:archived_)?sessions\//`；`:1791` `normalized.match(/\/(\d{4})\/(\d{2})\/(\d{2})\//)`；`:1803` `key.slice(separator + 1, separator + 11)`；`:1808` 正则判定 → `misc` |
| 86 | 「只加载今天需要的分片」`loadCodexFilesForPaths` / `canSkipCodexDay`（`:299-318,609-639`）；`codexRoots` 必须与扫描根同源（`sync.js:635-652`） | ✅ | `:299-317` 两轮重试加载；`:609-638` `canSkipCodexDay`（shardKey/dir metadata/inventory statKey/fileCount 四校验 + `validateProjectSummary`）；`sync.js:645-656` `codexNativeValue` → `resolveScanRoots` → `codexCursorRoots = scanRoots.codex.map(e => e.path)` → `openCursorStore({codexRoots})` |
| 87 | 跨版本兼容字段（`:974-986`）：老读者拒绝 `core-v3.json`，靠 `previous` 指向的冻结内联检查点兜底 | ✅ | `:974-986` 代码块与文档引用**逐字相同**（含注释「Older v2 readers reject core-v3.json and fall back to this frozen, fully-inline checkpoint instead of opening an empty message index.」） |
| 88 | 损坏恢复是**两级**：`manifest.rollback \|\| manifest.previous`（`：` 是逻辑或，`previous` 只在 `rollback` 为空时才轮到）（`:124-128`） | ✅ | `:124-128` `const opened = await readGeneration({storeRoot, generationId: manifest.current}) \|\| await readGeneration({storeRoot, generationId: manifest.rollback \|\| manifest.previous});` —— 确为两级而非三级 |
| 89 | `openManifestGeneration` 失败抛 `TOKENTRACKER_CURSOR_STORE_CORRUPT`（`:72-74`）；分片完整性 `buildShardIntegrity`（`:1466-1471`）/`assertShardIntegrity`（`:1473-1481`）；RETRY 与 CORRUPT 的分野（`sync.js:1069-1085` / `:1086-1087`） | ✅ | `:72-74` `throw cursorStoreCorruption(\`No valid cursor store generation in ${storeRoot}\`)`；`:1466-1471` `{bytes, sha256}`；`:1473-1481` 不一致即抛；`sync.js:1069-1085` `isCursorStoreRetry(err)` → `loadCodexFilesForPaths` 后重跑；`:1086-1087` `else if (err?.code === "TOKENTRACKER_CURSOR_STORE_CORRUPT") { throw err; }` |
| 90 | `legacyFingerprint` 检测 `cursors.json` 在 v2 建立后被外部改动即重新迁移（`:44-58`） | ✅ | `:40` `fingerprintFile(cursorsPath)`、`:44-58` `legacyChanged = !sameFingerprint(...)` → `migrateLegacyCursorState(...)` + `migratedDuringOpen = true` |

### B-8 §3.5–§3.8（L452–L571）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 91 | `scan-roots.js:3-14` 问题定义（#657/#639，「一个 30 分钟桶被重发 138 次、单日 1.2B token 报成 59B」） | ✅ | `:3-14` 注释逐字含「#639: one 30-minute bucket re-emitted 138 times, 59B tokens for a 1.2B day」 |
| 92 | 合并顺序：native → env → config（先出现者胜，realpath 去重）（`:19-23,163-202`）；相对路径一律相对 `home`（`:46-55`）；非 native 且不可用的根仍保留（`:158-162,228-232`） | ✅ | `:19-23` 注释三条顺序；`:174-179` `base(native)` → `resolveEnvRoot(env)` → `configured(config)`；`:187-189` `identityKey` 去重；`:51-54` `~/`、`~\`、绝对、否则 `path.resolve(home, trimmed)`；`:157-162` 注释「non-existent or unreadable configured roots are kept in the list」+ `:228-231` `.filter(e => e.origin !== "native" && e.exists)` |
| 93 | 三态 `scanRootDirState`（`:89-110`）：`{exists:true}` / `{exists:false,error:null}` / `{exists:false,error:code}`；`opendirSync(...).closeSync()` 探「可列举」；`describeScanRootState` 渲染 `" (unreadable: <code>)"` 与 `" (missing)"`（`:248-253`）；函数本身只返回字符串、无强制点 | ✅ | `:97-110` 逐字（含 `// Probe listability without reading entries; large roots stay cheap.`）；`:250-252` `return entry.error ? \` (unreadable: ${entry.error})\` : " (missing)";`；`:89-96` 注释是调用方意图（「so callers can say "unreadable" instead of "missing"」），无断言/抛错点 |
| 94 | WSL 探测：`wsl.exe -l -v` + UTF-16LE + 清洗 `\0`/BOM + `*` 默认标记 + 版本号（`:26-55`）；默认发行版优先（`:91`）；`_cachedDistros` + strict 重试（`:57-97`）；非 0 退出时用 `-l -q` 成功且空确认（`:69-80`）；home 为 `\\wsl$\<distro>\home\<user>\<providerDir>`，v1 优先 `\\wsl.localhost\`、v2 优先 `\\wsl$`（`:180-204`）；模式五值（`:18-24,106-109`，默认 `wsl-first`）；`:149` 非 win32 返回双 null；「只返回 native」落点 `install-resolver.js:21-23`；`mapWslCwdToUnc`（`:206-242`）；`snapshotSqliteDb` 复制 db + `-wal`/`-shm`/`-journal`（`:244-262`） | ✅ | 全部逐字命中：`:29` `utf16 ? buf.toString("utf16le") : buf.toString("utf8")`；`:33` `.replace(/\0/g, "").replace(/\uFEFF/g, "").trim()`；`:45-48` `cells[0] === "*"`；`:51` 版本号；`:91` `sort((a,b) => (b.isDefault?1:0) - (a.isDefault?1:0))`；`:62` 缓存 + `:87` `if (strict && error?.code !== "ENOENT" && !confirmedEmpty) throw error;`；`:77-78` `-l -q` 且 `cleanWslOutput(quiet) === ""`；`:193-195` v1/v2 前缀顺序；`:18-24` `WSL_MODES` 五值、`:108` 默认 `wsl-first`；`:149` 双 null；`:21-23` 逐字；`:232-241` cwd 重锚 + `WINDOWS_DRIVE_PREFIX_RE` 跳过 `/C:/…`；`:250-255` 后缀三件套复制 |
| 95 | `install-resolver`：`requireAnyChild` 折进探测本身（`:31-33`）、理由见 `sync.js:761-772`；`union` 把模式降级为优先级、前提是路径无关身份去重（`:6-13`）；实测 `union` 只用于 codex（`sync.js:782-787`，`requireAnyChild:["sessions","archived_sessions"]`）与 acode（`:815-820`） | ✅ | `:31-33` `wslExists = requireAnyChild ? (p) => Boolean(pathExists(p,…)) && hasAnyChild(p, requireAnyChild, …) : deps.existsSync`；`sync.js:767-772` 注释逐字「Pass `wslDir` rather than a pre-resolved home so requireAnyChild is folded INTO the discovery probe」；`install-resolver.js:14-18` 注释逐字含 `#27`；全 `sync.js` 检索 `union: true` **恰 2 处**（`:786`、`:819`），与文档「实测只用于 codex 与 acode」**完全一致** |
| 96 | `multiInstallParse`（`multi-install-parser.js:8-59`）三条不变量：①单安装回落扁平游标、幸存者由 `wsl.isUncPath` 判定（`:13-22`）②扁平→命名空间迁移种子默认全塞、只有探针恰好命中一个才让另一个空起步（`:61-85`）③RETRY 异常路径把 `ns` 写回 `cursors[providerName]`（`:49-54`） | ✅ | `:21` `const survivorKey = wsl.isUncPath(paths[installKeys[0]]) ? "wsl" : "native";`；`:14-20` 注释逐字（wsl-first 下不得「捐赠」游标）；`:70-84` `resolveMigrationSeedKeys` → `hits.length === 1 ? hits : installKeys`；`:67-69` 注释逐字「an install re-parsed without its dedup state would double-count its entire history, and a bounded backfill gap is the cheaper failure」；`:49-53` `if (parseErr?.code !== "TOKENTRACKER_CURSOR_STORE_RETRY") { cursors[providerName] = ns; } throw parseErr;` |
| 97 | `async-queue-reader.js`（117 行）六条：签名命中直接返回（`:35`，并发注释 `:12-15`；`:99-104` 是 ENOENT 清理与降级）②只把完整行计入 `consumedBytes`、残行放 `tail` 且 key 不进已提交 map（`:64-69,87-91`）③每 64 KB 让出事件循环（`:5,79`）④坏行只 `malformed++` 并计数告警（`:51-57,95`）⑤同路径并发合并（`:109-114`）⑥LRU 上限 4 与出错回退缓存（`:16,94`、`:102-103`） | ✅ | 文件实为 **117 行**；`:8-10` `signature` 用 `dev:ino:size:mtimeNs:ctimeNs`；`:35` `if (cached?.signature === nextSignature) return cached.rows;`；`:40-47` head+anchor 双探针；`:68-69` `lastIndexOf(0x0a, …)` 推进 `consumedBytes`；`:87-91` `tail` Map + 注释「their provisional key never enters the committed map」；`:5` `CHUNK_BYTES = 64 * 1024`、`:79` `await yieldToRequests()`；`:56` `catch { malformed++; }`、`:95` `console.error(\`[LocalAPI] ${label}: skipped ${malformed} malformed line(s)\`)`；`:110-112` `pending`；`:16` `maxEntries = 4`、`:94` while 驱逐、`:102-103` `console.error` + `return cached?.rows \|\| []` |
| 98 | `jsonl-lines.js`：自研字节级生成器（`:3-11` 注释说 `readline` 会把 U+2028/U+2029 当分隔符）；只认 `\n`（`:57`）；`maxPhysicalBytes` 超限抛 `PhysicalJsonlLimitError`（`:12-18,61-63,96-98`）；`maxRecordBytes` 超限不缓冲不解码直接产出 `{line:null, utf8Valid:null, oversized:true, physicalBytes, terminated:true}`（`:65-73`），末行未闭合带 `terminated:false`（`:111-114`），`physicalBytes` 仍计入预算；`TextDecoder(fatal)` + `invalidUtf8: "throw"｜"record"`（`:20-27,34-36,84-90`）；`stripCr` 对调用方不可选（对外 opts 只有三项 `:29-33`，调用点硬编码 `{stripCr:true}` `:85`）；末行未终止由 `terminated:false` 标记（`:127`） | ✅ | 逐条逐字命中（含 `:72` 与 `:113` 的两种 oversized 形状差异：前者 `terminated:true`、后者 `terminated:false`） |
| 99 | `normalizeQueueRow`（`local-api.js:170-200`）：读侧纠错、不重写队列；`isLegacyInclusiveCodexRow` 是**不变量**而非版本号（`cached > 0 && input >= cached && total === input + output`）（`:155-168`）；Cursor/Antigravity 提升 `billable_total_tokens`（issue #106）；同函数被上传路径复用（`sync.js:4509`） | ✅ | `:172-177` 减缓存；`:191-198` `if (sourceName === "cursor" \|\| sourceName === "antigravity") { … if (totalTokens > 0 && billable < totalTokens) … }`；`:156-167` 不变量三条件 + 注释逐字；`:179-190` 注释含 issue #106 与 `e69e2746a`；`sync.js:4509` `bucket = require("../lib/local-api").normalizeQueueRow(bucket);` |
| 100 | 上传侧：`iterateQueueLines`（`upload-queue.js:22-43`）按 `\n` 切、跨块拼接、`complete:false` 标尾行；注释强调 offset 是**字节位置**（`:20-21`）；`drainQueueToCloud` 只上传完整行（`sync.js:4449`）；单批上限同时约束桶行与 `account_session_state` 行（`:4489,4512`） | ✅ | `:22-43` 逐字；`:20-21` 注释逐字「Upload offsets are byte positions, not JavaScript string lengths.」；`sync.js:4449` `if (!record.complete) break;`；`:4489` 与 `:4512` 两处 `if (linesRead >= maxBuckets) break;` |
| 101 | 其余读取面：`readBodyLimited`（`:722-740`）、头像 512 KB/64 条（`:46-47`）、本地 API 触发的 sync 有 120s 预算（`SYNC_TIMEOUT_MS = 120_000`、`:22`、超时 SIGTERM + `SYNC_TIMEOUT` `:778-792`）、env 白名单式透传（`:754-762`） | ✅ | 五处逐字；`sync.js:295-300` 注释「120-second child-process budget」与 `:297` 呼应 |
| 102 | §3.7 调度表四条：notify 20s（`init.js:1519-1536`）、omp 12s（`omp-hook.js:66,99-109`）/ openclaw 15s（`openclaw-session-plugin.js:363,502-518`）、serve 60s（`serve.js:35,294-366`）、自动重试上限 2h（`sync.js:4169-4293`） | ✅ | `omp-hook.js:66` `const MIN_INTERVAL_MS = 12_000;`；`openclaw-session-plugin.js:363` `const SESSION_TRIGGER_THROTTLE_MS = 15_000;` + `:502-518` `allowTrigger`；`serve.js:35` `NATIVE_BACKGROUND_SYNC_INTERVAL_MS = 60_000`；`sync.js:4310` `AUTO_RETRY_MAX_DELAY_MS = 2 * 60 * 60 * 1000` |
| 103 | `startNativeBackgroundSync` 六设计点：仅 Windows（`:303-304`）、桌面托管让位（`:309`）、子进程（`:311-315` 注释「several provider parsers read SQLite databases via execFileSync…」）、强制 `native-only`（`:335-342`，注释提到 msrdc 抢焦点）、单飞防重入（`:332-334`）、关停（`:281-288`） | ✅ | 六处逐字（`:334` `if (inFlight) return inFlight;`；`:344` `sync(["--auto","--background","--all-local-sources"], {env: backgroundEnv})`；`:285` `setTimeout(() => process.exit(0), 3000);`） |
| 104 | 6 小时网络心跳（`serve.js:266-273`）与 30s 锁心跳同名不同物 | ✅ | `:266-272` `maybeSendHeartbeat` + `setInterval(sendHeartbeat, 6 * 60 * 60 * 1000).unref();`；`sync.js:3699-3704` 尾部同函数 + 注释「Anonymous daily heartbeat (shared 24h throttle…)」 |
| 105 | 冷扫描审计（`sync.js:3763-3783`）：①无状态 ②距上次全量 ≥ 24h（`:294`）③累计 sync 次数 ≥ 288（`:301`）；时钟回拨保护 `lastFullScanAtMs - nowMs > 5 * 60 * 1000`（`:3772`）；`recordCodexColdScanAudit` 6 字段（`:3785-3819`）；`skipNoOpCursorCommit` 时增量写 `deferred-codex-audit.json`（`cursor-store.js:640-660`） | ✅ | `:3769`–`:3782` 三分支逐字；`:3772` 逐字；`:3797-3816` 六字段 `{version, lastFullScanAtMs, lastFullScanAt, syncsSinceFullScan, lastSkippedFiles, updatedAt}`；`cursor-store.js:640-661` 读/写/清三方法；`sync.js:965-969` 唯一调用点 |
| 106 | 自动重试：`auto.retry.json` payload 七字段；分离 `node -e` 脚本 `setTimeout` 后重读 marker（`:4278-4285`）；上限 2h（`:4310`）；marker 权威（`:4244-4247`）；已有更早计划不重排（`:4186-4202`）；`AUTO_RETRY_NO_SPAWN`/`autoRetryNoSpawn`（`:4217-4219`）；触发条件（`:3654-3676`） | ✅ | `:4230-4242` `buildAutoRetryPayload` 七字段；`:4270-4290` 脚本体内 `if (!retryAtMs \|\| Date.now() + 1000 < retryAtMs) process.exit(0);`；`:4244-4247` `clearAutoRetry` unlink；`:4186-4201` 早于现有计划则 `scheduled:false`；`:4217-4219`；`:3661-3676` `pendingBytes <= 0 \|\| !canUpload()` → `clearAutoRetry`，否则 `opts.auto && uploadAttempted && !opts.publishAccount` → `scheduleAutoRetry` |
| 107 | §3.8 上传三重约束：批大小默认 200、硬上限 `MAX_INGEST_BUCKETS = 500`（`:4313,4329`）；批数默认 5、`--drain` 时 100（`:3563`）；总预算 `UPLOAD_BUDGET_MS = 90_000` 并用 `AbortSignal.timeout` 传给 fetch（`:168,4362`）；`community` 压到 10（`:4392`）；每批成功即写 offset（`:4397-4400`）；同批同键去重（`:4510`）；`account_session_state` 取严格更新（`:4478-4484`）；超时但有成功批次不抛错；`AUTO_UPLOAD_CONFIG`（`:162-167`） | ✅ | 全部逐字命中（`:4315` 签名 `maxBatches = 5, batchSize = 200`；`:4329` `Math.min(Math.max(1, Math.floor(Number(batchSize \|\| 200))), MAX_INGEST_BUCKETS)`；`:4405-4410` `if (!requestTimedOut \|\| batches === 0) { error.uploadResult = resultSoFar(); throw error; }`） |
| 108 | `compactPendingQueue`（`upload-queue.js:64-132`）：只在 `pendingBytes >= 1_000_000` 且 `offset-1` 处是 `\n` 时动手（`:73-78`）；只重写 `[offset,size)`、原样字节复制 `[0,offset)`；pending 区每键留最后一条（`:79-91,105-110`）；`account_session_state` 用 `verifiedMs` 严格更大；替换前竞态校验（`:111-121`） | ✅ | `:73` `pendingBytesBefore < minPendingBytes` → unchanged；`:74-78` 边界字节 `!== 10` → unchanged；`:79-91` `latest` Map + `snapshot.verifiedMs > previous.verifiedMs`；`:97-103` 复制已上传区；`:105-109` 只写「仍是最新」的行；`:111-121` `output.sync()`→`output.close()`→注释→`input.close()`→`fs.stat` 三元比对→`throw "Queue changed during pending compaction"`→`fs.rename` |

### B-9 §4 解析层（L575–L871，任务指定必查）

| # | 断言 | 判定 | 依据 |
| --- | --- | --- | --- |
| 109 | 「没有统一的返回契约」：5 个转发薄壳（`parseOmpIncremental:16087`、`parseOmoIncremental:16100`、`parsePiIncremental:16689`、`parsePrimeAgentIncremental:16699`、`parseMinimaxCodeIncremental:16775`），其余 40 个自持 return | ✅ | 独立脚本重建：45 个 `parse*Incremental` 中**恰 5 个**无对象字面量 return，且函数定义行号与文档逐个相同；5 个函数体形态确为 `return parseOmpLikeIncremental({ ...options, stateKey, source, … })` / `parsePiLikeIncremental({...})` |
| 110 | 40 个自持 return **去重后共 11 种键集合** | ✅ | 脚本重建得 **11 种**，与文档 11 完全一致 |
| 111 | 骨架分布：骨架 C 19 / 骨架 A 6 / 骨架 D 5 / 骨架 B 3 / 骨架 C+扩展 7 单例 | ✅ | 脚本重建：`{recordsProcessed,eventsAggregated,bucketsQueued}` = **19**；`{filesProcessed,…}` = **6**；`{recordsProcessed,…,projectBucketsQueued}` = **5**；`{messagesProcessed,…}` = **3**；单例 7（Copilot/`usageClaims`、CopilotAppDb/`dbErrors`、TraeCnApi/`skippedRows`、Dsh/`deferredMigrations`、Trae 7 键、CommandCode 7 键、CopilotSessionStore 9 键）——**19+6+5+3+7 = 40** ✅ |
| 112 | 代表函数行号（骨架 C：`parseClaudeScienceIncremental`(7137)、`parseKiroCliIncremental`(9268)、`parseZedIncremental`(12839)、`parseGooseIncremental`(14767)、`parseCraftIncremental`(17297)；骨架 A 6 个：705/1239/1409/1562/1922/21394；骨架 D 5 个：14456/15542/16084/16686/20551；骨架 B 3 个：6027/6398/6788） | ✅ | 脚本逐一比对，**19 个行号逐个命中** |
| 113 | 40/40 都有的键只有 `eventsAggregated` 与 `bucketsQueued`；计数键在三种之间跳——`recordsProcessed` 31 / `filesProcessed` 6 / `messagesProcessed` 3 | ✅ | 脚本重建：公共键恰为 `eventsAggregated,bucketsQueued`；计数键分布 **31 / 6 / 3**，与文档逐位吻合 |
| 114 | 三个加宽签名逐字：Copilot(`:18111`)+`usageClaims`、Trae(`:22175`)7 键、CopilotSessionStore(`:19003`)9 键 | ✅ | `:18111` `return { recordsProcessed, eventsAggregated, bucketsQueued, usageClaims: recentOtelUsageEvents };`；`:22175` 单行 7 键（含 `recordsSkipped, estimatedRecords, unpricedRecords, errors`）；`:19003-19013` 9 键（`active, healthy, adoptedThisRun, canonicalDbPaths, recordsProcessed, eventsAggregated, bucketsQueued, dbErrors, usageClaims`） |
| 115 | `schemaMigrated`/`cursorUnchanged` 全库只在 Command Code 一处（`:24670-24671`）；其余 44 个 parser 无条件重写 `cursors.hourly`（`:692-697,1227-1232`） | ✅ | `:24664-24672` 唯一 7 键返回含这两个键；`:692-693` 与 `:1227-1228` 均无条件 `hourlyState.updatedAt = …; cursors.hourly = hourlyState;`（无 `cursorUnchanged` 门控） |
| 116 | 三个「假签名」陷阱：`:7270` = `recordsProcessed: total`、`:22791` 同理（真扩展键是 `skippedRows`）、`:20551` = `recordsProcessed: eventsAggregated`（键不重复但绑定语义可疑） | ✅ | 三处逐字命中（`:20551-20556` 多行返回体里 `recordsProcessed: eventsAggregated,` 后紧跟 `eventsAggregated,` —— 键确不重复） |
| 117 | 早退分支会让**同一函数出现第二种键集合**，举例 `parseTraeCnApiIncremental:22634`（3 键、没有 `skippedRows`）、`parseCopilotIncremental:17944,17963`、`parseTraeIncremental:22163` | ⚠️ | 只有第一例成立：`:22634` = `{recordsProcessed: 0, eventsAggregated: 0, bucketsQueued: 0}`（3 键）vs 主 return `:22791`（4 键，多 `skippedRows`）；`:17944-17949` 与 `:17963-17968` 的键集合与主 return `:18111`（4 键）**完全相同**，`:22163` 与主 return `:22175`（7 键）**完全相同**——三例同属早退，但后两例不构成「第二种键集合」。见必须修 #4 |
| 118 | `onProgress` 形状 `{index, total, filePath, filesProcessed, eventsAggregated, bucketsQueued: touchedBuckets.size}`（`:671-679`） | ✅ | `:671-679` 逐字 |
| 119 | 文件发现族：`mapConcurrent`(72)/`FILE_METADATA_CONCURRENCY = 32`(70)、`listRolloutFiles`(91) 严格三层、`listRolloutDayFiles`(143) statKey 缓存、`listRolloutFilesDeep`(201) 任意深度、`listClaudeProjectFiles`(219)/`walkClaudeProjects`(5013)、`safeReadDir`(5005) 失败返回 `[]`、`isRolloutFileName`(179)、`directoryInventoryStatKey`(186) 四元组 | ✅ | 八处逐字（`:186-192` = `ino:size:mtimeMs:ctimeMs`） |
| 120 | 增量三态（`:487-518`）与四情形表 | ✅ | `:488-492` 逐字与文档代码块相同；`:493-500` `rebuildingCodexBaseline` 条件 = 同 inode + 未截断 + `prevOffset > 0` + `prevOffset < st.size` + `(!prev.lastTotal \|\| typeof prev.lastTotal !== "object")` **逐字吻合**；`:501-507` 两句置 null；`:513-517` pre-#652 注释 |
| 121 | 稳态短路（`:538-555`）与 `projectSettled`/`projectContextOnlyScan` | ✅ | `:538-543` 条件逐字；`:531-537` `projectContextOnlyScan`；`:1121-1134` Claude 侧 `idle`/`projectKnownAbsent`/`projectSettled`；`:1115-1120` 注释逐字「A file that's still growing gets retried」 |
| 122 | `endOffset = st.size` 全文件**恰 3 处**（`:2291`/`:2682`/`:2793`），只有 `:2793` 落到 token 游标 | ✅ | 正则 `endOffset = st\.size` 全文件命中 **3** 处，行号逐个相同；`:2291` 属 `parseRolloutFile`（`:2294-2302` 早退、`:2652` 正式返回 `committedEndOffset`）；`:2682` 属 `scanRolloutProjectFileContexts`（`:2764-2765` `endOffset: committedEndOffset`）；`:2793` 属 `parseClaudeFile`，`:2889` `return { endOffset, eventsAggregated };`；调用侧 `:1194` `offset: result.endOffset,` |
| 123 | OpenClaw 有显式行边界检查（`:1949`） | ✅ | `:1952` 函数 `openclawOffsetEndsAtLineBoundary`（位于 `:1949` 附近注释块内），读 `offset-1` 一字节判定 `0x0a \|\| 0x0d` |
| 124 | Claude 路径**没有**该保护：`readline` yield 半行 → `JSON.parse` 失败被吞（`:2843-2847`）→ 游标仍推进到 `st.size`；非严格永久（`:488-504`） | ✅ | `:2843-2847` `obj = JSON.parse(line); } catch (_e) { continue; }`；`:2838-2839` 无行边界校验；`:1049-1054` 只在 inode 变化/截断时重置 |
| 125 | 坏行容错：`bad_lines\|badLines\|bad_line` 在 `src/lib/` 零命中；Codex `invalidRecordPolicy` 默认 `"skip"`（`:261`）、非法值 `TypeError`（`:263-265`）、`"throw"` 只被血缘重建用（`sync.js:5774`）、skip 下静默 `continue`；Claude 完全静默（`:2812-2816`、`:2843-2847`）；`countMalformedCodexProjectQueueRows`（`sync.js:6436`）全库只有一个调用点（`:5824`），只影响修复是否执行 | ✅ | 全部逐字命中（`:263-265` `if (invalidRecordPolicy !== "skip" && invalidRecordPolicy !== "throw") throw new TypeError(…)`；`sync.js:5774` `invalidRecordPolicy: "throw",`；`:5824-5830` 畸形行 > 0 则 `return false`） |
| 126 | 超大文件保护是三层降本、非单文件上限；`parseRolloutFile` 只传 `invalidUtf8`（`:2396-2397`）；有 `maxRecordBytes` 保护的只有 OMP 族（`:15715,15717`，默认 64 MB） | ✅ | `:2396-2397` `const invalidUtf8 = …; for await (const record of physicalJsonlRecords(stream, { invalidUtf8 }))`（无 `maxRecordBytes`）；`:15715` `const OMP_LIKE_MAX_RECORD_BYTES = 64 * 1024 * 1024;`、`:15717` `scanOmpLikeTranscript(filePath, startOffset, maxRecordBytes, onEntry)` |
| 127 | 半小时桶：`toUtcHalfHourStart`（`:4173`）语义（任意可被 `Date` 解析的串、`>=30 → :30`、秒毫秒清零、全 UTC、`toISOString()`、非法返回 `null`、调用方 `if (!bucketStart) continue;`）；两处独立实现（全库 `function toUtcHalfHourStart` 恰 2 命中）；副本 `claude-categorizer.js:1175`；改动任一处不会让另一处失败 | ✅ | 主实现 `:4173-4190` 逐字与文档代码块相同；两处调用方逐字（`:2605-2606`、`:2867-2868`、Claude Science `:7071-7074`）；`grep function toUtcHalfHourStart` 在 `src/` **恰 2 命中**（`src/lib/rollout.js:4173` 与 `src/lib/claude-categorizer.js:1175`）；副本 `:1175-1191` 为单条 `return new Date(Date.UTC(...)).toISOString();` 表达式（文档写作「单行链式返回」，指单条语句而非单物理行，语义成立） |
| 128 | 「`test/` 下引用 `toUtcHalfHourStart` 的两个测试都 `require("../src/lib/rollout")`；`claude-categorizer.js` 的副本无人测」 | ✅ | `test/sync-droid-dup-session-repair.test.js:29` 与 `test/trae-entitlement.test.js:29` 均从 `../src/lib/rollout` 解构；`test/` 下无任何文件 import `claude-categorizer` |
| 129 | 「`test/trae-entitlement.test.js:29` 从 `../src/lib/rollout` 导入，`:130-137` **三条断言**钉死 `:00`/`:30` 取整」 | ❌ | `:130` 起该 test 内**只有 2 条** `assert.equal`（`:131-134` 与 `:135-138`）。见必须修 #1 |
| 130 | `rowDayKey`（`local-api.js:213`）：有上下文走 `getZonedParts` + `formatPartsDayKey`，否则 `hs.slice(0,10)`（`:224`）；`getZonedParts` 用 `Intl.DateTimeFormat("en-CA", {timeZone, hourCycle:"h23"})`（`codex-rollout-parser.js:84-97`），失败退化 `offsetMinutes`（`:103-110`）；写入侧唯一用「本地日历日」的是 Codex 冷文件跳过（`rollout.js:923` + `:893`） | ✅ | 四处逐字；`local-api.js:213-225` 与文档代码块相同；`codex-rollout-parser.js:88-94` formatter 选项逐字、`:103-109` offset 平移；`rollout.js:923-926` 用 `getFullYear/getMonth/getDate`（本地） |
| 131 | 桶键三原语（`:4222-4231`）与三常量 `DEFAULT_SOURCE="codex"`/`DEFAULT_MODEL="unknown"`/`BUCKET_SEPARATOR="\|"`（`:55-57`）；`groupBucketKey`（`:4233-4236`）、`parseBucketKey`（`:4238-4249`）；`module.exports` 起点 `:24675` 且含 `bucketKey`/`toUtcHalfHourStart`/`totalsKey`/`groupBucketKey` | ✅ | 全部逐字命中（`:4222-4231` 与文档代码块相同；`^module.exports` 唯一命中 `:24675`，导出清单含这四个名字） |
| 132 | `normalizeHourlyState`：形如 `{version:3, buckets, groupQueued, updatedAt}`；version < 2 把模型名抹成 `unknown` 并把 `queuedKey` 提升为 group 级，返回 version 3（`:3634-3652`）；`:399-409` 是 `syncDiagnostics` 字段赋值区、与本函数无关 | ✅ | `:3627-3652` 逐字；`:3640` `bucketKey(source, DEFAULT_MODEL, hourStart)`、`:3643` `groupQueued[groupBucketKey(source, hourStart)] = value.queuedKey`、`:3647` `version: 3`；`:399-409` 实为 `syncDiagnostics` 相关（`content_files_read` 等） |
| 133 | `initTotals()` 9 个数值字段（`:4053-4065`）；5 个可选子集标记（`:4073-4079`）永不参与求和、值为 0 时不写出（`prioritySubsetOf`，`:4090-4097`）；`total_cost_usd` 累加带 $10^{-6}$ 取整（`addTotals`，`:4111-4113`，`USD_TICKS_PER_USD`） | ✅ | `:4054-4064` 恰 9 字段；`:4073-4079` 恰 5 对；`:4090-4096` 只在 `value > 0` 时写入；`:4111-4113` `Math.round(((target.total_cost_usd \|\| 0) + (delta.total_cost_usd \|\| 0)) * USD_TICKS_PER_USD) / USD_TICKS_PER_USD` |
| 134 | §4.5 主队列行构造（`:3328-3345`）逐字段 | ✅ | `:3329-3344` 字段顺序与文档代码块**逐字相同**（`source/model/hour_start` → 六桶 → `billable_total_tokens: totals.billable_total_tokens ?? totals.total_tokens` → `total_cost_usd: totals.total_cost_usd \|\| 0` → `usage_precision: usagePrecision \|\| undefined` → `...prioritySubsetOf(totals)` → `conversation_count`） |
| 135 | `usage_precision` 参与 `queuedKey` 计算（`:3325-3326`） | ✅ | `:3325-3326` `const usagePrecision = bucket.usage_precision \|\| null; const key = usagePrecision ? \`${totalsKey(totals)}\|${usagePrecision}\` : totalsKey(totals);` |
| 136 | `conversation_count` 两条主线含义不同：Codex 每条被接受的 delta 恒写 1（`:2554`）；Claude assistant 行恒 0（`:2865`）、主会话 user 文本行 +1（`:2807-2831`） | ✅ | `:2554` `delta.conversation_count = 1;`；`:2865` `delta.conversation_count = 0;`；`:2810-2834` 判定 `isMainSession && line.includes('"type":"user"')` + `hasText` + 去重后 `userBucket.totals.conversation_count += 1;` |
| 137 | 控制行 `account_session_state` 与 `account_sync_watermark`（读侧显式丢弃，`sync.js:4497`） | ✅ | `:4497` `if (bucket?.kind === "account_sync_watermark") continue;`；`local-api.js:205` 两类都 `return null` |
| 138 | 项目队列行非主行同构：多 `project_ref`+`project_key`、少 `model`/`usage_precision`/`priority_*`（`:3515-3532`）；缺键在 `:3513` 直接跳过 | ✅ | `:3516-3529` 字段清单确无 `model`、无 `usage_precision`、无 `priority_*`；`:3513` `if (!projectRef \|\| !projectKey) continue;` |
| 139 | 两套读侧 key（`local-api.js:202-211` 与 `:145-153`）与后写覆盖（`async-queue-reader.js:55,86`）；日聚合跨模型求和（`:260-298`） | ✅ | `:207` `key: \`${row.source \|\| ""}\|${row.model \|\| ""}\|${row.hour_start \|\| ""}\``；`:149` `key: \`${row.project_key \|\| ""}\|${row.source \|\| ""}\|${row.hour_start \|\| ""}\``；`:74` `target.set(entry.key, entry.row)` 与 `:86` `for (const [key,row] of changes) seen.set(key,row)`；`:260-298` `byDay` 逐 row 累加 |
| 140 | §4.6 幂等：写前比对 `queuedKey`（`:3327` 前身 `:3325-3326`）相等即跳过；写成功后 `bucket.queuedKey = key`（`:3346`）只记在内存、`commit()` 才落盘；指纹含 `total_cost_usd` 与 `conversation_count`（`totalsKey`，`:4155-4171`） | ✅ | `:3327` `if (bucket.queuedKey === key) continue;`；`:3346` `bucket.queuedKey = key;`；`:4156-4170` 九个字段 + 条件追加 `\|p:${priority.join(",")}`；`:4167-4168` 注释逐字「Appended only when a priority subset exists, so buckets that never see the tier keep their previously persisted queuedKey and are not re-enqueued.」 |
| 141 | 撤回行三种：①unknown 桶撤回（`:3291-3316`，`retractedUnknownKey` 去重）②对齐模型撤回（`:3273-3290`）③整源撤回（`retractStaleSourceQueueRows`，`:19990`，写点在 `:20031`） | ✅ | 三处逐字（`:3297` `if (unknownBucket.retractedUnknownKey !== zeroKey) {` … `:3314` `unknownBucket.retractedUnknownKey = zeroKey;`；`:20031` `await fs.appendFile(queuePath, \`${lines.join("\n")}\n\`, "utf8");`） |
| 142 | `unknown → dominant` 对齐三函数与触发逻辑（`:3542-3558`、`:3566-3607`、`:3609-3625`、`:3258-3360`、`:3317`、`:3320-3324`、`:3353-3356`、`:3574`） | ✅ | 全部逐字（`pickDominantModel` 并列取字典序小：`:3551` `(total === dominantTotal && model < dominantModel)`；`collectCodexDominantModels` 只认 `codex`：`:3574` `if (source !== DEFAULT_SOURCE) continue;`；`findNearestCodexModel` 按时间距离、并列取更早 `:3619`；`:3317` 清 `alignedModel`；`:3321-3323` `cloneTotals` + `addTotals(unknownBucket.totals)`；`:3353-3356` every-code 分支） |
| 143 | 增量范围：只有被 touch 的分组参与重算（`:3243`） | ✅ | `:3243` `if (!recomputeGroups.has(groupKey)) continue;` |
| 144 | 写路径：append 主队列 `queue.jsonl` **至少 6 处**（`rollout.js:3483/20031/22594` + `sync.js:5126/5207/5281`），另有 `sync.js:4114` fallback、`rollout.js:3536` project 队列；原子重写见 `compactPendingQueue` 与 `repairClaudeQueueFromGroundTruth`（`sync.js:6844`）；**没有针对 `queue.jsonl` 的文件锁**，依赖外层 `sync.lock`（`sync.js:552-559`）；`local-api.js` 触发的是 `spawn(tracker.js sync …)`（`:742-764`） | ✅ | 全 `src/` 检索 `appendFile(Sync)?\(` 共 10 命中：主队列**恰 6 处**（`rollout.js:3483/20031/22594`、`sync.js:5126/5207/5281`）+ `sync.js:4114` fallback + `rollout.js:3536` project + 2 处与用量无关（`init.js:1514` debug log、`skills-manager.js:145` activity）；`sync.js:552-553` `const lockPath = path.join(trackerDir, "sync.lock"); const lock = await acquireSyncLock(...)`；`local-api.js:742-746` 确为 spawn `TRACKER_BIN` / `["sync"]` |
| 145 | §4.7 三个事实：只追加（`fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8")`，`rollout.js:3483`）；读端同键覆盖（`async-queue-reader.js:55,86`、`sync.js:4510`、`sync.js:249-256` 云端整行 upsert）；写者崩溃只会「多写一行」 | ✅ | 三处逐字；`:3483` 与文档引文完全相同；「多写不会少写」的推理与 §2.3 的写序（append 在前、commit 在后）一致 |
| 146 | §4.7 代价：`mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal`（**取最大**，`:6540`），再由 `collapseModelTotalsByHour` 跨模型求和（`:6564-6571`） | ✅ | `:6529` 函数定义、`:6540` `setMaxTotal(modelTotals, \`${row.source}\|${model}\|${row.hour_start}\`, Number(row.total_tokens \|\| 0));`、`:6542` `return collapseModelTotalsByHour(modelTotals);`；`:6564-6571` 逐字；`setMaxTotal` 定义 `:6585` |

## C. 内部一致性与矛盾

### C1. 同一事实两处写法（逐项对照，未发现冲突）

| 事实 | 出现位置 | 是否一致 |
| --- | --- | --- |
| `rollout.js` 24932 行 / 942,266 字节 | §1.1（L82）、§1.3 表（L113）、§5 行号口径段（L876） | ✅ 三处一致 |
| GBK 漂移「单文件最多 15 行、全 `src/` 累计 52 行」 | 元信息（L15）、§1.3（L96）、§5（L876） | ✅ 三处一致，且本轮实测复现 |
| 45 家拆解 `12 + 1 + 17 + 15 = 45` | §3.2（L302）与 §13.6 台账（L4289） | ✅ 一致，且本轮逐分支重数成立 |
| append 主队列「6 处」 | §4.6（L856）与 §13.1 R1-A 台账（L4208） | ✅ 一致，本轮 grep 复现恰 6 处 |
| `cmdSync` 3194 行 | §3.3（L320）与 §13.5 E4 修正记录（L4269） | ✅ 一致（实测 516–3709） |
| `skipNoOpCursorCommit` 13 个条件项 | §3.3（L380）与 §13.1（L4208）/§13.6（L4287 附近） | ✅ 一致 |
| pendingBytes 只按主队列 | §2.3（L168）与 §3.3 表第 22 行（L335） | ✅ 一致（同一 `:3654-3659`） |
| 锁心跳 30s vs 网络心跳 6h | §3.7（L546）显式区分 | ✅ 无歧义 |
| §2.4 目录树里 `cursor-store-v2/` 与 §3.4 布局 | §2.4（L191）与 §3.4（L419-428） | ✅ 一致（常量 `:13-21` 对齐） |

### C2. 结构检查

- **代码围栏**：范围内 33 对 ` ``` `，**全部闭合**，无未闭合围栏。
- **表格列数**：脚本对连续 `|` 行分块校验表头/分隔行/数据行列数，报 1 处异常——`L104` 的 `` `(Get-Content $p \| Measure-Object -Line).Lines` `` 含**转义管道符 `\|`**，按 Markdown 语义仍是 2 列；**这是脚本假阳性，不是文档问题**。范围内共 15 张表，其余 14 张列数（表头/分隔行/数据行）全部一致。
- **断裂引用**：无（无「有行号无文件」的孤立引用；裸行号均可按同行/小节上下文归属）。

### C3. 结论与源码方向

逐条比对后**未发现方向相反的结论**。三处最容易被读反的地方经核对均为正确方向：reasoning 在 Codex 侧是子集标记（`pricing/index.js:248-256` 白名单置 0）、Claude 侧是独立加成项（§6 范围，本轮未复核其余）；`isLegacyInclusiveCodexRow` 是「旧行 input 含缓存」的保守不变量；`needsHistoricalRolloutDedup` 的兜底方向是「宁可重扫也不漏去重」。

## 必须修清单（按严重度）

| # | 级别 | 位置 | 现写法 | 问题 | 建议改法 |
| --- | --- | --- | --- | --- | --- |
| 1 | **P1 计数错（事实）** | L703（§4.3） | 「`test/trae-entitlement.test.js:29` 从 `../src/lib/rollout` 导入，`:130-137` **三条断言**钉死 `:00`/`:30` 取整」 | 该 test 内**只有 2 条** `assert.equal`（`:131-134`、`:135-138`），第三条不存在 | 「`:130-138` 两条断言」 |
| 2 | **P1 引用错（事实）** | L151（§2.2 表「计价」行） | 「计价输入是五列之和，`total_tokens` 不参与计费（`src/lib/pricing/index.js:248-256`）」 | 结论正确但引用区间不含五列算式——五列算式在 `:257-264`，`:248-256` 是 reasoning 门控 | 区间改为 `:248-264`（或 `:257-264`） |
| 3 | **P1 事实歧义** | L122（§1.4） | 「`~/.gemini` 下 **207 个文件**」 | 该目录实际 **965** 个文件；207 是「`*.jsonl` + `*.json`（≤8 MB）」的**解析范围**计数（89+118=207，与 `OPENQ-W3-corpus-stats.md:8` 一致） | 补范围限定：「`~/.gemini` 下 **207 个 `*.jsonl`+`*.json`（≤8 MB）**」 |
| 4 | P2 表述过宽 | L615（§4.1） | 「早退分支会让同一函数出现**第二种键集合**…`parseCopilotIncremental:17944,17963`、`parseTraeIncremental:22163`」 | 只有 `parseTraeCnApiIncremental:22634` 换了键集合；后两处早退 return 与主 return **键集合相同**（仅值为 0），不会让调用方解构出 `undefined` | 保留第一例，后两例改述为「另有早退返回同键全零值（不会缺键）」 |
| 5 | P2 表述过宽 | L316（§3.2） | 「全仓引用它的只有 `status.js`」 | `test/passive-mode.test.js:24`、`test/workbuddy-ai.test.js:17` 亦 import | 改「`src/` 内引用它的只有 `status.js`」 |
| 6 | P2 表述过窄 | L309（§3.2） | 「标记检测只出现在卸载路径」 | `hasPluginMarker` 亦被 `isOpencodePluginInstalled`（`opencode-config.js:104`）调用 | 改「标记检测只在卸载与探测路径出现，写入路径无该分支」 |
| 7 | P2 引用精度 | L388（§3.3 去重表①…③行） | 「才标 `duplicateOf` 并把 offset 直接推到 `st.size`（`:1073-1081`）」 | `duplicateOf: primary` 在 `:1084`，落在区间外 3 行 | 区间放宽为 `:1073-1084` |
| 8 | P2 完整性 | §1（L21–L49） | 时间线列 R1/R2/本文/R3/R4/OPEN-Q(R6) | 缺 **R5**（§13.8 有完整记载，§13 标题为「R1–R6」）；另 R2 的 I=24／U=14 在 §13.2 无对应合计行（E=15 可由三份 R2 报告相加复现） | §1 补一句 R5；或在 §13.2 补 R2 合计行 |

**无机制性错误、无幻觉、无「结论与源码方向相反」的条目**；范围内 177 个显式引用的**文件存在性与行号边界全部通过**（0 越界）。

## 复核证据抽样（证明真的打开过）

以下为本轮**亲自读取并摘录**的关键证据（行号均为 UTF-8 口径）：

1. **GBK 漂移实测（PowerShell 5.1）**
   `powershell.exe -NoProfile -Command "(Get-Content $p).Count; (Get-Content $p | Measure-Object -Line).Lines; (Get-Content $p -Encoding UTF8).Count; $PSVersionTable.PSVersion.ToString(); [System.Text.Encoding]::Default.WebName"`
   → `24917` / `23260` / `24932` / `5.1.19041.6456` / `gb2312`（文档 §1.3 表格三个数字 **逐位命中**）
   全 `src/` 逐文件比对 → `TOTAL_AFFECTED_LINES=52`、`FILES_WITH_DRIFT=20`，最大 15（`rollout.js`）

2. **`src/lib/fs.js:69-74`（锁常量，逐字）**
   ```js
   const LOCK_STALE_MS = 5 * 60 * 1000; // 5 minutes
   const LOCK_HEARTBEAT_MS = 30 * 1000;
   const MAX_RECLAIM_DEPTH = 4;
   const MAX_RECLAIM_SWEEP_DEPTH = 64;
   const TRANSITION_GUARD_RETRY_MS = 5;
   const TRANSITION_GUARD_ATTEMPTS = 1000;
   ```
   `:88-91` `heartbeatPathFor` = `sha256(token)` → `${lockPath}.heartbeat.${tokenDigest}`
   `:93-103` `isProcessAlive`：`if (e?.code === "ESRCH") return false;` + 注释 `EPERM means the process exists but belongs to another user` + `return true;`
   `:296` `await fs.open(lockPath, "wx")`；`:305-306` `heartbeatHandle = await fs.open(heartbeatPath, "wx"); await heartbeatHandle.writeFile(\`${token}\n\`, "utf8");`
   `:368` `if (!(await existingLockCanBeReclaimed(lockPath))) return null;`；`:370` `.stale.${process.pid}.${crypto.randomUUID()}`
   `:200-201` `for (let index = chain.length - 1; index >= 0; index -= 1)`（清扫从深到浅）

3. **`src/commands/sync.js:295-300`（锁等待常量，逐字）** —— `130_000` / `5_000` / `90_000` / `250`，含 120s 预算注释
   `:438` `const waitsForPriority = Boolean(opts.waitForLock || opts.drain || opts.publishAccount);`
   `:461` `if (!opts.fromNotify || notifyWaitMs <= 0) return null;`；`:466` `${lockPath}.notify-wait`
   `:3424-3437` `skipNoOpCursorCommit`（逐项点数 = 13）；`:3444-3446` commit；`:3456-3459` deferred audit

4. **`src/lib/cursor-store.js:855-1016` 九步抽样**
   `:856` `const generationId = newGenerationId();`；`:877-881` `cloneGenerationFiles`；`:1357-1361` `await fs.copyFile(source, target, fssync.constants.COPYFILE_FICLONE);`（**reflink，非硬链接**）
   `:899-921` 只重写脏分片 + 空分片 `delete`；`:923-945` 事件集；`:965-970` `generation.json`；`:972` `maybeInjectFailure(…, "beforeManifestSwap")`
   `:974-986` 与文档代码块**逐字相同**（含 `previous: compatibleGeneration` 注释）
   `:987` manifest 最后替换；`:1005-1009` 三代保留；`:1010-1015` 未 publish 删半成品

5. **`src/lib/rollout.js` 关键逐字点**
   `:2291` / `:2682` / `:2793` 三处 `const endOffset = st.size;`（正则全文件**恰 3 命中**）
   `:2889` `return { endOffset, eventsAggregated };`
   `:2554` `delta.conversation_count = 1;`；`:2865` `delta.conversation_count = 0;`
   `:3329-3344` 主队列行字段（与 §4.5 代码块逐字相同）；`:3346` `bucket.queuedKey = key;`
   `:3483` `await fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8");`
   `:3516-3529` 项目行（无 `model` / 无 `usage_precision` / 无 `priority_*`）；`:3536` project 队列 append
   `:3542-3558` `pickDominantModel`（并列取字典序小）；`:3574` `if (source !== DEFAULT_SOURCE) continue;`
   `:4054-4064` `initTotals` 9 字段；`:4073-4079` 5 组 `priority_*`
   `:4156-4170` `totalsKey` 九字段 + 条件 `|p:`
   `:4173-4190` `toUtcHalfHourStart`；`:4222-4231` `bucketKey`/`projectBucketKey`

6. **§4.1 独立重建（脚本 `verify41c.mjs`，非照抄文档）**
   ```
   parsers: 45   top-level funcs: 572
   薄壳数: 5    自持 return 数: 40
   不同 main key 集合数 === 11
   [19] {recordsProcessed,eventsAggregated,bucketsQueued}
   [6]  {filesProcessed,eventsAggregated,bucketsQueued,projectBucketsQueued}
   [5]  {recordsProcessed,eventsAggregated,bucketsQueued,projectBucketsQueued}
   [3]  {messagesProcessed,eventsAggregated,bucketsQueued,projectBucketsQueued}
   + 7 个单例（Copilot 4 键含 usageClaims / CopilotAppDb +dbErrors / TraeCnApi +skippedRows /
     Dsh +deferredMigrations / Trae 7 键 / CommandCode 7 键 / CopilotSessionStore 9 键）
   公共键: eventsAggregated,bucketsQueued
   计数键分布: recordsProcessed 31 / filesProcessed 6 / messagesProcessed 3
   ```
   → 与文档 §4.1 的 45 / 5 / 40 / 11 / 19 / 6 / 5 / 3 / 7 / 31 / 6 / 3 **逐项一致**

7. **全仓 grep 复核（三处「唯一性」断言）**
   - `function toUtcHalfHourStart` → `src/` 内恰 **2** 命中（`rollout.js:4173`、`claude-categorizer.js:1175`）✅
   - `bad_lines|badLines|bad_line` → `src/` 内 **No matches found** ✅
   - `unmanaged-extension-present` → 产生点恰 **1** 处（`omp-hook.js:391`），消费点 `init.js:813`，测试 3 处断言 ✅
   - `appendFile(Sync)?\(` → 主队列 append **恰 6 处**，与 §4.6 括注一致 ✅
   - `union: true` → `sync.js` 内 **恰 2 处**（`:786`、`:819`），与文档「实测只用于 codex 与 acode」一致 ✅
   - `passive-mode` → `src/` 内仅 `status.js:44`，但 `test/` 另有两处 ⚠️（必须修 #5）

8. **本机语料计数（只计数、不含内容）**
   ```powershell
   # 仅统计文件数与行数
   claude jsonl files=22   claude total lines=4609     ← 与 §1.4 逐位吻合
   codex rollout files=64  codex total lines=179776   ← 与 §1.4 逐位吻合
   ~/.gemini: 全目录 965 文件；*.jsonl 89 + *.json 118 = 207（全部 ≤8MB）
   Test-Path ~/.tokentracker → False
   ```

9. **样本仓库完整性**：复核前后 `git -C tt-research status --porcelain` 均为**空**；`git rev-parse HEAD` = `b19d27b0ccd59ce621f4fa17207b06358e67987c`（未做任何 git 写操作）；主文档未改动。
