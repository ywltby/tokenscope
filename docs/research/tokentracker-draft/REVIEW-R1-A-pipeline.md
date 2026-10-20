# 复核报告 R1-A：S1-pipeline-and-collection.md + S1b-worth-copying.md

样本：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）。
口径：全程 `[IO.File]::ReadAllLines(...,UTF8)` / `read` 工具（LF、UTF-8），未使用默认编码的 `Get-Content`。
只读：未修改样本仓库、未执行其中脚本、未 `npm install`。

## 结论

**两份分片整体可用，但必须修订**：结构、机制描述、绝大多数行号经得起逐条证伪（抽验 139 条中 111 条完全成立），
但有 **9 条错误**，其中 3 条会影响移植决策（`notify.cjs` 模板范围、队列"唯一写点"、OpenCode 哨兵范式），必须修。

## 抽样统计

- 抽验断言数：**139**（其中 E=9，I=9，U=10，通过 111）
- 其中对分片内**代码块引用行号**的核对：**63 条**（要求 ≥15）
- 覆盖：S1 §0–§7 全章节、S1b §1–§18 与"不建议照抄"节，按章节均匀分布（不集中开头）
- 实际打开并逐行核对的源码文件：

`bin/tracker.js`、`src/cli.js`、`src/lib/debug-flags.js`、`src/lib/tracker-paths.js`、`src/lib/notify-command.js`、
`src/lib/claude-config.js`、`src/lib/gemini-config.js`、`src/lib/codex-config.js`、`src/lib/grok-hook.js`、
`src/lib/opencode-config.js`、`src/lib/omp-hook.js`、`src/lib/openclaw-session-plugin.js`、`src/lib/openclaw-cli.js`、
`src/lib/passive-mode.js`、`src/lib/scan-roots.js`、`src/lib/install-resolver.js`、`src/lib/multi-install-parser.js`、
`src/lib/wsl-probe.js`、`src/lib/fs.js`、`src/lib/cursor-store.js`、`src/lib/upload-queue.js`、`src/lib/async-queue-reader.js`、
`src/lib/jsonl-lines.js`、`src/lib/local-api.js`、`src/lib/runtime-config.js`、`src/lib/rollout.js`、`src/commands/sync.js`、
`src/commands/init.js`、`src/commands/serve.js`、`src/commands/status.js`（另用 `git ls-files`/`Get-ChildItem` 核 `src/` 文件数）。

## 错误（E）—— 必须修

- **[E1] S1 §0.1（分片 L32）**：断言「实测 `src/` 下共 **128 个文件**（含 `src/lib/pricing/`）」/
  实际 `git ls-files src` = **112**，`Get-ChildItem -Recurse -File -Force src` = **112**（含隐藏文件也是 112）。
  修正建议：改成 112，并注明计数口径（仓库跟踪文件数）。

- **[E2] S1 §2.3（L187）+ S1b §15（L546）**：断言 notify.cjs 模板位于 `src/commands/init.js:1445-1574` /
  实际 `buildNotifyHandler` 函数为 **1445-2232**，其内嵌模板字符串从 **1453** 一直到 **2231** 的 `` `; ``（1432 行起还有
  `resolveMaybeHome`/`isSelfNotify`/`shouldChainNotify`/`splitEnvString`/`findNodeLikeScriptIndex` 等**模板内**函数）。
  区间少了 657 行；分片列举的 6 条行为行号（1446-1447、1448、1461-1476、1488-1517、1519-1536、1538-1559、1561、1563-1573）
  全部正确，只是"模板边界"写错。
  修正建议：写作 `init.js:1453-2231（buildNotifyHandler 1445-2232）`。

- **[E3] S1 §2.8 ③（L315）+ S1b §16 ③（L614）**：断言「哨兵范式（omp / **OpenCode**）……带标记 → 覆盖，无标记 → 拒绝写入
  （`unmanaged-extension-present`）」，并引 `src/lib/opencode-config.js:8,73-75` /
  实际 `unmanaged-extension-present` **全仓只有一处产生**：`src/lib/omp-hook.js:391`（消费方 `src/commands/init.js:813`）。
  `opencode-config.js:73-75` 是 `if (existing === next) return {changed:false}` 的**内容等值短路**，与哨兵无关；
  `upsertOpencodePlugin`（`:66-87`）对**任何**已存在内容都是"备份后覆盖"，**不做**"无标记则拒绝"；
  无标记只在 `removeOpencodePlugin`（`:94`）里返回 `"unexpected-content"`。
  影响：S1b §16 据此得出的"OpenCode 会拒绝覆盖外来文件"是错的，移植时若照此实现会与源码行为不一致。
  修正建议：把 OpenCode 从"哨兵范式"移到独立的"内容等值短路 + 无条件备份覆盖"一类；哨兵范式只留 omp。

- **[E4] S1 §3.4（L457）**：断言「写队列**只有一处** `fs.appendFile`：`await fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8")`
  （`src/lib/rollout.js:3483`）」/ 实际 `queue.jsonl` 的 append 写点至少有 **5 处**：
  `src/lib/rollout.js:3483`（enqueueTouchedBuckets）、`src/lib/rollout.js:20031`（`retractStaleSourceQueueRows`）、
  `src/lib/rollout.js:22594`（`appendTraeCnSessionStates`）、`src/commands/sync.js:5126`（`repairGrokQueueFromSessionSnapshots`）、
  `src/commands/sync.js:5207` 与 `:5281`（`migrateRolloutCumulativeDeltaBuckets`）；另有 `sync.js:4114` 写 fallback 文件、
  `rollout.js:3536` 写 `project.queue.jsonl`。
  注意：状态没变差——**"无独立写侧互斥、依赖外层 `sync.lock`"的结论仍成立**（附录 A 的 B11 因此判"部分成立"）。
  修正建议：改为"main-queue 的 append 写点分布在 `rollout.js` 三条路径与 `sync.js` 三条 repair 路径，全部假定调用方持有 `sync.lock`"。

