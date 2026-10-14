# 全文复核 R7-2：§5 45 家 provider

- 被复核文档：`docs/research/2026-10-14-tokentracker-usage-statistics.md`，复核范围 = §5 全部内容（L872–L1647，第 5 章标题至第 6 章之前）。
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，复核前后 `git status --porcelain` 均空（未修改任何文件，未做任何 git 写操作）。
- 行号口径：全部用 Node `fs.readFileSync(...,'utf8').split(/\r?\n/)` / `[System.IO.File]::ReadAllLines` 的 UTF-8 实际行号；未使用裸 `Get-Content`。
- 复核方式：脚本机械抽取全部引用（`%TEMP%\r72_extract3.js`，统计输出 `%TEMP%\r72_refs3.txt`）+ 两个独立证伪子核验（分支 A：8 家、分支 B：9 组）+ 本人关键点位核验与否定性结论重建。子核验的每条指控均经本人回源复验后才采信（见 §B 备注）。
- 本报告**只做证伪与登记，不修改文档与样本仓库**。

## 结论

**§5 结论层全部成立，未发现被推翻的机制性断言；问题集中在「引用区间选点/引文出处」与 2 条需要限定或改写的语义表述。**

1. **引用存在性**：§5 范围内共抽取 **1527** 个引用点（显式 `路径:行号` 183 个 + 裸行号 1344 个，覆盖 23 个文件），**越界 0、指向不存在文件 0**。§5.1.2 表的 45 个 `parse*Incremental` 定义行与附录 A 与源码逐行一致（差异 0，本人用正则独立重生成验证）。
2. **逐家核验**：两个子核验按「路径拼接 / 字段→五桶映射 / 游标判据」三要素完整核验 **21 家**（WorkBuddy、Roo、Kilo、LM Studio、Kiro IDE、Kiro CLI、Craft、Reasonix、pi、Prime Agent、MiniMax Code、OMP、OmO、DuMate、ZCode、Mimo、Command Code、DSH、Antigravity、Trae、Trae CN）；本人另对 22 家做关键点位核验。**没有发现与源码语义相反的虚构断言**——问题全部是行号漂移（1–2 行或区间选点）、引文出处错位、顺序/措辞不精确。
3. **否定性结论重建（§5.6 风险区）**：三条核心否定性结论**全部成立**（A 类 3 家、全零落桶 ≥5 处、`usage_precision` 16 行/15 语句且无 antigravity/kiro）；`billable_total_tokens` 的「8 显式 + 13 兜底」**需补一条限定**（漏了 claude-science，见 M1）。
4. **必须修 2 条（中）+ 应修 12 条（低，全部为引用校准/措辞）**；其余章节无回改需求。

## A. 全量引用存在性

### A.1 抽取口径

- 抽取规则：显式 `file:line` / `file:line–line` 直接解析（短文件名按仓库唯一 basename 补全路径）；裸行号（反引号包裹的纯数字/区间）默认归属 `src/lib/rollout.js`，仅当同一行左侧 200 字符内有显式引用锚且中间无括号断裂时改归该文件（§5.3.1–§5.4 的 `sqlite-reader.js` / `cursor-config.js` / `trae-cn-config.js` / `grok-usage.js` / `devin-usage.js` / `trae-*.js` / `local-api.js` / `omp-hook.js` / `install-resolver.js` 段落按此命中）。
- 排除项：`MB`/`条`/`次`/`层` 等量词后的数字、`0`/`1` 之类非行号数字（人工复核时二次筛除）。

### A.2 统计表

| 口径 | 数量 |
|---|---|
| 引用点总数（去重后） | **1527** |
| 其中：显式 `路径:行号` | 183 |
| 其中：裸行号（默认 `src/lib/rollout.js`） | 1344 |
| 越界（起始或结束行 > 文件总行数） | **0** |
| 指向不存在的文件 | **0** |
| 抽取器自动改归文件后仍越界的条目 | 0（12 条初判异常经人工复核全部为「裸行号默认属 `rollout.js`」的吸附误判，数值均在 `rollout.js` 24932 行内） |

按文件分布（前 10）与按小节分布：

| 文件 | 引用数 | 小节 | 引用数 |
|---|---|---|---|
| `src/lib/rollout.js` | 1382 | §5.1（L872–1013） | 62 |
| `src/commands/sync.js` | 36 | §5.2（L1013–1250） | 436 |
| `src/lib/dumate.js` | 14 | §5.3（L1251–1375） | 615 |
| `src/lib/grok-usage.js` | 14 | §5.4（L1376–1417） | 94 |
| `src/lib/devin-usage.js` | 13 | §5.5（L1418–1508） | 122 |
| `src/lib/trae-usage.js` | 9 | §5.6（L1509–1647） | 198 |
| `src/lib/sqlite-reader.js` | 8 | — | — |
| `src/lib/omp-hook.js` | 7 | — | — |
| `src/lib/cursor-config.js` | 7 | — | — |
| `src/lib/trae-cn-config.js` | 6 | — | — |

