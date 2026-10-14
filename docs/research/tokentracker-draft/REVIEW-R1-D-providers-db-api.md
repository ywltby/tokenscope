# 复核报告 R1-D：`S3b-providers-db-api.md`

- 被复核对象：`docs/research/tokentracker-draft/S3b-providers-db-api.md`（2943 行）
- 样本：`C:\Users\admin\AppData\Local\Temp\tt-research`（HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，v1.3.14，只读）
- 行号口径：全部用 `read`/`File::ReadAllLines(UTF8)`（见 CHECKLIST §0）；`rollout.js` 权威总行数 **24932**，`sync.js` **7251**，`trae-cn-config.js` **548**，`trae-config` 系列见各节
- 本轮只做证伪，不做润色

## 结论

**需修订**。骨架可用、行号命中率约 94%（多数代码块引用是逐字精确的），但有 **9 处必须修的硬错误**，其中 3 处会直接误导下游实现（联网家数、SQLite 引擎结论、`trae` 的 billable 覆盖漏项）。

## 抽样统计

- 抽验断言数：**188**（其中**代码块引用行号 62 条**，逐字比对）
- 结论分布：**E=9，I=6，U=3**（其余 170 条与源码一致）
- 实际打开过的源码文件：
  `src/lib/rollout.js`（含 24932 行全文件行号索引）、`src/lib/sqlite-reader.js`（全文 215 行）、
  `src/lib/source-metadata.js`（全文 48 行）、`src/lib/devin-usage.js`（全文 212 行）、
  `src/lib/trae-db.js`（全文 127 行）、`src/lib/trae-snapshot.js`、`src/lib/trae-sqlite.js`（全文 143 行）、
  `src/lib/cursor-config.js`、`src/lib/trae-cn-config.js`、`src/lib/antigravity-paths.js`、
  `src/lib/grok-usage.js`（全文 95 行）、`src/lib/grok-hook.js`、`src/commands/sync.js`、
  `src/commands/status.js`、`src/lib/usage-limits.js`、`package.json`、`package-lock.json`、`deno.lock`

---

## B6 专项：23（实为 21）家的数据入口独立核对

**扫描方法**：`git grep -n -I -e 'fetch' -e 'undici' -e 'node:https' -e 'WebSocket' -- src/lib/rollout.js` 返回**零个调用点**（只有注释里的 "fetch" 一词）。全部 21 个解析器实体都在 `rollout.js` 内，**没有一家自己发起网络请求**；联网发生在 `src/commands/sync.js` 的调用点。

| 家（source） | 入口类型 | 源码行号（本次亲自打开） |
| --- | --- | --- |
| `opencode` | 本地 SQLite `opencode.db` | `sync.js:1369`、`1375-1381`；`rollout.js:5143-5147` |
| `qoder`（旧 DB） | 本地 SQLite `local.db` | `rollout.js:6030-6050`、`6064-6077`、`6129-6135` |
| `qoder`（新 JSONL） | 本地 `~/.qoder/projects/**/*.jsonl` | `rollout.js:6416-6419`、`6505-6511` |
| `qoder-cn` | 同上（CN 路径/游标隔离） | `rollout.js:6112-6127` |
| `cursor` | **远程 HTTP API + SQLite 取凭据** | `sync.js:2157-2187`；`cursor-config.js:46-47`、`150-152` |
| `devin` | 本地 SQLite `sessions.db` | `rollout.js:14106-14134`、`147-156`（devin-usage.js） |
| `omp` | 本地 JSONL | `rollout.js:11352-11410` |
| `omo` | 本地 JSONL | `rollout.js:11582-11619` |
| `pi` / `pi-<provider>` | 本地 JSONL | `rollout.js:16171-16223`（**分片此处行号错，见 E5**） |
| `prime-agent` | 本地 JSONL | `rollout.js:16235-16275` |
| `minimax-code` | 本地 JSONL | `rollout.js:16727-16745` |
| `craft` | 本地 JSONL（只读首行）+ `config.json` | `rollout.js:16812-16824`、`17123-17135` |
| `reasonix` | 本地遥测旁车 JSON | `rollout.js:16907-16915`、`16925-16934` |
| `copilot`（OTEL） | 本地 JSONL（**读侧完全离线**） | `rollout.js:17309-17341` |
| `copilot`（session-store） | 本地 SQLite | `rollout.js:18131-18155`、`18681` |
| `copilot`（app data.db） | 本地 SQLite | `rollout.js:19357-19379` |
| `grok` | 本地 JSONL + JSON 旁车（hook 无网络：`grok-hook.js` 全文无 fetch） | `rollout.js:19566-19605` |
| `antigravity` | 本地 JSONL + 本地 SQLite（无远程） | `rollout.js:20559-20566`、`21573-21581` |
| `trae` | 本地加密 SQLite（自解密） | `trae-db.js:15-45`、`102-125` |
| `trae-cn` | **远程 HTTP API + 读凭据 + opt-in** | `sync.js:2234-2262`；`trae-cn-config.js:27-33`、`37-45` |
| `dsh` | 本地 JSONL（含 zstd 帧） | `rollout.js:23066-23072`、`23347-23366` |
| `command-code` | 本地 JSONL | `rollout.js:24191-24216`、`24572-24576` |

