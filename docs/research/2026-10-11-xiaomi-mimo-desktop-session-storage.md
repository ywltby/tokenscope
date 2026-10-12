# Xiaomi MiMo 桌面版会话存储与 token 用量核验（候选适配器前置研究）

> 只读核验：结论来自两个独立证据源——①对**本机已安装程序** `C:\Program Files\Xiaomi MiMo`
> （Electron 外壳，`app.asar` 91.5 MB）与其持久化数据目录的静态分析；②对**上游开源仓库**
> `github.com/XiaomiMiMo/MiMo-Code` 的源码对照（§11）。所有 SQLite 一律以
> `file:...?mode=ro` 只读 URI 打开，未写入、未移动、未清理任何被扫描数据。探针脚本与
> 临时克隆目录只在工作区/系统临时目录存在，归档前已删除。
>
> 文档只保留结构化字段名、表名、行数、口径判断与源码证据位置，**不含真实会话 ID、消息 ID、
> 项目路径、提示词、工具输出或模型输出的任何内容**；涉及路径的示例一律符号化
> （`<项目路径>`、`ses_<26 字符>`、`msg_<...>`）。
>
> 结论分三档标注：**已验证**（本机实测可复算）／**一致解释**（多条证据指向同一原因，
> 无单点直证）／**未验证推论**（待新版本或补充样本复核）。
> 文件大小一律**十进制**（1 KB = 1000 B、1 MB = 1000000 B）。

## 1. 结论摘要

1. MiMo 桌面版是 **Electron 外壳 + 本地引擎（mimocode）** 的两层结构：外壳负责 UI / 账号 /
   遥测，引擎提供本地 HTTP 服务（日志实测 `http://127.0.0.1:<端口>`，带 Basic Auth）并
   **独占会话与用量的写入**。**引擎本体就是打包进 asar 的 37.2 MB `out/main/node.mjs`**
   （= 开源仓库 `XiaomiMiMo/MiMo-Code` 的构建产物；打包与接线实证见 §12）。
   **会话记录的唯一事实源是引擎主库 `%USERPROFILE%\.local\share\mimocode\mimocode.db`**
   （SQLite，本机 38.2 MB），不在 `C:\Program Files` 下，也不在 `%APPDATA%` 下。
2. 会话正文分三张表：`session`（9 行）→ `message`（1539 行）→ `part`（7035 行）；
   **正文以 JSON 文本列存**（`message.data` / `part.data`），不是 JSONL 文件。
3. **能拿到逐请求 token 用量**：1435 条 assistant 消息**全部**带 `data.tokens`
   （`{total?, input, output, reasoning, cache:{read,write}}`，其中 `total` 是**可选键**），
   其中 **1432 条带 `total`**；缺 `total` 的 3 条是失败回合（`tokens` 仍在但全 0，且带 `error`）。
4. **一次 LLM 调用 = 一条 assistant 消息**（已跨源验证，见 §6）：本机 1435 条 assistant 消息，
   与引擎自记账 `actor_registry.turn_count` **逐会话精确相等（9/9，合计 1435/1435）**；
   每条 assistant 消息只含 **1 个** `step-finish` 部件（1432 条为 1，3 条为 0）。
   即 **1435 = 调用尝试数（含 3 次失败），1432 = 有用量记录的调用数**，两数不可混用。
5. `message.data.tokens` 与 `part.data`（`type = step-finish`）的 `tokens`
   **1:1 恒等（1432/1432 逐字段全等）**——**两处只取一处**，同时累加即双倍虚增。
6. **`total` 恒等式**：`total = input + output + reasoning + cache.read + cache.write`
   在本机 **1432/1432 成立**；`cache.write` 恒为 0（1432/1432）。
   **恒等式的成因已由源码证实**（§11.3）：`total` 取 AI SDK 归一后的 `usage.totalTokens`
   （= 原始 inputTokens + outputTokens），而存库的 `input` 是「inputTokens − cache.read −
   cache.write」、`output` 是「outputTokens − reasoning」，两项相减正好抵消。
7. `input` 与 `cache.read` 是**并列互斥的输入侧两桶**（`input` 不含缓存读），与 Claude 口径
   同构；`reasoning` 是**独立第三桶**（本机单条 max 2,995、模型级合计 223,568），
   与 OpenAI 的「reasoning 含在 completion 内」不同。
8. `cost` 字段本机**恒为 0**（数值 0，非缺失）；`part.data`（`step-finish`）的 `cost` 同样恒 0。
   **源码证实这是「按本地价格表算出来就是 0」，不是「客户端拿不到计费」**（§11.4）：
   引擎用模型价格目录（models.dev）逐请求相乘累加，本机所用的
   `mimo-x-pro-preview` 未收录、`mimo-desktop` provider 无价格来源，故全程为 0。
   **不得把 0 当成真实费用，也不得当成「免费」**。
9. **子 agent 用量同库同表**：`message.agent_id` 本机取值为 `main` 与 `explore-1` /
   `explore-2` / `explore-3`（子 agent 按角色名 + 序号动态命名，非固定集合），
   不存在 WorkBuddy 那样的独立子转写文件；漏看该列会把子 agent 用量错并进主 agent。
10. **本地没有额度 / 余额数值**：桌面版**自带**「Usage & billing」设置页（套餐档
    Starter / Plus / Pro / Ultra、周限额进度条、剩余百分比、重置日期、续费与退订入口），
    数据**运行时从云端拉取**，仅存在于运行期内存（页面带 `loading` / `loadFailed` / `retry` /
    `authExpired` 等状态）；**本机已登录（`xiaomi-last-confirmed.json` 有账号记录）却仍
    未发现任何额度数值落盘**（逐类核查见 §5.5 与 §6），故 `TokenScope` 只能统计本地
    token，无法离线给出「剩余额度」。引擎侧另有额度耗尽错误码 `QUOTA_EXCEEDED` / `BALANCE_INSUFFICIENT` /
    `LONG_CONTEXT_CREDITS_REQUIRED`。

## 2. 方法与样本

- 程序侧：`C:\Program Files\Xiaomi MiMo\resources\app.asar`（91,502,971 B）只读解码为文本流
  （900,895 行，`split("\n")`），用于取 schema 定义与聚合逻辑的源码证据。**行号对
  `26.929.292248` 版本有效，升级后需重新定位**。
- 数据侧：`%USERPROFILE%\.local\share\mimocode`（Python `sqlite3` 只读 + 文件枚举）、
  `%APPDATA%\Xiaomi MiMo`（结构与配置 JSON）、`%USERPROFILE%\.cache\mimocode\models.json`。
- 会话样本：9 个会话 / 1539 条消息 / 7035 个部件；1435 条 assistant 消息全部带 `tokens`，
  其中带 `total` 的 1432 条构成本文的用量样本。
- 时间跨度：消息级 `time.created` 为 `2026-09-10T06:17:40.337Z` → `2026-10-01T11:22:19.594Z`；
  会话级 `session.time_created` 起点比它早 386 ms（`06:17:39.951Z`），两个口径勿混用。
- 运行态：核验期间 **MiMo 未运行**（引擎端口无监听、无进程），因此
  「运行时 API 实时查询」**未做实测**，只做了静态路由提取（§3.4）。
- 局限：单机单用户、单版本；两个模型（`mimo-x-pro-preview` 旧、`mimo-v2.6-pro` 新）覆盖，
  未覆盖付费账号下的 `cost` 非零形态。

## 3. 存储分层（已验证）

### 3.1 引擎侧（会话与用量的真实归属地）

