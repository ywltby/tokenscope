# TokenArena 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`poco-ai/TokenArena`](https://github.com/poco-ai/TokenArena)，
> main 分支 HEAD `9e75fcf`（2026-10-15 浅克隆到临时目录后逐文件精读）。
> **未运行其 CLI / Web 服务，未改动本机任何 agent 数据。**
>
> 证据分档：**已验证**（源码可直接指认，附 `文件:行号`）／**一致解释**（多条代码
> 证据指向同一结论，无单点直证）／**未验证推论**（结构推断，未实测）。
>
> 本文只记录字段名、路径规则与计算口径，不含任何真实会话内容。

## 1. 范围与方法

TokenArena 是一个 **pnpm monorepo**，由两个工作区组成（`pnpm-workspace.yaml`）：

- **采集端 `cli/`** —— npm 全局包 `@poco-ai/tokenarena`（`cli/package.json`），
  读本机各 AI coding CLI 落下的日志，聚合成"30 分钟桶 + 会话元数据"后上传。
- **服务端 `web/`** —— Next.js（App Router）+ PostgreSQL + Prisma + better-auth，
  提供 Usage 仪表盘、排行榜、成就与徽章分享。

代码规模（文件数按 shell globstar 语义统计 `cli/src/**/*.ts` 等——git pathspec
字面语义会漏掉零级目录下的文件；排除 `*.test.ts`；行数为文件总行数）：

| 范围 | 文件数 | 行数 |
| --- | --- | --- |
| `cli/src/**/*.ts` | 61 | 11071 |
| `cli/src/parsers/*.ts` | 22 | 5541 |
| `web/lib/**/*.ts` | 58 | 11424 |
| `web/lib/usage/*.ts` | 16 | 3111 |
| `web/lib/leaderboard/*.ts` | 7 | 1934 |
| `web/lib/achievements/*.ts` | 8 | 2616 |
| `web/app/**/*.ts` | 13 | 668 |

分析手段：`rg` 定位符号 → 分段读原文 → 交叉核对调用点与 DDL/Prisma schema；
每条例行性结论都附 `文件:行号`，读者可自行回源码核验。

术语约定：**桶（bucket）** = 30 分钟粒度的用量聚合单元；**会话元数据** = 会话级
汇总（时长、消息数、按模型用量）；**采集端 / 服务端** = `cli/` 与 `web/`。

## 2. 总体架构：本地聚合一次，服务端只入库不重算事实

与"本地统计型"工具（例如本仓已研究过的 cc-switch）不同，TokenArena 的用量事实
**不在本机留库**：采集端每次运行都把本机日志全量重扫、重新聚合成固定形状的
上传载荷，再增量推给服务端 Postgres。本机唯一的持久状态是"上传清单"、
配置（API key、deviceId）与同步状态文件（`status.json`，`state.ts:48-55`、
`runtime/paths.ts:24-26`）。

```
本机各 agent 日志 (JSONL / JSON / SQLite / 目录约定)
        │  cli/src/parsers/*.ts（每 agent 一个适配器）
        ▼
  TokenUsageEntry[]（统一逐条用量事件）+ SessionEvent[]（时序角色事件）
        │  cli/src/domain/aggregator.ts      │ cli/src/domain/session-extractor.ts
        ▼                                     ▼
  TokenBucket[]（30 分钟桶）             SessionMetadata[]（会话级汇总）
        │  隐私过滤（projectMode: hashed / raw / disabled）
        │  增量过滤（cli/src/domain/upload-manifest.ts）
        ▼
  POST /api/usage/ingest  { schemaVersion: 2, device, buckets[], sessions[] }
        │  web/lib/usage/ingest.ts（zod 校验 → Prisma upsert）
        ▼
  PostgreSQL（UsageBucket / UsageSession / leaderboard_user_day / 榜单快照 …）
        │  web/lib/usage/queries.ts（查询期聚合）
        │  web/lib/pricing/*（查询期按 models.dev 价目现算费用）
        ▼
  /usage 仪表盘、排行榜、成就、徽章
```

两端的职责边界（**已验证**）：

- 采集端负责"读日志 → 逐条事件 → 桶 + 会话"的全部脏活；服务端拿到的已经是
  固定形状的聚合结果，**没有原始逐条事件**（`web/lib/usage/contracts.ts:59-111`
  的摄入 schema 只有 bucket 与 session 两类对象）。
- 服务端在摄入时**不对桶做二次聚合**（桶写入即 upsert 覆盖，token 以上报为准）；
  但**会话侧会做一次归一化**：`normalizeSessionUsage`（`ingest.ts:83-149`）
  把 `modelUsages` 重新求和后**覆盖**顶层四桶（`:137-149`），只有会话没有
  `modelUsages` 时才回退用顶层字段（`:152-167`）。
- **费用（USD）全部由服务端计算，采集端上传的载荷里没有金额字段**：
  bucket 侧在查询期按当前价目表现算；session 侧在**摄入时**算好落库
  （见 §6.4）。
- 契约版本号 `schemaVersion = 2` 在两端各有一份定义
  （`cli/src/domain/types.ts:131`、`web/lib/usage/types.ts:1`），
  摄入请求必须携带且必须等于 2（`web/lib/usage/contracts.ts:104`）。

**注意**：`cli/src/domain/types.ts` 里 `UploadTokenBucket` 带有
`deviceId` / `hostname` 字段（`:44-57`），但服务端摄入 schema 中这两个字段
位于请求顶层的 `device` 对象里（`web/lib/usage/contracts.ts:105-108`）；
两端形状差异由采集端在发送前重组（见 §3.8）。

## 3. 采集端：20 个适配器怎么读日志

### 3.1 适配器契约与注册

适配器接口极简（`cli/src/parsers/types.ts:19-23`）：`parse(): Promise<ParseResult>`
加可选的 `isInstalled()`；`ToolDefinition` 只有 `id / name / dataDir` 三个字段
（`:10-14`）。注册靠**副作用 import**：`cli/src/index.ts:2-21` 依次 import 20 个
parser 模块，每个模块文件末行执行 `registerParser(new XxxParser())`
（`registry.ts:8-13`），在 CLI 构建命令树之前完成。

`isInstalled()` 的缺省回退是 `existsSync(dataDir)`（`registry.ts:23-31`），
但 20 个 parser 中有 18 个自实现该方法（只有 codex 与 gemini-cli 未实现、
走回退），所以 `dataDir` 常只是"声明值"：
cline 声明 `~/.cline` 而真实扫描 7 个宿主目录（`cline.ts:27-29` vs `:49-60`）、
roo-code 声明 `getHostRoots()[0]`（宿主根而非扩展目录）、qwenpaw 的 `dataDir` 甚至是一个**文件**路径（`qwenpaw.ts:16-18` 声明、
`:83` 赋值）。`detectInstalledTools()` 只被
命令层用于展示，**扫描不做安装过滤**——每个 parser 都会被调用。

解析调度在 `cli/src/services/parser-service.ts:29-48`：**串行**逐个 parser
调用 `parse()`，单个 parser 抛错只 `warn` 后继续，不影响其他 agent。

读取方式：全仓 grep `createReadStream|readline` 零匹配——**没有流式读取**。
文本类 parser 都是整文件读入后逐行解析：11 个走共享 helper
（`fs/utils.ts:48-54` 的 `readFileSafe` + `:59-70` 的 `parseJsonl`），
gemini-cli / openclaw / copilot-cli 等自实现同款逻辑；SQLite 类
（opencode / hermes / kiro / cursor）根本不走文本解析。坏行一律静默丢弃。

### 3.2 覆盖矩阵：20 个 agent 靠什么读

按数据面把 20 个 parser 分成四类（`id` 即上传后的 `source`）：

| 类别 | agent（`id`） | 数据面 |
| --- | --- | --- |
| A. 本地 JSONL 会话日志 | `claude-code`、`codex`、`gemini-cli`、`qwen-code`、`kimi-code`、`openclaw`、`gsd`、`oh-my-pi`、`pi-coding-agent`、`grok-build`、`copilot-cli` | 各工具自己落的逐条 JSONL |
| B. VSCode 扩展 globalStorage | `cline`、`roo-code` | `tasks/<id>/ui_messages.json` 里的字符串化 JSON |
| C. 直读 SQLite | `opencode`、`hermes`、`kiro`（SQLite 优先，退化 JSONL） | 工具自带 DB 的表 |
| D. 非逐请求日志源 | `droid`（配对 `<basename>.settings.json`）、`cursor`（云端 CSV + 本地 token）、`qodercli`（credits 差值）、`qwenpaw`（日期×模型聚合表） | 结构性数据，不是逐请求日志 |

### 3.3 逐个 agent 的读取要点

**A 类·Claude Code**（`claude-code.ts`）：根集合 `~/.claude` + `CLAUDE_CONFIG_DIR`
（realpath 去重，`:25-50`）；主链递归扫 `projects/**/*.jsonl`（`:83`），
次链 `transcripts|sessions/**/*.jsonl` **只产会话事件、不产 token**，且事件的
project 恒为 `"unknown"`（`:157-199`，特别是 `:188`）。
只取 `type === "assistant"` 且 `message.usage` 存在的行，要求
`input_tokens` / `output_tokens` 至少一个非 null（`:124-131`）；
`uuid` 全局 Set 去重（`:133-137`）；`input=usage.input_tokens||0`、
`output=usage.output_tokens||0`、`cached=usage.cache_read_input_tokens||0`、
**`reasoning` 恒 0**（`:139-149`）；`cache_creation_input_tokens` 未映射。
`tool_use`/`tool_result` 仅进会话事件（`:109-122`），token 无双计。
project 取路径首段目录名（`-` 分割取末段）的启发式（`:60-67`）。

**A 类·Codex CLI**（`codex.ts`）：**本仓语义最复杂的 token 口径**。优先
`payload.info.last_token_usage`；缺失时对 `total_token_usage` 做**同文件差分**
（per-model `prevTotal` Map，`:131`、`:177-203`），且该 key 首见时**直接采信
累计值**（`:200`）——同一模型首次出现的累计量会被全额计入。四桶映射：
`cached = cached_input_tokens`、`reasoning = reasoning_output_tokens`、
`input = max(0, input_tokens − cached)`、`output = max(0, output_tokens − reasoning)`
（`:208-217`），全零则跳过（`:219-226`）。`sessionId = 文件路径`（`:229`；各家的会话标识来源其实很杂：gemini-cli /
qwen-code / kimi-code / droid / openclaw / codex 用**完整文件路径**，
grok-build 与 copilot-cli 用**会话目录名**，hermes / opencode 用 DB 字段 id，
qwenpaw 用 chats.json 的 `session_id`，claude-code 用 basename
（`fs/utils.ts:92-94`），cline / roo 用 task id）。
project 优先 git `owner/repo`，否则 `cwd` 末段（`:69-87`）。
第一遍扫 `session_meta` 定位 project 时**遇坏行即 break**（`:109-122`）——
但这只终止元数据扫描，主循环（`:133` 起）仍会解析同一份内容，后果是
project 退化为 `"unknown"`。另外 `turn_context` 也会产 `role:"user"` 的会话
事件（`:141-147`），直接影响会话轮次划分与 `userPromptHours`。
差分基准还有一个放大边界：`prevTotal.set` **只在回退分支**执行
（`:202`）——同一文件内混用 `last_token_usage` 与 `total_token_usage` 时
基准不更新，后续 entry 可能把已计入的累计量再算一次。

