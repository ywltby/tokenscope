## 1. 范围与方法

### 1.1 研究对象与版本

| 项 | 值 |
| --- | --- |
| 仓库 | `https://github.com/xiufengsun/TokenTracker` |
| 本地样本 | `C:\Users\admin\AppData\Local\Temp\tt-research` |
| HEAD | `b19d27b0ccd59ce621f4fa17207b06358e67987c`（tag `v1.3.14`，工作树干净） |
| npm 包名 | `tokentracker-cli` |
| 交付形态 | Node.js CLI（CommonJS，`src/`）+ React Dashboard（`dashboard/`）+ macOS Swift 壳（`TokenTrackerBar/`）+ Windows .NET 壳（`TokenTrackerWin/`）+ Linux Tauri 壳（`TokenTrackerLinux/`） |
| 研究方式 | 只读：`read` / `Select-String` / `grep` / `glob` / `Get-Item`；未修改样本仓库、未执行仓库内脚本、未 `npm install`；覆盖采集与调度全链路、解析骨架与队列契约（三个原生壳内部的调度实现不在范围内） |

规模实测（口径「仓库跟踪文件数」）：`git ls-files src` = 112，`Get-ChildItem -Recurse -File -Force src` 同为 112。采集直接相关的核心模块：`rollout.js`（942,266 字节，全部 provider 的 `parse*Incremental`）、`local-api.js`（156 KB）、`session-analytics.js`（138 KB）、`cursor-store.js`（62 KB，游标持久化）、`fs.js`（15 KB，锁与原子写）、`upload-queue.js`、`async-queue-reader.js`、`jsonl-lines.js`、`scan-roots.js`、`wsl-probe.js`、`install-resolver.js`、`multi-install-parser.js`；`src/lib/pricing/` 另含 `index.js`、`matcher.js`、`litellm-fetcher.js`、`seed-snapshot.json`、`curated-overrides.json`。

### 1.2 证据分档

全文结论按三档标注，**不跨档引用**：

- **已验证**：在样本源码中直接读到并逐字核对（带 `相对路径:行号`），或由全库检索的命中／零命中闭合（例如「全库 `bad_lines|badLines|bad_line` 在 `src/lib/` 下零命中」）。可直接引用。
- **一致解释**：单点源码中没有一句断言，但多处**相互独立**的证据指向同一解释且未见反例。典型例证是 0.26.3 数据丢失事故：结论由「源码注释对事故的原文记录」与「事故之后新增的守卫代码」两类证据共同支持（`src/commands/sync.js:234-248`、`:258-268`）。
- **未验证**：须运行代码、须真实语料或须打开未覆盖目录才能判定。此类内容保留 `【未能确认】` 标记并给出判定条件（缺什么才能定）。

凡档位存在争议的按较低档写；本文不把任何 `【未能确认】` 升级成结论。

### 1.3 行号口径：PowerShell 的 GBK 偏移坑

样本源码含中文注释，其中 **15 行以 ≥0x81 的字节结尾**（行尾是多字节字符的末字节）。PowerShell 5.1 的 `Get-Content`（不指定编码）按 GBK 解码，把这些行尾字节与其后的 `0x0A` 一起吞掉，导致**总行数少 15，且从第一个此类行之后的行号整体偏移（累计最多 15 行）**。以 `src/lib/rollout.js` 实测：

| 取行数口径 | 结果 |
| --- | --- |
| `read` 工具（UTF-8/LF，**权威**） | 24932 |
| `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8).Count` | 24932 |
| `(Get-Content $p -Encoding UTF8).Count` | 24932 |
| `(Get-Content $p).Count`（默认编码，**错**） | 24917 |
| `(Get-Content $p \| Measure-Object -Line).Lines`（**错**，且忽略空行） | 23260 |

正确核对方法（任选）：`read` 工具的 `offset`/`limit`；`[System.IO.File]::ReadAllLines(path, [System.Text.Encoding]::UTF8)` 后按 0-based 下标取行；`Select-String -Path <file> -Pattern <pat> -Encoding UTF8`。**用默认编码 `Get-Content` 数出的行号都会错位**，核对前先用上表自检。

与之相关的规模纠偏——先前流转的数字与实测不符，全文以实测为准：

| 文件 | 先前数字 | 实测 |
| --- | --- | --- |
| `src/commands/sync.js` | 6900 行 | **7251 行** / 288,151 字节 |
| `src/lib/rollout.js` | 23260 行 | **24932 行** / 942,266 字节 |
| `src/commands/status.js` | 1647 行 | **1729 行** / 76,611 字节 |
| `src/cli.js` | 100 行 | **107 行** / 4,060 字节 |
| `src/commands/init.js` | 82654 字节 | 82,654 字节一致，**2427 行** |
| `bin/tracker.js` | 100 行 | **60 行** |

### 1.4 数据来源限定

- 全文结论**仅来自样本仓库源码的只读阅读**；未运行仓库内任何脚本、未安装依赖、未修改样本仓库任何文件。
- **未读取任何真实会话数据**：本机 `~/.claude/projects/**`、`~/.codex/sessions/**` 及各 provider 的 SQLite 库均未纳入证据。凡须真实语料才能定的语义（例如某字段在真实日志中的出现率）一律进 `【未能确认】`。
- 明确不在本文范围：逐 provider 的解析算法细节（Claude Code 与 Codex 两条主线除外，仅骨架层）、定价计算、Dashboard 前端渲染、云侧 ingest schema、三个原生壳。

---

## 2. 总体架构与数据流

### 2.1 五种交付形态

| 形态 | 入口/位置 | 职责 | 依据 |
| --- | --- | --- | --- |
| Node CLI | `bin/tracker.js`（60 行）+ `src/cli.js`（107 行）+ `src/commands/*` | 全部真实工作：init 装 hook、sync 采集上传、status 上报、serve 起服务 | `bin/tracker.js:5-59`、`src/cli.js:12-66` |
| 本地 Dashboard 服务 | `src/commands/serve.js` + `src/lib/local-api.js` | 托管 React 产物与 SPA 回退、暴露本地 HTTP API；首次启动自动 `init --yes`；Windows 上跑 60s 后台轮询 | `src/commands/serve.js:69,80-90,257-260` |
| React Dashboard | `dashboard/` | 展示层；只从本地 API 读队列行，不参与解析 | `src/lib/local-api.js:202-211` |
| macOS Swift 壳 | `TokenTrackerBar/` | 自管唤醒循环（决定 serve 侧 60s 轮询是否启用） | 注释见 `src/commands/serve.js:248-256`；壳代码本轮未读 |
| Windows .NET 壳 | `TokenTrackerWin/` | 托盘宿主，自带 5 分钟后台 sync 计时器；被托管时 serve 侧轮询让位 | 注释见 `src/commands/serve.js:305-309`；**壳内计时器【未能确认】** |
| Linux Tauri 壳 | `TokenTrackerLinux/` | 壳层交付 | 实现细节本轮未读 |

Windows 侧一条平台特殊性：服务默认监听 `127.0.0.1:7680`，而 Windows 的 Delivery Optimization 服务占用 7680 并会接受 TCP 连接后丢弃 HTTP 请求，于是 WSL 下默认改用 `7681`；`EADDRINUSE/EACCES/EPERM` 时自动 +1 重试、最多 20 次（`src/commands/serve.js:14-22,679-714`）。**但 +1 回退只发生在默认启动**：显式传 `--port`（或 `PORT`）时 `allowFallback: !opts.portExplicit` 为假、并挂 `ensurePortFree`，端口被占即严格失败——打印 `buildPortInUseHint` 后 `process.exitCode = 1`（`src/commands/serve.js:209-224`）。

### 2.2 分层

| 层 | 位置 | 职责 |
| --- | --- | --- |
| 发现 | `scan-roots.js` / `install-resolver.js` / `wsl-probe.js` / `rollout.js` 的 `list*` 家族 | 产出「根列表 → 安装选择 → 候选文件列表」 |
| 解析 | `rollout.js` 的 45 个 `parse*Incremental` | 逐文件增量读，产出事件并累加进桶 |
| 游标/桶 | `cursors.files[*]`、`cursors.hourly.buckets`、`cursor-store.js` | 记住读到哪、已计过什么、每个半小时桶的累计值 |
| 队列 | `queue.jsonl` / `project.queue.jsonl`（追加）+ `upload-queue.js`（压缩） | 桶 → 整行累计快照；本地与云端各一条通道 |
| 读取/聚合 | `async-queue-reader.js` / `local-api.js` | 增量读队列、同键取最后一条、按天跨模型求和 |
| 计价 | `src/lib/pricing/*` | 队列行内已带 `total_cost_usd`；计价输入是五列之和，`total_tokens` 不参与计费（`src/lib/pricing/index.js:248-256`） |
| 展示 | `dashboard/` | 只消费本地 API 的输出 |

### 2.3 一条完整的数据流

```
provider CLI 会话日志（~/.claude/projects/**、~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl、各 provider 的 SQLite…）
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
三个必须记住的顺序事实：**hook 触发的 sync 是单源扫描**，手动 `sync` 才是全源（`resolveAutoSourceScope`，`src/commands/sync.js:3747-3753`）；**队列写入在游标提交之前**（`src/lib/rollout.js:3483` 先 append，`src/commands/sync.js:3444-3446` 后 commit），崩溃会留下重复同键行，幂等由读侧与云端兜住（§4.7）；**游标每轮只提交一次**，且在全部 provider 解析完成之后。另外，**项目队列不进上传通道**：`pendingBytes` 只按主队列算（`src/commands/sync.js:3654-3659`）。

### 2.4 目录布局与文件清单

数据目录由 `src/lib/tracker-paths.js:4-12` 决定：

```js
async function resolveTrackerPaths({ home = os.homedir() } = {}) {
  const rootDir = path.join(home, ".tokentracker");
  return { rootDir, trackerDir: path.join(rootDir, "tracker"),
           binDir: path.join(rootDir, "bin"), cacheDir: path.join(rootDir, "cache") };
}
```

**易错点：队列、游标、锁都在 `~/.tokentracker/tracker/` 下，不在根目录**（`src/commands/sync.js:568-573` 显式拼接 `trackerDir`）；`bin/`（notify.cjs）与 `cache/`（定价快照）是它的兄弟目录。

```
~/.tokentracker/
├── app/bin/tracker.js   # runtime 拷贝（init 写入；hook 优先跑这份）
├── bin/notify.cjs       # init.js 用模板生成的 hook 处理器（非仓库静态文件）
├── cache/               # 定价快照
└── tracker/
    ├── queue.jsonl / queue.state.json   # 主队列 + 云端上传字节 offset（唯一上传对象；project.queue.jsonl 与 project.queue.state.json 是本地项目队列，不上传）
    ├── cursors.json | cursor-store-v2/  # 游标两代（单文件 < 16 MB / manifest + generations 分片）
    ├── deferred-codex-audit.json        # 被跳过的冷扫描审计计数
    ├── sync.lock / sync.lock.heartbeat.*  # 互斥锁与 token 专属心跳
    ├── sync.throttle / notify.signal / sync.skip.json / upload.throttle.json / auto.retry.json  # 节流 / 活动时间 / 抢锁失败 marker / 上传退避 / 重试计划
    └── {codex,acode,code}_notify_original.json | openclaw-plugin/ | openclaw-hook/  # 用户原 notify / OpenClaw 插件与 legacy hook（后者仅探测与移除）