| 路径 | 角色 | 是否承载正文 |
| --- | --- | --- |
| `%USERPROFILE%\.local\share\mimocode\mimocode.db` | **引擎主库（唯一事实源）** | 是 |
| `%USERPROFILE%\.local\share\mimocode\mimocode.db-wal` / `-shm` | 运行期残留文件；核验时**确实存在**（`-wal` 0 B 已 checkpoint、`-shm` 32 KB） | 否 |
| `%USERPROFILE%\.local\share\mimocode\storage\session_diff\<sessionID>.json` | 会话 diff（本机 8/9 为空数组） | 否 |
| `%USERPROFILE%\.local\share\mimocode\snapshot\<projectID>\<40hex>\` | 会话快照，**每个快照是一份独立 git 仓库**（含 `HEAD` / `objects` / `packed-refs`）；本机 4.84 MB / 50 文件 | 文件副本 |
| `%USERPROFILE%\.local\share\mimocode\tool-output\tool_*` | 超长工具输出外置（本机 3 个，最大 441 KB） | 是（正文延展） |
| `%USERPROFILE%\.local\share\mimocode\log\*.log` | 引擎日志（**不含用量数值**，仅错误与生命周期） | 否 |
| `%USERPROFILE%\.local\share\mimocode\compose\<version>` | 版本化技能包（`skills\` + `.extracted`），本机 3 个版本 | 否 |
| `%USERPROFILE%\.local\share\mimocode\builtin_skills\<version>` | 内置技能正文，本机 3 个版本 / 1095 文件 / 6.27 MB | 否 |
| `%USERPROFILE%\.cache\mimocode\models.json` | models.dev 快照（5.28 MB，225 provider） | 否 |
| `%USERPROFILE%\.cache\mimocode\bin\rg.exe` | 引擎自带 ripgrep | 否 |
| `%USERPROFILE%\.config\mimocode\` | 引擎运行时 node_modules（zod 等） | 否 |

引擎日志行首格式：`LEVEL ISO8601 +<delta>ms service=<svc> key=value ...`，
其中 `service=llm` 行会记录 `providerID` / `modelID` / `session.id` / `agent` / `mode`，
但**不打印 usage 数值**。全量核对（7 份引擎日志 + 7 份外壳日志，共 3.83 MB 文本）：
`input_tokens` / `total_tokens` / `cache_read` / `cached_tokens` / `reasoning_tokens` /
`usage=` / `credits` **均 0 次**；仅有的 2 次 `output_tokens` 落在**系统提示词正文**里
（工具描述中的 "output exceeds 30000 tokens"），`max_tokens` 则出现在错误体的
`requestBodyValues`（请求参数，非用量）。**日志不可作用量来源。**

### 3.2 外壳侧（`%APPDATA%\Xiaomi MiMo`）

| 路径 | 角色 |
| --- | --- |
| `db\artifacts.db`（586 KB） | 交付物索引（`artifact` / `artifact_source` + FTS5 trigram），含 `session_id` / `call_id`，**无 token** |
| `db\rolechat.db`（20 KB） | 角色会话索引（`session_id` / `name` / `task_status`） |
| `db\session-review.db`（20 KB） | 会话评审过滤标记（`session_id` / `turn_key`） |
| `preferences.json` | 桌面偏好：`model`、`perm`、`permByConvo`（**会话级权限表**）、`fullAccessEnabled` … |
| `composer-input.json` | 输入框草稿与 `convoMeta`（`project` 归属） |
| `repo-roots-cache.json` | 项目根解析缓存（路径 → git 根） |
| `model-catalog.json` | 云端模型目录快照（含 `mimo-v2.6-pro` / `mimo-v2.6-flash` / TTS / ASR / 图像） |
| `models-with-claude.json`（4.58 MB） | **另一份 models.dev 快照**（213 provider / 7695 模型，含 Claude 全系）；与 `~/.cache/mimocode/models.json`（225 provider）**不是同一份**，两者时间点不同，选计价来源时需区分 |
| `onetrack-cc.json` | 遥测开关配置（事件含 `mimo_turn_usage`、`agent_request`、`tool_call`）；**队列文件 `onetrack-queue.jsonl` 本机 0 字节** |
| `feature-gates.json` / `apm-region.json` / `xiaomi-last-confirmed.json` | 灰度、区域（`CN`）、账号摘要 |
| `logs\YYYY-MM-DD.log` | 外壳日志：`loadEngineSessions` 诊断（走引擎 API 拉会话列表，非直读库）；**全文 0 处用量数值** |
| `git-version-store\` | 一个 git 仓库，按被跟踪路径镜像（`C_/Users/<用户>/…`），本机 2.12 MB / 496 文件；用途**推断**为会话版本回滚 |
| `mimocode\`（= 引擎 state 根，见 §12.5） | 由外壳用 `XDG_STATE_HOME` 重定向而来；本机含空的 `locks\`（引擎文件锁）与 `llm-server\` |
| `mimocode\llm-server\<hash>\tokens.json` | **本地能力令牌**（ASR/TTS 模型，带 `id` / `hash` / `models` / `idle_ms`）——敏感 |
| `Partitions\xiaomi-account\` | 账号分区（Cookies / Local Storage）——敏感 |

### 3.3 与项目目录无关，但易误判的一处

`%USERPROFILE%\XiaomiMiMoProjects\.mimo-sessions\YYYY\MM\DD\<名称>\` 是**会话产出的工作区文件**
（用户要求「按日/按站点归档产物」时生成的目录），**不是会话记录**；本机该树下只有产物与
`work\` 中间文件。

### 3.4 运行时可用的引擎 HTTP 路由（静态提取，未实测）

`app.asar` 内 Hono 路由（与用量相关的部分）：`GET /experimental/session`（会话列表，query
支持 `directory` / `roots` / `start` / `cursor` / `search` / `limit` / `archived`）、
`GET /session/:sessionID/message`（消息，含 `tokens`）、`GET /session/:id/actors`、
`GET /session/:id/task`、`GET /session/:id/todo`、`GET /event`（SSE）。
外壳日志显示其实际调用形如 `GET /experimental/session?roots=true&limit=2000`
（响应约 266 KB / 9 条会话）。**该服务带 Basic Auth**（用户名固定 `opencode`、密码每次
启动随机生成并由外壳注入，见 §12.3），外部进程拿不到凭据。**TokenScope 不建议走 API**：
引擎未运行时不可用，且违背只读直连库的既有分层（见 §8）。

## 4. 会话与正文格式（已验证）

### 4.1 表清单（`mimocode.db`，共 37 张表，drizzle 迁移账本 53 条）

| 表 | 行数 | 说明 |
| --- | --- | --- |
| `session` | 9 | 会话主表：`id` / `project_id` / `parent_id` / `slug` / `directory` / `title` / `version` / `title_source` / `time_created` / `time_updated` / `time_archived` |
| `message` | 1539 | 消息：`id` / `session_id` / `agent_id` / `time_created` / `time_updated` / `data`(JSON) |
| `part` | 7035 | 消息部件：`id` / `message_id` / `session_id` / `time_created` / `data`(JSON) |
| `project` | 3 | 项目：`id` / `worktree` / `vcs` / `name` |
| `actor_registry` | 12 | actor（主 + 子 agent）：`actor_id` / `mode` / `agent` / `turn_count` / `context_mode` / `lifecycle` |
| `session_prefix_snapshot` | 13 | prompt 前缀指纹：`profile_key` / `system_hash` / `tools_hash` / `revision` |
| `task` / `task_event` | 46 / 119 | 任务追踪（与用量无关） |
| `history_fts` + `history_fts_idx` | 4169 | **内容全文索引**（`body`），非用量；由触发器与 `part` 同步。FTS5 的辅助表另有行数（`_config` 1 / `_data` 735 / `_docsize` 4169 / `_idx` 728） |
| `history_index_migration` | 5 | 全文索引的迁移进度（`version` / `phase` / `cursor`） |
| `memory_fts` / `inbox` / `todo` / `workspace` / `permission` / `permission_grant` / `workflow_run` / `event` / `event_sequence` / `session_share` / `account_state` | 0 | 本机未用（**仅主表为 0**；`memory_fts` 的 FTS5 辅助表仍有 1 / 2 行） |
| `account` / `control_account` | 0 | 云端账号令牌表，本机 0 行（未使用；账号侧数据在 `%APPDATA%` 账号分区） |
| `claude_import` / `external_import` | 0 | **外部会话导入账本**（支持导入 Claude Code 会话） |

### 4.2 `message.data` 形态

`role` 两种：`user` 104 条 / `assistant` 1435 条。键集合（全量计数）：

| 键 | 出现次数 | 说明 |
| --- | --- | --- |
| `role` / `time` / `agent` | 1539 | 公共；`time = {created, completed?}`，**毫秒 epoch（UTC）** |
| `parentID` / `mode` / `path` / `cost` / `tokens` / `modelID` / `providerID` | 1435 | **仅 assistant**；其中 `mode` 上游已标 `@deprecated`（§11.2） |
| `finish` | 1432 | 仅 assistant；**缺 `finish` 的 3 条正是失败回合** |
| `model` / `system` / `systemMode` / `harness` / `summary` | 101～104 | **仅 user**（`model` 104、其余 101） |
| `error` / `actorResult` | 3 | 失败回合 |

- assistant 的 `path = {cwd, root}`：**每条 assistant 消息都带请求时 cwd 与项目根**，
  可直接用于项目归属（`root` 在未识别 git 根时为 `"/"`）；user 消息**不带** `path`。
- assistant 的 `finish` 分布：`tool-calls` 1342 / `stop` 90 / **缺失 3**（合计 1435）。
  **`stop` 才是一次用户回合的结束**，`tool-calls` 是中间步。
- user 的 `system` 是**完整系统提示词正文**（长文本），`summary = {diffs:[{file, patch, additions, ...}]}`
  是发送时的**未提交改动快照**——两者都属敏感内容，采集时不得落库。
- 3 条 `error` 消息带 `{name:"APIError", data:{message, statusCode, responseBody, metadata:{url}}}`；
  本机为会员/鉴权类 403。**错误消息 `tokens` 全 0 且无 `total`**。

### 4.3 `part.data` 形态（部件类型分布）

| `type` | 数量 | 关键字段 |
| --- | --- | --- |
| `tool` | 2250 | `callID` / `tool` / `state{status, input, output, title, time, metadata?, error?}` |
| `step-start` | 1434 | `snapshot?` |
| `step-finish` | 1432 | **`tokens` / `cost` / `reason` / `snapshot?`** |
| `reasoning` | 1157 | `text` / `time` |
| `text` | 713 | `text` / `ignored` / `synthetic` / `metadata` / `time` |
| `patch` | 49 | `files` / `hash` |

- 工具名分布（前 8）：`bash` 692、`read` 485、`edit` 470、`grep` 237、`write` 135、
  `task` 121、`present_files` 51、`actor` 17（余 `glob` / `webfetch` / `history` / `skill` /
  `memory` / `websearch` / `invalid`）。
- `tool.state.metadata` 是工具级元数据（本机抽样为 `{id, status, truncated}` 之类），
  **不含 token**。
- 同一条 assistant 消息的 `part` 数最大 11（reasoning + text + 多个 tool）。
- `step-start`（1434）比 `step-finish`（1432）多 2 条：**存在「已开始但未收尾」的步骤**，
  解析不得假设两者严格成对（用量只在 `step-finish` 上）。

### 4.4 项目身份（可直接复用既有口径）

- `project.worktree` 是**项目根**；`id = 'global'` 是**哨兵项目**，源码里有三种成因
  （§11.5）：向上找不到 `.git`（`worktree = "/"`，本机属此形态）、`.git` 落在 home 或
  文件系统根、git 不可用或非仓库。本机 9 个会话中 **7 个归属 `global`**，
  **不能**用 `worktree` 直接当项目路径（会得到 `"/"`）。
- `session.directory` 是**会话工作目录**，逐会话稳定（同一 `global` 项目下多个不同目录）。
- `message.data.path.cwd` 是**逐消息工作目录**（同会话内不变），`path.root` 是当时的根。
- 落地建议：项目 key = `project.worktree`（非 `global` 时）**回退** `session.directory`，
  与 TokenScope 既有「规范化绝对路径 → 项目 key」规则对齐；`session.directory` 与其子目录
  按既有「项目根归并」规则处理。

## 5. 用量记账口径（核心）

### 5.1 tokens 结构与恒等式（已验证）

assistant 消息 `data.tokens`（源码权威定义见 §5.4）：

```json
{"total": 0, "input": 0, "output": 0, "reasoning": 0, "cache": {"read": 0, "write": 0}}
```

- `total` 出现于 **1432** 条消息（即全部可用消息）；**3 条失败消息无 `total`**
  （`tokens` 键仍在但各桶全 0）。
- 恒等式 `total == input + output + reasoning + cache.read + cache.write`：
  **1432/1432 成立，零反例**。
- `cache.write` 本机**恒 0**（1432/1432）；单条极值：`input` 611,637 / `output` 23,393 /
  `reasoning` 2,995 / `cache.read` 753,280 / `total` 755,727（**单次请求可达 75 万 token**，
  与 1,048,576 的上下文窗口量级吻合）。
- 按模型（仅带 `total` 的 1432 条）：

| providerID | modelID | 条数 | input | output | reasoning | cache.read | total 合计 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `xiaomi` | `mimo-x-pro-preview` | 391 | 1,957,283 | 284,319 | 0 | 99,576,512 | 101,818,114 |
| `mimo-desktop` | `mimo-v2.6-pro` | 1041 | 3,495,600 | 439,003 | 223,568 | 327,959,168 | 332,117,339 |
| **合计** | | **1432** | **5,452,883** | **723,322** | **223,568** | **427,535,680** | **433,935,453** |

- 旧模型（`mimo-x-pro-preview`）391 条的 `reasoning` **恒 0**，新模型（`mimo-v2.6-pro`）
  1041 条合计 223,568 → **该桶是否有值取决于模型或引擎版本**，解析不得假设它恒 0 或恒有值。
- 首次请求（`cache.read == 0`）全库仅 1 条：`input = 64,588` → 系统提示词 + 工具定义约
  6.5 万 token；其后请求靠 prompt cache，缓存读占比高达 **98.5%**（427.5 M / 434.0 M）。
  **这正是必须区分 input 与 cache.read 的原因**：混淆会让「输入量」与「费用」全错。

### 5.2 message 与 step-finish 的关系（已验证，防双计）

- 每条 assistant 消息的 `step-finish` 数分布：**`{1: 1432, 0: 3}`** —— 严格 1:1（除失败回合）。
- `message.data.tokens` 与对应 `step-finish.tokens` **逐字段全等：1432/1432**；
  3 条不等者即「有 message 无 step-finish」的失败消息。
- **结论：两者是同一份数据的两个副本。TokenScope 应只取 `message.data`（消息表已带
  `session_id` / `agent_id` / 时间戳，且是引擎 turn usage 的聚合基准）；若取 `part`，
  必须按 `message_id` 去重。同时累计 → 2 倍虚增。**

### 5.3 桶归一（接入铁律）

| TokenScope 桶 | MiMo 来源 | 说明 |
| --- | --- | --- |
| `input_tokens` | `tokens.input` | 未命中缓存的输入，**直取**，无需减 |
| `cache_read_input_tokens` | `tokens.cache.read` | 命中缓存的输入 |
| `cache_creation_input_tokens` | `tokens.cache.write` | 本机恒 0（保留字段） |
| `output_tokens` | `tokens.output` | 生成侧 |
| （新增桶）`reasoning_tokens` | `tokens.reasoning` | **不与 output 重叠**；若 TokenScope 无此桶，需先决定是并入 output 还是单独展示，不可默认丢弃 |
| 校验 | `tokens.total` | 应等于各分项之和（input + output + reasoning + cache.read + cache.write）；不等则记 bad 行，不静默吞掉 |
| 请求数（成功） | 带 `total` 的 assistant 消息 = `step-finish` 条数 | 本机同为 **1432**，恒等，取一处 |
| 请求数（尝试） | assistant 消息总数 | 本机 **1435**（含 3 条失败），与 `actor_registry.turn_count` 同口径 |

### 5.4 源码证据（一）：`app.asar` 打包产物，版本 `26.929.292248`

- **schema 权威定义**（行 833555～833572，zod）：

```js
StepFinishPart = PartBase2.extend({
  type: zod_default.literal("step-finish"),
  reason: zod_default.string(),
  snapshot: zod_default.string().optional(),
  cost: zod_default.number(),
  tokens: zod_default.object({
    total: zod_default.number().optional(),
    input: zod_default.number(),
    output: zod_default.number(),
    reasoning: zod_default.number(),
    cache: zod_default.object({ read: zod_default.number(), write: zod_default.number() })
  })
})
```

- **引擎自身的 turn usage 聚合**（行 104267～104268）：只在 `message.updated` 事件上聚合，
  把 `{input, output, reasoning, cache:{read,write}}` **扁平化**为
  `{input, output, reasoning, cacheRead, cacheWrite, providerId, modelId}`，
  并仅在 `input+output+cacheRead+cacheWrite > 0` 时上报（`kind:"usage"`）。
  **这正是 `mimo_turn_usage` 遥测的构造点**（事件名见相邻行 104268）——即引擎自己也是
  **按 message 记账、并排除全 0**。
  同处的 part 分支只把 `step-finish.tokens` 记入 `turnUsageSteps`，**不单独计费**。
- **外部导入路径**（行 714299，与 `ClaudeImport` 同模块导出）有 `tokensFrom(usage)`：
  把 Anthropic 风格 `input_tokens` / `output_tokens` / `cache_read_input_tokens` /
  `cache_creation_input_tokens` 映射为同一内部结构（`reasoning: 0`）——**一致解释**：
  内部 tokens 结构对所有来源统一，导入条目也走这套映射。

### 5.5 费用与额度（`cost` 恒 0 的成因 + 额度不可离线取得）

- `message.data.cost` 与 `step-finish.cost` 本机 **恒为 0**（数值 0，非缺失）。
  机制由源码证实（§11.4）：**cost 是引擎用本地模型价格目录逐请求算出来的**，
  本机所用模型没有可用价格（`mimo-x-pro-preview` 未收录；`mimo-desktop` provider
  不在 models.dev 目录内），四则运算结果自然是 0。**这与「会员制不计费」是两回事**，
  换一个价格目录里有价的 provider，同一份代码就会写出非 0 cost。
- 客户端内置了**模型单价目录**（models.dev 快照，`%USERPROFILE%\.cache\mimocode\models.json`），
  其中 `xiaomi` provider 带真实单价（本机快照示例：`mimo-v2.6-pro` input 0.435 /
  output 0.87 / cache_read 0.0036；`mimo-v2.6-flash` 0.14 / 0.28 / 0.0028），
  而 `xiaomi-token-plan-{cn,ams,sgp}` 三个 provider 的单价全 0（套餐制）。
- **对 TokenScope 的含义**：MiMo 的 `cost` 恒 0 只说明「**按本地价格目录算出来是 0**」，
  **既不能作为费用未知的判据，也不能读成「免费」**；应走既有三来源候选计价（可用该 models.json 作为
  「本机第三方模型目录」的额外来源，但**必须先确认其快照时间与许可**）。
- 套餐额度只在云端，且**桌面版自带展示页**：源码中的 account schema 为
  `{email, name, aliases[], type, plan, planEndpoint, planStatus}`，usage schema 为
  `{percent, resetDate}`；设置页有「Usage & billing」区块（`billing.*` i18n：套餐档
  Starter / Plus / Pro / Ultra、周限额进度条 `{{percent}}% remaining`、`Reset: {{resetDate}}`、
  续费/退订入口），并带 `loading` / `loadFailed` / `retry` / `xiaomiLoginRequired` /
  `authExpired` 等运行态——**均为运行时从云端拉取，不落盘**。另有一份 demo 种子数据形如
  `usage:{usedPercent:62, resetInDays:3, used:"6.2M", total:"10M", unit:"tokens"}`，
  是打包进 asar 的演示值，**不是本机数据**。
- 扫描证据：本机 `%APPDATA%\Xiaomi MiMo` 下 **4052 个文本文件 + 28 个 leveldb / 二进制文件**，
  外加 4 个 SQLite 库（3 个外壳库 + 引擎主库）全扫。含关键词（`plan` / `percent` /
  `balance` / `resetDate` 等）的文本文件 **296 个**，逐类核查后**全部是无关文本**：
  LibreOffice 的 UI 定义（`NumberFormatPercent`、`Zoom100Percent`）、技能文档（`SKILL.md`）、
  模型描述（"Balanced … model"）、shiki 缓存哈希文件。二进制与 leveldb **0 命中**，
  4 个库里也没有额度列。故 TokenScope 无法离线给出「剩余额度」。

## 6. 交叉验证记录

| 断言 | 证据 | 结论 |
| --- | --- | --- |
| 一次 LLM 调用 = 一条 assistant 消息 | 1435 条 assistant 消息；`actor_registry.turn_count` 逐会话合计 **9/9 相等**（总计 1435） | 已验证 |
| message.tokens 与 step-finish.tokens 同源 | 1432/1432 逐字段全等；step 数分布 `{1:1432, 0:3}` | 已验证 |
| total 恒等式 | 1432/1432 成立，零反例 | 已验证 |
| cache.write 恒 0 | 1432 行全部为 0 | 已验证 |
| 失败回合不入账 | 3 条 `error` 消息无 `total`、tokens 全 0 | 已验证 |
| 子 agent 同库记账 | `agent_id` 分布 main 1367 / explore-1 63 / explore-2 34 / explore-3 75；单个含子 agent 会话 `49+62+33+74 = 218` 恰好等于该会话 assistant 消息数 | 已验证 |
| cost 不可用作费用 | 1435 行 `cost` 全为 0（含 3 条失败消息） | 已验证 |
| 日志不含用量 | 引擎日志 7 份 + 外壳日志 7 份全文检索 token/usage 关键词，仅 3 行命中且均为错误体（无数值） | 已验证 |
| 会话列表来自引擎 API | 外壳日志 `loadEngineSessions` → `GET /experimental/session` | 已验证 |
| 本地无额度数值 | 4052 文本 + 28 二进制 + 4 个 SQLite 库全扫；含关键词的 **296 个文本文件**逐类核查后均为无关文本（LibreOffice UI / 技能文档 / 模型描述），二进制与库 **0 命中** | 已验证 |
| `finish` 只覆盖成功回合 | 1432/1435（缺 `finish` 的 3 条 = 带 `error` 的 3 条） | 已验证 |
| 步骤边界不成对 | `step-start` 1434 vs `step-finish` 1432，差 2 | 已验证 |
| 两份模型目录不重合 | `models-with-claude.json` 213 provider vs `~/.cache/mimocode/models.json` 225 provider | 已验证 |

## 7. 会话生命周期与写入语义

- **写入者唯一**：只有引擎进程写 `mimocode.db`；外壳通过 API 读。核验期间引擎未运行，
  但目录内**确实存在** `mimocode.db-wal`（0 B，已 checkpoint）与 `mimocode.db-shm`（32 KB），
  只读打开实测成功；引擎运行期须容忍 `database is locked` 与 WAL 增长。
- **消息粒度**：用户发一条消息 → 引擎生成 N 条 assistant 消息（每条 = 一次模型调用，
  以 `parentID` 指向该 user 消息），直到某个 assistant 的 `finish == "stop"`。
  本机 104 条 user 消息对应 1435 次调用（均值 13.8 次/轮）。
- **前缀快照**：`session_prefix_snapshot` 用 `(session_id, profile_key)` 记录系统前缀的
  `system_hash` / `tools_hash` / `watermark_message_id` / `revision`（本机 13 条、`revision`
  全为 1），与 prompt 缓存分段相关（**一致解释**，未直接验证其消费方）。
  另有 3 条 user 消息缺 `system` 字段（同时 `systemMode` 为缺省），**成因未验证**——
  它们仍带 `summary.diffs`，与压缩的因果关系不成立，解析不得依赖 `system` 必存在。
- **导入**：`claude_import` / `external_import` 表（本机空）说明 MiMo 支持把
  Claude Code 会话**导入**本库；导入条目走 `tokensFrom` 归一（**一致解释**）。
  **但桌面版默认禁用了这条路径**（外壳注入 `MIMOCODE_DISABLE_CLAUDE_IMPORT=1`，§12.3），
  因此本机 `claude_import` 为空是预期结果；该风险只在用户跑 **CLI 版**并执行导入时成立，
  届时会与 TokenScope 的 Claude Code 来源重复计数。
- **归档**：`session.time_archived` 本机全为缺省；归档只改字段不移库。
- **会话快照**：`snapshot/<projectID>/<40hex>/` 每份是一个独立 git 仓库；**源码已证实**
  `<40hex>` 是 `Hash.fast(worktree)`（§11.6），gitdir 指向该目录，供会话回滚；
  与用量无关。上游还有一道保护：`.git` 若位于用户 home 或文件系统根，**主动禁用快照**
  （避免 git pathspec 扫全盘）。

## 8. 对 TokenScope 的落地建议

1. **来源发现**：探测 `%USERPROFILE%\.local\share\mimocode\` 下的
   `mimocode.db` **与 `mimocode-*.db`**（后者是 latest/beta/prod 之外通道的库名，
   见 §12.6-2）；
   若 `MIMOCODE_HOME` 有设置，优先探测 `<它>/data/mimocode.db`。都不存在则静默零结果。
   **不要**扫 `XiaomiMiMoProjects\.mimo-sessions`（那是产物目录）、
   **不要**扫 `git-version-store`（文件正文副本）。
2. **只读打开**：一律 `file:...?mode=ro` URI；引擎运行中可能有 WAL，需容忍
   `database is locked` 并降级为「稍后重试」，不得写入。
3. **事件抽取 SQL 形态**：`SELECT m.id, m.session_id, m.agent_id, m.time_created, m.data`
   `FROM message m WHERE m.data LIKE '%"role":"assistant"%'`（JSON1 可用则用
   `json_extract`，**注意存储为紧凑 JSON：键与值之间无空格**，字符串匹配写
   `"role":"assistant"`，带空格会零命中——本次核验已踩过该坑）。
4. **身份键**：`(session_id, message_id)`，本机唯一性退化为 `message.id` 主键；
   另建议以 `(session_id, message.id)` 作幂等键，`agent_id` 作为**归属维度**而非身份维度。
5. **用量口径**：只取 `message.data.tokens`；桶归一按 §5.3；`total` 用于校验不用于展示；
   `cache.write == 0` 时按「显式 0」处理（与既有三态单价语义一致）。
6. **请求数**：成功请求 = 带 `total` 的 assistant 消息数（本机 1432）；调用尝试数 =
   assistant 消息总数（本机 1435）。两者**都不是** user 消息数（104）或 `part` 行数。
   `finish == "stop"` 可用于统计「回合数」这一衍生指标（本机 90）。
7. **模型身份**：`providerID` + `modelID` 双列入库；本机出现 `xiaomi/mimo-x-pro-preview`
   与 `mimo-desktop/mimo-v2.6-pro` 两代，前者在 models.dev 快照的 **225 个 provider 全库检索中
   零命中**（未收录 → 走既有「未知不按 0」路径）。
8. **项目身份**：见 §4.4；`global` 项目必须回退 `session.directory`。
9. **费用**：MiMo 侧 `cost` 恒 0 且**不代表免费**，一律走 TokenScope 既有候选计价；
   `mimo-v2.6-pro` 的单价可从 `%USERPROFILE%\.cache\mimocode\models.json` 的
   `xiaomi` provider 取（若决定纳入该来源，需在设置页显式声明来源与快照时间）。
   注意本机**同时存在两份 models.dev 快照**（`%APPDATA%\Xiaomi MiMo\models-with-claude.json`
   213 provider 与 `~/.cache/mimocode/models.json` 225 provider），选源后必须固定其一
   并披露快照时间，**不要**在两次查询间混用。
   若要自己按价目算 MiMo 的费用，可直接复用上游公式（§11.4）：四个分项各乘单价除 1e6、
   **reasoning 按 output 单价**、并支持 `experimentalOver200K` 分段（`input + cache.read
   > 200,000` 时切换到高价档）。**不要**用 MiMo 的 `cost` 字段反推单价。
10. **时间口径**：`time_created` 为毫秒 epoch（UTC），按既有约定转 ISO8601 UTC 存一次，
    展示层做单次时区转换。
11. **敏感性**：`message.data`（user 的 `system` 全文与 `summary.diffs` 补丁）、
    `part.data`（`text` / `reasoning` / `tool.state.input|output`）、
    `tool-output/`、`snapshot/`、`git-version-store/`、`llm-server/**/tokens.json`、
    `Partitions/xiaomi-account/` **全部属敏感**，只读扫描时**不得落库、不得进导出物**；
    测试 fixture 必须手工合成。
12. **重复计数风险（必须披露）**：a) message 与 part 双取 → 2 倍；
    b) 用户在 **CLI 版**里导入过 Claude 会话 → 与 Claude Code 来源重叠
    （桌面版默认禁用该能力，见 §12.3）；
    c) 子 agent（`explore-*`）若按主 agent 归并会掩盖真实分布。
13. **解析容错**：`part.type` 上游共 13 种（§11.8），本机只出现 6 种，**遇到未知类型
    必须静默跳过**；不要假设 `step-start` 与 `step-finish` 成对，也不要把
    `message.data` 里的 `mode` 当稳定字段（上游已标 `@deprecated`）。

## 9. 风险与未知

- **版本漂移**：全部字段与行号基于 `26.929.292248`（引擎 compose `desktop-1579e7d`）。
  引擎是 **opencode 血统**（表名 / drizzle / `experimental` 路由 / `ClaudeImport` 均可佐证），
  上游演进速度快，`tokens` 结构已在本机跨两代模型出现差异（`reasoning` 从恒 0 变为有值）。
- **未验证推论**：① 每条 assistant 消息恒为 1 个 step-finish 是否在所有路径成立
  （本机 1432/1432，但未见多 step 形态，无法排除长任务的 multi-step 变体）；
  ② 标题生成 / 语音 / 图像请求不入 `message` 表（日志可见 `agent=title`、
  `modelID=mimo-flash` 的请求，而库中无对应记录，**一致解释**但未做端到端对照）；
  ③ 3 条 user 消息缺 `system` 字段的成因未知（既非压缩摘要，也无其他已识别特征）；
  ④ `snapshot/` 与会话 revert 的对应关系：**已由源码证实**（§11.6），不再是推断。
  （原「`total` 由引擎本地求和」一条已由源码否证并更正确认，见 §11.3。）
- **额度不可离线获取**：套餐剩余量只在云端，且无稳定的本地缓存；任何「额度」字段都属
  推断，不应写入历史库。
- **多账号 / 多区域未采样**：`model-catalog.json` 的 `account` 字段与
  `xiaomi-last-confirmed.json` 的 `region` 表明存在账号绑定与区域分流，
  本机单账号（CN），跨账号切换后的库形态未采样。
- **库体量**：38.2 MB 对应 9 会话 / 7035 部件（含 FTS 索引）；本机样本小，
  大历史下的查询耗时与 WAL 行为未验证。

## 10. 待办与未覆盖

- 运行态核验：启动 MiMo 后对比 `GET /experimental/session` 与库内容，确认运行期写入
  （WAL / 事务时序）对只读打开的干扰程度。
- 多 step-finish 形态的定向寻找（长任务 / 子 agent 深度 > 1 的样本）。
- `step-start` 与 `step-finish` 不成对的 2 条：定位其所属会话与中断原因，确认是否计入用量。
- 3 条缺 `system` 的 user 消息：补样本确认其触发条件（编辑重发 / 压缩 / 其他）。
- `history_fts` 与 `part` 的同步一致性（是否可作为内容检索的冗余来源）。
- `artifacts.db` 与 `message.call_id` 的闭合关系（交付物 → 请求的映射是否有用量价值）。
- 导入态复核：若导入过 Claude Code 会话，核验 `claude_import` 行的用量是否与
  TokenScope 的 Claude 来源重复。
- 跨版本：升级 MiMo 后重跑 §6 全部断言，重点看 `tokens` 是否新增桶
  （如 `cache.write` 从恒 0 变为有值）。
- 源码对照衍生项（§11）：① `part` 类型全集 13 种而本机只出现 6 种，需确认其余
  7 种（`file` / `snapshot` / `agent` / `retry` / `checkpoint` / `compaction` / `subtask`）
  是版本差还是功能未触发；② 官方 `stats` 的日期过滤走 `session.time.updated`，
  TokenScope 若要「按天」对齐需自行决定口径（见 §11.7）。

## 11. 上游源码对照（MiMo-Code）

> 证据源：`github.com/XiaomiMiMo/MiMo-Code`（克隆到系统临时目录做静态阅读，归档前已删除）。
> **仓库版本与本机安装版 `26.929.292248` 不保证同一 commit**，因此「源码有、本机无」
> 一律先按版本差处理，不直接判定为行为差异。下文行号以**本次克隆当时的 HEAD** 为准
> （浅克隆，未记录 commit）——需要复核时按函数名/字符串重新定位比按行号更可靠。

### 11.1 仓库形态

bun monorepo，`packages/{cli,plugin,sdk,shared}`；根 `package.json` 的 `name` 仍是
`opencode`，且带 `docs/compose/spec/rename-packages-opencode-to-cli.md` —— **确认是
opencode 的 fork**。本文档 §1 所称「引擎 = mimocode」在源码层面就是 `packages/cli`。
**该仓库只是引擎源码，不含桌面外壳**；两者的确切打包/接线关系见 §12。

### 11.2 schema 逐项对照（与黑盒实测完全一致）

| 对象 | 本机实测 | 源码位置 | 结论 |
| --- | --- | --- | --- |
| `StepFinishPart`（含 `tokens` 五字段） | asar 行 833555；库中 1432 条 | `packages/cli/src/session/message-v2.ts:373-391` | **逐字段一致** |
| `Assistant.tokens` | 1435 条全带 | `message-v2.ts:611-620` | 与 `StepFinishPart` 同构 |
| `Assistant.finish` 可选 | 3 条缺失 | `message-v2.ts:630`（`.optional()`） | 一致 |
| `Assistant.path{cwd,root}` | 1435 条 | `message-v2.ts:605-608` | 一致 |
| `Assistant.mode` | 1435 条 | `message-v2.ts:600-603`（**标 `@deprecated`**） | 一致；新代码不应依赖 |
| `UserMessage{model,system,systemMode,harness}` | 101～104 条 | `message-v2.ts:540-549` | 一致 |
| `session` / `message` / `part` 三表 | 9 / 1539 / 7035 行 | `session/session.sql.ts:15-120` | 表名列名一致 |
| `session_prefix_snapshot` | 13 行 | `session.sql.ts:66-84` | 一致（`system`/`tools` 均为 JSON 列） |
| `part` 类型全集 | 本机 6 种 | `message-v2.ts:555-570`（**13 种**） | 见 §11.8 |

### 11.3 `tokens` 的生成算法（`session/getUsage`，`session.ts:385-448`）

```ts
const inputTokens  = safe(usage.inputTokens ?? 0)
const outputTokens = safe(usage.outputTokens ?? 0)
const reasoningTokens = safe(usage.outputTokenDetails?.reasoningTokens ?? usage.reasoningTokens ?? 0)
const cacheReadInputTokens  = safe(usage.inputTokenDetails?.cacheReadTokens ?? usage.cachedInputTokens ?? 0)
const cacheWriteInputTokens = safe(usage.inputTokenDetails?.cacheWriteTokens
  ?? metadata?.anthropic?.cacheCreationInputTokens ?? /* vertex / bedrock / venice */ 0)