- **[E5] S1 §4.4（L607-609）**：断言「其它按行号定位的 provider（Gemini / **Kilo** 等）……记 `mtimeMs` + `lastIndex` + `lastTotals`
  （`src/lib/rollout.js:1289-1307`、`:1462-1516`）」/ 实际 `:1289-1307` 是 Gemini ✓，但 **`:1462-1516` 是 opencode 路径**
  （`:1498 parseOpencodeMessageFile`；`:1513-1520` 写 `inode/size/mtimeMs/lastTotals/messageKey`，**没有 `lastIndex`**）。
  KiroCli 的 `mtimeMs + lastIndex` 游标实际在 `src/lib/rollout.js:9371-9375, 9451-9471`（`parseKiroCliIncremental`，`:8886` 起）。
  修正建议：第二段改为 `:9371-9471`（KiroCli），opencode 单独说明其游标是 `mtimeMs + lastTotals + messageKey`。

- **[E6] S1 §7.1（L993）**：断言「签名不变 → 直接返回缓存行（Dashboard 一次刷新会并发打多个端点，这个缓存是必需的，
  `:99-104` 的注释也说明了目的）」/ 实际 `:99-104` 是 `ENOENT` 清理与**失败回退**分支；
  "签名相同即返回缓存"在 `src/lib/async-queue-reader.js:35`，并发目的注释在 `:12-15`。
  修正建议：行号改为 `:35`（+ 注释 `:12-15`）。

- **[E7] S1 §6.3（L905）**：断言「非 win32 平台一律只返回 native（`:149`）」/
  实际 `src/lib/wsl-probe.js:149` 是 `if (platform !== "win32") return { native: null, wsl: null };`（两侧都为 null）；
  非 win32 返回 native 的是 `src/lib/install-resolver.js:21-23`（`{ native: nativeValue ?? null, wsl: null }`）。
  修正建议：行号改为 `install-resolver.js:21-23`，或改写为"发现层非 win32 只做 native 探测"。

- **[E8] S1b §6（L236、L242）**：断言「他们的 `cloneGenerationFiles` 用**硬链接**/复制二选一，见 `:1357-1362`」，并据此在"移植注意"
  推出"硬链接会污染旧代"的推理 / 实际 `cloneOrCopy` 是
  `await fs.copyFile(source, target, fssync.constants.COPYFILE_FICLONE)`（`:1361`）——reflink/写时复制，**不支持时回退为普通复制**，
  与硬链接无关。结论（重写前先 `fs.unlink(targetPath)`，`:907`）仍成立，但论证前提错误。
  修正建议：改为"clone 用 `COPYFILE_FICLONE`（CoW）或普通复制；因脏分片是**整文件重写**（先 unlink 再 create），旧代不受影响"。

- **[E9] S1b "不建议照抄" 1（L727）**：断言「`skipNoOpCursorCommit`（`:3424-3437`）这类判据要同时引用 **9 个变量**」/
  实际 `src/commands/sync.js:3424-3437` 的条件项共 **13 个**：`opts.auto`、`!isFullSourceScan`、`cursorStore.mode`、
  `requiresCommit`、`totalParsed === (commandCodeResult.cursorUnchanged ? … : 0)`、`totalBuckets`、两处 `projectBucketsQueued`、
  `commandCodeResult.schemaMigrated`、`codexColdAuditDue`、`codexFallbackRetryRan`、`grokHookSignalConsumed`、`opts.repairGrok`。
  修正建议：改"13 个条件项"，并同步修正 S1 §3.5 的判据列举（见 I4）。

## 不完整（I）—— 建议补

- **[I1] S1 §3.1 第 3 步（L331）**：写「拿不到锁则写 `sync.skip.json` 并**静默 return**」/ 实际 `:557` 会
  `process.stderr.write(await recordSyncSkip(trackerDir, lockPath))`，输出 `Sync skipped: <reason> — <detail>`；
  且 §5.3 自己就是把这条"可见化"当卖点写的，两处口径应统一为"不抛错、写 marker 并向 stderr 报一行"。
- **[I2] S1 §2.3（L203-207）+ S1b §15（L567）**：notify.cjs 的"双路径"判据实际是同一个路径：
  模板里 `trackerBinPath = path.join(trackerDir,'app','bin','tracker.js')`（`:1483`，模板内）与
  `depsMarkerPath = path.join(trackerDir,'app','bin','tracker.js')`（`:1484`，模板内）**字面相同**，
  因此 `hasLocalRuntime && hasLocalDeps` 退化为一次 `existsSync`。同一 quirk 也出现在 OpenClaw 插件模板
  （`src/lib/openclaw-session-plugin.js:358` 与 `:361`）。移植时别照抄这层冗余判断。
- **[I3] S1 §4.7（L694）**：`recordCodexColdScanAudit` 的字段集实际有 **6** 个：除分片列的 5 个外还有
  `lastFullScanAt`（`src/commands/sync.js:3810`、`:3813-3815`）。
- **[I4] S1 §3.4（L460）**：写「另一处写队列的方式是原子替换，**只在两处出现**：`compactPendingQueue` 与各类 repair」
  ——"只在两处"与后文列举的"各类 repair"自相矛盾；结合 E4，实际是"append 与原子重写两条写路径，各自有多个实现点"。
- **[I5] S1b §8（L321）**：写「只有当'单路径 + 同 inode + 未截断 + 有历史 offset'四条同时成立时，才允许用廉价的本轮集合」/
  实际 `needsHistoricalRolloutDedup` 在四条之后还有两条兜底：`if (prev?.lastTotal) return true;`、`return false;`
  （`src/lib/rollout.js:456-460`）；缺了 `prev.lastTotal` 这条会使"何时动持久集合"的判断不完整。
