# TokenTracker 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`xiufengsun/TokenTracker`](https://github.com/xiufengsun/TokenTracker)，
> HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14，提交时间 2026-10-13 21:49:05 +0800，
> npm 包名 `tokentracker-cli`）。浅克隆到临时目录后逐文件精读，**未运行其程序、未执行仓库内脚本、
> 未安装依赖、未读取任何真实会话数据、未改动本机 `~/.tokentracker`**。
>
> 证据分档：**已验证**（源码可证，且行号经至少一轮对抗式复核）／**一致解释**（多条代码证据指向同一结论，
> 无单点直证）／**未验证**（结构推断或需运行时/语料才能判定；全文统一写成 `【未能确认】` 并附判定条件）。
>
> 行号口径：一律为**文件实际行号（按 `\n` 分行、UTF-8 解码，与编辑器/`read` 工具一致）**。
> 特别注意：PowerShell 5.1 的 `Get-Content`（不指定编码）在中文环境按 GBK 解码，本仓库有多行以
> 多字节字符结尾，会把这些行尾字节与随后的换行一起吞掉，导致**行号整体偏移（单文件最多 15 行；全 `src/` 累计受影响 52 行）**。
> 若你要自行核对本文行号，请用 `[System.IO.File]::ReadAllLines(path,[Text.Encoding]::UTF8)`
> 或 `read` 工具；本文所有引用均已按该口径校验。
>
> 本文只记录字段名、表名、路径规则、计算口径与代码结构，不含任何真实会话内容。

## 本文是怎么做出来的（复核流程与可信度）

本文不是单轮生成的产物，而是**研究 → 对抗式复核 → 修正 → 再复核**的闭环结果：

1. **分片研究**：8 份分片（`docs/research/tokentracker-draft/S0…S6`）分别覆盖采集管线、解析框架、
   45 家 provider 读取方式、聚合与去重、定价与金额、消费端输出，每条结论都要求给出 `文件:行号` 与源码摘录。
2. **第一轮对抗式复核（R1）**：6 个独立复核 agent 按统一靶子清单（`REVIEW-CHECKLIST.md`）逐条回源码证伪，
   抽验合计 **772 条**断言，报出 **E=50（错误）／I=43（不完整）／U=29（无法验证）**，
   其中包含若干**结论级**推翻（例如「KiroCli 全部靠字符估算」被证伪、「三族返回签名」实为 11 种键集合、
   「OpenCode 会拒绝覆盖外来文件」不成立）。
3. **修正轮**：6 个修正 agent 逐条回源码修正，并在每份分片末尾留下「修正记录」表（编号／原表述／修正后／依据行号）。
   修正过程中还反向纠正了复核报告自身的若干误判（例如把 `recordsProcessed: total` 误读为新增键）。
4. **第二轮独立复核（R2）**：另一组独立 agent 复查修正点、做全新抽样（含跨文件一致性），
   合计检查 **306 项**，报出 **E=15／I=24／U=14**；随后又做了一轮对应修正（R2 修正），
   其中 1 条复核结论被修正方**反向否定**并附源码证据（见 §13.3）。
5. **本文**：在上述基础上分块合成（§1–4／§5／§6／§7–8／§9／§10–13＋附录），
   并按「已验证／一致解释／未验证」重新标注。所有被 R1/R2 判为错误的内容已从正文剔除或改写；
   仍无法判定的内容保留在 §12「未验证项与风险清单」，并写明判定条件。
6. **第三轮复核（R3）**：对**本文正本文本**再做一次独立抽样复核（行号命中率、是否引入分片外的新结论、
   附录索引准确性），四个复核单元合计 478 条/点，命中率 88.9%–99.5%，问题**全部落在引用与计数层**
   （无机制性错误、无幻觉）；修正后进入末轮验收（§13.5–§13.6）。
7. **末轮验收复核（R4）**：两个独立复核单元验证「R3 修正是否真的落到文本里」并做全新抽样——
   修正点验收 29/29 与 31/32 通过，全新抽样命中率 94.0% 与 97.7%，只剩 7 处引用级校准，全部已修正（§13.7）。

读者可以据此判断每一条结论的可信度层级。**凡本文与源码冲突，以源码为准**（仓库为移动目标，本文锚定上述 HEAD）。

## 目录

1. 范围与方法
2. 总体架构与数据流
3. 采集管线：触发、调度、游标
4. 解析层：统一骨架与队列契约
5. 45 家 provider：逐家读取方式
6. Claude Code 与 Codex 深度解剖
7. 聚合、去重与时间语义
8. 金额计算全链路
9. 消费端：CLI、本地 API、Dashboard、原生壳、云
10. 与 TokenScope 的对照：可抄与需避
11. 关键常量与阈值速查
12. 未验证项与风险清单
13. 复核记录（R1–R4 问题台账与修正结果）
- 附录 A：45 个解析器行号索引
- 附录 B：术语表

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

样本源码含中文注释，其中有**一批行以 ≥0x81 的字节结尾**（行尾是多字节字符的末字节）。PowerShell 5.1 的 `Get-Content`（不指定编码）按 GBK 解码，把这些行尾字节与其后的 `0x0A` 一起吞掉，导致**总行数变少，且从第一个此类行之后的行号整体偏移**。本轮逐文件实测（默认编码行数 vs UTF-8 行数）：全 `src/` 共 **52 行**受影响，**单文件最多 15 行**——`src/lib/rollout.js` 差 15（正是本仓库受影响最多的文件）、`src/lib/local-api.js` 差 6、`src/commands/sync.js` 差 5、`src/lib/session-analytics.js` 差 4，其余 16 个文件各差 1–3 行。**「15」不是全库漂移量，只是最坏单文件**；且漂移量随位置累积（越靠文件末尾越大）。以 `src/lib/rollout.js` 实测：

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

provider 总名单是 `SUPPORTED_PROVIDERS`（**45 项**，`src/commands/init.js:119-165`），按 `applyIntegrationSetup`（`:674-1149`）的处理方式拆解为 **45 = 写配置 12 + 仅探测 1 + 纯被动读取 17 + 仅出现在欢迎名单 15**（四类互斥、并集恰为 45 项，本轮按 `init.js` 逐项枚举复核）。**口径先说清**（此处最易误读）：`写配置` = 分支内真的调用 `upsert*` / `installOpenclawSessionPlugin` 改动用户配置或写盘；`仅探测` = 只读探测后报 detected/skipped；`纯被动读取` = 各自有 `if (exists)` 包装，但**分支内只 `push` 一条摘要行（默认 `status: "detected"`；`cline` 的发现失败会另推一条 `status: "error"`）、不写盘**——「没有写盘分支」不等于「没有 `if` 分支」，MiniMax Code(`:851-856`)、Command Code(`:860-865`)、Craft Agents(`:870-875`)、Reasonix(`:878-883`) 四家都在这一类，**不在**欢迎名单类。逐类名单：

写配置 12（名单按行号顺序，与括号内行号一一对应）：Codex CLI、AStudio、Claude Code、Gemini CLI、OpenCode、oh-my-pi、Grok Build、CodeBuddy、WorkBuddy、WorkBuddy AI、OpenClaw session plugin、Every Code（`:686-702,704-720,722-731,733-742,744-756,789-828,901-916,1037-1046,1050-1059,1062-1071,1073-1118,1130-1146`）。仅探测 1：Cursor（`:758-776`，只报 detected/skipped）。纯被动读取 17：Kimi Code、pi、Prime Agent、MiniMax Code、Command Code、Craft Agents、Reasonix、TRAE、Kilo CLI、DuMate、Mimo、LM Studio、Unsloth Studio、AnythingLLM Desktop、Devin CLI、Kilo Code VS Code 扩展、Cline（`init` 只打印 detected/reader，不装 hook）。仅欢迎名单 15：Antigravity、Kiro、Hermes Agent、GitHub Copilot、OmO、Dots、Roo Code、Zed Agent、Goose、Droid、ZCode、Qoder、Claude Science、DeepSeek Harness、TRAE Work CN——这 15 家在 `applyIntegrationSetup` 内**没有任何 `summary.push` 分支**，只出现在 `SUPPORTED_PROVIDERS` 与欢迎屏的计数/名称预览里（`:274-276`）。OpenClaw 的 legacy hook 移除（`:1120-1128`）是**同一个 provider** 的附加清理动作，不构成独立类别；`12 + 1 + 17 + 15 = 45`，与 `SUPPORTED_PROVIDERS` 项数闭合。

**幂等范式（四类 + 一例外）**：

1. **Claude 范式**：`hasHook` 结构性比对（command + `shell` 一致性，`src/lib/claude-config.js:143-156`）；已存在即完全不写盘（`if (!changed) return {changed:false}`，`:39`）；`normalizeEntriesForCommand` 就地修复旧形态并按 matcher scope 去重（`:181-230`）。
2. **Codex 范式**：TOML 数组严格等值比较（相等即 `changed:false`，`src/lib/codex-config.js:76,107`）；「是否我们写的」用 `isManagedNotifyCmd`——数组内存在以 `notify.cjs` 结尾且路径含 `/.tokentracker/` 的项、且尾参一致（`:40-58`）。TOML 是**手工行编辑**：`extractNotify` 只找首个表格头之前的 `notify =` 行（`:245-269`），`setNotify` 原位置替换、找不到则插到第一个 `[table]` 前（`:271-309`）。
3. **哨兵范式（全仓只有 omp）**：扩展文件首写标记串 `// @tokentracker-managed-omp-extension`（`src/lib/omp-hook.js:17`）；**带标记 → 覆盖，无标记 → 拒绝写入**（`skippedReason: "unmanaged-extension-present"`，`:388-395`，该字符串全仓仅此一处产生，消费方 `src/commands/init.js:813`）。
4. **内容等值短路 + 无条件备份覆盖（OpenCode）**：`upsertOpencodePlugin` 先比对内容，`if (existing === next) return { changed: false }`（`src/lib/opencode-config.js:73-75`）；内容一旦不等，无论原文件是什么都**先备份再覆盖**（`:80-85`），**没有**「无标记则拒绝写入」的分支；标记检测只出现在卸载路径，内容不匹配时返回 `skippedReason: "unexpected-content"`（`:94`）。**OpenCode 不属于哨兵范式**。
5. **例外（Grok）**：不做任何幂等判断，每次 init 都重写 hook JSON 与 handler，`upsertGrokHook` 恒返回 `changed:true`——理由是「bin 路径可能变化，始终覆盖以保持命令最新」（`src/lib/grok-hook.js:85-87,307-317`），且**没有备份**（`:85-91`）。

备份策略：Claude/Gemini 复制 `settings.json.bak.<ISO>`（`src/lib/claude-config.js:248-262`、`src/lib/gemini-config.js:265-279`）；Codex/acode/every-code 复制 `config.toml.bak.<ISO>` 并**先**把原 notify 存到 `<trackerDir>/{codex,acode,code}_notify_original.json`（`src/lib/codex-config.js:89-104`）；OpenCode 备份旧插件（`src/lib/opencode-config.js:79-83`）；omp 靠哨兵串 + **inode 身份校验**保护——`writeManagedThroughHandle` 在写入前后各取一次 `pathIdentity` 并用 `sameFileIdentity` 比对，路径被换掉就不算写成功（`src/lib/omp-hook.js:222-281`）；**OpenClaw 的保护不是哨兵串也不是 inode 校验**，而是 `openclaw.json` 的 `plugins.entries` 策略（`ensureOpenclawSessionPluginPolicy`，`src/lib/openclaw-session-plugin.js:180` 起，负责 `enabled: true`、补 `hooks` 与 `allowConversationAccess`），同文件 `:154-178` 是**状态探测**（`enabled` / `linked` / `installed` / `pluginFilesReady` / `conversationAccess` 五个布尔量的计算），不写盘。回滚全部集中在 `uninstall`（`src/commands/uninstall.js:76-137`），还原时用 `isManagedNotifyCmd` 把关：不是我们写的 notify 返回 `current-not-managed` 且不还原（`src/lib/codex-config.js:110-133`）。

触发粒度差异（直接决定新鲜度语义）：**Codex 系（codex/acode/every-code）走 `notify` 回调**，逐 turn 触发，这条路径**独占用户的 `notify` 槽位**，所以必须备份原 notify 并链式调用。**Claude 系走 `hooks.Stop` + `hooks.SessionEnd`**，不争槽位，但 `Stop` 的触发粒度在仓库内只有事件字符串、没有语义注释（`src/lib/claude-config.js:7-8`），本地源码判不出它是否严格等于「每轮」——**【未能确认】**。**Gemini 走 `SessionEnd` + matcher**，默认 matcher 是 `exit|clear|logout|prompt_input_exit|other`（`src/lib/gemini-config.js:10`），**只有特定退出原因才触发**，且安装时会强制 `tools.enableHooks = true`（`:147-152`），即会改动用户的工具开关。**OpenCode 的注释明确说明**「opencode 行本来就能从磁盘 storage 与 opencode.db 被动解析，hook 只是让 sync 更早发生」（`src/lib/opencode-config.js:55-56`）。

安装失败的降级（`passive mode`）：判定三条件为「hook 期望、hook 未装上、日志在」（`src/lib/passive-mode.js:189-204`）；`dirHasFile` 只做**至多一层**浅递归且把 readdir 失败当 false 吞掉（`:49-68`）；失败原因只区分 `ENOENT`/`EACCES`/`EROFS` 与其它（`:70-81`）；覆盖 8 个 hook 驱动 provider（claude、gemini、codex、acode、every_code、codebuddy、workbuddy、workbuddy-ai，`:96-185`），Grok/OpenCode/OpenClaw 不在名单内。**关键澄清**：`passive-mode.js` 与 `init.js` 之间**没有任何代码调用关系**——全仓引用它的只有 `status.js`（`status.js:44` require、`:1021` 调用、`:1248` 输出、`:1448,1688` 呈现），所以它只是 status 层的补偿性检测与告知，init 不会因 hook 装不上而走别的路径。模块头注释的语义边界：**hook 缺失永不导致 sync 失败**，只把「秒级新鲜」降级为「分钟级新鲜」（`src/lib/passive-mode.js:22-25`）。

### 3.3 `sync` 编排

`cmdSync` 单函数体从 `src/commands/sync.js:516` 延伸到 `:3709`（3194 行），`module.exports` 在 `:3821-3857`，其后 `:3859-7251` 全是 repair/migration/helper。主流程按执行顺序：

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
| ③ 文件级 | Claude：`fileId = claudeFileIdentityHash(path, st)`（前 64 KB sha256 + size）；**只有「该文件从无游标（`!prev`）且本运行内该 `fileId` 已被先到的文件认领」时**，才标 `duplicateOf` 并把 offset 直接推到 `st.size`（`:1073-1081`）。**已有游标的重复文件不走这条**——它仍按正常增量解析，重复计数由第 ④ 层的消息哈希兜底 | `src/lib/rollout.js:937,1056-1105` |
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

损坏恢复：读取侧先试 `manifest.current`，失败后才试 `manifest.rollback || manifest.previous`（`src/lib/cursor-store.js:124-128`）——**这是两级回退而不是三级依次回退**：`||` 是逻辑或，`rollback` 有值时 `previous` 根本不会被尝试，`previous` 只在 `rollback` 为空（首次落盘）或 manifest 属别人的旧代时才轮到；`openManifestGeneration` 失败抛 `TOKENTRACKER_CURSOR_STORE_CORRUPT`（`:72-74`）；每个分片写盘时把 `serialized` 摘要与 count 记进 metadata（`buildShardIntegrity`，`:1466-1471`），读取时 `assertShardIntegrity` 校验（`:1473-1481`）。**「可重试」与「损坏」的分野**：分片缺失导致的 `TOKENTRACKER_CURSOR_STORE_RETRY` 是「加载缺失分片后重试」，sync 捕获后会 `loadCodexFilesForPaths` 再跑一次全量解析（`src/commands/sync.js:1069-1085`）；`CORRUPT` 直接上抛，不做静默修复（`:1086-1087`）。`legacyFingerprint` 会检测 `cursors.json` 在 v2 建立之后是否被外部改动，一旦改动就重新迁移（`src/lib/cursor-store.js:44-58`）。

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

三态是 `{exists:true}` / `{exists:false,error:null}`（真不存在）/ `{exists:false,error:"EACCES"}`（存在但读不了）；第三态由 `describeScanRootState` 渲染成 `" (unreadable: <code>)"`、与 `" (missing)"` 区分（`src/lib/scan-roots.js:248-253`）——注意这是**渲染与意图**：`:89-96` 的注释把「应当区别于 missing」写成调用方意图（`callers can say "unreadable" instead of "missing"`），函数本身**只返回字符串、没有强制点**（是否用上取决于调用方是否调 `describeScanRootState`）。理由：只做 `stat` 时，一个权限 000 的目录会被判为「存在」，进扫描后产出 0 个文件、状态页显示「存在」，并**绕过 deferred-repair 守卫**（守卫语义是「根不存在就不许重算历史」）。

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

**两处独立实现**（全库 `function toUtcHalfHourStart` 仅 2 命中，不是共享 import）：主实现 `src/lib/rollout.js:4173`（被 `module.exports` 导出，所有增量 parser 使用）；副本 `src/lib/claude-categorizer.js:1175`（Claude 真值修复专用）。二者关系是「**语义等价、文本不同**」：取整规则完全相同，但前者分步构造 `bucketStart` 后再 `toISOString()`，后者写成单行链式返回。**没有任何测试或断言锁定二者的等价性**（指「两份实现语义等价」这一关系本身；主实现自己有定向测试——`test/trae-entitlement.test.js:29` 从 `../src/lib/rollout` 导入，`:130-137` 三条断言钉死 `:00`/`:30` 取整）：`test/` 下引用 `toUtcHalfHourStart` 的两个测试都 `require("../src/lib/rollout")`（`test/sync-droid-dup-session-repair.test.js`、`test/trae-entitlement.test.js:130-137`），**`claude-categorizer.js` 的那一份副本无人测**；改动任一处不会让另一处失败。一旦真值修复与增量解析分叉，队列里会出现两套时间分桶。

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

`cursors.hourly` 的状态机（`:3627` 起的 `normalizeHourlyState`）：形如 `{version: 3, buckets, groupQueued, updatedAt}`；`normalizeHourlyState` 对 **version < 2 的旧桶会把模型名抹成 `unknown` 并把 `queuedKey` 提升为 group 级**，返回 version 3（该 v<2 分支体在 `src/lib/rollout.js:3634-3652`；`:399-409` 是 `syncDiagnostics` 字段赋值区，与本函数无关）。这是历史遗留路径，与其配套的 `legacyGroups` / `groupQueued` 分支语义**【未能确认】**：已确认「有该键则走聚合分支」，但新装机会不会写入该键未验证——若不会，该分支对现有用户是死代码；若会，则存在与修复意图冲突的再膨胀路径（相关站点 `:3181-3194,3430-3480`）。判定条件：构造一个含 `groupQueued` 的 v1 游标 fixture，跑 `enqueueTouchedBuckets` 观察输出行数与模型名。

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

## 5. 45 家 provider：逐家读取方式

本章只回答一个问题：**TokenTracker 到底从哪里、以什么格式、按什么口径把各家的 token 读出来**。所有结论都锚定 `src/lib/rollout.js`（`parse*Incremental` 的唯一真源，共 45 个函数定义，行号区间 `251`–`24458`）与各家的专用模块（`sqlite-reader.js`、`cursor-config.js`、`trae-*.js`、`devin-usage.js`、`grok-usage.js`、`dumate.js`、`install-resolver.js`）。读者可只看 §5.1 的总表，再按需下钻；§5.6 是给下游（TokenScope 侧）的价值最高的一节。

**行号口径**：全部为文件实际行号（UTF-8、按 `\n` 分行，与编辑器 / `read` 工具一致）。本仓库有多行以多字节字符结尾，PowerShell 5.1 的 `Get-Content`（不指定编码）按 GBK 解码会吞掉换行，导致行号漂移——**单文件最多漂移 15 行（`rollout.js`，全 `src/` 累计受影响 52 行；§1.3 有逐文件实测表）**，所以 **不要用 `Get-Content` 复核本章行号**，请用 `[System.IO.File]::ReadAllLines(path,[Text.Encoding]::UTF8)`。

---

### 5.1 总表（最重要）

**读表说明**：`source` 列写的是**真正进队列桶键的字面量**（`getHourlyBucket` / `bucketKey` 的 `source` 段），不是 provider 显示名——两者有 **8 处错位**（逐条可回源，重叠形态见 §5.1.1）：Kimi Code→`kimi`、Kiro CLI→`kiro`、OpenCode 两条支路（磁盘 / SQLite）共用 `opencode`、Qoder 新旧两套共用 `qoder`、WorkBuddy 一行含 `workbuddy`/`workbuddy-ai` 两个 source、Copilot 三通道共用 `copilot`、Codex 家族一行三 source、pi 与 Prime Agent 按 `provider` 字段派生 `pi-<provider>`/`prime-agent-<provider>`。「hook/插件」列只回答「有没有主动上报通道」；**没有任何一家把 hook 当作解析前提**：hook 只让同步更早发生（`src/lib/passive-mode.js:4` 起模块头明确写「解析器可以直接读那些日志，唯一损失只是‘数据在数秒内是新鲜的’」），唯一例外是「上游要用户先开导出才有文件可读」的 Copilot OTEL 与 trae-cn 的强制 opt-in（后者另计）。

| # | 显示名 | `source` 字面量 | 读取方式 | 数据路径 | hook / 插件 | 联网 / 凭据 | 关键字段映射（一句话） |
|---|--------|------------------|----------|----------|-------------|-------------|------------------------|
| 1 | Gemini CLI | `gemini` | 整份 JSON（`session-*.json`） | `~/.gemini/tmp/<project>/chats/session-*.json`（`GEMINI_HOME` 可覆盖） | 有（`gemini-config.js` 的 hook，仅新鲜度） | 离线 | `tokens.input`/`cached`/`output`+`tool`/`thoughts`；`total=max(上报, 分桶和)` |
| 2 | OpenCode（磁盘消息支路） | `opencode` | 单消息 JSON 目录 | `$OPENCODE_HOME`（默认 `$XDG_DATA_HOME/opencode`）`/storage/message/**/msg_*.json` | 无 | 离线 | `tokens.input`/`output`/`reasoning`/`cache.read`/`cache.write`，input 已是纯非缓存 |
| 3 | OpenClaw | `openclaw` | JSONL 转录（+ agent SQLite） | `~/.openclaw/agents/<agent>/sessions/*.jsonl` | 有插件（`openclaw-session-plugin` / `openclaw-hook`） | 离线 | `message.usage.input`（先减 `cacheRead`）/`cacheRead`/`cacheWrite`/`output` |
| 4 | Kiro（IDE） | `kiro` | SQLite（另有 JSONL 兼容支路） | `%APPDATA%\Kiro\User\globalStorage\kiro.kiroagent\dev_data\devdata.sqlite` | 无 | 离线 | `tokens_prompt`→input、`tokens_generated`→output；缓存/推理列恒 0 |
| 5 | Hermes | `hermes` | SQLite | `~/.hermes/state.db`（Windows 另探 `%LOCALAPPDATA%\hermes`；含 `profiles/*/state.db`） | 无 | 离线 | 列名即桶名；`message_count` 差分 → `conversation_count` |
| 6 | Kiro CLI | `kiro`（与 IDE 合并） | SQLite（字符估算）+ legacy 会话 `.json`（真实值）+ v2 `messages.jsonl`（估算） | `~/.kiro/kiro-cli/data.sqlite3` + `~/.kiro` 下的会话文件 | 无 | 离线 | legacy 优先 `input_token_count`/`output_token_count`，皆为 0 才回退字符数 ÷ 4 |
| 7 | Kimi | `kimi` | JSONL `wire.jsonl` | `~/.kimi/sessions/<workspace>/<session>/wire.jsonl` | 无（源码声明不需要） | 离线 | `token_usage.input_other`/`input_cache_read`/`input_cache_creation`/`output` |
| 8 | Kimi Code | `kimi`（与 Kimi 同源） | JSONL `wire.jsonl`（递归 ≤5 层） | `~/.kimi-code/sessions/**` | 无 | 离线 | `usage.inputOther`（或 Anthropic / OpenAI 两种旧形状）；reasoning 恒 0 |
| 9 | CodeBuddy | `codebuddy` | JSONL + IDE 扩展日志（双源） | `~/.codebuddy/projects/**/*.jsonl` + IDE 日志根 | 有（Claude 系 `settings.json` hook） | 离线 | 多镜像优先级取 input/cache；`reasoningTokens` 独立桶 |
| 10 | WorkBuddy（含 WorkBuddy AI） | `workbuddy` / `workbuddy-ai`（按 variant） | JSONL + traces JSON + SQLite（三级） | `~/.workbuddy{,-ai}/projects/**/*.jsonl`（+`traces/`） | 有（两个 variant 各装一次） | 离线 | `rawUsage.prompt_tokens−缓存` → input；`completion−reasoning` → output |
| 11 | Roo Code | `roocode` | `ui_messages.json`（IDE 任务目录） | `<IDE 根>/User/globalStorage/rooveterinaryinc.roo-cline/tasks/**` | 无 | 离线 | `payload.tokensIn`/`tokensOut`/`cacheReads`/`cacheWrites` |
| 12 | Cline | `cline` | 会话转录 JSON | `~/.cline/data/sessions/**`（Windows 双安装 / WSL 探测） | 无 | 离线 | `metrics.inputTokens−缓存`、`cacheRead/WriteTokens`、`reasoningTokenCount` 只上报；**唯一解析器自产 `cost`** |
| 13 | Zed | `zed` | SQLite BLOB（`json` / `zstd`） | `~/.local/share/Zed/threads/threads.db`（macOS `~/Library/Application Support/...`） | 无 | 离线 | 线程级 `input_tokens`/`output_tokens`/`cache_read`/`cache_creation` 四元组差分 |
| 14 | LM Studio | `lmstudio` | 文本日志内嵌 JSON | `~/.lmstudio/server-logs/**/*.log` | 无 | 离线 | `prompt_tokens`/`completion_tokens`/`total_tokens` + `*_details.cached_tokens`；input 由 total 反推 |
| 15 | Unsloth Studio | `unsloth` | SQLite | `~/.unsloth/studio/studio.db` | 无 | 离线 | `prompt_tokens`/`completion_tokens`/`total_tokens`（共用本地推理归一化器） |
| 16 | AnythingLLM | `anythingllm` | SQLite | `%APPDATA%\anythingllm-desktop\storage\anythingllm.db` | 无 | 离线 | `response.metrics.prompt_tokens`/`completion_tokens`；**差额被判为 reasoning** |
| 17 | Goose | `goose` | SQLite | `%APPDATA%\goose\sessions\sessions.db`（macOS/Linux 走 `Application Support` / `XDG`） | 无 | 离线 | `accumulated_input/output/total_tokens` 差分；`model_config_json.model_name` 缺则整条丢弃 |
| 18 | Droid (Factory) | `droid` | `*.settings.json` | `~/.factory/sessions/**/*.settings.json`（`DROID_SESSIONS_DIR` 可覆盖） | 无 | 离线 | `tokenUsage.inputTokens`/`outputTokens`/`cacheCreation`/`cacheRead`/`thinkingTokens` + total 兜底填充 |
| 19 | Kilo Code | `kilo-code` | `ui_messages.json`（IDE 任务目录） | `<IDE 根>/User/globalStorage/kilocode.kilo-code/tasks/**` | 无 | 离线 | `payload.tokensIn`/`tokensOut`/`cacheReads`/`cacheWrites`；模型只报 `provider:<slug>` |
| 20 | OpenCode（SQLite 支路） | `opencode` | SQLite（四种库形态） | `$OPENCODE_HOME/opencode.db` | 无 | 离线 | 同一 `normalizeOpencodeTokens`；`data.tokens.*` |
| 21 | Qoder（旧库） | `qoder` / `qoder-cn` | SQLite（三表 JOIN） | `…/Qoder/SharedClientCache/cache/db/local.db`（CN 版独立路径） | 无 | 离线 | `token_info.prompt_tokens`（**含缓存，需减**）+`completion_tokens` |
| 22 | Qoder（新 JSONL） | `qoder`（与旧库**同名同源**） | 自有 JSONL 快照 | `~/.qoder/projects/**/*.jsonl`（CN 版 `~/.qoder-cn`） | 无 | 离线 | `usage.input_tokens`/`cache_read_*`/`cache_creation_*`/`output_tokens`；`credits` 原样挂载不换算 |
| 23 | Cursor | `cursor` | 远程 API（CSV 导出）+ SQLite 取凭据 | `Cursor/User/globalStorage/state.vscdb` + `cursor.com` API | 无 | **需联网 + 需凭据** | CSV 列名按表头匹配；`total` 由四分项求和，不用 CSV 的 `Total Tokens` |
| 24 | Devin CLI | `devin` | SQLite | `$XDG_DATA_HOME/devin/cli/sessions.db`（Windows 仅经 WSL） | 无 | 离线 | `metrics.input_tokens` 等四分项（互斥列）+ `request_id` 全安装规范的账本 |
| 25 | oh-my-pi (OMP) | `omp` | JSONL | `~/.omp/agent/sessions/**/*.jsonl` | 有（extension hook `tokentracker-notify.ts`，可选） | 离线 | `usage.input`/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens` |
| 26 | OmO | `omo` | JSONL（与 OMP 同一实现） | `~/.omo/agent/sessions/**/*.jsonl` | 无 | 离线 | 同上，但 `reasoningTokens`/`reasoning` **已并入 output**，求和时排除 |
| 27 | （共享实现壳） | —（由 OMP/OmO 注入） | — | — | — | — | `parseOmpLikeIncremental`：不为自身产出任何桶 |
| 28 | pi（路由型 agent） | `pi` / `pi-<provider>` | JSONL | `~/.pi/agent/sessions/**/*.jsonl` + `~/.multica/pi-sessions/*.jsonl` | 无 | 离线 | `usage.input`/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens`（reasoning 不并入 output） |
| 29 | Prime Agent | `prime-agent` / `prime-agent-<provider>` | JSONL | `~/.prime/agent/sessions/*.jsonl` | 无 | 离线 | 同 pi 家族字段表 |
| 30 | MiniMax Code | `minimax-code` | JSONL | `~/.minimax/v2/sessions/YYYY/MM/DD/<ts>-session_<id>/messages.jsonl` | 无 | 离线 | 同 pi 家族；**唯一可用时间源是 `message.timestamp`（entry 级恒 `null`）** |
| 31 | （共享实现壳） | —（由 pi 家族三家注入） | — | — | — | — | `parsePiLikeIncremental`：不为自身产出任何桶 |
| 32 | Craft Agents | `craft` | JSONL（只读首行预聚合头） | `~/.craft-agent/**/sessions/<sid>/session.jsonl` + `config.json` | 无 | 离线 | 会话头 `usage.inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheCreationTokens` 差分 |
| 33 | Reasonix | `reasonix` | JSONL 遥测旁车（无正文） | `~/.reasonix/**`、`%APPDATA%\reasonix\**`（`*.jsonl.telemetry.json`） | 无 | 离线 | `promptTokens`/`cacheHitTokens`/`cacheMissTokens`/`cacheWriteTokens`/`completionTokens`/`reasoningTokens` |
| 34 | Copilot CLI / Chat 扩展（OTEL） | `copilot` | OTEL JSONL | `~/.copilot/otel/*.jsonl`、`~/.copilot-otel/*.jsonl` | 无 | 离线（**前提：用户先开启 Copilot 侧 OTEL 文件导出**） | `gen_ai.usage.input_tokens`（CLI span 含读+写缓存，扩展只含读）+ `output_tokens` |
| 35 | Copilot 本地运行时 session-store | `copilot` | SQLite | `~/.copilot/session-store.db` | 无 | 离线 | `assistant_usage_events` 列 + `token_details_json`（`exact`/`fallback` 两档精度） |
| 36 | Copilot App | `copilot` | SQLite（会话级累计） | `~/.copilot/data.db` | 无 | 离线 | `sessions.total_input/output/cached/reasoning_tokens` 差值；**无 cache write 列** |
| 37 | Grok Build (xAI) | `grok` | JSONL + `signals.json`/`summary.json` | `~/.grok/sessions/<encodedCwd>/<sid>/{updates.jsonl,signals.json,summary.json}` | 有（SessionEnd hook，仅新鲜度） | 离线 | `inputTokens`（camelCase 含缓存，需双扣）/`cachedRead`/`cacheCreation`/`outputTokens`/`reasoningTokens` + 上游 ticks 成本 |
| 38 | Google Antigravity | `antigravity` | JSONL 转录 + SQLite（protobuf 二进制列） | `<geminiHome>` 下 `antigravity` / `antigravity-ide` / `antigravity-cli` 三目录的 `brain/**/transcript.jsonl`，配对 `conversations/<uuid>.db` | 无 | 离线 | protobuf `f4.1/f4.2`→input、`f4.5`→cached、`f4.9`→output、`f4.10`→reasoning；兜底走字符估算 |
| 39 | TRAE SOLO（国际版） | `trae` | **SQLCipher 加密 SQLite + WASM 内存快照** | `%APPDATA%` 下 `Trae` 或 `TRAE SOLO` 的 `ModularData/ai-agent` 与 `ModularData/ai-chat` 两处 `database.db` | 无 | 离线 | `chat_turn.context.token_usage.*`（11 个候选列名，唯一解判定后落桶） |
| 40 | TRAE Work CN（国内版） | `trae-cn` | 远程 API + 加密凭据 blob | `%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json` + `api.trae.cn` | 无 | **需联网 + 需凭据 + 需显式 opt-in** | `input_token`（含缓存，需减）/`output_token`/`cache_read_token`/`cache_write_token` |
| 41 | DeepSeek Harness (DSH) | `dsh` | 会话日志 JSONL 或 **zstd 拼接帧** | `~/.dsh/sessions/<project>/<session>/session[.vN].jsonl[.zstd]` | 无 | 离线 | `usage.inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheWriteTokens`/`reasoningTokens`（互斥列） |
| 42 | Command Code (`cmd`) | `command-code` | JSONL 转录 | `~/.commandcode/projects/<cwd-slug>/<session>.jsonl` | 无 | 离线 | `usage.inputTokens`（含缓存，需双扣）/`cacheRead`/`cacheWrite`/`outputTokens`；reasoning 恒 0 |
| 43 | Codex 家族（Codex / AStudio / Every Code） | `codex` / `acode` / `every-code` | rollout JSONL（逐行事件） | `~/.codex/sessions/**/rollout-*.jsonl`（+ 扁平的 `archived_sessions/`）；`~/.acode/sessions`；Every Code 的 `sessions/` | 有（Codex/AStudio/Every Code 各自的 `notify` 改写） | 离线 | 见第 6 章（`parseRolloutIncremental` 一个函数按文件条目自带的 `source` 分流三家） |
| 44 | Claude Code | `claude` | 会话 JSONL | `~/.claude/projects/**/*.jsonl`（`CLAUDE_CONFIG_DIR` / 外置 `scanRoots` 可加根） | 有（`settings.json` 的 usage hook） | 离线 | 见第 6 章 |
| 45 | Claude Science | `claude-science` | SQLite（`operon-cli.db` / 旧名 `operon.db`） | `~/.claude-science`（`CLAUDE_SCIENCE_HOME` 可覆盖，`src/lib/rollout.js:6877`–`6883`） | 无 | 离线 | `input_tokens`/`cache_read_tokens`/`cache_write_tokens`/`output_tokens`（含 `aux_*` 辅助计数）；reasoning 恒 0 |

#### 5.1.1 口径说明：45 个函数名 / 39 个唯一实体 / 2 个共享实现 / 语义重叠

**这四组数字不是同一个东西，混用会得出互相矛盾的结论。** 逐条对齐：

| 口径 | 数字 | 含义与来源 |
|------|------|------------|
| `parse*Incremental` 函数名 | **45** | `src/lib/rollout.js` 内 `function parse*Incremental` 的定义条数（行号区间 `251`–`24458`）。这是「函数」计数，不是「产品」计数 |
| 唯一数据源实体 | **39** | 文本 / JSONL 类 **19** 家 + 数据库 / API 类 **21** 家 − **1** 家重叠（`opencode`：19 家侧记磁盘消息支路、21 家侧记 `opencode.db` 支路）= 19 + 21 − 1 = **39** |
| 共享实现（「Like」壳） | **2** | `parseOmpLikeIncremental`（`src/lib/rollout.js:15753`，服务 `parseOmpIncremental` / `parseOmoIncremental`）与 `parsePiLikeIncremental`（`src/lib/rollout.js:16319`，服务 pi / Prime Agent / MiniMax Code）。这两个名字本身**不产出任何桶**，只承接注入参数 |
| 合并口径的算术闭合 | 45 = 39 + 1 + 2 + 3 | 本章的 **39** 个实体由 **40** 个「自有实现」函数名服务（其中 `opencode` 占两个名：磁盘消息支路 + `opencode.db` 支路）；再加 **2** 个共享实现壳；再加 **3** 个归入第 6 章的函数名（`parseClaudeIncremental`、`parseClaudeScienceIncremental`、`parseRolloutIncremental`）。40 + 2 + 3 = 45 |

**语义重叠（一个 `source` 字面量对应多个解析入口，或一个入口对应多个 source）**——这是总表与「家数」最容易对不上的地方：

| 重叠形态 | 涉及入口 | 说明 |
|----------|----------|------|
| 两来源共用 `kimi` | `parseKimiIncremental` + `parseKimiCodeIncremental` | 两个函数都写 `source="kimi"`，只靠 `seenIds` / 文件偏移各自隔离 |
| 两来源共用 `kiro` | `parseKiroIncremental`（IDE）+ `parseKiroCliIncremental`（CLI） | 源码注释明说要让两者「collapse」到同一 `source='kiro'` 与规范化模型名下 |
| 两来源共用 `qoder` | `parseQoderDbIncremental`（旧库）+ `parseQoderNewIncremental`（新 JSONL） | 靠 `messageKey` 前缀 `row:` / `jsonl:` 与独立游标命名空间隔离，但**上传时同源** |
| 三通道共用 `copilot` | OTEL / session-store / App DB 三份实现 | 靠「用量认领」「会话基线」「`observeOnly`」三重机制分工，见 §5.3 |
| 一入口两 source | `parseWorkbuddyIncremental` → `workbuddy` / `workbuddy-ai` | variant 直接当 `source`（`src/lib/rollout.js:10651`、`11004`） |
| 一入口三 source（第 6 章） | `parseRolloutIncremental` → `codex` / `acode` / `every-code` | 按文件条目自带的 `source` 分流（`src/lib/rollout.js:281`、`494`、`521`、`3227`） |
| 一入口多 source | `parsePiIncremental` → `pi` / `pi-<provider>`，`parsePrimeAgentIncremental` → `prime-agent` / `prime-agent-<provider>` | 路由器形态的 agent，按 `provider` 字段派生 source，避免不同后端的 turn 合并进同一桶 |

**另有 3 个真实来源不在 45 个函数名里，读表时不要当成遗漏**：`parseDumateIncremental`（`src/lib/dumate.js:32`，DuMate 桌面端，入口函数定义在 `dumate.js` 而非 `rollout.js`）、ZCode 与 Mimo（**没有独立入口函数**，复用 `parseOpencodeDbIncremental`，`source` 分别为 `zcode` / `mimo`）。三者的读取细节见 §5.3.3。

---

#### 5.1.2 函数名 ↔ 显示名 ↔ `source` 索引（按 `src/lib/rollout.js` 定义行号排序）

给从代码侧进入本文的读者：**这一节与 §5.1 的总表是同一批 45 行，只是换了排序键**，两表可互相校验。行号即函数定义行。

| # | 函数名 | 定义行 | 显示名 | `source` |
|---|--------|--------|--------|----------|
| 1 | `parseRolloutIncremental` | `251` | Codex 家族（Codex / AStudio / Every Code） | `codex` / `acode` / `every-code` |
| 2 | `parseClaudeIncremental` | `954` | Claude Code | `claude` |
| 3 | `parseGeminiIncremental` | `1242` | Gemini CLI | `gemini` |
| 4 | `parseOpencodeIncremental` | `1412` | OpenCode（磁盘消息支路） | `opencode` |
| 5 | `parseOpenclawIncremental` | `1654` | OpenClaw | `openclaw` |
| 6 | `parseOpencodeDbIncremental` | `5761` | OpenCode（SQLite 支路） | `opencode` |
| 7 | `parseQoderDbIncremental` | `6218` | Qoder（旧库） | `qoder` / `qoder-cn` |
| 8 | `parseQoderNewIncremental` | `6552` | Qoder（新 JSONL） | `qoder` / `qoder-cn` |
| 9 | `parseClaudeScienceIncremental` | `7051` | Claude Science | `claude-science` |
| 10 | `parseCursorApiIncremental` | `7172` | Cursor | `cursor` |
| 11 | `parseKiroIncremental` | `7472` | Kiro（IDE） | `kiro` |
| 12 | `parseHermesIncremental` | `7900` | Hermes | `hermes` |
| 13 | `parseKiroCliIncremental` | `8886` | Kiro CLI | `kiro` |
| 14 | `parseKimiIncremental` | `9528` | Kimi | `kimi` |
| 15 | `parseKimiCodeIncremental` | `9723` | Kimi Code | `kimi` |
| 16 | `parseCodebuddyIncremental` | `10164` | CodeBuddy | `codebuddy` |
| 17 | `parseWorkbuddyIncremental` | `10803` | WorkBuddy / WorkBuddy AI | `workbuddy` / `workbuddy-ai` |
| 18 | `parseRoocodeIncremental` | `11824` | Roo Code | `roocode` |
| 19 | `parseClineIncremental` | `12200` | Cline | `cline` |
| 20 | `parseZedIncremental` | `12650` | Zed | `zed` |
| 21 | `parseLmstudioIncremental` | `13267` | LM Studio | `lmstudio` |
| 22 | `parseUnslothIncremental` | `13691` | Unsloth Studio | `unsloth` |
| 23 | `parseAnythingllmIncremental` | `13938` | AnythingLLM | `anythingllm` |
| 24 | `parseDevinIncremental` | `14207` | Devin CLI | `devin` |
| 25 | `parseGooseIncremental` | `14604` | Goose | `goose` |
| 26 | `parseDroidIncremental` | `15137` | Droid (Factory) | `droid` |
| 27 | `parseKilocodeIncremental` | `15545` | Kilo Code | `kilo-code` |
| 28 | `parseOmpLikeIncremental` | `15753` | **共享实现壳**（OMP / OmO 注入用） | —（不产桶） |
| 29 | `parseOmpIncremental` | `16087` | oh-my-pi (OMP) | `omp` |
| 30 | `parseOmoIncremental` | `16100` | OmO | `omo` |
| 31 | `parsePiLikeIncremental` | `16319` | **共享实现壳**（pi 家族注入用） | —（不产桶） |
| 32 | `parsePiIncremental` | `16689` | pi | `pi` / `pi-<provider>` |
| 33 | `parsePrimeAgentIncremental` | `16699` | Prime Agent | `prime-agent` / `prime-agent-<provider>` |
| 34 | `parseMinimaxCodeIncremental` | `16775` | MiniMax Code | `minimax-code` |
| 35 | `parseReasonixIncremental` | `17050` | Reasonix | `reasonix` |
| 36 | `parseCraftIncremental` | `17080` | Craft Agents | `craft` |
| 37 | `parseCopilotIncremental` | `17878` | Copilot CLI / Chat 扩展（OTEL） | `copilot` |
| 38 | `parseCopilotSessionStoreIncremental` | `18579` | Copilot 本地运行时 session-store | `copilot` |
| 39 | `parseCopilotAppDbIncremental` | `19381` | Copilot App | `copilot` |
| 40 | `parseGrokBuildIncremental` | `20084` | Grok Build (xAI) | `grok` |
| 41 | `parseAntigravityIncremental` | `20800` | Google Antigravity | `antigravity` |
| 42 | `parseTraeIncremental` | `22033` | TRAE SOLO（国际版） | `trae` |
| 43 | `parseTraeCnApiIncremental` | `22598` | TRAE Work CN（国内版） | `trae-cn` |
| 44 | `parseDshIncremental` | `23678` | DeepSeek Harness (DSH) | `dsh` |
| 45 | `parseCommandCodeIncremental` | `24458` | Command Code (`cmd`) | `command-code` |

**表外入口（复用上述解析器，不占 45 个函数名）**：`parseDumateIncremental`（`src/lib/dumate.js:32` → `source: "dumate"`）、ZCode（`source: "zcode"`）、Mimo（`source: "mimo"`）——三家都走 `parseOpencodeDbIncremental`（`src/lib/rollout.js:5761`）与各自的判别器，见 §5.3.3。

### 5.2 文本 / JSONL 类（19 家）

**本节覆盖 19 家**，顺序按「先常用、后长尾」排列（Gemini、OpenCode、OpenClaw、WorkBuddy、CodeBuddy、Kiro / Kiro CLI、Kimi / Kimi Code、Cline、Zed、Goose、Droid、Unsloth、LM Studio、AnythingLLM、Hermes、Roo Code、Kilo Code）。**Every Code 与 Grok Build 不在本节**：前者是 Codex 家族（`parseRolloutIncremental`，见总表第 43 行与第 6 章）的一个 `source`，后者是 JSONL + 估算路径（总表第 37 行，估算细节见 §5.5）。另外要说明：「文本 / JSONL 型」这个分类只对了一半——**这 19 家里有 7 家其实是 SQLite 读取**（Kiro、Hermes、Kiro CLI、Zed、Unsloth、AnythingLLM、Goose），OpenClaw 另有 agent SQLite 支路。

#### 5.2.0 所有家共享的落盘机制（读各家前必须先知道）

- **桶键与默认值**：`server/source/model/hour_start` 三段拼键（`src/lib/rollout.js:4222`–`4226`），`DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`（`src/lib/rollout.js:55`–`56`）——「模型未识别」落成字符串 `unknown`，不是空串。`source` 统一小写化（`src/lib/rollout.js:4254`）。
- **累加器**：`addTotals`（`src/lib/rollout.js:4099`）里 `total_tokens` 必须由各家解析器自己算，只有 `total_tokens` 的 delta 也会被原样累加并写进队列行；`billable_total_tokens` 用 `?? total_tokens` 兜底（同处）。
- **UTC 半小时桶**：`toUtcHalfHourStart`（`src/lib/rollout.js:4173`）把时间戳按 UTC 取整到 `:00` / `:30`，`hour_start` 形如 `2026-10-13T21:30:00.000Z`；无效时间返回 `null`，各家普遍 `continue` **跳过事件**（不落回 now）。兜底到 mtime / now 的只有少数几家（Droid 用 mtime、LM Studio 用 mtime、Zed 与 Goose 用 `now`、Grok 用 `Date.now()`）。
- **队列行契约**：字段集 `source/model/hour_start/input_tokens/cached_input_tokens/cache_creation_input_tokens/output_tokens/reasoning_output_tokens/total_tokens/billable_total_tokens/total_cost_usd`（`src/lib/rollout.js:3328`）。队列是**绝对值快照流**而非增量事件流：同桶内容不变则跳过（`src/lib/rollout.js:3327`），消费端按 `(source, model, hour_start)` 覆盖。
- **零用量闸门**：`isAllZeroUsage`（`src/lib/rollout.js:4945`）把 `total_tokens` 也算作「非零」——这是 §5.6 里 $0 退化风险的判据来源。
- **本地推理共用归一化器**：`normalizeLocalStudioTokens`（`src/lib/rollout.js:12994`，返回块 `13039`–`13049`）服务 LM Studio 与 Unsloth；落桶入口 `reconcilePassiveUsageEvent`（`src/lib/rollout.js:13242`）**同 key 先减旧再加新**。

#### 5.2.1 Gemini（`parseGeminiIncremental`，`src/lib/rollout.js:1242`）

- `source`：调用方显式传 `"gemini"`（`src/commands/sync.js:1272`），解析器兜底 `normalizeSourceInput(source) || "gemini"`（`src/lib/rollout.js:1265`），并允许逐文件覆盖（`src/lib/rollout.js:1275`）。
- 路径：官方目录 `~/.gemini`（`GEMINI_HOME` 可覆盖）+ `tmp`（`src/commands/sync.js:1224`、`1234`）；目录约定 `tmp/<project>/chats/session-*.json`（`src/lib/rollout.js:226`）。
- 格式：整份 JSON，事件粒度是 `session.messages` 数组元素（`src/lib/rollout.js:2916`）；文件身份含 inode（`src/lib/rollout.js:1284`，判等 `1287`–`1291`）。
- 五桶映射（`normalizeGeminiTokens`，`src/lib/rollout.js:4734`）：`tokens.input`→input；`tokens.cached`→cached；`tokens.output + tokens.tool`→output；`tokens.thoughts`→reasoning（**不并入 output**）；`tokens.total`→total 取 `max(上报值, 分桶和)`（`4743`）；`cache_creation_input_tokens` 恒 0。
- 模型：逐条消息 `msg.model`，会话内沿用上次值（`src/lib/rollout.js:2952`）；无 Gemini 专属兜底，缺失落 `unknown`。
- 时间与桶：`msg.timestamp`，缺失即跳过（`src/lib/rollout.js:2956`、`2964`）。
- 增量判据：数组下标 + 累计基线（`cursors.files[key] = { lastIndex, lastTotals, lastModel }`，`src/lib/rollout.js:1361`）；数组被截断即整段重置（`2917`）。差分用 `diffGeminiTotals`（`286`），`conversation_count` 计「产生非零差分的消息条数」（`2974`）。
- 特殊：**19 家里唯一有 $0 退化路径的一家**（`total` 取 max + 闸门把 `total_tokens` 当非零，`src/lib/rollout.js:2971`、`4945`）——若日志只写 `tokens.total`，会写出 input/output/cache 全 0 的队列行；成立前提与计价侧条件见 §5.6。

#### 5.2.2 OpenCode（磁盘消息支路，`parseOpencodeIncremental`，`src/lib/rollout.js:1412`）

- `source`：`"opencode"`（`src/commands/sync.js:1440`），兜底 `src/lib/rollout.js:1438`。
- 路径：`OPENCODE_HOME || $XDG_DATA_HOME/opencode`（`src/commands/sync.js:1339`），再拼 `storage`（`src/commands/sync.js:1400`）→ `<storage>/message/**`（`walkOpencodeMessages` 递归，只收 `msg_*.json`，`src/lib/rollout.js:5025`、`5033`；`listOpencodeMessageFiles` 只拼一层 `<storage>/message`，`243`）。
- 五桶映射（`normalizeOpencodeTokens`，`4755`）：`tokens.input`→input（已是**纯非缓存输入**，不做减法）；`tokens.cache.read`→cached；`tokens.cache.write`→cache_creation；`tokens.output`→output；`tokens.reasoning`→reasoning（不并入 output）；`total` 由五分项重算。
- 模型：兼容 `modelID` / `model`(字符串) / `model: { id }`（`src/lib/rollout.js:4272`）；落桶兜底 `DEFAULT_MODEL`（`3143`）。
- 时间与桶：`msg.time.completed` → `msg.time.created`，两者皆无则不产事件（`src/lib/rollout.js:3122`）。
- 增量判据：`cursors.files[key]`（`1513`）存 `lastTotals/fingerprint/messageKey/dedupedForkCopy`；差分复用 `diffGeminiTotals`（`3109`）；fork 副本去重（`3805`），副本「复活」时整份重加而非只补差（`3105`）。
- 特殊：同寄主另有 **SQLite 支路**（`src/lib/rollout.js:5038` 起，见 §5.3），两路结果在 `src/commands/sync.js:1385-1388` 相加；纯 total 行会被静默丢弃（差分函数对全 0 返回 `null`）。

#### 5.2.3 OpenClaw（`parseOpenclawIncremental`，`src/lib/rollout.js:1654`）

- `source`：`"openclaw"`（`src/commands/sync.js:1109`），逐文件可覆盖（`src/lib/rollout.js:1698`）。
- 路径：`~/.openclaw`（三档 env 覆盖）+ `agents/<agent>/sessions/*.jsonl`（`src/lib/rollout.js:1582`、`1622`）；子目录集合 `["sessions","session-sqlite-import-archive"]`（`1612`），另有 agent SQLite（`1638`）。Windows 额外探测 WSL。
- 五桶映射（`src/lib/rollout.js:2094`）：`message.usage.input` **先减** `cacheRead` 再进 input；`cacheRead`→cached；`cacheWrite`→cache_creation；`output`→output；reasoning 恒 0；`total` = 四桶和。
- 关键点：`totalTokens` **只用于行级预筛，从不入库**（`src/lib/rollout.js:2055`：`if (!line.includes('"usage"') || !line.includes("totalTokens")) continue;`）。
- 模型：`normalizeModelInput(msg.model) || DEFAULT_MODEL`（`2074`）。时间：`obj.timestamp`，缺失即跳过（`2071`）。
- 增量判据：字节偏移（`1710`）+ 事件身份计数表（`2114`）+ 时间闸门 `aggregateAfterMs`（`2128`）。
- 特殊：**项目维度不支持**（`src/lib/rollout.js:2141`：`// Project-level OpenClaw attribution is not supported yet (no stable cwd info).`）；有插件但解析完全被动（`src/commands/sync.js:1102` 注释）。

#### 5.2.4 Kiro（IDE，`parseKiroIncremental`，`src/lib/rollout.js:7472`）

- `source`：硬编码 `"kiro"`（`src/lib/rollout.js:7567`）。
- 路径：`%APPDATA%\Kiro\User\globalStorage\kiro.kiroagent`（suffix 见 `7282`）→ `dev_data/devdata.sqlite` 与 `dev_data/tokens_generated.jsonl`（`resolveKiroDbPath` / `resolveKiroJsonlPath`，`7298`–`7304`）。
- 五桶映射（`7545`）：`tokens_prompt`→input、`tokens_generated`→output；**两列皆 0 即跳过**（`7547`）；`cached_input_tokens` / `reasoning_output_tokens` 写死 0，`cache_creation_input_tokens` 键缺失。
- 模型：`.chat` 时间线反查，兜底 `"kiro-agent"`（`7555`）。
- 时间与桶：`row.timestamp` 按 **UTC** 解释 naive 串（`ts.replace(" ","T") + "Z"`，`7549`）——SQLite `CURRENT_TIMESTAMP` 语义。
- 增量判据：自增 `id` 水位（`7541`、`7572`）；JSONL 兼容支路用行号（`countKiroJsonlLines`，`7338`）。
- 特殊：`source='kiro'` 与 Kiro CLI 合并（`8112` 注释），规范模型名后两者「collapse」到同一桶。

#### 5.2.5 Hermes（`parseHermesIncremental`，`src/lib/rollout.js:7900`）

- `source`：`"hermes"`（`src/lib/rollout.js:8018`）。
- 路径：`TOKENTRACKER_HERMES_HOME` → `~/.hermes` / `%LOCALAPPDATA%\hermes`（`7604`、`7610`）；DB 为 `state.db`，外加 `profiles/*/state.db`（`7659`、`7676`）。UNC 路径先本地快照（`7722`）。
- 五桶映射：SQL 列名即桶名（SQL 原文 `7718`），落桶在 `8008`：`input_tokens`/`cache_read_tokens`→cached/`cache_write_tokens`→cache_creation/`output_tokens`/`reasoning_tokens`；`total` 为五项之和；**`message_count` 差分 → `conversation_count`**（`dMessageCount`）。
- 模型：`normalizeModelInput(row.model) || "hermes-agent"`（`8006`）。
- 时间与桶：**取 `ended_at`，回落 `started_at`**（`7999`）——长会话的全部增量会堆在「结束时刻」的那个半小时桶里。
- 增量判据：`started_at` 水位 + 未完成会话强制重读（`7707`、`7708`）；SQL 层 `WHERE` 要求至少一列 > 0，天然不产 $0 行。
- 特殊：`conversation_count` 语义是「消息数差分」，与会话/请求都不同源（见 §5.6）。

#### 5.2.6 Kiro CLI（`parseKiroCliIncremental`，`src/lib/rollout.js:8886`）

- `source`：**`"kiro"`**（`src/lib/rollout.js:9191`、`9210`），与 IDE 共享。
- **三条支路、两种精度**：legacy 会话 `.json` **优先读真实用量** `turn.input_token_count` / `turn.output_token_count`（`8371`、`8372`），**只有两者皆为 0 才回退**字符估算（闸门 `8374`，估算点 `8386`、`8387`）；SQLite `data.sqlite3` 支路为纯字符估算（常量 `KIRO_CLI_CHARS_PER_TOKEN = 4` 在 `8118`，使用点 `9152`–`9159`，乘法点 `9155`）；v2 `messages.jsonl` 支路也是纯估算（该文件不含 token 字段，注释 `8475`–`8481`）。
- 路径：`kiro-cli/data.sqlite3`（三平台分叉 `8132`、`8138`、`8140`）；会话文件由 `resolveKiroCliSessionFiles` 给出（`8165`），根为 `KIRO_HOME || ~/.kiro`（`8960`）。
- 五桶映射（SQLite 支路，落桶 `9209`）：`floor(promptChars/4)`→input、`floor(responseChars/4)`→output、`floor(reasoningChars/4)`→reasoning，两个缓存桶 0；闸门为 `approxInput > 0 || approxOutput > 0 || approxReasoning > 0`（`9209`）。
- 模型：`r.model_id || r.session_model_id` → 规范化 → 兜底 `"kiro-cli-agent"`（`9167`）。时间：`request_start_timestamp_ms`，非正即跳过（`9161`）。
- 增量判据：per-request 指纹（`${promptChars}:${responseChars}:${reasoningChars}:${model}:${tsMs}`，`9172`）——**先减旧再加新**，回撤是把负数 delta 交给 `addTotals`（`9192`），`conversation_count` 随之 −1（`9203`）；另有「会话文件 → SQLite 迁移后撤回」通道（`9036`）。
- 特殊：**credits 不进 token 桶**，只写侧车 `kiro-credits.json`（`8119`、`8955`）。会话支路的零值闸门只检 4 列、**不含 reasoning**（`9414`），随后 delta 含 `total_tokens: … + reasoning`（`9446`）→ 可产出 `input=0/output=0/reasoning>0/total>0` 的行（见 §5.6 B 类）。

#### 5.2.7 Kimi（`parseKimiIncremental`，`src/lib/rollout.js:9528`）

- `source`：硬编码 `"kimi"`（`src/lib/rollout.js:9611`）。
- 路径：`~/.kimi/sessions/<workspace>/<session>/wire.jsonl`（`resolveKimiWireFiles` `9507`，`path.join(kimiHome, "sessions")` `9510`；`9501` 只是 `wslProviderDir: ".kimi"`）。
- 五桶映射（`9586`）：`msg.payload.token_usage.input_other`→input、`input_cache_read`→cached、`input_cache_creation`→cache_creation、`output`→output；**四列全 0 提前跳过**（`9590`）；reasoning 恒 0，`total` 重算。只处理 `message.type === "StatusUpdate"` 且带 `token_usage` / `message_id` 的记录（`9575`）。
- 模型：**来自配置而非日志**（`src/lib/rollout.js:9540`：`model || resolveKimiDefaultModel(env || process.env)`），兜底 `"kimi-for-coding"`（`8080`）。
- 时间与桶：`entry.timestamp ?? payload.timestamp`（**秒**），缺失即跳过（`9595`）。
- 增量判据：文件 `size` / `ino` 偏移 + `message_id` 集合（`9557`、`9542`）；size 回退或 inode 变化则从 0 重读（`9557`）。
- 特殊：源码明确不需要 hook（`src/lib/rollout.js:8078`：`// No hook installation needed; Kimi writes wire.jsonl automatically.`）。

#### 5.2.8 Kimi Code（`parseKimiCodeIncremental`，`src/lib/rollout.js:9723`）

- `source`：**`"kimi"`**（`src/lib/rollout.js:9844`），与 Kimi 同源。
- 路径：`~/.kimi-code/sessions/**` 递归收集 `wire.jsonl`，深度 ≤ 5（`9677`）。
- 五桶映射（**三种历史形状**，`9802`–`9817`）：新版 camelCase（`usage.inputOther` 存在）→ `inputOther`/`inputCacheRead`/`inputCacheCreation`/`output`；Anthropic 风格 → `input_tokens`/`cache_read_input_tokens`/`cache_creation_input_tokens`/`output_tokens`；OpenAI 兼容 → `input_tokens − details.cached_tokens`/`details.cached_tokens`/`cache_creation_input_tokens`/`output_tokens`。
- 只读 `step.end` 且带 `usage` 与 `evt.uuid` 的事件（`9775`、`9782`），`config.update` 只更新模型——避免与 `usage.record` 双计（注释 `9796`）。
- 模型：`config.update.modelAlias` 覆盖（`9769`），默认来自 `~/.kimi-code/config.toml`，兜底 `"kimi-for-coding"`（函数头 `resolveKimiCodeDefaultModel` 在 `9703`，兜底在 `9704`）。
- 时间与桶：`entry.time ?? evt.time`（**毫秒**），缺失即跳过（`9828`）。
- 增量判据：**仅 `seenIds`（uuid），无字节偏移**（`9783`）——文件被整体重写时依赖 uuid 而非位置。
- 特殊：四桶全 0 提前跳过（`9823`）。

#### 5.2.9 CodeBuddy（`parseCodebuddyIncremental`，`src/lib/rollout.js:10164`）

- `source`：`"codebuddy"`（`src/lib/rollout.js:10384`）。
- **双源设计**：主源 `~/.codebuddy/projects/**/*.jsonl`（`CODEBUDDY_HOME` 可覆盖，`10028`、`10035`）；兜底源为 IDE 扩展日志，平台分叉在 `10069`（`CodeBuddy CN/logs`、`Code/logs`、`CodeBuddyExtension/Logs/CodeBuddyIDE`）。
- 五桶映射：按优先级取镜像（`10278`、`10293`）——input：`cachedMissTokens`/`cacheMissTokens`/`input_tokens`/`inputTokens`/`prompt_tokens`；output：`output_tokens`/`outputTokens`/`completion_tokens`；cached：`cache_read_input_tokens`/`cacheReadInputTokens`/`cacheTokens`/`prompt_cache_hit_tokens`/`cached_tokens`；cache_creation：`cache_creation_input_tokens`/`cacheCreationInputTokens`/`cachedWriteTokens`/`prompt_cache_write_tokens`。**非缓存输入修正**（`10337`–`10343`）：当 input 来自「总量字段」且 cacheRead > 0 时，`inputTokens = max(0, inputTokens − cacheRead)`。
- 落桶（`10374`）：含 `reasoningTokens`，`total` 为五分量和，`conversation_count: 1`。
- 模型：`modelsByAgent` → `logModelsByAgent` → `fallbackModel`（`10355`），兜底 `"codebuddy-unknown"`（函数头 `9923`，兜底 `9924`）。
- 时间与桶：`entry.timestamp`（毫秒）/ 日志行时间，缺失即跳过。
- 增量判据：`seenIds`（`messageId`）+ 用量指纹（`10360`、`10369`），指纹表上限 10k（`capCodebuddyUsageFingerprints`，`10022`）。
- 特殊：**两条支路都有零值闸门**——扩展日志支路要求 input/completion/cacheRead/cacheCreation 至少一个 > 0（`10345`，**该判定不含 reasoning**，故「只有 reasoning」的行会被丢）；JSONL 支路是含 `reasoningTokens` 的五桶零闸门（`10476`–`10485`）。纯 total 行被静默丢弃，不产 $0 行。hook 属 Claude 系（`src/lib/passive-mode.js:156`，安装点 `src/commands/init.js:577`）。

#### 5.2.10 WorkBuddy / WorkBuddy AI（`parseWorkbuddyIncremental`，`src/lib/rollout.js:10803`）

- `source`：**唯一随 variant 变化的实现**——`WORKBUDDY_VARIANTS` 定义 `workbuddy`（`.workbuddy`，`WORKBUDDY_HOME`）与 `workbuddy-ai`（`.workbuddy-ai`，`WORKBUDDY_AI_HOME`）两个 variant（`10651`），落桶直接用 variant 当 source（`11004`：`getHourlyBucket(hourlyState, variant, model, bucketStart)`）；调用侧传 `source: "workbuddy-ai"`（`src/commands/sync.js:2679`）。
- **三类数据源，优先级严格**（注释 `10694`）：`projects/**/*.jsonl`（含 `subagents/agent-*.jsonl`）> `traces/<pid>/trace_*.json` > 同寄主 SQLite；压制关系在 `11013`、`11088`、`11164`–`11168`。
- 五桶映射：入口是 `providerData.rawUsage`（`10907`）：input = `prompt_tokens − cacheRead − cacheCreation`（`10963`，避免缓存双计）；cached = 三个镜像取最大（`cache_read_input_tokens` / `prompt_tokens_details.cached_tokens` / `prompt_cache_hit_tokens`）；cache_creation = `cache_creation_input_tokens`；output = `completion_tokens − reasoning`；reasoning = `completion_tokens_details.reasoning_tokens`。
- traces 支路**不同形**：`modelInfo.totalInputTokens`/`totalOutputTokens`/`totalCachedTokens`（`11085`–`11087`）+ `startedAtRaw`（`11081`，秒/毫秒自适应）+ `modelInfo.models[0]`（`11103`）；`input = totalInput − totalCached`（`11105`），两个缓存/推理桶恒 0（`11109`、`11111`），`conversation_count: 1`（`11113`）。
- 模型：`provider.model` → `provider.requestModelId` → `entry.model` → `fallbackModel`（`10987`），兜底 `"auto"`（函数头 `10677`，兜底 `10678`，且 `auto` 会逐字上报，`10632` 注释）。
- 时间与桶：`entry.timestamp`（毫秒），缺失即跳过（`10919`、`10979`）。
- 增量判据：`seenIds`（消息 ID，`11007`）+ 旧版 entry-id 双写（`11008`）；`detailedSessions` 压制 traces 兜底（`11013`）。
- 特殊：五桶全 0 跳过（`10968`）；hook 装在两个 variant 上（`src/lib/passive-mode.js:166`、`177`；`src/commands/init.js:581`、`585`）。

#### 5.2.11 Roo Code（`parseRoocodeIncremental`，`src/lib/rollout.js:11824`）

- `source`：`"roocode"`（`src/lib/rollout.js:11939`）。
- 路径：**复用 Kilo Code 的 IDE 根列表**（`11767`），再拼 `<IDE 根>/User/globalStorage/rooveterinaryinc.roo-cline/tasks`（`11767` 起）。
- 五桶映射（`11906`）：`payload.tokensIn`→input、`cacheReads`→cached、`cacheWrites`→cache_creation、`tokensOut`→output；reasoning 恒 0。
- 入口只认 `msg.say ∈ {api_req_started, api_req_deleted}`（`11892`）；**零值占位故意不入 `seen`**，以便后续回填（`11910` 注释）。
- 模型：真实模型藏在兄弟文件 `api_conversation_history.json` 的 `<environment_details><model>` 里（注释 `11749`，正则 `11805`）；三级兜底（`11814`）：真实模型 → `protocol:<apiProtocol>` → `"unknown"`。
- 时间与桶：`msg.ts`（毫秒，`11899`）。增量判据：`taskUuid:ts`（`11902`）+ 文件 mtime 闸门。
- 特殊：Kilo Code 与 Roo Code 字段同形、只有 `source` 与目录不同。

#### 5.2.12 Cline（`parseClineIncremental`，`src/lib/rollout.js:12200`）

- `source`：`"cline"`（`src/lib/rollout.js:12396`）。
- 路径：`resolveClineSessionsDirs`（`12054`，Windows 双安装 + WSL 探测在 `12069`–`12073`）→ `scanClineSessionFiles`（`12086`）；带错误/完成态的包装 `resolveClineSessionFilesWithStatus`（`12136`）。
- 五桶映射（`12356`）：input = `inputTokens − cacheRead − cacheWrite`（上游 `inputTokens` **含缓存**）；`cacheReadTokens` / `cacheWriteTokens` 各自成桶；output = `outputTokens`（**含** reasoning）；reasoning = `reasoningTokenCount`（只上报，**不加入 total_tokens**，也不在 output 之上再计费）。
- **唯一自产成本的解析器**：`metrics.cost` → `total_cost_usd`（差分后入桶，`12356`）。
- 模型：`msg.modelInfo.id` → 会话 sidecar `model` → `provider:<name>` → `DEFAULT_MODEL`（`normalizeClineModel`，函数头 `12178`，兜底链 `12179`–`12189`）。
- 时间与桶：`msg.ts`，另有**导入闸门** `if (importedAtMs !== null && ts <= importedAtMs) continue;`（`12352`；`12350` 是上一行的 `const ts = Number(msg.ts);`）——导入历史不被重复计。
- 增量判据：三键兼容游标（`12369`）+ **五路逐指标差分**（`12383`：`deltaInput/deltaCached/deltaCreation/deltaOutput/deltaReasoning`，各 `max(0, …)`）；`conversation_count = previous ? 0 : 1`（`12405`），只计首次。
- 特殊：跳过条件包含 `cost`（`12381`）——**只有成本变化、token 无增量的行也会入队**。

#### 5.2.13 Zed（`parseZedIncremental`，`src/lib/rollout.js:12650`）

- `source`：`"zed"`（`src/lib/rollout.js:12782`）。
- 路径：`threads/threads.db`（darwin `~/Library/Application Support/Zed/...`、win32 `%LOCALAPPDATA%\Zed\...`，`12490`）；Windows 探测 WSL（`12501`），sync 侧双安装合并（`src/commands/sync.js:2062`）。
- 格式：`threads` 表 BLOB，`data_type` 为 `json` 或 `zstd`（`12518`）——`zstdDecompressSync` 优先、`@mongodb-js/zstd` 回落。
- 五桶映射（`12553`）：`input_tokens`→input、`output_tokens`→output、`cache_read_input_tokens`→cached、`cache_creation_input_tokens`→cache_creation；reasoning 恒 0。取值优先级 `thread.request_token_usage`（逐请求和）→ `thread.cumulative_token_usage`（`12597`、`12601`）。
- 模型：`thread.model.model`（`12594`）；跳过 `thread.imported === true`（`12585`）与 `ZED_DOUBLE_COUNTED_PROVIDERS`（`12593`）。
- 时间与桶：`updated_at ?? created_at ?? thread.updated_at ?? new Date().toISOString()`（`12764`）——**末级兜底是 now**。
- 增量判据：SQL `updated_at > cursor`（`12636`）+ 线程累计差分；`isReset = currSum > 0 && currSum < prevSum`（`12742`，`12732` 是差分基线，不是 reset 判据）；`threadTotals` 上限 10k（`12816`）。
- 特殊（限定条款）：`totalDelta` 只由 input/output/cache_read/cache_write 求和（`12751`）且 `totalDelta <= 0` 即跳过（`12752`），因此**只有缓存列增长**时仍会落出 `input=0 / output=0 / total>0` 的行——「结构上不可能出现六列全零」不等于「不可能出现 input/output = 0」。

#### 5.2.14 LM Studio（`parseLmstudioIncremental`，`src/lib/rollout.js:13267`）

- `source`：`const LMSTUDIO_SOURCE = "lmstudio";`（`12852`）。
- 路径：`TOKENTRACKER_LMSTUDIO_HOME` / `LM_STUDIO_HOME` / `~/.lmstudio`（`12859`）+ `server-logs` 递归 `*.log`（`12871`）。
- 格式：文本日志内嵌 JSON（模块注释 `12845`，段头 `12843`），解析靠 `"usage"` 字节标记 + 花括号配平（`12897`、`12917`），带 8MB 窗口 / 64KB 分块上限（`LMSTUDIO_WINDOW_BYTES` at `12853`、`LMSTUDIO_CHUNK_BYTES` at `12854`）。
- 五桶映射：复用 `normalizeLocalStudioTokens`（`12994`，返回块 `13039`–`13049`）——`total` 取 `total_tokens`/`totalTokens` 与 `prompt+completion` 的较大者；cached ≤ prompt；cache_creation ≤ prompt − cacheRead；reasoning ≤ completion；**input 由 `total − completion − cacheRead − cacheWrite` 反推**。
- 模型：`lastLmstudioJsonStringField(metadataBuffer, "model") || carried.model`（`13091`），兜底 `DEFAULT_MODEL`。
- 时间与桶：日志行 `[YYYY-MM-DD HH:MM:SS]`（**本地时间**，`12979`）→ 跨窗口携带 → **文件 mtime**（`13094`、`13129`）。
- 增量判据：文件 `dev/ino/size/mtime` 三态（`unchanged` / `canResume` / 整源重建，`13325`）+ 事件 key（`lmstudio:<responseId>` 或哈希，`13099`）。
- 特殊：`total_cost_usd: 0` 与 `conversation_count: 1` 在共用归一化器里写死（返回块 `13039`–`13049`）。

#### 5.2.15 Unsloth Studio（`parseUnslothIncremental`，`src/lib/rollout.js:13691`）

- `source`：`const UNSLOTH_SOURCE = "unsloth";`（`13467`）；落桶经 `reconcilePassiveUsageEvent`（`13730`）。
- 路径：`<studioHome>/studio.db`，默认 `~/.unsloth/studio/studio.db`（`13482`）。
- 五桶映射：`normalizeUnslothUsageRow`（`13667`）把 `prompt_tokens`/`completion_tokens`/`total_tokens`/`cached_tokens`/`cache_write_tokens`/`reasoning_tokens` 交给共用归一化器；**`api_usage_events` 表的投影把缓存/推理列写死 0**（`0 AS cached_tokens, 0 AS cache_write_tokens, 0 AS reasoning_tokens`，`13634`–`13636`）。
- 模型：`response_model` → `requested_model` → `fallback_model` → `DEFAULT_MODEL`（`13650`），再加前缀：本地 → `local/…`、未计费 provider → `unpriced/<provider>/…`、已知 provider → `<provider>/<model>`（白名单 `13470`）。
- 时间与桶：`coerceEpochMs(created_at) || parseIsoTimestampMs(created_at)`（`13677`）。
- 增量判据：扫描窗口 `overlap/latest`（`created_at` + `id` 二元组）+ DB 侧车指纹；**只在扫描有界时才走快路径**（判定 `13714` `hasBoundedScan`、短路返回 `13717`），尾部保留末 256 行（`13469`）。
- 特殊：与 LM Studio 同用「先减旧再加新」的被动对账入口，避免整源重建时重复累加。

#### 5.2.16 AnythingLLM（`parseAnythingllmIncremental`，`src/lib/rollout.js:13938`）

- `source`：`"anythingllm"`（`src/lib/rollout.js:14053`）。
- 路径：`%APPDATA%\anythingllm-desktop\storage\anythingllm.db`（`13808`）；数据来自 `workspace_chats.response.metrics`（`src/commands/sync.js:1908`；SELECT 本体 `13874`–`13887`，SQL 层只过滤游标/`include`，**不做零值过滤**）。
- 五桶映射（`14013`、`14042`）：`prompt_tokens`→input、`completion_tokens`→output，两者皆 0 则该行丢弃（`14016`）；**`reasoning = max(0, total_tokens − input − output)`**——「未解释差额」被判为 reasoning，**不保证真的是推理量**（本章里最反直觉的一处）；两个缓存桶恒 0。
- 模型：`row.model || "anythingllm-unknown"`（`14040`）。
- 时间与桶：`createdAt ?? lastUpdatedAt`，解析器同时吃 epoch 毫秒/秒、数字串、`YYYY-MM-DD HH:MM:SS`（**按 UTC**，`13856`）。
- 增量判据：自增 `id` 水位（`14010`）+ `pendingChatIds` 补扫（`14024`）。
- 特殊：纯 total 行会被丢弃（不产 $0 行），但**「差额即 reasoning」会让 reasoning 桶变成一个语义垃圾桶**，下游按 output 单价计费 reasoning 时会被高估。

#### 5.2.17 Goose（`parseGooseIncremental`，`src/lib/rollout.js:14604`）

- `source`：`"goose"`（`src/lib/rollout.js:14730`）。
- 路径：`GOOSE_PATH_ROOT` → `<root>/data/sessions/sessions.db`（`14489`）；Windows 为 `%APPDATA%\goose\sessions\sessions.db`（`14503`），macOS/Linux 走 `Application Support` / `$XDG_DATA_HOME`（含 `Block/goose` 备用）；sync 侧双安装合并（`src/commands/sync.js:1993`）。
- 五桶映射（`14674-14686`、`14708`）：优先 `accumulated_input_tokens` / `accumulated_output_tokens`（回退单回合列）差分进 input/output；`reasoningDelta = max(0, dTotal − dInput − dOutput)`（`14711`，Goose 把推理并进 `total_tokens`）；两个缓存桶恒 0。
- 模型：`parseGooseModelName(row.model_config_json)`（函数头 `14529`）**无兜底常量**，解析不到就 `continue`。
- 时间与桶：`parseGooseCreatedAt(row.created_at) || new Date().toISOString()`（`14713`）。
- 增量判据：DB mtime 短路（`14639`）+ 会话累计差分 + `isReset`（`14683`）。
- 特殊（$0 弱化版）：只要 `dTotal > 0` 就落桶，continue 条件是三项皆 0（`14686`）；若 input/output 差分为 0（只有 total 在涨），会产出 `input=0 / output=0 / reasoning>0` 的行。因 `goose` 不在「推理并入 output」集合内，该行按 output 单价计费，**不是 $0**（见 §5.6）。

#### 5.2.18 Droid（Factory，`parseDroidIncremental`，`src/lib/rollout.js:15137`）

- `source`：`"droid"`（`src/lib/rollout.js:15381`）。
- 路径：`DROID_SESSIONS_DIR`（逗号分隔）→ `FACTORY_DIR/sessions` → `~/.factory/sessions`（`14797`，Windows 带 WSL 分支）；文件筛选 `*.settings.json`（`14836`）；sync 侧去重 `dedupeDroidSettingsFilesBySession`（**定义在 `src/lib/rollout.js:15069`**，调用 `15165`、导出 `24839`；`src/commands/sync.js:6595` 只是一条注释），另有 `prune: true`（`src/commands/sync.js:2032`）。
- 五桶映射（`15272`）：`tokenUsage.inputTokens`/`outputTokens`/`cacheCreationTokens`/`cacheReadTokens` 各自成桶；`thinkingTokens`→reasoning；`totalTokens` 只做兜底。
- **本家独有的「total 兜底填充」**（`applyDroidTotalFallback`，`14972`）：算 `known = input + output + cacheCreation + cacheRead + thinking`，若 `total > known`，缺口优先填 `output`（当 output 为 0），否则并入 `thinking`——**结构性规避「只有 total」的 $0 退化**。
- 模型：`resolveDroidModel`（定义 `15046`–`15057`）三级链：`settings.model` → sidecar `<id>.jsonl` 抓取 → `providerLock` / 模型片段推断 → `defaultDroidModelForProvider`；调用点 `15348`，上方注释 `15343`–`15346` 与实现一致；`normalizeDroidModelName` 在 `14858`。
- 时间与桶：**唯一用文件 mtime 的家**（`15210`、`15328`）：`toUtcHalfHourStart(new Date(mtimeMs || Date.now()).toISOString())`。
- 增量判据：会话累计 + mtime（`15243`）；`conversation_count = isFirstSeenSession || isReset ? 1 : 0`（`15379`）。
- 特殊：因时间来自 mtime，**历史会话被复制/触碰会把用量搬到「触碰时刻」的桶**，这是与其它家的本质差异。

#### 5.2.19 Kilo Code（`parseKilocodeIncremental`，`src/lib/rollout.js:15545`）

- `source`：**`"kilo-code"`（带连字符）**（`src/lib/rollout.js:15651`）。
- 路径：`<IDE 根>/User/globalStorage/kilocode.kilo-code/tasks`（`11706`）；sync 侧双安装合并（`src/commands/sync.js:1949`）。
- 五桶映射（`15624`、`15640`）：`payload.tokensIn`→input、`tokensOut`→output、`cacheReads`→cached、`cacheWrites`→cache_creation；reasoning 恒 0；`total` = 四桶和。
- **模型：只报 provider，不报模型**（`normalizeKilocodeProviderToModel`，`11730`）——slug 化后返回 `provider:<slug>`，全分隔符的 slug 返 `provider:unknown`；原因是真实 model id 存在 workspace state、不归属到单轮（`11724` 注释）。
- 时间与桶：`msg.ts`（`15617`）。增量判据：`taskUuid:ts` + 文件 `{size, mtimeMs, ino}`。
- 特殊：零值占位不入 `seen`（与 Roo Code 同族），后续回填；无 $0 退化。

---

#### 5.2.20 19 家横向对照（时间源 / 游标形状 / 模型来源 / 纯 total 处置）

| Provider | 时间源 | 桶起点兜底 | 增量游标形状 | 模型来源 | 纯 total 行处置 |
|----------|--------|------------|--------------|----------|-----------------|
| Gemini | `msg.timestamp` | 否（缺失即跳过，`2956`、`2964`） | 数组下标 + 累计基线（`1361`） | `msg.model`，会话内沿用（`2952`） | **落桶 → $0**（`2971`） |
| OpenCode（磁盘） | `msg.time.completed ?? created` | 否（`3122`） | 文件 key：`lastTotals`/`fingerprint`/`messageKey`（`1513`） | `modelID` / `model` / `model.id`（`4272`），兜底 `unknown`（`3143`） | 丢弃（`3109`） |
| OpenClaw | `obj.timestamp` | 否（`2071`） | 字节偏移 + 事件身份计数（`1710`、`2114`） | `msg.model`，兜底 `unknown`（`2074`） | 丢弃（`2109`） |
| Kiro（IDE） | `row.timestamp`（按 UTC 补 `Z`） | 否（`7549`） | 自增 `id` 水位（`7541`）/ JSONL 行号（`7338`） | `.chat` 反查 → `kiro-agent`（`7555`） | 丢弃（`7547`） |
| Hermes | `ended_at ?? started_at` | 否（`7999`） | `started_at` 水位 + 未完会话强制重读（`7707`） | `row.model` → `hermes-agent`（`8006`） | SQL 层拦掉（`7718`） |
| Kiro CLI | `request_start_timestamp_ms` | 否（`9161`） | per-request 指纹（`9172`） | `model_id` / `session_model_id` → `kiro-cli-agent`（`9167`） | 丢弃；会话支路可产 reasoning-only 行（`9414`） |
| Kimi | `entry.timestamp ?? payload.timestamp`（秒） | 否（`9595`） | `size`/`ino` 偏移 + `message_id`（`9557`） | **来自配置文件**（`9540`）→ `kimi-for-coding`（`8080`） | 丢弃（`9590`） |
| Kimi Code | `entry.time ?? evt.time`（毫秒） | 否（`9828`） | 仅 `seenIds`，无偏移（`9783`） | `config.update.modelAlias`（`9769`）→ `kimi-for-coding`（`9704`） | 丢弃（`9823`） |
| CodeBuddy | `entry.timestamp`（毫秒） | 否 | `seenIds` + 用量指纹（`10360`） | 三级链（`10355`）→ `codebuddy-unknown`（`9924`） | 丢弃（`10345`、`10476`–`10485`） |
| WorkBuddy | `entry.timestamp`（毫秒） | 否（`10919`） | `seenIds` + 旧版 entry-id（`11007`、`11008`） | `provider.model` → `requestModelId` → `entry.model`（`10987`）→ `auto`（`10678`） | 丢弃（`10968`） |
| Roo Code | `msg.ts`（毫秒） | 否（`11899`） | `taskUuid:ts`（`11902`）+ 文件 mtime 闸门 | 兄弟文件 `<environment_details><model>`（`11805`） | 丢弃（`11910`） |
| Cline | `msg.ts` + 导入闸门（`12352`） | 否 | 三键兼容（`12369`）+ 五路差分（`12383`） | `modelInfo.id` → sidecar → `provider:<名>`（`12178`） | 丢弃（`12381`） |
| Zed | `updated_at ?? created_at ?? thread.updated_at` | **是（now）**（`12764`） | SQL `updated_at > cursor`（`12636`）+ 线程差分 | `thread.model.model`（`12594`） | 六列全零不可能；**缓存-only 仍可 input=0**（`12751`、`12752`） |
| LM Studio | 日志行本地时间（`12979`）→ **文件 mtime**（`13129`） | **是（mtime）** | `dev/ino/size/mtime` 三态（`13325`） | `metadataBuffer` 的 `model`（`13091`） | 丢弃（归一化器由 total 反推 input） |
| Unsloth | `created_at`（`13677`） | 否 | 扫描窗口 `(created_at, id)` + 侧车指纹（`13714`） | `response_model` → …（`13650`） | 丢弃 |
| AnythingLLM | `createdAt ?? lastUpdatedAt`（`13856`） | 否 | 自增 `id` 水位（`14010`）+ `pendingChatIds`（`14024`） | `row.model` → `anythingllm-unknown`（`14040`） | 丢弃（`14016`） |
| Goose | `created_at` | **是（now）**（`14713`） | DB mtime 短路（`14639`）+ 会话累计差分 | `model_config_json.model_name`（`14529`，**无兜底**） | 可产 reasoning-only 行（`14686`） |
| Droid | **文件 mtime**（`15210`、`15328`） | **是（mtime）** | 会话累计 + mtime（`15243`） | `resolveDroidModel` 三级链（`15046`–`15057`） | total 兜底填充（`14972`） |
| Kilo Code | `msg.ts`（`15617`） | 否 | `taskUuid:ts` + `{size, mtimeMs, ino}` | 只报 `provider:<slug>`（`11730`） | 丢弃（`15628`） |

**读表要点**：本表 19 家里时间链**兜底到「当前时刻」或「文件触碰时刻」**的有 4 家（Zed、LM Studio、Goose、Droid）；把 §5.3.6 的同族一起算，**共 7 家**——本表 4 家 + `grok`（`20214` 的 `Date.now()` 末级兜底）、`craft`（`17223` `tsMs = stat.mtimeMs`）、`reasonix`（`17021` 的 `snapshot.stat.mtimeMs`），这 7 家的历史数据可能被记进「触碰时刻」的桶，与其余家「严格按日志时间戳落桶」的口径不可混用。另有 `copilot`(App DB) 属**同类破坏面的另一种机制**：它不是时间兜底，而是会话级累计快照——一条会话的全部历史 token 会落在「本次观测到的那个半小时桶」里（§5.3.6）。**模型来源**列还揭示一件事：19 家里只有 Kimi 一族的模型名来自**非日志来源**（配置文件），其余全部来自会话/消息记录本身，或来自上游改写过的展示名。

### 5.3 SQLite / 数据库类

本节写清**读取后端**与**各家的库级形态差异**。文本类小节里已经逐家写过的 7 家 SQLite（Kiro、Hermes、Kiro CLI、Zed、Unsloth、AnythingLLM、Goose）不重复展开，只在下表列出它们的读法与特例。

#### 5.3.1 共用读取层：只有两级后端（系统 CLI + `node:sqlite`）

- **一级（系统 `sqlite3` CLI）**：`execFileSync("sqlite3", ["-readonly","-json", dbPath, sql])`（`src/lib/sqlite-reader.js:33`–`45`），靠 `-readonly` 参数保证只读。
- **二级（Node 22+ `node:sqlite`）**：`new DatabaseSync(dbPath, { readOnly: true })`（`src/lib/sqlite-reader.js:67`–`79`）。
- 同步版 `readSqliteJsonRows`（`sqlite-reader.js:81`–`128`）；异步孪生 `readSqliteJsonRowsAsync`（CLI 走 `execFile`，`node:sqlite` 回落仍是同步，`sqlite-reader.js:148`–`195`）。
- 默认 `timeout = 30_000`、`maxBuffer = 50 * 1024 * 1024`（`sqlite-reader.js:91`–`92`）；两级都不可用时**只打印一次**警告（进程级 Set 去重，`17`–`31`），只有调用方传 `throwOnReadFailure` 才抛错（`119`–`126`）。
- **生产路径不使用 JS 内嵌引擎**：`src/**` 对 `sql.js` / `better-sqlite3` 的命中数为 0；`package.json` 的 `dependencies` 只有 `@journeyapps/wa-sqlite`、`@mongodb-js/zstd`、`smol-toml`、`undici`、`yauzl`（`engines.node >= 20`）。（`sql.js@1.14.2` 与 `@electric-sql/pglite` 只存在于 devDependencies，供测试构造 fixture。）
- **没有显式 `BEGIN`**：一致性靠「WAL 边车指纹 + 快照复制 + 读后重算指纹」实现（各家 `sqliteSidecarFingerprint`）。
- **UNC / WSL 例外**：`\\wsl$` / `\\wsl.localhost` 上的库直接开会被 Plan 9/SMB 桥拒锁（注释 `src/lib/rollout.js:7693`–`7696`），所以先 `snapshotSqliteDb(dbPath)` 复制到本地临时目录再读。命中该分支的至少包括 opencode（`5194`–`5199`）、kiro（`7315`–`7320`）、devin（`14184`–`14189`）、antigravity（`21573`–`21581`）、copilot session-store（`18681`）。
- **zstd 的真实用途（防误读依赖）**：`@mongodb-js/zstd` 只服务两家——Zed 线程 blob（`12514`–`12533`）与 **DSH 会话日志**（`23066`–`23072`）；DSH 优先用 Node 内置 `zlib.zstdDecompressSync`，只在 Node 20 回落该依赖。`@journeyapps/wa-sqlite` 只服务 Trae（`src/lib/trae-sqlite.js:7`–`15`）。全仓**没有任何 LZ4 代码路径**。

#### 5.3.2 逐库要点

| Provider | 库与表 | 读法要点 | 关键行号 |
|----------|--------|----------|----------|
| OpenCode（DB 支路） | `opencode.db`，`message`(v1) / `session_message`(v2) + `session`/`session_v2` | **四种库形态**须分别处理（纯 v1 / beta-17887 / upstream v2 / transitional）；一次往返同时探测「v2 有无数据 + 哪张 session 表存在」；`data` 是纯 JSON 文本，无压缩列 | `5041`–`5045`、`5108`–`5133`、`5143`–`5147`、`5215` |
| Qoder（旧库） | `local.db`，`chat_message` JOIN `chat_record` JOIN `chat_session` | 三表 JOIN，`role='assistant' AND token_info 非空`；走**异步**读取版（热路径不阻塞事件循环）；CN 版**刻意不复用国际版 `QODER_HOME`**（否则同一库双计） | `6030`–`6050`、`6129`–`6135`、`6112`–`6127` |
| Devin CLI | `sessions.db`，`message_nodes`(+`sessions`) | 先探测表存在再拼 JOIN；用 `json_extract`/`json_valid` 就地投影；**只投影 token 与归属字段**，正文/`cogs_json`/凭据状态从不选取 | `devin-usage.js:25`–`58`、`14172`–`14181`、`devin-usage.js:11`–`13` |
| Copilot session-store | `session-store.db`，`assistant_usage_events`(+`schema_version`) | 先探测表与 `schema_version`；`id > lastId` 增量；**读前整库快照**（`snapshotSqliteDb`）避免读到半写状态；`token_details_json` 决定 `exact`/`fallback` 两档精度 | `18264`–`18278`、`18414`–`18447`、`18681`、`18534`–`18576` |
| Copilot App | `data.db`，`sessions` | 先 `PRAGMA table_info(sessions)` 判定可用列，再按需生成 `NULL AS <col>` 占位（兼容老 schema）；含内容列（`title` 等）**从不进查询** | `19255`–`19286`、`19270`–`19271` |
| Antigravity | `conversations/<uuid>.db`，`gen_metadata` | 用 `quote(data)` 把 **BLOB 转十六进制文本**，再手工解 protobuf（字段号硬编码）——不是压缩列，是二进制 protobuf 列；「读失败」与「读成功但为空」严格区分（`null` = 可重试失败，空 Map = 真无元数据） | `21586`–`21593`、`21598`–`21599`、`21609`–`21611` |
| TRAE（国际版） | `ModularData/{ai-agent,ai-chat}/database.db`，`chat_turn` | **绕开系统 sqlite3 与 `node:sqlite`**：先逐页解密成内存快照（SQLCipher 4 raw key / AES-256-CBC / 4096 字节页 / SHA-512 HMAC），再用 `@journeyapps/wa-sqlite` 的只读 VFS 查内存快照，全程不落盘；写操作一律拒绝；带 3 次重试 + 稳定性校验，**绝不发布混合快照** | `trae-snapshot.js:38`–`45`、`trae-sqlite.js:19`–`32`、`74`–`76`、`116`、`trae-db.js:102`–`125` |
| Claude Science | 行由上层数据库读取层给出（`dbRows`） | `normalizeClaudeScienceTokens` 处理「主计数 + `aux_*` 辅助计数」两组，各自独立 clamp 后再相减（避免一组缓存掩盖另一组缺口）；`total <= 0` 返回 `null` | `7003`–`7027` |
| Zed | `threads.db`（BLOB） | `data_type === "zstd"` 才解压（Node 内置优先） | `12514`–`12533` |
| DSH | 会话日志（非 SQLite） | 自研 **zstd 拼接帧**解析：先扫 `contentSize` 声明做解压炸弹防护，再逐帧解码；单文件压缩 64MB / 解压后 128MB 上限 | `23060`–`23072`、`22818`–`22819` |
| DuMate / ZCode / Mimo | OpenCode **派生库**（同一 `message` 表 schema） | 见 §5.3.3 | `dumate.js:32`、`5380`–`5446`、`5477`–`5533` |

#### 5.3.3 DuMate / ZCode / Mimo：三家 OpenCode 派生库（词表在 `source` 里，但解析器复用）

- **DuMate（千帆桌面端）**：`parseDumateIncremental`（`src/lib/dumate.js:32`）先自己按 `providerID === "qianfanpersonalquota"` 过滤（`dumate.js:37`–`47`），再做**跨安装最新副本选择**（`timeUpdated` 比较，`dumate.js:52`–`56`），最后把消息交给 `parseOpencodeDbIncremental`（`source: "dumate"`、`cursorKey: "dumate"`，`dumate.js:80`–`82`）。库路径 `%APPDATA%\qianfan-desktop-app\qianfan_desk_xdg\…\data\opencode\opencode.db`（`dumate.js:7`–`30`，候选含多安装目录并用 `realpath` 去重）。**已知差异**：DuMate 的 OpenAI 兼容适配器把 reasoning 留在 output 里，因此该家把 output 减去 reasoning 后再交给共用解析器（`dumate.js:70`–`75`）。
- **Mimo（mimocode）**：库 `~/.local/share/mimocode/mimocode.db`（`MIMO_HOME` 覆盖；Windows 另探 `%APPDATA%`），读取器 `readMimoDbMessages` **按 `providerID ∈ {mimo, xiaomi}` 过滤**（`src/lib/rollout.js:5396`–`5407`）。原因为 mimocode 会把用户的 Claude Code / claude-mem 历史镜像进自己的 `message` 表：开发机上镜像量约 39 亿 token、真实 mimo 用量约 2200 万——**不过滤会把 Claude 用量双计并错标成 mimo**，因此有一次性修复（清空 `source=mimo` 的队列行与桶，`src/commands/sync.js:4777` 起）。
- **ZCode（Z.ai / 智谱）**：库 `~/.zcode/cli/db/db.sqlite`（`src/lib/install-resolver.js:52`–`79`，含 `%APPDATA%\.zcode\...` 与 WSL 分支）。**判别方向与 Mimo 相反**：ZCode 允许用户自定义 provider（providerID 是随机 UUID），因此**不能用白名单**，改用**黑名单**排除 `anthropic` / `openai` / `google` 三个直连厂商（`isZcodeNativeMessage`，`src/lib/rollout.js:5430`–`5439`，理由见 `5410`–`5429`：那些 turn 会写进 `~/.claude` / `~/.codex` / `~/.gemini`，由各自解析器计数）。另有两处库级差异：**ZCode 的父计数器是「包含式」的**（cache read/write 含在 input 里、reasoning 含在 output 里），故先用 `normalizeZcodeInclusiveTokens` 拆开再算队列行与成本（`5441`–`5464`）；同时它还有新版原生用量表 `model_usage`（`ZCODE_NATIVE_USAGE_COLUMNS` + `detectZcodeNativeUsageLayout`，`5477`–`5533`），空库/不可读的判定分成三态（`zcodeDbHasContent`，`5555`–`5590`）。
- 三家的 `source` 都进桶键（`dumate` / `mimo` / `zcode`），因此**在最终统计里它们与 `opencode` 是四家不同的来源**，共用的是读取实现而不是身份。

#### 5.3.4 Copilot 三通道：谁负责计数、谁只做观测

三份实现共用 `source = "copilot"`，靠三层机制避免重复计：

1. **用量认领（OTEL ↔ session-store）**：OTEL CLI span 结算后写 `recentUsageEvents`，session-store 通道消费时不重复计；`usageClaims` 保留 7 天、上限 10000 条（`src/lib/rollout.js:17375`–`17405`、`18058`–`18064`）。
2. **App DB 的基线采纳与跳过**：`shouldCountCopilotAppSession` 明确跳过「可识别的 Copilot CLI / VS Code 扩展会话」（`19213`–`19224`），注释直说这是为了避免 GitHub 把这些会话镜像进 `data.db` 后产生同源重复桶；当 session-store 通道已激活时，App DB **只观测不计数**（`observeOnly`，调用点 `src/commands/sync.js:3207`–`3221`）。
3. **首见不计数（adoption）语义**：新发现的库首见只建基线、不计数，除非 `backfillOnFirstRun`（`18717`–`18732`）；与 App DB 的残留对账只补差量（`legacyResiduals`，`18834`–`18856`、`18942`–`18949`）。

另需注意：session-store 是**跨 App/CLI 都完整**的本地源（`18122`–`18125` 注释），App DB 只保留 App 归属的那部分；行身份用 `sha256(session_id + 模型 + 五桶 + 时间戳)` 指纹（`18188`–`18211`），**重建/重索引时按历史指纹多重集逐一核销**（`18755`–`18806`），这是 45 家里最复杂的去重设计。

#### 5.3.5 Qoder 旧库 / 新 JSONL：同名同源的两套通道

- 旧库通道是**全量快照 + 幂等重算**（不是水位）：每次同步重算全部行，未变则跳过，变了就**先减旧桶再加新桶**（`6307`–`6363`）；`conversation_count` 由「整份有序快照重算的 request 归属」决定，而不是沿用上次归属（`6277`–`6286`，原因：Qoder 可能把 `token_info` 补挂到更早的 assistant 行上，沿用旧归属会把一个 request 抬成两个会话）。
- 新 JSONL 通道是**集合差量**：每次重建 `currentByKey`，与游标逐键比对，消失或变化的先撤回、再补加（`6701`–`6711`、`6751`–`6773`）。
- 两通道上传时**同 `source="qoder"`**（`6412`–`6414` 注释），靠 `messageKey` 前缀 `row:` / `jsonl:` 与游标命名空间 `qoder` / `qoderNew` 隔离；CN 版复用同一函数、换路径与 cursor 命名空间（`qoder-cn` 走旧库、`sourceKey` 覆盖）。

---

#### 5.3.6 数据库 / 自有快照类的逐家细节补全

（本节补总表第 20–42 行里 §5.2 未展开的家。格式统一为：库/文件 → 字段 → 模型 → 时间 → 游标与重置 → 特例。）

**OpenCode（SQLite 支路）** — 路径 `$OPENCODE_HOME/opencode.db`（`src/commands/sync.js:1369`、`1348`–`1355`）；无压缩列（`JSON.parse(row.data)`，`5215`）。字段复用 `normalizeOpencodeTokens`（`4755`，与磁盘支路同一张表）；模型 `dbModelId || DEFAULT_MODEL`（`5905`–`5906`），**一条 assistant 消息只落一个模型桶、不为多模型拆分**。时间 `msg.time.completed ?? msg.time.created`，无则丢弃（`5892`–`5903`）。游标 `cursors.<ns>.dbCursor = {version, v1:{maxRowId,maxUpdatedAt,anchor}, v2:{…}}`（`5054`–`5062`、`5314`–`5324`），谓词 `rowid > maxRowId OR time_updated >= maxUpdatedAt`（`5097`–`5106`）；**头部锚点**（读前后各取头部行 sha256）防「库被原地替换」（`5088`–`5095`、`5287`–`5308`），两次仍不稳定则返回行但**不推进游标**（`5327`–`5328`）。差分是**带符号**的（允许负值做校正，`4815`–`4836`）。

**Qoder（旧库）** — 路径优先级 `QODER_DB_PATH` → `QODER_HOME` → 平台默认（`6064`–`6077`）；CN 版用 `QODER_CN_DB_PATH`/`QODER_CN_HOME` 且**刻意不复用国际版变量**（`6112`–`6127`）。字段 `token_info.prompt_tokens`（含缓存，`input = prompt − cached`）、`cached_tokens`、`completion_tokens`（`6151`–`6178`）；模型三级回落 → `"qoder-agent"`（`6185`–`6195`）；时间 `gmt_create`（`6263`–`6266`），无 key/无 tokens 的行同时跳过（`6267`–`6270`）。`messageKey = session_id|id`，缺失回落 `row:<rowid>`（`6198`–`6205`）；`project_uri` 以 `file://` 开头时做一次 `decodeURIComponent(new URL(raw).pathname)`（`6207`–`6216`）。跨设备/多安装靠**独立游标命名空间**隔离，避免不同机的 `rowid` 互撞（`6114`–`6119`）。

**Qoder（新 JSONL）** — 目录 `~/.qoder/projects`（CN 版 `~/.qoder-cn`，`6432`、`6454`），覆盖变量 `QODER_PROJECTS_DIR` / `QODER_CN_PROJECTS_DIR`（`6417`–`6419`、`6436`–`6438`），文件经 `listQoderNewSessionFiles`（`6457`–`6474`）。只解析 `record.type === "assistant"` 且 `message.role === "assistant"` 的行（`6622`–`6624`）；字段见 §5.5.2。模型剥掉 CN BYOK 的「安装本地 UUID 前缀」（`qoder-custom-<uuid>/glm-5.3-flash` → 裸 id），使桶键跨重装稳定（`6479`–`6487`）；时间取 `record.timestamp` 或 `record.message.timestamp`（`6505`–`6511`、`6632`–`6633`）。一次性迁移把旧 `qoder` 游标里的 `jsonl:` 条目搬到 `qoderNew`（`6564`–`6590`）。

**Devin CLI** — 路径 `~/.local/share/devin/cli/sessions.db`（`TOKENTRACKER_DEVIN_DB` 覆盖，`14106`–`14126`）；**Windows 无原生路径，只能经 WSL 探测**（`14120`–`14125`）。表探测 `message_nodes`/`sessions`（`src/lib/devin-usage.js:25`–`26`），主查询 `json_extract` 投影 + `json_valid` + `role='assistant'` + `request_id` 非空（`38`–`58`）。字段四分项**互斥**（input 不做缓存扣减，源码给依据 `21`–`23`）；`input`/`output` 为 NULL 表示**记录尚未完成 → 整行不计数**，缓存列 NULL 当 0（`76`–`87`、`113`–`119`）；时间优先「原始生成时间」，注释明确 **`n.created_at`（节点插入时间）永不作兜底**（`121`–`126`）。增量两级：库级 `sqliteSidecarFingerprint`（`-shm` 被刻意剔除，避免只读打开 bump 触发重扫，`14130`–`14133`、`19297`–`19314`）；行级用**全安装规范的 `request_id` 账本**，`cursors.devin.requests[requestId]` 首次观察为权威，replay/fork/compaction 产生的多行同 request 记录由 `compareCandidates` 裁决（`devin-usage.js:149`–`156`、`14307`–`14347`）；账本用 `Object.create(null)` 防 `__proto__` 污染（`14136`–`14147`），两个队列都 append 成功才发布游标（`14149`–`14169`、`14239`–`14246`）。

**OMP / OmO（共享实现）** — 会话文件 `<ompHome>/agent/sessions/--<cwd-encoded>--/<ts>_<sessionId>.jsonl`（注释 `11405`–`11407`），主会话与子 agent 两类文件分开发现（`11352`–`11499`）；家目录覆盖 OMP 用 `OMP_HOME`/`PI_CONFIG_DIR`（`11355`–`11356`）、OmO 用 `TOKENTRACKER_OMO_HOME`（`11582`–`11595`）；游标命名空间 `cursors.omp` / `cursors.omo`。字段（`15868`–`15872`）：`input`（**互斥列，不扣缓存**）/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens`；**OmO 的 `reasoningIncludedInOutput = true`**（`16110`），因此 total 兜底求和时必须排除 reasoning（`15897`–`15915` 与注释 `15898`）；`reasoningFields` 差异：omp 只认 `reasoningTokens`、omo 也认 `reasoning`（`16096`、`16109`）。时间 `msg.timestamp`（毫秒）或 entry ISO 串，**带 2020-01-01 下限**以防「占位小数字落进 1970 桶」（`15701`–`15710`），无时间戳即跳过（`15885`–`15891`）；增量用文件偏移 + inode 变化检测（`15838`–`15847`），**只提交到最后一个完整换行**（撕裂尾巴下次重读，`15724`–`15747`），`seenIds` 上限 10000（`16041`–`16050`），单记录上限 64MB（`15712`–`15715`）；项目归属用**独立游标**（`projectSeenIds`/`projectFileOffsets`）以便升级后回填历史而不影响总量（`15941`–`15946`）。OMP 是唯一带 extension hook 主动上报的一家（`src/lib/omp-hook.js:16`、`48`、`66`、`117`、`135`、`139`、`143`；安装点 `src/commands/init.js:806`），OmO 纯被动。另有防撞车守卫：OMP 与 OmO / pi 的 agent 目录相同时跳过对应 provider，防止同一批会话被两个 source 各记一次（`16157`–`16169`；`src/commands/sync.js:2735`–`2741`、`2772`–`2774`）。

**pi / Prime Agent / MiniMax Code（共享实现）** — `source` 由 provider 派生（`piSourceForProvider`，`16286`–`16295`；`primeAgentSourceForProvider`，`16297`–`16305`；MiniMax 恒 `"minimax-code"`，常量 `16722`、注入 `16775`–`16785`），设计意图是「pi 是路由器，同一会话可能发给 Anthropic / Copilot / 其它后端，不能合并进一个桶或继承错误价格」（`16281`–`16285`）。路径：pi 为 `~/.pi/agent/sessions/**/*.jsonl`（`16171`–`16223`）+ `~/.multica/pi-sessions/*.jsonl`（`16200`–`16220`）；Prime 为 `~/.prime/agent/sessions/*.jsonl`（`16235`–`16275`）；MiniMax Code 为 `~/.minimax/v2/sessions/YYYY/MM/DD/<HH-MM-SS-mmm>-session_<id>/messages.jsonl`（注释 `16711`，覆盖 `16728`）。字段三家同表（`16449`–`16453`）：`input`（不扣缓存）/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens`，total 优先 `usage.totalTokens`、否则四分项 + reasoning（`16482`–`16485`）。时间 `msg.timestamp`（毫秒）优先、回落 entry ISO 串（`16466`–`16471`）——**MiniMax Code 例外：其记录包装器把外层 `timestamp` 恒置 `null`，ISO 回落分支永不触发，唯一有效路径是 `message.timestamp`**（`16766`–`16773`、`16467`、`16473`–`16476`）。增量与 OMP 同构（偏移 + inode + 只提交完整行 + `seenIds` 上限 10000，`16405`–`16426`、`16680`），解析前复制 `hourlyState.groupQueued` 与自有桶以保证队列写失败不污染调用方状态（`16369`–`16386`）。MiniMax Code 的 `usage.cost` 恒 0 被显式忽略、改由下游价格表算（`16716`–`16717`），历史迁移会话以 `model="historical-transcript"` + 全零 usage 重放并被零 token 护栏丢弃（`16717`–`16718`）。

**Craft Agents** — 磁盘布局 `~/.craft-agent/{config.json, workspaces/<id>/sessions/<sid>/session.jsonl}` + 自定义工作区根（注释 `16812`–`16816`）；**必须读 `config.json` 枚举自定义工作区**，不能只 glob 默认目录（`16818`–`16820`、`16862`–`16876`）。只读首行的预聚合头（读取上限 1 MiB，`17125`–`17135`；首行不是合法 JSON 即跳过，`17151`–`17158`）；字段 `usage.inputTokens`（**纯非缓存输入**，注释 `16822`–`16824`）/`outputTokens`/`cacheReadTokens`/`cacheCreationTokens`（`17172`–`17179`），delta 由会话头快照差分得出并 clamp 负值（`17189`–`17195`、`17236`–`17244`）。时间三级 `header.lastMessageAt` → `lastUsedAt` → `createdAt`，**末级兜底文件 mtime**（`17214`–`17223`）。游标 `cursors.craft.sessionTotals[sessionId]`（`17251`–`17258`），**上限 5000 条、按 `lastSeenAt` 最近优先淘汰**——注释说明用插入顺序淘汰会把长活会话的整份总量当增量重记（`17271`–`17275`）。`conversation_count` 是**会话级**且源码明确警告跨 provider 不可比（`17232`–`17235`）。

**Reasonix** — 只在 `~/.reasonix` / `%APPDATA%\reasonix` 下读 `*.jsonl.telemetry.json` 与其同名 `.meta`（`16959`、`16982`–`16985`、`16925`–`16934`），**刻意只读「无正文」文件以保证 prompt 与工具输出不外流**（`16907`–`16908`）；扫描深度上限 8（`16949`）。字段经 `normalizeReasonixTotals`（`16989`–`17005`）：`promptTokens` 按 `cacheMissTokens` 是否存在分两种语义算纯输入，`cacheHit`/`cacheMiss`/`cacheWriteTokens` 归缓存桶，`output = completionTokens − reasoningTokens`，**`conversation_count = requestCount`**（`17044`）。时间 `meta.updated_at` → `created_at` → 文件 mtime（`17016`–`17022`）；增量是累计快照差值且**只 clamp 不做负向回撤**（`17008`–`17014`），差值全 0 且请求数未增则整文件跳过（`17067`–`17068`）。**文件级身份只按路径，没有跨安装去重**（多份同名文件在不同 home 下会各算一次）。

**Copilot（OTEL 通道）** — 需要用户在 Copilot 侧开启 `COPILOT_OTEL_ENABLED` + 文件导出（注释 `17301`–`17306`）；读侧目录 `~/.copilot/otel`、`~/.copilot-otel` 或显式 `COPILOT_OTEL_FILE_EXPORTER_PATH`（`17309`–`17341`，Windows 另探 WSL `17328`–`17335`）。字段（`17501`–`17521`）：`gen_ai.usage.input_tokens`/`output_tokens`/`cache_read.*`（四镜像）/`cache_write.*`（四镜像）/`reasoning.*`（三镜像）；**CLI span 的 input 同时含 cache read 与 write（双扣），Chat 扩展的 LogRecord 只含 read（单扣）**——同一 provider 两代导出形态语义不同，是本项目唯一按代分别处理的地方（`17523`–`17535`）；reasoning 从 output 拆出（`17521`–`17522`、`17578`–`17579`）。模型 `gen_ai.response.model` → `gen_ai.request.model`（`17367`–`17373`）→ `normalizeCopilotAppModel`（`19244`–`19252`）→ `"github-copilot"`（`19033`）。时间是 OTEL 的 `[秒, 纳秒]` 数组（`17358`–`17365`，字段顺序 `endTime`→`startTime`→`hrTime`→`hrTimeObserved`，`17545`–`17549`）。去重三重：文件偏移 + inode（`17983`–`17992`，读后再 `stat` 吸收解析期间的追加，`18078`–`18086`）、记录级 `traceId:spanId` 或 `resp:<response.id>`（`17447`–`17460`）、解析器版本迁移（`COPILOT_PARSER_VERSION = 3`，`17375`；v1→v2 按行哈希跳过已计前缀 `17917`–`17927`，v2→v3 因 v2 误用 `spanContext` 作键而需先修复、修不了就**保持 v2 游标下次重试** `17930`–`17951`）。

**Copilot（session-store 通道）** — 覆盖变量 `TOKENTRACKER_COPILOT_SESSION_STORE_DB` / `COPILOT_HOME`（`18162`、`18146`–`18153`），文件名 `session-store.db`。字段**两档精度**（`18534`–`18563`）：若 `token_details_json` 的 `input + cacheRead + cacheWrite` 与 `output` 完全等于行内总量 → 采用明细、精度记 `exact`；否则按行内列扣减、精度记 `fallback`（`precision` 只用于诊断/披露，不进 totals，`1373` 段）；reasoning 被 clamp 到 output 内再拆出（`18565`–`18576`）。时间不可解析的行**登记进 `pendingMalformedEvents` 留待下次重试而不是丢弃**（`18800`–`18802`、`18870`–`18876`）。游标 `cursors.copilotStore.dbs[dbPath]` 含 `lastId`/`lastRowKey`（游标行 sha256）/`seenEventCounts`/`pendingMalformedEvents`/`dbIno`/`adoptedAt`；**id 回退或游标行指纹变化即判库被重建/重索引**并走全量对账（`18698`–`18720`）；同一物理库的多路径别名先合并状态防双计（`18610`–`18617`）。

**Copilot（App DB 通道）** — 路径 `~/.copilot/data.db`（覆盖 `TOKENTRACKER_COPILOT_APP_DB`，`19190`–`19198`）。字段是**会话级累计的差值**（`19357`–`19379`）：`total_input_tokens` 含缓存，故 `cachedDelta` 先 clamp 到 `inputDelta` 内再扣除；`reasoning` 从 output 拆出；**`cache_creation_input_tokens` 恒 0**（App DB 没有 cache write 列）。时间用排序后的 `updated_at`/`created_at`（`19266`–`19269`）经 `parseCopilotAppTimestamp`（兼容秒/毫秒数字与可解析字符串，`19227`–`19241`）——**由于是会话级累计，一条会话的全部历史 token 会落在「本次观测到的那个半小时桶」里**（与 Grok 的估算路径同类风险）。游标：库级指纹未变即跳过（`19439`）、会话级 `sessionTotals` 差值（`19419`–`19422`），会话总数上限 10000 且**非零会话优先保留**、零值会话按 `updatedAt` 倒序补位（`19334`–`19355`）。

**Grok Build** — 会话目录 `~/.grok/sessions/<encodedCwd>/<sid>/`，同时存在 `signals.json` 或 `updates.jsonl` 才纳入（`19566`–`19605`），家目录 `TOKENTRACKER_GROK_HOME` → `GROK_HOME` → `~/.grok`（`19553`–`19564`）。字段首选 `normalizeGrokUsage`（`src/lib/grok-usage.js:44`–`61`）：**camelCase 时 `inputTokens` 含缓存（要双扣），snake_case 时已是纯输入（不扣）**（语义注释 `grok-usage.js:3`–`10`），`reasoningTokens` 从 `outputTokens` 拆出，`totalTokens` 为 0 时返回 `null`（`grok-usage.js:63`–`90`）；**一家一次 turn 可拆成多个模型事件**（`usage.modelUsage` 逐模型，`19822`–`19839`），这是 45 家里唯一支持单请求多模型的实现。模型兜底 `"grok-build"`（`19675`–`19682`），免费 SKU 规范化为 `"grok-build-free"` 以免与付费 `grok-4.5` 模糊匹配（`19780`–`19791`）。时间三兜底 `event.timestamp` → `lastActive` → `Date.now()`（`20214`、`19684`–`19693`）。增量：文件偏移 + inode（`19870`–`19883`）且**只推进到最后一个换行处**（`19886`–`19900`、`grokFileEndsWithNewline` `19758`–`19777`）；会话快照 `cursors.grok.sessionSnapshots` 上限 10000（`19645`–`19649`）；游标版本 `GROK_CURSOR_VERSION = 5`（`19551`），升级时**清空 grok 桶**并从 `turn_completed.usage` 重建（`20099`–`20123`）。

**Antigravity** — 转录路径在 `<geminiHome>` 下的 `antigravity` / `antigravity-ide` / `antigravity-cli` 三个 `brain` 目录（`resolveAntigravityBrainDirs`，`20559`–`20566`），每个会话目录下的 `.system_generated/logs/transcript.jsonl`（`20604`–`20615`）；配对库由转录路径正则反推 `conversations/<uuid>.db`（`21558`–`21563`）。SQLite 路径字段号硬编码（`21505`–`21535`、落桶 `21794`–`21808`）；兜底路径用字符估算（见 §5.5.4）。模型三级（转录内「设置变更」文本 → `settings.json` → SQLite `f1.19`）并做 slug 归一：剥括号与 thinking/xhigh/high/medium/low/fast 词，再按 `gemini|claude|gpt` 截断，否则加 `antigravity-` 前缀（`21929`–`21949`；默认模型 `21904`–`21918`，桶内兜底 `"antigravity-unknown"` `21785`）。时间取转录事件 `parsed.created_at`，缺失则该事件**只累加上下文、不落桶**（`21771`–`21783`）。游标：`cursors.files[filePath]` 记 `lastLine`/`size`/`inode`/`contributions`/`cursorVersion`/`usageSource`（`20641`、`20645`），归属账本按 `source|model|bucketStart|projectKey` 聚合、贡献上限 4096（`20646`、`21847`–`21858`）；账本被截断且需要撤回时触发**源级重建**，但发现不完整（`inventoryComplete=false`）时改为**推迟该文件**以免抹掉未发现会话的贡献（`20858`–`20908`）；SQLite 读失败且上次来源是 sqlite 时整文件推迟、**不回退到估算**（`21682`–`21697`）。

**TRAE（国际版）** — 见 §5.3.2 的读法要点；补字段与游标：`normalizeTraeUsage` 先做「候选唯一解」判定（含/不含缓存与推理的 4 种组合必须只有一种与 `total_tokens` 自洽，否则整行放弃，`src/lib/trae-usage.js:63`–`81`），Gemini 行有两处显式修复（缓存双写误判与 thoughts 缺失残差，均标 `estimated`，`45`–`56`），另有一类「知道计量但无法拆分」的差额只进 `total_tokens` 并挂 `unpriced_input_tokens`（`83`–`92`）。模型归一：剥 `__` 后缀、小写、空格转 `-`，并把 `"claude-4-sonnet"` 映射到 `claude-sonnet-4-20250514` 以免模糊匹配丢缓存价（`98`–`105`）。时间 `created_at` 优先、`updated_at` 用于冲突裁决，**无兜底**（`rollout.js:22094`、`22107`、`22097`–`22100`）。游标用 128 位摘要身份（`digest`，`22062`）：`turn_id` 存在时用 `[session, turn_id]`、否则用 `[databaseKey, rowId]`（`22104`–`22111`），**跨两家国际版商店的副本靠 `T_STORE` 与 `T_MODIFIED` 裁决**（不让陈旧库回撤更新的用量，`22104`–`22111`）；版本 `TRAE_CURSOR_VERSION = 2`（`22002`），不匹配时先清 trae 桶重建（`22049`–`22054`），重建要求所有库都读成功否则整轮回退（`22159`–`22164`），重建后还要**撤回旧桶对应的队列行**（`22168`–`22173`）。

**DSH** — 路径 `~/.dsh/sessions/<project-key>/<session-id>/session[.v3].jsonl[.zstd]`（注释 `22878`–`22880`；家目录 `TOKENTRACKER_DSH_HOME` → `DSH_HOME` → `~/.dsh`，`22826`–`22833`，Windows 另探 WSL `22839`–`22865`）；同名文件多版本时按「最近写入 > 更高版本 > zstd」择一（`22901`–`22925`）。字段为五个**互斥**列（`23345`–`23365`，注释明确「DISJOINT…no cache subtraction, unlike Codex」）；模型回落链「事件内 `data.message.source.model` → request/header 配置 → 事件自身」，并只取最后一个 `/` 之后的名字（`23649`–`23652`、`23336`–`23342`），无模型即跳过（`23658`）。时间取事件级 `time`/`timestamp`（epoch 毫秒），缺失即跳过（`23660`–`23666`）。游标：文件身份 `cursors.dsh.files[path]`（inode/size/mtime，`23761`–`23793`）+ **记录级 `seq` 水位**（`23628`–`23645`），撕裂行要求「行自身是完整 JSON」才推进水位（`23610`–`23615`）；会话贡献账本支持「同会话换文件路径」的迁移（新路径接管先撤回旧路径贡献，`23717`–`23746`、`23766`–`23786`）。**隐私设计**：刻意不做整行 JSON 解码（用切片取字段），避免物化消息正文（`23368`–`23372`、`24244`–`24248`）；zstd 是自研**拼接帧**解析，先扫 `contentSize` 做解压炸弹防护，单文件压缩上限 64MB、解压后 128MB（`23060`–`23072`、`22818`–`22819`）。

**Command Code** — 路径 `~/.commandcode/projects/<cwd-slug>/<session-id>.jsonl`（注释 `24129`–`24132`，家目录 `24085`–`24089`、WSL `24095`–`24127`），排除 `.prompts.` 与 `.checkpoints.jsonl`（`24074`–`24081`）。字段：`usage.inputTokens` **含缓存 → 双扣**、`cacheReadTokens`/`cacheWriteTokens`/`outputTokens`，reasoning 恒 0、成本恒 0 哨兵（`24187`–`24215`）；**显式全零视为「校正」**（能撤回既有观测），空占位不算使用（`24199`–`24204`）。模型取行字段最后一个 `/` 之后（`24176`–`24185`），缺失落 `DEFAULT_MODEL`（`24283`）。时间戳必须是能被 `new Date(...)` 解析的字符串，否则该记录被丢弃（`24262`–`24276`）。游标**刻意不用字节偏移**（注释说明 resume/compaction 会重写文件）：重建每文件快照并与持久观测账本按 `recordId|timestamp` 对账（`24060`–`24066`、`24542`；另有 legacy 键形 `24544`），版本常量 `COMMAND_CODE_STATE_VERSION`/`FILE_CACHE_VERSION = 3`（`24068`–`24069`、`24473`–`24475`）；同一记录出现在非拥有者文件时**重读该文件**解析冲突而不是双记账本（`24614`–`24627`）；零 token 记录只在能撤回既有计数时保留（`24606`–`24612`）；成本无关的变化不触发入队（`24629`–`24641`）。

#### 5.3.7 多安装 / WSL / 跨设备处理速查

| 机制 | 涉及家 | 源码 |
|------|--------|------|
| UNC 路径先快照再读（避开 Plan 9/SMB 桥的锁失败） | `opencode`、`devin`、`antigravity`、`copilot`(store) | `5194`–`5199`、`14184`–`14189`、`21573`–`21581`、`18681` |
| WSL 家目录探测 + 原生/WSL 双路径解析 | `qoder`(两套)、`devin`、`dsh`、`command-code`、`copilot`、`omp`/`omo`/`pi`/`prime`/`minimax`/`craft`、`grok` | `6091`–`6109`、`6424`–`6452`、`14120`–`14125`、`22839`–`22865`、`24095`–`24127`；`pickWin32ProviderPath`（`7650`–`7653`） |
| 多安装游标命名空间隔离 | `opencode`（`native`/`wsl`/`flat`）、`zcode`（保留首次选择的安装，避免两个库累进同一个 flat 游标） | `src/commands/sync.js:1429`、`5788`；`rollout.js:5600`–`5605` |
| 路径相同时跳过第二家（防双计） | `omp` vs `omo`、`omp` vs `pi` | `16157`–`16169`；`src/commands/sync.js:2735`–`2741`、`2772`–`2774` |
| DB 别名路径合并（同物理库多路径） | `copilot`(store)、`copilot`(App) | `18610`–`18617`、`3153`–`3157` |
| 会话级跨设备对账（云端 LWW） | `trae-cn` | `22532`–`22595` |
| 副本/分叉去重（本地） | `opencode`（#426 指纹）、`devin`（`request_id` 账本）、`trae`（turn 摘要账本）、`command-code`（`recordId\|timestamp`）、`claude`（fork 别名） | `5847`–`5888`、`14303`–`14347`、`22104`–`22111`、`24542`、`989`–`1013` |
| 复制型安装的「最新副本优先」 | `dumate`（跨安装同消息取最新 `timeUpdated`） | `src/lib/dumate.js:52`–`56` |

#### 5.3.8 读取参数与容量上限速查（内存 / 延迟相关）

| 常量 / 参数 | 值 | 位置 | 作用 |
|-------------|----|------|------|
| SQLite 读取层默认超时 / `maxBuffer` | 30s / 50MB | `src/lib/sqlite-reader.js:91`–`92`（异步同值 `158`–`159`） | 所有 SQLite 读取的默认值 |
| 逐家上调的读取缓冲 | Devin 32MB/30s；Copilot store 64MB/60s；Copilot App 64MB/60s；Antigravity 64MB/15s | `14172`–`14181`、`18430`–`18437`、`19287`–`19294`、`21586`–`21593` | 大库专项参数 |
| OMP / pi 家族单记录上限 | 64MB | `15712`–`15715` | 防单条超大记录拖垮解析 |
| `seenIds` 去重集合上限 | 10000 | `16041`–`16050`（OMP/OmO）、`16680`（pi 家族）、`18089`–`18091`（Copilot OTEL） | 超限后先入先出淘汰 |
| CodeBuddy 用量指纹表上限 | 10k | `10022` | 同上 |
| Craft `sessionTotals` 上限 | 5000，按 `lastSeenAt` 最近优先淘汰 | `17271`–`17275` | 防止淘汰长活会话后把整份总量当增量重记 |
| Zed `threadTotals` 上限 | 10000 | `12816` | 线程累计差分表 |
| Grok 会话快照上限 | 10000 | `19645`–`19649` | 会话级基线 |
| Copilot App 会话表上限 | 10000（非零会话优先保留） | `19334`–`19355` | 保留策略 |
| Copilot `usageClaims` 窗口 | 7 天 / 10000 条 | `17375`–`17405`、`18058`–`18064` | 跨通道用量认领 |
| Antigravity 归属贡献上限 | 4096 | `20646` | 超限且需撤回 → 源级重建；发现不完整 → 推迟 |
| DSH 单文件上限 | 压缩 64MB / 解压后 128MB | `22818`–`22819` | 解压炸弹防护 |
| LM Studio 扫描窗口 | 8MB 窗口 / 64KB 分块 | `12853`、`12854` | 文本日志字节扫描 |
| Craft 单文件读取上限 | 1 MiB | `17125`–`17135` | 只读首行头 |
| Unsloth 尾部保留 | 末 256 行 | `13469` | 扫描窗口回落区间 |

### 5.4 API / 凭据类

45 家里**只有两家**必须联网且必须读取本机登录凭据才能产出任何数据，且两家都是**账号级来源**（`ACCOUNT_LEVEL_SOURCES = new Set(["cursor","trae-cn"])`，`src/lib/source-metadata.js:1`，用于把「仅本地」视图过滤掉它们）。

#### 5.4.1 `cursor` — 远程 CSV 导出 + `state.vscdb` 取 JWT

- **触发条件**：`sourceAllowed("cursor")` 即尝试；未安装 Cursor 或取不到凭据则**整段 try/catch 跳过**，只打印一行错误，不影响其它 provider（`src/commands/sync.js:2158`–`2194`）。
- **凭据来源**：SQLite `Cursor/User/globalStorage/state.vscdb`，`SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'`（`src/lib/cursor-config.js:46`–`47`），走 §5.3.1 的两级后端（`timeout: 5000`、`maxBuffer: 1MB`，`56`–`66`）。`userId` 取 `~/.cursor/cli-config.json` 的 `authInfo.authId`，失败则从 JWT 解出（`102`–`107`）；最终组装 `WorkosCursorSessionToken=<userId>%3A%3A<jwt>` cookie（`109`–`111`），并有 WorkOS 主体前缀白名单（`114`–`116`）。
- **端点与超时**：`https://cursor.com/api/dashboard/export-usage-events-csv?strategy=tokens`、`https://cursor.com/api/usage-summary`、`https://api2.cursor.sh`（`150`–`152`）；CSV 与 summary 各 30s、RPC 15s（`196`、`231`、`367`）。
- **失败降级**：没有 refresh token 流程、也**没有「仅本地估算」开关**——CSV 拿不到就是没有数据。与其它家的差别是**解析器本身不碰网络**：它接收的是上游已经解析好的 CSV 记录（`7164`–`7170` 注释）。
- **增量语义（与所有本地家根本不同）**：CSV 是账号级导出、不是追加型本地日志，因此按「先清桶再重建」处理，且清桶范围有护栏——只清「本次导出最早记录」之后的桶；**一条日期都解析不出来就完全不清桶**（避免把整段历史写成 0），`7184`–`7210`。游标 `cursors.cursorApi.lastRecordTimestamp` 只作展示/诊断，**不参与过滤**（`7244`–`7247`、`7263`–`7268`）。
- **成本与计数**：`total_cost_usd` 从不写入（成本交下游价格表，`cursor-config.js:525`–`536` 注释）；`conversation_count` 每条 CSV 记录 +1（`rollout.js:7232`）——**是请求级计数而非会话级**。历史坑：早期版本因 Cursor 新增 `Cloud Agent ID` / `Automation ID` 列而把记录写成 `model="unknown"`，有一次 purge + 游标重置（`src/commands/sync.js:2144`–`2152`）。

#### 5.4.2 `trae-cn` — AES 加密凭据 blob + 强制 opt-in

- **触发条件（三重）**：非轻量后台同步 **且** `TOKENTRACKER_TRAE_CN_USAGE` 显式为 `1`/`true` **且** `sourceAllowed("trae-cn")`（`src/commands/sync.js:2234`–`2238`）；调用侧固定 30 天滚动窗口并对齐半小时（`2241`–`2250`）。
- **隐私边界（唯二的强制闸门）**：源码注释写明「该读取会把本机存储的登录 JWT 发给 TRAE 官方端点，因此严格 opt-in：除非用户显式设置该标志，否则什么都不会发出；除 1/true 外任何值都视为关闭」（`src/lib/trae-cn-config.js:24`–`33`）。凭据来自 `%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json` 的 `iCubeAuthInfo://icube.cloudide`，可能是明文 JSON，也可能是 Base64 的 tc v5 密文（`[6 字节 magic][32 字节 salt][AES-128-CBC 密文]`，key/iv 由 `SHA-512(salt)` 与 `JG xor KG` 两次 KDF 导出；`trae-cn-config.js:11`–`16`、`35`）。
- **端点与请求**：`https://api.trae.cn/trae/api/v1/pay/query_user_usage_group_by_session`，POST + `Authorization: Cloud-IDE-JWT <jwt>`，body 含 `usage_type: [7]`、分页 20 / 最多 100 页 / 页间 300ms / 超时 30s（`37`–`45`、`332`–`344`）。`usage_type` 常量取自「官方客户端用量页实际发出的请求」，注释明确该枚举服务端无文档、其它取值未验证。
- **失败降级**：**401/403 时重读并重新解密 `storage.json` 后重试恰好一次**，不做 refresh token 流程，且**从不持久化或打印 JWT / refresh token**（`486`–`521`）；容量超限时递归二分窗口（最多 8 层），二分仍超限则 fail-closed（`46`–`49`、`465`–`483`）。
- **快照契约（fail-closed）**：空响应是**成功空操作**（不当作「全部删除」）；单个畸形行让**整份快照失败**，不作为权威状态入队——源码给出证据说明「会话集合稳定」但「响应中缺席 ≠ 权威删除」未被证明，故缺席契约必须保持对称（`22626`–`22634`）。持久化贡献在减去前逐字段校验（安全整数、`reasoning=0`、`conversation_count=1`、`total=input+cached+cacheWrite+output`、`billable=total`、桶起点必须是标准 UTC 半小时），撤回前还要断言旧贡献不超过当前桶（`22402`–`22413`、`assertTraeCnBucketCovers` `22513`–`22530`）。跨设备另写 `kind: "account_session_state"` 队列记录，带 `snapshot_verified_at` 逻辑戳做云端 LWW 收敛（`22532`–`22595`）。
- **语义判断的来源**：`input_token` 是「含缓存」这一点，是**用 TRAE CN 自家积分账单反推验证**出来的（`credits = fresh_input*p + cache_read*(p/4) + output*q` 对全体观测行零残差），而**不是**文档（`rollout.js:22467`–`22474`）。若 TRAE 调整积分权重，该结论需重验。

#### 5.4.3 两家账号级来源对照

| 维度 | `cursor` | `trae-cn` |
|------|----------|-----------|
| 触发条件 | `sourceAllowed("cursor")` 即尝试；未安装 / 无凭据整段跳过（`src/commands/sync.js:2158`–`2194`） | 非轻量同步 **且** `TOKENTRACKER_TRAE_CN_USAGE` 为 `1`/`true` **且** `sourceAllowed("trae-cn")`（`src/commands/sync.js:2234`–`2238`） |
| 凭据位置 | `state.vscdb` 的 `cursorAuth/accessToken`（`src/lib/cursor-config.js:46`–`47`）+ `cli-config.json` 的 `authInfo.authId`（`102`–`107`） | `storage.json` 的 `iCubeAuthInfo://icube.cloudide`（明文或 Base64 的 tc v5 密文）（`src/lib/trae-cn-config.js:35`、`11`–`16`） |
| 鉴权形式 | `WorkosCursorSessionToken=<userId>%3A%3A<jwt>` cookie（`109`–`111`，含 WorkOS 主体前缀白名单 `114`–`116`） | `Authorization: Cloud-IDE-JWT <jwt>` 头（`332`–`344`） |
| 端点与超时 | CSV 导出 + usage-summary（各 30s）、RPC（15s）（`150`–`152`、`196`、`231`、`367`） | 单一 usage POST（30s；分页 20、最多 100 页、页间 300ms）（`37`–`45`） |
| 失败降级 | 无 refresh 流程、**无本地估算兜底**——拿不到就是没有数据 | 401/403 时**重读并重新解密一次**；容量超限递归二分窗口（≤8 层），仍超限 fail-closed（`486`–`521`、`46`–`49`、`465`–`483`） |
| 凭据落盘/打印 | 不落盘，仅内存 cookie | **从不持久化或打印 JWT / refresh token**（`486`–`521` 注释） |
| 增量语义 | 账号级快照替换 + 清桶护栏（只清「本次导出最早记录」之后的桶；一条日期都解析不出则**完全不清桶**）（`src/lib/rollout.js:7184`–`7210`） | 会话级快照对账，全部 fail-closed；空响应 = 成功空操作；单行畸形 → **整份快照失败**（`22626`–`22634`） |
| 计数口径 | `conversation_count` = CSV 记录数（请求级，`7232`） | 每会话行恒 1（`22500`–`22508`） |
| 隐私边界 | 账号级来源，被排除在「仅本地」视图外（`src/lib/source-metadata.js:1`） | 同上，且是**唯一需要显式 opt-in 才会发出网络请求**的来源 |

#### 5.4.4 opt-in 开关与路径覆盖的边界

**真正意义上的「功能 opt-in 开关」只有 `TOKENTRACKER_TRAE_CN_USAGE` 一个。** Copilot 的三个 `COPILOT_OTEL_*` 是「让上游工具开始导出」的前置条件（对 TokenTracker 而言是数据存在性条件，不是自身开关）；其余 `TOKENTRACKER_*` 变量全部是**路径覆盖**，不具备关闭能力。`TOKENTRACKER_TRAE_SQLCIPHER_KEY` 也不是 opt-in，而是 Trae 库密钥的可选覆盖（默认使用内嵌共享密钥，`src/lib/trae-snapshot.js:9`–`10`；覆盖与格式校验 `38`–`40`；状态页会披露用的是哪个来源，`src/commands/status.js:1215`）。

---

### 5.5 估算与非 token 单位

本节把「不是真实 token 计数」的取值全部列出，含**反例**（明确拒绝把非 token 单位当 token 的家）。

#### 5.5.1 Kiro CLI：字符估算（何时用真实值、何时回退）

- **先真实、后估算**：legacy 会话 `.json` 支路取 `turn.input_token_count` / `turn.output_token_count`（`src/lib/rollout.js:8371`、`8372`），**只有两者皆为 0 才回退**字符估算（闸门 `8374`，估算点 `8386`、`8387`）。
- **纯估算支路两条**：SQLite `data.sqlite3`（`9152`–`9159`，乘法点 `9155`）与 v2 `messages.jsonl`（该文件不含 token 字段，注释 `8475`–`8481`）；换算率统一 `KIRO_CLI_CHARS_PER_TOKEN = 4`（`8118`，其余使用点 `8386`/`8387`/`8429`/`8430`）。
- **落桶闸门**：`approxInput > 0 || approxOutput > 0 || approxReasoning > 0`（`9209`）——**六列全零 + total>0 的形态不出现**。
- credits 不进 token 桶，只写侧车 `kiro-credits.json`（`8119`、`8955`）。

#### 5.5.2 Qoder：`credits` 进 totals 但不换算

- 全项目唯一把 `credits` 写进 totals 的地方：`rollout.js:6541`（`credits: Number.isFinite(credits) && credits > 0 ? credits : 0`），来源 `usage.credits ?? usage.original_credits`（`6515`）。
- **不换算、不计入 total**：`addTotals` 只识别九个字段（`4099`–`4115`），`credits` 不在其中，因此**当前不构成数值污染**。真正的风险是下游若按 totals 的任意字段做泛化求和会误用 credits。
- **悬空注释（注释与实现不一致，须登记）**：`6726` 写着「credits cost 很小，确保它被累积」，但下一行 `6727` 就是 `touchedBuckets.add(...)`——**上下文里没有任何 credits 累积语句**。同家注释明确拒绝换算（`6405`–`6411`：没有第一方证据支持 credit→token 汇率，`total_cost_usd` 保持 0）。

#### 5.5.3 Grok Build：上下文窗口 token 按 0.8 硬拆 input/output

- **触发条件**：该会话历史上从未出现 `turn_completed`（`rollout.js:20238`–`20241`）。
- **数据源是上下文窗口的瞬时大小，不是可计费累计**（源码自己的警告 `19704`–`19706`）：水印取自 `signals.totalTokens` / `totalTokensBeforeCompaction` / `contextTokensUsed`（`grokEffectiveTotalFromSignals`，`19707`–`19718`）。
- **硬拆比例**：`GROK_ESTIMATED_INPUT_RATIO = 0.8`（`19549`），`input = round(total × 0.8)`、`output = total − input`，且**该路径的 cached 与 reasoning 恒为 0**（`19945`–`19961`）。
- **免责披露**：该路径写 `usage_precision = "estimated"` 并参与桶级精度合流（`20255`、`20268`、`20286`–`20289`）；同一桶内混有 reported 与 estimated 时标记为 `"mixed"`（`19964`–`19968`）。因为缓存与推理恒 0，**估算行的价格会系统性地与真实行不同**。
- 上游成本（唯一直接用上游美元 ticks 的本地 provider）：`USD_TICKS_PER_USD = 10_000_000_000`（`grok-usage.js:12`），`ticks / 1e10` 得美元；`costIsPartial` / `usageIsIncomplete` 为真时返回 `null`（不写假精确的 0），`grok-usage.js:26`–`39`。
- 版本迁移的教训值得抄：`GROK_CURSOR_VERSION = 5`（`19551`），注释说明 v3 及更早把「上下文窗口总量」当「累计消费」、v4 仍重叠 output/reasoning、丢弃 cache creation、忽略上游成本，因此 v5 升级时**清空 grok 桶并从 `turn_completed.usage` 重建**（`20099`–`20123`）。

#### 5.5.4 Antigravity：字符估算当 token，且不写 `usage_precision`

- 兜底路径的估算器 `estimateAntigravityTokens`（`21971`–`21983`）：CJK 码点按 1 token、其它按 `ceil(n/4)`（`isCjkCodePoint` 覆盖 `0x3400`–`0x4dbf`、`0x4e00`–`0x9fff`、`0x3040`–`0x30ff`）。
- 该路径把估算值直接写进 input/output/reasoning（`21821`–`21833`），且 `input` 是**上下文增量**而非真实 prompt 长度（上一轮 planner 的历史在下一轮被计为 input，注释 `21710`–`21713`、`21881`–`21887`）；两个缓存桶保持 0。
- **披露缺口**：来源标记 `usageSource: "estimated"` **只记在游标里**（`21898`），**没有任何 `usage_precision` 写进队列行**——`usage_precision` 的全部写入点清单里没有 antigravity（清单见 §5.6 末）。因此**下游无法区分 antigravity 的 SQLite 精确行与字符估算行**，这是本章范围内最明确的披露缺口。
- 与之对照，Trae 会把精度写进队列行的 `usage_precision`（`22144`–`22158`）。

#### 5.5.5 非 token 单位清单（含反例）

| 家 | 非 token 量 | 处理方式 | 是否污染 token 列 | 行号 |
|----|-------------|----------|------------------|------|
| `qoder`（新 JSONL） | `credits` / `original_credits` | 原样挂到 totals 的 `credits` 键；不换算、不入 total | 否（`addTotals` 白名单外） | `6515`、`6541`、`4099`–`4115` |
| `reasonix` | `requestCount`（请求数） | **当 `conversation_count` 用**——请求数冒充会话数 | 否（不是 token，但污染「会话数」指标） | `17004`、`17044` |
| `grok` | 上下文窗口 token | 按 0.8 硬拆 input/output，标 `estimated` | 是（估算 token 进桶，但被标精度） | `19549`、`19945`–`19961`、`20255` |
| `grok` | 上游美元 ticks | 走独立字段 `total_cost_usd`，不碰 token 列 | 否 | `grok-usage.js:12`、`26`–`39` |
| `antigravity` | 字符数 | 估算成 token 写进三桶 | 是（且**不标精度**） | `21971`–`21983`、`21821`–`21833`、`21898` |
| `kiro`（CLI） | 字符数 | 估算成 token（legacy 支路优先真实值） | 是（标不到精度） | `8371`–`8387`、`9152`–`9159` |
| `trae`（国际版，Gemini 特例） | 「知道计量但无法拆分」的残差 | 反推 `reasoning`（仅 Gemini 模型 + 缺 thoughts + 残差为正），标 `estimated`；另一类差额进 `unpriced_input_tokens` | 是（被标精度；不可定价部分**只进 total 不进可定价列**） | `trae-usage.js:49`–`56`、`88`–`91`、`94` |
| **反例** `cursor` | CSV 的 `Cost` 列 | 只记 `record.cost`，**从不写 `total_cost_usd`**、不参与 token | 否 | `cursor-config.js:498`、`512`–`538` |
| **反例** `command-code` | 上游 `costUsd` | **主动丢弃**并写零成本哨兵 `total_cost_usd: 0`；注释说明那是「显示价」不是账单价 | 否 | `24055`–`24058`、`24213` |
| **反例** `qoder`（旧库） | —（无 credits 概念） | 只有 token 字段 | 否 | `6148`–`6179` |

**净结论**：真正把**非 token 计费单位**（credits）当 token 的只有半家（`qoder` 新通道），且因为累加器白名单而**没有实际数值污染**；其余「估算」都是 token→token 的推算，风险集中在**精度披露**（antigravity 完全不标、kiro 无标记）而非单位混淆。

---

#### 5.5.6 精度标记（`usage_precision`）逐家覆盖

| 家 | 是否写 `usage_precision` | 取值 | 依据 |
|----|--------------------------|------|------|
| `grok` | 是 | 兜底路径 `"estimated"`；同桶混装时升为 `"mixed"` | `20255`、`20268`、`20286`–`20289`、`19964`–`19968` |
| `trae` | 是 | `"estimated"` 或缺省 `null` | `22144`–`22158`、`src/lib/trae-usage.js:94` |
| `qoder`（新 JSONL） | 是（显式写 `null`） | `null` | `6542` |
| `antigravity` | **否**（只在游标里记 `usageSource`） | — | `21898` |
| `kiro`（CLI 字符估算） | **否** | — | `8371`–`8387`、`9152`–`9159` |
| 其余各家 | 否（无估算路径，无需标注） | — | 写入点清单见下 |

`usage_precision` 的全部写入点集中在 `src/lib/rollout.js` 的 **16 行 / 15 个独立语句**：`3325`、`3341`、`3403`、`3422`、`6542`、`6648`、`6725`、`6731`、`6755`、`6760`、`20286`–`20287`（跨行一条赋值）、`22011`（`22009`–`22011` 数组字面量续行）、`22025`、`22113`、`22157`。清单里**没有 `antigravity` 与 `kiro`**——这是 §5.6.6 里「披露缺口」那一条的否定性证据。统计口径限 `src/lib/rollout.js`；不含 `src/lib/session-analytics.js`（5 行）、`src/lib/trae-usage.js`（1 行）、`dashboard/src/lib/sessions-api.ts`（1 行）与测试用例。

#### 5.5.7 字段名速查（19 家的日志字段原文）

给需要写解析器或写 fixture 的读者：这一列是**上游日志里的原始键名**，不是桶名。

| Provider | 日志字段（原文） | 备注 |
|----------|------------------|------|
| Gemini | `tokens.input` / `.cached` / `.output` / `.tool` / `.thoughts` / `.total` | `tool` 并入 output |
| OpenCode | `tokens.input` / `.output` / `.reasoning` / `.cache.read` / `.cache.write` | 嵌套 cache |
| OpenClaw | `message.usage.input` / `.cacheRead` / `.cacheWrite` / `.output`（`totalTokens` 仅预筛） | input 先减 cacheRead |
| Kiro | `tokens_prompt` / `tokens_generated` | 无缓存列 |
| Hermes | `input_tokens` / `output_tokens` / `cache_read_tokens` / `cache_write_tokens` / `reasoning_tokens` / `message_count` | 会话数 = 消息数差分 |
| Kiro CLI | SQLite：`user_prompt_length` / `response_size` / `reasoning_size`（字符 ÷4）；legacy 会话 `.json`：`input_token_count` / `output_token_count`（**真实值**，皆为 0 才回退估算）；v2：`messages.jsonl` 字符数 ÷4 | 另有 `request_start_timestamp_ms` / `model_id`；credits 只写 sidecar |
| Kimi | `token_usage.input_other` / `.output` / `.input_cache_read` / `.input_cache_creation` | 秒级时间戳 |
| Kimi Code | `usage.inputOther` / `.inputCacheRead` / `.inputCacheCreation` / `.output`（或 Anthropic / OpenAI 形状） | 毫秒时间戳 |
| CodeBuddy | `cachedMissTokens` / `input_tokens` / `prompt_tokens`；`output_tokens`；`cache_read_input_tokens` / `cached_tokens` | 多镜像优先级 |
| WorkBuddy | `rawUsage.prompt_tokens` / `.completion_tokens` / `.cache_read_input_tokens` / `.cache_creation_input_tokens` / `completion_tokens_details.reasoning_tokens` | prompt 减双向缓存 |
| Roo Code / Kilo Code | `payload.tokensIn` / `.tokensOut` / `.cacheReads` / `.cacheWrites` | 同形状，source 不同 |
| Cline | `metrics.inputTokens` / `.outputTokens` / `.cacheReadTokens` / `.cacheWriteTokens` / `.reasoningTokenCount` / `.cost` | 唯一产 cost |
| Zed | `input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` | 线程四元组 |
| LM Studio | `prompt_tokens` / `completion_tokens` / `total_tokens` / `*_details.cached_tokens` … | 字节扫描 |
| Unsloth | `prompt_tokens` / `completion_tokens` / `total_tokens` / `cached_tokens` / `cache_write_tokens` / `reasoning_tokens` | 共用归一化器 |
| AnythingLLM | `prompt_tokens` / `completion_tokens` / `total_tokens` | 差额判为 reasoning |
| Goose | `accumulated_total_tokens`（回退 `total_tokens`）/ `model_config_json` | 无缓存列 |
| Droid | `tokenUsage.inputTokens` / `.outputTokens` / `.cacheCreationTokens` / `.cacheReadTokens` / `.thinkingTokens` / `.totalTokens` | total 兜底 |
| Claude Science | `input_tokens` / `cache_read_tokens` / `cache_write_tokens` / `output_tokens`（各含 `aux_*` 辅助组） | input 为 cache-inclusive，需减去两组缓存 |

### 5.6 风险清单（对下游最有价值）

#### 5.6.1 「只有 total 没有分桶 → $0 成本」：逐家判定

**计价侧前提（先读这条）**：`src/lib/pricing/index.js:248`–`256` 只把 `codex` / `acode` / `every-code` / `omo` / `cline` **五家**的 `reasoning_output_tokens` 视为 output 的子集（`reasoningIncludedInOutput`），集合外 source 的推理**按 output 单价单独计费**。下文所有「非 $0」判定都以此为前提。

| 类别 | 家 | 判定 | 依据 |
|------|----|------|------|
| **A. 可从源码推出 $0 退化路径（3 家）** | `gemini` | `total = max(上报值, 分桶和)`（`4743`）+ 闸门把 `total_tokens` 当非零（`4945`、`2971`）→ 若日志只写 `tokens.total`，产出 input/output/cache 全 0 的队列行；此时 `thoughts` 同为 0，故该行**确实为 $0** | `4734`、`4743`、`2971`、`4945` |
| ↑ 同 A 类 | `grok` | 归一化器**先取上报 `totalTokens`、仅当 ≤0 才回退求和**，唯一拒绝条件是 `total <= 0`（`grok-usage.js:63-71`）→ `usage = {totalTokens: N}`（分量字段缺失）即得「分量全 0、total=N」；消费侧**没有任何分量闸门**，把 `event.total_tokens` 等直接写进 delta 并落桶（`20217`–`20228`，`conversation_count: event.conversation_count \|\| 1`） | `grok-usage.js:63-71`、`20217`–`20228` |
| ↑ 同 A 类 | `codex`（含 acode / every-code 同格式） | `normalizeUsage` 只对 input 做缓存减法、**total 原样保留**，注释自述保留理由包含「`some component fields are absent`」（`4873`–`4876`）；唯一前置拒绝是 `isCodexTotalOnlyResetSentinel`（`4881`–`4893`），它只拒「last 有 total 而累计 total 归零」这一特定 reset 形态，**不拒「分量全 0、total>0」的普通增量**；随后的闸门 `isAllZeroUsage` 也把 `total_tokens` 算作非零（`4945`–`4953`、`2553`）→ 同样落出 $0 行 | `4851`–`4879`、`4873`–`4876`、`4881`–`4893`、`2553` |
| **B. 弱化版（2 家）**：可产出 `input=0/output=0/reasoning>0`；因不在上述五家集合内，**不为 $0** | `goose` | `reasoningDelta = max(0, dTotal − dInput − dOutput)`（`14711`），`dTotal > 0` 即落桶，continue 条件为三项皆 0（`14686`） | `14711`、`14686` |
| ↑ 同 B 类 | `kiro`（CLI 会话支路） | 会话支路零值闸门只检 4 列、**不含 reasoning**（`9414`），随后 delta 含 `total_tokens: … + reasoning`（`9446`）→ 同形；SQLite 支路有 `approx* > 0` 闸门（`9209`），不产该形态 | `9414`、`9446`、`9209` |
| **C. 明确规避（3 家）** | `droid` | `applyDroidTotalFallback`（`14972`）把 total 缺口填进 `output` 或 `thinking`，结构性规避 | `14972`–`14979` |
| ↑ 同 C 类 | `lmstudio`、`unsloth` | `input_tokens = max(0, total − completion − cacheRead − cacheWrite)`（共用归一化器 `13040`） | `12994`、`13039`–`13049` |
| **D. 不产 $0 行，但把纯 total 行静默丢弃（10 家）** | `kimi`(9590)、`kimi`(KimiCode, 9823)、`workbuddy`(10968)、`openclaw`(2109)、`roocode`(11910)、`kilo-code`(15628)、`cline`(12381)、`anythingllm`(14016)、`kiro`(IDE, 7547)、`codebuddy`（扩展日志闸门 `10345`；JSONL 支路五桶零闸门 `10476`–`10485`） | 队列行不会被写出，代价是**丢失纯 total 的用量** | 括号内行号 |
| **E. 结构上不可能出现「六列全零 + total>0」（3 家）** | `zed`（`12598`、`12752`）、`hermes`（SQL `WHERE`，`7718`）、`opencode`（`3109`） | 限定：仅指上述六列全零形态；**Zed 在只有缓存列增长时仍会落出 `input=0/output=0/total>0` 的行**（`totalDelta` 只由 4 桶求和 `12751`、`<= 0` 跳过 `12752`），Cline 同理（`12403` 的 `total_tokens: deltaTotal`）——下游不得把 E 类读成无条件安全 | 括号内行号 |
| **F. 未判定** | — | **0 家**（原唯一存疑的 Codebuddy 两条支路都有零值闸门，已并入 D 类） | `10345`、`10476`–`10485` |

`【未能确认】` **A 类的真实触发频率**：本类是**条件形态**——「若日志里出现只写 total 的行，就会落出 $0 行」由源码可证，但三家（`gemini` / `grok` / `codex`）的日志里是否真会出现这种行、出现多少，本轮无真实语料，**不能读成「这三家一定在产 $0 行」**。判定条件：对三家各取一段真实会话日志（Gemini `session-*.json`、Grok `updates.jsonl`、Codex `rollout-*.jsonl`），统计「`total` > 0 且 input/output/cache/reasoning 全 0」的行占比；或用同形 fixture 跑对应 parser 后检查队列行。

**数据库 / API 子集的对照结论**：这一子集里**确实存在「只填 total 而不填 input/output」的家**，不能一概说「没有任何一家」——

- 反例一：`grok`。`normalizeGrokUsage` 先取上报 `totalTokens`，**只有 `totalTokens <= 0` 才回退求和**，而回退后的唯一拒绝条件同样只是 `totalTokens <= 0`（`grok-usage.js:63`–`71`）→ 当上报形状是「有 total、分量字段缺失」时，返回对象就是 `input_tokens: 0` / `output_tokens: 0` 而 `total_tokens > 0`（返回块 `76`–`89`）。这正是「只填 total」的形态。
- 反例二：`qoder`（旧库）。`normalizeQoderTokens` 的判空**只是数值合法性**（`Number.isFinite` 且各分量 ≥ 0，`6154`–`6163`），**不是零值闸门**——`token_info` 全 0 时它会返回一个全 0 对象（`6170`–`6178`）。此前把这一条引到 `6154` 是把它当成了零值判定，实际该行做的是合法性判定。

其余家按「input / output / reasoning / 缓存至少一个 > 0」或「整体判空丢弃」两类列出（`craft`(17199)、`dsh`(23354)、`command-code`(24204) 已逐行确认，其余沿用同一清单）：`opencode`（磁盘支路闸门 `3109`–`3110` 的 `isAllZeroUsage`，`opencode.db` 支路在 `5222`–`5227` 要求四分项至少一个 > 0——注意这里引 `4755` 是 normalizer、不是闸门）、`cursor`(7230)、`devin`(`devin-usage.js:76`/`113`)、`omp`/`omo`(15874)、pi 家族(16455)、`copilot` 三通道(17536/18863/19369)、`antigravity`(21808/21833/21837)、`trae`(`trae-usage.js:60`)、`trae-cn`(22477/22749)。

#### 5.6.2 「全零 token 但仍落桶」：解析器构造点至少 5 处

| 家 | 构造点 | 说明 |
|----|--------|------|
| `qoder`（新 JSONL） | `6639`–`6660` | `billable !== false` 时显式构造 `input/cached/cache_creation/output/reasoning/total = 0`、`billable_total_tokens: 0`、`total_cost_usd: 0`、**`conversation_count: 1`** 的 totals 并 `addTotals` 进桶；注释直说这是「允许可计费的零 token 消息仍然计一次会话」 |
| `qoder`（旧库） | `6151`–`6177`（归一化）、`6281`（会话数）、`6338`–`6340`（落桶） | 归一化的判空只是数值合法性（finite 且 ≥0），**零值不会被拒**；`currentTotals` 带 `conversation_count = requestOwners.get(requestKey) === messageKey ? 1 : 0`，随后**无条件** `addTotals(bucket.totals, currentTotals)` → `token_info` 全 0 的行会写出「六列全 0 + `billable_total_tokens: 0` + `conversation_count: 1`」的队列行 |
| `trae-cn` | `22483`–`22492`（校验）、`22500`–`22508`（写全零 totals）、`22749`–`22751`（落桶） | 字段校验只要求 `Number.isSafeInteger(value) && value >= 0`——**`0` 合法**；落桶处**无任何零值护栏**，只跳过「未变化」的条目 |
| `claude`（用户轮次桶） | `2828`–`2835`（构造）、`3546`（跳过）、`3403`–`3408`（入队） | 用户行（`type === "user"`）只对桶做 `totals.conversation_count += 1` 并 `touchedBuckets.add(...)`，模型是 `DEFAULT_MODEL`（`unknown`）；`pickDominantModel` **跳过 `unknown`**（`3546`），所以「这个 `(source, hour)` 分组只有 unknown 桶」时不会走合并分支，而是由 `enqueueTouchedBuckets` 用 `totalsKey`（含 `conversation_count`，`4165`）比较后照发 → 队列出现 `claude\|unknown\|<hour>`、`total_tokens: 0`、`conversation_count ≥ 1` 的行。**不是边角情形**：任何「用户轮次所在的半小时桶里该分组没有 token 行」都会命中 |
| `reasonix` | `17067`–`17068`（触发）、`17035`（total）、`17044`（会话数） | 触发条件是 `(tokenDelta > 0 \|\| delta.requests > 0)`，而 `total_tokens = delta.input + cacheRead + cacheWrite + output + reasoning` 可以为 0、`conversation_count = delta.requests` 可以 > 0 → 「token 全 0、会话数 >0」的行 |

**必须区分的一类零值行（写入端，不是解析器）**：§4.6 的三种撤回/对齐零行——unknown 桶撤回（`3291`–`3316`）、对齐模型撤回（`3273`–`3290`）、整源撤回（`19990`/`20031`）——同样是「六列全 0」的行，但它们由 `enqueueTouchedBuckets` 在**写入端**构造，目的是用同键零值行覆盖历史行（声明式撤回），不来自 provider 日志。§5.6.2 的 5 处与它们是**两个不同的集合**，下游做「零值行」统计时应分开计数（漏掉这一点会把撤回行误判成上游脏数据，或把解析器零行误判成撤回）。

**反例（易误报）**：`craft` **不会**产生该形态——`17199`–`17212` 在四分量 delta 全 0 时直接 `continue`，走到 `17243`（`conversation_count: prev.total === 0 ? 1 : 0`）时至少一个 token 分量 > 0。

影响：该桶的 `conversation_count` 会增加，但所有 token 列为 0 → 该会话行的成本贡献为 $0。该行是否最终入队还取决于 `enqueueTouchedBuckets` 的 `queuedKey` 比较（`3325`–`3347`），但**桶 totals 确实被改动了**。

`【未能确认】` **trae-cn 真实响应里是否真会出现 `input_token=0` 且 `output_token=0` 的会话行**：源码层「允许」已确认，真实频率未知。**判定条件**：取得一次真实 `query_user_usage_group_by_session` 响应样本（含全部 token 字段为 0 的会话行），或用同形 fixture 跑完整 `parseTraeCnApiIncremental` 并检查队列是否出现全零 `total_tokens` 且 `conversation_count = 1` 的桶行。

#### 5.6.3 缓存字段（`cache_read` / `cache_write`）落空的分布

| 形态 | 家 | 说明与行号 |
|------|----|------------|
| **两个缓存桶都恒 0** | `kiro`(IDE)、`goose`、`anythingllm` | Kiro 连 `cache_creation_input_tokens` 键都缺失（`7545`）；Goose 无缓存列（`14674-14686`/`14708`）；AnythingLLM 的 metrics 只有 prompt/completion/total（`14013`/`14042`） |
| **有 cache_read、无 cache_write（恒 0）** | `gemini`（`4743` 区域，`cache_creation` 恒 0）、`qoder`（旧库，`6173` 写 `cache_creation_input_tokens: 0`）、`copilot`（App DB，注释明确「App DB 没有 cache write 列」，`19357`–`19379`）、`antigravity`（两条路径都没有 cache write 来源，走 `initTotals` 默认 0） | 缓存写入量在这些家里**不可得**，不是零 |
| **表级投影写死** | `unsloth` | `api_usage_events` 的 SELECT 直接写 `0 AS cached_tokens, 0 AS cache_write_tokens, 0 AS reasoning_tokens`（`13634`–`13636`）——即便上游有列也不读 |
| **reasoning 恒 0（无推理桶）** | `kimi`、`kimi`(KimiCode)、`openclaw`、`zed`、`roocode`、`kilo-code`、`kiro`(IDE)、`qoder`（新旧两套）、`cursor`、`devin`、`trae-cn`、`craft`、`command-code`、`claude-science` | 列在 `normalize*` 的返回块里写死 0。`devin` 更极端：**SQL 投影只选四个 metrics 字段**（`src/lib/devin-usage.js:48`–`51`），未来若上游新增 reasoning 列会被静默丢弃 |
| **reasoning 是「差额」而非推理量** | `anythingllm`（`max(0, total − input − output)`，`14042`）、`goose`（`max(0, dTotal − dInput − dOutput)`，`14711`） | 语义不保证，跨家不可比 |
| **上游把 reasoning 并进 output（求和须排除）** | `omo`（`reasoningIncludedInOutput: true`，`16110`；求和在 `15897`–`15915`）、`cline`（`outputTokens` 含 reasoning，注释 `12356`） | 若下游定价表只认 output，会低估或高估 |

**通用判据**：`billable_total_tokens` 只影响费用口径，缓存列落空影响的是**缓存命中率与缓存单价**；对本地推理两家（`lmstudio`/`unsloth`），缓存是真的没有；对 `gemini`/`qoder`/`copilot App`/`antigravity`，缓存**存在但不可得**——下游不能把「0」当作「零缓存」。

#### 5.6.4 `billable_total_tokens` 覆盖情况（8 显式 + 13 兜底）

**显式写入的 8 家**（全部在数据库 / API 子集）：

| 家 | 值 | 行号 |
|----|----|------|
| `qoder`（旧 DB） | `prompt + output`（含缓存） | `6177` |
| `qoder`（新 JSONL） | `inp + cach + cc + out`（零 token 分支写 0） | `6540`、`6657` |
| `devin` | 四分量之和 | `devin-usage.js:197` |
| `cursor` | 四分量之和（注释解释为何不能写 0：会把非计费用量整体隐藏） | `cursor-config.js:536` |
| `grok` | `totalTokens`（含估算路径） | `grok-usage.js:83`、`rollout.js:19959` |
| `antigravity` | `delta.total_tokens`（两条路径都写） | `21806`、`21831` |
| `trae-cn` | `rawInput + output` | `22507` |
| `command-code` | 四分量之和 | `24212` |

**不写、依赖 `addTotals` 兜底的 13 项**（按解析项计）：`opencode`、`omp`、`omo`、`pi`、`prime-agent`、`minimax-code`、`craft`、`reasonix`、`copilot`（OTEL / session-store / App DB 三条通道）、`trae`、`dsh`。合计 **8 + 13 = 21 项**，与 21 家数据源实体完全对齐（`qoder-cn` 复用 `qoder` 的写入路径，不额外计）。兜底规则：`target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0`（`4110`），聚合侧同样兜底（`src/lib/local-api.js:2615`、`2628`）。

**文本 / JSONL 子集的覆盖**：19 家里能确认**显式写入**的只有 `lmstudio` 与 `unsloth`（共用归一化器返回块 `13039`–`13049` 含 `billable_total_tokens: total`）；其余家在各自函数区段内没有 `billable_total_tokens` 赋值语句，一律靠 `4110` 兜底。

**历史坑（必读）**：早期 Cursor 行把 `billable_total_tokens` 写成 0，导致聚合侧「把非计费 Cursor 用量整体隐藏」，因此有一次性的局部修复与重算（`src/lib/local-api.js:179`–`182` 注释）。**结论：这个字段是「可计费口径」的开关，任何「写 0 而不是留空」的实现都会污染聚合结果。**

#### 5.6.5 `conversation_count` 语义不可比（各家口径清单）

同一列名称在 45 家里至少有 **10 种语义**（下表逐行列出，其中两行来自 §6 的两条主线——claude 与 codex——不再单列），dashboard 若直接当「会话数」展示会同时高估与低估：

| 语义 | 家 | 行号 |
|------|----|------|
| 产生差分的消息条数 | `gemini`、`opencode`（磁盘支路） | `2974`、`3120` |
| 每条 assistant 消息 +1（**请求级**） | `kimi` 一族、`codebuddy`、`workbuddy`、`roocode`、`kilo-code`、`opencode`(DB 仅首次)、`omp`/`omo`、pi 家族、`dsh`、`antigravity`、`command-code`、`cursor`（每条 CSV 记录） | `15914`、`17586`、`21807`、`23364`、`24214`、`7232` |
| 仅首次（首见该记录记 1，之后 0） | `cline`（`previous ? 0 : 1`）、`trae`（会话首次）、`trae-cn`（恒 1）、`copilot` session-store（按 session）、`devin`（按 session） | `12405`、`22115`–`22118`、`22508`、`18886`、`14340`–`14346` |
| **用户轮次（只 +1，不写 token）** | `claude`（`claude\|unknown\|<hour>`，见 §5.6.2） | `2833` |
| **每个 `token_count` 事件 +1** | `codex` 家族（含 acode / every-code 同格式） | `2554` |
| 仅首次会话或重置 | `droid` | `15379` |
| 恒 1 | `goose`、`lmstudio`、`unsloth`（共用归一化器写死）、`craft`（会话级之外的历史路径） | `13039`–`13049` |
| DB `message_count` 差分 | `hermes` | `8008`、`7718` |
| **用请求数冒充会话数** | `reasonix`（`conversation_count: delta.requests`） | `17004`、`17044` |
| 会话级（首见记 1，源码明确警告不可比） | `craft`（`17232`–`17235` 注释：与 omp/Claude 的「每条消息 +1」不同，跨 provider 的会话总数**不可直接相加**） | `17232`–`17244` |

**下游处置建议**（依据源码事实，不是猜测）：按 `source` 分组展示、把该列标注为「计数单位随来源而变」，或在 TokenScope 侧**不要**用它做跨工具汇总。

#### 5.6.6 其余需要登记的破坏面

- **精度披露不完整**：`antigravity` 完全不在队列行写 `usage_precision`（只在游标里记 `usageSource`，`21898`）；`kiro` 的字符估算路径也没有精度标记。相对地 `grok` / `trae` 会写 `"estimated"`/`"mixed"`（`20255`、`22144`–`22158`）。`usage_precision` 的全部写入点集中在 `src/lib/rollout.js` 的 16 行 / 15 个独立语句（`3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286-20287/22011/22025/22113/22157`），清单里没有 `antigravity` 与 `kiro`。
- **时间兜底最激进的一家是 `grok`**：桶起点依次为 `event.timestamp` → `lastActive` → `Date.now()`（`20214`），且 `lastActive` 自身也会兜底到 `new Date().toISOString()`（`19684`–`19693`）——**历史用量可能被记进「当前」半小时桶**。同类（用 now）还有 `zed`（`12764` 末级兜底）与 `goose`（`14713`）。
- **`droid` 用文件 mtime 定桶**（`15210`、`15328`）：任何触碰文件的操作都会把该会话的增量搬到触碰时刻；以 mtime 作**末级兜底**的还有 `craft`（`17223`，三级时间链的末级）与 `reasonix`（`17021`）——同属「触碰即搬迁」的破坏面（§5.2.20 已把七家并列）。
- **累计快照 clamp 差值的重放风险**：`craft`（`17189`–`17195`）与 `reasonix`（`17008`–`17014`）都把负差 clamp 到 0 而不做负向回撤；若上游先重置又快速累积回原量级，**这段「重放」会被再次计入**。两家都没有 Qoder 式「先减旧再加新」的回撤路径。
- **`copilot` 三通道「同时活跃时互斥完备」未被证明**：`【未能确认】`。判定条件：给出 `test/copilot-app-parser.test.js` 与 `test/copilot-session-store-parser.test.js` 中交叉通道用例（含「同时活跃」场景）的断言清单；缺该清单则维持「未确认」。
- **成本口径一列三种处理方式**：`command-code` 明确丢弃上游成本写 0 哨兵（`24055`–`24058`）、`qoder` 声明成本永不估算（`6405`–`6411`）、`grok` 反之直接采信上游 ticks（`grok-usage.js:26`–`39`）——下游聚合必须按 `source` 区分，不能把 `total_cost_usd` 当同质列。
- **`config.update` 类旁路字段的可用性**：`kimi-code` 的模型别名、`antigravity` 的模型 slug 归一（`21929`–`21949`）、`trae` 的显示名到 API id 修正（`claude-4-sonnet` → `claude-sonnet-4-20250514`，`trae-usage.js:98`–`105`）都说明**模型名在多数家里是「被上游改写过的展示名」**，不是稳定的计价键——下游必须套等价规则归一后再匹配价格。

#### 5.6.7 给下游（TokenScope 侧）的检查清单（按优先级）

1. **不要把 `conversation_count` 当可加指标**：45 家里至少 10 种语义（§5.6.5），跨来源汇总必然同时高估与低估；需要「会话数」时按 `source` 分别定义，或自行从请求明细派生。
2. **`total_tokens > 0` 不等于「有分桶」**：**至少 3 家**可产 $0 行（§5.6.1 A 类：`gemini` / `grok` / `codex`），另有**至少 5 处**「全零 token 仍落桶」的解析器构造点（§5.6.2：`qoder` 新旧两套 / `trae-cn` / `claude` 用户轮次桶 / `reasonix`），外加写入端的撤回与对齐零行（§4.6、§5.6.2 末）。解析侧应把「非零桶全为 0 而 total > 0」显式判为异常行并计数——TokenTracker 自己只做丢弃/放行，**不做标记**。
3. **缓存列 0 有两种含义（真无缓存 vs 存在但不可得）**：`gemini`、`qoder`(旧)、`copilot`(App DB)、`antigravity` 属后者（§5.6.3）。若用「缓存命中率」做展示，须按来源区分，否则会给出「该模型从不缓存」的错误结论。
4. **`billable_total_tokens` 必须走兜底而不是写 0**：`?? total_tokens` 兜底（`4110`）与 Cursor 的历史修复（`src/lib/local-api.js:179`–`182`）都指向同一结论（§5.6.4）。
5. **模型名是展示名，不是计价键**：`kilo-code` 只报 `provider:<slug>`（`11730`）、`kimi` 一族来自配置文件（`9540`）、`trae` 与 `antigravity` 会做别名改写（`trae-usage.js:98`–`105`、`rollout.js:21929`–`21949`）——必须先做等价归一（小写 + 去 `-` / `.` / `_`）再匹配价格。
6. **时间归属至少有七家会漂到「触碰时刻」**：`zed`、`lmstudio`、`goose`、`droid`、`grok`、`craft`、`reasonix`（§5.2.20、§5.3.6），另有 `copilot`(App DB) 因「会话级累计快照」把一条会话的整段历史落在本次观测到的那个桶里（§5.3.6，机制不同、破坏面同类）。做日粒度看板时，这几家的历史数据可能整段落在错误日期上。
7. **估算行必须可识别**：`grok` 与 `trae` 会标精度，`antigravity` 与 `kiro` **不标**（§5.5.6）。若需要「只统计真实值」的视图，这两家只能按来源整体排除或整体保留。
8. **成本列不可同质处理**：`cline` 自产 `cost`、`grok` 采信上游 ticks、`command-code` 主动丢弃上游成本、`qoder` 明确拒绝估算（§5.6.6）——导入后必须按来源决定「用上游值还是用自己的价格表」。
9. **重复计数防线要照抄的三样**：fork / 副本去重（`opencode` #426 指纹、`devin` `request_id` 账本、`trae` turn 摘要、`command-code` `recordId|timestamp`）、镜像历史过滤（`mimo` 的 providerID 白名单、`zcode` 的三厂商黑名单）、跨通道认领（`copilot` 三通道）（§5.3.3、§5.3.4）。
10. **只读与隐私纪律**：这 45 家的 SQLite 读取统一走 `-readonly` / `readOnly: true`、且**没有显式 `BEGIN`**；`reasonix`、`copilot`(store/app)、`trae`、`devin` 都刻意排除了正文列（`16907`–`16908`、`19270`–`19271`、`trae-db.js:71`–`84`、`devin-usage.js:11`–`13`）。若要复用这些读取器，应保持同一投影纪律，不要为了「顺手多取一列」而把正文带进本地库。

---

**本章未闭合项（仍需外部证据）**：`trae-cn` 的真实全零行频率（§5.6.2，判定条件已写）、`copilot` 三通道互斥完备性（§5.6.6，判定条件已写）、`antigravity` 估算行是否真的对下游不可见（本章依据「`usage_precision` 全部写入点清单里没有 antigravity」这一**否定性证据**成立；若云端 ingest 侧能从 `cursors.files[].usageSource` 反推，该披露缺口不成立——判定条件：取得云端 ingest 侧读取该游标字段的代码或运行证据）。

## 6. Claude Code 与 Codex 深度解剖

本章引用均相对 TokenTracker 仓库根（`xiufengsun/TokenTracker`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`），行号经两轮逐字复核。两条主线共用 `rollout.js` 这一副骨架，但身份、去重、缓存语义与缺陷形态几乎处处对立——这个对立本身就是本章最有信息量的部分。

### 6.1 Claude Code

数据流一条：发现 `~/.claude/projects/**/*.jsonl` → 逐行筛 `usage` → 归一成五桶 → 按 `message.id`（可选叠加 `requestId`）去重 → 用 `uuid`/`forkedFrom` 折叠 fork 会话 → 写进键为 `claude|<model>|<UTC 半小时>` 的桶。

#### 6.1.1 日志发现路径与 slug 编码

```js
// src/commands/sync.js:685
const claudeNativeHome = path.join(home, ".claude");
const wslClaudeHome = process.platform === "win32" && wsl.shouldProbeWsl(process.env)
  ? wsl.discoverWslHome(".claude")
  : null;
const claudeInstallHomes = [];
if (process.platform !== "win32" || wsl.shouldProbeNative(process.env)) {
  claudeInstallHomes.push(claudeNativeHome);
}
if (wslClaudeHome) claudeInstallHomes.push(wslClaudeHome);
```

再叠加 `CLAUDE_CONFIG_DIR` 与 `config.scanRoots.claude`（`:694-699`），取各安装根的 `projects/` 子目录并按 realpath 去重（`:700-703`）。**与 Codex 的立场相反**：Claude **UNION 全部安装**（native + WSL），不做 native-first/WSL-first 单选；同段注释（`:676-684`）给出理由——单选会让 WSL 抢占 native，而随后"磁盘即真值"的一次性修复会把 native 历史**抹掉**。

发现函数 `listClaudeProjectFiles` → `walkClaudeProjects`：递归任意深度、只认 `.jsonl`（`src/lib/rollout.js:5013-5023`）。slug 编码**不被解析**：目录名 `<dash-encoded-cwd>` 既是"不在真实项目 git checkout 内"（靠目录走永远找不到项目），本身也不可逆解码（真实路径里的连字符与路径分隔符编码冲突）——注释逐字见 `src/lib/rollout.js:4600-4606`，落点是常量 `CLAUDE_CWD_SCAN_MAX_BYTES = 65536`（`:4607`）。**结论：slug 只是目录名，不参与任何身份判定**；项目身份一律来自内容里的 `cwd`（6.1.8）。

子代理转录识别靠路径：`const isMainSession = !/[\\/]subagents[\\/]/.test(filePath);`（`:2803`）同时接受正反斜杠，理由是 WSL 经 `\\wsl$` 访问时 `path.join` 会产出反斜杠（`:2800-2802` 注释，issue #307）。跨根同文件两层去重：同一次运行内 `seenClaudeFiles` Set 按路径去重（`src/commands/sync.js:1137-1145`），跨环境同内容副本另由 `claudeFileIdentityHash` 处理（`src/lib/rollout.js:1021-1107`）。`claude-mem` 观察者目录有专属身份：常量 `CLAUDE_MEM_OBSERVER_PATH_SEGMENT = "--claude-mem-observer-sessions"`（`:58`），命中即赋予固定 `projectRef = "https://local.tokentracker/claude-mem/observer-sessions"`（`:59-60`、`:4297-4301`）——它以 URL 形式混进"远程仓库地址"那套项目标识；另有一次性迁移 `reincludeClaudeMemObserverFiles`（`src/commands/sync.js:1148`、`:7100`）。

一处常见误判需纠正：`parseClaudeScienceIncremental` **不是** Claude Code 的第二个解析器。它读 SQLite 的 `frames` 表（`src/lib/rollout.js:6841-6871`），库在 `~/.claude-science/operon-cli.db` 或 `operon.db`（`:6875`、`:6882`），多组织走 `orgs/<slug>/`（`:6892-6906`），Windows 无原生版须探 WSL（`:6910-6929`）；映射独立——`claudeScienceUncachedInput = input - cacheRead - cacheWrite`（`:6999`），`reasoning_output_tokens` 恒 0（`:7023`），合并用幂等减法（`:7097-7117`），`conversation_count` 只记根帧（`:7079-7085`），`source` 是字面量 `"claude-science"`（`:7101`）。

#### 6.1.2 逐行解析与五桶映射

```js
// src/lib/rollout.js:2841
if (!line.includes('"usage"')) continue;
let obj;
try {
  obj = JSON.parse(line);
} catch (_e) {
  continue;
}

if (isOmOClaudeSdkMirror(obj)) continue;

const usage = obj?.message?.usage || obj?.usage;
if (!usage || typeof usage !== "object") continue;
```

三点：字符串预筛省掉无 `usage` 行的解析；`usage` 双位置回退（`message.usage` 优先，顶层 `usage` 兜底，`:2851`）；**OmO 镜像行被剥离**（`:2849`，判定 `entrypoint === "sdk-ts" && userType === "external"`，`:4910-4912`）——那是 OmO Native 经 Claude Agent SDK 的镜像拷贝，剥离以避免与 `source=omo` 双计（`:4906-4909`，issue #726）。

五桶归一（`src/lib/rollout.js:4921`，节选）：

```js
const reasoningTokens = Math.min(outputTokens, toNonNegativeInt(
  u?.output_tokens_details?.thinking_tokens ?? u?.output_tokens_details?.reasoning_tokens,
));
const totalTokens = inputTokens + outputTokens + cacheCreation + cacheRead;
return {
  input_tokens: inputTokens,
  cached_input_tokens: cacheRead,
  cache_creation_input_tokens: cacheCreation,
  // Claude rows price reasoning separately; it is already included in usage.output_tokens.
  output_tokens: outputTokens - reasoningTokens,
  reasoning_output_tokens: reasoningTokens,
  total_tokens: totalTokens,
};
```

| 输出桶 | 来源字段 | 变换 |
|---|---|---|
| `input_tokens` | `usage.input_tokens` | 直取（Claude 原生语义已是"不含缓存"） |
| `cached_input_tokens` | `usage.cache_read_input_tokens` | 直取（缓存读） |
| `cache_creation_input_tokens` | `usage.cache_creation_input_tokens` | 直取（缓存写） |
| `output_tokens` | `usage.output_tokens` | **减去 reasoning** |
| `reasoning_output_tokens` | `output_tokens_details.thinking_tokens` 或 `.reasoning_tokens` | 取先存在者，`Math.min(outputTokens, …)` 封顶防倒挂 |
| `total_tokens` | — | `input + output(原始含推理) + cacheCreation + cacheRead` |

关键不对称：**`total_tokens` 用未减 reasoning 的 output 计算**，而 output 列已拆走推理——两列互补，保证 `input + cached + cacheCreate + output + reasoning == total`（`test/claude-reasoning-usage.test.js:44` 显式断言）。`toNonNegativeInt`（`:4964`）把非有限值与负数吸收为 0、其余 `Math.floor`，所以小数/负 token 都落成确定值。缓存字段只认两个名字：Claude 路径读 `cache_read_input_tokens` / `cache_creation_input_tokens`（`:4927-4928`），**不兼容** `cache_write_input_tokens` 别名；别名兜底只存在于 Codex 路径（`src/lib/codex-token-usage.js:25-27`）。

【未能确认】跨 provider（Kimi/DeepSeek/Mimo/MiniMax 等 anthropic 兼容端点）若用 `cache_write_input_tokens` 写缓存写量，Claude 路径会静默丢为 0。判定条件：统计 Claude 会话行里该键的出现率——同行为 `cache_write_input_tokens` 非 0 且缺 `cache_creation_input_tokens` 才成立；两者同时出现或同时缺失则实际影响为零。现无真实样本，只记为条件性风险。

零用量与写入判定（`:2861-2865`）：`normalizeClaudeUsage` 后 `isAllZeroUsage(delta)` 为真则整行跳过（`:4945` 定义，六桶全 0 才算），且 **assistant 行的 `conversation_count` 恒置 0**。会话计数只来自 user 文本行（`:2810-2831`）：仅主会话、`type === "user"`、`message.content` 是字符串或含 `{type:"text"}` 块之一 → `conversation_count += 1`，并记进 `unknown` 模型桶（`:2831` 的 `const userModel = DEFAULT_MODEL;`），与 token 桶分离；只有 `tool_result` 块的工具结果行被明确排除（`:2807-2809` 注释）。

#### 6.1.3 去重身份：`message.id` / `requestId`

```js
// src/lib/rollout.js:4895 区块（注释 + claudeMessageDedupKey）
// Stable dedup key for one Claude jsonl entry. Anthropic's official protocol
// guarantees `message.id` is globally unique per response, so msgId alone is a
// valid dedup key. Older code required both msgId AND requestId, which short-
// circuited dedup entirely for jsonl entries where `requestId` is absent ...
function claudeMessageDedupKey(obj) {
  const msgId = typeof obj?.message?.id === "string" && obj.message.id ? obj.message.id : null;
  if (!msgId) return null;
  const reqId = typeof obj?.requestId === "string" && obj.requestId ? obj.requestId : null;
  return reqId ? `${msgId}:${reqId}` : msgId;
}
```

规则与后果：无 `message.id` → 返回 `null` → **完全不去重**，该行必计；有 `requestId` 时键为 `msgId:reqId`，否则为裸 `msgId`，两种格式共用一个 Set 而不冲突（`msgId` 串不含 `:`）；`uuid` / `parentUuid` **不参与 token 去重**，只服务会话身份与 fork 折叠（6.1.4）。

注释里那段历史是本设计最硬的实证：旧代码要求 `msgId` **且** `reqId`，对缺 `requestId` 的行直接短路，导致受影响 provider **1.6–3.7 倍超算**（issue #64）。缺 `requestId` 的来源有两类——DeepSeek/Kimi/Mimo/MiniMax 的 anthropic 兼容端点不返回 `request-id` HTTP 头；Claude Code 的子代理与 thinking 传输路径也会丢该字段。持久化与上限（`src/lib/rollout.js:1233-1237`）：哈希集写回 `cursors.claudeHashes`，**上限 100k 且保留最后 100k**；超出时最早的哈希被挤出，理论上是批量重扫旧文件的重计窗口——注释（`:1021-1025`）承认该风险并因此加了文件级 `fileId` 去重层，而那层只在"同一次运行内同时见到 `\\wsl$` 与本地路径"时启用（`sawUncFile && sawLocalFile`，`:1035`）。

#### 6.1.4 fork 会话折叠：`uuid → forkedFrom.messageUuid`

```js
// src/lib/claude-user-identity.js:5
function noteClaudeFork(aliases, obj) {
  const uuid = obj?.uuid;
  const parent = obj?.forkedFrom?.messageUuid;
  if (typeof uuid === "string" && uuid && typeof parent === "string" && parent && uuid !== parent) {
    aliases[uuid] = parent;
  }
}
```

`claudeUserIdentity`（定义在 `src/lib/claude-user-identity.js:13`，`rollout.js` 经 require 使用；`rollout.js:921-933` 是 `formatLocalDate` 与文件身份哈希注释，与本函数无关）沿别名链上溯到根，用 `u:<root>` 作身份；环（畸形别名）也能确定性折叠，且**与文件顺序无关**。约束四条：别名来自**顶层 `uuid` + `forkedFrom.messageUuid`**，且**只采信 `type === "user"` 的行**（`src/lib/claude-user-identity.js:44`）；子代理路径排除（`:31`）；身份前缀 `u:` 与消息键区分，历史哈希折叠只对 `startsWith("u:")` 生效（`src/lib/rollout.js:997-1003`）；别名表持久化在 `cursors.claudeForkAliases`（`:1234`）。

**预扫描 + 净值修正**（`src/lib/rollout.js:987-1015`）：`collectClaudeForkAliases` 在计数**之前**遍历完整批文件（"fork 文件可能排在前面"，`:987-988` 注释），随后把历史计数中"同一族多于 1 条"的部分从 `conversation_count` 里**减掉**，并有下界保护（`:1011` 的 `if (!bucket || bucket.totals.conversation_count < count - 1) continue;`）。关键约束：**只修 `conversation_count`，不动 token**（`:992-994` 注释：`Preserve all token totals and history whose source files were removed.`）。

`test/claude-fork-conversations.test.js` 把语义钉死：

| 断言位置 | 含义 |
|---|---|
| `:37-39` | 三层链 `original ← copy ← nested` 只算 1 次会话、token 仍 30；结果与文件顺序无关 |
| `:40-43` | 游标序列化/反序列化后再跑，计数仍是 1/30（幂等） |
| `:44-45` | 与独立实现 `computeClaudeGroundTruthBuckets` 的会话数对账一致 |
| `:48-59` | 同一 fork 文件后追加的新 user 轮次独立计数（1 → 2），全量重跑不增长 |
| `:61-78` | 复现旧版污染态（计数 4 + 三个 `u:` 哈希混入），重扫后修复为 2、`total_tokens` 仍 30 |
| `:80-86` | 删掉原始文件、只剩 fork 副本时仍只算 1 次 |
| `:88-95` | 父文件**后来**才出现时，已计过的嵌套 fork 被回溯修正 |

#### 6.1.5 sidechain 与子代理：全部由路径表达

`sidechain` 字段在 token 统计路径上**零命中**——全库 `grep sidechain` 只命中 `src/lib/session-analytics.js:2660`，那是会话浏览器的注释。"子代理"概念完全由**文件路径**表达：会话计数排除 `.../subagents/...`（`src/lib/rollout.js:2803`、`:2810`）；fork 别名索引排除同路径（`src/lib/claude-user-identity.js:31`）；**token 计数不排除**——子代理转录里的 assistant usage 行照常计入，只是归属到该子代理文件所属的 cwd 项目。即：子代理的 token 是算的，子代理的"会话"是不算的；两个决定落在两个文件里，改一处不牵动另一处。

#### 6.1.6 模型名来源与哨兵值

```js
// src/lib/rollout.js:2857
const model = normalizeModelInput(obj?.message?.model || obj?.model) || DEFAULT_MODEL;
```

先 `message.model`，后顶层 `model`；`normalizeModelInput`（`:4260`）**只做 trim**，不改大小写、不做别名归一；无值 → `DEFAULT_MODEL = "unknown"`（`:56`）。因此 opus/sonnet/haiku 各变体、带日期后缀的版本串、非 Anthropic 路由串（如 `grok-4.5-build`）在这里**全部原样落桶**——归一化不在解析层，而在定价/分组链路，属另一条独立版本轴。

哨兵值 `<synthetic>` 在**解析层没有任何特判**，全库 3 处出现都在会话浏览器：`src/lib/session-analytics.js:101` 的集合、`:621-624` 注释（`They are not a billable model and can appear after the real assistant messages in the same session.`）、`:1968-1971` 注释（Claude Memory 观察者转录）。集合逐字：

```js
// src/lib/session-analytics.js:101
const PLACEHOLDER_MODELS = new Set(["<synthetic>", "synthetic", "<unknown>", "unknown"]);
```

后果：若某行 `message.model === "<synthetic>"` 且 usage 非零，它会照常生成 `claude|<synthetic>|<hour>` 桶（`:"<synthetic>"` 是真值字符串）；usage 全零则被 `isAllZeroUsage` 丢弃（`src/lib/rollout.js:2862`）。

【未能确认】真实 Claude Code 是否会为 `<synthetic>` 行写非零 usage——代码层无法判定，测试 fixture 也不含该形态。判定条件：在本机 `~/.claude/projects/**/*.jsonl` 中找 `"model":"<synthetic>"` 的行，看同行 usage 是否非零。若存在非零 usage，面板会出现名为 `<synthetic>` 的模型行——纯展示层后果，总量不变。

#### 6.1.7 时间戳、时区与半小时桶

| 环节 | 代码 | 说明 |
|---|---|---|
| 时间戳取值 | `src/lib/rollout.js:2858-2859` | 非字符串一律丢弃整行 |
| 桶生成 | `src/lib/rollout.js:2867` | `toUtcHalfHourStart(tokenTimestamp)`，非法 → `null` → `continue` |
| 桶键 | `src/lib/rollout.js:2872` | `bucketKey(source, model, bucketStart)` → `claude\|<model>\|<ISO>` |
| user 会话计数 | `src/lib/rollout.js:2828-2835` | 同一时间戳路径，模型固定 `unknown` |

桶算法（`src/lib/rollout.js:4173`）：全 UTC 对齐，分钟数 `>= 30` 取 `:30` 否则 `:00`，秒与毫秒清零，输出 `toISOString()`，非法时间戳返回 `null`。该算法在仓库内**存在两份实现**：`src/lib/rollout.js:4173`（主实现，被所有增量 parser 使用）与 `src/lib/claude-categorizer.js:1175`（真值修复专用）。两份**语义等价、文本不同**，且**没有任何测试锁定二者等价**——`test/` 下引用 `toUtcHalfHourStart` 的两处（`test/sync-droid-dup-session-repair.test.js:29`、`test/trae-entitlement.test.js:130-137`）都 `require("../src/lib/rollout")`，`claude-categorizer.js` 那份无人测；改动任一处不会让另一处失败，一旦分叉，增量解析与真值修复会写出两套时间分桶。

时区**事实**：解析层**不读取任何时区字段**，完全信任 `timestamp` 字符串自带偏移；仓库内**没有**"Claude 日志时区"配置项参与 `hour_start`；唯一与此沾边的显式代码是 WSL 路径映射 `wsl.mapWslCwdToUnc`（`:1161`），那管 cwd 不管时间。跨日**只发生在读侧**——写入侧桶只有 UTC 半时划分，`hour_start` 的日期部分永远是 UTC 日期，`rowDayKey`（`src/lib/local-api.js:213`）才把 UTC 桶映射到展示时区的自然日。**"桶归属"（写侧）与"展示日"（读侧）是两个正交阶段**，任何"跨日漂移"都必须在这两层之间解释——这是最容易误判的一点。

#### 6.1.8 项目上下文与 cwd 的角色

```js
// src/lib/rollout.js:4609
async function resolveClaudeFileCwd(filePath) {
  const stream = fssync.createReadStream(filePath, {
    encoding: "utf8", start: 0, end: CLAUDE_CWD_SCAN_MAX_BYTES,
  });
  const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });
  try {
    for await (const line of rl) {
      if (!line || !line.includes('"cwd"')) continue;
      ...
      if (typeof obj?.cwd === "string" && obj.cwd.trim()) return obj.cwd.trim();
```

只扫**前 64KB**（`:4607`），命中即返回、**取第一条**；结果缓存在游标 `claudeCwd`（`:1200`），"确认不存在"用 `null` 显式表示，与 `undefined`（未探过）语义不同（`:1132` 的 `projectKnownAbsent`）。路径 → 项目 key：`resolveProjectContextForPath`（`:4549`）内部走 `resolveProjectMetaForPath`（`:4293`），自下而上找 `.git`；worktree/submodule 支持（`.git` 是文件时读 `gitdir:` 再读 `commondir`，`:4634-4668`）；远端 URL 优先 `origin`、否则取第一个（`:4690-4692`）；`canonicalizeProjectRef`（`:4695`）与 `deriveProjectKeyFromRef`（`:4429`）产出稳定 key；缓存按目录回填（`:4303-4312`）；WSL 侧 cwd 先过 `mapWslCwdToUnc`（`:1161`）再解析。

Claude 的**特殊之处**：`projectRef`/`projectKey` 是**文件级**的——一次解出后写进文件游标（`:1200-1203`），会话中途换目录不改变归属；Codex 相反，是**行级推进**（`turn_context.cwd` 变更即切，`:2496-2511`）。归属粒度不同，聚合时必须知道。

一处假设需纠正：`src/lib/claude-config.js`（279 行）与统计**零关系**，全部是 Claude Code `settings.json` 的 hook 增删改查——`CLAUDE_USAGE_EVENTS = ["Stop", "SessionEnd"]`（`:7-8`）、`upsertClaudeHook`（`:10`）、写盘前 `fs.copyFile` 备份成 `${settingsPath}.bak.<ISO>`（`:252-256`）；唯一与"解析"沾边的注释是泛化说明：CodeBuddy CLI 是 Claude Code fork、共用同一 hook schema（`:107-109`）。项目归属完全在 `rollout.js` 与根发现侧。

#### 6.1.9 已知缺陷 A：增量游标没有行边界提交

`const endOffset = st.size;` 在 `rollout.js` 中共 **3 处**（`:2291`、`:2682`、`:2793`），**只有第 3 处落到 token 游标**。三处分工必须分清，否则会把局部问题夸大成全局问题：

| 站点 | 所属 | 形态 | 是否落到 token 游标 |
|---|---|---|---|
| `:2291` | `parseRolloutFile`（`:2264`，Codex 单文件主解析） | 等于 `st.size`，仅用于 `startOffset >= endOffset` 的"无新字节"早退（`:2294-2302`）；数据没读就返回，返回值不可能大于入参 | **否**（正式返回走 `committedEndOffset`，`:2652`） |
| `:2682` | `scanRolloutProjectFileContexts`（`:2666`，Codex 项目上下文扫描） | `return { endOffset: committedEndOffset, … }`（`:2764-2765`） | **否**（只推进 `projectOffset`） |
| `:2793` | `parseClaudeFile`（`:2776`，Claude 单文件主解析） | `return { endOffset, eventsAggregated }`（`:2889`）→ `cursors.files[key].offset = result.endOffset`（`:1194`） | **是——唯一一处** |

**非对称性**（缺陷本体）：Codex 的 token 路径提交 `committedEndOffset`（`:2652`），只推进到最后一个完整行；Codex **连项目上下文扫描位都做了同样的提交保护**（`:2765`）；OpenClaw 另有显式保护 `openclawOffsetEndsAtLineBoundary()`（`:1949`），OMP 族有 `maxRecordBytes`；**只有 Claude 这条路径完全没有**。

后果链：同步瞬间文件正在被写、尾行未闭合时，`readline` 会把未闭合尾行也 `yield` 出来（`crlfDelay: Infinity`），`JSON.parse` 失败被 `continue` 吞掉（`:2843-2847`），而 `offset` 仍推进到 `st.size`。**确切范围**（必须收窄，不能写成"永久丢失"）：该行的字节**在本代文件内不可恢复**；该文件下一次 inode 变化或截断时会走三态判定重置路径 `startOffset = 0` 全量重扫（`:488-504`），届时该行会被补上。

复核动作：构造"写入半行 JSONL → 跑 `parseClaudeIncremental` → 补全该行 → 再跑"的定向测试，看补全后的那行是否被计入；若未计入，即为确定性数据丢失 bug，不是理论风险。

#### 6.1.10 已知不一致 B：一次性真值修复与增量解析的口径分叉

`src/lib/claude-categorizer.js` 有两个截然不同的职责，只有一个影响统计。**职责一**是 Context Breakdown 分类视图（读侧，不写队列）：文件头注释逐字 `Computes on demand — no queue schema changes, no parser changes, no sync changes.`（`:2-13`），七个分类键（`:33-41`）为 `system_prefix, conversation_history, user_input, tool_calls, subagents, reasoning, assistant_response`，分配靠字符长度比例（`splitOutputByContent`，`:367`）与"首个 cache_creation 视为 system_prefix"（`:378-380`），由 `local-api.js:69` 引入给面板。**职责二**是 `computeClaudeGroundTruthBuckets`——覆盖统计的一次性真值修复（`:1193`）。

触发与破坏面：只在**全量扫描**分支调用（`src/commands/sync.js:1183-1190`），且**只跑一次**（迁移键 `"claudeGroundTruthRepair_2026_05_v4"`，`src/commands/sync.js:248`）；**原子重写 `queue.jsonl`**——保留非 claude 行、删除所有 `source === "claude"` 与 `"claude-mem"` 行、追加真值行，`tmp + rename`（`:6871-6921`）；**清空** `cursors.hourly.buckets` 与 `groupQueued` 中所有 `claude|` / `claude-mem|` 前缀键（`:6930-6943`）；整体替换 `cursors.claudeHashes`（注释 `:6858-6861`：`clears all claude buckets and replaces claudeHashes wholesale`）；防护是"若某配置根当前不可读/不存在而历史上供过文件，则放弃本次修复"（迁移键不写、留待后续，避免把历史抹成空，`:1149-1182`，判定函数 `rootPreviouslySuppliedFiles`，`:1163-1166`）。

口径差异（"不一致"的全部内容）：

| 维度 | `parseClaudeFile`（增量，日常路径） | `computeClaudeGroundTruthBuckets`（一次性修复） |
|---|---|---|
| usage 取值 | `obj?.message?.usage \|\| obj?.usage`（`src/lib/rollout.js:2851`） | **只有** `obj?.message?.usage`（`src/lib/claude-categorizer.js:1267`） |
| reasoning 来源 | `usage.output_tokens_details.thinking_tokens ?? .reasoning_tokens`（`:4924-4926`） | `usage.reasoning_output_tokens`（`:69`）——**另一个字段名** |
| output 与 reasoning | `output_tokens = 原始 - reasoning`（拆分） | `output_tokens = 原始`（不拆），reasoning 另记 |
| `total_tokens` | `in + out(含推理) + cacheC + cacheR` | `in + cacheR + cacheC + out`（`:70-71`）——数值一致 |
| 身份折叠 | 用 `forkAliases` 折叠 user 身份（`:996-1013`） | 同样折叠（`:1214`、`:1246`），但**不看历史游标**，从零重建 |
| 未知模型行 | `DEFAULT_MODEL`（`"unknown"`） | 字面量 `"unknown"`（`:1277`），同值 |

结论：**总量口径相容**（分母相同：input + 原样 output + 两个缓存桶），但**列分布不同**——修复后 `reasoning_output_tokens` 通常变成 0、`output_tokens` 相应变大（等于把推理并回 output）。由于 `computeRowCost` 只依赖五列之和（`test/claude-reasoning-usage.test.js:45-47` 用 `< 1e-12` 断言列分布可换、费用不变），**费用不变**；两个副作用则是确定的：**"推理 token"这一展示列会在修复后归零**，且 `obj.usage` 兜底路径的数据在真值修复中**会消失**。修复不可逆（原子重写 + 哈希全替换），受影响安装无法回到修复前的列分布。复核动作：同一份合成 fixture 跑两条路径逐列比对（预期 total 相同、`output_tokens` 与 `reasoning_output_tokens` 不同）。

---

### 6.2 Codex

Codex 的世界模型与 Claude 相反：**没有"响应级 usage 行"，只有会话级累计快照**，所有增量靠减法恢复；一份 rollout 里还可能同时存在**多条累计谱系**。

#### 6.2.1 日志发现路径与扫描调度

```js
// src/commands/sync.js:645
const codexNativeValue = resolveEnvRoot("codex", { env: process.env, home }) || path.join(home, ".codex");
```

```js
// src/commands/sync.js:782
const codexPaths = resolveInstallPaths({
  nativeValue: codexNativeValue,
  wslDir: ".codex",
  requireAnyChild: ["sessions", "archived_sessions"],
  union: true,
});
```

随后按"根 × (sessions | archived_sessions)"推入 sources（`:788-810`）：`sessions` 总是扫，`archived_sessions` 带 `deep: true`（`:885` 的 `listRolloutFilesDeep`，因为归档目录是扁平的，`src/lib/rollout.js:195-200` 注释，issue #187），且**后台轻量同步默认不扫归档**（除非首轮历史迁移，`:790`、`:807`）。两个要点：`union: true` 表示 native + WSL 都要（与 Claude 的 UNION 同构，但走 `resolveInstallPaths`）；`requireAnyChild: ["sessions", "archived_sessions"]` 防"空壳 `~/.codex` 抢占"（`:762-772` 注释，issue `codex-wsl-shadow`）。

命名与日期：文件名 `rollout-<ISO 时间戳>-<uuid>.jsonl`；日期取文件名前缀（`src/lib/rollout.js:4192` 的 `rolloutDateFromPath`，正则 `^rollout-(\d{4}-\d{2}-\d{2})T`）；会话 id 从文件名尾部 uuid 抓（`codexSessionIdFromPath`，`:2256-2262`）；目录是严格 `YYYY/MM/DD`，年/月/日各自用 `^[0-9]{4}$` / `^[0-9]{2}$` 校验（`:107-131`）。

**冷文件跳过**（`filterColdCodexRolloutFiles`，`src/lib/rollout.js:708`–`879`，与 §4.2 引用同一函数）是 Codex 独有的调度优化，五步：名字日期落在活跃窗口内（默认今天起 2 天 + 全库最大日期，`activeCodexRolloutDates`，`:893-921`）→ 必扫；`auditDue`（每天一次冷审计）→ 必扫；近 30 天内的冷文件 `fs.stat` 一次、**比游标 offset 大就扫**（`:808-822`）；否则问游标 store `canSkipCodexDay`（`:824-827`），可跳则 `skipped += 1`；**cursor store 重启时整批回退**（`if (cursorStoreRestarted) { … return { rolloutFiles: files, skipped: 0, restarted: true }; }`，`:859-869`）。回归点写在文件注释里：`#592`（会话跨天仍在增长）与 `#187`（归档搬迁）（`:63-69`、`:806-807`）。

#### 6.2.2 事件类型清单与 `token_count` 真相事件

```js
// src/lib/rollout.js:4838
function extractTokenCount(obj) {
  const payload = obj?.payload;
  if (!payload) return null;
  if (payload.type === "token_count") {
    return { info: payload.info, timestamp: obj?.timestamp || null };
  }
  const msg = payload.msg;
  if (msg && msg.type === "token_count") {
    return { info: msg.info, timestamp: obj?.timestamp || null };
  }
  return null;
}
```

**两种信封**：`payload.type === "token_count"`（主流）与 `payload.msg.type === "token_count"`（旧/嵌套形态）。按解析路径实证的事件清单：

| 事件 | 匹配方式 | 作用 | 行号 |
|---|---|---|---|
| `event_msg/token_count` | `line.includes('"token_count"')` | **唯一用量来源** | `src/lib/rollout.js:2427`、`:4838` |
| `turn_context` | 含 `"model"`/`"cwd"`/`"current_date"`/`"forked_from_id"` 之一 | 推进模型、cwd、当前日期 | `:2431-2437`、`:2485-2513` |
| `session_meta` | 同上 | fork 标记（`forked_from_id`）、弱模型回退 | `:2490-2492` |
| `event_msg/thread_settings_applied` | `line.includes('"thread_settings_applied"')` | service tier（`default`/`priority`/`fast`） | `:2438-2440`、`:2479-2483` |
| `token_usage_record` | `line.includes('"token_usage_record"')` | **只用于 compaction 计费** | `:2441-2446`、`:2515-2520` |
| `compacted` | 紧随 record 的下一条完整行 | compaction 判定 | `:2410-2425` |
| `task_complete` / `turn_aborted` | `event_msg.payload.type` | 结束 record 的等待态 | `src/lib/codex-usage-record.js:82-86` |
| `model/rerouted` | 8 种信封形态枚举 | **前瞻**（真实语料 0 命中） | `src/lib/codex-model-attribution.js:33-66` |
| `response_item/function_call`、`event_msg/exec_command_end` | 仅在 breakdown 采集时 | 工具归因（不影响 token） | `src/lib/codex-rollout-parser.js:1049-1060` |

性能设计是"先字符串包含、后 `JSON.parse`"，并有一个兜底分支保证未匹配行只要合法 JSON 也推进提交位（`:2447-2464`）：

```js
// src/lib/rollout.js:2454
if (invalidRecordPolicy === "throw" || !record.terminated) {
  try {
    JSON.parse(line);
    committedEndOffset = scannedEndOffset;
  } catch (error) {
    if (invalidRecordPolicy === "throw") throw error;
    break;
  }
}
```

注意 `break`：**遇到未闭合的不合法行会停止本轮读取**，保留可提交前缀——这正是 Claude 侧缺失的那层保护。落桶字段路径（`:2530-2554`）：

```js
// src/lib/rollout.js:2530
const info = token.info;
if (!info || typeof info !== "object") continue;
...
const lastUsage = info.last_token_usage;
const totalUsage = info.total_token_usage;
if (totalUsage && typeof totalUsage === "object") latestTotal = totalUsage;

const rawDelta = consumeUsageDelta(usageDeltaState, lastUsage, totalUsage);
```

即真相事件的两个字段：**`payload.info.last_token_usage`（本轮增量）+ `payload.info.total_token_usage`（会话累计）**。规范形状由测试 fixture 固化（`test/codex-union-cursor-divergence.test.js:79-85`），其中 `usageOf`（`:76-78`）给出原生字段名：`input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens, total_tokens`（外加 `cache_creation_input_tokens` 及其别名 `cache_write_input_tokens`）。

#### 6.2.3 差值引擎：`last_token_usage` vs `total_token_usage`

```js
// src/lib/codex-token-usage.js:3
// A rollout can contain TokenCount events from more than one Codex SessionState
// (for example, a parent agent and an embedded reviewer). Each SessionState has
// its own cumulative total, but the event does not carry a stable stream id.
// Recover the stream from Codex's invariant: total - last = that stream's prior
// total. Keeping a small LRU of stream heads also makes repeated rate-limit
// snapshots idempotent when the streams are interleaved.
```

状态与上限（`:10-18`）：六桶字段表 + `const MAX_USAGE_BASELINES = 32;`——**LRU 流头谱系最多 32 条**。判定流程（`:98-158`）四分支：**重复快照**时 `total` 命中已知流头 → 返回 `null`（**不重复计**）并置 `sawInterleaved`；**正常增长**时 `last` 存在且 `total - last` 命中某个流头 → 返回 `last`（把 `last_token_usage` 当本轮增量）；**`total - last` 不命中任何流头但仍可减** → 置 `sawDivergentCumulative` 后返回 `last`（偏向"少算"而非虚高）；**无 `last` 或流断裂** → 用 `diffUsage(total, activeHead)` 取差，若差值比 `last` 还大则判为新流（`:150-153` 注释：`A cumulative jump larger than last usage cannot come from the normal append_last_usage path`）。`subtractUsage` 的硬约束（`:49-59`）：任一字段 `total < last` 即返回 `null`——对溢出/回绕的**显式拒绝**，不是饱和。

测试固化（`test/codex-token-usage.test.js`）：交错两谱系时增量序列为 `[100, 200, null, 50, 30]`（第三个重复快照不计）且 `sawInterleaved === true`、基线数 2（`:24-38`）；单谱系 `[100, 50, 25]`、两个告警位 false、基线恒 1（`:40-48`）；`last` 与累计不一致时取**较小的累计差**（`:50-57`）；基线数被截断到 32（构造 37 个流头，`:59-67`）。

`total` 与 `last` 的选择顺序：`total_token_usage` 非空对象时刷新 `latestTotal`（`src/lib/rollout.js:2547`）——它是**文件级权威累计**；增量取 `consumeUsageDelta(state, lastUsage, totalUsage)`；若事件推进了累计谱系但被判重复，返回 `null` 且**游标/基线仍已推进**（注释 `:2570-2571`：`usageDeltaState is already advanced above`），保证后续行血缘正确。测试语义（`test/rollout-parser.test.js:104-158`）：4 条 `token_count`、第 2 与第 4 条是同一累计值的重复推送，断言 `eventsAggregated === 2`、`bucketsQueued === 1`、`model === "unknown"`、队列合计等于 `usage1 + usage2` 的 `total_tokens` 之和；注释逐字：`The cumulative total_token_usage value is authoritative for a file; if it did not move, the repeated last_token_usage must not be counted again.`（`:105-107`）

#### 6.2.4 compaction 计费：`token_usage_record`

```js
// src/lib/codex-usage-record.js:10
// Compaction. Codex bills the compaction model call in a record that is
// immediately followed by a top-level `compacted` line, and the next
// token_count repeats the previous cumulative total (its last_token_usage is
// the post-compaction context size, not a response). The cumulative delta is
// zero, so without the record the call is lost.
```

规则：计数条件 = `compacted` 紧邻 + 下一个 `token_count` 的 `total_token_usage` 与上一次**完全相等**（`sameUsage(previousTotal, totalUsage)`，`:150`）；去重键是 `payload.response_id`，**跨 fork 重放与文件复制稳定**（`:19-21`），上限 20000（`:60`）；无 `response_id` 的 record **永不计**（`:20-21`、`src/lib/rollout.js:2356`）；未被任何 `token_count` 匹配的文件被标 `recordOnly` 并留在 `cursors.codexUsageRecordOnlyFiles` 供 sync 告警（`src/lib/rollout.js:649-655`；`codex-usage-record.js:174-177`）；`payload.turn_token_usage` / `thread_token_usage` **从不读取**（`:31-32`）。这是一处易漏语义：compaction 那次模型调用的 token 在累计曲线上**不可见**（累计没动），只能靠"记录 + 下一条 total 相等"这条侧信道捞回；漏掉即少算、重复捞即多算，所以去重键必须跨文件复制稳定。

#### 6.2.5 归一化：`input -= cached` 与零值重置哨兵

```js
// src/lib/rollout.js:4864
// Codex rollouts (and Every Code, which shares the format) report
// `input_tokens` as the TOTAL prompt, with `cached_input_tokens` as the
// cached subset — i.e. the cached slice is INSIDE the input count. Our
// queue schema (CLAUDE.md → Token Normalization Convention) stores
// `input_tokens` as pure non-cached input and `cached_input_tokens`
// separately. Without this subtraction the cost formula bills the cached
// bytes twice: once at the full input rate and again at the cache_read
// rate, producing ~6–7x cost inflation on cache-heavy Codex sessions
// (verified against ccusage's per-day numbers on the same rollouts).
...
out.input_tokens = Math.max(0, out.input_tokens - out.cached_input_tokens);
```

这段注释是本章最硬的一处定量证据：不做减法会在缓存重的会话上产生 **约 6–7 倍的费用膨胀**（同一批 rollout 与 ccusage 的按日数字对账）。`Math.max(0, …)` 保下界，`normalizeUsage`（`:4851-4887`）对六桶只做 `Math.floor` 与非负吸收。

| 队列列 | 来源 | 变换 |
|---|---|---|
| `input_tokens` | `input_tokens` | **减 `cached_input_tokens`** |
| `cached_input_tokens` | `cached_input_tokens` | 直取（缓存读） |
| `cache_creation_input_tokens` | `cache_creation_input_tokens` **或** `cache_write_input_tokens` | 直取（写法兼容见 `src/lib/codex-token-usage.js:25-27`） |
| `output_tokens` | `output_tokens` | 直取，**不减 reasoning** |
| `reasoning_output_tokens` | `reasoning_output_tokens` | 直取、不拆列（子集标记，见 6.2.6） |
| `total_tokens` | `total_tokens` | 直取（保留上游口径，不重算） |

**零值重置哨兵**（`:4881-4893`）：Codex 偶发写入"五个分量全 0、仅 `total_tokens` 非 0"的伪事件，在归一化**之前**即被拒绝，避免污染 `input - cached`：

```js
// src/lib/rollout.js:4881（节选）
const componentTotal = (usage) => [
  usage?.input_tokens,
  usage?.cached_input_tokens,
  usage?.cache_creation_input_tokens ?? usage?.cache_write_input_tokens,
  usage?.output_tokens,
  usage?.reasoning_output_tokens,
].reduce((sum, value) => sum + Math.max(0, Number(value) || 0), 0);
return Number(lastUsage?.total_tokens || 0) > 0
  && Number(totalUsage?.total_tokens || 0) === 0
  && componentTotal(lastUsage) === 0
  && componentTotal(totalUsage) === 0;
```

用法在 `src/lib/rollout.js:2550-2553`：**仅在 `source === "codex"` 时启用**。**会话计数的语义分叉**同样在此落定：Codex 侧每个通过零值闸门并被接受的 delta 都写 `delta.conversation_count = 1`（`:2554`），Claude 侧 assistant 行恒 0、只有主会话 user 文本行 +1（`:2865`、`:2807-2831`）——两条主线在桶里用同一列名、求和方式相同，但**含义不可互换**：Codex 的累计值实际是"被接受的 token 事件数"，Claude 的才是"用户轮次数"。

#### 6.2.6 reasoning 是 output 的子集标记（与 Claude 相反）

解析侧对 `reasoning_output_tokens` **没有任何算术改写**（`src/lib/rollout.js:4851-4879`，没有 `output -= reasoning` 这类行）；注释 `:4873-4876` 逐字写着保留 `total` 是为了兼容 `older Codex / Every Code shapes where output_tokens can exclude reasoning`——即上游默认 **output 含 reasoning**，本工具只是原样保留。计价侧对此显式置 0（`src/lib/pricing/index.js:248-256`）：

```js
// src/lib/pricing/index.js:248
const reasoningIncludedInOutput =
  row.source === "codex" ||
  row.source === "acode" ||
  row.source === "every-code" ||
  row.source === "omo" ||
  row.source === "cline";
const reasoningCost = reasoningIncludedInOutput
  ? 0
  : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
```

变量名本身就是判据："reasoning **已含于** output"。`computeRowCost` 中 reasoning 相关项共 3 处（`:254` 基础费、`:297-299` 长上下文溢价、`:325-327` priority 溢价），**三处全部用同一个门控**——没有漏掉、也没有反向叠加，因此不存在双计路径。

Claude 侧写法**相反**（`src/lib/rollout.js:4930-4939`，逐字节选）：

```js
// Claude rows price reasoning separately; it is already included in usage.output_tokens.
output_tokens: outputTokens - reasoningTokens,
reasoning_output_tokens: reasoningTokens,
```

Claude 原生 output **同样**包含推理，但工具选择**显式拆出**，拆出后 reasoning 列**照 output 单价计费**（白名单不含 `claude`），`total_tokens` 仍用未减的 output 计算。两侧写法相反，恰好互证"不拆" = 已含在内：Codex 是"不拆列 + 计价置 0"（子集标记），Claude 是"拆列 + 照 output 价"（独立加成项）。**统一口径**：喂给 `computeRowCost` 的都是**五列之和**，`total_tokens` **永不参与计费**（`CLAUDE.md:60-64`；`test/claude-reasoning-usage.test.js:45-47` 用 `< 1e-12` 断言列分布可换、费用不变）。

一处真实风险：Codex 侧"不双计"依赖 `source` 白名单**硬编码**（`codex`/`acode`/`every-code`/`omo`/`cline`），一旦上游新增 `*-code` 变体而白名单漏加，就会**静默双计**每一条 reasoning token，且现有测试全绿。判定与加固：补一条对等断言——`computeRowCost({source:"codex", output_tokens:518, reasoning_output_tokens:163})` 与同参数但 reasoning 为 0 的结果差 `< 1e-12`；并把白名单改为结构性判定（如 `source.endsWith("-code") || source === "codex"`）。

#### 6.2.7 模型唯一权威：`turn_context.model`

```js
// src/lib/codex-model-attribution.js:3
// Codex token_count events do not carry a model id. Every usage event is
// therefore attributed to the model named by the most recent turn_context,
// which Codex emits at the start of each turn and which tracks mid-session
// model switches exactly. That per-turn path is what actually runs today.
// ...
// Deliberately NOT used for attribution: event_msg/thread_settings_applied.
// It carries thread_settings.model, but it fires when the user picks a model
// in the UI - seconds to minutes before the switch takes effect - so usage
// still streaming from the in-flight turn belongs to the previous model.
// turn_context is the only signal that marks where a turn actually begins.
```

规则：`turn_context.payload.model` → `selectedModel`，并清 `rerouted`（`:82-92`）；`session_meta.payload.model` **只在 `selectedModel` 为空时**采纳，且**永不覆盖** `turn_context`（`:96-102`，注释指出 fork 会重放父级 `session_meta`）；`thread_settings_applied` **被刻意排除在模型归属之外**（`:11-15`）——UI 选择领先生效数秒到分钟，正在流出的用量仍属上一模型。实测量级（注释给出）：`0 of 10310 session_meta rows across 5849 local rollouts carry the field (codex-cli 0.151.0)`（`:18-20`）。调用点在 `src/lib/rollout.js:2476-2477`，模型状态**持久化进文件游标**（`modelAttributionState`，`:639`），跨增量续用，初值由 `lastModel` 注入（`:2320-2322`）。

【未能确认】`model/rerouted` 分支从未在真实语料验证（`src/lib/codex-model-attribution.js:47-51` 自述命中 0）；一旦被真实触发，其 8 种信封形态与字段名（`fromModel/toModel/from_model/to_model`）都属推断。判定条件：检索本机 rollout 中是否出现 `model/rerouted` / `model_rerouted`。

#### 6.2.8 provider / base_url 不进桶键

队列行**没有 provider 列**（行格式为 `source, model, hour_start, 六桶, total, billable, cost, conversation_count`）；`source` 常量对 Codex 家族固定为 `"codex"` / `"acode"` / `"every-code"`，由 `sync.js` 组装文件条目时决定（`src/commands/sync.js:894`、`:789`）。`session_meta.payload.model_provider`（如 `"openai"`）在**用量路径**上完全未被读取——`src/lib/codex-model-attribution.js:19-23` 明确称其为 `provenance rather than a model`；使用 `model_provider` 的是**会话浏览器 / breakdown** 侧（`src/lib/codex-rollout-parser.js:1023`、`src/lib/session-analytics.js:822-834`），且带防护：

```js
// src/lib/session-analytics.js:822
// Older Codex rollouts can omit turn_context.model. The shared parser then
// falls back to model_provider (for example "openai"), which is provenance
// rather than a model and must not become a model-table row.
```

全库对 `azure` 的检索在 `src/lib/` 下**零命中**；`base_url` 命中都属网络/遥测配置（`src/lib/doctor.js:62`、`src/lib/runtime-config.js:78`），与 Codex 日志解析无关。**结论**：第三方 base_url / Azure 路由的 Codex 会话，用量**按 `turn_context` 声明的模型串原样落进 `codex|<model串>|<hour>`**，没有任何 provider 维度参与身份或去重，同模型的跨渠道请求合并到同一桶——这与"能区分不同渠道"的直觉预期相反，且当前 schema **没有位置可放**该维度。

#### 6.2.9 service tier：子集列，不参与求和

```js
// src/lib/codex-service-tier.js:20,26
const CODEX_SERVICE_TIER_MARKER = '"thread_settings_applied"';
const PRIORITY_SERVICE_TIERS = new Set(["priority", "fast"]);
```

归属规则：`a token_count row belongs to the most recent preceding thread_settings_applied`（`:12-13`）；无前置记录即 `null`，`null` **按 Standard 计**，从不猜（`:12-15`）。落桶是**子集标记**而非独立桶（`src/lib/rollout.js:4073-4088` 的 `PRIORITY_SUBSET_FIELDS`：`priority_input_tokens` / `priority_cached_input_tokens` / `priority_cache_creation_input_tokens` / `priority_output_tokens` / `priority_reasoning_output_tokens`）；注释（`:4067-4072`）强调这些列**永不参与任何求和**，`total_tokens` 仍等于五桶之和，为零时整列省略。

【未能确认】覆盖率数字（1213 个本地文件中 167 个带该事件、覆盖 25% 的 `token_count`；`default` 814 / `priority` 58）出自上游源码注释里的**维护者本机语料观测**，非本次实测，不能外推为任意用户环境。判定条件：在本机 `~/.codex/sessions/**` 重跑同样计数（分母 = 含 `token_count` 的文件数，分子 = 含 `thread_settings_applied` 的文件数）。代码侧规则与结构（归属、`null` 按 Standard、子集列不参与求和）本次逐行核对通过。

#### 6.2.10 家族关系：Every Code / OmO / acode / cline

Codex rollout 不是 Codex 独占的格式：归一化注释直接点名 `Codex rollouts (and Every Code, which shares the format)`（`src/lib/rollout.js:4864-4865`），`:4873-4876` 又提到兼容 `older Codex / Every Code shapes where output_tokens can exclude reasoning`；计价白名单把 `codex`、`acode`、`every-code`、`omo`、`cline` **并列**为"reasoning 已含于 output"（`src/lib/pricing/index.js:248-256`）；OmO 在 Claude 侧还有镜像防护——`entrypoint === "sdk-ts" && userType === "external"` 的行被解析层剥离，避免与 `source=omo` 双计（`src/lib/rollout.js:4906-4912`，issue #726）。意义：**语义只有一份实现**，差值、模型归属、档位、compaction 四个核心状态机被整个家族复用，家族成员只在 `source` 常量上分流——这也解释了白名单漏加成员为何会静默错账（6.2.6）。

需区分的是：`src/lib/codex-rollout-parser.js`（52451 字节）与 `src/lib/codex-context-breakdown.js`（27929 字节）默认**不影响统计**——两者对 `queuePath` / `appendFile` / `enqueueTouchedBuckets` 的检索零命中（"不写队列"已闭合），消费者是只读视图（`src/lib/local-api.js:71`、`src/lib/session-analytics.js:32`），`codex-rollout-parser.js:1-4` 自述 `Does NOT hold any aggregation state`。

【未能确认】"不影响统计"这一更强结论依赖前提：`src/commands/sync.js:198` 借用其 `extractTokenCount` 的那条路径是**一次性迁移**（6.2.11），而该迁移是否在某些安装上**永不触发**取决于运行时游标状态（迁移键是否已置位、文件是否被判受影响），源码层判定不了。判定条件：读一个真实 `~/.tokenscope/cursors.json`，看该迁移键是否已置位；置位则本条影响面为零。

#### 6.2.11 血缘修复三类：rescan inflation / fork replay / interleaved usage

Codex 侧所有"少算/多算"防线都在修补同一件事：**累计谱系的连续性**。三类触发、修法、对账方式如下。

| 类别 | 触发 | 修法 | 对账方式 |
|---|---|---|---|
| **rescan inflation**（重扫膨胀） | inode 变化导致从 0 重扫，所有 delta 会重新加进**持久化桶**；外部工具（Codex-Manager 切换渠道）用原子重写改 inode，没有防线就每次切换翻倍（`src/lib/rollout.js:2608-2618` 注释） | 事件层去重键 `sessionUUID:eventTimestamp`（`:2626`），配两级追踪器：`getAppendOnlyEvents()`（本次运行新写，稳态快路径）与 `getHistoricalEvents()`（持久化集合或分片 store，重扫/换路径/union 用）（`:302-380`）；选择器 `needsHistoricalRolloutDedup()` 的判定顺序（`:428-461`）至关重要——**同会话换游标路径的判定必须在稳态短路之前** | e2e：`test/codex-union-cursor-divergence.test.js` 两个用例真跑 `cmdSync([], {})`，读真实 `queue.jsonl` 后按 `source\|model\|hour_start` **取最后一条**求和（`:104-112`）；断言 120 → 220 → **仍是 220**（滞后根不得重放，`:118-153`、`:155-189`） |
| **fork replay**（fork 重放） | `isForkedRollout` 且回放旧轮次：跨日场景 `currentDate < rolloutDate`；同日场景为毫秒级爆发推送 | 两重机制（`:2556-2603`）：跨日日期守卫 `isForkedReplayToken({isForkedRollout, rolloutDate, currentDate})`（`:4212`）；同日子毫秒爆发检测 `FORK_REPLAY_GAP_MS = 500`（`:4210`），只跳过**起始连续密集段**，首次出现 >500ms 间隔即**永久关闭**（`:2588-2590`） | 阈值依据（`:4203-4209` 注释）：重放 flush 行距 sub-ms~几 ms、真实轮次 ≥~4.6s、重放→真实断点 ≥11s；500ms 偏向"宁可少算也不多算"。首行无法前视，故仍有 **<1% 的残余 over-count**（`:2567-2569`）——被显式承认的残留，不是待修 bug |
| **interleaved usage**（交错谱系） | 一份 rollout 含多个 `SessionState` 的累计总数（父 agent + 内嵌 reviewer），事件不带流 id | LRU 流头（上限 32）+ 不变量 `total - last = 该流此前的 total`（`src/lib/codex-token-usage.js:3-8`、`:10-18`），四分支判定见 6.2.3 | 单测四组断言（`test/codex-token-usage.test.js:24-38`、`:40-48`、`:50-57`、`:59-67`）；另有**一次性修复**用同一状态机做受影响判定，见下 |

interleaved 那条的修复路径值得单列：`src/commands/sync.js:198` 从视图模块借用 `extractTokenCount`，用于 codex 一次性修复的**受影响判定**（`:6233-6252`）：

```js
// src/commands/sync.js:6246
const token = extractTokenCount(obj);
const info = token?.info;
if (!info || typeof info !== "object") continue;
consumeUsageDelta(state, info.last_token_usage, info.total_token_usage);
if (state.sawInterleaved || state.sawDivergentCumulative) {
  affected = true;
}
```

该修复会**重写历史桶**——原子重建到临时 queue，成功才替换、失败完全不动（`src/commands/sync.js:5750-5804`）；进入重建前还有一道闸门：若 project 队列检出畸形行则放弃本次修复以免丢数据（`countMalformedCodexProjectQueueRows` 定义在 `:6436`，**全库唯一调用点** `:5824-5830`）。所以它是"影响统计的"——但它是**迁移**，不是日常解析路径。

union 安装的意义也要看清：同一会话文件在两条路径各有游标时，"滞后根"的增量会被判成新数据。e2e 头注释把这处历史缺陷写得很直白（`test/codex-union-cursor-divergence.test.js:13`）：`needsHistoricalCodexDedup` 曾在 `startOffset > 0` 上短路，**先于**它自己的"同会话在另一路径下"判定，于是用了只知本次运行的 append-only 追踪器；当已同步的根无新内容时它一个键都不贡献，滞后根的重放就落进持久化桶与队列。修法是调整判定顺序，并把持久化事件集由 `codexEventStore` 注入（`src/commands/sync.js:1049-1051`）。

---

### 6.3 两家的对照表

| 维度 | Claude Code | Codex |
|---|---|---|
| **读取路径** | `~/.claude/projects/**/*.jsonl`，递归任意深度；UNION native + WSL + `CLAUDE_CONFIG_DIR` + `scanRoots.claude`（`src/commands/sync.js:685`、`:700-703`；`src/lib/rollout.js:5013-5023`） | `~/.codex/{sessions,archived_sessions}/YYYY/MM/DD/rollout-*.jsonl`，union native + WSL，归档目录扁平需 `deep` 扫描（`src/commands/sync.js:645`、`:782`、`:789-810`、`:885`） |
| **去重键** | `message.id`；有 `requestId` 时为 `message.id:requestId`；无 `message.id` 则完全不去重（`src/lib/rollout.js:4895` 区块） | 三层：累计未变即丢弃（`src/lib/codex-token-usage.js:109-119`）；`sessionUUID:eventTimestamp`（`src/lib/rollout.js:2626`）；compaction 用 `response_id`（`src/lib/codex-usage-record.js:60-80`） |
| **缓存字段** | 读 `cache_read_input_tokens`、写 `cache_creation_input_tokens`，**只认这两个名**（`src/lib/rollout.js:4927-4928`） | `cached_input_tokens` + `cache_creation_input_tokens`，兼容 `cache_write_input_tokens` 别名（`src/lib/codex-token-usage.js:25-27`） |
| **reasoning 处理** | **独立加成项**：解析显式拆出（`output -= reasoning`），计价照 output 单价，`total` 用未减 output（`src/lib/rollout.js:4930-4939`） | **子集标记**：解析不拆列，计价置 0（`src/lib/pricing/index.js:248-256`）；两侧都以五列之和进 `computeRowCost`，`total_tokens` 不参与计费 |
| **`conversation_count` 语义** | assistant 行恒 0；主会话 user 文本行 +1 且记入 `unknown` 模型桶；fork 折叠后再做净值修正（`src/lib/rollout.js:2865`、`:2810-2831`、`:987-1015`） | 每个被接受的 delta 恒写 1（`src/lib/rollout.js:2554`）——实际含义是"被接受的 token 事件数"，与 Claude 的"用户轮次数"不可互换 |
| **已知缺陷** | A：增量游标无行边界提交（`endOffset = st.size`，`:2793` 是唯一落 token 游标的站点；确切范围是本代文件内不可恢复）；B：一次性真值修复与增量解析口径分叉（usage 取值 / reasoning 字段名 / 是否拆分，费用不变但展示列改变且不可回退） | ① reasoning 不双计依赖 `source` 白名单硬编码，漏加即静默双计且测试全绿；② 模型串无 provider 维度，跨渠道同模型合并；③ `model/rerouted` 分支未在真实语料验证；④ 语义修复的触发依赖运行时迁移状态，无法从源码判定 |

最后一条共同底座：队列消费端按 `source|model|hour_start` **取最后一条（不是求和）**（`test/codex-union-cursor-divergence.test.js:104-112`；对照 `src/lib/local-api.js:207` 的 `readQueueData`），因此任何"同键数值回退"或"重复追加"都会直接改变最终读数——这正是两家在去重、折叠、血缘三条线上都写得异常小心的原因。

## 7. 聚合、去重与时间语义

TokenTracker 的本地用量数据**不是事件流，而是一份可重放的「累计快照日志」**。这一选择决定了本章全部结论的形态：写入端只追加、读端只取末行、修复只能靠原子重写或撤回行、时间语义全部落在半小时桶这一粒度上。同一份文件还被两套口径同时读取（展示用末行、守卫用最大值），理解这一点是读懂 repair 体系的前提。

**证据口径**：本章所有 `路径:行号` 均为样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）按 UTF-8 解码、以 `\n` 分行后的**文件实际行号**（与编辑器、`read` 工具一致）。PowerShell 5.1 的 `Get-Content` 在中文环境按 GBK 解码会吞掉多字节行尾、导致行号整体偏移（**单文件最多 15 行**，全 `src/` 累计受影响 52 行），不可用于核对。标 `【未能确认】` 者为未能直证、仅由结构或行为反推的条目，均已附判定条件。

### 7.1 队列作为「累计快照日志」（写入端幂等 + 撤回行 + unknown 对齐）

**三个文件**：`~/.tokentracker/tracker/queue.jsonl`（主用量队列，生成于 `src/commands/sync.js:569`，是**唯一**上传云的队列，`:3656`）、`project.queue.jsonl`（项目维度队列，`:571`，**不上传**云端）、`queue.state.json`（上传偏移与状态，`:3950`、`:4668`）。读取侧自行解析路径、不依赖 sync：`resolveQueuePath()` 硬编码返回主队列（`src/lib/local-api.js:83-86`），项目队列由主队列路径派生出同目录文件（`:135`）。

**写入端幂等**。入口 `enqueueTouchedBuckets`（`src/lib/rollout.js:3169`）只处理本次被修改的桶键集合 `touchedBuckets`（`:3173`），并有两道判定：

```js
// src/lib/rollout.js:3403-3408
const key = usagePrecision ? `${totalsKey(unknownBucket.totals)}|${usagePrecision}` : totalsKey(unknownBucket.totals);
const outputKey = outputModel === DEFAULT_MODEL ? key : `${key}|${outputModel}`;
if (unknownBucket.queuedKey === outputKey) continue;
```

判定一：**累计值指纹未变则不写**——`totalsKey`（`src/lib/rollout.js:4155-4171`）把九个数值列拼成指纹字符串，与上次入队的 `queuedKey` 相同即跳过。判定二：**写的是该桶的当前累计快照，不是增量**——行内含全部九个用量列加 `conversation_count`（`:3409-3427`），最后一次性 `appendFile`（`:3482-3484`）。

维度归属要说准：**优先子集** `PRIORITY_SUBSET_FIELDS` 拼在 `totalsKey` 内部（`:4167-4170`），而 **`usage_precision` 与输出模型**的拼接发生在调用点而非 `totalsKey` 主体（`:3404-3407`）。两道判定合起来的效果是：**同一次同步不会为同一桶写两行，但不同次同步会给同一桶追加多行「该桶在若干时刻的绝对累计值」**——同一键的出现次数等于该桶被触碰的轮次数，语义是「快照序列」而非「事件流」，这正是读端必须 last-row-wins 的唯一原因。项目队列走同型模式，但键**不含 model**、含 `project_key`/`project_ref`，且两个字段必须同时存在才写（`:3505-3513`、行结构 `:3515-3529`）；无法解析出仓库标识的用量**不进项目队列**。

**撤回行：逻辑删除 = 写一行零值快照**。旧值**既不参与相加，也不被物理删除**，而是靠「同键零值行压过旧行」实现逻辑删除。三个触发点：模型别名行失效（桶改名、或 unknown 桶被对齐到具体模型时为旧模型补零行，`src/lib/rollout.js:3272-3288` 的 `alignedModel` 分支）；整源/整桶撤回（`retractStaleSourceQueueRows`，`:19990`、核心循环 `:20016-20032`）；迁移期清理（`migrateCursorUnknownBuckets`，`src/commands/sync.js:5173`、`:5207`）。

```js
// src/lib/rollout.js:20016-20032（节选）
for (const [key, entry] of latestRows.entries()) {
  if (keepKeys.has(key)) continue;
  if (totalsKey(entry.row) === totalsKey(zero)) continue;
  lines.push(JSON.stringify({ source, model: entry.model, hour_start: entry.hour_start, ...zero }));
}
if (lines.length === 0) return 0;
await fs.appendFile(queuePath, `${lines.join("\n")}\n`, "utf8");
```

**唯一会做物理删除的场合**只有两处：未上传尾部的压缩（本节末）与 repair 的原子重写（7.5）。

**桶键构造与归一化**。分隔符固定为 `|`；常量 `DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`、`BUCKET_SEPARATOR = "|"`（`src/lib/rollout.js:55-57`）。

| 函数 | 形态 | 行号 |
| --- | --- | --- |
| `bucketKey(source, model, hourStart)` | `source\|model\|hourStart` | `src/lib/rollout.js:4222` |
| `projectBucketKey(projectKey, source, hourStart)` | `projectKey\|source\|hourStart`（**无 model**） | `:4228` |
| `groupBucketKey(source, hourStart)` | `source\|hourStart`（整源整点聚合） | `:4233` |
| `parseBucketKey(key)` | 按 `source\|model\|hourStart` 反解，缺段兜底 | `:4238` |

归一化不对称：`source` 小写化加 trim、空值兜底 `codex`；`model` **只 trim、保留大小写**、空值兜底 `unknown`（`src/lib/rollout.js:4254-4264`）。**结构性隐患（已验证）**：`projectBucketKey` 的布局是 `projectKey|source|hourStart`，而 `parseBucketKey` 按 `source|model|hourStart` 解析，两者**不通用**——若把项目键喂进 `parseBucketKey`，`projectKey` 会被当成 source。当前**没有**误用：`parseBucketKey` 在主队列内共 **15 处调用**（`:1973`、`:3174`、`:3199`、`:3223`、`:3238`、`:3434`、`:3570`、`:3636`、`:3655`、`:7212`、`:12286`、`:13378`、`:14251`、`:16376`、`:20776`）+ 定义 `:4238`，逐处打开确认容器分别是 `hourlyState.buckets`（11 处）、`attemptHourly.buckets`（`:1973`）、`touchedBuckets` 集合（`:3174`）、游标态 `state.buckets`（`:3636`、`:3655`）——**全部落在主队列键空间**；项目键另有专门解析器 `projectUsageKeyFromFields`（`src/commands/sync.js:6422`）。

**未上传尾部压缩**（唯一按字节重排队列的操作）。`compactPendingQueue`（`src/lib/upload-queue.js:62`）的注释同时给出设计意图与前置条件：**调用者必须已持 `sync.lock`，只重写待上传快照，已上传历史与 `queue.state.json` 在原子替换前后逐字节不变**。门槛：待上传部分 < 1 MB（`BACKLOG_BYTES`，`:5`）不动，`offset` 必须落在行边界（前一字节为 `\n`）否则放弃（`:70-78`）。去重键：用量行 `["hourly", source, model, hour_start]`、会话状态行 `["session", source, session_id]`，带 `kind` 的未知控制记录一律不压缩（`:45-60`）；同键保留最后一条，会话状态按**严格更新**替换、等值保留首条（`:79-92`）。原子替换：`wx` 新建 → 拷已上传前缀 → 拷胜出行 → `sync()` → **关句柄** → 复查 inode/size/mtime → `rename`（`:94-131`；`:112-123` 为关句柄与复查，注释说明 Windows 上必须先关输入句柄才能替换目标）。调用点在 `src/commands/sync.js:3543`；测试 `test/upload-queue.test.js:26`（保留已上传字节且 LWW 的向下修正/零值快照不被压掉）、`:53`、`:77`、`:95`、`:103`、`:113`、`:291`。

### 7.2 读端合并与缓存（last-row-wins、签名缓存、锚点续读、坏行容错、读时纠错）

**合并规则：Map 覆盖即「后行胜出」**。两个消费者：本地 API 的 `readQueueData` / `readProjectQueueData`（`src/lib/local-api.js:202`、`:145`），以及 CLI wrapped 聚合器（`src/lib/wrapped-aggregator.js:70`）。主队列键是 `source|model|hour_start`，项目队列键把 `model` 换成 `project_key`：

```js
// src/lib/local-api.js:202-211（主队列，节选）
const readQueueData = createAsyncQueueReader({
  label: "readQueueData",
  parseRow(row) {
    if (row?.kind === "account_session_state" || row?.kind === "account_sync_watermark") return null;
    return { key: `${row.source || ""}|${row.model || ""}|${row.hour_start || ""}`, row: normalizeQueueRow(row) };
  },
});
```

`parseRow` 返回 `null` 即「这一行不是用量行」——账户会话状态与水印等控制记录在此静默丢弃，不进入任何聚合。真正的合并只有一句 `Map.set`：`for (const [key, row] of changes) seen.set(key, row);`，最终 `rows` 由已提交映射加尾块构成（`src/lib/async-queue-reader.js:86-91`）。`Map.set` 对已存在键保留插入顺序、替换值，因此每个键只留最后一次出现的行——等价于「按行序 later-wins」，与写入端「后写的是更完整的累计值」互为前提。注意末条边界：**没有末尾换行的残行**只进 `tail` 参与本次返回，键不进已提交映射（补全后键可能变化）。坏行不进任何集合：`JSON.parse` 抛错即 `malformed++` 并跳过（`:49-57`）。CLI 侧独立复刻同一规则，注释直接点明与 dashboard 对齐（`src/lib/wrapped-aggregator.js:71-85`，原文含 `Naively summing every row double-counts every previously synced bucket`）。

**两套口径并存：展示取末行，守卫取最大值**。

| 口径 | 消费者 | 规则 | 证据 |
| --- | --- | --- | --- |
| last-row-wins | 展示链（本地 API、wrapped） | `Map.set` 覆盖式合并 | `src/lib/async-queue-reader.js:55`、`:86-91` |
| MAX | repair 的**数据丢失守卫** | `setMaxTotal` 同键取最大；`mergeMaxTotals` 跨「现有队列 + 游标态」与「重建结果」两组来源取最大 | `src/commands/sync.js:6585-6589`、`:6574-6583` |

为什么不能统一：撤回（零值行）会把末行压成 0，若守卫也用 last-wins，历史峰值会被「撤回动作」抹掉，从而误判「无丢失」并放行原子重写。两套读同一份文件、**从不互相写回**，因此不产生数据矛盾；唯一副作用是撤回历史会让守卫更容易跳过修复——那是**欠修**，不是错修。

**缓存、续读与容错**。

| 机制 | 行为 | 证据 |
| --- | --- | --- |
| 文件签名 | `dev:ino:size:mtimeNs:ctimeNs`，且在**已打开的 fd 上 stat**（避免 stat/open 之间被原子替换） | `src/lib/async-queue-reader.js:8-10`、`:30-35` |
| 增量续读 | inode 不变、文件变大、且首部与「上次消费边界前 256 字节锚点」都比对一致时，从上次 `consumedBytes` 继续 | `:40-47` |
| 单飞 / 容量 | 同一路径并发请求共享同一个 Promise；LRU `maxEntries` 默认 4 | `:109-114`、`:16`、`:94` |
| 事件循环 | 64 KiB 分块读，每块后 `yieldToRequests()` | `:5`、`:79` |
| 错误处理 | `ENOENT` 清缓存返回空数组；其他读错回退到最后一次成功快照 | `:98-103` |

测试：`test/async-queue-reader.test.js:16`（冷启动并发读共享解析且不阻塞无关 HTTP 请求）、`:48`（**替换、截断、同尺寸重写、删除**都必须丢弃陈旧桶）、`:66`（半行尾部、UTF-8 分块边界、畸形行都不能破坏已完成行）。

**读时纠错：`normalizeQueueRow` 不重写文件**（`src/lib/local-api.js:170`），两类**读时修补**：① 历史 codex 行的「input 含 cache」口径——用精确不变量识别（`total == input + output` 且 `cached > 0` 且 `input >= cached`），读取时把 `input_tokens` 减去 cached 部分（`:155-178`）；② cursor / antigravity 的 `billable = 0`——置为等于 `total_tokens`，注释给出两处成因（GitHub issue #106 的 kind 门控、一次 O(N) 重构误删，`:179-198`）：

```js
// src/lib/local-api.js:191-198（节选）
if (sourceName === "cursor" || sourceName === "antigravity") {
  const totalTokens = Number(normalized.total_tokens || 0);
  const billable = Number(normalized.billable_total_tokens || 0);
  if (totalTokens > 0 && billable < totalTokens) normalized = { ...normalized, billable_total_tokens: totalTokens };
}
```

**风险（已验证）**：这是读时修补而非数据迁移。规则一旦变化，历史数字会**追溯变化**，且与云端已上传值可能不一致。

### 7.3 半小时桶与时间语义（UTC 取整、当地日键、两级时区回退的真实行为）

**生成函数：一律 UTC**（`src/lib/rollout.js:4173-4190`）：

```js
function toUtcHalfHourStart(ts) {
  const dt = new Date(ts);
  if (!Number.isFinite(dt.getTime())) return null;          // :4175 无效时间戳 → null
  const minutes = dt.getUTCMinutes();
  const halfMinute = minutes >= 30 ? 30 : 0;                // :4177 边界取 >=
  return new Date(Date.UTC(dt.getUTCFullYear(), dt.getUTCMonth(), dt.getUTCDate(), dt.getUTCHours(), halfMinute, 0, 0)).toISOString();
}
```

| 问法 | 答案 | 证据 |
| --- | --- | --- |
| 分钟 <30 → `:00`，≥30 → `:30` | 是；`08:30:00.000` 归 `:30`，`08:29:59.999` 归 `:00` | `src/lib/rollout.js:4177` |
| 是否 UTC | 是，全用 `getUTC*` 与 `Date.UTC`，返回带 `Z` 的 `toISOString()` | `:4176-4189` |
| 是否参与本机时区 | 否，写入侧完全不含本机时区信息 | 同上 |

**覆盖范围与三份独立实现**。`toUtcHalfHourStart(` 的分布（逐文件正则重算，区分「出现次数」与「所在行数」）：

| 位置 | 出现次数 | 行数 | 构成 |
| --- | --- | --- | --- |
| `src/lib/rollout.js` | **70** | 66 | 定义 1（`:4173`）+ 调用 **69 次 / 65 行**（`:20214` 一行 3 次；`:20265`、`:20418` 各 2 次） |
| `src/commands/sync.js` | 1 | 1 | `:6663`（droid 修复调用） |
| `src/lib/claude-categorizer.js` | 3 | 3 | **独立副本**定义 `:1175` + 调用 `:1250`、`:1279` |
| 全 `src` 合计 | **74** | **70** | = 72 次调用 + 2 处定义 |
| `test/`（参考） | 10 | 10 | `test/sync-droid-dup-session-repair.test.js` 8 处、`test/trae-entitlement.test.js` 2 处 |

**三份实现语义等价（已验证）**：`rollout.js:4173-4190`（权威）；`claude-categorizer.js:1175-1191`（取整逻辑逐行相同，仅中间变量写法不同）；`src/commands/sync.js:4538-4549`（grok 修复路径，数学等价——毫秒整除半小时，且兼容**秒级时间戳**：小于 `1e10` 的数字按秒处理再乘 1000，`:4542`）。并非所有来源都用半小时桶：copilot 另有 `copilotOtelAggregateKey(model, bucketStart)`（`src/lib/rollout.js:17462`），其桶粒度 `【未能确认】`。

**从桶到日：两族函数、两级不同的回退基准**。时区上下文来自**查询参数**（不是服务端配置）：`tz`（IANA 名）优先，`tz_offset_minutes`（整数分钟）兜底（`src/lib/local-api.js:512-519`）。关键是缺省时的回退基准**分两条路径，必须分别记**。

第一族 `rowDayKey`（日键与日键区间过滤）**缺省取 UTC 日**：只在 `timeZoneContext.timeZone` 或有限 `offsetMinutes` 存在时才走 `getZonedParts`，否则直接 `return hs.slice(0, 10)`，即 `hour_start` 的 **UTC 日期**（`src/lib/local-api.js:213-225`，`:224` 为该行）。由它产出的日键（`buildDailyAggregates` 逐行调用，`:260`、`:266`）以及全部**日键区间过滤**（`day >= from && day <= to`）都以这条为准。

第二族 `getZonedParts` 系（小时键 / 今天 / 徽章）是**三级回退，末级是运行环境本机时区**：① 请求 `tz` 的 IANA formatter（`:525-571`）→ ② `tz_offset_minutes` 固定偏移（`:573-583`）→ ③ **本机时区**（`:585-592`，用 `dt.getFullYear()/getMonth()/getDate()/getHours()` 这些本地 getter）。凡**没有** `timeZoneContext && (timeZone || Number.isFinite(offsetMinutes))` 守卫、直接调它的位置都落第 ③ 级：

| 调用点 | 用途 | 行号 |
| --- | --- | --- |
| `aggregateHourlyByDay` | 日过滤 + 小时键 | `src/lib/local-api.js:606`、`:608` |
| `usage-summary` | `todayStr`（rolling 窗口终点） | `:2382` |
| `usage-heatmap` | `todayStr`（窗口终点） | `:2538` |
| `night_owl` 徽章 | 请求小时判定 | `:482` |

**可推出的后果（源码谓词推出，未起服务实测）**：非 UTC 机器上不带 `tz`/`tz_offset_minutes` 调 `-usage-hourly` 时，响应 `day` 的默认值是 **UTC 今天**（`:3147` 的 `new Date().toISOString().slice(0, 10)`），而过滤谓词用的是**本机时区日键**（`:608`）——两者基准不同，返回的是「本机时区日 = `day`」的桶，跨日时段可能整日为空。同类基准错位还出现在 summary 的 `rolling.last_7d`/`last_30d`（窗口终点取本机时区「今天」`:2382`，再与 UTC 日键构成的 `allDaily` 比对 `:2390-2397`）与 heatmap 的窗口终点（`:2538-2544`）。**实际暴露面**：三端壳都会显式传 `tz`——macOS 的 `withAccountQueryItems` 本身就是 `withTimeZoneQueryItems` + `account=1`（`TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift:241-254`）、Linux 扩展 `_getJson` 自动附 `tz`/`tz_offset_minutes`（`TokenTrackerLinux/.../extension.js:997`）、Windows `UsagePoller` 自带 `tzQuery`（`TokenTrackerWin/UsagePoller.cs:321`）。因此这是**留给裸 HTTP / 脚本 / 测试的坑**，不是三端壳的现网故障；但「两族缺省基准不同」是源码事实，跨端时区设计必须按「哪一级回退给谁」分别处理。

**桶→日的跨时区示例与端点粒度**。桶本身是 UTC，「某个桶属于哪一天」完全由查询侧时区决定。`2026-10-09T18:20:00Z` 的桶：无 `tz`/`tz_offset_minutes` 时 `rowDayKey` 族给出 `2026-10-09`（UTC 日）；`tz=Asia/Kathmandu`（UTC+5:45）时当地为 `2026-10-10 00:05`、日键 `2026-10-10`——非整小时偏移被测试直接锁定（`test/session-cross-day-usage.test.js:40-58`，测试名含 "quarter-hour timezone boundaries"）。粒度上有一处必须点名：`/functions/tokentracker-usage-hourly`（`src/lib/local-api.js:3146`）返回的是**当地小时**，不是逐半小时——`aggregateHourlyByDay(rows, day, tz)`（`:600-609`）把两个半小时桶按当地小时合并，**键空间**是 24 个整点键 `${dayKey}T${HH}:00:00`，响应里不存在 `:30` 键，且返回 `Array.from(byHour.values())`（`:639`）即**只含有数据的桶，不预填充 24 项**。

**过滤是「日键闭区间」**：`const daily = allDaily.filter((d) => d.day >= from && d.day <= to);`（`src/lib/local-api.js:2364`）；项目端点谓词同型（`:130-134`：`(!from || day >= from) && (!to || day <= to)`，另用 `hasRange` 短路）。`from`/`to` 缺省为空串时比较退化为恒真（即「全部时间」）；区间**两端都含**；过滤粒度是**日**不是桶，因此同一天内无法用 `from/to` 做小时级筛选（小时级只能走 `usage-hourly?day=`）。同一面板里实际并存**三处**时间口径：热力图网格的**数据日键**走 `rowDayKey`（缺 `tz` = UTC 日；日数据先由 `aggregateByDay` 产出 `:2537`，`byDay` 索引在 `:2545` 构建、单元格在 `:2564` 查表并 `:2566` 构造）；热力图/汇总的**窗口终点** `todayStr` 与 `night_owl` 小时都走 `getZonedParts`（缺 `tz` = 运行环境本机时区，`:2538`、`:2382`、`:482`、`:585-592`）。

**会话级时间语义（另一套函数）**，三个函数在 `src/lib/session-analytics.js:2343-2383`：

| 函数 | 行为 | 行号 |
| --- | --- | --- |
| `sessionUsageDay(timestamp, { timeZone, offsetMinutes })` | `timeZone` 存在时用 `Intl.DateTimeFormat("en-CA")` 取 IANA 日串，formatter 抛错才落下一级；否则按 `offsetMinutes`（缺省 0）平移后取 **UTC 日** | `:2343-2354`（`:2353` 为 UTC 日分支） |
| `withinDayRange(row, from, to, context)` | **区间相交**判定（会话早于窗口开始但结束落在窗口内仍命中） | `:2356-2362` |
| `scopeSessionUsage(row, from, to, context)` | 按 `usage_buckets[].timestamp` 的日串过滤桶（闭区间 `:2366-2369`），重算 `tokens` 与 `model_usage`，并把生命周期值另存为 `lifetime_total_tokens`/`lifetime_cost_usd`（`:2381-2382`），`cost_usd: 0` 交给 `repriceSessionRecord` | `:2364-2383` |

**注意与 `getZonedParts` 的差异**：`sessionUsageDay` 的缺省是**绝对 UTC**，不会落到本机时区。行为由测试锁定（`test/session-cross-day-usage.test.js`）：跨日会话**每一天只计当天新增用量**，而不是把生命周期总量摊到每天（`:26-34`，断言首日 120 / 次日 60，且两日成本之和等于整行成本）；无 `from/to` 或带时区查询时回到生命周期总量（`:33`、`:34`）；重复的 `token_count` 快照不计两次（`:21` 构造两个完全相同的 count 行，期望总量仍 180）；效率面板保留生命周期总量。

**`heatmap-compact.js` 与日汇总的关系**：前者是云端 `format=compact` 稀疏载荷的读取适配器，只处理**已由边端聚合好的日粒度**数据，不做桶→日转换（`src/lib/heatmap-compact.js:3-24`）。三条硬规则：日键按 UTC 日历日推进（`nextDayKey` 用 `setUTCDate`，`:26-31`）；重复日 last-row-wins、present-but-empty 的 `models` 是 `{}` 而非 `null`（`:68-71`）；`billable_total_tokens` **直接复制** `total_tokens`，`level` 由窗口最大值按 0.25/0.5/0.75 派生（`:74-83`、`:89-97`）。模型名走字典加索引扁平对，`model_names` 缺失时**整包拒绝**而不是静默丢数据（`:52`）。本地热力图端点另有两条恒值：`week_starts_on` 恒为 `"sun"`（`src/lib/local-api.js:2586`）、`streak_days` **恒为 0**（`:2588`）——真实 streak 只在云端紧凑载荷里（`src/lib/heatmap-compact.js:108-109` 原样透传）。

### 7.4 去重机制全景（队列级 / 账户级 / 会话级 / fork / 双安装 / 跨根 / 事件级）

去重是**分层叠加**的，每层针对不同重复来源、用不同键，且**不能互相替代**：

| 层 | 场景 | 去重键 | 收敛判据 | 证据 |
| --- | --- | --- | --- | --- |
| 1 | 队列行（主队列） | `source\|model\|hour_start` | 后行覆盖前行（last-row-wins） | `src/lib/async-queue-reader.js:86`、`src/lib/local-api.js:207` |
| 1' | 队列行（项目队列） | `project_key\|source\|hour_start` | 同上 | `src/lib/local-api.js:149` |
| 2a | 账户级 `cursor`（无稳定会话身份） | `(source, hour_start, model)` | 整行 **MAX**：`total_tokens` → `updated_at` → `device_id` 字典序 | `src/lib/account-usage-dedup.js:188`、`:195-207`、`:230`、`:240` |
| 2b | 账户级 `trae-cn`（有会话身份） | `(user_id, source, session_id)`，内存键 `${source}\|${session_id}` | **严格更新**（`>`）才整行替换；等值保留先落库者（重放幂等） | `src/lib/account-usage-dedup.js:20-22`、`:113`、`:120-133` |
| 3 | 会话碎片 | `row.session_hash` | 数值列累加、上下文列取 max、布尔取 or、不一致标 `mixed` | `src/lib/session-analytics.js:118`、`:2665-2711` |
| 4a | OpenCode fork/clone | 消息指纹 + 会话归属 | 拷贝前缀只计一次，fork 自己的续写保留；同会话内重复永不删 | `src/lib/rollout.js:3805`、`:3722`、`:3795`、`:3717` |
| 4b | Codex fork 重放 | 跨日：`rolloutDate` vs `currentDate`；同日：500 ms 突发窗 | 日期更早或间隔 `< FORK_REPLAY_GAP_MS` 视为重放而跳过 | `src/lib/rollout.js:4210-4214`、判定实现 `:2588-2601` |
| 4c | Codex 事件级 | 游标键 `codexHashes` | 未读实现，哈希输入 `【未能确认】` | `src/lib/rollout.js:382`、诊断分支 `:351` |
| 4d | Claude 消息级 | `claudeMessageDedupKey(obj)` | **msgId 必需**；有 reqId 时合成 `msgId:reqId`，无 reqId 时退化为纯 msgId（去重照常生效） | `src/lib/rollout.js:4914-4918`、使用处 `:2854` |
| 5 | 双安装 / WSL | cursor 命名空间（`native`/`wsl`）+ both 模式路径 Set | 各自独立光标；扁平→命名空间迁移仅「恰好命中一个安装」才 seed 单槽 | `src/lib/multi-install-parser.js:31-56`、`:70-85`、`:92-110` |
| 6 | Claude 跨根同 UUID | 文件名 UUID | 字节完全相同才折叠（胜者取 mtime 更新者）；歧义时**退出去重** | `src/lib/session-analytics.js:1824`、`:1830-1845`、`:1895-1909` |
| 6' | Codex 跨根 | 文件名 UUID | 归组后**事件级**并入 | `src/lib/session-analytics.js:1912-1926` |
| 7 | 事件级并入（Claude/Codex 跨根） | 未定位到具体并入键 → `【未能确认】` | 行为由测试锁定：共享前缀计一次、两侧发散尾部都计入 | `test/session-analytics-claude-cross-root-dedup.test.js:174`、`:288` |
| — | 上传压缩 | `["hourly", source, model, hour_start]` / `["session", source, session_id]` | 保留最后（会话状态取严格更新） | `src/lib/upload-queue.js:53`、`:59`、`:88` |
| — | CLI wrapped | `source\|model\|hour_start` | 与第 1 层同规则 | `src/lib/wrapped-aggregator.js:80` |

**账户级（第 2 层）**。背景是账户级来源（`cursor`、`trae-cn`）的数据取自**每账户**的云 API，每台设备都存同一份副本——目标是「把 N 份副本收敛成 1 份真值」，所以 `cursor` 用 MAX 而不是 SUM（注释原文 `Mirrors the legacy SQL DISTINCT ON (hour, source, model) ORDER BY total_tokens DESC, updated_at DESC (device_id asc as the final deterministic tiebreak)`，`src/lib/account-usage-dedup.js:184-198`）。`trae-cn` 分支在同一操作里收敛三类修正（下调 token / 换模型 / 换半小时桶，`:30-32`），并且**缺席不等于删除**——空快照什么都不断言（`:34-36`）；回聚为小时行时按 `(bucket_start, source, model)` 求和，`conversations` 记会话条数而非消息数（`:159-174`）；控制记录用 `kind` 标记、读端一律跳过（`SESSION_STATE_KIND = "account_session_state"`，`:55-57`）。测试：`test/account-usage-dedup.test.js:129`（桶迁移 10:00→10:30 只剩后半、总量 100）、`:152`（模型迁移）、`:173`（**向下修正** 100→60 取 60，绝不取 MAX）、`:192`（新快照中缺席的会话保留）、`:212`（空快照）、`:233`（重放幂等）、`:267`（等值保留首条）、`:319`（畸形观察）、`:398`（cursor 仍走整行 MAX）、`:483`（读取器不得把会话状态控制记录当用量行曝露）。

**fork 检测（第 4 层）**。四套机制并存。Codex 是「跨日靠日期守卫、同日靠 500 ms 突发检测」：`FORK_REPLAY_GAP_MS = 500`、`isForkedReplayToken({ isForkedRollout, rolloutDate, currentDate })` 判定 `currentDate < rolloutDate`（`src/lib/rollout.js:4210-4214`）；真正的判定在解析循环里：`tokenMs - prevForkedTokenMs >= FORK_REPLAY_GAP_MS` → `replayPrefixActive = false`（实时回合重置突发窗，`:2588-2590`），`< FORK_REPLAY_GAP_MS` → `forkedReplaySkip = true`（视为重放，`:2591-2594`），跳过点是 `if (forkedReplaySkip || isForkedReplayToken(...)) continue;`（`:2601`）。500 ms 是刻意偏向下界的经验值——注释给出实测分布：重放行间隔亚毫秒至数毫秒、真实回合 ≥ 约 4.6 秒、重放→实时断裂 ≥ 11 秒，取舍理由是「合并一个真实快速回合（低估）比留下一个慢重放（有界高估）更糟」（`:4203-4209`）。Claude 侧是消息加请求 ID 键：**`msgId` 是硬门槛，缺失即返回 `null`（该行不去重）；有 `reqId` 时合成 `${msgId}:${reqId}`，否则退化为纯 `msgId`**（`:4914-4918`）。

必须与另一处同名判据分开记：**「`msgId && reqId` 才建键」是 v3 地面真值扫描的历史缺陷**（`src/commands/sync.js:227-233`），它静默关闭了所有缺 `requestId` 的 provider 的去重（DeepSeek/Kimi/Mimo/MiniMax 的 anthropic 兼容端点，以及 Claude Code 的子代理/thinking 通道），使真值膨胀 1.6–3.7 倍，v4 修掉了它；解析侧函数 `claudeMessageDedupKey` 从来没有这个严格判据。通用教训：**去重键的守卫条件会静默退化成「不去重」**。

**双安装 / WSL（第 5 层）**。机制是**光标命名空间隔离**（`native` / `wsl` 各一套），不是事后合并——每个安装按自己的偏移解析，天然不会重复回读（`src/lib/multi-install-parser.js:31-56`）。三条判定：单安装归位按真实身份而非模式偏好（用 `wsl.isUncPath(...)` 判定幸存者，`:13-22`）；扁平→命名空间迁移的归属靠一次性探针，命中**恰好一个**安装才只 seed 那一个，无探针、无命中、多命中、探针报错**全部退回 seed 全部**（`:61-85`，注释：「少了去重状态会整段重复计数，而有界回填缺口是更便宜的失败」）；both 模式下按路径 Set 去重文件清单（`:92-110`）。测试：`test/claude-wsl-dual-install.test.js:65`、`:93`、`:138`、`:170`、`:219`；`test/codex-wsl-shadow.test.js:181`；`test/dual-install-*`（4 个文件）。

**跨根（第 6 层）**。会话文件在多个扫描根下按**文件名 UUID** 归组（`groupClaudeFilesAcrossRoots`，`src/lib/session-analytics.js:1824`），两条保守规则：同一根内出现重复 UUID → **该 UUID 整体退出去重**（「歧义重复代价低，删掉会话代价高」，`:1830-1845`）；只在**字节完全相同**时折叠，内容发散者全部保留（`sameFileContent` `:1879`、`dedupeClaudeFilesAcrossRoots` `:1895-1909`）。Codex 侧同型但更简单（`groupCodexFiles`，`:1912-1926`）。不变量（`test/session-analytics-claude-cross-root-dedup.test.js`）：两根同会话折叠为一（`:65`）、胜者按 mtime 而非根序（`:77`）、不同会话都保留（`:87`）、单根不触发归组（`:98`）、**单根同 UUID 兄弟关闭去重**（`:120`）、非 UUID 文件名永不参与（`:139`）、顺序跟随根序（`:156`）、**同 UUID 不同内容都保留**（`:165`）、Claude 跨根共享前缀计一次且两侧发散尾部并入（`:174`）、Codex 跨根重复 token 事件只计一次（`:288`）、镜像读取中途失败时丢弃暂存记录（`:247`）。

**`conversation_count` 跨来源不可比**。源码里至少三种口径：按会话（首见即 1、后续同步 0，Craft，`src/lib/rollout.js:17232-17243`）、按**主会话 user 文本消息**（每条用户文本消息 +1，排除 `tool_result` 与子代理；计数语句在 `:2833`，位于 `isMainSession && userObj?.type === "user"` → `hasText` 内层分支，`:2807-2837`）、按请求数（`conversation_count: delta.requests`，`:17044`）；另有若干处刻意置 0 以避免重复计数（`:18886`、`:9287`）。**源码自身两处注释互相矛盾**：Craft 侧注释自称 Claude「count one-per-assistant-message」（`:17233-17234`，与代码不符），解析侧注释与代码一致（`:2807-2809`）——**以代码为准**。云端还曾因字段映射 bug 把 `conversation_count` 读成 `b.conversations` 而长期归零，修复方式是重置上传偏移、整队列重放（`src/commands/sync.js:249-257`）。

### 7.5 repair / 迁移体系（历史数据的影响面是本章最重的一条）

**触发与记账**。所有一次性迁移都靠**游标里的 migration 记录**做幂等门禁，键集中在 `src/commands/sync.js:204-309`（`cursorUnknownPurge_2026_04`、`rolloutCumulativeDeltaReparse_2026_05`、`claudeMemObserverReinclude_2026_05_v3`、`grokAppendOnlyRepair_2026_05_v4`、`codebuddyLogJsonlOverlapRepair_2026_08`、`workbuddyContextUsageRepair_2026_08`、`claudeGroundTruthRepair_2026_05_v4`、`cloudConversationsBackfill_2026_06`、`codexRescanDedupRepair_2026_06`、`codexForkReplayRepair_2026_07`、`codexUsageLineageRepair_2026_07_v2`、`droidDupSessionInflationRepair_2026_06`、`mimoClaudeMislabelRepair_2026_06`、`deepseekHarnessSourceMigration_2026_08`、`zcodeNativeUsageRepair_2026_08`、`zcodeInclusiveTokenRepair_2026_09`）。典型门禁形态是「读一次 migration 记录，已应用即 `return false`」（如 `repairGrokQueueFromSessionSnapshots` `:5010-5016`、`migrateLegacyDeepseekHarnessSource` `:3865-3868`）。且**「无事可做」也会记账**（写 `status: "noop"`），避免每轮重复扫描（`:3913-3920`）。

| # | 函数（定义处） | 触发键 | 调用点 | 做了什么 | 对历史数据的影响 |
| --- | --- | --- | --- | --- | --- |
| 1 | `migrateRolloutCumulativeDeltaBuckets` `src/commands/sync.js:5216` | `rolloutCumulativeDeltaReparse_2026_05` | `:900` | 重解析累积型 delta 的桶，对受影响键**追加**撤回行 | 只追加（`:5281` 为 `appendFile`） |
| 2 | `repairCodexRescanInflation` `:5682` | `codexRescanDedupRepair_2026_06` | `:901` | 从磁盘按事件去重重建 codex 桶，**原子丢弃**膨胀行、追加重建行、重置上传偏移 | 原子重写 + 偏移清零（`:5895-5922`） |
| 3 | `repairCodexForkReplayInflation` `:6038` | `codexForkReplayRepair_2026_07` | `:909` | 用**当前 fork-aware 解析器**重跑 #2 的带守卫重建以排除重放行 | 同 #2；另有前置门禁（磁盘无 fork rollout 则直接标完成） |
| 4 | `repairCodexInterleavedUsageInflation` `:6134` | `codexUsageLineageRepair_2026_07_v2` | `:918`、`:938` | 修复多 SessionState 交错导致的巨额 delta | 同 #2（v2 去掉不可靠前置门禁，改为逐条校验） |
| 5 | `repairDroidDuplicateSessionInflation` `:6612` | `droidDupSessionInflationRepair_2026_06` | `:927` | 重建 droid 桶（依赖 `repairKeys` 过滤 `:6765`），**先备份再原子写** | 原子重写，**有备份**（`:6743`、`:6772`） |
| 6 | `repairMimoClaudeMislabel` + `dropMimoQueueRows` `:4782`、`:4744` | `mimoClaudeMislabelRepair_2026_06` | `:928` | 0.57.0 把 mimocode 镜像的 Claude/claude-mem 历史误标成 `mimo`；本修复**清除**本地队列与游标里的 `source=mimo` 数据，等下一轮用 providerID 过滤的读取器重建 | **会删数据**（设计如此，靠重扫恢复；注释 `:302-306`） |
| 7 | `reincludeClaudeMemObserverFiles` + `relabelClaudeMemQueueRows` `:7097`、`:7147` | `claudeMemObserverReinclude_2026_05_v3` | `:1148`、`:7135` | 重新纳入曾被排除的 claude-mem observer 会话文件，并给队列行重打标签 | 原子重写（`:7207-7209`） |
| 8 | `repairClaudeQueueFromGroundTruth` `:6844` | `claudeGroundTruthRepair_2026_05_v4` | `:1183` | 用地面真值扫描重建 `source=claude` 行，原子重写主队列（`:6874`、`:6918-6920`）并修项目队列（`:7015`） | **原子重写，历史上导致过大规模数据丢失**（见下） |
| 9 | `repairZcodeNativeUsageMigration` `:4994` | `zcodeNativeUsageRepair_2026_08` | `:1761` | 修复 zcode 原生用量模式解析 | 整队列 tmp+rename 原子重写——`repairZcodeUsageMigration`（`:4940`）调 `dropZcodeQueueRows`（`:4884`）：先 `backupExistingFile`（`:4929`），再临时文件（`:4930`）`writeFile`（`:4935`）后 `rename`（`:4936`）；主队列带 `{ retainRetractions: true }` 保留撤回零行（`:4953`）、项目队列不保留（`:4955`）；仅 `removed > 0` 时重置上传偏移（`:4979-4984`） |
| 10 | `repairZcodeInclusiveTokenMigration` `:5002` | `zcodeInclusiveTokenRepair_2026_09` | `:1769` | 修复 zcode「含缓存」token 口径（对应 `dropZcodeQueueRows` `:4884`） | 涉及删除 zcode 行；保留撤回行的开关是签名参数 `dropZcodeQueueRows(filePath, { retainRetractions = false } = {})`（`:4884`） |
| 11 | `migrateLegacyDeepseekHarnessSource` `:3865` | `deepseekHarnessSourceMigration_2026_08` | `:1808` | `source="deepseek"` 重标为 `"dsh"`；为每个旧键**追加显式零行**；把最新规范行重追加到队尾；重置上传偏移 | 整队列 tmp+rename 原子重写（`:3945-3948`：临时文件 `writeFile` + `rename` 覆盖；既有行全部原样保留 `:3887`/`:3905`，非「只追加」）→ 重写后再追加规范行（`:3925-3927`）与显式零行（`:3929-3943`）→ 偏移自 0 重放（`:3950-3962`） |
| 12 | `migrateCursorUnknownBuckets` `:5173` | `cursorUnknownPurge_2026_04` | `:2151` | cursor 的 unknown 桶清理 | 只追加撤回行（`:5207`） |
| 13 | `repairCodebuddyLogJsonlOverlap` `:5315` | `codebuddyLogJsonlOverlapRepair_2026_08` | `:2575` | 修复 log 与 jsonl 双源重叠；先写临时队列重建（`:5401`），再 tmp+rename 落位（`:5460-5462`） | 原子重写 |
| 14 | `repairWorkbuddyContextUsage` `:5508` | `workbuddyContextUsageRepair_2026_08` | `:2624` | 同型：临时队列重建 + 原子落位（`:5571`、`:5628-5630`） | 原子重写 |
| 15 | `repairGrokQueueFromSessionSnapshots` `:5010` | `grokAppendOnlyRepair_2026_05_v4` | `:2986` | 从 `cursors.grok.sessionSnapshots` 重建 grok 桶；清空游标里所有 `grok\|` 桶与 groupQueued（`:4629-4657`）；追加修复行（`:5126`）；重置上传偏移 | 清空游标态 + 追加重建行 + 偏移清零；**原始队尾有备份**（`:5124`） |
| 16 | `backupExistingFile(filePath)` `:4711` | — | — | 备份工具，供 #5、#15 等调用 | — |

（上表「定义处/调用点」列的行号均属 `src/commands/sync.js`。）grok 修复的 token 拆解是**估算**而非实测：`estimateGrokRepairTotals` 按 80/20 切分输入输出（`:4551-4563`）。

**守卫族与调用范围（第二轮复核后的准确边界）**。原子重写类 repair 的「数据丢失守卫」由两个函数组成：`mergeMaxTotals`（`src/commands/sync.js:6574-6583`）跨「现有队列 + 游标态」与「重建结果」两组来源取 MAX；`setMaxTotal`（`:6585-6589`）对同一键取 MAX。**`mergeMaxTotals` 的调用点恰四处**：`:5848`、`:5852`、`:5856`、`:5860`（含判定循环 `:5865-5879`），全部位于 codex rescan/fork 重建流程的 `projectRepairEnabled` 分支内（该 `if` 块为 `:5823-5880`）——这四处就是 `mergeMaxTotals` 的全部调用。`setMaxTotal` 另有 5 处族内调用（`:6492`、`:6512`、`:6540`、`:6559`、`:6579`，位于 `:6477-6562` 的四个 totals 函数与 `mergeMaxTotals` 内部），与上句的四处分开计数。同流程内共有**四道守卫**，判据各不相同，不可混记：

| 守卫 | 位置 | 判据 | 是否在调用族范围内 |
| --- | --- | --- | --- |
| 重建产出守卫 | `src/commands/sync.js:5813-5822` | 重建产出 0 个 codex 桶 | 否 |
| 项目键集合守卫 | `:5841-5846` | `missingProjectKeys = existingProjectKeys \ rebuiltProjectKeys`，用**键集合**而非 MAX | **否**（不在 `mergeMaxTotals` 调用族内；它位于同一函数体 `:5823-5880` 之中、判据也不同） |
| 总量下降守卫 | `:5865-5879` | 某项目键 `rebuiltTotal < existingTotal` 且主桶又没有对应提升 → 跳过本轮修复（`console.error(... "skipping to avoid data loss")` + `return false`） | **是**（在调用族范围内） |
| 文件快照守卫 | `:5882-5890` | 文件快照校验失败 | 否 |

同族的 claude 侧守卫是**「任何曾贡献过的会话文件已不在磁盘则整轮跳过」**，注释句首在 `:264`（整段 `:258-267`）：`GUARDED: skips if any codex session file that previously contributed is no longer on disk … clearing its bucket would lose that history (ref the v6 ground-truth-repair data-loss incident)`。整体取向可以概括为：**宁可欠修，不可删史**。

**0.26.3 事故：repair 真的删过 21.7 亿 token**。

```js
// src/commands/sync.js:234-247（节选）
// v6 was bumped in 0.26.3 to re-run the repair with the zero-usage dedup fix
// applied. SHIPPING 0.26.3 caused catastrophic data loss on every upgrader
// whose ~/.claude session jsonls had been pruned by Claude Code's own
// cleanup: the repair does atomic-drop + rescan, so any hour_start no longer
// represented in the on-disk logs is silently removed from queue.jsonl. On
// the reporter's machine this wiped 2.17B claude tokens (-1.27B opus-4-7,
// -474M opus-4-6, -376M sonnet-4-5, -48M haiku, -6M sonnet-4-6). The
// upload-offset reset also propagated the damage to the cloud.
// 0.26.4 HALTS back at v4 so the buggy atomic-rewrite path stops auto-firing
// on existing installs.
```

数字自洽性验算：事件规模 **2.17B = 21.7 亿**；分项求和 `1270M + 474M + 376M + 48M + 6M = 2174M ≈ 2.174B`，与源码的 2.17B 一致。回退事实：`0.26.4 HALTS back at v4`（`:242`），key 仍为 `claudeGroundTruthRepair_2026_05_v4`（`:248`）。**根因结构**：`queue.jsonl` 是**唯一**持有某些历史的副本。当上游 agent 自己清理了会话日志（Claude Code 的 cleanup），「从磁盘重建」就等于「以磁盘为空为由删除历史」；上传偏移清零后又把删掉的结果推到云端。这条对任何「本地汇总 + 云上传」的设计都成立。

**四种安全模式（同一仓库都用过）**：只追加撤回行（最安全：`retractStaleSourceQueueRows` `src/lib/rollout.js:19990`、`migrateCursorUnknownBuckets` `src/commands/sync.js:5207`）；原子重写 + 前置备份（`backupExistingFile` `:4711` → tmp+rename `:6772`）；原子重写 + 不可复现守卫（磁盘缺文件就整轮跳过，`:264`）；清空重扫（最危险，仅用于误标修复：`dropMimoQueueRows` `:4744`，仅在「磁盘一定还在」时安全）。另有一处**不属于上述四档**的形态：`migrateLegacyDeepseekHarnessSource`（`:3865-3962`）是「原子重写但全量保留既有行」——它把原文件的每一行都原样保留（`:3887`、`:3905`），不删任何既有行；并会对每个受影响旧键**重加一条规范行**（`:3925-3927`）与一条零值撤回行（`:3929-3943`）后重排，与「只往文件尾追加」是两回事。

另有一条被多处沿用的设计路径：**上传偏移清零会把本地修复结果广播到云端**。`CLOUD_CONVERSATIONS_BACKFILL_KEY` 的注释明说云端历史**只能**靠本地队列重放修复（`src/commands/sync.js:249-256`）。这意味着「本地算错 → 修好 → 云被覆盖」是设计路径；反过来，若本地修复本身错误，云端也会被污染。

### 7.6 查询与分页、派生指标

**服务外壳与鉴权**：只绑回环（`LOCAL_BIND_HOST = "127.0.0.1"`，`src/commands/serve.js:34`、`:157`、`:672`）；**写操作**需本地 token + 回环 Origin，读端点不需要（`src/lib/local-api.js:1629-1637`）；token 是进程内随机 24 字节，经 `/api/local-auth` 下发给同源页面（`:1016`、`:1681-1693`）；跨站或非回环 Origin 在消耗请求体前返回 403 `untrusted_auth_origin`（`:1644-1648`）；头 `x-tokentracker-instance` 或已签发 token 与当前后端指纹不符时返回 409 `instance_changed`，不静默转发（`:1658-1669`）；会话浏览端点带 `Cache-Control: no-store` + `X-Content-Type-Options: nosniff`，失败时不把绝对路径写进响应体（`:2509-2510`、`:2516-2521`）。

**查询链：共享前两跳，之后分叉**。除会话/项目/上下文体检等支线外，所有用量端点的第一跳都是 `scopedQueueRows`（`src/lib/local-api.js:403-412`），它同时完成**队列去重**（`readQueueData`）与**来源范围过滤**（`filterRowsByUsageScope`，定义在 `src/lib/source-metadata.js`）；`scope` 由 `include_account_level=1` 或 `scope` 参数决定（`:398-401`）。第二跳是读时纠错 `normalizeQueueRow`（`:170`，见 7.2）。之后**各端点分叉**：

| 口径 | 端点 | 计算位置 | 关键行 |
| --- | --- | --- | --- |
| 总量（hero） | `/functions/tokentracker-usage-summary` | 在 `aggregateByDay` 结果上过滤 `from/to` 后 reduce | `src/lib/local-api.js:2358`、`:2363`、`:2365` |
| 滚动 7/30 天 | 同上，`rolling` 字段 | `collectDays(n)` + `sumDays` | `:2390`、`:2399`、`:2406` |
| 按天 | `/functions/tokentracker-usage-daily` | 直接回传日数组 | `:2425`、`:2430` |
| 按来源 + 按模型 | `/functions/tokentracker-usage-model-breakdown` | 自建 `bySource` → `models` 两级 Map | `:2596`、`:2602`、`:2640` |
| 按月 | `/functions/tokentracker-usage-monthly` | `byMonth`，月键 = `day.slice(0,7)` | `:3156`、`:3168` |
| 某日逐小时（**当地小时**） | `/functions/tokentracker-usage-hourly` | `aggregateHourlyByDay`，两个半小时桶按当地小时合并 | `:3146`、`:600-609`、`:639` |
| 热力图 | `/functions/tokentracker-usage-heatmap` | 日数组 + 网格 + 分档 | `:2533`、`:2549` |
| 按项目 | `/functions/tokentracker-project-usage-summary` / `-detail` | 走**项目队列**，另一套键 | `:2731`、`:2828` |

**只有 summary、daily、heatmap 三个端点走 `aggregateByDay`**（`src/lib/local-api.js:227`）；hourly 走 `aggregateHourlyByDay`（`:600`）；model-breakdown（`:2602-2641`）、monthly（`:3161-3186`）、project-summary（`:2744-2798`）、project-detail（`:2866-2898`）都是**各自遍历行自建 Map**。并不存在「所有端点共享一条聚合链」——这是一个常见误读点。日聚合内部还有一个必须记住的口径——**成本按行计算后累加，不是「先合 token 再定价」**：`a.total_cost_usd += computeRowCost(row)`（`:285`），原因是优先档/长上下文档位非线性（`:2634-2640` 的注释解释了为何不能先汇总再计价，见 8.1）。聚合中途价格表被刷新的重入保护：`if (pricingRevision !== getPricingRevision()) return handleLocalApi(req, res, url);`（`:2642`）。模型分解另有「服务端自报成本优先」与「混合来源」标记：`reportedCost = ["grok", "cline"].includes(src) && Number(row.total_cost_usd) > 0`，`cost_source` 在 `provider_reported` / `model_pricing` / `mixed` 三态间切换（`:2624-2626`）。

**项目维度是另一条链**：不走 `readQueueData`，而是 `readProjectUsageContext`（`:125`）+ `readProjectQueueData`（`:145`）。额外三件事：剔除 claude-mem 伪项目（过滤键由 rollout 常量派生，`:88-94`）；建立 `dayInRange` 谓词；每行日键只计算一次（注释指出 `rowDayKey` 内部的 `Intl.DateTimeFormat` 是主导成本，`:121-124`）。兜底行为：**仅当项目队列完全为空**时改用主队列按 source 聚合，且刻意不伪造 `project_ref`（旧行为会造出 `https://codex.ai` 之类可点击假链接，`:2803-2818`）。因为项目行**没有 model 维度**，项目详情里的 `models` 字段实际装的是 **source**（注释明说是为了复用 TrendMonitor 的堆叠段槽位，`:2877-2883`）。

**分页与排序**。**用量端点全部不做服务端分页**（返回整段数组，由前端分页），只有两支例外：`sessions` 的 `limit` 上限 **2000**（`0` 或非法值 = 不限，`src/lib/local-api.js:2504-2505`）；`project-usage-summary` 的 `limit` 硬上限 `PROJECT_USAGE_MAX_ENTRIES = 10`（默认值即上限，`:2737-2740`、`:95`）。`usage-daily` / `-summary` / `-model-breakdown` / `-monthly` / `project-usage-detail` 均无分页（`:2430`、`:2414`、`:2656`、`:3187`、`:2900`）。明细页的真正分页在**前端**：`dashboard/src/lib/details.ts` 的 `paginateRows(rows, pageIndex, pageSize)`，由 `test/details-pagination.test.js:5-24` 锁定（25 行数据、页大小 12 → 12/12/1）。

| 位置 | 排序键 | 证据 |
| --- | --- | --- |
| 日聚合 / 月聚合 | `day` / `month` 字典序升序 | `src/lib/local-api.js:299`、`:3187` |
| 模型分解 | `total_tokens` 降序 | `:2650` |
| 项目汇总 / 项目详情 sources | `billable_total_tokens` 降序 / `total_tokens` 降序 | `:2785`、`:2913` |
| 账户会话聚合 | `hour_start` → `source` → `model` 三级字典序 | `src/lib/account-usage-dedup.js:176-181` |
| 本地徽章 | 按 `hour_start` 升序重放，取首次越过阈值的桶 | `src/lib/local-api.js:435-439` |
| 上下文体检最大项 | `estimated_tokens` 降序取前 10 | `src/lib/context-health.js:111` |

**查询侧缓存与失效**。

| 缓存 | 键 | 失效条件 | 位置 |
| --- | --- | --- | --- |
| 队列解析结果 | 队列路径（`pending` 单飞） | 文件签名 `dev:ino:size:mtimeNs:ctimeNs` 变化；ENOENT 清缓存 | `src/lib/async-queue-reader.js:17`、`:34`、`:98` |
| 日聚合 | `WeakMap<行数组, Map<时区\|偏移\|价格修订>>` | 行数组对象身份变化（队列变了）或价格修订变化 | `src/lib/local-api.js:104`、`:229`、`:115-117` |
| IANA 时区 formatter | 时区 id | LRU 16 条（失败结果也缓存） | `:105`、`:96`、`:545-552` |
| 上下文体检 | `home\0cwd\0两个环境变量` | TTL 5 分钟，LRU 8 条 | `src/lib/context-health.js:64-67`、`:119-120` |
| 会话分析 | 文件统计键 + 侧车文件 | TTL 5 分钟 / `force` 参数 | `src/lib/session-analytics.js:2037`、`:2046`、`:2100`、`:2140`、`:2315` |
| 账户视图失败退避 | 进程内 | 2 分钟冷却 | `src/lib/local-api.js:27` |
| 头像代理 | URL | TTL 1 小时，LRU 64，单图 ≤512 KiB | `src/lib/local-api.js:45-48` |

日聚合缓存的键是「行数组对象身份 + 时区 + 价格修订」三者缺一不可，且**价格修订在聚合中途变化会重算而非混用**（`src/lib/local-api.js:104`、`:115-117`、`:243-249`）。常量：`PROJECT_USAGE_MAX_ENTRIES = 10`、`MAX_TIME_ZONE_CACHE_ENTRIES = 16`、`AGGREGATION_BATCH_ROWS = 256`（每 256 行让出事件循环）（`:95-97`、`:264`）。测试：`test/local-api-queue-cache.test.js:52`（缓存复用同一次解析直到文件变化）、`:103`（缓存下 IANA 时区日边界仍正确）、`:129`（价格修订失效缓存但不重读队列）、`:170`（大聚合期间仍能响应其他 HTTP 请求）、`:205`（聚合过程中价格刷新不允许发布混合修订）。

**派生指标：数据来源与口径**。

| 视图 | 数据来源 | 关键口径 | 证据 |
| --- | --- | --- | --- |
| 上下文体检 | **本机配置文件，不读队列** | 估算固定开销；MCP 部分非实测 | `src/lib/context-health.js:43-62`、`:69-90` |
| 会话性能 | 会话日志的**时间戳与 usage 记录**（不是队列行） | 分子取「已按 canonical 规则只计过一次」的 output+reasoning | `src/lib/session-performance.js:5-12`、`:56-59`、`:79-86` |
| wrapped | 队列行（自行按 7.4 第 1 层去重） | 取 `billable_total_tokens ?? total_tokens`；日 = `hour_start.slice(0,10)`（UTC 日）；小时取 ISO 的 `T(\d{2})` | `src/lib/wrapped-aggregator.js:29-53`、`:77-85` |
| 本地徽章 | 项目队列 + 主队列 | `night_owl` 按请求时区小时（`parts.hour < 6`），阈值 `[5,20,60,150]` | `src/lib/local-api.js:419-423`、`:478-486` |
| outcomes（质量/美元） | 侧车 `outcomes.jsonl` + 队列行 | 读时 join，文件不存在则 `available:false` | `src/lib/local-api.js:2435-2467` |
| 会话浏览器 | 会话分析 + 逐日切片 | 见 7.3 会话级时间语义 | `src/lib/session-analytics.js:2343-2383` |

**上下文体检**来源是指令文件（`~/.claude/CLAUDE.md`、`~/.codex/AGENTS.md`、`$cwd/CLAUDE.md`、`$cwd/AGENTS.md`，`:69-78`）、技能文件（四个技能根下的 `SKILL.md`，遍历上限 500 文件，`:79-90`、`:25`）、MCP 服务器计数（`:43-62`）。估算口径：CJK 每字符 1 token、其余每 4 字符 1 token（函数 `:11-16`）；MCP 部分**不是实测**，而是 `服务器数 × 每服务器工具数(默认 5) × 每工具 schema tokens(默认 400)`（环境变量 `TOKENTRACKER_MCP_TOOL_SCHEMA_TOKENS` / `TOKENTRACKER_MCP_TOOLS_PER_SERVER`）；分级阈值 `total >= 50000 ? "high" : total >= 20000 ? "medium" : "low"`（`:94-98`）。**它不读队列、不计入任何 token 总量**，只产出「固定开销估计」，并自标 `provenance.confidence = "inferred"`、`content_retained: false`（`:112-117`；文件头亦声明不返回文件内容，`:3-4`）。测试：`test/context-health.test.js:10`。

**会话性能**的口径常量是 `MIN_OUTPUT = 200`、`MIN_DURATION_MS = 1000`、`MAX_DURATION_MS = 1 小时`、`SAME_FLUSH_MS = 100`（`:5-12`）；派生量 `tokens_per_second = output / (duration/1000)`、`first_response_ms = total / samples`（`:19-26`）；`record()` 用三个常量门控后累加（`:56-59`）。**关键约束**：性能模块**不重复计 token**——分子直接取会话扫描中已按 canonical 规则只计过一次的 output + reasoning，注释原文 `Later content blocks extend only the timing, never the numerator`（`:79-86`）。测试锁定的边界（`test/session-performance.test.js`）：Claude 请求速度按父边界与完整内容块计一次（`:37`）；缺开始时间/截断/错误/窗口过短都不估算（`:52`）；Codex 请求速度**排除工具执行时间**（`:80`）；重复快照不结束在途请求也不翻倍 token（`:104`）；原生与 WSL 镜像每个 canonical 请求只留一个样本（`:220`）。

### 7.7 并发与一致性

**互斥：进程内串行 + 跨进程文件锁**。

| 层次 | 手段 | 证据 |
| --- | --- | --- |
| 跨进程互斥 | `sync.lock`（`openLock(lockPath, …)` / `inspectLock`，来自 `src/lib/fs.js`） | `src/commands/sync.js:18`、`:552`、`:439`、`:449`、`:466`、`:476` |
| 压缩前置条件 | **调用者必须已持锁**（注释写在函数头） | `src/lib/upload-queue.js:62` |
| 常规桶 / 项目桶写入 | 纯 `fs.appendFile`，单行 JSON + `\n` | `src/lib/rollout.js:3483`、`:3489`、`:20031` |
| pending 压缩 | tmp + `sync()` + 一致性复查（inode/size/mtime）+ `rename` | `src/lib/upload-queue.js:94`、`:111`、`:121` |
| repair 重写 | tmp + `rename` | `src/commands/sync.js:3946`、`:5920`、`:6772`、`:7207` |

`src/lib/fs.js` 的锁实现细节（是否 `O_EXCL`、是否写 pid、超时与陈旧锁回收策略）本次未展开 → `【未能确认】`。**判定条件**：读 `src/lib/fs.js` 全文，核对 `openLock` 的创建标志位与 `inspectLock` 的陈旧判定，并查 `test/` 中 lock 相关用例。

**崩溃窗口与原子性**。

| 场景 | 崩溃时的可恢复性 |
| --- | --- |
| 追加写一半（进程被杀） | 尾部可能是**不完整行**。读端两个保护：`JSON.parse` 失败即计 `malformed` 并跳过（`src/lib/async-queue-reader.js:49-57`）；无末尾换行的残行只进 `tail`、键不进已提交映射（`:86-91` 注释）。测试 `test/async-queue-reader.test.js:66` |
| 压缩中途 | 目标文件在 `rename` 前字节不变，临时文件失败即清理；替换前有 inode/size/mtime 复查，检测到并发改动直接抛错（`src/lib/upload-queue.js:112-123`） |
| repair 原子重写中途 | 同上（tmp + rename）；部分 repair 前置 `backupExistingFile`（`src/commands/sync.js:4711`） |
| 上传偏移与队列不一致 | `queue.state.json` 与 `queue.jsonl` 分开写；窗口靠「重放幂等」兜住（云端整行 upsert，重复上传不增加用量，`src/commands/sync.js:249-256`） |

**读侧与写侧的相互假设**（读侧全部建立在「追加写是单行原子、且后写的累计值更完整」之上）：

1. **签名一致性优先于内容新鲜度**：签名不变即复用上次快照；签名变化（含同尺寸重写、截断、删除）必须丢弃（`src/lib/async-queue-reader.js:30-35`；测试 `:48`）。
2. **续读需要双重证据**：inode 不变 + 文件变大 + 首部 256 字节锚点一致，三者同时成立才从上一次 `consumedBytes` 继续（`:40-47`）——这排除了「文件被替换成更大文件」这一最常见的错误续读场景。
3. **读端不做跨行推算**：除了 `normalizeQueueRow` 的读时纠错，读端不推断缺失行、不补零、不跨桶聚合（7.2）。
4. **写侧假设自己独占追加权**：所有写入路径都在 `sync.lock` 下，压缩额外要求持锁（`src/lib/upload-queue.js:62`）。
5. **崩溃后不会出现「半行被当成有效行」**：坏行跳过并计数，不 panic（`test/async-queue-reader.test.js:66`）。
6. **没有「冻结快照」概念**：每次请求都重新解析 `new Date()`（`src/lib/local-api.js:2382`），不存在 as_of 冻结——同一秒内两次请求的「今天」可能不同。

**未闭合项（供第 12 章汇总）**。

| # | 项 | 现状 | 判定条件 |
| --- | --- | --- | --- |
| 1 | `src/lib/fs.js` 锁实现（`O_EXCL` / pid / 超时 / 陈旧回收） | `【未能确认】` | 读 `src/lib/fs.js` 全文 + `test/` 中 lock 用例 |
| 2 | `codexHashes` 事件去重的哈希输入 | `【未能确认】`（`src/lib/rollout.js:382` 只是游标键名） | 定位写入点，确认哈希覆盖字段集合与跨文件碰撞风险 |
| 3 | 跨根「事件级并入」的具体并入键 | `【未能确认】`（行为由 `test/session-analytics-claude-cross-root-dedup.test.js:174`、`:288` 锁定） | 沿 `scanClaudeSession` 的 `seenMessageHashes` 分支（`src/lib/rollout.js:2854`）向下读跨根汇合点 |
| 4 | copilot 桶粒度（`copilotOtelAggregateKey`，`src/lib/rollout.js:17462`） | `【未能确认】` | 读该函数体与调用点，确认第二参数来自 `toUtcHalfHourStart` 还是别的桶函数 |
| 5 | 若干端点的参数与返回字段（`cloud-session`、`local-sync`、`cloud-sync-pref`、`proxy-config`、`proxy-test`、`telemetry-pref`、`subscription-manager`、`skills`、`usage-limits`） | 端点存在，参数未逐行确认 | 逐个读 `src/lib/local-api.js` 对应 `if (p === ...)` 分支的 `url.searchParams.get(...)` 集合与响应载荷 |
| 6 | `src/lib/source-metadata.js` 的 scope 过滤规则 | 未展开 | 读该文件全文，列出 `ACCOUNT_LEVEL_SOURCES` 成员与 `normalizeUsageScope` 取值域，对照 `listExcludedSources`（`src/lib/local-api.js:410`） |
| 7 | `test/billable-total-tokens-migration.test.js` 的真实用途（实测 9 行、顶层 `test(` 0 次） | `【未能确认】` | 读全文并 `rg` 其引用方，确认是辅助模块还是失效残留 |

---

## 8. 金额计算全链路

金额的唯一计算入口是 `computeRowCost(row)`，返回一个 `number`（USD）。围绕它有三层价格表、一套 8 步匹配算法、四类特殊档位，以及**两张不同步的云端手抄表**。本章按「单行语义 → 表来源 → 匹配算法 → 特殊计价 → 缺口与风险 → 手工演算」展开。

**三条总纲**：① 计价**只用五个 token 桶**（`input` / `output` / `cached_input` / `cache_creation` / 条件性的 `reasoning`），**从不用 `total_tokens`**；② 价格表缺字段一律按 0 计，**不做任何推导或插值**；③ 前端不重算，金额只由后端与云端 edge（各一份实现）产生。

### 8.1 `computeRowCost` 逐行语义（四桶计价 + reasoning 门控 + 短路规则 + 长上下文溢价 + priority 倍率）

**签名、参数与返回**。`src/lib/pricing/index.js:227` 起，**单参数、无 options 对象**；`row` 是鸭子类型对象，只读 `source`、`model`、`total_cost_usd`、`hour_start`、`pricing_tier`、`input_tokens`、`output_tokens`、`cached_input_tokens`、`cache_creation_input_tokens`、`reasoning_output_tokens`、`long_context_*`、`priority_*`（`:228-350`），**不要求 `total_tokens`**。返回值是纯 `number`；无命中价格时返回 `0`——「未定价」与「免费」在这一层**不可区分**，需要区分时走 `getModelPricingInfo`（`:141-176`，见 8.3 末段）。导出在 `:371`；`src/lib/local-api.js:57-64` 是 import 解构入口（`:60`），`:3527-3531` 是 `module.exports` 再导出（`:3530`），两者是同一符号的入口与出口，不是两处定义。**返回值的三个可能来源**：常数 `0`（`lmstudio` / Pi 订阅源，`:228`、`:232`）；`row.total_cost_usd`（`grok` / `cline` 且为正，`:236-241`）；五桶算式（其余情况，`:254-354`）——其中 `reasoning` 那一桶对 5 个 source 恒为 0，等价于四桶。

**三条前置短路（在算钱之前就返回）**：

```js
// src/lib/pricing/index.js:227-242（节选）
function computeRowCost(row) {
  if (LOCAL_INFERENCE_SOURCES.has(String(row?.source || "").toLowerCase())) return 0;
  if (PI_SUBSCRIPTION_SOURCES.has(String(row?.source || "").toLowerCase())) return 0;
  const reportedCost = Number(row?.total_cost_usd);
  if (SOURCES_WITH_AUTHORITATIVE_COST.has(row?.source) && Number.isFinite(reportedCost) && reportedCost > 0) return reportedCost;
  const pricing = getRowPricing(row);
```

| 短路 | 集合 | 语义 | 证据 |
| --- | --- | --- | --- |
| 本地推理 | `LOCAL_INFERENCE_SOURCES = new Set(["lmstudio"])` | 本地开发服务器提供的推理无边际成本（Secure Cloud 走另一条计费路径，不出现在这些日志里） | `src/lib/pricing/index.js:27`、注释 `:24-26` |
| Pi 订阅路由 | `PI_SUBSCRIPTION_SOURCES`（4 个成员：`pi-github-copilot`、`pi-copilot`、`prime-agent-github-copilot`、`prime-agent-copilot`） | 订阅制 Copilot 路由的每 token 边际成本为 0 | `:18-23` |
| 服务端上报成本优先 | `SOURCES_WITH_AUTHORITATIVE_COST = new Set(["grok", "cline"])` | 命中且 `reportedCost > 0` 即直接采用 | `:32`、`:237-241` |

上报成本优先的判定是 **`> 0` 而不是 `>= 0`**：注释写明 `zero remains the legacy "unreported" sentinel`（`:234-235`），即 0 被视为「没上报」，会继续走模型定价。**这三条短路是「成本只由五桶计算」这一结论的第一个反例**：对 `grok`/`cline`，金额可以完全绕开价格表。

**四桶算式（唯一的计价主体）**：

```js
// src/lib/pricing/index.js:254-264
  const reasoningCost = reasoningIncludedInOutput ? 0 : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
  const baseCost = (
    ((row.input_tokens || 0) * (pricing.input || 0) +
      (row.output_tokens || 0) * (pricing.output || 0) +
      (row.cached_input_tokens || 0) * (pricing.cache_read || 0) +
      (row.cache_creation_input_tokens || 0) * (pricing.cache_write || 0) +
      reasoningCost) / 1_000_000
  );
```

即 `cost = ( input×p.input + output×p.output + cached×p.cache_read + cache_creation×p.cache_write + reasoning×p.output_or_0 ) / 1_000_000`。映射表（**源码唯一来源**）：`input_tokens` → `pricing.input`（`:258`）、`output_tokens` → `pricing.output`（`:259`）、`cached_input_tokens` → `pricing.cache_read`（`:260`）、`cache_creation_input_tokens` → `pricing.cache_write`（`:261`）、`reasoning_output_tokens` → `pricing.output`（条件性，`:256`）。三个必须同读的细节：**单位是 USD / 百万 token**，除数硬编码 `1_000_000`（`:263`），价格表内部一律以 MTok 计价（`src/lib/pricing/curated-overrides.json:4` 声明 `"units": "usd_per_million_tokens"`）；**缺失字段不报错、不推导、不插值**，一律 `(pricing.x || 0)` 落到 0——「价格表没写 `cache_write`」等价于「缓存写入免费」，这是理解整份价格表的关键；**不使用 `total_tokens`，也不使用 `billable_total_tokens`**（后者只服务展示层与热力图，`src/lib/local-api.js:284`、`:614`、`:2565`）。

**reasoning 门控：按 source 分支，共 5 个折叠源**。折叠集合（`reasoning ∈ output`，`reasoningCost` 恒为 0）：`codex`、`acode`、`every-code`、`omo`、`cline`（`src/lib/pricing/index.js:248-253`）；非折叠集合（`reasoning` 按 `pricing.output` 单价单独计费）：其余全部 source，含 `claude`、`gemini`、`cursor`、`opencode` 等（`:256`）。源码注释解释了三家折叠的理由（`OmO, like Codex, reports reasoning as a subset of output … charging it again here would double-bill every reasoning token. Cline is the same: its outputTokens is the AI SDK total (text + reasoning)`，`:249-252`）。**判断依据是 source 字符串，不是模型**，因此同一模型名走不同 source 会得到不同金额。测试锁定：`test/gpt-6-astra-pricing.test.js:44-48`（同一行 `source: "codex"` → 73.5、`every-code` → 73.5、`gemini` → **98.5**，差 25 = 50 万 reasoning × $50/MTok ÷ 1e6）、`test/pricing.test.js:1164-1183`、`:1111-1134`、`test/model-breakdown.test.js:427-446`。

另有一条容易误读的表述：**不存在「先并入 output 再二次计费」的路径**。`reasoningCost` 是 `baseCost` 的加项（`:262`），`output_tokens` 在 `:259` 只乘一次；全局搜索 `src/`，`reasoning_output_tokens` 在定价代码里只出现在 `:256`、`:299`、`:327`、`:348` 四处（全部是加项或子集标注）。「非折叠 = 重复计费」只在 provider 的 `output_tokens` 本身已含推理时才成立。

**长上下文溢价（OpenAI 白名单，>272K）**。白名单：`model === "gpt-5.6" || model.includes("gpt-5.6-sol") || model.includes("gpt-6-astra") || model.includes("gpt-6-sol") || model.includes("gpt-6.1-sol")`（`:267-269`）。要点：阈值常量在 `src/lib/pricing/index.js:31` 定义、`:375` 导出，**在本文件内从未参与计算**；真正判阈值的是解析期的另一份同名常量 `src/lib/codex-rollout-parser.js:51`，在 `:733` 使用（`Number(rawRequestUsage?.input_tokens || 0) > OPENAI_LONG_CONTEXT_INPUT_THRESHOLD`）——这是一个**双份常量、无测试守护**的漂移点（见 8.5）。溢价公式（`:300-306`）逐项为 `longInput×input + longCached×cache_read + longCacheWrite×cache_write + 0.5×longOutput×output + 0.5×longReasoning×output`，再 `/1e6`；最终 `baseCost + longContextPremium`（`:308`）。溢价读的是**子集列** `long_context_*`，五个子集列由解析器只对「原始（含缓存）输入 > 272K 的请求」累加（`src/lib/codex-rollout-parser.js:732-741`）；每个子集列都被 `bounded(value, total)` 夹在 `[0, 对应基础列]` 之间（`src/lib/pricing/index.js:277-296`）——**子集不可能大于全集，超报被截断而不是外推**；溢价**恰好叠加一次**；模型不在白名单则 `longContextPremium = 0` 并在 `:275` 提前 `return baseCost`。测试：`test/pricing.test.js:1136-1162`（Sol 长上下文只对观测子集生效）、`test/gpt-6-astra-pricing.test.js:50-62`（`large aggregate is not a long request`——100 万 token 的行聚合不触发长上下文）。

**priority（Fast）倍率：乘子不是替换**。进入条件是 `Number.isFinite(priorityMultiplier) && priorityMultiplier > 1`，且只有 `curated` 显式带该字段的条目才进入（`:273-275`）；实测只有 3 条：`gpt-6-astra` = 2、`gpt-6.1-sol` = 2、`gpt-6-sol` = 2。两段式溢价（`:328-354`）：

```js
// src/lib/pricing/index.js:328-354（节选）
  const priorityBase = ( priorityInput * (pricing.input || 0) + priorityCached * (pricing.cache_read || 0) +
    priorityCacheWrite * (pricing.cache_write || 0) + priorityOutput * (pricing.output || 0) +
    priorityReasoning * (pricing.output || 0) ) / 1_000_000;
  const priorityLong = usesOpenAILongContextTier ? ( bounded(row.priority_long_context_input_tokens, Math.min(priorityInput, longInput)) * (pricing.input || 0) + ... ) / 1_000_000 : 0;
  const priorityPremium = (priorityMultiplier - 1) * (priorityBase + priorityLong);
  return baseCost + longContextPremium + priorityPremium;
```

| 语义 | 说明 | 证据 |
| --- | --- | --- |
| 乘子只作用于**观测子集** | `priority_*` 是注解列，不是加数（注释原文 `They are annotations, never addends`） | `src/lib/pricing/index.js:281-285`、`:282-283` |
| 长上下文 × Fast 是**求交集**，不是两个溢价相加 | `priority_long_context_*` 列由解析器显式记录；缺交集列时回落到短上下文档乘子（`priorityLong = 0`） | `src/lib/codex-rollout-parser.js:742-762`；`src/lib/pricing/index.js:336-351`（注释 `:311-317`） |
| `service_tier` 标量、`-fast` 模型后缀**都不改价** | 只有列存在才改价 | `test/gpt-6-astra-pricing.test.js:115-122` |
| 交集列同样被 `Math.min` 夹紧 | 超报不抬价 | `src/lib/pricing/index.js:337-350`；`test/gpt-6-astra-pricing.test.js:153-159` |
| 无 `priority_multiplier` 的模型带 `priority_*` 列也不加价 | — | `test/gpt-6-astra-pricing.test.js:162-193` |

Fast 档的申报依据写在价格表注释里：`curated-overrides.json:98` 明确「The 2.5x figure in the Codex speed docs is a subscription-credit rate and must not be used here.」。`service_tier` 的来源是 Codex 的独立 rollout 记录，不在 `token_count` 行上（`src/lib/codex-service-tier.js:1-15`），观测值只有 `"default"` 与 `"priority"`，接受集合 `["priority","fast"]`（`:26`）；**未记录 tier 的请求一律按 Standard 计费，不猜测**（`:12-15`）。

**`getRowPricing`：DeepSeek 峰谷折扣的唯一入口**（`computeRowCost` 不直接调 `getModelPricing`，而是调它，`:242`）：

```js
// src/lib/pricing/index.js:209-222
function getRowPricing(row) {
  const pricing = getModelPricing(row?.model, { source: row?.source });
  // AStudio uses iFlytek MaaS fixed prices and does not inherit DeepSeek public API time-based discounts.
  if (String(row?.source || "").toLowerCase() === "acode") return pricing;
  if (!isDeepSeekTimePricedModel(row?.model) || !isDeepSeekOffPeak(row)) return pricing;
  return { ...pricing, input: (pricing.input || 0) * 0.5, output: (pricing.output || 0) * 0.5,
    cache_read: (pricing.cache_read || 0) * 0.5, cache_write: (pricing.cache_write || 0) * 0.5 };
}
```

折扣是**四个价格字段全部 ×0.5**（不区分桶），不是「折扣价单独一张表」；`acode`（AStudio / iFlytek MaaS）**显式豁免**（`:213`）。细节见 8.4。

**`total_tokens` 从不参与的源码证据**：在 `src/lib/pricing/` 目录下搜索 `total_tokens`，**全目录仅 1 次命中，且位于 `index.js:283` 的注释内**（该目录三个 JS 文件 `index.js`（378 行）、`matcher.js`（439 行）、`litellm-fetcher.js`（172 行）中仅此一处）；`index.js:236` 的 `const reportedCost = Number(row?.total_cost_usd);` 是金额字段，与 `total_tokens` 无关——两个标识符视觉相似，是唯一的「疑似命中」。反向验证：`computeRowCost` 的唯一计价分支是 `baseCost` 与两个溢价块，全部只读 `pricing.*` 与五个 token 列（`:257-354`）；`getRowPricing`（`:209-222`）只读 `model`/`source`/`pricing_tier`/`hour_start`。同一不变量在 `src/lib/cursor-config.js:528` 被再次书面确认。

### 8.2 价格表三层来源与优先级（curated 覆盖 84 条 / LiteLLM 运行期 / 内置 seed 快照）

| 层 | 文件 / 来源 | 载体 | 优先级 |
| --- | --- | --- | --- |
| L0 | 内置「策展表」 | `src/lib/pricing/curated-overrides.json`（29754 字节，775 行，84 条 `exact`） | **最高**，`require` 进内存，永不被网络覆盖 |
| L1 | LiteLLM 主表（**运行期**） | 磁盘缓存 `~/.tokentracker/cache/pricing.json` → 上游 URL → 陈旧缓存 → 内置 seed | 次之 |
| L2 | LiteLLM seed 快照 | `src/lib/pricing/seed-snapshot.json`（256869 字节，2284 条模型） | 最低（离线兜底） |

**没有 models.dev、没有 OpenRouter、没有用户可编辑的外置价格文件**（`src/` 全目录 0 命中；价格加载链只有上述三层）。L1 的唯一上游是 LiteLLM（`src/lib/pricing/litellm-fetcher.js:10-11`）。完整解析顺序以 8 步文档注释写在 `src/lib/pricing/matcher.js:3-12`（见 8.3）。

**L0：`curated-overrides.json` —— 覆盖而非补充**。顶层 5 个键：`_meta`、`exact`（84 条）、`alias`（1 条）、`source_exact`（1 个 source：`acode`，64 条）、`fuzzy`（47 条）。其 `_meta.note` 是全表最权威的自述：

> Curated price overrides. Always wins over LiteLLM. Two reasons to live here: (1) self-defined alias names that LiteLLM will never carry (kiro-*, hy3-*, composer-*, kimi-for-coding, free-tier OpenRouter routes); (2) prices we want to pin even if LiteLLM has the model (e.g. cache_write fields LiteLLM often omits). Units: USD per million tokens. Edit this file to override pricing without redeploying.
>
> —— `src/lib/pricing/curated-overrides.json:3`

```json
// src/lib/pricing/curated-overrides.json:92-99（节选）
    "gpt-6-astra": {
      "input": 10, "output": 50, "cache_read": 1, "cache_write": 12.5, "priority_multiplier": 2,
      "note": "Official Standard USD/MTok rates verified 2026-09-07: ... Requests above 272K input tokens use 2x input/cache and 1.5x output rates; computeRowCost applies this only to observed long-context request subsets. priority_multiplier is the Fast tier, whose published table ... is exactly 2x the Standard rate of the same context tier ... The 2.5x figure in the Codex speed docs is a subscription-credit rate and must not be used here."
    },
```

被手工修正的条目按 `note` 自述可归为四类：**手写别名**（LiteLLM 永不会有：`kiro-*`、`hy3-*`、`hy4-*`、`composer-*`、`kimi-for-coding`、`k3`、`k3-256k`、`cline-gateway-models`、全部 `*-free` 条目）；**钉住已有条目以消除缓存差异**（`claude-haiku-4-5` 与 `-20251001` 同价、`claude-sonnet-5` 不等 24h 刷新周期）；**补齐 `cache_write`**（LiteLLM 常缺：`claude-opus-5*`、`claude-sonnet-5*`、`claude-fable-5`、`gpt-6-*`、`gpt-5.6-*`、`deepseek-*`、`hy3/hy4`、`MiniMax-M2.7*`、`qwen3.8-flash`、`step-*`、`longcat-2.0`、`sakana/fugu-ultra`）；**按汇率从 RMB 换算**（`hy3/hy4` 约 `7.2 RMB/USD`、`longcat-2.0`、`step-*`、以及 `source_exact.acode` 全部 64 条，见 `:6` 与 `test/pricing.test.js:1313`）。

其余子结构的语义：`fuzzy` 是 `{ match, ref }` 数组，`ref` 必须是 `exact` 里存在的键（失效引用被跳过），**顺序即优先级**（第一个命中即返回，`src/lib/pricing/matcher.js:331-343`、`:335`）——顺序错误已被测试锁死（`test/edge-pricing-parity.test.js:159-160` 断言 `glm-5.3-flash` 必须先于 `glm-5.3`、`glm-5.3` 必须先于 `glm-5`，注释给出量化后果「or flash rows bill at 6.7x」，`:158`）；`source_exact` 是**按 source 分域的精确表**（只有 `acode`），命中即直接返回、查找大小写不敏感（`matcher.js:280-286`、`:282`）；`alias` 是字面映射（实测只有 `{"auto": "composer-1"}`），返回前校验 `curated.exact[alias[model]]` 存在（`matcher.js:316-322`）。代码里对 `curated.exact` 的硬依赖（改表会破坏这些路径）：`src/lib/pricing/index.js:363`（`const MODEL_PRICING = curatedOverrides.exact;`，向后兼容导出，注释 `:357-362` 说明它不含 LiteLLM 条目）与 `src/lib/pricing/matcher.js:291`（Qwen3.8 Flash 特例正则内直接检查 `curated.exact?.["qwen3.8-flash"]`）。

**L2：`seed-snapshot.json` —— 构建期抓取、四字段白名单**。来源是 LiteLLM 官方价目表，构建期抓取（`src/lib/pricing/litellm-fetcher.js:10-11` 的 `LITELLM_PRICING_URL = "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json"`）。生成脚本 `scripts/build-pricing-seed.cjs`（71 行），触发两处：`package.json` 的 `prepublishOnly` 与手工 `npm run pricing:build-seed`。每条记录**只保留 4 个价格字段**（脚本 `:14-19` 的 `KEEP_FIELDS` 白名单）：`input_cost_per_token`、`output_cost_per_token`、`cache_read_input_token_cost`、`cache_creation_input_token_cost`——上下文长度、模态、`input_cost_per_image` 等字段在生成期就被丢弃。

| 实测指标 | 值 |
| --- | --- |
| 字节数 / 顶层键 | 256869 / 2285（`_meta` 1 个、模型 2284 个） |
| `_meta` 原文 | `{"source":"https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json","generated_at":"2026-05-26T11:12:29.761Z","kept_models":2284}` |
| 字段直方图 | `input_cost_per_token` 2274、`output_cost_per_token` 2273、`cache_read_input_token_cost` **566**、`cache_creation_input_token_cost` **188** |
| 全部字段名集合 | 恰为上述 4 个（白名单生效） |

首 5 键 `sample_spec`、`ai21.j2-mid-v1`、`ai21.j2-ultra-v1`、`ai21.jamba-1-5-large-v1:0`、`ai21.jamba-1-5-mini-v1:0`；末 5 键为若干 `bedrock/*` 条目。`sample_spec` 是 LiteLLM 自带的样例条目，被原样保留。**LiteLLM 的字段单位是 USD / 单 token，不是 MTok**，内部换算在 `src/lib/pricing/matcher.js:398-414`：四个字段各自 `× 1e6` 映射为 `input` / `output` / `cache_read`（`cache_read_input_token_cost`）/ `cache_write`（`cache_creation_input_token_cost`），全部经 `roundToTenDecimals` 消除浮点漂移（`1e-7 * 1e6 = 0.09999999999999999`，`:391-396`）。**没有任何价格字段被推导**（例如没有 `read = input × 0.1`），缺就是缺，`buildLitellmPerMillionMap` 还会丢弃「一个价格字段都没有」的条目（`:418-427`）。**seed 加载是同步的、`require` 期就完成**（`src/lib/pricing/index.js:44-53`：`readFileSync` + `JSON.parse` + `delete parsed._meta`，异常则返回 `{}`）；动机见注释 `:40-43`（`Done at require-time so callers that haven't awaited ensurePricingLoaded() (e.g. tests, vite mock startup, edge functions) still get LiteLLM-backed pricing instead of all-zero.`）。读取后立刻剥 `_meta`（`:48`），运行期内存里的 raw map 不含它。

**L1：运行期 LiteLLM —— 24h 缓存 + 四级降级**。调用时机是**后台不阻塞启动**（`src/commands/serve.js:121-129`）：注释说明 seed 已在 `require` 期同步加载、成本计算当下就能工作，`ensurePricingLoaded()` 只负责升级到新鲜磁盘缓存或上游数据；若在此 `await`，离线/防火墙环境会阻塞启动整整 10 秒，`.catch` 只写一行 stdout 警告。TTL 与超时：`DEFAULT_TTL_MS = 24 * 60 * 60 * 1000`、`DEFAULT_FETCH_TIMEOUT_MS = 10_000`（`src/lib/pricing/litellm-fetcher.js:12-13`）。写盘前把上游条目裁成 4 个价格字段并写 `_meta`（`:68-105`、`:95-103`）；磁盘缓存的 `_meta.cached_at` 与 seed 的 `_meta.generated_at` 字段名不同（`scripts/build-pricing-seed.cjs:55`），读取时两者都被 `delete data._meta` 剥掉（`litellm-fetcher.js:132`、`:162`；`index.js:48`）。

| 顺序 | 降级链条件 | 记录的 `source` | 行号 |
| --- | --- | --- | --- |
| 1 | **新鲜磁盘缓存**（`mtime < ttl`） | `"disk-cache"` | `src/lib/pricing/litellm-fetcher.js:128-137` |
| 2 | 抓上游并写缓存 | `"upstream"` | `:139-146` |
| 3 | 网络失败时用**陈旧磁盘缓存** | `"stale-cache"` | `:148-158` |
| 4 | 兜底**内置 seed 快照** | `"seed-snapshot"` | `:160-163` |

`state.source` 被记录下来（`src/lib/pricing/index.js:83`），初始值是 `"seed-snapshot:sync"`（`:63`、`:103`）。四步都有测试：`test/pricing.test.js:508-582`（fresh cache 不触发 fetch / stale 触发 fetch / 无 cache 失败回落 seed / 有 stale cache 优先 stale 而非 seed）。**不存在**重试、ETag / If-None-Match、超时后的二次尝试——一次失败直接进下一步。加载本身是「一次性、幂等、可并发共享」的：`state.loaded` 短路、`state.loadingPromise` 共享（`src/lib/pricing/index.js:73-94`）、成功后 `revision += 1`、`negativeCache.clear()`（`:85-86`，字段声明 `:64-66`），并发只 fetch 一次（`test/pricing.test.js:1287-1308`）。

**价格来源的「反面清单」：没有环境变量、没有外置文件**。**`TOKENTRACKER_*` 定价相关环境变量：0 个**——对 `src/` 全目录搜索 `TOKENTRACKER_[A-Z_]+` 得到 250+ 命中，逐条核对后与定价相关的为 0；最接近的 `TOKENTRACKER_OPENCODE_GO_LOCAL_ESTIMATE`（`src/lib/opencode-go-limits.js:20`、`:51`）控制的是 opencode-go **额度**的本地估算口径，不参与 `computeRowCost`；其余命中全部是路径覆盖（`TOKENTRACKER_*_HOME`）、调试开关（`TOKENTRACKER_DEBUG`）、网络与同步（`TOKENTRACKER_INSFORGE_*`、`TOKENTRACKER_DEVICE_TOKEN`）、遥测（`TOKENTRACKER_NO_TELEMETRY`）之类。**可覆盖的接口只有编程接口**：`ensurePricingLoaded(opts)` 会把 `opts` 透传给 `loadLitellmData`，后者接受 `{ cachePath, ttlMs, fetchTimeoutMs, fetchImpl, url, logger }`（`src/lib/pricing/litellm-fetcher.js:112-119`）；**生产代码**对它只有两处调用：`src/commands/serve.js:127`（传 `cachePath`）与 `src/lib/local-api.js:3340`（不传参，走默认路径）。全仓口径拆分（供核对）：正则 `/ensurePricingLoaded\(/` 命中 **27 行** = 生产调用 **2** + 函数定义 **1**（`src/lib/pricing/index.js:73`）+ 注释 **4**（`index.js:4`/`:41`/`:42`、`src/commands/serve.js:123`）+ 测试调用 **20**（`test/pricing.test.js` 16 处：`:586`、`:602`、`:629`、`:755`、`:784`、`:819`、`:1049`、`:1063`、`:1083`、`:1097`、`:1114`、`:1167`、`:1297`、`:1298`、`:1299`、`:1306`；`test/local-api-queue-cache.test.js:149`/`:220`；`test/grok-parser.test.js:301`；`test/omo-parser.test.js:229`）；若用**裸标识符**口径（含 import/export 与测试名）则共 **36 行**，多出的 9 行是 `serve.js:9`、`local-api.js:61`/`:3531`、`index.js:97`/`:366`、`grok-parser.test.js:13`、`omo-parser.test.js:15`、`pricing.test.js:583`/`:1287`——因此「其余 20 行全部在测试里」是**测试调用行**口径，不等于「27 减 2”。

**配置文件覆盖：不存在**。仓库里没有 `pricing.toml` 之类的外置价格表；`~/.tokentracker/` 下**与价格有关**的派生物只有 `cache/pricing.json`。限定词「与价格有关」是必需的：该数据目录并非只有价格缓存——限额链另有多份磁盘缓存常量（`src/lib/usage-limits.js:70` `usage-limits-cache.json`、`:118` `claude-usage-limits-cache.json`、`:128` `codex-usage-limits-cache.json`、`:134` `opencode-go-usage-limits-cache.json`、`:141` `claude-usage-rate-limit.json`），其中两份的路径拼接为 `<home>/.tokentracker/tracker/<文件名>`（`:2219`、`:2299`），它们与定价无关。默认缓存路径的另一份定义在 `src/lib/pricing/index.js:69-71`，与 `resolveTrackerPaths()`（`src/lib/tracker-paths.js:4-12`：`rootDir = ~/.tokentracker`、`cacheDir = <rootDir>/cache`，`home` 只接显式传参）结果一致。想改价格只能改 `curated-overrides.json`（需重新发布或就地改源码）——这与该文件 `:3` 的「Edit this file to override pricing without redeploying」一致：改的是**仓库文件**，不是用户配置。

### 8.3 模型名匹配算法（0–7 级解析顺序、per-source 归一化、后缀剥离、前缀剥离、反向子串防误配）

**解析顺序（源码注释即权威规格）**：

```
// src/lib/pricing/matcher.js:3-12
// Resolve order:
//   0. CURATED source exact match (source-specific pricing)
//   1. CURATED exact match (self-defined aliases like kiro-*, hy3-*)
//   2. LiteLLM exact match (mainstream claude/gpt-5/gemini)
//   3. CURATED alias (e.g. "auto" -> "composer-1")
//   4. CURATED fuzzy substring (e.g. "kiro-future-xyz" matches via "kiro")
//   5. LiteLLM suffix-strip (gpt-5-codex-high-fast -> gpt-5-codex)
//   5b. LiteLLM provider-prefix strip (mimo-v2.5-pro -> openrouter/xiaomi/mimo-v2.5-pro)
//   6. LiteLLM reverse substring (longest-key first)
//   7. null  (caller decides what to do — typically zero-pricing + negative cache)
```

| 步骤 | 行号 | 返回的 `source` 标签 |
| --- | --- | --- |
| 空值守卫 | `src/lib/pricing/matcher.js:253-255` | `"empty"` |
| 早期 source 特例（WorkBuddy `auto` / Cline `:free` / AStudio `auto`） | `:265-279` | `"miss"` / `"curated:cline-free-suffix"` |
| 0 CURATED source exact | `:280-286` | `"curated:source-exact"` |
| 1 CURATED exact（含 Qwen 特例 + dot 形态） | `:291-300` | `"curated:exact"` / `"curated:exact-dot"` |
| 2 LiteLLM exact（含 dot 形态 + curated dot 包含匹配） | `:302-313` | `"litellm:exact"` / `"litellm:exact-dot"` / `"curated:exact-dot"` |
| 3 CURATED alias | `:315-322` | `"curated:alias"` |
| 4 CURATED fuzzy | `:331-343` | `"curated:fuzzy"` |
| 5 LiteLLM 后缀剥离 | `:345-351` | `"litellm:strip"` |
| 5b LiteLLM provider 前缀剥离 | `:361-370` | `"litellm:prefix-strip"` |
| 6 LiteLLM 反向子串（长键优先） | `:372-383` | `"litellm:fuzzy"` |
| 7 未命中 | `:385` | `"miss"` |

| 归一化规则 | 内容 | 证据 |
| --- | --- | --- |
| 大小写 | 主路径靠 `lookupExactCaseInsensitive`（先试 `table[model]`，否则把 key 逐个 `toLowerCase()` 线性比对）；子串路径先 `toLowerCase()` 再比。**没有预建小写索引**，未命中是 O(表大小) 扫描——这是 `negativeCache` 存在的理由 | `matcher.js:226-234`、`:259`、`:376-379`；`index.js:64-66`；`test/pricing.test.js:1094-1109` |
| `-` / `.` / `_` 互换 | **没有全局互换**。只有一条通用规则 `buildDotRestoredModel`：`lower.replace(/(\d+)-(\d+)/g, "$1.$2")`，即只把「数字-数字」还原成「数字.数字」（`glm-5-1-0` → `glm-5.1.0`），非数字段不动，`_` 完全不处理；还原结果与原文相同则返回空串 | `matcher.js:219-224`、`:324-330`；`test/pricing.test.js:191-205`、`:313-329`（明确列举 `claude-3-7-sonnet` / `gpt-5-codex` / `gemini-2-5-pro` 不受影响） |
| 日期后缀 | **没有通用的「去日期」步骤**。能命中是因为 LiteLLM 表里本身就并存带日期与不带日期的键（如 seed 中 `vertex_ai/claude-opus-4-6` 与 `vertex_ai/claude-opus-4-6@default`），以及 step 6 的反向子串；唯一针对日期的正则特例是 Qwen | `matcher.js:291`、`:379` |
| provider 前缀 | 两条独立机制：① Claude 专用归一化（先于所有查表执行）；② 通用 5b 前缀剥离。歧义处理**确定性**：多个 provider 同名时取**字典序最小**的键，与 JSON 顺序无关 | `matcher.js:89-95`、`:361-370`、注释 `:357-360`；`test/pricing.test.js:246-265` |
| `:free` 后缀 | **只对 `cline` source 生效**，且必须在 curated 查表之前（注释说明 source 专属价只作用于其 source，避免与同名模型的公共价碰撞） | `matcher.js:268-274`、注释 `:262-267` |
| `-latest` | **没有特殊处理**。`xai/grok-4-latest` 这类键在 LiteLLM 表里真实存在，靠 step 1/2 精确命中；不在表里则只退到 step 6 | `matcher.js`；seed 原文 |
| 推理强度后缀 | `SUFFIX_STRIP_PATTERNS` 九条正则（`-xhigh-fast`、`-high-fast`、`-medium-fast`、`-low-fast`、`-xhigh`、`-high`、`-medium`、`-low`、`-fast`），只用于 step 5，且**只在剥离后的键在 LiteLLM 表里存在时**才接受；顺序重要（`-xhigh-fast` 必须排在 `-fast` 前） | `matcher.js:14-31`、`:347-350`；`test/pricing.test.js:330-340` |

Qwen 日期正则（唯一）与 Claude 版本号重排：

```js
// src/lib/pricing/matcher.js:291
  if (/(?:^|\/)qwen3[.-]8-flash(?:-\d{4}-\d{2}-\d{2})?$/.test(lower.trim()) && curated.exact?.["qwen3.8-flash"]) {
    return { hit: true, source: "curated:exact", value: curated.exact["qwen3.8-flash"] };
  }
// src/lib/pricing/matcher.js:116-118
  if (/^claude-(?:[4-9]|\d{2,})[.-]\d+-(?:sonnet|opus|haiku)/.test(m)) {
    return m.replace(/^claude-(\d+)[.-](\d+)-(sonnet|opus|haiku)/, "claude-$3-$1-$2");
  }
```

即 `claude-4.6-opus` → `claude-opus-4-6`，而 3.x 的版本优先写法（`claude-3-5-sonnet`）**必须保持不动**（注释 `matcher.js:112-115`）。Claude 归一化还会先剥 provider 路径前缀（`anthropic/...`、`openrouter/anthropic/...`，注释说明否则 `/` 会被后续正则压成 `-` 而破坏匹配，`matcher.js:89-95`）。

**反向子串的防误配**（step 6，`model 包含 key`、最长键优先，`matcher.js:379`）靠两道防线：逐 source 的**哨兵归一化**（Unsloth 的 `local/`、`unpriced/` 前缀被映射到哨兵 `__tokentracker_unpriced_unsloth_model__`，防止本地模型被反向子串误配到云端 SKU，`matcher.js:182-189`）；以及**测试把「不得命中更粗基档」写成断言**（`test/pricing.test.js:146-190`、`:206-235`、`:330-363`）。

per-source 归一化表（**只作用于计价查找，原始模型名照样存储与展示**，`matcher.js:191-193` 注释原文 `Per-source model-name normalizers, applied at pricing-lookup time only (the raw model name is preserved for storage/display).`）：`acode: normalizeIFlytekMaasModel`、`antigravity: normalizeAntigravityModel`、`claude: normalizeClaudeModel`、`cursor: normalizeCursorModel`、`pi-anthropic` **与** `prime-agent-anthropic`: `normalizeClaudeModel`、`zed: normalizeZedModel`、`unsloth: normalizeUnslothModel`、`workbuddy: normalizeWorkbuddyModel`（`matcher.js:194-204`）。

| 归一化器 | 可验证要点 | 证据 |
| --- | --- | --- |
| `normalizeAntigravityModel` | 剥括号、剥 thinking/effort 词、把 Gemini 3.x 系列**特意降级映射到 2.5 档**（`gemini-3.x-pro` → `gemini-2.5-pro`）、`gpt-oss-120b*` → `antigravity-gpt-oss-120b`；「别名只对 antigravity 生效」被双重锁定（同一行 `gpt-oss-120b`，`source: "openrouter"` 得 0，`source: "antigravity"` 得 0.0125） | `matcher.js:33-58`（`:49-51`、`:55`）；`test/pricing.test.js:364-398`、`:1060-1078` |
| `normalizeCursorModel` | 按空格/符号切分后**只剔除** `thinking\|xhigh\|high\|medium\|low`，**`fast` 保留**（`fast` 可能是独立 SKU）；Grok 4.5 有专门 SKU 分流 | `matcher.js:126-150`（`:146`、`:138-140`） |
| `normalizeZedModel` | **不剥 `fast`**（`grok-code-fast-1` 里 `fast` 是名字的一部分），并把 Claude 的点号 minor 连字符化（`claude-opus-4.8` → `claude-opus-4-8`） | `matcher.js:68-81`、注释 `:64-67` |
| `normalizeWorkbuddyModel` | `auto` → `hy3-preview-agent`，且**必须与 Cursor 的 `auto`（→ `composer-1`）区分**；另有一道守卫：source 为 `workbuddy-ai` 且模型是 `auto` 时直接返回 `miss` | `matcher.js:161-166`、`:265-267`；`test/pricing.test.js:1320-1347`、`test/edge-pricing-parity.test.js:374-385` |
| `normalizeUnslothModel` | `local/`、`unpriced/` 前缀 → 哨兵 `__tokentracker_unpriced_unsloth_model__` | `matcher.js:182-189` |

**未命中行为与三态语义**。`lookupPricing` 的返回值恒为三字段对象：命中 `{ hit: true, source: "<标签>", value: {...} }`；空模型名 `{ hit: false, source: "empty", value: null }`；未命中 `{ hit: false, source: "miss", value: null }`（`matcher.js:253-255`、`:385`）。`getModelPricing` 未命中时写入 `negativeCache` 并返回共享常量 `ZERO_PRICING`（`src/lib/pricing/index.js:112-132`、`:17`），负缓存键是 `` `${lookupSource}\0${model}` ``（`:120`）——**按 source 分域**，因为同一模型名在不同 source 下可能一个命中一个未命中；`getModelPricingInfo`（`:157-176`）把三态显式化：`unpriced`（表里没有）/ `free`（表里有且四价全 0）/ `priced`（至少一个价 > 0），本地推理与 Pi 订阅源直接返回 `{status:"free", source:"local_inference"|"subscription"}`（`:158-160`）。结果按 pricing revision 记忆化并在返回时浅拷贝（`:137-155`，注释 `:134-136`：`Copies go out so a caller that mutates its object cannot poison the cache.`）。前端据此显示不同文案（`dashboard/src/pages/DashboardPage.jsx:1253-1259`、`dashboard/src/ui/dashboard/components/CostAnalysisModal.jsx:173-183`）。云端也有同样约束并被测试守死（`test/edge-pricing-parity.test.js:223-233` 断言所有 edge 文件都不得包含 `getModelDisplayName` / `IFLYTEK_MAAS_MODEL_NAMES`）。

**输入侧边界：队列行没有上下文子集时交集为 0**。匹配算法只决定「用哪张价」，**改价的是列而非名字**。两类随行数据的边界必须并读：① **`long_context_*` 子集列缺席**（队列行、云端行常见）→ `longContextPremium = 0`，即使模型在白名单内也按短上下文档计价（`test/gpt-6-astra-pricing.test.js:50-62` 明确断言「100 万 token 的行聚合不触发长上下文」）；② **`priority_long_context_*` 交集列缺席** → `priorityLong = 0`，长上下文 × Fast 的溢价回落到短上下文档乘子（`src/lib/pricing/index.js:336-351`，注释 `:311-317`）——这不是零成本，而是**交集项恰为 0** 的部分计价。两条共同解释了为什么「先汇总 token 再计价一次」会静默丢钱：`priority_*` / `long_context_*` 标记的是基础列的子集，**只有行完整时才存活**（`src/lib/local-api.js:2634-2640`）。

### 8.4 特殊计价

**DeepSeek 峰谷：5 折 + 北京周末规则**。模型白名单是硬编码 4 个：`deepseek-v4-flash`、`deepseek-v4.1-flash`、`deepseek-flash`、`deepseek-v4-pro`（`src/lib/pricing/index.js:34-39`），匹配方式 `lower.includes(name)`（`:180`）。峰谷判定（`:199-207`）优先级：**显式 `pricing_tier` > 北京周末规则 > UTC 峰段判定 > 默认峰值**（时间戳非法时返回 `false` = 按峰值算，**fail closed**）。峰段窗口是 UTC **01:00–04:00 与 06:00–10:00**（`(hour >= 1 && hour < 4) || (hour >= 6 && hour < 10)`）。

```js
// src/lib/pricing/index.js:186-197（节选）
// From 00:00 Beijing time on 2026-08-23 — this instant — DeepSeek bills whole
// Beijing weekends off-peak, peak hours included.
// https://api-docs.deepseek.com/quick_start/pricing/
const DEEPSEEK_WEEKEND_OFF_PEAK_FROM_MS = Date.UTC(2026, 7, 22, 16, 0, 0);
function isBeijingWeekendOffPeak(timestamp) {
  if (timestamp < DEEPSEEK_WEEKEND_OFF_PEAK_FROM_MS) return false;
  const beijingDay = new Date(timestamp + 8 * 60 * 60 * 1000).getUTCDay();
  return beijingDay === 0 || beijingDay === 6;
}
```

生效起点 `Date.UTC(2026, 7, 22, 16, 0, 0)` = `2026-08-22T16:00Z` = 北京时间 `2026-08-23 00:00`；折算是「+08:00 固定偏移」（注释 `:188-192` 解释了为何不能直接对原始瞬时取 `getUTCDay()`）。**价格表里存的是峰值价，折扣由代码施加**（`curated-overrides.json:5` 的 `deepseek_v4_pricing_note`：`Exact entries store peak rates. computeRowCost applies the time-of-use discount from each row's UTC hour_start.`），幅度是四个价格字段各 ×0.5（`src/lib/pricing/index.js:215-221`）。测试：`test/pricing.test.js:891-983`（11 个纯时间点断言 `:917-921`、`:940`、`:945`、`:950`、`:955`、`:960`、`:965`，另有 3 条显式 `pricing_tier` 断言与 1 条非法时间戳断言，含「非法时间戳 fail closed 到峰值」「显式 tier 覆盖周末规则」「legacy id 保持静态价」）与 `:985-1016`（**展示型模型名不触发折扣**）、`:1349+`（v4.1 Flash）。

**OpenAI 长上下文：272K 阈值，2× 输入 / 1.5× 输出**。

| 项 | 值 | 证据 |
| --- | --- | --- |
| 阈值 | `272_000` 输入 token（单请求、含缓存的原始输入） | `src/lib/codex-rollout-parser.js:51`、`:733`；`src/lib/pricing/index.js:31` |
| 适用模型 | `gpt-5.6`、`gpt-5.6-sol*`、`gpt-6-astra*`、`gpt-6-sol*`、`gpt-6.1-sol*`（硬编码白名单） | `src/lib/pricing/index.js:267-269` |
| 倍率 | 输入与 `cache_read` 2×、`cache_write` 2×、输出与推理 1.5×（公式中的 `0.5×` 加项） | `src/lib/pricing/index.js:300-306` |
| 作用范围 | **只对观测到的请求子集加价**，不做整桶外推 | `src/lib/pricing/index.js:286-289`、`:277-296`；`test/gpt-6-astra-pricing.test.js:50-62` |

**不存在**通用的「>200K 输入整体换价目表」机制。阈值是 272K，且只属于上述白名单模型；其他模型无论输入多长都按同一单价算。

**priority / Fast 倍率**见 8.1 末段：只有 3 条 curated 条目带 `priority_multiplier = 2`；乘子只作用于 `priority_*` 观测子集；长上下文 × Fast 是**求交集后相乘**（`test/gpt-6-astra-pricing.test.js:138-160` 显式断言 `notEqual(1.55*2 + (2.85-1.55))`）；`service_tier` 与 `-fast` 后缀都不改价；无 `priority_multiplier` 的模型带 `priority_*` 列也不加价。

**Grok / Cline：权威成本优先于价格表**。`SOURCES_WITH_AUTHORITATIVE_COST = new Set(["grok", "cline"])`（`src/lib/pricing/index.js:32`）；命中且 `row.total_cost_usd > 0` 时**直接返回上报价**（`:236-241`），完全绕开价格表；`0` 是 legacy「未上报」哨兵，会继续走模型定价（注释 `:234-235`）。测试：`test/pricing.test.js:1185-1197`（`0.130486`）、`:1199-1216`（`0.25`）、`:1230-1246`（非上报时与非权威来源结果相等）。聚合层有对应标签机制：`cost_source` 在 `provider_reported` / `model_pricing` / `mixed` 三态间切换（`src/lib/local-api.js:2624-2626`），且**服务端自报成本在聚合后必须存活**（`test/model-breakdown.test.js:835-888`，注释记录过一次「汇总对象上不存在 `total_cost_usd` / `usage_precision`，导致 Grok 上报价在列表里显示 $0.00」的真实回归）。另有一条同族但方向相反的规则：**Command Code 用表价、忽略 CLI 显示估算**（`test/pricing.test.js:1248-1262`，3 模型 × 3 种 `total_cost_usd`（含 999）结果恒定）。

**LM Studio / Pi / Cline 网关：零边际成本**。

| 来源 | 判据 | 证据 |
| --- | --- | --- |
| LM Studio（本地推理） | `LOCAL_INFERENCE_SOURCES = ["lmstudio"]` → 0 | `src/lib/pricing/index.js:27`、`:228` |
| Pi 订阅路由 | `PI_SUBSCRIPTION_SOURCES`（4 个成员）→ 0；**token 仍保留**（只是不产生金额） | `:18-23`、`:232`；`test/pricing.test.js:1218-1228`、`:1264-1278` |
| Cline 网关命名空间 | `cline-free/*`（免费层）与 `cline-pass/*`（Cline Pass 包月）在 `curated` 里显式记 0，采用与 Pi 订阅相同的约定 | `curated-overrides.json:43-49` |
| Cline `:free` 后缀 | 只对 `cline` source 的 `:free` 后缀返回全 0 | `src/lib/pricing/matcher.js:268-274` |

**明确不存在的档位（否定结论）**。

| 档位 | 结论 | 证据 |
| --- | --- | --- |
| 批处理折扣 | **不存在**。`src/` 全目录搜 `batch` 命中 37 处，全部是上传/同步的批处理或注释字样；`computeRowCost` 无 `batch` 维度，价格表也无 `batch_*` 字段 | `src/lib/upload-throttle.js:5`、`src/commands/sync.js:4315` 等 |
| cache 5m / 1h 差异 | **不存在**。价格表只有一个 `cache_write` 字段，没有任何字段/参数/分支区分 5 分钟与 1 小时缓存写入 | `src/lib/pricing/matcher.js:410-412` |
| 图片 / 音频独立计费 | **不存在**。队列行的 token 桶固定为五列；`src/` 搜 `audio_token` / `image_token` / `image_generation` **0 命中**；LiteLLM 里的音频/图像条目只被当作普通输入/输出单价收录 | `src/lib/pricing/index.js:257-262`；`scripts/build-pricing-seed.cjs:14-19` |
| 订阅节省估算 | **不存在**。`src/lib/subscriptions.js`（508 行）无任何金额比较；`savings` 命中只有 Cloud 年付相对月付的折扣百分比 | `dashboard/src/pages/CloudPage.jsx:95` |
| 额度限额与成本换算 | **无关**。`src/lib/usage-limits.js`（4500+ 行）的 `computeRowCost` 命中为 0，量纲是百分比与剩余额度 | `src/lib/usage-limits.js:165`、`:1910-1911` |

**其他金额消费点与前端**。`wrapped` 聚合器（`src/lib/wrapped-aggregator.js`，236 行）**不含任何金额**——`totals` 只有 `tokens / conversations / sources / models / active_days`（`:212-218`），文件内无 `cost` 字样、无 `require("./pricing")`。`quality-per-dollar` 逐行 `computeRowCost(row) || 0`（`src/lib/outcomes-engine.js:24`、`:207`），但 `:208` 的 token 数用 `row.total_tokens`——**钱用 `computeRowCost`、token 数用 `total_tokens`**，这是「绝不用 total_tokens」规则的适用边界示例（它约束的是成本，不是计数）；文档原文 `Costs come entirely from TokenTracker's existing pricing. The sidecar only supplies the accepted/total counts.`（`docs/quality-per-dollar.md:18`），派生量见 `src/lib/outcomes-engine.js:154`（`quality_per_dollar`）、`:156-157`（`effective_tokens` / `effective_cost_usd`）。技能用量在 `src/lib/local-api.js:3378-3382`（`:3381` 为累加行）；会话侧 `repriceSessionRecord` 逐 bucket / 逐 model_usage 行计价（`src/lib/session-analytics.js:269-308`，`:272`、`:279`、`:302`），会话聚合对已算好的 `cost_usd` 求和（`:2414`、`:2695`）；云端排行榜逐行计价后取整到**分**（`tokentracker-leaderboard-refresh.ts:1181`、`:1349`）。`dashboard/src/` 下 `MODEL_PRICING` / `computeRowCost` / `getModelPricing` **0 命中**：前端只做求和与格式化（`dashboard/src/lib/model-breakdown.ts:116-117`、`:179`、`:217`；`dashboard/src/lib/sessions-insights.js:19-30`），唯一例外是 mock 数据（`dashboard/src/lib/mock-data.ts:391`）与 dev/mock 通道（`dashboard/vite.config.js:508-514`、`:1052-1057`、`:1067` require 同一 pricing 模块），不参与真实路径。云端 `tokentracker-account-model-breakdown.ts:906` 用 `ma.totalCostUsd += subscriptionBacked ? 0 : ...` 确保订阅行不进金额；排行榜展示带免责声明（`dashboard/src/pages/LeaderboardPage.jsx:817` 的 `Based on estimated API pricing, not actual billing`，另见 `:927`、`:978`）。

### 8.5 缺口与风险

**缓存价缺失的实测分布（最直接的口径后果）**。

| 表 | 条目数 | 有 `cache_read` | 有 `cache_write` |
| --- | --- | --- | --- |
| `curated-overrides.json` 的 `exact` | 84 | 84（缺 0） | **43（缺 41）** |
| `seed-snapshot.json` | 2284 | 566 | **188** |

`curated` 缺 `cache_write` 的 41 条几乎全是 GLM 系、Kimi 系、Composer 系、Grok 系与 `*-free` 条目。LiteLLM 侧换算成比例：**约 75% 的模型没有缓存读价、约 92% 没有缓存写价**。直接推论：**走 LiteLLM 路径的模型，只要 `cached_input_tokens` 非零而该模型没有 `cache_read_input_token_cost`，这部分缓存读取就会被算成 $0**（源于 8.1 的 `(pricing.cache_read || 0)` 与「不做推导」两条）。`curated-overrides.json:3` 把「cache_write fields LiteLLM often omits」列为该文件存在的第二个理由，与此吻合。这条语义曾被当成 bug 修过：`test/edge-pricing-parity.test.js:87` 的测试名直说 `Qwen3.8 Flash local and all cloud paths share full cache pricing (#715)`，断言 `Math.abs(cost - 0.836) < 1e-12`（四个桶各 100 万 token，`0.15 + 0.47 + 0.016 + 0.2 = 0.836` ✓）；同一文件还断言不同 SKU 不得共用这张价（`:103-105`）。缓存写的丢失也可以发生在**解析/差分层**：`test/cache-token-accounting.test.js:9-13` 的注释即记录「归一化或差分丢掉 `cache_creation_input_tokens` 会静默低报缓存写」的已修 bug（`:45-65` 为对应断言）。

**`unpriced` 与「$0 的两种含义」**。`computeRowCost` 返回 `0` 时无法区分「未定价」与「免费」，必须由 `getModelPricingInfo` 的三态补足：`unpriced`（`!result.hit`，价格表里没有这个模型——**不能当作 0 元**）/ `free`（命中且四价全 0，确实无每 token 边际成本，如本地推理、订阅路由、免费网关）/ `priced`（命中且至少一个价 > 0）——判据实现见 `src/lib/pricing/index.js:157-176`。**未命中会进负缓存**，所以「表加载后新出现的模型」只有在 `revision` 变化（清空 `negativeCache`）之后才会被重新求值（`:86`、`:120`）。

**公有云第二张价格表：9 个键的分歧**。云端 5 份 edge 文件各自**手抄**一份价格表，要求逐字节一致：`CANONICAL = "tokentracker-leaderboard-refresh.ts"`，`MIRRORS` 为 `tokentracker-account-daily.ts`、`tokentracker-account-summary.ts`、`tokentracker-account-model-breakdown.ts`、`tokentracker-leaderboard-profile.ts`（`test/edge-pricing-parity.test.js:25-31`，路径前缀 `dashboard/edge-patches/`，`:23`）。该目录下共 **28** 个 `.ts` 文件，**除这 5 份外另有 23 个**，**其中只有这 5 份内嵌价格表**；`tokentracker-account-hourly.ts` / `-monthly.ts` 出现在 `MODEL_API_CALL_SITES`（`:43-44`）只是因为它们要原样透传模型 ID。当前修订下五份的抽取块**完全一致**（块 394 行、`sha256[:16]` 均为 `37840dc53f7b6e2f`），块内 `MODEL_PRICING` **119 键**（`tokentracker-leaderboard-refresh.ts:142-332`），另有独立常量 `IFLYTEK_MAAS_MODEL_PRICING` **64 键**（`:339-404`），两者相加 183。`getRowPricing` 是**第二块**被要求逐字一致的手抄块（`test/edge-pricing-parity.test.js:403-413`，注释：`Substring checks alone let one file's window drift while every assertion still passes.`）。设计自述也是唯一的动机记录（`test/edge-pricing-parity.test.js:3-12` 节选）：这些副本**必须字节一致**，因为漂移会让同一行在不同端点得到不同价格——**真实事故**是 `leaderboard-profile` 直到 2026-06 都缺 `kimi-k2.6` 与 `mimo-v2.5` 家族，导致 profile 弹窗对这两个模型显示 `$0` / `-60%`，而排行榜列表计价正确。

两向差集（**分歧实态，本次研究最值得上报的一条**）：

| 方向 | 数量 | 内容 |
| --- | --- | --- |
| 在 `curated.exact` 但不在 edge `MODEL_PRICING` | **9 键** | `claude-opus-5.5`、`claude-sonnet-5.5`、`cline-gateway-models`、`claude-opus-4-8`、`claude-haiku-4-5`、`antigravity-gpt-oss-120b`、`grok-4.5-build`、`grok-build-free`、`grok-4.5-build-free` |
| 在 edge `MODEL_PRICING` 但不在 `curated.exact` | **44 键** | 全部主流条目（`claude-opus-4-6`、`gpt-5.4`、`gpt-5.5`、`gemini-2.5-pro`、`gemini-3-pro-preview`、`gpt-5-codex`、`mimo-v2.5*` ……）；**iFlytek MaaS 的 64 条不计入**（被拆到独立常量） |

9 个缺口里多数被云端「更粗的匹配器」意外兜住，但有两处是**真实价格分歧**：

| 缺失键 | 本地（curated） | 云端 edge 实际走向 | 是否等价 |
| --- | --- | --- | --- |
| `claude-opus-5.5` / `claude-sonnet-5.5` | 4/20/0.2/5、2/10/0.1/2.5 | 正则 `/opus-5[.-]5(?:$\|[^0-9.])/` 等显式覆盖（edge `:436-437`） | 等价 |
| `claude-haiku-4-5` | 1/5/0.1/1.25 | `lower.includes("haiku")` → `claude-haiku-4-5-20251001`（edge `:442`） | 等价 |
| `claude-opus-4-8` | 5/25/0.5/6.25 | `lower.includes("opus")` → `claude-opus-4-6`（edge `:441`），同值 | 数值巧合下等价 |
| `cline-gateway-models` | 0/0/0/0 | 内联 `cline-free/`、`cline-pass/` 判断（edge `:432`） | 等价（实现方式不同） |
| `grok-4.5-build` | 2/6/0.5/0 | `lower.includes("grok-4.5")` → `cursor-grok-4.5`（edge `:480`），同值 | 数值巧合下等价 |
| **`grok-build-free`** | **0/0/0/0** | `lower.includes("grok-build")` → `grok-build` = `{input:1.25, output:2.5, cache_read:0.2}`（edge `:273`、`:481`） | **不等价** |
| **`grok-4.5-build-free`** | **0/0/0/0** | `lower.includes("grok-4.5")` → `cursor-grok-4.5` = 2/6/0.5 | **不等价** |
| `antigravity-gpt-oss-120b` | 2.5/10/0 | edge 无 antigravity 归一化，`gpt-oss-120b` 不含任何匹配片段 → `ZERO_PRICING`（edge `:534`） | 不等价（但 antigravity 是否走云端未知） |

`grok-build-free` 的差距可以逐字对照：本地断言 `getModelPricing("grok-build-free", { source: "grok" }).input === 0`（`test/grok-parser.test.js:302`）且 `computeRowCost(rows[0]) === 0`（`:342-343`）；云端 `tokentracker-leaderboard-refresh.ts:273` 的 `"grok-build": { input: 1.25, output: 2.50, cache_read: 0.20 }` 加 `:481` 的 `if (lower.includes("grok-build")) return MODEL_PRICING["grok-build"];`。**量化：100 万 input token → 本地 $0，云端 $1.25。** 显现前提是该行 `total_cost_usd <= 0` 或缺省——`grok` 在权威成本集合里（本地 `src/lib/pricing/index.js:32` / 云端 `tokentracker-leaderboard-refresh.ts:8`），上报价为正时两端都直接采用上报价。**parity 测试为何没抓到**：整块字节比对只做 **edge↔edge**（`test/edge-pricing-parity.test.js:76-85`）。与 `curated-overrides.json` 的对照**只有 4 处**且全在同文件内：`:170-172`（iFlytek MaaS 全表与 `curatedPricing.source_exact.acode` `deepEqual`）、`:175`（`source_alias?.acode === undefined`）、`:208`、`:209`（`xsparkx2` / `xopglm53` 取值 deepEqual）。Qwen 用例（`:87-107`）比的是**硬编码字面量**（`:89`），`test/model-breakdown.test.js:480-503` 比的是**本地↔edge** 的 Kiro 逐字节等价，`test/grok-parser.test.js:302`/`:342-343` 只断言本地侧。**没有任何测试断言「edge 表 ⊇ curated.exact」或「逐键价格相等」——这是漂移的实际入口。**

**本地与云端的能力不对等（另三处）**。

| 项 | 本地 | 云端 edge | 证据 |
| --- | --- | --- | --- |
| 长上下文 / Fast 溢价 | 有 `long_context_*` 与 `priority_*` 分支 | **没有**，只有纯四桶 + 条件性推理折叠 | `tokentracker-leaderboard-refresh.ts:569-613`；本地 `src/lib/pricing/index.js:267-354` |
| Pi 订阅成员 | 4 个（含 `prime-agent-*`） | **2 个**（`pi-github-copilot`、`pi-copilot`）；`prime-agent-*` 两行在云端**不会**短路为 0，会落到模型定价 | `src/lib/pricing/index.js:18-23` vs `tokentracker-leaderboard-refresh.ts:575`；parity 测试 `:250-269` 也只断言那两个字符串 |
| 金额精度 | 日聚合内部浮点原值；`usage-summary` 与模型分解 `toFixed(6)`（字符串）；会话侧浮点求和 | 取整到**分** | `src/lib/local-api.js:2415`、`:2648`；`src/lib/session-analytics.js:289`；`tokentracker-leaderboard-refresh.ts:1349` |

长上下文/Fast 的差异是**被文档化的有意设计**：`curated-overrides.json:98` 写明 `Cloud aggregates use Standard short-context estimates because they do not retain per-request context or service tier.` 但没有测试断言「云端结果必然 ≤ 本地结果」或给出量化允许差。跨端对账必须按端点选择比较口径。

**无测试守护的三处不一致**。

| # | 不一致 | 两侧口径 | 证据 |
| --- | --- | --- | --- |
| 1 | `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 双份定义 | `src/lib/pricing/index.js:31` 定义、`:375` 导出、**全文零内部读取**；生效者是 `src/lib/codex-rollout-parser.js:51` → `:733`。改一处不改另一处不会有测试失败。`src/lib/session-analytics.js:245` 的注释还把这个常量当作权威，实际只被注释引用 | 同上 |
| 2 | 推理折叠名单两套 | 计价侧 5 个（`codex`/`acode`/`every-code`/`omo`/`cline`，`src/lib/pricing/index.js:248-253`）；token 审计侧 2 个（`codex`/`omo`，`scripts/audit-token-correctness.cjs:34-40`）。两侧各有理由，但**无任何测试把两者绑定** | 同上 |
| 3 | `PI_SUBSCRIPTION_SOURCES` 成员数本地 4 / 云端 2 | 见上表 | `src/lib/pricing/index.js:18-23` vs `tokentracker-leaderboard-refresh.ts:575` |

顺带说明审计脚本的边界：`audit:tokens`（`scripts/audit-token-correctness.cjs`，176 行）**只审计 token、不审计金额**——全文无 `pricing` / `cost` / `computeRowCost`；它按 `(source, model, hour_start)` 去重取最新行（`:54-60`）、对 legacy codex 行减去 cached（`:42-50`、`:94`）、算 `baseParts = input + output + cacheRead + cacheWrite`（`:95`）、按 `isReasoningInformational(source)` 决定推理是否进 `sumParts`（`:34-40`、`:96-99`）、`diff = total - sumParts` 非零即 `invariant_failures`（`:100`、`:117-129`）。对应用例 `test/token-audit.test.js` 4 条，**四条全部与金额无关**。

**本节未闭合项（供第 12 章汇总）**。

| # | 项 | 现状 | 判定条件 |
| --- | --- | --- | --- |
| 1 | LiteLLM 上游价的时效性 | `seed-snapshot.json` 的 `_meta.generated_at` 是 `2026-05-26T11:12:29.761Z`，而运行期磁盘缓存 TTL 是 24 小时——**离线/首启状态下使用的价格表年龄可能远大于 24h**。上游仓库当前是否仍以 `model_prices_and_context_window.json` 为唯一价目文件、字段名是否仍为那 4 个成本字段 `【未能确认】` | 联网核对上游仓库 |
| 2 | `curated-overrides.json` 注释引用的第三方定价页与表内数字是否一致 | `【未能确认】`（该表运行在 2026 年时间线，`claude-opus-5-5`、`gpt-6-astra`、`deepseek-v4` 等不存在可核对的外部真实价页） | 无外部事实源 |
| 3 | edge 五个文件的**部署状态** | `dashboard/edge-patches/` 是「待部署的补丁源」，实际线上函数是否为同一修订 `【未能确认】`（`CLAUDE.md:42` 提到「Deploy the touched edge functions after editing」，说明部署是人工步骤） | 需线上函数产物或版本指针 |
| 4 | 限额磁盘缓存中另外三份的落盘目录 | 部分结案：`usage-limits-cache.json` 与 `claude-usage-limits-cache.json` 的路径拼接点已读实（`<home>/.tokentracker/tracker/`，`src/lib/usage-limits.js:2219`、`:2299`）；codex / opencode-go / rate-limit 三份同形态常量未逐个核拼接点 | 读 `usage-limits.js` 对应常量的使用点 |

### 8.6 手工演算示例（用已复核的真实价格做两段演算）

统一用一组 token：`input_tokens = 10000`、`cached_input_tokens = 200000`、`output_tokens = 5000`、`cache_creation_input_tokens = 1500`。算式骨架（`src/lib/pricing/index.js:257-264`）：

```
cost = ( input×p.input + output×p.output + cached×p.cache_read
         + cache_creation×p.cache_write + reasoning×p.output_or_0 ) / 1_000_000
```

```json
// src/lib/pricing/seed-snapshot.json
"claude-opus-4-6":{"input_cost_per_token":0.000005,"output_cost_per_token":0.000025,"cache_read_input_token_cost":5e-7,"cache_creation_input_token_cost":0.00000625}
"gpt-5.4":{"input_cost_per_token":0.0000025,"output_cost_per_token":0.000015,"cache_read_input_token_cost":2.5e-7}
"gemini-3-pro-preview":{"input_cost_per_token":0.000002,"output_cost_per_token":0.000012,"cache_read_input_token_cost":2e-7}
```

换算后的内部单位（USD/MTok，×1e6）：

| 模型 | input | output | cache_read | cache_write | 命中路径 |
| --- | --- | --- | --- | --- | --- |
| `claude-opus-4-6` | 5 | 25 | 0.5 | 6.25 | `litellm:exact`（`matcher.js:303`）；`curated.exact` 中**缺席**（实测） |
| `gpt-5.4` | 2.5 | 15 | 0.25 | **缺失 → 计 0** | `litellm:exact`；云端手抄表同值但同样无 `cache_write` |
| `gemini-3-pro-preview` | 2 | 12 | 0.2 | **缺失 → 计 0** | 同上 |

**演算 A：`claude-opus-4-6`，`source = "claude"`，`reasoning_output_tokens = 0`**。`reasoningIncludedInOutput`：`claude` 不在五个折叠 source 中（`src/lib/pricing/index.js:248-253`）→ `false`，故 `reasoningCost = 0 × 25 = 0`。`usesOpenAILongContextTier`：`claude-opus-4-6` 不满足 `:268-269` 任一条件 → 在 `:275` `return baseCost`（不再进入 Fast / 长上下文分支）。

```
input          10000 × 5     =  50 000
output          5000 × 25    = 125 000
cache_read    200000 × 0.5   = 100 000
cache_write     1500 × 6.25  =   9 375
reasoning          0         =       0
                       合计    = 284 375
cost = 284 375 / 1_000_000    = 0.284375 USD
```

**演算 B：`gpt-5.4`，`source = "codex"`，`reasoning_output_tokens = 0`**。`reasoningIncludedInOutput`：`source === "codex"` → `true`（`src/lib/pricing/index.js:249`）→ `reasoningCost = 0`。`pricing.cache_write` 缺失 → `(undefined || 0) = 0`。`usesOpenAILongContextTier`：`gpt-5.4` 不在白名单（`:268-269`）→ `:275` 提前返回。

```
input          10000 × 2.5   =  25 000
output          5000 × 15    =  75 000
cache_read    200000 × 0.25  =  50 000
cache_write     1500 × 0     =       0   ← LiteLLM 无 cache_creation 价
reasoning          0         =       0
                       合计    = 150 000
cost = 150 000 / 1_000_000    = 0.150000 USD
```

同一行在云端得到完全相同的 `0.15`：`tokentracker-leaderboard-refresh.ts:455` 命中 `gpt-5.4` 表项（值同为 2.5/15/0.25），`:605-612` 的算式与本地一致。**对照（推理桶的影响）**：把演算 B 的 source 改成 `gemini`（非折叠），`reasoning = 0` 时结果仍是 `0.15`；若 `reasoning_output_tokens = 800` 且该 provider 的 `output_tokens` 已含这 800，本地会多收 `800 × 15 / 1e6 = 0.012`，即 `0.162`。同理，`gemini-3-pro-preview`（2/12/0.2）在同样 token 下的基准是 `0.12`，加 800 推理后为 `0.1296`（`800 × 12 / 1e6 = 0.0096`）。这正是 `test/gpt-6-astra-pricing.test.js:44-48` 用 `73.5` vs `98.5` 锁定的那类差异。

**三个测试原文锚点（供交叉核对）**。

| 场景 | 期望值 | 手工核对 |
| --- | --- | --- |
| Qwen3.8 Flash 四桶各 100 万（`test/edge-pricing-parity.test.js:89`、`:99-101`） | `0.836` | `0.15 + 0.47 + 0.016 + 0.2 = 0.836` ✓ |
| GPT-6 Astra 四桶各 100 万 + 推理 50 万（Codex，`test/gpt-6-astra-pricing.test.js:45`） | `73.5` | `10 + 50 + 1 + 12.5 + 0 = 73.5` ✓（推理折叠，不加项） |
| Command Code 小请求（`input=100, output=20, cached=600, cache_creation=50`，`claude-sonnet-4-6`；`test/pricing.test.js:1249`、`test/edge-pricing-parity.test.js:334`） | `0.0009675` | `100×3 + 20×15 + 600×0.3 + 50×3.75 = 967.5`；`/1e6` ✓（该模型 seed 原文为 `{"input_cost_per_token":0.000003,"output_cost_per_token":0.000015,"cache_read_input_token_cost":3e-7,"cache_creation_input_token_cost":0.00000375}`，换算即 3 / 15 / 0.3 / 3.75） |

以上两段演算与三个锚点，均已由复核方用 `seed-snapshot.json` / `curated-overrides.json` 原文独立复算，逐值一致；三锚点所用条目（`claude-opus-4-6`、`gpt-5.4`、`gemini-3-pro-preview`）在 `curated.exact` 中确实缺席（即 seed 命中），`claude-sonnet-4-6` 的 seed 原值与 edge `:443` 同值。

## 9. 消费端：CLI、本地 API、Dashboard、原生壳、云

样本仓库 `xiufengsun/TokenTracker`，本地检出 `C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）。本章全程只读，未改样本仓文件、未执行其脚本。

范围：统计结果从「算出来」到「呈现给人」的全部通路——CLI 输出面、本地 HTTP API、Dashboard 前端消费与二次聚合、导出/分享衍生面、云上传与云端再聚合、三端原生壳取数，以及与用量统计相邻但不属于它的边角面（限额、成就、宠物、技能）。不覆盖各 agent 日志解析内部与定价算法推导。

引用约定：`相对路径:行号`（相对样本仓根），行号取自本次实际读取；未亲自确认者标 `【未能确认】` 并在 9.7 汇总判定条件。有一个贯穿本章的结构性事实先说：**同一份 `queue.jsonl` 在仓内至少存在四份独立聚合实现**——本地 API（`src/lib/local-api.js`）、年度聚合器（`src/lib/wrapped-aggregator.js`）、Vite dev 插件（`dashboard/vite.config.js`）、云端 `account-*` 系列（`dashboard/edge-patches/`）。本章的分歧点几乎都源于这四份实现之间没有共享代码约束。

### 9.1 CLI 的对外输出面

#### 9.1.1 命令清单

入口 `src/cli.js`，**无参数时直接进 `serve`**（起本地服务 + Dashboard），所以「打开界面」是 CLI 的默认行为：`if (!command) { await cmdServe(argv); return; }`（`src/cli.js:15-19`）。命令分派见 `src/cli.js:32-65`：

| 命令 | 实现文件 | 输出形态 |
| --- | --- | --- |
| `serve`（默认） | `src/commands/serve.js` | 起本地 HTTP + 静态 Dashboard，打印 URL 与数据路径 |
| `init` | `src/commands/init.js`（2427 行） | 安装引导（不在本章范围） |
| `sync` | `src/commands/sync.js`（7251 行） | 进度与汇总计数 |
| `status` | `src/commands/status.js`（1729 行） | 人读列表 / `--json` / `--light` ASCII 表 |
| `diagnostics` | `src/commands/diagnostics.js` | 纯 JSON（`{ok, version, env, paths, ...}`） |
| `doctor` / `uninstall` / `device-login` | 各自同名文件 | 人读（`doctor` 支持 `--json` / `--out`） |
| `wrapped` | `src/commands/wrapped.js` | ASCII 年框 / `--json` |
| `sessions` | `src/commands/sessions.js` | JSON / CSV，可 `--out` |

`-v/--version`、`-h/--help` 在分派前处理（`src/cli.js:21-30`）。关键事实：**CLI 里没有「用量报表」命令**——`status` 报环境与上传健康度，`wrapped` 是唯一直接计算 token 用量的命令，`sessions` 导出会话级元数据。

#### 9.1.2 `status`：环境与上传健康度，不解析队列内容

`status` 读取一组状态文件（路径常量在 `src/commands/status.js:199-208` 共 10 个：`config.json`、`queue.jsonl`、`queue.state.json`、`cursors.json`、`notify.signal`、`openclaw.signal`、`sync.throttle`、`upload.throttle.json`、`auto.retry.json`、`sync.skip.json`）。**它只用 `safeStatSize` 拿队列字节数，减去 `queue.state.json` 的上传偏移，得到「待上传字节」，从不解析队列行内容**（`src/commands/status.js:278-288`：`const pendingBytes = Math.max(0, queueSize - (queueState.offset || 0));`）。本行独立复核：全文件检索 `total_tokens|total_cost|"cost"` 为 **0 命中**。时间线来自 `upload.throttle.json` 的 `lastSuccessMs` / `nextAllowedAtMs` / `backoffUntilMs`（`:341-347`），`last_parse` 取 `cursors.json` 的 `updatedAt`（`:1050`）。

`status --json` 顶层字段（`src/commands/status.js:1040-1259`）：

```js
// src/commands/status.js:1040-1059（节选）
    const summary = {
      version: pkg.version,
      generated_at: new Date().toISOString(),
      base_url: config?.baseUrl || null,
      device_token_set: Boolean(config?.deviceToken),
      queue: {
        pending_bytes: pendingBytes,
        size_bytes: queueSize,
        offset: queueState.offset || 0,
      },
      last_parse: cursors?.updatedAt || null,
      last_upload: lastUpload || null,
      next_upload_after: nextUpload || null,
      backoff_until: backoffUntil || null,
      last_upload_error: lastUploadError || null,
```

其余顶层键：`extra_scan_roots[]`（`{provider, origin, path, exists, error}`）、`hooks{}`（键名逐字：`codex_notify`、`acode_notify`、`every_code_notify`、`claude`、`gemini`、`opencode_plugin`、`openclaw_session_plugin`、`openclaw_session_plugin_conversation_access`、`openclaw_legacy`、`codebuddy`、`workbuddy`、`workbuddy-ai`、`grok`）、`providers{}`（约 40 家，每家 `{installed, files?, detail?, error?}`，`:1081-1238`）、`copilot{}`（`:1239-1246`）、`passive_mode{}`（`:1247-1250`）、Windows 专有 `wsl_mode`/`wsl_mode_invalid`/`wsl_distros[]`（`:1251-1257`）、`subscriptions`（`:1258`）。

三种渲染（`src/commands/status.js:1261-1266`）：`--json` 输出 `JSON.stringify(summary, null, 2)`；`--light` 输出固定宽度 ASCII 表，注释自述定位为「for AI agents and CI: deterministic columns, no emoji or spinner side effects, easy to grep」（`:1590-1593`）；无 flag 输出人读列表（首行 `TokenTracker v...`，含 `- Queue: N bytes pending`，`test/status-json-light.test.js:77-83` 锁定）。`--diagnostics` 走完全不同的形状（`{env, paths}`），测试要求 summary 的键不得泄漏进来（`test/status-json-light.test.js:68-75`）。参数仅 `--json`、`--light`、`--diagnostics`、`--probe-keychain`、`--probe-keychain-details`，未知 flag 直接抛错（`:1576-1584`）。

#### 9.1.3 `wrapped`：唯一直接算用量的 CLI

`cmdWrapped` **自己读 `queue.jsonl`，不经过本地 API**（`src/commands/wrapped.js:122-128`，`const rows = await readQueueRows(queuePath);`）。坏行容错是「静默跳过」（逐行 `JSON.parse` 失败即忽略，`:14-22`，注释说明生产队列的「半行」会在下次读取时自愈）。参数只有 `--json` 与 `--year`（年份校验 2000–2100，`:26-41`）。

聚合口径（`src/lib/wrapped-aggregator.js`）：

| 维度 | 规则 | 位置 |
| --- | --- | --- |
| 去重 | 按 `source\|model\|hour_start` 建 Map，后写胜出，并逐行应用 `normalizeQueueRow()` 旧行修正 | `:77-86` |
| 年份选择 | 未指定 `--year` 时取「数据中最新的一年」，不是当前年 | `:91-99` |
| 切天/切年/切小时 | `hour_start.slice(0,10)` 切天、前 4 位切年、正则 `/T(\d{2}):/` 取小时——**全 UTC** | `:37-53`；`:66-67` JSDoc 明写 "calendar year (UTC)" |
| 连续天数 | `new Date(iso + "T00:00:00Z")` 按日步进（UTC） | `:148-152` |
| token 口径 | `billable_total_tokens` 优先，回落 `total_tokens` | `:29-35` |
| 成本 | **无**——全程不触碰 `total_cost_usd`（文件内 `cost` 字样 0 命中），不 require 定价模块 | `:17-23`（惰性 `require("./local-api")` 的注释，自述为避免加载 pricing 表） |

```js
// src/lib/wrapped-aggregator.js:209-223
  return {
    year,
    range: { from: earliest, to: latest },
    totals: {
      tokens: totalTokens,
      conversations: totalConvs,
      sources: tokensBySource.size,
      models: tokensByModel.size,
      active_days: activeDays.length,
    },
    top: { sources: topSources, models: topModels, days: topDays },
    peak_hour: peakHour,
    longest_streak: longestStreak,
    highlights,
  };
```

`top.sources`/`top.models` 各取 5 并附 `share` 占比，`top.days` 取 5（`:177-187`）；`highlights` 是四条硬编码英文句式（顶层模型占比、最忙一天 token、峰值小时、连续天数）+ 一条「工具数 ≥ 4」（`:189-207`）。ASCII 渲染（`src/commands/wrapped.js:43-120`）为 60 宽 `╔═╗` 框；`Total tokens` 走 `formatCompact`（T/B/M/K，`src/lib/wrapped-aggregator.js:226-234`）；**`Active days` 硬写 `/ 365`**（`:67`）；`Peak hour` 显示 `NN:00 UTC`（`:70-77`）——时区基准在输出上被显式标注。

#### 9.1.4 `sessions`：会话级元数据导出

`cmdSessions` 是**独立于 `wrapped` 的第二条统计链**（`session.queue` 侧车 + 直接扫会话文件，主体在 `src/lib/session-analytics.js`）：`buildSessionAnalytics({force})` → 可选 `buildGitOutcomes` → `summarizeSessions({from,to})`（`src/commands/sessions.js:12-27`）。CSV 列定义在 `src/lib/session-analytics.js:2909`：`session_hash, source, project_key, model, model_usage_json, started_at, ended_at, duration_ms, turns, edit_turns, retry_turns, subagent_calls, total_tokens, cost_usd, productive, first_pass, one_shot`。写文件时 `mode: 0o600`（`src/commands/sessions.js:22`）。

`summarizeSessions()` 返回 `available`、`sessions[]`（默认剥离 `project_ref`/`session_id`/`parent_session_id`/`title`，`src/lib/session-analytics.js:2526-2543`）、`session_count`、`summary{}`（含 `productive_rate`、`one_shot_rate`、`edit_session_rate`、`tokens_per_edit`、`cost_per_edit`，`:2545-2555`）、`by_model[]`、`subagents[]`、`provenance{source:"local-session-log", confidence:"observed", privacy:"metadata-only", methodology:"edit-turn-v2", ...}`（`:2558-2565`）。**源码自带口径警告**：`by_model[].sessions` 是「触碰过该模型的会话数」，**不可求和**为总会话数；只有 token/成本列可求和（`:2421-2427`）。

#### 9.1.5 时区口径冲突：`wrapped` 与 Dashboard 的两套切天基准

这一条必须**按路径分两级**描述，不能简化为「不带 tz 就回 UTC」：

| 路径 | 未显式传时区时的基准 | 证据 |
| --- | --- | --- |
| `wrapped`（CLI 与 `/functions/tokentracker-wrapped` 共用聚合器） | **UTC** 切年/切天/切小时/算连续天数，无回退分支 | `src/lib/wrapped-aggregator.js:37-53`、`:148-152`、`:170` |
| 本地 API 日聚合一族（`rowDayKey`：daily 与 summary 的日聚合） | 带 `tz`/`tz_offset_minutes` 时按该时区切天；**未带则回落 UTC 日** | `src/lib/local-api.js:213-225`（`:224` 即 `hs.slice(0,10)`） |
| 本地 API `getZonedParts` 系：hourly 的日过滤与小时键、summary 的 `todayStr`、heatmap 的 `todayStr`、`night_owl` 徽章 | 三级回退的**第三级是运行环境本机时区**（这几处无 tz 守卫） | `src/lib/local-api.js:585-592`、`:606`、`:608`、`:2382`、`:2538`、`:482` |

由此得两条强度不同的结论：**对真实产品路径**（前端与三端壳一律显式传 tz：`dashboard/src/lib/api.ts:244-249`、`TokenTrackerBar/.../APIClient.swift:241-254`、`TokenTrackerWin/UsagePoller.cs:321`、`TokenTrackerLinux/.../extension.js:997`），非 UTC 时区下同一用户同一年会看到两个不同的「活跃天数」与「最忙一天」——Wrapped 按 UTC、Dashboard 按所选时区，**该冲突在真实前端路径上成立**；**对裸调 API 不传 tz 的调用者**，与 `wrapped` 的 UTC 基准真正分歧的是 `getZonedParts` 一系（本机时区），其中 `hourly` 的 `day` 默认值又是 UTC 今天（`:3147`），与它自己的过滤谓词不同基准。

### 9.2 本地 API 全清单

#### 9.2.1 服务骨架与响应约定

`serve` 的 HTTP 服务按前缀三分派：`/functions/`、`/api/`、`/proxy/` 交给 `handleApi`，其余走静态资源与 SPA fallback（`src/commands/serve.js:171-189`）。

| 项 | 取值 | 证据 |
| --- | --- | --- |
| 绑定 | 仅 `127.0.0.1` | `src/commands/serve.js:34` |
| 端口 | 默认 7680；WSL 下 7681；非显式端口时最多顺延 20 个 | `src/commands/serve.js:14-22`、`:202-215` |
| CORS | 任何 `OPTIONS`（`/api/auth/` 前缀除外）回 204 并放行 `GET, POST, OPTIONS` | `src/commands/serve.js:162-169` |
| 队列路径 | `~/.tokentracker/tracker/queue.jsonl`（`resolveQueuePath()`） | `src/lib/local-api.js:83-86` |
| 静态资源 | `dashboard/dist`，两层候选路径 | `src/commands/serve.js:716-725` |
| 后台任务 | Windows 专有每分钟 `sync --auto --background --all-local-sources`；匿名心跳启动一次 + 每 6 小时复查 | `src/commands/serve.js:294-366`（`:303-309` 条件）、`:266-273` |
| 响应头 | 所有 JSON 统一 `Cache-Control: no-store` | `src/lib/local-api.js:914-927` |

鉴权三层：**① 本地令牌** `crypto.randomBytes(24).toString("hex")`（`src/lib/local-api.js:1016`），经 `GET /api/local-auth` 暴露（`:1681-1695`），`serve.js` 用同一端点做重复实例与健康判定（`src/commands/serve.js:508-534`）；**② 写操作**校验头 `x-tokentracker-local-auth`（`isAuthorizedLocalMutation`，`src/lib/local-api.js:1629-1637`）；**③ `/api/auth/*` 与 `GET /api/auth-bridge/verifier`** 额外要求同源/回环来源，否则 403 `untrusted_auth_origin`（`:1643-1648`）。前端取令牌见 `dashboard/src/lib/local-api-auth.ts`（`getLocalApiAuthToken()` / `getLocalApiAuthHeaders()`，进程内缓存）。

#### 9.2.2 通用查询参数（用量类端点）

| 参数 | 语义 | 证据 |
| --- | --- | --- |
| `from` / `to` | `YYYY-MM-DD`，按**日键闭区间**过滤（`day >= from && day <= to`） | `src/lib/local-api.js:2359-2364`、`:130-134` |
| `tz` / `tz_offset_minutes` | IANA 名与分钟偏移（fallback） | `getTimeZoneContext` `:512` 起；键生成 `:115-117` |
| `scope` / `include_account_level` | 默认 `all`；`personal\|local` 排除账号级来源；`include_account_level=1` 强制 `all` | `:398-401`、`src/lib/source-metadata.js:15-26` |
| `source` / `model` / `device_id` | 前端拼进查询串 | `dashboard/src/lib/api.ts:254-263` |
| `account=1` | 触发账号（跨设备）视图代理 | `src/lib/local-api.js:2347-2355` |
| `weeks` / `day` / `limit` | 热力图周数默认 52；hourly 的日（默认 UTC 今天）；项目汇总默认 10（`PROJECT_USAGE_MAX_ENTRIES`）、会话浏览器上限 2000 | `:2534`、`:3147`、`:95`、`:2504-2505` |

账号级来源白名单**只有两家**：`cursor` 与 `trae-cn`（`src/lib/source-metadata.js:1`），其用量在账号视图里是「跨设备去重而非相加」，一致性由 `test/account-source-parity.test.js:3-18` 强制（4 处硬编码必须同步，漂移即重复计数 bug 回归）。

#### 9.2.3 `/functions/tokentracker-*` 端点总表（27 个）

路由字面量清点所得，**共 27 个**（独立复核确认）。「定义行」为 `if (p === ...)` 所在行。

| # | 路径 | 方法 | 主要参数 | 返回顶层字段（摘要） | 用途 | 定义行 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `-usage-summary` | GET | `from`,`to`,`tz*`,`scope`,`account` | `from,to,days,scope,excluded_sources,totals{},rolling{}` | 概览 | `:2358` |
| 2 | `-usage-daily` | GET | `from`,`to`,`tz*` | `from,to,scope,excluded_sources,data[]` | 逐日 | `:2425` |
| 3 | `-usage-hourly` | GET | `day` | `day,scope,excluded_sources,data[]` | 逐小时 | `:3146` |
| 4 | `-usage-monthly` | GET | `from`,`to` | `data[]` | 逐月 | `:3156` |
| 5 | `-usage-heatmap` | GET | `weeks` | `from,to,scope,excluded_sources,week_starts_on,active_days,streak_days,weeks[][],total_cost_usd` | 热力图 | `:2533` |
| 6 | `-usage-model-breakdown` | GET | `from`,`to` | `from,to,days,scope,excluded_sources,sources[],pricing{}` | 模型维度 | `:2596` |
| 7 | `-usage-category-breakdown` | GET | `from`,`to`,`source` | 类别分解（可带降级标注） | 语义类别 | `:2668` |
| 8 | `-wrapped` | GET | `year` | `scope,excluded_sources,` + 年度结构 | 年度报告 | `:2330` |
| 9 | `-outcomes` | GET | `from`,`to` | `available,by_model,by_tool,totals` | 产出质量 | `:2441` |
| 10 | `-context-health` | GET | — | `computeContextHealth({home,cwd,env})` 原样 | 上下文健康 | `:2526` |
| 11 | `-session-insights` | GET | `from`,`to`,`refresh`,`include_sessions`,`format=csv` | `from,to,` + 会话摘要 | 会话效率 | `:2470` |
| 12 | `-sessions` | GET | `from`,`to`,`refresh`,`limit` | `from,to,` + 会话浏览结果 | 会话浏览器（本地专用） | `:2500` |
| 13 | `-project-usage-summary` | GET | `from`,`to`,`tz*`,`limit` | `generated_at,entries[]` | 项目汇总 | `:2731` |
| 14 | `-project-usage-detail` | GET | `project_key`（必填，缺失回 400 `missing_project_key`） | `generated_at,from,to,project_key,project_ref,totals{},days_active,range_total_tokens,daily[],sources[]` | 项目详情 | `:2828` |
| 15 | `-usage-limits` | GET | `refresh`,`devin` | 各 provider 键对象 + `fetched_at` | 限额窗口 | `:3482` |
| 16 | `-subscription-manager` | GET/POST | POST 动作 `create\|update\|delete` | `subscriptions[]` | 手填订阅（续费/到期日） | `:3240` |
| 17 | `-achievements` | GET | `tz*` | `generated_at,achievements[]` | 徽章 | `:3470` |
| 18 | `-skills` | GET/POST | GET `mode=installed\|repos\|discover\|search\|popular\|updates\|activity\|skill_usage`；POST 多动作 | 随 mode 变化 | 技能 | `:3294` |
| 19 | `-pets` | GET/POST | GET `scope=codex`；POST `install_url\|remove\|import_codex` | `pets[],hiddenBuiltinIds[]` 或 `importable[],codexDetected` | 桌宠 | `:3192` |
| 20 | `-user-status` | GET | — | `user_id,email,name,is_public,pro{},account{}` | 本地 stub 用户态 | `:2920` |
| 21 | `-cloud-session` | POST | `{clear?,session_id?,expectedOwnerId,insforgeBaseUrl?}` | `session_id,device_id,created_at` | 云会话 | `:2137` |
| 22 | `-local-sync` | POST | 多个布尔开关（`auto`/`background`/`lightweight`/`drain`/`publishAccount`/`allLocalSources`/…） | 成功 `ok,uploaded,pending,next_allowed_at?`；失败 `ok:false,error,code,recovery_url,next_allowed_at,retry_after_seconds,stdout,stderr` | 触发同步 | `:2166` |
| 23 | `-cloud-sync-pref` | GET/POST | POST `{enabled}` | GET `enabled,account_available`；POST `ok,enabled` | 云同步开关 | `:2940` |
| 24 | `-machine-id` | GET | — | `machineId,deviceName`（deviceName = 主机名，截断 128） | 设备标识 | `:3137`、`:31-41` |
| 25 | `-telemetry-pref` | GET | — | `disabled` | 心跳偏好 | `:3121` |
| 26 | `-proxy-config` | GET/POST | POST 经 `parseProxyPayload` | `mode,protocol,host,port,effective,applyError` | 代理配置 | `:2982` |
| 27 | `-proxy-test` | POST | `{mode:"manual",...}` | `runProxyConnectivityTest(...)` 结果 | 代理测试 | `:3074` |

（表中路径省略统一前缀 `/functions/tokentracker`。）21–27 与 15–19 的 POST 均需本地鉴权头。

#### 9.2.4 `/api/*` 与 `/proxy/*`

| 端点 | 方法 | 参数 / body | 返回 | 定义行 |
| --- | --- | --- | --- | --- |
| `/api/runtime-config.js` | GET | — | `window.__TOKENTRACKER_RUNTIME_CONFIG__={baseUrl,anonKey,dashboardUrl,configurationError}`（`application/javascript`，转义 `<` 与 U+2028/9） | `:1670-1679` |
| `/api/local-auth` | GET | — | `{token, serverVersion?}` | `:1681-1695` |
| `/api/pets/local/:id/spritesheet.webp`、`/api/pets/codex/:id/spritesheet.webp` | GET/HEAD | — | `image/webp` + `no-cache` + `nosniff` | `:1698`、`:1724` |
| `/api/pets/import` | POST | body 上限 `pets.MAX_PACKAGE_BYTES` | 导入结果 | `:1747` |
| `/api/auth-bridge/verifier` | GET/PUT/POST | body `{native:boolean}` | `{native}`（一次性读取，TTL 5 分钟） | `:1767-1788` |
| `/api/auth/*` | 任意 | 透传到 InsForge；`refresh\|logout` 有 csrf/relay 特例 | 上游响应原样 | `:1792` 起 |
| `/api/avatar-proxy` | GET | `?url=` | 图片字节（内存 LRU：1h TTL、512 KiB/条、64 条） | `:2038`、`:43-48` |
| `/api/geoip/`、`/api/geoip-batch`、`/api/iprisk/`、`/api/dns/result/` | 任意 | 代理到 `ip.net.coffee` 对应接口 | 上游响应 | `:1973-1976` |
| `/proxy/ipcheck` | 任意 | 透传到 `https://ip.net.coffee`，剥离 hop-by-hop 头 | 上游响应 | `:939-1001` |

#### 9.2.5 聚合链：谁共用主链，谁各自遍历

**共用主链**：`-usage-summary`、`-usage-daily`、`-usage-hourly`、`-usage-monthly`、`-usage-heatmap`、`-usage-model-breakdown` 六个端点都建立在「读 `queue.jsonl` → 逐行去重与旧行修正 → 按时区切天聚合」上（`readQueueData` 的 `parseRow` 完成去重与修正，`src/lib/local-api.js:170-211`），差别只在粒度与投影字段。注意 daily 的桶结构里 `models` 映射按 **`total_tokens`** 聚合：`a.models[model] = (a.models[model] || 0) + (row.total_tokens || 0);`（`:297`）——而 `wrapped` 的份额按 billable 算（`src/lib/wrapped-aggregator.js:182-186`），两处「模型占比」分母不同源。

| 端点族 | 数据来源 | 关系 |
| --- | --- | --- |
| `-project-usage-summary` / `-detail` | 派生队列 `project.queue.jsonl`（`:135`），剔除 claude-mem 伪项目（`:136-138`） | 独立文件、独立聚合；全空时降级为「按 source 聚合」的伪条目且 `project_ref` 留空（`:2803-2818`） |
| `-session-insights` / `-sessions` | `buildSessionAnalytics`（会话文件 + `session.queue` 侧车） | 完全独立的第二条链 |
| `-outcomes` | `outcomes.jsonl` 侧车 + 会话分析 + `computeQualityPerDollar`；未开启时回 `{available:false, by_model:[], by_tool:[], totals:null}`（`:2460`） | 独立；注释强调不改 queue schema、不碰同步链（`:2435-2440`） |
| `-usage-limits` | provider 官方 API + 本地磁盘缓存 | **与队列零关系**（见 9.7.1） |
| `-wrapped` | `aggregateWrapped(rows, {year})`（`:2333-2336`） | 复用主链的**行**，但用**独立的 UTC 聚合器** |
| `-achievements` / `-skills` / `-pets` | 各自模块（徽章回放主链行；`skill_usage` 另有计价段） | 部分复用主链行 |

#### 9.2.6 `account=1` 包装层与失败标记

账号（跨设备）视图**不是新端点，而是前置包装层**：

```js
// src/lib/local-api.js:2347-2355
    if (url.searchParams.get("account") === "1") {
      const usageSlug = p.startsWith("/functions/") ? p.slice("/functions/".length) : "";
      if (accountSlugFor(usageSlug)) {
        const result = await tryServeAccountView(usageSlug, url, res);
        if (result === "served") return true;
        res.setHeader("X-TokenTracker-Account-View", "0");
        res.setHeader("X-TokenTracker-Account-Fallback", result);
      }
    }
```

映射覆盖六个 slug（`src/lib/cloud-account.js:20-27`）：`usage-summary→account-summary`、`-daily`、`-hourly`、`-monthly`、`-heatmap`、`-model-breakdown`。`project-usage`、`usage-limits`、`category-breakdown` 被源码注释明确排除（「remain local-only and are intentionally absent here」，`:17-19`）。成功时打头 `X-TokenTracker-Account-View: 1`（`src/lib/local-api.js:1565`）；失败打 `: 0` 并附加原因串，**继续走本地 handler**——调用者拿到本机单机数据 + 一个「这不是跨设备数据」的标注。失败冷却 2 分钟（`:27`）。

#### 9.2.7 若干返回字段的准确语义（易踩）

| 字段 | 准确语义 | 证据 |
| --- | --- | --- |
| `usage-summary.rolling.last_7d/30d.active_days` | 只统计「确有日汇总的活跃日」（跳过空日），**不等于 7/30** | `src/lib/local-api.js:2390-2404` |
| `usage-model-breakdown.pricing.source` | **硬编码标签 `"litellm"`**，不代表本行实际命中来源（命中 `curated:*` 时仍写 litellm）；真实来源看每行 `cost_source`（`provider_reported`/`model_pricing`/`mixed`） | `:2658`；对照 `:2624-2626`；dev mock 同类字段硬编码 `"default"`（`dashboard/vite.config.js:1067`） |
| 成本求和顺序 | **必须逐行计价后再求和**；源码解释「先加列再计价」会吞掉长上下文/加急档位（#653 的修法） | `src/lib/local-api.js:2634-2640` |
| `project-usage-summary.entries[].total_tokens` | **字符串**（大整数过 JSON 的显式处理）；排序按 `billable_total_tokens` 降序 | `:2787-2794` |
| `usage-heatmap.streak_days` | 本地端点**硬编码 0**（见 9.3.3） | `:2588` |
| `category-breakdown` 降级 | `codex` 分支总量为 0 时改用队列行降级口径，返回带 `breakdown_status:"queue_fallback"`、`fallback:"queue_totals"`，并声明 `includes_content=false`/`includes_inputs=false` | `:2692-2698`、`:329-395`（`:376-379`、`:386-389`） |
| `-sessions` 的本地性 | 注释写明 LOCAL ONLY 且 never proxied to the cloud account view；出错时把 fs 错误挡在响应体外（避免绝对路径泄漏），只回 `{available:false, error:"Session browser failed"}` | `:2495-2499`、`:2509`、`:2516-2521` |

### 9.3 Dashboard 如何消费

#### 9.3.1 唯一 API 层与本地/云端双路由

`dashboard/src/lib/api.ts` 集中端点表（`:141-158`）与「本地 usage-* → 云端 account-*」映射（`:166-173`）；注释说明云端 `account-*` 的 schema **逐字段镜像**本地 `tokentracker-usage-*`，「so the dashboard components render identically off either source」。

| 行为 | 规则 | 证据 |
| --- | --- | --- |
| 本地路径 | `new URL(\`/functions/${slug}\`, window.location.origin)`，`cache: "no-store"` | `:222-234` |
| 云端路径触发条件 | 非 localhost + 有 accessToken + 有 accountSlug 映射 | `:191-199` |
| 非 localhost 且无 token | 抛 `LOCAL_API_UNAVAILABLE`（404） | `:216-221` |
| 账号响应缓存 | summary 120 s、其余 300 s、`stale-if-error` 5 分钟、上限 64 条；键 `(origin, path+排序后 query, token sub/anon key)` | `:29-31`、`:75-129` |
| in-flight 合并 | `coalesceJsonGet()`，键为完整 URL | `:62-73` |
| 热力图传输 | 账号路径带 `format=compact`，响应过 `expandHeatmapCompact` | `:199`、`:212` |

#### 9.3.2 页面 → 端点映射

| 页面 / 组件 | 消费的端点 | 证据 |
| --- | --- | --- |
| `DashboardPage.jsx` | usage-summary / daily / hourly / monthly / model-breakdown / heatmap / project-usage-* / sessions / category-breakdown | `dashboard/src/pages/DashboardPage.jsx`（示例 `:565`、`:602`、`:819`）；端点常量 `dashboard/src/lib/api.ts:141-158` |
| `SessionsPage.jsx` | `-sessions`、`-session-insights` | `dashboard/src/lib/api.ts:681`、`:157` |
| `WrappedPage.jsx` | `-wrapped?year=` | `dashboard/src/pages/WrappedPage.jsx:43-45` |
| `LimitsPage.jsx` | usage-limits（+ subscription-manager） | `dashboard/src/pages/LimitsPage.jsx:49-66`、`:123-133` |
| `AchievementsPage.jsx` | achievements | `dashboard/src/lib/api.ts:151`、`:328-334` |
| `LeaderboardPage.jsx` / `LeaderboardProfilePage.jsx` | 云端 leaderboard / leaderboard-profile / profile-likes | `dashboard/src/lib/api.ts:383-583` |
| `DevicePage.jsx` | account-devices（云）/ device-rename | `dashboard/src/lib/api.ts:840-860`、`:1040-1063` |
| `CloudPage.jsx` / `CloudCheckoutPage.jsx` | billing / cloud-session / checkout | `dashboard/src/lib/cloud-checkout.js`、`dashboard/src/lib/api.ts:157` |
| `PetPage.jsx` / Linux 桌宠宿主 | pets、usage-summary、heatmap、model-breakdown、usage-limits | `dashboard/src/lib/pets-api.js:21`、`:39`、`:66`；`dashboard/src/lib/pet-linux-host.js:194`、`:211`、`:223`、`:237` |
| `SkillsPage.jsx` / `SkillDetailPanel.jsx` | skills（多 mode） | `dashboard/src/lib/api.ts:151`；端点 `src/lib/local-api.js:3294` |
| 设置页账号区 | user-status / cloud-sync-pref / telemetry-pref / proxy-config | `dashboard/src/hooks/use-proxy-settings.js:4-5`、`dashboard/src/lib/cloud-sync-prefs.ts:98`、`:157`、`dashboard/src/lib/analytics.js:67` |

桌宠那一路值得单列：`pet-linux-host.js` 是**独立于 React 页面的取数宿主**，自己拼 `?weeks=52&tz=...&account=1` 并读 `data.streak_days`、`sources[].models[]`、`totals`（`dashboard/src/lib/pet-linux-host.js:194-237`）。

#### 9.3.3 后端优先 + 前端兜底：`activeDays` 与 `streak_days`

`activeDays` 的策略是「后端优先、缺失才自算」：

```js
// dashboard/src/pages/DashboardPage.jsx:819-850（节选）
  const activeDays = useMemo(() => {
    if (!signedIn && !mockEnabled && !publicMode && !isLocalMode) return 0;
    const serverActive = Number(heatmap?.active_days);
    if (Number.isFinite(serverActive)) return serverActive;

    let count = 0;
    const seen = new Set();
    const considerDay = (day, value, level) => {
      if (typeof day !== "string" || !day) return;
      if (seen.has(day)) return;
      if (!hasUsageValue(value, level)) return;
      seen.add(day);
      count += 1;
    };
```

拿不到 `heatmap.active_days` 时才遍历 `heatmapDaily` 与 `heatmap.weeks` 自行点数（`:835-847`）。

`streak_days` 更尖锐——**服务端在本地端点里恒为 0，前端自算并覆盖**：`src/lib/local-api.js:2587-2588` 同一响应里 `active_days: activeDays.length`（真实计算）与 `streak_days: 0,`（常量）；前端 `dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295` 四处调用 `computeActiveStreakDays({...})`（导入 `:4`）写入自己的值。消费者分布：

| 消费者 | 连续天数来源 | 后果 |
| --- | --- | --- |
| Dashboard 热力图 | 前端 `computeActiveStreakDays` 覆盖 | 正确 |
| Linux 桌宠宿主 | **直读 `r.body?.streak_days`**（`dashboard/src/lib/pet-linux-host.js:198`） | 本地模式下拿到 0 |
| Windows 托盘 | `UsagePoller.cs:314-328` 读 `streak_days`，注释却写「streak is server-computed」 | **本地模式下预计显示 0** |

`【未能确认】` Windows 托盘在本地模式下的实际显示值（源码可证「本地端点恒 0 + 托盘直读该字段」，界面表现需实测；判定条件：启动 Windows 壳 → 非账号视图 → 观察托盘/小组件）；`【未能确认】` 云端 `tokentracker-account-heatmap.ts` 是否返回真实 `streak_days`（判定条件：读该文件全篇，或账号视图下抓响应体查 `streak_days`）。

#### 9.3.4 紧凑热力图的前端展开

账号视图下热力图走稀疏线格式，前端还原（`dashboard/src/lib/heatmap-compact.js:20-75`）：

```js
// dashboard/src/lib/heatmap-compact.js:49-72（节选）
  const maxValue = Number(payload.max_value) || 0;
  const levelOf = (v) => {
    if (v <= 0) return 0;
    if (maxValue === 0) return 1;
    const r = v / maxValue;
    if (r <= 0.25) return 1;
    if (r <= 0.5) return 2;
    if (r <= 0.75) return 3;
    return 4;
  };
  ...
    cells.push({
      day: cursor,
      total_tokens: total,
      billable_total_tokens: total,
      level: levelOf(total),
      models: hit ? hit.models : null,
    });
```

三点结论：**① 展开会重算 `level`**，用的是与服务端相同的「按窗口最大值归一、四档 0.25/0.5/0.75」规则（服务端规则见 `src/lib/local-api.js:2547-2557`）——规则在两处各写一遍，靠数值约定而非共享代码保持一致；**② 紧凑路径下 `billable_total_tokens` 被赋成 `total`**（`:69`，稀疏格式只带一个 `total`），因此账号视图热力图单元格**丢失 billable 与 raw total 的差异**，而 `activeDays` 兜底路径用 `getBillableTotal(row)` 读数（`dashboard/src/pages/DashboardPage.jsx:837`）在紧凑数据上等价于读 total；**③ 防御性设计明确**：非紧凑或形状不合规时原样返回（`:20-28`，同时让新 CLI 对旧 edge 保持正确），缺 `model_names` 字典时拒绝展开而不是静默丢数据（`:24-27`）。

#### 9.3.5 前端是否存在二次计价：**不存在**

独立检索 `dashboard/src` 全目录（`*.js/jsx/ts/tsx`）中 `computeRowCost` 与 `getModelPricing` 的引用：**0 命中**。结论：前端从不重新计价，`total_cost_usd`、逐行成本、档位与分项一律由后端给出。后端 `src/lib/local-api.js` 内的计价只出现三处：`buildDailyAggregates`（`:285`）、`usage-model-breakdown`（`:2640`）、`skill_usage` 定价段（`:3381`）——该计数只限本文件；`src/lib/session-analytics.js`、`src/lib/outcomes-engine.js` 与云端 edge 侧各有独立受理点（见 8.4）。

#### 9.3.6 前端二次聚合清单

| 前端行为 | 位置 | 性质 |
| --- | --- | --- |
| 活跃天数后端优先、缺失才重算 | `dashboard/src/pages/DashboardPage.jsx:819-850` | 后端优先 + 前端兜底 |
| 连续天数重算并覆盖 | `dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295` | 纯前端（服务端本地值为 0） |
| 紧凑热力图展开（含重算 level） | `dashboard/src/lib/heatmap-compact.js:20-75` | 传输优化的还原 |
| 趋势缩放统计（总量/成本/活跃桶/峰值） | `dashboard/src/lib/trend-stats.js:34-79` | 前端二次汇总 |
| 桶标签本地化与「一律 `timeZone:"UTC"`」格式化 | `dashboard/src/lib/trend-stats.js:106-138`、`:152-180` | 纯展示层 |
| 明细表排序/分页 | `dashboard/src/pages/DashboardPage.jsx:681-750` | 前端（后端用量端点不分页） |
| 显示币种换算 | `dashboard/src/hooks/useCurrency.js:3-15` | 前端换算（服务端只给 USD） |
| token 数字格式 | `dashboard/src/hooks/useTokenFormat.js:2-20`、`dashboard/src/lib/token-format.js` | 前端展示 |
| 榜单列宽/排序/分页配置 | `dashboard/src/lib/leaderboard-columns.js`、`leaderboard-ui.js` | 前端 |
| 份额/占比 | `-project-usage-detail` 已给 `range_total_tokens`（`src/lib/local-api.js:2908-2910`） | 后端给分子分母，前端只做除法 |

**只在后端算、前端一律不重算**：逐行成本与档位、去重（`source|model|hour_start` last-wins）与旧行修正、项目归属与 claude-mem 伪项目剔除、热力图服务端 level、本地徽章阈值与达成时间（`src/lib/local-api.js:419-423`、`:434-511`）、账号级来源跨设备去重语义、会话效率整套定义（`src/lib/session-analytics.js:2385-2566`）。

#### 9.3.7 轮询、缓存与刷新触发

| 机制 | 参数 | 证据 |
| --- | --- | --- |
| 进程内 in-flight 合并 | 键为完整 URL | `dashboard/src/lib/api.ts:62-73` |
| 账号响应 TTL 缓存 | summary 120 s、其余 300 s、stale-if-error 5 min、上限 64 | `dashboard/src/lib/api.ts:75-129` |
| 预取 | `DASHBOARD_PRELOAD_TARGETS = ["limits", "leaderboard"]`；20 条榜单内存 LRU | `dashboard/src/lib/dashboard-preload.js:3`、`:23-71` |
| macOS 事件驱动 | 监听 `queue.jsonl` 写入与 `queue.state.json`，突发写 settle 后再刷一次 | `TokenTrackerBar/TokenTrackerBar/Services/QueueActivityMonitor.swift:3-5` |
| Windows | 健康检查 30 s；用量轮询在 `UsagePoller` | `TokenTrackerWin/Constants.cs:14-15`、`TokenTrackerWin/UsagePoller.cs:85-131` |
| Linux 扩展 | `Soup.Session({timeout: 20})`；菜单打开与固定周期都取数 | `extension.js:784`、`:830-832` |
| 埋点 | PostHog，`autocapture: false`；本地/原生环境先问 `telemetry-pref`，「答不出来就关」（fail-closed） | `dashboard/src/lib/analytics.js:24-45`、`:13`、`:67` |

#### 9.3.8 Vite dev 的 mock 路径（第四份聚合实现）

仅 `npm run dev` 时存在：插件**直接读 `~/.tokentracker/tracker/queue.jsonl`**（`dashboard/vite.config.js:418` 注释、路径常量 `:570`），复刻主链的 last-wins 去重与 codex 旧行修正（`:572-604`：`seen.set(\`${row.source||""}|${row.model||""}|${row.hour_start||""}\`, normalizeQueueRow(row))`），并自带日聚合（`:610-639`）。**它不复制价格表**——通过 `createRequire(import.meta.url)` 直接 require 同一份 `src/lib/pricing`（`getModelPricing`/`computeRowCost` 均来自该模块），`:508-514` 注释逐字写明「Per-model pricing — delegated to `src/lib/pricing/` (CJS)」。唯一已知差异：dev mock 对**按模型聚合后的 totals 只计一次价**（`:1052-1057`），与 `src/lib/local-api.js:2634-2640` 修掉的写法相同，仅影响 dev。

`【未能确认】` dev 中间件对外暴露的具体路径前缀集合（判定条件：读 `configureServer` 的 `server.middlewares.use(...)` 注册串）。

### 9.4 数据导出与分享、成就与宠物等衍生面

#### 9.4.1 Share Card：两套口径拼在一张图上

组装链：页面状态 → `useShareCardData()` → `buildShareCardData()` → `ShareCard.jsx` → 截图。`handle`、`startDate`、`activeDays`、`summary`、`topModels`、`heatmap` 全部来自当前页面已拉到的数据，**唯有 `rank` 单独向云端榜单请求**（`dashboard/src/ui/share/use-share-card-data.ts:11-26`、`:49-80`；`leaderboardPeriod` 把 `day`/`custom` 一律按 `week` 取，`:55`）。

| 细节 | 规则 | 证据 |
| --- | --- | --- |
| 排名可见性 | 只在第一页（`limit: 100`）里找 `is_me`——**排名超过 100 时不显示** | `use-share-card-data.ts:70-72` |
| token 口径 | `billable_total_tokens ?? total_tokens` | `dashboard/src/ui/share/build-share-card-data.ts:47-50` |
| 成本 / topModels / level | `total_cost_usd`；topModels 上限 3；热力图 level 夹在 0–4 | `:118-119`、`:62-85` |
| 截图 | 动态 `import("html-to-image")`，注入 Google Fonts（PRIVACY 已披露） | `dashboard/src/ui/share/capture-share-card.ts:53`、`:4-5`；`docs/PRIVACY.md:86` |
| 契约测试 | 回落 `—`、topModels 截断 3、非法 rank 归 null、千分位边界 | `test/share-card-data.test.js:23-138` |

**结论**：同一张卡片上 token/成本是单机（或账号去重后）口径，rank 是平台全体已上传用户口径——两个数字都不错，但不能相互印证。

#### 9.4.2 徽章与嵌入卡片 SVG（云端产物）

| 产物 | 端点与参数 | 要点 | 证据 |
| --- | --- | --- | --- |
| 徽章 | `/functions/tokentracker-badge-svg?user_id=<uuid>&metric=tokens\|cost\|rank`，缓存 60 s | 自绘 SVG（含 `flat-square` 分支）；数字 `compactNumber`、金额 `formatCost`；先解析榜单窗口 `windowBounds(client, period)` 再读快照行（`rank`、`total_tokens`）；封禁账号走黑名单 | `dashboard/edge-patches/tokentracker-badge-svg.ts:7-21`、`:136-148`、`:68`、`:78`、`:106-129`、`:37` |
| 嵌入卡片 | `tokentracker-embed-svg.ts`（320 行） | 同样窗口计算；头像由显示名哈希生成；主题 light/dark；占位文案 `placeholderSvg` | `:149-159`、`:109-123`、`:124` |

两者都依赖账号与榜单快照，与本地统计无关。契约测试是非 Deno 的 CommonJS 移植镜像，注释声明「Deno copy 是 source of truth，改渲染必须同步」（`test/badge-svg-render.test.js:1-14`）。

#### 9.4.3 Wrapped 的三条消费路径（同一份计算）

| 路径 | 取数 | 渲染 |
| --- | --- | --- |
| CLI `wrapped` | 直接读 `queue.jsonl`（`src/commands/wrapped.js:126-128`） | ASCII 框（`:43-120`）/ `--json` |
| `/wrapped` 页 | `fetch("/functions/tokentracker-wrapped")`（`dashboard/src/pages/WrappedPage.jsx:43-45`） | React，`highlights` 用 copy 模板重排（`:100-121`） |
| 本地 API | `aggregateWrapped(rows, {year})`（`src/lib/local-api.js:2333-2336`） | JSON |

三者共用 `src/lib/wrapped-aggregator.js`（模块头注释自称 sources of truth），因此**内部口径一致，包括同一处 UTC 偏向**。

#### 9.4.4 成就、宠物、技能

本地徽章只有三枚，且**阈值只存在于服务端**：

```js
// src/lib/local-api.js:419-423
const LOCAL_BADGE_THRESHOLDS = {
  project_hopper: [3, 5, 10, 20], // distinct projects
  project_devotion: [1000000, 10000000, 100000000, 1000000000], // max tokens in one project
  night_owl: [5, 20, 60, 150], // active hour buckets between 00:00–05:59 local
};
```

注释同时写明「the dashboard renders whatever the payload says and embeds none of these numbers」（`:416-418`）。达成时间按 `hour_start` 顺序回放、取首次跨阈值的桶（`test/local-achievements.test.js` 锁定）；`night_owl` 按**本地** 00:00–05:59 判定（`:482` 一族）。宠物（`-pets` + `/api/pets/*` 资产路由）与用量统计无关，只是 usage 端点的消费者；技能（`-skills` 的 `skill_usage` 模式）把技能调用量与安装列表 join，按 `source:"claude"` 计价，返回 `{generatedAt, scannedFiles, totalInvocations, cached, skills[{skill, invocations, lastUsedAt, tokens, cost, installed, skillId, directory}], unusedInstalled[]}`（`src/lib/local-api.js:3337-3408`）——这是用量统计向技能维度的一次外延，但不进主链。

#### 9.4.5 营销口径 vs 统计口径

**源码里不存在「节省金额」类指标**。全仓检索 `savings|amount_saved|saved_amount|total_saved` 共 **9 处命中 / 7 个文件**，其中 `amount_saved|saved_amount|total_saved` 为 **0 命中**；`savings` 的 9 处全部与用量统计无关：`dashboard/src/pages/CloudPage.jsx:95`、`:195`（`cloudAnnualSavings(monthly, annual)`＝**订阅年付相对月付的折扣百分比**）、`dashboard/src/content/i18n/zh/dashboard.json:650`、`zh-TW/dashboard.json:709`、`dashboard/src/content/copy.csv:1699`、`DESIGN.md:17`（订阅折扣徽标）、`scripts/cloud-sandbox/hosted-archive-acceptance.cjs:377`、`:450`（`bill_savings_claimed: false` 免责声明）、`docs/cloud-usage-archive.md:70`。

真正存在**口径错配风险**的是三处：**① Wrapped 的 UTC 偏向**（`peak_hour`/`longest_streak`/`active_days`/`top.days` 全按 UTC，`src/lib/wrapped-aggregator.js:37-53`、`:142-168`）与页面按时区切天的 heatmap 冲突，同一用户同年出现两个「活跃天数」；**② Share card 的 rank 与 token 不同源**（`use-share-card-data.ts:55-63`），未登录时 rank 为 `null`（`:51`）；**③ `wrapped` 用 billable 优先且份额按同一 billable 值算**（`src/lib/wrapped-aggregator.js:29-35`、`:182-186`），而 `usage-daily` 的 `models{}` 用 `total_tokens`（`src/lib/local-api.js:297`）。

### 9.5 云上传与聚合

#### 9.5.1 上传内容（字段级）

客户端请求体（`src/commands/sync.js:4367-4374`）：`{upload_id, hourly: result.buckets, ...(sessionStates.length > 0 ? {account_session_states: result.sessionStates} : {})}`。ingest edge 把 `hourly` 逐列映射为数据库行：

```ts
// dashboard/edge-patches/tokentracker-ingest.ts:176-194（节选）
  const mappedRows = buckets.map((b: Record<string, unknown>) => ({
    user_id: userId,
    device_id: deviceId,
    hour_start: b.hour_start,
    source: b.source || "unknown",
    model: b.model || "unknown",
    input_tokens: b.input_tokens || 0,
    cached_input_tokens: b.cached_input_tokens || 0,
    cache_creation_input_tokens: b.cache_creation_input_tokens || 0,
    output_tokens: b.output_tokens || 0,
    reasoning_output_tokens: b.reasoning_output_tokens || 0,
    total_tokens: b.total_tokens || 0,
    billable_total_tokens: b.billable_total_tokens || 0,
    total_cost_usd: Number(b.total_cost_usd) || 0,
    conversations: b.conversation_count ?? b.conversations ?? 0,
  }));
```

**实际随桶上传的业务字段（12 个）**：`hour_start`、`source`、`model`、`input_tokens`、`cached_input_tokens`、`cache_creation_input_tokens`、`output_tokens`、`reasoning_output_tokens`、`total_tokens`、`billable_total_tokens`、`total_cost_usd`、`conversation_count`（服务端另补 `user_id`、`device_id`）。批内去重：按 `(hour_start, source, model)` 取 `total_tokens` **最大**者（`:196-211`），因为 CLI 队列追加式、同一逻辑桶随会话填充被重复写入且每次携带累计值。

`account_session_states` 是**条件上传的第二种记录**：

| 项 | 规则 | 证据 |
| --- | --- | --- |
| 仅接受来源 | `trae-cn`（其余一律 400 拒收）；客户端侧同样只接受 `trae-cn` | `:139-152`；`src/lib/upload-queue.js:9-18` |
| 字段 | `source`、`session_id`、`model`、`bucket_start`、`snapshot_verified_at` + 六类 token | `:153-160` |
| 身份 | `(user, source, session_id)`——**device_id 不是身份**，同账号多设备经 LWW upsert 收敛到同一行 | `:103-109` |
| 校验 | `total_tokens === input + cached + cacheCreation + output` 双向强制 | `:148-149`、`upload-queue.js:16` |
| 批内冲突 | 同 `(source, session_id)` 只保留最后一条（否则 ON CONFLICT 对同一行触发两次） | `:162-167` |
| 单批上限 | 会话状态 500、桶 500；请求体 1 MiB | `:111-113`、`:172-174`、`:47-49` |
| 只有状态没有桶 | 合法上传，不落 `no-buckets` 拒绝分支 | `:108-109`、`:169-171` |

#### 9.5.2 频率与节流

| 参数 | 值 | 来源 |
| --- | --- | --- |
| 基础间隔 / 抖动 / 积压阈值 | 30 分钟 / ≤60 s / 1,000,000 字节（与 `upload-queue.js:5` 的 `BACKLOG_BYTES` 同值） | `src/lib/upload-throttle.js:2-4` |
| 批量 | 客户端读取默认 200；后端 batch 300；ingest 上限 500 | `:5`、`src/commands/sync.js:4439-4441`、`tokentracker-ingest.ts:172-174` |
| 批量档位 | 小 2 批 / 大 4 批 | `:6-7`、`:57` |
| 失败退避 | 60 s 起、上限 30 分钟、指数 `2^step`、step 上限 10（记录时 20） | `:8-9`、`:109-110`、`:123` |
| 服务端截止 | `Retry-After` 优先于配置上限；`cloud_sync_throttled` + 429 时以服务端时间为 `nextAllowedAtMs` | `:104-107`、`:114-116` |
| 生效判定 / 云侧下发 | `max(nextAllowedAtMs, backoffUntilMs)` 未到即拒绝；云侧 `sync_interval_seconds` 与 `next_allowed_at` 取更晚者 | `:46-55`、`:72-77` |
| 心跳 | 24 小时 + 60 s 抖动 | `src/lib/telemetry.js:41-46`、`:116-124` |

上传是**按字节偏移的追加流**（`queue.state.json` 记偏移，`status` 的 `pending_bytes` 即「文件字节数 − 偏移」），重复上传靠云端批内 MAX 与 RPC 幂等兜底。

#### 9.5.3 边缘函数与云端再聚合

`dashboard/edge-patches/` 共 **28 个文件**（全部 `.ts`），关键项：

| 文件 | 角色与要点 |
| --- | --- |
| `tokentracker-ingest.ts`（225 行） | 用量写入。Bearer = device token → sha256 → 查 `tokentracker_device_tokens`（`:69-82`）；封禁用户 403（`:92-94`）；调 `cloud_ingest_usage` RPC 带 `p_upload_id`（`:214-218`） |
| `tokentracker-telemetry.ts`（85 行） | 匿名心跳；`machine_hash` 必须匹配 `/^[0-9a-f]{64}$/`（`:25`、`:53`）；注释声明「one row per (machine_hash, day)」且「cannot be joined to device rows」（`:6-9`） |
| `tokentracker-account-summary.ts`（954 行） | 云端概览，**显式声明镜像本地响应 schema**（`:3`、`:12`）；用 `account_summary_wire` RPC（`:690`）并解码紧凑线格式（`:623`）；**自带一份定价表与 `getModelPricing`**（`:398`、`:524`、`:718`）；跨设备去重（`:22`）；交叉设备 30 s 共享缓存（`:662`） |
| `-account-daily.ts`（894）/ `-hourly.ts`（333）/ `-monthly.ts`（261） | 日/小时/月镜像 |
| `-account-heatmap.ts`（391） | 热力图镜像；`format=compact`（`:114-147`）；30 s 缓存 + 5 分钟 stale-if-error（`:145-148`） |
| `-account-model-breakdown.ts`（956）/ `-account-devices.ts`（268）/ `-account-skills.ts`（251） | 模型/设备/技能维度镜像 |
| `tokentracker-leaderboard.ts`（262） | 读榜单快照；`period=week\|month\|total` 决定 `(from_day, to_day)`（`:97`、`:131-152`）；按 `rank` 升序分页（`:167-181`）；附徽章数据 RPC（`:220`） |
| `-leaderboard-refresh.ts`（1449）/ `-leaderboard-profile.ts`（1074） | 榜单快照生成（含异常账号排除）；个人公开档案页数据 |
| `-community-models.ts`（150） | 社区模型统计，**读预计算表 `tokentracker_community_stats`**（`:84-85`），测试要求不得回退全表扫描（`test/community-stats-cache.test.js:16-20`） |
| `-public-visibility.ts`（210）/ `-profile-likes.ts`（354） | 公开可见性与点赞 |
| 设备令牌族 / 计费族 | `-device-token-issue.ts`（183）+ 3 个 device-flow + `-device-rename.ts`（164）；`-billing.ts`（492）+ 4 个 webhook（支付宝/Paddle/Waffo/微信，不在统计范围） |

**云端另有一份 schema 镜像与一份定价实现**。`test/edge-pricing-parity.test.js`（420 行）就是为防止两份定价漂移，但其第一层只做 edge↔edge 比对，不含「edge 覆盖本地 curated 全量」的断言——「本地算得对」推不出「云端也对的」。

#### 9.5.4 社区/排行榜与本地口径的五处差异

1. **数据总体不同**：本地看本机 `queue.jsonl`；榜单看「所有已上传设备」的快照表，未登录/未同步的历史在云端不存在，云端也可能含本机未见的设备。
2. **去重语义不同**：账号级来源（`cursor`、`trae-cn`）跨设备**去重不求和**，本地单机视角没有这个问题；一致性由 `test/account-source-parity.test.js:3-18` 强制。
3. **时间窗口不同**：榜单按 `(period, from_day, to_day)` 快照查询（`tokentracker-leaderboard.ts:131-152`），窗口由服务端刷新任务写入；本地端点按请求参数实时算。
4. **成本口径可能不同**：云端自带定价实现（`-account-summary.ts:398`、`:524`），与本地 `src/lib/pricing/` 是两份代码。
5. **可见性不同**：榜单只展示开公开资料的用户；关闭后从榜单移除并把徽章变成占位（`docs/PRIVACY.md:113`）。

#### 9.5.5 用量归档

热表保留小时级原始行；冷档按 `(user, device, UTC day)` 打成私有 PostgreSQL JSONB 包（`docs/cloud-usage-archive.md:3`）。读取路径 `cloud_usage_hourly(user, device, from, to)` 先按用户/设备/UTC 日筛 manifest，再展开 JSONB，并按 `(user_id, device_id, hour_start, source, model)` + 服务端修订号降序合并冷热（`:17`），仅 `project_admin` 可执行。prepare/commit 两步：`cloud_prepare_usage_archive`（单包上限 10,000 条 / 8 MiB）与 `cloud_commit_usage_archive(generation)`，后者只在「修订号仍等于快照」时删热行（`:35-39`）。**可见性限制 ≠ 删除**（个人小时级 90 天、日/月级 24 个月，`:13`）。本地 runner `scripts/cloud-usage-archive.cjs` 默认 dry-run，`--apply` 才写，凭据不入 checkpoint（`:48-53`）；文档末尾明确**尚未启用任何调度**、未迁移或删除生产数据（`:5`、`:74-83`）。

#### 9.5.6 隐私边界核验（逐字段）

| 实际上传项 | 内容 | 结论 |
| --- | --- | --- |
| 小时桶 | 9.5.1 所列 12 个业务字段 + `user_id`、`device_id` | 与文档「小时桶」方向一致，**字段枚举未逐字覆盖** |
| `billable_total_tokens` | 「计费口径 token」 | **文档未列**（`tokentracker-ingest.ts:188`） |
| `total_cost_usd` | 金额 | **文档未列**（`:189`） |
| `account_session_states`（仅 trae-cn） | 含 `session_id`、`model`、`bucket_start`、`snapshot_verified_at` + 六类 token | **文档「Never sent: Local session records」一句与此存在字面张力**；字段中无 prompt/对话正文 |
| 心跳 | `machine_hash`、`app_version`、`platform`、`shell`（`src/lib/telemetry.js:75-82`） | 与文档一致；`machine_hash` 版本化、无法反推 device 行（`:34`） |
| 设备注册 | `machineId` + **主机名作为 `device_name`**（`src/lib/local-api.js:3137-3143`、`:31-41`，截断 128 字符） | **文档写了 machine id，未写「主机名会作为设备显示名」**；云端设备页与榜单会出现该名称（`dashboard/src/lib/device-label.js`、`dashboard/src/lib/api.ts:1054-1063`） |
| 不上传项核验 | `session.queue.jsonl` / `project.queue.jsonl` 不在上传路径内（`src/commands/sync.js:3656` 注释逐字「Only the main queue is uploaded」） | 一致 |

需要披露的三处文档未覆盖实现细节：**① `billable_total_tokens` 与 `total_cost_usd` 随桶上传**；**② `account_session_states` 携带 `session_id`**（仅 trae-cn，需本地已登录该应用，无对话内容）；**③ 设备注册上传 `machineId` + 主机名作为设备显示名**。另有三条实现比文档更严（不构成风险）：心跳可由 `TOKENTRACKER_NO_TELEMETRY`/`DO_NOT_TRACK`/`config.telemetry=false` 任一关闭（`src/lib/telemetry.js:48-53`），前端「问不到偏好就关」（`dashboard/src/lib/analytics.js:13`、`:67`）；会话浏览器端点仅本地（`src/lib/local-api.js:2495-2499`）；`summarizeSessions` 默认剥离 `project_ref`/`session_id`/`title` 且 `provenance.privacy = "metadata-only"`（`src/lib/session-analytics.js:2526-2543`、`:2561`）。

### 9.6 三端原生壳如何拿到统计

#### 9.6.1 端口矩阵与理由

| 端 | 端口 | 证据 |
| --- | --- | --- |
| CLI `serve` | 7680，忙则顺延（最多 20 次） | `src/commands/serve.js:14`、`:22`、`:208-215` |
| CLI under WSL | 7681（7680 被 Windows DoSvc 占用） | `src/commands/serve.js:15-21`、`:202-206` |
| macOS app | 固定 7680（`NativeQAProfile` 可覆盖） | `TokenTrackerBar/TokenTrackerBar/Utilities/Constants.swift:4-5` |
| Windows app | 运行时挑空闲回环端口，**优先 17680** | `TokenTrackerWin/ServerManager.cs:469-476`、`:157` |
| Linux app | 固定 17680 | `TokenTrackerLinux/gnome-extension/tokentracker@tokentracker.cc/extension.js:18-19`；`TokenTrackerLinux/src-tauri/src/server.rs` |

Windows 偏好 17680 的理由是三条约束的交集（源码注释逐字，`TokenTrackerWin/ServerManager.cs:471-476`）：OAuth 回调 `http://127.0.0.1:<port>/auth/callback` 必须在 InsForge 白名单里（动态端口做不到，故用固定端口，与 macOS 的 `:7680` 并列登记）、落在 IANA registered 段（10000–49151，Windows 不会当临时端口发出去）、避开 DoSvc 占用的 `:7680`。

#### 9.6.2 macOS：spawn CLI serve + 原生视图

启动内嵌 server：找 CLI 入口后 spawn `[entryPath, "serve", "--port", "<7680>", "--no-sync", "--no-open"]`（`TokenTrackerBar/.../ServerManager.swift:195`），走登录 shell 的兜底分支为 `zsh -lc "<bin> serve --port <7680> --no-sync"`（`:228`），启动前用 `lsof -ti tcp:<port> -sTCP:LISTEN` 清占用者（`:83-99`）。**数据入口是 HTTP，不是直读文件**：

| 方法 | 路径 | 行号（`TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift`） |
| --- | --- | --- |
| `fetchSummary` / `fetchDaily` | `-usage-summary?from&to` / `-usage-daily` | `:43-52` / `:66-69` |
| `fetchHeatmap` | `-usage-heatmap?weeks=52`（`tz`/`tz_offset_minutes`/`account=1` 由 helper 追加） | `:73-77` + `:241-254` |
| `fetchModelBreakdown` / `fetchProjectUsage` | `-usage-model-breakdown` / `-project-usage-summary` | `:79-82` / `:86-90` |
| `fetchMonthly` / `fetchHourly` | `-usage-monthly` / `-hourly?day=` | `:94-97` / `:101-105` |
| `fetchUsageLimits` / `fetchSubscriptions` | `-usage-limits`（`devin=1` 可选）/ `-subscription-manager` | `:110-113` / `:119-121` |
| `triggerSync` / `checkServerHealth` | `-local-sync` / `-user-status`（只判 200） | `:127-143` / `:148-150` |

所有请求都经查询串 helper：`withTimeZoneQueryItems` 追加 `tz` + `tz_offset_minutes`（`APIClient.swift:241-246`），`withAccountQueryItems` 在其上再追加 `account=1`（`:252-254`）；`fetchSummary`/`fetchHeatmap`/`fetchModelBreakdown`/`fetchMonthly`/`fetchHourly` 用后者，`fetchProjectUsage` 用前者——**macOS 壳不缺 tz**（这是判断 9.1.5 冲突是否在真实路径生效的前提）。`fetchHourly(day:)` 的 `day` 来自 `DateHelpers.dayRange`（`DateHelpers.swift:44-50`，`.current` 时区的 `yyyy-MM-dd`），调用点 `DashboardViewModel.swift:192-194`/`:311`，与随请求发出的 `tz` 同基准。

菜单栏/弹出层由 `StatusBarController`（1507 行）与 `DashboardViewModel`（1151 行）驱动，后者是唯一取数状态机。小组件（`TokenTrackerWidget/`）**不联网**，读 App Group 里的快照：容器 `group.com.tokentracker.bar`、文件 `widget-snapshot.json`、schema 版本 2（`TokenTrackerBar/Shared/WidgetSnapshot.swift:18-30`）；字段含 `serverOnline`、`today`/`last7d`/`last30d`/`total`/`selected` 的 `PeriodTotals`、`dailyTrend[]`、`topModels[]`、`sources[]`、`heatmap`、`limits[]`（`:35-66`）；写入时机为 `DashboardViewModel.loadAll()` 后 `WidgetSnapshotWriter.update(from:capturedAt:)` 原子写入并 `WidgetCenter.reloadAllTimelines()`（`WidgetSnapshotWriter.swift:54-110`）。**唯一直接触碰本地文件的路径**是活动监听（不解析内容）：`QueueActivityMonitor` 以 `O_EVTONLY` 打开 `queue.jsonl` 监听写入/替换，第二实例监听 `queue.state.json` 发布「上传完成」（`QueueActivityMonitor.swift:1-36`）。

#### 9.6.3 Windows：宿主定端口，托盘自建 HttpClient

端口由宿主决定：`Port = PickServerPort()`、`BaseUrl => http://127.0.0.1:{Port}`（`ServerManager.cs:12-20`、`:34-38`、`:157`）；健康循环 30 s、启动等待 20 s（`Constants.cs:14-18`）。托盘取数用 `UsagePoller`（自建 `HttpClient`，`UseProxy=false`，6 s 超时，`:38-40`）：`FetchLimitsAsync` → `-usage-limits`（`:182-187`）；`FetchAsync` → `-usage-summary?from&to...`（`:226-250`）；`FetchHeatmapAsync` → `-usage-heatmap?weeks=52&tz...&account=...`（`:316-328`）；`FetchTopModelsAsync` → `-usage-model-breakdown?...`（`:340-354`）。UI 与桥接：`DashboardWindow.cs`（WebView2，1235 行）、`WidgetWebViewEnvironment.cs`、`QuotaWidgetWindow.cs`（配额小组件）、`TrayApplicationContext.cs`（托盘与菜单）、`LocalSyncPublisher.cs`（同步结果广播）。宿主还驱动后台同步（`ServerManager.cs:313` 在服务器就绪后发布），与 CLI 侧 `TOKENTRACKER_NATIVE_SYNC_OWNER=windows-host` 判定互斥（`src/commands/serve.js:305-309`），避免双份自动采集。

`【未能确认】` `TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源（判定条件：读该文件；推测同为本地 API 或经 `UsagePoller` 推送，未验证）。

#### 9.6.4 Linux：Tauri 壳 + GNOME 扩展

Tauri 壳 `src-tauri/src/server.rs` 负责 spawn/记录/回收内嵌 server（每进程一个 `server-{owner_pid}.json`，含 pid/port/boot_id/进程启动时间，`:138-160`、`:246-257`；`reap_orphaned_servers()` 启动时清理孤儿，`:343-372`）；`main.rs`、`tray.rs`、`pet.rs`、`cloud_export.rs` 分别负责窗口/托盘/桌宠/云导出。GNOME 扩展直连 HTTP（`const BASE_URL = 'http://127.0.0.1:17680'`，`extension.js:18-19`）：

| 用途 | 路径 | 行号 |
| --- | --- | --- |
| 周期数据 | `-usage-model-breakdown`、`-hourly?day=`（`day` = `dayString(new Date())`，本机时区当日，`:1044`/`:273-275`）、`-monthly` | `:1046-1051` |
| 今日摘要 / 全量视图 | `-usage-summary?from=today&to=today`；`-usage-summary`（total）、`-daily`、`-heatmap?weeks=52` | `:1086`；`:1110-1116` |
| 限额 / 触发同步 | `-usage-limits`；`POST -local-sync` | `:1137`；`:1025` |

扩展把「非 404 的 HTTP 错误」与「端口被别的服务占用（404 → `ForeignServerError`）」区分开（`:229-245`），避免读到别人的服务；所有 `/functions/` 读取都经 `_getJson`，自动拼 `tz`、`tz_offset_minutes` 与 `account=1`（`:997`）。

#### 9.6.5 是否存在绕过 serve 直读队列

| 途径 | 是否直读 `queue.jsonl` | 证据 |
| --- | --- | --- |
| macOS 壳 | 只做文件事件监听，**不解析内容** | `QueueActivityMonitor.swift:1-36` |
| Windows 壳 | 未发现；文件级接触限于 `~/.tokentracker/pets` | `TokenTrackerWin/PetWindow.cs:1042`、`TrayApplicationContext.cs:566` |
| Linux 壳 / GNOME 扩展 | 未发现；全部经 HTTP | `extension.js` 的 `_getJson` 一族 |
| **Vite dev 插件** | **是**——直读、自行去重、自行日聚合 | `dashboard/vite.config.js:418`、`:570`、`:583-639` |

### 9.7 与用量统计的边界

#### 9.7.1 限额链路与 token 统计完全独立

`-usage-limits` 走「provider 官方 API + 本地磁盘缓存」，**从不进入 `queue.jsonl`**，在数据源、聚合、缓存、展示四层都不相交：

| 维度 | 事实 | 证据 |
| --- | --- | --- |
| 抓取入口 | `fetchUsageLimitsUncached()` 并行拉取多家 provider | `src/lib/usage-limits.js:3976`、`:4055` |
| 进程内缓存 | 按「选择键」分区，TTL 2 分钟（`CACHE_TTL_MS`） | `:64` |
| 选择键 | 二元组 `${devin}-${antigravity}`：`devin` 由 `devinEnabled === true` 决定，`antigravity` 由 `isAntigravityQuotaDisabled()` **取反**决定；分区枚举 `off-on`/`off-off`/`on-on`/`on-off` | `:56-63` |
| 并发合并 | single-flight（in-flight 表），同一键只抓一次 | `:3954-3970`、`:3962-3968` |
| 磁盘缓存 | 每家 provider 一份 `*-usage-limits-cache.json`，最长保留 7 天；Claude 另有新鲜度 10 分钟与 429 冷却 | `:71`、`:118-129`、`:120`、`:141-144`；`docs/PRIVACY.md:43` |
| provider 数 | `Promise.all` 解构 18 个、`data` 收录 17 家（`claudeServiceStatus` 未进 `data`） | `:4055`、`:4406-4440` |
| 可信度元数据 | 注入 `stale`（显式或 `cached_at` 早于 10 分钟）、`source`（显式，否则 `disk-cache`/`provider-api`）、`confidence`（来源串匹配 `estimate\|inferred` → `inferred`；匹配 `local\|database\|sqlite\|cache` → `observed`；否则 `official`），另附 `captured_at`、`age_seconds` | `:4442-4461` |
| 窗口结构 | 统一 `buildWindow({usedPercent, resetAt, windowSeconds})`，百分比经 `clampPercent`；Codex 有 5 小时（18000 s）与每周（604800 s） | `:161`、`:146`、`:332-333` |
| 前端展示与一致性 | `LimitsPage.jsx` 直接 `Object.entries(data)` 铺平 provider 行，用 `provenance.stale` / `source==="disk-cache"` 判定「全部来自缓存」；前后端 provider 清单由 `test/limits-providers-parity.test.js` 锁定 | `dashboard/src/pages/LimitsPage.jsx:49`、`:59-66` |

**结论**：限额视图是「配额百分比与重置时间」，用量统计是「token 与成本」；两者唯一耦合是共用同一个 `serve` 进程与前缀路由。

#### 9.7.2 其它相邻但不属于统计的边角面

| 面 | 归属 | 与用量统计的关系 |
| --- | --- | --- |
| 成就/徽章 | 本地阈值在服务端、达成时间按 `hour_start` 回放；云端徽章读榜单快照 | 消费主链行，不改用量口径 |
| 宠物（桌宠） | UI 与资源包 | 只是 usage 端点的消费者 |
| 技能（`-skills` / `skill_usage`） | 独立聚合 + `source:"claude"` 计价 | 一次 token 外延统计，不进主链 |
| 会话效率（`-sessions` / `-session-insights`） | 独立第二条链（`session-analytics.js`） | 产出 token 与成本，但**与主链不同源**，不可直接对账 |
| 计费/订阅/webhook | `tokentracker-billing.ts` + 4 个 webhook | 商业链路，与 token 统计无关 |
| 代理/geoip/iprisk/avatar | 纯网络工具端点 | 与统计无关 |
| SCIP 代码图索引 | `graph-auto-index-*` | 与统计无关 |

#### 9.7.3 口径对账顺序（推荐）

要与其他工具对账 TokenTracker 的数字，按此顺序定位分歧命中率最高：**① 时区基准**（`wrapped` 恒 UTC；Dashboard 与三端壳按传参 tz；裸调 API 分两族）；**② 去重语义**（本地 last-wins vs 云端批内 MAX）；**③ token 口径**（`billable_total_tokens` vs `total_tokens`）；**④ 来源范围**（`scope` 是否含账号级来源 `cursor`/`trae-cn`；账号视图是跨设备去重而非相加）；**⑤ 是否账号视图**（看 `X-TokenTracker-Account-View` 与 `-Fallback` 头）；**⑥ 成本来源**（本地逐行计价求和 vs 云端自带定价实现）。

#### 9.7.4 本章未能确认项与判定条件

| # | 未确认内容 | 判定条件 |
| --- | --- | --- |
| 1 | Windows 托盘在本地模式下「连续天数」的实际显示值（源码可证本地端点恒 0、托盘直读该字段） | 运行 Windows 壳 → 非账号视图 → 观察托盘/小组件；若显示真实值则说明另有来源 |
| 2 | 云端 `tokentracker-account-heatmap.ts` 是否返回真实 `streak_days` | 读该文件全篇，或账号视图下抓响应体查 `streak_days` |
| 3 | `-local-sync` 成功响应中 `syncResult` 的具体键名 | 读 `src/commands/sync.js` 的 drain 返回值；本章只确认 `{ok, uploaded, pending, next_allowed_at?}` 与失败形状（`src/lib/local-api.js:2322`、`:2324`） |
| 4 | `-context-health` 的字段结构 | 读 `src/lib/context-health.js` |
| 5 | `/api/auth/*` 的完整转发规则（cookie relay、csrf 优先级、relay 回退条件） | 读 `src/lib/local-api.js:1792-1940` 全段 |
| 6 | `src/lib/git-outcomes.js` 的「只用 subject 检测 revert」是否逐行成立 | 读该模块并与 `docs/PRIVACY.md:21` 对照 |
| 7 | Vite dev 中间件对外暴露的路径前缀集合 | 读 `dashboard/vite.config.js` 中 `configureServer` 的中间件注册串 |
| 8 | `usage-limits` 的 provider 计数口径；`ark` 两家与 `qoder` 两家是否共用凭据 | 读 `:4055` 解构段与 `:4406-4440`、各 `*-limits.js` 凭据读取 |
| 9 | 榜单/社区表的 SQL 细节 | 读 `scripts/ops/*.sql`、`migrations/` |
| 10 | 「compressed usage metrics 可长期保留」的保留期限数值 | 源码/文档未见最终数值；`docs/cloud-usage-archive.md:74-83` 明确未启用 |
| 11 | macOS 小组件刷新频率与快照 schemaVersion 2 的兼容迁移细节 | 读 `WidgetSnapshotWriter.swift` 与 Widget 侧读取段 |
| 12 | `TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源 | 读该文件；推测同为本地 API 或经 `UsagePoller` 推送，未验证 |

上述 12 项均不影响本章已给结论：每条结论要么有源码行号支撑，要么已在本表标明边界。

## 10. 与 TokenScope 的对照：可抄与需避

以下判断只做**工程可移植性**评价，不做风格评价。行号为样本仓库 `xiufengsun/TokenTracker`（HEAD `b19d27b0`，v1.3.14）的实际行号，口径为 **UTF-8 解码 + 按 `\n` 分行**。带行号的源码事实与跨语言/跨平台的工程推断**不是同一证据等级**——凡属后者的，本节标注「工程推断」。

### 10.1 值得直接抄的实现细节

排序遵循「并发正确性 > 数据完整性 > 性能 > 兼容性」。

**① 锁协议：独占创建 + 持有者令牌 + 心跳 + 独立回收闸**

问题：多个生产者（hook 触发的 sync、后台轮询、本地 API 触发的 sync、自动重试）会同时抢跑同一份解析，还要处理 `kill -9` 残留、PID 复用、以及「回收残留锁」这个动作自身的互斥。

要点（`src/lib/fs.js:69-74,284-414`）：`LOCK_STALE_MS = 5 min`、`LOCK_HEARTBEAT_MS = 30 s`、`MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64`（`:69-72`）。
```js
const handle = await fs.open(lockPath, "wx");        // O_EXCL 语义
const token = crypto.randomUUID();
const heartbeatPath = heartbeatPathFor(lockPath, token);   // ${lockPath}.heartbeat.${sha256(token)}
await handle.writeFile(JSON.stringify({ pid: process.pid, token, createdAt }) + "\n", "utf8");
```
- 心跳 30s 一次，同时 `utimes` 锁文件与心跳文件，`timer.unref()`（`:206-224`）。
- 回收判定：先看 PID 是否存活（`process.kill(pid, 0)`，`ESRCH` 才算死，`EPERM` 保守当作存活），再看 **token 专属心跳**是否超过 `LOCK_STALE_MS`；老格式锁（无心跳）退回 mtime 超期判断（`:105-144`）。
- 回收动作本身用 `.reclaim` 次级锁串行化，且**持闸后重新校验一次**（TOCTOU，`:342-368`）。
- 释放时同样持闸，**只有 token 匹配才 unlink**（`:264-275`）；先关句柄再删文件（Windows 上文件被打开时无法 unlink）。

动机：单纯的「PID 存活 + 时间阈值」无法区分 **PID 复用**与**长时间运行但健康的锁**；引入 token 专属心跳后，「这个锁还活着吗」被解耦成「这个 token 的心跳还新鲜吗」。`.reclaim` 闸解决的是多个收割者抢同一份残留的竞态。
移植（工程推断）：Rust 用 `OpenOptions::new().write(true).create_new(true).open()`；令牌用 `uuid`；心跳用 `tokio::time::interval` 并显式 `abort()`。进程存活无 std 等价物——Windows 用 `OpenProcess(SYNCHRONIZE)` + `WaitForSingleObject(0)`，或 `QueryFullProcessImageName` 校验身份。**Windows 上 PID 复用更频繁，务必配 token 心跳，不要只信 PID。** 若跨机共享锁（NFS/SMB），`.reclaim` 基于 `rename` 的原子性假设会失效。

**② 目录探测：把「存在」定义为「能被列举」**

问题：只做 `stat` 会把权限 `000`（或 ACL 拒绝）的目录判为「存在」，进入扫描后产出 0 个文件，在状态页显示「存在」，还会**绕过 deferred-repair 守卫**（守卫语义是「根不存在就不许重算历史」）。

要点（`src/lib/scan-roots.js:89-110`）：
```js
if (!statSync(target).isDirectory()) return { exists: false, error: null };
// Probe listability without reading entries; large roots stay cheap.
opendirSync(target).closeSync();
return { exists: true, error: null };
// catch: ENOENT/ENOTDIR → {exists:false,error:null}；其余 → {exists:false,error:code}
```

三态语义是重点：`{exists:true}` / `{exists:false,error:null}`（真不存在）/ `{exists:false,error:"EACCES"}`（存在但读不了）。第三种在 UI 上必须显示 `unreadable: EACCES` 而不是 `missing`（`:248-253`）。

动机：`opendir().close()` 只取目录句柄、不读条目，成本与 `stat` 同阶，却把「能否读取」变成可证事实。区分后调用方能做**不同决策**——不存在可跳过，读不了必须推迟重算。
移植（工程推断）：Rust 用 `std::fs::read_dir(path)` 后立刻 drop 迭代器即可。**不要用 `Path::exists()`**——它把「不可读」和「不存在」都吞成 `false`。把 `ErrorKind::NotFound` 与 `PermissionDenied` 分开映射。

**③ 追加型文件的读侧：签名缓存 + 锚点续读 + 显式让出**

问题：一次刷新会并发打多个端点，每个都要读同一个 append-only 的 `queue.jsonl`（可能几十 MB）。朴素做法每次全量读 + 全量 `JSON.parse`，直接打满事件循环。

要点（`src/lib/async-queue-reader.js`）：文件签名为 `${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:${stat.ctimeNs}`（`:8-10`，bigint stat）。
```js
// :40-47 只有"同 dev:ino + size 变大 + 头部与锚点内容一致"才从 consumedBytes 续读
if (cached && cached.identity === identity && size > cached.size && cached.consumedBytes > 0) {
  const head = await probe(file, 0, cached.head.length);
  const anchor = await probe(file, cached.consumedBytes - cached.anchor.length, cached.anchor.length);
  if (head.equals(cached.head) && anchor.equals(cached.anchor)) { offset = cached.consumedBytes; seen = cached.seen; }
}
```

动机：三个独立问题被分开解决——重复读用签名缓存；增量读用**锚点探针**而不是「相信 offset 仍有效」（文件可能被原子替换或压缩重写）；事件循环饥饿用显式让出。最妙的是残行策略：**「可见行集合」与「已消费字节位置」分离**，读者不需要回退，也不会把半行当数据。
移植（工程推断）：`tokio::task::yield_now()` 与 `setImmediate` **语义不完全等价**——前者只让出一次 poll，后者保证让出到下一个事件循环轮次。若读侧有同步 SQLite 访问，光让出没用，必须把重活丢到 `spawn_blocking`。`mtimeNs` 只有 Node 的 `stat({bigint:true})` 才给；Rust `Metadata` 只有 `SystemTime`，**不要假设时间戳有纳秒唯一性**，强判据要用内容锚点。

**④ 压缩追加型大文件：行边界 + 原子替换 + ino/size/mtime 三重校验**

问题：append-only 队列会无限增长（历史重算的日子同一小时键被反复重写），需要在不打断写者、不破坏上传字节 offset 的前提下删掉同键旧行。

要点（`src/lib/upload-queue.js:64-132`）：
```js
// :73-78 只在"够大 + offset 落在行边界"时动手
if (!before.isFile() || !Number.isSafeInteger(offset) || offset < 0 || offset >= before.size ||
    pendingBytesBefore < minPendingBytes) return unchanged;   // minPendingBytes 默认 1_000_000
if (offset > 0) {
  const boundary = Buffer.alloc(1);
  await input.read(boundary, 0, 1, offset - 1);
  if (boundary[0] !== 10) return unchanged;                   // offset-1 必须是 '\n'
}

// :111-121 替换前后的竞态校验
await output.sync(); await output.close(); output = null;
await input.close(); input = null;   // Windows: 目标仍被打开时无法替换
const current = await fs.stat(queuePath);
if (current.ino !== before.ino || current.size !== before.size || current.mtimeMs !== before.mtimeMs) {
  throw new Error("Queue changed during pending compaction");
}
await fs.rename(temporary, queuePath);
```

保留语义显式：`[0, offset)` 原样字节复制（已上传历史不变），`[offset, size)` 里每个键只保留**最后一条**（`:79-91,105-110`），且 `account_session_state` 用 `verifiedMs` 严格更大而非「最后一条」（防时钟回拨）。常量：`BACKLOG_BYTES = 1_000_000`、`CHUNK_BYTES = 64 * 1024`（`:5-6`；该文件全文 134 行）。

动机：直接 `truncate`/重写整文件会破坏 `queue.state.json` 的字节 offset 语义。只在 pending 区整理，把「压缩」与「上传进度」完全解耦。最后的三重校验是兜底——**不校验的话，一次并发 append 就会永久丢失刚写入的行**。
移植（工程推断）：Rust 用 `create_new(true)` 建临时文件 → 写 → `sync_all()` → `drop` 读句柄 → `fs::metadata` 校验 → `fs::rename`。`fsync` 在压缩场景必需，否则掉电可能出现「新文件存在但半写」。别用「读全文件到内存再写回」替代——队列可能几百 MB，且会放大竞态窗口。

**⑤ 原子写 JSON：tmp 名带 UUID + rename + 失败清理**

问题：并发写同一个 JSON 状态文件会互相踩（半写、丢失、`Date.now()` 冲突）。项目里有多个此类文件：`queue.state.json`、`upload.throttle.json`、`config.json`、`cursors.json`、`sync.skip.json`。

要点（`src/lib/fs.js:9-33`）：
```js
const tmp = `${filePath}.tmp.${Date.now()}.${crypto.randomUUID()}`;
try {
  await fs.writeFile(tmp, content, mode == null ? { encoding: "utf8" } : { encoding: "utf8", mode });
  await fs.rename(tmp, filePath);
} catch (err) { try { await fs.unlink(tmp); } catch { /* tmp may not exist */ } throw err; }
```

配套：`updateJsonLocked` 用 `<file>.lock` 做读-改-写整体互斥，超时 30s（`:416-443`），写后尽力 `chmod 600`。

动机：`rename` 在同一文件系统内原子，读者永远看到完整旧版或完整新版。tmp 名追加 UUID 是因为只靠 `Date.now()` 会在同一毫秒内冲突，两个写者写同一 tmp 并互相覆盖。失败路径必须删 tmp，否则崩溃会留下 `*.tmp.*` 污染目录枚举。
移植（工程推断）：tmp 必须与目标**同目录**（`std::fs::rename` 跨文件系统会 `CrossesDevices`）。Node 的 `mode` 只在创建时生效（源码注释特别说明，`:15-17`），Rust 用 `OpenOptions::mode(0o600)`。**若同一个 JSON 有多个写者，光有原子写不够**，还需要读改写锁。

**⑥ 三代保留 + 清单原子切换的代际存储**

问题：游标状态（几十万文件游标 + 事件去重集合）既大又关键。单 JSON 更新有两个问题：每次提交全量重写（写放大）；写一半崩溃得到损坏状态且无回退点。

要点（`src/lib/cursor-store.js:13-23,855-1016`）：`STORE_VERSION = 2`、`STORE_DIRNAME = "cursor-store-v2"`、`MANIFEST_FILENAME = "manifest.json"`、`GENERATIONS_DIRNAME = "generations"`、`SHARDED_CORE_FILENAME = "core-v3.json"`（`:13-18`）。

提交流程（`:855-989`）：新代目录 `gen-<ms>-<uuid8>` → 从不需改写的旧代分片整文件克隆 → 只重写脏分片 → 写 `core-v3.json` → 写 `generation.json`（含各分片 count 与完整性摘要）→ **最后**原子替换 `manifest.json`（`:977-990`）。未 publish 前任何失败都会 `fs.rm(generationDir,{recursive:true,force:true})` 清理半成品（`:1010-1015`）；成功后只保留 `current/rollback/previous` 三代（`:1005-1009`）；读取侧依次尝试 `current → rollback → previous`（`:124-139`）；分片完整性用 metadata 摘要校验（`:1466-1481`）。

动机：「清单指向哪一代」是唯一的原子切换点，代目录内部的多分片写入无需原子——因为**还没被任何人指向**。这让「大状态 + 多分片 + 崩溃安全」同时成立，代价是磁盘上最多 3 份数据。
移植：Rust 侧写完要 `sync_all` 目录（Linux 需要，Windows 无需）再替换 manifest；摘要用 `blake3` 写进 metadata。**分片键必须白名单化再拼路径**（源码用 `safeShardFilename` 只允许 `^\d{4}-\d{2}-\d{2}$`，否则写 `misc`，`:1807-1809`），否则来自文件路径的分片键就是路径穿越。要看清克隆语义：`cloneOrCopy` 用 `fs.copyFile(..., COPYFILE_FICLONE)`（`:1361`）= **CoW/reflink，与硬链接无关**，不支持时回退普通复制；真正需要防守的是「对共享存储做原地改写」——本项目脏分片是**整文件重写**（先 `unlink` 再 create，`:907`），所以旧代始终自洽。Rust 侧对应要求：分片重写必须「先删再建或写临时文件后 rename」，不要 `truncate` 后原地覆盖。

**⑦ 一次性历史重算的「数据不全就不修」守卫**

问题：口径变化后唯一正确的修法是按当前口径重算历史，但重算前提是**磁盘上还留着当初的数据**——这个前提经常不成立（用户清过日志、产品自身滚动删除、归档目录不在扫描范围、远端根临时不可用）。项目在这里翻过车：0.26.3 的重算把 `~/.claude` 日志已被清理的小时从队列中**静默抹掉**，一位用户损失 21.7 亿 claude token（`src/commands/sync.js:234-248`；源码自述分项 `-1.27B / -474M / -376M / -48M / -6M` 求和 = 2174M ≈ 2.174B，与总数自洽）。

要点——三件套守卫 + 一条「推迟而非跳过」原则：
- **可行性预检**：每个 repair 先用廉价信号判断能否执行（`:258-268` 的注释即契约）。
- **扫描根可用性守卫**：非 native 的根当前 `!exists` 或不可读、且历史上贡献过文件 → **推迟**（不设迁移键，等下次全量再试），普通扫描照常（`:1147-1192`）。
- **重建前后文件快照校验**：重建前记录每个输入文件的快照（`readCodexFileSnapshot`/`sameCodexFileSnapshot`，`:6271-6293`），重建后逐文件比对（`validateCodexRebuildFileSnapshots`，`:6294-6314`），并检查「历史是否被盘上数据覆盖」（`isCodexHistoryCovered`，`:6316-6336`；`:6338-6346` 已是下一函数 `scanForForkedCodexRollout` 的头注释块）。
- **迁移键即幂等闸**：`cursors.migrations[KEY]` 存在即不再跑；失败路径**故意不写**该键，让下次全量重试（`:952-959`）。

动机：一次性迁移有两类失败模式——「该修没修」（可恢复）与「不该修的修了」（**不可恢复的数据丢失**）。守卫的取舍是宁可推迟，绝不基于残缺数据重写历史。这直接推翻了「全量重算=安全」的直觉。
移植：任何 `DELETE`/重写历史的操作都应先证明「新数据覆盖了旧数据的全部时间范围」。**「推迟」要显式可观测**——源码会打印 `Claude ground-truth repair deferred: ...`（`:1177-1181`），但**该行提示被 `if (!opts.auto)` 包住**，即 `--auto`（hook / 后台轮次）下是静默推迟的；要后台留痕应写 marker / 状态文件而不是仅靠 stderr。

**⑧ 事件级去重：路径无关身份 + 「追加视图/历史视图」双闭包**

问题：日志文件会被原子替换（新 inode）、被截断重写，同一会话还可能出现在两条路径下（native + WSL 安装、`sessions/` 与 `archived_sessions/`）。仅靠文件字节 offset 无法保证不重不漏。

要点（`src/lib/rollout.js:302-380`）：去重集合按「本轮新键」与「历史键」分成两种视图。
```js
const recordNewEvent = (key) => {
  ensureNewEventKeys();
  if (newEventKeySet.has(key)) return;
  newEventKeySet.add(key); newEventKeys.push(key);
  if (seenEvents) seenEvents.add(key);
  externalEventStore?.add(key);
};
const getAppendOnlyEvents = () => ({ has: (key) => Boolean(newEventKeySet?.has(key) || seenEvents?.has(key)), add: recordNewEvent });
```
关键在**何时选历史视图**（`:428-461`）：只有「单路径 + 同 inode + 未截断 + 有历史 offset」四条同时成立才允许走廉价的本轮集合；四条之后还有两条兜底——`if (prev?.lastTotal) return true;`（`:459`）与 `return false;`（`:460`）。含义是：稳态四条件不成立时，**只要游标里带着 `prev.lastTotal`（Codex 累计基线）就仍然动用持久集合**。【未能确认】`:459` 这条兜底的确切动机源码未加注释，推测与 `rebuildingCodexBaseline` 那条「从 0 重读」的路径有关。

两套身份键（按 provider 不同粒度）：Codex 用 `sessionUUID:eventTimestamp`（`:292-295`）；Claude 用 `claudeMessageDedupKey = msgId ? (reqId ? \`${msgId}:${reqId}\` : msgId) : null`（`:4914-4919`），持久化时截断到最近 10 万条（`:1233-1237`）。

动机：历史视图需要把几十万键读进内存（v2 存储里按日分片、按需加载），本轮视图只需一个 `Set`。「什么时候可以省掉历史视图」被化简成一个**可证伪的四条件判断**，依据全部来自游标本身（不需要扫盘）。
移植：**身份键的选择比实现更关键**——键必须满足「同一逻辑事件在任何路径拼写下都得到同一字符串」。若事件没有天然唯一 id，就用 `(session_id, 序号, 四类 token)` 之类的内容指纹，但要在注释里声明它只是**保守重播指纹**，不能对外宣称是独立请求的强身份。集合规模必须有预算：Claude 侧硬截断 10 万条，Codex 侧靠按日分片落盘；若改用 SQLite 存事件键，给 `(day, key)` 建主键并整表删除过期日。若身份键含时间戳，注意**时钟回拨**——本项目在 snapshots 比较处用「严格更大才替换」（`src/commands/sync.js:4478-4484`）。

**⑨ 跨环境文件级去重：文件身份哈希 + `duplicateOf`**

问题：Windows 主机可同时看到 `C:\Users\x\.claude\projects\...` 与 `\\wsl$\Ubuntu\home\x\.claude\projects\...`——同一批会话文件有两种路径拼写。按路径去重会双计，按 realpath 去重在某些挂载形态下也不可靠。

要点（`src/lib/rollout.js:1056-1107`）：
```js
const cachedFileId = sameInode && typeof prev?.fileId === "string" && prev?.fileIdSize === st.size ? prev.fileId : null;
fileId = cachedFileId || (await claudeFileIdentityHash(filePath, st));
if (fileId) {
  if (prev && (prev.fileId !== fileId || prev.fileIdSize !== st.size)) { prev.fileId = fileId; prev.fileIdSize = st.size; }
  const primary = seenFileIds.get(fileId);
  if (primary === undefined) seenFileIds.set(fileId, filePath);
  else if (!prev && primary !== filePath) {
    cursors.files[key] = { inode, offset: st.size, ..., fileId, fileIdSize: st.size, duplicateOf: primary, ... };
    continue;      // 直接标记为"已追平"，不解析
  }
}
```

三个细节：哈希结果按 `(inode, size)` 缓存在游标上，空转文件不再重算；只有**从未解析过**的重复项才走 `duplicateOf` 快路径（已解析过的重复项交给消息哈希层）；native 副本排在前面，因此被保留的是 native 那份（`src/commands/sync.js:683-684`）。

动机：文件身份（内容/元数据派生）比路径稳定，跨挂载拼写、软链接、大小写差异都能归一。注意它**没有**替代事件级去重——`duplicateOf` 只解决「同一文件两处可见」，两份副本内容开始分叉时仍需消息哈希兜底（源码注释明写，`:1074-1077`）。
移植（工程推断）：Unix 用 `MetadataExt::{ino,dev}`；Windows 没有 Unix 语义的 `ino()`，等价物是 `MetadataExt::file_index()` 配 `volume_serial_number()`，用 `#[cfg]` 封装成统一的 `FileIdentity`。跨 WSL UNC 或 SMB 时 `file_index()` 可能为 0，需按「取不到就退回内容哈希/路径身份」实现。缓存键**必须带 size**（本实现用 `fileIdSize`），否则文件被追加后会把两个不同文件误判为同一份。哈希失败应设计为**回退到路径身份**而不是报错。

**⑩ 双队列分离：本地维度状态与云端事实状态彻底分开**

问题：同一批解析结果既要服务云端（按 `source/model/hour` 计费与展示），又要服务本地项目视图（按 `project_key/source/hour`）。混在一个文件里会造成两类污染：项目维度字段进入云端 schema；**本地队列的积压被误算成待上传量**。

要点：一次解析同时向两个队列 append（`src/lib/rollout.js:683-686`）；项目行是主行的**裁剪版本，不是同构**——比主行**多** `project_ref` + `project_key` 两个前导字段，同时**少三类列**：`model`（项目视图不按模型细分）、`usage_precision`（主行在 `:3341` 写入）、以及 `...prioritySubsetOf(totals)` 展开的 `priority_*` 子集列（主行在 `:3342` 展开，字段表 `:4073-4079`，值 > 0 才出现）。项目行 `:3515-3532` vs 主行 `:3328-3344`；`project_ref`/`project_key` 缺失的项目桶在 `:3513` 直接跳过。

这条裁剪与读侧 key 互为因果：项目队列读侧 key 是 `project_key|source|hour_start`（`src/lib/local-api.js:145-153`，**不含 model**）；主队列 key 是 `source|model|hour_start`（`:202-211`）。云端路径**只读主队列**（`src/commands/sync.js:3654-3659`）：
```js
// Only the main queue is uploaded by drainQueueToCloud. project.queue.jsonl
// is local project-usage state, so counting it here creates false backlog
// and can keep auto retry alive even after cloud sync has drained.
const pendingBytes = Math.max(0, queueSize - Number(afterState.offset || 0));
```

两侧状态文件也分离：`queue.state.json`（上传字节 offset）/ `project.queue.state.json`（本地读侧 offset）；两个读侧 reader 实例独立（`src/lib/local-api.js:145,202`）。

动机：把「云端事实」与「本地视图」当作两条**独立演进**的通道——项目维度是纯本地的产品特性，可随时增删而不动云端契约。`pendingBytes` 只算主队列，把「本地项目数据堆积导致自动重试永不停歇」这个 bug 从设计上排除。
移植：可以用两个独立表/两个 append-only 文件，但**必须让「是否需要上传」只依赖上传通道自己的水位**（常见错误是共用一个 `dirty` 标志）。项目维度的键要显式规定归并方向，否则跨工具同名目录会分裂成多个项目。

**⑪ 上传进度点：逐批推进 + 只发完整行 + 控制行计入上限**

问题：上传可能被拆成多批、跨多次进程（hook 触发的 sync 每次只有几十秒）。进度必须**可恢复**，不能把写者正在写的半行发出去，也不能让某类控制记录把批次撑爆。

要点（`src/commands/sync.js:4315-4413`）：
```js
for (let batch = 0; batch < batchBudget; batch++) {
  if (Date.now() >= deadline) break;
  if (offset >= queueSize) break;
  const result = await readQueueBatch(queuePath, offset, limit);
  offset = result.nextOffset;
  state.offset = offset; state.updatedAt = new Date().toISOString();
  await writeJson(queueStatePath, state);       // 每批成功后立即落盘
}
```
- 只发完整行（`:4447-4449`）：`if (!record.complete) break;`
- 批上限同时约束两类记录（`:4488-4490,4511-4512`）：控制行 `linesRead += 1; if (linesRead >= maxBuckets) break;`（源码注释说明：该记录随本批发送且字节被 `nextOffset` 覆盖，故下一批紧接其后恢复）。
- 上限链：`MAX_INGEST_BUCKETS = 500`（硬上限，`:4313`）→ `batchSize` 默认 200（`:4315`）→ `upload_scope === "community"` 时批数压到 10（`:4392`）；夹取式 `Math.min(Math.max(1, batchSize), MAX_INGEST_BUCKETS)`（`:4329`）。
- 总预算 `UPLOAD_BUDGET_MS = 90_000`（`:168`），剩余预算作为 `AbortSignal.timeout` 传给 fetch（`:4360-4362`）。
- 超时但有成功批次时**不抛错**，保留已推进的 offset（`:4402-4411`）。

动机：三层限制解决三类故障——单请求记录数上限对应用户端 500 条硬约束（源码注释记录了「TRAE 首次同步每个会话一条 state 记录、不设上限就永久 400」的事故，`:4433-4441`）；批数上限控制单次运行时长；总预算防止某次网络慢把 hook 子进程拖死。**「进度逐批落盘 + 只发完整行」**是让「随时被杀」变成安全操作的关键两条。
移植：offset 必须是**字节**位置，不能用字符数或行号（源码在 `src/lib/upload-queue.js:20-21` 特别声明）。若上传带重试，务必保证服务端对该批次**幂等**（整行 upsert 或带 `upload_id` 去重）。把「已发送字节」与「已确认字节」区分开。

**⑫ 把昂贵的全量扫描摊销成后台审计**

问题：为保持「秒级新鲜」，hook 触发的增量扫只扫近期文件；但历史文件也需偶尔重新审计（口径变化、早期 bug、文件被外部改动）。既不能每轮全量扫，也不能永远不扫。

要点（`src/commands/sync.js:3763-3819`）：
```js
const CODEX_COLD_SCAN_AUDIT_INTERVAL_MS = 24 * 60 * 60 * 1000;   // :294
const CODEX_COLD_SCAN_AUDIT_MAX_SYNCS = 288;                     // :301
if (!state || typeof state !== "object" || state.version !== 1) return true;
if (!Number.isFinite(lastFullScanAtMs) || lastFullScanAtMs <= 0) return true;
if (Number.isFinite(nowMs) && lastFullScanAtMs - nowMs > 5 * 60 * 1000) return true;   // 时钟回拨保护
if (Number.isFinite(nowMs) && nowMs - lastFullScanAtMs >= CODEX_COLD_SCAN_AUDIT_INTERVAL_MS) return true;
const syncsSinceFullScan = Number(state.syncsSinceFullScan || 0) + Math.max(0, Number(deferredSyncs) || 0);
return Number.isFinite(syncsSinceFullScan) && syncsSinceFullScan >= CODEX_COLD_SCAN_AUDIT_MAX_SYNCS;
```
配套：状态写进游标（`{version, lastFullScanAt, lastFullScanAtMs, syncsSinceFullScan, lastSkippedFiles, updatedAt}`，共 6 个字段）；本轮**跳过游标提交**时，把审计增量写进独立文件 `deferred-codex-audit.json`（`:3456-3460`）。

动机：双阈值（时间 24h / 次数 288 ≈ 5 分钟粒度下的 24h）覆盖两种场景——机器长期开着、机器频繁开关机。`lastFullScanAtMs - nowMs > 5min` 这条看似奇怪的判断是**时钟回拨保护**：用户改系统时间后 `nowMs - lastFullScanAtMs` 会变负，普通阈值永远不到期，审计被饿死。
移植：别把「审计」实现成「每 24h 全量重扫并全量重写」——本项目把全量重扫与**修复**分开：审计只让 `parseRolloutIncremental` 看到全部文件（增量逻辑仍按 offset 跳过没变的文件），真正的重建只在 repair 里发生。

**⑬ 读时纠错：不改历史文件，只改解释方式**

问题：口径变了、早期版本写错了字段，但重写历史数据文件会引入不可逆风险；完全不修则历史数字错误。

要点（`src/lib/local-api.js:155-200`）：
```js
function isLegacyInclusiveCodexRow(row) {
  ...
  if (cachedInputTokens <= 0 || inputTokens < cachedInputTokens) return false;
  // 旧口径 input 含 cache read，而 total 仍是 input + output —— 用这个不变量识别
  return totalTokens === inputTokens + outputTokens;
}
```

修正动作：`input_tokens -= cached_input_tokens`；对 `cursor`/`antigravity` 这类「`billable_total_tokens` 曾为 0」的行，读时抬到 `total_tokens`（`:191-198`，对应 issue #106）。同一函数被**读侧与上传侧共用**（上传前也过一遍，`src/commands/sync.js:4504-4509`），保证本地与云端口径一致。

动机：用**不变量**（而不是版本号或时间戳）识别历史坏行，使判定与「什么时候修的」解耦——任何来源的坏行都能被识别，且不需要记录迁移状态。代价是每次读取都做这个判断（几个数值比较，非常便宜）。
移植：读路径与发送路径**共用一个**纠正调用点，并在类型层面禁止绕过。但用不变量识别要谨慎：口径多次变化时不变量可能对某些版本不成立（本项目对 Codex 就是这样，`total_tokens === input + output` 只对「input 含缓存」的一代成立）；无法用单一不变量覆盖时，退回「版本号字段 + 迁移记录」。长期看读时纠错会累积成越来越厚的解释层，建议给每条纠错标注引入原因与 issue 编号，并设定「什么时候可以删掉」的条件。

**⑭ 把「被跳过的运行」做成可见事实**

问题：采集系统最危险的失败模式是**静默停摆**——hook 还在触发、进程也正常退出，但数据一直不更新。常见根因是锁争用或残留锁。

要点（`src/commands/sync.js:484-506`）：
```js
const SYNC_SKIP_MARKER = "sync.skip.json";
const reason = holder.exists && holder.pid && !holder.alive ? "lock_debris" : "lock_busy";
const detail = !holder.exists ? "sync lock could not be acquired"
  : holder.pid ? `sync lock held by pid ${holder.pid} (${holder.alive ? "running" : "not running"})`
  : "sync lock has no owner record";
await writeJson(path.join(trackerDir, SYNC_SKIP_MARKER), { reason, detail, at: new Date().toISOString(), lockPath });
```
抢到锁后**立即删除** marker（`:565`），因此 marker 存在即代表「最近一次尝试被跳过」；状态命令读它并上报（`src/commands/status.js:285,1058`）。同类的可见化还有：上传退避写 `upload.throttle.json` 并在 status 暴露 `next_upload_after` / `backoff_until` / `last_upload_error`（`:1054-1057`）。

动机：`lock_busy` 与 `lock_debris` 的区分是核心——前者是正常并发（无需处理），后者是**需要人工介入**的状态（锁残留）。把两者写进同一个可查询的产物，让看板能直接给结论而不是靠猜。
移植：**别退化成一个布尔量**——「被跳过」的排障价值全在 reason 上。时间戳统一用 UTC ISO8601。

**⑮ hook 触发的三件套：先落信号、再节流、然后脱离进程**

问题：provider 的 hook 会**同步等待**命令返回。hook 里直接跑解析会把几秒到几十秒算进用户交互延迟；每个 turn 都触发一次完整解析会拖垮机器。

要点（生成 `~/.tokentracker/bin/notify.cjs` 字面内容的模板 `src/commands/init.js:1453-2231`，宿主函数 `buildNotifyHandler` `:1445-2232`）：
```js
// :1446-1447 文件头注释直接写明了设计约束
// Keep this file dependency-free: Node built-ins only.
// It must never block Codex; it spawns sync in the background and exits 0.
// :1495-1498 ① 先落信号（即使后续节流跳过，也留下"有过活动"的证据）
try { fs.mkdirSync(trackerDir, { recursive: true });
      fs.writeFileSync(signalPath, new Date().toISOString(), { encoding: 'utf8' }); } catch (_) {}
// :1519-1536 ② 20 秒节流 + ③ 脱离进程
if (now - last > 20_000) {
  try { fs.writeFileSync(throttlePath, String(now), 'utf8'); } catch (_) {}
  const syncArgs = ['sync', '--auto', '--from-notify', '--source', source];
  if (hasLocalRuntime && hasLocalDeps) spawnDetached([process.execPath, trackerBinPath, ...syncArgs]);
  else spawnDetached(['npx', '--yes', fallbackPkg, ...syncArgs]);
}
// :1563-1573
function spawnDetached(argv) {
  const child = cp.spawn(argv[0], argv.slice(1), { detached: true, windowsHide: true, stdio: 'ignore', env: process.env });
  child.unref();
}
```
再加两件：④ **链式调用用户原有的 hook**（先备份原命令再在末尾追回，`:1538-1559`）；⑤ 无条件 `process.exit(0)`（`:1561`），保证 hook 永不因我们失败而失败。

另需知道一处**冗余判断**：`if (hasLocalRuntime && hasLocalDeps)` 里两个变量指向**同一个路径**——模板内 `trackerBinPath`（`:1483` 是 `JSON.stringify(trackerBinPath)`）与 `depsMarkerPath`（`:1484` 是字面 `path.join(trackerDir,'app','bin','tracker.js')`）**求值相同、字面不同**（值同为 `<trackerDir>/app/bin/tracker.js`，因此该条件退化为**一次 `existsSync`**；同一 quirk 也出现在 OpenClaw 插件模板（`src/lib/openclaw-session-plugin.js:358,361`）。移植时按「本地 runtime 存在 → 本地 exe；否则走兜底命令」实现即可，不必照抄这层冗余。

动机：三点顺序不可换——signal 先写，所以即使节流拦掉本次 spawn，也能知道「用户刚刚在活动」；节流在 spawn 之前写时间戳，避免并发 hook 同时通过判断造成雪崩；detached + unref + `stdio:'ignore'` 让子进程完全脱离父进程生命周期。
移植（工程推断）：真 detached 在 Windows 上要 `CREATE_NEW_PROCESS_GROUP` / `DETACHED_PROCESS`（`std::os::windows::process::CommandExt::creation_flags`），Unix 用 `setsid`；若不做 detached，父进程被 kill 时子进程可能一起死。节流状态文件是**读-判断-写**三步，本身不原子（并发 hook 可能都通过），本项目接受这个近似（重复 spawn 的后果只是多一次解析）。hook 处理器**不要依赖仓库/依赖包**——本项目生成的文件只用 Node 内置模块，并在本地 runtime 缺失时回退 `npx --yes <pkg>`。

**⑯ 向用户配置文件写入：三种幂等范式 + 一类整体覆盖 + 一律先备份**

问题：往用户的 `settings.json` / `config.toml` 里插自己的 hook，必须重复安装不出重复项、命令路径变化时不产生「旧条目 + 新条目」、卸载时能精确摘掉自己的项且不动别人的配置。

要点（按配置类型选范式）：
- **① 命令身份识别**（跨路径漂移仍认出自己）——`src/lib/notify-command.js:23-36`：解析命令行 token，只接受 `node(.exe) notify.cjs --source=<id>` 这一形状，返回 `${file}\u0000${args[2]}` 作为身份。
- **② 就地修复 + 同 scope 去重**（JSON hooks 结构）——`src/lib/claude-config.js:181-230`：把过时形态改写为 `{type:"command", command, shell?}`，并按 `matcher` scope 去重。
- **③ 哨兵串 + 拒绝覆盖外来文件**（**本项目只对 omp 这么做**）——`src/lib/omp-hook.js:17,388-395`：文件带 `// @tokentracker-managed-omp-extension` 则覆盖，不带则跳过并回报 `skippedReason: "unmanaged-extension-present"`（该字符串**全仓仅此一处产生**，消费方 `src/commands/init.js:813`）。
- **④ 内容等值短路 + 无条件备份覆盖**（OpenCode 插件）——`src/lib/opencode-config.js:66-87`：先比对内容，`if (existing === next) return { changed: false }`（`:73-75`，**这是内容等值短路，与哨兵无关**）；内容一旦不等，**无论原文件里是什么都先备份再覆盖**（`:80-85`）。标记检测（`hasPluginMarker`）只出现在 `removeOpencodePlugin`：内容不带标记时返回 `skippedReason: "unexpected-content"`（`:94`）。**OpenCode 不会拒绝覆盖外来文件**——不要把它当哨兵范式。

配套：所有写入路径先复制 `settings.json.bak.<ISO>`（`src/lib/claude-config.js:248-262`、`src/lib/gemini-config.js:265-279`、`src/lib/codex-config.js:89-104`）；仅 Codex 系需要额外把**被覆盖的原命令**存到 `*_notify_original.json`（因为 notify 是独占槽位）。TOML 侧不引入解析库，用手写的最小行编辑（`src/lib/codex-config.js:245-309`）。

动机：不同配置文件类型对「幂等」的容错能力不同——JSON hooks 是数组结构，适合结构化比对与修复；TOML 只有一行 `notify = [...]`，适合等值比较；插件/扩展文件是**整个文件属于别人**，最稳的形态是「不认识就不写」（哨兵），代价是用户原有的同名插件会被永久跳过，所以 OpenCode 改用「先备份再覆盖」。把这几类分别实现，比试图写一个通用「配置补丁器」可靠得多。
移植：Rust 侧 JSON 用 `serde_json::Value` 做结构化改写并保留未知字段；TOML 若用 `toml_edit` 务必用其「保留注释与格式」的编辑 API——本项目的手写行编辑会在复杂 TOML 特性（多行数组、行内注释、多行字符串）下失效【未能确认：源码注释自称支持 single-line / multi-line arrays，未覆盖面更广的语法】。**备份策略要一致**：Grok 分支无备份、无条件覆盖（`src/lib/grok-hook.js:85-91`），是明显的策略缺口，别照抄（见 10.2）。

**⑰ 发现层的三个反直觉细节**

① **「必须包含子目录」的判定要折进探测本身**（`src/lib/install-resolver.js:25-36`）：`populated(p) = !requireAnyChild || hasAnyChild(p, requireAnyChild, deps.existsSync)`，WSL 候选也套同一谓词。原因是 `discoverWslHome` 的语义是「返回第一个存在的候选」——若先拿回候选再校验，第一个空壳就会胜出并且**再也不会去看第二个发行版**（`src/commands/sync.js:766-772`）。

② **合并模式只在解析器具备路径无关身份时才启用**（`src/lib/install-resolver.js:6-18` 的注释即契约）：`union: true` 仅用于 Codex/acode（事件按 `sessionUUID:eventTimestamp` 去重）与 Claude（手工 union + 文件哈希 + 消息哈希）。这条约束防止把「双计」当「覆盖」。

③ **WSL 会话的 POSIX cwd 要用转录文件自身的 UNC 前缀重锚**（`src/lib/wsl-probe.js:232-242`）：
```js
if (typeof cwd !== "string" || !cwd.startsWith("/")) return cwd;
// `new URL("file:///C:/dev/app").pathname` is "/C:/dev/app" — a leading
// slash, but a Windows path rather than a POSIX one. Qoder feeds exactly
// that shape in via `project_uri`; re-anchoring it would invent
// `\\wsl$\Distro\C:\dev\app`.
if (WINDOWS_DRIVE_PREFIX_RE.test(cwd)) return cwd;
const root = wslUncRoot(transcriptPath);
if (!root) return cwd;
return root + cwd.slice(1).replaceAll("/", "\\");
```
前缀取自实际读取的文件路径，因此自动选中本机真正可用的 `\\wsl$` 或 `\\wsl.localhost` 之一（两者并非总是等价，本项目按发行版版本排序两种前缀，`:193-201`），同时显式避免把 `C:\...` 变形成 `\\wsl$\Distro\C:\...`。

移植：Rust 侧判断「以斜杠开头但不是 POSIX 路径」建议显式实现盘符前缀正则（本项目用 `/^\/[A-Za-z]:[\\/]/`），因为 `file://` URI 解析结果经常长这样。跨 UNC 读 SQLite 用「复制 db + `-wal` + `-shm` + `-journal` 到临时目录」的方式（`:244-262`）；用 `rusqlite` 时优先尝试 `VACUUM INTO`，失败退回复制文件组。

**⑱ 可测性与兼容性的两个小成本高收益做法**

**A. 依赖注入默认参数化**，全部落在函数签名上（生产代码零分支）：`probeWslDistros(deps = {})`（`src/lib/wsl-probe.js:57-63`，`deps.runWsl` 可替换）；`openCursorStore({ ..., failureInjector = null } = {})`（`src/lib/cursor-store.js:25-32`）；`const cursorSyncDeps = context && typeof context.cursorSyncDeps === "object" ? context.cursorSyncDeps : {};`（`src/commands/sync.js:516-538`）。`maybeInjectFailure(this.failureInjector, "beforeManifestSwap")`（`src/lib/cursor-store.js:972`）允许测试精确验证「manifest 切换前崩溃」的清理路径。【未能确认】这些测试缝在生产路径是否完全惰性（源码注释称 "no public CLI flag/config is introduced"，`src/commands/sync.js:526-531`），但从签名看生产调用者不传即为 no-op。

**B. 显式别名表 + 双层白名单**（`src/commands/sync.js:310-373`）：
```js
const AUTO_SYNC_SOURCE_ALIASES = new Map([
  ["code", "every-code"], ["commandcode", "command-code"], ["deepseek", "dsh"],
  ["everycode", "every-code"], ["kilo", "kilo-cli"], ["kilo-code", "kilocode"],
  ["kimi_code", "kimi-code"], ["roo-code", "roocode"],
]);
const AUTO_SYNC_SOURCES = new Set([ /* 实测 45 项 */ ]);
const BACKGROUND_AUTO_SYNC_SOURCES = new Set(["acode", "codex", "every-code", "reasonix"]);
```
别名表把外部叫法与内部 id 解耦，使外部参数可随产品改名而不动内部状态；双层白名单让「后台轻量扫描」的源集合成为显式、可审计的常量。别名归一只做一次（入口 `normalizeSyncSource`，`:3755-3762`），后续所有路径使用内部 id。`AUTO_SYNC_SOURCES` 实测成员 45 项（逐项读取 `:320-366`）：`acode, antigravity, anythingllm, claude, claude-science, cline, codebuddy, codex, command-code, copilot, craft, cursor, devin, droid, dumate, dsh, every-code, gemini, goose, grok, hermes, kilo-cli, kilocode, kiro, kimi, kimi-code, lmstudio, mimo, minimax-code, omo, omp, opencode, openclaw, pi, qoder, qoder-cn, reasonix, roocode, trae, trae-cn, unsloth, workbuddy, workbuddy-ai, zcode, zed`。

### 10.2 不建议照抄的设计

以下条目附源码依据或源码内的事故记录。

1. **`cmdSync` 单函数 3194 行 / `sync.js` 7251 行的巨型编排**（`src/commands/sync.js:516-3709`，起止行含在内）。锁、路径解析、40+ provider 分支、迁移、上传、重试、输出全塞进一个 async 函数，靠 `let` 变量的顺序赋值维持隐式状态机。`skipNoOpCursorCommit`（`:3424-3437`）的判据要同时引用 **13 个条件项**（`opts.auto`、`!isFullSourceScan`、`cursorStore.mode === "v2"`、`cursorStore.requiresCommit !== true`、`totalParsed === (…)`、`totalBuckets === 0`、两处 project 桶均为 0、`!commandCodeResult.schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok`）。**应拆成显式阶段（discover → parse → persist → upload），用类型/结构体传递阶段产物。**
2. **`.reclaim` 闸链需要「迭代清扫」才能自愈**（`src/lib/fs.js:178-204`，issue #431）。锁协议复杂到需要专门清扫自己的残留闸，说明协议过重。更简单的替代：单一 `O_EXCL` 锁 + token 心跳 + 超时接管，且**接管动作本身**也走「原子 rename 到隔离名」路径，无需第二层闸。
3. **队列写入无任何写侧互斥**。主队列 append 写点**至少 6 处**（`src/lib/rollout.js:3483`、`:20031`、`:22594`，`src/commands/sync.js:5126`、`:5207`、`:5281`），另 `src/commands/sync.js:4114` 写 fallback、`src/lib/rollout.js:3536` 写 project 队列——全部假定调用方持有 `sync.lock`。一旦加第二个写者，两个 `appendFile` 会交错写出多行撕裂。**建议：要么给队列加单写者约束（只有持锁进程能打开 append 句柄并用 flock 强制），要么用 SQLite/日志合并结构代替。**
4. **`repair` 的「原子删除 + 重扫」模式本身**（0.26.3 的真实数据丢失事故，`src/commands/sync.js:234-248`）。它已用守卫弥补，但根因是**把「重算」实现成了「先删后建」**。更安全的形态是写入新快照、**新数据集完整生成后**整体切换（即本项目 cursor-store 里已用得很好的 generations 模式），绝不在旧数据上做删除。
5. **Grok 分支无条件覆盖、无备份**（`src/lib/grok-hook.js:85-91`，注释 `always overwrite to keep command up-to-date if bin path changes`）。与其它 provider 的「幂等 + 备份 + 还原」策略不一致；`~/.grok/hooks/99-tokentracker-usage.json` 这一固定文件名下的用户自定义内容会被直接覆盖。**要抄就抄 Claude/Codex 范式。**
6. **同一语义存在三套并行纠错机制**：一次性 `repair` 迁移键、读时 `normalizeQueueRow`（`src/lib/local-api.js:170-200`）、云端整行 upsert。三者口径必须人工保持同步——历史上就出现过「云端 `conversation_count` 恒为 0 而本地正确」这类不一致（`src/commands/sync.js:249-256` 的注释即为此事故）。**建议：把口径收敛成一份可执行规范（含测试向量），让三条路径都跑同一份用例。**
7. **文件体积驱动的行为切换**（16 MB 阈值决定用 LegacyCursorStore 还是 v2，`src/lib/cursor-store.js:21,77-86`）。同一用户的存储形态会在某次运行后**静默改变**，且两套实现能力不等价（legacy 下 `codexEventStore = null`，事件去重退化为内存数组，`:165-197`）。**建议：一次性迁移到一个形态，用显式 schema 版本字段而不是体积阈值。**
8. **`legacyGroups` / `groupQueued` 那条「整小时聚合成 `model=DEFAULT_MODEL`」的遗留路径**（`src/lib/rollout.js:3181-3194,3430-3480`）。其语义依赖「某个键存在与否」作为隐含标记，**新装机会不会写入该键尚未验证**（见 §12 A4）；这类**用状态形状编码语义**的做法极易在后续修复中互相干扰（v2→v3 的注释正是这类冲突的产物，`src/commands/sync.js:216-226`）。
9. **手动代理无法生效时整个进程 `exit(1)`**（`bin/tracker.js:36-40`，判据是 `applyResult.unprotected === true`，即代理未能生效**且**无法阻断出站流量；普通网络失败不走这条），hook 路径同样受影响。结果是「代理挂了 → 采集静默停止」，数据不更新但没有任何面向用户的告警（只有被 `stdio:'ignore'` 丢弃的 stderr）。**建议：采集路径不能因为网络配置问题硬失败；应降级跳过上传并留下可查询状态。**
10. **`local-api.js`（3543 行 / 156 KB）职责混杂**：读队列、项目归因、账号体系、头像代理、宠物、静态资源与 SPA 回退都在一个 handler 工厂里。虽然每个功能内部质量不差，但耦合度高到任何改动都要回归全部端点。**建议按端点族拆分模块，用薄路由层组合。**
11. **测试专用钩子进入生产函数签名**（`context.traeCnFetchImpl`、`context.cursorSyncDeps`、`context.scanSources`，`src/commands/sync.js:519-538`）。成本低但持续污染 API 表面积；更干净的做法是把依赖收进一个 `Deps` 结构体，或用 `#[cfg(test)]` / 依赖注入容器统一管理。
12. **依赖 `st.ino` 的变化来检测原子替换**（`src/lib/rollout.js:489-492`）。在 inode 语义不可靠的环境（某些网络盘、容器叠加文件系统、WSL UNC）上 `ino` 可能恒为 0 或恒定，导致「替换后从 0 重读」的检测失效，此时只能靠事件哈希兜底。**建议：把「文件身份」做成可配置策略（inode → file-id → 内容抽样哈希），并在启动诊断里显示实际采用的策略。**
13. **把 Windows 侧 7680 端口占用交给「WSL 默认换端口 7681」**（`src/commands/serve.js:15-21`）。这是对 Delivery Optimization 服务行为的补偿性绕行，但仍会与其它占用 7681 的进程冲突。**建议：端口发现走「嗅探 + 用户可见提示」，而不是固定的 +1 规则。**
14. **`status` 的 `hookStatus` 传参里有 3 个键在 passive 判定中永不使用**（`src/commands/status.js:1024-1036` 传入 **11 键**，`detectPassiveProviders` 只消费 8 个：`claude`/`gemini`/`codex_notify`/`acode_notify`/`every_code_notify`/`codebuddy`/`workbuddy`/`workbuddy-ai`；未消费的是 `opencode_plugin`、`openclaw_session_plugin`、`grok`，消费面 `src/lib/passive-mode.js:93-186`），以及 `workbuddy-ai` 这个带连字符的特殊键名。另有一处**双向漂移**：`passive-mode.js:88-90` 的 JSDoc 自述形状是 `{claude, gemini, codex, every_code, opencode, openclaw, codebuddy, workbuddy, grok}`——JSDoc 里的 `opencode`/`openclaw`/`grok` 并未被实现读取，实现真正读的 `acode_notify`/`workbuddy-ai` 又不在 JSDoc 里。这类「传参与消费不一致」的接口会随 provider 增加持续腐化。**建议：hook 状态用显式 provider 枚举 + 结构体，编译期保证双方一致（Rust 的 enum 天然能做到）。**
15. **关键常量双份定义、无测试绑定**。`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 在 `src/lib/pricing/index.js:31` 定义并 `:375` 导出，但**在本文件内从未被读取**；真正生效的是 `src/lib/codex-rollout-parser.js:51` 的另一份同名常量（用于 `:733`）。两处目前都是 `272_000`，改一处不另一处不会有测试失败。同类：半小时桶的 `toUtcHalfHourStart` 有两份实现（`src/lib/rollout.js:4173-4190` 与 `src/lib/claude-categorizer.js:1175-1191`，**语义等价、文本不同**），`test/` 下两个测试都只指向 `rollout.js` 那份，副本**无人测**——真值修复与增量解析一旦分叉，队列会出现两套时间分桶。
16. **「不变量」式识别若无对等回归测试，会静默双计**。Codex 侧的 reasoning 不并入 output 依赖 `src/lib/pricing/index.js:248-256` 的 `source` 白名单硬编码（`codex`/`acode`/`every-code`/`omo`/`cline`）；一旦上游新增一个 `*-code` 变体而白名单漏加，就会**静默双计**每条 reasoning token，而现有测试全绿（`test/claude-reasoning-usage.test.js:45-47` 只锁定了 Claude 的「列分布可换、费用不变」）。**建议白名单改为结构性判定**（如 `source.endsWith("-code") || source === "codex"`）并补对等断言。
17. **回退链的本地/云端能力不对等**。云端 `computeRowCost`（`dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:569-613`）只有纯四桶 + 条件性推理折叠，**没有**长上下文 / Fast 溢价分支；该表与 `curated-overrides.json` 存在 **9 键**差集，其中 `grok-build-free`（本地 $0 vs 云端按 `grok-build` = 1.25/2.5/0.2 计）与 `grok-4.5-build-free`（本地 $0 vs 云端 `cursor-grok-4.5` = 2/6/0.5）是**价格层面的实际分歧**。parity 测试（`test/edge-pricing-parity.test.js:76-85`）只做 edge↔edge 字节比对，**不含**「edge ⊇ curated」断言，抓不到这类漂移。

> 结论性判断：该仓库**在「崩溃安全、并发正确性、历史数据不可逆风险」这三类问题上给出的机制质量很高，可以直接照抄**（10.1 全部条目）；而**在编排结构、状态形状语义、多路径口径一致性、以及「网络/平台失败是否应硬退出」这些地方是反面教材**（10.2）。移植时应把两半严格分开对待。

### 10.3 TokenScope 直接相关的差异提示

以下差异不是优劣判断，而是**会让两边数字不同**的口径事实，移植时必须显式决策。

**① 项目身份：仓库 URL vs 规范化绝对路径 + 层级归并**

TokenTracker 的用量侧项目身份来自 `deriveProjectKeyFromRef`，**只吃 URL**（`src/lib/rollout.js:4429-4442`：按 `/` 切段后 `segments.join("/")`），无 cwd、无层级归并，且 `.git` 后缀不剥离；会话侧项目键另取 basename（`src/lib/session-analytics.js:405-408`）。TokenScope 的既定口径是「规范化绝对路径 + 按项目根归并、判定按分量边界前缀、不做文件系统访问」。**二者不可互相替代**：前者在远程托管场景下跨机器天然一致，后者在本地多目录场景下才正确。若照搬 URL 身份，会丢掉「同名不同盘」「子目录归属」这两类信息。

**② `conversation_count` 跨来源不可比，且同名指标在 21 家里语义五家五样**

至少三种口径并存：产生差分的消息数（Gemini/Opencode）、每条事件 +1（Kimi 一族）、仅首次（Cline）、仅首次会话或重置（Droid）、恒 1（Goose/Lmstudio/Unsloth）、DB `message_count` 差分（Hermes）、线程更新次数（Zed）。Claude 侧按**主会话 user 文本消息**计（`src/lib/rollout.js:2807-2837`，`:2833` 落在 `isMainSession && type==="user"` → `hasText` 内层分支），而 Craft 侧的注释自称 "count one-per-assistant-message" **与代码冲突**（`:17233-17234`）——源码自身两处说法互相打架，以代码为准。**任何跨来源「会话数」汇总都只是粗略值**，若不打算把它当可比指标，就不要在 UI 上并列展示。

**③ 队列是累计快照 + 撤回行，而不是事件流**

TokenTracker 的 `queue.jsonl` 是**每 `(source, model, 半小时)` 一行的累计快照**，写入是纯追加（`src/lib/rollout.js:3482-3484`，仅在有新增行时执行），读侧同键「最后一条胜出」（`src/lib/local-api.js:202-211`；`src/lib/async-queue-reader.js:51-57,86`）。当 `unknown` 桶被对齐到真实模型时，会**先追加一条全零行**抵消旧归属（retraction，`src/lib/rollout.js:3251-3317,3351-3402`）。因此：
- 「同一键出现多行」是常态而非异常，零行必须被解释为撤回而不是「零用量」；
- `setMaxTotal`（`src/commands/sync.js:6585-6589`）与 `mergeMaxTotals`（`:6574-6583`）**只在 codex rescan/fork 修复的数据丢失守卫里使用**（唯一调用族 `:5848-5879`，两处 `skipping to avoid data loss`）：因为撤回零行会把 last-wins 的末行压成 0，只有 MAX 才能保住历史峰值。**两套口径并存不矛盾，但用途必须分清。**
- TokenScope 是 SQLite `history.db` + 每来源事件，没有「零行撤回」概念，对应语义要重新设计（例如显式的 correction 事件或归属修订）。

**④ 时区：TokenTracker 的两级回退 vs TokenScope 的单一权威源**

TokenTracker 的时间口径分两级：① `rowDayKey` 一族（日键 + 日键区间过滤）在 `tz`/`tz_offset_minutes` 皆缺时**静默退化为 UTC 日**（`src/lib/local-api.js:213-225`，注释在 `:224`），不报错；② 半小时桶本身**恒为 UTC**（`toUtcHalfHourStart`），日归属完全交给请求参数。此外同一面板内存在两套口径：热力图网格的日期轴与单元格键按 UTC 日（终点 `new Date(\`${todayStr}T00:00:00Z\`)` 与单元格 `cursor.toISOString().slice(0, 10)`，`src/lib/local-api.js:2540`、`:2563`），而网格数据来自按请求时区聚合的 `aggregateByDay`（`:2537`、`:2545`），`night_owl` 徽章按请求时区小时（`:482`）。TokenScope 的既定口径是「存储层一律 UTC 原样持有、全链路只做一次时区转换、时区唯一权威来源是 `settings.toml`、一次查询只在入口解析一次『今天』并冻结」。**TokenTracker 没有「冻结 as_of」这个概念**——每次请求都重新解析 `new Date()`（`:2382`）。移植时不要去仿它的两级回退。

**⑤ 计价：只有 LiteLLM 一家动态来源，且没有外置价格文件**

TokenTracker 的价格是三层：`curated-overrides.json`（内置策展表，84 条 `exact` + `source_exact.acode` 64 条 + `fuzzy` 47 条，最高优先级、永不被网络覆盖）→ LiteLLM 运行期数据（磁盘缓存 `~/.tokentracker/cache/pricing.json` → 上游 URL → 陈旧缓存 → 内置 seed，四级降级，`src/lib/pricing/litellm-fetcher.js:107-164`）→ 内置 `seed-snapshot.json`（2284 条模型，最低优先级、离线兜底）。**没有 models.dev、没有 OpenRouter、没有用户可编辑的外置价格文件**（`src/` 全目录零命中；数据目录下与价格有关的派生物只有 `cache/pricing.json`）。

与前序研究的对照更能说明差异：ccusage 是**单表 + 覆盖**（键级整条替换、用户 override 字段级、内置表默认 `or_insert`），并**有编译期内嵌价格表**（编译期裁剪 LiteLLM + models.dev 快照）；tokscale 是**6 个上游来源（3 动态 + 3 内置）+ 1 张用户自定义表**、候选择优；TokenTracker 是**3 层、只有 1 家动态上游、有内置 seed 但无用户外置表**；TokenScope 是**3 来源候选（外置 `pricing.toml` / models.dev / OpenRouter）且无编译期内置表**。四者的「内置表」取舍各不相同：TokenScope 用「首次启动需同步」换「未收录模型始终未知、不静默按 0 吞掉」；TokenTracker 用「内置 2284 条 seed」换离线可用，代价是**离线/首启状态下价表年龄可能远大于 TTL**（seed 的 `_meta.generated_at` 是 `2026-05-26T11:12:29.761Z`，而运行期磁盘缓存的 TTL 只有 24 小时，`src/lib/pricing/litellm-fetcher.js:12`）。

另有两处 TokenScope 已明确规避、而 TokenTracker 存在的行为：**缓存写入只有单一 `cache_write` 槽位**（`src/lib/pricing/matcher.js:410-412`，不区分 5 分钟/1 小时缓存档，因此会低估 1 小时缓存写入的成本）；**推理折叠名单是硬编码白名单**而非结构性判定（见 10.2 第 16 条）。还有一处数量级差异：LiteLLM 侧约 **75% 的模型没有缓存读价、约 92% 没有缓存写价**（`seed-snapshot.json` 2284 条中 `cache_read_input_token_cost` 仅 566 条、`cache_creation_input_token_cost` 仅 188 条），走该路径且事件带非零 `cached_input_tokens` 的模型，缓存读取部分会被算成 **$0**——这不是 bug 而是上游数据缺口，移植时若只接 LiteLLM 一家会继承同样的缺口。

**⑥⑦ 去重分层与增量策略的取舍方向**

TokenTracker 的去重分层达七层，其中两个概念 TokenScope 目前没有对应物：**账户级来源**（`src/lib/source-metadata.js:1,7-13` 把 `cursor`、`trae-cn` 标为 account 级，可被 personal/local 过滤排除）与**双安装命名空间光标**。这两者是否需要在 TokenScope 落地，应在设计阶段显式回答，而不是靠移植代码时顺手带上。
增量策略上三者方向相反：TokenTracker 每次全量扫描文件系统但**省重解析**（文件 offset + inode + 事件哈希三层游标）；ccusage 每次全量重扫但**省读取**（按 mtime + 会话组并查集剪枝 + 行级预过滤）；tokscale 只对 Codex 做文件 append 增量。TokenScope 是「每次串行全量解析 + `(size, mtime)` 指纹缓存」。三种取舍各有代价，建议在显式记录口径后再决定是否引入剪枝——**若引入 mtime 剪枝，必须按会话组整体判断**，否则会踩 sidechain 跨文件去重的坑。

---

## 11. 关键常量与阈值速查

| 常量名 | 值 | 位置 | 含义 |
| --- | --- | --- | --- |
| 半小时桶 | UTC 半点对齐 | `src/lib/rollout.js:4173-4190` | `toUtcHalfHourStart`：所有行的桶起点恒为 UTC 半小时；另有语义等价副本 `src/lib/claude-categorizer.js:1175-1191` |
| 日键回退 | 缺失 `tz` 时退化 UTC 日 | `src/lib/local-api.js:213-225` | `rowDayKey`：有有效时区时用 `Intl.DateTimeFormat` 分区格式化，否则 `hour_start.slice(0,10)`（注释在 `:224`） |
| 队列批量与上限 | `batchSize` 默认 **200**、`MAX_INGEST_BUCKETS = 500`（硬上限）、社区档批数 **10** | `src/commands/sync.js:4313,4315,4392`（夹取 `:4329`） | 单批记录数 / 单请求硬上限（对应用户端 500 条约束，事故注释 `:4433-4441`）/ `upload_scope === "community"` 时 `batchBudget = Math.min(batchBudget, 10)` |
| 上传总预算 | `UPLOAD_BUDGET_MS = 90_000`（90 s） | `src/commands/sync.js:168` | 单次 drain 的总时长预算，剩余预算作为 `AbortSignal.timeout` 传给 fetch（`:4360-4362`）；超时但有成功批次时不抛错（`:4402-4411`） |
| hook 节流 | **20 s** | `src/commands/init.js:1519-1536` | notify.cjs 模板：`if (now - last > 20_000)` 才 spawn sync；时间戳在 spawn 前写入 |
| 锁心跳 / 锁超期 | `LOCK_HEARTBEAT_MS = 30 s` / `LOCK_STALE_MS = 5 min` | `src/lib/fs.js:69-70` | 持锁期间 `utimes` 锁文件与心跳文件并 `timer.unref()`；心跳超期即视为遗弃（老格式锁退回 mtime 判断 `:105-144`） |
| 回收闸 / 过渡守卫 | `MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64`、`TRANSITION_GUARD_RETRY_MS = 5`、`TRANSITION_GUARD_ATTEMPTS = 1000` | `src/lib/fs.js:71-74` | `.reclaim` 链最大嵌套 / 迭代清扫链上限（issue #431 自愈）/ 锁状态转换的自旋重试参数 |
| JSON 读改写锁超时 | **30 s** | `src/lib/fs.js:416-443` | `updateJsonLocked` 用 `<file>.lock` 做读-改-写整体互斥 |
| 冷扫描触发 | 24 h（`CODEX_COLD_SCAN_AUDIT_INTERVAL_MS`）或 288 次（`CODEX_COLD_SCAN_AUDIT_MAX_SYNCS`）；时间戳落在未来 > 5 min（时钟回拨保护）亦强制触发 | `src/commands/sync.js:294,301,3772-3781` | 双阈值覆盖「机器长期开着」与「频繁开关机」两种场景；回拨保护避免审计被永久饿死 |
| 游标阈值 | `DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024` | `src/lib/cursor-store.js:21` | 旧 `cursors.json` 超过 16 MB 才启用 v2 代际存储（`:77-86`） |
| 项目上下文重扫 | `PROJECT_ABSENT_CONTEXT_RESCAN_MS = 24 h` | `src/lib/cursor-store.js:22` | 项目上下文缺失的重新探测周期（`:1404-1406`） |
| 长上下文阈值 | `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000` | 定义 `src/lib/pricing/index.js:31`（**本文件零读取**）；生效者 `src/lib/codex-rollout-parser.js:51` → `:733` | 输入（含缓存）超 272K 才启用 OpenAI 长上下文溢价；仅 `gpt-5.6`/`gpt-5.6-sol`/`gpt-6-astra`/`gpt-6-sol`/`gpt-6.1-sol` 系列启用（`src/lib/pricing/index.js:266-306`） |
| DeepSeek 周末起点 | `Date.UTC(2026, 7, 22, 16, 0, 0)` | `src/lib/pricing/index.js:186` | off-peak 判定：按 +08:00 平移取 weekday，周末即 off-peak；工作日 `peak = [1,4) ∪ [6,10)`（UTC 小时，`:183-207`） |
| DeepSeek 折扣 | 四类单价 **×0.5** | `src/lib/pricing/index.js:34-39,209-222` | 命中 `DEEPSEEK_TIME_PRICED_MODELS` 且判为 off-peak；`acode` 来源豁免 |
| 价格缓存 TTL 与降级链 | `DEFAULT_TTL_MS = 24 h`、`DEFAULT_FETCH_TIMEOUT_MS = 10_000`；降级 **4 级** | `src/lib/pricing/litellm-fetcher.js:12-13`、`:107-164` | 磁盘缓存新鲜度阈值与上游取数超时；disk-cache（`:128-137`）→ upstream（`:139-146`）→ stale-cache（`:148-158`）→ seed-snapshot（`:160-163`） |
| 负缓存 | `state.negativeCache`（`Set`） | `src/lib/pricing/index.js:64-66,120-131` | 按 `source\0model` 记忆未命中，避免每次全表遍历；`revision` 变化时清空（`:86`、`:105`） |
| 策展表条目 | `exact` **84** / `alias` 1 / `source_exact.acode` **64** / `fuzzy` **47** | `src/lib/pricing/curated-overrides.json`（775 行 / 29754 字节） | 最高优先级内置表；`MODEL_PRICING = curatedOverrides.exact`（`src/lib/pricing/index.js:363`） |
| seed 快照条目 | 顶层 2285 = 1 `_meta` + **2284** 模型 | `src/lib/pricing/seed-snapshot.json`（256869 字节） | 最低优先级离线兜底；`require` 时同步加载（`src/lib/pricing/index.js:44-55`） |
| 云端手抄表 | **119** 键（+ iFlytek MaaS 64 键） | `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142-332`（119）/ `:339-404`（64） | 与 `curated-overrides.json` 存在 9 键差集，parity 测试不校验超集关系 |
| priority 乘数条目 | **3** 条（`gpt-6-astra`/`gpt-6.1-sol`/`gpt-6-sol` = 2） | `src/lib/pricing/curated-overrides.json` 的 `exact` | 只有 `pricing.priority_multiplier > 1` 才启用 Fast 档溢价 |
| fork 判定 | `FORK_REPLAY_GAP_MS = 500` | `src/lib/rollout.js:4210` | 重放行间隔 < 500 ms 视为 fork 重放突发（实证：重放 flush 亚毫秒到几毫秒，真实轮次 ≥ ~4.6 s，重放→实况断点 ≥11 s） |
| fork 判定（应用点） | 双条件 | `src/lib/rollout.js:2588-2601` | 跨日靠日期守卫（`:4212-4214` `isForkedReplayToken`），同日靠 500ms 突发检测；`replayPrefixActive = startOffset === 0`（`:2339`） |
| Claude 哈希上限 | **100 000**（保留最后 10 万） | `src/lib/rollout.js:1236-1237` | `cursors.claudeHashes` 截断；注释 `:1021-1025` 自述淘汰会让批量历史双计，故加文件级去重作为第二层 |
| 事件集合 | 按日分片持久化 | `src/lib/cursor-store.js:681-726` | `loadEventSet`：Codex 侧事件键按日分片落盘，Claude 侧靠 10 万条硬截断 |
| project 汇总上限 | **10** | `src/lib/local-api.js:2737-2740` | `/functions/tokentracker-project-usage-summary` 的 `limit` 硬上限（排序按 `billable_total_tokens` 降序，`:2785`） |
| sessions 分页上限 | **2000** | `src/lib/local-api.js:2505` | `/functions/tokentracker-sessions`：`Math.min(limitParam, 2000)`，缺省 0（不分页）；其余用量端点一律返回全量数组（该文件无对应分页常量，`:95` 的 `PROJECT_USAGE_MAX_ENTRIES = 10` 只服务 project-usage-summary） |
| 队列压缩块 | `CHUNK_BYTES = 64 KiB` / `BACKLOG_BYTES = 1_000_000` | `src/lib/upload-queue.js:5-6` | 压缩读写分块 / 触发压缩的最小 pending 字节 |
| 读侧分块 | `CHUNK_BYTES = 64 * 1024` | `src/lib/async-queue-reader.js:5` | 每块后 `setImmediate` 让出；读缓存最多 4 个文件（`:16,94`） |
| seenIds / 认领 / 快照上限 | 各 provider `seenIds` **10000**；`usageClaims` 保留 7 天、上限 10000；会话快照 10000 | `src/lib/rollout.js:16041-16050`、`:16680`、`:18089-18091`（seenIds）；`:17375-17405`、`:18058-18064`（claims）；`:19334-19355`、`:19645-19649`（快照） | entry id 去重集合 / Copilot 三通道用量认领 / 非零会话优先保留、零值按 `updatedAt` 倒序补位 |
| 记录大小上限 | `OMP_LIKE_MAX_RECORD_BYTES = 64 MB` / `LMSTUDIO_WINDOW_BYTES = 8 MB` | `src/lib/rollout.js:15712-15715`、`:12853` | 单行/窗口大小上限；**Codex 主路径不设单行上限**（`parseRolloutFile` 只传 `invalidUtf8`，`:2397`） |
| 上传退避 | 60 s 起、上限 30 分钟、指数 `2^step`、step 上限 10（记录时 20） | `src/lib/upload-throttle.js:8-9,109-110,123` | 失败退避；状态经 `upload.throttle.json` 暴露 |
| 云端归档包 | 单包上限 10000 条 / 8 MiB | 服务端硬界 `migrations/20261005120000_cloud-usage-archive.sql:72`（`CHECK(row_count > 0 AND row_count <= 10000)`）与 `:213`/`:309`（`pg_column_size(v_payload) > 8388608` 即抛 `exceeds pack budget`）；客户端 runner `scripts/cloud-usage-archive.cjs` | `cloud_prepare_usage_archive`（同 SQL `:186`）与 `cloud_commit_usage_archive`（`:221`）的两步提交 |
| 后台轻量源集 | `BACKGROUND_AUTO_SYNC_SOURCES` = **4** 项 | `src/commands/sync.js:367` | `acode, codex, every-code, reasonix`；注释称「把无 scope 的 native 5 分钟同步限制在有日期的本地会话树上」 |
| 自动同步源集 | `AUTO_SYNC_SOURCES` = **45** 项 | `src/commands/sync.js:320-366` | 与 45 个 `parse*Incremental` 数量一致；别名表 8 条见 `:310-318` |
| 帮助文本自述 | `sync parses ~/.codex/sessions/**/rollout-*.jsonl …` | `src/cli.js:97-101` | 与实测覆盖范围（45 家）差距悬殊，以源码为准 |

---

## 12. 未验证项与风险清单

分类口径：**A**＝需运行时/真实语料才能判定；**B**＝需外部信息（上游 API、线上部署、真实壳行为）；**C**＝已知设计缺陷（有源码证据但当前版本未修）；**D**＝已结案（列出结论）。每条给判定条件。

### A. 需运行时或真实语料才能判定（18 条）

| # | 事项 | 判定条件 |
| --- | --- | --- |
| A1 | **Claude 增量游标无行边界提交**：`const endOffset = st.size;` 共 3 处（`src/lib/rollout.js:2291`、`:2682`、`:2793`），只有 `:2793` 落到 token 游标（`:1194`）。若同步瞬间读到半行，`readline` 会 yield 未闭合尾行、`JSON.parse` 失败被吞（`:2843-2847`），游标仍推进到文件尾，该行字节被跳过。非对称性：Codex 的 token 路径用 `committedEndOffset`（`:2652`），连项目上下文扫描位也做了提交保护（`:2765`），OpenClaw 另有 `openclawOffsetEndsAtLineBoundary`（`:1949`）——**只有 Claude 一条路径完全没有** | 构造「写入半行 JSONL → 跑 `parseClaudeIncremental` → 补全该行 → 再跑」的定向测试，看补全后的行是否被计入。**注意收窄**：该跳过会在该文件下次 inode 变化或截断时被全量重扫抵消（`:488-504` 的三态判定重置 `startOffset=0`），准确说法是「本代文件内不可恢复」而非严格永久 |
| A2 | **Claude 缓存字段别名未兼容**：`src/lib/rollout.js:4927-4928` 只读 `cache_creation_input_tokens`，而 `src/lib/codex-token-usage.js:25-27` 侧存在 `cache_write_input_tokens` 写法。anthropic 兼容端点（Kimi/DeepSeek/Mimo/MiniMax）若用别名，缓存写会被静默丢为 0 | 统计这些 provider 的合成 fixture 或真实样本里 `cache_write_input_tokens` 的出现率；若该字段非 0 而 `cache_creation_input_tokens` 缺失，即为确定性丢量 |
| A3 | **`<synthetic>` 模型行**：全库 3 处出现（`src/lib/session-analytics.js:101` 集合、`:621-624`、`:1968-1971` 注释），无显式 usage 处理 | 在本机 `~/.claude/projects/**` 里检索 `"model":"<synthetic>"` 并看同行 usage 是否非零；若非零，面板会出现名为 `<synthetic>` 的模型行 |
| A4 | **`legacyGroups`/`groupQueued` 分支对现有安装是否死代码**：`src/lib/rollout.js:3181-3194,3430-3480` 存在「整小时聚合成 `model=DEFAULT_MODEL`」路径，`src/commands/sync.js:216-220` 注释说 `groupQueued` 的存在本身就是 legacy 标记 | 构造含 `groupQueued` 的 v1 游标 fixture，跑 `enqueueTouchedBuckets` 观察输出行数与模型名；若不写该键则为死代码，若会写则存在与修复意图冲突的再膨胀路径 |
| A5 | **同批内同键双行的覆盖方向**：`readQueueBatch`（`src/commands/sync.js:4422-4512`）在同一批内用 `bucketMap.set(...)` 覆盖，取决于文件顺序 | 构造同键双行（旧在前、新在后）的队列 fixture 直接跑 `readQueueBatch`，确认是否以旧覆盖新 |
| A6 | **压缩与上传 offset 的交互边界**：`compactPendingQueue` 只重写 `[offset, size)` 并原样复制 `[0, offset)`（`src/lib/upload-queue.js:97-110`） | 测「pending 区出现跨行边界残缺（写者正在写）时 `iterateQueueLines` 的行为」与「压缩后 offset 恰落在文件末尾」的边界（`input.close()` 与 `fs.stat` 校验之间存在窗口） |
| A7 | **`multiInstallParse` 异常分支的写回语义** | 读 `src/lib/multi-install-parser.js:49-54` 并构造异常路径 fixture |
| A8 | **`claudeHashes` 10 万上限的淘汰窗口**：`src/lib/rollout.js:1236-1237` 超限保留最后 10 万；`fileId` 第二层去重只在「同一次运行内同时看到 `\\wsl$` 与本地路径」时启用（`sawUncFile && sawLocalFile`，`:1035`） | 构造 >100k 哈希 + 同内容双路径的用例，验证是否双计 |
| A9 | **`rebuildingCodexBaseline` 与 `prev.lastTotal` 兜底**（`src/lib/rollout.js:459`） | 读 `:459` 附近上下文与 `rebuildingCodexBaseline` 路径，确认兜底动机 |
| A10 | **Copilot 三通道互斥完备性**：三份实现共用 `source = "copilot"`，靠 `usageClaims`（OTEL↔store）、`shouldCountCopilotAppSession`（app-db 跳过 CLI/VS Code）、`usageClaimsComplete` 三重机制分工 | 给出 `test/copilot-app-parser.test.js` 与 `test/copilot-session-store-parser.test.js` 的**交叉通道用例断言清单**（含「同时活跃」场景的输入与期望输出）；缺该清单则维持未确认 |
| A11 | **`qoder(new)` 全零 token 行是否真的入队**：`src/lib/rollout.js:6639-6660` 构造 `conversation_count = 1`（token 全 0）的 totals 并 `addTotals` 到桶，是否入队取决于 `totalsKey` 比较（`:3325-3347`） | 以 fixture 复现「billable 零 token 行」跑完整 `parseQoderNewIncremental` + 入队，检查队列行是否出现全零 `total_tokens` 且 `conversation_count = 1` |
| A12 | **`trae-cn` 真实响应是否出现全零会话行**：源码层「允许」已确认（`src/lib/rollout.js:22477-22492` 只校验 `Number.isSafeInteger(value) && value >= 0`，`:22500-22508` 写全零 totals，`:22749-22751` 无零值护栏直接 `addTotals`） | 取得一次真实 `query_user_usage_group_by_session` 响应样本（含全零 token 字段的会话行），或用同形 fixture 跑完整 `parseTraeCnApiIncremental` 并检查桶行 |
| A13 | **`reasonix` 跨 home 重复计数**：`reasonixHomeCandidates` 同时返回 `~/.reasonix` 与 `%APPDATA%\reasonix`（`src/lib/rollout.js:16925-16934`），`resolveReasonixTelemetryFiles` 只去重**路径**（`:16970-16975`），**没有按 inode/内容去重**（对比 Copilot 的 `uniqueCopilotDbPaths` 用 `dev:ino`） | Windows 上实测双目录并存且互为拷贝时的表现 |
| A14 | **`trae` 的 `T_STORE` 冲突裁决在三份以上拷贝时**：冲突规则只处理「两份拷贝」（`src/lib/rollout.js:22111`：`previous[T_STORE] !== store && modifiedAt <= previous[T_MODIFIED]`） | 三份以上拷贝轮流被扫描时的最终归属收敛性；注释只声明「不能让陈旧库回撤更新的用量」，未给收敛性证明 |
| A15 | **`grok` 兜底路径的时间归属**：桶起点为 `toUtcHalfHourStart(event.timestamp) \|\| toUtcHalfHourStart(lastActive) \|\| toUtcHalfHourStart(Date.now())`（`src/lib/rollout.js:20249-20252`、`:20265`）；`lastActive` 也缺失时回落 `new Date().toISOString()`（`:19691`），**历史用量会被记进「当前」半小时桶** | 统计该组合在真实老会话上的触发频率 |
| A16 | **`craft` 与 `reasonix` 在极端截断下 `total_tokens` 失真**：两家都用「累计快照 clamp 差值」，Craft 注释承认负面差值是截断信号并 clamp 到 0（`src/lib/rollout.js:17189-17195`），Reasonix 同样 clamp（`:17011`）。若上游重置后在短时间内又累积回原量级，该段「重放」差值会被再次计入；两家都没有 Qoder 那样的「先减旧再加新」回撤路径 | 实测截断/重置场景 |
| A17 | **`mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal` 而非「最后一条」**（`src/commands/sync.js:6540`），与 `readQueueData` 的 last-wins（`src/lib/local-api.js:207`）语义不同 | 确认该函数的所有调用点是否都作用于「已被重写过的干净队列」（真值修复是原子重写，理论上不会出现同键数值回退） |
| A18 | **`model/rerouted` 归属路径从未在真实语料验证**：`src/lib/codex-model-attribution.js:47-51` 自述 `0` 命中；该分支一旦被真实触发，其信封形状与字段名（`fromModel/toModel/from_model/to_model`）都属推断 | 检索本机 rollout 中是否出现 `model/rerouted` / `model_rerouted` |

补充登记（同属 A 类）：`passive-mode.js` 只覆盖 8 个 hook 驱动 provider（`src/lib/passive-mode.js:96-185`），Grok / OpenCode / OpenClaw / omp 的「hook 缺失但日志在」状态由 `src/commands/status.js` 的独立路径分头上报（omp 在 `:1317-1318`、OpenCode 在 `:1363`、Grok 在 `:1452`；OpenClaw 是 hook 状态行 `:1296-1298`，不是「日志在」reader 行），**未逐一核对**；CodeBuddy / WorkBuddy 是否真的消费 `settings.json` 的 `SessionEnd` 只有源码内「fork 同 schema」注释断言（`src/commands/init.js:623-625,1034-1036,1048-1049`），无 schema 校验、无第三方验证；`usage_precision` 的取值域与全部消费方未追踪完整（全文件出现点 16 行、按自然语句计 **15** 条——`:20286-20287` 是一条跨两行赋值——已在 `src/lib/rollout.js` 内清点，「无 antigravity 写入点」这一否定性结论依赖该清单完备）；`codexServiceTier` 的语料统计（1213 文件 / 167 命中 / 25%；814 default / 58 priority）是**上游源码注释里的本机观测**而非本轮实测（`src/lib/codex-service-tier.js:8-15`）。

### B. 需外部信息才能判定（10 条）

| # | 事项 | 判定条件 |
| --- | --- | --- |
| B1 | **云端 ingest 是否会从 `cursors.files[].usageSource` 反推估算行**。「`antigravity` 估算行对下游不可见」这一结论建立在「`usage_precision` 全部写入点清单里没有 antigravity」的**否定性证据**上 | 取得云端 ingest 侧读取 `cursors.files[].usageSource` 的代码或运行证据——能反推则该披露缺口不成立，不能则维持现状 |
| B2 | **edge 五份价格表的线上部署修订**。仓库内无部署产物或版本指针，仅 `CLAUDE.md:42` 提示人工部署（"Deploy the touched edge functions after editing"） | 取线上函数代码或版本指针，与 `dashboard/edge-patches/` 逐份比对 |
| B3 | **`curated-overrides.json` 各 `note` 引用的第三方定价页与表内数字是否一致** | 该表运行在虚构时间线（`claude-opus-5-5`、`gpt-6-astra`、`deepseek-v4` 等），**不存在可核对的外部真实价页**，联网核对无事实源——只能核对表内自洽性 |
| B4 | **LiteLLM 上游当前是否仍以 `model_prices_and_context_window.json` 为唯一价目文件、字段名是否仍为那 4 个成本字段** | 联网核对上游仓库（本轮未做） |
| B5 | **Windows 托盘在本地模式下连续天数是否显示 0**。两侧源码已确认：本地端点硬编码 `streak_days: 0`（`src/lib/local-api.js:2588`，同响应 `:2587` 另给真实 `active_days`），前端 `dashboard/src/hooks/use-activity-heatmap.ts:206,220,284,295` 自行重算覆盖；`dashboard/src/lib/pet-linux-host.js:198` 直读该字段 | 运行 Windows 壳实测显示值 |
| B6 | **`usage-limits` 的 provider 数量口径**：`fetchUsageLimitsUncached` 的 `Promise.all` 解构出 18 个结果变量（`src/lib/usage-limits.js:4055`），但 `data` 对象只收录 17 家（`claudeServiceStatus` 未进入 `data`，`:4406-4440`） | 核对 `ark` 两家与 `qoder` 两家是否共用同一凭据 |
| B7 | **Windows 壳「5 分钟后台 sync 计时器」**：只在 `src/commands/serve.js:305-309` 注释中被提及，**未在壳代码中核实** | 读 `TokenTrackerWin/` 目录源码 |
| B8 | **本地 API 若干端点的字段结构**：`/functions/tokentracker-local-sync` 成功响应的 `syncResult` 键名、`/functions/tokentracker-context-health` 的字段结构、`/api/auth/*` 的完整转发规则（cookie relay、csrf 优先级、relay 回退条件，只读到 `src/lib/local-api.js:1792-1940` 片段） | 逐端点读对应 `if (p === ...)` 分支的参数与响应载荷 |
| B9 | **榜单/社区表的 SQL 细节**（`scripts/ops/*.sql`、`migrations/`） | 只通过测试注释间接确认，需逐行读 SQL 正文 |
| B10 | **云端压缩表的保留期限**：`docs/PRIVACY.md` 与 `docs/cloud-guide.md` 提到「compressed usage metrics 可长期保留」，但 `docs/cloud-usage-archive.md:74-83` 明确写「未启用」 | 取云端归档表的实际启用状态与保留策略 |

补充登记（同属 B 类）：macOS 菜单栏小组件的刷新频率（`WidgetCenter.reloadAllTimelines` 之后的系统策略）与「快照 schemaVersion 2 的兼容迁移」细节；`TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源（推测同为本地 API 或经 `UsagePoller` 推送，**未验证**）。

### C. 已知设计缺陷（有源码证据，当前版本未修）（24 条）

1. **`cmdSync` 巨型编排**：**3194** 行单函数、`skipNoOpCursorCommit` 引用 13 个条件项（`src/commands/sync.js:516-3709`、`:3424-3437`）。
2. **`.reclaim` 闸链需迭代清扫才能自愈**（`src/lib/fs.js:178-204`，issue #431）。
3. **队列写入无写侧互斥**：主队列 append 写点至少 6 处（`src/lib/rollout.js:3483`、`:20031`、`:22594`；`src/commands/sync.js:5126`、`:5207`、`:5281`），另 `src/commands/sync.js:4114` 写 fallback、`src/lib/rollout.js:3536` 写 project 队列。
4. **`repair` 的「先删后建」根因未变**：0.26.3 真实数据丢失事故（`src/commands/sync.js:234-248`，2.174B ≈ 21.7 亿 token），已用守卫弥补但模式仍在；`dropMimoQueueRows`（`:4744`）与 `repairClaudeQueueFromGroundTruth`（`:6844`）属同类。
5. **Grok 分支无条件覆盖、无备份**（`src/lib/grok-hook.js:85-91`）。
6. **同一语义三套并行纠错机制**（一次性 repair 键 / 读时 `normalizeQueueRow` / 云端整行 upsert），口径需人工同步；历史事故：云端 `conversation_count` 恒为 0 而本地正确（`src/commands/sync.js:249-256`）。
7. **16 MB 体积阈值静默切换存储形态**，且 legacy 与 v2 能力不等价（`src/lib/cursor-store.js:21,77-86,165-197`）。
8. **代理无法生效时整个进程 `exit(1)`**（`bin/tracker.js:36-40`：判据是 `applyResult.unprotected === true`，即手动代理未能生效**且**无法阻断出站流量），hook 路径同样受影响 → 采集静默停摆且无面向用户告警。
9. **`local-api.js` 3543 行职责混杂**（读队列 / 项目归因 / 账号体系 / 头像代理 / 宠物 / 静态资源 / SPA 回退）。
10. **测试专用钩子进入生产函数签名**（`context.traeCnFetchImpl`、`context.cursorSyncDeps`、`context.scanSources`，`src/commands/sync.js:519-538`）。
11. **依赖 `st.ino` 检测原子替换**，在网络盘/容器叠加文件系统上会失效（`src/lib/rollout.js:489-492`）。
12. **Windows 7680 端口占用靠「WSL 默认换 7681」绕行**（`src/commands/serve.js:15-21`）。
13. **`hookStatus` 传参与消费不一致**：传入 11 键中有 3 键（`opencode_plugin`/`openclaw_session_plugin`/`grok`）不在 passive-mode 的消费列表内（冗余传参）；`passive-mode.js:88-90` 的 JSDoc 自述形状与实现**双向不一致**；`workbuddy-ai` 键名风格与其它下划线键不一致（`src/commands/status.js:1024-1036` vs `src/lib/passive-mode.js:93-186`）。
14. **`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 双份定义且无测试绑定**（`src/lib/pricing/index.js:31` 零内部读取；生效者 `src/lib/codex-rollout-parser.js:51`）。
15. **推理折叠名单两套**：计价侧 5 个（`src/lib/pricing/index.js:248-253`），token 审计侧 2 个（`scripts/audit-token-correctness.cjs:34-40`），无测试把两者绑在一起。
16. **云端与本地对 9 个 `curated.exact` 键的覆盖不同**，其中 `grok-build-free`、`grok-4.5-build-free` 是价格层面的实际分歧（`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 比对）。
17. **`PI_SUBSCRIPTION_SOURCES` 本地 4 个成员、云端 2 个成员**（`src/lib/pricing/index.js:18-23` vs `tokentracker-leaderboard-refresh.ts:575`），`prime-agent-*` 两条在云端不会短路为零。
18. **云端 `computeRowCost` 没有长上下文 / Fast 溢价**（`tokentracker-leaderboard-refresh.ts:569-613` 无 `long_context_*` / `priority_*` 分支），且无测试断言「云端结果必然 ≤ 本地结果」。
19. **金额精度三处不同**：`toFixed(6)`（`src/lib/local-api.js:2415`、`:2648`）、浮点原值（`src/lib/session-analytics.js:289`）、取整到分（`tokentracker-leaderboard-refresh.ts:1349`）。
20. **半小时桶两份实现无等价性锁定**（`src/lib/rollout.js:4173-4190` vs `src/lib/claude-categorizer.js:1175-1191`，副本无测试）。
21. **Codex「reasoning 不双计」无对等回归测试**：`source` 白名单漏加会静默双计（`src/lib/pricing/index.js:248-256`）。
22. **同一面板两套时间口径**：热力图网格的日期轴与单元格键按 UTC 日（`src/lib/local-api.js:2540`、`:2563`），网格数据却来自按请求时区聚合的 `aggregateByDay`（`:2537`、`:2545`），`night_owl` 徽章按请求时区小时（`:482`）；另有 `hourly` 端点 `day` 默认值（UTC 今天）与其过滤谓词（当地日键）**基准不同**。
23. **`pricing.source` 元数据在多端点是硬编码标签而非真实来源**；`streak_days` 恒 0、`week_starts_on` 恒 `"sun"`（`src/lib/local-api.js:2586,2588`），而云端 compact 载荷会带真实值（`src/lib/heatmap-compact.js:108-109` 透传）——同一 UI 在本地/云两条路径下显示可能不一致。
24. **Claude 真值修复与增量解析的口径不一致**（至少三处：usage 取值路径 `obj.message.usage \|\| obj.usage` vs 仅 `obj.message.usage`；reasoning 字段名 `output_tokens_details.thinking_tokens` vs `usage.reasoning_output_tokens`；output 是否拆分），且修复是**一次性且不可逆**（重写 `queue.jsonl` + `cursors.claudeHashes` 全替换，`src/commands/sync.js:6871-6921`）。

补充登记：项目队列丢失模型信息——项目行的 `model` 列被裁剪（`src/lib/rollout.js:3515-3532`），而项目详情端点的 `models` 字段实际装的是 **source**（`src/lib/local-api.js:2877`），下游若按「按模型」理解会出错；`projectBucketKey` 布局 `projectKey|source|hourStart` 与 `parseBucketKey` 的 `source|model|hourStart` 不通用，两套键共用一个分隔符而解析器不匹配（`src/lib/rollout.js:4228` vs `:4238`）；Claude 项目身份的 `.git` 后缀不剥离（`src/lib/rollout.js:4429-4442`）；`src/lib/rollout.js:6726` 悬空注释（称 credits 会累积，但下一行即 `touchedBuckets.add`，上下文无实现）。

### D. 已结案（13 条）

| # | 结论 | 依据 |
| --- | --- | --- |
| D1 | `resetWslProbeCache` **生产调用为 0**（只有定义与导出），「缓存刷新时机不对」不构成缺陷；长驻进程内新增发行版不会被发现这一现象依然存在，属新功能而非修复 | `src/lib/wsl-probe.js:11`、`:268`；唯一调用者是 `test/*.test.js` |
| D2 | `init --link-code` 是**遗留参数**：init.js 内只用它关闭浏览器认证（`src/commands/init.js:1423,339`），`link_code_state.json` 在 init.js 内无任何读写（`:176,234,359` 仅为声明与传参），全仓零实际引用 → 帮助文本对该开关的暗示存在误导 | `src/lib/runtime-config.js:149`（清理清单）；`src/cli.js:78,92` |
| D3 | KiroCli 会话文件侧字段**不是**模块注释所写的 `user_prompt_length`/`response_size`，而是 `turn.input_token_count` / `turn.output_token_count`；仅两者皆 0 才回退 4 字符/token 近似；v2 支路另读 `messages.jsonl` 且**不含 token 字段** | `src/lib/rollout.js:8371-8372`、`:8316`、`:8482`、`:8475-8481` |
| D4 | Opencode storage 下层结构：`walkOpencodeMessages` 递归遍历且只收 `msg_*.json`；`listOpencodeMessageFiles` 只负责拼 `<storage>/message` 一层 | `src/lib/rollout.js:5025`、`:5033`、`:243` |
| D5 | Anythingllm 的 SELECT 本体为 `FROM workspace_chats`，列经 `json_extract(response,'$.metrics.*')`，SQL 层只过滤 `whereClause`、**不做零值过滤**（纯 total 行在 JS 侧被丢弃，与源码自洽） | `src/lib/rollout.js:13874-13887` |
| D6 | Cline 的目录解析与扫描都在 `src/lib/rollout.js`（**不在** sync.js）：`resolveClineSessionsDirs`（`:12054`，Windows 双安装 + WSL 探测 `:12069-12073`）与 `scanClineSessionFiles`（`:12086`） | 同上；`~/.cline/data/sessions` 与 `src/commands/sync.js:2110` 注释一致 |
| D7 | Kimi / KimiCode 的 home 优先级已逐字确认 | `src/lib/rollout.js:9493`（`KIMI_HOME` `:9495`，win32 WSL 分支 `:9497-9502`）、`:9669`（`KIMI_CODE_HOME` `:9671`） |
| D8 | Codebuddy 的 JSONL 支路闸门：现代 CodeBuddy 把用量写进 `providerData.rawUsage`；该支路零值判定为含 `reasoningTokens` 的五桶闸门 → 归「D 类」，F 类清空 | `src/lib/rollout.js:9940-9943`、`:10476-10485` |
| D9 | Workbuddy traces 支路字段**与 JSONL 支路不同形**：traces 用 `modelInfo.total{Input,Output,Cached}Tokens`，JSONL 用 `providerData.rawUsage.*`；`src/lib/rollout.js:11246` 属**第三条 SQLite 支路** | `src/lib/rollout.js:11100-11260` 段 |
| D10 | vite dev **不复制价格表**：`dashboard/vite.config.js:508-514` 用 `createRequire` 直接 require 同一 `src/lib/pricing` 模块；唯一差异是 dev mock 对按模型聚合后的 totals 只计一次价（`:1052-1057`），仅影响 dev | 同上 |
| D11 | 上游价格表键数：edge `MODEL_PRICING` = **119** 键（+ iFlytek MaaS 64 键）；`curated-overrides.json` 与云端差集 = **9 键**（逐字一致） | `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142-332`、`:339-404`；`test/edge-pricing-parity.test.js:76-85` |
| D12 | 三份 UTC 半小时桶实现**语义等价**（`rollout` 权威、`claude-categorizer` 副本、sync 的 grok 毫秒版），此前自标的「未确认」已解除 | `src/lib/rollout.js:4173-4190`、`src/lib/claude-categorizer.js:1175-1191`、`src/commands/sync.js:4538-4549` |
| D13 | 关键行数断言经逐字重算：`rollout.js` **24932**、`sync.js` **7251**、`init.js` **2427**、`local-api.js` **3543**、`session-analytics.js` **2934**、`status.js` **1729**、`pricing/index.js` **378**、`pricing/matcher.js` **439**、`curated-overrides.json` **775**、`upload-queue.js` **134** | `[System.IO.File]::ReadAllLines(...,UTF8)` 逐个重取 |

---

## 13. 复核记录（R1 / R2 / R3 / R4）

本研究不是单轮产物，而是**研究 → 对抗式复核 → 修正 → 再复核**的滚动闭环（截至本文共三轮：R1 六单元、R2 三单元、R3 四单元）。以下台账数字取自各复核报告自报的「抽样统计」节；每轮样本仓库 HEAD 全程未变（`b19d27b0ccd59ce621f4fa17207b06358e67987c`），复核过程**未修改样本仓库**。

### 13.1 第一轮复核（R1，6 个独立复核 agent）

| 轮次 | 对象 | 抽验条数 | E | I | U | 通过 | 关键发现 | 处置结果 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R1-A | 采集管线（触发/调度/游标/队列） | 139 | 9 | 9 | 10 | 111 | 队列写点**不是一处**（实为 6+ 处）；`skipNoOpCursorCommit` 条件项是 13 个不是 9 个；`cloneOrCopy` 是 reflink 不是硬链接 | 全部采纳并回改 |
| R1-B | Claude Code 与 Codex 深度 | 46 | 1 | 8 | 4 | 33 | 「45 个 parser 存在三族签名」实为**无统一契约**（45 声明中 5 个转发薄壳、40 个自持 return、去重后 **11 种键集合**）；Claude `endOffset` 三处分工需分清 | 采纳；同时对复核报告**反向纠正 4 处**（见 13.3） |
| R1-C | 19 家 JSONL 类 provider | 141 | 12 | 7 | 2 | 120 | KiroCli「全部靠字符估算」被**证伪**（真实 token 字段存在）；行号清单 40 条中 5 条错误，**全部落在 ≥7000 行且偏移量与 GBK 口径吻合** | 采纳；错误全部改正，并写明 GBK 漂移机制 |
| R1-D | SQLite/API 类 provider | 188（代码块行号 62 条逐字比对） | 9 | 6 | 3 | 170 | 「21 家」与「23 个 `parse*` 函数名」的口径关系需定义；「唯一一处全零 token 行」实为**两处**（qoder new + trae-cn），craft 反而应作反例（此项在 R3 被进一步重建为 **≥5 处解析器构造点**，见 §5.6.2） | 采纳；计数口径写入结论速览 |
| R1-E | 聚合、查询与去重 | 157 | 8 | 6 | 7 | 136 | `conversation_count` 是**主会话 user 文本消息**而非 assistant 消息（源码注释与 Craft 侧注释互相冲突，以代码为准）；`toUtcHalfHourStart` 调用数需分口径（`rollout.js` 70 次出现 / 66 行） | 采纳；同时确证两套口径（last-wins 与 setMaxTotal）**并存不矛盾** |
| R1-F | 定价与消费端 | 101 | 11 | 7 | 3 | 80 | `MODEL_PRICING` 键数 183 → **119**；edge-only 44 键 / curated-only 9 键；`savings` 类词命中从「1 处」改 **9 处 / 7 文件** | 采纳；行号与键数全部重算 |
| **合计** | — | **772** | **50** | **43** | **29** | **650** | — | E/I 共 93 条全部采纳或按修正后版本落实；U 组 29 条保留并补判定条件 |

R1 还产生若干**结论级推翻**（不只是行号微调）：除上表所列，另有「三份 UTC 桶实现语义等价」「Codex 连项目上下文扫描都做了提交位保护而 Claude 没有」「reasoning 在两侧的处理方向相反（Codex 是子集标记并置 0，Claude 是独立加成项并照 output 价）」。这些都已按修正后版本写入正文。

### 13.2 第二轮独立复核（R2，3 个复核 agent）

| 轮次 | 对象 | 工作量 | 关键发现 | 处置结果 |
| --- | --- | --- | --- | --- |
| R2-1 | 管线类文档的修正点复查 + 独立抽样 + 跨文档一致性 | A 组复查 40 条：**35 ✅ / 4 ⚠️ / 1 ❌**；B 组独立抽样 40 条：**未发现新的实质性错误**（含 8 条「文档未覆盖但源码如此」的补白）；C 组 13 处比对：**未发现互相矛盾的断言**（2 处行号范围写法差异属可接受近似） | 必须修 3 条，均为**表述级**（不影响机制结论）：一句会被自身文件证伪的元断言、「项目行其余同构」与源码不符（缺两列）、opencode 游标字段清单漏 1 个字段。**R1 点名的三类高风险项（notify.cjs 模板边界、队列「唯一写点」、OpenCode 哨兵范式）经独立回源码复核均已改对。** 结论：**没有发现会影响移植决策的机制性错误** | 3 条表述级修正全部落实 |
| R2-2 | provider 类文档的修正点复查与继续证伪 | A 组复查 40 条（S3a 8 + S3b 15 + 收紧/结案 17）；B 组独立抽样 54 条；C 组横向一致性 14 处；另对两份分片全部 686 条 `rollout.js` 行号引用做机械筛查；在修正记录之外又查出 **6 处残留漂移行号** | 残留漂移的偏移量与文档自述的漂移表**精确吻合**（≈10700 行处 +6、≈13700 行处 +10、≈15300 行处 +10），说明「只查清单内 40 条」的自查会过滤掉散落在正文里的真错项；另：R1 遗留的一处未验证项（Workbuddy traces 字段）本轮**结案**（见 D9） | 6 处全部改正；U 组减 1 条转 D 类 |
| R2-3 | 聚合/定价/消费端的修正点复查 + 新抽样 + 端到端链条 | A 组复查 **49** 条（S4 18 / S5 17 / S6 14）+ B 组独立抽样 **46** 条（S4 18 / S5 16 / S6 12）+ C 组端到端 10 处；计数类断言重算 21 组；新增判定 E2 / I8 / U1·U6 | 端到端链条**未发现金额重复计入或漏计**的实证；新发现两处基准不一致：`hourly` 端点的 `day` 默认值（UTC 今天）与其过滤谓词（当地日键）基准不同、多端点 `pricing.source` 是硬编码标签而非真实来源 | 已登记进 C 类（见 §12 C22、C23） |

R2 的净收益是**两类**：把 R1 修正后的文本再筛一遍（抓出 6 处残留漂移 + 3 条表述级问题），并对 R1 较少触及的章节做全新抽样与补白。R2 明确否定了「修正记录写完就等于干净」的假设。

### 13.3 被反向纠正的复核结论（可信度证据）

复核报告本身也被证伪过——**这是本文可信度最高的部分**，因为它说明修正环节不是单向服从：

1. **`recordsProcessed: total` 被误读为新增键**。复核清单曾把 `parseCursorApiIncremental`、`parseTraeCnApiIncremental` 的返回字面量判为「多一个键 `total`」，把 `parseGrokBuildIncremental` 判为「键重复」。实测三处都是**值标识符被当成键名**：真实写法是 `recordsProcessed: total`（`src/lib/rollout.js:7270`、`:22791`）与 `recordsProcessed: eventsAggregated`（`:20551`）。它们的键集合分别是骨架 C、骨架 C+`skippedRows`、骨架 D，**没有多出的键也没有重复键**。
2. **「12 种返回字面量」实为 11 种**。复核漏掉了 `parseCopilotSessionStoreIncremental` 的 9 键集合（`src/lib/rollout.js:19003-19013`），同时把上述 3 个值标识符计成了键。
3. **`rollout.js` 行数：23260 → 24932**。复核报告自身也测到 24932，却判定「数据一致」，属**漏检**——这一条直接决定了全文行号口径的权威性（本文采用 UTF-8 口径的 24932）。
4. **「误引 `:2682`」不成立**。复核称某处误引了 `:2682`，实测该文档全文检索 `2682` **零命中**，误引不存在；同时复核把 `:2291` 归为「项目上下文扫描」，实测它属于 `parseRolloutFile` 的「无新字节」早退。最终只补了 3 处分工表，未删任何引用。
5. **macOS 壳 heatmap「不传 tz」的暴露面判定被反驳**。R2 曾断言「macOS 壳的 heatmap 调用不带 `tz`（`APIClient.swift:73-75` 只构造 `weeks` 参数）→ 是实际触发面」。实测 `:73-75` 只是 `queryItems` 数组字面量，整数组被传给 `withAccountQueryItems`，而它是在 `withTimeZoneQueryItems` 之上再追加 `account=1`（`APIClient.swift:241-246`、`:252-254`）——**macOS 壳的 heatmap 实际带 tz**，该结论不成立，予以反驳。
6. **Droid 行号被二次推翻**。R1 曾指出引 `15333/15338` 有误并给出「修正」；R2 逐行复核后判定修正本身仍错——实际注释在 `15343-15346`、调用点在 `15348`，`15333` 是对象字面量闭合 `};`、`15338` 是 `const bucketStart = toUtcHalfHourStart(`，两处恰好为 **+10 漂移**。
7. **「仅由 PowerShell 定位的 40 条自查清单」本身是错的**。该清单把 35 条**正确**的行号标为「未复核」，却漏掉真正的错项；更严重的是其中一条自称「已 read 确认」的引用实际是 GBK 口径附近位。教训已写入方法学：**「自称已确认」不能替代逐条重验**，自查清单本身也会骗人。

### 13.4 行号口径的硬结论
- 本仓库有多行以多字节字符结尾；PowerShell 的 `Get-Content`（不指定编码）在中文环境按 GBK 解码，会把行尾字节与随后的换行一起吞掉，导致**行号整体偏移（单文件最多 15 行、全 `src/` 累计受影响 52 行）**，且**漂移量随位置累积**（≈10700 行处 +6、≈13700 行处 +10、≈15300 行处 +10、文件末尾 +15）。
- 权威口径只有两种：`read` 工具，或 `[System.IO.File]::ReadAllLines($path, [System.Text.Encoding]::UTF8)`。本文所有行号已按该口径校验；复核过程中发现的历史错误**全部落在 ≥7000 行**，且偏移量与上述漂移表吻合。

### 13.5 第三轮复核（R3，4 个复核 agent）

R3 把全文切成四段并行复核，四份报告的抽样范围互不重叠；下列规模数字均取自各报告自报值：

| 复核单元 | 复核范围 | 抽验规模 | 命中率 | 判定条数 | 处置 |
| --- | --- | --- | --- | --- | --- |
| R3-1 | 元信息 + §1–§4 | 70 个引用簇 / **182** 个行号点（另 8 条全库级检索） | 99.5%（182 点中 1 点偏差，其余最差只差 ±1–3 行邻域） | **必须修 7 处**（完整清单 12 条，含低优先与可选） | 按清单回写 §1–§4 |
| R3-2 | §5–§6 | **72** 条行号抽样 | 88.9%（64/72） | **必须修 17 条**（P0×4 结论性 / P1×7 计数与硬错 / P2×6 偏移与口径） | 按清单回写 §5–§6 |
| R3-3 | §7–§9 | **112** 条（表格行口径） | 92.0%（103/112；行号级命中 108/112） | **必须修 12 项**（S1–S9 + 3 项低危） | 按清单回写 §7–§9 |
| R3-4 | §10–§13 + 附录 A/B | **112** 条 | 94.6%（106/112） | **必须修 6 条（E1–E6）+ I 级 5 条（I1–I5）** | **本轮全部落实**（见下） |

**问题类型分布**（四份报告高度一致，全部落在引用与计数层，不在机制叙述层）：

- **引用越界**：R3-3 S1——`docs/quality-per-dollar.md` 全文仅 67 行，而文档引其 `:154`/`:156-157`（真出处是 `src/lib/outcomes-engine.js:154,156-157`）。
- **行号错指**：R3-2 的 4 处硬错（WorkBuddy `911`、Kimi 路径 `9501`、opencode `12405`、`codex-service-tier.js:17`）+ 2 处函数/守护定位错 + 3 处 1–2 行偏移；R3-3 S2/S4；R3-4 I1/I2/I4。
- **否定性断言被证伪**：R3-2 有五处「唯一 / 只有两处 / 五家 / 没有任何一家」经重建判定集合后被推翻或必须限定（A 类唯一性、全零落桶两处、数据库/API 子集判空、`$0` 行来源清单）；R3-3 E5 同族。
- **路径虚构**：R3-4 E1——`src/lib/cloud/usage-archive.js` 整仓不存在（无 `src/lib/cloud/` 目录），而该行数值 10000 条 / 8 MiB 落在 SQL 里**为真**，属「引用虚构、数值正确」。
- **计数偏差**：R3-2 P1 组与 R3-4 的 E2/E3/E4/E5（`usage_precision` 语句数、§13.2 的 R2-3 工作量、`cmdSync` 行数、passive 未消费键数）。

同一轮也给出三条**正面结论**，它们构成全文可信度的上界证据：

1. **附录 A 零差异**。R3-4 用正则独立重生成 `src/lib/rollout.js` 的顶层函数表（实测 572 个顶层函数，筛出 45 个 `parse*Incremental`），函数名 / 起始行 / 覆盖行数与文档表**逐行完全一致（差异数 = 0）**，无 ±1 偏差；`sync.js` 侧 52 个调用点与 source 开关位抽查 **52/52 全中**，附录 A 的三点说明（薄壳/共享实现、`parseDumateIncremental` 不在本表、46 个 `sourceAllowed` 字面量）全部成立。
2. **§11 常量取值零错误**。27 个常量/阈值逐项复算（200/500/10、90 s、20 s、16 MB、272 000、DeepSeek `Date.UTC(2026,7,22,16,0,0)` 与 ×0.5、24 h TTL、10 万哈希、2000、45/8/4 源集等）取值全部为真——E1/E6 是**引用**问题，不是取值问题。
3. **§8 计价事实全部通过独立复算**。三处前置短路集合、四桶算式与除数、reasoning 折叠的 5 个 source、272 000 阈值与长上下文溢价公式、priority 交集公式、DeepSeek 峰谷窗口与 `acode` 豁免、价格表计数（84 / 47 / 1 / 64、2284 模型、`cache_read` 566 / `cache_creation` 188）逐项与源码一致，**未发现任何编造的算法或数字**。

**R3-4 的 11 条已全部落实，无一条被判定为不成立**（本轮逐条回源复核后改写）：E1 改为 `migrations/20261005120000_cloud-usage-archive.sql:72,213,309` + `scripts/cloud-usage-archive.cjs`；E2 计数口径改为「16 行、按语句 15 条」并注明 `:20286-20287` 跨两行；E3 补全 R2-3 工作量（A 49 = S4 18 + S5 17 + S6 14，B 46，C 10）；E4 `cmdSync` 改为 **3194** 行（`:516`–`:3709`，实测 `:3709` 是闭括号、`:3711` 起为 `parseArgs`）；E5 未消费键由 2 改 **3**（+`grok`）并补 `passive-mode.js:88-90` 的 JSDoc 双向漂移；E6 删去 sessions 行末的错误引用 `:95`（该行是 `PROJECT_USAGE_MAX_ENTRIES = 10`）。I1 `isCodexHistoryCovered` 收窄为 `:6316-6336`、I2 `validateCodexRebuildFileSnapshots` 收窄为 `:6294-6314`、I3「字面相同」改为「求值相同、字面不同」、I4 把 Grok/OpenCode/OpenClaw/omp 的上报路径拆成 `status.js` `:1452` / `:1363` / `:1296-1298` / `:1317-1318`、I5 把 `exit(1)` 判据收窄为 `unprotected === true`。

顺带核对 §13.2 的 R2 行工作量数字（逐份打开 R2 报告自报值）：R2-1 行原本已一致（A 40 / B 40 / C 13）；R2-2 行原缺工作量，本轮补 A 40 / B 54 / C 14；R2-3 行原漏 S6 段 14 条与整个 B 组，本轮补为 A 49（S4 18 / S5 17 / S6 14）+ B 46 + C 10。

本轮分工作为：§10 及其之后（含本小节与附录 A/B）由 FIX-R3-C 修正；R3-1/2/3 的条目落在 §1–§9，由同一修正批次的对应单元回写。**R3 发现的全部问题均已在本轮修正，无遗留 TODO。**

### 13.6 R3 修正轮的采纳与反驳台账

三个修正单元的结果（逐条都回源码独立确认后才落笔）：

| 单元 | 范围 | 采纳 | 反驳 | 说明 |
| --- | --- | --- | --- | --- |
| FIX-R3-A | §1–§6（R3-1 的 7 处 + R3-2 的 17 条） | 24 条 | **3 条** | 见下「被反驳的复核结论」 |
| FIX-R3-B | §7–§9（R3-3 的 S1–S9 + 低危 3 项中的 10 条） | 10 条 | 0 条 | 含把 zcode 写盘方式从「未确认」升级为已证 |
| FIX-R3-C | §10–§13 + 附录（R3-4 的 E1–E6 + I1–I5） | 11 条 | 0 条 | 并补全 §13.2 的 R2 工作量数字 |

**被反驳的复核结论（3 条，均保留原写法）**：

1. **R3-1 必修 2「45 家拆解应为 15 / 1 / 15 / 14」不成立**。修正单元逐分支重数 `src/commands/init.js:674-1149` 与 `SUPPORTED_PROVIDERS`（`:119-165`，实测 45 项）后确认：
   **写配置 12 + 仅探测 1 + 纯被动 17 + 仅欢迎名单 15 = 45**，四类互斥、并集无缺无重（脚本验证 missing/extra/dup 均为空）。
   复核方多算 3 家（把 TRAE、Cursor、OmO 计入「写配置」）才得到 15/1/15/14。本文保留 **12/1/17/15**，并在 §3 补了口径定义与名单顺序以消歧义。
2. **R3-1 必修 10「骨架 D 代码块写 `:16080`」不可复现**：全文 `16080` 零命中，代码块与表格均为 `:16084`，与 `src/lib/rollout.js:16084` 的 `return {...}` 一致。
3. **R3-1 必修 11「`setMaxTotal` 调用点不在 `sync.js:6540`」不成立**：`:6540` 正是 `mainUsageHourTotalsFromQueueRows`（定义 `:6529`、返回 `:6542`）内的 `setMaxTotal(...)` 调用；函数定义在 `:6585` 与之不矛盾。

**跨区一致性残留（由主线在修正批次之后亲自收尾）**：`cmdSync` 行数统一为 3194；GBK 偏移量表述统一为「单文件最多 15 行、全 `src/` 累计 52 行」（元信息头部、§3、§7、§13.4 四处）；§13.1 的 R1-D 台账补注「该项在 R3 被进一步重建为 ≥5 处解析器构造点」；§10 与 §12 中热力图时间口径的引用由单点 `:2540` 扩为「UTC 构造的网格终点 `:2540` + UTC 单元格键 `:2563`」，并补出网格数据实际来自按请求时区聚合的 `:2537`/`:2545`，与 §7.3 的写法对齐。

### 13.7 末轮验收复核（R4，2 个复核 agent）

R4 只做两件事：**验证 R3 修正是否真的落到文本里**，以及**做一轮全新抽样**（点位避开前三轮）。

| 单元 | 范围 | 修正点验收 | 全新抽样 | 结果 |
| --- | --- | --- | --- | --- |
| R4-1 | 元信息 + §1–§6 | **29/29 通过**（R3-1 清单 12 条 + R3-2 清单 17 条逐条回源） | 50 条 / 约 80 个行号点：命中 47 条（**94.0%**）／77 点（96.3%） | 3 条【低】引用校准 |
| R4-2 | §7–§13 + 附录 A/B | **32 条：31 成立、1 条为改写新引入的错误** | 44 条：通过 43（**97.7%**）、硬错 0 | 2 条【中】+ 2 条低危 |

**R4-1 对 §13.6 三条反驳的独立裁决：全部判修正方胜、原复核方错**——① 45 家拆解 `12/1/17/15` 成立（逐行枚举 `src/commands/init.js:674-1149`：写配置恰 12、仅探测 1（Cursor，注释 `no hooks needed`）、纯被动 17、仅欢迎名单 15，复核方误把 Cursor/TRAE/OmO 计入「写配置」）；② `16080` 在 `rollout.js` 全文零命中，正确值 `16084`；③ `setMaxTotal(` 的调用点为 `:6492`/`:6512`/`**:6540**`/`:6559`/`:6579`（定义 `:6585`），`:6540` 确在 `mainUsageHourTotalsFromQueueRows`（`:6529`–`:6542`）之内。

**R4 发现的问题已全部修正（7 处，均为引用级，无机制性错误）**：
- §7.5 守卫表把「项目键集合守卫」写成「在 `:5823-5880` 之外」——它其实在同一函数体**之内**（`:5841-5846`），只是不属于 `mergeMaxTotals` 调用族；已改写。
- §7.5「四种安全模式」段与同节 #11 表格互相否定（前者说 `migrateLegacyDeepseekHarnessSource` 不追加撤回行，后者说追加显式零行）——已按源码（`:3929-3943` 确为零值行）统一：它保留全部既有行，但会对受影响旧键追加零值撤回行并重排规范行。
- §9.1.2「`status` 读取四个状态文件」——实际 `src/commands/status.js:199-208` 定义 10 个路径常量，已改为完整列举。
- §7.3 热力图「单元格查表在 `:2545`」——`:2545` 是构建 `byDay` 索引，查表在 `:2564`；已改。
- 三处低级引用校准：goose 五桶 `14670` → `14674-14686`（两处）；qoder 旧库 `cache_creation_input_tokens: 0` 的 `6177-6178` → `6173`；opencode 双支路求和 `sync.js:1384` → `:1385-1386`。

**至此四轮复核全部闭环**：R4 之后无「必须修」级别遗留，且三轮全新抽样的命中率稳定在 94%–99.5% 区间，未再出现机制性错误或幻觉。**残留的只是 §12 已登记的 `【未能确认】` 项（需运行时/语料/线上部署才能判定），它们不是缺陷，而是有明确判定条件的开放问题。**

### 13.8 最终验证（R5）

R4 修正之后再派一个独立 agent 做终检，方法与结果：

- **最后 7 处修正逐条回源**：全部确认写对（守卫归属、DeepSeek 迁移的写盘三要素、`status` 的 10 个路径常量、热力图 `byDay` 构建/查表/构造三行、goose 五桶区间、qoder `cache_creation: 0` 行、opencode 双支路求和）。
- **确定性抽样 30 条**（930 条去重引用按字符串排序、步长 31 取索引 7/38/…/906，避免主观挑选）：**30/30 命中（100%）**，错例 0。
- **结构检查**：96 个代码围栏全部闭合、116 张表格列数与分隔行全部一致、55 处 `§X.Y` 交叉引用全部命中真实标题、160 处裸行号抽样 22 条全部可锚定。
- **R5 提出的 4 条非阻塞遗留（J1–J4）已由主线全部收尾**：J1 删除 §7.5 中与源码 `:3925-3927` 抵触的「不在尾部追加规范行」（唯一一处内部矛盾）；J2 统一 goose 引用为 `14674-14686`；J3 把 opencode 求和引用放宽为 `:1385-1388`（求和表达式实际跨该区间）；J4 给 §9.6.2 的 macOS 表头补上文件名 `APIClient.swift`，并逐行复核该表 11 个行号（`:43`/`:66`/`:73`/`:79`/`:86`/`:94`/`:101`/`:110`/`:119`/`:127`/`:148`）与源码函数一一对应。

**最终状态**：全文 4323 行、1047 处 `路径:行号` 引用，机械检查确认**全部引用无越界、无指向不存在文件**；四轮复核 + 终检共发现问题 100% 处置完毕（含 3 条被反向否定的复核结论），无遗留「必须修」项。

---

## 附录 A：45 个解析器行号索引

生成方式：以 UTF-8 口径 `[System.IO.File]::ReadAllLines($p, [System.Text.Encoding]::UTF8)` 读取 `src/lib/rollout.js`，用正则 `^(async\s+)?function\s+\w+` 收集全部**顶层**函数定义行（实测 572 个），筛出函数名匹配 `parse\w*Incremental` 者（实测 **45** 个），区间以「下一个顶层函数定义的上一行」截断。该数量与 README 自述的「45 AI coding tools」一致。

| # | 函数名 | 起始行 | 覆盖行数 | `sync.js` 调用点 |
| --- | --- | --- | --- | --- |
| 1 | `parseRolloutIncremental` | 251 | 457 | `src/commands/sync.js:1046`（经 `runCodexParse`）、`:5769` |
| 2 | `parseClaudeIncremental` | 954 | 288 | `:1201` |
| 3 | `parseGeminiIncremental` | 1242 | 170 | `:1258` |
| 4 | `parseOpencodeIncremental` | 1412 | 170 | `:1361` |
| 5 | `parseOpenclawIncremental` | 1654 | 271 | `:1105`、`:4117` |
| 6 | `parseOpencodeDbIncremental` | 5761 | 291 | `:1375`、`:1665` |
| 7 | `parseQoderDbIncremental` | 6218 | 198 | `:1471`、`:1549` |
| 8 | `parseQoderNewIncremental` | 6552 | 300 | `:1509`、`:1599` |
| 9 | `parseClaudeScienceIncremental` | 7051 | 90 | `:1642` |
| 10 | `parseCursorApiIncremental` | 7172 | 108 | `:2173` |
| 11 | `parseKiroIncremental` | 7472 | 132 | `kiroResult` 段，source 开关 `:2307` |
| 12 | `parseHermesIncremental` | 7900 | 180 | `:2358` |
| 13 | `parseKiroCliIncremental` | 8886 | 393 | source 开关 `:2429` |
| 14 | `parseKimiIncremental` | 9528 | 141 | `:2516` |
| 15 | `parseKimiCodeIncremental` | 9723 | 187 | `:2544` |
| 16 | `parseCodebuddyIncremental` | 10164 | 492 | `:2594`、`:5409` |
| 17 | `parseWorkbuddyIncremental` | 10803 | 549 | `:2639`、`:2674`、`:5579` |
| 18 | `parseRoocodeIncremental` | 11824 | 200 | `:2091` |
| 19 | `parseClineIncremental` | 12200 | 290 | `:2124` |
| 20 | `parseZedIncremental` | 12650 | 209 | `:2074` |
| 21 | `parseLmstudioIncremental` | 13267 | 215 | `:1879` |
| 22 | `parseUnslothIncremental` | 13691 | 117 | `:1897` |
| 23 | `parseAnythingllmIncremental` | 13938 | 168 | `:1916` |
| 24 | `parseDevinIncremental` | 14207 | 282 | `:1935` |
| 25 | `parseGooseIncremental` | 14604 | 193 | `:2005` |
| 26 | `parseDroidIncremental` | 15137 | 408 | `:2024`、`:6698` |
| 27 | `parseKilocodeIncremental` | 15545 | 142 | `:1960` |
| 28 | `parseOmpLikeIncremental` | 15753 | 334 | （共享实现，被 29 转发） |
| 29 | `parseOmpIncremental` | 16087 | 13 | `:2708` |
| 30 | `parseOmoIncremental` | 16100 | 29 | `:2747` |
| 31 | `parsePiLikeIncremental` | 16319 | 370 | （共享实现，被 32 转发） |
| 32 | `parsePiIncremental` | 16689 | 10 | `:2780` |
| 33 | `parsePrimeAgentIncremental` | 16699 | 28 | `:2809` |
| 34 | `parseMinimaxCodeIncremental` | 16775 | 55 | `:2837` |
| 35 | `parseReasonixIncremental` | 17050 | 30 | `:2890` |
| 36 | `parseCraftIncremental` | 17080 | 229 | `:2865` |
| 37 | `parseCopilotIncremental` | 17878 | 253 | `:3108` |
| 38 | `parseCopilotSessionStoreIncremental` | 18579 | 456 | `:3053`、`:3282` |
| 39 | `parseCopilotAppDbIncremental` | 19381 | 172 | `:3216` |
| 40 | `parseGrokBuildIncremental` | 20084 | 475 | `:2960` |
| 41 | `parseAntigravityIncremental` | 20800 | 597 | `:1315` |
| 42 | `parseTraeIncremental` | 22033 | 148 | `:2200` |
| 43 | `parseTraeCnApiIncremental` | 22598 | 228 | `:2280` |
| 44 | `parseDshIncremental` | 23678 | 396 | `:1819` |
| 45 | `parseCommandCodeIncremental` | 24458 | 475 | `:1855` |

**三点说明**：
- **薄壳与共享实现**。第 28、31 两项是**共享实现**（`OmpLike`、`PiLike`），第 29、32 两项是**转发薄壳**（分别 13 行、10 行），另有 30、33、35 三项同样极短（29、28、30 行）。这与「45 个声明中 5 个转发薄壳、40 个自持 return」的统计相符。**任何按函数名反查解析逻辑的代码都需先过这张转发关系**。
- **`parseDumateIncremental` 不在本表**。它出现在 `src/commands/sync.js:1701`（`sourceAllowed("dumate")` 在 `:1696`），但**定义不在 `src/lib/rollout.js`**——本索引以该文件为范围，故 45 项不含它。
- **`source` 字面量不能由函数名推断**。`sync.js` 侧的 `sourceAllowed(<字面量>)` 实测有 **46** 个不同实参（比 45 个解析器多）：`acode, antigravity, anythingllm, claude, claude-science, cline, codebuddy, codex, command-code, copilot, craft, cursor, devin, droid, dumate, dsh, every-code, gemini, goose, grok, hermes, kilo-cli, kilocode, kimi, kimi-code, kiro, lmstudio, mimo, minimax-code, omo, omp, openclaw, opencode, pi, prime-agent, qoder, qoder-cn, reasonix, roocode, trae, trae-cn, unsloth, workbuddy, workbuddy-ai, zcode, zed`。差集来自**多解析器共享一个 source**（`copilot` 三通道 = 三个解析器、`kiro` 两套 = 两个解析器）与**使用通用 DB 解析器但独立 source**（`dumate`/`mimo`/`zcode`/`kilo-cli`）。因此「按 source 反查解析器」必须查 `sync.js` 的调用点，**不能反推**。

自检结果（UTF-8 口径）：`parseRolloutIncremental` = **251** ✅、`parseClaudeIncremental` = **954** ✅、`parseCommandCodeIncremental` = **24458**（文件总行数 24932）。作为对照，同一文件用 GBK 口径读取时总行数为 **24917**（少 15 行），同一函数落在 **24443**——即任何给出 24443 的引用都应被视为 GBK 口径产物，不可采信。

## 附录 B：术语表

| 术语 | 定义 | 出处 |
| --- | --- | --- |
| **队列（queue）** | `~/.tokentracker/tracker/queue.jsonl`，append-only 的**累计快照日志**：每 `(source, model, 半小时)` 一行，不是事件流。同键后写覆盖前写（last-wins） | `src/commands/sync.js:569`；`src/lib/rollout.js:3482-3484` |
| **项目队列** | `project.queue.jsonl`，项目维度的本地状态，只有本地视图读它；**不上传**，因此其积压不计入 pending | `src/commands/sync.js:571`、`:3654-3659` |
| **桶（bucket）** | 一个 `(source, model, hour_start)` 组合的累计计数器，键形如 `source\|model\|hourStart`；`hour_start` 恒为 UTC 半小时 | `src/lib/rollout.js:4221-4245` |
| **项目桶** | `projectKey\|source\|hourStart`，无 model 维度；字段布局与主桶**不通用** | `src/lib/rollout.js:4228` vs `:4238` |
| **游标（cursor）** | 记录「每个文件读到第几字节」及各类去重集合的持久状态；v2 形态下按代际 + 分片存储 | `src/lib/cursor-store.js:13-23`；`src/lib/rollout.js:1194` |
| **代际存储（generation store）** | `cursor-store-v2/generations/gen-<ms>-<uuid8>/`，每个代目录自洽；`manifest.json` 是唯一的原子切换点，保留 `current/rollback/previous` 三代 | `src/lib/cursor-store.js:855-1016` |
| **retraction（撤回）** | 当 `unknown` 桶被对齐到真实模型时，先追加一条**全零行**抵消旧归属，避免重复计入；零行必须被解释为撤回而不是「零用量」 | `src/lib/rollout.js:3251-3317`、`:3351-3402` |
| **证件行 / billable** | `billable_total_tokens` 字段：用于对外计费的 token 总量，缺省回落 `total_tokens` | `src/lib/rollout.js:3332-3345`（`?? total_tokens`） |
| **`usage_precision`** | 队列行上的可选精度标记；参与 `queuedKey` 的相等判定，但不进项目行（项目行被裁剪掉该列）。全部出现点 16 行（其中 `:20286-20287` 是一条跨两行赋值，按语句计 **15** 条），**无 antigravity 写入点** | `src/lib/rollout.js:3325`、`:3341`、`:3403`、`:3422`、`:6542`、`:6648`、`:6725`、`:6731`、`:6755`、`:6760`、`:20286-20287`、`:22011`、`:22025`、`:22113`、`:22157` |
| **priority 子集** | `...prioritySubsetOf(totals)` 展开出的 `priority_*` 列（值 > 0 才出现），记录 Fast/priority 档位贡献的 token；字段表 `src/lib/rollout.js:4073-4079`，主行展开 `:3342`，`totalsKey` 内判定 `:4167-4170` | 同上 |
| **权威成本（authoritative cost）** | 来源在 `SOURCES_WITH_AUTHORITATIVE_COST = {grok, cline}` 且行上 `total_cost_usd > 0` 时**直接采信该值**；`0` 视为「未上报」哨兵，继续走价格表 | `src/lib/pricing/index.js:32`、`:236-241` |
| **last-wins** | 读侧同键覆盖语义：解析时 `target.set(entry.key, entry.row)`，末行胜出。用于展示与一般聚合 | `src/lib/local-api.js:202-211`；`src/lib/async-queue-reader.js:51-57,86` |
| **setMaxTotal** | 与 last-wins 并存的第二套语义（取 MAX），**仅**用于 codex rescan/fork 修复的数据丢失守卫——撤回零行会把 last-wins 的末行压成 0，只有 MAX 能保住历史峰值 | `src/commands/sync.js:6574-6589`，唯一调用族 `:5848-5879` |
| **血缘修复（lineage repair）** | 针对 fork/复制产生的重复计数的一次性修复路径：收集 fork 别名（`collectClaudeForkAliases`）与家族时间，重算归属 | `src/lib/rollout.js:987-999`、`:1189-1234`；OpenCode 版本 `:3729-3748`、`:3959` |
| **claim（用量认领）** | Copilot 三通道共享 `source = "copilot"` 时的互斥机制：OTEL CLI span 结算后写 `recentUsageEvents`，供 session-store 通道避免重复计数；保留 7 天、上限 10000 条 | `src/lib/rollout.js:17375-17405`、`:18058-18064` |
| **evidence（来源证据）** | 对「这条用量从哪个字段读出来的」的标记体系，包括 `usageSource`、估算标记与读时修补判据；**否定性证据**（如「清单里没有 antigravity」）是这类结论的常见形态 | `src/lib/rollout.js:6726`（悬空注释）；`src/lib/local-api.js:155-200` |
| **deferred（推迟）** | 守卫命中时的第三态：既不执行也不跳过，而是**不写迁移键**、留待下次全量重试。`--auto` 轮次下该提示是静默的 | `src/commands/sync.js:952-959`、`:1147-1192`、`:1177-1181` |
| **union 安装模式** | 同一 provider 的 native 与 WSL 两份安装合并扫描；**仅在解析器具备路径无关身份时启用**（Codex/acode 的事件键、Claude 的文件哈希 + 消息哈希） | `src/lib/install-resolver.js:6-18` |
| **扫根三态** | 目录探测的三种结果：存在 / 不存在 / 存在但不可读（带 `error` code）。第三种必须显示为 `unreadable: <code>` 并触发「推迟」而非「跳过」 | `src/lib/scan-roots.js:89-110`、`:248-253` |
| **账户级来源** | 被标为 account 级、可被 personal/local 过滤排除的来源：`cursor`、`trae-cn` | `src/lib/source-metadata.js:1`、`:7-13` |
| **半小时桶** | `toUtcHalfHourStart(ts)`：把时间戳向下取整到 UTC 半点（`:00` 或 `:30`）。这是队列行的时间粒度，与「日归属」是两件事 | `src/lib/rollout.js:4173-4190`；副本 `src/lib/claude-categorizer.js:1175-1191` |
| **fork 重放突发** | 被 fork 的 Codex/Acode rollout 会把父会话全部历史一次性写入子文件；判据是「新文件 + 行间隔 < 500 ms」的突发，跨日另有日期守卫兜底 | `src/lib/rollout.js:2330-2340`、`:2556-2601`、`:4203-4214` |
| **内容等值短路** | 写配置前的优化：内容与目标全等则直接返回 `{changed:false}`，不写盘。与「哨兵串」是两种不同机制，不可混用 | `src/lib/opencode-config.js:73-75` |
| **哨兵串** | 文件内的自有标记（如 `// @tokentracker-managed-omp-extension`），带标记才覆盖、不带则跳过并回报 `unmanaged-extension-present`。本项目**只对 omp** 使用 | `src/lib/omp-hook.js:17,388-395`；`src/commands/init.js:813` |
| **负缓存** | 按 `source\0model` 记忆未命中查询的 `Set`，避免每次全表遍历；价格表 `revision` 变化时清空 | `src/lib/pricing/index.js:64-66,120-131` |
| **候选式计价** | 与「单表覆盖」相对的定价模型：多渠道条目作为独立候选，完整候选优先，再取**最高费用**；来源等级仅在同价时决胜 | 见 §10.3；匹配顺序 0→7 在 `src/lib/pricing/matcher.js:3-12`、`:252-386` |
