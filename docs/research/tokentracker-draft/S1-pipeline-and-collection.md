# S1 · 采集管线与调度全链路

> 本文件是 TokenScope 团队对 **TokenTracker（npm 包名 `tokentracker-cli`）** 的只读源码研究成果，用于对照自家采集管线的设计取舍。以下所有结论均来自对样本仓库源码的实际阅读；凡不能从源码直接确认的，一律标注 `【未能确认】`。

## 0. 样本、范围与方法

| 项 | 值 |
| --- | --- |
| 仓库 | `https://github.com/xiufengsun/TokenTracker` |
| 本地样本路径 | `C:\Users\admin\AppData\Local\Temp\tt-research` |
| HEAD | `b19d27b0ccd59ce621f4fa17207b06358e67987c` |
| 版本 | v1.3.14（`package.json`） |
| npm 包名 | `tokentracker-cli` |
| 形态 | Node.js CLI（CommonJS，`src/`）+ React Dashboard（`dashboard/`）+ macOS Swift 壳（`TokenTrackerBar/`）+ Windows .NET 壳（`TokenTrackerWin/`）+ Linux Tauri 壳（`TokenTrackerLinux/`） |
| 研究范围 | 采集与调度全链路：入口/命令面、hook 触发、`sync` 编排、游标与增量、被动/自动模式、多安装多根发现、读取安全 |
| 明确不含 | 逐 provider 解析细节、定价计算、Dashboard 前端渲染、云侧 schema |
| 方法 | 只读：`read`/`Select-String`；不修改仓库、不执行仓库脚本、不 `npm install` |

### 0.1 与共享情报的实测偏差（先纠偏，后文按实测行号）

共享情报中若干“关键文件行数/规模”与实测不符，本文一律以实测为准：

| 文件 | 共享情报 | 实测 |
| --- | --- | --- |
| `src/commands/sync.js` | 6900 行 | **7251 行** / 288,151 字节 |
| `src/lib/rollout.js` | 23260 行 | **24932 行** / 942,266 字节 |
| `src/commands/status.js` | 1647 行 | **1729 行** / 76,611 字节 |
| `src/cli.js` | 100 行 | **107 行** / 4,060 字节 |
| `src/commands/init.js` | 82654 字节 | 82,654 字节一致，实际 **2427 行** |
| `bin/tracker.js` | — | **60 行**（非 100 行） |

实测 `src/` 下共 **112 个文件**（口径：**仓库跟踪文件数**——`git ls-files src` = 112，`Get-ChildItem -Recurse -File -Force src` 同为 112，含 `src/lib/pricing/`：`index.js`、`matcher.js`、`litellm-fetcher.js`、`seed-snapshot.json`、`curated-overrides.json`）。`src/lib/` 内与采集直接相关的核心模块：`rollout.js`（942 KB，全部 provider 的 `parse*Incremental`）、`local-api.js`（156 KB，本地 HTTP API）、`session-analytics.js`（138 KB）、`usage-limits.js`（175 KB，额度，非采集）、`cursor-store.js`（62 KB，游标持久化）、`fs.js`（15 KB，锁与原子写）、`upload-queue.js`、`async-queue-reader.js`、`jsonl-lines.js`、`scan-roots.js`、`wsl-probe.js`、`install-resolver.js`、`multi-install-parser.js`。

### 0.2 一句话数据流（源码级）

```
provider CLI 会话日志（~/.claude/projects/**, ~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl, SQLite…）
  → [触发] hook（SessionEnd / Stop / notify 回调 / plugin）→ notify.cjs → 脱离父进程 spawn `tracker.js sync --auto --from-notify --source=<src>`
     或者 [兜底] serve.js 的 Windows 侧 60s 后台轮询 / 用户手动 `tracker sync`
  → [发现] sync.js 解析 scanRoots + WSL 双安装 → listRolloutFiles / listClaudeProjectFiles / …
  → [增量] cursor-store-v2 里按文件 inode+offset 续读，事件/消息哈希去重
  → [桶] 半小时 UTC 桶累加进 cursors.hourly.buckets / cursors.projectHourly.buckets
  → [写队列] enqueueTouchedBuckets → fs.appendFile(queue.jsonl)（同源 project.queue.jsonl）
  → [游标提交] cursorStore.commit()（generations 目录整体轮换 + manifest 原子切换）
  → [上传] drainQueueToCloud → POST /functions/tokentracker-ingest，queue.state.json 记录字节 offset
  → [读] local-api.js 用 createAsyncQueueReader 增量读 queue.jsonl → Dashboard
```

数据目录（`src/lib/tracker-paths.js:4-12`）：

```js
async function resolveTrackerPaths({ home = os.homedir() } = {}) {
  const rootDir = path.join(home, ".tokentracker");
  return {
    rootDir,
    trackerDir: path.join(rootDir, "tracker"),
    binDir: path.join(rootDir, "bin"),
    cacheDir: path.join(rootDir, "cache"),
  };
}
```

注意：**队列、游标、锁都在 `~/.tokentracker/tracker/` 下**，不是 `~/.tokentracker/` 根目录（`src/commands/sync.js:568-573` 显式拼接 `trackerDir`）。`bin/`（notify.cjs 等）与 `cache/`（定价快照）是兄弟目录。

---

## 1. 入口与命令面

### 1.1 `bin/tracker.js`：进程级前置，然后交给 `run()`

`bin/tracker.js` 只有 60 行，职责**不是**命令分发，而是三件进程环境准备：

1. 剥离 `--debug` 旗标并落成环境变量（`bin/tracker.js:5,12-13`）：

```js
const { stripDebugFlag } = require('../src/lib/debug-flags');
const { argv, debug } = stripDebugFlag(process.argv.slice(2));
if (debug) process.env.TOKENTRACKER_DEBUG = '1';
```

2. **代理自举与“代理失效则中止”**（`bin/tracker.js:15-40`）：先读持久化代理配置，需要时用新环境变量重启自身；随后给旧 Node 装 undici `ProxyAgent`。如果手动代理无法生效**且**无法阻断出站流量，直接 `exit(1)`：

```js
const applyResult = applyUndiciProxyIfNeeded({ proxyConfig: persistedProxy });
if (applyResult && applyResult.unprotected === true) {
  console.error('[proxy] Manual proxy could not be applied and outbound traffic could not be blocked; aborting.');
  console.error('[proxy] 手动代理无法生效且无法阻断出站流量，已中止。');
  process.exit(1);
}
```

这一条对采集语义有直接影响：**代理配置错误会让整个 CLI（含 hook 触发的 sync）硬失败**，而不是静默降级。

3. 统一错误出口（`bin/tracker.js:42-59`）：`run(argv).catch(...)`，非 debug 时只打印 stack，debug 时额外打印 `status`/`code`/`originalMessage`/`nextActions`，最后 `process.exitCode = 1`。

### 1.2 `src/cli.js`：10 个子命令的静态 switch

分发是**无语法的静态 switch**，无 commander/yargs（`src/cli.js:12-66`）：

```js
async function run(argv) {
  const [command, ...rest] = argv;
  // No args → launch dashboard
  if (!command) { await cmdServe(argv); return; }
  if (command === "-h" || command === "--help") { printHelp(); return; }
  if (command === "-v" || command === "--version") {
    const pkg = require("../package.json");
    console.log(`v${pkg.version}`);
    return;
  }
  switch (command) {
    case "serve": await cmdServe(rest); return;
    // init / sync / status / diagnostics / doctor / uninstall / device-login / wrapped / sessions …
    default: throw new Error(`Unknown command: ${command}`);
  }
}
```

要点：

- **无参即 `serve`**（直接拉起本地 Dashboard），这是产品默认路径。
- `parseArgs` 在**各子命令内部**实现，未知选项抛 `Unknown option: xxx`（`src/commands/sync.js:3742`）。
- `--debug` 不在子命令解析内，而是被入口剥离（所以 `--debug` 可以放在子命令前/后任意位置，见 `src/cli.js:77-86` 帮助文本写法）。

### 1.3 全部子命令 → 实现文件对照表

| 子命令 | 实现文件 | 入口符号 | 采集管线相关性 |
| --- | --- | --- | --- |
| （无参） | `src/commands/serve.js` | `cmdServe` (`src/commands/serve.js:69`) | 高：首次启动自动 `init --yes`（`:80-90`），可 `--no-sync` 跳过启动同步（`:106-115`），并启动 60s 后台轮询（`:257-260`） |
| `serve` | 同上 | 同上 | 高 |
| `init` | `src/commands/init.js` | `cmdInit` | 高：写 hook、写 notify.cjs、写 runtime 拷贝 |
| `sync` | `src/commands/sync.js` | `cmdSync` (`src/commands/sync.js:516`) | 核心 |
| `status` | `src/commands/status.js` | `cmdStatus` | 高：hook 状态、passive 模式、游标摘要、pending bytes |
| `diagnostics` | `src/commands/diagnostics.js`（1062 B）+ `src/lib/diagnostics.js`（16962 B） | `cmdDiagnostics` | 中：导出诊断 JSON |
| `doctor` | `src/commands/doctor.js`（2983 B）+ `src/lib/doctor.js`（10830 B） | `cmdDoctor` | 中：连通性/环境自检 |
| `uninstall` | `src/commands/uninstall.js`（14087 B） | `cmdUninstall` | 中：摘 hook、`--purge` 清数据 |
| `device-login` | `src/commands/device-login.js`（9360 B） | `cmdDeviceLogin` | 低：设备配对（15 分钟码，`src/cli.js:100`） |
| `wrapped` | `src/commands/wrapped.js`（4913 B） | `cmdWrapped` | 低：年度汇总视图 |
| `sessions` | `src/commands/sessions.js`（1088 B） | `cmdSessions` | 低：会话效率分析导出（元数据 only，`src/cli.js:101`） |

### 1.4 `--help` 中暴露的隐藏契约

`src/cli.js:68-105` 的 help 文本本身就是一份行为说明书，其中与采集直接相关的三行值得单列：

```
"  - Every Code notify installs when ~/.code/config.toml exists.",
"  - auto sync waits for a device token.",
"  - sync parses ~/.codex/sessions/**/rollout-*.jsonl and ~/.code/sessions/**/rollout-*.jsonl, then uploads token deltas.",
```

即：**目录存在性即安装判定**、**自动同步受 device token 约束**、**Codex 系解析基于 `rollout-*.jsonl` 命名约定**。

---

## 2. 触发方式：hook 安装全链路

### 2.1 三层结构：命令构造 → hook 文件 → notify.cjs 处理器

TokenTracker 的触发设计是**三层解耦**：

| 层 | 模块 | 职责 |
| --- | --- | --- |
| ① 命令字符串构造 | `src/lib/notify-command.js`（38 行） | 按平台生成 `<node> <notify.cjs> --source=<name>` 的**转义后**命令串 |
| ② 各 provider 的 hook 文件读写 | `src/lib/claude-config.js` / `gemini-config.js` / `codex-config.js` / `grok-hook.js` / `omp-hook.js` / `openclaw-hook.js` / `opencode-config.js` | 把①的命令串幂等地写进各自配置文件，并支持移除/探测 |
| ③ 运行时处理器 | `~/.tokentracker/bin/notify.cjs`（由 `init.js` 生成，非仓库文件） | 落 signal、节流、**脱离进程** spawn sync、链式调用用户原有 notify |

### 2.2 ① 命令构造：平台差异集中在 8 行里

`src/lib/notify-command.js:13-19`：

```js
function buildNotifyCommand(notifyPath, source, { platform = process.platform, execPath = process.execPath } = {}) {
  if (!/^[a-z0-9-]+$/i.test(source)) throw new Error("Invalid notify source");
  if (platform === "win32") {
    return `& ${powershellLiteral(execPath)} ${powershellLiteral(notifyPath)} --source=${source}`;
  }
  return `/usr/bin/env node ${quotePosix(notifyPath)} --source=${source}`;
}
```

- Windows 用 PowerShell 调用运算符 `& '...' '...'`（单引号字面量，`powershellLiteral` 把 `'` 转义为 `''`，`src/lib/notify-command.js:3-5`），因此 hook 项还要附加 `"shell": "powershell"`（见 `src/lib/claude-config.js:176-179` 的 `hookShell`）。
- POSIX 用 `/usr/bin/env node`，路径走 `quotePosix`（转义 `\`、`"`、`$`、`` ` ``，`src/lib/notify-command.js:7-11`）。
- `source` 参数被白名单正则为 `[a-z0-9-]+`，这是**跨 provider 复用同一 notify.cjs 的唯一区分手段**。

### 2.3 ③ notify.cjs 的真实行为（`init.js` 内嵌模板）

notify.cjs 不是仓库里的静态文件，而是 `init.js` 用模板字符串生成的（**模板字符串区间 `src/commands/init.js:1453-2231`**，宿主函数 `buildNotifyHandler` 为 `:1445-2232`；模板内部自成一体，还含 `resolveMaybeHome`（`:1575`）、`isSelfNotify`（`:1583`）/`isSelfNotifyPath`（`:1597`）、`shouldChainNotify`（`:1609`）、`splitEnvString`（`:1748`）、`findNodeLikeScriptIndex`（`:1811`）等**模板内**辅助函数。`writeNotifyHandler` 在 `:425-435` 落盘并 `chmod 755`）。其行为可逐条列出：

1. **解析 `--source`**，其余参数保留为 `payloadArgs` 透传给用户原 hook（`src/commands/init.js:1461-1476`）。
2. **落 signal 文件**：`fs.writeFileSync(signalPath, new Date().toISOString())`，路径 `~/.tokentracker/tracker/notify.signal`（`:1448,1496-1498`）。
3. **20 秒节流**：读 `~/.tokentracker/tracker/sync.throttle`，若距上次 > 20s 才 spawn（`:1519-1536`）：

```js
const throttlePath = path.join(trackerDir, 'sync.throttle');
const now = Date.now();
let last = 0;
try { last = Number(fs.readFileSync(throttlePath, 'utf8')) || 0; } catch (_) {}
if (now - last > 20_000) {
  try { fs.writeFileSync(throttlePath, String(now), 'utf8'); } catch (_) {}
  const hasLocalRuntime = fs.existsSync(trackerBinPath);
  const hasLocalDeps = fs.existsSync(depsMarkerPath);
  const syncArgs = ['sync', '--auto', '--from-notify', '--source', source];
  if (hasLocalRuntime && hasLocalDeps) {
    spawnDetached([process.execPath, trackerBinPath, ...syncArgs]);
  } else {
    spawnDetached(['npx', '--yes', fallbackPkg, ...syncArgs]);
  }
}
```

   所谓“双路径”：模板里 `trackerBinPath`（模板内 `:1483`）与 `depsMarkerPath`（模板内 `:1484`）**字面完全相同**（都是 `path.join(trackerDir,'app','bin','tracker.js')`），因此 `hasLocalRuntime && hasLocalDeps` 实际退化为**一次 `existsSync`**——判据本身冗余，效果是“本地 runtime 拷贝存在 → 直接跑 `~/.tokentracker/app/bin/tracker.js`；否则退回 `npx --yes tokentracker-cli sync …`”。同一 quirk 也出现在 OpenClaw 插件模板（`src/lib/openclaw-session-plugin.js:358` 与 `:361`）。移植时保留“本地优先、npx 兜底”的语义即可，别照抄这层冗余判断。`spawnDetached` 用 `detached: true` + `unref()`（`:1563-1573`），保证**永不阻塞 provider**（注释明写 “It must never block Codex”，`:1447`）。
