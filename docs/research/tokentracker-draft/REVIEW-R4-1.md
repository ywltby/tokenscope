# 验收报告 R4-1：主文档 元信息 + §1–§6

**复核对象**：`docs/research/2026-10-14-tokentracker-usage-statistics.md` 第 1–2148 行（元信息头部 +「本文是怎么做出来的」+ `## 1.`–`## 6.` 全文，`## 7.` 标题之前）。
**样本**：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）。
**口径**：一律 `read` 工具 / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；全程只读，未改样本仓库与主文档（`git status --porcelain` 为空）。
**本轮职责**：证伪。① 验收第三轮修正单位的「采纳 24 条」是否真的改对；② 独立裁决其「反驳 3 条」；③ 做一轮全新抽样（避开前三轮已抽点）。

---

## 结论（是否可交付 / 是否仍有必须修的问题）

**可交付。第三轮修正是真实生效的，不是台账上的数字。**

- **修正点验收 29/29 全部通过**：R3-1 清单 12 条 + R3-2 清单 17 条逐条回源码核对——**24 条采纳项全部按源码改写正确**（无一例"说要改、实际没改"或"改过头"），**3 条反驳项经独立裁决全部成立（复核方错、修正方对）**，2 条为复核方自标的可选建议、未采纳不构成问题。修正单位 §13.6 自报的「采纳 24 / 反驳 3」与实际文本改动**逐项吻合**，台账可信。
- **三条反驳全部判修正方胜**，且每条都能用源码一行定案（`cursor-store` 之外的两条是复核方自己的引用错误，见 A2）。
- **四项高价值修正质量达标**：§5.6.1 A 类已列 **3 家**（gemini/grok/codex，含 grok 与 codex 的完整证据链）；§5.6.2 已列 **5 处**解析器构造点并显式区分写入端撤回零行；§5.6.7 第 2 条与 §5.6.2 已改为同一口径（"至少 3 家…另有至少 5 处"），矛盾消除；§5.1 的「8 处错位」自洽可回源。
- **全新抽样 50 条（约 80 个行号点）：命中 47 条（94.0%）／77 点（96.3%）**。3 条错例**全部是【低】级 1–3 行邻域偏移**（指向同函数/同返回块），不改变任何结论，但引用需校准。
- **未发现**：编造、证据等级被静默升级（抽查的 `【未能确认】` 全部保持原档位并带判定条件）、与源码方向相反的断言、以及把 R3 问题"表面上改写、实质保留"的情况。

**必须修：3 条，全部【低】（引用级，见末节清单）。无【高】/【中】级问题。**

---

## A. 修正点验收（逐条：原问题 / 现写法 / 判定 / 依据）

清单来源：`REVIEW-R3-1.md`「必须修清单」12 条、`REVIEW-R3-2.md`「必须修清单」17 条，共 29 条。判定一律以**当前主文档文本 + 样本源码**双端对照。

### A-1 ｜R3-1 清单（§1–§4）

| # | 原问题（R3-1） | 现写法（文档行） | 判定 | 依据（实测摘录） |
|---|---|---|---|---|
| 1 | L439 损坏恢复链写成三级回退 | **L441**：读取侧先试 `manifest.current`，失败后才试 `manifest.rollback \|\| manifest.previous`——**这是两级回退而不是三级依次回退**…`previous` 只在 `rollback` 为空（首次落盘）或 manifest 属别人的旧代时才轮到 | ✅ | `cursor-store.js:124-128`：`await readGeneration({generationId: manifest.current}) \|\| await readGeneration({generationId: manifest.rollback \|\| manifest.previous})`——措辞与代码语义逐字对齐 |
| 2 | L291 45 家拆解 12/1/17/15 内部矛盾、分类与源码不符 | 保留 12/1/17/15，并补「口径先说清」段（写配置 / 仅探测 / 纯被动读取 / 仅欢迎名单四类的判定标准 + 逐类名单 + 行号区间） | ❌→**反驳** | 见 A2-1：**修正方对，复核方错** |
| 3 | L377 文件级去重漏掉 `!prev` 条件 | **L379**：**只有「该文件从无游标（`!prev`）且本运行内该 `fileId` 已被先到的文件认领」时**，才标 `duplicateOf` 并把 offset 直接推到 `st.size`（`:1073-1081`）；**已有游标的重复文件不走这条** | ✅ | `rollout.js:1073`：`} else if (!prev && primary !== filePath) {`；`:1078-1080` `cursors.files[key] = { inode, offset: st.size, …duplicateOf: primary }` |
| 4 | L465 第三态被写成函数契约（"必须"） | **L467**：第三态由 `describeScanRootState` 渲染成 `" (unreadable: <code>)"`、与 `" (missing)"` 区分——注意这是**渲染与意图**：`:89-96` 的注释把「应当区别于 missing」写成调用方意图…**函数本身只返回字符串、没有强制点** | ✅ | `scan-roots.js:250-253` 返回字符串；`:95-96` 注释 `so callers can say "unreadable" instead of "missing"` |
| 5 | L301 把 omp 的 inode 校验张冠李戴给 OpenClaw | **L303**：**OpenClaw 的保护不是哨兵串也不是 inode 校验**，而是 `openclaw.json` 的 `plugins.entries` 策略（`ensureOpenclawSessionPluginPolicy`，`openclaw-session-plugin.js:180` 起）；同文件 `:154-178` 是**状态探测**（五个布尔量） | ✅ | `:154-167` 计算 `enabled/linked/installed/conversationAccess`；`:169-177` 返回含 `pluginFilesReady` 的五布尔对象；`:180` 才是 `ensureOpenclawSessionPluginPolicy` |
| 6 | L369 建议补 13 项溯源（标注为可选；切勿误删） | 未加溯源句，13 项清单原样保留 | ✅（允许） | R3-1 原文即「**低**…可选…**不要**误删该项」；13 项与 `sync.js:3424-3437` 相符（R1/R2 已背书） |
| 7 | L692 "没有任何测试"表述过宽 | **L694**：…（指「两份实现语义等价」这一关系本身；主实现自己有定向测试——`test/trae-entitlement.test.js:29` 从 `../src/lib/rollout` 导入，`:130-137` 三条断言钉死 `:00`/`:30` 取整）…**`claude-categorizer.js` 的那一份副本无人测** | ✅ | `test/trae-entitlement.test.js:29` `toUtcHalfHourStart,`；`:130-137` 两条断言（`01:15→01:00`、`01:45→01:30`）逐字 |
| 8 | L87/L13/L865 GBK 措辞把「rollout 的 15」说成「样本源码的 15」 | **L13/L87/L867** 三处统一为「单文件最多 15 行；全 `src/` 累计受影响 52 行」，并补 `local-api` 6 / `sync` 5 / `session-analytics` 4 +「其余 16 个文件各差 1–3 行」 | ✅ | 本轮**独立复现**（cp936 整体解码 vs `ReadAllLines` UTF-8）：受影响 **20 个文件 / 吞行合计 52**；`rollout.js` 15、`local-api.js` 6、`sync.js` 5、`session-analytics.js` 4，其余 16 个各 1–3。`(Get-Content -Encoding Default).Count` = 24917 亦复现 |
| 9 | L730 把 `:399-409` 说成 `normalizeHourlyState` | **L732**：该 v<2 分支体在 `src/lib/rollout.js:3634-3652`；`:399-409` 是 `syncDiagnostics` 字段赋值区，与本函数无关 | ✅ | `:3627` `function normalizeHourlyState(raw) {`；`:3634` `if (!Number.isFinite(version) || version < 2) {`；`:3646-3651` `return { version: 3, … }` |
| 10 | L379 骨架 D 引法应统一为 16084 | 代码块与表格均为 `:16084` | ❌→**反驳** | 见 A2-2：**修正方对，复核方错** |
| 11 | L857 `setMaxTotal` 调用点不在 `sync.js:6540` | 保留 `:6540` | ❌→**反驳** | 见 A2-3：**修正方对，复核方错** |
| 12 | L32 R2 合计口径建议注明「自三份报告相加」 | 未加注明（`§13.2` 有三份报告的分项工作量表） | ✅（允许） | R3-1 原文为「【低】…建议注明」；上下文可自解，不构成错误 |

