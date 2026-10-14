# codeburn 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`getagentseal/codeburn`](https://github.com/getagentseal/codeburn)，
> HEAD `9a40cc6`（`package.json` 版本 0.9.27）。浅克隆到临时目录后逐文件精读，
> **未运行其二进制、未改动本机任何 agent 数据**。
>
> 证据分档：**已验证**（源码可证，附 `文件:行号`）／**一致解释**（多条代码证据指向同一结论，
> 无单点直证）／**未验证推论**（结构推断，未实测）。
>
> 本文只记录字段名、路径规则、表名与计算口径，不含任何真实会话内容。
> 行号以 HEAD `9a40cc6` 为准；`src/parser.ts` 共 6785 行（`read` 工具计数）。

## 1. 范围与方法

- 代码规模（`src/`，未含 `dash/`、`app/`、`mac/`、`windows/`、`gnome/` 各平台外壳；大小为十进制 KB）：
  `parser.ts` 338 KB、`session-cache.ts` 160 KB、`daily-cache.ts` 116 KB、
  `usage-aggregator.ts` 104 KB、`models.ts` 101 KB、`optimize.ts` 199 KB、`main.ts` 167 KB；
  `src/providers/` 49 个文件合计约 1.0 MB；价格数据 `src/data/litellm-snapshot.json` 487 KB、
  `src/data/pricing-fallback.json` 40 KB。
- 分析手段：`rg` 定位符号 → 分段精读 → 交叉核对调用点、类型定义与测试；关键断言附 `文件:行号`。
- 版本敏感性：codeburn 迭代很快，解析版本号（`PROVIDER_PARSE_VERSIONS`、`CACHE_SCHEMA_VERSION`）
  与价格表会随版本变动；本文结论只对上述 HEAD 负责。

## 2. 总体架构

### 2.1 一个 CLI + 三面平台外壳

macOS 菜单栏（Swift，`mac/`）、Windows 托盘（Tauri 2 + React，`windows/`）、GNOME 扩展
（纯 JS，`gnome/`）都只是壳：它们调用 `codeburn status --format menubar-json --period <p>`
并解析 JSON，与 CLI 不共享代码，只依赖输出契约（`docs/architecture.md:5-27`）。

### 2.2 核心管线

```
provider.discoverSessions()                        # 发现层：决定读哪些文件
        │  → SessionSource[]
        ▼
provider.createSessionParser(source, seenKeys)      # 解析层：逐源产出统一调用
        │  yields ParsedProviderCall（1 条 = 1 次 API 调用）
        ▼
src/parser.ts  parseAllSessions()                   # 汇总层：call → turn → session → project
        │  → ProjectSummary[]
        ▼
src/day-aggregator.ts / src/daily-cache.ts          # 聚合层：按日封存（含历史保留）
        │  → DailyEntry[]
        ▼
渲染：Ink TUI / JSON / menubar-json / CSV export
```

依据：`docs/architecture.md:48-68`、`src/providers/types.ts:28-30`、`src/parser.ts:6299-6307`。

### 2.3 两级 provider 注册：eager 与 lazy

`src/providers/index.ts:200` 的 `coreProviders` 在模块加载期导入 32 个 provider；
`:204` 的 `lazyProviderNames` 列出 11 个按需 `import()` 的 provider（各自带来 SQLite、
protobuf、网络客户端等重依赖）。`getAllProviders()`（`:241-258`）合并两级，懒加载失败静默跳过，
但仍留在 `allProviderNames()`（`:233-239`）里参与 `--provider` 参数校验。
README 的对外口径是「42 个工具 + Vercel AI Gateway 单独一行」（`README.md:201-213`），
合计 43 个 provider 实现。

### 2.4 发现层的失败隔离

每个 provider 的 `discoverSessions()` 都包在 `discoverOne`（`src/providers/index.ts:268-286`）里：
抛错只跳过该 provider（记入 `provider-issues` 并在 stderr 警告一次，`isBlockedDatabaseError` 例外），
`discoverAllSessionsWithFailures`（`:298-316`）并发跑全部 provider、按注册表顺序拼接结果，
并回传 `failedProviders`，让上层能区分「这个 provider 报错」与「这个 provider 没有会话」
（注释明确：把前者当后者会以为自己看过了从未看过的语料）。

### 2.5 并行冷解析（`parse-workers.ts`）

冷解析的耗时几乎全在「按文件、纯计算」的阶段（读一个 Claude 会话 JSONL、解码一个 Codex
rollout、逐行转成中间记录）。`src/parse-workers.ts` 在待处理量大到值得时把这一层挪到
`worker_threads`：每个 worker 用**空去重集**跑与串行路径同一个单文件函数
（Claude 是 parseClaudeFileFull、Codex 是 parseCodexFileFull），把结果连同它声明占用的
全部去重键以 JSON 字符串传回；主线程按串行顺序安装，凡是跨文件状态（去重集、规范项目路径、
subagent spawn 链接、PR 关联、Codex 结果缓存）都留在主线程。**键已被先前文件占用的文件、
或 worker 失败的文件，会在进程内重解析**，因此输出与串行路径逐一相同——这也正是被 fork 的
Codex rollout 的安全网：它按父命名空间重放父文件的 `token_count` 历史，会与父文件撞键，
于是被拉回主线程按真实去重集重解析（`docs/architecture.md:72-88`）。

线程池默认对「非大型冷解析」关闭（`src/parse-workers.ts:46-86`，`docs/architecture.md:99-131`
给出同一套门槛）：待解析字节 `< MIN_PENDING_BYTES`（200 MB）、`cores <= 2`、
可用内存 `< 4 GB` 时走串行；否则

```
memoryBudget = min(0.25 × availableBytes, MEMORY_BUDGET_CAP_BYTES)   // 上限 2 GB
perWorker    = min(1 GB, max(256 MB, 2 × (bytes / max(1, files)) + 128 MB))
workers      = min(cores − 1,
                   floor(memoryBudget / perWorker),
                   max(floor(files / 50), floor(bytes / 200 MB)))
```

（`:69-84`；`max(1, files)` 防止零文件时除零。）环境变量 `CODEBURN_PARSE_WORKERS=0|N`
可跳过全部门槛强制覆盖，但 `N` 仍被 `Math.min(floor(N), cores)` 封顶（`:55-61`）；
非法或负数取值直接判 0（= 串行，`src/parse-workers.ts:58`）。
只有「整文件解析」下放线程；增量/追加路径（Claude append、Codex 字节偏移续读）留在主进程。

## 3. 读取层：43 个应用分别从哪里读

### 3.1 四条基础事实

1. **注册表是 43 个 Provider 对象，不是 42**：`coreProviders` 32 个（eager，`src/providers/index.ts:200`）
   + 懒加载 11 个（`:204`）＝ 43。README 的「42 tools」＝ 43 − `vercel-gateway`
   （README 自己在 `README.md:213` 把 Vercel AI Gateway 单列为「gateway 而非 tool」）；
   `docs/architecture.md:196-197` 的两份清单与源码逐字一致。
2. **43 个里只有 1 个是纯联网拉取**：`vercel-gateway`（`network?: true` 全仓唯一，
   `src/providers/types.ts:143-145`、`src/providers/vercel-gateway.ts:136`）。
   其余 42 个全部读本地文件 / 本地 SQLite / 本地 protobuf。
3. **「一次会话」的身份不统一，共四类形态**：
   ① **文件路径**为主键（多数 provider）；② **目录**为主键（`claude` 的 `projects/<slug>`、
   `codebuff` 的 chat 目录、`cline` 的 task 目录、`mistral-vibe` 的 session 目录）；
   ③ **SQLite 虚路径**（`<dbPath>:<sessionId>`、`<dbPath>#hermes-session=<id>`、
   `<dbPath>#cursor-ws=<folder>`）；④ **整库/整文件一条 source**
   （`zed` 的 `threads.db`、`copilot` 的 OTel 库、`vercel-gateway` 的合成路径）。
4. **发现失败被隔离**：`discoverOne` 把每个 provider 的 `discoverSessions()` 包在 try/catch，
   失败返回空数组、记入 `failedProviders` 并写 `provider-issues`
   （`src/providers/index.ts:268-316`；`src/provider-issues.ts:6-23`，只存
   `{provider, stage, kind}` 枚举、**刻意不存错误消息**，因为消息可能带路径）。

### 3.2 读取基础设施

| 项 | 值 | 位置 |
| --- | --- | --- |
| 一次性读硬上限 | `MAX_SESSION_FILE_BYTES = 128 MiB`（`readSessionFile`/`readSessionFileSync` 的主守卫；`dsh` 也复用该常量作 zstd 解码预算与尺寸守卫，`:264`、`:293-296`。超限默认静默，见下行） | `src/fs-utils.ts:7` |
| 大行阈值 | `LARGE_STREAM_LINE_BYTES = 32 KiB`（超限以 Buffer 交付，绕开 V8 字符串） | `src/fs-utils.ts:8` |
| 流式读上限 | `MAX_STREAM_SESSION_FILE_BYTES = 4 GiB`（多 GB Codex 会话是常态） | `src/fs-utils.ts:16` |
| 元数据并发度 | `FS_SCAN_CONCURRENCY = 32`，`mapWithConcurrency` 保序 | `src/fs-utils.ts:233-250` |
| 一次性读超限（> 128 MiB） | 整体跳过 + `warn()` 提示，**受 `CODEBURN_VERBOSE=1` 门控**（默认静默） | `src/fs-utils.ts:22-24`、`:44-47`、`:66-69` |
| 流式读超限（> 4 GiB） | 整体跳过 + `notice()`，**始终打印**（明确写出「its usage is NOT counted」） | `src/fs-utils.ts:28-30`、`:112-117` |
| 截断策略 | **不做截断**：超限即整份跳过（两种上限都返回空） | 同上 |

### 3.3 增量与指纹（发现层的持久化）

| 机制 | 语义 |
| --- | --- |
| `fingerprintFile` | 普通文件 `{dev, ino, mtimeMs, sizeBytes}`；SQLite 取 `max(db, db-wal)` 的 mtime 与大小之和并**故意忽略 `-shm`**（读也会改它）；WSL UNC 路径抹掉 dev/ino；`CURRENT`（Unified Vibe）为复合指纹；`stat` 失败时按 `#`、`:` 剥虚后缀再 stat（`src/session-cache.ts:2568-2619`、`:2548-2561`） |
| `reconcileFile` 四态 | 无缓存→`new`；四项全等→`unchanged`；同 dev/ino 且 size 增长且 `lastCompleteLineOffset ≤ size`→`appended(readFromOffset)`；其它→`modified`（`:2654-2685`） |
| 环境指纹 | `computeEnvFingerprint = sha256(provider 声明的环境变量 + parser=<PROVIDER_PARSE_VERSIONS>)[:16]`；33 个 provider 各有 env 白名单（`:1034-1040`、`:300-342`） |
| 解析器版本 | `PROVIDER_PARSE_VERSIONS`（`:361+`）——解析语义一改就强制重解析 |
| 永久保留 | `DURABLE_PROVIDER_NAMES = {copilot}`（`:346`）：条目永不逐出，源文件消失也保留 |
| 全量加载名单 | `FULL_LOAD_PROVIDER_NAMES = {hermes, quickdesk, antigravity, devin, gemini}`（`:353`），范围加载时也全量读 |

### 3.4 43 个 provider 的读取矩阵

图例：**格式** JSONL/JSON/SQLite/PB(protobuf)/SQLite-v 表示 SQLite 虚路径；**身份**列给会话主键形态。

