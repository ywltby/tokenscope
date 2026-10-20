# 复核报告 R1-C：S3a-providers-jsonl.md

## 结论

**需修订**——分片的 19 家事实链主体成立（函数头、路径、字段映射、source 命名、hook 判定、`conversation_count` 语义抽样全部命中），但存在 **2 处结论性错误**（KiroCli「全部按字符估算」与由此推出的 §5 分类）与 **10 处行号错误**，其中 6 处可证明是 **GBK 口径混入**（PowerShell 行号被当作 `read` 行号写进正文），另 2 处（`12190`、`1307`）两种口径都不匹配。

B4 与 B5 的证伪结论：

- **B4 成立**（Gemini 确为唯一「`total_tokens` 非零而全部计价桶为 0」的一家），但其唯一性依赖 `pricing/index.js:248-256` 的推理计费集合（`codex/acode/every-code/omo/cline` 之外推理**按 output 价计费**），即 Goose 的 `reasoning>0` 行不是 $0——分片没写出这条依赖，属**论证不完整但结论正确**。
- **B5 不成立**（KiroCli 并非全部估算：legacy 会话 `.json` 路径优先读真实 `turn.input_token_count` / `turn.output_token_count`，仅两者都是 0 时才回退 `floor(chars/4)`）。credits 只落 sidecar 的部分**成立**。

---

## 抽样统计

- 抽验断言数：**141**（其中 **E=12, I=7, U=2**，其余 120 条为通过）
- 构成：§7.1 第 10 条行号清单 40 条全部重验 + §7.1 其余存疑项 10 条 + 19 家解析器函数头 19 条 + B4 独立扫描（19 家 `total_tokens` 赋值点 20 + 前置闸门 15 + 计价侧 3）+ B5 专项 12 条 + hook/被动判定 12 条 + `conversation_count` 14 条 + 正文行号引用约 20 条
- 实际打开过的源码文件清单：
  - `src/lib/rollout.js`（24932 行；读段：55–61、220–259、4732–4789、4890–4989、2930–2989、3100–3150、3322–3351、3802–3810、4250–4274、5015–5044、7272–7351、7535–7579、7712–7725、7975–8015、8060–8090、8100–8179、8312–8396、8475–8504、8940–8979、9025–9049、9140–9239、9395–9459、9486–9515、9583–9602、9660–9689、9925–10034、10138–10151、10310–10414、10415–10491、10495–10531、10518–10531、10647–10671、10904–10913、10960–11014、11700–11790、11785–11824、11890–11939、12150–12194、12375–12424、12588–12605、12712–12761、12750–12809、12812–12871、12890–12929、12988–13052、13610–13687、13872–13901、13995–14034、14038–14057、14663–14687、14695–14744、14968–14979、15374–15385、15620–15659）
  - `src/lib/passive-mode.js`（全文 219 行）
  - `src/lib/pricing/index.js`（220–359）
  - `src/commands/init.js`（560–634）
  - `src/commands/sync.js`（1098–1111、1378–1389、1903–1914、2005–2044、2058–2071、2104–2115、2312–2321）
  - 双向口径探针：`Get-Content`（GBK）与 `[System.IO.File]::ReadAllLines(...,UTF8)` 全文比对（见「行号漂移统计」）

---

## 错误（E）—— 必须修

- **[E1] §3.6 + §0 速览 + §6 速查表**：断言「**本家没有真实 token 字段，全部按「字符数 ÷ 4」估算**」。
  实际：只有 SQLite `conversations_v2` 路径与 v2 `messages.jsonl` 路径是纯字符估算；**legacy 会话 `.json` 路径优先读真实用量**（`src/lib/rollout.js:8371`）：
  ```js
  // Prefer real token counts if kiro-cli populated them.
  let inputTokens = toNonNegativeInt(turn.input_token_count);
  let outputTokens = toNonNegativeInt(turn.output_token_count);

  if (inputTokens === 0 && outputTokens === 0) {
    ...
    inputTokens = Math.floor(promptChars / KIRO_CLI_CHARS_PER_TOKEN);   // 8386
    outputTokens = Math.floor(assistantChars / KIRO_CLI_CHARS_PER_TOKEN); // 8387
  }
  ```
  另外 `KIRO_CLI_CHARS_PER_TOKEN` 的使用点还有 `src/lib/rollout.js:8428`（分片只列了 `8118`/`9155`）。
  修正建议：改为「SQLite 与 v2 `messages.jsonl` 两支纯字符估算；legacy `.json` 支优先 `input_token_count`/`output_token_count`，两者皆为 0 才回退字符估算」，并在 §0 速览「数据形态」列注明。

