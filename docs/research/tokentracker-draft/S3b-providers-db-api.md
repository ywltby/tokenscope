# S3b — TokenTracker 数据库 / API / 快照型 provider 解析器研究

- 样本：`https://github.com/xiufengsun/TokenTracker`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，版本 1.3.14
- 本地样本路径：`C:\Users\admin\AppData\Local\Temp\tt-research`（只读研究，未做任何修改）
- 覆盖范围：任务书指定的 **23 个** 解析器函数名（`parseOpencodeDbIncremental` … `parseCommandCodeIncremental`）；其中 `parseOmpLikeIncremental`（`rollout.js:15753`）与 `parsePiLikeIncremental`（`rollout.js:16319`）是 2 个**共享实现**，实际对应 **21 个独立数据源实体**（下称「家」，= §20.1 表 19 行 + §20.2 表 2 行）
- 本文所有行号均为相对仓库根路径的行号；代码引用为逐字摘录（每次 ≤15 行）

**结论摘要（先看这 5 条）**

1. 21 家中有 **2 家必须联网且必须读取本机登录凭据**才能产出任何数据：`cursor`（Cursor API，凭据取 `state.vscdb` 的 `cursorAuth/accessToken`）、`trae-cn`（TRAE 官方 usage API，凭据取 `storage.json` 的加密 blob，且需显式 opt-in）。其余 19 家全部是纯本地文件/本地库读取，**离线可用**。
2. 真正把 SQLite 当数据源的有 **7 项**（`copilot` 拆两份，口径见 §20.4）：`opencode`(opencode.db)、`qoder`(local.db)、`devin`(sessions.db)、`copilot`（两份：`session-store.db` + `data.db`）、`antigravity`(conversations/<uuid>.db)、`trae`(SQLCipher 加密 database.db)；`cursor` 另有 SQLite（`state.vscdb`）**仅用于取凭据**，用量本身来自远程 CSV，不计入数据源类型。其中 **Trae 是唯一一家绕开系统 sqlite3/`node:sqlite`、用 WASM（`@journeyapps/wa-sqlite`）解密 + 读内存快照**的实现。
3. 共用的 SQLite 读取层 `src/lib/sqlite-reader.js` **只依赖系统 `sqlite3` CLI（`-readonly -json`）与 Node 22+ `node:sqlite`** 两级回落；**生产代码 `src/**` 不使用 `sql.js`、`better-sqlite3`**（`src/**` 命中 0）。注意 `sql.js@1.14.2` 与 `@electric-sql/pglite` 确实存在于 **devDependencies**（仅测试用；见 §1.3 反证）。
4. **把非 token 计费单位当 token 的只有一家半**：`qoder` 新 JSONL 通道把 `credits` 字段原样带进 totals（`rollout.js:6541`），但**不换算、不计入 total**；`reasonix` 用 `requestCount` 当 `conversation_count`（`rollout.js:17044`），是请求计数冒充会话计数，不是 token。**Grok 估算路径**（`rollout.js:19945`）把上下文窗口 token 按下 0.8 硬拆 input/output，属于 token→token 的猜测式拆分，不涉及 credits 换算。
5. **只填 `total_tokens` 而 input/output 为 0 的唯一一家是 `antigravity`** 的字符估算分支——不，反证见 §26.1：Antigravity 的估算分支同时写 input 与 output；真正会出现「`conversation_count=1` 且全部 token 列为 0」的行有 **两处**：**`qoder` 新 JSONL 的 billable 零 token 分支**（`rollout.js:6639-6660`）与 **`trae-cn` 的零值行**（`rollout.js:22477-22492` 只校验 ≥0、`22500-22508` 写全零 totals、`22749-22751` 无护栏落桶）。`craft` **不会**产生该形态：`rollout.js:17199-17212` 有四分量全零 `continue` 护栏（`17243` 的 `conversation_count: prev.total === 0 ? 1 : 0` 只在至少一个分量 > 0 时执行）。逐条证据见 §22.1 与 §26.12。

---

## 1. 共用基础设施（21 家共享的前提）

### 1.1 数据流与队列行形状

每个 provider 解析器都不直接产出 JSON 文件，而是把「本家被触碰的半小时桶」写进队列：

```js
// src/lib/rollout.js:4222-4226
function bucketKey(source, model, hourStart) {
  const safeSource = normalizeSourceInput(source) || DEFAULT_SOURCE;
  const safeModel = normalizeModelInput(model) || DEFAULT_MODEL;
  return `${safeSource}${BUCKET_SEPARATOR}${safeModel}${BUCKET_SEPARATOR}${hourStart}`;
}
```

- `BUCKET_SEPARATOR = "|"`：`src/lib/rollout.js:57`
- `DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`：`src/lib/rollout.js:55-56`
- 桶键三段解析（`source|model|hourStart`）在 `parseBucketKey`：`src/lib/rollout.js:4238-4252`

累加器（所有 provider 最终都落到这里）：

```js
// src/lib/rollout.js:4099-4115（节选）
function addTotals(target, delta) {
  for (const [field] of PRIORITY_SUBSET_FIELDS) { ... }
  target.input_tokens += delta.input_tokens || 0;
  target.cached_input_tokens += delta.cached_input_tokens || 0;
  target.cache_creation_input_tokens += delta.cache_creation_input_tokens || 0;
  target.output_tokens += delta.output_tokens || 0;
  target.reasoning_output_tokens += delta.reasoning_output_tokens || 0;
  target.total_tokens += delta.total_tokens || 0;
  target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0;
  target.total_cost_usd = Math.round(
    ((target.total_cost_usd || 0) + (delta.total_cost_usd || 0)) * USD_TICKS_PER_USD,
  ) / USD_TICKS_PER_USD;
  target.conversation_count += delta.conversation_count || 0;
}
```

`initTotals()` 的九个字段即队列行 schema：`src/lib/rollout.js:4053-4065`。

**重要口径**：`billable_total_tokens ?? total_tokens` 表示**没写 `billable_total_tokens` 的 provider 会用 `total_tokens` 兜底**（`rollout.js:4110`）。`PRIORITY_SUBSET_FIELDS` 只做分段标注、永不参与求和：`rollout.js:4067-4075`。

### 1.2 时间戳与半小时桶

```js
// src/lib/rollout.js:4173-4190（节选）
function toUtcHalfHourStart(ts) {
  const dt = new Date(ts);
  if (!Number.isFinite(dt.getTime())) return null;
  const minutes = dt.getUTCMinutes();
  const halfMinute = minutes >= 30 ? 30 : 0;
  const bucketStart = new Date(Date.UTC(dt.getUTCFullYear(), dt.getUTCMonth(), dt.getUTCDate(),
    dt.getUTCHours(), halfMinute, 0, 0));
  return bucketStart.toISOString();
}
```

- 一律 **UTC**，秒与毫秒被清零 → `hour_start` 形如 `YYYY-MM-DDTHH:00:00.000Z` / `...:30:00.000Z`。`trae-cn` 把这个形状当不变式校验：`rollout.js:22372-22377`。
- 秒/毫秒自适应：`coerceEpochMs` 中 `< 1e12` 视为秒并乘 1000（`rollout.js:4998-5003`）。

### 1.3 SQLite 读取层：生产路径只有两级后端（`sql.js` 仅存在于测试）

```js
// src/lib/sqlite-reader.js:33-45（节选）
function readSqliteRowsWithCli(dbPath, sql, { execFileSync, timeout, maxBuffer, readOnly }) {
  const args = readOnly ? ["-readonly", "-json", dbPath, sql] : ["-json", dbPath, sql];
  const raw = execFileSync("sqlite3", args, { windowsHide: true, encoding: "utf8",
    maxBuffer, timeout, stdio: ["pipe", "pipe", "pipe"] });
  if (!raw || !raw.trim()) return [];
  const rows = JSON.parse(raw);
  return Array.isArray(rows) ? rows : [];
}
```

```js
// src/lib/sqlite-reader.js:67-79（节选）
function readSqliteRowsWithNode(dbPath, sql, { requireFn }) {
  const { DatabaseSync } = requireFn("node:sqlite");
  ...
  const db = new DatabaseSync(dbPath, { readOnly: true });
  try {
    const rows = db.prepare(sql).all();
    return Array.isArray(rows) ? rows : [];
  } finally { db.close(); }
}
```

- 同步版 `readSqliteJsonRows`：`sqlite-reader.js:81-128`；异步孪生 `readSqliteJsonRowsAsync`（CLI 走 `execFile`，`node:sqlite` 回落仍是同步）：`sqlite-reader.js:148-195`。
- 默认 `timeout = 30_000`、`maxBuffer = 50 * 1024 * 1024`：`sqlite-reader.js:91-92`（异步版同值：`:158-159`）。
- 两级都不可用时**只打印一次**警告（进程级 Set 去重）：`sqlite-reader.js:17-31`；只有调用方传 `throwOnReadFailure` 才抛错：`:119-126`。
- **反证「生产代码不使用 sql.js / better-sqlite3」**：全仓 `src/**` 命中 `sql.js` / `better-sqlite3` 的次数为 0；`package.json` 的 `dependencies` 只有 `@journeyapps/wa-sqlite`、`@mongodb-js/zstd`、`smol-toml`、`undici`、`yauzl`（`engines.node >= 20`）。因此**「生产路径的 SQLite 打开方式」等价于「sqlite3 CLI 或 node:sqlite」**（Trae 例外是 wa-sqlite WASM 引擎，见 §20.3）。
- **devDependencies 里确实还有两个内嵌引擎（原稿遗漏，R1 修正）**：`sql.js@1.14.2`（`package.json:67`；被 5 个测试 `require`：`test/sync-trae.test.js:9`、`test/trae-db.test.js:7`、`test/trae-snapshot.test.js:6`、`test/trae-sqlite.test.js:3`、`test/trim-embedded-deps.test.js:8`，用于构造 Trae SQLCipher fixture；`trim-embedded-deps.test.js:21` 还用 wa-sqlite 跑 TRAE reader）与 `@electric-sql/pglite@^0.5.8`（`package.json:61`；12 个云端计费/配额测试与 helper 引用）。二者均不进生产路径，但「仓库不依赖 `sql.js`」的原表述不成立。
- 文件锁/只读事务：CLI 路径靠 `-readonly` 参数；`node:sqlite` 靠 `{ readOnly: true }`。**没有显式 `BEGIN`**，一致性靠「WAL 边车指纹 + 快照复制 + 读后重算指纹」实现（见各家的 `sqliteSidecarFingerprint`）。
- **UNC/WSL 例外**：`\\wsl$` / `\\wsl.localhost` 上的库直接开会被 Plan 9/SMB 桥拒锁（注释：`rollout.js:7693-7696`），所以各家先 `snapshotSqliteDb(dbPath)` 复制到本地临时目录再读。命中该分支的至少包括 opencode（`rollout.js:5194-5199`）、kiro（`:7315-7320`）、devin（`:14184-14189`）、antigravity（`:21573-21581`）、copilot store（`:18681`）。

### 1.4 zstd 与 wa-sqlite 的真实用途（防止误读依赖）

- `@mongodb-js/zstd`：**只服务两家**——Zed 线程 blob（`rollout.js:12514-12533`，本任务范围外）与 **DSH 会话日志**（`rollout.js:23066-23072`）；DSH 优先用 Node 内置 `zlib.zstdDecompressSync`，只在 Node 20 回落该依赖。
- `@journeyapps/wa-sqlite`：**只服务 Trae**（`src/lib/trae-sqlite.js:7-15`）。
- 全仓 **没有任何 LZ4 代码路径**；`Zed` 的 zstd 列治理方式见 `rollout.js:12526-12533`（`data_type === "zstd"` 时才解压）。

### 1.5 身份、跨安装与 source 归属

- `source` 最终以小写形态进桶键：`normalizeSourceInput`（`rollout.js:4254-4258`）。
- 账号级来源集合（用于「仅本地」视图过滤）：`src/lib/source-metadata.js:1` `ACCOUNT_LEVEL_SOURCES = new Set(["cursor", "trae-cn"])`。
- `sync.js` 的 source 白名单（是否被 `--source` 允许跑）：`src/commands/sync.js:320-366`；别名表 `src/commands/sync.js:310-319`。
- 后台轻量自动同步只跑 4 家：`src/commands/sync.js:367-373`（`acode`、`codex`、`every-code`、`reasonix`）。

---

## 2. `parseOpencodeDbIncremental` — OpenCode SQLite

**1) source 标识**：`"opencode"`。

```js
// src/lib/rollout.js:5788-5790（节选）
const cursorNamespace = typeof cursorKey === "string" && cursorKey.length > 0 ? cursorKey : "opencode";
...
const defaultSource = normalizeSourceInput(source) || "opencode";
```

调用点未覆盖 `source`，因此默认值生效：`src/commands/sync.js:1375-1381`。

**2) 数据来源类型**：SQLite。库路径 `path.join(dbDir, "opencode.db")`（`src/commands/sync.js:1369`），其中 `dbDir = process.env.OPENCODE_HOME || path.join(xdgDataHome, "opencode")`（`sync.js:1348-1355`），Windows 下另探 WSL 发行版家目录（`sync.js:1349-1351`）。磁盘 `.json` 消息目录与 DB 并行解析（`sync.js:1360-1365`，走 `parseOpencodeIncremental`，不在本文范围）。

**3) SQLite 细节**：四种库形态在源码注释里明确列举——

```js
// src/lib/rollout.js:5041-5045
//   A  pure v1:        `message`(data) + `session_message`(empty) [+ `session`]
//   B  beta-17887:     `session_message`(data) + `session_v2`
//   C  upstream v2:    `session_message`(data) + `session`
//   D  transitional:   `message`(old data) + `session_message`(new data)
```

查询原文（v1）：

```js
// src/lib/rollout.js:5108-5113
function buildV1Sql(watermark = null) {
  return (
    `SELECT rowid AS row_id, id, session_id, time_updated, data FROM message ` +
    `WHERE json_extract(data, '$.role') = 'assistant'` +
    `${opencodeDbIncrementalPredicate("", watermark)} ORDER BY time_created ASC`
  );
}
```

查询原文（v2，`sessionTable` 由探测决定）：

```js
// src/lib/rollout.js:5120-5133（节选）
function buildV2Sql(sessionTable, watermark = null) {
  const joinClause = sessionTable ? `LEFT JOIN ${sessionTable} s ON s.id = sm.session_id ` : "";
  ...
  `FROM session_message sm ${joinClause}` +
  `WHERE sm.type = 'assistant'` + ... ` ORDER BY sm.time_created ASC`
}
```

布局探测一次往返同时回答「v2 有无数据」和「哪张 session 表存在」：

```js
// src/lib/rollout.js:5143-5147（节选）
rows = readSqliteJsonRows(dbPath,
  `SELECT (SELECT 1 FROM session_message LIMIT 1) AS hasRows,
          (SELECT name FROM sqlite_master WHERE name IN ('session','session_v2') ORDER BY name DESC LIMIT 1) AS sessionTable`,
  { label: "OpenCode", timeout: 10_000, maxBuffer: 1024 * 1024, ...sqliteOptions, throwOnReadFailure: true });
```

**无压缩列**（`session_message.data` / `message.data` 是纯 JSON 文本，`JSON.parse(row.data)`：`rollout.js:5215`）。

**4) 远程 API**：无。

**5) 字段映射**（逐字）：

```js
// src/lib/rollout.js:4755-4772
function normalizeOpencodeTokens(tokens) {
  if (!tokens || typeof tokens !== "object") return null;
  const input = toNonNegativeInt(tokens.input);
  const output = toNonNegativeInt(tokens.output);
  const reasoning = toNonNegativeInt(tokens.reasoning);
  const cached = toNonNegativeInt(tokens.cache?.read);
  const cacheWrite = toNonNegativeInt(tokens.cache?.write);
  const total = input + output + reasoning + cached + cacheWrite;
  return { input_tokens: input, cached_input_tokens: cached,
    cache_creation_input_tokens: cacheWrite, output_tokens: output,
    reasoning_output_tokens: reasoning, total_tokens: total };
}
```

即 `tokens.input` 已经是**纯非缓存输入**（不做减法），`reasoning` **不并入 output**，`total` 由五个分量相加得到（不取上游字段）。

**6) 模型名与多模型**：`normalizeOpencodeModelFields` 同时兼容 v1 扁平字段与 v2 嵌套对象：

```js
// src/lib/rollout.js:4272-4290（节选）
const directModel = normalizeModelInput(msg?.modelID)
  || normalizeModelInput(typeof msg?.model === "string" ? msg.model : null)
  || normalizeModelInput(msg?.modelId);
if (directModel) { return { modelId: directModel, providerId: ... }; }
const nested = msg?.model;
if (nested && typeof nested === "object") { return { modelId: normalizeModelInput(nested.id), ... }; }
```

桶内模型取 `dbModelId || DEFAULT_MODEL`（`rollout.js:5905-5906`）。**不为多模型拆分**——一条 assistant 消息只落一个模型桶。

**7) 时间戳**：`msg.time.completed` 优先，缺失回落 `msg.time.created`：

```js
// src/lib/rollout.js:5892-5903（节选）
const timestampMs = coerceEpochMs(msg?.time?.completed) || coerceEpochMs(msg?.time?.created);
if (!timestampMs) { messagesProcessed += 1; continue; }
const tsIso = new Date(timestampMs).toISOString();
const bucketStart = toUtcHalfHourStart(tsIso);
```

无兜底到文件 mtime/当前时间——**无时间戳即丢弃**。

**8) 增量/去重**：三层。

- 库级水位：`cursors.<namespace>.dbCursor`，由 `readOpencodeDbMessagesIncremental` 维护，形如 `{ version: 1, v1: {maxRowId, maxUpdatedAt, anchor}, v2: {...} }`（`rollout.js:5054-5062`、`5314-5324`）。
- 谓词：`rowid > maxRowId OR time_updated >= maxUpdatedAt`（`rollout.js:5097-5106`，注释说明重读边界时间戳以防并发漏读，重复由 `messageIndex` 消掉）。
- 头部锚点防「库被原地替换」：读前读后各取一次 `MAX(rowid)` 头部行的 sha256 锚（`opencodeDbRowAnchor`，`rollout.js:5088-5095`；比对逻辑 `5287-5308`），不一致则强制重放一次；两次仍不稳定就返回行但**不推进游标**（`5327-5328`）。
- 消息级：`deriveOpencodeMessageKey` + #426 fork 指纹（`rollout.js:5847-5888`），fork 拷贝会被回撤并落墓碑 `dedupedForkCopy: true`。
- 多安装：`opencodeCursorNamespace` 在双安装时取安装键（`"native"`/`"wsl"`），单安装为 `"flat"`（`sync.js:1429`）。

**9) 特殊规则**：

- 差值语义是**带符号差值**而非仅累加：`diffOpencodeTotals` 明确允许负差值（校正），`rollout.js:4815-4836`。
- `conversation_count`：首次见该 messageKey 记 1，已有配额记 0（`rollout.js:5969-5971`）。
- 归属变动（模型或半小时桶变化的「搬运」）会先 `subtractCountedOpencodeMessage` 再整份加回（`rollout.js:5958-5971`）。
- 无 `billable_total_tokens`（靠 `addTotals` 的 `?? total_tokens` 兜底，见 §1.1）。
- 另有 `0.8` 之外的聚类逻辑（`unknown` 模型对齐到主导模型）在 `enqueueTouchedBuckets`：`rollout.js:3262-3348`。

---

## 3. `parseQoderDbIncremental` — Qoder 旧版 SQLite

**1) source 标识**：默认 `"qoder"`；CN 版通过参数传入 `"qoder-cn"` 复用同一函数（见下）。

```js
// src/lib/rollout.js:6218-6227
async function parseQoderDbIncremental({
  dbMessages,
  dbPath,
  cursors,
  queuePath,
  projectQueuePath,
  onProgress,
  publicRepoResolver,
  sourceKey = "qoder",
  cursorKey = "qoder",
} = {})
```

**2) 数据来源类型**：SQLite。路径解析优先级：`QODER_DB_PATH` → `QODER_HOME` → 平台默认。

```js
// src/lib/rollout.js:6064-6077（节选）
if (platform === "darwin") { root = path.join(home, "Library", "Application Support", appDir); }
else if (platform === "win32") { root = path.join(env.APPDATA || path.join(home, "AppData", "Roaming"), appDir); }
else { root = path.join(home, ".config", appDir); }
return path.join(root, "SharedClientCache", "cache", "db", "local.db");
```

CN 版另用 `QODER_CN_DB_PATH` / `QODER_CN_HOME`（`rollout.js:6112-6127`），注释说明**刻意不复用国际版的 `QODER_HOME`**，否则两个库会指向同一文件双计。

**3) SQLite 细节**：三表 JOIN，SQL 原文：

```sql
-- src/lib/rollout.js:6030-6050
SELECT
  cm.rowid AS row_id, cm.id, cm.session_id, cm.request_id, cm.token_info, cm.model_info,
  cm.gmt_create, cr.extra AS record_extra, cs.preferred_model_info, cs.project_uri, cs.project_name
FROM chat_message AS cm
LEFT JOIN chat_record AS cr ON cr.request_id = cm.request_id
LEFT JOIN chat_session AS cs ON cs.session_id = cm.session_id
WHERE cm.role = 'assistant' AND cm.token_info IS NOT NULL
  AND trim(cm.token_info) NOT IN ('', '{}')
ORDER BY cm.gmt_create, cm.rowid
```

读取走异步版（热路径不阻塞事件循环）：`readSqliteJsonRowsAsync(dbPath, QODER_USAGE_SQL, { label: "Qoder", ... })`（`rollout.js:6129-6135`）。无压缩列。

**4) 远程 API**：无。

**5) 字段映射**（关键：上游 `prompt_tokens` 含缓存，必须减）：

```js
// src/lib/rollout.js:6164-6178（节选）
// Qoder's prompt_tokens already includes cached_tokens. Keep cached input in
// its own column and report only the remainder as ordinary input, otherwise
// cached context is counted twice in dashboards and cost calculations.
const input = Math.max(0, Math.trunc(prompt) - Math.trunc(cached));
const cachedInput = Math.min(Math.trunc(prompt), Math.trunc(cached));
const output = Math.trunc(completion);
return { input_tokens: input, cached_input_tokens: cachedInput,
  cache_creation_input_tokens: 0, output_tokens: output,
  reasoning_output_tokens: 0, total_tokens: Math.trunc(prompt) + output,
  billable_total_tokens: Math.trunc(prompt) + output };
```

来源字段：`token_info.prompt_tokens` / `token_info.cached_tokens` / `token_info.completion_tokens`（`rollout.js:6151-6153`）。**该家写 `billable_total_tokens`**，且取 `prompt + output`（含缓存）。

**6) 模型名**：三级回落，最后落到常量 `"qoder-agent"`：

```js
// src/lib/rollout.js:6185-6195（节选）
return (
  normalizeModelInput(direct?.model_key || direct?.modelKey) ||
  normalizeModelInput(recordExtra?.modelConfig?.key || recordExtra?.model_config?.key) ||
  normalizeModelInput(preferred?.model_key || preferred?.modelKey
    || preferred?.preferred_model || preferred?.preferredModel) ||
  "qoder-agent"
);
```

**7) 时间戳**：`gmt_create` 经 `coerceEpochMs`（`rollout.js:6263-6266`）；无时间戳即跳过（同时跳过无 key/无 tokens 的行，`6267-6270`）。

**8) 增量/去重**：**全量快照 + 幂等重算**（不是水位）。

- 游标 `cursors.<cursorKey>.messages[messageKey] = { totals, conversationCount, requestKey, bucketStart, model, projectKey, projectRef, updatedAt }`（`rollout.js:6352-6361`）。
- 每次同步重算全部行，未变（`totalsKey` + bucketStart + model + projectKey 全等）则跳过，否则**先减旧桶再加新桶**（`rollout.js:6307-6363`）。
- 跨设备/多安装：`rowid` 会把两个库的行撞在一起，注释明确用独立 cursor 命名空间隔离（`rollout.js:6114-6119`）。
- `messageKey` = `session_id|id`，缺失回落 `row:<rowid>`（`rollout.js:6198-6205`）。

**9) 特殊规则**：

- `conversation_count` 由**整份有序快照重算的 request 归属**决定，而不是沿用上次的归属：

