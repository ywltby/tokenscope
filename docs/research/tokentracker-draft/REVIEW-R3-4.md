# 复核报告 R3-4：主文档 §10–§13 与附录 A/B

- 复核对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md`（4232 行）的 **§10（3534-3959）、§11（3961-3999）、§12（4003-4095）、§13（4098-4140）、附录 A（4144-4201）、附录 B（4203-4232）**
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14），**全程只读**，未修改样本仓库与主文档任何一个字节
- 行号口径：`[System.IO.File]::ReadAllLines($p, [Text.Encoding]::UTF8)` 与 `read` 工具；**未使用**裸 `Get-Content`。`rollout.js` 权威总行数 **24932**
- 职责：证伪。不润色、不补写、只报差异
- 独立重生成、独立重算（不看文档自报）：附录 A 全表、45/8/4 源集、46 个 `sourceAllowed` 字面量、`curated-overrides.json` 与 `seed-snapshot.json` 计数、edge↔curated 键差集、`upload-queue.js`/`usage-limits.js`/`edge ts` 等文件行数

---

## 结论

**附录 A 零差异；§10–§13 抽验 112 条命中 106 条（命中率 94.6%），必须修 6 条。**

1. **附录 A 45 行逐行一致（差异数 = 0）**。我用 `^(async\s+)?function\s+\w+` 重生成顶层函数表（**572 个**顶层函数，与文档自述一致），筛 `parse\w*Incremental` 得 **45 个**，函数名 / 起始行 / 覆盖行数（下一个顶层函数起始行 − 起始行）与文档表**逐行完全相同**，无 ±1 偏差。三点说明全部成立。`sync.js` 调用点抽查 **52/52 全部命中**（文档共列 52 个调用/开关位）。
2. **§11 常量表的取值全部为真**：我逐项复算 27 个常量/阈值，**取值零错误**（含 200/500/10、90 s、20 s、16 MB、272 000、DeepSeek `Date.UTC(2026,7,22,16,0,0)`、24 h TTL、10 万哈希、10、2000、45/8/4 等）。但**有一处位置引用指向不存在的文件**（E1）。
3. **§10–§13 的机制性描述基本可信**；「不建议照抄」清单 17 条中抽查 17 条，**全部有源码依据**（含 21.7 亿 token 事故的 2.17B 与分项自洽、6 处队列写点、13 个 `skipNoOpCursorCommit` 条件、44/9 键差集）。
4. **问题集中在自报计数与引用精度**，不是机制结论：1 条不存在路径（E1）、1 条被 R2 判不可复现却未处置的计数（E2）、1 条 §13 工作量与 R2-3 报告不一致（E3）、1 条行数算术（E4）、1 条计数不完整（E5）、1 条引用错行（E6）；另 4 条范围/措辞级（I1–I4）。

---

## A. 附录 A 穷举核对（差异表）

重生成口径：`[System.IO.File]::ReadAllLines(src/lib/rollout.js, UTF8)` → 总行 **24932**；`^(async\s+)?function\s+\w+` 顶层匹配 → **572** 个；`^parse\w*Incremental$` → **45** 个；覆盖行数 = 下一个顶层函数起始行 − 本函数起始行。

**差异数：0。45/45 函数名、起始行、覆盖行数全部一致。**

| # | 函数名 | 文档起始行 | 重算起始行 | 文档行数 | 重算行数 | 判定 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `parseRolloutIncremental` | 251 | 251 | 457 | 457 | ✅ |
| 2 | `parseClaudeIncremental` | 954 | 954 | 288 | 288 | ✅ |
| 3 | `parseGeminiIncremental` | 1242 | 1242 | 170 | 170 | ✅ |
| 4 | `parseOpencodeIncremental` | 1412 | 1412 | 170 | 170 | ✅ |
| 5 | `parseOpenclawIncremental` | 1654 | 1654 | 271 | 271 | ✅ |
| 6 | `parseOpencodeDbIncremental` | 5761 | 5761 | 291 | 291 | ✅ |
| 7 | `parseQoderDbIncremental` | 6218 | 6218 | 198 | 198 | ✅ |
| 8 | `parseQoderNewIncremental` | 6552 | 6552 | 300 | 300 | ✅ |
| 9 | `parseClaudeScienceIncremental` | 7051 | 7051 | 90 | 90 | ✅ |
| 10 | `parseCursorApiIncremental` | 7172 | 7172 | 108 | 108 | ✅ |
| 11 | `parseKiroIncremental` | 7472 | 7472 | 132 | 132 | ✅ |
| 12 | `parseHermesIncremental` | 7900 | 7900 | 180 | 180 | ✅ |
| 13 | `parseKiroCliIncremental` | 8886 | 8886 | 393 | 393 | ✅ |
| 14 | `parseKimiIncremental` | 9528 | 9528 | 141 | 141 | ✅ |
| 15 | `parseKimiCodeIncremental` | 9723 | 9723 | 187 | 187 | ✅ |
| 16 | `parseCodebuddyIncremental` | 10164 | 10164 | 492 | 492 | ✅ |
| 17 | `parseWorkbuddyIncremental` | 10803 | 10803 | 549 | 549 | ✅ |
| 18 | `parseRoocodeIncremental` | 11824 | 11824 | 200 | 200 | ✅ |
| 19 | `parseClineIncremental` | 12200 | 12200 | 290 | 290 | ✅ |
| 20 | `parseZedIncremental` | 12650 | 12650 | 209 | 209 | ✅ |
| 21 | `parseLmstudioIncremental` | 13267 | 13267 | 215 | 215 | ✅ |
| 22 | `parseUnslothIncremental` | 13691 | 13691 | 117 | 117 | ✅ |
| 23 | `parseAnythingllmIncremental` | 13938 | 13938 | 168 | 168 | ✅ |
| 24 | `parseDevinIncremental` | 14207 | 14207 | 282 | 282 | ✅ |
| 25 | `parseGooseIncremental` | 14604 | 14604 | 193 | 193 | ✅ |
| 26 | `parseDroidIncremental` | 15137 | 15137 | 408 | 408 | ✅ |
| 27 | `parseKilocodeIncremental` | 15545 | 15545 | 142 | 142 | ✅ |
| 28 | `parseOmpLikeIncremental` | 15753 | 15753 | 334 | 334 | ✅ |
| 29 | `parseOmpIncremental` | 16087 | 16087 | 13 | 13 | ✅ |
| 30 | `parseOmoIncremental` | 16100 | 16100 | 29 | 29 | ✅ |
| 31 | `parsePiLikeIncremental` | 16319 | 16319 | 370 | 370 | ✅ |
| 32 | `parsePiIncremental` | 16689 | 16689 | 10 | 10 | ✅ |
| 33 | `parsePrimeAgentIncremental` | 16699 | 16699 | 28 | 28 | ✅ |
| 34 | `parseMinimaxCodeIncremental` | 16775 | 16775 | 55 | 55 | ✅ |
| 35 | `parseReasonixIncremental` | 17050 | 17050 | 30 | 30 | ✅ |
| 36 | `parseCraftIncremental` | 17080 | 17080 | 229 | 229 | ✅ |
| 37 | `parseCopilotIncremental` | 17878 | 17878 | 253 | 253 | ✅ |
| 38 | `parseCopilotSessionStoreIncremental` | 18579 | 18579 | 456 | 456 | ✅ |
| 39 | `parseCopilotAppDbIncremental` | 19381 | 19381 | 172 | 172 | ✅ |
| 40 | `parseGrokBuildIncremental` | 20084 | 20084 | 475 | 475 | ✅ |
| 41 | `parseAntigravityIncremental` | 20800 | 20800 | 597 | 597 | ✅ |
| 42 | `parseTraeIncremental` | 22033 | 22033 | 148 | 148 | ✅ |
| 43 | `parseTraeCnApiIncremental` | 22598 | 22598 | 228 | 228 | ✅ |
| 44 | `parseDshIncremental` | 23678 | 23678 | 396 | 396 | ✅ |
| 45 | `parseCommandCodeIncremental` | 24458 | 24458 | 475 | 475 | ✅ |

### A2. `sync.js` 调用点抽查（52 个全部打开核对，远超 ≥15 要求）

判定标准：该行确实是该解析器的调用（`= await parseXIncremental({` / `parseXIncremental({`）或文档已声明的「source 开关」（`if (sourceAllowed("…"))`）。

| 文档调用点 | 实测行内容（截断） | 判定 |
| --- | --- | --- |
| 1 `:1046` | `const runCodexParse = (files) => parseRolloutIncremental({` | ✅（「经 runCodexParse」属实） |
| 2 `:5769` | `await parseRolloutIncremental({` | ✅ |
| 3 `:1201` | `claudeResult = await parseClaudeIncremental({` | ✅ |
| 4 `:1258` `:1361` `:1105` `:4117` | `geminiResult/parseOpencode/parseOpenclaw…= await parse…Incremental({` | ✅ 4/4 |
| 5 `:1375` `:1665` | `parseOpencodeDbIncremental({` | ✅ 2/2 |
| 6 `:1471` `:1549` `:1509` `:1599` | `parseQoderDb/parseQoderNew …Incremental({` | ✅ 4/4 |
| 7 `:1642` `:2173` `:2358` `:2516` `:2544` `:2594` `:5409` | `parseClaudeScience / parseCursorApi / parseHermes / parseKimi / parseKimiCode / parseCodebuddy ×2` | ✅ 7/7 |
| 8 `:2639` `:2674` `:5579` | `parseWorkbuddyIncremental({`（含 `workbuddy-ai` 分支） | ✅ 3/3 |
| 9 `:2091` `:2124` `:2074` `:1879` `:1897` `:1916` `:1935` `:2005` `:2024` `:6698` `:1960` | `roocode / cline / zed / lmstudio / unsloth / anythingllm / devin / goose / droid ×2 / kilocode` | ✅ 11/11 |
| 10 `:2708` `:2747` `:2780` `:2809` `:2837` `:2890` `:2865` | `omp / omo / pi / primeAgent / minimaxCode / reasonix / craft` | ✅ 7/7 |
| 11 `:3108` `:3053` `:3282` `:3216` | `parseCopilot / parseCopilotSessionStore ×2 / parseCopilotAppDb` | ✅ 4/4 |
| 12 `:2960` `:1315` `:2200` `:2280` `:1819` `:1855` | `grokBuild / antigravity / trae / traeCn / dsh / commandCode` | ✅ 6/6 |
| 13 `:2307` `:2429`（`parseKiro`/`parseKiroCli`） | `if (sourceAllowed("kiro")) {`（两处同形，与「kiro 两套解析器共享 source」自洽） | ✅ 2/2（文档已声明是「source 开关」） |

### A3. 附录 A 三点说明核对

- **① 薄壳与共享实现**：`parseOmpLike` 334 行 / `parseOmp` 13 行、`parsePiLike` 370 行 / `parsePi` 10 行，另有 `parseOmo` 29、`parsePrimeAgent` 28、`parseReasonix` 30 行 ✅ 与「5 个转发薄壳（13/10 行两处）+ 40 个自持」相符。
- **② `parseDumateIncremental` 不在本表**：`rollout.js` 内 `*Dumate*` **零命中** ✅；`sync.js:1696` `sourceAllowed("dumate")` ✅、`:1701` `parseDumateIncremental({` ✅；**真身在 `src/lib/dumate.js:32`**（`sync.js:11` 的 require 指向 `../lib/dumate`）。文档「定义不在 `rollout.js`，故 45 项不含它」的表述成立 ✅（建议补一句真身路径，非错误）。
- **③ `sourceAllowed` 字面量 46 个**：程序化枚举 `sourceAllowed\("([^"]+)"\)` → 调用 **61 次 / 去重 46 个**，与文档列出的 46 个字面量**逐字相同**（含 `dumate`、`mimo`、`zcode`、`kilo-cli`）✅。`AUTO_SYNC_SOURCES` 成员 **45 项 / 0 重复** ✅、别名表 **8 条**（`code→every-code`…`roo-code→roocode`）逐字一致 ✅、`BACKGROUND_AUTO_SYNC_SOURCES` **4 项** ✅。
- **自检结果**：`parseRolloutIncremental`=251 ✅、`parseClaudeIncremental`=954 ✅、`parseCommandCodeIncremental`=24458（文件 24932）✅ 全部复核命中。

---

## B. 抽样与证伪（§10 / §11 / §12 / §13 分节）

统计口径：一条 = 一个可证伪的行号引用 / 取值 / 计数 / 范围断言。**合计抽验 112 条：§10 42、§11 27、§12 36、§13 7；命中 106、必须修 6、建议收窄 4。**

### B.1 §10（42 条，命中 40，必须修 2）

| # | 抽样点 | 文档断言 | 实测 | 判定 |
| --- | --- | --- | --- | --- |
| 1 | 10.1① `fs.js:69-74` | `LOCK_STALE_MS=5min`、`LOCK_HEARTBEAT_MS=30s`、`MAX_RECLAIM_DEPTH=4`、`MAX_RECLAIM_SWEEP_DEPTH=64`（+`TRANSITION_GUARD_RETRY_MS=5`/`ATTEMPTS=1000`） | `:69-74` 逐字一致 | ✅ |
| 2 | 10.1① `fs.js:206-224` | 30 s 心跳 `utimes` 锁与心跳文件 + `timer.unref()` | `:206-224` 一致 | ✅ |
| 3 | 10.1① `fs.js:264-275` | 释放时持闸 + **只有 token 匹配才 unlink** | `:268-274` `owner?.token === token` 后 unlink 两文件 | ✅ |
| 4 | 10.1① `fs.js:342-368` | `.reclaim` 次级锁 + **持闸后重新校验**（TOCTOU） | `:342` 开闸、`:352` 迭代清扫、`:368` `existingLockCanBeReclaimed(lockPath)` | ✅ |
| 5 | 10.1② `scan-roots.js:89-110` | `opendirSync().closeSync()` 把「存在」定义为「能被列举」；三态语义 | `:101-108` 逐字（含注释 "Probe listability without reading entries; large roots stay cheap."） | ✅ |
| 6 | 10.1② `:248-253` | `unreadable: CODE` vs `missing` | `:248-252` 逐字 | ✅ |
| 7 | 10.1③ `async-queue-reader.js:8-10` | 签名 `${dev}:${ino}:${size}:${mtimeNs}:${ctimeNs}` | `:8-10` 逐字 | ✅ |
| 8 | 10.1③ `:40-47` | 四条件续读（同 dev:ino + size 变大 + 头部与锚点一致） | `:40-47` 逐字 | ✅ |
| 9 | 10.1④ `upload-queue.js:5-6,134` | `BACKLOG_BYTES=1_000_000`、`CHUNK_BYTES=64KiB`、全文 134 行 | 常量一致；`total=134` | ✅ |
| 10 | 10.1④ `:73-78` | 只在「够大 + offset 落在行边界」动手；`minPendingBytes` 默认 1_000_000 | `:73` 守卫、`:74-77` 边界字节判定（`!== 10`）、`:64` 默认值 | ✅ |
| 11 | 10.1④ `:111-121` | `output.sync/close` → `input.close` → `stat` → `ino/size/mtimeMs` 三重校验 → `rename` | `:111-121` 逐行一致（含 Windows 注释） | ✅ |
| 12 | 10.1④ `:79-91,105-110` | `[0,offset)` 原样复制；`[offset,size)` 每键留最后一条；account 用 `verifiedMs` 严格更大 | `:84-90` 与 `:97-110` 一致 | ✅ |
| 13 | 10.1⑤ `fs.js:9-33`,`:15-17` | tmp 名带 UUID；失败 unlink；`mode` 仅创建时生效 | `:12-14` UUID 注释、`:24` rename、`:26-30` unlink、`:15-17` mode 注释 | ✅ |
| 14 | 10.1⑤ `fs.js:416-443` | `updateJsonLocked` 用 `<file>.lock`、超时 30 s | `:419` `timeoutMs = 30_000`、`:421` lockPath、`:437-438` 写 + chmod600 | ✅ |
| 15 | 10.1⑥ `cursor-store.js:13-18` | 五常量（`STORE_VERSION=2`、`cursor-store-v2`、`manifest.json`、`generations`、`core-v3.json`） | `:13-18` 逐字 | ✅ |
| 16 | 10.1⑥ `:855-1016` | 代目录 → 脏分片整文件重写 → `generation.json` → **最后**替换 manifest → 失败 rm → 保留三代 | `:856-863`、`:903-913`（`:907` unlink 后重建）、`:972` 注入点、`:977-987`、`:1005-1009`、`:1010-1015` | ✅ |
| 17 | 10.1⑥ `:1361`,`:1466-1481`,`:1807-1809` | `COPYFILE_FICLONE`（非硬链接）；摘要校验；`safeShardFilename` 白名单 + 否则 `misc` | `:1361` 逐字；`:1466-1480` sha256/bytes；`:1807-1808` 正则一致 | ✅ |
| 18 | 10.1⑥ `:165-197` | legacy 下 `codexEventStore = null`、能力不等价 | `:165` `this.codexEventStore = null;` 及后续空实现 | ✅ |
| 19 | 10.1⑦ `sync.js:234-248` | 0.26.3 事故；**21.7 亿** token；分项 `-1.27B/-474M/-376M/-48M/-6M` 求和自洽 | `:239` `wiped 2.17B claude tokens (-1.27B opus-4-7, -474M opus-4-6, -376M sonnet-4-5, -48M haiku, -6M sonnet-4-6)`；1270+474+376+48+6=**2174M** | ✅ |
| 20 | 10.1⑦ `:1147-1192`,`:1177-1181` | 扫根守卫「推迟」；提示语被 `if (!opts.auto)` 包住 | `:1176-1181` 逐字（`Claude ground-truth repair deferred: …` 在 `!opts.auto` 内） | ✅ |
| 21 | 10.1⑦ `:6271-6293`,`:6294-6315` | 快照读写/比较三函数 | `readCodexFileSnapshot :6271-6280`、`sameCodexFileSnapshot :6282-6292` ✅；`validateCodexRebuildFileSnapshots` 实际闭合于 `:6314`（文档写 `:6315`，±1） | ⚠️I2 |
| 22 | 10.1⑦ `:6316-6346` | `isCodexHistoryCovered` | 函数体实为 `:6316-6336`；`:6338-6346` 已是 `scanForForkedCodexRollout` 的注释块（+10） | ⚠️I1 |
| 23 | 10.1⑦ `:952-959` | 迁移键即幂等闸，失败路径故意不写键 | `:950-959` 注释 "Rebuild failures intentionally leave the migration key unset…" + 可重试哨兵 | ✅ |
| 24 | 10.1⑧ `rollout.js:302-380`,`:428-461`,`:459-460` | 本轮/历史双视图；四条件；两条兜底 `:459` `if (prev?.lastTotal) return true;` / `:460` `return false;` | `needsHistoricalRolloutDedup :428-461`；`:458` 四条件、`:459/:460` 兜底逐字 | ✅ |
| 25 | 10.1⑧ `:292-295`,`:4914-4919`,`:1233-1237` | Codex 键 `sessionUUID:eventTimestamp`；Claude 键 `msgId[:reqId]`；10 万截断 | `:292-295` 注释、`:4914-4919` 函数、`:1236-1237` `slice(len-100_000)` | ✅ |
| 26 | 10.1⑨ `:1035`,`:1056-1107`,`:1074-1077`；`sync.js:683-684` | `fileId` 缓存/`duplicateOf` 快路径；native 排前 | 全部逐行一致（`:1035` `new Map()` 条件、`:1070-1077` primary 判定、`:683-684` 注释） | ✅ |
| 27 | 10.1⑩ `rollout.js:3328-3344/:3341/:3342/:4073-4079/:3513/:3515-3532`；`local-api.js:145-153/:202-211`；`sync.js:3654-3659` | 项目行裁剪（多 2 列、少 model/`usage_precision`/`priority_*`）；两队列 key；只算主队列 pending | 全部逐行一致（`:3341` usage_precision、`:3342` prioritySubsetOf、`:3513` 缺键跳过、`:149` `project_key|source|hour_start`、`:207` `source|model|hour_start`、`:3654-3659` 注释 + `pendingBytes`） | ✅ |
| 28 | 10.1⑪ `sync.js:4447-4449/:4488-4490/:4511-4512/:4313/:4329/:4392` | 只发完整行；控制行计入上限；200/500/10 夹取 | `:4449` `if (!record.complete) break;`；`:4488-4489`、`:4511-4512`；`:4313` 500、`:4329` 夹取、`:4392` community 10 | ✅ |
| 29 | 10.1⑫ `sync.js:3763-3819` | 双阈值 + 回拨保护 + 6 字段状态 + 独立 `deferred-codex-audit.json` | `:3769-3782` 逐行；`:3797-3807` 5 字段 + `fullAudit` 补 `lastFullScanAt` = 6；`:3456-3459` `writeDeferredCodexAuditSyncs` | ✅ |
| 30 | 10.1⑬ `local-api.js:155-200` | 不变量识别 + `input -= cached` + `billable` 抬到 `total` | `:167` 不变量逐字、`:176` 减法、`:192-197` cursor/antigravity 抬升 | ✅ |
| 31 | 10.1⑬ `sync.js:4504-4509` | 上传前共用同一纠正 | `:4509` `require("../lib/local-api").normalizeQueueRow(bucket)` + `:4504-4508` 注释 | ✅ |
| 32 | 10.1⑭ `sync.js:484-506`,`:565`；`status.js:285`,`:1054-1058` | marker 语义 + 抢到锁即删 + status 上报 | `:484`、`:491-505`、`:565` `clearSyncSkip`、`status.js:285` 读、`:1058` `last_sync_skipped` | ✅ |
| 33 | 10.1⑮ `init.js:1445-2232/:1453-2231/:1446-1447/:1495-1498/:1519-1536/:1538-1559/:1561/:1563-1573` | 模板边界、头注释、先落信号、20 s 节流、链式 hook、`exit(0)`、detached | 全部逐行一致（宿主函数 `:1445`、反引号 `:1453`/`:2231`、闭括号 `:2232`；`:1446-1447` 两行注释逐字） | ✅ |
| 34 | 10.1⑮ `:1483-1484`；`openclaw-session-plugin.js:358,361` | 两变量同路径 → 条件退化 | `:1483` `JSON.stringify(trackerBinPath)`、`:1484` 字面 `path.join(...)`；openclaw `:358/:361` 同形 ✅；但「**字面相同**」措辞不准（求值相同、字面不同，R2-1 A10 已按「字面不同求值相同」记） | ⚠️I3 |
| 35 | 10.1⑯ `notify-command.js:23-36` | 只接受 `node(.exe) notify.cjs --source=<id>`，身份 `${file}\u0000${args[2]}` | `:32-35` 逐字 | ✅ |
| 36 | 10.1⑯ `claude-config.js:181-230/:248-262`；`gemini-config.js:265-279`；`codex-config.js:89-104/:245-309` | 就地修复 + scope 去重；三处 `.bak.<ISO>`；notify 原命令另存；TOML 手写行编辑 | `:183-187` repair、`:189-228` 去重；`claude-config:254`、`gemini-config:271`、`codex-config:101` 备份；`:93-98` `*_notify_original.json`；`:245-250` 自述支持单行/多行数组 | ✅ |
| 37 | 10.1⑯ `omp-hook.js:17,388-395` + `init.js:813` | 哨兵范式只用于 omp；消费方 init.js | `:17` MARKER、`:388-394` `unmanaged-extension-present`、`init.js:813` 分支 | ✅ |
| 38 | 10.1⑯ `opencode-config.js:66-87`,`:73-75`,`:80-85`,`:94` | 内容等值短路**非**哨兵；先备份再无条件覆盖；`hasPluginMarker` 只在 remove | `:73-75`、`:80-85`、`:94` `unexpected-content` 全部逐行一致 | ✅ |
| 39 | 10.1⑯ `grok-hook.js:85-91` | 注释「always overwrite…」 | `:85` 注释逐字、`:87/:91` 无条件 `writeFile` | ✅ |
| 40 | 10.1⑰ `install-resolver.js:25-36/:6-18`；`wsl-probe.js:232-242/:193-201/:244-262` | `populated` 折进探测；union 契约；UNC 重锚 + 盘符前缀 + sqlite 快照 | `:30`、`:32/:36`；`:6-18` 注释逐字（Codex `sessionUUID:eventTimestamp`、Claude 手工 union）；`:233-241`、`:193-195`、`:249-254` | ✅ |
| 41 | 10.1⑱ `wsl-probe.js:57-63`；`cursor-store.js:25-32/:972`；`sync.js:516-538/:310-373` | 依赖注入默认参数化；别名表 8 条；`AUTO_SYNC_SOURCES` 45 项 | `:57/:63`、`:25-31`（含 `failureInjector`）、`:972`、`:519-538`；`:310-317` 8 条、`:320-365` **45 项 0 重复**、`:367` 4 项 | ✅ |
| 42 | 10.2#1 | `cmdSync` 单函数 **3190 行**，`src/commands/sync.js:516-3709` | 范围正确；但 `:516` 起、`:3709` 闭合（`:3711` 即 `parseArgs`）→ **3194 行**，差 4 | ❌E4 |

**10.2 其余条目（另计 15 条，全部 ✅）**：`fs.js:178-204`（issue #431 迭代清扫，注释逐字）；队列写点「≥6 处」+2 旁路（`rollout.js:3483/20031/22594`、`sync.js:5126/5207/5281`、`sync.js:4114`、`rollout.js:3536` **8/8 命中**）；`sync.js:249-256` 云端 `conversation_count` 事故注释；`cursor-store.js:21,77-86,165-197`；`rollout.js:3181-3194,3430-3480`（`groupQueued` 存在即 legacy 标记 + 整小时聚合成 `DEFAULT_MODEL`）；`bin/tracker.js:36-40`（`unprotected === true` → 两句 `console.error` + `process.exit(1)`，措辞宜收窄为「代理无法生效且无法阻断出站」）；`local-api.js` 全文 **3543** 行；`sync.js:519-538` 测试钩子；`rollout.js:489-492`（`st.ino`/`sameInode`/`truncated`）；`serve.js:15-21`（DoSvc 注释 + `WSL_DEFAULT_PORT = 7681`）；`pricing/index.js:31`（本文件**零内部读取**，仅 `:375` 导出）+ `codex-rollout-parser.js:51` → `:733`；`claude-categorizer.js:1175-1191` 副本语义等价、`test/` 两处（`sync-droid-dup-session-repair.test.js:29` 与 `trae-entitlement.test.js:30`）**均从 `../src/lib/rollout` 引入**、副本零测试引用；`pricing/index.js:248-253` 五源白名单 + `test/claude-reasoning-usage.test.js:45-47`；`leaderboard-refresh.ts:569-613` 无长上下文/Fast 分支、`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 字节比对、**独立复算 edge-only 44 / curated-only 9**，`grok-build-free`、`grok-4.5-build-free` 正在 curated-only 集合内。

**10.2#14 单列（❌E5）**：文档称 `hookStatus` 传参里「**有两个键**在 passive 判定中永不使用」。实测 `status.js:1024-1036` 传入 **11 键**，`detectPassiveProviders`（`passive-mode.js:93-186`）只消费 8 个，**未消费的是 3 个**：`opencode_plugin`、`openclaw_session_plugin`、**`grok`**。且 `<jsdoc>`（`passive-mode.js:88-90`）自述形状为 `{claude, gemini, codex, every_code, opencode, openclaw, codebuddy, workbuddy, grok}` —— 与实现**双向**不一致（JSDoc 里的 `opencode`/`openclaw`/`grok` 未实现，实现读的 `acode_notify`/`workbuddy-ai` 不在 JSDoc）。证据比文档叙述更强，但「两个键」的计数与键名列举都不完整。

**10.3（另计 8 条，全部 ✅）**：retraction 零行（`rollout.js:3364-3372` 对齐模型、`:3388-3396` 撤回 `DEFAULT_MODEL`，均取 `zeroTotals`，且 `:3382` 有 `retractedUnknownKey` 幂等闸）；`setMaxTotal/mergeMaxTotals` 仅用于 codex rescan 守卫（唯一调用族 `:5848-5879`，`console.error` 两处含 `skipping to avoid data loss`，`:5843`/`:5876` 逐字）；`local-api.js:213-225`,`:224`,`:2540`,`:482`；`curated-overrides.json` = **84/1/64/47**、775 行 / 29754 字节；`seed-snapshot.json` = 顶层 **2285**（1 `_meta` + **2284**）、256869 字节、`_meta.generated_at = 2026-05-26T11:12:29.761Z`、`cache_read` 键 **566**、`cache_creation` 键 **188**（→ 缺读价 75.2%、缺写价 91.8%，与文档「约 75%/92%」一致）。

### B.2 §11（27 条，命中 26，引用错误 1）

| # | 常量 / 阈值 | 文档值 | 实测 | 判定 |
| --- | --- | --- | --- | --- |
| 1 | 队列批量与上限 | 200 / 500 / 社区 10 | `sync.js:4315` 形参 `batchSize = 200`；`:4313` `MAX_INGEST_BUCKETS = 500`；`:4329` 夹取；`:4392` `Math.min(batchBudget, 10)` | ✅ |
| 2 | 上传总预算 | `UPLOAD_BUDGET_MS = 90_000`，`:168` | `:168` 逐字；`:4360-4362` 剩余预算进 `AbortSignal.timeout` | ✅ |
| 3 | hook 节流 | 20 s，`init.js:1519-1536` | `:1519` 注释 + `:1525` `if (now - last > 20_000)`，时间戳在 spawn 前写 | ✅ |
| 4 | 锁心跳 / 超期 | 30 s / 5 min，`fs.js:69-70` | `:69-70` 逐字 | ✅ |
| 5 | 回收闸 / 过渡守卫 | 4 / 64 / 5 / 1000，`fs.js:71-74` | `:71-74` 逐字 | ✅ |
| 6 | JSON 读改写锁超时 | 30 s，`fs.js:416-443` | `:419` `timeoutMs = 30_000`、`:421` `<file>.lock` | ✅ |
| 7 | 冷扫描触发 | 24 h / 288 次 / 回拨 >5 min | `:294`、`:301`、`:3772-3781` 逐字 | ✅ |
| 8 | 游标阈值 | 16 MiB，`cursor-store.js:21`，切换 `:77-86` | `:21` `16 * 1024 * 1024`；`:78-86` `legacySize >= activationBytes` | ✅ |
| 9 | 项目上下文重扫 | 24 h，`:22`，判定 `:1404-1406` | `:22` 逐字；`:1406` `checkedAtMs + PROJECT_ABSENT_CONTEXT_RESCAN_MS` | ✅ |
| 10 | 长上下文阈值 | 272_000；定义 `index.js:31`（零内部读取），生效 `codex-rollout-parser.js:51` → `:733` | 两处均 `272_000`；`index.js` 仅在 `:31` 定义、`:375` 导出；`:733` 为 `isLongContext` 判据 | ✅ |
| 11 | 长上下文启用集合 | `gpt-5.6`/`-sol`/`gpt-6-astra`/`gpt-6-sol`/`gpt-6.1-sol`，`index.js:266-306` | `:267-269` 逐字 | ✅ |
| 12 | DeepSeek 周末起点 | `Date.UTC(2026, 7, 22, 16, 0, 0)`，`index.js:186` | `:186` 逐字（+ `:194` 起判定） | ✅ |
| 13 | DeepSeek 折扣 | ×0.5；peak = [1,4)∪[6,10) UTC；acode 豁免 | `:205-206`、`:217-220`、`:213` 全部一致 | ✅ |
| 14 | 价格缓存 TTL / 降级链 | 24 h / 10 s / 4 级，`litellm-fetcher.js:12-13`、`:107-164` | `:12-13` 逐字；`:127-137`→`:139-146`→`:148-158`→`:160-163` 四级 | ✅ |
| 15 | 负缓存 | `index.js:64-66,120-131`；`revision` 变化清空 `:86`,`:105` | `:66` `new Set()`、`:120-131` 命中/写入、`:86`/`:105` `clear()` | ✅ |
| 16 | 策展表条目 | exact **84** / alias 1 / acode **64** / fuzzy **47**；775 行 / 29754 字节 | 独立重算（Node `JSON.parse`）：**84 / 1 / 64 / 47**；`775` / `29754` | ✅ |
| 17 | seed 快照条目 | 2285 = 1 `_meta` + **2284**；256869 字节 | 顶层 **2285**、模型 **2284**、字节 **256869** | ✅ |
| 18 | 云端手抄表 | **119** 键（`:142-332`）+ iFlytek **64** 键（`:339-404`） | 我按 `"key": { input:` 正则提取 = **119**；头部 `IFLYTEK_MAAS_MODEL_PRICING` 起 `:339`、块尾 `:332` | ✅ |
| 19 | priority 乘数条目 | **3** 条 = 2 | 独立枚举 `priority_multiplier > 1` = **3**：`gpt-6-astra`、`gpt-6.1-sol`、`gpt-6-sol` | ✅ |
| 20 | fork 判定 | `FORK_REPLAY_GAP_MS = 500`，`rollout.js:4210`；应用 `:2588-2601`；`replayPrefixActive` `:2339` | `:4210` 逐字、`:2588-2594` 突发检测、`:4212-4214` 日期守卫 | ✅ |
| 21 | Claude 哈希上限 | **100 000**，`:1236-1237`；注释 `:1021-1025` | `:1236-1237` 逐字；`:1233` 注释 | ✅（注释行号 ±4 未逐字，不作差异） |
| 22 | 事件集合按日分片 | `cursor-store.js:681-726` | `loadEventSet :681-693` / `loadEventSetUnchecked :695-` 一致 | ✅ |
| 23 | project 汇总上限 | **10**，`local-api.js:2737-2740`；排序 `:2785` | `:95` `PROJECT_USAGE_MAX_ENTRIES = 10`、`:2738-2740` `Math.min(..., 10)`、`:2785` 降序 | ✅ |
| 24 | sessions 分页上限 | **2000**，`local-api.js:2505`，缺省 0；「其余端点全部量（`:95`）」 | `:2505` `Math.min(limitParam, 2000)`、缺省 `0` ✅；**但 `:95` 是 `PROJECT_USAGE_MAX_ENTRIES = 10`，与「其余端点无分页」无关**；按 `no pagination/full array/unpaged` 等词在 `local-api.js` 全文检索**零命中** | ❌E6 |
| 25 | 压缩 / 读侧分块 | `CHUNK_BYTES=64KiB`、`BACKLOG_BYTES=1_000_000`；读侧 64 KiB + 缓存 4 文件 | `upload-queue.js:5-6`；`async-queue-reader.js:5`、`:16` `maxEntries = 4`、`:94` | ✅ |
| 26 | 记录大小上限 | `OMP_LIKE_MAX_RECORD_BYTES = 64 MB`、`LMSTUDIO_WINDOW_BYTES = 8 MB` | `rollout.js:15715`、`:12853` 逐字 | ✅ |
| 27 | 上传退避 / 源集 | 60 s 起 / 30 min 上限 / `2^step` / step 上限 10（记 20）；`AUTO_SYNC_SOURCES=45`、`BACKGROUND=4` | `upload-throttle.js:8-9`、`:109-110`、`:123` 逐字；`sync.js` 程序化计数 **45 / 4**（0 重复，与文档列举逐字一致） | ✅ |
| 28 | 云端归档包 | 单包 **10000 条 / 8 MiB**，位置 `src/lib/cloud/usage-archive.js:35-39` | 值**成立**（`migrations/20261005120000_cloud-usage-archive.sql:72` `CHECK(row_count <= 10000)`、`:213`/`:309` `v_count>10000 OR pg_column_size(v_payload)>8388608`）；**但 `src/lib/cloud/` 目录整仓不存在**，客户端实现在 `scripts/cloud-usage-archive.cjs`（`test/cloud-usage-archive-runner.test.js:12` require） | ❌E1 |

### B.3 §12（36 条，命中 33，计数/范围 3）

| 类 | 抽样点 | 文档判定条件 / 断言 | 实测 | 判定 |
| --- | --- | --- | --- | --- |
| A1 | Claude 无行边界提交 | `const endOffset = st.size;` 3 处 `:2291,:2682,:2793`；只有 `:2793` 落 token 游标 `:1194`；Codex 用 `committedEndOffset :2652`、项目上下文 `:2765`；OpenClaw `:1949` | 精确命中：`:2291`（无新字节早退）、`:2682`、`:2793` 各一处；`:1194` `offset: result.endOffset`；`:2652`/`:2765` 均为 `committedEndOffset`；`openclawOffsetEndsAtLineBoundary` 定义 `:1949` 且用在 `:1787` | ✅ |
| A1 | 收窄条件 | 半行被跳过可由 inode 变化/截断抵消（`:488-504`） | `:501` `sameInode && !truncated && !rebuildingCodexBaseline ? prevOffset : 0`；`:492` `truncated` 定义 | ✅ |
| A2 | Claude 缓存别名 | `rollout.js:4927-4928` 只读 `cache_creation_input_tokens`；`codex-token-usage.js:25-27` 有 `cache_write_input_tokens` | `:4927` 仅 `u?.cache_creation_input_tokens`；`codex-token-usage.js:25-26` `value.cache_creation_input_tokens ?? value.cache_write_input_tokens` | ✅ |
| A4 | legacy 死代码 | `rollout.js:3181-3194,3430-3480`；`sync.js:216-220` | `:3181-3194` 一致；`sync.js:215-220` 注释「enqueueTouchedBuckets uses presence of that key as the legacy-group marker」 | ✅ |
| A6 | 压缩/offset 边界 | `upload-queue.js:97-110` | `:97-103` 复制 `[0,offset)`、`:105-110` 重写 pending | ✅ |
| A7 | `multiInstallParse` 异常写回 | `multi-install-parser.js:49-54` | `:50-53` 条件恢复 `cursors[providerName] = ns` 后 `throw` | ✅ |
| A11 | qoder new 全零行 | `rollout.js:6639-6660`；入队取决于 `:3325-3347` | `:6650-6659` 全零 + `conversation_count: 1`；`:3325-3326` `usagePrecision` 进 `key`、`:3327` `queuedKey === key` 短路 | ✅ |
| A12 | trae-cn 全零会话行 | `:22477-22492` 只校验非负；`:22500-22508` 写全零 totals；`:22749-22751` 无零值护栏 | `:22500-22509` 全零 totals + `conversation_count: 1`；`:22749-22751` `addTotals` 直入 | ✅ |
| A13 | reasonix 双 home | `:16925-16934` 返回 `~/.reasonix` 与 `%APPDATA%\reasonix` | `:16925` + `:16932-16934` 逐字 | ✅ |
| A15 | grok 兜底时间归属 | `:20249-20252`、`:20265`、`:19691` 回落 `new Date().toISOString()` | `toUtcHalfHourStart` 精确落在 `:20250/:20251/:20252/:20265`；`:19691` 链尾 `new Date().toISOString()` | ✅ |
| A16 | craft 截断 clamp | `:17189-17195` 注释承认负面差值并 clamp 0 | `:17189-17195` 逐字（`Math.max(0, totalInput - prev.input)` 系列） | ✅ |
| A17 | `mainUsageHourTotalsFromQueueRows` 用 MAX | `sync.js:6540` vs `local-api.js:207` last-wins | `:6540` `setMaxTotal(...)`；`local-api.js:207` `key = source|model|hour_start`（last-wins） | ✅ |
| A18 | `model/rerouted` 从未命中 | `codex-model-attribution.js:47-51` 自述 0 命中 | `:47-51` 逐字（"no model/rerouted event has been observed in any real rollout … inferred from the app-server protocol"） | ✅ |
| A补充 | passive-mode 覆盖 8 家 | `passive-mode.js:96-185` 只覆盖 8 个 hook 驱动 provider | `:97-186` push 数组实为 **8 家**：claude/gemini/codex/acode/every_code/codebuddy/workbuddy/workbuddy-ai | ✅ |
| A补充 | Grok/OpenCode/OpenClaw/omp 由独立路径上报 | `status.js:1300-1336` | 该区间只有 **omp（`:1317-1318`）**；**OpenCode 在 `:1363`、Grok 在 `:1452`**，OpenClaw 是 hook 状态行 `:1296-1298`（非「日志在」reader 行） | ⚠️I4 |
| B5 | 本地 `streak_days` 恒 0 | `local-api.js:2588`，同响应 `:2587` 给 `active_days`；前端自行重算 `use-activity-heatmap.ts:206,220,284,295`；`pet-linux-host.js:198` 直读 | 四点全部逐行命中（`streak_days: computeActiveStreakDays(...)` / `streak: toNumber(r.body?.streak_days) ?? 0`） | ✅ |
| B6 | usage-limits 口径 | `:4055` 解构 18 个变量；`data` 只收 17 家（`:4406-4440`） | `:4055` 起 destructure 多行；`:4406-4440` `data` 键 = 17 家 + `fetched_at` | ✅ |
| B10 | 云端归档未启用 | `docs/cloud-usage-archive.md:74-83` | 文件 83 行；`:74` 「Activation checks still pending」、`:83` 「do not enable a retention schedule…」 | ✅ |
| C1 | `cmdSync` 巨型编排 | 3190 行、13 个条件项 | 13 项**逐项计数成立**；行数 3194 见 E4 | ❌E4 |
| C3 | 队列写点 | 6 处主队列 + `:4114` + `:3536` | 8/8 命中（见 B.1） | ✅ |
| C4 | repair「先删后建」 | 2.174B ≈ 21.7 亿 | 源码 `2.17B` + 分项求和 2174M | ✅ |
| C5 | Grok 无备份覆盖 | `grok-hook.js:85-91` | 逐行一致 | ✅ |
| C6 | 三套并行纠错 | `sync.js:249-256` | 注释逐字（ingest 读 `b.conversations`） | ✅ |
| C7 | 16 MB 静默切换能力不等价 | `cursor-store.js:21,77-86,165-197` | 一致 | ✅ |
| C8 | 代理失败 `exit(1)` | `bin/tracker.js:36-40` | 一致；判据措辞见 I5 | ✅ |
| C10 | 测试钩子入生产签名 | `sync.js:519-538` | `:519/:538` 及 `context.*` 逐行 | ✅ |
| C11 | 依赖 `st.ino` | `rollout.js:489-492` | 逐行 | ✅ |
| C13 | hookStatus 传参与消费不一致 | 「两个键」 | 实为 3 键（+`grok`），且 JSDoc 双向漂移 | ❌E5 |
| C14 | 双份 272_000 无测试绑定 | `index.js:31` 零读取 / `codex-rollout-parser.js:51` | 一致 | ✅ |
| C15 | 推理折叠名单两套 | `index.js:248-253` 5 个 | `:248-253` 恰为 `codex/acode/every-code/omo/cline` | ✅ |
| C16 | 9 键差集 | `edge-pricing-parity.test.js:76-85` 只做 edge↔edge | 测试体逐字（"byte-identical across all 5 edge files"）；独立复算 **edge-only 44 / curated-only 9** | ✅ |
| C17 | `PI_SUBSCRIPTION_SOURCES` 4 vs 2 | `index.js:18-23` vs `leaderboard-refresh.ts:575` | `:18-23` 4 成员；`:575` `pi-github-copilot`/`pi-copilot` 2 成员 | ✅ |
| C18 | 云端无长上下文/Fast | `:569-613` | 函数 `:569-613` 闭合，确无 `long_context_*`/`priority_*` 分支 | ✅ |
| C19 | 金额精度三处 | `local-api.js:2415,:2648`、`session-analytics.js:289`、`ts:1349` | `:2415`/`:2648` `toFixed(6)`；`:1349` `Math.round(x*100)/100` | ✅ |
| C21 | reasoning 无对等回归 | `test/claude-reasoning-usage.test.js:45-47` | `:45-47` 恰为 `computeRowCost` 前后对比（列分布可换、费用不变） | ✅ |
| C22 | 同一面板两套时间口径 | `:2540`（UTC 日）vs `:482`（请求时区小时） | `:2540` `new Date(\`${todayStr}T00:00:00Z\`)`；`:482` `getZonedParts(new Date(row.hour_start), timeZoneContext || {})` | ✅ |
| C23 | `streak_days`/`week_starts_on` 硬编码 | `:2586,2588` | `:2586` `week_starts_on: "sun"`、`:2588` `streak_days: 0` | ✅ |
| C补充 | 项目详情 `models` 装的是 source | `local-api.js:2877` | `:2877` 注释「`models` keyed by source (project rows carry no model column)」 | ✅ |
| C补充 | `rollout.js:6726` 悬空注释 | 称 credits 会累积，下一行即 `touchedBuckets.add` | `:6726` 注释、`:6727` `touchedBuckets.add(...)`，上下文无实现 | ✅ |
| C补充 | `usage_precision` 16 行 / **14 个语句** | `rollout.js` 全清点 | 16 行**逐行复现**（与文档清单完全同集）；「14 个语句」**不可复现**：`:20286-20287` 是一条跨两行赋值，合并后 15 条。R2-2 已判此项 ❌（"我算 15"），主文档与附录 B 均未处置 | ❌E2 |
| D9 | Workbuddy traces 字段不同形 | `:11100-11260` 段 | R2-2 第 6 条独立结案，本轮采信（未重复逐行） | ✅（引用 R2-2） |
| D11 | 119 / 64 / 9 键 | `ts:142-332`、`:339-404` | 独立重算一致（R2-3 A19 同结论） | ✅ |
| D13 | 行数断言 | `rollout 24932 / sync 7251 / init 2427 / local-api 3543 / session-analytics 2934 / status 1729 / index 378 / matcher 439 / curated 775 / upload-queue 134` | **10/10 全部重算命中** | ✅ |

### B.4 §13（7 条，命中 6，不一致 1）

R1 六份报告自报数字（我逐份打开头部「抽样统计」节核对）与 §13.1 表**逐格一致**，合计由我重算：

| 项 | 文档 13.1 表 | 六份报告自报（重算） | 判定 |
| --- | --- | --- | --- |
| 抽验条数 | 772 | 139+46+141+188+157+101 = **772** | ✅ |
| E | 50 | 9+1+12+9+8+11 = **50** | ✅ |
| I | 43 | 9+8+7+6+6+7 = **43** | ✅ |
| U | 29 | 10+4+2+3+7+3 = **29** | ✅ |
| 通过 | 650 | 111+33+120+170+136+80 = **650** | ✅ |
| 自洽 | — | 50+43+29+650 = **772** = 表头总数 | ✅ |

- R1-D 的报告头明确「抽验断言数：**188**（代码块行号 62 条）」「结论分布 **E=9，I=6，U=3**（其余 170 条）」→ 与文档 R1-D 行一致 ✅。
- R2-1：报告「A 组 40 条中 35 ✅ / 4 ⚠️ / 1 ❌；B 组 40 条；C 组 13 处；必须修 3 条，均为表述级」→ 与文档 13.2 R2-1 行一致 ✅。
- R2-2：报告第 2 条原文即「在**修正记录之外**又查出 **6 处残留漂移行号**」→ 与文档「6 处」一致 ✅，且偏移量 +6/+10/+10 描述一致 ✅。**但报告所列必须修实为 E1–E10**（另含两处兜底常量 `9697→9704`、`9918→9924`，S3a E12 替换值仍错，S3b MiniMax Code 语义反转，以及 `usage_precision`「14 个语句」不可复现）—— 文档摘要只保留「6 处漂移 + D9 结案」。
- **R2-3（❌E3）**：报告自报「A 组 **49** 条（S4 **18** / S5 **17** / S6 **14**）、B 组 **46** 条、C 组 **10** 处；计数类断言重算 21 组；新增判定 E2 / I8 / U1·U6」。文档 13.2 写作「A 组复查 S4 段 18 条 + S5 段 17 条」（= 35），**漏 S6 14 条与 B 组 46 条**，与报告自报工作量不一致。

### B.5 交叉核对：13.3「被反向纠正的复核结论」可复现性

| 项 | 断言 | 实测 | 判定 |
| --- | --- | --- | --- |
| 1 | `recordsProcessed: total`（`:7270`、`:22791`）、`recordsProcessed: eventsAggregated`（`:20551`）三处是**值标识符**非键名 | `sync.js` 与 `rollout.js` 相关行形态一致（`:19008` 亦为 `recordsProcessed,` 简写） | ✅ |
| 2 | 「12 种返回字面量」实为 11 种，漏 `parseCopilotSessionStoreIncremental` 的 **9 键**集合 `rollout.js:19003-19013` | 逐字：`active/healthy/adoptedThisRun/canonicalDbPaths/recordsProcessed/eventsAggregated/bucketsQueued/dbErrors/usageClaims` = **9 键** | ✅ |

---

## C. 幻觉与越界

1. **不存在文件（最重）**：`src/lib/cloud/usage-archive.js`。整仓**没有** `src/lib/cloud/` 目录；递归检索 `usage-archive*` 只命中测试与 `docs/cloud-usage-archive.md`；实现体是 `scripts/cloud-usage-archive.cjs` + `migrations/20261005120000_cloud-usage-archive.sql`。**该行的数值（10000 条 / 8 MiB）经我落到 SQL 里复核为真**（`:72` CHECK、`:213`/`:309` `8388608` 字节），故属「**引用路径虚构、数值正确**」——但读者按此路径无法复核，必须修。
2. **计数越界（1 条）**：§12 C 补充与附录 B 的「`usage_precision` … 14 个语句」。16 行可复现，语句数不可复现（自然口径 15，R2-2 亦算 15）。**这条已经被上一轮点名，仍留在正文**——属「复核结论未回写」类残留，非新错。
3. **范围越界（1 条）**：§12 补充登记称 Grok/OpenCode/OpenClaw/omp 的「hook 缺失但日志在」由 `status.js:1300-1336` 上报；实测该区间只含 omp（OpenCode `:1363`、Grok `:1452`）。原句已自标「未逐一核对」，但仍应修正区间或删去三家。
4. **函数范围越界（2 条）**：`isCodexHistoryCovered` 写作 `:6316-6346`（实为 `:6316-6336`，多出的是下一函数注释）；`validateCodexRebuildFileSnapshots` 写作 `:6294-6315`（闭合于 `:6314`）。
5. **未发现证据等级被升级的断言**：§10–§13 与附录中被标为「工程推断」「【未能确认】」「未逐一核对」的条目，抽查后**全部保持在推断级**（例如 §10.1⑦ 的 `:459` 动机仍标【未能确认】；§10.1⑯ 的 TOML 覆盖面仍标【未能确认】；§12 A/B 两类的判定条件仍写「需运行时/外部信息」）。**未发现把推断写成源码事实、或把否定性证据写成肯定性证据的情况。**
6. **未发现方向相反的断言**：三处最容易反的方向 —— 「队列写点不是一处（8 处）」「reasoning 在 Codex 侧是子集并置 0（本地 §10.2#16）」「OpenCode 不拒绝覆盖外来文件、哨兵只属 omp」—— 逐条回源码后**方向均正确**。
7. **越界检查**：§10–§13 与附录 A/B 全文不含对样本仓库的修改声明、不含本机 `~/.tokentracker` 读取声明；本轮亦未写入样本仓库（`git status --porcelain` 空）。

---

## 必须修清单（按严重度排序）

| 级别 | 位置 | 问题 | 建议改法 |
| --- | --- | --- | --- |
| **E1** | §11「云端归档包」行 | 位置 `src/lib/cloud/usage-archive.js:35-39` **不存在**（整仓无 `src/lib/cloud/`） | 改为 `migrations/20261005120000_cloud-usage-archive.sql:72,213,309`（SQL 侧硬界）+ `scripts/cloud-usage-archive.cjs`（客户端 runner）；值 10000/8 MiB 保留 |
| **E2** | 附录 B「`usage_precision`」行 + §12 C 补充 | 「14 个语句」不可复现（自然口径 15；R2-2 已判 ❌ 未处置） | 删「/ 14 个语句」或改写为「16 行（其中 `:20286-20287` 为一条跨两行赋值）」 |
| **E3** | §13.2 R2-3 行 | A 组写作「S4 18 + S5 17」，报告自报 **49（S4 18/S5 17/S6 14）**，且漏 B 组 46 条 | 补 S6 14 条与「B 组 46 条、C 组 10 处、新增判定 E2/I8/U1·U6」 |
| **E4** | §10.2#1 + §12 C1 | 「`cmdSync` 单函数 **3190** 行」；实测 `:516`–`:3709` = **3194** | 改为 3194 行（范围 `:516-3709` 保留） |
| **E5** | §10.2#14 + §12 C13 | 「**两个键**在 passive 判定中永不使用」；实测未消费的是 **3 个**（`opencode_plugin`、`openclaw_session_plugin`、`grok`） | 改为 3 键；并补「`passive-mode.js:88-90` 的 JSDoc 自述形状与实现双向不一致」 |
| **E6** | §11「sessions 分页上限」行末 | 「其余用量端点一律返回全量数组（`:95`）」——`:95` 是 `PROJECT_USAGE_MAX_ENTRIES = 10`，与该句无关 | 删括号引用，或换成可核实的引用（`local-api.js` 全文检索无对应注释） |
| I1 | §10.1⑦ | `isCodexHistoryCovered` 范围 `:6316-6346` → `:6316-6336` | 收窄，+10 越界 |
| I2 | §10.1⑦ | `validateCodexRebuildFileSnapshots` `:6294-6315` → `:6294-6314` | ±1 |
| I3 | §10.1⑮ | 「两个变量……**字面相同**」 | 改「求值相同、字面不同」（R2-1 A10 已如此记录） |
| I4 | §12 补充登记 | Grok/OpenCode 的独立上报路径不在 `:1300-1336` | 改为 `omp :1317-1318`、`OpenCode :1363`、`Grok :1452`、`OpenClaw hook 状态 :1296-1298` |
| I5 | §10.2#9 / §12 C8 | 「代理配置错误让整个进程 exit(1)」 | 收窄为「手动代理未能生效且无法阻断出站流量（`unprotected === true`）时 `exit(1)`」 |

---

## 复核到的行号抽样

以下为我本轮**实际打开并逐字读到**的行（UTF-8 口径），用于证明 A/B/C 三节的判定可复现：

```
rollout.js   24932 行总长；572 个顶层函数；45 个 parse*Incremental（251/954/1242/1412/1654/…/24458，逐行与附录 A 一致）
rollout.js   292-295   seen Codex event keys (sessionUUID:eventTimestamp)
rollout.js   428-461   needsHistoricalRolloutDedup（:458 四条件、:459 prev?.lastTotal、:460 return false）
rollout.js   492-501   truncated / rebuildingCodexBaseline / startOffset 三态重置
rollout.js   1035      const seenFileIds = sawUncFile && sawLocalFile ? new Map() : null;
rollout.js   1058-1077 fileId 缓存 + primary/duplicateOf 快照分支
rollout.js   1194      offset: result.endOffset,
rollout.js   1236-1237 claudeHashes 截断到 100_000
rollout.js   15715     const OMP_LIKE_MAX_RECORD_BYTES = 64 * 1024 * 1024;
rollout.js   16925-16934  reasonix 双候选 home
rollout.js   17189-17195 craft clamp 0
rollout.js   17376-17377 7 天 / 10000 的 Copilot claim 上限
rollout.js   19003-19013 parseCopilotSessionStoreIncremental 的 9 键返回
rollout.js   19691     ... || new Date().toISOString()（grok lastActive 兜底）
rollout.js   20031     await fs.appendFile(queuePath, `${lines.join("\n")}\n`, "utf8");
rollout.js   20250-20252 toUtcHalfHourStart(…event/lastActive/Date.now())
rollout.js   22500-22509 trae-cn 全零 totals + conversation_count: 1
rollout.js   22594     await fs.appendFile(queuePath, lines.join("\n") + "\n", "utf8");
rollout.js   22749-22751 addTotals 无零值护栏
rollout.js   3181-3194 groupQueued 存在 → legacyGroups
rollout.js   3325-3345 主行 push（:3341 usage_precision、:3342 prioritySubsetOf）
rollout.js   3364-3399 撤回零行（alignedModel / DEFAULT_MODEL 两支）
rollout.js   3482-3484 主队列 append（toAppend.length > 0）
rollout.js   3513-3536 项目行 18 列 + 项目队列 append
rollout.js   4073-4079 PRIORITY_SUBSET_FIELDS 五行
rollout.js   4210      const FORK_REPLAY_GAP_MS = 500;
rollout.js   4222-4238 bucketKey / projectBucketKey / parseBucketKey
rollout.js   4429-4442 deriveProjectKeyFromRef（URL 分段 join，无 .git 剥离）
rollout.js   4885/4927 codex 侧 ?? cache_write_input_tokens  vs Claude 侧仅 cache_creation_input_tokens
rollout.js   4914-4919 claudeMessageDedupKey
rollout.js   6639-6659 qoder new 零 totals（conversation_count: 1）
rollout.js   6726-6727 悬空注释 + touchedBuckets.add
sync.js      168       const UPLOAD_BUDGET_MS = 90_000;
sync.js      234-248   v6 天灾注释：2.17B（-1.27B/-474M/-376M/-48M/-6M）
sync.js      294/301   CODEX_COLD_SCAN_AUDIT_INTERVAL_MS / MAX_SYNCS = 288
sync.js      310-317   别名表 8 条
sync.js      320-365   AUTO_SYNC_SOURCES 45 项
sync.js      367-372   BACKGROUND_AUTO_SYNC_SOURCES（acode/codex/every-code/reasonix）
sync.js      484/491-506 SYNC_SKIP_MARKER + recordSyncSkip
sync.js      516       async function cmdSync(argv, context = {}) {   ← 与 :3709 构成 3194 行（E4）
sync.js      565       await clearSyncSkip(trackerDir);
sync.js      683-684   native is listed first…
sync.js      1176-1181 Claude ground-truth repair deferred（!opts.auto 内）
sync.js      3424-3437 skipNoOpCursorCommit（13 项）
sync.js      3456-3459 writeDeferredCodexAuditSyncs
sync.js      3654-3659 pendingBytes 只算主队列
sync.js      3763-3782 isCodexColdScanAuditDue（含回拨保护）
sync.js      3802-3818 codexColdScanAudit 6 字段
sync.js      4313/4315/4329/4392 MAX_INGEST_BUCKETS / batchSize=200 / 夹取 / community 10
sync.js      4447-4449 / 4488-4489 / 4511-4512 readQueueBatch 完整性 + 控制行计上限
sync.js      4509     require("../lib/local-api").normalizeQueueRow(bucket)
sync.js      5843/5876 …skipping to avoid data loss（两处）
sync.js      6540/6574-6589 setMaxTotal / mergeMaxTotals
sync.js      6271-6292 / 6294-6314 / 6316-6336 快照与覆盖三函数
local-api.js 3543 行；95 PROJECT_USAGE_MAX_ENTRIES = 10；145-153 项目 reader key；202-211 主 reader key
local-api.js 167       return totalTokens === inputTokens + outputTokens;
local-api.js 192-197   cursor/antigravity billable 抬升
local-api.js 213-224   rowDayKey（:224 hs.slice(0,10)）
local-api.js 482       getZonedParts(new Date(row.hour_start), timeZoneContext || {})
local-api.js 2415/2648 total_cost_usd.toFixed(6)
local-api.js 2505      Math.min(limitParam, 2000)
local-api.js 2540      new Date(`${todayStr}T00:00:00Z`)
local-api.js 2586/2588 week_starts_on: "sun" / streak_days: 0
local-api.js 2738-2740 Math.min(floor(limitParam), PROJECT_USAGE_MAX_ENTRIES)
local-api.js 2877      // `models` keyed by source
fs.js        455 行；9-33 writeFileAtomic；69-74 锁常量；206-224 心跳；264-274 释放；342-368 闸+重校验；416-443 updateJsonLocked(30_000)
cursor-store.js 13-18 常量；25-31 openCursorStore(failureInjector)；165 codexEventStore=null；855-1015 代际提交；1361 COPYFILE_FICLONE；1466-1480 摘要；1807-1808 safeShardFilename
scan-roots.js 101-108 三态；248-252 unreadable/missing
upload-queue.js 5-6 / 64 / 73-77 / 79-90 / 105-110 / 111-121（全文 134 行）
async-queue-reader.js 5 / 8-10 / 16 / 40-47 / 51-57 / 86 / 94
pricing/index.js 378 行；18-23 PI 订阅 4 成员；31 272_000（仅 :375 导出）；186 周末起点；205-206 peak；248-253 五源白名单；267-269 长上下文模型集；363 MODEL_PRICING = curatedOverrides.exact
pricing/litellm-fetcher.js 12-13 / 127-163 四级降级
curated-overrides.json 775 行 / 29754 B；exact 84 / alias 1 / source_exact.acode 64 / fuzzy 47；priority_multiplier>1 = 3
seed-snapshot.json 256869 B；顶层 2285（1 _meta + 2284）；generated_at 2026-05-26T11:12:29.761Z；cache_read 566 / cache_creation 188
init.js      2427 行；1445-1447 头注释；1453/2231 模板反引号；1483-1484；1495-1498；1519-1536（20_000）；1563-1573 spawnDetached
status.js    1729 行；285 读 marker；1024-1036 hookStatus 11 键；1054-1058 上传退避三键；1296-1298 / 1317-1318 / 1363 / 1452
passive-mode.js 93-186 8 家 provider；88-90 JSDoc 形状
source-metadata.js 1 ACCOUNT_LEVEL_SOURCES = ["cursor","trae-cn"]
grok-hook.js 85-87；opencode-config.js 73-75/80-85/94；omp-hook.js 17/388-394；notify-command.js 32-35
bin/tracker.js 36-40 exit(1)；serve.js 15-21 WSL_DEFAULT_PORT = 7681
wsl-probe.js 57/63；193-195；233-241；249-254
install-resolver.js 21-23 / 30-38
multi-install-parser.js 49-54；codex-model-attribution.js 47-51；codex-token-usage.js 25-27
upload-throttle.js 8-9 / 109-110 / 123；usage-limits.js 4055 / 4406-4440
leaderboard-refresh.ts 1449 行；142-332（119 键）/339-404（64 键）；569-613；575；1349
migrations/20261005120000_cloud-usage-archive.sql 72 / 213 / 309（10000 与 8388608 的唯一真值位点）
test/edge-pricing-parity.test.js 76-85；test/claude-reasoning-usage.test.js 45-47；test/sync-droid-…:29 与 test/trae-entitlement:30 均 require ../src/lib/rollout
docs/cloud-usage-archive.md 83 行；74 / 83
```

**本轮未做的事**（避免越界）：未运行样本仓库任何程序与脚本；未安装依赖；未读取任何真实会话数据；未修改主文档与样本仓库；未对 §1–§9 做任何判定（属其它复核 agent 范围）。