- **[E2] §5 分类**：把 **KiroCli 归入 E 类「结构上不可能」**。
  实际：legacy 会话支路的零值闸门只检 4 列、**不含 reasoning**（`src/lib/rollout.js:9414`）：
  ```js
  if (input === 0 && output === 0 && cacheRead === 0 && cacheCreation === 0) { maxIndex = i; continue; }
  ```
  紧随其后的 delta `total_tokens: input + output + cacheRead + cacheCreation + reasoning`（`src/lib/rollout.js:9446`）→ 可产出 `input=0 / output=0 / reasoning>0 / total>0` 的行，与 §5 B 类的 Goose 同形。
  修正建议：KiroCli 从 E 类移入 B 类（弱化版），并说明它在 `pricing/index.js:248-256` 下不是 $0（推理按 output 计费）。

- **[E3] §7.1 第 10 条 Kiro `7294`**：实际 `7294` 是 linux 分支收尾的 `}`；`resolveKiroDbPath` / `resolveKiroJsonlPath` 在 **7298–7304**（`src/lib/rollout.js:7298`）。该数字 = GBK 口径（UTF-8 7298 → GBK 7294）。
  修正建议：`7294` → `7298`（或函数体区间 `7298–7304`）。

- **[E4] §3.9 + §7.1 第 10 条 Codebuddy `10331`（引作「非缓存输入修正」块头）**：实际该块在 **10337–10343**；`10331` 是 `let inputTokens = toNonNegativeInt(inputRaw);`。GBK 口径（UTF-8 10337 → GBK 10331）。
  修正建议：`10331` → `10337`。

- **[E5] §3.9 与 §5 F 的 Codebuddy 扩展日志零值闸门 `10339`**：实际闸门在 **10345**；`10339` 是 `usage.cacheMissTokens === undefined &&`。GBK 口径（UTF-8 10345 → GBK 10339）。同时 §5 F 称该判定「未复核行号」。
  修正建议：`10339` → `10345`。

- **[E6] §7.1 第 1 条 `9938`（自称「read 确认」）**：实际 `9938` 是 `}`；说明「Modern CodeBuddy writes token usage into `providerData.rawUsage`」的注释在 **9940–9943**（`src/lib/rollout.js:9940`）。该数字 = GBK 位（UTF-8 9940 → GBK 9934；GBK 9938 则落在 `function codebuddyJsonlHasUsage` 头 9944 附近）。
  修正建议：`9938` → `9940`，并删掉「read 确认」字样。

- **[E7] §7.1 第 10 条 Cline `12190`**：实际 `12190` 是 `normalizeClineModel` 的闭合 `}`；函数头在 **12178**，四级兜底链在 **12179–12189**（`src/lib/rollout.js:12178`）。该数字**两种口径都对不上**（UTF-8 12178 / GBK 12169）。
  修正建议：`12190` → `12178`。

- **[E8] §3.13「线程累计差分（`src/lib/rollout.js:12732` 的 isReset）」**：实际 `isReset` 在 **12742**；`12732` 是 `const prev = threadTotals[row.id] || {...}`（差分基线，非 isReset）。GBK 口径（UTF-8 12742 → GBK 12732）。
  修正建议：把「isReset」的引用改为 `12742`。

- **[E9] §3.15 标题的 `parseUnslothIncremental`，`src/lib/rollout.js:13681`**：实际函数头在 **13691**；`13681` 是 `normalizeUnslothUsageRow` 内部的 `if (!id || !totals || !bucketStart) return null;`。GBK 口径（UTF-8 13691 → GBK 13681）。
  修正建议：`13681` → `13691`。

- **[E10] §3.1「文件身份含 inode（`src/lib/rollout.js:1307`）」**：实际 `1307` 是 `let lastModel = prev && prev.inode === inode ? prev.lastModel || null : null;`；inode 身份在 **1284**（`const inode = st.ino || 0;`）与 **1287–1291**（`sameFileMetadata`）。该数字两种口径都对不上。
  修正建议：`1307` → `1284` 或 `1287–1291`。

- **[E11] §7.1 第 10 条的归属矛盾**：`12845` / `12853` 被列在「**Zed 的** 12722/12726/12732/12816/12845/12853」名下，但两者都是 **LM Studio** 模块（`12845` = LM Studio 模块注释首行、`12853` = `const LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024;`），且 §3.14 正文自己就按 Lmstudio 引用。同一份文档两处归属互相打架。
  修正建议：从 Zed 清单删除 12845/12853，只保留 Lmstudio 条目。

- **[E12] §3.18「sync 侧去重（`dedupeDroidSettingsFilesBySession`，`src/commands/sync.js:2013`）」**：全文件仅出现一次，在 **`src/commands/sync.js:6595`**；`2013` 是 Droid 段落的文件合并起点（`const droidSettingsFiles = sourceAllowed("droid") ? mergeBothFileSources({...})`）。
  修正建议：引用改为 `sync.js:6595`，或把 `2013` 改述为「Droid 文件合并起点」。

---

## 不完整（I）—— 建议补

