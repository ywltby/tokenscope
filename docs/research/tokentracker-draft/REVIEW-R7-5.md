# 复核报告 R7-5：主文档 §11–§13 与附录 A/B（第七轮全文复核）

- 被复核对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md`（**4448 行**，本次范围 **L4011–L4448**：`## 11.` / `## 12.` / `## 13.` / 附录 A / 附录 B）
- 交叉核对材料：`tokentracker-draft/` 下 `OPENQ-W1..W4*.md`、`REVIEW-R1-A..F`、`REVIEW-R2-1..3`、`REVIEW-R3-1..4`、`REVIEW-R4-1..2`、`REVIEW-R5-final.md`、`REVIEW-R6-openq-writeback.md`
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`（HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，全程**只读**；复核前后 `git status --porcelain` 均为空）
- 行号口径：`read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`（含 `Get-Content` 默认编码的 GBK 对照实验）；未使用裸 `Get-Content`
- 职责：**证伪**，未修改任何文件（主文档、样本仓库、既有报告均未写入；分析脚本只落 `%TEMP%\r75\`）
- 隐私纪律：本报告只含计数、键路径名、行号与样本仓源码片段；未读取或输出任何本机真实会话内容

## 结论

**§11–§13 与附录 A/B 整体可用，必须修 2 处（1 处表述/取值级、1 处 1 行行号偏差），建议补 2 处引用范围。** 机制层、计数层、附录 A 索引层均未发现新错误：附录 A 用正则独立重生成后**与文档逐行零差异（45/45，含 45 个调用点）**；§13.4 的 GBK 口径硬结论被我独立复现命中（**全 `src/` 累计 52 行、`rollout.js` 15 行**）；§13.1 的 R1 六份自报值与合计（772/50/43/29/650）逐格可加和验证；§12.E 的全部关键数字（65381 / 0 反例 / 60715 / 321 / 32-64 / 433 / 445 / 385-48 / 19174-960-7688 / 535 / 550 / 1870）与 OPENQ 报告或本机复算值一致。

- **任务 A（全量引用）**：范围内共 **585 条**展开引用（唯一 `文件:行` 组合 **512** 个，覆盖 **76** 个文件），**越界 0、指向不存在文件 0**。机械初筛报出的 10 条「越界」经逐条回源全部确认为我脚本的短引用归属歧义（见 A.3），**非文档缺陷**。
- **任务 B**：抽验 **≈250 条**（E=2、I=2、U=0；含附录 A 的 90 个索引点）；其中 §11 常量取值 **31 组全对**、§12 全部条目（A-1 12 + A-2 6 + B-1 4 + B-2 6 + C 25 + D 13 + E 8 + 补充登记 5）逐条回源、§13 台账六轮逐份对照、附录 B 核验 21 条。
- **任务 C**：范围内 **15 张表**列数与分隔行**全部一致**（4 处初筛告警系 `\|` 转义造成的误报）、代码围栏 0 个、反引号奇偶异常 0 行；跨章节口径（常量、计数、判定）**未发现互相矛盾的断言**。

## A. 任务 A：全量引用存在性与越界

### A.1 统计表

| 口径 | 数值 |
| --- | --- |
| 复核范围 | L4011–L4448（438 行） |
| 提取方式 | 反引号内 `` `路径:行号` `` 与 `` `:行号` ``，含 `file:1,2-3,4` 复合形式逐项展开 |
| 展开引用计数 | **585** |
| 唯一 `文件:行号` 组合 | **512** |
| 被引文件数 | **76** |
| 越界 | **0** |
| 指向不存在文件 | **0** |
| 短引用归属不确定 | 0（全部可在同句/同段最近前向文件内闭合） |

### A.2 被引文件行数（本次实际打开并计数的关键文件）

| 文件 | 行数 | 文件 | 行数 |
| --- | --- | --- | --- |
| `src/lib/rollout.js` | 24932 | `src/lib/local-api.js` | 3543 |
| `src/commands/sync.js` | 7251 | `src/lib/session-analytics.js` | 2934 |
| `src/commands/init.js` | 2427 | `src/lib/cursor-store.js` | 1919 |
| `src/commands/status.js` | 1729 | `src/lib/usage-limits.js` | 4520 |
| `src/lib/pricing/index.js` | 378 | `src/lib/pricing/matcher.js` | 439 |
| `src/lib/upload-queue.js` | 134 | `src/lib/async-queue-reader.js` | 117 |
| `src/lib/upload-throttle.js` | 171 | `src/lib/fs.js` | 455 |
| `src/lib/passive-mode.js` | 219 | `migrations/20261005120000_cloud-usage-archive.sql` | 625 |
| `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts` | 1449 | `test/edge-pricing-parity.test.js` | 420 |
| `TokenTrackerWin/TrayApplicationContext.cs` | 1214 | `TokenTrackerWin/UsagePoller.cs` | 526 |
| `TokenTrackerWin/QuotaWidgetWindow.cs` | 554 | `TokenTrackerBar/.../APIClient.swift` | 313 |

（**D13 的 10 个行数断言全部复核为真**：24932 / 7251 / 2427 / 3543 / 2934 / 1729 / 378 / 439 / 775 / 134，逐项用 `ReadAllLines(UTF8)` 重取。）

### A.3 初筛「越界」的 10 条——逐条裁定：全部为脚本归属歧义，非文档缺陷

| # | 文档行 | 表面归属 | 实际归属 | 裁定 |
| --- | --- | --- | --- | --- |
| 1 | L4046 | `scripts/cloud-usage-archive.cjs:186/221` | `migrations/20261005120000_cloud-usage-archive.sql`（`:186` = `CREATE FUNCTION cloud_prepare_usage_archive`、`:221` = `cloud_commit_usage_archive`） | 引用正确；短引用承前文「同 SQL」 |
| 2-3 | L4122 | `UsagePoller.cs:1014-1024/1022` | `TokenTrackerWin/TrayApplicationContext.cs:1014-1024`（`OnLimitsUpdated`） | 正确 |
| 4-5 | L4257 | `docs/quality-per-dollar.md:154/156-157` | 该条正是「**R3-3 S1 引用越界**」的**例证文本**（doc 仅 67 行，真出处 `src/lib/outcomes-engine.js`） | 论述内容，非引用 |
| 6-7 | L4290 | `init.js:16080/16084` | `src/lib/rollout.js`（`:16084` = `return { recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };`；`16080` 为**被反驳的错误值**） | 论述内容 + 正确引用 |
| 8-10 | L4310 | `status.js:2545/2564` | `src/lib/local-api.js:2545`（`byDay` 构建）、`:2564`（`byDay.get(day)` 查表） | 正确 |

## B. 任务 B：核验明细

### B.1 §11 常量表（31 组取值逐项回源，全部命中）

| 常量 | 文档值 | 源码实测 | 判定 |
| --- | --- | --- | --- |
| `batchSize` 默认 | 200 | `sync.js:4315 batchSize = 200` | ✅ |
| `MAX_INGEST_BUCKETS` | 500 | `sync.js:4313 const MAX_INGEST_BUCKETS = 500;` | ✅ |
| 社区档批数 | 10 | `sync.js:4392 batchBudget = Math.min(batchBudget, 10)` | ✅ |
| 批量夹取 | `:4329` | `sync.js:4329 Math.min(Math.max(1, ...), MAX_INGEST_BUCKETS)` | ✅ |
| `UPLOAD_BUDGET_MS` | 90 s | `sync.js:168 const UPLOAD_BUDGET_MS = 90_000;` | ✅ |
| 剩余预算→AbortSignal | `:4360-4362` | `:4360 remainingMs`、`:4362 AbortSignal.timeout(...)` | ✅ |
| 超时有成功批次不抛 | `:4402-4411` | `:4407 if (!requestTimedOut \|\| batches === 0) throw error;` | ✅ |
| hook 节流 | 20 s | `init.js:1519 // Throttle spawn: at most once per 20 seconds.`；`:1525 if (now - last > 20_000)`；`:1526` 写时间戳先于 spawn | ✅ |
| `LOCK_HEARTBEAT_MS`/`LOCK_STALE_MS` | 30 s / 5 min | `fs.js:69 LOCK_STALE_MS = 5*60*1000`、`:70 LOCK_HEARTBEAT_MS = 30*1000` | ✅ |
| 老格式锁 mtime 判据 | `:105-144` | `fs.js:105 existingLockCanBeReclaimed`、`:136 Date.now() - stat.mtimeMs > LOCK_STALE_MS` | ✅ |
| 回收闸/过渡守卫 4/64/5/1000 | 同 | `fs.js:71-74` 逐字 | ✅ |
| JSON 锁超时 30 s | 同 | `fs.js:416 updateJsonLocked(`、`:419 { timeoutMs = 30_000, retryMs = 10 }` | ✅ |
| 冷扫描 24 h / 288 次 | 同 | `sync.js:294 24*60*60*1000`、`:301 288`、`:3772 lastFullScanAtMs - nowMs > 5*60*1000` | ✅ |
| 游标 16 MB | 同 | `cursor-store.js:21 DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024`（`:78-82` shouldActivate） | ✅ |
| 项目重扫 24 h | 同 | `cursor-store.js:22` + `:1406 checkedAtMs + PROJECT_ABSENT_CONTEXT_RESCAN_MS` | ✅ |
| 长上下文 272000 | 同 | `pricing/index.js:31`（全文仅 :31 定义 + :375 导出，**零内部读取成立**）；生效 `codex-rollout-parser.js:51` → `:733` | ✅ |
| OpenAI 溢价系列 | `:266-306` | `:267-269 gpt-5.6/gpt-5.6-sol/gpt-6-astra/gpt-6-sol/gpt-6.1-sol`；`:300-306` 溢价公式 | ✅ |
| DeepSeek 周末起点 | `Date.UTC(2026,7,22,16,0,0)` | `pricing/index.js:186` 逐字 | ✅ |
| DeepSeek 峰窗 `[1,4)∪[6,10)` | 同 | `:205-206 getUTCHours(); return !((hour>=1&&hour<4)\|\|(hour>=6&&hour<10));` | ✅ |
| DeepSeek ×0.5（四类） | 同 | `:215-221` 四类 ×0.5；`:213 acode` 豁免 | ✅ |
| 价格 TTL / 取数超时 | 24 h / 10 s | `litellm-fetcher.js:12 DEFAULT_TTL_MS = 24*60*60*1000`、`:13 10_000` | ✅ |
| 四级降级链 | disk→upstream→stale→seed | `:128-137` / `:139-146` / `:148-158` / `:160-163` 逐级命中 | ✅ |
| 负缓存 | `:64-66,120-131`（清空 `:86`、`:105`） | 逐字命中 | ✅ |
| 策展表 84/1/64/47 | 同 | JSON 解析实测 `exact=84, alias=1, source_exact.acode=64, fuzzy=47`；775 行 / 29754 字节 | ✅ |
| seed 2285=1+2284 | 同 | `_meta` 存在、models=2284、`generated_at 2026-05-26T11:12:29.761Z`、256869 字节 | ✅ |
| 云端手抄表 119 / 64 | `ts:142-332` / `:339-404` | 逐块键计数实测 119 / 64；`:142` = `const MODEL_PRICING...`、`:339` = `IFLYTEK_MAAS_MODEL_PRICING` | ✅ |
| priority 乘数 3 条 = 2 | 同 | `gpt-6-astra=2, gpt-6.1-sol=2, gpt-6-sol=2` | ✅ |
| `FORK_REPLAY_GAP_MS` | 500 | `rollout.js:4210 const FORK_REPLAY_GAP_MS = 500;`（注释 `:4205` 含 ≥4.6s / ≥11s） | ✅ |
| fork 应用点 / `replayPrefixActive` | `:2588-2601` / `:2339` | 逐字命中（`:2588` 突发检测、`:2601` 应用、`:2339 startOffset === 0`） | ✅ |
| Claude 哈希上限 | 10 万 | `rollout.js:1236-1237 slice(allHashes.length - 100_000)`；注释 `:1021-1025` 逐字 | ✅ |
| `loadEventSet` 日分片 | `:681-726` | `cursor-store.js:681 loadEventSet`、`:695 loadEventSetUnchecked`、`:723 loadedEventSets.set` | ✅ |
| project 上限 10 / sessions 2000 | 同 | `local-api.js:95 PROJECT_USAGE_MAX_ENTRIES = 10`、`:2739 Math.min(floor, PROJECT_USAGE_MAX_ENTRIES)`、`:2505 Math.min(limitParam, 2000)`、`:2785` 排序 | ✅ |
| 压缩块 64 KiB / 1 MB | 同 | `upload-queue.js:5 BACKLOG_BYTES = 1_000_000`、`:6 CHUNK_BYTES = 64*1024` | ✅ |
| 读侧分块 / 缓存 4 文件 | `:5` / `:16,94` | `async-queue-reader.js:5 CHUNK_BYTES`、`:16 maxEntries = 4`、`:94` 淘汰 | ✅ |
| seenIds 10000 | `:16041-16050`、`:16680`、`:18089-18091` | 三处逐字命中（含 `slice(-10_000)`） | ✅ |
| claims 7 天 / 10000 | `:17375-17405`、`:18058-18064` | `:17376 COPILOT_USAGE_CLAIM_RETENTION_MS = 7*24*60*60*1000`、`:17377 ..._MAX_ENTRIES = 10_000`、`:17402-17404` 截断 | ✅ |
| 快照上限 10000 | `:19334-19355`、`:19645-19649` | `:19334 capCopilotAppSessionTotals(sessionTotals, maxEntries = 10_000)`、`:19645-19648 capGrokSessionSnapshots`（`<= 10_000`） | ✅ |
| 记录上限 64 MB / 8 MB | `:15712-15715`、`:12853` | `:15715 OMP_LIKE_MAX_RECORD_BYTES = 64*1024*1024`、`:12853 LMSTUDIO_WINDOW_BYTES = 8*1024*1024`；Codex 主路径仅传 `invalidUtf8`（`:2397`） | ✅ |
| 上传退避 60 s→30 min / 2^step / 10（记 20） | `upload-throttle.js:8-9,109-110,123` | `:8 backoffInitialMs: 60_000`、`:9 backoffMaxMs: 30*60_000`、`:109 Math.min(10, ...)`、`:110 Math.pow(2, step)`、`:123 Math.min(20, ...)` | ✅ |
| 归档包 10000 / 8 MiB | `SQL:72,213,309,186,221` | `:72 CHECK(row_count > 0 AND row_count <= 10000)`、`:213`/`:309 pg_column_size>8388608`、`:186`/`:221` 两函数 | ✅ |
| 后台源集 4 / 自动源集 45 | `sync.js:367` / `:320-366` | 成员实测 `acode, codex, every-code, reasonix`（4）；`AUTO_SYNC_SOURCES` 恰 45 项；别名表 8 条（`:311-318`） | ✅ |
| 帮助文本 | `cli.js:97-101` | `:97 "sync parses ~/.codex/sessions/**/rollout-*.jsonl ..."` | ✅ |
| 半小时桶 / 副本 | `rollout.js:4173-4190` / `claude-categorizer.js:1175-1191` | 两份实现逐字（副本仅少一个中间变量） | ✅ |
| 日键回退 | `local-api.js:213-225` | `:213 rowDayKey`、`:224 return hs.slice(0, 10);` | 取值 ✅（**表述 ❌ 见 E1**） |

