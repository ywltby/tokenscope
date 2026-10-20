# 复核报告 R1-B：S2-framework-claude-codex.md

样本：`C:\Users\admin\AppData\Local\Temp\tt-research`（`xiufengsun/TokenTracker`）
HEAD 复核：`b19d27b0ccd59ce621f4fa17207b06358e67987c`（`b19d27b chore(release): v1.3.14 (#792)`）——与分片声明一致。
行号口径：全部用 `read` 工具或 `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`（权威口径，`rollout.js`=24932 行，实测一致）。

## 结论

**该分片可用，但需修订**：抽查 46 条断言中仅 1 条实质错误（B8 的族数不完备、族内归属错），另有 8 条不完整（含 B1、B3 各一处措辞/覆盖面问题），4 条无法验证；行号准确率 96.7%（29/30）。**B1 的两侧不冲突**——分片结论方向正确，但 B8 必须改口径、B3 必须补两个新发现的 `endOffset = st.size` 站点。

## 抽样统计

- 抽验断言数：**46**（其中 E=1, I=8, U=4，其余 33 条通过）
- 代码块引用行号核对：**30 条**（要求 ≥12）
- 实际打开过的源码文件清单（13 个，全部逐行读过，非 grep 推断）：
  - `src/lib/rollout.js`（分片涉及的 40+ 区段）
  - `src/lib/pricing/index.js`（1-378 全量）
  - `src/commands/sync.js`（190-205 / 243-292 / 676-812 / 890-900 / 4430-4524 / 5740-5872 / 6228-6258 / 6470-6600 / 6840-6950）
  - `src/lib/local-api.js`（170-300）
  - `src/lib/async-queue-reader.js`（1-117 全量）
  - `src/lib/jsonl-lines.js`（1-100）
  - `src/lib/claude-user-identity.js`（1-56 全量）
  - `src/lib/claude-categorizer.js`（1-45 / 55-82 / 1170-1201 / 1259-1330）
  - `src/lib/claude-config.js`（1-16 / 46 / 88 / 107-116 / 252-256）
  - `src/lib/codex-model-attribution.js`（1-128 全量）
  - `src/lib/codex-usage-record.js`（1-195 全量）
  - `src/lib/codex-service-tier.js`（1-49 全量）
  - `src/lib/codex-rollout-parser.js`（1-45 / 51 / 692-740 / 995-1032）
  - `src/lib/cursor-store.js`（1-96 / 252 / 1711 / 1788-1800）
  - `src/lib/session-analytics.js`（99-104 / 818-836 / 2655-2665）
  - `test/claude-fork-conversations.test.js` / `test/claude-reasoning-usage.test.js` / `test/rollout-parser.test.js`（全文）
  - `CLAUDE.md`（54-76 行）

## B 组重点判定

### B1（最重要）：两侧**不冲突**。Codex 的 reasoning 是 output 的**子集**，分片措辞把「列独立」误读成了「语义独立」

**解析侧实测**（`src/lib/rollout.js:4851-4879`）：

```js
// src/lib/rollout.js:4851
function normalizeUsage(u) {              // 注：函数体不截断，见下行
```

```js
// src/lib/rollout.js:4853-4877（逐字）
  for (const k of [
    "input_tokens", "cached_input_tokens", "cache_creation_input_tokens",
    "output_tokens", "reasoning_output_tokens", "total_tokens",
  ]) {
    const n = Number(u[k] || 0);
    out[k] = Number.isFinite(n) && n >= 0 ? Math.floor(n) : 0;
  }
  ...
  // Preserve the reported total for compatibility with older Codex / Every
  // Code shapes where output_tokens can exclude reasoning or some component
  // fields are absent. ...
  out.input_tokens = Math.max(0, out.input_tokens - out.cached_input_tokens);
  return out;
```

- **没有** `out.output_tokens -= out.reasoning_output_tokens` 这一行；**没有任何**对 `reasoning_output_tokens` 的算术改写。原文明确写着「Preserve the reported total for compatibility … where output_tokens **can exclude** reasoning」——即**上游语义默认 output 含 reasoning**，源码只是原样保留。
- 与 Claude 侧形成对照（`src/lib/rollout.js:4930-4939`，逐字）：Claude **有**显式拆分 `output_tokens: outputTokens - reasoningTokens`，并配注释 `Claude rows price reasoning separately; it is already included in usage.output_tokens.`（`:4934`）。两侧写法相反，正说明「不拆」= 含在内。