- **[I1] §5 F 类「Codebuddy 未判定」可结案**。JSONL 支路**有**五桶零闸门（`src/lib/rollout.js:10476–10485`，含 `reasoningTokens`），扩展日志支路闸门在 `10345`。结论：Codebuddy 应移入 **D 类**（丢纯 total 行），F 类清空。
- **[I2] §5 E 类「Zed 结构上不可能」措辞过强**。Zed 的 `totalDelta` 只由 input/output/cache_read/cache_write 求和（`src/lib/rollout.js:12751`）并 `totalDelta <= 0` 跳过（`12752`）——「全零 → $0」确实不成立，但 `input=0 / output=0 / total>0` 仍可出现（**只有缓存列增长**时）。同理适用于 Cline（`12403`）。建议加一句限定，避免下游把「不可能」当无条件。
- **[I3] §0 速览表未记 Opencode 的 SQLite 支路**。速览第 2 行「数据形态：单消息 JSON 目录」漏记；正文 §3.2 已承认「同寄主另有 SQLite 支路」，且 `src/lib/rollout.js:5038` 起确有 `OpenCode SQLite DB reader`（`sync.js:1386–1388` 两路相加）。
- **[I4] §1.5 引 `12994` 称其为「return 块」**：`12994` 是 `function normalizeLocalStudioTokens(usage)` 函数头，被引用的 return 块在 **13039–13049**（`conversation_count: 1` 在 13048、`total_cost_usd: 0` 在 13047）。行号可用但归属描述不准。
- **[I5] §7.1 第 4 条可结案**：会话侧字段名不是模块注释 8109 的 `user_prompt_length`/`response_size`，而是 `input_token_count` / `output_token_count`（`src/lib/rollout.js:8371–8372`）；`readKiroCliV2SessionTurns`（`8482`）另外读 `messages.jsonl`（`8475–8481` 注释说明其无 token 字段、纯 4 字符/token）。建议把该条从「未核对」改为结论。
- **[I6] §7.1 第 2 条可结案**：`walkOpencodeMessages`（`src/lib/rollout.js:5025`）确认只收 `<storage>/message/**` 下 `msg_*.json`（`5033`），「消息文件在 `<storage>/message/**`」成立。
- **[I7] §7.1 第 8 条可结案**：Anythingllm 的 SELECT 本体在 `src/lib/rollout.js:13874–13887`（`FROM workspace_chats`，列经 `json_extract(response,'$.metrics.*')`），SQL 层只过滤 `whereClause`（游标/`include`），**不在 SQL 层做零值过滤**——与 §3.16「纯 total 行会被丢弃（在 JS 侧 14016）」自洽。另外 §7.1 第 3 条也随之可结案：`resolveClineSessionsDirs`（`rollout.js:12054`）、`scanClineSessionFiles`（`12086`）。

---

## 无法验证（U）

- **[U1] §7.1 第 6 条（Workbuddy traces 支路字段名）**：本次未读 traces 分支的完整字段映射段，故「traces 兜底字段名」是否与 JSONL 支路同形无法判定。需读 `src/lib/rollout.js` 11100–11260 段（`total_tokens: ...` 在 11112/11246 两处，疑为 traces/兜底路径）才能结案。
- **[U2] §7.1 第 7 条（Droid `resolveDroidModel` 全链）**：本次只核到调用点（`15338` 附近）与 `applyDroidTotalFallback`（`14972`）逐字一致，未展开三级链实现，无法判定 `*-unknown` 兜底是否与正文描述一致。

---

## 行号漂移统计

- **§7.1 第 10 条清单（40 条）逐条重验：正确 35 条 / 错 5 条，漂移率 12.5%。**
- **全文抽样口径正确率：约 120/132 ≈ 91%**（132 条为本次逐条核对的行号类断言）。
- **关键机制发现**：`Get-Content`（GBK）与 UTF-8 的行号在 **~1300 行之前完全相同**（首个分歧点在 2000 行附近，delta=1），因此小行号引用「看起来是对的」；漂移随文件增大累积，**7000 行处 delta=4，10000 行处 delta=6，13000 行以上 delta=10，文件末 delta=15**（24932 → 24917）。
- 分片的实际错误分布与之吻合：错行号集中在 **≥7000 行**（7294/10331/10339/13681/12732 全部 = UTF-8 行号 −(4~10) = GBK 行号），即 **PowerShell 口径混入正文**；而 `12190`、`1307` 两种口径都不匹配，属无来源数字。
- 分片 §7.1 的自查清单本身**不准确**：它把「仅由 PowerShell 定位」的条目全列为「未复核」，但其中 35 条其实与 `read` 口径一致；反而有一条**自称「read 确认」**的 `9938` 是 GBK 口径（见 E6）。自查清单不能替代逐条重验。

### 行号漂移表（§7.1 第 10 条全量 40 条重验）