### B.2 §12 逐条核验

**A-1 已判定（12 条）——来源与判据均真实**

| # | 核验要点 | 源报告 | 判定 |
| --- | --- | --- | --- |
| A1 | `const endOffset = st.size;` **全文恰 3 处**（`.Select-String` 实测 `L2291/L2682/L2793`）；`:2843-2847` JSON.parse 失败被吞；`:1194` 游标落点；W4 的 381/107/607 数字逐字 | W4 §A1 | ✅ |
| A2 | `rollout.js:4927-4928`（cache_creation/cache_read 取值）、`codex-token-usage.js:25-27`（`?? value.cache_write_input_tokens` 兜底）逐字 | W3 §5 | ✅ |
| A3 | `session-analytics.js:101`（`PLACEHOLDER_MODELS` 含 `<synthetic>`）、`:621-624`、`:1968-1971` 逐字 | W3 §6 | ✅ |
| A4 | `rollout.js:3181-3194`（`legacyGroups`/`groupQueued` 收集）、`:3430` 起写出段；`sync.js:216-220` 注释逐字 | W4 §A4 | ✅ |
| A5 | `sync.js:4422` 起 `readQueueBatch` 主循环、`:3824` 导出点、`local-api.js:202-211` 逐字 | W4 §A5 | ✅ |
| A8 | `rollout.js:1236-1237`、`:1035`、`:1021-1025` 逐字 | W4 §A8 | ✅ |
| A9 | `:493-500`（`rebuildingCodexBaseline` 五条件）、`:456-460`（稳态短路后 `lastTotal` 兜底）、`:428-437`、`:584`（传入点）、`:513-517`（pre-#652）逐字 | W1 §9 | ✅ |
| A11 | `:6639-6660`、`:3325-3347` 区间成立 | W4 §A11 | ✅ |
| A12 | `:22477-22492`（仅 `Number.isSafeInteger(x) && x >= 0` 校验）、`:22500-22508`（全零 totals 写出）、`:22749-22751`（落桶）逐字 | W4 §A12 | ✅ |
| A16 | `:17189-17195`（craft clamp）、`:17011`（reasonix `diffReasonixTotals`）逐字；W4 的 1100→1870、虚增 550 逐字 | W4 §A16 | ✅ |
| A17 | `:6529-6589`（`:6540` 调用）、`:5848-5873` 守门逐字 | W4 §A17 | ✅ |
| A18 | `codex-model-attribution.js:47-51`（「no model/rerouted event has been observed…shapes are inferred」）逐字 | W3 §4 | ✅ |

