# 复核报告 R4-2：主文档 §7–§13 与附录 A/B（末轮验收复核）

- 复核对象：`docs/research/2026-10-14-tokentracker-usage-statistics.md`（4300 行）的 **§7（2149–2512）、§8（2513–2912）、§9（2913–3550）、§10（3551–3976）、§11（3978–4018）、§12（4020–4113）、§13（4115–4209）、附录 A（4212–4269）、附录 B（4271–4300）**
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14），**全程只读**，未改样本仓与主文档任何一个字节（`git status --porcelain` 空）
- 行号口径：`[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`；**未使用**裸 `Get-Content`
- 被复核的修正单位：FIX-R3-B（§7–§9 采纳 10 条）、FIX-R3-C（§10+ 采纳 11 条）、主线跨区收尾
- 职责：**证伪**。只报差异，不润色不补写

---

## 结论

**第三轮修正基本改对了：修正点验收 32 条中 31 条成立，1 条在改写时引入新的事实错误；全新抽样 44 条通过 43 条（97.7%，另 1 条为表述级），硬错 0，必须修 2 条 + 低危建议 2 条。**

1. **任务 A 点名的每一项都真改对了**（逐条回源，见表 A）：
   - §8.4 末段派生量已改为 `src/lib/outcomes-engine.js:154`（`quality_per_dollar`）/ `:156`（`effective_tokens`）/ `:157`（`effective_cost_usd`）——三行逐字命中，原越界的 `docs/quality-per-dollar.md:154` 已消失；
   - §9.1.1 已改为 `src/cli.js:15-19`——注释 `:15`、`if (!command)` `:16`、`cmdServe` `:17`、`return` `:18`、`}` `:19` 逐行命中；
   - §7.5 #11 已改为「tmp+rename 整队列原子重写」——`:3945` `ensureDir`、`:3946` `tmpPath`、`:3947` `writeFile(tmpPath, …)`、`:3948` `rename(tmpPath, queuePath)` 逐字命中；`:3887`/`:3905` 确为「保留既有行」的两个 push；
   - §7.5 #9/#10 的 zcode 已从「未确认」升级为已证，六个行号（`:4940`/`:4884`/`:4929`/`:4930`/`:4935`/`:4936`）与两个语义（主队列 `retainRetractions: true` `:4953`、项目队列不保留 `:4955`）、`removed > 0` 才重置偏移（`:4979-4984`）全部命中；
   - 守卫表述已按 `mergeMaxTotals` 四处 + `setMaxTotal` 另五处分开计数——`git grep` 复核：`mergeMaxTotals` 全 `src/` 5 命中 = 4 调用（`:5848/:5852/:5856/:5860`）+ 1 定义（`:6574`）；`setMaxTotal` 6 命中 = 5 调用（`:6492/:6512/:6540/:6559/:6579`）+ 1 定义（`:6585`），与文档完全一致；
   - §8.4 的 DeepSeek 断言已改为 11 个纯时间点（`:917-921` 五条 + `:940/:945/:950/:955/:960/:965` 六条），逐行打开确认；
   - §11 云端归档包已改为 `migrations/20261005120000_cloud-usage-archive.sql:72`（`CHECK(row_count > 0 AND row_count <= 10000)`）/ `:213`、`:309`（`pg_column_size(v_payload)>8388608` 抛 `exceeds pack budget`）+ `scripts/cloud-usage-archive.cjs`——**目录与文件均真实存在**（SQL 44852 B / 625 行、runner 10307 B），且 `10000` 与 `8388608` 在 SQL 中各自只有这三个位点，为真；同行的 `:186` `cloud_prepare_usage_archive`、`:221` `cloud_commit_usage_archive` 也逐字命中；
   - §10.2#14 / §12 C13 的未消费键已由 2 改 3（`opencode_plugin`/`openclaw_session_plugin`/`grok`）——`status.js:1024-1036` 实传 11 键、`passive-mode.js` 实消费 8 键（`:100`/`:112`/`:125`/`:136`/`:148`/`:159`/`:169`/`:180`），差集恰为那 3 个；`:88-90` 的 JSDoc 双向漂移描述也逐字成立；
   - §13.2 的三份 R2 工作量已补全并与报告自报值**逐字一致**（R2-1 A40/B40/C13；R2-2 A40/B54/C14——我按 `^\|\s*[ABC]\d+` 重数 R2-2 表格：A=40、B=54（最大编号 B54）、C=14；R2-3 A49（S4 18/S5 17/S6 14）+B46+C10）；
   - §13.5 的四单元规模数字与四份 R3 报告自报一致（R3-3 抽样表重数 = 112 行；R3-2 必须修 17 条确为 P0×4/P1×7/P2×6）；§13.6 的 3 条反驳与 R3-1 必须修 2/10/11 一一对应，其中反驳 3（`setMaxTotal` 调用点 `:6540`）经独立验证**成立**（`:6540` 正是 `mainUsageHourTotalsFromQueueRows` 内的调用）；
   - 主线收尾项已落实：`cmdSync` 统一为 3194（`:516` 定义、`:3709` 闭括号、`:3711` 起 `parseArgs`，3709−516+1=3194）；热力图时间口径在 §12 C22 已展开为 `:2540`（UTC 网格终点 `new Date(\`${todayStr}T00:00:00Z\`)`）+ `:2563`（UTC 单元格键 `cursor.toISOString().slice(0, 10)`）+ `:2537`（`aggregateByDay` 调用）/`:2545`（`byDay` Map 构建）——四行内容全部与断言相符。

2. **唯一由修正引入的新错误**（表 A 第 19 条 → 必须修 1）：§7.5 守卫表的「项目键集合守卫」行写「**否**（在 `:5823-5880` 之外，判据也不同）」。实测该守卫 `:5841-5846` **就在** `:5823`（`if (projectRepairEnabled) {`）与 `:5880`（`}`）之间。R3-3 只要求限定 `mergeMaxTotals` 的计数措辞，这一句是新写进去的（旧版为「否 | —」类的空白判据），改错了位置判定。

