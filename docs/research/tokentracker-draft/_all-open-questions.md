# 各分片「存疑与未验证」节汇总（自动抽取，供主文档 §12 与复核使用）

## 来源：S0-mainline-anchors.md
（未找到存疑节）

## 来源：S1-pipeline-and-collection.md
## 8. 存疑与待复核点

按“影响结论可信度”排序。每条给出卡点位置与建议验证方式。

1. **`enqueueTouchedBuckets` 的 `legacyGroups` / `groupQueued` 分支语义未完全验证**。`src/lib/rollout.js:3181-3194,3430-3480` 存在一条“整小时聚合成 `model=DEFAULT_MODEL` 一行”的遗留路径，且 v2→v3 的注释说 `groupQueued` 的存在本身就是 legacy 标记（`src/commands/sync.js:216-220`）。我只确认了“有该键则走聚合分支”，**未验证**新装机会不会写入该键（如果不会，则该分支对现有用户是死代码；如果会，则存在与修复意图冲突的再膨胀路径）。建议：构造一个含 `groupQueued` 的 v1 游标 fixture，跑 `enqueueTouchedBuckets` 观察输出行数与模型名。
2. **崩溃窗口造成的重复行在上传侧的去重强度**。写入是 appendFile，游标提交在其后（§3.4），因此崩溃会留下重复的同键行。读侧与 `compactPendingQueue` 都按“同键最后一条胜出”处理，云端是整行 upsert；但 `readQueueBatch`（`src/commands/sync.js:4422-4512`）在同一批内也是 `bucketMap.set(...)` 覆盖，**未验证**同一批内出现同键但 token 更小的旧行时是否会以旧覆盖新（取决于文件顺序）。建议：构造同键双行（旧在前、新在后）的队列 fixture 直接跑 `readQueueBatch`。
3. **`compactPendingQueue` 与 `queue.state.json` offset 的交互**。代码只重写 `[offset, size)` 区间并原样复制 `[0, offset)`（`src/lib/upload-queue.js:97-110`），理论上 offset 仍有效；但**未验证**“pending 区出现跨行边界残缺（写者正在写）时 `iterateQueueLines` 的行为”以及压缩后 offset 恰好落在文件末尾的边界（`input.close()` 与 `fs.stat` 校验之间有窗口）。
4. **游标存储 `previous` / `rollback` / `current` 三代语义的完整使用矩阵**。我只读了 commit 与读取回退两处；哪些代码路径会真正读到 `previous`（而非只读 `rollback`）**未能确认**——`src/lib/cursor-store.js:974-986` 的注释指向“老版本 v2 读者”，属跨版本兼容，需要对照历史版本代码才能定性。
5. **`multiInstallParse` 异常分支的写回语义**（§6.5 已标）。卡在 `src/lib/multi-install-parser.js:49-54`。
6. **Claude `Stop` 事件的触发粒度**。仓库内只有事件字符串，无语义注释（`src/lib/claude-config.js:7-8`；`src/commands/init.js:724`）；共享情报称 Claude 侧是“每轮 + 会话结束”，但**本地源码无法证实**该断言。
7. **CodeBuddy / WorkBuddy 是否真的消费 `settings.json` 的 `SessionEnd`**。源码以“fork 同 schema”注释断言（`src/commands/init.js:623-625,1034-1036,1048-1049`），无 schema 校验、无第三方验证。
8. **`passive-mode.js` 的覆盖缺口**。它只覆盖 8 个 hook 驱动 provider（`src/lib/passive-mode.js:96-185`）；Grok / OpenCode / OpenClaw / omp 的“hook 缺失但日志在”状态由 `status.js` 的独立路径上报，**未逐一核对**这些 provider 的降级是否也被表达为“passive”（`status.js:1300-1336` 只是各自的 `passive reader (...)` 文案）。
9. **`workbuddy-ai` 的 hook 键名一致性**。`passive-mode.js` 用 `hookStatus?.["workbuddy-ai"]`（带连字符，`src/lib/passive-mode.js:180`），而 `status.js` 传的键确实是 `"workbuddy-ai"`（`src/commands/status.js:1034,1078`）——一致，但 `src/commands/sync.js` 的 source 白名单里同时存在 `workbuddy` 与 `workbuddy-ai`（`:362-363`），**未确认** `parseWorkbuddyIncremental` 是否用 `cursorKey` 或 `source` 区分二者（`sync.js:2639,2674` 传参差异未逐字段核对）。
10. **`usage_precision` 与 `prioritySubsetOf(totals)` 的语义**。它们出现在队列行里（`src/lib/rollout.js:3325-3343`），影响 `queuedKey` 的相等判定（`:3326`）。本文未追究其取值域与消费方，属范围外但会影响“同键覆盖”语义的字段，建议后续研究价格/展示链时补。
11. **`bin/tracker.js` 的代理自举对 hook 触发路径的实际影响**。`applyUndiciProxyIfNeeded` 返回 `unprotected` 时**整个进程 exit(1)**，包括 hook 触发的 sync；这意味着“配了代理但代理挂了”会静默让采集停摆（只有 stderr）。**未验证** status 是否会把这种情况上报为可见故障。
12. **WSL 探测缓存的失效 ——【R1 复核后已结案】结论：生产路径无调用，风险不成立**。`resetWslProbeCache`（`src/lib/wsl-probe.js:11`）在 `src/` 内除定义与导出（`:268`）外**零调用点**，唯一调用者是 `test/*.test.js`。长驻进程内新增发行版不会被发现这一现象依然存在，但既然没有任何生产路径刷新过缓存，也就没有“调用时机不对”的问题；若后续要做缓存刷新，属于新功能而非缺陷修复。
13. **`auto.retry.json` 的清理时机**。`clearAutoRetry` 在“无 pending 或不允许上传”时调用（`src/commands/sync.js:580-582,3661-3662`），但 `publishAccount` 分支与 native 壳自管路径的交互**未逐条验证**。
14. **`init` 的 `--link-code` 实际消费者 ——【R1 复核后已结案】结论：遗留参数**。init.js 内**只**用它关闭浏览器认证（`src/commands/init.js:1423,339`），`link_code_state.json` 在 init.js 内**无任何读写**（`:176,234,359` 仅为声明与传参）；全仓在 `src/`、`dashboard/` 内除该三处与 `src/lib/runtime-config.js:149`（清理清单）外**零引用**。可判定为纯遗留参数——即帮助文本（`src/cli.js:78,92`）对该开关的暗示存在误导。
15. **`deferred-codex-audit.json` 与 `codexColdScanAudit` 的双写一致性**。前者在“跳过游标提交”时累加（`src/commands/sync.js:3456-3460`），后者在审计记录时写入（`:3785-3819`），两处计数何时合并读回只看到 `readDeferredCodexAuditSyncs` 一处调用（`:965-969` 附近），**未完整追踪**合并后的清除路径。
16. **共享情报的行数/规模偏差**（§0.1 已列表纠偏）。若后续分析继续引用旧情报中的行号，会全部错位；建议统一以本文件实测行号为准。
17. **`status.js` 的 hook 状态传参完整性**。`detectPassiveProviders` 收到的 `hookStatus` 含 11 个键（`src/commands/status.js:1024-1036`），其中 `opencode_plugin`/`openclaw_session_plugin` 并不在 passive-mode 的 provider 列表内（`src/lib/passive-mode.js:96-185`），属**冗余传参**；反之 `passive-mode.js` 需要 `workbuddy-ai` 而它的键名风格与其它下划线键不一致，属潜在命名陷阱（当前一致，见第 9 条）。
18. **未覆盖范围声明**：本文不含逐 provider 解析算法、定价、Dashboard 渲染、云端 schema、macOS/Windows/Linux 壳的调度细节（`TokenTrackerBar/`、`TokenTrackerWin/`、`TokenTrackerLinux/` 三目录未读）。其中 Windows 壳的“5 分钟后台 sync 计时器”（`src/commands/serve.js:305-309` 注释提及）**未在壳代码中核实**，属明显缺口。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-A-pipeline.md`（样本 HEAD `b19d27b0` 未变）。本节只记录**改动**，未改动处即视为复核通过。修正 agent：FIX-A，所有改动均回读源码逐条确认后落笔。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| E1（§0.1） | 实测 `src/` 下共 **128 个文件** | **112 个文件**，并注明口径为「仓库跟踪文件数」 | `git ls-files src` = 112；`Get-ChildItem -Recurse -File -Force src` = 112 |
| E2（§2.3） | notify.cjs 模板 `init.js:1445-1574` | 模板区间 **`:1453-2231`**，宿主 `buildNotifyHandler` **`:1445-2232`**；补模板内辅助函数行号 | `init.js:1445,1453,1575,1583,1597,1609,1748,1811,2231,2232` |
| E3（§2.8） | 「哨兵范式（omp / **OpenCode**）」；引 `opencode-config.js:8,107-109` | 哨兵范式**只有 omp**（`omp-hook.js:17,388-395`，字符串全仓唯一产生点，消费方 `init.js:813`）；OpenCode 独立成「**内容等值短路 + 无条件备份覆盖**」条目（`:66-87`、`:73-75`、`:80-85`、`:94`） | `omp-hook.js:17,388-395`；`opencode-config.js:66-96`；`init.js:813` |
| E4（§3.4） | 写队列**只有一处** `fs.appendFile`（`rollout.js:3483`） | 主队列 append 写点**至少 6 处**（`rollout.js:3483`/`:20031`/`:22594`，`sync.js:5126`/`:5207`/`:5281`），另 `sync.js:4114` 写 fallback、`rollout.js:3536` 写 project 队列；**保留**“无独立写侧互斥、全部假定调用方持有 `sync.lock`”结论 | 全仓 `appendFile` 命中 9 处，逐一核对 |
| E5（§4.4） | 「Gemini / Kilo 等」记 `mtimeMs + lastIndex + lastTotals`（`:1289-1307`、`:1462-1516`） | 拆成三套：**KiroCli** `mtimeMs+lastIndex+lastTotals`（`:9371-9375`、`:9451-9471`）、Gemini（`:1289-1307`）、**opencode** 游标为 `inode/size/mtimeMs/lastTotals/messageKey` 且**无 `lastIndex`**（`:1462-1516`） | `rollout.js:1498,1513-1520,9371-9375` |
| E6（§7.1） | 「签名相同即返回缓存」引 `:99-104` | 命中在 **`:35`**，并发目的注释在 **`:12-15`**；`:99-104` 明确标注为 ENOENT 清理与失败回退 | `async-queue-reader.js:12-15,35,99-104` |
| E7（§6.3） | 「非 win32 平台一律只返回 native（`:149`）」 | `wsl-probe.js:149` 返回 `{native:null, wsl:null}`（发现层不做 WSL 探测）；「只返回 native」的落点是 **`install-resolver.js:21-23`** | `wsl-probe.js:149`；`install-resolver.js:21-23` |
| E9（§3.5） | `skipNoOpCursorCommit` 判据列举为 9 项 | 条件项 **13 个**，逐项列出（含 `!codexColdAuditDue`、`!codexFallbackRetryRan`、`opts.auto`、`totalParsed===…` 等） | `sync.js:3424-3437` |
| I1（§3.1） | 拿不到锁「写 `sync.skip.json` 并**静默 return**」 | 改为「不抛错：写 marker **并向 stderr 输出 `Sync skipped: …`**」（表内第 3 步与锁策略小节同步改） | `sync.js:555-558` |
| I2（§2.3） | 「双路径」判据叙述 | 指明模板内 `:1483`/`:1484` 路径**字面相同**，判据退化为一次 `existsSync`；同 quirk 见 `openclaw-session-plugin.js:358,361` | `init.js:1483-1484`；`openclaw-session-plugin.js:358,361` |
| I3（§4.7） | `codexColdScanAudit` 列 5 个字段 | **6 个**：补 `lastFullScanAt`（ISO 文本副本） | `sync.js:3810,3813-3815` |
| I4（§3.4） | 原子替换「**只在两处出现**」 | 改为「append 与原子重写**两条写路径，各自多个实现点**」 | 同 E4；`upload-queue.js:64-132` |
| I6（§7.2） | CRLF「可选 `stripCr`（`:22-27`）」 | `stripCr` **对调用方不可选**：对外 opts 只有 `invalidUtf8/maxPhysicalBytes/maxRecordBytes`，调用点硬编码 `{stripCr:true}` | `jsonl-lines.js:29-33,85` |
| I8（§8 第 12/14 条） | 两条「未确认」 | 均**结案**：`resetWslProbeCache` 生产调用为 0（仅定义 `wsl-probe.js:11` 与导出 `:268`）；`link_code_state.json` 除 `init.js:176,234,359` 与 `runtime-config.js:149` 外零引用 → 判为**遗留参数** | `wsl-probe.js:11,268`；`init.js:176`；`runtime-config.js:149` |
| I9（§4.8） | 守卫列表末尾述及 `:6745` | 补【存疑】一句：**其它 repair 的“保留不可解析行”语义未逐条核对**，`sync.js:6844,7147` 亦未逐行打开 | `sync.js:6745`（属 Droid 修复） |

未在本文件落实、留给 S1b 或不适用本文件的项：E8（`cloneOrCopy` 非硬链接）、I5（`needsHistoricalRolloutDedup` 兜底两条）、I7（deferred 提示受 `!opts.auto` 约束）——三条均只影响 S1b 对应段落，已在 [S1b-worth-copying.md](./S1b-worth-copying.md) 修正；U 组 10 条按“无本仓库证据”统一处理，见 S1b 末尾说明。

## 来源：S1b-worth-copying.md
（未找到存疑节）

## 来源：S2-framework-claude-codex.md
## 存疑与待复核点

按"影响可控性"从高到低排列，每条给出可执行的复核动作。

1. **Claude 增量游标无行边界提交**（`src/lib/rollout.js:2793`、`:2889`）。`const endOffset = st.size;` 共 3 处（`:2291`、`:2682`、`:2793`），**只有 `:2793` 落到 token 游标**（`:1194` 的 `cursors.files[key].offset = result.endOffset`）；`:2291` 是"无新字节"早退，`:2682` 是项目上下文扫描且返回 `committedEndOffset`（`:2765`）。Claude 侧若同步瞬间读到半行，`readline` 会 yield 未闭合尾行、`JSON.parse` 失败被吞（`:2843-2847`），游标仍推进到文件尾，该行字节被跳过。**非对称性**：Codex 的 token 路径用 `committedEndOffset`（`:2652`），**连项目上下文扫描位也做了同样的提交保护**（`:2765`），OpenClaw 另有 `openclawOffsetEndsAtLineBoundary`（`:1949`）——只有 Claude 一条路径完全没有。
   补充（收窄"永久"）：该跳过**在该文件下一次 inode 变化或截断时会被全量重扫抵消**（`:488-504` 的三态判定，重置 `startOffset=0`），因此准确说法是"在本代文件内不可恢复"，而非严格永久。
   复核动作：构造"写入半行 JSONL → 跑 `parseClaudeIncremental` → 补全该行 → 再跑"的定向测试，看补全后的那行是否被计入。若未计入，即为确定性数据丢失 bug（不是理论风险）。

2. **Claude 真值修复与增量解析的口径不一致**（B.7.2 表格）。差异至少三处：usage 取值路径（`obj.message.usage || obj.usage` vs 仅 `obj.message.usage`）、reasoning 字段名（`output_tokens_details.thinking_tokens` vs `usage.reasoning_output_tokens`）、output 是否拆分。修复是**一次性且不可逆**（重写 `queue.jsonl` + `cursors.claudeHashes` 全替换，`src/commands/sync.js:6871-6921`）。
   复核动作：拿同一份合成 fixture 跑两条路径，逐列比对（不只是 total）。预期 total 相同、`output_tokens` 与 `reasoning_output_tokens` 不同。

3. **Claude 缓存字段命名未兼容 `cache_write_input_tokens`**（`src/lib/rollout.js:4927-4928` vs `src/lib/codex-token-usage.js:25-27`）。anthropic 兼容端点（Kimi/DeepSeek/Mimo/MiniMax）若用别名，缓存写会被静默丢为 0。
   复核动作：查这些 provider 的合成 fixture 或真实样本中 `cache_creation_input_tokens` 的出现率；若为 0 而 `cache_write_input_tokens` 非 0，即为确定性丢量。

4. **`claudeHashes` 上限 100k 的淘汰窗口**（`src/lib/rollout.js:1236-1237`）。超限时保留**最后** 100k；注释（`:1021-1025`）自己承认"message-hash 层有上限，淘汰会让批量历史双计"，因此加了文件级 `fileId` 去重作为第二层。但 `fileId` 只在"同一次运行内看到 `\\wsl$` 与本地路径混杂"时启用（`sawUncFile && sawLocalFile`，`:1035`）。
   复核动作：构造 >100k 哈希 + 同内容双路径的用例，验证是否双计。

5. **Codex 主路径无单行字节上限**（A.2.2）。`physicalJsonlRecords` 支持 `maxRecordBytes`，但 `parseRolloutFile` 只传 `invalidUtf8`（`src/lib/rollout.js:2397`）；有此保护的只有 OMP 族（`:15717`）。
   复核动作：确认这是"有意不设限"还是遗漏；若某 rollout 含超大 `response_item`，会整行读进内存。

6. **`<synthetic>` 无显式处理**（B.4.2）。真实 Claude 日志是否会给该模型名配非零 usage，本次未验证；若会，则面板会出现一个名为 `<synthetic>` 的模型行。
   复核动作：在本机 `~/.claude/projects/**` 里 `grep '"model":"<synthetic>"'` 并看同行 usage 是否非零。

7. **`parseClaudeScienceIncremental` 的字段语义未与主线对齐**。`reasoning_output_tokens` 恒为 0（`src/lib/rollout.js:7023`），`input` 的"含缓存"前提（`:6995-7001` 注释）来自该产品的私有约定，本次无真实 DB 样本可验证。
   复核动作：若本机有 `~/.claude-science/operon-cli.db`，抽样比对 `frames` 表列值与 UI 显示。

8. **`usage_precision` 语义未展开**。它作为可选列进入队列行（`src/lib/rollout.js:3325-3326`、`:3341`），并在 `queuedKey` 中参与比对；本次未追踪其来源与消费方（推测与某些 provider 的"估算值"标记有关）。
   复核动作：`grep -rn "usage_precision" src/ test/` 追全部写入与读取点。

9. **`mainUsageHourTotalsFromQueueRows` 用 `setMaxTotal` 而非"最后一条"**（`src/commands/sync.js:6540`）。与 `readQueueData`（最后一条胜出，`src/lib/local-api.js:207`）语义不同，两者是否会给出不同结果，取决于队列是否可能出现"同键数值回退"（例如真值修复删行追加后的旧值仍在文件中——但修复是原子重写，理论上不会）。
   复核动作：确认该函数的所有调用点是否都作用于"已被重写过的干净队列"。

10. **Codex 模型串未归一化，跨渠道同模型合并**（C.5.2）。若用户经多 provider 路由同一模型，用量会合并进同一个 `codex|<model>|<hour>` 桶，且**没有 provider 维度可供拆分**。
    复核动作：确认产品是否需要该维度；若需要，需在事件层新增字段（当前 schema 无位可放）。

11. **`model/rerouted` 归属路径从未在真实语料验证**（`src/lib/codex-model-attribution.js:47-51` 自述 `0` 命中）。该分支一旦被真实触发，其信封形状与字段名（`fromModel/toModel/from_model/to_model`）都属推断。
    复核动作：检索本机 rollout 中是否出现 `model/rerouted`/`model_rerouted`。

12. **半小时桶算法有两份实现且无等价性锁定**（A.5.2）。`src/lib/rollout.js:4173` 与 `src/lib/claude-categorizer.js:1175` **语义等价、文本不同**（前者分步 `const bucketStart = …` + `return bucketStart.toISOString()`，后者单行链式 `return new Date(…)…).toISOString()`），改动任一处不会让另一处失败；`test/` 下引用 `toUtcHalfHourStart` 的两个测试都指向 `rollout.js` 那份，`claude-categorizer.js` 的副本**无人测**。真值修复与增量解析一旦分叉，队列会出现两套时间分桶。
    复核动作：加一条"两份输出对同一批时间戳完全相等"的回归测试（从两个模块各取一份实现对比，而不是只测其中一份）。

13. **未复核区**：`src/lib/cursor-store.js`（1918 行）的分片完整性校验、代际（generations）轮换与 `codexEventStore` 的失败注入路径，本次只读了常量与函数清单；若 S2 后续需要"游标损坏后的恢复语义"，须单独一轮。

14. **Codex 侧的"reasoning 不双计"没有对等回归测试**。`test/claude-reasoning-usage.test.js:45-47` 只用 `< 1e-12` 锁定了 Claude 的"列分布可换、费用不变"；Codex 路径依赖 `src/lib/pricing/index.js:248-256` 的 `source` 白名单硬编码（`codex`/`acode`/`every-code`/`omo`/`cline`），一旦上游新增一个 `*-code` 变体而白名单漏加，就会**静默双计**每一条 reasoning token，且现有测试全绿。
    复核动作：补一条对等断言——`computeRowCost({ source: "codex", output_tokens: 518, reasoning_output_tokens: 163 })` 与同参数但 `reasoning_output_tokens: 0` 的结果差 `< 1e-12`；并考虑把白名单改为按 `source.endsWith("-code") || source === "codex"` 之类的结构性判定。

---

## 附：本次研究的证据索引（可直接定位）

| 主题 | 入口 |
|---|---|
| 45 个 parser 清单 | `src/lib/rollout.js:251`…`:24458`（表见 A.1.2） |
| 导出表 | `src/lib/rollout.js:24675` |
| 半小时桶 | `src/lib/rollout.js:4173`；副本 `src/lib/claude-categorizer.js:1175` |
| 桶与队列写入 | `src/lib/rollout.js:3169`、`:3489` |
| 桶结构 | `src/lib/rollout.js:4053`（`initTotals`）、`:3996`（`getHourlyBucket`） |
| 队列读取口径 | `src/lib/local-api.js:202`、`:207`、`:213`；`src/commands/sync.js:4510`、`:6540` |
| JSONL 行读取 | `src/lib/jsonl-lines.js:29` |
| Codex 主解析 | `src/lib/rollout.js:251`（入口）、`:2264`（单文件）、`:4192`（文件名日期） |
| Codex 差值 | `src/lib/codex-token-usage.js:98` |
| Codex compaction | `src/lib/codex-usage-record.js:1-35` |
| Codex 模型归属 | `src/lib/codex-model-attribution.js:3-23` |
| Codex 档位 | `src/lib/codex-service-tier.js:1-42` |
| Claude 主解析 | `src/lib/rollout.js:954`（入口）、`:2776`（单文件）、`:4921`（usage 归一） |
| Claude 去重与身份 | `src/lib/rollout.js:4914`；`src/lib/claude-user-identity.js:13` |
| Claude 真值修复 | `src/lib/claude-categorizer.js:1193`；`src/commands/sync.js:6844` |
| 测试 | `test/rollout-parser.test.js:104`、`test/codex-token-usage.test.js:24`、`test/codex-union-cursor-divergence.test.js:118`、`test/claude-fork-conversations.test.js:35`、`test/claude-reasoning-usage.test.js:11` |

---

## 修正记录（R1 复核后）

依据 = `REVIEW-R1-B-claude-codex.md` 的 E/I/U 三组 ＋ 本次对样本仓库重跑的实测（45 个 `parse*Incremental` 的顶层 return 键名全量提取、`endOffset = st.size` 全量定位、计价门控逐行核对）。样本仓库**未做任何修改**。行号口径与正文一致。

| 编号 | 原表述 | 修正后 | 依据 |
|---|---|---|---|
| E1 | 「45 个 parser 存在**三族**签名」 | 「**无统一契约**：45 个声明中 5 个转发薄壳、40 个自持 return、去重后 **11 种键集合**」，并给出完整清单与逐字证据 | `rollout.js:1239`、`:6027`、`:7137`、`:16084`、`:18111`、`:19003-19013`、`:19538`、`:22175`、`:22791`、`:24029`、`:24664`；本次重跑：45 声明 / 40 顶层字面量 / 11 键集合 |
| E1-a | 复核清单中的 `parseCursorApiIncremental`(+`total`)、`parseTraeCnApiIncremental`(+`total`)、`parseGrokBuildIncremental`(「键重复」) | 三处均为**值标识符被当成键名**：实际写法是 `recordsProcessed: total`（`:7270`、`:22791`）与 `recordsProcessed: eventsAggregated`（`:20551`，疑似上游把记录数写成事件数）。它们的键集合分别是骨架 C、骨架 C+`skippedRows`、骨架 D，**没有多出的键、也没有重复键** | `rollout.js:7270`、`:22791`、`:20551` 逐字 |
| E1-b | 复核「**12 种**返回字面量」 | **11 种**：复核漏掉了 `parseCopilotSessionStoreIncremental` 的 9 键集合，同时把 3 个值标识符计成了键 | `rollout.js:19003-19013` 逐字 |
| E1-c | §0 第 1 条「`rollout.js` 共 **23260 行**」 | 实测 **24932 行**（`ReadAllLines(UTF8).Count`）；字节数 `942266` 无误。复核报告自身也测到 24932，却判"分片 §0 数据一致"，属漏检 | `rollout.js` 全文行数实测；对照 `REVIEW-R1-B-claude-codex.md:151` |
| I1 | §0 第 6 条「**reasoning 不被并入 output**，而是独立成列」 | 改为两侧各自的准确表述：Codex 的 reasoning 是 `output_tokens` 的**子集标记**（解析不拆列 `:4851-4879`、计价**置 0** `pricing/index.js:248-256`）；Claude 的 reasoning 是**独立加成项**（解析**显式拆出** `rollout.js:4930-4939`、计价**照 output 价**）。两者在 `computeRowCost` 里都以**五列之和**为输入，**`total_tokens` 永不参与计费** | `rollout.js:4851-4879`、`:4930-4939`；`pricing/index.js:248-256`、`:297-299`、`:325-327`；`CLAUDE.md:60-64` |
| I2 | §0 第 8 条「Claude 的 `endOffset` 直接取 `st.size`」（暗示仅 1 处） | 写清 **3 处分工**：`:2291`（`parseRolloutFile` 的"无新字节"早退，返回值不可能大于入参 `startOffset`，不计 token）、`:2682`（`scanRolloutProjectFileContexts`，返回 `{ endOffset: committedEndOffset, … }`，`:2765`）、`:2793`（**唯一**落到 token 游标的站点，`:1194`）；并写明非对称性——**Codex 连项目扫描都做了提交位保护** | `rollout.js:2291`、`:2294-2302`、`:2652`、`:2666-2682`、`:2764-2765`、`:2793`、`:2889`、`:1194` |
| I2-附 | 复核称「§C 第 10 条误引 `:2682`」 | 全文检索 `2682` 在本分片**零命中**，该误引不存在；§A.3.3 引的 Codex 提交位 `:2652` 经实测**正确**，故未删任何引用，只补了 3 处分工表 | 分片全文 `grep 2682` 零命中；`rollout.js:2652` 逐字 |
| I3 | §A.3.3 / 存疑 1「该行字节**永久跳过**」 | 收窄为"**本代文件内不可恢复**"：该文件下次 inode 变化或截断会走重置路径全量重扫（`startOffset=0`） | `rollout.js:488-504`、`:2843-2847` |
| I4 | （缺项） | 存疑新增第 **14** 条：Codex 侧"reasoning 不双计"无对等回归测试，`source` 白名单漏加会**静默双计** | `pricing/index.js:248-256`；对照 `test/claude-reasoning-usage.test.js:45-47` |
| I5 | §A.2.3「【未能确认】队列层坏行计数是否影响用量」 | **闭合**：`countMalformedCodexProjectQueueRows` 全库只有一个调用点，只决定"是否执行修复"，不产生用量 | `sync.js:6436`（定义）、`:5824-5830`（唯一调用点） |
| I6 | §A.3.2 游标字段表把项目字段写成无条件 | 加前提：Claude 的 `claudeCwd` / `projectFileContext` / `projectRef` / `projectKey` 只在 `projectEnabled` 分支写入；Codex 的 `projectOffset` / `projectFileContext` 受 `codexProjectFastPath` 控制 | `rollout.js:1198-1205`、`:656-665` |
| I7 | §A.5.2「两份实现的**源码逐字一致**」 | 改为「**语义等价、文本不同**」（单行链式 `return new Date(…).toISOString()` vs 分步 `const bucketStart` + `return`）；保留成立的那半句「副本无测试锁定」（两个测试都 `require("../src/lib/rollout")`） | `rollout.js:4173-4190` vs `claude-categorizer.js:1175-1191`；`test/sync-droid-dup-session-repair.test.js:29`、`test/trae-entitlement.test.js:29`、`:130-137` |
| I8 | §A.3.3 注释引用 `:1114-1120` | 改为 `:1115-1120`（`:1114` 是空注释分隔行 `//`） | `rollout.js:1114-1120` 逐字 |
| U1 | §B.2.3「跨 provider … 会静默丢缓存写」 | 保留为**条件性风险**并写明判定缺口：需要 Claude 会话行里 `cache_write_input_tokens` 的出现率统计（出现且 `cache_creation_input_tokens` 缺失才成立） | `rollout.js:4927-4928`、`codex-token-usage.js:25-27` |
| U2 | §B.4.2「全库**唯一**出现 `<synthetic>` 的地方是 `:101`」 | 补齐为 **3 处**（`:101` 集合、`:621-624` 注释、`:1968-1971` 注释），并写明判定所需样本（本机 `~/.claude/projects/**` 里 `"model":"<synthetic>"` 行是否带非零 usage） | `session-analytics.js:101`、`:621-626`、`:1968-1971` |
| U3 | §C.5.3 语料统计（1213 文件 / 167 命中 / 25%；814 default / 58 priority） | 标注为"**上游源码注释里的本机语料观测**，非本次实测"，并写明重跑口径（分母=含 `token_count` 的文件数）；代码侧规则与结构仍判为已核对 | `codex-service-tier.js:8-15` |
| U4 | §C.6.1「不影响 token 统计」 | 拆成两段：`不写队列` **已闭合**（检索零命中）；`不影响统计` 依赖"`sync.js:198` 借用属一次性迁移"这一前提，真正缺的是"该修复是否在某些安装上**永不触发**"（运行时游标状态） | `local-api.js:71`、`session-analytics.js:32`、`sync.js:198`、`:6844-6854` |

**未采纳**：0 条（E/I/U 全部按复核结论或其纠正后的版本落实）。
**对复核报告的 4 处纠正**：E1-a（值标识符误读）、E1-b（11 而非 12）、**E1-c（`rollout.js` 行数 24932，非 23260；复核漏检）**、I2-附（`:2682` 误引在分片中不存在；另复核把 `:2291` 归为"项目上下文扫描"，实测它属于 `parseRolloutFile`）。

## 来源：S3a-providers-jsonl.md
## 7. 存疑与待复核点

### 7.1 未确认项（R1 复核后重写：7 条结案、2 条转 U 组，第 10 条自查清单整体重写）

**已结案（本轮按权威口径 `read` / `[System.IO.File]::ReadAllLines(...,UTF8)` 打开过对应行）**：

1. **（结案）Codebuddy 的 JSONL 支路闸门**：注释 `src/lib/rollout.js:9940`～`9943` 说明现代 CodeBuddy 把用量写进 `providerData.rawUsage`；该支路零值判定为含 `reasoningTokens` 的五桶闸门（`src/lib/rollout.js:10476`～`10485`）→ Codebuddy 归 §5 D 类，F 类清空。（原引 `9938` 实为 GBK 口径——该行是 `}`；原「read 确认」字样与事实不符，已删。）
2. **（结案）Opencode 的 storage 下层结构**：`walkOpencodeMessages`（`src/lib/rollout.js:5025`）递归遍历，只收 `msg_*.json`（`src/lib/rollout.js:5033`）；「消息文件在 `<storage>/message/**`」成立，`listOpencodeMessageFiles`（`src/lib/rollout.js:243`）只负责拼 `<storage>/message` 一层。
3. **（结案）Cline 的根目录实现**：`resolveClineSessionsDirs`（`src/lib/rollout.js:12054`，Windows 双安装 + WSL 探测在 `12069`～`12073`）与 `scanClineSessionFiles`（`src/lib/rollout.js:12086`）都在 `src/lib/rollout.js`（**不在** `sync.js`）；`~/.cline/data/sessions` 与 `src/commands/sync.js:2110` 注释一致（原引 2109 为 −1）。
4. **（结案，推翻原描述）KiroCli 会话文件侧字段名**：**不是**模块注释 `8109` 的 `user_prompt_length`/`response_size`，而是 `turn.input_token_count` / `turn.output_token_count`（`src/lib/rollout.js:8371`～`8372`，由 `readKiroCliSessionTurns`（`8316`）读取）。`readKiroCliV2SessionTurns`（`8482`）另读 `messages.jsonl`，其注释（`8475`～`8481`）明说该文件不含 token 字段、沿用 4 字符/token 近似。`writeKiroCliCreditsSidecar`（`8701`）只写 credits 侧车，不进 token 桶。
5. **（结案）Kimi / KimiCode 的 home 优先级**：`resolveKimiHome`（`src/lib/rollout.js:9493`；`KIMI_HOME` 在 `9495`，win32 WSL 分支 `9497`～`9502`）与 `resolveKimiCodeHome`（`src/lib/rollout.js:9669`；`KIMI_CODE_HOME` 在 `9671`）。
6. **（部分结案）Workbuddy 的 traces 支路字段名**：「JSONL 优先、traces 兜底、`detailedSessions` 压制」已确认（`src/lib/rollout.js:11013`）；traces 分支的完整字段映射段未读 → **转 U1**。
7. **（部分结案）Droid 的 `resolveDroidModel` 全链**：调用点与注释已核（`src/lib/rollout.js:15333`、`15338`），`applyDroidTotalFallback`（`14972`～`14979`）逐字一致，`normalizeDroidModelName` 在 `14858`；三级链实现未展开 → **转 U2**。
8. **（结案）Anythingllm 的 SELECT 语句**：`src/lib/rollout.js:13874`～`13887`，`FROM workspace_chats`，列经 `json_extract(response,'$.metrics.*')`，SQL 层只过滤 `whereClause`（游标 / `include`）、**不做零值过滤** → 与 §3.16「纯 total 行在 JS 侧 `14016` 被丢弃」自洽。
9. **（结案）`src/commands/init.js` 的 OpenClaw 插件安装行号**：`installOpenclawSessionPlugin` 调用点确在 `src/commands/init.js:1078`（另在 `init.js:54` 有 import）。

10. **行号自查清单（R1 复核后重写：原版把「仅由 PowerShell 定位」的 40 条整体标为“未复核”，该自查本身不准确）**——R1 复核用权威口径（`read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`）逐条重验这 40 条：**35 条正确、5 条错误**（另在清单外又查出 2 条错误 + 1 处归属错误，合计 8 处已在上文改正）。
    - **重验为正确、可直接引用（35 条）**：Gemini 的 `1265`/`1275`/`1283`/`1361`/`1403`；Opencode 的 `1438`/`1448`/`1513`；Kiro 的 `7282`/`7305`（指向空行，`readKiroDbTokens` 函数头实为 `7306`，在 −1 容差内）/`7334`/`7338`；KiroCli 的 `8132`/`8138`/`8140`/`8165`；Codebuddy 的 `9979`/`10022`/`10141`/`10320`/`10322`/`10525`；Roocode 的 `11749`/`11762`/`11790`/`11805`/`11814`；Cline 的 `12157`；Zed 的 `12722`/`12726`/`12732`（行号本身正确，但正文把 `isReset` 挂在此处 → 已改为 `12742`）/`12816`；Lmstudio 的 `12897`/`12917`。
    - **已改正的 8 处**：`1307`→`1284`（inode 身份；`1307` 实为 `let lastModel = …`）、`7294`→`7298`（`resolveKiroDbPath` 函数头；`7294` 是 `}`）、`10331`→`10337`（非缓存输入修正块头；`10331` 是 `let inputTokens = toNonNegativeInt(inputRaw);`）、`10339`→`10345`（扩展日志零值闸门；`10339` 是 `usage.cacheMissTokens === undefined &&`）、`9938`→`9940`（`providerData.rawUsage` 注释，实为 `9940`～`9943`；`9938` 是 `}`）、`12190`→`12178`（`normalizeClineModel` 函数头，四级兜底链 `12179`～`12189`；`12190` 是闭合 `}`）、`13681`→`13691`（`parseUnslothIncremental` 函数头；`13681` 是 `normalizeUnslothUsageRow` 内的 `if (!id || !totals || !bucketStart) return null;`）、`12732`→`12742`（`isReset` 实际位置；`12732` 是差分基线 `const prev = threadTotals[row.id] || {…}`）。
    - **归属错误（E11）**：`12845` / `12853` 曾被列在本条「Zed 的 `12722`/`12726`/`12732`/`12816`/`12845`/`12853`」名下，但两者都是 **LM Studio 模块**（`12843` 的 `// LM Studio` 段头；`12845` 为模块注释首行 `pretty-printed OpenAI-compatible final responses`、`12853` 为 `const LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024;`），§3.14 正文本来就按 Lmstudio 引用——同一文档两处归属互相打架，现已从 Zed 清单删除，只保留 Lmstudio 条目（`12845`/`12853`/`12897`/`12917`）。
    - **口径教训**：错误全部落在 ≥7000 行，且都等于「UTF-8 行号 −(4~10)」= GBK 行号，即 **PowerShell 口径混入正文**；`12190`、`1307` 两种口径都对不上，属无来源数字。**「自称已 read 确认」不能替代逐条重验**——原第 1 条的 `9938` 就自称 read 确认，实为 GBK 口径附近位（UTF-8 `9940` 的 GBK 位是 `9934`，而 `9938` 落在 `function codebuddyJsonlHasUsage` 头 `9944` 附近，两种口径都不精确对应）。自查清单本身也会骗人：它把 35 条正确的行号标成「未复核」，却漏掉真正的错项。
    - 参考：`src/commands/init.js:568`～`589` 的 hook 安装点**已复核**（与本清单口径一致）。

**无法验证（U 组，保留；判定条件已写明）**：

- **U1 — Workbuddy traces 支路的字段名是否与 JSONL 支路同形**：需读 `src/lib/rollout.js:11100`～`11260` 段（`total_tokens:` 出现在 `11112`、`11246` 两处，疑为 traces/兜底路径）才能结案；读到该段之前，§3.10 的字段表**只对 JSONL 支路负责**。
- **U2 — Droid `resolveDroidModel` 的三级链是否与注释一致（`*-unknown` 兜底）**：需读 `src/lib/rollout.js:15300`～`15345` 的函数体；目前只核到调用点（`15338`）、注释（`15333`）与 `normalizeDroidModelName`（`14858`），故 §3.18 的模型链描述属未验证。

### 7.2 下游必须注意的三点

1. **“文本/JSONL 型”只对了一半**：7 家（Kiro、Hermes、KiroCli、Zed、Unsloth、Anythingllm、Goose）是 SQLite 读取；若 S3b 也按文件类型分工，会出现职责重叠或空白。
2. **`source` 不是 provider 名**：`kimi` 聚合两套解析器、`kiro` 聚合两套、`kilo-code` 带连字符、Workbuddy 用 variant。任何“按 source 反查解析器/ hook 状态”的逻辑都必须先过 §2 的对照。
3. **`conversation_count` 语义五家五样**：产生差分的消息数（Gemini/Opencode）、每条事件 +1（Kimi 一族等）、仅首次（Cline）、仅首次会话或重置（Droid）、恒 1（Goose/Lmstudio/Unsloth）、DB `message_count` 差分（Hermes）、线程更新次数（Zed）。dashboard 若直接当“会话数”展示，会同时出现高估与低估。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-C-providers-jsonl.md`（E=12 / I=7 / U=2）。**采纳 21 条（E1–E12、I1–I7 及第 9/5 条结案），保留 U1/U2（写明判定条件），反驳 0 条**。行号口径统一为 `read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；每条均在本轮开过对应行才落笔。

| 编号 | 原表述 | 修正后 | 依据行号（`read`/UTF-8） |
|---|---|---|---|
| E1 / B5 | §3.6·§0 速览·§6 速查表：「本家没有真实 token 字段，全部按字符数 ÷ 4 估算」 | 推翻：legacy 会话 `.json` 优先读**真实** `turn.input_token_count` / `turn.output_token_count`，仅两者皆 0 才回退字符估算；纯字符估算只剩 SQLite `data.sqlite3` 与 v2 `messages.jsonl` 两支；常量使用点补漏列的第 4 处 `8428` | `rollout.js:8371`、`8372`、`8374`、`8386`、`8387`、`8428`、`8475`–`8481`、`9152`–`9159` |
| E2 | §5 把 KiroCli 归入 E 类「结构上不可能」 | 移入 **B 类「弱化版」**：会话支路零值闸门只检 4 列、不含 reasoning（`9414`），delta 含 `reasoning`（`9446`）→ 可产出 `input=0/output=0/reasoning>0` 行；因 `kiro` 不在推理并入集合内，**不是 $0** | `rollout.js:9414`、`9446`、`9209`；`pricing/index.js:248`–`256` |
| E3 | Kiro 路径引用 `7294` | `7298`（`resolveKiroDbPath` 函数头，函数体 `7298`–`7304`；`7294` 是 `}`） | `rollout.js:7298` |
| E4 | Codebuddy「非缓存输入修正」块头 `10331` | `10337`（块 `10337`–`10343`） | `rollout.js:10337` |
| E5 | §3.9 + §5 F 的扩展日志闸门 `10339`（标注未复核） | `10345`，并去掉「未复核」标注 | `rollout.js:10345` |
| E6 | §7.1 第 1 条 `9938`（自称 read 确认） | `9940`（注释 `9940`–`9943`）；「read 确认」字样删除并说明其为 GBK 口径附近位 | `rollout.js:9940` |
| E7 | Cline `normalizeClineModel` 引用 `12190` | `12178`（兜底链 `12179`–`12189`；`12190` 是闭合 `}`） | `rollout.js:12178` |
| E8 | §3.13 把 `isReset` 挂在 `12732` | 改为 `12742`（`isReset = currSum > 0 && currSum < prevSum`）；注明 `12732` 是差分基线 | `rollout.js:12742` |
| E9 | §3.15 标题 `parseUnslothIncremental` 引 `13681` | `13691`（`13681` 是 `normalizeUnslothUsageRow` 内的空值守卫） | `rollout.js:13691` |
| E10 | §3.1「文件身份含 inode（`1307`）」 | `1284`（`const inode = st.ino \|\| 0;`），判等见 `1287`–`1291` | `rollout.js:1284` |
| E11 | `12845` / `12853` 被列在 **Zed** 名下 | 改归 **LM Studio**（`12843` 段头、`12845` 注释首行、`12853` = `LMSTUDIO_WINDOW_BYTES`）；与 §3.14 正文的自我矛盾解除 | `rollout.js:12843`、`12845`、`12853` |
| E12 | §3.18 引 `sync.js:2013` 为 `dedupeDroidSettingsFilesBySession` | 改为 `sync.js:6595`（全集唯一出现点）；`2013` 改述为 Droid 文件合并起点 | `sync.js:6595` |
| B4 前提 | §5 A 只说 Gemini 唯一，未写依赖 | §5 开头补「**计价侧前提**」：`reasoningIncludedInOutput` 仅含 `codex/acode/every-code/omo/cline`，集合外 source 的推理按 output 计费 ⇒ **Goose reasoning-only 行不是 $0** | `pricing/index.js:248`–`256` |
| I1 | §5 F 类「Codebuddy 未判定」 | 结案归 **D 类**：扩展日志闸门 `10345`；JSONL 支路为含 `reasoningTokens` 的五桶零闸门 ⇒ F 类清空 | `rollout.js:10345`、`10476`–`10485` |
| I2 | §5 E 类「结构上不可能」措辞过强 | 加限定：仅指「六列全零 + `total>0`」不可能；Zed 的 `totalDelta` 只由 4 桶求和（`12751`）且 `<= 0` 跳过（`12752`），缓存-only 时仍可 `input=0/output=0`；Cline 同理 | `rollout.js:12751`、`12752`、`12403` |
| I3 | §0 速览 Opencode 行漏记 SQLite 支路 | 数据形态列补「**+ 同寄主 SQLite 支路**」（`5038` 起），与 sync 两路相加一致 | `rollout.js:5038`；`sync.js:1386`–`1388` |
| I4 | §1.5 把 `12994` 称作「return 块」 | 改述为 `normalizeLocalStudioTokens` **函数头**；返回块为 `13039`–`13049`（`total_cost_usd: 0` 在 `13047`、`conversation_count: 1` 在 `13048`） | `rollout.js:12994`、`13039`–`13049` |
| I5 | §7.1 第 4 条 KiroCli 会话字段名「未核对」 | **结案并推翻**：会话侧字段是 `input_token_count` / `output_token_count`，**不是**注释 `8109` 的 `user_prompt_length`/`response_size`；v2 支路另说明其无 token 字段 | `rollout.js:8371`、`8372`、`8482`、`8475`–`8481` |
| I6 | §7.1 第 2 条 Opencode storage 结构未读 | **结案**：`walkOpencodeMessages` 递归且只收 `msg_*.json` | `rollout.js:5025`、`5033` |
| I7 | §7.1 第 8 条 Anythingllm SELECT 未读 | **结案**：SELECT 本体 `13874`–`13887`，SQL 层只过滤 `whereClause`、不做零值过滤；第 3 条（Cline 目录）同时结案 | `rollout.js:13874`–`13887`、`12054`、`12086` |
| 第 9 条 | init.js OpenClaw 插件安装行号未复核 | **结案**：`installOpenclawSessionPlugin` 调用点 `init.js:1078` | `init.js:1078` |
| 第 5 条 | Kimi / KimiCode home 优先级未逐字确认 | **结案**：`resolveKimiHome`（`9493`，`KIMI_HOME` `9495`，WSL `9497`–`9502`）、`resolveKimiCodeHome`（`9669`，`KIMI_CODE_HOME` `9671`） | `rollout.js:9493`、`9669` |
| §7.1 第 10 条 | 「仅 PowerShell 定位、未复核」的自查清单（把 35 条正确项标错、漏掉真正错项） | **整体重写**为「已用权威口径逐条重验」版本：35 条正确 / 5 条错误 + 清单外 2 条错误 + 1 处归属错误；写明 GBK 漂移机制（行尾字节 ≥ 0x81 吞换行，漂移随位置累积至 15）与「自称 read 确认 ≠ 已重验」的教训 | `REVIEW-R1-C` 行号漂移表；本文件 §7.1 第 10 条 |
| U1 / U2 | Workbuddy traces 字段名、Droid `resolveDroidModel` 三级链 | **保留为 U 组**，写明判定条件：分别需读 `rollout.js:11100`–`11260` 与 `15300`–`15345` | `rollout.js:11112`、`11246`、`15333`、`15338` |

**附带收紧（复核标为通过、本文件顺手对齐口径，不改结论）**：`rollout.js:7338`（`countKiroJsonlLines`）、`8132`/`8138`/`8140`/`8165`（KiroCli 路径）、`10022`（`capCodebuddyUsageFingerprints`）的「未复核」标注删除；Unsloth 投影 SQL 由「`13620` 附近」收紧为 `13634`–`13636`；Droid `prune: true` 由 `2031` 对齐为 `2032`；§3.14 明确 `12845`/`12853` 属 LM Studio 模块。

**未落实项**：无 E/I 项遗留。仍无法验证者仅 U1/U2 两条（已写明所需读段）；原报告的 ±1 容差项（`sync.js:1102`/`1908`/`2062`/`2109`/`2315`、`rollout.js:7305`/`11706`、`init.js:568` 附近）按其容差保留原文，未逐条改写（`sync.js:2031` 已随 E12 一并对齐为 `2032`）。

**样本仓库状态**：本轮仅读取 `C:\Users\admin\AppData\Local\Temp\tt-research`，未写入任何文件；所有改动只落在本分片。

## 来源：S3b-providers-db-api.md
## 26. 存疑与待复核点

以下各项**在源码中未能取得决定性证据**，或存在本文未能闭合的推理链，按「需要复核的优先级」排列。

### 26.1 `TOKENTRACKER_ANTIGRAVITY_HOME` 与用量解析无关（已复核，登记为「易误判点」）

复核结论：该变量**只作用于技能目录**，不覆盖用量解析的家目录。

```js
// src/lib/antigravity-paths.js:24-30（节选）
function resolveAntigravitySkillDirs(env = process.env) {
  const override = trimmed(env.TOKENTRACKER_ANTIGRAVITY_HOME);
  if (override) return [path.join(override, "skills")];

  const home = geminiHome(env);
  const mainSkills = path.join(home, MAIN_APP_SUBDIR, "skills");
```

用量侧的家目录来自 `geminiPaths`（`sync.js:1281-1295`），其 native/wsl 两路分别喂给 `listAntigravityTranscriptsWithStatus`。**待复核的剩余项**：`geminiPaths` 自身的解析函数（`resolveGeminiHome` / `resolveGeminiPaths`）未在本次范围内逐行核对，因此「`GEMINI_HOME` 之类的变量能否重定向 Antigravity 用量扫描」仍未确认。

### 26.2 【未能确认】`antigravity` 估算行是否真的对下游不可见
本文的结论（§21.5）建立在「`usage_precision` 的全部写入点清单里没有 antigravity」这一**否定性证据**上。已核对 `rollout.js` 内 **16 行 / 14 个语句**的 `usage_precision` 引用（清单见 §21.5；R1 已补全 `6725/6731/6755/6760/22113`，原稿「25 处」为误计）与 `session-analytics.js:1155/1338/1730/2640/2709`、`local-api.js` 的聚合路径，但**未逐一核对云端 ingest 侧是否会从 `cursors.files[].usageSource` 反推**。若云端能读到游标状态，披露缺口可能不成立。**判定条件（U2）**：取得云端 ingest 侧读取 `cursors.files[].usageSource` 的代码或运行证据——能反推则披露缺口不成立，不能则维持现状。

### 26.3 【未能确认】`qoder(new)` 全零 token 行是否真的进入队列
`rollout.js:6639-6660` 构造了 `conversation_count = 1`（token 全 0）的 totals 并 `addTotals` 到桶，但**该桶是否会因 `queuedKey` 未变而不入队**取决于 `enqueueTouchedBuckets` 的 `totalsKey` 比较（`rollout.js:3325-3347`）。本文未构造实际运行验证；需以 fixture 复现「billable 零 token 行」跑一次完整 `parseQoderNewIncremental` + 入队，检查队列行是否出现全零 `total_tokens` 且 `conversation_count = 1`。

### 26.4 【未能确认】`reasonix` 是否存在跨 home 重复计数
`reasonixHomeCandidates` 同时返回 `~/.reasonix` 与 `%APPDATA%\reasonix`（`rollout.js:16925-16934`），若两处都存在且互为拷贝，`resolveReasonixTelemetryFiles` 只是去重**路径**（`16970-16975`），**没有按 inode/内容去重**（对比 Copilot 的 `uniqueCopilotDbPaths` 用 `dev:ino`）。需实测 Windows 上双目录并存时的表现。

### 26.5 【未能确认】`trae` 的 `T_STORE` 冲突裁决在 3 台以上安装时的行为
冲突规则只处理「两份拷贝」（`previous[T_STORE] !== store && modifiedAt <= previous[T_MODIFIED]`，`rollout.js:22111`）。三份以上拷贝轮流被扫描时的最终归属未在源码中给出收敛性证明；注释也只声明「不能让陈旧库回撤更新的用量」。

### 26.6 【未能确认】`grok` 兜底路径的时间归属
兜底路径的桶起点为 `toUtcHalfHourStart(event.timestamp) || toUtcHalfHourStart(lastActive) || toUtcHalfHourStart(Date.now())`（`rollout.js:20249-20252`、`20265`）。当 `lastActive` 也缺失时（`grokLastActiveFromSignals` 最终回落 `new Date().toISOString()`，`rollout.js:19691`），**历史用量会被记进「当前」半小时桶**。本文未验证该组合在真实老会话上的触发频率。

### 26.7 【未能确认】`craft` 与 `reasonix` 的 `total_tokens` 是否会在极端截断下失真
两家都用「累计快照 clamp 差值」。Craft 的注释承认负面差值是截断信号并 clamp 到 0（`rollout.js:17189-17195`）；Reasonix 同样 clamp（`rollout.js:17011`）。**若上游重置后在短时间内又累积回原来的量级，这段「重放」的差值会被再次计入**。两家都没有像 Qoder 那样的「先减旧再加新」回撤路径。需实测截断/重置场景。

### 26.8 【未能确认】`devin` 的 `reasoning_output_tokens` 是否真的不存在
`devin-usage.js:190-200` 恒写 0，而 SQL 投影里**也只选择了四个 metrics 字段**（`devin-usage.js:48-51`）。若 Devin 未来新增 reasoning 列，本解析器会静默丢弃。属设计选择而非 bug，但需登记。

### 26.9 【未能确认】21 家的 `conversation_count` 语义统一性
本文已逐家给出语义（请求级 / 消息级 / 会话级），但**没有核对聚合侧（`local-api.js`、`session-analytics.js`）是否对「会话数」做任何按 source 的归一或标注**。跨 provider 汇总时该指标很可能不可加。

### 26.10 【未能确认】`copilot` 三通道并存的最终一致性证明
三份实现共用 `source = "copilot"`，靠 `usageClaims`（OTEL↔store）、`shouldCountCopilotAppSession`（app-db 跳过 CLI/VS Code）、`usageClaimsComplete` 三重机制分工。源码有大量注释说明设计意图，但**没有看到「三通道同时活跃时互斥完备」的证明或对账测试引用**。R1 实证：`test/copilot-app-parser.test.js` 与 `test/copilot-session-store-parser.test.js` 均命中 `usageClaims`/`observeOnly` 关键字（`git grep -ln`），但**「三通道同时活跃时互斥完备」是否被断言未逐用例阅读**。**判定条件（U2）**：给出这两个测试文件的交叉通道用例断言清单（含「同时活跃」场景的输入与期望输出）；缺该清单则维持「未确认」。

### 26.11 已确认但值得登记的「不变量破坏面」
- `rollout.js:6410-6411` 明确声明 cost 永不估算（qoder）；`rollout.js:24055-24058` 明确丢弃上游 cost（command-code）；`grok-usage.js:26-39` 反之采信上游 cost。**同一条「成本」语义在 21 家之间有三种处理方式**，下游聚合时必须按 source 区分。

### 26.12 【未能确认】`trae-cn` 真实响应中是否出现 `input_token=0` 且 `output_token=0` 的会话行（U1）

源码层「允许」已确认：`rollout.js:22477-22492` 只校验 `Number.isSafeInteger(value) && value >= 0`（`0` 合法），`22500-22508` 会写入 `total_tokens: 0`/`billable_total_tokens: 0`/`conversation_count: 1`，`22749-22751` 无零值护栏直接 `addTotals` 进桶（§22.1 第二处）。

**判定条件**：取得一次真实 `query_user_usage_group_by_session` 响应样本（含全部 token 字段为 0 的会话行），或用同形 fixture 跑完整 `parseTraeCnApiIncremental` 并检查队列是否出现全零 `total_tokens` 且 `conversation_count = 1` 的桶行。缺该样本时，结论停留在「源码允许、真实频率未知」。

---

## 27. 引用索引（本文出现过的关键源码位置）

- SQLite 读取层：`src/lib/sqlite-reader.js:33-195`
- OpenCode SQLite：`src/lib/rollout.js:5041-5147`、`5108-5133`、`5761-6028`
- Qoder 旧库：`src/lib/rollout.js:6030-6050`、`6148-6196`、`6218-6399`
- Qoder 新 JSONL：`src/lib/rollout.js:6401-6414`、`6513-6550`、`6552-6789`
- Cursor：`src/lib/cursor-config.js:46-47`、`150-152`、`451-538`；`src/lib/rollout.js:7164-7271`；`src/commands/sync.js:2144-2195`
- Devin：`src/lib/devin-usage.js:25-58`、`163-206`；`src/lib/rollout.js:14104-14347`
- OMP/OmO：`src/lib/rollout.js:11352-11620`、`15701-16112`
- pi 家族：`src/lib/rollout.js:16129-16317`、`16319-16687`、`16689-16786`
- Craft：`src/lib/rollout.js:16788-16905`、`17080-17298`
- Reasonix：`src/lib/rollout.js:16907-17078`
- Copilot OTEL：`src/lib/rollout.js:17301-17586`、`17878-18117`
- Copilot session-store：`src/lib/rollout.js:18119-18169`、`18407-18926`
- Copilot App DB：`src/lib/rollout.js:19190-19379`、`19381-19539`
- Grok：`src/lib/grok-usage.js:1-95`；`src/lib/rollout.js:19541-20557`
- Antigravity：`src/lib/rollout.js:20559-20649`、`21455-21991`、`20800-21450`
- Trae：`src/lib/trae-db.js:1-127`、`src/lib/trae-snapshot.js:1-230`、`src/lib/trae-sqlite.js:1-143`、`src/lib/trae-usage.js:1-121`、`src/lib/rollout.js:21993-22176`
- Trae CN：`src/lib/trae-cn-config.js:1-548`、`src/lib/rollout.js:22311-22800`
- DSH：`src/lib/rollout.js:22800-24050`
- Command Code：`src/lib/rollout.js:24050-24673`
- 队列与聚合基础：`src/lib/rollout.js:3169-3530`、`3996-4190`、`4240-4264`、`4090-4171`
- 账号级来源：`src/lib/source-metadata.js:1-48`

---

## 修正记录（R1 复核后）

本节登记 R1 复核（`REVIEW-R1-D-providers-db-api.md`，E=9 / I=6 / U=3）后的全部改动。所有计数类结论均已按样本仓库源码重新自查（HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`），未修改样本仓库。**E/I 共 15 条全部采纳（无反驳）；U 组 3 条全部保留并写清判定条件。**

| 编号 | 原表述 | 修正后 | 依据（样本源码行号） |
|---|---|---|---|
| E1 | §0 结论 1「23 家中有 4 家必须联网…其余 19 家」（只列 2 家） | 「**21 家**中有 **2 家必须联网**且读凭据（`cursor`、`trae-cn`）；其余 19 家离线可用」；全文「23 家」统一为「21 家」，口径：21 家 = 23 个 `parse*` 函数名 − 2 个共享实现 | §20.1 表 19 行 + §20.2 表 2 行；`rollout.js:15753`、`16319`；`sync.js:2157-2187`、`2234-2262` |
| E2 | §0 结论 3 / §1.3「仓库不依赖 `sql.js`、`better-sqlite3`」「不存在 JS 内嵌引擎路径」 | 限定为「**生产代码 `src/**` 零命中**」；补登 devDependencies 的 `sql.js@1.14.2`（5 个测试 `require`）与 `@electric-sql/pglite@^0.5.8`（此前完全未提）；§1.3 标题同步改 | `package.json:61`、`67`；`test/sync-trae.test.js:9`、`test/trae-db.test.js:7`、`test/trae-snapshot.test.js:6`、`test/trae-sqlite.test.js:3`、`test/trim-embedded-deps.test.js:8`；`src/**` grep 命中 0 |
| E3+E6 | §0 结论 5 / §22.1「**唯一一处**全零 token + `conversation_count=1`」，并把 `craft` 列为第二处 | 改为**两处**：`qoder` 新 JSONL（`6639-6660`）+ `trae-cn`（`22483-22492` 只校验 ≥0、`22500-22508` 写全零 totals、`22749-22751` 无护栏落桶）；`craft` 改为**反例**（`17199-17212` 四分量全零 `continue`） | `rollout.js:17199-17212`、`17243`、`22483-22492`、`22500-22508`、`22749-22751` |
| E4 | §17.1「落在桶键 `rollout.js:22680` 附近」 | 「真实落桶点 `22749-22751`；`22680` 是 fail-closed 抛错处，不是落桶点」 | `rollout.js:22680`（throw）、`22738-22746`、`22749-22751` |
| E5 | §8.2「pi：…（`rollout.js:17171-17223`）」（实际落在 craft 函数体内） | 改为 `rollout.js:16171-16223`（`resolvePiSessionFiles`） | `rollout.js:16171`、`17080`（`parseCraftIncremental` 起点） |
| E7 | §23「不写 `billable_total_tokens` 的 11 家」（实为 10 家/12 项，且漏 `trae`） | 「不写 **13 项**（含 `trae`）；按「家」口径（`copilot` 三通道合算）11 家」+ 逐项自查；显式写 8 项不变，8 + 13 = 21 项与 §20.1/§20.2 对齐 | `git grep 'billable_total_tokens'`：trae 区段（21993-22176）零命中；`17485`/`23212`/`23239` 为校验列表中的字符串 |
| E8 | §21.5/§26.2「已核对 **25 处** `usage_precision` 引用」，清单仅 10 处 | 「**16 行 / 14 个语句**」，清单补 `6725/6731/6755/6760/22113`；否定性结论（无 antigravity 写入点）不变 | `rollout.js:3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286-20287/22011/22025/22113/22157` |
| E9 | §0 结论 2「真正走 SQLite 的有 **8 家**」（只列 7 名） | 统一为「SQLite 数据源 **7 项**（`copilot` 拆两份）；`cursor` 另有 SQLite 仅取凭据、不计入数据源类型」；§20.4 表头「家数」→「项数」并补口径注 | §20.4 表成员；`cursor-config.js:46-47` |
| I1 | §21.1 未登记 `rollout.js:6726` 悬空注释 | 增列：注释称 credits 会累积，但下一行即 `touchedBuckets.add`，上下文无实现；下游须按 totals 白名单消费 | `rollout.js:6726-6727` |
| I2 | 「23 家」计数口径未定义 | 在 §0 首条与 §1 标题定义：21 家（独立数据源实体）与 23 个 `parse*` 函数名（含 2 个共享实现）的关系 | `rollout.js:15753`、`16319` |
| I3 | §1.3「不存在 JS 内嵌引擎路径」措辞过强 | 与 E2 合并限定为「生产路径」；保留 Trae 的 wa-sqlite WASM 例外声明 | `trae-sqlite.js:7-11` |
| I4 | §20.2 `readTraeEntitlementFromStorage` 行号 `22236-22259`（指向相邻函数） | 改为定义 `rollout.js:22269`（`22236-22260` 是 `normalizeTraeEntitlement`）；唯一调用点 `status.js:840`，不进解析路径 | `rollout.js:22269`；`status.js:840` |
| I5 | §10.8 `17008-17013`、`17067-17069`；§7.6 `11612-11616` | 分别改为 `17008-17014`、`17067-17068`、`11612-11615` | `rollout.js:17014`、`17068-17069`、`11615` |
| I6 | §22.3 表未覆盖 `trae`/`trae-cn` 的零值形态 | 表内补「值为 0 是否合法」：`trae` 只保护首次观测、`trae-cn` 无零值护栏；表后加说明 | `rollout.js:22112`、`22483-22492`、`22749-22751` |
| U1 | 原稿未登记（`trae-cn` 真实全零行是否出现） | 新增 §26.12，判定条件：真实 `query_user_usage_group_by_session` 样本，或同形 fixture 跑完整解析检查桶行 | `rollout.js:22477-22492`、`22500-22508`、`22749-22751` |
| U2 | §26.2 / §26.10 无明确判定条件 | 写清判定条件：云端 ingest 是否反推 `usageSource`；两测试文件的交叉通道断言清单（已实证 `test/copilot-app-parser.test.js`、`test/copilot-session-store-parser.test.js` 命中 `usageClaims`/`observeOnly`） | `git grep -ln 'usageClaims\|observeOnly' -- test/` |
| U3 | §26.1 | 保留原样（`geminiPaths` 行级核对未做），判定条件已写明 | `antigravity-paths.js:24-30`；`sync.js:1281-1295` |

**未落实项**：无。I2、I3 分别并入 E1、E2 条目落实；原稿其余内容（含全部未被 R1 证伪的代码块引用）未删改。


## 来源：S4-aggregation-query-dedup.md
## 存疑与待复核点

以下条目在本次研究中**未能确认**，或存在需要复核的风险点。按风险排序。每条后面补上「判定条件」——即闭合它需要读的文件或需要观察的行为（R1 复核确认这些点在原位置确实无法判定）。

### A. 未确认的实现细节

1. `src/lib/fs.js` 的锁实现：`openLock` / `inspectLock` / `updateJsonLocked` 的内部机制（是否 `O_EXCL`、是否写 pid、超时与陈旧锁回收策略）未读。仅确认 sync 用 `sync.lock` 且压缩要求调用方持锁。**判定条件**：读 `src/lib/fs.js` 全文，核对 `openLock` 的创建标志位与 `inspectLock` 的陈旧判定（pid/时间戳），并查 `test/` 中 lock 相关用例。
2. `src/lib/rollout.js` 中 `codexHashes` 事件去重的**哈希输入**（`src/lib/rollout.js:382` 只是游标键名）。**判定条件**：定位写入 `codexHashes` 的构造点，确认哈希覆盖的字段集合（是整行、或用量三元组、或事件时间戳组合），并确认跨文件运行时是否会碰撞。
3. `copilot` 的桶粒度与 `copilotOtelAggregateKey`（`src/lib/rollout.js:17462`）的键构造——可能不是半小时。**判定条件**：读 `copilotOtelAggregateKey` 函数体与其调用点，确认第二个参数来自 `toUtcHalfHourStart` 还是别的桶函数。
4. ~~`src/lib/claude-categorizer.js:1175` 的 `toUtcHalfHourStart` 副本是否与 `src/lib/rollout.js:4173` 版本逐字节等价~~ —— **R1 复核已解除**：两版逐行等价（仅中间变量写法不同），详见 §2.3。
5. 会话分析里跨根"事件级并入"的具体并入键（`test/session-analytics-claude-cross-root-dedup.test.js:174`、`:288` 只锁行为，未定位实现）。**判定条件**：沿会话扫描路径（`scanClaudeSession` 的 `seenMessageHashes` 分支，`src/lib/rollout.js:2854`）继续向下读跨根汇合点，确认并入键是消息哈希、`(session_hash, message_key)` 还是事件指纹。
6. `sessionUsageDay` / `withinDayRange` / `scopeSessionUsage` 三个函数的函数体未逐行读，行为仅由测试反推。**判定条件**：读 `src/lib/session-analytics.js:2343-2400` 区段，核对时区换算（`Asia/Kathmandu` UTC+5:45 场景）与闭区间比较符。
7. `repairZcodeNativeUsageMigration` / `repairZcodeInclusiveTokenMigration` 的写盘方式（是否原子重写、是否删行）。**判定条件**：`src/commands/sync.js:4994` / `:5002` 均委托 `repairZcodeUsageMigration`，需读该函数体确认是「追加零行」还是「tmp+rename 原子重写」，以及是否调用 `dropZcodeQueueRows`（`:4884`）。
8. 端点 `cloud-session`、`local-sync`、`cloud-sync-pref`、`proxy-config`、`proxy-test`、`telemetry-pref`、`subscription-manager`、`skills`、`usage-limits` 的参数与返回字段未逐行确认（表中已标）。**判定条件**：逐个端点读 `src/lib/local-api.js` 对应 `if (p === ...)` 分支的 `url.searchParams.get(...)` 集合与响应 `JSON.stringify(...)` 载荷。部分已解除：`user-status` 的 `account{available, cloud_sync_enabled, account_view}`（`:2927-2931`）、`achievements` 的 `tz*`（`:3470-3471`）、`usage-limits` 存在性（`:3482`）。
9. `test/billable-total-tokens-migration.test.js` 内部无顶层 `test(` 用例，其真实用途未确认（R1 复核复核了「0 用例」这一观察，实测 9 行、`test(` 出现 0 次）。**判定条件**：读该文件全文（9 行）并 `rg` 其文件名的引用方，确认它是被 import 的辅助模块还是失效残留。
10. `src/lib/source-metadata.js` 的 scope 过滤规则（`filterRowsByUsageScope`、`ACCOUNT_LEVEL_SOURCES`）未展开——它决定了"哪些来源进入默认总量"。**判定条件**：读 `src/lib/source-metadata.js` 全文，列出 `ACCOUNT_LEVEL_SOURCES` 成员与 `normalizeUsageScope` 的取值域，并对照 `listExcludedSources`（`src/lib/local-api.js:410`）确认界面提示口径。

### B. 需要复核的风险点

1. **`projectBucketKey` 与 `parseBucketKey` 不通用**：前者布局是 `projectKey|source|hourStart`（无 model），后者按 `source|model|hourStart` 解析。当前调用点上没有误用（`parseBucketKey` 只用于主队列键），但这是"两套键共用一个分隔符、解析器不匹配"的结构性隐患（`src/lib/rollout.js:4228` vs `src/lib/rollout.js:4238`）。
2. **`project_key` 无 model 维度**：项目队列丢失了模型信息，项目详情里的 `models` 字段实际装的是 **source**（`src/lib/local-api.js:2877`）。若下游按"按模型"理解该字段会出错。
3. **`conversation_count` 跨来源不可比**：至少三种口径（Craft 按会话 / Claude 按主会话 **user 文本消息**（`src/lib/rollout.js:2807-2837`，**不是** assistant 消息）/ 按请求 `delta.requests`），源码自己写明了"不可比"这一点（`src/lib/rollout.js:17233-17235`）；且该处注释对 Claude 口径的描述与解析侧代码相反（见 §4.8）。任何跨来源"会话数"汇总都只是粗略值。
4. **`repair` 的删除风险仍在**：`dropMimoQueueRows`（`src/commands/sync.js:4744`）与 `repairClaudeQueueFromGroundTruth`（`src/commands/sync.js:6844`）都属于"原子丢弃 + 重扫"，其安全性依赖"磁盘日志仍在"。v6 事故证明这个前提会破（`src/commands/sync.js:234-247`）。队列是部分历史的**唯一**副本。
5. **本地热力图的 `streak_days` 恒为 0**（`src/lib/local-api.js:2588`）、`week_starts_on` 恒为 `"sun"`（`src/lib/local-api.js:2586`），而云端 compact 载荷会带真实值（`src/lib/heatmap-compact.js:108-109` 透传）。同一 UI 在本地/云两条路径下显示可能不一致。
6. **同一面板里两套时间口径**：热力图网格按 UTC 日（`src/lib/local-api.js:2540`），`night_owl` 徽章按请求时区小时（`src/lib/local-api.js:482`）。
7. **上传偏移清零会广播本地修复结果到云端**（多处 repair 都重置 offset）。这意味着"本地算错 → 修好 → 云被覆盖"是设计路径；反过来，若本地修复本身错误，云端也会被污染。`CLOUD_CONVERSATIONS_BACKFILL_KEY` 的注释明说云端历史**只能**靠本地队列重放修复（`src/commands/sync.js:249-256`）。
8. **半小时桶是 UTC 的，但跨日归属在客户端**：如果客户端不传 `tz`/`tz_offset_minutes`，日归属静默退化为 UTC 日（`src/lib/local-api.js:224`），不会报错。
9. **`normalizeQueueRow` 是读时修补**：历史 codex/cursor 行的修正在每次查询时重算（`src/lib/local-api.js:170-199`）。若将来修改该规则，历史数字会**追溯变化**，且与云端的已上传值可能不一致。
10. **`aggregateByDay` 的缓存以行数组对象身份为键**（WeakMap，`src/lib/local-api.js:104`）：一旦某个调用方对行做了浅拷贝，缓存立刻失效并重算。这是性能敏感点，也是 `test/local-api-queue-cache.test.js:52` 存在的原因。

### C. 对 TokenScope 的移植提示（仅事实性差异，不含方案建议）

- TokenTracker 的**项目身份 = 远程仓库 `owner/repo`**（`src/lib/rollout.js:4429`）；TokenScope 规格要求的是**本地规范化绝对路径 + 层级归并**。二者不可互相替代。
- TokenTracker 的**队列是唯一事实源 + append-only 快照日志**；TokenScope 是 **SQLite `history.db` + 每来源事件**。因此 TokenTracker 的"零行撤回 / last-row-wins / 原子重写 repair"三件套在 TokenScope 里对应的语义不同。
- TokenTracker 的**半小时桶由 UTC 计算**，日归属交给请求参数；TokenScope 规格是**存储 UTC、聚合时区由 settings 决定、查询入口冻结一次"今天"**。TokenTracker 没有"冻结 as_of"概念——每次请求都重新解析 `new Date()`（`src/lib/local-api.js:2382`）。
- TokenTracker 的**去重分层多达七层**，其中"账户级去重"和"双安装命名空间光标"是 TokenScope 目前没有对应物的两个概念。

---

## 修正记录（R1 复核后）

依据 `REVIEW-R1-E-aggregation.md`（E=8 / I=6 / U=7）逐条落实：**E 组 8 条、I 组 6 条全部采纳，无反驳**；计数类数字全部由本轮 FIX-E **独立重算**（UTF-8 读取 + 正则计数）；样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c` 全程只读，未修改。行号一律为样本仓 `src/...` 行号。

### E 组：事实性错误（8 条）

| 编号 | 原表述 | 修正后 | 依据 |
| --- | --- | --- | --- |
| E1 | 「`conversation_count` **按 assistant 消息**（每次消息 +1）」 | 「按**主会话 user 文本消息**计（排除 `tool_result` 与子代理）」，并明确写出**源码注释与实现不一致**：解析侧注释 `:2807-2809` 与代码一致，Craft 侧注释 `:17233-17234` 自称 "count one-per-assistant-message" 与代码冲突，以代码为准；「跨来源不可比」结论保留（§4.8、存疑 B.3 两处同改） | `rollout.js:2807-2837`（`:2833` 落在 `isMainSession && type==="user"` → `hasText` 内层分支）、`rollout.js:17233-17234` |
| E2 | 表 4d「**需 msgId 与 reqId 同时存在**才启用」 | 「**msgId 必需**；有 reqId 时合成 `msgId:reqId`，无 reqId 时**退化为纯 msgId**（去重照常生效）」；§4.4(d) 增补：`if (msgId && reqId)` 是 v3 ground-truth 扫描的缺陷（`sync.js:227-233`），与解析侧函数语义不同 | `rollout.js:4914-4918`；`sync.js:227-233` |
| E3 | 「`toUtcHalfHourStart(` 全仓共 **70 处调用**：65 处在 `rollout.js`」 | 改为分口径表：`rollout.js` **70 次出现 / 66 行** = 定义 1（`:4173`）+ 调用 **69 次 / 65 行**（`:20214` 一行 3 次，`:20265`、`:20418` 各 2 次）；`sync.js` 1（`:6663`）；`claude-categorizer.js` 3（定义 `:1175` + 调用 `:1250`/`:1279`）；**全 `src` 74 次出现 = 72 次调用 + 2 处定义**；`test/` 另有 10 次 | 本轮逐文件重算（正则 `toUtcHalfHourStart\(`） |
| E4 | usage-hourly 为「某日**逐半小时**」（§3.4 表、§7.2 表两处） | 改为「当日**逐小时**（当地小时，24 个整点，两个半小时桶按当地小时合并；响应无 `:30` 键）」 | `local-api.js:600-609`（`hourKey = \`${dayKey}T${HH}:00:00\``） |
| E5 | 「`src/lib/upload-queue.js:20031` 同型」 | 改为 `src/lib/rollout.js:20031`（`upload-queue.js` 全文实测 **134 行**，不可能有 20031 行） | `rollout.js:20031`；`upload-queue.js` 行数重算 |
| E6 | `heatmap-compact.test.js`「**16** 用例」 | 改为 **15**（重算：行首 `test(` = 15，任意位置同为 15，无 `test.each`；文件 217 行） | 本轮重算 |
| E7 | `model-breakdown.test.js`「**30+** 用例」 | 改为 **25**（重算：行首 `test(` = 25，任意位置 27，无 `test.each`；文件 888 行） | 本轮重算 |
| E8 | `dropZcodeQueueRows` 保留撤回行的开关见 `:4894` | 改为 `:4884`（函数签名 `dropZcodeQueueRows(filePath, { retainRetractions = false } = {})`）；`:4894` 实际是 `let removed = 0;` | `sync.js:4884`、`:4894` |

### I 组：不完整（6 条）

| 编号 | 原表述 | 修正后 | 依据 |
| --- | --- | --- | --- |
| I1 | §0 第 5 条「dashboard **所有**用量端点共享同一条聚合链 → `aggregateByDay` → reduce」 | 改为「共享前两跳（`scopedQueueRows` `:403-412`、`rowDayKey` `:213-225`、`readQueueData` `:202-211`），之后分叉」：**仅 summary `:2358` / daily `:2425` / heatmap `:2533` 走 `aggregateByDay` `:227`**；hourly 走 `aggregateHourlyByDay` `:600`；model-breakdown `:2602-2641`、monthly `:3161-3186`、project-summary `:2744-2798`、project-detail `:2866-2898` 各自遍历自建 Map | 端点逐一抽查（含 `:2604-2641`、`:2744-2760` 逐行） |
| I2 | 「`parseBucketKey` 只用于主队列键（`:3199`、`:3434`）」 | 改为「共 **15 处调用**（`:1973`、`:3174`、`:3199`、`:3223`、`:3238`、`:3434`、`:3570`、`:3636`、`:3655`、`:7212`、`:12286`、`:13378`、`:14251`、`:16376`、`:20776`）+ 定义 `:4238`，全部落在主队列键空间」，结论方向不变 | 本轮重算（16 次出现 = 15 调用 + 1 定义） |
| I3 | 500ms 窗只给常量与日期守卫 | 补真正的判定实现 `:2588-2601`（`>=` 重置 `replayPrefixActive`；`<` 置 `forkedReplaySkip`；skip 点 `:2601`），并注明「跨日靠日期守卫、同日靠 500ms 突发检测」；§4.9 表 4b 行同步补行号 | `rollout.js:2588-2601`、`:4212-4214` |
| I4 | 守卫注释引用 `sync.js:263` | 改为 `:264`（注释句首；整段 `:258-267`），§0 第 7 条与 §5.4 第 3 条两处同改 | `sync.js:264` |
| I5 | §1.6 代码块标 `upload-queue.js:114-123` | 改为 `:112-123`（块首 `await output.close();` 在 112，111 是 `output.sync()`） | `upload-queue.js:111-123` |
| I6 | §1.2「优先子集 / `usage_precision` / 模型三个额外维度（`:4167-4170`）」 | 拆开：优先子集 `PRIORITY_SUBSET_FIELDS` 在 `totalsKey` 内（`:4167-4170`）；`usage_precision` 与输出模型拼接在调用点（`:3404-3407`）；`totalsKey` 本体 `:4155-4171` | `rollout.js:4155-4171`、`:3404-3407` |

### 已确证并写入正文的靶子结论

| 编号 | 结论 | 依据 |
| --- | --- | --- |
| T1（B2） | 两套口径**并存不矛盾**：读端 last-wins 用于展示（`async-queue-reader.js:55`/`:86-91`）；`setMaxTotal`（`sync.js:6585-6589`）取 MAX、`mergeMaxTotals`（`:6574-6583`）跨来源取 MAX，**仅**用于 codex rescan/fork 修复的数据丢失守卫（唯一调用族 `:5848-5879`，两处 `skipping to avoid data loss` + `return false`）。原因：撤回零行会把 last-wins 的末行压成 0，用 MAX 才能保住历史峰值。已写入 §1.3 末尾「口径边界」段 | `sync.js:5848-5879`、`:6574-6589`；`local-api.js:202-211`；`async-queue-reader.js:49-57`/`:86-91` |
| T2 | 三份 UTC 桶实现**语义等价**（rollout 权威、claude-categorizer 副本、sync 的 grok 毫秒版），原自标 U **已解除**，已写入 §2.3；存疑 A.4 标记为已解除 | `rollout.js:4173-4190`、`claude-categorizer.js:1175-1191`、`sync.js:4538-4549` |
| T3 | repair 数字自洽：源码 **2.17B = 21.7 亿**；分项 `-1.27B / -474M / -376M / -48M / -6M` 求和 = **2174M ≈ 2.174B**，与总数一致（与 S1/S1b 口径一致）；`0.26.4 HALTS back at v4`（`:242`）+ key 仍为 v4（`:248`）。已写入 §5.3 | `sync.js:234-247`、`:242`、`:248` |
| T4 | 项目身份两条：用量侧 `deriveProjectKeyFromRef`（`rollout.js:4429-4442`）**只吃 URL，无 cwd、无层级归并**；会话侧 `projectKey`（`session-analytics.js:405-408`）取 basename。已写入 §6.1/§6.2 并标「R1 复核确证」；新增残余待验点：`.git` 后缀不剥离 | `rollout.js:4429-4442`；`session-analytics.js:405-408` |

### U 组：保留，并补齐判定条件

7 条 U（U1 fs.js 锁、U2 codexHashes 哈希输入、U3 跨根事件级并入键、U4 sessionUsageDay 三函数体、U5 zcode 修复写盘方式、U6 copilot 桶粒度、U7 若干端点参数）**全部保留**，已在「存疑与待复核点 §A」逐条追加「**判定条件**」（需读的文件/区段，或需观察的行为），并注明 U7 的部分解除项（`user-status` `:2927-2931`、`achievements` `:3470-3471`、`usage-limits` `:3482`）。`billable-total-tokens-migration.test.js` 的「0 用例」观察经复核确认（实测 9 行、`test(` 0 次），用途仍列为 U。


## 来源：S5-pricing-and-cost.md
## 11. 存疑与待复核点

以下条目按「证据强度」从高到低排列。凡未能从源码或测试原文确认的，一律标 `【未能确认】`。

### 11.1 已由源码坐实、但没有测试守护的不一致

1. **`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 双份定义**。`src/lib/pricing/index.js:31` 定义并 `:375` 导出，但**在本文件内从未被读取**；真正生效的是 `src/lib/codex-rollout-parser.js:51` 的另一份同名常量（用于 `:733`）。两处目前都是 `272_000`，改一处不改另一处不会有测试失败。`src/lib/session-analytics.js:245` 的注释还把这个常量当作权威（`in src/lib/pricing/index.js`），实际上它只被注释引用。**【R1 复核状态：已验证】**（`:31` 定义 / `:375` 导出 / `index.js` 全程零读取 / `codex-rollout-parser.js:51` → `:733` 生效）。

2. **推理折叠名单两套**。计价侧 5 个（`codex`/`acode`/`every-code`/`omo`/`cline`，`src/lib/pricing/index.js:248-253`）；token 审计侧 2 个（`codex`/`omo`，`scripts/audit-token-correctness.cjs:34-40`）。两者口径不同，注释各自成理，但没有任何测试把两者绑在一起。**【R1 复核状态：已验证】**（两套集合逐条核对属实，确无测试绑定）。

3. **云端与本地对 9 个 `curated.exact` 键的覆盖不同**（§7.4）。其中 `grok-build-free`（100 万 input token：本地 $0 vs 云端按 `grok-build` 计 $1.25）与 `grok-4.5-build-free`（本地 $0 vs 云端 $2）是**价格层面的实际分歧**，只在 `total_cost_usd` 未上报时才显现。parity 测试不含「edge ⊇ curated」这类断言，抓不到。**这是本次研究中最值得上报的一条。** **【R1 复核状态：已验证，行号采信】**——`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 字节比对；9 键差集逐字重算一致；云端走向 `tokentracker-leaderboard-refresh.ts:481`（`grok-build-free` → `grok-build` 1.25/2.5/0.2）与 `:480`（`grok-4.5-build-free` → `cursor-grok-4.5` 2/6/0.5）逐行确认。

4. **`PI_SUBSCRIPTION_SOURCES` 本地 4 个成员、云端 2 个成员**（`src/lib/pricing/index.js:18-23` vs `tokentracker-leaderboard-refresh.ts:575`）。`prime-agent-*` 两条在云端不会短路为零。**【R1 复核状态：已验证】**

5. **云端 `computeRowCost` 没有长上下文 / Fast 溢价**（`tokentracker-leaderboard-refresh.ts:569-613` 无 `long_context_*` / `priority_*` 分支）。价格表注释把这一点写成有意设计（`curated-overrides.json:98`），但没有测试断言「云端结果必然 ≤ 本地结果」或给出量化允许差。**【R1 复核状态：已验证】**（edge `computeRowCost` `:569-613` 确无 `long_context_*` / `priority_*` 分支）。

6. **金额精度在三处不同**：`toFixed(6)`（`src/lib/local-api.js:2415`、`:2648`）、浮点原值（`src/lib/session-analytics.js:289`）、取整到分（`tokentracker-leaderboard-refresh.ts:1349`）。跨端对账需按端点选择比较口径。**【R1 复核状态：已验证】**

### 11.2 需要外部事实核对、本次未联网确认的点

7. **LiteLLM 上游价的时效性**：`seed-snapshot.json` 的 `_meta.generated_at` 是 `2026-05-26T11:12:29.761Z`（`seed-snapshot.json` 原文），而运行期磁盘缓存的 TTL 是 24 小时（`litellm-fetcher.js:12`）。也就是说：**离线/首启状态下使用的价格表年龄可能远大于 24h**。上游 LiteLLM 仓库当前是否仍以 `model_prices_and_context_window.json` 为唯一价目文件、字段名是否仍为那 4 个成本字段【未能确认】——本次未联网核对上游仓库。

8. **`curated-overrides.json` 注释里引用的第三方定价页**（`platform.claude.com`、`developers.openai.com`、`api-docs.deepseek.com`、`maas.xfyun.cn` 等）与表内数字是否一致【未能确认】——这些是项目自己在 note 里声明的核价来源与日期，本次研究未逐条复核外部页面。

9. **edge 五个文件的部署状态**：本仓库的 `dashboard/edge-patches/` 是「待部署的补丁源」，实际线上函数是否为同一修订【未能确认】。`CLAUDE.md:42` 提到「Deploy the touched edge functions after editing」，说明部署是人工步骤。

### 11.3 明确的否定结论（研究过程中排除的可能性）

10. **不存在** `models.dev` 或 OpenRouter 价源（`src/` 全目录 0 命中；价格加载链只有 seed + LiteLLM + curated 三层）。
11. **不存在**任何定价相关的 `TOKENTRACKER_*` 环境变量（全量盘点后为 0）。
12. **不存在**用户可编辑的外置价格文件（无 `pricing.toml` 之类；`~/.tokentracker/` 下与价格相关的只有 `cache/pricing.json` 这个派生物）。
13. **不存在**批处理折扣、cache 5m/1h 差异、图片/音频独立计费（§4.2、§4.4、§4.5）。
14. **不存在**订阅节省估算（§8.3）。
15. **前端不重复计价**（`dashboard/src/` 下 `MODEL_PRICING` / `computeRowCost` / `getModelPricing` 均 0 命中）。

### 11.4 方法学留痕（便于复核）

行号一律以 `read` 工具的 LF 编号为准（PowerShell `Get-Content` 在本机对该仓库个别文件少算 4–6 行，已用 Node 逐字节验证：`local-api.js` LF=3543、`session-analytics.js` LF=2934、`tokentracker-leaderboard-refresh.ts` LF=1449）。JSON 结构统计（键数、字段直方图、差集、块哈希）由 Node `JSON.parse` + `crypto.createHash` 只读完成；五份 edge 文件的抽取正则与 `test/edge-pricing-parity.test.js:55-56` 完全一致。`git rev-parse HEAD` = `b19d27b0ccd59ce621f4fa17207b06358e67987c`，`git describe --tags --always` = `v1.3.14`。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-F-pricing-consumers.md`（样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`）。下列每条都在本轮**独立重算**后落实，未修改样本仓。行号/键数一律用 `[System.IO.File]::ReadAllLines(...,UTF8)` 或 Node 只读探针重取。

| 编号 | 原表述 | 修正后 | 依据（重算方式 / 样本仓位置） |
|---|---|---|---|
| R1-1 [E1] | §7.2 表头「`MODEL_PRICING` 键数」= **183** | **119**；并注明「块内定价键总数 183 = 119 + 64」 | 花括号配平逐键清点：`dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142-332` = 119 键、`:339-404`（`IFLYTEK_MAAS_MODEL_PRICING`）= 64 键 |
| R1-2 [E2] | `index.js:284-285` | `index.js:283-284` | `src/lib/pricing/index.js:283` = `// annotations, never addends — total_tokens stays the sum of the base columns` |
| R1-3 [E3] | `index.js:288-295`（引 `They are annotations, never addends`） | `index.js:282-283`（整段注解 `:281-285`） | `:282` 行末 `They are`、`:283` 起 `annotations, never addends…`；`:288-289` 实为「The parser records only usage from requests whose cache-inclusive input exceeded 272K」 |
| R1-4 [E4] | `local-api.js:615` | `local-api.js:614` | `:614` = `billable_total_tokens: 0,`；`:615` = `input_tokens: 0,` |
| R1-5 [E5] | `test/pricing.test.js` 1391 行 | **1392** 行 | `ReadAllLines` = 1392 |
| R1-6 [E6] | `src/lib/subscriptions.js` 507 行 | **508** 行 | `ReadAllLines` = 508 |
| R1-7 [I1] | 「与 `curated-overrides.json` 的对照只有 4 处」 | **6 处**（补 `:175` 的 `source_alias?.acode === undefined`、`:207-216` 的 `xsparkx2/xopglm53` deepEqual）；结论不变 | `test/edge-pricing-parity.test.js` 全文 420 行逐处核对 |
| R1-8 [I2] | 「在 edge 有、`curated` 没有的**大量**键」 | 量化为 **44 键**；并声明 iFlytek 64 条不属 `MODEL_PRICING` | 双向 Set 差集：curated-only **9**（逐字一致）/ edge-only **44** |
| R1-9 [I3] | 「全仓库对 `ensurePricingLoaded` 的调用只有两处」 | 「**生产代码**两处」（另有 20 行测试调用） | 全仓 grep 27 命中：`src/commands/serve.js:127`、`src/lib/local-api.js:3340` + `test/pricing.test.js` 16 处、`test/local-api-queue-cache.test.js:149`/`:220`、`test/grok-parser.test.js:301`、`test/omo-parser.test.js:229` |
| R1-10 [I4] | 「价格表只存在于后端与 edge 两处」 | 作用域写明「**生产路径 + `dashboard/src/`**」，并注明 dev/mock 例外 | `dashboard/src/` 下 4 类标识符 0 命中；例外：`dashboard/vite.config.js:508-514`（require 同一 pricing 模块）、`:1052-1057`、`:1067` |
| R1-11 [I7] | 「上游再导出到 `src/lib/local-api.js:60` 与 `:3530`」 | `:60` 是 **import 解构**入口、`:3530` 是 `module.exports` 再导出 | `src/lib/local-api.js:57-64` / `:3527-3531` |
| R1-12 [同源] | §7.1「`edge-patches/` 下另有 20+ 个 `.ts` 文件」 | 共 **28** 个文件（全部 `.ts`），除 5 份价格表外另有 **23** 个 | `Get-ChildItem -File` = 28 |

### 已确认靶子结论（R1 复核后标为已验证，行号已写入正文）

- **B9**：`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 字节比对，**无** `edge ⊇ curated` 断言；差集恰 **9 键**（`claude-opus-5.5`、`claude-sonnet-5.5`、`cline-gateway-models`、`claude-opus-4-8`、`claude-haiku-4-5`、`antigravity-gpt-oss-120b`、`grok-4.5-build`、`grok-build-free`、`grok-4.5-build-free`，本轮逐字重算一致）；`grok-build-free` 云端落 `tokentracker-leaderboard-refresh.ts:481` → `:273` 的 `grok-build` = 1.25/2.5/0.2，本地 $0；`grok-4.5-build-free` 落 `:480` → `cursor-grok-4.5` = 2/6/0.5，本地 $0。
- **B10**：`src/lib/pricing/index.js:31` 定义、`:375` 导出、全文**零内部读取**；生效者 `src/lib/codex-rollout-parser.js:51` → `:733`；`src/lib/session-analytics.js:245` 仅为注释引用。
- **B16**：`dashboard/src/` 零价格副本；逐行计价点 `src/lib/local-api.js:2634-2640`（注释与实现逐字属实）。
- **§10 手工演算**：四段演算 + 3 锚点已用 JSON 原文独立复算，逐值一致。

### 未落实 / 保留为未确认

- **U1**（edge 五份函数的线上部署修订）：仓库内无部署产物或版本指针，仅 `CLAUDE.md:42` 提示人工部署，§11.2 第 9 条保留 `【未能确认】`。
- **U2**（`curated-overrides.json` note 引用的第三方定价页）：该表运行在 2026 年时间线（`claude-opus-5-5`、`gpt-6-astra`、`deepseek-v4` 等），不存在可核对的外部真实价页，联网核对无事实源，§11.2 第 8 条保留。
- 其余 E/I 项之外的分片内容本轮未改动；`index.js` 378、`matcher.js` 439、`curated-overrides.json` 775、`local-api.js` 3543、`session-analytics.js` 2934 等行数断言重算后**逐字命中**，保持原样。

## 来源：S6-consumers-output.md
## 10. 存疑与待复核点

1. **`streak_days` 的权威来源到底是谁。** 本地热力图端点硬编码 0（`src/lib/local-api.js:2588`），前端 `use-activity-heatmap.ts:284-295` 自行重算，Windows 托盘 `UsagePoller.cs:314-328` 注释却说"server-computed"。**【R1 复核状态：B15 已验证（两侧源码逐行属实）；Windows 托盘实际显示值仍未能确认——需运行 Windows 壳实测（U3）。】** 账号视图（`tokentracker-account-heatmap.ts`）是否返回真实 streak，本文**未能确认**（该文件 grep 未命中 `streak`，但紧凑格式解码路径未逐行读完）。
2. **`/functions/tokentracker-local-sync` 成功响应的 `syncResult` 具体键名**未逐字段确认（`src/commands/sync.js` 的 drain 返回值结构未读），本文只确认了 `{ok, uploaded, pending, next_allowed_at?}` 与失败形状（`src/lib/local-api.js:2322`、`:2324`）。
3. **`/functions/tokentracker-context-health` 的字段结构**（`src/lib/context-health.js`）未展开。
4. **`/api/auth/*` 的完整转发规则**（cookie relay、csrf 优先级、relay 回退条件）只读到片段（`src/lib/local-api.js:1792-1940`），本文未逐条列出。
5. **`src/lib/git-outcomes.js` 的“只用 subject 检测 revert”**未逐行核对，仅依据 `docs/PRIVACY.md:21` 的声明。
6. **`dashboard/preload` 的 `limits` 预取具体端点**（`dashboard/src/lib/dashboard-preload.js` 只显示目标名与页面映射，未显示它调用哪个 api 函数）。
7. **`vite.config.js` 的 dev 中间件暴露的路径前缀**未确认（只确认它直读 `queue.jsonl`、自带去重与日聚合、并自带 `computeRowCost`；`dashboard/vite.config.js:610-639`）。**【R1 复核后结案：dev 环境不复制价格表。】**`dashboard/vite.config.js:508-514` 注释明写「Per-model pricing — delegated to `src/lib/pricing/` (CJS)」，并在 `:512-514` 用 `createRequire(import.meta.url)` **直接 require 同一 `src/lib/pricing` 模块**（`getModelPricing` / `computeRowCost` 均来自该模块），dev mock 拿到的就是本地那份表（require 时同步加载 bundled seed）。唯一差异在 `:1052-1057`：dev mock 对**按模型聚合后的 totals 只计一次价**，与 `local-api.js:2634-2640` 修掉的 #653 是同一种写法，仅影响 dev mock，不影响线上。
8. **`usage-limits` 的 provider 数量口径**：`fetchUsageLimitsUncached` 的 `Promise.all` 解构出 18 个结果变量（`src/lib/usage-limits.js:4055`），但 `data` 对象只收录其中 17 家（`claudeServiceStatus` 未进入 `data`，`src/lib/usage-limits.js:4406-4440`），加上 `fetched_at` 共 18 个顶层键；本文未穷举 `ark` 两家（codingPlan/agentPlan）与 `qoder` 两家是否共用同一凭据。
9. **榜单/社区表的 SQL 细节**（`scripts/ops/*.sql`、`migrations/`）只通过测试注释间接确认，未逐行读 SQL 正文。
10. **`docs/PRIVACY.md` 与 `docs/cloud-guide.md` 提到的"compressed usage metrics 可长期保留"**，其具体压缩表与保留期限在源码/文档里未给出最终数值（`docs/cloud-usage-archive.md:74-83` 明确"未启用"）。
11. **macOS 菜单栏小组件的刷新频率**（`WidgetCenter.reloadAllTimelines` 之后的系统策略）与"快照 schemaVersion 2 的兼容迁移"细节未确认。
12. **`TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源**未逐行确认（推测同为本地 API 或经 `UsagePoller` 推送，**未验证**）。

---

## 附：本文未覆盖但相邻的议题

- 各 agent 日志解析与事件去重（S1/S4）。
- 定价匹配、等价键、分段与峰谷规则（S4 §4）。
- 计费/订阅/赠送码、Waffo/Paddle/支付宝/微信 webhook 细节（`dashboard/edge-patches/tokentracker-billing.ts`、`-*-webhook.ts`）。
- SCIP 代码图索引（`graph-auto-index-*` 测试与 `scripts/`）。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-F-pricing-consumers.md`（样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`）。每条均在本轮**独立重算**后落实，未修改样本仓。

| 编号 | 原表述 | 修正后 | 依据（重算方式 / 位置） |
|---|---|---|---|
| R1-1 [E7] | §0-7 / §4.5「全仓 grep `savings\|amount_saved\|saved_amount\|total_saved` **只命中** 1 处」 | **9 处命中 / 分布在 7 个文件**（`amount_saved\|saved_amount\|total_saved` 为 0 命中），逐处列出 | 全仓 grep：`CloudPage.jsx:95`、`:195`、`i18n/zh/dashboard.json:650`、`zh-TW/dashboard.json:709`、`content/copy.csv:1699`、`DESIGN.md:17`、`hosted-archive-acceptance.cjs:377`、`:450`、`docs/cloud-usage-archive.md:70` |
| R1-2 [E8] | §6.3「共 **29** 个文件」 | **28** 个（全部 `.ts`） | `Get-ChildItem -File` = 28 |
| R1-3 [E9] | §6.3 各文件行数与任何单一口径都不符 | 统一**总行数**口径并逐条刷新（account-summary **954**、account-daily **894**、account-heatmap **391**、account-model-breakdown **956**、leaderboard-refresh **1449**、leaderboard-profile **1074**、public-visibility **210**、device-token-issue **183**、device-rename **164**；同表其余条目同步刷新） | `[System.IO.File]::ReadAllLines(...,UTF8)` 逐个重取 |
| R1-4 [E10] | §1.1 `init.js`（2254 行）、`sync.js`（6900 行） | **2427** / **7251** | 同上 |
| R1-5 [E11] | §9 声明取 `Measure-Object -Line`（非空行数），但两行给的是总行数 | 全表统一为**总行数（`read` 口径）**，14 项行数刷新（status.test.js **725**、local-api-security.test.js **1561**、local-api-account-view.test.js **1386**、badge-svg-render.test.js **147**、account-source-parity.test.js **107** 等） | `ReadAllLines` |
| R1-6 [同源] | §6.4 parity 测试「392 行」 | **420** 行 | `ReadAllLines` |
| R1-7 [同源] | §4.2 badge-svg「268 行」、§4.3 embed-svg「289 行」、§7.2 StatusBarController「1336 行」/ DashboardViewModel「1064 行」、§7.3 DashboardWindow.cs「1142 行」 | **295** / **320** / **1507** / **1151** / **1235**（同一口径错误，一并刷新） | `ReadAllLines` |
| R1-8 [I5] | §0-2「两个最忙一天」未给前提 | 补前提：本地 API 请求不带 tz 时回落 UTC；前端一直传 tz，故真实前端路径的分歧成立 | `src/lib/local-api.js:512-519`、`:213-225`、`dashboard/src/lib/api.ts:244-249` |

### 已确认靶子结论（R1 复核后标为已验证，行号已写入正文）

- **B14（成立）**：`src/lib/wrapped-aggregator.js:37-53`（`slice(0,10)` 切天、`slice(0,4)` 切年、`/T(\d{2}):/` 取小时）、`:66-67`（JSDoc 明写「calendar year (UTC)」）、`:148-152`（UTC 步进）、`:170`（峰值小时 UTC）；对照 `src/lib/local-api.js:213-225` + 前端传 tz。前提见 §0 第 2 条。
- **B15（成立）**：`src/lib/local-api.js:2588` 硬编码 `streak_days: 0,`（同响应 `:2587` 服务端算真实 `active_days`）；`dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295` 调 `computeActiveStreakDays(...)` 覆盖；`dashboard/src/lib/pet-linux-host.js:198` 直读 `streak_days`（本地模式拿到 0）。
- **27 个端点（复核确认，未改动）**：本轮自建 Node 探针从 `src/lib/local-api.js` 抽出全部 `/functions/tokentracker-*` 路由字符串并去重清点 = **27 个**，与 §0-3 及 §2.10 总表 27 行一致。
- **上传字段与隐私差口（复核确认，未改动）**：`src/commands/sync.js:4367-4374`（`{upload_id, hourly, account_session_states?}`）；ingest 映射多出 `billable_total_tokens`（`:188`）与 `total_cost_usd`（`:189`）；`account_session_states` 含 `session_id` 且仅收 `trae-cn`（`:139-147`、`:153-160` + `src/lib/upload-queue.js:11-16`）；设备名 = `os.hostname()` 截断 128（`src/lib/local-api.js:29-41`、`:3137-3143`）。
- **`status` 非用量报表（复核确认）**：`src/commands/status.js` 全文 grep `total_tokens\|"cost"\|total_cost` = **0 命中**。

### 结案

- **§10 第 7 条（vite dev 是否复制价格表）→ 结案**：`dashboard/vite.config.js:508-514` 通过 `createRequire` 直接 require 同一 `src/lib/pricing` 模块（`getModelPricing` / `computeRowCost`），**dev 不复制价格表**；唯一差异是 `:1052-1057` 的 dev mock 对按模型聚合后的 totals 只计一次价（与 `local-api.js:2634-2640` 修掉的 #653 同一种写法，仅 dev）。

### 未落实 / 保留为未确认

- **U3**（Windows 托盘在本地模式下连续天数是否显示 0）：仅确认两侧源码（本地硬编码 0 + 前端自行覆盖），需运行 Windows 壳实测；§10 第 1 条保留 `【未能确认】`。
- §10 其余存疑项（2–6、8–12）本轮未触及，保持原状。
- 未被报告涉及、但本轮顺手重算命中的项（`status.js` 1729 行、`test/share-card-data.test.js` 138 行、`test/status-json-light.test.js` 96 行、`upload-throttle.js` 参数、§7 端口矩阵）经核对一致，未改动。