```js
// src/lib/rollout.js:6277-6286（节选）
// Recompute request ownership from the complete ordered DB snapshot on
// every pass. Qoder can attach token_info to an earlier assistant row only
// after a later row was already counted; retaining the old per-message
// owner in that case inflates one request to two conversations.
const conversationCount = requestOwners.get(requestKey) === messageKey ? 1 : 0;
```

- `reasoning_output_tokens` 恒为 0（Qoder 不上报）。
- `cache_creation_input_tokens` 恒为 0。
- 项目归属：`project_uri` 以 `file://` 开头时做一次 `decodeURIComponent(new URL(raw).pathname)`（`rollout.js:6207-6216`）。

---

## 4. `parseQoderNewIncremental` — Qoder 新版 JSONL（credit 计费 SDK）

**1) source 标识**：`"qoder"`（与旧版**同名同源**，靠 messageKey 前缀 `jsonl:` 与 `row:` 隔离游标）。

```js
// src/lib/rollout.js:6552-6560（节选）
async function parseQoderNewIncremental({
  sessionFiles, cursors, queuePath, projectQueuePath, onProgress,
  sourceKey = "qoder", cursorKey = "qoderNew", publicRepoResolver,
} = {})
```

`rollout.js:6412-6414` 明确写下这一约定：「both sources now use distinct cursor namespaces (qoder vs qoderNew) via disjoint messageKey prefixes (row: vs jsonl:) but upload under the same source="qoder"」。

**2) 数据来源类型**：自有 JSONL 快照。目录：

```js
// src/lib/rollout.js:6432 / 6454
return path.join(home, ".qoder", "projects");      // 国际版
return path.join(home, ".qoder-cn", "projects");   // CN 版
```

覆盖变量 `QODER_PROJECTS_DIR` / `QODER_CN_PROJECTS_DIR`（`rollout.js:6417-6419`、`6436-6438`）。文件为 `projects/**/*.jsonl`（`listQoderNewSessionFiles`，`rollout.js:6457-6474`）。

**3) SQLite**：不涉及（旧 `local.db` 作为 legacy fallback 另由 §3 处理）。

**4) 远程 API**：无。

**5) 字段映射**：**这是全项目唯一把 `credits` 写进 totals 的地方**。

```js
// src/lib/rollout.js:6526-6549（节选）
// Only rows with authoritative token fields are counted; anything else
// falls through to null (unsupported, no token delta).
if (input > 0 || cached > 0 || cacheCreation > 0 || output > 0) {
  const inp = Math.max(0, Math.trunc(input)); ...
  return { input_tokens: inp, cached_input_tokens: cach,
    cache_creation_input_tokens: cc, output_tokens: out,
    reasoning_output_tokens: 0, total_tokens: inp + cach + cc + out,
    billable_total_tokens: inp + cach + cc + out,
    credits: Number.isFinite(credits) && credits > 0 ? credits : 0,
    usage_precision: null };
}
// Credit-only usage without authoritative token fields is intentionally
// not counted (no token delta): there is no first-party evidence for a
// credit→token rate.
return null;
```

- 原始字段：`usage.input_tokens`、`usage.cache_read_input_tokens ?? usage.cached_tokens`、`usage.cache_creation_input_tokens`、`usage.output_tokens`、`usage.credits ?? usage.original_credits`（`rollout.js:6515-6519`）。
- **`credits` 不参与任何换算**，只被原样挂到 totals 上（后续 `addTotals` 不识别该字段，因此不会进桶 totals 的数值列）。源码注释同时声明成本永不估算（`rollout.js:6405-6411`）。
- **`reasoning_output_tokens` 恒 0**。
- **注意 `input_tokens` 不做 cache 扣减**（与 §3 语义不同）。

**6) 模型名与多模型**：`message.model`，且剥掉 CN BYOK 的安装本地 UUID 前缀：

```js
// src/lib/rollout.js:6479-6487（节选）
// CN BYOK routes embed an install-local provider UUID in the model id
// ("qoder-custom-<uuid>/glm-5.3-flash"). Keep the bare model id so bucket
// keys stay stable across reinstalls ...
const stripped = direct.replace(
  /^qoder-custom-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\//i, "");
return normalizeModelInput(stripped) || "qoder-agent";
```

**7) 时间戳**：`record.timestamp` 或 `record.message.timestamp`，可解析 ISO 串，否则 0 即丢弃（`rollout.js:6505-6511`、`6632-6633`）。

**8) 增量/去重**：**集合差量**——每次重建 `currentByKey`，与游标逐键比对：

```js
// src/lib/rollout.js:6701-6711（节选）
// Subtract contributions that disappeared or changed
for (const [key, prev] of Object.entries(qoderState.messages)) {
  if (!key.startsWith("jsonl:")) continue;
  const cur = currentByKey.get(key);
  const unchanged = cur && totalsKey(prev.totals) === totalsKey(cur.totals)
    && prev.bucketStart === cur.bucketStart && prev.model === cur.model
    && (prev.projectKey || null) === (cur.projectKey || null)
    && (prev.totals?.total_cost_usd || 0) === (cur.totals.total_cost_usd || 0);
  if (unchanged) continue;
  ...subtractTotals(oldBucket.totals, prev.totals)...
```

随后 6751-6773 补加新键。一次性迁移把旧的 `qoder` 游标里的 `jsonl:` 条目搬到 `qoderNew`（`rollout.js:6564-6590`）。

**9) 特殊规则（含风险行）**：

- **billable 零 token 分支会产出 `conversation_count=1` 且 token 全 0 的行**：

```js
// src/lib/rollout.js:6629-6660（节选）
// Allow billable zero-token messages to still count conversation (no token delta)
const isBillable = usage.billable !== false;
if (!base && !isBillable) continue;
...
} : {
  input_tokens: 0, cached_input_tokens: 0, cache_creation_input_tokens: 0,
  output_tokens: 0, reasoning_output_tokens: 0, total_tokens: 0,
  billable_total_tokens: 0, total_cost_usd: 0, conversation_count: 1,
};
```

- `total_cost_usd: 0` 显式写入（`rollout.js:6647`），与注释中「cost stays 0」自洽。
- 只解析 `record.type === "assistant"` 且 `message.role === "assistant"` 的行（`rollout.js:6622-6624`）。

---
## 5. `parseCursorApiIncremental` — Cursor（账号级远程 API）

**1) source 标识**：`"cursor"`（调用点显式传入，默认值同值）。

```js
// src/lib/rollout.js:7172-7180（节选）
async function parseCursorApiIncremental({ records, cursors, queuePath, onProgress, source }) {
  await ensureDir(path.dirname(queuePath));
  const defaultSource = normalizeSourceInput(source) || "cursor";
```

调用点：`src/commands/sync.js:2173-2187`（`source: "cursor"`）。

**2) 数据来源类型**：**远程 HTTP API**（CSV 导出）+ **SQLite 取凭据**。解析器本身不碰网络——它接收的是上游已解析好的 CSV 记录：

```js
// src/lib/rollout.js:7164-7170
/**
 * Incremental parser for Cursor usage data fetched via API.
 *
 * Unlike other parsers that read local files, this one receives pre-parsed
 * CSV records from cursor-config.js and aggregates them into 30-min buckets.
 *
 * Incremental state is tracked in `cursors.cursorApi.lastRecordTimestamp`.
 */
```

**3) SQLite（仅凭据）**：库路径 `Cursor/User/globalStorage/state.vscdb`（`src/lib/cursor-config.js:28-32`），SQL：

```js
// src/lib/cursor-config.js:46-47
const CURSOR_ACCESS_TOKEN_SQL =
  "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken';";
```

取值走 `readSqliteFirstValue`（即 §1.3 的 CLI/`node:sqlite` 两级后端），`timeout: 5000`、`maxBuffer: 1MB`（`cursor-config.js:56-66`）。

**4) 远程 API**：端点与鉴权（逐字）：

```js
// src/lib/cursor-config.js:150-152
const CURSOR_CSV_URL = "https://cursor.com/api/dashboard/export-usage-events-csv?strategy=tokens";
const CURSOR_SUMMARY_URL = "https://cursor.com/api/usage-summary";
const CURSOR_RPC_ORIGIN = "https://api2.cursor.sh";
```

Cookie 组装（凭据三来源合一）：

```js
// src/lib/cursor-config.js:109-111
  // 3. Build cookie
  const cookie = `WorkosCursorSessionToken=${userId}%3A%3A${jwt}`;
  return { cookie, userId, accessToken: jwt };
```

- `jwt` ← `state.vscdb`；`userId` ← `.cursor/cli-config.json` 的 `authInfo.authId`，失败则从 JWT 解出（`cursor-config.js:102-107`）。
- WorkOS 主体前缀白名单：`cursor-config.js:114-116`。
- 超时：CSV 与 summary 都是 `30_000`，RPC 为 `15_000`（`cursor-config.js:196`、`231`、`367`）。
- 失败降级：调用点整段 `try/catch`，仅打印一行错误、**不影响其它 provider**；未安装 Cursor 或取不到凭据则整段跳过（`sync.js:2158-2194`）。
- **无「仅本地估算」开关**——CSV 拿不到就是没有数据。

**5) 字段映射**：CSV 列名 → 记录字段（按表头名解析，不按固定列序）：

```js
// src/lib/cursor-config.js:463-499（节选）
  const inputWithIdx = columnIndex.get("Input (w/ Cache Write)");
  const inputWithoutIdx = columnIndex.get("Input (w/o Cache Write)");
  const cacheReadIdx = columnIndex.get("Cache Read");
  const outputIdx = columnIndex.get("Output Tokens");
  ...
      inputTokens: inputWithoutCache,
      cacheWriteTokens: Math.max(0, inputWithCache - inputWithoutCache),
      cacheReadTokens: toNum(fields[cacheReadIdx]),
      outputTokens: toNum(fields[outputIdx]),
      totalTokens: toNum(fields[totalIdx]),
      cost: toFloat(fields[costIdx]),
```

归一化：

```js
// src/lib/cursor-config.js:512-524（节选）
function normalizeCursorUsage(record) {
  const inputTokens = Math.max(0, Math.floor(record.inputTokens || 0));
  const cacheWrite = Math.max(0, Math.floor(record.cacheWriteTokens || 0));
  const cacheRead = Math.max(0, Math.floor(record.cacheReadTokens || 0));
  const outputTokens = Math.max(0, Math.floor(record.outputTokens || 0));
  const totalTokens = inputTokens + outputTokens + cacheWrite + cacheRead;
  return { input_tokens: inputTokens, cached_input_tokens: cacheRead,
    cache_creation_input_tokens: cacheWrite, output_tokens: outputTokens,
    reasoning_output_tokens: 0, total_tokens: totalTokens, billable_total_tokens: totalTokens };
```

- **`total_tokens` 由四分项相加**，不取 CSV 的 `Total Tokens` 列（该列只用于判空：`cursor-config.js:501`）。
- `reasoning_output_tokens` 恒 0。
- `kind`/`Max Mode` 两列只用于 `billableKind` 标记，**不改变 token 数**（`cursor-config.js:489-492`、`540-546`）。

**6) 模型名**：CSV `Model` 列（`cursor-config.js:487`）→ `rollout.js:7237` `normalizeModelInput(record.model) || DEFAULT_MODEL`。**每条记录独立计数、天然多模型**。

**7) 时间戳**：CSV `Date` 列字符串直接进 `toUtcHalfHourStart`（`rollout.js:7204`、`7234`）。`Date` 缺失即跳过（`rollout.js:7226`）。

**8) 增量/去重**：**账号级快照替换**，这是与所有本地 provider 的根本差异：

```js
// src/lib/rollout.js:7184-7187（节选）
  // Cursor's CSV is an account-level API export, not an append-only local log.
  // Treat the fetched CSV as authoritative so historical backfills and row
  // corrections replace prior local bucket totals instead of being skipped.
  const lastTs = cursors?.cursorApi?.lastRecordTimestamp || null;
```

先清桶再重建，且清桶范围有护栏：

```js
// src/lib/rollout.js:7194-7210（节选）
    // Guard (2026-06 audit): only wipe buckets the fetched export can
    // actually rebuild. ... Wiping from the earliest record onward is
    // identical for full exports and fail-safe for partial ones.
    let earliestBucketStart = null;
    for (const record of records) { ... }
    // No parseable record date at all means the export is malformed —
    // refilling would add nothing, so wiping would zero out (and upload
    // zeros over) the entire history. Skip the wipe entirely.
    if (earliestBucketStart) { ... bucket.totals = initTotals(); ... }
```

游标 `cursors.cursorApi.lastRecordTimestamp` 只作展示/诊断，**不用于过滤**（`rollout.js:7244-7247`、`7263-7268`）。

**9) 特殊规则**：

- `conversation_count` 每条 CSV 记录 `+1`（`rollout.js:7232`）→ 它是**请求级计数**而非会话级。
- `total_cost_usd` 从不写入；成本由下游价格表算（`cursor-config.js:525-536` 注释明确「Cost is computed independently from per-column tokens」）。
- 账号级来源，`source-metadata.js:1` 把它排除在「仅本地」视图之外。
- 迁移修复：早期版本因 Cursor 新增 `Cloud Agent ID`/`Automation ID` 列而把记录写成 `model="unknown"`，有一次性 purge + 游标重置（`sync.js:2144-2152`、`migrateCursorUnknownBuckets`）。

---

## 6. `parseDevinIncremental` — Devin CLI

**1) source 标识**：常量 `"devin"`。

```js
// src/lib/rollout.js:14104
const DEVIN_SOURCE = "devin";
```

**2) 数据来源类型**：SQLite。路径解析：

```js
// src/lib/rollout.js:14106-14126（节选）
function resolveDevinDbPath(env = process.env, deps = {}) {
  const override = typeof env.TOKENTRACKER_DEVIN_DB === "string" && env.TOKENTRACKER_DEVIN_DB.trim();
  if (override) return path.resolve(env.TOKENTRACKER_DEVIN_DB.trim());
  const home = env.HOME || env.USERPROFILE || os.homedir();
  const xdgDataHome = ...;
  if (process.platform !== "win32") { return path.join(xdgDataHome, "devin", "cli", "sessions.db"); }
  // Devin CLI is not known to write a native-Windows data dir; on Windows its
  // sessions.db lives under the distro's XDG home, reachable over \\wsl$.
  const wslDir = wsl.shouldProbeWsl(env) ? wsl.discoverWslHome(".local/share/devin/cli", { ...deps, env }) : null;
```

即默认 `~/.local/share/devin/cli/sessions.db`；**Windows 上没有原生路径，只能通过 WSL 探测**。

**3) SQLite 细节**：先探测表，再按需拼 JOIN。

```js
// src/lib/devin-usage.js:25-26
const DEVIN_TABLE_PROBE_SQL =
  "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('message_nodes','sessions')";
```

```js
// src/lib/devin-usage.js:38-58（节选）
  return `
    SELECT
      n.row_id AS row_id, n.session_id AS session_id, ${sessionCreatedAt} AS session_created_at,
      ${workingDirectory} AS working_directory,
      ${metricField("$.metadata.request_id", "request_id")},
      ${metricField("$.metadata.generation_model", "generation_model")},
      ${metricField("$.metadata.started_generation_at", "started_generation_at")},
      ${metricField("$.metadata.created_at", "message_created_at")},
      ${metricField("$.metadata.metrics.input_tokens", "input_tokens")}, ...
    FROM message_nodes n
    ${join}
    WHERE json_valid(n.chat_message)
      AND json_extract(n.chat_message, '$.role') = 'assistant'
      AND json_extract(n.chat_message, '$.metadata.request_id') IS NOT NULL
    ORDER BY n.row_id
  `.trim();
```

- 读取参数：`label:"Devin"`、`maxBuffer: 32MB`、`timeout: 30s`、`readOnly: true`、`throwOnReadFailure: true`（`rollout.js:14172-14181`），UNC 先快照（`:14184-14189`）。
- 压缩列：无（`chat_message` 是 JSON 文本，用 `json_extract`/`json_valid` 就地投影）。
- 隐私设计：只投影 token 与归属字段，注释声明正文、`sessions.cogs_json`、任何凭据状态从不选取（`devin-usage.js:11-13`）。

**4) 远程 API**：无。

**5) 字段映射**：

```js
// src/lib/devin-usage.js:190-200（节选）
      totals: {
        input_tokens: canonical.input,
        cached_input_tokens: canonical.cacheRead,
        cache_creation_input_tokens: canonical.cacheCreation,
        output_tokens: canonical.output,
        reasoning_output_tokens: 0,
        total_tokens: total,
        billable_total_tokens: total,
        total_cost_usd: 0,
        conversation_count: 0,
      },
```

- `total = input + output + cacheRead + cacheCreation`（`devin-usage.js:183`）。
- **`input_tokens` 不做 cache 扣减**，源码给出依据：

```js
// src/lib/devin-usage.js:21-23
// Token components are disjoint: the DB's `metrics.input_tokens` already
// excludes `cache_read_tokens` and `cache_creation_tokens`, so each reported
// counter is added once and caches are never subtracted from input.
```

- 缺失语义严格：`input`/`output` 缺失（NULL）表示**记录尚未完成**，整行不计数（`devin-usage.js:76-82` 注释 + `113-119`）；`cache_read/cache_creation` 缺失当 0（`optionalCount`，`:84-87`）。
- `reasoning_output_tokens` 恒 0；`total_cost_usd` 显式 0。

**6) 模型名**：`$.metadata.generation_model`，缺失落 `"unknown"`（`devin-usage.js:134`）。

**7) 时间戳**：优先「原始生成时间」，**绝不用节点插入时间兜底**：

```js
// src/lib/devin-usage.js:121-126（节选）
  // The original generation time decides the bucket; the node insertion time
  // (n.created_at) is clone/replay insertion time and is never a fallback.
  const tsMs = parseDevinTimestampMs(row?.started_generation_at)
    || parseDevinTimestampMs(row?.message_created_at);
  if (!tsMs) return null;
```

`sessions.created_at` 是 Unix 秒，单独用 `epochSecondsToMs` 转换（`devin-usage.js:98-102`）。

**8) 增量/去重**：两级。

- 库级：`sqliteSidecarFingerprint`（`db`/`-wal`/`-shm`/`-journal` 的 size+mtime+dev+ino，`rollout.js:19297-19314`）比对，未变即整段跳过（`rollout.js:14231-14234`）；`-shm` 被刻意剔除，避免只读打开 bump 触发重扫（`rollout.js:14130-14133`）。
- 行级：**`request_id` 是全安装的规范身份**，replay/fork/compaction 会产生多行同 request 记录：

```js
// src/lib/devin-usage.js:149-156
function compareCandidates(a, b) {
  if (a.tsMs !== b.tsMs) return a.tsMs - b.tsMs;
  const sessionA = a.sessionCreatedAtMs ?? Number.POSITIVE_INFINITY;
  const sessionB = b.sessionCreatedAtMs ?? Number.POSITIVE_INFINITY;
  if (sessionA !== sessionB) return sessionA - sessionB;
  if (a.sessionId !== b.sessionId) return a.sessionId < b.sessionId ? -1 : 1;
  return b.rowId - a.rowId;
}
```

- 持久账本 `cursors.devin.requests[requestId]`（首次观察的归属为权威，`rollout.js:14307-14347`）。
- 防 `__proto__` 污染：账本用 `Object.create(null)`（`rollout.js:14136-14147`）。
- 事务性：先 `stageDevinBuckets` 私有拷贝，两个队列 append 都成功后才发布游标（`rollout.js:14149-14169`、`14239-14246`）。

**9) 特殊规则**：

- `conversation_count` 由「哪些 session 已付过」集合派生，且**该集合从账本自身重建**，不额外持久化（`rollout.js:14283-14296`）：

```js
// src/lib/rollout.js:14340-14346（节选）
      // A fork made only of copied requests pays no conversation of its own;
      // the first genuinely new request in a conversation pays it once.
      const convKey = event.sessionId || `request:${event.requestId}`;
      if (countedConversations.has(convKey)) { event.totals.conversation_count = 0; }
      else { event.totals.conversation_count = 1; countedConversations.add(convKey); }
```

- `reasoning_output_tokens` 恒 0；`cache_creation_input_tokens` 由上游独立列提供（非从 input 反推）。

---

## 7. `parseOmpIncremental` / `parseOmoIncremental` / `parseOmpLikeIncremental` — oh-my-pi 与 OmO

三个函数共用一份实现，只是注入参数不同（`rollout.js:16087-16112`）。

**1) source 标识**：`"omp"` 与 `"omo"`。

```js
// src/lib/rollout.js:16087-16098（节选）
async function parseOmpIncremental(options = {}) {
  return parseOmpLikeIncremental({
    ...options,
    stateKey: "omp",
    source: "omp",
    resolveSessionFiles: resolveOmpSessionFiles,
    ...
    reasoningFields: ["reasoningTokens"],
  });
}
```

```js
// src/lib/rollout.js:16100-16111（节选）
async function parseOmoIncremental(options = {}) {
  return parseOmpLikeIncremental({
    ...options,
    stateKey: "omo",
    source: "omo",
    ...
    reasoningFields: ["reasoningTokens", "reasoning"],
    reasoningIncludedInOutput: true,
  });
}
```

调用点不覆盖 source（`sync.js:2708-2722`、`2747-2761`）。

**2) 数据来源类型**：自有 JSONL 会话文件。omp：`<ompHome>/agent/sessions/--<cwd-encoded>--/<ts>_<sessionId>.jsonl`（注释 `rollout.js:11405-11407`），主会话 + 子 agent 两类文件分开发现（`11352-11499`）。`stateKey` 分别为 `"omp"`、`"omo"`（游标命名空间 `cursors.omp` / `cursors.omo`）。

家目录覆盖：OMP 用 `OMP_HOME`、`PI_CONFIG_DIR`（`rollout.js:11355-11356`）；OmO 用 `TOKENTRACKER_OMO_HOME`（`rollout.js:11582-11595`）。

**3) SQLite**：不涉及。

**4) 远程 API**：无。

**5) 字段映射**（共用主体）：

```js
// src/lib/rollout.js:15868-15872（节选）
      const input = toNonNegativeInt(usage.input);
      const output = toNonNegativeInt(usage.output);
      const cacheRead = toNonNegativeInt(usage.cacheRead);
      const cacheWrite = toNonNegativeInt(usage.cacheWrite);
      const reasoningTokens = pickUsageInt(usage, reasoningFields);
```

```js
// src/lib/rollout.js:15897-15915（节选）
      // Use provided totalTokens when available; otherwise sum all components.
      // OmO folds reasoning into output, so the fallback must not add it again.
      const totalTokens =
        Number.isFinite(Number(usage.totalTokens)) && Number(usage.totalTokens) > 0
          ? toNonNegativeInt(usage.totalTokens)
          : input + output + cacheRead + cacheWrite +
            (reasoningIncludedInOutput ? 0 : reasoningTokens);
```

```js
// src/lib/rollout.js:15907-15915（节选）
      const delta = {
        input_tokens: input, cached_input_tokens: cacheRead,
        cache_creation_input_tokens: cacheWrite, output_tokens: output,
        reasoning_output_tokens: reasoningTokens, total_tokens: totalTokens,
        conversation_count: 1,
      };
```

- **`input` 不做 cache 扣减**（oh-my-pi 语义为互斥列）。
- **OmO 的 `reasoningIncludedInOutput=true`**：reasoning 已经算在 `output` 里，因此**求和时必须排除**它，否则双计（`rollout.js:15898`、`16110`）。
- `reasoningFields` 差异：omp 只认 `reasoningTokens`；omo 认 `reasoningTokens` 或 `reasoning`（`rollout.js:16096`、`16109`）。
- 全零 usage 行会被 `seenIds` 记名后跳过（`rollout.js:15874-15883`）。

**6) 模型名**：`normalizeModelInput(msg.model) || fallbackModel`（`rollout.js:15905`）。fallback 常量：omp `"omp-unknown"`（`rollout.js:11498-11501`）、omo 由 `resolveOmoDefaultModel` 给出（`rollout.js:11612-11615`）。

**7) 时间戳**：消息级毫秒 + 校验下限 2020-01-01（防「占位小数字落进 1970 桶」，注释 `rollout.js:15697-15701`）：