**R3-1 小结**：采纳 7 条**全部改对**；反驳 3 条全部成立；2 条可选建议未采纳（合规）。

### A-2 ｜R3-2 清单（§5–§6）

| # | 原问题（R3-2） | 现写法（文档行） | 判定 | 依据（实测摘录） |
|---|---|---|---|---|
| P0-1 | §5.6.1 A 类「唯一一家 gemini」被证伪 | **L1508-1510** 列 **3 家**：`gemini`、`grok`、`codex`（含 acode / every-code），并各自附消费侧闸门证据 | ✅ | `grok-usage.js:63-71`：`let totalTokens = nonNegative(usage.totalTokens ?? usage.total_tokens); if (totalTokens <= 0) { …求和 } if (totalTokens <= 0) return null;`；`rollout.js:20217-20228` 直接把 `event.total_tokens` 写进 delta（无分量闸门）；`4873-4876` 保留 total 的注释 + `isCodexTotalOnlyResetSentinel`（`4881-4893`）只拒 reset 形态 |
| P0-2 | §5.6.2「只有两处」被证伪 | **L1528-1536** 列 **5 处**：`qoder`(新)/`qoder`(旧库)/`trae-cn`/`claude`(用户轮次桶)/`reasonix`；**L1538** 另起一段把**写入端**撤回/对齐零行单列，明确「两个不同的集合」 | ✅ | 逐处命中：`6151-6177`+`6281`+`6338-6340`（旧库无零闸门）；`2828-2835`+`3546`+`3403-3408`（user 桶 + `pickDominantModel` 跳过 unknown）；`17067-17068`+`17035`+`17044`（reasonix）；`22483-22508`/`22749-22751`、`6639-6660`（R3-2 已核） |
| P0-3 | §5.6.1「数据库/API 子集没有任何一家只填 total」被证伪 | **L1521-1526** 改为「**确实存在**…不能一概说没有任何一家」，给 `grok` 与 `qoder`（旧库）两条反例，并写明 qoder 的 `6154` 是合法性判定而非零值闸门 | ✅ | `grok-usage.js:76-89` 返回块可产 `input=0/output=0/total>0`；`6154-6163` 仅 `Number.isFinite` + `>= 0` |
| P0-4 | §5.6.7 第 2 条与 §5.6.2 自相矛盾 | **L1612**：「**至少 3 家**可产 $0 行（§5.6.1 A 类…），另有**至少 5 处**「全零 token 仍落桶」的解析器构造点（§5.6.2…），外加写入端的撤回与对齐零行」 | ✅ | 与 L1508/L1528/L1538 三处口径一致，**矛盾已消除** |
| P1-5 | §5.2.20 注 + §5.6.7 第 6 条「5 家」→ 至少 7 家 | **L1240**「共 7 家」（本表 4 家 + grok/craft/reasonix）；**L1616** 第 6 条同列七家 + copilot(App DB) 单列 | ✅ | `17223`：`if (tsMs == null) tsMs = stat.mtimeMs;`；`17021`：`return snapshot.stat.mtimeMs;`；`20214` 的 `Date.now()` 末级兜底 |
| P1-6 | §5.6.5 表 opencode 磁盘支路引 `12405`（实为 cline） | **L1586** 改 `2974`、`3120` | ✅ | `rollout.js:3120`：`delta.conversation_count = 1;`（OpenCode 磁盘支路）；`12405` 属 Cline |
| P1-7 | §6.1.4 `claudeUserIdentity` 位置错 | **L1743**：定义在 `src/lib/claude-user-identity.js:13`…`rollout.js:921-933` 是 `formatLocalDate` 与文件身份哈希注释，与本函数无关 | ✅ | `claude-user-identity.js:13`：`function claudeUserIdentity(obj, aliases) {` |
| P1-8 | §6.1.4 下界保护 `:952` → `:1011` | **L1745**：`if (!bucket \|\| bucket.totals.conversation_count < count - 1) continue;`（`:1011`） | ✅ | `rollout.js:1011` 逐字一致 |
| P1-9 | §6.2.9 代码块头 `codex-service-tier.js:17` | **L2090**：`// src/lib/codex-service-tier.js:20,26` | ✅ | `:20` `const CODEX_SERVICE_TIER_MARKER = '"thread_settings_applied"';`；`:26` `const PRIORITY_SERVICE_TIERS = new Set(["priority","fast"]);`（`:17` 是 `"use strict";`） |
| P1-10 | §5.2.10 WorkBuddy input 减法引 `911` | **L1114**：`10963` | ✅ | `:10963` `const inputTokens = Math.max(0, promptTokens - cacheRead - cacheCreation);` |
| P1-11 | §5.1 读表说明「6 处不一致」不可复现 | **L873** 改为「两者有 **8 处错位**（逐条可回源，重叠形态见 §5.1.1）」并逐条列出 8 项 | ✅ | 8 项逐条命中：Kimi Code→`kimi`(`9844`)、Kiro CLI→`kiro`(`9191`/`9210`)、OpenCode 两支路共用 `opencode`(`5790`+`5972` 落桶 / `sync.js:1440`)、Qoder 新旧共用 `qoder`(`6412-6414` 注释)、WorkBuddy 一行两 source(`10651`/`11004`)、Copilot 三通道(`17878`/`18579`/`19381`)、Codex 家族一行三 source(`281`/`494`/`521`)、pi/Prime 派生(`16286-16295`/`16297-16305`) |
| P2-12 | §5.2.7 Kimi 路径引 `9501` | **L1081**：`resolveKimiWireFiles` `9507`、`path.join(kimiHome,"sessions")` `9510`；`9501` 只是 `wslProviderDir: ".kimi"` | ✅ | 三行逐字 |
| P2-13 | §5.2.12 导入闸门 `12350` → `12352` | **L1139**：`if (importedAtMs !== null && ts <= importedAtMs) continue;`（`12352`；`12350` 是上一行的 `const ts = Number(msg.ts);`） | ✅ | 逐字 |
| P2-14 | §5.6.3 AnythingLLM reasoning 引 `14040` | **L1554**：`max(0, total − input − output)`，`14042` | ✅ | `:14042` `const reasoningTokens = Math.max(0, reportedTotal - accounted);`（`:14040` 是模型兜底） |
| P2-15 | §5.6.5 trae-cn 引 `22506` | **L1588**：`22508` | ✅ | `:22506` `total_tokens: total,`；`:22507` `billable_total_tokens: total,`；`:22508` `conversation_count: 1,` |
| P2-16 | §6.2.1 冷文件跳过引 `775-857` | **L1880**：`filterColdCodexRolloutFiles`，`src/lib/rollout.js:708`–`879` | ✅ | `:708` 函数签名；`:878` `return { rolloutFiles: out, skipped, restarted: false };`；`:879` `}` |
| P2-17 | §5.6.5「7 种语义」表缺 §6 两条 | **L1582** 改「至少 **10 种语义**」，表内新增「用户轮次（只 +1，不写 token）｜claude｜`2833`」与「每个 `token_count` 事件 +1｜codex 家族｜`2554`」两行 | ✅ | `rollout.js:2833` `userBucket.totals.conversation_count += 1;`；`:2554` `delta.conversation_count = 1;` |