| # | provider（id） | 数据面（读什么） | 格式 | 会话身份 | token 来源 |
| --- | --- | --- | --- | --- | --- |
| 1 | `claude` | `<cfgDir>/projects/<slug>/`（**目录**）；Claude Desktop `local-agent-mode-sessions/**/projects/<slug>`；`usage-ledger/*.ndjson` | JSONL / 目录 | 目录 | 真实（Claude Code JSONL，解析在 `parser.ts`） |
| 2 | `codex` | `CODEX_HOME\|~/.codex` 下 `sessions/YYYY/MM/DD/rollout-*.jsonl` + `archived_sessions/`；launcher nest；WSL home | JSONL | 文件（身份取首行 `session_id`） | 真实（`token_count`/`token_usage_record`） |
| 3 | `copilot` | 6 类源：OTel `agent-traces.db`、`~/.copilot/session-store.db`、`session-state/<sid>/events.jsonl`、VS Code `workspaceStorage/*/chatSessions/*.json(l)`、`GitHub.copilot-chat/transcripts/*.jsonl`、JetBrains `copilot-*-nitrite.db` | SQLite / JSONL / Nitrite | 整库或文件 | 混合（OTel 与 store 真实；JSONL 多为估算） |
| 4 | `cursor` | `<EditorDataDir>/Cursor/User/globalStorage/state.vscdb` + 同层 `workspaceStorage/<hash>/` | SQLite | 虚路径 `#cursor-ws=` | 混合（input 来自上下文计量，output 估算） |
| 5 | `cursor-agent` | `~/.cursor`（Windows 路径源码自认未验证）下 `projects/<id>/`、`chats/`、`ai-tracking/ai-code-tracking.db` | `.txt` / `.jsonl` / SQLite | 文件 | 估算（`chars/4`，显示名带 `(est.)`） |
| 6 | `cline` | VS Code globalStorage ×3 变体 + `~/.cline/data` 的 `tasks/<taskId>/` | JSON | 目录 | 真实（`api_req_started` 内 `tokensIn/Out/cacheReads/Writes`） |
| 7 | `cline-cli` | `CLINE_SESSION_DATA_DIR`（或 `CLINE_DATA_DIR`/`CLINE_DIR`/`~/.cline`）下 `sessions/<sid>/` | JSON | 文件 `<sid>.messages.json` | 真实（每消息 `metrics`；input 已含 cache 需相减） |
| 8 | `kilo-code` | VS Code `globalStorage/kilocode.kilo-code/tasks/*` + `~/.local/share/kilo/kilo*.db` | JSON + SQLite | 目录 / 虚路径 | 真实 |
| 9 | `ibm-bob` | `IBM Bob`、`Bob-IDE` 两目录的 `ibm.bob-code/tasks/<taskId>/` | JSON（Cline 同构） | 目录 | 真实（复用 Cline 解析器） |
| 10 | `gemini` | `~/.gemini/tmp/<project>/chats/session-*.json`（或 `.jsonl`） | JSON / JSONL | 文件 | 真实（input/output/cached/thoughts） |
| 11 | `dsh` | `DSH_HOME\|~/.dsh` 下 `sessions/<enc-cwd>/<sid>/session[.vN].jsonl[.zstd]` | JSONL（可 zstd 帧拼接） | 文件（选最高代次） | 真实（step 的 `assistant/message`） |
| 12 | `zcode` | `~/.zcode/cli/db/db.sqlite`（表 `model_usage` + `session`） | SQLite | 虚路径 | 真实（cache 已含 input 需扣减） |
| 13 | `opencode` | `$XDG_DATA_HOME\|~/.local/share/opencode` 下 `opencode*.db` + 旧 `storage/` 文件树 | SQLite + JSON | 虚路径 / 文件 | 真实（`tokens.*` 或 `usage.*`） |
| 14 | `openclaw` | 4 个家目录（`.openclaw`/`.clawdbot`/`.moltbot`/`.moldbot`）的 `agents/<agent>/`：`sessions.json` 索引 + `*.jsonl` + `agent/openclaw-agent.sqlite` | JSONL + SQLite | 文件 / 虚路径 | 真实（assistant `usage` 块；SQLite 优先于 legacy JSONL） |
| 15 | `openclaude` | `CODEBURN_OPENCLAUDE_DIR\|~/.openclaude` 下 `projects/<slug>/<uuid>.jsonl` | JSONL（Claude schema） | 文件 | 真实 |
| 16 | `kimi` | `KIMI_SHARE_DIR\|~/.kimi` 下 `sessions/<workdir-hash>/<sid>/wire.jsonl` + `subagents/` | JSONL | 文件 | 真实（`StatusUpdate.token_usage`） |
| 17 | `kimicode` | `KIMI_CODE_HOME\|~/.kimi-code`（+ macOS 桌面版 home）下 `sessions/wd_*/<sid>/agents/<agent>/wire.jsonl` | JSONL | 文件（按 agent） | 真实 |
| 18 | `hermes` | `HERMES_HOME\|%LOCALAPPDATA%\hermes\|~/.hermes` 下 `state.db` + `profiles/*/state.db` | SQLite | 虚路径 `#hermes-session=` | 真实（列直读） |
| 19 | `mistral-vibe` | `VIBE_HOME\|~/.vibe` 下 `logs/session/<dir>/`（`meta.json`+`messages.jsonl`）+ `agents/*` + WSL home + Unified `CURRENT` | JSON + JSONL | 目录 | 真实（meta 累计值） |
| 20 | `mux` | `MUX_ROOT\|~/.mux` 下 `sessions/<workspaceId>/chat.jsonl` + `subagent-transcripts/` | JSONL | 文件（子 agent 单独成源） | 真实 |
| 21 | `open-design` | 平台数据目录下 `namespaces/<ns>/data/runs/<runDir>/events.jsonl` | JSONL | 文件 | 真实 |
| 22 | `pi` | `~/.pi/agent/sessions/<project>/*.jsonl` | JSONL | 文件 | 真实 |
| 23 | `omp` | `~/.omp/agent/sessions/<project>/**/*.jsonl`（多一层子目录） | JSONL | 文件（带 `agentName`） | 真实 |
| 24 | `qwen` | `QWEN_DATA_DIR\|~/.qwen/projects` 下 `<proj>/chats/*.jsonl` | JSONL | 文件 | 真实 |
| 25 | `quickdesk` | `QUICKWORK_HOME\|~/.quickwork`（按 `profiles.json` 的 data_path）每 profile：`metrics-*.jsonl` + `sessions/sessions.db` | JSONL + SQLite | 按 `sourceId` 判别 | 真实 |
| 26 | `zerostack` | `ZS_DATA_DIR/sessions` 或平台数据目录下 `zerostack/sessions/*.json` | JSON | 文件 | 真实但**整会话累计**（无逐调用拆分） |
| 27 | `grok` | `GROK_HOME\|~/.grok` 下 `sessions/<cwdDir>/<sid>/`（`summary.json`+`updates.jsonl`）+ `logs/unified.jsonl` | JSON + JSONL | 文件 | 真实 |
| 28 | `grokbot` | 客户端持久化目录（`%APPDATA%\Grok Bot\...` 等）的 `*.blob`（base32 命名 slice）+ `cursor-import` 导入文件 | blob（base32） | 文件 | **估算**（`chars/4`，模型恒为 `grokbot-auto`，`costIsEstimated: true`） |
| 29 | `amp` | `AMP_DATA_DIR`（逗号分隔多根）或 `~/.local/share/amp` 下 `threads/**/*.json`；project 恒为 `Amp` | JSON | 文件 | 真实 token（credits 不换算） |
| 30 | `command-code` | `CODEBURN_COMMANDCODE_DIR\|~/.commandcode` 下 `projects/<proj>/<id>.jsonl`（明确排除 `.checkpoints.jsonl`） | JSONL | 文件 | 真实（`usage.*`） |
| 31 | `codebuff` | `CODEBUFF_DATA_DIR` 或 `~/.config/{manicode,manicode-dev,manicode-staging}/projects/<proj>/chats/<chatId>/` | JSON | 目录 | 混合（有 stash token 则真实，否则 credits × $0.01） |
| 32 | `codewhale` | `CODEWHALE_HOME/sessions`（精确覆盖）或 `~/.codewhale/sessions` + 遗留 `~/.deepseek/sessions` | JSON | 文件（id 取 `metadata.id`） | 真实但只有 `total_tokens` 聚合 |
| 33 | `kiro` | 4 类源：`kiro.kiroagent/<hash>/.chat`、`workspaceStorage/<hash>`、CLI `~/.kiro/sessions/cli/*.jsonl`、v2 `sessions/<hash>/sess_*/` | JSON + JSONL + 目录 | 文件 / 目录 | 估算（模型不暴露 → `kiro-auto`） |
| 34 | `devin` | `~/.local/share/devin/cli/sessions.db`（`DEFAULT_DEVIN_CLI_DIR`，`devin.ts:159-165`）+ `transcripts/*.json` | SQLite 优先，JSON 兜底 | 虚路径 `<dbPath>:<id>`（`devin.ts:754`） | 真实（`prompt/completion/cached_tokens`） |
| 35 | `droid` | `FACTORY_DIR\|~/.factory` 下 `sessions/<subDir>/*.jsonl`（首行必须 `session_start`） | JSONL | 文件 | 真实 |
| 36 | `lingtai-tui` | `~/.lingtai/<agent>/logs/token_ledger.jsonl` + `~/.lingtai-tui/registry.jsonl` 注册的项目 | JSONL | 文件 | 真实（`input − cached` 归一） |
| 37 | `antigravity` | `~/.gemini/{antigravity,antigravity-cli,antigravity-ide}/conversations`（`.pb`/`.db`）+ `implicit` + statusline 合成源；另有**可选**的本地语言服务器 RPC（仅在进程枚举发现 `language_server` 时启用，失败回退 statusline/缓存） | protobuf / JSONL | 文件 | 混合（statusline 带真实 `current_usage`） |
| 38 | `forge` | `~/.forge/.forge.db`（表 `conversations`） | SQLite | 虚路径 `<dbPath>:<conversation_id>`（`forge.ts:249`） | 真实（assistant usage） |
| 39 | `goose` | `GOOSE_PATH_ROOT/data/sessions/sessions.db` 或平台数据目录 | SQLite | 虚路径 `<dbPath>:<id>`（`goose.ts:252`） | 真实 |
| 40 | `crush` | `CRUSH_GLOBAL_DATA/projects.json` 注册表 → 每项目 SQLite | JSON + SQLite | 虚路径 `<dbPath>:<id>`（`crush.ts:217`） | 真实 |
| 41 | `warp` | `WARP_DB_PATH` 或 macOS Group Containers 下 `warp.sqlite`（stable/preview 两个 bundleId） | SQLite | 虚路径 `<dbPath>:<conversation_id>`（`warp.ts:552`，每会话一条源） | 混合（工具自报金额阶梯，份额按字符加权） |
| 42 | `zed` | 平台数据目录 `Zed/threads/threads.db`（`data` 是 zstd 压缩 JSON） | SQLite | 整库一条源 | 真实（blob 内 per-request usage） |
| 43 | `vercel-gateway` | **无本地文件**：`https://ai-gateway.vercel.sh/v1/report`（Bearer 认证） | REST（按天按模型聚合） | 合成路径 | 真实但无请求身份；`request_count` 承载请求数 |

（编号只为计数，不代表重要性。证据逐项见各 `src/providers/*.ts` 的 `discoverSessions` 实现。）

### 3.5 平台差异与 WSL

- 三套平台路径（darwin / win32 / linux）分别处理：`claude`（含 Windows MSIX 的
  `Packages/Claude_*`）、`cline`/`ibm-bob`/`kilo-code`（VS Code、Insiders、VSCodium 三变体）、
  `kiro`、`zed`、`goose`、`crush`、`grokbot`、`open-design`、`zerostack`。
- WSL（Windows 专属，`src/wsl.ts`）：`wslHomes()` 跑
  `%SystemRoot%\System32\wsl.exe --list --quiet --running`（3 s 超时、绝对路径防劫持），
  解码 UTF-16LE，过滤容器发行版与非法名字，枚举 `\\wsl$\<distro>\home\*` 与 `\root`；
  结果 60 s TTL（`:127-150`）。`CODEBURN_WSL=off|all|running` 控制；读取**仅运行中的发行版**
  是有意为之（触碰 `\\wsl$` 会启动已停止的发行版）。
- 吃 WSL home 的 provider 只有三个：`claude`（无条件附加，`claude.ts:96-97`）、
  `codex` 与 `mistral-vibe`（**仅在调用方未显式指定 home 时才附加**，
  `codex.ts:1595-1596`、`mistral-vibe.ts:410-411`）。
- 关键注释：WSL UNC 路径的 dev/ino 每次挂载都重新合成，因此指纹**抹掉 dev/ino**，
  只用 mtime+size（`src/session-cache.ts:2597-2600`）。