```js
// src/lib/rollout.js:15701-15710
const MIN_OMP_TIMESTAMP_MS = Date.UTC(2020, 0, 1);
function ompEntryTimestampMs(msg, entry) {
  const direct = Number(msg.timestamp);
  if (Number.isFinite(direct) && direct >= MIN_OMP_TIMESTAMP_MS) return direct;
  if (typeof entry.timestamp === "string" && entry.timestamp) {
    const parsed = Date.parse(entry.timestamp);
    if (Number.isFinite(parsed) && parsed >= MIN_OMP_TIMESTAMP_MS) return parsed;
  }
  return null;
}
```

无时间戳即跳过（`rollout.js:15885-15891`）。

**8) 增量/去重**：

- 文件级字节偏移 + inode 变化检测（变小或换 inode 则从 0 重读）：`rollout.js:15838-15847`。
- 支持「最终不完整记录」：只提交到最后一个完整换行处的偏移，撕裂尾巴下次重读（`rollout.js:15724-15747`）。
- entry id 去重集合 `seenIds`，**上限 10000 条**（`rollout.js:16041-16050`）。
- 项目归属使用**独立**的 `projectSeenIds` / `projectFileOffsets` 游标，以便升级后回填历史会话的项目归属而不影响总量（`rollout.js:15941-15946`）。
- 超大记录防护：`OMP_LIKE_MAX_RECORD_BYTES = 64MB`（`rollout.js:15712-15715`）。

**9) 特殊规则**：

- **`conversation_count` 是「每条 assistant 消息 +1」**，即请求级（`rollout.js:15914`）——与 Craft（会话级）语义不同，跨 provider 不可直接比较（对比注释 `rollout.js:17232-17235`）。
- 无 `billable_total_tokens`、无 `total_cost_usd`。
- 另有防撞车守卫：`piAgentDirCollidesWithOmp` / `omoAgentDirCollidesWithOmp`（`rollout.js:16157-16169`），`sync.js:2735-2741`、`2772-2774` 会在路径相同时跳过对应 provider，防止同一批会话被两个 source 各记一次。
- **hook 状态（R2 补充，I8）**：OMP 是本文 21 家中**唯一带 extension hook 主动上报**的一家——安装 `<ompAgentDir>/extensions/tokentracker-notify.ts`（`src/lib/omp-hook.js:16`、`48`），注册 `turn_end` + `agent_end` + `session_shutdown`（`omp-hook.js:135`、`139`、`143`），扩展内 `spawn(process.execPath, [notifyPath, "--source=omp"])` 且自限速 12s（`omp-hook.js:117`、`66`）；安装点 `src/commands/init.js:806`（`upsertOmpHook`）。**hook 只是让同步更早发生**：OMP 行本身始终可从会话文件被动解析。OmO 无 hook（纯被动）；`grok` 另有 SessionEnd hook（S1 §2.4 已列，非本片增量）。

---

## 8. `parsePiIncremental` / `parsePrimeAgentIncremental` / `parseMinimaxCodeIncremental` — pi 家族（共享 `parsePiLikeIncremental`）

**1) source 标识**：三家都不传死量，而是**按 provider 字段派生**。

```js
// src/lib/rollout.js:16286-16295（节选）
function piSourceForProvider(provider) {
  if (typeof provider !== "string" || !provider.trim()) return "pi";
  const slug = provider.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "").slice(0, 64);
  return slug ? `pi-${slug}` : "pi";
}
```

```js
// src/lib/rollout.js:16297-16305（节选）
function primeAgentSourceForProvider(provider) {
  if (typeof provider !== "string" || !provider.trim()) return "prime-agent";
  ...
  return slug ? `prime-agent-${slug}` : "prime-agent";
}
```

```js
// src/lib/rollout.js:16775-16785（节选）
async function parseMinimaxCodeIncremental(options = {}) {
  return parsePiLikeIncremental({
    ...options,
    // Session files have no cwd header, so project attribution is unsupported.
    projectQueuePath: undefined,
    stateKey: "minimaxCode",
    ...
    sourceForProvider: () => MINIMAX_CODE_SOURCE,
```

即 pi 的 source 是 **`pi` 或 `pi-<provider>`**，Prime 是 **`prime-agent` 或 `prime-agent-<provider>`**，MiniMax Code 恒为 `"minimax-code"`（常量 `rollout.js:16722`）。设计意图写在注释里：

```js
// src/lib/rollout.js:16281-16285（节选）
// Pi is a router: the same session can send turns to Anthropic, GitHub
// Copilot, or another backend. Keep provider names in the queue source so
// those turns cannot collapse into one bucket (or inherit the wrong pricing).
// Missing providers are deliberately kept on the historical `pi` source ...
```

**2) 数据来源类型**：自有 JSONL。

- pi：`<piHome>/agent/sessions/**/*.jsonl`（`rollout.js:16171-16223`）+ `~/.multica/pi-sessions/*.jsonl`（`rollout.js:16200-16220`）。家目录 `TOKENTRACKER_PI_AGENT_DIR` / `PI_CODING_AGENT_DIR` / `~/.pi`（`rollout.js:16141-16150`）。
- Prime Agent：`~/.prime/agent/sessions/*.jsonl`，覆盖 `TOKENTRACKER_PRIME_AGENT_HOME` / `TOKENTRACKER_PRIME_AGENT_DIR`（`rollout.js:16235-16275`）。
- MiniMax Code：`~/.minimax/v2/sessions/YYYY/MM/DD/<HH-MM-SS-mmm>-session_<id>/messages.jsonl`（注释 `rollout.js:16711`），覆盖 `TOKENTRACKER_MINIMAX_HOME`（`rollout.js:16728`）。

**3) SQLite**：不涉及。

**4) 远程 API**：无。

**5) 字段映射**（三家同表）：

```js
// src/lib/rollout.js:16449-16453
      const input = toNonNegativeInt(usage.input);
      const output = toNonNegativeInt(usage.output);
      const cacheRead = toNonNegativeInt(usage.cacheRead);
      const cacheWrite = toNonNegativeInt(usage.cacheWrite);
      const reasoningTokens = toNonNegativeInt(usage.reasoningTokens);
```

```js
// src/lib/rollout.js:16482-16485
      const totalTokens =
        Number.isFinite(Number(usage.totalTokens)) && Number(usage.totalTokens) > 0
          ? toNonNegativeInt(usage.totalTokens)
          : input + output + cacheRead + cacheWrite + reasoningTokens;
```

- `input_tokens = usage.input`（不扣缓存）；`reasoning_output_tokens = usage.reasoningTokens`（**不并入 output**，且 total 兜底时**要加它**）。
- 三家都无 `billable_total_tokens`、无 `total_cost_usd`。

**6) 模型名**：`normalizeModelInput(msg.model) || fallbackModel`（`rollout.js:16487`）。fallback：pi `"pi-unknown"`（`rollout.js:16225-16228`）、Prime `"prime-agent-unknown"`（`rollout.js:16277-16279`）、MiniMax Code 由内联函数生成（`rollout.js:16782`）。

**7) 时间戳**：`msg.timestamp`（毫秒）优先，回落 entry 的 ISO 串：

```js
// src/lib/rollout.js:16466-16471（节选）
      let tsMs = null;
      if (Number.isFinite(Number(msg.timestamp)) && Number(msg.timestamp) > 0) {
        tsMs = Number(msg.timestamp);
      } else if (typeof record.timestamp === "string" && record.timestamp) {
        const parsed = Date.parse(record.timestamp);
        if (Number.isFinite(parsed) && parsed > 0) tsMs = parsed;
      }
```

无时间戳即丢弃（`rollout.js:16473-16476`）。**（R2 修正，原表述与源码相反）** MiniMax Code 的记录包装器 `readMinimaxCodeEntry`（`rollout.js:16766-16773`）把外层 `timestamp` 恒置为 `null`，因此 `record.timestamp` 恒为 `null`，entry 级 ISO 回落分支 `rollout.js:16469` **永不触发**；唯一有效路径是 `message.timestamp` 的**数值毫秒**（`16467`，即 `msg.timestamp`），缺失即整条丢弃（`16473-16476`）。原稿「只能靠 entry 级 ISO 串」为语义反转。测试 fixture 印证结构：`test/minimax-code-parser.test.js:37-64` 的顶层只有 `message_id` / `turn_id` / `message`，`timestamp` 在 `message` 内。**下游含义**：MiniMax Code 行的可解析性完全取决于 `message.timestamp`，entry 级 ISO 串不构成任何兜底（pi / Prime 用 `readPiMessageEntry`，其 entry 带 `type === "message"`，`rollout.js:16310-16317`，同样走 `16467` 优先）。

**8) 增量/去重**：与 §7 同构——文件偏移 + inode、只提交到最后一个完整换行（`rollout.js:16405-16426`）、`seenIds` 上限 10000（`rollout.js:16680`）、项目用独立游标。另外 `hourlyState.groupQueued` 与自有桶在解析前被复制一份，确保队列写失败时不污染调用方状态（`rollout.js:16369-16386`）。

**9) 特殊规则**：

- 记录包装形态由 `readEntry` 注入：pi/Prime 用 `readPiMessageEntry`（要求 `entry.type === "message"`，`rollout.js:16310-16317`）；MiniMax Code 用 `readMinimaxCodeEntry`（用 `entry.message_id` 去重，`rollout.js:16766-16773`）。
- `conversation_count` 每条 assistant 消息 +1（与 §7 同构）。
- MiniMax Code 的 `usage.cost` 块恒 0 被显式忽略，改由下游价格表计算（注释 `rollout.js:16716-16717`）。
- MiniMax Code 的历史迁移会话会以 `model="historical-transcript"` + 全零 usage 重放，由零 token 护栏丢弃（注释 `rollout.js:16717-16718`）。

---

## 9. `parseCraftIncremental` — Craft Agents

**1) source 标识**：`"craft"`（硬编码在桶键处）：`rollout.js:17246`、`17248`。

**2) 数据来源类型**：自有 JSONL，**只读首行**的预聚合头。磁盘布局（源码注释）：

```js
// src/lib/rollout.js:16812-16816
// Disk layout:
//   ~/.craft-agent/                    ← config dir (override: CRAFT_CONFIG_DIR)
//     config.json                      ← workspaces[].rootPath list
//     workspaces/<id>/sessions/<sid>/session.jsonl  (default)
//   <user-chosen-rootPath>/sessions/<sid>/session.jsonl  (custom workspaces)
```

必须读 `config.json` 枚举自定义工作区（不能只 glob 默认目录）：`rollout.js:16818-16820`、`16862-16876`。

**3) SQLite**：不涉及。

**4) 远程 API**：无。

**5) 字段映射**：

```js
// src/lib/rollout.js:17172-17179
    const totalInput = toNonNegativeInt(usage.inputTokens);
    const totalOutput = toNonNegativeInt(usage.outputTokens);
    const totalCacheRead = toNonNegativeInt(usage.cacheReadTokens);
    const totalCacheWrite = toNonNegativeInt(usage.cacheCreationTokens);
    const totalReported =
      Number.isFinite(Number(usage.totalTokens)) && Number(usage.totalTokens) > 0
        ? toNonNegativeInt(usage.totalTokens)
        : totalInput + totalOutput + totalCacheRead + totalCacheWrite;
```

```js
// src/lib/rollout.js:17236-17244（节选）
    const delta = {
      input_tokens: dInput,
      cached_input_tokens: dCacheRead,
      cache_creation_input_tokens: dCacheWrite,
      output_tokens: dOutput,
      reasoning_output_tokens: 0,
      total_tokens: dTotal > 0 ? dTotal : dInput + dOutput + dCacheRead + dCacheWrite,
      conversation_count: prev.total === 0 ? 1 : 0,
    };
```

- 上游 `inputTokens` 是**纯非缓存输入**（注释 `rollout.js:16822-16824` 明确「no Codex-style trap」）。
- `reasoning_output_tokens` 恒 0；无 `billable_total_tokens`。

**6) 模型名**：首行 `header.model`，缺失落 `"craft-unknown"`（`rollout.js:17230`、`16902-16905`）。注释说明 Craft 是路由器、模型按会话头给出（`16820-16821`）。

**7) 时间戳**：三级候选 + **文件 mtime 兜底**（与 §2/§6 的「无时间戳即丢弃」不同）：

```js
// src/lib/rollout.js:17214-17223（节选）
    // Bucket on lastMessageAt (preferred) or createdAt — both ms epoch.
    let tsMs = null;
    const tsCandidates = [header.lastMessageAt, header.lastUsedAt, header.createdAt];
    for (const cand of tsCandidates) {
      if (Number.isFinite(Number(cand)) && Number(cand) > 0) { tsMs = Number(cand); break; }
    }
    if (tsMs == null) tsMs = stat.mtimeMs;
```

**8) 增量/去重**：**按会话头快照差分**——头会随会话增长被原地重写，因此记上一份快照、只提交增量：

```js
// src/lib/rollout.js:17189-17195（节选）
    // Compute the delta since the last sync. Negative deltas mean the session
    // was reset/truncated — clamp to 0 and replace the snapshot.
    const dInput = Math.max(0, totalInput - prev.input);
    const dOutput = Math.max(0, totalOutput - prev.output);
    const dCacheRead = Math.max(0, totalCacheRead - prev.cacheRead);
    const dCacheWrite = Math.max(0, totalCacheWrite - prev.cacheWrite);
    const dTotal = Math.max(0, totalReported - prev.total);
```

游标 `cursors.craft.sessionTotals[sessionId] = {input, output, cacheRead, cacheWrite, total, lastSeenAt}`（`rollout.js:17251-17258`），**上限 5000 条、按 `lastSeenAt` 最近优先淘汰**（避免淘汰长活会话导致下轮把整份总量当增量重记）：

```js
// src/lib/rollout.js:17271-17275（节选）
  // Cap session-totals map at 5k entries to bound cursor state size. Evict by
  // lastSeenAt (least-recently-seen first) so that long-lived sessions stay
  // tracked even when many newer one-shot sessions cycle through. Insertion
  // order would silently re-zero a long-running session and double-count its
  // total on the next sync.
```

**9) 特殊规则**：

- **`conversation_count` 是会话级**（首次见该 session 记 1，之后恒 0），源码明确警告跨 provider 不可比：

```js
// src/lib/rollout.js:17232-17235
    // conversation_count: 1 the first time we see a session, 0 on subsequent
    // syncs of the same session. NOTE: this differs from omp/Claude which
    // count one-per-assistant-message. Cross-provider "conversations" totals
    // are therefore not directly comparable — Craft's are per-session.
```

- 读取上限 1 MiB（`rollout.js:17125-17135`），首行不是合法 JSON 即跳过（`17151-17158`）。
- 上游可以为 `SessionHeader.id` 之外的 `sdkSessionId` 兜底（`rollout.js:17162-17167`）。

---

## 10. `parseReasonixIncremental` — Reasonix

**1) source 标识**：`"reasonix"`（硬编码）：`rollout.js:17036`、`17046`。

**2) 数据来源类型**：自有 JSON「遥测旁车」，**刻意只读无内容文件**：

```js
// src/lib/rollout.js:16907-16908
// Reasonix persists content-free cumulative usage beside each session JSONL.
// Reading only these telemetry sidecars keeps prompts and tool output private.
```

文件为 `*.jsonl.telemetry.json`（`rollout.js:16959`），配套同名 `.meta`（`rollout.js:16982-16985`）。家目录候选（Windows 特例）：

```js
// src/lib/rollout.js:16925-16934（节选）
  const candidates = [path.join(home, ".reasonix")];
  ...
  if (process.platform === "win32" && env.APPDATA) {
    candidates.push(path.join(env.APPDATA, "reasonix"));
  }
```

扫描深度上限 `REASONIX_SCAN_MAX_DEPTH = 8`（`rollout.js:16949`）。

**3) SQLite**：不涉及。

**4) 远程 API**：无。

**5) 字段映射**（含缓存减法，是全项目为数不多「prompt 含缓存」的处理点）：

```js
// src/lib/rollout.js:16989-17005（节选）
function normalizeReasonixTotals(usage) {
  const prompt = toNonNegativeInt(usage.promptTokens);
  const reasoning = toNonNegativeInt(usage.reasoningTokens);
  const completion = toNonNegativeInt(usage.completionTokens);
  const cacheMiss = Math.min(prompt, toNonNegativeInt(usage.cacheMissTokens));
  const cacheHit = Math.min(prompt, toNonNegativeInt(usage.cacheHitTokens));
  const hasCacheMiss = usage.cacheMissTokens != null;
  const uncachedPrompt = hasCacheMiss ? cacheMiss : Math.max(0, prompt - cacheHit);
  const cacheWrite = Math.min(uncachedPrompt, toNonNegativeInt(usage.cacheWriteTokens));
  return {
    input: uncachedPrompt - cacheWrite,
    cacheRead: hasCacheMiss ? prompt - cacheMiss : cacheHit,
    cacheWrite,
    output: Math.max(0, completion - reasoning),
    reasoning,
    requests: toNonNegativeInt(usage.requestCount),
  };
}
```

```js
// src/lib/rollout.js:17037-17045（节选）
  addTotals(bucket.totals, {
    input_tokens: delta.input,
    cached_input_tokens: delta.cacheRead,
    cache_creation_input_tokens: delta.cacheWrite,
    output_tokens: delta.output,
    reasoning_output_tokens: delta.reasoning,
    total_tokens: total,
    conversation_count: delta.requests,
  });
```

- `total = input + cacheRead + cacheWrite + output + reasoning`（`rollout.js:17035`）。
- **`reasoning` 从 `completion` 里减掉**（`output: completion - reasoning`），即上游 completion 含推理。
- **`conversation_count = requestCount`**：用「请求数」冒充「会话数」，见 §26 风险点。
- 无 `billable_total_tokens`。

**6) 模型名**：`meta.model` 的**最后一段**（按 `/` 切）：

```js
// src/lib/rollout.js:17024-17029
function normalizeReasonixModel(value) {
  const normalized = normalizeModelInput(value);
  if (!normalized) return "reasonix-unknown";
  const segments = normalized.split("/").filter(Boolean);
  return segments.at(-1) || "reasonix-unknown";
}
```

**7) 时间戳**：`meta.updated_at` → `meta.created_at` → **文件 mtimeMs**（`rollout.js:17016-17022`）。

**8) 增量/去重**：**累计快照差值**，游标 `cursors.reasonix.sessionTotals[filePath]`：

```js
// src/lib/rollout.js:17008-17014
function diffReasonixTotals(current, previous = {}) {
  const delta = {};
  for (const key of ["input", "cacheRead", "cacheWrite", "output", "reasoning", "requests"]) {
    delta[key] = Math.max(0, current[key] - toNonNegativeInt(previous[key]));
  }
  return delta;
}
```

差值全 0 且 `requests` 未增则整文件跳过（`rollout.js:17067-17068`）。快照文件被删/回退时，差值 clamp 到 0，不做负向回撤（与 Qoder 的「先减旧再加新」不同）。

**9) 特殊规则**：无 `billable_total_tokens`、无 `cost`；`usage_precision` 也不写。文件级身份只按路径（`filePath` 即游标键），**没有跨安装去重**（多份同名文件在不同 home 下会各算一次）。

---

## 11. `parseCopilotIncremental` — GitHub Copilot CLI / Chat 扩展（OTEL JSONL）

**1) source 标识**：`"copilot"`（硬编码）：`rollout.js:18051`、`18056`。

**2) 数据来源类型**：自有 JSONL（OTEL 导出）。**必须用户显式开启导出**：

```js
// src/lib/rollout.js:17301-17306（节选）
// GitHub Copilot CLI — OpenTelemetry JSONL exporter
// User must opt in by setting:
//   COPILOT_OTEL_ENABLED=true
//   COPILOT_OTEL_EXPORTER_TYPE=file
//   COPILOT_OTEL_FILE_EXPORTER_PATH=$HOME/.copilot/otel/copilot-otel-...jsonl
```

发现目录：`~/.copilot/otel/*.jsonl`、`~/.copilot-otel/*.jsonl`，以及显式 `COPILOT_OTEL_FILE_EXPORTER_PATH`（`rollout.js:17309-17341`）；Windows 另探 WSL 家目录（`17328-17335`）。CLI 会提示用户怎么开（`src/commands/status.js:1519-1526`）。

**3) SQLite**：本函数不涉及（另两份 Copilot 源才是 SQLite，见 §12/§13）。

**4) 远程 API**：无。

**5) 字段映射**（字段名多形态 + 含/不含缓存的两种语义）：

```js
// src/lib/rollout.js:17501-17521（节选）
  const inputRaw = toNonNegativeInt(attrs["gen_ai.usage.input_tokens"]);
  const output = toNonNegativeInt(attrs["gen_ai.usage.output_tokens"]);
  const cacheRead = toNonNegativeInt(
    attrs["gen_ai.usage.cache_read.input_tokens"] ?? attrs["gen_ai.usage.cache_read_input_tokens"]
      ?? attrs["gen_ai.usage.cached_input_tokens"]);
  const cacheWrite = toNonNegativeInt(
    attrs["gen_ai.usage.cache_write.input_tokens"] ?? attrs["gen_ai.usage.cache_creation.input_tokens"]
      ?? attrs["gen_ai.usage.cache_write_input_tokens"] ?? attrs["gen_ai.usage.cache_creation_input_tokens"]);
  const reasoning = toNonNegativeInt(
    attrs["gen_ai.usage.reasoning.output_tokens"] ?? attrs["gen_ai.usage.reasoning_tokens"]
      ?? attrs["gen_ai.usage.reasoning_output_tokens"]);
  const reasoningClamped = Math.min(reasoning, output);
```

```js
// src/lib/rollout.js:17523-17535（节选）
  const cliSpan = isCopilotV1ChatSpan(record);
  // CLI input includes both cache reads and writes. Chat-extension LogRecords
  // expose cache creation separately, so preserve their existing input-minus-read semantics.
  const cacheReadClamped = Math.min(cacheRead, inputRaw);
  const cacheWriteClamped = Math.min(cacheWrite, Math.max(0, inputRaw - cacheReadClamped));
  const cacheWriteForAccounting = cliSpan ? cacheWriteClamped : cacheWrite;
  const input = Math.max(0, inputRaw - cacheReadClamped - (cliSpan ? cacheWriteClamped : 0));
```

- **推理被拆出 output**：`output_tokens = output - reasoningClamped`，`reasoning_output_tokens = reasoningClamped`（`rollout.js:17521-17522`、`17578-17579`）。
- `total_tokens` 由五分量相加（`rollout.js:17580-17585`）。
- **CLI span 与 Chat 扩展语义不同**：CLI 的 `input_tokens` 同时含 cache read + write（双扣）；扩展的 LogRecord 只含 read（单扣）。这是本项目里唯一按「同一 provider 的两代导出形态」分别处理字段语义的地方。
- 无 `billable_total_tokens`。

**6) 模型名**：`gen_ai.response.model` 优先、`gen_ai.request.model` 次之（`rollout.js:17367-17373`），再经 `normalizeCopilotAppModel`（把 `claude-sonnet-4.5` → `claude-sonnet-4-5`，`rollout.js:19244-19252`），兜底 `"github-copilot"`（常量 `COPILOT_APP_DEFAULT_MODEL`，`rollout.js:19033`）。

**7) 时间戳**：OTEL 的 `[秒, 纳秒]` 数组（四种字段任取其一）：

```js
// src/lib/rollout.js:17358-17365
function copilotOtelTimeToMs(value) {
  if (!Array.isArray(value) || value.length < 2) return null;
  const seconds = Number(value[0]);
  const nanos = Number(value[1]);
  if (!Number.isFinite(seconds)) return null;
  const ns = Number.isFinite(nanos) ? nanos : 0;
  return Math.round(seconds * 1000 + ns / 1_000_000);
}
```

字段顺序 `endTime` → `startTime` → `hrTime` → `hrTimeObserved`（`rollout.js:17545-17549`）；都取不到即丢弃。

**8) 增量/去重**：三重。

- 文件级偏移 + inode 变化（`rollout.js:17983-17992`），读后再 `stat` 一次以吸收解析期间的追加（注释 `18078-18086`）。
- 记录级 `dedupKey`：CLI span 用 `traceId:spanId`，扩展 LogRecord 用 `resp:<gen_ai.response.id>`（`rollout.js:17447-17460`）。
- 解析器版本迁移：`COPILOT_PARSER_VERSION = 3`（`rollout.js:17375`），v1→v2 用行哈希跳过已计入前缀（`17917-17927`），v2→v3 因为 v2 误用 `spanContext` 作去重键而需要先修复已计前缀，修不了就**保持 v2 游标下次重试**（`17930-17951`）。
- `seenIds` 上限 10000（`rollout.js:18089-18091`）。

