# 复核报告 R3-1：主文档 元信息 + §1–§4

复核对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md` 第 19–859 行（元信息头部 +「本文是怎么做出来的」+ `## 1.`–`## 4.`，第 5 章标题前的全部分）。
样本：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（`git log -1` 实测，v1.3.14）。
口径：一律 `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` / `read`；全程只读，未改样本仓库、未改主文档。
抽样规模：**70 个引用簇 / 182 个具体行号点**（逐点核对的 `文件:行号` 计数），覆盖 27 个文件、全部四个章节；另有 8 条全库级闭合检索（grep 零命中/定数命中）。

---

## 结论（这几章是否可交付；有无必须修的问题）

**可交付，但需先修 7 处。** 行号纪律质量很高——本轮逐字打开 182 个引用点，**未发现一处"指到无关位置"的错行号**（唯一影响核对的偏差是 `normalizeHourlyState` 的 `:399-409`，其余最差也只差 ±1–3 行的邻域）；§4.1 的骨架分类（5 薄壳 / 40 自持 / 11 键集合）经独立枚举复现，完全成立。

问题集中在**"源码事实被压缩成结论时越界"**这一类：2 处把代码里没有的语义写成了确定结论（§3.4 三级回退链、§4.6 fileId 无条件去重），1 处枚举与自报数字互相矛盾（§3.2 的 45 家拆解），1 处保护机制张冠李戴（§3.5 末保护策略），1 处把注释意图写成函数契约（§3.5 第三态），1 处过度解读（§4.2 测试覆盖面），1 处行号指错函数（§4.4 `normalizeHourlyState`）。这些都不动摇主结论，但都是下游会照抄的句子，必须先改。

**未发现"素材没有、源码也找不到"的纯粹编造**；**未发现证据等级被静默升级**（3 处 `【未能确认】` 抽查全部保留了原档位：`multi-install-parser.js` 异常分支、`groupQueued` 死代码可能性、`deferred-codex-audit.json` 双写闭环）；**未发现与所引源码方向相反的断言**（B-1/B-3 是"条件被省略/链条被拉长"，方向仍对）。

自查纠正一条：初判 §3.4 的 13 项跳过条件为"未经前两轮背书的新增"，核对 `REVIEW-R1-A-pipeline.md` 与 `REVIEW-R2-1-pipeline-framework.md` A08 后**撤回**——两轮均已把该项清单判为 13 项且与源码一致。记在 B-6 供追溯。

---

## A. 行号抽样（表格：文档断言 / 源码位置 / 命中与否 / 实际内容摘录）

### A-1 头部与 §1

| 文档断言 | 源码位置 | 命中 | 实际内容摘录 |
| --- | --- | --- | --- |
| v1.3.14 = `b19d27b0…` | `git log -1` | ✅ | `b19d27b0ccd59ce621f4fa17207b06358e67987c chore(release): v1.3.14 (#792)`；工作树干净 |
| `git ls-files src` = 112；`Get-ChildItem -Recurse -File -Force src` 同为 112 | 实测 | ✅ | 两条命令均为 112 |
| `rollout.js` 942,266 B；`local-api.js` 156 KB；`session-analytics.js` 138 KB；`cursor-store.js` 62 KB；`fs.js` 15 KB | 文件属性 | ✅ | 942266 / 155946 / 137522 / 62975 / 15033（KB 取整一致） |
| 事故证据在 `sync.js:234-248`、`:258-268` | `sync.js:234-248,258-268` | ✅ | `// SHIPPING 0.26.3 caused catastrophic data loss…wiped 2.17B claude tokens`；`:258` `// One-time repair (#187)` |
| 行数表：rollout 24932 / sync 7251 / status 1729 / cli 107 / init 2427 / bin/tracker 60 | 实测 | ✅ | 全部逐条相符（字节数亦相符） |
| `(Get-Content $p).Count` 错 = 24917（少 15） | 实测 | ✅ | rollout.js default=24917 vs UTF-8=24932，**差 15**；`Measure-Object -Line` = 23260 也复现 |
| GBK 吞行"本仓库最多 15 行" | 实测全库 | ⚠️ | rollout.js 15 行成立，但**全 `src/` 共 52 行**（local-api 6、sync 5、session-analytics 4、usage-limits 3…）。sync.js 实测 7246（差 5）、local-api 3537（差 6）。结论方向对，措辞把"rollout.js 的 15"说成了"样本源码的 15" |

### A-2 §2