4. **链式调用用户原 notify**：从 `codex_notify_original.json` / `acode_notify_original.json` / `code_notify_original.json` 读回被覆盖前的原始命令并 spawn（`:1538-1559`）。Claude/Gemini/OpenCode/CodeBuddy/WorkBuddy 分支显式不链式（`originalPath = null`，`:1540-1547`）——因为这些 provider 的 hook 不共享 `notify` 这一独占槽位。
5. **始终 `process.exit(0)`**（`:1561`）。
6. 可选调试：`TOKENTRACKER_NOTIFY_DEBUG=1` 时向 `notify.debug.jsonl` 追加 `{ts, source, cwd}`，默认上限 1 MB（`:1488-1517`）。

### 2.4 各 provider 的 hook 落点（含命令原文）

下表为**实测配置路径 + 写入内容**。命令原文按 `src/lib/notify-command.js` 的构造规则给出（`<NODE>` = `process.execPath`，`<NOTIFY>` = `~/.tokentracker/bin/notify.cjs`）。

| Provider | 配置文件 | 事件 | 写入的命令原文 | 源码位置 |
| --- | --- | --- | --- | --- |
| Claude Code | `~/.claude/settings.json` | `Stop` + `SessionEnd`（**两个都装**） | Windows: `& '<NODE>' '<NOTIFY>' --source=claude`；POSIX: `/usr/bin/env node <NOTIFY> --source=claude` | 安装 `src/commands/init.js:724-728`（`upsertClaudeUsageHooks`）；事件常量 `src/lib/claude-config.js:7-16`；构造 `:116-118` |
| CodeBuddy（腾讯，Claude 分支） | `~/.codebuddy/settings.json` | **仅 `SessionEnd`** | `… --source=codebuddy` | 安装 `src/commands/init.js:1039-1043`；构造 `:627-628`、`src/lib/claude-config.js:10-12` |
| WorkBuddy | `~/.workbuddy/settings.json` | **仅 `SessionEnd`** | `… --source=workbuddy` | 安装 `src/commands/init.js:1052-1056`；构造 `:631-632` |
| WorkBuddy AI | `~/.workbuddy-ai/settings.json` | **仅 `SessionEnd`** | `… --source=workbuddy-ai` | 安装 `src/commands/init.js:1064-1068`；构造 `:637-638` |
| Gemini CLI | `~/.gemini/settings.json` | `SessionEnd`（带 `matcher`） | `… --source=gemini` | 安装 `src/commands/init.js:735-739`；事件/matcher `src/lib/gemini-config.js:8-10`；构造 `:103-106` |
| Codex CLI | `~/.codex/config.toml` 的**顶层 `notify` 数组** | turn 结束（notify 回调） | `notify = ["/usr/bin/env", "node", "<NOTIFY>"]`；Windows: `notify = ["<NODE>", "<NOTIFY>"]` | 安装 `src/commands/init.js:689-694`；构造 `src/lib/codex-config.js:9-17`；写入 `setNotify` `:271-309` |
| acode（AStudio） | `~/.acode/config.toml`（受 `TOKENTRACKER_ACODE_HOME` 覆盖） | 同 Codex notify | `notify = [..., "<NOTIFY>", "--source=acode"]` | 安装 `src/commands/init.js:707-712`；构造 `src/lib/codex-config.js:22-24` |
| Every Code | `~/.code/config.toml`（受 `CODE_HOME` 覆盖） | 同 Codex notify | `notify = [..., "<NOTIFY>", "--source=every-code"]` | 安装 `src/commands/init.js:1133-1138`；构造 `src/lib/codex-config.js:18-20` |
| Grok Build | `~/.grok/hooks/99-tokentracker-usage.json` | `SessionEnd` | 独立 handler：`<NODE> <TRACKER_BIN>/grok-session-end-hook.cjs`（**不经过 notify.cjs**） | 安装 `src/commands/init.js:906`、`src/lib/grok-hook.js:307-317`；文件/handler `:68-99`；hook JSON `:34-58` |
| oh-my-pi / OMP | `<ompAgentDir>/extensions/tokentracker-notify.ts` | `turn_end` + `agent_end` + `session_shutdown` | 扩展内 `spawn(process.execPath, [notifyPath, "--source=omp"])`，自限速 12s | 安装 `src/commands/init.js:806`；`src/lib/omp-hook.js:16-17,52,66-67,86,117,135-143` |
| OpenClaw（session plugin 路径） | 插件目录 `~/.tokentracker/tracker/openclaw-plugin/openclaw-session-sync/`，注册进 `~/.openclaw/openclaw.json` | `agent_end` + `gateway_start` + `gateway_stop` | 插件内 `[process.execPath, trackerBinPath, 'sync', '--auto', '--from-openclaw']` | 安装 `src/commands/init.js:1078-1083`；`src/lib/openclaw-session-plugin.js:37-84,52-57,200-224,363-399` |
| OpenClaw（legacy hook，仅移除） | `~/.tokentracker/tracker/openclaw-hook/tokentracker-openclaw-sync/` | 旧 CLI hook 注册 | 已被 session plugin 取代，init 只做 `probe` + `remove` | `src/commands/init.js:1120-1128`；`src/lib/openclaw-hook.js:7-31,33-53` |
| OpenCode | `<configDir>/plugin/tokentracker.js`（`$OPENCODE_CONFIG_DIR` / `$XDG_CONFIG_HOME/opencode` / `~/.config/opencode`） | `session.updated` | 插件内 shell 调用：`node <NOTIFY> --source=opencode` | 安装 `src/commands/init.js:744-747`；`src/lib/opencode-config.js:7-9,11-22,24-66,36-40` |
| Cursor | **不写任何配置** | 无 hook（走 API 拉取） | — | 探测 `src/commands/init.js:759-776`；注释 “Cursor (API-based, no hooks needed)” `:758`；会话 token 读取 `src/lib/cursor-config.js:10-33,79-104` |

补充事实：

- init 的 provider 总名单是 `SUPPORTED_PROVIDERS`（**45 项**，`src/commands/init.js:119-165`）；按 `applyIntegrationSetup`（`src/commands/init.js:674-1149`）的处理方式拆解为 **45 = 写配置 12 + 仅探测 1 + 纯被动读取 17 + 仅出现在欢迎名单 15**（原表述把 OpenClaw 的 legacy hook 移除单列为第 5 类，得 46 ≠ 45——该移除分支是**同一个 provider**（OpenClaw）的附加清理动作，不构成独立 provider）。写配置的 12 个：Claude Code(`:722-731`)、Codex CLI(`:686-702`)、AStudio(`:704-720`)、Every Code(`:1130-1146`)、Gemini CLI(`:733-742`)、CodeBuddy(`:1037-1046`)、WorkBuddy(`:1050-1059`)、WorkBuddy AI(`:1062-1071`)、Grok Build(`:901-916`)、oh-my-pi(`:789-828`)、OpenCode(`:744-756`)、OpenClaw session plugin(`:1073-1118`)；仅探测 1 个 = **Cursor**(`:758-776`，只报 detected/skipped，不写任何配置)；legacy hook 移除见 `:1120-1128`。纯被动读取（init 只打印 “detected/reader”，不装 hook）的 17 个：Kimi Code(`:785`)、pi(`:838`)、Prime Agent(`:846`)、MiniMax Code(`:854`)、Command Code(`:863`)、Craft Agents(`:873`)、Reasonix(`:881`)、TRAE(`:891-897`)、Kilo CLI(`:926`)、DuMate(`:933`)、Mimo(`:943`)、LM Studio(`:951-955`)、Unsloth Studio(`:962-966`)、AnythingLLM Desktop(`:974-978`)、Devin CLI(`:986-990`)、Kilo Code VS Code 扩展(`:1002-1006`)、Cline(`:1019-1030`)。其余 15 个在 `applyIntegrationSetup` 内**没有任何分支**（既不安装也不探测），仅为欢迎名单而存在：Antigravity、Kiro、Hermes Agent、GitHub Copilot、OmO、Dots、Roo Code、Zed Agent、Goose、Droid、ZCode、Qoder、Claude Science、DeepSeek Harness、TRAE Work CN。12 + 1 + 17 + 15 = 45，与 `SUPPORTED_PROVIDERS` 项数闭合。
- 集群安装的调用点是 `applyIntegrationSetup`（`src/commands/init.js:674-1149`）；`serve` 自愈走的是同一批 upsert 的 `repairRuntimeIntegrations`（`src/commands/init.js:525-602`，逐项 try/catch 收集 warnings，`:546-553`）。

关键差异总结（直接决定“新鲜度”语义）：

- **Codex 系（codex / acode / every-code）走 `notify` 回调**，是逐 turn 触发；且这条路径**独占用户的 `notify` 槽位**，所以必须把用户原 notify 备份到 `*_notify_original.json` 并在 notify.cjs 里链式调用（`src/commands/init.js:1538-1559`）。
- **Claude 系（claude / codebuddy / workbuddy / workbuddy-ai）走 `hooks.Stop` + `hooks.SessionEnd`**，即“每轮结束 + 会话结束”双触发，不与用户其它配置争槽位。
- **Gemini 走 `SessionEnd` + matcher**，默认 matcher 是 `exit|clear|logout|prompt_input_exit|other`（`src/lib/gemini-config.js:10`）——意味着**只有特定退出原因**才触发。
- **Grok 用自己的 hook 文件 + 专用 handler**（`grok-session-end-hook.cjs`），不写 notify.cjs 契约（`src/lib/grok-hook.js:83-98`）。
- **OMP/OpenCode/OpenClaw 走插件/extension**，且 OpenCode 的注释明确说明“opencode 行本来就能从磁盘 storage 与 opencode.db 被动解析，hook 只是让 sync 更早发生”（`src/lib/opencode-config.js:55-56`）。

### 2.5 幂等、修复与备份

各 config 模块共享同一套幂等策略，值得照抄的细节：

1. **身份识别而非字符串相等**：`notifyIdentity()` 会把命令解析成 token（去掉 `&`、`/usr/bin/env node` 前缀），要求恰好 3 段且第 2 段以 `notify.cjs` 结尾、第 3 段形如 `--source=<name>`，最后返回 `file\u0000source` 作为身份（`src/lib/notify-command.js:23-36`）。这让“路径变了（npm 全局 → 内嵌 runtime）但仍是我们的 hook”能被识别（`src/lib/codex-config.js:40-57`）。
2. **就地修复而非重复追加**：`normalizeEntriesForCommand` 会把已存在但形态过时的条目改写成 `{type:"command", command, shell?}`，并**在同一 event/scope 上删除重复项**（`src/lib/claude-config.js:181-230`）。
3. **写入前备份**：`writeClaudeSettings` / `writeGeminiSettings` 复制出 `settings.json.bak.<ISO时间>`（`src/lib/claude-config.js:248-262`、`src/lib/gemini-config.js:265-279`）。
4. **`config.toml` 是手工 TOML 行编辑**：`extractNotify` 只找**首个表格头之前**的 `notify =` 行（`src/lib/codex-config.js:245-269`），`setNotify` 在原位置替换（找不到则插到第一个 `[table]` 前）（`:271-309`）。这是有意为之的最小侵入式 TOML 编辑，不引入 TOML 库。
5. **Gemini 额外开关**：安装 hook 时会强制 `tools.enableHooks = true`（`src/lib/gemini-config.js:147-152`）——即 **TokenTracker 会改动用户的工具开关**。
6. **安全模式修复**：`repairRuntimeIntegrations({safeMode:true})` 在探测到第三方 notify 时按策略跳过覆盖（`src/commands/init.js:437-479`、`shouldRepairCodexNotify` 判定见 `:460-469`），并把“原 notify”记入 `*_notify_original.json` 供卸载还原（`src/lib/codex-config.js:60-134`）。

### 2.6 安装失败的降级：passive mode

`src/lib/passive-mode.js` 定义了“hook 装不上但日志在”这一降级态。判定条件三合一（`src/lib/passive-mode.js:189-204`）：

```js
function buildEntry({ name, hookExpected, hookInstalled, logsDir, logsPredicate, settingsPath }) {
  const logsPresent = dirHasFile(logsDir, logsPredicate);
  const passive = hookExpected && !hookInstalled && logsPresent;
  let reason = null;
  if (hookExpected && !hookInstalled) {
    reason = classifyWritableFailure(settingsPath);
  }
  return { name, hook_expected: hookExpected, hook_installed: hookInstalled,
           logs_present: logsPresent, passive, hook_failure_reason: reason };
}
```

- `dirHasFile` 只做**至多一层**浅递归（覆盖 `projects/<name>/*.jsonl`），且把 readdir 失败当作 false 吞掉（`src/lib/passive-mode.js:49-68`）。
- 失败原因分类只区分 `ENOENT` / `EACCES` / `EROFS` 与其它（`src/lib/passive-mode.js:70-81`）——对应的正是注释里点名的 WSL 只读挂载、UNC、锁定的 settings、沙箱进程四类环境（`src/lib/passive-mode.js:9-15`）。
- 覆盖的 provider 名单：claude、gemini、codex、acode、every_code、codebuddy、workbuddy、workbuddy-ai（`src/lib/passive-mode.js:96-185`）。注意 **Grok / OpenCode / OpenClaw 不在 passive 名单内**（它们走插件路径，状态由 `status.js` 单独上报）。
- 语义边界（模块头注释明示）：**hook 缺失永不导致 sync 失败**，只是把“秒级新鲜”降级为“分钟级新鲜”（`src/lib/passive-mode.js:22-25`）。

**关键澄清（复核后确认）**：`src/lib/passive-mode.js` **与 `init.js` 之间没有任何代码调用关系**。全仓引用它的只有 `src/commands/status.js`（`status.js:44` require，`:1021` 调用，`:1248` 输出 `passive_mode` 字段，`:1448`、`:1688` 呈现）。也就是说：**init 不会因为 hook 装不上而回退到别的方式，passive 只是 status 层的补偿性检测与告知**。init 侧的对应事实是：

- init.js 内**没有**针对 WSL 只读挂载 / UNC / EROFS 的专门分支（对 `EROFS|read-only|UNC|WSL|/mnt/|drvfs` 在 init.js 内零命中）。
- init.js 内唯一与权限相关的落点是 `probeFile`：`EACCES`/`EPERM` 被降级为 `reason: "permission-denied"` 而不抛错（`src/commands/init.js:2244-2255`），再由 `renderSkipDetail` 渲染（`:1327-1332`）。
- `applyIntegrationSetup` 中**唯一自带 try/catch 的 provider 是 Grok**（失败推 `status:"error"`，`src/commands/init.js:902-916`）；其余 provider 的失败会向上冒泡到 `cmdInit` 的 try/catch，做一次 `spinner.stop()` 后 rethrow（`src/commands/init.js:226-244`）。
- `serve` 对这两类失败都只打印警告后继续（`src/commands/serve.js:84-89`、`:92-103`）。

### 2.7 init 的开关、跳过与回滚

