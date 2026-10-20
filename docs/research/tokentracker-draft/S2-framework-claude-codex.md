# S2 · 解析框架 + Claude Code + Codex 两条主线（TokenTracker v1.3.14 源码研究）

> 样本：`C:\Users\admin\AppData\Local\Temp\tt-research`（`https://github.com/xiufengsun/TokenTracker`）
> HEAD：`b19d27b0ccd59ce621f4fa17207b06358e67987c`（tag `v1.3.14`，`git status --porcelain` 为空，工作树干净）
> 研究方式：只读（`read`/`grep`/`glob`/`Get-Item`），未修改、未执行仓库内任何脚本。
> 全文行号均相对仓库根，格式为 `相对路径:行号`。

---

## 0. 摘要：本次研究最重的十条事实

1. `src/lib/rollout.js` 共 **24932 行 / 942266 字节**，是 45 个 `parse*Incremental` 的唯一真源；`module.exports` 单一块从 `src/lib/rollout.js:24675` 起。
2. 45 个 parser **没有统一的返回契约**——初稿的「三族签名」不成立。实测：45 个 `parse*Incremental` 里 **5 个是转发薄壳**（没有自己的 return 字面量），其余 **40 个的顶层 `return` 去重后共 11 种键集合**：`{recordsProcessed, eventsAggregated, bucketsQueued}`（19 个）、`{filesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued}`（6 个）、`{recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued}`（5 个）、`{messagesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued}`（3 个），再加 7 个各自独立的扩展签名（Copilot 的 `usageClaims`、CopilotSessionStore 的 9 键、CopilotAppDb 的 `dbErrors`、Trae 的 `recordsSkipped/estimatedRecords/unpricedRecords/errors`、TraeCnApi 的 `skippedRows`、Dsh 的 `deferredMigrations`、CommandCode 的 7 键）。**连"族"内键数都不一致**，唯一 40/40 都有的键只有 `eventsAggregated` 与 `bucketsQueued`，而计数键的名字在 `filesProcessed` / `messagesProcessed` / `recordsProcessed` 三种之间跳。详见 A.1.3。
3. 真正的「统一」在**游标-桶-队列**三层：所有 parser 都写同一个 `cursors.hourly.buckets`（键 `source|model|hourStart`），再经 `enqueueTouchedBuckets()` 追加成**整行快照**，消费端按 `source|model|hour_start` **取最后一条**（不是求和）。
4. 半小时桶函数是 `toUtcHalfHourStart()`（`src/lib/rollout.js:4173`），**纯 UTC 取整**，`minutes >= 30 ? 30 : 0`，秒毫秒清零，输出 ISO 字符串；本地日归属发生在读取侧（`rowDayKey`），不在写入侧。
5. Codex 的真相事件是 `event_msg/payload.type == "token_count"`，数据在 `payload.info.last_token_usage` 与 `payload.info.total_token_usage`；`turn_context` 是**唯一权威的模型归属信号**，`session_meta` 只是防御性回退（`codex-model-attribution.js:5-23`）。
6. Codex 的 `input_tokens` **包含** `cached_input_tokens`，`normalizeUsage()` 会减掉（`src/lib/rollout.js:4864-4877`）；**reasoning 与 output 两侧都不冲突，但两条主线的列分布有意相反**：Codex 的 `reasoning_output_tokens` 是 `output_tokens` 的**子集标记**——解析侧不拆（`normalizeUsage` 只做 `Math.floor`，对 reasoning 无任何算术改写，`:4851-4879`），计价侧**置 0**（`src/lib/pricing/index.js:248-256` 的 `reasoningIncludedInOutput` 白名单含 `codex`）；Claude 的 reasoning 是**独立加成项**——解析侧**显式拆出**（`output_tokens: outputTokens - reasoningTokens`、`reasoning_output_tokens: reasoningTokens`，`src/lib/rollout.js:4930-4939`），计价侧**照 output 价计费**（同一白名单不含 `claude`）。两侧在 `computeRowCost` 的输入都是**五列之和**，`total_tokens` **永不参与计费**。CLAUDE.md 第 60-61 行的"fold them into output_tokens for cost"指的正是这个上游子集语义，不是本工具的重写。
7. Claude 侧的去重键是 `message.id`（可选拼 `requestId`），**不再要求两者同时存在**（`src/lib/rollout.js:4895-4919`）；fork 会话用 `uuid → forkedFrom.messageUuid` 折叠成规范身份（`src/lib/claude-user-identity.js:13-25`）。
8. `const endOffset = st.size;` 在 `rollout.js` 共 **3 处**（`:2291`、`:2682`、`:2793`），**只有 `:2793` 落到 token 游标**：前两处是 Codex 侧，`:2291` 是 `parseRolloutFile` 的"无新字节"早退（数据没读就返回，返回的 `endOffset` 不可能大于入参 `startOffset`），`:2682` 是 `scanRolloutProjectFileContexts` → 返回 `{ endOffset: committedEndOffset, … }`（`:2765`）。**关键非对称点**：Claude 的 `parseClaudeFile`（`:2776`）把 `st.size` 直接写进 `cursors.files[key].offset`（`:1194`）且无行边界保护；Codex 的 token 路径用 `committedEndOffset`（`:2652`）只提交到最后一个完整行，**连项目上下文扫描位都做了同样的提交保护**（`:2765`），OpenClaw 另有显式 `openclawOffsetEndsAtLineBoundary()`（`:1949`）。
9. `src/lib/claude-config.js` **不是项目归属模块**（任务书假设有误），它是 Claude Code `settings.json` 的 hook 增删（`Stop`/`SessionEnd`）。项目归属在 `rollout.js` 的 `resolveClaudeFileCwd()` + `resolveProjectMetaForPath()`。
10. `codex-rollout-parser.js` / `codex-context-breakdown.js` **不写队列、不影响 token 统计**（全库对 `queuePath`/`appendFile` 的检索为空）；它们只服务 session-analytics 与 Context Breakdown 视图。唯一例外：`src/commands/sync.js:198` 借用其 `extractTokenCount` 做一次性 codex 血缘修复。

---

## A. rollout.js 的通用解析骨架

### A.1 模块导出与 45 个 parse 的统一契约

#### A.1.1 `module.exports` 原文（分段）

导出块从 `src/lib/rollout.js:24675` 开始，首个导出即 `parseTraeIncremental`，说明**不是按 provider 顺序排列**，而是历史堆积顺序。逐字引用开头：

```js
// src/lib/rollout.js:24675
module.exports = {
  parseTraeIncremental,
  listRolloutFiles,
  listRolloutFilesDeep,
  codexSessionIdFromPath,
  filterColdCodexRolloutFiles,
  listClaudeProjectFiles,
  listGeminiSessionFiles,
  listOpencodeMessageFiles,
  readOpencodeDbMessages,
  readOpencodeDbMessagesIncremental,
  readMimoDbMessages,
  readZcodeDbMessages,
  coerceEpochMs,
  normalizeOpencodeModelFields,
  hasZcodeNativeUsageSchema,
```

结尾处（`src/lib/rollout.js:24855` 之后）是注释分组的追加导出，包含 4 组显式"为测试而导出"的键。逐字引用收尾段：

```js
// src/lib/rollout.js（尾部注释分组，节选）
  // Exposed for regression tests covering cache-token accounting.
  normalizeGeminiTokens, normalizeOpencodeTokens, normalizeQoderTokens, normalizeQoderNewTokens,
  sameGeminiTotals, diffGeminiTotals,
  // Exposed so the queue-repair migration can mutate cursors state in the
  // same key format sync uses elsewhere.
  bucketKey, toUtcHalfHourStart, totalsKey, claudeMessageDedupKey, normalizeClaudeUsage, groupBucketKey,
```

结论：`module.exports` 是**扁平名字袋**（无命名空间、无类型），`bucketKey` / `toUtcHalfHourStart` / `totalsKey` 三个内部函数被显式导出给修复迁移与测试复用（`src/lib/rollout.js` 尾部注释分组）。

#### A.1.2 45 个 `parse*Incremental` 的完整签名表

用 `Select-String '^async function parse[A-Za-z0-9]*Incremental\('` 精确枚举，共 **45 个**，与情报一致。参数按 `{ ... } = {}` 解构，参数名即契约：

| 行号 | 函数 | 解构参数 |
|---|---|---|
| 251 | `parseRolloutIncremental` | `rolloutFiles, cursors, codexEventStore, queuePath, projectQueuePath, onProgress, source, publicRepoResolver, diagnostics, invalidRecordPolicy` |
| 954 | `parseClaudeIncremental` | `projectFiles, cursors, queuePath, projectQueuePath, onProgress, source, publicRepoResolver` |
| 1242 | `parseGeminiIncremental` | `sessionFiles, cursors, queuePath, projectQueuePath, onProgress, source, publicRepoResolver` |
| 1412 | `parseOpencodeIncremental` | `messageFiles, cursors, queuePath, projectQueuePath, onProgress, source, publicRepoResolver` |
| 1654 | `parseOpenclawIncremental` | `sessionFiles, cursors, queuePath, projectQueuePath, onProgress, source` |
| 5761 | `parseOpencodeDbIncremental` | `dbMessages, dbPath, dbCursor, cursors, queuePath, projectQueuePath, onProgress, source, cursorKey, publicRepoResolver, opencodeCursorStore, opencodeCursorNamespace` |
| 6218 | `parseQoderDbIncremental` | `dbMessages, dbPath, cursors, queuePath, projectQueuePath, onProgress, publicRepoResolver, sourceKey, cursorKey` |
| 6552 | `parseQoderNewIncremental` | `sessionFiles, cursors, queuePath, projectQueuePath, onProgress, sourceKey, cursorKey, publicRepoResolver` |
| 7051 | `parseClaudeScienceIncremental` | `dbRows, cursors, queuePath, onProgress`（+ 内部第 5 个 `hourlyState`） |
| 7172 | `parseCursorApiIncremental` | `records, cursors, queuePath, onProgress, source` |
| 7472 | `parseKiroIncremental` | `basePath, dbPath, jsonlPath, cursors, queuePath, onProgress, sqliteOptions` |
| 7900 | `parseHermesIncremental` | `hermesPath, dbPath, cursors, queuePath, onProgress, sqliteOptions` |
| 8886 | `parseKiroCliIncremental` | `sessionFiles, cursors, queuePath, onProgress, env, sqliteOptions` |
| 9528 | `parseKimiIncremental` | `wireFiles, cursors, queuePath, onProgress, env, model` |
| 9723 | `parseKimiCodeIncremental` | `wireFiles, cursors, queuePath, onProgress, env, model` |
| 10164 | `parseCodebuddyIncremental` | `projectFiles, cursors, queuePath, onProgress, env, defaultModel, fileOffsets` |
| 10803 | `parseWorkbuddyIncremental` | `projectFiles, cursors, queuePath, onProgress, env, defaultModel, source, fileOffsets` |
| 11824 | `parseRoocodeIncremental` | `taskFiles, cursors, queuePath, onProgress, env, fileOffsets` |
| 12200 | `parseClineIncremental` | `sessionFiles, scanCompleteRoots, cursors, queuePath, onProgress, env` |
| 12650 | `parseZedIncremental` | `dbPath, cursors, queuePath, onProgress, env, sqliteOptions` |
| 13267 | `parseLmstudioIncremental` | `logFiles, cursors, queuePath, onProgress, messageLimit, readerOptions` |
| 13691 | `parseUnslothIncremental` | `dbPath, cursors, queuePath, onProgress, env, sqliteOptions`（+ 内部续参） |
| 13938 | `parseAnythingllmIncremental` | `dbPath, cursors, queuePath, onProgress, env, sqliteOptions`（+ 内部续参） |
| 14207 | `parseDevinIncremental` | `dbPath, cursors, queuePath, projectQueuePath, onProgress, env, sqliteOptions, publicRepoResolver` |
| 14604 | `parseGooseIncremental` | `dbPath, cursors, queuePath, onProgress, env, sqliteOptions` |
| 15137 | `parseDroidIncremental` | `settingsFiles, cursors, queuePath, projectQueuePath, publicRepoResolver, onProgress, env`（+ 续参） |
| 15545 | `parseKilocodeIncremental` | `taskFiles, cursors, queuePath, onProgress, env, fileOffsets` |
| 15753 | `parseOmpLikeIncremental` | `sessionFiles, subagentFiles, cursors, queuePath, projectQueuePath, publicRepoResolver, onProgress, env, defaultModel, stateKey, source, resolveSessionFiles, resolveSubagentFiles, resolveDefaultModel, resolveFileCwd, reasoningFields, reasoningIncludedInOutput, maxRecordBytes, fileOffsets` |
| 16087 | `parseOmpIncremental` | `options = {}`（薄壳，转发给 `parseOmpLikeIncremental`） |
| 16100 | `parseOmoIncremental` | `options = {}`（薄壳） |
| 16319 | `parsePiLikeIncremental` | `sessionFiles, cursors, queuePath, projectQueuePath, publicRepoResolver, onProgress, env, defaultModel, stateKey, resolveSessionFiles, resolveDefaultModel, sourceForProvider, readEntry, fileOffsets` |
| 16689 | `parsePiIncremental` | `options = {}`（薄壳） |
| 16699 | `parsePrimeAgentIncremental` | `options = {}`（薄壳） |
| 16775 | `parseMinimaxCodeIncremental` | `options = {}`（薄壳） |
| 17050 | `parseReasonixIncremental` | `telemetryFiles, cursors, queuePath, onProgress, env` |
| 17080 | `parseCraftIncremental` | `sessionFiles, cursors, queuePath, onProgress, env, defaultModel`（+ 续参） |
| 17878 | `parseCopilotIncremental` | `otelPaths, cursors, queuePath, onProgress, env`（+ 续参） |
| 18579 | `parseCopilotSessionStoreIncremental` | `dbPath, dbPaths, cursors, queuePath, onProgress, env, sqliteOptions`（+ 续参） |
| 19381 | `parseCopilotAppDbIncremental` | `dbPath, dbPaths, cursors, queuePath, onProgress, env, sqliteOptions`（+ 续参） |
| 20084 | `parseGrokBuildIncremental` | `sessions, cursors, queuePath, projectQueuePath, publicRepoResolver, onProgress` |
| 20800 | `parseAntigravityIncremental` | `sessionFiles, cursors, queuePath, projectQueuePath, onProgress, source, publicRepoResolver, inventoryComplete` |
| 22033 | `parseTraeIncremental` | `dbPaths, cursors, queuePath, env, onProgress, readUsageRows` |
| 22598 | `parseTraeCnApiIncremental` | `sessions, cursors, queuePath, onProgress, windowStartMs, windowEndMs, snapshotVerifiedAtMs` |
| 23678 | `parseDshIncremental` | `sessionFiles, cursors, queuePath, onProgress` |
| 24458 | `parseCommandCodeIncremental` | `sessionFiles, cursors, queuePath, projectQueuePath, onProgress, publicRepoResolver`（+ 续参） |

（薄壳证据逐字：`src/lib/rollout.js:16087-16098`）

```js
// src/lib/rollout.js:16087
async function parseOmpIncremental(options = {}) {
  return parseOmpLikeIncremental({
    ...options,
    stateKey: "omp",
    source: "omp",
    resolveSessionFiles: resolveOmpSessionFiles,
    resolveSubagentFiles: resolveOmpSubagentFiles,
    resolveDefaultModel: resolveOmpDefaultModel,
    resolveFileCwd: resolveOmpFileCwd,
    reasoningFields: ["reasoningTokens"],
  });
}
```

结论：所谓"统一签名"是**约定俗成**——`cursors` / `queuePath` / `onProgress` 是恒定三件套；`projectQueuePath` + `publicRepoResolver` 是"项目归属能力"的开关；`source` 只在会被多种 agent 复用的 parser 上出现。

#### A.1.3 返回值契约（**无统一契约**：40 个自持 return、11 种键集合）

初稿写「三族签名」——**不成立**。对 45 个 `parse*Incremental` 的顶层 `return { … }` 逐个提取键名后的实测结果：

