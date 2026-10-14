# 复核报告 R1-F：S5 定价与成本 / S6 消费端与输出面

样本仓库（只读）：`C:\Users\admin\AppData\Local\Temp\tt-research`
HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（`git describe` = `v1.3.14`，已复核）
被复核分片：`S5-pricing-and-cost.md`（1397 行）、`S6-consumers-output.md`（1001 行）
复核口径：全文件用 `read` 工具 / `[System.IO.File]::ReadAllLines(...,UTF8)`；JSON、key 差集、块哈希、行数用自写 Node 只读探针（未修改样本仓、未执行样本仓任何脚本）。

## 结论

**两份分片可用，需修订**：核心高风险断言（B9/B10/B16、§10 手工演算、S6 的 B14/B15、27 端点清单、上传字段清单）经独立重算**全部成立**，且 B9 的 9 键差集逐字一致；错误集中在**行号偶发漂移（3 处）与「计数/行数口径」类断言（8 处）**，无一条推翻分片结论。

## 抽样统计

- 抽验断言数：**101**（S5 58 条 + S6 43 条），其中 **E=11、I=7、U=3**
- 报送 E/I/U 之外，另附「计数类断言重算结果」独立一节（14 项，全部逐项重算）
- 实际打开过的源码文件（含行区间读取与 grep 逐条核对）：
  `src/lib/pricing/index.js`（全文 378 行）、`src/lib/pricing/matcher.js`（全文 439 行）、
  `src/lib/pricing/litellm-fetcher.js`、`src/lib/pricing/curated-overrides.json`、`src/lib/pricing/seed-snapshot.json`、
  `test/edge-pricing-parity.test.js`（全文 420 行）、`test/pricing.test.js`、`test/gpt-6-astra-pricing.test.js`、
  `test/model-breakdown.test.js`、`test/grok-parser.test.js`、`test/cache-token-accounting.test.js`、
  `test/account-source-parity.test.js`、`test/community-stats-cache.test.js`、`test/badge-svg-render.test.js`、`test/status-json-light.test.js`、
  `src/lib/codex-rollout-parser.js`、`src/lib/codex-service-tier.js`、`src/lib/local-api.js`、`src/lib/session-analytics.js`、
  `src/lib/outcomes-engine.js`、`src/lib/subscriptions.js`、`src/lib/usage-limits.js`、`src/lib/wrapped-aggregator.js`、
  `src/lib/upload-queue.js`、`src/lib/upload-throttle.js`、`src/lib/telemetry.js`、`src/lib/tracker-paths.js`、`src/lib/source-metadata.js`、
  `src/commands/serve.js`、`src/commands/status.js`、`src/commands/wrapped.js`、`src/commands/sessions.js`、`src/commands/sync.js`、
  `scripts/build-pricing-seed.cjs`、`scripts/audit-token-correctness.cjs`、`package.json`、`CLAUDE.md`、`docs/PRIVACY.md`、
  `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts`、`-ingest.ts`、`dashboard/vite.config.js`、
  `dashboard/src/lib/api.ts`、`dashboard/src/lib/model-breakdown.ts`、`dashboard/src/lib/pet-linux-host.js`、
  `dashboard/src/pages/DashboardPage.jsx`、`dashboard/src/pages/LimitsPage.jsx`、`dashboard/src/hooks/use-activity-heatmap.ts`、
  `dashboard/src/ui/dashboard/components/CostAnalysisModal.jsx`、`dashboard/src/ui/share/{use-share-card-data.ts,build-share-card-data.ts}`、
  `TokenTrackerBar/.../APIClient.swift`、`TokenTrackerBar/.../Utilities/Constants.swift`、`TokenTrackerWin/ServerManager.cs`、
  `TokenTrackerLinux/gnome-extension/.../extension.js`

---

## 错误（E）—— 必须修

- [E1] **S5 §7.2**：断言「`MODEL_PRICING` 键数 **183**」/ 实际 `MODEL_PRICING` 只有 **119** 键；183 = 119 + 64（同一抽取块内另一个常量 `IFLYTEK_MAAS_MODEL_PRICING` 的键数）。
  证据：`dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142`（声明）～`:332`（表结束，逐键重算 119）；`:339`-`:404`（iFlytek 64 键）。
  修正建议：把表头改为「块内定价键总数 183（MODEL_PRICING 119 + IFLYTEK_MAAS_MODEL_PRICING 64）」。