| 开关 | 语义 | 源码位置 |
| --- | --- | --- |
| `--dry-run` | 解析后打印干跑横幅，走 `buildDryRunSummary` → `previewIntegrations` 后**直接 return，不写任何配置**；`applyIntegrationSetup` 内 dryRun 仅对 oh-my-pi 生效（改 probe 不写） | 解析 `src/commands/init.js:1427`；横幅 `:191-193`；提前返回 `:210-221`；dryRun 分叉 `:679,796-804` |
| `--yes` | 跳过交互确认（条件 `isInteractive() && !opts.yes && !opts.dryRun`） | `src/commands/init.js:1426,195-208` |
| `--link-code <code>` | 在本文件内的**唯一效果**是关掉浏览器认证（`shouldUseBrowserAuth → false`） | `src/commands/init.js:1423,339`；`link_code_state.json` 在 init.js 内只被声明与传参（`:176,234,359`），**无任何读写**——全仓另一处引用只是 `src/lib/runtime-config.js:149` 的清理清单 |
| `--no-auth` / `--no-open` | 关认证 / 不打开浏览器 | `src/commands/init.js:1424-1425,338` |
| `TOKENTRACKER_SKIP_LOCAL_RUNTIME_COPY=1` | 跳过把 runtime 拷进 `~/.tokentracker/app` | `src/commands/init.js:2271` |
| `TOKENTRACKER_SKIP_FIRST_SYNC=1` | 装完不跑首次 sync | `src/commands/init.js:2330` |
| `TOKENTRACKER_SKIP_OPENCLAW_CLI=1` | 跳过 openclaw CLI 调用 | `src/lib/openclaw-cli.js:58` |
| 未知参数 | 直接抛错 | `src/commands/init.js:1428` |

备份与还原：

- Claude 系：`settings.json` → `settings.json.bak.<ISO>`（`src/lib/claude-config.js:248-262`）；Gemini 同构（`src/lib/gemini-config.js:265-279`）。
- Codex / acode / every-code：`config.toml` → `config.toml.bak.<ISO>`，且**先**把原 notify 备份到 `<trackerDir>/{codex,acode,code}_notify_original.json`（`src/lib/codex-config.js:89-104`）。
- OpenCode：旧插件文件 `.bak.<ISO>`（`src/lib/opencode-config.js:79-83`）。
- **Grok 没有备份**（直接覆盖，`src/lib/grok-hook.js:85-91`）；**omp / OpenClaw 没有内容备份**，靠哨兵串与 inode 校验保护（`src/lib/omp-hook.js:17,222-281,388-395`；`src/lib/openclaw-session-plugin.js:154-178,217-222`）。
- 回滚全部集中在 `uninstall`：`restoreCodexNotify/restoreAcodeNotify/restoreEveryCodeNotify` + `removeClaudeUsageHooks/removeClaudeHook/removeGeminiHook/removeOpencodePlugin/removeOpenclawSessionPluginConfig/removeOpenclawHookConfig/removeGrokHook/removeOmpHook`（`src/commands/uninstall.js:76-137`），并删除 notify.cjs（`uninstall.js:137`）。`restoreNotify` 会用 `isManagedNotifyCmd` 把关：不是我们写的 notify 时返回 `current-not-managed` 且不还原（`src/lib/codex-config.js:110-133`）。

### 2.8 幂等实现的四种范式（对本项目最有借鉴价值的部分）

1. **Claude 范式**：`hasHook` 做结构性比对（command + `shell` 一致性，`src/lib/claude-config.js:143-156`）；已存在即**完全不写盘**（`if (!changed) return {changed:false}`，`:39`）；另有 `normalizeEntriesForCommand` 做**就地修复 + 按 matcher scope 去重**（`:181-230`）。
2. **Codex 范式**：TOML 数组严格等值比较（相等即 `changed:false`，`src/lib/codex-config.js:76,107`）；“是否我们写的”用 `isManagedNotifyCmd`——数组里存在以 `notify.cjs` 结尾且路径含 `/.tokentracker/` 的项且尾参一致（`:40-58`）。
3. **哨兵范式（全仓只有 omp 一家）**：扩展文件首写标记串 `// @tokentracker-managed-omp-extension`（`src/lib/omp-hook.js:17`）；**带标记 → 覆盖，无标记 → 拒绝写入**（`skippedReason: "unmanaged-extension-present"`，`src/lib/omp-hook.js:388-395`），这是对“用户已有自己的扩展”场景的规避策略。注意 `unmanaged-extension-present` **全仓仅此一处产生**，消费方是 `src/commands/init.js:813`。
4. **内容等值短路 + 无条件备份覆盖范式（OpenCode）**：`upsertOpencodePlugin`（`src/lib/opencode-config.js:66-87`）先做**内容等值短路**——`if (existing === next) return { changed: false }`（`:73-75`），与哨兵无关；但**内容一旦不等，无论文件里原本是什么都“先备份再覆盖”**（`:80-85`），**没有**“无标记则拒绝写入”的分支。标记检测（`hasPluginMarker`）只出现在卸载路径 `removeOpencodePlugin`：内容不匹配时返回 `skippedReason: "unexpected-content"`（`:94`）。**因此 OpenCode 不属于哨兵范式**——把它归入哨兵范式会得出“OpenCode 会拒绝覆盖外来文件”的错误印象。

**Grok 是唯一的反例**：不做任何幂等判断，每次 init 都重写 hook JSON 与 handler，`upsertGrokHook` 恒返回 `changed:true`——源码注释的理由是“bin 路径可能变化，始终覆盖以保持命令最新”（`src/lib/grok-hook.js:85-87,307-317`）。

---

## 3. `sync` 命令编排

`cmdSync` 单函数体从 `src/commands/sync.js:516` 一直延伸到 `:3709`（约 3190 行），是“一个巨型编排函数 + 尾部一堆 repair 子函数”的结构。`module.exports` 出现在 `:3821-3857`，之后（`:3859-7251`）全是 repair/migration/helper 函数。

### 3.1 主流程骨架（按执行顺序）

| # | 阶段 | 关键行为 | 行号 |
| --- | --- | --- | --- |
| 1 | 参数与上下文 | `parseArgs(argv)`；接收测试注入（`home`、`trackerDataDir`、`traeCnFetchImpl`、`cursorStoreOptions`…） | `:516-545` |
| 2 | 上传许可判定 | `requiresCloudSyncPref = auto‖background‖fromRetry‖fromNotify‖fromOpenclaw‖有 LOCAL_SYNC_ATTEMPT_ID`；`canUpload()` 只对非交互场景要求 `readCloudSyncEnabled` | `:541-545` |
| 3 | 建目录 + 加锁 | `ensureDir(trackerDir)`；`openLock(sync.lock)`；拿不到锁则**不抛错**——写 `sync.skip.json`（marker）并向 stderr 输出一行 `Sync skipped: <reason> — <detail>` 后 return（`process.stderr.write(await recordSyncSkip(trackerDir, lockPath))`，`:557`），**不是静默 return** | `:547-559` |
| 4 | 路径与状态装载 | `config.json`、`cursors.json`、`queue.jsonl`、`queue.state.json`、`project.queue.jsonl`、`project.queue.state.json`、`upload.throttle.json`、grok signal | `:567-575` |
| 5 | 历史遗留 baseUrl 迁移 | 若 `config.baseUrl` 命中已退役 InsForge 主机 → 把 `queue.state.offset` 置 0 并清 backoff（可重试、幂等重放） | `:584-634` |
| 6 | 解析扫描根 | `resolveEnvRoot("codex")` + `resolveScanRoots({home, env, config, base})` → `codexCursorRoots` 喂给游标存储 | `:645-661` |
| 7 | 打开游标存储 | `openCursorStore({trackerDir, cursorsPath, codexRoots})`；`cursors = cursorStore.cursors` | `:653-661` |
| 8 | 模式判定 | `isBackgroundLightweightSync` / `isFullSourceScan` / `sourceAllowed()` 闭包（三档：后台轻量、后台全本地、全量） | `:713-735` |
| 9 | 后台专属迁移豁免 | `backgroundCodexUsageRepair`：只在第一个合格后台轮次升级为“深扫修复” | `:742-757` |
| 10 | 组装 source 列表 | 每个 provider 解析出 `sessionsDir`（native/WSL/额外根），组装 `sources[]`；`union: true` 的 provider 两套都收 | `:759-862` |
| 11 | 文件发现 | `uniqueSources` 去重（按 `sessionsDir`）→ `listRolloutFiles`/`listRolloutFilesDeep`（并发受 `FILE_METADATA_CONCURRENCY` 限制）→ 拍平成 `rolloutFiles[{path, source}]` | `:864-896` |
| 12 | 全量扫描前的修复 | `materializeAllCodexState` → `migrateRolloutCumulativeDeltaBuckets` → `repairCodexRescanInflation` → `repairCodexForkReplayInflation` → `repairCodexInterleavedUsageInflation` → `repairDroidDuplicateSessionInflation` → `repairMimoClaudeMislabel` | `:898-934` |
| 13 | Codex 冷扫描过滤 | `filterColdCodexRolloutFiles`（后台轻量只扫近期），并读 `deferredCodexAuditSyncs` | `:935-991` |
| 14 | Codex 解析 | `parseRolloutIncremental`；捕获 `TOKENTRACKER_CURSOR_STORE_RETRY` 时**降级重试**（加载分片后重跑全量），`TOKENTRACKER_CURSOR_STORE_CORRUPT` 则直接抛 | `:1046-1099` |
| 15 | OpenClaw | plugin signal 优先 + 全量时补扫；另有 `applyOpenclawTotalsFallback` 兜底 | `:1004-1133` |
| 16 | Claude | 多根去重（`seenClaudeFiles`）→ 全量时 `reincludeClaudeMemObserverFiles` → 根可用性守卫 → `repairClaudeQueueFromGroundTruth` → `parseClaudeIncremental` | `:1135-1220` |
| 17 | Gemini/Antigravity | `resolveInstallPaths` 选 native/WSL → `parseGeminiIncremental` / `parseAntigravityIncremental` | `:1222-1330` |
| 18 | 其余 ~40 个 provider | 统一模式：sourceAllowed → 发现 → `parseXxxIncremental` / `multiInstallParse`（详见 3.2 表） | `:1337-3300` |
| 19 | 汇总计数 | 把各 provider 的 `bucketsQueued` 相加得到 `totalBuckets` | `:3380-3423` |
| 20 | 游标提交 | `skipNoOpCursorCommit` 短路（仅 auto + 非全量 + v2 + 无变更）；否则 `cursorStore.commit(cursors)` | `:3424-3460` |
| 21 | 运行时配置与上传 | `resolveRuntimeConfig` → 节流判定 `decideAutoUpload` → `compactPendingQueue` → `drainQueueToCloud` | `:3467-3652` |
| 22 | 收尾 | 计算 `pendingBytes`；必要时 `scheduleAutoRetry`；非 auto 时打印汇总；`maybeSendHeartbeat` | `:3654-3704` |
| 23 | 释放锁 | `finally { progress?.stop(); await lock.release(); }` | `:3705-3708` |

锁的获取策略（值得注意的设计）：`acquireSyncLock` 把等待行为分成三档（`src/commands/sync.js:428-482`）：

- `waitForLock` / `drain` / `publishAccount` → **优先级等待** 90s，每 250ms 重试，超时抛 `SYNC_BUSY`；
- `fromNotify` → 走一个**低频合并闸**（`${lockPath}.notify-wait`），最多等 130s（`NOTIFY_LOCK_WAIT_MS`），保证“一次 notify 不丢 Codex 最后一轮”，同时避免几百个 notify 进程空转；
- 其它（含 `--auto`）→ 立即返回 null：写 `sync.skip.json` 并向 stderr 报一行 `Sync skipped: …`（`:557`），调用方随即 return，不抛错。

```js
// src/commands/sync.js:295-300
const NOTIFY_LOCK_WAIT_MS = 130_000;
const NOTIFY_LOCK_POLL_MS = 5_000;
// Local API sync requests have a 120-second child-process budget. Reserve at
// least 30 seconds for parsing, persistence, and the response after contention.
const PRIORITY_LOCK_WAIT_MS = 90_000;
const PRIORITY_LOCK_POLL_MS = 250;
```

### 3.2 逐 provider 解析调用点（实测行号）

| provider | 解析调用行 | provider | 解析调用行 |
| --- | --- | --- | --- |
| openclaw | `:1105` | kimi / kimi-code | `:2516` / `:2544` |
| claude | `:1201` | codebuddy / workbuddy / workbuddy-ai | `:2594` / `:2639` / `:2674` |
| gemini | `:1258` | omp / omo / pi | `:2708` / `:2747` / `:2780` |
| antigravity | `:1315` | prime-agent / minimax-code / craft | `:2809` / `:2837` / `:2865` |
| opencode（文件 + DB） | `:1361` / `:1375` | reasonix | `:2890` |
| qoder（multi + DB/new） | `:1419,1467,1471,1509` | grok-build（scan） | `:2960` |
| qoder-cn | `:1545,1549,1599` | copilot（store / otel / app-db） | `:3053,3108,3216,3282` |
| claude-science | `:1642` | kiro / kiro-cli | `:2325` / `:2478` |
| kilo-cli / dumate / mimo | `:1686` / `:1701` / `:1722` | hermes | `:2358,2379` |
| zcode | `:1776` | cursor（API） | `:2173` |
| dsh / command-code | `:1819` / `:1855` | trae / trae-cn | `:2200` / `:2280` |
| lmstudio / unsloth / anythingllm / devin | `:1879` / `:1897` / `:1916` / `:1935` | roocode / cline | `:2091` / `:2124` |
| kilocode / goose / droid | `:1960` / `:1994,2005` / `:2024` | zed | `:2063,2074` |

（上表由 `= await parse*Incremental(` / `= await multiInstallParse(` 在 `src/commands/sync.js` 内的全部命中整理；provider 用 `multiInstallParse` 包裹即表示它支持 native/WSL 双安装命名空间游标，见 §6.4。）

### 3.3 `queue.jsonl` vs `project.queue.jsonl`

两者由**同一次解析**产出，但维度不同：

| 维度 | `queue.jsonl` | `project.queue.jsonl` |
| --- | --- | --- |
| 归属键 | `(source, model, hour_start)` | **额外** `project_ref` + `project_key` |
| 写入函数 | `enqueueTouchedBuckets`（`src/lib/rollout.js:3169`） | `enqueueTouchedProjectBuckets`（`src/lib/rollout.js:3489`） |
| 状态文件 | `queue.state.json`（云端上传字节 offset） | `project.queue.state.json`（本地读侧 offset） |
| 是否上传 | **是**（唯一上传对象） | **否**——源码注释明确：`Only the main queue is uploaded by drainQueueToCloud`（`src/commands/sync.js:3656-3658`） |
| 触发条件 | `touchedBuckets` 非空 | `projectQueuePath && projectState && projectTouchedBuckets` 均存在（`src/lib/rollout.js:3494-3500`） |
| 读侧消费者 | `readQueueData`（`src/lib/local-api.js:202`） | `readProjectQueueData`（`src/lib/local-api.js:145`） |

读侧 key 公式（决定了“同键最新行胜出”的语义）：

```js
// src/lib/local-api.js:202-211（queue.jsonl）
const readQueueData = createAsyncQueueReader({
  label: "readQueueData",
  parseRow(row) {
    if (row?.kind === "account_session_state" || row?.kind === "account_sync_watermark") return null;
    return { key: `${row.source || ""}|${row.model || ""}|${row.hour_start || ""}`, row: normalizeQueueRow(row) };
  },
});
```