**R3-2 小结**：17 条**全部改对**，无一条"改一半"或"改出新错"。

### A-3 ｜任务点名的修正质量专项（7 项）

| 专项 | 判定 | 实测依据 |
|---|---|---|
| §5.6.1 A 类应列 3 家 | ✅ | 三家证据链齐备（gemini `4743`+`4945`；grok `grok-usage.js:63-71`+`rollout.js:20217-20228`；codex `4873-4876`+`4881-4893`+`4945-4953`+`2553`），且 **L1519 补了 `【未能确认】` 触发频率**并给判定条件——没有把条件形态写成确定结论 |
| §5.6.2 全零落仓应 ≥5 处 | ✅ | 5 处逐处可回源；且 **L1538** 显式区分「解析器构造零行」与「写入端撤回/对齐零行」（`3291-3316`/`3273-3290`/`19990`+`20031`），并要求下游分开计数 |
| §5.6.7 与 §5.6.2 是否已不矛盾 | ✅ | L1612 与 L1508/L1528/L1538 三处数字、集合、口径完全一致；矛盾消除 |
| §5.1「8 处错位」是否自洽 | ✅ | 列举 8 项 ↔ 总表 45 行 source 列逐项吻合；与 §5.1.1「重叠形态」表（7 行）**口径不同但不矛盾**：opencode 的"两支路共用"记在 §5.1.1 的计数口径里（L930「19+21−1=39」、L932「opencode 占两个名」），其余 7 类记在重叠表 |
| §3.4 两级回退 | ✅ | 见 A-1#1；`rollback \|\| previous` 的逻辑或语义被逐字解释，无残留"三级"表述 |
| §3.3 去重的 `!prev` 条件 | ✅ | 见 A-1#3；且补出「已有游标的重复文件仍走正常增量解析、由消息哈希兜底」，与 `:1074-1077` 注释一致 |
| §1.3 GBK 口径（单文件 15 / 全 src 52） | ✅ | 本轮独立复现：52 = 15+6+5+4+(3+3+2+2+1×12)；「其余 16 个文件各差 1–3 行」逐项成立；`(Get-Content $p).Count` = 24917 亦复现 |