// AI SDK v6 把所有 provider 的 inputTokens 归一为「含 cached」，所以必须减掉
const adjustedInputTokens = safe(inputTokens - cacheReadInputTokens - cacheWriteInputTokens)

const tokens = {
  total: usage.totalTokens,                       // ← 直接取 AI SDK 归一值，非本地求和
  input: adjustedInputTokens,
  output: safe(outputTokens - reasoningTokens),   // ← output 已扣 reasoning
  reasoning: reasoningTokens,
  cache: { write: cacheWriteInputTokens, read: cacheReadInputTokens },
}
```

由该实现可得三条**权威口径**，与 §5.1 / §5.3 的实测结论互相印证：

1. **`total` 是 AI SDK 的 `usage.totalTokens`**，不是引擎自己加的 —— 本机文档原先
   把它列为「未验证推论」，现予**否证并更正**。
2. `total = 原始 inputTokens + outputTokens`，而库里的 `input` / `output` 各自减掉了
   缓存与 reasoning，两者相减正好抵消 → **§1 第 6 条的恒等式是结构性的，必然成立**。
3. `input` 与 `cache.read` 互斥、`reasoning` 与 `output` 互斥，**均为源码显式保证**，
   不是经验观察。另有旁证：`session/processor.ts:654` 用
   `input + cache.read + cache.write` 作为「输入总 token」。

### 11.4 `cost` 的生成算法（同函数下半段）

```ts
const costInfo = input.model.cost?.experimentalOver200K
  && tokens.input + tokens.cache.read > 200_000
    ? input.model.cost.experimentalOver200K : input.model.cost