- **5 个是转发薄壳**，没有自己的返回字面量（直接返回被转发函数的对象）：`parseOmpIncremental:16087`、`parseOmoIncremental:16100`、`parsePiIncremental:16689`、`parsePrimeAgentIncremental:16699`、`parseMinimaxCodeIncremental:16775`；
- **其余 40 个的顶层 return 字面量去重后共 11 种键集合**；
- **40/40 都有的键只有两个**：`eventsAggregated` 与 `bucketsQueued`。计数键的名字在三种之间跳——`recordsProcessed`（31 个）、`filesProcessed`（6 个）、`messagesProcessed`（3 个），所以连"族"的划分本身都不稳定；
- 部分 parser 另有**早退 return**，其键集合与顶层不同（见下文"早退分支"）。

| # | 键集合 | 个数 | 函数（括号内为 return 所在行） |
|---|---|---|---|
| 1 | `{recordsProcessed, eventsAggregated, bucketsQueued}` | 19 | `parseClaudeScienceIncremental`(7137)、`parseCursorApiIncremental`(7270)、`parseKiroIncremental`(7597)、`parseHermesIncremental`(8069)、`parseKiroCliIncremental`(9268)、`parseKimiIncremental`(9643)、`parseKimiCodeIncremental`(9875)、`parseCodebuddyIncremental`(10587)、`parseWorkbuddyIncremental`(11314)、`parseRoocodeIncremental`(11968)、`parseClineIncremental`(12449)、`parseZedIncremental`(12839)、`parseLmstudioIncremental`(13455)、`parseUnslothIncremental`(13788)、`parseAnythingllmIncremental`(14072)、`parseGooseIncremental`(14767)、`parseKilocodeIncremental`(15681)、`parseReasonixIncremental`(17077)、`parseCraftIncremental`(17297) |
| 2 | `{filesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued}` | 6 | `parseRolloutIncremental`(705)、`parseClaudeIncremental`(1239)、`parseGeminiIncremental`(1409)、`parseOpencodeIncremental`(1562)、`parseOpenclawIncremental`(1922)、`parseAntigravityIncremental`(21394) |
| 3 | `{recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued}` | 5 | `parseDevinIncremental`(14456)、`parseDroidIncremental`(15542)、`parseOmpLikeIncremental`(16084)、`parsePiLikeIncremental`(16686)、`parseGrokBuildIncremental`(20551) |
| 4 | `{messagesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued}` | 3 | `parseOpencodeDbIncremental`(6027)、`parseQoderDbIncremental`(6398)、`parseQoderNewIncremental`(6788) |
| 5 | 骨架 1 + `usageClaims` | 1 | `parseCopilotIncremental`(18111) |
| 6 | 骨架 1 + `dbErrors` | 1 | `parseCopilotAppDbIncremental`(19538) |
| 7 | 骨架 1 + `skippedRows` | 1 | `parseTraeCnApiIncremental`(22791) |
| 8 | 骨架 1 + `deferredMigrations` | 1 | `parseDshIncremental`(24029) |
| 9 | 骨架 1 + `recordsSkipped` / `estimatedRecords` / `unpricedRecords` / `errors`（7 键） | 1 | `parseTraeIncremental`(22175) |
| 10 | 骨架 1 + `messagesProcessed` / `projectBucketsQueued` / `schemaMigrated` / `cursorUnchanged`（7 键） | 1 | `parseCommandCodeIncremental`(24664) |
| 11 | `active` / `healthy` / `adoptedThisRun` / `canonicalDbPaths` + `dbErrors` + `usageClaims`（9 键） | 1 | `parseCopilotSessionStoreIncremental`(19003) |

逐字证据（4 种骨架）：

```js
// src/lib/rollout.js:1239  骨架 A（filesProcessed 族）
return { filesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };
```

```js
// src/lib/rollout.js:6027  骨架 B（messagesProcessed 族）
return { messagesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };
```

```js
// src/lib/rollout.js:7137  骨架 C（recordsProcessed 三键族）
return { recordsProcessed, eventsAggregated, bucketsQueued };
```

```js
// src/lib/rollout.js:16084  骨架 D（recordsProcessed + projectBucketsQueued 族）
return { recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };
```

加宽签名的逐字证据（3 例）：

```js
// src/lib/rollout.js:18111  Copilot
  return {
    recordsProcessed,
    eventsAggregated,
    bucketsQueued,
    usageClaims: recentOtelUsageEvents,
  };
```

```js
// src/lib/rollout.js:22175  Trae（共 7 键）
  return { recordsProcessed, eventsAggregated, bucketsQueued, recordsSkipped, estimatedRecords, unpricedRecords, errors };
```

```js
// src/lib/rollout.js:19003  Copilot Session Store（共 9 键）
  return {
    active: canonicalActive,
    healthy: allCurrentPathsHealthy,
    adoptedThisRun,
    canonicalDbPaths,
    recordsProcessed,
    eventsAggregated,
    bucketsQueued,
    dbErrors,
    usageClaims: recentEvents,
  };
```

**三个"假签名"陷阱**（把**值**当键名来读，会数出不存在的键）：

- `parseCursorApiIncremental:7270` 实际是 `return { recordsProcessed: total, eventsAggregated, bucketsQueued }`——`total` 是 `recordsProcessed` 的**值**，不是第 4 个键；它属于骨架 C（上表 19 个之一）；
- `parseTraeCnApiIncremental:22791` 同理（`recordsProcessed: total`），它的**真**扩展键是 `skippedRows`；
- `parseGrokBuildIncremental:20551` 是 `recordsProcessed: eventsAggregated`（上游疑似把"记录数"写成了"事件数"）——**键并不重复**，但该绑定的语义可疑，引用时不要写成"同址重复键"。

**早退分支会让同一函数出现两种键集合**（顶层之外的第二处 `return`，往往在嵌套块内）：

- `parseTraeCnApiIncremental:22634` 早退 `{ recordsProcessed: 0, eventsAggregated: 0, bucketsQueued: 0 }`——**3 键，没有 `skippedRows`**；
- `parseCopilotIncremental:17944`、`:17963` 早退（4 键，值全 0，`usageClaims: recentOtelUsageEvents`）；
- `parseTraeIncremental:22163` 早退（7 键齐全，`bucketsQueued: 0`）。

调用方若按"顶层键集合"统一解构早退值，会拿到 `undefined`。

**Command Code 的加宽签名**（初稿称"族 3"，此处仅作逐字证据，`src/lib/rollout.js:24664-24672`）：

```js
// src/lib/rollout.js:24664
  return {
    recordsProcessed,
    messagesProcessed: currentByKey.size,
    eventsAggregated,
    bucketsQueued,
    projectBucketsQueued,
    schemaMigrated,
    cursorUnchanged,
  };
```

`schemaMigrated` / `cursorUnchanged` 在全库只出现在这一处（`grep` 命中 `src/lib/rollout.js:24670-24671`），即**只有 Command Code 会向调用方反馈"游标是否无需重写"**；其余 44 个 parser 一律无条件重写 `cursors.hourly`（`src/lib/rollout.js:692-697`、`:1227-1232`）。

**关键点**：所有 parser 的返回值都只是**统计口径的进度计数**，不是用量数据本身。用量数据的唯一出口是 `queuePath` 文件。`onProgress` 回调统一形如：

```js
// src/lib/rollout.js:671-679（Codex 与 Claude 完全一致）
      cb({
        index: idx + 1,
        total: totalFiles,
        filePath,
        filesProcessed,
        eventsAggregated,
        bucketsQueued: touchedBuckets.size,
      });
```

---

### A.2 文件发现、增量读取、行解析 helper

#### A.2.1 并发与目录枚举

| 函数 | 行号 | 语义 |
|---|---|---|
| `mapConcurrent(items, concurrency, mapper)` | `src/lib/rollout.js:72` | 手写有界并发；`FILE_METADATA_CONCURRENCY = 32`（`:70`）；顺序稳定（结果按 index 落位） |
| `safeReadDir(dir)` | `src/lib/rollout.js:5005` | `fs.readdir(withFileTypes)` 包 try/catch，**读失败返回 `[]`**（不抛） |
| `listRolloutFiles(sessionsDir, options)` | `src/lib/rollout.js:91` | 严格 `YYYY/MM/DD/rollout-*.jsonl`；三层目录扫描；结果 `localeCompare` 排序 |
| `listRolloutDayFiles(dayDir, {dayInventoryCache, stats})` | `src/lib/rollout.js:143` | 以目录 statKey 命中缓存；未命中才 readdir |
| `isRolloutFileName(name)` | `src/lib/rollout.js:179` | 前缀 `rollout-` + 后缀 `.jsonl`，且必须等于 `path.basename(name)`（防注入路径） |
| `directoryInventoryStatKey(st)` | `src/lib/rollout.js:186` | `ino:size:mtimeMs:ctimeMs` 四元组字符串 |
| `listRolloutFilesDeep(dir)` | `src/lib/rollout.js:201` | 任意深度递归，供 `archived_sessions` 这种扁平归档 |
| `listClaudeProjectFiles(projectsDir)` | `src/lib/rollout.js:219` | 递归 → `walkClaudeProjects` |
| `walkClaudeProjects(dir, out)` | `src/lib/rollout.js:5013` | 任意深度；**只认 `endsWith(".jsonl")`**，不解析 slug |
| `claudeFileIdentityHash(filePath, st)` | `src/lib/rollout.js:937` | 前 64KB（`CLAUDE_FILE_ID_PREFIX_BYTES`，`:936`）sha256 + 文件大小 → `${hash}:${size}`，用于跨环境去重 |

`directoryInventoryStatKey` 与 `isRolloutFileName` 逐字：

```js
// src/lib/rollout.js:179
function isRolloutFileName(name) {
  if (typeof name !== "string") return false;
  if (name.includes("/") || name.includes("\\")) return false;
  if (name !== path.basename(name)) return false;
  return name.startsWith("rollout-") && name.endsWith(".jsonl");
}
```

```js
// src/lib/rollout.js:186
function directoryInventoryStatKey(st) {
  return [
    Number(st.ino || 0),
    Number(st.size || 0),
    Number(st.mtimeMs || 0),
    Number(st.ctimeMs || 0),
  ].join(":");
}
```

#### A.2.2 行解析 helper：`physicalJsonlRecords`

Codex 主线**不用 `readline`**，用的自研字节级异步生成器（`src/lib/jsonl-lines.js:29`），文件头注释逐字说明设计意图：

```js
// src/lib/jsonl-lines.js:3-10
/**
 * Yield physical JSONL records. Unlike node:readline, this intentionally treats
 * only LF as a record separator so valid U+2028/U+2029 characters inside JSON
 * strings remain part of the record. Reading raw bytes keeps byte budgets exact
 * and lets malformed UTF-8 fail closed.
 * Records longer than maxRecordBytes content bytes (excluding LF) are reported
 * as { line: null, utf8Valid: null, oversized: true } without being buffered
 * beyond the limit or decoded.
 */
```

产出对象逐字（`src/lib/jsonl-lines.js:91`）：

```js
// src/lib/jsonl-lines.js:91
yield { line, utf8Valid: true, oversized: false, physicalBytes, terminated: true };
```

四字段语义：

- `line`：解码后文本（`stripCr=true` 去掉尾部 `\r`），坏 UTF-8 或超长时为 `null`；
- `utf8Valid`：`false` 表示这段字节无法用 fatal decoder 解码（超长记录为 `null`）；
- `oversized`：超过 `maxRecordBytes`；
- `physicalBytes`：本记录**含换行符**的物理字节数——这是游标推进的原子单位；
- `terminated`：是否以 LF 结尾。**`terminated === false` 表示最后一行还未写完**，是"提交/不提交"的判据。

两个阈值与异常：

- `maxPhysicalBytes` 超限 → 抛 `PhysicalJsonlLimitError`（`src/lib/jsonl-lines.js:12`、`:62`）；
- `invalidUtf8` 策略只接受 `"throw" | "record"`（`src/lib/jsonl-lines.js:34`）。

**Codex 主路径传入的实参**（`src/lib/rollout.js:2396-2397`）：只传 `invalidUtf8`，**不设字节上限**：

```js
// src/lib/rollout.js:2396
  const invalidUtf8 = invalidRecordPolicy === "throw" ? "throw" : "record";
  for await (const record of physicalJsonlRecords(stream, { invalidUtf8 })) {
    scannedEndOffset += record.physicalBytes;
    if (record.terminated) committedEndOffset = scannedEndOffset;
```

`maxRecordBytes` 在全库只被 `parseOmpLikeIncremental` 族使用（`src/lib/rollout.js:15717`、`:15771`、`:15849`、`:15973`），且默认常量是 `OMP_LIKE_MAX_RECORD_BYTES`。**结论：Codex/Claude 两条主线没有"单行太长则跳过"的保护**，只有"最后一行不完整则不提交"的保护。

#### A.2.3 坏行计数现状（与任务书假设不符）

全库检索 `bad_lines|badLines|bad_line` 在 `src/lib/` 下**零命中**。真实做法是：

- Codex：`invalidRecordPolicy` 二态，默认 `"skip"`（`src/lib/rollout.js:261`），非法值抛 `TypeError`（`:263-265`）。`"throw"` 只被一次性血缘重建使用（`src/commands/sync.js:5774`）。坏 JSON 行在 skip 模式下**静默 `continue`**，不计数、不上报。
- Claude：坏行 **完全静默**。`src/lib/rollout.js:2812-2816`（user 行）与 `:2843-2847`（usage 行）都是 `catch (_e) { continue; }`。

**队列层的畸形行计数（已闭合）**：`countMalformedCodexProjectQueueRows`（`src/commands/sync.js:6436`）**全库只有一个调用点**——`src/commands/sync.js:5824`，位于 codex 重扫修复内部，作用是"重建前若发现 project 队列有畸形行，就放弃本次修复以免丢数据"（`:5825-5830` 打印错误并 `return false`）。它**只影响修复是否执行，不影响用量统计**：判定通过后仍由正常解析链按 `invalidRecordPolicy` 处理坏行。所以"队列层坏行计数"不构成用量的第二个来源。

#### A.2.4 超大文件保护的实际形态

不是"单文件字节上限"，而是三层降本：

1. **天目录清单缓存**：`cursors.codexDayInventoryCache`（`src/commands/sync.js:789` 的 `inventoryCacheKey: "codexDayInventoryCache"` 注入），配合 `listRolloutDayFiles` 的 statKey 命中（`src/lib/rollout.js:148-158`）。
2. **冷文件跳过**：`filterColdCodexRolloutFiles()`（`src/lib/rollout.js:708`），仅 `opts.auto` 下启用（`src/commands/sync.js:964-980`）；`DEFAULT_CODEX_COLD_SKIP_RECENT_DAYS = 2`（`:62`），且**近 30 天内的冷文件仍要 stat 一次看是否长大**（`:69`、`:806-822`，注释指向 issue #592）。
3. **游标分区存储**：`cursor-store-v2` 分片（`src/lib/cursor-store.js:13-21`），`DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024`——`cursors.json` 到 16MB 才升级为 v2 分片目录（`src/lib/cursor-store.js:21`、`:77-95`）。

---

### A.3 游标（cursor）对象的结构与语义

#### A.3.1 顶层结构

`cursors` 是一个大 JSON（或 v2 的分片集合），顶层键按用途分三类：

| 键 | 归属 | 语义 |
|---|---|---|
| `cursors.files[filePath]` | 双主线共用 | 逐文件游标（见下表） |
| `cursors.hourly` | 所有 parser | `{version: 3, buckets, groupQueued, updatedAt}`（`src/lib/rollout.js:3646-3651`） |
| `cursors.projectHourly` | 项目追踪开启时 | 项目维度桶（`normalizeProjectState`，`src/lib/rollout.js:3673`） |
| `cursors.claudeHashes` | Claude | 已计消息哈希数组，**上限 100000**（`src/lib/rollout.js:1236-1237`） |
| `cursors.claudeForkAliases` | Claude | `uuid → parentUuid` 别名表（`:1234`） |
| `cursors.codexHashes` | Codex | `sessionUUID:eventTimestamp` 已计事件数组（`:303`、`:2626`） |
| `cursors.acodeHashes` | Acode | 同上，按需懒建（`:385-391`） |
| `cursors.codexCompactionResponseIds` | Codex | compaction 响应 id，上限 20000（`src/lib/codex-usage-record.js:60`、`:77`） |
| `cursors.codexUsageRecordOnlyFiles` | Codex | `recordOnly` 告警文件表（`src/lib/rollout.js:649-655`） |
| `cursors.migrations[key]` | 全局 | 一次性修复标记（`src/commands/sync.js:248`、`:288` 等） |
| `cursors.<provider>` 私有命名空间 | 多个 provider parser | **未穷尽**：`cursors.claudeScience`（`src/lib/rollout.js:7136`）、`cursors.cursorApi`（`:7264-7268`，说明注释 `:7170`）、`cursors.kiro`（`:7589`）、`cursors.kiroCli`（`:9261`、`:9482`）、`cursors.traeCn`（`:22789`）等，各自持有 provider 专属的增量状态 |