**A-2（6 条）**：A6 `upload-queue.js:97-110` 逐字（`:97-103` 复制 `[0,offset)`、`:105-110` 重写 pending）；A7 `multi-install-parser.js:49-54` 逐字，且**生产调用点实测 11 处**（`sync.js:1419/1467/1545/1686/1722/1776/1994/2063/2325/2379/2478`，与清单一一对应 —— R6 的修正已落地）；A10 机制名 `shouldCountCopilotAppSession`（`:19210` 定义 / `:19466` 调用）与 `usageClaimsComplete`（`:17845`/`:17904-17905`/`:18037`）实测存在；A13 `:16925-16934`（`~/.reasonix` + `%APPDATA%\reasonix`）、`:16970-16975`（仅路径去重）逐字；A14 `:22111` 逐字；A15 `:20249-20252`、`:20265`、`:19691` 逐字。

**B-1 已判定（4 条）**

| # | 核验要点 | 判定 |
| --- | --- | --- |
| B6 | `usage-limits.js:4279-4280`（`claudeServiceStatus` 挂 `claude.service_status`，条件 `configured && indicator !== "none"`）、`:4183-4185`（无 token → `Promise.resolve(null)`）、`:4406` 起 data、`:4055` 并行位、`:4044-4052` 缓存注释；`ark-coding-plan-limits.js:410-413,65,297-301`、`ark-agent-plan-limits.js:330-333,51`、`qoder-limits.js:819-827`（CN 分离凭据/目录/命名空间）全部逐字 | ✅ |
| B7 | `TrayApplicationContext.cs:84`（`5 * 60 * 1000`）、`:240`（Tick）、`:965-971`（Running 启动+立即触发）、`:972-974`（Failed 停止）、`:1186/1198`（Stop/Dispose）；`ServerManager.cs:83`（5 min 预算）、`:212-228`（`_syncInFlight` 单飞）、`:694/700`（两环境变量，仅 serve）；`serve.js:35`（60 s 兜底）、`:304-309`（`!== "windows"` 直接 return null + 三句注释）、`:314-315` | ✅ |
| B8 | `local-api.js:794-796`、`:2313-2321`、`:2324`、`:1629-1637`、`:2526-2530`（context-health 无包装直回）、`:1644-1657`（预检先于 OPTIONS + 403 `untrusted_auth_origin`）；`context-health.js:91-118`（severity 50000/20000、六键 breakdown、前 10、provenance、400 token/5 servers、缓存 8 键）；`cloud-sync.ts:159-162,180-183` | ✅ |
| B9 | `20260817120000_account-session-states.sql:166-172` 注释、`:202-248`（`DISTINCT ON (user_id, COALESCE(machine_cluster_id, device_id::text), source, model, hour_start)` + `ORDER BY total_tokens DESC, updated_at DESC` + 跨簇 SUM + `revoked_at IS NULL`）；`20260821060000:19-23,49-78,98-111`（quarantine 搬移 + 一致性闸门）；`20260923093000:49-95,352-359`（`model <> ''`/`lower(model) <> 'auto'`/整账号排除）；`20260718082000:20-24,59-63`（`BEFORE INSERT OR UPDATE OF daily_growth` 触发器）；`community-stats-refresh-cron.sql:8-12`（`'17 * * * *'`） | ✅ |