**9) 特殊规则**：

- 「用量认领」机制：OTEL CLI span 结算后写 `recentUsageEvents`，供 session-store 通道避免重复计数；`usageClaims` 保留 7 天、上限 10000 条（`rollout.js:17375-17405`、`18058-18064`）。
- `conversation_count` 每条聊天 span +1（`rollout.js:17586`）。
- 指标记录（`record.scopeMetrics`）与 chat span 的识别条件：`gen_ai.operation.name === "chat"` 或 `name` 以 `chat ` 开头的 span（`rollout.js:17343-17356`）。
- `usageClaimsComplete` 标记「本设备是否完整掌握了 CLI 侧的用量认领」，缺失时会影响跨通道去重的保守程度（`rollout.js:18037`、`18904-18925`）。

## 12. `parseCopilotSessionStoreIncremental` — Copilot 本地运行时 session-store.db

**1) source 标识**：**同为 `"copilot"`**（硬编码）：`rollout.js:18888`、`18890`。三份 Copilot 源共用同一 source，靠去重机制避免重复计。

**2) 数据来源类型**：SQLite。这是「CLI 1.0.70+ 每次 LLM 请求一行」的权威本地源：

```js
// src/lib/rollout.js:18122-18125（节选）
// Copilot CLI 1.0.70+ persists one row per LLM request here. The Copilot App
// uses the same runtime, so this is the only local source that remains complete
// when one session moves App -> CLI -> App (or the reverse). data.db only keeps
// the App-owned portion of those sessions.
```

路径解析：

```js
// src/lib/rollout.js:18131-18155（节选）
function resolveCopilotDbPaths({ fileName, overrideEnvKey }, env = process.env) {
  const home = process.platform === "win32"
    ? env.USERPROFILE || env.HOME || require("node:os").homedir()
    : env.HOME || require("node:os").homedir();
  ...
  if (typeof env[overrideEnvKey] === "string" && env[overrideEnvKey].trim()) { addDbPath(env[overrideEnvKey]); }
  if (typeof env.COPILOT_HOME === "string" && env.COPILOT_HOME.trim()) { addHome(env.COPILOT_HOME); }
  addDbPath(path.join(home, ".copilot", fileName));
```

覆盖变量 `TOKENTRACKER_COPILOT_SESSION_STORE_DB`（`rollout.js:18162`）；文件名 `session-store.db`。

**3) SQLite 细节**：

探测与元数据（`assistant_usage_events` 是否存在、`schema_version`、最大 id 与游标行指纹）：

```js
// src/lib/rollout.js:18264-18278（节选）
  const tables = readSqliteJsonRows(dbPath,
    "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('assistant_usage_events', 'schema_version')",
    { label: "GitHub Copilot session store", maxBuffer: 4 * 1024 * 1024, timeout: 10_000,
      readOnly: true, throwOnReadFailure: true, ...sqliteOptions });
  const names = new Set(tables.map((row) => row?.name).filter(Boolean));
  if (!names.has("assistant_usage_events")) { return { active: false, schemaVersion: null, maxId: 0 }; }
```

主查询（节选）：

```js
// src/lib/rollout.js:18414-18428（节选）
    `
      SELECT
        id, session_id, model, input_tokens, output_tokens,
        cache_read_tokens, cache_write_tokens, reasoning_tokens,
        token_details_json, created_at
      FROM assistant_usage_events
      WHERE ${whereClause}
      ORDER BY id ASC
    `.trim(),
```

- 增量条件 `id > ${lastId}`：`rollout.js:18441-18447`；重置/对账时传 0 全量重读（`18698-18728`）。
- 读参数：`maxBuffer: 64MB`、`timeout: 60_000`、`readOnly: true`、`throwOnReadFailure: true`（`rollout.js:18430-18437`）。
- **读前先做整库快照** `snapshotSqliteDb(resolvedDb)`（`rollout.js:18681`），`finally` 里 `snap.cleanup()`（`18937-18939`）——避免读到半写状态。
- 无压缩列；`token_details_json` 是 JSON 文本（`parseCopilotStoreTokenDetails`）。

**4) 远程 API**：无。

**5) 字段映射**（两档精度）：

```js
// src/lib/rollout.js:18534-18563（节选）
function normalizeCopilotSessionStoreUsage(row) {
  const inputRaw = toNonNegativeInt(row?.input_tokens);
  const outputRaw = toNonNegativeInt(row?.output_tokens);
  const reasoning = toNonNegativeInt(row?.reasoning_tokens);
  const details = parseCopilotStoreTokenDetails(row?.token_details_json);
  let precision = "fallback";
  if (details && details.input + details.cacheRead + details.cacheWrite === inputRaw
      && details.output === outputRaw) {
    input = details.input; cacheRead = details.cacheRead; cacheWrite = details.cacheWrite;
    output = details.output; precision = "exact";
  } else {
    cacheRead = Math.min(toNonNegativeInt(row?.cache_read_tokens), inputRaw);
    cacheWrite = Math.min(toNonNegativeInt(row?.cache_write_tokens), Math.max(0, inputRaw - cacheRead));
    input = Math.max(0, inputRaw - cacheRead - cacheWrite);
    output = outputRaw;
  }
```

```js
// src/lib/rollout.js:18565-18576（节选）
  const reasoningClamped = Math.min(reasoning, output);
  const outputWithoutReasoning = Math.max(0, output - reasoningClamped);
  return { input_tokens: input, cached_input_tokens: cacheRead,
    cache_creation_input_tokens: cacheWrite, output_tokens: outputWithoutReasoning,
    reasoning_output_tokens: reasoningClamped,
    total_tokens: input + cacheRead + cacheWrite + outputWithoutReasoning + reasoningClamped, precision };
```

- **`input_tokens`（列）是含缓存的**，因此这里必须扣 cache read + write（与 §11 的 CLI span 语义一致）。
- `precision` 只用于诊断/披露（`"exact"` / `"fallback"`），不进 totals。
- 消费侧再把 `precision` 之外的六列映射进 delta（`rollout.js:18879-18887`），无 `billable_total_tokens`。

**6) 模型名**：`row.model` → `normalizeCopilotAppModel(...)` → 兜底 `"github-copilot"`（`rollout.js:18877-18878`）。

**7) 时间戳**：`row.created_at` 经 `parseCopilotAppTimestamp`（同时兼容秒/毫秒数字与可解析字符串）：

```js
// src/lib/rollout.js:19227-19241（节选）
function parseCopilotAppTimestamp(value) {
  if (value === null || value === undefined || value === "") return null;
  if (typeof value === "number") {
    const ms = value > 10_000_000_000 ? value : value * 1000;
    const d = new Date(ms);
    return Number.isNaN(d.getTime()) ? null : d.toISOString();
  }
  ...
  if (/^\d+$/.test(trimmed)) { return parseCopilotAppTimestamp(Number(trimmed)); }
  const parsed = new Date(trimmed);
```

时间戳缺失/不可解析时，该行被登记进 `pendingMalformedEvents` 留待下次重试，而不是丢弃（`rollout.js:18800-18802`、`18870-18876`）。

**8) 增量/去重**：这是 21 家里机制最复杂的一家。

- 逐库游标 `cursors.copilotStore.dbs[dbPath]`，含 `lastId`、`lastRowKey`（对游标行做 sha256 指纹）、`seenEventCounts`（事件指纹多重集）、`pendingMalformedEvents`、`dbIno`、`adoptedAt`。
- **重置检测**：id 回退或游标行指纹变化即判为「库被重建/重索引」，走全量对账（`rollout.js:18698-18720`）。
- 行身份指纹：

```js
// src/lib/rollout.js:18188-18211（节选）
function copilotStoreEventFingerprint(row) {
  const sessionId = typeof row?.session_id === "string" ? row.session_id.trim() : "";
  if (!sessionId) return null;
  const normalized = normalizeCopilotSessionStoreUsage(row);
  const timestamp = parseCopilotAppTimestamp(row?.created_at) || `invalid:${String(row?.created_at ?? "")}`;
  return crypto.createHash("sha256").update(JSON.stringify([
    sessionId, normalizeCopilotAppModel(row?.model) || COPILOT_APP_DEFAULT_MODEL,
    normalized.input_tokens, normalized.output_tokens, normalized.cached_input_tokens,
    normalized.cache_creation_input_tokens, normalized.reasoning_output_tokens, timestamp,
  ])).digest("hex");
}
```

- 全量对账时按「历史指纹多重集」逐一核销，保证重索引不重复计入：

```js
// src/lib/rollout.js:18755-18806（节选）
      // Reindex/reset can rewrite every SQLite id. Subtract the exact metadata
      // multiset already observed so only genuinely new requests are emitted.
      const historicalFingerprintCounts = reconcileAllRows ? new Map(...) : null;
      ...
        if (historicalCount > 0) { historicalFingerprintCounts.set(eventFingerprint, historicalCount - 1); ... continue; }
```

- 「首次采纳」（adoption）语义：新发现的库首见只建基线不计数，除非 `backfillOnFirstRun`（`rollout.js:18717-18732`）。
- 跨通道去重：与 §11 的 OTEL 认领、以及与 §13 的 App DB 会话级基线互认（`otelUsageMatcher.consume(...)`，`rollout.js:18822-18824`）。
- 同一物理库的多路径别名会先合并状态，避免双计：`coalesceCopilotDbStatesByIdentity`（`rollout.js:18610-18617`）。

**9) 特殊规则**：

- `conversation_count` 按**会话**：见**本节（§12）** delta 生成处 `seenSessions.has(sessionId) ? 0 : 1`（`rollout.js:18886`，位于 `parseCopilotSessionStoreIncremental` 内；R2 修正原「见 §12」的自指）。
- 与 App DB 的残留对账：首见采纳时，若某会话在 App DB 里已有基线，则只补差量（`legacyResiduals`，`rollout.js:18834-18856`、`18942-18949`）。
- 无 `billable_total_tokens`。

---

## 13. `parseCopilotAppDbIncremental` — Copilot App data.db（会话级汇总）

**1) source 标识**：**同为 `"copilot"`**：`rollout.js:19455`（delta 生成处）。

**2) 数据来源类型**：SQLite。路径 `~/.copilot/data.db`，覆盖变量 `TOKENTRACKER_COPILOT_APP_DB`（`rollout.js:19190-19198`）。

**3) SQLite 细节**：先 `PRAGMA table_info(sessions)` 判定可用列，再按需生成 `NULL AS <col>` 占位（兼容老版本 schema）：

```js
// src/lib/rollout.js:19255-19286（节选）
  const pragmaRows = readSqliteJsonRows(dbPath, "PRAGMA table_info(sessions)", { ... });
  const columns = new Set(pragmaRows.map((row) => row?.name).filter(Boolean));
  if (!columns.has("id")) return [];
  const optional = (col) => (columns.has(col) ? col : `NULL AS ${col}`);
  ...
  const sql = `
    SELECT
      id, ${optional("session_type")}, ${optional("model")}, ${optional("provider_id")},
      ${optional("created_at")}, ${optional("updated_at")},
      ${optional("total_input_tokens")}, ${optional("total_output_tokens")},
      ${optional("total_cached_tokens")}, ${optional("total_reasoning_tokens")}
    FROM sessions
    ${orderBy}
  `.trim();
```

注释同时声明 `title` 等含内容列**从不进查询**（`rollout.js:19270-19271`）。读参数 `maxBuffer: 64MB` / `timeout: 60_000` / `readOnly` / `throwOnReadFailure`（`rollout.js:19287-19294`）。

**4) 远程 API**：无。

**5) 字段映射**（会话累计差值，且**强制无 cache write**）：

```js
// src/lib/rollout.js:19357-19379（节选）
function normalizeCopilotAppTokenDelta(curr, prev) {
  const inputDelta = Math.max(0, Number(curr?.input || 0) - Number(prev?.input || 0));
  const cachedDelta = Math.max(0, Number(curr?.cached || 0) - Number(prev?.cached || 0));
  const cachedInputTokens = Math.min(cachedDelta, inputDelta);
  const inputTokens = Math.max(0, inputDelta - cachedInputTokens);
  const outputDelta = Math.max(0, Number(curr?.output || 0) - Number(prev?.output || 0));
  const reasoningDelta = Math.max(0, Number(curr?.reasoning || 0) - Number(prev?.reasoning || 0));
  const reasoningTokens = Math.min(reasoningDelta, outputDelta);
  const outputTokens = Math.max(0, outputDelta - reasoningTokens);
  const totalTokens = inputTokens + cachedInputTokens + outputTokens + reasoningTokens;
  return { input_tokens: inputTokens, cached_input_tokens: cachedInputTokens,
    cache_creation_input_tokens: 0, output_tokens: outputTokens,
    reasoning_output_tokens: reasoningTokens, total_tokens: totalTokens,
    conversation_count: copilotAppBaselineTotal(prev) === 0 && totalTokens > 0 ? 1 : 0 };
}
```

- `total_*_tokens` 是**含缓存的整会话累计**：`total_cached_tokens` 被 clamp 到 inputDelta 内并从中扣除（`rollout.js:19358-19361`）。
- **`cache_creation_input_tokens` 恒为 0**（App DB 没有 cache write 列）。
- `reasoning` 从 output 里拆出（`19367-19368`）。
- 无 `billable_total_tokens`。

**6) 模型名**：`sessions.model` → `normalizeCopilotAppModel` → 兜底 `"github-copilot"`。

**7) 时间戳**：`updated_at` / `created_at`（`rollout.js:19266-19269` 排序用），实际分桶沿用同一套 `parseCopilotAppTimestamp`。**该通道是会话级累计差值，因此一条会话的全部历史 token 会落在「本次观测到的那个半小时桶」里**——见 §26 风险点。

**8) 增量/去重**：

- 库级指纹未变即跳过（语句在 `rollout.js:19439`；`19438` 是空白行——R2 修正 ±1）。
- 会话级 `sessionTotals` 差值（`19419-19422`）。
- 会话总数上限 10000，**非零会话优先保留**、零值会话按 `updatedAt` 倒序补位（`rollout.js:19334-19355`）。
- 与 OTEL 通道的分工：明确跳过 CLI/VS Code 扩展会话，避免同一批请求被两条通道各记一次：

```js
// src/lib/rollout.js:19213-19224（节选）
  // OTEL remains the owner for recognizable Copilot CLI / VS Code extension
  // sessions. ... skipping known OTEL-backed surfaces avoids duplicate
  // source="copilot" buckets if GitHub mirrors those sessions into data.db
  ...
  return !markers.some((value) => value === "cli" || value === "copilot-cli"
    || value.includes("vscode") || value.includes("extension") || value.includes("otel"));
```

- `observeOnly` 模式：当 session-store 通道已激活时，App DB 只观测不计数（调用点 `sync.js:3207-3221`）。

**9) 特殊规则**：`conversation_count` 由「会话首个非零观测」给出（`rollout.js:19377`），即**会话级**。

---

## 14. `parseGrokBuildIncremental` — Grok Build (xAI)

**1) source 标识**：`"grok"`（硬编码）：`rollout.js:20284`、`20290`。

**2) 数据来源类型**：自有 JSONL + JSON 旁车。目录结构：

```js
// src/lib/rollout.js:19566-19605（节选）
  const sessionsRoot = path.join(home, "sessions");
  ...
    for (const sid of sessionIds) {
      const sessionDir = path.join(cwdPath, sid);
      const signalsPath = path.join(sessionDir, "signals.json");
      const updatesPath = path.join(sessionDir, "updates.jsonl");
      if (fssync.existsSync(signalsPath) || fssync.existsSync(updatesPath)) {
        results.push({ sessionDir, updatesPath, signalsPath,
          summaryPath: path.join(sessionDir, "summary.json"), sessionId: sid, encodedCwd: cwdDir });
```

家目录 `TOKENTRACKER_GROK_HOME` → `GROK_HOME` → `~/.grok`（`rollout.js:19553-19564`）。

**3) SQLite**：不涉及。

**4) 远程 API**：无（`grok-limits.js` 里的配额查询属 usage-limits 路径，不在本文范围）。

**5) 字段映射**（首选路径）：`normalizeGrokUsage`（`src/lib/grok-usage.js`）。

```js
// src/lib/grok-usage.js:44-61（节选）
  const camelCaseInput = usage.inputTokens != null;
  const reportedInput = nonNegative(usage.inputTokens ?? usage.input_tokens);
  const cachedInput = nonNegative(usage.cachedReadTokens ?? usage.cacheReadInputTokens
    ?? usage.cache_read_input_tokens ?? usage.cached_input_tokens);
  const cacheCreationInput = nonNegative(usage.cacheCreationTokens ?? usage.cachedWriteTokens
    ?? usage.cacheWriteInputTokens ?? usage.cache_creation_input_tokens);
  const rawOutput = nonNegative(usage.outputTokens ?? usage.output_tokens);
  const reasoningOutput = nonNegative(usage.reasoningTokens ?? usage.reasoning_output_tokens);
  const inputTokens = camelCaseInput ? Math.max(0, reportedInput - cachedInput - cacheCreationInput) : reportedInput;
  const outputTokens = Math.max(0, rawOutput - reasoningOutput);
```

```js
// src/lib/grok-usage.js:63-90（节选）
  let totalTokens = nonNegative(usage.totalTokens ?? usage.total_tokens);
  if (totalTokens <= 0) { totalTokens = inputTokens + cachedInput + cacheCreationInput + outputTokens + reasoningOutput; }
  if (totalTokens <= 0) return null;
  ...
  return { input_tokens: inputTokens, cached_input_tokens: cachedInput,
    cache_creation_input_tokens: cacheCreationInput, output_tokens: outputTokens,
    reasoning_output_tokens: reasoningOutput, total_tokens: totalTokens,
    billable_total_tokens: totalTokens, total_cost_usd: reportedCostUsd(usage, {...}),
    cost_is_partial: costIsPartial, usage_is_incomplete: usageIsIncomplete,
    model_calls: nonNegative(usage.modelCalls ?? usage.model_calls),
    api_duration_ms: nonNegative(usage.apiDurationMs ?? usage.api_duration_ms) };
```

- **camelCase 与 snake_case 语义不同**：camelCase 时 `inputTokens` 含缓存（要双扣），snake_case 时已是纯输入（不扣）。源码注释逐字说明（`grok-usage.js:3-10`）。
- `reasoningTokens` 从 `outputTokens` 拆出（`grok-usage.js:61`）。
- **`total_cost_usd` 可由上游 ticks 直接给出**（见 §14.9）。
- `billable_total_tokens` = `total_tokens`。

**6) 模型名与多模型**：

```js
// src/lib/rollout.js:19822-19839（节选）
  const modelUsage = usage.modelUsage && typeof usage.modelUsage === "object" ? usage.modelUsage : null;
  const events = [];
  if (modelUsage && Object.keys(modelUsage).length > 0) {
    for (const [modelName, modelUsageEntry] of Object.entries(modelUsage)) {
      if (!modelUsageEntry || typeof modelUsageEntry !== "object") continue;
      const event = normalizeGrokTurnUsage(modelUsageEntry, modelName, timestamp, `${baseEventId}|${modelName}`);
      if (event) events.push(event);
    }
  }
  if (events.length === 0) { const event = normalizeGrokTurnUsage(usage, fallbackModel, timestamp, baseEventId); ... }
```

- **一家一次 turn 可以拆成多个模型事件**（`usage.modelUsage` 逐模型），这是 21 家里唯二支持单请求多模型的实现（另一家是 Cursor 的多行 CSV）。
- 模型兜底 `"grok-build"`（`rollout.js:19675-19682`）；免费 SKU 被规范化成 `"grok-build-free"`，避免与付费 `grok-4.5` 价格模糊匹配：

```js
// src/lib/rollout.js:19780-19791（节选）
function canonicalizeGrokUsageModel(model) {
  const raw = normalizeModelInput(model) || "grok-build";
  const lower = raw.toLowerCase();
  // Free Build SKU must not fuzzy-match paid grok-4.5 rates in native clients.
  if (lower.includes("build-free") || lower.endsWith("-free") || lower.includes("free-tier")) { return "grok-build-free"; }
  if (lower === "grok-4.5-build" || lower === "grok-4-5-build") { return "grok-4.5-build"; }
  return raw;
}
```

**7) 时间戳**：事件级时间优先，然后会话最后活跃时间，**最后回落到 `Date.now()`**：

```js
// src/lib/rollout.js:20214（节选）
      const hourStartStr = toUtcHalfHourStart(event.timestamp) || toUtcHalfHourStart(lastActive) || toUtcHalfHourStart(Date.now());
```

`lastActive` 亦带 `Date.now()` 兜底（`rollout.js:19684-19693`：`... || new Date().toISOString()`）。**这是 21 家里时间兜底最激进的一家**（见 §26 风险点）。

**8) 增量/去重**：

- 文件级字节偏移 + inode：`readGrokUpdateTokenEvents`（`rollout.js:19870-19883`）。
- **只推进到最后一个换行处**，防止半写行被永久跳过（`rollout.js:19886-19900`、`grokFileEndsWithNewline` `:19758-19777`）。
- 会话级快照 `cursors.grok.sessionSnapshots[sessionId]`（含 `legacySeen` 基线标记），上限 10000（`rollout.js:19645-19649`）。
- **游标版本迁移**：`GROK_CURSOR_VERSION = 5`（`rollout.js:19551`），v5 之前用「上下文窗口总量当累计消费」是错的，升级时清空 grok 桶并只保留 `legacySeen` 基线：

```js
// src/lib/rollout.js:20099-20123（节选）
  // v3 and earlier treated context-window totalTokens as cumulative spend;
  // v4 still overlapped output/reasoning, discarded cache creation, and ignored
  // provider-reported cost. Rebuild from turn_completed.usage for v5.
  ...
  if (needsTurnUsageMigration) { ... clearSourceHourlyBuckets(hourlyState, "grok"); }
```

**9) 特殊规则（含最大风险源）**：

- **水印兜底 + 0.8 硬拆**：老日志没有 `turn_completed` 时，用上下文窗口 token 的增量按下述比例拆 input/output，并标记 `usage_precision = "estimated"`：

```js
// src/lib/rollout.js:19945-19961（节选）
function estimateGrokTokenDelta(totalTokens, conversationCount, options = {}) {
  const total = Math.trunc(normalizeNonNegativeNumber(totalTokens));
  const inputTokens = Math.round(total * GROK_ESTIMATED_INPUT_RATIO);
  const outputTokens = Math.max(0, total - inputTokens);
  ...
  return { input_tokens: inputTokens, cached_input_tokens: 0, cache_creation_input_tokens: 0,
    output_tokens: outputTokens, reasoning_output_tokens: 0, total_tokens: total,
    billable_total_tokens: total, conversation_count: conversations };
}
```

常量：`GROK_ESTIMATED_INPUT_RATIO = 0.8`（`rollout.js:19549`）。**`reasoning` 与 `cached` 在该路径恒为 0。**
- 水印来源：`signals.totalTokens` / `contextTokensUsed`（**上下文窗口大小时刻值，不是可计费累计**，注释 `rollout.js:19704-19706`）：

```js
// src/lib/rollout.js:19707-19718（节选）
function grokEffectiveTotalFromSignals(signals) {
  if (!signals || typeof signals !== "object") return 0;
  const beforeCompaction = normalizeNonNegativeNumber(signals.totalTokensBeforeCompaction);
  const totalTokens = normalizeNonNegativeNumber(signals.totalTokens);
  if (signals.contextTokensUsed == null) { return beforeCompaction + totalTokens; }
  return Math.max(totalTokens, beforeCompaction + normalizeNonNegativeNumber(signals.contextTokensUsed));
}
```

- 上游成本（唯一一家直接用上游 ticks 的本地 provider）：

```js
// src/lib/grok-usage.js:12
const USD_TICKS_PER_USD = 10_000_000_000;
```

