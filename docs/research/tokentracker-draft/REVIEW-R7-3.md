# 全文复核 R7-3：§6 Claude/Codex 深度解剖、§7 聚合去重与时间语义、§8 金额计算全链路

- 被复核文档：`docs/research/2026-10-14-tokentracker-usage-statistics.md`，复核范围 = §6 全部（L1648–L2176）+ §7 全部（L2177–L2540）+ §8 全部（L2541–L2940）。
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14），复核前后一致。复核全程只读：未改样本仓任何文件、未做 git 写操作、未改主文档。
- 行号口径：一律 `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` 或等价的 `fs.readFileSync(...).split("\n")`（去尾换行后取行数）；**未使用裸 `Get-Content`**。文中「文件 N 行」均按去尾换行口径。
- 复核方式：脚本机械抽取 §6–§8 全部 `路径:行号` 与裸行号续引 → 存在性/越界检查 → 初判异常逐条回读原文定性；随后由本人回源核验 **62 组机制级断言**并贴源码摘录（所有计数类数字用脚本独立重算，不算引用值）；§8.6 的两段演算与三个测试锚点**全部自己重算**（不复用文档算式）。
- 真实语料只做计数类只读核对，本报告不含任何会话内容（无会话文本、无代码、无模型串、无项目名）。
- 本报告**只做证伪与登记**，不修改主文档、不修改样本仓库。

## 结论

**§6/§7 的机制性结论（五桶映射、差值引擎、去重分层、repair 守卫、两级时区回退、三份半小时桶实现）经 62 组回源核验全部成立；§8 的四桶算式、五源推理门控、三层价格表、0–7 级匹配链与 §8.6 的两段演算逐值正确。问题集中在 5 条引用/叙述与 3 条口径，其中 3 条必须修。**

1. **引用存在性**：§6–§8 共抽取 **433** 个显式 `路径:行号` 引用 + **667** 个同行省略路径的裸行号续引 = **1100** 个引用点。显式引用 **越界 0、指向不存在的文件 0**（433/433 全部解析成功）；裸行号续引按段级上下文机械归属后报出 59 个初判越界，**逐条人工回读后全部为归属器伪影，真越界 0**。
2. **引用内容级错位 5 条**（行号存在但指向的对象/顺序/范围不对）：§8.2 的「`src/` 全目录 0 命中」被证伪（P1）、§7.5 表 #11 的执行顺序写反（P3）、§7.7 写点 `:3489`（应为 `:3536`，P5）、§8.1 的注释行号 `:249-252`（应为 `:243-247`，P6）、§8.4 的 `:1067` 不属「require 同一 pricing 模块」（P7）。
3. **§6.1.6 的 `【未能确认】` 已过时**：其自述的判定条件正是 §12.A3 / §12.E6 已执行并判定「本机无害」的那一项检查——同一文档两处结论相反（P2，必修）。
4. **计数复算全部吻合**：`mergeMaxTotals` 恰 4 处调用（`:5848/5852/5856/5860`）、`setMaxTotal` 恰 5 处（`:6492/6512/6540/6559/6579`）；`toUtcHalfHourStart(` 分布 70 次/66 行 + 1 次/1 行 + 3 次/3 行 = **74 次 / 70 行**；`parseBucketKey` 15 处调用 + 1 处定义；`ensurePricingLoaded(` 27 行 / 裸标识符 36 行；`src/lib/pricing/` 内 `total_tokens` 仅 1 次且位于 `index.js:283` 注释内；seed 的 2285 顶层键 / 4 字段白名单 / 直方图 2274·2273·566·188 逐值一致；edge 五份手抄块 394 行、`sha256[:16] = 37840dc53f7b6e2f`、119 + 64 键、9 键与 44 键双向差集全部逐键一致。
5. **§8.6 独立复算**：演算 A = `0.284375`、演算 B = `0.150000`；三个锚点 = `0.836` / `73.5` / `0.0009675`，**与文档逐值相同**（另复算推理对照项 0.162、0.1296 也成立）。
6. **问题计数：必须修 3 条、应修 3 条、建议修 4 条。**

---

## A. 全量引用存在性

### A.1 抽取口径

- 显式引用：正则 `([路径]/[文件名].(js|ts|json|cjs|cs|swift|jsx|md|jsonl|sql)):N[-M]`，共 **433** 个。
- 裸行号续引：同行内 `:N` / `:N-M` 且同一段落中已有文件上下文者，共 **667** 个。
- 短文件名补路径规则：`matcher.js`/`index.js`/`litellm-fetcher.js` → `src/lib/pricing/`；`tokentracker-*.ts` → `dashboard/edge-patches/`；`rollout.js`/`claude-categorizer.js`/`local-api.js`/`codex-rollout-parser.js`/`codex-usage-record.js` → `src/lib/`；`serve.js` → `src/commands/`。
- 排除项：`:00` / `T30` / `:0` 一类时间与格式串（7 条，全部人工回读为 `HH:MM`、`${dayKey}T${HH}:00:00` 等非行号）；`TokenTrackerLinux/.../extension.js:997` 的省略号路径按 `TokenTrackerLinux` 下唯一 `extension.js` 解析成功。

### A.2 统计表

| 口径 | 数量 |
|---|---|
| 显式 `路径:行号` 引用点 | **433** |
| 裸行号续引 | **667** |
| 引用点合计 | **1100** |
| 显式引用：文件不存在 | **0** |
| 显式引用：越界 | **0** |
| 裸引用：机械初判越界（归属器伪影） | 59 → **真越界 0** |
| 区间端点倒置 / 非法 | **0** |

被引用的样本仓库文件数（短名补全路径后去重）**72** 个。引用最密的前 12 个文件：

| 文件 | 引用数 | 文件 | 引用数 |
|---|---|---|---|
| `src/lib/rollout.js` | 70 | `src/lib/codex-token-usage.js` | 6 |
| `src/lib/pricing/index.js` | 44 | `src/lib/upload-queue.js` | 6 |
| `src/lib/local-api.js` | 38 | `src/lib/pricing/matcher.js`（含裸 `matcher.js`） | 21 + 10 |
| `src/commands/sync.js` | 35 | `test/pricing.test.js` | 17 |
| `src/lib/session-analytics.js` | 16 | `test/edge-pricing-parity.test.js` | 10 |
| `test/gpt-6-astra-pricing.test.js` | 10 | `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts` | 9 |

### A.3 59 条初判越界的逐条定性（全部伪影，真越界 0）

归属器只按「同行 / 同段落最近出现的完整路径」取上下文，而本文常见写法是「一行内先给 A 文件的行号、再给 B 文件的完整引用」。抽 10 类代表逐条回读：

| 文档行 | 初判 | 定性证据（本人回读） |
|---|---|---|
| L1726 `:4964`、`:4927-4928` | 归到 `codex-token-usage.js`（195 行）越界 | 实指 `src/lib/rollout.js`：`:4927-4928` = `const cacheCreation = toNonNegativeInt(u?.cache_creation_input_tokens);` / `const cacheRead = toNonNegativeInt(u?.cache_read_input_tokens);`；`:4964` = `function toNonNegativeInt(v) {` ✓ |
| L1867 `:4924-4926` | 归到 `claude-categorizer.js` 越界 | 实指 `rollout.js:4924-4926`：`Math.min(outputTokens, toNonNegativeInt(u?.output_tokens_details?.thinking_tokens ?? ...))` ✓ |
| L2049 `:4873-4876` | 归到 `pricing/index.js` 越界 | 实指 `rollout.js:4873-4876`（`Preserve the reported total for compatibility with older Codex / Every Code shapes …`）✓ |
| L2185 `:3656`、`:3950`、`:4668` | 归到 `local-api.js` 越界 | 实指 `src/commands/sync.js`：`:3656` 主队列上传注释、`:3950`/`:4668` 两处偏移重置 ✓ |
| L2200 `:19990`、`:20016-20032` | 归到 `sync.js` 越界 | 实指 `rollout.js`：`retractStaleSourceQueueRows` 定义与零值撤回循环 ✓ |
| L2224 `:12286`…`:20776` | 归到 `sync.js` 越界 | 实指 `rollout.js`：`rg` 复算的 15 处 `parseBucketKey` 调用点之一 ✓ |
| L2324 `:3147`/`:608`/`:2382`/`:2390-2397`/`:2538-2544` | 归到 `UsagePoller.cs` 越界 | 实指 `src/lib/local-api.js` ✓ |
| L2455 `:2737-2740`/`:95`/`:2430`/`:2414`/`:2656`/`:3187`/`:2900` | 归到 `details-pagination.test.js` 越界 | 实指 `src/lib/local-api.js` 的分页缺失断言组 ✓ |
| L2685 `:118`/`:128`/`:134`/`:141`/`:2219`/`:2299` | 归到 `tracker-paths.js`（16 行）越界 | 实指 `src/lib/usage-limits.js`（4520 行）的五份缓存常量与两处路径拼接 ✓ |
| L2813 `:2414`/`:2695`/`:1349` 等 | 归到 `LeaderboardPage.jsx` 越界 | 实指 `session-analytics.js` / `tokentracker-leaderboard-refresh.ts` ✓ |