```js
// src/lib/local-api.js:145-153（project.queue.jsonl）
const readProjectQueueData = createAsyncQueueReader({
  label: "readProjectQueueData",
  parseRow(row) {
    return {
      key: `${row.project_key || ""}|${row.source || ""}|${row.hour_start || ""}`,
      row: normalizeQueueRow(row),
    };
  },
});
```

两个 reader 的 `key` 都不含 token 值，所以**后写的行覆盖先写的行**（`seen.set(key, row)`，`src/lib/async-queue-reader.js:55,86`）——这正是“append-only + 最新覆盖”的读侧契约，与 `compactPendingQueue` 在写侧做的“同键只留最后一条”互为镜像。

一行真实队列行的形状（`src/lib/rollout.js:3328-3345`）：

```js
toAppend.push(
  JSON.stringify({
    source: group.source,
    model,
    hour_start: group.hourStart,
    input_tokens: totals.input_tokens,
    cached_input_tokens: totals.cached_input_tokens,
    cache_creation_input_tokens: totals.cache_creation_input_tokens,
    output_tokens: totals.output_tokens,
    reasoning_output_tokens: totals.reasoning_output_tokens,
    total_tokens: totals.total_tokens,
    billable_total_tokens: totals.billable_total_tokens ?? totals.total_tokens,
    total_cost_usd: totals.total_cost_usd || 0,
    usage_precision: usagePrecision || undefined,
    ...prioritySubsetOf(totals),
    conversation_count: totals.conversation_count,
  }),
);
```

即共享情报里的字段集之外，实际还有 `billable_total_tokens`、`total_cost_usd`、`usage_precision`，以及可选的 `prioritySubsetOf(totals)` 展开项。另有**控制行**：`kind: "account_session_state"`（云端会话真值）与 `kind: "account_sync_watermark"`（已废弃，读侧显式丢弃，`src/commands/sync.js:4497`）。

### 3.4 写入方式：append-only，无独立写锁

- **写队列的 `fs.appendFile` 写点不止一处**：主队列 `queue.jsonl` 至少有 **6 处**——`src/lib/rollout.js:3483`（`enqueueTouchedBuckets`，`await fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8")`）、`src/lib/rollout.js:20031`（`retractStaleSourceQueueRows`）、`src/lib/rollout.js:22594`（`appendTraeCnSessionStates`）、`src/commands/sync.js:5126`（`repairGrokQueueFromSessionSnapshots`）、`src/commands/sync.js:5207` 与 `:5281`（`migrateRolloutCumulativeDeltaBuckets`）。另有两处写别的文件：`src/commands/sync.js:4114` 写 fallback 文件、`src/lib/rollout.js:3536` 写 `project.queue.jsonl`。
- **没有针对 queue.jsonl 的文件锁**。互斥完全依赖外层 `sync.lock`（`src/commands/sync.js:552-559`）——这要求“所有会把行写进队列的进程都必须是 sync 进程”。仓库内 `local-api.js` 触发的是 `spawn(tracker.js sync …)`（`src/lib/local-api.js:742-764`），符合该前提。
- 每行写完后，`bucket.queuedKey = key`（`src/lib/rollout.js:3346`）被记在**内存游标**上，`cursorStore.commit()` 时才落盘。这意味着：**若在 appendFile 之后、commit 之前进程被杀，下一次 sync 会重新扫到同一批数据并再次 append**（重复行），由读侧“同键最新覆盖”与云端整行 upsert 兜住幂等性。
- 另一条写路径是 **原子替换（整文件重写）**：`compactPendingQueue`（`src/lib/upload-queue.js:64-132`）与各类 repair（如 `repairClaudeQueueFromGroundTruth` 全文重写、`relabelClaudeMemQueueRows`，`src/commands/sync.js:6844,7147`）。即真实形态是「**append 与原子重写两条写路径，各自有多个实现点**」，而不是“某一种方式只在两处出现”。

`compactPendingQueue` 的竞态防护值得单独记录（`src/lib/upload-queue.js:94-121`）：写临时文件 `queue.jsonl.compact.<uuid>` → `fsync` → 关闭 → 关闭读句柄（Windows 不允许替换已打开的文件）→ **重新 stat 校验 `ino/size/mtimeMs` 未变** → `fs.rename` 原子替换；任一步不符就抛 `Queue changed during pending compaction`。同时只有 `pendingBytes >= 1_000_000` 且 `offset` 落在行边界（`offset-1` 处必须是 `\n`）才动手（`:73-78`）。

### 3.5 游标提交：一次提交、可短路

游标不是每个 provider 各提交一次，而是**全部解析完成后提交一次**（`src/commands/sync.js:3444-3446`）：

```js
if (!skipNoOpCursorCommit) {
  cursors.updatedAt = new Date().toISOString();
  await cursorStore.commit(cursors);
```

`skipNoOpCursorCommit` 是一个保守的“本轮什么都没变”判据（`src/commands/sync.js:3424-3437`），条件项共 **13 个**：`opts.auto`、`!isFullSourceScan`、`cursorStore.mode === "v2"`、`cursorStore.requiresCommit !== true`、`totalParsed === (commandCodeResult.cursorUnchanged ? commandCodeResult.recordsProcessed : 0)`、`totalBuckets === 0`、两处 project 桶均为 0（`grokResult.projectBucketsQueued`、`commandCodeResult.projectBucketsQueued`）、`!commandCodeResult.schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok`。若确实跳过但审计到期，则把审计计数**延迟累加**到 `deferred-codex-audit.json`（`:3456-3460`）。

### 3.6 上传：批量、预算、节流三重约束

`drainQueueToCloud`（`src/commands/sync.js:4315-4413`）的约束：

- 批大小：`batchSize` 默认 200，硬上限 `MAX_INGEST_BUCKETS = 500`（`:4313,4329`）。
- 单次调用批数：`maxBatches` 默认 5，`--drain` 时 100（`:3563`）。
- 总时长预算：`UPLOAD_BUDGET_MS = 90_000`，并用 `AbortSignal.timeout` 把同一预算传给 fetch（`:168,4362`）。
- `upload_scope === "community"` 时把批数再压到 10（`:4392`）。
- 每批成功后**立即**写 `queue.state.json` 的 offset（`:4397-4400`）——即 offset 是逐批推进的，不是整轮结束才落盘。
- 只上传**完整行**：`readQueueBatch` 遇到未以 `\n` 结尾的尾行直接 `break`（`:4447-4449`），避免把写者正在写的半行发出去。
- 同批内同一 `(source, model, hour_start)` 去重为一条（`bucketMap`，`:4510`）；`account_session_state` 按 `(source, session_id)` 取**严格更新的 `snapshot_verified_at`**（`:4478-4484`）。

自动上传还有一层节流：`AUTO_UPLOAD_CONFIG = { intervalMs: 15 min, batchSize: 200, maxBatchesSmall: 5, maxBatchesLarge: 100 }`（`src/commands/sync.js:162-167`）+ `decideAutoUpload` 的 jitter/backoff（`:3517-3534`）；失败时 `recordUploadFailure` 落 `upload.throttle.json`（`:3637-3643`），`--drain` 遇到节流直接抛 `SYNC_UPLOAD_BACKOFF`（`:3529-3534`）。

### 3.7 `sync` 的参数全集

`parseArgs`（`src/commands/sync.js:3711-3745`）支持：`--auto`、`--from-notify`、`--from-retry`、`--from-openclaw`、`--source <name>`（含 `--source=` 形式）、`--drain`、`--wait-for-lock`、`--background`（别名 `--lightweight`）、`--publish-account`、`--all-local-sources`、`--repair-grok`。未知选项抛错。

`--auto` 的语义被三处放大：① 决定 `sourceAllowed` 的白名单（`:719-735`）；② 决定 `skipNoOpCursorCommit`；③ 决定日志静默（`warnProviderParseFailure` 在 auto 下不打印，`:375-378`）。`--background` 进一步把 codex 深度扫描限制为“只有 `BACKGROUND_AUTO_SYNC_SOURCES`”四个源（`:367-373`）。

`resolveAutoSourceScope`（`:3747-3753`）把触发来源映射为**单一 provider 扫描范围**：openclaw → `"openclaw"`；notify/retry → `--source` 值（经 `AUTO_SYNC_SOURCE_ALIASES` 归一：`code→every-code`、`kilo-code→kilocode`、`deepseek→dsh` 等，`:310-319`）。因此 hook 触发的 sync 是**单源增量扫描**，而手动 `sync` 是全源。

---

## 4. 游标与增量状态

### 4.1 两代存储：legacy `cursors.json` 与 `cursor-store-v2/`

`openCursorStore`（`src/lib/cursor-store.js:25-107`）按**文件体积**决定用哪一代：

```js
// src/lib/cursor-store.js:21,77-86
const DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024;
...
const legacySize = Number(legacyFingerprint?.size || 0);
const shouldActivate = forceV2 || (
  Number.isFinite(activationBytes) && activationBytes >= 0 && legacySize >= activationBytes
);
if (!shouldActivate) {
  const cursors = (await readJson(cursorsPath)) || defaultCursorState();
  return new LegacyCursorStore({ cursorsPath, cursors });
}
```

| | LegacyCursorStore | V2CursorStore |
| --- | --- | --- |
| 载体 | 单个 `~/.tokentracker/tracker/cursors.json` | `cursor-store-v2/` 目录（manifest + generations） |
| 激活条件 | 文件 < 16 MB 且未迁移 | 文件 ≥ 16 MB，或已存在 manifest，或 `forceV2` |
| 提交 | `writeJson(cursorsPath, cursors)`（全量重写，`src/lib/cursor-store.js:199-201`） | 新 generation 全量落盘 + manifest 原子切换（`:855-1016`） |
| Codex 事件集 | 无独立存储（`codexEventStore = null`） | 按日分片的 `codex-events/*.txt`（`:663-726`） |
| 其他能力 | 全部空实现（`:177-197`） | `codex-files/*.json` 分片、opencode 消息/指纹分片、`canSkipCodexDay`、deferred audit |

`defaultCursorState()` 只有三个字段：`{ version: 1, files: {}, updatedAt: null }`（`src/lib/cursor-store.js:1816-1818`）。

v2 的目录布局（常量在 `src/lib/cursor-store.js:13-21`）：

```
~/.tokentracker/tracker/cursor-store-v2/
├── manifest.json                     # { version, current, rollback, previous, legacyFingerprint, updatedAt }
├── deferred-codex-audit.json         # 跳过的冷扫描审计计数
└── generations/<gen-…>/
    ├── generation.json               # metadata（含各分片 count 与完整性哈希）+ counts
    ├── core-v3.json                  # 非 Codex 文件游标 + hourly/projectHourly/… 全部非分片状态
    ├── codex-files/<YYYY-MM-DD>.json # 按日分片的 Codex 文件游标
    ├── codex-events/<YYYY-MM-DD>.txt # 按日分片的事件去重键（每行一个）
    ├── opencode-messages/…           # opencode 消息游标分片
    └── opencode-fingerprints/…       # opencode 指纹分片
```

### 4.2 v2 提交是“整代重建 + manifest 原子切换”

`V2CursorStore.commit`（`src/lib/cursor-store.js:855-1016`）的关键步骤：

1. 新建 `gen-<Date.now()>-<uuid8>` 目录（`newGenerationId`，`:1820-1822`）。
2. `cloneGenerationFiles` 把上一代文件中**不需要改写的**分片整体拷贝（`:877-881,1349-1362`）。
3. 只重写“脏”分片：`dirtyFileShards = loadedFileShards ∪ codexFilesByShard`（`:899-921`）；空分片显式 `delete nextMetadata.codexFiles[shardKey]`。
4. 事件集：若本轮 `materializedCodexHashes`，则整体重建（`rebuildEventShards`）；否则只增量追加 `pendingEventKeys`（`:923-945`）。
5. 写 `core-v3.json`（并显式 `delete core.codexHashes`，因为事件已分片，`:953-963`）。
6. 写 `generation.json`（含 `counts`，由 `calculateGenerationCounts` 计算，`:1744-1762`）。
7. **最后**用 `writeJson` 原子替换 `manifest.json`（`:987`），把 `current` 指向新代、`rollback` 指向旧代。
8. 清理只保留 `current/rollback/previous` 三代（`:1005-1009`）。
9. 失败回滚：若尚未 publish，删除半成品代目录（`:1010-1015`）。

这套设计的收益是：**任一时刻 manifest 指向的 generation 都是一致快照**，读取侧（`readCursorStateSummary`）在主代损坏时可自动回退到 `rollback` 或 `previous`（`src/lib/cursor-store.js:124-139`）。

值得注意的两个兼容字段（`:974-986`）：

```js
const compatibleGeneration = this.generation?.coreFile === "core.json"
  ? this.generation.id
  : this.manifest?.previous || null;
const nextManifest = {
  version: STORE_VERSION, current: generationId, rollback: this.generation?.id || null,
  // Older v2 readers reject core-v3.json and fall back to this frozen,
  // fully-inline checkpoint instead of opening an empty message index.
  previous: compatibleGeneration,
  legacyFingerprint: this.manifest?.legacyFingerprint || null,
  updatedAt: new Date().toISOString(),
};
```

即：老版本读者遇到 `core-v3.json` 会拒绝，此时靠 `previous` 指向的**冻结内联检查点**兜底，避免“读到空消息索引”。

### 4.3 分片与归属规则

- **Codex 文件归属**：`isCodexSessionCursorPathForRoots`（`src/lib/cursor-store.js:1771-1779`）——路径形如 `**/.codex/sessions/**` 或 `**/.codex/archived_sessions/**`，**或**位于本进程 `codexRoots` 下的 `sessions/`、`archived_sessions/`。命中 → 进按日分片；否则进 `core-v3.json`。
- **文件分片键**：从路径里抓 `/(\d{4})/(\d{2})/(\d{2})/`，得到 `YYYY-MM-DD`，否则 `misc`（`:1788-1793`）。
- **事件分片键**：键格式是 `<identity>:<ISO 时刻>`，取冒号后 10 字符当日期，非法则 `misc`（`:1800-1805`）。
- **文件名校验**：`safeShardFilename` 只允许 `^\d{4}-\d{2}-\d{2}$`，否则写 `misc`（`:1807-1809`）——防止分片键成为路径穿越。

这套分片直接服务于“**只加载今天需要的分片**”：`loadCodexFilesForPaths`/`canSkipCodexDay`（`:299-318`、`:609-639`）让一次增量 sync 不必把几十万个历史文件游标读进内存。`codexRoots` 必须与扫描根**同源**（`src/commands/sync.js:635-652` 的注释解释了不一致会导致同文件被反复重解析，见 issue #639/#657）。

### 4.4 单文件游标的字段（三套并存）

**Codex 系**（`src/lib/rollout.js:632-665`）：

```js
const nextCursor = {
  inode,
  offset: result.endOffset,
  lastTotal: result.lastTotal,
  tokenUsageBaselines: result.tokenUsageBaselines,
  lastModel: result.lastModel,
  lastServiceTier: result.lastServiceTier || null,
  modelAttributionState: result.modelAttributionState,
  updatedAt: new Date().toISOString(),
};
// 可选追加：codexUsageRecord（本文件是否只有 token_usage_record 行）
// projectOffset / projectFileContext（仅 Codex 项目快速路径）
```

