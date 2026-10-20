# 复核报告 R2-1：S1 / S1b / S2（第二轮）

样本：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14，工作树干净）。
口径：全部结论基于本人用 `read` 工具或 `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` 亲自打开的源码区段；未使用裸 `Get-Content`；**未修改样本仓库、未修改任何分片**（发现的问题只报告）。
方法：除逐段读源码外，另在样本仓库之外写了两支只读脚本（`%TEMP%\r2-parsers*.js`）对 `rollout.js` 做全量静态提取，用于独立重跑分片声明的派生数字。
与 R1 的关系：本报告**不复述** R1 结论，只做三件事——复查 R1 修正点是否改对（A）、在 R1 未抽样处独立抽样（B）、跨分片一致性（C）。与 R1 结论不同处已单独标注。

## 结论

**三份分片整体可用，修正记录大体落实**：A 组复查 40 条中 **35 条 ✅ 完全成立、4 条 ⚠️ 部分成立（行号范围或措辞需收紧）、1 条 ❌ 表述需修**；B 组独立抽样 40 条**未发现新的实质性错误**（含 8 条"分片未覆盖但源码如此"的补白）；C 组 13 处比对**未发现互相矛盾的断言**（仅 2 处同一事实的行号范围写法不同，属可接受近似）。

**必须修（E）共 3 条**，均为表述级（不影响机制结论）：E1 是 S2 修正记录里一句会被自身文件证伪的元断言；E2 是 S1b"项目行其余同构"与源码不符（缺两列）；E3 是 S1 §4.4 的 opencode 游标字段清单漏 1 个字段。**没有发现会影响移植决策的机制性错误**——R1 点名的三类高风险项（notify.cjs 模板边界、队列"唯一写点"、OpenCode 哨兵范式）经我独立回源码复核**均已改对**。

## A. 修正点复查（逐条：编号 / 修正后表述 / 结论 / 依据）

### S1（`S1-pipeline-and-collection.md` 修正记录 15 条）

| # | 项 | 修正后表述（摘要） | 结论 | 依据（我打开的位置） |
| --- | --- | --- | --- | --- |
| A01 | E1 §0.1 | `src/` 下 112 个文件（口径：仓库跟踪文件数） | ✅ | `Get-ChildItem -Recurse -File -Force src` 实测 = 112，与 `git ls-files src` 一致 |
| A02 | E2 §2.3 | 模板区间 `init.js:1453-2231`，宿主 `buildNotifyHandler` `:1445-2232`；模板内 `resolveMaybeHome:1575`、`isSelfNotify:1583`、`isSelfNotifyPath:1597`、`shouldChainNotify:1609`、`splitEnvString:1748`、`findNodeLikeScriptIndex:1811` | ✅ | `init.js:1445` `function buildNotifyHandler({ trackerDir, packageName }) {`；`:1453` 反引号起；`:2231` 反引号止；`:2232` 函数闭括号 |
| A03 | E3 §2.8 ③ | 哨兵范式只剩 omp；`unmanaged-extension-present` 全仓唯一产生点 `omp-hook.js:388-395`，消费方 `init.js:813` | ✅ | grep `unmanaged-extension-present` 全 `src/` 仅 2 命中：`omp-hook.js:391`、`init.js:813` |
| A04 | E4 §3.4 | 主队列 append 写点 ≥6 处；另有 `sync.js:4114` fallback、`rollout.js:3536` project 队列 | ✅ | grep `appendFile` 逐一核对：`rollout.js:3483/20031/22594`、`sync.js:5126/5207/5281` = 6 处主队列 + 2 处旁路 |
| A05 | E5 §4.4 | 三套：KiroCli `mtimeMs+lastIndex+lastTotals`（`:9371-9375`、`:9451-9471`）、Gemini（`:1289-1307`）、opencode `inode/size/mtimeMs/lastTotals/messageKey` 无 `lastIndex`（`:1462-1516`） | ⚠️ | 三套归属✅；KiroCli 实际**游标写回**在 `rollout.js:9468-9472`（`:9451-9467` 是桶累加与 onProgress），建议收窄 |
| A06 | E6 §7.1 | 命中在 `async-queue-reader.js:35`，并发目注释 `:12-15`；`:99-104` 是 ENOENT 清理与失败回退 | ✅ | `:35` `if (cached?.signature === nextSignature) return cached.rows;`；`:12-15` 注释；`:98-100` ENOENT、`:102-103` 降级 |
| A07 | E7 §6.3 | `wsl-probe.js:149` 返回 `{native:null,wsl:null}`；"只返回 native"落点是 `install-resolver.js:21-23` | ✅ | `wsl-probe.js:149` 逐字；`install-resolver.js:21-23` `if (platform !== "win32") return { native: nativeValue ?? null, wsl: null };` |
| A08 | E9 §3.5 / §8 | `skipNoOpCursorCommit` 条件项 13 个 | ✅ | `sync.js:3424-3437` 逐项计数 = 13（`opts.auto`…`!opts.repairGrok`），清单与分片列举**逐项一致、顺序一致** |
| A09 | I1 §3.1 | 拿不到锁：写 marker + 向 stderr 输出 `Sync skipped: …`，不抛错 | ✅ | `sync.js:554-559`：`process.stderr.write(await recordSyncSkip(trackerDir, lockPath)); return;` |
| A10 | I2 §2.3 | 模板内 `:1483`/`:1484` 字面不同但**求值相同**，判据退化为一次 `existsSync`；同 quirk `openclaw-session-plugin.js:358,361` | ✅ | `init.js:1483`=`JSON.stringify(trackerBinPath)`、`:1484`=`path.join(trackerDir,'app','bin','tracker.js')`（值同为 `…/app/bin/tracker.js`）；`openclaw-session-plugin.js:358/361` 同形 |
| A11 | I3 §4.7 | `codexColdScanAudit` 6 字段，含 `lastFullScanAt` ISO 副本 | ✅ | `sync.js:3806-3818`：全量分支 `:3810` 写 `lastFullScanAt`，非全量分支 `:3813-3815` 写；字段共 6 |
| A12 | I4 §3.4 | 「append 与原子重写两条写路径，各自多个实现点」 | ✅ | 与 A04 同源证据；`upload-queue.js:64-132` 原子替换 |
| A13 | I6 §7.2 | `stripCr` 对调用方不可选：对外 opts 只有 3 项，调用点硬编码 `{stripCr:true}` | ✅ | `jsonl-lines.js:29-33` opts 三项；`:22` `decodePhysicalLine(buffer,{stripCr=false})`；`:85` 硬编码 true |
| A14 | I8 §8-12/14 | `resetWslProbeCache` 生产调用 0；`link_code_state.json` 仅 `init.js:176,234,359` + `runtime-config.js:149` | ✅ | grep：`resetWslProbeCache` 仅 `wsl-probe.js:11`（定义）与 `:268`（导出）；`link_code_state.json` 命中 `init.js:176`、`runtime-config.js:149` 清理清单 |
| A15 | I9 §4.8 | 补【存疑】其它 repair 保留语义未逐条核对，`sync.js:6844,7147` 未逐行打开 | ✅（诚实化） | 我打开确认：`:6844` `repairClaudeQueueFromGroundTruth`、`:7147` `relabelClaudeMemQueueRows` 两处定义；`:6745` 为 Droid 修复的"保留不可解析行"注释 |

