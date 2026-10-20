# 复核报告 R6：存疑攻坚回写

- 被复核对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md`（4427 行，mtime 2026-10-14 14:47:04）
- 回写依据：`tokentracker-draft/OPENQ-W1-source-reads.md`（14:26:36）、`OPENQ-W2-shells-api.md`（14:27:05）、`OPENQ-W3-corpus-stats.md`（14:26:38）、`OPENQ-W4-experiments.md`（14:27:55）
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`（HEAD `b19d27b0`，全程只读，未做任何写入）
- 行号口径：`read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；未使用裸 `Get-Content`
- 隐私纪律：本报告只含计数、键路径名与样本仓源码行文本；未输出任何本机真实会话内容片段

## 结论

- **判定一致性：满足**。§12 A-1（12 条）/ A-2（6 条）/ B-1（4 条）/ B-2（6 条）共 28 条，与四份攻坚报告的判定逐条对齐；**没有出现「报告说部分解决却写成已判定」或反向**（A7 在文档与 W1 中都标「部分解决」；W2 §8 的 macOS 组件项在 B-2 补充登记中标「部分解决」）。A2 / A18 两条「本机不成立」均为**条件性风险**措辞（「风险仅在其它环境出现该形态/别名时成立」），未写成绝对否定。
- **§13.9 计数与 §12 正文自洽**：各路 8+1+0（W1）/ 8+1+0（W2）/ 10+0+0（W3）/ 8+0+0（W4）与四份报告自报判定一致；A 类 12/18（含 A16）、B 类 4/10 与 A-1/A-2/B-1/B-2 四表行数吻合（12+6=18、4+6=10）。
- **行号：本轮抽查 40 处全部命中**（含 §2.4 / §3.3 / §5.6 / §6.2.x / §9.2.3 / §9.2.7 / §9.6.2 / §9.6.3 / §9.7.4 的新增引用），未发现偏移或错指。
- **必须修 3 项**：① §12.E E4 表头「320 条」与其自身括注（320 + 1）矛盾、且与正文 §5.6 的 321 不一致；② A7「生产调用点共 12 处」实为 11 处；③ §13.9 末条「本轮只增写 §12 与 §13.9」与正文实际回写范围不符。
- **需补出处 2 项**：E5 / §6.2.9 的 `19174 / 960 / 7688` 与 §6.2.3 的 `2128 / 9,740,950,476,478 / 4,578,527,502` 不在四份攻坚报告内（标注的「依据 OPENQ-W3 §3」不成立）——本报告已独立复算，**数值全部正确**，属出处/口径标注问题。
- **独立计数复核通过**：Codex `reasoning_output_tokens > output_tokens` **反例 0**；`cache_write_input_tokens` 在 `message.usage` 键路径 **0 命中**（文档口径成立）。
- **未发现回写引入的新机制性矛盾**（未出现「某处已结案、另一处仍写未确认」的并存），唯一数字冲突即上文 ①。

## A. §12 新结构核验（逐条）

### A-1 已判定（12 条）

| # | 文档结论要点 | 报告依据 | 判定 |
| --- | --- | --- | --- |
| A1 | 半行丢量确定性；offset=381=文件大小；补全后 `eventsAggregated: 0`、桶仍 100、追加 c 后 107；截断重写 714→239 找回、桶 607；缺尾换行不受影响 | W4 §A1 | ✅ 逐值一致 |
| A2 | 22 jsonl / 4609 行 / assistant 1520；别名 0 行命中、同现 0；`cache_creation_input_tokens` 1520 出现 / 1164 非零 / 上限 225051；cache_read 1520/1228；条件性风险措辞 | W3 §5 | ✅ 一致；措辞为条件性 ✅ |
| A3 | `<synthetic>` 6 行 / 4 文件；usage 缺失 0、非零 0（12 键各 6 次） | W3 §6 | ✅ 一致 |
| A4 | 三组 fixture：unknown 合并行 `{input 600, output 60, conversation_count 3}`；单模型组保留真实名；双模型组 `unknown`、input 700 | W4 §A4 | ✅ 一致 |
| A5 | `readQueueBatch` 新行胜出 400、`nextOffset 592 = st.size`；`maxBuckets=1` 时分批各自上传（296/592） | W4 §A5 | ✅ 一致 |
| A8 | 裁剪 100001→100000、seed-0/1 丢弃；淘汰后重计 1、未淘汰仍 0；端到端 100002→100000、桶 400 双计成立；UNC 混合形态被 `fileId` 拦住 | W4 §A8 | ✅ 一致 |
| A9 | `rebuildingCodexBaseline` 条件（同 inode / 未截断 / 0<prevOffset<size / lastTotal 非对象）→ 强制重扫；`:456-460` 兜底在稳态短路之后；`:436` 第一优先 | W1 §9 | ✅ 一致 |
| A11 | 两组零形态均 `messagesProcessed/eventsAggregated/bucketsQueued: 1`、模型 `qmodel_38max`；对照 `billable:false` 为 0；保留「真实频率未覆盖」限定 | W4 §A11 | ✅ 一致 |
| A12 | 全零通过校验、写出全零 totals + `conversation_count: 1`、`skippedRows: 0`；另产 `account_session_state` 全零行；缺 model 回落 `trae-cn-unknown` | W4 §A12 | ✅ 一致 |
| A16 | 1000→500→1200 三轮：craft/reasonix 均 1100→1870、虚增 550；clamp 机制；已迁 C25 并保留编号 | W4 §A16 | ✅ 一致（迁出标注完整） |
| A17 | MAX 读 900 vs last-wins 读 400（`divergent: true`）；可达性 qoder 1010→510；MAX 族仅服务 rescan repair 守门（`:5848-5873`） | W4 §A17 | ✅ 一致 |
| A18 | `model/rerouted` / `model_rerouted` 0 命中；`session_meta.payload.model` 0/78；provenance 30/78、turn_context 62/64；20/21 一致、1 不一致、4 文件双值；条件性风险措辞 | W3 §4 | ✅ 一致；措辞条件性 ✅ |

### A-2 部分解决 / 维持（6 条）

| # | 文档判定 | 报告依据 | 判定 |
| --- | --- | --- | --- |
| A6 | 维持（四路未覆盖） | 四份报告均未覆盖 | ✅ 与「维持」一致 |
| A7 | **部分解决**（异常写回语义已定；「是否重复解析」未定；补更强判定路径 + 调用点清单） | W1 §8「部分解决」 | ✅ 判定一致；**调用点计数错（12→11），见必须修 #2** |
| A10 | 维持（要求通道交叉用例清单） | 未覆盖 | ✅ |
| A13 | 维持（跨 home 重复计数） | 未覆盖 | ✅ |
| A14 | 维持（三份以上拷贝收敛性） | 未覆盖 | ✅ |
| A15 | 维持（grok 兜底时间归属） | 未覆盖 | ✅ |

### B-1 已判定（4 条）

| # | 文档结论要点 | 报告依据 | 判定 |
| --- | --- | --- | --- |
| B6 | 18 并发位 ↔ data 17 家；`claudeServiceStatus` 为 `claude.service_status` 子字段（`4279-4280`）；Ark 共用凭据族 / Qoder 分离（`qoder-limits.js:819-827`）；`stale` 门槛 10 分钟 | W2 §5 | ✅ 一致 |
| B7 | 壳内 5 分钟计时器（`TrayApplicationContext.cs:84`、`:965-974`）；`TOKENTRACKER_NATIVE_SYNC_OWNER` 注入（`ServerManager.cs:694,700`）；**反向口径：serve 侧兜底仅 Windows**（`serve.js:304`） | W2 §1 | ✅ 一致（含反向口径） |
| B8 | 「`syncResult` 键名」前提被推翻；成功响应顶层展开 `{ok, code, stdout, stderr, uploaded, pending, next_allowed_at?}`；context-health 无包装；`/api/auth/*` 8 条规则 | W2 §4 | ✅ 一致（前提推翻已写明） |
| B9 | 无 SQL 视图；小时级簇去重 → 日 → 全时段物化表 → RPC；聚合层不读反作弊表、不读 `billable_total_tokens`；SUPERSEDED 与旧表 backfill 两条警示；仓库外 5 项 | W2 §6 | ✅ 一致 |

### B-2 尚需外部信息（6 条）+ 补充登记

| # | 文档判定 | 报告依据 | 判定 |
| --- | --- | --- | --- |
| B1–B5、B10 | 6 条维持（判定条件保留/换新） | 四份报告未覆盖（B1 的否定性证据在 W1 §6 复核成立，文档已注明） | ✅ |
| 补充① macOS 快照迁移 | 已解决（不存在 v1→v2 迁移分支，`:362` 严格相等 + `:354-356` 注释） | W2 §8 | ✅ |
| 补充② macOS 刷新频率 | 部分解决（源码侧全给；系统侧缺证据） | W2 §8 | ✅ |
| 补充③ QuotaWidget 数据来源 | 已解决（本地 API 取数 + 轮询器推送、组件零取数） | W2 §2 | ✅ |
| 补充④ `~/.tokentracker/logs/` | 已结项（以仓库为准不存在创建点） | W2 §3 | ✅ |
| 补充⑤ `hookStatus` 11 传 8 消费 + 3 死回退别名 | 已核对 | W1 §7 + W2 §9 | ✅ |
| 补充⑥ `usage_precision` 取值域/消费方（16 行 / 15 条） | 已复核成立 | W1 §6 | ✅ |
| 补充⑦ `codexServiceTier` 语料统计 | 已换本机口径 | W3 §3 | ✅ 数字一致；**出处标注见需修 #4** |
| 补充⑧ CodeBuddy/WorkBuddy `SessionEnd` | 仍未确认 + 本机否证据 | W3 §10 | ✅ |

### C 类新增 C25

`craft` / `reasonix` 截断重放：三轮 1000→500→1200、桶 1100→**1870**、虚增 **550**（= 被 clamp 差额）、`Math.max(0, current - prev)` + 快照无条件替换、两家无 Qoder 式回撤、触发频率未统计 —— **与 W4 §A16 逐值一致** ✅。引用 `src/lib/rollout.js:17189-17195`、`:17011` 已回源确认（craft 段注释逐字为「Negative deltas mean the session was reset/truncated — clamp to 0 and replace the snapshot」；`:17011` 属 `diffReasonixTotals` 的 `Math.max(0, …)` 行）✅。

### §12.E（E1–E8）与 W3 数字对照

| 项 | 文档数字 | W3 数字 | 判定 |
| --- | --- | --- | --- |
| E1 | 65381 对 / 0 反例 / 上界 1.0000 / `reasoning>0` 60715 / `==` 328 / 同为 0 322 / 100% 同现 | §1 表 + §30 补充：完全一致 | ✅ |
| E2 | 各 27802、同现 100%；total 52 文件 49 单调 / 3 回退（−2965231 / −3780242 / −34082848）；last 36 文件回退；thread 17 文件单调 | §2：一致 | ✅ |
| E3 | output 求和 35351865647 vs 16395234、约 2000 倍 | §2 结论：一致（2156 ≈ 2000） | ✅ |
| E4 | **320 条**（括注：320 落 last、1 落 total） | §2 正文 321 对（last 320 + total 1）；§2 结论段 320 条 | ❌ **表头 320 与括注 321 矛盾；正文 §5.6 用 321** → 必须修 #1 |
| E5 | 32/64、433 事件、385/48；归类 19174 / 960 / 7688 | §3 给 32/64、433、385/48、445 与 12；**无 19174/960/7688** | ⚠️ 前段 ✅；归类数字出处缺失（独立复现一致）→ 需修 #4 |
| E6 | 别名 0、rerouted 0、`<synthetic>` 6 行全 0、顶层 usage 0、thinking 1288（406 非零）/`reasoning_output_tokens` 0、Antigravity total 类 0、zcode `cacheWriteTokens` 87/87 | §5/§6/§7/§9/§10：一致 | ✅ |
| E7 | 14/22 子代理文件（1098 行）、535 assistant 全带 usage、isSidechain 535 重合、meta.json 无 token | §8：一致 | ✅ |
| E8 | 三处无 `SessionEnd` 配置 | §10：一致 | ✅ |

### §13.9 计数

- 表内 8+1+0 / 8+1+0 / 10+0+0 / 8+0+0 ↔ W1「已解决 8 / 部分 1」、W2「已解决 8 / 部分 1」、W3「已判定 10」、W4「8 条全部已判定」✅。
- 处置段「A 类 12 条（A1/A2/A3/A4/A5/A8/A9/A11/A12/A16/A17/A18）、1 条部分（A7）、5 条维持」「B 类 4 条（B6–B9）、6 条维持」 ↔ §12 表 ✅。
- ⚠️ 末条「**本轮只增写 §12 … 与本节 13.9，未改动任何既有行号引用**」与事实不符（正文多处含「2026-10-14 攻坚补记/结案追记」，主文档 mtime 14:47 晚于四份报告 14:26–14:27）→ 必须修 #3。

## B. 正文回写抽查（逐条）

| # | 位置 | 抽查内容 | 回源结果 |
| --- | --- | --- | --- |
| B1 | §2.4 L194 | `logs/` 不存在创建点；三壳落盘路径（`tracker-paths.js:5-11`、`Diag.cs:11-13`、`server.rs:587-604`） | ✅ 两处回源命中（tracker-paths 返回四目录；Diag 落 `%LOCALAPPDATA%\TokenTracker\windows-host.log`）；与 W2 §3 判定一致 |
| B2 | §3.3 L316–331 | 主流程表与 `cmdSync` 边界（`:516-3709`、`module.exports :3821-3857`） | ✅ 抽样 `:3824` 为 `readQueueBatch` 导出点，与 A5 引用互证 |
| B3 | §3.3 L333 | `skipNoOpCursorCommit` 条件项 **13** 个（`:3424-3437`） | ✅ 回源：`const skipNoOpCursorCommit =` 于 `:3424`，条件 3425–3437 共 13 行 |
| B4 | §3.3 L349 | 锁常量与三分支（`fs.js:69-74,296-307,117-136,244-254,342-368,264-275`）、`updateJsonLocked` 30 s/10 ms（`:416-443`）、迭代清扫 `:178-204` | ✅ 抽样 6 处命中（`:419 timeoutMs = 30_000, retryMs = 10`；`:342-346` reclaimGuard；`:264-275` 仅 token 匹配才 unlink） |
| B5 | §3.3 L384 | `claudeFileIdentityHash`（`:937`，前 64 KB）、`fileId` 认领条件（`:1056-1105`） | ✅ `:936-937` 命中（`CLAUDE_FILE_ID_PREFIX_BYTES = 64 * 1024`、函数定义） |
| B6 | §3.3 L385 | Claude `${message.id}:${requestId}` + 10 万条截断（`:1233-1237`） | ✅ `:1233-1237` 命中（注释「cap at 100k entries」） |
| B7 | §3.3 L386 | `needsHistoricalRolloutDedup` 与 `prev.lastTotal` 兜底（`:428-461`） | ✅ `:428-437` 命中（`if (rebuildingBaseline) return true;`） |
| B8 | §5.6.1 L1524–1528 | A 类语料证据段（321 对、codex「弱化版」、gemini 不成立、grok 未取语料） | ⚠️ 与 W3 一致但**与 E4 的 320 冲突**（正文此处为 321，口径更正确）；数值本身（6456–205888）与 W3 一致 |
| B9 | §5.6.2 L1553–1558 | 全零行入队实验表（A11/A12） | ✅ 与 W4 §A11/§A12 逐值一致（含 `skippedRows: 0`、`account_session_state` 行、`billable:false` 对照 0） |
| B10 | §5.6.4 L1596 | 云侧补记（`billable_total_tokens` 不入榜单；迁移 SQL 全文 7 行；测试 9 行不注册 `test()`） | ✅ 与 W1 §5 + W2 §6.2 一致 |
| B11 | §6.1.2 L1724 | A2 条件性风险（本机不成立）+ 判定条件保留 | ✅ 与 W3 §5 一致；「条件性风险」标题与 A-1 措辞一致 |
| B12 | §6.2.3 L1981 | 同现 100%、49/3 回退、36 文件回退、`2128` 与 `2156` 两口径、`16/17` 精确相等 | ⚠️ 数字经独立复算**全部正确**（见 C 节），但 `2128` 口径不在 W3 → 需修 #4 |
| B13 | §6.2.6 L2062 | 0 反例 / 1.0000 / 60715 / 328 | ✅ 与 W3 §1 一致 + 独立复现 0 反例 |
| B14 | §6.2.7 L2092–2094 | 上游注释 0 of 10310 / 5849（`:18-20`）+ 本机 0/78、30/78、62/64、20/21、4 文件双值 | ✅ 注释在 `codex-model-attribution.js:18-19` 逐字命中；本机数字与 W3 §4 一致 |
| B15 | §6.2.9 L2121 | 445 事件（12 无键）、385/48、归类 19174/960/7688、上游 1213/167/25% 不可外推 | ✅ 内部自洽（433 = 385+48；445 = 433+12）+ 归类数字独立复现一致（见 C 节）；⚠️ 出处标注见需修 #4 |
| B16 | §9.2.3 第 22 行 | local-sync 顶层展开、无 `syncResult`、失败形态 | ✅ 与 W2 §4a 一致；§9.7.4 #3 引「第 22 行」核对为 `-local-sync` 行 ✅ |
| B17 | §9.2.4 L3123 | `/api/auth/*` 8 条规则与四个行号段（`:1644-1657`、`:1809-1841`、`:1864-1869`、`:1884-1907`、`:1922-1942`） | ✅ 与 W2 §4c 逐条一致（含 409 两码、mobile 回退、stale-CSRF 救援） |
| B18 | §9.2.7 L3171 | usage_precision 七态 + 前端缺 `"estimated"` + 五个 session-analytics 行号 | ✅ 回源 `:1155`、`:1338`、`:1730`、`:2640`、`:2709` 全部命中；`sessions-api.ts:113` 命中（联合缺 `"estimated"`）；`rollout.js:20286`、`trae-usage.js:94` 命中 |
| B19 | §9.6.2 L3494 | 小组件无迁移分支 + 三层刷新（`:362`、`:354-356`、`:108-110`、`:36-38`、`.after(now + 15 分钟)`） | ✅ 与 W2 §8 一致（含「系统侧预算零证据」） |
| B20 | §9.6.3 L3498–3502 | 5 分钟计时器 + QuotaWidget 取数链 + 「一分钟兜底仅 Windows」 | ✅ 回源命中（`TrayApplicationContext.cs:84`、`:965-974`；`ServerManager.cs:694,700`；`serve.js:304-309`） |
| B21 | §9.7.4 L3563–3578 | 12 条按攻坚结果分区（#3/#4/#5/#7/#12 结案等） | ✅ 与 W2 §附映射一致；引用的「§9.2.3 第 22 行 / 第 10 行」核对正确 |

抽查 21 组（≥15 ✅），覆盖 §2.4 / §3.3 / §5.6 / §6.1.2 / §6.2.3 / §6.2.6 / §6.2.7 / §6.2.9 / §9.2.3 / §9.2.4 / §9.2.7 / §9.6.2 / §9.6.3 / §9.7.4。

**同事实跨章节一致性**：语料数字在 §5.6、§6.2.x、§12.E 三处并行出现，逐项比对后只有一处不一致（E4 的 320 vs 正文 321）；`433` 与 `445` 在 §6.2.9（445 + 385/48）与 §12 补充登记/E5（433 + 445/12）之间**不构成矛盾**（同一集合的两种分解）。

## C. 关键数字独立复核

方法：Node `readline` 全量流式（不整文件读入）+ 真实 JSON 解析；只输出计数、键路径名；脚本落在 `%TEMP%`（`r6-recount.cjs` / `r6-recount2.cjs` / `r6-recount3.cjs`），未修改样本仓库与主文档；未输出任何会话内容。

**C1 Codex `reasoning_output_tokens > output_tokens` 反例数**

| 口径 | 结果 |
| --- | --- |
| 全量扫描范围 | 64 文件 / 179776 行 / 0 解析失败（与文档一致） |
| 递归配对口径（凡同时含两键的对象） | 65498 对、**violations = 0**、`reasoning > 0` 60787 对 |
| 五类路径口径（`last` / `total` / `payload.usage` / `turn` / `thread`） | 55604 对（= 27802×2）、**violations = 0**；其余三类按 W3 的分组在 `token_usage_record` 下（65381 = 27802×2 + 3259×3，算术自洽） |
| **裁定** | **「0 反例」独立成立** ✅。我的 65498 / 60787 与 W3 的 65381 / 60715 差 117 / 72，属递归配对口径多计（含非 5 类的嵌套 usage 对象），不是文档缺陷 |

**C2 `cache_write_input_tokens` 命中**

| 口径 | 结果 |
| --- | --- |
| **键路径口径**（递归收集键名恰为 `cache_write_input_tokens` 的路径） | **命中 0 个键实例** → 文档 / W3 的「0 行命中」（`message.usage` 内）**成立** ✅ |
| 整行子串口径（`line.includes(...)`，含会话正文与工具参数） | 43 行（`user` 27 / `assistant` 9 / `attachment` 3 / `queue-operation` 4；其中 18 行同时含 `cache_creation_input_tokens`） |
| 裁定 | 两者不矛盾：43 行是**内容层**命中（正文/参数里出现该字段名），不是 usage 字段。**建议在文档补一句「按键路径口径」**，否则后来者用 grep 复核会误判为 43 |
| 附带 | Claude 侧共 22 文件 / 4609 行 / 0 解析失败 / `assistant` 行 1520 —— 与文档三项数字精确一致 ✅ |

**C3 为裁定 ①④而做的加测（超出要求）**

| 项 | 我的复算 | 文档 | 裁定 |
| --- | --- | --- | --- |
| `output_tokens == 0 && total_tokens > input_tokens` | **321 对**（`last_token_usage` **320** / `total_token_usage` **1**） | E4 表头 320（括注 320+1）；§5.6 正文 321 | **321 为正确口径**（正文对、E4 表头错） |
| `total_token_usage.total_tokens` 求和 / `last_token_usage.total_tokens` 求和 | 9,740,950,476,478 / 4,578,527,502（比值 2127.5） | §6.2.3：「≈2128」 | ✅ 精确一致 |
| output 列口径 | 35,351,865,647 / 16,395,234（比值 2156.2） | E3 / §6.2.3：「≈2156」 | ✅ 精确一致 |
| `thread_settings_applied` 事件 | 433 条**带** `service_tier` 键（385 + 48），0 条无键命中（我的行过滤跳过了不含该串的行） | 445 事件 = 433 + **12 无键** | ✅ **自洽**：445 与 433 是同一集合的两种计数，文档两处采用正确 |
| token_count 事件按前置 `thread_settings_applied` 归类 | `default` **19174** / `priority` **960** / 无前置 **7688**（合计 27822） | §12.E E5、§6.2.9：完全相同 | ✅ 精确一致（但 W3 报告未收录该口径 → 出处需补） |

## 必须修清单

| # | 位置 | 问题 | 建议修法 |
| --- | --- | --- | --- |
| 1 | §12.E E4 表头（L4180） | 写「**320 条** `output_tokens = 0` 而 `total_tokens > input_tokens`」，同一行括注却写「其中 320 条落在 `last_token_usage`、1 条落在 `total_token_usage`」（加总 321）；正文 §5.6.1（L1526）/§5.6.7（L1630）均为 321 | 表头改为 **321 条**（并在括注保留「last 320 / total 1」）；或表头限定为「320 条落在 `last_token_usage`」。独立复算支持 **321** |
| 2 | §12 A-2 A7（L4079） | 「生产调用点共 **12** 处」，但清单只列 11 个行号；实测 `sync.js` 内 `multiInstallParse(` 恰为 **11 处**（1419/1467/1545/1686/1722/1776/1994/2063/2325/2379/2478，与列出的行号一一对应）——源头 W1 §8 同错 | 计数改 **11**；行号全部保留（实测全对） |
| 3 | §13.9 末条（L4335） | 「本轮只增写 §12（…）与本节 13.9，**未改动任何既有行号引用**」与事实不符：§2.4、§3.3、§5.6.1–5.6.4、§6.1.2、§6.2.3、§6.2.6、§6.2.7、§6.2.9、§9.2.3（第 22 行）、§9.2.7、§9.6.2、§9.6.3、§9.7.4 均含 2026-10-14 攻坚补记/结案追记（主文档 mtime 14:47 晚于四份报告 14:26–14:27） | 改为准确范围：§12 重写 + §13.9 新增 + 正文若干处的「攻坚补记」与结案标注；删去「只增写 §12」的限定 |
| 4 | §12 补充登记（L4089）、§12.E E5（L4181）、§6.2.9（L2121）；§6.2.3（L1981） | ① `19174 / 960 / 7688`（归类口径）标「依据 `OPENQ-W3-corpus-stats.md` §3」，但 W3 §3 **无**该项；② §6.2.3 的 `9,740,950,476,478 / 4,578,527,502 / ≈2128` 亦不在四份报告内 | 两处数字**经本轮独立复算全部正确**，缺的是出处：补注「回写阶段独立复算（口径：归属最近一条前置 `thread_settings_applied`／`total_tokens` 全列求和）」或回填进 W3 报告 |
| 5 | §12.E E6 / A2（建议级） | 「`cache_write_input_tokens` 0 命中」按**键路径**成立；按整行子串口径会得到 43 行（内容层命中） | 补一句口径说明（「键路径级；正文内容提及不计」），避免后续用 grep 复核误判 |
| 6 | §5.6.1 L1515（建议级） | 「**本机真实语料已观测到该形态**」易被读成「已观测到 $0 形态」，而实测只是弱形态（`total > input + output`，output 缺项但 input 非零） | 收紧为「已观测到**弱形态**（差额落在可计费分量之外）」，与同节 L1526 的澄清保持一致 |

## 复核到的行号抽样

抽查 40 处（样本仓路径省略 `src/` 以外层级按原文写法），命中 40/40：

| 引用 | 回源内容（摘要） | 结果 |
| --- | --- | --- |
| `rollout.js:4927-4928` | `cacheCreation` / `cacheRead` 取值 | ✅ |
| `codex-token-usage.js:25-27` | `cache_creation_input_tokens ?? cache_write_input_tokens` | ✅ |
| `rollout.js:17189-17195` | craft「clamp to 0 and replace the snapshot」+ `Math.max(0, …)` 五行 | ✅ |
| `rollout.js:17011` | `diffReasonixTotals` 的 `Math.max(0, current[key] - …)` | ✅ |
| `rollout.js:1233-1237` | `cap at 100k entries` + `slice(len - 100_000)` | ✅ |
| `rollout.js:1021-1025`、`:1035` | UNC/本地混合去重注释 + `seenFileIds = sawUncFile && sawLocalFile ? … : null` | ✅ |
| `rollout.js:3181-3194`、`:3430-3480` | `groupQueued` / `legacyGroups` 收集与合并写出、`:3480` 回写 | ✅ |
| `sync.js:216-220` | v3 修复注释（`groupQueued` 遗留标记、`v3 leaves groupQueued alone`） | ✅ |
| `sync.js:4422-4512`、`:3824` | `readQueueBatch` 主体（`:4510` 同键 `set`、`:4512` `maxBuckets`）与导出点 | ✅ |
| `local-api.js:202-211` | `readQueueData` 的 `parseRow` 键 `source|model|hour_start` | ✅ |
| `sync.js:6529-6589`（`:6540`） | `mainUsageHourTotalsFromQueueRows` + `setMaxTotal` 调用/实现 | ✅ |
| `sync.js:5848-5873` | rescan repair 的 `rebuilt >= existing` 守门 | ✅ |
| `rollout.js:6639-6660` | qoder-new 全零 totals 构造块 | ✅ |
| `rollout.js:3325-3347` | 入队侧 `usage_precision` 参与 `queuedKey` | ✅ |
| `rollout.js:22477-22492`、`:22500-22508`、`:22749-22751` | trae-cn 字段校验 / 全零 totals / 落桶 | ✅ |
| `rollout.js:493-504` | `rebuildingCodexBaseline` + `startOffset` + `lastTotal` | ✅ |
| `session-analytics.js:1155`、`:1338`、`:1730`、`:2640`、`:2709` | usage_precision 写入/透传/混合改写 | ✅ |
| `codex-model-attribution.js:18-20`、`:47-51` | 「0 of 10310 … 5849 local rollouts」/「no model/rerouted event observed」 | ✅ |
| `sessions-api.ts:113` | 类型联合缺 `"estimated"` | ✅ |
| `rollout.js:20286` | `mergeGrokUsagePrecision` 调用 | ✅ |
| `sync.js:1069-1085`、`:1443-1445` | cursor-store retry 重跑（Codex / opencode） | ✅ |
| `tracker-paths.js:5-11` | 四目录（无 `logs`） | ✅ |
| `Diag.cs:11-13` | `%LOCALAPPDATA%\TokenTracker\windows-host.log` | ✅ |
| `fs.js:419`、`:206-212`、`:342-346`、`:264-275`、`:178-204` | 30 s/10 ms、心跳 `utimes`、reclaimGuard、仅 token 匹配 unlink | ✅ |
| `rollout.js:936-937`、`:1056-1060` | 64 KB 前缀哈希与 fileId 缓存 | ✅ |
| `sync.js:3424-3437` | `skipNoOpCursorCommit` 13 条件项 | ✅ |
| `TrayApplicationContext.cs:84`、`:965-974`、`ServerManager.cs:694,700`、`serve.js:304-309`、`usage-limits.js:4279-4280`、`local-api.js:2526-2530` | B 类与 §9.6 引用 | ✅ |
| `multiInstallParse` 调用点 | 实测 11 处（见必须修 #2） | ⚠️ 计数 |

**结论性提示**：本轮回写的行号质量与四份报告的抽样命中率（≈100%）一致；问题集中在**三处计数/口径表述**与**两处数字出处**，均为定点可修，不涉及机制性结论。