- [E2] **S5 §1.5**：断言注释「`total_tokens stays the sum of the base columns whether or not they are present`」位于 `index.js:284-285` / 实际 **283-284**（grep 单行命中 `283:   // annotations, never addends — total_tokens stays the sum of the base columns`）。
  修正建议：改为 `index.js:283-284`。

- [E3] **S5 §1.7**：断言 `index.js:288-295` 是注释原文「They are annotations, never addends」/ 实际该短语**全仓仅一处**：`src/lib/pricing/index.js:283`；:288-289 是「The parser records only usage from requests whose cache-inclusive input exceeded 272K…」。
  修正建议：改为 `index.js:281-283`（并把 §1.5 与 §1.7 的同一引用统一）。

- [E4] **S5 §1.3**：断言 `billable_total_tokens` 的使用点见 `src/lib/local-api.js:615` / 实际 `:615` 是 `input_tokens: 0,`，`billable_total_tokens` 在 `:614`。
  修正建议：改为 `:614`。

- [E5] **S5 §9.1**：断言「`test/pricing.test.js`，**1391** 行」/ 实际 LF=**1392**（`ReadAllLines` 亦 1392）。
  修正建议：改为 1392。

- [E6] **S5 §8.3**：断言「`src/lib/subscriptions.js`（**507** 行）」/ 实际 **508** 行。
  修正建议：改为 508。

- [E7] **S6 §0 第 7 条 / §4.5**：断言「全仓 grep `savings|amount_saved|saved_amount|total_saved` **只命中** `dashboard/src/pages/CloudPage.jsx:95` 的 `cloudAnnualSavings`」/ 实际另有 4 类命中：
  `dashboard/src/content/i18n/zh/dashboard.json:650` 与 `zh-TW/dashboard.json:709`（`"cloud.price.savings"`）、`dashboard/src/content/copy.csv:1699`、`DESIGN.md:17`（"savings badges"）、`scripts/cloud-sandbox/hosted-archive-acceptance.cjs:377`、`:450`（`bill_savings_claimed: false` / `production_bill_savings_claimed: false`）。
  修正建议：把「只命中」改为「命中 9 处，全部与用量统计无关（另见 S5 §8.3 的「与无关文案」表述）」——S5 的写法是对的，S6 需按 S5 口径改。

- [E8] **S6 §6.3**：断言 `dashboard/edge-patches/`「共 **29** 个文件」/ 实际 **28** 个（全部为 `.ts`；`Get-ChildItem -File` 亦 28）。
  修正建议：改为 28。

- [E9] **S6 §6.3 的文件行数逐个与任何单一口径都不符**（该节未声明口径）：实测（总行数 / 非空行数）——
  `-account-summary.ts` 954/914（分片 911）、`-account-daily.ts` 894/863（分片 860）、`-account-heatmap.ts` 391/363（分片 362）、`-account-model-breakdown.ts` 956/917（分片 915）、`-leaderboard-refresh.ts` 1449/1393（分片 1387）、`-leaderboard-profile.ts` 1074/1033（分片 1031）、`-public-visibility.ts` 210/198（分片 194）、`-device-token-issue.ts` 183/167（分片 164）、`-device-rename.ts` 164/152（分片 151）。
  修正建议：统一用 `read` 工具口径（总行数）并逐条刷新；若沿用 `Measure-Object -Line`（=非空行数），须在 §6.3 显式声明并修正偏差（最大 6 行）。

- [E10] **S6 §1.1 命令表**：断言 `src/commands/init.js`（**2254** 行）、`src/commands/sync.js`（**6900** 行）/ 实际 `init.js` 总 2427 行（非空 2255）、`sync.js` 总 7251 行（非空 6905）。
  修正建议：同 E9 统一口径（此二处按非空口径也差 1 行与 5 行）。

- [E11] **S6 §9 行数口径自相矛盾**：同节声明「取自 `Measure-Object -Line`」（=非空行数），但两行给的是总行数：
  `test/status-json-light.test.js（96 行）`（总 96 / 非空 87）、`test/share-card-data.test.js（138 行）`（总 138 / 非空 128）。其余各行（如 `status.test.js 659`=非空 659、`badge-svg-render.test.js 132`=非空 132）符合非空口径。
  修正建议：统一为总行数口径，或在表头写清各行口径。

---

## 不完整（I）—— 建议补

