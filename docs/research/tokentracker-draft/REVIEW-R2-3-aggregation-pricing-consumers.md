# 复核报告 R2-3：S4 / S5 / S6（第二轮）

- 复核对象（只读）：
  - `docs/research/tokentracker-draft/S4-aggregation-query-dedup.md`（1456 行）
  - `docs/research/tokentracker-draft/S5-pricing-and-cost.md`（1435 行）
  - `docs/research/tokentracker-draft/S6-consumers-output.md`（1038 行）
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14），全程只读，未修改样本仓与分片任何一个字节。
- 行号口径：`[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` 或 `read` 工具；禁止裸 `Get-Content`（本机对该仓库确有少算，如 `local-api.js` 实为 3543 行）。
- 与 R1 的关系：R1 已抽验的 157 条（E 报告附录）+ S5/S6 清单**不重复**；本轮 A 组核对的是两份「修正记录（R1 复核后）」的落实，B 组抽取的是 R1 较少触及的章节（S4 §2/§6/§7/§8/§9、S5 §2.4/§3/§5.3/§8、S6 §2/§3/§5/§7/§8）。
- 计数类断言一律由本轮**独立重算**（正则/JSON.parse/Node 探针），不看分片自报。

## 结论

1. **修正记录整体可信**：A 组 49 条逐条复查，48 条与源码逐字/逐值一致（含 119/64 键、44/9 差集、74/70/66 计数、9 处/7 文件 savings、27 端点、全部行数断言），**只有 1 条措辞需限定、1 条数字口径未写明、1 条疑似残留字符**（详见 E/I）。
2. **本轮最重的独立发现**（R1 未触及）：**「不带 tz 时回退 UTC」只在 `rowDayKey` 一族成立**；`getZonedParts` 的第三级回退是**运行环境本机时区**，而被 `aggregateHourlyByDay`（hourly 小时键）、`summary/heatmap` 的 `todayStr`、`night_owl` 徽章**无条件调用**。=> 非 UTC 机器 + 无 tz 时，同一份数据在"日聚合"与"小时键/今天"两条路径上的日基准不同（E1）。
3. **端到端链条**（C 组 10 处）未发现金额重复计入或漏计的实证；但发现 hourly 端点的 `day` 默认值（UTC 今天）与其过滤谓词（当地日键）**基准不同**，以及多端点 `pricing.source` 元数据是硬编码标签而非真实来源。

## A. 修正点复查（逐条）

判定符号：✔ 与源码一致；△ 部分/措辞需限定；✘ 与源码不符。

### A-S4（18 条）

| # | 修正点 | 判定 | 本轮核到的源码（摘录） |
|---|---|---|---|
| A1 | E1 `conversation_count` = 主会话 user 文本消息；注释与实现矛盾 | ✔ | `rollout.js:2807-2809` "Count user-typed messages as conversations (main sessions only). Exclude tool_result…"；`:2810` `isMainSession && line.includes('"type":"user"')`；`:2819-2821` `hasText`；`:2833` `userBucket.totals.conversation_count += 1;`；`rollout.js:17232-17235` "NOTE: this differs from omp/Claude which **count one-per-assistant-message**… Craft's are per-session"。三处全对，"以代码为准"成立 |
| A2 | E2 `claudeMessageDedupKey` 退化为纯 msgId | ✔ | `rollout.js:4914-4918` `if (!msgId) return null; … return reqId ? \`${msgId}:${reqId}\` : msgId;`；`sync.js:227-233` v4 注释 "v3's ground-truth scan itself used `if (msgId && reqId)` … silently disabled dedup" |
| A3 | E3 `toUtcHalfHourStart` 计数：rollout 70 出现/66 行、sync 1、categorizer 3、全 src 74、test 10 | ✔ | 本轮正则 `toUtcHalfHourStart\(` 重算：rollout 70/66（多出现行 `:20214`×3、`:20265`×2、`:20418`×2）、sync 1（`:6663`）、claude-categorizer 3、src 合计 **74**、test 10（sync-droid 8 + trae-entitlement 2）；定义 2（`:4173`、categorizer `:1175`） |
| A4 | E4 usage-hourly 为"当地小时 24 整点、无 `:30` 键" | ✔（表述见 I2） | `local-api.js:609` `const hourKey = \`${dayKey}T${String(parts.hour).padStart(2,"0")}:00:00\`;`；`:600-608` 按 tz 日键过滤后合并；响应 `Array.from(byHour.values())`（`:639`）——**无预填充** |
| A5 | E5 `20031` 归属 `rollout.js` | ✔ | `rollout.js:20031` `await fs.appendFile(queuePath, \`${lines.join("\n")}\n\`, "utf8");`；`upload-queue.js` 实测 134 行（不可能有 20031 行） |
| A6 | E6 heatmap-compact.test.js = 15 用例 | ✔ | 重算：行首 `test(` 15、任意位置 15、`test.each` 0、文件 217 行 |
| A7 | E7 model-breakdown.test.js = 25 用例 | ✔ | 重算：行首 25、任意 27、文件 888 行 |
| A8 | E8 开关行号 `:4884`（非 `:4894`） | ✔ | `sync.js:4884` `async function dropZcodeQueueRows(filePath, { retainRetractions = false } = {}) {`；`:4894` `let removed = 0;` |
| A9 | I1 聚合链分叉（仅 summary/daily/heatmap 走 aggregateByDay） | ✔ | `aggregateByDay` 定义 `:227`，端点内调用点：summary `:2363`、daily `:2430`、heatmap `:2537`（`:248` 是缓存重建递归）；hourly `:3150`→`aggregateHourlyByDay:600`；model-breakdown `:2602-2641`、monthly `:3161-3186`、project-summary `:2744-2798`、project-detail `:2862-2898` 各自建 Map |
| A10 | I2 `parseBucketKey` 15 处调用 + 定义 1 | ✔ | 16 次出现=15 调用+定义 `:4238`；调用行号与分片列表**逐字一致**（`:1973/:3174/:3199/:3223/:3238/:3434/:3570/:3636/:3655/:7212/:12286/:13378/:14251/:16376/:20776`） |
| A11 | I3 500ms 窗判定实现 `:2588-2601` | ✔ | `:2588` `>= FORK_REPLAY_GAP_MS` 重置；`:2591-2594` `<` 置 `forkedReplaySkip`；`:2601` skip 点 |
| A12 | I4 守卫注释引 `sync.js:264` | ✔ | `:264` "corrected values overwrite the cloud. **GUARDED: skips if any codex session**…"（整段 `:258-267`） |
| A13 | I5 `upload-queue.js` 代码块改 `:112-123` | ✔ | `:111` `await output.sync();`、`:112` `await output.close();`、`:113` `output = null;` … `:123` `return { compacted: true, … }` |
| A14 | I6 `PRIORITY_SUBSET_FIELDS` 在 `totalsKey` 内、`usage_precision` 在调用点 | ✔ | `:4155-4171` `totalsKey`（`:4169` 拼优先子集）；`:3404-3407` `usagePrecision` + `outputModel` 拼接 |
| A15 | T1 两套口径并存；MAX 仅用于 codex rescan 守卫 | ✔（行号范围见 I1） | `mergeMaxTotals` 调用点仅 `:5848/:5852/:5856/:5860`；`setMaxTotal` 非 merge 调用点 `:6492/:6512/:6540/:6559` 全在上述 helper 内，helper 唯一外部调用者即 `:5849-5862`；`:5874-5879` `… skipping to avoid data loss` + `return false` |
| A16 | T2 三份 UTC 桶语义等价 | ✔ | `rollout.js:4173-4190`、`claude-categorizer.js:1175-1191`（同 `Date.UTC` 结构）、`sync.js:4538-4549`（毫秒版 `Math.floor(millis/1_800_000)*1_800_000`） |
| A17 | T3 repair 数字 2.17B 自洽、0.26.4 回 v4、key 仍 v4 | ✔ | `sync.js:239-240` "wiped 2.17B claude tokens (-1.27B opus-4-7, -474M opus-4-6, -376M sonnet-4-5, -48M haiku, -6M sonnet-4-6)"（求和 2174M）；`:242` "0.26.4 HALTS back at v4"；`:248` `…REPAIR_KEY = "claudeGroundTruthRepair_2026_05_v4";` |
| A18 | T4 项目身份两条（URL→owner/repo；cwd→basename） | ✔ | `rollout.js:4429-4442` 只吃 URL、`segments.join("/")`；`session-analytics.js:405-408` `path.basename(...)`；`.git` 不剥离（无剥离逻辑） |