**结论：无一条真实越界。**

---

## B. 核心机制核验（62 条，全部本人回源 + 贴摘录）

### B.1 §6.1 Claude（12 条）

1. **五桶归一与 total 口径**（`src/lib/rollout.js:4921-4939`）— 逐行核对：`reasoningTokens = Math.min(outputTokens, toNonNegativeInt(u?.output_tokens_details?.thinking_tokens ?? u?.output_tokens_details?.reasoning_tokens))`；`totalTokens = inputTokens + outputTokens + cacheCreation + cacheRead`（`:4929`，用**未减**的 output）；`output_tokens: outputTokens - reasoningTokens`；`cached_input_tokens: cacheRead`。文档表格的六行映射与「两列互补」结论**逐项成立**。
2. **`toNonNegativeInt`**（`:4964-4968`）：`if (!Number.isFinite(n) || n < 0) return 0; return Math.floor(n);` ✓ 与文档「非有限值与负数吸收为 0、其余 Math.floor」一致。
3. **缓存字段只认两个名字**（`:4927-4928`）+ 别名兜底只在 Codex 路径（`src/lib/codex-token-usage.js:25-27`：`field === "cache_creation_input_tokens" ? value.cache_creation_input_tokens ?? value.cache_write_input_tokens : value[field]`）✓ 建立。
4. **`claudeMessageDedupKey`**（`:4914-4918`）逐字：`const msgId = typeof obj?.message?.id === "string" && obj.message.id ? obj.message.id : null; if (!msgId) return null; const reqId = ...; return reqId ? \`${msgId}:${reqId}\` : msgId;` ✓；使用处 `:2854`（`const dedupHash = seenMessageHashes ? claudeMessageDedupKey(obj) : null;`）✓。
5. **1.6–3.7 倍超算注释**（`:4895-4905`）逐字含「The short-circuit caused 1.6–3.7x overcounting on every affected provider — see issue #64.」；来源两类（DeepSeek/Kimi/Mimo/MiniMax 兼容端点不返回 `request-id`、Claude Code 子代理/thinking 传输丢字段）✓；与 v3 地面真值缺陷的对应（`src/commands/sync.js:227-233`，`v3's ground-truth scan itself used \`if (msgId && reqId)\` to build the dedup key`）✓。
6. **`cursors.claudeHashes` 上限**（`:1236-1237`）：`allHashes.length > 100_000 ? allHashes.slice(allHashes.length - 100_000) : allHashes` ✓「上限 100k 且保留最后 100k」。文件级去重层只在混合形态启用（`:1035` `const seenFileIds = sawUncFile && sawLocalFile ? new Map() : null;`）✓。
7. **fork 别名与身份上溯**（`src/lib/claude-user-identity.js`）：`noteClaudeFork` 在 `:5-11`（顶层 `uuid` + `forkedFrom.messageUuid`，`uuid !== parent`）✓；`claudeUserIdentity` 在 `:13` ✓；子代理路径排除 `:31`（`if (!filePath || /[\\/]subagents[\\/]/.test(filePath)) continue;`）✓；只采信 `type === "user"` 在 `:44` ✓；环折叠确定性在 `:23-24` ✓。
8. **预扫描 + 净值修正只动 `conversation_count`**（`rollout.js:987-1015`）：`:987` 注释「Resolve all changed-file lineage before counting: fork files may sort first.」✓；`:992-994` 注释「Preserve all token totals and history whose source files were removed.」✓；下界保护 `:1011` 逐字 `if (!bucket || bucket.totals.conversation_count < count - 1) continue;` ✓；历史哈希折叠只对 `startsWith("u:")` 生效（`:997-1003`）✓；别名表持久化 `:1234` ✓。
9. **`endOffset = st.size` 三处分工**：`:2291`（`parseRolloutFile`，仅用于 `:2294` 的 `startOffset >= endOffset` 早退，正式返回走 `:2652 committedEndOffset`）✓；`:2682`（`scanRolloutProjectFileContexts`，返回 `:2765 committedEndOffset`）✓；`:2793`（`parseClaudeFile`，返回 `:2889 { endOffset, eventsAggregated }` → 游标 `:1194 offset: result.endOffset`）✓。非对称性三处对照（`openclawOffsetEndsAtLineBoundary` 在 `:1949`、OMP `maxRecordBytes`、Codex 连项目扫描位都有提交保护）✓；三态重置路径 `startOffset = 0` 在 `:488-504`（`rebuildingCodexBaseline` 与 `truncated` 判定）✓。
10. **usage 双位置回退 + OmO 镜像剥离**（`:2841` 字符串预筛 / `:2849` `isOmOClaudeSdkMirror` / `:2851` `obj?.message?.usage || obj?.usage`）✓；判定 `:4910-4912`：`obj?.entrypoint === "sdk-ts" && obj?.userType === "external"` ✓；`:4906-4909` 的 issue #726 说明 ✓。
11. **`conversation_count` 语义**：assistant 行 `:2865` `delta.conversation_count = 0;` ✓；user 文本行计数在 `:2810-2836`，`userModel = DEFAULT_MODEL` 在 `:2831`，计数语句 `+= 1` 在 `:2833` ✓；`tool_result` 排除注释 `:2807-2809` ✓。
12. **`<synthetic>` 无解析特判**：`session-analytics.js:101` 的 `PLACEHOLDER_MODELS` 逐字 `new Set(["<synthetic>", "synthetic", "<unknown>", "unknown"])` ✓；`rollout.js` 用量路径确实无该字符串特判 ✓。

### B.2 §6.2 Codex（12 条）