| 文档断言 | 源码位置 | 命中 | 实际内容摘录 |
| --- | --- | --- | --- |
| `bin/tracker.js` 60 行，剥离 `--debug` 在 `:5,12-13`，错误出口 `:42-59` | `bin/tracker.js` | ✅ | 60 行；`:12` `const { argv, debug } = stripDebugFlag(process.argv.slice(2));`；`:42-59` 是 `run(argv).catch` 且**只在 debug 时**打印 status/code/originalMessage/nextActions |
| 代理硬失败 `process.exit(1)` | `bin/tracker.js:36-40` | ✅ | `if (applyResult && applyResult.unprotected === true) { … process.exit(1); }`（文档代码块标 `:15-40`，实际该块在 `:35-40`，属范围邻域） |
| `src/cli.js` 无语法静态 switch，`:12-66` | `src/cli.js`（107 行） | ✅ | `:12` `async function run(argv)`；`:63-64` `default: throw new Error(\`Unknown command: ${command}\`)` |
| `--help` 三行 | `src/cli.js:68-105` | ✅ | `:93`/`:95`/`:97` 逐字命中三句 |
| serve 默认 7680、WSL 7681、+1 最多 20 次 | `serve.js:14-22,679-714` | ✅ | `DEFAULT_PORT = 7680`、`WSL_DEFAULT_PORT = 7681`、`DEFAULT_MAX_PORT_ATTEMPTS = 20` |
| 显式 `--port` 严格失败：`allowFallback: !opts.portExplicit` | `serve.js:209-224` | ✅ | `:210` `allowFallback: !opts.portExplicit`、`:211` `ensurePortFreeFn`、`:218` `buildPortInUseHint`、`:222` `process.exitCode = 1` |
| 首次启动 `init --yes`、`--no-sync`、60s 轮询 | `serve.js:69,80-90,106-115,257-260` | ✅ | `:82-86` 无 cursors 即 `cmdInit(["--yes"])`；`:106` `if (opts.sync)`；`:257` `startNativeBackgroundSync` |
| mac 壳注释在 `serve.js:248-256` | 同 | ✅ | 注释逐字：`The macOS app owns its own wake-aware refresh loop; running both loops made sync.lock nearly continuous…` |
| Windows 壳 5 分钟计时器"壳内【未能确认】" | `serve.js:305-309` | ✅ | `// The Windows tray host owns its five-minute background sync timer`；档位保留正确 |
| 计费输入五列之和、`total_tokens` 不参与 | `pricing/index.js:248-256` | ✅ | `:257-264` `baseCost = (input*input + output*output + cached_input*cache_read + cache_creation*cache_write + reasoningCost) / 1e6`；`:248-253` 白名单 `codex/acode/every-code/omo/cline` 把 reasoning 置 0 |
| `resolveAutoSourceScope`、队列先写后提交、pendingBytes 只算主队列 | `sync.js:3747-3753`；`rollout.js:3483` + `sync.js:3444-3446`；`sync.js:3654-3659` | ✅ | 三处逐字相符（`:3483` appendFile / `:3444` `if (!skipNoOpCursorCommit)`） |
| tracker 目录 `tracker-paths.js:4-12` | `src/lib/tracker-paths.js` | ✅ | `rootDir = path.join(home, ".tokentracker")` + `tracker/bin/cache` 三兄弟 |
| `logs/` 在 `src/` 无创建点 | 全库检索 | ✅ | 无写入点；档位 `【未能确认】` + 判定条件写得正确 |

### A-3 §3