**B-2（6 条）**：B1–B4、B10 属「需外部信息」（云端 ingest 代码、edge 部署修订、虚构时间线的价页、LiteLLM 上游现状、云端保留策略）——**在样本仓库内确实不可判定**，维持标记正确；B5 的 `local-api.js:2588 streak_days: 0`、`:2587 active_days`、`heatmap-compact.js:108-109` 透传、`pet-linux-host.js:198` 均已回源成立（仅「Windows 壳显示值」为运行时项）。

**C 类 25 条（逐条回源，全部成立）**：C1 `:516` 起 `cmdSync`、`:3709` 收尾行数 **3194** ✅、`skipNoOpCursorCommit` 条件项**恰 13 个**（`:3424-3437` 逐项数）；C2 `fs.js:178-204`（issue #431 注释+迭代清扫）✅；C3 六处 append 全中（`rollout.js:3483/20031/22594`、`sync.js:5126/5207/5281`）+ `sync.js:4114` + `rollout.js:3536` ✅；C4 `sync.js:234-248`（`2.17B` 事故注释）、`:4744`、`:6844` ✅；C5 `grok-hook.js:85-91` ✅；C6 `sync.js:249-256`（云端 conversation_count 恒 0 事故）✅；C7 `cursor-store.js:21,77-86,165-197`（legacy 存根方法即「能力不等价」）✅；C8 `bin/tracker.js:36-40`（`unprotected === true` → `exit(1)`）✅；C9 3543 行 + 头像代理（`:43-48`/`:2030-2065`）/宠物（`:1705`/`:1731`/`:1756`）✅；C11 `rollout.js:489-492` ✅；C12 `serve.js:15-21`（7681）✅；C13 `status.js:1024-1036` 传 11 键 vs `passive-mode.js:93-186` 消费 8 键、JSDoc `:88-90` 写 9 键（含 `codex`/`every_code` 而非 `codex_notify`/`every_code_notify`）→ 双向不一致成立 ✅；C14 双份定义 ✅；C15 5 个 vs 2 个（`pricing/index.js:248-253` vs `audit-token-correctness.cjs:34-40`）✅；C16 **9 键差集实测恰 9**（`claude-opus-5.5, claude-sonnet-5.5, cline-gateway-models, claude-opus-4-8, claude-haiku-4-5, antigravity-gpt-oss-120b, grok-4.5-build, grok-build-free, grok-4.5-build-free`），含两条 grok ✅，`edge-pricing-parity.test.js:76-85` 逐字（仅 edge↔edge `byte-identical`）✅；C17 本地 4 成员 vs 云端 `:575` 2 个 ✅；C18 **`:569-613` 内 `long_context\|priority` 命中 0** ✅；C19 `:2415`/`:2648` `toFixed(6)`、`session-analytics.js:289` 浮点、`ts:1349` 取整到分 ✅；C20 两份桶实现 ✅；C21 `pricing/index.js:248-256` ✅；C22 逐点命中（`:2540` UTC 网格终点、`:2563` UTC 单元格键、`:2537` aggregateByDay、`:2545` byDay、`:482` 时区小时）+ 补充核实 **`usage-hourly`（`:3145-3152`）`day` 默认 `new Date().toISOString().slice(0,10)`（UTC 今天）而过滤走 `rowDayKey`（当地日键）** ✅；C23 `:2586`/`:2588` + `heatmap-compact.js:108-109` ✅；C24 `sync.js:6871-6921`（原子重写 + 全量替换）✅；C25 `:17189-17195`/`:17011` ✅。

