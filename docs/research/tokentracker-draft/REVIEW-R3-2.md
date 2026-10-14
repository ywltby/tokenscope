# 复核报告 R3-2：主文档 §5–§6

## 结论

**范围**：主文档 `docs/research/2026-10-14-tokentracker-usage-statistics.md` 第 861–2130 行 —— §5「45 家 provider：逐家读取方式」全部（861–1607）与 §6「Claude Code 与 Codex 深度解剖」全部（1608–2130），共 1270 行。
**样本**：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14），`src/lib/rollout.js` 实测 24932 行。全程只读，未改动样本仓与主文档。

**方法**：`[System.IO.File]::ReadAllLines(path,[Text.Encoding]::UTF8)` + `read` 逐条打开；另做全量交叉验证——`function parse*Incremental` 枚举、`billable_total_tokens` / `usage_precision` / `toUtcHalfHourStart` / `sidechain` / `endOffset = st.size` / `const …_SOURCE` 全量 grep 与逐行定位；否定性断言（「唯一」「两处」「五家」「没有任何一家」）不做字面采信，一律**自己重建判定集合**。

四条结论：

1. **行号抽样 72 条，命中 64 条，命中率 88.9%**（8 个抽样行含错引、共 9 处：其中 4 处硬错——WorkBuddy `911`、Kimi 路径 `9501`、§5.6.5 opencode `12405`、`codex-service-tier.js:17`；2 处函数/守护定位错——`claudeUserIdentity :921-933`、下界保护 `:952`；3 处 1–2 行偏移——Cline `12350`、AnythingLLM `14040`、trae-cn `22506`）。**§5.1.2 的 45 个函数定义行号与 §5.1 宣称的 `251`–`24458` 区间 45/45 全中**，可认为该索引已是硬事实。
2. **未发现整段编造**。§5/§6 的机制性描述（差值引擎、compaction 侧信道、桶键与撤回、缓存语义分叉、去重指纹）逐条回源成立；三处高价值定量证据均能逐字定位：`~6–7x cost inflation`（`rollout.js:4864-4872`）、`0 of 10310 session_meta rows across 5849 local rollouts`（`codex-model-attribution.js:18-20`）、`1213-file corpus / 167 files / 25%` 与 `default 814 / priority 58`（`codex-service-tier.js:8-9`、`22`）。
3. **五处否定性/唯一性断言被证伪或必须限定**（B.2 逐条给重建证据）：§5.6.1 A 类「唯一一家」、§5.6.2「只有两处」、§5.6.1「数据库/API 子集没有任何一家只填 total」、§5.2.20+§5.6.7「五家」清单、§5.6.7 第 2 条自相矛盾。另有 §5.1 读表说明的「6 处不一致」不可复现。
4. **内部一致性 12 项抽查，11 项一致，1 项硬矛盾**：§4.6 自述的写入端零值撤回行与 §5.6.2「全零 token 落桶只有两处」互相否定（C8）。§4 骨架口径（桶键、队列行、累加器、五列计费、`endOffset` 三处、`usage_precision` 写入点）与 §5/§6 完全对齐。

问题集中在**否定性计数的边界**与**少量跨函数错引**，不在机制叙述本身；下游若照抄 §5.6 的三条结论（A 类唯一、两处、五家），会漏掉 grok / codex / qoder 旧库 / Claude 用户行 / reasonix / craft 六个面。

---

## A. 行号抽样