| 分片行号 | 实际行号（UTF-8/`read`） | 实际内容（摘录） | 判定 |
|---|---|---|---|
| 1265 | **1265** | `const defaultSource = normalizeSourceInput(source) \|\| "gemini";` | ✅ |
| 1275 | **1275** | `const fileSource =` | ✅ |
| 1283 | **1283** | `const prev = cursors.files[key] \|\| null;` | ✅ |
| 1307 | 1284 / 1287–1291 | `const inode = st.ino \|\| 0;` / `sameFileMetadata` 判等（1307 实为 `let lastModel = ...`） | ❌ 见 E10 |
| 1361 | **1361** | `cursors.files[key] = {`（`lastIndex`/`lastTotals`/`lastModel` 在 1365–1367） | ✅ |
| 1403 | **1403** | `cursors.hourly = hourlyState;` | ✅ |
| 1438 | **1438** | `const defaultSource = normalizeSourceInput(source) \|\| "opencode";` | ✅ |
| 1448 | **1448** | `const fileSource =` | ✅ |
| 1513 | **1513** | `cursors.files[key] = {`（`lastTotals`/`messageKey` 在 1517–1518） | ✅ |
| 7282 | **7282** | `const suffix = ["Kiro", "User", "globalStorage", "kiro.kiroagent"];` | ✅ |
| 7294 | 7298（–7304） | `function resolveKiroDbPath(basePath) {`（7294 是 `}`） | ❌ 见 E3 |
| 7305 | 7306（–7331） | 空行；`function readKiroDbTokens(dbPath, sinceId, ...)` 在 7306 | ⚠️ −1（指向空行，实质容差内） |
| 7334 | **7334** | `// Each line: {"model":"agent","provider":"kiro","promptTokens":N,...}` | ✅ |
| 7338 | **7338** | `function countKiroJsonlLines(jsonlPath) {` | ✅ |
| 8132 | **8132** | `return path.join(localAppData, "kiro-cli", "data.sqlite3");` | ✅ |
| 8138 | **8138** | `return path.join(dataHome, "kiro-cli", "data.sqlite3");` | ✅ |
| 8140 | **8140** | `return path.join(home, "Library", "Application Support", "kiro-cli", "data.sqlite3");` | ✅ |
| 8165 | **8165** | `function resolveKiroCliSessionFiles(env = process.env) {` | ✅ |
| 9979 | **9979** | `function codebuddyUsageFingerprint({ model, timestampMs, ... }) {` | ✅ |
| 10022 | **10022** | `function capCodebuddyUsageFingerprints(state, maxEntries = 10_000) {` | ✅ |
| 10141 | **10141** | `function parseLogTimestampMs(line, fallbackTs) {` | ✅ |
| 10320 | **10320** | `usage.completionThinkingTokens,`（reasoning 候选列表内） | ✅ |
| 10322 | **10322** | `]);`（紧接 10318 的 `firstPresentNonNegativeInt([`） | ✅ |
| 10331–10339 | 10337–10345 | `const isFromTotalField =`（10337）… `if (inputTokens === 0 && ...)`（10345） | ❌ 见 E4/E5（−6） |
| 10525 | **10525** | `const bucket = getHourlyBucket(hourlyState, "codebuddy", model, bucketStart);` | ✅ |
| 11749 | **11749** | `//   1. The model name is NOT in the per-turn payload (Roo Code only writes` | ✅ |
| 11762 | **11762** | `//   2. Same multi-IDE root scan as Kilo Code (Cursor, Code, CodeBuddy, …) —` | ✅ |
| 11790 | **11790** | `function readRoocodeTaskModel(uiMessagesPath) {` | ✅ |
| 11805 | **11805** | `const re = /<model>\s*([^<\s][^<]*?)\s*<\/model>/g;` | ✅ |
| 11814 | **11814** | `function normalizeRoocodeModel({ explicitModel, apiProtocol }) {` | ✅ |
| 12157 | **12157** | `// mid-session. Imported sessions also retain the source transcript's usage;` | ✅ |
| 12190 | 12178（–12189） | `function normalizeClineModel({ modelInfo, fallbackModel }) {`（12190 是 `}`） | ❌ 见 E7 |
| 12722 | **12722** | `let jsonText;` | ✅ |
| 12726 | **12726** | `let thread;` | ✅ |
| 12732 | **12732** | `const prev = threadTotals[row.id] \|\| { input: 0, ... };` | ✅（正文把 isReset 挂在此处 → E8） |
| 12816 | **12816** | `if (entries.length > 10_000) {`（threadTotals 上限） | ✅ |
| 12845 | **12845** | `// Data: pretty-printed OpenAI-compatible final responses beneath`（**LM Studio 模块**） | ✅ 行号对，但归属 Zed → E11 |
| 12853 | **12853** | `const LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024;`（**LM Studio**） | ✅ 行号对，但归属 Zed → E11 |
| 12897 | **12897** | `function scanLmstudioUsageObjectStart(buffer, from = 0) {` | ✅ |
| 12917 | **12917** | `function lmstudioBalancedObjectEnd(buffer, start) {` | ✅ |

### §7.1 其余存疑项重验