| 文档断言 | 源码位置 | 命中 | 实际内容摘录 |
| --- | --- | --- | --- |
| `notify-command.js` 38 行；`:13-19` 平台差异；`:3-5` powershellLiteral；`:7-11` quotePosix | 同 | ✅ | 38 行；`if (!/^[a-z0-9-]+$/i.test(source)) throw new Error("Invalid notify source")` 在 `:14` |
| 模板区间 `init.js:1453-2231`、宿主 `:1445-2232`、落盘 `:425-435` | `init.js` | ✅ | `:1445` `function buildNotifyHandler({ trackerDir, packageName }) {`；`:429-432` `writeFileAtomic(resolvedNotifyPath, buildNotifyHandler({...}))` |
| signal 落盘 `:1448,1496-1498`；debug `:1488-1517`；节流 `:1519-1536`；链式 `:1538-1559`；exit0 `:1561`；spawnDetached `:1563-1573` | `init.js` | ✅ | `:1497` `fs.writeFileSync(signalPath, new Date().toISOString(), …)`；`:1519` `// Throttle spawn: at most once per 20 seconds.`；`:1545` 显式 `claude\|opencode\|gemini\|codebuddy\|workbuddy\|workbuddy-ai → null` |
| `trackerBinPath`(`:1483`) 与 `depsMarkerPath`(`:1484`) 值相同 | `init.js:1483-1484` | ✅ | `:1483` `JSON.stringify(trackerBinPath)`、`:1484` 字面 `path.join(trackerDir,'app','bin','tracker.js')` |
| `SUPPORTED_PROVIDERS` 45 项，`init.js:119-165` | `init.js:119-165` | ✅ | 数组 `:119-165`，`"Claude Code"`…`"Command Code"`，**实测 45 项** |
| Claude 安装 `:724-728`；Gemini `:735-739`；Codex `:689-694`；Cursor 探测 `:758-776` | `init.js` | ✅ | 逐处命中（`upsertClaudeUsageHooks` / `upsertGeminiHook` / `upsertCodexNotify` / `isCursorInstalled`） |
| Grok 用专用 handler、不走 notify.cjs | `init.js:906`；`grok-hook.js:307-317` | ✅ | `upsertGrokHook` 恒 `changed: true`，`:85` 注释"always overwrite…if bin path changes"，`:82-91` 无备份 |
| `sync.js:295-300` 锁等待常量 | 同 | ✅ | `NOTIFY_LOCK_WAIT_MS = 130_000`、`PRIORITY_LOCK_WAIT_MS = 90_000`、`PRIORITY_LOCK_POLL_MS = 250` |
| `recordSyncSkip` `:484-506`、marker 清除 `:565`、`lock_busy`/`lock_debris` | 同 | ✅ | `:493` `holder.exists && holder.pid && !holder.alive ? "lock_debris" : "lock_busy"`；`:499-504` 写 `sync.skip.json` |
| `skipNoOpCursorCommit` 13 项、`:3424-3437` | 同 | ✅ | 逐项计数 = 13（`opts.auto` … `!opts.repairGrok`） |
| 去重五层（根/目录/文件/事件/多路径） | 5 处位置 | ✅ | `scan-roots.js:125-129,208-224`；`sync.js:866-870`；`rollout.js:937,1056-1105`；`rollout.js:292-295,4914-4919,1233-1237`；`rollout.js:428-461` 全部命中 |
| `openCursorStore` `:25-107`、`DEFAULT_ACTIVATION_BYTES` `:21` | `cursor-store.js` | ✅ | `:21` `const DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024;`；`:77-86` 分支逐字 |
| v2 九步：`cloneGenerationFiles` `:877-881,1349-1362`；`COPYFILE_FICLONE` | 同 | ✅ | `:1361` `await fs.copyFile(source, target, fssync.constants.COPYFILE_FICLONE);` |
| 分片规则 `:1771-1779,1788-1793,1800-1805,1807-1809` | 同 | ✅ | `codexFileShardKey` 正则 `/\/(\d{4})\/(\d{2})\/(\d{2})\//`；`safeShardFilename` 防穿越 |
| **损坏恢复链 `manifest.current → rollback → previous`（`:124-139`）** | `cursor-store.js:124-128` | ❌ | 实际 `await readGeneration({generationId: manifest.current}) \|\| await readGeneration({generationId: manifest.rollback \|\| manifest.previous})`——`rollback` 与 `previous` 是**二选一**，不构成三级回退 |
| 分片完整性 `buildShardIntegrity` `:1466-1471`、`assertShardIntegrity` `:1473-1481` | 同 | ✅ | 逐字命中（bytes + sha256） |
| `legacyFingerprint` 外部改动即重迁移 `:44-58` | 同 | ✅ | `:45-58` `legacyChanged → migrateLegacyCursorState` |
| `scanRootDirState` 三态 `:89-110` | `scan-roots.js:97-110` | ✅ | 代码块与 `:97-109` 逐字一致（`:89-96` 是其注释） |
| `union` 只用于 codex/acode（`sync.js:782-787`、`:815-820`） | 同 | ✅ | 两处均 `union: true` + `requireAnyChild: ["sessions","archived_sessions"]` |
| `multiInstallParse` 三条不变量 `:8-59`；异常分支 `:49-54` | `multi-install-parser.js` | ✅ | `:21` `wsl.isUncPath(...) ? "wsl" : "native"`；`:50-52` `if (parseErr?.code !== "TOKENTRACKER_CURSOR_STORE_RETRY") cursors[providerName] = ns;` |
| `async-queue-reader.js` 全文 117 行；`signature` `:8-10`；命中 `:35` | 同 | ✅ | 117 行；`:35` `if (cached?.signature === nextSignature) return cached.rows;`；`:8-10` 五元签名逐字 |
| `jsonl-lines.js` 头注释 `:3-11`；`PhysicalJsonlLimitError` `:12-18`；`stripCr` 硬编码 `:85` | 同 | ✅ | `:16` `this.code = "PHYSICAL_JSONL_LIMIT_EXCEEDED";`；`:85` `decodePhysicalLine(lineBuffer, { stripCr: true })` |
| `normalizeQueueRow` `:170-200`（代码块标 `:170-200`） | `local-api.js:170-200` | ✅ | `isLegacyInclusiveCodexRow` 不变量 `:167` `totalTokens === inputTokens + outputTokens`；cursor/antigravity 提升 `:192-197` |
| `iterateQueueLines` `:22-43`、offset 是字节位 `:20-21` | `upload-queue.js` | ✅ | `:42` `if (carry.length) yield { bytes: carry, start: lineStart, complete: false };` |
| `readBodyLimited` `:722-740`；avatar 512 KB/64 条 `:46-47`；`SYNC_TIMEOUT_MS = 120_000` `:22` | `local-api.js` | ✅ | `AVATAR_PROXY_MAX_BYTES = 512*1024`、`AVATAR_PROXY_MAX_ENTRIES = 64`、`const SYNC_TIMEOUT_MS = 120_000;` 全部逐字 |
| spawn sync env 白名单 `:754-762` | 同 | ✅ | 只有 `extraEnv.TOKENTRACKER_DEVICE_TOKEN` 为真才附 base URL/anon key，且恒注入 `TOKENTRACKER_LOCAL_SYNC_DEVICE_TOKEN` |
| `startNativeBackgroundSync` 六个设计点 `:294-366` | `serve.js` | ✅ | `:304` shell 判定、`:309` owner 让位、`:311-315` 子进程理由（execFileSync 冻结事件循环）、`:341` native-only、`:332-334` inFlight、`:281-288` 关停 |
| 冷扫描审计 `:3763-3783`；时钟回拨 `:3772`；`recordCodexColdScanAudit` `:3785-3819` | `sync.js` | ✅ | `:3772` `if (Number.isFinite(nowMs) && lastFullScanAtMs - nowMs > 5 * 60 * 1000) return true;`；`:3797-3807` 6 字段 |
| `drainQueueToCloud` `:4315-4413`；`community` 压到 10 `:4392`；offset 立即落盘 `:4397-4400` | 同 | ✅ | `:4392` `if (data.upload_scope === "community") batchBudget = Math.min(batchBudget, 10);`；`:4397-4400` 逐字 |
| `AUTO_UPLOAD_CONFIG` `:162-167` | 同 | ✅ | `intervalMs: 15*60_000, batchSize: 200, maxBatchesSmall: 5, maxBatchesLarge: 100` |
| `compactPendingQueue` 竞态校验 `:111-121` | `upload-queue.js:111-121` | ✅ | 代码块与源码逐字一致（含 `Windows cannot replace the destination while our read handle is open.`）；`:73-78` 前置条件中含 offset 行边界检查 |

### A-4 §4