**结论**：
1. 「只有 `cursor` 与 `trae-cn` 必须联网/读凭据」**成立**（独立验证）。
2. `Copilot` 的 OTEL **不需要网络** ✓（分片正确）；`Grok` 纯本地 ✓；`Antigravity` 纯本地 ✓。
3. 但分片 §0 结论 1 写「**4 家**必须联网且必须读取本机登录凭据……其余 **19 家**」是本分片最严重的表述错误（见 E1）。
4. **边界情形（分片未披露，属其它分片范围）**：`sync.js:4363` 存在 `fetchFunctionResponse(functionUrlFor(root, INGEST_SLUG), ...)` 云端上传调用；`trae` 的 `readTraeEntitlementFromStorage`（`rollout.js:22269`）读本地 `storage.json` 权益快照，调用点仅在 `status.js:840`（展示），不进解析路径 ✓。

---

## 错误（E）—— 必须修

### [E1] §0 结论 1：联网家数自相矛盾，且分母与 §20 表不符

- 断言（分片第 10 行）：「23 家中有 **4 家必须联网且必须读取本机登录凭据**才能产出任何数据：`cursor`（…）、`trae-cn`（…）。其余 **19 家**全部是纯本地文件/本地库读取」——只列出 **2 家**。
- 实际：`§20.1` 表 19 行 + `§20.2` 表 2 行 = **21 家**；本轮独立扫描确认只有 2 家（`cursor`、`trae-cn`）在同步路径联网。23 − 4 = 19 与 23 − 2 = 21 不可能同时成立。
- 修正建议：改为「**2 家必须联网且必须读凭据**（`cursor`、`trae-cn`）；其余 **19 家**离线可用」；并把「23 家」的计数口径写清（见 I2）。

### [E2] §1.3：`sql.js` 依赖断言为假（devDependencies 中真实存在）

- 断言（分片第 109 行）：「`package.json` 的 `dependencies` 只有 `@journeyapps/wa-sqlite`、`@mongodb-js/zstd`、`smol-toml`、`undici`、`yauzl`…因此**仓库不依赖 `sql.js`、`better-sqlite3`**，不存在 JS 内嵌引擎路径」。
- 实际：
  - `package.json:67` → `"sql.js": "1.14.2"`（devDependencies；`package-lock.json:31/1654`、`deno.lock:460` 同）；
  - `package.json:61` → `"@electric-sql/pglite": "^0.5.8"`（另一个内嵌引擎，分片完全未提）；
  - 5 个测试文件 `require("sql.js")`：`test/trae-db.test.js:7`、`test/trae-snapshot.test.js:6`、`test/trae-sqlite.test.js:3`、`test/sync-trae.test.js:9`、`test/trim-embedded-deps.test.js:8`；`test/trim-embedded-deps.test.js:21` 还用 wa-sqlite 跑 TRAE reader。
- 说明：断言的前半句（**`src/**` 中 `sql.js`/`better-sqlite3` 命中为 0**）经 `git grep` 证实**为真**——生产路径确实只有 CLI/`node:sqlite` 两级。
- 修正建议：限定为「**生产代码 `src/**` 不使用** `sql.js`/`better-sqlite3`；`sql.js` 是 devDependency，仅用于构造 Trae SQLCipher fixture；仓库另有 `@electric-sql/pglite` devDependency，同样不进生产路径」。

### [E3] §0 结论 5 / §22：「`craft` 会产生 token 全零行」为假

- 断言（分片第 14 行）：「真正会出现「`conversation_count=1` 且全部 token 列为 0」的行来自 `qoder` 新 JSONL 的 billable 零 token 分支（`rollout.js:6650-6660`）与 **`craft` 的 `prev.total===0` 首见标记（`rollout.js:17243`）**」。
- 实际：`rollout.js:17199-17212` 有四分量全零护栏——`if (dInput === 0 && dOutput === 0 && dCacheRead === 0 && dCacheWrite === 0) { … continue; }`。走到 `17243` 的 `conversation_count: prev.total === 0 ? 1 : 0` 时**至少一个 token 分量 > 0**，因此 craft **不可能**产出「全部 token 列为 0」的行。同一分片 `§22.3` 表 craft 行（`rollout.js:17199-17211` 「四个 delta 全 0 → 只更新快照」）与该结论**自相矛盾**。
- 修正建议：删掉 craft 那一半，只保留 `qoder` 新 JSONL；另见 E6（`trae-cn` 才是被漏掉的第二家）。

### [E4] §17.1：`trae-cn` 落桶行号错（`22680` 是抛错处，不是落桶处）

- 断言（分片第 2091 行）：「落在桶键 `rollout.js:22680` 附近与 account_session_state 记录 `22580`」。
- 实际：`rollout.js:22680` = `throw new Error(\`Trae CN snapshot is not authoritative: ${skippedRows} malformed row…\`)`（fail-closed 抛错）。**真实落桶点**：
  - `rollout.js:22738-22746`（旧贡献撤回 `subtractTotals`）
  - `rollout.js:22749-22751`（`getHourlyBucket(hourlyState, TRAE_CN_SOURCE, current.model, current.bucketStart)` + `addTotals`）
  - `rollout.js:22580` `source: TRAE_CN_SOURCE`（account_session_state）**正确** ✓