其余：`trae-sqlite.js` 5、`passive-mode.js` 4、`init.js` 4、`trae-snapshot.js` 4、`local-api.js` 4、`trae-db.js` 2、`source-metadata.js` 2、`install-resolver.js` 1、`status.js` 1、`pricing/index.js` 1、`package.json` 1、`migrations/20260817120000_account-session-states.sql` 1、`migrations/20260904064000_cache-leaderboard-total-rollup.sql` 1。

### A.3 内容级抽核（引用是否指向声称的东西）

机械存在性之外，对 60+ 个引用点做了「指向内容」核验，全部正确，摘录见「复核证据抽样」。仅 12 条存在**区间选点/出处**问题，列入 L1–L12。

## B. 逐家核验

### B.1 §5.1 总表与 §5.1.2/附录 A 索引表

**总表抽样 20 行**（覆盖 20 家 / 45 行），核「显示名 / `source` 字面量 / 路径 / hook / 联网」逐项比对源码：

| 总表行 | `source` 断言 | 源码证据 | 判定 |
|---|---|---|---|
| Gemini CLI | `gemini` | `sync.js:1272 source: "gemini"` | ✅ |
| Kiro（IDE） | 硬编码 `kiro` | `rollout.js:7567 getHourlyBucket(hourlyState, "kiro", model, bucketStart)` | ✅ |
| Hermes | `hermes` | `rollout.js:8018` | ✅ |
| Kiro CLI | `kiro`（与 IDE 合并） | `rollout.js:9191`/`:9210` 两处均为 `"kiro"` | ✅ |
| Kimi | `kimi` | `rollout.js:9611` | ✅ |
| Kimi Code | `kimi`（与 Kimi 同源） | `rollout.js:9844` | ✅ |
| CodeBuddy | `codebuddy` | `rollout.js:10384` | ✅ |
| WorkBuddy | `workbuddy`/`workbuddy-ai` 按 variant | `rollout.js:11004 getHourlyBucket(hourlyState, variant, ...)`；`sync.js:2679 source: "workbuddy-ai"` | ✅ |
| Roo Code | `roocode` | `rollout.js:11939` | ✅ |
| Cline | `cline` | `rollout.js:12396` | ✅ |
| Zed | `zed` | `rollout.js:12782` | ✅ |
| LM Studio | `lmstudio` | `rollout.js:12852 const LMSTUDIO_SOURCE = "lmstudio"` | ✅ |
| Unsloth | `unsloth` | `rollout.js:13467 const UNSLOTH_SOURCE = "unsloth"` | ✅ |
| AnythingLLM | `anythingllm` | `rollout.js:14053` | ✅ |
| Goose | `goose` | `rollout.js:14730` | ✅ |
| Droid | `droid` | `rollout.js:15381` | ✅ |
| Kilo Code | `kilo-code`（带连字符） | `rollout.js:15651` | ✅ |
| Copilot 三通道 | 共用 `copilot` | 三份实现分别自查（§B.2）；`pricing/index.js:248–256` 中 copilot 不在折叠集 | ✅ |
| Cursor / Trae CN 账号级 | `cursor`/`trae-cn` | `source-metadata.js:1 const ACCOUNT_LEVEL_SOURCES = new Set(["cursor", "trae-cn"]);` | ✅ |
| Trae CN 三重触发 | opt-in + 非轻量 + allowed | `sync.js:2234–2238`（`!isBackgroundLightweightSync && isTraeCnUsageEnabled(process.env) && sourceAllowed("trae-cn")`）；`trae-cn-config.js:24–33` 注释原文「strictly opt-in: nothing is sent unless the user explicitly sets this flag」 | ✅ |

**索引表（§5.1.2 45 行 + 附录 A 46 条）**：用正则独立重生成 `rollout.js` 的 45 个 `function parse*Incremental` 定义行——**45/45 与 §5.1.2 表一致，附录 A 差异 0**。抽验加宽定义形态：`parseKiroIncremental`(7472)、`parseHermesIncremental`(7900)、`parseDshIncremental`(23678)、`parseOmpIncremental`(16087，`options = {}` 形态) 均与源码逐字一致。