- **[I6] S1 §7.2（L1011）**：写「CRLF | 可选 `stripCr`（`:22-27`）」/ `stripCr` 只是 `decodePhysicalLine` 的默认参数，
  `physicalJsonlRecords` 的对外 opts 仅 `invalidUtf8/maxPhysicalBytes/maxRecordBytes`（`src/lib/jsonl-lines.js:29-33`），
  调用点硬编码 `{ stripCr: true }`（`:85`）——对调用方**不可选**。
- **[I7] S1b §7 ②（L286）**：deferred 提示实际只在 `!opts.auto` 时打印
  （`src/commands/sync.js:1177-1181`）；hook/后台轮次会静默推迟，与"不要把推迟做成静默 return"的建议需要并列说明前提。
- **[I8] S1 §8 第 12/14 条（L1071、L1073）**：两个"未确认"可结案——`resetWslProbeCache` 的生产路径调用为 **0**（仅
  `test/*.test.js` 调用，`src/` 内除定义与导出无调用点）；`link_code_state.json` 在 `src/` 与 `dashboard/` 内除
  `init.js:176,234,359`（声明/传参）与 `src/lib/runtime-config.js:149`（清理清单）外**零引用**，可判为遗留参数。
- **[I9] S1 §4.8（L745 附近）**：`queue.jsonl` 的"保留不可解析行"策略只在一处注释点到（`:6745` 属 Droid 修复），
  但 `repairClaudeQueueFromGroundTruth` / `relabelClaudeMemQueueRows` 等其它 repair 的保留语义未逐条核对；
  另 §3.4 引用的 `sync.js:6844,7147` 未逐行打开（本次只核到 `:6745` 与 `:6288-6372` 段），建议下一轮补。

## 无法验证（U）

S1b 的"移植注意"大量断言的是**另一门语言/平台**的行为，本仓库无证据。按 CHECKLIST 口径逐条归类（不是错误）：

- **[U1] S1b §1（L51-54）**：Rust `create_new(true)`、`uuid/rand` 令牌、`OpenProcess(SYNCHRONIZE)`+`WaitForSingleObject(0)`
  判活、"Windows 上 PID 复用更频繁"——无本仓库证据。
  **可被本仓库间接支持的部分**：释放顺序"先关句柄再删文件"（`src/lib/fs.js:266-267` 先 close 再 readFile/unlink ✓）、
  NFS/SMB 上 `rename` 原子性假设失效（分片自标未能确认 ✓）。
- **[U2] S1b §3（L120-122）**：`yield_now()` 与 `setImmediate` 的语义差异、`tokio::time::sleep(ZERO)`、
  NTFS 100ns / Linux 秒级时间戳粒度——无本仓库证据（`src/lib/async-queue-reader.js:3,79` 只证明 Node 侧用 `setImmediate`）。
- **[U3] S1b §4（L162-165）**：`MoveFileExW(MOVEFILE_REPLACE_EXISTING)`、`File::sync_all()` 的掉电必要性、
  `renameat2(RENAME_EXCHANGE)`——无证据（分片已自标未能确认）。
- **[U4] S1b §5（L196-197）**：Rust `CrossesDevices`、`OpenOptions::mode(0o600)`、Windows ACL——无证据；
  "mode 只在创建时生效"本仓库有注释支持（`src/lib/fs.js:15-17` ✓）。
- **[U5] S1b §6/§7（L240、L285）**：Linux 目录 `fsync` 持久化 rename、`blake3` 摘要、
  "把守卫写成 `Result<RepairDecision>`"——无证据。
- **[U6] S1b §9（L370-372）**：`file_index()` = Node `st.ino` 来源、`volume_serial_number()`、
  "跨 WSL UNC 或 SMB 时 `file_index()` 可能为 0"——无本仓库证据（源码只显示 `st.ino` 与 `sha256(前 64KB)+size` 两条身份路径，
  见 `src/lib/rollout.js:937-952`）。
- **[U7] S1b §11（L448-450）**：`File::seek/read_exact`、"幂等性主要靠云端整行 upsert"——本地只能证到注释层面
  （`src/commands/sync.js:249-256` 称 ingest 是 whole-row upsert ✓），云端实现不在仓库。
- **[U8] S1b §12（L476）**：`std::time::Instant/SystemTime` 的分工——无证据。
- **[U9] S1b §15（L586）**：Windows `CREATE_NEW_PROCESS_GROUP/DETACHED_PROCESS`、Unix `setsid`——无证据；
  Node 侧行为有源码支持（`init.js:1563-1573` `detached+windowsHide+stdio:'ignore'+unref` ✓）。
- **[U10] S1b §17（L667-668）**：`rusqlite` + `VACUUM INTO` 失败回退——无证据；
  "双前缀都要试"有源码支持（`src/lib/wsl-probe.js:193-201` ✓）。
- 另：S1 §8 第 6、7 条（Claude `Stop` 的真实触发粒度、CodeBuddy/WorkBuddy 是否真消费 `settings.json` 的 `SessionEnd`）
  经复核**仍为 U**：仓库内只有事件字符串与"fork 同 schema"注释（`claude-config.js:7-8`、`init.js:623-625`），无第三方 schema 可判定。

## 行号漂移统计

- 抽样中行号完全正确的比例：**135/139 ≈ 97.1%**（4 条行号类问题：E2、E5、E6、E7）
- 未发现 CHECKLIST §0 所述 GBK 解码型整体漂移：所有抽验行号都是 ±0 精确命中（例：`rollout.js:4221` 不需偏移即命中，
  本次用 `ReadAllLines(...,UTF8)` 与 `read` 工具双口径互证）
- 发现漂移/错配的位置（分片 → 实际）：
  - S1 L187 / S1b L546：`init.js:1445-1574` → `1445-2232`（模板 `1453-2231`）
  - S1 L609：`rollout.js:1462-1516`（Kilo）→ `rollout.js:9371-9471`（KiroCli）
  - S1 L993：`:99-104`（缓存命中）→ `:35`（注释 `:12-15`）
  - S1 L905：`wsl-probe.js:149`（非 win32 返回 native）→ `install-resolver.js:21-23`