---

## A2. 三条反驳的独立裁决（逐条：谁对 / 证据）

### A2-1 ｜45 家拆解：`12/1/17/15` 还是 `15/1/15/14`？—— **修正方对，复核方错**

**复核方（R3-1 必修 2）的主张**：写配置实为 15（含 Claude/Codex/**Cursor**/AStudio/Every Code/Gemini/CodeBuddy/WorkBuddy/WorkBuddy AI/Grok/oh-my-pi/OpenCode/OpenClaw/**TRAE**/**OmO**）；「仅欢迎名单」为 14。

**我的独立枚举**（亲自逐行读 `src/commands/init.js:674-1149` 全文，按"分支内是否调用 `upsert*`/`installOpenclawSessionPlugin` 改动用户配置或写盘"分类）：

写配置 **12 家**（名字/行号与主文档 L293 **一一对应、完全一致**）：

| # | label | 行号区间 | 写盘点 |
|---|---|---|---|
| 1 | Codex CLI | 686-702 | `upsertCodexNotify` (`:689`) |
| 2 | AStudio | 704-720 | `upsertAcodeNotify` (`:707`) |
| 3 | Claude | 722-731 | `upsertClaudeUsageHooks` (`:724`) |
| 4 | Gemini | 733-742 | `upsertGeminiHook` (`:735`) |
| 5 | Opencode Plugin | 744-756 | `upsertOpencodePlugin` (`:744`) |
| 6 | oh-my-pi | 789-828 | `upsertOmpHook` (`:806`) |
| 7 | Grok Build | 901-916 | `upsertGrokHook` (`:906`) |
| 8 | CodeBuddy | 1037-1046 | `upsertClaudeHook` (`:1039`) |
| 9 | WorkBuddy | 1050-1059 | `upsertClaudeHook` (`:1052`) |
| 10 | WorkBuddy AI | 1062-1071 | `upsertClaudeHook` (`:1064`) |
| 11 | OpenClaw Session Plugin | 1073-1118 | `installOpenclawSessionPlugin` (`:1078`) |
| 12 | Every Code | 1130-1146 | `upsertEveryCodeNotify` (`:1133`) |

仅探测 **1 家**：Cursor（`:758-776`，注释 `// Cursor (API-based, no hooks needed)`，三个分支只 `summary.push`）。
纯被动读取 **17 家**：`:784` Kimi Code、`:837` pi、`:845` Prime Agent、`:853` MiniMax Code、`:862` Command Code、`:872` Craft Agents、`:880` Reasonix、`:890` TRAE、`:925` Kilo CLI、`:932` DuMate、`:942` Mimo、`:950` LM Studio、`:961` Unsloth、`:973` AnythingLLM、`:985` Devin、`:1000` Kilo Code (VS Code extension)、`:1018`+`:1025` Cline——每家都是 `if (exists)` 包装内只 push（Cline 另有一条 `status:"error"` 的 push）。
仅欢迎名单 **15 家**：与 `SUPPORTED_PROVIDERS`（`:119-165`，实测 45 项）做差集，余下恰为 Antigravity、Kiro、Hermes Agent、GitHub Copilot、OmO、Dots、Roo Code、Zed Agent、Goose、Droid、ZCode、Qoder、Claude Science、DeepSeek Harness、TRAE Work CN——**这 15 个名字在 `applyIntegrationSetup` 全文中不出现任何一次**。

`12 + 1 + 17 + 15 = 45` ✅；与 `SUPPORTED_PROVIDERS.length` 闭合。

**复核方三处误判的定位**：

1. **Cursor**：源码注释与三个分支共同证明它**不写盘**；复核方把它计入"写配置"。
2. **TRAE**：`:891-897` 只有 `summary.push({label:"TRAE", status:"detected", …})`，detail 是 "Local usage reader…"（只读），**没有任何 `upsert*`**。
3. **OmO**：在 `applyIntegrationSetup` 里**根本不出现**（唯一的 OmO 语义在解析侧 `reasoningIncludedInOutput`），却被复核方计入"写配置"并从欢迎名单移出——这是它得到 15/14 的直接原因。

**R3-1 的合理部分已被吸收**：复核方指出「MiniMax Code/Command Code/Craft Agents/Reasonix 有显式分支、不是"无分支"」是对的——主文档 L291 现版**新增了口径定义**并把这四家明确归入「纯被动读取」，同时写明「**"没有写盘分支"不等于"没有 `if` 分支**」；歧义已消。

**裁决：`12/1/17/15` 成立，复核方错。**

### A2-2 ｜骨架 D 代码块写 `:16080` 还是 `:16084`？—— **修正方对，复核方错**

- 主文档现文本：**L581** 代码块 `// :16084 骨架 D`、**L588** 表格 `parseOmpLikeIncremental(16084)` —— 两处一致。
- 样本仓 `src/lib/rollout.js` 全文检索 `16080`：**零命中**（`Select-String -Encoding UTF8`）。
- `src/lib/rollout.js:16084` 逐字：`return { recordsProcessed, eventsAggregated, bucketsQueued, projectBucketsQueued };`（`:16080` 是上一段对象字面量的 `: {}),`）。
- 「全文 `16080` 零命中」也已在主文档 §13.6 的反驳记录里写明（**L4205**，该行是**引用复核方的错误主张**，不是文档自身的引用，属正常留痕）。

**裁决：复核方复核时看到的 `:16080` 不可复现，修正方对。**

### A2-3 ｜`setMaxTotal` 是否在 `sync.js:6540`？—— **修正方对，复核方错**

`src/commands/sync.js` 中 `setMaxTotal(` 的全部出现（6 处）：