- [I1] **S5 §7.4**：「与 `curated-overrides.json` 的对照只有 4 处」不完全——同一个 `test/edge-pricing-parity.test.js` 内另有 2 处对照：`:175` 断言 `curatedPricing.source_alias?.acode === undefined`，`:207-216` 用 `curatedPricing.source_exact.acode.xsparkx2 / xopglm53` 做 deepEqual。结论（**无任何测试断言 edge ⊇ curated.exact 或逐键价格相等**）不变。

- [I2] **S5 §7.4**：两个方向的键数应量化——实测「在 edge `MODEL_PRICING` 但不在 `curated.exact`」为 **44** 键（分片只说「大量」）；iFlytek 的 64 条并不在 `MODEL_PRICING` 内（分片已在括注中说明，但句式易读成同表）。

- [I3] **S5 §2.5**：「全仓库对 `ensurePricingLoaded` 的调用只有两处」——在 `src/` 内确为 2 处（`src/commands/serve.js:127`、`src/lib/local-api.js:3340`），但全仓 grep（27 命中）另有 20 行测试调用（`test/pricing.test.js` 16 处、`test/local-api-queue-cache.test.js:149/:220`、`test/grok-parser.test.js:301`、`test/omo-parser.test.js:229`）。建议改成「生产代码只有两处」。

- [I4] **S5 §6.3**：「价格表只存在于后端与 edge 两处，前端没有任何价格副本」的作用域实际是 `dashboard/src/`（该处 grep 确为 0 命中）。`dashboard/vite.config.js`（dev server 插件，位于 `dashboard/` 而非 `dashboard/src/`）另有：`:508-514` 直接 require 同一 `src/lib/pricing`，`:1052-1057` 调 `computeRowCost`，`:1067` 内嵌一份展示价 `rates_per_million_usd {input:1.750000, cached_input:0.175000, output:14.000000}`。建议把作用域写明为「生产路径 + `dashboard/src/`」。

- [I5] **S6 §0 第 2 条 / B14**：「Dashboard 的所有用量端点按请求侧 `tz`/`tz_offset_minutes` 切天」成立，但**本地 API 无 tz 参数时回落 UTC**（`src/lib/local-api.js:512-519` → `rowDayKey` 的 `hs.slice(0,10)`，`:213-225`）。前端确实一直传 tz（`dashboard/src/lib/api.ts:244-249`、`:279` 等），因此「同一份数据两个最忙一天」在真实前端路径成立；建议补一句「差异的前提是客户端传 tz；裸 API 调用者不传时与 wrapped 同为 UTC」。

- [I6] **S6 §10 第 7 条**（原标【未能确认】）可关闭：`dashboard/vite.config.js:508-514` 明写「Per-model pricing — delegated to `src/lib/pricing/`」，dev 环境**不复制价格表**。但它对「按模型聚合后的 totals 只计一次价」（`:1052-1057`），与 #653 在 `local-api.js:2634-2640` 修掉的「先汇总再计价」是同一种写法（仅 dev mock，不影响线上）。

- [I7] **S5 §1.1**：「上游再导出到 `src/lib/local-api.js:60` 与 `:3530`」——`:60` 是从 pricing 模块的 **import**（解构 `computeRowCost,`），`:3530` 才是 `module.exports` 里的再导出。建议措辞分开。

---

## 无法验证（U）

- [U1] **S5 §11.2 第 9 条**：`dashboard/edge-patches/` 的 5 份函数**线上部署修订**是否等于仓库修订——仓库内无部署产物/版本指针（仅 `CLAUDE.md:42` 提示人工部署步骤），需平台侧证据。
- [U2] **S5 §11.2 第 8 条**：`curated-overrides.json` 各 `note` 引用的第三方官方定价页与表内数字是否一致——本仓库运行在虚构时间线（2026 年的 `claude-opus-5-5`、`gpt-6-astra`、`deepseek-v4` 等），**外部真实页面不可能作为该时间线的事实源**，联网核对无意义；只能核对表内自洽性（已做）。
- [U3] **S6 §2.4.5 的推论**「Windows 托盘在本地模式下连续天数显示 0」（`TokenTrackerWin/UsagePoller.cs:314-328` 注释称 streak 是 server-computed，而本地热力图恒 0）——需运行 Windows 壳实测；本次仅确认「本地热力图 `streak_days` 恒 0」与「前端用自己的 `computeActiveStreakDays` 覆盖」两侧源码为真。

---

## 行号漂移统计