- 修正建议：落桶行号改为 `rollout.js:22749-22751`。

### [E5] §8.2：`pi` 路径发现行号落在 `craft` 函数体内

- 断言（分片第 916 行）：「pi：`<piHome>/agent/sessions/**/*.jsonl`（`rollout.js:17171-17223`）」。
- 实际：`17171-17223` 属 `parseCraftIncremental`（函数定义 `rollout.js:17080`，该区间是 craft 的 header 字段映射与时间戳兜底）。pi 的路径发现实际在 **`rollout.js:16171-16223`**（`resolvePiSessionFiles` 定义于 `16171`，`~/.multica/pi-sessions` 分支在 `16200-16220`，与分片同段的第二半句一致）。
- 修正建议：改为 `rollout.js:16171-16223`。

### [E6] §22.1：「唯一一处」全零 token + `conversation_count=1` 为假（`trae-cn` 同样能产生）

- 断言（分片第 2749 行）：「这是**唯一一处**「全零 token + conversation_count=1」被显式构造并写进桶的地方」。
- 实际：`trae-cn` 的写入路径**没有任何零值护栏**：
  - `rollout.js:22477-22487` 只校验 `Number.isSafeInteger(value) && value >= 0`（**0 合法**）；
  - `rollout.js:22492` `const total = rawInput + outputTokens;`（0 + 0 = 0 合法）；
  - `rollout.js:22500-22508` 写入 `total_tokens: 0`、`billable_total_tokens: 0`、`conversation_count: 1`；
  - `rollout.js:22749-22751` 直接 `addTotals` 进桶（对比 `unchanged` 分支只跳过"未变化"的条目，不筛零值）。
- 说明：**是否真会从真实账号拿到 `input_token=0/output_token=0` 的行**需要真实数据（见 U1）；但源码层"允许"是确定的，故「唯一」不成立。
- 修正建议：改为「两处：`qoder` 新 JSONL 的 billable 零 token 分支 + `trae-cn` 的零值行（后者无护栏）」；并把 `trae-cn` 补进 §22.3 表。

### [E7] §23：「不写 `billable_total_tokens` 的 11 家」漏 `trae`，且项数对不上

- 断言（分片第 2801 行）：「**不写、依赖 `addTotals` 兜底的 11 家**：`opencode`、`omp`、`omo`、`pi`、`prime-agent`、`minimax-code`、`craft`、`reasonix`、`copilot`（OTEL / session-store / app-db 三条通道）、`dsh`。」
- 实际：该清单按「家」数是 **10 家**，按「项」数是 **12 项**，两种口径都不是 11；并且**漏掉 `trae`**（分片自己在 `§16.9` 写了「无 `billable_total_tokens` 写入（由 `addTotals` 兜底）」）。`git grep 'billable_total_tokens' -- src/lib/rollout.js` 在 trae 区段（21993-22176）**零命中**，证实不写。
- 正确口径（本次逐项核对）：**显式写 8 项**（`qoder` 旧 DB `6177`、`qoder` 新 `6540`/`6657`、`cursor` `cursor-config.js:536`、`devin` `devin-usage.js:197`、`grok` `grok-usage.js:83` + `rollout.js:19959`、`antigravity` `21806`/`21831`、`trae-cn` `22507`、`command-code` `24212`）；**不写 13 项**（上述 12 项 + `trae`）。
- 修正建议：改为「不写 13 项（含 `trae`）」，或统一声明计数口径。

### [E8] §26.2 / §21.5：`usage_precision` 引用数夸大，清单不全

- 断言（分片第 2889 行）：「已核对 `rollout.js` 内 **25 处** `usage_precision` 引用」；`§21.5`（第 2692 行）给的行号清单：`rollout.js:3325/3341/3403/3422/6542/6648/20286/22011/22025/22157`。
- 实际（逐行扫描 `rollout.js`，「引用」= 含该词的行）：**16 行 / 14 个语句** —
  `3325, 3341, 3403, 3422, 6542, 6648, 6725, 6731, 6755, 6760, 20286, 20287(续行), 22011, 22025, 22113, 22157`。
  分片清单**漏了 5 处**：`6725`、`6731`、`6755`、`6760`（qoder 新 JSONL 的桶级 `bucket.usage_precision = cur.totals.usage_precision`）与 `22113`（trae 的 `estimatedRecords` 计数）。
- 影响：`§21.5` 的**否定性结论仍成立**——补上这 5 处后清单里依然没有 `antigravity` 的写入点（`21898` 只是游标里的 `usageSource`）✓。所以这是"结论对、支撑数据错"。
- 修正建议：数字改 16（或"14 个语句"），清单补全 5 处。

### [E9] §0 结论 2：SQLite 家数（8）与 §20.4 表（7）冲突

- 断言（分片第 11 行）：「真正走 SQLite 的有 **8 家**」，随后列出 7 个名字（`opencode`、`qoder`、`devin`、`copilot`、`antigravity`、`trae`、`cursor`）并注 `copilot` 有两份库。
- 实际：`§20.4` 表把 SQLite 明确计入 **7 项**（`copilot` 两份拆开）+ 单列「SQLite 只用于取凭据 1 | `cursor`」。两处对同一事实给出 8 / 7+1 两套口径，且 `cursor` 的用量本身来自 CSV 而非 SQLite。
- 修正建议：统一为「SQLite 数据源 **7 项**（`copilot` 拆两份）；`cursor` 另有 SQLite 仅用于取凭据，不计入数据源类型」。

