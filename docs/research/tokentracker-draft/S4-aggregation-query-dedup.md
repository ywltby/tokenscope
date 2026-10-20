# S4 研究报告：TokenTracker 的聚合、去重、桶、查询与时间语义

- 样本仓库：`https://github.com/xiufengsun/TokenTracker`
- 本地检出：`C:\Users\admin\AppData\Local\Temp\tt-research`
- HEAD：`b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）
- 研究方法：只读。全程使用 `read` / `grep` / `glob` / PowerShell 只读命令，未修改样本仓任何文件，未执行其中脚本。
- 本文只覆盖「聚合 / 去重 / 桶 / 查询 / 时间语义 / 修复迁移 / 派生指标」，不涉及定价金额算法的内部推导（另有分工）。
- 引用格式：`相对路径:行号`，路径相对样本仓库根。所有行号取自本次实际读取的内容。
- 凡未亲眼确认者，一律标注 `【未能确认】`，并在末节汇总。

---

## 0. 结论速览

按与研究目标的相关度排序，最有分量的十条：

1. **`queue.jsonl` 是"追加式的累计快照日志"，不是增量事件流**。每次同步对"被触碰的桶"追加写入该桶**当前累计值**的完整行（`src/lib/rollout.js:3408`、`src/lib/rollout.js:3482`）。同一个 `(source, model, hour_start)` 会被反复写入，读端以**最后一行胜出**（last-row-wins）作为唯一合并规则（`src/lib/async-queue-reader.js:86`）。
2. **桶键的三元组是 `(source, model, hour_start)`，分隔符固定 `|`**，且 `source`/`model` 在入键前做归一化（小写、trim、空值兜底）（`src/lib/rollout.js:57`、`src/lib/rollout.js:4222`、`src/lib/rollout.js:4254`）。
3. **半小时桶一律以 UTC 计算**：`toUtcHalfHourStart` 取 `getUTCMinutes() >= 30 ? 30 : 0`，返回 ISO 字符串（`src/lib/rollout.js:4173`）。本机时区**不参与**写入侧分桶，只在读取/展示侧把桶换算成日键（`src/lib/local-api.js:213`）。
4. **旧值不是相加、也不是删行**。要抵消一个不再成立的旧行，写一行**零值同键行**（retraction）压过它（`src/lib/rollout.js:3376`、`src/lib/rollout.js:19990`、`src/commands/sync.js:5173`）。
5. **dashboard 的用量端点共享前两跳，之后分叉**：所有端点都走 `scopedQueueRows`（队列去重 + scope 过滤，`src/lib/local-api.js:403-412`）与 `rowDayKey`（日键换算，`:213-225`），读取靠 `readQueueData`（去重后的最新行，`src/lib/local-api.js:202-211`）；但**按天聚合只属于 summary（`:2358`）、daily（`:2425`）、heatmap（`:2533`）三个端点**——它们走 `aggregateByDay`（`:227`）；hourly 走 `aggregateHourlyByDay`（`:600`），model-breakdown（`:2602-2641`）、monthly（`:3161-3186`）、project-summary（`:2744-2798`）、project-detail（`:2866-2898`）都是**各自遍历行自建 Map**。分叉点逐端点见 §3.4。
6. **去重不是一层，而是至少七层**：队列行级 last-wins、账户级（cursor 整行 MAX / trae-cn 会话级 LWW）、会话碎片合并（`session_hash`）、fork 拷贝检测（opencode 指纹 / codex 重放窗口 / claude 消息键）、双安装命名空间光标、跨根同 UUID 归组、以及事件级哈希（codex）。每一层的**去重键都不同**，见 §4。
7. **repair 是真会删历史的**，且历史上删过：`claudeGroundTruthRepair` 的 v6 在 0.26.3 上造成 21.7 亿 claude token 丢失，0.26.4 回退到 v4（`src/commands/sync.js:234-247`）。后续版本改为"带守卫的原子重建"，守卫是"任何曾贡献过的会话文件已不在磁盘则跳过"（`src/commands/sync.js:264`，注释句首；整段注释 `:258-267`）。
8. **项目身份在 TokenTracker 里是远程仓库，不是本地 cwd**：`project_key` 由 `project_ref`（仓库 URL）解析出 `owner/repo`，GitLab 嵌套 group 保留全路径（`src/lib/rollout.js:4429`）。本地 cwd 只在会话分析里做显示名（`path.basename`，`src/lib/session-analytics.js:405`）。
9. **时间范围过滤是"日键闭区间"**（`day >= from && day <= to`），日键由请求侧传入的 `tz` / `tz_offset_minutes` 决定；缺省时退化为 `hour_start` 前 10 个字符即 UTC 日（`src/lib/local-api.js:130`、`src/lib/local-api.js:224`）。**【R2 限定（E1）】该"退化 UTC"只对 `rowDayKey` 一族（日键与日键过滤）成立**；`getZonedParts` 系（hourly 小时键、`todayStr`、`night_owl`）缺省回退的是**运行环境本机时区**——详见 §2.4。
10. **分页在本地 API 里非常弱**：只有会话浏览器（上限 2000）和项目汇总（硬上限 10 条）两支；用量类端点一律返回全量数组，前端自行分页（`src/lib/local-api.js:2505`、`src/lib/local-api.js:95`）。

---

## 1. 队列写入与合并语义

### 1.1 文件位置与角色

| 文件 | 生成处 | 作用 |
| --- | --- | --- |
| `~/.tokentracker/tracker/queue.jsonl` | `src/commands/sync.js:569` | 主用量队列，**唯一**上传云的队列（`src/commands/sync.js:3656`） |
| `~/.tokentracker/tracker/project.queue.jsonl` | `src/commands/sync.js:571` | 项目维度队列，不参与云上传 |
| `~/.tokentracker/tracker/queue.state.json` | `src/commands/sync.js:3950`、`:4668` | 上传偏移与状态 |

读取侧自行解析路径、不依赖 sync：`resolveQueuePath()` 返回主队列（`src/lib/local-api.js:83-86`，硬编码 `~/.tokentracker/tracker/queue.jsonl`）；项目队列由主队列路径派生（`path.dirname(qp) + "/project.queue.jsonl"`，`src/lib/local-api.js:135`）。

### 1.2 写入：只写"被触碰的桶"，且写的是累计快照

写入入口是 `enqueueTouchedBuckets`（`src/lib/rollout.js:3169`），只对本次被修改的桶键集合 `touchedBuckets` 生成行（`src/lib/rollout.js:3173`）。

判定一：**累计值未变则不写**——`totalsKey`（`src/lib/rollout.js:4155-4171`）把九个数值列拼成指纹字符串，与上次入队时相同就跳过；额外维度分两处：**优先子集** `PRIORITY_SUBSET_FIELDS` 拼在键尾（`:4167-4170`），而 **`usage_precision` 与输出模型**的拼接发生在调用点而非 `totalsKey` 内部（`:3404-3407`）：

```js
// src/lib/rollout.js:3403-3408
const key = usagePrecision
  ? `${totalsKey(unknownBucket.totals)}|${usagePrecision}`
  : totalsKey(unknownBucket.totals);