上表覆盖通用键与双主线专用键，**不声明穷尽**：provider 私有命名空间另成一行，未逐一展开。

`hourly` 的状态机逐字（`src/lib/rollout.js:3627`）：

```js
// src/lib/rollout.js:3627
function normalizeHourlyState(raw) {
  const state = raw && typeof raw === "object" ? raw : {};
  const version = Number(state.version || 1);
  ...
  if (!Number.isFinite(version) || version < 2) {
    for (const [key, value] of Object.entries(rawBuckets)) {
      const parsed = parseBucketKey(key);
      const hourStart = parsed.hourStart;
      if (!hourStart) continue;
      const source = normalizeSourceInput(parsed.source) || DEFAULT_SOURCE;
      const normalizedKey = bucketKey(source, DEFAULT_MODEL, hourStart);
```

关键：**version < 2 的旧桶会把模型名抹成 `unknown` 并把 `queuedKey` 提升为 group 级**，返回 version 3。

#### A.3.2 逐文件游标字段（Codex 与 Claude 的差异）

| 字段 | Codex | Claude | 语义 |
|---|---|---|---|
| `inode` | ✔ `src/lib/rollout.js:633` | ✔ `:1193` | `st.ino || 0`（WSL 下不稳定） |
| `offset` | ✔ `:634` | ✔ `:1194` | **字节偏移**（非行号） |
| `updatedAt` | ✔ `:640` | ✔ `:1196` | ISO 写入时间 |
| `lastTotal` | ✔ `:635` | ✘ | 上次累计 `total_token_usage` |
| `tokenUsageBaselines` | ✔ `:636` | ✘ | LRU≤32 的流头快照 |
| `lastModel` / `lastServiceTier` | ✔ `:637` / `:638` | ✘ | 跨增量续用的模型/档位 |
| `modelAttributionState` | ✔ `:639` | ✘ | 模型归属状态机快照 |
| `codexUsageRecord` | ✔ `:645` | ✘ | `token_usage_record` 状态机快照 |
| `projectOffset` / `projectFileContext` | ✔ `:657-664`（**仅 `codexProjectFastPath` 为真时**才写新的；否则沿用 `prev`） | ✔ `:1200-1203`（**仅 `projectEnabled` 为真时**才写入） | 项目队列推进位 |
| `fileId` / `fileIdSize` | ✘ | ✔ `:1197` | 跨环境文件去重指纹 |
| `claudeForkIndexed` | ✘ | ✔ `:1195` | 该文件已做过 fork 别名索引 |
| `claudeCwd` / `projectRef` / `projectKey` | ✘（用 `projectFileContext`） | ✔ `:1200-1203`（**仅 `projectEnabled` 为真时**才写入） | Claude 的 cwd 可缓存（会话生命周期不变） |
| `duplicateOf` | ✘ | ✔ `:1084` | 被判定为另一路径副本时的主路径 |

**整表的前提（易误读）**：Claude 侧 `claudeCwd` / `projectFileContext` / `projectRef` / `projectKey` 四项不是无条件字段，而是写在 `...(projectEnabled ? { … } : {})` 的展开里（`src/lib/rollout.js:1198-1205`）——**项目追踪关闭时这四个键根本不存在**。Codex 侧的 `projectOffset` / `projectFileContext` 同理受 `codexProjectFastPath` 分支控制（`:656-665`）：命中快路径时写入新值，未命中时只在 `prev.projectOffset` 有限时沿用旧值，否则两者都不写。

#### A.3.3 unchanged / appended / rewritten 的判定

三态全靠 `fs.stat` + 两个布尔量。逐字（`src/lib/rollout.js:488-504`，Codex）：

```js
// src/lib/rollout.js:488
    const prev = cursors.files[key] || null;
    const inode = st.ino || 0;
    const sameInode = prev && prev.inode === inode;
    const prevOffset = sameInode ? prev.offset || 0 : 0;
    const truncated = sameInode && prevOffset > st.size;
```

- **unchanged（idle）**：`sameInode && !truncated && startOffset >= st.size`（Codex `:539-543`；Claude `:1121` 记为 `idle`）。此时直接 `continue`，**不重写游标**——这也是共享 `fileId` 缓存的正当性来源（`src/lib/rollout.js:1064-1065` 注释）。
- **appended**：`sameInode && !truncated && startOffset < st.size` → 从 `prevOffset` 续读。
- **rewritten（被外部工具改写）**：
  - inode 变化 → `sameInode=false` → `startOffset=0`、`lastTotal=null` 全量重读（`:501`）；
  - 文件被截断 → `truncated=true` → 同样回到 0；
  - 特殊态 `rebuildingCodexBaseline`：inode 未变、有增长、但 `prev.lastTotal` 不是对象 → 从 0 重扫以重建累计基线（`:493-500`）。
- **Claude 额外一态**：`projectSettled`（`:1133-1134`）——idle 且项目上下文已定（`claudeCwd === null` 或 `projectFileContext.absent` 或 `isProjectFileContextFresh`）才跳过；**仍在增长的文件即使 cwd 未找到也会重试**，避免早期同步丢掉整段会话的项目归属（`:1115-1120` 注释）。

offset 的**提交粒度不同**。先纠一个易误传的口径：`const endOffset = st.size;` 在 `rollout.js` 里共 **3 处**（`grep` 精确命中 `:2291`、`:2682`、`:2793`），**但只有第 3 处落到 token 游标**：

| 站点 | 所属函数 | 返回什么 | 是否推进 token 游标 |
|---|---|---|---|
| `:2291` | `parseRolloutFile`（`:2264`，Codex 单文件主解析） | 该字节等于 `st.size`，仅用于 `startOffset >= endOffset` 的"无新字节"早退（`:2294-2302`）——数据没读就返回，返回值不可能大于入参 `startOffset` | **否**（正式返回仍走 `committedEndOffset`，`:2652`） |
| `:2682` | `scanRolloutProjectFileContexts`（`:2666`，Codex 项目上下文扫描） | `return { endOffset: committedEndOffset, … }`（`:2764-2765`） | **否**（只推进 `projectOffset`） |
| `:2793` | `parseClaudeFile`（`:2776`，Claude 单文件主解析） | `return { endOffset, eventsAggregated }`（`:2889`）→ `cursors.files[key].offset = result.endOffset`（`:1194`） | **是——唯一一处** |

- Codex 的 token 路径：`endOffset: committedEndOffset`（`src/lib/rollout.js:2652`），只提交到最后一个 `terminated` 行；
- **Codex 的非 token 路径也做了同样的保护**：项目上下文扫描位同样返回 `committedEndOffset`（`:2765`）——即"提交位保护"在 Codex 侧是**全游标一致的应用**，不是只保护 token；
- Claude：`const endOffset = st.size;`（`:2793`）+ `return { endOffset, eventsAggregated }`（`:2889`）——**直接跳到文件当前大小**，token 与项目上下文共用同一个 `offset`；
- OpenClaw 用显式边界检查补齐（`:1949`）：

```js
// src/lib/rollout.js:1949
async function openclawOffsetEndsAtLineBoundary(fileHandle, offset) {
  const byteOffset = Math.max(0, Number(offset) || 0);
  if (byteOffset === 0) return true;
  const buffer = Buffer.allocUnsafe(1);
  const { bytesRead } = await fileHandle.read(buffer, 0, 1, byteOffset - 1);
  return bytesRead === 1 && (buffer[0] === 0x0a || buffer[0] === 0x0d);
}
```

**结论（重要）**：Claude 路径对"同步过程中文件正在被写、最后一行未闭合"的处理方式，与 Codex/OpenClaw **不一致**；`readline` 会把未闭合尾行也 `yield` 出来（`crlfDelay: Infinity`），JSON.parse 失败则 `continue`（`:2843-2847`），随后 `offset` 仍推进到 `st.size`——**该行的字节在本代文件内被跳过**（该文件下次 inode 变化或截断时会全量重扫，见 `:488-504`，所以不是严格永久）。本仓库其他 parser（Codex 的 `committedEndOffset`、OpenClaw 的 `openclawOffsetEndsAtLineBoundary`、OMP 族的 `maxRecordBytes`）都为此写了保护，Claude 没有。→ 列入「存疑与待复核点」。

#### A.3.4 cursor-store v2 分片

`src/lib/cursor-store.js` 的常量与开库逻辑逐字：

```js
// src/lib/cursor-store.js:13
const STORE_VERSION = 2;
const STORE_DIRNAME = "cursor-store-v2";
const MANIFEST_FILENAME = "manifest.json";
const GENERATIONS_DIRNAME = "generations";
const DEFERRED_CODEX_AUDIT_FILENAME = "deferred-codex-audit.json";
const SHARDED_CORE_FILENAME = "core-v3.json";
const OPENCODE_MESSAGES_DIRNAME = "opencode-messages";
const OPENCODE_FINGERPRINTS_DIRNAME = "opencode-fingerprints";
const DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024;
```

两层形态：`LegacyCursorStore`（单 `cursors.json`）与 v2 分片存储；分片维度是"codex 文件游标按天目录 / 事件哈希按前缀"（`partitionCursorFiles`，`src/lib/cursor-store.js:1711`；`codexFileShardKey`，`:1788`；`codexDayDirectory`，`:1795`；`codexEventShardKey`，`:1800`）。Codex 事件去重集还有一个外部存储后端（`this.codexEventStore`，`:252`；由 `sync.js:1049-1051` 传入）。

---

### A.4 解析器如何被 sync.js 调用（与解析契约相关的部分）

#### A.4.1 发现与传参

Codex 调用点（`src/commands/sync.js:1046`）：

```js
// src/commands/sync.js:1046
    const runCodexParse = (files) => parseRolloutIncremental({
      rolloutFiles: files,
      cursors,
      codexEventStore: Array.isArray(cursors.codexHashes)
        ? null
        : cursorStore.codexEventStore,
      queuePath,
      projectQueuePath,
      diagnostics: syncDiagnostics,
      onProgress: (p) => { ... },
    });
```

要点：`codexEventStore` 的取值是**"若 `cursors.codexHashes` 还是旧数组形态就传 null（走内存 Set），否则用分片 store"**——这是 v1→v2 迁移的兼容分支。`source` 未传，由每个 entry 自带的 `source` 决定（`src/commands/sync.js:894` 组装 `{path, source}`）。

Claude 调用点（`src/commands/sync.js:1201`）：

```js
// src/commands/sync.js:1201
        claudeResult = await parseClaudeIncremental({
          projectFiles: claudeFiles,
          cursors,
          queuePath,
          projectQueuePath,
          onProgress: (p) => { ... },
          source: "claude",
        });
```

失败处理：两边都是 `warnProviderParseFailure(...)` 吞掉异常继续（Codex 例外：`isCursorStoreRetry(err)` 时会重载游标并**对全量文件重跑一次**，`src/commands/sync.js:1068-1085`；`TOKENTRACKER_CURSOR_STORE_CORRUPT` 直接抛出，`:1086-1087`）。

#### A.4.2 队列写入：整行快照 + 差量去重

`enqueueTouchedBuckets()`（`src/lib/rollout.js:3169`）是唯一出口，写入前用 `queuedKey` 比对避免重复追加（`:3327` `if (bucket.queuedKey === key) continue;`）。行格式逐字（`:3328-3345`）：

```js
// src/lib/rollout.js:3328
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

项目队列另有一份（`enqueueTouchedProjectBuckets`，`src/lib/rollout.js:3489`），主键换成 `project_key|source|hour_start`，并列携带 `project_ref`（`:3516-3518`）。

#### A.4.3 聚合口径（消费端）

**本地面板**：`readQueueData` 的 key 逐字（`src/lib/local-api.js:207`）：

```js
// src/lib/local-api.js:207
      key: `${row.source || ""}|${row.model || ""}|${row.hour_start || ""}`,
      row: normalizeQueueRow(row),
```

配合 `createAsyncQueueReader`（`src/lib/async-queue-reader.js`）——**同键后写覆盖前写**，即"最后一条快照即现值"。日聚合是**跨模型求和**（`buildDailyAggregates`，`src/lib/local-api.js:260-298`）。

**上传批次**：`readQueueBatch` 同样是 Map 覆盖（`src/commands/sync.js:4510`），并按 `nextOffset` 推进；未闭合尾行会 `break` 留待下次（`:4449`）。

**修复/对账**：`mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal(key, total_tokens)`（`src/commands/sync.js:6540`）——**取最大值而非最后值**，然后 `collapseModelTotalsByHour` 再跨模型求和（`:6564-6571`）。这是一处与读取语义不同的对账口径。

**日归属**：`rowDayKey()`（`src/lib/local-api.js:213`）先把 `hour_start` 按 `timeZoneContext` 转分区时间，再 `formatPartsDayKey`；没有时区上下文时**直接截字符串前 10 字符**（`:224`）：

```js
// src/lib/local-api.js:213
function rowDayKey(row, timeZoneContext) {
  const hs = row.hour_start;
  if (!hs) return "";
  if (
    timeZoneContext &&
    (timeZoneContext.timeZone || Number.isFinite(timeZoneContext.offsetMinutes))
  ) {
    const parts = getZonedParts(new Date(hs), timeZoneContext);
    const key = formatPartsDayKey(parts);
    if (key) return key;
  }
  return hs.slice(0, 10);
}
```

**会话计数的两套语义**（跨主线对照，供消费端注意）：Codex 每条被接受的 delta 记 `conversation_count = 1`，而 Claude 的 assistant 行恒 0、只有主会话 user 文本行 +1——同一列名在两条主线含义不同，详见 §C.3.2 与 §B.2.5。

---

### A.5 半小时桶（half-hour bucket）如何生成

#### A.5.1 唯一实现（逐字）

```js
// src/lib/rollout.js:4173
function toUtcHalfHourStart(ts) {
  const dt = new Date(ts);
  if (!Number.isFinite(dt.getTime())) return null;
  const minutes = dt.getUTCMinutes();
  const halfMinute = minutes >= 30 ? 30 : 0;
  const bucketStart = new Date(
    Date.UTC(
      dt.getUTCFullYear(),
      dt.getUTCMonth(),
      dt.getUTCDate(),
      dt.getUTCHours(),
      halfMinute,
      0,
      0,
    ),
  );
  return bucketStart.toISOString();
}
```

语义拆解：

- 入参 `ts` 是**任意可被 `Date` 解析的字符串**（Claude 用 `obj.timestamp`；Codex 用 `token.timestamp`）；
- 取整规则：`minutes >= 30 → :30`，否则 `:00`；秒、毫秒强制清零；
- **全 UTC**：`getUTC*` + `Date.UTC`，输入若是带偏移的 ISO 串会先被 Date 规范化到 UTC；
- 输出 `toISOString()`，形如 `2026-10-01T12:30:00.000Z`；
- 非法/缺失时间戳 → `null`，调用方一律 `if (!bucketStart) continue;`（Claude `:2867-2868`；Codex `:2605-2606`；Claude Science `:7071-7074`）。

#### A.5.2 两处独立复制（必须注意）

同一算法在仓库内**存在两份实现**（全库 `function toUtcHalfHourStart` 仅 2 命中，不是共享 import）：

1. `src/lib/rollout.js:4173`（主实现，被 `module.exports` 导出，所有增量 parser 使用）；
2. `src/lib/claude-categorizer.js:1175`（Claude 真值修复专用）。

**两份的关系是"语义等价、文本不同"，不是"逐字一致"**：

- 取整规则完全相同（`minutes >= 30 ? 30 : 0`；秒、毫秒清零；`Date.UTC` + `getUTC*`；非法时间戳返回 `null`）；
- 但写法不同：`rollout.js:4178-4189` 先 `const bucketStart = new Date(Date.UTC(…))` 再 `return bucketStart.toISOString()`；`claude-categorizer.js:1180-1190` 把 `new Date(Date.UTC(…)).toISOString()` 写成单行链式返回（`return new Date(…)…).toISOString();`）。

**没有任何测试或断言锁定二者的等价性**（这半句经实测成立）：`test/` 下引用 `toUtcHalfHourStart` 的只有 `test/sync-droid-dup-session-repair.test.js` 与 `test/trae-entitlement.test.js:130-137`，两处都 `require("../src/lib/rollout")`——**`claude-categorizer.js` 的那一份无人测**。改动任一处不会让另一处失败。→ 列入存疑清单。

#### A.5.3 跨日与本地时区

写入侧**完全不做跨日处理**：桶只按 UTC 半时划分，`hour_start` 的日期部分永远是 UTC 日期。

跨日语义全部在**读侧**：`toUtcHalfHourStart` 无本地时区参与，`rowDayKey`（`src/lib/local-api.js:213`）才把 UTC 桶映射到展示时区的自然日；`getZonedParts` 用 `Intl.DateTimeFormat("en-CA", { timeZone, ... hourCycle: "h23" })`（`src/lib/codex-rollout-parser.js:84-97`），失败时退化为 `offsetMinutes` 平移（`:103-110`）。

**唯一在写入侧使用"本地日历日"的地方**是 Codex 冷文件跳过：`formatLocalDate`（`src/lib/rollout.js:923`）+ `activeCodexRolloutDates`（`:893`），它比较的是**文件名里的日期**（`rolloutDateFromPath`，`:4192`）与本地日历日——这属于扫描调度，不影响桶。

---

## B. Claude Code（`parseClaudeIncremental` + `parseClaudeScienceIncremental`）

### B.1 日志发现路径

#### B.1.1 多根组装（逐字）

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

再叠加 `CLAUDE_CONFIG_DIR` / `config.scanRoots.claude`（`:694-699`），最后落到 `projects/` 子目录并按 realpath 去重（`:700-703`）：

```js
// src/commands/sync.js:700
    const claudeProjectsDirs = appendUniqueDirs(
      claudeInstallHomes.map((h) => path.join(h, "projects")),
      extraScanRootPaths(scanRoots.claude).map((h) => path.join(h, "projects")),
    );