3. **另有一处内部矛盾**（必须修 2）：§7.5 #11 表格写 `migrateLegacyDeepseekHarnessSource`「为每个旧键**追加显式零行**」，而 §7.5 末「四种安全模式」段写它「**既不追加撤回行**、也不删任何既有行」。源码 `:3929-3943` 确实 `output.push({ source: "deepseek", …, input_tokens: 0, …, conversation_count: 0 })`——**为零值行**。表格对、模式段错（除了「不删任何既有行」正确）。

4. **未发现**：幻觉（无一条断言找不到源码位点）、证据等级被升级（zcode 等原 `【未能确认】` 处现均已被源码直证，其余 `【未能确认】` 抽查仍为真未证）、方向相反的断言。

5. 全新抽样 44 条中唯一非 ✓ 的是 B22（§9.1.2「四个状态文件」的概括口径，表述级，见 S1）；M1/M2 来自修正点验收与相邻段落（§7.5 内部），抽样本身无新增硬错。

---

## A. 修正点验收（32 条）

判定：`✓` 与源码逐字/逐值一致；`✗` 与源码不符（新引入）；`△` 行号对但表述需修。

| # | 文档位置（主文档行号） | 现写法要点 | 回源实测（UTF-8 行号 + 摘录） | 判定 |
| --- | --- | --- | --- | --- |
| A1 | §8.4 末（2785） | 派生量见 `src/lib/outcomes-engine.js:154`/`:156-157` | `:154 const quality_per_dollar = cost_usd > 0 && outcomes > 0 ? accepted / cost_usd : null;`；`:156 const effective_tokens = …`；`:157 const effective_cost_usd = …` | ✓ |
| A2 | §9.1.1（2925） | 引文 `（src/cli.js:15-19）` | `:15 // No args → launch dashboard`、`:16 if (!command) {`、`:17 await cmdServe(argv);`、`:18 return;`、`:19 }` | ✓ |
| A3 | §7.5 #11（2364） | 「整队列 tmp+rename 原子重写（`:3945-3948`：临时文件 `writeFile` + `rename` 覆盖…）」 | `:3946 const tmpPath = \`${queuePath}.tmp.${process.pid}.${Date.now()}\`;`、`:3947 await fs.writeFile(tmpPath, …)`、`:3948 await fs.rename(tmpPath, queuePath);` | ✓ |
| A4 | §7.5 #11 | 「既有行全部原样保留 `:3887`/`:3905`」 | `:3887 output.push(line);`（JSON.parse 失败分支）、`:3905 output.push(serialized);` | ✓ |
| A5 | §7.5 #11 | 「重写后再追加规范行（`:3925-3927`）」 | `:3925 for (const key of legacyKeys.keys()) {`、`:3926 const canonical = latestExplicitCanonicalRows.get(key) \|\| latestMigratedRows.get(key);`、`:3927 if (canonical) output.push(canonical);` | ✓ |
| A6 | §7.5 #11 | 「与显式零行（`:3929-3943`）」 | `:3929-3934 for (…legacyKeys.values()) { output.push(JSON.stringify({ source: "deepseek", model, hour_start, input_tokens: 0,` … `:3940 billable_total_tokens: 0, :3941 conversation_count: 0,` | ✓（但模式段写法错，见 M2） |
| A7 | §7.5 #11 | 「偏移自 0 重放（`:3950-3962`）」 | `:3958 state.offset = 0;`、`:3960 state.note = "reset_after_deepseek_harness_source_migration_2026_08";`、`:3962 await fs.writeFile(queueStatePath, …)` | ✓ |
| A8 | §7.5 #9（2362） | `repairZcodeUsageMigration`（`:4940`）调 `dropZcodeQueueRows`（`:4884`） | `:4940 async function repairZcodeUsageMigration({`、`:4884 async function dropZcodeQueueRows(filePath, { retainRetractions = false } = {}) {` | ✓ |
| A9 | §7.5 #9 | 「先 `backupExistingFile`（`:4929`），再临时文件（`:4930`）`writeFile`（`:4935`）后 `rename`（`:4936`）」 | 四行逐字命中（`:4929 await backupExistingFile(filePath);` … `:4936 await fs.rename(tmp, filePath);`） | ✓ |
| A10 | §7.5 #9 | 「主队列带 `{ retainRetractions: true }`（`:4953`）、项目队列不保留（`:4955`）」 | `:4953 const mainRepair = await dropZcodeQueueRows(queuePath, { retainRetractions: true });`、`:4955 ? await dropZcodeQueueRows(projectQueuePath)` | ✓ |
| A11 | §7.5 #9 | 「仅 `removed > 0` 时重置上传偏移（`:4979-4984`）」 | `:4979 if (mainRepair.removed > 0) {`、`:4982 if (projectRepair.removed > 0) {` | ✓ |
| A12 | §7.5 #10（2363） | 签名参数 `dropZcodeQueueRows(filePath, { retainRetractions = false } = {})`（`:4884`） | 逐字命中（同 A8 行） | ✓ |
| A13 | §7.7 未闭合项（2501-2509） | zcode 写盘条目已删（现 7 项） | 现表为 fs.js 锁 / codexHashes / 跨根并键 / copilot 桶粒度 / 端点字段 / source-metadata / billable 迁移测试——**无 zcode** | ✓ |
| A14 | §7.5 守卫（2373） | `mergeMaxTotals`（`:6574-6583`）、`setMaxTotal`（`:6585-6589`） | `:6574 function mergeMaxTotals(...maps) {` … `:6583 }`；`:6585 function setMaxTotal(map, key, total) {` … `:6589 }` | ✓ |
| A15 | §7.5 守卫 | 「`mergeMaxTotals` 的调用点恰四处：`:5848`、`:5852`、`:5856`、`:5860`」 | `git grep`：全 `src/` 仅 5 命中，调用为 `:5848 existingProjectTotals`、`:5852 rebuiltProjectTotals`、`:5856 existingMainHourTotals`、`:5860 rebuiltMainHourTotals` | ✓ |
| A16 | §7.5 守卫 | 「`setMaxTotal` 另有 5 处族内调用（`:6492`、`:6512`、`:6540`、`:6559`、`:6579`）」 | `git grep`：调用 5 处逐条命中（`:6579` 在 `mergeMaxTotals` 内，其余 4 处在 `projectUsageTotalsFromQueuePath`/`FromState`/`mainUsageHourTotalsFromQueueRows`/`FromState`） | ✓ |
| A17 | §7.5 守卫 | 「该 `if` 块为 `:5823-5880`」 | `:5823 if (projectRepairEnabled) {`、`:5880 }` | ✓ |
| A18 | §7.5 守卫表 | 「总量下降守卫 `:5865-5879`」 | `:5865 for (const [key, existingTotal] of existingProjectTotals.entries()) {` … `:5876 …without a matching main-bucket repair — skipping to avoid data loss`、`:5878 return false;` | ✓ |
| A19 | §7.5 守卫表 | 项目键集合守卫栏「**否**（在 `:5823-5880` 之外，判据也不同）」 | `:5841 if (missingProjectKeys.length > 0) {` … `:5845 return false;`、`:5846 }`——**位于 5823–5880 之内** | **✗** |
| A20 | §8.4 DeepSeek（2749） | 「11 个纯时间点断言 `:917-921`、`:940`、`:945`、`:950`、`:955`、`:960`、`:965`」 | `test/pricing.test.js` 逐行：`:917/:918/:919/:920/:921`（5 条 `hour_start` 字面量）+ `:940/:945/:950/:955/:960/:965`（6 条）= 11 条；另 `:923`/`:927`/`:969-972` 三条 `pricing_tier` 断言 | ✓ |
| A21 | §11 云端归档包（4013） | `migrations/20261005120000_cloud-usage-archive.sql:72`（`CHECK(row_count > 0 AND row_count <= 10000)`）与 `:213`/`:309`（`pg_column_size(v_payload) > 8388608`）+ `scripts/cloud-usage-archive.cjs` | 目录/文件均存在（SQL 625 行；runner 10307 B）；`:72 row_count integer NOT NULL CHECK(row_count > 0 AND row_count <= 10000),`；`:213 …>8388608 THEN RAISE EXCEPTION 'Archive day exceeds the bounded pack budget';`；`:309 …'Merged cold day exceeds pack budget';` | ✓ |
| A22 | §11 云端归档包 | 「`cloud_prepare_usage_archive`（同 SQL `:186`）与 `cloud_commit_usage_archive`（`:221`）」 | `:186 CREATE FUNCTION public.cloud_prepare_usage_archive(p_user_id uuid,p_device_id uuid,p_day date)`、`:221 CREATE FUNCTION public.cloud_commit_usage_archive(p_generation uuid)` | ✓ |
| A23 | §11 sessions 行（4007） | 已删「（`:95`）」误引，改为「该文件无对应分页常量，`:95` 的 `PROJECT_USAGE_MAX_ENTRIES = 10` 只服务 project-usage-summary」 | `local-api.js:95 const PROJECT_USAGE_MAX_ENTRIES = 10;`（陈述为真） | ✓ |
| A24 | §10.2#14（3933）/ §12 C13（4080） | 「3 个键在 passive 判定中永不使用：`opencode_plugin`、`openclaw_session_plugin`、`grok`」 | `status.js:1024-1036` 传 11 键；`passive-mode.js` 消费点 `:100 claude`、`:112 gemini`、`:125 codex_notify ?? codex`、`:136 acode_notify ?? acode`、`:148 every_code_notify ?? every_code`、`:159 codebuddy`、`:169 workbuddy`、`:180 ["workbuddy-ai"]` = 8 键，差集恰 3 键 | ✓ |
| A25 | 同上 | 「`passive-mode.js:88-90` 的 JSDoc 自述形状与实现双向不一致」 | `:89-90 (already collected by status.js); shape: { claude, gemini, codex, every_code, opencode, openclaw, codebuddy, workbuddy, grok }`——`opencode/openclaw/grok` 不被读、被读的 `acode_notify/workbuddy-ai` 不在其中 | ✓ |
| A26 | §13.2 R2-1 行（4137） | A 40 / B 40 / C 13 | R2-1 报告结论第 3 段逐字：「A 组复查 40 条中 35 ✅ / 4 ⚠️ / 1 ❌；B 组独立抽样 40 条；C 组 13 处」 | ✓ |
| A27 | §13.2 R2-2 行（4138） | A 40（S3a 8 + S3b 15 + 收紧/结案 17）/ B 54 / C 14 | 重数 R2-2 表格行：`^\|\s*A\d+` = 40、`^\|\s*B\d+` = 54（max B54）、`^\|\s*C\d+` = 14（max C14）；A 节结构 `### A-2 S3b 修正记录（15 条）` | ✓ |
| A28 | §13.2 R2-3 行（4139） | A 49（S4 18 / S5 17 / S6 14）+ B 46 + C 10 | R2-3 自报逐字：「A 组 49 条（S4 18 / S5 17 / S6 14）、B 组 46 条（S4 18 / S5 16 / S6 12）、C 组 10 处」；节标题 `### A-S5（17 条）`、`### A-S6（14 条）`、`## B. 独立新抽样（46 条…）` | ✓ |
| A29 | §13.5 表（4165-4168） | R3-1 70 簇/182 点/必须修 7 处（清单 12 条）；R3-2 72 条/88.9%（64/72）/17 条（P0×4/P1×7/P2×6）；R3-3 112 条/92.0%（103/112）/12 项；R3-4 112 条/94.6%（106/112）/6+I 5 | 四份报告自报逐字一致；R3-3 抽样表重数 = 112；R3-2 必须修分段确为 P0 4 条（#1-#4）/P1 7 条（#5-#11）/P2 6 条（#12-#17）；R3-1 清单 12 条（含 #10 `:16080`、#11 `:6540`） | ✓ |
| A30 | §13.6 反驳台账（4200-4206） | 3 条反驳：45 家拆解、骨架 D `:16080` 不可复现、`setMaxTotal` 点 `:6540` 不成立 | 反驳 2：全文搜 `1608[04]` 仅 `:16084`（§4 代码块与 §5 表）与 §13.6 记录行本身 → 正文无 `16080`；反驳 3：`:6540 setMaxTotal(modelTotals, \`${row.source}\|${model}\|${row.hour_start}\`, …)` 确为调用 | ✓ |
| A31 | §12 C22（4089） | 热力图「`:2540`、`:2563`」+ `aggregateByDay`（`:2537`、`:2545`） | `:2540 const end = new Date(\`${todayStr}T00:00:00Z\`);`；`:2563 const day = cursor.toISOString().slice(0, 10);`；`:2537 const daily = await aggregateByDay(rows, timeZoneContext);`；`:2545 const byDay = new Map(daily.map((d) => [d.day, d]));` | ✓ |
| A32 | 附录 B `usage_precision`（4283） | 16 行、按语句 15 条、`:20286-20287` 跨两行；15 项行号清单 | 全文件 `usage_precision` 命中 **16 行**：3325, 3341, 3403, 3422, 6542, 6648, 6725, 6731, 6755, 6760, 20286, 20287, 22011, 22025, 22113, 22157——与清单**逐行一致** | ✓ |