### 3.6 读取层的文档不符点（以源码为准）

| 文档说法 | 源码事实 |
| --- | --- |
| `docs/architecture.md:184-192` 的 `Provider` 类型片段只有 6 个成员 | 真实类型还有 `network?` / `durableSources?` / `probeRoots?`（`src/providers/types.ts:140-165`） |
| `docs/architecture.md:135` 说「三个缓存」 | 实际还有 `session-cache.v10/` 分片目录、`antigravity-results.v9.json`、`antigravity-statusline.jsonl`、`imports/cursor-usage.v1.json` |
| `docs/how-it-works.md:47` Kiro 只写「`.chat` JSON files」 | 实际 4 类源（`.chat` + workspaceStorage + CLI JSONL + v2 目录） |
| `docs/how-it-works.md:49` OpenClaw 只写 `~/.openclaw/agents/*.jsonl`（已含三个遗留家目录） | 未述 `sessions/sessions.json` 索引与每 agent 的 `agent/openclaw-agent.sqlite`（`openclaw.ts:405`、`:462-506`） |
| `docs/how-it-works.md:39-40` Claude 只列 `~/.claude/projects` | 实际还有 `config.json` 的 `claudeConfigDirs`、WSL homes、Claude Desktop `local-agent-mode-sessions` 与 `usage-ledger/*.ndjson`；且 transcript 类 source 的 `path` 是**项目目录**而非 `.jsonl`（ledger 类 source 则是**文件**，`claude.ts:599-608`） |
| `README.md:201`「All 42 tools」 | 与 `getAllProviders()` 的 43 个不一致（差 `vercel-gateway`，README 自己单列） |

### 3.7 读取层未决点（专项复核后收敛）

1. **【已验证：传播链已查清】** `failedProviders` 全仓唯一消费点是 `src/parser.ts:6687`
   （`discoveryFailed`）：`:6692` 据此跳过 `markProviderComplete`、`:6696` 抑制
   `diskCache.complete`，后果限于「冷/热判定与常驻 `serve` 的首屏 floor」
   （`session-cache.ts:1064/1109`、`serve.ts:795/887`）；`failedProviders` **本身**不进
   payload、退出码或 `codeburn doctor`（doctor 走自己的发现，`doctor.ts:282`）。
   用户可见提示走平行通道：
   `recordProviderIssue`（`index.ts:274`）→ `provider-issues.ts:26` → `main.ts:1418-1423`
   → `bridge-handlers.ts:519`（遥测）。
   附注：`parser.ts:6732` 的 `sessionHydrationComplete` 不含 `discoveryFailed`，
   发现失败的 provider 会出现「零用量但 hydration=complete」的组合。
2. **【已验证】`crush` / `forge` / `goose` 的 DB 发现细节**（三家都是每会话一条源，
   `path = <dbPath>:<id>`，右切以兼容盘符）：
   `crush` 过滤 `WHERE parent_session_id IS NULL AND (cost > 0 OR prompt_tokens > 0
   OR completion_tokens > 0)`，project 取注册表路径 sanitize（`crush.ts:210-217`、`:251`）；
   `forge` 过滤 `WHERE context IS NOT NULL`，project = `title ?? workspace_id`（`forge.ts:243-250`）；
   `goose` 全表读后内存过滤（累计 token > 0），project = `sanitize(working_dir) || 'goose'`
   （`goose.ts:245-253`）。
3. **【已验证（注释↔实现一致）】** `amp` 的 v2 口径与注释一致：
   `billedOutput = output + max(0, total − input − output − 两 cache 桶)`，cache 桶经
   `toMessageId` 回连（`amp.ts:162-163`、`:127-139`），`usageLedger.events` 优先。
   **v1→v2 的具体新增项无法证实**（克隆为 `--depth 1`，无历史可比），此处如实标为未证。

## 4. 解析层：原始日志 → 统一用量事件

### 4.1 三层数据结构

codeburn 的「统一用量事件」在不同层有不同形状，但 token 语义一致：

| 层 | 类型 | 载体 | 特征 |
| --- | --- | --- | --- |
| provider 层 | `ParsedProviderCall` | `src/providers/types.ts:32-129` | 扁平：7 个 token 字段直接铺开（input/output/cacheCreation/cacheRead/cachedInput/reasoning/webSearch）+ `costUSD`/`costIsEstimated`/`costFromBilling`/`fallbackCostUSD`/`tools`/`bashCommands`/`skills`/`subagentTypes`/`timestamp`/`speed`/`deduplicationKey`/`turnId`/`sessionId`/`project`/`route`/`billing` |
| 解析层 | `ParsedApiCall` | `src/types.ts:111-187` | 改为桶 `usage: TokenUsage`（`src/types.ts:3-11`）；增加 `mcpTools`/`spawnToolUseIds`/`cwd`/`isEstimated`/`savingsUSD`/`hasAgentSpawn`/`hasPlanMode`/`interrupted`/`userModified`/`toolErrors` 等派生字段 |
| 缓存层 | `CachedCall` / `CachedUsage` | `src/session-cache.ts:15-95` | 多一列 `cacheCreationOneHourTokens`（1h 写档拆分）；`costUSD`/`fallbackCostUSD`/`isEstimated` 随缓存落盘（`costFromBilling` 的调用据此把成本带过缓存），但**读路径仍会重算** `calculateCost`，落盘的 `costUSD` 只是优先级最高的取值分支（`src/parser.ts:2984-3002`） |

两级归属：**call（1 条 = 1 次 API 调用）→ turn**。provider 侧没有 turn 概念，只用 `turnId`
（Codex 形如 `<sessionId>:t<counter>`，`src/providers/codex.ts:1324`；ZCode 用
`model_usage.turn_id`），`providerCallsToCachedTurns` 按 `sessionId\0turnId` 归并
（`src/parser.ts:2945-2975`），无 `turnId` 的调用各自成 turn。Claude 侧走独立路径：
`ParsedTurn { userMessage, assistantCalls[], timestamp, sessionId, gitBranch?, prRefs?, spawnToolUseIds? }`
（`src/types.ts:90-109`）→ `ClassifiedTurn`（加 `category`/`retries`/`hasEdits`，`src/types.ts:210-219`）。

### 4.2 解析主流程

| 步骤 | 内容 | 证据 |
| --- | --- | --- |
| 1 发现 | `discoverSessions()` → `SessionSource[]`；provider 之间互相隔离 | `src/providers/index.ts:268-316` |
| 2 指纹对账 | `fingerprintFile` → `reconcileFile` 得 `unchanged / appended / changed`；append 走增量字节快路径 | `src/parser.ts:2183-2220` |
| 3 去重集 | 调用方持有的跨 provider `seenKeys` + 本 pass 的 `parserDedup`，并预置所有未变更文件/stub 的去重键 | `src/parser.ts:3782-3792` |
| 4 解析 | `createSessionParser(source, parserDedup, dateRange).parse()` 异步生成器逐条 yield | `src/parser.ts:3889-3900` |
| 5 项目规范化 | `canonicalizeProviderCallProject`：linked worktree → 主仓库路径 | `src/parser.ts:2854-2870`、`232-275` |
| 6 装配缓存 | `providerCallsToCachedTurns` → 按 provider 语义合并（durable 取并集，其余覆盖/追加） | `src/parser.ts:3928-3974` |
| 7 汇总 | `buildSessionSummary` → `summarizeProject` 得 `ProjectSummary[]` | `src/parser.ts:1849-2003`、`2754-2768` |
| 8 巨型文件 | Claude/Codex 全文件解析下放 worker；主线程检查键交集，碰撞则整文件重解析 | `src/parser.ts:2273-2291`、`3804-3889` |

读取方式：**Claude 与 Codex 主路径**走 `readSessionLines` 流式逐行（`createReadStream` + 手切 `\n`，
`src/fs-utils.ts:98-227`）；行长超过 32 KiB 时以 `Buffer` 交付（`fs-utils.ts:8`、`:124-128`），
由紧凑解码器直接扫字节，避免把整行变成 V8 字符串。
但**并非所有 provider 都流式**：19 个 provider 文件共 31 处调用 `readSessionFile()` 把文件**整份读入**
（含整份 `.jsonl`，如 `openclaude.ts:171`、`qwen.ts:92`、`pi.ts:260`、`command-code.ts:72`、
`openclaw.ts:254`、`droid.ts:119`、`grok.ts:388`、`kiro.ts:873`、`dsh.ts:308`），
这些源一旦超过 128 MiB 会被**默认静默**跳过（见 §3.2 的 `warn()` 门控）。
时间范围早剪：`shouldSkipLine` 只看行首 2048 字节的顶层
`type`/`timestamp`，对早于 `range.start − 24h` 的 `user`/`assistant` 行在不 `JSON.parse`
的前提下丢弃（`src/parser.ts:1045-1052`）。

容错三层（一律「跳过 + 计数」，不 panic）：行级 `JSON.parse` 失败返回 `null`
（`src/parser.ts:280-295`）；文件级 catch 后写 `{ turns: [], failed: true }` 负结果标记，
指纹不变就不重读（`src/parser.ts:2492-2501`）；流级 `readSessionLines` 出错只告警并终止该文件
（`src/fs-utils.ts:222-226`），超过 4 GiB 的文件整份跳过并在 stderr 明说「其用量未被计入」
（`src/fs-utils.ts:111-117`）。

### 4.3 Claude Code JSONL 的解析

**消费的行类型**（`src/parser.ts:1715-1761`）：

| `type` | 处理 |
| --- | --- |
| `assistant` | 唯一产出计量事件的行：需要 `message.usage` 与 `message.model` 同时在场；缺任一项返回 null（`:1488-1491`） |
| `user` | 开启新 turn（取纯文本作为用户消息），并从原始行收集 `toolUseResult`（`:1717-1728`、`:1412-1429`） |
| `attachment` | `deferred_tools_delta` → MCP 清单；`queued_command` 队列提示视为 turn 起点（`:1729-1740`、`:1822-1838`、`:1768-1779`） |
| `pr-link` | 把 `prUrl` 收进当前 turn 的 `prRefs`（`:1757-1760`） |
| `ai-title` | 只在原始行取标题，不参与 turn（`:1435-1437`） |
| `summary` / `system` 等 | **无消费方**（`compactEntry`（`src/parser.ts:1080-1222`）对每行只保留 `type`/`timestamp`/`sessionId`/`cwd`/`gitBranch`/`pr-link` URL 与两类 attachment，user 文本截断保留，assistant 保留 `usage` 与 `tool_use` 块（每块只留白名单参数），`text`/`thinking` 内容块整体丢弃；`groupIntoTurns` 对这些行不分支） |

**usage 字段映射**（`src/parser.ts:1493-1503`）：`input_tokens`/`output_tokens`/
`cache_read_input_tokens` 同名直读；cache 写入取
`max(cache_creation_input_tokens, ephemeral_5m + ephemeral_1h)` 并返回
`oneHourTokens = min(1h, 总额)`（`src/parser.ts:1296-1315`）；
`cachedInputTokens` 与 `reasoningTokens` **恒为 0**（Claude 不单列推理，thinking 含在 output 内）；
`webSearchRequests` 取 `server_tool_use.web_search_requests`；`speed` 取 `usage.speed ?? 'standard'`。

**去重**：主键 `message.id`，无 id 退化为 `claude:<timestamp>`（`src/parser.ts:1578`）。
全仓 Claude 路径**从不使用 `requestId`**（`requestId` 只出现在 copilot/cursor/grokbot）。
同文件内的流式重复先由 `dedupeStreamingMessageIds` 折叠：同 `message.id` 多行只保留**最后一行**
内容，但**时间戳用第一行**（`:1658-1680`）；跨文件再靠调用方持有的 `seenKeys`
（`:1743-1745`、`:3784`；worker 线程拿到的是**空集**，其键交集由主线程校验后回退重解析，见 §2.5）。