**A 类·Gemini CLI**（`gemini-cli.ts`）：自建扫描 `~/.gemini/tmp/*/chats/**`
（深度 ≤2，同时收 `.jsonl` 与 `.json`，`:26-59`；`:25` 注释说 "descend one
level"，与 `:44` 的 `depth > 2` 实现并不一致——实际允许两层）。双分支映射
（`tokens` 对象或 `usage/usageMetadata/token_count`），两分支都是**裸减法**：
`input = (input||0) − cached`、`output = (output||0) − thoughts`（`:204-241`）。
注释明确"Gemini 的 output 已含 thoughts、input 已含 cached，减掉避免重复计数"
（`:208-209`）。裸减法无 `Math.max` 兜底 → 一旦 `cached > input`，负值 entry 会在
聚合层被**整条丢弃**（见 §3.4）。无任何去重。

**A 类·Qwen Code**（`qwen-code.ts`）：发现 `<dataDir>/<workspace>/chats/*.jsonl`
（`:65-91`）；扣除式 `input = max(0, totalInput − cached)`、
`output = max(0, totalOutput − reasoning)`（`:194-195`）；`uuid` 去重位于零值
过滤之后（`:174-186`）；`sessionId = 文件路径`（`:136`）。

**A 类·Kimi Code**（`kimi-code.ts`）：三层结构 `~/.kimi/sessions/<workDirHash>/<session>/wire.jsonl`
（`:74-105`），project 名靠 `~/.kimi/kimi.json` 的映射表还原（`:136-160`）；
仅 `type === "StatusUpdate"` 产 entry（`:230`）；`input_other → input`、
`output → output`、`input_cache_read → cached`、reasoning 恒 0
（`:235-238`、`:270-273`）；
**`input_cache_creation` 参与"全零判定"后被丢弃**（`:240-247`）；
`message_id` 去重（`:249-252`）；model 与 timestamp 支持跨行继承/三级回退
（`:206-217`）。

**A 类·OpenClaw**（`openclaw.ts`）：根含 `~/.openclaw`、`^\.openclaw-.+` 变体目录
与三个遗留目录名（`.clawdbot/.moltbot/.moldbot`，`:17`、`:63-82`）；扫
`agents/<agent>/sessions/*.jsonl`（非递归，`:96-118`）；13 个用量字段别名，
取值规则是"首个 number 且 > 0"（`:52-61`、`:165-187`）；reasoning 恒 0（`:181`）；
project = agent 目录名（`:109`）。数字时间戳按**毫秒**直解（`:141-143` 的
三元两臂相同，是恒等死代码）——与 grok-build 的 "<1e12 按秒" 策略不一致。

**A 类·GSD / Oh My Pi / Pi Coding Agent**（`gsd.ts`、`oh-my-pi.ts`、`pi-coding-agent.ts`）：
**三者同源**（实测 diff：gsd 与 oh-my-pi 只差常量与标识符改名，**归一化后
逐行相同**；与 pi 的差异是默认目录 + 签名换行，文件行数 241/241/238）。
头行 `{type:"session", id, cwd}` 覆写 sessionId/project 后 break
（`gsd.ts:146-158`）；四桶取值用 `getUsageNumber` 多别名"首个 > 0"（`:72-85`、
`:187-200`）；**按 message 行 `id` 全局去重，无 id 不去重**（`:211-214`）；
无相减、无差分。

**D 类·Droid**（`droid.ts`）：递归 `~/.factory/sessions/**/*.jsonl` 并配对
`<basename>.settings.json`，**用量只在 settings 文件里**（jsonl 只产会话事件，
`:45-67`、`:134-138`）；含相减：`input = max(0, inputTokens − cacheReadTokens)`、
`output = max(0, outputTokens − thinkingTokens)`，cached/reasoning 原样
（`:151-160`）；`sessionId = 文件路径`，project = 目录 slug 末段（`:69-72`、`:96`）；
entry 时间戳取**会话首条消息**的时间（`:121-123`、`:176`）。

**A 类·Grok Build**（`grok-build.ts`）：两层目录且**必须存在 `updates.jsonl`**
（`:181`）；`events.jsonl` 只出现在注释里、从未读取（`:23`）；
`usage.modelUsage` 会**展开为多模型多 entry**（`:108-111`）；相减同 droid
（`:114-119`）；时间戳优先 `_meta.agentTimestampMs`（`:61-65` 两处取值），
顶层 `obj.timestamp` 为数字且 < 1e12 时按秒 ×1000（`:70-75`）；user chunk
按 `promptId` 去重，`turn_completed` **不去重**（`:264-283`）。

**A 类·GitHub Copilot CLI**（`copilot-cli.ts`）：递归查找文件名恰为 `events.jsonl`
（带 visited 防环，`:44-74`）；**只有 `session.shutdown` 事件产用量**，per-model：
`input = max(0, inputTokens − cacheReadTokens)`、`output = outputTokens`、
`reasoning = 0`、`cached = cacheReadTokens`（`:146-173`）；
**`cacheWriteTokens` 字段声明了但从未消费**（`:37`；且 `:159-161` 的零值判定
只查 input/cacheRead/output 三项，**只含缓存写入的 shutdown 会整条 entry 消失**——
比"静默丢弃"更重）；
无 shutdown 行 = 该会话完全无用量。project 取**最近一条
`session.start/resume` 上下文**的 `gitRoot || cwd` 末段（`:84-90`、`:122-124`）。

**B 类·Cline**（`cline.ts`）：单扩展 ID `saoudrizwan.claude-dev`（`:14`），
7 个宿主 × 平台根拼 `<root>/User/globalStorage/<ID>`（`:16-24`、`:49-60`），
定深 `statSync` 不递归；读 `state/taskHistory.json` 与 `tasks/<id>/ui_messages.json`
（`:92-110`，token 藏在 `text` 字段的字符串化 JSON 里）；
`input = max(0, tokensIn) + max(0, cacheWrites)`、`output = max(0, tokensOut)`、
`cached = cacheReads`、reasoning 恒 0（`:132-151`）；
**entry 不带 sessionId**（`:143-152`）→ 会话级汇总拿不到 token；
无去重、无累计值差分。project 取 `cwdOnTaskInitialization` 的末段
（`:99-102`）。

**B 类·Roo Code**（`roo-code.ts`）：单 ID `rooveterinaryinc.roo-cline`（`:14`），
同款 7 宿主（宿主常量 `:16-24`，`findExtensionDirs` 在 `:46-57`）；任务表优先 `tasks/_index.json.entries`，否则一层 readdir +
`history_item.json`（`:89-113`）；token 口径与 cline 一致，
但 entry 带 `sessionId = taskId`、project 取 `workspace`、model 回退
`apiConfigName || "roo-unknown"`（`:139-142`、`:170-189`）。

**C 类·OpenCode**（`opencode.ts`）：SQLite 分支实现最完整的一家（另见
hermes、kiro；三者都以 SQLite 为主路径）。
5 个候选 root（env/XDG/默认/`LOCALAPPDATA`/`APPDATA`，Set 去重，`:53-63`）；
读 `<root>/opencode.db` 表 `message`，SQL 原文见 `:229-236`
（`session_id` 物理列 + `json_extract(data,'$.role'/'$.time.created'/'$.modelID'/'$.tokens'/'$.path.root')`）；
优先内置 `node:sqlite`（`:105-142`），失败退 `sqlite3 -json` CLI（`:144-181`），
再失败降级 legacy JSON 文件（`:211-225`、`:294-369`）；
映射直取、**不扣减**：`input/output/reasoning = tokens.*||0`、
`cached = tokens.cache?.read||0`（`:281-284`）。该文件被 vitest 覆盖率排除
（`cli/vitest.config.ts:19`；该排除清单共 9 项，还含 `cursor.ts:17`、
`hermes.ts:18`、`kiro.ts:16`、`client.ts:13`、`sync-service.ts:15` 等），
SQLite 分支无单测。

**C 类·Hermes**（`hermes.ts`）：读 `~/.hermes` 下的 SQLite（`state.db`，
`dataDir` 即 db 所在目录，`:64-70`），表 `sessions`（SQL 原文 `:19-31`）与
`messages`（`:33-39`）；`node:sqlite` 优先、`sqlite3 -json` 回退（`:123-207`）；
四桶**直取无相减**（`:239-242`）；**project 恒 `"unknown"`**（`:260`）；
会话级累计、不差分、无 id 去重；`messages` 查询失败时降级为只产 buckets（`:269-280`）。

**C 类·Kiro**（`kiro.ts`）：基础路径可被 `KIRO_BASE_PATH` 覆盖（`:47-51`）；
数据文件二选一：`dev_data/devdata.sqlite` 优先，否则 `dev_data/tokens_generated.jsonl`
（`:246-257`）。**SQLite 走 `sqlite3` CLI 硬依赖、没有 `node:sqlite` 优先**
（`:70-79`、`:258-269`），锁冲突时复制 `db+shm+wal` 快照（`:89-108`）；
SQL 过滤 `WHERE tokens_prompt>0 OR tokens_generated>0`（`:64-68`）；
JSONL 回退路径的整份记录时间戳取**文件 mtime**（`:129-139`）；
model 由 `<base>/*/*.chat` 的 metadata 时间线推断（10 分钟容差）+
`normalizeModelName` + 回退 `"kiro-agent"`（`:165-231`、`:290`）；
`input = max(0, tokens_prompt)`、`output = max(0, tokens_generated)`、
reasoning/cached 恒 0、project 恒 `"unknown"`、**无 sessionId**（`:276-301`）。
`id` 字段读了不用，无去重。**parsers 目录下唯一没有测试文件的 parser**。

**D 类·Cursor**（`cursor.ts`）：**唯一联网的 parser**。先用 `sqlite3 -json` 从
`state.vscdb` 取 `cursorAuth/accessToken`（锁冲突时复制 `db+shm+wal` 快照，
`:62-116`），再 GET `cursor.com/api/dashboard/export-usage-events-csv?strategy=tokens`
（认证尝试数 = 1 + 2×Cookie 值个数，JWT `sub` 解析成功时最多 5 种、否则 3 种；
网络异常/超时静默返回空结果——`:162-170` 打 skip 标记、`:300-304` 捕获，
`:135-152` 为 URL 与认证尝试的构造；**HTTP 非 2xx 会抛错**
（`:175`）并经 parser-service 记为 warn，不是静默）；
CSV 列映射：`input = 'Input (w/ Cache Write)' + 'Input (w/o Cache Write)'`、
`cached = 'Cache Read'`、`output = 'Output Tokens'`、reasoning 恒 0、
project 恒 `"unknown"`、**无 sessionId**、时间粒度是**当天 00:00:00Z**、
`sessions: []`（`:237-244`、`:311-352`）。