```

一处缺口：任务清单里出现过的 `logs/`，在 `src/` 内**没有任何创建点**（全库检索 `path.join(<trackerDir>, "logs")` 类写入零命中）。它在 `~/.tokentracker/**` 下是否存在取决于本轮未读的三个原生壳——**【未能确认】**；判定条件：打开 `TokenTrackerWin/`、`TokenTrackerBar/`、`TokenTrackerLinux/` 检索日志目录常量。

---

## 3. 采集管线：触发、调度、游标

### 3.1 入口与命令面

`bin/tracker.js` 只有 60 行，职责**不是**命令分发，而是三件进程级准备：剥离 `--debug` 并落成 `TOKENTRACKER_DEBUG`（`bin/tracker.js:5,12-13`）；**代理自举与「代理失效则中止」**；统一错误出口（`:42-59`，非 debug 只打印 stack，debug 追加 `status`/`code`/`originalMessage`/`nextActions`，最后 `process.exitCode = 1`）。

```js
// bin/tracker.js:15-40（节选）
const applyResult = applyUndiciProxyIfNeeded({ proxyConfig: persistedProxy });
if (applyResult && applyResult.unprotected === true) {
  console.error('[proxy] Manual proxy could not be applied and outbound traffic could not be blocked; aborting.');
  console.error('[proxy] 手动代理无法生效且无法阻断出站流量，已中止。');
  process.exit(1);
}
```

这一条直接改变采集语义：**代理配置错误会让整个 CLI 硬失败**，包括 hook 触发的 sync，而不是静默降级为「只采不上传」。由于 hook 子进程的 stderr 被 `stdio: 'ignore'` 丢弃（`src/commands/init.js:1563-1573`），失败现场只剩「数据不更新」这一个可观察症状。【未能确认】`status` 是否会把这种情况上报为可见故障——`src/` 内未见针对该 exit 路径的状态标记。

`src/cli.js` 是**无语法的静态 switch**，无 commander/yargs（`src/cli.js:12-66`）：

```js
async function run(argv) {                                             // src/cli.js:12-66
  const [command, ...rest] = argv;
  if (!command) { await cmdServe(argv); return; }                       // 无参即 serve
  if (command === "-h" || command === "--help") { printHelp(); return; }
  if (command === "-v" || command === "--version") { console.log(`v${require("../package.json").version}`); return; }
  switch (command) {
    case "serve": await cmdServe(rest); return;
    // init / sync / status / diagnostics / doctor / uninstall / device-login / wrapped / sessions …
    default: throw new Error(`Unknown command: ${command}`);
  }
}
```

要点：**无参即 `serve`**（产品默认路径）；`parseArgs` 在各子命令内部实现，未知选项抛 `Unknown option: xxx`（`src/commands/sync.js:3742`）；`--debug` 被入口剥离，因此可放在子命令前或后任意位置（`src/cli.js:77-86`）。

| 子命令 | 实现 | 采集相关性 |
| --- | --- | --- |
| （无参）/ `serve` | `src/commands/serve.js:69` | 高：首次启动自动 `init --yes`（`:80-90`），`--no-sync` 跳过启动同步（`:106-115`），Windows 上起 60s 轮询（`:257-260`） |
| `init` | `src/commands/init.js` | 高：写 hook、写 notify.cjs、拷 runtime |
| `sync` | `src/commands/sync.js:516` | 核心 |
| `status` | `src/commands/status.js` | 高：hook 状态、passive 模式、游标摘要、pending bytes、`last_sync_skipped` |
| `diagnostics` / `doctor` | `src/commands/diagnostics.js`+`lib/diagnostics.js`；`doctor.js`+`lib/doctor.js` | 中：导出诊断与环境自检 |
| `uninstall` | `src/commands/uninstall.js:76-137` | 中：摘 hook、还原 notify、`--purge` 清数据 |
| `device-login` / `wrapped` / `sessions` | 各自文件 | 低：设备配对 / 年度视图 / 会话效率导出 |

`--help` 文本本身就是一份行为说明书，其中三行与采集直接相关（`src/cli.js:68-105`）：**目录存在性即安装判定**（`Every Code notify installs when ~/.code/config.toml exists.`）、**自动同步受 device token 约束**（`auto sync waits for a device token.`）、**Codex 系解析基于 `rollout-*.jsonl` 命名约定**（`sync parses ~/.codex/sessions/**/rollout-*.jsonl and ~/.code/sessions/**/rollout-*.jsonl, then uploads token deltas.`）。

### 3.2 hook 安装：三层结构、四类幂等范式、notify.cjs 行为

触发设计是**三层解耦**：① 命令字符串构造 `src/lib/notify-command.js`（38 行，按平台生成**转义后**的 `<node> <notify.cjs> --source=<name>`）；② 各 provider 的 hook 文件读写（`claude-config.js` / `gemini-config.js` / `codex-config.js` / `grok-hook.js` / `omp-hook.js` / `openclaw-hook.js` / `opencode-config.js`，负责幂等写入、移除与探测）；③ 运行时处理器 `~/.tokentracker/bin/notify.cjs`（由 `init.js` 生成，非仓库文件）。

平台差异集中在 8 行里（`src/lib/notify-command.js:13-19`）：

```js
function buildNotifyCommand(notifyPath, source, { platform = process.platform, execPath = process.execPath } = {}) {
  if (!/^[a-z0-9-]+$/i.test(source)) throw new Error("Invalid notify source");
  if (platform === "win32") return `& ${powershellLiteral(execPath)} ${powershellLiteral(notifyPath)} --source=${source}`;
  return `/usr/bin/env node ${quotePosix(notifyPath)} --source=${source}`;
}
```

Windows 用 PowerShell 调用运算符 `& '…' '…'`（`powershellLiteral` 把 `'` 转义为 `''`，`src/lib/notify-command.js:3-5`），所以 hook 项还要附加 `"shell": "powershell"`（`src/lib/claude-config.js:176-179`）；POSIX 用 `/usr/bin/env node` 并经 `quotePosix` 转义（`:7-11`）。`source` 的白名单正则是**跨 provider 复用同一 notify.cjs 的唯一区分手段**。

`notify.cjs` 的行为（模板区间 `src/commands/init.js:1453-2231`，宿主 `buildNotifyHandler` 为 `:1445-2232`；落盘见 `:425-435`）：① 解析 `--source`，其余参数保留为 `payloadArgs` 透传给用户原 hook（`:1461-1476`）；② **落 signal 文件** `~/.tokentracker/tracker/notify.signal`（`:1448,1496-1498`）——即使后续节流拦掉 spawn，也留下「有过活动」的证据；③ **20 秒节流 + 脱离进程 spawn**（下段）；④ **链式调用用户原 notify**（从 `codex_notify_original.json` / `acode_notify_original.json` / `code_notify_original.json` 读回被覆盖前的命令并 spawn，`:1538-1559`；Claude/Gemini/OpenCode/CodeBuddy/WorkBuddy 分支显式不链式，`:1540-1547`，因为它们不共享 `notify` 这一独占槽位）；⑤ **无条件 `process.exit(0)`**（`:1561`）——hook 永不因本工具失败而失败；⑥ 可选调试：`TOKENTRACKER_NOTIFY_DEBUG=1` 时向 `notify.debug.jsonl` 追加 `{ts, source, cwd}`，默认上限 1 MB（`:1488-1517`）。

```js
// src/commands/init.js:1519-1536（模板生成的内容，节选）
const throttlePath = path.join(trackerDir, 'sync.throttle');
const now = Date.now(); let last = 0;
try { last = Number(fs.readFileSync(throttlePath, 'utf8')) || 0; } catch (_) {}
if (now - last > 20_000) {
  try { fs.writeFileSync(throttlePath, String(now), 'utf8'); } catch (_) {}
  const syncArgs = ['sync', '--auto', '--from-notify', '--source', source];
  const hasLocalRuntime = fs.existsSync(trackerBinPath);
  const hasLocalDeps = fs.existsSync(depsMarkerPath);
  if (hasLocalRuntime && hasLocalDeps) spawnDetached([process.execPath, trackerBinPath, ...syncArgs]);
  else spawnDetached(['npx', '--yes', fallbackPkg, ...syncArgs]);
}
```

判据有一处冗余：`trackerBinPath`（`:1483`）与 `depsMarkerPath`（`:1484`）**字面完全相同**（都是 `path.join(trackerDir,'app','bin','tracker.js')`），该条件实际退化为一次 `existsSync`；同一 quirk 也出现在 OpenClaw 插件模板（`src/lib/openclaw-session-plugin.js:358,361`）。`spawnDetached` 用 `detached: true` + `unref()` + `stdio: 'ignore'`（`:1563-1573`），保证**永不阻塞 provider**。

各 provider 的 hook 落点（`<NODE>` = `process.execPath`，`<NOTIFY>` = `~/.tokentracker/bin/notify.cjs`）：

| Provider | 配置文件 | 事件 | 源码位置 |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/settings.json` | `Stop` **+** `SessionEnd`（两个都装） | 安装 `src/commands/init.js:724-728`；事件常量 `src/lib/claude-config.js:7-16` |
| CodeBuddy / WorkBuddy / WorkBuddy AI | `~/.codebuddy/settings.json`、`~/.workbuddy/settings.json`、`~/.workbuddy-ai/settings.json` | **仅 `SessionEnd`** | `src/commands/init.js:1039-1043,1052-1056,1064-1068` |
| Gemini CLI | `~/.gemini/settings.json` | `SessionEnd` + `matcher` | `src/commands/init.js:735-739`；matcher `src/lib/gemini-config.js:8-10` |
| Codex CLI | `~/.codex/config.toml` 顶层 `notify` 数组 | turn 结束（notify 回调） | `src/commands/init.js:689-694`；写入 `src/lib/codex-config.js:271-309` |
| acode / Every Code | `~/.acode/config.toml` / `~/.code/config.toml` | 同 Codex notify | `src/commands/init.js:707-712,1133-1138` |
| Grok Build | `~/.grok/hooks/99-tokentracker-usage.json` | `SessionEnd` | **不经过 notify.cjs**，用专用 handler（`src/commands/init.js:906`、`src/lib/grok-hook.js:307-317`） |
| oh-my-pi（OMP） | `<ompAgentDir>/extensions/tokentracker-notify.ts` | `turn_end` + `agent_end` + `session_shutdown` | 扩展内 spawn，自限速 12s（`src/lib/omp-hook.js:16-17,66,135-143`） |
| OpenClaw | 插件目录 + `~/.openclaw/openclaw.json` 注册 | `agent_end` + `gateway_start` + `gateway_stop` | `src/lib/openclaw-session-plugin.js:37-84,363-399` |
| OpenCode | `<configDir>/plugin/tokentracker.js` | `session.updated` | `src/lib/opencode-config.js:7-9,11-22` |
| Cursor | **不写任何配置** | 无 hook（走 API 拉取） | 仅探测 `src/commands/init.js:758-776` |

provider 总名单是 `SUPPORTED_PROVIDERS`（**45 项**，`src/commands/init.js:119-165`），按 `applyIntegrationSetup`（`:674-1149`）的处理方式拆解为 **45 = 写配置 12 + 仅探测 1 + 纯被动读取 17 + 仅出现在欢迎名单 15**。写配置 12：Claude Code、Codex CLI、AStudio、Every Code、Gemini CLI、CodeBuddy、WorkBuddy、WorkBuddy AI、Grok Build、oh-my-pi、OpenCode、OpenClaw session plugin（逐项行号 `:686-702,704-720,722-731,733-742,744-756,789-828,901-916,1037-1046,1050-1059,1062-1071,1073-1118,1130-1146`）。仅探测 1：Cursor（`:758-776`，只报 detected/skipped）。纯被动读取 17：Kimi Code、pi、Prime Agent、MiniMax Code、Command Code、Craft Agents、Reasonix、TRAE、Kilo CLI、DuMate、Mimo、LM Studio、Unsloth Studio、AnythingLLM Desktop、Devin CLI、Kilo Code VS Code 扩展、Cline（`init` 只打印 detected/reader，不装 hook）。仅欢迎名单 15：Antigravity、Kiro、Hermes Agent、GitHub Copilot、OmO、Dots、Roo Code、Zed Agent、Goose、Droid、ZCode、Qoder、Claude Science、DeepSeek Harness、TRAE Work CN（在 `applyIntegrationSetup` 内**没有任何分支**）。OpenClaw 的 legacy hook 移除（`:1120-1128`）是**同一个 provider** 的附加清理动作，不构成独立类别；`12 + 1 + 17 + 15 = 45`，与 `SUPPORTED_PROVIDERS` 项数闭合。

**幂等范式（四类 + 一例外）**：

1. **Claude 范式**：`hasHook` 结构性比对（command + `shell` 一致性，`src/lib/claude-config.js:143-156`）；已存在即完全不写盘（`if (!changed) return {changed:false}`，`:39`）；`normalizeEntriesForCommand` 就地修复旧形态并按 matcher scope 去重（`:181-230`）。
2. **Codex 范式**：TOML 数组严格等值比较（相等即 `changed:false`，`src/lib/codex-config.js:76,107`）；「是否我们写的」用 `isManagedNotifyCmd`——数组内存在以 `notify.cjs` 结尾且路径含 `/.tokentracker/` 的项、且尾参一致（`:40-58`）。TOML 是**手工行编辑**：`extractNotify` 只找首个表格头之前的 `notify =` 行（`:245-269`），`setNotify` 原位置替换、找不到则插到第一个 `[table]` 前（`:271-309`）。
3. **哨兵范式（全仓只有 omp）**：扩展文件首写标记串 `// @tokentracker-managed-omp-extension`（`src/lib/omp-hook.js:17`）；**带标记 → 覆盖，无标记 → 拒绝写入**（`skippedReason: "unmanaged-extension-present"`，`:388-395`，该字符串全仓仅此一处产生，消费方 `src/commands/init.js:813`）。
4. **内容等值短路 + 无条件备份覆盖（OpenCode）**：`upsertOpencodePlugin` 先比对内容，`if (existing === next) return { changed: false }`（`src/lib/opencode-config.js:73-75`）；内容一旦不等，无论原文件是什么都**先备份再覆盖**（`:80-85`），**没有**「无标记则拒绝写入」的分支；标记检测只出现在卸载路径，内容不匹配时返回 `skippedReason: "unexpected-content"`（`:94`）。**OpenCode 不属于哨兵范式**。
5. **例外（Grok）**：不做任何幂等判断，每次 init 都重写 hook JSON 与 handler，`upsertGrokHook` 恒返回 `changed:true`——理由是「bin 路径可能变化，始终覆盖以保持命令最新」（`src/lib/grok-hook.js:85-87,307-317`），且**没有备份**（`:85-91`）。

