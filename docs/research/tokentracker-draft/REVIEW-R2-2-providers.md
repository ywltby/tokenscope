# 复核报告 R2-2：S3a / S3b（第二轮）

> 复核对象：`S3a-providers-jsonl.md`（1586 行）、`S3b-providers-db-api.md`（2998 行）
> 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`（只读），HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`
> 行号口径：`read` 工具 / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`（**未使用**裸 `Get-Content`）
> 职责：**证伪**。不修改样本仓库与任何分片。
> 复核脚本口径说明：本轮用 PowerShell 读取时数组变量名与循环变量名**避免同名**（`$arr`/`$k`），否则会复现「`$i`/`$I` 同变量」的假空白——这一点本身也是本报告 E1 系列结论为何只能靠逐行打开确认的原因。

---

## 结论

1. **R1 修正记录中列出的行号修正，逐条核实后全部成立**（S3a 8 处 + S3b 15 条口径/行号）。R1 修正的**方向与数值均正确**，`1284`/`7298`/`10337`/`10345`/`9940`/`12178`/`13691`/`12742` 八个新值逐字命中。
2. **但 GBK 行号漂移并未被清干净**：我在**修正记录之外**又查出 **6 处残留漂移行号**（S3a 4 处：`10672/10688/13704/13720`；S3a §7.1 第 7 条的 `15333/15338`），偏移量与 S3a 自述的漂移表**精确吻合**（≈10700 行处 +6、≈13700 行处 +10、≈15300 行处 +10）。这说明 §7.1 第 10 条的自查清单（"35 正确 / 5 错"）**过滤掉了真正的错项类型**：它只查了清单内 40 条，而漂移还散落在正文。
3. **两处仍在分片正文的兜底常量引用错误**（`9697`→`9704`、`9918`→`9924`），同属 +6/+7 漂移。
4. **S3a 修正记录 E12 的替换值仍然错**：`dedupeDroidSettingsFilesBySession` 的定义在 `rollout.js:15069`，`sync.js:6595` 只是一条注释；「全集唯一出现点」的措辞与全仓库 4 处命中不符。
5. **S3b 发现 1 处语义级错误（非行号）**：§8 第 7 点称 MiniMax Code「只能靠 entry 级 ISO 串」，与源码相反——包装器把外层 `timestamp` 置 null 后，`record.timestamp` 恒为 `null`，ISO 回落分支**永不触发**，唯一有效路径是 `message.timestamp` 的数值毫秒。
6. **R1 遗留的 U1（Workbuddy traces 字段）本轮可以结案**：traces 支路字段是 `modelInfo.total{Input,Output,Cached}Tokens`，与 JSONL 支路的 `providerData.rawUsage.*` **不同形**；`11246` 属第三条 SQLite 支路，不是 traces。
7. **跨分片横向核对基本自洽**：S2 的 45 条解析器签名表与我的独立枚举 **45/45 逐行一致**；`conversation_count`、`usage_precision`、`billable_total_tokens` 的跨分片引用均可对上。发现的横向问题集中在 S1/S4 侧（S1 的 45 项分解算术=46；S4 对 `9287` 的表述）。
8. **计数类断言我全部重算**：S3b「21 家 = 23 − 2」✅；「8 显式写 + 13 兜底 = 21」✅；`sql.js` 5 个测试 ✅；`usage_precision` **16 行** ✅ 但「14 个语句」不可复现（我算 15）❌；S3a §5 分类计数 1+2+3+10+3=19 ✅。

**总体判定**：S3a/S3b 的结构、字段映射、三要素（路径/字段/游标）绝大部分经得起逐字对照；**必须修**的是 E1–E10（尤其是 E1–E8 的同族残留漂移与 E9 的语义反转）。I 类为口径与 ±1，可随修。

---

## A. 修正点复查（逐条）

### A-1 S3a 修正记录（8 处行号修正 + 判据变更）

| # | 修正项 | 新值是否命中源码 | 旧值是否确为所述的「别的行」 | 判定 |
|---|---|---|---|---|
| A1 | E10 `1307`→`1284`（inode 身份） | ✅ `1284: const inode = st.ino || 0;`，判等 `1287-1291` | ✅ `1307: let lastModel = …` | 通过 |
| A2 | E3 `7294`→`7298`（`resolveKiroDbPath` 头） | ✅ `7298: function resolveKiroDbPath(basePath) {`，体 `7298-7300` | ✅ `7294: }` | 通过 |
| A3 | E4 `10331`→`10337`（非缓存输入修正块头） | ✅ `10337: const isFromTotalField =`，块 `10337-10343` | ✅ `10331: let inputTokens = toNonNegativeInt(inputRaw);` | 通过 |
| A4 | E5 `10339`→`10345`（扩展日志零值闸门） | ✅ `10345: if (inputTokens === 0 && completionTokens === 0 && cacheRead === 0 && cacheCreation === 0) {` | ✅ `10339: usage.cacheMissTokens === undefined &&` | 通过 |
| A5 | E6 `9938`→`9940`（`providerData.rawUsage` 注释） | ✅ 注释 `9940-9943`，`9944: function codebuddyJsonlHasUsage(filePath) {` | ✅ `9938: }` | 通过 |
| A6 | E7 `12190`→`12178`（`normalizeClineModel`） | ✅ `12178: function normalizeClineModel({ modelInfo, fallbackModel }) {`，兜底链 `12179-12189` | ✅ `12190: }`（闭合） | 通过 |
| A7 | E9 `13681`→`13691`（`parseUnslothIncremental` 头） | ✅ `13691: async function parseUnslothIncremental({` | ✅ `13681: if (!id || !totals || !bucketStart) return null;` | 通过 |
| A8 | E8 `12732`→`12742`（`isReset`） | ✅ `12742: const isReset = currSum > 0 && currSum < prevSum;` | ✅ `12732: const prev = threadTotals[row.id] || {…}` | 通过 |
| A9 | E1 KiroCli 推翻为「真实 token 优先」 | ✅ `8371: let inputTokens = toNonNegativeInt(turn.input_token_count);`、`8372: …output_token_count`，回退条件 `8374`，字符估算 `8386`/`8387` | — | 通过 |
| A10 | E1 补漏「常量使用点第 4 处 `8428`」 | ⚠️ `8428` 是**注释行**（`// KIRO_CLI_CHARS_PER_TOKEN; bypass that by pre-multiplying here.`），实际使用在 `8429`/`8430`；全文件引用共 `8118`(定义)/`8386`/`8387`/`8428`/`8429`/`8430`/`9155`/`9156`/`9158`，「第 4 处」口径不可复现 | — | **I1** |
| A11 | E2 §5 把 KiroCli 从 E 类移入 B 类 | ✅ 会话支路闸门 `9414` 只检 4 列（input/output/cacheRead/cacheCreation，**不含** reasoning）；`9446: total_tokens: input + output + cacheRead + cacheCreation + reasoning` | — | 通过 |
| A12 | E2 前提「`kiro` 不在推理并入集合」 | ✅ `pricing/index.js:248-253` 集合仅 `codex`/`acode`/`every-code`/`omo`/`cline` | — | 通过 |
| A13 | B4 前提（Gemini 唯一性的隐含条件） | ✅ `pricing/index.js:254-256: const reasoningCost = reasoningIncludedInOutput ? 0 : (row.reasoning_output_tokens || 0) * (pricing.output || 0);`，与 §5 引文逐字一致 | — | 通过 |
| A14 | I1 Codebuddy 结案归 D 类 | ✅ 扩展日志闸门 `10345`（四桶）；JSONL 支路 `10476-10485` 五桶含 `reasoningTokens` | — | 通过 |
| A15 | I2 §5 E 类限定（`totalDelta` 只由 4 桶求和） | ✅ `12751: const totalDelta = delta.input + delta.output + delta.cache_read + delta.cache_write;`、`12752: if (totalDelta <= 0) {`；Cline `12403: total_tokens: deltaTotal` | — | 通过 |
| A16 | I4 `12994` 改称函数头 | ✅ `12994: function normalizeLocalStudioTokens(usage) {`；返回块 `13039-13049`，其中 `13047: total_cost_usd: 0,`、`13048: conversation_count: 1,` | — | 通过 |
| A17 | I5 KiroCli v2 支路无 token 字段 | ✅ `8475-8481` 注释逐字（"do not carry token counts … same 4 chars/token approximation"），`8482: async function readKiroCliV2SessionTurns(messagesPath) {` | — | 通过 |
| A18 | I3 Opencode SQLite 支路补记 | ✅ `5038` 段注释起，与 `sync.js:1386-1388` 三路相加（`filesResult.* + dbResult.*`）一致 | — | 通过 |
| A19 | 第 5 条 KimiCode home 结案 | ✅（KimiCode 侧）`9669: function resolveKimiCodeHome(env = process.env) {`、`9671: …env?.KIMI_CODE_HOME…` | — | 部分通过（Kimi 侧见 U2） |
| A20 | 附带收紧：`7338`/`10022`/`2032`/`13634-13636` | ✅ `7338: function countKiroJsonlLines(jsonlPath) {`；`10022: function capCodebuddyUsageFingerprints(state, maxEntries = 10_000) {`；`sync.js:2032: prune: true,`（2031 为注释）；`13634-13636: 0 AS cached_tokens, / 0 AS cache_write_tokens, / 0 AS reasoning_tokens` | — | 通过 |
| A21 | Cline `sync.js:2110` 注释 | ✅ `2110: // ── Cline (CLI v3 / desktop app — ~/.cline/data/sessions) ──` | — | 通过 |
| A22 | **E12 `sync.js:6595`（全集唯一出现点）** | ❌ 函数定义在 **`rollout.js:15069: function dedupeDroidSettingsFilesBySession(files) {`**（调用 `rollout.js:15165`、导出 `24839`）；`sync.js:6595` 只是一条注释（`// session showed as 40.06B). The forward fix is dedupeDroidSettingsFilesBySession`）。全仓库命中 4 处，「唯一出现点」不成立；`sync.js:2013` 确为 `const droidSettingsFiles = sourceAllowed("droid")` ✅ | — | **E6** |
| A23 | **§7.1 第 7 条 `15333`/`15338`（Droid 调用点与注释）** | ❌ 实为 **`15343-15346`（注释：`// Model resolution mirrors ccusage's chain: settings.model → sidecar <id>.jsonl scrape → \`<provider>-unknown\`…`）与 `15348: const model = resolveDroidModel(settings, filePath);`**；`15333` 是对象字面量闭合 `};`，`15338` 是 `const bucketStart = toUtcHalfHourStart(`。两处恰好为 **+10 漂移** | — | **E5** |
| A24 | §7.1 第 10 条「35 正确 / 5 错误 + 清单外 2 错误 + 1 归属错误」 | ✅ 其中「已改正的 8 处」我独立复核全部成立（见 A1-A8）；`12845`/`12853` 确属 LM Studio 模块 ✅（`12843: // LM Studio`、`12845: // Data: pretty-printed OpenAI-compatible final responses beneath`、`12853: const LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024;`） | 但清单外仍有残留，见 E1–E4、E7、E8 | 部分通过 |
| A25 | U1 Workbuddy traces 字段名 | ✅ **本轮结案**：traces 支路 `11082-11132` 用 `modelInfo.totalInputTokens`/`totalOutputTokens`/`totalCachedTokens` + `startedAtRaw` + `models[0]`，**与 JSONL 支路不同形**；零值闸门 `11093`；落桶 `11115` 用 `variant`。`11246` 位于 `11134` 之后的 **SQLite 支路**（`11240-11248`），非 traces | — | 通过（U 关闭） |

### A-2 S3b 修正记录（15 条）

| # | 修正项 | 核实结果 | 判定 |
|---|---|---|---|
| A26 | E1 全文「23 家」→「21 家」，口径 = 23 个 `parse*` − 2 共享实现 | ✅ `15753: async function parseOmpLikeIncremental({`、`16319: async function parsePiLikeIncremental({`；21 项 × 1 函数 + 2 共享 = 23 ✅ 算术闭合 | 通过 |
| A27 | E1 §20.1/§20.2 表 19 + 2 行 | ✅ §20.1 行 1–19、§20.2 行 20–21；表内条目与源码路径/行号抽查一致（如 omp `rollout.js:11433`、pi `16171-16223`、prime `16258-16275`、minimax `16745-16764` 均命中） | 通过 |
| A28 | E2 `sql.js` 断言限定为「生产 `src/**` 零命中」+ 补登 pglite | ✅ `package.json:61: "@electric-sql/pglite": "^0.5.8",`、`67: "sql.js": "1.14.2",`，两者均在 `"devDependencies"`（`60` 起）内；**`sql.js` 恰为 5 个测试 `require`**（`sync-trae:9`、`trae-db:7`、`trae-snapshot:6`、`trae-sqlite:3`、`trim-embedded-deps:8`）✅；`pglite` 另有 10 个测试使用（分片未给数，非错误） | 通过 |
| A29 | E3+E6 改为「两处全零」：qoder 新 JSONL + trae-cn | ✅ qoder：`6639` 起三元表达式，全零分支 `6650-6660`（`total_tokens: 0`、`billable_total_tokens: 0`、`conversation_count: 1`）；trae-cn：`22483-22487` 只校验 `Number.isSafeInteger(value) && value >= 0`（0 合法）、`22492: const total = rawInput + outputTokens;`、`22500-22508` 写 `total_tokens: total`/`billable_total_tokens: total`/`conversation_count: 1`、`22749-22751` 无零值护栏落桶 | 通过 |
| A30 | E3+E6 craft 改为反例 | ✅ `17199: if (dInput === 0 && dOutput === 0 && dCacheRead === 0 && dCacheWrite === 0) {` → `17211: continue;`；`17243: conversation_count: prev.total === 0 ? 1 : 0,` 与 I6 引文一致 | 通过 |
| A31 | E4 落桶点 `22749-22751`，`22680` 是 fail-closed | ✅ `22680-22682: throw new Error(\`Trae CN snapshot is not authoritative: …\`)`；`22738-22746` 回撤路径（`assertTraeCnBucketCovers` → `subtractTotals` → `touchedBuckets.add`）；落桶 `22749-22751` | 通过 |
| A32 | E5 pi 路径 `17171-17223` → `16171-16223` | ✅ `16171: function resolvePiSessionFiles(env = process.env) {`、`16223: }`；`17080: async function parseCraftIncremental({` 确为 craft 起点 | 通过 |
| A33 | E7 「不写 `billable_total_tokens` 的 13 项」+ 逐项自查 | ✅ 我重算：显式写 8 项（`6177` qoder 旧库、`6540`/`6646`/`6657`/`7025` qoder 新、`devin-usage.js:197`、`cursor-config.js:536`、`grok-usage.js:83`+`rollout.js:19959`、`21806`/`21831` antigravity、`22507` trae-cn、`24212` command-code）＋兜底 13 项 = **21** ✅；`trae` 区段 `21993-22176` 对 `billable_total_tokens` **零命中** ✅；`17485`/`23212`/`23239` 确为字段清单里的字符串 ✅（`23212`/`23239` 落在 dsh 区段内，但形态是 `"billable_total_tokens",`，非写入） | 通过 |
| A34 | E8 `usage_precision` 「16 行 / 14 个语句」 | ✅ **16 行**精确：`3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286/20287/22011/22025/22113/22157`；⚠️ **语句数不可复现** | **I3** |
| A35 | E9 SQLite 数据源 7 项 + `cursor` 仅取凭据 | ✅ §20.4 表：SQLite 7 项；JSONL 13 项；远程 2 项；项数之和 22 > 21 已有口径注 ✅；`cursor-config.js:46-47: const CURSOR_ACCESS_TOKEN_SQL = "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken';"` | 通过 |
| A36 | I1 登记 `6726` 悬空注释 | ✅ `6726: // addTotals handles total_cost_usd via USD_TICKS, but credits cost is small; ensure it accumulates`，下一行 `6727: touchedBuckets.add(bucketKey(sourceKey, cur.model, cur.bucketStart));`，上下文确无实现 | 通过 |
| A37 | I3 Trae wa-sqlite WASM 例外 | ✅ `trae-sqlite.js:7-11`：动态 `import("@journeyapps/wa-sqlite/dist/wa-sqlite.mjs")` / `"@journeyapps/wa-sqlite"` / `"…/src/FacadeVFS.js"` | 通过 |
| A38 | I4 `readTraeEntitlementFromStorage = 22269` | ✅ `22269: function readTraeEntitlementFromStorage(storagePath) {`；`22236: function normalizeTraeEntitlement(serverData) {`、`22260: }`；唯一调用点 `status.js:840: ? readTraeEntitlementFromStorage(traeStoragePath)` | 通过 |
| A39 | I5 三处区间纠正 | ✅ `17008: function diffReasonixTotals(current, previous = {}) {` … `17014: }`；`17067-17068: if ((tokenDelta > 0 || delta.requests > 0) && / addReasonixDelta(…)`；`11612: function resolveOmoDefaultModel() {` … `11615: }` | 通过 |
| A40 | I6 §22.3 表补 trae/trae-cn 零值形态 | ✅ `22112: if (!previous && totals.total_tokens === 0) continue;`（只保护首次观测）与 §22.3 表 2794 行表述一致 | 通过 |

---

## B. 独立新抽样（≥35 条；避开 R1 已抽查家）

三要素 = 路径拼接 + 字段映射 + 游标判据。

### B-1 S3a 侧

| # | 家 | 三要素核实 | 判定 |
|---|---|---|---|
| B1 | Workbuddy | **路径** ✅ `10651: const WORKBUDDY_VARIANTS = {`（`workbuddy`/`workbuddy-ai` + `WORKBUDDY_HOME`/`WORKBUDDY_AI_HOME`）；`10662: function resolveWorkbuddyHome(env = process.env, variant = "workbuddy") {` | 通过 |
| B2 | Workbuddy | **字段** ✅ `10907: const provider = entry.providerData;`；`10963: const inputTokens = Math.max(0, promptTokens - cacheRead - cacheCreation);`；`10965: const reasoningTokens = Math.min(completionTokens, toNonNegativeInt(completionDetails.reasoning_tokens));`；`10966: const outputTokens = Math.max(0, completionTokens - reasoningTokens);` | 通过 |
| B3 | Workbuddy | **游标** ✅ 五桶闸门 `10968-10977`；`11004: …getHourlyBucket(hourlyState, variant, model, bucketStart);`；`11007: seenIds.add(messageId);`；`11008` 旧 entry-id 双写注释；`11013: detailedSessions[sessionId] = true;` | 通过 |
| B4 | Workbuddy | ❌ **兜底模型行号错**：分片称 `"auto"` 兜底在 `10672`，实为 **`10678: const fallback = "auto";`**（`10677: function resolveWorkbuddyDefaultModel(env = process.env, variant = "workbuddy") {`） | **E1** |
| B5 | Workbuddy | ❌ **三类数据源注释行号错**：分片称 `10688`，实为 **`10694: // Recursively collect every *.jsonl under ~/.workbuddy/projects so that`** | **E2** |
| B6 | Roocode | **路径** ✅ `11767: function resolveRoocodeTaskFiles(env = process.env) {`（复用 `resolveKilocodeRoots`，拼 `User/globalStorage/rooveterinaryinc.roo-cline/tasks`） | 通过 |
| B7 | Roocode | **字段** ✅ `11906-11909`：`tokensIn`/`tokensOut`/`cacheReads`/`cacheWrites`；入口 `11892: if (msg.say !== "api_req_started" && msg.say !== "api_req_deleted") continue;` | 通过 |
| B8 | Roocode | **游标** ✅ `11899: const ts = Number(msg.ts);`、`11902: const dedupKey = \`${taskUuid}:${ts}\`;`；模型三级兜底 `11814: function normalizeRoocodeModel({ explicitModel, apiProtocol }) {`（正则 `11805`） | 通过 |
| B9 | Kilocode | **路径** ✅ `11706: function resolveKilocodeTaskFiles(env = process.env) {`（`kilocode.kilo-code/tasks`）；sync 侧双安装合并 `sync.js:1949-1951`（`mergeBothFileSources`） | 通过 |
| B10 | Kilocode | **字段** ✅ `15624-15627` 四镜像；`15640-15648` delta（`reasoning_output_tokens: 0`，`total_tokens = tokensIn + tokensOut + cacheReads + cacheWrites`） | 通过 |
| B11 | Kilocode | **游标+source** ✅ `15617: const ts = Number(msg.ts);`；零值闸门 `15628`；`15651: …getHourlyBucket(hourlyState, "kilo-code", model, bucketStart);`；模型 `11730: function normalizeKilocodeProviderToModel(providerName) {`（注释 `11724`） | 通过 |
| B12 | Lmstudio | **路径** ✅ `12859: function resolveLmstudioHome(env = process.env) {`；`12871: async function resolveLmstudioLogFiles(env = process.env) {`；`12852: const LMSTUDIO_SOURCE = "lmstudio";` | 通过 |
| B13 | Lmstudio | **字段** ✅ `12994: function normalizeLocalStudioTokens(usage) {`、`13040: input_tokens: Math.max(0, total - completion - cacheRead - cacheWrite),`、`13047/13048` 常量列；字节扫标记 `12897`、花括号配平 `12917`、窗口 `12853/12854` | 通过 |
| B14 | Lmstudio | **游标** ✅ 文件 stat 三态 `13325: const requiresRebuild = filePlans.some(`；事件键 `13099: const fallback = crypto.createHash("sha256")`；mtime 回落 `13129: const fallbackTimestamp = initialStat.mtimeMs;`；行时间 `12979` | 通过 |
| B15 | Kiro（IDE 版） | **路径** ✅ `7282: const suffix = ["Kiro", "User", "globalStorage", "kiro.kiroagent"];`；`7298-7304` `resolveKiroDbPath`/`resolveKiroJsonlPath` | 通过 |
| B16 | Kiro | **字段** ✅ `7545: const inputTokens = toNonNegativeInt(row.tokens_prompt);`、`7546: …row.tokens_generated`、`7547: if (inputTokens === 0 && outputTokens === 0) continue;`；`7567` source `"kiro"`；模型 `7555: const resolvedModel = resolveKiroModel(modelTimeline, ts);` | 通过 |
| B17 | Kiro | **游标** ✅ `7541: let maxId = lastDbId;`、`7572: if (usingDb && row.id && row.id > maxId) maxId = row.id;`；JSONL 支路行号 `7338` + `7479: const lastJsonlLine = …jsonlState.lastLine…`、`7496-7508`（DB 消失时推进 JSONL 水位） | 通过 |
| B18 | Cline（metrics 语义） | ✅ **逐字一致**：`12356-12359`（`inputTokens = inclusiveInput - cacheRead - cacheWrite`）、`12360-12361` 注释（reasoning 是 `outputTokens` 子集、不进 total）、`12362/12363`（output/reasoning）、`12365: const totalTokens = inputTokens + cacheRead + cacheWrite + outputTokens;`、`12388` 成本差分、`12403: total_tokens: deltaTotal`、`12404: total_cost_usd: deltaCost`、`12405: conversation_count: previous ? 0 : 1` | 通过 |
| B19 | Cline | **模型+游标** ✅ `12178` 四级兜底；`12185-12188` `provider:` 前缀；三键兼容 `12369-12373`；导入闸门 `12350-12352`（`importedAtMs`） | 通过 |
| B20 | Goose（累计差分） | ✅ `14686: if (totalNow === 0 && inputNow === 0 && outputNow === 0) continue;`；`14693: const isReset = totalNow > 0 && totalNow < prev.total;`；`14694-14696` 三分量差分；`14710: const accountedDelta = dInput + dOutput;`、`14711: const reasoningDelta = Math.max(0, dTotal - accountedDelta);`；落桶 `14720-14728`（`total_tokens: dInput + dOutput + reasoningDelta`）；`14730` source `"goose"` | 通过 |
| B21 | Goose | **语义印证** ✅ B 类成立：输入/output 无增长而 total 增长时（如 `input_tokens=0/output_tokens=0/accumulated_total_tokens=1000` 的会话首见）可产出 `input=0/output=0/reasoning>0/total>0`，且 `goose` 不在 `pricing/index.js:248-253` 集合内 ⇒ 按 output 价计费，非 $0 | 通过 |
| B22 | ★ 新增 | ❌ **KimiCode 兜底常量行号错**：分片 §3.8 称 `"kimi-for-coding"` 在 `9697`；实为 **`9703: function resolveKimiCodeDefaultModel(env = process.env) {` / `9704: const fallback = "kimi-for-coding";`**；`9697` 是 `resolveKimiCodeWireFiles` 内 `for` 循环的闭合 `}` | **E7** |
| B23 | ★ 新增 | ❌ **Codebuddy 兜底常量行号错**：分片 §3.9 称 `"codebuddy-unknown"` 在 `9918`；实为 **`9924: const fallback = "codebuddy-unknown";`**；`9918` 是 `resolveCodebuddyHome` 内 `});` | **E8** |
| B24 | ★ 新增 | ⚠️ §3.17 称「`14687` 的 continue 条件是三者皆 0」：该闸门实为 **`14686`**（`totalNow/inputNow/outputNow` 三者皆 0）；`14687` 为空白行 | **I2** |
| B25 | ★ 新增 | ❌ **Unsloth 快路径行号错**：分片 §3.15 称「有界时才走快路径（`13704`）」；实为 **`13714: const hasBoundedScan = priorState.scan`** + `13717: if (hasBoundedScan && sameSqliteFingerprint(…)`；`13704` 是 `: {};` | **E3** |
| B26 | ★ 新增 | ❌ **Unsloth 落桶函数行号错**：分片 §3.15 称「落桶经 `reconcilePassiveUsageEvent`（`13720`）」；实为 **`13730: if (event && reconcilePassiveUsageEvent({`**；`13720` 为空白行 | **E4** |

### B-2 S3b 侧

| # | 家 | 三要素核实 | 判定 |
|---|---|---|---|
| B27 | Omp/Omo（共享实现） | **source 注入** ✅ `16087-16098`（omp：`stateKey/source "omp"`、`reasoningFields: ["reasoningTokens"]`）；`16100-16111`（omo：`source "omo"`、`reasoningFields: ["reasoningTokens","reasoning"]`、`reasoningIncludedInOutput: true`） | 通过 |
| B28 | Omp | **路径** ✅ `11352: function resolveOmpHome(env = process.env) {`、`11355: if (env.OMP_HOME) …`、`11356: if (env.PI_CONFIG_DIR) …`、`11395: if (env.TOKENTRACKER_OMP_AGENT_DIR) {`、`11433: function resolveOmpSessionFiles(env = process.env) {` | 通过 |
| B29 | Omp | **字段** ✅ `15868-15872`（`input/output/cacheRead/cacheWrite` + `pickUsageInt(usage, reasoningFields)`）；`15874-15883` 五桶零闸门；`15897-15903` total 逻辑（`reasoningIncludedInOutput ? 0 : reasoningTokens`）；`15905: const model = normalizeModelInput(msg.model) || fallbackModel;`；`15907-15915` delta | 通过 |
| B30 | Omp | **游标** ✅ `15941-15946` 项目归属独立游标；`16041-16050` 上限 10000（`seen` 与 `projectSeen` 各一）；`15701-15710` `MIN_OMP_TIMESTAMP_MS` 校验；`15712-15715` `OMP_LIKE_MAX_RECORD_BYTES = 64MB` | 通过 |
| B31 | Omp/Omo 防撞车 | ✅ `16157-16169` `piAgentDirCollidesWithOmp` / `omoAgentDirCollidesWithOmp`（`path.resolve` 相等判定） | 通过 |
| B32 | Omo | **路径** ✅ `11582: function resolveOmoHome(env = process.env) {`、`11596: function resolveOmoAgentDir(env = process.env) {`、`11604: function resolveOmoSessionFiles(env = process.env) {`；兜底模型 `11612-11615` | 通过 |
| B33 | Pi | **source 派生** ✅ `16286: function piSourceForProvider(provider) {`（`pi-<slug>` / `pi`）、`16297: function primeAgentSourceForProvider(provider) {`（`prime-agent-<slug>` / `prime-agent`）、设计意图注释 `16281-16285` | 通过 |
| B34 | Pi | **路径** ✅ `16142/16145` 覆盖变量、`16171: function resolvePiSessionFiles(env = process.env) {`；Prime `16236/16251/16258-16275`；Minimax `16728` + `16745-16764`；路径形态注释 `16711` | 通过 |
| B35 | Pi 家族 | **字段** ✅ `16449-16453`（四桶 + `reasoningTokens`）；`16482-16485` total 兜底**含** reasoning（与 §7 的 omo 分支相反）；`16487: const model = normalizeModelInput(msg.model) || fallbackModel;`；模型兜底 `16225`/`16277`/`16782`（`${MINIMAX_CODE_SOURCE}-unknown`） | 通过 |
| B36 | Pi 家族 | **游标** ✅ `16434: const record = readEntry(entry);`、`16438: if (!msg || msg.role !== "assistant") continue;`、`16445: if (seenIds.has(entryId)) continue;`；撕裂尾巴 `16405-16426`；`16680: projectSeenIds: Array.from(projectSeenIds).slice(-10_000),` | 通过 |
| B37 | Pi 家族（分片 W） | ❌ **§8 第 7 点时间戳表述与源码相反**：分片称 MiniMax「记录包装器把 `timestamp` 恒置 null（`16771`），因此**只能靠 entry 级 ISO 串**」。实际包装器 `16766-16773` 返回 `timestamp: null`，而回落分支是 `16469: } else if (typeof record.timestamp === "string" && record.timestamp) {` —— **`record.timestamp` 恒为 null，该分支永不触发**；唯一有效路径是 `16467: if (Number.isFinite(Number(msg.timestamp)) && Number(msg.timestamp) > 0)`，即 `message.timestamp` 的数值毫秒。测试 fixture 印证结构（`test/minimax-code-parser.test.js:37-64`：顶层只有 `message_id`/`turn_id`/`message`，`timestamp` 在 `message` 内） | **E9** |
| B38 | Copilot session-store（基线采纳） | ✅ `18717-18723`：`needsAdoption = globalAdoptionPending \|\| !dbState.adoptedAt \|\| resetDetected`；`baselineOnly = resetWithoutFingerprintHistory \|\| (needsAdoption && !resetDetected && !backfillOnFirstRun)`；`18724-18727` 依据 `baselineOnly` 决定从 0 或 `priorLastId` 读 | 通过 |
| B39 | Copilot 跨通道 | ✅ `18822-18824` OTEL 认领（`otelUsageMatcher.consume(recentEvent)`）；`18834-18856` `legacyResiduals` 累计（`priorResidual.storeTokens += normalized.total_tokens`）；`18942-18949` 补差（`normalizeCopilotAppTokenDelta(baseline, zeros)`，`18950: const residualTokens = Math.max(0, toNonNegativeInt(residual.storeTokens) - appTotals.total_tokens,`） | 通过 |
| B40 | Copilot App DB（基线采纳） | ✅ `19357-19379` `normalizeCopilotAppTokenDelta`（cache 先 clamp 再扣、`cache_creation_input_tokens: 0`、reasoning 从 output 拆、`conversation_count: copilotAppBaselineTotal(prev) === 0 && totalTokens > 0 ? 1 : 0`）；跳过 OTEL 已覆盖面 `19213-19224`；`19381-19399`（含 `observeOnly`）；库级指纹跳过 `19439` | 通过 |
| B41 | Copilot App DB | ⚠️ §13 第 8 点称「库级指纹未变即跳过（`19438-19439`）」：语句实为 **`19439`**（`if (dbState.lastDbFingerprint && sameSqliteFingerprint(…)`），`19438` 为空白行（±1） | **I5** |
| B42 | OpenCode SQLite | ✅ `5969-5971: const contribution = moved ? { …currentTotals, conversation_count: 1 } : { …delta, conversation_count: effectiveLastTotals ? 0 : 1 };` 与 §2 的「首次见该 messageKey 记 1，已有配额记 0」逐字一致 | 通过 |
| B43 | Reasonix | ✅ `17044: conversation_count: delta.requests,`；`17038-17043` 其余五列；`17067-17068` 差值阈值；`17008-17014` `diffReasonixTotals` | 通过 |
| B44 | Craft | ✅ `17199-17212` 四分量全零 continue（反例成立）；`17242: total_tokens: dTotal > 0 ? dTotal : dInput + dOutput + dCacheRead + dCacheWrite,`；`17243` 会话级计数 | 通过 |
| B45 | Antigravity | ✅ source 由 `source` 变量传入（`21861: …getHourlyBucket(hourlyState, source, model, bucketStart);`、回撤 `21210`）；billable 两条路径 `21806`/`21831: delta.billable_total_tokens = delta.total_tokens;` | 通过 |
| B46 | Trae | ✅ `22002: const TRAE_CURSOR_VERSION = 2;`；重建分支注释 `22049`、`22159`（`all or nothing`）；冲突裁决 `22111`；`22112` 首次观测零值护栏；落桶 `22127/22131`；`22174: cursors.trae = { version: TRAE_CURSOR_VERSION, stores, databases, turns };` | 通过 |
| B47 | Trae CN | ✅ `22322: const TRAE_CN_SOURCE = "trae-cn";`；`22477: const fields = [`；`22483-22492` 校验；`22500-22508` totals；`22680` fail-closed；`22749-22751` 落桶 | 通过 |
| B48 | DSH | ✅ `22820: const DSH_SOURCE = "dsh";`；`22827/22829` 覆盖变量；`22815` 水位注释；落桶 `23965` | 通过 |
| B49 | Command Code | ✅ `24067: const COMMAND_CODE_SOURCE = "command-code";`；`24086: const explicit = env?.TOKENTRACKER_COMMANDCODE_HOME;`；`24212: billable_total_tokens: total,`；`24337: billable_total_tokens: totals.billable_total_tokens ?? totals.total_tokens ?? total,` | 通过 |
| B50 | Devin | ✅ `DEVIN_SOURCE = "devin"`（`14104`）；解析器头 `14207`；`devin-usage.js:48-51` SQL 只选四个 metrics 字段；`190-200` totals（`reasoning_output_tokens: 0`、`billable_total_tokens: total`、`conversation_count: 0`） | 通过 |
| B51 | Cursor | ✅ `cursor-config.js:46-47` 凭据 SQL；`525-535` 为何不写 0 的注释；`536: billable_total_tokens: totalTokens,` | 通过 |
| B52 | Grok | ✅ `grok-usage.js:26-39` `reportedCostUsd`（ticks 优先，未完成/部分成本返回 null）；`83: billable_total_tokens: totalTokens,`；`rollout.js:19551: const GROK_CURSOR_VERSION = 5;`；`19645-19649` `capGrokSessionSnapshots`；`20249-20252`/`20265` 兜底桶起点；`20224/20380` 事件透传 billable | 通过 |
| B53 | ★ 指定家：DuMate / ZCode / Mimo | 三家在 **S3a、S3b 全文零命中**。归属核实：`parseDumateIncremental` 在 **独立模块 `src/lib/dumate.js:32`**（非 `rollout.js`，故不属 S3a 声明范围「全部在 `src/lib/rollout.js`」）；ZCode 走 `session-analytics.js` 的 `readZcodeDbMessages` / `install-resolver.js` 的 `resolveZcodeNativeDbPath`；Mimo 走 `rollout.js` 的 mimo/zcode 判别器（`5168`、`5380-5385`）并由 **S2**（`readMimoDbMessages`、`readZcodeDbMessages`）与 **S4**（`repairMimoClaudeMislabel`、`dropMimoQueueRows`、`repairZcode*`）承接。三家均在 `init.js:119-165` 的 `SUPPORTED_PROVIDERS` 名单内（`150: "Mimo"`、`151: "DuMate"`、`152: "ZCode"`）。**结论：无覆盖缺口**，S3a 的排除声明成立 | 通过（范围外） |
| B54 | ★ 自动审计 | 我对两份分片**全部 686 条（S3a 290 + S3b 396）** `rollout.js:NNNN` 引用做了「是否指向空白行 / 孤立 `}`」的机械筛查，命中可疑项 12 条；逐条打开后排除了合法范围端点（`17014`、`18430`、`19438`±1 等）与已登记项（`7305` −1 容差、`17171` 为 E5 原稿旧值），**其余 6 条即 E1/E2/E3/E4/E5(×2)/E7/E8 与 I2/I5**，全部在上表列出 | 通过 |

---

## C. 横向一致性

| # | 检查项 | 结果 |
|---|---|---|
| C1 | **S2 解析器签名表 vs 我的独立枚举** | ✅ S2 `73-117` 列出 45 条 `parse*Incremental` 及其行号；我按 `^(async )?function parse\w*Incremental` 独立枚举得 45 条，**逐条行号 45/45 完全一致**（含 `14207` devin、`15753` ompLike、`16319` piLike、`20800` antigravity、`22033` trae、`24458` command-code） | 通过 |
| C2 | S2 表 vs S3b「2 个共享实现」口径 | ✅ S2 同时列出 `parseOmpLikeIncremental`(15753)/`parsePiLikeIncremental`(16319) 与四个薄壳（`16087`/`16100`/`16689`/`16699`/`16775`），与 S3b E1 的「23 − 2 = 21」口径同源 | 通过 |
| C3 | hook 判定：Claude 系 | ✅ S1 §2.4（`245`）「Claude 系（claude / codebuddy / workbuddy / workbuddy-ai）走 `hooks.Stop` + `hooks.SessionEnd`」与 S3a §4.2 判定表（Gemini、CodeBuddy、WorkBuddy、WorkBuddy AI）一致；安装点 `init.js:581`/`585` 与 S3a 引文一致 | 通过 |
| C4 | hook 判定：OMP | ⚠️ **三份分片表述不齐**：S1 `231` 明确「oh-my-pi / OMP → `<ompAgentDir>/extensions/tokentracker-notify.ts`（`turn_end` + `agent_end` + `session_shutdown`）」，S1 `248` 亦列 OMP 走 extension；S3a §4.1 承认 `omp-hook.js` 存在但把 OMP 排除在 19 家外（合理）；**S3b §7/§20 全文未提 OMP 的 hook 状态**（S3b 全文仅 `2554` 一处 hook 提及，且是 grok）。不是矛盾，但「21 家 hook/被动」这一维度在 S3b 缺失 | **I8** |
| C5 | `conversation_count`：reasonix | ✅ S4 `821`「按请求数——例如 `conversation_count: delta.requests`（`17044`）」与 S3b `1150`/§21.2/`2621` 一致；源码 `17044` 逐字为 `conversation_count: delta.requests,`，`requests ← usage.requestCount`（S3b `1130`） | 通过 |
| C6 | `conversation_count`：copilot | ✅ S4 `823` 引 `18886` 与 S3b §12 第 9 点引 `18886` 同值（`seenSessions.has(sessionId) ? 0 : 1`） | 通过 |
| C7 | `conversation_count`：craft | ✅ S4 `816`（`prev.total === 0 ? 1 : 0`）与 S3b `17243` 一致 | 通过 |
| C8 | `usage_precision` 写入点 | ✅ S4 `1442` 说明「`usage_precision` 与输出模型拼接在调用点」与 S3b E8 的 16 行清单兼容；S3b 补入的 `6725/6731/6755/6760/22113` 我逐行确认存在 | 通过 |
| C9 | `conversation_count`：`9287` 的表述 | ⚠️ S4 `823` 称 `rollout.js:9287` 属「刻意置 0 以避免重复计数」；源码 `9284-9288` 是**负值 clamp**（`if (bucket.totals.conversation_count < 0) … = 0;`，注释 `9283: // negatives come from the subtract-old pass on mutation or retraction.`）。语义不同（收敛保护 vs 去重） | **I7**（S4 侧） |
| C10 | 家数拆解算术 | ❌ S1 `239`：「`SUPPORTED_PROVIDERS`（**45 项**，`init.js:119-165`）」行号与项数我对过：`120-164` 恰 **45 项** ✅；但同句拆解「写配置 12 + OpenClaw legacy 1 + 仅探测 1 + 纯被动读取 17 + 仅出现在欢迎名单 15」= **46 ≠ 45**，口径未闭合 | **E10**（S1 侧） |
| C11 | 「21 家」与 S3a「19 家」是否冲突 | ✅ 不冲突但未合并口径：S3b §20.4 已声明按「解析通道」计项、按「家」为 21；S3a §7.2 第 1 点自认 7 家实为 SQLite 读取（职责与 S3b 部分重叠）。两份分片均未给「S3a ∪ S3b 去重后唯一 provider 实体数」（`opencode` 在两侧各占一行） | **I9**（轻微） |
| C12 | `reasoningIncludedInOutput` 集合 | ✅ S2 `1422-1426` 与 S3a §5 B4 前提 `pricing/index.js:248-256` 为同一集合（codex/acode/every-code/omo/cline），引文一致 | 通过 |
| C13 | S3a §5 分类计数 | ✅ A=1、B=2、C=3、D=10、E=3 ⇒ 1+2+3+10+3 = **19** 与 S3a 标题「19 家」闭合；D 类名单 10 项（Kimi/KimiCode/Workbuddy/Openclaw/Roocode/Kilocode/Cline/Anythingllm/Kiro/Codebuddy）逐一对上源码闸门行 | 通过 |
| C14 | 「每条 assistant 消息 +1」跨分片冲突排查 | ✅ **不冲突**：S4 `E1` 把该措辞仅在 **Claude** 名下改正为「主会话 user 文本消息」；S3b §7/§8 用同措辞指 omp/omo/pi，我实证两者均有 `role !== "assistant"` 过滤（`15856`、`16438`）⇒ S3b 表述成立 | 通过 |

---

## 错误（E）/ 不完整（I）/ 无法验证（U）

### E — 错误（须修）

| 编号 | 位置 | 问题 | 正确值 / 事实 |
|---|---|---|---|
| E1 | S3a §3.10 | `"auto"` 兜底常量行号 | `10672` → **`10678`**（`10677: function resolveWorkbuddyDefaultModel(…)`） |
| E2 | S3a §3.10 | 三类数据源注释行号 | `10688` → **`10694`** |
| E3 | S3a §3.15 | Unsloth 有界扫描/快路径行号 | `13704` → **`13714`**（判定在 `13717`） |
| E4 | S3a §3.15 | Unsloth 落桶函数行号 | `13720` → **`13730`** |
| E5 | S3a §7.1 第 7 条 + 修正记录第 7 条 | Droid 调用点/注释行号（R1 已「核」但仍为 GBK 口径，+10 漂移） | `15333`/`15338` → **`15343-15346`（注释）/ `15348`（调用点）**；`resolveDroidModel` 定义在 **`15046`** |
| E6 | S3a 修正记录 E12 | 「`sync.js:6595`（全集唯一出现点）」不成立 | 定义在 **`rollout.js:15069`**；`sync.js:6595` 为注释；全仓库 4 处命中（`rollout.js:15069/15165/24839`、`sync.js:6595`） |
| E7 | S3a §3.8 | KimiCode 兜底常量行号 | `9697` → **`9704`** |
| E8 | S3a §3.9 | Codebuddy 兜底常量行号 | `9918` → **`9924`** |
| E9 | S3b §8 第 7 点 | MiniMax Code 时间戳路径描述**与源码相反** | 「只能靠 entry 级 ISO 串」应改为「外层 `timestamp` 被包装器置 null（`16771`），ISO 回落分支（`16469`）恒不触发；唯一路径是 `message.timestamp` 数值毫秒（`16467`）；缺失即丢弃（`16473-16476`）」 |
| E10 | S1 §2.5（`239` 行，跨分片） | 45 项分解算术 = 46 | 12+1+1+17+15 = 46 ≠ 45；需 S1 侧复核各子集定义 |

> E1–E8 的偏移量：`+6`（≈10700 行处，2 处）、`+10`（≈13700/15300 行处，4 处）、`+6/+7`（≈9700/9900 行处，2 处）——**与 S3a 自述的 GBK 漂移表（10000 行处≈6、13000 行以上≈10）精确吻合**，属同一根因的未清残渣。

### I — 不完整 / 不精确（建议修）

| 编号 | 位置 | 问题 |
|---|---|---|
| I1 | S3a 修正记录 E1 | 「常量使用点补漏列的第 4 处 `8428`」：`8428` 是注释行，实际使用在 `8429`/`8430`；全文件引用 8118(定义)+8386/8387+8429/8430+9155/9156/9158，「第 4 处」口径不可复现 |
| I2 | S3a §3.17 | 「`14687` 的 continue 条件是三者皆 0」→ 实为 `14686`（`14687` 空白） |
| I3 | S3b E8 | 「16 行」✅ 精确；「**14 个语句**」不可复现——我按独立出现点重算为 **15**（`20286-20287` 为跨行单语句） |
| I4 | S3b §12 第 9 点 | 「见 **§12** 消费侧」在 §12 内自指（该段本身即 §12）；应指向 App DB（§13）或写「本节」 |
| I5 | S3b §13 第 8 点 | `19438-19439` → 语句实为 `19439` |
| I6 | S3b §15 | `billedPlanner` 判定引 `21788`，声明实为 `21787`（`21788` 空白） |
| I7 | S4 §4.8（`823` 行，跨分片） | 引 `9287` 为「刻意置 0 以避免重复计数」，实为**负值 clamp**（`9283-9288`） |
| I8 | S3b §7 / §20（跨分片） | OMP 的 hook 状态缺项：S1 明确有 extension hook（`omp-hook.js` / `tokentracker-notify.ts`），S3b 全文未提 |
| I9 | S3a §0 + S3b §0（口径） | 两份分片各自给 19 / 21 家，但未给「去重后唯一 provider 实体数」；`opencode` 在两片各占一行 |

### U — 无法验证 / 本轮未覆盖

| 编号 | 内容 | 判定条件 |
|---|---|---|
| U1 | S3a 第 9 条 `init.js:1078`（`installOpenclawSessionPlugin` 调用点）本轮**未独立打开** | 需 `read src/commands/init.js` 1070-1090 |
| U2 | S3a 第 5 条 Kimi 侧 `9493`/`9495`/`9497-9502`（`resolveKimiHome`）本轮仅复核 KimiCode 侧（`9669`/`9671` ✅） | 需打开 `rollout.js:9485-9510` |
| U3 | S3a 原 **U2**（Droid `resolveDroidModel` 三级链实现）仍开放，且**判定条件行号有误**（见 E5） | 应读 `rollout.js:15046-15100`（定义）+ `15343-15352`（链与用法），而非原文的 `15300-15345` |
| U4 | S3b 原 U2（云端 ingest 是否从 `cursors.files[].usageSource` 反推）、U3（`geminiPaths` 逐行） | 维持原判定条件，本轮未取得新证据 |
| — | **S3a 原 U1（Workbuddy traces 字段名）本轮已结案**，见 A25 | 无需后续 |

---

## 复核到的行号抽样

> 全部由 `read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` 打开后逐字摘录。

**`src/lib/rollout.js`**
```
1284   const inode = st.ino || 0;
1287-1291  const sameFileMetadata = prev && prev.inode === inode && prev.size === size && prev.mtimeMs === mtimeMs;
7298   function resolveKiroDbPath(basePath) {
7545   const inputTokens = toNonNegativeInt(row.tokens_prompt);
7546   const outputTokens = toNonNegativeInt(row.tokens_generated);
7547   if (inputTokens === 0 && outputTokens === 0) continue;
7567   const bucket = getHourlyBucket(hourlyState, "kiro", model, bucketStart);
8371   let inputTokens = toNonNegativeInt(turn.input_token_count);
8372   let outputTokens = toNonNegativeInt(turn.output_token_count);
8386   inputTokens = Math.floor(promptChars / KIRO_CLI_CHARS_PER_TOKEN);
8387   outputTokens = Math.floor(assistantChars / KIRO_CLI_CHARS_PER_TOKEN);
8429   user_prompt_length: inputTokens * KIRO_CLI_CHARS_PER_TOKEN,
8475-8481  // Kiro CLI 2.13+ … The records do not carry token counts … same 4 chars/token approximation
8482   async function readKiroCliV2SessionTurns(messagesPath) {
9209   if (approxInput > 0 || approxOutput > 0 || approxReasoning > 0) {
9216   reasoning_output_tokens: approxReasoning,
9414   if (input === 0 && output === 0 && cacheRead === 0 && cacheCreation === 0) {
9446   total_tokens: input + output + cacheRead + cacheCreation + reasoning,
9703   function resolveKimiCodeDefaultModel(env = process.env) {
9704   const fallback = "kimi-for-coding";
9918   });                                        ← 分片误引（E8）
9924   const fallback = "codebuddy-unknown";
9940-9943  // Modern CodeBuddy writes token usage into providerData.rawUsage on JSONL records…
10337  const isFromTotalField =
10345  if (inputTokens === 0 && completionTokens === 0 && cacheRead === 0 && cacheCreation === 0) {
10476-10485  if (inputTokens === 0 && completionTokens === 0 && cacheRead === 0 && cacheCreation === 0 && reasoningTokens === 0) { … continue; }
10651  const WORKBUDDY_VARIANTS = {
10662  function resolveWorkbuddyHome(env = process.env, variant = "workbuddy") {
10672  });                                        ← 分片误引（E1）
10678  const fallback = "auto";
10688  } catch (_e) {                             ← 分片误引（E2）
10694  // Recursively collect every *.jsonl under ~/.workbuddy/projects so that
10907  const provider = entry.providerData;
10963  const inputTokens = Math.max(0, promptTokens - cacheRead - cacheCreation);
10965  const reasoningTokens = Math.min(completionTokens, toNonNegativeInt(completionDetails.reasoning_tokens));
10968-10977  五桶零值闸门（含 reasoningTokens）
11004  const bucket = getHourlyBucket(hourlyState, variant, model, bucketStart);
11112  total_tokens: inputTokens + totalCached + totalOutput,          ← traces 支路（U1 结案）
11115  const bucket = getHourlyBucket(hourlyState, variant, model, bucketStart);
11246  total_tokens: inputTokens + completionTokens + cacheRead + cacheCreation + reasoningTokens,  ← SQLite 支路
11706  function resolveKilocodeTaskFiles(env = process.env) {
11730  function normalizeKilocodeProviderToModel(providerName) {
11814  function normalizeRoocodeModel({ explicitModel, apiProtocol }) {
11892  if (msg.say !== "api_req_started" && msg.say !== "api_req_deleted") continue;
11902  const dedupKey = `${taskUuid}:${ts}`;
11910  if (tokensIn === 0 && tokensOut === 0 && cacheReads === 0 && cacheWrites === 0) {
12054  function resolveClineSessionsDirs(env = process.env, deps = {}) {
12178  function normalizeClineModel({ modelInfo, fallbackModel }) {
12350  if (importedAtMs !== null && ts <= importedAtMs) continue;
12356-12365  const cacheRead = …; const inclusiveInput = …; const inputTokens = Math.max(0, inclusiveInput - cacheRead - cacheWrite); … const totalTokens = inputTokens + cacheRead + cacheWrite + outputTokens;
12381  if (totalTokens === 0 && reasoningTokens === 0 && cost === 0) continue;
12403  total_tokens: deltaTotal,
12405  conversation_count: previous ? 0 : 1,
12742  const isReset = currSum > 0 && currSum < prevSum;
12751  const totalDelta = delta.input + delta.output + delta.cache_read + delta.cache_write;
12752  if (totalDelta <= 0) {
12843  // LM Studio
12853  const LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024;
12994  function normalizeLocalStudioTokens(usage) {
13040  input_tokens: Math.max(0, total - completion - cacheRead - cacheWrite),
13047  total_cost_usd: 0,
13048  conversation_count: 1,
13634-13636  0 AS cached_tokens, / 0 AS cache_write_tokens, / 0 AS reasoning_tokens
13691  async function parseUnslothIncremental({
13704  : {};                                      ← 分片误引（E3）
13714  const hasBoundedScan = priorState.scan
13717  if (hasBoundedScan && sameSqliteFingerprint(initialFingerprint, priorState.fingerprint)) {
13730  if (event && reconcilePassiveUsageEvent({
14686  if (totalNow === 0 && inputNow === 0 && outputNow === 0) continue;
14693  const isReset = totalNow > 0 && totalNow < prev.total;
14710  const accountedDelta = dInput + dOutput;
14711  const reasoningDelta = Math.max(0, dTotal - accountedDelta);
15046  function resolveDroidModel(settings, filePath) {
15069  function dedupeDroidSettingsFilesBySession(files) {          ← E12 正解
15333  };                                         ← 分片误引（E5）
15338  const bucketStart = toUtcHalfHourStart(      ← 分片误引（E5）
15343-15346  // Model resolution mirrors ccusage's chain: settings.model → sidecar <id>.jsonl scrape → `<provider>-unknown` …
15348  const model = resolveDroidModel(settings, filePath);
15701  const MIN_OMP_TIMESTAMP_MS = Date.UTC(2020, 0, 1);
15715  const OMP_LIKE_MAX_RECORD_BYTES = 64 * 1024 * 1024;
15856  if (!msg || msg.role !== "assistant") return;
15868-15872  const input = …; const output = …; const cacheRead = …; const cacheWrite = …; const reasoningTokens = pickUsageInt(usage, reasoningFields);
15897-15903  const totalTokens = Number.isFinite(Number(usage.totalTokens)) && … ? … : input + output + cacheRead + cacheWrite + (reasoningIncludedInOutput ? 0 : reasoningTokens);
15907-15915  const delta = { …, reasoning_output_tokens: reasoningTokens, total_tokens: totalTokens, conversation_count: 1 };
15941-15946  // Project attribution has an independent cursor …
16041-16050  // Cap dedup set to last 10k IDs … cappedProjectSeen …
16087-16098  async function parseOmpIncremental(options = {}) { … stateKey: "omp", source: "omp", reasoningFields: ["reasoningTokens"] }
16100-16111  async function parseOmoIncremental(options = {}) { … source: "omo", reasoningFields: ["reasoningTokens","reasoning"], reasoningIncludedInOutput: true }
16157  function piAgentDirCollidesWithOmp(env = process.env) {
16164  function omoAgentDirCollidesWithOmp(env = process.env) {
16171  function resolvePiSessionFiles(env = process.env) {
16286  function piSourceForProvider(provider) {
16297  function primeAgentSourceForProvider(provider) {
16434  const record = readEntry(entry);
16438  if (!msg || msg.role !== "assistant") continue;
16467  if (Number.isFinite(Number(msg.timestamp)) && Number(msg.timestamp) > 0) {
16469  } else if (typeof record.timestamp === "string" && record.timestamp) {   ← E9 证伪点
16473  if (tsMs == null) {
16482-16485  const totalTokens = … ? … : input + output + cacheRead + cacheWrite + reasoningTokens;
16722  const MINIMAX_CODE_SOURCE = "minimax-code";
16766-16773  function readMinimaxCodeEntry(entry) { … timestamp: null, … }   ← E9 证伪点
16783  sourceForProvider: () => MINIMAX_CODE_SOURCE,
17008  function diffReasonixTotals(current, previous = {}) {
17044  conversation_count: delta.requests,
17067  if ((tokenDelta > 0 || delta.requests > 0) &&
17199  if (dInput === 0 && dOutput === 0 && dCacheRead === 0 && dCacheWrite === 0) {
17243  conversation_count: prev.total === 0 ? 1 : 0,
18717-18723  const needsAdoption = …; const baselineOnly = resetWithoutFingerprintHistory || (needsAdoption && !resetDetected && !backfillOnFirstRun);
18822-18824  const matchedOtelUsage = recentEvent && otelUsageMatcher.consume(recentEvent); if (matchedOtelUsage) recentEvent.consumed = true;
18834-18856  if (needsAdoption && backfillOnFirstRun && excludedFirstRunSessions.has(sessionId)) { … priorResidual.storeTokens += normalized.total_tokens; … }
18942-18949  for (const [sessionId, residual] of legacyResiduals) { const baseline = …; const appTotals = normalizeCopilotAppTokenDelta(baseline, {…});
19213-19224  // OTEL remains the owner for recognizable Copilot CLI / VS Code extension sessions …
19357-19379  function normalizeCopilotAppTokenDelta(curr, prev) { … conversation_count: copilotAppBaselineTotal(prev) === 0 && totalTokens > 0 ? 1 : 0 }
19439  if (dbState.lastDbFingerprint && sameSqliteFingerprint(currentFingerprint, dbState.lastDbFingerprint)) {
19551  const GROK_CURSOR_VERSION = 5;
19645  function capGrokSessionSnapshots(sessionSnapshots) {
19959  billable_total_tokens: total,                  ← estimateGrokTokenDelta（19945）
21806  delta.billable_total_tokens = delta.total_tokens;
21831  delta.billable_total_tokens = delta.total_tokens;
22002  const TRAE_CURSOR_VERSION = 2;
22111  if (previous && previous[T_STORE] !== store && modifiedAt <= previous[T_MODIFIED]) continue;
22112  if (!previous && totals.total_tokens === 0) continue;
22322  const TRAE_CN_SOURCE = "trae-cn";
22483-22487  for (const [label, value] of fields) { if (!Number.isSafeInteger(value) || value < 0) { throw … } }
22500-22508  totals: { …, total_tokens: total, billable_total_tokens: total, conversation_count: 1 }
22680-22682  throw new Error(`Trae CN snapshot is not authoritative: …`)
22749-22751  const bucket = getHourlyBucket(hourlyState, TRAE_CN_SOURCE, …); addTotals(bucket.totals, current.totals); touchedBuckets.add(…);
22820  const DSH_SOURCE = "dsh";
24067  const COMMAND_CODE_SOURCE = "command-code";
```
**其它文件**
```
src/lib/pricing/index.js:248-256   const reasoningIncludedInOutput = row.source === "codex" || … "cline"; const reasoningCost = … ? 0 : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
src/lib/passive-mode.js:166/177    WorkBuddy / WorkBuddy AI 条目（hookExpected: true）
src/commands/init.js:119-165       const SUPPORTED_PROVIDERS = [ … "Mimo", "DuMate", "ZCode", … ];   ← 45 项
src/commands/init.js:581/585       ["workbuddy" …] / ["workbuddy-ai" …] 的 upsertClaudeHook 安装点
src/commands/init.js:1078          （U1：本轮未独立打开）
src/commands/sync.js:1386-1388     recordsProcessed/eventsAggregated/bucketsQueued = filesResult.* + dbResult.*
src/commands/sync.js:1949-1951     mergeBothFileSources({ resolveFiles: resolveKilocodeTaskFiles, … })
src/commands/sync.js:2032          prune: true,
src/commands/sync.js:2110          // ── Cline (CLI v3 / desktop app — ~/.cline/data/sessions) ──
src/commands/sync.js:2315-2319     // installs never spin up a parse or seed a cursor namespace. / const kiroHasData = … / if (!kiroHasData(…)) … = null;
src/commands/sync.js:6595          // session showed as 40.06B). The forward fix is dedupeDroidSettingsFilesBySession   ← E6 证伪点
src/commands/status.js:840         ? readTraeEntitlementFromStorage(traeStoragePath)
src/lib/trae-sqlite.js:7-11        import("@journeyapps/wa-sqlite/dist/wa-sqlite.mjs") / "@journeyapps/wa-sqlite" / "…/src/FacadeVFS.js"
src/lib/cursor-config.js:46-47     const CURSOR_ACCESS_TOKEN_SQL = "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken';"
src/lib/cursor-config.js:525-536   // Usage tracking and billing are orthogonal … billable_total_tokens: totalTokens,
src/lib/grok-usage.js:26-39        function reportedCostUsd(usage, { costIsPartial, usageIsIncomplete }) { … }
src/lib/grok-usage.js:83           billable_total_tokens: totalTokens,
src/lib/devin-usage.js:48-51       SQL 只选 input/output/cache_read/cache_creation 四个 metrics 字段
src/lib/devin-usage.js:197         billable_total_tokens: total,
package.json:60-68                 "devDependencies": { "@electric-sql/pglite": "^0.5.8", …, "sql.js": "1.14.2", … }
test/minimax-code-parser.test.js:37-64   assistantLine 结构：{ message_id, turn_id, message: { role, model, provider, timestamp, usage } }
```

---

**未落实项**：无（本轮只读、只报告；未修改样本仓库与任何分片）。