```
6492  setMaxTotal(totals, key, Number(row.total_tokens || 0));
6512  setMaxTotal(totals, usageKey, Number(bucket?.totals?.total_tokens || 0));
6540  setMaxTotal(modelTotals, `${row.source}|${model}|${row.hour_start}`, Number(row.total_tokens || 0));
6559  setMaxTotal(modelTotals, `${bucketSource}|${model}|${hourStart}`, Number(bucket?.totals?.total_tokens || 0));
6579  setMaxTotal(merged, key, total);
6585  function setMaxTotal(map, key, total) {      ← 定义
```

`:6540` **确实是调用点**，且落在 `mainUsageHourTotalsFromQueueRows`（定义 `:6529`、返回 `:6542`）体内：

```js
6529  function mainUsageHourTotalsFromQueueRows(rows, source) {
6539    const model = typeof row?.model === "string" && row.model ? row.model : "unknown";
6540    setMaxTotal(modelTotals, `${row.source}|${model}|${row.hour_start}`, Number(row.total_tokens || 0));
6541    }
6542    return collapseModelTotalsByHour(modelTotals);
```

定义在 `:6585` 与"调用点在 `:6540`"毫不矛盾（先有调用、后有定义，函数声明提升）。主文档 §4.7（**L859**）写「`mainUsageHourTotalsFromQueueRows`…用 `setMaxTotal(key, total_tokens)`（取最大值，`src/commands/sync.js:6540`）…再由 `collapseModelTotalsByHour` 跨模型求和（`:6564-6571`）」——行号与函数名都对（`collapseModelTotalsByHour` 实测 `:6564` 定义、`:6571` 返回）。

**裁决：复核方的"调用点不对应"不成立，修正方对。**

---

## B. 全新抽样（逐条）

抽样原则：只取**前三轮报告未出现过的行段**；每条亲自打开（`ReadAllLines` UTF-8）并逐字比对。共 **50 条**（覆盖约 80 个行号点），命中 **47 条 / 77 点**。