## 复核到的行号抽样（证明真的打开过）

格式：分片位置 → 源码位置 → 结论。E/I/U 已在上面详列，下表只列抽验点（含全部 E/I 的原始断言位置）。

### S1（分片行号 / 源码 / 结论）

| # | 分片 | 断言要点 | 源码位置 | 结论 |
| --- | --- | --- | --- | --- |
| 1 | L25 | `sync.js` 7251 行 / 288151 B | `src/commands/sync.js` 全文计数 | ✓ |
| 2 | L26 | `rollout.js` 24932 行 / 942266 B | 同上 | ✓ |
| 3 | L27 | `status.js` 1729 行 / 76611 B | 同上 | ✓ |
| 4 | L28-29 | `cli.js` 107/4060；`init.js` 2427/82654 | 同上 | ✓ |
| 5 | L30 | `bin/tracker.js` 60 行 | 同上 | ✓ |
| 6 | L32 | `src/` 128 个文件 | `git ls-files src`=112 | **E1** |
| 7 | L49-61 | 数据目录四路径 | `src/lib/tracker-paths.js:4-12` | ✓ |
| 8 | L63 | 队列/游标/锁在 `trackerDir` 下 | `src/commands/sync.js:568-573` | ✓ |
| 9 | L73-79 | `--debug` 剥离 | `bin/tracker.js:5,12-13` | ✓ |
| 10 | L81-90 | 代理 `unprotected` → `exit(1)` | `bin/tracker.js:15-40`（:36-39） | ✓ |
| 11 | L94 | 统一错误出口 | `bin/tracker.js:42-59` | ✓ |
| 12 | L98 | 静态 switch、无 commander | `src/cli.js:12-66` | ✓ |
| 13 | L122 | `Unknown option:` | `src/commands/sync.js:3742` | ✓ |
| 14 | L129 | 首次自动 `init --yes` / `--no-sync` / 60s 轮询 | `src/commands/serve.js:69,80-90,106-115,257-260` | ✓ |
| 15 | L132-139 | 子命令→实现文件／入口符号 | `sync.js:516`；`cli.js:100-101` | ✓ |
| 16 | L143-149 | help 三行原文 | `src/cli.js:93,95,97` | ✓ |
| 17 | L163 | `notify-command.js` 38 行 | 全文 | ✓ |
| 18 | L169-179 | `buildNotifyCommand` 平台分支 | `src/lib/notify-command.js:13-19` | ✓ |
| 19 | L181 | Windows 需附加 `shell: powershell` | `src/lib/claude-config.js:176-179` | ✓ |
| 20 | L182 | `quotePosix` 转义集 | `src/lib/notify-command.js:7-11` | ✓ |
| 21 | L187 | notify.cjs 模板范围 | `src/commands/init.js:1445-2232` | **E2** |
| 22 | L189 | 解析 `--source` / `payloadArgs` | `src/commands/init.js:1461-1476` | ✓ |
| 23 | L190 | signal 落盘路径 | `src/commands/init.js:1448,1496-1498` | ✓ |
| 24 | L191-209 | 20s 节流 + 双路径 spawn（代码块逐字） | `src/commands/init.js:1519-1536` | ✓ |
| 25 | L211 | `spawnDetached` 参数 | `src/commands/init.js:1563-1573` | ✓ |
| 26 | L212 | 链式原 notify + 显式不链式名单 | `src/commands/init.js:1538-1559`（:1540-1547） | ✓ |
| 27 | L213 | 无条件 `process.exit(0)` | `src/commands/init.js:1561` | ✓ |
| 28 | L214 | 调试日志与 1 MB 上限 | `src/commands/init.js:1488-1517` | ✓ |
| 29 | L222 | Claude：`Stop`+`SessionEnd`、安装点 | `init.js:724-728`；`claude-config.js:7-16,116-118` | ✓ |
| 30 | L223 | CodeBuddy 仅 `SessionEnd` | `init.js:1039-1043`；`:627-628`；`claude-config.js:10-12` | ✓ |
| 31 | L224-225 | WorkBuddy / WorkBuddy AI | `init.js:1052-1056,1064-1068`；`:631-632,637-638` | ✓ |
| 32 | L226 | Gemini 事件 + matcher | `init.js:735-739`；`gemini-config.js:8-10,103-106` | ✓ |
| 33 | L227 | Codex notify 数组（无 `--source`） | `init.js:689-694`；`codex-config.js:9-17,271-309` | ✓ |
| 34 | L228-229 | acode / Every Code（`CODE_HOME`） | `init.js:707-712,1133-1138`；`codex-config.js:18-24` | ✓ |
| 35 | L230 | Grok 专用 handler、hook 文件名 | `init.js:906`；`grok-hook.js:6,34-58,68-99,307-317` | ✓ |
| 36 | L231 | omp 扩展：三事件、12s 自限速 | `init.js:806`；`omp-hook.js:16-17,52,66-67,86,117,135-143` | ✓ |
| 37 | L232-233 | OpenClaw session plugin / legacy 移除 | `init.js:1078-1083,1120-1128`；`openclaw-session-plugin.js:37-84,52-57,200-224,363-399` | ✓ |
| 38 | L234 | OpenCode 插件路径与 shell 调用 | `init.js:744-747`；`opencode-config.js:7-9,11-22,24-66,36-40` | ✓ |
| 39 | L235 | Cursor 不写配置 | `init.js:758-776` | ✓ |
| 40 | L239 | `SUPPORTED_PROVIDERS` 45 项 + 17 个被动名单 | `init.js:119-165`；`:785,838,846,854,863,873,881,891-897,926,933,943,951-955,962-966,974-978,986-990,1002-1006,1019-1030` | ✓ |
| 41 | L240 | `applyIntegrationSetup`/`repairRuntimeIntegrations` | `init.js:674-1149`；`:525-602`（抽样 :590-605） | ✓ |
| 42 | L246 | Gemini matcher 原文 | `src/lib/gemini-config.js:10` | ✓ |
| 43 | L248 | OpenCode "hook 只是让 sync 更早" 注释 | `src/lib/opencode-config.js:55-56` | ✓ |
| 44 | L254 | `notifyIdentity` 三段判定 | `src/lib/notify-command.js:23-36` | ✓ |
| 45 | L255 | `normalizeEntriesForCommand` 就地修复 + scope 去重 | `src/lib/claude-config.js:181-230` | ✓ |
| 46 | L256 | 写前备份 `.bak.<ISO>` | `claude-config.js:248-262`；`gemini-config.js:265-279` | ✓ |
| 47 | L257 | 手工 TOML 行编辑 | `src/lib/codex-config.js:245-269,271-309` | ✓ |
| 48 | L258 | 强制 `tools.enableHooks=true` | `src/lib/gemini-config.js:147-152` | ✓ |
| 49 | L263-276 | passive 判定（代码块逐字） | `src/lib/passive-mode.js:189-204` | ✓ |
| 50 | L278 | `dirHasFile` 一层浅递归、吞 readdir 失败 | `src/lib/passive-mode.js:49-68` | ✓ |
| 51 | L279 | 失败分类 ENOENT/EACCES/EROFS | `src/lib/passive-mode.js:70-81`；注释 `:9-15` | ✓ |
| 52 | L280 | passive 覆盖 8 个 provider | `src/lib/passive-mode.js:96-185`（name 98/110/123/134/146/157/167/178） | ✓ |
| 53 | L281 | "hook 缺失不失败" 注释 | `src/lib/passive-mode.js:22-25` | ✓ |
| 54 | L283 | 与 init 无调用关系；仅 status 引用 | `status.js:44,1021,1248,1448,1688` | ✓ |
| 55 | L285-286 | init.js 无 EROFS/WSL 分支；`probeFile` 降级 | `init.js` 零命中；`:2244-2255`；`renderSkipDetail :1327-1332` | ✓ |
| 56 | L287 | Grok 是唯一自带 try/catch 的 provider | `init.js:902-916`（674-1160 内唯一 try） | ✓ |
| 57 | L288 | serve 只警告后继续 | `src/commands/serve.js:84-89,92-103` | ✓ |
| 58 | L294-301 | init 开关表 8 项 | `init.js:1423-1428,191-193,210-221,679,796-804,195-208,339,176,234,359,2271,2330` | ✓ |
| 59 | L305-309 | 备份/还原矩阵、`restoreNotify` 把关 | `codex-config.js:89-104,110-133`；`grok-hook.js:85-91`；`omp-hook.js:17,222-281,388-395`；`openclaw-session-plugin.js:154-178,217-222` | ✓ |
| 60 | L313-317 | 三范式（Claude/Codex/哨兵） | `claude-config.js:39,143-156`；`codex-config.js:40-58,76,107`；`omp-hook.js:17,388-395`；`opencode-config.js:8,73-75` | **E3（opencode 项）** |
| 61 | L323 | `cmdSync` 体量 + exports 边界 | `sync.js:516,3709,3821-3857` | ✓ |
| 62 | L329-351 | 23 步骨架（抽样 6-23 步） | `sync.js:645-661,713-735,742-757,759-862,864-896,898-934,1046-1099,1004-1133,1135-1220,1222-1330,1337-3300,3380-3423,3424-3460,3467-3652,3654-3704,3705-3708` | ✓ |
| 63 | L353-366 | 锁三档 + 常量（代码块逐字） | `sync.js:428-482,295-300` | ✓ |
| 64 | L371-385 | 逐 provider 解析调用行（全表 40 项） | `sync.js:1105,1201,1258,1315,1361,1375,1419,1467,1471,1509,1545,1549,1599,1642,1686,1701,1722,1776,1819,1855,1879,1897,1916,1935,1960,1994,2005,2024,2063,2074,2091,2124,2173,2200,2280,2325,2358,2379,2478,2516,2544,2594,2639,2674,2708,2747,2780,2809,2837,2865,2890,2960,3053,3108,3216,3282` | ✓ 全中 |
| 65 | L396-400 | 双队列写入函数/状态文件/读侧消费者 | `rollout.js:3169,3489`；`local-api.js:145,202` | ✓ |
| 66 | L405-425 | 两条读侧 key 公式（代码块逐字） | `local-api.js:202-211,145-153` | ✓ |
| 67 | L428 | 后写覆盖先写（`seen.set`） | `async-queue-reader.js:55,86` | ✓ |
| 68 | L430-451 | 队列行字段（代码块逐字） | `rollout.js:3328-3345` | ✓ |
| 69 | L453 | 控制行两类，`account_sync_watermark` 读侧丢弃 | `sync.js:4497` | ✓ |
| 70 | L457 | append 写点（"唯一写点"） | `rollout.js:3483`（另有 :20031,:22594；`sync.js:5126,5207,5281`） | **E4** |
| 71 | L459 | `bucket.queuedKey` 仅内存、commit 才落盘 | `rollout.js:3346`；`sync.js:552-559` | ✓ |
| 72 | L462 | compact 竞态防护（写 tmp→fsync→关句柄→stat 校验→rename） | `upload-queue.js:94-121,73-78` | ✓ |
| 73 | L466-474 | 游标单次提交 + `skipNoOpCursorCommit` | `sync.js:3444-3446,3424-3437,3456-3460` | ✓（判据列举不全见 I4） |
| 74 | L478-488 | 上传批/预算/节流约束 | `sync.js:4315-4413,4313,4329,3563,168,4362,4392,4397-4400,4447-4449,4510,4478-4484,162-167,3517-3534,3529-3534` | ✓ |
| 75 | L492-496 | `parseArgs` 全集 + auto/background 语义 | `sync.js:3711-3745,719-735,375-378,367-373,3747-3753,310-319` | ✓ |
| 76 | L504-528 | 游标两代 + 16 MB 阈值 + 目录布局 | `cursor-store.js:25-107,21,77-86,199-201,177-197,1816-1818,13-21,663-726` | ✓ |
| 77 | L547-557 | commit 九步（含 clone/脏分片/事件集/core/manifest/三代/回滚） | `cursor-store.js:855-1016,1820-1822,877-881,1349-1362,899-921,923-945,953-963,1744-1762,987,1005-1009,1010-1015,124-139` | ✓ |
| 78 | L561-577 | `previous` 兼容字段（代码块逐字） | `cursor-store.js:974-986` | ✓ |
| 79 | L581-586 | 分片归属/键/白名单 + `codexRoots` 同源 | `cursor-store.js:1771-1779,1788-1793,1800-1805,1807-1809,299-318,609-639`；`sync.js:635-652` | ✓ |
| 80 | L590-609 | 三套游标字段 | `rollout.js:632-665,1192-1206,1289-1307`（Kiro 见 E5） | **E5** |
| 81 | L613-647 | 起点判定 + 稳态短路 + 项目上下文单扫（两段代码块逐字） | `rollout.js:487-518,538-555,520-537,589-604` | ✓ |
| 82 | L651-682 | 三重威胁与去重层 | `rollout.js:292-295,428-461,302-380,381-391,4914-4919,1233-1237,937,1070-1105,1057-1069,287-290,646-655` | ✓ |
| 83 | L686-694 | 冷扫描审计双阈值 + 时钟回拨 | `sync.js:3763-3783,294,301,3772,3785-3819`；`cursor-store.js:640-660` | ✓（字段集见 I3） |
| 84 | L698-745 | 迁移键表 16 项 + 翻车史 + 守卫 | `sync.js:204-309,211-248,258-268,1147-1192,6294-6315,6316-6346,6347-6370,6745` | ✓ |
| 85 | L749-752 | 损坏恢复路径（三代回退、分片完整性、RETRY/CORRUPT） | `cursor-store.js:124-139,72-74,1466-1471,1473-1481,44-58`；`sync.js:1069-1085,1086-1087` | ✓ |
| 86 | L760-789 | 四个调度源 + serve 轮询四要点（代码块逐字） | `serve.js:35,294-366,303-309,339-345,248-256,311-315,335-342,266-273` | ✓ |
| 87 | L795-808 | `sync.skip.json` marker 与 `lock_busy`/`lock_debris` 区分（代码块逐字） | `sync.js:484-506,565`；`status.js:285,351-353,1058` | ✓ |
| 88 | L812-827 | 自动重试 marker/上限/no-spawn 开关（代码块逐字） | `sync.js:4169-4228,4309,4310,4278-4285,4244-4247,4186-4202,4217-4219,3654-3676`；`runtime-config.js:69-75` | ✓ |
| 89 | L833-845 | env 开关表（抽查 8 项） | `wsl-probe.js:18-24,106-123`；`serve.js:295-309,341`；`local-api.js:761,2282`；`sync.js:543-545,3490-3492,3514-3523`；`openclaw-cli.js:58` | ✓ |
| 90 | L849-856 | 单源收敛 + 两层白名单 | `sync.js:3747-3753,719-735,320-373` | ✓ |
| 91 | L876-893 | scan-roots 问题定义/合并顺序/相对路径/配置形状/可列性 | `scan-roots.js:3-14,19-23,163-202,46-55,158-162,228-232,25-27,89-110` | ✓ |
| 92 | L897-907 | WSL 探测七机制 | `wsl-probe.js:18-24,26-55,91,57-97,69-80,180-204,106-109,125-159,206-242,244-262`（`:149` 误引见 E7） | **E7** |
| 93 | L911-936 | `resolveInstallPaths`（代码块逐字）+ union 调用点 | `install-resolver.js:19-45,6-13`；`sync.js:761-772,782-787,815-820` | ✓ |
| 94 | L940-971 | `multiInstallParse`（代码块）+ 三条不变量 | `multi-install-parser.js:8-59,13-22,61-85,49-54,92-111` | ✓ |
| 95 | L975-983 | 去重五层 | `scan-roots.js:116-146,204-224`；`sync.js:864-870`；`rollout.js:1056-1105,292-295,4914-4919,428-461`；`sync.js:249-256` | ✓ |
| 96 | L991-999 | `createAsyncQueueReader` 七点 | `async-queue-reader.js:8-10,33,40-47,64-69,87-91,5,79,51-57,95,109-114,16,94,102-103`（`:99-104` 误引见 E6） | **E6** |
| 97 | L1005-1012 | `physicalJsonlRecords` 六能力 | `jsonl-lines.js:3-11,57,12-18,61-63,96-98,65-73,99-107,111-114,20-27,34-36,84-90,22-27,127` | ✓（`stripCr` 见 I6） |
| 98 | L1018-1020 | 上传侧读取契约 | `upload-queue.js:22-43,20-21,29`；`sync.js:4449,4489,4512,4433-4441` | ✓ |
| 99 | L1024-1045 | `normalizeQueueRow`（代码块逐字）+ 不变量 + issue #106 | `local-api.js:170-200,155-168`；`sync.js:4509` | ✓ |
| 100 | L1049-1052 | 读取面防护（body 上限/头像/120s/白名单/端口） | `local-api.js:722-740,46-47,22,778-792,754-762`；`serve.js:14-22,679-714` | ✓ |