### S1b（`S1b-worth-copying.md` 修正记录 8 条）

| # | 项 | 修正后表述（摘要） | 结论 | 依据 |
| --- | --- | --- | --- | --- |
| A16 | E2 §15 | 同 A02（模板 `1453-2231` / 宿主 `1445-2232`） | ✅ | 同 A02 |
| A17 | E3 §16 ③④ | 哨兵只剩 omp；新增 ④「内容等值短路 + 无条件备份覆盖」描述 OpenCode；明确"OpenCode 不会拒绝覆盖外来文件" | ✅ | `omp-hook.js:17,388-395`；`init.js:813`；OpenCode 侧描述与 `opencode-config.js` 行为一致（R1 已核，我复验 `unmanaged-extension-present` 全仓 2 命中） |
| A18 | E8 §6 | `cloneOrCopy` 用 `fs.copyFile(..., COPYFILE_FICLONE)`（`:1361`），非硬链接；脏分片重写先 `fs.unlink(targetPath)`（`:907`） | ✅ | `cursor-store.js:1357-1362` 逐字；`:907` `await fs.unlink(targetPath).catch(() => {});` |
| A19 | E9 不建议照抄 1 | `skipNoOpCursorCommit` 条件项 13 个，逐项列出 | ✅ | 同 A08 |
| A20 | I2 §15 | 模板内两路径字面相同，判据冗余 | ✅ | 同 A10 |
| A21 | I5 §8 | 代码块补 `if (prev?.lastTotal) return true;` 与 `return false;` 两行 | ✅ | `rollout.js:456-460`：`:458` 稳态短路、`:459` `lastTotal` 兜底、`:460` `return false` |
| A22 | I7 §7 ② | deferred 提示被 `if (!opts.auto)` 包住，`--auto` 轮次静默推迟 | ✅ | `sync.js:1175-1182`：`:1177` `if (!opts.auto) {`，`:1179` 才是提示文本 |
| A23 | U1–U10 | 文件头新增统一前置说明：平台/语言断言无本仓库证据 | ✅ | `S1b-worth-copying.md:7` 该段落存在且措辞与复核结论一致 |

### S2（`S2-framework-claude-codex.md` 修正记录 16 条）