**计价侧实测**（`src/lib/pricing/index.js:243-264`）：

```js
// src/lib/pricing/index.js:243-256（逐字）
  // OmO, like Codex, reports reasoning as a subset of `output` (its own
  // usage.cost bills no separate reasoning component), so charging it again
  // here would double-bill every reasoning token. Cline is the same: ...
  const reasoningIncludedInOutput =
    row.source === "codex" || row.source === "acode" || row.source === "every-code" ||
    row.source === "omo" || row.source === "cline";
  const reasoningCost = reasoningIncludedInOutput
    ? 0
    : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
```

**token 流向（多算/少算判定）**：

| 路径 | output 列 | reasoning 列 | baseCost 实际计费 | 是否双计 |
|---|---|---|---|---|
| Codex（走 `normalizeUsage`） | 含 reasoning 的原始值 | reasoning（output 的子集） | `out*P` + `reasoningCost = 0` | 否 |
| Claude（走 `normalizeClaudeUsage`） | 已剥掉 reasoning | reasoning | `out*P` + `reasoning*P` | 否 |

即两侧**在同一语义约定（subset ⇒ 不重复计价）下自洽**：解析侧不拆列 + 计价侧置 0，是配套的一对；Claude 侧拆列 + 计价侧照价，也是配套的一对。**没有多算，也没有少算**。

**判定**：

1. `reasoningIncludedInOutput` 这个变量名本身就是权威判据——它是「reasoning 已含于 output」，不是「reasoning 未被计入 output」。
2. 分片 §C.3.2 映射表「`reasoning_output_tokens` | 直取，且已经包含在 `output_tokens` 里（上游语义）」**正确**；但 §0 摘要第 6 条「**reasoning 不被并入 output**，而是独立成列」**措辞错误**——正确表述是「reasoning **不被拆出** output（保持子集），同时**另存为独立列**」。子树据「独立成列」推断出「并入/不并入」的对立，是一个假冲突。
3. 分片在 §0 第 6 条已主动把 CLAUDE.md 第 60-61 行的「fold them into output_tokens for cost」判为"上游原生语义"——这个辩解方向对（`CLAUDE.md:60` 原文 `(Codex/acode/every-code/OmO/Cline fold them into output_tokens for cost)`，实测一致），但结论句写反了。
4. **反面证据检查**：若真有冲突，`reasoning_output_tokens` 应在某处**独立加进** baseCost。实测 `computeRowCost` 只有 3 处 reasoning 相关项（`:254` baseCost、`:297` longContextPremium、`:325` priorityPremium），三处**全部**用同一 `reasoningIncludedInOutput` 门控——无一处漏掉、无一处反向。**不存在叠加多计的路径。**

**可复现的判定依据**：`test/claude-reasoning-usage.test.js:45-47`（实测逐字）用 `computeRowCost({...row, output_tokens: 518, reasoning_output_tokens: 0})` 与拆分后的行做 `< 1e-12` 等值断言，证明「五列之和」是唯一计费输入、列分布可换。该测试只覆盖 Claude；Codex 侧无等价测试（缺口，见 I4）。

---

## 错误（E）—— 必须修