**Claude 系**（`src/lib/rollout.js:1192-1206`）：`inode` / `offset` / `claudeForkIndexed` / `updatedAt` / `fileId` / `fileIdSize` / `claudeCwd` / `projectFileContext` / `projectRef` / `projectKey`；跨环境重复文件另写 `duplicateOf: <primary 路径>`（`:1078-1093`）。

**其它按行号定位的 provider**：不记字节 offset，改用 `mtimeMs` + 行序号类游标，按 provider 分三套：

- **KiroCli**（`parseKiroCliIncremental`，自 `src/lib/rollout.js:8886` 起）：`mtimeMs` + `lastIndex`（行序号）+ `lastTotals`，游标**读取**见 `:9371-9375`（`prevEntry.mtimeMs` / `lastIndex`，`:9376` 的 `mtimeMs <= prevMtime` 早退），**写回**见 `:9468-9472`（`fileOffsets[filePath] = { mtimeMs, size, lastIndex }`）；`:9451-9467` 是桶累加与 `onProgress` 回调段，不是游标写回。整个 `fileOffsets` 容器在 `:9482` 并入 `cursors.kiroCli`。
- **Gemini**：`mtimeMs` + `lastIndex`（`src/lib/rollout.js:1289-1307`）。
- **opencode**：游标是 `inode` + `size` + `mtimeMs` + `lastTotals` + `messageKey` + `opencodeForkRepairVersion`（恒写 `1`），**没有 `lastIndex`**（`src/lib/rollout.js:1462-1516`；`:1519` 写 `opencodeForkRepairVersion: 1`，`:1498` 调 `parseOpencodeMessageFile`，`:1513-1521` 写回游标）。该键**参与“文件未变”判定**（`:1465` 要求 `prev.opencodeForkRepairVersion === 1`），因此**旧代游标（无此键）会被判为已变并重扫一次**，之后才回到稳态短路。

这是对“文件会被整体重写、但内容按行追加”的妥协方案。

### 4.5 “从哪读起”的判定算法

核心只有 30 行（`src/lib/rollout.js:487-518`）：

```js
const prev = cursors.files[key] || null;
const inode = st.ino || 0;
const sameInode = prev && prev.inode === inode;
const prevOffset = sameInode ? prev.offset || 0 : 0;
const truncated = sameInode && prevOffset > st.size;
const rebuildingCodexBaseline = Boolean(
  (fileSource === DEFAULT_SOURCE || fileSource === "acode") &&
  sameInode && !truncated && prevOffset > 0 && prevOffset < st.size &&
  (!prev.lastTotal || typeof prev.lastTotal !== "object"),
);
const startOffset = sameInode && !truncated && !rebuildingCodexBaseline ? prevOffset : 0;
```

| 情形 | `sameInode` | `truncated` | 起点 | 说明 |
| --- | --- | --- | --- | --- |
| 首次见到 | 否 | — | 0 | 全新文件全量读 |
| 稳态追加 | 是 | 否 | `prev.offset` | 只读尾部新增字节 |
| 文件被截断/重写变小 | 是 | 是 | 0 | 重读全文（`prevOffset > st.size`） |
| 原子替换（新 inode） | 否 | — | 0 | 重读全文 |
| 游标缺 `lastTotal`（老格式） | 是 | 否 | 0 | `rebuildingCodexBaseline` 强制重建基线 |

稳态短路（“文件没长就跳过”，`:538-555`）：

```js
if (sameInode && !truncated && startOffset >= st.size &&
    (!projectEnabled || projectUpToDate)) {
  // 仅回调进度，不做任何解析、不重写游标
  continue;
}
```

注意 `projectUpToDate` 分支：开了项目归因时，即使文件没增长也要检查 `projectFileContext` 的**新鲜度**（git 配置指纹过期就重扫上下文），并存在一条只解析项目上下文、不算 token 的路径 `projectContextOnlyScan`（`:520-537,589-604`）。

### 4.6 重写 / 截断 / 复制三重威胁与去重层

**威胁一：同 inode 被整体重写（内容不复现）** → 靠“**事件身份集合**”兜底。Codex 的事件键是 `sessionUUID:eventTimestamp`（`src/lib/rollout.js:292-295` 注释），持久化在 `cursors.codexHashes` 或 v2 的 `codex-events/<日期>.txt` 分片里。

**威胁二：换 inode 原子替换（Codex-Manager 换账号时重写 rollout）** → 起点回 0，但事件键命中则跳过，因此幂等（issue #187，`src/commands/sync.js:258-268`）。

**威胁三：同一 session 出现在两条路径（native ∪ WSL、`sessions/` → `archived_sessions/`）** → `needsHistoricalRolloutDedup` 决策“用持久集合还是本轮集合”（`src/lib/rollout.js:428-461`）：

```js
const sessionId = codexSessionIdFromPath(filePath);
if (!sessionId) return true;
const knownPaths = getCursorSessionPaths().get(sessionId);
if (knownPaths) { for (const knownPath of knownPaths) { if (knownPath !== filePath) return true; } }
// Steady state: one path, already read past, not rotated.
if (sameInode && !truncated && startOffset > 0) return false;
```

即：**只要同一 session 存在第二条游标路径，就必须动用持久化事件集合**（本轮集合只知道本次运行写入的键），否则落后一侧会把历史重播进桶（注释原文见 `:439-446`）。

去重层的实现与上限：

- `createRolloutEventDedup`（`src/lib/rollout.js:302-380`）提供两种视图：`getAppendOnlyEvents`（只看本轮新键 + 内存集合）与 `getHistoricalEvents`（查持久集合或 v2 分片存储）；`persist()` 把新键追加回游标数组。
- Codex 用 `codexHashes`，acode 用 `acodeHashes`（`:381-391`）。
- Claude 侧用**消息键**：`claudeMessageDedupKey = msgId ? (reqId ? `${msgId}:${reqId}` : msgId) : null`（`:4914-4919`），持久化时**截断到最近 10 万条**（`:1233-1237`）。

```js
cursors.claudeForkAliases = forkAliases;
const allHashes = Array.from(seenMessageHashes);
cursors.claudeHashes =
  allHashes.length > 100_000 ? allHashes.slice(allHashes.length - 100_000) : allHashes;
```

- 跨环境**文件级**去重：Claude 走 `claudeFileIdentityHash(filePath, st)`（`:937`）。同一 `fileId` 的第二份文件被标记为 `duplicateOf` 并把 offset 直接推到 `st.size`（`:1070-1105`），既防双计也不反复读；缓存判据是 `prev.fileIdSize === st.size`（`:1057-1069`）。
- 另有 `compactionResponseIds`（按 `response_id` 统计 Codex compaction，属 core 游标状态，`src/lib/rollout.js:287-290`）与 Codex 的 `codexUsageRecordOnlyFiles` 集合（只含 `token_usage_record` 的文件既不计数也不进分片，`src/lib/rollout.js:646-655`）。

### 4.7 冷扫描审计：把“全量重扫”摊到 24 小时

后台轻量 sync 只扫近期文件，于是需要一个机制保证历史最终被审计到。`isCodexColdScanAuditDue`（`src/commands/sync.js:3763-3783`）在三种情况下要求做一次全量：

1. 从来没有审计状态（`version !== 1` 或 `lastFullScanAtMs` 非法）；
2. 距上次全量 ≥ `CODEX_COLD_SCAN_AUDIT_INTERVAL_MS = 24h`（`src/commands/sync.js:294`）；
3. 累计“自上次全量以来的 sync 次数” ≥ `CODEX_COLD_SCAN_AUDIT_MAX_SYNCS = 288`（`:301`）——288 ≈ 24h × 12 次/小时，与 5 分钟粒度对应。

另有一条**时钟回拨保护**：`lastFullScanAtMs - nowMs > 5 * 60 * 1000` 时也判定到期（`:3772`）。

跳过的轮次不丢计数：`recordCodexColdScanAudit`（`:3785-3819`）写 `cursors.codexColdScanAudit`，字段共 **6 个**：`{version, lastFullScanAtMs, lastFullScanAt, syncsSinceFullScan, lastSkippedFiles, updatedAt}`（`lastFullScanAt` 是 ISO 文本副本，在 `:3810` 全量分支与 `:3813-3815` 非全量分支写入）；而 `skipNoOpCursorCommit` 时这些增量写进 `deferred-codex-audit.json`（`cursorStore.writeDeferredCodexAuditSyncs`，`src/lib/cursor-store.js:640-660`）。

### 4.8 repair / migration 体系：一次性的“历史重算”

这是本仓库最有学习价值、也最危险的部分：**每一次解析口径变更都伴随一个一次性迁移键**，并在“磁盘是真相”的前提下重算历史。键常量集中在 `src/commands/sync.js:204-309`，export 在 `:3847-3857`。

| 迁移键 | 行号 | 触发场景（源码注释口径） |
| --- | --- | --- |
| `cursorUnknownPurge_2026_04` | `:204` | 清理 `model=unknown` 的历史桶 |
| `rolloutCumulativeDeltaReparse_2026_05` | `:205` | Codex 累计量 delta 口径修正 |
| `claudeMemObserverReinclude_2026_05_v3` | `:206` | 重新纳入 claude-mem observer 会话（v3） |
| `grokAppendOnlyRepair_2026_05_v4` | `:207` | Grok append-only 重算 |
| `codebuddyLogJsonlOverlapRepair_2026_08` | `:208` | CodeBuddy log/jsonl 重叠 |
| `workbuddyContextUsageRepair_2026_08` | `:209` | WorkBuddy 上下文用量口径 |
| `claudeGroundTruthRepair_2026_05_v4` | `:248` | Claude 全量真值重建（**历经 v1–v6 的翻车史**） |
| `cloudConversationsBackfill_2026_06` | `:257` | 云端 `conversation_count` 全量重放 |
| `codexRescanDedupRepair_2026_06` | `:268` | inode 重写导致的双计修复（issue #187） |
| `codexForkReplayRepair_2026_07` | `:280` | fork 会话重播父会话 token 的修复 |
| `codexUsageLineageRepair_2026_07_v2` | `:288` | 交错 SessionState 造成的超大 delta |
| `droidDupSessionInflationRepair_2026_06` | `:293` | Droid 重复会话膨胀 |
| `mimoClaudeMislabelRepair_2026_06` | `:306` | mimo 误标 Claude 历史，清 `source=mimo` |
| `deepseekHarnessSourceMigration_2026_08` | `:307` | `source=deepseek` → `dsh` 改名 |
| `zcodeNativeUsageRepair_2026_08` | `:308` | ZCode 原生用量口径 |
| `zcodeInclusiveTokenRepair_2026_09` | `:309` | ZCode input 含缓存的修正 |

**必须引用的翻车史**（`src/commands/sync.js:211-248`，原文摘录）：

```js
// v1 had a cursor-format bug (wrote plain integer instead of {inode, offset,
// updatedAt}), which made parseClaudeIncremental reread every jsonl from
// byte 0 on the next sync and double everything. v2 fixed the format.
// v3 fixes two latent issues caught by adversarial review: ...
// v6 was bumped in 0.26.3 to re-run the repair with the zero-usage dedup fix
// applied. SHIPPING 0.26.3 caused catastrophic data loss on every upgrader
// whose ~/.claude session jsonls had been pruned by Claude Code's own
// cleanup: the repair does atomic-drop + rescan, so any hour_start no longer
// represented in the on-disk logs is silently removed from queue.jsonl. On
// the reporter's machine this wiped 2.17B claude tokens ...
// 0.26.4 HALTS back at v4 so the buggy atomic-rewrite path stops auto-firing
```

对应到代码，后续所有 repair 都补上了**“盘上数据不全就不修”**的守卫。最典型的是 `repairCodexRescanInflation` 的预检（`src/commands/sync.js:258-268`）：

```js
// GUARDED: skips if any codex session file that previously contributed is no longer on disk
// (deleted, or moved to ~/.codex/archived_sessions/ which sync does not scan) — clearing its bucket
// would lose that history (ref the v6 ground-truth-repair data-loss incident).
```

以及 Claude 真值重建前的扫描根可用性守卫（`:1147-1192`）：如果某个非 native 根当前不存在/不可读，且该根历史上贡献过文件，就**推迟修复**（migration key 保持未设置，等下次全量再试），而不是在缺数据的情况下重建。

同类守卫还包括：`scanForForkedCodexRollout` 预检 fork 是否存在（`:6347-6370`）、`validateCodexRebuildFileSnapshots`（`:6294-6315`）、`isCodexHistoryCovered`（`:6316-6346`）、以及修复过程中对 `queue.jsonl` 的“保留不可解析行”策略（`:6745` 注释，属 Droid 修复）。

【存疑】其它 repair 的“保留不可解析行”语义**未逐条核对**——`repairClaudeQueueFromGroundTruth`、`relabelClaudeMemQueueRows` 等的保留策略只在本轮核到 `:6745` 与 `:6288-6372` 段，前述 §3.4 引用的 `sync.js:6844,7147` 也未逐行打开，留待下一轮补齐。

### 4.9 游标损坏的恢复路径

- 主代损坏：`readCursorStateSummary` 依次尝试 `manifest.current` → `manifest.rollback` → `manifest.previous`（`src/lib/cursor-store.js:124-139`）；`openManifestGeneration` 失败则抛 `TOKENTRACKER_CURSOR_STORE_CORRUPT`（`:72-74`）。
- 分片完整性：每个分片写盘时把 `serialized` 的摘要与 count 记入 metadata（`buildShardIntegrity`，`:1466-1471`），读取时 `assertShardIntegrity` 校验（`:1473-1481`）；不一致抛 corruption。
- **可重试**与**损坏**的分野：分片缺失导致的 `TOKENTRACKER_CURSOR_STORE_RETRY` 是“加载缺失分片后重试”，sync 捕获后会 `loadCodexFilesForPaths` 再跑一次全量解析（`src/commands/sync.js:1069-1085`）；而 `TOKENTRACKER_CURSOR_STORE_CORRUPT` 直接上抛，不做静默修复（`:1086-1087`）。
- 回退代的存在使“读到旧一代”成为合法状态，且 `legacyFingerprint` 会检测 `cursors.json` 在 v2 建立之后是否被外部改动，一旦改动就**重新迁移**（`src/lib/cursor-store.js:44-58`）。

---

## 5. 被动 / 自动模式与调度

### 5.1 四个调度源

| 源 | 频率 | 范围 | 实现 |
| --- | --- | --- | --- |
| provider hook → notify.cjs | 事件驱动，**20s 节流** | 单源（`--source`） | `src/commands/init.js:1519-1536` |
| omp 扩展 | 事件驱动，**12s 节流** | `omp` | `src/lib/omp-hook.js:66,99-109` |
| OpenClaw 插件 | 事件驱动，**15s 节流** | `openclaw` | `src/lib/openclaw-session-plugin.js:363,502-518` |
| serve 后台轮询 | **60s**（仅 Windows 壳，且未被 desktop 托管时） | 本地全源轻量 | `src/commands/serve.js:35,294-366` |
| 自动重试（backoff 到期） | 一次性延迟进程，上限 2h | 触发来源单源 | `src/commands/sync.js:4169-4293` |

### 5.2 serve.js 的 60s 后台轮询（`startNativeBackgroundSync`）

