## 7. 聚合、去重与时间语义

TokenTracker 的本地用量数据**不是事件流，而是一份可重放的「累计快照日志」**。这一选择决定了本章全部结论的形态：写入端只追加、读端只取末行、修复只能靠原子重写或撤回行、时间语义全部落在半小时桶这一粒度上。同一份文件还被两套口径同时读取（展示用末行、守卫用最大值），理解这一点是读懂 repair 体系的前提。

**证据口径**：本章所有 `路径:行号` 均为样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）按 UTF-8 解码、以 `\n` 分行后的**文件实际行号**（与编辑器、`read` 工具一致）。PowerShell 5.1 的 `Get-Content` 在中文环境按 GBK 解码会吞掉多字节行尾、导致行号整体偏移（本仓库最多 15 行），不可用于核对。标 `【未能确认】` 者为未能直证、仅由结构或行为反推的条目，均已附判定条件。

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

**过滤是「日键闭区间」**：`const daily = allDaily.filter((d) => d.day >= from && d.day <= to);`（`src/lib/local-api.js:2364`）；项目端点谓词同型（`:130-134`：`(!from || day >= from) && (!to || day <= to)`，另用 `hasRange` 短路）。`from`/`to` 缺省为空串时比较退化为恒真（即「全部时间」）；区间**两端都含**；过滤粒度是**日**不是桶，因此同一天内无法用 `from/to` 做小时级筛选（小时级只能走 `usage-hourly?day=`）。同一面板里实际并存**三处**时间口径：热力图网格的**数据日键**走 `rowDayKey`（缺 `tz` = UTC 日，`:2540`）；热力图/汇总的**窗口终点** `todayStr` 与 `night_owl` 小时都走 `getZonedParts`（缺 `tz` = 运行环境本机时区，`:2538`、`:2382`、`:482`、`:585-592`）。

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
| 9 | `repairZcodeNativeUsageMigration` `:4994` | `zcodeNativeUsageRepair_2026_08` | `:1761` | 修复 zcode 原生用量模式解析 | 写盘方式 `【未能确认】`（判定条件：读 `repairZcodeUsageMigration` 函数体，确认是「追加零行」还是「tmp+rename 原子重写」，以及是否调用 `dropZcodeQueueRows`） |
| 10 | `repairZcodeInclusiveTokenMigration` `:5002` | `zcodeInclusiveTokenRepair_2026_09` | `:1769` | 修复 zcode「含缓存」token 口径（对应 `dropZcodeQueueRows` `:4884`） | 涉及删除 zcode 行；保留撤回行的开关是签名参数 `dropZcodeQueueRows(filePath, { retainRetractions = false } = {})`（`:4884`） |
| 11 | `migrateLegacyDeepseekHarnessSource` `:3865` | `deepseekHarnessSourceMigration_2026_08` | `:1808` | `source="deepseek"` 重标为 `"dsh"`；为每个旧键**追加显式零行**；把最新规范行重追加到队尾；重置上传偏移 | 只追加 + 偏移自 0 重放（`:3946-3962`） |
| 12 | `migrateCursorUnknownBuckets` `:5173` | `cursorUnknownPurge_2026_04` | `:2151` | cursor 的 unknown 桶清理 | 只追加撤回行（`:5207`） |
| 13 | `repairCodebuddyLogJsonlOverlap` `:5315` | `codebuddyLogJsonlOverlapRepair_2026_08` | `:2575` | 修复 log 与 jsonl 双源重叠；先写临时队列重建（`:5401`），再 tmp+rename 落位（`:5460-5462`） | 原子重写 |
| 14 | `repairWorkbuddyContextUsage` `:5508` | `workbuddyContextUsageRepair_2026_08` | `:2624` | 同型：临时队列重建 + 原子落位（`:5571`、`:5628-5630`） | 原子重写 |
| 15 | `repairGrokQueueFromSessionSnapshots` `:5010` | `grokAppendOnlyRepair_2026_05_v4` | `:2986` | 从 `cursors.grok.sessionSnapshots` 重建 grok 桶；清空游标里所有 `grok\|` 桶与 groupQueued（`:4629-4657`）；追加修复行（`:5126`）；重置上传偏移 | 清空游标态 + 追加重建行 + 偏移清零；**原始队尾有备份**（`:5124`） |
| 16 | `backupExistingFile(filePath)` `:4711` | — | — | 备份工具，供 #5、#15 等调用 | — |