---

## B. 全新抽样（44 条，避开 R1–R3 已核点位）

判定同 A。所有行为本轮亲自打开。

**§7 聚合与时间语义（16 条）**

| # | 引用 | 文档断言要点 | 实测摘录 | 判定 |
| --- | --- | --- | --- | --- |
| B1 | `src/lib/rollout.js:4254-4264` | source 小写+trim；model 只 trim 保留大小写 | `:4256 const trimmed = value.trim().toLowerCase();` / `:4262 const trimmed = value.trim();` | ✓ |
| B2 | `src/lib/rollout.js:3403-3408` | usage_precision 与输出模型在调用点拼接、指纹未变即跳过 | `:3404-3406 const key = usagePrecision ? \`${totalsKey(...)}\|${usagePrecision}\` : totalsKey(...)`、`:3407 outputKey`、`:3408 if (unknownBucket.queuedKey === outputKey) continue;` | ✓ |
| B3 | `src/lib/rollout.js:4155-4171` | totalsKey 拼九个数值列；priority 子集在内部 | `:4157-4165` 九列（input/cached/cache_creation/output/reasoning/total/billable/cost/conversation_count）、`:4169 PRIORITY_SUBSET_FIELDS.map(...)`、`:4170` 有 >0 才追加 `\|p:` | ✓ |
| B4 | `src/lib/rollout.js:3272-3288` | alignedModel 分支为旧模型补零行 | `:3273 if (alignedModel && !group.buckets.has(alignedModel)) {` + `:3279-3287` 全零字段 push | ✓ |
| B5 | `src/lib/rollout.js:19990`、`:20016-20032` | retractStaleSourceQueueRows 只追加零值行 | `:19990 async function retractStaleSourceQueueRows(queuePath, source, keepKeys) {`；`:20018-20020` 判定、`:20031 await fs.appendFile(queuePath, \`${lines.join("\n")}\n\`, "utf8");` | ✓ |
| B6 | `src/lib/local-api.js:512-519` | tz 优先、tz_offset_minutes 兜底 | `:513 const tz = String(url.searchParams.get("tz") \|\| "").trim();`、`:517 offsetMinutes: Number.isFinite(rawOffset) ? Math.trunc(rawOffset) : null,` | ✓ |
| B7 | `src/lib/local-api.js:585-592` | 末级回退是本机时区（本地 getter） | `:586-591 year: dt.getFullYear(), month: dt.getMonth() + 1, day: dt.getDate(), hour: dt.getHours()…` | ✓ |
| B8 | `src/lib/local-api.js:606`、`:608`、`:609` | 小时键按当地小时、24 整点键 | `:606 const parts = getZonedParts(new Date(row.hour_start), timeZoneContext);`、`:608 if (formatPartsDayKey(parts) !== dayKey) continue;`、`:609 const hourKey = \`${dayKey}T${String(parts.hour).padStart(2, "0")}:00:00\`;` | ✓ |
| B9 | `src/lib/local-api.js:2364`、`:2390-2397` | 日键闭区间过滤；collectDays 按 todayStr 回推 | `:2364 const daily = allDaily.filter((d) => d.day >= from && d.day <= to);`；`:2393 const ds = shiftDay(todayStr, -i);`、`:2394 const dd = allDaily.find((x) => x.day === ds);` | ✓ |
| B10 | `src/lib/local-api.js:482-483` | night_owl 按请求时区小时、`parts.hour < 6` | `:482 const parts = getZonedParts(new Date(row.hour_start), timeZoneContext \|\| {});`、`:483 if (!parts \|\| parts.hour >= 6) continue;` | ✓ |
| B11 | `src/lib/local-api.js:213-225` | rowDayKey 缺省 `hs.slice(0,10)`（`:224`） | `:216-219` 守卫、`:220 getZonedParts`、`:224 return hs.slice(0, 10);` | ✓ |
| B12 | `src/lib/async-queue-reader.js:8-10`、`:30-35` | 签名 `dev:ino:size:mtimeNs:ctimeNs`；在已打开 fd 上 stat | `:9 return \`${stat.dev}:${stat.ino}:${stat.size}:${stat.mtimeNs}:${stat.ctimeNs}\`;`；`:31-32` 注释、`:33 const stat = await file.stat({ bigint: true });`、`:35 if (cached?.signature === nextSignature) return cached.rows;` | ✓ |
| B13 | `src/lib/async-queue-reader.js:40-47` | 续读三条件（inode、变大、首部与 256B 锚点一致） | `:40 if (cached && cached.identity === identity && size > cached.size && cached.consumedBytes > 0) {`、`:41-43` head/anchor 比对、`:44 offset = cached.consumedBytes;` | ✓ |
| B14 | `src/lib/async-queue-reader.js:91`、`:94`、`:98-103`、`:109-114` | tail 不进已提交映射；LRU maxEntries；ENOENT 清缓存；单飞 pending | `:91 const rows = tail.size ? Array.from(new Map([...seen, ...tail]).values()) : …`；`:94 while (cache.size > maxEntries) …`；`:98-100 ENOENT → 清缓存返回 []`；`:110 if (pending.has(queuePath)) return pending.get(queuePath);` | ✓ |
| B15 | `src/lib/session-analytics.js:2343-2354`、`:2356-2362`、`:2364-2383` | 会话日键三级（IANA→offset→UTC）；区间相交；桶级重算 + lifetime 另存 | `:2347-2350` `Intl.DateTimeFormat("en-CA")`；`:2353 return new Date(date.getTime() + (…offsetMinutes * 60_000 : 0)).toISOString().slice(0, 10);`；`:2359-2360` 相交判定；`:2368` 闭区间；`:2381-2382 cost_usd: 0, lifetime_total_tokens: row.total_tokens, lifetime_cost_usd: row.cost_usd` | ✓ |
| B16 | `src/lib/heatmap-compact.js:26-31`、`:52`、`:70-71`、`:74-83`、`:91-97`、`:108-109` | UTC 日推进；缺 model_names 整包拒绝；last-wins；四档 level；billable 复制 total；streak_days 原样透传 | `:28-30` `new Date(\`${dayKey}T00:00:00Z\`)` + `setUTCDate`；`:52 if (!Array.isArray(payload.model_names)) return payload;`；`:71 byDay.set(String(day), …)`；`:79-82 r <= 0.25/0.5/0.75`；`:94 billable_total_tokens: total,`；`:109 streak_days: payload.streak_days,` | ✓ |