**D 类·Qoder CLI**（`qodercli.ts`）：**单位是 credits 不是 token**（`:44-48`）。
取数链：`~/.qoder/logs/runs/<runId>/qodercli.log` 中必须含
`"quota/usage response:"`，配合同目录 `manifest.json`（`:383-406`）；
`totalCreditsUsed = userQuota.used + orgResourcePackage.used + addOnQuota.used`
（`:162-168`），`inputTokens = 相邻轮次的**正差值**`（`:276-286`），
output/reasoning/cached 恒 0。去重靠**全局跨 run 时序差分**（`:418-420`），
负差（配额重置）整轮丢弃（`:277-278`）；transcripts 只产会话事件（`:314-315`）。

**D 类·QwenPaw**（`qwenpaw.ts`）：用量文件是"日期 × 模型"的**聚合表**
（`:21-29`），不是逐请求日志；timestamp = 当天 `T00:00:00.000Z`（`:92-99`）；
project 恒 `"unknown"`、cached/reasoning 恒 0、`call_count` 未使用；
会话侧走**自建 `aggregateSessions`**（`:298-391`），与公共 `extractSessions`
存在多处语义差异：sessionHash 是**明文 session_id**（`:342`，未哈希）、
会话 token 全 0（`:351-356`）、`userPromptHours` 为空（`:381`）、
`activeSeconds` 直接等于 `durationSeconds`（`:378`）、`primaryModel` 恒
`"unknown"`（`:387`）、秒数取整用 `Math.floor`（`:345`）。`new Date(msg.timestamp)`
缺 NaN 守卫（`:229`），与 qwen-code/kimi-code 不一致。

### 3.4 统一事件模型与合法性校验

所有 parser 的产出都收敛到 `TokenUsageEntry`（`cli/src/domain/types.ts:4-14`）：
`sessionId? / source / model / project / timestamp / inputTokens / outputTokens /
reasoningTokens / cachedTokens`（四桶均为**绝对值**，不是增量）。

合法性校验 `hasInvalidTokenCounts`（`token-usage.ts:10-17`）：四桶任一不是
安全整数或为负 → 返回 true，**整条 entry 被丢弃**——聚合层
（`aggregator.ts:22-24`）与会话层（`session-extractor.ts:28-30`）都执行这一过滤。
多数 parser 自带 `Math.max(0, …)` 钳制（cline、roo-code、kiro、qwen-code、
grok-build、copilot-cli 等，见 §3.3 各处）；gemini-cli 是**唯一不做钳制**的
扣减实现，其裸减法风险就落在这道聚合层丢弃上。

一个重要分工：**会话级的 token 汇总只统计带 `sessionId` 的 entry**
（`session-extractor.ts:23-26`）。所以 cline / cursor / kiro / qwenpaw 这类
"无 sessionId"的 parser 的会话**拿不到 token**（仍会产出并上传会话对象、
token 为 0；其中 qwenpaw 走自建会话聚合、cline 也产出会话行）；完全不上传
会话的只有 `cursor.ts:352` 与 `kiro.ts:304`（`sessions: []`）。

### 3.5 两种产出：30 分钟桶 + 会话元数据

**桶**（`aggregator.ts:8-58`）：`roundToHalfHour` 按**本地时区分钟**向下取整
（`<30 → 0`，否则 `30`）再 `toISOString()`；分组键
`source|model|project|bucketStart`，只做相加、条间零去重；`totalTokens`
在累加侧独立计算（`aggregator.ts:50-54`），语义是四桶之和。

**会话**（`session-extractor.ts:68-188`）：按 sessionId 分组事件；`durationSeconds`
= 首末消息墙钟差；**`activeSeconds` = 各轮"生成时长"之和**，轮的边界定义是
"用户提示后的第一个 AI 响应 → 下一个用户提示前的最后一个 AI 响应"
（`:61-67`、`:96-118`）。两点限定：结算条件是严格 `turnEnd > turnStart`
（`:98`、`:114`），**单响应轮贡献 0 秒**；且该区间含轮内工具调用/等待间隔，
不是纯生成时长（`:65` 的注释口径比实现更乐观）。`userPromptHours[24]` 按
`getUTCHours` 计数（`:125`），
但**上传 schema 里没有这个字段，采集端会丢弃它**（`domain/types.ts:84` vs
`:97-117`）；`primaryModel` = 该会话按 totalTokens 降序第一个模型（`:131-158`，
缺省 `""`）；
`sessionHash = sha256(sessionId).slice(0,16)`（`:160-163`）——**会话身份是小写
16 位十六进制字符串，不可逆**；project 缺省 `"unknown"`（`:167`）。

`addHostname()` / `addHostnameToSessions()`（`aggregator.ts:63-68`、
`session-extractor.ts:193-198`）**在全仓无调用点**（死代码）；hostname 其实在
构造时就已写入（`aggregator.ts:19`/`:35`、`session-extractor.ts:79`/`:169`），
上传时再由 `sync-service` 用设备名覆盖为相同值（见 §3.8）。

### 3.6 项目身份与隐私三态

`toProjectIdentity`（`project-identity.ts:3-25`）是唯一过滤点，三态：

| `projectMode` | `projectKey` | `projectLabel` |
| --- | --- | --- |
| `hashed`（默认） | `HMAC-SHA256(salt, project)` 十六进制前 16 位（64 bit） | `Project ` + key 前 6 位 |
| `raw` | 项目名原文 | 项目名原文 |
| `disabled` | `"unknown"` | `"Unknown Project"` |

盐（`projectHashSalt`）由服务端下发（见 §4.5），CLI 不生成；`disabled` 只抹掉
项目名、**仍然上传** token。注意 CLAUDE.md 关注的路径归一化问题在这里不存在也
不处理：CLI 对 project 字符串**不做任何分隔符/盘符/URI 归一**，直接 HMAC——
项目身份完全依赖各 parser 提取的字符串（大多是"路径末段"启发式）。

### 3.7 增量上传清单（upload-manifest）

清单文件：`~/.local/state/tokenarena/upload-manifest.json`
（`runtime/paths.ts:28-30`；XDG 逻辑见 §3.9）。结构
（`domain/upload-manifest.ts:32-38`）：

```json
{ "version": 1,
  "scope": { "apiKeyHash", "apiUrl", "deviceId", "projectHashSaltHash",
             "projectMode", "snapshotProtocolVersion" },
  "buckets": { "<deviceId|source|model|projectKey|bucketStart>": "<sha256-16>" },
  "sessions": { "<deviceId|source|sessionHash>": "<sha256-16>" },
  "updatedAt": "..." }
```

- 键：桶 `deviceId|source|model|projectKey|bucketStart`（`:104-112`）、
  会话 `deviceId|source|sessionHash`（`:114-118`）。
- 值：**内容哈希**（`sha256-16`），覆盖 token 四桶 + totalTokens + hostname +
  projectLabel（桶，`:120-132`）或会话的 16 个展示字段（`:134-155`；
  不含 `sessionHash` / `deviceId`——它们在 key 里）。
- 跳过判据：`previous[key] === next[key]`（`:236-243`）——**同一桶 token 增长
  会被识别为新内容并重新上传**（覆盖语义，依赖服务端 upsert，见 §4.2）。
- 重置：清单缺失（`runtime/upload-manifest.ts:46-50`）或损坏（校验器
  `:22-44`、读取容错 `:52-67`）、或 scope 任一字段变化
  （`domain/upload-manifest.ts:79-99`）→ 旧表**视为空**
  （`domain/upload-manifest.ts:227-234`）→ **全量重传**。
- scope 变化后的远端处理（`sync-service.ts:84-104`）：`server_or_api_key` 变化
  → 不删远端；`project_identity` / `snapshot_protocol` 变化 → 先
  `DELETE /api/usage/ingest?deviceId=<old>` 清掉该设备的远端快照再传；
  仅 `device_id` 变化 → 不删。
- `removedBuckets/Sessions`（本地消失的记录）只计数并 `warn`，
  **不做远端删除**（`sync-service.ts:348-362`，日志原文"Remote deletions are
  not supported yet"）。
- `deviceId = randomUUID()`，持久化在 config.json（`config/manager.ts:67-74`）。

### 3.8 同步主流程与 HTTP 客户端

`runSync`（`sync-service.ts:233-555`）步骤序列：

1. 抢同步锁（`sync-service.ts:239-253`；锁文件 `$XDG_RUNTIME_DIR/tokenarena/sync.lock`，
   未设则回退 stateHome；陈旧锁会被回收，`lock.ts:44-54`）。占用则返回
   `{ buckets: 0, sessions: 0, skipped: "locked" }` 且**不抛错**
   （状态文件里记作 `skipped_locked` 枚举，`state.ts:10`）。注意抢锁代码
   位于主 `try`（`:261`）之外，非 `EEXIST` 的锁错误（`lock.ts:77-79`、
   `:104`）会直接抛出——既不写状态、也不经 `finally` 释放路径
   （daemon 侧只打印后继续，`daemon.ts:83`）。
2. `runAllParsers()` 串行解析全部 20 个 parser（`:266`）；空数据短路（`:268-274`）。
3. `new ApiClient(apiUrl || "https://token.poco-ai.com", apiKey)`（`:285-286`），
   `fetchSettings()` 拉服务端设置（`:290-308`）：401 → `auth_error` 终止；
   非 2xx / 字段缺失 → 报错终止。
4. 归一化：`toUploadBuckets` / `toUploadSessions`（`:137-221`）——应用
   projectMode、把 `project` 改名 `projectKey/projectLabel`、注入
   `deviceId` 与 `device.hostname`、`reasoning/cached` 一律 `||0`，
   并按 `source|model|projectKey|bucketStart|deviceId` **二次合并**
   （`:150-166`，注意二次合并时 `totalTokens` 直接相加、不重算）。
5. 清单 diff（`:319-325`）；可选远端清理（`:364-381`）。
6. 分批上传：**桶 100/批、会话 500/批**（`:39-40`），批数取二者较大
   （`:405-409`）；payload 形状 `{schemaVersion:2, device:{deviceId,hostname},
   buckets[], sessions[]}`（`client.ts:79-86`）。
7. 全部成功后写清单 + `markSyncSucceeded`（`:503-507`）；`finally` 释放锁；
   失败按类抛出或 `exit(1)`（`:513-554`）；daemon 模式用 `throws:true`
   （`commands/daemon.ts:70-74`）。