- **[E1] 分片 §0 第 2 条 / §A.1.3：「45 个 parser 存在**三族**签名」/ 实际存在 **5 类运行期返回字面量**，且族 3 的归属唯一性断言为假。**
  - 实测（脚本对 45 个函数体逐个提取 `return {` 后的键名，去重后）：
    - `filesProcessed,eventsAggregated,bucketsQueued,projectBucketsQueued` → `parseRolloutIncremental@705`、`parseClaudeIncremental@1239`、`parseGeminiIncremental@1409`、`parseOpencodeIncremental@1562`、`parseOpenclawIncremental@1922`、`parseAntigravityIncremental@21394`
    - `messagesProcessed,eventsAggregated,bucketsQueued,projectBucketsQueued` → `parseOpencodeDbIncremental@6027`、`parseQoderDbIncremental@6398`、`parseQoderNewIncremental@6788`
    - `recordsProcessed,eventsAggregated,bucketsQueued` → 21 个函数（含 `parseClaudeScienceIncremental@7137`、`parseTraeCnApiIncremental@22634`）
    - `recordsProcessed,total,eventsAggregated,bucketsQueued` → **`parseCursorApiIncremental@7270`**（多出 `total`）
    - `recordsProcessed,total,eventsAggregated,bucketsQueued,skippedRows` → **`parseTraeCnApiIncremental@22791`**
    - `recordsProcessed,eventsAggregated,bucketsQueued,recordsSkipped,estimatedRecords,unpricedRecords,errors` → **`parseTraeIncremental@22163`**
    - `recordsProcessed,eventsAggregated,bucketsQueued,usageClaims,recentOtelUsageEvents` → **`parseCopilotIncremental@17944`**
    - `recordsProcessed,eventsAggregated,bucketsQueued,deferredMigrations` → **`parseDshIncremental@24029`**
    - `recordsProcessed,eventsAggregated,bucketsQueued,dbErrors` → **`parseCopilotAppDbIncremental@19538`**
    - `recordsProcessed,eventsAggregated,bucketsQueued,projectBucketsQueued` → `parseDevinIncremental@14226`、`parseDroidIncremental@15178`、`parseOmpLikeIncremental@15818`、`parsePiLikeIncremental@16360`、`parseGrokBuildIncremental@20551`
    - `recordsProcessed,eventsAggregated,eventsAggregated,bucketsQueued,projectBucketsQueued` → `parseGrokBuildIncremental@20551`（同址重复键）
    - `recordsProcessed,messagesProcessed,…,schemaMigrated,cursorUnchanged` → `parseCommandCodeIncremental@24664`
  - 分片 §A.1.3 只列了族 1（4 键 filesProcessed）、族 2（`parseClaudeScienceIncremental` + `parseOmpLikeIncremental`）、族 3（`parseCommandCodeIncremental` 7 键），**遗漏了 8 类扩展签名**，并把 `parseClaudeScienceIncremental`（3 键）与 `parseOmpLikeIncremental`（4 键）当作同族。
  - **修正建议**：把「三族签名」改成「**5 类签名骨架 / 12 种字面量**：`filesProcessed(4键)`、`messagesProcessed(4键)`、`recordsProcessed(3键)` + 若干带扩展键的变体（`total`、`skippedRows`、`recordsSkipped`、`usageClaims`、`deferredMigrations`、`dbErrors`、`schemaMigrated/cursorUnchanged`）」，并明确「**没有**统一契约，连族内键数都不一致」。

## 不完整（I）—— 建议补

- **[I1] B1 表述需修正（详见上文 B1 判定）**：§0 摘要第 6 条「reasoning 不被并入 output」应改为「reasoning 不从 output 拆出（保持子集），且独立成列」；否则读者会得出「计价侧置 0 = 少算」的错误推论。分片 §C.3.2 映射表本身是对的，需与摘要对齐。
- **[I2] B3 覆盖面不全（新发现）**：`grep -n 'endOffset = st.size'` 在 `rollout.js` 命中 **3 处**，不是 1 处：
  - `src/lib/rollout.js:2291` —— `scanRolloutProjectFileContexts`，只扫 `turn_context/session_meta + cwd`，注释与返回值都走 `committedEndOffset`（`return { endOffset: committedEndOffset, ... }`）：**无行边界保护，但不计 token，风险低**。
  - `src/lib/rollout.js:2682` —— 同为项目上下文扫描函数，同样返回 `committedEndOffset`（`:2765`）：**同上**。
  - `src/lib/rollout.js:2793` —— `parseClaudeFile`，返回 `{ endOffset, eventsAggregated }`（`:2889`）且 `cursors.files[key].offset = result.endOffset`（`:1194`）：**唯一落到 token 游标的站点，即分片指出的那处**。
  - 分片 §C 第 10 条（`:4838` extractTokenCount 区段）的 `:2682` 引用实际是项目上下文扫描，与 token 无关；分片把它并入 Codex 主线会误导。建议 §A.3.3 注明「`endOffset = st.size` 共 3 处，其中 2 处只推进"项目上下文扫描位"、1 处推进 token 游标」。