| # | 文档引用（文档行） | 期望要点 | 实测 | 判定 |
|---|---|---|---|---|
| B-01 | `src/lib/fs.js:69-74`（L344） | 锁四常量 | `LOCK_STALE_MS = 5*60*1000`、`LOCK_HEARTBEAT_MS = 30*1000`、`MAX_RECLAIM_DEPTH = 4`、`MAX_RECLAIM_SWEEP_DEPTH = 64` | ✅ |
| B-02 | `src/lib/fs.js:206-224`（L344/537） | 心跳 `utimes` 两文件 + unref | `:210-213` `handle.utimes(now,now)` + `heartbeatHandle.utimes(now,now)`；`:222` `timer.unref?.()` | ✅ |
| B-03 | `src/lib/fs.js:105-144`（L344） | 回收判定（PID → token 心跳 → mtime） | `:118` `if (!isProcessAlive(owner.pid)) return true;`；`:127-129` 心跳 `Date.now()-mtimeMs > LOCK_STALE_MS`；`:136` 老格式退回 mtime | ✅ |
| B-04 | `src/lib/fs.js:342-368`（L344） | `.reclaim` 次级锁 + 持闸重校验 | `:342` 开 `${lockPath}.reclaim`；`:352` `sweepAbandonedReclaimGuards`；`:366-368` `// Re-check while holding the guard` + `existingLockCanBeReclaimed` | ✅ |
| B-05 | `src/lib/fs.js:260-278`（L344） | 只有 token 匹配才 unlink | `:269` `if (owner?.token === token)` → `:273-274` 两次 unlink | ✅ |
| B-06 | `src/commands/sync.js:3711-3745`（L328） | `parseArgs` 11 个选项 + 未知抛错 | `:3712-3724` 默认值表；`:3735` `--source=`；`:3738` `--background \|\| --lightweight`；`:3742` `throw new Error(\`Unknown option: ${a}\`)` | ✅ |
| B-07 | `src/commands/sync.js:719-735`（L328） | `--auto` 决定 sourceAllowed 白名单 | `:721-731` 轻量分支含 `BACKGROUND_AUTO_SYNC_SOURCES` / `AUTO_SYNC_SOURCES`；`:733` `autoSourceScope` | ✅ |
| B-08 | `src/commands/sync.js:367-373`（L328） | 背景四源 | `new Set(["acode","codex","every-code","reasonix"])`（恰 4 项） | ✅ |
| B-09 | `src/commands/sync.js:4186-4200`（L541） | 已有更早计划不重复排 | `:4186` `if (existingMs && existingMs >= retryMs - 1000)` → 仅在 `syncSource` 变化时改写 | ✅ |
| B-10 | `src/commands/sync.js:4244-4247`（L541） | `clearAutoRetry` 删 marker | `:4244` 函数定义、`:4246` `await fs.unlink(retryPath)` | ✅ |
| B-11 | `src/commands/sync.js:4305-4313`（L541/547） | 2h 上限 + 500 上限 | `:4310` `AUTO_RETRY_MAX_DELAY_MS = 2*60*60*1000`；`:4313` `MAX_INGEST_BUCKETS = 500` | ✅ |
| B-12 | `src/commands/sync.js:552-559`（L317） | 抢不到锁不抛错 | `:553` `acquireSyncLock`；`:554-558` `process.stderr.write(await recordSyncSkip(...)); return;` | ✅ |
| B-13 | `src/commands/sync.js:4447-4449`（L512/547） | 只上传完整行 | `:4447` `for await (const record of iterateQueueLines(handle, startOffset, st.size))`；`:4449` `if (!record.complete) break;` | ✅ |
| B-14 | `src/commands/sync.js:4478-4489`（L547/512） | `(source,session_id)` 严格更新 + 批上限 | `:4479-4484` `verifiedMs > priorVerifiedMs` 才覆盖；`:4489` `if (linesRead >= maxBuckets) break;` | ✅ |
| B-15 | `src/lib/rollout.js:2401-2426`（L1909） | `compacted` 紧随判定 | `:2401` 未终止即 break；`:2410` `awaitsCompactedLine`；`:2422-2424` `next?.type === "compacted"` → 推进提交位 + continue | ✅ |
| B-16 | `src/lib/rollout.js:2530-2556`（L1929-1943/1976-2021） | `token.info` / `latestTotal` / 哨兵 / `conversation_count` | `:2530-2531`；`:2547` `latestTotal = totalUsage`；`:2550-2552` `source === DEFAULT_SOURCE && isCodexTotalOnlyResetSentinel(...)`；`:2554` `delta.conversation_count = 1;` | ✅ |
| B-17 | `src/lib/rollout.js:3316-3347`（L752-766/827-845） | 主队列行 11 字段 + 撤回分支 | `:3325-3327` `queuedKey` 短路；`:3339` `?? totals.total_tokens`；`:3341` `usage_precision`；`:3342` `...prioritySubsetOf(totals)`；`:3346` 回写 | ✅ |
| B-18 | `src/lib/rollout.js:1194-1204`（L659/1194 引用） | Claude 游标写入 | `:1194` `offset: result.endOffset,`；`:1197` `fileId`；`:1200-1203` `claudeCwd`/`projectFileContext`/`projectRef`/`projectKey` | ✅ |
| B-19 | `src/lib/rollout.js:520-540`（L651） | `projectUpToDate` / `projectContextOnlyScan` | `:523-530` 四条件；`:531-537` 六条件（`startOffset >= st.size && !projectUpToDate`） | ✅ |
| B-20 | `src/lib/rollout.js:589-604`（L651） | 只扫上下文不算 token 的路径 | `:589` `const result = projectContextOnlyScan ? await scanRolloutProjectFileContexts({…})` | ✅ |
| B-21 | `src/lib/rollout.js:4755-4770`（L1032） | `normalizeOpencodeTokens` 五桶 | `:4757-4762` 五入参 + `total = input+output+reasoning+cached+cacheWrite`；`:4764-4769` 五桶返回 | ✅ |
| B-22 | `src/lib/rollout.js:2916`（L1021） | Gemini 事件粒度 | `const messages = Array.isArray(session?.messages) ? session.messages : [];` | ✅ |
| B-23 | `src/lib/rollout.js:2952/2956/2964`（L1023-1024） | 模型沿用 + 时间缺失跳过 | `:2952` `normalizeModelInput(msg.model)`；`:2953` `if (normalizedModel && idx >= hourlyBegin) model = normalizedModel;`；`:2964-2966` `if (!timestamp \|\| !currentTotals) continue;` | ✅ |
| B-24 | `src/lib/rollout.js:1284/1287-1291`（L1021） | Gemini 文件身份含 inode | `:1284` `const inode = st.ino \|\| 0;`；`:1287-1291` `sameFileMetadata` 三元组比较 | ✅ |
| B-25 | `src/lib/rollout.js:4272`（L1033） | modelID / model / model.id 兼容 | `:4272` `function normalizeOpencodeModelFields(msg) {`；`:4273-4274` `normalizeModelInput(msg?.modelID) \|\|` | ✅ |
| B-26 | `src/lib/rollout.js:3805/3105`（L1035） | fork 副本去重 / 复活重加 | `:3805` `isOpencodeForkCopy(fingerprintIndex, fingerprint, messageKey)`；`:3105-3108` `effectiveLastTotals = prev?.dedupedForkCopy === true ? null : lastTotals;` + 注释 | ✅ |
| B-27 | `src/commands/sync.js:1384`（L1036） | 磁盘 + SQLite 两路结果相加 | `:1384` 是空行；相加语句在 **`:1385-1386`**（`recordsProcessed: filesResult.filesProcessed + dbResult.messagesProcessed`） | ⚠️ 偏移 1–2 行 |
| B-28 | `src/lib/rollout.js:2141`（L1046） | OpenClaw 不支持项目维度 | `// Project-level OpenClaw attribution is not supported yet (no stable cwd info).` | ✅ |
| B-29 | `src/lib/rollout.js:1710/2114/2128`（L1045） | 偏移 + 事件身份计数 + 时间闸门 | `:1710` `previousOffset`；`:2114` `priorCount`；`:2128-2130` `aggregateAfterMs` 判定 | ✅ |
| B-30 | `src/lib/rollout.js:7659/7676/8006/8008`（L1061-1063） | Hermes state.db / profiles / 模型兜底 / 落桶 | `:7659` `resolveHermesDbPath`；`:7676` `path.join(profilesDir, entry.name, "state.db")`；`:8006` `\|\| "hermes-agent"`；`:8008` `const delta = {` | ✅ |
| B-31 | `src/lib/rollout.js:10035/10069/10278/10293`（L1102-1103） | CodeBuddy 双源 + 镜像优先级 | `:10035` `path.join(codebuddyHome,"projects")`；`:10069` `} else if (process.platform === "win32")`；`:10278` marker；`:10293` `firstPresentNonNegativeInt([usage.cachedMissTokens, usage.cacheMissTokens, …])` | ✅ |
| B-32 | `src/lib/rollout.js:11892/11899/11902`（L1126-1128） | Roo 入口/时间/去重键 | `:11892` `if (msg.say !== "api_req_started" && msg.say !== "api_req_deleted") continue;`；`:11899` ts 校验；`:11902` `${taskUuid}:${ts}` | ✅ |
| B-33 | `src/lib/rollout.js:12518/12636/12742/12816`（L1146-1150） | Zed blob/SQL 游标/reset/上限 | `:12518` `decodeZedThreadBlob`；`:12636` `WHERE updated_at > '…'`；`:12742` `isReset = currSum > 0 && currSum < prevSum`；`:12816` `if (entries.length > 10_000)` | ✅ |
| B-34 | `src/lib/rollout.js:12859/13099/13482/13714/13717`（L1156/1161/1167/1171） | LM Studio home / 哈希 key / Unsloth 路径 / 有界快路径 | 五处逐字命中（`:13714` `hasBoundedScan`、`:13717` 短路返回） | ✅ |
| B-35 | `src/lib/rollout.js:14797/15069/15243/15379`（L1197-1202） | Droid 路径/去重函数/mtime 判据/会话数 | `:14797` `resolveDroidSessionsDirs`；`:15069` `dedupeDroidSettingsFilesBySession`；`:15243` `globalNeedsUpdate = !(mtimeMs && mtimeMs === prev.mtimeMs)`；`:15379` `isFirstSeenSession \|\| isReset ? 1 : 0` | ✅ |
| B-36 | `src/lib/rollout.js:15617/11706/11730`（L1207-1211） | Kilo 时间/任务文件/provider 模型 | `:15617` `const ts = Number(msg.ts);`；`:11706` `resolveKilocodeTaskFiles`；`:11730` `normalizeKilocodeProviderToModel` | ✅ |
| B-37 | `src/lib/rollout.js:243/245/5025/5033`（L1031） | OpenCode 消息文件收集 | `:243` `listOpencodeMessageFiles`；`:245` `path.join(storageDir,"message")`；`:5025` `walkOpencodeMessages`；`:5033` `startsWith("msg_") && endsWith(".json")` | ✅ |
| B-38 | `src/lib/rollout.js:9575/9557/9542/9595`（L1082-1085） | Kimi StatusUpdate/游标/时间 | `:9575-9576` `msg.type !== "StatusUpdate"`；`:9557` `fileOffsets[filePath]`；`:9542` `cursors.kimi = {…seenIds, fileOffsets…}`；`:9595` `entry.timestamp ?? payload.timestamp` | ✅ |
| B-39 | `src/lib/rollout.js:22626-22634`（L1386） | trae-cn 缺席契约 | `:22626-22633` `Evidence check (2026-08-16 …)` + `the absence contract is NOT PROVEN and must stay symmetric`；`:22634` 早退 3 键 | ✅ |
| B-40 | `src/lib/rollout.js:22402-22413`（L1386） | 持久化贡献逐字段校验 | `:22403` `reasoning_output_tokens !== 0 \|\| conversation_count !== 1`；`:22406-22411` sum 校验 + `billable_total_tokens !== total_tokens` | ✅ |
| B-41 | `src/lib/trae-usage.js:63-81`（L1399/§5.5.2） | 候选唯一解 | `:64-69` 双层 `new Set([...])` 组合 + `!== total` 剔除；`:81` `if (candidates.size !== 1) return null;` | ✅ |
| B-42 | `src/lib/rollout.js:7184-7210`（L1377/1399） | Cursor 清桶护栏 | `:7194-7200` Guard 注释；`:7207-7209` `No parseable record date … Skip the wipe entirely`；`:7210` `if (earliestBucketStart)` | ✅ |
| B-43 | `src/lib/rollout.js:7545/14670/14013`（L1550） | kiro 两列 / goose / anythingllm 映射 | `:7545-7547`（kiro ✅）；`:14013`（anythingllm ✅）；**`:14670` 是 `parseGooseModelName(row.model_config_json)`**，五桶取值在 `:14674-14686` | ⚠️ 1 点指向不精确 |
| B-44 | `src/lib/rollout.js:15914/21807/23364/24214`（L1587/1590） | 四家 conversation_count 段 | `:15914` `conversation_count: 1,`；`:21807` `delta.conversation_count = 1;`；`:23364` `conversation_count: 1,`；`:24214` `conversation_count: total > 0 ? 1 : 0,` | ✅ |
| B-45 | `src/lib/rollout.js:22157`（L1472/1601） | trae 精度落桶 | `hourlyState.buckets[key].usage_precision = precision === "reported" ? null : precision \|\| null;` | ✅ |
| B-46 | `src/lib/openclaw-session-plugin.js:154-178/180`（L303） | 状态探测 vs 策略 | `:154-167` 五布尔计算；`:169-177` 返回块；`:180` `ensureOpenclawSessionPluginPolicy` | ✅ |
| B-47 | `test/trae-entitlement.test.js:29/130-137`（L694） | 主实现定向测试 | `:29` `toUtcHalfHourStart,`（来自 `../src/lib/rollout`）；`:130-137` 两条 `assert.equal` | ✅ |
| B-48 | `src/lib/rollout.js:14674-14713`（L1188/1236） | Goose 五桶 + 时间兜底 | `:14674-14686` total/input/output 取值与 `continue` 条件；`:14711` `reasoningDelta = Math.max(0, dTotal - accountedDelta)`；`:14713` `\|\| new Date().toISOString()` | ✅ |
| B-49 | `src/lib/rollout.js:5790/5972`（§5.1 第 20 行） | OpenCode DB 支路 source | `:5790` `const defaultSource = normalizeSourceInput(source) \|\| "opencode";`；`:5972` `getHourlyBucket(hourlyState, defaultSource, model, bucketStart)` | ✅ |
| B-50 | `src/lib/rollout.js:20031/22594` + `src/commands/sync.js:4114`（L841/847） | append 点 | `:20031` `await fs.appendFile(queuePath, \`${lines.join("\n")}\n\`, "utf8");`；`:22594` 同形；`sync.js:4114` 写 fallback 文件 | ✅ |