| # | 文档引用（相对样本仓根） | 文档断言要点 | 源码实测 | 判定 |
|---|---|---|---|---|
| 1 | `src/lib/rollout.js:251`–`24458` | 45 个 `parse*Incremental` 的定义区间 | 枚举得 45 个 `async function parse*Incremental`，首 251、末 24458 | 命中 |
| 2 | §5.1.2 全表 45 行「定义行」 | 45 个函数名 ↔ 定义行 | 45/45 逐行匹配（含 `parseMinimaxCodeIncremental:16775`、`parseDshIncremental:23678`） | 命中 |
| 3 | `rollout.js:10651` / `11004` | WorkBuddy variant 当 `source` | `const WORKBUDDY_VARIANTS = {` 在 10651；11004 `getHourlyBucket(hourlyState, variant, model, bucketStart)` | 命中 |
| 4 | `rollout.js:15753` / `16319` | 两个共享实现壳 | 15753 `parseOmpLikeIncremental`、16319 `parsePiLikeIncremental` | 命中 |
| 5 | `rollout.js:6877`–`6883` | Claude Science 家目录 | 6877 `resolveClaudeScienceRoot`，6882 `return override \|\| path.join(home, ".claude-science")` | 命中 |
| 6 | `rollout.js:281`/`494`/`521`/`3227` | `parseRolloutIncremental` 按条目 `source` 分流三家 | 281 `const defaultSource = normalizeSourceInput(source) \|\| DEFAULT_SOURCE;`；494 `fileSource === DEFAULT_SOURCE \|\| fileSource === "acode"`；3227 `if (source !== "every-code") continue;` | 命中 |
| 7 | `rollout.js:55`–`56` / `4222`–`4226` / `4254` | `DEFAULT_SOURCE`/`DEFAULT_MODEL`/桶键/小写化 | 55–56 常量；4222 `function bucketKey`；4254 `normalizeSourceInput`（`toLowerCase()` 在 4256） | 命中 |
| 8 | `rollout.js:4099` / `4110` / `3328` / `3327` | `addTotals`、`billable_total_tokens` 兜底、队列行、同键跳过 | 4099/4110 一致；3328 `toAppend.push(`；3327 `if (bucket.queuedKey === key) continue;` | 命中 |
| 9 | `rollout.js:4743` / `2971` / `4945` | Gemini `total = max(上报, 分桶和)`；闸门把 total 当非零 | 4743 `const total = Math.max(reportedTotal, computedTotal);`；4953 `"total_tokens",` 在 `isAllZeroUsage` 键表内；2971 `if (!delta \|\| isAllZeroUsage(delta))` | 命中 |
| 10 | `rollout.js:2094`–`2105` | OpenClaw `input` 先减 `cacheRead` | 2094–2105 逐行一致（`openclawInput = Math.max(0, openclawRawInput - openclawCached)`） | 命中 |
| 11 | `rollout.js:2055` | `totalTokens` 只做行级预筛 | `if (!line.includes('"usage"') \|\| !line.includes("totalTokens")) continue;` | 命中 |
| 12 | `rollout.js:226` / `sync.js:1224`、`1234` | Gemini 目录约定与 `tmp` 拼接 | 1224 `process.env.GEMINI_HOME \|\| path.join(home, ".gemini")`；1235 `listGeminiSessionFiles(path.join(geminiPaths.native, "tmp"))` | 命中 |
| 13 | `sync.js:1272` / `1440` / `1110` / `2679` | 调用侧传 `source` | 1272 `source: "gemini"`；1440 `"opencode"`；1110 `"openclaw"`；2679 `"workbuddy-ai"` | 命中 |
| 14 | `rollout.js:5025`/`5033`/`243` | OpenCode 消息文件收集 | 5025 `walkOpencodeMessages`；5033 `startsWith("msg_") && endsWith(".json")`；243 `listOpencodeMessageFiles`（245 拼 `"message"`） | 命中 |
| 15 | `rollout.js:4755` | `normalizeOpencodeTokens` | 4755 定义；4757–4770 五桶 + `total = input+output+reasoning+cached+cacheWrite` | 命中 |
| 16 | `rollout.js:3109`/`3120` | OpenCode 差分为空即丢 / 会话计数 | 3109 `diffGeminiTotals`；3120 `delta.conversation_count = 1;` | 命中 |
| 17 | `rollout.js:7567`/`7282`/`7298`–`7304`/`7547`/`7549` | Kiro IDE source、suffix、库路径、零闸门、UTC 补 `Z` | 全线逐行一致 | 命中 |
| 18 | `rollout.js:7541`/`7572`/`7338`/`8112` | Kiro 自增 id 水位、JSONL 行号、与 CLI 合并注释 | 7541 `let maxId = lastDbId;`；7572 `if (usingDb && row.id && row.id > maxId) maxId = row.id;`；8112 `token. Source is merged with Kiro IDE (source='kiro')` | 命中 |
| 19 | `rollout.js:8018`/`7718`/`7999`/`8006` | Hermes source、SQL 列名即桶名、`ended_at ?? started_at`、模型兜底 | 逐行一致（7718 SQL 末段 `reasoning_tokens > 0)` 即零值闸门） | 命中 |
| 20 | `rollout.js:9191`/`9210`/`9414`/`9446` | Kiro CLI `source='kiro'`、回撤负数 delta、会话支路闸门只检 4 列 | 9192 `addTotals(prevBucket.totals, { input_tokens: -prev.input_tokens,`；9203 `conversation_count: -1,`；9414 闸门无 reasoning；9446 `total_tokens: input + output + cacheRead + cacheCreation + reasoning` | 命中 |
| 21 | `rollout.js:8371`–`8387`/`8118`/`8475`–`8481` | Kiro CLI 先真实后估算、`CHARS_PER_TOKEN = 4`、v2 无 token 字段 | 逐行一致 | 命中 |
| 22 | `rollout.js:9611`/`9586`–`9590`/`8078`/`8080`/`9540` | Kimi source、字段映射、四列零闸门、无 hook 注释、模型来自配置 | 9611 `getHourlyBucket(hourlyState, "kimi", kimiModel, bucketStart)`；8078 `// No hook installation needed; Kimi writes wire.jsonl automatically.`；9540 `const kimiModel = model \|\| resolveKimiDefaultModel(...)` | 命中 |
| 23 | `rollout.js:9501`（§5.2.7 路径） | `~/.kimi/sessions/<workspace>/<session>/wire.jsonl` | **9501 = `wslProviderDir: ".kimi",`**（`resolveKimiHome` 内）；路径装配在 9507 `resolveKimiWireFiles` / 9510 `path.join(kimiHome, "sessions")` | **错例** |
| 24 | `rollout.js:9844`/`9802`–`9817`/`9823`/`9828`/`9703`–`9704` | Kimi Code `source='kimi'`、三种历史形状、零闸门、毫秒时间、默认模型 | 逐行一致 | 命中 |
| 25 | `rollout.js:10384`/`10337`–`10343`/`10345`/`10476`–`10485`/`9924`/`10022` | CodeBuddy source、非缓存输入修正、扩展支路闸门不含 reasoning、JSONL 五桶闸门、兜底模型、指纹上限 | 逐行一致 | 命中 |
| 26 | `rollout.js:911`（§5.2.10） | WorkBuddy `input = prompt_tokens − cacheRead − cacheCreation` | **911 = `const source = typeof entry === "string" ? DEFAULT_SOURCE : normalizeSourceInput(entry?.source) \|\| DEFAULT_SOURCE;`**（文件枚举通用段）；正确点 10963 `const inputTokens = Math.max(0, promptTokens - cacheRead - cacheCreation);` | **错例** |
| 27 | `rollout.js:10907`/`10955`–`10966`/`11085`–`11113`/`10987` | WorkBuddy `providerData.rawUsage`、缓存镜像取最大、traces 支路、模型链 | 逐行一致（10955–10959 三镜像取 max；11105 `input = totalInput − totalCached`） | 命中 |
| 28 | `rollout.js:11939`/`11906`/`11910`/`11805`/`11814`/`11767` | Roo source、字段、零占位不入 seen、模型正则、三级兜底、复用 IDE 根 | 逐行一致（11913–11917 注释：`Marking the zero placeholder as seen would skip the back-filled tokens forever`） | 命中 |
| 29 | `rollout.js:12396`/`12356`/`12381`/`12383`/`12405`/`12054`/`12086`/`12136` | Cline source、字段、`cost` 参与跳过、五路差分、会话计数、路径发现 | 逐行一致 | 命中 |
| 30 | `rollout.js:12350`（§5.2.12 导入闸门） | `if (importedAtMs !== null && ts <= importedAtMs) continue;` | 该语句在 **12352**；12350 是 `const ts = Number(msg.ts);` | 轻微偏移 |
| 31 | `rollout.js:12782`/`12553`–`12556`/`12585`/`12593`/`12594`/`12597`/`12601`/`12742`/`12732`/`12751`–`12752`/`12764`/`12816` | Zed 全线 | 逐行一致（12764 起 `updated_at ?? created_at ?? thread.updated_at`；12752 `totalDelta <= 0` 分支；12816 `> 10_000`） | 命中 |
| 32 | `rollout.js:12852`–`12854`/`13091`/`13094`/`13129`/`13325`/`13039`–`13049` | LM Studio 常量、模型、时间兜底 mtime、三态重建、共用归一化器 | 逐行一致（13129 `const fallbackTimestamp = initialStat.mtimeMs;`） | 命中 |
| 33 | `rollout.js:13467`/`13634`–`13636`/`13650`/`13667`/`13677`/`13714`/`13717`/`13730` | Unsloth 常量、投影写死 0、模型链、归一化、时间、快路径、落桶 | 逐行一致（13634–13636 三个 `0 AS …`） | 命中 |
| 34 | `rollout.js:14053`/`14013`/`14016`/`13808`/`13874`–`13887` | AnythingLLM source、字段、零闸门、库路径、SELECT 本体 | 逐行一致 | 命中 |
| 35 | `rollout.js:14040`（§5.6.3 reasoning 为差额） | `reasoning = max(0, total − input − output)` | 公式在 **14042**（`const reasoningTokens = Math.max(0, reportedTotal - accounted);`）；14040 是模型兜底行 | 轻微偏移 |
| 36 | `rollout.js:14730`/`14670`/`14686`/`14711`/`14713`/`14489`/`14503`/`14529` | Goose 全线 | 逐行一致 | 命中 |
| 37 | `rollout.js:15381`/`14972`–`14979`/`15210`/`15328`/`15379`/`14797`/`14836` | Droid 全线（含 total 兜底填充、mtime 定桶） | 逐行一致 | 命中 |
| 38 | `rollout.js:15651`/`15624`/`15628`/`11730`/`11706` | Kilo `source='kilo-code'`、字段、零占位、provider slug、IDE 根 | 逐行一致（11730 `normalizeKilocodeProviderToModel`） | 命中 |
| 39 | `sqlite-reader.js:33`–`45`/`67`–`79`/`81`/`91`–`92`/`148`/`158`–`159`/`17`–`31`/`119`–`126` | 两级后端、只读参数、默认超时/缓冲、一次性告警、`throwOnReadFailure` | 34 `readOnly ? ["-readonly","-json",dbPath,sql] : …`；67 `readSqliteRowsWithNode`；91–92 `30_000` / `50 * 1024 * 1024`；19 `if (warnedSqliteReadFailures.has(key)) return;` | 命中 |
| 40 | `source-metadata.js:1` | `ACCOUNT_LEVEL_SOURCES` | `const ACCOUNT_LEVEL_SOURCES = new Set(["cursor", "trae-cn"]);` | 命中 |
| 41 | `cursor-config.js:46`–`47`/`102`–`111`/`114`–`116`/`150`–`152`/`196`/`231`/`367`/`498`/`512`–`538` | 凭据 SQL、userId 链、cookie、WorkOS 前缀、三端点与超时、CSV `cost`、`normalizeCursorUsage` | 逐行一致（150–152 三个 URL；196/231 `30000`；367 `15000`；536 `billable_total_tokens: totalTokens`） | 命中 |
| 42 | `trae-cn-config.js:11`–`16`/`24`–`33`/`35`/`37`–`45`/`46`–`49`/`332`/`465`/`486` | tc v5 blob 结构、强制 opt-in、凭据键、端点与 `usage_type`、8 层二分、重试一次 | 逐行一致（486–488 注释 `retry exactly once. Never persists or logs the JWT / refresh token`） | 命中 |
| 43 | `trae-usage.js:45`–`56`/`60`/`63`–`81`/`83`–`92`/`94`/`98`–`105` | Gemini 两处修复、total 必需、候选唯一解、`unpriced_input_tokens`、精度、别名改写 | 逐行一致（81 `if (candidates.size !== 1) return null;`；103 `if (model === "claude-4-sonnet") return "claude-sonnet-4-20250514";`） | 命中 |
| 44 | `grok-usage.js:12`/`26`–`39`/`44`–`61`/`63`–`90`/`71`/`83` | ticks、上游成本门控、双形态、total 逻辑、`billable` | 逐行一致；**但 71 行「判空」只针对 total，不拦「分量全 0 + total>0」**（见 B.2） | 命中（语义有边界问题） |
| 45 | `devin-usage.js:11`–`13`/`21`–`23`/`25`–`26`/`38`–`58`/`48`–`51`/`76`–`87`/`113`–`119`/`121`–`126`/`149`–`156`/`197` | 投影纪律、互斥四列、表探测、NULL 语义、时间、账本、billable | 逐行一致（122 `… (n.created_at) is clone/replay insertion time and is never a fallback.`） | 命中 |
| 46 | `dumate.js:7`–`30`/`32`/`37`–`47`/`52`–`56`/`80`–`82` | 路径候选、入口、`qianfanpersonalquota` 过滤、最新副本、复用解析器 | 逐行一致（81 `...options, dbPath, dbMessages, source: "dumate", cursorKey: "dumate",`） | 命中 |
| 47 | `rollout.js:21898`/`21971`–`21983`/`21821`–`21833`/`21710`–`21713` | Antigravity `usageSource` 仅在游标、字符估算、估算写三桶、上下文增量语义 | 逐行一致（21982 `return cjk + Math.ceil(other / 4);`） | 命中 |
| 48 | `rollout.js:20214`/`19549`/`19551`/`19945`–`19961`/`20255`/`20268`/`19964`–`19968`/`20099`–`20123` | Grok 时间三兜底、0.8 硬拆、精度合流、v5 重建 | 逐行一致（20214 `… \|\| toUtcHalfHourStart(lastActive) \|\| toUtcHalfHourStart(Date.now());`） | 命中 |
| 49 | `rollout.js:6541`/`6515`/`6726`/`6727`/`6405`–`6411`/`4099`–`4115` | Qoder `credits` 唯一写入点、悬空注释、拒绝换算、累加器白名单 | 逐行一致（6726 注释后一行即 `touchedBuckets.add(bucketKey(...))`，上下文无 credits 累积语句） | 命中 |
| 50 | `rollout.js:3325`/`3341`/`3403`/`3422`/`6542`/`6648`/`6725`/`6731`/`6755`/`6760`/`20286`–`20287`/`22011`/`22025`/`22113`/`22157` | `usage_precision` 全部写入点（16 行 / 15 语句） | 全量 grep 命中的 16 行与清单**逐行一致**，且无 antigravity / kiro 行 | 命中（否定性证据成立） |
| 51 | `rollout.js:6639`–`6660` | Qoder 新 JSONL 显式构造全零 totals + `conversation_count: 1` | 6650–6659 逐字段 0、`conversation_count: 1` | 命中 |
| 52 | `rollout.js:22483`–`22492`/`22500`–`22508`/`22749`–`22751` | Trae CN 校验只要求 `isSafeInteger ≥ 0`、写全零合法、落桶无护栏 | 22484 `if (!Number.isSafeInteger(value) \|\| value < 0)`；22508 `conversation_count: 1`；22749–22751 无零值判断 | 命中 |
| 53 | `rollout.js:17199`–`17212` | Craft 四分量全 0 直接 `continue`（§5.6.2 反例） | 17199 条件 + 17211 `continue;` | 命中 |
| 54 | `rollout.js:6177`/`6540`/`6657`/`19959`/`21806`/`21831`/`22507`/`24212` | §5.6.4「显式写入 billable 的 8 家」 | 8 处全部命中（另 grep 证实 `13046` 为 lmstudio/unsloth 共用） | 命中 |
| 55 | `rollout.js:15914`/`17586`/`21807`/`23364`/`24214`/`7232`/`22115`–`22118`/`22506`/`18886`/`14340`–`14346`/`15379`/`17243` | §5.6.5 `conversation_count` 语义表 | `22506` 是 `total_tokens`，`conversation_count: 1` 在 **22508**；其余全部命中 | 1 条轻微偏移 |
| 56 | `rollout.js:12405`（§5.6.5 opencode 磁盘支路） | OpenCode 磁盘支路「产生差分的消息条数」 | **12405 属 Cline**（`conversation_count: previous ? 0 : 1`，同段 12396 `getHourlyBucket(…, "cline", …)`）；OpenCode 磁盘支路对应点在 **3120** | **错例** |
| 57 | `sync.js:685`/`676`–`684`/`694`–`703`/`1137`–`1145`/`1148` | Claude 家目录 UNION、理由注释、附加根、跨根去重、claude-mem 重纳入 | 逐行一致（678 `Claude UNIONS every allowed install instead of taking…`） | 命中 |
| 58 | `rollout.js:5013`–`5023`/`4600`–`4607`/`2803`/`1132`/`1200` | `walkClaudeProjects`、slug 不解析注释、子代理路径判定、`projectKnownAbsent`、cwd 缓存 | 逐行一致（4607 `const CLAUDE_CWD_SCAN_MAX_BYTES = 65536;`） | 命中 |
| 59 | `claude-user-identity.js:5`/`31`/`44` + `rollout.js:921`–`933`/`997`–`1003`/`1234`/`987`–`1015`/`992`–`994`/`952` | fork 折叠链、子代理排除、user 行采信、`claudeUserIdentity` 位置、历史哈希折叠、别名持久化、预扫描、下界保护 | 前四项命中（993 `Preserve all token totals and history whose source files were removed.`）；**`claudeUserIdentity` 定义在 `claude-user-identity.js:13`，rollout.js 921–933 是 `activeCodexRolloutDates` 尾 + `formatLocalDate` + 身份哈希注释**；**下界保护实际在 `rollout.js:1011`（`bucket.totals.conversation_count < count - 1`），952 是该文件哈希函数的右括号** | 2 条错例 |
| 60 | `session-analytics.js:2660`/`101`/`621`–`624`/`1968`–`1971`/`822`–`834` + `claude-config.js:7`–`8`/`10`/`107`–`109`/`252`–`256`（文件 279 行） | `sidechain` 全库唯一命中、`PLACEHOLDER_MODELS` 逐字、注释原文、观察者转录、model_provider 防护、hook 配置职责 | 逐行一致（`grep sidechain` 在 `src/` 下仅 2660 一处） | 命中 |
| 61 | `rollout.js:4173`/`rollout.js:2867`/`2872`/`2858`–`2859` + `claude-categorizer.js:1175` + `test/sync-droid-dup-session-repair.test.js`、`test/trae-entitlement.test.js:130`–`137` | 半小时桶两份实现、无测试锁定等价 | 4173 与 1175 两份实现均存在；`test/` 下引用 `toUtcHalfHourStart` 的仅这两个测试文件且都 `require("../src/lib/rollout")` | 命中 |
| 62 | `rollout.js:2291`/`2682`/`2793`/`2264`/`2294`/`2652`/`2666`/`2764`–`2765`/`2776`/`2889`/`1194`/`1949`/`2843`–`2847`/`488`–`504` | §6.1.9 三处 `endOffset = st.size`、唯一落 token 游标、对照保护 | `grep 'endOffset\s*=\s*st\.size'` 恰好 3 处（2291/2682/2793）；其余逐行一致 | 命中 |
| 63 | `claude-categorizer.js:2`–`13`/`33`–`41`/`367`/`378`–`380`/`69`/`70`–`71`/`1193`/`1214`/`1246`/`1267`/`1277` | 职责一/职责二、七分类键、字符比例、首个 cache_creation、真值修复字段名与身份折叠 | 逐行一致（6–7 `Computes on demand — no queue schema changes, no parser changes, no sync changes.`；1267 只有 `obj?.message?.usage`） | 命中 |
| 64 | `sync.js:1183`–`1190`/`248`/`1149`–`1182`/`1163`–`1166`/`6858`–`6861`/`6871`–`6921`/`6930`–`6943` | 真值修复触发、迁移键、缺根保护、原子重写、清桶 | 逐行一致（248 `"claudeGroundTruthRepair_2026_05_v4"`） | 命中 |
| 65 | `rollout.js:4838`/`2427`/`2431`–`2437`/`2438`–`2440`/`2441`–`2446`/`2447`–`2464`/`2454`/`2490`–`2492`/`2530`/`2547`/`2550`–`2554` | Codex 事件清单与提交位兜底 | 逐行一致（2454 `if (invalidRecordPolicy === "throw" \|\| !record.terminated)`） | 命中 |
| 66 | `codex-usage-record.js:10`/`19`–`21`/`31`–`32`/`60`/`82`–`86`/`150`/`174`–`177` | compaction 侧信道、`response_id` 去重、`turn_token_usage` 不读、上限 20000、turn 结束事件、匹配条件、recordOnly 告警 | 逐行一致（85 `return type === "task_complete" \|\| type === "turn_aborted";`） | 命中 |
| 67 | `rollout.js:4864`–`4865`/`4873`–`4876`/`4877`/`4881`–`4893`/`4851`–`4879` | Codex `input -= cached`、保留 total 的注释、重置哨兵 | 逐行一致（4877 `out.input_tokens = Math.max(0, out.input_tokens - out.cached_input_tokens);`） | 命中 |
| 68 | `pricing/index.js:248`–`256`/`254`/`297`–`299`/`325`–`327` | reasoning 子集白名单与三处门控 | 全量 grep：`reasoningIncludedInOutput` 出现在 248/254/297/325，`reasoningCost` 用在 262；三处门控齐全、无第四处 | 命中 |
| 69 | `codex-model-attribution.js:3`/`11`–`15`/`18`–`20`/`19`–`23`/`33`–`66`/`82`–`92`/`96`–`102` | 模型唯一权威、排除 `thread_settings_applied`、语料实测、`model_provider` 定位、信封枚举、turn_context/session_meta | 逐行一致（18–19 `0 of 10310 session_meta rows across 5849 / local rollouts carry the field (codex-cli 0.151.0)`） | 命中 |
| 70 | `codex-service-tier.js:17`（§6.2.9 代码块头） | `CODEX_SERVICE_TIER_MARKER` / `PRIORITY_SERVICE_TIERS` | **17 = `"use strict";`**；`CODEX_SERVICE_TIER_MARKER` 在 **20**、`PRIORITY_SERVICE_TIERS` 在 **26** | **错例** |
| 71 | `rollout.js:4073`–`4088`/`4067`–`4072`/`2608`–`2618`/`2626`/`302`–`380`/`428`–`461`/`2556`–`2603`/`4210`/`4212`/`2567`–`2569`/`639`/`2320`–`2322`/`2476`–`2477` | priority 子集列、rescan inflation、血缘去重、fork 守卫、状态持久化 | 逐行一致（2626 ``const dedupKey = `${sessionId \|\| filePath}:${tokenTimestamp}`;``） | 命中 |
| 72 | `test/codex-token-usage.test.js:24`–`67`/`test/codex-union-cursor-divergence.test.js:13`/`76`–`85`/`104`–`112`/`118`/`155`/`test/rollout-parser.test.js:104`–`158`/`test/claude-reasoning-usage.test.js:44`/`45`–`47`/`test/claude-fork-conversations.test.js:37`–`95` | 单测与 e2e 断言 | 逐行一致（47 行 `< 1e-12`；104–107 行注释逐字；76–78 `usageOf`） | 命中 |