备份策略：Claude/Gemini 复制 `settings.json.bak.<ISO>`（`src/lib/claude-config.js:248-262`、`src/lib/gemini-config.js:265-279`）；Codex/acode/every-code 复制 `config.toml.bak.<ISO>` 并**先**把原 notify 存到 `<trackerDir>/{codex,acode,code}_notify_original.json`（`src/lib/codex-config.js:89-104`）；OpenCode 备份旧插件（`src/lib/opencode-config.js:79-83`）；omp / OpenClaw 靠哨兵串与 inode 校验保护（`src/lib/omp-hook.js:222-281`、`src/lib/openclaw-session-plugin.js:154-178`）。回滚全部集中在 `uninstall`（`src/commands/uninstall.js:76-137`），还原时用 `isManagedNotifyCmd` 把关：不是我们写的 notify 返回 `current-not-managed` 且不还原（`src/lib/codex-config.js:110-133`）。

触发粒度差异（直接决定新鲜度语义）：**Codex 系（codex/acode/every-code）走 `notify` 回调**，逐 turn 触发，这条路径**独占用户的 `notify` 槽位**，所以必须备份原 notify 并链式调用。**Claude 系走 `hooks.Stop` + `hooks.SessionEnd`**，不争槽位，但 `Stop` 的触发粒度在仓库内只有事件字符串、没有语义注释（`src/lib/claude-config.js:7-8`），本地源码判不出它是否严格等于「每轮」——**【未能确认】**。**Gemini 走 `SessionEnd` + matcher**，默认 matcher 是 `exit|clear|logout|prompt_input_exit|other`（`src/lib/gemini-config.js:10`），**只有特定退出原因才触发**，且安装时会强制 `tools.enableHooks = true`（`:147-152`），即会改动用户的工具开关。**OpenCode 的注释明确说明**「opencode 行本来就能从磁盘 storage 与 opencode.db 被动解析，hook 只是让 sync 更早发生」（`src/lib/opencode-config.js:55-56`）。

安装失败的降级（`passive mode`）：判定三条件为「hook 期望、hook 未装上、日志在」（`src/lib/passive-mode.js:189-204`）；`dirHasFile` 只做**至多一层**浅递归且把 readdir 失败当 false 吞掉（`:49-68`）；失败原因只区分 `ENOENT`/`EACCES`/`EROFS` 与其它（`:70-81`）；覆盖 8 个 hook 驱动 provider（claude、gemini、codex、acode、every_code、codebuddy、workbuddy、workbuddy-ai，`:96-185`），Grok/OpenCode/OpenClaw 不在名单内。**关键澄清**：`passive-mode.js` 与 `init.js` 之间**没有任何代码调用关系**——全仓引用它的只有 `status.js`（`status.js:44` require、`:1021` 调用、`:1248` 输出、`:1448,1688` 呈现），所以它只是 status 层的补偿性检测与告知，init 不会因 hook 装不上而走别的路径。模块头注释的语义边界：**hook 缺失永不导致 sync 失败**，只把「秒级新鲜」降级为「分钟级新鲜」（`src/lib/passive-mode.js:22-25`）。

### 3.3 `sync` 编排

`cmdSync` 单函数体从 `src/commands/sync.js:516` 延伸到 `:3709`（约 3190 行），`module.exports` 在 `:3821-3857`，其后 `:3859-7251` 全是 repair/migration/helper。主流程按执行顺序：

| # | 阶段 | 关键行为 | 行号 |
| --- | --- | --- | --- |
| 1 | 参数与上下文 | `parseArgs(argv)`；接收测试注入（`home`、`trackerDataDir`、`traeCnFetchImpl`、`cursorStoreOptions`…） | `:516-545` |
| 2 | 上传许可判定 | `requiresCloudSyncPref = auto‖background‖fromRetry‖fromNotify‖fromOpenclaw‖有 LOCAL_SYNC_ATTEMPT_ID`；`canUpload()` 只对非交互场景要求 `readCloudSyncEnabled` | `:541-545` |
| 3 | 建目录 + 加锁 | `openLock(sync.lock)`；抢不到锁**不抛错**——写 `sync.skip.json` marker 并向 stderr 输出 `Sync skipped: <reason> — <detail>` 后 return | `:547-559` |
| 4 | 路径与状态装载 | `config.json`、`cursors.json`、`queue.jsonl`、`queue.state.json`、`project.queue.jsonl`、`project.queue.state.json`、`upload.throttle.json`、grok signal | `:567-575` |
| 5 | 历史遗留 baseUrl 迁移 | `config.baseUrl` 命中已退役主机 → `queue.state.offset` 置 0 并清 backoff（可重试、幂等重放） | `:584-634` |
| 6–9 | 扫描根、游标存储、模式判定 | `resolveEnvRoot("codex")` + `resolveScanRoots(...)` → `openCursorStore({trackerDir, cursorsPath, codexRoots})`；`isBackgroundLightweightSync` / `isFullSourceScan` / `sourceAllowed()` 三档；`backgroundCodexUsageRepair` 只在第一个合格后台轮次升级为深扫修复 | `:645-661`、`:713-757` |
| 10–11 | source 组装与文件发现 | 组装 `sources[]`（`union: true` 的 provider 两套都收）→ `uniqueSources` 去重 → `listRolloutFiles`/`listRolloutFilesDeep` → `rolloutFiles[{path, source}]` | `:759-896` |
| 12–13 | 全量前修复与冷扫描过滤 | 六个 repair 依次执行；`filterColdCodexRolloutFiles`（后台轻量只扫近期）+ 读 `deferredCodexAuditSyncs` | `:898-991` |
| 14–17 | 四条主线解析 | Codex（含 cursor-store 重试降级）→ OpenClaw → Claude（多根去重 + 根可用性守卫 + 真值修复）→ Gemini/Antigravity | `:1004-1330` |
| 18 | 其余约 40 个 provider | 统一模式：`sourceAllowed` → 发现 → `parseXxxIncremental` / `multiInstallParse` | `:1337-3300` |
| 19–21 | 汇总、游标提交、上传 | 各 provider 的 `bucketsQueued` 相加；`skipNoOpCursorCommit` 短路，否则 `cursorStore.commit(cursors)`；`resolveRuntimeConfig` → `decideAutoUpload` → `compactPendingQueue` → `drainQueueToCloud` | `:3380-3652` |
| 22–23 | 收尾与释放锁 | 计算 `pendingBytes`；必要时 `scheduleAutoRetry`；`maybeSendHeartbeat`；`finally { progress?.stop(); await lock.release(); }` | `:3654-3708` |

参数全集（`parseArgs`，`src/commands/sync.js:3711-3745`）：`--auto`、`--from-notify`、`--from-retry`、`--from-openclaw`、`--source <name>`（含 `--source=`）、`--drain`、`--wait-for-lock`、`--background`（别名 `--lightweight`）、`--publish-account`、`--all-local-sources`、`--repair-grok`；未知选项抛错。`--auto` 的语义被三处放大：决定 `sourceAllowed` 白名单（`:719-735`）、决定 `skipNoOpCursorCommit`、决定日志静默（`warnProviderParseFailure` 在 auto 下不打印，`:375-378`）；`--background` 进一步把 codex 深度扫描限制为 `BACKGROUND_AUTO_SYNC_SOURCES` 四个源（`:367-373`）。

**锁与并发**。`acquireSyncLock` 把等待行为分三档（`src/commands/sync.js:428-482`）：

```js
// src/commands/sync.js:295-300
const NOTIFY_LOCK_WAIT_MS = 130_000;
const NOTIFY_LOCK_POLL_MS = 5_000;
// Local API sync requests have a 120-second child-process budget. Reserve at
// least 30 seconds for parsing, persistence, and the response after contention.
const PRIORITY_LOCK_WAIT_MS = 90_000;
const PRIORITY_LOCK_POLL_MS = 250;
```

`waitForLock` / `drain` / `publishAccount` → **优先级等待** 90s，每 250ms 重试，超时抛 `SYNC_BUSY`；`fromNotify` → 走低频合并闸 `${lockPath}.notify-wait`，最多等 130s，保证「一次 notify 不丢 Codex 最后一轮」且避免几百个 notify 进程空转；其它（含 `--auto`）→ 立即返回 null：写 marker + stderr 一行，调用方 return，不抛错。

锁文件协议本身比「PID + 超时」多两层（`src/lib/fs.js:69-74,296-306,206-224,105-144,342-368,264-275`）：常量 `LOCK_STALE_MS = 5min`、`LOCK_HEARTBEAT_MS = 30s`、`MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64`；加锁用 `fs.open(lockPath, "wx")`（O_EXCL 语义），锁文件与心跳文件都带**随机 token**（心跳路径 `${lockPath}.heartbeat.${sha256(token)}`），心跳 30s 一次并 `utimes` 两个文件、`timer.unref()`；回收判定先看 PID 是否存活（`process.kill(pid, 0)`，只有 `ESRCH` 算死，`EPERM` 保守当存活），再看 token 专属心跳是否超过 `LOCK_STALE_MS`，老格式锁（无心跳）退回 mtime 超期；回收动作本身用 `.reclaim` 次级锁串行化且**持闸后重新校验一次**（TOCTOU），释放时同样持闸且**只有 token 匹配才 unlink**。被 kill 留下的嵌套 `.reclaim` 闸会永久堵死后续加锁（issue #431），因此有一次性迭代清扫：从深到浅只删「owner 已死或心跳超期」的那一级（`:178-204`）。这套设计的价值在于把「这个锁还活着吗」从「PID 是否存活 + 时间阈值」解耦成「**这个 token 的心跳还新鲜吗**」——PID 复用与长时间健康运行因此不再混淆。

**抢锁失败的可见化**（`src/commands/sync.js:484-506`）：