（上表「定义处/调用点」列的行号均属 `src/commands/sync.js`。）grok 修复的 token 拆解是**估算**而非实测：`estimateGrokRepairTotals` 按 80/20 切分输入输出（`:4551-4563`）。

**守卫族与调用范围（第二轮复核后的准确边界）**。原子重写类 repair 的「数据丢失守卫」由两个函数组成：`mergeMaxTotals`（`src/commands/sync.js:6574-6583`）跨「现有队列 + 游标态」与「重建结果」两组来源取 MAX；`setMaxTotal`（`:6585-6589`）对同一键取 MAX。**调用点恰四处**：`:5848`、`:5852`、`:5856`、`:5860`（含判定循环 `:5865-5879`），全部位于 codex rescan/fork 重建流程的 `projectRepairEnabled` 分支内（该 `if` 块为 `:5823-5880`）。这四处**就是「唯一调用族」的全集**。同流程内共有**四道守卫**，判据各不相同，不可混记：

| 守卫 | 位置 | 判据 | 是否在调用族范围内 |
| --- | --- | --- | --- |
| 重建产出守卫 | `src/commands/sync.js:5813-5822` | 重建产出 0 个 codex 桶 | 否 |
| 项目键集合守卫 | `:5841-5846` | `missingProjectKeys = existingProjectKeys \ rebuiltProjectKeys`，用**键集合**而非 MAX | **否**（在 `:5823-5880` 之外，判据也不同） |
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

**四种安全模式（同一仓库都用过）**：只追加撤回行（最安全：`retractStaleSourceQueueRows` `src/lib/rollout.js:19990`、`migrateLegacyDeepseekHarnessSource` `src/commands/sync.js:3929`、`migrateCursorUnknownBuckets` `:5207`）；原子重写 + 前置备份（`backupExistingFile` `:4711` → tmp+rename `:6772`）；原子重写 + 不可复现守卫（磁盘缺文件就整轮跳过，`:264`）；清空重扫（最危险，仅用于误标修复：`dropMimoQueueRows` `:4744`，仅在「磁盘一定还在」时安全）。

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
| 4 | zcode 两个修复的写盘方式 | `【未能确认】` | 读 `repairZcodeUsageMigration` 函数体，确认追加零行还是 tmp+rename，是否调 `dropZcodeQueueRows` |
| 5 | copilot 桶粒度（`copilotOtelAggregateKey`，`src/lib/rollout.js:17462`） | `【未能确认】` | 读该函数体与调用点，确认第二参数来自 `toUtcHalfHourStart` 还是别的桶函数 |
| 6 | 若干端点的参数与返回字段（`cloud-session`、`local-sync`、`cloud-sync-pref`、`proxy-config`、`proxy-test`、`telemetry-pref`、`subscription-manager`、`skills`、`usage-limits`） | 端点存在，参数未逐行确认 | 逐个读 `src/lib/local-api.js` 对应 `if (p === ...)` 分支的 `url.searchParams.get(...)` 集合与响应载荷 |
| 7 | `src/lib/source-metadata.js` 的 scope 过滤规则 | 未展开 | 读该文件全文，列出 `ACCOUNT_LEVEL_SOURCES` 成员与 `normalizeUsageScope` 取值域，对照 `listExcludedSources`（`src/lib/local-api.js:410`） |
| 8 | `test/billable-total-tokens-migration.test.js` 的真实用途（实测 9 行、顶层 `test(` 0 次） | `【未能确认】` | 读全文并 `rg` 其引用方，确认是辅助模块还是失效残留 |

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