cost = input      × costInfo.input          / 1e6
     + output     × costInfo.output         / 1e6
     + cache.read × costInfo.cache.read     / 1e6
     + cache.write× costInfo.cache.write    / 1e6
     + reasoning  × costInfo.output         / 1e6   // 「先用 output 价计 reasoning」
```

- **cost 是引擎本地算的**，价格来自模型价格目录，不是服务端返回值。
- 支持 **`experimentalOver200K` 分段定价**（按 `input + cache.read > 200,000` 切段）。
- **reasoning 按 output 单价计**（源码留有 TODO 待 models.dev 改价）。
- 价格装配见 `provider/provider.ts:1366-1373`：`cost.input ?? 0`、`cost.output ?? 0`、
  `cache.read / cache.write ?? 0` —— **缺价格一律回落 0**。这就解释了本机 cost 恒 0
  （§5.5）：所用模型在目录中无价格，而不是「没有计费数据」。
- 目录来源见 `provider/models-catalog.ts`：本地快照 + 网络缓存两层
  （`overlayEntities`：实体集合以权威目录为准，快照只补字段，**不会复活上游删掉的模型**），
  TTL 5 分钟、后台每小时刷新 —— 对应 §3.2 的两份 `models*.json`。

### 11.5 项目身份（`project/project-id.ts` + `project/project.ts`）

- **项目 ID 是持久化在项目里的**，不是路径哈希：有 git → 读写
  `<git-common-dir>/mimocode-project-id`；无 git → 读写 `<目录>/.mimocode-project-id`
  （即**会在用户目录里落一个文件**）。
- `global` 哨兵有三种成因（`project.ts:200-287`）：①向上找不到 `.git` →
  `worktree = "/"`（**本机 7 个会话属此形态**）；②`.git` 落在 home 或文件系统根 →
  保留真实 `worktree` 但**禁用快照**；③git 不可用 / 非仓库。
- 与 §4.4 的落地建议一致：凡 `global` 都必须回退 `session.directory`。

### 11.6 目录布局：四个根与三个子目录（`shared/global.ts` 等）

- 四个根由 `resolveMimocodeHome()` 决定：设了 `MIMOCODE_HOME` 则为其下
  `data/cache/config/state`，否则走 **XDG** → `~/.local/share/mimocode`（data）、
  `~/.cache/mimocode`（cache）、`~/.config/mimocode`（config）、`~/.local/state/mimocode`（state）。
  本机前三个实测存在，`state` 不存在 —— **成因已在 §12.5 查清**（桌面版用
  `XDG_STATE_HOME` 把它重定向了）。
- `snapshot/index.ts:86`：`gitdir = <data>/snapshot/<project.id>/Hash.fast(worktree)`
  → **`<40hex>` 即 worktree 的 fast hash**，并解释了「每份快照是独立 git 仓库」。
- `tool/truncation-dir.ts:4`：`tool-output` 是工具输出的**截断外置目录**。
- `storage/storage.ts:198`：`session_diff/<sessionID>.json` 是会话 diff 的落盘路径；
  同文件里的 `migration.2` 是**旧 JSON 文件存储 → SQLite 的迁移器**，
  说明 `storage/` 目录是历史形态的残迹（本机只剩 `session_diff/`）。

### 11.7 官方 `stats` 命令的口径（`cli/cmd/stats.ts`，可直接对照）

引擎自带 `stats`（`--days` / `--tools` / `--models` / `--project`），其聚合逻辑是官方口径：

| 项 | 官方实现 | 与本文件的差异/提示 |
| --- | --- | --- |
| 数据源 | 遍历 `session` 表 → `Session.Service.messages({agentID: "*"})` | `*` 表示**含子 agent**，印证 §1 第 9 条 |
| 累加对象 | 仅 `role === "assistant"` 的 `info.tokens` | 与 §5.3 一致 |
| 五桶 | input / output / reasoning / cache.read / cache.write 各自累加 | 一致 |
| 总量 | `input+output+reasoning+cache.read+cache.write`（**含 reasoning**） | 与 §1 第 6 条恒等式同构 |
| 日期过滤 | 用 **`session.time.updated`**（不是消息时间） | **提示**：TokenScope 的按天口径不要照搬 |
| 模型级明细 | `tokens.output += output + reasoning`（**把 reasoning 并进 output**） | 官方在这里是两套口径并存 |
| 顶层展示 | 只打印 Input / Output / Cache Read / Cache Write，**不打印 reasoning 行** | 展示层省略，不代表数据缺失 |
| Messages | `messages.length`（**含 user 消息**） | 不是「调用数」，勿与 §5.3 的请求数混淆 |
| 成本 | `message.info.cost` 累加 | 本机为 0，原因见 §11.4 |

### 11.8 `part` 类型全集（13 种）与本机覆盖（6 种）

源码 `message-v2.ts:555-570`：`text / subtask / reasoning / file / tool / step-start /
step-finish / snapshot / patch / agent / retry / checkpoint / compaction`。
本机实测仅出现 `text / reasoning / tool / step-start / step-finish / patch` 六种。
**解析侧含义**：适配器必须对未知 `type` 静默跳过而非报错；用量只挂在
`step-finish`（与 `message` 重复，见 §5.2），其余类型不含 token。
另注：`compaction` part 内含 `compacted_tool_calls[].tokens`（**压缩时各工具调用的 token**），
本机未触发，未来若出现需另行决定是否计入。

### 11.9 对正文结论的处置汇总

| 原结论 | 源码结论 | 处置 |
| --- | --- | --- |
| `total` 可能由引擎本地求和（未验证推论） | 取 AI SDK `usage.totalTokens` | **否证并更正**（§1 第 6 条、§5.1、§9） |
| cost 恒 0 = 会员制、客户端不计价 | cost 由本地价格表计算，缺价格回落 0 | **更正**（§1 第 8 条、§5.5） |
| `input` / `cache.read` 互斥（实测观察） | 源码显式相减保证 | 升级为「结构保证」 |
| `reasoning` 独立成桶（实测观察） | 源码 `output - reasoningTokens` 保证 | 升级为「结构保证」 |
| `snapshot/<40hex>` 用途推断 | `<40hex>` = `Hash.fast(worktree)` | **已证实**（§7） |
| `global` 只有一种成因 | 三种成因，`worktree` 取值不同 | **补充**（§4.4） |
| 引擎日志"不含用量" | 源码中用量只入库，不入日志 | 间接印证 |

## 12. 桌面版与 MiMo-Code（引擎）的确切关系

> 本节回答一个容易含糊的问题：`github.com/XiaomiMiMo/MiMo-Code` 与
> `C:\Program Files\Xiaomi MiMo` 到底是什么关系。证据全部来自 asar 内部文件
> 与 `app.asar` 文本流的字符串/上下文提取。

### 12.1 一句话结论

**MiMo-Code 仓库是「引擎」的源码；桌面版是「Electron 外壳 + 该引擎的打包产物 + 一层
Bun→Node 兼容垫片」的组装体。两者共用同一份数据目录与同一套 `MIMOCODE_*` 约定，
桌面版通过环境变量把引擎切成 desktop 模式。**

### 12.2 打包关系（asar 内部实证）

| asar 内路径 | 大小 | 说明 |
| --- | --- | --- |
| `/out/main/node.mjs` | **37.2 MB**（37,229,974 B） | **打包后的引擎本体**（即 `packages/cli` 的构建产物）；含 `mimocode.db`×6、`getChannelPath`×4、`InstallationVersion`×28、`StepFinishPart`×6、`session_prefix_snapshot`×4、`desktop-1579e7d`×1 等全部引擎特征串 |
| `/out/main/engine-entry.mjs` | 127 B | 引擎入口：挂三个 shim 后 `export * from "./node.mjs"` |
| `/out/main/bun-shim.mjs` | 742 B | 给 `globalThis.Bun` 打 polyfill（`Bun.file` / `Bun.write`） |
| `/out/main/sqlite-bun-compat-shim.mjs` | 621 B | 注释头写着 **`// electron/lib/sqlite-bun-compat.ts`**，给 `node:sqlite` 的 `DatabaseSync.prototype` 补 Bun 风格的 `query` 方法 |
| `/out/main/index.mjs` | 2.4 MB（2,429,461 B） | Electron 外壳主进程 |
| `/out/main/launch.mjs` | 405 B | Electron 入口（`main` 字段指向它） |
| `/out/package.json` | 167 B | `{"name":"xiaomi-mimo-desktop","version":"26.929.292248","main":"main/launch.mjs"}` |