**D 类 13 条（逐条）**：D1 `wsl-probe.js:11` 定义 + `:268` 导出，生产调用 0 ✅；D2 `init.js:1423`/`:339`、`:176/234/359`（全文件 `link_code_state` 仅此 3 处，无读写）、`runtime-config.js:149`、`cli.js:78/92` ✅；D3 `:8371-8372`/`:8374`/`:8316`/`:8482` ✅；D4 `:5025`/`:5033`/`:243` ✅；D5 `:13874-13887`（`FROM workspace_chats` + `json_extract` + 仅 whereClause）✅；D6 `:12054`/`:12069-12073`/`:12086` ✅；D7 `:9493`/`:9495`/`:9497-9502`/`:9669`/`:9671` ✅；D8 `:9940-9943`/`:10476-10485`（五桶含 `reasoningTokens`）✅；D9 `:11100-11260` 段 + `:11246` ✅；D10 `vite.config.js:508-514`/`:1052-1057` ✅；D11 119 键 / 9 键差集 ✅；D12 三份桶实现（含 `sync.js:4538-4549` 的 `toGrokRepairHalfHourStart` 毫秒版）✅；D13 10 项行数 ✅。

**E 类 8 条（与 OPENQ-W3 逐值对照）**：E1 65381 / 0 反例 / 上界 1.0000 / 60715 / 328 / 322 / 100% 同现 ✅；E2 各 27802、同现 27802、52 文件 49 单调不减 + 3 次回退 −2965231/−3780242/−34082848、17 文件全单调 ✅；E3 35351865647 vs 16395234、约 2000 倍 ✅；E4 **321 条**（last 320 + total 1）、差值 6456–205888 ✅；E5 32/64、433、385/48、445=433+12、归类 19174/960/7688（合计 27822 ✅）、上游 1213/167/25%/814/58 出自 `codex-service-tier.js:8-10` 与 `:22` ✅；E6 1288/406、`reasoning_output_tokens` 0、`<synthetic>` 6 行、cache 别名 0、zcode `cacheWriteTokens` 87/87 全 0 ✅；E7 14/22、1098、535/535 ✅；E8 三处 SessionEnd 无配置 ✅。

**补充登记（5 项）**：分头上报（`status.js:1317-1318`omp / `:1363`OpenCode+`:1362` / `:1452`Grok+`:1451` / `:1296-1298`OpenClaw / `:1445-1450` 汇总）**逐字命中**，`passive-mode.js` 实测 `out.push(buildEntry` 恰 **8** 处；`usage_precision` 全文命中 **16 行**且行号集合与附表完全一致、`sessions-api.ts:113` 类型联合确不含 `"estimated"`；`codexServiceTier` 上游注释逐字（**814/58 在 `:22`，见 I2**）；CodeBuddy/WorkBuddy 注释断言 `init.js:623-625,1034-1036,1048-1049` 逐字。

### B.3 §13 台账核对（逐份打开各轮报告自报值）

| 轮次 | 文档自报 | 报告自报 | 判定 |
| --- | --- | --- | --- |
| R1-A | 139 / 9 / 9 / 10 / 111 | 同（「抽验断言数：139（E=9，I=9，U=10，通过 111）」） | ✅ |
| R1-B | 46 / 1 / 8 / 4 / 33 | 同 | ✅ |
| R1-C | 141 / 12 / 7 / 2 / 120 | 同 | ✅ |
| R1-D | 188（代码块 62）/ 9 / 6 / 3 / 170 | 同（「E=9，I=6，U=3（其余 170 条）」） | ✅ |
| R1-E | 157 / 8 / 6 / 7 / 136 | 同 | ✅ |
| R1-F | 101 / 11 / 7 / 3 / 80 | 101（S5 58+S6 43）/ 11 / 7 / 3；80 = 101−11−7−3 自洽 | ✅ |
| **合计** | **772 / 50 / 43 / 29 / 650** | 自算 139+46+141+188+157+101 = **772**；9+1+12+9+8+11 = **50**；9+8+7+6+6+7 = **43**；10+4+2+3+7+3 = **29**；111+33+120+170+136+80 = **650** | ✅ |
| R2-1 | A40（35✅/4⚠️/1❌）、B40、C13 | 同 | ✅ |
| R2-2 | A40、B54、C14、686 条筛查、6 处残留 | 686 = S3a 290 + S3b 396 ✅ | ✅ |
| R2-3 | A49（18/17/14）+B46（18/16/12）+C10 | 同 | ✅ |
| R3-1 | 182 点、99.5%、必修 7（清单 12） | 同（「70 个引用簇 / 182 个具体行号点」） | ✅ |
| R3-2 | 72、88.9%（64/72）、17 条（P0×4/P1×7/P2×6） | 同（4+7+6 = 17 自洽） | ✅ |
| R3-3 | 112、92.0%（103/112；行号级 108/112） | 同（报告写 96.4% = 108/112 自洽） | ✅ |
| R3-4 | 112、94.6%（106/112）、E1–E6 + I1–I5 | 同 | ✅ |
| R4-1 | 29/29、50 条/约 80 点、47（94.0%）/77（96.3%）、3 条【低】 | 同 | ✅ |
| R4-2 | 32（31 成立 + 1 新错）、44（43 通过 97.7%、硬错 0）、2【中】+2 低危 | 同 | ✅ |
| R5 | 30/30、96 围栏、116 表、55 处 §X.Y、160 裸行号抽 22 | 同（930 条去重、步长 31） | ✅ |
| R6 | §12 28 条 + C25 + E1–E8、21 组、40 处、6 项修正 | 同 | ✅ |

