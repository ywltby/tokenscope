# 全文复核 R7-4：§9 消费端（CLI / 本地 API / Dashboard / 原生壳 / 云）与 §10 与 TokenScope 的对照

- 被复核文档：`docs/research/2026-10-14-tokentracker-usage-statistics.md`，复核范围 = §9 全部（L2941–L3582）+ §10 全部（L3583–L4009）。
- 样本仓库：`C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）。复核前后未修改仓库任何文件，未做任何 git 写操作（`git status --porcelain` 空）。
- 行号口径：全部用 `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)` 的实际行号；未使用裸 `Get-Content`（唯一例外是文件行数对照表中一次 `Measure-Object -Line` 交叉验证，结论取 `ReadAllLines`）。
- 复核方式：脚本机械抽取 §9–§10 全部 `路径:行号` 与裸行号引用（输出 `%TEMP%\refs_filled.csv`、`%TEMP%\refs_s9_s10.tsv`）→ 逐条解析为 (文件, 行号) 做存在性/越界检查 → 初判异常逐条回读原文人工定性 → 本人对 34 组重点断言回源核验并贴摘录（计数类全部自己重算）。
- 本报告**只做证伪与登记，不修改主文档，不修改样本仓库**。

## 结论

**§9 的两处机制性结论需要改写（1 处前端消费链、1 处鉴权规则），§10 的可抄/避抄判断与三项点名数字全部成立。其余为引用校准与措辞问题。**

1. **引用存在性**：§9–§10 共抽取 **730** 个引用点（§9 = 535、§10 = 195；显式 `路径:行号` 286 个、同行省略路径的续引 444 个），展开为 **786** 个行号点逐一检查 → **越界 0、指向不存在的文件 0**。24 条机器初判异常经逐条回读全部定性为抽取器伪影（端口号 `:7680`、时间串 `T00:00:00Z`、跨文件上下文错配），**无一条真实越界**。
2. **§9.2.3 端点总表（27 个）独立重清点**：本人按 `if (p === "…")` 字面量重新清点 `/functions/tokentracker-*` = **27 个**，与文档表**数量一致、27/27 定义行号全对**。`/api/auth/*` 的 **8 条转发规则 7 成立 / 1 条错**（见 M2）；`-local-sync` 成功响应为顶层展开 `{ok:true, code, stdout, stderr, uploaded, pending, next_allowed_at?}`，`syncResult` 全库不存在（仅 `dashboard/src/lib/cloud-sync.test.ts:9,13` 作形参名）——**此处文档写法已正确，前轮推翻结论不需要恢复**。
3. **前端零重算成立**：`dashboard/src` 全目录（*.js/jsx/ts/tsx）检索 `computeRowCost|getModelPricing` = **0 命中**（命中只在 `dashboard/vite.config.js` 与 `dashboard/edge-patches/*.ts`，与 §9.3.8/§9.5 的描述自洽）。
4. **三端壳、限额链路、§10 三项点名数字全部成立**：端口矩阵（7680 / WSL 7681 / Windows 17680+运行时挑 / Linux 固定 17680）、macOS `APIClient.swift` 表 11 个方法行号、Windows 5 分钟计时器与 `QuotaWidgetWindow` 推送链、`usage-limits.js` 的 18 并发位 ↔ 17 家 provider、repair 事故 21.7 亿 token 与分项求和、队列写点 8 处、`skipNoOpCursorCommit` 13 个条件项——**逐条回源确认，未发现虚构**。
5. **问题计数：必须修 2 条、应修 3 条、建议修 5 条**；另 1 项任务点名项在本章范围内不适用（见 A.5）。

---

## A. 全量引用存在性

### A.1 抽取口径

- 抽取规则：正则匹配反引号内的 `路径:行号`（`<file>:12`、`<file>:12-34`、`<file>:12,34`）与裸行号续引（`:12`）。裸引用按「同一行内向前最近出现的完整路径」归属；同一行内无完整路径的极少（§9 中仅 6 处），全部人工回读确认（见 A.3）。
- 短文件名按仓库唯一 basename 补路径（如 `serve.js`→`src/commands/serve.js`、`api.ts`→`dashboard/src/lib/api.ts`、`APIClient.swift`→`TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift`）；同名多份的手工指定（`heatmap-compact.js` 取 `dashboard/src/lib/`、`index.js` 取 `src/lib/pricing/`、`usage-limits.js` 取 `src/lib/`、`server.rs` 取 `TokenTrackerLinux/src-tauri/src/`）。
- 排除项：端口号（`:7680`、`:17680`）、时间字面量（`T00:00:00Z`）、`http://127.0.0.1:7680` 一类 URL 端口、`8 亿`/`10 万` 一类量词数字。

### A.2 统计表

| 口径 | 数量 |
|---|---|
| 引用点总数 | **730**（§9 = 535、§10 = 195） |
| 其中：显式 `路径:行号` | 286 |
| 其中：同行省略路径的续引 | 444 |
| 展开后的行号点（含区间与逗号分组） | **786** |
| 越界（行号 > 文件实际行数） | **0** |
| 指向不存在的文件 | **0** |
| 机器初判异常（经逐条回读定性后） | 24 → **真异常 0** |

引用文件分布（前 16，填充路径后）：

| 文件 | 引用数 | 文件 | 引用数 |
|---|---|---|---|
| `src/lib/local-api.js` | 62 | `src/commands/status.js` | 11 |
| `src/commands/sync.js` | 28 | `TokenTrackerWin/ServerManager.cs` | 10 |
| `src/lib/rollout.js` | 25 | `TokenTrackerWin/UsagePoller.cs` | 9 |
| `dashboard/vite.config.js` | 21 | `docs/cloud-usage-archive.md` | 9 |
| `src/commands/serve.js` | 19 | `dashboard/src/hooks/use-activity-heatmap.ts` | 9 |
| `dashboard/src/lib/api.ts` | 15 | `TokenTrackerWin/TrayApplicationContext.cs` | 9 |
| `src/lib/session-analytics.js` | 14 | `TokenTrackerWin/Constants.cs` | 7 |
| `src/lib/wrapped-aggregator.js` | 13 | `src/lib/passive-mode.js` | 7 |

被引用的样本仓库文件总数 **101** 个（含 `dashboard/edge-patches/*` 21 个、`TokenTrackerBar/*` 12 个、`TokenTrackerWin/*` 6 个、`TokenTrackerLinux/*` 2 个）。

### A.3 24 条初判异常的逐条定性（全部伪影）

| 行 | 初判 | 定性证据 |
|---|---|---|
| L3309 `:7680`×1 | 越界 `vite.config.js` | URL 端口 `http://127.0.0.1:7680`，非行号 |
| L3479 `:7680`×2 | 越界 `ServerManager.cs` | 同上（端口出现两次） |
| L3008 / L3355 / L3359 `:00` | 越界 0 行 | 时间字面量 `T00:00:00Z` 与 `00:00–05:59` |
| L3994 `:00`×2 | 越界 `local-api.js` | 同上（`T00:00:00Z`） |
| L2995 `:1576-1584` | 越界 `status-json-light.test.js` | 实属 `src/commands/status.js:1576-1584`（同行前文 `:1590-1593` 的同行漂移）；回读确认 `status.js:1576` 为 `--diagnostics` 参数解析 ✓ |
| L3031 `:67`/`:70-77` | 越界 `wrapped-aggregator.js` | 实属 `src/commands/wrapped.js:67`（`"${...active_days} / 365"`）与 `:70-77`（`Peak hour … UTC`）✓ |
| L3049 `:3147` | 越界 `extension.js` | 实属 `src/lib/local-api.js:3147`（`get("day") \|\| new Date().toISOString().slice(0,10)`）✓ |
| L3502 八条 | 越界 `Constants.cs` | 实属 `UsagePoller.cs`（`:38-40`、`:182-187`、`:226-250`、`:316-328`、`:340-354`）✓ 均在 526 行内 |
| L3506 三条 | 越界 `UsagePoller.cs` | 实属 `TrayApplicationContext.cs:1014-1024`/`:1022` ✓ |
| L3576 两条 | 越界 `qoder-limits.js` | 实属 `src/lib/usage-limits.js:4442-4461` ✓ |

**结论**：本章引用行号无越界；所有"越界"均为抽取器在同一行/相邻行发生的跨文件吸附。

### A.4 引用校准问题（缩写与出处）

- **【建议修 L1】§9.5.3 表的缩写引用**：`-account-daily.ts`、`-account-hourly.ts`、`-account-monthly.ts`、`-account-heatmap.ts`、`-account-model-breakdown.ts`、`-account-devices.ts`、`-account-skills.ts`、`-leaderboard-refresh.ts`、`-leaderboard-profile.ts`、`-community-models.ts`、`-public-visibility.ts`、`-profile-likes.ts`、`-device-token-issue.ts`、`-device-rename.ts`、`-billing.ts`、`-account-summary.ts` 共 **16 类、21 处** 省略了 `tokentracker-` 前缀。真实文件名全部带前缀（`dashboard/edge-patches/tokentracker-account-daily.ts` 等），机械检索 `*-account-daily.ts` 不可命中。上下文（同节已写完整名 `dashboard/edge-patches/tokentracker-ingest.ts`）使语义可辨，但建议统一补前缀。
- **【建议修 L4】§9.6.3**：`ServerManager.cs:12-20` 指向类注释块（讲 `serve --port P` 的启动方式与端口选择理由），`Port` 属性在 `:35`、`BaseUrl => http://127.0.0.1:{Port}` 在 `:38`。文档同句已给 `:34-38`，故只需把 `:12-20` 改为 `:35`/`:38` 或删去。
- **【建议修 L5】§9.6.3**：「**该环境变量只由 Windows 壳注入**（`ServerManager.cs:694,700`）」——`:694` 是 `psi.Environment["TOKENTRACKER_APP_SHELL"] = "windows"`，`:700` 才是 `TOKENTRACKER_NATIVE_SYNC_OWNER = "windows-host"`。两处并列易读成 694 也注入该变量；建议写成 `:698-700`。

### A.5 任务点名项中不适用的一条

`migrations/*.sql` 在 §9–§10 范围内**无任何引用**（全文检索 `migrations` 只命中 L3713 的 `` `cursors.migrations[KEY]` ``，那是游标状态文件内的迁移键，不是仓库 `migrations/` 目录）。该点名项在本章范围内不适用，如实登记。

---

## B. 重点核验

### B.1 §9.2 本地 API 全清单（独立重清点）

**路由字面量重清点**：在 `src/lib/api.js`→`src/lib/local-api.js` 内检索 `p === "…"`，得 `/functions/tokentracker-*` 路由 **27 条**（行号如下），与文档 §9.2.3 表 **27 行逐条比对，定义行号 27/27 全对**：

```
2137 cloud-session   2166 local-sync      2330 wrapped         2358 usage-summary
2425 usage-daily     2441 outcomes         2470 session-insights 2500 sessions
2526 context-health  2533 usage-heatmap    2596 usage-model-breakdown
2668 usage-category-breakdown 2731 project-usage-summary 2828 project-usage-detail
2920 user-status     2940 cloud-sync-pref  2982 proxy-config    3074 proxy-test
3121 telemetry-pref  3137 machine-id       3146 usage-hourly    3156 usage-monthly
3192 pets            3240 subscription-manager 3294 skills       3470 achievements
3482 usage-limits
```

**`-local-sync` 响应结构（前轮被推翻的 `syncResult` 键，独立确认）**：
```js
// src/lib/local-api.js:2313-2322
let cloudAccess = { uploaded: false, pending: true };
try {
  const state = JSON.parse(fs.readFileSync(path.join(path.dirname(qp), "upload.throttle.json"), "utf8"));
  if (state.nextAllowedAtMs) cloudAccess.next_allowed_at = new Date(state.nextAllowedAtMs).toISOString();
  cloudAccess.uploaded = state.lastSuccessAttemptId === extraEnv.TOKENTRACKER_LOCAL_SYNC_ATTEMPT_ID && Number(state.lastSuccessMs || 0) >= syncStartedAt;
  const queueState = JSON.parse(fs.readFileSync(path.join(path.dirname(qp), "queue.state.json"), "utf8"));
  cloudAccess.pending = fs.statSync(qp).size > Number(queueState.offset || 0);
} catch { /* no upload has been attempted yet */ }
json(res, { ok: true, ...result, ...cloudAccess });
```
`result` 来自 `runSyncCommand`（`:742`）的 resolve 值 `const r = { code: code ?? 1, stdout: trimOutput(stdout), stderr: trimOutput(stderr) }`（`:794`）→ 顶层展开 **`{ok:true, code, stdout, stderr, uploaded, pending, next_allowed_at?}`** ✓ 文档写法正确。
`syncResult` 全仓检索（排除 `node_modules`）：**2 处命中，均在 `dashboard/src/lib/cloud-sync.test.ts:9,13`**（函数形参名 `syncResult`），与 §9.7.4 #3「全库不存在 `syncResult` 键」一致 ✓。
失败体 `:2324` = `{ok:false, error, code, recovery_url, next_allowed_at, retry_after_seconds, stdout, stderr}` ✓ 逐字一致。

**其余抽查全部成立**：`resolveQueuePath()`（`:83-86`）、JSON 统一 `Cache-Control: no-store`（`:914-927`）、`crypto.randomBytes(24).toString("hex")`（`:1016`）、`/api/local-auth`（`:1681-1695`，返回 `{token, serverVersion?}`）、宠物资产两条前缀正则（`:1698`、`:1724`，`image/webp`+`no-cache`+`nosniff`）、`/api/pets/import`（`:1747`）、`/api/auth-bridge/verifier`（`:1767-1788`，TTL 5 分钟、一次性读取）、头像代理 LRU（`:43-48`：1h / 512 KiB / 64 条）、`rowDayKey`（`:213-225`，`:224` = `hs.slice(0,10)`）、`scope`/`include_account_level`（`:398-401`）、去重键（`:202-211` = `source|model|hour_start`）、项目队列键（`:145-153` = `project_key|source|hour_start`）、`account=1` 包装层（`:2347-2355`，成功打 `X-TokenTracker-Account-View: 1` 在 `:1565`、失败冷却 2 分钟 `:27` = `ACCOUNT_VIEW_FAILURE_BACKOFF_MS`，`:1134` 使用）、`cloud-account.js:20-27` 六 slug 映射 + `:17-19` 注释「remain local-only and are intentionally absent here」、`usage-summary` rolling `active_days` 跳过空日（`:2390-2404`）、`usage-model-breakdown` 硬编码 `pricing.source:"litellm"`（`:2658`）与 `cost_source` 三态（`:2624-2626`）、逐行计价再求和（`:2634-2640`）、`project-usage-summary` 字符串化 token（`:2789-2790`）+ billable 降序（`:2785`）、热力图 `streak_days: 0`（`:2588`）、codex 降级 `breakdown_status:"queue_fallback"` / `fallback:"queue_totals"` / `includes_content:false` / `includes_inputs:false`（`:332`/`:336`/`:377`/`:387`）、`-sessions` 本地性（`:2495-2499`、`:2509`、`:2516-2521`）、`context-health` 无包装（`:2528`）+ 六顶层键 + `largest_items` 取 10（`context-health.js:91-118`）、`skill_usage` 返回结构（`:3337-3408`，计价 `computeRowCost({...tokens, model, source:"claude"})` 在 `:3381`）、徽章阈值（`:419-423`）+ 注释「embeds none of these numbers」（`:416-418`）、`night_owl` 用请求时区小时（`:482`）。

**`/api/auth/*` 八条规则逐条核对**（证据均在 `src/lib/local-api.js`）：

| # | 文档断言 | 源码 | 判定 |
|---|---|---|---|
| ① | 入口预检先于 OPTIONS，403 `untrusted_auth_origin` | `:1644-1648` | ⚠️ 成立但**不完整**（见 S2） |
| ② | 有浏览器 cookie → relay 集覆盖；无 → 清空该头 | `:1811-1812` | ✅ 逐字 |
| ③ | csrf 与 relay 不等 → 删除；refresh 非回退分支强制用 relay csrf | `:1813`、`:1831-1833` | ✅ |
| ④ | 注入条件 `p !== "/api/auth/refresh" \|\| hasClientCookie \|\| hasEffectiveCsrfHeader` | `:1837-1838` | ✅ 逐字 |
| ⑤ | cookie-less 回退：`?client_type=mobile` + body `{refresh_token}` + 禁用注入 | `:1829-1830`、`:1864-1869`、`:1839-1841` | ✅ 逐字 |
| ⑥ | stale-CSRF 救援：403 且体匹配 `/invalid csrf token/i` → 去 cookie/csrf 重放 | `:1884-1907` | ✅ 逐字 |
| ⑦ | `Domain=` 重写 `Domain=localhost`；**仅 2xx 才捕获 relay cookie** | `:1927`；`:1922` | ❌ 前半 ✅、**后半错**（见 M2） |
| ⑧ | 一致性闸门：`apikey` == anonKey、baseUrl/anonKey 或会话代际变化 → 409 | `:1801-1804`、`:1910-1913`、`:1917-1921` | ✅ 成立（行号 `:1922-1942` 实际覆盖的是 ⑦ 的内容，⑧ 的三个判据点未被该区间覆盖——引用选点偏） |

### B.2 §9.3 Dashboard 消费

- **前端零重算（`:3272-3274` 结论）成立**：`dashboard/src` 全目录 `computeRowCost|getModelPricing` = **0 命中**（本人独立检索）。后端 `local-api.js` 内计价三处：`buildDailyAggregates`（`:285`）、`usage-model-breakdown`（`:2640`）、`skill_usage`（`:3381`）✓ 计数正确。
- **`streak_days` 链（文档核心断言，判定为错）**：见 M1。
- **热力图服务端 level 规则**：`local-api.js:2547-2557`（`billable` 归一，四档 0.25/0.5/0.75）+ 每格带 `level`（`:2566`）✓；前端 `heatmap-compact.js` 的 `levelOf`（`:50-58`）与之同规则、`cells.push` 带 `level`（`:70`）、`billable_total_tokens: total`（`:69`）、缺 `model_names` 拒绝展开（`:24-27`）、非紧凑原样返回（`:20-28`）✓ 全部逐字成立。
- **`activeDays` 后端优先**：`DashboardPage.jsx:819-850`（`Number(heatmap?.active_days)` 有限则直接用；否则遍历 `heatmapDaily` 与 `heatmap.weeks`，`getBillableTotal(row)` 在 `:837`）✓。
- **桌宠宿主**：`pet-linux-host.js:194`（`?weeks=52${tz}&${ACCOUNT_QUERY}`）、`:198`（`streak: toNumber(r.body?.streak_days) ?? 0`）、`:211`、`:223`、`:237` ✓ 逐字。
- **`quota.jsx` 零 fetch**：全文 77 行仅 `postMessage`（监听 `quota:context` 在 `:19`，发 `quota:ready` 在 `:24`，`:71` 发 `quota:dashboard`）✓ 无 `fetch`。
- **Vite dev 前缀集合（§9.3.8）**：`configureServer` `:1200`、`middlewares.use` `:1202`、14 条精确路径 `:1208-1242`、`proxyToLocalCli` → `127.0.0.1:7680`（`:1249`、`:1141-1151`）、502（`:1161-1168`）、`:1259 next()`、`:1321-1325 historyApiFallback`、`:1332-1336` `/proxy/ipcheck`、`:1341-1347` `/api/auth` 原生 proxy、`:508-514` 注释「delegated to `src/lib/pricing/` (CJS)」、`:1052-1057` 只计一次价 ✓ 全部在位（`:7680` 是端口，见 A.3）。

### B.3 §9.6 三端原生壳

- **端口矩阵**：`serve.js:14`（默认 7680）、`:15-21`/`:202-206`（WSL 7681）、`:208-215`（顺延 ≤20）✓；`TokenTrackerBar/.../Constants.swift:4-5`（`http://localhost:7680` / `serverPort = 7680`，`NativeQAProfile` 可覆盖）✓；`ServerManager.cs:157`（`Port = PickServerPort()`）、`:35`（`Port` 属性）、`:38`（`BaseUrl`）、`:469-476`（三条理由 + `PreferredPort = 17680`）✓；`extension.js:18-19`（`127.0.0.1:17680`）✓。
- **macOS 表 11 个方法**：`APIClient.swift` `:43-52`/`:66-69`/`:73-77`/`:79-82`/`:86-90`/`:94-97`/`:101-105`/`:110-113`/`:119-121`/`:127-143`/`:148-150` **全部落在对应函数体内** ✓；`withTimeZoneQueryItems` `:241-246`、`withAccountQueryItems` `:252-254`（追加 `account=1`）✓；`fetchHourly(day:)` 的 `day` 来自 `DateHelpers.dayRange`（`DateHelpers.swift:44-50`，本地 `yyyy-MM-dd`），调用点 `DashboardViewModel.swift:192-194`（rolling range 构造）与 `:311`（`fetchHourly(day: rollingTo)`）✓。
- **macOS 启动与小组件**：`ServerManager.swift:195`（`[entryPath,"serve","--port",…,"--no-sync","--no-open"]`）、`:228`（`zsh -lc "… serve --port … --no-sync"`）、`:83-99`（`lsof -ti tcp:<port> -sTCP:LISTEN`）、`:203`（只注入 `TOKENTRACKER_APP_SHELL=macos`）✓；`QueueActivityMonitor.swift:3-5`（queue.jsonl + queue.state.json）✓；`WidgetSnapshot.swift:18-30`（`group.com.tokentracker.bar` / `widget-snapshot.json` / schema 2）、`:362`（严格相等过滤）、`:354-356`（解码失败即 nil）✓；`StaticSnapshotProvider.swift:20`（`.placeholder`）、`:25`（`.empty`）、`:24-29`（`.after(now + 15 分钟)`）✓；`NativeBridge.swift:452,457,468,484` 四处 `WidgetCenter.shared.reloadAllTimelines()` ✓；`WidgetSnapshotWriter.swift:36-38`（five-minute freshness contract）、`:54`（`update(from:capturedAt:)`）、`:108-110`（写盘后 reload）✓。
- **Windows**：`Constants.cs:14-18`（健康 30 s / 启动等待 20 s，文件共 25 行）✓；`TrayApplicationContext.cs:84`（`_syncTimer` 5×60×1000）、`:240`（Tick → `TriggerBackgroundSync`）、`:965-971`（Running 时 `Start()` + 立即 `TriggerBackgroundSync()`）、`:972-974`（Failed 时 Stop）、`:234-235`（`SyncStarted`/`SyncCompleted` 订阅）、`:996-1006`（回流 UI）、`:286`（`IncludeLimits = 宠物可见 || 配额启用`）、`:238`（`LimitsFailed` → `MarkFailed()`）、`:1014-1024`（`OnLimitsUpdated` → `_quotaWindow.ApplyLimits`）✓ 全对；`UsagePoller.cs:38-40`（`UseProxy=false`、6 s）、`:144-158`（`IncludeLimits` → 并发 `FetchLimitsAsync` + `RaiseLimitsUpdated`/`LimitsFailed`）、`:182-195`（GET `-usage-limits`，取数 `:187`）、`:226-250`（`-usage-summary`）、`:316-328`（`-usage-heatmap?weeks=52`，读 `streak_days`/`active_days`）、`:340-354`（`-usage-model-breakdown`，近 30 天）✓；`QuotaWidgetWindow.cs:126-131`（`ApplyLimits`）、`:151-155`（`MarkFailed`）、`:389-394`（导航 `${BaseUrl}/quota.html?app=1`）、`:396-409`（`PushContext` 发 `{type:"quota:context", limits, failed, selected, locale, light}`）✓ 逐字；`ServerManager.cs:313`（`LocalSyncPublisher.PublishAsync`）、`:698-700`（仅 `serve` + 非 native-only WSL 注入 `TOKENTRACKER_NATIVE_SYNC_OWNER`）✓。
- **Linux**：`server.rs:138-160`（`server-{owner_pid}.json` + pid/port/start_time/boot_id/owner）、`:246-257`（`record_server`）、`:343-372`（`reap_orphaned_servers`）、`:700`（`TOKENTRACKER_APP_SHELL=linux`）✓；`extension.js:784`（`Soup.Session({timeout:20})`）、`:229-245`（404 → `ForeignServerError`）、`:997`（`_getJson` 自动拼 tz/tz_offset_minutes/account）、`:1025`（`POST -local-sync`）、`:1044`/`:273-275`（`dayString(new Date())`）、`:1046-1051`、`:1086`、`:1110-1116`、`:1137`、`:830-832`（固定周期取数）✓。

### B.4 §9.7 与用量统计的边界

- **限额链路独立**：`fetchUsageLimitsUncached`（`:3976`）→ `Promise.all` **18 个并发位**（`:4055`，本人逐个点名重算：claudeResult、codexResult、cursor、kimi、gemini、kiro、antigravity、copilot、grok、zcode、opencodeGoRaw、qoder、qoderCn、codingPlan、agentPlan、commandCodeRaw、devinRaw、claudeServiceStatus）；`data` 收录 **17 家**（`:4406-4440`，`fetched_at` + 16 家 + `claudeServiceStatus` 不在其中）✓ **18 ↔ 17 差额 = `claudeServiceStatus`** 成立；挂载条件 `if (claude.configured && claudeServiceStatus && claudeServiceStatus.indicator !== "none")`（`:4279-4280`）✓；无 token 时 `Promise.resolve(null)`（`:4183-4185`）✓。
- 缓存与选择键：`CACHE_TTL_MS = 2*60*1000`（`:64`）、分区键 `${devin}-${antigravity}`（`:56-63`，antigravity 由 `isAntigravityQuotaDisabled()` 取反）✓、single-flight（`:3954-3970`）、磁盘缓存 7 天（`:71`、`:118-129`）、Claude 新鲜度 10 分钟（`:120`）、429 冷却（`:141-144`）✓。
- provenance（`:4442-4461`）：`stale = provider.stale === true || ageMs > 10 分钟`、`source` 三态、`confidence` 三段匹配 ✓ 逐字。
- 窗口：`buildWindow`（`:161`）、`clampPercent`（`:146`）、Codex 18000 s / 604800 s（`:332-333`）✓。

### B.5 §10 对照清单

**「可抄」18 条的行号与内容——核验 18/18 有源码依据，未发现虚构**（每条至少抽 2–4 个点位回读）：

| 条目 | 核验结果 |
|---|---|
| ① 锁协议 | `fs.js:69-74` 六常量逐字；`openLock`（`:284-…`）`open(wx)` + `randomUUID` + `${lockPath}.heartbeat.${sha256(token)}`；心跳 `utimes` + `timer.unref()`（`:206-224`）；回收三分支（`:105-144`：owner 不存在→true；owner 存活→只看 token 心跳 mtime；无 owner 记录→锁文件 mtime）✅ |
| ② 目录探测 | `scan-roots.js:89-110` 逐字（`opendirSync(target).closeSync()`、ENOENT/ENOTDIR→`error:null`、其余→code）；`:248-253` `" (unreadable: CODE)"` / `" (missing)"` ✅ |
| ③ 追加读侧 | `async-queue-reader.js:8-10` 签名含 `mtimeNs/ctimeNs`；`:40-47` 锚点续读逐字；`:51-57` 残行 `catch { malformed++ }`；`:84-90` 尾部处理 ✅ |
| ④ 压缩 | `upload-queue.js:5-6` `BACKLOG_BYTES`/`CHUNK_BYTES`；`:64-78` 行边界判断逐字；`:105-121` 三重校验 + rename 逐字；`:20-21` 「byte positions, not JavaScript string lengths」；文件实际 134 行 ✅ |
| ⑤ 原子写 | `fs.js:9-33` 逐字（tmp 名 UUID、rename、失败 unlink）；`:15-17` mode 只在创建时生效的注释；`updateJsonLocked`（`:416-443`，30 s / 10 ms / `.lock` / chmod600）✅ |
| ⑥ 代际存储 | `cursor-store.js:13-23` 五个常量；`:855-1016` commit；`:977-990` manifest 最后写；`:1005-1009` 三代；`:1010-1015` 半成品清理；`:124-139` current→rollback→previous；`:1466-1481` sha256 摘要校验；`:1807-1809` `safeShardFilename`；`:1361` `COPYFILE_FICLONE`；`:907` 先 unlink 再建；`:21` `DEFAULT_ACTIVATION_BYTES = 16 MiB` ✅ |
| ⑦ 重算守卫 | `sync.js:234-248` 事故注释；`:1147-1192` 根守卫；`:6271-6293`/`:6294-6314`/`:6316-6336` 快照三件套；`:6338-6346` 确为下一函数头注释；`:952-959` 失败不写迁移键；`:1177-1181` `if (!opts.auto)` 包住 stderr ✅ |
| ⑧ 事件去重 | `rollout.js:302-330` 双闭包；`:428-461` 判据；`:436` `if (rebuildingBaseline) return true;`；`:459`/`:460`；`:292-295` Codex 键；`:4914-4919` `msgId:reqId`；`:1233-1237` 10 万截断；`sync.js:4478-4484` 严格更大 ✅ |
| ⑨ 文件级去重 | `rollout.js:1056-1107`；`:1074-1077` 消息哈希兜底注释；`sync.js:683-684` native 优先 ✅ |
| ⑩ 双队列 | `rollout.js:683-686`；主行 `:3328-3344`（含 `usage_precision`、`...prioritySubsetOf`）、项目行 `:3515-3532`（多 `project_ref`/`project_key`，少 `model`/`usage_precision`/`priority_*`）；`:3513` 缺键跳过；`:4073-4079` 字段表；`sync.js:3654-3659` 逐字 ✅ |
| ⑪ 上传进度 | `sync.js:4313`/`:4315`/`:4329`/`:4392`/`:168`/`:4360-4362`/`:4402-4411`/`:4433-4441`/`:4447-4449`/`:4488-4490`/`:4511-4512` 全部在位 ✅ |
| ⑫ 后台审计 | `sync.js:294`/`:301`/`:3763-3819`/`:3456-3460` ✅ |
| ⑬ 读时纠错 | `local-api.js:155-200`（`isLegacyInclusiveCodexRow` + cursor/antigravity 抬升，`:191-198`）；`sync.js:4504-4509` 上传侧共用 `normalizeQueueRow` ✅ |
| ⑭ 跳过可见化 | `sync.js:484-506`（`lock_debris`/`lock_busy`、`{reason,detail,at,lockPath}`）、`:565` 删除、`status.js:285`/`:1058`/`:1054-1057` ✅ |
| ⑮ hook 三件套 | `init.js:1445`（宿主）、`:1446-1447` 注释逐字、`:1483`/`:1484` 冗余判断、`:1495-1498` 先落信号、`:1519-1536` 20 s 节流、`:1538-1559` 链式、`:1561` `process.exit(0)`、`:1563-1573` `spawnDetached`；`openclaw-session-plugin.js:358,361` 同一 quirk ✅ |
| ⑯ 配置写入 | `notify-command.js:23-36`；`claude-config.js:181-230` + `:248-262`；`omp-hook.js:17`/`:388-395`（`unmanaged-extension-present`）+ `init.js:813` 消费；`opencode-config.js:66-87`/`:73-75`/`:80-85`/`:94`；`codex-config.js:89-104`/`:245-309`；`gemini-config.js:265-279`；`grok-hook.js:85-91` 无备份覆盖 ✅ |
| ⑰ 发现层细节 | `install-resolver.js:6-18` union 注释、`:25-36` 折进探测；`wsl-probe.js:232-242` 逐字、`:193-201` 两种前缀顺序、`:244-262` SQLite 快照（-wal/-shm/-journal）；`sync.js:766-772` ✅ |
| ⑱ 可测性 | `wsl-probe.js:57-63`；`cursor-store.js:25-32`、`:972`（`beforeManifestSwap` 注入点）；`sync.js:516-538`、`:526-531` 注释、`:310-373`、`:3755-3762`；`AUTO_SYNC_SOURCES` 实测 **45 项**（本人重算，逐项读取 `:320-366`）✅（顺序差异见 L2） |

**三项点名数字——全部正确**：

1. **「repair 先删后建致 21.7 亿 token 丢失」**：`sync.js:239-240` 逐字 `wiped 2.17B claude tokens (-1.27B opus-4-7, -474M opus-4-6, -376M sonnet-4-5, -48M haiku, -6M sonnet-4-6)`。分项求和 `1270 + 474 + 376 + 48 + 6 = 2174M ≈ 2.174B` ✓ 与总数 2.17B 自洽；2.17B = **21.7 亿** ✓ 单位换算正确。另：注释同时确认「atomic-drop + rescan」为根因（`the repair does atomic-drop + rescan`）、0.26.4 回退到 v4（`:242`）、key 仍为 `claudeGroundTruthRepair_2026_05_v4`（`:248`）——文档 §10.2 #4 与 §7（L2412-2428）两处叙述一致。
2. **「队列写点 8 处」**：本人独立 grep `appendFile` 全仓——主队列 `rollout.js:3483`、`:20031`、`:22594` + `sync.js:5126`、`:5207`、`:5281` = **6 处**；另 `sync.js:4114`（fallback synthetic message）、`rollout.js:3536`（`projectQueuePath`）= **共 8 处** ✓ 文档「至少 6 处 + 2 处」的分解与总数均正确。
3. **「13 个条件项」**：`sync.js:3424-3437` 实数 **13** 个（`opts.auto`、`!isFullSourceScan`、`cursorStore.mode==="v2"`、`cursorStore.requiresCommit!==true`、`totalParsed===…`、`totalBuckets===0`、`!(grokResult.projectBucketsQueued>0)`、`!(commandCodeResult.projectBucketsQueued>0)`、`!commandCodeResult.schemaMigrated`、`!codexColdAuditDue`、`!codexFallbackRetryRan`、`!grokHookSignalConsumed`、`!opts.repairGrok`）✓ 与 §2（L380）、§10.2 #1 完全一致。

**「不建议照抄」17 条——抽查 10 条均有源码依据**：单函数 3194 行（`sync.js:516-3709`）✓；`.reclaim` 三处（`fs.js:178-204` 迭代清扫，文档 §10.2 #2）在文件 455 行内 ✓；无写侧互斥 ✓；repair 先删后建 ✓；Grok 无备份覆盖 ✓；三套并行纠错（`normalizeQueueRow` `local-api.js:170-200`；云端 upsert；repair 迁移键）+ `sync.js:249-256` 注释（云端 `conversation_count` 恒 0 事故）✓；体积阈值双形态（`cursor-store.js:21`/`:77-86`）✓；legacy `groupQueued` 键存在性语义（`rollout.js:3181-3194` + `sync.js:216-226` v3 注释解释该键曾被误当聚合标记）✓；`bin/tracker.js:36-40` `unprotected === true` → `exit(1)` ✓；`status` 传 11 键 / 消费 8 键 / 三个死别名（`passive-mode.js:125`/`:136`/`:148` 的 `?? hookStatus?.codex` 等）+ JSDoc 漏 `env`（`passive-mode.js:88-90` vs `:93`）✓；`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 在 `pricing/index.js` 内仅 `:31` 定义 + `:375` 导出（**本文件内从未读取**）✓，真正生效的是 `codex-rollout-parser.js:51`（用于 `:733`）✓；`toUtcHalfHourStart` 双份（`rollout.js:4173` / `claude-categorizer.js:1175`，测试只指向前者）✓；reasoning 白名单硬编码 `pricing/index.js:248-253`（codex/acode/every-code/omo/cline）✓ 与文档列出的五家逐字一致。

**数量级数字复算（§10.3⑤）**：

| 文档断言 | 实测 | 判定 |
|---|---|---|
| curated `exact` 84 条 | 84 | ✅ |
| `source_exact.acode` 64 条 | 64 | ✅ |
| `fuzzy` 47 条 | 47 | ✅ |
| seed 2284 条模型 | 2284（`_meta.kept_models` 亦为 2284） | ✅ |
| `cache_read_input_token_cost` 566 条 | 566（≈24.8%，「约 75% 没有」✅） | ✅ |
| `cache_creation_input_token_cost` 188 条 | 188（≈8.2%，「约 92% 没有」✅） | ✅ |
| seed `_meta.generated_at = 2026-05-26T11:12:29.761Z` | 该值 | ✅ |
| 磁盘缓存 TTL 24 h（`litellm-fetcher.js:12`） | `DEFAULT_TTL_MS = 24*60*60*1000` | ✅ |
| 四级降级链（`:107-164`） | 1 新鲜磁盘 → 2 上游 → 3 陈旧磁盘 → 4 seed | ✅ |
| 单 `cache_write` 槽位（`matcher.js:410-412`） | `cache_creation_input_token_cost → out.cache_write`，无 5 m/1 h 分档 | ✅ |

**文件行数声明（22 处）**：`tokentracker-ingest.ts` 225 / `-telemetry.ts` 85 / `-account-summary.ts` 954 / `-account-daily.ts` 894 / `-account-hourly.ts` 333 / `-account-monthly.ts` 261 / `-account-heatmap.ts` 391 / `-account-model-breakdown.ts` 956 / `-account-devices.ts` 268 / `-account-skills.ts` 251 / `-leaderboard.ts` 262 / `-leaderboard-refresh.ts` 1449 / `-leaderboard-profile.ts` 1074 / `-community-models.ts` 150 / `-public-visibility.ts` 210 / `-profile-likes.ts` 354 / `-device-token-issue.ts` 183 / `-device-rename.ts` 164 / `-billing.ts` 492 / `-embed-svg.ts` 320 / `init.js` 2427 / `sync.js` 7251 / `status.js` 1729 / `local-api.js` 3543 / `upload-queue.js` 134 / `edge-pricing-parity.test.js` 420 / `status-json-light.test.js` 96 —— **全部一致，误差 0**。`dashboard/edge-patches/` 实测 **28 个文件，全部 .ts** ✅。

---

## C. 内部一致性

- **§9 与 §5/§7 无冲突**：§9 的「queue.jsonl 累计快照 + last-wins」与 §7/§10.3③ 一致；「同键多行是常态、零行是撤回」与 §7 的守卫描述（L2252）一致；去重键三处（`local-api.js:202-211`、`wrapped-aggregator.js:77-86`、`vite.config.js:572-604`）逐字相同 ✅。
- **同一数字多处书写一致**：「21.7 亿 / 2.17B / 2174M」在 L2412、L2428、L3707、L4130 四处出现，四处互不矛盾（L2428 明确给出求和验算 1274M→2174M）✅；「13 个条件项」在 L380、L3953、L4127 三处一致 ✅；「7251 / 2427 / 1729 / 3543 行」在 §1（L112-116）与 §9.1.1（L2958-2960）两处数值一致 ✅。
- **潜在混淆（不构成错误，供编辑判断）**：§5 的「45 家 provider」（`SUPPORTED_PROVIDERS`，L300/L872）与 §10.1⑱ 的「`AUTO_SYNC_SOURCES` 实测 45 项」是两个不同的 45（后者是自动同步源白名单，恰好也是 45 项，且成员集合与 provider 名单并不相同——例如 `dumate`、`claude-science` 在名单里但 `craft` 等归类不同）。两处各自都准确，但读者易误以为同一个数。
- **表格与围栏**：§9–§10 范围内代码围栏 **44 个，全部成对**；**表格列数不一致 0 处**（首轮机器报 10 处，全部为单元格内已转义的 `\|` 造成的伪阳性，剥除转义后复核为 0）。§10.1 条目编号 ①–⑱ 连续无缺、无误标。
- **【未能确认】标注**：本章 4 处（L2947 引用约定、L3242 两处判定条件、L3909、L3935），均附判定条件，与 §9.7.4 表对应 ✅；§9.7.4 表 12 条中 #3/#4/#5/#7/#12 标「已结案」——其中 **#5 需要随 M2 回改**（见 D）。

---

## D. 问题清单

### 必须修（2 条）

**M1. §9.3.3 表格「Dashboard 热力图 → 前端 `computeActiveStreakDays` 覆盖 → 正确」与源码不符；同段「前端自算并覆盖」的因果描述缺触发条件。**

证据链（三处，全部亲自回读）：

```js
// dashboard/src/hooks/use-activity-heatmap.ts:174-179 —— 只有响应缺 level 才走前端自算分支
const hasLevels = weeksData.some((week: any) =>
  (Array.isArray(week) ? week : []).some(
    (cell: any) => cell && Number.isFinite(Number(cell.level)),
  ),
);
if (!hasLevels && weeksData.length) {   // ← :206 / :220 的 computeActiveStreakDays 都在此分支内
  ...
}
// :234-235 —— 否则服务端对象整体采用（含 streak_days）
if (!isCurrent()) return;
setHeatmap(res || null);
```
```js
// src/lib/local-api.js:2587-2588 —— 本地端点每格自带 level，且 streak_days 硬编码 0
active_days: cells.filter((c) => c.billable_total_tokens > 0).length,
streak_days: 0,
// :2566 —— cells.push({ day, total_tokens, billable_total_tokens, level: calcLevel(billable), models })
```

因此：**本地模式（localhost）下 `hasLevels === true`，前端走 `:235 setHeatmap(res)`，服务端 `streak_days: 0` 被原样消费——"前端自算并覆盖"不会发生**。`:206`/`:220`/`:284`/`:295` 四处调用的前提是「响应无 `level`」（旧 edge 或非本地来源），不是常态。

补充证据（说明该字段在 UI 上根本没有独立出口）：
- `dashboard/src` 全目录检索 `streak_days`：除 `use-activity-heatmap.ts` 四处写入外，只有 `heatmap-compact.js:84` 的透传与 `pet-linux-host.js:198` 的直读，**没有任何组件读 `heatmap.streak_days` 显示**；
- 身份卡的「连续天数」实际绑的是活跃天数：`dashboard/src/ui/dashboard/views/DashboardView.jsx:235` → `<StatsPanel … streakDays={activeDays} …>`，`activeDays` 来自 `DashboardPage.jsx:1390`，渲染在 `StatsPanel.jsx:167-168`。

修法建议：把 §9.3.3 表格首行改为「Dashboard 热力图 | 服务端值（本地恒 0），前端仅在响应缺 `level` 的兼容分支自算 | 本地模式下显示 0」；正文「前端自算并覆盖」补上 `:174-179` 的触发条件；并新增一句「身份卡展示的『连续天数』实为 `activeDays`」。这同时解释了 §9.3.3 表格后两行（Linux 桌宠拿到 0、Windows 托盘拿到 0）——三者是同一机制，不是三处独立缺陷。

**M2. §9.2.4 `/api/auth/*` 规则⑦「**仅 2xx 才捕获** relay cookie」错。**

```js
// src/lib/local-api.js:1922
const allowRelayCapture = proxyRes.status < 400;      // ← 2xx 与 3xx 都会捕获，仅 ≥400 排除
...
// :1926-1929
if (k.toLowerCase() === "set-cookie") {
  const rewritten = v.replace(/;\s*[Dd]omain=[^;]*/g, "; Domain=localhost");
  if (allowRelayCapture) captureSetCookies(rewritten);