---

## 不完整（I）—— 建议补

### [I1] §21.1 漏登记 `rollout.js:6726` 的悬空注释（credits 风险的直接证据）

`rollout.js:6726` 原文：`// addTotals handles total_cost_usd via USD_TICKS, but credits cost is small; ensure it accumulates`——其下一行（`6727`）是 `touchedBuckets.add(...)`，**上下文里没有任何 credits 累积语句**。`§21.1` 的结论（credits 不换算、不入 total）正确，但这条"说要累积、实现没累积"的残留注释本身就是该家 credits 风险的证据，分片未提。建议在 §21.1 增列并标注「注释与实现不一致，需下游按 totals 白名单消费」。

### [I2] 「23 家」计数口径未定义

实测 `rollout.js` 内被本文覆盖的解析器实体是 **21 个**（`parseOpencodeDb…` … `parseCommandCode…`，逐个数过：`5761/6218/6552/7172/14207/16087/16100/16319 系/16235 系/16775/17080/17050/17878/18579/19381/20084/20800/22033/22598/23678/24458`）。要凑到 23 只能把 `parseOmpLikeIncremental`（15753）与 `parsePiLikeIncremental`（16319）两个**共享实现**也计为"解析器"。而 `§20.1`（19 行）+ `§20.2`（2 行）表用的是"家"。建议统一为「**21 家 / 23 个解析器项（含 2 个共享实现）**」。

### [I3] §1.3「不存在 JS 内嵌引擎路径」需限定作用域

同 E2。另建议补一句：`src/lib/trae-sqlite.js:7-11` 的 `wa-sqlite` 是 WASM 引擎（非 JS 内嵌），生产路径确实只有这一处"非 CLI/非 `node:sqlite`"的例外，分片已在 §20.3 声明 Trae 例外，但 §1.3 的措辞（"不存在"）与之相抵。

### [I4] §20.2 的 Trae 权益函数行号指向了另一个函数

分片第 2570 行写「`readTraeEntitlementFromStorage`（`rollout.js:22236-22259`）读的是本地 `storage.json` 的权益快照」。
实际：`22236-22260` 是 `normalizeTraeEntitlement`；`readTraeEntitlementFromStorage` 定义于 **`rollout.js:22269`**，唯一非导出调用点在 **`src/commands/status.js:840`**（状态展示），不进用量解析路径。
结论（本地、不联网、不影响产出）成立 ✓，行号需改。

### [I5] §10.8 两处行号尾部偏差（±1）

- `diffReasonixTotals` 标 `rollout.js:17008-17013` → 实际闭括号在 **17014**。
- 「差值全 0 且 `requests` 未增则整文件跳过」标 `rollout.js:17067-17069` → 判定条件实际是 **17067-17068**（17069 已是快照赋值）。
（同类 ±1 偏差还有 §7.6 的 `11612-11616`（实际 11612-11615）。均不影响语义。）

### [I6] §22.3 判定表未覆盖 `trae` 与 `trae-cn` 的零值形态

`§22.3` 表 18 行覆盖 21 家（`omp/omo` 合行、`pi` 家族合行、`copilot` 三通道分行），但 `trae` 行的条件（`rollout.js:22112` `if (!previous && totals.total_tokens === 0) continue;`）只保护**首次**观测，`trae-cn` 行（`22477-22487`）只保护"缺失"。建议在表中补一列「值为 0 是否合法」，把 E6 的事实显式化。

---

## 无法验证（U）

### [U1] `trae-cn` 真实响应中是否出现 `input_token=0` 且 `output_token=0` 的行

源码层"允许"已确认（E6）。要判定"是否真会发生"需要真实账号的一次 `query_user_usage_group_by_session` 响应样本，或抓取 fixture。缺什么：真实响应体（含全部 token 字段为 0 的会话行）。

### [U2] §26.10「三通道互斥完备」的测试覆盖

`test/` 下存在 `copilot-app-parser.test.js` 与 `copilot-session-store-parser.test.js`，两者均命中 `usageClaims`/`observeOnly` 关键字（`git grep -ln`）。但"三通道同时活跃时互斥完备"是否被断言，需要逐用例阅读（本次未做）。缺什么：交叉通道用例的断言清单。

### [U3] §26.1 保留项：`geminiPaths` 能否被环境变量重定向 Antigravity 扫描

分片已自认为未闭合。本轮只确认 `antigravity-paths.js:24-30` 的 `TOKENTRACKER_ANTIGRAVITY_HOME` 只作用于技能目录（✓ 与分片一致）；`sync.js:1281-1295` 的 `geminiPaths` 解析函数未逐行核对。缺什么：`resolveGeminiHome`/`resolveGeminiPaths` 的行级核对。

---

## 行号漂移统计