- 抽样中**明确打开并逐字对照过的行号引用：88 处**；行号内容与分片一致 **85 处**，漂移 **3 处**（正确率 96.6%）：`index.js:284-285 → 283-284`、`index.js:288-295 → 282-283`、`local-api.js:615 → 614`。
- 另有 4 处**数值型计数**偏差（非行号）：`MODEL_PRICING 183 → 119`、`pricing.test.js 1391 → 1392`、`subscriptions.js 507 → 508`、`edge-patches 29 → 28` 文件。
- 未发现整段累计漂移：`src/lib/pricing/index.js` / `matcher.js` / `local-api.js` / `tokentracker-leaderboard-refresh.ts` 的其余全部引用（含 matcher 解析步骤表 11 行、§6.2 聚合表 9 行、S6 §2.10 端点总表 27 行）**行号完全命中**。

## 复核到的行号抽样（证明真的打开过）

- `src/lib/pricing/index.js:31` → `const OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000;`（`grep` 全仓该常量仅 5 处：本行、`:375` 导出、`codex-rollout-parser.js:51`、`:733`、`session-analytics.js:245` 注释）
- `src/lib/pricing/index.js:227` → `function computeRowCost(row) {`
- `src/lib/pricing/index.js:257-264` → `const baseCost = ((row.input_tokens||0)*(pricing.input||0) + … ) / 1_000_000;`
- `src/lib/pricing/index.js:248-253` → `const reasoningIncludedInOutput = row.source === "codex" || … "cline";`
- `src/lib/pricing/index.js:283` → `// annotations, never addends — total_tokens stays the sum of the base columns`
- `src/lib/pricing/index.js:363` → `const MODEL_PRICING = curatedOverrides.exact;`
- `src/lib/pricing/matcher.js:3-12` → `// Resolve order:`（0–7 步）
- `src/lib/pricing/matcher.js:291` → `if (/(?:^|\/)qwen3[.-]8-flash(?:-\d{4}-\d{2}-\d{2})?$/.test(lower.trim()) && curated.exact?.["qwen3.8-flash"]) {`
- `src/lib/pricing/matcher.js:335` → `if (!curated.exact[ref]) continue;`
- `src/lib/pricing/matcher.js:398-414` → `function convertLitellmEntry(entry) {`（4 字段 ×1e6 + roundToTenDecimals）
- `src/lib/pricing/litellm-fetcher.js:10-13` → LITELLM_PRICING_URL / `DEFAULT_TTL_MS = 24h` / `DEFAULT_FETCH_TIMEOUT_MS = 10_000`
- `src/lib/pricing/litellm-fetcher.js:160-163` → `// 4. Bundled seed snapshot` → `return { data: seed, source: "seed-snapshot" };`
- `test/edge-pricing-parity.test.js:76-85` → `test("MODEL_PRICING + getModelPricing are byte-identical across all 5 edge files", …)`（只比 edge↔edge）
- `test/edge-pricing-parity.test.js:163-174` → iFlytek 块 `assert.deepEqual(…, curatedPricing.source_exact.acode, …)`
- `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:273` → `"grok-build": { input: 1.25, output: 2.50, cache_read: 0.20 },`
- `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:480-481` → `if (lower.includes("grok-4.5")) return MODEL_PRICING["cursor-grok-4.5"];` / `if (lower.includes("grok-build")) return MODEL_PRICING["grok-build"];`
- `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:569-613` → edge `computeRowCost`（无 `long_context_*` / `priority_*` 分支）
- `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:1181` → `agg.estimated_cost_usd += computeRowCost(row);`；`:1349` → `estimated_cost_usd: Math.round(agg.estimated_cost_usd * 100) / 100,`
- `dashboard/edge-patches/tokentracker-ingest.ts:188-189` → `billable_total_tokens: b.billable_total_tokens || 0,` / `total_cost_usd: Number(b.total_cost_usd) || 0,`
- `dashboard/edge-patches/tokentracker-ingest.ts:203-211` → 批内按 `hour_start|source|model` 取 `total_tokens` 最大
- `src/lib/local-api.js:2634-2640` → `// Price each row, exactly as aggregateByDay does for the hero (#653). …` + `ma.totals.total_cost_usd = Number(...) + computeRowCost(row);`
- `src/lib/local-api.js:2588` → `streak_days: 0,`（同块 `:2587` 服务端算 `active_days`）
- `src/lib/local-api.js:213-225` → `function rowDayKey(row, timeZoneContext) { … return hs.slice(0, 10); }`
- `src/lib/local-api.js:2415` / `:2648` / `:2652` → `total_cost_usd: totalCost.toFixed(6)` / 逐模型 `toFixed(6)` / source 汇总
- `src/lib/wrapped-aggregator.js:37-40` → `function rowDay(row) { … return row.hour_start.slice(0, 10); }`；`:148-152` → `new Date(iso + "T00:00:00Z")` 步进
- `dashboard/src/hooks/use-activity-heatmap.ts:206` / `:284` → `streak_days: computeActiveStreakDays({ dailyRows: rows, to: range.to }),`
- `dashboard/src/pages/DashboardPage.jsx:822-823` → `const serverActive = Number(heatmap?.active_days); if (Number.isFinite(serverActive)) return serverActive;`
- `src/commands/status.js:200-206` → `const queuePath = path.join(trackerDir, "queue.jsonl"); …`；全文 grep `total_tokens|"cost"|total_cost` = **0 命中**
- `src/lib/upload-throttle.js:1-10` → `intervalMs: 30 * 60_000, batchSize: 300, maxBatchesSmall: 2, maxBatchesLarge: 4, backoffInitialMs: 60_000,`
- `src/lib/upload-queue.js:11-16` → `row.source.trim().toLowerCase() !== "trae-cn"` + `total = input+output+cached+cache_creation` 校验
- `src/commands/sync.js:4367-4374` → `body: JSON.stringify({ upload_id, hourly: result.buckets, ...(… account_session_states …) })`
- `src/lib/telemetry.js:75-82` → `{ machine_hash, app_version, platform, shell }`
- `TokenTrackerWin/ServerManager.cs:476` → `private const int PreferredPort = 17680;`