```js
// src/lib/grok-usage.js:26-39（节选）
function reportedCostUsd(usage, { costIsPartial, usageIsIncomplete }) {
  if (costIsPartial || usageIsIncomplete) return null;
  const ticks = Number(usage.costUsdTicks ?? usage.totalCostUsdTicks
    ?? usage.cost_usd_ticks ?? usage.total_cost_usd_ticks);
  if (Number.isFinite(ticks) && ticks >= 0) return ticks / USD_TICKS_PER_USD;
  const usd = Number(usage.costUsd ?? usage.totalCostUsd ?? usage.cost_usd ?? usage.total_cost_usd);
  return Number.isFinite(usd) && usd >= 0 ? usd : null;
}
```

- 精度合流：同一桶内混有 reported 与 estimated 时标记为 `"mixed"`（`rollout.js:19964-19968`）。
- `conversation_count`：turn 路径每条 `turn_completed` 记 1（`grok-usage.js:19801`）；兜底路径用助手消息数的增量（`rollout.js:20296-20302`）。

---

## 15. `parseAntigravityIncremental` — Google Antigravity

**1) source 标识**：`"antigravity"`（调用点显式传入，默认同值）：

```js
// src/lib/rollout.js:20806 / 20834（节选）
  source,
  ...
  const defaultSource = normalizeSourceInput(source) || "antigravity";
```

调用点：`src/commands/sync.js:1329`。

**2) 数据来源类型**：**自有 JSONL 转录 + SQLite 生成元数据（protobuf 二进制列）**，双源互补。转录路径：

```js
// src/lib/rollout.js:20559-20566
function resolveAntigravityBrainDirs(geminiHome) {
  if (!geminiHome || typeof geminiHome !== "string") return [];
  return [
    path.join(geminiHome, "antigravity", "brain"),
    path.join(geminiHome, "antigravity-ide", "brain"),
    path.join(geminiHome, "antigravity-cli", "brain"),
  ];
}
```

每个会话目录下的 `.system_generated/logs/transcript.jsonl`：`rollout.js:20604-20615`。

**3) SQLite 细节**：会话库 `conversations/<uuid>.db`，表 `gen_metadata`，查询原文：

```js
// src/lib/rollout.js:21586-21593（节选）
    const rows = readSqliteJsonRows(
      effectiveDbPath,
      "SELECT idx, quote(data) as hex FROM gen_metadata ORDER BY idx",
      { readOnly: true, throwOnReadFailure: true, maxBuffer: 64 * 1024 * 1024, timeout: 15_000 },
    );
```

- 用 `quote(data)` 把 BLOB 转成 `X'...'` 十六进制文本，再手工解 protobuf——**不是压缩列，是二进制 protobuf 列**（`rollout.js:21598-21599`）。
- 读取时对 UNC 路径先快照（`rollout.js:21573-21581`），并在失败与「读成功但为空」之间保持区分（注释 `21609-21611`：`null` = 可重试的瞬时失败，空 Map = 已成功读到但暂无元数据）。
- 库路径推导（同一 uuid 目录）：

```js
// src/lib/rollout.js:21558-21563（节选）
function resolveAntigravityDbPath(transcriptPath) {
  if (!transcriptPath || typeof transcriptPath !== "string") return null;
  const m = transcriptPath.match(/^(.*)[/\\]brain[/\\]([^/\\]+)[/\\]\.system_generated[/\\]logs[/\\]transcript.*\.jsonl$/);
  if (!m) return null;
  return path.join(m[1], "conversations", `${m[2]}.db`);
}
```

**4) 远程 API**：无。

**5) 字段映射**：两条完全不同的路径。

首选（SQLite protobuf，字段号硬编码）：

```js
// src/lib/rollout.js:21505-21535（节选）
      // Antigravity internal usage message layout (Field 4):
      // - f4.1: system / tool instruction prefix tokens (per-model fixed prefix)
      // - f4.2: user / prompt tokens for this turn
      // - f4.3: total output tokens checksum (f4.9 text + f4.10 reasoning)
      // - f4.5: cached prompt tokens (cache read)
      // - f4.9: text output tokens
      // - f4.10: reasoning / thought tokens
      ...
        const sysTokens = Number.isSafeInteger(sTok) ? sTok : 0;
        const promptTokens = Number.isSafeInteger(pTok) ? pTok : 0;
        uncachedInput = sysTokens + promptTokens;
        cachedInput = Number.isSafeInteger(cTok) ? cTok : 0;
        outputTokens = Number.isSafeInteger(oTok) ? oTok : 0;
        textOutput = Number.isSafeInteger(tTok) ? tTok : 0;
        reasoningOutput = Number.isSafeInteger(rTok) ? rTok : 0;
```

```js
// src/lib/rollout.js:21794-21808（节选）
        const uncached = dbTurn.uncachedInput || 0;
        const cached = dbTurn.cachedInput || 0;
        const reasoning = dbTurn.reasoningOutput || 0;
        let output = 0;
        if (Number.isFinite(dbTurn.textOutput) && dbTurn.textOutput > 0) { output = dbTurn.textOutput; }
        else if (Number.isFinite(dbTurn.outputTokens)) { output = Math.max(0, dbTurn.outputTokens - reasoning); }
        delta.input_tokens = uncached;
        delta.cached_input_tokens = cached;
        delta.output_tokens = output;
        delta.reasoning_output_tokens = reasoning;
        delta.total_tokens = uncached + cached + output + reasoning;
        delta.billable_total_tokens = delta.total_tokens;
        delta.conversation_count = 1;
```

兜底（转录字符估算）：

```js
// src/lib/rollout.js:21971-21990（节选）
function estimateAntigravityTokens(text) {
  if (typeof text !== "string" || text.length === 0) return 0;
  let cjk = 0; let other = 0;
  for (const ch of text) {
    if (isCjkCodePoint(ch.codePointAt(0))) { cjk += 1; } else { other += 1; }
  }
  return cjk + Math.ceil(other / 4);
}
function isCjkCodePoint(code) {
  return ((code >= 0x3400 && code <= 0x4dbf) || (code >= 0x4e00 && code <= 0x9fff)
    || (code >= 0x3040 && code <= 0x30ff));
}
```

```js
// src/lib/rollout.js:21821-21833（节选）
        const inputDelta = Math.max(0, contextTokens - previousContextTokens);
        const outputTokens = antigravityValueTokens(content) + antigravityValueTokens(parsed.tool_calls);
        const reasoningTokens = antigravityValueTokens(thinking);
        delta.input_tokens = inputDelta;
        delta.output_tokens = outputTokens;
        delta.reasoning_output_tokens = reasoningTokens;
        delta.total_tokens = inputDelta + outputTokens + reasoningTokens;
        delta.billable_total_tokens = delta.total_tokens;
        delta.conversation_count = 1;
```

- 兜底路径的 `cached_input_tokens` 与 `cache_creation_input_tokens` 保持 0（`initTotals` 默认）。
- 兜底路径的 `input` 是「上下文增量」而非真实 prompt 长度——上一轮 planner 的历史在下一轮被计为 input（注释 `rollout.js:21710-21713`、`21881-21887`）。
- 两条路径都写 `billable_total_tokens`。

**6) 模型名**：三级来源（转录内「设置变更」文本解析 → `settings.json` → SQLite f1.19），并做 slug 归一：

```js
// src/lib/rollout.js:21929-21949（节选）
  let slug = modelName.trim().replace(/\([^)]*\)/g, " ")
    .replace(/\b(thinking|xhigh|high|medium|low|fast)\b/gi, " ").toLowerCase()
    .replace(/[^a-z0-9.]+/g, "-").replace(/^-+|-+$/g, "").replace(/-{2,}/g, "-");
  ...
  for (const marker of ["gemini", "claude", "gpt"]) { const idx = slug.indexOf(marker); if (idx >= 0) { slug = slug.slice(idx); break; } }
  if (/^(gemini|claude|gpt)-/.test(slug)) return slug;
  return `antigravity-${slug}`;
```

默认模型兜底：`settings.json` 的 `model`（`rollout.js:21904-21918`）；桶内兜底字符串 `"antigravity-unknown"`（`rollout.js:21785`）。

**7) 时间戳**：转录事件的 `parsed.created_at`（`rollout.js:21771`）；缺失则该事件只累加上下文、不落桶（`21772-21783`）。

**8) 增量/去重**：

- 每文件游标 `cursors.files[filePath]` 记 `lastLine` / `size` / `inode` / `contributions` / `cursorVersion` / `usageSource`。
- `ANTIGRAVITY_CURSOR_VERSION = 2`（`rollout.js:20641`）、`ANTIGRAVITY_EXTRACTOR_REVISION = 1`（`20645`，与游标版本**分开**，用于「提取器开始读取以前丢弃的记录」时强制重放）。
- 归属账本按 `source|model|bucketStart|projectKey` 聚合（`rollout.js:21847-21858`），贡献条目上限 `ANTIGRAVITY_MAX_CONTRIBUTIONS = 4096`（`rollout.js:20646`）；账本被截断且需要撤回时会触发**源级重建**，而发现不完整（`inventoryComplete=false`）时改为**推迟该文件**而不是重建，避免抹掉未发现会话的贡献（逻辑 `20858-20908`）。
- SQLite 读失败且上次来源是 sqlite 时，整文件**推迟**（`deferred: true`），不回退到估算（`rollout.js:21682-21697`）。
- 全量状态用 `structuredClone` 隔离，两个队列都写成功才发布（`rollout.js:20820-20829`）。

**9) 特殊规则**：

- `conversation_count`：每个被计费的 `PLANNER_RESPONSE` +1（`rollout.js:21807`、`21832`）——请求级。
- 来源标记 `usageSource` 只记录在游标里（`"sqlite"` / `"estimated"`，`rollout.js:21898`），**不写进队列行的 `usage_precision`**——这一点与 Trae/Grok 不同（见 §26 披露缺口）。
- 同一转录里 `PLANNER_RESPONSE` 之外的其它事件类型也可能带 `created_at`，但只有 planner 才计费（`billedPlanner` 声明在 `rollout.js:21787`，赋值/判定在 `21808`、`21833`；`21788` 是空白行——R2 修正 ±1）。

---

## 16. `parseTraeIncremental` — TRAE SOLO（国际版，SQLCipher + WASM）

**1) source 标识**：`"trae"`（硬编码）：`rollout.js:22127`、`22131`、`22149`、`22171-22172`。

**2) 数据来源类型**：**加密 SQLite（SQLCipher 4）+ 内存快照 + WASM 引擎**，是最特殊的一家。

库路径解析：

```js
// src/lib/trae-db.js:15-24（节选）
function resolveTraeDbPaths(env = process.env, { platform = process.platform, home = os.homedir() } = {}) {
  const explicit = String(env.TOKENTRACKER_TRAE_DB || "").trim();
  if (explicit) return [path.resolve(explicit)];
  const customHome = String(env.TOKENTRACKER_TRAE_HOME || "").trim();
  const parent = platform === "win32" ? env.APPDATA || path.join(home, "AppData", "Roaming")
    : platform === "darwin" ? path.join(home, "Library", "Application Support")
      : env.XDG_CONFIG_HOME || path.join(home, ".config");
  const roots = customHome ? [customHome] : [path.join(parent, "Trae"), path.join(parent, "TRAE SOLO")];
```

文件名固定为 `<root>/ModularData/{ai-agent,ai-chat}/database.db`（`trae-db.js:29-42`）。

**3) SQLite 细节**：**不走系统 sqlite3，也不走 `node:sqlite`**，而是：

1. 先把（加密的）库 + WAL 读成**内存快照**并逐页解密（SQLCipher 4 raw key、AES-256-CBC、4096 字节页、SHA-512 HMAC）：

```js
// src/lib/trae-snapshot.js:38-45（节选）
function pageDecryptor(firstPage, env) {
  const hex = String(env.TOKENTRACKER_TRAE_SQLCIPHER_KEY || "").trim() || DEFAULT_SQLCIPHER_KEY;
  if (!/^[0-9a-f]{64}$/i.test(hex)) throw error("TOKENTRACKER_TRAE_SQLCIPHER_KEY must contain 64 hex characters.");
  const key = Buffer.from(hex, "hex");
  const salt = Buffer.from(firstPage.subarray(0, 16).map((byte) => byte ^ 0x3a));
  // SQLCipher 4 raw-key format: AES-256-CBC, 4096-byte pages, SHA-512 HMAC.
  const hmacKey = crypto.pbkdf2Sync(key, salt, 2, 32, "sha512");
```

内嵌的共享应用密钥常量：`trae-snapshot.js:10`。也支持 `TOKENTRACKER_TRAE_SQLCIPHER_KEY` 覆盖（`trae-snapshot.js:39-40`），`status` 里会披露用的是哪个来源（`src/commands/status.js:1215`）。

2. 再用 **`@journeyapps/wa-sqlite`（WASM）** 通过自定义只读 VFS 直接查这份内存快照，全程不落盘、不写回：

```js
// src/lib/trae-sqlite.js:19-32（节选）
  // Register one VFS for the lifetime of the engine. Each query gets a distinct
  // virtual filename, without accumulating WASM callbacks or touching disk.
  class TraeVFS extends FacadeVFS {
    ...
    jOpen(name, fileId, flags, outputFlags) {
      const state = snapshots.get(name);
      if (!state || !(flags & sqlite.SQLITE_OPEN_MAIN_DB)) return sqlite.SQLITE_CANTOPEN;
      handles.set(fileId, state);
      outputFlags.setInt32(0, sqlite.SQLITE_OPEN_READONLY, true);
      return sqlite.SQLITE_OK;
    }
```

```js
// src/lib/trae-sqlite.js:116
    await query("PRAGMA query_only=ON; PRAGMA cache_size=-4096; PRAGMA temp_store=MEMORY;");
```

写操作一律拒绝（`jWrite/jTruncate/jDelete` 返回 `SQLITE_READONLY`，`trae-sqlite.js:74-76`）；由于 WASM 上下文共享，查询被串行化到单引擎上（`trae-sqlite.js:135-141`）。

查询投影（只取 token 与模型字段，正文/费用/账号数据永不出库）：

```js
// src/lib/trae-db.js:71-84（节选）
  const fields = TOKEN_FIELDS.map((field) => `CASE
    WHEN json_type(context, '$.token_usage.${field}') IN ('integer', 'real')
    THEN json_extract(context, '$.token_usage.${field}') END AS "${field}"`);
  ...
  const query = `SELECT CAST(${column("id", "rowid")} AS TEXT) AS id,
    ${column("session_id")} AS session_id, ${column("turn_id")} AS turn_id,
    ${date} AS created_at, ${column("updated_at")} AS updated_at,
    COALESCE(${model.join(", ")}, 'trae-unknown') AS model,
    ${fields.join(", ")}, (${invalidFields.join(" OR ")}) AS invalid_usage
    FROM chat_turn WHERE CASE WHEN json_valid(context)
      THEN json_type(context, '$.token_usage') = 'object' ELSE 0 END`;
```

`TOKEN_FIELDS` 11 个候选列名：`trae-db.js:8-13`。

读取带 3 次重试 + 稳定性校验，绝不发布「混合快照」：

```js
// src/lib/trae-db.js:102-125（节选）
async function readTraeUsageRows(dbPath, { env = process.env } = {}) {
  for (let attempt = 0; attempt < 3; attempt++) {
    ...
      try { result = await withTraeSqlite(snapshot, projectUsage); }
      catch (err) {
        // A writer may have changed pages while SQLite was reading. Discard
        // every projected row and retry; never publish a mixed snapshot.
        if (snapshot.isStable()) throw err;
        continue;
      }
      if (snapshot.isStable()) return result;
```

**4) 远程 API**：无（国际版纯本地）。

**5) 字段映射**：交给 `normalizeTraeUsage`（最多候选舍入判定）。

```js
// src/lib/trae-usage.js:31-38（节选）
  const input = usage.prompt_tokens ?? usage.input_tokens ?? usage.prompt_tokens_total ?? 0;
  const output = usage.completion_tokens ?? usage.output_tokens ?? usage.completion_tokens_total ?? 0;
  const aggregateInput = usage.prompt_tokens_total > 0 ? usage.prompt_tokens_total : input;
  const aggregateOutput = usage.completion_tokens_total > 0 ? usage.completion_tokens_total : output;
  if (aggregateInput < input || aggregateOutput < output) return null;
  const cached = usage.cache_read_input_tokens ?? 0;
  let written = usage.cache_creation_input_tokens ?? 0;
  let reasoning = usage.reasoning_tokens ?? usage.reasoning_output_tokens ?? 0;
  let estimated = false;
```

唯一解判定（含/不含缓存与推理的组合必须只有一种与 `total_tokens` 自洽）：

```js
// src/lib/trae-usage.js:63-81（节选）
  const candidates = new Map();
  for (const includedCache of new Set([cached + written, cached, 0])) {
    for (const includedReasoning of new Set([reasoning, 0])) {
      const noncached = input - includedCache;
      const nonreasoning = output - includedReasoning;
      if (noncached < 0 || nonreasoning < 0) continue;
      if (noncached + cached + written + nonreasoning + reasoning !== total) continue;
      const totals = { input_tokens: noncached, cached_input_tokens: cached,
        cache_creation_input_tokens: written, output_tokens: nonreasoning,
        reasoning_output_tokens: reasoning, total_tokens: total };
      candidates.set(JSON.stringify(totals), totals);
    }
  }
  if (candidates.size !== 1) return null;
```

**两处明确的 Gemini 修复（都标 `estimated`）**：

```js
// src/lib/trae-usage.js:45-56（节选）
  if (isGemini && cached > 0 && written === cached && cached + written > input) {
    written = 0;
    estimated = true;
  }
  // Older TRAE Gemini rows omit thoughts while retaining Gemini's total
  // (prompt + candidates + thoughts). Infer only that documented residual...
  if (isGemini && usage.reasoning_tokens == null && usage.reasoning_output_tokens == null &&
    usage.total_tokens > input + output && written === 0 && cached <= input) {
    reasoning = usage.total_tokens - input - output;
    estimated = true;
  }
```

**不可定价的输入差额**（计入总量但不进可定价列）：

```js
// src/lib/trae-usage.js:83-92（节选）
  if (aggregateInput !== input || aggregateOutput !== output) {
    // The *_total counters cover the whole turn; the cache and reasoning fields
    // describe its final request. The difference in prompt tokens has no cache
    // split, so it counts toward total_tokens but no priced column.
    const unpriced = aggregateInput - input;
    totals.output_tokens += aggregateOutput - output;
    totals.total_tokens += unpriced + aggregateOutput - output;
    if (unpriced > 0) totals.unpriced_input_tokens = unpriced;
  }
```

**6) 模型名**：

```js
// src/lib/trae-usage.js:98-105（节选）
function normalizeTraeModel(value) {
  if (typeof value !== "string") return "trae-unknown";
  const model = value.trim().split("__")[0].toLowerCase().replace(/\s+/g, "-");
  // TRAE's display alias otherwise fuzzy-matches a regional Sonnet SKU and
  // loses cache pricing. Sonnet 4 has the fixed 2025-05-14 API model id.
  if (model === "claude-4-sonnet") return "claude-sonnet-4-20250514";
  return model && model !== "-" && !model.includes("|") ? model : "trae-unknown";
}
```

**7) 时间戳**：

```js
// src/lib/trae-usage.js:107-118（节选）
function traeTimestamp(value) {
  if (value == null || value === "") return null;
  let millis;
  if (typeof value === "number" || (typeof value === "string" && /^\d+(\.\d+)?$/.test(value))) {
    const numeric = Number(value);
    millis = numeric < 100_000_000_000 ? numeric * 1000 : numeric;
  } else if (typeof value === "string") { millis = Date.parse(value); }
  ...
  return Number.isFinite(date.getTime()) ? date.toISOString() : null;
}
```

`created_at` 优先、`updated_at` 用于冲突裁决（`rollout.js:22094`、`22107`）。**无兜底**（取不到即跳过该行，`22097-22100`）。

**8) 增量/去重**：紧凑元组账本 + 128 位摘要身份：

```js
// src/lib/rollout.js:22062（节选）
  const digest = (value) => crypto.createHash("sha256").update(value).digest().subarray(0, 16).toString("base64url");
```

```js
// src/lib/rollout.js:22104-22111（节选）
      // Stable turn ids deduplicate copies across the two international app
      // stores. Numeric SQLite ids are only unique within their database.
      const identity = digest(JSON.stringify(row.turn_id ? [session, row.turn_id] : [databaseKey, rowId]));
      const previous = turns[identity];
      const modifiedAt = Date.parse(traeTimestamp(row.updated_at) || timestamp);
      // A copied turn in another install can lag behind the original. Never
      // let an unrelated rescan of that stale store retract newer usage.
      if (previous && previous[T_STORE] !== store && modifiedAt <= previous[T_MODIFIED]) continue;
```

- 游标版本 `TRAE_CURSOR_VERSION = 2`（`rollout.js:22002`）；版本不匹配时**重建**（先清 trae 桶，`22049-22054`），且重建要求所有库都读成功，否则整轮回退（`22159-22164`）。
- 库级指纹（含 WAL、排除 shm）：`devinSqliteFingerprint`（`rollout.js:22080-22081`）。
- 重建后还要撤回旧桶对应的队列行：`retractStaleSourceQueueRows(queuePath, "trae", keepKeys)`（`rollout.js:22168-22173`）。

**9) 特殊规则**：

- 账本把 13 个字段压成数组（含精度与不可定价标记）：`rollout.js:22004-22012`。
- `conversation_count` 按会话首次为 1（`rollout.js:22115-22118`）。
- 精度写进队列行的 `usage_precision`（`"estimated"` / 缺省 `null`，`rollout.js:22144-22158`）。
- 无 `billable_total_tokens` 写入（由 `addTotals` 兜底）。

---

## 17. `parseTraeCnApiIncremental` — TRAE Work CN（国内版，账号级远程 API）

**1) source 标识**：常量 `"trae-cn"`（`rollout.js:22322` `TRAE_CN_SOURCE`），真实落桶点在 `rollout.js:22749-22751`（`getHourlyBucket` + `addTotals`；旧贡献撤回在 `22738-22746`、`subtractTotals`），account_session_state 记录（`source: TRAE_CN_SOURCE`）在 `22580`。注意 `22680` 是 fail-closed 抛错处（`Trae CN snapshot is not authoritative`），**不是落桶点**（R1 修正）。

**2) 数据来源类型**：**远程 HTTP API**（`trae-cn` 与 `cursor` 是仅有的两个账号级来源，见 `source-metadata.js:1`）。

**3) SQLite**：不涉及。

**4) 远程 API**（端点为编译期常量，用户不可改向）：

```js
// src/lib/trae-cn-config.js:37-45（节选）
const TRAE_CN_USAGE_URL = "https://api.trae.cn/trae/api/v1/pay/query_user_usage_group_by_session";
// usage_type [7] mirrors the constant the official TRAE client sends when its
// usage page queries this endpoint (observed in the client's outbound
// requests; the enum is undocumented server-side). Other values unverified.
const TRAE_CN_USAGE_TYPE = [7];
const TRAE_CN_USAGE_PAGE_SIZE = 20;
const TRAE_CN_USAGE_MAX_PAGES = 100;
const TRAE_CN_USAGE_DELAY_MS = 300;
const TRAE_CN_USAGE_TIMEOUT_MS = 30 * 1000;
```

请求与鉴权头：

```js
// src/lib/trae-cn-config.js:332-344（节选）
      response = await fetchImpl(TRAE_CN_USAGE_URL, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: `Cloud-IDE-JWT ${jwt.trim()}` },
        body: JSON.stringify({ usage_type: TRAE_CN_USAGE_TYPE, start_time, end_time, page_num, page_size }),
        signal: controller.signal,
      });
```

**凭据读取（加密 blob 解密）**：`storage.json` 的 `iCubeAuthInfo://icube.cloudide`（`trae-cn-config.js:35`），可能是明文 JSON，也可能是 Base64 的 tc v5 密文：

```js
// src/lib/trae-cn-config.js:11-16（节选）
 * tc v5 blob: [6-byte magic "tc\x05\x10\x00\x00"][32-byte salt][AES-128-CBC ct]
 *   kdf_buf  = SHA-512(salt) || (JG xor KG)   // 128 bytes
 *   kdf_out  = SHA-512(kdf_buf)               // 64 bytes
 *   key = kdf_out[0..16], iv = kdf_out[16..32]
 *   plaintext = [SHA-512(json)][json]   // integrity prefix stripped
```

**opt-in 开关**（唯一一处「读本机登录凭据并发出请求」的显式闸门）：