**命中率**：64/72 = **88.9%**。含错引的抽样行共 8 个（#23、#26、#30、#35、#55、#56、#59 两处、#70），合计 9 处错。

---

## B. 幻觉与越界

### B.1 §5.1 总表逐列抽查（16 行 × 7 列）

抽查口径：每行核对「显示名 / `source` 字面量 / 读取方式 / 数据路径 / hook / 联网 / 字段映射」七列，`source` 以 `getHourlyBucket(...)`、`bucketKey(...)` 的第一参数或专用常量（`LMSTUDIO_SOURCE` 等）为准，不以函数名推断。

| 行 | 显示名 | `source` | 读取方式 | 路径 | hook | 联网 | 字段映射 | 判定 |
|---|---|---|---|---|---|---|---|---|
| 1 | Gemini CLI | `gemini`（`sync.js:1272`） | 整份 JSON | `tmp` 由调用侧拼（`sync.js:1235`） | `gemini-config.js` 有 `SessionEnd` hook 逻辑 | 无网络调用 | `4743` 取 max | 一致（路径与映射引号内的 `rollout.js:226` 未逐字核，但目录约定与 `listGeminiSessionFiles` 行为一致） |
| 2 | OpenCode（磁盘） | `opencode`（`sync.js:1440`） | 单消息 JSON | `<storage>/message/**/msg_*.json`（5025/5033/243） | 无 | 无 | 4755 且 input 不减缓存 | 一致 |
| 3 | OpenClaw | `openclaw`（`sync.js:1110`） | JSONL + agent SQLite（1638 库候选） | `agents/<a>/sessions/*.jsonl`（1612/1622） | `openclaw-hook.js` + `openclaw-session-plugin.js` 存在 | 无 | 2094–2105 input 先减 cacheRead | 一致 |
| 4 | Kiro（IDE） | `kiro`（7567） | SQLite + JSONL 兼容支路（7298–7304） | `…/kiro.kiroagent/dev_data/devdata.sqlite`（7282） | 无 | 无 | 7545/7547（缓存/推理恒 0 在 7558 起的 delta） | 一致 |
| 5 | Hermes | `hermes`（8018） | SQLite | `state.db` + `profiles/*/state.db`（7659/7676） | 无 | 无 | 7718 SQL 列名即桶名 | 一致 |
| 6 | Kiro CLI | `kiro`（9191/9210） | SQLite 估算 + legacy `.json` 真实值 + v2 估算 | `kiro-cli/data.sqlite3`（8132/8138/8140） | 无 | 无 | 8371–8387 + 8118 | 一致 |
| 7 | Kimi | `kimi`（9611） | `wire.jsonl` | 见 A#23（**路径引号错**） | 无（8078 注释为证） | 无 | 9586–9590 | 一致（除路径引号） |
| 8 | Kimi Code | `kimi`（9844） | `wire.jsonl` 递归 | `~/.kimi-code/sessions/**`（9677 仅 `wslProviderDir` 一处） | 无 | 无 | 9802–9817 三形态 | 一致 |
| 9 | CodeBuddy | `codebuddy`（10384） | JSONL + IDE 日志双源 | `~/.codebuddy/projects`（10035）+ 平台分叉 10069 | `passive-mode.js:156` + `init.js:577` | 无 | 10337–10345 / 10476–10485 | 一致 |
| 10 | WorkBuddy | `workbuddy` / `workbuddy-ai`（10651/11004） | JSONL > traces > SQLite | `~/.workbuddy{,-ai}/projects` | `passive-mode.js:166`/`177` + `init.js:581`/`585` | 无 | 10955–10966（**`911` 引号错**） | 一致（除引号） |
| 11 | Roo Code | `roocode`（11939） | `ui_messages.json` | 复用 Kilo 根（11767） | 无 | 无 | 11906 | 一致 |
| 12 | Cline | `cline`（12396） | 会话转录 JSON | `~/.cline/data/sessions`（12069–12073 WSL） | 无 | 无 | 12356–12358 | 一致 |
| 13 | Zed | `zed`（12782） | SQLite BLOB（`json`/`zstd`） | `threads/threads.db`（12490/12501） | 无 | 无 | 12553–12597 | 一致 |
| 14 | LM Studio | `lmstudio`（12852） | 文本日志内嵌 JSON | `~/.lmstudio/server-logs`（12871） | 无 | 无 | 13039–13049 | 一致 |
| 15 | Unsloth | `unsloth`（13467） | SQLite | `<studioHome>/studio.db`（13482） | 无 | 无 | 13634–13636 + 13667 | 一致 |
| 16 | Kilo Code | `kilo-code`（15651） | `ui_messages.json` | `kilocode.kilo-code/tasks`（11706） | 无 | 无 | 15624–15628 | 一致 |