| 分片条目 | 分片行号 | 实际行号 | 实际内容 | 判定 |
|---|---|---|---|---|
| 第 1 条 providerData.rawUsage 注释 | 9938 | **9940–9943** | `// Modern CodeBuddy writes token usage into providerData.rawUsage on JSONL` | ❌ E6 |
| 第 2 条 walkOpencodeMessages | 5025 | **5025** | `async function walkOpencodeMessages(dir, out) {`（只收 `msg_*.json`，5033） | ✅ 可结案 |
| 第 2 条 listOpencodeMessageFiles | 243 | **243** | `async function listOpencodeMessageFiles(storageDir) {` | ✅ |
| 第 3 条 resolveClineSessionsDirs / scanClineSessionFiles | 未给 | **12054 / 12086** | 两函数均存在（在 `rollout.js`，不在 `sync.js`） | ✅ 可结案 |
| 第 3 条 `~/.cline/data/sessions` | sync.js:2109 | **2110** | `// ── Cline (CLI v3 / desktop app — ~/.cline/data/sessions) ──` | ✅ ±1 |
| 第 4 条 模块注释字段名 | 8109 | **8109–8111** | `// Kiro CLI does NOT store explicit token counts locally. Each request row carries: user_prompt_length (chars), response_size (chars), model_id,` | ✅ 但与会话支路字段名冲突 → E1/I5 |
| 第 4 条 readKiroCliSessionTurns | 8316 | **8316** | `async function readKiroCliSessionTurns(jsonPath) {`（真实字段 8371–8372） | ✅ |
| 第 4 条 readKiroCliV2SessionTurns | 8482 | **8482** | `async function readKiroCliV2SessionTurns(messagesPath) {` | ✅ |
| 第 4 条 writeKiroCliCreditsSidecar | 8701 | **8701** | `async function writeKiroCliCreditsSidecar({` | ✅ |
| 第 5 条 resolveKimiHome | 9493 | **9493** | `function resolveKimiHome(env = process.env) {`（`KIMI_HOME` 9495、win32 WSL 9497–9502） | ✅ 可结案 |
| 第 5 条 resolveKimiCodeHome | 9669 | **9669** | `function resolveKimiCodeHome(env = process.env) {`（`KIMI_CODE_HOME` 9671） | ✅ 可结案 |
| 第 9 条 init.js OpenClaw 插件安装 | ~1078 | **init.js:1078** | `installOpenclawSessionPlugin` 首次出现即 1078（另在 54 行 import） | ✅ 可结案 |

---

## 复核到的行号抽样（证明真的打开过）

**19 家解析器函数头（逐条对齐分片 §3.x 标题行号）：**

- `src/lib/rollout.js:1242` → `parseGeminiIncremental` ✅
- `src/lib/rollout.js:1412` → `parseOpencodeIncremental` ✅
- `src/lib/rollout.js:1654` → `parseOpenclawIncremental` ✅
- `src/lib/rollout.js:7472` → `parseKiroIncremental` ✅
- `src/lib/rollout.js:7900` → `parseHermesIncremental` ✅
- `src/lib/rollout.js:8886` → `parseKiroCliIncremental` ✅
- `src/lib/rollout.js:9528` → `parseKimiIncremental` ✅
- `src/lib/rollout.js:9723` → `parseKimiCodeIncremental` ✅
- `src/lib/rollout.js:10164` → `parseCodebuddyIncremental` ✅
- `src/lib/rollout.js:10803` → `parseWorkbuddyIncremental` ✅
- `src/lib/rollout.js:11824` → `parseRoocodeIncremental` ✅
- `src/lib/rollout.js:12200` → `parseClineIncremental` ✅
- `src/lib/rollout.js:12650` → `parseZedIncremental` ✅
- `src/lib/rollout.js:13267` → `parseLmstudioIncremental` ✅
- `src/lib/rollout.js:13691` → `async function parseUnslothIncremental({` ❌ 分片写 13681（E9）
- `src/lib/rollout.js:13938` → `parseAnythingllmIncremental` ✅
- `src/lib/rollout.js:14604` → `parseGooseIncremental` ✅
- `src/lib/rollout.js:15137` → `parseDroidIncremental` ✅
- `src/lib/rollout.js:15545` → `parseKilocodeIncremental` ✅
- `src/lib/rollout.js:12136` → `function resolveClineSessionFilesWithStatus(env = process.env, deps = {}) {` ✅

**B4 关键证据链（逐字核对）：**