**§5.1.1 计数口径**：`45 = 39 + 1 + 2 + 3`（39 实体 = 19 文本 + 21 数据库/API − 1 opencode 重叠；1 = opencode 占两名；2 = OmpLike/PiLike 壳；3 = 第 6 章函数名）算术闭合，且与本人枚举逐一核对成立。8 处「显示名 ↔ source」错位清单（Kimi Code、Kiro CLI、opencode 双支路、Qoder 双通道、WorkBuddy 双 source、Copilot 三通道、Codex 家族三 source、pi/Prime 派生）逐条成立。

### B.2 §5.2 / §5.3 逐家三要素核验（21 家完整 + 22 家抽核）

**完整三要素核验的 21 家结论**：

| 家 | 路径拼接 | 字段→五桶 | 游标判据 | 结论 |
|---|---|---|---|---|
| WorkBuddy | ✅ `10651–10653` 两 variant、`sync.js:2679` | ✅ `10963` 双扣、`10966` output−reasoning、`10968` 五桶闸门；traces 支路 `11105`/`11109`/`11111`/`11113` | ✅ `11007`+`11008–11012` 双写 | 符合 |
| Roo Code | ✅ 复用 Kilo 根 `11768` + `11771` 目录 | ✅ `11906–11909`、`11930–11931` | ✅ `11902 taskUuid:ts` + mtime 闸门 | 符合 |
| Kilo Code | ✅ `11710`、`sync.js:1949` | ✅ `15624–15647`、模型 `11730`/`11740 provider:${slug}` | ✅ `15620` + `15658 {size,mtimeMs,ino}` | 符合 |
| LM Studio | ✅ `12852–12854`、`12859–12868`、`12871` | ✅ `13002–13045`（total=max、cached≤prompt、`13040` 反推 input） | ✅ `13315–13327` 三态、`13094`/`13129` mtime | 符合（表格栏除外，见 M2） |
| Kiro（IDE） | ✅ `7282`/`7299`/`7303` | ✅ `7545–7563`（两列、无 cache_creation 键、`7547` 闸门） | ✅ `7541`/`7572` 自增 id、`7338` JSONL 行号 | 符合 |
| Kiro CLI | ✅ `8132–8140`、`8165` | ✅ `8371–8387` 真实值优先、`9152–9159` 估算、`9209` 闸门、`9414` 只检 4 列、`9446` total 含 reasoning | ✅ `9172–9175` 指纹、`9192–9204` 先减后加 | 符合（两处行号小偏，L9/L10） |
| Craft | ✅ `16813–16820`、`16863–16872` | ✅ `17172–17175`、`17236–17244`、`17243` 会话首见 | ✅ `17251–17258`、`17271–17280` 5000 上限按 `lastSeenAt` | 符合 |
| Reasonix | ✅（漏两个 env，L11） | ✅ `16989–17005`（`17002` output−reasoning、`17004 requests`） | ✅ `17008–17014` 仅 clamp、`17016–17022` 三级时间 | 部分不符（L11） |
| pi / Prime / MiniMax | ✅ `16130–16138`/`16171–16223`/`16200–16220`、`16250–16275`、`16727–16764` | ✅ `16449–16453`、`16482–16485`、`16766–16776` 的 `timestamp: null` | ✅ `16400–16426`；`seenIds` 上限行号偏（L8） | 部分不符（L8） |
| OMP / OmO | ✅ `11405–11407`、`11355–11356`、`11582–11595` | ✅ `15868–15872`、`16110 reasoningIncludedInOutput: true`、`15902–15903` 求和排除 | ✅ `15845–15846`、撕裂尾 `15739–15745`、`16044–16045` | 符合 |
| DuMate | ✅ `dumate.js:7–30`（含 realpath 去重） | ✅ `72–73` output−reasoning | ✅ `dumate.js:52–56` 最新副本 | 符合 |
| ZCode | ✅ 库路径对；「含 WSL 分支」的引用区间不准（L7） | ✅ `5434–5438` 黑名单、`5455–5456` 包含式拆分、`5477–5533` model_usage | ✅ 三态 `5555–5584`、`5600–5607` 保留首次选择 | 部分不符（L7） |
| Mimo | ✅ `install-resolver.js:98–112` | ✅ `5396–5400`/`5404–5407` providerID 白名单 | ✅ 一次性修复 `sync.js:4777` 起 | 符合 |
| Command Code | ✅ `24129–24132`、`24085–24089`、`24095–24127` | ✅ `24193–24197` 双扣、`24199–24204` 显式全零、`24210–24213` 哨兵 | ✅ `24060–24066`/`24542`/`24544`/`24068–24069`；「重读」引用点偏（L5） | 部分不符（L5） |
| DSH | ✅ `22826–22833`/`22839–22865`/`22901–22925` | ✅ `23345–23365` 互斥五列、零值闸门 `23354` | ✅ `23628–23645` seq 水位、`23610–23615` 撕裂、`23717–23746` 迁移；引文出处与落库区间偏（L1–L4） | 部分不符（L1–L4） |
| Antigravity | ✅ `20559–20566`、`20604–20615`、`21558–21563` | ✅ `21505–21535`、`21794–21808`、`21821–21833`、`21971–21983` 估算 | ✅ 账本 `21847–21858`、`20646` 上限 4096；游标字段引用点偏（L6） | 部分不符（L6） |
| Trae（国际版） | ✅ `22062`/`22002` | ✅ `trae-usage.js:45–105` 全链（唯一解判定、`unpriced_input_tokens`、别名映射） | ✅ `22104–22111`（`modifiedAt` 裁决）、`22159–22173` | 符合 |
| Trae CN | ✅ `trae-cn-config.js:11–16`/`35`/`37–45` | ✅ `22460–22508`（含 credit 反推证据 `22467–22474`） | ✅ `22513–22530` 断言、`22532–22595` LWW、`22626–22634` fail-closed | 符合 |