---

## 计数类断言重算结果（全部独立重算）

| 分片断言 | 重算方法 | 重算结果 | 判定 |
|---|---|---|---|
| `curated-overrides.json` 顶层 5 键 / `exact` 84 / `alias` 1 / `source_exact.acode` 64 / `fuzzy` 47（S5 §2.3） | `JSON.parse` + `Object.keys` | 5（`_meta`,`exact`,`alias`,`source_exact`,`fuzzy`）/ 84 / 1（`{"auto":"composer-1"}`）/ 64 / 47 | ✅ |
| `_meta` 内容（S5 §2.4 引 `:5`、`:6`） | 逐行读 | `note`/`units`/`deepseek_v4_pricing_note`/`iflytek_maas_pricing_note` 四键，单位 `usd_per_million_tokens` | ✅（分片只引 2 条，无冲突） |
| `exact` 缺 `cache_write` 41 条、缺 `cache_read`/`input`/`output` 0 条（S5 §5.3） | 逐键 `in` 判定 | 41 / 0 / 0 / 0（41 条键名已导出，含 `k3-256k`、`grok-build`、全部 GLM/Kimi/Composer/free 条目） | ✅ |
| `source_exact.acode` 64 条全带 4 字段（S5 §5.2） | 逐键判定 | 缺 `cache_write` = 0；`xopkimik27code`=0.9/3.75/0.9/0.9、`xsparkx2`=0.42×4 | ✅ |
| 带 `priority_multiplier` 的 exact 条目 3 条、值均为 2（S5 §1.7） | 逐键判定 | `gpt-6-astra`=2、`gpt-6.1-sol`=2、`gpt-6-sol`=2（无其它） | ✅ |
| `seed-snapshot.json` 256869 字节 / 顶层 2285 = 1 `_meta` + 2284 模型 / `kept_models`=2284（S5 §2.2） | `Buffer.byteLength` + `Object.keys` | 256869 / 2285 / 2284 / 2284；`_meta.generated_at = 2026-05-26T11:12:29.761Z` | ✅ |
| seed 字段直方图 2274 / 2273 / **566** / **188**（S5 §2.2、§5.3） | 逐条计数 | `input_cost_per_token` 2274、`output_cost_per_token` 2273、`cache_read_input_token_cost` **566**、`cache_creation_input_token_cost` **188**；无第 5 个字段 | ✅ |
| seed 首 5 / 末 5 键（S5 §2.2） | 逐键 | 与分片逐字一致（`sample_spec` … `bedrock/us-gov-west-1/anthropic.claude-haiku-4-5-20251001-v1:0`） | ✅ |
| 5 份 edge 块 394 行、sha256[:16] = `37840dc53f7b6e2f` 全同（S5 §7.2） | 与 `test` 相同正则抽取 + `crypto` | 5/5 文件皆 394 行、哈希全同 `37840dc53f7b6e2f` | ✅ |
| edge `MODEL_PRICING` 键数（S5 §7.2 写 183） | 花括号配平解析对象字面量 | **119**（+ iFlytek 64 = 183，见 E1） | ❌ 见 E1 |
| 「curated 有、edge 没有」差集 9 个键（S5 §7.4 / B9） | 双向 `Set` 差集 | **恰好 9 个，逐字一致**：`claude-opus-5.5`、`claude-sonnet-5.5`、`cline-gateway-models`、`claude-opus-4-8`、`claude-haiku-4-5`、`antigravity-gpt-oss-120b`、`grok-4.5-build`、`grok-build-free`、`grok-4.5-build-free` | ✅ |
| 「edge 有、curated 没有」键数（分片未给数） | 同上 | **44**（含 `claude-opus-4-6`、`gpt-5.4`、`mimo-v2.5*` 等） | 供补数（见 I2） |
| 定价相关 `TOKENTRACKER_*` 环境变量 = 0（S5 §2.5、§11.3-11） | `src/` 全目录 grep `TOKENTRACKER_[A-Z0-9_]+` | 252 命中（工具截断于 250）；逐类清点后与定价相关 **0**；最接近的是 `TOKENTRACKER_OPENCODE_GO_LOCAL_ESTIMATE`（`opencode-go-limits.js:20`、`:51`，额度估算非计价）与 `TOKENTRACKER_*_BILLING_BASE_URL`（配额/账单端点，不改 `computeRowCost`） | ✅ |
| 无 models.dev / 无独立 OpenRouter 价源（S5 §11.3-10） | 全仓 grep `models\.dev|modelsdev` | **0 命中**；`openrouter` 仅作为 LiteLLM 键前缀与注释出现 | ✅ |
| 无图片/音频计费字段（S5 §4.5） | 全仓 grep `audio_token|image_token|image_generation` | **0 命中** | ✅ |
| 无 `savings / amount_saved / saved_amount / total_saved` 营销指标（S6 §0-7、§4.5） | 全仓 grep | `amount_saved|saved_amount|total_saved` = 0；`savings` = **9 处**（分片称 1 处，见 E7） | ❌ 见 E7 |
| `batch` 命中 37 处且全部非定价（S5 §4.2） | `src/` grep `batch` | **恰 37 命中**；逐条为上传/同步批处理与注释（`upload-throttle.js:5`、`sync.js:4315` 等），无一进入 `computeRowCost` | ✅ |
| 本地 API `/functions/tokentracker-*` 端点 27 个（S6 §0-3、§2.10） | grep 路由定义 + 逐个行号比对 | **27 个**，行号与 §2.10 总表 27 行**全部一致** | ✅ |
| `dashboard/edge-patches` 文件数（S6 §6.3 写 29） | `Get-ChildItem -File` | **28**（全 `.ts`） | ❌ 见 E8 |
| `usage-limits` 解构 18 家、`data` 17 家 + `fetched_at` = 18 顶层键（S6 §10-8） | 逐行清点 `usage-limits.js:4055` 与 `:4406-4440` | 解构 18 个变量 ✅；`data` 顶层 **18** 键（`fetched_at` + claude/codex/cursor/kimi/gemini/kiro/antigravity/copilot/grok/zcode/opencodeGo/commandCode/devin/qoder/qoderCn/codingPlan/agentPlan = 17 家），`claudeServiceStatus` 未入 `data` ✅ | ✅ |
| 行数类：`index.js` 378、`matcher.js` 439、`curated` 775、`local-api.js` 3543、`session-analytics.js` 2934、`leaderboard-refresh.ts` 1449、`build-pricing-seed.cjs` 71、`audit-token-correctness.cjs` 176（S5 §0/§11.4） | LF 计数 + `ReadAllLines` | 全部**逐字命中** | ✅ |
| 行数类：`pricing.test.js` 1391、`subscriptions.js` 507（S5）；`init.js` 2254、`sync.js` 6900（S6）；`share-card-data.test.js` 138、`status-json-light.test.js` 96（S6 §9） | 同上（并存「总行数 / 非空行数」两种口径） | 1392 / 508 / 2427(2255) / 7251(6905) / 138(128) / 96(87) | ❌ 见 E5/E6/E10/E11 |