另抽查第 21/22（Qoder 双通道 `qoder`/`qoder-cn`，6412–6414）、第 34–36（Copilot 三通道同 `copilot`，17375/18122/19213）、第 37（Grok `grok`，19553/19566）、第 43（Codex 家族三 `source`，281/494/521/3227）、第 45（Claude Science `claude-science`，6877–6883/7101）五组，均与源码一致。

**结论**：总表 16+5 行的七列抽查**未发现编造**，两处问题都是引号内的行号（#23、#26）；`source` 字面量、路径、hook 存在性、联网属性全部可回源。

### B.2 否定性断言的独立重建

**B2-1 §5.6.1「A. 可从源码推出 $0 退化路径（唯一一家）：gemini」——不成立，至少 3 家。**

- 文档的判据（自述）：`total = max(上报值, 分桶和)` + 前置闸门把 `total_tokens` 算作非零 → 只写 total 的行产出「分量全 0、total>0」。
- Gemini 成立（`4743` + `isAllZeroUsage` 的键表含 `total_tokens`（`4947-4953`）+ `diffGeminiTotals` 的 `total_tokens` 为差分（`4809`））。
- **grok 同形**：`grok-usage.js:63-70` 先取上报 `totalTokens`，仅当 `≤0` 才回退求和（`71` 只拦 total）；消费侧 `rollout.js:20212-20228` 把 `event.total_tokens` 直接写进 delta，**没有任何「分量至少一个 >0」的闸门**：

  ```js
  // src/lib/rollout.js:20217-20228（grok turn_completed 落桶）
  const delta = { input_tokens: event.input_tokens, …, total_tokens: event.total_tokens,
                  billable_total_tokens: event.billable_total_tokens,
                  total_cost_usd: event.total_cost_usd, conversation_count: … };
  pendingBucketDeltas.push({ model: eventModel, hourStartStr, delta, usagePrecision: "reported" });
  ```

  即 `usage = {totalTokens: N}`（分量字段缺失）→ `input/output/cache/reasoning` 全 0、`total = N` → 与 Gemini 完全同形的 $0 行。