- **[I3] B3 结论需收窄**：分片称损失「永久」。实测 `parseClaudeFile` 的 `endOffset` 在 `st.size` 处，只有当「同步瞬间读到未闭合尾行 **且** 该行 JSON.parse 失败」时才发生；`readline` + `crlfDelay:Infinity` 会把未闭合尾行 yield 出来。此时 `catch (_e) { continue; }`（`:2843-2847`）吞掉该行，游标已推进到 `st.size` → **该行字节被跳过且不会重读**（除非该文件 inode 变化或截断触发 `:501` 式全量重扫）。原描述方向正确，但应补一句「该文件下一次 inode 变化/截断时会被全量重扫，非严格永久」。
- **[I4] 缺 Codex 侧的 reasoning 计费对等测试**：`test/claude-reasoning-usage.test.js` 只锁 Claude。Codex 的「reasoning ⊂ output 且不双计」无回归测试，`pricing/index.js:248-256` 的 source 白名单一旦漏加某个 `*-code` 变体，静默双计。建议补一条 `computeRowCost({source:"codex", output_tokens: 518, reasoning_output_tokens: 163})` 与 `reasoning_output_tokens: 0` 的等值断言。
- **[I5] §A.2.3「全库检索 `bad_lines|badLines|bad_line` 零命中」口径需限定**：实测在 `src/` 全树零命中，结论成立；但分片同时给出的 `countMalformedCodexProjectQueueRows`（`sync.js:6436`，实测 `:5824` 处被调用，用于"重建前发现畸形 project 行则放弃修复"，**不影响用量**）可以顺手闭合，不必留作「【未能确认】」。
- **[I6] §A.3.2 游标字段表缺一个前提**：Claude 的 `claudeCwd/projectRef/projectKey` 三项**只在 `projectEnabled` 分支才写入**（`src/lib/rollout.js:1198-1205` 的 `...(projectEnabled ? {...} : {})`）。项目追踪关闭时这三个字段根本不存在，而分片表格把它写成无条件字段。同理 Codex 的 `projectOffset/projectFileContext` 受 `codexProjectFastPath` 分支影响（`:656-665`）。
- **[I7] §A.5.2「两份实现逐字一致」需加限定**：`claude-categorizer.js:1175-1191` 与 `rollout.js:4173-4190` **算法一致但文本不一致**——前者把 `new Date(...).toISOString()` 写在一行（`:1180-1190`），后者先赋 `const bucketStart` 再 `return bucketStart.toISOString()`（`:4178-4189`）。「逐字一致」不成立，应改为「语义等价（同一取整规则），无等价性测试锁定」。分片的**结论**（无测试锁定）经实测成立：`test/` 下 `toUtcHalfHourStart` 只被 `sync-droid-dup-session-repair.test.js` 与 `trae-entitlement.test.js:130` 引用，均针对 `rollout.js` 的导出，`claude-categorizer.js` 的那份无人测。
- **[I8] §B.6.1 一处注释行号漂移**：分片称 `:1114-1120 注释`，实测该注释块为 **`:1115-1120`**（`:1114` 是空注释分隔行 `//`）。属 1 行级小漂移，建议顺手改。

## 无法验证（U）

- **[U1] §B.2.3「跨 provider 若使用 `cache_write_input_tokens`，Claude 路径会静默丢缓存写」**：代码事实**成立**（`rollout.js:4927-4928` 只读 `cache_creation_input_tokens`，而 `codex-token-usage.js:25-27` 有 `?? value.cache_write_input_tokens` 别名兜底，逐字实测一致）。但「这些 provider 是否会这样写」需要真实样本或合成 fixture 的出现率统计——**本次无样本，无法判定实际丢量规模**。
- **[U2] §B.4.2「真实 Claude Code 是否为 `<synthetic>` 行写非零 usage」**：实测 `<synthetic>` 在 `src/` 命中 3 处（`session-analytics.js:101` / `:622` / `:1969`），**解析层零特判**的结论成立；但**是否存在带非零 usage 的 `<synthetic>` 行**需查本机 `~/.claude`，本次未取样本（分片已自行标注）。补：`session-analytics.js:622`、`:1969` 两处注释也提到 `<synthetic>` 是 "not a billable model"，分片只引了 `:101`。
- **[U3] §C.5.3 / §C.5.1 的语料统计（1213 文件 167 带 `thread_settings_applied` 覆盖 25%；0/10310 `session_meta` 带 model；814 default / 58 priority；5832 文件 0 命中 `model/rerouted`）**：这些是维护者对**本机语料**的观测数，无法从源码判定。代码侧**结构与规则**全部核对通过（见「复核到的行号抽样」），统计数字**存疑但不可证伪**。
- **[U4] §C.6.1「零命中」断言的性质**：`codex-context-breakdown.js` / `codex-rollout-parser.js` 不写队列——实测二者对 `queuePath`/`enqueueTouchedBuckets` 确无命中，「不写队列」成立；但「**不影响 token 统计**」在 `sync.js:198` 与 `:6246-6252` 的借用路径下需要理解「一次性修复是迁移而非日常解析」这一前提（分片已给出该前提，属**表述充分**）。真正无法从代码判定的是「该修复是否在某些安装上永不触发」——需运行时状态。