### B9 / B10 / B16 三项终判

- **B9（成立，且比「9 键」更严重的一点被确认）**：`test/edge-pricing-parity.test.js` 第一层（`:76-85`）**只做 edge↔edge 字节比对**；全文件对 `curated-overrides.json` 的对照仅 `source_exact.acode` 全表 deepEqual（`:163-191`）、`source_alias?.acode`（`:175`）与 `xsparkx2/xopglm53` 取值（`:207-216`），**没有任何 `edge ⊇ curated.exact` 或逐键等价断言**。差集 9 键已独立重算并逐字一致。
  真实分歧确认：`grok-build-free` 本地 = `{0,0,0,0}`（`curated.exact`，matcher 步骤 1 先命中），云端无该键 → 落入 `:481 if (lower.includes("grok-build")) return MODEL_PRICING["grok-build"]` = `{input:1.25, output:2.50, cache_read:0.20}`；`grok-4.5-build-free` 本地 = 0，云端落入 `:480` → `cursor-grok-4.5` = `{2,6,0.5,0}`。仅在 `total_cost_usd` 未为正时显现（`grok` 属 `SOURCES_WITH_AUTHORITATIVE_COST`，两端都会优先采信正上报价）。影响面 = 本地 dashboard/`computeRowCost` 计 $0，云端 leaderboard/profile/model-breakdown 计非 $0——同一批桶在两端金额不同。