**本人抽核的 22 家**（关键点位，非全要素）：Gemini（`4734–4752` 映射、`2943–2974` 时间/桶、`4945` 闸门）、OpenCode 磁盘（`1513` 游标、`3109–3122`）、OpenCode DB（`5194–5199` UNC 快照、`5222–5227` 四分项闸门）、OpenClaw（`1710`/`2071`/`2074`/`2109`/`2114`）、Hermes（`7707–7718` SQL、`7999–8006`）、Kimi（`9540`/`9557`/`9590`/`9595`/`8080`）、Kimi Code（`9769`/`9783`/`9823`/`9828`/`9704`）、CodeBuddy（`10341–10345`、`10472–10485` 两条闸门）、Cline（`12350–12385`、`12178–12189`、`12403–12405`）、Zed（`12514–12520`/`12553–12558`/`12585–12601`/`12732`/`12742`/`12764`/`12816`）、Unsloth（`13469`/`13482`/`13634–13636`/`13650`/`13677`/`13714–13717`）、AnythingLLM（`13808`/`13856`/`14010`/`14013–14016`/`14024`/`14042`）、Goose（`14489`/`14503`/`14529`/`14639`/`14708–14713`）、Droid（`15069`/`15165`/`15210`/`15243`/`15328`/`15379`/`24839`/`14972`）、Qoder 旧（`6148–6179`/`6281`/`6338–6340`）、Qoder 新（`6513–6515`/`6540–6542`/`6630–6660`/`6718–6765`）、Cursor（`7164–7172`/`7184–7210`/`7244–7268`/`cursor-config.js:520–536`）、Devin（`devin-usage.js:44–58`/`190–201`、`14172–14181`）、Copilot OTEL（`17376–17399`/`17534–17538`/`18058–18091`）、Copilot store（`18120–18127`/`18426–18437`/`18715–18810`/`18834–18856`/`18863–18866`/`19210–19224`）、Copilot App（`19287–19294`/`19334–19355`/`19357–19379`）、Grok（`19549`/`19704–19718`/`19945–19961`/`20099–20123`/`20210–20228`/`20284–20290`）、Claude Science（`7003–7027`）、Claude 用户轮次桶（`2828–2835`/`3403–3408`/`3546`/`4165`）、Codex $0 链（`4851–4893`）。

### B.3 §5.6 风险清单重建（否定性结论独立重建）