`client.ts` 要点：`POST /api/usage/ingest`（`:78`）、
`GET /api/usage/settings`（`:172`，超时 10s）、
`DELETE /api/usage/ingest?deviceId=`（`:236-237`）；一律
`Authorization: Bearer <api key>`（key 形如 `ta_...`）；写入请求超时 60s；
**重试策略 `MAX_RETRIES = 3`、指数退避 `1000 * 2^attempt`**
（`:12-13`、`:47-68`），但 **401 与所有 4xx 不重试**（`:54-61`）；
`fetchSettings` / `deleteDeviceData` 无重试。请求体按 16 KiB 分片写入并回调进度
（`:145-163`）。计数兼容两代字段：`bucketCount ?? ingested`、
`sessionCount ?? sessions`（`:128-129`）。

### 3.9 时间与路径

- 分桶时间戳：本地时区分钟取整 → `toISOString()`，即**ISO UTC 字符串**
  （`aggregator.ts:8-12`、`:26`）。
- 会话首末消息时间：`toISOString()`（`session-extractor.ts:170-171`）；
  时长/活跃时长是纯毫秒差折算秒，与时区无关。
- **`settings.timezone` 在 CLI 内只做非空校验，没有任何换算用途**
  （`client.ts:208`）——即"采集端写 UTC，展示时区完全由服务端负责"。
- CLI 的**状态/配置层**无平台分支，统一 XDG（parser 层则相反：cline / kiro /
  cursor / roo-code 等含 `process.platform` 分支）。配置 `~/.config/tokenarena/config.json`
  （Windows 也是 `C:\Users\<user>\.config\...`；`TOKEN_ARENA_DEV=1` 时
  `config.dev.json`，`config/manager.ts:20-22`）；状态与清单
  `~/.local/state/tokenarena/{status.json,upload-manifest.json}`
  （`runtime/paths.ts:16-30`）。

### 3.10 本地 dashboard 与上传口径的差异

本地汇总的入口是 **`tokenarena status --show`**（`cli.ts:79-84` →
`status.ts:57-59`）的统计视图（`local-usage-summary.ts:207-259`）：
**无时间范围参数、全量统计**，也不拉 settings、不应用 projectMode
（项目名明文显示）。截断分两层：各榜 `slice(0, 8)` 在 domain 层
（`:186`、`:187`、`:195`），渲染层再截到 6 行
（`local-usage-dashboard.ts:9-10`、`:171`、`:191`）→ 最终实际显示 6 行。
与上传口径的差异：明文项目名 vs `projectKey`、
`raw/disabled` 下上传侧会合并而本地不合并、hostname 被覆盖、`userPromptHours`
被丢弃。

## 4. 服务端：入库、数据模型与再聚合

### 4.1 数据模型（`web/prisma/schema.prisma`）

命名两套风格并存：**用量与几个身份/社交类模型没有 `@@map`**（Postgres 表名 =
模型名，如 `"UsageBucket"`、`"UsageApiKey"`、`"Device"`、`"UsagePreference"`、
`"Follow"`）；带 snake_case `@@map` 的共 11 处——better-auth 四表
（`user:86` / `session:102` / `account:122` / `verification:134`）+ 排行榜/成就表
（`leaderboard_user_day:275`、`leaderboard_snapshot:287`、
`leaderboard_snapshot_entry:310`、`user_achievement:326`、
`achievement_award:346`、`leaderboard_period_result:361`、
`leaderboard_period_entry:384`）。

| 模型 | 表名 | 用途 | 唯一键 |
| --- | --- | --- | --- |
| `UsageBucket` | `UsageBucket` | **用量事实（30 分钟桶）** | `[userId, deviceId, source, model, projectKey, bucketStart]`（:221） |
| `UsageSession` | `UsageSession` | **用量事实（会话）** | `[userId, deviceId, source, sessionHash]`（:251） |
| `UsageApiKey` | `UsageApiKey` | CLI API key（`keyHash`） | `keyHash`（:172） |
| `Device` | `Device` | 设备名册 | `[userId, deviceId]`（:194） |
| `UsagePreference` | `UsagePreference` | 时区 / projectMode / **projectHashSalt** | `userId` |
| `LeaderboardUserDay` | `leaderboard_user_day` | **排行榜日汇总（服务端算的）** | `[userId, statDate]`（:272） |
| `LeaderboardSnapshot`(`Entry`) | `leaderboard_snapshot(_entry)` | 榜单快照 | `period`（:280）/ `[snapshotId,userId]`+`[snapshotId,rank]` |
| `LeaderboardPeriodResult`(`Entry`) | `leaderboard_period_result(_entry)` | 已结算周期与徽章 | `[period, windowStart, windowEnd]`（:359） |
| `UserAchievement` / `AchievementAward` | `user_achievement` / `achievement_award` | 成就状态 / 发放流水 | `@@id([userId, code])`（:324）/ `dedupeKey`（:336） |
| `User` / `Session` / `Account` / `Verification` / `Follow` | `user` / `session` / `account` / `verification` / `Follow` | better-auth 与社交 | — |

关键列语义：

- **token 全部 `BigInt`**（`UsageBucket:207-211`、`UsageSession:238-242`、
  `LeaderboardUserDay:259-263`、快照与周期表同样）。默认值分布：
  `UsageBucket` 只有 `reasoningTokens` 有 `@default(0)`（`:209`），
  其余四列无默认；`UsageSession` 五列全部 `@default(0)`（`:238-242`）。
- `UsageBucket` 有 5 个 `estimated*Usd Float?`（:212-216）、`UsageSession` 有
  1 个 `estimatedCostUsd Float?`（:244）。bucket 侧这 5 列**既无写入方也无
  读取方（恒为 NULL）**（成本在查询期现算，见 §6.4；注意 `queries.ts:67` 的
  `findMany` 无 `select`，Prisma 会把这些列随行取回——只是没人用）；
  session 侧 `estimatedCostUsd` 是 ingest 时算好落库、查询直接读的真列。
- 桶表带 `apiKeyId`（:200）但**唯一键不含它**：同一桶换 key 重传会覆盖行并
  改写 `apiKeyId`；`applyBucketFilters` 支持按 `apiKeyId` 过滤，因此
  "换 key 后历史数据归属会漂移"。另外 `apiKeyId` 无外键约束（`:200` 无
  relation），删除 API key（`api-keys.ts:81` 物理删除）后这批历史行的
  `apiKeyId` 悬空，该维度再也筛不出它们（筛选值来自 `queries.ts:809-813`）。
- 时间列 `bucketStart`（桶起点）/ `firstMessageAt`、`lastMessageAt`（会话）、
  `statDate`（排行榜日，Asia/Shanghai 日界，见 §7.1）。

### 4.2 摄入流程与幂等

路由 `web/app/api/usage/ingest/route.ts`：Bearer token → `findUsageApiKeyByRaw`
（`api-keys.ts:88-104`：要求 `ta_` 前缀、按 **sha256 hex 无盐** 比对
`keyHash`、`disabled` 直接 null）→ 401（`route.ts:30-32`）；zod 校验
（`contracts.ts:103-111`：`schemaVersion` 必须是字面量 2、数组**无长度上限**）
→ 400 `INVALID_PAYLOAD`（`route.ts:34-44`）。

写入 `ingestUsagePayload`（`ingest.ts:360-419`）：

1. 单个 `prisma.$transaction`（:364）内：Device upsert、key `lastUsedAt` 更新
   （:372-377）、查已存在 session 的日期集合、
   `Promise.all` **逐桶/逐会话 upsert**（`:197-228`、`:236-291`，
   **无分批、无并发上限**）→ 得到受影响日期。
2. **幂等 = upsert 覆盖，不是累加**：幂等键即表的唯一约束
   `[userId, deviceId, source, model, projectKey, bucketStart]`
   （与 API key **无关**，`apiKeyId` 只是被覆盖的普通列，`:213`），
   所以采集端的"全量重扫 + 内容增量上传"不会重复计数；代价是
   **旧值被覆盖、不会部分合并**。
3. 事务内重算受影响日的 `leaderboard_user_day`（见 §4.3），并全量失效榜单快照
   ——**仅当确有受影响日期时执行**（`ingest.ts:401-407`，空载荷不触发）。
4. 返回 `{ ok, bucketCount, sessionCount, deviceId }`（:409-414）；
   事务外同步成就（:417）。未见 try/catch——异常交给框架 500；
   事务也未传 `transactionOptions`（走 Prisma 默认超时上限）。

**会话 token 归一化（重要）**：服务端对会话**不是"信任上报值"**——
`normalizeSessionUsage`（`ingest.ts:83-149`）把 `modelUsages` 求和后
**覆盖顶层四桶**（`:137-149`）；只有会话没有 `modelUsages` 时才回退用顶层字段
（`:152-167`），`totalTokens` 缺失时按四桶之和补（`:162-164`）。
update 时用 `...(sessionUsageWrite ?? {})`（`:263`）——归一化为 null 时不覆盖
既有 token/成本列；**bucket 侧则始终整体覆盖**（`:212-215`）。

### 4.3 服务端再聚合：三层，全部与排行榜有关

1. **桶/会话 → 日汇总**：`recomputeLeaderboardUserDays`
   （`leaderboard/aggregates.ts:113-233`）在 ingest 事务内执行——`findMany`
   拉原始行后用 **JS 累加**（:170-231）写 `leaderboard_user_day`，
   不是 SQL 聚合。
2. **日汇总 → 榜单快照**：`leaderboard/queries.ts:488-520` 用
   `prisma.leaderboardUserDay.groupBy`（**SQL 聚合**）产出 Snapshot 条目，
   有 5 分钟 TTL（`queries.ts:24`、:473-484，窗口切换会优先于 TTL 触发重建）；
   ingest 时 `deleteMany` 全清失效（`aggregates.ts:107-111`）。
   **该 TTL 只作用于全局 token 榜**：cost 榜（`:646-761`）与 following
   token 榜（`:908-950`，直接查 `leaderboard_user_day`）都不读快照。
3. **周期结算**：`finalizePendingLeaderboardPeriods`（`finalize.ts:89-304`，
   其中 `:121-156` 是 `groupBy`/`take: 100` 查询段）→ 写
   `leaderboard_period_result/entry`（**100 条**）并给 `rank <= 50` 发徽章
   （`finalize.ts:67`、`:186-301`）；
   **懒触发**（`achievements/queries.ts:637,649,674,702` + 页面侧
   `leaderboard/queries.ts:992`），没有 cron。

**仪表盘侧不参与这三层**：`web/lib/usage/queries.ts` 内**没有 Prisma 的
`groupBy` / `$queryRaw`**（该文件里出现的 `groupBy` 只是 import 的日历工具函数
`groupByHourOrDay`；真正的 SQL 聚合只出现在 `leaderboard/rank.ts:22`、
`social/badges.ts:336` 等），所有指标在 Node 内存里对全量行现算（见 §5.3）。

### 4.4 BigInt 精度处理

封装在 `web/lib/token-counts.ts`（**不在 `lib/usage/` 下**）：