```js
// src/lib/trae-cn-config.js:24-33（节选）
// The TRAE CN usage read transmits the locally stored sign-in JWT to TRAE's
// official endpoint, so it is strictly opt-in: nothing is sent unless the
// user explicitly sets this flag. Any value other than 1/true keeps it off.
const TRAE_CN_USAGE_ENV = "TOKENTRACKER_TRAE_CN_USAGE";
function isTraeCnUsageEnabled(env = process.env) {
  const value = env ? env[TRAE_CN_USAGE_ENV] : undefined;
  const normalized = typeof value === "string" ? value.trim().toLowerCase() : "";
  return normalized === "1" || normalized === "true";
}
```

调用点条件（还需非轻量同步 + source 允许 + storage 存在）：

```js
// src/commands/sync.js:2234-2238（节选）
    if (
      !isBackgroundLightweightSync &&
      isTraeCnUsageEnabled(process.env) &&
      sourceAllowed("trae-cn")
    ) {
```

调用点还固定 30 天滚动窗口并对齐半小时（`sync.js:2241-2250`）。

**失败降级**：401/403 时**重读并重新解密 storage 后重试一次**，不做 refresh token 流程：

```js
// src/lib/trae-cn-config.js:486-521（节选）
 * High-level storage-backed fetch: read the JWT from storage.json, fetch
 * usage, and on a 401/403 re-read + re-decrypt storage.json and retry exactly
 * once. Never persists or logs the JWT / refresh token; no refreshToken flow.
  ...
    const authFailure = error?.code === "AUTH_EXPIRED" || error?.status === 401 || error?.status === 403;
    if (!authFailure) throw error;
    // Session may have rotated since the first read; re-read storage once.
    auth = readAuth(storageOptions);
```

容量超限时递归二分窗口（最多 8 层），二分仍超限就 fail-closed（`trae-cn-config.js:46-49`、`465-483`）。

**5) 字段映射**（cache 包含式 input 必须扣）：

```js
// src/lib/rollout.js:22475-22509（节选）
  const cacheRead = traeCnTokenField(row, extra, "cache_read_token");
  const cacheWrite = traeCnTokenField(row, extra, "cache_write_token");
  const fields = [
    ["input_token", traeCnTokenField(row, extra, "input_token")],
    ["output_token", traeCnTokenField(row, extra, "output_token")],
    ...
  ];
  const rawInput = fields[0][1];
  const outputTokens = fields[1][1];
  const cachedInput = Math.min(rawInput, fields[2][1]);
  const cacheCreation = Math.min(rawInput - cachedInput, fields[3][1]);
  const total = rawInput + outputTokens;
  ...
    totals: {
      input_tokens: rawInput - cachedInput - cacheCreation,
      output_tokens: outputTokens,
      cached_input_tokens: cachedInput,
      cache_creation_input_tokens: cacheCreation,
      reasoning_output_tokens: 0,
      total_tokens: total,
      billable_total_tokens: total,
      conversation_count: 1,
    },
```

字段可从 `extra_info`（对象或 JSON 字符串）或行顶层读取（`rollout.js:22430-22434`）。源码给出的「input 含缓存」证据链是**用自家积分账单反推验证**的：

```js
// src/lib/rollout.js:22467-22474（节选）
  // `input_token` is cache-INCLUSIVE: verifying real rows against TRAE CN's
  // own credit billing, credits = fresh_input*p + cache_read*(p/4) + output*q
  // fits every row with zero residual only under this reading (same
  // convention as Codex / Qoder). Peel the cache subsets off into their own
  // columns and report only the remainder as ordinary input, otherwise cached
  // context is double-counted in dashboards and cost calculations. cache_write
  // was 0 across all observed rows; it is treated as a further subset ...
```

- `total = rawInput + outputTokens`（**因为 rawInput 已含缓存，不能再加 cached/cacheWrite**，否则重复）。
- `reasoning_output_tokens` 恒 0。

**6) 模型名**：`row.model_name`；缺失落 `"trae-cn-unknown"`（`rollout.js:22324`、`22445-22454`）；含 `|` 视为不支持并抛错。

**7) 时间戳**：`usage_time`（**Unix 秒**）×1000：

```js
// src/lib/rollout.js:22455-22461（节选）
  if (!Number.isSafeInteger(row?.usage_time) || row.usage_time <= 0) {
    throw new Error("Trae CN session row has an invalid usage_time.");
  }
  const bucketStart = toUtcHalfHourStart(row.usage_time * 1000);
```

**8) 增量/去重**：**会话级快照对账**，全部 fail-closed。

- 游标 `cursors.traeCn = { version: 1, sessions: {...}, prunedBeforeMs, updatedAt }`（`rollout.js:22326-22351`）。
- 持久化贡献在减去前逐字段校验（安全整数、`reasoning=0`、`conversation_count=1`、`total=input+cached+cacheWrite+output`、`billable=total`、桶起点必须是标准半小时 UTC）：

```js
// src/lib/rollout.js:22402-22413（节选）
  const { totals } = entry;
  if (totals.reasoning_output_tokens !== 0 || totals.conversation_count !== 1) {
    throw new Error("Trae CN stored session totals are malformed.");
  }
  const sum = totals.input_tokens + totals.cached_input_tokens
    + totals.cache_creation_input_tokens + totals.output_tokens;
  if (totals.total_tokens !== sum || totals.billable_total_tokens !== totals.total_tokens) {
    throw new Error("Trae CN stored session totals are malformed.");
  }
```

- 撤回前还要断言旧贡献不超过当前桶（防游标被篡改后把桶减成负数）：`assertTraeCnBucketCovers`（`rollout.js:22513-22530`）。
- 空响应是**成功空操作**（不当作「全部删除」），单个畸形行让**整份快照**失败（不作为权威状态入队）：

```js
// src/lib/rollout.js:22626-22634（节选）
    // Evidence check (2026-08-16, two real fetches 17min apart over the same
    // account: 137 -> 141 sessions, 0 disappeared, 0 rows changed) shows the
    // session set is stable, but NOTHING proves 'absent from the response'
    // means 'authoritatively deleted / zero' — so the absence contract is
    // NOT PROVEN and must stay symmetric everywhere ...
    return { recordsProcessed: 0, eventsAggregated: 0, bucketsQueued: 0 };
```

- 跨设备：额外写一批 `kind: "account_session_state"` 队列记录，带 `snapshot_verified_at` 逻辑戳，云端 LWW upsert 收敛（注释 `rollout.js:22532-22567`，实现 `22568-22595`）。

**9) 特殊规则**：

- `conversation_count` 恒 1（会话行即一次会话）。
- `billable_total_tokens` = `total_tokens`。
- 队列行的 `total_cost_usd` 不写（由下游定价）。

## 18. `parseDshIncremental` — DeepSeek Harness

**1) source 标识**：常量 `"dsh"`（`rollout.js:22820`），落桶处 `rollout.js:23965-23967`。

**2) 数据来源类型**：自有会话日志（JSONL 或 **zstd 压缩的拼接帧**）。目录：

```js
// src/lib/rollout.js:22878-22880（节选）
// Walk the harness sessions root for per-session log artifacts. The tree is
// `<sessions-root>/<project-key>/<session-id>/session[.v3].jsonl[.zstd]`; only
// the matching leaf names are collected so unrelated harness files are ignored.
```

家目录 `TOKENTRACKER_DSH_HOME` → `DSH_HOME` → `~/.dsh`（`rollout.js:22826-22833`），Windows 可再探 WSL（`22839-22865`）。同名文件多个版本时选「最近写入优先，其次更高格式版本，再次 zstd」（`rollout.js:22901-22925`）。

**3) SQLite**：不涉及。

**4) 远程 API**：无。

**5) 字段映射**：

```js
// src/lib/rollout.js:23345-23365（节选）
// Convert a harness `TokenUsage` object into our queue `totals` delta. Counts
// are disjoint, so every column maps 1:1; returns null for an all-zero usage.
function dshUsageToTotals(usage) {
  if (!usage || typeof usage !== "object") return null;
  const input = toNonNegativeInt(usage.inputTokens);
  const output = toNonNegativeInt(usage.outputTokens);
  const cacheRead = toNonNegativeInt(usage.cacheReadTokens);
  const cacheWrite = toNonNegativeInt(usage.cacheWriteTokens);
  const reasoning = toNonNegativeInt(usage.reasoningTokens);
  if (input === 0 && output === 0 && cacheRead === 0 && cacheWrite === 0 && reasoning === 0) { return null; }
  return {
    input_tokens: input, cached_input_tokens: cacheRead,
    cache_creation_input_tokens: cacheWrite, output_tokens: output,
    reasoning_output_tokens: reasoning,
    total_tokens: input + output + cacheRead + cacheWrite + reasoning,
    conversation_count: 1,
  };
}
```

- `inputTokens` 是**纯非缓存输入**（注释 `rollout.js:22804-22806` 明确「DISJOINT … no cache subtraction, unlike Codex」），不做扣减。
- 五分量相加得 total。无 `billable_total_tokens`。

**6) 模型名**：事件内嵌 `data.message.source.model`，回落 `request/header` 配置里的模型，再回落事件自身 `model`：

```js
// src/lib/rollout.js:23649-23652（节选）
    const model = normalizeDshModelName(
      (source && parseDshJsonString(findDshJsonProperty(source, "model")))
        || parseDshJsonString(findDshJsonProperty(data, "model")),
    ) || headerModel;
```

```js
// src/lib/rollout.js:23336-23342（节选）
function normalizeDshModelName(value) {
  ...
  const slash = trimmed.lastIndexOf("/");
  const name = slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
```

无模型即跳过该事件（`rollout.js:23658`）。

**7) 时间戳**：事件级 `time` / `timestamp`（epoch 毫秒），缺失即跳过事件：

```js
// src/lib/rollout.js:23660-23666（节选）
    const timeMs = parseDshJsonNumber(findDshJsonProperty(line, "time"))
      || parseDshJsonNumber(findDshJsonProperty(line, "timestamp"))
      || parseDshJsonNumber(findDshJsonProperty(data, "time"))
      || parseDshJsonNumber(findDshJsonProperty(data, "timestamp"));
    if (!Number.isFinite(timeMs) || timeMs <= 0) continue;
```

**8) 增量/去重**：

- 文件级 `cursors.dsh.files[path]`（inode/size/mtime 身份），未变即跳过（逻辑入口 `rollout.js:23761-23793`）。
- **记录级 `seq` 水位**：

```js
// src/lib/rollout.js:23628-23645（节选）
    const seq = parseDshJsonNumber(findDshJsonProperty(line, "seq"));
    const seqKnown = Number.isFinite(seq) && seq >= 0;
    if (seqKnown && seq > maxSeq) maxSeq = seq;
    ...
    if (eventType !== "assistant/message" && eventType !== "message/assistant") continue;
    if (seqKnown && seq <= watermark) continue;
```

- 撕裂行防护：行必须自身是**完整 JSON** 才推进水位，遇到不完整行就停在该前缀并让下次重试：

```js
// src/lib/rollout.js:23610-23615（节选）
    if (!isCompleteDshJsonLine(line)) {
      complete = false;
      // A later event cannot make this gap safe to acknowledge. Stop at the
      // complete prefix so a repaired record is retried before seq advances.
      break;
    }
```

- 会话级贡献账本（`cursors.dsh.sessions`）支持「同会话换文件路径」迁移：新路径接管时先撤回旧路径贡献，路径消失时保留旧路径待重试（`rollout.js:23717-23746`、`23766-23786`）。
- 全量状态用 `structuredClone` 隔离（`rollout.js:23690-23703`）。

**9) 特殊规则**：

- 解析刻意**不做整行 JSON 解码**（用 `findDshJsonProperty` 切片取字段），以避免物化消息正文（注释 `rollout.js:23368-23372`、`24244-24248`）。
- **zstd 是自研帧解析**：先扫描拼接帧的 `contentSize` 声明做解压炸弹防护，再逐帧解码；解码优先 Node 内置 zstd，Node 20 回落 `@mongodb-js/zstd`：

```js
// src/lib/rollout.js:23060-23072（节选）
// Decode one already-isolated frame. Prefer Node's built-in zstd (22.15+):
// the desktop bundles install dependencies with --ignore-scripts, so
// @mongodb-js/zstd ships without its native binding there and the require()
// would throw — silently skipping every compressed session (issue #465)...
async function decodeDshZstdFrame(frameBytes) {
  const zlib = require("node:zlib");
  if (typeof zlib.zstdDecompressSync === "function") { return zlib.zstdDecompressSync(frameBytes); }
  return Buffer.from(await require("@mongodb-js/zstd").decompress(frameBytes));
}
```

- 上限常量：单文件压缩 64MB（`DSH_SESSION_LOG_MAX_BYTES`，`rollout.js:22818`）、解压后 128MB（`DSH_SESSION_TEXT_MAX_BYTES`，`22819`）。
- 无 `usage_precision`、无 cost；`conversation_count` 每条 assistant 消息 +1（`rollout.js:23364`）。

---

## 19. `parseCommandCodeIncremental` — Command Code (`cmd`)

**1) source 标识**：常量 `"command-code"`（`rollout.js:24067`），落桶处 `24572-24576`。

**2) 数据来源类型**：自有 JSONL 转录。目录：

```js
// src/lib/rollout.js:24129-24132（节选）
// Walk `<home>/projects/<cwd-slug>/` for `<session-id>.jsonl` transcripts. The
// sibling checkpoint snapshots and `.prompts.` sidecars are not transcripts
// and must not inflate discovery or status session counts.
```

家目录 `TOKENTRACKER_COMMANDCODE_HOME` → `~/.commandcode`（`rollout.js:24085-24089`），Windows 可探 WSL（`24095-24127`）；`~/.commandcode/projects` 常量见 `24071-24072`。排除规则：`.prompts.` 与 `.checkpoints.jsonl`（`24074-24081`）。

**3) SQLite**：不涉及。

**4) 远程 API**：无。

**5) 字段映射**：

```js
// src/lib/rollout.js:24187-24215（节选）
// Map Command Code's usage object onto disjoint queue columns. `inputTokens`
// already includes cache reads and writes (see the section comment), so subtract
// both back out first. Explicit numeric zero usage can correct an existing
// observation; empty placeholders are not usage. Cost uses model-table pricing.
function commandCodeUsageToTotals(usage) {
  ...
  const inclusiveInput = toNonNegativeInt(usage.inputTokens);
  const cachedInput = toNonNegativeInt(usage.cacheReadTokens);
  const cacheWrite = toNonNegativeInt(usage.cacheWriteTokens);
  const output = toNonNegativeInt(usage.outputTokens);
  const input = Math.max(0, inclusiveInput - cachedInput - cacheWrite);
  const total = input + cachedInput + cacheWrite + output;
  const explicitZero = ...;
  if (total === 0 && !explicitZero) return null;
  return { input_tokens: input, cached_input_tokens: cachedInput,
    cache_creation_input_tokens: cacheWrite, output_tokens: output,
    reasoning_output_tokens: 0, total_tokens: total,
    billable_total_tokens: total, total_cost_usd: 0,
    conversation_count: total > 0 ? 1 : 0 };
}
```

- `inputTokens` 含缓存 → 双扣（与 §11 CLI span、§12 同族语义）。
- **`reasoning_output_tokens` 恒 0**；**`total_cost_usd` 恒 0**（注释见 §19.9）。
- **显式全零视为校正**（能撤回既有观测），空占位不视为使用（`24199-24204`）。

**6) 模型名**：行字段 `model` 取最后一个 `/` 之后：

```js
// src/lib/rollout.js:24176-24185（节选）
// Row models are provider-qualified ("deepseek/deepseek-v4.1-flash"); the
// pricing tables and bucket keys use the bare id, matching the dsh reader.
function normalizeCommandCodeModelName(value) {
  ...
  const slash = trimmed.lastIndexOf("/");
  const name = slash >= 0 ? trimmed.slice(slash + 1) : trimmed;
```

缺失落 `DEFAULT_MODEL`（`"unknown"`，`rollout.js:24283`）。

**7) 时间戳**：行内 `timestamp` 字符串直接进桶（不解析为毫秒）：

```js
// src/lib/rollout.js:24262-24276（节选）
  const timestamp = parseDshJsonString(findDshJsonProperty(raw, "timestamp"));
  if (!id || !timestamp) return null;
  ...
  const bucketStart = toUtcHalfHourStart(timestamp);
  if (!bucketStart) return null;
```

即时间戳必须能被 `new Date(...)` 解析，否则该记录被丢弃。

**8) 增量/去重**：**文件快照 + 观测账本**，明确不用字节偏移：

```js
// src/lib/rollout.js:24060-24066（节选）
// Transcripts are append-only in practice, but a resume/compaction REWRITES the
// file, so byte offsets are the wrong cursor shape here. This reader rebuilds a
// per-file snapshot and reconciles it against a durable observation ledger keyed
// by `recordId|timestamp`. Forks and clones copy those fields into transcripts
// with new session headers, so the header cannot be part of the key. Files whose
// (size, mtime) pair is unchanged can reuse their owned ledger records. A
// non-owning duplicate is re-read rather than storing a second full ledger.
```

- 记录身份：`${COMMAND_CODE_SOURCE}:${record.id}|${record.timestamp}`（`rollout.js:24542`），另有 legacy 键形 `command-code:<sessionId>|<id>`（`24544`）。
- 游标版本：`COMMAND_CODE_STATE_VERSION = 3`、`COMMAND_CODE_FILE_CACHE_VERSION = 3`，不匹配即标记 `schemaMigrated`（`24068-24069`、`24473-24475`）。
- 归属冲突：同一记录出现在非拥有者文件里时，**重新读取该文件**以解析冲突，而不是双记账本：

```js
// src/lib/rollout.js:24614-24627（节选）
  for (const [filePath, index] of Object.entries(nextFileIndex)) {
    // Check ownership before dropping uncounted keys: a positive record that
    // lost to a zero in another file is still a non-owning snapshot.
    const ownsAllRecords = index.messageKeys.every((key) => currentOwners.get(key) === filePath);
    index.completeOwnedSnapshot = ownsAllRecords;
    // Retain the identities of uncounted zeros without full message metadata.
    const zeroKeys = [...new Set([...])];
    if (zeroKeys.length) index.zeroKeys = zeroKeys;
    index.messageKeys = index.messageKeys.filter((key) => state.messages[key]);
  }
```

- 零 token 记录只在能撤回既有计数时保留（`rollout.js:24606-24612`）。
- 成本无关变化不触发入队（`prepareBuckets`，`24629-24641`）。
- 全文状态 `structuredClone` 隔离 + 两个队列都成功才发布（`24468-24479`、`24650-24662`）。

**9) 特殊规则**：

- 成本口径说明（逐字）：

```js
// src/lib/rollout.js:24055-24058（节选）
//  2. `costUsd` is Command Code's display-rate estimate, not a billed amount.
//     Ignore it and emit the zero cost sentinel; local readers and cloud
//     endpoints estimate cost from the shared model price table.
```

即**上游成本被主动丢弃**，写 `total_cost_usd: 0` 哨兵（与 Grok 相反）。

- `conversation_count`：`total > 0` 时 1，否则 0（`rollout.js:24214`）——请求级。
- 记录完整性用括号配对检查而非 JSON 解码（`isCompleteCommandCodeRecord`，`rollout.js:24220-24241`），撕裂尾巴不贡献、下次同步补齐（`24289-24290`）。
- 项目归属由首行 `cwd` 提供（`rollout.js:24253-24258`、`24512-24517`）。

## 20. 判定表：离线可用性 / 凭据依赖 / opt-in 开关

### 20.1 完全没有网络、未登录时仍可用（纯本地文件）

| # | 解析器 | source | 依据（源码行） |
|---|--------|--------|----------------|
| 1 | `parseOpencodeDbIncremental` | `opencode` | 只读 `opencode.db`（`sync.js:1369-1381`），无网络调用 |
| 2 | `parseQoderDbIncremental` | `qoder` | 只读 `local.db`（`rollout.js:6129-6135`） |
| 3 | `parseQoderNewIncremental` | `qoder` | 只读 `~/.qoder/projects/**/*.jsonl`（`rollout.js:6457-6474`） |
| 4 | `parseDevinIncremental` | `devin` | 只读 `sessions.db`（`rollout.js:14172-14205`） |
| 5 | `parseOmpLikeIncremental`→`parseOmpIncremental` | `omp` | 只读 `~/.omp/agent/sessions`（`rollout.js:11433-11435`） |
| 6 | `parseOmpLikeIncremental`→`parseOmoIncremental` | `omo` | 只读 OmO agent dir（`rollout.js:11604-11607`） |
| 7 | `parsePiLikeIncremental`→`parsePiIncremental` | `pi` / `pi-<provider>` | 只读 `~/.pi/agent/sessions` + `~/.multica/pi-sessions`（`rollout.js:16171-16223`） |
| 8 | `parsePiLikeIncremental`→`parsePrimeAgentIncremental` | `prime-agent` / `prime-agent-<provider>` | 只读 `~/.prime/agent/sessions`（`rollout.js:16258-16275`） |
| 9 | `parsePiLikeIncremental`→`parseMinimaxCodeIncremental` | `minimax-code` | 只读 `~/.minimax/v2/sessions/**/messages.jsonl`（`rollout.js:16745-16764`） |
| 10 | `parseCraftIncremental` | `craft` | 只读 `session.jsonl` 首行 + `config.json`（`rollout.js:16830-16900`） |
| 11 | `parseReasonixIncremental` | `reasonix` | 只读 `*.jsonl.telemetry.json`（`rollout.js:16951-16976`） |
| 12 | `parseCopilotIncremental`（OTEL 通道） | `copilot` | 只读 `~/.copilot/otel/*.jsonl`；需用户先开启 OTEL **导出**，但读侧完全离线（`rollout.js:17309-17341`） |
| 13 | `parseCopilotSessionStoreIncremental` | `copilot` | 只读 `session-store.db`（`rollout.js:18158-18166`） |
| 14 | `parseCopilotAppDbIncremental` | `copilot` | 只读 `data.db`（`rollout.js:19190-19208`） |
| 15 | `parseGrokBuildIncremental` | `grok` | 只读 `~/.grok/sessions/**`（`rollout.js:19566-19605`）；hook 只是「让同步更早发生」，非必需 |
| 16 | `parseAntigravityIncremental` | `antigravity` | 只读 `brain/**/transcript.jsonl` + `conversations/<uuid>.db`（`rollout.js:20559-20624`、`21558-21593`） |
| 17 | `parseTraeIncremental` | `trae` | 只读（解密）本地 `database.db`（`trae-db.js:15-45`） |
| 18 | `parseDshIncremental` | `dsh` | 只读 `~/.dsh/sessions/**`（`rollout.js:22881-22936`） |
| 19 | `parseCommandCodeIncremental` | `command-code` | 只读 `~/.commandcode/projects/**`（`rollout.js:24132-24159`） |

即 **19/21 家离线可用**（21 − 2 家必须联网 = 19，与 §0 结论 1 一致）。

### 20.2 必须联网或必须读取凭据

| # | 解析器 | source | 硬依赖 |
|---|--------|--------|--------|
| 20 | `parseCursorApiIncremental` | `cursor` | **必须联网**（CSV 导出）+ **必须读凭据**（`state.vscdb` 的 JWT + `cli-config.json` 的 userId，组装 `WorkosCursorSessionToken` cookie）。两者缺一即整段跳过：`sync.js:2158-2160`。 |
| 21 | `parseTraeCnApiIncremental` | `trae-cn` | **必须联网** + **必须读凭据**（`storage.json` 里可能是 AES-128-CBC 加密的 JWT blob）+ **必须显式 opt-in**。 |

另有两家属于「本地可用但有更权威的联网路径」，本任务范围内只涉及本地路径：
- `parseGrokBuildIncremental`：`grok-limits.js` 另有配额查询（不在本文范围），但用量解析本身不联网。
- `parseTraeIncremental`：国际版本地加密库；`readTraeEntitlementFromStorage`（定义于 `rollout.js:22269`；`22236-22260` 是其相邻的 `normalizeTraeEntitlement`，原稿行号指向了该函数）读的是本地 `storage.json` 的权益快照，也不联网。该函数唯一非导出调用点在 `src/commands/status.js:840`（状态展示），**不进用量解析路径**（R1 修正）。

### 20.3 opt-in 环境变量开关（含源码行）