| # | 项 | 修正后表述（摘要） | 结论 | 依据（含本人独立重跑） |
| --- | --- | --- | --- | --- |
| A24 | E1 §0-2/§A.1.3 | 「无统一契约：5 转发薄壳 + 40 自持 return + 11 种键集合」 | ✅✅ | **独立重跑复现**：脚本枚举 `^(async )?function parse\w*Incremental\(` = 45；尾随顶层 `return` 非字面量者 = 5（`parseOmpIncremental@16087`→`parseOmpLikeIncremental`、`parseOmoIncremental@16100`→`parseOmpLikeIncremental`、`parsePiIncremental@16689`→`parsePiLikeIncremental`、`parsePrimeAgentIncremental@16699`→`parsePiLikeIncremental`、`parseMinimaxCodeIncremental@16775`→`parsePiLikeIncremental`）；自持 return = 40；键集合去重 = **11** |
| A25 | E1-a | `recordsProcessed: total`（`:7270`、`:22791`）与 `recordsProcessed: eventsAggregated`（`:20551`）均为值标识符，非键 | ✅ | `rollout.js:7270` `return { recordsProcessed: total, eventsAggregated, bucketsQueued };`；`:22791` 同形 + `skippedRows`；`:20551-20553` `recordsProcessed: eventsAggregated,` + `eventsAggregated,`（键不重复） |
| A26 | E1-b | 11 种（含 `parseCopilotSessionStoreIncremental` 9 键） | ✅ | 我的脚本输出 11 个集合，含 9 键 `{active, adoptedThisRun, bucketsQueued, canonicalDbPaths, dbErrors, eventsAggregated, healthy, recordsProcessed, usageClaims}@19003` |
| A27 | E1-c | `rollout.js` 实测 24932 行 | ✅ | `ReadAllLines(UTF8).Count` = 24932；字节 942266 |
| A28 | I1 §0-6 | Codex = 子集标记（解析不拆 `:4851-4879`、计价置 0 `pricing/index.js:248-256`）；Claude = 独立加成项（解析拆出 `rollout.js:4930-4939`、计价照价）；两侧输入=五列之和，`total_tokens` 不计费 | ✅ | `:4864-4877` 注释与 `out.input_tokens = Math.max(0, …)`（**无** 对 reasoning 的算术改写）；`:4934-4936` `output_tokens: outputTokens - reasoningTokens`；`pricing/index.js:248-256` 白名单 `codex/acode/every-code/omo/cline`，`:297-299`、`:325-327` 两处溢价同门控 |
| A29 | I2 §A.3.3/存疑 1 | `endOffset = st.size` 3 处分工：`:2291` 早退 / `:2682` 项目扫描返回 `committedEndOffset`（`:2765`）/ `:2793` 唯一落 token 游标（`:1194`）；Codex token 路径 `:2652` | ✅ | grep 精确 3 命中；`:2652` `endOffset: committedEndOffset,`；`:2765` 同；`:2889` `return { endOffset, eventsAggregated };`；`:1194` `offset: result.endOffset,` |
| A30 | I2-附 | 「复核称『§C 第 10 条误引 `:2682`』——全文检索 `2682` 在本分片零命中，该误引不存在」 | ❌ 表述须修 | **修正后分片内 `2682` 有 7 处命中**（L19/451/456/1740/1818/1819/1832）。结论本身可救：现存 §C 正文确无 `:2682` 的原始引用，R1 的指控不成立——但"零命中"会被后续读者直接证伪，应改为"**修正前正文**的 §C 章节无 2682 引用" |
| A31 | I3 存疑 1 | 收窄为"本代文件内不可恢复"：inode 变化/截断会走重置路径全量重扫 | ✅ | `rollout.js:488-504` 三态判定（`sameInode`/`truncated`/`startOffset=0` 重置）与 `:1121-1122` 同构 |
| A32 | I4 存疑 14 | Codex 侧"reasoning 不双计"无对等回归测试，`pricing/index.js` source 白名单漏加会静默双计 | ✅ | `test/claude-reasoning-usage.test.js`（全 51 行）`:45-47` 的 `< 1e-12` 等值断言只覆盖 Claude；`pricing/index.js:248-253` 为硬编码白名单，无结构性判定 |
| A33 | I5 §A.2.3 | `countMalformedCodexProjectQueueRows` 全库一个调用点，只决定"是否修复"，不产生用量 | ✅ | grep：定义 `sync.js:6436`、唯一调用 `sync.js:5824` |
| A34 | I6 §A.3.2 | Claude 四字段仅在 `projectEnabled` 分支写入；Codex 的 `projectOffset/projectFileContext` 受 `codexProjectFastPath` 控制 | ✅ | `rollout.js:1198-1205` `...(projectEnabled ? { claudeCwd, projectFileContext, projectRef, projectKey } : {})`；`:656-665` `if (codexProjectFastPath) {…} else if (Number.isFinite(prev?.projectOffset)) {…}` |
| A35 | I7 §A.5.2 | "语义等价、文本不同"：`rollout.js:4173-4190` 分步 vs `claude-categorizer.js:1175-1191` 单行链式；副本无测试锁定 | ✅ | 两份实现逐行比对：取整规则与 `Date.UTC` 参数完全相同，唯一差别是 `const bucketStart` 中间变量；`test/` 中 `toUtcHalfHourStart` 仅被 `sync-droid-dup-session-repair.test.js:29` 与 `trae-entitlement.test.js:29` 引用，均 `require("../src/lib/rollout")` |
| A36 | I8 §B.6.1 | 注释改引 `:1115-1120` | ✅ | `rollout.js:1114` 确为空注释行 `    //`；正文注释在 `:1115-1120` |
| A37 | U2 §B.4.2 | `<synthetic>` 全库 3 处：`session-analytics.js:101`、`:621-626`、`:1968-1971` | ✅ | 三处逐一打开，文本与分片引用一致 |
| A38 | U3 §C.5.3 | 语料统计标为"上游源码注释观测" | ✅ | `codex-service-tier.js:8-10` 原文 `on a 1213-file local corpus only 167 files carried the event, covering 25% of token_count events` |
| A39 | U4 §C.6.1 | 拆成"不写队列已闭合"+"不影响统计依赖一次性迁移前提" | ✅ | `local-api.js:71` `computeCodexContextBreakdown`、`session-analytics.js:32` `parseCodexRolloutFile` 均为读侧引入；`sync.js:198` 借用 `extractTokenCount` |
| A40 | 未采纳=0 / 4 处纠正 | 四条纠正（E1-a/E1-b/E1-c/I2-附） | ✅（除 A30 措辞） | 四条中三条经我独立复现成立；I2-附见 A30 |

