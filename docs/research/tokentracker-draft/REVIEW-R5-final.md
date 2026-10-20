# 最终验证报告 R5

- 验证对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md`（4323 行，主仓 HEAD `9ff5b69`，`git status` 对该目录为空 = 已提交）
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14），**全程只读**
- 行号口径：`[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；未使用裸 `Get-Content`
- 职责：独立验证最后 7 处修正 + 确定性抽样 + 最终状态检查。只报告，未修改样本仓与主文档任何字节

---

## 结论（是否可以定稿）

**可以定稿。** 7 处点名修正全部确认已写对；30 条确定性抽样 **30/30 命中（100.0%）**，无硬错；结构检查（96 个代码块全闭合、116 张表格列数全一致且均有分隔行、反引号奇偶零异常、无段落级重复）零缺陷；55 处 `§X.Y` 交叉引用全部命中现有标题。

遗留 4 条，**全部为措辞/锚定粒度级、非事实错误**：1 条措辞冲突（§7.5 一处短语与源码及同节表格抵触，见 J1）、1 条引用起点不统一（goose 两处，见 J2）、1 条引用范围略窄（opencode，见 J3）、1 条表格未声明所属文件（§9.6.2，见 J4）。任何一条都不需要回源重查或重做结论；若追求零瑕疵，J1 按建议删 8 个字即可闭环。

---

## A. 最后 7 处修正验证（逐条 / 判定 / 依据）

| # | 文档位置 | 现写法要点 | 回源实测（UTF-8 行号 + 摘录） | 判定 |
| --- | --- | --- | --- | --- |
| A1 | §7.5 守卫表（2381） | 「项目键集合守卫 `:5841-5846` … **不在 `mergeMaxTotals` 调用族内**；位于同一函数体 `:5823-5880` 之中」 | `:5841 if (missingProjectKeys.length > 0) {`；`:5843 \`[sync] codex rescan repair: project rebuild missed ${missingProjectKeys.length} existing codex project bucket(s) — skipping to avoid data loss\`,`；`:5845 return false;`；`:5848-5851 const existingProjectTotals = mergeMaxTotals(await projectUsageTotalsFromQueuePath(projectQueuePath, "codex"), projectUsageTotalsFromState(cursors.projectHourly, "codex"));`（`:5852/:5856/:5860` 为另三处 mergeMaxTotals）。块边界：`:5823 if (projectRepairEnabled) {` … `:5880 }` | ✓ |
| A2 | §7.5 四种安全模式段（2405） | 「保留全部既有行；对受影响旧键**追加零值撤回行**并**重排规范行**」 | `:3887 output.push(line);`（`JSON.parse` 失败分支原样保留）；`:3905 output.push(serialized);`；`:3925-3927 for (const key of legacyKeys.keys()) { const canonical = latestExplicitCanonicalRows.get(key) \|\| latestMigratedRows.get(key); if (canonical) output.push(canonical); }`；`:3929-3943` 逐旧键 push `{source:"deepseek", model, hour_start, input_tokens:0, cached_input_tokens:0, cache_creation_input_tokens:0, output_tokens:0, reasoning_output_tokens:0, total_tokens:0, billable_total_tokens:0, conversation_count:0}` | ✓（三要素全对；同段残留短语见 J1） |
| A3 | §9.1.2（2945） | 「路径常量在 `src/commands/status.js:199-208` 共 10 个」并列举 | `:199 config.json`、`:200 queue.jsonl`、`:201 queue.state.json`、`:202 cursors.json`、`:203 notify.signal`、`:204 openclaw.signal`、`:205 sync.throttle`、`:206 upload.throttle.json`、`:207 auto.retry.json`、`:208 sync.skip.json`——**恰 10 个，逐个同名同序** | ✓ |
| A4 | §7.3 热力图（2303 及 §10/§12 同口径） | 「`byDay` 在 `:2545` **构建**、单元格在 `:2564` **查表**并 `:2566` 构造」 | `:2545 const byDay = new Map(daily.map((d) => [d.day, d]));`；`:2564 const data = byDay.get(day);`；`:2566 cells.push({ day, total_tokens: data?.total_tokens \|\| 0, billable_total_tokens: billable, level: calcLevel(billable), models: data?.models \|\| null });` | ✓ |
| A5 | 五桶映射（1191） | goose `14674-14686` | `:14674-14677 const totalNow = Math.max(0, Number(row.accumulated_total_tokens ?? row.total_tokens ?? 0) \|\| 0);`；`:14678-14681` inputNow（`accumulated_input_tokens ?? input_tokens`）；`:14682-14685` outputNow；`:14686 if (totalNow === 0 && inputNow === 0 && outputNow === 0) continue;` | ✓ |
| A6 | §10 缓存写缺失表（1554） | qoder 旧库 `6173` 写 `cache_creation_input_tokens: 0` | `:6170-6173 return { input_tokens: input, cached_input_tokens: cachedInput, cache_creation_input_tokens: 0,` （`normalizeQoderTokens` 返回对象内，前有 `:6167 input = max(0, prompt − cached)`） | ✓ |
| A7 | §5 特殊支路（1039） | opencode 双支路结果在 `sync.js:1385-1386` 相加 | `:1385 return {`；`:1386 recordsProcessed: filesResult.filesProcessed + dbResult.messagesProcessed,`；`:1387 eventsAggregated: … + dbResult.eventsAggregated,`；`:1388 bucketsQueued: … + dbResult.bucketsQueued,` | ✓（起点正确；求和表达式实际跨 `:1386-1388`，见 J3） |

附注（A1 精度）：`:5823-5880` 严格说是 `projectRepairEnabled` 的 **if 块**范围，其所属函数为 `repairCodexRescanInflation`（`:5682` 起，`:6033` 收 `}`）。文档「位于同一函数体 `:5823-5880` 之中」的判定方向正确，无需改。

---

## B. 确定性随机抽样 30 条（逐条 / 判定 / 依据）

抽样方法（可复现）：正则 `[A-Za-z0-9_\-]+(?:[./\\][A-Za-z0-9_\-]+)*\.(js|ts|json|toml|md|sql|cjs|mjs|yml|yaml)[:：]\d+(?:-\d+)?` 抽取全文 `路径:行号` 引用 → 去重 930 条 → 按字符串升序排序 → 取索引 7、38、69…（步长 31）共 30 条（末条索引 906）。命中 = 该行内容支持文档在该处的断言（按引用所在文档上下文判定）。

| # | 抽样引用 | 文档锚点 | 源码实测摘录 | 判定 |
| --- | --- | --- | --- | --- |
| 1 | `CLAUDE.md:60-64` | 2053 | `:60 reasoning_output_tokens = reasoning tokens (Codex/acode/…/Cline fold them into output_tokens for cost)`；`:64 **Cost is computed from input_tokens + output_tokens + cached_input_tokens + cache_creation_input_tokens + reasoning_output_tokens only — never total_tokens**` | ✓ |
| 2 | `dashboard/src/lib/api.ts:62-73` | 3271 | `:62 function coalesceJsonGet(key, request)`、`:63-67 inFlightJsonGets.get/set(key, pending)`、`:71 pending.then(cleanup, cleanup)`；调用点 `:229 return coalesceJsonGet(url.toString(), async () => {` | ✓ |
| 3 | `docs/cloud-usage-archive.md:74-83` | 3548、4065 | `:74 ## Activation checks still pending`；`:83 Until these checks pass, do not enable a retention schedule or claim production cost reduction.` | ✓ |
| 4 | `matcher.js:112-115` → `src/lib/pricing/matcher.js` | 2717 | `:112-115 // Some relays invert tier/version order (\`claude-4.6-opus\` …). Restore it ONLY for major>=4 — Claude 3.x is genuinely version-first … and must stay untouched.` | ✓ |
| 5 | `rollout.js:7232` → `src/lib/rollout.js` | 1381 | `:7232 delta.conversation_count = 1;`（cursor CSV 逐记录置 1，`:7229-7230` 前有 `normalizeCursorUsage` + `isAllZeroUsage` 跳过） | ✓ |
| 6 | `src/commands/init.js:735-739` | 285 | `:735-738 await upsertGeminiHook({ settingsPath: context.geminiSettingsPath, hookCommand: context.geminiHookCommand });`；配套 `src/lib/gemini-config.js:8 const DEFAULT_EVENT = "SessionEnd";`、`:10 const DEFAULT_MATCHER = "exit\|clear\|logout\|prompt_input_exit\|other";` | ✓ |
| 7 | `src/commands/status.js:278-288` | 2945 | `:278 const queueState = (await readJson(queueStatePath)) \|\| { offset: 0 };`；`:287 const queueSize = await safeStatSize(queuePath);`；`:288 const pendingBytes = Math.max(0, queueSize - (queueState.offset \|\| 0));` | ✓ |
| 8 | `src/commands/sync.js:216-220` | 4034 | `:216 // enqueueTouchedBuckets uses presence of that key`、`:217 // as the legacy-group marker, so any later sync that touched a`、`:219 // the entire hour as one aggregate row under model=DEFAULT_MODEL,` | ✓ |
| 9 | `src/commands/sync.js:4313` | 3987 | `:4313 const MAX_INGEST_BUCKETS = 500;`（`:4315` 签名 `batchSize = 200`） | ✓ |
| 10 | `src/commands/sync.js:5813-5822` | 2380 | `:5816 const rebuiltCodexKeys = Object.keys(rebuilt.buckets).filter((k) => k.startsWith("codex\|"));`；`:5817 if (codexFiles.length > 0 && rebuiltCodexKeys.length === 0) {`；`:5819 …rebuild produced 0 codex buckets… skipping to avoid data loss`；`:5821 return false;` | ✓ |
| 11 | `src/lib/account-usage-dedup.js:188` | 2325 | `:185-187 // ORDER BY total_tokens DESC, updated_at DESC (device_id asc as the final deterministic tiebreak). Used ONLY for account-level sources without a stable session identity ('cursor')…`；`:188 function pickLegacyRow(rows) {` | ✓ |
| 12 | `src/lib/codex-model-attribution.js:3` | 2060（代码块内注释出处） | `:3 // Codex token_count events do not carry a model id. Every usage event is`、`:4 // therefore attributed to the model named by the most recent turn_context…` | ✓ |
| 13 | `src/lib/cursor-store.js:44-58` | 444 | `:45-47 const legacyChanged = legacyFingerprint ? !sameFingerprint(legacyFingerprint, manifest.legacyFingerprint) : false;`；`:48-56 if (legacyChanged) { manifest = await migrateLegacyCursorState({…}); }` | ✓ |
| 14 | `src/lib/install-resolver.js:6-18` | 3885、4315 | `:6 // \`union\` (opt-in): return BOTH qualifying installs instead of a single pick.`；`:14-18 // Only providers whose parser dedups by a path-independent identity may opt in … Codex qualifies via its sessionUUID:eventTimestamp event dedup; Claude does the same union by hand in sync.js with file-hash + message-hash dedup behind it.` | ✓ |
| 15 | `src/lib/local-api.js:2435-2467` | 2463 | `:2436-2440 // Reads the optional ~/.tokentracker/tracker/outcomes.jsonl sidecar and joins it to the existing token/$ rows at READ time. Returns available:false …`；`:2441 if (p === "/functions/tokentracker-outcomes") {`；`:2458-2461 if (!outcomes.length) { json(res, { available: false, … }); return true; }`；`:2463-2464 scopedQueueRows + computeQualityPerDollar(rows, outcomes, {from,to})` | ✓ |
| 16 | `src/lib/local-api.js:512-519` | 2286 | `:512 function getTimeZoneContext(url) {`；`:513 const tz = String(url.searchParams.get("tz") \|\| "").trim();`；`:514-517 offsetMinutes = Number.isFinite(rawOffset) ? Math.trunc(rawOffset) : null` | ✓ |
| 17 | `src/lib/outcomes-engine.js:24` | 2788 | `:24 const { computeRowCost } = require("./pricing");`（调用点 `:207 const cost = computeRowCost(row) \|\| 0;`、`:208 const tokens = Number((row && row.total_tokens) \|\| 0);` 与文档同引） | ✓ |
| 18 | `src/lib/pricing/index.js:277-296` | 2566 | `:277-280 const bounded = (value, total) => Math.min(Math.max(0, Number(value) \|\| 0), Math.max(0, Number(total) \|\| 0));`；`:290-296 longInput/longCached/longCacheWrite/longOutput = bounded(row.long_context_*, row.<基础列>)` | ✓ |
| 19 | `src/lib/pricing/matcher.js:410-412` | 2783、3972 | `:410 if (typeof entry.cache_creation_input_token_cost === "number") {`；`:411 out.cache_write = roundToTenDecimals(entry.cache_creation_input_token_cost * 1_000_000);`（单一 `cache_write` 槽位，无 5m/1h 分支） | ✓ |
| 20 | `src/lib/rollout.js:1438` | 1033 | `:1438 const defaultSource = normalizeSourceInput(source) \|\| "opencode";` | ✓ |
| 21 | `src/lib/rollout.js:20249-20252` | 4045 | `:20249-20252 const hourStartStr = toUtcHalfHourStart(event.timestamp) \|\| toUtcHalfHourStart(lastActive) \|\| toUtcHalfHourStart(Date.now());` | ✓ |
| 22 | `src/lib/rollout.js:2862` | 1782 | `:2861 const delta = normalizeClaudeUsage(usage);`；`:2862 if (!delta \|\| isAllZeroUsage(delta)) continue;` | ✓ |
| 23 | `src/lib/rollout.js:4155-4171` | 811、814、2171 | `:4155 function totalsKey(totals) {`；`:4156-4166` 九列 `input/cached/cache_creation/output/reasoning/total/billable/total_cost_usd/conversation_count` join("｜")；`:4169-4170` priority 子集追加 | ✓ |
| 24 | `src/lib/rollout.js:4927-4928` | 2145、4032 | `:4927 const cacheCreation = toNonNegativeInt(u?.cache_creation_input_tokens);`；`:4928 const cacheRead = toNonNegativeInt(u?.cache_read_input_tokens);`（只认这两个名） | ✓ |
| 25 | `src/lib/rollout.js:937` | 382 | `:936 const CLAUDE_FILE_ID_PREFIX_BYTES = 64 * 1024;`；`:937 async function claudeFileIdentityHash(filePath, st) {`；`:938 const length = Math.min(CLAUDE_FILE_ID_PREFIX_BYTES, st.size);` | ✓ |
| 26 | `src/lib/session-analytics.js:32` | 2106 | `:32 const { parseCodexRolloutFile } = require("./codex-rollout-parser");`（只读消费者侧引入） | ✓ |
| 27 | `src/lib/upload-throttle.js:5` | 2782 | `:5 batchSize: 300,`（`DEFAULTS` 内，周边全为上传/节流参数） | ✓ |
| 28 | `test/codex-token-usage.test.js:24-38` | 2118 | `:24 test("usage delta state follows alternating cumulative lineages and skips repeated snapshots", …)`；`:29 consumeUsageDelta(state, usage(100), usage(100))`；`:34-36` 断言 `[100,200,null,50,30]` + `sawInterleaved` + `sawDivergentCumulative` | ✓ |
| 29 | `test/model-breakdown.test.js:835-888` | 2767 | `:835 test("#653 server-reported cost survives aggregation (Grok has no published rate)", …)`；`:836-840 // … Neither field exists on a summed object, so the old path priced these rows off a zero-rate model and showed $0.00 in the list …`；`:884 断言 total_cost_usd = 0.06596 + 0.06718` | ✓ |
| 30 | `tokentracker-ingest.ts:172-174` | 3386 | `dashboard/edge-patches/tokentracker-ingest.ts`：`:172 if (buckets.length > 500) {`；`:173 return json({ error: "Too many buckets (max 500)" }, 400);` | ✓ |

**命中率：30/30 = 100.0%（错例 0）。** 唯一需要补一句解释的是 #2（表格「键为完整 URL」由 `:229 url.toString()` 佐证，函数本身在 `:62-73`）。#17 的引用给的是 `require` 行、调用点在同一文档引用对里另有 `:207`，组合成立。

---

## C. 最终状态检查

**结构完整性（程序化检查，全部 fence-aware）**
- 总行数 4323；代码围栏行 192 = 96 个块，**全部闭合**（收尾状态 0）；非代码行反引号奇偶异常 **0** 行。
- 表格 **116 张**：列数不一致 **0** 张、缺分隔行 **0** 张、结尾断行表格 **0** 张（文档末行即附录 B 表格的完整行）。
- 「重复段落」检查：长度 ≥60 的非表格行完全重复共 18 处，**全部位于代码块内部**（同一段源码在不同章节被再次引用，如 `721-728`、`1691↔2048`、`504↔2245`），属引用式重复，非复制粘贴事故。
- `§X.Y` 交叉引用 55 处，**全部命中现有标题**（missing = 0）。

**断裂引用检查**
- 自动筛出 160 处「前后 6 行窗口内无文件路径」的裸行号引用，抽查 22 处（含文档前段 §3/§5 与后段 §13/附录）——**全部可在 30–40 行内或表头/小节声明的文件中锚定**（如表内承全路径、代码块注释承 `// src/lib/rollout.js:4xxx`），无一真正悬空。

**内部一致性（同一事实多处写法）**

| 事实 | 出现处 | 结果 |
| --- | --- | --- |
| 守卫块 `:5823-5880` / `:5841-5846` / 四处 mergeMaxTotals | 2376、2381、3959、4310、4225 | 一致 |
| `cmdSync` 3194 行（`:516-3709`） | 314、3923、4071、4159 | 一致 |
| GBK 偏移「单文件最多 15 行 / 全 `src/` 52 行」 | 13、90、870、2156、4159 | 一致（4 处口径统一） |
| `status` 10 个路径常量 | 2945 | 仅一处，与源码逐字一致 |
| 热力图时间口径（`:2540` + `:2563` 网格；`:2537`/`:2545` 数据） | 2303、3964、4092、4228 | 一致（§7.3 与 §10/§12 已对齐） |
| goose / qoder / opencode 三处引用校准 | 1191、1553、1554、1568、1039、4229 | 见 J2、J3 |

**唯一实质性内部冲突**：2405 行（§7.5 四种安全模式段）残留短语「既不在尾部追加规范行」——与同段后半句「重排该键的规范行」、同节 #11 表格「重写后再追加规范行（`:3925-3927`）」以及源码 `:3927 if (canonical) output.push(canonical);` 均抵触（见 J1）。这是 R4 修正时未清干净的旧措辞，事实方向已由后半句与 `:3929-3943` 改正。

---

## 遗留问题（若有）

| ID | 位置 | 问题 | 性质 | 建议 |
| --- | --- | --- | --- | --- |
| J1 | §7.5（2405） | 「**既不在尾部追加规范行**、也不删任何既有行」——源码 `:3925-3928` 确实把该键的最新规范行**重追加到 output 尾部**（`if (canonical) output.push(canonical);`），与 #11 表「重写后再追加规范行」抵触 | 措辞冲突（唯一一处内部矛盾；事实主体已写对） | 删去「既不在尾部追加规范行、」8 字，保留「也不删任何既有行」与后句「追加零值撤回行并重排规范行」 |
| J2 | 1191 / 1553 / 4229 | goose 五桶引用两处起点不一：1191 用 `14674-14686`，1553 用 `14678-14686`；台账 4229 写「→ `14674-14686`（两处）」，与实际文本不完全对应 | 粒度不一致（两处各自均能支持其断言：1553 谈「无缓存列」） | 如统一，把 1553 改为 `14674-14686`，或把台账措辞改为「主体引用」 |
| J3 | §5（1039） | opencode 双支路求和引用 `sync.js:1385-1386`，实际求和表达式为 `:1386-1388`（`:1385` 是 `return {`） | 引用范围略窄（起点正确） | 可扩为 `:1385-1389` |
| J4 | §9.6.2（3459-3466） | macOS 方法表用裸行号列（`:43-52`/`:66-69`/`:73-77`/`:79-82`/`:86-90`/`:94-97`/`:101-105`/`:110-113`/`:119-121`/`:127-143`/`:148-150`），表头与小节均未声明所属文件；实测 11 项行号**全部**对应 `TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift`（`:43 func fetchSummary`、`:66 fetchDaily`、`:73 fetchHeatmap`、`:79 fetchModelBreakdown`、`:86 fetchProjectUsage`、`:94 fetchMonthly`、`:101 fetchHourly`、`:110 fetchUsageLimits`、`:119 fetchSubscriptions`、`:127 triggerSync`、`:148 checkServerHealth`） | 锚定弱（内容全对） | 表头补「行号 = `TokenTrackerBar/.../Services/APIClient.swift`」一处即可 |

**判定：J1–J4 均不构成事实错误，不影响 §12 开放问题清单（`【未能确认】` 项）的既有结论，也不改变任何机制性判断。主文档可按现状态定稿；是否收尾 J1–J4 由主线决定。**