- **codex 同形**：`normalizeUsage`（`4851-4879`）只对 `input` 做缓存减法、**total 原样保留**，注释自述保留原因包含「`some component fields are absent`」（`4873-4876`）；唯一前置拒绝是 `isCodexTotalOnlyResetSentinel`（`4881-4893`），它只拒「last 有 total 而累计 total 归零」这一特定 reset 形态，**不拒「分量全 0、total>0」的普通增量**；`consumeUsageDelta`（`codex-token-usage.js:98-158`）的「正常增长」分支直接把 `last` 当增量返回（`121-138`）。→ `codex|<model>|<hour>` 亦可产出同形 $0 行。
- 因此正确表述应为：**A 类 3 家（gemini / grok / codex）**；若坚持「唯一」，唯一能站住的限定是「**文本/JSONL 类 19 家的自有 `total_tokens` 赋值点内唯一**」——这正是文档「唯一性依据」那句话的实际范围，但表头与 §5.6.7 第 2 条把它写成了全局唯一。

**B2-2 §5.6.2「全零 token 但仍落桶：两处，且只有两处」——不成立，至少 5 处（外加写入端撤回行）。**

文档只列 `qoder`（新 JSONL）与 `trae-cn`。重建后另有：

1. **qoder 旧库**：`normalizeQoderTokens`（`6151-6177`）的「判空」只是数值合法性（`6154-6163` 检查 finite/非负），**不拒绝 0**；`currentTotals` 带 `conversation_count = requestOwners.get(requestKey) === messageKey ? 1 : 0`（`6281-6286`），随后无条件 `addTotals(bucket.totals, currentTotals)`（`6338-6340`）。→ `token_info` 全 0 的行会写出「六列全 0 + `billable=0` + `conversation_count: 1`」的队列行。
2. **Claude 用户行桶**：`rollout.js:2828-2835` 只对桶做 `conversation_count += 1` 并把键放进 `touchedBuckets`：

   ```js
   // src/lib/rollout.js:2830-2834
   if (userBucketStart) {
     const userModel = DEFAULT_MODEL;
     const userBucket = getHourlyBucket(hourlyState, source, userModel, userBucketStart);
     userBucket.totals.conversation_count += 1;
     touchedBuckets.add(bucketKey(source, userModel, userBucketStart));
   }
   ```

   `pickDominantModel`（`3542-3556`）**跳过 `DEFAULT_MODEL`**，故「只有 unknown 桶」的分组不会走合并分支；`enqueueTouchedBuckets` 对该桶按 `totalsKey`（`4155-4166`，含 `conversation_count`）比较后照发（`3403-3408`）→ 队列出现 `claude|unknown|<hour>` 且 `total_tokens: 0`、`conversation_count ≥ 1` 的行。这不是边角情形：任何「用户轮次所在半小时内该分组无 token 行」都会命中。