**§8 计价（3 条）**

| # | 引用 | 文档断言要点 | 实测摘录 | 判定 |
| --- | --- | --- | --- | --- |
| B17 | `src/lib/codex-rollout-parser.js:51`、`:733` | 阈值常量与判定 | `:51 const OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000;`、`:733 Number(rawRequestUsage?.input_tokens \|\| 0) > OPENAI_LONG_CONTEXT_INPUT_THRESHOLD;` | ✓ |
| B18 | `src/lib/codex-rollout-parser.js:732-741` | 五个长上下文子集列只对 >272K 请求累加 | `:734 if (isLongContext) {` + `:736-740` 五列 `long_context_*` | ✓ |
| B19 | `src/lib/codex-rollout-parser.js:742-762` | priority × 长上下文交集列显式记录 | `:742-745` 注释（multiplies…instead of replacing）、`:754 if (isLongContext) {` + `:755-760` 五列 `priority_long_context_*` | ✓ |

**§9 消费端（14 条）**

| # | 引用 | 文档断言要点 | 实测摘录 | 判定 |
| --- | --- | --- | --- | --- |
| B20 | `src/lib/upload-queue.js:5-6`、`:45-60`、`:62-64` | BACKLOG/CHUNK；两类去重键 + 未知控制记录不压缩；持锁注释 | `:5 const BACKLOG_BYTES = 1_000_000;`、`:6 CHUNK_BYTES = 64 * 1024`；`:53 key: JSON.stringify(["session", …])`、`:59 key: JSON.stringify(["hourly", source, model, row.hour_start])`、`:56` 带 `kind` 直接 `return null`；`:62-63 // Caller holds sync.lock. Only rewrite pending snapshots; …byte-for-byte unchanged` | ✓ |
| B21 | `src/lib/upload-throttle.js:2-9`、`:46-47`、`:57`、`:72-77`、`:104-116`、`:123` | 30 min/60 s/1 MB；2/4 批；blockedUntil；服务端下发取更晚者；Retry-After 优先；step 10/记录 20 | `:2 intervalMs: 30 * 60_000`、`:4 backlogBytes: 1_000_000`、`:5 batchSize: 300`、`:6-7 maxBatchesSmall 2 / maxBatchesLarge 4`；`:46 const blockedUntilMs = Math.max(s.nextAllowedAtMs \|\| 0, s.backoffUntilMs \|\| 0);`；`:57 maxBatches = pending >= cfg.backlogBytes ? …`；`:77 const nextAllowedAtMs = Math.max(serverNextAllowedAtMs, nowMs + intervalMs + jitter);`；`:107 backoffMs = Math.max(cfg.backoffInitialMs, retryAfterMs);`、`:109 Math.min(10, …)`；`:123 backoffStep: Math.min(20, …)` | ✓ |
| B22 | `src/commands/status.js:200-206` | 「读取四个状态文件」（列举 queue.jsonl / queue.state.json / cursors.json / upload.throttle.json） | 区间内实际声明 7 个路径：`:203 notify.signal`、`:204 openclaw.signal`、`:205 sync.throttle` 亦在其中；另 `:282 autoRetryPath`、`:285 syncSkipPath` 于函数体读取 | △ |
| B23 | `src/commands/status.js:278-288`、`:341-347`、`:1050` | pendingBytes = 队列字节−偏移；时间线三键；last_parse | `:288 const pendingBytes = Math.max(0, queueSize - (queueState.offset \|\| 0));`；`:346 nextUpload` / `:347 backoffUntil`；`:1050 last_parse: cursors?.updatedAt \|\| null,` | ✓ |
| B24 | `src/lib/wrapped-aggregator.js:29-35`、`:37-53` | billable 优先回落；UTC 切天/切年/切小时 | `:30-34 rowTokens` 三级回落；`:39 row.hour_start.slice(0, 10)`；`:45 Number(d.slice(0, 4))`；`:51 const m = /T(\d{2}):/.exec(row.hour_start);` | ✓ |
| B25 | `src/lib/wrapped-aggregator.js:148-152`、`:182-186`、`:212-218`、`:226-234` | 连续天数 UTC 步进；份额同源；totals 五键；formatCompact | `:149-151` `new Date(iso + "T00:00:00Z")` + `setUTCDate`；`:185 share: totalTokens > 0 ? tokens / totalTokens : 0,`；`:212-217` 五键；`:229-232 T/B/M/K` | ✓ |
| B26 | `src/lib/local-api.js:92-97` | 三个常量（10/16/256） | `:95 PROJECT_USAGE_MAX_ENTRIES = 10;`、`:96 MAX_TIME_ZONE_CACHE_ENTRIES = 16;`、`:97 AGGREGATION_BATCH_ROWS = 256;` | ✓ |
| B27 | `src/lib/local-api.js:115-117` | 时区上下文缓存键含价格修订 | `:116 return \`${timeZone \|\| ""}\|${Number.isFinite(offsetMinutes) ? offsetMinutes : ""}\|p${getPricingRevision()}\`;` | ✓ |
| B28 | `src/lib/local-api.js:264` | 每 256 行让出事件循环 | `:264 if (++processed % AGGREGATION_BATCH_ROWS === 0) await yieldToRequests();` | ✓ |
| B29 | `src/lib/local-api.js:2547-2557` | 服务端 level 四档 0.25/0.5/0.75 | 同 `heatmap-compact.js:79-82`，`:2553-2556` 逐行 | ✓ |
| B30 | `src/lib/local-api.js:2634-2642` | 逐行计价注释（#653）+ 价格修订重入保护 | `:2634 // Price each row, exactly as aggregateByDay does for the hero (#653).`、`:2638-2639 Summing the base columns first and pricing once silently dropped every premium`、`:2642 if (pricingRevision !== getPricingRevision()) return handleLocalApi(req, res, url);` | ✓ |
| B31 | `src/lib/local-api.js:2787-2794`、`:2785` | entries 的 token 为字符串；按 billable 降序 | `:2789 total_tokens: String(entry.total_tokens),`、`:2790 billable_total_tokens: String(entry.billable_total_tokens),`；`:2785 .sort((a, b) => b.billable_total_tokens - a.billable_total_tokens)` | ✓ |
| B32 | `src/lib/local-api.js:2495-2510`、`:2516-2521` | sessions 端点 LOCAL ONLY + no-store/nosniff + 错误体不含路径 | `:2498-2499 It is intentionally served only from the local API and never proxied to the cloud account view.`；`:2509-2510` 两头；`:2517-2518 Node fs errors embed the absolute path… Keep that out of the HTTP body`；`:2520 json(res, { available: false, error: "Session browser failed" }, 500);` | ✓ |
| B33 | `dashboard/src/lib/api.ts:244-252` | 前端显式传 tz/tz_offset_minutes | `:244 function buildTimeZoneParams({ timeZone, tzOffsetMinutes } = {})`、`:247 if (tz) params.tz = tz;`、`:249 params.tz_offset_minutes = String(Math.trunc(tzOffsetMinutes));` | ✓ |