- **B10（成立）**：`src/lib/pricing/index.js:31` 定义、`:375` 导出，`index.js` 内**无任何读取点**（全文 378 行逐行读过）；真正生效的是 `src/lib/codex-rollout-parser.js:51`（注释 `:49-50` 自述「Match src/lib/pricing/index.js」）并在 `:733` 用于 `Number(rawRequestUsage?.input_tokens || 0) > OPENAI_LONG_CONTEXT_INPUT_THRESHOLD`。`src/lib/session-analytics.js:245` 的注释把 `pricing/index.js` 称作权威——实际只是注释引用。
- **B16（成立，含一处作用域修正）**：`dashboard/src/` 下 `computeRowCost` / `getModelPricing` / `MODEL_PRICING` / `pricePerMTok` 全部 **0 命中**；金额只在后端逐行计价后相加（`local-api.js:285`、`:2640`、`:3381`，`session-analytics.js:272/:279/:302`，`outcomes-engine.js:207`，edge `:1181`），前端仅做求和与格式化（`model-breakdown.ts:116-117`、`sessions-insights.js:19-30`、`DashboardPage.jsx:1256-1257`）。`local-api.js:2634-2640` 的「必须先逐行计价再加总」注释与实现逐字属实。唯一例外是 mock/dev 通道（见 I4）。

### §10 手工演算独立复算（B 组第 4 项重点）

用 `seed-snapshot.json` / `curated-overrides.json` 原文复算，**四段演算全部一致**：

- 价格原文核对：`claude-opus-4-6` = `5e-6/2.5e-5/5e-7/6.25e-6` → `5/25/0.5/6.25` ✅；`gpt-5.4` = `2.5e-6/1.5e-5/2.5e-7`（无 cache_creation）→ `2.5/15/0.25/—` ✅；`gemini-3-pro-preview` = `2e-6/1.2e-5/2e-7` → `2/12/0.2/—` ✅；三者在 `curated.exact` 中**确实缺席**（JSON.stringify 中为 undefined）。
- 演算 A：10000×5 + 5000×25 + 200000×0.5 + 1500×6.25 = 284,375 → **0.284375** ✅
- 演算 B：10000×2.5 + 5000×15 + 200000×0.25 + 1500×0 = 150,000 → **0.150000** ✅（codex 折叠推理不改变 `reasoning_output_tokens=0` 的结果）
- 演算 C：120,000/1e6 = **0.120000**；+800×12=9,600 → 129,600 → **0.129600** ✅
- 演算 D 差值：800×15/1e6 = **0.012** → 0.162 ✅
- §10.3 三锚点：`0.15+0.47+0.016+0.2 = 0.836` ✅；`10+50+1+12.5+0 = 73.5` ✅；`100×3+20×15+600×0.3+50×3.75 = 967.5 → 0.0009675` ✅（`claude-sonnet-4-6` seed 原文 `3e-6/1.5e-5/3e-7/3.75e-6` 与 edge `:443` 的 `3/15/0.3/3.75` 同值）
- 附加核对：`test/gpt-6-astra-pricing.test.js:44-48` 的 73.5 / 73.5 / 98.5（差 25 = 500,000 × $50 / 1e6）✅

