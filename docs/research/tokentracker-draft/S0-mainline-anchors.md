# S0 主线亲证锚点（由主研究（主线 agent）亲自打开源码核实）

> 样本：`C:\Users\admin\AppData\Local\Temp\tt-research`，`xiufengsun/TokenTracker`，
> HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14，提交时间 2026-10-13 21:49:05 +0800）。
> 本文件只收录**主线亲自 read 过**的断言，用于：(a) 合成主文档时的骨架；(b) 对抗式复核的 A 组靶子。
> 行号口径：文件实际行号（read 工具口径，含空行）。

## A. 入口与骨架

| # | 断言 | 位置 |
| --- | --- | --- |
| A1 | 命令分发：`run(argv)` 用 switch 分发 `serve/init/sync/status/diagnostics/doctor/uninstall/device-login/wrapped/sessions`；**无参数默认 `cmdServe(argv)`**；未知命令抛 `Unknown command` | `src/cli.js:12-66` |
| A2 | 帮助文本自述：`sync parses ~/.codex/sessions/**/rollout-*.jsonl and ~/.code/sessions/**/rollout-*.jsonl, then uploads token deltas`；`sessions` 导出 metadata-only 分析 | `src/cli.js:97-101` |
| A3 | 数据目录：`~/.tokentracker/{rootDir,tracker,bin,cache}` 由 `resolveTrackerPaths()` 拼接 | `src/lib/tracker-paths.js:4-13` |
| A4 | 队列与状态文件路径常量：`queue.jsonl`、`queue.state.json`、`project.queue.jsonl`、`project.queue.state.json` | `src/commands/sync.js:569-572` |
| A5 | 本地 API 读队列路径：`~/.tokentracker/tracker/queue.jsonl`（`os.homedir()` 拼接） | `src/lib/local-api.js:83-86` |
| A6 | `cmdSync` 是单一大函数（`src/commands/sync.js:516` 起，文件 7251 行），内部线性调用全部 provider 解析器（约 1066–2300+ 行区间） | `src/commands/sync.js:516`、`1066`、`1201`、`1258`、`2173`、`2280` 等 |
| A7 | 采集双路：hook 触发 + 被动扫描。`passive-mode.js` 注释自述「某些环境 SessionEnd hook 永不触发，但底层 CLI 仍会写日志」 | `src/lib/passive-mode.js:4-11` |
| A8 | source 作用域：`cursor`、`trae-cn` 被标为 **account 级**来源（可被 personal/local 过滤排除） | `src/lib/source-metadata.js:1`、`7-13` |

## B. 桶与队列（写入端）

| # | 断言 | 位置 |
| --- | --- | --- |
| B1 | 桶键常量：`DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`、`BUCKET_SEPARATOR = "|"` | `src/lib/rollout.js:55-57` |
| B2 | `bucketKey(source, model, hourStart) = source\|model\|hourStart`；另有 `projectBucketKey = projectKey\|source\|hour`、`groupBucketKey = source\|hour`、`parseBucketKey` 反向解析 | `src/lib/rollout.js:4221-4245` |
| B3 | 写入核心 `enqueueTouchedBuckets({queuePath, hourlyState, touchedBuckets})`：只重算被触碰的 `(source,hour)` 组，逐模型比对指纹 `totalsKey(totals)` 与上次写入值 `bucket.queuedKey`，**相同则跳过**（幂等去重） | `src/lib/rollout.js:3169-3200`、`3326-3327`、`3404-3407` |
| B4 | 队列行字段（真实写法）：`source, model, hour_start, input_tokens, cached_input_tokens, cache_creation_input_tokens, output_tokens, reasoning_output_tokens, total_tokens, billable_total_tokens (?? total_tokens), total_cost_usd (|| 0), usage_precision?, ...prioritySubsetOf(totals), conversation_count` | `src/lib/rollout.js:3329-3345`、`3410-3425` |
| B5 | 物理写入是**纯追加**：`fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8")`；仅在有新增行时执行 | `src/lib/rollout.js:3482-3484` |
| B6 | 撤回机制（retraction）：当 unknown 桶被「对齐」到真实模型（`alignedModel` / `nextAligned` / dominant model）时，会先追加一条**全零行**抵消旧归属，避免重复计入 | `src/lib/rollout.js:3272-3317`、`3351-3402` |
| B7 | `unknown` 桶对齐逻辑：无模型名的用量先落 `unknown`，再由 `pickDominantModel` / `findNearestCodexModel` 归属到当小时主导模型；`every-code` 走 codex 主导模型对齐 | `src/lib/rollout.js:3262-3267`、`3353-3356` |
| B8 | 旧格式兼容：`hourlyState.groupQueued` 记录 group 级指纹，命中 `legacyGroups` 的组走 group 汇总路径（模型数=1 时用该模型名，否则回落 `unknown`） | `src/lib/rollout.js:3181-3194`、`3430-3480` |
| B9 | 桶累加器 `getHourlyBucket(state, source, model, hourStart)` 与 `getProjectBucket(...)` 存在 | `src/lib/rollout.js:3996`、`4019` |
| B10 | 项目维度队列另有入口 `enqueueTouchedProjectBuckets({projectQueuePath, projectState, projectTouchedBuckets})` | `src/lib/rollout.js:3489-3493` |