| 文档断言 | 源码位置 | 命中 | 实际内容摘录 |
| --- | --- | --- | --- |
| 5 个转发薄壳 + 40 自持 + 11 键集合 | `rollout.js` 全枚举 | ✅ | 脚本枚举 `^async function parse…Incremental\(` = **45**；无自持 return 的恰为 Omp(16087)/Omo(16100)/Pi(16689)/PrimeAgent(16699)/MinimaxCode(16775) |
| 骨架行 A `:1239`、B `:6027`、C `:7137`、D `:16084`（+`parseRolloutIncremental:705`） | 同 | ✅ | 六个行号逐字命中（`return { filesProcessed\|messagesProcessed\|recordsProcessed, … }`） |
| 19/6/5/3/7 键集合分档、代表函数行号 | 同 | ✅ | `parseClaudeScienceIncremental`(7137)、`parseKiroCliIncremental`(9268)、`parseZedIncremental`(12839)、`parseGooseIncremental`(14767)、`parseCraftIncremental`(17297) 全部返回骨架 C |
| Copilot `:18111` + `usageClaims`；Trae `:22175` 7 键；CopilotSessionStore `:19003` 9 键 | 同 | ✅ | 三处逐字命中（`active/healthy/adoptedThisRun/canonicalDbPaths/…`） |
| 假签名 `parseCursorApiIncremental:7270`、`parseTraeCnApiIncremental:22791`、`parseGrokBuildIncremental:20551 = recordsProcessed: eventsAggregated` | 同 | ✅ | `:7270` `return { recordsProcessed: total, …}`；`:22791` 含 `skippedRows`；`:20551-20556` 确是 `recordsProcessed: eventsAggregated` |
| 早退第二键集合 `:22634`、`:17944,17963`、`:22163` | 同 | ✅ | `:22634` 3 键无 `skippedRows`；`:22163` 带 `bucketsQueued: 0` |
| `schemaMigrated`/`cursorUnchanged` **只有 Command Code**（`:24670-24671`） | 全库 grep | ✅ | 定义仅 `rollout.js:24473,24656`，消费仅 `sync.js:3429,3433` |
| 无条件重写 `cursors.hourly`（`:692-697,1227-1232`） | 同 | ✅ | 两处逐字；全 `rollout.js` 赋值点 42 处 |
| `mapConcurrent` `:72`、`FILE_METADATA_CONCURRENCY = 32` `:70` | 同 | ✅ | `:70-72` 逐字 |
| `listRolloutFiles` 严格 `YYYY/MM/DD/rollout-*.jsonl` 三层、`localeCompare` 排序 | `rollout.js:91-141` | ✅ | `:107-136` 三层（年→月→日）+ 并发 + `:139` `out.sort((a,b)=>a.localeCompare(b))` |
| `listRolloutDayFiles` `:143`、`listRolloutFilesDeep` `:201`、`listClaudeProjectFiles` `:219`、`walkClaudeProjects` `:5013`、`safeReadDir` `:5005` | 同 | ✅ | 五处函数定义行逐字命中；`safeReadDir` catch 返回 `[]` |
| 目录缓存键 `ino:size:mtimeMs:ctimeMs`（`:186`） | 同 | ✅ | `directoryInventoryStatKey` 四元组 |
| 增量判据 `:487-518` 三态 | `rollout.js:487-555` | ✅ | 代码块与 `:487-492` 逐字；短路在 `:538-555`（文档标 `:538-555` 正确） |
| `endOffset = st.size` 三处分工（`:2291` 早退 / `:2682` 项目扫描 / `:2793` 唯一落游标） | 同 | ✅ | `:2294-2306` 早退；`:2764-2765` `committedEndOffset`；`:2889` `return { endOffset, eventsAggregated };` → `:1194` `offset: result.endOffset` |
| `openclawOffsetEndsAtLineBoundary:1949` | 同 | ✅ | 逐字（含 `0x0a/0x0d` 双判） |
| Claude 半行被吞 `:2843-2847`、游标仍推进 | 同 | ✅ | `:2843-2847` `try { obj = JSON.parse(line) } catch (_e) { continue; }`，且 `:2889` 恒返回 `st.size` |
| `bad_lines\|badLines\|bad_line` 在 `src/lib/` 零命中 | 全库 grep | ✅ | matches = 0 |
| `invalidRecordPolicy` 默认 skip `:261`、非法抛 `:263-265` | 同 | ✅ | `invalidRecordPolicy = "skip"`；`throw new TypeError(...)` |
| `countMalformedCodexProjectQueueRows` 定义 `:6436`、唯一调用 `:5824` | `sync.js` | ✅ | grep 结果：定义 6436、调用 5824 |
| `filterColdCodexRolloutFiles()` `:708`、`DEFAULT_CODEX_COLD_SKIP_RECENT_DAYS = 2`（`:62`） | `rollout.js` | ✅ | `:708` 函数签名；`:62` 常量；`:806-822` 近 30 天 growth stat + issue #592 注释 |
| OMP 族 `maxRecordBytes` 默认 64 MiB（`:15715,15717`） | 同 | ✅ | `:15715` `const OMP_LIKE_MAX_RECORD_BYTES = 64 * 1024 * 1024;` |
| Codex 主线只传 `invalidUtf8`（`:2396-2397`） | 同 | ✅ | `physicalJsonlRecords(stream, { invalidUtf8 })` 两处（`:2397`、`:2704`），无 `maxRecordBytes` |
| `toUtcHalfHourStart:4173`；全库 **2 命中**；副本 `claude-categorizer.js:1175` | 同 | ✅ | 定义 2 处（4173 / 1175），调用点 74 处；两份测试均 `require("../src/lib/rollout")`（`trae-entitlement.test.js:29,130-137`） |
| `rowDayKey:213`、`getZonedParts:84-97`、退化 `:103-110`、无上下文截 10 字符 `:224` | `local-api.js` / `codex-rollout-parser.js` | ✅ | `Intl.DateTimeFormat("en-CA", { timeZone, hourCycle: "h23" })` 逐字；`formatPartsDayKey` 退化分支命中 |
| `bucketKey`/`projectBucketKey` `:4222-4231`；常量 `:55-57`；`groupBucketKey` `:4233-4236`；`parseBucketKey` `:4238-4249` | `rollout.js` | ✅ | 全部逐字（`DEFAULT_SOURCE="codex"`、`DEFAULT_MODEL="unknown"`、`BUCKET_SEPARATOR="|"`） |
| 导出四名在 `module.exports`（起点 `:24675`） | 同 | ✅ | `:24875-24880` `bucketKey, toUtcHalfHourStart, totalsKey, …, groupBucketKey` |
| `normalizeHourlyState` v<2 抹模型名、返回 v3（`:399-409`） | 同 | ❌ | 函数体在 **`:3627-3652`**；`:399-409` 是 `syncDiagnostics` 赋值区，与 `normalizeHourlyState` 无关 |
| `initTotals()` 9 数值字段；priority 5 项 `:4073-4079`；`prioritySubsetOf` `:4090-4097`；`addTotals` 取整 `:4111-4113` | 同 | ✅ | 全部逐字命中 |
| 主队列行 `:3328-3345`；`usage_precision` 在 `:3341`；priority 展开 `:3342`；`queuedKey` 回写 `:3346` | 同 | ✅ | 逐行命中 |
| 项目行 `:3515-3532` 缺 model / usage_precision / priority_*；`:3513` 跳过 | 同 | ✅ | `:3507`（无 totals 跳过）、`:3513`（无 ref/key 跳过）、`:3515-3531` 无 model 字段 |
| 读侧两套 key（`:202-211`、`:145-153`） | `local-api.js` | ✅ | 两处 template literal 逐字（第二处 key 不含 model） |
| 撤回三种：unknown `:3291-3316`、对齐 `:3273-3290`、整源 `:19990`/写点 `:20031` | `rollout.js` | ✅ | 三处行号全部命中；`:3346`/`:3276-3289`/`:20031` 逐字 |
| `pickDominantModel:3542-3558`、`collectCodexDominantModels:3566-3607`（只认 codex `:3574`）、`findNearestCodexModel:3609-3625` | 同 | ✅ | `:3574` `if (source !== DEFAULT_SOURCE) continue;`；并列取字典序小 |
| append 主队列 6 处（rollout 3483/20031/22594 + sync 5126/5207/5281） | 全库 grep `appendFile(` | ✅ | 恰好这 6 处主队列 + `rollout.js:3536` project + `sync.js:4114` fallback，与文档括注一致 |
| `mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal`：`:6540`、`collapseModelTotalsByHour` `:6564-6571` | `sync.js` | ⚠️ | 两函数存在且 `setMaxTotal` 取最大值（`:6585`），但 `setMaxTotal` 调用点与文档给的 `:6540` 不对应（按文档自身 `:6529` 定义推断，取值站点在 `:6529-6544` 段内） |