## 行号漂移统计

- 抽样中**代码块引用行号正确比例：29/30 = 96.7%**（另 1 处为注释行号小漂移，未计入代码块）
- 发现漂移的位置：仅 §B.6.1 的注释引用（分片写 `:1114-1120` → 实际 `:1115-1120`）
- 未发现 CHECKLIST §0 所警告的「累计 15 行整体偏移」：本报告全部结论基于 `read` / `ReadAllLines(UTF8)` 口径，与分片口径一致（`rollout.js` 两边均为 24932 行）

## 复核到的行号抽样（证明真的打开过）

**A 组（框架）**

- `src/lib/rollout.js:24932` → 文件总行数（`ReadAllLines(UTF8).Count`）；`942266` 字节（`Get-Item`）→ 分片 §0 数据一致
- `src/lib/rollout.js:251/954/1242/1412/1654/5761/6218/6552/7051/7172/7472/7900/8886/9528/9723/10164/10803/11824/12200/12650/13267/13691/13938/14207/14604/15137/15545/15753/16087/16100/16319/16689/16699/16775/17050/17080/17878/18579/19381/20084/20800/22033/22598/23678/24458` → 脚本枚举 `^(async )?function parse\w*Incremental\(` 精确 **45** 个，与分片 §A.1.2 表**逐行一致**
- `src/lib/rollout.js:24675` → `module.exports = {`
- `src/lib/rollout.js:24866-24880` → `// Exposed for regression tests covering cache-token accounting.` / `bucketKey,` / `toUtcHalfHourStart,` / `totalsKey,` / `claudeMessageDedupKey,` / `normalizeClaudeUsage,` / `groupBucketKey,`
- `src/lib/rollout.js:1239` → `return { filesProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };`
- `src/lib/rollout.js:7137` → `return { recordsProcessed, eventsAggregated, bucketsQueued };`
- `src/lib/rollout.js:16084` → `return { recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };`
- `src/lib/rollout.js:24664-24672` → `return {` / `recordsProcessed,` / `messagesProcessed: currentByKey.size,` / `schemaMigrated,` / `cursorUnchanged,` / `};`
- `src/lib/rollout.js:24670-24671` → `schemaMigrated` / `cursorUnchanged`（`grep` 全 `src/lib` 仅此 2 命中 + `:24473`、`:24656` 赋值处）
- `src/lib/rollout.js:16087` → `async function parseOmpIncremental(options = {}) {`（薄壳逐字相符）
- `src/lib/rollout.js:671-679` → `if (cb) { cb({ index: idx + 1, total: totalFiles, filePath, filesProcessed, eventsAggregated, bucketsQueued: touchedBuckets.size, }); }`
- `src/lib/rollout.js:72` → `async function mapConcurrent(items, concurrency, mapper) {`；`:70` `const FILE_METADATA_CONCURRENCY = 32;`
- `src/lib/rollout.js:179` → `function isRolloutFileName(name) {`；`:186` `function directoryInventoryStatKey(st) {`
- `src/lib/rollout.js:195-200` → `// Collect rollout-*.jsonl at ANY depth ... (issue #187)`；`:201` `async function listRolloutFilesDeep(dir) {`
- `src/lib/rollout.js:4173` → `function toUtcHalfHourStart(ts) {`；`src/lib/claude-categorizer.js:1175` → 同名函数（算法同、文本不同）
- `src/lib/rollout.js:2672-2680/2691-2693/2780-2791/2803/2810/2841/2851/2857-2858/2861-2868/2889` → Claude 逐行解析全链，**逐行一致**
- `src/lib/rollout.js:3627-3651` → `function normalizeHourlyState(raw) {` … `version: 3, buckets, groupQueued, updatedAt`
- `src/lib/rollout.js:3328-3345` → 队列行 `JSON.stringify({ source, model, hour_start, input_tokens, …, conversation_count })`（逐字一致）
- `src/lib/rollout.js:1192-1206` → `cursors.files[key] = { inode, offset: result.endOffset, claudeForkIndexed: true, …, ...(projectEnabled ? { claudeCwd, projectFileContext, projectRef, projectKey } : {}) }`
- `src/lib/rollout.js:1233-1237` → `cursors.claudeForkAliases = forkAliases;` / `allHashes.length > 100_000 ? allHashes.slice(allHashes.length - 100_000) : allHashes`
- `src/lib/rollout.js:1004-1014` → fork 折叠净值修正族循环（只改 `conversation_count`，不动 token；有 `< count - 1` 下界保护）
- `src/lib/rollout.js:633-640` / `:1193-1203` / `:1084` → 游标字段表
- `src/lib/async-queue-reader.js:51-57` → `function parse(line, target) { … target.set(entry.key, entry.row); }`（同键后者覆盖前者 ⇒ last-row-wins）
- `src/lib/local-api.js:202-211` → `const readQueueData = createAsyncQueueReader({ … key: \`${row.source}\|\|${row.model}\|\|${row.hour_start}\` … })`；`:213-225` `function rowDayKey(row, timeZoneContext) { … return hs.slice(0, 10); }`
- `src/lib/pricing/index.js:248-264` → `reasoningIncludedInOutput` 白名单 + `reasoningCost = … 0 …` + `baseCost` 公式
- `src/lib/pricing/index.js:297-306` / `:325-351` → `longReasoning` / `priorityReasoning`（同样门控）
- `src/lib/pricing/index.js:31` / `:375` → `const OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000;` / 仅出现在导出列表（本文件内**零内部使用**，实测）
- `src/lib/jsonl-lines.js:3-11` / `:29` / `:34` / `:91` → 头部注释、`physicalJsonlRecords(input, { invalidUtf8 = "throw", maxPhysicalBytes = Infinity, maxRecordBytes = Infinity } = {})`、policy 校验、`yield { line, utf8Valid: true, oversized: false, physicalBytes, terminated: true };`