```js
// src/commands/serve.js:303-309,339-345
const normalizedShell = String(appShell || "").trim().toLowerCase();
if (normalizedShell !== "windows") return null;
// The Windows tray host owns its five-minute background sync timer ...
if (String(nativeSyncOwner || "").trim().toLowerCase() === "windows-host") return null;
...
const backgroundEnv = { ...process.env, TOKENTRACKER_WSL_MODE: "native-only" };
const pending = Promise.resolve()
  .then(() => sync(["--auto", "--background", "--all-local-sources"], { env: backgroundEnv }))
```

六个设计点：

1. **仅 Windows**（`TOKENTRACKER_APP_SHELL === "windows"`），macOS 壳自管唤醒循环；注释说明双循环会让 `sync.lock` 近乎连续占用、饿死 provider 的 notify sync（`src/commands/serve.js:248-256`）。
2. **桌面托管时让位**（`TOKENTRACKER_NATIVE_SYNC_OWNER=windows-host` 时返回 null，`:305-309`）。
3. **跑在子进程**：解析器里有 `execFileSync` 读 SQLite，在主进程会冻结事件循环并连带阻塞所有 Dashboard 端点（`:311-315`）。子进程 spawn 参数与 hook 一致，因此与 hook sync 争同一把 `sync.lock`。
4. **强制 `native-only`**：避免 UNC 元数据探测唤醒 WSLg 并弹出不可见的 msrdc 窗口抢焦点（`:335-342`）——这是一个很具体的平台坑，值得记入我们的对照清单。
5. **单飞防重入**：模块内 `let inFlight = null;`，`run()` 首句 `if (inFlight) return inFlight;`——上一轮子进程未退出时**不再 spawn 第二个**，避免慢解析叠成并发洪峰（`src/commands/serve.js:332-334`）。
6. **关停路径**：`SIGINT`/`SIGTERM` → `nativeBackgroundSync?.stop()`（清定时器）→ `server.close()` 回调里 `process.exit(0)`，另有 `setTimeout(…, 3000)` 兜底强退（`src/commands/serve.js:281-288`）。

另有 6 小时一次的心跳定时器（`src/commands/serve.js:266-273`），走 `maybeSendHeartbeat`，与 sync 尾部用的是同一节流状态（`src/commands/sync.js:3699-3704`）。

### 5.3 锁争用可见化：`sync.skip.json`

被跳过的 sync 过去只静默 exit 0，导致“被杀进程留下的锁碎片”能让解析停滞一整天而无人察觉（issue #431）。现在每次抢锁失败都写 marker（`src/commands/sync.js:484-506`）：

```js
const SYNC_SKIP_MARKER = "sync.skip.json";
async function recordSyncSkip(trackerDir, lockPath) {
  const holder = await inspectLock(lockPath);
  const reason = holder.exists && holder.pid && !holder.alive ? "lock_debris" : "lock_busy";
  const detail = !holder.exists ? "sync lock could not be acquired"
    : holder.pid ? `sync lock held by pid ${holder.pid} (${holder.alive ? "running" : "not running"})`
    : "sync lock has no owner record";
  await writeJson(path.join(trackerDir, SYNC_SKIP_MARKER), { reason, detail, at: ..., lockPath });
  return `Sync skipped: ${reason} — ${detail}\n`;
}
```

差异很关键：`lock_busy` = 正常的并发争用；`lock_debris` = 持有者已死，属需处理状态。成功拿到锁后立即清除 marker（`:565`）。`status` 读它并输出 `last_sync_skipped`（`src/commands/status.js:285,351-353,1058`）。

### 5.4 自动重试：延迟进程 + marker 双重控制

`scheduleAutoRetry`（`src/commands/sync.js:4169-4228`）写出 `auto.retry.json`（`AUTO_RETRY_FILENAME`，`:4309`），payload 形如 `{version:1, retryAtMs, retryAt, reason, pendingBytes, scheduledAt, source, syncSource?}`；随后 spawn 一个**分离的 `node -e` 脚本**，脚本体内 `setTimeout` 到点后重新读 marker：

```js
// src/commands/sync.js:4278-4285（生成的脚本片段）
if (!retryAtMs || Date.now() + 1000 < retryAtMs) process.exit(0);
const argv = ['sync', '--auto', '--from-retry'];
if (payload && typeof payload.syncSource === 'string' && payload.syncSource.trim()) {
  argv.push('--source', payload.syncSource.trim());
}
const cmd = fs.existsSync(trackerBinPath) ? [process.execPath, trackerBinPath, ...argv]
                                          : ['npx', '--yes', fallbackPkg, ...argv];
```

要点：① 延迟上限 `AUTO_RETRY_MAX_DELAY_MS = 2h`（`:4310`）；② **marker 是权威**——被 `clearAutoRetry` 删掉后，醒来的进程会因读不到 `retryAtMs` 直接退出（`:4244-4247`）；③ 已有更早的重试计划时不重复排（`:4186-4202`）；④ `TOKENTRACKER_AUTO_RETRY_NO_SPAWN` / `config.autoRetryNoSpawn` 可禁止 spawn，只留 marker（`:4217-4219`，`src/lib/runtime-config.js:69-75`）。

触发条件：`pendingBytes > 0` 且有 `canUpload()`，且本轮确实尝试过上传、且不是 `publishAccount`（native 壳自管）（`src/commands/sync.js:3654-3676`）。

### 5.5 环境变量开关表（采集/调度相关子集）

以下是 `src/` 内实际被读取的 `TOKENTRACKER_*` 变量（全量 100+ 个中与本文范围相关者）：

| 变量 | 作用 | 位置 |
| --- | --- | --- |
| `TOKENTRACKER_WSL_MODE` | `wsl-first`（默认）/ `native-first` / `wsl-only` / `native-only` / `both` | `src/lib/wsl-probe.js:18-24,106-123`；后台轮询强制 `native-only`（`src/commands/serve.js:341`） |
| `TOKENTRACKER_APP_SHELL` / `TOKENTRACKER_NATIVE_SYNC_OWNER` | 决定是否启用 serve 侧 60s 轮询 | `src/commands/serve.js:295-309` |
| `TOKENTRACKER_LOCAL_SYNC_DEVICE_TOKEN` / `..._ATTEMPT_ID` | 本地 API 触发的 sync 传递凭证与批次标识 | `src/lib/local-api.js:761,2282`；`src/commands/sync.js:543-545,3490-3492` |
| `TOKENTRACKER_CLOUD_RECHECK` | `--drain` 时强制重探云端权限 | `src/commands/sync.js:3514-3523` |
| `TOKENTRACKER_AUTO_RETRY_NO_SPAWN` | 禁止自动重试 spawn | `src/lib/runtime-config.js:69-75`；`src/commands/sync.js:4217` |
| `TOKENTRACKER_NOTIFY_DEBUG` / `..._MAX_BYTES` | notify.cjs 调试日志与上限 | `src/commands/init.js:1489-1493` |
| `TOKENTRACKER_SKIP_FIRST_SYNC` / `..._SKIP_LOCAL_RUNTIME_COPY` / `..._SKIP_OPENCLAW_CLI` | init 阶段跳过项 | `src/commands/init.js:2271,2330`；`src/lib/openclaw-cli.js:58` |
| `TOKENTRACKER_SKIP_*`（其它） | 各 provider 的路径覆盖与调试（如 `TOKENTRACKER_ACODE_HOME`、`TOKENTRACKER_OMP_AGENT_DIR`、`TOKENTRACKER_ZCODE_HOME`） | 分布于 `src/lib/*-config.js`、`src/lib/rollout.js` |
| `TOKENTRACKER_DEBUG` | 提级错误输出 | `bin/tracker.js:13`；`src/lib/runtime-config.js:68` |

【未能确认】共享情报提到的 `TOKENTRACKER_LOCAL_SYNC_*` 之外的“被动模式开关”（例如强制 passive）在仓库内**不存在**——passive 是**探测结果**而非配置项（`src/lib/passive-mode.js` 内不接受任何 env 开关，只有 `detectPassiveProviders({home, hookStatus, env})` 三个入参）。

### 5.6 自动模式的“单源收敛”语义

一次 hook 触发的 sync 并**不会**扫全部 provider：`resolveAutoSourceScope` 把来源收敛为单一 provider（`src/commands/sync.js:3747-3753`），再由 `sourceAllowed` 门控每个 provider 段（`:719-735`）。后台轮询虽带 `--all-local-sources`，但仍受 `AUTO_SYNC_SOURCES`（45 项白名单）与 `BACKGROUND_AUTO_SYNC_SOURCES`（仅 `acode/codex/every-code/reasonix` 四个）两层约束（`:320-373`）。

这意味着新鲜度的**分层**：

- 你刚用过的那个 agent：hook 触发，秒级；
- 其它 hook 驱动的 agent：靠 `Stop`/`SessionEnd`/notify 各自触发；
- Windows 上所有本地源：60s 兜底；
- 深度历史（archived_sessions、冷扫描审计）：24h 或 288 次 sync 后才有一次完整扫描。

---

## 6. 多安装 / 多根目录发现与去重

### 6.1 三层发现模型

```
① 出产地（scan-roots）：native home、WSL home、CODEX_HOME/CLAUDE_CONFIG_DIR、config.scanRoots.<provider>[]
        ↓  realpath 归一 + 可列性校验
② 安装选择（install-resolver）：union（都收）或单选（wsl-first / native-first / wsl-only / native-only / both）
        ↓  可选 requireAnyChild 过滤“空壳安装”
③ 文件发现（rollout.js）：listRolloutFiles / listRolloutFilesDeep / listClaudeProjectFiles / …
        ↓
④ 去重三层：sessionsDir 去重 → 路径无关身份去重 → 事件/消息哈希去重
```

### 6.2 `config.scanRoots`：让所有生产者看到同一份根列表

`src/lib/scan-roots.js` 头部注释把问题定义得很清楚（`:3-14`）：某个 agent harness 用私有 home（`CODEX_HOME=...`）跑 Codex，这些会话**只在“恰好继承同一环境变量的进程”里可见**——hook 触发的 sync 看得到，桌面 App 的后台刷新看不到；更糟的是两个生产者对“某 rollout 的游标存在哪”无法达成一致（游标存储只把位于自己 `codexRoots` 下的 rollout 归入按日分片），于是**每次交替都从字节 0 重解析**（issue #639 记录了“一个 30 分钟桶被重发 138 次、单日 1.2B token 报成 59B”）。

解法：根列表来自**持久化配置**，且游标存储的 `codexRoots` 由同一列表派生（`src/commands/sync.js:635-652`）。合并顺序（先出现者胜，realpath 去重，`src/lib/scan-roots.js:19-23,163-202`）：

1. 调用方本来就会扫的根（native home / WSL 安装）→ `origin: "native"`；
2. 本进程的隐含根 `CODEX_HOME` / `CLAUDE_CONFIG_DIR` → `origin: "env"`；
3. `config.scanRoots.<provider>[]` → `origin: "config"`。

两个细节：**相对路径一律相对 `home`**（绝不相对 cwd，因为 hook/CLI/桌面 App 的 cwd 不同，`:46-55`）；非 native 且当前不可用的根**仍保留在列表里**（让游标路径分类保持稳定），只是调用方用 `exists` 跳过（`:158-162,228-232`）。

配置形状（注释给出，`:25-27`）：

```json
{ "scanRoots": { "codex": ["~/.local/share/agent/codex"],
                 "claude": ["~/.local/share/agent/claude", "~/.claude-native"] } }
```

`scanRootDirState` 的“存在”定义是**能被列目录**，而不只是能 stat（`:89-110`）——因为一个 mode 000 的目录若被判为存在，就会进入扫描、产出 0 个文件、在 status 里显示“存在”，并绕过 sync 的 deferred-repair 守卫。这个判据很值得抄。

### 6.3 WSL 探测（`src/lib/wsl-probe.js`）