**sidechain / subagent**：`collectJsonlFiles` 递归收 `<project>/<sid>.jsonl` 与
`subagents/**/*.jsonl`（含 `subagents/workflows/<wf>/agent-*.jsonl`，`:2082-2106`）；
`entry.isSidechain === true` 把**整个文件**标记为 sidechain（粘性，`:1442-1444`），
父会话 id 取 sidechain 行自身 `sessionId`（`:1444-1448`）；`agentType` 读同名 `.meta.json`，
缺失且路径含 `subagents/workflows/` 时回退 `'workflow-subagent'`（`:2108-2121`）。
**sidechain 的 token/cost 全额计入所有总量**，只有「人类会话人口」口径才按 `isSidechain` 排除
（`src/types.ts:268-272`）。父侧的 `agentSpawnLinks`/`spawnPrSets`/`ambiguousSpawnAgentIds`
用于把子代理花费折回发起它的 turn（`src/parser.ts:1450-1485`、`1786-1812`）。

**时间与项目**：时间戳原样取 `entry.timestamp` 字符串，**解析层不做时区转换、不做跨日拆分**
（`:1576`、`:1723`）；范围过滤用 turn 的**第一笔 assistant call** 的时间（`:2047-2059`），
跨午夜 turn 后续按 call 切片（`classifiedTurnSlicedToRange`，`:3577-3582`）。`cwd` 逐 call 记录（`:1746-1749`），
会话级 `workingDirectory` 取第一个非空值（`:1840-1847`）；`resolveCanonicalProjectPath`
向上找 `.git`，linked worktree canonical 到主仓库（`:232-275`），
Cowork 容器内 cwd 被显式判定不可信（`:202-210`），项目名改由 `spaces.json` /
`userSelectedFolders` / `title` 解析（`src/providers/claude.ts:435-461`）。
一个会话跨项目时按 call 的 cwd 切分，`projectSplit.primary` 标记花费最大的那片（`src/parser.ts:2675-2718`）。

### 4.4 Codex rollout 的解析

**文件校验**：首行必须是 `type === 'session_meta'` 且 `payload` 是对象；
**不用 `originator` 做字符串白名单**（注释说明那是客户端身份字段，白名单会让第三方前端全丢，
`src/providers/codex.ts:238-263`）。

**计量事件三种来源**（`codex.ts:1337-1476`）：

| 事件 | 处理 |
| --- | --- |
| `event_msg` / `token_count`（`payload.info`） | 主力：`info.last_token_usage` 优先；无 `last` 时用 `total_token_usage` 减上一次累计值 |
| `token_usage_record`（`payload.usage` + `response_id`） | response 级新记录，**一旦出现即成为该 rollout 的唯一来源**，其后 `token_count` 全部跳过 |
| `token_count` 无 `info` | 降级为字符估算（`chars/4`），标记 `costIsEstimated` |

`total_token_usage` 是**整个（子）会话的累计快照**，`last_token_usage` 是**本次请求的增量**；
每次都要把「上一次累计」推进为当前快照，否则混合会话会把整个累计窗口重复计一次
（注释记录了该历史 bug，`codex.ts:1462-1476`）。重复折叠按**两道、顺序固定**：
① 逐字节相同的 `info` 先丢弃（`codex.ts:1433-1435`；注释给出实测：1313 个 `token_count` 事件中
603 个是逐字节重复）；② 累计 `total_tokens` 与前一条相同时再丢弃（`:1420-1437`）。

**token 桶映射**（`codex.ts:915-1001`）：`input = max(0, input_tokens − cached_input_tokens) − billedCacheWrite`——
OpenAI 把 cached 计入 input，这里**显式归一为 Anthropic 语义**；cache write 仅当定价源对该模型
给出了真实写价（`cacheWriteCostIsExplicit`）时才入账（`:928-943`）；output 原样，
reasoning 单独成桶但通过 `billableOutputTokens` 保证与 output 不重复；`speed` 由 service tier 映射
（`flex → 'flex'`、`priority|fast → 'fast'`，`:450-467`）。

**fork 重放去重**（三层，`codex.ts:1034-1127`、`src/codex-fork-replay.ts`）：
① 1 秒突发窗口（`CODEX_FORK_REPLAY_MAX_GAP_MS = 1000`）内把复制来的
`task_started/function_call/token_count/...` 全部跳过；
② 与父 rollout 到 fork 时刻的用量身份集合对账（`total_token_usage`/`last_token_usage` 六元组、
`response:<id>`），**首条就不在父集合里则整段丢弃**，父文件不在磁盘也整段丢弃（宁可少算不重复算）；
③ 去重键用**父/子共享命名空间 + 累计值** `codex:<forkedFromId|sessionId>:<累计>:…`，
让 fork 与原会话自然碰撞。归档去重按 basename（日期目录优先，`codex.ts:603-655`）。

### 4.5 实测 vs 估算：每个 provider 的 token 来源

估算公式只有一处实现：`CHARS_PER_TOKEN = 4`、`estimateTokensFromChars(chars) = Math.ceil(chars / 4)`
（`src/token-estimate.ts:1-5`）；Copilot 另有一份内联副本（`CHARS_PER_TOKEN_LEGACY`，
`src/providers/copilot.ts:102`）。**没有任何真正的分词器**。

| 类别 | provider | 说明 |
| --- | --- | --- |
| 直读真实 usage | claude（JSONL 与 Cowork ledger）、codex（有 `info`/`token_usage_record`）、dsh、zcode、opencode、cline/cline-cli/kilo-code/ibm-bob、gemini、antigravity、hermes、copilot session-store.db | 各家字段名不同，映射到统一四桶（+reasoning） |
| 内容长度估算（`chars/4`） | cursor（无条件标估算）、cursor-agent、kiro、warp、quickdesk、grokbot、codex 无 `info` 分支、copilot 无精确字段分支 | 估算的代价是「可能偏离真实账单」 |
| 记账金额直通 | warp、kiro、codebuff（credits × $0.01）、copilot 的 chat-session 行（nano-AIU） | 部分 provider 直接读工具自报金额 |
| 远端 API | vercel-gateway | `/v1/report` 日汇总，`requestCount` 承载请求数，无请求身份 |

估算标记的传递链（只做展示、不改总量）：`ParsedProviderCall.costIsEstimated` →
`CachedCall.isEstimated`（`src/parser.ts:2807-2853`）→ 读缓存还原（`:3002`）→
`modelBreakdown[*].estimatedCostUSD` / `SessionSummary.totalEstimatedCostUSD`（`:1915`、`:1941`、`:1985`；
`src/types.ts:144-147` 注释明确「display/metadata only, never changes totals」）。

### 4.6 去重与合并一览

| 场景 | 机制 | 键 |
| --- | --- | --- |
| Claude 流式同消息多行 | 保最后内容、用第一时间 | `message.id` |
| Claude 跨文件重复 | 全局 `seenKeys`（worker 结果亦按 key 交集判定丢弃重解析） | `message.id` |
| Claude 文件追加 | 只读增量字节；增量区出现前缀已有 id → 弃增量、从 0 全量重解析 | `message.id` |
| Codex fork 重放 | 突发窗口 + 父文件身份对账 + 父命名空间键 | 见 §4.4 |
| Codex 重复 `token_count` | 累计相同/逐字节相同即折叠 | JSON 序列化身份 |
| Copilot store 行 | 内容判别式（防 id 复用） | `copilot-store:<sid>:<id>:<fnv1a64(…)>` |
| Copilot 关闭汇总 | 与 store 行配对，靠 `supplementaryAccounting` 保留 tokens/cost 但不计请求数 | — |
| OpenCode 文件 ↔ SQLite | 消息 id 相同即跳过 | `<provider>:<sid>:<msgId>` |
| Claude Desktop ledger ↔ transcript | 30 s 窗口 + 模型身份 + 用量键比对 | — |

### 4.7 解析层未决点（专项复核后收敛）

1. **【机制已验证；风险带条件】** Codex `hasTokenUsageRecord` 的置位条件是首个**可用**的
   `token_usage_record`（`codex.ts:1341-1345`）；此后**本文件**的全部 `token_count`
   被跳过（`:1359`），且该标志经 resume state 跨 append 持久（`:1152` ↔ `:829`）。
   即：只有当同一 rollout **代次回退**（先有 record、后无 record）时才会丢量；
   该情形在实际语料中未见观测记录，属机制上可达的条件性风险，且该语义已被
   `tests/providers/codex-resume.test.ts:131-169` 直接断言（续扫段中 `token_count(999)`
   被丢弃、总量 190）——属**有意设计并被测试固定**，不是偶发缺陷。
2. **【丢弃已验证；是否等于丢量仍属一致解释】** `compactEntry` 只保留 `advisor_message`
   迭代（`parser.ts:1189-1210`），源码注释自认 `fallback_message` 是既有缺口
   （`:1184-1188`），全仓零实现零测试；advisor 之所以被单独产出，是因为它的 token
   不在顶层 `usage` 总量内（`:1590-1595`）。缓存层同样不保留该迭代，且 claude 的
   `PROVIDER_PARSE_VERSIONS` 未因此 bump（`session-cache.ts:380`）；
   「fallback 重试是否另写独立 usage 行」仍未证。
3. **【已验证：确有消费】** `cachedInputTokens` 是 OpenAI 词汇的别名列，**5 个文件共 6 处**
   读取以 `max(cacheRead, cached)` 归并以防双计：`models-report.ts:247/258`、
   `audit-report.ts:109`、`export.ts:128`、`sync/otlp.ts:323`、`sync/cli.ts:415`
   （`audit-report.ts:105` 是 raw 累加、不是归并；GUI 无生产读取）。

## 5. 聚合层：会话 → 日 → 周期

### 5.1 三层聚合与两条并行管线

```
ProjectSummary[]（会话级，进程内）
   │  aggregateProjectsIntoDays()          src/day-aggregator.ts:57
   ▼
DailyEntry[]（日级）
   │  mergeDayEntries + ensureCacheHydrated src/daily-cache.ts:1508/1703
   ▼
daily-cache.v<版本>.json（聚合层的持久事实源）   src/daily-cache.ts:429
   │  unionDaysForPeriod()  ← 缓存历史日 ∪ 今日活解析    src/usage-aggregator.ts:685
   ▼
buildPeriodDataFromDays() → PeriodData（headline）     src/day-aggregator.ts:277
```

两条管线在同一函数里并行（`src/usage-aggregator.ts:1016-1060`）：

1. **headline / durable 管线**：`buildDurablePeriod`（`:908`）→ 缓存日 ∪ 今日日 →
   `buildPeriodDataFromDays`。**cost / calls / savings / sessions / 四类 token 由它决定**（`:1020`）。
2. **明细 / live 管线**：同一函数再跑 `buildPeriodData(label, liveProjects)`（`:1026`），
   只把 `estimatedCostUSD` / `unpricedModels` / `workflow` / `topReworkedFiles` /
   `pricingCoverage` 覆盖回 headline（`:1027-1031`）。

**一致解释**：headline 在源文件过期后仍保留（来自缓存），明细字段必然随源文件存活率缩水，
因此两者可能不等——注释明确这是设计而非缺陷（`:1022-1025`）。
按项目过滤时覆盖面更大：四类 token 与 `models`/`categories` 也改用 live 管线（`:1045-1052`），
因为 v15 之前的日行没有 per-project 拆分，无法切片（`:624-635`）。

### 5.2 日聚合的分桶规则（同一会话的三种日锚）

`aggregateProjectsIntoDays`（`src/day-aggregator.ts:57-275`）对同一会话用了三套日锚，
源码注释自陈（`:113-131`）：

| 类别 | 锚点 | 结果 |
| --- | --- | --- |
| 会话计数（`day.sessions` / 切片 / 项目） | `session.firstTimestamp` 的日（`:97-98`） | **整个会话只给一天 +1** |
| turn 级判定（`editTurns`/`oneShotTurns`/`categories[].turns`） | turn 时间戳的日（`:132-147`） | 跨午夜 turn **在两侧各记一次**（文档化语义） |
| call 级数值（cost/savings/calls/tokens/model/project/provider 切片） | **每个 call 自己的日**（`:193-268`） | day-N + day-N+1 精确等于整段范围 |

跨午夜 turn 的切分由 parser 的 range slicer 完成（`src/parser.ts:5833-5836`、
`classifiedTurnSlicedToRange`（`:3577-3582`，另有按谓词 `:3590-3595` 与按日集合 `:3600-3608` 两个变体）），
这正是 call 级分桶能精确的原因
（`src/day-aggregator.ts:122-131` 说明：在 parser 按 call 切片之前，call 级分桶会造成与
整段 headline 的恒定偏差）。