- 抽样中行号**正确的比例：约 170/188 = 90.4%**（严格计入 ±1 尾部偏差时为 94%）
- CHECKLIST §0 警告的"GBK 解码导致累计 15 行偏移"**未发生**：本分片所有行号与权威 UTF-8 口径一致，没有整体漂移。发现的问题全部是**局部指向错误**（指向了相邻函数/别的 provider 区段）：

| 分片位置 | 分片行号 | 实际应为 | 类型 |
| --- | --- | --- | --- |
| §17.1 | `rollout.js:22680` 落桶 | `22749-22751` | 指向抛错语句 |
| §8.2 | `rollout.js:17171-17223`（pi 路径） | `16171-16223` | 跨函数串行（落在 craft 内） |
| §20.2 | `rollout.js:22236-22259`（readTraeEntitlementFromStorage） | `22269` | 指向相邻函数 |
| §10.8 | `17008-17013` | `17008-17014` | ±1 尾部 |
| §10.8 | `17067-17069` | `17067-17068` | ±1 尾部 |
| §7.6 | `11612-11616` | `11612-11615` | ±1 尾部 |
| §26.2 | "25 处 usage_precision" | 16 行 / 14 语句 | 计数夸大 |

---

## 复核到的行号抽样（证明真的打开过；摘录逐字）

**队列与聚合基础**
- `src/lib/rollout.js:55-57` → `const DEFAULT_SOURCE = "codex";` / `const DEFAULT_MODEL = "unknown";` / `const BUCKET_SEPARATOR = "|";`
- `src/lib/rollout.js:4053-4065` → `function initTotals() {`（九字段，含 `billable_total_tokens: 0,`）
- `src/lib/rollout.js:4067-4075` → `PRIORITY_SUBSET_FIELDS` 定义（注释「NEVER part of any sum」）
- `src/lib/rollout.js:4099-4115` → `function addTotals(target, delta) {`（`4110: target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0;`）
- `src/lib/rollout.js:4173-4190` → `function toUtcHalfHourStart(ts) {`
- `src/lib/rollout.js:4222-4226` → `function bucketKey(source, model, hourStart) {`
- `src/lib/rollout.js:4238-4252` → `function parseBucketKey(key) {`
- `src/lib/rollout.js:4254-4258` → `function normalizeSourceInput(value) {`
- `src/lib/rollout.js:4998-5003` → `function coerceEpochMs(v) {` / `if (n < 1e12) return Math.floor(n * 1000);`

**SQLite 读取层**
- `src/lib/sqlite-reader.js:33` → `function readSqliteRowsWithCli(dbPath, sql, { execFileSync, timeout, maxBuffer, readOnly }) {`
- `src/lib/sqlite-reader.js:34` → `const args = readOnly ? ["-readonly", "-json", dbPath, sql] : ["-json", dbPath, sql];`
- `src/lib/sqlite-reader.js:67-79` → `function readSqliteRowsWithNode(dbPath, sql, { requireFn }) {`（`72: const db = new DatabaseSync(dbPath, { readOnly: true });`）
- `src/lib/sqlite-reader.js:81-128` → `function readSqliteJsonRows(...)`（`91-92` 默认 `30_000` / `50 * 1024 * 1024`）
- `src/lib/sqlite-reader.js:148-195` → `async function readSqliteJsonRowsAsync(...)`（`158-159` 同默认值）
- `src/lib/sqlite-reader.js:17-31` → `warnSqliteUnavailable`（`warnedSqliteReadFailures` 进程级 Set 去重）
- `src/lib/source-metadata.js:1` → `const ACCOUNT_LEVEL_SOURCES = new Set(["cursor", "trae-cn"]);`

**OpenCode / Qoder**
- `src/lib/rollout.js:4755-4772` → `function normalizeOpencodeTokens(tokens) {`
- `src/lib/rollout.js:4819-4836` → `function diffOpencodeTotals(current, previous) {`（带符号差值）
- `src/lib/rollout.js:5041-5045` → 四种库形态注释（A pure v1 / B beta-17887 / C upstream v2 / D transitional）
- `src/lib/rollout.js:5088-5095` → `function opencodeDbRowAnchor(row) {`（sha256 锚）
- `src/lib/rollout.js:5108-5113` → `function buildV1Sql(watermark = null) {`
- `src/lib/rollout.js:5143-5147` → 布局探测 SQL（`hasRows` / `sessionTable`）
- `src/lib/rollout.js:5892-5903` → `const timestampMs = coerceEpochMs(msg?.time?.completed) || coerceEpochMs(msg?.time?.created);`
- `src/lib/rollout.js:5969-5971` → `const contribution = moved ? { ...currentTotals, conversation_count: 1 } : { ...delta, conversation_count: effectiveLastTotals ? 0 : 1 };`
- `src/lib/rollout.js:6030-6050` → `const QODER_USAGE_SQL = `（三表 JOIN 原文）
- `src/lib/rollout.js:6064-6077` → Qoder 平台默认路径 → `SharedClientCache/cache/db/local.db`
- `src/lib/rollout.js:6112-6127` → CN 版隔离注释 + `resolveQoderCnDbPaths`
- `src/lib/rollout.js:6148-6178` → `function normalizeQoderTokens(tokenInfo) {`（`6177: billable_total_tokens: Math.trunc(prompt) + output,`）
- `src/lib/rollout.js:6401-6414` → Qoder 新版注释（`qoder vs qoderNew`、`row: vs jsonl:`）
- `src/lib/rollout.js:6513-6549` → `function normalizeQoderNewTokens(usage) {`（`6541: credits: Number.isFinite(credits) && credits > 0 ? credits : 0,`）
- `src/lib/rollout.js:6629-6660` → billable 零 token 分支（`6657: billable_total_tokens: 0,` / `6659: conversation_count: 1,`）
- `src/lib/rollout.js:6726` → `// addTotals handles total_cost_usd via USD_TICKS, but credits cost is small; ensure it accumulates`（**其下无实现**）