**B/C 组（Claude / Codex 主线）**

- `src/lib/rollout.js:4914-4919` → `function claudeMessageDedupKey(obj)`：`return reqId ? \`${msgId}:${reqId}\` : msgId;`（B5 去重键成立）
- `src/lib/rollout.js:4921-4939` → `normalizeClaudeUsage` 五桶映射（逐字一致，含 `Math.min(outputTokens, …)` 封顶）
- `src/lib/rollout.js:4964-4968` → `function toNonNegativeInt(v) { … return Math.floor(n); }`
- `src/lib/rollout.js:4945-4958` → `function isAllZeroUsage(u)` 六桶判零
- `src/lib/claude-user-identity.js:5-25` / `:31` / `:44` → `noteClaudeFork` / `claudeUserIdentity`（含 `u:` 前缀与环折叠）/ `/[\//]subagents[\//]/` 排除 / `if (obj?.type === "user")`
- `src/lib/claude-config.js:279`（总行数）/ `:7-8` `const DEFAULT_EVENT = "SessionEnd"; const CLAUDE_USAGE_EVENTS = ["Stop", DEFAULT_EVENT];` / `:10/14/46/88/116` 五个函数 / `:107-109` CodeBuddy 注释 / `:252-256` `.bak.<ISO>` 备份 → **§B.6.3「不是项目归属模块」成立**
- `src/commands/sync.js:676-684` → `// … Claude UNIONS every allowed install instead of taking resolveAllWin32Paths' single pick …`；`:685-693` 逐字一致；`:700-703` `appendUniqueDirs`
- `src/commands/sync.js:782-793` → `resolveInstallPaths({ …, requireAnyChild: ["sessions","archived_sessions"], union: true })` + `sources.push({ source:"codex", sessionsDir: path.join(codexPaths.native,"sessions"), inventoryCacheKey: "codexDayInventoryCache" })`
- `src/commands/sync.js:894` → `rolloutFiles.push({ path: filePath, source: entry.source });`
- `src/commands/sync.js:198` → `const { extractTokenCount } = require("../lib/codex-rollout-parser");`
- `src/commands/sync.js:6246-6252` → `const token = extractTokenCount(obj); … consumeUsageDelta(state, info.last_token_usage, info.total_token_usage); if (state.sawInterleaved || state.sawDivergentCumulative) { affected = true; }`
- `src/commands/sync.js:248` → `const CLAUDE_GROUND_TRUTH_REPAIR_KEY = "claudeGroundTruthRepair_2026_05_v4";`
- `src/commands/sync.js:6844-6854` → `async function repairClaudeQueueFromGroundTruth({ … }) {` + 迁移键短路
- `src/commands/sync.js:6871-6921` → 原子重写 `queue.jsonl`（`truthLines` + `fs.writeFile(tmp)` + `fs.rename(tmp, queuePath)`）
- `src/commands/sync.js:6930-6944` → 清空 `hourly.buckets` / `hourly.groupQueued` 中 `claude|` / `claude-mem|` 前缀
- `src/commands/sync.js:1149-1191` → 根被剥夺即放弃修复（`rootPreviouslySuppliedFiles` 判定）
- `src/commands/sync.js:4499-4512` → `bucketMap.set(\`${source}|${model}|${hourStart}\`, bucket)`（上传批次，last-row-wins）+ `linesRead >= maxBuckets` 截断
- `src/commands/sync.js:4447-4449` → `for await (const record of iterateQueueLines(handle, startOffset, st.size)) { if (!record.complete) break; }`（未闭合尾行留待下次）
- `src/commands/sync.js:6538-6542` → `if (row?.source !== source || …) continue; … setMaxTotal(modelTotals, \`${row.source}|${model}|${row.hour_start}\`, Number(row.total_tokens || 0)); return collapseModelTotalsByHour(modelTotals);`
- `src/commands/sync.js:6564-6589` → `collapseModelTotalsByHour`（跨模型求和）+ `function setMaxTotal(map, key, total) { … if (!Number.isFinite(prev) || total > prev) map.set(key, total); }`
- `src/commands/sync.js:5769-5775` / `:5824-5829` / `:5848-5872` → 三条 `setMaxTotal` 调用点全部在 Codex 血缘修复的「重建 vs 现状」对账路径内
- `src/lib/codex-model-attribution.js:3-23` → 逐字（含 `turn_context is the only signal that marks where a turn actually begins.`、`0 of 10310 session_meta rows across 5849 local rollouts … (codex-cli 0.151.0)`、`provenance rather than a model`）
- `src/lib/codex-model-attribution.js:82-102` → `turn_context` 推进 + `session_meta` 仅在 `!state.selectedModel` 时采纳、永不覆盖
- `src/lib/codex-model-attribution.js:33-45` → `eventEnvelope` 及 `model/rerouted` 8 种信封候选
- `src/lib/rollout.js:2476-2477` → `applyCodexModelEvent(modelAttributionState, obj); model = currentCodexModel(modelAttributionState) || model;`
- `src/lib/rollout.js:2626` → `const dedupKey = \`${sessionId || filePath}:${tokenTimestamp}\`;`
- `src/lib/rollout.js:4192-4195` → `function rolloutDateFromPath(filePath) { … /^rollout-(\d{4}-\d{2}-\d{2})T/ }`
- `src/lib/rollout.js:2256-2262` → `function codexSessionIdFromPath(filePath) { … }`（UUID 正则逐字一致）
- `src/lib/rollout.js:2396-2399` / `:2447-2463` → `for await (const record of physicalJsonlRecords(stream, { invalidUtf8 })) { scannedEndOffset += record.physicalBytes; if (record.terminated) committedEndOffset = scannedEndOffset;` / 兜底分支 + `break`
- `src/lib/rollout.js:2427-2446` → 事件字符串预筛（token_count / model/rerouted / turn_context|session_meta / thread_settings_applied / token_usage_record+task_complete|turn_aborted）
- `src/lib/rollout.js:2550-2553` → `const totalOnlyResetSentinel = source === DEFAULT_SOURCE && isCodexTotalOnlyResetSentinel(...)`
- `src/lib/rollout.js:2588-2595` → `FORK_REPLAY_GAP_MS` 首次超间隔即永久关闭
- `src/lib/rollout.js:2651-2663` → `return { endOffset: committedEndOffset, lastTotal: latestTotal, … }`
- `src/lib/rollout.js:4067-4079` → `PRIORITY_SUBSET_FIELDS`（逐字一致，含 `NEVER part of any sum` 注释）
- `src/lib/rollout.js:1949-1955` → `async function openclawOffsetEndsAtLineBoundary(fileHandle, offset) { … buffer[0] === 0x0a || buffer[0] === 0x0d }`；调用点 `:1787`
- `src/lib/rollout.js:1840-1845` → `if (!sameOpenclawFileGeneration(openedStat, closedStat) || !sameOpenclawFileGeneration(closedStat, pathStat)) { continue; }` + `src/lib/rollout.js:1784-1787` → `const offsetBoundaryMatches = hasUsageEventCursor && previousOffset <= fileEndOffset && await openclawOffsetEndsAtLineBoundary(fileHandle, previousOffset);`（OpenClaw 用「代际一致 + 行边界匹配」双保护决定可否复用旧游标）
- `src/lib/codex-usage-record.js:10-21` / `:49-52` / `:60` / `:82-86` / `:146-152` → compaction 规则、`payload.response_id` 去重键、上限 20000、`task_complete|turn_aborted`、`sameUsage` 计数条件（逐字一致）
- `src/lib/codex-service-tier.js:3-15` / `:20` / `:26` / `:28-42` → 档位归属规则、`CODEX_SERVICE_TIER_MARKER`、`["priority","fast"]`、`unknown` 从不猜
- `src/lib/codex-token-usage.js:3-8` / `:10-18`（`MAX_USAGE_BASELINES = 32`）/ `:25-27`（`value.cache_creation_input_tokens ?? value.cache_write_input_tokens`）
- `src/lib/codex-rollout-parser.js:1-4` / `:13-42`（四模块共享 import）/ `:51`（**第二份** `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000`）/ `:733`（唯一内部使用者）/ `:999-1008` / `:1023`（`provider = p.model_provider || provider;`）
- `src/lib/session-analytics.js:822-834` → 「`model_provider` … is provenance rather than a model and must not become a model-table row」防护
- `src/lib/cursor-store.js:13-21` → 常量块逐字一致；`:77-95` 激活判定与 `LegacyCursorStore` 分支；`:1711` `partitionCursorFiles` / `:1788` `codexFileShardKey` / `:1795` `codexDayDirectory` / `:1800` `codexEventShardKey` / `:252` `this.codexEventStore`；文件总行数 **1918**（分片 §存疑 13 一致）
- `test/claude-fork-conversations.test.js:35-95` → 7 个用例全部核对（`:37-39` 顺序无关、`:44-45` 与真值对账、`:48-59` 新增轮次 1→2、`:61-78` 污染态修复为 2、`:80-86` 仅剩 fork 副本、`:88-95` 父文件后到回溯修正）；文件 95 行
- `test/claude-reasoning-usage.test.js:14-25` / `:41` / `:42` / `:43` / `:44` / `:45-47` / `:48-49` → 10 组输入、`9999→518` 封顶、`output_tokens === 518 - reasoning`、`total_tokens === 1058`、五列和不变式、`< 1e-12` 计费等值、幂等；文件 51 行
- `test/rollout-parser.test.js:104-158` → `eventsAggregated === 2` / `bucketsQueued === 1` / `model === "unknown"` / 队列合计 = `usage1 + usage2` 的 `total_tokens`；注释 `:105-107` 逐字一致
- `CLAUDE.md:54/60/61/64` → `### Token normalization`、`reasoning_output_tokens = reasoning tokens (Codex/acode/every-code/OmO/Cline fold them into output_tokens for cost)`、`… keep reasoning as an informational subset because it is already included in output_tokens`、`**Cost is computed from input_tokens + output_tokens + cached_input_tokens + cache_creation_input_tokens + reasoning_output_tokens only — never total_tokens**`

## 待父 agent 决策的两点

1. **B8 的族数必须重写**（E1）：分片 §0 第 2 条与 §A.1.3 的「三族」结论会被任何后续读者当成事实引用，而它漏掉了 `parseCursorApiIncremental` / `parseTraeCnApiIncremental` / `parseTraeIncremental` / `parseCopilotIncremental` / `parseDshIncremental` / `parseCopilotAppDbIncremental` / `parseGrokBuildIncremental` 这 7 个不合族的 parser。建议在 S2 里改成「**无统一契约，12 种字面量**」并附我给出的完整清单。
2. **B1 的最终定性**：两侧**不冲突**，且**没有多算/少算**。但「谁的解释对」这一问在 CHECKLIST 里被预设为二选一，实际答案是**两边都对、只是 S2 的措辞把「列独立」写成了「语义独立」**。若父 agent 需要一句可直接引用的结论：「Codex 的 `reasoning_output_tokens` 是 `output_tokens` 的**子集标记**（解析侧不拆分、计价侧置 0），Claude 的 reasoning 是**独立加成项**（解析侧拆分、计价侧照价）；两条主线的 `computeRowCost` 输入都是五列之和，`total_tokens` 永不参与计费。」