### S1b（分片行号 / 源码 / 结论）

| # | 分片 | 断言要点 | 源码位置 | 结论 |
| --- | --- | --- | --- | --- |
| 101 | L12-19 | 锁四常量（代码块逐字） | `src/lib/fs.js:69-74` | ✓ |
| 102 | L21-29 | `wx` 独占 + token 心跳（代码块逐字） | `src/lib/fs.js:284-306`；`heartbeatPathFor :88-91` | ✓ |
| 103 | L31 | 心跳 30s、utimes 两文件、unref | `src/lib/fs.js:206-224` | ✓ |
| 104 | L32 | 回收判定（ESRCH/EPERM + 心跳/老格式 mtime） | `src/lib/fs.js:105-144`（:93-103 判活） | ✓ |
| 105 | L33 | `.reclaim` 串行 + 持闸重校验（TOCTOU） | `src/lib/fs.js:342-368` | ✓ |
| 106 | L34-43 | 释放持闸 + token 匹配才 unlink（代码块逐字） | `src/lib/fs.js:264-275` | ✓ |
| 107 | L45 | 迭代清扫嵌套 `.reclaim`（issue #431） | `src/lib/fs.js:178-204` | ✓ |
| 108 | L53 | "先关句柄再删文件" | `src/lib/fs.js:265-268` | ✓ |
| 109 | L60-77 | 目录"存在=可列"（代码块逐字） | `src/lib/scan-roots.js:89-110` | ✓ |
| 110 | L79 | "unreadable: CODE" 展示 | `src/lib/scan-roots.js:248-253` | ✓ |
| 111 | L91-107 | 签名 + 锚点续读（两段代码块逐字） | `src/lib/async-queue-reader.js:8-10,40-47` | ✓ |
| 112 | L112-114 | 完整行才推进/残行不入 map/64KB 让出/并发合并/LRU 4/失败回退 | `async-queue-reader.js:64-70,87-91,3,79,109-114,16,94,102-103` | ✓ |
| 113 | L128-139 | 压缩前置条件（代码块逐字） | `src/lib/upload-queue.js:73-78` | ✓ |
| 114 | L141-154 | 竞态校验与原子替换（代码块逐字） | `src/lib/upload-queue.js:111-121` | ✓ |
| 115 | L156 | 保留语义 + `verifiedMs` 严格更大 | `src/lib/upload-queue.js:79-91,105-110` | ✓ |
| 116 | L171-188 | `writeFileAtomic`（代码块逐字） | `src/lib/fs.js:9-33` | ✓ |
| 117 | L190 | `updateJsonLocked` 读改写锁、超时 30s | `src/lib/fs.js:416-443` | ✓ |
| 118 | L197 | `mode` 仅创建时生效 | `src/lib/fs.js:15-17` | ✓ |
| 119 | L204-212 | v2 常量（代码块逐字） | `src/lib/cursor-store.js:13-23` | ✓ |
| 120 | L214-229 | commit 尾段（代码块逐字） | `src/lib/cursor-store.js:855-989`（:974-988） | ✓ |
| 121 | L231-234 | 失败清理/三代保留/回退/完整性/legacyFingerprint | `cursor-store.js:1010-1015,1005-1009,124-139,1466-1471,1473-1481,44-58` | ✓ |
| 122 | L236-242 | `COPYFILE_FICLONE` 被写成"硬链接/复制二选一" | `src/lib/cursor-store.js:1357-1362`；`unlink :907` | **E8** |
| 123 | L241 | `safeShardFilename` 白名单 | `src/lib/cursor-store.js:1807-1809` | ✓ |
| 124 | L246 | 21.7 亿 token 事故 | `src/commands/sync.js:234-248`（:239 "wiped 2.17B claude tokens"） | ✓（与 S1 §4.8 的 "2.17B" 同一事件、同一处原文，量级自洽） |
| 125 | L252-259 | Codex #187 守卫（代码块逐字） | `src/commands/sync.js:258-268` | ✓ |
| 126 | L263-275 | Claude 根可用性守卫（代码块逐字） | `src/commands/sync.js:1147-1192`（:1167-1175） | ✓（提示仅 `!opts.auto` 打印，见 I7） |
| 127 | L277 | 快照校验/覆盖检查/廉价预检 | `sync.js:6271-6293,6294-6315,6316-6346,6347-6370` | ✓ |
| 128 | L279 | 迁移键幂等闸 + sentinel | `sync.js:952-959`（:956-957） | ✓ |
| 129 | L286-287 | deferred 提示 + 云端整行 upsert | `sync.js:1177-1181,249-256` | ✓ |
| 130 | L293-305 | 去重双视图（代码块逐字） | `src/lib/rollout.js:302-380` | ✓ |
| 131 | L309-321 | 历史视图决策（代码块逐字）+ 四条件 | `src/lib/rollout.js:428-461`（:456-460 兜底见 I5） | ✓ |
| 132 | L325-326 | 两套身份键 + 10 万条截断 | `rollout.js:292-295,4914-4919,1233-1237`；`cursor-store.js:681-726`；`sync.js:4478-4484` | ✓ |
| 133 | L341-364 | `duplicateOf` 快路径（代码块逐字）+ native 优先 | `rollout.js:1056-1107,1074-1077`；`sync.js:683-684` | ✓ |
| 134 | L380-399 | 双队列 append（代码块逐字）+ 只读主队列（代码块逐字） | `rollout.js:683-686`；`sync.js:3654-3659` | ✓ |
| 135 | L389 | 项目行只多两字段、无 `model` | `rollout.js:3515-3532` | ✓ |
| 136 | L409-401 | 两个独立 reader 实例 | `local-api.js:145,202` | ✓ |
| 137 | L417-442 | 上传主循环/只发完整行/控制行计入上限/上限链（代码块逐字） | `sync.js:4315-4413,4447-4449,4488-4490,4511-4512,4313,3564,4392,4360-4362,4402-4411,4433-4441,4368` | ✓ |
| 138 | L448-450 | 字节 offset 声明 + `upload_id` + "服务端 OK 后才推进" | `upload-queue.js:20-21`；`sync.js:4368,4397-4400` | ✓ |
| 139 | L458-467 | 审计判定（代码块逐字） | `sync.js:3763-3819,294,301,3456-3460` | ✓ |
| 140 | L486-501 | `normalizeQueueRow` 读时纠错（代码块逐字）+ 读/上传共用 | `local-api.js:155-200,191-198`；`sync.js:4504-4509` | ✓ |
| 141 | L517-531 | `recordSyncSkip`（代码块逐字）+ 抢锁后清 marker | `sync.js:484-506,565`；`status.js:285,1058,1054-1057` | ✓ |
| 142 | L548-578 | notify.cjs 三件套（代码块逐字） | `init.js:1446-1447,1495-1498,1519-1536,1563-1573`；范围见 E2 | **E2** |
| 143 | L580 | ④ 链式 ⑤ `exit(0)` | `init.js:1538-1559,1561` | ✓ |
| 144 | L596-616 | 三范式 + `notifyIdentity`（代码块逐字）+ 备份矩阵 | `notify-command.js:23-36`；`claude-config.js:181-230,248-262`；`gemini-config.js:265-279`；`codex-config.js:89-104,245-309`；`omp-hook.js:17,388-395`；`opencode-config.js:8,73-75`；`grok-hook.js:85-91` | **E3（opencode 项）** |
| 145 | L632-644 | `requireAnyChild` 折进探测（代码块逐字）+ union 契约 | `install-resolver.js:25-36,6-18`；`sync.js:766-772` | ✓ |
| 146 | L646-660 | `mapWslCwdToUnc`（代码块逐字） | `src/lib/wsl-probe.js:232-242` | ✓ |
| 147 | L667-668 | 双前缀尝试 + SQLite 快照 | `wsl-probe.js:193-201,244-262` | ✓ |
| 148 | L677-696 | 依赖注入缝 + `failureInjector`（两段代码块逐字） | `wsl-probe.js:57-63`；`cursor-store.js:25-32,972`；`sync.js:516-538,526-531` | ✓ |
| 149 | L704-719 | 别名表 + 双层白名单（代码块逐字） | `sync.js:310-373`；`normalizeSyncSource :3755-3762` | ✓ |
| 150 | L727 | `skipNoOpCursorCommit` "9 个变量" | `sync.js:3424-3437`（13 条件项） | **E9** |
| 151 | L728 | `.reclaim` 迭代清扫自愈（issue #431） | `src/lib/fs.js:178-204` | ✓ |
| 152 | L729 | 队列写入无写侧互斥 | `rollout.js:3483`（写点不唯一，见 E4；无锁结论不变） | ✓（前提不全） |
| 153 | L730 | 0.26.3 事故 + generations 替代 | `sync.js:234-248`；`cursor-store.js:855-1016` | ✓ |
| 154 | L731 | Grok 无条件覆盖、注释原文 | `src/lib/grok-hook.js:85-91` | ✓ |
| 155 | L732 | 三套并行纠错机制 | `sync.js:249-256`；`local-api.js:170-200` | ✓ |
| 156 | L733 | 体积驱动切换 + legacy 能力退化 | `cursor-store.js:21,77-86,165-197` | ✓ |
| 157 | L734 | `legacyGroups`/`groupQueued` 遗留路径 | `rollout.js:3181-3194,3430-3480`；`sync.js:216-226` | ✓ |
| 158 | L735 | 代理错误令进程 `exit(1)` | `bin/tracker.js:36-40` | ✓ |
| 159 | L736 | `local-api.js` 3543 行 / 156 KB | 实测 3543 行 / 155946 B | ✓ |
| 160 | L737 | 测试钩子进入生产签名 | `sync.js:519-538` | ✓ |
| 161 | L738 | 依赖 `st.ino` 检测替换 | `rollout.js:489-492` | ✓ |
| 162 | L739 | 7680/7681 绕行 | `serve.js:15-21` | ✓ |
| 163 | L740 | hookStatus 传参与消费不一致 | `status.js:1030-1031` vs `passive-mode.js:93-186` | ✓ |