- 桌面版 `package.json` 里 `packageManager` 写的是 **`bun@1.3.14`**，与 MiMo-Code 仓库
  **完全一致** —— 两者出自同一套构建体系。
- asar 清单里 `node_modules\` 有 **769 个文件 + 158 个目录**，**逻辑大小 376.2 MB**。
  注意这是**跨平台清单**，不等于本机占用：其中 372.8 MB 标记为 `unpacked`（不进 asar 本体，
  含 darwin / linux / win32-arm64 的 onnxruntime 与 skia 库），而磁盘上 `app.asar.unpacked\`
  **只落了当前平台（win32-x64）的 82 个文件 / 66.5 MB**（canvas 的 skia、onnxruntime 的 dll、
  node-pty、parcel watcher）。asar 本体（带 offset 的内容）合计 **91.0 MB**，与 91.5 MB 的
  asar 文件大小吻合。这些构成引擎的运行期依赖。
- 结论：**引擎不是外部安装的独立 CLI 包**。实测证据：①`mimocode` / `mimo` 命令不存在；
  ②npm 全局包仅有 `@anthropic-ai/claude-code`、`@deepseek-ai/dsh`、`@openai/codex`、
  `command-code`、`npm`，**无 mimocode**；③pnpm 全局目录为空；
  ④`~/.config/mimocode/package.json` 只声明插件依赖 `@mimo-ai/plugin@0.1.15`（是插件 SDK，
  不是引擎）。

### 12.3 启动与接线（外壳侧 `index.mjs`）

外壳把环境变量**写进自己的进程环境**后，用 dynamic import 加载引擎：

```js
Object.assign(process.env, { ...Tie({ password, dataDir }), ...others })
const Qe = await import("./engine-entry.mjs")      // ← 同进程加载
d = Qe
Cz(Qe.HostModelTransport)                          // 接管模型传输层
Ve = Qe.HostMcp; Ve.set(h)                         // 接管 MCP 宿主
```

其中 `Tie()` 就是桌面版注入的**引擎定制面**（实测字符串）：

```js
{
  MIMOCODE_CLIENT: "desktop",           // 引擎侧默认是 "cli"
  MIMOCODE_SERVER_USERNAME: "opencode", // 本地 HTTP 服务的 Basic Auth 用户名
  MIMOCODE_SERVER_PASSWORD: <随机>,      // 每次启动生成
  XDG_STATE_HOME: <外壳 userData，推测为 %APPDATA%\Xiaomi MiMo，见 §12.5>,
  MIMOCODE_DISABLE_CLAUDE_IMPORT: "1",
  MIMOCODE_CODEX_MODE: "false",
  MIMOCODE_DISABLE_CHECKPOINT: "1",
  MIMOCODE_EXPERIMENTAL_CRON: "false",
  MIMOCODE_PERMISSION: JSON.stringify(bie()),   // 权限规则集（序列化后注入）
  MIMOCODE_ENABLE_ANALYSIS: "false",
}
```

- 这解释了 §3.4 提到的本地 HTTP 服务：**带 Basic Auth**（用户名固定 `opencode`，
  密码随机），外壳持有凭据。
- 引擎侧 `Flag.MIMOCODE_CLIENT` 的实现是 `process.env["MIMOCODE_CLIENT"] ?? "cli"`，
  即**同一份引擎源码按不同构建产出**：桌面版构建把客户端标识默认成 desktop、
  并把 `InstallationVersion` 写成 `desktop-*`（§12.4）；CLI 发行版则走各自的版本串。
  **不要**理解成"桌面与 CLI 共用同一个二进制文件"。
- 外壳另有 `aux-process` 机制（Electron `utilityProcess.fork`，按 `kind` 管理辅助进程池），
  用于 worker 类任务；**引擎主路径是上文的同进程 import**。
  （**未实测**：核验期间 MiMo 未运行，进程模型未做运行时验证。）

### 12.4 版本常量（引擎侧 `node.mjs`）

```js
// src/installation/version.ts
var InstallationVersion = "desktop-1579e7d"
var InstallationChannel = "latest"
```

- `InstallationVersion` **就是本机 `session.version` 的值**，也是
  `~/.local/share/mimocode/{compose,builtin_skills}/desktop-1579e7d` 的目录名来源。
- `InstallationChannel = "latest"` → `getChannelPath()` 返回
  `<data>/mimocode.db`（只有 latest/beta/prod 之外的通道才会是 `mimocode-<channel>.db`），
  **与本机实测的库文件名一致**。
- 应用版本（`26.929.292248`）与引擎版本（`desktop-<hash>`）是**两条独立版本轴**，
  证据里不要混用。

### 12.5 数据目录：两边各自实现同一套 XDG 规则

- 外壳侧自带一份解析函数：`XDG_DATA_HOME ? <它>/mimocode : ~/.local/share/mimocode`。
- 引擎侧是 `resolveMimocodeHome()`（§11.6），规则相同。
- 因此**桌面版与 CLI 版默认落在同一个数据目录**；`MIMOCODE_HOME` 若被设置则两者
  都会切到 `<home>/{data,cache,config,state}`。
- **§11.6 遗留的疑问在这里解决**：桌面版把 `XDG_STATE_HOME` 重定向到**外壳自己的
  userData 目录**（`%APPDATA%\Xiaomi MiMo`），因此引擎的 state 根变成
  **`%APPDATA%\Xiaomi MiMo\mimocode\`**，而不再是 `~/.local/state/mimocode`。
  本机实测闭环：`~/.local/state` **不存在**，而 `%APPDATA%\Xiaomi MiMo\mimocode\`
  **存在**，内含 `locks\`（引擎的文件锁目录，对应源码里的 `Flock.setGlobal({ state })`）
  与 `llm-server\<hash>\tokens.json`。
  说明：`Tie()` 里 `XDG_STATE_HOME: e.dataDir` 的实参在 minified 代码中无法直接读出绑定，
  上述落点是由**目录存在性反推**的（**一致解释**），不是从代码里逐字确认的。

### 12.6 对 TokenScope 的直接含义

1. **一次采集即覆盖两边**：桌面版产出的记录和 CLI 版产出的记录写进**同一个
   `mimocode.db`**，来源维度不需要（也无法）按"桌面 / CLI"拆分；能区分的只有
   `message.data` 里没有客户端标识这一点 —— 若确有需要，只能靠 `session.version`
   （`desktop-*` vs CLI 版本串）间接判断。
2. **库文件名不止一个**：`latest` / `beta` / `prod` 三个通道都写 `mimocode.db`，
   **其余通道**才会写 `mimocode-<channel>.db`（通道名做安全字符替换）。适配器应同时发现
   `mimocode.db` 与 `mimocode-*.db`，不要写死单名。
3. **`MIMOCODE_HOME` 会让数据离开默认位置**：若用户设置过，四个根整体迁移到
   `<home>/{data,cache,config,state}`，探测顺序应为
   「`MIMOCODE_HOME/data/mimocode.db` → `~/.local/share/mimocode/mimocode.db`」。
4. **桌面版禁用了若干引擎能力**（`MIMOCODE_DISABLE_CLAUDE_IMPORT` /
   `MIMOCODE_DISABLE_CHECKPOINT` 等），因此 §7「导入」一节描述的能力在 desktop
   模式下**默认不生效**；`claude_import` 表存在只说明该能力被编译进来，不代表本机
   跑过导入。相应地把 §7 与 §8-12 的重复计数风险降级为"仅在 CLI 侧导入时成立"。
5. **不要依赖 `MIMOCODE_*` 的默认值**：`Tie()` 每次启动注入 **10 个**变量，外壳代码里
   另外还出现过若干（合计 18 个名字），引擎侧识别的开关有 40+ 个。同一份引擎在不同注入
   组合下行为不同。**适配器读库即可，不必模拟这些开关。**

## 13. 逐条复核记录（2026-10-11）

### 13.1 方法与与前几轮的区别

本文档的工具链方法是「**先重算、后比对**」：把文档里所有可量化断言先列成清单，
然后**不看文档**、从原始数据（SQLite 只读查询 / 文件系统枚举 / asar header 与内容提取 /
重新克隆的上游源码）独立算出结果，最后逐条与文档文本比对。

前几轮采用的是**抽查式复核**（挑重点重看），因此每一轮都还能再暴露出新问题。
本节的结论只对**本机这一次快照**负责。

### 13.2 独立重算后完全吻合的断言（未做任何修改）

| 范围 | 吻合项 |
| --- | --- |
| DB 全量 | 37 张表的行数；1539 / 1435 / 1432 / 104 各类消息计数；`message.data` 18 个键的逐个频次；`finish` 分布 1342 / 90 / 3；`agent_id` 分布 1367 / 63 / 34 / 75；`part` 六种类型计数（713 / 1157 / 2250 / 1434 / 1432 / 49）；工具名 15 项分布；`part` 数上限 11 |
| DB 用量 | `total` 恒等式 1432/1432；`cache.write` 全 0；单条极值（611,637 / 23,393 / 2,995 / 753,280 / 755,727）；五行汇总（5,452,883 / 723,322 / 223,568 / 427,535,680 / 433,935,453）；按模型的 391 / 1041 两组全部七列；`cache.read==0` 恰 1 条且 `input=64,588` |
| DB 一致性 | `message.tokens` 与 `step-finish.tokens` 1432/1432 全等；step 数分布 `{1:1432, 0:3}`；`turn_count` 合计 1435；`message.cost` 1435 行全 0、`part.cost` 1432 行全 0；`session_prefix_snapshot.revision` 13 行全 1 |
| DB 时间 | 两个时间口径的起止（`…06:17:40.337Z` / `…11:22:19.594Z`）与 386 ms 差值 |
| asar | 91,502,971 B 与 `split("\n")` 900,895 行；行号 833556 / 104267 / 104268 / 714299；7 个文件的大小（37,229,974 / 127 / 742 / 621 / 2,429,461 / 405 / 167）；8 个引擎特征串的逐个计数 |
| 上游源码 | `message-v2.ts` 的 373-391 / 540-549 / 555-570 / 600-608 / 611-620 / 630；`session.ts` 385-448（含 415-428 的 tokens 构造与 434-448 的 cost 计算）；`provider.ts` 1366-1373；`project.ts` 200；`snapshot/index.ts` 86；`tool/truncation-dir.ts` 4；`storage/storage.ts` 198；`storage/db.ts` 31-44 与 92-97；`processor.ts` 654；`overflow.ts` 104。**两次不同 commit 的克隆中以上行号均零偏移**（文件总行数差 1） |
| 文件系统 | 引擎日志 7 份 / 外壳日志 7 份；`artifacts.db` 585,728；`rolechat.db` 与 `session-review.db` 各 20,480；`models-with-claude.json` 4,581,473；`models.json` 5,282,225；`onetrack-queue.jsonl` 0；额度扫描的 4052 文本 / 28 二进制口径 |

### 13.3 本轮修正（8 处，均为精度、量纲或表述问题）

| # | 位置 | 原表述 | 修正为 |
| --- | --- | --- | --- |
| 1 | §3.1 | `builtin_skills` 6.0 MB | **6.27 MB**（6,265,420 B，原为 MiB 口径残留） |
| 2 | §3.1 | `snapshot` 4.6 MB | **4.84 MB**（4,835,671 B） |
| 3 | §3.2 | `git-version-store` 2.0 MB | **2.12 MB**（2,121,149 B） |
| 4 | §12.2 | "asar 内另有 927 个 `node_modules` 条目（未压缩约 376 MB）" | **769 文件 + 158 目录**；376.2 MB 是**跨平台清单逻辑大小**，本机 unpacked 实际落盘仅 **82 文件 / 66.5 MB**，asar 本体 91.0 MB |
| 5 | §4.1 | `memory_fts*` 等一律 0 | 区分**主表 0** 与 FTS5 辅助表（`_config` / `_data` 有行）；补 `history_index_migration`（5 行）与 `event` / `event_sequence` / `session_share` / `account_state` |
| 6 | §5.5 / §6 | 额度扫描"0 处落盘" | **296 个文本文件含关键词**，逐类核查后均为无关文本（LibreOffice UI / 技能文档 / 模型描述）；二进制与库 0 命中 |
| 7 | §3.1 | 引擎日志"3 行含关键词均为错误体" | 精确到关键词：7 个用量类关键词 **0 次**，`output_tokens` 的 2 次落在**系统提示词正文**，`max_tokens` 落在错误体请求参数 |
| 8 | 文档头 | （未声明量纲） | 明示"文件大小一律十进制" |

**本轮未发现结论性错误**：§1–§7 的因果判断、§11 的源码引用、§12 的打包与接线关系
均通过独立重算保持不变。修正项全部属于精度与完整性层面。

### 13.4 仍然存在的复核边界

- 所有"已验证"项都只覆盖**本机这一个快照**（9 会话 / 1435 调用 / 单账号 / 单版本）。
- **进程模型**（引擎是同进程 import 还是独立进程）仍未做运行时验证——核验期间 MiMo 未运行。
- asar 内 35 MB+ 的 `node.mjs` 只做了**特征串与片段级**核验，未逐行审读；
  §5.4 与 §12 的结论都限定在所引片段的范围内。
- 上游源码只读了**与用量/存储/接线相关**的文件，未通读仓库；`packages/plugin` 与
  `packages/sdk` 未展开。