```js
const SYNC_SKIP_MARKER = "sync.skip.json";                    // src/commands/sync.js:484-506
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

`lock_busy` = 正常并发争用；`lock_debris` = 持有者已死、属需处理状态。成功拿锁后立即清除 marker（`:565`），`status` 读它并输出 `last_sync_skipped`（`src/commands/status.js:285,351-353,1058`）。

**游标提交是一次性的、且可短路**（`src/commands/sync.js:3444-3446`）：

```js
if (!skipNoOpCursorCommit) {
  cursors.updatedAt = new Date().toISOString();
  await cursorStore.commit(cursors);
```

`skipNoOpCursorCommit`（`:3424-3437`）是保守的「本轮什么都没变」判据，条件项共 **13 个**：`opts.auto`、`!isFullSourceScan`、`cursorStore.mode === "v2"`、`cursorStore.requiresCommit !== true`、`totalParsed === (commandCodeResult.cursorUnchanged ? commandCodeResult.recordsProcessed : 0)`、`totalBuckets === 0`、两处 project 桶均为 0、`!commandCodeResult.schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok`。若跳过但审计到期，则把审计计数延迟累加到 `deferred-codex-audit.json`（`:3456-3460`）。

**去重防线（避免重复计数的全部答案，共五层）**：

| 层 | 机制 | 位置 |
| --- | --- | --- |
| ① 根去重 | `identityKey` = `realpath` + 分隔符归一 + 去尾斜杠；`appendUniqueDirs` 只对「附加根」去重，**native/WSL 对故意不去重** | `src/lib/scan-roots.js:116-146,204-224` |
| ② 目录去重 | `uniqueSources` 按 `sessionsDir` 去重 | `src/commands/sync.js:864-870` |
| ③ 文件级 | Claude：`fileId = claudeFileIdentityHash(path, st)`（前 64 KB sha256 + size），重复者标 `duplicateOf` 并把 offset 直接推到 `st.size` | `src/lib/rollout.js:937,1056-1105` |
| ④ 事件/消息级 | Codex `sessionUUID:eventTimestamp`；Claude `${message.id}:${requestId}`（无 requestId 时退化为 `message.id`），持久化时截断到最近 10 万条 | `src/lib/rollout.js:292-295,4914-4919,1233-1237` |
| ⑤ 同 session 多路径 | `needsHistoricalRolloutDedup` 强制改用持久化事件集合（本轮集合只知道本次运行写入的键） | `src/lib/rollout.js:428-461` |

服务端也有对应保证：ingest 按桶键**整行 upsert**，因此重放幂等（`src/commands/sync.js:249-256` 注释）。

### 3.4 游标：两代存储与提交语义

`openCursorStore`（`src/lib/cursor-store.js:25-107`）按**文件体积**决定用哪一代：

```js
const DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024;         // :21
const legacySize = Number(legacyFingerprint?.size || 0);   // :77-86
const shouldActivate = forceV2 || (
  Number.isFinite(activationBytes) && activationBytes >= 0 && legacySize >= activationBytes);
if (!shouldActivate) {
  const cursors = (await readJson(cursorsPath)) || defaultCursorState();
  return new LegacyCursorStore({ cursorsPath, cursors });
}
```

| | LegacyCursorStore | V2CursorStore |
| --- | --- | --- |
| 载体 | 单个 `~/.tokentracker/tracker/cursors.json` | `cursor-store-v2/`（manifest + generations） |
| 激活条件 | 文件 < 16 MB 且未迁移 | ≥ 16 MB，或已存在 manifest，或 `forceV2` |
| 提交 | 全量重写（`src/lib/cursor-store.js:199-201`） | 新 generation 全量落盘 + manifest 原子切换（`:855-1016`） |
| Codex 事件集 | 无独立存储（`codexEventStore = null`） | 按日分片 `codex-events/*.txt`（`:663-726`） |
| 其他能力 | 全部空实现（`:177-197`） | `codex-files/*.json` 分片、opencode 消息/指纹分片、`canSkipCodexDay`、deferred audit |

裸状态只有三个字段：`{ version: 1, files: {}, updatedAt: null }`（`defaultCursorState`，`:1816-1818`）。v2 目录布局（常量在 `:13-21`）：

```
~/.tokentracker/tracker/cursor-store-v2/
├── manifest.json                # { version, current, rollback, previous, legacyFingerprint, updatedAt }
├── deferred-codex-audit.json    # 跳过的冷扫描审计计数
└── generations/<gen-…>/
    ├── generation.json          # metadata（各分片 count 与完整性摘要）
    ├── core-v3.json             # 非 Codex 文件游标 + hourly/projectHourly 等非分片状态
    ├── codex-files/<YYYY-MM-DD>.json  |  codex-events/<YYYY-MM-DD>.txt
    └── opencode-messages/… | opencode-fingerprints/…
```

`V2CursorStore.commit`（`:855-1016`）的九步：① 新建 `gen-<Date.now()>-<uuid8>`；② `cloneGenerationFiles` 把不需要改写的分片整体克隆（`:877-881,1349-1362`，底层 `cloneOrCopy` 用 `COPYFILE_FICLONE`，即 reflink/CoW，不支持时回退普通复制——**与硬链接无关**）；③ 只重写脏分片（`:899-921`），空分片显式 `delete`；④ 事件集按需整体重建或增量追加（`:923-945`）；⑤ 写 `core-v3.json`；⑥ 写 `generation.json` 与 counts；⑦ **最后**原子替换 `manifest.json`（`:987`），`current` 指向新代、`rollback` 指向旧代；⑧ 只保留 `current/rollback/previous` 三代（`:1005-1009`）；⑨ 未 publish 前的失败会删除半成品代目录（`:1010-1015`）。收益是**任一时刻 manifest 指向的 generation 都是一致快照**。

分片规则：Codex 文件归属由 `isCodexSessionCursorPathForRoots` 判定（`**/.codex/sessions/**`、`**/.codex/archived_sessions/**`，或位于本进程 `codexRoots` 下的同名目录，`:1771-1779`）；文件分片键从路径抓 `/(\d{4})/(\d{2})/(\d{2})/` 得 `YYYY-MM-DD`，否则 `misc`（`:1788-1793`）；事件分片键取 `<identity>:<ISO 时刻>` 冒号后的 10 字符（`:1800-1805`）；`safeShardFilename` 只允许 `^\d{4}-\d{2}-\d{2}$`，否则写 `misc`（`:1807-1809`）——**防止分片键变成路径穿越**。这套分片支撑「只加载今天需要的分片」：`loadCodexFilesForPaths` / `canSkipCodexDay`（`:299-318,609-639`）；`codexRoots` 必须与扫描根同源，否则同一文件会被反复重解析（`src/commands/sync.js:635-652` 注释，issue #639/#657）。

两个跨版本兼容字段（`:974-986`）：

```js
const compatibleGeneration = this.generation?.coreFile === "core.json" ? this.generation.id : this.manifest?.previous || null;
const nextManifest = {
  version: STORE_VERSION, current: generationId, rollback: this.generation?.id || null,
  // Older v2 readers reject core-v3.json and fall back to this frozen,
  // fully-inline checkpoint instead of opening an empty message index.
  previous: compatibleGeneration,
  legacyFingerprint: this.manifest?.legacyFingerprint || null,
  updatedAt: new Date().toISOString(),
};
```

即：老版本读者遇到 `core-v3.json` 会拒绝，此时靠 `previous` 指向的**冻结内联检查点**兜底，避免读到空消息索引。

损坏恢复：读取侧依次尝试 `manifest.current → rollback → previous`（`:124-139`）；`openManifestGeneration` 失败抛 `TOKENTRACKER_CURSOR_STORE_CORRUPT`（`:72-74`）；每个分片写盘时把 `serialized` 摘要与 count 记进 metadata（`buildShardIntegrity`，`:1466-1471`），读取时 `assertShardIntegrity` 校验（`:1473-1481`）。**「可重试」与「损坏」的分野**：分片缺失导致的 `TOKENTRACKER_CURSOR_STORE_RETRY` 是「加载缺失分片后重试」，sync 捕获后会 `loadCodexFilesForPaths` 再跑一次全量解析（`src/commands/sync.js:1069-1085`）；`CORRUPT` 直接上抛，不做静默修复（`:1086-1087`）。`legacyFingerprint` 会检测 `cursors.json` 在 v2 建立之后是否被外部改动，一旦改动就重新迁移（`src/lib/cursor-store.js:44-58`）。

### 3.5 多安装 / 多根 / WSL

三层发现模型：① **出产地**（native home、WSL home、`CODEX_HOME`/`CLAUDE_CONFIG_DIR`、`config.scanRoots.<provider>[]`）→ realpath 归一 + 可列性校验；② **安装选择**（union 或单选 `wsl-first`/`native-first`/`wsl-only`/`native-only`/`both`）→ 可选 `requireAnyChild` 过滤「空壳安装」；③ **文件发现**（`listRolloutFiles` / `listRolloutFilesDeep` / `listClaudeProjectFiles`）→ 去重五层（见 §3.3 末表）。

`config.scanRoots` 解决的问题定义在 `src/lib/scan-roots.js:3-14`：某 agent harness 用私有 home（`CODEX_HOME=...`）跑 Codex 时，这些会话**只在恰好继承同一环境变量的进程里可见**——hook 触发的 sync 看得到、桌面 App 的后台刷新看不到；更糟的是两个生产者对「某 rollout 的游标存在哪」达不成一致（游标存储只把位于自己 `codexRoots` 下的 rollout 归入按日分片），于是每次交替都**从字节 0 重解析**（issue #639 记录了「一个 30 分钟桶被重发 138 次、单日 1.2B token 报成 59B」）。解法是根列表来自**持久化配置**，且游标存储的 `codexRoots` 由同一列表派生（`src/commands/sync.js:635-652`）。合并顺序（先出现者胜，realpath 去重，`src/lib/scan-roots.js:19-23,163-202`）：调用方本来就会扫的根（`origin: "native"`）→ 本进程隐含根（`origin: "env"`）→ `config.scanRoots.<provider>[]`（`origin: "config"`）。两个细节：**相对路径一律相对 `home`**，绝不相对 cwd（各进程 cwd 不同，`:46-55`）；非 native 且当前不可用的根**仍保留在列表里**（让游标路径分类稳定），由调用方用 `exists` 跳过（`:158-162,228-232`）。

「存在」的判定是三态的，这一条值得单列（`src/lib/scan-roots.js:89-110`）：

```js
function scanRootDirState(target, deps = {}) {
  const statSync = deps.statSync || fs.statSync;
  const opendirSync = deps.opendirSync || fs.opendirSync;
  try {
    if (!statSync(target).isDirectory()) return { exists: false, error: null };
    opendirSync(target).closeSync();   // 探测「可列举」，不读条目，大根目录仍然廉价
    return { exists: true, error: null };
  } catch (e) {
    const code = e && typeof e.code === "string" ? e.code : "EUNKNOWN";
    return code === "ENOENT" || code === "ENOTDIR"
      ? { exists: false, error: null } : { exists: false, error: code };
  }
}
```

三态是 `{exists:true}` / `{exists:false,error:null}`（真不存在）/ `{exists:false,error:"EACCES"}`（存在但读不了）；第三态在 UI 上必须显示 `unreadable: EACCES` 而不是 `missing`（`:248-253`）。理由：只做 `stat` 时，一个权限 000 的目录会被判为「存在」，进扫描后产出 0 个文件、状态页显示「存在」，并**绕过 deferred-repair 守卫**（守卫语义是「根不存在就不许重算历史」）。

WSL 探测（`src/lib/wsl-probe.js`）：枚举发行版用 `wsl.exe -l -v`、**UTF-16LE 解码**、清洗 `\0`/BOM、解析 `*` 默认标记与版本号（`:26-55`），默认发行版优先（`:91`），进程内缓存 `_cachedDistros` 且 strict 模式会重试被缓存过的失败（`:57-97`）；某些 WSL 版本对空 verbose 列表返回非 0，此时用 `wsl.exe -l -q` 成功且为空来确认空（`:69-80`）；home 路径为 `\\wsl$\<distro>\home\<user>\<providerDir>`，**v1 发行版优先 `\\wsl.localhost\`、v2 优先 `\\wsl$`**（`:180-204`）；模式由 `TOKENTRACKER_WSL_MODE` ∈ {`wsl-first`(默认), `native-first`, `wsl-only`, `native-only`, `both`} 控制（`:18-24,106-109`）；非 win32 时 `wsl-probe.js:149` 返回 `{ native: null, wsl: null }`（发现层根本不探测 WSL），「只返回 native」的落点在上一层 `src/lib/install-resolver.js:21-23`。两个平台坑的修法：WSL 会话的 POSIX cwd 在 Windows 上会被解析成 `C:\home\...` 导致项目归因永远失败（issue #374），`mapWslCwdToUnc` 用**转录文件自身的 UNC 前缀**重锚（不重新探测，因此能挑到本机真正可用的前缀），并显式跳过 `/C:/...` 这类「以斜杠开头的 Windows 路径」（`:206-242`）；跨 UNC 读活跃 SQLite 用 `snapshotSqliteDb` 把 db 与 `-wal`/`-shm`/`-journal` 一起复制到临时目录再读（`:244-262`）。

安装选择（`src/lib/install-resolver.js:19-45`）的两个关键设计：① **`requireAnyChild` 折进探测本身**——把「必须含 `sessions`/`archived_sessions`」塞进传下去的 `existsSync`，于是 `discoverWslHome` 会**跳过空壳发行版继续找下一个**；理由（`src/commands/sync.js:761-772`）：先取候选再校验的话，第一个发行版里一个空的 `~/.codex` 壳会胜出并被判无效，而真正有数据的后面那个发行版永远不会被看到。② **`union` 把模式从「排他」降级为「优先级」**——因为「一个 WSL 空壳把 native 顶掉」曾导致「会话浏览器列得出会话、Dashboard 却一个 token 都不计」（issue #27，`src/lib/install-resolver.js:6-13`）；前提是**该 provider 的解析器按路径无关身份去重**，否则 union 就是双计。实测 `union` 只用于 codex（`src/commands/sync.js:782-787`，`requireAnyChild: ["sessions","archived_sessions"]`）与 acode（`:815-820`）。

`union` 给出两个安装时，`multiInstallParse` 使用**按安装名分区的游标**（`src/lib/multi-install-parser.js:8-59`），三条不变量：① **单安装时必须回落到扁平游标**，且「幸存者」由 **UNC 前缀判定**（`wsl.isUncPath`）而非模式偏好——注释明写：wsl-first 下若 WSL 安装消失（发行版被删、探测瞬时失败），其游标不得被「捐赠」给剩下的 native 安装，否则后者会双计整个历史（`:13-22`）；② **扁平 → 命名空间的迁移种子**默认是「所有命名空间都塞一份」，只有在 `detectInstall` 探针**恰好命中一个**安装时才让另一个空起步（`:61-85`），理由是「一个安装在没有去重状态的情况下重解析会双计全部历史，而有限的回填缺口是更便宜的失败」；③ 异常路径 `parseErr?.code !== "TOKENTRACKER_CURSOR_STORE_RETRY"` 时把 `ns` 写回 `cursors[providerName]`（`:49-54`）——【未能确认】该分支的完整语义，从这段控制流看不出两种写回差别对最终状态的全部影响，判定条件：构造 cursor-store 重试场景的定向测试。

### 3.6 读取安全

**Dashboard 读队列的唯一入口**是 `createAsyncQueueReader`（`src/lib/async-queue-reader.js`，全文 117 行）：

```js
function signature(stat) { return `${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:${stat.ctimeNs}`; }  // :8-10
if (cached && cached.identity === identity && size > cached.size && cached.consumedBytes > 0) {              // :40-47
  const head = await probe(file, 0, cached.head.length);
  const anchor = await probe(file, cached.consumedBytes - cached.anchor.length, cached.anchor.length);
  if (head.equals(cached.head) && anchor.equals(cached.anchor)) { offset = cached.consumedBytes; seen = cached.seen; }
}
```

配套六条：① 签名不变**直接返回缓存行**（命中在 `:35`，设计目的是「Dashboard 一次刷新会并发打多个端点」，注释在 `:12-15`；`:99-104` 是 `ENOENT` 清理与读取失败降级，不是命中路径）；② 只把**以 `\n` 结尾的完整行**计入 `consumedBytes`，尾部残行单独放 `tail`，且**残行的 key 不进已提交 map**（`:64-69,87-91`）——写者可能还没写完这行、键值还会变；③ 每 64 KB 让出事件循环（`CHUNK_BYTES = 64 * 1024`，`await yieldToRequests()`，`:5,79`），避免冷启动读大队列卡死 HTTP 事件循环；④ 坏行容错：`JSON.parse` 失败只 `malformed++`，最后 `console.error` 计数（`:51-57,95`）；⑤ 同路径并发合并（`pending` map，`:109-114`）；⑥ LRU 上限 4 个文件（`:16,94`），出错时回退到上一次缓存行（`:102-103`）。

**物理 JSONL 解析**用自研字节级生成器而非 `readline`（`src/lib/jsonl-lines.js:3-11`：`readline` 会把 U+2028/U+2029 当行分隔符，而它们在 JSON 字符串里是合法字符）：只认 `\n`（`chunk.indexOf(0x0a)`，`:57`）；`maxPhysicalBytes` 超限抛 `PhysicalJsonlLimitError`（`code: "PHYSICAL_JSONL_LIMIT_EXCEEDED"`，`:12-18,61-63,96-98`）；`maxRecordBytes` 超限则**不缓冲不解码**，直接产出 `{line:null, utf8Valid:null, oversized:true, physicalBytes, terminated:true}`（`:65-73`，末行未闭合的超长行带 `terminated:false`，`:111-114`），但 `physicalBytes` **仍计入字节预算**——超长行照常推进按字节提交的游标；UTF-8 走 `TextDecoder("utf-8", {fatal:true})`，策略 `invalidUtf8: "throw"｜"record"`（`:20-27,34-36,84-90`）；`stripCr` **对调用方不可选**——对外 opts 只有 `invalidUtf8`/`maxPhysicalBytes`/`maxRecordBytes`（`:29-33`），调用点硬编码 `{ stripCr: true }`（`:85`）；末行未终止由 `terminated:false` 标记，交给调用方决定是否重试（`:127`）。

读侧纠错 `normalizeQueueRow`（`src/lib/local-api.js:170-200`）：历史口径修正**不重写队列文件**，只在读取时纠正——旧口径 Codex 行把 `input_tokens -= cached_input_tokens`，Cursor/Antigravity 的 `billable_total_tokens` 从 0 抬到 `total_tokens`：

```js
function normalizeQueueRow(row) {
  let normalized = row;
  if (isLegacyInclusiveCodexRow(normalized)) {
    normalized = { ...normalized, input_tokens: Number(normalized.input_tokens || 0) - Number(normalized.cached_input_tokens || 0) };
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

「legacy Codex 行 input 含缓存」的判定是一个**不变量**而非版本号：`cached > 0 && input >= cached && total === input + output`（`isLegacyInclusiveCodexRow`，`:155-168`）；Cursor/Antigravity 那半段是「Included in Pro / no charge」被错误门控的遗留（issue #106）。同一函数也被**上传路径复用**（`src/commands/sync.js:4509`），保证本地与云端同一口径。

**上传侧读取**：`iterateQueueLines(handle, start, end)`（`src/lib/upload-queue.js:22-43`）同样按 `\n` 切、跨块拼接、显式标注 `complete: false` 的尾行；注释强调「offset 是**字节位置**，不是字符长度」（`:20-21`）。`drainQueueToCloud` 只上传完整行（`if (!record.complete) break;`，`src/commands/sync.js:4449`）；单批记录数上限同时约束桶行与 `account_session_state` 行（`:4489,4512`）——注释记录了曾经的故障：TRAE 首次同步会产生每会话一条的 state 记录，若不计入上限就会一次性发出超过 edge 500 上限的请求并**永久失败**（`:4433-4441`）。

**其余读取面防护**：请求体上限 `readBodyLimited(req, maxBytes)`（`src/lib/local-api.js:722-740`）；头像代理 512 KB / 64 条上限（`:46-47`）；本地 API 触发的 sync 有 **120s 子进程预算**（`SYNC_TIMEOUT_MS = 120_000`，`:22`），超时 `SIGTERM` 并返回 `code: "SYNC_TIMEOUT"`（`:778-792`）——sync 侧为优先级锁预留 90s，正是为了给解析与持久化留 ≥30s（`src/commands/sync.js:295-300`）；spawn sync 时环境变量透传是**白名单式**的（只有 `TOKENTRACKER_DEVICE_TOKEN` 存在才附 base URL/anon key，且始终注入 `TOKENTRACKER_LOCAL_SYNC_DEVICE_TOKEN`，`:754-762`）。

### 3.7 调度源与新鲜度分层

| 源 | 频率 | 范围 | 实现 |
| --- | --- | --- | --- |
| provider hook → notify.cjs | 事件驱动，**20s 节流** | 单源（`--source`） | `src/commands/init.js:1519-1536` |
| omp 扩展 / OpenClaw 插件 | 事件驱动，**12s** / **15s 节流** | `omp` / `openclaw` | `src/lib/omp-hook.js:66,99-109`；`src/lib/openclaw-session-plugin.js:363,502-518` |
| serve 后台轮询 | **60s**（仅 Windows 壳，且未被桌面托管时） | 本地全源轻量 | `src/commands/serve.js:35,294-366` |
| 自动重试（backoff 到期） | 一次性延迟进程，上限 2h | 触发来源单源 | `src/commands/sync.js:4169-4293` |

`startNativeBackgroundSync` 的六个设计点（`src/commands/serve.js:294-366`）：① **仅 Windows**（`TOKENTRACKER_APP_SHELL === "windows"`），macOS 壳自管唤醒循环——注释说明双循环会让 `sync.lock` 近乎连续占用、饿死 provider 的 notify sync（`:248-256`）；② **桌面托管时让位**（`TOKENTRACKER_NATIVE_SYNC_OWNER=windows-host` 时返回 null）；③ **跑在子进程**——解析器里用 `execFileSync` 读 SQLite，在主进程会冻结事件循环并连带阻塞所有 Dashboard 端点（`:311-315`），子进程 spawn 参数与 hook 一致，因此与 hook sync 争同一把 `sync.lock`；④ **强制 `native-only`**（`:335-342`），避免 UNC 元数据探测唤醒 WSLg 并弹出不可见的 msrdc 窗口抢焦点；⑤ **单飞防重入**——`let inFlight = null;`，`run()` 首句 `if (inFlight) return inFlight;`，上一轮子进程未退出时不再 spawn 第二个（`:332-334`）；⑥ **关停路径**——`SIGINT`/`SIGTERM` → `nativeBackgroundSync?.stop()` → `server.close()` 回调里 `process.exit(0)`，另有 `setTimeout(…, 3000)` 兜底强退（`:281-288`）。

```js
// src/commands/serve.js:303-309,339-345（节选）
if (String(appShell || "").trim().toLowerCase() !== "windows") return null;
// The Windows tray host owns its five-minute background sync timer ...
if (String(nativeSyncOwner || "").trim().toLowerCase() === "windows-host") return null;
const backgroundEnv = { ...process.env, TOKENTRACKER_WSL_MODE: "native-only" };
const pending = Promise.resolve()
  .then(() => sync(["--auto", "--background", "--all-local-sources"], { env: backgroundEnv }))
```

另有 **6 小时一次的心跳定时器**（`:266-273`），走 `maybeSendHeartbeat`，与 sync 尾部用的是同一节流状态（`src/commands/sync.js:3699-3704`）。注意与**锁心跳**区分：锁心跳是 30s 一次的文件 `utimes`（`src/lib/fs.js:206-224`），这里的心跳是 6h 一次的网络上报，两者同名不同物。

**冷扫描审计**把「全量重扫」摊到 24 小时（`src/commands/sync.js:3763-3783`）：① 从来没有审计状态（`version !== 1` 或 `lastFullScanAtMs` 非法）；② 距上次全量 ≥ `CODEX_COLD_SCAN_AUDIT_INTERVAL_MS = 24h`（`:294`）；③ 累计「自上次全量以来的 sync 次数」≥ `CODEX_COLD_SCAN_AUDIT_MAX_SYNCS = 288`（`:301`，≈24h × 12 次/小时，与 5 分钟粒度对应）。另有一条**时钟回拨保护**：`lastFullScanAtMs - nowMs > 5 * 60 * 1000` 时也判定到期（`:3772`）——否则用户改系统时间后普通阈值判断永远不会到期，审计被永久饿死。跳过的轮次不丢计数：`recordCodexColdScanAudit`（`:3785-3819`）写 `cursors.codexColdScanAudit`，字段共 6 个（`{version, lastFullScanAtMs, lastFullScanAt, syncsSinceFullScan, lastSkippedFiles, updatedAt}`）；`skipNoOpCursorCommit` 时增量写进 `deferred-codex-audit.json`（`cursorStore.writeDeferredCodexAuditSyncs`，`src/lib/cursor-store.js:640-660`）。【未能确认】`deferred-codex-audit.json` 与 `codexColdScanAudit` 的双写合并与清除路径——只看到 `readDeferredCodexAuditSyncs` 一处调用（`src/commands/sync.js:965-969` 附近）；判定条件：追踪该文件在两代存储下的完整读写闭环。

**自动重试**（`scheduleAutoRetry`，`:4169-4228`）写 `auto.retry.json`（payload `{version:1, retryAtMs, retryAt, reason, pendingBytes, scheduledAt, source, syncSource?}`），随后 spawn 一个**分离的 `node -e` 脚本**，脚本体内 `setTimeout` 到点后重新读 marker（`:4278-4285`）。要点：① 延迟上限 `AUTO_RETRY_MAX_DELAY_MS = 2h`（`:4310`）；② **marker 是权威**——被 `clearAutoRetry` 删掉后，醒来的进程因读不到 `retryAtMs` 直接退出（`:4244-4247`）；③ 已有更早的重试计划时不重复排（`:4186-4202`）；④ `TOKENTRACKER_AUTO_RETRY_NO_SPAWN` / `config.autoRetryNoSpawn` 可禁止 spawn、只留 marker（`:4217-4219`）。触发条件是 `pendingBytes > 0` 且有 `canUpload()`、本轮确实尝试过上传、且不是 `publishAccount`（`:3654-3676`）。

**新鲜度的分层结果**：刚用过的 agent（hook）秒级 → 其它 hook 驱动的 agent 各自触发 → Windows 上所有本地源 60s 兜底 → 深度历史（`archived_sessions`、冷扫描审计）24h 或 288 次 sync 后才有一次完整扫描。

### 3.8 上传与队列压缩

`drainQueueToCloud`（`src/commands/sync.js:4315-4413`）受三重约束：批大小默认 200、硬上限 `MAX_INGEST_BUCKETS = 500`（`:4313,4329`）；单次调用批数默认 5、`--drain` 时 100（`:3563`）；总时长预算 `UPLOAD_BUDGET_MS = 90_000`，并用 `AbortSignal.timeout` 把同一预算传给 fetch（`:168,4362`）。另有：`upload_scope === "community"` 时批数再压到 10（`:4392`）；每批成功后**立即**写 `queue.state.json` 的 offset（`:4397-4400`）；只上传完整行（`:4447-4449`）；同批内同一 `(source, model, hour_start)` 去重为一条（`bucketMap`，`:4510`）；`account_session_state` 按 `(source, session_id)` 取**严格更新的 `snapshot_verified_at`**（`:4478-4484`，防时钟回拨）；超时但有成功批次时**不抛错**，保留已推进的 offset。自动上传另有一层节流：`AUTO_UPLOAD_CONFIG = { intervalMs: 15 min, batchSize: 200, maxBatchesSmall: 5, maxBatchesLarge: 100 }`（`:162-167`）+ `decideAutoUpload` 的 jitter/backoff（`:3517-3534`）；失败时 `recordUploadFailure` 落 `upload.throttle.json`（`:3637-3643`），`--drain` 遇到节流直接抛 `SYNC_UPLOAD_BACKOFF`（`:3529-3534`）。

**队列压缩 `compactPendingQueue`**（`src/lib/upload-queue.js:64-132`）的竞态防护：

```js
// :111-121 替换前后的竞态校验
await output.sync(); await output.close(); output = null;
// Windows cannot replace the destination while our read handle is open.
await input.close(); input = null;
const current = await fs.stat(queuePath);
if (current.ino !== before.ino || current.size !== before.size || current.mtimeMs !== before.mtimeMs) {
  throw new Error("Queue changed during pending compaction");
}
await fs.rename(temporary, queuePath);
```
它只在「够大（`pendingBytes >= 1_000_000`）且 offset 落在行边界（`offset-1` 处必须是 `\n`）」时动手（`:73-78`），只重写 `[offset, size)` 区间、原样字节复制 `[0, offset)`（已上传历史不变）；pending 区每个键只保留**最后一条**（`:79-91,105-110`），`account_session_state` 用 `verifiedMs` 严格更大而非「最后一条」。**只在 offset 之后整理**这一条把「压缩」与「上传进度」完全解耦——重写整文件会破坏 `queue.state.json` 的字节 offset 语义。

---

## 4. 解析层：统一骨架与队列契约

### 4.1 45 个解析器的真实契约

**没有统一的返回契约。** 对 45 个 `parse*Incremental` 的顶层 `return { … }` 逐个提取键名后的实测结果：

- **5 个是转发薄壳**，没有自己的返回字面量：`parseOmpIncremental`(`src/lib/rollout.js:16087`)、`parseOmoIncremental`(`:16100`)、`parsePiIncremental`(`:16689`)、`parsePrimeAgentIncremental`(`:16699`)、`parseMinimaxCodeIncremental`(`:16775`)——形态是 `return parseOmpLikeIncremental({ ...options, stateKey, source, resolveSessionFiles, ... })`。
- 其余 **40 个自持 return，去重后共 11 种键集合**。
- **40/40 都有的键只有两个**：`eventsAggregated` 与 `bucketsQueued`。计数键的名字在三种之间跳——`recordsProcessed`（31 个）、`filesProcessed`（6 个）、`messagesProcessed`（3 个），所以连「族」的划分本身都不稳定。

```js
// 四种骨架（逐字；行号为 return 所在行）
return { filesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };      // :1239  骨架 A
return { messagesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };   // :6027  骨架 B
return { recordsProcessed, eventsAggregated, bucketsQueued };                          // :7137  骨架 C
return { recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };    // :16084 骨架 D
```

| # | 键集合 | 个数 | 代表函数（括号内为 return 所在行） |
| --- | --- | --- | --- |
| 1 | 骨架 C | 19 | `parseClaudeScienceIncremental`(7137)、`parseKiroCliIncremental`(9268)、`parseZedIncremental`(12839)、`parseGooseIncremental`(14767)、`parseCraftIncremental`(17297) 等 |
| 2 | 骨架 A | 6 | `parseRolloutIncremental`(705)、`parseClaudeIncremental`(1239)、`parseGeminiIncremental`(1409)、`parseOpencodeIncremental`(1562)、`parseOpenclawIncremental`(1922)、`parseAntigravityIncremental`(21394) |
| 3 | 骨架 D | 5 | `parseDevinIncremental`(14456)、`parseDroidIncremental`(15542)、`parseOmpLikeIncremental`(16084)、`parsePiLikeIncremental`(16686)、`parseGrokBuildIncremental`(20551) |
| 4 | 骨架 B | 3 | `parseOpencodeDbIncremental`(6027)、`parseQoderDbIncremental`(6398)、`parseQoderNewIncremental`(6788) |
| 5 | 骨架 C + 扩展键 | 7 个单例 | Copilot(18111)+`usageClaims`、CopilotAppDb(19538)+`dbErrors`、TraeCnApi(22791)+`skippedRows`、Dsh(24029)+`deferredMigrations`、Trae(22175) 7 键、CommandCode(24664) 7 键、CopilotSessionStore(19003) 9 键 |

三个加宽签名的逐字证据：

```js
// src/lib/rollout.js:18111  Copilot
  return { recordsProcessed, eventsAggregated, bucketsQueued, usageClaims: recentOtelUsageEvents };
// src/lib/rollout.js:22175  Trae（共 7 键）
  return { recordsProcessed, eventsAggregated, bucketsQueued, recordsSkipped, estimatedRecords, unpricedRecords, errors };
// src/lib/rollout.js:19003  Copilot Session Store（共 9 键）
  return { active: canonicalActive, healthy: allCurrentPathsHealthy, adoptedThisRun, canonicalDbPaths,
           recordsProcessed, eventsAggregated, bucketsQueued, dbErrors, usageClaims: recentEvents };
```

`schemaMigrated`/`cursorUnchanged` 全库只出现在 Command Code 一处（`:24670-24671`），即**只有它会向调用方反馈「游标是否无需重写」**；其余 44 个 parser 一律无条件重写 `cursors.hourly`（`:692-697,1227-1232`）。

三个「假签名」陷阱（把**值**当键名读会数出不存在的键）：`parseCursorApiIncremental:7270` 实际是 `return { recordsProcessed: total, eventsAggregated, bucketsQueued }`；`parseTraeCnApiIncremental:22791` 同理（真扩展键是 `skippedRows`）；`parseGrokBuildIncremental:20551` 是 `recordsProcessed: eventsAggregated`（上游疑似把「记录数」写成了「事件数」，键并不重复，但该绑定的语义可疑）。**早退分支会让同一函数出现第二种键集合**，调用方若按顶层键集合统一解构早退值会拿到 `undefined`：`parseTraeCnApiIncremental:22634`（3 键、没有 `skippedRows`）、`parseCopilotIncremental:17944,17963`、`parseTraeIncremental:22163`。

所谓「统一签名」是**约定俗成**：`cursors` / `queuePath` / `onProgress` 是恒定三件套；`projectQueuePath` + `publicRepoResolver` 是「项目归属能力」的开关；`source` 只在会被多种 agent 复用的 parser 上出现（参数表以 `^async function parse[A-Za-z0-9]*Incremental\(` 全量枚举）。**最重要的契约事实**：所有 parser 的返回值都只是**进度计数**，不是用量数据本身——用量数据的唯一出口是 `queuePath` 文件。`onProgress` 的形状两条主线一致（`:671-679`）：`{index, total, filePath, filesProcessed, eventsAggregated, bucketsQueued: touchedBuckets.size}`。

### 4.2 解析器共同机制

**文件发现**：`mapConcurrent(items, concurrency, mapper)`（`:72`）手写有界并发，`FILE_METADATA_CONCURRENCY = 32`（`:70`），结果按 index 落位、顺序稳定；`listRolloutFiles(sessionsDir, options)`（`:91`）严格匹配 `YYYY/MM/DD/rollout-*.jsonl`、三层目录扫描、结果 `localeCompare` 排序；`listRolloutDayFiles(dayDir, …)`（`:143`）以目录 statKey 命中缓存、未命中才 readdir；`listRolloutFilesDeep(dir)`（`:201`）任意深度递归，供 `archived_sessions` 这类扁平归档；`listClaudeProjectFiles` / `walkClaudeProjects`（`:219` / `:5013`）任意深度、**只认 `endsWith(".jsonl")`**、不解析 slug；`safeReadDir(dir)`（`:5005`）把 `fs.readdir` 包 try/catch、**读失败返回 `[]`**。

```js
// src/lib/rollout.js:179
function isRolloutFileName(name) {
  if (typeof name !== "string") return false;
  if (name.includes("/") || name.includes("\\") || name !== path.basename(name)) return false;
  return name.startsWith("rollout-") && name.endsWith(".jsonl");
}
```

目录清单缓存键是 `ino:size:mtimeMs:ctimeMs` 四元组（`directoryInventoryStatKey`，`:186`）。

**增量读取与游标判据**——核心只有约 30 行（`:487-518`），三态全靠 `fs.stat` + 两个布尔量：

```js
    const prev = cursors.files[key] || null;                            // src/lib/rollout.js:487-518
    const inode = st.ino || 0;
    const sameInode = prev && prev.inode === inode;
    const prevOffset = sameInode ? prev.offset || 0 : 0;
    const truncated = sameInode && prevOffset > st.size;
```

| 情形 | `sameInode` | `truncated` | 起点 | 说明 |
| --- | --- | --- | --- | --- |
| 首次见到 | 否 | — | 0 | 全新文件全量读 |
| 稳态追加 | 是 | 否 | `prev.offset` | 只读尾部新增字节 |
| 文件被截断/变小 | 是 | 是 | 0 | 重读全文 |
| 游标缺 `lastTotal`（老格式） | 是 | 否 | 0 | `rebuildingCodexBaseline` 强制重建累计基线（`:493-500`）；原子替换（新 inode）则 `sameInode=false` → 起点 0、`lastTotal=null`，全量重读 |

稳态短路（「文件没长就跳过」，`:538-555`）：

```js
if (sameInode && !truncated && startOffset >= st.size && (!projectEnabled || projectUpToDate)) {
  // 仅回调进度，不做任何解析、不重写游标
  continue;
}
```

`projectUpToDate` 分支值得注意：开了项目归因时，即使文件没增长也要检查 `projectFileContext` 的**新鲜度**（git 配置指纹过期就重扫上下文），并存在一条只解析项目上下文、不算 token 的路径 `projectContextOnlyScan`（`:520-537,589-604`）。Claude 侧额外有一态 `projectSettled`（`:1133-1134`）：idle 且项目上下文已定才跳过；**仍在增长的文件即使 cwd 未找到也会重试**，避免早期同步丢掉整段会话的项目归属（`:1115-1120` 注释）。

**offset 的提交粒度不一致**——`const endOffset = st.size;` 在 `src/lib/rollout.js` 共 **3 处**（`:2291`、`:2682`、`:2793`），**只有 `:2793` 落到 token 游标**：

| 站点 | 所属函数 | 是否推进 token 游标 |
| --- | --- | --- |
| `:2291` | `parseRolloutFile`（Codex 单文件主解析） | **否**——仅用于「无新字节」早退（`:2294-2302`），正式返回走 `committedEndOffset`（`:2652`） |
| `:2682` | `scanRolloutProjectFileContexts` | **否**（只推进 `projectOffset`，返回 `committedEndOffset`，`:2764-2765`） |
| `:2793` | `parseClaudeFile` | **是——唯一一处**（`return { endOffset, eventsAggregated }`，`:2889` → `cursors.files[key].offset = result.endOffset`，`:1194`） |

即 Codex 的 token 路径只提交到最后一个 `terminated` 行，**连项目上下文扫描位都做了同样的提交保护**（`:2765`），OpenClaw 另有显式边界检查：

```js
async function openclawOffsetEndsAtLineBoundary(fileHandle, offset) {   // src/lib/rollout.js:1949
  const byteOffset = Math.max(0, Number(offset) || 0);
  if (byteOffset === 0) return true;
  const buffer = Buffer.allocUnsafe(1);
  const { bytesRead } = await fileHandle.read(buffer, 0, 1, byteOffset - 1);
  return bytesRead === 1 && (buffer[0] === 0x0a || buffer[0] === 0x0d);
}
```
**Claude 路径没有这一保护**：同步瞬间读到半行时 `readline` 会 yield 未闭合尾行、`JSON.parse` 失败被吞（`:2843-2847`），游标仍推进到 `st.size`，该行字节**在本代文件内不可恢复**（文件下次 inode 变化或截断时会走重置路径全量重扫，`:488-504`，所以不是严格永久）。→ 列入待复核点。

**坏行容错**：全库检索 `bad_lines|badLines|bad_line` 在 `src/lib/` 下**零命中**。真实做法是——Codex：`invalidRecordPolicy` 二态、默认 `"skip"`（`:261`），非法值抛 `TypeError`（`:263-265`），`"throw"` 只被一次性血缘重建使用（`src/commands/sync.js:5774`），skip 模式下坏 JSON 行**静默 `continue`**、不计数不上报；Claude：坏行**完全静默**（`:2812-2816` user 行与 `:2843-2847` usage 行都是 `catch (_e) { continue; }`）。队列层的畸形行计数 `countMalformedCodexProjectQueueRows`（`src/commands/sync.js:6436`）**全库只有一个调用点**（`:5824`，位于 codex 重扫修复内部，作用是「重建前若发现 project 队列有畸形行就放弃本次修复以免丢数据」，`:5825-5830`）——它只影响**修复是否执行**，不影响用量统计。

**超大文件保护**的实际形态是三层降本，而非单文件字节上限：① 天目录清单缓存 `cursors.codexDayInventoryCache`（`src/commands/sync.js:789` 注入）；② 冷文件跳过 `filterColdCodexRolloutFiles()`（`src/lib/rollout.js:708`），仅 `opts.auto` 下启用（`src/commands/sync.js:964-980`），`DEFAULT_CODEX_COLD_SKIP_RECENT_DAYS = 2`（`:62`），且**近 30 天内的冷文件仍要 stat 一次看是否长大**（`:69,806-822`，注释指向 issue #592）；③ 游标分区存储（见 §3.4）。**Codex/Claude 两条主线没有「单行太长则跳过」的保护**——`physicalJsonlRecords` 支持 `maxRecordBytes`，但 `parseRolloutFile` 只传 `invalidUtf8`（`src/lib/rollout.js:2396-2397`），有此保护的只有 OMP 族（`:15715,15717`，默认 `OMP_LIKE_MAX_RECORD_BYTES = 64 * 1024 * 1024`）。【未能确认】这是有意不设限还是遗漏；判定条件：确认某 rollout 若含超大 `response_item` 时的内存行为。

### 4.3 半小时桶的唯一实现

```js
function toUtcHalfHourStart(ts) {                                      // src/lib/rollout.js:4173
  const dt = new Date(ts);
  if (!Number.isFinite(dt.getTime())) return null;
  const minutes = dt.getUTCMinutes();
  const halfMinute = minutes >= 30 ? 30 : 0;
  const bucketStart = new Date(
    Date.UTC(dt.getUTCFullYear(), dt.getUTCMonth(), dt.getUTCDate(), dt.getUTCHours(), halfMinute, 0, 0));
  return bucketStart.toISOString();
}
```

语义拆解：入参是**任意可被 `Date` 解析的字符串**（Claude 用 `obj.timestamp`；Codex 用 `token.timestamp`）；取整规则 `minutes >= 30 → :30`，否则 `:00`，秒与毫秒强制清零；**全 UTC**（`getUTC*` + `Date.UTC`，带偏移的 ISO 串会先被规范化到 UTC）；输出 `toISOString()`（形如 `2026-10-01T12:30:00.000Z`）；非法或缺失时间戳返回 `null`，调用方一律 `if (!bucketStart) continue;`（Claude `:2867-2868`、Codex `:2605-2606`、Claude Science `:7071-7074`）。

**两处独立实现**（全库 `function toUtcHalfHourStart` 仅 2 命中，不是共享 import）：主实现 `src/lib/rollout.js:4173`（被 `module.exports` 导出，所有增量 parser 使用）；副本 `src/lib/claude-categorizer.js:1175`（Claude 真值修复专用）。二者关系是「**语义等价、文本不同**」：取整规则完全相同，但前者分步构造 `bucketStart` 后再 `toISOString()`，后者写成单行链式返回。**没有任何测试或断言锁定二者的等价性**：`test/` 下引用 `toUtcHalfHourStart` 的两个测试都 `require("../src/lib/rollout")`（`test/sync-droid-dup-session-repair.test.js`、`test/trae-entitlement.test.js:130-137`），`claude-categorizer.js` 的那一份无人测；改动任一处不会让另一处失败。一旦真值修复与增量解析分叉，队列里会出现两套时间分桶。

**跨日与本地时区**：写入侧**完全不做跨日处理**——桶只按 UTC 半时划分，`hour_start` 的日期部分永远是 UTC 日期。跨日语义全部在读侧，`rowDayKey`（`src/lib/local-api.js:213`）把 UTC 桶映射到展示时区的自然日：

```js
function rowDayKey(row, timeZoneContext) {                             // src/lib/local-api.js:213
  const hs = row.hour_start;
  if (!hs) return "";
  if (timeZoneContext && (timeZoneContext.timeZone || Number.isFinite(timeZoneContext.offsetMinutes))) {
    const parts = getZonedParts(new Date(hs), timeZoneContext);
    const key = formatPartsDayKey(parts);
    if (key) return key;
  }
  return hs.slice(0, 10);
}
```

`getZonedParts` 用 `Intl.DateTimeFormat("en-CA", { timeZone, hourCycle: "h23" })`（`src/lib/codex-rollout-parser.js:84-97`），失败时退化为 `offsetMinutes` 平移（`:103-110`）；没有时区上下文时**直接截字符串前 10 字符**（`:224`）。唯一在写入侧使用「本地日历日」的地方是 Codex 冷文件跳过（`formatLocalDate`，`src/lib/rollout.js:923` + `activeCodexRolloutDates`，`:893`）——比较的是**文件名里的日期**与本地日历日，属扫描调度，不影响桶。

### 4.4 桶累加器与 bucketKey

桶键的三个原语（`src/lib/rollout.js:4222-4231`）：

```js
function bucketKey(source, model, hourStart) {                         // src/lib/rollout.js:4222-4231
  const safeSource = normalizeSourceInput(source) || DEFAULT_SOURCE;
  const safeModel = normalizeModelInput(model) || DEFAULT_MODEL;
  return `${safeSource}${BUCKET_SEPARATOR}${safeModel}${BUCKET_SEPARATOR}${hourStart}`;
}

function projectBucketKey(projectKey, source, hourStart) {
  const safeSource = normalizeSourceInput(source) || DEFAULT_SOURCE;
  return `${projectKey}${BUCKET_SEPARATOR}${safeSource}${BUCKET_SEPARATOR}${hourStart}`;
}
```

三个常量：`DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`、`BUCKET_SEPARATOR = "|"`（`:55-57`）。配套的 `groupBucketKey(source, hourStart)`（`:4233-4236`，去掉 model 维度）与 `parseBucketKey(key)`（`:4238-4249`，按前两个分隔符切回三段）。`bucketKey` / `toUtcHalfHourStart` / `totalsKey` / `groupBucketKey` 都被显式导出给修复迁移与测试复用（`module.exports` 起点 `:24675`，尾部注释分组含这四个名字）。

`cursors.hourly` 的状态机（`:3627-3651`）：形如 `{version: 3, buckets, groupQueued, updatedAt}`；`normalizeHourlyState` 对 **version < 2 的旧桶会把模型名抹成 `unknown` 并把 `queuedKey` 提升为 group 级**，返回 version 3（`:399-409`）。这是历史遗留路径，与其配套的 `legacyGroups` / `groupQueued` 分支语义**【未能确认】**：已确认「有该键则走聚合分支」，但新装机会不会写入该键未验证——若不会，该分支对现有用户是死代码；若会，则存在与修复意图冲突的再膨胀路径（相关站点 `:3181-3194,3430-3480`）。判定条件：构造一个含 `groupQueued` 的 v1 游标 fixture，跑 `enqueueTouchedBuckets` 观察输出行数与模型名。

累加器的字段全集由 `initTotals()` 定义（`:4053-4065`），共 9 个数值字段：`input_tokens`、`cached_input_tokens`、`cache_creation_input_tokens`、`output_tokens`、`reasoning_output_tokens`、`total_tokens`、`billable_total_tokens`、`total_cost_usd`、`conversation_count`。外加 5 个**可选子集标记**（`:4073-4079`）：

```js
// Observed-subset markers, same shape as the session sidecar's long_context_*
// columns: how much of the columns above came from priority (Astra Fast) requests.
// They annotate the existing columns and are NEVER part of any sum — total_tokens
// stays input + output + cache_creation + cache_read + reasoning_output. They are
// omitted entirely when zero so no other provider's queue rows change.
const PRIORITY_SUBSET_FIELDS = [
  ["priority_input_tokens", "input_tokens"], ["priority_cached_input_tokens", "cached_input_tokens"],
  ["priority_cache_creation_input_tokens", "cache_creation_input_tokens"],
  ["priority_output_tokens", "output_tokens"], ["priority_reasoning_output_tokens", "reasoning_output_tokens"],
];
```
即：`priority_*` 是「上面这些列里有多少来自 priority（Astra Fast）档请求」的**观察子集标记**，永不参与任何求和，且值为 0 时整个字段不写出——这样其它 provider 的队列行不会因为新增这组列而变化（`prioritySubsetOf`，`:4090-4097`）。`total_cost_usd` 的累加带一个 $10^{-6}$ 级取整（`addTotals`，`:4111-4113`，`USD_TICKS_PER_USD`）。

### 4.5 队列行 schema 逐字段

主队列行的构造（`src/lib/rollout.js:3328-3345`）：

```js
        toAppend.push(JSON.stringify({            // src/lib/rollout.js:3328-3345
          source: group.source, model, hour_start: group.hourStart,
          input_tokens: totals.input_tokens, cached_input_tokens: totals.cached_input_tokens,
          cache_creation_input_tokens: totals.cache_creation_input_tokens,
          output_tokens: totals.output_tokens, reasoning_output_tokens: totals.reasoning_output_tokens,
          total_tokens: totals.total_tokens,
          billable_total_tokens: totals.billable_total_tokens ?? totals.total_tokens,
          total_cost_usd: totals.total_cost_usd || 0,
          usage_precision: usagePrecision || undefined,
          ...prioritySubsetOf(totals),
          conversation_count: totals.conversation_count,
        }));
```

| 字段 | 语义 | 注意 |
| --- | --- | --- |
| `source` | 内部 provider id（如 `claude`、`codex`、`every-code`） | 已由 `normalizeSourceInput` 归一 |
| `model` | 归一后的模型串 | 与 `source`、`hour_start` 一起构成读侧主键 |
| `hour_start` | UTC 半小时桶起点 ISO 串 | 无本地时区参与（§4.3） |
| 五桶（`input_tokens`/`cached_input_tokens`/`cache_creation_input_tokens`/`output_tokens`/`reasoning_output_tokens`） | 明细 | **计费输入是这五列之和**；`total_tokens` 永不参与计费 |
| `total_tokens` | 总量 | 累计口径由各 provider 自己定义（注释口径为 `input + output + cache_creation + cache_read + reasoning_output`） |
| `billable_total_tokens` | 计费口径总量 | 字段缺失时回落 `total_tokens`（`??`）；读侧对 cursor/antigravity 会做提升纠正（§3.6） |
| `total_cost_usd` | 已算好的费用 | 缺失回落 `0`；前端零重算 |
| `usage_precision` | 精度标记，`undefined` 时不写出 | **取值域与消费方【未能确认】**——它同时参与 `queuedKey` 计算（`:3325-3326`），因此会影响「同键覆盖」的判定 |
| `priority_*`（最多 5 个） | priority 档观察子集 | 值 > 0 才写出（§4.4） |
| `conversation_count` | 会话计数 | **同一列名在两条主线含义不同**：Codex 每条被接受的 delta 恒写 1（`:2554`），Claude 的 assistant 行恒 0、只有主会话 user 文本行 +1（`:2865`、`:2807-2831`） |

另有**控制行**：`kind: "account_session_state"`（云端会话真值，走 `snapshot_verified_at` 严格更新比较）与 `kind: "account_sync_watermark"`（已废弃，读侧显式丢弃，`src/commands/sync.js:4497`）。

**项目队列行是主行的裁剪版本，不是同构**（`src/lib/rollout.js:3515-3532`）：比主行**多** `project_ref` + `project_key` 两个前导字段，同时**少三类列**——`model`（项目视图不按模型细分）、`usage_precision`（主行在 `:3341` 写入）、`...prioritySubsetOf(totals)` 展开的 `priority_*`（主行在 `:3342` 展开）。token 桶、`billable_total_tokens`、`total_cost_usd`、`conversation_count` 与主行同源；`project_ref`/`project_key` 缺失的项目桶在 `:3513` 直接跳过。

两套读侧 key 与各自的行字段**一一对应**（这是「同键后写覆盖前写」的前提）：

```js
// src/lib/local-api.js:202-211（queue.jsonl）
    if (row?.kind === "account_session_state" || row?.kind === "account_sync_watermark") return null;
    return { key: `${row.source || ""}|${row.model || ""}|${row.hour_start || ""}`, row: normalizeQueueRow(row) };
// src/lib/local-api.js:145-153（project.queue.jsonl）—— key 不含 model
      key: `${row.project_key || ""}|${row.source || ""}|${row.hour_start || ""}`, row: normalizeQueueRow(row),
```

两个 reader 的 `key` 都**不含 token 值**，所以后写的行覆盖先写的行（`seen.set(key, row)`，`src/lib/async-queue-reader.js:55,86`）。日聚合则是**跨模型求和**（`buildDailyAggregates`，`src/lib/local-api.js:260-298`）。

### 4.6 写入幂等、撤回与模型对齐

**幂等机制：`queuedKey` 指纹比对。** 写队列前，累加器为每个桶算一个指纹，与桶上记着的 `queuedKey` 比较，相等就跳过：

```js
        const usagePrecision = bucket.usage_precision || null;
        const key = usagePrecision ? `${totalsKey(totals)}|${usagePrecision}` : totalsKey(totals);
        if (bucket.queuedKey === key) continue;
```

指纹本身就是「整行快照」的序列化（`src/lib/rollout.js:4155-4171`）：

```js
function totalsKey(totals) {                                           // src/lib/rollout.js:4155-4171
  const base = [
    totals.input_tokens || 0, totals.cached_input_tokens || 0, totals.cache_creation_input_tokens || 0,
    totals.output_tokens || 0, totals.reasoning_output_tokens || 0, totals.total_tokens || 0,
    totals.billable_total_tokens ?? totals.total_tokens ?? 0,
    totals.total_cost_usd || 0, totals.conversation_count || 0,
  ].join("|");
  const priority = PRIORITY_SUBSET_FIELDS.map(([field]) => Number(totals[field]) || 0);
  // Appended only when a priority subset exists, so buckets that never see the
  // tier keep their previously persisted queuedKey and are not re-enqueued.
  return priority.some((value) => value > 0) ? `${base}|p:${priority.join(",")}` : base;
}
```

写成功后 `bucket.queuedKey = key`（`:3346`）**记在内存游标上**，`cursorStore.commit()` 时才落盘。由此推出两个必须记住的后果：① 若在 `appendFile` 之后、commit 之前进程被杀，下一次 sync 会重新扫到同一批数据并再次 append（重复行），幂等由读侧「同键最新覆盖」与云端整行 upsert 兜住；② 指纹包含 `total_cost_usd` 与 `conversation_count`，所以「费用变了但 token 没变」也会触发重写行。

**撤回行（retraction）**。同一函数内有三种零值行，作用都是**声明式的撤回**而不是删除：

1. **unknown 桶撤回**（`:3291-3316`）：当某 `(source, hour)` 分组存在 `model = unknown` 的桶、且它此前写过一行（`queuedKey` 非零值指纹），就追加一行 **token 全 0 的 `model: "unknown"` 行**。

```js
      if (unknownBucket && !alignedModel && unknownBucket.queuedKey && unknownBucket.queuedKey !== zeroKey) {
        if (unknownBucket.retractedUnknownKey !== zeroKey) {
          toAppend.push(JSON.stringify({ source: group.source, model: DEFAULT_MODEL,
            hour_start: group.hourStart, /* 九个数值字段全零 */ }));
          unknownBucket.retractedUnknownKey = zeroKey;
        }
      }