### A-S5（17 条）

| # | 修正点 | 判定 | 本轮核到的源码/重算 |
|---|---|---|---|
| A19 | R1-1 `MODEL_PRICING` 119 键、IFLYTEK 64 键（184=? 183） | ✔ | 逐块清点 `tokentracker-leaderboard-refresh.ts`：`:142-332` = **119**、`:339-404` = **64**、合计 **183**；块外无其它同形态键行 |
| A20 | R1-2 `index.js:283-284` | ✔ | `:282` 行末 "They are"、`:283` "annotations, never addends — total_tokens stays the sum of the base columns"、`:284` "whether or not they are present…" |
| A21 | R1-3 `index.js:282-283`（原 `:288-295`） | ✔ | `:287-289` 实为 "The parser records only usage from requests whose cache-inclusive input exceeded 272K" |
| A22 | R1-4 `local-api.js:614` | ✔ | `:614` `billable_total_tokens: 0,`、`:615` `input_tokens: 0,` |
| A23 | R1-5 `pricing.test.js` = 1392 行 | ✔ | ReadAllLines = 1392 |
| A24 | R1-6 `subscriptions.js` = 508 行 | ✔ | ReadAllLines = 508 |
| A25 | R1-7 parity 与 curated 的对照 = 6 处 | ✔（口径见 I4） | `edge-pricing-parity.test.js`：`:172-173` acode 全表 deepEqual、`:175` `source_alias?.acode === undefined`、`:208`/`:209` xsparkx2/xopglm53；另 `model-breakdown.test.js`、`grok-parser.test.js` 侧 |
| A26 | R1-8 edge-only 44 键 / curated-only 9 键 | ✔ | Node 双向差集：edge 119 vs `curated.exact` 84 → **edge-only 44**、**curated-only 9**，9 键与分片列表逐字一致 |
| A27 | R1-9 `ensurePricingLoaded` 生产两处 + 27 命中 | ✔（口径见 I3） | 生产调用：`serve.js:127`、`local-api.js:3340`；测试：pricing.test.js 16 行调用（另有 2 行测试名）、queue-cache `:149/:220`、grok `:301`、omo `:229` |
| A28 | R1-10 生产路径 + `dashboard/src/` 零副本，vite 例外 | ✔ | `dashboard/vite.config.js:508-514` `createRequire(import.meta.url)` → `require(path.resolve(REPO_ROOT,"src/lib/pricing"))`，取 `getModelPricing`/`computeRowCost`；`:1052-1057` mock 对聚合后 totals 只计一次价 |
| A29 | R1-11 `local-api.js:60` import 解构 / `:3530` module.exports | ✔ | `:57-64` 解构块（`:60` = `computeRowCost,`）；`:3528-3531` 导出块（`:3530` = `computeRowCost,`） |
| A30 | R1-12 edge-patches 28 文件（全 .ts） | ✔ | `Get-ChildItem -File` = 28，无非 `.ts` |
| A31 | B9 `:76-85` 仅 edge↔edge、无 edge⊇curated 断言 | ✔ | `:76-85` 循环 `MIRRORS` 与 canonical `strictEqual`；全文 420 行无 `curated.exact` 全表包含断言 |
| A32 | B9 量化 grok 两链 | ✔ | `:273` `"grok-build": { input: 1.25, output: 2.50, cache_read: 0.20 }`、`:481` `if (lower.includes("grok-build")) return MODEL_PRICING["grok-build"];`、`:480` `grok-4.5` → `cursor-grok-4.5` |
| A33 | B10 `index.js:31` 定义、`:375` 导出、零内部读取 | ✔ | 全仓 grep `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 仅 5 命中：index.js `:31`/`:375`、`codex-rollout-parser.js:51`/`:733`、`session-analytics.js:245`（注释） |
| A34 | B16 逐行计价点 `local-api.js:2634-2640` | ✔ | `:2634-2639` 注释 "computeRowCost is not linear… Summing the base columns first and pricing once silently dropped every premium"；`:2640` `ma.totals.total_cost_usd = Number(...)+ computeRowCost(row);` |
| A35 | 行数断言 378/439/775/3543/2934 | ✔ | ReadAllLines 全部逐字命中 |

### A-S6（14 条）

| # | 修正点 | 判定 | 本轮核到的源码/重算 |
|---|---|---|---|
| A36 | R1-1 savings 9 处 / 7 文件，`amount_saved\|saved_amount\|total_saved` 0 命中 | ✔ | 大小写敏感重算：`CloudPage.jsx:95/:195`、`i18n/zh:650`、`zh-TW:709`、`copy.csv:1699`、`DESIGN.md:17`、`hosted-archive-acceptance.cjs:377/:450`、`cloud-usage-archive.md:70` = 9 行 / 7 文件；另一正则 = 0 |
| A37 | R1-2 edge-patches 28 文件 | ✔ | 同 A30 |
| A38 | R1-3 九个 edge 文件行数（954/894/391/956/1449/1074/210/183/164） | ✔ | ReadAllLines 逐条命中 |
| A39 | R1-4 `init.js` 2427 / `sync.js` 7251 | ✔ | `src/commands/init.js` = 2427（注意实际路径在 `src/commands/`）、`src/commands/sync.js` = 7251 |
| A40 | R1-5 14 项行数（抽 5 项） | ✔ | status.test.js 725、local-api-security.test.js 1561、local-api-account-view.test.js 1386、badge-svg-render.test.js 147、account-source-parity.test.js 107 |
| A41 | R1-6 parity 测试 420 行 | ✔ | ReadAllLines = 420 |
| A42 | R1-7 badge-svg 295 / embed-svg 320 / StatusBarController 1507 / DashboardViewModel 1151 / DashboardWindow.cs 1235 | ✔ | `dashboard/edge-patches/tokentracker-badge-svg.ts` 295、`-embed-svg.ts` 320、`TokenTrackerBar/TokenTrackerBar/Services/StatusBarController.swift` 1507、`…/ViewModels/DashboardViewModel.swift` 1151、`TokenTrackerWin/DashboardWindow.cs` 1235 |
| A43 | R1-8（I5）前提：不带 tz 回落 UTC、前端一直传 tz | △ | `local-api.js:512-519` `getTimeZoneContext` ✓、`api.ts:244-249` `buildTimeZoneParams` ✓；但"回落 UTC"仅对 `rowDayKey` 成立（**E1**） |
| A44 | B14 wrapped 全 UTC（`:37-53`/`:66-67`/`:148-152`/`:170`） | ✔ | `:37-53` `rowDay/rowYear/isoHour`（`slice(0,10)`、`slice(0,4)`、`/T(\d{2}):/`）；`:66-67` "Restrict to a single calendar year (UTC)."；`:148-152` `stepDay` 用 `setUTCDate`；`:170` "// Peak hour-of-day (UTC)." |
| A45 | B15 `local-api.js:2588` 硬编码 `streak_days: 0` | ✔ | `:2587` `active_days: cells.filter(…)`、`:2588` `streak_days: 0,`；Windows 侧 `UsagePoller.cs:314` 注释 "streak is server-computed"、`:328` 读 `streak_days` |
| A46 | 27 个端点 | ✔ | 自建探针从 `local-api.js` 抽 `"/functions/tokentracker-…"` 去重 = **27**（raw 27） |
| A47 | 上传字段 `{upload_id, hourly, account_session_states?}`；ingest 多出 `billable_total_tokens`/`total_cost_usd` | ✔ | `sync.js:4367-4374` 逐字；`ingest.ts:188-189`、`:193` `conversations: b.conversation_count ?? b.conversations ?? 0` |
| A48 | `status.js` 非用量报表（0 命中） | ✔（沿用 R1，本轮抽 `status-json-light.test.js` 96 行佐证） | 分片结论与 `test/status-json-light.test.js`（96 行）形状一致 |
| A49 | vite dev 是否复制价格表 → 结案 | ✔ | 见 A28；另 `:1067` dev mock 的 `pricing.source` 硬编码 `"default"`（对比本地 `local-api.js:2658` 硬编码 `"litellm"`） |

## B. 独立新抽样（46 条，避开 R1 已抽章节）

抽样统计：A 组 49 条（S4 18 / S5 17 / S6 14）、B 组 46 条（S4 18 / S5 16 / S6 12）、C 组 10 处；计数类断言重算 21 组；新增判定 E2 / I8 / U1·U6。

### B-S4（18 条）

| # | 分片位置 | 源码核对（摘录） | 判定 |
|---|---|---|---|
| B1 | §2.1 代码块 `rollout.js:4173-4190` | 逐字一致（`getUTCMinutes()`、`minutes >= 30 ? 30 : 0`、`Date.UTC(...)`、`bucketStart.toISOString()`） | ✔ |
| B2 | §2.2 四键函数位置 `:4222/:4228/:4233/:4238` | `:4222` `bucketKey`、`:4228` `projectBucketKey`、`:4233` `groupBucketKey`、`:4238` `parseBucketKey`；常量 `:55-57` `DEFAULT_SOURCE="codex"/DEFAULT_MODEL="unknown"/BUCKET_SEPARATOR="|"` | ✔ |
| B3 | §2.2 归一化 `:4254-4264` | `normalizeSourceInput` 小写化、`normalizeModelInput` **只 trim 不降大小写**——分片"不对称"描述成立 | ✔ |
| B4 | §2.2 项目键另有解析器 | `sync.js:6422` `function projectUsageKeyFromFields({ projectKey, source, hourStart })` | ✔ |
| B5 | §2.3 rollout 覆盖示例 `:1007/:2111/:2360/:2605/:2867/:2975` | 六行**全部**是 `toUtcHalfHourStart(...)` 调用；`:17462` `function copilotOtelAggregateKey(model, bucketStart)` | ✔ |
| B6 | §2.3 grok 第三实现 | `sync.js:4538-4549`；`:4542` `? value < 10_000_000_000`（秒级兼容） | ✔ |
| B7 | §2.4 `getZonedParts` 描述（IANA + LRU16 + 失败固定偏移） | `:521-552` IANA formatter + `boundedCacheSet(..., MAX_TIME_ZONE_CACHE_ENTRIES)`；`:96` `MAX_TIME_ZONE_CACHE_ENTRIES = 16`；`:542-546` 无效时区也缓存 `null`；`:573-583` 偏移分支按**分钟**换算 | ✔（补充：`:585-592` 第三级回退为本机时区，见 E1） |
| B8 | §6.1 `projectKey` basename | `session-analytics.js:405-408` 逐字；`isWorktreeRef` `:401-403` 排除 `worktrees|subagents` | ✔ |
| B9 | §6.2 `deriveProjectKeyFromRef` + 状态码 | `rollout.js:4444-4461`：`blocked` + `reason ∈ {unparseable_ref, missing_ref}`、成功 `public_verified` | ✔ |
| B10 | §6.5 三函数体（分片自标"函数体未逐行读取"） | **本轮已读全**：`:2343-2354` `sessionUsageDay`（IANA → offsetMinutes → UTC）、`:2356-2362` `withinDayRange`（区间相交）、`:2364-2383` `scopeSessionUsage`（按 `usage_buckets` 日键过滤 + `repriceSessionRecord` + 保留 `lifetime_*`）。=> 该处 U 可解除 | ✔（U 解除） |
| B11 | §6.6 项目队列消费者/字段 | `local-api.js:2837-2858` emptyMeasures 八字段含 `conversation_count`；`:2900-2915` 返回 `{generated_at,from,to,project_key,project_ref,totals,days_active,range_total_tokens,daily[],sources[]}`；`:2877-2883` 注释明写 `models` 按 source | ✔ |
| B12 | §7.2 hourl y 参数/返回 | `:3146-3152`：`day`（默认 `new Date().toISOString().slice(0,10)`）、`tz*`、`scope`；`{day, scope, excluded_sources, data}` | ✔（默认值基准问题见 E1/C3） |
| B13 | §7.2 monthly 参数/返回 | `:3156-3187`：`from/to/tz*/scope`，`month = day.slice(0,7)`，项含九字段 + `models{}`，`sort(month)` | ✔ |
| B14 | §7.2 category-breakdown 分支 | `:2668-2698`：`source`（默认 claude）、claude 失败 → 500 + `unsupportedCategoryPayload`；codex 总量 0 → `buildCodexCategoryFallbackFromQueue(await readQueueData(qp), …)` | ✔ |
| B15 | §7.3 分页两支 | sessions `:2504-2505`（上限 2000）；project-summary `:2737-2740` + `:95`（`PROJECT_USAGE_MAX_ENTRIES = 10`） | ✔ |
| B16 | §7.4 排序 8 项 | `:299` day 升序、`:3187` month 升序、`:2650` model 按 token 降序、`:2785` 项目按 billable 降序、`:2913` 项目 sources 按 token 降序、`:435-439` 徽章按 `hour_start` 升序、`context-health.js:111`、`account-usage-dedup.js:176-181`（R1 已核） | ✔ |
| B17 | §7.6 缓存表（rowDayKey 成本注释） | `:115-117` `timeZoneContextKey`（含 `p{getPricingRevision()}`）、`:104-105` WeakMap/Map、`:26-27` 账户退避 2 分钟、`:45-48` 头像 LRU | ✔ |
| B18 | §8.3 night_owl 用本地时区小时；§9 用例数 | `:478-486` `getZonedParts(...)` + `parts.hour >= 6` 跳过；`LOCAL_BADGE_THRESHOLDS.night_owl = [5,20,60,150]`（`:419-423`）。§9 用例数重算：2/9/15/3/16/15/14/2/12/5 = 与分片一致；`billable-total-tokens-migration.test.js` 9 行 0 顶层用例 | ✔ |

### B-S5（16 条）

| # | 分片位置 | 源码核对（摘录） | 判定 |
|---|---|---|---|
| B19 | §2.4 四级降级链 | `litellm-fetcher.js:107-111` 注释（1 disk cache / 2 upstream / 3 stale cache / 4 seed）；实现 `:128-137` `"disk-cache"`、`:139-146` `"upstream"`、`:148-158` `"stale-cache"`、`:160-163` `"seed-snapshot"` | ✔ |
| B20 | §2.4 超时/TTL/缓存形状 | `:12-13` `24h` / `10_000`；`:68-105` 裁剪 4 字段 + `_meta{source,cached_at,kept_models}`；`:132`/`:162` `delete ._meta`；无重试/无 ETag（源码确实没有） | ✔ |
| B21 | §2.5 加载与路径 | `index.js:69-71` `~/.tokentracker/cache/pricing.json`；`:73-94` 幂等+共享 promise+`revision += 1`+`negativeCache.clear()`；`tracker-paths.js:4-12`；seed 同步加载 `:44-55`、`:61-63` `"seed-snapshot:sync"` | ✔ |
| B22 | §3.1 解析顺序 11 行行号表 | 11/11 命中：空值守卫 `:253-255`、早期特例 `:265-279`、0 `:280-286`、1 `:291-300`、2 `:302-313`、3 `:315-322`、4 `:331-343`、5 `:345-351`、5b `:361-370`、6 `:372-383`、7 `:385`；返回标签 `curated:source-exact`/`litellm:exact-dot`/`litellm:prefix-strip`/`litellm:fuzzy` 等逐一证实 | ✔ |
| B23 | §3.2 (a)(b)(c) | `:226-234` `lookupExactCaseInsensitive` 线性扫描（无小写索引）；`:219-224` `buildDotRestoredModel` 仅 `(\d+)-(\d+)` → `$1.$2`；注释 `:324-330`；Qwen 正则 `:291` | ✔ |
| B24 | §3.2(d)/§3.3 归一化器 | `:89-95` Claude 去前缀（`model.lastIndexOf("/")`）；`:33-58` antigravity（Gemini 3.x→2.5 映射 `:49-51`、gpt-oss→`antigravity-gpt-oss-120b` `:55`）；`:126-150` cursor（保留 fast、Grok 4.5 分流 `:138-140`）；`:68-81` zed（不剥 fast）；`:161-166` workbuddy（`auto`→`hy3-preview-agent`）；`:182-189` unsloth 哨兵 | ✔ |
| B25 | §3.3 归一化表 `:194-204` | 逐字一致（9 个键，`pi-anthropic`/`prime-agent-anthropic` 复用 Claude 归一化器） | ✔ |
| B26 | §3.4 结果对象/未命中 | `{hit:true,source,value}` / `{hit:false,source:"empty"}` / `{hit:false,source:"miss"}`；`index.js:112-132` 未命中入 `negativeCache` | ✔ |
| B27 | §5.3 curated 计数（84 条 / 缺 cache_write 41 / 缺 cache_read 0） | Node 重算：`curated.exact` 84 条；缺 `cache_write` **41**、缺 `cache_read` **0**、缺 input/output 0 | ✔ |
| B28 | §5.3 seed 计数（2284 / 2274 / 2273 / 566 / 188） | Node 重算（口径=排除 `_meta`，含 `sample_spec`）：2284；input 2274、output 2273、cache_read **566**、cache_write **188**；75%/92% 亦对（566/2284=24.8%、188/2284=8.2%）。**分片四个数字逐字命中** | ✔ |
| B29 | §5.3 `source_exact.acode` 64 条全带 4 字段 | JSON 结构：`source_exact.acode` 64 键；`curated.exact` 的 84 键中仅 43 键同时有 cache_read+cache_write | ✔ |
| B30 | §8.1 wrapped 不涉及金额 | `wrapped-aggregator.js` 236 行；`:21-23` 唯一 require 是 `./local-api` 的 `normalizeQueueRow`；`:116-119` 四个 Map；`:212-218` totals 无金额 | ✔ |
| B31 | §8.2 quality-per-dollar | `outcomes-engine.js:24` require `computeRowCost`、`:207` 逐行计价、`:154` `quality_per_dollar`、`:156-157` effective tokens/cost、`:208` 用 `total_tokens`；`docs/quality-per-dollar.md` 67 行 | ✔ |
| B32 | §8.3 订阅节省不存在 | `subscriptions.js` 508 行、全文 `cost|price` **0 命中**；`subscription-manager.js` 293 行 | ✔ |
| B33 | §8.4 usage-limits 不用成本 | `usage-limits.js` **4520** 行；`:165` `used_percent: pct`、`:348-352`、`:1484-1486` 正则 `█+\s*(\d+)%`、`:1910-1911` `usedPercent = 100 - percentRemaining`；`:693`/`:4105` 仅注释 | ✔ |
| B34 | §8.3(853 附近) 榜单金额 | `leaderboard-refresh.ts:1181` `agg.estimated_cost_usd += computeRowCost(row)`、`:1349` `Math.round(...*100)/100`；`LeaderboardPage.jsx:817/:927/:978` 均带 `title="Based on estimated API pricing, not actual billing"`；`account-model-breakdown.ts:906` `ma.totalCostUsd += subscriptionBacked ? 0 : …` | ✔ |

### B-S6（12 条）

| # | 分片位置 | 源码核对（摘录） | 判定 |
|---|---|---|---|
| B35 | §2.3 通用参数 | `:2359-2364`/`:130-134` 日键闭区间；`:512` tz；`:115-117` 缓存键；`:398-401` `include_account_level=1 → "all"`；`source-metadata.js:1` `new Set(["cursor","trae-cn"])`、`:15-19` `normalizeUsageScope` | ✔（"3000" 残留见 E2） |
| B36 | §2.4.1 summary 返回 | `:2413-2420` 逐字：`from,to,days,scope,excluded_sources,totals{…total_cost_usd.toFixed(6)},rolling{last_7d{from,to,active_days,totals},last_30d{…avg_per_active_day}}`；`collectDays` `:2390-2397` 跳过空日 | ✔ |
| B37 | §2.4.2 daily 桶结构 | `:268-297` 桶含 `total_cost_usd` 逐行累加（`:285` `a.total_cost_usd += computeRowCost(row)`）与 `models{}` 按 `total_tokens`；`:2431` 返回 | ✔ |
| B38 | §2.4.8 wrapped 端点 | `:2330-2337` `year`、`aggregateWrapped(rows, year ? {year} : {})`、返回 `{scope, excluded_sources, …summary}` | ✔ |
| B39 | §3.1/§3.2 API 层 | `api.ts:141-158` PATHS 17 键、`:166-173` 6 条 usage→account 映射、`:191-199` 云端路由 + `format=compact`、`:216-221` `LOCAL_API_UNAVAILABLE`、`:222-234` 同源 + `cache:"no-store"`、`:212` `expandHeatmapCompact` | ✔ |
| B40 | §3.3 pet 宿主 | `pet-linux-host.js:32` `ACCOUNT_QUERY = "account=1"`、`:194` heatmap `?weeks=52&tz…&account=1`、`:211` model-breakdown、`:223` limits、`:237` summary | ✔ |
| B41 | §3.4 活跃天数"后端优先" | `DashboardPage.jsx:819-823` `const serverActive = Number(heatmap?.active_days); if (Number.isFinite(serverActive)) return serverActive;` 之后才遍历 weeks | ✔ |
| B42 | §5.1/§5.2/§5.3 限额链 | `usage-limits.js:3976` `fetchUsageLimitsUncached`、`:4055` 18 个解构变量、`:64` TTL 2 分钟、`:59-63` 选择键 `devin-antigravity`、`:3954-3970` single-flight、`:118/:128/:134/:141` 各磁盘缓存文件名、`:4406-4440` data 17 家、`:4442-4461` provenance（stale/source/confidence/captured_at/age_seconds）、`:161` `buildWindow`、`:332-333` 18000/604800；抓取函数行号 8 项 **全部命中**（`:281/:601/:762/:927/:1251/:2008/:3666/:1954`） | ✔（选择键描述见 I5） |
| B43 | §6.1 上传批次 | `sync.js:4422-4442` 只收 bucket 行 + `account_session_state` 控制记录（注释说明 500 上限与批上限耦合）；`:4367-4374` 请求体；`ingest.ts:110-113` >500 → 400、`:139-152` 非 `trae-cn` fail-closed、`:153-160` states 含 `session_id` | ✔ |
| B44 | §7.2/§7.3/§7.4 三端壳 | `APIClient.swift` 11 项路径行号全中（`:43-52/:66-69/:73-75/:79-82/:86-90/:94-97/:101-103/:110-113/:119-121/:127-143/:148-150`）；`UsagePoller.cs:38-40` `HttpClient` `UseProxy=false`/6s、`:182-187`、`:226-250`、`:316-328`、`:340-354`；`extension.js:18-19` `17680`、`:229-245`、`:784` `Soup.Session({timeout:20})`、`:1025/:1046-1051/:1086/:1110-1116/:1137`；`vite.config.js:418/:570/:583-608/:610-639` | ✔ |
| B45 | §8.2 隐私逐项 | `telemetry.js:75-82` 恰 4 字段（`machine_hash/app_version/platform/shell`）、`:34` 命名空间、`:48-53` 三选一即关；`ingest.ts:188-189` 多出两字段；`local-api.js:2495-2499` "LOCAL ONLY… never proxied"、`:3137-3143`/`:31-41` 设备名 | ✔ |
| B46 | §9 测试表行数与 §2.10 端点表 | 12 项行数全中（96/133/41/250/165/135/219/102/138/339/126/28）；§2.10 端点行号 24 项全中（`:2358/:2425/:3146/:3156/:2533/:2596/:2668/:2330/:2441/:2526/:2470/:2500/:2731/:2828/:3482/:3240/:3470/:3294/:3192/:2137/:2166/:2940/:3137/:3121/:2982/:3074/:2920` + 非 `/functions/` 的 `:1670/:1681/:1767/:1792/:939/:1973-1976/:2038/:1698/:1724/:1747`） | ✔ |

## C. 端到端口径串联（10 处）

| # | 链条 | 本轮核到的衔接点 | 结论 |
|---|---|---|---|
| C1 | `conversation_count`：Claude user 消息 → 桶 → 队列行 → 读端 → 聚合 → 上传 → 云端列 | `rollout.js:2833`（+1，含 `claudeUserIdentity` 去重守卫 `:2825-2827`）→ 行字段 `:3409-3427` → `local-api.js:2375`（summary reduce）→ `sync.js:4369` → `ingest.ts:193` `conversations: b.conversation_count ?? b.conversations ?? 0` | 全链一致；口径混合（Craft 按会话/Claude 按 user 消息/按请求）已在 §4.8 声明；**补充**：Claude 侧 `conversation_count` 也受 `seenMessageHashes` 去重影响，分片未提 |
| C2 | `billable_total_tokens`：写入 → 读端兜底 → 聚合 → 上传 → 云端 | 行字段（`rollout.js:3411-3425` 写入九列）→ `local-api.js:284/625/2615/2849-2850` 一律 `row.billable_total_tokens ?? row.total_tokens ?? 0` → `ingest.ts:188` | 三处兜底口径一致，无重复/漏计实证 |
| C3 | **时区日键**：事件 UTC → 桶 → 端点日基准 | `rollout.js:4173`（UTC 桶）；`local-api.js:213-225` `rowDayKey`（无 tz → `hs.slice(0,10)` **UTC 日**）；同文件 `:606` `aggregateHourlyByDay` 与 `:2382`/`:2538` `todayStr`、`:482` night_owl **无条件**走 `getZonedParts` → `:585-592` **本机时区** | **不一致（E1）**：hourly 端点 `day` 默认是 UTC 今天（`:3147`），过滤谓词却是当地日键（`:608`）；`summary.rolling` 的窗口终点亦为本机时区"今天"，与日键（UTC）比较 |
| C4 | 成本：逐行计价 → 各聚合点 → 上传 → 云端榜单 | `local-api.js:285`（daily）、`:2640`（model-breakdown）、`:3381`（skills）均为 `computeRowCost(row)` 逐行；`:2842/:2652` 只对已算好的行求和；`ingest.ts:189`；`leaderboard-refresh.ts:1181/:1349` | 未发现"先加 token 再计价"的重复径路；唯一同类写法在 dev mock（`vite.config.js:1052-1057`，仅 dev） |
| C5 | 缓存价缺口 → 金额 | seed 仅 566/2284 带 `cache_read`、188/2284 带 `cache_write`；`matcher.js:407-412` 缺字段则不产出该键；`index.js` 端 `(pricing.x \|\| 0)`；edge 表 119 键（与 curated 差 44/9） | 同一行本地/云端金额可不同（分片 §7.4 B9 已量化）；本地缺失缓存价 = **$0**（不是 0.1×input） |
| C6 | `streak_days`：服务端 → 前端 → 原生壳 | `local-api.js:2588` 恒 0 → `DashboardPage`/`use-activity-heatmap.ts` 自行覆盖 → `UsagePoller.cs:328` 直读（本地模式得 0） | 三处衔接一致，已列为 U3（需实机） |
| C7 | 三种"日"并存：wrapped(UTC) / 热力图(请求 tz) / night_owl(本机 tz) | `wrapped-aggregator.js:37-53`、`local-api.js:2538-2544`、`:482` | 分片 §8.3 末句已披露前两者；**night_owl 无 tz 时落本机时区**（与 C3 同源） |
| C8 | 项目维度：写入 → 展示 → 上传边界 | `rollout.js:3515-3529`（无 model）+ `local-api.js:2837-2915` → `sync.js:3656` 只上传主队列（注释 "Only the main queue is uploaded"） | 项目数据永不出本机 ✓ |
| C9 | 价格修订 → 聚合缓存失效 | `index.js:108-110` `getPricingRevision` → `local-api.js:116` 缓存键含 `p{revision}`、`:246-248` 运行中变更则丢弃并重建、`:2642` model-breakdown 重入 | 不会发布"混修订"结果 ✓ |
| C10 | 展示层来源标注 | `local-api.js:2658` 硬编码 `pricing.source: "litellm"`、`vite.config.js:1067` 硬编码 `"default"`，而实际命中可能来自 `curated:*` | 元数据是**标签而非真实来源**（分片已如实引用，未声称其为真值；建议在 S6 §2.4.6 加一句限定） |

## 错误（E）/ 不完整（I）/ 无法验证（U）

### E（必须修）

**E1（高）无 tz 参数时，"日"的缺省基准在端点之间不一致——分片把它统一描述成 UTC。**
- 分片 S4 §2.4（:391）写「两者皆缺时**静默退化为 UTC 日**（`hs.slice(0, 10)`）」，S6 §0 第 2 条（:1020）写「本地 API 请求不带 tz 时回落 UTC」。
- 实测：`rowDayKey`（`local-api.js:213-225`）确有该守卫；但 `getZonedParts`（`:521-592`）的第三级回退是**运行环境本机时区**（`:585-592` `dt.getFullYear()/getMonth()/…` 本地 getter），而 `aggregateHourlyByDay:606`、summary `todayStr:2382`、heatmap `todayStr:2538`、`night_owl:482` 均**没有** `timeZoneContext && (timeZone || Number.isFinite(offsetMinutes))` 守卫。
- 即：非 UTC 机器 + 请求不带 tz 时，hourly 的行日键 = 本机时区日，而 `dayKey` 默认 = UTC 日（`:3147`）→ 两者可能整天不匹配（返回空 `data`）；`rolling`/热力图的"今天"也用本机时区。**两条路径基准不同是源码事实**；"返回空"是从谓词推出的后果（本轮只读源码，未起服务实测）。
- 修法：把 §2.4 与 S6 §0-2 的句子限定为"`rowDayKey`/按天聚合回落 UTC；`getZonedParts` 系（hourly 小时键、`todayStr`、night_owl）回落**运行环境本机时区**"，并把 hourly 端点的 `day` 默认值基准单独说明。
- **暴露面证据（本轮已核，非推断）**：macOS 壳的 heatmap 调用**不带 tz**——`APIClient.swift:73-75` 只构造 `URLQueryItem(name:"weeks", value:…)`；`StatusBarController/DashboardViewModel` 是唯一取数状态机。即：macOS/Linux 壳在非 UTC 机器上，`todayStr`（本机今天）与日键（UTC 日）会错位一天；Windows 壳的 `FetchHeatmapAsync` 则带 `tz…`（S6 §7.3 表），不受影响。这使 E1 从"理论不一致"变为**可触发的跨端差异**。

**E2（低）S6 §2.3 表格残留字符。** 第 334 行取值列以 "`3000`：默认 `all`；…" 开头——`3000` 不是任何合法 scope 取值，是残留字符串，需删。

### I（不完整/建议补）

- **I1** S4 T1 正文把守卫族标为 `:5848-5879`，但同句引用的第一处守卫是 `:5841-5846`（在区间外）；两处行号本身正确，建议改为 `:5813-5890` 或分别标注。
- **I2** S4 E4 的「24 个整点」易被读成"响应固定 24 项"。实测 `aggregateHourlyByDay` 返回 `Array.from(byHour.values())`（`:639`），**只含有数据的桶**；建议写"键空间为 24 个整点"。
- **I3** S5 R1-9 的「全仓 grep 27 命中」与同句列举的 22 行不自洽：`ensurePricingLoaded` 裸标识符命中 36 行，含 `ensurePricingLoaded(` 的 27 行（含 `index.js` 定义与 3 行注释）。建议写明正则口径。
- **I4** S5 R1-7 的「6 处」是跨文件口径（parity 内 4 处 + `model-breakdown.test.js`、`grok-parser.test.js` 各 1）；建议分开标注。
- **I5** S6 §5.1 写选择键「由 `{devinEnabled, ...}` 生成」，实测为 `devin-antigravity` 二元组（`usage-limits.js:59-63`，antigravity 由 `isAntigravityQuotaDisabled()` 决定）。
- **I6** S4 §2.2 断言 15 处 `parseBucketKey` 调用「全部落在主队列键空间」：本轮抽验 8 处上下文（`:1973` 在 `mergeOpenclawAttemptBuckets` 的 `attemptHourly.buckets`、`:3174` 在 `enqueueTouchedBuckets`、`:12286` cline、`:16376` pi、`:20776` antigravity 重建）均为 `hourlyState.buckets` 系；余 7 处仅按形态判断，建议标注"抽样确认"。
- **I7** S5 §2.5「`~/.tokentracker/` 下的 JSON 只有 `cache/pricing.json`（缓存）这一类派生物」字面不成立：仓库另定义了多份限额磁盘缓存（`usage-limits.js:118/128/134/141`，含 `claude-usage-rate-limit.json`）与 `outcomes.jsonl` 等；若限定为"价格类 JSON"，则成立。其落盘目录本轮未逐行确认（U6）。
- **I8** S6 §2.4.6 建议补一句：响应里的 `pricing.source: "litellm"` 是硬编码标签（`local-api.js:2658`），经 `curated:*` 命中时并不代表真实来源（C10）。

### U（无法验证/本轮未核）

- **U1** macOS `fetchHourly(day:)`、Linux 扩展 `-hourly?day=` 的 `day` 取值来源（是否当地日）未逐行核 → 与 E1 的暴露面相关。
- **U2** S6 §5.4 的 `LimitsPage.jsx:49/:59-66`、`usage-limits-provider-specs.js` 未核（限额展示层）。
- **U3** S6 §2.5.2 的 `session-analytics.js:2893-2905` provenance 结构未核。
- **U4** 分片自标未能确认项照旧：cloud-session/local-sync/proxy-*/subscription-manager/skills 的入参与返回。
- **U5** 原残留 U 保留：Windows 托盘 streak 实测（U3）、`curated` note 外部价页核对（U2）、`git-outcomes.js` revert 细节。
- **U6** 限额磁盘缓存的实际落盘目录（`~/.tokentracker/` 还是其子目录）未核。
- **解除**：S4 §6.5「三函数体未逐行读取」本轮已读全（B10），可解除该项 U。

## 复核到的行号抽样

本轮实际打开并逐行读取的源码位置（节选，全部命中，除 E1 指出的语义差异）：

- `rollout.js`：`55-57`、`1007`、`1966-1980`、`2111`、`2311-2360`、`2585-2605`、`2795-2860`、`3400-3412`、`4150-4192`、`4224-4270`、`4425-4465`、`4905-4925`、`16370-16382`、`17225-17250`、`17462`、`20025-20036`、`20768-20782`
- `local-api.js`：`26-45`、`88-120`、`210-260`、`260-300`、`396-404`、`415-445`、`476-492`、`505-525`、`560-640`、`2330-2435`、`2533-2560`、`2580-2660`、`2668-2700`、`2725-2800`、`2828-2932`、`3146-3200`、`3524-3532`
- `sync.js`：`220-252`、`255-270`、`4878-4900`、`4534-4552`、`5790-5900`、`6418-6426`、`6480-6570`、`6570-6595`、`4360-4445`
- 计价：`pricing/index.js`（`25-110`、`275-300`、`370-378`）、`matcher.js` 全文 439 行、`litellm-fetcher.js` 全文 172 行、`curated-overrides.json`（结构 + 双向差集）、`seed-snapshot.json`（键级统计）、`tracker-paths.js`、`outcomes-engine.js`（`20-28/150-160/200-212`）
- `session-analytics.js`：`395-415`、`2330-2415`
- `usage-limits.js`：`56-72`、`116-146`、`158-166`、`330-335`、`3970-3985`、`4050-4060`、`4404-4465`
- `wrapped-aggregator.js`、`telemetry.js`、`source-metadata.js`、`upload-queue.js:105-125`、`async-queue-reader`（R1 已核）
- edge/前端：`tokentracker-leaderboard-refresh.ts`（`136-160/320-345/396-412`）、`tokentracker-ingest.ts`（`105-165/182-195`）、`tokentracker-account-model-breakdown.ts:906`、`dashboard/src/lib/api.ts`（`138-200/210-266/745-775`）、`pet-linux-host.js:190-240`、`DashboardPage.jsx:811-855`、`LeaderboardPage.jsx:817/927/978`、`vite.config.js:500-520/1045-1070`、`APIClient.swift`、`UsagePoller.cs`、`extension.js`、`StatusBarController.swift`、`WidgetSnapshot.swift`（目录级行数）
- 测试：`edge-pricing-parity.test.js` 全文 420 行、`heatmap-compact.test.js`、`model-breakdown.test.js` 及 12 个文件的行数/用例数重算