```

**关键差异（与 Codex 对立）**：Claude **UNION 全部安装**，不做 wsl-first / native-first 的单选（注释逐字见于 `src/commands/sync.js:676-684`：`Claude UNIONS every allowed install instead of taking resolveAllWin32Paths' single pick`）。理由是同段注释说明：单选会让 WSL 抢占 native，进而让"磁盘即真值"的修复**抹掉** native 历史。

#### B.1.2 目录内遍历与 slug 处理

发现函数是 `listClaudeProjectFiles` → `walkClaudeProjects`，**递归任意深度、只认 `.jsonl`**（`src/lib/rollout.js:5013-5023`）。slug 编码**不被解析**，理由逐字（`src/lib/rollout.js:4600-4606`）：

```js
// src/lib/rollout.js:4600
// "cwd" field) — unlike the file's on-disk location under
// ~/.claude/projects/<dash-encoded-cwd>/, which is not itself inside the
// real project's git checkout and can never resolve a project via directory
// walk. The encoded name can't be decoded reliably either (dashes in the
// real path collide with the path-separator encoding), so read the real cwd
// from content instead. It's set once at session start and bounded scan is
// cheap since it's a top-level field on the first several lines.
const CLAUDE_CWD_SCAN_MAX_BYTES = 65536;
```

**结论：slug 只是目录名，不参与任何身份判定**；项目身份来自内容里的 `cwd`（见 B.6）。

文件类型：`projects/**/*.jsonl`，其中 `**/subagents/*.jsonl` 被识别为子代理转录（`isMainSession` 判定，`src/lib/rollout.js:2803`）：

```js
// src/lib/rollout.js:2803
  const isMainSession = !/[\\/]subagents[\\/]/.test(filePath);
```

注意该判定**同时接受 `/` 与 `\`**，原因是 WSL 经 `\\wsl$` 访问时 `path.join` 会产出反斜杠（`src/lib/rollout.js:2800-2802` 注释，指向 issue #307）。

多根下同文件去重：`src/commands/sync.js:1137-1145` 用 `seenClaudeFiles` Set 按路径去重；跨环境同内容副本另由 `claudeFileIdentityHash` 处理（`src/lib/rollout.js:1021-1107`）。

#### B.1.3 `claude-mem` 观察者目录的特殊处理

`CLAUDE_MEM_OBSERVER_PATH_SEGMENT = "--claude-mem-observer-sessions"`（`src/lib/rollout.js:58`），命中即赋予固定 `projectRef`（`:59-60`、`:4297-4301`）：

```js
// src/lib/rollout.js:59
const CLAUDE_MEM_OBSERVER_PROJECT_REF =
  "https://local.tokentracker/claude-mem/observer-sessions";
```

`projectRef` 以 `https://` 开头，说明它被当作"远程仓库地址"塞进同一套项目标识；另有一次性迁移 `reincludeClaudeMemObserverFiles`（`src/commands/sync.js:1148`、`:7100`）。

#### B.1.4 `parseClaudeScienceIncremental` 的真实对象（纠正）

它**不是** Claude Code 的第二个解析器。证据（`src/lib/rollout.js:6841-6952`）：

- 数据源是 SQLite 的 `frames` 表，不是 JSONL：`buildClaudeScienceFramesQuery` 读 `id, parent_frame_id, model, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, aux_*`（`:6841-6871`）；
- 库路径 `~/.claude-science/operon-cli.db` 或 `operon.db`（`:6875`、`:6882`），多组织安装走 `orgs/<slug>/`（`:6892-6906`），Windows 无原生版，需探 WSL（`:6910-6929`）；
- 令牌映射完全独立：`claudeScienceUncachedInput = input - cacheRead - cacheWrite`（`:6999`），`reasoning_output_tokens: 0` 恒为 0（`:7023`）；
- 聚合用**幂等减法**（先 `subtractTotals` 旧贡献再 `addTotals` 新值，`:7097-7117`），`conversation_count` 只记根帧（`parent_frame_id` 为空，`:7079-7085`）；
- `source` 常量是字面量 `"claude-science"`（`:7101`）。

---

### B.2 逐行解析：读哪些字段、五桶如何映射

#### B.2.1 过滤与提取

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

要点：

- **字符串预筛**：`line.includes('"usage"')`，无 usage 直接跳过解析（廉价优化）；
- **usage 双位置回退**：`obj.message.usage` 优先，`obj.usage` 兜底（`:2851`）；
- **OmO 镜像行**被剥离（`:2849`）：判定为 `entrypoint === "sdk-ts" && userType === "external"`（`:4910-4912`），注释说明这是 OmO Native 经 Claude Agent SDK 的镜像拷贝，避免与 `source=omo` 双计（`:4906-4909`，issue #726）。

#### B.2.2 五桶映射（逐字）

```js
// src/lib/rollout.js:4921
function normalizeClaudeUsage(u) {
  const inputTokens = toNonNegativeInt(u?.input_tokens);
  const outputTokens = toNonNegativeInt(u?.output_tokens);
  const reasoningTokens = Math.min(outputTokens, toNonNegativeInt(
    u?.output_tokens_details?.thinking_tokens ?? u?.output_tokens_details?.reasoning_tokens,
  ));
  const cacheCreation = toNonNegativeInt(u?.cache_creation_input_tokens);
  const cacheRead = toNonNegativeInt(u?.cache_read_input_tokens);
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
}
```

映射表：

| 输出桶 | 来源字段 | 变换 |
|---|---|---|
| `input_tokens` | `usage.input_tokens` | **直取**（Claude 原生语义已是"不含缓存"，无需减法） |
| `cached_input_tokens` | `usage.cache_read_input_tokens` | 直取（=缓存**读**） |
| `cache_creation_input_tokens` | `usage.cache_creation_input_tokens` | 直取（=缓存**写**） |
| `output_tokens` | `usage.output_tokens` | **减去 reasoning**（`outputTokens - reasoningTokens`） |
| `reasoning_output_tokens` | `usage.output_tokens_details.thinking_tokens` **或** `.reasoning_tokens` | 取先存在者；`Math.min(outputTokens, …)` 封顶防倒挂 |
| `total_tokens` | — | `input + output(原始含推理) + cacheCreation + cacheRead` |

即：**`total_tokens` 用未减 reasoning 的 output 计算**，与"output 拆成两列"互补，保证 `input + cached + cacheCreate + output + reasoning == total`（这正是 `test/claude-reasoning-usage.test.js:44` 的断言）。

`toNonNegativeInt` 定义（`src/lib/rollout.js:4964`）：非有限或负 → `0`；否则 `Math.floor`。所以小数 token、负值都被吸收为确定值。

#### B.2.3 缓存字段的三种命名兼容

Claude 路径**只认 `cache_read_input_tokens` / `cache_creation_input_tokens`**（`:4927-4928`）。Codex 路径额外兼容 `cache_write_input_tokens` 别名（`src/lib/codex-token-usage.js:25-27`）：

```js
// src/lib/codex-token-usage.js:25
    const raw = field === "cache_creation_input_tokens"
      ? value.cache_creation_input_tokens ?? value.cache_write_input_tokens
      : value[field];
```

两处命名不一致：**代码事实成立**——Claude 路径只读 `cache_creation_input_tokens`（`:4927-4928`），`cache_write_input_tokens` 别名兜底只存在于 Codex 路径（`src/lib/codex-token-usage.js:25-27`，逐字如上）。因此跨 provider（Kimi/DeepSeek/Mimo 等 anthropic 兼容端点）**若**用别名写缓存写量，Claude 路径会**静默丢缓存写**。

【判定缺口】"这些 provider 实际是否会这么写"**无法从源码判定**，需要的是：本机（或合成 fixture）中 Claude 会话行里 `cache_write_input_tokens` 的出现率统计——若该键出现次数 > 0 而同行 `cache_creation_input_tokens` 缺失，丢量即确定发生；若两者都出现或都不出现，则本条的**实际**影响为零。现有证据不足以给出丢量规模，故只能保留为"条件性风险"。→ 列入存疑清单。

#### B.2.4 零用量与写入判定

```js
// src/lib/rollout.js:2861
    const delta = normalizeClaudeUsage(usage);
    if (!delta || isAllZeroUsage(delta)) continue;

    if (dedupHash) seenMessageHashes.add(dedupHash);
    delta.conversation_count = 0;
```

**assistant 行的 `conversation_count` 恒为 0**（`:2865`）；会话计数只来自 user 文本行（见下）。`isAllZeroUsage`（`:4945`）对六个桶全为 0 时返回 true。

#### B.2.5 会话计数（user 文本行）

```js
// src/lib/rollout.js:2810
    if (isMainSession && line.includes('"type":"user"')) {
      ...
        const content = userObj?.message?.content;
        const hasText =
          typeof content === "string" ||
          (Array.isArray(content) && content.some((b) => b?.type === "text"));
```

规则：**仅主会话**、`type === "user"`、`message.content` 是字符串或含 `{type:"text"}` 块之一 → `conversation_count += 1`，且**记进 `unknown` 模型桶**（`:2831` `const userModel = DEFAULT_MODEL;`），与 token 桶分离。工具结果行（只有 `tool_result` 块）被排除（`:2807-2809` 注释）。

---

### B.3 去重与身份

#### B.3.1 消息去重键（逐字）

```js
// src/lib/rollout.js:4895
// Stable dedup key for one Claude jsonl entry. Anthropic's official protocol
// guarantees `message.id` is globally unique per response, so msgId alone is a
// valid dedup key. Older code required both msgId AND requestId, which short-
// circuited dedup entirely for jsonl entries where `requestId` is absent
// (DeepSeek/Kimi/Mimo/MiniMax anthropic-compatible endpoints don't return the
// `request-id` HTTP header, and Claude Code's sub-agent / thinking transport
// paths drop the field too). The short-circuit caused 1.6–3.7x overcounting on
// every affected provider — see issue #64. Falling back to msgId-only keeps
// backward compatibility for the (msgId, reqId) format already persisted in
// cursors.claudeHashes (msgId strings don't contain `:`, so the two formats
// share the same Set without collision).
function isOmOClaudeSdkMirror(obj) {
  return obj?.entrypoint === "sdk-ts" && obj?.userType === "external";
}

function claudeMessageDedupKey(obj) {
  const msgId = typeof obj?.message?.id === "string" && obj.message.id ? obj.message.id : null;
  if (!msgId) return null;
  const reqId = typeof obj?.requestId === "string" && obj.requestId ? obj.requestId : null;
  return reqId ? `${msgId}:${reqId}` : msgId;
}
```

要点：

- 无 `message.id` → 返回 `null` → **完全不去重**（该行必计）；
- 有 `requestId` 时键为 `msgId:reqId`，否则为裸 `msgId`；两种格式共用一个 Set 而不冲突（注释给出理由）；
- `uuid` / `parentUuid` **不参与 token 去重**，只用于会话身份与 fork 折叠（B.3.2）。

持久化与上限（`src/lib/rollout.js:1233-1237`）：

```js
// src/lib/rollout.js:1233
  // Persist message hashes for cross-sync dedup; cap at 100k entries to bound size.
  cursors.claudeForkAliases = forkAliases;
  const allHashes = Array.from(seenMessageHashes);
  cursors.claudeHashes =
    allHashes.length > 100_000 ? allHashes.slice(allHashes.length - 100_000) : allHashes;
```

**上限 100k 且是"保留最后 100k"**——超出时最早的哈希被挤出，理论上给"批量重扫旧文件"留下重计窗口（注释在 `:1021-1025` 承认该风险并因此加了文件级去重层）。

#### B.3.2 fork 会话身份折叠（`claude-user-identity.js`）

规则（`src/lib/claude-user-identity.js:5-25`）：

```js
// src/lib/claude-user-identity.js:5
function noteClaudeFork(aliases, obj) {
  const uuid = obj?.uuid;
  const parent = obj?.forkedFrom?.messageUuid;
  if (typeof uuid === "string" && uuid && typeof parent === "string" && parent && uuid !== parent) {
    aliases[uuid] = parent;
  }
}

function claudeUserIdentity(obj, aliases) {
  let uuid = typeof obj?.uuid === "string" && obj.uuid ? obj.uuid : null;
  if (!uuid) return null;
  const chain = new Set();
  while (!chain.has(uuid)) {
    chain.add(uuid);
    const parent = aliases[uuid];
    if (typeof parent !== "string" || !parent) return `u:${uuid}`;
    uuid = parent;
  }
  // Malformed cycles still collapse deterministically regardless of file order.
  return `u:${Array.from(chain).slice(Array.from(chain).indexOf(uuid)).sort()[0]}`;
}
```

- 别名来源：**顶层 `uuid` + `forkedFrom.messageUuid`**，且**只采信 `type === "user"` 的行**（`src/lib/claude-user-identity.js:44`）；
- 子代理路径被排除（`:31` `if (!filePath || /[\\/]subagents[\\/]/.test(filePath)) continue;`）；
- 身份前缀是 `u:`，与消息键区分（`src/lib/rollout.js:997-1003` 只对 `startsWith("u:")` 的历史哈希做折叠）；
- 别名表持久化在 `cursors.claudeForkAliases`（`src/lib/rollout.js:1234`）。

**预扫描 + 净值修正**（`src/lib/rollout.js:987-1015`）：`collectClaudeForkAliases` 在计数**之前**跑完整批文件，因为"fork 文件可能排在前面"（`:987-988` 注释）；随后把历史计数中"同一族多于 1 条"的部分**从 `conversation_count` 里减掉**：

```js
// src/lib/rollout.js:1004
    for (const [canonical, count] of families) {
      if (count < 2) continue;
      const ts = familyTimes.get(canonical);
      const hour = ts ? toUtcHalfHourStart(ts) : null;
      if (!hour) continue;
      const key = bucketKey(defaultSource, DEFAULT_MODEL, hour);
      const bucket = hourlyState.buckets[key];
      if (!bucket || bucket.totals.conversation_count < count - 1) continue;
      bucket.totals.conversation_count -= count - 1;
      touchedBuckets.add(key);
    }
```