`output` 一律经 `billableOutputTokens(provider, output, reasoning)` 折算（`:209-213`、`:237`、`:265`），
`calls` 一律经 `behavioralCallWeight`（`:199/208/221/233/243/253/261`；该函数的定义与语义见
`src/behavioral-weight.ts:15-29`）。
turn 内出现多 provider 时，cost 各归各家，但 turn 级计数只归**主 provider**
（call 数最多者，平局保留第一个，`:157-191`）。

### 5.3 聚合键

| 维度 | 键 | 构造位置 |
| --- | --- | --- |
| 日 | `YYYY-MM-DD`（本地或指定 IANA 时区） | `src/day-aggregator.ts:27`、`:38` |
| 项目（日行） | `` `${project}\u0000${path}` `` | `src/daily-cache.ts:455` |
| 项目（payload 行） | 绝对路径折叠（Windows 大小写/分隔符）否则用标签 | `src/spend-flow.ts:28-37`、`src/parser.ts:119` |
| 项目（仓库行） | git origin 归一（`linkedOriginKey ?? originKey ?? projectOriginKey` → 目录名 → 临时） | `src/usage-aggregator.ts:1578-1581` |
| 模型（日行） | `modelRowKey(call.model, call.route)`（短名 + 渠道后缀） | `src/day-aggregator.ts:227`、`src/models.ts:1941` |
| provider 切片 | `call.provider` 原样 | `src/day-aggregator.ts:242` |
| 会话（周期唯一计数） | `` `${provider}\0${loc}\0${sessionId}` `` | `src/usage-aggregator.ts:1101-1104` |
| 会话（跨项目折回） | `` `${projectSplit.primaryProjectPath}\0${sessionId}` `` | `src/parser.ts:5191`、`src/day-aggregator.ts:89` |
| 任务分类 | `turn.category`（13 类，正则常量、不可配置） | `src/classifier.ts:200`、`src/types.ts:195-208` |
| 分支 | 分支名原样 + turn 级 carry-forward；**行标识 = `projectId` + `branch` 两字段**（累加键只有 branch，项目隔离靠逐项目调用） | `src/branch-spend.ts:299-302`、`:241-244`、`:343-345` |

### 5.4 缓存分层与「绝不缩水」

| 层 | 载体 | 内容 |
| --- | --- | --- |
| L0 进程内 | `sessionCache` Map（range+provider 键，180 s TTL，验证干净的条目可延长） | `src/parser.ts:6339-6360` |
| L1 会话级磁盘缓存 | `session-cache.v10/`（分片：`index.<provider>.<hex>.json` + 每 `provider-日` 一个 piece + `.keys.json`） | `src/session-cache.ts:239`、`:244`、`:2165-2167` |
| L2 日级磁盘缓存 | `daily-cache.v75.json`（`DailyCache{days: DailyEntry[]}`） | `src/daily-cache.ts:364`、`:429` |

- L2 文件名是**版本化**的：`DAILY_CACHE_VERSION = 75`、`MIN_SUPPORTED_VERSION = 28`
  （`src/daily-cache.ts:364-365`）。`docs/architecture.md:141` 写的 `daily-cache.json`
  已与实现不符（以源码为准）。
- 整体重派生的三个触发（`src/daily-cache.ts:1793-1811` 的 `ensureCacheHydrated`）：
  ① `savingsConfigHash` 变化（= `localModelSavings` + `priceOverrides` + `flatRateModels`
  三段拼接，`src/usage-aggregator.ts:219-224`）；② 缓存从未在「完整会话解析」下定稿
  （`complete !== true`）；③ `tzKey` 与当前时区不同（`src/daily-cache.ts:549-551`）。
- 版本不匹配**不是丢弃而是 adopt**：`loadDailyCache` 解析失败或版本不符时
  `adoptOlderDailyCaches` 读缓存目录里**所有** `daily-cache*.json`（含 `.tmp`/`.bak`/旧版本），
  按「complete 优先 → 高版本 → 新 mtime」排序，只采纳同一时区的文件
  （早于 `tzKey` 字段的旧文件无法辨别时区、仍被保留，`src/daily-cache.ts:854-857`），
  再按 `(day, provider)` 求并集并标 `carried`（`src/daily-cache.ts:817-869`）。这是 v14 起写入的
  「NEVER-LOSE history」原则：会话文件会过期消失，只有这份缓存记得住历史。
- **settle 窗口 7 天**（`SETTLE_DAYS = 7`，`src/daily-cache.ts:1446`）+ **不缩水守卫**：
  `mergeDayEntries`（`src/daily-cache.ts:1508`）以 primary 为准，`guardPartialSurvival` 打开时，
  若「日期早于 settle cutoff 且 fresh.calls < cached.calls」，则**缓存胜**
  （`isPartialSurvival`，`src/daily-cache.ts:1452-1454`、`:1583-1592`）。理由：源文件持续老化，
  新的 provider 作用域解析只是**下界**而非更正。读路径另有 `'prefer-richer'` 变体，
  任何日期都取解释更多 calls 的一侧（`src/usage-aggregator.ts:733`）。
- 缓存**只存过去的整天**（`d.date < today`，`src/daily-cache.ts:1738-1742`），今天一律 live；
  保留 3650 天（`src/daily-cache.ts:1677`），回填窗口 365 天（`:1669`）。
- 损坏恢复：写入走「临时文件 + fsync + rename」且权限 `0o600`（`src/daily-cache.ts:934-948`）；
  读取时 `JSON.parse` 失败**静默**落到 adopt 路径（`:791-795`，注释明确「损坏的当前文件
  不能让旧文件里的历史陪葬」）；`detectPhantomSpend`（`:1646-1658`）只告警、绝不改数。
- 锁：`src/daily-cache.ts` 内部只有进程内 Promise 链锁 `withDailyCacheLock`（`:1660-1666`）；
  **跨进程**锁是 `session-refresh.lock`（`src/cache-refresh-lock.ts:8`，心跳 10 s、
  stale 90 s、默认等待 120 s），它保护的是 session 缓存事务，不是 daily 缓存。
  会话缓存是否「完整」决定 daily 能否 finalize（`src/usage-aggregator.ts:237` →
  `src/parser.ts:6728-6733`）。锁超时/不可用时降级为只读旧快照
  （`src/parser.ts:6436-6444`），等待期有心跳保活（`:6420-6432`）。

### 5.5 会话计数：为什么需要三个函数

| 函数 | 位置 | 用途 |
| --- | --- | --- |
| `uniqueCanonicalSessionCount(sessions, projectPath?)` | `src/usage-aggregator.ts:1106` | 单项目/单分组场景去重 |
| `uniqueCanonicalSessionCountFromProjects(projects)` | `:1119` | `ProjectSummary[]` 全量去重 |
| `uniqueCanonicalSessionCountForProvider(projects, provider)` | `:1131` | 按 provider 过滤后再去重 |

三者共用 `canonicalSessionCountKey`（`:1101-1104`）= `provider \0 loc \0 sessionId`；
没有 `sessionId` 的会话单独计入 `anonymous`，不参与去重。

`boundSessionCount(liveUnique, cacheDayBound, unknownCache)`（`:1151-1158`）：
无未知缓存 → 精确 `{sessions: liveUnique, basis: 'identity'}`；
有未知缓存 → 下界 `{sessions: max(liveUnique, cacheDayBound), basis: 'partial'}`。
三个具体风险点与处理：① 日占用 tick **不可加**，跨两天会产生两个 tick，所以取
**单日最大**（`maxDaySessionBound`，`:1160-1167`；注释 `:62-64`）；
② live 精确值与缓存下界不可相加，只能取 max 并把 `basis` 降级为 `partial`，
UI 显示 "At least N sessions"（`src/session-count-label.ts:12-21`）；
③ 同一会话跨项目切片会被拆成多个 `SessionSummary`，靠键里带 `primaryProjectPath` 折回，
仓库行再减去共享量（`:1603-1604`）。测试：`tests/session-period-count.test.ts:115/174/213/283/288`。

### 5.6 provider 切片 overlay 与 gateway 剔除

**overlay 的必要性**：`buildDurablePeriod` 在 `providerFilter !== 'all'` 时**故意不水合全量缓存**
（`src/usage-aggregator.ts:925-929`），否则一个 provider 标签页会退化成「隐藏的全 provider 冷扫描」。
于是 provider 作用域的新解析结果必须「叠」到共享缓存上：

```
overlayProviderDaySlices(baseline, fresh, provider)                 src/usage-aggregator.ts:585
  = mergeDayEntries(fresh 的该 provider 切片, 缓存的该 provider 切片,
                    guardPartialSurvival = true)
```

双方先被压成单 provider 的日（`sliceDayToProvider`，`:418`）。`fresh` 必须是 primary，
守卫才会按预期方向读到「缩水」。**结论**：fresh 可以**填充**缓存缺的 `(date, provider)`、
可以在 settle 窗口内的日子上胜出，但**永远不能缩小一个已封存的 settled 日**（注释 `:567-584`）。

**gateway 剔除**：Vercel AI Gateway 的 `/v1/report` 是按天按模型的**聚合行**，没有请求身份，
与被指向它的本地工具记录的是同一批请求 ⇒ 同时计入必然双算（`src/parser.ts:6242-6249`）。
处理方式：**写侧保留**（过去某天的聚合行无法重新获取，`src/parser.ts:6291-6298`、`src/usage-aggregator.ts:448-452`），
**读侧剔除**：`excludeProviderFromDay`（`:464-540`）从日级每个汇总里减去该 provider
（`sub = max(0, total − part)`），并在 `models`/`projects`/`categories` 上逐键相减，
但 `day.providers[provider]` 原样保留，使 provider 行仍能报出全量并标 `excludedFromTotal`
（`:1777-1785`）；被剔除的金额由 `excludedGatewayTotals`（`:554-565`）披露。
默认开关 `excludesAggregateOnlyProviders` = `providerFilter === 'all' && !gatewayIncludedInTotals()`
（`src/parser.ts:6256-6258`）；`codeburn gateway-totals include` 可改为计入。

### 5.7 时间区间边界

权威定义在 `src/cli-date.ts`（进程本地时区，`--timezone <IANA>` > `CODEBURN_TZ` >
机器时区，`src/main.ts:555-569`）：

| period | start | end |
| --- | --- | --- |
| `today` | 本地 00:00:00.000 | 本地 23:59:59.999 |
| `week` | 今天 − 7 天的 00:00 | 今日 23:59:59.999 |
| `30days` | 今天 − 30 天的 00:00 | 同上 |
| `month` | 本月 1 日 00:00 | 同上 |
| `all` | 今天 − 6 个月的 1 日（`ALL_TIME_MONTHS = 6`） | 同上 |
| `lifetime` | `new Date(1970, 0, 1)` | 同上 |

（`src/cli-date.ts:16`、`:77-87`、`:165-192`。）

三个要点：**① `week` 实际覆盖 8 个日历日**（8 个 `date` 键）、`30days` 覆盖 31 个——
标签写 "7 Days"，日桶是 8 个，这是源码事实（`:154-162`、`:174`）；
**② 一律闭区间**：`ts >= start && ts <= end`（`src/parser.ts:2058`、`:3550`），
日键比较用零填充字符串的 `>=`/`<=`（`src/daily-cache.ts:1697-1701`），
**没有左闭右开路径**；**③ DST 安全**：日期推进用 `new Date(y, m, d ± N)` 的日历算术
（`src/usage-aggregator.ts:697`、`src/daily-cache.ts:1814`、`:1948`），唯一毫秒回退是
`--from` 缺省（`src/cli-date.ts:128-131`）。已在运行时对照验证（`TZ=America/New_York`，
跨 2026-11-01 fallback）：日历算术保持本地 00:00，毫秒算术偏移 1 小时，
`dateKey`/`dateKeyInTz` 在 fallback 日的四个时刻都归入同一日期键（详见 §5.9）。
缓存永远不含今天；今天由 live 提供，并集前先用 `filterProjectsByDays` 重锚跨午夜 turn，
避免今天的分类/turn 数留在昨天（`src/usage-aggregator.ts:943-957`）。
另一个「今天」的不同义：durable 实时扫描窗口是 `[今日 00:00, now]`（`:917-919`），
而报告区间的 end 是 `23:59:59.999`。