3. **reasonix**：`17067-17068` 的触发条件是 `(tokenDelta > 0 || delta.requests > 0)`；`addReasonixDelta`（`17035-17044`）在不校验分量时写 `total_tokens = delta.input + …`（可为 0）与 `conversation_count: delta.requests` → 「token 全 0、会话数 >0」的行。
4. 另外，§4.6 自述的写入端撤回机制本身就在写全零行（`3291-3316`、`3376-3401`），与 §5.6.2 的「只有两处」互相否定（见 C8）。

**B2-3 §5.6.1「数据库 / API 子集里没有任何一家会『只填 total 而不填 input/output』」——不成立。**

- 反例一：`grok-usage.js` 的 normalizer 在 `totalTokens > 0` 且分量缺失时返回 `input_tokens: 0, output_tokens: 0`（`63-89`），正是「只填 total」的形态。
- 反例二（引号错）：`qoder` 旧库被判空条件引到 `6154`，该行是 finite/非负检查，不是零值闸门；`normalizeQoderTokens` 可以返回全 0 对象。
- 同句列举里 `opencode(4755)` 引的是 normalizer 而非闸门（真正的门在 `3109-3110`、`5222` 区间），`craft(17199)`、`dsh(23354)`、`command-code(24204)` 三处则确实成立。

**B2-4 §5.2.20 注与 §5.6.7 第 6 条「时间漂移到触碰时刻的有 5 家（zed/lmstudio/goose/droid/grok）」——不完整，至少 7 家。**

- `craft`：`17223 if (tsMs == null) tsMs = stat.mtimeMs;`（文档 §5.3.6 自己写了这条）。
- `reasonix`：`reasonixTimestamp` 末级 `return snapshot.stat.mtimeMs;`（`17021`，§5.3.6 也写了）。
- 另有 §5.3.6 自认 `copilot` App DB「一条会话的全部历史 token 会落在『本次观测到的那个半小时桶』里（**与 Grok 的估算路径同类风险**）」——口径上属同一破坏面。

**B2-5 §5.6.4「8 显式 + 13 兜底 = 21」——重建通过。** 全量 grep `billable_total_tokens` 在 `rollout.js` 的**解析器级赋值**只有：6177、6540/6646/6657、7025（claude-science，属 §6 实体）、13046（lmstudio/unsloth 共用）、19959、20224、21806/21831、22507、24212、24337；与「8 家显式」清单一一对应，其余家靠 `4110` 兜底。文档「文本/JSONL 子集只有 lmstudio 与 unsloth 显式写入」也成立。

**B2-6 §5.5.6 的 `usage_precision` 写入点清单（16 行 / 15 语句）——重建通过。** `grep usage_precision src/lib/rollout.js` 的 16 行与清单逐行相同（含 `3325`/`3341`/`3403`/`3422`/`20286`–`20287`/`22011`/`22157`），**清单里确实没有 antigravity 与 kiro**；`session-analytics.js`(5)/`trae-usage.js`(1)/`dashboard/src/lib/sessions-api.ts`(1) 的行数也准确。§5.5.4 的披露缺口结论成立（但见 B.3 的边界提醒：`antigravity` 在游标里写了 `usageSource`）。

### B.3 证据等级（是否被升级）

- **未升级**：`trae-cn` 全零行频率（§5.6.2）、`copilot` 三通道互斥完备（§5.6.6）、`model/rerouted`（§6.2.7）、`<synthetic>` 非零 usage（§6.1.6）、service tier 覆盖数字（§6.2.9）、`groupQueued` 死代码（§4.4）均带 `【未能确认】` 且附判定条件 —— 处理规范。
- **需要收紧一处**：§5.6.1 A 类用的是「**可从源码推出**」——把「若日志只写 total，则为 $0」这一**条件形态**写成了确定性归类，并且没有标注为条件性风险；同一句话的兄弟条目（B/C/D/E）都用「可产出/规避/不产」这类条件词，唯独 A 类把 `【未能确认】`（真实日志是否只写 total）省略。建议补一句与 §5.6.2 同规格的 `【未能确认】`。
- **需要收紧一处**：§5.5.4「下游无法区分 antigravity 的 SQLite 精确行与字符估算行」是**基于写入点清单的否定性证据**，成立；但报告应显式写出该证据的边界（只在「队列行」这一层成立，游标层 `cursors.files[].usageSource` 是有标记的）——§5.5.4 已写「只在游标里记」，边界表述正确，无需改。

### B.4 §6 推理与缓存语义回源（逐条）

