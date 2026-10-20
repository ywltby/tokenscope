# OPEN-W4 运行时只读实验报告

针对主文档 `_part6.md` §12「A. 需运行时或真实语料才能判定」中 8 条存疑项（A1 / A4 / A5 / A8 / A11 / A12 / A16 / A17），用 `node` 直接加载样本仓库的纯函数与解析器，以合成 fixture 做最小化只读实验。

**判定总览：8 条全部已判定**（A11 / A12 在「真实语料出现占比」这一维度仍属未覆盖，见各节限定）。

## 实验环境与纪律自证

- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0`（2026-10-13）。实验前后 `git status --porcelain` 输出为空，仓库零改动。
- 全部产物写在 `%TEMP%\tt-exp\*`（脚本 `a1/a4/a5/a8/a8b/a8c/a11/a12/a16/a17/a17b.cjs` 及各自 fixture 目录），未在样本仓内新建任何文件。
- 未联网：全程未调用 `ensurePricingLoaded`、`getModelPricing` 或任何 fetch 路径。
- 未触碰真实 agent 日志：脚本只加载样本仓模块；凡 `require` 了 `src/commands/sync.js` 的实验（A5 / A17）都在 `require` 之前把 `USERPROFILE` / `HOME` / `APPDATA` / `LOCALAPPDATA` 重定向到临时 home，并在结束后确认该目录为空（`tempHomeTouched: []`）。
- Node v24.14.0；模块加载方式 `require(".../src/lib/rollout.js")`、`require(".../src/commands/sync.js")`、`require(".../src/lib/async-queue-reader.js")`，只调用导出函数。

---

## A1 Claude 半行丢量

- **判定**：已判定
- **结论**：会丢，且在本代文件内不可恢复；半行的字节被算进游标 offset，补全后该行从中间开始被读取、永远解析失败，`claudeHashes` 里也不会有它的指纹，只有发生 inode 变化或截断（size 变小）触发全量重读时才能找回。
- **实验方法**：调 `parseClaudeIncremental({ projectFiles:[file], cursors, queuePath })`，`cursors = { files: {} }`，跑三轮；fixture 为三条同一半小时桶（`2026-10-01T00:00Z`）的 Claude assistant 行：a=100 tokens、b=500 tokens、c=7 tokens。
  ```js
  fs.writeFileSync(file, a + "\n" + b.slice(0, Math.floor(b.length * 0.6)));   // 半行
  await parseClaudeIncremental({ projectFiles: [file], cursors, queuePath });  // sync1
  fs.appendFileSync(file, b.slice(half.length) + "\n");                        // 补全
  await parseClaudeIncremental({ projectFiles: [file], cursors, queuePath });  // sync2
  ```
  对照两组：(1) 同一 inode 截断重写（`writeFileSync` 只留 b 行）；(2) 完整 JSON 但缺尾换行。
- **关键输出**：
  - sync1 后 `cursors.files[file].offset = 381`，与 `st.size` 相等（`offsetEqualsFileSize: true`），即半行字节已计入游标；桶 = 100，半行未计入。
  - sync2（补全后）`startOffset = 381`，落在 b 行中间，`eventsAggregated: 0`、`bucketsQueued: 0`，桶仍 = 100。
  - sync3 追加 c 后桶 = **107**（若 b 被计入应为 607）；`claudeHashes = ["msg-a:req-a", "msg-c:req-c"]`，无 msg-b。
  - 对照 1（截断重写，size 714 → 239）：桶 = **607**，b 被找回。对照 2（完整 JSON 缺尾换行）：sync1 直接计入（input 400），sync2 追加后 = 411，无重复。
- **对主文档的影响**：`_part6.md` §12 A1 行由「需定向测试确认」改为结论态：**已实测为确定性丢量**（非理论风险），保留原文「本代文件内不可恢复」的收窄措辞并补上实测依据（offset 等于文件大小、补全后 `eventsAggregated: 0`、桶值 107 而非 607）。同时保留非对称性对比（Codex `committedEndOffset` / OpenClaw `openclawOffsetEndsAtLineBoundary`），并补一句：缺尾换行但 JSON 完整不受影响，危害仅限「JSON 未闭合」的半个 JSON 对象。

## A4 `legacyGroups` / `groupQueued` 分支

- **判定**：已判定
- **结论**：不是死代码，命中该分支会写队列行；模型名只有在该分组内出现 ≥2 个不同模型名时才退化为 `unknown`，组内单一模型名时沿用真实模型名；语义是把同一 (source, 半小时) 的所有模型桶累加合并成一行。
- **实验方法**：三组 fixture 都通过导出的 `parseClaudeIncremental` 间接驱动 `enqueueTouchedBuckets`（该函数本身未导出），每组一个含 500 tokens 的 Claude 行，写入同一小时桶。
  ```js
  // (a) v1 游标：旧键 `claude|<hour>` + bucket.queuedKey
  cursors.hourly = { version: 1, buckets: { ["claude|<hour>"]: { totals, queuedKey: "legacy-key-1" } } };
  // (b) v3 游标：显式 groupQueued 命中，组内仅一个真实模型名
  cursors.hourly = { version: 3, buckets: {...}, groupQueued: { ["claude|<hour>"]: "legacy-key-2" } };
  ```
- **关键输出**：
  - (a) v1 → `normalizeHourlyState` 升为 v3，`buckets` 键归一为 `claude|unknown|<hour>`，并据 `queuedKey` 重建 `groupQueued`；随后**写出 1 行** `{source:"claude", model:"unknown", input_tokens:600, output_tokens:60, conversation_count:3}`（旧 100 + 新 500 合并），`groupQueued` 被更新为合并后 totals 的 key。
  - (b) 组内单模型名 → **写出 1 行**，`model: "claude-opus-5-5"`（非 `unknown`），input 600。
  - (c) 组内两个真实模型名（`claude-opus-5-5` + `grok-4.5-build`）→ **写出 1 行**，`model: "unknown"`，input 700。
- **对主文档的影响**：§12 A4 行由「是否死代码待定」改为：**该分支活着且会落盘**。措辞建议：「对含 `groupQueued` 的 v1/v3 游标，该分支会把该 (source, 半小时) 的所有模型桶合并为一条 `model=unknown` 行（仅当组内模型名 ≥2 时）；单模型组的模型名被保留」。并去掉「若不写该键则为死代码」的假设分支，同时把它从「与修复意图冲突的再膨胀路径」下调为「按 (source, 小时) 聚合的遗留写入路径，会丢失该小时的模型维度」。

## A5 `readQueueBatch` 同批同键覆盖方向

- **判定**：已判定
- **结论**：以**新行**覆盖旧行（文件顺序上后出现的行胜出），不是以旧覆盖新；同批读完后 `nextOffset` 跳过两行全部字节，旧行不会被单独上传。
- **实验方法**：调导出的 `readQueueBatch(queuePath, 0, 200)`（`sync.js:3824` 已导出）。fixture 为同键两行，旧行 900 tokens 在前、新行 400 tokens 在后；另用 `maxBuckets = 1` 复跑观察分批行为。
  ```js
  fs.writeFileSync(queuePath, row(900) + "\n" + row(400) + "\n");   // 同键：claude|claude-opus-5-5|<hour>
  const batch = await sync.readQueueBatch(queuePath, 0, 200);
  ```
- **关键输出**：
  - 同批：`buckets.length = 1`，`total_tokens = 400`（新行胜出），`nextOffset = 592 = st.size`（两行字节均被计入偏移）。
  - 分批（`maxBuckets = 1`）：第一批 `[900]`、offset 296；第二批 `[400]`、offset 592。即两行会作为**两条独立记录**各上传一次，去重责任落在服务端 upsert。
- **对主文档的影响**：§12 A5 行判定条件里的「确认是否以旧覆盖新」应改为明确结论：**新覆盖旧（last-wins）**，`readQueueBatch` 与 `readQueueData` 方向一致。真正需要提示的风险不是覆盖方向，而是「同键两行分属两批时会各自上传，最终值由云端 upsert 语义决定」,建议在此行补上这一条与 `maxBuckets` 的耦合。

## A8 `claudeHashes` 淘汰窗口

- **判定**：已判定
- **结论**：超限时保留插入顺序的最后 10 万条、丢弃最早插入的条目；被淘汰的指纹不再拦截，同一 (message.id, requestId) 在另一路径再次解析时会被**重新计入**；而承担第二层防护的 `fileId` 去重只在同一次扫描里同时见到 UNC 与本地路径时才启用，纯本地双副本完全依赖这 10 万条的 hash 层。
- **实验方法**：三段实验，全部调 `parseClaudeIncremental`。
  ```js
  cursors.claudeHashes = Array.from({length: 100001}, (_, i) => `seed-${i}`);   // 已满 +1
  await parseClaudeIncremental({ projectFiles: [fileA], cursors, queuePath });   // 触发裁剪
  // 淘汰后：另一路径携带同一 msgId 再解析
  await parseClaudeIncremental({ projectFiles: [fileB], cursors, queuePath });   // fileB: msgId = seed-0
  ```
  另有 UNC 混扫实验（`\\localhost\C$\...` 与本地路径指向同一文件）与端到端闭环实验（同内容双副本 + 10 万条 filler 灌满后重扫副本）。
- **关键输出**：
  - 裁剪：100001 → **100000**，`seed-0` / `seed-1` 被丢弃（`firstElement = "seed-2"`），尾部与新指纹保留在末尾。
  - 淘汰后重计：未淘汰的 `seed-100000` 在新文件里 `eventsAggregated: 0`（去重仍生效）；已被淘汰的 `seed-0` 在新文件里 `eventsAggregated: 1`（**重新计入**）。
  - fileId 层：本地 + UNC 指向同一文件 → `filesProcessed: 1`，第二路径游标写 `duplicateOf`，桶 = 400（非 800，未双计）；纯本地双副本 → 游标 `hasFileId: false`（fileId 层未启用），仅靠 hash 层兜住（`eventsAggregated: 1`，桶 = 400）。
  - 端到端闭环：副本 A 首解析（`dup-x` 入集合）→ 副本 B 被去重（0）→ 灌入 10 万条 filler 触发裁剪（`dup-x` 被挤出，100002 → 100000）→ 副本 B 重扫 **`eventsAggregated: 1`，桶 = 400 → 双计成立**。
- **对主文档的影响**：§12 A8 行由「构造用例验证是否双计」改为结论态，并精确化触发条件三件套：**历史消息数超过 10 万** + **存在同内容副本（备份 / 同步工具 / 多份拷贝）** + **两副本不在同一次扫描里以 UNC+本地混合形态出现**（否则 `fileId` 层会拦住，实测有效）。措辞建议补一句：`fileId` 第二层并非通用防线，`sawUncFile && sawLocalFile` 为假时 `hasFileId` 恒为 false，本地双副本的去重全部押在 10 万条 hash 上。

## A11 qoder(new) 全零 token 行是否入队

- **判定**：已判定（「队列里确实会出现六列全 0 + `conversation_count: 1` 的行」已实测成立；真实 qoder 日志中该形态的出现频率未测）
- **结论**：会出现。只要 `usage` 对象存在且 `usage.billable !== false`，即使没有任何权威 token 字段，该消息也会带着全零 totals 与 `conversation_count: 1` 进入桶并写出队列行，模型名取真实模型（非 `unknown`）。
- **实验方法**：调导出的 `parseQoderNewIncremental({ sessionFiles:[f], cursors, queuePath })`（默认 `sourceKey = "qoder"`、`cursorKey = "qoderNew"`），fixture 为单行 JSONL `{type:"assistant", sessionId, cwd, timestamp, message:{id, role:"assistant", model:"qmodel_38max", usage}}`，三组 usage 形态。
  ```js
  { billable: true, credits: 3 }                                        // 无 token 字段
  { billable: true, input_tokens: 0, output_tokens: 0, ... }             // 显式全零
  { billable: false, credits: 3 }                                        // 对照
  ```
- **关键输出**：
  - 前两组：`messagesProcessed: 1, eventsAggregated: 1, bucketsQueued: 1`，队列 1 行 `{source:"qoder", model:"qmodel_38max", input/cached/cache_creation/output/reasoning/total/billable 全 0, conversation_count: 1}`。
  - 对照组：`messagesProcessed: 0, eventsAggregated: 0, bucketsQueued: 0`，队列 0 行。
- **对主文档的影响**：§12 A11 行由「是否真的入队取决于 totalsKey 比较」改为已判定：**会入队**，并注明零值行本身不会被 `pickDominantModel` / `queuedKey` 短路吞掉（`pickDominantModel` 在存在非 `unknown` 模型名时即返回该名，与 total 是否为 0 无关）。可另标一条属于 C 类的观测：全零行会污染「会话数」而不贡献 token，展示层需自行决定是否过滤。

## A12 trae-cn 全零行

- **判定**：已判定（同形 fixture 路径；真实 `query_user_usage_group_by_session` 响应是否真含全零行未测）
- **结论**：会落出全零桶行。`input_token` / `output_token` 为 0 通过全部校验（只校验 `Number.isSafeInteger(x) && x >= 0`），写入全零 totals 且 `conversation_count: 1`，并被原样写进队列；同时还会额外产出一条同样全零的 `account_session_state` 控制记录。
- **实验方法**：调导出的 `parseTraeCnApiIncremental({ sessions, cursors, queuePath, windowStartMs, windowEndMs, snapshotVerifiedAtMs })`，三组 sessions 数组。
  ```js
  { session_id:"sess-zero", model_name:"Doubao-1.5-pro", usage_time:<epoch秒>, input_token:0, output_token:0 }
  { session_id:"sess-live", model_name:"Doubao-1.5-pro", usage_time:…, input_token:1000, output_token:100, cache_read_token:200 }
  { session_id:"sess-nomodel", usage_time:…, input_token:0, output_token:0 }   // 缺 model_name
  ```
- **关键输出**：
  - 全零：`recordsProcessed: 1, eventsAggregated: 1, bucketsQueued: 1, skippedRows: 0`；队列 2 行，桶行 `{source:"trae-cn", model:"Doubao-1.5-pro", 六列全 0, conversation_count: 1}`，另 1 行 `kind:"account_session_state"` 同样全零。
  - 缺 `model_name`：模型名回落为 `trae-cn-unknown`，仍写全零桶行。
  - 对照组（有 token）：`input 800 / cached 200 / output 100 / total 1100`，cache-inclusive 剥离正确，无异常。
- **对主文档的影响**：§12 A12 行由「源码层允许，是否出现待验」改为：**fixture 已证实全零行会完整落盘**（含桶行与会话状态行各一），把剩余不确定项收窄为「真实响应里全零会话行出现的频率」。措辞建议保留「源码允许」的证据行号，并补「`skippedRows: 0`，即该形态不会被 `fail-closed` 抛错拦住」。

## A16 craft / reasonix 截断重放

- **判定**：已判定
- **结论**：截断段会被重复计入，桶累计**高于**会话真实累计；虚增额恰等于被 clamp 掉的那段差额（本轮实测 550 tokens，等于 500 input + 50 output）。两家都没有回撤路径。
- **实验方法**：三轮改写同一个累计快照（1000 → **500 截断** → 1200 升回），每轮调用一次解析器并读队列最后一行。
  ```js
  await parseCraftIncremental({ sessionFiles:[file], cursors, queuePath });        // 首行 SessionHeader 携带 tokenUsage
  await parseReasonixIncremental({ telemetryFiles:[tf], cursors, queuePath });     // <name>.jsonl.telemetry.json + .meta
  ```
  reasonix 另跑两种 usage 形态（带/不带 cache 字段）以排除列归属对结论的影响。
- **关键输出**：
  - craft：step1 桶 = 1100（input 1000 / output 100）；step2 截断时 `eventsAggregated: 0`、桶不动；step3 桶 = **1870**（input 1700 / output 170），真实累计 1320 → **虚增 550**。
  - reasonix（两种形态一致）：step1 桶 total = 1100；step2 不动；step3 桶 total = **1870**，真实累计 1320 → **虚增 550**。带 cache 字段时 input 列为 0、prompt 全额落在 `cached_input_tokens`；不带 cache 字段时落在 `input_tokens`，两者 total 结论相同。
- **对主文档的影响**：§12 A16 行由「若上游重置后又累积回原量级，该段重放差值会被再次计入」的推测改为实测确认：**会虚增，且量值等于截断差额**。建议在结论里补精确机制：`Math.max(0, current - prev)` 把负差额 clamp 为 0，同时快照无条件替换成回退后的较小值，因此截断时「多算的部分」既未扣回也无标记，回升段（1200 - 500 = 700）被全额计入，而桶里仍留着旧的 1000。该行可从 A 类（待验证）标记为「已验证 + 仍属当前版本未修」，建议同步登记进 C 类缺陷清单。

## A17 `setMaxTotal` 与 last-wins 的差异

- **判定**：已判定
- **结论**：两者确实给出不同结果（同键双行、第二行更小时，`mainUsageHourTotalsFromQueueRows` 取旧的大值 900，两个 last-wins 读取器取新的小值 400）；而「队列里出现同键数值回退」在真实路径上可达（qoder 的先减旧再加新就会写出更小的同键行），差异不只是理论。
- **实验方法**：三个读取器跑同一个队列文件。
  ```js
  const block = srcLines.slice(6528, 6589).join("\n");     // sync.js:6529-6589 逐字切片，零手抄
  const MAX = new Function(block + "\nreturn { mainUsageHourTotalsFromQueueRows };")();
  await sync.readQueueBatch(queuePath, 0, 200);            // 导出函数
  await readQueueDataEquivalent(queuePath);                // createAsyncQueueReader + local-api.js:204-210 的 parseRow
  ```
  `readQueueData` 未导出，故用同一 reader 工厂（`async-queue-reader.js` 的 `createAsyncQueueReader`）复现其 `parseRow`（键 `source|model|hour_start`，`target.set(...)` 覆盖）以代表其语义；`mainUsageHourTotalsFromQueueRows` 用源码字节切片提取，避免手抄失真（断言 `blockExtractedFromSource: true`）。
  可达性补充实验：qoder 同一 `message.id` 两轮同步，用量 1000 → 500。
- **关键输出**：
  - 同键双行（900 前 / 400 后）：`mainUsageHourTotalsFromQueueRows` = **900**（MAX）；`readQueueData` 等价 = **400**；`readQueueBatch` = **400**。`divergent: true`。
  - 可达性：qoder 队列实际写出 `1010` 后接 `510`（同键更小的行）；此时 MAX 版读到 1010，last-wins 版读到 510，`divergent: true`。
  - 调用点静态核对：`mainUsageHourTotalsFromQueueRows/FromQueuePath/FromState` 全部只在 codex rescan repair 的「重建 vs 现状」守门里使用（`sync.js:5848-5873`），用途是判断 `rebuilt < existing` 时放弃 repair；`readQueueBatch` / `readQueueData` 用于上传与展示。
- **对主文档的影响**：§12 A17 行由「两者是否给出不同结果，取决于队列是否可能出现同键数值回退」改为：**两者确实不同，且回退可达**。建议措辞：「`mainUsageHourTotalsFromQueueRows` 的 MAX 语义与本地展示读取器的 last-wins 在队列出现同键回退行时给出不同数字（实测 900 vs 400）。该函数仅服务于 codex rescan repair 的保守守门（`:5848-5873`），MAX 在此方向上偏保守（更易判定 rebuild 降低了已有桶从而放弃 repair），因此它是刻意的取舍而非缺陷；但需在文档中声明：**展示值与守门值在回退场景下会分叉**，且 qoder / trae-cn 的先减后加回撤语义会产生这类回退行」。

---

## 附录：复现清单

| 实验 | 脚本（`%TEMP%\tt-exp\`） | 命令 |
| --- | --- | --- |
| A1 | `a1.cjs` | `node %TEMP%\tt-exp\a1.cjs` |
| A4 | `a4.cjs` | `node %TEMP%\tt-exp\a4.cjs` |
| A5 | `a5.cjs` | `node %TEMP%\tt-exp\a5.cjs` |
| A8 | `a8.cjs` / `a8b.cjs` / `a8c.cjs` | 顺序执行 |
| A11 | `a11.cjs` | `node %TEMP%\tt-exp\a11.cjs` |
| A12 | `a12.cjs` | `node %TEMP%\tt-exp\a12.cjs` |
| A16 | `a16.cjs` | `node %TEMP%\tt-exp\a16.cjs` |
| A17 | `a17.cjs` / `a17b.cjs` | 顺序执行 |

脚本为一次性产物，只落在 `%TEMP%`，不进样本仓。A5 / A17 依赖 `require` `src/commands/sync.js`；脚本已在 `require` 前把 `USERPROFILE` / `HOME` / `APPDATA` / `LOCALAPPDATA` 重定向到临时 home（实测运行后该目录仍为空）。

## 遗留未覆盖项（本组未做，交由后续）

- A11 / A12 的「真实语料占比」：本组只用合成 fixture 证明机制可达，未统计真实日志中全零行的出现频率。
- A16 的真实触发频率：upstream 重置后短时间内回升到原量级这一组合在真实会话上的发生率未统计。
- A8 的跨机器副本场景：`\\wsl$` 真实桥接路径未使用（用 `\\localhost\C$` 等价验证了 `isUncPath` 分支与 `fileId` 层行为）。