## C. 队列（读取端）

| # | 断言 | 位置 |
| --- | --- | --- |
| C1 | 读取键 = `${source}\|${model}\|${hour_start}`；`kind === "account_session_state"`/`"account_sync_watermark"` 的行被**丢弃**（返回 null） | `src/lib/local-api.js:202-211` |
| C2 | last-row-wins 语义：解析时 `target.set(entry.key, entry.row)` 到 Map —— 同键后者覆盖前者 | `src/lib/async-queue-reader.js:51-57`、`86` |
| C3 | 读取缓存签名：`dev:ino:size:mtimeNs:ctimeNs`（bigint stat，对打开的 fd stat），追加/重写/原子替换都会让缓存失效 | `src/lib/async-queue-reader.js:8-10`、`30-35` |
| C4 | 增量续读：同一 inode 且 size 增长时，用 head 探针 + 断点锚（anchor）校验后**从上次 offset 继续**读，否则整读 | `src/lib/async-queue-reader.js:36-47` |
| C5 | 大文件不阻塞事件循环：64 KiB 分块（`CHUNK_BYTES`）、每块后 `setImmediate` 让出；未换行结尾的残留行进入 provisional `tail` 但**不进入 committed 集合**（按 key 可能被后续完成的行改变） | `src/lib/async-queue-reader.js:5`、`60-80`、`87-91` |
| C6 | 坏行容错：`JSON.parse` 失败计入 `malformed`，读完后 `console.error` 汇总「skipped N malformed line(s)」，不抛错 | `src/lib/async-queue-reader.js:51-57`、`95` |
| C7 | 读时修补 1：legacy Codex/every-code 行 `input_tokens` 含缓存读（判据 `total_tokens === input_tokens + output_tokens` 且 `cached > 0` 且 `input >= cached`）→ 读时减去 `cached_input_tokens` | `src/lib/local-api.js:155-178` |
| C8 | 读时修补 2：`cursor`/`antigravity` 行 `billable_total_tokens < total_tokens` 时抬到 `total_tokens`（修 0 billable 导致总量消失，issue #106） | `src/lib/local-api.js:179-198` |
| C9 | 日聚合缓存：`dailyAggregationCache` 是 `WeakMap<rows, Map<cacheKey, result>>`，cacheKey 含时区与**定价 revision**；定价在聚合中途刷新会重建而非混用两个 revision | `src/lib/local-api.js:104-117`、`227-249` |
| C10 | 行日键 `rowDayKey(row, timeZoneContext)`：有有效时区时用 `Intl.DateTimeFormat` 分区格式化，否则退回 `hour_start.slice(0,10)`（UTC 日） | `src/lib/local-api.js:213-225` |
| C11 | 项目用量读路径会**过滤掉 claude-mem 观察者伪项目**（`CLAUDE_MEM_OBSERVER_PROJECT_KEY`，由 `rollout.js` 的 ref 派生以防改名失同步） | `src/lib/local-api.js:88-94`、`125-140` |

## D. 定价与金额（主线完整读过 `pricing/index.js` 与 `pricing/matcher.js`）