**附加抽验（不单列）**：`rollout.js:6173`（`cache_creation_input_tokens: 0`，见必须修 #3）、`rollout.js:7232`（cursor `conversation_count = 1`）、`rollout.js:21831`/`:21832`（antigravity billable + 会话数）、`rollout.js:6412-6414`（qoder 双通道同 source 注释）、`rollout.js:16286-16295`/`16297-16305`（pi / prime-agent source 派生）、`sync.js:6844`（`repairClaudeQueueFromGroundTruth`）、`sync.js:5848-5879`（`mergeMaxTotals`/`mainUsageHourTotalsFromQueueRows` 调用族）、`rollout.js` 全量 `usage_precision` 16 行（`3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286/20287/22011/22025/22113/22157`，与 §5.5.6 清单**逐行相同**、无 antigravity/kiro）。

**命中率**：条 **47/50 = 94.0%**（3 条 ⚠️ 均为低危偏移）；行号点 **约 77/80 = 96.3%**。

---

## 必须修清单

**无【高】/【中】级问题。以下 3 条【低】为引用级校准，建议本轮或下一轮顺手改掉（都不影响任何结论）：**

1. **§5.6.3 表（L1551）**：`qoder`（旧库）"cache_creation 恒 0"引 `6177`–`6178` —— 该赋值在 **`6173`**（`cache_creation_input_tokens: 0,`）；`:6177` 是 `billable_total_tokens`、`:6178` 是 `};`。建议改为 `6173`，或与 §5.6.4 的 `6177`（billable）分开标注。
2. **§5.2.17（L1188）与 §5.6.3（L1550）**：`goose` 五桶/无缓存列引 **`14670`** —— 该行是 `parseGooseModelName(row.model_config_json); if (!model) continue;`（模型链，不是桶映射）。五桶取值在 **`14674`–`14686`**；`:14708`–`:14711` 的 reasoning 差额引用**正确**。建议把 `14670` 改为 `14674`–`14686`（或保留 14670 作为"模型链"引用并补五桶行号）。
3. **§5.2.2 末（L1036）**：`sync.js:1384` 指"两路结果相加" —— 实际相加在 **`1385`–`1386`**（`:1384` 为空行）。建议改为 `1385-1386`。