| # | 文档断言 | 重建方法 | 裁定 |
|---|---|---|---|
| 1 | **A 类共 3 家**（gemini / grok / codex 可产 $0 行） | 逐条回源：`normalizeGeminiTokens` `4743 total = Math.max(reportedTotal, computedTotal)` + `2971` 零闸门用 `isAllZeroUsage`（`4945–4956` 将 `total_tokens` 计为非零）；`grok-usage.js:63–71` 先取上报 total、仅 `total <= 0` 拒绝，消费侧 `20217–20228` 直接落桶；`normalizeUsage` `4877` 只减缓存、`isCodexTotalOnlyResetSentinel` `4881–4893` 只拒特定 reset 形态 | **成立**（三家证据链完整；`pricing/index.js:248–256` 五家 reasoning 折叠集亦逐字核对） |
| 2 | **全零落桶 ≥5 处**（qoder 新旧 / trae-cn / claude 用户轮次 / reasonix） | 5 处逐一回源：`6639–6660`（`billable !== false` 分支显式构造全零 totals + `conversation_count: 1`）；`6148–6179`+`6281`+`6338–6340`（判空仅数值合法性、落桶无条件）；`22483–22508`（`Number.isSafeInteger(value) && value >= 0`、0 合法）+`22749–22751`（无零护栏）；`2828–2835`（user 桶只 +conversation_count）+`3546`（`pickDominantModel` 跳过 unknown）+`3403–3408`+`4165`；`17035`/`17044`/`17067–17068`（total 可为 0、会话数=requests） | **成立**（且文档已正确区分「解析器构造点」与「写入端撤回零行」两个集合） |
| 3 | **`usage_precision` 全部写入点 = 16 行 / 15 语句，无 antigravity 与 kiro** | 对 `rollout.js` 全文做 `includes("usage_precision")` 独立计数：命中 **恰好 16 行**，行号集合与文档列举（`3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286–20287/22011/22025/22113/22157`）**逐一对齐**；其中 `20286–20287` 为跨行单语句 → 15 语句成立 | **成立**（§5.5.6 与 §5.6.6 的「披露缺口」结论可维持） |
| 4 | **`billable_total_tokens` 覆盖 = 8 显式 + 13 兜底 = 21 项** | 全文独立枚举 `billable_total_tokens`（`rollout.js` 命中 34 行 + 其他模块）：显式赋值来源除文档 8 家外，**还有 `rollout.js:7025`（`normalizeClaudeScienceTokens`，`billable_total_tokens: total`）** 与 `13046`（lmstudio/unsloth，文档已单列） | **需限定**（见 M1）：8+13=21 与「45−19−2−3」的函数名口径自洽，但文档未声明该口径，且把 claude-science 的显式写入漏在清单外；「其余家在各自函数区段内没有赋值语句」的表述在跨口径阅读下会误导 |
| 5 | **时间兜底漂到「触碰时刻」共 7 家** | 逐家回源：`zed 12764`、`lmstudio 13129`（`initialStat.mtimeMs`）、`goose 14713`（`|| new Date().toISOString()`）、`droid 15210`、`grok 20214`、`craft 17223`（`if (tsMs == null) tsMs = stat.mtimeMs`）、`reasonix 17021`（`return snapshot.stat.mtimeMs`） | **成立**（7 家全部验证；`copilot` App DB 的「会话级累计」破坏面另计亦正确） |

补充核验（非否定性）：§5.6.4 云侧补记的 `scripts/ops/billable-total-tokens-migration.sql` **7 行**、`test/billable-total-tokens-migration.test.js` **9 行且不注册 `test()`**、`package.json:41 "test": "node --test test/*.test.js"` —— 三项均按 `ReadAllLines` 口径复算**完全一致**。

### B.4 子核验的复核（对分支 A/B 结论的再证伪）

分支 A 的 8 家与分支 B 的 9 组结论均经本人回源抽验，**未采信任何未复验的指控**：

- 分支 A 的「LM Studio 表格语义矛盾」本人独立复核成立（`13006 if (total <= 0) return null;` + `13040` 反推 → 只写 total 的行必然落桶成 `input=total`，不可能"丢弃"）→ 升级为 M2。
- 分支 A 的「Kiro CLI 8960/9167 行号小偏」本人复核：`8167 const kiroHome = env.KIRO_HOME || path.join(home, ".kiro");`、`9167 const rawModel = r.model_id || r.session_model_id;`、`9169 const model = canonical || "kiro-cli-agent";` → 成立，列为 L9/L10。
- 分支 A 的「Reasonix 17067–17068 措辞」复核：`17067–17068` 只控制入桶，`17069 sessionTotals[filePath] = current;` 仍推进 → 成立，列为 L11。
- 分支 B 的 5 组「部分不符」全部回源确认（L1–L6、L8），无一条被推翻。其未决项「Antigravity 模型三级顺序」本人补了一条证据：`21460–21469` 证明第三级确实存在（`const f19 = inner.find((f) => f.num === 19)?.val;`），但三级之间的**优先顺序**未逐级确认（`currentModel` 赋值链跨 `21600–21949`），**维持「部分证据、未见反证」**，不升也不降。

## C. 内部一致性