| # | 断言 | 位置 |
| --- | --- | --- |
| D1 | 成本公式（**只用五桶，绝不用 `total_tokens`**）：`((input*P_in) + (output*P_out) + (cached*P_cache_read) + (cache_creation*P_cache_write) + reasoningCost) / 1_000_000` | `src/lib/pricing/index.js:257-264` |
| D2 | 推理 token 计费门控：`codex/acode/every-code/omo/cline` 的 `reasoning_output_tokens` 视为 output 子集 → `reasoningCost = 0`；其余来源按 `pricing.output` 单独计 | `src/lib/pricing/index.js:248-256` |
| D3 | 权威成本优先：来源在 `SOURCES_WITH_AUTHORITATIVE_COST = {"grok","cline"}` 且行上 `total_cost_usd > 0` 时**直接返回该值**（0 视为「未上报」哨兵，继续走价格表） | `src/lib/pricing/index.js:32`、`236-241` |
| D4 | 免费来源短路：`LOCAL_INFERENCE_SOURCES = {"lmstudio"}` → 成本 0；`PI_SUBSCRIPTION_SOURCES = {pi-github-copilot, pi-copilot, prime-agent-github-copilot, prime-agent-copilot}` → 成本 0（订阅边际成本） | `src/lib/pricing/index.js:18-27`、`228-232` |
| D5 | DeepSeek 峰谷：`DEEPSEEK_TIME_PRICED_MODELS` 命中且判定 off-peak 时四类单价**一律 ×0.5**；`acode` 来源豁免（AStudio 用讯飞 MaaS 固定价） | `src/lib/pricing/index.js:34-39`、`209-222` |
| D6 | off-peak 判定：行上显式 `pricing_tier` 优先；否则北京时间周末（`Date.UTC(2026,7,22,16,0,0)` 起，按 +08:00 平移后取 weekday）判 off-peak；工作日按 UTC 小时，**peak = [1,4) ∪ [6,10)**，其余为 off-peak | `src/lib/pricing/index.js:183-207` |
| D7 | OpenAI 长上下文溢价：阈值 `OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000`；仅 `gpt-5.6`/`gpt-5.6-sol`/`gpt-6-astra`/`gpt-6-sol`/`gpt-6.1-sol` 系列启用；溢价 = `longInput*P_in + longCached*P_cache_read + longCacheWrite*P_cache_write + 0.5*longOutput*P_out + 0.5*longReasoning*P_out`（对已有基础价**再加一次**溢价部分） | `src/lib/pricing/index.js:31`、`266-306` |
| D8 | priority/Fast 档：`pricing.priority_multiplier > 1` 才启用；溢价 = `(multiplier-1) * (priorityBase + priorityLong)`；`priority_long_context_*` 是「既长上下文又 priority」的交集列（队列行不含上下文子集 → 交集为 0） | `src/lib/pricing/index.js:273-275`、`310-354` |
| D9 | 三态语义：`getModelPricingInfo` 返回 `status ∈ {priced, free, unpriced}`（命中但四价全 0 → free；未命中 → unpriced），`source` 记录命中的规则名；LM Studio/Pi 订阅直接返回 `free` 且标注 `local_inference`/`subscription` | `src/lib/pricing/index.js:141-176` |
| D10 | 匹配顺序（0→7）：`curated:source-exact` → `curated:exact`(含 dot 还原/包含式 exact) → `litellm:exact` → `curated:alias` → `curated:fuzzy`(子串) → `litellm:strip`(推理后缀) → `litellm:prefix-strip`(provider 前缀剥离，多命中取字典序最小) → `litellm:fuzzy`(反向子串，长键优先) → miss | `src/lib/pricing/matcher.js:3-12`、`252-386` |
| D11 | 推理后缀剥离模式：`-xhigh-fast/-high-fast/-medium-fast/-low-fast/-xhigh/-high/-medium/-low/-fast`（首个命中即剥） | `src/lib/pricing/matcher.js:14-31` |
| D12 | L4 反向子串的防误配：拒绝「模型代际被更短数字版本吃掉」——匹配后缀若以 `.` 或数字开头则跳过（注释：Opus 5.5 与 Opus 5 是不同 SKU） | `src/lib/pricing/matcher.js:236-250` |
| D13 | 反向子串方向性：只接受「模型包含 key」而非「key 包含模型」（注释：避免 `gpt-5` 反向命中 `gpt-5-pro`） | `src/lib/pricing/matcher.js:372-383` |
| D14 | LiteLLM 条目换算：per-token → per-million（×1e6），保留 10 位有效小数（避免 `1e-7*1e6=0.09999999999999999`）；缺字段**保持缺失**，由调用方 `(x || 0)` 兜底；`_` 开头的 meta 键跳过 | `src/lib/pricing/matcher.js:388-427` |
| D15 | 每来源模型名归一化器登记表：`acode, antigravity, claude, cursor, pi-anthropic, prime-agent-anthropic, zed, unsloth, workbuddy`（**仅影响查价，原始名仍用于存储/展示**） | `src/lib/pricing/matcher.js:194-204` |
| D16 | 价格表加载：require 时**同步**读包内 `seed-snapshot.json`（否则测试/mock 会拿到全 0）；`ensurePricingLoaded()` 之后再升级为磁盘缓存 `~/.tokentracker/cache/pricing.json` 或上游；`revision` 每次加载自增，负缓存与 info 缓存随之清空 | `src/lib/pricing/index.js:33`、`44-71`、`73-110`、`137-155` |
| D17 | 未知模型负缓存：`negativeCache` 按 `source\0model` 记忆未命中，避免每次全表遍历 | `src/lib/pricing/index.js:120-131` |
| D18 | 兼容导出：`MODEL_PRICING = curatedOverrides.exact`（供旧测试读 `kiro-agent` 等条目；LiteLLM 条目**不在**其中） | `src/lib/pricing/index.js:357-363` |