| 断言 | 回源结果 |
|---|---|
| Claude `output_tokens = 原始 − reasoning`、`reasoning_output_tokens = min(output, thinking_tokens ?? reasoning_tokens)`、`total_tokens` 用未减 output | **成立**：`rollout.js:4924-4937`；补足式断言在 `test/claude-reasoning-usage.test.js:44`，费用不变在 `:45-47`（`< 1e-12`） |
| Claude 缓存字段只认 `cache_read_input_tokens` / `cache_creation_input_tokens`，别名只在 Codex 路径 | **成立**：`4927-4928`；`codex-token-usage.js:25-27` 的 `?? value.cache_write_input_tokens` |
| Codex reasoning 是 output 的**子集标记**（解析不改写、计价置 0） | **成立**：`4851-4879` 无 `output -= reasoning`；`pricing/index.js:248-256` 白名单 + 三处门控（`254`/`297`/`325`，全量 grep 确认无第四处） |
| Codex 侧 `input -= cached`，六桶只做 `Math.floor`/非负吸收 | **成立**：`4861-4862`、`4877`；6–7 倍膨胀注释 `4864-4872` 逐字 |
| 「喂给 `computeRowCost` 的是五列之和，`total_tokens` 永不参与计费」 | **成立**：`pricing/index.js:257-262` 基础费只含五列；Claude 侧同规则由测试 `45-47` 锁定 |
| 零值重置哨兵只在 `source === "codex"` 启用 | **成立**：`2550-2553`（`source === DEFAULT_SOURCE`） |
| 家族白名单漏加成员会**静默双计** | **成立**：门控按字面量硬编码，`pricing/index.js:248-253`，无结构性判定 |
| `model/rerouted` 8 种信封为前瞻、真实语料 0 命中 | **成立**：`codex-model-attribution.js:8-9`（`forward-looking`）、自述命中 0 |

---

## C. 内部一致性

| # | 对照点 | §4 口径 | §5/§6 口径 | 实测 | 结论 |
|---|---|---|---|---|---|
| C1 | 桶键与哨兵常量 | §4.4：`55-57`、`4222-4231` | §5.2.0：`1008`（`4222`–`4226`、`4254`） | 一致 | 一致 |
| C2 | 队列行字段集 | §4.5：`3328-3345` | §5.2.0：`1011`（`3328`） | 一致 | 一致 |
| C3 | 累加器与 billable 兜底 | §4.4：`4053-4065`、`4099` | §5.2.0：`1009`；§5.6.4：`4110` | 一致 | 一致 |
| C4 | 计费输入 | §4.5：`772`「五列之和，total 不参与」 | §6.2.6、§6.3 同表述 | 一致 | 一致 |
| C5 | `conversation_count` 语义 | §4.5：`778` 两条主线 | §6.2.5/§6.3 同表述 | 一致 | **§5.6.5 的「7 种语义」表未纳入 §6 的 Claude 用户轮次（`2833`）与 Codex 事件数（`2554`）**，与 §6 未对齐（「至少」措辞可保，但表格应补两行或加范围说明） |
| C6 | `usage_precision` | §4.5：`776`（参与 `queuedKey`，`3325-3326`） | §5.5.6：16 行清单（含 `3325`/`3403`/`3422`） | 全量 grep 一致 | 一致 |
| C7 | 半小时桶两份实现 | §4.3：`4173` + `claude-categorizer.js:1175`，无测试锁定 | §6.1.7 同表述 | 一致（test 引用仅 2 处） | 一致 |
| C8 | 零值行 | §4.6：`827`/`837` 撤回零行、`838` 对齐零行 | §5.6.2：「全零 token 落桶只有两处」 | **互相否定**（§4.6 自己就描述了写入端两处零值行；§5.6.2 未加「解析器构造」限定） | **矛盾，必须修** |
| C9 | `endOffset = st.size` 三处 | §4.2：`651-657` | §6.1.9：`1802-1810` | 3 处（`2291`/`2682`/`2793`） | 一致 |
| C10 | Codex 家族 | — | §5.1 第 43 行、§5.1.1、§6.2.10（`4864`、`4873`–`4876`、pricing 白名单、`4906-4912`） | 一致 | 一致 |
| C11 | Claude Science 定位 | — | §5.1 第 45 行 `6877`–`6883` vs §6.1.1 `:6875`、`:6882` | 两者分别指「路径解析块」与「文件名常量/家目录」，均正确 | 一致（引用粒度不同，不必改） |
| C12 | 冷文件跳过定位 | §4.2：`filterColdCodexRolloutFiles()` `rollout.js:708` | §6.2.1：同一函数写 `775-857` | 函数起于 708、正文 775–869 | 建议统一为 `708`–`869`（P2） |

---

## 必须修清单（按严重度排序）

**P0 — 结论性错误，会直接误导下游**

1. **§5.6.1 A 类唯一性**：`gemini` 不是唯一。至少 `grok`（`grok-usage.js:63-71` + `rollout.js:20212-20228`）与 `codex`（`rollout.js:4873-4876` + `4851-4879` + `4945-4958`）同形。改法：A 类列 3 家，并把「唯一性依据」明确限定为「19 家文本/JSONL 类的自定义 `total_tokens` 赋值点内唯一」。
2. **§5.6.2「只有两处」**：补 `qoder` 旧库（`6151-6177` + `6281-6286` + `6338-6340`）、`claude` 用户行桶（`2828-2835` + `3542-3556` + `3403-3408`）、`reasonix`（`17067-17068` + `17035-17044`）；并显式区分「解析器构造的零行」与「写入端撤回/对齐零行」（§4.6 的 `3291-3316`、`3376-3401`）。
3. **§5.6.1 数据库/API 子集判空结论**：「没有任何一家只填 total」不成立（grok normalizer 反例）；同时把 `qoder` 旧库的判空引号从 `6154`（数值合法性）改到能真正拦住 0 的位置（实际上没有，应写明「无零值闸门」）。
4. **§5.6.7 第 2 条**：「Gemini 是唯一可产 $0 行的来源」与同节 §5.6.2 自相矛盾，需与 1、2 同步改写。

**P1 — 计数/清单不完整或行号硬错**

5. **§5.2.20 注 + §5.6.7 第 6 条**：「5 家」→ 至少 7 家（补 `craft` `17223`、`reasonix` `17021`），并说明 `copilot` App DB 属同类风险（§5.3.6 自述）。
6. **§5.6.5 表**：`opencode`（磁盘支路）的行号 `12405` 实为 **cline**，应为 `3120`。
7. **§6.1.4**：`claudeUserIdentity` 的定义位置应为 `src/lib/claude-user-identity.js:13`（`rollout.js:921-933` 无关）。
8. **§6.1.4**：fork 净值修正的下界保护行号 `:952` → `:1011`。
9. **§6.2.9 代码块头**：`src/lib/codex-service-tier.js:17` → `:20`（marker）与 `:26`（tier 集合）。
10. **§5.2.10**：WorkBuddy input 减法引号 `911` → `10963`。
11. **§5.1 读表说明**：「两者有 6 处不一致」不可复现（§5.1.1 重叠表 8 行；按显示名/字面量口径 ≥12 行）——需给出计数定义或换成重叠条目数。