**Cursor / Devin**
- `src/lib/cursor-config.js:46-47` → `const CURSOR_ACCESS_TOKEN_SQL = "SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken';"`
- `src/lib/cursor-config.js:109-111` → `const cookie = \`WorkosCursorSessionToken=${userId}%3A%3A${jwt}\`;`
- `src/lib/cursor-config.js:150-152` → `CURSOR_CSV_URL` / `CURSOR_SUMMARY_URL` / `CURSOR_RPC_ORIGIN`
- `src/lib/cursor-config.js:512-537` → `function normalizeCursorUsage(record) {`（`536: billable_total_tokens: totalTokens,`）
- `src/lib/rollout.js:7225-7240` → `if (isAllZeroUsage(delta)) continue;`（`7230`）/ `7232: delta.conversation_count = 1;`
- `src/lib/devin-usage.js:25-26` → `DEVIN_TABLE_PROBE_SQL`
- `src/lib/devin-usage.js:76-87` → `requiredCount` / `optionalCount`
- `src/lib/devin-usage.js:113-126` → 缺 NULL 即整行不计 + 「node insertion time is never a fallback」
- `src/lib/devin-usage.js:149-156` → `function compareCandidates(a, b) {`
- `src/lib/devin-usage.js:183-200` → `total` 与 totals 块（`197: billable_total_tokens: total,`）
- `src/lib/rollout.js:14128-14134` → `function devinSqliteFingerprint(dbPath) {`（`14132: delete fingerprint["-shm"];`）
- `src/lib/rollout.js:14231-14234` → 指纹未变即整段跳过
- `src/lib/rollout.js:19297-19314` → `function sqliteSidecarFingerprint(dbPath) {`

**OMP / pi 家族 / Craft / Reasonix**
- `src/lib/rollout.js:11352-11410` → `resolveOmpHome` / `resolveOmpAgentDir` / `collectPiStyleSessionFiles`
- `src/lib/rollout.js:11405-11407` → `//   <agentDir>/sessions/--<cwd-encoded>--/<timestamp>_<sessionId>.jsonl`
- `src/lib/rollout.js:11582-11615` → `resolveOmoHome` / `resolveOmoAgentDir` / `resolveOmoDefaultModel`
- `src/lib/rollout.js:15868-15915` → omp/omo 字段映射、全零护栏、OmO 求和排除 reasoning、delta
- `src/lib/rollout.js:16449-16464` → pi 家族字段映射与全零护栏
- `src/lib/rollout.js:16812-16824` → Craft 磁盘布局与其后语义注释
- `src/lib/rollout.js:16907-16915` → `// Reasonix persists content-free cumulative usage beside each session JSONL.`
- `src/lib/rollout.js:16925-16934` → `%APPDATA%\reasonix` 候选分支
- `src/lib/rollout.js:16989-17005` → `function normalizeReasonixTotals(usage) {`
- `src/lib/rollout.js:17037-17045` → `addTotals(bucket.totals, { … conversation_count: delta.requests, })`
- `src/lib/rollout.js:17044` → `conversation_count: delta.requests,`
- `src/lib/rollout.js:17123-17135` → Craft 只读首行、`end: 1024 * 1024 - 1`
- `src/lib/rollout.js:17199-17212` → Craft 四分量全零护栏（**E3 的关键证据**）
- `src/lib/rollout.js:17236-17244` → Craft delta（`17243: conversation_count: prev.total === 0 ? 1 : 0,`）
- `src/lib/rollout.js:17251-17258` / `17271-17280` → Craft 快照与 5k 上限淘汰

**Copilot 三通道**
- `src/lib/rollout.js:17309-17341` → `resolveCopilotOtelPaths`（`17330-17333` WSL 探测）
- `src/lib/rollout.js:17375-17405` → `COPILOT_PARSER_VERSION = 3` / claims 7 天 & 1 万条
- `src/lib/rollout.js:17447-17460` → `function getCopilotDedupKey(record, attrs) {`
- `src/lib/rollout.js:17497-17542` → `extractCopilotOtelUsage`（`17521-17522` 拆 reasoning；`17536-17542` `totalInteresting === 0` → null）
- `src/lib/rollout.js:17575-17586` → OTEL totals（`17586: conversation_count: 1,`）
- `src/lib/rollout.js:17917-17951` → v1→v2 / v2→v3 迁移
- `src/lib/rollout.js:18045-18095` → OTEL 落桶、`recentOtelUsageEvents`、读后 re-stat、`seenIds` 上限
- `src/lib/rollout.js:18122-18125` / `18131-18155` / `18162` → store 注释、`resolveCopilotDbPaths`、覆盖变量
- `src/lib/rollout.js:18414-18447` → store 主查询、`id > lastId`、读参数
- `src/lib/rollout.js:18534-18576` → `normalizeCopilotSessionStoreUsage`（exact/fallback 两档）
- `src/lib/rollout.js:18681` → `snap = snapshotSqliteDb(resolvedDb);`
- `src/lib/rollout.js:18863-18890` → store 判空、delta、落桶、`conversation_count: seenSessions.has(sessionId) ? 0 : 1`
- `src/lib/rollout.js:19334-19355` → `capCopilotAppSessionTotals`（非零优先）
- `src/lib/rollout.js:19357-19379` → `normalizeCopilotAppTokenDelta`（`19377` 首见非零记 1）