**§10–§13 与附录（5 条）**

| # | 引用 | 文档断言要点 | 实测摘录 | 判定 |
| --- | --- | --- | --- | --- |
| B34 | `src/lib/fs.js:178-204` | `.reclaim` 链迭代清扫（issue #431） | `:181-182 (issue #431). Recursion cannot clean the chain`、`:186 async function sweepAbandonedReclaimGuards(lockPath) {`、`:200-201` 由深到浅 removeAbandonedLockFile | ✓ |
| B35 | `src/lib/rollout.js:16041-16050`、`:17232-17243` | seenIds 截断 1 万；Craft 注释与代码冲突 | `:16045 seenArr.length > 10_000 ? seenArr.slice(seenArr.length - 10_000) : seenArr;`；`:17233-17234 NOTE: this differs from omp/Claude which count one-per-assistant-message`、`:17243 conversation_count: prev.total === 0 ? 1 : 0,` | ✓ |
| B36 | `src/lib/rollout.js:2291`、`:2682`、`:2793`、`:1949`、`:2652`、`:2765`、`:1194`、`:488-504` | A1 的三处 `endOffset = st.size`、OpenClaw 边界函数、committedEndOffset 两处、Claude 游标写点、三态重置 | 八处逐一命中（`:2291`/`:2682`/`:2793` 均为 `const endOffset = st.size;`；`:1949 async function openclawOffsetEndsAtLineBoundary(fileHandle, offset) {`；`:2652`/`:2765 endOffset: committedEndOffset,`；`:1194 offset: result.endOffset,`；`:492-501 truncated/rebuildingCodexBaseline/startOffset`） | ✓ |
| B37 | `src/lib/cursor-store.js:21`、`:22`、`:77-86`、`:1404-1406` | 16 MB 阈值 / 24 h 重扫 / 阈值判定 / 到期计算 | `:21 DEFAULT_ACTIVATION_BYTES = 16 * 1024 * 1024`、`:22 PROJECT_ABSENT_CONTEXT_RESCAN_MS = 24 * 60 * 60 * 1000`；`:81 legacySize >= activationBytes`；`:1406 const expiresAt = checkedAtMs + PROJECT_ABSENT_CONTEXT_RESCAN_MS;` | ✓ |
| B38 | `src/commands/sync.js:6271-6292`、`:6294-6314`、`:6316-6336` | §10.1⑦ 三函数区间（I1/I2 修正后） | `:6271 async function readCodexFileSnapshot(filePath) {`；`:6294 async function validateCodexRebuildFileSnapshots(rebuiltFiles, expectedSnapshots) {`；`:6316 function isCodexHistoryCovered(cursors, rolloutFiles) {` … `:6335 return true;`、`:6336 }` | ✓ |