1. **计数类断言自洽**：§5.1.1 的 `45/39/2/21/19` 四组数字与 §5.2 标题「19 家」、§5.3.8 表、§5.6.4 的「21 项」互相咬合；本人逐项枚举验证无矛盾（唯一需补声明的是 §5.6.4 的口径，见 M1）。
2. **§5.1.2 ↔ 附录 A**：两个索引表对同一批 45 个函数名与定义行**零差异**（这是全文最强的一致性证据）。
3. **§4 ↔ §5**：§4.1 的「5 个转发薄壳（`16087/16100/16689/16699/16775`）」与 §5.1.2/§5.1.1 的「2 个 Like 壳 + 3 个 pi 家族注入」口径并用不冲突（前者数转发函数、后者数共享实现体），行号互不矛盾。
4. **§6 ↔ §5**：§6.3 对照表中 Claude/Codex 的读取路径、去重键、缓存字段、reasoning 处理、`conversation_count` 语义与 §5 总表第 43/44 行一致；§6.3 引 `pricing/index.js:248–256` 与 §5.6.1 前提段同源同值。
5. **内部矛盾 1 处**：§5.2.20 表 LM Studio 行「纯 total 行处置 = 丢弃」与 §5.6.1 C 类（lmstudio 属「明确规避」、由 total 反推 input 落桶）直接冲突，且该格自身两句（"丢弃" vs "由 total 反推 input"）互斥 → M2。
6. **口径歧义 1 处**：§5.6.4「8 显式 + 13 兜底 = 21 项，与 21 家数据源实体完全对齐」——"21 家"实为「45 − 19(§5.2) − 2(壳) − 3(第 6 章)」的函数名口径（Claude Science 被排除），而 §5.1 总表第 45 行的 Claude Science 属数据库/API 子集且**确有显式写入** → M1。
7. 其余跨节引用（§5.6.1 引 §5.5.6、§5.6.7 引 §5.3/§5.6 各处、§5.2.20 末段与 §5.3.6 的「7 家」呼应）均一致。

## 必须修清单

| # | 级别 | 位置 | 问题 | 修法 |
|---|---|---|---|---|
| M1 | **中**（口径 + 漏项） | §5.6.4 「显式写入的 8 家」段 | 漏 `claude-science`：`rollout.js:7003–7027 normalizeClaudeScienceTokens` 返回块含 `7025 billable_total_tokens: total,`。同时「21 家」沿用 §5.1.1 的 45−19−2−3 函数名口径但未声明，导致「其余家在各自函数区段内没有赋值语句」在跨口径阅读下不成立 | 补一句：Claude Science 亦为显式写入（`7025`），因 §5.1.1 的 21 家口径（不含第 6 章三个函数名）未计入；或把 8 改为「9 家（含 claude-science）」并注明 lmstudio/unsloth 在文本子集 |
| M2 | **中**（内部矛盾） | §5.2.20 表 LM Studio 行「纯 total 行处置」栏 | 写「丢弃（归一化器由 total 反推 input）」——与 §5.6.1 C 类（明确规避）及该格自身语义冲突；源码 `13006 if (total <= 0) return null;` + `13040 input_tokens: Math.max(0, total - completion - cacheRead - cacheWrite)` 决定只写 total 的行会落桶成 `input=total` | 改为「**落桶 → input=total**（归一化器由 total 反推，非丢弃）」 |
| L1 | 低（引文出处） | §5.3.6 DSH | 「DISJOINT…no cache subtraction, unlike Codex」原文在 `22804–22806`；`23345–23346` 是另一句「Counts are disjoint, so every column maps 1:1」 | 双引用：`22804–22806`（语义）+ `23345–23365`（映射实现） |
| L2 | 低（顺序） | §5.3.6 DSH | 模型回落链写成「`data.message.source.model` → request/header 配置 → 事件自身」；源码 `23649–23652` 实为 `source.model → data.model → headerModel` | 改为「`data.message.source.model` → 事件 `data.model` → 最近 `request/header` 配置」 |
| L3 | 低（区间） | §5.3.2 / §5.3.6 DSH | 「先扫 `contentSize` 声明做解压炸弹防护」引 `23060–23072`，该区间是帧解码器选择（`decodeDshZstdFrame`）；防护在 `22997–23006`（`declaredContentBytes` 与 limit 比较） | 引用改为 `22997–23006` + `23060–23072` 双区间 |
| L4 | 低（区间） | §5.3.6 DSH | 文件身份「inode/size/mtime」引 `23761–23793`，该区间是读取/比较；实际写入 `cursors` 形状在 `23973–23981` | 补 `23973–23981` |
| L5 | 低（区间） | §5.3.6 Command Code | 「同一记录出现在非拥有者文件时重读该文件」引 `24614–24627`，该区间是归属记账（`completeOwnedSnapshot`/`zeroKeys`）；重读判定在 `24496–24505`（`ownsSnapshot` + `readCommandCodeSessionSnapshot`） | 引用改为 `24496–24505`，`24614–24627` 留给归属记账 |
| L6 | 低（区间） | §5.3.6 Antigravity | 游标字段清单引 `20641`/`20645`，两行是常量 `ANTIGRAVITY_CURSOR_VERSION = 2` / `_EXTRACTOR_REVISION = 1`；字段实际写入在 `21093–21104`、`21353–21365` | 常量与写入点分开引用 |
| L7 | 低（归属） | §5.3.3 ZCode | 「`install-resolver.js:52–79`，含 `%APPDATA%\.zcode\...` 与 WSL 分支」——该区间只有 home 候选（`74`）与 APPDATA 候选（`79`），无 WSL 分叉；WSL 在 `rollout.js:5603–5610` 与 `install-resolver.js:44`（`resolveInstallPaths` → `wsl.resolveAllWin32Paths`） | 拆成两处引用 |
| L8 | 低（行号） | §5.3.6 / §5.3.8 pi 家族 `seenIds` 上限 | 引 `16680`，实为 `projectSeenIds` 上限；主 `seenIds` 上限在 `16656–16658` | 改 `16656–16658`，项目级另注 `16680` |
| L9 | 低（行号） | §5.2.6 Kiro CLI 路径根 | 「根为 `KIRO_HOME || ~/.kiro`（`8960`）」——根解析在 `8167`；`8960` 是 sidecar `installKey` 复用同一表达式 | 改 `8167`（旁注 `8960`） |
| L10 | 低（行号） | §5.2.6 Kiro CLI 模型兜底 | 兜底字面量在 `9169`（`canonical \|\| "kiro-cli-agent"`）；`9167` 是取值行 | 引用放宽为 `9167–9169` |
| L11 | 低（措辞 + 遗漏） | §5.3.6 Reasonix | ①「差值全 0 且请求数未增则整文件跳过（`17067`–`17068`）」——实为只跳过入桶，`17069 sessionTotals[filePath] = current;` 仍推进快照；②路径覆盖变量只列 `~/.reasonix`/`%APPDATA%\reasonix`，源码另有 `16910 TOKENTRACKER_REASONIX_HOME`、`16913 REASONIX_STATE_HOME`（win32 优先 USERPROFILE，`16921–16924`） | 措辞改「不入桶但推进快照」；补两个 env |
| L12 | 低（精确化） | §5.6.5 表 | `command-code` 的语义格写「每条 assistant 消息 +1（请求级）」，源码 `24214 conversation_count: total > 0 ? 1 : 0`——零 token 消息不计 | 限定为「每条**有 token 的** assistant 消息 +1」 |