```    `retractedUnknownKey` 去重避免每次 sync 都补写；读侧按同键取最后一条，于是历史 unknown 行被这一行零值覆盖——**不需要改动已写出的历史行**。
2. **对齐模型撤回**（`:3273-3290`）：当桶上记录的 `alignedModel` 指向的模型已不在本分组内时，补发一行该模型的零值行——把「曾经对齐过去、现在不成立了」同样表达成覆盖。
3. **整源撤回**（`retractStaleSourceQueueRows`，`:19990`，写点在 `:20031`）：按 provider 维度清理不再成立的行，属修复路径而非每轮路径。

**`unknown` → `dominant` 模型对齐**由三个函数组成，全部在 `enqueueTouchedBuckets` 内部触发：`pickDominantModel(buckets)`（`:3542-3558`）在**同一 `(source, hourStart)` 分组内**取 `total_tokens` 最大的非 unknown 模型（并列取字典序小的）；`collectCodexDominantModels(hourlyState)`（`:3566-3607`）从整个 hourly 状态收集 **source === `codex`** 的各小时 dominant 模型（按小时把同一模型的 total 求和后取最大）；`findNearestCodexModel(hourStart, dominants)`（`:3609-3625`）按时间距离找最近一小时的 codex dominant 模型。触发逻辑（`:3258-3360`）：若该分组存在 unknown 桶且存在 dominant 模型，则把 unknown 桶的 totals **并入 dominant 行**（`cloneTotals` 后 `addTotals`，`:3320-3324`），并清掉桶上的 `alignedModel`（`:3317`）；若该分组**只有 unknown（没有 dominant）**、且 `source === "every-code"`，则用 `findNearestCodexModel` 找最近的 codex dominant 模型作为输出模型（`:3353-3356`）——这正是「every-code 的会话被标成非模型名」这类历史问题的收口方式。注意 `collectCodexDominantModels` 只认 `source === codex`（`:3574`），所以这不是通用的跨源推断。

**增量范围**：只有本轮被 touch 的 `(source, hourStart)` 分组参与重算（`if (!recomputeGroups.has(groupKey)) continue;`，`:3243`），因此撤回与对齐都是**增量式**的，不做全表重写。

**写路径是「append + 原子重写」两条，各自多个实现点**：append 主队列 `queue.jsonl` 至少 **6 处**（`src/lib/rollout.js:3483` 的 `enqueueTouchedBuckets`、`:20031`、`:22594`；`src/commands/sync.js:5126`、`:5207`、`:5281`），另有 `src/commands/sync.js:4114` 写 fallback 文件、`src/lib/rollout.js:3536` 写 `project.queue.jsonl`；原子重写整文件见 `compactPendingQueue`（`src/lib/upload-queue.js:64-132`）与各类 repair（如 `repairClaudeQueueFromGroundTruth` 全文重写，`src/commands/sync.js:6844`）。**没有针对 `queue.jsonl` 的文件锁**——互斥完全依赖外层 `sync.lock`（`src/commands/sync.js:552-559`），这要求「所有会把行写进队列的进程都必须是 sync 进程」；仓库内 `local-api.js` 触发的是 `spawn(tracker.js sync …)`（`src/lib/local-api.js:742-764`），符合该前提。

### 4.7 为什么「追加累计快照 + 读端 last-row-wins」是核心

这套设计的全部取舍都能从三个事实推出：

**事实一：写端只追加，不做原地修改。** 每一行都是某个 `(source, model, hour_start)` 桶在**写入那一刻的累计快照**（不是 delta），同键可以出现多行（重算、撤回、崩溃窗口都会产生）。写端唯一要保证的是「这一行必须是完整的一行」：`fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8")`，一次调用写完一批（`src/lib/rollout.js:3483`）。

