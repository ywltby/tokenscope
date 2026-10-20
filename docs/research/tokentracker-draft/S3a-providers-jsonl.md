# S3a：文本 / JSONL 型 provider 解析器逐家取证（19 家）

> 样本：`xiufengsun/TokenTracker` v1.3.14，本地只读副本 `C:\Users\admin\AppData\Local\Temp\tt-research`（研究过程未修改任何文件、未执行仓库脚本）。
> 行号口径：全部以本会话 `read` 工具输出为准（`read` 报该文件共 **24932** 行）。**警告**：PowerShell 默认 `Get-Content` 对该文件报 24917 行，两者存在 5～18 行漂移。**漂移机制**（R1 复核实测）：`Get-Content` 按 GBK 解码，行尾字节 ≥ 0x81 的行会把下一行的换行一并吞掉，故 GBK 与 UTF-8 行号在 ~1300 行前完全相同，之后随位置累积——2000 行处 delta≈1、7000 行处 ≈4、10000 行处 ≈6、13000 行以上 ≈10、文件末 ≈15（24932 → 24917）。凡仅由 PowerShell 定位的行号即为 **GBK 口径**，不得与 `read` 口径混用。本文件原有「仅由 PowerShell 定位」清单已按权威口径（`read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`）逐条重验，结论见 §7.1 第 10 条，修正明细见文末《修正记录（R1 复核后）》。
> 范围：`parseGeminiIncremental`、`parseOpencodeIncremental`、`parseOpenclawIncremental`、`parseKiroIncremental`、`parseHermesIncremental`、`parseKiroCliIncremental`、`parseKimiIncremental`、`parseKimiCodeIncremental`、`parseCodebuddyIncremental`、`parseWorkbuddyIncremental`、`parseRoocodeIncremental`、`parseClineIncremental`、`parseZedIncremental`、`parseLmstudioIncremental`、`parseUnslothIncremental`、`parseAnythingllmIncremental`、`parseGooseIncremental`、`parseDroidIncremental`、`parseKilocodeIncremental`（全部在 `src/lib/rollout.js`）。
> 不含：Claude / ClaudeScience / Rollout(Codex)，以及任何 `*DbIncremental` / `*ApiIncremental`。

---

## 0. 速览

| # | Provider | `source` | 数据形态 | 事件时间 | 去重机制 | reasoning 并入 output | $0 风险 |
|---|----------|----------|----------|----------|----------|----------------------|---------|
| 1 | Gemini | `gemini` | 整份 JSON `session-*.json` | `msg.timestamp` | 数组下标 + 累计差分 | 否（`thoughts` 独立） | **有** |
| 2 | Opencode | `opencode` | 单消息 JSON 目录（**+ 同寄主 SQLite 支路**，`src/lib/rollout.js:5038` 起） | `msg.time.completed ?? created` | 消息 key + 指纹 + 差分 | 否 | 无 |
| 3 | Openclaw | `openclaw` | JSONL 转录 + agent SQLite | `obj.timestamp` | 字节偏移 + 事件计数身份 | 否（恒 0） | 无 |
| 4 | Kiro | `kiro` | SQLite（+ JSONL 兼容） | `row.timestamp`(UTC 文本) | 自增 `id` 水位 | 否（恒 0） | 无 |
| 5 | Hermes | `hermes` | SQLite `state.db` | `ended_at ?? started_at` | `started_at` 水位 + 差分 | 否（独立桶） | 无 |
| 6 | KiroCli | **`kiro`** | SQLite（字符估算）+ legacy 会话 `.json`（**优先真实 `input_token_count`/`output_token_count`**）+ v2 `messages.jsonl`（字符估算） | `request_start_timestamp_ms` | per-request 指纹（可回退） | 否（独立桶） | 低（reasoning-only 行） |
| 7 | Kimi | `kimi` | JSONL `wire.jsonl` | `entry.timestamp`（秒） | 文件偏移 + `message_id` | 否（恒 0） | 无 |
| 8 | KimiCode | **`kimi`** | JSONL `wire.jsonl`(递归) | `entry.time ?? evt.time`（毫秒） | `evt.uuid` | 否（恒 0） | 无 |
| 9 | Codebuddy | `codebuddy` | JSONL + IDE 扩展日志 | `entry.timestamp` / 日志行时间 | `messageId` + 用量指纹 | 否（独立桶） | 无（丢纯 total 行） |
| 10 | Workbuddy | `workbuddy` / `workbuddy-ai` | JSONL + traces JSON + SQLite | `entry.timestamp` | `messageId` 集合 | 否（从 completion 拆出） | 无 |
| 11 | Roocode | `roocode` | `ui_messages.json` | `msg.ts` | `taskUuid:ts` | 否（恒 0） | 无 |
| 12 | Cline | `cline` | 会话转录 JSON | `msg.ts` | 消息 key + 逐指标差分 | 否（不并入 total） | 无 |
| 13 | Zed | `zed` | SQLite BLOB（zstd/json） | `updated_at ?? created_at ?? now` | `updated_at` 水位 + 线程差分 | 否（恒 0） | 无 |
| 14 | Lmstudio | `lmstudio` | 文本 `.log`（内嵌 JSON） | 日志行时间 / 文件 mtime | `messages` key + 文件 stat | 否（拆出） | 无 |
| 15 | Unsloth | `unsloth` | SQLite `studio.db` | `created_at` | 扫描窗口 + 指纹 | 否（拆出） | 无 |
| 16 | Anythingllm | `anythingllm` | SQLite | `createdAt ?? lastUpdatedAt` | 自增 `id` 水位 | 否（由 total 反推） | 无 |
| 17 | Goose | `goose` | SQLite `sessions.db` | `created_at` / now | 会话差分 + DB mtime | 否（由 total 反推） | 低 |
| 18 | Droid | `droid` | `*.settings.json` | **文件 mtime** | 会话差分 + mtime | 否（`thinking` 独立） | 无（有兜底） |
| 19 | Kilocode | **`kilo-code`** | `ui_messages.json` | `msg.ts` | `taskUuid:ts` | 否（恒 0） | 无 |

---

## 1. 所有 provider 共享的落盘机制

### 1.1 累加器与默认值

`src/lib/rollout.js:55`、`src/lib/rollout.js:56`：`DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`。即“模型未识别”落成字符串 `unknown`，不是空串。

`src/lib/rollout.js:3996`：

```js
function getHourlyBucket(state, source, model, hourStart) {
  const buckets = state.buckets;
  const normalizedSource = normalizeSourceInput(source) || DEFAULT_SOURCE;
  const normalizedModel = normalizeModelInput(model) || DEFAULT_MODEL;
```

`src/lib/rollout.js:4053`：

```js
function initTotals() {
  return {
    input_tokens: 0,
    cached_input_tokens: 0,
    cache_creation_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
    total_tokens: 0,
    billable_total_tokens: 0,
    total_cost_usd: 0,
    conversation_count: 0,
  };
}
```

`addTotals` 的关键两行（`src/lib/rollout.js:4099`）：

```js
  target.total_tokens += delta.total_tokens || 0;
  target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0;
```

**要点**：`total_tokens` 必须由各解析器自己算；只填 `total_tokens` 的 delta 会被原样累加并写进队列行——这是 §5 的判据。

### 1.2 UTC 半小时桶

`src/lib/rollout.js:4173`：

```js
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
```

无效时间返回 `null`，调用点普遍 `if (!bucketStart) continue;`（**跳过事件**，不落回 now）。例外：Droid 用文件 mtime、Lmstudio 用 mtime 兜底、Zed 与 Goose 用 `new Date().toISOString()` 兜底。

### 1.3 队列行字段（输出契约）

`src/lib/rollout.js:3328`：

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
```

行里还会按条件出现 `usage_precision`、`priority_*`（`src/lib/rollout.js:3341`、`src/lib/rollout.js:3342`），任务给的标准字段集未含它们。

`source` 统一小写化（`src/lib/rollout.js:4254`）：

```js
function normalizeSourceInput(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim().toLowerCase();
  return trimmed.length > 0 ? trimmed : null;
}
```

队列是**绝对值快照流**而非增量事件流：同桶内容不变则跳过（`src/lib/rollout.js:3327`：`if (bucket.queuedKey === key) continue;`），消费端应按 `(source, model, hour_start)` 覆盖。

### 1.4 “零用量”闸门把 total 也算作非零

`src/lib/rollout.js:4945`：

```js
function isAllZeroUsage(u) {
  if (!u || typeof u !== "object") return true;
  for (const k of [
    "input_tokens",
    "cached_input_tokens",
    "cache_creation_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
  ]) {
    if (Number(u[k] || 0) !== 0) return false;
  }
  return true;
}
```

### 1.5 本地推理共用归一化器

`src/lib/rollout.js:12994`（`function normalizeLocalStudioTokens(usage)` 函数头，Lmstudio 与 Unsloth 共用；`conversation_count` 与 `total_cost_usd` 在返回块 `src/lib/rollout.js:13039`～`13049` 写死——`total_cost_usd: 0` 在 `13047`、`conversation_count: 1` 在 `13048`）：

```js
  return {
    input_tokens: Math.max(0, total - completion - cacheRead - cacheWrite),
    cached_input_tokens: cacheRead,
    cache_creation_input_tokens: cacheWrite,
    output_tokens: Math.max(0, completion - reasoning),
    reasoning_output_tokens: reasoning,
    total_tokens: total,
    billable_total_tokens: total,
    total_cost_usd: 0,
    conversation_count: 1,
  };