**计数/否定性断言（6 条）**

| # | 引用 | 文档断言要点 | 实测摘录 | 判定 |
| --- | --- | --- | --- | --- |
| B39 | `git grep appendFile -- src` | 主队列 append 写点 ≥6 处（3483/20031/22594 + 5126/5207/5281），另 4114 fallback、3536 project | 命中 `rollout.js:3483/3536/20031/22594`、`sync.js:4114/5126/5207/5281`（另 2 处为 init.js/skills-manager 的日志与非用量写点），逐点与文档一致 | ✓ |
| B40 | `git grep mergeMaxTotals` | 调用点恰 4 处 | 5 命中 = 4 调用 + 1 定义 | ✓ |
| B41 | `git grep setMaxTotal` | 族内另 5 处 | 6 命中 = 5 调用 + 1 定义 | ✓ |
| B42 | `rollout.js` `usage_precision` 计数 | 16 行 | 逐行扫描得 16 行，行号集合与附录 B 清单完全相等 | ✓ |
| B43 | `test/pricing.test.js:891-983` | 11 个纯时间点断言 | 见 A20 | ✓ |
| B44 | `sync.js:516`/`:3709`/`:3711` | cmdSync 3194 行 | `:516 async function cmdSync(argv, context = {}) {`、`:3709 }`、`:3711 function parseArgs(argv) {`（3709−516+1 = 3194） | ✓ |