**§13.3–§13.10 抽验**：§13.3 第 1 条 `:7270`/`:22791` 逐字成立、`:20551` 差 1 行（**E2**）；第 2 条 `:19003-19013` 实测恰 **9 键** ✅；第 4 条 `:2291` 归属 `parseRolloutFile`（`:2264`）✅；第 6 条 Droid `:15343-15346` 注释、`:15348` 调用、`:15333`/`:15338` 为旧值 ✅；第 5 条 macOS `fetchHeatmap`（`APIClient.swift:73`）+ `withAccountQueryItems`（`:252`）→ `withTimeZoneQueryItems`（`:241`）链成立 ✅。§13.4 独立复现：CP936 口径 **`rollout.js` 24917（差 15）**、**全 `src/` 累计漂移 52 行**（20 个文件），与文档**逐字一致** ✅。§13.6/§13.7：`SUPPORTED_PROVIDERS`（`:119-165`）实测 **45 项**；我按 `applyIntegrationSetup`（`:674-1149`）的 `label` 集合独立归类得 **30 家出现于该函数 + 15 家仅欢迎名单 = 45**，且 12+1+17 = 30 —— **与 `12/1/17/15` 完全自洽**；`16080` 在 `rollout.js` 全文零命中、正确值 `:16084` ✅；`setMaxTotal(` 调用点 `:6492/6512/6540/6559/6579`（定义 `:6585`）实测逐字 ✅。§13.9：W1 8+1+0、W2 8+1+0、W3 10+0+0、W4 8+0+0 与四份报告自报分布一致 ✅；A 类 18 = 12+6、B 类 10 = 4+6 自洽 ✅。§13.10 六项修正全部落地（E4 表头 321 ✅、A7 11 处 ✅、§13.9 逐章节列明 ✅、三处口径注 ✅）。

### B.4 附录 A（独立重生成）

用 `^(async\s+)?function\s+\w+\s*\(` 正则独立重扫 `src/lib/rollout.js`（UTF-8 口径，24932 行）：

- 顶层函数 **572** 个 ✅（与文档自述一致）
- 筛出 `parse\w*Incremental` **45** 个 ✅
- **函数名 / 起始行 / 覆盖行数 45 项逐行完全一致，差异数 = 0**（无 ±1 偏差；含 `parseOmpIncremental` 13 行、`parsePiIncremental` 10 行两个薄壳与 `OmpLike`/`PiLike` 两个共享实现）
- `sync.js` 调用点抽查 **45/45 命中**（`:1046/:5769`、`:1201`、`:1258`、`:1361`、`:1105/:4117`、`:1375/:1665`、`:1471/:1549`、`:1509/:1599`、`:1642`、`:2173`、`:2307`（Kiro source 开关，源码 `if (sourceAllowed("kiro"))`）、`:2358`、`:2429`（KiroCli source 开关）、`:2516`、`:2544`、`:2594/:5409`、`:2639/:2674/:5579`、`:2091`、`:2124`、`:2074`、`:1879`、`:1897`、`:1916`、`:1935`、`:2005`、`:2024/:6698`、`:1960`、`:2708`、`:2747`、`:2780`、`:2809`、`:2837`、`:2890`、`:2865`、`:3108`、`:3053/:3282`、`:3216`、`:2960`、`:1315`、`:2200`、`:2280`、`:1819`、`:1855`）
- 三点说明全部成立：`parseDumateIncremental` 定义不在 `rollout.js`（出现在 `sync.js:1701`，`sourceAllowed("dumate")` 在 `:1696`）✅；`sourceAllowed(<字面量>)` 实测 **46 个**不同实参，与我独立枚举的集合**逐个一致** ✅

### B.5 附录 B 术语表（核验 21 条）

队列（`sync.js:569` + `rollout.js:3482-3484`）✅；项目队列（`:571`、`:3654-3659` 注释逐字）✅；桶 / 项目桶（`:4221-4245`、`:4228` vs `:4238`）✅；游标（`cursor-store.js:13-23`、`rollout.js:1194`）✅；代际存储（**`gen-${Date.now()}-${crypto.randomUUID().slice(0, 8)}`（`:1820-1821`）**、`manifest.current/rollback/previous`（`:979-983`、`:1099`）、区间 `:855-1016`）✅；retraction（`:3251-3317`、`:3351-3402`）✅；billable（`:3339 ?? total_tokens`）✅；`usage_precision`（16 行集合逐一对上，`queuedKey` 参与判定见 `:3326`）✅；priority 子集（`:4073-4079`、`:3342`、`:4167-4170`）✅；权威成本（`pricing/index.js:32`、`:236-241`）✅；last-wins（`local-api.js:202-211`、`async-queue-reader.js:51-57,86`）✅；setMaxTotal（`sync.js:6574-6589`、调用族 `:5848-5879`）✅；血缘修复（`:987-999`、`:1189-1234`、OpenCode `:3729-3748`/`:3959`）✅；claim（`:17375-17405`、`:18058-18064`）✅；evidence（`:6726` 悬空注释、`local-api.js:155-200`）✅；deferred（`:952-959`、`:1147-1192`、`:1177-1181` —— 「`--auto` 下静默」逐字）✅；union 安装模式（`install-resolver.js:6-18`）✅；扫根三态（`scan-roots.js:89-110`、`:248-253`）✅；账户级来源（`source-metadata.js:1,7-13`）✅；半小时桶（`:4173-4190` + 副本）✅；fork 重放突发（`:2330-2340`、`:2556-2601`、`:4203-4214`）✅；内容等值短路（`opencode-config.js:73-75`）✅；哨兵串（`omp-hook.js:17,388-395`、`init.js:813`）✅；负缓存 ✅；候选式计价（`matcher.js:3-12`、`:252-386`）✅。