**与本项目（TokenScope）的显著差异**：codeburn 只有单一闭区间模型，
**没有**滚动窗口（`rolling_hours`）也没有「时刻模式左闭右开」。

### 5.8 关键类型

- `DailyEntry`（`src/daily-cache.ts:484-508`）：`date / cost / savingsUSD / calls / sessions /
  inputTokens / outputTokens / cacheReadTokens / cacheWriteTokens / editTurns / oneShotTurns /
  models(键=modelRowKey) / categories / providers / projects? / carried?`。
  其中 `sessions` 是**当日占用 tick**，不是唯一身份。
- `ProviderDaySlice`（`:464-482`）：必填 `calls/cost/savingsUSD`，其余**可选**——这种可选性是有意的，
  让读取方区分「没有拆分」与「拆分为 0」（`src/usage-aggregator.ts:44-54`）。
- `PeriodData`（`src/menubar-json.ts:8-85`）：周期汇总；`sessionCountBasis` 的
  `identity` = 精确、`partial` = 下界；`models[].inputTokens` 等可选字段的契约是
  **缺失必须渲染为「未知」，不得当 0，也不得用周期总数顶替**（`:41-54`）。

### 5.9 聚合层未决点（专项复核后收敛）

1. **【已验证：无清理器，可能永久残留】** `.hb-next` 是固定名
   `session-refresh.lock.hb-next`（`src/cache-refresh-lock.ts:408`），只有成功 rename 才消费它
   （`:411`）；失败分支仅有一次机会性 `unlink`（`:416`）。信号清理路径（`:83-114`）、
   `cache-sweep.ts:17` 的文件名正则、`session-cache.ts:2716-2775` 的 `.tmp` 清扫
   **均不覆盖**它——进程若恰在 `writeFile` ↔ `rename` 窗口内被杀则会残留。
   因为是固定名、同名覆盖，故不累积，但可永久留存。
2. **【已验证（运行时对照）】** DST 行为：在 `TZ=America/New_York` 下对照了两种日期推进——
   日历算术（`new Date(y, m, d − N)`）跨 2026-11-01 fallback 时保持本地 00:00，
   而毫秒算术（`− N × 86400000`）会偏移 1 小时；`dateKey` / `dateKeyInTz` 在 fallback 日
   的四个时刻（含重复出现的 01:30 两次）都归入同一个日期键。
   趋势/周期对比层另有 `tests/granular-history.test.ts:424`、`tests/period-diff.test.ts:259`
   的定向测试；**日聚合分桶**本身仍无专门测试（本条由结构推断升级为运行时对照验证）。
3. **【已验证】** 进程内并发 `loadCache` **无 promise 级 single-flight**：
   `src/session-cache.ts:1805-1867` 的 memo 在完成后才写入（`:1865`），同 scope 并发会各读一次
   （对照 `codex-cache.ts:203-216` 有 `inFlightLoads`）。**容错不对称同样确认**：
   全量读按整片 `validateFiles`（`:1260-1263`、`:1666-1674`）失败即整片判 dirty，
   范围读逐成员 `continue`（`:1693-1701`）只丢坏成员；构造「坏成员 + 源已删除的好成员」
   即可让同一份缓存出现两种结果，用 `CODEBURN_CACHE_SCOPE=all`（`:1806`）可开关对照。

## 6. 计价层：token → USD

### 6.1 完整调用链

缓存回放（最常见路径）：

1. `cachedCallToApiCall(call)`（`src/parser.ts:2977`）。
2. `billableOutputTokens(provider, outputTokens, reasoningTokens)`（`:2983`）——
   `REASONING_INCLUDED_IN_OUTPUT = {claude, codex, copilot, dsh}` 时取 `outputTokens`，
   否则 `output + reasoning`（`src/models.ts:60-67`）。
3. `calculateCost(pricingModelAt(call.model, call.timestamp), input, output, cacheCreation,
   cacheRead, webSearch, speed, cacheCreationOneHourTokens, provider)`（`:2984-2988`）。
4. `getModelCosts(model)`（`src/models.ts:1527`）→ 为 null 直接返回 0（`:1528-1540`）。
5. 主公式求和（`:1543-1563`，见 §6.5）。
6. `costUSD: call.costUSD ?? recordedCostFallback(...) ?? costUSD`（`src/parser.ts:3001`）：
   工具有记录价时优先。`applyLocalModelSavings` 另把配置了 savings 映射的调用
   强制 `costUSD = 0` 并把基线价记入 `savingsUSD`（`:1322-1342`）。

冷解析的同类调用点：`src/parser.ts:1512-1522`（Claude JSONL）、
`src/providers/codex.ts:952-962`、`src/providers/copilot.ts:725`、
`src/providers/cursor.ts:830/902/928`、`src/providers/kiro.ts:242/394/490/650/768`
等；`src/audit-report.ts:125-128` 是审计**重算**，不参与记账。
**落盘的是 USD**：`costUSD` 原样进 `CachedCall`，只在输出/导出边界乘汇率
（`src/main.ts:812-818`、`src/export.ts:89/129`）。

### 6.2 计价数据来源与优先序

| 数据面 | 位置 | 生成者 | 运行时角色 |
| --- | --- | --- | --- |
| 主表（内置） | `src/data/litellm-snapshot.json`（6039 key） | `scripts/bundle-litellm.mjs` | 静态 import → `loadSnapshot()`（`src/models.ts:237-243`） |
| 缺口表（内置） | `src/data/pricing-fallback.json`（563 key，与主表 key 交集为 0） | 同上 | `fallbackCosts`（`:248-255`，key 转小写、首个胜出） |
| 运行时拉取 | `~/.cache/codeburn/litellm-pricing.json` | `loadPricing` 从 GitHub raw 拉 LiteLLM JSON（`:93`） | 24 h TTL、schema 8（`:94`、`:110`、`:509-520`） |

生成规则（`scripts/bundle-litellm.mjs`）：源优先级 LiteLLM → `MANUAL_ENTRIES` →
models.dev（仅第一方厂商集合）→ OpenRouter（`:5-19`）；主表两遍扫描（直连条目优先，
再带前缀条目，`:194-276`），带前缀条目剥首段写入裸 key 但只补空槽
（`fillsOnly`，`:266-270`）；缺口表把 models.dev 的 **USD/百万**除以 1e6（`:358-361`）、
OpenRouter 本身是 per-token（`:377-380`），并带一层 carry-forward：
「上一版有价、新版无来源覆盖」的 key 原样搬运（`:398-418`）。

运行时优先序（`src/models.ts`）：`pricingCache = 内置主表 + BUILTIN_PRICE_OVERRIDES`
（`:230-235`、`:257`）→ 磁盘缓存命中则 `mergeSnapshotFallbacks`（**live 缺的 key 用内置补，
然后重打内置覆盖**，`:522-527`）→ 未命中且非 `CODEBURN_PRICING_SNAPSHOT_ONLY` 时联网拉取
（`:547-554`）→ 失败保持内置（`:555-558`）。**内置覆盖永远压过 live**。

`BUILTIN_PRICE_OVERRIDES`（`:127-137`）显式覆盖：`deepseek-v3.2`、`swe-2`、
Cursor composer 系列（`composer-1/1.5/2/2.5`）、`kimi-k2.7-code-highspeed`。

**关键实现细节**：`fallbackCosts` 全文件只被三处引用（定义 `:248`、大小写索引 `:288`、
命名空间推导 `:1174`）——**缺口表只能通过「大小写不敏感 + 小写精确相等」命中**
（`:1345-1349`），不参与 exact/canonical/prefix/routed 任何一环，也没有前缀回退。
两表 key 的**精确交集为 0**；另有 5 个 key 仅大小写不同（缺口表按小写索引匹配，见 §6.4 第 9 步）。

### 6.3 条目字段与单位

主表 tuple（`src/models.ts:84-91`）：

```
[row]: [input, output, cacheWrite|null, cacheRead|null, fastMultiplier|null,
        longContextTier|null, flexTuple|null]
```

- 前 4 位单位 **USD/token**（例：`claude-opus-4-6 = [5e-6, 25e-6, 6.25e-6, 5e-7]`
  ⇔ $5 / $25 / $6.25 / $0.50 每 1M）。
- 第 5 位 `fast` 是**倍数**（`gpt-5.6-luna = 2`）；第 6 位 tier =
  `{threshold, input, output, cacheWrite, cacheRead, fast?}`；第 7 位 flex 是同构嵌套 tuple。
- tuple 长度可变（实测）：**主表为 4 位（旧条目 7 条）/ 6 位（5891 条）/ 7 位（141 条，含 flex），
  没有 5 位**；缺口表为 5 位（219 条）与 6 位（344 条），都无 flex。
  6 位里的 tier 槽与 7 位的 flex 槽都可为 `null`，解析时按缺省处理。
- 运行时 `ModelCosts`（`:10-34`）：`inputCostPerToken` / `outputCostPerToken` /
  `cacheWriteCostPerToken` / `cacheReadCostPerToken` / `webSearchCostPerRequest`（常量 0.01）/
  `fastMultiplier` / `cacheWriteCostIsExplicit` / `longContextTier` / `flex`。
- 用户配置入口 `priceOverrides` 的单位是 **USD / 1,000,000**（`:786-789`）。

### 6.4 模型名匹配阶梯（`getModelCosts`，`src/models.ts:1294-1364`）

先派生三个键：`withPrefix`（去掉 `@pin` 与 `-YYYYMMDD`，`:1296`）、
`canonicalName = getCanonicalName(model)`（`:1116-1132`：剥 `@…` → 剥 8 位数字日期 →
剥 `[…]` 尾注；**若剥完的整串本身是别名就停手**；再只剥一层**已知**命名空间，
已知集 = `EXTRA_NAMESPACES` + 价格表中出现过的 `/` 前缀，`ollama` 被显式排除）、
`canonical = resolveAlias(canonicalName)`（用户别名优先，其次内置别名精确、再小写；
内置别名 `BUILTIN_ALIASES` 共 117 条，`src/models.ts:597-771`）。

查找顺序即优先级：

1. 精确价覆盖 `getPriceOverrideExact(model, withPrefix, canonicalName, canonical)`（`:1300`）。
2. 「裸名显式别名优先」特例：`canonical !== canonicalName && withPrefix === canonicalName
   && pricingCache.has(canonical)` → 取别名目标（`:1308-1310`，防 `snowflake/claude-4-opus`
   这类被剥离后的 reseller 裸键压过官方行）。
3. `pricingCache.has(withPrefix)`（`:1312`，大小写敏感）。
4. `pricingCache.has(canonical)`（`:1314`）。
5. 路由候选逐个试（`:1316-1324`）：逐级剥 `ROUTER_PREFIXES`
   （`omniroute:`、`cp/`、`cline-pass/`、`cline-free/`、`cmd/`、`antigravity/`、
   `orcarouter/`、`cliproxy/`，`:1201-1218`），每个候选各做一次命名空间剥离
   （`:1244`）；未知 `provider/model` 保持原样 ⇒ 未定价。
6. 覆盖表前缀匹配 `canonical.startsWith(key + '-')`（`:1326-1327`）。
7. 主表前缀匹配：**key 长度降序**，`canonical === key || canonical.startsWith(key + '-')`
   （`:1329-1336`；注释明确：防 `gpt-5-mini` 掉到 `gpt-5`）。
8. 覆盖表大小写不敏感（`:1338-1339`）。
9. 主表/缺口表大小写不敏感索引，先 `canonical` 后 `withPrefix`（`:1345-1349`）。
10. 变体后缀剥离后**递归**：`:(thinking|cloud)`、`-TEE`（`:1248-1256`、`:1351-1361`）。
11. 都没有 → `null`（`:1363`）。

补充：`pricingModelAt`（`:1272-1281`）只对 `codex-auto-review` 与 `kimi-for-coding`/`kimi-code`
按调用时间换模型（用户别名/价覆盖优先）；日期或 `-v1:0` 等 Bedrock 版本号**不**被当作本地模型
（`:1383-1388`）。显示名（`getShortModelName` 等）与计价完全分离；
`modelRowKey`（`:1941-1948`）只决定「一行一 SKU」的分组，不移动金额。