**抽样命中率**：44 条中 43 条 ✓、1 条 △（B22），**命中率 97.7%**；若把 B22 计入「行号命中但表述需修」则为命中 44/44、表述级 1 条。硬错 0。

---

## 必须修清单（按严重度排序）

| 级别 | 位置（主文档行号） | 问题 | 建议改法 |
| --- | --- | --- | --- |
| **M1【中】** | §7.5 守卫表第三行（**2378**） | 「项目键集合守卫 … **否**（在 `:5823-5880` 之外，判据也不同）」——`:5841-5846` 实测**在该 `if` 块内部**（`:5823 if (projectRepairEnabled) {` → `:5880 }`）。这一句是第三轮改写守卫段时新引入的 | 改为「**是**（在该 `if` 块内，但判据是键集合而非 MAX）」；或把该列标题明确为「是否属于 `mergeMaxTotals` 调用点集合」并逐行对齐 |
| **M2【中低】** | §7.5「四种安全模式」段（**2402**）与 #11 表格（2364） | 表格写「为每个旧键**追加显式零行**」，模式段却写它「**既不追加撤回行**、也不删任何既有行」——`:3929-3943` 实测确实 push 零值行（`input_tokens: 0` … `conversation_count: 0`），零值行在 §7.1 的定义里就是「撤回零行」。两处互相否定 | 模式段改为「原子重写但全量保留既有行 + 为旧键补显式零行；与『只追加撤回行』的区别在于它同时重写了整个队列（tmp+rename）」 |
| **S1【低，建议】** | §9.1.2（**2942**） | 「`status` 读取四个状态文件（`src/commands/status.js:200-206`）」——该区间实际声明 7 个路径（另含 `notify.signal`、`openclaw.signal`、`sync.throttle`），函数体还读 `autoRetryPath`（`:282`）与 `syncSkipPath`（`:285`） | 改为「核心四个状态文件」或补「（该函数另读 notify/openclaw 信号与 auto-retry/skip marker）」 |
| **S2【低，建议】** | §7.3（**2300** 行） | 「单元格查表在 `:2545` 的 `byDay`、构造在 `:2564`/`:2566`」——`:2545` 是 `byDay` Map 的**构建**，真正的查表是 `:2564 const data = byDay.get(day);` | 改为「`byDay` 映射构建在 `:2545`，单元格查表 `:2564`、构造 `:2566`」 |

**未发现**：幻觉/编造、证据等级升级、方向相反的断言、样本仓或主文档被本轮改动（`git status --porcelain` 空；主文档仅由他人既有内容读取）。

---

## 复核到的行号抽样（原始记录）

以下为本轮**亲自打开并逐字读到**的行（UTF-8 口径），供二次核对；格式为「文件:行号 → 行内容（截断）」：