注意：**只修 `conversation_count`，不动 token**（`:992-994` 注释：`Preserve all token totals and history whose source files were removed.`）；且有下界保护（`bucket.totals.conversation_count < count - 1` 则跳过）。

#### B.3.3 两个测试的断言含义

`test/claude-fork-conversations.test.js`（95 行）：

| 断言位置 | 含义 |
|---|---|
| `:37-39`（两种文件顺序） | 三层链 `original ← copy ← nested` 的三个 `uuid` 只算 **1** 次会话，token 仍为 `10+20 = 30`；**结果与文件顺序无关**（`for (const reverse of [false, true])`，`:35`） |
| `:40-43` | 游标 JSON 序列化/反序列化后再跑一次，计数**仍然是 1/30**（幂等） |
| `:44-45` | 与独立实现 `computeClaudeGroundTruthBuckets` 的会话数对账一致 |
| `:48-59` | 同一个 fork 文件里后追加的**新** user 轮次应独立计数（1 → 2），且再次全量重跑不增长 |
| `:61-78` | 复现"旧版本把三个 uuid 都算过"的污染状态：`conversation_count` 被手工改成 4、`claudeHashes` 塞入 `["u:original","u:copy","u:nested","response:request"]`、删掉 `claudeForkAliases` 与 `claudeForkIndexed` → 重扫后应修复为 **2**（三个 uuid 折叠成 1 个 + 一个已删除会话的历史保留 = 2），`total_tokens` 仍为 30 |
| `:80-86` | 删掉原始文件、只剩 fork 副本时，仍只算 1 次会话 |
| `:88-95` | 父文件**后来**才出现在扫描列表时，已计过的嵌套 fork 要被回溯修正 |

`test/claude-reasoning-usage.test.js`（51 行）断言的是**推理拆分的边界与账单不变式**：

- 10 组 `output_tokens_details` 输入（`:14-25`），包括 `thinking_tokens` 与 `reasoning_tokens` 两种命名、负值、字符串、超限值 `9999`；
- `:41` 断言行内 `reasoning_output_tokens` 等于期望值（`9999` 被 `Math.min(518, 9999)` 封顶为 `518`）；
- `:42` `output_tokens === 518 - reasoning`；
- `:43` `total_tokens === 1058`（`420 + 100 + 20 + 518`），**不随拆分方式变化**；
- `:44` 显式校验五列之和等于 `total_tokens`；
- `:45-47` 关键：把 `{output_tokens: 518, reasoning: 0}` 作为"未拆分"的对照，`computeRowCost` 结果**必须相同**（`< 1e-12`）——即拆分是**纯展示层**操作，不能改变费用；
- `:48-49` 再跑一次 `parseClaudeIncremental`，队列文件字节完全不变（幂等）。

#### B.3.4 sidechain（子代理）

**`sidechain` 字段在 token 统计路径上零命中**（全库 `grep sidechain` 只命中 `src/lib/session-analytics.js:2660`，那是会话浏览器的注释）。Claude 主线的"子代理"概念完全由**路径**表达：

- 会话计数排除 `.../subagents/...`（`src/lib/rollout.js:2803`、`:2810`）；
- fork 别名索引排除同路径（`src/lib/claude-user-identity.js:31`）；
- **token 计数不排除**——子代理转录里的 assistant usage 行照常计入，只是归属到该子代理文件所属的 cwd 项目。

---

### B.4 模型名来源与哨兵值

#### B.4.1 取值顺序（逐字）

```js
// src/lib/rollout.js:2857
    const model = normalizeModelInput(obj?.message?.model || obj?.model) || DEFAULT_MODEL;
```

- 先 `message.model`，后顶层 `model`（`obj.model`）；
- `normalizeModelInput`（`:4260`）**只做 trim，不改大小写、不做任何别名归一**；
- 无值 → `DEFAULT_MODEL = "unknown"`（`:56`）。

**因此 opus/sonnet/haiku 各变体、带日期后缀的版本串、非 Anthropic 路由串（如 `grok-4.5-build`）在这里全部原样落桶**。归一化不在解析层——它与定价/分组是另一条链路（本仓 `pricing/matcher.js` 的分段匹配），S2 不越界展开。

#### B.4.2 `<synthetic>` 等哨兵值

- **解析层没有任何 `<synthetic>` 特判**。全库出现 `<synthetic>` 的只有会话浏览器，共 3 处：`src/lib/session-analytics.js:101`（下示集合）、`:621-624`（注释：`"<synthetic>". They are not a billable model and can appear after the real assistant messages in the same session.`——同文件 `:625-626` 的 `normalizeSessionModel` 用来防止该标记覆盖真实模型名）、`:1968-1971`（注释：Claude Memory 的观察者转录里是 `<synthetic>`/haiku 记账）：

```js
// src/lib/session-analytics.js:101
const PLACEHOLDER_MODELS = new Set(["<synthetic>", "synthetic", "<unknown>", "unknown"]);
```

- 后果：若某行 `message.model === "<synthetic>"` 且 usage 非零，它会**照常生成 `claude|<synthetic>|<hour>` 桶**（因为 `:"<synthetic>"` 是真值字符串）；若 usage 全零则被 `isAllZeroUsage` 丢弃（`src/lib/rollout.js:2862`）。
- 【未能确认，缺什么才能判定】真实 Claude Code 是否会为 `<synthetic>` 行写入非零 usage——**本次未拿到真实日志样本，代码层面无法判定**。要闭合只需一条检索：在本机 `~/.claude/projects/**/*.jsonl` 里找 `"model":"<synthetic>"` 的行，看同行 `message.usage` 是否非零（本仓测试 fixture 不含该形态）。若存在非零 usage，面板会出现一个名为 `<synthetic>` 的模型行——这是纯展示层后果，不影响总量（那些 token 本来就该计）。

---

### B.5 时间戳来源、时区处理与 `hour_start` 生成

| 环节 | 代码 | 说明 |
|---|---|---|
| 时间戳取值 | `src/lib/rollout.js:2858` | `typeof obj?.timestamp === "string" ? obj.timestamp : null`；**非字符串一律丢弃整行**（`:2859`） |
| 桶生成 | `src/lib/rollout.js:2867` | `toUtcHalfHourStart(tokenTimestamp)`，非法 → `null` → `continue` |
| 桶键 | `src/lib/rollout.js:2872` | `bucketKey(source, model, bucketStart)` → `claude|<model>|<ISO>` |
| user 会话计数同理 | `src/lib/rollout.js:2828-2835` | 同一时间戳路径，模型固定 `unknown` |

时区处理的**事实**：

- 解析层**不读取任何时区字段**，完全信任 `timestamp` 字符串自带偏移；
- 仓库内**没有**"Claude 日志时区"配置项参与 `hour_start`；跨日只发生在读侧（A.5.3）；
- 唯一与"Claude 时区"相关的显式代码是 WSL 路径映射 `wsl.mapWslCwdToUnc(nextCwd, filePath)`（`src/lib/rollout.js:1161`），那是 cwd 而非时间。

`hour_start` 的**归属桶**（写侧）与**展示日**（读侧）是两个正交阶段，任何"跨日漂移"都必须在这两层之间解释——这是本报告认为最容易误判的一点。

---

### B.6 项目归属

#### B.6.1 cwd 提取（有界扫描）

```js
// src/lib/rollout.js:4609
async function resolveClaudeFileCwd(filePath) {
  const stream = fssync.createReadStream(filePath, {
    encoding: "utf8",
    start: 0,
    end: CLAUDE_CWD_SCAN_MAX_BYTES,
  });
  const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });
  try {
    for await (const line of rl) {
      if (!line || !line.includes('"cwd"')) continue;
      ...
      if (typeof obj?.cwd === "string" && obj.cwd.trim()) return obj.cwd.trim();
```

- 只扫**前 64KB**（`CLAUDE_CWD_SCAN_MAX_BYTES`，`:4607`）；
- 命中即返回，**取第一条**；
- 结果缓存在游标的 `claudeCwd`（`:1200`），"找不到"用 `null` 显式表示（`nextCwd === undefined ? null : nextCwd`，`:1200`）；
- 辨析：`claudeCwd === null`（确认没有）与 `undefined`（未探过）语义不同（`:1132` `const projectKnownAbsent = cachedCwd === null || cachedContextAbsent;`）。

#### B.6.2 路径 → 项目 key

`resolveProjectContextForPath({startDir, projectMetaCache, publicRepoCache, publicRepoResolver, projectState})`（`src/lib/rollout.js:4549`），内部 `resolveProjectMetaForPath`（`:4293`）自下而上找 `.git`：

- worktree/submodule 支持：`.git` 是文件时读 `gitdir:`，再读 `commondir`（`:4634-4668`）；
- 远端 URL 解析：优先 `origin`，否则取第一个（`:4690-4692`）；
- `canonicalizeProjectRef`（`:4695`）与 `deriveProjectKeyFromRef`（`:4429`）产出稳定 key；
- 缓存键是按目录的（`:4303-4312` 的 `visited` 回填）。
- WSL 侧 cwd 先过 `mapWslCwdToUnc`（`:1161`）再解析。

Claude 的**特殊之处**：`projectRef/projectKey` 是**文件级**（一次解出，写进文件游标 `:1200-1203`），而 Codex 是**行级推进**（`turn_context.cwd` 变更即切，`:2496-2511`）。这与 CLAUDE.md 中 TokenScope 的设计描述同构（Claude 按会话固定根、Codex 由 session_meta 重置 / turn_context 推进）。

#### B.6.3 `claude-config.js` 的真实职责（纠正任务书假设）

`src/lib/claude-config.js` 共 279 行，全部是 **Claude Code `settings.json` 的 hook 增删改查**，与统计零关系：

- hook 事件集合：`const CLAUDE_USAGE_EVENTS = ["Stop", "SessionEnd"];`（`src/lib/claude-config.js:7-8`）；
- 核心函数：`upsertClaudeHook`（`:10`）、`upsertClaudeUsageHooks`（`:14`）、`removeClaudeHook`（`:46`）、`areClaudeUsageHooksConfigured`（`:88`）、`buildClaudeHookCommand`（`:116`）；
- 写盘带备份：`writeClaudeSettings` 会 `fs.copyFile` 成 `${settingsPath}.bak.<ISO>`（`:252-256`）；
- 唯一与"解析"沾边的注释是泛化说明：CodeBuddy CLI 是 Claude Code fork、共用同一 hook schema（`:107-109`）。

**因此：项目归属完全不在 `claude-config.js`**，而在 `rollout.js`（B.6.1/B.6.2）与 `scan-roots.js`（根发现）。

---

### B.7 `claude-categorizer.js`（47422 字节）职责，以及它是否影响 token 统计

#### B.7.1 两个截然不同的职责

**职责一：Context Breakdown 分类视图（读侧，不影响队列）**。文件头注释逐字：

```js
// src/lib/claude-categorizer.js:2-13
// Claude Code "Context Breakdown" categorizer.
//
// Reads ~/.claude/projects/**/*.jsonl and splits each assistant message's
// usage into seven semantic buckets, mirroring (approximately) the Claude
// Code in-CLI /context view but as a historical aggregate. Computes on
// demand — no queue schema changes, no parser changes, no sync changes.
```

七个分类键（`:33-41`）：`system_prefix, conversation_history, user_input, tool_calls, subagents, reasoning, assistant_response`。输出分配靠**字符长度比例**（`splitOutputByContent`，`:367`）与"首个 cache_creation 视为 system_prefix"（`:378-380` 注释）。它由 `local-api.js:69` 引入给面板，**不写队列**。

**职责二：`computeClaudeGroundTruthBuckets`——会覆盖统计的真值修复**（`:1193`）。这一个**确实影响 token 统计**：

```js
// src/commands/sync.js:6844
async function repairClaudeQueueFromGroundTruth({
  cursors, queuePath, queueStatePath = null, projectQueuePath = null,
  projectQueueStatePath = null, rootDirs = null,
}) {
  if (!cursors || typeof cursors !== "object") return false;
  const migrations = (cursors.migrations ||= {});
  if (migrations[CLAUDE_GROUND_TRUTH_REPAIR_KEY]) return false;
```

触发条件与破坏范围：

- 只在**全量扫描**分支调用（`src/commands/sync.js:1183-1190`），且**只跑一次**（迁移键 `"claudeGroundTruthRepair_2026_05_v4"`，`src/commands/sync.js:248`）；
- **原子重写 `queue.jsonl`**：保留非 claude 行、删除所有 `source === "claude"` 与 `"claude-mem"` 行、追加真值行，`tmp + rename`（`src/commands/sync.js:6871-6921`）；
- **清空** `cursors.hourly.buckets` 与 `groupQueued` 中所有 `claude|` / `claude-mem|` 前缀键（`:6930-6943`）；
- 整体替换 `cursors.claudeHashes`（注释明说 "clears all claude buckets and replaces claudeHashes wholesale"，`:6858-6861`）。

**根被剥夺时的防护**（重要设计）：若某配置根当前不可读/不存在而历史上供过文件，则**放弃本次修复**（迁移键不写，留待后续），避免把历史抹成空（`src/commands/sync.js:1149-1182`，判定函数 `rootPreviouslySuppliedFiles`，`:1163-1166`）。

#### B.7.2 真值修复与增量解析的**口径差异**（关键发现）

| 维度 | `parseClaudeFile`（增量，日常路径） | `computeClaudeGroundTruthBuckets`（一次性修复） |
|---|---|---|
| usage 取值 | `obj?.message?.usage || obj?.usage`（`src/lib/rollout.js:2851`） | **只有** `obj?.message?.usage`（`src/lib/claude-categorizer.js:1267`） |
| reasoning 来源 | `usage.output_tokens_details.thinking_tokens ?? .reasoning_tokens`（`:4924-4926`） | `usage.reasoning_output_tokens`（`:69`）——**另一个字段名** |
| output 与 reasoning 关系 | `output_tokens = 原始 - reasoning`（拆分） | `output_tokens = 原始`（不拆），`reasoning_output_tokens` 另记 |
| `total_tokens` | `in + out(含推理) + cacheC + cacheR` | `in + cacheR + cacheC + out`（`:70-71`）——数值一致 |
| 身份折叠 | 用 `forkAliases` 折叠 user 身份（`:996-1013`） | 同样折叠（`:1214`、`:1246`），但**不看历史游标**，从零重建 |
| 未知模型行 | `DEFAULT_MODEL`（`"unknown"`） | 字面量 `"unknown"`（`:1277`），同值 |

结论：**总量口径相容**（分母相同：input + 原样 output + 两个缓存桶），但**列分布不同**——修复后 `reasoning_output_tokens` 通常变成 0，`output_tokens` 相应变大（等于把推理并回了 output）。由于 `computeRowCost` 只依赖五列之和（CLAUDE.md 明示、测试 `claude-reasoning-usage.test.js:45-47` 也断言），**费用不变**；但**"推理 token"这一展示列会在修复后归零**，且 `obj.usage` 兜底路径的数据在真值修复中会消失。

→ 列入存疑清单（这是本次研究里最具体的一处口径不一致）。

---

## C. Codex（`parseRolloutIncremental` + `codex-rollout-parser.js`）

### C.1 日志发现路径

#### C.1.1 原生根与额外根（逐字）

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

随后按"根 × (sessions | archived_sessions)"推入 sources（`src/commands/sync.js:788-810`）：

```js
// src/commands/sync.js:789
        sources.push({ source: "codex", sessionsDir: path.join(codexPaths.native, "sessions"), inventoryCacheKey: "codexDayInventoryCache" });
        if (!isBackgroundLightweightSync || backgroundCodexUsageRepair) {
          sources.push({ source: "codex", sessionsDir: path.join(codexPaths.native, "archived_sessions"), deep: true });
        }
```

要点：

- `union: true` 表示 **native + WSL 都要**（与 Claude 的 UNION 同构，但这里走 `resolveInstallPaths`）；
- `requireAnyChild: ["sessions", "archived_sessions"]` 防"空壳 `~/.codex` 抢占"（`:762-772` 注释，issue #codex-wsl-shadow）；
- `archived_sessions` 用 `deep: true` → `listRolloutFilesDeep`（`:885`），因为归档目录是扁平的（`src/lib/rollout.js:195-200` 注释，issue #187）；
- 后台轻量同步**默认不扫 archived**（除非首轮历史迁移，`:790`、`:807`）。