const outputKey = outputModel === DEFAULT_MODEL ? key : `${key}|${outputModel}`;
if (unknownBucket.queuedKey === outputKey) continue;
```

判定二：**写入的是该桶的当前累计快照，不是增量**——行内含全部九个用量列 + `conversation_count`（`src/lib/rollout.js:3409-3427`），最后一次性 append：

```js
// src/lib/rollout.js:3482-3484
if (toAppend.length > 0) {
  await fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8");
}
```

结论：**一次同步给同一个桶追加的是"同步完成时该桶的绝对累计值"**。因此队列里同一个键会出现多行，语义是"该桶在若干时刻的快照序列"，而不是"每个快照相加"。

项目队列走同一模式，但键不含 model、含 `project_key`/`project_ref`：

```js
// src/lib/rollout.js:3505-3513（节选）
for (const key of projectTouchedBuckets) {
  const bucket = projectState.buckets[key];
  if (!bucket || !bucket.totals) continue;
  const totals = bucket.totals;
  const queuedKey = totalsKey(totals);
  if (bucket.queuedKey === queuedKey) continue;
  const projectRef = typeof bucket.project_ref === "string" ? bucket.project_ref : null;
  const projectKey = typeof bucket.project_key === "string" ? bucket.project_key : null;
  if (!projectRef || !projectKey) continue;
```

### 1.3 读取：最后一行胜出（last-row-wins）

读端有两个消费者：本地 API 的 `readQueueData` / `readProjectQueueData`（`src/lib/local-api.js:202`、`src/lib/local-api.js:145`），以及 CLI wrapped 聚合器（`src/lib/wrapped-aggregator.js:70`）。

本地 API 的键定义（主队列 / 项目队列各一，后者把 `model` 换成 `project_key`）：

```js
// src/lib/local-api.js:202-211（主队列）
const readQueueData = createAsyncQueueReader({
  label: "readQueueData",
  parseRow(row) {
    if (row?.kind === "account_session_state" || row?.kind === "account_sync_watermark") return null;
    return { key: `${row.source || ""}|${row.model || ""}|${row.hour_start || ""}`, row: normalizeQueueRow(row) };
  },
});
// src/lib/local-api.js:145-153（项目队列）
const readProjectQueueData = createAsyncQueueReader({
  label: "readProjectQueueData",
  parseRow(row) {
    return { key: `${row.project_key || ""}|${row.source || ""}|${row.hour_start || ""}`, row: normalizeQueueRow(row) };
  },
});
```

解析器返回 `null` 表示"这一行不是用量行"（账户会话状态、水印等控制记录被直接丢弃，见 §4.2）。

"最新一条"的实现就是 Map 顺序覆盖：

```js
// src/lib/async-queue-reader.js:49-57（节选）
const changes = new Map();
let malformed = 0;
function parse(line, target) {
  if (!line.trim()) return;
  try {
    const entry = parseRow(JSON.parse(line));
    if (entry) target.set(entry.key, entry.row);
  } catch { malformed++; }
}
```

```js
// src/lib/async-queue-reader.js:86-91
for (const [key, row] of changes) seen.set(key, row);
const tail = new Map();
parse(remainder, tail);
// Legacy files without a final newline are readable, but their provisional
// key never enters the committed map (completion may change that key).
const rows = tail.size ? Array.from(new Map([...seen, ...tail]).values()) : Array.from(seen.values());
```

`Map.set` 对已存在的键**保留插入顺序、替换值**，所以最终 `rows` 里每个键只留最后一次出现的行。**行为等价于"按行序 later-wins"**，与写入侧"后写的是更完整的累计值"前提一致。

CLI 侧独立复刻了同一规则，且注释直接点明它会与 dashboard 对齐：

```js
// src/lib/wrapped-aggregator.js:71-85（节选）
// queue.jsonl is append-only: each sync re-emits the cumulative totals for
// every touched (source, model, hour_start). Naively summing every row
// double-counts every previously synced bucket. Keep only the latest entry
// per tuple — same dedup rule that local-api.js readQueueData enforces for
// the dashboard. This makes `tracker wrapped` totals match what the
// /wrapped dashboard page shows.
const dedup = new Map();
for (const row of Array.isArray(rows) ? rows : []) {
  if (!row || typeof row !== "object") continue;
  const key = `${row.source || ""}|${row.model || ""}|${row.hour_start || ""}`;
  dedup.set(key, normalizeQueueRow(row));
}
```

**口径边界（R1 复核专项，已确证）**：上面这条「唯一合并规则」只对**展示链**成立；库里另有一套 **MAX 口径**，只服务于 repair 的**数据丢失守卫**，两者并存且不互相矛盾：

- 读端（展示真值）：`Map.set` 覆盖式合并，后行胜出（`src/lib/async-queue-reader.js:55`、`:86-91`）。
- 守卫端（对账门禁）：`setMaxTotal`（`src/commands/sync.js:6585-6589`）对同一键取 **MAX**；`mergeMaxTotals`（`:6574-6583`）再跨「现有队列 + 游标态」与「重建结果」两组来源取 MAX，**调用点恰四处**（`:5848`、`:5852`、`:5856`、`:5860`），全在 codex rescan/fork 重建流程的 `projectRepairEnabled` 分支内（该 `if` 块 `:5823-5880`）。**【R2 修正（T1/I1）】上面这四处即"唯一调用族"的全集，范围是 `:5848-5879`（含 `:5865-5879` 的判定循环）；"两处跳过守卫"中只有 `:5874-5879` 在该范围内**——它是总量守卫：某项目键 `rebuiltTotal < existingTotal`、且主桶又没有对应提升时 **跳过本轮修复**（`console.error(... "skipping to avoid data loss")` + `return false`）。另一处 `:5841-5846` 在范围**之外**、判据也不同：它是**项目键集合守卫**（`missingProjectKeys = existingProjectKeys \ rebuiltProjectKeys`，用键集合而非 MAX 值）。同一流程另有 `:5813-5822`（重建产出 0 个 codex 桶）与 `:5882-5890`（文件快照校验失败）两道同类守卫——宁可欠修，不可删史。

为什么需要两套：队列行是「累计快照序列」，展示取最后一行即当前真值；但**撤回（零值行）会把最后一行压成 0**（零行来源 `src/lib/rollout.js:3376`、`:19988`、`src/commands/sync.js:5207`），若守卫也用 last-wins，历史峰值就会被「撤回动作」抹掉，从而误判「无丢失」并放行原子重写。**结论：MAX 用于门禁，last-wins 用于展示**；两者读同一份文件但从不互相写回，因此不产生数据矛盾，唯一后果是撤回历史会让守卫更容易跳过修复（欠修而非错修）。

### 1.4 旧值如何处理：覆盖、零行抵消，以及"不删行"

明确回答任务里的三选一：**既不是相加，也不是简单覆盖后丢弃旧值**，而是：

1. 同键新行在读取时覆盖旧行（§1.3）。
2. 当某个**模型别名行**失效（桶的模型改名、或 unknown 桶被对齐到具体模型），写入侧**追加该旧模型的零值行**，让旧模型在 last-wins 下归零：

```js
// src/lib/rollout.js:3272-3288（节选）
if (dominantModel) {
  if (alignedModel && !group.buckets.has(alignedModel)) {
    toAppend.push(
      JSON.stringify({
        source: group.source,
        model: alignedModel,
        hour_start: group.hourStart,
        input_tokens: zeroTotals.input_tokens,
        ...
        conversation_count: zeroTotals.conversation_count,
      }),
    );
```

3. 撤回整源/整桶时同样是"追加零行"，而不是删除历史行。函数级证据：

```js
// src/lib/rollout.js:20016-20032（节选）
const zero = initTotals();
const lines = [];
for (const [key, entry] of latestRows.entries()) {
  if (keepKeys.has(key)) continue;
  if (totalsKey(entry.row) === totalsKey(zero)) continue;
  lines.push(
    JSON.stringify({
      source,
      model: entry.model,
      hour_start: entry.hour_start,
      ...zero,
    }),
  );
}
if (lines.length === 0) return 0;
await fs.appendFile(queuePath, `${lines.join("\n")}\n`, "utf8");
```

也就是说：**"逻辑删除"= 写零值快照**。物理删除只发生在 §1.6 的 compaction 与 §5 的 repair 重写里。

### 1.5 缓存与并发：签名失效 + 单飞

- **签名**：`dev:ino:size:mtimeNs:ctimeNs`，且在**已打开的 fd 上 stat**（避免 stat/open 之间被原子替换导致签名与字节不符），签名一致直接复用上次快照（`src/lib/async-queue-reader.js:8-10`、`src/lib/async-queue-reader.js:30-35`）。
- **增量续读**：inode 不变、文件变大、且首部与"上次消费边界前 256 字节锚点"都比对一致时，从上一次 `consumedBytes` 继续（`src/lib/async-queue-reader.js:40-47`）。
- **单飞**：同一路径的并发请求共享同一个 Promise（`src/lib/async-queue-reader.js:109-114`）；缓存 LRU 上限 `maxEntries` 默认 4（`src/lib/async-queue-reader.js:16`、`src/lib/async-queue-reader.js:94`）。
- **不霸占事件循环**：64 KiB 分块读，每块后 `yieldToRequests()`（`src/lib/async-queue-reader.js:5`、`src/lib/async-queue-reader.js:79`）。
- **错误处理**：`ENOENT` 清缓存返回空数组；其他读错回退到最后一次成功快照（`src/lib/async-queue-reader.js:98-103`）。

### 1.6 compaction：只压缩未上传的尾部，其余字节不动

设计意图与前置条件（调用者必须持 `sync.lock`）：

```js
// src/lib/upload-queue.js:62-64
// Caller holds sync.lock. Only rewrite pending snapshots; uploaded history and
// queue.state.json stay byte-for-byte unchanged across an atomic replacement.
async function compactPendingQueue({ queuePath, offset = 0, minPendingBytes = BACKLOG_BYTES }) {
```

- 门槛：待上传部分 < 1 MB（`BACKLOG_BYTES`，`src/lib/upload-queue.js:5`）不动；`offset` 必须落在行边界（前一字节为 `\n`）否则放弃（`src/lib/upload-queue.js:70-78`）。
- 去重键：用量行为 `["hourly", source, model, hour_start]`；会话状态行为 `["session", source, session_id]` 并附带时间戳；带 `kind` 的未知控制记录一律不做快照（`src/lib/upload-queue.js:45-60`）。
- 胜出规则：同键保留最后一条；会话状态按**严格更新**替换、等值保留首条；完全没有可移除记录时不重写（`src/lib/upload-queue.js:79-92`）。
- 原子替换：`wx` 新建 → 拷已上传前缀 → 拷胜出行 → `sync()` → 关闭句柄（Windows 无法在句柄打开时替换目标）→ 复查 inode/size/mtime → `rename`；失败清理临时文件（`src/lib/upload-queue.js:94-131`）。

```js
// src/lib/upload-queue.js:112-123（节选；块首行 `await output.close();` 在第 112 行，111 行是 `output.sync()`）
await output.close();
// Windows cannot replace the destination while our read handle is open.
await input.close();
const current = await fs.stat(queuePath);
if (current.ino !== before.ino || current.size !== before.size || current.mtimeMs !== before.mtimeMs) {
  throw new Error("Queue changed during pending compaction");
}
await fs.rename(temporary, queuePath);
```

调用点：`src/commands/sync.js:3543`。

### 1.7 原子性总览

| 路径 | 手段 | 证据 |
| --- | --- | --- |
| 常规桶写入 | 纯 `fs.appendFile`，单行 JSON + `\n` | `src/lib/rollout.js:3483`、`src/lib/rollout.js:20031` 同型 |
| 项目桶写入 | 纯 `fs.appendFile` | `src/lib/rollout.js:3489` 起的同型实现 |
| pending 压缩 | tmp 文件 + `sync()` + 一致性复查 + `rename` | `src/lib/upload-queue.js:94`、`src/lib/upload-queue.js:111`、`src/lib/upload-queue.js:121` |
| repair 重写 | tmp + `rename` | `src/commands/sync.js:3946`、`src/commands/sync.js:5920`、`src/commands/sync.js:6772`、`src/commands/sync.js:7207` |
| 跨进程互斥 | `sync.lock`（`openLock`，来自 `src/lib/fs.js`） | `src/commands/sync.js:18`、`src/commands/sync.js:552` |
| 压缩前置条件 | 调用者必须已持锁 | `src/lib/upload-queue.js:62` |

关于 `fs.js` 的锁实现细节（是否用 O_EXCL、是否写 pid、超时策略）本次**未展开**，仅确认 sync 通过 `openLock(lockPath, …)` 与 `inspectLock` 取得/检查锁：`src/commands/sync.js:439`、`src/commands/sync.js:449`、`src/commands/sync.js:466`、`src/commands/sync.js:476`。

---

## 2. 半小时桶规则

### 2.1 生成函数（权威实现）

```js
// src/lib/rollout.js:4173-4190
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

逐条对照任务的问法：

| 问法 | 答案 | 证据 |
| --- | --- | --- |
| 分钟 <30 → `:00`，≥30 → `:30` | 是。边界取 `>=`，`08:30:00.000` 归 `:30`，`08:29:59.999` 归 `:00` | `src/lib/rollout.js:4177` |
| 是否 UTC | 是。全用 `getUTC*` 与 `Date.UTC`，返回 `toISOString()`（带 `Z`） | `src/lib/rollout.js:4176-4189` |
| 是否用本机时区 | 否。写入侧完全不含本机时区信息 | 同上 |
| 无效时间戳 | 返回 `null`，调用方需自行跳过 | `src/lib/rollout.js:4175` |

### 2.2 桶键构造与解析

四种键共用分隔符 `|`（`src/lib/rollout.js:57`）：

| 函数 | 形态 | 行号 |
| --- | --- | --- |
| `bucketKey(source, model, hourStart)` | `source\|model\|hourStart` | `src/lib/rollout.js:4222` |
| `projectBucketKey(projectKey, source, hourStart)` | `projectKey\|source\|hourStart`（**无 model**） | `src/lib/rollout.js:4228` |
| `groupBucketKey(source, hourStart)` | `source\|hourStart`（整源整点聚合用） | `src/lib/rollout.js:4233` |
| `parseBucketKey(key)` | 按 `source\|model\|hourStart` 反解，缺段用默认值兜底 | `src/lib/rollout.js:4238` |

```js
// src/lib/rollout.js:4222-4231
function bucketKey(source, model, hourStart) {
  const safeSource = normalizeSourceInput(source) || DEFAULT_SOURCE;
  const safeModel = normalizeModelInput(model) || DEFAULT_MODEL;
  return `${safeSource}${BUCKET_SEPARATOR}${safeModel}${BUCKET_SEPARATOR}${hourStart}`;
}

function projectBucketKey(projectKey, source, hourStart) {
  const safeSource = normalizeSourceInput(source) || DEFAULT_SOURCE;
  return `${projectKey}${BUCKET_SEPARATOR}${safeSource}${BUCKET_SEPARATOR}${hourStart}`;
}
```

常量与其含义：

```js
// src/lib/rollout.js:55-57
const DEFAULT_SOURCE = "codex";
const DEFAULT_MODEL = "unknown";
const BUCKET_SEPARATOR = "|";
```

归一化规则（`source` 小写化、`model` 保留大小写只 trim）：

```js
// src/lib/rollout.js:4254-4264
function normalizeSourceInput(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim().toLowerCase();
  return trimmed.length > 0 ? trimmed : null;
}

function normalizeModelInput(value) {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}
```

**注意一个不对称**：`projectBucketKey` 的布局是 `projectKey|source|hourStart`（无 model），而 `parseBucketKey`（按 `source|model|hourStart` 解析）如果被用在项目键上会把 `projectKey` 当 source、`source` 当 model。本次重算：`parseBucketKey` 在 `src/lib/rollout.js` 内共 **15 处调用**（`:1973`、`:3174`、`:3199`、`:3223`、`:3238`、`:3434`、`:3570`、`:3636`、`:3655`、`:7212`、`:12286`、`:13378`、`:14251`、`:16376`、`:20776`）+ 定义 1 处（`:4238`）。**【R2 复核（I6）：15 处已逐处打开确认，不是只按形态判断】**具体容器为——`hourlyState.buckets`（`:3199`、`:3223`、`:3238`、`:3434`、`:3570`、`:7212`、`:12286`、`:13378`、`:14251`、`:16376`、`:20776`）、`attemptHourly.buckets`（`:1973`，遍历 `attemptTouched`）、`enqueueTouchedBuckets` 的 `touchedBuckets` 集合（`:3174`，随后用 `parsed.source` 生成 `groupBucketKey`）、游标态 `state.buckets`（`normalizeHourlyState` 的 v1/v2 迁移，`:3636`、`:3655`）——**全部落在主队列键空间**，项目键另有专门解析（`src/commands/sync.js:6422` 的 `projectUsageKeyFromFields`）。这是"两套键共存但解析器不通用"的潜在坑，见末节。

### 2.3 覆盖范围

`toUtcHalfHourStart(` 的分布（本次逐文件重算：正则 `toUtcHalfHourStart\(`，UTF-8 读取，区分「出现次数」与「所在行数」两个口径）：

| 位置 | 出现次数 | 行数 | 构成 |
| --- | --- | --- | --- |
| `src/lib/rollout.js` | **70** | 66 | 函数定义 1（`:4173`）+ 调用 **69 次 / 65 行**（`:20214` 一行 3 次，`:20265`、`:20418` 各 2 次） |
| `src/commands/sync.js` | 1 | 1 | `:6663`（droid 修复）调用 |
| `src/lib/claude-categorizer.js` | 3 | 3 | **独立副本**定义 `:1175` + 调用 2（`:1250`、`:1279`） |
| **全 `src` 合计** | **74** | **70** | = 72 次调用 + 2 处定义 |
| `test/`（参考） | 10 | 10 | `test/sync-droid-dup-session-repair.test.js` 8、`test/trae-entitlement.test.js` 2 |

`rollout.js` 的覆盖范围是 Claude/Codex/Gemini/OpenCode/Qoder/Cursor/Kiro/Hermes/Trae/Droid/Dsh/Pi/Craft/Goose 等解析路径，例如 `:1007`、`:2111`、`:2360`、`:2605`、`:2867`、`:2975`。

（口径提示：旧版本文把 `rollout.js` 的「70 次出现」误记为「全仓 70 处调用」、把「65 行」当作该文件的调用次数——两者是不同口径，不能混用。）

**`claude-categorizer.js:1175-1191` 的副本与 `rollout.js:4173-4190` 语义等价（已逐行比对，原自标 `【未能确认】` 已解除）**：取整逻辑逐行相同（`minutes >= 30 ? 30 : 0`、`Date.UTC(getUTCFullYear/Month/Date/Hours, halfMinute, 0, 0)`）；唯一差异是 rollout 版先用中间变量 `bucketStart` 再 `toISOString()`，categorizer 版直接 `return new Date(Date.UTC(...)).toISOString();`，行为与返回值一致。

grok 修复路径是**第三种独立实现**（数学等价，毫秒整除半小时）：

```js
// src/commands/sync.js:4538-4549
function toGrokRepairHalfHourStart(value) {
  if (value == null) return null;
  const millis =
    typeof value === "number"
      ? value < 10_000_000_000
        ? value * 1000
        : value
      : Date.parse(String(value));
  if (!Number.isFinite(millis)) return null;
  const halfHourMs = 30 * 60 * 1000;
  return new Date(Math.floor(millis / halfHourMs) * halfHourMs).toISOString();
}
```

注意其输入兼容：小于 1e10 的数字按**秒**处理再乘 1000（`src/commands/sync.js:4542`）。

并非所有来源都用半小时桶：copilot 另有 `copilotOtelAggregateKey(model, bucketStart)`（`src/lib/rollout.js:17462`），其桶粒度 `【未能确认】`。

### 2.4 从桶到日：转换只发生在读取/展示侧

时区上下文来自**查询参数**（不是服务端配置）：`tz`（IANA 名）优先，`tz_offset_minutes`（整数分钟）兜底。**【R2 复核修正（E1）：缺省时的回退基准在两条路径上不同，不能笼统写成"静默退化为 UTC 日"】**

- **「日」这一族（`rowDayKey`）→ UTC 日**：`rowDayKey`（`src/lib/local-api.js:213-225`）只在 `timeZoneContext.timeZone` 或有限 `offsetMinutes` 存在时才走 `getZonedParts`；两者皆缺时直接 `return hs.slice(0, 10)`，即 `hour_start` 的 **UTC 日期**。由它产出的日键（`buildDailyAggregates` 逐行调用，`:260`、`:266`）以及全部**日键区间过滤**（`day >= from && day <= to`）都以这条为准。
- **`getZonedParts` 系（小时键 / 今天 / 徽章）→ 运行环境本机时区**：`getZonedParts`（`:521-592`）是三级回退——① 请求 `tz` 的 IANA formatter（`:525-571`）→ ② `tz_offset_minutes` 固定偏移（`:573-583`）→ ③ **本机时区**（`:585-592`，用 `dt.getFullYear()/getMonth()/getDate()/getHours()` 这些**本地 getter**）。凡**没有** `timeZoneContext && (timeZone || Number.isFinite(offsetMinutes))` 守卫、直接调它的位置都落第 ③ 级：`aggregateHourlyByDay` 的日过滤与小时键（`:606`、`:608`）、summary 的 `todayStr`（`:2382`）、heatmap 的 `todayStr`（`:2538`）、`night_owl` 徽章的小时（`:482`）。
- **可产生的后果（源码谓词推出，未起服务实测）**：非 UTC 机器上，不带 `tz`/`tz_offset_minutes` 调 `-usage-hourly` 时，响应 `day` 的默认值是 **UTC 今天**（`:3147` `new Date().toISOString().slice(0, 10)`），而过滤谓词用的是**本机时区日键**（`:608`）——两者基准不同：返回的是"本机时区日 = `day`"的桶，跨日时段可能整日为空。同类基准错位还出现在 summary 的 `rolling.last_7d/last_30d`（窗口终点取本机时区"今天"`:2382`，再与 UTC 日键构成的 `allDaily` 比对（`:2390-2397`））与 heatmap 的窗口终点（`:2538-2544`）。
- **实际暴露面（R2 修正 R2-3 报告的结论）**：三端壳与前端都会显式传 `tz` —— macOS 的 `withAccountQueryItems` 本身就是 `withTimeZoneQueryItems` + `account=1`（`TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift:241-254`，`fetchHeatmap`/`fetchHourly` 都经它），Linux 扩展 `_getJson` 自动附 `tz`/`tz_offset_minutes`（`TokenTrackerLinux/.../extension.js:997`），Windows `UsagePoller` 自带 `tzQuery`（`TokenTrackerWin/UsagePoller.cs:321`）。因此这是**留给不带 tz 的调用者（裸 HTTP、脚本、测试）的坑**，不是三端壳的现网故障；但"两条路径缺省基准不同"是源码事实，跨端时区设计必须按"哪一级回退给谁"分别处理。

```js
// src/lib/local-api.js:213-225（节选）
function rowDayKey(row, timeZoneContext) {
  const hs = row.hour_start;
  if (!hs) return "";
  if (timeZoneContext && (timeZoneContext.timeZone || Number.isFinite(timeZoneContext.offsetMinutes))) {
    const parts = getZonedParts(new Date(hs), timeZoneContext);
    const key = formatPartsDayKey(parts);
    if (key) return key;
  }
  return hs.slice(0, 10);
}
// src/lib/local-api.js:512-519
function getTimeZoneContext(url) {
  const tz = String(url.searchParams.get("tz") || "").trim();
  const rawOffset = Number(url.searchParams.get("tz_offset_minutes"));
  return { timeZone: tz || null, offsetMinutes: Number.isFinite(rawOffset) ? Math.trunc(rawOffset) : null };
}
```

`getZonedParts` 优先用 IANA 时区（`Intl.DateTimeFormat`，带 fail-soft 与 16 条 LRU 缓存），失败则退回固定偏移（`src/lib/local-api.js:521`、`src/lib/local-api.js:545`、`src/lib/local-api.js:96`）。

**跨日/跨时区行为**：桶本身是 UTC 的，因此"某个桶属于哪一天"完全由查询侧时区决定。一个 `2026-10-09T18:20:00Z` 的桶：

- 默认（无 tz 参数）→ 日键 `2026-10-09`（这是 `rowDayKey` 族，即 UTC 日；`getZonedParts` 系另见上）
- `tz=Asia/Kathmandu`（UTC+5:45）→ 当地 `2026-10-10 00:05` → 日键 `2026-10-10`

这一非整小时偏移被测试直接锁定（`test/session-cross-day-usage.test.js:40-58`，测试名含 "quarter-hour timezone boundaries"）。

### 2.5 `heatmap-compact.js` 与日汇总的关系

要分清两件事：

**(a) 本地热力图端点**把桶聚合为**日**，网格一律按 UTC 日运算（`todayStr` 取自请求时区，**无 tz 时取运行环境本机时区**，见 §2.4；此后全 UTC：`src/lib/local-api.js:2538-2544`）；`level` 由窗口最大值按 0.25/0.5/0.75 分档（`src/lib/local-api.js:2549-2557`）。响应中 `week_starts_on` 恒为 `"sun"`（`:2586`），`streak_days` **恒为 0**（`:2588`）——真实 streak 只在云端紧凑载荷里（见下）。

**(b) `heatmap-compact.js` 是云端 `format=compact` 稀疏载荷的读取适配器**。文件头注释给了动机：该端点是最大出流量（"2.55 GB/day on 2026-09-21, 31% of all egress"），且展开逻辑**逐步对齐** edge 补丁实现（`src/lib/heatmap-compact.js:3-24`）。三处与聚合语义直接相关的硬规则：

1. 日键按 **UTC 日历日**推进：`nextDayKey` 用 `setUTCDate`（`src/lib/heatmap-compact.js:26-31`）。
2. 重复日 **last-row-wins**；present-but-empty 的 `models` 是 `{}` 而非 `null`（`src/lib/heatmap-compact.js:68-71`）。
3. `billable_total_tokens` **直接复制** `total_tokens`，`level` 由窗口最大值派生（阈值 0.25/0.5/0.75，`src/lib/heatmap-compact.js:74-83`、`:89-97`）。

```js
// src/lib/heatmap-compact.js:89-97（节选）
    const hit = byDay.get(cursor);
    const total = hit ? hit.total : 0;
    cells.push({
      day: cursor,
      total_tokens: total,
      billable_total_tokens: total,
      level: levelOf(total),
      models: hit ? hit.models : null,
    });
```

模型名走字典 + 索引扁平对；`model_names` 缺失时**整包拒绝**而不是静默丢数据（`src/lib/heatmap-compact.js:52`），索引无值或尾随单索引被丢弃（`src/lib/heatmap-compact.js:63-68`）。

**与日汇总的关系总结**：`heatmap-compact` 只处理**已由边端聚合好的日粒度**数据，不做桶→日转换；桶→日的转换只发生在 `rowDayKey`/`buildDailyAggregates`（`src/lib/local-api.js:213`、`src/lib/local-api.js:260`）。二者共用同一分档阈值（0.25/0.5/0.75）与 UTC 日键约定，因此渲染结果可对齐。

---

## 3. 总量聚合路径

### 3.1 唯一入口：`scopedQueueRows`

除会话/项目/上下文体检等支线外，所有用量端点的第一跳都是同一个函数：

```js
// src/lib/local-api.js:398-412
function getRequestedUsageScope(url) {
  if (url.searchParams.get("include_account_level") === "1") return "all";
  return normalizeUsageScope(url.searchParams.get("scope"));
}

async function scopedQueueRows(queuePath, url) {
  const scope = getRequestedUsageScope(url);
  const allRows = await readQueueData(queuePath);
  return {
    scope,
    allRows,
    rows: filterRowsByUsageScope(allRows, scope),
    excludedSources: listExcludedSources(allRows, scope),
  };
}
```

这里做了两件事：**队列去重（`readQueueData`）** 与 **来源范围过滤（`filterRowsByUsageScope`）**。范围语义与来源分类定义在 `src/lib/source-metadata.js`（本次未展开）。

### 3.2 第二跳：读取时的历史修正

行先过 `normalizeQueueRow`（`src/lib/local-api.js:170`），做两类**读时修补**（不重写文件）：

1. **历史 codex 行的"input 含 cache"口径**：用精确不变量识别（`total == input + output` 且 `cached > 0` 且 `input >= cached`），读取时把 `input_tokens` 减去 cached 部分（`src/lib/local-api.js:155-178`）。
2. **cursor / antigravity 的 `billable = 0`**：置为等于 `total_tokens`（注释给出两处成因：GitHub issue #106 的 kind 门控、一次 O(N) 重构误删，`src/lib/local-api.js:179-198`）。

```js
// src/lib/local-api.js:191-198
const sourceName = String(normalized.source || "").toLowerCase();
if (sourceName === "cursor" || sourceName === "antigravity") {
  const totalTokens = Number(normalized.total_tokens || 0);
  const billable = Number(normalized.billable_total_tokens || 0);
  if (totalTokens > 0 && billable < totalTokens) {
    normalized = { ...normalized, billable_total_tokens: totalTokens };
  }
}
```

### 3.3 第三跳：按天聚合（两级缓存）

缓存结构是 `WeakMap<行数组, Map<缓存键, Promise<日数组>>>`：键维度 = **行数组对象身份** + **时区/偏移/价格修订**。价格修订在聚合中途变化会**重算**而非混用两个修订（`src/lib/local-api.js:104`、`src/lib/local-api.js:115-117`、`src/lib/local-api.js:243-249`）。

```js
// src/lib/local-api.js:104-117（节选）
const dailyAggregationCache = new WeakMap();
const zonedPartsFormatters = new Map();
function timeZoneContextKey({ timeZone, offsetMinutes } = {}) {
  return `${timeZone || ""}|${Number.isFinite(offsetMinutes) ? offsetMinutes : ""}|p${getPricingRevision()}`;
}
```

相关常量：`PROJECT_USAGE_MAX_ENTRIES = 10`、`MAX_TIME_ZONE_CACHE_ENTRIES = 16`、`AGGREGATION_BATCH_ROWS = 256`（每 256 行让出事件循环）（`src/lib/local-api.js:95-97`、`src/lib/local-api.js:264`）。

```js
// src/lib/local-api.js:263-299（节选）
  for (const row of normalizedRows) {
    if (++processed % AGGREGATION_BATCH_ROWS === 0) await yieldToRequests();
    if (!row.hour_start) continue;
    const day = rowDayKey(row, timeZoneContext);
    if (!day) continue;
    ...
    a.total_cost_usd += computeRowCost(row);
    const model = row.model || "unknown";
    a.models[model] = (a.models[model] || 0) + (row.total_tokens || 0);
  }
  const daily = Array.from(byDay.values()).sort((a, b) => a.day.localeCompare(b.day));
```

要点：**成本按行计算后累加**（`src/lib/local-api.js:285`），不是"先合 token 再定价"——与模型分解端点同口径，后者注释解释了原因（优先/长上下文档位非线性，`src/lib/local-api.js:2634-2640`）。

### 3.4 各口径的分叉点

| 口径 | 端点 | 计算位置 | 关键行 |
| --- | --- | --- | --- |
| 总量（hero） | `/functions/tokentracker-usage-summary` | 在 `aggregateByDay` 结果上过滤 `from/to` 后 reduce | `src/lib/local-api.js:2358`、`src/lib/local-api.js:2363`、`src/lib/local-api.js:2365` |
| 滚动 7/30 天 | 同上，`rolling` 字段 | `collectDays(n)` + `sumDays` | `src/lib/local-api.js:2390`、`src/lib/local-api.js:2399`、`src/lib/local-api.js:2406` |
| 按天 | `/functions/tokentracker-usage-daily` | 直接回传日数组 | `src/lib/local-api.js:2425`、`src/lib/local-api.js:2430` |
| 按来源 + 按模型 | `/functions/tokentracker-usage-model-breakdown` | 自建 `bySource` → `models` 两级 Map | `src/lib/local-api.js:2596`、`src/lib/local-api.js:2602`、`src/lib/local-api.js:2640` |
| 按月 | `/functions/tokentracker-usage-monthly` | `byMonth`，月键 = `day.slice(0,7)` | `src/lib/local-api.js:3156`、`src/lib/local-api.js:3168` |
| 某日逐小时（**当地小时**，非逐半小时） | `/functions/tokentracker-usage-hourly` | `aggregateHourlyByDay(rows, day, tz)`：两个半小时桶按当地小时合并，**键空间**为 24 个整点键 `${dayKey}T HH:00:00`，响应里不存在 `:30` 键；返回 `Array.from(byHour.values())`（`:639`），即**只含有数据的桶，不预填充 24 项** | `src/lib/local-api.js:3146`、`src/lib/local-api.js:600-609`、`:639` |
| 热力图 | `/functions/tokentracker-usage-heatmap` | 日数组 + 网格 + 分档 | `src/lib/local-api.js:2533`、`src/lib/local-api.js:2549` |
| 按项目 | `/functions/tokentracker-project-usage-summary` / `-detail` | 走项目队列，另一套键 | `src/lib/local-api.js:2731`、`src/lib/local-api.js:2828` |

总量口径的字段集合在 reduce 里显式列出（九个字段），`conversation_count` 亦参与（`src/lib/local-api.js:2375`）；成本最后 `toFixed(6)` 返回（`src/lib/local-api.js:2415`）。

按来源 + 按模型的路径要点（含"服务端自报成本优先"与"混合来源"标记）：

```js
// src/lib/local-api.js:2624-2626
const reportedCost = ["grok", "cline"].includes(src) && Number(row.total_cost_usd) > 0;
const costSource = reportedCost ? "provider_reported" : "model_pricing";
ma.cost_source = ma.cost_source && ma.cost_source !== costSource ? "mixed" : costSource;
```

以及价格目录在聚合中途被刷新的重入保护：

```js
// src/lib/local-api.js:2642
if (pricingRevision !== getPricingRevision()) return handleLocalApi(req, res, url);
```

排序口径：模型分解按 `total_tokens` 降序（`src/lib/local-api.js:2650`），项目汇总按 `billable_total_tokens` 降序（`src/lib/local-api.js:2785`）。

### 3.5 调用链总图（自端点向下）

```
GET /functions/tokentracker-usage-summary      (src/lib/local-api.js:2358)
  └─ scopedQueueRows(qp, url)                  (src/lib/local-api.js:403)
       ├─ readQueueData(queuePath)             (src/lib/local-api.js:202)
       │    └─ createAsyncQueueReader(...)     (src/lib/async-queue-reader.js:16)
       │         └─ parseRow → key=(source|model|hour_start)，last-row-wins
       │                                                 (src/lib/async-queue-reader.js:86)
       ├─ filterRowsByUsageScope(rows, scope)  (src/lib/source-metadata.js)
       └─ normalizeQueueRow(row)               (src/lib/local-api.js:170)
  └─ aggregateByDay(rows, tz)                  (src/lib/local-api.js:227)
       └─ buildDailyAggregates(...)            (src/lib/local-api.js:260)
            └─ rowDayKey(row, tz)              (src/lib/local-api.js:213)
  └─ daily.filter(from/to) → reduce → totals   (src/lib/local-api.js:2364)
```

### 3.6 项目维度是**另一条链**

项目端点不走 `readQueueData`，而是 `readProjectUsageContext`（`src/lib/local-api.js:125`）+ `readProjectQueueData`（`src/lib/local-api.js:145`）。它额外做三件事：

1. 剔除 claude-mem 伪项目（过滤键由 rollout 常量派生，避免重命名走样，`src/lib/local-api.js:88-94`）。
2. 建立 `dayInRange` 谓词（`from`/`to` 闭区间；无范围恒真）。
3. 每行的日键只计算一次（注释指出 `rowDayKey` 内部的 `Intl.DateTimeFormat` 是主导成本，`src/lib/local-api.js:121-124`）。

```js
// src/lib/local-api.js:130-139（节选）
  const dayInRange = (day) => {
    if (!hasRange) return true;
    if (!day) return false;
    return (!from || day >= from) && (!to || day <= to);
  };
  const projectQueuePath = path.join(path.dirname(qp), "project.queue.jsonl");
  const projectRows = (await readProjectQueueData(projectQueuePath)).filter(
    (row) => row.project_key !== CLAUDE_MEM_OBSERVER_PROJECT_KEY,
  );
```

兜底：**仅当项目队列完全为空**时改用主队列按 source 聚合，且刻意不伪造 `project_ref`（旧行为会造出 `https://codex.ai` 之类可点击假链接，`src/lib/local-api.js:2803-2818`）。

---

## 4. 去重机制全景

TokenTracker 的去重是**分层叠加**的，每层针对不同重复来源、用不同键。下面逐层给出键与证据。

### 4.1 第一层：队列行 last-row-wins

- 键：`(source, model, hour_start)`
- 规则：后出现的行覆盖先出现的行
- 证据：`src/lib/async-queue-reader.js:86`、`src/lib/local-api.js:207`；项目队列把 `model` 换成 `project_key`（`src/lib/local-api.js:149`）
- 作用：吸收"同一桶被反复写入累计快照"造成的重复
- 测试：`test/wrapped-aggregator-dedup.test.js:23`（"dedupes by (source, model, hour_start) keeping latest"）、`test/local-api-project-usage.test.js:128`（"dedups append-only rows by (project, source, hour)"）

### 4.2 第二层：账户级跨设备去重（`account-usage-dedup.js`）

背景：账户级来源（cursor、trae-cn）的数据取自**每账户**的云 API，因此每台设备都存了**同一份**数据的副本——去重目标不是"消除重复事件"，而是"把 N 份副本收敛成 1 份真值"。

**分支 A：`cursor` —— 无稳定会话身份 → 整行 MAX 收敛**（函数 `pickLegacyRow`，`src/lib/account-usage-dedup.js:188`）

- 分组键：`(source, hour_start)` → 再按 `model`（`src/lib/account-usage-dedup.js:230`、`src/lib/account-usage-dedup.js:240`）
- 收敛规则（三级决胜）：`total_tokens` 最大 → `updated_at` 更新 → `device_id` 字典序更小（`src/lib/account-usage-dedup.js:195-207`）
- 是 **MAX 而不是 SUM**，因为每行是同一份数据的不同设备副本

```js
// src/lib/account-usage-dedup.js:184-198（节选）
// Mirrors the legacy SQL DISTINCT ON (hour, source, model)
// ORDER BY total_tokens DESC, updated_at DESC (device_id asc as the final
// deterministic tiebreak). Used ONLY for account-level sources without a
// stable session identity ('cursor') and watermark-less history.
function pickLegacyRow(rows) {
  let best = null;
  for (const row of rows) {
    ...
    const rowTotal = Number(row.total_tokens) || 0;
    const bestTotal = Number(best.total_tokens) || 0;
    if (rowTotal !== bestTotal) { if (rowTotal > bestTotal) best = row; continue; }
```

**分支 B：`trae-cn` —— 有会话身份 → 会话级 LWW**（函数 `upsertAccountSessionStates`，`src/lib/account-usage-dedup.js:113`）

- 身份三元组 `(user_id, source, session_id)`；**`device_id` 不是身份**（`src/lib/account-usage-dedup.js:20-22`）
- 内存键：`${source}|${session_id}`（`src/lib/account-usage-dedup.js:120`）
- 规则：**严格更新**（`>`）才整行替换；相等时间戳保留先落库的那条，因此重放幂等（`src/lib/account-usage-dedup.js:127-132`、`:43-46`）
- 三类修正在同一操作里收敛：下调 token / 换模型 / 换半小时桶（`src/lib/account-usage-dedup.js:30-32`）
- **缺席不等于删除**——空快照什么都不断言（`src/lib/account-usage-dedup.js:34-36`）
- 回聚为小时行时按 `(bucket_start, source, model)` 求和，`conversations` = 会话条数（不是消息数）（`src/lib/account-usage-dedup.js:159-174`）

```js
// src/lib/account-usage-dedup.js:120-133（节选）
    const key = `${row.source}|${row.session_id}`;
    const stored = table.get(key);
    if (!stored) { table.set(key, row); applied += 1; continue; }
    const storedMs = parseMs(stored.snapshot_verified_at) ?? -Infinity;
    const rowMs = parseMs(row.snapshot_verified_at) ?? -Infinity;
    if (rowMs > storedMs) { table.set(key, row); applied += 1; }
```

控制记录用 `kind` 标记，读端一律跳过（不进入用量聚合）：`SESSION_STATE_KIND = "account_session_state"`（`src/lib/account-usage-dedup.js:55-57`）、`src/lib/local-api.js:205`。压缩阶段对这类行还做 token 自洽性校验（`total == input + output + cached + cache_creation`，任一不满足即**不可压缩**，`src/lib/upload-queue.js:9-18`）。

### 4.3 第三层：会话碎片合并（`session_hash`）

会话分析把同一会话的多个文件碎片按哈希折叠，而不是按文件名：

- 哈希函数：`sessionHash(source, id)`（`src/lib/session-analytics.js:118`）
- 合并函数：`mergeSessionFragments(rows)`（`src/lib/session-analytics.js:2665`）
- 合并键：`row.session_hash`（`src/lib/session-analytics.js:2668`）

```js
// src/lib/session-analytics.js:2665-2693（节选）
function mergeSessionFragments(rows) {
  const byHash = new Map();
  for (const row of rows || []) {
    const key = row.session_hash;
    const cur = byHash.get(key);
    if (!cur) {
      byHash.set(key, { ...row, tokens: ..., _repr_tokens: finite(row.total_tokens) });
      continue;
    }
    cur.turns = finite(cur.turns) + finite(row.turns);
    ...
    addTotals(cur.tokens, row.tokens);
    if (Array.isArray(row.usage_buckets)) {
      cur.usage_buckets = [...(cur.usage_buckets || []), ...row.usage_buckets];
    }
    cur.model_usage = normalizeModelUsageRows([...(cur.model_usage || []), ...(row.model_usage || [])]);
    cur.total_tokens = finite(cur.total_tokens) + finite(row.total_tokens);
```

可加字段累加；上下文类字段取 **max**（`src/lib/session-analytics.js:2704-2706`）；布尔取 **or**（`src/lib/session-analytics.js:2707-2711`）；`usage_precision` / `cost_source` 不一致时标 `"mixed"`（`src/lib/session-analytics.js:2709-2710`）。

父会话链接冲突时**清空并打标**而不是二选一：

```js
// src/lib/session-analytics.js:2714-2718
if (!cur.parent_session_id && row.parent_session_id) cur.parent_session_id = row.parent_session_id;
if (cur.parent_session_id && row.parent_session_id && cur.parent_session_id !== row.parent_session_id) {
  cur.parent_session_id = null;
  cur.parent_link_conflict = true;
}
```

### 4.4 第四层：fork / clone 拷贝检测（四套机制）

**(a) OpenCode：消息指纹 + 会话归属**——`isOpencodeForkCopy(fingerprintIndex, fingerprint, messageKey)`（`src/lib/rollout.js:3805`），配套 `deriveOpencodeMessageKey`（`:3722`）、`opencodeMessageKeySession`（`:3795`）、`normalizeMessageKeyPart`（`:3717`）。

`test/opencode-fork-dedup.test.js` 锁定的不变量：拷贝前缀只计一次而 fork 自己的续写保留（`:102`）；fork 出现在后续同步轮次也能去重（`:223`）；指纹机制上线前已计数的拷贝能修回（`:260`）；子先于父出现仍去重（顺序无关，`:329`）；**token 数恰好相同的真实不同调用不能误删**（`:353`）；**同会话内重复消息永不删**（`:388`）；指纹归属修正后保留备用同会话所有者（`:487`）；去重按 cursor 命名空间隔离（`:627`）。

**(b) Codex：重放时间窗**（跨日 fork 由日期守卫，同日突发由 500ms 间隔判定）：

```js
// src/lib/rollout.js:4210-4214
const FORK_REPLAY_GAP_MS = 500;

function isForkedReplayToken({ isForkedRollout, rolloutDate, currentDate }) {
  return Boolean(isForkedRollout && rolloutDate && currentDate && currentDate < rolloutDate);
}
```

注释给出的经验值：重放行间隔亚毫秒至数毫秒，真实回合 ≥~4.6 秒，重放→实时断裂 ≥11 秒；500ms 刻意偏向下界（"合并一个真实快速回合（低估）比留下一个慢重放（有界高估）更糟"，`src/lib/rollout.js:4203-4209`）。

**真正的判定实现**在解析循环里（`src/lib/rollout.js:2588-2601`）：`tokenMs - prevForkedTokenMs >= FORK_REPLAY_GAP_MS` → `replayPrefixActive = false`（`:2588-2590`，实时回合重置突发窗）；`tokenMs - prevForkedTokenMs < FORK_REPLAY_GAP_MS` → `forkedReplaySkip = true`（`:2591-2594`，视为重放）；skip 点是 `if (forkedReplaySkip || isForkedReplayToken(...)) continue;`（`:2601`）。也就是说：**跨日靠日期守卫（`:4212-4214` 的 `isForkedReplayToken`），同日靠 500ms 突发检测**（R1 复核确证）。

**(c) Codex 事件级哈希**：游标键 `codexHashes`（`src/lib/rollout.js:382`），诊断分支 `src/lib/rollout.js:351`。哈希输入 `【未能确认】`。

**(d) Claude：消息 + 请求 ID 键**——`claudeMessageDedupKey(obj)`（`src/lib/rollout.js:4914-4918`）的语义是：**`msgId` 是硬门槛**，缺失即返回 `null`（该行不去重）；有 `reqId` 时合成 `${msgId}:${reqId}`，否则**退化为纯 `msgId`**（去重依然生效）。使用处带**开关语义**（`const dedupHash = seenMessageHashes ? claudeMessageDedupKey(obj) : null;`，`src/lib/rollout.js:2854`）。

必须把两个不同实现的守卫分开记：**「`msgId && reqId` 才建键」是 v3 地面真值扫描（`src/commands/sync.js:227-233`）的历史缺陷**——它静默关闭了所有缺 `requestId` 的 provider 的去重（DeepSeek/Kimi/Mimo/MiniMax 的 anthropic 兼容端点，以及 Claude Code 的子代理/thinking 通道），使真值膨胀 1.6–3.7 倍；v4 修掉了它。解析侧函数 `claudeMessageDedupKey` 从来没有这个严格判据（R1 复核确证 `:4914-4918` 的实现）。教训：**去重键的守卫条件会静默退化成"不去重"**，且不要把扫描侧与解析侧的同名语义混为一谈。

### 4.5 第五层：双安装 / WSL 去重

机制是**光标命名空间隔离**（`native` / `wsl` 各一套），不是事后合并——每个安装按自己的偏移解析，天然不会重复回读（`src/lib/multi-install-parser.js:31-56`）。

三个必须记住的判定：

1. **单安装归位按真实身份，不按模式偏好**：用 `wsl.isUncPath(...)` 判定幸存者是不是 WSL，因为非 both 模式下槽位名不可靠（`src/lib/multi-install-parser.js:13-22`）。
2. **扁平→命名空间迁移的归属靠一次性探针**：`detectInstall` 命中**恰好一个**安装才只 seed 那一个；无探针、无命中、多命中、探针报错**全部退回 seed 全部**——"少了去重状态会整段重复计数，而有界回填缺口是更便宜的失败"（`src/lib/multi-install-parser.js:61-85`）。
3. **both 模式下按路径 Set 去重**文件清单（`src/lib/multi-install-parser.js:92-110`）。

```js
// src/lib/multi-install-parser.js:70-85（节选）
function resolveMigrationSeedKeys({ paths, installKeys, providerName, cursors, detectInstall }) {
  if (typeof detectInstall !== "function") return installKeys;
  const state = cursors[providerName] && typeof cursors[providerName] === "object" ? cursors[providerName] : {};
  const isFlat = state.native === undefined && state.wsl === undefined;
  if (!isFlat || Object.keys(state).length === 0) return installKeys;
  const hits = [];
  for (const key of installKeys) {
    let owns = false;
    try { owns = detectInstall(paths[key], state, key) === true; } catch (_e) { }
    if (owns) hits.push(key);
  }
  return hits.length === 1 ? hits : installKeys;
}
```

对应测试：`test/claude-wsl-dual-install.test.js:65`、`:93`、`:138`、`:170`、`:219`；`test/codex-wsl-shadow.test.js:181`；`test/dual-install-*`（4 个文件）。

### 4.6 第六层：跨根同 UUID 归组（Claude / Codex 会话分析）

会话文件在多个扫描根（原生、WSL、额外根）下按**文件名 UUID** 归组（`groupClaudeFilesAcrossRoots`，`src/lib/session-analytics.js:1824`）。两条保守规则：

1. **同一根内出现重复 UUID → 该 UUID 整体退出去重**（无法判断另一根的副本镜像的是哪个兄弟文件；"歧义重复代价低，删掉会话代价高"，`src/lib/session-analytics.js:1830-1845`）。
2. **只在字节完全相同时折叠**，胜者取 mtime 更新者；内容发散的副本**全部保留**（`sameFileContent` `:1879`、`dedupeClaudeFilesAcrossRoots` `:1895-1909`）。

```js
// src/lib/session-analytics.js:1895-1907（节选）
function dedupeClaudeFilesAcrossRoots(groups) {
  const mtimeOf = (filePath) => { try { return fs.statSync(filePath).mtimeMs; } catch { return -Infinity; } };
  const out = [];
  for (const filePaths of groupClaudeFilesAcrossRoots(groups)) {
    if (filePaths.length <= 1 || !sameFileContent(filePaths)) { out.push(...filePaths); continue; }
    out.push(filePaths.reduce((winner, candidate) => (mtimeOf(candidate) > mtimeOf(winner) ? candidate : winner)));
  }
```

Codex 侧同型但更简单：`groupCodexFiles(filePaths)` 按文件名 UUID 归组（`src/lib/session-analytics.js:1912-1926`）。

`test/session-analytics-claude-cross-root-dedup.test.js` 锁定的不变量：

| 用例 | 不变量 | 行号 |
| --- | --- | --- |
| 两根同会话折叠为一 | 跨根归组生效 | `:65` |
| "较旧副本在另一根更新"时胜者改变 | 胜者按 mtime 而非根序 | `:77` |
| 不同会话都保留 | 不误合 | `:87` |
| 单根原样返回（含同 UUID 兄弟） | 单根不触发归组 | `:98` |
| 单根同 UUID 兄弟**关闭**去重 | 歧义时选择"保留" | `:120` |
| 非 UUID 文件名永不参与 | 键形态受限 | `:139` |
| 顺序跟随根序 | 原生优先级稳定 | `:156` |
| 同 UUID 但内容不同 → 都保留 | 不按 mtime 二选一 | `:165` |
| Claude 跨根共享前缀计一次 + 两侧发散尾部 | **事件级**并入而非整文件取舍 | `:174` |
| Codex 跨根重复 token 事件只计一次 | 事件级并入 | `:288` |
| 分组镜像读取中途失败时丢弃暂存记录 | 失败不留半量 | `:247` |

### 4.7 第七层：事件级并入（Claude / Codex 跨根）

上面两个用例（`:174`、`:288`）表明 **token 事件**也被并入去重，而不是仅按文件取舍。实现位于会话扫描流程（`scanClaudeSession` 的 `seenMessageHashes` 分支，`src/lib/rollout.js:2854`），以及会话分析的跨根并入路径。**具体的并入键实现本次未逐行读取** → `【未能确认】`（但行为由上述测试锁定）。

### 4.8 `conversation_count` 的语义

**它不是一个跨来源可比的指标**。源码里至少三种口径：

1. **按会话**（首见即 1，后续同步 0）——Craft：

```js
// src/lib/rollout.js:17232-17243（节选）
// conversation_count: 1 the first time we see a session, 0 on subsequent
// syncs of the same session. NOTE: this differs from omp/Claude which
// count one-per-assistant-message. Cross-provider "conversations" totals
// are therefore not directly comparable — Craft's are per-session.
const delta = {
  ...
  conversation_count: prev.total === 0 ? 1 : 0,
};
```

2. **按主会话 user 文本消息**（每条用户文本消息 +1，排除 `tool_result` 与子代理）——计数语句是 `userBucket.totals.conversation_count += 1;`（`src/lib/rollout.js:2833`），位于 `isMainSession && userObj?.type === "user"` → `hasText` 的内层分支内（`src/lib/rollout.js:2807-2837`）。**源码自身两处注释互相矛盾**：解析侧注释写的是 "Count user-typed messages as conversations (main sessions only). Exclude tool_result messages"（`src/lib/rollout.js:2807-2809`，与代码一致），而 Craft 侧注释却自称 Claude「count one-per-assistant-message」（`src/lib/rollout.js:17233-17234`，与代码不符）。**以代码为准**：Claude 路径按 user 文本消息计，Craft 侧注释对 Claude 口径的描述是错的（R1 复核确证）。
3. **按请求数**——例如 `conversation_count: delta.requests`（`src/lib/rollout.js:17044`）

另外还有几处刻意置 0 以避免重复计数（"seenSessions.has(sessionId) ? 0 : 1"，`src/lib/rollout.js:18886`；`conversation_count = 0`，`src/lib/rollout.js:9287`）。

因此 dashboard 上的 "conversations" 是**各来源自报口径的混合值**，跨来源相加只有粗略意义。云端还曾因字段映射 bug 把 `conversation_count` 读成 `b.conversations` 而长期归零，修复方式是重置上传偏移、整队列重放（`src/commands/sync.js:249-257`）。

### 4.9 逐层去重键汇总表

| 层 | 场景 | 去重键 | 收敛规则 | 证据 |
| --- | --- | --- | --- | --- |
| 1 | 队列行 | `source\|model\|hour_start` | 后行覆盖前行 | `src/lib/async-queue-reader.js:86`、`src/lib/local-api.js:207` |
| 1' | 项目队列行 | `project_key\|source\|hour_start` | 同上 | `src/lib/local-api.js:149` |
| 2a | 账户级 cursor | `(source, hour_start, model)` | 整行 MAX（total→updated_at→device_id） | `src/lib/account-usage-dedup.js:188`、`:197` |
| 2b | 账户级 trae-cn | `source\|session_id` | 严格更新才替换，等值留首条 | `src/lib/account-usage-dedup.js:120`、`:127` |
| 3 | 会话碎片 | `session_hash` | 数值累加、上限取 max、布尔取 or | `src/lib/session-analytics.js:2668`、`:2704` |
| 4a | OpenCode fork | 消息指纹 + 会话归属 | 拷贝前缀只计一次，同会话内重复不删 | `src/lib/rollout.js:3805`；`test/opencode-fork-dedup.test.js:388` |
| 4b | Codex fork 重放 | `rolloutDate` vs `currentDate`（跨日）+ 500ms 突发窗（同日） | 日期更早或间隔 <500ms 视为重放 | `src/lib/rollout.js:4210`、`:4212`；判定实现 `:2588-2601` |
| 4c | Codex 事件 | `codexHashes` 游标 | 未读实现 | `src/lib/rollout.js:382` |
| 4d | Claude 消息 | `claudeMessageDedupKey(obj)` | **msgId 必需**；有 reqId 时合成 `msgId:reqId`，无 reqId 时**退化为纯 msgId**（去重照常生效） | `src/lib/rollout.js:4914-4918`、`:2854` |
| 5 | 双安装/WSL | cursor 命名空间（native/wsl）+ 路径 Set | 各自独立光标；迁移仅唯一命中才 seed | `src/lib/multi-install-parser.js:32`、`:84`、`:105` |
| 6 | Claude 跨根同 UUID | 文件名 UUID | 字节相同才折叠（mtime 胜）；歧义退出去重 | `src/lib/session-analytics.js:1828`、`:1901` |
| 6' | Codex 跨根 | 文件名 UUID | 归组后事件级并入 | `src/lib/session-analytics.js:1916` |
| — | CLI wrapped | `source\|model\|hour_start` | 与第 1 层同规则 | `src/lib/wrapped-aggregator.js:80` |
| — | 上传压缩 | `["hourly", source, model, hour_start]` / `["session", source, session_id]` | 保留最后（会话取严格更新） | `src/lib/upload-queue.js:53`、`:59`、`:88` |

---

## 5. repair / 迁移逻辑

### 5.1 触发与记账机制

所有一次性迁移都靠**游标里的 migration 记录**做幂等门禁。键集中在 `src/commands/sync.js:204-309`：

```js
// src/commands/sync.js:204-209（节选）
const CURSOR_UNKNOWN_MIGRATION_KEY = "cursorUnknownPurge_2026_04";
const ROLLOUT_CUMULATIVE_DELTA_MIGRATION_KEY = "rolloutCumulativeDeltaReparse_2026_05";
const CLAUDE_MEM_OBSERVER_REINCLUDE_KEY = "claudeMemObserverReinclude_2026_05_v3";
const GROK_APPEND_ONLY_REPAIR_MIGRATION_KEY = "grokAppendOnlyRepair_2026_05_v4";
const CODEBUDDY_LOG_JSONL_REPAIR_KEY = "codebuddyLogJsonlOverlapRepair_2026_08";
const WORKBUDDY_CONTEXT_USAGE_REPAIR_KEY = "workbuddyContextUsageRepair_2026_08";
```

```js
// src/commands/sync.js:248-257（节选）
const CLAUDE_GROUND_TRUTH_REPAIR_KEY = "claudeGroundTruthRepair_2026_05_v4";
// One-time full re-upload: the cloud ingest dropped `conversation_count` to 0
// from 2026-04-18 until the 2026-06-10 field-mapping fix (it read
// `b.conversations`; queue rows carry `conversation_count`). Historical cloud
// rows can only be repaired from each user's local queue.jsonl — resetting the
// upload offset replays the full queue and the ingest's whole-row upsert
// overwrites every historical bucket with the correct conversation counts
const CLOUD_CONVERSATIONS_BACKFILL_KEY = "cloudConversationsBackfill_2026_06";
```

```js
// src/commands/sync.js:268-309（节选）
const CODEX_RESCAN_DEDUP_REPAIR_KEY = "codexRescanDedupRepair_2026_06";
const CODEX_FORK_REPLAY_REPAIR_KEY = "codexForkReplayRepair_2026_07";
const CODEX_USAGE_LINEAGE_REPAIR_KEY = "codexUsageLineageRepair_2026_07_v2";
const LEGACY_BASE_URL_MIGRATION_NOTE = "reset_after_legacy_baseurl_migration_2026_07";
const DROID_DUP_SESSION_REPAIR_KEY = "droidDupSessionInflationRepair_2026_06";
const MIMO_PROVIDER_REPAIR_KEY = "mimoClaudeMislabelRepair_2026_06";
const DSH_LEGACY_SOURCE_MIGRATION_KEY = "deepseekHarnessSourceMigration_2026_08";
const ZCODE_NATIVE_USAGE_REPAIR_KEY = "zcodeNativeUsageRepair_2026_08";
const ZCODE_INCLUSIVE_TOKEN_REPAIR_KEY = "zcodeInclusiveTokenRepair_2026_09";
```

门禁的典型形态（读一次 migration 记录，已应用即返回 false）：

```js
// src/commands/sync.js:5010-5016（节选，grok 修复）
async function repairGrokQueueFromSessionSnapshots({ cursors, queuePath, queueStatePath } = {}) {
  if (!cursors || typeof cursors !== "object") return false;
  const grokState = (cursors.grok ||= {});
  const migrations = (grokState.migrations ||= {});
  if (hasAppliedGrokRepairMigration(migrations[GROK_APPEND_ONLY_REPAIR_MIGRATION_KEY])) {
    return false;
  }
```

```js
// src/commands/sync.js:3865-3868（节选，dsh 迁移）
async function migrateLegacyDeepseekHarnessSource({ cursors, queuePath, queueStatePath } = {}) {
  if (!cursors || typeof cursors !== "object") return false;
  const migrations = (cursors.migrations ||= {});
  if (migrations[DSH_LEGACY_SOURCE_MIGRATION_KEY]) return false;
```

"无事可做"也会记账（写 `status: "noop"`），避免每轮重复扫描：

```js
// src/commands/sync.js:3913-3920（节选）
if (legacyKeys.size === 0) {
  migrations[DSH_LEGACY_SOURCE_MIGRATION_KEY] = {
    status: "noop",
    appliedAt: new Date().toISOString(),
    reason: "no-legacy-deepseek-rows",
  };
  return false;
}
```

### 5.2 各 repair/迁移逐个说明

下表按源码顺序。**"对历史数据的影响"一列区分三类：只追加（安全）、原子重写（可能删历史）、仅重置上传偏移（本地不变、云被覆盖）**。

| # | 函数（定义处） | 触发键 | 调用点 | 做了什么 | 对历史数据的影响 |
| --- | --- | --- | --- | --- | --- |
| 1 | `migrateRolloutCumulativeDeltaBuckets` `src/commands/sync.js:5216` | `rolloutCumulativeDeltaReparse_2026_05` | `src/commands/sync.js:900` | 把累积型 delta 的桶重解析，并对受影响键**追加**撤回行 | 只追加（`src/commands/sync.js:5281` 为 `appendFile`） |
| 2 | `repairCodexRescanInflation` `src/commands/sync.js:5682` | `codexRescanDedupRepair_2026_06` | `src/commands/sync.js:901` | 从磁盘按事件去重重建 codex 桶，**原子丢弃**膨胀行，追加重建行，重置上传偏移 | 原子重写 + 上传偏移清零（`src/commands/sync.js:5895-5922`） |
| 3 | `repairCodexForkReplayInflation` `src/commands/sync.js:6038` | `codexForkReplayRepair_2026_07` | `src/commands/sync.js:909` | 以新键重跑 #2 的带守卫重建，用**当前 fork-aware 解析器**排除重放行 | 同 #2；有前置门禁（磁盘无 fork rollout 则直接标完成） |
| 4 | `repairCodexInterleavedUsageInflation` `src/commands/sync.js:6134` | `codexUsageLineageRepair_2026_07_v2` | `src/commands/sync.js:918`、`src/commands/sync.js:938` | 修复多 SessionState 交错导致的巨额 delta | 同 #2（v2 去掉了不可靠的前置门禁，改为逐条校验） |
| 5 | `repairDroidDuplicateSessionInflation` `src/commands/sync.js:6612` | `droidDupSessionInflationRepair_2026_06` | `src/commands/sync.js:927` | 重建 droid 桶，依赖 `repairKeys` 过滤（`src/commands/sync.js:6765`），先备份再原子写（`src/commands/sync.js:6743`、`src/commands/sync.js:6772`） | 原子重写，**有备份** |
| 6 | `repairMimoClaudeMislabel` + `dropMimoQueueRows` `src/commands/sync.js:4782`、`src/commands/sync.js:4744` | `mimoClaudeMislabelRepair_2026_06` | `src/commands/sync.js:928` | 0.57.0 把 mimocode 镜像的 Claude/claude-mem 历史误标成 `mimo`；本修复**清除**本地队列与游标里的 `source=mimo` 数据，等下一轮用 providerID 过滤的读取器重建 | **会删数据**（设计如此，靠重扫恢复）；注释见 `src/commands/sync.js:302-306` |
| 7 | `reincludeClaudeMemObserverFiles` + `relabelClaudeMemQueueRows` `src/commands/sync.js:7097`、`src/commands/sync.js:7147` | `claudeMemObserverReinclude_2026_05_v3` | `src/commands/sync.js:1148`、`src/commands/sync.js:7135` | 重新纳入曾被排除的 claude-mem observer 会话文件，并给队列行重打标签；原子重写（`src/commands/sync.js:7207-7209`） | 原子重写 |
| 8 | `repairClaudeQueueFromGroundTruth` `src/commands/sync.js:6844` | `claudeGroundTruthRepair_2026_05_v4` | `src/commands/sync.js:1183` | 用地面真值扫描重建 `source=claude` 行；原子重写主队列（`src/commands/sync.js:6874`、`src/commands/sync.js:6918-6920`）并修项目队列（`src/commands/sync.js:7015`） | **原子重写，历史上导致过大规模数据丢失**（见 §5.3） |
| 9 | `repairZcodeNativeUsageMigration` `src/commands/sync.js:4994` | `zcodeNativeUsageRepair_2026_08` | `src/commands/sync.js:1761` | 修复 zcode 原生用量模式解析 | 未细读，`【未能确认】`具体写盘方式 |
| 10 | `repairZcodeInclusiveTokenMigration` `src/commands/sync.js:5002` | `zcodeInclusiveTokenRepair_2026_09` | `src/commands/sync.js:1769` | 修复 zcode "含缓存"token 口径（对应 `dropZcodeQueueRows` `src/commands/sync.js:4884`） | 涉及删除 zcode 行（`dropZcodeQueueRows`），保留撤回行的开关见 `:4884`（`dropZcodeQueueRows(filePath, { retainRetractions = false } = {})` 的参数，`:4894` 只是 `let removed = 0;`） |
| 11 | `migrateLegacyDeepseekHarnessSource` `src/commands/sync.js:3865` | `deepseekHarnessSourceMigration_2026_08` | `src/commands/sync.js:1808` | `source="deepseek"` 重标为 `"dsh"`；为每个旧键**追加显式零行**；把最新规范行重追加到队尾；重置上传偏移 | 只追加行 + 偏移自 0 重放（`src/commands/sync.js:3946-3962`） |
| 12 | `migrateCursorUnknownBuckets` `src/commands/sync.js:5173` | `cursorUnknownPurge_2026_04` | `src/commands/sync.js:2151` | cursor 的 unknown 桶清理 | 只追加撤回行（`src/commands/sync.js:5207`） |
| 13 | `repairCodebuddyLogJsonlOverlap` `src/commands/sync.js:5315` | `codebuddyLogJsonlOverlapRepair_2026_08` | `src/commands/sync.js:2575` | 修复 log 与 jsonl 双源重叠；先写临时队列重建（`src/commands/sync.js:5401`），再 tmp+rename 落位（`src/commands/sync.js:5460-5462`） | 原子重写 |
| 14 | `repairWorkbuddyContextUsage` `src/commands/sync.js:5508` | `workbuddyContextUsageRepair_2026_08` | `src/commands/sync.js:2624` | 同型：临时队列重建 + 原子落位（`src/commands/sync.js:5571`、`src/commands/sync.js:5628-5630`） | 原子重写 |
| 15 | `repairGrokQueueFromSessionSnapshots` `src/commands/sync.js:5010` | `grokAppendOnlyRepair_2026_05_v4` | `src/commands/sync.js:2986` | 从 `cursors.grok.sessionSnapshots` 重建 grok 桶；清空游标里所有 `grok\|` 桶与 groupQueued；追加修复行（`src/commands/sync.js:5126`）；重置上传偏移 | 清空游标态 + 追加重建行 + 偏移清零；**原始队尾有备份**（`src/commands/sync.js:5124`） |
| 16 | 备份工具 | — | — | `backupExistingFile(filePath)` `src/commands/sync.js:4711` | 供 #5、#15 等调用 |

grok 修复的桶重建细节（含"先清空该源全部桶再重建"）：

```js
// src/commands/sync.js:4629-4657（节选）
for (const key of Object.keys(buckets)) {
  if (key.startsWith("grok|")) {
    delete buckets[key];
  }
}
for (const key of Object.keys(groupQueued)) {
  if (key.startsWith("grok|")) {
    delete groupQueued[key];
  }
}

for (const row of rows) {
  const totals = { ... };
  buckets[bucketKey("grok", row.model, row.hour_start)] = {
    totals,
    queuedKey: totalsKey(totals),
    source: "grok",
    hour_start: row.hour_start,
  };
}
```

注意其中的 token 拆解是**估算**（80/20 输入输出切分），不是真实测量：

```js
// src/commands/sync.js:4551-4563（节选）
function estimateGrokRepairTotals(totalTokens, conversationCount) {
  const total = Math.trunc(normalizeGrokRepairNumber(totalTokens));
  const inputTokens = Math.round(total * 0.8);
  const outputTokens = Math.max(0, total - inputTokens);
  return {
    input_tokens: inputTokens,
    ...
```

### 5.3 repair 会不会删数据：会，且有实证

这是本研究里最需要警惕的一条。源码自己的事故记录：

```js
// src/commands/sync.js:234-247
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

**数字自洽性验算（R1 复核交叉验证，与 S1/S1b 一致）**：源码记的事件规模是 **2.17B** claude tokens = **21.7 亿**（同一数字的两种写法）；分项 `-1.27B opus-4-7`、`-474M opus-4-6`、`-376M sonnet-4-5`、`-48M haiku`、`-6M sonnet-4-6` 求和 = 1270M + 474M + 376M + 48M + 6M = **2174M ≈ 2.174B** ≈ 源码的 2.17B，**分项与总数自洽**。回退事实亦确认：`0.26.4 HALTS back at v4`（`src/commands/sync.js:242`），key 仍为 `claudeGroundTruthRepair_2026_05_v4`（`:248`）。

**根因结构**值得单独记住：`queue.jsonl` 是**唯一**持有某些历史的副本。当上游 agent 自己清理了会话日志（Claude Code 的 cleanup），"从磁盘重建"就等于"以磁盘为空为由删除历史"；上传偏移清零后又把删掉的结果推到云端。

后续 codex 修复因此加了守卫——**任何曾贡献过的会话文件已不在磁盘则整轮跳过**（注释原文："GUARDED: skips if any codex session file that previously contributed is no longer on disk … clearing its bucket would lose that history (ref the v6 ground-truth-repair data-loss incident)"，`src/commands/sync.js:258-267`）。

### 5.4 修复的四种安全模式（同一仓库都用过）

1. **只追加撤回行**（最安全）：`retractStaleSourceQueueRows`（`src/lib/rollout.js:19990`）、`migrateLegacyDeepseekHarnessSource`（`src/commands/sync.js:3929`）、`migrateCursorUnknownBuckets`（`src/commands/sync.js:5207`）
2. **原子重写 + 前置备份**：`backupExistingFile`（`src/commands/sync.js:4711`）→ tmp+rename（`src/commands/sync.js:6772`）
3. **原子重写 + 不可复现守卫**：磁盘缺文件就整轮跳过（注释句首 `src/commands/sync.js:264`，整段注释 `:258-267`）
4. **清空重扫**（最危险，仅用于误标修复）：`dropMimoQueueRows`（`src/commands/sync.js:4744`）——只有当"磁盘一定还在"时才安全

---

## 6. 会话与项目归属

### 6.1 会话分析里的项目键：**目录 basename**

```js
// src/lib/session-analytics.js:398-408
// Throwaway per-agent checkouts (`.claude/worktrees/<id>`, `.codex/worktrees/…`)
// are not the project the user is working on, and the directory is usually gone
// by the time the session is browsed.
function isWorktreeRef(ref) {
  return /[\\/](worktrees|subagents)[\\/]/.test(String(ref || ""));
}

function projectKey(cwd, filePath) {
  const value = cwd || path.dirname(filePath || "");
  return path.basename(String(value).replace(/[\\/]+$/, "")) || "unknown";
}
```

**这是路径末段，不是路径本身。** 因此：

- 同名不同路径的目录会**碰撞**（`D:\a\demo` 与 `D:\b\demo` 都得到 `demo`）
- 没有任何"父目录并入子目录"或"越出根才算切换"的规则，因为键根本不含路径层级（**R1 复核确证**：`session-analytics.js:405-408` 只取 `path.basename(cwd)`，cwd 缺失时退回 `dirname(filePath)` 的 basename）
- 但项目**引用**（`project_ref`）保留在行里，用于展示与下钻；worktree/subagents 的 ref 被显式排除（见 §6.3）

### 6.2 用量主链的项目键：**远程仓库 `owner/repo`**

与 §6.1 完全不同的一套语义：用量侧项目键来自**仓库 URL**（`project_ref`），解析出 `owner/repo`（GitLab 嵌套 group 保留全路径）：

```js
// src/lib/rollout.js:4429-4442
function deriveProjectKeyFromRef(projectRef) {
  if (typeof projectRef !== "string") return null;
  try {
    const parsed = new URL(projectRef);
    const segments = parsed.pathname.split("/").filter(Boolean);
    if (segments.length < 2) return null;
    // GitHub paths are always owner/repo, but GitLab supports nested groups
    // (group/subgroup/.../repo). Preserve the full path so nested-group repos
    // don't collapse to the first two segments.
    return segments.join("/");
  } catch (_e) { return null; }
}
```

配套解析器给出状态与原因码：成功即 `status: "public_verified"`，失败为 `status: "blocked"` 且 `reason ∈ {"unparseable_ref", "missing_ref"}`（`src/lib/rollout.js:4444-4461`）。项目桶键无 model 维度（`src/lib/rollout.js:4228-4231`）；写入要求 `project_ref` 与 `project_key` **同时存在**（`src/lib/rollout.js:3511-3513`）——无法解析出仓库标识的用量**不进项目队列**。

**R1 复核确证（对 TokenScope 的两条关键对照结论）**：① 该函数**只吃 URL 字符串**（`new URL(projectRef)` → `pathname.split("/")`），**不含 cwd、不含任何层级归并逻辑**——「根 / 子目录 / 越出根」这套语义在这里完全不存在；② 残余待验点：`segments.join("/")` 不剥离 `.git` 后缀，若上游 `project_ref` 带 `.git` 会原样进入 `project_key`（本次未定位上游剥离点）。

### 6.3 cwd 语义：有没有父/子目录归并？

**结论：用量主链不看 cwd；会话分析用 basename 做标签；两者都没有层级归并。**

1. 分桶只吃 `timestamp`（`toUtcHalfHourStart` 无路径参数，`src/lib/rollout.js:4173`）；项目归属是另一个维度，键是仓库标识（§6.2）。
2. `projectKey(cwd, filePath)` 取 basename（`src/lib/session-analytics.js:407`），天然无层级概念。
3. worktree/subagents 目录被识别并**降级**，而不是并入父项目——"Only let a worktree fragment name the session when nothing better is available"（`src/lib/session-analytics.js:2728-2737`）。
4. 会话的项目由"**最忙碎片**"决定，且同样带 worktree 守卫（`src/lib/session-analytics.js:2725-2736`）。

对照 TokenScope 规格（`CLAUDE.md` 的"项目身份"要求"进入当前根的子目录仍归该根、越出才算切换、按分量边界前缀判定"）——TokenTracker **没有**实现这种层级归并，本地路径只用于展示与 worktree 排除，不能照搬其实现。

### 6.4 会话内的用量归属

- 会话行携带自身 token 汇总，另有 `usage_buckets` 逐桶明细，合并时拼接（`src/lib/session-analytics.js:2686-2688`）。
- **父→子用量是派生视图**，不改权威的 per-session `total_tokens`。注释原文："Combined fields are derived views only, so summing total_tokens across all rows still equals the global usage ledger"（`src/lib/session-analytics.js:2759-2764`，函数 `annotateThreadUsage`）。
- 父子链带**环检测**（`hasParentCycle`，`src/lib/session-analytics.js:2782-2791`）与**冲突标记**：同一身份出现两次时双方都打 `parent_link_conflict`，不静默择一（`src/lib/session-analytics.js:2773-2778`）。
- 该 pass 只对 `codex` / `zcode` / `opencode` 生效（`src/lib/session-analytics.js:2772`）。

### 6.5 会话级时间语义

```js
// src/lib/session-analytics.js:2343-2383（R2 复核已逐行读取全部三个函数体，签名与行号一致）
function sessionUsageDay(timestamp, { timeZone, offsetMinutes } = {})   // :2343-2354
function withinDayRange(row, from, to, context)                         // :2356-2362
function scopeSessionUsage(row, from, to, context)                      // :2364-2383
```

**【R2 补读（原 U 结案）】三个函数体的实际行为**：

- `sessionUsageDay`（`:2343-2354`）：`timeZone` 存在时用 `Intl.DateTimeFormat("en-CA")` 直接取 IANA 日串，formatter 抛错才落下一级；否则按 `offsetMinutes`（缺省 0）平移后取 **UTC 日**（`:2353`）——**注意与 `local-api.js` 的 `getZonedParts` 不同：这里的缺省是绝对 UTC，不会落到运行环境本机时区**（对照 §2.4）。
- `withinDayRange`（`:2356-2362`）：区间**相交**判定（`startDay`/`endDay` 各自与 `from`/`to` 比较；会话早于窗口开始但结束落在窗口内仍命中），与 `:2338-2342` 注释一致。
- `scopeSessionUsage`（`:2364-2383`）：按 `usage_buckets[].timestamp` 的日串过滤桶（闭区间，`:2366-2369`），重算 `tokens` 与 `model_usage`，并**保留生命周期值**——`:2381-2382` 把原 `row.total_tokens`/`row.cost_usd` 另存为 `lifetime_total_tokens`/`lifetime_cost_usd`，同时 `cost_usd: 0` 交给 `repriceSessionRecord` 重算。

行为由测试锁定（`test/session-cross-day-usage.test.js`）：

- 一个跨日的 Codex 会话，**每一天只计当天新增的用量**，而不是把生命周期总量摊到每一天；同时"效率"面板保留生命周期总量（测试注释即写明 "efficiency keeps lifetime tokens and edit counts in the same population"）：

```js
// test/session-cross-day-usage.test.js:26-34（节选）
  const first = listSessionsForBrowser([row], { from: "2026-10-09", to: "2026-10-09" });
  const second = listSessionsForBrowser([row], { from: "2026-10-10", to: "2026-10-10" });
  assert.equal(first.sessions[0].total_tokens, 120);
  assert.equal(second.sessions[0].total_tokens, 60);
  assert.ok(Math.abs(first.sessions[0].cost_usd + second.sessions[0].cost_usd - row.cost_usd) < 1e-12);
  assert.equal(summarizeSessions([row], { from: "2026-10-10", to: "2026-10-10" }).summary.total_tokens, 180, ...);
```

- 无 `from/to` 或带时区查询时回到生命周期总量（`:33`、`:34`）
- 重复的 `token_count` 快照不计两次（`:21` 行构造了两个完全相同的 count 行，期望总量仍 180）

### 6.6 `project.queue.jsonl` 的角色

| 方面 | 结论 | 证据 |
| --- | --- | --- |
| 写入者 | `enqueueTouchedProjectBuckets`（rollout 侧） | `src/lib/rollout.js:3489` |
| 行结构 | `project_ref`, `project_key`, `source`, `hour_start`, 九个用量列（**无 model**） | `src/lib/rollout.js:3515-3529` |
| 去重键 | `project_key\|source\|hour_start` | `src/lib/local-api.js:149` |
| 上传 | **不上传**云 | `src/commands/sync.js:3656` |
| 消费者 | 项目汇总/详情端点、本地成就 | `src/lib/local-api.js:145`、`:434` |
| 数据清洗 | 读取时剔除 claude-mem 伪项目 | `src/lib/local-api.js:136-138` |
| 维护工具 | `project-usage-purge.js` / `purgeProjectUsage` | `src/commands/sync.js:176` |

因为**没有 model 维度**，项目详情里的 `models` 字段实际装的是 **source**（注释明说是为了复用 TrendMonitor 的堆叠段槽位，`src/lib/local-api.js:2877-2883`）。

---

## 7. 查询与分页

### 7.1 服务外壳

- **只绑回环**：`LOCAL_BIND_HOST = "127.0.0.1"`（`src/commands/serve.js:34`），`http.createServer` + `server.listen(port, host)`（`src/commands/serve.js:157`、`:672`）。
- **写操作**需本地 token + 回环 Origin；**读端点不需要**（`isAuthorizedLocalMutation`，`src/lib/local-api.js:1629-1637`）。token 是进程内随机 24 字节（`src/lib/local-api.js:1016`），经 `/api/local-auth` 下发给同源页面（`src/lib/local-api.js:1681-1693`）。
- **鉴权路由前置拒绝**：跨站或非回环 Origin 在消耗请求体前返回 403 `untrusted_auth_origin`（`src/lib/local-api.js:1644-1648`）。
- **实例换绑**：请求头 `x-tokentracker-instance` 或已签发 token 与当前后端指纹不符 → 409 `instance_changed`，不静默转发（`src/lib/local-api.js:1658-1669`）。
- **会话浏览端点**：`Cache-Control: no-store` + `X-Content-Type-Options: nosniff`（`src/lib/local-api.js:2509-2510`），失败时不把 Node fs 错误里的绝对路径写进响应体（`src/lib/local-api.js:2516-2521`）。

```js
// src/lib/local-api.js:1629-1637
  function isAuthorizedLocalMutation(req) {
    const headerToken = req?.headers?.["x-tokentracker-local-auth"];
    const cookieToken = parseCookieHeader(req?.headers?.cookie).get("tokentracker_local_auth");
    const token = typeof headerToken === "string" && headerToken.trim()
      ? headerToken.trim()
      : cookieToken || "";
    if (!token || token !== localAuthToken) return false;
    return hasAllowedLoopbackOrigin(req?.headers || {});
  }
```

### 7.2 端点清单

下表为本次 `grep '"/functions/'` 的全量结果（28 条路由）。"参数"一列只写**实际读过代码确认**的；未确认者明确标注。

| 路由 | 行号 | 方法 | 参数（已确认） | 返回要点 |
| --- | --- | --- | --- | --- |
| `/functions/tokentracker-cloud-session` | `src/lib/local-api.js:2137` | GET | `【未能确认】` | 云端会话（未逐行读） |
| `/functions/tokentracker-local-sync` | `src/lib/local-api.js:2166` | POST | `【未能确认】`（含 120s 子进程预算，`src/commands/sync.js:297`） | 触发本地采集 |
| `/functions/tokentracker-wrapped` | `src/lib/local-api.js:2330` | GET | `【未能确认】`（用 `aggregateWrapped`） | 年终汇总 |
| `/functions/<任意 slug>` | `src/lib/local-api.js:2348` | GET | 透传 | 未命中本地路由时转发到云端函数 |
| `/functions/tokentracker-usage-summary` | `src/lib/local-api.js:2358` | GET | `from`, `to`, `tz`, `tz_offset_minutes`, `scope`, `include_account_level` | `from/to/days/scope/excluded_sources/totals{9 字段+total_cost_usd 字符串}/rolling{last_7d,last_30d}` |
| `/functions/tokentracker-usage-daily` | `src/lib/local-api.js:2425` | GET | 同上 | `{from,to,scope,excluded_sources,data[日数组]}` |
| `/functions/tokentracker-outcomes` | `src/lib/local-api.js:2441` | GET | `from`, `to` | `{available,by_model,by_tool,totals}`；无数据时 `available:false` |
| `/functions/tokentracker-session-insights` | `src/lib/local-api.js:2470` | GET | `from`, `to`, `refresh`, `format=csv`, `include_sessions`, `tz*` | 会话效率汇总；`format=csv` 时返回 CSV 附件 |
| `/functions/tokentracker-sessions` | `src/lib/local-api.js:2500` | GET | `from`, `to`, `refresh`, `limit`（上限 2000）, `tz*` | 逐会话行（含 session_id 与本地项目路径），`Cache-Control: no-store` |
| `/functions/tokentracker-context-health` | `src/lib/local-api.js:2526` | GET | 无 | `{generated_at,estimated_fixed_tokens,severity,breakdown,largest_items,provenance}` |
| `/functions/tokentracker-usage-heatmap` | `src/lib/local-api.js:2533` | GET | `weeks`（默认 52）, `tz*`, `scope` | `{from,to,week_starts_on,active_days,streak_days,weeks,total_cost_usd}` |
| `/functions/tokentracker-usage-model-breakdown` | `src/lib/local-api.js:2596` | GET | `from`, `to`, `tz*`, `scope` | `{sources[{source,source_scope,totals,models[{...,cost_source}]}],pricing}` |
| `/functions/tokentracker-usage-category-breakdown` | `src/lib/local-api.js:2668` | GET | `source`（`claude`/`codex`/`grok`）, `from`, `to`, `tz*` | 语义分类明细；codex 无数据时回退 `buildCodexCategoryFallbackFromQueue` |
| `/functions/tokentracker-project-usage-summary` | `src/lib/local-api.js:2731` | GET | `limit`（硬上限 10）, `from`, `to`, `tz*` | `{generated_at,entries[{project_key,project_ref,9 字段,sources[]}]}` |
| `/functions/tokentracker-project-usage-detail` | `src/lib/local-api.js:2828` | GET | `project_key`（必填）, `from`, `to`, `tz*` | `{totals,days_active,range_total_tokens,daily[],sources[]}`；缺 key 返回 400 `missing_project_key` |
| `/functions/tokentracker-user-status` | `src/lib/local-api.js:2920` | GET | 无 | 本地桩：`user_id:"local-user"`、`pro.active:true`、`account{available,cloud_sync_enabled,account_view}` |
| `/functions/tokentracker-cloud-sync-pref` | `src/lib/local-api.js:2940` | GET/POST | `【未能确认】` | 云同步开关派生 |
| `/functions/tokentracker-proxy-config` | `src/lib/local-api.js:2982` | GET/POST | `【未能确认】` | 代理配置 |
| `/functions/tokentracker-proxy-test` | `src/lib/local-api.js:3074` | POST | `【未能确认】` | 代理连通性测试 |
| `/functions/tokentracker-telemetry-pref` | `src/lib/local-api.js:3121` | GET/POST | `【未能确认】` | 遥测开关 |
| `/functions/tokentracker-machine-id` | `src/lib/local-api.js:3137` | GET | 无 | `{...,deviceName}` |
| `/functions/tokentracker-usage-hourly` | `src/lib/local-api.js:3146` | GET | `day`（默认 `new Date().toISOString().slice(0,10)`，即 **UTC 今天**；注意过滤谓词按**当地日键**，缺 tz 时两者基准不同，见 §2.4）, `tz*`, `scope` | `{day,scope,excluded_sources,data}`（当日**逐小时**：键空间为当地小时 24 个整点，两个半小时桶在此合并；响应**只含有数据的桶**，`src/lib/local-api.js:600-609`、`:639`） |
| `/functions/tokentracker-usage-monthly` | `src/lib/local-api.js:3156` | GET | `from`, `to`, `tz*`, `scope` | `{from,to,scope,excluded_sources,data[月数组]}` |
| `/functions/tokentracker-pets` | `src/lib/local-api.js:3192` | GET/POST | POST 需本地鉴权；`scope=codex` 走可导入列表 | 宠物资源管理 |
| `/functions/tokentracker-subscription-manager` | `src/lib/local-api.js:3240` | `【未能确认】` | `【未能确认】` | 订阅管理 |
| `/functions/tokentracker-skills` | `src/lib/local-api.js:3294` | `【未能确认】` | `【未能确认】` | 技能管理 |
| `/functions/tokentracker-achievements` | `src/lib/local-api.js:3470` | GET | `tz*`（夜间徽章按本地小时） | 本地徽章（阈值 `src/lib/local-api.js:419-423`） |
| `/functions/tokentracker-usage-limits` | `src/lib/local-api.js:3482` | `【未能确认】` | `【未能确认】` | 各 provider 限额 |

非 `/functions/` 的本地路由另有两个：`/api/runtime-config.js`（注入 `window.__TOKENTRACKER_RUNTIME_CONFIG__`，`src/lib/local-api.js:1670-1678`）与 `/api/local-auth`（下发本地 token，`src/lib/local-api.js:1681`）。

### 7.3 分页

**用量端点全部不做服务端分页**（返回整段数组，由前端分页）；只有两支例外：

| 端点 | 分页 | 证据 |
| --- | --- | --- |
| `sessions` | `limit` 上限 2000；`0` 或非法值 = 不限 | `src/lib/local-api.js:2504-2505` |
| `project-usage-summary` | `limit` 硬上限 `PROJECT_USAGE_MAX_ENTRIES = 10`，默认值即上限 | `src/lib/local-api.js:2737-2740`、`:95` |
| `usage-daily` / `-summary` / `-model-breakdown` / `-monthly` / `project-usage-detail` | 无 | `src/lib/local-api.js:2430`、`:2414`、`:2656`、`:3187`、`:2900` |

```js
// src/lib/local-api.js:2504-2505
      const limitParam = parseInt(url.searchParams.get("limit") || "0", 10);
      const limit = Number.isFinite(limitParam) && limitParam > 0 ? Math.min(limitParam, 2000) : 0;
```

明细页真正分页在前端：`dashboard/src/lib/details.ts` 的 `paginateRows(rows, pageIndex, pageSize)`，由 `test/details-pagination.test.js:5-24` 锁定（25 行数据、页大小 12 → 12/12/1）。

### 7.4 排序

| 位置 | 排序键 | 证据 |
| --- | --- | --- |
| 日聚合 | `day` 字典序升序 | `src/lib/local-api.js:299` |
| 月聚合 | `month` 字典序升序 | `src/lib/local-api.js:3187` |
| 模型分解 | `total_tokens` 降序 | `src/lib/local-api.js:2650` |
| 项目汇总 | `billable_total_tokens` 降序 | `src/lib/local-api.js:2785` |
| 项目详情 sources | `total_tokens` 降序 | `src/lib/local-api.js:2913` |
| 账户会话聚合 | `hour_start` → `source` → `model` 三级字典序 | `src/lib/account-usage-dedup.js:176-181` |
| 本地徽章 | 按 `hour_start` 升序重放，取首次越过阈值的桶 | `src/lib/local-api.js:435-439` |
| 上下文体检最大项 | `estimated_tokens` 降序取前 10 | `src/lib/context-health.js:111` |

### 7.5 时间范围过滤

**统一是"日键闭区间"**，日键由 §2.4 的时区上下文决定：

```js
// src/lib/local-api.js:2364（usage-summary）
      const daily = allDaily.filter((d) => d.day >= from && d.day <= to);
```

```js
// src/lib/local-api.js:130-134（项目端点谓词）
  const dayInRange = (day) => {
    if (!hasRange) return true;
    if (!day) return false;
    return (!from || day >= from) && (!to || day <= to);
  };
```

要点：

- `from`/`to` 缺省时是空串，比较退化为恒真（即"全部时间"），项目端点还额外用 `hasRange` 短路。
- 区间**两端都含**（`<=`）。
- 过滤粒度是**日**，不是桶；因此同一天内无法用 `from/to` 做小时级筛选——小时级只能走 `usage-hourly?day=`。
- 会话端点的时间过滤走另一套（`scopeSessionUsage` / `withinDayRange`，`src/lib/session-analytics.js:2356`、`:2364`），行为见 §6.5。

### 7.6 缓存与失效

| 缓存 | 键 | 失效条件 | 位置 |
| --- | --- | --- | --- |
| 队列解析结果 | 队列路径（`pending` 单飞） | 文件签名 `dev:ino:size:mtimeNs:ctimeNs` 变化；ENOENT 清缓存 | `src/lib/async-queue-reader.js:17`、`:34`、`:98` |
| 日聚合 | `WeakMap<行数组, Map<时区\|偏移\|价格修订>>` | 行数组对象变化（即队列变了）或价格修订变化 | `src/lib/local-api.js:104`、`:229`、`:116` |
| IANA 时区 formatter | 时区 id | LRU 16 条（含失败结果也缓存） | `src/lib/local-api.js:105`、`:96`、`:545-552` |
| 上下文体检 | `home\0cwd\0两个环境变量` | TTL 5 分钟，LRU 8 条 | `src/lib/context-health.js:64-67`、`:119-120` |
| 会话分析 | 文件统计键 + 侧车文件 | TTL 5 分钟 / `force` 参数 | `src/lib/session-analytics.js:2037`、`:2046`、`:2100`、`:2140`、`:2315` |
| 账户视图失败退避 | 进程内 | 2 分钟冷却 | `src/lib/local-api.js:27` |
| 头像代理 | URL | TTL 1 小时，LRU 64，单图 ≤512 KiB | `src/lib/local-api.js:45-48` |

"共享一次解析"由测试锁定：`test/local-api-queue-cache.test.js:52`（"local usage endpoints reuse one parsed queue until the file changes"）、`:103`（缓存下 IANA 时区日边界仍正确）、`:129`（价格修订失效缓存但不重读队列）、`:170`（大聚合期间仍能响应其他 HTTP 请求）、`:205`（聚合过程中价格刷新不允许发布混合修订）。

---

## 8. 派生指标

### 8.1 上下文体检（`context-health.js`）

**数据来源**：本机配置文件，**不是**队列或用量——指令文件（`~/.claude/CLAUDE.md`、`~/.codex/AGENTS.md`、`$cwd/CLAUDE.md`、`$cwd/AGENTS.md`，`src/lib/context-health.js:69-78`）、技能文件（四个技能根下的 `SKILL.md`，遍历上限 500 文件，`:79-90`、`:25`）、MCP 服务器计数（`:43-62`）。

**估算口径**：CJK 每字符 1 token、其余每 4 字符 1 token；MCP 部分**不是实测**，而是 `服务器数 × 每服务器工具数(默认 5) × 每工具 schema tokens(默认 400)`；分级阈值 50k/20k。

```js
// src/lib/context-health.js:11-16 与 :94-98（合并节选）
function estimateTokens(text) {
  const value = String(text || "");
  let cjk = 0;
  for (const char of value) if (/\p{Script=Han}|\p{Script=Hiragana}|\p{Script=Katakana}/u.test(char)) cjk += 1;
  return Math.ceil(cjk + (value.length - cjk) / 4);
}
  const perToolSchemaTokens = Math.max(0, Number(env.TOKENTRACKER_MCP_TOOL_SCHEMA_TOKENS || 400));
  const assumedToolsPerServer = Math.max(1, Number(env.TOKENTRACKER_MCP_TOOLS_PER_SERVER || 5));
  const mcpSchemaTokens = mcpServers * perToolSchemaTokens * assumedToolsPerServer;
  const severity = total >= 50_000 ? "high" : total >= 20_000 ? "medium" : "low";
```

**与用量统计的关系：不相关。** 它不读队列、不计入任何 token 总量，只产出"固定开销估计"，并自标 `provenance.confidence = "inferred"`、`content_retained: false`（`src/lib/context-health.js:112-117`；文件头亦声明不返回文件内容，`:3-4`）。

### 8.2 会话性能（`session-performance.js`）

**数据来源**：会话日志的**时间戳与 usage 记录**（不是队列行）。口径常量：`MIN_OUTPUT = 200`、`MIN_DURATION_MS = 1000`、`MAX_DURATION_MS = 1 小时`、`SAME_FLUSH_MS = 100`（`src/lib/session-performance.js:5-12`）；派生量 `tokens_per_second = output / (duration/1000)`、`first_response_ms = total / samples`（`:19-26`）。

```js
// src/lib/session-performance.js:56-59
  function record(model, outputTokens, duration) {
    if (outputTokens < MIN_OUTPUT || duration < MIN_DURATION_MS || duration > MAX_DURATION_MS) return;
    add(model, { estimated_output_tokens: outputTokens, estimated_duration_ms: duration, estimated_request_count: 1 });
  }
```

**与用量统计的关系（关键约束）**：分子直接取会话扫描中"已按 canonical 规则只计过一次"的 output + reasoning，性能模块**不重复计 token**——注释原文 "Later content blocks extend only the timing, never the numerator"（`src/lib/session-performance.js:79-86`）。

测试锁定的边界（`test/session-performance.test.js`）：Claude 请求速度按父边界与完整内容块计一次（`:37`）；缺开始时间/截断/错误/窗口过短都不估算（`:52`）；Codex 请求速度**排除工具执行时间**（`:80`）；重复快照不结束在途请求也不翻倍 token（`:104`）；原生与 WSL 镜像每个 canonical 请求只留一个样本（`:220`）。

### 8.3 其他派生视图

| 视图 | 来源 | 关键口径 | 证据 |
| --- | --- | --- | --- |
| wrapped | 队列行（自行按 §4.1 去重） | 取 `billable_total_tokens ?? total_tokens`；日= `hour_start.slice(0,10)`（UTC 日）；小时取 ISO 的 `T(\d{2})` | `src/lib/wrapped-aggregator.js:29-53`、`:77-85` |
| 本地徽章 | 项目队列 + 主队列 | `night_owl` 按请求时区的小时（`parts.hour < 6`），阈值 `[5,20,60,150]` | `src/lib/local-api.js:419-423`、`:478-486` |
| 热力图 | 日聚合 / 云端 compact | 见 §2.5 | — |
| outcomes（质量/美元） | 侧车 `outcomes.jsonl` + 队列行 | 读时 join，不存在则 `available:false` | `src/lib/local-api.js:2435-2467` |
| 会话浏览器 | 会话分析 + 逐日切片 | 见 §6.5 | — |

值得注意：`night_owl` 徽章用的是 `getZonedParts` 的小时——**请求 tz 优先，缺省回退运行环境本机时区**（`src/lib/local-api.js:482`、`:585-592`）；而热力图网格的数据日键走 `rowDayKey`（缺 tz 时 UTC 日），窗口终点/`todayStr` 又走 `getZonedParts`——同一个面板里三处时间口径并存，详见 §2.4。

---

## 9. 测试佐证

挑 12 个与聚合/去重/桶/时间直接相关的测试，逐个说明它锁定的不变量。

### 9.1 `test/wrapped-aggregator-dedup.test.js`

CLI wrapped 与 dashboard 共用同一 last-row-wins 规则：`:23` 按 `(source, model, hour_start)` 保留最新；`:34` 不同桶之间不互相去重（键必须是三元组）。

### 9.2 `test/local-api-project-usage.test.js`

项目队列读取契约（9 用例）：`:74` 按时区做 from/to 过滤；`:96` 剔除 claude-mem 伪项目；`:115` **保留行的 billable 值**而非用 total 覆盖；`:128` 按 `(project, source, hour)` 去重；`:143` 按来源分解；`:161` 无项目行时退化为按来源聚合（**只**在空集时）；`:190`/`:218`/`:242` 详情端点：日序列+来源分解、缺 `project_key` 报错、伪项目不计入分母。

### 9.3 `test/account-usage-dedup.test.js`

账户级两类去重的**全部修正类**（R1–R6 + LWW）：`:129` 桶迁移（10:00→10:30 只剩 10:30，总量 100）；`:152` 模型迁移（只剩 B）；`:173` **向下修正** 100→60 取 60，绝不取 MAX；`:192` 新快照里缺席的会话**保留**；`:212` 空快照什么都不断言；`:233` 重放幂等；`:267` 严格更新才替换、等值保留首条；`:319` 畸形观察什么都不断言；`:398` cursor 类行仍走整行 MAX；`:483` 本地读取器**不得**把会话状态控制记录当用量行曝露。

### 9.4 `test/async-queue-reader.test.js`

读取器的并发与失效语义：`:16` 冷启动并发读共享解析且不阻塞无关 HTTP 请求；`:48` **替换、截断、同尺寸重写、删除**都必须丢弃陈旧桶；`:66` 半行尾部、UTF-8 分块边界、畸形行都不能破坏已完成行。

### 9.5 `test/upload-queue.test.js`

压缩与上传预算（16 用例）：`:26` 保留已上传的 UTF-8 字节且 LWW 的向下修正/零值快照不被压掉；`:53` 替换前必须先关闭输入句柄（Windows 语义）；`:77` 未知/非法/水印/CRLF/不完整尾行**逐字节保留**；`:95` 低于阈值或偏移落在行中时队列不动；`:103` 非法 UTF-8→日期→会话身份的行不能压掉；`:113` 替换失败后原队列与偏移仍可重试；`:291` 会话真值在压缩与批次边界间存活。

### 9.6 `test/heatmap-compact.test.js`

云端 compact 载荷展开的兼容性（15 用例）：`:29` 稀疏日展开为整周网格；`:39` 标量（`active_days`/`streak_days`）原样透传**不重算**；`:55` 无数据的日 → 0 与 `null` models；`:67` 有行但 models 空 → `{}` 而非 `null`；`:78` level 阈值恰好卡在 0.25/0.5/0.75；`:101` `max_value` 缺失或 0 时所有正日都是 level 1；`:128` 重复日 last-row-wins；`:152`/`:164` 旧格式或畸形载荷原样退回；`:190`/`:199`/`:208` 字典缺失、尾随索引、同日内重复模型索引（last-pair-wins）。

### 9.7 `test/opencode-fork-dedup.test.js`

fork 去重的**顺序无关性与误删防护**（14 用例，详见 §4.4a）。最关键的两条是反向保护：`:353` 共享 token 数的真实不同调用不能删；`:388` 同会话内重复永不删。

### 9.8 `test/session-cross-day-usage.test.js`

跨日聚合口径（2 用例，详见 §6.5）：`:8` 跨日会话每天只计当天增量（120 + 60 = 180）、重复 `token_count` 不翻倍、效率面板保留生命周期值；`:40` 优先/长上下文档位在按日切片后仍保留（`priority_long_context_input_tokens = 50000`），且 `Asia/Kathmandu`（UTC+5:45）的四分之一小时偏移下日归属正确。

### 9.9 `test/session-analytics-claude-cross-root-dedup.test.js`

跨根归组的**保守性**（12 用例，详见 §4.6 表）。最有分量的两条是 `:120`（单根同 UUID 兄弟 → 关闭去重）与 `:165`（同 UUID 内容不同 → 都保留）：都选择"宁多不删"。

### 9.10 `test/local-api-queue-cache.test.js`

缓存**正确性**而非性能（5 用例，详见 §7.6）。最值得记的是 `:205`：聚合过程中价格修订变化时**不允许**发布混合修订的结果。

### 9.11 `test/claude-wsl-dual-install.test.js` 与 `test/codex-wsl-shadow.test.js`

双安装/WSL 的并入语义：`test/claude-wsl-dual-install.test.js:65` 两套安装合并解析、游标按路径分键；`:93` 完全同步的副本按**文件哈希**整体跳过；`:138` 内容发散的副本都要解析，重叠由消息哈希兜住；`:170` 单环境下去重层保持"休眠"（不误伤）；`:219` 地面真值桶跨根合并并去重；`test/codex-wsl-shadow.test.js:181` 矩阵式用例。

### 9.12 其他直接相关的测试（未逐条展开）

| 测试 | 锁定对象 |
| --- | --- |
| `test/details-pagination.test.js:5` | 前端明细分页 `paginateRows(rows, page, size)` 的切页边界（25 行 / 页 12 → 12/12/1） |
| `test/details-sort.test.js`、`test/details-trim.test.js` | 明细排序与裁剪（同目录姊妹测试，未逐行读） |
| `test/model-breakdown.test.js`（25 用例） | 模型分解的合并与成本口径：`:666` IDE+CLI 同模型合并为一行、`:764` 每模型成本等于 hero 的逐行成本（含 Fast 溢价）、`:835` 服务端自报成本在聚合后存活 |
| `test/session-performance.test.js:220` | 原生 + WSL 镜像组每个 canonical 请求只留一个性能样本 |
| `test/context-health.test.js:10` | 体检只返回计数与估计，**不含文件内容** |
| `test/sync-codex-rescan-repair.test.js`、`test/sync-mimo-repair.test.js`、`test/sync-codebuddy-repair.test.js`、`test/sync-workbuddy-repair.test.js`、`test/sync-droid-dup-session-repair.test.js`、`test/sync-zcode-native-migration.test.js`、`test/sync-cursor-unknown-migration.test.js`、`test/deepseek-harness-migration.test.js`、`test/sync-claude-mem-observer-reinclude.test.js`、`test/sync-cloud-conversations-backfill.test.js` | §5 表里各 repair/迁移的专用回归测试（逐个存在，本次未逐行读其断言） |
| `test/session-analytics-wsl-roots.test.js`、`test/session-analytics-scan-roots.test.js`、`test/session-analytics-codex-subagents.test.js` | 会话发现的根集合、子代理归属 |
| `test/billable-total-tokens-migration.test.js` | billable 字段迁移（文件内**无** `test(` 顶层用例，推测为辅助/被引用模块 → `【未能确认】` 用途） |

---

## 存疑与待复核点

以下条目在本次研究中**未能确认**，或存在需要复核的风险点。按风险排序。每条后面补上「判定条件」——即闭合它需要读的文件或需要观察的行为（R1 复核确认这些点在原位置确实无法判定）。

### A. 未确认的实现细节

1. `src/lib/fs.js` 的锁实现：`openLock` / `inspectLock` / `updateJsonLocked` 的内部机制（是否 `O_EXCL`、是否写 pid、超时与陈旧锁回收策略）未读。仅确认 sync 用 `sync.lock` 且压缩要求调用方持锁。**判定条件**：读 `src/lib/fs.js` 全文，核对 `openLock` 的创建标志位与 `inspectLock` 的陈旧判定（pid/时间戳），并查 `test/` 中 lock 相关用例。
2. `src/lib/rollout.js` 中 `codexHashes` 事件去重的**哈希输入**（`src/lib/rollout.js:382` 只是游标键名）。**判定条件**：定位写入 `codexHashes` 的构造点，确认哈希覆盖的字段集合（是整行、或用量三元组、或事件时间戳组合），并确认跨文件运行时是否会碰撞。
3. `copilot` 的桶粒度与 `copilotOtelAggregateKey`（`src/lib/rollout.js:17462`）的键构造——可能不是半小时。**判定条件**：读 `copilotOtelAggregateKey` 函数体与其调用点，确认第二个参数来自 `toUtcHalfHourStart` 还是别的桶函数。
4. ~~`src/lib/claude-categorizer.js:1175` 的 `toUtcHalfHourStart` 副本是否与 `src/lib/rollout.js:4173` 版本逐字节等价~~ —— **R1 复核已解除**：两版逐行等价（仅中间变量写法不同），详见 §2.3。
5. 会话分析里跨根"事件级并入"的具体并入键（`test/session-analytics-claude-cross-root-dedup.test.js:174`、`:288` 只锁行为，未定位实现）。**判定条件**：沿会话扫描路径（`scanClaudeSession` 的 `seenMessageHashes` 分支，`src/lib/rollout.js:2854`）继续向下读跨根汇合点，确认并入键是消息哈希、`(session_hash, message_key)` 还是事件指纹。
6. ~~`sessionUsageDay` / `withinDayRange` / `scopeSessionUsage` 三个函数的函数体未逐行读，行为仅由测试反推~~ —— **R2 复核已读全并结案**：`src/lib/session-analytics.js:2343-2354`（IANA → 偏移 → UTC）、`:2356-2362`（区间相交）、`:2364-2383`（按 `usage_buckets` 日键过滤 + 重算 + 保留 `lifetime_*`），行为与 §9.8 测试一致，详见 §6.5。残留注意点：`sessionUsageDay` 的缺省是 **UTC 日**（无 tz 时不落本机时区），与 `local-api.js` 的 `getZonedParts` 缺省不同（§2.4）。
7. `repairZcodeNativeUsageMigration` / `repairZcodeInclusiveTokenMigration` 的写盘方式（是否原子重写、是否删行）。**判定条件**：`src/commands/sync.js:4994` / `:5002` 均委托 `repairZcodeUsageMigration`，需读该函数体确认是「追加零行」还是「tmp+rename 原子重写」，以及是否调用 `dropZcodeQueueRows`（`:4884`）。
8. 端点 `cloud-session`、`local-sync`、`cloud-sync-pref`、`proxy-config`、`proxy-test`、`telemetry-pref`、`subscription-manager`、`skills`、`usage-limits` 的参数与返回字段未逐行确认（表中已标）。**判定条件**：逐个端点读 `src/lib/local-api.js` 对应 `if (p === ...)` 分支的 `url.searchParams.get(...)` 集合与响应 `JSON.stringify(...)` 载荷。部分已解除：`user-status` 的 `account{available, cloud_sync_enabled, account_view}`（`:2927-2931`）、`achievements` 的 `tz*`（`:3470-3471`）、`usage-limits` 存在性（`:3482`）。
9. `test/billable-total-tokens-migration.test.js` 内部无顶层 `test(` 用例，其真实用途未确认（R1 复核复核了「0 用例」这一观察，实测 9 行、`test(` 出现 0 次）。**判定条件**：读该文件全文（9 行）并 `rg` 其文件名的引用方，确认它是被 import 的辅助模块还是失效残留。
10. `src/lib/source-metadata.js` 的 scope 过滤规则（`filterRowsByUsageScope`、`ACCOUNT_LEVEL_SOURCES`）未展开——它决定了"哪些来源进入默认总量"。**判定条件**：读 `src/lib/source-metadata.js` 全文，列出 `ACCOUNT_LEVEL_SOURCES` 成员与 `normalizeUsageScope` 的取值域，并对照 `listExcludedSources`（`src/lib/local-api.js:410`）确认界面提示口径。

### B. 需要复核的风险点

1. **`projectBucketKey` 与 `parseBucketKey` 不通用**：前者布局是 `projectKey|source|hourStart`（无 model），后者按 `source|model|hourStart` 解析。当前调用点上没有误用（`parseBucketKey` 只用于主队列键），但这是"两套键共用一个分隔符、解析器不匹配"的结构性隐患（`src/lib/rollout.js:4228` vs `src/lib/rollout.js:4238`）。
2. **`project_key` 无 model 维度**：项目队列丢失了模型信息，项目详情里的 `models` 字段实际装的是 **source**（`src/lib/local-api.js:2877`）。若下游按"按模型"理解该字段会出错。
3. **`conversation_count` 跨来源不可比**：至少三种口径（Craft 按会话 / Claude 按主会话 **user 文本消息**（`src/lib/rollout.js:2807-2837`，**不是** assistant 消息）/ 按请求 `delta.requests`），源码自己写明了"不可比"这一点（`src/lib/rollout.js:17233-17235`）；且该处注释对 Claude 口径的描述与解析侧代码相反（见 §4.8）。任何跨来源"会话数"汇总都只是粗略值。
4. **`repair` 的删除风险仍在**：`dropMimoQueueRows`（`src/commands/sync.js:4744`）与 `repairClaudeQueueFromGroundTruth`（`src/commands/sync.js:6844`）都属于"原子丢弃 + 重扫"，其安全性依赖"磁盘日志仍在"。v6 事故证明这个前提会破（`src/commands/sync.js:234-247`）。队列是部分历史的**唯一**副本。
5. **本地热力图的 `streak_days` 恒为 0**（`src/lib/local-api.js:2588`）、`week_starts_on` 恒为 `"sun"`（`src/lib/local-api.js:2586`），而云端 compact 载荷会带真实值（`src/lib/heatmap-compact.js:108-109` 透传）。同一 UI 在本地/云两条路径下显示可能不一致。
6. **同一面板里两套（严格说是三处）时间口径**：热力图网格的**数据日键**来自 `rowDayKey`（无 `tz` 时 UTC 日），而其**窗口终点/`todayStr`** 与 `night_owl` 徽章小时都走 `getZonedParts`（无 `tz` 时**运行环境本机时区**）——缺省时两者差一级，见 §2.4（`src/lib/local-api.js:2540`、`:482`、`:585-592`）。
7. **上传偏移清零会广播本地修复结果到云端**（多处 repair 都重置 offset）。这意味着"本地算错 → 修好 → 云被覆盖"是设计路径；反过来，若本地修复本身错误，云端也会被污染。`CLOUD_CONVERSATIONS_BACKFILL_KEY` 的注释明说云端历史**只能**靠本地队列重放修复（`src/commands/sync.js:249-256`）。
8. **半小时桶是 UTC 的，但跨日归属在客户端**：如果客户端不传 `tz`/`tz_offset_minutes`，日归属静默退化为 UTC 日（`src/lib/local-api.js:224`），不会报错（仅指 `rowDayKey` 一族；`getZonedParts` 系另回退本机时区，见 §2.4）。
9. **`normalizeQueueRow` 是读时修补**：历史 codex/cursor 行的修正在每次查询时重算（`src/lib/local-api.js:170-199`）。若将来修改该规则，历史数字会**追溯变化**，且与云端的已上传值可能不一致。
10. **`aggregateByDay` 的缓存以行数组对象身份为键**（WeakMap，`src/lib/local-api.js:104`）：一旦某个调用方对行做了浅拷贝，缓存立刻失效并重算。这是性能敏感点，也是 `test/local-api-queue-cache.test.js:52` 存在的原因。

### C. 对 TokenScope 的移植提示（仅事实性差异，不含方案建议）

- TokenTracker 的**项目身份 = 远程仓库 `owner/repo`**（`src/lib/rollout.js:4429`）；TokenScope 规格要求的是**本地规范化绝对路径 + 层级归并**。二者不可互相替代。
- TokenTracker 的**队列是唯一事实源 + append-only 快照日志**；TokenScope 是 **SQLite `history.db` + 每来源事件**。因此 TokenTracker 的"零行撤回 / last-row-wins / 原子重写 repair"三件套在 TokenScope 里对应的语义不同。
- TokenTracker 的**半小时桶由 UTC 计算**，日归属交给请求参数；TokenScope 规格是**存储 UTC、聚合时区由 settings 决定、查询入口冻结一次"今天"**。TokenTracker 没有"冻结 as_of"概念——每次请求都重新解析 `new Date()`（`src/lib/local-api.js:2382`）。
- TokenTracker 的**去重分层多达七层**，其中"账户级去重"和"双安装命名空间光标"是 TokenScope 目前没有对应物的两个概念。

---

## 修正记录（R1 复核后）

依据 `REVIEW-R1-E-aggregation.md`（E=8 / I=6 / U=7）逐条落实：**E 组 8 条、I 组 6 条全部采纳，无反驳**；计数类数字全部由本轮 FIX-E **独立重算**（UTF-8 读取 + 正则计数）；样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c` 全程只读，未修改。行号一律为样本仓 `src/...` 行号。

### E 组：事实性错误（8 条）

| 编号 | 原表述 | 修正后 | 依据 |
| --- | --- | --- | --- |
| E1 | 「`conversation_count` **按 assistant 消息**（每次消息 +1）」 | 「按**主会话 user 文本消息**计（排除 `tool_result` 与子代理）」，并明确写出**源码注释与实现不一致**：解析侧注释 `:2807-2809` 与代码一致，Craft 侧注释 `:17233-17234` 自称 "count one-per-assistant-message" 与代码冲突，以代码为准；「跨来源不可比」结论保留（§4.8、存疑 B.3 两处同改） | `rollout.js:2807-2837`（`:2833` 落在 `isMainSession && type==="user"` → `hasText` 内层分支）、`rollout.js:17233-17234` |
| E2 | 表 4d「**需 msgId 与 reqId 同时存在**才启用」 | 「**msgId 必需**；有 reqId 时合成 `msgId:reqId`，无 reqId 时**退化为纯 msgId**（去重照常生效）」；§4.4(d) 增补：`if (msgId && reqId)` 是 v3 ground-truth 扫描的缺陷（`sync.js:227-233`），与解析侧函数语义不同 | `rollout.js:4914-4918`；`sync.js:227-233` |
| E3 | 「`toUtcHalfHourStart(` 全仓共 **70 处调用**：65 处在 `rollout.js`」 | 改为分口径表：`rollout.js` **70 次出现 / 66 行** = 定义 1（`:4173`）+ 调用 **69 次 / 65 行**（`:20214` 一行 3 次，`:20265`、`:20418` 各 2 次）；`sync.js` 1（`:6663`）；`claude-categorizer.js` 3（定义 `:1175` + 调用 `:1250`/`:1279`）；**全 `src` 74 次出现 = 72 次调用 + 2 处定义**；`test/` 另有 10 次 | 本轮逐文件重算（正则 `toUtcHalfHourStart\(`） |
| E4 | usage-hourly 为「某日**逐半小时**」（§3.4 表、§7.2 表两处） | 改为「当日**逐小时**（当地小时，24 个整点，两个半小时桶按当地小时合并；响应无 `:30` 键）」 | `local-api.js:600-609`（`hourKey = \`${dayKey}T${HH}:00:00\``） |
| E5 | 「`src/lib/upload-queue.js:20031` 同型」 | 改为 `src/lib/rollout.js:20031`（`upload-queue.js` 全文实测 **134 行**，不可能有 20031 行） | `rollout.js:20031`；`upload-queue.js` 行数重算 |
| E6 | `heatmap-compact.test.js`「**16** 用例」 | 改为 **15**（重算：行首 `test(` = 15，任意位置同为 15，无 `test.each`；文件 217 行） | 本轮重算 |
| E7 | `model-breakdown.test.js`「**30+** 用例」 | 改为 **25**（重算：行首 `test(` = 25，任意位置 27，无 `test.each`；文件 888 行） | 本轮重算 |
| E8 | `dropZcodeQueueRows` 保留撤回行的开关见 `:4894` | 改为 `:4884`（函数签名 `dropZcodeQueueRows(filePath, { retainRetractions = false } = {})`）；`:4894` 实际是 `let removed = 0;` | `sync.js:4884`、`:4894` |

### I 组：不完整（6 条）

| 编号 | 原表述 | 修正后 | 依据 |
| --- | --- | --- | --- |
| I1 | §0 第 5 条「dashboard **所有**用量端点共享同一条聚合链 → `aggregateByDay` → reduce」 | 改为「共享前两跳（`scopedQueueRows` `:403-412`、`rowDayKey` `:213-225`、`readQueueData` `:202-211`），之后分叉」：**仅 summary `:2358` / daily `:2425` / heatmap `:2533` 走 `aggregateByDay` `:227`**；hourly 走 `aggregateHourlyByDay` `:600`；model-breakdown `:2602-2641`、monthly `:3161-3186`、project-summary `:2744-2798`、project-detail `:2866-2898` 各自遍历自建 Map | 端点逐一抽查（含 `:2604-2641`、`:2744-2760` 逐行） |
| I2 | 「`parseBucketKey` 只用于主队列键（`:3199`、`:3434`）」 | 改为「共 **15 处调用**（`:1973`、`:3174`、`:3199`、`:3223`、`:3238`、`:3434`、`:3570`、`:3636`、`:3655`、`:7212`、`:12286`、`:13378`、`:14251`、`:16376`、`:20776`）+ 定义 `:4238`，全部落在主队列键空间」，结论方向不变 | 本轮重算（16 次出现 = 15 调用 + 1 定义） |
| I3 | 500ms 窗只给常量与日期守卫 | 补真正的判定实现 `:2588-2601`（`>=` 重置 `replayPrefixActive`；`<` 置 `forkedReplaySkip`；skip 点 `:2601`），并注明「跨日靠日期守卫、同日靠 500ms 突发检测」；§4.9 表 4b 行同步补行号 | `rollout.js:2588-2601`、`:4212-4214` |
| I4 | 守卫注释引用 `sync.js:263` | 改为 `:264`（注释句首；整段 `:258-267`），§0 第 7 条与 §5.4 第 3 条两处同改 | `sync.js:264` |
| I5 | §1.6 代码块标 `upload-queue.js:114-123` | 改为 `:112-123`（块首 `await output.close();` 在 112，111 是 `output.sync()`） | `upload-queue.js:111-123` |
| I6 | §1.2「优先子集 / `usage_precision` / 模型三个额外维度（`:4167-4170`）」 | 拆开：优先子集 `PRIORITY_SUBSET_FIELDS` 在 `totalsKey` 内（`:4167-4170`）；`usage_precision` 与输出模型拼接在调用点（`:3404-3407`）；`totalsKey` 本体 `:4155-4171` | `rollout.js:4155-4171`、`:3404-3407` |

### 已确证并写入正文的靶子结论

| 编号 | 结论 | 依据 |
| --- | --- | --- |
| T1（B2） | 两套口径**并存不矛盾**：读端 last-wins 用于展示（`async-queue-reader.js:55`/`:86-91`）；`setMaxTotal`（`sync.js:6585-6589`）取 MAX、`mergeMaxTotals`（`:6574-6583`）跨来源取 MAX，**仅**用于 codex rescan/fork 修复的数据丢失守卫（**唯一调用族 = 四处调用 `:5848`/`:5852`/`:5856`/`:5860`，判定循环 `:5865-5879`；R2 更正：`:5841-5846` 是范围外的键集合守卫**）。原因：撤回零行会把 last-wins 的末行压成 0，用 MAX 才能保住历史峰值。已写入 §1.3 末尾「口径边界」段 | `sync.js:5848-5879`、`:6574-6589`；`local-api.js:202-211`；`async-queue-reader.js:49-57`/`:86-91` |
| T2 | 三份 UTC 桶实现**语义等价**（rollout 权威、claude-categorizer 副本、sync 的 grok 毫秒版），原自标 U **已解除**，已写入 §2.3；存疑 A.4 标记为已解除 | `rollout.js:4173-4190`、`claude-categorizer.js:1175-1191`、`sync.js:4538-4549` |
| T3 | repair 数字自洽：源码 **2.17B = 21.7 亿**；分项 `-1.27B / -474M / -376M / -48M / -6M` 求和 = **2174M ≈ 2.174B**，与总数一致（与 S1/S1b 口径一致）；`0.26.4 HALTS back at v4`（`:242`）+ key 仍为 v4（`:248`）。已写入 §5.3 | `sync.js:234-247`、`:242`、`:248` |
| T4 | 项目身份两条：用量侧 `deriveProjectKeyFromRef`（`rollout.js:4429-4442`）**只吃 URL，无 cwd、无层级归并**；会话侧 `projectKey`（`session-analytics.js:405-408`）取 basename。已写入 §6.1/§6.2 并标「R1 复核确证」；新增残余待验点：`.git` 后缀不剥离 | `rollout.js:4429-4442`；`session-analytics.js:405-408` |

### U 组：保留，并补齐判定条件

7 条 U（U1 fs.js 锁、U2 codexHashes 哈希输入、U3 跨根事件级并入键、U4 sessionUsageDay 三函数体、U5 zcode 修复写盘方式、U6 copilot 桶粒度、U7 若干端点参数）**全部保留**，已在「存疑与待复核点 §A」逐条追加「**判定条件**」（需读的文件/区段，或需观察的行为），并注明 U7 的部分解除项（`user-status` `:2927-2931`、`achievements` `:3470-3471`、`usage-limits` `:3482`）。`billable-total-tokens-migration.test.js` 的「0 用例」观察经复核确认（实测 9 行、`test(` 0 次），用途仍列为 U。（**R2 更新**：U4 已由 R2-3 读全三函数体结案，见 §6.5 与「存疑 §A-6」；其余 U 判定条件不变。）

### R2 修正（第二轮复核后）

依据 `REVIEW-R2-3-aggregation-pricing-consumers.md`（E=2 / I=8 / U=6；样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，全程只读，未修改样本仓）。本轮**采纳 4 条、反驳 1 条（E1 的"macOS 为触发面"部分）**，全部结论均在本轮**独立打开源码复核**后落实。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| E1（§2.4、§0-9、§2.5、§8.3、存疑 B-6/B-8） | 「`tz`/`tz_offset_minutes` 两者皆缺时**静默退化为 UTC 日**」（把两条路径说成同一条），且 R2 判定 macOS 壳 heatmap 不传 tz 是"实际触发面" | 拆成两级写明：① `rowDayKey` 一族（日键 + 日键区间过滤）缺省 **UTC 日**（`:213-225`、`:224`、`:266`）；② `getZonedParts` 系（hourly 日过滤/小时键 `:606`/`:608`、summary `todayStr` `:2382`、heatmap `todayStr` `:2538`、`night_owl` `:482`）三级回退的第三级是**运行环境本机时区**（`:585-592`）。后果：非 UTC 机器 + 不带 tz 时，`-usage-hourly` 的 `day` 默认值（**UTC 今天**，`:3147`）与其过滤谓词（本机时区日键）基准不同 → 返回的是"本机时区日 = `day`"的桶、跨日时段可能整日为空；summary `rolling` 窗口终点与 heatmap 窗口终点同理（`:2382-2397`、`:2538-2544`） | `local-api.js:213-225`、`:224`、`:266`、`:482`、`:521-592`（尤其 `:585-592`）、`:606`、`:608`、`:2382`、`:2538`、`:3147`；`local-api.js:639`（无预填充） |
| E1-反驳（R2-3「暴露面」结论） | R2-3 断言「macOS 壳 heatmap **不传 tz**（`APIClient.swift:73-75`）→ 是实际触发面」 | **不成立，予以反驳**：`:73-75` 只是 `queryItems` 数组字面量，整数组被传进 `fetchWithSource(..., queryItems: withAccountQueryItems([...]))`，而 `withAccountQueryItems` = `withTimeZoneQueryItems(items) + account=1`，**必然追加 `tz` 与 `tz_offset_minutes`**。三端壳全带 tz：macOS `APIClient.swift:241-254`、Linux 扩展 `extension.js:997`、Windows `UsagePoller.cs:321`。故真实暴露面是**不带 tz 的调用者**（裸 HTTP / 脚本 / `test/devin-sync.test.js:262` 这类） | `APIClient.swift:73-77`、`:101-105`、`:241-254`；`extension.js:997`；`UsagePoller.cs:321`；`test/devin-sync.test.js:262` |
| T1 / I1（§1.3、修正记录 T1） | 「唯一调用族在 codex rescan/fork 重建流程（`:5848-5879`）……（两处 `skipping to avoid data loss`，`:5841-5846`、`:5874-5879`）」——范围与引用自相矛盾 | 写明 `mergeMaxTotals` **调用点恰四处**（`:5848`/`:5852`/`:5856`/`:5860`，含判定循环 `:5865-5879`）在 `projectRepairEnabled` 块（`:5823-5880`）内；两处跳过守卫分属不同判据：`:5874-5879` **在范围内**（总量下降且主桶无对应提升）、`:5841-5846` **在范围外**（项目键集合缺失，非 MAX 判据）；另补同流程 `:5813-5822`、`:5882-5890` 两道守卫 | `sync.js:5813-5822`、`:5823-5880`、`:5841-5846`、`:5848-5862`、`:5865-5879`、`:5882-5890` |
| I2（§3.4 表、§7.2 表） | 「24 个整点键」易读成"响应固定 24 项" | 改为"**键空间**为 24 个整点；`aggregateHourlyByDay` 返回 `Array.from(byHour.values())` = **只含有数据的桶，不预填充**"；§7.2 表并把 `day` 默认值基准点名 | `local-api.js:609`、`:639`、`:3147` |
| I6（§2.2） | 「逐处核对上下文后确认全部落在主队列键空间」（R2 只抽验了 8 处，建议降级为"抽样确认"） | **维持"全部"并补证据**：本轮把 15 处逐个打开，容器分别为 `hourlyState.buckets`（11 处）、`attemptHourly.buckets`（`:1973`）、`touchedBuckets` 集合（`:3174`）、游标态 `state.buckets`（`:3636`/`:3655`，`normalizeHourlyState` 迁移）——确认无需降级 | `rollout.js:1973`、`:3174`、`:3199`、`:3223`、`:3238`、`:3434`、`:3570`、`:3636`、`:3655`、`:7212`、`:12286`、`:13378`、`:14251`、`:16376`、`:20776` |
| U 解除（§6.5、存疑 §A-6） | 「`sessionUsageDay` / `withinDayRange` / `scopeSessionUsage` 函数体未逐行读取」列为 U | **结案**：三函数已读全（`:2343-2354` IANA→偏移→**UTC**、`:2356-2362` 区间相交、`:2364-2383` 按桶日键过滤 + 保留 `lifetime_*`），并补记"`sessionUsageDay` 缺省为 UTC，与 `getZonedParts` 缺省（本机时区）不同" | `session-analytics.js:2343-2383` |
| 声明补充（§8.3、存疑 B-6） | 「热力图按 UTC 日 / `night_owl` 按请求时区小时」（两套口径） | 改为三处并存并写明缺省级别：数据日键（`rowDayKey`，缺 tz=UTC 日）／窗口终点 `todayStr`、`night_owl` 小时（`getZonedParts`，缺 tz=本机时区） | `local-api.js:2538`、`:2540`、`:482`、`:585-592` |

保留未动：U1（`fs.js` 锁实现）、U2（`codexHashes` 哈希输入）、U3（跨根事件级并入键）、U5（zcode 修复写盘方式）、U6（copilot 桶粒度）、U7 残余（cloud-session 等端点参数）判定条件不变；§9.9 等测试用例数、`async-queue-reader` 缓存与 27 端点等 R2 已复核项保持原样。