### B14 / B15 终判

- **B14（成立）**：`src/lib/wrapped-aggregator.js:37-53`（`slice(0,10)` 切天、`slice(0,4)` 切年、`/T(\d{2}):/` 取小时）、`:66-67`（JSDoc 明确「calendar year (UTC)」）、`:148-152`（UTC 步进）、`:170`（峰值小时 UTC）；对照 `src/lib/local-api.js:213-225`（按 `timeZoneContext` 切天）+ `dashboard/src/lib/api.ts:244-249`（前端一直传 `tz`）。两侧源码为真，**「最忙一天 / 活跃天数 / 峰值小时」在跨时区用户处确实会给出两个不同结果**（前提见 I5）。
- **B15（成立）**：`src/lib/local-api.js:2588` `streak_days: 0,` 硬编码（同响应 `:2587` 服务端算真实 `active_days`）；前端 `dashboard/src/hooks/use-activity-heatmap.ts:206/:220/:284/:295` 调 `computeActiveStreakDays(...)` 覆盖。`dashboard/src/lib/pet-linux-host.js:198` 也直接读 `r.body?.streak_days`（本地模式拿到 0）；Windows 托盘行为待实测（U3）。

### S5 其他重点复核结论（摘）

- `computeRowCost` 三态返回（常数 0 / `row.total_cost_usd` / 五桶算式）与 `grok`、`cline` 短路条件 `Number.isFinite && > 0` 逐字属实（`index.js:227-241`）。
- 折叠集合 5 个 source（`codex/acode/every-code/omo/cline`，`index.js:248-253`）与 token 审计侧 2 个（`codex/omo`，`audit-token-correctness.cjs:34-40`）**确为两套口径**（无测试绑定）✅。
- 长上下文/优先级溢价：白名单 `:267-269`、子集夹紧 `:277-296`、`:318-351`、最终 `baseCost + longContextPremium + priorityPremium`（`:354`）逐字属实；`:308` 无 Fast 时提前返回 ✅。
- DeepSeek 峰谷：模型名单 `:34-39`、`:199-207`、`:186-197`、四价 ×0.5（`:215-221`）、`acode` 豁免（`:213`）✅。
- 云端与本地能力差：edge `computeRowCost`（`:569-613`）确无长上下文/Fast 分支；edge `PI_SUBSCRIPTION_SOURCES` 只有 `pi-github-copilot`/`pi-copilot`（`:575`，`:626` 另有一处引用）vs 本地 4 成员 ✅。
- S5 §9.3 审计脚本：176 行、只审计 token 恒等式、无 `pricing/cost/computeRowCost` ✅。

### S6 其他重点复核结论（摘）

- 「`status` 不是用量报表」为**强证据**：`src/commands/status.js` 全文 grep `total_tokens|"cost"|total_cost` = 0 命中；`--json` 顶层字段与 `:1040-1059` 逐字一致。
- 上传字段：客户端 `sync.js:4367-4374`（`{upload_id, hourly, account_session_states?}`）✅；ingest 映射 `:176-194` 确实多出 `billable_total_tokens`（`:188`）与 `total_cost_usd`（`:189`），而 `docs/PRIVACY.md:102` 的字段枚举（10 项）未含二者 → **隐私披露差口成立**；`account_session_states` 字段（`source/session_id/model/bucket_start/snapshot_verified_at` + 6 类 token，`:153-160`）且 source 必须为 `trae-cn`（`:139-147`）+ 客户端同样校验（`upload-queue.js:11-16`）→ **含 `session_id` 成立**；设备名 = `os.hostname()` 截断 128（`local-api.js:29-41`，`:3137-3143`）→ **主机名成立**。
- 节流参数表（`upload-throttle.js:1-10`、`:57`、`:104-116`、`:123`）逐行一致 ✅。
- 端口矩阵：macOS `Constants.swift:4-5` = 7680；Windows `ServerManager.cs:476` = 17680（注释 `:471-475` 说明 OAuth 白名单 + 避开 DoSvc 7680）；Linux `extension.js:19` = 17680；CLI `serve.js:14`/`:21` = 7680/7681 ✅。
- 前端二次聚合清单（`activeDays` 兜底、`computeActiveStreakDays`、`trend-stats.js`、`sessions-insights.js`、`pet-linux-host.js`）均实存；`api.ts` 双路由/紧凑热力图/缓存 TTL 逐条命中 ✅。