## C. 任务 C：内部一致性

- **§11 ↔ 正文其它章节**：长上下文阈值「本文件零读取」↔ C14 ✅；16 MB ↔ C7 ✅；哈希 10 万 ↔ A8 ✅；500 ms ↔ 附录 B ✅；10/2000 ↔ §9.2 ✅；119 键 ↔ D11 ✅；272000 ↔ §8 与 C18 ✅。
- **§12 ↔ §13**：A 类 18 = A-1 12 + A-2 6 ✅；B 类 10 = B-1 4 + B-2 6 ✅；C 类 25（末条 = A16 迁入，编号连续）✅；E1–E8 与 §13.9「新增 8 项」✅；§13.10 的「28 条」= 12+6+4+6 ✅；R6 六项修正逐条落地 ✅。
- **§12 内部**：E5（433/445、385+48）与该节补充登记（同数字）✅；C16「9 键」与 D11「差集 9 键」✅；A7「11 处」与 11 个行号 ✅；C19 三处精度与 `ts:1349` ✅。
- **结构**：范围内 **15 张表** —— 列数与分隔行**全部一致**（脚本初筛的 4 处告警均为 `\|` 转义导致的误报，按其排除后 0 问题）；**代码围栏 0 个**（该范围无代码块）；反引号奇偶异常 **0 行**；无断行表格（文末附录 B 表格完整）。

### 范围内的 7 张核心表（§11 速查表 + §12 六张）

| 表 | 起行 | 表头列数 | 数据行列数一致 | 分隔行 |
| --- | --- | --- | --- | --- |
| §11 常量与阈值速查 | 4013 | 4 | ✅ | ✅ |
| §12 A-1 已判定 | 4063 | 3 | ✅ | ✅ |
| §12 A-2 部分解决 | 4080 | 3 | ✅ | ✅ |
| §12 B-1 已判定 | 4100 | 3 | ✅ | ✅ |
| §12 B-2 尚需外部信息 | 4109 | 3 | ✅ | ✅ |
| §12 D 已结案 | 4157 | 3 | ✅ | ✅ |
| §12 E 语料实证 | 4179 | 3 | ✅ | ✅ |

## D. 错误（E）—— 必须修

- **[E1] §11「日键回退」行末「（注释在 `:224`）」不成立**（同源表述亦见 §10.3 / L3994）。
  - 断言：`rowDayKey` 有有效时区时用 `Intl.DateTimeFormat` 分区格式化，否则 `hour_start.slice(0,10)`（**注释在 `:224`**）。
  - 实际：`src/lib/local-api.js:224` = `  return hs.slice(0, 10);` —— **是退化分支的可执行代码行，不是注释**；且 `:213-225` 整个函数体内**没有任何注释行**（`:213` 函数头、`:216-222` 时区分支、`:224` 退化返回、`:225` 闭括号）。
  - 修正建议：删去「（注释在 `:224`）」，或改为「（退化返回在 `:224`）」；§10.3 同步改。
- **[E2] §13.3 第 1 条的行号差 1 行**：`recordsProcessed: eventsAggregated` 引作 `src/lib/rollout.js:20551`。
  - 实际：`:20551` = `  return {`；`recordsProcessed: eventsAggregated,` 在 **`:20552`**（`:20553` 才是 `eventsAggregated`）。
  - 同条另两处行号正确（`:7270` = `return { recordsProcessed: total, eventsAggregated, bucketsQueued };`；`:22791` = 同形态 + `skippedRows`）。
  - 修正建议：`:20551` → `:20552`。

## E. 不完整（I）—— 建议补

- **[I1] §12 C10 的引用范围漏 `context.scanSources` 的实际位置**。断言列三个测试钩子并统一引 `src/commands/sync.js:519-538`；实测 `context.traeCnFetchImpl`（`:529`）与 `context.cursorSyncDeps`（`:535`）在区间内，但 `context.scanSources` 全文件仅出现于 **`:720`**（`if (Array.isArray(context.scanSources) && !sources.some(source => context.scanSources.includes(source))) return false;`）。建议改为 `:519-538,720`。
- **[I2] §12 补充登记（service_tier 条）与 §12.E E5 的出处标注不完整**。两处都写「上游注释的『1213 文件 / 167 命中 / 25%、814 default / 58 priority』……（`src/lib/codex-service-tier.js:8-15`）」；实测 `:8-10` 覆盖 1213/167/25%，而 **`814` 与 `58` 在 `:22`**（`// Observed values on the local corpus are "default" (814) and "priority" (58).`）。建议补 `:22`。

## F. 无法验证（U）

范围内无 U 级新发现：§12 B-2 保留的 6 条（B1–B4、B10 与其补充登记中的「系统侧刷新频率」）在我职责内属**转述性**条目，其「需外部信息」标记与 OPENQ-W2/W3 报告的未确认清单一致，判定条件成立，不需要新的运行时证据来确认「当前不可判定」。

## G. 行号漂移统计

- 任务 A 全量机械检查：**585 条展开引用 / 512 唯一组合 / 76 个文件 —— 越界 0、文件缺失 0**。
- 内容级抽验（≈250 条）：**行号正确率 ≈ 99.2%**（错 2 条，均为 1 行级偏差：E1 的 `:224` 表述、E2 的 `:20551`）。
- 未发现 GBK 型整体漂移：范围内全部抽验行号均按 UTF-8 口径命中；**唯一一次疑似漂移（§13.4 的 15/52）经独立复现证伪了「疑似」——文档正确**。
- 历史自报口径复算：R1 合计 772/50/43/29/650 与我逐份加和**完全一致**；R2/R3/R4/R5/R6 的自报值**无一不符**。