## B. 独立新抽样（逐条：断言 / 结论 / 依据）

避开 R1 抽样点，集中在用户点名的区域：`serve.js` 路由与定时器、`passive-mode.js` 判定链、`scan-roots.js` 可列性、`fs.js` 心跳/reclaim、`jsonl-lines.js` 字节上限、S1b 第 10–14/17/18 条、S2 非主线 parser 与早退条件。

| # | 断言（我选的易错点） | 结论 | 依据 |
| --- | --- | --- | --- |
| B01 | `serve.js` 的 HTTP 路由只把三类前缀交给 API handler：`/functions/`、`/api/`、`/proxy/`；未命中再走静态、再 SPA 回退、最后 404 | ✅ 成立（S1 §7.5 未描述路由构成） | `serve.js:172-191` |
| B02 | CORS preflight：`OPTIONS` 且非 `/api/auth/` → 204，允许 `GET, POST, OPTIONS` | ✅ 成立（分片未记） | `serve.js:162-169` |
| B03 | 端口回退有开关：**显式 `--port` 时 `allowFallback:false`（严格失败）**，默认启动才 +1 重试 | ✅ 成立；S1 §7.5 只写了"+1 最多 20 次"，缺这半句 | `serve.js:209-215` `allowFallback: !opts.portExplicit` |
| B04 | 心跳定时器：启动即发一次 + `setInterval(…, 6h).unref()` | ✅ 与 S1 §5.2 一致 | `serve.js:269-273` |
| B05 | 后台轮询有 `inFlight` 防重入（上一轮未结束不重复 spawn） | ✅ 成立；S1/S1b 未记，属可移植细节 | `serve.js:332-334` |
| B06 | 关停路径：`SIGINT/SIGTERM` → `nativeBackgroundSync.stop()` → `server.close` 后 3s 兜底强退 | ✅ 成立（分片未记） | `serve.js:281-288` |
| B07 | `classifyWritableFailure` 有第 5 态：`accessSync` 成功 → `"writable (hook may have been removed externally)"`；S1 §8-8 只说 ENOENT/EACCES/EROFS+其它 | ✅ 源码如此（分片表述不算错，但少这一态） | `passive-mode.js:70-81`（`:73-74`） |
| B08 | `hookStatus` 读取存在**两种风格**：`codex/acode/every_code` 用 `*_notify ?? 裸名` 双回退，`claude/gemini/codebuddy/workbuddy/workbuddy-ai` 只读裸名，`workbuddy-ai` 用连字符键 | ✅ 成立；与 S1 §8-9/§8-17 的命名陷阱一致但更细 | `passive-mode.js:100,112,125,136,148,159,169,180` |
| B09 | `status.js` 传 11 个键（两键 `opencode_plugin`/`openclaw_session_plugin` 在 passive 判定中永不被读） | ✅ 与 S1 §8-17 一致 | `status.js:1024-1036` vs `passive-mode.js:96-185` |
| B10 | `scanRootDirState` **定义**在 `scan-roots.js:97`（`:89-96` 是它的问题定义注释）；三分支：非目录→absent / `opendirSync().closeSync()` 成功→exists / 其余→error | ✅ 成立（建议引用写作 `:89-110` 或 `:97-110`） | `scan-roots.js:97-110` |
| B11 | 锁常量块共 6 个：`LOCK_STALE_MS=5min`、`LOCK_HEARTBEAT_MS=30s`、`MAX_RECLAIM_DEPTH=4`、`MAX_RECLAIM_SWEEP_DEPTH=64`、`TRANSITION_GUARD_RETRY_MS=5`、`TRANSITION_GUARD_ATTEMPTS=1000` | ✅ 成立（S1b §1 引 `:69-74` 恰好覆盖 6 个） | `fs.js:69-74` |
| B12 | `.reclaim` 迭代清扫**从最深一级开始**删（否则浅层删掉后深层孤闸会重建同一条堵塞链） | ✅ 与 S1b §1 描述一致 | `fs.js:186-204`（`:200-202` reverse 循环） |
| B13 | 心跳同时 `utimes` 锁文件与心跳文件；`heartbeatPathFor` = `lockPath + ".heartbeat." + sha256(token)` | ✅ 与 S1b §1 一致 | `fs.js:88-91,206-214` |
| B14 | `oversized` 记录**仍带** `physicalBytes` 与 `terminated:true`（游标可按物理字节推进），S1 §7.2 表格只写 `{line:null, oversized:true}` | ✅ 源码如此（建议补 `physicalBytes/terminated`） | `jsonl-lines.js:72` |
| B15 | 字节预算按**含 LF 的物理字节**判定（`physicalBytesRead + physicalBytes > byteLimit` → 抛错），且判定先于超长丢弃 | ✅ 成立 | `jsonl-lines.js:60-63,96-98` |
| B16 | 压缩时 `verifiedMs === null` 的普通行**总是**覆盖前条（last-wins）；只有 account 行才严格更大才替换 | ✅ 与 S1b §4 一致 | `upload-queue.js:88` |
| B17 | S1b §10 的 `rollout.js:683-686` 双队列 append 逐字成立 | ✅ | `rollout.js:683-686` |
| B18 | 项目行**没有** `model` 字段（S1b §10 成立） | ✅ | `rollout.js:3515-3530` |
| B19 | `pendingBytes` 只读主队列（注释明写"counting project queue creates false backlog and keeps auto retry alive"） | ✅ S1b §10 代码块逐字 | `sync.js:3654-3659` |
| B20 | 每批成功后**立即**落盘 offset（`state.offset=offset; writeJson(queueStatePath, state)`） | ✅ S1b §11 逐字 | `sync.js:4397-4400` |
| B21 | `upload_id` 随批次体发送 | ✅ S1b §11 成立 | `sync.js:4368` |
| B22 | 控制行（`account_session_state`）与桶行共用同一 `linesRead >= maxBuckets` 闸 | ✅ S1b §11 逐字 | `sync.js:4488-4489`、`:4511-4512` |
| B23 | `upload_scope === "community"` → `batchBudget = Math.min(batchBudget, 10)` | ✅ S1b §11 成立 | `sync.js:4392` |
| B24 | 冷扫描双阈值常量与"deferred 累加"落点 | ✅ S1b §12 成立 | `sync.js:3763-3783`（判定）、`:3456-3460`（`writeDeferredCodexAuditSyncs(deferredCodexAuditSyncs + 1)`） |
| B25 | `normalizeQueueRow` 在上传路径被复用（且注释解释"否则云端永久保留 6–7x 膨胀的旧值"） | ✅ S1b §13 成立 | `sync.js:4504-4509` |
| B26 | `status.js` 读 `sync.skip.json` 与上传退避字段并输出 `last_sync_skipped`/`next_upload_after`/`backoff_until`/`last_upload_error` | ✅ S1b §14 成立 | `status.js:285`、`:1054-1058` |
| B27 | `install-resolver` 把 `requireAnyChild` 折进 `existsSync` 探针（`wslExists`），使 `discoverWslHome` 跳过空壳发行版 | ✅ S1b §17 逐字 | `install-resolver.js:30-36` |
| B28 | WSL 双前缀顺序按版本：v1 先 `\\wsl.localhost\`，v2 先 `\\wsl$\` | ✅ S1b §17 与 S1 §6.3 一致 | `wsl-probe.js:193-201` |
| B29 | 别名表 8 项（含 `code→every-code`、`kilo-code→kilocode`）；`AUTO_SYNC_SOURCES` **45 项**（首 `acode` 末 `zed`）；`BACKGROUND_AUTO_SYNC_SOURCES` 4 项 | ✅ S1b §18 的"45 项"经脚本计数成立 | `sync.js:310-319`、`:320-366`、`:367-373` |
| B30 | `openCursorStore` 签名含 `failureInjector`；注入点在 `beforeManifestSwap`（manifest 写入之前） | ✅ S1b §18 成立 | `cursor-store.js:25-32`、`:972` |
| B31 | `parseClaudeScienceIncremental`：`input` 含缓存的私有约定通过**两组独立 clamp** 拆分（主/aux 分别减 cache），`reasoning_output_tokens` 恒 0，`billable_total_tokens = total` | ✅ 成立；S2 §B.1.4 的行号引用（`:6999`、`:7023`）精确 | `rollout.js:6995-7027` |
| B32 | 该函数早退条件是 `!frameId || !base || !bucketStart` 三元组（`recordsProcessed += 1` 后 `continue`） | ✅ 成立；S2 §A.5.1 只引了时间戳那一支 | `rollout.js:7074-7077` |
| B33 | 幂等减法是"先 `subtractTotals` 旧贡献（并把它记进 `touchedBuckets`）再 `addTotals`"，且 `unchanged` 需 `totalsKey` 相等 + 桶 + 模型三者同时相同 | ✅ 与 S2 §B.1.4 一致 | `rollout.js:7088-7118` |
| B34 | `parseCursorApiIncremental` 是**清桶重填**语义：先按本次导出最早桶起清零该 source 的历史桶，且**导出里没有任何可解析日期时整体跳过清桶**（防把全史清零并上传 0） | ✅ 源码如此；S2 未展开该守卫（值得补入 S2） | `rollout.js:7184-7221` |
| B35 | 该函数返回 `recordsProcessed: total`（`total = records.length`，即"处理的记录数"是**总数**而非增量计数）；游标是 `cursors.cursorApi.lastRecordTimestamp`（字符串比较取最大） | ✅ 成立；S2 §A.1.3 的"假签名"判定正确，且此处的语义与其它 parser 的累计计数不同 | `rollout.js:7170-7171,7191,7245-7268,7270` |
| B36 | 十分钟桶之外的第二个 provider 专属游标命名空间（`cursors.cursorApi`）未被 S2 §A.3.1 的顶层键表覆盖 | ✅ 现状如此（表未声明穷尽） | 同 B35；对照 `rollout.js:7170` 注释 `Incremental state is tracked in cursors.cursorApi.lastRecordTimestamp` |
| B37 | Codex 主线的零值哨兵只在 `source === DEFAULT_SOURCE`（常量 `"codex"`）时启用；`normalizeUsage` 直取 `total_tokens` 不做重算 | ✅ 成立 | `rollout.js:55`、`:2550-2552`、`:4853-4862` |
| B38 | Codex 每个被接受的 delta 行都写 `conversation_count = 1`（与 Claude 的"user 文本行计数"是两套语义） | ✅ 源码如此；S2 的 C 章与 §A.4 均未记录 Codex 侧会话计数语义（覆盖缺口） | `rollout.js:2554`；对比 `rollout.js:2865`（Claude assistant 行 = 0） |
| B39 | 上传侧对缺字段行做兜底：`source` 缺省 `"codex"`、`model` 缺省 `"unknown"`（与解析侧默认不同源） | ✅ 成立（分片未记） | `sync.js:4500-4501` |
| B40 | 解析侧 `source`/`model` 归一为小写 trim 后再入桶键 | ✅ 成立（S1 §3.3 只说键含 source/model，未说归一） | `sync.js:4500-4502` |

## C. 跨文件一致性检查

同一事实在三份分片中的表述比对（均为我实测过的源码事实）：

| # | 事实 | S1 | S1b | S2 | 判定 |
| --- | --- | --- | --- | --- | --- |
| C01 | `rollout.js` 24932 行 / 942266 B | §0.1 ✅ | — | §0.1 ✅ | 一致（实测 24932 / 942266） |
| C02 | `sync.js` 7251 行 / 288151 B | §0.1 ✅ | 不建议照抄 1 ✅ | — | 一致（实测） |
| C03 | `status.js` 1729 / `cli.js` 107 / `init.js` 2427 / `bin/tracker.js` 60 | §0.1 ✅ | — | §B.6.3 用 `claude-config.js` 279 行（实测 279） | 一致 |
| C04 | 0.26.3 事故量级：`2.17B claude tokens` | §4.8 "2.17B" | §7 "21.7 亿" | — | 一致（同一注释原文 `sync.js:239-240`，两种数字写法） |
| C05 | `claudeHashes` 上限 10 万、保留最后 10 万 | §4.6 `:1233-1237` ✅ | — | §A.3.1 `:1236-1237`、§B.3.1 ✅ | 一致（`rollout.js:1236-1237`） |
| C06 | rewrite 触发全量重扫（inode 变/截断） | §4.5 ✅ | §6（重写前先 unlink）✅ | §A.3.3 ✅ | 一致（`rollout.js:488-504`、`:907`） |
| C07 | `union` 只给 codex/acode；every-code/gemini 单选 | §6.4（`:782-787`、`:815-820`、`:855`、`:1228`）✅ | §17 ② ✅ | §C.1.1（codex union）✅ | 一致（我实测 `sync.js:855` 与 `:1228` 均无 `union:true`） |
| C08 | Claude 消息去重键 `${msgId}:${reqId}` / 退化为 `msgId` | §4.6 ✅ | — | §B.3.1（含 issue #64 原委）✅ | 一致（`rollout.js:4895-4919`） |
| C09 | `passive` 覆盖 8 个 hook 驱动 provider；`status.js` 传 11 键 | §2.6（8 个）+ §8-17（11 键）✅ | 不建议照抄 14（两键未使用）✅ | — | 一致（`passive-mode.js:96-185`、`status.js:1024-1036`） |
| C10 | v2→v3 的 `groupQueued` 遗留标记注释 | §8-1 引 `sync.js:216-220` | 不建议照抄 8 引 `sync.js:216-226` | — | **行范围写法不同**（同一连续注释块 `:214-247` 的子段），非矛盾；建议统一为 `:215-220` |
| C11 | 迁移键 `claudeGroundTruthRepair_2026_05_v4` | §4.8 `:248` ✅ | §7 ✅ | §B.7.1 `:248` ✅ | 一致（`sync.js:248`） |
| C12 | 半小时桶实现唯一的写入侧 | S1 未涉 | — | §A.5.2（两份实现、无等价锁定）✅ | 一致（S2 更细；我实测两份均存在） |
| C13 | `endOffset = st.size` 站点数 | — | — | §0-8 / 存疑 1 / 修正记录三处均写 "3 处" | 内部自洽（实测 3 处） |

## 错误（E）/ 不完整（I）/ 无法验证（U）

### E（必须修，3 条；均为表述级，不动机制结论）

- **E1｜S2 修正记录 I2-附（`S2-framework-claude-codex.md:1819`）**：`全文检索 2682 在本分片零命中` 在**修正后的分片内可被直接证伪**（现存 7 处命中：L19、L451、L456、L1740、L1818、L1819、L1832）。**结论本身成立**（现存 §C 正文无 `:2682` 原始引用，R1 的指控不成立），但断言须限定语境。**修法**：改为"§C 正文（修正前）无 `2682` 引用；修正记录自身含该字串"。
- **E2｜S1b §10（`S1b-worth-copying.md:392`）**：`项目行只多两个字段，其余同构（rollout.js:3515-3532）`——不成立。主队列行含 `usage_precision` 与 `...prioritySubsetOf(totals)` 展开（`rollout.js:3341-3342`），项目行两者皆无（`:3515-3530` 只有 `project_ref/project_key` + 10 列）。**修法**：改为"项目行比主行**少** `usage_precision` 与 priority 子集列（项目视图不区分档位/精度）"。
- **E3｜S1 §4.4（`S1-pipeline-and-collection.md:614`）**：opencode 游标字段表漏 `opencodeForkRepairVersion`——该字段既写回（`rollout.js:1519` `opencodeForkRepairVersion: 1`）又参与 idle 判定（`:1465` `prev.opencodeForkRepairVersion === 1`），是"文件未变"判据的组成部分。**修法**：字段表补该键，并注明它使"旧代无此字段的游标会被重判为已变"。

### I（不完整，7 条；建议补，不影响结论可用性）

- **I1｜S1 §4.4 KiroCli 行号收窄**：`:9451-9471` 建议改为 `:9468-9472`（游标写回段），或写成"桶累加 `:9451-9467` + 写回 `:9468-9472`"。
- **I2｜S1b §10 补一句**：项目队列的读侧 key 是 `project_key|source|hour_start`（`local-api.js:145-153`），**不含 model**，与项目行"无 model 字段"互为因果（S1b 已说无 model，但未把它与读侧 key 对齐说明）。
- **I3｜S2 §A.3.1 顶层游标键表**：未覆盖 provider 专属命名空间（`cursors.cursorApi` `rollout.js:7170`、`cursors.claudeScience` `:7136`、`cursors.kiro` `:7589`、`cursors.kiroCli` `:9261`、`cursors.traeCn` `:22789`）。表未声明穷尽，但建议补一行"另有一批 provider 私有命名空间"。
- **I4｜S2 §A.4/C 章**：未记录 Codex 的 `conversation_count = 1`（每 delta 行，`rollout.js:2554`）与 Claude（user 文本行 +1、assistant 恒 0）的语义差异。三份分片里唯一提到 `conversation_count` 语义的是 S2 §B.2.4/B.2.5（Claude 侧），Codex 侧缺位。
- **I5｜S1 §7.2 `oversized` 记录形态**：补 `physicalBytes` 与 `terminated:true`（`jsonl-lines.js:72`），否则读者会以为超长行不推进字节预算。
- **I6｜S1 §7.5 端口回退**：补"显式 `--port` 时不回退（`allowFallback: !opts.portExplicit`，`serve.js:210`）"。
- **I7｜S1 §5.2 后台轮询**：补 `inFlight` 防重入（`serve.js:332-334`）与关停路径（`:281-288`）两条可移植细节。

### U（无法验证，4 条——全部属于"需要真实语料/运行时状态"，与 R1 的 U 组口径一致）

- **U1｜S2 §B.2.3**（承 R1）：`cache_write_input_tokens` 别名在 Claude 会话行中的出现率——需本机 `~/.claude/projects/**` 语料统计，源码不可判。代码事实已复核成立（`rollout.js:4927-4928` 只读 `cache_creation_input_tokens`；`codex-token-usage.js:25-27` 有别名兜底）。
- **U2｜S2 §B.4.2**：真实 Claude 是否给 `"model":"<synthetic>"` 行写非零 usage——需真实日志。
- **U3｜S2 §C.5.3**：`1213 文件/167 命中/25%`、`814 default/58 priority` —— 上游注释的本机语料观测，源码不可判（我已确认这些数字确系注释原文，见 A38）。
- **U4｜S2 §C.6.1**：血缘修复在具体安装上是否永不触发——取决于 `cursors.migrations` 运行时状态。

## 复核到的行号抽样（证明真的打开过）

以下为我本轮**新打开**的源码位置（不含 R1 已列位置）及其用途，均可复现：

| 位置 | 用途 |
| --- | --- |
| `src/commands/serve.js:157-198, 209-215, 264-288, 303-343, 620-660` | 路由三分支、端口严格/回退、6h 心跳、后台轮询 inFlight 与 native-only |
| `src/lib/passive-mode.js:49-81, 93-204` | 判定链、10 键读取风格、8 provider 名单 |
| `src/commands/status.js:282-288, 1014-1037, 1050-1059` | marker 读取、11 键 hookStatus、上传退避字段 |
| `src/lib/scan-roots.js:86-110, 244-253` | 可列性三态、unreadable 展示 |
| `src/lib/fs.js:60-100, 176-214` | 6 常量、判活、reclaim 清扫、心跳 |
| `src/lib/jsonl-lines.js:20-45, 55-100, 120-129` | opts、预算判定、oversized、末行未终止 |
| `src/lib/upload-queue.js:18-43, 70-95` | iterateQueueLines 契约、压缩前置条件与 verifiedMs 语义 |
| `src/lib/async-queue-reader.js:12-16, 33-37, 97-106` | 并发注释、签名命中、ENOENT 与降级 |
| `src/commands/sync.js:196-200, 310-378, 516-570, 1175-1183, 3420-3462, 3558-3568, 3755-3783, 3806-3818, 4310-4368, 4392-4400, 4431-4452, 4486-4514, 5824, 6436, 6743-6747, 6844, 7147` | 全部 A/B 组引用点 |
| `src/lib/install-resolver.js:1-45`、`src/lib/wsl-probe.js:145-152, 188-206`、`src/lib/cursor-store.js:20-36, 904-910, 969-976, 1349-1363` | install-resolver 与游标/克隆语义 |
| `src/lib/rollout.js:54-58, 450-462, 632-666, 681-690, 1074-1086, 1112-1122, 1192-1206, 1231-1238, 1287-1307, 1460-1520, 1947-1956, 2548-2556, 2650-2654, 2763-2767, 2887-2891, 3325-3348, 3512-3533, 3644-3652, 3763-3783(→sync), 4168-4192, 4851-4893, 4925-4942, 6980-7154, 7168-7295, 9370-9376, 9451-9472, 20548-20553, 22788-22793` | 游标字段、去重视图、去重键、半小时桶、normalizeUsage/sentinel、非主线 parser |
| `src/lib/pricing/index.js:243-258, 293-300, 320-330`、`src/lib/session-analytics.js:30-34, 99-103, 620-626, 1966-1972`、`src/lib/codex-service-tier.js:6-16`、`src/lib/omp-hook.js:15-19, 386-396`、`src/lib/opencode-config.js:9, 31-32, 112`、`src/lib/grok-hook.js:4-10`、`src/lib/claude-categorizer.js:1173-1192`、`src/lib/local-api.js:69-73, 140-201`、`src/commands/init.js:176, 811-815, 1445-1455, 1481-1486, 2229-2234` | 计价门控、哨兵值、语料注释、hook 事件名、模板边界 |
| `test/claude-reasoning-usage.test.js:43-49`（全 51 行）、`test/*.test.js` 中 `toUtcHalfHourStart` 全部命中 | 回归测试覆盖面 |
| 实测脚本 | `rollout.js` 45 个 parser 的薄壳/自持 return/键集合全量提取（复现 5/40/11）；`AUTO_SYNC_SOURCES` 计数（45）；`src/` 文件计数（112）；各文件行数/字节数 |