| 变量名 | 作用域 | 语义 | 源码行 |
|--------|--------|------|--------|
| `TOKENTRACKER_TRAE_CN_USAGE` | `trae-cn` | **未设置则完全不同步该源**；只有 `1`/`true` 才启用。这是唯一由「远程读取凭据」触发的强制闸门 | `trae-cn-config.js:27-33`；调用点 `sync.js:2234-2238` |
| `COPILOT_OTEL_ENABLED` / `COPILOT_OTEL_EXPORTER_TYPE` / `COPILOT_OTEL_FILE_EXPORTER_PATH` | `copilot`（OTEL 通道） | 由 **Copilot 自己**消费、决定是否写导出文件；TokenTracker 只是读目录。注意 TokenTracker 自己也读该变量做状态提示 | `rollout.js:17301-17306`（注释）、`17336-17339`（读显式路径）；`status.js:1519-1526`（提示文案）；`usage-limits.js:1922`（另一处判定） |
| `TOKENTRACKER_OPENCODE_GO_LOCAL_ESTIMATE` | **不属于本 21 家**（属于 OpenCode Go 配额模块） | 「仅本地估算」开关，用 `opencode.db` 里每次 Go turn 的 USD `cost` ÷ 官方美元上限做估算，`source` 标记为 `'local-estimate'` | `opencode-go-limits.js:19-28`、`51-59` |
| `TOKENTRACKER_OPENCODE_GO_LIMITS` | 同上 | 自定义配额窗口（如 `"12,30,60"`） | `opencode-go-limits.js:323-327` |
| `TOKENTRACKER_TRAE_SQLCIPHER_KEY` | `trae` | **不是 opt-in，是可选的密钥覆盖**：Trae 库用内嵌共享密钥解密，此变量仅在密钥变更/自定义时使用 | `trae-snapshot.js:9-10`（默认常量）、`38-40`（覆盖与格式校验）；状态披露 `status.js:1215` |

**结论**：本 21 家中，**真正意义上的「功能 opt-in 开关」只有 `TOKENTRACKER_TRAE_CN_USAGE` 一个**。Copilot 的三个 `COPILOT_OTEL_*` 是「让上游工具开始导出」的前置条件（对 TokenTracker 而言是数据存在性条件，而非自身开关）。其余变量全部是**路径覆盖**（见 §24 附录），不是能力开关。

### 20.4 数据来源类型总表

| 类型 | 项数 | 成员 |
|------|------|------|
| SQLite（本机文件） | 7 | `opencode`、`qoder`、`devin`、`copilot`(store)、`copilot`(app db)、`antigravity`、`trae`（加密） |
| 自有 JSONL / JSON 快照 | 13 | `qoder`(new)、`omp`、`omo`、`pi`、`prime-agent`、`minimax-code`、`craft`、`reasonix`、`copilot`(otel)、`grok`、`antigravity`、`dsh`、`command-code` |
| 远程 HTTP API | 2 | `cursor`、`trae-cn` |
| SQLite 只用于取凭据 | 1 | `cursor`（`state.vscdb`） |
| LevelDB | 0 | — |

（**项数按解析通道计**：`copilot` 三份实现分别是 SQLite / SQLite / JSONL（3 项）、`qoder` 占 DB 与 JSONL 两行、`antigravity` 是「JSONL + SQLite」双源（2 项）。**「家」口径（独立数据源实体，按 source 与解析器计）为 21 家**——即 §20.1 表 19 行 + §20.2 表 2 行；本表项数之和大于 21 是因为上述多家占多行。R1 修正：§0 原稿此处写「8 家」，已统一为本表口径「SQLite 数据源 7 项」。**R2 补充（去重口径）**：与 S3a（JSONL / 文本通道，19 家）合并时，两片名单的**唯一重叠是 `opencode`**（S3a 记其 JSONL 支路、本文记 `opencode.db` 支路，见 §2），故**去重后唯一 provider / 解析通道实体数 = 19 + 21 − 1 = 39**；读者按「家」汇总时不要直接把 19 与 21 相加。）

---

## 21. 风险点：非 token 计费单位被当作 token 或反推 token

逐条列出，全部给源码证据。

### 21.1 `qoder` 新 JSONL：`credits` 被写进 totals 的 `credits` 字段（不换算、不入 total）

```js
// src/lib/rollout.js:6541-6543（节选）
      credits: Number.isFinite(credits) && credits > 0 ? credits : 0,
      usage_precision: null,
```

- 该字段来自 `usage.credits ?? usage.original_credits`（`rollout.js:6515`）。
- **`addTotals` 只识别九个字段**（`rollout.js:4099-4115`），`credits` 不在其中，因此它**不会进入桶的总量**。
- 结论：**存在「credits 挂在 totals 上的形态风险」但当前不构成数值污染**；真正的风险是**下游若按 totals 的任意字段做泛化求和，会误用 credits**（建议复核消费侧是否遍历 totals 的键）。
- **悬空注释（注释与实现不一致，须登记）**：`rollout.js:6726` 写着 `// addTotals handles total_cost_usd via USD_TICKS, but credits cost is small; ensure it accumulates`，但其下一行 `6727` 就是 `touchedBuckets.add(...)`——**上下文里没有任何 credits 累积语句**。「说要累积、实现没累积」的残留注释本身就是该家 credits 风险的直接证据；下游必须按 totals 白名单消费，不得依赖该注释所称的行为。
- 同一家的注释明确拒绝换算：`rollout.js:6405-6411`「no first-party evidence for a credit→token rate … total_cost_usd stays 0」。

### 21.2 `reasonix`：`requestCount` 直接当 `conversation_count`

```js
// src/lib/rollout.js:17044
    conversation_count: delta.requests,
```

- `requests` 来自 `usage.requestCount`（`rollout.js:17004`）。
- 风险：**「请求数」冒充「会话数」**，会让「会话/对话数」这一指标在跨 provider 汇总时被抬高。这不是 token 污染，但会让 `conversation_count` 语义在 21 家之间不一致（对比：Craft 是会话级、OMP/Pi 是消息级、Cursor 是 CSV 行级）。

### 21.3 `grok` 兜底路径：上下文窗口 token → 按 0.8 硬拆 input/output

```js
// src/lib/rollout.js:19549
const GROK_ESTIMATED_INPUT_RATIO = 0.8;
```

```js
// src/lib/rollout.js:19945-19961（节选）
function estimateGrokTokenDelta(totalTokens, conversationCount, options = {}) {
  const total = Math.trunc(normalizeNonNegativeNumber(totalTokens));
  const inputTokens = Math.round(total * GROK_ESTIMATED_INPUT_RATIO);
  const outputTokens = Math.max(0, total - inputTokens);
  ...
    cached_input_tokens: 0, cache_creation_input_tokens: 0,
    output_tokens: outputTokens, reasoning_output_tokens: 0,
```

- 触发条件：该会话历史上从未出现 `turn_completed`（`rollout.js:20238-20241`）。
- 数据源是**上下文窗口的瞬时大小**而非可计费累计（源码自己的警告：`rollout.js:19704-19706`）。
- 该路径**标记**了 `usage_precision = "estimated"`（`rollout.js:20255`、`20268`）并参与桶级精度合流（`20286-20289`）——这是它的免责披露。
- 但**缓存与推理在该路径恒为 0**，因此「估算行」的价格会系统性地与真实行不同。

### 21.4 `trae`（国际版）：从总量反推 opaque 分量（Gemini 特例）

```js
// src/lib/trae-usage.js:49-56（节选）
  // Older TRAE Gemini rows omit thoughts while retaining Gemini's total
  // (prompt + candidates + thoughts). Infer only that documented residual,
  // never an arbitrary provider's unexplained total.
  if (isGemini && usage.reasoning_tokens == null && usage.reasoning_output_tokens == null &&
    usage.total_tokens > input + output && written === 0 && cached <= input) {
    reasoning = usage.total_tokens - input - output;
    estimated = true;
  }
```

- 反推的是 `reasoning_output_tokens`（**token → token**，不是 credits → token），且只在 Gemini 模型、只在缺失 thoughts 字段、只在残差为正时执行。
- 同时会写 `usage_precision = "estimated"`（`trae-usage.js:94`），并让桶级精度变成 `"estimated"`/`"mixed"`（`rollout.js:22144-22158`）。
- 另有「总量无法归因到缓存列」的差额被计入 `total_tokens` 但不进任何可定价列：

```js
// src/lib/trae-usage.js:88-91（节选）
    const unpriced = aggregateInput - input;
    totals.output_tokens += aggregateOutput - output;
    totals.total_tokens += unpriced + aggregateOutput - output;
    if (unpriced > 0) totals.unpriced_input_tokens = unpriced;
```

- 该差额在 CLI 里被显式披露（`sync.js:2212-2214`：`unpricedRecords`）。**这是本项目里唯一把「知道计量但无法拆分」的 token 单独建模的地方**。

### 21.5 `antigravity`：字符估算当 token，且**不写 `usage_precision`**

```js
// src/lib/rollout.js:21971-21983
function estimateAntigravityTokens(text) {
  if (typeof text !== "string" || text.length === 0) return 0;
  let cjk = 0;
  let other = 0;
  for (const ch of text) {
    if (isCjkCodePoint(ch.codePointAt(0))) { cjk += 1; } else { other += 1; }
  }
  return cjk + Math.ceil(other / 4);
}
```

- 兜底路径把**字符估算值当成 `input_tokens` / `output_tokens` / `reasoning_output_tokens` 写入**（`rollout.js:21827-21832`），且 `input` 是「上下文增量」而非真实 prompt。
- **该家只在游标里记 `usageSource: "estimated"`**（`rollout.js:21898`），**没有任何 `usage_precision` 写进队列行**（`rollout.js` 内文本命中 **16 行 / 15 个独立语句**：`rollout.js:3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286-20287/22011/22025/22113/22157`，其中 `20286-20287` 是一条跨行赋值、`22011` 是 `22009`–`22011` 数组字面量的续行；**统计口径**限定在本文件的 `src/lib/rollout.js`，不含 `session-analytics.js` 的 5 行、`trae-usage.js` 1 行、`dashboard/src/lib/sessions-api.ts` 1 行与测试用例。R2 重算：原写「14 个语句」不可复现；R1 已补全 `6725/6731/6755/6760/22113` 五处，否定性结论不变）。因此**下游无法区分 antigravity 的 sqlite 精确行与字符估算行**——这是本任务范围内最明确的披露缺口。

### 21.6 `trae-cn`：`input_token` 为 cache 包含式（原本最容易双计的地方）

```js
// src/lib/rollout.js:22467-22474（节选）
  // `input_token` is cache-INCLUSIVE: verifying real rows against TRAE CN's
  // own credit billing, credits = fresh_input*p + cache_read*(p/4) + output*q
  // fits every row with zero residual only under this reading ...
```

- 减法是正确方向（避免双计），且 `total = rawInput + output` 不重复加缓存（`rollout.js:22492`）。
- 值得注意：**「input 含缓存」这一结论本身是用 credits 账单反推出来的**——即该家的语义判断依赖积分计费模型，而非文档。若 TRAE 改变积分权重，结论需重验（见 §27 存疑）。

### 21.7 明确**不**把非 token 单位当 token 的三家

- `cursor`：CSV 有 `Cost` 列，只用于记录 `record.cost`，**从不写入 `total_cost_usd`，也不参与 token**（`cursor-config.js:498`、归一化函数 `512-538` 完全不含 cost）。
- `command-code`：上游 `costUsd` 被**主动丢弃**并写零成本哨兵（`rollout.js:24055-24058`、`24213`）。
- `qoder` 旧 DB 通道：只有 token 字段，没有 credits 概念（`rollout.js:6148-6179`）。

### 21.8 `grok` 的上游 ticks（美元）

```js
// src/lib/grok-usage.js:12 / 29-35（节选）
const USD_TICKS_PER_USD = 10_000_000_000;
...
  const ticks = Number(usage.costUsdTicks ?? usage.totalCostUsdTicks
    ?? usage.cost_usd_ticks ?? usage.total_cost_usd_ticks);
  if (Number.isFinite(ticks) && ticks >= 0) return ticks / USD_TICKS_PER_USD;
```

- 这是**唯一直接用上游美元成本**的本地 provider，但走的是独立字段 `total_cost_usd`，**不污染 token 列**；并且 `costIsPartial` / `usageIsIncomplete` 为真时返回 `null`（不写假精确的 0）。
- 累计时按 `USD_TICKS_PER_USD` 取整防浮点漂移（`rollout.js:4111-4113`）。

---

## 22. 只填 `total_tokens` 而 input/output 为 0（会导致 $0 成本）

**先给结论**：在 21 家中，**没有任何一家会「只填 total 而不填 input/output」**。所有 21 家的 normalizer 都是「input 与 output 至少有一个被赋值」或「整体判空丢弃」。但存在**三类等价风险**，逐条列源码：

### 22.1 `conversation_count = 1` 且**全部 token 列为 0**（会入队零值行）

`qoder` 新 JSONL 的 billable 分支：

```js
// src/lib/rollout.js:6629-6660（节选）
      // Allow billable zero-token messages to still count conversation (no token delta)
      const isBillable = usage.billable !== false;
      if (!base && !isBillable) continue;
      ...
      } : {
        input_tokens: 0, cached_input_tokens: 0, cache_creation_input_tokens: 0,
        output_tokens: 0, reasoning_output_tokens: 0, total_tokens: 0,
        billable_total_tokens: 0, total_cost_usd: 0, conversation_count: 1,
      };
```

- 这是**「全零 token + conversation_count=1」被显式构造并写进桶的构造点之一**（`rollout.js:6639-6660`）——**但不是唯一一处**（R1 修正：原稿称「唯一」，实为两处）。
- **第二处：`trae-cn`（无任何零值护栏）**。字段校验只要求 `Number.isSafeInteger(value) && value >= 0`——**`0` 合法**：

```js
// src/lib/rollout.js:22483-22492（节选）
  for (const [label, value] of fields) {
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new Error(`Trae CN session row has an invalid ${label}.`);
    }
  }
  ...
  const total = rawInput + outputTokens;
```

  随后 `rollout.js:22500-22508` 写入 `total_tokens: 0`、`billable_total_tokens: 0`、`conversation_count: 1`，并在 `rollout.js:22749-22751` 直接 `addTotals` 进桶（`unchanged` 分支只跳过「未变化」的条目，不筛零值）。
- **反例（原稿误报为第二处）**：`craft` **不会**产生该形态——`rollout.js:17199-17212` 在四分量 delta 全 0 时 `continue`；走到 `17243`（`conversation_count: prev.total === 0 ? 1 : 0`）时至少一个 token 分量 > 0。
- 影响：该桶的 `conversation_count` 会增加，但所有 token 列为 0 → 该会话行的成本贡献为 $0。
- 该行的存在是否最终入队取决于 `enqueueTouchedBuckets` 的去重（`queuedKey` 比较，`rollout.js:3325-3347`），但**桶 totals 确实被改动了**。`trae-cn` 是否会从真实账号拿到全零行见 §26.12（U1 判定条件）。

### 22.2 `reasoning` 被并入/拆出时可能让 output 变 0（合法的「output=0 但 total>0」）

- OmO：`reasoningIncludedInOutput = true`（`rollout.js:16110`）→ 当一次 turn 只有推理 token 时，`output` 可能为 0，而 `reasoning_output_tokens > 0`、`total > 0`。此时**成本仍可算**（reasoning 有独立价格），但如果下游定价表只认 output，就会得到偏低的成本。相关求和排除逻辑：`rollout.js:15899-15903`。
- Copilot 两通道都把 reasoning 从 output 里拆出（`rollout.js:17521-17522`、`18565-18566`）；若某次 turn 的 output 全为推理，则 `output_tokens = 0`。
- Trae CN：`reasoning_output_tokens` 恒 0（`rollout.js:22505`），不存在该形态。

### 22.3 全部 21 家的「整体判空」护栏（证明不存在「只有 total」）

| 家 | 判空条件 | 源码行 |
|----|----------|--------|
| opencode | `normalizeOpencodeTokens` 返回 null；且入库前还要 `hasTokens` 检查 | `rollout.js:4755-4756`、`5222-5227` |
| qoder(DB) | `prompt/completion` 非有限数或为负 → null | `rollout.js:6154-6163` |
| qoder(new) | 四个 token 列全为 0 → null（credits-only 不计） | `rollout.js:6526-6549` |
| cursor | `isAllZeroUsage(delta)` 即跳过 | `rollout.js:7230` |
| devin | `input`/`output` 任一为 NULL → 整行不计 | `devin-usage.js:76-82`、`113-119` |
| omp/omo | 五个分量全 0 → 记 seenIds 后跳过 | `rollout.js:15874-15883` |
| pi/prime/minimax | 五个分量全 0 → 跳过 | `rollout.js:16455-16464` |
| craft | 四个 delta 全 0 → 只更新快照 | `rollout.js:17199-17211` |
| reasonix | `tokenDelta <= 0 && delta.requests <= 0` → 不加桶 | `rollout.js:17067-17068` |
| copilot(otel) | `totalInteresting === 0` → null | `rollout.js:17536-17542` |
| copilot(store) | `normalized.total_tokens <= 0` → 跳过 | `rollout.js:18863-18867` |
| copilot(app) | `totalTokens` 为 0 时 `conversation_count` 也是 0（仍会加桶，但零值不改变总数） | `rollout.js:19369-19377` |
| grok | `normalizeGrokUsage` 在 `totalTokens <= 0` 时返回 null | `grok-usage.js:71` |
| antigravity | `billedPlanner = delta.total_tokens > 0` 才落桶 | `rollout.js:21808`、`21833`、`21837` |
| trae | `normalizeTraeUsage` 返回 null 即跳过；`total_tokens === 0` 且无前值时跳过（**只保护首次观测**，有前值的后续回撤不受此护栏约束） | `trae-usage.js:60-62`、`rollout.js:22112` |
| trae-cn | 字段缺失/非安全整数即抛错（fail-closed）；**`0` 合法、无零值护栏 → 全零行会落桶**（值为 0 合法） | `rollout.js:22477-22487`（校验）、`22749-22751`（落桶） |
| dsh | 五分量全 0 → null | `rollout.js:23354-23356` |
| command-code | `total === 0 && !explicitZero` → null | `rollout.js:24204` |

（R1 修正：本表补入「值为 0 是否合法」视角——`trae` 的零值护栏只管首次观测、`trae-cn` 完全无零值护栏，是仅有的两处零值开放式形态；其余各家的判空条件都会在入桶前处理全零。）

**净结论**：`$0 成本`风险在本子集里的表现不是「只有 total」，而是 **`conversation_count` 与 token 计费脱钩**（§22.1）与**reasoning/output 分离后定价依赖输出列**（§22.2）。

---

## 23. `billable_total_tokens` 覆盖情况（影响「可计费口径」）

**显式写入该字段的 8 家**：

| 家 | 值 | 源码行 |
|----|----|--------|
| `qoder`（旧 DB） | `prompt + output`（含缓存） | `rollout.js:6177` |
| `qoder`（新 JSONL） | `inp + cach + cc + out` | `rollout.js:6540`（零 token 分支写 `0`：`6657`） |
| `devin` | 四分量之和 | `devin-usage.js:197` |
| `cursor` | 四分量之和（注释解释为何不写 0：`cursor-config.js:525-536`） | `cursor-config.js:536` |
| `grok` | `totalTokens`（含估算路径） | `grok-usage.js:83`；`rollout.js:19959` |
| `antigravity` | `delta.total_tokens`（两条路径都写） | `rollout.js:21806`、`21831` |
| `trae-cn` | `rawInput + output` | `rollout.js:22507` |
| `command-code` | 四分量之和 | `rollout.js:24212` |

**不写、依赖 `addTotals` 兜底的 13 项**（按解析项计；若按「家」口径把 `copilot` 三通道合算，则为 11 家）：`opencode`、`omp`、`omo`、`pi`、`prime-agent`、`minimax-code`、`craft`、`reasonix`、`copilot`（OTEL / session-store / app-db 三条通道）、**`trae`**、`dsh`。

逐项自查（R1 重算）：`git grep 'billable_total_tokens' -- src/lib/rollout.js` 在 `trae` 区段（21993-22176）**零命中**（原稿漏列 `trae`）；`opencode`/`omp`/`omo`/`pi`/`prime-agent`/`minimax-code`/`craft`/`reasonix`/`copilot` 三通道/`dsh` 各区段同样无写入——`17485`、`23212`、`23239` 命中的是字段校验/对账列表里的字符串，不是写入。合计 **8 项显式写 + 13 项兜底 = 21 项**，与 §20.1 表 19 行 + §20.2 表 2 行完全对齐（`qoder-cn` 复用 `qoder` 的写入路径，不额外计）。

兜底规则（所有消费侧都遵守，因此多数情况下看不出差异）：

```js
// src/lib/rollout.js:4110
  target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0;
```

```js
// src/lib/local-api.js:2615 / 2628（节选，聚合侧同样兜底）
        sa.totals.billable_total_tokens += row.billable_total_tokens ?? row.total_tokens ?? 0;
```

唯一需要注意的历史坑（注释里写明的迁移）：早期 Cursor 行把 `billable_total_tokens` 写成 0，导致聚合侧把非计费 Cursor 用量整体隐藏，于是有一次性修复与局部重算：

```js
// src/lib/local-api.js:179-182（节选）
  // Legacy Cursor rows from versions ≤ 0.26.5 wrote billable_total_tokens = 0
  ...
  // sums billable_total_tokens across sources, so those rows silently ...
```

---

## 24. 附录：21 家的路径 / 覆盖变量速查

| 解析器 | source | 本地数据位置 | 路径覆盖变量 |
|--------|--------|--------------|--------------|
| `parseOpencodeDbIncremental` | `opencode` | `<dbDir>/opencode.db` | `OPENCODE_HOME`（`sync.js:1348`） |
| `parseQoderDbIncremental` | `qoder` / `qoder-cn` | `…/Qoder/SharedClientCache/cache/db/local.db` | `QODER_DB_PATH`、`QODER_HOME`、`QODER_CN_DB_PATH`、`QODER_CN_HOME`（`rollout.js:6059-6127`） |
| `parseQoderNewIncremental` | `qoder` / `qoder-cn` | `~/.qoder/projects`、`~/.qoder-cn/projects` | `QODER_PROJECTS_DIR`、`QODER_CN_PROJECTS_DIR`（`rollout.js:6417`、`6436`） |
| `parseCursorApiIncremental` | `cursor` | `Cursor/User/globalStorage/state.vscdb`（仅凭据）+ 远程 CSV | 无（`cursor-config.js:10-33` 平台默认） |
| `parseDevinIncremental` | `devin` | `$XDG_DATA_HOME/devin/cli/sessions.db` | `TOKENTRACKER_DEVIN_DB`（`rollout.js:14108`） |
| `parseOmpIncremental` | `omp` | `~/.omp/agent/sessions/**` | `TOKENTRACKER_OMP_AGENT_DIR`、`OMP_HOME`、`PI_CONFIG_DIR`（`rollout.js:11355-11356`、`11395`） |
| `parseOmoIncremental` | `omo` | `~/.omo/agent/sessions/**` | `TOKENTRACKER_OMO_AGENT_DIR`、`TOKENTRACKER_OMO_HOME`（`rollout.js:11582-11596`） |
| `parsePiIncremental` | `pi` / `pi-<provider>` | `~/.pi/agent/sessions/**`、`~/.multica/pi-sessions/*.jsonl` | `TOKENTRACKER_PI_AGENT_DIR`、`PI_CODING_AGENT_DIR`（`rollout.js:16142-16145`） |
| `parsePrimeAgentIncremental` | `prime-agent` / `prime-agent-<provider>` | `~/.prime/agent/sessions/*.jsonl` | `TOKENTRACKER_PRIME_AGENT_HOME`、`TOKENTRACKER_PRIME_AGENT_DIR`（`rollout.js:16236`、`16251`） |
| `parseMinimaxCodeIncremental` | `minimax-code` | `~/.minimax/v2/sessions/**/messages.jsonl` | `TOKENTRACKER_MINIMAX_HOME`（`rollout.js:16728`） |
| `parseCraftIncremental` | `craft` | `~/.craft-agent/**/sessions/<sid>/session.jsonl` + `config.json` | `CRAFT_CONFIG_DIR`（`rollout.js:16831`） |
| `parseReasonixIncremental` | `reasonix` | `~/.reasonix/**`、`%APPDATA%\reasonix/**` 的 `*.jsonl.telemetry.json` | `TOKENTRACKER_REASONIX_HOME`、`REASONIX_STATE_HOME`（`rollout.js:16910-16914`） |
| `parseCopilotIncremental` | `copilot` | `~/.copilot/otel/*.jsonl`、`~/.copilot-otel/*.jsonl` | `COPILOT_OTEL_FILE_EXPORTER_PATH`（`rollout.js:17336`）+ `COPILOT_OTEL_*` 前置条件 |
| `parseCopilotSessionStoreIncremental` | `copilot` | `~/.copilot/session-store.db` | `TOKENTRACKER_COPILOT_SESSION_STORE_DB`、`COPILOT_HOME`（`rollout.js:18146-18153`） |
| `parseCopilotAppDbIncremental` | `copilot` | `~/.copilot/data.db` | `TOKENTRACKER_COPILOT_APP_DB`、`COPILOT_HOME`（`rollout.js:19190-19194`） |
| `parseGrokBuildIncremental` | `grok` | `~/.grok/sessions/<encodedCwd>/<sid>/{updates.jsonl,signals.json,summary.json}` | `TOKENTRACKER_GROK_HOME`、`GROK_HOME`（`rollout.js:19554-19555`） |
| `parseAntigravityIncremental` | `antigravity` | `<geminiHome>/{antigravity,antigravity-ide,antigravity-cli}/brain/**` + `conversations/<uuid>.db` | **无用量侧覆盖变量**；`TOKENTRACKER_ANTIGRAVITY_HOME` 只作用于技能目录（`antigravity-paths.js:24-26`），家目录继承 `geminiPaths`（`sync.js:1281-1295`） |
| `parseTraeIncremental` | `trae` | `<APPDATA>/{Trae,TRAE SOLO}/ModularData/{ai-agent,ai-chat}/database.db` | `TOKENTRACKER_TRAE_DB`、`TOKENTRACKER_TRAE_HOME`（`trae-db.js:16-18`） |
| `parseTraeCnApiIncremental` | `trae-cn` | `%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json`（凭据）+ 远程 API | `TOKENTRACKER_TRAE_CN_HOME`（`trae-cn-config.js:23`） |
| `parseDshIncremental` | `dsh` | `~/.dsh/sessions/<project>/<session>/session[.vN].jsonl[.zstd]` | `TOKENTRACKER_DSH_HOME`、`DSH_HOME`（`rollout.js:22827-22829`） |
| `parseCommandCodeIncremental` | `command-code` | `~/.commandcode/projects/<cwd-slug>/<session>.jsonl` | `TOKENTRACKER_COMMANDCODE_HOME`（`rollout.js:24086`） |