## H. 复核到的行号抽样（证明真的打开过）

| 引用 | 源码实测摘录 | 出处 |
| --- | --- | --- |
| `src/lib/rollout.js:4210` | `const FORK_REPLAY_GAP_MS = 500;` | §11 |
| `src/lib/pricing/index.js:186` | `const DEEPSEEK_WEEKEND_OFF_PEAK_FROM_MS = Date.UTC(2026, 7, 22, 16, 0, 0);` | §11 |
| `src/lib/fs.js:70` | `const LOCK_HEARTBEAT_MS = 30 * 1000;` | §11 |
| `src/lib/cursor-store.js:21` | `const DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024;` | §11 |
| `src/lib/upload-throttle.js:123` | `backoffStep: Math.min(20, (s.backoffStep \|\| 0) + 1),` | §11 |
| `migrations/20261005120000_cloud-usage-archive.sql:213` | `IF v_count>10000 OR pg_column_size(v_payload)>8388608 THEN RAISE EXCEPTION 'Archive day exceeds the bounded pack budget'; END IF;` | §11 |
| `src/commands/sync.js:3424` | `const skipNoOpCursorCommit =`（13 项条件起点） | §12 C1 |
| `src/commands/sync.js:3709` | `}`（`cmdSync` 收尾；`:3711 function parseArgs(argv) {`） | §12 C1 |
| `src/lib/rollout.js:489` | `const inode = st.ino \|\| 0;` | §12 C11 |
| `bin/tracker.js:36` | `if (applyResult && applyResult.unprotected === true) {` | §12 C8 |
| `src/lib/rollout.js:2291` | `const endOffset = st.size;`（3 处之一；函数头 `:2264 parseRolloutFile`） | §12 A1 |
| `src/lib/rollout.js:6648` | `usage_precision: base.usage_precision \|\| undefined,` | §12 A11 / 附录 B |
| `src/lib/rollout.js:17011` | `delta[key] = Math.max(0, current[key] - toNonNegativeInt(previous[key]));` | §12 C25 |
| `src/lib/rollout.js:20552` | `recordsProcessed: eventsAggregated,` | §13.3（**E2**） |
| `src/lib/local-api.js:224` | `return hs.slice(0, 10);` | §11（**E1**） |
| `src/lib/local-api.js:3147` | `const day = url.searchParams.get("day") \|\| new Date().toISOString().slice(0, 10);` | §12 C22 |
| `src/lib/passive-mode.js:88-90` | `@param {Object} opts.hookStatus - ... shape: { claude, gemini, codex, every_code, opencode, openclaw, codebuddy, workbuddy, grok }` | §12 C13 |
| `src/lib/codex-service-tier.js:22` | `// Observed values on the local corpus are "default" (814) and "priority" (58).` | §12 补充登记（**I2**） |
| `src/lib/codex-service-tier.js:8-10` | `...on a 1213-file local corpus only 167 files carried the event, covering 25% of token_count events.` | §12 E5 |
| `src/lib/rollout.js:16045` | `seenArr.length > 10_000 ? seenArr.slice(seenArr.length - 10_000) : seenArr;` | §11 |
| `src/lib/rollout.js:17376-17377` | `COPILOT_USAGE_CLAIM_RETENTION_MS = 7 * 24 * 60 * 60 * 1000;` / `..._MAX_ENTRIES = 10_000;` | §11 |
| `src/lib/rollout.js:1821` | ``return `gen-${Date.now()}-${crypto.randomUUID().slice(0, 8)}`;`` | 附录 B |
| `src/lib/pricing/matcher.js:3-12` | `// Resolve order: 0. CURATED source exact ... 7. null` | 附录 B |
| `src/lib/scan-roots.js:107-108` | `if (code === "ENOENT" \|\| code === "ENOTDIR") return { exists: false, error: null }; / return { exists: false, error: code };` | 附录 B |
| `src/commands/sync.js:6579` | `setMaxTotal(merged, key, total);`（5 个调用点之一；定义 `:6585`） | §13.7 |
| `src/commands/sync.js:1152` | `// scoped sync queued, so rebuilding without it would erase that history.` | 附录 B（deferred） |
| `TokenTrackerWin/TrayApplicationContext.cs:84` | `private readonly System.Windows.Forms.Timer _syncTimer = new() { Interval = 5 * 60 * 1000 };` | §12 B7 |
| `src/commands/serve.js:304` | `if (normalizedShell !== "windows") return null;` | §12 B7 |
| `migrations/20260817120000_account-session-states.sql:215-220` | `SELECT DISTINCT ON (h.user_id, COALESCE(dm.machine_cluster_id, h.device_id::text), h.source, h.model, h.hour_start)` | §12 B9 |
| `test/edge-pricing-parity.test.js:76` | `test("MODEL_PRICING + getModelPricing are byte-identical across all 5 edge files", () => {` | §12 C16 |

## I. 方法与自证

- 引用提取脚本对 `file:1,2-3,4` 复合形式逐项展开；短引用（`:NNN`）按「同 tick 最近前向文件 → 前文最近文件 → 范围命中优先」归属，初筛报出的 10 条越界全部逐条回源裁定（A.3 表）。
- 附录 A 用与文档声明**相同的正则**但**独立实现**重扫，未复用文档表格数据；45 项起始行/覆盖行数逐行比对。
- §13.4 的 GBK 漂移用 `[System.Text.Encoding]::GetEncoding(936)` 全 `src/` 逐文件复现（20 个文件漂移、累计 52 行、`rollout.js` 15 行），与文档逐字一致。
- 全程未写入样本仓库与主文档；`%TEMP%\r75\` 下脚本与中间数据用完即弃。