```
文档现文：「响应侧：`set-cookie` 的 `Domain=` 一律重写为 `Domain=localhost`，**仅 2xx 才捕获** relay cookie（3xx/4xx 的删除型 set-cookie 不得破坏在效会话）」。**`3xx` 会被捕获**，故括号里的「3xx」不成立。另注：源码在 `:1943-1950` 还有一条文档未提的清理分支（`isStaleCsrf403 && 403 && injectedRelayCookies && !hasClientCookie` → `clearRelayCookies`），补上更完整。
连带：§9.7.4 #5「8 条规则逐条可核对」的结案结论需随本条回改（其余 7 条成立）。

### 应修（3 条）

**S1. §10.3② 的 Craft 注释归属描述错位。**
文档：「而 Craft 侧的注释自称 "count one-per-assistant-message" **与代码冲突**（`:17233-17234`）——源码自身两处说法互相打架，以代码为准。」
源码实际是：
```js
// src/lib/rollout.js:17232-17235
// conversation_count: 1 the first time we see a session, 0 on subsequent
// syncs of the same session. NOTE: this differs from omp/Claude which
// count one-per-assistant-message. Cross-provider "conversations" totals
// are therefore not directly comparable — Craft's are per-session.
```
注释**没有**"自称"是 one-per-assistant-message；它自述 Craft 是 **per-session**，并把「one-per-assistant-message」归于 **omp/Claude**。真正与代码冲突的是该注释**对 Claude 的描述**——Claude 实际按主会话**用户文本消息**计数（`rollout.js:2807-2837`，`:2833` `userBucket.totals.conversation_count += 1;`）。修法：主体改为「Craft 侧注释把 Claude/omp 描述成 one-per-assistant-message，与 Claude 实际按 user 文本消息计数冲突」。

**S2. §9.2.4 ① 入口预检描述不完整（安全相关事实缺失）。**
```js
// src/lib/local-api.js:691-703
function hasAllowedLoopbackOrigin(headers = {}) {
  const candidates = [headers.origin, headers.referer];
  for (const raw of candidates) {
    if (raw == null || raw === "") continue;      // ← 两个头都缺失 → 循环不进入 → return true
    ...
  }
  return true;
}
```
文档写「`origin`/`referer` 须为 `http:` 且主机 ∈ {127.0.0.1, localhost, ::1}，或 `sec-fetch-site: cross-site` → 403」，未写「**两者皆缺时直接放行**」。建议补一句。另 `isLoopbackHostname`（`:687-689`）还接受 `[::1]`，白名单比文档多一种拼写。

**S3. §9.3.3 表格 Windows 行「注释却写『streak is server-computed』」措辞与源码不符。**
```csharp
// TokenTrackerWin/UsagePoller.cs:314-315
/// <summary>Heatmap: all-time active days + current streak (streak is server-computed; the
/// local server returns 0, matching how the macOS pet reads it against the same backend).</summary>
```
注释**同时写明**「the local server returns 0」，并不存在"注释与事实矛盾"的意味。建议改为「注释自述 streak 由服务端计算并明确本地返回 0」。

### 建议修（5 条，均为引用/措辞校准）

- **L1**：§9.5.3 表 16 类、21 处缩写文件名缺 `tokentracker-` 前缀（见 A.4）。
- **L2**：§10.1⑱ `AUTO_SYNC_SOURCES` 成员顺序与源码不一致：文档 `… reasonix, roocode, trae, trae-cn, unsloth …`，源码 `:320-366` 为 `… reasonix, roocode, trae-cn, trae, unsloth …`（成员集合 45 项一致，仅顺序）。若按「逐项读取」的措辞，建议照源码顺序重排。
- **L3**：§9.2.3 表 `-user-status` 返回字段漏 `created_at`（`local-api.js:2923`）；`pro` 对象还有 `partial`/`as_of`（`:2924`），`account` 含 `available`/`cloud_sync_enabled`/`account_view`（`:2927-2931`）——建议表内改为「`user_id,email,name,is_public,created_at,pro{},account{}`」。
- **L4**：§9.6.3 `ServerManager.cs:12-20` 选点指向类注释，建议改 `:35`/`:38`。
- **L5**：§9.6.3 `ServerManager.cs:694,700` 中 694 是 `TOKENTRACKER_APP_SHELL`，建议写 `:698-700`。

---

## 复核证据抽样

**M1 关键链（三处）**
```
use-activity-heatmap.ts:174-179   const hasLevels = weeksData.some((week:any) => (Array.isArray(week)?week:[]).some((cell:any)=> cell && Number.isFinite(Number(cell.level)))); if (!hasLevels && weeksData.length) {
use-activity-heatmap.ts:206        streak_days: computeActiveStreakDays({ dailyRows: rows, to: res?.to || range.to }),
use-activity-heatmap.ts:234-235    if (!isCurrent()) return; setHeatmap(res || null);
local-api.js:2566                  cells.push({ day, total_tokens: data?.total_tokens || 0, billable_total_tokens: billable, level: calcLevel(billable), models: data?.models || null });
local-api.js:2587-2588             active_days: cells.filter((c) => c.billable_total_tokens > 0).length, streak_days: 0,
DashboardView.jsx:235              streakDays={activeDays}
StatsPanel.jsx:167-168             <span>{copy("identity_card.streak_label")}</span> <span …>{streakValue}</span>
```

**M2 关键链**
```
local-api.js:1922                  const allowRelayCapture = proxyRes.status < 400;
local-api.js:1926-1929             if (k.toLowerCase() === "set-cookie") { const rewritten = v.replace(/;\s*[Dd]omain=[^;]*/g, "; Domain=localhost"); if (allowRelayCapture) captureSetCookies(rewritten); return [k, rewritten]; }
local-api.js:1943-1950             if (isStaleCsrf403 && proxyRes.status === 403 && injectedRelayCookies && !hasClientCookie) clearRelayCookies("stale refresh cookie without local CSRF context");
```

**27 端点定义行清点（脚本输出，节选）**
```
2137 /functions/tokentracker-cloud-session      2166 /functions/tokentracker-local-sync
2330 /functions/tokentracker-wrapped            2358 /functions/tokentracker-usage-summary
2425 usage-daily   2441 outcomes   2470 session-insights   2500 sessions   2526 context-health
2533 usage-heatmap 2596 usage-model-breakdown  2668 usage-category-breakdown
2731 project-usage-summary  2828 project-usage-detail  2920 user-status  2940 cloud-sync-pref
2982 proxy-config  3074 proxy-test  3121 telemetry-pref  3137 machine-id  3146 usage-hourly
3156 usage-monthly 3192 pets  3240 subscription-manager  3294 skills  3470 achievements  3482 usage-limits
```

**「8 处写点」「13 项判据」复算**
```
select-string appendFile:
  src\lib\rollout.js:3483   await fs.appendFile(queuePath, toAppend.join("\n") + "\n", "utf8");
  src\lib\rollout.js:3536   await fs.appendFile(projectQueuePath, toAppend.join("\n") + "\n", "utf8");
  src\lib\rollout.js:20031  await fs.appendFile(queuePath, `${lines.join("\n")}\n`, "utf8");
  src\lib\rollout.js:22594  await fs.appendFile(queuePath, lines.join("\n") + "\n", "utf8");
  src\commands\sync.js:4114 await fs.appendFile(fallbackFilePath, `${JSON.stringify(syntheticMessage)}\n`, "utf8");
  src\commands\sync.js:5126 await fs.appendFile(queuePath, `${repairLines.join("\n")}\n`, "utf8");
  src\commands\sync.js:5207 await fs.appendFile(queuePath, retractions.join("\n") + "\n");
  src\commands\sync.js:5281 await fs.appendFile(queuePath, retractions.join("\n") + "\n");
sync.js:3424-3437  13 个 && 判据（opts.auto … !opts.repairGrok）
```

**repair 事故数字**
```
sync.js:239-240  // the reporter's machine this wiped 2.17B claude tokens (-1.27B opus-4-7,
                 // -474M opus-4-6, -376M sonnet-4-5, -48M haiku, -6M sonnet-4-6).
求和：1270M + 474M + 376M + 48M + 6M = 2174M = 2.174B ≈ 2.17B（文档「21.7 亿」= 21.74 亿，口径一致）
```

**限额 provider 计数**
```
usage-limits.js:4055  const [claudeResult, codexResult, cursor, kimi, gemini, kiro, antigravity, copilot, grok, zcode, opencodeGoRaw, qoder, qoderCn, codingPlan, agentPlan, commandCodeRaw, devinRaw, claudeServiceStatus] = await Promise.all([   ← 18
usage-limits.js:4406-4440  const data = { fetched_at, claude, codex, cursor, kimi, gemini, kiro, antigravity, copilot, grok, zcode, opencodeGo, commandCode, devin, qoder, qoderCn, codingPlan, agentPlan };  ← 17 家
usage-limits.js:4279  if (claude.configured && claudeServiceStatus && claudeServiceStatus.indicator !== "none") claude.service_status = claudeServiceStatus;
```

**价格表计数（Node 复算）**
```
curated-overrides.json  exact=84  fuzzy=47  alias=1  source_exact.acode=64
seed-snapshot.json      models=2284  cache_read=566  cache_creation=188
                        _meta.generated_at=2026-05-26T11:12:29.761Z  kept_models=2284
```

**统计口径原始输出**：`%TEMP%\refs_filled.csv`（730 条引用逐条归属与行号）、`%TEMP%\refs_s9_s10.tsv`（原始抽取）、文件行数对照与端点清点为本次会话内联脚本。