#### C.1.2 命名与日期

- 文件名：`rollout-<ISO 时间戳>-<uuid>.jsonl`；日期解析逐字（`src/lib/rollout.js:4192`）：

```js
// src/lib/rollout.js:4192
function rolloutDateFromPath(filePath) {
  const match = path.basename(String(filePath || "")).match(/^rollout-(\d{4}-\d{2}-\d{2})T/);
  return match ? match[1] : null;
}
```

- 会话 id 从文件名尾部 uuid 抓（`:2256-2262`）：

```js
// src/lib/rollout.js:2256
function codexSessionIdFromPath(filePath) {
  if (typeof filePath !== "string") return null;
  const m = filePath.match(
    /([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\.jsonl$/,
  );
  return m ? m[1] : null;
}
```

- 严格目录结构 `YYYY/MM/DD`（`src/lib/rollout.js:107-131`），年/月/日各自用 `^[0-9]{4}$` / `^[0-9]{2}$` 校验。

#### C.1.3 冷文件跳过（`filterColdCodexRolloutFiles`）

判定链（`src/lib/rollout.js:775-857`）：

1. 名字日期在活跃窗口内（默认今天起 2 天 + 全库最大日期，`activeCodexRolloutDates`，`:893-921`）→ 必扫；
2. `auditDue`（每天一次冷审计）→ 必扫；
3. 近 30 天内的冷文件：`fs.stat` 一次，**比游标 offset 大就扫**（`:808-822`）；
4. 否则问游标 store `canSkipCodexDay`（`:824-827`），可跳则 `skipped += 1`；
5. **cursor store 重启时整批回退**：`if (cursorStoreRestarted) { await codexCursorStore.loadCodexFilesForPaths(files, cursors); ... return { rolloutFiles: files, skipped: 0, restarted: true }; }`（`:859-869`）。

回归点：`#592`（会话跨天仍在增长）与 `#187`（归档搬迁）都在文件注释里写明（`:63-69`、`:806-807`）。

---

### C.2 事件类型与 token 数据落点

#### C.2.1 token 事件的原样结构（逐字）

提取函数（`src/lib/rollout.js:4838`）：

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

**两种信封**：`payload.type === "token_count"`（主流）与 `payload.msg.type === "token_count"`（旧/嵌套形态）。

落到桶的字段路径（`src/lib/rollout.js:2530-2554`）：

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

即：**`payload.info.last_token_usage`（本轮增量）+ `payload.info.total_token_usage`（会话累计）**，规范形状由测试 fixture 固化（`test/codex-union-cursor-divergence.test.js:79-85`）：

```js
// test/codex-union-cursor-divergence.test.js:79
function eventLine(tsSuffix, turnTokens, cumulativeTotal) {
  return JSON.stringify({
    type: "event_msg",
    timestamp: `${DATE}T00:0${tsSuffix}:00.000Z`,
    payload: { type: "token_count", info: { last_token_usage: usageOf(turnTokens), total_token_usage: usageOf(cumulativeTotal) } },
  }) + "\n";
}
```

其中 `usageOf`（`:76-78`）揭示六个桶的**原生字段名**：

```js
// test/codex-union-cursor-divergence.test.js:76
function usageOf(n) {
  return { input_tokens: n, cached_input_tokens: 0, output_tokens: 0, reasoning_output_tokens: 0, total_tokens: n };
}
```

#### C.2.2 事件类型清单（按解析路径实证）

| 事件 | 匹配方式 | 作用 | 行号 |
|---|---|---|---|
| `event_msg/token_count` | `line.includes('"token_count"')` | **唯一用量来源** | `src/lib/rollout.js:2427`、`:4838` |
| `turn_context` | 需同时含 `"model"`/`"cwd"`/`"current_date"`/`"forked_from_id"` 之一 | 推进模型、cwd、当前日期 | `:2431-2437`、`:2485-2513` |
| `session_meta` | 同上 | fork 标记（`forked_from_id`）、弱模型回退 | `:2490-2492` |
| `event_msg/thread_settings_applied` | `line.includes('"thread_settings_applied"')` | service tier（`default`/`priority`/`fast`） | `:2438-2440`、`:2479-2483` |
| `token_usage_record` | `line.includes('"token_usage_record"')` | **只用于 compaction 计费** | `:2441-2446`、`:2515-2520` |
| `compacted` | 紧随 record 的下一条完整行 | compaction 判定 | `:2410-2425` |
| `task_complete` / `turn_aborted` | `event_msg.payload.type` | 结束 record 的等待态 | `src/lib/codex-usage-record.js:82-86` |
| `model/rerouted` | 8 种信封形态枚举 | **前瞻**（真实语料 0 命中） | `src/lib/codex-model-attribution.js:33-66` |
| `response_item/function_call`、`event_msg/exec_command_end` | 仅在 breakdown 采集时 | 工具归因（不影响 token） | `src/lib/codex-rollout-parser.js:1049-1060` |

**性能设计**：先用字符串包含判断再 `JSON.parse`，且有一个兜底分支保证未匹配行只要是合法 JSON 也推进提交位（`src/lib/rollout.js:2447-2464`）：

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

注意 `break`：**遇到未闭合的不合法行会停止本轮读取**（保留可提交前缀）。

---

### C.3 增量 vs 累计：`cached_input_tokens` / `reasoning_output_tokens` 的映射与差值语义

#### C.3.1 差值引擎（`codex-token-usage.js`）

文件头注释给出问题定义（`src/lib/codex-token-usage.js:3-8`）：

```js
// src/lib/codex-token-usage.js:3
// A rollout can contain TokenCount events from more than one Codex SessionState
// (for example, a parent agent and an embedded reviewer). Each SessionState has
// its own cumulative total, but the event does not carry a stable stream id.
// Recover the stream from Codex's invariant: total - last = that stream's prior
// total. Keeping a small LRU of stream heads also makes repeated rate-limit
// snapshots idempotent when the streams are interleaved.
```

状态与上限（`:10-18`、`:78-90`）：

```js
// src/lib/codex-token-usage.js:10
const USAGE_FIELDS = [
  "input_tokens",
  "cached_input_tokens",
  "cache_creation_input_tokens",
  "output_tokens",
  "reasoning_output_tokens",
  "total_tokens",
];
const MAX_USAGE_BASELINES = 32;
```

主判决流程（`:98-158`）的四条分支：

1. **重复快照**：`total` 命中已知流头 → 返回 `null`（**不重复计**），并置 `sawInterleaved`；
2. **正常增长**：`last` 存在且 `total - last` 命中某个流头 → 返回 `last`（把 `last_token_usage` 当作本响应增量）；
3. **`total - last` 不命中任何流头但仍可减** → 置 `sawDivergentCumulative` 后返回 `last`（偏向"少算"而非虚高）；
4. **无 `last` 或流断裂** → 用 `diffUsage(total, activeHead)` 取差；若差值比 `last` 还大则判为新流（`:150-153` 注释：`A cumulative jump larger than last usage cannot come from the normal append_last_usage path`）。

`subtractUsage` 的硬约束（`:49-59`）：任一字段 `total < last` 即返回 `null`——这是"溢出/回绕"的显式拒绝，而不是饱和。

测试固化（`test/codex-token-usage.test.js`）：

| 测试 | 断言 |
|---|---|
| `:24-38` | 交错两条累计谱系时增量序列为 `[100, 200, null, 50, 30]`（第三个重复快照**不计**），且 `sawInterleaved === true`、基线长度 2 |
| `:40-48` | 单谱系正常增长 `[100, 50, 25]`，两个告警位均为 false，基线恒为 1 |
| `:50-57` | `last` 与累计不一致时（`last=150`、`total=22`）取**较小的累计差 7** |
| `:59-67` | 基线数量被截断到 `MAX_USAGE_BASELINES`（构造 37 个流头 → 32） |

#### C.3.2 归一化与字段语义（逐字）

```js
// src/lib/rollout.js:4851
function normalizeUsage(u) {
  const out = {};
  for (const k of [
    "input_tokens",
    "cached_input_tokens",
    "cache_creation_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
  ]) {
    const n = Number(u[k] || 0);
    out[k] = Number.isFinite(n) && n >= 0 ? Math.floor(n) : 0;
  }
```

随后是关键减法与解释（`:4864-4877`）：

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
  return out;
```

**映射表（Codex）**：

| 队列列 | 来源 | 变换 |
|---|---|---|
| `input_tokens` | `input_tokens` | **减 `cached_input_tokens`**（`Math.max(0, …)`） |
| `cached_input_tokens` | `cached_input_tokens` | 直取（缓存**读**） |
| `cache_creation_input_tokens` | `cache_creation_input_tokens` **或** `cache_write_input_tokens` | 直取（写法兼容见 `codex-token-usage.js:25-27`） |
| `output_tokens` | `output_tokens` | **直取，不减 reasoning** |
| `reasoning_output_tokens` | `reasoning_output_tokens` | **直取、不拆列**；它是 `output_tokens` 的**子集标记**（上游语义），计价侧据此**置 0**（`src/lib/pricing/index.js:248-256`） |
| `total_tokens` | `total_tokens` | 直取（保留上游口径，不重算） |

**"为什么 reasoning 不另计"的源码依据**：**不是本工具改写，而是上游 Codex 的原生语义 + 本仓计价侧的子集门控**。

解析侧（`src/lib/rollout.js:4851-4879`）：`normalizeUsage` 对六个桶**只做 `Math.floor` 与非负吸收，对 `reasoning_output_tokens` 没有任何算术改写**（没有 `output -= reasoning` 这类行）；注释 `:4873-4876` 逐字写着 `Preserve the reported total for compatibility with older Codex / Every Code shapes where output_tokens can exclude reasoning…`——即上游默认 **output 含 reasoning**，本工具只是原样保留。

计价侧（`src/lib/pricing/index.js:248-256`，逐字）：

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

变量名本身就是权威判据："reasoning **已含于** output"。`computeRowCost` 里 reasoning 相关的项共 3 处（`:254` 基础费、`:297-299` 长上下文溢价、`:325-327` priority 溢价），**三处全部用同一个 `reasoningIncludedInOutput` 门控**——没有一处漏掉、也没有一处反向叠加，因此不存在双计路径。

对照 Claude 侧（`src/lib/rollout.js:4930-4939`，逐字）：Claude 原生 `output_tokens` **也**包含推理，但本工具选择**显式拆出**：

```js
// src/lib/rollout.js:4934
    // Claude rows price reasoning separately; it is already included in usage.output_tokens.
    output_tokens: outputTokens - reasoningTokens,
    reasoning_output_tokens: reasoningTokens,
```

拆出后 reasoning 列**照 output 单价计费**（白名单不含 `claude`），`total_tokens` 仍用未减的 output 计算——两侧写法相反，恰好互证"不拆" = 已含在内。**统一口径**：两条主线喂给 `computeRowCost` 的都是**五列之和**（input + output + cached + cache_creation + reasoning），`total_tokens` **永不参与计费**（`CLAUDE.md:64`；测试 `test/claude-reasoning-usage.test.js:45-47` 用 `< 1e-12` 断言列分布可换、费用不变）。附带结论：**两侧没有多算，也没有少算**——Codex「不拆列 + 置 0」与 Claude「拆列 + 照价」是两套配套写法。

仓库自述（`CLAUDE.md:60-61`，逐字）：

```
reasoning_output_tokens       = reasoning tokens (Codex/acode/every-code/OmO/Cline fold them into output_tokens for cost)
total_tokens                  = input + output + cache_creation + cache_read (+ reasoning_output when output excludes reasoning); Codex/acode/every-code/OmO/Cline keep reasoning as an informational subset because it is already included in output_tokens
```

**零值重置哨兵**（`:4881-4893`）：Codex 偶发写入"五分量全 0、仅 `total_tokens` 非 0"的伪事件，被显式拒绝（在归一化之前判定，避免污染 `input - cached`）：

```js
// src/lib/rollout.js:4881
function isCodexTotalOnlyResetSentinel(lastUsage, totalUsage) {
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
}
```

用法在 `src/lib/rollout.js:2550-2553`：仅在 `source === "codex"` 时启用。

**会话计数（`conversation_count`）在 Codex 侧恒为 1/条**：每个通过零值闸门并被接受的 delta 都写 `delta.conversation_count = 1`（`src/lib/rollout.js:2554`，紧接 `:2553` 的 `isAllZeroUsage` 过滤）。这与 Claude 主线**语义不同**——Claude 的 assistant 行恒置 0、只有主会话 user 文本行 +1（`:2865`、`:2807-2831`，见 §B.2.5）。对照口径：Codex 的 `conversation_count` 累加值实为「被接受的 token 事件数」，Claude 的才是「用户轮次数」；两条主线在桶里用同一列名、求和方式相同，但**含义不可互换**。

#### C.3.3 compaction 计费（`token_usage_record`）

设计动机逐字（`src/lib/codex-usage-record.js:10-15`）：

```js
// src/lib/codex-usage-record.js:10
// Compaction. Codex bills the compaction model call in a record that is
// immediately followed by a top-level `compacted` line, and the next
// token_count repeats the previous cumulative total (its last_token_usage is
// the post-compaction context size, not a response). The cumulative delta is
// zero, so without the record the call is lost. Inside a file that has
// already shown a token_count, such a record is held as `compaction` and
// counted when the next token_count arrives with a total identical to the
// previous one.
```

- 计数条件：`compacted` 紧邻 + 下一个 `token_count` 的 `total_token_usage` 与上一次**完全相等**（`sameUsage(previousTotal, totalUsage)`，`:150`）；
- 去重键是 `payload.response_id`，**跨 fork 重放与文件复制稳定**（`:19-21`），上限 20000（`:60`）；
- 无 `response_id` 的 record **永不计**（`:20-21`、`src/lib/rollout.js:2356`）；
- 未被任何 token_count 匹配的文件会被标 `recordOnly` 并保留在 `cursors.codexUsageRecordOnlyFiles` 供 sync 告警（`src/lib/rollout.js:649-655`；`codex-usage-record.js:174-177`）；
- `payload.turn_token_usage` / `thread_token_usage` **从不读取**（`:31-32`）。

---

### C.4 去重：重复推送、`total` vs `last` 的选择、两个测试

#### C.4.1 三层去重

| 层 | 键/判据 | 代码 |
|---|---|---|
| 差值层 | `total_token_usage` 未变 → 返回 `null` | `src/lib/codex-token-usage.js:109-119` |
| 事件层 | `sessionUUID:eventTimestamp` | `src/lib/rollout.js:2626` `const dedupKey = \`${sessionId || filePath}:${tokenTimestamp}\`;` |
| compaction 层 | `payload.response_id` | `src/lib/codex-usage-record.js:60-80` |

事件层的注释（`src/lib/rollout.js:2608-2618`）说明了它存在的唯一理由：inode 变化导致**从 0 重扫**，所有 delta 会重新加进**持久化桶**；外部工具（Codex-Manager 切换渠道）用原子重写改了 inode，没有这层就会每次切换都翻倍。

两级事件追踪器（`:302-380`）：

- `getAppendOnlyEvents()`：只知道**本次运行新写**的键 → 稳态快路径；
- `getHistoricalEvents()`：并入**持久化**集合（或分片 store）→ 重扫/换路径/union 时使用；
- 选择器 `needsHistoricalRolloutDedup()`（`:428-461`）的判定顺序**至关重要**（测试专门锁定了这一点）：

```js
// src/lib/rollout.js:439-460（节选）
    // Same session under another cursor path — the two roots of a `union`
    // install (src/lib/install-resolver.js) reaching one file, or a
    // sessions/ -> archived_sessions/ move. This MUST be decided before the
    // steady-state short-circuit below: the append-only tracker only knows keys
    // written during the current run, ...
    if (sameInode && !truncated && startOffset > 0) return false;
    if (prev?.lastTotal) return true;
    return false;
```

#### C.4.2 `total` 与 `last` 的选择顺序（总结）

1. `total_token_usage` **非空对象**时刷新 `latestTotal`（`:2547`），它是**文件级权威累计**；
2. 增量取 `consumeUsageDelta(state, lastUsage, totalUsage)`；
3. 若该事件推进了累计谱系但被判为重复，返回 `null` 且**游标/基线仍已推进**（注释 `:2570-2571`：`usageDeltaState is already advanced above`）——保证后续行的血缘正确。