**另记 1 条（不在本次范围，供 §7 之后单元参考）**：附录 B 术语表「`setMaxTotal`…`src/commands/sync.js:6574-6589`，唯一调用族 `:5848-5879`」——`:5848-5879` 是**间接**调用族（`mergeMaxTotals` → `mainUsageHourTotalsFromQueueRows` 等），`setMaxTotal` 的**直接**调用点在 `:6492/6512/6540/6559/6579`。表述可更精确，但不构成错误。

---

## 复核到的行号抽样（证明真的打开过）

本轮实际打开（`ReadAllLines` UTF-8 / `read`）并逐字比对的源码位置：

| 文件 | 打开的行段 |
|---|---|
| `src/lib/rollout.js` | 241-247、520-540、589-604、706-712、875-882、946-1015、1070-1082、1194-1204、1283-1292、1710-1712、2112-2130、2139-2143、2401-2426、2530-2556、2826-2836、2914-2918、2950-2966、3103-3108、3116-3124、3316-3350、3400-3410、3542-3558、3625-3660、3803-3808、4271-4274、4755-4770、4881-4893（引用）、4940-4960、5024-5034、5755-5800、5904-5907、5972-5986、6150-6180、6277-6290、6336-6342、6410-6416、7182-7212、7230-7234、7543-7549、7657-7678、8006-8010、9538-9542、9555-9559、9573-9597、10033-10037、10067-10071、10276-10295、10960-10965、11704-11708、11728-11732、11890-11904、12348-12354、12516-12520、12634-12638、12740-12744、12814-12817、12857-12860、13097-13101、13480-13484、13712-13718、14011-14017、14038-14044、14668-14690、14704-14714、14795-14799、15067-15071、15241-15245、15377-15381、15615-15619、15912-15916、16080-16090、16284-16306、17018-17024、17033-17048、17064-17070、17220-17226、19800-19810（引用）、20212-20230、20252-20258、20266-20270、20284-20290、21100-21110（引用）、21805-21809、21829-21833、22155-22159、22400-22415、22504-22510、22592-22596、22624-22636、23362-23366、24100-24110（引用）、24212-24216 |
| `src/lib/fs.js` | 69-74、105-144、200-205、206-224、260-278、342-368 |
| `src/lib/grok-usage.js` | 60-92 |
| `src/lib/trae-usage.js` | 61-83 |
| `src/lib/cursor-store.js` | 120-130 |
| `src/lib/scan-roots.js` | 89-96、246-254 |
| `src/lib/claude-user-identity.js` | 5-20、28-33、42-46 |
| `src/lib/codex-service-tier.js` | 15-30 |
| `src/lib/openclaw-session-plugin.js` | 152-182 |
| `src/commands/sync.js` | 365-375、552-560、719-736、1367-1386、3711-3745、4186-4200、4240-4250、4305-4314、4445-4452、4470-4490、5846-5880、6525-6600、6842-6846 |
| `src/commands/init.js` | 119-165、272-278、674-800、800-1149（全文逐行枚举） |
| `test/trae-entitlement.test.js` | 28-30、128-139 |

**全库级复现命令（均为只读）**：

- `Select-String -Path src/lib/rollout.js -Pattern "16080" -Encoding UTF8` → **0 命中**
- `Select-String -Path src/lib/rollout.js -Pattern "usage_precision" -Encoding UTF8` → 恰好 16 行，与 §5.5.6 清单逐行相同
- `Select-String -Path src/commands/sync.js -Pattern "setMaxTotal\(" -Encoding UTF8` → 6 处（6492/6512/6540/6559/6579 + 定义 6585）
- cp936 整体解码 vs `ReadAllLines` UTF-8：`src/**/*.js` 受影响 **20 个文件、吞行合计 52**（`rollout.js` 15 / `local-api.js` 6 / `sync.js` 5 / `session-analytics.js` 4 / 其余 16 个各 1–3）
- `(Get-Content src/lib/rollout.js -Encoding Default).Count` → **24917**（UTF-8 为 24932，差 15）

**范围声明**：本轮未修改样本仓库（`git status --porcelain` 为空）与主文档；未读取任何真实会话数据；§5.3.2–§5.4.4 中未逐条展开的行号（如 `trae-snapshot.js`/`trae-sqlite.js`/`trae-db.js` 细节、`sqlite-reader.js` 异步孪生）不在本次抽样表内，不计入命中统计。