**Grok / Antigravity**
- `src/lib/rollout.js:19549-19551` → `GROK_ESTIMATED_INPUT_RATIO = 0.8;` / `GROK_CURSOR_VERSION = 5;`
- `src/lib/rollout.js:19566-19605` → `resolveGrokBuildSessions`（signals.json / updates.jsonl）
- `src/lib/rollout.js:19675-19692` → `grokModelFromSignals` 兜底 `"grok-build"` / `grokLastActiveFromSignals` 兜底 `new Date().toISOString()`
- `src/lib/rollout.js:19945-19961` → `estimateGrokTokenDelta`（`19959: billable_total_tokens: total,`）
- `src/lib/rollout.js:19964-19968` → `mergeGrokUsagePrecision` → `"mixed"`
- `src/lib/rollout.js:20250-20268` → 估算桶起点三级兜底 + `usagePrecision: "estimated"`
- `src/lib/rollout.js:20283-20291` → grok 落桶与精度合流
- `src/lib/grok-usage.js:3-10` → camelCase/snake_case 语义差异注释
- `src/lib/grok-usage.js:12` → `const USD_TICKS_PER_USD = 10_000_000_000;`
- `src/lib/grok-usage.js:26-39` → `reportedCostUsd`
- `src/lib/grok-usage.js:71-83` → `if (totalTokens <= 0) return null;` / `83: billable_total_tokens: totalTokens,`
- `src/lib/rollout.js:20559-20566` → `resolveAntigravityBrainDirs`
- `src/lib/rollout.js:21573-21581` → antigravity UNC 快照
- `src/lib/rollout.js:21787-21833` → `billedPlanner` 两条路径（`21806`/`21831` billable）
- `src/lib/rollout.js:21847-21858` → 归属账本 key `${source}|${model}|${bucketStart}|${projectKey || ""}`
- `src/lib/rollout.js:21898` → `usageSource: hasSqliteMetadata ? "sqlite" : "estimated",`
- `src/lib/rollout.js:21971-21991` → `estimateAntigravityTokens` / `isCjkCodePoint`
- `src/lib/antigravity-paths.js:24-30` → `resolveAntigravitySkillDirs`（`TOKENTRACKER_ANTIGRAVITY_HOME` 只作用于 skills）