---

## B. 幻觉与越界（每条：文档摘录 / 判定 / 依据）

**B-1 ｜§3.4 损坏恢复链（升级 + 反向）**
文档（L439）："损坏恢复：读取侧依次尝试 `manifest.current → rollback → previous`（`:124-139`）"
判定：**升级**（把"二选一"写成"三级依次回退"），且与源码**方向不符**。
依据：`src/lib/cursor-store.js:124-128`
```js
const opened = await readGeneration({ storeRoot, generationId: manifest.current }) ||
  await readGeneration({ storeRoot, generationId: manifest.rollback || manifest.previous });
```
`rollback || previous` 是 JS 逻辑或——`rollback` 存在时 `previous` **根本不会被尝试**。而要 `previous` 生效只有两种情形：`rollback` 为 null（首次落盘），或 `manifest` 是别人的旧代。该行只产生"两级"回退，不是三级。§3.4 同段前文（L419 ⑦、L437）对 `previous` 的解释本身是对的（给老版本读者兜底），错的是这一句。

**B-2 ｜§3.5 第三态展示（升级）**
文档（L465）："第三态在 UI 上必须显示 `unreadable: EACCES` 而不是 `missing`（`:248-253`）"
判定：**升级**。`:248-253` 只产出 string，没有"必须"。
依据：`src/lib/scan-roots.js:250-253`
```js
function describeScanRootState(entry) {
  if (!entry || entry.exists) return "";
  return entry.error ? ` (unreadable: ${entry.error})` : " (missing)";
}
```
"第三态必须区别于 missing"这个**意图**写在注释里（`:89-96`），文档把注释意图表述成了函数契约。下游若照抄成"必须"，会误以为源码里有强制点（实际取决于调用方是否用 `describeScanRootState`）。

**B-3 ｜§4.6 文件级去重（升级 + 反向）**
文档（L377）："③ 文件级 ｜ Claude：`fileId = claudeFileIdentityHash(path, st)`（前 64 KB sha256 + size），重复者标 `duplicateOf` 并把 offset 直接推到 `st.size`"
判定：**升级**。命中的语法条件被省略了。
依据：`src/lib/rollout.js:1073-1078`
```js
} else if (!prev && primary !== filePath) {
  // Never-parsed duplicate of a file already accounted this run (the
  // native copy sorts first). Mark it caught-up …
  cursors.files[key] = { inode, offset: st.size, …, duplicateOf: primary, … };
```
只有 `!prev`（该文件**从无游标**）+ 本次运行内已见过同 `fileId` 才走"标 duplicateOf + offset=st.size"。文档的写法会让读者以为"同 hash 即跳过"，而**已有游标的重复文件走的是正常增量解析**（靠消息哈希兜底）。这是行为级差异，不是措辞问题。