**事实二：读端用同键覆盖消化重复。** 两个 reader 的 key 都不含 token 值，`seen.set(key, row)` 让后写覆盖先写（`src/lib/async-queue-reader.js:55,86`）；上传批次同样用 `bucketMap.set(...)` 覆盖（`src/commands/sync.js:4510`）；云端是**按桶键整行 upsert**（`src/commands/sync.js:249-256` 注释）。三处口径一致，于是「同一行被写多次」与「同一行被重传」都自然幂等。

**事实三：写者崩溃不需要任何补偿逻辑。** 因为游标提交在行写入之后（§2.3），崩溃只可能产生「多写一行」，不可能产生「少写一行」——多写的那一行要么与已有行同值（被覆盖，无影响），要么是更旧的值（被更新的行覆盖）。而「少写」才是需要恢复的病态，这里被写序排除了。

代价与边界同样清楚：**读端必须全部遵守 last-row-wins**，任何「求和」式消费者都会把重复行算两遍——仓库内已有一处**口径不同**的对账实现 `mainUsageHourTotalsFromQueueRows`，它用 `setMaxTotal(key, total_tokens)`（**取最大值**，`src/commands/sync.js:6540`）、再由 `collapseModelTotalsByHour` 跨模型求和（`:6564-6571`），两者是否可能给出不同结果取决于队列里是否会出现「同键数值回退」（真值修复是原子重写，理论上不会）——【未能确认】该函数的所有调用点是否都作用于「已被重写过的干净队列」，判定条件：枚举其调用点并逐步确认输入队列的来源。**压缩必须避开上传水位**（§3.8），**同键写入后的读端可见性依赖行边界**（§3.6、§3.8 的完整行纪律）。

一句话概括：**这套管线用「整行累计快照 + 追加写 + 读端覆盖」换掉了「写端互斥 + 崩溃恢复 + 重放去重」三件难事**，代价是把正确性要求前移到三处纪律——不完整行永不提交、读端永不求和、压缩永不越过已上传 offset。