其余「低危但可接受」项（不列入清单、供读者知悉）：§5.6.5 表「恒 1」行把 `craft` 与 goose/lmstudio/unsloth 并列（craft 实为「会话首见记 1」，同表最后一行已另列并注明注释），措辞可收紧但不算错误；§5.5.2 的 `6726` 悬空注释（与 §12 C 类补充登记一致，属已登记的既有缺陷，非本文错误）。

## 复核证据抽样

以下摘录均为本次亲自读取的源码原文（UTF-8 行号）：

**索引与骨架**
```
rollout.js:251      async function parseRolloutIncremental({
rollout.js:954      async function parseClaudeIncremental({
rollout.js:1242     async function parseGeminiIncremental({
rollout.js:24458    async function parseCommandCodeIncremental({
rollout.js:55-56    const DEFAULT_SOURCE = "codex";  const DEFAULT_MODEL = "unknown";
rollout.js:4222-4225 function bucketKey(source, model, hourStart) { ... `${safeSource}|${safeModel}|${hourStart}` }
rollout.js:4099-4110 function addTotals(target, delta) { ... target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0;
rollout.js:4173-4190 function toUtcHalfHourStart(ts) { ... const halfMinute = minutes >= 30 ? 30 : 0; ...
rollout.js:4945-4955 function isAllZeroUsage(u) { for (const k of [... "total_tokens"]) { if (Number(u[k] || 0) !== 0) return false;
```

**风险区核心证据**
```
pricing/index.js:248-253  const reasoningIncludedInOutput = row.source === "codex" || ... "acode" || ... "every-code" || ... "omo" || ... "cline";
rollout.js:4743    const total = Math.max(reportedTotal, computedTotal);
rollout.js:4877    out.input_tokens = Math.max(0, out.input_tokens - out.cached_input_tokens);
rollout.js:4881-4893 function isCodexTotalOnlyResetSentinel(lastUsage, totalUsage) { ... componentTotal(...) === 0; }
grok-usage.js:63-71 let totalTokens = nonNegative(usage.totalTokens ?? usage.total_tokens); if (totalTokens <= 0) { totalTokens = ... } if (totalTokens <= 0) return null;
rollout.js:20217-20228 const delta = { input_tokens: event.input_tokens, ... total_tokens: event.total_tokens, billable_total_tokens: event.billable_total_tokens, ... conversation_count: event.conversation_count || 1 };
rollout.js:6650-6659 } : { input_tokens: 0, ... total_tokens: 0, billable_total_tokens: 0, total_cost_usd: 0, conversation_count: 1, };
rollout.js:2833    userBucket.totals.conversation_count += 1;
rollout.js:3546    if (model === DEFAULT_MODEL) continue;
rollout.js:7025    billable_total_tokens: total,        // normalizeClaudeScienceTokens（7003–7027）
rollout.js:22507   billable_total_tokens: total,        // trae-cn buildTraeCnSessionTotals
rollout.js:24212   billable_total_tokens: total,        // command-code
rollout.js:13046   billable_total_tokens: total,        // 共用归一化器（lmstudio/unsloth）
```