- `src/lib/rollout.js:4945` → `function isAllZeroUsage(u) {`，六列含 `"total_tokens"`（4947–4956）✅ 与分片引文一致
- `src/lib/rollout.js:4741–4743` → `const reportedTotal = toNonNegativeInt(tokens.total);` / `const computedTotal = input + cached + output + tool + thoughts;` / `const total = Math.max(reportedTotal, computedTotal);` ✅
- `src/lib/rollout.js:4734` → `function normalizeGeminiTokens(tokens) {` ✅
- `src/lib/rollout.js:4751` → `total_tokens: total,`（Gemini 桶）✅
- `src/lib/rollout.js:2970–2975` → `const delta = diffGeminiTotals(currentTotals, totals);` / `if (!delta || isAllZeroUsage(delta)) {` / `totals = currentTotals;` / `} else {` / `delta.conversation_count = 1;` / `bucketStart = toUtcHalfHourStart(timestamp);`（分片引作 2968，**实际块起 2970**）
- `src/lib/rollout.js:2956–2959` → `const timestamp = typeof msg.timestamp === "string" ? msg.timestamp : null;` … ✅ 与分片引文一致
- `src/lib/rollout.js:4964` → `function toNonNegativeInt(v) {` ✅
- `src/lib/pricing/index.js:227` → `function computeRowCost(row) {`；**248–256** 关键：
  ```js
  const reasoningIncludedInOutput =
    row.source === "codex" || row.source === "acode" || row.source === "every-code" ||
    row.source === "omo" || row.source === "cline";
  const reasoningCost = reasoningIncludedInOutput ? 0 : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
  ```
  → 集合外的 source（含 `gemini`/`goose`/`kiro`）**推理按 output 单价计费**；Gemini 的 $0 行要求六列同时为 0，Goose 的 reasoning 行会被计价 ⇒ 分片「Gemini 唯一」结论成立，但依据未写出

**19 家 `total_tokens` 赋值点独立扫描（分片 B4 唯一性复查）：**

| Provider | 赋值点 | 形态 | 是否可「全零桶 + total>0」 |
|---|---|---|---|
| Gemini | 4751 / 4743 | `max(reported, computed)` + 闸门 2971 | **是**（唯一） |
| Opencode | 4770 | 五桶求和 + 闸门 3110 | 否 |
| Openclaw | 2105 | 四桶求和 + 闸门 2109 | 否 |
| Kiro | 7563 | `input+output` + 闸门 7547 | 否 |
| Hermes | 8014 | 五桶求和 + SQL WHERE 7718 | 否 |
| KiroCli | 9217（SQLite）/ **9446（会话）** | 求和 / 求和（闸门 9414 不含 reasoning） | 否（但可 input=0/output=0/reasoning>0 → E2） |
| Kimi | 9607 | 四桶求和 + 闸门 9590 | 否 |
| KimiCode | 9840 | 四桶求和 + 闸门 9823 | 否 |
| Codebuddy | 10380（日志）/ 10520（JSONL） | 五桶求和 + 闸门 10345 / **10476–10485** | 否（F 类可结案） |
| Workbuddy | 10999 | 五桶求和 + 闸门 10968 | 否 |
| Roocode | 11931 | 四桶求和 + 闸门 11910 | 否 |
| Cline | 12403 | 四桶求和 + 闸门 12381/12390 | 否 |
| Zed | 12778 | 四桶求和（无 reasoning）+ 闸门 12752 | 否（但 cache-only 可 input=0/output=0 → I2） |
| Lmstudio | 13045 | `normalizeLocalStudioTokens` 13040 反推 | 否（C 类，正确） |
| Unsloth | 13671 / 13040 | 同上（SQL 投影 13634–13636 写死三个 0） | 否 |
| Anythingllm | 14049 | `accounted + reasoning` + 闸门 14016 | 否 |
| Goose | 14726 | `dInput+dOutput+reasoningDelta`，闸门 14697 | 否，但 reasoning>0 可单独落（B 类，正确） |
| Droid | 15378 | 五桶求和 + `applyDroidTotalFallback`（14972） | 否（C 类，正确） |
| Kilocode | 15646 | 四桶求和 + 闸门 15628 | 否 |

**B5 专项（KiroCli）：**

- `src/lib/rollout.js:8118` → `const KIRO_CLI_CHARS_PER_TOKEN = 4;` ✅
- `src/lib/rollout.js:8119` → `const KIRO_CLI_CREDITS_SIDECAR = "kiro-credits.json";` ✅
- `src/lib/rollout.js:9152–9159` → `user_prompt_length`/`response_size`/`reasoning_size` → `Math.floor(x / KIRO_CLI_CHARS_PER_TOKEN)` ✅
- `src/lib/rollout.js:8386–8387`、`8428` → 另外三处字符估算使用点（分片漏列）
- `src/lib/rollout.js:8370–8372` → `// Prefer real token counts if kiro-cli populated them.` + `turn.input_token_count` / `turn.output_token_count` ❌ 推翻「全部估算」
- `src/lib/rollout.js:9191–9204` → 负数 delta 回退（`conversation_count: -1` 在 9203）✅
- `src/lib/rollout.js:9209–9210` → `if (approxInput > 0 || approxOutput > 0 || approxReasoning > 0) {` / `getHourlyBucket(hourlyState, "kiro", ...)` ✅
- `src/lib/rollout.js:9161–9173` → `request_start_timestamp_ms` / 指纹 `${promptChars}:${responseChars}:${reasoningChars}:${model}:${tsMs}` ✅
- `src/lib/rollout.js:9036` → `for (const [reqId, prev, sid] of toRetract) {`（迁移后撤回通道）✅
- credits 全链路（`8926–8967`）：`kiroCredit*` 五个累加变量**只**喂给 `writeKiroCliCreditsSidecar`（8955），`grep` 全文件无第二处消费者 ⇒「credits 不进 token 桶」**成立** ✅

