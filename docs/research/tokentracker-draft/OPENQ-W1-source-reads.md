# OPENQ-W1 源码直读结项（9 项存疑）

- 样本仓库（只读，未执行任何仓库脚本）：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）。
- 行号口径：`read` 工具与 `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；关键计数另用 `Select-String -SimpleMatch` 独立复算（未使用裸 `Get-Content` 取行号）。
- 判定统计：**已解决 8 条**（1–7、9）、**部分解决 1 条**（8）、维持未确认 0 条。
- 引用范围约定：`src/…`、`test/…`、`dashboard/…` 均相对样本仓库根；证据块为原文摘录，省略处标 `…`。

---

## 1 `src/lib/fs.js` 锁实现细节（openLock / inspectLock / updateJsonLocked / `.reclaim` 闸链）

- **判定**：已解决
- **结论**：锁是 `wx`（等价 `O_CREAT|O_EXCL|O_WRONLY`）独占创建的 JSON 租约文件，内容 `{pid, token, createdAt}`；每把锁额外持有 `${lockPath}.heartbeat.<sha256(token)>` 心跳文件（同样 `wx`）与释放用的 `${lockPath}.reclaim` 过渡闸（`wx`，自带 pid/token/heartbeat，`serializeRelease:false` 不递归加闸）；陈旧回收判据三分支——owner 进程不存在即刻回收、PID 存活时只在心跳 mtime 超过 `LOCK_STALE_MS`（5 min）后回收、无 owner 记录（空/坏文件）时按锁文件 mtime 超过 5 min 回收；回收走 quarantine 原子 rename + 共享闸串行化。全部等待均为显式常量，无「永不超时」路径。
- **证据**：

`src/lib/fs.js:69-74`（超时/闸链常量全集）

```js
const LOCK_STALE_MS = 5 * 60 * 1000; // 5 minutes
const LOCK_HEARTBEAT_MS = 30 * 1000;
const MAX_RECLAIM_DEPTH = 4;
const MAX_RECLAIM_SWEEP_DEPTH = 64;
const TRANSITION_GUARD_RETRY_MS = 5;
const TRANSITION_GUARD_ATTEMPTS = 1000;
```

`src/lib/fs.js:296-307`（创建标志 + 写 pid/token + 心跳文件）

```js
    const handle = await fs.open(lockPath, "wx");
    const token = crypto.randomUUID();
    const heartbeatPath = heartbeatPathFor(lockPath, token);
    let heartbeatHandle = null;
    try {
      await handle.writeFile(
        JSON.stringify({ pid: process.pid, token, createdAt: new Date().toISOString() }) + "\n",
        "utf8",
      );
      heartbeatHandle = await fs.open(heartbeatPath, "wx");
      await heartbeatHandle.writeFile(`${token}\n`, "utf8");
      const heartbeatTimer = startLockHeartbeat(handle, heartbeatHandle);
```

`src/lib/fs.js:117-128`（陈旧判定三分支）

```js
    if (owner) {
      if (!isProcessAlive(owner.pid)) return true;
      let heartbeatHandle = null;
      try {
        heartbeatHandle = await fs.open(heartbeatPathFor(lockPath, owner.token), "r");
        const heartbeat = await heartbeatHandle.stat();
        return Date.now() - heartbeat.mtimeMs > LOCK_STALE_MS;
      } catch (_e) {
        return false;
      } finally {
        await heartbeatHandle?.close().catch(() => {});
      }
```

`src/lib/fs.js:136`（无 owner 记录的 fallback：按锁文件 mtime）

```js
    return Date.now() - stat.mtimeMs > LOCK_STALE_MS;
```

`src/lib/fs.js:337-346`（回收深度上限 + 回收同样走 `.reclaim` 闸）

```js
        if (reclaimDepth >= MAX_RECLAIM_DEPTH) return null;
        const reclaimGuard = await openLock(`${lockPath}.reclaim`, {
          quietIfLocked: true,
          reclaimDepth: reclaimDepth + 1,
          serializeRelease: false,
        });
```

`src/lib/fs.js:190`（闸链路径逐层追加）

```js
    guardPath = `${guardPath}.reclaim`;
```

`src/lib/fs.js:200-202`（最深优先清扫，仅回收已死的 owner）

```js
  for (let index = chain.length - 1; index >= 0; index -= 1) {
    if (await removeAbandonedLockFile(chain[index])) removed += 1;
  }
```

`src/lib/fs.js:244-254`（释放侧先取闸，5 ms × 1000 次 ≈ 5 s 上限）

```js
    for (let attempt = 0; attempt < TRANSITION_GUARD_ATTEMPTS; attempt += 1) {
      transitionGuard = await openLock(`${lockPath}.reclaim`, {
        quietIfLocked: true,
        reclaimDepth: reclaimDepth + 1,
        serializeRelease: false,
      });
      if (transitionGuard) break;
      await new Promise((resolve) => setTimeout(resolve, TRANSITION_GUARD_RETRY_MS));
    }
```

`src/lib/fs.js:146-157`（`inspectLock` 三态：不存在 / 存在但无 owner / 有 owner 且存活）

```js
async function inspectLock(lockPath) {
  let raw = null;
  try {
    raw = await fs.readFile(lockPath, "utf8");
  } catch (e) {
    if (e?.code === "ENOENT") return { exists: false, pid: null, alive: false };
    return { exists: true, pid: null, alive: true };
  }
  const owner = parseLockOwner(raw);
  if (!owner) return { exists: true, pid: null, alive: true };
  return { exists: true, pid: owner.pid, alive: isProcessAlive(owner.pid) };
}
```

`src/lib/fs.js:419-431`（`updateJsonLocked` 唯一超时：默认 30 s，10 ms 轮询）

```js
  { timeoutMs = 30_000, retryMs = 10 } = {},
) {
  const lockPath = `${filePath}.lock`;
  const deadline = Date.now() + timeoutMs;
  let lock = null;
  while (!lock) {
    lock = await openLock(lockPath, { quietIfLocked: true });
    if (lock) break;
    if (Date.now() >= deadline) {
      throw new Error(`Timed out waiting to update JSON file: ${filePath}`);
    }
    await new Promise((resolve) => setTimeout(resolve, retryMs));
```

`test/fs-lock.test.js` 锁定的 12 条不变量（用例名 → 起始行）：

```text
:20  sync lock records its live owner and releases its own lease
:36  locked JSON updates serialize their read-modify-write transactions
:79  sync lock immediately reclaims a fresh lease owned by a dead process
:100 sync lock reclaims a reused PID only after its heartbeat expires
:124 sync lock never reclaims an old lease while its owner is still alive
:137 sync lock keeps fresh legacy locks but reclaims them after the fallback age
:150 an obsolete owner cannot remove a replacement lock
:166 release and stale reclaim serialize ownership before replacement
:215 two stale reclaimers cannot both acquire the same lock
:247 notify sync waits for the active owner and acquires the released lock
:266 notify sync permits only one coalesced lock waiter
:291 notify sync stops waiting at its bounded deadline
```

补充（调用侧等待常量，非 fs.js）：`src/commands/sync.js:295-300` —— `NOTIFY_LOCK_WAIT_MS = 130_000`、`NOTIFY_LOCK_POLL_MS = 5_000`、`PRIORITY_LOCK_WAIT_MS = 90_000`、`PRIORITY_LOCK_POLL_MS = 250`；notify 合并等待器自身用 `${lockPath}.notify-wait` 锁实现单例（`src/commands/sync.js:461-481`，测试用例 `:266`/`:291` 覆盖）。`inspectLock` 的生产消费点 = `recordSyncSkip`（`src/commands/sync.js:491-493`，把 `lock_debris` 与 `lock_busy` 分开）。

---

## 2 `codexHashes` 事件去重的哈希输入（写入点、覆盖字段、碰撞面、实际作用）

- **判定**：已解决
- **结论**：`src/lib/rollout.js:382` 只是游标键名（`cursorKey: "codexHashes"`）；真正写入点是 `parseRolloutFile` 内的 `codexEvents.add(dedupKey)`（`src/lib/rollout.js:2628`），键为 `` `${sessionId || filePath}:${tokenTimestamp}` ``（`:2626`）——`sessionId` 是 rollout 文件名里的 UUID（`:2256-2262`，调用点 `:628`），`tokenTimestamp` 是 token_count 事件的 `timestamp` 字符串（`:2542`）。**键不含 usage 数值、模型、事件序号、行内容、文件大小或偏移**，且**不是摘要哈希**（`codexHashes` 是历史命名，落盘的是明文键）；命中即 `continue`（`:2627`）跳过桶累加。跨文件「碰撞」在「同一会话的多个物理副本/多路径」上是设计目标（Codex-Manager 原子重写换 inode、`sessions/ → archived_sessions/` 移动后仍去重，`:2608-2618` 注释逐字说明）；真实误合并窗口只有「同一 session 内两个不同 token_count 事件共享同一 timestamp 字符串」，以及 sessionId 解析失败时退化为 `filePath:timestamp` 造成**漏**去重（方向相反）。
- **证据**：

`src/lib/rollout.js:2622-2629`（写入点与跳过语义）

```js
    const codexEvents = typeof seenCodexEvents === "function"
      ? seenCodexEvents()
      : seenCodexEvents;
    if (codexEvents && (source === "codex" || source === "acode")) {
      const dedupKey = `${sessionId || filePath}:${tokenTimestamp}`;
      if (codexEvents.has(dedupKey)) continue;
      codexEvents.add(dedupKey);
    }
```

`src/lib/rollout.js:2542-2553`（timestamp 来源；delta 已在 `:2549` 推进）

```js
    const tokenTimestamp = typeof token.timestamp === "string" ? token.timestamp : null;
    if (!tokenTimestamp) continue;
    const lastUsage = info.last_token_usage;
    const totalUsage = info.total_token_usage;
    if (totalUsage && typeof totalUsage === "object") latestTotal = totalUsage;
    const rawDelta = consumeUsageDelta(usageDeltaState, lastUsage, totalUsage);
    const totalOnlyResetSentinel = source === DEFAULT_SOURCE
      && isCodexTotalOnlyResetSentinel(lastUsage, totalUsage);
    const delta = rawDelta && !totalOnlyResetSentinel ? normalizeUsage(rawDelta) : null;
    if (!delta || isAllZeroUsage(delta)) continue;
    delta.conversation_count = 1;
```

`src/lib/rollout.js:2256-2262`（sessionId = 路径里的 UUID）

```js
function codexSessionIdFromPath(filePath) {
  if (typeof filePath !== "string") return null;
  const m = filePath.match(
    /([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})\.jsonl$/,
  );
  return m ? m[1] : null;
}
```

`src/lib/rollout.js:320-324`（登记新事件）

```js
      newEventKeySet.add(key);
      newEventKeys.push(key);
      if (seenEvents) seenEvents.add(key);
      externalEventStore?.add(key);
    };
```

`src/lib/rollout.js:367-371`（无外部存储时持久化进游标数组）

```js
      persist() {
        if (!externalEventStore) {
          for (const key of newEventKeys || []) previousHashes.push(key);
        }
      },
```

`src/lib/cursor-store.js:1800-1805`（分片键取键内第 2 段的日期，印证 `session:时间戳` 键形）

```js
function codexEventShardKey(key) {
  if (typeof key !== "string") return "misc";
  const separator = key.indexOf(":");
  const day = separator >= 0 ? key.slice(separator + 1, separator + 11) : "";
  return /^\d{4}-\d{2}-\d{2}$/.test(day) ? day : "misc";
}
```

`src/lib/cursor-store.js:252-264`（落盘形态 `has/add`；`sync.js:1049-1051` 注入；`size` 内部省略）

```js
    this.codexEventStore = {
      get size() {
        …
      },
      has(key) {
        return self.hasCodexEvent(key);
      },
      add(key) {
        self.addCodexEvent(key);
      },
    };
```

测试侧直接以 `` `${sessionId}:${timestamp}` `` 断言该集合：`test/codex-sync-hot-path.test.js:862-863`。

---

## 3 跨根「事件级并入」的并入键（Claude 跨根）

- **判定**：已解决
- **结论**：跨根并入分两层、键各不相同。**文件分组键 = 会话 UUID**：`groupClaudeFilesAcrossRoots` 按文件 basename 的 `^([0-9a-f-]{36})\.jsonl$` 归组（`src/lib/session-analytics.js:1827-1828`），同一 UUID 跨 root 的多条路径并成一组交给 `scanClaudeSession`（`readableSessionPaths` 接受数组，`:516-523`）；**同一 root 内出现重复 UUID 视为身份歧义、该 UUID 整体退出并入**（`:1836-1845`）。**记录级去重键有两个**：(a) `priorRecordHashes` = 整行文本 `sha256(line).digest("base64url")`（`:584`），逐文件扫完后并入（`:654`）→「共享前缀（逐字节相同的行）计一次、两侧发散尾部行 hash 不同全部计入」正是这个集合实现的；(b) `seenMessages` = `claudeMessageDedupKey(obj)` = `message.id[:requestId]`（`src/lib/rollout.js:4914-4919`），控制计费 usage 只算一次（`:590-591`、`:618-620`）。任务描述中的 `seenMessageHashes` 是 `rollout.js` 内 **Claude 增量解析**路径的集合（`src/lib/rollout.js:982-985`、`:2854-2864`，键同为 `claudeMessageDedupKey` + 用户身份键 `u:`），session-analytics 侧对应集合名是 `seenMessages` —— 两处是不同阶段的同名机制，引用时不要混淆。
- **证据**：

`src/lib/session-analytics.js:1807-1810`（函数自述：共享前缀计一次、尾部两侧都留）

```js
// Group one logical session discovered under more than one root. A group is
// scanned as a unit so identical prefixes count once while divergent tails are
// both retained. Same-UUID siblings inside one Claude root stay separate: the
// path alone cannot prove which one a file in another root mirrors.
```

`src/lib/session-analytics.js:575-586`（行级哈希去重）

```js
  const priorRecordHashes = new Set();
  let scannedFiles = 0;
  for (const currentFilePath of filePaths) {
    const currentRecordHashes = new Set();
    try {
      const input = fs.createReadStream(currentFilePath, { encoding: "utf8" });
      const lines = readline.createInterface({ input, crlfDelay: Infinity });
      for await (const line of lines) {
        if (filePaths.length > 1) {
          const recordHash = crypto.createHash("sha256").update(line).digest("base64url");
          if (priorRecordHashes.has(recordHash)) continue;
          currentRecordHashes.add(recordHash);
```

`src/lib/session-analytics.js:590-591`（计费抑制键）

```js
        const timingKey = claudeMessageDedupKey(obj);
        const countedUsage = obj.type === "assistant" && obj.message && (!timingKey || !seenMessages.has(timingKey))
          ? normalizeClaudeUsage(obj.message.usage) : null;
```

`src/lib/session-analytics.js:618-620`（同一键决定 assistant 行是否计入）

```js
        const dedupKey = claudeMessageDedupKey(obj);
        if (dedupKey && seenMessages.has(dedupKey)) continue;
        if (dedupKey) seenMessages.add(dedupKey);
```

`src/lib/rollout.js:4914-4919`（`claudeMessageDedupKey` 定义）

```js
function claudeMessageDedupKey(obj) {
  const msgId = typeof obj?.message?.id === "string" && obj.message.id ? obj.message.id : null;
  if (!msgId) return null;
  const reqId = typeof obj?.requestId === "string" && obj.requestId ? obj.requestId : null;
  return reqId ? `${msgId}:${reqId}` : msgId;
}
```

`src/lib/session-analytics.js:1824-1832`（分组入口与 UUID 判定）

```js
function groupClaudeFilesAcrossRoots(groups) {
  const rootGroups = (groups || []).filter((group) => Array.isArray(group) && group.length > 0);
  if (rootGroups.length <= 1) return [...new Set(rootGroups[0] || [])].map((filePath) => [filePath]);
  const sessionIdOf = (filePath) =>
    path.basename(filePath).match(/^([0-9a-f-]{36})\.jsonl$/i)?.[1] || null;
```

接入点与配套：`src/lib/session-analytics.js:1965`（`const allClaude = groupClaudeFilesAcrossRoots(claudeGroups);`）；字节相同镜像的兼容折叠 `dedupeClaudeFilesAcrossRoots`（`:1895-1909`，只保留 mtime 更新的一侧）；专项测试 `test/session-analytics-claude-cross-root-dedup.test.js:70-170`。

---

## 4 Copilot 桶粒度：`copilotOtelAggregateKey` 的函数体与调用点

- **判定**：已解决
- **结论**：函数体即 `JSON.stringify([model, bucketStart])`（`src/lib/rollout.js:17462-17464`）；第二个实参不是别的桶函数产物，就是 `extractCopilotOtelUsage` 返回的 `usage.bucketStart`，该字段由 `toUtcHalfHourStart(new Date(tsMs).toISOString())` 生成（`:17551`）——**聚合粒度 = 每（模型 × UTC 半小时）一格**，同格多请求的 delta 累加（`addCopilotOtelAggregate`，`:17466-17474`），**不是每请求**。调用点仅两处，都在 v2→v3 的 Copilot Chat LogRecord 去重迁移校验里（`:17728`、`:17740`），用于分别重算 legacy key 与新 key 两套口径下的历史贡献并逐键做覆盖校验（`:17758-17769`）；主解析路径不用这个键，落桶用 `bucketKey("copilot", model, bucketStart)`（`:17766`）。
- **证据**：

`src/lib/rollout.js:17462-17464`（函数体）

```js
function copilotOtelAggregateKey(model, bucketStart) {
  return JSON.stringify([model, bucketStart]);
}
```

`src/lib/rollout.js:17466-17474`（唯一封装，桶内 delta 累加）

```js
function addCopilotOtelAggregate(aggregates, model, bucketStart, delta) {
  const key = copilotOtelAggregateKey(model, bucketStart);
  let totals = aggregates.get(key);
  if (!totals) {
    totals = initTotals();
    aggregates.set(key, totals);
  }
  addTotals(totals, delta);
}
```

`src/lib/rollout.js:17545-17552`（`bucketStart` 的来源）

```js
  const tsMs =
    copilotOtelTimeToMs(record.endTime) ||
    copilotOtelTimeToMs(record.startTime) ||
    copilotOtelTimeToMs(record.hrTime) ||
    copilotOtelTimeToMs(record.hrTimeObserved);
  if (!tsMs) return null;
  const bucketStart = toUtcHalfHourStart(new Date(tsMs).toISOString());
  if (!bucketStart) return null;
```

`src/lib/rollout.js:17724-17734`（legacy key 一侧调用；`:17736-17745` 是同形的新 key 一侧）

```js
          const oldKey = getCopilotLegacyDedupKey(record, record.attributes || {});
          const oldDuplicate = oldKey && oldSeen.has(oldKey);
          if (oldKey) oldSeen.add(oldKey);
          if (!oldDuplicate) {
            addCopilotOtelAggregate(
              oldTotals,
              usage.model,
              usage.bucketStart,
              usage.delta,
            );
          }
```

调用所属函数：`migrateCopilotChatLogRecordDedup`（`src/lib/rollout.js:17659`）。全仓 `addCopilotOtelAggregate` 仅 3 处命中（定义 `:17466` + 调用 `:17728/:17740`），无第四处消费。

---

## 5 `test/billable-total-tokens-migration.test.js` 的真实用途

- **判定**：已解决
- **结论**：它不是失效残留、也不被任何模块内联——它是**由 `npm test` 顶层执行的极弱门禁断言文件**：全文 9 行、顶层 2 条 `assert.ok`、**不注册任何 `test()`**；运行器为 `node --test test/*.test.js`（`package.json:41`），文件名匹配该 glob，故顶层断言随测试套件执行（失败即非零退出）。它守护的是线上迁移脚本 `scripts/ops/billable-total-tokens-migration.sql`（318 B，实测存在：给 `public.tokentracker_hourly` 加 `billable_total_tokens bigint`、`billable_rule_version smallint` 两列并建部分索引），断言内容仅「SQL 文本含这两个字段名」。全仓对该文件名的引用只有它自己（零 require 者，CI 无单独入口），断言强度只到「字符串存在」，不含任何 SQL 语义校验。
- **证据**：

`test/billable-total-tokens-migration.test.js:1-9`（全文）

```js
"use strict";

const fs = require("node:fs");
const assert = require("node:assert/strict");

const sql = fs.readFileSync("scripts/ops/billable-total-tokens-migration.sql", "utf8");

assert.ok(sql.includes("billable_total_tokens"));
assert.ok(sql.includes("billable_rule_version"));
```

`package.json:41`（执行入口）

```json
    "test": "node --test test/*.test.js",
```

`scripts/ops/billable-total-tokens-migration.sql:1-5`（实际内容前 5 行）

```sql
alter table public.tokentracker_hourly
  add column if not exists billable_total_tokens bigint,
  add column if not exists billable_rule_version smallint;

create index concurrently if not exists tokentracker_hourly_billable_null_idx
```

复核说明：`glob scripts/ops/*.sql` 未能列出该文件（工具侧盲区），改用 `Get-ChildItem` / `Test-Path` 逐字确认存在（`scripts/ops/` 下另有 4 份 SQL、2 份 cjs、1 份 baseline json）。

---

## 6 `usage_precision` 的完整取值域与全部消费方

- **判定**：已解决（含「16 行 / 15 条语句」与「无 antigravity 写入点」两项复核）
- **结论**：取值域 = `"reported" | "reported_incomplete" | "estimated" | "mixed" | "unavailable" | null | undefined`（`undefined` 只出现在写入侧 `|| undefined`，JSON 序列化后等于键缺失）。**写入源头只有三处**：`src/lib/trae-usage.js:94`（`"estimated"`）；`src/lib/rollout.js`（grok：`:20228/:20255/:20268` 输入 + `:20286` 经 `mergeGrokUsagePrecision`（`:19964-19968`）合并出 `reported|estimated|mixed`；trae 账本：`:22025` 出 `estimated`、`:22157` 归一为 `null|estimated`；qoder-new：`:6542` 显式 `null`、`:6648` 透传）；`src/lib/session-analytics.js`（`:1155` 出 `reported_incomplete|reported|unavailable`、`:1338`、`:1730` 同域，`:2709` 跨源聚合改写为 `mixed`）。其余 rollout.js 命中全是读取/传播：`:3325/:3341`、`:3403/:3422` 入队侧读桶值参与 `queuedKey` 与行字段；`:6725/:6731/:6755/:6760` qoder 桶传播；`:22011/:22113/:22151` trae 打包与计数。前端消费两点：`dashboard/src/lib/sessions-api.ts:113`（类型联合，**未包含 `"estimated"`**）与 `dashboard/src/ui/dashboard/components/SessionDetailModal.jsx:257`（`isGrok && session.usage_precision` 真值判断）。`dashboard/edge-patches/`、`migrations/`、`scripts/` 全仓零命中 → 仓库内不存在云端消费点。计数复核：`src/lib/rollout.js` 字面命中 **16 行**（`3325,3341,3403,3422,6542,6648,6725,6731,6755,6760,20286,20287,22011,22025,22113,22157`），其中 `20286-20287` 是同一赋值 → **15 条自然语句**，与主文档一致；全仓字面检索无任何 antigravity 文件命中 → 「无 antigravity 写入点」成立。
- **证据**：

`src/lib/rollout.js:19964-19968`（三值合并器，trae 亦复用）

```js
function mergeGrokUsagePrecision(current, next) {
  if (!current) return next;
  if (!next || current === next) return current;
  return "mixed";
}
```

`src/lib/rollout.js:22151-22157`（trae 桶精度重建，`reported` 落盘归一为 `null`）

```js
    bucketPrecisions.set(key, mergeGrokUsagePrecision(
      bucketPrecisions.get(key), turn[T_ESTIMATED] ? "estimated" : "reported",
    ));
  }
  for (const key of touchedBuckets) {
    const precision = bucketPrecisions.get(key);
    hourlyState.buckets[key].usage_precision = precision === "reported" ? null : precision || null;
  }
```

`src/lib/rollout.js:3325-3327`（入队侧读取，精度参与 `queuedKey` 比较）

```js
        const usagePrecision = bucket.usage_precision || null;
        const key = usagePrecision ? `${totalsKey(totals)}|${usagePrecision}` : totalsKey(totals);
        if (bucket.queuedKey === key) continue;
```

`src/lib/rollout.js:3341`（入队主循环，缩进 12 空格）

```js
            usage_precision: usagePrecision || undefined,
```

`src/lib/rollout.js:3422`（unknownBucket 分支，同形赋值，缩进 8 空格）

```js
        usage_precision: usagePrecision || undefined,
```

`src/lib/session-analytics.js:1155`（会话行写入）与 `:2709`（跨源聚合）

```js
    usage_precision: usageIsIncomplete ? "reported_incomplete" : (usageEvents > 0 ? "reported" : "unavailable"),
…
    if (cur.usage_precision !== row.usage_precision) cur.usage_precision = "mixed";
```

`dashboard/src/lib/sessions-api.ts:113-114`（前端类型联合，缺 `"estimated"`）

```ts
  usage_precision: "reported" | "reported_incomplete" | "mixed" | "unavailable" | null;
  usage_is_incomplete: boolean;
```

`dashboard/src/ui/dashboard/components/SessionDetailModal.jsx:257`（唯一前端读取点：真值判据）

```jsx
            {isGrok && session.usage_precision ? <p className="pb-3 text-xs leading-5 text-oai-gray-500 dark:text-oai-gray-400">{copy("sessions.grok.runtime_breakdown", ...)}</p> : null}
```

`src/lib/trae-usage.js:93-94`（`estimated` 唯一赋值点）

```js
  if (Object.values(totals).some((value) => !Number.isSafeInteger(value))) return null;
  if (estimated) totals.usage_precision = "estimated";
```

`src/lib/rollout.js:22011` 与 `:22025`（trae 账本 pack / unpack）

```js
    totals.usage_precision === "estimated" ? 1 : 0, totals.unpriced_input_tokens || 0];
…
  if (turn[T_ESTIMATED]) totals.usage_precision = "estimated";
```

计数复核（`Select-String -SimpleMatch`，独立于 `read` 工具）：

```text
rollout.js literal usage_precision lines: 16
3325,3341,3403,3422,6542,6648,6725,6731,6755,6760,20286,20287,22011,22025,22113,22157
repo-wide hits: dashboard\src\lib\sessions-api.ts(1) dashboard\src\pages\SessionsPage.test.jsx(2)
  dashboard\src\ui\dashboard\components\SessionDetailModal.jsx(1) src\lib\rollout.js(16)
  src\lib\session-analytics.js(5) src\lib\trae-usage.js(1) test\(6 files, 18)
```

---

## 7 `passive-mode.js` 的 8 个 hook 驱动 provider 与 status.js 分头上报行号

- **判定**：已解决
- **结论**：`detectPassiveProviders` 恰好 push **8 个** provider（`src/lib/passive-mode.js:97` claude、`:109` gemini、`:122` codex、`:133` acode、`:145` every_code、`:156` codebuddy、`:166` workbuddy、`:177` workbuddy-ai，覆盖 `:96-184`），消费 `hookStatus` 的 8 组键（`claude / gemini / codex_notify|codex / acode_notify|acode / every_code_notify|every_code / codebuddy / workbuddy / workbuddy-ai`，见 `:100/:112/:125/:136/:148/:159/:169/:180`）；`status.js` 传入 11 键（`src/commands/status.js:1024-1036`），未进入消费列表的 3 键 = `opencode_plugin`、`openclaw_session_plugin`、`grok`（冗余传参）。主文档 §12 补充登记的四条行号**逐条一致**：omp `:1317-1318`、OpenCode `:1363`（条件行 `:1362`）、Grok `:1452`（条件行 `:1451`）、OpenClaw `:1296-1298`（三行 hook 状态行，非「日志在」reader 行）。四者与 `Passive mode:` 汇总行（`:1445-1450`）同属 `cmdStatus`（`src/commands/status.js:189` 起）的人类可读报告数组；`passiveProviders` 在 `:1021` 一次性构建，`:1248-1249`（JSON 路径）与 `:1445`（文本路径）双消费。
- **证据**：

`src/lib/passive-mode.js:93-104`（第 1 个 provider 与入口）

```js
function detectPassiveProviders({ home, hookStatus, env = process.env }) {
  const out = [];
  // Claude Code — logs at ~/.claude/projects/<name>/*.jsonl
  out.push(buildEntry({
    name: "claude",
    hookExpected: true,
    hookInstalled: Boolean(hookStatus?.claude),
```

`src/lib/passive-mode.js:132-142` 与 `:175-184`（第 4 与第 8 个 provider）

```js
  const acodeHome = env.TOKENTRACKER_ACODE_HOME || path.join(home, ".acode");
  out.push(buildEntry({
    name: "acode",
    hookExpected: true,
    hookInstalled: Boolean(hookStatus?.acode_notify ?? hookStatus?.acode),
…
  const workbuddyAiHome = expandHome(env.WORKBUDDY_AI_HOME, home) || path.join(home, ".workbuddy-ai");
  out.push(buildEntry({
    name: "workbuddy-ai",
    hookExpected: true,
    hookInstalled: Boolean(hookStatus?.["workbuddy-ai"]),
```

`src/commands/status.js:1024-1035`（传入 11 键，其中 3 键不被消费）

```js
    hookStatus: {
      codex_notify: notifyConfigured,
      acode_notify: acodeConfigured,
      every_code_notify: everyCodeConfigured,
      claude: claudeHookConfigured,
      gemini: geminiHookConfigured,
      opencode_plugin: opencodePluginConfigured,
      openclaw_session_plugin: Boolean(openclawSessionPluginState?.configured),
      codebuddy: Boolean(codebuddyHookConfigured),
      workbuddy: Boolean(workbuddyHookConfigured),
      "workbuddy-ai": Boolean(workbuddyAiHookConfigured),
      grok: Boolean(grokHookState?.configured),
```

`src/commands/status.js:1317-1318`（omp「日志在」reader 行）

```js
      ompInstalled || ompHookState.ompPresent
        ? `- oh-my-pi: passive reader (${ompFiles.length} session jsonl file${ompFiles.length !== 1 ? "s" : ""} found${ompHookState.configured ? ", notify extension: yes" : ", notify extension: no"})`
```

`src/commands/status.js:1362-1363`（OpenCode）与 `:1451-1452`（Grok）

```js
      opencodeInstalled
        ? `- OpenCode: passive reader (storage: ${opencodeStorageActive.join(" | ") || "not found"}, DB: ${opencodeDbActive.join(" | ") || "not found"})`
…
      grokInstalled
        ? `- Grok Build (xAI): ${grokHookState.configured ? "hook installed" : "detected"} (${grokSessions.length} session${grokSessions.length !== 1 ? "s" : ""} found, hook: ${grokHookState.configured ? "yes" : "no"})`
```

`src/commands/status.js:1295-1298`（OpenClaw 三行 hook 状态，无「日志在」reader 行）

```js
      `- Opencode plugin: ${opencodePluginConfigured ? "set" : "unset"}`,
      `- OpenClaw session plugin: ${openclawSessionPluginState?.configured ? "set" : "unset"}`,
      `- OpenClaw session plugin conversation access: ${openclawSessionPluginState?.conversationAccess ? "set" : "unset"}`,
      `- OpenClaw hook (legacy): ${openclawHookState?.configured ? "set" : "unset"}`,
```

---

## 8 `multiInstallParse` 异常分支的写回语义

- **判定**：部分解决
- **结论**：`catch` 分支只在错误码**不是** `TOKENTRACKER_CURSOR_STORE_RETRY` 时执行 `cursors[providerName] = ns`——把 provider 槽位写回**命名空间表**（该表含此前成功 install 的游标），随后原样抛出；RETRY 时**不写回**，保留 parserFn 刚写入的现场交给上层重试（该码含义 = 「游标存储 generation 变化，操作需重启」，`src/lib/cursor-store.js:1889-1896`；重试消费点 `src/commands/sync.js:1443-1445` 对 opencode 重试一次、`src/commands/sync.js:1069-1085` 对 Codex 重新加载后重跑）。写回值是「命名空间表」而非任何回滚值：**异常不回滚已完成 install 的进度**，也不重置游标——`test/multi-install-parser.test.js:107` 断言第一个 install 的游标状态被保留，`:127-130` 断言 RETRY 时保留 parserFn 激活的 fallback 命名空间。「会不会导致重复解析」在静态层面只能给条件结论：已成功 install 的游标继续前进、不会重复；抛错 install 的游标停在抛错瞬间的在途状态，是否重复取决于该 provider 的 parserFn 是否「先写桶、后提交游标」，无法从 `multi-install-parser.js` 本身判定。
- **证据**：

`src/lib/multi-install-parser.js:49-54`（异常分支本体）

```js
    } catch (parseErr) {
      if (parseErr?.code !== "TOKENTRACKER_CURSOR_STORE_RETRY") {
        cursors[providerName] = ns;
      }
      throw parseErr;
    }
```

`src/lib/multi-install-parser.js:39-45, 56`（进入/成功路径的槽位切换，供对比）

```js
    cursors[providerName] = ns[key];
    try {
      const result = await parserFn({
        ...getParams(paths[key], key), ...shared, cursors,
        onProgress: wrapProgress(onProgress, key),
      });
      ns[key] = cursors[providerName];
…
  cursors[providerName] = ns;
```

`test/multi-install-parser.test.js:93-107`（非 RETRY 异常：第一个 install 进度保留）

```js
    () => multiInstallParse({
      paths: { native: "/a", wsl: "/b" },
      parserFn: async ({ resolvedPath, cursors: c }) => {
        if (resolvedPath === "/a") c.hermes = { done: true };
        if (resolvedPath === "/b") throw new Error("second install failed");
        return { recordsProcessed: 1 };
      },
      providerName: "hermes",
…
  assert.deepEqual(cursors.hermes.native, { done: true }, "first install's cursor state should be preserved");
```

`test/multi-install-parser.test.js:110-120, 127`（RETRY 例外：保留现场）

```js
test("cursor-store retry preserves the activated fallback namespaces", async () => {
  const cursors = { opencode: { native: { old: true }, wsl: { old: true } } };
  await assert.rejects(
    () => multiInstallParse({
      parserFn: async ({ cursors: current }) => {
        current.opencode = { native: { fallback: true }, wsl: { fallback: true } };
        const error = new Error("retry");
        error.code = "TOKENTRACKER_CURSOR_STORE_RETRY";
        throw error;
      },
…
  assert.deepEqual(cursors.opencode, { native: { fallback: true }, wsl: { fallback: true } });
```

`src/lib/cursor-store.js:1889-1896`（RETRY 语义定义）

```js
function cursorStoreRetry(cause) {
  const error = new Error(
    "Cursor store generation changed; restart the operation",
    cause ? { cause } : undefined,
  );
  error.code = CURSOR_STORE_RETRY_CODE;
  return error;
}
```

- **若维持**：要把「是否重复解析」从条件结论升级为确定结论，需按 provider 逐一构造「第一个 install 正常 + 第二个 install 在写桶与提交游标之间抛异常」的 fixture，跑完 `multiInstallParse` 后再跑一次同步，检查该 install 对应桶是否被二次累加。生产调用点共 12 处，全部在 `src/commands/sync.js`：opencode `:1419/:1467/:1545`、kilo `:1686`、mimo `:1722`、zcode `:1776`、goose `:1994`、zed `:2063`、kiro `:2325`、hermes `:2379`、kiro-cli `:2478`。

---

## 9 `rebuildingCodexBaseline` 与 `prev.lastTotal` 兜底动机

- **判定**：已解决
- **结论**：两条兜底分别防「基线缺失导致增量口径断裂」与「重扫双计」。
  `rebuildingCodexBaseline` 的判定条件是「同 inode、未截断、已读过部分字节（`0 < prevOffset < size`）、但游标里的 `lastTotal` 缺失或不是对象」（`src/lib/rollout.js:493-500`）——命中即视为基线不可用，强制 `startOffset = 0`、`lastTotal = null`、`tokenUsageBaselines = null`（`:501-507`）整文件重扫；动机是 Codex 的 `total_token_usage` 是**累计快照**，增量必须减上次基线（`usageDeltaState`，`:2323-2327`），基线没了只能重扫重建（注释自述涉及 pre-#652 游标，`:513-517`）。
  `prev?.lastTotal` 兜底位于 `needsHistoricalRolloutDedup` 的稳态短路之后（`:456-460`）：`sameInode && !truncated && startOffset > 0` 表示「纯追加，offset 之后全是新事件」，直接返回 `false` 省掉持久去重集构造；落到 `:459` 的情形（offset 未推进/归零）若游标仍带 `lastTotal`，说明该文件此前确实解析过内容，于是保守返回 `true` 强制使用**持久事件集**——防的就是重扫时把已计入的历史事件再加一遍；新文件（`prev` 为空）才返回 `false`。
  两条协同点：`rebuildingBaseline` 在 `needsHistoricalRolloutDedup` 内是第一优先判据（`:436` 无条件 `return true`），并经 `:584` 传入，即「因基线缺失而重扫」必然携带持久去重集，不会双计。
- **证据**：

`src/lib/rollout.js:493-504`（基线重建判定与后果）

```js
    const rebuildingCodexBaseline = Boolean(
      (fileSource === DEFAULT_SOURCE || fileSource === "acode") &&
      sameInode &&
      !truncated &&
      prevOffset > 0 &&
      prevOffset < st.size &&
      (!prev.lastTotal || typeof prev.lastTotal !== "object")
    );
    const startOffset = sameInode && !truncated && !rebuildingCodexBaseline ? prevOffset : 0;
    const lastTotal = sameInode && !truncated && !rebuildingCodexBaseline
      ? prev.lastTotal || null
      : null;
```

`src/lib/rollout.js:456-460`（稳态短路与 `prev.lastTotal` 兜底）

```js
    // Steady state: one path, already read past, not rotated. Everything after
    // our own offset is genuinely new, so skip the persisted-set construction.
    if (sameInode && !truncated && startOffset > 0) return false;
    if (prev?.lastTotal) return true;
    return false;
```

`src/lib/rollout.js:428-437`（`rebuildingBaseline` 第一优先启用持久去重集）

```js
  const needsHistoricalRolloutDedup = ({
    filePath,
    prev,
    sameInode,
    truncated,
    startOffset,
    rebuildingBaseline,
  }) => {
    if (rebuildingBaseline) return true;
    if (prev && (!sameInode || truncated)) return true;
```

`src/lib/rollout.js:577-587`（两条判据的实际接线）

```js
    const codexEventTracker = rolloutEventDedup
      ? (needsHistoricalRolloutDedup({
          filePath,
          prev,
          sameInode,
          truncated,
          startOffset,
          rebuildingBaseline: rebuildingCodexBaseline,
        })
          ? rolloutEventDedup.getHistoricalEvents
          : rolloutEventDedup.getAppendOnlyEvents)
```

`src/lib/rollout.js:513-517`（基线缺失的来源：老游标）

```js
    // Only valid for the bytes before the resume offset; a rescan from 0
    // rebuilds it. A cursor from before #652 with a lastTotal has seen a
    // token_count already.
    const usageRecordState = startOffset > 0
      ? prev?.codexUsageRecord || (prev?.lastTotal ? { sawTokenCount: true } : null)
      : null;
```

---

## 结项摘要

| # | 事项 | 判定 | 关键锚点 |
| --- | --- | --- | --- |
| 1 | `fs.js` 锁实现细节 | 已解决 | `src/lib/fs.js:69-74,296-307,117-128,136,337-346,189-200,419-431`；`test/fs-lock.test.js` 12 用例 |
| 2 | `codexHashes` 哈希输入与去重作用 | 已解决 | `src/lib/rollout.js:2622-2629,2542-2553,2256-2262,320-324,367-371` |
| 3 | 跨根事件级并入键 | 已解决 | `src/lib/session-analytics.js:1807-1810,575-586,590-591,618-620,1824-1832`；`src/lib/rollout.js:4914-4919` |
| 4 | Copilot 桶粒度 | 已解决 | `src/lib/rollout.js:17462-17473,17551,17724-17734` |
| 5 | 迁移测试用途 | 已解决 | `test/billable-total-tokens-migration.test.js:1-9`；`package.json:41` |
| 6 | `usage_precision` 取值域与消费方 | 已解决 | `src/lib/rollout.js` 16 行 / 15 条；`dashboard/src/lib/sessions-api.ts:113` 缺 `"estimated"` |
| 7 | passive-mode 8 provider 与 status 行号 | 已解决 | `src/lib/passive-mode.js:96-184`；`src/commands/status.js:1317-1318,1363,1452,1296-1298` |
| 8 | `multiInstallParse` 异常写回语义 | 部分解决 | `src/lib/multi-install-parser.js:49-54`；`test/multi-install-parser.test.js:107,127-130` |
| 9 | 两条兜底动机 | 已解决 | `src/lib/rollout.js:493-504,456-460,428-437,577-587,513-517` |

建议回写主文档的两处：

1. §12 补充登记中「`usage_precision` 全文件出现点 16 行、按自然语句计 15 条」**成立**，可保留；但应补一句：前端类型联合（`dashboard/src/lib/sessions-api.ts:113`）**不含 `"estimated"`**，而 `estimated` 由 trae / grok 两个来源真实写入（trae 国际版 `src/lib/trae-usage.js:94`、grok 估算分支 `src/lib/rollout.js:20286`），前端目前只以真值判断消费该字段。
2. 「Grok / OpenCode / OpenClaw / omp 分头上报**未逐一核对**」这一表述可以升级为**已核对、行号一致**（见第 7 节，四条全部逐字命中，且同为 `cmdStatus` 内的报告行）。