- 写入路径 `tokenCountToBigInt`（:4-6）：`bigint` 直通，
  否则 `BigInt(Math.trunc(value ?? 0))`。
- 读取路径 `tokenCountToNumber`（:8-22）：超出 `Number.MAX_SAFE_INTEGER`
  或低于 `MIN_SAFE_INTEGER` 时**饱和钳制**（不是抛错），number 分支
  `value ?? 0` 兜底。
- 使用点覆盖 ingest、usage/queries、leaderboard/*、finalize、achievements、
  social 等全部读写 token 的地方。

### 4.5 API key、settings 与 preferences

- API key 生成：`ta_` + `randomBytes(24).hex`（`api-keys.ts:13-21`），
  展示用前缀 `raw.slice(0,11)`（:9-11），库存 sha256 hex（:5-7）；
  创建接口**只返回一次明文**（`app/api/usage/keys/route.ts:25-38`）；
  PATCH 改名/停用（`api-keys.ts:47-70`）、DELETE 物理删除（:72-86）。
  **每用户 key 数量上限：未找到佐证**（无 count 校验）。
- `GET /api/usage/settings`（Bearer **API key**，`settings/route.ts:11-31`）
  返回 `{ schemaVersion: 2, projectMode, projectHashSalt, timezone }`，
  取值自 `UsagePreference`；`projectHashSalt` 是**服务端惰性生成**的
  `randomBytes(16).hex`（`preferences.ts:11-13`、:24-55），CLI 只消费。
- Web 侧 `GET/PATCH /api/usage/preferences` 走登录态、**不返回 salt**
  （`app/api/usage/preferences/route.ts:16-53`）。
- 仪表盘解析日期范围用的时区来自 `preference.timezone`
  （`dashboard.server.ts:50-56`；schema 默认 `"UTC"`）。

### 4.6 保留与清理

- **无 cron、无 retention/裁剪**：全仓 cron 只出现在 GitHub Actions
  （codeql.yml、feishu-bot.yml），Web 无 vercel.json/cron 路由。
- 唯一的用户级删除是 `DELETE /api/usage/ingest?deviceId=`（`ingest.ts:294-358`）
  ——按 deviceId **整机删除**该设备的 bucket/session（Device 名册行保留），
  由采集端在 scope 变化时调用（§3.7）；删除后同样按受影响日重算
  `leaderboard_user_day`、清空榜单快照（`:339-345`），并在确有删除时同步成就
  （`:353-355`）。
- 排行榜侧的删除只有"失效/重建"（`aggregates.ts:110,191-196`、
  `queries.ts:562-566`、`finalize.ts:208-212`）。
- 摄入契约的 token 字段只有 `int().nonnegative()`（`contracts.ts:65-69`、
  `:93-97`）——**无上界 / safe-integer 校验**：超过 2^53 的上报会被
  `tokenCountToNumber` 静默钳制成 `MAX_SAFE_INTEGER` 参与聚合（§4.4）。

## 5. 服务端：查询聚合与指标口径

### 5.1 时间口径（`web/lib/usage/date-range.ts`）

预设只有四个：`1d / 7d / 30d / custom`（`types.ts:9`；缺省 `7d`，
`date-range.ts:246`），解析在 `resolveDashboardRange`（`date-range.ts:243-286`）：

| 预设 | 区间 | 粒度 |
| --- | --- | --- |
| `1d` | 统计时区**当天 0 点** → `now` | hour |
| `7d` | 今天 0 点 − 6 天 → `now` | day |
| `30d` | 今天 0 点 − 29 天 → `now` | day |
| `custom` | `from` 当日起点 → `to` 当日**末毫秒**（`+1 天 −1ms`，`:201-220`） | 跨度 ≤ 36h 为 hour，否则 day（`:256-257`） |

（`+1 天 −1ms` 只在输入是纯日期 `YYYY-MM-DD` 时生效；传 ISO 时刻会被
`new Date(value)` 原样解析，`:236-240`。UI 只会发纯日期。）

三点要义：

1. **区间右端是 `now` 而不是"今天 24:00"**——1d/7d/30d 都是"截至此刻"，
   不是完整自然日。
2. **没有周/月预设**；weekday 只作为热力图维度（`:67-76`）。
3. 时区来自用户偏好 `preference.timezone`（`dashboard.server.ts:50-56`），
   请求参数**不带时区**（`contracts.ts:113-130`）；换算靠
   `Intl.DateTimeFormat` + "**最多三轮、通常 1–2 轮**迭代求 offset"
   （`:92-122`，收敛即 `break`）来兜住 DST，
   加日/加小时在**墙钟域**做（`:124-155`），但**无 DST 定向测试**。
   day 键（`YYYY-MM-DD` 或 `YYYY-MM-DD HH:00`）按该时区用 `en-CA` 格式化后
   拼装（`:298-306`），归属判据是**桶起点那一刻**落在哪一格。

`getPreviousRange`（`:288-296`）取等长紧邻前段用于环比；`listRangeBuckets`
（`:308-330`）把区间展开成连续格子（**无数据的格子补零**）：day 粒度下点数
固定（7d 恒 7 点、30d 恒 30 点），1d 的 hour 粒度则是"当天 0 点 → 当前小时"
共 N+1 格。

### 5.2 九个查询函数与指标定义（`web/lib/usage/queries.ts`）

| 函数 | 行 | 维度 / 用途 |
| --- | --- | --- |
| `getOverviewMetrics` | :237 | 区间总指标 + 环比（bucket + session 两源拼装） |
| `getTokenTrend` | :257 | day/hour 趋势（token 四桶 + **现算成本** + 会话总时长） |
| `getActivityTrend` | :314 | day/hour 活跃趋势（纯 session 源） |
| `getHourlyActivityHeatmap` | :382 | weekday × hour 共 168 格 |
| `getBreakdowns` | :497 | 四张分解卡：device / source / model / projectKey |
| `getPricingSummaryAndRows` | :705 | 按模型的价格命中、费率、成本与覆盖率 |
| `getSessionRows` | :729 | 每会话一行明细 |
| `getFilterOptions` | :805 | 筛选下拉数据 |
| `getLastSyncedAt` | :862 | 三表（bucket/session/device）最大时间戳 |

指标口径（与 §3 的采集端定义对应）：

- **token**：直接读库里的 BigInt 列，查询不重算；`totalTokens` 是采集端
  约定的四桶之和。会话级 `sessions` = 区间内 session 行数，
  **判据是 `firstMessageAt` 落在区间**（`:100-103`、`:167`）——
  跨区间的会话归属由"开始时间"决定。
- **时长**：`activeSeconds` / `totalSeconds` 分别是会话 `activeSeconds`、
  `durationSeconds` 列的求和（`:170-171`），语义回到 §3.5。
- **messages / userMessages**：列求和。
- **无任何平均值指标、无"请求数"指标**；唯一的比率是分解卡的
  `share`（`:436`，前端按所选指标重算）与价格覆盖率
  `coverage = pricedTokens / totalTokens`（`:681-702`）。
- **成本**：两套来源并存——bucket 侧**查询期现算**（`:113-136`），
  session 侧**读 ingest 时落库的列**（`:764`、`:795`，值在
  `ingest.ts:97-111`/`:146-148` 按 `modelUsages` 计算，
  无已定价模型时为 `null`）。详见 §6.4。

排序规则：bucket `bucketStart asc`（:78）、session `firstMessageAt asc`（:107）、
会话明细 `firstMessageAt/lastMessageAt desc`（:746）、分解行
`totalTokens desc + share`（:429-440）、模型行 cost→tokens→名称（:653-666）。

### 5.3 数据加载方式：全量行 + 内存聚合

**服务端仪表盘没有 Prisma `groupBy` / `$queryRaw`**：`loadBuckets`（:62-89）与
`loadSessions`（:91-109）是裸 `findMany`（时间闭区间查询
`gte: from, lte: to`），**无 `take`/分页/游标**，全部行拉进 Node 进程后
在 JS 里汇总（`:153-177` 的 `summarizeTotals`、`:284-298` 的按时间格子累加、
`:617-651` 的模型成本汇总）。这意味着查询成本与"用户区间内的事实行数"
线性相关；服务端也没有任何 limit 保护（grep `take:`/`skip:`/`cursor:`
只命中 social 与 leaderboard 模块）。

实测到的两个额外负担：一次页面渲染会**重复全量查询**——`loadBuckets` 在该文件
被调用 7 次（`:245/247`、`:264`、`:389`、`:504`、`:716/717`）、session
`findMany` 7 次（`:246/248`、`:265`、`:319`、`:390`、`:505`、`:735`）、
`device.findMany` 3 次（`:506`、`:769`、`:814`）；且 `getFilterOptions` 再拉一次
**无时间条件**的全量桶（`:819-827`）。另外 `getActivityTrend` 的结果
（`dashboard.server.ts:77`）在 **usage 页面组件里未被消费**
（`page.tsx:206-209` 只传 trendData/heatmapData；`/api/usage/dashboard`
路由仍会返回完整 dashboard）——页面渲染白跑一次 session 全量查询。
索引只有 `[userId, bucketStart]` 与 `[userId, firstMessageAt]`
（`schema.prisma:222`、`:252`）。

还有一条与本页无关但同类的问题路径：`social/badges.ts:323-345` 的徽章
生成对 UsageBucket / UsageSession 做**无时间窗口**的全量读取。

前端侧的裁剪：会话明细 `PAGE_SIZE = 20`（`sessions-section.tsx:36`、:83-91）、
分解卡 `maxVisibleRows = 5` + `__others__` 合并行
（`breakdown-grid.tsx:76`、:123-132）。

### 5.4 过滤与一处已知口径错配

- 支持的过滤维度：`apiKeyId / deviceId / source / model / projectKey`
  （`types.ts:22-28`），桶侧全支持（`:34-46`）。
- **已知错配（重要）**：`applySessionFilters` 显式**排除 `model`**
  （`:48-60`，注释说明"UsageSession 没有 model 字段"）。后果：
  当用户按某个模型筛选时，**token 与成本按模型过滤，但会话数 / 消息数 /
  时长仍统计全模型会话**——两类指标口径不一致。
- `getFilterOptions`：`source/model/project` 由用户**全部** bucket
  `findMany` 后在 JS 里用 `Map` 去重（`:819-842`，不是 Prisma `distinct`），
  **不随所选时间区间裁剪**；model 是原始串、未做归一（`gpt-4o` 与
  `openai/gpt-4o` 会各占一行）。排序：筛选项按 label 升序（`:850-858`）、
  apiKeys 按 `createdAt desc`（`:811`）、devices 按 `lastSeenAt desc`（`:816`）。
- 设备展示名：同名设备自动追加 deviceId 前 8 位区分（测试
  `queries.test.ts:86-135`）。

### 5.5 空态与未知模型

- 整页空态判据 `totalTokens > 0 || sessions > 0`（`app/[locale]/usage/page.tsx:111-124`），
  空态显示 `npm install -g @poco-ai/tokenarena` + `tokenarena init` 引导
  （`usage-empty-guide.ts:2-4`）。
- 未知模型：`estimatedCostUsd = null`、排序按 −1 垫底、**不计成本但计入
  覆盖率分母**（`:624-626`、`:654-655`、`:674`、`:681-702`）。价格弹窗的
  文案判据是 **provider 徽章是否为空**（`pricing-match-dialog.tsx:35-43`、
  :113-117）：provider 由独立的前缀规则解析（`queries.ts:618-620`），早于
  模型匹配——"前缀能认出厂商、但目录里没有该模型"的行只显示 provider 徽章
  与 «—»，不出现 "No official match"。区间内无已定价模型时显示
  "No priced usage in this range."（`messages/en.json:266,297,349`）。
- 未知项目兜底在采集端（`project-identity.ts:9` 的 `unknown` /
  `Unknown Project`）；`web/lib/usage/project.ts:3-17` 的 `formatProjectLabel`
  无生产引用（仅测试）。
- `lastSyncedAt = null` 时显示 "No sync yet"（`page.tsx:125-133`）。

## 6. 金额计算（USD）

**结论先行**：TokenArena 的金额是**服务端估值**，只在展示/排行/成就路径上
现算；价目目录取自 models.dev，匹配规则是"归一化 + 候选串按顺序首中即用"；
公式是**线性五折（四桶）**，`reasoning` 单价缺失时回退 `output` 单价、
`cache_read` 缺失按 0；未知模型不给金额（`null`/0）但计入覆盖率分母。

### 6.1 价目目录（`web/lib/pricing/catalog.ts`）

- 来源：`https://models.dev/api.json`（:4）；Next `unstable_cache` 缓存
  **12 小时**（:5、:178），缓存键 `pricing-catalog`；fetch 用
  `cache: "no-store"` 绕过 Next 的 fetch 缓存，改为缓存**序列化后的精简快照**
  （:156-180，注释说明上游 payload 过大会超 Next 数据缓存条目上限）。
- 失败语义：fetch 抛错/非 2xx → 返回 `null` → 上层所有 resolve 得 `null`
  → 金额为 0（bucket 侧）或 `null`（明细侧），**不报错**。
- 结构：`Map<providerId, { name, modelsByLower: Map<归一化模型 id, {id,name,cost}> }>`；
  归一化 = `trim().toLowerCase()`（:61-63），**不做 `-`/`.`/`_` 折叠**。
  为绕开 Next 数据缓存条目上限，上游 payload 先被压成元组数组
  （`catalog.ts:46-59`）：`[lookupKey, modelId, modelName|null, cost|null]`
  与 `[providerId, providerName|null, models[]]`，且 `name === id` 时压缩为
  `null`（`:101`、`:109`）。
- `PricingCost` 五个单价键：`input / output / reasoning / cache_read /
  cache_write`（:8-14）。**`cache_write` 全仓只有类型声明、无任何消费方**
  （见 6.5）。

### 6.2 模型匹配（`web/lib/pricing/resolve.ts`）

两段式：先定 provider，再在该 provider 的模型表里查候选。

1. **候选串构造** `buildModelLookupCandidates`（:106-135），按顺序去重：
   归一化全名 → 若含 `/` 追加"最后一段" → 若含 `:` 追加"`:` 前变体"
   （对全名与末段各做一次）。
   例：`moonshotai/kimi-k2.5:latest` → `["moonshotai/kimi-k2.5:latest",
   "kimi-k2.5:latest", "moonshotai/kimi-k2.5", "kimi-k2.5"]`。
2. **provider 推断** `OFFICIAL_PRICING_PROVIDER_RULES`（:16-79）：
   前缀匹配的静态规则表，共 11 家——`anthropic`(`claude-`)、
   `openai`(`gpt-`/`gpt-oss-`/`o1`/`o3`/`o4`/`text-embedding-`/
   `text-moderation-`/`omni-moderation-`)、`google`(`gemini-`)、
   `deepseek`(`deepseek-`)、`moonshotai`(`kimi-`)、`zai`(`glm-`)、
   `alibaba`(`qwen`/`qwq-`/`qvq-`)、`minimax`(`minimax-`)、`xai`(`grok-`)、
   `mistral`(6 个前缀)、`cohere`(`command-`/`embed-`)。
   命中即取该 provider（`:140-146` 双层循环：外层规则表、内层候选串；
   这些前缀分属 11 个互不重叠的 provider——openai 内部 `gpt-` 包含
   `gpt-oss-`，但同属一家——所以"规则表顺序"实际不会改变结果）。
3. **取模型** `findCatalogModel`（:175-196）：按候选顺序查
   `provider.modelsByLower`，**首个命中即返回**。可用的回退**只有**上面两种
   结构拆分（`/` 末段、`:` 变体）——没有模糊匹配、没有日期后缀/前缀归一，
   所以 `claude-sonnet-4-20260101` 这类带日期串的名字在目录里没有同名条目时
   会直接匹配失败（`resolve.test.ts:206-216` 覆盖 null catalog 与"厂商命中
   但模型缺失"两种失败）。

### 6.3 计费公式（`estimateCostUsd`，:229-254）

```ts
inputUsd     = inputTokens     / 1_000_000 * (cost.input       ?? 0)
outputUsd    = outputTokens    / 1_000_000 * (cost.output      ?? 0)
reasoningUsd = reasoningTokens / 1_000_000 * (cost.reasoning ?? cost.output ?? 0)
cacheUsd     = cachedTokens    / 1_000_000 * (cost.cache_read  ?? 0)
totalUsd     = inputUsd + outputUsd + reasoningUsd + cacheUsd
```

- 单位：**美元 / 每 100 万 token**；四桶线性相加，无阶梯、无峰谷、
  无最低消费。
- **回退规则**：`reasoning` 单价缺失 → 用 `output` 单价；
  `cache_read` 缺失 → 按 0 计（**缓存读被当免费**，会系统性低估）。
- `cost` 为 `null`/`undefined` → 函数返回 `null`，调用点自行兜底
  （bucket 路径 `?? 0`、明细路径 `?? null`）。
- 细节：展示层的 `reasoningRateUsdPerMillion` 取
  `cost.reasoning ?? null`（`queries.ts:644`）——即"按 output 价计费"的
  reasoning 在**费率列上仍显示 null**，与成本计算结果不完全同源。

### 6.4 六个生产调用点与两套口径

全仓 `estimateCostUsd` 的生产调用点共**六处**，全部复用同一对
`resolveOfficialPricingMatch + estimateCostUsd`：

| 调用点 | 行 | 计算时机 | 结果去向 |
| --- | --- | --- | --- |
| `usage/queries.ts`（桶现算） | :113-136 | **查询期现算** | 总览/趋势/分解 |
| `usage/queries.ts`（模型价格行） | :618-651 | 查询期现算 | 定价表/覆盖率 |
| `usage/ingest.ts`（会话） | :97-111 | **写入期算好落库** | `UsageSession.estimatedCostUsd` 列 |
| `leaderboard/queries.ts`（封装 `estimateGroupedRowCostUsd`） | :205-221（内部 :209-210） | 榜单生成/水合时现算 | 榜单成本列 |
| `achievements/queries.ts` | :92-93 | 现算 | 成就判定 |
| `social/badges.ts` | :273-274 | 现算 | 徽章 SVG |

因此同一"金额"存在**两套取数路径**：

- **bucket 侧**：`UsageBucket` 表里那 5 个 `estimated*Usd` 列**既无写入方也无
  读取方（恒为 NULL）**（ingest 不写、queries 不消费），所有桶成本都在查询时
  现算（`queries.ts:113-136`）。改价目表/改公式会**同时**改变历史区间的展示成本。
- **session 侧**：`UsageSession.estimatedCostUsd` 在
  `ingest.ts:97-111`/`:146-148` 按 `modelUsages` 逐模型计算后落库，
  查询直接读列（`queries.ts:764`、`:795`）；无已定价模型时为 `null`。
  这个值**不会因价目表更新而重算**（除非该会话被重传）。

补一个容易误读的点：**榜单页展示的成本列同样来自查询期实时计算**——快照表
没有成本列（`schema.prisma:290-311`），生成时把 `estimatedCostUsd` 写成 `0`
（`leaderboard/queries.ts:534`、:636、:964），再由 `hydrateEntries`（:401-471）
发现"全为 0"时调 `getEstimatedCostMapForUsers` 实时 `groupBy` 补算
（:411-413、:424-426、:451-454）。副作用：真实成本恰为 0 的用户会被反复
判定为"未定价"而多走一次实时路径。**token 榜也一样**（`:534`、`:636`、
`:964`、`:815` 处把成本赋值 0；`:855`、`:973` 是水合调用行）——即打开任何
榜单页都会触发一次实时成本 `groupBy`；
`all_time` 窗口下该 `groupBy` 无时间条件（`buildBucketWindowWhere` 返回 `{}`，
`:97-108`），会扫描该用户全部历史 bucket 行（`:304-338`）。
另外 `achievements/queries.ts:88-90` 在目录不可用时是**显式早退返回 0**，
而 `usage/queries.ts:135` / `leaderboard/queries.ts:220` /
`social/badges.ts:284` 走 `estimate?.totalUsd ?? 0`；`leaderboard/queries.ts:217`
与 `queries.ts:132` 用 `match?.cost`，`queries.ts:635` 用 `match.cost`
（依赖其前的 `if (!match) continue` 早退）——各调用点的空值契约并不统一。

### 6.5 覆盖率与未知模型

`summarizePricingRows`（`queries.ts:669-702`）给出价格覆盖率：

- `coverage = pricedTokens / totalTokens`、`pricedModels / totalModels`
  （分母含未知模型，分子只含成功匹配的 token）。
- 未知模型：`estimatedCostUsd = null`、排序垫底（`:653-666`）、
  不计成本但计入分母 → 面板会明确展示"有多少 token 没算进金额"。

与 TokenScope 的对照点：TokenArena **没有**三态单价（数字/显式 0/未知）的
严格区分——`cache_read ?? 0` 把"未知"与"免费"混同；也**没有**同价键多渠道
候选与"取最高费用"的策略，一个模型只认一个 provider 条目的首个命中。

## 7. 排行榜与成就

### 7.1 周期定义：硬编码 Asia/Shanghai

`web/lib/leaderboard/date.ts` 用**固定 +8 小时偏移**（`SHANGHAI_OFFSET_MS = 8h`，
:5）而不是 IANA 时区换算，周期四种（`:101-139`）：

| 周期 | 窗口 | 备注 |
| --- | --- | --- |
| `day` | 上海日 0 点 → 次日 0 点 | |
| `week` | 上海**周一起**（周日的 `deltaToMonday = -6`，:63-70） | |
| `month` | 自然月（跨年判断在 :134-136） | |
| `all_time` | `start/end` 均为 `null` | |

**排行榜的时区与仪表盘互相独立**：这里硬编码上海（`LeaderboardPeriodResult.timezone`
默认值也是 `"Asia/Shanghai"`，`schema.prisma:352`），不跟随用户偏好时区——
两个页面在跨时区用户眼里"日"的边界可以不同。

结算时机（`:141-185`）：当期开始 **4 小时后**才允许结算**上一个**窗口
（`finalizeAt = currentWindow.start + 4h`，`:155`）；上一窗口的构造按周期分支
——day 回退 1 天、week 回退 7 天、month 回退到上一个自然月（`:161-184`）；
**`all_time` 直接返回 `null`，永不进入结算**（`:145-147`）。
同文件另有导出常量 `SHANGHAI_TIMEZONE = "Asia/Shanghai"`（`:7`），
生产引用仅一处且不参与计算：`components/social/leaderboard-window-badge.tsx:25`
只把它当参数传给 `formatLeaderboardWindowLabel`，而该参数在
`date.ts:187-211` 内除恒假的 `timezone === null` 判断外不参与换算；
写库用的是字面量（`finalize.ts:197,201` 与 `schema.prisma:352` 默认值）
——"硬编码上海"的运行时证据始终是那个 `+8h` 偏移常量。

### 7.2 数据流：日汇总 → 快照 → 三条读取路径

1. **日汇总**：`leaderboard_user_day`，由 ingest 事务内的
   `recomputeLeaderboardUserDays` 用 findMany + JS 累加写入
   （`aggregates.ts:113-233`；**无 SQL 聚合**）。
2. **快照**：`leaderboardSnapshot` 用 `prisma.leaderboardUserDay.groupBy`
   （SQL 聚合）生成，**TTL 5 分钟**缓存（`queries.ts:22-24`、:486-644），
   每次 ingest 都 `deleteMany` 全清（`aggregates.ts:107-111`）。
   快照**存 100 条、读 50 条**。
3. 读取有三条路径并列（不是同一份数据）：
   - 全局 **token 榜** → 快照表；
   - **cost 榜** → **实时** `usageBucket.groupBy`（`queries.ts:646-761`）；
   - **following（关注）token 榜** → 直接查 `leaderboard_user_day`
     （:908 起）。
   → 因此"关注榜"与"全局榜"的时效性和口径来源都不同。

排行规则分两条路径，**并不完全一致**：

- 实时路径 `rankLeaderboardSummaries`（`queries.ts:289-302`）：按指标降序 →
  `totalTokens` 降序 → `userId` 升序，**不共享名次**（并列不占同位）。
- 快照 / finalize 路径（`:522-542`、`:952-972`、`finalize.ts:158-177`）：
  走 SQL `orderBy` + `take`，名次在**过滤前**就用 `index + 1` 定死，随后
  `if (totalTokens > 0)` 逐行跳过——被跳过的行会造成**名次断层**，
  而不是"门槛过滤后重排"。

门槛：cost 榜 `cost > 0`（判据 `hasMetricValue`，`:257-259`）、token 相关 `totalTokens > 0`。

### 7.3 结算与徽章

- `finalizePendingLeaderboardPeriods` **懒触发**：只在页面访问
  （`queries.ts:992`）与 ingest 路径（`achievements/queries.ts:637,649,674,702`）
  被调用，**没有 cron**（与 §4.6 一致）。
- 幂等：靠 `badgesIssuedAt` 早退（`finalize.ts:103-119`）。
- 发徽章用 `dedupeKey` 唯一约束去重：`finalize.ts:232-247` 先按 dedupeKey
  `findMany` 过滤，再 `createMany`（`:250`，**未加 `skipDuplicates`**）；
  对照：成就评估侧 `achievements/queries.ts:573-586` 的同名调用**有**
  `skipDuplicates: true`。冲突窗口只剩"并发未提交期"，风险低但存在；
  同类写法还有 `leaderboardPeriodEntry.createMany`（`finalize.ts:215-228`，
  由前置 `deleteMany` 兜住；但交错并发下仍可能撞
  `@@unique([resultId, userId])`，`schema.prisma:380-381`——窗口比上面更宽）。
- 周期结果写入 `leaderboard_period_result/entry`，并给 top50 发徽章
  （`finalize.ts:186-301`）。

### 7.4 可见性与榜单/看板的口径差异

- 排行榜**唯一的可见性过滤**是 `publicProfileEnabled = true`
  （`queries.ts:495`、:661；`finalize.ts:128`），其 schema 默认值为 `true`
  （`schema.prisma:144`）——即**默认公开**；用户在设置里把它改成
  `false` 时，`preferences.ts:89-94` 会立刻失效榜单快照，使变更当轮生效。
- 榜单目录**不按 source / apiKey / 设备 / 项目过滤**：一个用户的所有工具
  数据合并参赛。看板则可以按这些维度筛选、可自定义区间、并展示未匹配价目
  的清单——两者不是同一套口径。
- following 榜对"自己"放宽：允许未公开的自己仍可见——token 路径在
  `:915-928`，**cost 路径另有一份同义实现**在 `:720-733`（`userId = viewer`
  OR `publicProfileEnabled = true`）；改一处不改另一处会造成口径分裂。

### 7.5 成就体系

- **榜类成就 6 个码**（`achievements/catalog.ts:390-431`；分值由
  `tierPoints`（`:7-12`：bronze 10 / silver 20 / gold 40 / special 60）注入，
  六码实为 10/20/20/40/60/60）：day/week/month 各一个（top50）、
  all_time 三个（top100 / top10 / 第一）。
  名次**恒按 total tokens**（`achievements/queries.ts:526`、`rank.ts:14-46`，
  JOIN `publicProfileEnabled`、`SUM(...) > 0`）——与榜单页的 cost 指标
  **不对齐**。
- 发放通道两分：day/week/month 属 external，**只由 finalize 发**
  （`records.ts:57-60`、:144）；all_time 三码由评估器 recross 发
  （`records.ts:53-56`）。
- 其他成就取数：token / cost / 会话 / 时长 / 模型多样性时间线 +
  累计阈值（`achievements/queries.ts:187-432`、`evaluate.ts:155-191`）；
  总分 = `Σ points × awardCount`（`evaluate.ts:739`）。

## 8. 关键语义速查表

| 问题 | TokenArena 的答案 | 佐证 |
| --- | --- | --- |
| 事实存哪 | 服务端 Postgres 两张表：`UsageBucket`（30 分钟桶）+ `UsageSession` | `schema.prisma:197-253` |
| 本机留什么 | 只留上传清单 + 配置（**不存用量事实**） | `upload-manifest.ts:32-38` |
| 采集粒度 | 30 分钟对齐（本地时区分钟取整） | `aggregator.ts:8-12` |
| 重复上传会重复计数吗 | 不会：桶/会话按唯一键 **upsert 覆盖**；清单按内容哈希跳过未变化项 | `ingest.ts:360-419`、`upload-manifest.ts:236-243` |
| 桶的幂等键 | `userId+deviceId+source+model+projectKey+bucketStart` | `schema.prisma:221` |
| token 总量定义 | 四桶之和（input+output+reasoning+cached），上传方约定 | `aggregator.ts:50-54` |
| 会话归属区间 | 按 `firstMessageAt`（会话开始）落区间 | `queries.ts:100-103` |
| 活跃时长定义 | Σ 各轮"首个 AI 响应→最后 AI 响应" | `session-extractor.ts:61-67` |
| 时区（看板） | 用户偏好 IANA；区间右端是 `now` | `date-range.ts:243-286` |
| 时区（排行榜） | 硬编码 Asia/Shanghai；4 小时结算缓冲 | `leaderboard/date.ts:5`、:155 |
| 金额算在哪 | 服务端查询期现算（bucket）／ingest 落库（session，两套并存） | `queries.ts:113-136`、`ingest.ts:97-111` |
| 金额公式 | 四桶 × 单价 / 1e6；reasoning 回退 output、cache_read 缺失按 0 | `resolve.ts:229-254` |
| 价目来源 | `models.dev/api.json`，12h 缓存，失败静默降级 | `catalog.ts:4-6`、:156-180 |
| 未知模型 | 不给金额（null/0），计入覆盖率分母 | `queries.ts:669-702` |
| 项目隐私 | 默认 `hashed`：HMAC-SHA256(salt, 项目名) 前 16 位 | `project-identity.ts:16-19`、`schema.prisma:143` |
| 采集端时区用途 | 无（只写 UTC ISO 串，唯一校验非空） | `client.ts:208` |
| 有请求数指标吗 | 没有；只有 token / 会话 / 消息 / 时长 | `types.ts:30-41` |
| 服务端有 SQL 聚合吗 | 看板无（全量行 + 内存聚合）；排行榜有 `groupBy` | `queries.ts:62-109`、`leaderboard/queries.ts:488-520` |
| 数据保留策略 | 无（无 cron / 无 TTL），只有按设备手工删除 | `ingest.ts:294-358` |

## 9. 本仓视角：与 TokenScope 的差异要点

只列口径级差异（不评优劣）：

1. **事实归属**：TokenArena 事实在服务端、本机不留库；TokenScope 反过来
   （`~/.tokenscope/history.db` 是本机事实源）。因此 TokenArena 的"多次扫描
   不重复"靠网络侧 upsert 兜底，TokenScope 靠本地指纹。
2. **聚合时机**：TokenArena 把"逐条 → 30 分钟桶"放在采集端并**丢弃逐条事件**；
   TokenScope 保留逐请求明细（`UsageEvent`）后再聚合。
3. **模型身份**：TokenArena 不折叠模型变体（`gpt-4o` / `openai/gpt-4o` 是两个
   筛选值），没有等价键与展示名分离；TokenScope 有 `Group.key/label`。
4. **计价**：TokenArena 单源（models.dev）、线性公式、`cache_read ?? 0`
   把未知与免费混同、渠道不区分；TokenScope 三来源独立候选 + 三态单价 +
   同价键多渠道取最高。
5. **时间口径**：TokenArena 的看板与排行榜各有一套时区（用户偏好 vs 硬编码
   上海）；TokenScope 全局单一时区权威（`settings.toml` 的 `timezone`）。
6. **隐私三态**：TokenArena 的 `disabled` 仍会上传 token，只抹项目名；
   没有"来源级禁用"概念。

## 10. 源码索引（按职责）

| 职责 | 文件 |
| --- | --- |
| CLI 适配器契约 / 注册 | `cli/src/parsers/types.ts`、`registry.ts` |
| CLI 20 个适配器 | `cli/src/parsers/*.ts` |
| 统一事件类型 | `cli/src/domain/types.ts` |
| 桶聚合 / 会话提取 | `cli/src/domain/aggregator.ts`、`session-extractor.ts` |
| 项目身份三态 | `cli/src/domain/project-identity.ts` |
| 增量清单 | `cli/src/domain/upload-manifest.ts`、`infrastructure/runtime/upload-manifest.ts` |
| 同步编排 / HTTP | `cli/src/services/sync-service.ts`、`infrastructure/api/client.ts` |
| 本地 dashboard | `cli/src/domain/local-usage-summary.ts`、`infrastructure/ui/local-usage-dashboard.ts` |
| 服务端摄入 | `web/lib/usage/ingest.ts`、`web/app/api/usage/ingest/route.ts` |
| 服务端 schema | `web/prisma/schema.prisma`、`web/prisma/migrations/**` |
| 看板查询 | `web/lib/usage/queries.ts`、`date-range.ts`、`contracts.ts` |
| 计价 | `web/lib/pricing/catalog.ts`、`resolve.ts` |
| 排行榜 | `web/lib/leaderboard/{queries,aggregates,date,finalize,rank}.ts` |
| 成就 / 徽章 | `web/lib/achievements/*`、`web/lib/social/badges.ts` |

## 11. 未决与存疑

以下事项均已在源码中定位到对应实现，按「**已证实的行为**」与「**未证实的
推断**」两类列出（本节经逐条回源复核；标注"源码无法判定"的项依赖上游日志
语义或 Next 运行时行为，静态核对到此为止）。

### 11.1 已证实的行为与风险

- **无流式解析**：文本类 parser 都是整文件读内存（`fs/utils.ts:48-54` 的
  `readFileSafe`；另有 9 处自实现同款读取——`gemini-cli.ts:97`、
  `copilot-cli.ts:107`、`openclaw.ts:125`、`kiro.ts:119/:185`、`cline.ts:64`、
  `roo-code.ts:61`、`opencode.ts:325`、`qodercli.ts:332/:399`），大日志场景的
  内存占用未做保护。**解析层**没有增量游标（每轮 sync 全量重扫，
  `sync-service.ts:266`；不过上传层有内容哈希差异，`upload-manifest.ts:236-243`）。
- **codex 的差分基准只在 total 分支回写**（`codex.ts:202`），且 `prevTotal`
  每文件新建、不跨 sync 持久化（`:131`）：当同一文件内该 model key 已由
  `last_token_usage` 分支计入过用量时，随后首见的累计值会被**整额再计一次**
  （`:198-201`）。纯累计（无 last）的文件不会高估，只是把首见前的累计量整额
  归到首见时间点。
- **gemini-cli 裸减法**（`gemini-cli.ts:219-220`、`:233-237`）：`cached > input`
  或 `thoughts > output` 时该条 entry 的**四个桶整体不入账**（判据
  `token-usage.ts:13-16`，执行点 `aggregator.ts:21-24`、
  `session-extractor.ts:28-30`）——不是把负桶钳到 0，连本次有效的
  cached/reasoning 也一并丢失；丢弃粒度是单条消息，会话对象（时间/轮次/
  消息数）仍会产出，只是 token 记 0。
- **copilot-cli 与 kimi-code 的缓存写入 token 未进入任何桶**：copilot-cli 的
  `cacheWriteTokens` 只在 `copilot-cli.ts:37` 声明、`:155-173` 的 entry 映射
  从不读取，且 `:159-161` 的零值判定**不含**它——只含缓存写入的 shutdown 会
  **整条消失**；kimi-code 的 `input_cache_creation` 在 `kimi-code.ts:238`
  读出后仅参与 `:240-247` 的全零判定、`:264-274` 未映射进 entry——只含它的
  entry 不消失，而是产出一条**全零 entry**。
- **Cursor 的用量只能来自联网 CSV**（`cursor.ts:135-152`）：网络异常/超时
  （`:162-170` 打 skip 标记）与取不到 token（`:295`）都会**静默**返回空结果、
  无日志；但 HTTP 非 2xx 会在 `:175` 抛出 `Cursor usage export auth failed`，
  经 `parser-service.ts:45-47` 记为 warn——"静默"只覆盖网络层与无 token
  两类分支。全程无本地降级路径（`sessions: []`，`:352`）。
- **qodercli 的 credits ≠ token**：credit 差值写进 `inputTokens`
  （`qodercli.ts:286`，单位声明 `:44-48`），会直接进入桶与总 token
  （`aggregator.ts:50-54`、`queries.ts:159-165`），与其它 agent 不可加。
  费用影响分两种：模型串命中官方前缀（`resolve.ts:16-79`）则按 token 单价
  乘 credit 数（数值无意义）；落到 `"credits"` 兜底（`:61`）则计未定价
  （金额 0、计入覆盖率分母）。
- **`UsageBucket` 的 5 个 `estimated*Usd` 列恒为 NULL**
  （`schema.prisma:212-216`）：`ingest.ts:68-77` 不写（upsert 是唯一写点）、
  `queries.ts:67` 的 `findMany` 无 `select` 会把它们随行取回但下游只读 token
  字段、桶成本一律查询期现算（`:113-136`）。**既无写入方也无读取方**，
  只有随行取回的传输开销。
- **会话与桶两套成本口径不合并**：桶侧金额每次查询按当前价目现算
  （`queries.ts:113-136`，KPI 取 `:669-703` 的 `currentUsd`；分解 `:532-540`），
  会话侧读 ingest 期落库的 `UsageSession.estimatedCostUsd`
  （`ingest.ts:97-111`、`queries.ts:795`）。**改价后** KPI / 定价表 / 分解 /
  榜单金额会变，**历史会话明细不变**——只有会话被重传时才按新价重写
  （`ingest.ts:263`；而清单内容哈希不因改价变化，`upload-manifest.ts:236-243`）。
  **目录不可用**时桶侧归 0、会话明细仍显示旧值。两处本来也不等：会话 token
  只统计带 sessionId 的 entry（`session-extractor.ts:23-30`），
  cline/cursor/kiro/qwenpaw 的会话 token 恒 0、金额 null。
- **`cache_write` 全仓无计算消费**：`catalog.ts:13` 仅是类型字段——取值确实
  随价目快照传入（`:102`），但 `estimateCostUsd` 只读 input/output/reasoning/
  cache_read。同类还有 `resolve.ts:102-104` 的 `_unique`（非 export、零引用）。
- **看板无 SQL 聚合、无分页/limit**（`queries.ts:62-109`）：一次仪表盘渲染会
  执行 `loadBuckets` 7 次、session 全量读 7 次（行号见 §5.3），其中 5 遍逐行
  调用 `estimateBucketCostUsd`（`queries.ts:297`、`:408`、`:532`、`:720-721`），
  单行工作量为固定上限（`resolve.ts:16-79` 的 11 条规则 × ≤3 候选）。
  **摄入数组无长度上限**：`contracts.ts:109-110` 无 `.max()`，唯一校验点是
  `api/usage/ingest/route.ts:34`，`ingest.ts` 与 `next.config.ts` 均无二次限制。
- **model 过滤下会话指标不随过滤**（§5.4 的错配）：按 `model` 筛选时，
  **随过滤**＝token 四桶、分解卡 token/成本、价格表、趋势图 token/成本、
  KPI 的 `estimatedCostUsd`（`kpi-grid.tsx:145-151` 取自 bucket 侧
  `pricingSummary`）；**不随过滤**＝sessions / messages / userMessages /
  activeSeconds / totalSeconds（KPI、分解卡、趋势图 totalSeconds、热力图、
  会话明细列表）。成本合计因此不受错配影响，但会话明细行的
  `estimatedCostUsd` 是 ingest 落库的**全模型**值，与按模型过滤的 KPI 成本
  不可对账。
- **排行榜结算在并发未提交期仍有撞唯一约束的窗口**：`finalize.ts:232-247`
  先按 `dedupeKey` `findMany` 过滤（`schema.prisma:336` 为 `@unique`），
  `:250` 的 `createMany` 未加 `skipDuplicates`（对照
  `achievements/queries.ts:585` 有）；`:103-119` 的幂等早退无锁——风险低但存在。
- **无跨 DST 转换时刻的测试**：`date-range.test.ts:25-35` 是全仓唯一的负偏移
  用例（`America/Los_Angeles`，2026-03-26→27，处于 PDT 生效期、不跨转换点），
  其余时区用例均为 `Asia/Shanghai`（无 DST）；墙钟→UTC 换算统一走
  `date-range.ts:92-122` 的 ≤3 轮偏移迭代，格子递推在 `listRangeBuckets`
  （`:322-326`）——转换点行为缺乏定向用例。
- **测试覆盖缺口**：`kiro.ts` 无测试（parsers 目录唯一没有同名 `.test.ts` 的
  实现；`parser-service.test.ts:3-4` 用 mock 替换 registry）；`opencode.ts`
  **有**测试但只覆盖 JSON 存储分支，SQLite 分支（`opencode.ts:212-217`、
  `:228-239`）无用例。二者同被列入 coverage exclude（`cli/vitest.config.ts:16`、
  `:19`）——"排除"与"无测试"是两件事。
- **采集端拉取的 `settings.timezone` 除完整性校验外不参与任何计算或上传**：
  `client.ts:204-212` 校验缺失即判 settings 无效，使本轮同步失败
  （`sync-service.ts:303-308`）；CLI 全仓无其它使用点。
- **`codex.ts:110` 的 `const sessionModel = "unknown"` 是死变量**：回退链
  （`:206-207`）在它之前已由 `turnContextModel`（初值同为 `"unknown"`，
  `:124`）截断，末段实际兜底就是字面量。
- **锁被占用时的 `lastCompletedAt`**：`sync-service.ts:239-253` 走
  `markSyncFailed(..., "skipped_locked")`，该函数（`state.ts:87-102`）会把
  `lastCompletedAt` 设为当前时间，与"本轮未完成"的字段语义矛盾；但该字段
  **只写不读**（全仓仅 `state.ts:16`、`:76`、`:96`；`status.ts:112-138`
  展示的是 `lastAttemptAt`/`lastSuccessAt`），不会造成展示误读——风险在
  后续新增消费者时兑现。

### 11.2 未证实的推断（源码无法判定）

- codex 若把 `total_token_usage` 作为会话级累计、而差分键按 `info.model`
  分裂，则换模型时新 key 的首见值可能含旧模型累计——依赖上游日志语义。
- qodercli 的 quota 是账户级三池之和（`qodercli.ts:162-168`）且无 entrypoint
  维度过滤，差值是否包含 Qoder IDE 的消耗同样属上游语义。
- 价目目录抓取失败返回 `null` 后，该 `null` 是否被 `unstable_cache` 按 12
  小时缓存，取决于 Next 运行时行为；源码未处理"负缓存"。
- DST 转换时刻的实际换算输出正确性（静态核对只能证明"无定向用例"）。
- 排行榜条目表 `leaderboard_period_entry` 的 `@@unique([resultId, userId])`
  （`schema.prisma:380-381`）与 `finalize.ts:208-228` 的
  `deleteMany`+`createMany` 组合，在交错并发下的窗口比上一条更宽——
  结构化推断，未做并发验证。