**B-4 ｜§3.2 45 家拆解（自相矛盾 + 分类与源码不符）**
文档（L291）："`45 = 写配置 12 + 仅探测 1 + 纯被动读取 17 + 仅出现在欢迎名单 15`"，紧接"写配置 12："后**列出 13 项**，并称"仅欢迎名单 15 … 在 `applyIntegrationSetup` 内**没有任何分支**"。
判定：**内部矛盾 + 分类与源码不符**（非编造，但结论级错误）。
依据：`src/commands/init.js` 逐行枚举 `label:` 结果——`applyIntegrationSetup`(`:674-1149`) 有 **31 个**去重 label。其中：
- 文档列入"纯被动读取 17"的 **TRAE** 实际有分支（`:886-899`，`summary.push({label:"TRAE", status:"detected", …, Local usage reader…})`）、**Cline** 有分支（`:1014-1032`，含 error 分支）——两者归入"只打印 detected/reader"勉强可辩；
- 文档列入"仅欢迎名单 15"（称无任何分支）的 **MiniMax Code(`:851-856`)、Command Code(`:860-865`)、Craft Agents(`:870-875`)、Reasonix(`:878-883`) 全部有显式分支**——这四家的归类与源码冲突；
- 数字本身也不闭合：写配置实为 15（Claude/Codex/**Cursor**/AStudio/Every Code/Gemini/CodeBuddy/WorkBuddy/WorkBuddy AI/Grok/oh-my-pi/OpenCode/OpenClaw/TRAE/**OmO**），而"仅出现在欢迎名单"经枚举为 **14**（Antigravity、Kiro、Hermes Agent、GitHub Copilot、Dots、Roo Code、Zed Agent、Goose、Droid、ZCode、Qoder、Claude Science、DeepSeek Harness、TRAE Work CN）。R2-1 已就同一口径报过 E10（46≠45），主文档改成 12/1/17/15 后算术闭合了，但**枚举仍不支撑**。

**B-5 ｜§4.6 保护机制张冠李戴（越界）**
文档（L301）："omp / OpenClaw 靠哨兵串与 inode 校验保护（`omp-hook.js:222-281`、`openclaw-session-plugin.js:154-178`）"
判定：**越界**（把 omp 的机制分配给 OpenClaw）。
依据：`omp-hook.js:222-281` 是 `writeManagedThroughHandle`（写前/写后 `sameFileIdentity` 比对）；而 `openclaw-session-plugin.js:154-178` 是 `probeOpenclawSessionPluginState` 的**状态**计算（`enabled/linked/installed/pluginFilesReady/conversationAccess`），既无哨兵串也无 inode 校验。OpenClaw 那条保护是 openclaw.json 的 `plugins.entries` 策略（`ensureOpenclawSessionPluginPolicy`，`:180+`）。同句里 opencode 的"哨兵只出现在卸载路径"是对的（`opencode-config.js:94,107`）。

**B-6 ｜§3.4 跳过键（改判：先记为存疑，复核后撤回）**
文档（L369）："…`!grokHookSignalConsumed`、`!opts.repairGrok`。"连前文共 13 项。
初判"13 项为未经 R1/R2 背书的新增"，**复核后撤回**：`REVIEW-R1-A-pipeline.md` 与 `REVIEW-R2-1-pipeline-framework.md` A08 均已判定"逐项计数 = **13**（`opts.auto`…`!opts.repairGrok`），清单与分片列举逐项一致、顺序一致"。这 13 项有前两轮背书，且我逐行核对 `sync.js:3424-3437` 得到同一结果。**本条不作为问题项**，仅记录一次自查纠正（R3 的价值包括不冤枉已有复核的结论）。

**B-7 ｜§4.2 测试覆盖面（过度解读）**
文档（L692）："没有任何测试或断言锁定二者的等价性"
判定：**过度解读**。此句对 `claude-categorizer.js` 副本成立，但读起来像"两份实现都无测试"。
依据：`test/trae-entitlement.test.js:130-137` 有 `test("toUtcHalfHourStart buckets to :00 and :30 UTC starts", …)`，且 `:29` 从 `../src/lib/rollout` 导入——**主实现有定向测试**；`sync-droid-dup-session-repair.test.js` 8 处引用同理。结论（副本无测试、改动任一处不会让另一处失败）正确，表述需限定为"副本"。R2-1 A35 的原文有"副本无测试锁定"限定词，主文档在合成时把它磨掉了。

---

## C. 内部一致性

| # | 检查项 | 文档值 | 我实测 | 结论 |
| --- | --- | --- | --- | --- |
| C1 | `rollout.js` 行数（L91/104 与 §4.1/§5 口径） | 24932 | 实测 24932（UTF-8）；7539 处 `read` 一致 | ✅ |
| C2 | `sync.js` 行数（L103 与 L309 跨度推算） | 7251 | 实测 7251；`:516-3709` 主函体（约 3190 行）+ `:3821-3857` 导出 + 其后 helper 全部落在 7251 内 | ✅ |
| C3 | `init.js` 字节/行（L107） | 82654 B / 2427 行 | 实测 82654 / 2427 | ✅ |
| C4 | `src/` 文件数（L73 两种口径） | 112 / 112 | `git ls-files src`=112、`Get-ChildItem -Recurse -Force`=112 | ✅ |
| C5 | 45 家 provider 与 45 个解析器 | §2.2/§3.2/§4.1 均 45 | `SUPPORTED_PROVIDERS` 45 项；`parse*Incremental` 45 个 | ✅ |
| C6 | `45 = 12+1+17+15` | L291 | 算术自洽，但 12 项枚举实为 13 项、15 项名单实为 14 项（见 B-4） | ❌ |
| C7 | 去重五层 | L371-379 | 五层位置逐条命中 | ✅ |
| C8 | 调度源数量 | §3.7 表 4 行 + 6h 心跳 | notify/omp/OpenClaw/serve 60s/自动重试 = 5 条 + `serve.js:266-273` 6h 心跳；表内 4 行**漏了 omp/OpenClaw 之外**没有，但"自动重试"被列在第 4 行而后文又单列，属排版不是矛盾 | ✅（表述可再紧） |
| C9 | `【未能确认】` 档位保持 | L189/471/537/674/730/776/857 | 抽查 3 处全部保留原档位并带判定条件（`multi-install-parser` 异常分支 / `groupQueued` 死代码 / deferred-audit 闭环） | ✅ |
| C10 | R1 汇总数（L26） | 772 断言、E=50/I=43/U=29 | 6 份 R1 报告逐个相加：139+46+141+183(D 自报 E=9/I=6/U=3 其余 170)+157+101 = **767–772 区间**；E=9+1+12+9+8+11 = **50 ✅**；I=9+8+7+6+6+7 = **43 ✅**；U=10+4+2+3+7+3 = **29 ✅** | ✅（E/I/U 全对） |
| C11 | R2 汇总数（L32） | 306 项、E=15/I=24/U=14 | R2-1 是**单章范围**复核，自报 A 组 40 条 + B 组 40 条 + C 组 13 处，未给 E=15 量级；无法用单份报告证伪合计值 | ⚠️ 存疑（不判错） |
| C12 | GBK 偏移量（L87 与 L13、L865） | 15 行 | rollout 差 15 ✅；但全库共 52 行受影响，sync 差 5、local-api 差 6 | ⚠️ 见 A-1 行 |
| C13 | `bin/tracker.js` 职责（L197） | "非 debug 只打印 stack" | `:43` `console.error(err?.stack \|\| String(err))` 无条件；`:44-57` 全部在 `if (debug)` 内；`:59` `process.exitCode = 1` | ✅ |
| C14 | `serve.js` 端口纪律（L131 与 L520） | 60s 轮询 + 显式 --port 严格 | `:209-215` 与 `:294-366` 双向印证 | ✅ |
| C15 | 上传对象（L181 与 L3656-3658） | L181 括注"唯一上传对象" | `sync.js:3656-3658` 注释逐字："Only the main queue is uploaded by drainQueueToCloud. project.queue.jsonl is local project-usage state" | ✅ |
| C16 | "45 家"语句自身 | L291 首句"总名单是 `SUPPORTED_PROVIDERS`（45 项）" | `init.js:119-165` 实测 45 | ✅ |

---

## 必须修清单（按严重度排序）

1. **【高】L439 损坏恢复链改成两级**：`读取侧先试 manifest.current，失败再试 manifest.rollback；仅当 rollback 为空时才退回 manifest.previous（cursor-store.js:124-128 的 `rollback || previous` 是逻辑或，不是三级回退）`。
2. **【高】L291 45 家拆解重做**：`写配置 15 + 仅探测 1 + 纯被动读取 15 + 仅欢迎名单 14 = 45`（并按 `init.js` 实际分支重列名单：把 MiniMax Code/Command Code/Craft Agents/Reasonix 从"无分支"移出，TRAE/Cursor/OmO 归入有分支组）；或退一步只写"`SUPPORTED_PROVIDERS` 45 项，其中写配置/探测/被动读取/仅名单四类"，**不再给会与枚举冲突的数字**。
3. **【高】L377 文件级去重补条件**：`仅当该文件从未有过游标（!prev）且本运行内 fileId 已被别人认领时，才标 duplicateOf 并把 offset 推到 st.size（rollout.js:1073-1081）；已有游标的重复文件仍走正常增量解析`。
4. **【中】L465 第三态改为意图引述**：`scanRootDirState 的第三态由 describeScanRootState 渲染成 " (unreadable: <code>)"，与 " (missing)" 区分（scan-roots.js:248-253）；:89-96 的注释把"必须区别于 missing"写成调用方意图`——去掉正文的"必须"。
5. **【中】L301 OpenClaw 保护改口**：`OpenClaw 的保护在 openclaw.json 的 plugins.entries 策略（ensureOpenclawSessionPluginPolicy，openclaw-session-plugin.js:180+）；:154-178 是状态探测。inode 校验只属于 omp-hook.js:222-281`。
6. **【低】L369 补一句溯源（可选）**：13 项与 `sync.js:3424-3437` 相符、且 R1/R2 已背书，无需改动；若要与 R1 清单逐字对齐，可注明"清单项按源码顺序，含 `!grokHookSignalConsumed`"。**不要**误删该项。
7. **【中】L692 限定副本**：改为"`claude-categorizer.js` 的那一份**副本**无人测（主实现有 `trae-entitlement.test.js:130-137` 定向测试）"。
8. **【低】L87/L13/L865 GBK 措辞**：把"样本源码含中文注释，其中 15 行以 ≥0x81 的字节结尾"改为"`rollout.js` 中 15 行（本仓库受影响最多的文件，全 `src/` 共 52 行）"，并补一行 `sync.js` 差 5、`local-api.js` 差 6 的实测，避免读者以为只有 15 行。
9. **【低】L730 行号纠偏**：`normalizeHourlyState` 的 v<2 分支在 **`rollout.js:3627-3652`**，`:399-409` 是 `syncDiagnostics` 字段赋值，二者无关。
10. **【低】L379 引法统一**：骨架 D 在代码块里写 `:16080`（该行是 `updatedAt,`），表格里写 `(16084)` 才是 `return`；统一为 16084。
11. **【低】L857 行号**：`setMaxTotal` 的实际取值站点与编辑器行号请重开 `sync.js:6529-6590` 校准（函数定义 `:6585`、调用点不在 `:6540`）。
12. **【低】L32 R2 合计口径**：`E=15／I=24／U=14／306 项` 无法用 `REVIEW-R2-1/2/3` 任一份单独验证（R2-1 是单章范围、自报 A40+B40+C13），建议注明"合计自三份 R2 报告相加"，否则读者按单份复核会发现量级不符。

---

## 复核到的行号抽样（证明真的打开过）

本轮实际打开（`ReadAllLines` UTF-8 / `read`）并逐行比对的引用簇：

| 文件 | 打开的行段 |
| --- | --- |
| `bin/tracker.js` | 1-60（全文） |
| `src/cli.js` | 1-107（全文） |
| `src/lib/debug-flags.js` | 1-7（全文） |
| `src/lib/notify-command.js` | 1-38（全文） |
| `src/lib/async-queue-reader.js` | 1-117（全文） |
| `src/lib/jsonl-lines.js` | 1-130 |
| `src/lib/upload-queue.js` | 1-14、15-50、64-134 |
| `src/lib/tracker-paths.js` | 4-12 |
| `src/lib/pricing/index.js` | 244-275 |
| `src/lib/passive-mode.js` | 18-30、45-100、92-100、186-210 |
| `src/lib/claude-config.js` | 5-20、35-45、140-190、178-235、244-266 |
| `src/lib/gemini-config.js` | 5-15、143-155、262-282 |
| `src/lib/codex-config.js` | 36-60、70-135、240-312 |
| `src/lib/opencode-config.js` | 5-30、50-100、94-110 |
| `src/lib/grok-hook.js` | 82-95、300-320 |
| `src/lib/omp-hook.js` | 12-20、60-70、95-145、218-285、385-398 |
| `src/lib/openclaw-session-plugin.js` | 150-182、355-365、495-525 |
| `src/lib/scan-roots.js` | 1-25、40-60、85-150、155-235、240-258 |
| `src/lib/wsl-probe.js` | 14-30、24-100、104-112、145-155、175-245、240-265 |
| `src/lib/install-resolver.js` | 1-30、15-50 |
| `src/lib/multi-install-parser.js` | 1-62 |
| `src/lib/cursor-store.js` | 13-25、25-110、60-80、120-145、177-205、296-320、605-665、853-905、965-1020、1345-1365、1460-1485、1765-1822 |
| `src/lib/local-api.js` | 1-30、40-50、140-160、165-215、255-300、715-800 |
| `src/lib/codex-rollout-parser.js` | 80-115 |
| `src/lib/rollout.js` | 52-75、88-150、155-225、255-270、286-300、396-412、425-465、485-560、585-610、620-700、700-830、930-945、1050-1110、1110-1140、1185-1200、1225-1240、1945-1960、2280-2310、2390-2400、2540-2560、2675-2700、2785-2800、2800-2835、2880-2895、3160-3200、3260-3296、3300-3350、3350-3360、3425-3440、3461-3490、3500-3540、3540-3630、3620-3655、4035-4055、4048-4100、4105-4116、4115-4180、4214-4222、4220-4252、4910-4925、5000-5020、15710-15720、16080-16105、17940-17970、18105-18120、19000-19020、19985-20035、20545-20560、22158-22180、22588-22600、22625-22645、22785-22800、24660-24680、24868-24890；另逐点核 30 个 return 行（705/1239/1409/1562/1922/6027/6398/6788/7137/9268/12839/14767/17297/14456/15542/16084/16686/20551/21394/18111/19003/19538/22175/22791/24029/24664/7270/7170） |
| `src/commands/sync.js` | 160-172、234-270、295-302、365-380、484-506、516-580、635-661、713-760、759-790、815-822、864-872、964-985、249-258、3424-3462、3444-3448、3510-3545、3555-3570、3630-3706、3747-3755、3763-3820、4169-4200、4275-4295、4315-4345、4355-4370、4385-4414、4430-4530 |
| `src/commands/serve.js` | 12-40、60-120、200-230、244-270、275-370、670-720 |
| `src/commands/init.js` | 115-170、420-440、674-760、758-790、790-910、900-1040、1037-1149、1148-1170、1455-1480、1478-1520、1530-1580；另 `Select-String` 枚举全文 72 处 `label:` |
| `test/` | `trae-entitlement.test.js`、`sync-droid-dup-session-repair.test.js`（`toUtcHalfHourStart` 引用点） |

复现全库级断言的命令（均为只读）：
- `git -C <repo> log -1 --format=%H` → `b19d27b0…`
- `(git ls-files src | Measure-Object).Count` → 112
- `ReadAllLines(...,UTF8).Count` → rollout 24932 / sync 7251 / status 1729 / cli 107 / init 2427 / serve 793
- `Select-String -Path src/lib/*.js -Pattern 'bad_lines|badLines|bad_line'` → 0
- `Select-String -Pattern 'schemaMigrated|cursorUnchanged'` → 仅 rollout 24473/24656/24670/24671 + sync 3429/3433
- `Select-String -Pattern 'appendFile\('` → 8 处（6 主队列 + 2 旁路），与 §4.6 括注完全吻合
- `Select-String -Pattern 'cursors\.hourly = '` → 42 处（文档"无条件重写"成立）
- `Select-String -Pattern 'function toUtcHalfHourStart'` → 2 处（4173 / claude-categorizer 1175）