## 附录 A：CHECKLIST §3 中与本分片相关的 B 组断言判定

- **B11（队列唯一写点是 `rollout.js` 的 `fs.appendFile`、无独立写锁）**：**部分成立**。
  "无独立写锁" ✓（所有写点都假定调用方持 `sync.lock`：`upload-queue.js:62` 注释明写 "Caller holds sync.lock"）。
  "唯一写点" ✗ —— 见 E4，另有 `rollout.js:20031,22594` 与 `sync.js:5126,5207,5281` 三处 repair 也 append 主队列；
  崩溃窗口留重复行的推论不变，但影响面比 S1 描述的大（repair 路径也参与）。
- **B12（游标两代并存：单文件 `cursors.json` < 16 MB vs `cursor-store-v2/` 代际存储）**：**成立**。
  `DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024`（`cursor-store.js:21`）、激活分支 `:44-58,77-86`、
  manifest/generations 结构 `:13-21,1820-1826`、三代保留 `:1005-1009`、manifest 原子切换 `:987`——
  与 S1 §4.1/§4.2 描述逐条一致；"整代重建 + manifest 原子切换"经 commit 全段核对属实（`cloneGenerationFiles` 用 CoW/复制，
  见 E8 的措辞修正）。
- **B13（0.26.3 repair 的 token 丢失量级）**：**自洽**。S1 §4.8 引 `sync.js:211-248`、S1b §7 引 `sync.js:234-248`，
  两处命中同一段注释，原文为 `On the reporter's machine this wiped 2.17B claude tokens (-1.27B opus-4-7, -474M opus-4-6,
  -376M sonnet-4-5, -48M haiku, -6M sonnet-4-6)`（`src/commands/sync.js:239-240`）。
  "2.17B" 与 S1b 的 "21.7 亿" 为同一数字的两种写法，**不存在不一致**；两分片指向同一事件（v6 / 0.26.3 / 0.26.4 回退到 v4）。
  分片 S1 另注"历经 v1–v6 的翻车史"与 `:211-247` 注释的 v1/v2/v3/v4/v6 段落一致（v5 未出现在注释中）。