**hook vs 被动（重点 4）抽样 ≥6 家：**

- `src/lib/passive-mode.js:97/109/122/133/145/156/166/177` → 8 个 `hookExpected: true` 条目：`claude`/`gemini`/`codex`/`acode`/`every_code`/`codebuddy`/`workbuddy`/`workbuddy-ai` ✅ 与分片一致
- `src/lib/passive-mode.js:191` → `const passive = hookExpected && !hookInstalled && logsPresent;` ✅ 逐字一致
- `src/lib/passive-mode.js:89–90` → 注释里的 `hookStatus` 形状确含 `opencode`/`openclaw`/`grok`，而 `detectPassiveProviders` 实现**未**产出这三者 ✅ 分片「以实现为准」的提醒正确
- `src/lib/passive-mode.js:13–15` → `…Our parsers can read those logs directly without the hook; the only loss is "data is fresh within seconds"…` ✅
- `src/commands/init.js:568–589` → `const hookRepairs = [` 五条（claude / gemini / codebuddy / workbuddy / workbuddy-ai）✅ 与分片引文逐字一致
- `src/commands/init.js:577` → `["codebuddy", context.codebuddyDir, () => upsertClaudeHook({` ✅
- `src/commands/init.js:623–625` → CodeBuddy 是 Claude-Code fork、同一 `settings.json` hook schema 的注释 ✅
- `src/lib/grok-hook.js` / `omp-hook.js` / `openclaw-hook.js` / `openclaw-session-plugin.js` 四文件确实存在于 `src/lib/`，`src/lib/` 下**无**其他 `*hook*` 文件 ✅（分片「证据一」成立）
- `src/commands/sync.js:1102–1103` → `if (sourceAllowed("openclaw") && ...) {` / `// Parses plugin-triggered and/or passively discovered session files.` ✅（分片引 1102，实为 1103，±1）
- `src/lib/rollout.js:1612` → `const OPENCLAW_TRANSCRIPT_SUBDIRS = ["sessions", "session-sqlite-import-archive"];` ✅
- `src/lib/rollout.js:1618` → `async function resolveOpenclawSessionFiles(env = process.env, deps = {}) {` ✅
- `src/lib/rollout.js:8078` → `// No hook installation needed; Kimi writes wire.jsonl automatically.` ✅
- `src/lib/rollout.js:12054/12086` → Cline 目录发现与扫描函数 ✅
- `src/commands/sync.js:1386–1388` → Opencode 文件路径与 SQLite 路径的结果相加 ✅
- `src/commands/sync.js:2316–2317` → `const kiroHasData = (base) => ... resolveKiroDbPath(base) || resolveKiroJsonlPath(base)` ✅（分片引 2315，±1）
- `src/commands/sync.js:2063` → `zedResult = await multiInstallParse({ paths: zedPaths, parserFn: parseZedIncremental, ...})` ✅（分片引 2062，±1）
- `src/commands/sync.js:2032` → `prune: true,`（Droid）✅（分片引 2031，±1）
- `src/commands/sync.js:1909` → `// ── AnythingLLM Desktop (workspace_chats.response.metrics) ──` ✅（分片引 1908，±1）

**`conversation_count` 语义（重点 5）抽样 5+ 家：**

- Gemini：`src/lib/rollout.js:2974` → `delta.conversation_count = 1;`（**产生差分的消息**计 1）✅
- Opencode：`src/lib/rollout.js:3120` → `delta.conversation_count = 1;` ✅
- Kimi / KimiCode：`src/lib/rollout.js:9608` / `9841` → `conversation_count: 1,` ✅（每条事件 +1）
- Cline：`src/lib/rollout.js:12405` → `conversation_count: previous ? 0 : 1,`（**仅首次**）✅
- Droid：`src/lib/rollout.js:15379` → `conversation_count: isFirstSeenSession || isReset ? 1 : 0,` ✅
- Goose / Lmstudio·Unsloth：`src/lib/rollout.js:14727` / `13048` → 恒 `1` ✅
- Hermes：`src/lib/rollout.js:8015` → `conversation_count: dMessageCount,`（DB `message_count` 差分，`7987/8002` 处 `dMessageCount`）✅
- Zed：`src/lib/rollout.js:12779` → `conversation_count: 1,`（每次**线程更新**计 1，非会话数）✅
- 另有 Kilocode `15647`、Roocode `11932`、Workbuddy `11001`、Codebuddy `10381`/`10522`、Openclaw `2106`、Anythingllm `14050`、KiroCli `9218`/`9447`、Kiro `7564` 同为「每条事件 +1」✅

⇒ §7.2 第 3 条的语义分类**成立**（「五家五样」为口语说法，实列 7 类，与源码一致）。

**§2 分类勘误与 source 命名抽查：**