**Trae 三件套 / Trae CN**
- `src/lib/trae-db.js:8-13` → `TOKEN_FIELDS`（11 个候选列名）
- `src/lib/trae-db.js:15-45` → `resolveTraeDbPaths`
- `src/lib/trae-db.js:71-84` → 列投影 SQL（`json_type(context, '$.token_usage.…')`）
- `src/lib/trae-db.js:102-125` → 3 次重试 + `isStable()` + `// never publish a mixed snapshot`
- `src/lib/trae-snapshot.js:9-10` → `// TRAE's shared application key` / `const DEFAULT_SQLCIPHER_KEY = "3605f6691095a993f03d5009c918352ef5be31ae31e8f000212b81ff058da773";`（**硬编码确认**）
- `src/lib/trae-snapshot.js:38-45` → `pageDecryptor`（key 覆盖 + 64 hex 校验 + `pbkdf2Sync(key, salt, 2, 32, "sha512")`）
- `src/lib/trae-sqlite.js:7-11` → wa-sqlite 三处 import
- `src/lib/trae-sqlite.js:19-32` → `TraeVFS` / `jOpen` 只读
- `src/lib/trae-sqlite.js:74-76` → `jWrite/jTruncate/jDelete` 返回 `SQLITE_READONLY`
- `src/lib/trae-sqlite.js:116` → `PRAGMA query_only=ON; PRAGMA cache_size=-4096; PRAGMA temp_store=MEMORY;`
- `src/lib/trae-sqlite.js:135-141` → `withTraeSqlite` 单引擎串行化
- `src/lib/rollout.js:22002` → `const TRAE_CURSOR_VERSION = 2;`
- `src/lib/rollout.js:22004-22005` → 13 字段元组解构
- `src/lib/rollout.js:22049-22054` → 版本不匹配则 `clearSourceHourlyBuckets(hourlyState, "trae")`
- `src/lib/rollout.js:22062` → `const digest = (value) => crypto.createHash("sha256").update(value).digest().subarray(0, 16).toString("base64url");`
- `src/lib/rollout.js:22080-22084` → `devinSqliteFingerprint` 用于 trae 库指纹
- `src/lib/rollout.js:22104-22118` → `identity` 摘要、`T_STORE` 冲突裁决（`22111`）、`22112` 零值护栏
- `src/lib/rollout.js:22127-22132` → `getHourlyBucket(hourlyState, "trae", …)` 落桶
- `src/lib/rollout.js:22149-22158` → trae 桶级精度合流（`22157`）
- `src/lib/rollout.js:22162-22173` → 重建整轮回退 + `retractStaleSourceQueueRows(queuePath, "trae", keepKeys)`
- `src/lib/rollout.js:22269` → `function readTraeEntitlementFromStorage(storagePath) {`（**E/I4 证据**）
- `src/lib/rollout.js:22322-22324` → `TRAE_CN_SOURCE = "trae-cn"` / `TRAE_CN_STATE_VERSION = 1` / `TRAE_CN_UNKNOWN_MODEL`
- `src/lib/rollout.js:22300-22308` → `iCubeServerData://icube.cloudide` 权益读取
- `src/lib/rollout.js:22477-22492` → trae-cn 字段校验（`Number.isSafeInteger(value) && value >= 0`）/ `const total = rawInput + outputTokens;`
- `src/lib/rollout.js:22500-22508` → trae-cn totals（`22507: billable_total_tokens: total,`）
- `src/lib/rollout.js:22580` → `source: TRAE_CN_SOURCE,`（account_session_state）
- `src/lib/rollout.js:22626-22634` → 空响应=成功空操作注释
- `src/lib/rollout.js:22637-22660` → 全量预校验 + 畸形容错
- `src/lib/rollout.js:22680` → `throw new Error(\`Trae CN snapshot is not authoritative: …\`)`（**E4 证据**）
- `src/lib/rollout.js:22738-22751` → trae-cn 真实落桶点
- `src/lib/trae-cn-config.js:11-16` → tc v5 blob 格式注释
- `src/lib/trae-cn-config.js:27-33` → `TRAE_CN_USAGE_ENV` + `isTraeCnUsageEnabled`
- `src/lib/trae-cn-config.js:35` → `const TRAE_CN_AUTH_KEY = "iCubeAuthInfo://icube.cloudide";`
- `src/lib/trae-cn-config.js:37-49` → URL / `usage_type [7]` / 分页 / 超时 / 二分深度 8
- `src/commands/sync.js:2234-2250` → trae-cn 调用条件 + 30 天窗口对齐半小时
- `src/commands/status.js:840` → `? readTraeEntitlementFromStorage(traeStoragePath)`
- `src/commands/status.js:1215` → `usage_key_source: process.env.TOKENTRACKER_TRAE_SQLCIPHER_KEY?.trim() ? "environment" : "application",`
- `src/commands/status.js:1519-1526` → Copilot OTEL 提示文案

**DSH / Command Code**
- `src/lib/rollout.js:22818-22820` → `DSH_SESSION_LOG_MAX_BYTES` / `DSH_SESSION_TEXT_MAX_BYTES` / `DSH_SOURCE = "dsh"`
- `src/lib/rollout.js:23060-23072` → `decodeDshZstdFrame`（Node 内置优先，`@mongodb-js/zstd` 回落）
- `src/lib/rollout.js:23345-23366` → `dshUsageToTotals`（五分量全零 → null；`23364: conversation_count: 1,`）
- `src/lib/rollout.js:24055-24072` → Command Code 成本哨兵注释 / `COMMAND_CODE_SOURCE` / 版本 / 目录常量
- `src/lib/rollout.js:24187-24216` → `commandCodeUsageToTotals`（`24204` 判空 / `24212` billable / `24213` cost 0 / `24214` conversation）
- `src/lib/rollout.js:24569-24578` → `applyObservation`（`24572-24576` 落桶，含项目桶）

**sync.js 基础设施**
- `src/commands/sync.js:310-319` → `AUTO_SYNC_SOURCE_ALIASES`
- `src/commands/sync.js:320-366` → `AUTO_SYNC_SOURCES`
- `src/commands/sync.js:367-373` → `BACKGROUND_AUTO_SYNC_SOURCES`（`acode`、`codex`、`every-code`、`reasonix`）
- `src/commands/sync.js:1329` → `source: "antigravity",`
- `src/commands/sync.js:1348-1381` → opencode 路径解析与调用（`1369` db 路径）
- `src/commands/sync.js:1429` → `opencodeCursorNamespace: installKeys.length > 1 ? key : "flat",`
- `src/commands/sync.js:2157-2187` → cursor 抓取与调用（`2186: source: "cursor",`）
- `src/commands/sync.js:2212-2214` → TRAE `unpricedRecords` 披露

**其它**
- `src/lib/rollout.js:7693-7696` → UNC / Plan 9 桥锁注释
- `src/lib/rollout.js:5194-5199` / `7315-7320` / `14184-14189` / `21573-21581` → 各家 UNC 先快照
- `src/lib/rollout.js:12514-12535` → `decodeZedThreadBlob`（`zstd` 分流）
- `src/lib/usage-limits.js:1920-1922` → `describeCopilotOtelStatus` / `env.COPILOT_OTEL_ENABLED`
- `package.json:53-69` → dependencies 5 项 + devDependencies 含 `sql.js`、`@electric-sql/pglite`
- `test/trae-db.test.js:7` 等 5 处 → `const initSqlJs = require("sql.js");`