```

`reconcilePassiveUsageEvent`（`src/lib/rollout.js:13242`，Lmstudio/Unsloth 的落桶入口，**同 key 先减旧再加新**）：

```js
function reconcilePassiveUsageEvent({ event, source, messages, hourlyState, touchedBuckets }) {
  const previous = messages[event.key];
  const unchanged = previous
    && previous.model === event.model
    && previous.bucketStart === event.bucketStart
    && totalsKey(previous.totals) === totalsKey(event.totals);
  if (unchanged) return false;

  if (previous?.model && previous?.bucketStart && previous?.totals) {
    const oldBucket = getHourlyBucket(hourlyState, source, previous.model, previous.bucketStart);
    subtractTotals(oldBucket.totals, previous.totals);
```

---

## 2. 分类勘误

“文本 / JSONL 型”只对了一半：**7 家实际是 SQLite 读取**（Kiro、Hermes、KiroCli、Zed、Unsloth、Anythingllm、Goose），其余以 JSON/JSONL/日志为主（Openclaw 另有 agent SQLite 支路）。

三处 `source` 命名陷阱：

- `kimi` 由 Kimi 与 KimiCode 两套解析器共用（`src/lib/rollout.js:9611`、`src/lib/rollout.js:9844`）。
- `kiro` 由 Kiro IDE 与 Kiro CLI 共用（`src/lib/rollout.js:7567`、`src/lib/rollout.js:9210`、`src/lib/rollout.js:9450`）；源码明说要合并（`src/lib/rollout.js:8112`）：

```
// ... Source is merged with Kiro IDE (source='kiro') and canonicalized
// model names are used so CLI and IDE rows collapse when they refer to the
// same underlying Bedrock model.
```

- `kilo-code` 带连字符（`src/lib/rollout.js:15651`）；Workbuddy 用 variant 直接当 source（`src/lib/rollout.js:11004`）。

---

## 3. 逐家取证

### 3.1 Gemini（`parseGeminiIncremental`，`src/lib/rollout.js:1242`）

**source**：调用方显式传 `"gemini"`（`src/commands/sync.js:1272`）；解析器兜底 `src/lib/rollout.js:1265`（`normalizeSourceInput(source) || "gemini"`），并允许逐文件覆盖（`src/lib/rollout.js:1275`）。

**路径**：官方目录 `~/.gemini`（`GEMINI_HOME` 可覆盖）+ `tmp`（`src/commands/sync.js:1224`、`src/commands/sync.js:1234`）：

```js
      const geminiNativeValue = process.env.GEMINI_HOME || path.join(home, ".gemini");
```
```js
        fileSets.push(await listGeminiSessionFiles(path.join(geminiPaths.native, "tmp")));
```

目录约定 `tmp/<project>/chats/session-*.json`（`src/lib/rollout.js:226`）：

```js
async function listGeminiSessionFiles(tmpDir) {
  const out = [];
  const roots = await safeReadDir(tmpDir);
  for (const root of roots) {
    if (!root.isDirectory()) continue;
    const chatsDir = path.join(tmpDir, root.name, "chats");
    const chats = await safeReadDir(chatsDir);
    for (const entry of chats) {
      if (!entry.isFile()) continue;
      if (!entry.name.startsWith("session-") || !entry.name.endsWith(".json")) continue;
```

**判定**：被动扫描官方目录。

**格式**：整份 JSON；事件粒度是 `session.messages` 数组元素（`src/lib/rollout.js:2916`）；文件身份含 inode（`src/lib/rollout.js:1284` 的 `const inode = st.ino || 0;`，判等见 `src/lib/rollout.js:1287`～`1291` 的 `sameFileMetadata`）。

**字段映射**（`src/lib/rollout.js:4734`）：

```js
function normalizeGeminiTokens(tokens) {
  if (!tokens || typeof tokens !== "object") return null;
  const input = toNonNegativeInt(tokens.input);
  const cached = toNonNegativeInt(tokens.cached);
  const output = toNonNegativeInt(tokens.output);
  const tool = toNonNegativeInt(tokens.tool);
  const thoughts = toNonNegativeInt(tokens.thoughts);
  const reportedTotal = toNonNegativeInt(tokens.total);
  const computedTotal = input + cached + output + tool + thoughts;
  const total = Math.max(reportedTotal, computedTotal);
```

| 日志字段 | 桶 |
|----------|----|
| `tokens.input` | `input_tokens` |
| `tokens.cached` | `cached_input_tokens` |
| `tokens.output` + `tokens.tool` | `output_tokens` |
| `tokens.thoughts` | `reasoning_output_tokens`（**不并入 output**） |
| `tokens.total` | `total_tokens` = `max(reported, 分桶之和)` |
| — | `cache_creation_input_tokens` 恒 0 |

缺字段一律 0（`toNonNegativeInt`，`src/lib/rollout.js:4964`）。

**模型**：逐条消息 `msg.model`，会话内沿用上次值（`src/lib/rollout.js:2952`）；无 Gemini 专属兜底，缺失落 `unknown`。

**时间戳**：`msg.timestamp`，缺失即跳过（`src/lib/rollout.js:2956`、`src/lib/rollout.js:2964`）：

```js
    const timestamp = typeof msg.timestamp === "string" ? msg.timestamp : null;
    const currentTotals = normalizeGeminiTokens(msg.tokens);
    if (idx >= hourlyBegin && (!timestamp || !currentTotals)) {
      totals = currentTotals || totals;
    }
```

**游标**：数组下标 + 累计基线（`cursors.files[key] = { lastIndex, lastTotals, lastModel }`，`src/lib/rollout.js:1361`）；数组被截断即整段重置（`src/lib/rollout.js:2917`）。

**差分落桶**（`src/lib/rollout.js:2968`）：

```js
      const delta = diffGeminiTotals(currentTotals, totals);
      if (!delta || isAllZeroUsage(delta)) {
        totals = currentTotals;
      } else {
        delta.conversation_count = 1;
        bucketStart = toUtcHalfHourStart(timestamp);
```

`conversation_count` = 产生非零差分的消息条数（`src/lib/rollout.js:2974`）。

**$0 风险：有**——`total` 取 max 且闸门 `isAllZeroUsage` 含 `total_tokens`（`src/lib/rollout.js:2971`）。若日志只写 `tokens.total`，会写出 input/output 全 0 的队列行（19 家中唯一；成立前提与计价侧条件见 §5 A 与 §5 开头的「计价侧前提」）。

### 3.2 Opencode（`parseOpencodeIncremental`，`src/lib/rollout.js:1412`）

**source**：`"opencode"`（`src/commands/sync.js:1440`），兜底见 `src/lib/rollout.js:1438`。

**路径**：`OPENCODE_HOME || $XDG_DATA_HOME/opencode`，再拼 `storage`（`src/commands/sync.js:1339`、`src/commands/sync.js:1400`）：

```js
      const opencodeStorageNativeValue = process.env.OPENCODE_HOME || path.join(xdgDataHome, "opencode");
```
```js
          ? await listOpencodeMessageFiles(path.join(storagePaths[key], "storage"))
```

消息目录（`src/lib/rollout.js:243`）：

```js
async function listOpencodeMessageFiles(storageDir) {
  const out = [];
  const messageDir = path.join(storageDir, "message");
  await walkOpencodeMessages(messageDir, out);
```

**判定**：被动扫描官方目录（同寄主另有 SQLite 支路，属他人范围；两路结果在 `src/commands/sync.js:1384` 相加）。

**字段映射**（`src/lib/rollout.js:4755`）：

```js
function normalizeOpencodeTokens(tokens) {
  if (!tokens || typeof tokens !== "object") return null;
  const input = toNonNegativeInt(tokens.input);
  const output = toNonNegativeInt(tokens.output);
  const reasoning = toNonNegativeInt(tokens.reasoning);
  const cached = toNonNegativeInt(tokens.cache?.read);
  const cacheWrite = toNonNegativeInt(tokens.cache?.write);
  const total = input + output + reasoning + cached + cacheWrite;
```

| 日志字段 | 桶 |
|----------|----|
| `tokens.input` | `input_tokens` |
| `tokens.cache.read` | `cached_input_tokens` |
| `tokens.cache.write` | `cache_creation_input_tokens` |
| `tokens.output` | `output_tokens` |
| `tokens.reasoning` | `reasoning_output_tokens` |
| — | `total_tokens` 重算 |

**模型**：兼容 `modelID` / `model`(字符串) / `model: { id }`（`src/lib/rollout.js:4272`）；落桶兜底 `DEFAULT_MODEL`（`src/lib/rollout.js:3143`）：

```js
  const { modelId: fileModelId } = normalizeOpencodeModelFields(msg);
  const model = fileModelId || DEFAULT_MODEL;
```

**时间戳**（`src/lib/rollout.js:3122`）：`msg.time.completed` → `msg.time.created`，都无则不产事件。

**游标**：`cursors.files[key]`（`src/lib/rollout.js:1513`）存 `lastTotals/fingerprint/messageKey/dedupedForkCopy`；差分复用 `diffGeminiTotals`（`src/lib/rollout.js:3109`）；fork 副本去重（`src/lib/rollout.js:3805`），复活时整份重加（`src/lib/rollout.js:3105`）：

```js
  // A formerly deduped copy can become a genuinely distinct in-place update.
  // Its prior totals were removed from the buckets, so re-add the full current
  // snapshot instead of only the delta from the suppressed value.
  const effectiveLastTotals = prev?.dedupedForkCopy === true ? null : lastTotals;
```

**$0 风险**：无（差分函数对全 0 返回 null）；代价是纯 total 行会被静默丢弃。

### 3.3 Openclaw（`parseOpenclawIncremental`，`src/lib/rollout.js:1654`）

**source**：`"openclaw"`（`src/commands/sync.js:1109`），逐文件可覆盖（`src/lib/rollout.js:1698`）。

**路径**：`~/.openclaw`（三档 env 覆盖）+ `agents/<agent>/sessions/*.jsonl`，Windows 额外探测 WSL（`src/lib/rollout.js:1582`、`src/lib/rollout.js:1622`）：

```js
  return override || path.join(os.homedir(), ".openclaw");
```
```js
    const agentsDir = path.join(openclawHome, "agents");
```

子目录集合（`src/lib/rollout.js:1612`）：`["sessions", "session-sqlite-import-archive"]`；另有 agent SQLite（`src/lib/rollout.js:1638`）。

**判定**：被动扫描（sync 注释 `src/commands/sync.js:1102`：`// Parses plugin-triggered and/or passively discovered session files.`）。hook/插件存在但只影响触发时点。

**字段映射**（`src/lib/rollout.js:2094`）：

```js
    const openclawRawInput = Number(usage.input || 0);
    const openclawCached = Number(usage.cacheRead || 0);
    const openclawCacheWrite = Number(usage.cacheWrite || 0);
    const openclawOutput = Number(usage.output || 0);
    const openclawInput = Math.max(0, openclawRawInput - openclawCached);
```

| 字段 | 桶 |
|------|----|
| `message.usage.input`（先减 cached） | `input_tokens` |
| `message.usage.cacheRead` | `cached_input_tokens` |
| `message.usage.cacheWrite` | `cache_creation_input_tokens` |
| `message.usage.output` | `output_tokens` |
| — | `reasoning_output_tokens` 恒 0；`total_tokens` = 四桶和 |

**关键点**：`totalTokens` 只用于行级预筛，**从不入库**（`src/lib/rollout.js:2055`）：

```js
    // Fast-path filter: OpenClaw assistant messages include message.usage.totalTokens.
    if (!line.includes('"usage"') || !line.includes("totalTokens")) continue;
```

**模型**：`normalizeModelInput(msg.model) || DEFAULT_MODEL`（`src/lib/rollout.js:2074`）。

**时间戳**：`obj.timestamp`（`src/lib/rollout.js:2071`），缺失即跳过。

**游标**：字节偏移 `prev.offset`（`src/lib/rollout.js:1710`）+ 事件身份计数表（`src/lib/rollout.js:2114`）：

```js
    const priorCount = Number(priorCounts[eventIdentity.key] || 0);
    const nextCount = Number(scanCounts[eventIdentity.key] || 0) + 1;
    ...
    usageEvents[eventIdentity.key] = eventIdentity.unique
      ? 1
      : Math.max(Number(usageEvents[eventIdentity.key] || 0), nextCount);
```

另有时间闸门 `aggregateAfterMs`（`src/lib/rollout.js:2128`）。**项目维度不支持**（`src/lib/rollout.js:2141`：`// Project-level OpenClaw attribution is not supported yet (no stable cwd info).`）。

**$0 风险**：无（`isAllZeroUsage(delta)` 丢弃，`src/lib/rollout.js:2109`）。

### 3.4 Kiro（`parseKiroIncremental`，`src/lib/rollout.js:7472`）

**source**：硬编码 `"kiro"`（`src/lib/rollout.js:7567`）。

**路径**：`%APPDATA%\Kiro\User\globalStorage\kiro.kiroagent`（suffix 见 `src/lib/rollout.js:7282`）→ `dev_data/devdata.sqlite` 与 `dev_data/tokens_generated.jsonl`（`src/lib/rollout.js:7298`～`7304` 的 `resolveKiroDbPath` / `resolveKiroJsonlPath`）：

```js
function resolveKiroDbPath(basePath) {
  return path.join(basePath || resolveKiroBasePath(), "dev_data", "devdata.sqlite");
}

function resolveKiroJsonlPath(basePath) {
  return path.join(basePath || resolveKiroBasePath(), "dev_data", "tokens_generated.jsonl");
}
```

**判定**：被动扫描；sync 对“无数据安装”提前跳过（`src/commands/sync.js:2315`）。

**字段映射**（`src/lib/rollout.js:7545`）：

```js
    const inputTokens = toNonNegativeInt(row.tokens_prompt);
    const outputTokens = toNonNegativeInt(row.tokens_generated);
    if (inputTokens === 0 && outputTokens === 0) continue;
```

| 列 | 桶 |
|----|----|
| `tokens_prompt` | `input_tokens` |
| `tokens_generated` | `output_tokens` |
| — | `cached_input_tokens` / `reasoning_output_tokens` 写死 0；`cache_creation_input_tokens` 键缺失 |

**模型**（`src/lib/rollout.js:7555`）：`.chat` 时间线反查，兜底 `"kiro-agent"`。

**时间戳**（`src/lib/rollout.js:7549`）：

```js
    const ts = row.timestamp ? row.timestamp.replace(" ", "T") + "Z" : null;
    const bucketStart = ts ? toUtcHalfHourStart(ts) : null;
```

显式按 UTC 解释 naive 串（SQLite `CURRENT_TIMESTAMP`）。

**游标**：自增 `id` 水位（`src/lib/rollout.js:7541`、`src/lib/rollout.js:7572`）。JSONL 支路用行号（`countKiroJsonlLines`，`src/lib/rollout.js:7338`）。

**$0 风险**：无。

### 3.5 Hermes（`parseHermesIncremental`，`src/lib/rollout.js:7900`）

**source**：`"hermes"`（`src/lib/rollout.js:8018`）。

**路径**：`TOKENTRACKER_HERMES_HOME` → `~/.hermes` / `%LOCALAPPDATA%\hermes`（`src/lib/rollout.js:7604`、`src/lib/rollout.js:7610`）：

```js
  const home = require("node:os").homedir();
  const defaultPath = path.join(home, ".hermes");
  if (process.platform === "win32") {
    const localAppData = typeof env.LOCALAPPDATA === "string" ? env.LOCALAPPDATA.trim() : "";
    const nativeValue = localAppData.length > 0 ? path.join(localAppData, "hermes") : null;
```

DB：`state.db`，外加 `profiles/*/state.db`（`src/lib/rollout.js:7659`、`src/lib/rollout.js:7676`）。

**判定**：被动扫描；UNC 路径先本地快照（`src/lib/rollout.js:7722`）。

**字段映射**：SQL 列名即字段名（`src/lib/rollout.js:7718`）：

```js
  const sql = `SELECT id, model, started_at, ended_at, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, message_count FROM sessions WHERE (started_at >= ${since} OR ended_at IS NULL${forceIncludeSql}) AND (input_tokens > 0 OR output_tokens > 0 OR cache_read_tokens > 0 OR cache_write_tokens > 0 OR reasoning_tokens > 0) ORDER BY started_at ASC`;
```

| 列 | 桶（差分） |
|----|-----------|
| `input_tokens` | `input_tokens` |
| `cache_read_tokens` | `cached_input_tokens` |
| `cache_write_tokens` | `cache_creation_input_tokens` |
| `output_tokens` | `output_tokens` |
| `reasoning_tokens` | `reasoning_output_tokens` |
| `message_count` | **`conversation_count`** |

落桶（`src/lib/rollout.js:8008`）：

```js
      const delta = {
        input_tokens: dInput,
        cached_input_tokens: dCacheRead,
        cache_creation_input_tokens: dCacheWrite,
        output_tokens: dOutput,
        reasoning_output_tokens: dReasoning,
        total_tokens: dInput + dOutput + dCacheRead + dCacheWrite + dReasoning,
        conversation_count: dMessageCount,
      };
```

**模型**：`normalizeModelInput(row.model) || "hermes-agent"`（`src/lib/rollout.js:8006`）。

**时间戳**（`src/lib/rollout.js:7999`）：

```js
      // Prefer ended_at for bucket placement; fall back to started_at
      const epochSec = endedAt ?? startedAt;
```

**注意**：桶位置取 `ended_at`，长会话的全部增量会堆在结束时刻的桶。

**游标**：`started_at` 水位 + 未完成会话强制重读（`src/lib/rollout.js:7707`、`src/lib/rollout.js:7708`）。

**$0 风险**：无（SQL `WHERE` 要求至少一列 > 0）。

### 3.6 KiroCli（`parseKiroCliIncremental`，`src/lib/rollout.js:8886`）

**source**：**`"kiro"`**（`src/lib/rollout.js:9191`、`src/lib/rollout.js:9210`）。

**关键事实（R1 复核后改写；原「本家没有真实 token 字段，全部按字符数 ÷ 4 估算」已被推翻）**——三条支路来源不同，只有两条是纯字符估算：

- **legacy 会话 `.json` 支路优先读真实用量**：先取 `turn.input_token_count` / `turn.output_token_count`（`src/lib/rollout.js:8371`、`src/lib/rollout.js:8372`），**只有两者皆为 0 才回退**字符估算（闸门在 `src/lib/rollout.js:8374`）：

```js
// Prefer real token counts if kiro-cli populated them.
let inputTokens = toNonNegativeInt(turn.input_token_count);
let outputTokens = toNonNegativeInt(turn.output_token_count);

if (inputTokens === 0 && outputTokens === 0) {
  // Fall back to char-count approximation. ...
  inputTokens = Math.floor(promptChars / KIRO_CLI_CHARS_PER_TOKEN);      // 8386
  outputTokens = Math.floor(assistantChars / KIRO_CLI_CHARS_PER_TOKEN);  // 8387
}
```

- **SQLite `data.sqlite3` 支路为纯字符估算**（常量 `src/lib/rollout.js:8118`；使用点 `src/lib/rollout.js:9152`～`9159`）：

```js
const KIRO_CLI_CHARS_PER_TOKEN = 4;
```
```js
    const approxInput = Math.floor(promptChars / KIRO_CLI_CHARS_PER_TOKEN);
    const approxOutput = Math.floor(responseChars / KIRO_CLI_CHARS_PER_TOKEN);
    const approxReasoning = Math.floor(
      reasoningChars / KIRO_CLI_CHARS_PER_TOKEN,
    );
```

- **v2 `messages.jsonl` 支路为纯字符估算**：该文件不含 token 字段（注释 `src/lib/rollout.js:8475`～`8481`），沿用同一 4 字符/token。
- **常量使用点**：定义 `8118`；真正的乘法使用点为 `8386`、`8387`、`8429`、`8430`、`9155`（原分片只列 `8118`/`9155`；R1 补漏时把 `8428` 当使用点是错的——`8428` 是注释行 `// KIRO_CLI_CHARS_PER_TOKEN; bypass that by pre-multiplying here.`，实际做乘法的是 `8429`/`8430`）。

**路径**：`kiro-cli/data.sqlite3`，三平台分叉（`src/lib/rollout.js:8132`、`src/lib/rollout.js:8138`、`src/lib/rollout.js:8140`）；会话文件目录由 `resolveKiroCliSessionFiles` 给出（`src/lib/rollout.js:8165`），根为 `KIRO_HOME || ~/.kiro`（`src/lib/rollout.js:8960`）。

**判定**：被动扫描。

**落桶**（`src/lib/rollout.js:9209`）：

```js
    if (approxInput > 0 || approxOutput > 0 || approxReasoning > 0) {
      const bucket = getHourlyBucket(hourlyState, "kiro", model, bucketStart);
      addTotals(bucket.totals, {
        input_tokens: approxInput,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: approxOutput,
        reasoning_output_tokens: approxReasoning,
        total_tokens: approxInput + approxOutput + approxReasoning,
        conversation_count: 1,
      });
```

**credits 不进 token 桶**：只写侧车 `kiro-credits.json`（`src/lib/rollout.js:8119`、`src/lib/rollout.js:8955` 的 `writeKiroCliCreditsSidecar({ ..., totalCredits: kiroCreditTotal })`）。

**模型**（`src/lib/rollout.js:9167`）：`r.model_id || r.session_model_id` → 规范化 → 兜底 `"kiro-cli-agent"`。

**时间戳**（`src/lib/rollout.js:9161`）：`request_start_timestamp_ms`，非正即跳过。

**游标：per-request 指纹 + 先减旧再加新**（`src/lib/rollout.js:9172`）：

```js
    const fingerprint =
      `${promptChars}:${responseChars}:${reasoningChars}:${model}:${tsMs}`;
    const prev = requestState[requestId];
    if (prev && prev.fingerprint === fingerprint) continue; // unchanged
```

回滚是把**负数 delta** 交给 `addTotals`（`src/lib/rollout.js:9192`），`conversation_count` 随之 −1（`src/lib/rollout.js:9203`）。另有一条“会话文件 → SQLite 迁移后撤回”的通道（`src/lib/rollout.js:9036`）。

**会话支路的零值闸门（R1 复核新增）**：legacy 会话支路只检 4 列、**不含 reasoning**（`src/lib/rollout.js:9414`）：

```js
      const reasoning = toNonNegativeInt(turn.reasoning_output_tokens);
      if (input === 0 && output === 0 && cacheRead === 0 && cacheCreation === 0) {
        maxIndex = i;
        continue;
      }
```

紧随其后的 delta 含 `total_tokens: input + output + cacheRead + cacheCreation + reasoning`（`src/lib/rollout.js:9446`）：

```js
      const delta = {
        input_tokens: input,
        cached_input_tokens: cacheRead,
        cache_creation_input_tokens: cacheCreation,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        total_tokens: input + output + cacheRead + cacheCreation + reasoning,
        conversation_count: 1,
      };
```

→ 可产出 `input=0 / output=0 / reasoning>0 / total>0` 的行，与 §5 B 类的 Goose 同形。

**$0 风险：低（不是 $0）**——上述 reasoning-only 行在 `src/lib/pricing/index.js:248-256` 下**按 output 单价计费**（`kiro` 不属于「推理并入 output」的五家集合），故不为 $0；SQLite 支路有 `approxInput > 0 || approxOutput > 0 || approxReasoning > 0` 闸门（`src/lib/rollout.js:9209`），六列全零 + `total>0` 的形态不出现。

### 3.7 Kimi（`parseKimiIncremental`，`src/lib/rollout.js:9528`）

**source**：硬编码 `"kimi"`（`src/lib/rollout.js:9611`）。

**路径**：`~/.kimi/sessions/<workspace>/<session>/wire.jsonl`（`src/lib/rollout.js:9501`）：

```js
  const sessionsDir = path.join(kimiHome, "sessions");
  ...
      for (const sess of fssync.readdirSync(wsDir)) {
        const wireFile = path.join(wsDir, sess, "wire.jsonl");
        if (fssync.existsSync(wireFile)) files.push(wireFile);
```

**判定**：被动扫描；`src/lib/rollout.js:8078` 注释直说：`// No hook installation needed; Kimi writes wire.jsonl automatically.`

**字段映射**（`src/lib/rollout.js:9586`）：

```js
      const input = toNonNegativeInt(token_usage.input_other);
      const output = toNonNegativeInt(token_usage.output);
      const cacheRead = toNonNegativeInt(token_usage.input_cache_read);
      const cacheCreation = toNonNegativeInt(token_usage.input_cache_creation);
      if (input === 0 && output === 0 && cacheRead === 0 && cacheCreation === 0) {
        seenIds.add(message_id);
        continue;
      }
```

| 字段 | 桶 |
|------|----|
| `msg.payload.token_usage.input_other` | `input_tokens` |
| `msg.payload.token_usage.input_cache_read` | `cached_input_tokens` |
| `msg.payload.token_usage.input_cache_creation` | `cache_creation_input_tokens` |
| `msg.payload.token_usage.output` | `output_tokens` |
| — | reasoning 恒 0；total 重算 |

仅处理 `message.type === "StatusUpdate"` 且带 `token_usage`/`message_id` 的记录（`src/lib/rollout.js:9575`）。

**模型**：**来自配置而非日志**（`src/lib/rollout.js:9540`：`model || resolveKimiDefaultModel(env || process.env)`），兜底 `"kimi-for-coding"`（`src/lib/rollout.js:8080`）。

**时间戳**：`entry.timestamp ?? payload.timestamp`（**秒**），缺失即跳过（`src/lib/rollout.js:9595`）。

**游标**：文件 `size`/`ino` 偏移 + `message_id` 集合，两者都持久化（`src/lib/rollout.js:9557`、`src/lib/rollout.js:9542`）：

```js
    const prevSize = Number(prevEntry.size) || 0;
    const prevIno = prevEntry.ino;
    const inodeChanged = typeof prevIno === "number" && prevIno !== stat.ino;
    const startOffset = stat.size < prevSize || inodeChanged ? 0 : prevSize;
```

**$0 风险**：无。

### 3.8 KimiCode（`parseKimiCodeIncremental`，`src/lib/rollout.js:9723`）

**source**：**`"kimi"`**（`src/lib/rollout.js:9844`）。

**路径**：`~/.kimi-code/sessions/**` 递归收集 `wire.jsonl`，深度 ≤ 5（`src/lib/rollout.js:9677`）。

**判定**：被动扫描。

**事件筛选**（`src/lib/rollout.js:9775`、`src/lib/rollout.js:9782`）：`step.end` 且带 `usage` 与 `evt.uuid`；`config.update` 只更新模型。

**字段映射：三种历史形状**（`src/lib/rollout.js:9802`）：

```js
      if (usage.inputOther != null) {
        input = toNonNegativeInt(usage.inputOther);
        cacheRead = toNonNegativeInt(usage.inputCacheRead);
        cacheCreation = toNonNegativeInt(usage.inputCacheCreation);
        output = toNonNegativeInt(usage.output);
      } else {
        cacheCreation = toNonNegativeInt(usage.cache_creation_input_tokens);
        if (usage.cache_read_input_tokens != null) {
          cacheRead = toNonNegativeInt(usage.cache_read_input_tokens);
          input = toNonNegativeInt(usage.input_tokens);
        } else {
          ...
          input = Math.max(0, toNonNegativeInt(usage.input_tokens) - cached);
        }
        output = toNonNegativeInt(usage.output_tokens);
      }
```

| 形状 | input / cached / creation / output |
|------|-----------------------------------|
| 新版 camelCase（`inputOther` 存在） | `inputOther` / `inputCacheRead` / `inputCacheCreation` / `output` |
| Anthropic 风格 | `input_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` / `output_tokens` |
| OpenAI 兼容 | `input_tokens − details.cached_tokens` / `details.cached_tokens` / `cache_creation_input_tokens` / `output_tokens` |

只读 `step.end`，避免与 `usage.record` 双计（注释 `src/lib/rollout.js:9796`）。

**模型**：`config.update.modelAlias` 覆盖（`src/lib/rollout.js:9769`），默认来自 `~/.kimi-code/config.toml`，兜底 `"kimi-for-coding"`（`src/lib/rollout.js:9704`，函数头 `resolveKimiCodeDefaultModel` 在 `9703`）。

**时间戳**：`entry.time ?? evt.time`（**毫秒**），缺失即跳过（`src/lib/rollout.js:9828`）。

**游标**：仅 `seenIds`（uuid），**无字节偏移**（`src/lib/rollout.js:9783`）。

**$0 风险**：无（四桶全 0 提前跳过，`src/lib/rollout.js:9823`）。

### 3.9 Codebuddy（`parseCodebuddyIncremental`，`src/lib/rollout.js:10164`）

**source**：`"codebuddy"`（`src/lib/rollout.js:10384`，read 确认）。

**路径：双源设计**。主源 `~/.codebuddy/projects/**/*.jsonl`（`CODEBUDDY_HOME` 覆盖；`src/lib/rollout.js:10028`、`src/lib/rollout.js:10035`）：

```js
function resolveCodebuddyProjectFiles(env = process.env) {
  const codebuddyHome = resolveCodebuddyHome(env);
  const jsonlFiles = [];
  const logFiles = [];

  // 1. Recursive JSONL scan in codebuddyHome/projects
  if (codebuddyHome) {
    const projectsDir = path.join(codebuddyHome, "projects");
```

兜底源为 IDE 扩展日志，平台分叉（`src/lib/rollout.js:10069`）：

```js
    logRoots.push({ dir: path.join(appData, "CodeBuddy CN", "logs"), pattern: "codebuddy-extension-log" });
    logRoots.push({ dir: path.join(appData, "Code", "logs"), pattern: "codebuddy-extension-log" });
    logRoots.push({ dir: path.join(localAppData, "CodeBuddyExtension", "Logs", "CodeBuddyIDE"), pattern: "*.log" });
```

**判定：官方目录 + hook 双轨**。它属于 hook 名单（`src/lib/passive-mode.js:156`），安装点在 `src/commands/init.js:577`（read 确认）：

```js
    ["codebuddy", context.codebuddyDir, () => upsertClaudeHook({
      settingsPath: context.codebuddySettingsPath,
      hookCommand: context.codebuddyHookCommand,
```

CodeBuddy 是 Claude Code 的 fork，复用同一套 `settings.json` hook（`src/commands/init.js:623` 注释）。

**扩展日志字段映射**（`src/lib/rollout.js:10278`、`src/lib/rollout.js:10293`）：

```js
        const marker = "Agent execution successful with usage:";
        const usageParts = line.split(marker);
```
```js
        const inputRaw = firstPresentNonNegativeInt([
          usage.cachedMissTokens,
          usage.cacheMissTokens,
          usage.input_tokens,
          usage.inputTokens,
          usage.prompt_tokens,
        ]);
```

非缓存输入修正（`src/lib/rollout.js:10337`～`10343`）：

```js
        const isFromTotalField =
          usage.cachedMissTokens === undefined &&
          usage.cacheMissTokens === undefined &&
          usage.prompt_cache_miss_tokens === undefined;
        if (isFromTotalField && cacheRead > 0) {
          inputTokens = Math.max(0, inputTokens - cacheRead);
        }
```

| 字段（按优先级） | 桶 |
|------------------|----|
| `cachedMissTokens` / `cacheMissTokens` / `input_tokens` / `inputTokens` / `prompt_tokens` | `input_tokens` |
| `output_tokens` / `outputTokens` / `completion_tokens` | `output_tokens` |
| `cache_read_input_tokens` / `cacheReadInputTokens` / `cacheTokens` / `prompt_cache_hit_tokens` / `cached_tokens` | `cached_input_tokens` |
| `cache_creation_input_tokens` / `cacheCreationInputTokens` / `cachedWriteTokens` / `prompt_cache_write_tokens` | `cache_creation_input_tokens` |

落桶（`src/lib/rollout.js:10374`）：

```js
        const delta = {
          input_tokens: inputTokens,
          cached_input_tokens: cacheRead,
          cache_creation_input_tokens: cacheCreation,
          output_tokens: completionTokens,
          reasoning_output_tokens: reasoningTokens,
          total_tokens: inputTokens + completionTokens + cacheRead + cacheCreation + reasoningTokens,
          conversation_count: 1,
        };
```

**模型**（`src/lib/rollout.js:10355`）：`modelsByAgent` → `logModelsByAgent` → `fallbackModel`；兜底 `"codebuddy-unknown"`（`src/lib/rollout.js:9924`，函数头 `resolveCodebuddyDefaultModel` 在 `9923`）。

**游标**：`seenIds` + 用量指纹（`src/lib/rollout.js:10360`、`src/lib/rollout.js:10369`）：

```js
        const fingerprint = codebuddyUsageFingerprint({
          model,
          timestampMs: tsMs,
          inputTokens,
          cacheRead,
          cacheCreation,
          outputTokens: completionTokens,
          reasoningTokens,
        });
        if (consumeCodebuddyLogFingerprint(usageFingerprints, fingerprint)) {
```

指纹表有 10k 上限（`capCodebuddyUsageFingerprints`，`src/lib/rollout.js:10022`）。

**$0 风险：无（R1 复核结案，原「未判定」作废）**——两条支路都有闸门：

- **扩展日志支路**要求 input/completion/cacheRead/cacheCreation 至少一个 > 0（`src/lib/rollout.js:10345`；该判定**不含 reasoning**，故“只有 reasoning”的行会被丢）。
- **JSONL 支路**（`providerData.rawUsage`）有含 `reasoningTokens` 的五桶零闸门（`src/lib/rollout.js:10476`～`10485`）：

```js
        if (
          inputTokens === 0 &&
          completionTokens === 0 &&
          cacheRead === 0 &&
          cacheCreation === 0 &&
          reasoningTokens === 0
        ) {
          seenIds.add(messageId);
          continue;
        }
```

结论：**纯 total 行被静默丢弃，不产 $0 行** → 归 §5 D 类，F 类清空。

### 3.10 Workbuddy（`parseWorkbuddyIncremental`，`src/lib/rollout.js:10803`）

**source：唯一随 variant 变的实现**（`src/lib/rollout.js:10651`）：

```js
const WORKBUDDY_VARIANTS = {
  workbuddy: { dir: ".workbuddy", envVar: "WORKBUDDY_HOME" },
  "workbuddy-ai": { dir: ".workbuddy-ai", envVar: "WORKBUDDY_AI_HOME" },
};
```

落桶用 variant 当 source（`src/lib/rollout.js:11004`）：`getHourlyBucket(hourlyState, variant, model, bucketStart)`；调用侧传 `source: "workbuddy-ai"`（`src/commands/sync.js:2679`）。

**路径**（`src/lib/rollout.js:10662`）：

```js
function resolveWorkbuddyHome(env = process.env, variant = "workbuddy") {
  const spec = WORKBUDDY_VARIANTS[resolveWorkbuddyVariant(variant)];
  const home = env.HOME || env.USERPROFILE || require("node:os").homedir();
  const configured = expandHome(env[spec.envVar], home);
  if (configured) return configured;
```

三类数据源（`src/lib/rollout.js:10694`）：

```js
// Recursively collect every *.jsonl under ~/.workbuddy/projects so that
// per-session conversation logs AND their nested subagents/agent-*.jsonl files
// are both discovered. WorkBuddy also writes completed trace summaries under
// ~/.workbuddy/traces/<pid>/trace_*.json. Those summaries are useful only when
// the detailed JSONL for a session is absent, ...
```

**判定：官方目录 + hook 双轨**（`src/lib/passive-mode.js:166`、`src/lib/passive-mode.js:177`；安装点 `src/commands/init.js:581`、`src/commands/init.js:585`，read 确认）。

**字段映射**：入口是 `providerData.rawUsage`（`src/lib/rollout.js:10907`）：

```js
      const provider = entry.providerData;
      const rawUsage = provider && typeof provider === "object" ? provider.rawUsage : null;
      if (!rawUsage || typeof rawUsage !== "object") continue;
```

```js
      const cacheRead = Math.max(
        toNonNegativeInt(rawUsage.cache_read_input_tokens),
        toNonNegativeInt(promptDetails.cached_tokens),
        toNonNegativeInt(rawUsage.prompt_cache_hit_tokens),
      );
      const cacheCreation = toNonNegativeInt(rawUsage.cache_creation_input_tokens);
      // prompt_tokens is the FULL prompt: subtract BOTH reads and writes so
      // input_tokens is pure non-cached input (no double-counting cache writes).
      const inputTokens = Math.max(0, promptTokens - cacheRead - cacheCreation);
```

| 字段 | 桶 | 备注 |
|------|----|------|
| `rawUsage.prompt_tokens` − cacheRead − cacheCreation | `input_tokens` | |
| 三镜像取最大 | `cached_input_tokens` | `cache_read_input_tokens` / `prompt_tokens_details.cached_tokens` / `prompt_cache_hit_tokens` |
| `rawUsage.cache_creation_input_tokens` | `cache_creation_input_tokens` | |
| `rawUsage.completion_tokens` − reasoning | `output_tokens` | reasoning 从 completion 拆出 |
| `completion_tokens_details.reasoning_tokens` | `reasoning_output_tokens` | |

**模型**（`src/lib/rollout.js:10987`，read 确认）：`provider.model` → `provider.requestModelId` → `entry.model` → `fallbackModel`；兜底 `"auto"`（`src/lib/rollout.js:10678`，函数头 `resolveWorkbuddyDefaultModel` 在 `10677`），且 `auto` 会逐字上报（`src/lib/rollout.js:10632` 注释）。

**时间戳**：`entry.timestamp`（毫秒），缺失即跳过（`src/lib/rollout.js:10919`、`src/lib/rollout.js:10979`，read 确认）。

**游标**：`seenIds`（消息 ID，`src/lib/rollout.js:11007`）+ 旧版 entry-id 双写（`src/lib/rollout.js:11008` 注释）；`detailedSessions` 压制 traces 兜底（`src/lib/rollout.js:11013`）。

**$0 风险**：无（五桶全 0 跳过，`src/lib/rollout.js:10968`，read 确认）。

**traces 兜底支路（原 U1，R2 结案）**：字段与 JSONL 支路**不同形**——取 `modelInfo.totalInputTokens` / `totalOutputTokens` / `totalCachedTokens`（`src/lib/rollout.js:11085`～`11087`，cached 先 clamp 到 totalInput）+ `startedAtRaw`（`11081`，秒/毫秒自适应）+ `modelInfo.models[0]`（`11103`）；`input_tokens = totalInput − totalCached`（`11105`），`cache_creation_input_tokens` 与 `reasoning_output_tokens` 恒 0（`11109`、`11111`），`total_tokens = inputTokens + totalCached + totalOutput`（`11112`），`conversation_count: 1`（`11113`），落桶 `11115`、`11117`。原文「疑为 traces 的 `11246`」实属**第三条 SQLite 支路**（`dbExists` 分支自 `11134` 起，`session_usage` 查询的 `total_tokens` 含 `reasoningTokens`）。三支路优先级：JSONL > traces > SQLite（`11088`、`11164`～`11168` 逐级压制）。

### 3.11 Roocode（`parseRoocodeIncremental`，`src/lib/rollout.js:11824`）

**source**：`"roocode"`（`src/lib/rollout.js:11939`）。

**路径**（`src/lib/rollout.js:11767`）：复用 Kilocode 的 IDE 根列表：

```js
function resolveRoocodeTaskFiles(env = process.env) {
  const roots = resolveKilocodeRoots(env);
  const out = [];
  for (const root of roots) {
    const tasksDir = path.join(root, "User", "globalStorage", "rooveterinaryinc.roo-cline", "tasks");
```

**判定**：被动扫描。

**字段映射**（`src/lib/rollout.js:11906`）：

```js
      const tokensIn = toNonNegativeInt(payload.tokensIn);
      const tokensOut = toNonNegativeInt(payload.tokensOut);
      const cacheReads = toNonNegativeInt(payload.cacheReads);
      const cacheWrites = toNonNegativeInt(payload.cacheWrites);
```

| payload 字段 | 桶 |
|--------------|----|
| `tokensIn` | `input_tokens` |
| `cacheReads` | `cached_input_tokens` |
| `cacheWrites` | `cache_creation_input_tokens` |
| `tokensOut` | `output_tokens` |
| — | reasoning 恒 0 |

入口只认 `msg.say` ∈ {`api_req_started`, `api_req_deleted`}（`src/lib/rollout.js:11892`），零值占位**故意不入 seen** 以便回填（`src/lib/rollout.js:11910` 注释）。

**模型**：真实模型藏在兄弟文件 `api_conversation_history.json` 的 `<environment_details><model>` 里（`src/lib/rollout.js:11749` 注释；正则 `src/lib/rollout.js:11805`）；三级兜底（`src/lib/rollout.js:11814`）：真实模型 → `protocol:<apiProtocol>` → `"unknown"`。

**时间戳**：`msg.ts` 毫秒（`src/lib/rollout.js:11899`）。**游标**：`taskUuid:ts`（`src/lib/rollout.js:11902`）+ 文件 mtime 闸门。

**$0 风险**：无。

### 3.12 Cline（`parseClineIncremental`，`src/lib/rollout.js:12200`）

**source**：`"cline"`（`src/lib/rollout.js:12396`）。

**路径**：`resolveClineSessionsDirs`（`src/lib/rollout.js:12054`，含 Windows 双安装/WSL 探测）→ `scanClineSessionFiles`（`src/lib/rollout.js:12086`）；带错误/完成态（`src/lib/rollout.js:12136`）：

```js
function resolveClineSessionFilesWithStatus(env = process.env, deps = {}) {
  const out = [];
  const seen = new Set();
  const completedRoots = [];
  const errors = [];
  for (const sessionsDir of resolveClineSessionsDirs(env, deps)) {
    const result = scanClineSessionFiles(sessionsDir);
```

**判定**：被动扫描。

**字段映射**（`src/lib/rollout.js:12356`）：

```js
        const cacheRead = toNonNegativeInt(metrics.cacheReadTokens);
        const cacheWrite = toNonNegativeInt(metrics.cacheWriteTokens);
        const inclusiveInput = toNonNegativeInt(metrics.inputTokens);
        const inputTokens = Math.max(0, inclusiveInput - cacheRead - cacheWrite);
        // Reasoning is a subset of outputTokens; it is reported, never added to
        // total_tokens and never billed on top of output.
        const outputTokens = toNonNegativeInt(metrics.outputTokens);
        const reasoningTokens = toNonNegativeInt(metrics.reasoningTokenCount);
        const cost = toNonNegativeNumber(metrics.cost);
```

| metrics 字段 | 桶 | 备注 |
|--------------|----|------|
| `inputTokens` − cacheRead − cacheWrite | `input_tokens` | 该字段含缓存 |
| `cacheReadTokens` / `cacheWriteTokens` | `cached_input_tokens` / `cache_creation_input_tokens` | |
| `outputTokens` | `output_tokens` | **含** reasoning |
| `reasoningTokenCount` | `reasoning_output_tokens` | 只上报，不加入 total |
| `cost` | `total_cost_usd`（差分） | 19 家中唯一解析器自产成本 |

**模型**：`msg.modelInfo.id` → 会话 sidecar `model` → `provider:<name>` → `DEFAULT_MODEL`（`src/lib/rollout.js:12178` 的 `normalizeClineModel`，四级兜底链 `12179`～`12189`）。

**时间戳**：`msg.ts`，另有导入闸门（`src/lib/rollout.js:12350`）：

```js
        if (importedAtMs !== null && ts <= importedAtMs) continue;
```

**游标**：三键兼容（`src/lib/rollout.js:12369`）+ **五路逐指标差分**（`src/lib/rollout.js:12383`）：

```js
        const deltaInput = Math.max(0, inputTokens - (Number(previous?.input) || 0));
        const deltaCached = Math.max(0, cacheRead - (Number(previous?.cached_input) || 0));
        const deltaCreation = Math.max(0, cacheWrite - (Number(previous?.cache_creation) || 0));
        const deltaOutput = Math.max(0, outputTokens - (Number(previous?.output) || 0));
        const deltaReasoning = Math.max(0, reasoningTokens - (Number(previous?.reasoning) || 0));
```

`conversation_count = previous ? 0 : 1`（`src/lib/rollout.js:12405`）——只计首次。

**$0 风险**：无（`totalTokens` 由分桶重算，`src/lib/rollout.js:12381` 的跳过条件含 `cost`）。

### 3.13 Zed（`parseZedIncremental`，`src/lib/rollout.js:12650`）

**source**：`"zed"`（`src/lib/rollout.js:12782`）。

**路径**（`src/lib/rollout.js:12490`）：

```js
  if (process.platform === "darwin") {
    return path.join(home, "Library", "Application Support", "Zed", "threads", "threads.db");
  }
  if (process.platform === "win32") {
    const local = env.LOCALAPPDATA || path.join(home, "AppData", "Local");
    const native = path.join(local, "Zed", "threads", "threads.db");
```

**判定**：被动扫描；Windows 探测 WSL（`src/lib/rollout.js:12501`），sync 侧双安装合并（`src/commands/sync.js:2062`）。

**格式**：`threads` 表 BLOB，`data_type` 为 `json` 或 `zstd`（`src/lib/rollout.js:12518`）：

```js
  if (type === "zstd") {
    const zlib = require("node:zlib");
    const out =
      typeof zlib.zstdDecompressSync === "function"
        ? zlib.zstdDecompressSync(data)
        : Buffer.from(await require("@mongodb-js/zstd").decompress(data));
```

**字段映射**（`src/lib/rollout.js:12553`）：

```js
  return {
    input: coerce(value.input_tokens),
    output: coerce(value.output_tokens),
    cache_read: coerce(value.cache_read_input_tokens),
    cache_write: coerce(value.cache_creation_input_tokens),
  };
```

取值优先级：`thread.request_token_usage`（逐请求和）→ `thread.cumulative_token_usage`（`src/lib/rollout.js:12597`、`src/lib/rollout.js:12601`）。

| 线程字段 | 桶 |
|----------|----|
| `input_tokens` | `input_tokens` |
| `cache_read_input_tokens` | `cached_input_tokens` |
| `cache_creation_input_tokens` | `cache_creation_input_tokens` |
| `output_tokens` | `output_tokens` |
| — | reasoning 恒 0 |

**模型**：`thread.model.model`（`src/lib/rollout.js:12594`）；跳过 `thread.imported === true`（`src/lib/rollout.js:12585`）与 `ZED_DOUBLE_COUNTED_PROVIDERS`（`src/lib/rollout.js:12593`）。

**时间戳**（`src/lib/rollout.js:12764`）：

```js
    const tsIso =
      (typeof row.updated_at === "string" && row.updated_at) ||
      (typeof row.created_at === "string" && row.created_at) ||
      (typeof thread.updated_at === "string" && thread.updated_at) ||
      new Date().toISOString();
```

末级兜底是 **now**。

**游标**：SQL `updated_at > cursor`（`src/lib/rollout.js:12636`）+ 线程累计差分（`src/lib/rollout.js:12742` 的 `isReset = currSum > 0 && currSum < prevSum`；`12732` 是差分基线 `const prev = threadTotals[row.id] || {…}`，**不是** `isReset`）；`threadTotals` 上限 10k（`src/lib/rollout.js:12816`）。

**$0 风险**：无（六列全零 + `total>0` 不成立）。**限定（R1 复核 I2）**：`totalDelta` 只由 input/output/cache_read/cache_write 求和（`src/lib/rollout.js:12751`）并在 `totalDelta <= 0` 时跳过（`12752`），因此**只有缓存列增长**时仍会出现 `input=0 / output=0 / total>0` 的行——“结构上不可能”不等于“不可能出现 input/output = 0”。

### 3.14 Lmstudio（`parseLmstudioIncremental`，`src/lib/rollout.js:13267`）

**source**：`src/lib/rollout.js:12852`：`const LMSTUDIO_SOURCE = "lmstudio";`

**路径**：`TOKENTRACKER_LMSTUDIO_HOME` / `LM_STUDIO_HOME` / `~/.lmstudio`（`src/lib/rollout.js:12859`）+ `server-logs` 递归 `*.log`（`src/lib/rollout.js:12871`）：

```js
async function resolveLmstudioLogFiles(env = process.env) {
  const root = path.join(resolveLmstudioHome(env), "server-logs");
  ...
      else if (entry.isFile() && entry.name.toLowerCase().endsWith(".log")) files.push(fullPath);
```

**判定**：被动扫描。

**格式**：文本日志内嵌 JSON（`src/lib/rollout.js:12845` 注释，**属 LM Studio 模块，与 Zed 无关**：`12843` 的 `// LM Studio` 段头、`12845` 的 `pretty-printed OpenAI-compatible final responses`）；解析靠 `"usage"` 字节标记 + 花括号配平（`src/lib/rollout.js:12897`、`src/lib/rollout.js:12917`），带 8MB 窗口 / 64KB 分块上限（`src/lib/rollout.js:12853` 的 `LMSTUDIO_WINDOW_BYTES` / `12854` 的 `LMSTUDIO_CHUNK_BYTES`）。

**字段映射**：复用 `normalizeLocalStudioTokens`（见 §1.5，`src/lib/rollout.js:12994`）：

| 字段（多镜像） | 桶 |
|----------------|----|
| `total_tokens`/`totalTokens` 与 `prompt+completion` 取 max | `total_tokens` |
| `prompt_tokens_details.cached_tokens` / `cache_read_tokens` / `cached_tokens` | `cached_input_tokens`（≤ prompt） |
| `prompt_tokens_details.cache_creation_input_tokens` / `cache_write_tokens` | `cache_creation_input_tokens`（≤ prompt − cacheRead） |
| `completion_tokens_details.reasoning_tokens` / `reasoning_tokens` | `reasoning_output_tokens`（≤ completion） |
| 反推 | `input_tokens = total − completion − cacheRead − cacheWrite` |

**模型**（`src/lib/rollout.js:13091`）：`lastLmstudioJsonStringField(metadataBuffer, "model") || carried.model`，兜底 `DEFAULT_MODEL`。

**时间戳**：日志行 `[YYYY-MM-DD HH:MM:SS]`（**本地时间**，`src/lib/rollout.js:12979`）→ 跨窗口携带 → **文件 mtime**（`src/lib/rollout.js:13094`、`src/lib/rollout.js:13129`）。

**游标**：文件 `dev/ino/size/mtime` 三态（`unchanged` / `canResume` / 整源重建，`src/lib/rollout.js:13325`）+ 事件 key（`lmstudio:<responseId>` 或哈希 `src/lib/rollout.js:13099`）。

**$0 风险**：无。

### 3.15 Unsloth（`parseUnslothIncremental`，`src/lib/rollout.js:13691`）

**source**：`src/lib/rollout.js:13467`：`const UNSLOTH_SOURCE = "unsloth";`；落桶经 `reconcilePassiveUsageEvent`（`src/lib/rollout.js:13730`）。

**路径**（`src/lib/rollout.js:13482`）：

```js
  if (studioHome) return path.join(path.resolve(studioHome), "studio.db");
  return path.join(env.HOME || env.USERPROFILE || os.homedir(), ".unsloth", "studio", "studio.db");
```

**判定**：被动扫描。

**字段映射**（`src/lib/rollout.js:13667`）：

```js
function normalizeUnslothUsageRow(row) {
  const totals = normalizeLocalStudioTokens({
    prompt_tokens: row?.prompt_tokens,
    completion_tokens: row?.completion_tokens,
    total_tokens: row?.total_tokens,
    cached_tokens: row?.cached_tokens,
    cache_creation_input_tokens: row?.cache_write_tokens,
    reasoning_tokens: row?.reasoning_tokens,
  });
```

`api_usage_events` 表的投影把缓存/推理列写死 0（`src/lib/rollout.js:13634`～`13636` 的 SQL：`0 AS cached_tokens, 0 AS cache_write_tokens, 0 AS reasoning_tokens`）。

**模型**（`src/lib/rollout.js:13650`）：`response_model` → `requested_model` → `fallback_model` → `DEFAULT_MODEL`，再加前缀：本地→`local/…`、未计费 provider→`unpriced/<provider>/…`、已知 provider→`<provider>/<model>`（白名单 `src/lib/rollout.js:13470`）。

**时间戳**：`coerceEpochMs(created_at) || parseIsoTimestampMs(created_at)`（`src/lib/rollout.js:13677`）。

**游标**：扫描窗口 `overlap/latest`（`created_at` + `id` 二元组）+ DB 侧车指纹；有界时才走快路径（判定在 `src/lib/rollout.js:13714`，短路返回在 `13717`），保留末 256 行（`src/lib/rollout.js:13469`）。

**$0 风险**：无。

### 3.16 Anythingllm（`parseAnythingllmIncremental`，`src/lib/rollout.js:13938`）

**source**：`"anythingllm"`（`src/lib/rollout.js:14053`）。

**路径**（`src/lib/rollout.js:13808`）：

```js
  if (platform === "win32") {
    const home = env.USERPROFILE || os.homedir();
    const appData = env.APPDATA || path.join(home, "AppData", "Roaming");
    return path.join(appData, "anythingllm-desktop", "storage", "anythingllm.db");
  }
```

**判定**：被动扫描；数据来自 `workspace_chats.response.metrics`（sync 注释 `src/commands/sync.js:1908`）。

**字段映射**（`src/lib/rollout.js:14013`、`src/lib/rollout.js:14040`）：

```js
    const inputTokens = toNonNegativeInt(row?.prompt_tokens);
    const outputTokens = toNonNegativeInt(row?.completion_tokens);
    const reportedTotal = toNonNegativeInt(row?.total_tokens);
    if (inputTokens === 0 && outputTokens === 0) {
```
```js
    const accounted = inputTokens + outputTokens;
    const reasoningTokens = Math.max(0, reportedTotal - accounted);
```

| 列 | 桶 | 备注 |
|----|----|------|
| `prompt_tokens` | `input_tokens` | |
| `completion_tokens` | `output_tokens` | |
| `total_tokens` | 反推 | `reasoning = max(0, total − input − output)` |
| — | 两个缓存桶 | 恒 0 |

**这条“差额即 reasoning”的判定是本文件里最反直觉的一处**：该家的 `reasoning_output_tokens` 语义是“未解释差额”，不保证真的是推理量。

**模型**：`row.model || "anythingllm-unknown"`（`src/lib/rollout.js:14040`）。

**时间戳**：`createdAt ?? lastUpdatedAt`，解析器同时吃 epoch 毫秒/秒、数字串、`YYYY-MM-DD HH:MM:SS`（**按 UTC**，`src/lib/rollout.js:13856`）。

**游标**：自增 `id` 水位（`src/lib/rollout.js:14010`）+ `pendingChatIds` 补扫（`src/lib/rollout.js:14024`）。

**$0 风险**：无；纯 total 行会被丢弃。

### 3.17 Goose（`parseGooseIncremental`，`src/lib/rollout.js:14604`）

**source**：`"goose"`（`src/lib/rollout.js:14730`）。

**路径**（`src/lib/rollout.js:14489`）：

```js
  const root = typeof env.GOOSE_PATH_ROOT === "string" ? env.GOOSE_PATH_ROOT.trim() : "";
  if (root) return path.join(root, "data", "sessions", "sessions.db");
```

Windows 为 `%APPDATA%\goose\sessions\sessions.db`（`src/lib/rollout.js:14503`），macOS/Linux 走 `Application Support` / `$XDG_DATA_HOME`（含 `Block/goose` 备用）；sync 侧双安装合并（`src/commands/sync.js:1993`）。

**判定**：被动扫描。

**字段映射**（`src/lib/rollout.js:14670`、`src/lib/rollout.js:14708`）：

```js
    const model = parseGooseModelName(row.model_config_json);
    if (!model) continue;

    // Prefer accumulated_*; fall back to single-turn columns.
    const totalNow = Math.max(
      0,
      Number(row.accumulated_total_tokens ?? row.total_tokens ?? 0) || 0,
    );
```
```js
    // If total grew more than (input + output), treat the excess as reasoning
    // — matches Goose's accounting (it lumps reasoning into `total_tokens`).
    const accountedDelta = dInput + dOutput;
    const reasoningDelta = Math.max(0, dTotal - accountedDelta);
```

| 列 | 桶 | 备注 |
|----|----|------|
| `accumulated_input_tokens`（回退单回合列） | `input_tokens`（差分） | |
| `accumulated_output_tokens`（回退） | `output_tokens`（差分） | |
| `accumulated_total_tokens`（回退 `total_tokens`） | 反推 reasoning | |
| `model_config_json.model_name` | 模型名 | 缺则整条丢弃 |
| — | 两个缓存桶 | 恒 0 |

**模型**：`parseGooseModelName`（`src/lib/rollout.js:14529`）**无兜底常量**，解析不到就 `continue`。

**时间戳**：`parseGooseCreatedAt(row.created_at) || new Date().toISOString()`（`src/lib/rollout.js:14713`；三形态兼容见 `src/lib/rollout.js:14541` 注释）。

**游标**：DB mtime 短路（`src/lib/rollout.js:14639`）+ 会话累计差分 + `isReset`（`src/lib/rollout.js:14683`）。

**$0 风险：低但存在**——只要 `dTotal > 0` 就落桶；若 input/output 差分为 0（只有 total 在涨），会产出 `input=0 / output=0 / reasoning>0` 的行（`src/lib/rollout.js:14686` 的 continue 条件是三者皆 0）。

### 3.18 Droid（`parseDroidIncremental`，`src/lib/rollout.js:15137`）

**source**：`"droid"`（`src/lib/rollout.js:15381`）。

**路径**（`src/lib/rollout.js:14797`）：`DROID_SESSIONS_DIR`（逗号分隔）→ `FACTORY_DIR/sessions` → `~/.factory/sessions`（Windows 带 WSL 分支）；文件筛选 `*.settings.json`（`src/lib/rollout.js:14836`）。

**判定**：被动扫描；sync 侧去重（`dedupeDroidSettingsFilesBySession`，**定义在 `src/lib/rollout.js:15069`**，调用点 `rollout.js:15165`、导出 `rollout.js:24839`；`src/commands/sync.js:6595` 只是**一条注释**（`// … The forward fix is dedupeDroidSettingsFilesBySession`），不是出现点；`src/commands/sync.js:2013` 实为 Droid 段落的文件合并起点 `const droidSettingsFiles = sourceAllowed("droid") ? mergeBothFileSources({…})`）+ `prune: true`（`src/commands/sync.js:2032`）。

**字段映射**（`src/lib/rollout.js:15272`）：

```js
    const tokenUsage = settings.tokenUsage;
    if (!tokenUsage || typeof tokenUsage !== "object") continue;

    const filled = applyDroidTotalFallback({
      input: Math.max(0, Number(tokenUsage.inputTokens || 0)),
      output: Math.max(0, Number(tokenUsage.outputTokens || 0)),
      cacheCreation: Math.max(0, Number(tokenUsage.cacheCreationTokens || 0)),
      cacheRead: Math.max(0, Number(tokenUsage.cacheReadTokens || 0)),
      thinking: Math.max(0, Number(tokenUsage.thinkingTokens || 0)),
      totalTokens: Math.max(0, Number(tokenUsage.totalTokens || 0)),
    });
```

**本家独有的“total 兜底填充”**（`src/lib/rollout.js:14972`）：

```js
function applyDroidTotalFallback(usage) {
  const known =
    usage.input + usage.output + usage.cacheCreation + usage.cacheRead + usage.thinking;
  const total = usage.totalTokens || 0;
  const missing = total > known ? total - known : 0;
  if (missing === 0) return usage;
  if (usage.output === 0) {
    return { ...usage, output: missing };
  }
  return { ...usage, thinking: usage.thinking + missing };
}
```

它把“只有 total 没有分桶”的缺口填进 `output` 或 `thinking`，**结构性规避 $0 退化**。

| settings 字段 | 桶 |
|---------------|----|
| `inputTokens` / `outputTokens` / `cacheCreationTokens` / `cacheReadTokens` | 同名四桶 |
| `thinkingTokens` | `reasoning_output_tokens` |
| `totalTokens` | 仅兜底填充 |

**模型**：`resolveDroidModel` **定义在 `src/lib/rollout.js:15046`**（体 `15046`–`15057`：`normalizeDroidModelName(settings.model)` → 空则 `extractDroidModelFromSidecarJsonl(filePath)` → 仍空则 `normalizeDroidProvider(settings.providerLock)` / `inferDroidProviderFromModel(settings.model)` → `defaultDroidModelForProvider(provider)`）；调用点 `src/lib/rollout.js:15348`，其上方注释 `15343`–`15346` 描述同一三级链与 `*-unknown` 兜底；另有 `normalizeDroidModelName`（`src/lib/rollout.js:14858`）。

**时间戳：唯一用文件 mtime 的家**（`src/lib/rollout.js:15210`、`src/lib/rollout.js:15328`）：

```js
    const bucketStart = toUtcHalfHourStart(
      new Date(mtimeMs || Date.now()).toISOString(),
    );
```

**游标**：会话累计 + mtime（`src/lib/rollout.js:15243`）；`conversation_count = isFirstSeenSession || isReset ? 1 : 0`（`src/lib/rollout.js:15379`）。

**$0 风险**：无（有显式兜底）。

### 3.19 Kilocode（`parseKilocodeIncremental`，`src/lib/rollout.js:15545`）

**source**：**`"kilo-code"`**（`src/lib/rollout.js:15651`）。

**路径**（`src/lib/rollout.js:11706`）：

```js
    const tasksDir = path.join(root, "User", "globalStorage", "kilocode.kilo-code", "tasks");
```

**判定**：被动扫描；sync 侧双安装合并（`src/commands/sync.js:1949`）。

**字段映射**（`src/lib/rollout.js:15624`、`src/lib/rollout.js:15640`）：

```js
      const tokensIn = toNonNegativeInt(payload.tokensIn);
      const tokensOut = toNonNegativeInt(payload.tokensOut);
      const cacheReads = toNonNegativeInt(payload.cacheReads);
      const cacheWrites = toNonNegativeInt(payload.cacheWrites);
```
```js
      const delta = {
        input_tokens: tokensIn,
        cached_input_tokens: cacheReads,
        cache_creation_input_tokens: cacheWrites,
        output_tokens: tokensOut,
        reasoning_output_tokens: 0,
        total_tokens: tokensIn + tokensOut + cacheReads + cacheWrites,
        conversation_count: 1,
      };
```

**模型：只报 provider，不报模型**（`src/lib/rollout.js:11730`）：

```js
function normalizeKilocodeProviderToModel(providerName) {
  if (typeof providerName !== "string" || !providerName.trim()) return "provider:unknown";
  const slug = providerName
    .trim()
    .toLowerCase()
    .replace(/\s+/g, "-")
    .replace(/[^a-z0-9._-]/g, "");
  // A slug consisting only of separators (dashes/dots/underscores) carries no
  // information — treat it as unknown.
  if (!slug || !/[a-z0-9]/.test(slug)) return "provider:unknown";
  return `provider:${slug}`;
}
```

原因见注释（`src/lib/rollout.js:11724`）：真实 model id 存在 workspace state，不归属到单轮。

**时间戳**：`msg.ts`（`src/lib/rollout.js:15617`）。**游标**：`taskUuid:ts` + 文件 `{size, mtimeMs, ino}`。

**$0 风险**：无。

---

## 4. hook 主动上报 vs 被动扫描

### 4.1 三份独立证据

**证据一：`src/lib/` 下存在的 hook/插件文件只有 4 个**——`grok-hook.js`（Grok，不在本 19 家）、`omp-hook.js`（OMP/OmO，不在本 19 家）、`openclaw-hook.js`、`openclaw-session-plugin.js`。Claude/Codex/Gemini/CodeBuddy/WorkBuddy 的 hook 以“改写各 CLI 自己的 `settings.json`”实现，代码在 `claude-config.js`（`upsertClaudeUsageHooks`）、`gemini-config.js`（`ensureHooksEnabled`，`src/lib/gemini-config.js:147`）、`codex-config.js`。

**证据二：`passive-mode.js` 的“期望 hook”白名单**（`src/lib/passive-mode.js:93` 起，`hookExpected: true` 共 8 个）：`claude`、`gemini`、`codex`、`acode`、`every_code`、`codebuddy`、`workbuddy`、`workbuddy-ai`。判定式（`src/lib/passive-mode.js:191`）：

```js
  const passive = hookExpected && !hookInstalled && logsPresent;
```

模块头明确 hook 只影响“新鲜度”，不影响解析可行性（`src/lib/passive-mode.js:4` 起：`…Our parsers can read those logs directly without the hook; the only loss is "data is fresh within seconds"…`）。

**证据三：安装点（read 已复核）** `src/commands/init.js:568`：

```js
  const hookRepairs = [
    ["claude", context.claudeDir, () => upsertClaudeUsageHooks({
      settingsPath: context.claudeSettingsPath,
      hookCommand: context.claudeHookCommand,
    })],
    ["gemini", context.geminiConfigDir, () => upsertGeminiHook({
      settingsPath: context.geminiSettingsPath,
      hookCommand: context.geminiHookCommand,
    })],
    ["codebuddy", context.codebuddyDir, () => upsertClaudeHook({
      settingsPath: context.codebuddySettingsPath,
      hookCommand: context.codebuddyHookCommand,
    })],
    ["workbuddy", context.workbuddyDir, () => upsertClaudeHook({
      settingsPath: context.workbuddySettingsPath,
      hookCommand: context.workbuddyHookCommand,
    })],
    ["workbuddy-ai", context.workbuddyAiDir, () => upsertClaudeHook({
      settingsPath: context.workbuddyAiSettingsPath,
      hookCommand: context.workbuddyAiHookCommand,
    })],
  ];
```

OpenClaw 插件安装调用点在 `src/commands/init.js` 的 `installOpenclawSessionPlugin`（`src/commands/init.js:1078`，R1 复核已确认）。

### 4.2 归类结果

| 类型 | 家 | 依据 |
|------|----|------|
| hook 挂载 + 解析仍读官方日志（hook 只决定新鲜度） | **Gemini、CodeBuddy、WorkBuddy、WorkBuddy AI** | `passive-mode.js` 名单 + `init.js:568` 起的安装点 |
| 有 hook/插件，但解析完全被动 | **OpenClaw** | 两个 hook 文件存在；sync 注释 `src/commands/sync.js:1102`：`// Parses plugin-triggered and/or passively discovered session files.`；输入来自 `resolveOpenclawSessionFiles` 的文件遍历（`src/lib/rollout.js:1618`） |
| 纯被动扫描（无 hook 文件、不在名单） | **Opencode、Kiro、Hermes、KiroCli、Kimi、KimiCode、Roocode、Cline、Zed、Lmstudio、Unsloth、Anythingllm、Goose、Droid、Kilocode**（15 家） | 无 hook 文件；Kimi 侧源码直接声明 `src/lib/rollout.js:8078`：`// No hook installation needed; Kimi writes wire.jsonl automatically.` |

**易误判点**：`passive-mode.js` 的注释提到 `hookStatus` 形状含 `opencode`/`openclaw`/`grok`（`src/lib/passive-mode.js:88` 附近），但 `detectPassiveProviders` 实现里**没有**为 opencode/openclaw 生成条目——以实现为准。

---

## 5. 「只有 total_tokens 没有分桶」风险清单

共享闸门 `isAllZeroUsage` 把 `total_tokens` 也算“非零”（`src/lib/rollout.js:4945`），因此风险取决于各家自己的前置判定：

**计价侧前提（R1 复核补写，B4 唯一性的隐含条件）**：`src/lib/pricing/index.js:248`～`256` 只把 `codex`/`acode`/`every-code`/`omo`/`cline` 五家的 `reasoning_output_tokens` 视为 output 的子集：

```js
  const reasoningIncludedInOutput =
    row.source === "codex" || row.source === "acode" || row.source === "every-code" ||
    row.source === "omo" || row.source === "cline";
  const reasoningCost = reasoningIncludedInOutput
    ? 0
    : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
```

集合外的 source（含 `gemini`/`goose`/`kiro`）**推理按 output 单价单独计费**。下文所有「非 $0」判定都以这条为前提：例如 **Goose 的 `reasoning>0` 行不是 $0**，会按 output 价计入费用。

**A. 可从源码推出 $0 退化路径（1 家，且为唯一）**

- **Gemini**：`total = Math.max(reportedTotal, computedTotal)`（`src/lib/rollout.js:4743`）+ 闸门 `isAllZeroUsage(delta)`（`src/lib/rollout.js:2971`）。日志若只写 `tokens.total`，产出 input/output/cache 全 0 的行；此时 reasoning（`thoughts`）同为 0，故该行在 `computeRowCost` 下**确实为 $0**。
- **唯一性的依据（本节新增）**：19 家 `total_tokens` 赋值点独立比对（R1 复核表），只有 Gemini 采用 `max(reported, computed)` 且前置闸门把 `total_tokens` 也当非零；其余 18 家均为「各桶求和」或「SQL/JS 层至少一桶 > 0」形态 → 不会出现「非零桶全 0 而 total > 0」。

**B. 弱化版（2 家）——可产出 `input=0 / output=0 / reasoning>0` 行；因不在上述集合内，计价不为 $0**

- **Goose**：`reasoningDelta = max(0, dTotal − dInput − dOutput)`（`src/lib/rollout.js:14711`），`dTotal > 0` 即落桶。可能产出 `input=0 / output=0 / reasoning>0` 的行。
- **KiroCli（R1 复核从 E 类移入）**：SQLite 支路有 `approxInput > 0 || approxOutput > 0 || approxReasoning > 0` 闸门（`src/lib/rollout.js:9209`）；但**会话支路的零值闸门只检 4 列、不含 reasoning**（`src/lib/rollout.js:9414`），紧随其后的 delta 含 `total_tokens: input + output + cacheRead + cacheCreation + reasoning`（`src/lib/rollout.js:9446`）→ 可产出 `input=0 / output=0 / reasoning>0 / total>0` 的行，与 Goose 同形。`kiro` 不在推理并入集合内 ⇒ 不为 $0。

**C. 明确规避（3 家）**

- **Droid**：`applyDroidTotalFallback`（`src/lib/rollout.js:14972`）把 total 缺口填进 `output` 或 `thinking`。
- **Lmstudio / Unsloth**：`input_tokens = max(0, total − completion − cacheRead − cacheWrite)`（`src/lib/rollout.js:13040`）。

**D. 不产 $0 行，但把纯 total 行静默丢弃（10 家；R1 复核后 Codebuddy 结案并入）**

Kimi（`src/lib/rollout.js:9590`）、KimiCode（`src/lib/rollout.js:9823`）、Workbuddy（`src/lib/rollout.js:10968`）、Openclaw（`src/lib/rollout.js:2109`）、Roocode（`src/lib/rollout.js:11910`）、Kilocode（`src/lib/rollout.js:15628`）、Cline（`src/lib/rollout.js:12381`）、Anythingllm（`src/lib/rollout.js:14016`）、Kiro（`src/lib/rollout.js:7547`）、**Codebuddy**（扩展日志闸门 `src/lib/rollout.js:10345`；JSONL 支路为含 `reasoningTokens` 的五桶零闸门 `src/lib/rollout.js:10476`～`10485`）。

**E. 结构上不可能出现「六列全零 + `total>0`」（3 家）**

Zed（`src/lib/rollout.js:12598`、`src/lib/rollout.js:12752`）、Hermes（`src/lib/rollout.js:7718` 的 SQL `WHERE`）、Opencode（`src/lib/rollout.js:3109`）。

**限定（R1 复核 I2）**：此处「不可能」仅指上述六列全零形态，**不等于「不可能出现 `input=0 / output=0`」**——Zed 的 `totalDelta` 只由 input/output/cache_read/cache_write 求和（`src/lib/rollout.js:12751`）、`totalDelta <= 0` 即跳过（`12752`），**只有缓存列增长**时仍会落出 `input=0 / output=0 / total>0` 的行；Cline 同理（`src/lib/rollout.js:12403` 的 `total_tokens: deltaTotal`）。下游不得把 E 类读成无条件安全。

**F. 未判定（0 家；R1 复核后清空）**

原唯一的 Codebuddy 已结案并移入 D 类：两条支路都有零值闸门（`10345` / `10476`～`10485`），不会产出「全零桶 + 非零 total」的队列行。

---

## 6. 字段名速查表

| Provider | 日志字段（原文） | 备注 |
|----------|------------------|------|
| Gemini | `tokens.input` / `.cached` / `.output` / `.tool` / `.thoughts` / `.total` | `tool` 并入 output |
| Opencode | `tokens.input` / `.output` / `.reasoning` / `.cache.read` / `.cache.write` | 嵌套 cache |
| Openclaw | `message.usage.input` / `.cacheRead` / `.cacheWrite` / `.output`（`totalTokens` 仅预筛） | input 先减 cacheRead |
| Kiro | `tokens_prompt` / `tokens_generated` | 无缓存列 |
| Hermes | `input_tokens` / `output_tokens` / `cache_read_tokens` / `cache_write_tokens` / `reasoning_tokens` / `message_count` | 会话数=消息数差分 |
| KiroCli | SQLite：`user_prompt_length` / `response_size` / `reasoning_size`（字符数 `/4`）；legacy 会话 `.json`：`input_token_count` / `output_token_count`（**真实值**，皆为 0 才回退字符估算）；v2：`messages.jsonl` 字符数 `/4` | 另有 `request_start_timestamp_ms` / `model_id`；credits 只写 sidecar |
| Kimi | `token_usage.input_other` / `.output` / `.input_cache_read` / `.input_cache_creation` | 秒级时间戳 |
| KimiCode | `usage.inputOther` / `.inputCacheRead` / `.inputCacheCreation` / `.output`（或 Anthropic / OpenAI 形状） | 毫秒时间戳 |
| Codebuddy | `cachedMissTokens` / `input_tokens` / `prompt_tokens`；`output_tokens`；`cache_read_input_tokens` / `cached_tokens` | 多镜像优先级 |
| Workbuddy | `rawUsage.prompt_tokens` / `.completion_tokens` / `.cache_read_input_tokens` / `.cache_creation_input_tokens` / `completion_tokens_details.reasoning_tokens` | prompt 减双向缓存 |
| Roocode / Kilocode | `payload.tokensIn` / `.tokensOut` / `.cacheReads` / `.cacheWrites` | 同形状，source 不同 |
| Cline | `metrics.inputTokens` / `.outputTokens` / `.cacheReadTokens` / `.cacheWriteTokens` / `.reasoningTokenCount` / `.cost` | 唯一产 cost |
| Zed | `input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` | 线程四元组 |
| Lmstudio | `prompt_tokens` / `completion_tokens` / `total_tokens` / `*_details.cached_tokens` … | 字节扫描 |
| Unsloth | `prompt_tokens` / `completion_tokens` / `total_tokens` / `cached_tokens` / `cache_write_tokens` / `reasoning_tokens` | 共用归一化器 |
| Anythingllm | `prompt_tokens` / `completion_tokens` / `total_tokens` | 差额判为 reasoning |
| Goose | `accumulated_total_tokens`（回退 `total_tokens`）/ `model_config_json` | 无缓存列 |
| Droid | `tokenUsage.inputTokens` / `.outputTokens` / `.cacheCreationTokens` / `.cacheReadTokens` / `.thinkingTokens` / `.totalTokens` | total 兜底 |

---

## 7. 存疑与待复核点

### 7.1 未确认项（R1 复核后重写：7 条结案、2 条转 U 组，第 10 条自查清单整体重写；**R2 复核后 9 条全部结案、U 组清空**）

**已结案（本轮按权威口径 `read` / `[System.IO.File]::ReadAllLines(...,UTF8)` 打开过对应行）**：

1. **（结案）Codebuddy 的 JSONL 支路闸门**：注释 `src/lib/rollout.js:9940`～`9943` 说明现代 CodeBuddy 把用量写进 `providerData.rawUsage`；该支路零值判定为含 `reasoningTokens` 的五桶闸门（`src/lib/rollout.js:10476`～`10485`）→ Codebuddy 归 §5 D 类，F 类清空。（原引 `9938` 实为 GBK 口径——该行是 `}`；原「read 确认」字样与事实不符，已删。）
2. **（结案）Opencode 的 storage 下层结构**：`walkOpencodeMessages`（`src/lib/rollout.js:5025`）递归遍历，只收 `msg_*.json`（`src/lib/rollout.js:5033`）；「消息文件在 `<storage>/message/**`」成立，`listOpencodeMessageFiles`（`src/lib/rollout.js:243`）只负责拼 `<storage>/message` 一层。
3. **（结案）Cline 的根目录实现**：`resolveClineSessionsDirs`（`src/lib/rollout.js:12054`，Windows 双安装 + WSL 探测在 `12069`～`12073`）与 `scanClineSessionFiles`（`src/lib/rollout.js:12086`）都在 `src/lib/rollout.js`（**不在** `sync.js`）；`~/.cline/data/sessions` 与 `src/commands/sync.js:2110` 注释一致（原引 2109 为 −1）。
4. **（结案，推翻原描述）KiroCli 会话文件侧字段名**：**不是**模块注释 `8109` 的 `user_prompt_length`/`response_size`，而是 `turn.input_token_count` / `turn.output_token_count`（`src/lib/rollout.js:8371`～`8372`，由 `readKiroCliSessionTurns`（`8316`）读取）。`readKiroCliV2SessionTurns`（`8482`）另读 `messages.jsonl`，其注释（`8475`～`8481`）明说该文件不含 token 字段、沿用 4 字符/token 近似。`writeKiroCliCreditsSidecar`（`8701`）只写 credits 侧车，不进 token 桶。
5. **（结案）Kimi / KimiCode 的 home 优先级**：`resolveKimiHome`（`src/lib/rollout.js:9493`；`KIMI_HOME` 在 `9495`，win32 WSL 分支 `9497`～`9502`）与 `resolveKimiCodeHome`（`src/lib/rollout.js:9669`；`KIMI_CODE_HOME` 在 `9671`）。
6. **（R2 结案，推翻「与 JSONL 同形」的开放假设）Workbuddy 的 traces 支路字段名**：「JSONL 优先、traces 兜底、`detailedSessions` 压制」已确认（`src/lib/rollout.js:11013`）；traces 支路字段为 `modelInfo.total{Input,Output,Cached}Tokens` + `startedAtRaw` + `models[0]`（`11081`、`11085`～`11105`），**与 JSONL 支路的 `providerData.rawUsage.*` 不同形**；详见 §3.10 末段。
7. **（R2 结案）Droid 的 `resolveDroidModel` 全链**：定义在 `src/lib/rollout.js:15046`–`15057`（链：`settings.model` → sidecar `<id>.jsonl` 抓取 → `providerLock`/模型片段推断 → `defaultDroidModelForProvider`），与调用点 `15348` 上方注释 `15343`–`15346` 一致；`applyDroidTotalFallback`（`14972`～`14979`）逐字一致，`normalizeDroidModelName` 在 `14858`。（原引「调用点与注释 `15333`/`15338`」为 GBK 口径残留。）
8. **（结案）Anythingllm 的 SELECT 语句**：`src/lib/rollout.js:13874`～`13887`，`FROM workspace_chats`，列经 `json_extract(response,'$.metrics.*')`，SQL 层只过滤 `whereClause`（游标 / `include`）、**不做零值过滤** → 与 §3.16「纯 total 行在 JS 侧 `14016` 被丢弃」自洽。
9. **（结案）`src/commands/init.js` 的 OpenClaw 插件安装行号**：`installOpenclawSessionPlugin` 调用点确在 `src/commands/init.js:1078`（另在 `init.js:54` 有 import）。

10. **行号自查清单（R1 复核后重写：原版把「仅由 PowerShell 定位」的 40 条整体标为“未复核”，该自查本身不准确）**——R1 复核用权威口径（`read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`）逐条重验这 40 条：**35 条正确、5 条错误**（另在清单外又查出 2 条错误 + 1 处归属错误，合计 8 处已在上文改正）。
    - **重验为正确、可直接引用（35 条）**：Gemini 的 `1265`/`1275`/`1283`/`1361`/`1403`；Opencode 的 `1438`/`1448`/`1513`；Kiro 的 `7282`/`7305`（指向空行，`readKiroDbTokens` 函数头实为 `7306`，在 −1 容差内）/`7334`/`7338`；KiroCli 的 `8132`/`8138`/`8140`/`8165`；Codebuddy 的 `9979`/`10022`/`10141`/`10320`/`10322`/`10525`；Roocode 的 `11749`/`11762`/`11790`/`11805`/`11814`；Cline 的 `12157`；Zed 的 `12722`/`12726`/`12732`（行号本身正确，但正文把 `isReset` 挂在此处 → 已改为 `12742`）/`12816`；Lmstudio 的 `12897`/`12917`。
    - **已改正的 8 处**：`1307`→`1284`（inode 身份；`1307` 实为 `let lastModel = …`）、`7294`→`7298`（`resolveKiroDbPath` 函数头；`7294` 是 `}`）、`10331`→`10337`（非缓存输入修正块头；`10331` 是 `let inputTokens = toNonNegativeInt(inputRaw);`）、`10339`→`10345`（扩展日志零值闸门；`10339` 是 `usage.cacheMissTokens === undefined &&`）、`9938`→`9940`（`providerData.rawUsage` 注释，实为 `9940`～`9943`；`9938` 是 `}`）、`12190`→`12178`（`normalizeClineModel` 函数头，四级兜底链 `12179`～`12189`；`12190` 是闭合 `}`）、`13681`→`13691`（`parseUnslothIncremental` 函数头；`13681` 是 `normalizeUnslothUsageRow` 内的 `if (!id || !totals || !bucketStart) return null;`）、`12732`→`12742`（`isReset` 实际位置；`12732` 是差分基线 `const prev = threadTotals[row.id] || {…}`）。
    - **归属错误（E11）**：`12845` / `12853` 曾被列在本条「Zed 的 `12722`/`12726`/`12732`/`12816`/`12845`/`12853`」名下，但两者都是 **LM Studio 模块**（`12843` 的 `// LM Studio` 段头；`12845` 为模块注释首行 `pretty-printed OpenAI-compatible final responses`、`12853` 为 `const LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024;`），§3.14 正文本来就按 Lmstudio 引用——同一文档两处归属互相打架，现已从 Zed 清单删除，只保留 Lmstudio 条目（`12845`/`12853`/`12897`/`12917`）。
    - **口径教训**：错误全部落在 ≥7000 行，且都等于「UTF-8 行号 −(4~10)」= GBK 行号，即 **PowerShell 口径混入正文**；`12190`、`1307` 两种口径都对不上，属无来源数字。**「自称已 read 确认」不能替代逐条重验**——原第 1 条的 `9938` 就自称 read 确认，实为 GBK 口径附近位（UTF-8 `9940` 的 GBK 位是 `9934`，而 `9938` 落在 `function codebuddyJsonlHasUsage` 头 `9944` 附近，两种口径都不精确对应）。自查清单本身也会骗人：它把 35 条正确的行号标成「未复核」，却漏掉真正的错项。
    - 参考：`src/commands/init.js:568`～`589` 的 hook 安装点**已复核**（与本清单口径一致）。
    - **R2 补充（该自查清单的盲区）**：清单只覆盖这 40 条，正文另有 **6 处同族漂移**未被它捕获（§3.10 的 `10672`/`10688`、§3.15 的 `13704`/`13720`、§3.8 的 `9697`、§3.9 的 `9918`，加 §7.1 第 7 条的 `15333`/`15338`），已在 R2 全部改正——「清单内自检通过」不等于正文无漂移。

**无法验证（U 组）——R2 复核后清空**：

- **原 U1 — Workbuddy traces 支路的字段名是否与 JSONL 支路同形**：**R2 已结案**——traces 支路字段为 `modelInfo.total{Input,Output,Cached}Tokens` + `startedAtRaw` + `models[0]`（`src/lib/rollout.js:11081`、`11085`～`11105`），与 JSONL 支路的 `providerData.rawUsage.*` **不同形**；原「疑为 traces 的 `11246`」实属**第三条 SQLite 支路**（`11134` 起 `dbExists` 分支）。详见 §3.10 末段。
- **原 U2 — Droid `resolveDroidModel` 的三级链是否与注释一致（`*-unknown` 兜底）**：**R2 已结案**——定义实为 `src/lib/rollout.js:15046`–`15057`（原判定条件给的读段 `15300`～`15345` 有误），链为 `settings.model` → sidecar `<id>.jsonl` 抓取 → `providerLock`/模型片段推断 → `defaultDroidModelForProvider`，与调用点 `15348` 上方注释 `15343`–`15346` 一致。

### 7.2 下游必须注意的三点

1. **“文本/JSONL 型”只对了一半**：7 家（Kiro、Hermes、KiroCli、Zed、Unsloth、Anythingllm、Goose）是 SQLite 读取；若 S3b 也按文件类型分工，会出现职责重叠或空白。
2. **`source` 不是 provider 名**：`kimi` 聚合两套解析器、`kiro` 聚合两套、`kilo-code` 带连字符、Workbuddy 用 variant。任何“按 source 反查解析器/ hook 状态”的逻辑都必须先过 §2 的对照。
3. **`conversation_count` 语义五家五样**：产生差分的消息数（Gemini/Opencode）、每条事件 +1（Kimi 一族等）、仅首次（Cline）、仅首次会话或重置（Droid）、恒 1（Goose/Lmstudio/Unsloth）、DB `message_count` 差分（Hermes）、线程更新次数（Zed）。dashboard 若直接当“会话数”展示，会同时出现高估与低估。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-C-providers-jsonl.md`（E=12 / I=7 / U=2）。**采纳 21 条（E1–E12、I1–I7 及第 9/5 条结案），保留 U1/U2（写明判定条件），反驳 0 条**。行号口径统一为 `read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；每条均在本轮开过对应行才落笔。

| 编号 | 原表述 | 修正后 | 依据行号（`read`/UTF-8） |
|---|---|---|---|
| E1 / B5 | §3.6·§0 速览·§6 速查表：「本家没有真实 token 字段，全部按字符数 ÷ 4 估算」 | 推翻：legacy 会话 `.json` 优先读**真实** `turn.input_token_count` / `turn.output_token_count`，仅两者皆 0 才回退字符估算；纯字符估算只剩 SQLite `data.sqlite3` 与 v2 `messages.jsonl` 两支；常量使用点补漏：`8429`/`8430`（**R2 修正**：R1 原写 `8428`，那是注释行） | `rollout.js:8371`、`8372`、`8374`、`8386`、`8387`、`8429`、`8430`、`8475`–`8481`、`9152`–`9159` |
| E2 | §5 把 KiroCli 归入 E 类「结构上不可能」 | 移入 **B 类「弱化版」**：会话支路零值闸门只检 4 列、不含 reasoning（`9414`），delta 含 `reasoning`（`9446`）→ 可产出 `input=0/output=0/reasoning>0` 行；因 `kiro` 不在推理并入集合内，**不是 $0** | `rollout.js:9414`、`9446`、`9209`；`pricing/index.js:248`–`256` |
| E3 | Kiro 路径引用 `7294` | `7298`（`resolveKiroDbPath` 函数头，函数体 `7298`–`7304`；`7294` 是 `}`） | `rollout.js:7298` |
| E4 | Codebuddy「非缓存输入修正」块头 `10331` | `10337`（块 `10337`–`10343`） | `rollout.js:10337` |
| E5 | §3.9 + §5 F 的扩展日志闸门 `10339`（标注未复核） | `10345`，并去掉「未复核」标注 | `rollout.js:10345` |
| E6 | §7.1 第 1 条 `9938`（自称 read 确认） | `9940`（注释 `9940`–`9943`）；「read 确认」字样删除并说明其为 GBK 口径附近位 | `rollout.js:9940` |
| E7 | Cline `normalizeClineModel` 引用 `12190` | `12178`（兜底链 `12179`–`12189`；`12190` 是闭合 `}`） | `rollout.js:12178` |
| E8 | §3.13 把 `isReset` 挂在 `12732` | 改为 `12742`（`isReset = currSum > 0 && currSum < prevSum`）；注明 `12732` 是差分基线 | `rollout.js:12742` |
| E9 | §3.15 标题 `parseUnslothIncremental` 引 `13681` | `13691`（`13681` 是 `normalizeUnslothUsageRow` 内的空值守卫） | `rollout.js:13691` |
| E10 | §3.1「文件身份含 inode（`1307`）」 | `1284`（`const inode = st.ino \|\| 0;`），判等见 `1287`–`1291` | `rollout.js:1284` |
| E11 | `12845` / `12853` 被列在 **Zed** 名下 | 改归 **LM Studio**（`12843` 段头、`12845` 注释首行、`12853` = `LMSTUDIO_WINDOW_BYTES`）；与 §3.14 正文的自我矛盾解除 | `rollout.js:12843`、`12845`、`12853` |
| E12 | §3.18 引 `sync.js:2013` 为 `dedupeDroidSettingsFilesBySession` | 改为定义点 `rollout.js:15069`（调用 `15165`、导出 `24839`）；`2013` 改述为 Droid 文件合并起点。**R2 修正**：R1 原写的 `sync.js:6595` 只是注释一条，「全集唯一出现点」不成立（全仓 4 处命中） | `rollout.js:15069`、`15165`、`24839`；`sync.js:6595` |
| B4 前提 | §5 A 只说 Gemini 唯一，未写依赖 | §5 开头补「**计价侧前提**」：`reasoningIncludedInOutput` 仅含 `codex/acode/every-code/omo/cline`，集合外 source 的推理按 output 计费 ⇒ **Goose reasoning-only 行不是 $0** | `pricing/index.js:248`–`256` |
| I1 | §5 F 类「Codebuddy 未判定」 | 结案归 **D 类**：扩展日志闸门 `10345`；JSONL 支路为含 `reasoningTokens` 的五桶零闸门 ⇒ F 类清空 | `rollout.js:10345`、`10476`–`10485` |
| I2 | §5 E 类「结构上不可能」措辞过强 | 加限定：仅指「六列全零 + `total>0`」不可能；Zed 的 `totalDelta` 只由 4 桶求和（`12751`）且 `<= 0` 跳过（`12752`），缓存-only 时仍可 `input=0/output=0`；Cline 同理 | `rollout.js:12751`、`12752`、`12403` |
| I3 | §0 速览 Opencode 行漏记 SQLite 支路 | 数据形态列补「**+ 同寄主 SQLite 支路**」（`5038` 起），与 sync 两路相加一致 | `rollout.js:5038`；`sync.js:1386`–`1388` |
| I4 | §1.5 把 `12994` 称作「return 块」 | 改述为 `normalizeLocalStudioTokens` **函数头**；返回块为 `13039`–`13049`（`total_cost_usd: 0` 在 `13047`、`conversation_count: 1` 在 `13048`） | `rollout.js:12994`、`13039`–`13049` |
| I5 | §7.1 第 4 条 KiroCli 会话字段名「未核对」 | **结案并推翻**：会话侧字段是 `input_token_count` / `output_token_count`，**不是**注释 `8109` 的 `user_prompt_length`/`response_size`；v2 支路另说明其无 token 字段 | `rollout.js:8371`、`8372`、`8482`、`8475`–`8481` |
| I6 | §7.1 第 2 条 Opencode storage 结构未读 | **结案**：`walkOpencodeMessages` 递归且只收 `msg_*.json` | `rollout.js:5025`、`5033` |
| I7 | §7.1 第 8 条 Anythingllm SELECT 未读 | **结案**：SELECT 本体 `13874`–`13887`，SQL 层只过滤 `whereClause`、不做零值过滤；第 3 条（Cline 目录）同时结案 | `rollout.js:13874`–`13887`、`12054`、`12086` |
| 第 9 条 | init.js OpenClaw 插件安装行号未复核 | **结案**：`installOpenclawSessionPlugin` 调用点 `init.js:1078` | `init.js:1078` |
| 第 5 条 | Kimi / KimiCode home 优先级未逐字确认 | **结案**：`resolveKimiHome`（`9493`，`KIMI_HOME` `9495`，WSL `9497`–`9502`）、`resolveKimiCodeHome`（`9669`，`KIMI_CODE_HOME` `9671`） | `rollout.js:9493`、`9669` |
| §7.1 第 10 条 | 「仅 PowerShell 定位、未复核」的自查清单（把 35 条正确项标错、漏掉真正错项） | **整体重写**为「已用权威口径逐条重验」版本：35 条正确 / 5 条错误 + 清单外 2 条错误 + 1 处归属错误；写明 GBK 漂移机制（行尾字节 ≥ 0x81 吞换行，漂移随位置累积至 15）与「自称 read 确认 ≠ 已重验」的教训 | `REVIEW-R1-C` 行号漂移表；本文件 §7.1 第 10 条 |
| U1 / U2 | Workbuddy traces 字段名、Droid `resolveDroidModel` 三级链 | R1 时保留为 U 组并写明判定条件；**R2 两条均已结案**（traces 字段见 §3.10 末段；Droid 链定义 `15046`–`15057`）。R1 给出的读段 `15300`–`15345` 有误 | `rollout.js:11081`、`11085`–`11117`、`15046`–`15057`、`15343`–`15348` |

**附带收紧（复核标为通过、本文件顺手对齐口径，不改结论）**：`rollout.js:7338`（`countKiroJsonlLines`）、`8132`/`8138`/`8140`/`8165`（KiroCli 路径）、`10022`（`capCodebuddyUsageFingerprints`）的「未复核」标注删除；Unsloth 投影 SQL 由「`13620` 附近」收紧为 `13634`–`13636`；Droid `prune: true` 由 `2031` 对齐为 `2032`；§3.14 明确 `12845`/`12853` 属 LM Studio 模块。

**未落实项**：无 E/I 项遗留。仍无法验证者仅 U1/U2 两条（已写明所需读段；**R2 已把这两条结案，见下方 R2 修正**）；原报告的 ±1 容差项（`sync.js:1102`/`1908`/`2062`/`2109`/`2315`、`rollout.js:7305`/`11706`、`init.js:568` 附近）按其容差保留原文，未逐条改写（`sync.js:2031` 已随 E12 一并对齐为 `2032`）。

**样本仓库状态**：本轮仅读取 `C:\Users\admin\AppData\Local\Temp\tt-research`，未写入任何文件；所有改动只落在本分片。

### R2 修正

依据：`REVIEW-R2-2-providers.md`（E=10 / I=9 / U=4；本片涉及 E1–E8、I1、I2、U1–U2 结案）。**采纳 14 条，反驳 0 条**；跨分片项（E10 属 S1、I7 属 S4）不属本片职责，未改。行号口径统一为 `read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；每条均在本轮开过对应行才落笔。R2 报告结论「E1–E8 全是同一 GBK 漂移根因的未清残渣（≈10700 行处 +6、≈13700/15300 行处 +10、≈9700/9900 行处 +6/+7）」经逐行复验成立。

| 编号 | 原表述 | 修正后 | 依据行号（已开行核对） |
|---|---|---|---|
| E1 | §3.10 兜底 `"auto"` 引 `rollout.js:10672` | `10678`（函数头 `resolveWorkbuddyDefaultModel` 在 `10677`；`10672` 是 `resolveWorkbuddyHome` 内 `});`） | `rollout.js:10677`–`10678` |
| E2 | §3.10 三类数据源注释引 `rollout.js:10688` | `10694`（`10688` 是 `} catch (_e) {`） | `rollout.js:10694`–`10699` |
| E3 | §3.15 Unsloth 快路径引 `rollout.js:13704` | 判定在 `13714`（`hasBoundedScan`），短路返回在 `13717`；`13704` 是 `: {};` | `rollout.js:13713`–`13718` |
| E4 | §3.15 落桶引 `rollout.js:13720` | `13730`（`13720` 为空白行） | `rollout.js:13730` |
| E5 | §3.18 模型行 + §7.1 第 7 条引 `15333`/`15338`（R1 标为「已核」，实为 GBK 口径） | 注释 `15343`–`15346`、调用点 `15348`；`resolveDroidModel` **定义在 `15046`**（体 `15046`–`15057`） | `rollout.js:15046`–`15057`、`15343`–`15348` |
| E6 | 修正记录 E12 的 `sync.js:6595`「全集唯一出现点」 | 定义在 `rollout.js:15069`（调用 `15165`、导出 `24839`）；`sync.js:6595` 仅一条注释；全仓共 4 处命中，「唯一出现点」不成立 | `rollout.js:15069`、`15165`、`24839`；`sync.js:6595` |
| E7 | §3.8 KimiCode 兜底引 `rollout.js:9697` | `9704`（函数头 `resolveKimiCodeDefaultModel` 在 `9703`；`9697` 是 `for` 循环闭合 `}`） | `rollout.js:9703`–`9704` |
| E8 | §3.9 Codebuddy 兜底引 `rollout.js:9918` | `9924`（函数头 `resolveCodebuddyDefaultModel` 在 `9923`；`9918` 是 `});`） | `rollout.js:9923`–`9924` |
| I1 | 修正记录 E1 的「常量使用点补漏列的第 4 处 `8428`」 | `8428` 是注释行（`// KIRO_CLI_CHARS_PER_TOKEN; bypass that by pre-multiplying here.`）；乘法使用点为 `8429`/`8430`；§3.6 与修正记录同步改 | `rollout.js:8426`–`8430` |
| I2 | §3.17 Goose「`14687` 的 continue 条件是三者皆 0」 | `14686`（`14687` 是空白行） | `rollout.js:14686` |
| U1（R1 U1） | Workbuddy traces 支路字段名未读 | **结案**：traces 支路 = `modelInfo.total{Input,Output,Cached}Tokens` + `startedAtRaw` + `models[0]`，与 JSONL 支路的 `providerData.rawUsage.*` **不同形**；原疑为 traces 的 `11246` 属**第三条 SQLite 支路**（`11134` 起 `dbExists` 分支）；详见表后新增段 | `rollout.js:11081`、`11085`–`11117`、`11246` |
| U2（R1 U2／R2 U3） | Droid 三级链「未展开」，判定条件写读 `15300`–`15345` | **结案**：定义 `15046`–`15057`（`settings.model` → sidecar JSONL → `providerLock`/推断 → `defaultDroidModelForProvider`），与调用点上方注释一致；原判定条件行号有误 | `rollout.js:15046`–`15057`、`15343`–`15348` |
| U（R2 U1） | 第 9 条 `init.js:1078` 本轮未独立打开 | **结案**：`const openclawInstall = await installOpenclawSessionPlugin({ home, trackerDir, packageName: "tokentracker-cli", … })` | `init.js:1078`–`1083` |
| U（R2 U2） | 第 5 条 Kimi 侧 `9493`/`9495`/`9497`–`9502` 仅复核 KimiCode 侧 | **结案**：`resolveKimiHome`(9493)、`KIMI_HOME`(9495)、win32 WSL 分支(9497–9502) 逐行一致 | `rollout.js:9493`–`9505` |

**未落实项**：无 E/I 项遗留；U 组已全部清空。跨分片的 E10（S1 的 45 项分解算术）与 I7（S4 对 `rollout.js:9287` 的表述）不在本片职责内，未改。