测试固化的语义（`test/rollout-parser.test.js:104-158`）：

- 4 条 token_count，其中第 2、4 条是**同一累计值的重复推送**；
- 断言 `eventsAggregated === 2`、`bucketsQueued === 1`、`model === "unknown"`、队列合计 = `usage1 + usage2` 的 `total_tokens` 之和。
- 注释逐字：`The cumulative total_token_usage value is authoritative for a file; if it did not move, the repeated last_token_usage must not be counted again.`（`test/rollout-parser.test.js:105-107`）

#### C.4.3 fork 重放抑制（与去重相邻的另一条防线）

两重机制（`src/lib/rollout.js:2556-2603`）：

1. **跨日日期守卫**：`isForkedReplayToken({isForkedRollout, rolloutDate, currentDate})`（`:4212`）→ `currentDate < rolloutDate` 视为重放；
2. **同日子毫秒爆发检测**：`FORK_REPLAY_GAP_MS = 500`（`:4210`），只跳过**起始连续密集段**，首次出现 >500ms 间隔即**永久关闭**（`:2588-2590`）。

阈值选择依据（`:4203-4209` 注释）：重放 flush 行距 sub-ms~几 ms，真实轮次 ≥~4.6s，重放→真实断点 ≥11s；500ms 偏向"宁可少算也不多算"。首行无法前视，故仍有 <1% 的残余over-count（`:2567-2569`）。

#### C.4.4 `codex-union-cursor-divergence.test.js` 断言了什么

测试头注释逐字（`test/codex-union-cursor-divergence.test.js:2-22`）点明了缺陷与修法：

```js
// test/codex-union-cursor-divergence.test.js:13
 * The persisted `codexHashes` set exists to make exactly that idempotent, but
 * `needsHistoricalCodexDedup` short-circuited on `startOffset > 0` BEFORE
 * reaching its own same-session-under-another-path check, so the append-only
 * tracker (which only knows keys written during the current run) was used
 * instead. When the up-to-date root has nothing new to read it contributes no
 * keys at all, and the lagging root's replay lands in the persisted buckets and
 * the queue — and from there in the cloud totals.
```

两个用例都是 **e2e 级**：真跑 `cmdSync([], {})`，读真实 `queue.jsonl` 后按 `source|model|hour_start` 取最后一条再求和（`:104-112`）：

| 用例 | 场景 | 断言 |
|---|---|---|
| `:118-153` | native + WSL 两个根指向同一文件；run1 累计 120 双根可见；run2 增长到 220 时 WSL 临时不可见；run3 WSL 恢复且磁盘无变化 | 120 → 220 → **必须仍是 220**（滞后根不得重放） |
| `:155-189` | 同上，但用 `TOKENTRACKER_WSL_MODE=native-only` 制造分歧再恢复 | 同上 |

提炼：**union 安装下"同一会话文件在两条路径各有游标"，必须靠持久化事件集兜底**；这也是 `sync.js` 传 `codexEventStore` 的意义（`src/commands/sync.js:1049-1051`）。

---

### C.5 多 provider 与模型名归属

#### C.5.1 模型归属状态机（`codex-model-attribution.js`）

优先级与理由逐字（`src/lib/codex-model-attribution.js:3-23`）：

```js
// src/lib/codex-model-attribution.js:3
// Codex token_count events do not carry a model id. Every usage event is
// therefore attributed to the model named by the most recent turn_context,
// which Codex emits at the start of each turn and which tracks mid-session
// model switches exactly. That per-turn path is what actually runs today.
//
// The model/rerouted upgrade below is forward-looking: ...
//
// Deliberately NOT used for attribution: event_msg/thread_settings_applied.
// It carries thread_settings.model, but it fires when the user picks a model
// in the UI - seconds to minutes before the switch takes effect - so usage
// still streaming from the in-flight turn belongs to the previous model.
// turn_context is the only signal that marks where a turn actually begins.
//
// session_meta is a defensive fallback, not an observed source. ...
```

- `turn_context.payload.model` → `selectedModel`，并清 `rerouted`（`:82-92`）；
- `session_meta.payload.model` **只在 `selectedModel` 为空时**采纳，且**永不覆盖** turn_context（`:96-102`，注释指出 fork 会重放父级 session_meta）；
- `thread_settings_applied` **被刻意排除在模型归属之外**（`:11-15`），但它承载 **service tier**（另一模块，见 C.5.3）；
- 实测量级（注释给出）：`0 of 10310 session_meta rows across 5849 local rollouts carry the field (codex-cli 0.151.0)`（`:18-20`）。

调用点（`src/lib/rollout.js:2476-2477`）：

```js
// src/lib/rollout.js:2476
    applyCodexModelEvent(modelAttributionState, obj);
    model = currentCodexModel(modelAttributionState) || model;
```

模型状态**持久化进文件游标**（`modelAttributionState`，`:639`），跨增量续用；初始值由 `lastModel` 注入（`:2320-2322`）。

#### C.5.2 provider / base_url：**不参与用量归属**

- 队列行**没有 provider 列**（见 A.4.2 逐字行格式：`source, model, hour_start, 六桶, total, billable, cost, conversation_count`）；
- `source` 常量对 Codex 家族固定为 `"codex"` / `"acode"` / `"every-code"`，由 `sync.js` 在组装文件条目时决定（`src/commands/sync.js:894`、`:789`）；
- `session_meta.payload.model_provider`（如 `"openai"`）在**用量路径**上完全未被读取——`codex-model-attribution.js:19-23` 明确称其为 `provenance rather than a model`；
- 使用 `model_provider` 的是**会话浏览器/breakdown** 侧：`src/lib/codex-rollout-parser.js:1023`（`provider = p.model_provider || provider;`）与 `src/lib/session-analytics.js:822-834`，其中还有一条重要防护逐字：

```js
// src/lib/session-analytics.js:822
  // Older Codex rollouts can omit turn_context.model. The shared parser then
  // falls back to model_provider (for example "openai"), which is provenance
  // rather than a model and must not become a model-table row.
```

- 全库对 `azure` 的检索在 `src/lib/` 下**零命中**；`base_url` 命中都属网络/遥测配置（`src/lib/doctor.js:62`、`src/lib/runtime-config.js:78`），与 Codex 日志解析无关。

**结论**：第三方 base_url / Azure 路由的 Codex 会话，其用量**按 `turn_context` 声明的模型串原样落进 `codex|<model串>|<hour>`**，没有任何 provider 维度参与身份或去重；同模型的跨渠道请求会合并到同一桶。这是与"能区分不同渠道"的预期相反的明确事实。

#### C.5.3 service tier（Astra Fast / priority）

```js
// src/lib/codex-service-tier.js:17
// Cheap line prefilter for parsers that avoid JSON.parse on most records.
const CODEX_SERVICE_TIER_MARKER = '"thread_settings_applied"';

// Observed values on the local corpus are "default" (814) and "priority" (58).
// "priority" is what the CLI writes for what OpenAI's price table and the Codex
// UI both call Fast; accept the product-facing spelling too so a future rename
// does not silently fall back to Standard.
const PRIORITY_SERVICE_TIERS = new Set(["priority", "fast"]);
```

- 归属规则：`a token_count row belongs to the most recent preceding thread_settings_applied`（`:12-13`）；无前置记录即 `null`，`null` **按 Standard 计**，从不猜（`:12-15`）；
- 覆盖度事实（**上游源码注释里的语料观测**，非本次实测）：1213 个本地文件里只有 167 个带该事件，覆盖 25% 的 token_count（`:8-10`）；同类观测还有 `:14` 的 `"default" (814) / "priority" (58)`。【缺什么才能判定】这些数字是维护者对**其本机语料**的统计，无法从源码判定真伪，也不代表任意用户环境的覆盖率；要判定需在本机 `~/.codex/sessions/**` 上重跑同样的计数（分母 = 含 `token_count` 的文件数，分子 = 含 `thread_settings_applied` 的文件数）。**代码侧的规则与结构**（归属规则、`null` 按 Standard、子集列名与门控）本次逐行核对通过，因此本条只影响"priority 覆盖率"这一句的可引用性；
- 落桶是**子集标记**而非独立桶（`src/lib/rollout.js:4073-4088`）：

```js
// src/lib/rollout.js:4073
const PRIORITY_SUBSET_FIELDS = [
  ["priority_input_tokens", "input_tokens"],
  ["priority_cached_input_tokens", "cached_input_tokens"],
  ["priority_cache_creation_input_tokens", "cache_creation_input_tokens"],
  ["priority_output_tokens", "output_tokens"],
  ["priority_reasoning_output_tokens", "reasoning_output_tokens"],
];
```

注释强调（`:4067-4072`）：这些列**永不参与任何求和**，`total_tokens` 仍等于五桶之和；为零时整列省略。

---

### C.6 `codex-rollout-parser.js`（52451 字节）与 `codex-context-breakdown.js`（27929 字节）

#### C.6.1 是否影响统计：**默认不影响**

证据链：

1. 两者对 `queuePath` / `appendFile` / `enqueueTouchedBuckets` 的检索**零命中**（本次逐个检索 `src/lib/codex-context-breakdown.js`、`src/lib/codex-rollout-parser.js`）。
2. `codex-rollout-parser.js` 头部逐字声明无聚合状态：

```js
// src/lib/codex-rollout-parser.js:1-4
// Codex rollout JSONL parser — extracted from codex-context-breakdown.js.
//
// Handles file discovery and per-file parsing. Does NOT hold any aggregation
// state; callers (computeCodexContextBreakdown) own the merge step.
```

3. `codex-context-breakdown.js` 头部逐字声明用途与隐私边界：

```js
// src/lib/codex-context-breakdown.js:1-9
// Codex CLI "Context Breakdown" — tool-oriented view.
//
// Privacy commitment: tokens + timestamps only. We do not return prompt text,
// assistant text, tool outputs, file contents, or exec_command arguments.
//
// Data source: ~/.codex/sessions/**/rollout-*.jsonl
// We treat each token_count event as the authoritative delta and attribute
// that delta to "turn" activity since the last token_count. Tool attribution
// is heuristic: delta is split evenly across tools used in that turn.
```

4. 消费者是只读视图：`src/lib/local-api.js:71`（`computeCodexContextBreakdown`）、`src/lib/session-analytics.js:32`（`parseCodexRolloutFile`）。

**已闭合**：两模块对 `queuePath` / `appendFile` / `enqueueTouchedBuckets` 的检索确为零命中，"**不写队列**"成立；这条从源码即可判定。

【判定缺口】"**不影响 token 统计**"这一更强结论依赖一个前提：`sync.js:198` 借用其 `extractTokenCount` 的那条路径是**一次性迁移，不是日常解析**（C.6.3：它会原子重建 `queue.jsonl`）。代码层判定不了的是"**该修复在某些安装上是否永不触发**"——这取决于 `cursors.migrations[CLAUDE_GROUND_TRUTH_REPAIR_KEY]` 的既有状态与文件是否被判为受影响（运行时状态，不在源码里）。要判定：读一个真实 `~/.tokenscope/cursors.json`，看该迁移键是否已置位；置位则本条的影响面为零。

#### C.6.2 但它**复用同一套核心状态机**——这是重要的一致性事实

`codex-rollout-parser.js` 直接 import 主路径的四个模块（`:13-42`）：`codex-token-usage`（差值）、`codex-model-attribution`（模型）、`codex-service-tier`（档位）、`codex-usage-record`（compaction）。也就是说**语义只有一份实现**，视图与统计不会各算各的。`src/lib/codex-rollout-parser.js:999-1008` 逐字说明了共享方式与理由：

```js
// src/lib/codex-rollout-parser.js:999
    const modelEvent = applyCodexModelEvent(modelAttributionState, obj);
    const attributedModel = currentCodexModel(modelAttributionState);
    model = attributedModel || model;
    // onObject runs AFTER the attribution state machine, and is handed the
    // model it just decided. A consumer sharing this pass (the delivery-signal
    // collector, which attributes edit turns) therefore sees exactly the model
    // recordModelUsage() bills the turn's tokens to. ...
```

#### C.6.3 唯一例外：血缘修复借用 `extractTokenCount`

`src/commands/sync.js:198` 从该模块引入 `extractTokenCount`，用于 codex 一次性修复的**受影响判定**（`:6233-6252`）：

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

该修复本身会**重写历史桶**（原子重建到临时 queue，成功后才替换；失败则完全不动，`src/commands/sync.js:5750-5804`），所以它是"影响统计"的——但它是**迁移**，不是日常解析路径。

---

## 存疑与待复核点

按"影响可控性"从高到低排列，每条给出可执行的复核动作。

1. **Claude 增量游标无行边界提交**（`src/lib/rollout.js:2793`、`:2889`）。`const endOffset = st.size;` 共 3 处（`:2291`、`:2682`、`:2793`），**只有 `:2793` 落到 token 游标**（`:1194` 的 `cursors.files[key].offset = result.endOffset`）；`:2291` 是"无新字节"早退，`:2682` 是项目上下文扫描且返回 `committedEndOffset`（`:2765`）。Claude 侧若同步瞬间读到半行，`readline` 会 yield 未闭合尾行、`JSON.parse` 失败被吞（`:2843-2847`），游标仍推进到文件尾，该行字节被跳过。**非对称性**：Codex 的 token 路径用 `committedEndOffset`（`:2652`），**连项目上下文扫描位也做了同样的提交保护**（`:2765`），OpenClaw 另有 `openclawOffsetEndsAtLineBoundary`（`:1949`）——只有 Claude 一条路径完全没有。
   补充（收窄"永久"）：该跳过**在该文件下一次 inode 变化或截断时会被全量重扫抵消**（`:488-504` 的三态判定，重置 `startOffset=0`），因此准确说法是"在本代文件内不可恢复"，而非严格永久。
   复核动作：构造"写入半行 JSONL → 跑 `parseClaudeIncremental` → 补全该行 → 再跑"的定向测试，看补全后的那行是否被计入。若未计入，即为确定性数据丢失 bug（不是理论风险）。

2. **Claude 真值修复与增量解析的口径不一致**（B.7.2 表格）。差异至少三处：usage 取值路径（`obj.message.usage || obj.usage` vs 仅 `obj.message.usage`）、reasoning 字段名（`output_tokens_details.thinking_tokens` vs `usage.reasoning_output_tokens`）、output 是否拆分。修复是**一次性且不可逆**（重写 `queue.jsonl` + `cursors.claudeHashes` 全替换，`src/commands/sync.js:6871-6921`）。
   复核动作：拿同一份合成 fixture 跑两条路径，逐列比对（不只是 total）。预期 total 相同、`output_tokens` 与 `reasoning_output_tokens` 不同。

3. **Claude 缓存字段命名未兼容 `cache_write_input_tokens`**（`src/lib/rollout.js:4927-4928` vs `src/lib/codex-token-usage.js:25-27`）。anthropic 兼容端点（Kimi/DeepSeek/Mimo/MiniMax）若用别名，缓存写会被静默丢为 0。
   复核动作：查这些 provider 的合成 fixture 或真实样本中 `cache_creation_input_tokens` 的出现率；若为 0 而 `cache_write_input_tokens` 非 0，即为确定性丢量。

4. **`claudeHashes` 上限 100k 的淘汰窗口**（`src/lib/rollout.js:1236-1237`）。超限时保留**最后** 100k；注释（`:1021-1025`）自己承认"message-hash 层有上限，淘汰会让批量历史双计"，因此加了文件级 `fileId` 去重作为第二层。但 `fileId` 只在"同一次运行内看到 `\\wsl$` 与本地路径混杂"时启用（`sawUncFile && sawLocalFile`，`:1035`）。
   复核动作：构造 >100k 哈希 + 同内容双路径的用例，验证是否双计。

5. **Codex 主路径无单行字节上限**（A.2.2）。`physicalJsonlRecords` 支持 `maxRecordBytes`，但 `parseRolloutFile` 只传 `invalidUtf8`（`src/lib/rollout.js:2397`）；有此保护的只有 OMP 族（`:15717`）。
   复核动作：确认这是"有意不设限"还是遗漏；若某 rollout 含超大 `response_item`，会整行读进内存。