**M2 的证据**（LM Studio 表格矛盾）
```
rollout.js:13006   if (total <= 0) return null;
rollout.js:13040   input_tokens: Math.max(0, total - completion - cacheRead - cacheWrite),
rollout.js:13047-13048  total_cost_usd: 0,  conversation_count: 1,
```

**低危引用校准证据**
```
rollout.js:22804-22806  // DISJOINT — `inputTokens` is uncached input only ... (no cache subtraction, unlike Codex).
rollout.js:23345-23346  // Convert a harness `TokenUsage` ... Counts are disjoint, so every column maps 1:1;
rollout.js:23649-23652  const model = normalizeDshModelName( (source && parseDshJsonString(findDshJsonProperty(source, "model"))) || parseDshJsonString(findDshJsonProperty(data, "model")), ) || headerModel;
rollout.js:22997-23006  const contentSizeBytes = ...; declaredContentBytes = readUnsignedLE(contentSizeBytes); ... throw new Error(`DeepSeek Harness decompressed session log exceeds ${limit} bytes`);
rollout.js:23060-23072  // Decode one already-isolated frame. Prefer Node's built-in zstd ... async function decodeDshZstdFrame(frameBytes) {...}
rollout.js:23973-23981  fileState[filePath] = { inode: snapshot.inode, size: snapshot.size, mtimeMs: snapshot.mtimeMs, sessionId, lastSeq: parsed.maxSeq, contributions: nextContributions, updatedAt, };
rollout.js:24496-24505  const ownsSnapshot = index?.completeOwnedSnapshot && ... ; const snapshot = await readCommandCodeSessionSnapshot(filePath, ownsSnapshot ? state.files[filePath] : null, ...);
rollout.js:24614-24627  for (const [filePath, index] of Object.entries(nextFileIndex)) { ... index.completeOwnedSnapshot = ownsAllRecords; ... index.zeroKeys = zeroKeys; ... }
rollout.js:20641    const ANTIGRAVITY_CURSOR_VERSION = 2;
rollout.js:20645    const ANTIGRAVITY_EXTRACTOR_REVISION = 1;
rollout.js:21093-21104  cursorVersion: ANTIGRAVITY_CURSOR_VERSION, ... lastLine: Number(prev.lastLine || 0), ... usageSource: prev.usageSource || (dbIdentity ? "sqlite" : "estimated"), ... contributions: contributionState.contributions,
install-resolver.js:52-83  // ZCode stores its CLI DB under the user's home dir ... path.join(zcodeHome, "cli", "db", "db.sqlite"), ... [path.join(env.APPDATA.trim(), ".zcode", "cli", "db", "db.sqlite")] ...   // 无 WSL 分支
rollout.js:5603-5610  function resolveZcodeInstallPaths(...) { ... }   // WSL 分派在 install-resolver.js:44 resolveAllWin32Paths
rollout.js:16656-16658  const seenArr = Array.from(seenIds); const cappedSeen = seenArr.length > 10_000 ? seenArr.slice(seenArr.length - 10_000) : seenArr;
rollout.js:16680    projectSeenIds: Array.from(projectSeenIds).slice(-10_000),
rollout.js:8167     const kiroHome = env.KIRO_HOME || path.join(home, ".kiro");
rollout.js:9167-9169 const rawModel = r.model_id || r.session_model_id; const canonical = canonicalizeKiroCliModelId(rawModel); const model = canonical || "kiro-cli-agent";
rollout.js:17067-17069  if ((tokenDelta > 0 || delta.requests > 0) && addReasonixDelta(...)) eventsAggregated++;  sessionTotals[filePath] = current;
rollout.js:24214    conversation_count: total > 0 ? 1 : 0,
```

**云侧补记复算**
```
scripts/ops/billable-total-tokens-migration.sql   ReadAllLines = 7 行（只加两列 + 一个部分索引，不写值）
test/billable-total-tokens-migration.test.js      ReadAllLines = 9 行（仅两条 assert.ok，不注册 test()）
package.json:41    "test": "node --test test/*.test.js",
```

**统计口径原始输出**：`%TEMP%\r72_refs3.txt`（1527 条引用逐条状态）、`%TEMP%\r72_extract3.js`（抽取脚本）、`%TEMP%\r72_defs.js`（45 定义行重建，输出「不一致数: 0 / 附录A 不一致: 0」）。