| 机制 | 实现 |
| --- | --- |
| 枚举发行版 | `wsl.exe -l -v`，**UTF-16LE 解码**，清洗 `\0`/BOM，解析 `*` 默认标记与版本号（`:26-55`） |
| 排序 | 默认发行版优先（`:91`） |
| 缓存 | 进程内缓存（`_cachedDistros`）；strict 模式会重试被任何调用者缓存过的失败（`:57-97`） |
| 空列表判定 | 某些 WSL 版本对空 verbose 列表返回非 0，此时用 `wsl.exe -l -q` 成功且为空来**确认**空，而不是靠诊断文本（`:69-80`） |
| 用户与 home | `wsl.exe -d <distro> -e whoami` → `\\wsl$\<distro>\home\<user>\<providerDir>`；**v1 发行版优先 `\\wsl.localhost\`**，v2 优先 `\\wsl$`（`:180-204`） |
| 模式 | `TOKENTRACKER_WSL_MODE` ∈ {`wsl-first`(默认), `native-first`, `wsl-only`, `native-only`, `both`}（`:18-24,106-109`） |
| 选择 | `pickWin32Path` / `resolveAllWin32Paths`（`:125-159`）；**:149 本身不是“非 win32 返回 native”**，而是 `if (platform !== "win32") return { native: null, wsl: null };`（两侧都为 null，即发现层根本不做 WSL 探测）。“非 win32 一律只返回 native”的真正落点在上一层 `src/lib/install-resolver.js:21-23`（`if (platform !== "win32") return { native: nativeValue ?? null, wsl: null };`） |
| cwd 重锚定 | WSL 会话的 POSIX cwd 在 Windows 上会被解析成 `C:\home\...`，导致项目归因永远失败（issue #374）。`mapWslCwdToUnc` 用**转录文件自身**的 UNC 前缀重锚（不重新探测，因此能挑到本机真正可用的 `\\wsl$`/`\\wsl.localhost`），并显式跳过 `/C:/...` 这种“以斜杠开头的 Windows 路径”（`:206-242`） |
| SQLite 快照 | `snapshotSqliteDb` 把 db 与 `-wal`/`-shm`/`-journal` 一起复制到临时目录再读（`:244-262`）——跨 UNC 读活跃 SQLite 的必要手段 |

### 6.4 安装选择与“空壳安装”防护（`src/lib/install-resolver.js`）

`resolveInstallPaths` 的三种行为（`:19-45`）：

```js
function resolveInstallPaths({ nativeValue, wslDir, wslValue, requireAnyChild, union } = {}, env = process.env, deps = {}) {
  const platform = deps.platform || process.platform;
  if (platform !== "win32") return { native: nativeValue ?? null, wsl: null };
  const populated = (p) => !requireAnyChild || hasAnyChild(p, requireAnyChild, deps.existsSync);
  const wslExists = requireAnyChild
    ? (p) => Boolean(pathExists(p, deps.existsSync)) && hasAnyChild(p, requireAnyChild, deps.existsSync)
    : deps.existsSync;
  const wslCandidate = wslValue !== undefined
    ? (wsl.shouldProbeWsl(env) && pathExists(wslValue, deps.existsSync) && populated(wslValue) ? wslValue : null)
    : (wslDir && wsl.shouldProbeWsl(env) ? wsl.discoverWslHome(wslDir, { ...deps, env, existsSync: wslExists }) : null);
  const nativeCandidate = wsl.shouldProbeNative(env) && nativeValue && populated(nativeValue)
    ? pathExists(nativeValue, deps.existsSync) : null;
  if (union) return { native: nativeCandidate, wsl: wslCandidate };
  return wsl.resolveAllWin32Paths({ nativeValue: nativeCandidate, wslValue: wslCandidate, env, platform });
}
```

两个关键设计：

1. **`requireAnyChild` 折进探测本身**：`wslDir` 路径下把“必须含 `sessions`/`archived_sessions`”塞进 `existsSync`，于是 `discoverWslHome` 会**跳过空壳发行版继续找下一个**。注释解释了为何必须如此：先 `discoverWslHome` 再校验的话，第一个发行版里一个空 `~/.codex` 壳会胜出、被判无效，而**真正有数据的后面那个发行版永远不会被看到**（`src/commands/sync.js:761-772`）。
2. **`union` 把模式从“排他”降级为“优先级”**：因为“一个 WSL 的空壳/B 安装把 native 顶掉”曾导致“会话浏览器列得出这些会话、Dashboard 却一个 token 都不计”（issue #27，`src/lib/install-resolver.js:6-13`）。前提是**该 provider 的解析器按路径无关身份去重**（Codex 用 `sessionUUID:eventTimestamp`，Claude 手工 union + 文件哈希/消息哈希），否则 union 就是双计。

`union` 的调用点（实测）：codex（`src/commands/sync.js:782-787`，`requireAnyChild: ["sessions","archived_sessions"]`）、acode（`:815-820`，同）；而 every-code/gemini 等仍是单选（`:855`、`:1228`）。

### 6.5 命名空间游标：双安装不再共享一份状态

当 `union` 给出两个安装时，`multiInstallParse` 用**按安装名分区的游标**（`src/lib/multi-install-parser.js:8-59`）：

```js
async function multiInstallParse({ paths, parserFn, providerName, cursors, getParams, onProgress, detectInstall, ...shared }) {
  const installKeys = Object.keys(paths).filter(k => paths[k]);
  if (installKeys.length === 0) return emptyResult();
  if (installKeys.length === 1) {
    // Flatten toward the SURVIVING install's namespace ...
    const survivorKey = wsl.isUncPath(paths[installKeys[0]]) ? "wsl" : "native";
    ensureFlatCursor(cursors, providerName, env, survivorKey);
    return await parserFn({ ...getParams(paths[installKeys[0]], installKeys[0]), ...shared, cursors, onProgress });
  }
  const activeKeys = resolveMigrationSeedKeys({ paths, installKeys, providerName, cursors, detectInstall });
  const ns = ensureNamespacedCursors(cursors, providerName, activeKeys);
  for (let i = 0; i < installKeys.length; i++) {
    const key = installKeys[i];
    cursors[providerName] = ns[key];
    const result = await parserFn({ ...getParams(paths[key], key), ...shared, cursors, onProgress: wrapProgress(onProgress, key) });
    ns[key] = cursors[providerName];
    ...
  }
  cursors[providerName] = ns;
```

加粗三条不变量：

- **单安装时必须回落到扁平游标**，且“幸存者”由 **UNC 前缀判定**（`wsl.isUncPath`）而不是模式偏好——注释明写：wsl-first 下若 WSL 安装消失（发行版被删、探测瞬时失败），其游标不得被“捐赠”给剩下的 native 安装，否则后者会双计整个历史（`src/lib/multi-install-parser.js:13-22`）。
- **扁平 → 命名空间的迁移种子**默认是“**所有命名空间都塞一份**”，只有在 `detectInstall` 探针**恰好命中一个**安装时才让另一个空起步（`:61-85`）。理由：一个安装在没有去重状态的情况下重解析会双计全部历史，而“有限的回填缺口”是更便宜的失败。
- 异常路径：`parseErr?.code !== "TOKENTRACKER_CURSOR_STORE_RETRY"` 时把 `ns` 写回 `cursors[providerName]`（`:49-54`）。
  【未能确认】该分支的语义看起来是“游标存储重试时不要覆盖命名空间状态”，但从这段控制流无法确定两种写回差别对最终状态的全部影响；卡在 `src/lib/multi-install-parser.js:49-54` 与上层 `multiInstallParse` 调用点的配合，需构造测试用例验证。

`mergeBothFileSources`（`:92-111`）处理 `TOKENTRACKER_WSL_MODE=both`：把同一 `resolveFiles` 分别在 `native-only` 与 `wsl-only` 环境下各跑一次，按去重合并文件列表。

### 6.6 去重四道防线（这是“避免重复计数”的全部答案）

| 层 | 机制 | 位置 |
| --- | --- | --- |
| ① 根去重 | `identityKey` = `realpath` + 分隔符归一 + 去尾斜杠；`appendUniqueDirs` 只对“附加根”去重，**native/WSL 对故意不去重** | `src/lib/scan-roots.js:116-146,204-224` |
| ② 目录去重 | `uniqueSources` 按 `sessionsDir` 去重 | `src/commands/sync.js:864-870` |
| ③ 文件级 | Claude：`fileId = claudeFileIdentityHash(path, st)`，重复者标 `duplicateOf` 并直接置 offset=size | `src/lib/rollout.js:1056-1105` |
| ④ 事件/消息级 | Codex `sessionUUID:eventTimestamp`；Claude `${message.id}:${requestId}`（无 requestId 时退化为 `message.id`） | `src/lib/rollout.js:292-295,4914-4919` |
| ⑤ 同 session 多路径 | `needsHistoricalRolloutDedup` 强制走持久集合 | `src/lib/rollout.js:428-461` |

服务端也有对应保证：ingest 是**按桶键整行 upsert**，因此重放是幂等的（`src/commands/sync.js:249-256` 注释）。

---

## 7. 读取安全：不阻塞、不吃爆内存、不因坏行崩

### 7.1 读侧增量：`createAsyncQueueReader`（`src/lib/async-queue-reader.js`）

这是 Dashboard 读 `queue.jsonl` 的唯一入口，全部 117 行都值得看。要点：

- **文件签名**用 `dev:ino:size:mtimeNs:ctimeNs`（`bigint` stat，`:8-10,33`）。签名不变 → **直接返回缓存行**：`if (cached?.signature === nextSignature) return cached.rows;`（`src/lib/async-queue-reader.js:35`）；“Dashboard 一次刷新会并发打多个端点，所以这个缓存是必需的”这一设计目的写在 `:12-15` 的注释里（“One in-flight read per path shares the dashboard's request fan-out”）。勿把 `:99-104` 当作命中路径——那里是 `ENOENT` 清理（`:99-100`）与读取失败降级（`:102-103`）。
- **同文件追加时增量续读**（`:40-47`）：条件是同 `dev:ino`、size 变大、且已消费字节 > 0；然后用两段“锚点探针”（头部 256 B + `consumedBytes` 前 256 B）比对，**只有内容一致才从 `consumedBytes` 续读**，否则从头读。这是对“append-only 但可能被原子替换/压缩重写”的双保险。
- **一行一行按 `\n` 切**，但只有**以换行结尾的完整行**才计入 `consumedBytes`（`:64-69`），尾部残行单独放 `tail`，且“**残行的 key 不进已提交 map**”（`:87-91`）——因为写者可能还没写完这一行，行的键值可能变化。
- **每 64 KB 让出事件循环**（`CHUNK_BYTES = 64 * 1024`，`await yieldToRequests()`，`:5,79`），避免冷启动读大队列时卡死 HTTP 事件循环。
- **坏行容错**：`JSON.parse` 失败只 `malformed++`，最后 `console.error` 计数（`:51-57,95`），不影响其它行。
- **同路径并发合并**：`pending` map 让同一文件的并发请求共享同一次读取（`:109-114`）。
- **LRU 上限 4 个文件**（`maxEntries = 4`，`:16,94`）；出错时回退到上一次缓存行（`:102-103`）。

### 7.2 物理 JSONL 解析：`physicalJsonlRecords`（`src/lib/jsonl-lines.js`）

不用 `readline`，理由写在文件头（`:3-11`）：`readline` 会把 U+2028/U+2029 当行分隔符，而它们在 JSON 字符串里是**合法字符**。

| 能力 | 实现 |
| --- | --- |
| 只认 `\n` 为记录分隔符 | `chunk.indexOf(0x0a)`（`:57`） |
| 字节预算 | `maxPhysicalBytes`：超限抛 `PhysicalJsonlLimitError`（`code: "PHYSICAL_JSONL_LIMIT_EXCEEDED"`，`:12-18,61-63,96-98`） |
| 单行上限 | `maxRecordBytes`：超限则**不缓冲不解码**，直接产出 `{line:null, utf8Valid:null, oversized:true, physicalBytes, terminated:true}`（`:65-73`）；末行未闭合的超长行带 `terminated:false`（`:111-114`）。`physicalBytes` 仍计入字节预算（`:61-63,69,96-98`），即超长行**照常推进物理字节预算**，不影响按字节提交的游标 |
| UTF-8 严格性 | `TextDecoder("utf-8", {fatal:true})`；策略 `invalidUtf8: "throw"｜"record"`（`:20-27,34-36,84-90`） |
| CRLF | 由 `decodePhysicalLine` 的默认参数 `stripCr` 处理，但**对调用方不可选**：`physicalJsonlRecords` 对外 opts 只有 `invalidUtf8` / `maxPhysicalBytes` / `maxRecordBytes`（`src/lib/jsonl-lines.js:29-33`），调用点硬编码 `{ stripCr: true }`（`:85`） |
| 末行未终止 | `terminated: false` 标记，交由调用方决定是否重试（`:127`） |

这套 API 的设计目标是“**字节精确预算 + 坏数据可降级但仍可观测**”，与本项目 Rust 侧“异常行计 bad_lines 跳过”的思路同源，但多了一条“超大行零拷贝丢弃”的保护。

### 7.3 上传侧读取：`iterateQueueLines` 与 `readQueueBatch`

- `iterateQueueLines(handle, start, end)`（`src/lib/upload-queue.js:22-43`）同样按 `\n` 切、跨块拼接、显式标注 `complete: false` 的尾行；注释强调“**offset 是字节位置，不是字符长度**，因此必须保留原始字节（含 CRLF）” （`:20-21`）。
- `drainQueueToCloud` 只上传完整行（`if (!record.complete) break;`，`src/commands/sync.js:4449`）；读不出字节时抛 `Queue changed while reading`（`src/lib/upload-queue.js:29`）。
- 单批记录数上限同时约束桶行与 `account_session_state` 行（`linesRead >= maxBuckets`，`src/commands/sync.js:4489,4512`），并在注释里记录了曾经的故障：TRAE 首次同步会产生每会话一条的 state 记录，若不计入上限就会**一次性发出超过 edge 500 上限的请求，永久失败**（`:4433-4441`）。

### 7.4 读时纠错：`normalizeQueueRow`

历史口径修正**不重写队列文件**，而是在读取时纠正（`src/lib/local-api.js:170-200`）：

```js
function normalizeQueueRow(row) {
  let normalized = row;
  if (isLegacyInclusiveCodexRow(normalized)) {
    normalized = { ...normalized,
      input_tokens: Number(normalized.input_tokens || 0) - Number(normalized.cached_input_tokens || 0) };
  }
  const sourceName = String(normalized.source || "").toLowerCase();
  if (sourceName === "cursor" || sourceName === "antigravity") {
    const totalTokens = Number(normalized.total_tokens || 0);
    const billable = Number(normalized.billable_total_tokens || 0);
    if (totalTokens > 0 && billable < totalTokens) normalized = { ...normalized, billable_total_tokens: totalTokens };
  }
  return normalized;
}
```

- “legacy Codex 行 input 含缓存”的判定是一个**不变量**：`cached > 0 && input >= cached && total === input + output`（`isLegacyInclusiveCodexRow`，`:155-168`）。
- Cursor/Antigravity 的 `billable = 0` 是历史上“Included in Pro / no charge”被错误门控的遗留（issue #106），读时抬到 `total_tokens`。
- 同一函数也被**上传路径**复用（`src/commands/sync.js:4509`），保证本地与云端看到同一口径。

### 7.5 其它读取面防护（同类可借鉴）

- Dashboard API 请求体上限：`readBodyLimited(req, maxBytes)`（`src/lib/local-api.js:722-740`）；头像代理 512 KB / 64 条上限（`:46-47`）。
- 本地 API 触发的 sync 有**120s 子进程预算**（`const SYNC_TIMEOUT_MS = 120_000`，`src/lib/local-api.js:22`），超时后 `SIGTERM` 并返回 `code: "SYNC_TIMEOUT"`（`:778-792`）；而 sync 侧为优先级锁预留 90s，正是为了给解析与持久化留 ≥30s（`src/commands/sync.js:295-300`）。
- 本地 API spawn sync 时的环境变量透传是**白名单式**的：只有 `TOKENTRACKER_DEVICE_TOKEN` 存在时才附 base URL/anon key，且始终注入 `TOKENTRACKER_LOCAL_SYNC_DEVICE_TOKEN`（`src/lib/local-api.js:754-762`）。
- serve 的端口策略：默认 `127.0.0.1:7680`，WSL 下默认 `7681`（因为 Windows 的 Delivery Optimization 服务占着 7680 且接受 TCP 后丢弃 HTTP 请求，issue #267），`EADDRINUSE/EACCES/EPERM` 时自动 +1 重试，最多 20 次（`src/commands/serve.js:14-22,679-714`）。**但 +1 回退只发生在默认启动**：显式传 `--port`（或 `PORT`）时 `allowFallback: !opts.portExplicit` 为假、且挂上 `ensurePortFree` 探测器，端口被占即严格失败——打印 `buildPortInUseHint` 后 `process.exitCode = 1`（`src/commands/serve.js:209-224`）。

---

## 8. 存疑与待复核点

按“影响结论可信度”排序。每条给出卡点位置与建议验证方式。

1. **`enqueueTouchedBuckets` 的 `legacyGroups` / `groupQueued` 分支语义未完全验证**。`src/lib/rollout.js:3181-3194,3430-3480` 存在一条“整小时聚合成 `model=DEFAULT_MODEL` 一行”的遗留路径，且 v2→v3 的注释说 `groupQueued` 的存在本身就是 legacy 标记（`src/commands/sync.js:216-220`）。我只确认了“有该键则走聚合分支”，**未验证**新装机会不会写入该键（如果不会，则该分支对现有用户是死代码；如果会，则存在与修复意图冲突的再膨胀路径）。建议：构造一个含 `groupQueued` 的 v1 游标 fixture，跑 `enqueueTouchedBuckets` 观察输出行数与模型名。
2. **崩溃窗口造成的重复行在上传侧的去重强度**。写入是 appendFile，游标提交在其后（§3.4），因此崩溃会留下重复的同键行。读侧与 `compactPendingQueue` 都按“同键最后一条胜出”处理，云端是整行 upsert；但 `readQueueBatch`（`src/commands/sync.js:4422-4512`）在同一批内也是 `bucketMap.set(...)` 覆盖，**未验证**同一批内出现同键但 token 更小的旧行时是否会以旧覆盖新（取决于文件顺序）。建议：构造同键双行（旧在前、新在后）的队列 fixture 直接跑 `readQueueBatch`。
3. **`compactPendingQueue` 与 `queue.state.json` offset 的交互**。代码只重写 `[offset, size)` 区间并原样复制 `[0, offset)`（`src/lib/upload-queue.js:97-110`），理论上 offset 仍有效；但**未验证**“pending 区出现跨行边界残缺（写者正在写）时 `iterateQueueLines` 的行为”以及压缩后 offset 恰好落在文件末尾的边界（`input.close()` 与 `fs.stat` 校验之间有窗口）。
4. **游标存储 `previous` / `rollback` / `current` 三代语义的完整使用矩阵**。我只读了 commit 与读取回退两处；哪些代码路径会真正读到 `previous`（而非只读 `rollback`）**未能确认**——`src/lib/cursor-store.js:974-986` 的注释指向“老版本 v2 读者”，属跨版本兼容，需要对照历史版本代码才能定性。
5. **`multiInstallParse` 异常分支的写回语义**（§6.5 已标）。卡在 `src/lib/multi-install-parser.js:49-54`。
6. **Claude `Stop` 事件的触发粒度**。仓库内只有事件字符串，无语义注释（`src/lib/claude-config.js:7-8`；`src/commands/init.js:724`）；共享情报称 Claude 侧是“每轮 + 会话结束”，但**本地源码无法证实**该断言。
7. **CodeBuddy / WorkBuddy 是否真的消费 `settings.json` 的 `SessionEnd`**。源码以“fork 同 schema”注释断言（`src/commands/init.js:623-625,1034-1036,1048-1049`），无 schema 校验、无第三方验证。
8. **`passive-mode.js` 的覆盖缺口**。它只覆盖 8 个 hook 驱动 provider（`src/lib/passive-mode.js:96-185`）；Grok / OpenCode / OpenClaw / omp 的“hook 缺失但日志在”状态由 `status.js` 的独立路径上报，**未逐一核对**这些 provider 的降级是否也被表达为“passive”（`status.js:1300-1336` 只是各自的 `passive reader (...)` 文案）。
9. **`workbuddy-ai` 的 hook 键名一致性**。`passive-mode.js` 用 `hookStatus?.["workbuddy-ai"]`（带连字符，`src/lib/passive-mode.js:180`），而 `status.js` 传的键确实是 `"workbuddy-ai"`（`src/commands/status.js:1034,1078`）——一致，但 `src/commands/sync.js` 的 source 白名单里同时存在 `workbuddy` 与 `workbuddy-ai`（`:362-363`），**未确认** `parseWorkbuddyIncremental` 是否用 `cursorKey` 或 `source` 区分二者（`sync.js:2639,2674` 传参差异未逐字段核对）。
10. **`usage_precision` 与 `prioritySubsetOf(totals)` 的语义**。它们出现在队列行里（`src/lib/rollout.js:3325-3343`），影响 `queuedKey` 的相等判定（`:3326`）。本文未追究其取值域与消费方，属范围外但会影响“同键覆盖”语义的字段，建议后续研究价格/展示链时补。
11. **`bin/tracker.js` 的代理自举对 hook 触发路径的实际影响**。`applyUndiciProxyIfNeeded` 返回 `unprotected` 时**整个进程 exit(1)**，包括 hook 触发的 sync；这意味着“配了代理但代理挂了”会静默让采集停摆（只有 stderr）。**未验证** status 是否会把这种情况上报为可见故障。
12. **WSL 探测缓存的失效 ——【R1 复核后已结案】结论：生产路径无调用，风险不成立**。`resetWslProbeCache`（`src/lib/wsl-probe.js:11`）在 `src/` 内除定义与导出（`:268`）外**零调用点**，唯一调用者是 `test/*.test.js`。长驻进程内新增发行版不会被发现这一现象依然存在，但既然没有任何生产路径刷新过缓存，也就没有“调用时机不对”的问题；若后续要做缓存刷新，属于新功能而非缺陷修复。
13. **`auto.retry.json` 的清理时机**。`clearAutoRetry` 在“无 pending 或不允许上传”时调用（`src/commands/sync.js:580-582,3661-3662`），但 `publishAccount` 分支与 native 壳自管路径的交互**未逐条验证**。
14. **`init` 的 `--link-code` 实际消费者 ——【R1 复核后已结案】结论：遗留参数**。init.js 内**只**用它关闭浏览器认证（`src/commands/init.js:1423,339`），`link_code_state.json` 在 init.js 内**无任何读写**（`:176,234,359` 仅为声明与传参）；全仓在 `src/`、`dashboard/` 内除该三处与 `src/lib/runtime-config.js:149`（清理清单）外**零引用**。可判定为纯遗留参数——即帮助文本（`src/cli.js:78,92`）对该开关的暗示存在误导。
15. **`deferred-codex-audit.json` 与 `codexColdScanAudit` 的双写一致性**。前者在“跳过游标提交”时累加（`src/commands/sync.js:3456-3460`），后者在审计记录时写入（`:3785-3819`），两处计数何时合并读回只看到 `readDeferredCodexAuditSyncs` 一处调用（`:965-969` 附近），**未完整追踪**合并后的清除路径。
16. **共享情报的行数/规模偏差**（§0.1 已列表纠偏）。若后续分析继续引用旧情报中的行号，会全部错位；建议统一以本文件实测行号为准。
17. **`status.js` 的 hook 状态传参完整性**。`detectPassiveProviders` 收到的 `hookStatus` 含 11 个键（`src/commands/status.js:1024-1036`），其中 `opencode_plugin`/`openclaw_session_plugin` 并不在 passive-mode 的 provider 列表内（`src/lib/passive-mode.js:96-185`），属**冗余传参**；反之 `passive-mode.js` 需要 `workbuddy-ai` 而它的键名风格与其它下划线键不一致，属潜在命名陷阱（当前一致，见第 9 条）。
18. **未覆盖范围声明**：本文不含逐 provider 解析算法、定价、Dashboard 渲染、云端 schema、macOS/Windows/Linux 壳的调度细节（`TokenTrackerBar/`、`TokenTrackerWin/`、`TokenTrackerLinux/` 三目录未读）。其中 Windows 壳的“5 分钟后台 sync 计时器”（`src/commands/serve.js:305-309` 注释提及）**未在壳代码中核实**，属明显缺口。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-A-pipeline.md`（样本 HEAD `b19d27b0` 未变）。本节只记录**改动**，未改动处即视为复核通过。修正 agent：FIX-A，所有改动均回读源码逐条确认后落笔。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| E1（§0.1） | 实测 `src/` 下共 **128 个文件** | **112 个文件**，并注明口径为「仓库跟踪文件数」 | `git ls-files src` = 112；`Get-ChildItem -Recurse -File -Force src` = 112 |
| E2（§2.3） | notify.cjs 模板 `init.js:1445-1574` | 模板区间 **`:1453-2231`**，宿主 `buildNotifyHandler` **`:1445-2232`**；补模板内辅助函数行号 | `init.js:1445,1453,1575,1583,1597,1609,1748,1811,2231,2232` |
| E3（§2.8） | 「哨兵范式（omp / **OpenCode**）」；引 `opencode-config.js:8,107-109` | 哨兵范式**只有 omp**（`omp-hook.js:17,388-395`，字符串全仓唯一产生点，消费方 `init.js:813`）；OpenCode 独立成「**内容等值短路 + 无条件备份覆盖**」条目（`:66-87`、`:73-75`、`:80-85`、`:94`） | `omp-hook.js:17,388-395`；`opencode-config.js:66-96`；`init.js:813` |
| E4（§3.4） | 写队列**只有一处** `fs.appendFile`（`rollout.js:3483`） | 主队列 append 写点**至少 6 处**（`rollout.js:3483`/`:20031`/`:22594`，`sync.js:5126`/`:5207`/`:5281`），另 `sync.js:4114` 写 fallback、`rollout.js:3536` 写 project 队列；**保留**“无独立写侧互斥、全部假定调用方持有 `sync.lock`”结论 | 全仓 `appendFile` 命中 9 处，逐一核对 |
| E5（§4.4） | 「Gemini / Kilo 等」记 `mtimeMs + lastIndex + lastTotals`（`:1289-1307`、`:1462-1516`） | 拆成三套：**KiroCli** `mtimeMs+lastIndex+lastTotals`（`:9371-9375`、`:9451-9471`）、Gemini（`:1289-1307`）、**opencode** 游标为 `inode/size/mtimeMs/lastTotals/messageKey` 且**无 `lastIndex`**（`:1462-1516`） | `rollout.js:1498,1513-1520,9371-9375` |
| E6（§7.1） | 「签名相同即返回缓存」引 `:99-104` | 命中在 **`:35`**，并发目的注释在 **`:12-15`**；`:99-104` 明确标注为 ENOENT 清理与失败回退 | `async-queue-reader.js:12-15,35,99-104` |
| E7（§6.3） | 「非 win32 平台一律只返回 native（`:149`）」 | `wsl-probe.js:149` 返回 `{native:null, wsl:null}`（发现层不做 WSL 探测）；「只返回 native」的落点是 **`install-resolver.js:21-23`** | `wsl-probe.js:149`；`install-resolver.js:21-23` |
| E9（§3.5） | `skipNoOpCursorCommit` 判据列举为 9 项 | 条件项 **13 个**，逐项列出（含 `!codexColdAuditDue`、`!codexFallbackRetryRan`、`opts.auto`、`totalParsed===…` 等） | `sync.js:3424-3437` |
| I1（§3.1） | 拿不到锁「写 `sync.skip.json` 并**静默 return**」 | 改为「不抛错：写 marker **并向 stderr 输出 `Sync skipped: …`**」（表内第 3 步与锁策略小节同步改） | `sync.js:555-558` |
| I2（§2.3） | 「双路径」判据叙述 | 指明模板内 `:1483`/`:1484` 路径**字面相同**，判据退化为一次 `existsSync`；同 quirk 见 `openclaw-session-plugin.js:358,361` | `init.js:1483-1484`；`openclaw-session-plugin.js:358,361` |
| I3（§4.7） | `codexColdScanAudit` 列 5 个字段 | **6 个**：补 `lastFullScanAt`（ISO 文本副本） | `sync.js:3810,3813-3815` |
| I4（§3.4） | 原子替换「**只在两处出现**」 | 改为「append 与原子重写**两条写路径，各自多个实现点**」 | 同 E4；`upload-queue.js:64-132` |
| I6（§7.2） | CRLF「可选 `stripCr`（`:22-27`）」 | `stripCr` **对调用方不可选**：对外 opts 只有 `invalidUtf8/maxPhysicalBytes/maxRecordBytes`，调用点硬编码 `{stripCr:true}` | `jsonl-lines.js:29-33,85` |
| I8（§8 第 12/14 条） | 两条「未确认」 | 均**结案**：`resetWslProbeCache` 生产调用为 0（仅定义 `wsl-probe.js:11` 与导出 `:268`）；`link_code_state.json` 除 `init.js:176,234,359` 与 `runtime-config.js:149` 外零引用 → 判为**遗留参数** | `wsl-probe.js:11,268`；`init.js:176`；`runtime-config.js:149` |
| I9（§4.8） | 守卫列表末尾述及 `:6745` | 补【存疑】一句：**其它 repair 的“保留不可解析行”语义未逐条核对**，`sync.js:6844,7147` 亦未逐行打开 | `sync.js:6745`（属 Droid 修复） |

未在本文件落实、留给 S1b 或不适用本文件的项：E8（`cloneOrCopy` 非硬链接）、I5（`needsHistoricalRolloutDedup` 兜底两条）、I7（deferred 提示受 `!opts.auto` 约束）——三条均只影响 S1b 对应段落，已在 [S1b-worth-copying.md](./S1b-worth-copying.md) 修正；U 组 10 条按“无本仓库证据”统一处理，见 S1b 末尾说明。

### R2 修正

依据 `REVIEW-R2-1-pipeline-framework.md`（E1/E2/E3 与 I1–I7）与 `REVIEW-R2-2-providers.md`（其中 E10 为跨分片的 S1 侧问题）。修正 agent：FIX-R2-A；所有改动均回读源码确认；**样本仓库未做任何修改**；R1 已完成的内容全部保留，本次只做收窄、补漏与算术闭合。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| R2-E3（§4.4） | opencode 游标字段列为 `inode/size/mtimeMs/lastTotals/messageKey` | 补 **`opencodeForkRepairVersion`**（恒写 `1`），并说明它参与“文件未变”判定（`:1465` 要求 `=== 1`）——**旧代游标缺此键会被重判为已变并重扫一次**，之后才回稳态短路 | `rollout.js:1460-1465,1513-1521` |
| R2-E10（§2.4 补充事实） | 「写配置 12 + OpenClaw legacy 移除 1 分支 + 仅探测 1 + 纯被动读取 17 + 仅欢迎名单 15」= **46 ≠ 45** | 拆解改为 **45 = 写配置 12 + 仅探测 1 + 纯被动读取 17 + 仅出现在欢迎名单 15**：OpenClaw 的 legacy hook 移除（`:1120-1128`）是**同一 provider** 的附加清理分支，不单列为第 5 类；同时补出写配置 12 个的逐项行号与「仅欢迎名单」的 15 个名单（Antigravity / Kiro / Hermes Agent / GitHub Copilot / OmO / Dots / Roo Code / Zed Agent / Goose / Droid / ZCode / Qoder / Claude Science / DeepSeek Harness / TRAE Work CN） | `init.js:119-165`（45 项）；逐分支 `:686-702,704-720,722-731,733-742,744-756,789-828,901-916,1037-1046,1050-1059,1062-1071,1073-1118,1130-1146`（写配置 12）；`:758-776`（Cursor）；`:784-786,833-840,843-848,851-856,860-865,870-875,878-883,886-899,921-928,930-935,940-945,948-957,959-968,971-980,983-992,997-1008,1014-1032`（纯被动 17） |
| R2-I1（§4.4） | KiroCli「游标读写见 `:9371-9375`、`:9451-9471`」 | 拆为**读取** `:9371-9375`（含 `:9376` 的 `mtimeMs <= prevMtime` 早退）与**写回** `:9468-9472`（`{ mtimeMs, size, lastIndex }`）；`:9451-9467` 是桶累加与 `onProgress` 段；`fileOffsets` 在 `:9482` 并入 `cursors.kiroCli`。R1 修正记录（E5/A05）里的 `:9451-9471` 以本条为准 | `rollout.js:9371-9376,9451-9472,9482` |
| R2-I5（§7.2） | oversized 记录写作 `{line:null, oversized:true}` | 补齐为 `{line:null, utf8Valid:null, oversized:true, physicalBytes, terminated:true}`（末行未闭合的超长行是 `terminated:false`），并写明 `physicalBytes` 仍计入字节预算——**超长行照常推进按字节提交的游标** | `jsonl-lines.js:61-63,65-73,96-98,111-114` |
| R2-I6（§7.5） | 「`EADDRINUSE/EACCES/EPERM` 时自动 +1 重试，最多 20 次」 | 补前提：**+1 回退只在默认启动发生**；显式 `--port`/`PORT` 时 `allowFallback: !opts.portExplicit` 为假并挂 `ensurePortFree`，端口被占即严格失败（`buildPortInUseHint` + `exitCode = 1`） | `serve.js:209-224` |
| R2-I7（§5.2） | 「四个设计点」 | 补第 5、6 点：**`inFlight` 单飞防重入**（上一轮未退出不重复 spawn）、**关停路径**（`SIGINT/SIGTERM` → `stop()` → `server.close` 后 3s 兜底强退） | `serve.js:281-288,332-334` |

**未采纳 / 反驳**：0 条——R2-1 的 E 组（本文件相关 E2/E3）与 I1/I5/I6/I7 经本人回源核实**全部成立**并已落实；R2-2 的 E10 经逐 provider 重数后确认原拆解多计 1 项，已按上述口径闭合。S1 无 R2 条目被判不成立。