**P2 — 轻微偏移与口径统一**

12. §5.2.7 路径引号 `9501` → `9507`/`9510`。
13. §5.2.12 导入闸门 `12350` → `12352`。
14. §5.6.3 AnythingLLM reasoning 引号 `14040` → `14042`。
15. §5.6.5 trae-cn 引号 `22506` → `22508`。
16. §6.2.1 冷文件跳过引号 `775-857` → 与 §4.2 统一为 `708`–`869`。
17. §5.6.5 的「7 种语义」表补 §6 的两条（Claude 用户轮次、Codex 事件数），或在表头加范围限定。

---

## 复核到的行号抽样

（本节为原始记录：每条给出「文档引用 → 实测行内容前若干字」，供主文档团队二次核对；顺序与 A 节表编号一致，仅列 9 条非命中与 12 条高风险命中的逐字摘录。）

**非命中（9 条）**

```
#23  src/lib/rollout.js:9501
      9501:       wslProviderDir: ".kimi",
      → §5.2.7 想要的是 9507 resolveKimiWireFiles / 9510 path.join(kimiHome, "sessions")

#26  src/lib/rollout.js:911
      911:       typeof entry === "string"
      → §5.2.10 想要的是 10963 const inputTokens = Math.max(0, promptTokens - cacheRead - cacheCreation);

#30  src/lib/rollout.js:12350
      12350:         const ts = Number(msg.ts);
      → 闸门语句在 12352: if (importedAtMs !== null && ts <= importedAtMs) continue;

#35  src/lib/rollout.js:14040
      14040:     const model = normalizeModelInput(row?.model) || "anythingllm-unknown";
      → 差额公式在 14042: const reasoningTokens = Math.max(0, reportedTotal - accounted);

#55  src/lib/rollout.js:22506
      22506:       total_tokens: total,
      → conversation_count: 1 在 22508

#56  src/lib/rollout.js:12405
      12396:         const bucket = getHourlyBucket(hourlyState, "cline", model, bucketStart);
      12405:           conversation_count: previous ? 0 : 1,
      → OpenCode 磁盘支路对应点在 3120: delta.conversation_count = 1;

#59a src/lib/rollout.js:921-933（§6.1.4 指 claudeUserIdentity）
      923: function formatLocalDate(value) {
      930-935: // Identity hash for cross-environment duplicate session files …
      → claudeUserIdentity 定义在 src/lib/claude-user-identity.js:13（rollout.js:1 处才 require）

#59b src/lib/rollout.js:952（§6.1.4 指下界保护）
      948:   } catch (_e) {
      952: }
      → 守卫在 1011: if (!bucket || bucket.totals.conversation_count < count - 1) continue;

#70  src/lib/codex-service-tier.js:17
      17: "use strict";
      → 20: const CODEX_SERVICE_TIER_MARKER = '"thread_settings_applied"';
      → 26: const PRIORITY_SERVICE_TIERS = new Set(["priority", "fast"]);
```

**高风险命中（12 条，用于支撑 B.2 的重建）**

```
B2-1 gemini  $0 路径成立
      4809:     total_tokens: Math.max(0, (current.total_tokens || 0) - (previous.total_tokens || 0)),
      4947-4953: ["input_tokens","cached_input_tokens","cache_creation_input_tokens",
                  "output_tokens","reasoning_output_tokens","total_tokens"]  ← total 参与非零判定
B2-1 grok   同形（total 优先 + 无分量闸门）
      grok-usage.js:63-71:  let totalTokens = nonNegative(usage.totalTokens ?? usage.total_tokens);
                            if (totalTokens <= 0) { …求和… }  if (totalTokens <= 0) return null;
      rollout.js:20217-20228: delta = { …event.input_tokens…, total_tokens: event.total_tokens } → 直接落桶
B2-1 codex  同形（保留上报 total + 只拦 reset 哨兵）
      4873-4876: // Preserve the reported total for compatibility with older Codex / Every
                 // Code shapes where output_tokens can exclude reasoning or some component
                 // fields are absent.
      4889-4892: lastUsage.total > 0 && totalUsage.total === 0 && componentTotal(...) === 0
B2-2 qoder 旧库 无零闸门
      6154-6163: 仅 Number.isFinite / >= 0 检查
      6281:     const conversationCount = requestOwners.get(requestKey) === messageKey ? 1 : 0;
      6338-6340: getHourlyBucket(...) + addTotals(bucket.totals, currentTotals) + touchedBuckets.add(...)
B2-2 claude 用户行桶
      2833-2834: userBucket.totals.conversation_count += 1;
                 touchedBuckets.add(bucketKey(source, userModel, userBucketStart));
      3546:     if (model === DEFAULT_MODEL) continue;      ← pickDominantModel 跳过 unknown
      3403-3408: const key = …totalsKey(unknownBucket.totals)…; if (unknownBucket.queuedKey === outputKey) continue;
      4165:     totals.conversation_count || 0,               ← 指纹含会话数，故零 token 行也会写出
B2-2 reasonix
      17066-17068: const tokenDelta = …; if ((tokenDelta > 0 || delta.requests > 0) && addReasonixDelta(...))
      17035/17043-17044: const total = delta.input + …; total_tokens: total; conversation_count: delta.requests
B2-3 grok normalizer 可产 input=0/output=0
      76-82: return { input_tokens: inputTokens, …, total_tokens: totalTokens, … }
B2-4 craft / reasonix mtime 兜底
      17223:     if (tsMs == null) tsMs = stat.mtimeMs;
      17021:   return snapshot.stat.mtimeMs;
B2-5 billable 显式写入点全量
      6177 / 6540 / 6657 / 19959 / 21806 / 21831 / 22507 / 24212（+7025 claude-science、+13046 lmstudio-unsloth）
B2-6 usage_precision 全量
      3325 3341 3403 3422 6542 6648 6725 6731 6755 6760 20286 20287 22011 22025 22113 22157（16 行，与清单逐行相同）
C9   endOffset 三处
      2291 / 2682 / 2793（grep 'endOffset\s*=\s*st\.size' 恰好 3 命中）
B.4  pricing 三处门控
      248-253（白名单）/ 262（reasoningCost 唯一使用）/ 297 / 325
```

**证据等级与其它自查项（命中）**：`codex-model-attribution.js:18-20`（`0 of 10310 … 5849 local rollouts`）、`codex-service-tier.js:8-9`（`1213-file … 167 … 25%`）与 `:22`（`default (814) / priority (58)`）均逐字存在，且主文档对这三处均按「维护者本机语料」处理，等级未被升级。

**复核范围声明**：本次未修改样本仓（`git status --porcelain` 空）与主文档。§5.3.2/§5.4/§5.3.6 中 `install-resolver.js:52`–`79`、`trae-snapshot.js`/`trae-sqlite.js`/`trae-db.js` 的少数行号未逐条展开（不在此抽样表内，不计入命中统计）；§5.1 第 1 行「目录约定 `rollout.js:226`」一处亦未逐字核，仅按其行为一致性判定。