## E. 覆盖范围自述（README，需与源码交叉验证）

| # | 断言 | 位置 |
| --- | --- | --- |
| E1 | 自述「45 AI coding tools」，并给出逐家清单（Claude Code、Codex CLI、AStudio、Cursor、Gemini CLI、Antigravity、Kiro、OpenCode、OpenClaw、Every Code、Hermes、GitHub Copilot、Kimi Code、CodeBuddy、WorkBuddy、WorkBuddy AI、Grok Build、oh-my-pi、OmO、pi、Dots、Prime Agent、Craft Agents、Reasonix、Kilo CLI、Kilo Code、Roo Code、Zed Agent、Goose、Droid、Mimo Code、DuMate、ZCode、Qoder、AnythingLLM、Claude Science、DeepSeek Harness、TRAE Work CN、LM Studio、Unsloth Studio、Devin CLI、Cline、MiniMax Code、Command Code、TRAE） | `README.md:150`、`308` |
| E2 | 「Supported AI Tools」表逐家给出 Detection/Method（SessionEnd hook、TOML notify、被动 SQLite/JSONL/OTEL 等） | `README.md:236-283` |
| E3 | 自述安装策略：hook 类（Claude/Codex/AStudio/Gemini/Every Code/CodeBuddy/WorkBuddy/Grok Build）、插件类（OpenCode/OpenClaw）、被动类（其余） | `README.md:285-292` |
| E4 | `grep parse*Incremental src/lib/rollout.js` 命中 **45** 个解析器（与 45 tools 数量一致） | `src/lib/rollout.js`（45 处函数定义，见主文档 §4） |

## F. 主线尚未亲自核实、留给分片/复核的区块

- `init.js`（82 KB）hook 安装的**逐家细节**（配置文件路径、写入的 hook 命令原文、幂等/回滚）。
- `cmdSync`（6900 行）内部完整阶段顺序与游标（`cursors.hourly.buckets` / `cursors.projectHourly`）结构。
- 45 家 provider 的**逐家**字段映射（S2/S3a/S3b）。
- `local-api.js` 端点全集与 dashboard 消费（S4/S6）。
- 各类 repair 迁移的影响面（S4）。
- 定价 `curated-overrides.json` / `seed-snapshot.json` 的真实条目与来源（S5）。

## 复核提示（给对抗式复核 agent）

- A/B/C/D 四组的行号均为「主研究亲自打开并引用」的锚点：请**逐条打开核对**，任何与源码不符者记为【错误】。
- 重点证伪方向：B3/B6 的「幂等 + 撤回」是否真能防止重复计入；C7/C8 的读时修补判据是否会误伤正常行；D6 的 peak 区间是否读反；D11 的剥离顺序是否会导致误配；D14 的「缺字段保持缺失」是否真被调用方兜底。