```
sync.js    516   async function cmdSync(argv, context = {}) {
sync.js    3709  }
sync.js    3711  function parseArgs(argv) {
sync.js    3945  await ensureDir(path.dirname(queuePath));
sync.js    3946  const tmpPath = `${queuePath}.tmp.${process.pid}.${Date.now()}`;
sync.js    3947  await fs.writeFile(tmpPath, `${output.join("\n")}\n`, "utf8");
sync.js    3948  await fs.rename(tmpPath, queuePath);
sync.js    3929  for (const { model, hour_start } of legacyKeys.values()) {
sync.js    3930  output.push(JSON.stringify({   / :3931 source: "deepseek" / :3934 input_tokens: 0
sync.js    3958  state.offset = 0;
sync.js    4884  async function dropZcodeQueueRows(filePath, { retainRetractions = false } = {}) {
sync.js    4929  await backupExistingFile(filePath);
sync.js    4930  const tmp = `${filePath}.zcoderepair.${process.pid}.${Date.now()}`;
sync.js    4935  await fs.writeFile(tmp, rewritten.length ? rewritten.join("\n") + "\n" : "", "utf8");
sync.js    4936  await fs.rename(tmp, filePath);
sync.js    4940  async function repairZcodeUsageMigration({
sync.js    4953  const mainRepair = await dropZcodeQueueRows(queuePath, { retainRetractions: true });
sync.js    4955  ? await dropZcodeQueueRows(projectQueuePath)
sync.js    4979  if (mainRepair.removed > 0) {
sync.js    5823  if (projectRepairEnabled) {
sync.js    5841  if (missingProjectKeys.length > 0) {      ← 在 5823–5880 内（M1）
sync.js    5865  for (const [key, existingTotal] of existingProjectTotals.entries()) {
sync.js    5876  …without a matching main-bucket repair — skipping to avoid data loss
sync.js    5880  }
sync.js    5848/5852/5856/5860  mergeMaxTotals(…)
sync.js    6492/6512/6540/6559  setMaxTotal(…)
sync.js    6574  function mergeMaxTotals(...maps) {
sync.js    6579  setMaxTotal(merged, key, total);
sync.js    6585  function setMaxTotal(map, key, total) {
sync.js    6271/6294/6316  readCodexFileSnapshot / validateCodexRebuildFileSnapshots / isCodexHistoryCovered
cli.js     15-19 // No args → launch dashboard / if (!command) { / await cmdServe(argv); / return; / }
outcomes-engine.js 154 quality_per_dollar / 156 effective_tokens / 157 effective_cost_usd
local-api.js 95/96/97  PROJECT_USAGE_MAX_ENTRIES = 10 / MAX_TIME_ZONE_CACHE_ENTRIES = 16 / AGGREGATION_BATCH_ROWS = 256
local-api.js 115-116  timeZoneContextKey（含 p${getPricingRevision()}）
local-api.js 213-225  rowDayKey（:224 return hs.slice(0, 10)）
local-api.js 264  if (++processed % AGGREGATION_BATCH_ROWS === 0) await yieldToRequests();
local-api.js 419-423  LOCAL_BADGE_THRESHOLDS（[3,5,10,20] / [1e6…] / [5,20,60,150]）
local-api.js 482-483  night_owl：getZonedParts + parts.hour >= 6 continue
local-api.js 512-519  getTimeZoneContext
local-api.js 585-592  末级本机时区（getFullYear/getMonth/getDate/getHours）
local-api.js 606/608/609  aggregateHourlyByDay：getZonedParts / 日键过滤 / 小时键
local-api.js 2364  const daily = allDaily.filter((d) => d.day >= from && d.day <= to);
local-api.js 2382  const todayParts = getZonedParts(new Date(), timeZoneContext);
local-api.js 2390-2397  collectDays
local-api.js 2495-2521  sessions 端点 LOCAL ONLY / no-store / nosniff / 错误体
local-api.js 2537  const daily = await aggregateByDay(rows, timeZoneContext);
local-api.js 2540  const end = new Date(`${todayStr}T00:00:00Z`);
local-api.js 2545  const byDay = new Map(daily.map((d) => [d.day, d]));
local-api.js 2547-2557  服务端 level（0.25/0.5/0.75）
local-api.js 2563  const day = cursor.toISOString().slice(0, 10);
local-api.js 2566  cells.push({ day, total_tokens: …, level: calcLevel(billable), … });
local-api.js 2586/2588  week_starts_on: "sun" / streak_days: 0,
local-api.js 2634-2642  逐行计价注释（#653）+ 价格修订重入保护
local-api.js 2737-2740  limit 夹取到 PROJECT_USAGE_MAX_ENTRIES
local-api.js 2785  .sort((a, b) => b.billable_total_tokens - a.billable_total_tokens)
local-api.js 2789-2790  total_tokens: String(entry.total_tokens) / billable…
local-api.js 2877  // `models` keyed by source (project rows carry no model column)
async-queue-reader.js 5/8-10/16/30-35/40-47/51-57/86/91/94/98-103/109-114
upload-queue.js 5/6/45-60/62-64
upload-throttle.js 2-9/46-47/57/72-77/104-116/123
wrapped-aggregator.js 29-35/37-53/148-152/182-186/212-218/226-234
session-analytics.js 2343-2354/2356-2362/2364-2383
session-performance.js 5-12/19-26/56-59/79-86
context-health.js 11-16/25/64-67/94-98/108-120
heatmap-compact.js 26-31/52/70-71/74-83/91-97/108-109
fs.js      178-204  sweepAbandonedReclaimGuards（issue #431）
cursor-store.js 21/22/77-86/1404-1406
rollout.js 4254-4264/3403-3408/4155-4171/3272-3288/19990/20016-20032
rollout.js 1194/1949/2291/2652/2682/2765/2793/488-504
rollout.js 16041-16050/17232-17243/12853（LMSTUDIO_WINDOW_BYTES = 8 * 1024 * 1024）
codex-rollout-parser.js 51/732-741/742-762
status.js  200-206/282-285/288/341-347/1050/1024-1036
passive-mode.js 88-90/100/112/125/136/148/159/169/180
migrations/20261005120000_cloud-usage-archive.sql 72/186/213/221/309
scripts/cloud-usage-archive.cjs（10307 B，存在）
docs/cloud-usage-archive.md 3/5/13/17/35-39/74-83（§9.5.5 全部引用命中）
dashboard/src/lib/api.ts 244-252
test/pricing.test.js 891/917-921/923/927/940/945/950/955/960/965/969-972
```

**本轮未做的事**（避免越界）：未运行样本仓库任何程序或脚本；未安装依赖；未读取任何真实会话数据；未修改主文档与样本仓库；未对 §1–§6 做判定（属其它复核 agent 范围）。