13. **`extractTokenCount` 两信封**（`rollout.js:4838-4849`）逐字：`payload.type === "token_count"` → `{ info: payload.info, timestamp: obj?.timestamp || null }`；否则 `payload.msg.type === "token_count"` → `{ info: msg.info, ... }` ✓。
14. **落桶字段与 `latestTotal`**（`:2530-2554`）：`const info = token.info; if (!info || typeof info !== "object") continue;`（`:2530-2531`）；`const lastUsage = info.last_token_usage; const totalUsage = info.total_token_usage; if (totalUsage && typeof totalUsage === "object") latestTotal = totalUsage;`（`:2545-2547`）；`const rawDelta = consumeUsageDelta(usageDeltaState, lastUsage, totalUsage);`（`:2549`）✓。
15. **差值引擎四分支**（`codex-token-usage.js:98-158`）：重复命中流头 → `touchBaselineAt` 后 `return null`（`:110-118`）；`total - last` 命中流头 → 返回 `last`（`:121-132`）；不命中但可减 → 置 `sawDivergentCumulative` 后返回 `last`（`:134-138`）；无 `last`/流断裂 → `diffUsage(total, active)`，`delta.total_tokens > last.total_tokens` 时判为新流（`:150-153` 注释逐字「A cumulative jump larger than last usage cannot come from the normal append_last_usage path.」）✓；`subtractUsage` 硬约束 `:49-59`（任一字段 `total < last` 即 `return null`，`:55`）✓。
16. **`MAX_USAGE_BASELINES = 32`**（`:18`）+ LRU 截断 `:176`（`while (state.baselines.length > MAX_USAGE_BASELINES) state.baselines.shift();`）✓。
17. **零值重置哨兵**（`rollout.js:4881-4893`）与文档引用块**逐字相同**（含 `?? usage?.cache_write_input_tokens` 的别名折叠、`componentTotal` 五项）；启用限定 `:2550-2553`（`source === DEFAULT_SOURCE`）✓。
18. **`input -= cached`**（`rollout.js:4851-4878`）：注释 `:4864-4872` 逐字含「the cached slice is INSIDE the input count」「producing ~6–7x cost inflation on cache-heavy Codex sessions (verified against ccusage's per-day numbers on the same rollouts)」；算式 `:4877` `out.input_tokens = Math.max(0, out.input_tokens - out.cached_input_tokens);` ✓；兼容说明 `:4873-4876` ✓。
19. **reasoning 是 output 子集（Codex 侧不拆列）**：`normalizeUsage`（`:4851-4878`）对六桶只做 `Math.floor` 与非负吸收，**无 `output -= reasoning`** ✓；计价置 0 的 5 元白名单 `pricing/index.js:248-253`（`codex`/`acode`/`every-code`/`omo`/`cline`）✓；三处使用同一门控（`:254` 基础费、`:297-299` 长上下文、`:325-327` priority）✓——**不存在双计路径**。
20. **`turn_context.model` 权威**（`codex-model-attribution.js`）：头注释 `:3-6` 逐字「Every usage event is therefore attributed to the model named by the most recent turn_context」；`:11-15` 明确排除 `thread_settings_applied`（「it fires when the user picks a model in the UI - seconds to minutes before the switch takes effect」）✓；`:82-92` turn_context 分支（`state.rerouted = false`）；`:96-102` session_meta 只在 `!state.selectedModel` 时采纳且**永不覆盖** turn_context（`:98` `if (!metaModel || state.selectedModel) return null;`）✓；`:18-23` 的 0/10310 实测注释 ✓；调用点 `rollout.js:2476-2477` ✓、状态持久化 `:639` ✓、初值注入 `:2320-2322` ✓。
21. **fork replay 双重机制**：`FORK_REPLAY_GAP_MS = 500`（`:4210`）与 `isForkedReplayToken`（`:4212-4214`，`currentDate < rolloutDate`）✓；阈值依据注释 `:4203-4209` 逐字（「replay flush spaces rows sub-ms to a few ms apart while genuine live turns arrive ≥~4.6s apart and the replay→live break is ≥11s」「500ms biases toward the low end because merging a real fast turn (under-count) is worse than leaving a slow replay counted (bounded over-count)」）✓；解析循环内 `:2588-2590` 永久关闭、`:2591-2594` 跳过、`:2601` 跳过点、`:2567-2569` 的 <1% 残余 over-count 自认 ✓。
22. **rescan 事件级去重与判定顺序**：去重键 `:2626` `const dedupKey = \`${sessionId || filePath}:${tokenTimestamp}\`;` ✓；inode 重写动机注释 `:2608-2618`（含 Codex-Manager 每次切渠道原子重写）✓；`needsHistoricalRolloutDedup` 判定顺序 `:428-461`：同会话换路径判定（`:439-454`）**先于**稳态短路（`:456-460`），注释逐字「This MUST be decided before the steady-state short-circuit below」✓。两级追踪器 `getAppendOnlyEvents`/`getHistoricalEvents` 在 `:302-380` ✓，诊断分支 `:351` 的 `hash_set_constructions` ✓。
23. **compaction 计费**（`codex-usage-record.js`）：头注释 `:10-21` 与文档引用**逐字一致**（含「the cumulative delta is zero, so without the record the call is lost」「A record without one is never counted.」）✓；计数条件 `:146-152`：`if (!compaction?.compacted) return null; if (!canonicalUsage(previousTotal) || !sameUsage(previousTotal, totalUsage)) return null;`（`sameUsage` 在 `:150`）✓；无 `response_id` 永不计（`:20-21` + `rollout.js:2356 if (!record.responseId) return null;`）✓；上限 `MAX_COMPACTION_RESPONSE_IDS = 20000`（`:60`）✓；`turn_token_usage`/`thread_token_usage` 从不读取（`:31-32`）✓；`recordOnly` 与告警（`rollout.js:649-655`、`codex-usage-record.js:154-177`）✓；`task_complete`/`turn_aborted`（`:82-86`）✓。
24. **service tier 子集列**：`codex-service-tier.js:20`/`:26` 两个常量逐字；归属规则 `:12-15`（最近一条前置 `thread_settings_applied`、未知按 Standard、从不猜）✓；`PRIORITY_SUBSET_FIELDS` 五个字段在 `rollout.js:4073-4078`，注释 `:4067-4072` 逐字含「They annotate the existing columns and are NEVER part of any sum」✓；为零时整列省略（`prioritySubsetOf`，`:4090-4095`）✓。

### B.3 §7 聚合、去重与时间语义（14 条）

25. **写入端幂等两道判定**：`rollout.js:3403-3408` 与文档代码块一致（`usagePrecision` 拼接在调用点、`outputKey`、`queuedKey` 短路）✓；写的是累计快照（`:3409-3427`，九个数值列 + `usage_precision` + priority 子集 + `conversation_count`）✓；`appendFile` 在 `:3482-3484` ✓；`totalsKey` 九个数值列在 `:4155-4166`、priority 子集拼在其内部 `:4167-4170` ✓；项目队列键不含 model、`project_key`/`project_ref` 同时存在才写（`:3505-3531`）✓。
26. **撤回行**三触发点：模型对齐补零（`:3272-3289`，`alignedModel` 分支）；整源撤回 `retractStaleSourceQueueRows`（`:19990` 定义、`:20016-20032` 核心循环，与文档代码块逐字一致，含 `if (totalsKey(entry.row) === totalsKey(zero)) continue;` 与 `fs.appendFile`）✓；游标 unknown 桶清理（`sync.js:5173`/`:5207 appendFile(retractions...)`）✓。
27. **桶键构造与归一化不对称**：`bucketKey :4222`、`projectBucketKey :4228`（`projectKey|source|hourStart`，无 model）、`groupBucketKey :4233`、`parseBucketKey :4238` ✓；`normalizeSourceInput :4254-4258`（trim + lowercase）与 `normalizeModelInput :4260-4264`（**只 trim**）✓。文档所述「项目键与主队列键不通用」成立（`:4241-4251` 按 `source|model|hourStart` 反解）✓。
28. **`parseBucketKey` 调用点复算**：`rg` 复算 = **15 处调用 + 1 处定义**，行号与文档列出的 15 个**完全一致**（`:1973`/`:3174`/`:3199`/`:3223`/`:3238`/`:3434`/`:3570`/`:3636`/`:3655`/`:7212`/`:12286`/`:13378`/`:14251`/`:16376`/`:20776`），定义 `:4238` ✓；`:1973` 处容器确为 `attemptHourly.buckets`、`:3174` 为 `touchedBuckets` ✓。
29. **未上传尾部压缩**（`upload-queue.js`）：`BACKLOG_BYTES = 1_000_000`（`:5`）✓；持锁前置注释 `:62-63` 逐字 ✓；门槛与行边界（`:71-78`，前 1 字节必须为 `0x0a`）✓；去重键 `:53`（session）/`:59`（hourly）✓；严格更新 + 等值保留首条 `:84-90`（注释「equal stamps retain the first observation」）✓；原子替换全链 `:94-131`（`wx` 新建 → 拷已上传前缀 `:97-103` → 拷胜出行 `:104-110` → `sync()` `:111` → 关 output `:112` → 关 input `:115`（注释「Windows cannot replace the destination while our read handle is open.」）→ 复查 inode/size/mtime `:117-120` → `rename` `:121`）✓ 与文档描述的七步**逐步一致**。
30. **读端 last-row-wins**：合并实现 `async-queue-reader.js:86-91`（`for (const [key, row] of changes) seen.set(key, row);` + `tail` 只进本次返回、注释 `:89-90`）✓；坏行 `:49-57`（`catch { malformed++; }`）✓；主/项目队列键 `local-api.js:202-211`/`:145-150` ✓；CLI 复刻 `wrapped-aggregator.js:77-85`（注释逐字含「Naively summing every row double-counts every previously synced bucket」）✓ 与 `:80` 键 ✓。
31. **MAX 与 last-wins 两套口径并存**：`setMaxTotal :6585-6589`、`mergeMaxTotals :6574-6583` ✓；**调用点复算**：`mergeMaxTotals` 全仓**恰 4 处**（`:5848/5852/5856/5860`）+ 定义 `:6574`；`setMaxTotal` 全仓**恰 5 处**（`:6492/6512/6540/6559/6579`）+ 定义 `:6585` —— 与文档「恰四处 / 另有 5 处族内调用」**完全一致**；四处均落在 `projectRepairEnabled` 的 `if` 块（`:5823-5880`）内 ✓；`:6477-6562` 确为四个 totals 函数 ✓。
32. **读端缓存、续读与容错**（`async-queue-reader.js`）：签名 `dev:ino:size:mtimeNs:ctimeNs` 在 `:8-10`，且在**已打开的 fd 上** stat（`:30-35`，注释「Stat the opened file, rather than a path which can be atomically replaced between stat and open.」）✓；双层续读证据（inode 不变 + 变大 + `head` 与 `consumedBytes - PROBE_BYTES` 锚点，`:40-47`，`PROBE_BYTES = 256` 在 `:6`）✓；单飞 `:109-114`、`maxEntries = 4` `:16`、LRU 淘汰 `:94` ✓；64 KiB 分块与让出事件循环 `:5`（`CHUNK_BYTES = 64 * 1024`）/`:79` ✓；`ENOENT` 清缓存返回空、其他错误回退上次快照 `:98-103` ✓。
33. **读时纠错 `normalizeQueueRow` 不重写文件**（`local-api.js:170-200`）：legacy codex 行精确不变量（`:155-168`，`totalTokens === inputTokens + outputTokens` 且 `cached > 0` 且 `input >= cached`）✓；cursor/antigravity 的 `billable < total` 提升（`:191-198`，与文档代码块逐字一致）✓；两处成因注释（`:179-190`：issue #106 的 kind 门控、`e69e2746a` 的 O(N) 重构误删 167 行）✓。
34. **`toUtcHalfHourStart` 三份实现与分布**：主实现 `rollout.js:4173-4190`（`>= 30 ? 30 : 0`、秒毫秒清零、`toISOString()`、非法返回 `null`）与文档代码块**逐字一致** ✓；副本 `claude-categorizer.js:1175-1191`（同数学、写法不同）✓；第三份 `sync.js:4538-4549` 的 `toGrokRepairHalfHourStart`（毫秒整除半小时，`value < 10_000_000_000` 按秒 ×1000，`:4542`）✓。**分布独立复算**：`rollout.js` 70 次/66 行（`:20214` 一行 3 次、`:20265`/`:20418` 各 2 次）、`sync.js` 1 次/1 行（`:6663`）、`claude-categorizer.js` 3 次/3 行（`:1175`/`:1250`/`:1279`），合计 **74 次 / 70 行** —— 与文档表格**逐格一致**。
35. **两级时区回退**：`rowDayKey`（`:213-225`）缺省 `return hs.slice(0, 10)`（`:224`，UTC 日）✓；`getZonedParts`（`:521-593`）三级：IANA formatter `:525-571`（失败也缓存，`:542-552`）→ `tz_offset_minutes` 固定偏移 `:573-583` → **本机 getter** `:585-592`（`dt.getFullYear()/getMonth()/getDate()/getHours()`）✓；时区上下文来自查询参数 `tz` / `tz_offset_minutes`（`:512-519`）✓；四类无守卫调用点（`aggregateHourlyByDay :606/:608`、`usage-summary :2382`、`usage-heatmap :2538`、`night_owl :482`）逐点确认 ✓。
36. **日键闭区间过滤与端点粒度**：`const daily = allDaily.filter((d) => d.day >= from && d.day <= to);`（`:2364`）逐字 ✓；项目端点谓词 `:130-134`（含 `hasRange` 短路）✓；`usage-hourly` 的 `day` 缺省 `new Date().toISOString().slice(0, 10)`（`:3147`）与过滤用当地日键（`:608`）**基准不同** ✓（文档「两族缺省基准不同」成立）；小时键空间 `${dayKey}T${HH}:00:00`（`:609`）、返回 `Array.from(byHour.values())`（`:639`，不预填 24 项）✓；`week_starts_on: "sun"`（`:2586`）与 `streak_days: 0`（`:2588`）恒值 ✓。
37. **会话级时间语义三函数**（`session-analytics.js`）：`sessionUsageDay :2343-2354`（IANA 优先、formatter 抛错才落下一级、否则 `offsetMinutes`（缺省 0）平移后取 UTC 日——`:2353` 逐字 `new Date(date.getTime() + (Number.isFinite(offsetMinutes) ? offsetMinutes * 60_000 : 0)).toISOString().slice(0, 10)`）✓；`withinDayRange :2356-2362`（**区间相交**：`if (from && (!endDay || endDay < from)) return false; if (to && (!startDay || startDay > to)) return false;`）✓；`scopeSessionUsage :2364-2383`（闭区间日过滤 `:2366-2369`、`cost_usd: 0` 交 `repriceSessionRecord`、生命周期另存 `:2381-2382`）✓；`repriceSessionRecord :269-308`（逐 bucket/逐 model_usage 计价，`:272`/`:279`/`:302`）✓。
38. **去重分层**：账户级 2a MAX（`account-usage-dedup.js:184-198` 的 `pickLegacyRow`，注释逐字含 `Mirrors the legacy SQL DISTINCT ON (hour, source, model) ORDER BY total_tokens DESC, updated_at DESC (device_id asc as the final deterministic tiebreak)`）✓；2b trae-cn 严格更新（`:113`/`:120-133`，键 `${row.source}|${row.session_id}`、`if (rowMs > storedMs)` 才替换 → 等值保留先落库者）✓；三类修正在同一操作收敛（`:30-32`）✓；缺席不断言（`:34-36`）✓；回聚 `(bucket_start, source, model)` 且 `conversations += 1`（`:159-174`）✓；控制记录 kind `:57` ✓；双安装命名空间（`multi-install-parser.js:13-22` 用 `wsl.isUncPath` 判幸存者、`:61-85` 迁移 seed 规则「恰好一个命中才只 seed 那一个」并注释「an install re-parsed without its dedup state would double-count its entire history, and a bounded backfill gap is the cheaper failure」、`:92-110` both 模式路径 Set）✓；跨根（`session-analytics.js:1824` 归组、`:1830-1845` 单根同 UUID 整体退出去重并注释「an ambiguous duplicate is cheap, a deleted session is not」、`:1895-1909` 仅字节相同才折叠且胜者取 mtime、`:1912-1926` Codex 侧）✓。
39. **repair 体系 16 键与 16 行表**：`sync.js:204-309` 逐键核对，文档列出的 16 个迁移键**全部命中**（`cursorUnknownPurge_2026_04 :204`、`rolloutCumulativeDeltaReparse_2026_05 :205`、`claudeMemObserverReinclude_2026_05_v3 :206`、`grokAppendOnlyRepair_2026_05_v4 :207`、`codebuddyLogJsonlOverlapRepair_2026_08 :208`、`workbuddyContextUsageRepair_2026_08 :209`、`claudeGroundTruthRepair_2026_05_v4 :248`、`cloudConversationsBackfill_2026_06 :257`、`codexRescanDedupRepair_2026_06 :268`、`codexForkReplayRepair_2026_07 :280`、`codexUsageLineageRepair_2026_07_v2 :288`、`droidDupSessionInflationRepair_2026_06 :293`、`mimoClaudeMislabelRepair_2026_06 :306`、`deepseekHarnessSourceMigration_2026_08 :307`、`zcodeNativeUsageRepair_2026_08 :308`、`zcodeInclusiveTokenRepair_2026_09 :309`）；调用点 `:900/901/909/918/927/928` + 后台分支 `:938` **逐点一致**；门禁形态（`repairGrokQueueFromSessionSnapshots :5010-5016`、`migrateLegacyDeepseekHarnessSource :3865-3868`）与 noop 记账（`:3913-3920`，`status: "noop"`）✓；表内 16 行的定义处/调用点/影响面**逐条回源确认**（含 `:5216/5281`、`:5682/5895-5922`、`:6038`、`:6134`、`:6612/6743/6772`、`:4782/4744`、`:7097/7147/7207-7209/7135`、`:6844/6874/6918-6920/7015`、`:4994/4884/4929-4936/4953/4955/4979-4984`、`:5002`、`:3865-3962`、`:5173/5207`、`:5315/5401/5460-5462`、`:5508/5571/5628-5630`、`:5010/5124/5126`、`:4711`）。
40. **四道守卫 + 0.26.3 事故**：重建产出守卫 `:5813-5822`、项目键集合守卫 `:5841-5846`（`missingProjectKeys`，用键集合而非 MAX）、总量下降守卫 `:5865-5879`（`rebuiltMainTotal >= existingMainTotal` 才记 `partialProjectKeys`，命中即 `console.error(... "skipping to avoid data loss")` + `return false`）、文件快照守卫 `:5882-5890` ✓ 四条判据与归属**逐条成立**；claude 侧守卫注释 `:258-267`（`:264` 起逐字 `GUARDED: skips if any codex session file that previously contributed is no longer on disk … (ref the v6 ground-truth-repair data-loss incident)`）✓。事故数字：`2.17B` = `-1.27B opus-4-7 -474M opus-4-6 -376M sonnet-4-5 -48M haiku -6M sonnet-4-6`，**本人重算** `1270+474+376+48+6 = 2174M ≈ 2.174B` ✓ 与文档「分项求和 2174M」一致；`:242` 的 `0.26.4 HALTS back at v4` ✓、`:248` key 仍为 v4 ✓。原子重写前 tmp 重建、失败不动（`:5800-5811`）✓；畸形项目行闸门（`:5824-5830`，定义 `:6436`）✓。
41. **上传偏移清零把本地修复广播到云端**：`CLOUD_CONVERSATIONS_BACKFILL_KEY` 注释 `:249-256` 逐字含「Historical cloud rows can only be repaired from each user's local queue.jsonl — resetting the upload offset replays the full queue」✓；两处偏移重置实现（`:3950-3962` dsh、`:4668-4680` grok）✓。

### B.4 §8 金额计算全链路（21 条）

42. **签名与返回值三来源**：`computeRowCost(row)` 在 `pricing/index.js:227`，单参、无 options ✓；返回 `0`（`:228`/`:232`）、`row.total_cost_usd`（`:236-241`）、五桶算式（`:254-354`）三来源 ✓；导出 `:371` ✓；`local-api.js` 解构入口 `:57-64`（`computeRowCost` 在 `:60`）与再导出 `:3527-3531`（`:3530`）✓。
43. **三条短路与 `> 0` 语义**：`LOCAL_INFERENCE_SOURCES = new Set(["lmstudio"])`（`:27`）、`PI_SUBSCRIPTION_SOURCES` 4 成员（`:18-23`）、`SOURCES_WITH_AUTHORITATIVE_COST = new Set(["grok", "cline"])`（`:32`）✓；判定 `reportedCost > 0`（`:240`）与注释 `:233-235`（「zero remains the legacy "unreported" sentinel」）✓。
44. **四桶算式**：与文档代码块逐字一致（`:254-264`，除数为硬编码 `1_000_000` `:263`）；映射 `input→pricing.input :258` / `output→pricing.output :259` / `cached→pricing.cache_read :260` / `cache_creation→pricing.cache_write :261` / `reasoning→pricing.output :256` ✓；缺字段一律 `(x || 0)`、无推导无插值 ✓；`billable_total_tokens` 只服务展示与热力图（`local-api.js:284`/`:614`/`:2565`）✓。
45. **推理门控 5 源与三处使用**：`:248-253` 5 个 source ✓；非折叠按 `pricing.output` 单价（`:254-256`）✓；全局搜索 `src/` 的 `reasoning_output_tokens` 在定价代码里只出现在 `:256`/`:299`/`:327`/`:348` 四处 ✓（本人复核该四点，全为加项或子集标注）。
46. **长上下文溢价**：白名单 `:267-269`（`gpt-5.6`/含 `gpt-5.6-sol`/`gpt-6-astra`/`gpt-6-sol`/`gpt-6.1-sol`）✓；`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000` 在 `:31` 定义、`:375` 导出，**本文件内零内部读取**（本人 grep 复算：`272_000` 在 `pricing/index.js` 仅 `:31` 一处）✓；真正判阈值者 `codex-rollout-parser.js:51` → `:733` 逐字 `Number(rawRequestUsage?.input_tokens || 0) > OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` ✓；子集列累加 `:732-741`、priority 交集列 `:742-762` ✓；`bounded(value, total)` `:277-280` 只夹不放大 ✓；公式 `:300-306` 与文档逐项一致、`baseCost + longContextPremium`（`:308`）✓、恰好叠加一次 ✓；不在白名单时 `longContextPremium = 0`（`:300` 三元）✓。
47. **priority 倍率**：进入条件 `Number.isFinite(priorityMultiplier) && priorityMultiplier > 1`（`:274`）✓；**实测 3 条**（`curated.exact` 中带 `priority_multiplier` 的键 = `gpt-6-astra`/`gpt-6.1-sol`/`gpt-6-sol`，值均为 2）✓；两段式 `:328-354` 与文档代码块一致（`priorityPremium = (priorityMultiplier - 1) * (priorityBase + priorityLong)`，`:353`）✓；交集列 `Math.min` 夹紧 `:336-351` ✓；注释 `:310-317` 解释为何需要交集（「without it (queue rows, which carry no context-length subset at all) the overlap is 0 and the premium falls back to the short-context multiple」）✓；Fast 申报依据 `curated-overrides.json:98` 逐字含「The 2.5x figure in the Codex speed docs is a subscription-credit rate and must not be used here.」✓。
48. **DeepSeek 峰谷**：白名单 4 个（`:34-39`）、匹配 `lower.includes(name)`（`:180`）✓；`isDeepSeekOffPeak :199-207` 优先级（显式 `pricing_tier` > 北京周末 > UTC 峰段 > 默认峰值；非法时间戳 `return false` = 按峰值，fail closed）✓；窗口 `(hour >= 1 && hour < 4) || (hour >= 6 && hour < 10)` 逐字（`:206`）✓；`DEEPSEEK_WEEKEND_OFF_PEAK_FROM_MS = Date.UTC(2026, 7, 22, 16, 0, 0)`（`:186`）与 `+08:00` 固定偏移取 `getUTCDay()`（`:195`）✓；四字段 ×0.5（`:215-221`）✓；`acode` 豁免（`:213`）✓；价格表存峰值价的依据 `curated-overrides.json:5` 逐字含「Exact entries store peak rates. computeRowCost applies the time-of-use discount from each row's UTC hour_start.」✓（`:6` 的 `7.2 RMB/USD` 亦逐字核对）。
49. **`total_tokens` 不参与计价**：本人复算 `src/lib/pricing/` 全目录 `total_tokens` 命中 = **index.js 1 次**（位于 `:283` 注释「total_tokens stays the sum of the base columns」），`curated-overrides.json`/`seed-snapshot.json`/`matcher.js`/`litellm-fetcher.js` **各 0** ✓；同目录文件行数 378 / 439 / 172 与文档一致 ✓；`cursor-config.js:528` 的书面确认逐字（「see computeRowCost — it never reads this field」）✓；`CLAUDE.md:64` 同口径 ✓。
50. **价格表三层**：L0 `curated-overrides.json` 775 行 / 29754 B，顶层 5 键（`_meta`/`exact`/`alias`/`source_exact`/`fuzzy`），`exact` = **84**、`alias` = `{"auto":"composer-1"}`、`source_exact.acode` = **64**、`fuzzy` = **47**（数组）✓ 全部本人 `JSON.parse` 复算一致；L1 唯一上游 LiteLLM（`litellm-fetcher.js:10-11`）✓；L2 `seed-snapshot.json` 256869 B ✓；「没有用户可编辑的外置价格文件」成立（`pricing.toml` 全仓 0 命中）✓。
51. **seed 实测指标逐值复算**：顶层键 **2285**（`_meta` 1 + 模型 2284）✓；`_meta` 原文与文档引用**逐字相同**（`generated_at: 2026-05-26T11:12:29.761Z`、`kept_models: 2284`）✓；字段直方图 `input 2274 / output 2273 / cache_read 566 / cache_creation 188` ✓；**全部字段名集合恰为那 4 个**（白名单生效）✓；首 5 模型键与末 5 键与文档所列一致（末 5 键中 4 条为 `bedrock/*`，另一条为 `bedrock_mantle/openai.gpt-oss-safeguard-20b`——见建议修 P10）✓。换算比例 `566/2284 ≈ 24.8%`（约 75% 缺缓存读价）、`188/2284 ≈ 8.2%`（约 92% 缺缓存写价）✓。
52. **LiteLLM 换算与无推导**：`convertLitellmEntry :398-414`（四字段各 `× 1e6`，经 `roundToTenDecimals :394-396`，注释 `:391-393` 逐字含 `1e-7 * 1e6 = 0.09999999999999999`）✓；`buildLitellmPerMillionMap :418-427` 丢弃「一个价格字段都没有」的条目（`:413 return Object.keys(out).length ? out : null;`）✓；**无任何推导**（不存在 `read = input × 0.1` 一类）✓。
53. **L1 四级降级链**：磁盘缓存 `:128-137`（`source: "disk-cache"`，新鲜判定 `isFresh(stat, ttlMs)`）→ 上游 `:139-146`（`"upstream"`）→ 陈旧缓存 `:148-158`（`"stale-cache"`）→ seed `:160-163`（`"seed-snapshot"`）✓ 四步行号与文档表格**逐行一致**；`DEFAULT_TTL_MS = 24*60*60*1000` / `DEFAULT_FETCH_TIMEOUT_MS = 10_000`（`:12-13`）✓；写盘裁成 4 字段 + `_meta`（`:68-105`，`cached_at` 在 `:98`）✓；三处 `delete data._meta`（`:132`/`:162`、`index.js:48`）✓；`state.source` 初值 `"seed-snapshot:sync"`（`index.js:63`/`:103`）与成功后写入（`:83`）✓；`state.loaded`/`loadingPromise` 短路（`:73-94`）+ `revision += 1`/`negativeCache.clear()`（`:85-86`）✓。
54. **`ensurePricingLoaded` 调用口径复算**：本人 `rg` 复算 `ensurePricingLoaded\(` = **27 行**，与文档拆分**完全一致**（生产 2 = `serve.js:127`、`local-api.js:3340`；定义 1 = `index.js:73`；注释 4 = `index.js:4/41/42`、`serve.js:123`；测试 20 = `pricing.test.js` 16 处 × 列出全对 + `local-api-queue-cache.test.js:149/220` + `grok-parser.test.js:301` + `omo-parser.test.js:229`）；裸标识符口径 **36 行**，多出的 9 行与文档列出的 9 处**逐行一致**（`serve.js:9`、`local-api.js:61/3531`、`index.js:97/366`、`grok-parser.test.js:13`、`omo-parser.test.js:15`、`pricing.test.js:583/1287`）✓。`TOKENTRACKER_[A-Z_]+` 在 `src/` 复算 252 行（文档「250+ 命中」）✓，`TOKENTRACKER_OPENCODE_GO_LOCAL_ESTIMATE` 落在 `opencode-go-limits.js:20`（注释）与 `:51`（`LOCAL_ESTIMATE_ENV`）✓。
55. **matcher 0–7 级链**：`matcher.js:3-12` 的 8 步注释**逐字一致** ✓；每一级的行号与返回标签逐条核对通过（空值守卫 `:253-255` `"empty"`；早期 source 特例 `:265-279`；0 `:280-286`；1 `:291-300`；2 `:302-313`；3 `:315-322`；4 `:331-343`；5 `:345-351`；5b `:361-370`；6 `:372-383`；7 `:385`）✓；`SUFFIX_STRIP_PATTERNS` 九条在 `:14-31` ✓ 且仅在剥离后键存在时接受（`:348`）✓；5b 取字典序最小（`:366 if (best === null || key < best) best = key;`，注释 `:356-360`）✓。
56. **归一化规则表**：`buildDotRestoredModel :219-224`（只 `(\d+)-(\d+) → $1.$2`，`_` 完全不处理）✓；Claude 版本号重排 `:116-118`（正则与替换逐字，注释 `:112-115` 说明 3.x 必须保持不动）✓；provider 前缀剥离 `:89-95` ✓；`-latest` **无特殊处理** ✓；per-source 归一化表 `:194-204` **九条与文档列出的九条逐项一致**（含双 source 共用 `normalizeClaudeModel`）✓；注释 `:191-193` 逐字与文档引用一致 ✓；各归一化器要点（Antigravity 降级到 2.5 档 `:49-51`、`gpt-oss-120b*` `:55`；Cursor 只剔 `thinking/xhigh/high/medium/low`、`fast` 保留 `:146`，Grok 4.5 双 SKU `:138-140`；Zed 不剥 `fast` + Claude 点号连字符化 `:68-81`；WorkBuddy `auto` → `hy3-preview-agent` `:161-166` + `workbuddy-ai` 守卫 `:265-267`；Unsloth 哨兵 `:182-189`）**全部逐字成立** ✓。
57. **未命中三态与负缓存**：`lookupPricing` 恒返三字段（`:253-255`/`:385`）✓；`getModelPricing` 未命中写负缓存并返回 `ZERO_PRICING`（`index.js:112-132`、`:17`）✓，负缓存键按 source 分域 `${lookupSource}\0${model}`（`:120`）✓；`getModelPricingInfo` 三态（`:157-176`，`unpriced`/`free`/`priced`，本地推理与订阅源直接 `{status:"free", source:"local_inference"|"subscription"}` 在 `:158-160`）✓；按 revision 记忆化 + 浅拷贝（`:137-155`，注释 `:134-136` 逐字「Copies go out so a caller that mutates its object cannot poison the cache.」）✓。
58. **云端第二张表（本人独立复现）**：`dashboard/edge-patches/` 共 **28 个 `.ts`**，内嵌价格表者**只有 5 份** ✓；抽取块（沿用 parity 测试的正则）**五份字节相同**、**394 行**、`sha256[:16] = 37840dc53f7b6e2f` ✓；`MODEL_PRICING` 声明在 `:142`、块止于 `:332`、**119 键** ✓；`IFLYTEK_MAAS_MODEL_PRICING` 在 `:339-404`、**64 键** ✓；**双向差集复现**：「在 `curated.exact` 不在 edge」= **9 键**，键名与顺序与文档完全一致；「在 edge 不在 `curated.exact`」= **44 键** ✓；等价性逐条抽查（`:432` cline 命名空间、`:436-437` opus/sonnet-5.5 正则、`:441` → `claude-opus-4-6`、`:442` haiku、`:480`/`:481` grok、`:534` `ZERO_PRICING`、`:273` `grok-build` = 1.25/2.50/0.20、`:8` 权威成本集合、`:455` gpt-5.4、`:575` 云端 Pi 仅 2 成员、`:569-613` 云端 computeRowCost 无长上下文/priority 分支、`:1181` 逐行计价、`:1349` 取整到分）**全部逐字成立** ✓。
59. **权威成本与反例**：`grok`/`cline` 短路后 cline `:free` 与 gateway 命名空间仍按规范（`curated-overrides.json:43-49` 逐字含 `cline-free/*`/`cline-pass/*` 与「same convention as PI_SUBSCRIPTION_SOURCES」）✓；`test/grok-parser.test.js:302`（`getModelPricing("grok-build-free", { source: "grok" }).input === 0`）与 `:342-343`（`computeRowCost(rows[0]) === 0`）逐行确认 ✓；Command Code 用表价：`test/pricing.test.js:1248-1262`（3 模型 × `total_cost_usd ∈ [999, 0, undefined]` 恒等 `expected`）逐行确认 ✓。
60. **零边际成本与否定结论**：LM Studio/Pi/Cline 三类与 `curated` 显式 0（`:43-49`）✓；`batch` 无计价维度 ✓；cache 5m/1h 无区分字段（`matcher.js:410-412` 只有 `cache_creation_input_token_cost` 一个映射）✓；`audio_token`/`image_token`/`image_generation` 全 `src/` 0 命中 ✓；`subscriptions.js` **508 行**且无金额比较 ✓；`usage-limits.js` **4520 行**且 `computeRowCost` **0 命中** ✓（本人 grep 复算）。
61. **三处无测试守护的不一致**：`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 双份定义（`pricing/index.js:31` 零内部读取 vs `codex-rollout-parser.js:51` 生效）✓；推理折叠名单两套（计价 5 vs `scripts/audit-token-correctness.cjs:34-40` 的 2）✓；`PI_SUBSCRIPTION_SOURCES` 本地 4 / 云端 2 ✓。审计脚本边界描述亦全部成立（`:42-50` legacy 减 cache、`:54-60` 按 `(source, model, hour_start)` 取最新行、`:95` `baseParts` 四项、`:96-99` 推理 gating、`:100`/`:117-129` `invariant_failures`）✓。
62. **§8.6 两段演算 + 三锚点（独立重算）**：见 §E。

---

## C. 内部一致性

### C.1 §6 / §7 / §8 之间的口径

| 项 | 状态 | 证据 |
|---|---|---|
| reasoning 处理 | **一致** | §6.2.6（Codex 子集、计价置 0）与 §8.1（5 元门控、`:248-256`）指向同一集合；§6.3 对照表的「独立加成项 vs 子集标记」与两侧代码（`rollout.js:4935` vs `normalizeUsage` 不拆）一致 |
| 计价只用五列 | **一致** | §6.2.6 末段「喂给 `computeRowCost` 的都是五列之和、`total_tokens` 永不参与」§8.1「三条总纲」§8.4（`outcomes-engine` 钱用 `computeRowCost`、token 数用 `total_tokens`）三处同口径 |
| 时区回退 | **一致** | §6.1.7（写侧 UTC 半时、读侧 `rowDayKey` 才映射自然日）与 §7.3（`rowDayKey` 缺省 UTC / `getZonedParts` 缺省本机）为同一结论的两种表述；§7.3 的「两级回退」与 §7.6 的缓存键（`时区\|偏移\|价格修订`）一致 |
| 金额精度 | **一致** | §7.6（`toFixed(6)` 于 `local-api.js:2415`/`:2648`）§8.5（精度三处不同：`toFixed(6)` / 浮点 / 取整到分）与 §8.6（本地=云端 `0.15`）互不冲突 |
| 半小时桶实现数 | **口径未声明** | §6.1.7 说「**两份实现**」，§7.3 与 §12.D12 说「**三份**实现语义等价」。二者分别按「同名定义」与「同一数学的所有实现」计数，结论都可用，但同文未注明计数口径（见建议修 P8） |
| `token_count` 事件计数 | **口径未标注** | §6.2.3/§12.E2 用 **27802**（`last_token_usage`/`total_token_usage` 各自的出现对数），§6.2.9/§12.E5 用 **27822**（`token_count` 事件数）；同一语料（64 rollout / 179776 行）下两值差 20，未见解释（见建议修 P9） |

### C.2 与 §12.E 语料数字对照

| 文档处 | §12.E 处 | 状态 |
|---|---|---|
| §6.2.6 `reasoning > output` 零反例、65381 对、`reasoning > 0` 60715、相等 328、比值上界 1.0000 | E1 | **一致**（连 5 类计数字段的拆分 27802/27802/3259/3259/3259 = 65381 亦自洽） |
| §6.2.3 `total`/`last` 同现 100%、各 27802、3 次回退 −2965231/−3780242/−34082848、36 文件回退 | E2 | **一致** |
| §6.2.3 比值 ≈2128（9740950476478 vs 4578527502）、output 列 2156（35351865647 vs 16395234） | E3（给 output 列 2156） | **一致**；本人重算 9740950476478/4578527502 = 2127.75→2128、35351865647/16395234 = 2156.2→2156 ✓ |
| §6.2.7 `session_meta.payload.model` 0/78、`base_instructions.provenance.model` 30/78、`turn_context.model` 62/64 文件、20/21 一致、4 文件双值 | A18 | **一致**（且 58 + 2 + 4 = 64、58 + 4 = 62 自洽） |
| §6.2.9 32/64 文件、445 事件（12 条无 tier）、`default` 385 / `priority` 48、按前置归类 27822 → 19174/960/7688 | E5 + A 类补充登记 | **一致**（385 + 48 = 433 = 445 − 12；19174 + 960 + 7688 = 27822，均自洽） |
| §6.2.7/§6.2.10 `model/rerouted` 0 命中 | E6 | **一致** |
| §6.1.2 缓存别名 0 命中 / 1520 条非零 1164 | A2 + E6 | **一致** |
| §6.1.6 `<synthetic>` **【未能确认】** | **A3 已判定**（6 行、usage 非零 0 行）+ E6 | **冲突（P2，必修）** |
| §6.1.10「`obj.usage` 兜底路径数据在真值修复中会消失」 | E6「Claude usage 仅存在于 `message.usage`（顶层 `usage` 0 行）」 | **不冲突但口径不对称**：§6.1.2 对同类风险标了「【条件性风险，本机语料不成立】」，§6.1.10 未标；建议对齐 |

### C.3 表格 / 围栏 / 引用完整性

- §6–§8 内 markdown 表格 **45 张**，表头与分隔行的竖线计数**逐张一致**（脚本核对 `|` 计数，异常 0）；无跨表错位。
- 代码围栏：§6–§8 共 **82 个 ``` 标记 = 41 个代码块**，**首尾配对**（奇偶校验通过、无未闭合围栏），语言标注齐全（`js`/`json`/`text`）。
- §6–§8 无表格内换行、无未转义竖线（`\|\|` 与 `\|` 用法仅在需要时出现）。
- 章节交叉引用（§4.2、§6.1.8、§8.1、§8.3、§8.4、§8.5、§12.E）**目标章节均存在**；无指向不存在的 §编号。

---

## D. 问题清单

### 必须修（3 条）

**P1｜§8.2「`src/` 全目录 0 命中」被证伪，且与 §8.3 自相矛盾。**
原文（L2641）：「**没有 models.dev、没有 OpenRouter、没有用户可编辑的外置价格文件**（`src/` 全目录 0 命中；价格加载链只有上述三层）。」
实测：`src/` 内**大小写不敏感**检索 `openrouter` 共 **108 处**，其中 `src/lib/pricing/seed-snapshot.json` **95 个键**就是 `openrouter/...`（如 `openrouter/anthropic/claude-3.5-sonnet`、`openrouter/xiaomi/mimo-v2.5-pro`），另有 `matcher.js` 4 处（注释与示例）、`curated-overrides.json` 4 处（`*-free` 说明）、`rollout.js` 2 处。`models.dev` 与 `pricing.toml` 确为 0 命中。
关键矛盾：§8.3 的 5b 级规则**明确以 `openrouter/xiaomi/mimo-v2.5-pro` 为例**说明 provider 前缀剥离，§8.2 却说「没有 OpenRouter」。
改法：「价格加载链只有三层、上游唯一是 LiteLLM」保留；把证据句改为「`src/` 内不存在 OpenRouter **价格源**（无 models.dev / OpenRouter API / 外置价格文件；`pricing.toml` 0 命中）」，并注明 L2 seed 表内**含 OpenRouter 路由的 SKU 键**（95 个），它们只作为 LiteLLM 条目的键名参与 5b/6 级匹配。

**P2｜§6.1.6 的 `【未能确认】` 与 §12.A3 / §12.E6 直接冲突。**
§6.1.6（L1803）把「真实 Claude Code 是否会为 `<synthetic>` 行写非零 usage」列为未能确认，自述判定条件 =「在本机 `~/.claude/projects/**/*.jsonl` 中找 `"model":"<synthetic>"` 的行，看同行 usage 是否非零」。
而 §12.A3 已按**同一条件**判定 ✅：「本机 6 行 `message.model == "<synthetic>"`（分布于 4 个文件），其中 usage 缺失 0 行、**usage 任意字段非零 0 行**（12 个键各 6 次全为 0）」；§12.E6 复述同一实测。
改法：§6.1.6 升级为「已判定（本机无害）」，引用 §12.A3 / §12.E6，并把结论写成「本机不存在非零 usage 的 `<synthetic>` 行；该分支的展示层后果仍为条件性（仅在其他环境成立时）」。

**P3｜§7.5 表 #11 的执行顺序被写反。**
原文：「整队列 tmp+rename 原子重写（`:3945-3948`：临时文件 `writeFile` + `rename` 覆盖；既有行全部原样保留 `:3887`/`:3905`，非「只追加」）→ **重写后再追加**规范行（`:3925-3927`）与显式零行（`:3929-3943`）→ 偏移自 0 重放（`:3950-3962`）」。
源码实际顺序：先在内存 `output` 数组里保留既有行（`:3905`）→ **追加**每个受影响键的规范行（`:3925-3927`）与零值撤回行（`:3929-3943`）→ **再**于 `:3945-3948` 一次性 `writeFile` + `rename` 原子落盘 → `:3950-3962` 偏移清零 → `:3972 return true`。**重写之后没有任何追加**。所引行号本身即可证伪叙述（`:3925`/`:3929` 早于 `:3945`）。
改法：改为「先在内存重建（保留既有行 + 追加规范行 + 追加零值撤回行）→ 再原子落盘（`:3945-3948`）→ 偏移清零」。

### 应修（3 条）

**P4｜§6.2.3 的两处告警位赋值未加条件限定。**
原文：「**重复快照**时 `total` 命中已知流头 → 返回 `null`（**不重复计**）并置 `sawInterleaved`」；「**`total - last` 不命中任何流头但仍可减** → 置 `sawDivergentCumulative` 后返回 `last`」。
源码：重复命中时**仅当**该流头签名 ≠ 当前 `lastTotal` 签名才 `state.sawInterleaved = true`（`codex-token-usage.js:111-116`）；第二分支置 `sawDivergentCumulative` 还要求 `canonicalUsage(state.lastTotal)` 非空（`:135`）。两处均为条件赋值。
改法：补「当且仅当当前活跃流头与命中流头不同」（同理「且已有活跃累计基线」）。测试 `test/codex-token-usage.test.js:35` 的 `sawInterleaved === true` 只在交错形态下成立，不构成反证。

**P5｜§7.7 表把 `:3489` 当作 `fs.appendFile` 写点。**
原文：「常规桶 / 项目桶写入 \| 纯 `fs.appendFile`，单行 JSON + `\n` \| `src/lib/rollout.js:3483`、`:3489`、`:20031`」。
实际：`:3483` 是主队列 `appendFile` ✓、`:20031` 是撤回 `appendFile` ✓，但 `:3489` 是 `async function enqueueTouchedProjectBuckets({` 的**函数定义行**；项目队列的 `appendFile` 在 **`:3536`**（`await fs.appendFile(projectQueuePath, toAppend.join("\n") + "\n", "utf8");`）。文档 §12.C3 用的正是 `:3536`，两处不一致。
改法：`:3489` → `:3536`。

**P6｜§8.1 的注释行号错位。**
原文：「源码注释解释了三家折叠的理由（`OmO, like Codex, reports reasoning as a subset of output … Cline is the same …`，`:249-252`）」。
该注释实际位于 `src/lib/pricing/index.js:243-247`；`:249-252` 是 `reasoningIncludedInOutput` 的四个 `||` 代码行（`row.source === "codex" ||` 等）。
改法：注释引用改 `:243-247`（或写「`:243-253` 注释 + 集合」）。

### 建议修（4 条）

**P7｜§8.4 的 dev/mock 通道行号范围含无关行。**
原文：「dev/mock 通道（`dashboard/vite.config.js:508-514`、`:1052-1057`、`:1067` require 同一 pricing 模块）」。
`:508-514` 确为 `createRequire` + `__viteRequire(path.resolve(REPO_ROOT, "src/lib/pricing"))` + 解构 `computeRowCost`；`:1052-1057` 确为 mock 的按模型计价；但 `:1067` 是响应里的**硬编码** `rates_per_million_usd` 元数据，不含 require。
改法：删 `:1067`，或把该行改述为「另有一条硬编码展示价（`:1067`）」以与「不复制价格表」的结论并写。

**P8｜半小时桶「两份 / 三份实现」的计数口径未声明（见 C.1）。**
建议在 §6.1.7 或 §7.3 任一处加括注：「按同名函数计 2 份（`rollout.js` 权威 + `claude-categorizer.js` 副本）；按同一取整数学计 3 份（另含 `sync.js:4538-4549` 的 `toGrokRepairHalfHourStart`，函数名不同）」，使 §12.C20 与 §12.D12 不再表面冲突。

**P9｜`token_count` 事件数 27802 / 27822 需并列标注口径。**
建议在 §6.2.9（或 §6.2.3）注明：「27822 = `token_count` 事件数；27802 = `last_token_usage`/`total_token_usage` 各自的出现对数，差额 20 条为该类事件中 usage 对象缺失/不可规范化的行」，或补一句「两个计数口径不同，不可互相推导」。

**P10｜§8.2 seed 末 5 键的措辞。**
原文「末 5 键为若干 `bedrock/*` 条目」；实测末 5 键为 `bedrock_mantle/openai.gpt-oss-safeguard-20b`、`bedrock/us-east-1/zai.glm-5`、`bedrock/us-west-2/zai.glm-5`、`bedrock/us-gov-east-1/anthropic.claude-haiku-4-5-20251001-v1:0`、`bedrock/us-gov-west-1/anthropic.claude-haiku-4-5-20251001-v1:0`。建议改为「末 5 键均为 Bedrock 系条目（含 1 条 `bedrock_mantle/*`）」。

---

## E. 演算复算（§8.6，全部独立重算，未复用文档算式）

统一 token 组：`input = 10000`、`cached = 200000`、`output = 5000`、`cache_creation = 1500`。价格取自 `seed-snapshot.json` 原文，本人按 `× 1e6` 换算并按 `pricing/index.js:257-264` 的算式重算。

| 演算 | 文档值 | 本人复算 | 分项（本人算） |
|---|---|---|---|
| A `claude-opus-4-6` / `source=claude` / r=0 | `0.284375` | **`0.284375`** ✓ | 50000 + 125000 + 100000 + 9375 + 0 = 284375 |
| B `gpt-5.4` / `source=codex` / r=0 | `0.150000` | **`0.150000`** ✓ | 25000 + 75000 + 50000 + 0 + 0 = 150000 |
| 对照 B′ `gpt-5.4` / `source=gemini` / r=800 | `0.162` | **`0.162`** ✓ | 多收 `800 × 15 / 1e6 = 0.012` |
| 对照 `gemini-3-pro-preview` 基准 / 加 800 推理 | `0.12` / `0.1296` | **`0.12` / `0.1296`** ✓ | `800 × 12 / 1e6 = 0.0096` |
| 锚点 1 Qwen3.8 Flash 四桶各 100 万 | `0.836` | **`0.836`** ✓ | `0.15 + 0.47 + 0.016 + 0.2` |
| 锚点 2 GPT-6 Astra 四桶各 100 万 + 推理 50 万（Codex） | `73.5` | **`73.5`** ✓ | `10 + 50 + 1 + 12.5 + 0`；同参数 `source=gemini` 复算 = **`98.5`** ✓（差 25） |
| 锚点 3 Command Code 小请求 | `0.0009675` | **`0.0009675`** ✓ | `100×3 + 20×15 + 600×0.3 + 50×3.75 = 967.5` |

附带核对：三条价格原文（`claude-opus-4-6` = `{5e-6, 2.5e-5, 5e-7, 6.25e-6}`、`gpt-5.4` = `{2.5e-6, 1.5e-5, 2.5e-7}`（无 cache_creation）、`gemini-3-pro-preview` = `{2e-6, 1.2e-5, 2e-7}`）与文档引用**逐字相同**；三者在 `curated.exact` 中**确实缺席**（本人差集复算确认，故走 `litellm:exact`）✓；`claude-sonnet-4-6` 的 seed 原值 `{3e-6, 1.5e-5, 3e-7, 3.75e-6}` → 3/15/0.3/3.75 与 edge `:443` 命中项的 `{input:3, output:15, cache_read:0.3, cache_write:3.75}` **同值** ✓。

---

## F. 未闭合项（承接现状，不新增）

§6–§8 自标的 `【未能确认】` 共 4 处，本人复核后的处置：

1. §6.1.6 `<synthetic>` 非零 usage —— **可结案**（§12.A3/E6 已判定，见 P2）。
2. §7.4 表 4c / §7.7 未闭合项 #2 的 `codexHashes` 事件去重哈希输入 —— 仍为 `【未能确认】`，文档未变；本轮未展开。
3. §6.2.10「`codex-rollout-parser.js` 不影响统计」的强结论依赖 `sync.js:198` 借用路径是否永不触发 —— 仍取决于运行时游标状态，源码层不可判；文档判定条件（读真实 `cursors.json` 看迁移键）仍成立。
4. §8.5 的 4 项未闭合（LiteLLM 上游时效、`curated` 注释引用的第三方价页、edge 五文件部署状态、限额缓存另外三份的落盘目录）—— 本轮未展开，判定条件不变。

另记一条本轮**新增可结案**项：§8.6 的两段演算、三个锚点、`mergeMaxTotals/setMaxTotal` 调用点计数、`toUtcHalfHourStart` 分布、`ensurePricingLoaded` 调用口径、seed 直方图与 edge 双向差集，均已在 R7-3 独立复算通过，可从「待核」转为「已核」。

---

## G. 复核动作台账（可复现）

- 引用抽取脚本：`%TEMP%\tt-check\refs.js` / `refs2.js`（段级上下文归属）。
- 行号/内容核对：`%TEMP%\tt-check\dump.js`（按 (文件, 行区间) 列表逐行输出）+ 14 份 `spec*.json` 范围清单、`out*.txt` 原始输出。
- 复算脚本：`%TEMP%\tt-check\recalc.js`（§8.6 两段演算 + 三锚点）、`edge.js`/`edge2.js`（edge 块哈希、键数、双向差集）、`final.js`/`final2.js`（调用口径、目录检索、常量路径）。
- 全程只读：无 git 写操作；`git -C <repo> rev-parse HEAD` 复核前后均为 `b19d27b0ccd59ce621f4fa17207b06358e67987c`。

**问题计数汇总：引用问题 5 条（P1、P3、P5、P6、P7）、机制性表述偏差 1 条（P4）、口径未声明 2 条（P8、P9）+ 措辞 1 条（P10）；其中必须修 3 条（P1、P2、P3——P2 属事实结论冲突）、应修 3 条、建议修 4 条。核验条数 62、演算复算 7 段全部逐值一致、引用点 1100 个真越界 0。**