### 6.5 主公式

```
safe(n) = (Number.isFinite(n) && n > 0) ? n : 0                     // models.ts:1543
W_total = max(safe(cacheCreation), safe(oneHourCacheCreation))      // :1545
W_1h    = safe(oneHourCacheCreation)
W_5m    = max(0, W_total − W_1h)                                    // :1546
promptTokens = safe(input) + safe(cacheRead) + W_total              // :1547（cache 计入 tier 阈值）
tiered  = tieredCostsFor(model, speed === 'flex' ? costs.flex ?? costs : costs,
                         promptTokens, provider)                    // :1548
M       = speed === 'fast' ? tiered.fastMultiplier : 1              // :1549
p_cw    = cacheWriteCostPerToken(model, tiered)                     // :1555

costUSD = M × ( safe(input) × tiered.inputCostPerToken
              + safe(output) × tiered.outputCostPerToken
              + W_5m × p_cw
              + W_1h × p_cw × 1.6                                    // :1560
              + safe(cacheRead) × tiered.cacheReadCostPerToken
              + safe(webSearch) × tiered.webSearchCostPerRequest )   // 0.01/次
```

- `1.6` = `ONE_HOUR_CACHE_WRITE_MULTIPLIER_FROM_FIVE_MINUTE_RATE`（`:112`），
  即 1h 写价 = 5m 写价 × 1.6；当写价取兜底 `1.25 × input` 时等价于输入价的 2.0×，
  但显式写价条目未必是 1.25×（实测 826 条显式写价条目中，`input > 0` 的 822 条里有 240 条
  比率 ≠ 1.25），两种说法不可混用。
  该乘法**无模型/厂商判断**（`:1560`），
  但 1h 桶只由 **`claude` provider 的解析路径**填充（`src/parser.ts:1296-1315` →
  缓存 `:2874` → 回放 `:3013`；`ParsedProviderCall` 根本没有该字段，
  `src/providers/types.ts:32-38`，其它 provider 显式写 0，`:2819`；
  展示层 `session-why.ts:167/202/712` 只读不构造）。注意不能简写成「仅 Anthropic」——
  经代理路由时 Claude 也可能跑非 Anthropic 模型（【已验证】）。
- `M` 乘**整条和**（含 cache、web search），不是只乘 input/output。
- 负值/NaN/Infinity 一律夹到 0（`:1543`，防坏 JSONL 反向扣钱）；单价另夹进 `[0, 1]` USD/token
  （`safePerTokenRate`，`:305-309`；作用于 **live 拉取解析与用户价格覆盖**两条路径，
  内置快照的卫生由打包脚本 `bundle-litellm.mjs` 负责）。

### 6.6 cache write 的三态

| 情形 | 行为 |
| --- | --- |
| 价格源给出真实写价 | 用它（`cacheWriteCostIsExplicit = true`，`:159`） |
| 价格源未给出写价 | `buildCosts:155` 先把 `cacheWriteCostPerToken` 填成兜底值 `input × 1.25`（这一步只是**取值**，不做模型判断） |
| 真正决定写档费率的函数 | `cacheWriteCostPerToken()`（`src/models.ts:1511-1514`）：`cacheWriteCostIsExplicit` 为真、或**模型串**匹配 `/claude\|anthropic/i` → 用兜底值；否则返回**输入价**，不凭空加 25% |

对应的 provider 侧分档决策：Codex 仅在 `getModelCosts(pricingModel)?.cacheWriteCostIsExplicit`
为真时才把 token 挪进写档（`src/providers/codex.ts:936-942`）；
Copilot 的 `inputTokens` 是 cache 包含式，计价前先减去 cache read/write 两档
（`src/providers/copilot.ts:1666-1673`）。

### 6.7 tier / fast / flex / 峰谷

- **长上下文 tier**（`tieredCostsFor`，`:203-222`）：触发条件是**整请求 prompt tokens**
  （input + cacheRead + cacheCreation）**严格大于**阈值（`:211`）；
  阈值来自 LiteLLM key 后缀（`272k` → 272000），且**只取最大阈值**（`:363-387`）。
  仅对 `TIERED_PRICING_PROVIDERS = {codex, antigravity, claude}` 生效（`:194`）——
  其它 provider 即使表里有 tier 也不套用（理由：只在实测确实按阶梯计费的 provider 上启用）。
  **不是边际分段**：命中后整个请求的 input/output 换成 tier 费率，未公布的槽位沿用基础价。
  `grok-4.6` 是硬编码特例：prompt ≥ 200k 时整请求换高价档（`:175-176`、`:205`）。
- **fast**：`speed === 'fast'` 时整条和 × `fastMultiplier`。倍数来自 Anthropic 的
  `provider_specific_entry.fast`，或由 OpenAI `<rate>_priority` 键**一致推导**
  （要求 input/output 都存在且所有桶比值一致、上限 100、四舍五入 4 位，`:337-361`）；
  Codex 的 Fast 设置走的就是这条路（`src/providers/codex.ts:453-467`）。
- **flex**：`speed === 'flex'` 时**整张价目**换成 `costs.flex`（缺则标准价），再走同一 tier 逻辑；
  flex 是**费率而非比值**（`:389-395`、`:1548`）。
- **峰谷/时段**：`src/peak-hours.ts:13-18` 明确「只分类、绝不重算价」，
  `classifyPeak` 只返回 `peak|off-peak|unknown`；`models-report.ts:271-286` 把**已存成本**
  按窗口拆成消费占比，两段相加等于 Cost 列。

### 6.8 估算、记账金额、订阅、未定价

- **估算**：`chars/4`（`src/token-estimate.ts:1-5`）；provider 具体实现见 §4.5。
  Copilot 的 **chat-session 行**带 `copilotCredits` 字段时**不估 token 直接折美元**
  （该字段是 GitHub 记录的整请求 nano-AIU 计费值，`src/providers/copilot.ts:207/561/584`；
  `creditsToUsd(nanoAiuToCredits(nano))`，1e9 nano-AIU = 1 credit = $0.01；
  `src/copilot-aiu.ts:6-18`、`src/providers/copilot.ts:725`、`:1853-1854`）；
  但 **session-store 行**的 `total_nano_aiu` 只是元数据、成本仍按 token 计价
  （`:3050`、`:3075`），**shutdown rollup 腿**同样按 token 计价（`:1685`）。
  Kiro 有 `executionCredits` 时 `× $0.04`（`src/providers/kiro.ts:14-18`）；
  Codebuff `credits × 0.01`（`src/providers/codebuff.ts:16`）。
- **工具记录价优先**：`recordedCostFallback(model, costUSD, recorded)` 仅在
  「codeburn 算得 0 + 工具有 >0 记录 + 非 expected-free」时采用记录值（`src/models.ts:1460-1462`）；
  某些 provider 整条链路直通记录价（`REPORTED_COST_PROVIDERS`，`src/parser.ts:2794-2804`）。
- **订阅 / 计费模式**：只有 `'metered' | 'subscription'` 两态（`src/models.ts:1803`），
  `callBillingMode` 优先采用 provider 自己记录的 facts，否则用生效路由的默认值，
  直连且无记录 ⇒ `undefined`（不猜，`:1915-1920`）。**计费模式不影响单价**
  （`models.ts:1797-1798` 注释与 `tests/model-routes.test.ts:178-193` 断言
  「routes never move a dollar」）。订阅覆盖的另一种表达是 `proxyPaths`：
  `totalCostUSD` 保留 API 价，另记 `totalProxiedCostUSD`（`src/parser.ts:2764`）。
- **flat rate 是「$0 正确」名单，不是把价改成 0**：`isFlatRateModel`（`:1014-1018`）只影响
  未定价告警与覆盖率分母（`:1476`、`:1446-1456`、`:1493`）。
- **未定价模型**：`calculateCost` 返回 **0**（`:1528-1540`）；告警默认静音，
  需要 `CODEBURN_VERBOSE=1`（`:1486-1501`），且每个名字每进程最多一次；
  渲染期由 `findUnpricedModels`（`:1464-1484`）统一收口并排除 local/flat/零价覆盖；
  暴露面包括 `models --unpriced`、导出 CSV 的 `unpriced` 列、菜单栏 payload 的
  `unpricedModels`/`pricingCoverage`。**统计上不做任何补偿**，只区分
  `estimatedCostUSD`（估算）与真实 `costUSD`。

### 6.9 货币换算

汇率源为 Frankfurter（ECB 数据，`src/currency.ts:15`），磁盘缓存
`~/.cache/codeburn/exchange-rate.json`（24 h TTL，`:14`、`:75-109`）；默认 USD，
取不到汇率回退 USD 并保留旧缓存（`:111-137`）；汇率必须有限且在 `[1e-4, 1e6]`（`:18-26`）。
`convertCost` 只在展示/导出边界调用（`:182-190`），且**故意不四舍五入**
（四舍五入交给 `roundForActiveCurrency` / `formatCost`，`:68-73`、`:192-203`）。
⇒ **用量事实的存储恒为 USD**（同步与 OTLP 路径同样传原值，`sync/push.ts:405`、
`sync/otlp.ts:285`），切换币种只改渲染。**例外**：导出产物与 renderer 持久化的**所有 complete report 快照**会保留换算后的展示值
（`export.ts:89/129/165`、`usePolled.ts:135-199`、`overviewSnapshot.ts:88-94`）——
因此「永远 USD」只对**用量事实存储**成立。

### 6.10 官方文档与源码不符之处（以源码为准）

| 文档说法 | 源码事实 |
| --- | --- |
| `docs/how-it-works.md:5`「Hardcoded fallbacks for **all Claude and GPT-5 models**」 | 内置兜底是**整份 LiteLLM 快照（6039 key）+ MANUAL_ENTRIES**，另加 563 条 models.dev/OpenRouter 缺口表；防模糊误配靠的是 **key 长度降序前缀**与精确阶梯，不是「Claude/GPT-5 全部硬编码」 |
| 同处「fast mode multiplier **for Claude**」 | 乘数机制通用：Anthropic 自报 `provider_specific_entry.fast` 或 OpenAI `*_priority` 推导；Codex Fast 实际吃它 |
| `docs/plans-and-quota.md:19`「Copilot spend is `total_nano_aiu / 1e9`, never token-priced USD」 | 只对**计划/额度视图**成立；成本层的 `costUSD` 在没有 nano-AIU 时确实按 token 计价 |
| `docs/architecture.md:277` 只提 `bundle-litellm.mjs` 拉 LiteLLM + MiniMax 手工覆盖 | 漏了 models.dev/OpenRouter 缺口表、`fillsOnly` 只补空槽、carry-forward 搬运 |
| `docs/architecture.md:141` 缓存文件写作 `daily-cache.json` | 实际是版本化文件名 `daily-cache.v75.json` |

### 6.11 计价层未决点（专项复核后收敛）

1. **【已验证（结构 + 运行时）】** 缺口表没有前缀回退：结构上 `fallbackCosts` 只在
   大小写索引（`:288`）与命名空间推导（`:1174`）被引用，而索引只做精确 `get`；
   运行时实测——缺口表独有键（`qvq-max`、`qwen-flash`、`qwen-mt-turbo` 等）精确命中且
   大小写不敏感（`QVQ-MAX` 同值）；对**任何前缀段都不在主表**的键，加后缀 `-zzz`
   返回 `null`（`qvq-max-zzz`、`qwen-flash-zzz`、`qwen-mt-turbo-zzz` 等实测），
   符合「无前缀回退」；一旦某个前缀段本身在主表，则仍会被主表回退命中（见下条）。
   **订正**：本文早先举的 `minimax-m3-custom` 是**错误示例**——`minimax-m3` 本身在主表，
   该串靠**主表前缀回退**命中。并补充限定：即使目标键只在缺口表，**主表的更短键**仍可能截胡
   （实测 `qwen-plus-character-ja-zzz` 命中的是主表 `qwen-plus` 费率，而非缺口表的
   `qwen-plus-character-ja`）。因此「未定价」的准确条件是**该 id 的任何前缀段都不在主表**。