- `src/lib/rollout.js:7567` / `9210` / `9450` → 三处均为 `getHourlyBucket(hourlyState, "kiro", ...)` ✅（`kiro` 由 Kiro IDE 与 Kiro CLI 共用，含会话支路 9450，分片未列 9450 以外的会话支路细节）
- `src/lib/rollout.js:15651` → `const bucket = getHourlyBucket(hourlyState, "kilo-code", model, bucketStart);` ✅ 带连字符
- `src/lib/rollout.js:11730` → `function normalizeKilocodeProviderToModel(providerName) {` ✅
- `src/lib/rollout.js:11706–11707` → Kilocode roots 解析（`resolveKilocodeRoots` 调用点 11707；分片引 11706，±1）✅
- `src/lib/rollout.js:11004` → `const bucket = getHourlyBucket(hourlyState, variant, model, bucketStart);` ✅ variant 当 source
- `src/lib/rollout.js:10651–10654` → `WORKBUDDY_VARIANTS` 两 variant ✅ 与分片引文一致
- `src/lib/rollout.js:9611` / `9844` → Kimi / KimiCode 的 `"kimi"` 入桶点（`getHourlyBucket`）✅
- 7 家 SQLite（Kiro/Hermes/KiroCli/Zed/Unsloth/Anythingllm/Goose）逐一在 `rollout.js` 内确有 SQLite 读取（`readSqliteJsonRows` / `PRAGMA table_info` / `readKiroDbTokens` 等）✅

**其余正文行号抽样（通过）：**

- `src/lib/rollout.js:55/56` → `const DEFAULT_SOURCE = "codex";` / `const DEFAULT_MODEL = "unknown";` ✅
- `src/lib/rollout.js:3328` / `3327` / `3341` / `3342` → 队列行 JSON、`if (bucket.queuedKey === key) continue;`、`usage_precision`、`...prioritySubsetOf(totals)` ✅
- `src/lib/rollout.js:4254–4258` → `normalizeSourceInput` ✅ 逐字一致
- `src/lib/rollout.js:2094–2107` / `2109` / `2055` / `2141` → Openclaw 字段映射与闸门/预筛/项目注释全部 ✅
- `src/lib/rollout.js:7718` → Hermes SQL（含 `message_count` 与五列 `> 0` 条件）✅ 逐字一致
- `src/lib/rollout.js:8006–8015` → Hermes 落桶（`"hermes-agent"` 兜底、`conversation_count: dMessageCount`）✅
- `src/lib/rollout.js:10907–10909` / `10968–10977` / `10987–10991` → Workbuddy providerData/rawUsage、五桶闸门、模型链 ✅
- `src/lib/rollout.js:11910–11919` → Roocode 零值占位「不入 seen」注释 ✅ 逐字一致
- `src/lib/rollout.js:12381` / `12389` / `12403` / `12405` → Cline 三重闸门与差分 ✅
- `src/lib/rollout.js:12594` / `12598` / `12752` / `12765–12768` → Zed 模型、请求级优先、`totalDelta <= 0`、时间戳链 ✅
- `src/lib/rollout.js:12994` / `13040` / `13045–13048` → `normalizeLocalStudioTokens` ✅（见 I4）
- `src/lib/rollout.js:13634–13636` → `0 AS cached_tokens, 0 AS cache_write_tokens, 0 AS reasoning_tokens` ✅（分片写「13620 附近」）
- `src/lib/rollout.js:13874–13887` → Anythingllm SELECT 本体 ✅（见 I7）
- `src/lib/rollout.js:14016` / `14040` / `14049` / `14053` → Anythingllm 闸门/模型/落桶 ✅
- `src/lib/rollout.js:14670` / `14697` / `14708–14711` / `14726` → Goose ✅
- `src/lib/rollout.js:14972–14979` → `applyDroidTotalFallback` ✅ 逐字一致
- `src/lib/rollout.js:15378` / `15379` / `15381` → Droid 落桶与 source ✅
- `src/lib/rollout.js:15624–15634` / `15646` → Kilocode 字段与闸门 ✅
- `src/commands/init.js:568` / `577` / `623` ✅
- `src/commands/sync.js:1272` / `1224` / `1235` / `1440` / `1339` / `1110` / `2679` / `2032` / `2110` ✅（1234/1109/2031/2109 均为 ±1）

---

## 给主线的最短处置建议

1. **必改 4 处数字**：`7294→7298`、`10331→10337`、`10339→10345`、`12190→12178`、`13681→13691`、`12732→12742`、`9938→9940`、`1307→1284`。
2. **必改 2 处结论**：§3.6/§0/§6 的「KiroCli 全部字符估算」；§5 把 KiroCli 从 E 类移入 B 类。
3. **可结案 3 条**：§5 F（Codebuddy → D 类）、§7.1 第 2/3/5/8/9 条。
4. **加一句限定**：§5 E 类的「结构上不可能」= 「不可能出现六列全零 + `total>0`」，不等于「不可能出现 `input/output=0`」。