6. **`<synthetic>` 无显式处理**（B.4.2）。真实 Claude 日志是否会给该模型名配非零 usage，本次未验证；若会，则面板会出现一个名为 `<synthetic>` 的模型行。
   复核动作：在本机 `~/.claude/projects/**` 里 `grep '"model":"<synthetic>"'` 并看同行 usage 是否非零。

7. **`parseClaudeScienceIncremental` 的字段语义未与主线对齐**。`reasoning_output_tokens` 恒为 0（`src/lib/rollout.js:7023`），`input` 的"含缓存"前提（`:6995-7001` 注释）来自该产品的私有约定，本次无真实 DB 样本可验证。
   复核动作：若本机有 `~/.claude-science/operon-cli.db`，抽样比对 `frames` 表列值与 UI 显示。

8. **`usage_precision` 语义未展开**。它作为可选列进入队列行（`src/lib/rollout.js:3325-3326`、`:3341`），并在 `queuedKey` 中参与比对；本次未追踪其来源与消费方（推测与某些 provider 的"估算值"标记有关）。
   复核动作：`grep -rn "usage_precision" src/ test/` 追全部写入与读取点。

9. **`mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal` 而非"最后一条"**（`src/commands/sync.js:6540`）。与 `readQueueData`（最后一条胜出，`src/lib/local-api.js:207`）语义不同，两者是否会给出不同结果，取决于队列是否可能出现"同键数值回退"（例如真值修复删行追加后的旧值仍在文件中——但修复是原子重写，理论上不会）。
   复核动作：确认该函数的所有调用点是否都作用于"已被重写过的干净队列"。

10. **Codex 模型串未归一化，跨渠道同模型合并**（C.5.2）。若用户经多 provider 路由同一模型，用量会合并进同一个 `codex|<model>|<hour>` 桶，且**没有 provider 维度可供拆分**。
    复核动作：确认产品是否需要该维度；若需要，需在事件层新增字段（当前 schema 无位可放）。

11. **`model/rerouted` 归属路径从未在真实语料验证**（`src/lib/codex-model-attribution.js:47-51` 自述 `0` 命中）。该分支一旦被真实触发，其信封形状与字段名（`fromModel/toModel/from_model/to_model`）都属推断。
    复核动作：检索本机 rollout 中是否出现 `model/rerouted`/`model_rerouted`。

12. **半小时桶算法有两份实现且无等价性锁定**（A.5.2）。`src/lib/rollout.js:4173` 与 `src/lib/claude-categorizer.js:1175` **语义等价、文本不同**（前者分步 `const bucketStart = …` + `return bucketStart.toISOString()`，后者单行链式 `return new Date(…)…).toISOString()`），改动任一处不会让另一处失败；`test/` 下引用 `toUtcHalfHourStart` 的两个测试都指向 `rollout.js` 那份，`claude-categorizer.js` 的副本**无人测**。真值修复与增量解析一旦分叉，队列会出现两套时间分桶。
    复核动作：加一条"两份输出对同一批时间戳完全相等"的回归测试（从两个模块各取一份实现对比，而不是只测其中一份）。

13. **未复核区**：`src/lib/cursor-store.js`（1918 行）的分片完整性校验、代际（generations）轮换与 `codexEventStore` 的失败注入路径，本次只读了常量与函数清单；若 S2 后续需要"游标损坏后的恢复语义"，须单独一轮。

14. **Codex 侧的"reasoning 不双计"没有对等回归测试**。`test/claude-reasoning-usage.test.js:45-47` 只用 `< 1e-12` 锁定了 Claude 的"列分布可换、费用不变"；Codex 路径依赖 `src/lib/pricing/index.js:248-256` 的 `source` 白名单硬编码（`codex`/`acode`/`every-code`/`omo`/`cline`），一旦上游新增一个 `*-code` 变体而白名单漏加，就会**静默双计**每一条 reasoning token，且现有测试全绿。
    复核动作：补一条对等断言——`computeRowCost({ source: "codex", output_tokens: 518, reasoning_output_tokens: 163 })` 与同参数但 `reasoning_output_tokens: 0` 的结果差 `< 1e-12`；并考虑把白名单改为按 `source.endsWith("-code") || source === "codex"` 之类的结构性判定。

---

## 附：本次研究的证据索引（可直接定位）

| 主题 | 入口 |
|---|---|
| 45 个 parser 清单 | `src/lib/rollout.js:251`…`:24458`（表见 A.1.2） |
| 导出表 | `src/lib/rollout.js:24675` |
| 半小时桶 | `src/lib/rollout.js:4173`；副本 `src/lib/claude-categorizer.js:1175` |
| 桶与队列写入 | `src/lib/rollout.js:3169`、`:3489` |
| 桶结构 | `src/lib/rollout.js:4053`（`initTotals`）、`:3996`（`getHourlyBucket`） |
| 队列读取口径 | `src/lib/local-api.js:202`、`:207`、`:213`；`src/commands/sync.js:4510`、`:6540` |
| JSONL 行读取 | `src/lib/jsonl-lines.js:29` |
| Codex 主解析 | `src/lib/rollout.js:251`（入口）、`:2264`（单文件）、`:4192`（文件名日期） |
| Codex 差值 | `src/lib/codex-token-usage.js:98` |
| Codex compaction | `src/lib/codex-usage-record.js:1-35` |
| Codex 模型归属 | `src/lib/codex-model-attribution.js:3-23` |
| Codex 档位 | `src/lib/codex-service-tier.js:1-42` |
| Claude 主解析 | `src/lib/rollout.js:954`（入口）、`:2776`（单文件）、`:4921`（usage 归一） |
| Claude 去重与身份 | `src/lib/rollout.js:4914`；`src/lib/claude-user-identity.js:13` |
| Claude 真值修复 | `src/lib/claude-categorizer.js:1193`；`src/commands/sync.js:6844` |
| 测试 | `test/rollout-parser.test.js:104`、`test/codex-token-usage.test.js:24`、`test/codex-union-cursor-divergence.test.js:118`、`test/claude-fork-conversations.test.js:35`、`test/claude-reasoning-usage.test.js:11` |

---

## 修正记录（R1 复核后）

依据 = `REVIEW-R1-B-claude-codex.md` 的 E/I/U 三组 ＋ 本次对样本仓库重跑的实测（45 个 `parse*Incremental` 的顶层 return 键名全量提取、`endOffset = st.size` 全量定位、计价门控逐行核对）。样本仓库**未做任何修改**。行号口径与正文一致。

| 编号 | 原表述 | 修正后 | 依据 |
|---|---|---|---|
| E1 | 「45 个 parser 存在**三族**签名」 | 「**无统一契约**：45 个声明中 5 个转发薄壳、40 个自持 return、去重后 **11 种键集合**」，并给出完整清单与逐字证据 | `rollout.js:1239`、`:6027`、`:7137`、`:16084`、`:18111`、`:19003-19013`、`:19538`、`:22175`、`:22791`、`:24029`、`:24664`；本次重跑：45 声明 / 40 顶层字面量 / 11 键集合 |
| E1-a | 复核清单中的 `parseCursorApiIncremental`(+`total`)、`parseTraeCnApiIncremental`(+`total`)、`parseGrokBuildIncremental`(「键重复」) | 三处均为**值标识符被当成键名**：实际写法是 `recordsProcessed: total`（`:7270`、`:22791`）与 `recordsProcessed: eventsAggregated`（`:20551`，疑似上游把记录数写成事件数）。它们的键集合分别是骨架 C、骨架 C+`skippedRows`、骨架 D，**没有多出的键、也没有重复键** | `rollout.js:7270`、`:22791`、`:20551` 逐字 |
| E1-b | 复核「**12 种**返回字面量」 | **11 种**：复核漏掉了 `parseCopilotSessionStoreIncremental` 的 9 键集合，同时把 3 个值标识符计成了键 | `rollout.js:19003-19013` 逐字 |
| E1-c | §0 第 1 条「`rollout.js` 共 **23260 行**」 | 实测 **24932 行**（`ReadAllLines(UTF8).Count`）；字节数 `942266` 无误。复核报告自身也测到 24932，却判"分片 §0 数据一致"，属漏检 | `rollout.js` 全文行数实测；对照 `REVIEW-R1-B-claude-codex.md:151` |
| I1 | §0 第 6 条「**reasoning 不被并入 output**，而是独立成列」 | 改为两侧各自的准确表述：Codex 的 reasoning 是 `output_tokens` 的**子集标记**（解析不拆列 `:4851-4879`、计价**置 0** `pricing/index.js:248-256`）；Claude 的 reasoning 是**独立加成项**（解析**显式拆出** `rollout.js:4930-4939`、计价**照 output 价**）。两者在 `computeRowCost` 里都以**五列之和**为输入，**`total_tokens` 永不参与计费** | `rollout.js:4851-4879`、`:4930-4939`；`pricing/index.js:248-256`、`:297-299`、`:325-327`；`CLAUDE.md:60-64` |
| I2 | §0 第 8 条「Claude 的 `endOffset` 直接取 `st.size`」（暗示仅 1 处） | 写清 **3 处分工**：`:2291`（`parseRolloutFile` 的"无新字节"早退，返回值不可能大于入参 `startOffset`，不计 token）、`:2682`（`scanRolloutProjectFileContexts`，返回 `{ endOffset: committedEndOffset, … }`，`:2765`）、`:2793`（**唯一**落到 token 游标的站点，`:1194`）；并写明非对称性——**Codex 连项目扫描都做了提交位保护** | `rollout.js:2291`、`:2294-2302`、`:2652`、`:2666-2682`、`:2764-2765`、`:2793`、`:2889`、`:1194` |
| I2-附 | 复核称「§C 第 10 条误引 `:2682`」 | 该误引**不成立**：R1 复核所指的「原始引用」在修正**前**的分片正文中并不存在（R2-1 复核确认）。修正后分片内 `2682` 共 **7 处**命中（L19、L451、L456、L1740、L1818、L1819、L1832），**全部出自本轮按 I2 补入的「3 处分工」内容与其修正记录自身**，无一为误引。故未删任何引用，只补了 3 处分工表；原「全文检索 `2682` 在本分片零命中」的写法会被修正后的文件自身证伪，以本行为准 | `REVIEW-R2-1-pipeline-framework.md` E1；7 处命中逐行核对均为本轮新增；`rollout.js:2652` 逐字 |
| I3 | §A.3.3 / 存疑 1「该行字节**永久跳过**」 | 收窄为"**本代文件内不可恢复**"：该文件下次 inode 变化或截断会走重置路径全量重扫（`startOffset=0`） | `rollout.js:488-504`、`:2843-2847` |
| I4 | （缺项） | 存疑新增第 **14** 条：Codex 侧"reasoning 不双计"无对等回归测试，`source` 白名单漏加会**静默双计** | `pricing/index.js:248-256`；对照 `test/claude-reasoning-usage.test.js:45-47` |
| I5 | §A.2.3「【未能确认】队列层坏行计数是否影响用量」 | **闭合**：`countMalformedCodexProjectQueueRows` 全库只有一个调用点，只决定"是否执行修复"，不产生用量 | `sync.js:6436`（定义）、`:5824-5830`（唯一调用点） |
| I6 | §A.3.2 游标字段表把项目字段写成无条件 | 加前提：Claude 的 `claudeCwd` / `projectFileContext` / `projectRef` / `projectKey` 只在 `projectEnabled` 分支写入；Codex 的 `projectOffset` / `projectFileContext` 受 `codexProjectFastPath` 控制 | `rollout.js:1198-1205`、`:656-665` |
| I7 | §A.5.2「两份实现的**源码逐字一致**」 | 改为「**语义等价、文本不同**」（单行链式 `return new Date(…).toISOString()` vs 分步 `const bucketStart` + `return`）；保留成立的那半句「副本无测试锁定」（两个测试都 `require("../src/lib/rollout")`） | `rollout.js:4173-4190` vs `claude-categorizer.js:1175-1191`；`test/sync-droid-dup-session-repair.test.js:29`、`test/trae-entitlement.test.js:29`、`:130-137` |
| I8 | §A.3.3 注释引用 `:1114-1120` | 改为 `:1115-1120`（`:1114` 是空注释分隔行 `//`） | `rollout.js:1114-1120` 逐字 |
| U1 | §B.2.3「跨 provider … 会静默丢缓存写」 | 保留为**条件性风险**并写明判定缺口：需要 Claude 会话行里 `cache_write_input_tokens` 的出现率统计（出现且 `cache_creation_input_tokens` 缺失才成立） | `rollout.js:4927-4928`、`codex-token-usage.js:25-27` |
| U2 | §B.4.2「全库**唯一**出现 `<synthetic>` 的地方是 `:101`」 | 补齐为 **3 处**（`:101` 集合、`:621-624` 注释、`:1968-1971` 注释），并写明判定所需样本（本机 `~/.claude/projects/**` 里 `"model":"<synthetic>"` 行是否带非零 usage） | `session-analytics.js:101`、`:621-626`、`:1968-1971` |
| U3 | §C.5.3 语料统计（1213 文件 / 167 命中 / 25%；814 default / 58 priority） | 标注为"**上游源码注释里的本机语料观测**，非本次实测"，并写明重跑口径（分母=含 `token_count` 的文件数）；代码侧规则与结构仍判为已核对 | `codex-service-tier.js:8-15` |
| U4 | §C.6.1「不影响 token 统计」 | 拆成两段：`不写队列` **已闭合**（检索零命中）；`不影响统计` 依赖"`sync.js:198` 借用属一次性迁移"这一前提，真正缺的是"该修复是否在某些安装上**永不触发**"（运行时游标状态） | `local-api.js:71`、`session-analytics.js:32`、`sync.js:198`、`:6844-6854` |

**未采纳**：0 条（E/I/U 全部按复核结论或其纠正后的版本落实）。
**对复核报告的 4 处纠正**：E1-a（值标识符误读）、E1-b（11 而非 12）、**E1-c（`rollout.js` 行数 24932，非 23260；复核漏检）**、I2-附（R1 所指的 §C `:2682` 误引在**修正前正文**中不存在；修正后本文件因 I2 补写而含该字串 7 处，均非误引；另复核把 `:2291` 归为"项目上下文扫描"，实测它属于 `parseRolloutFile`）。

---

### R2 修正

依据 `REVIEW-R2-1-pipeline-framework.md`（本文件相关条目：E1、I3、I4）与 `REVIEW-R2-2-providers.md` 的 C1/C2 横向核对（S2 侧判定通过、未产生需改项）。修正 agent：FIX-R2-A；改动前均回读源码逐字确认；**样本仓库未修改**；R1 已完成内容全部保留。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| R2-E1（本记录 I2-附） | 「全文检索 `2682` 在本分片**零命中**」——被修正后的文件自身证伪（现存 7 处命中） | 限定语境：R1 所指的 §C `:2682` 误引在**修正前正文**中不存在；现存 7 处命中（L19/L451/L456/L1740/L1818/L1819/L1832）**全部出自 R1 按 I2 补入的「3 处分工」内容与其修正记录自身**，均非误引。同步精确化本文末「4 处纠正」行中的同一表述 | `REVIEW-R2-1-pipeline-framework.md` E1；7 处命中逐行核对均为新增内容 |
| R2-I3（§A.3.1） | 顶层键表只列通用键与双主线专用键，未提 provider 私有命名空间（表未声明穷尽） | 表内新增一行 `cursors.<provider>` 私有命名空间（`claudeScience`、`cursorApi`、`kiro`、`kiroCli`、`traeCn` 等），并补一句明确**表不声明穷尽** | `rollout.js:7136,7170,7264-7268,7589,9261,9482,22789` |
| R2-I4（§A.4.3 / §C.3.2） | 只记了 Claude 侧 `conversation_count` 语义，Codex 侧缺位（此前唯一提及在 §B.2.4/B.2.5） | §C.3.2 补「Codex 每个被接受的 delta 恒写 `conversation_count = 1`」及其与 Claude（assistant 恒 0、主会话 user 文本行 +1）的语义差异；§A.4.3 补一句跨主线对照与交叉引用 | `rollout.js:2554`、`:2865`、`:2807-2831` |

**未采纳 / 反驳**：0 条。R2-1 对本文件的 3 条（E1、I3、I4）经回源确认全部成立并已落实；R2-2 的 45 条解析器签名表横向核对（C1/C2）判定 S2 通过，无需改动。