2. **【已验证】** `BUILTIN_PRICE_OVERRIDES` 的表归属：实测 7 个覆盖 key 中，
   `deepseek-v3.2`、`kimi-k2.7-code-highspeed` 在主表（取值被覆盖重写），
   `swe-2`、`composer-1/1.5/2/2.5` **不在任何表**（完全由覆盖提供）。
   由于没有任何 key 同时存在于缺口表与覆盖名单，`fallbackCosts` 未打补丁这一差别
   **在实践中不可观测**（「覆盖只作用于 `pricingCache`」是结构事实）。
3. **【已验证（运行时）】** `grok-4.6` 的 200k 阈值与别名传播：
   `resolveCanonicalModelId('grok-4.6-high')` = `grok-4.6`；
   `tieredCostsFor('grok-4.6', costs, 199_999)` 返回基础价（$2/$6 每 1M），
   `200_000` 返回高价档（$4/$12 每 1M）；别名 `grok-4.6-high` 同样命中高价档。
4. **【已验证：确无分档】** `webSearchCostPerRequest` 实测 5 个不同厂商模型
   （claude-opus-4-6、gpt-5.5、gemini-2.5-pro、deepseek-v3.2、grok-4.6）全部为 `0.01`；
   代码侧 `WEB_SEARCH_COST` 是常量（`models.ts:111`），无 provider 分支。

## 7. 总览与速查

### 7.1 全链路一句话

> `discoverSessions()` 找到各工具落在本机的会话文件 → `createSessionParser()` 把它解析成
> **统一调用事件**（1 条 = 1 次 API 调用，含 token 四桶 + reasoning + webSearch 与 `costUSD`）
> → `parser.ts` 把调用折成 **turn → session → project** 并写入分片会话缓存
> → `day-aggregator` 把项目折算成**日报**（聚合层的持久事实源：会话文件被工具清理后，
> 这段历史只此一份，版本化文件）
> → `usage-aggregator` 用「缓存历史日 ∪ 今日活解析」产出**周期汇总**（headline），
> 再用会话级明细覆盖估算/未定价等展示字段 → 渲染成 TUI / JSON / menubar-json / CSV。

### 7.2 关键文件与职责

| 文件 | 职责 |
| --- | --- |
| `src/providers/*.ts`（49 个文件，43 个 provider） | 发现 + 解析各工具数据面 |
| `src/parser.ts`（6785 行） | Claude 专用解析、通用解析主循环、去重、会话/项目汇总、缓存事务 |
| `src/session-cache.ts`（3200 行） | 分片会话缓存（`session-cache.v10/`）、指纹与增量、跨进程锁协作 |
| `src/daily-cache.ts`（2012 行） | 日报缓存（`daily-cache.v75.json`）、版本迁移与「绝不缩水」合并 |
| `src/day-aggregator.ts`（348 行） | 项目 → 日报的聚合规则（三种日锚、行为权重、模型行键） |
| `src/usage-aggregator.ts`（2112 行） | 周期汇总、overlay、gateway 剔除、会话计数、菜单栏 payload |
| `src/models.ts`（2035 行） | 价格表装载、模型名匹配阶梯、`calculateCost`、路由与计费模式 |
| `src/data/litellm-snapshot.json`（6039 条） + `pricing-fallback.json`（563 条） | 内置价格主表与缺口表 |
| `src/classifier.ts` | 13 类任务分类（纯正则，无 LLM 调用） |
| `src/parse-workers.ts` | 冷解析的 worker_threads 并行（仅 Claude/Codex） |

### 7.3 关键数字

| 数字 | 值 | 出处 |
| --- | --- | --- |
| provider 数 | 43（32 eager + 11 lazy）；对外口径 42 工具 + 1 gateway | `src/providers/index.ts:200/204` |
| 唯一联网 provider | 1（`vercel-gateway`，`network?: true` 全仓唯一） | `src/providers/types.ts:145`、`src/providers/vercel-gateway.ts:136` |
| 估算公式 | `ceil(字符数 / 4)`，无分词器 | `src/token-estimate.ts:1-5` |
| web search 单价 | 固定 $0.01/次 | `src/models.ts:111` |
| 1h 缓存写价 | 5m 写价 × 1.6；仅在写价取兜底 `1.25 × input` 时才等于输入价 × 2.0（显式写价条目未必是 1.25×） | `src/models.ts:112`、`:1560` |
| cache 读 / 写兜底 | 输入价 × 0.1 / × 1.25 | `src/models.ts:155-156` |
| 会话缓存版本 | `CACHE_VERSION = 10` | `src/session-cache.ts:239` |
| 日报缓存版本 | `DAILY_CACHE_VERSION = 75`（最低支持 28） | `src/daily-cache.ts:364-365` |
| 价格表缓存 TTL | 24 h（`~/.cache/codeburn/litellm-pricing.json`） | `src/models.ts:93-94` |
| settle 窗口 | 7 天（更早的日不允许被新解析「缩小」） | `src/daily-cache.ts:1446` |
| 读取上限 | 一次性 128 MiB（仅 `warn()`，默认静默）/ 流式 4 GiB（`notice()`，必打印） | `src/fs-utils.ts:7/16/44-47/112-117` |
| 并行解析门槛 | 待解析 ≥ 200 MB 且核数 > 2 且可用内存 ≥ 4 GB | `src/parse-workers.ts:18/25/66` |
| 日报保留 | 3650 天（`:1677`）；回填窗口 365 天（`:1669`） | `src/daily-cache.ts:1669/1677` |

### 7.4 读这份代码时最容易踩的七个坑（本文的结论索引）

1. **「42 个集成」不是 provider 数**（43 个），且 provider 数 ≠ 文件数（`pi.ts` 导出 `pi` 与 `omp`）。
2. **Claude Code 的 JSONL 解析不在 `src/providers/claude.ts`**——那里只有发现逻辑与
   Cowork ledger；真正的 Claude JSONL 解析在 `src/parser.ts:3269`（`parseClaudeFileFull`）。
3. **token 四桶的语义在 provider 侧就已归一**：Codex/Copilot/ZCode 的 `input` 原本含
   cache，被显式扣减成 Anthropic 式语义；`reasoning` 是否加回输出由
   `billableOutputTokens` 的 provider 白名单决定（claude/codex/copilot/dsh 已含）。
4. **日报是权威，headline 不是会话求和**：session 级求和只用于明细字段覆盖；
   缓存历史日在源文件消失后仍保留（adopt + 不缩水守卫）。
5. **同一会话有三种日锚**（会话计数锚首日、turn 级计数锚 turn 日、call 级数值锚各 call 日），
   跨午夜 turn 在两天都记 turn 数——这是文档化的既有语义，不是 bug。
6. **价格匹配是一条 11 级阶梯**，缺口表（models.dev/OpenRouter）只能**小写精确**命中、
   没有自己的前缀回退；但**主表的更短键**仍可能截胡
   （实测 `qwen-plus-character-ja-zzz` 命中主表 `qwen-plus`）。
   未定价模型一律计 0 且默认静默。
7. **日期区间是单一闭区间模型**（`start 00:00` 至 `end 23:59:59.999`），
   没有滚动窗口或半开区间；`week` 覆盖 8 个日期键、`all` 是「近 6 个月」而非全部。

### 7.5 复核记录

- **研究阶段**：读取层 / 解析层 / 聚合层 / 计价层四路并行精读，各自产出带 `文件:行号` 证据
  与三档标注的笔记（`R1`–`R4`），逐节核对后整合成本文。
- **第一轮复核**：三路独立复核分块回查全部引用（§1–§3 / §4–§5 / §6–§7），
  共抽样核对约 350 处引用，报出 10 条事实性错误与 27 条不精确表述。已修正的要点：
  ① 128 MiB 一次性读超限走 `warn()`（受 `CODEBURN_VERBOSE` 门控）与 4 GiB 流式超限走
  `notice()`（必打印）被混为一句；② `devin` 路径应为 `~/.local/share/devin/cli`；
  ③ `grokbot` 的 token 是全估算而非真实；④ `BUILTIN_ALIASES` 实为 117 条（非约 170）；
  ⑤ §7.3 的「保留/回填」行号与数值错序（`:1677` / `:1669`）；⑥ cache write 三态把
  「取值」与「判定」归错函数；⑦ `cachedCallToApiCall` 读缓存时**仍会重算** `calculateCost`；
  ⑧ §5.2 悬空引用、§4.5 `codebuff` 笔误、§5.8 类型行号上限（`:8-85`）、DST 例证行号归属；
  ⑨ 若干 provider 身份形态（devin/forge/goose/crush/warp 统一为 `<dbPath>:<id>` 虚路径）、
  copilot 目录名大小写、WSL 附加条件、OpenClaw 文档对照的措辞。
- **第二轮复核**：两路独立复核再回查修订后的全文（§1–§4 / §5–§7，含跨章节数字一致性
  与计算口径），验证第一轮 16 条修正中 14 条准确，并新报 3 条事实错误：
  ① §4.2「没有任何一处把整份 JSONL 读成字符串」与源码矛盾（19 个 provider 文件共 31 处
  调用 `readSessionFile()` 整份读入，超 128 MiB 默认静默跳过）；② Copilot 的 nano-AIU
  直折美元在 **chat-session 行**而非 session-store 行；③ 主表 tuple 实际分布为 4/6/7 位
  （5 位只存在于缺口表）。均已修正，另收窄若干表述（`compactEntry` 的保留范围、
  Codex 重复折叠的顺序、切片器函数边界、`safePerTokenRate` 的适用路径、
  adopt 时区规则、`CODEBURN_PARSE_WORKERS` 非法值语义等）。
- **第三轮复核**：两路独立复核各自回查修订后的半篇文档，确认第二轮 16 条修正中 15 条准确，
  并新报 1 处数字硬伤（`readSessionFile()` 的文件数：19 个文件 / 31 处调用，而非 20 个）
  与 3 处可收窄表述（Copilot chat-session 行的字段名为 `copilotCredits`、
  显式写价统计的可比子集口径、DST 测试的覆盖面）。均已修正。
  第三轮未发现新的机制性错误，独立回查 120+ 处引用全部命中，编辑副作用扫描零异常。
- **终检**：逐项验证第三轮后的全部改动（数字、限定条件、函数边界）与全文骨架，
  确认旧值零残留、括号/表格/跨行格式无破损。
- **存疑专项复核（第四轮）**：先做静态深挖（`failedProviders` 传播链、三家 DB 发现的
  SQL 与身份构造、`amp` v2 口径、Codex `token_usage_record` 置位语义、
  Claude `fallback_message` 丢弃行为、`cachedInputTokens` 下游、
  `.hb-next` 清理器穷举、`loadCache` 并发与容错、antigravity RPC 触发条件、
  分支行标识、货币换算边界、1h 桶来源、Frankfurter 数据源），
  再做**运行时验证**：安装依赖后以只读探针直接调用被测模块
  （缺口表前缀回退、内置覆盖表归属、`grok-4.6` 阈值与别名、web search 分档、
  1h 写价数值、DST 日历算术对照），并跑 codeburn 自带测试 **369 项**——9 个测试文件：
  `models`／`day-aggregator`／`session-period-count`／`daily-cache-carry-forward`／
  `vercel-gateway-excluded-totals`／`codex-pricing-1075`／`parser-advisor-usage`／
  `warp-billing-cost-cache`／`usage-aggregator-freshness`，全部通过。
  13 条存疑中 11 条升级为**已验证**、2 条收窄为带限定的一致解释；并订正一处错误示例
  （`minimax-m3-custom` 实际命中主表前缀，非「未定价」形态）。验证全程未触碰用户数据：
  探针把 `CODEBURN_CACHE_DIR` 指向临时目录、并锁定内置快照模式。
- **第五轮复核（存疑专项终检）**：逐条回查上述改写与 5 处正文同步，复跑只读探针与部分
  自带测试；确认两处数字订正（测试项数 **369**、`cachedInputTokens` 实为 **5 文件 6 处**）
  与三处「话说满」的收窄（`.hb-next` 的残留条件、缺口表后缀命中的前提、
  Codex 代次回退风险）均已落实，无机制性新错误。
- 复核结论分档与本文一致；凡复核未能确证的条目已在各节「未决点」中如实标注，
  不以推测替代证据。