**`total_tokens` 从不参与的源码证据**：在 `src/lib/pricing/index.js` 全文 378 行中搜索 `total_tokens`，只有 `:236` 是 `const reportedCost = Number(row?.total_cost_usd);`（**`total_cost_usd` 金额字段，不是 `total_tokens`**，文件名与字段名的视觉相似是唯一的「疑似命中」）；除此之外，`src/lib/pricing/` 目录下三个 JS 文件中该标识符总共只出现 **1 次**且在注释里（`:283-284`），`matcher.js` 与 `litellm-fetcher.js` 中各 0 次。反向验证：`computeRowCost` 的唯一计价分支是 `baseCost` 与两个溢价块，全部只读 `pricing.*` 与五个 token 列（`:257-354`）；`getRowPricing`（`:209-222`）只读 `model`/`source`/`pricing_tier`/`hour_start`。同一不变量在 `src/lib/cursor-config.js:528` 被再次书面确认。

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

生效起点 `Date.UTC(2026, 7, 22, 16, 0, 0)` = `2026-08-22T16:00Z` = 北京时间 `2026-08-23 00:00`；折算是「+08:00 固定偏移」（注释 `:188-192` 解释了为何不能直接对原始瞬时取 `getUTCDay()`）。**价格表里存的是峰值价，折扣由代码施加**（`curated-overrides.json:5` 的 `deepseek_v4_pricing_note`：`Exact entries store peak rates. computeRowCost applies the time-of-use discount from each row's UTC hour_start.`），幅度是四个价格字段各 ×0.5（`src/lib/pricing/index.js:215-221`）。测试：`test/pricing.test.js:891-983`（10 个时间点断言，含「非法时间戳 fail closed 到峰值」「显式 tier 覆盖周末规则」「legacy id 保持静态价」）与 `:985-1016`（**展示型模型名不触发折扣**）、`:1349+`（v4.1 Flash）。

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

**其他金额消费点与前端**。`wrapped` 聚合器（`src/lib/wrapped-aggregator.js`，236 行）**不含任何金额**——`totals` 只有 `tokens / conversations / sources / models / active_days`（`:212-218`），文件内无 `cost` 字样、无 `require("./pricing")`。`quality-per-dollar` 逐行 `computeRowCost(row) || 0`（`src/lib/outcomes-engine.js:24`、`:207`），但 `:208` 的 token 数用 `row.total_tokens`——**钱用 `computeRowCost`、token 数用 `total_tokens`**，这是「绝不用 total_tokens」规则的适用边界示例（它约束的是成本，不是计数）；文档原文 `Costs come entirely from TokenTracker's existing pricing. The sidecar only supplies the accepted/total counts.`（`docs/quality-per-dollar.md:18`），派生量见 `:154`、`:156-157`。技能用量在 `src/lib/local-api.js:3378-3382`（`:3381` 为累加行）；会话侧 `repriceSessionRecord` 逐 bucket / 逐 model_usage 行计价（`src/lib/session-analytics.js:269-308`，`:272`、`:279`、`:302`），会话聚合对已算好的 `cost_usd` 求和（`:2414`、`:2695`）；云端排行榜逐行计价后取整到**分**（`tokentracker-leaderboard-refresh.ts:1181`、`:1349`）。`dashboard/src/` 下 `MODEL_PRICING` / `computeRowCost` / `getModelPricing` **0 命中**：前端只做求和与格式化（`dashboard/src/lib/model-breakdown.ts:116-117`、`:179`、`:217`；`dashboard/src/lib/sessions-insights.js:19-30`），唯一例外是 mock 数据（`dashboard/src/lib/mock-data.ts:391`）与 dev/mock 通道（`dashboard/vite.config.js:508-514`、`:1052-1057`、`:1067` require 同一 pricing 模块），不参与真实路径。云端 `tokentracker-account-model-breakdown.ts:906` 用 `ma.totalCostUsd += subscriptionBacked ? 0 : ...` 确保订阅行不进金额；排行榜展示带免责声明（`dashboard/src/pages/LeaderboardPage.jsx:817` 的 `Based on estimated API pricing, not actual billing`，另见 `:927`、`:978`）。

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