---

## 25. WSL / 多安装（跨设备）处理汇总（本子集相关部分）

| 机制 | 涉及家 | 源码 |
|------|--------|------|
| UNC 路径先快照再读（避免 Plan 9 桥锁失败） | `opencode`、`devin`、`antigravity`、`copilot(store)` | `rollout.js:5194-5199`、`14184-14189`、`21573-21581`、`18681` |
| WSL 家目录探测 + 原生/WSL 双路径解析 | `qoder`、`qoder(new)`、`devin`、`dsh`、`command-code`、`copilot`、`omp`/`omo`/`pi`/`prime`/`minimax`/`craft`、`grok` | `rollout.js:6091-6109`、`6424-6452`、`14120-14125`、`22839-22865`、`24095-24127`；`pickWin32ProviderPath`（`rollout.js:7650-7653`） |
| 多安装游标命名空间隔离 | `opencode`（`native`/`wsl`/`flat`） | `sync.js:1429`、`rollout.js:5788` |
| 路径相同时跳过第二家（防双计） | `omp` vs `omo`、`omp` vs `pi` | `rollout.js:16157-16169`；`sync.js:2735-2741`、`2772-2774` |
| DB 别名路径合并（同物理库多路径） | `copilot(store)`、`copilot(app)` | `rollout.js:18610-18617`、`3153-3157` |
| 会话级跨设备对账（云端 LWW） | `trae-cn` | `rollout.js:22532-22595` |
| 副本/分叉去重（本地） | `opencode`(#426 指纹)、`devin`(request_id 账本)、`trae`(turn 摘要账本)、`command-code`(recordId\|timestamp) | `rollout.js:5847-5888`、`14303-14347`、`22104-22111`、`24542` |

---

## 26. 存疑与待复核点

以下各项**在源码中未能取得决定性证据**，或存在本文未能闭合的推理链，按「需要复核的优先级」排列。

### 26.1 `TOKENTRACKER_ANTIGRAVITY_HOME` 与用量解析无关（已复核，登记为「易误判点」）

复核结论：该变量**只作用于技能目录**，不覆盖用量解析的家目录。

```js
// src/lib/antigravity-paths.js:24-30（节选）
function resolveAntigravitySkillDirs(env = process.env) {
  const override = trimmed(env.TOKENTRACKER_ANTIGRAVITY_HOME);
  if (override) return [path.join(override, "skills")];

  const home = geminiHome(env);
  const mainSkills = path.join(home, MAIN_APP_SUBDIR, "skills");
```

用量侧的家目录来自 `geminiPaths`（`sync.js:1281-1295`），其 native/wsl 两路分别喂给 `listAntigravityTranscriptsWithStatus`。**待复核的剩余项**：`geminiPaths` 自身的解析函数（`resolveGeminiHome` / `resolveGeminiPaths`）未在本次范围内逐行核对，因此「`GEMINI_HOME` 之类的变量能否重定向 Antigravity 用量扫描」仍未确认。

### 26.2 【未能确认】`antigravity` 估算行是否真的对下游不可见
本文的结论（§21.5）建立在「`usage_precision` 的全部写入点清单里没有 antigravity」这一**否定性证据**上。已核对 `rollout.js` 内 **16 行 / 15 个独立语句**的 `usage_precision` 引用（清单与统计口径见 §21.5；R1 已补全 `6725/6731/6755/6760/22113`，原稿「25 处」为误计；R2 重算：原「14 个语句」不可复现）与 `session-analytics.js:1155/1338/1730/2640/2709`、`local-api.js` 的聚合路径，但**未逐一核对云端 ingest 侧是否会从 `cursors.files[].usageSource` 反推**。若云端能读到游标状态，披露缺口可能不成立。**判定条件（U2）**：取得云端 ingest 侧读取 `cursors.files[].usageSource` 的代码或运行证据——能反推则披露缺口不成立，不能则维持现状。

### 26.3 【未能确认】`qoder(new)` 全零 token 行是否真的进入队列
`rollout.js:6639-6660` 构造了 `conversation_count = 1`（token 全 0）的 totals 并 `addTotals` 到桶，但**该桶是否会因 `queuedKey` 未变而不入队**取决于 `enqueueTouchedBuckets` 的 `totalsKey` 比较（`rollout.js:3325-3347`）。本文未构造实际运行验证；需以 fixture 复现「billable 零 token 行」跑一次完整 `parseQoderNewIncremental` + 入队，检查队列行是否出现全零 `total_tokens` 且 `conversation_count = 1`。

### 26.4 【未能确认】`reasonix` 是否存在跨 home 重复计数
`reasonixHomeCandidates` 同时返回 `~/.reasonix` 与 `%APPDATA%\reasonix`（`rollout.js:16925-16934`），若两处都存在且互为拷贝，`resolveReasonixTelemetryFiles` 只是去重**路径**（`16970-16975`），**没有按 inode/内容去重**（对比 Copilot 的 `uniqueCopilotDbPaths` 用 `dev:ino`）。需实测 Windows 上双目录并存时的表现。

### 26.5 【未能确认】`trae` 的 `T_STORE` 冲突裁决在 3 台以上安装时的行为
冲突规则只处理「两份拷贝」（`previous[T_STORE] !== store && modifiedAt <= previous[T_MODIFIED]`，`rollout.js:22111`）。三份以上拷贝轮流被扫描时的最终归属未在源码中给出收敛性证明；注释也只声明「不能让陈旧库回撤更新的用量」。

### 26.6 【未能确认】`grok` 兜底路径的时间归属
兜底路径的桶起点为 `toUtcHalfHourStart(event.timestamp) || toUtcHalfHourStart(lastActive) || toUtcHalfHourStart(Date.now())`（`rollout.js:20249-20252`、`20265`）。当 `lastActive` 也缺失时（`grokLastActiveFromSignals` 最终回落 `new Date().toISOString()`，`rollout.js:19691`），**历史用量会被记进「当前」半小时桶**。本文未验证该组合在真实老会话上的触发频率。

### 26.7 【未能确认】`craft` 与 `reasonix` 的 `total_tokens` 是否会在极端截断下失真
两家都用「累计快照 clamp 差值」。Craft 的注释承认负面差值是截断信号并 clamp 到 0（`rollout.js:17189-17195`）；Reasonix 同样 clamp（`rollout.js:17011`）。**若上游重置后在短时间内又累积回原来的量级，这段「重放」的差值会被再次计入**。两家都没有像 Qoder 那样的「先减旧再加新」回撤路径。需实测截断/重置场景。

### 26.8 【未能确认】`devin` 的 `reasoning_output_tokens` 是否真的不存在
`devin-usage.js:190-200` 恒写 0，而 SQL 投影里**也只选择了四个 metrics 字段**（`devin-usage.js:48-51`）。若 Devin 未来新增 reasoning 列，本解析器会静默丢弃。属设计选择而非 bug，但需登记。

### 26.9 【未能确认】21 家的 `conversation_count` 语义统一性
本文已逐家给出语义（请求级 / 消息级 / 会话级），但**没有核对聚合侧（`local-api.js`、`session-analytics.js`）是否对「会话数」做任何按 source 的归一或标注**。跨 provider 汇总时该指标很可能不可加。

### 26.10 【未能确认】`copilot` 三通道并存的最终一致性证明
三份实现共用 `source = "copilot"`，靠 `usageClaims`（OTEL↔store）、`shouldCountCopilotAppSession`（app-db 跳过 CLI/VS Code）、`usageClaimsComplete` 三重机制分工。源码有大量注释说明设计意图，但**没有看到「三通道同时活跃时互斥完备」的证明或对账测试引用**。R1 实证：`test/copilot-app-parser.test.js` 与 `test/copilot-session-store-parser.test.js` 均命中 `usageClaims`/`observeOnly` 关键字（`git grep -ln`），但**「三通道同时活跃时互斥完备」是否被断言未逐用例阅读**。**判定条件（U2）**：给出这两个测试文件的交叉通道用例断言清单（含「同时活跃」场景的输入与期望输出）；缺该清单则维持「未确认」。

### 26.11 已确认但值得登记的「不变量破坏面」
- `rollout.js:6410-6411` 明确声明 cost 永不估算（qoder）；`rollout.js:24055-24058` 明确丢弃上游 cost（command-code）；`grok-usage.js:26-39` 反之采信上游 cost。**同一条「成本」语义在 21 家之间有三种处理方式**，下游聚合时必须按 source 区分。

### 26.12 【未能确认】`trae-cn` 真实响应中是否出现 `input_token=0` 且 `output_token=0` 的会话行（U1）

源码层「允许」已确认：`rollout.js:22477-22492` 只校验 `Number.isSafeInteger(value) && value >= 0`（`0` 合法），`22500-22508` 会写入 `total_tokens: 0`/`billable_total_tokens: 0`/`conversation_count: 1`，`22749-22751` 无零值护栏直接 `addTotals` 进桶（§22.1 第二处）。

**判定条件**：取得一次真实 `query_user_usage_group_by_session` 响应样本（含全部 token 字段为 0 的会话行），或用同形 fixture 跑完整 `parseTraeCnApiIncremental` 并检查队列是否出现全零 `total_tokens` 且 `conversation_count = 1` 的桶行。缺该样本时，结论停留在「源码允许、真实频率未知」。

---

## 27. 引用索引（本文出现过的关键源码位置）

- SQLite 读取层：`src/lib/sqlite-reader.js:33-195`
- OpenCode SQLite：`src/lib/rollout.js:5041-5147`、`5108-5133`、`5761-6028`
- Qoder 旧库：`src/lib/rollout.js:6030-6050`、`6148-6196`、`6218-6399`
- Qoder 新 JSONL：`src/lib/rollout.js:6401-6414`、`6513-6550`、`6552-6789`
- Cursor：`src/lib/cursor-config.js:46-47`、`150-152`、`451-538`；`src/lib/rollout.js:7164-7271`；`src/commands/sync.js:2144-2195`
- Devin：`src/lib/devin-usage.js:25-58`、`163-206`；`src/lib/rollout.js:14104-14347`
- OMP/OmO：`src/lib/rollout.js:11352-11620`、`15701-16112`
- pi 家族：`src/lib/rollout.js:16129-16317`、`16319-16687`、`16689-16786`
- Craft：`src/lib/rollout.js:16788-16905`、`17080-17298`
- Reasonix：`src/lib/rollout.js:16907-17078`
- Copilot OTEL：`src/lib/rollout.js:17301-17586`、`17878-18117`
- Copilot session-store：`src/lib/rollout.js:18119-18169`、`18407-18926`
- Copilot App DB：`src/lib/rollout.js:19190-19379`、`19381-19539`
- Grok：`src/lib/grok-usage.js:1-95`；`src/lib/rollout.js:19541-20557`
- Antigravity：`src/lib/rollout.js:20559-20649`、`21455-21991`、`20800-21450`
- Trae：`src/lib/trae-db.js:1-127`、`src/lib/trae-snapshot.js:1-230`、`src/lib/trae-sqlite.js:1-143`、`src/lib/trae-usage.js:1-121`、`src/lib/rollout.js:21993-22176`
- Trae CN：`src/lib/trae-cn-config.js:1-548`、`src/lib/rollout.js:22311-22800`
- DSH：`src/lib/rollout.js:22800-24050`
- Command Code：`src/lib/rollout.js:24050-24673`
- 队列与聚合基础：`src/lib/rollout.js:3169-3530`、`3996-4190`、`4240-4264`、`4090-4171`
- 账号级来源：`src/lib/source-metadata.js:1-48`

---

## 修正记录（R1 复核后）

本节登记 R1 复核（`REVIEW-R1-D-providers-db-api.md`，E=9 / I=6 / U=3）后的全部改动。所有计数类结论均已按样本仓库源码重新自查（HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`），未修改样本仓库。**E/I 共 15 条全部采纳（无反驳）；U 组 3 条全部保留并写清判定条件。**

| 编号 | 原表述 | 修正后 | 依据（样本源码行号） |
|---|---|---|---|
| E1 | §0 结论 1「23 家中有 4 家必须联网…其余 19 家」（只列 2 家） | 「**21 家**中有 **2 家必须联网**且读凭据（`cursor`、`trae-cn`）；其余 19 家离线可用」；全文「23 家」统一为「21 家」，口径：21 家 = 23 个 `parse*` 函数名 − 2 个共享实现 | §20.1 表 19 行 + §20.2 表 2 行；`rollout.js:15753`、`16319`；`sync.js:2157-2187`、`2234-2262` |
| E2 | §0 结论 3 / §1.3「仓库不依赖 `sql.js`、`better-sqlite3`」「不存在 JS 内嵌引擎路径」 | 限定为「**生产代码 `src/**` 零命中**」；补登 devDependencies 的 `sql.js@1.14.2`（5 个测试 `require`）与 `@electric-sql/pglite@^0.5.8`（此前完全未提）；§1.3 标题同步改 | `package.json:61`、`67`；`test/sync-trae.test.js:9`、`test/trae-db.test.js:7`、`test/trae-snapshot.test.js:6`、`test/trae-sqlite.test.js:3`、`test/trim-embedded-deps.test.js:8`；`src/**` grep 命中 0 |
| E3+E6 | §0 结论 5 / §22.1「**唯一一处**全零 token + `conversation_count=1`」，并把 `craft` 列为第二处 | 改为**两处**：`qoder` 新 JSONL（`6639-6660`）+ `trae-cn`（`22483-22492` 只校验 ≥0、`22500-22508` 写全零 totals、`22749-22751` 无护栏落桶）；`craft` 改为**反例**（`17199-17212` 四分量全零 `continue`） | `rollout.js:17199-17212`、`17243`、`22483-22492`、`22500-22508`、`22749-22751` |
| E4 | §17.1「落在桶键 `rollout.js:22680` 附近」 | 「真实落桶点 `22749-22751`；`22680` 是 fail-closed 抛错处，不是落桶点」 | `rollout.js:22680`（throw）、`22738-22746`、`22749-22751` |
| E5 | §8.2「pi：…（`rollout.js:17171-17223`）」（实际落在 craft 函数体内） | 改为 `rollout.js:16171-16223`（`resolvePiSessionFiles`） | `rollout.js:16171`、`17080`（`parseCraftIncremental` 起点） |
| E7 | §23「不写 `billable_total_tokens` 的 11 家」（实为 10 家/12 项，且漏 `trae`） | 「不写 **13 项**（含 `trae`）；按「家」口径（`copilot` 三通道合算）11 家」+ 逐项自查；显式写 8 项不变，8 + 13 = 21 项与 §20.1/§20.2 对齐 | `git grep 'billable_total_tokens'`：trae 区段（21993-22176）零命中；`17485`/`23212`/`23239` 为校验列表中的字符串 |
| E8 | §21.5/§26.2「已核对 **25 处** `usage_precision` 引用」，清单仅 10 处 | 「**16 行 / 15 个独立语句**」（R2 重算：原写「14 个语句」不可复现；口径＝`src/lib/rollout.js` 文本命中，`20286-20287` 跨行单语句、`22011` 为数组续行），清单补 `6725/6731/6755/6760/22113`；否定性结论（无 antigravity 写入点）不变 | `rollout.js:3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286-20287/22011/22025/22113/22157` |
| E9 | §0 结论 2「真正走 SQLite 的有 **8 家**」（只列 7 名） | 统一为「SQLite 数据源 **7 项**（`copilot` 拆两份）；`cursor` 另有 SQLite 仅取凭据、不计入数据源类型」；§20.4 表头「家数」→「项数」并补口径注 | §20.4 表成员；`cursor-config.js:46-47` |
| I1 | §21.1 未登记 `rollout.js:6726` 悬空注释 | 增列：注释称 credits 会累积，但下一行即 `touchedBuckets.add`，上下文无实现；下游须按 totals 白名单消费 | `rollout.js:6726-6727` |
| I2 | 「23 家」计数口径未定义 | 在 §0 首条与 §1 标题定义：21 家（独立数据源实体）与 23 个 `parse*` 函数名（含 2 个共享实现）的关系 | `rollout.js:15753`、`16319` |
| I3 | §1.3「不存在 JS 内嵌引擎路径」措辞过强 | 与 E2 合并限定为「生产路径」；保留 Trae 的 wa-sqlite WASM 例外声明 | `trae-sqlite.js:7-11` |
| I4 | §20.2 `readTraeEntitlementFromStorage` 行号 `22236-22259`（指向相邻函数） | 改为定义 `rollout.js:22269`（`22236-22260` 是 `normalizeTraeEntitlement`）；唯一调用点 `status.js:840`，不进解析路径 | `rollout.js:22269`；`status.js:840` |
| I5 | §10.8 `17008-17013`、`17067-17069`；§7.6 `11612-11616` | 分别改为 `17008-17014`、`17067-17068`、`11612-11615` | `rollout.js:17014`、`17068-17069`、`11615` |
| I6 | §22.3 表未覆盖 `trae`/`trae-cn` 的零值形态 | 表内补「值为 0 是否合法」：`trae` 只保护首次观测、`trae-cn` 无零值护栏；表后加说明 | `rollout.js:22112`、`22483-22492`、`22749-22751` |
| U1 | 原稿未登记（`trae-cn` 真实全零行是否出现） | 新增 §26.12，判定条件：真实 `query_user_usage_group_by_session` 样本，或同形 fixture 跑完整解析检查桶行 | `rollout.js:22477-22492`、`22500-22508`、`22749-22751` |
| U2 | §26.2 / §26.10 无明确判定条件 | 写清判定条件：云端 ingest 是否反推 `usageSource`；两测试文件的交叉通道断言清单（已实证 `test/copilot-app-parser.test.js`、`test/copilot-session-store-parser.test.js` 命中 `usageClaims`/`observeOnly`） | `git grep -ln 'usageClaims\|observeOnly' -- test/` |
| U3 | §26.1 | 保留原样（`geminiPaths` 行级核对未做），判定条件已写明 | `antigravity-paths.js:24-30`；`sync.js:1281-1295` |

**未落实项**：无。I2、I3 分别并入 E1、E2 条目落实；原稿其余内容（含全部未被 R1 证伪的代码块引用）未删改。

### R2 修正

依据：`REVIEW-R2-2-providers.md`（E=10 / I=9 / U=4；本片涉及 E9、I3–I6、I8、I9）。**采纳 7 条，反驳 0 条**；该报告的 E1–E8 全部落在 S3a、E10 落在 S1、I1/I2 落在 S3a、I7 落在 S4，不属本片职责，未改。行号口径统一为 `read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；每条均在本轮开过对应行才落笔。

| 编号 | 原表述 | 修正后 | 依据行号（已开行核对） |
|---|---|---|---|
| E9 | §8 第 7 点：「MiniMax Code 的记录包装器把 `timestamp` 恒置 null（`16771`），因此**只能靠 entry 级 ISO 串**」 | **语义反转修正**：包装器 `readMinimaxCodeEntry` 返回 `timestamp: null` ⇒ `record.timestamp` 恒为 `null`，ISO 回落分支 **永不触发**；唯一有效路径是 `message.timestamp` 数值毫秒（`16467`），缺失即整条丢弃（`16473-16476`）。fixture 印证（顶层仅 `message_id`/`turn_id`/`message`，`timestamp` 在 `message` 内） | `rollout.js:16467`、`16469`、`16473-16476`、`16766-16773`；`test/minimax-code-parser.test.js:37-64` |
| I3 | §21.5 / §26.2 / 修正记录 E8：「16 行 / **14 个语句**」 | 「16 行 / **15 个独立语句**」＋统计口径（`src/lib/rollout.js` 文本命中按独立语句计：`20286-20287` 是一条跨行赋值、`22011` 是 `22009`–`22011` 数组续行；不含 `session-analytics.js` 5 行、`trae-usage.js` 1 行、`dashboard/src/lib/sessions-api.ts` 1 行与测试用例）。否定性结论不变 | 16 行清单同 §21.5：`3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286-20287/22011/22025/22113/22157` |
| I4 | §12 第 9 点「见 §12 消费侧」（在 §12 内自指） | 改「见**本节（§12）** delta 生成处」，并注明 `18886` 位于 `parseCopilotSessionStoreIncremental`（`18579` 起）内 | `rollout.js:18886`、`18579` |
| I5 | §13 第 8 点「库级指纹未变即跳过（`19438-19439`）」 | `19439`（`19438` 是空白行） | `rollout.js:19439` |
| I6 | §15「`billedPlanner` 判定：`21788`、`21808`、`21833`」 | 声明在 `21787`（`let billedPlanner = false;`），赋值/判定在 `21808`、`21833`（`21788` 是空白行） | `rollout.js:21787`、`21808`、`21833` |
| I8 | §7 全节未登记 OMP 的 hook 状态 | §7 第 9 点补：OMP 是本文 21 家中唯一带 extension hook 主动上报的一家（`omp-hook.js:16/48/66/117/135/139/143`；安装点 `init.js:806` `upsertOmpHook`）；OmO 纯被动；`grok` 的 SessionEnd hook 已在 S1 §2.4 | `src/lib/omp-hook.js:16`、`48`、`66`、`117`、`135`、`139`、`143`；`init.js:806` |
| I9 | 两片均未给「去重后唯一 provider 实体数」 | §20.4 口径注补：与 S3a（19 家）两片名单的**唯一重叠是 `opencode`**（JSONL 支路 vs `opencode.db` 支路），去重后唯一实体数 = 19 + 21 − 1 = **39**；不要把 19 与 21 直接相加 | §20.4；§2；S3a §3.2 |

**未落实项**：无 E/I 项遗留。U 组维持 R1 判定条件（U4：云端 ingest 是否从 `cursors.files[].usageSource` 反推、`geminiPaths` 逐行核对——本轮未取得新证据）。跨分片的 E10（S1）与 I7（S4）不在本片职责内，未改。
