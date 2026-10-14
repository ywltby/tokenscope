# OPEN-W2：原生壳、本地 API、SQL 与 dev 中间件存疑攻坚

- **对象**：样本仓库 `C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）
- **方法**：纯静态阅读 + 检索；未修改样本仓库、未执行仓库脚本；行号口径为 `read` / `[System.IO.File]::ReadAllLines($p,[Text.Encoding]::UTF8)`
- **对应主文档**：§12 B 类 B6/B7/B8/B9、补充登记（macOS 小组件刷新与快照迁移、`QuotaWidgetWindow.cs` 数据来源）、C 类 C13；另复核 §L192 的「`logs/` 无创建点」条目
- **判定分布**：**已解决 8 条**（1、2、3、4、5、6、7、9；其中第 4 条含一处**前提推翻**）、**部分解决 1 条**（8：迁移已解决、系统刷新频率缺运行时证据）、**维持未确认 0 条**
- **证据强度约定**：每条结论均可被源码逐字支持；凡属推断（如 PostgreSQL 触发器语义、构建产物行为）在正文内单独标注，不混入结论句

---

## 1 Windows 壳「5 分钟后台 sync 计时器」（主文档 B7）

- **判定**：已解决
- **结论**：计时器在壳内、不在 serve 内：`TokenTrackerWin/TrayApplicationContext.cs:84` 定义 `_syncTimer` 间隔 `5 * 60 * 1000`，Tick 绑 `TriggerBackgroundSync`（`:240`）；只在本地服务状态转 `Running` 时启动并**立即触发一次**（`:965-971`），`Failed` 时停止（`:972-974`），Dispose 时停止并释放（`:1186,1198`）。壳启动 serve 子进程时同批注入 `TOKENTRACKER_APP_SHELL=windows` 与 `TOKENTRACKER_NATIVE_SYNC_OWNER=windows-host`（`ServerManager.cs:694,700`，仅对 `serve` 且非 native-only WSL 分支），使 serve 侧一分种兜底自禁用——`src/commands/serve.js:305-309` 注释三句（托盘自持 5 分钟计时器／接收完成事件／并行会造成两个独立 sync 生产者争 `sync.lock`）**逐句属实**。**一处口径修正**：serve 侧兜底只对 Windows 壳存在（`serve.js:304` `if (normalizedShell !== "windows") return null;`），macOS（`TokenTrackerBar/TokenTrackerBar/Services/ServerManager.swift:203`）与 Linux（`TokenTrackerLinux/src-tauri/src/server.rs:700`）只设 `TOKENTRACKER_APP_SHELL=macos|linux`，其 serve 进程**根本不会**进入该兜底分支。
- **证据**：

`TokenTrackerWin/TrayApplicationContext.cs:84`、`:239-241`、`:965-974`
```
 84:     private readonly System.Windows.Forms.Timer _syncTimer = new() { Interval = 5 * 60 * 1000 };
240:         _syncTimer.Tick += (_, _) => TriggerBackgroundSync();
965:             if (status == ServerManager.ServerStatus.Running)
966:             {
967:                 _poller.Start();   // begin (or resume) polling once the server answers
968:                 _poller.RefreshNow();
969:                 _syncTimer.Start();
970:                 TriggerBackgroundSync();
972:             else if (status == ServerManager.ServerStatus.Failed)
973:             {
974:                 _syncTimer.Stop();
```

`TokenTrackerWin/ServerManager.cs:694-700`、`：85-92`（完成事件）
```
694:         psi.Environment["TOKENTRACKER_APP_SHELL"] = "windows";
698:         if (!forceNativeOnlyWslMode && args.Length > 0 &&
699:             string.Equals(args[0], "serve", StringComparison.OrdinalIgnoreCase))
700:             psi.Environment["TOKENTRACKER_NATIVE_SYNC_OWNER"] = "windows-host";
 89:     public event Action? SyncStarted;
 92:     public event Action? SyncCompleted;
```

`src/commands/serve.js:35`、`:304-309`、`:314-315`
```
 35: const NATIVE_BACKGROUND_SYNC_INTERVAL_MS = 60_000;
304:   if (normalizedShell !== "windows") return null;
309:   if (String(nativeSyncOwner || "").trim().toLowerCase() === "windows-host") return null;
314:   // endpoint) once a minute. sync.lock is file-based, so the child contends
315:   // with hook-fired syncs exactly like an in-process run did.
```

- **补强证据（争锁机制与单飞闸）**：`src/commands/sync.js:552` `const lockPath = path.join(trackerDir, "sync.lock");`（锁确实按 trackerDir 落在文件上）；`ServerManager.cs:83` `BackgroundSyncDeadline = TimeSpan.FromMinutes(5)` 与 `:212-228` 的 `_syncInFlight` 单飞；完成事件回流 UI：`TrayApplicationContext.cs:234-235` 订阅，`:996-1006` 的 `OnSyncCompleted` 调 `_poller.RefreshNow(forceAccount: false)`（`:1000`）——注释「receives completion events for the UI」属实。
- **残余边界（不降级判定）**：注释给出的**后果**「refreshes stale and needlessly expensive」属运行时表现，静态不可证；但两条生产者确实独立存在（壳 5 分钟 + serve 1 分钟兜底）且共用一个文件锁，机制成立。另一处注释未提的副作用已在源码内写明：`BackgroundSyncDeadline` 被持有时 `StartDirectSync`（托盘「Sync Now」）在 `_backgroundSyncCts` 闸上静默空转（`ServerManager.cs:73-83,242`）。

---

## 2 `TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源（主文档 B 类补充登记）

- **判定**：已解决
- **结论**：**数据来自本地 API，但由 `UsagePoller` 通过托盘宿主推送进组件**——两条说法不冲突，链路上只有一个取数点。组件自身不抓取任何端点：它只做 WebView2 导航到 `${BaseUrl}/quota.html?app=1`（`QuotaWidgetWindow.cs:389-394`）与 postMessage 注入。完整调用链：`UsagePoller.FetchLimitsAsync` → `GET /functions/tokentracker-usage-limits`（`UsagePoller.cs:182-195`，取数在 `:187`）→ `RaiseLimitsUpdated`（`:155`）→ `TrayApplicationContext.OnLimitsUpdated`（`:1014-1024`）→ `_quotaWindow.ApplyLimits(json)`（`:1022`）→ 组件 `ApplyLimits`（`:126-131`）→ `PushContext` 发 `{type:"quota:context", limits, failed, selected, locale, light}`（`:396-409`）。前端 `quota.jsx` 只监听 `quota:context`、只发 `quota:ready` 等控制消息，**全文零 `fetch`**（`quota.jsx:17-27,71`）。轮询开关由可见性决定：`IncludeLimits = 宠物可见 || 配额组件启用`（`TrayApplicationContext.cs:286`）。
- **证据**：

`TokenTrackerWin/UsagePoller.cs:182-195`、`:150-158`
```
187:                 _baseUrl() + "/functions/tokentracker-usage-limits", cancellationToken);
144:                 var includeLimits = IncludeLimits;
147:                 var limitsTask = includeLimits ? FetchLimitsAsync(token) : null;
155:                         if (limits is not null) RaiseLimitsUpdated(limits);
156:                         else LimitsFailed?.Invoke();
```

`TokenTrackerWin/QuotaWidgetWindow.cs:389-394`、`:126-131`、`:396-409`、`:244`
```
393:             core.Navigate(_server.BaseUrl + "/quota.html?app=1");
244:             case "quota:ready": _ready = true; ResizeWidget(false); _lastContext = null; PushContext(); break;
126:     public void ApplyLimits(string json)
127:     {
128:         try { _limits = JsonNode.Parse(json); _failed = false; }
129:         catch (JsonException) { _failed = true; }
130:         PushContext();
401:             type = "quota:context", limits = _limits, failed = _failed,
```

`dashboard/src/quota.jsx:17-27`（前端零取数）
```
 18:     const receive = event => {
 19:       if (event.data?.type !== "quota:context") return;
 21:       setContext(previous => ({ ...event.data, limits: mergeQuotaSnapshot(previous.limits, event.data.limits) }));
 23:     window.chrome?.webview?.addEventListener("message", receive);
 24:     send("quota:ready");
 25:     const timer = setInterval(() => setNow(Date.now()), 30000);
```

- **附带确认**：失败态也不来自组件自己的请求，而是 `LimitsFailed` 事件 → `MarkFailed()`（`TrayApplicationContext.cs:238`、`QuotaWidgetWindow.cs:151-155`）。

---

## 3 `~/.tokentracker/logs/` 是否存在创建点（主文档 §L192 条目）

- **判定**：已解决（否定结论）
- **结论**：**仓库内不存在 `~/.tokentracker/logs/` 的创建点或写入点**。路径解析器只产出四个目录，没有 `logs`：`src/lib/tracker-paths.js:5-11` 返回 `rootDir` / `trackerDir` / `binDir` / `cacheDir`。全库检索 `logs` 相关常量只命中各 provider 自己的日志目录（CodeBuddy `logs`、ZCode `v2/logs`、Qoder `logs`、Antigravity `.system_generated/logs`、LM Studio `server-logs`），没有一条挂在 `.tokentracker` 下。三个原生壳的日志各落别处：Windows `%LOCALAPPDATA%\TokenTracker\windows-host.log`（`TokenTrackerWin/Diag.cs:11-13`）；Linux `$XDG_STATE_HOME/tokentracker/server.log` → `$HOME/.local/state/tokentracker/server.log` → `/tmp/tokentracker-server.log`（`TokenTrackerLinux/src-tauri/src/server.rs:587-604`，另有 `servers/` 子目录 `:116`）；macOS 走 `os.Logger` 统一日志、不落普通日志目录（`TokenTrackerBar/TokenTrackerBar/Services/WidgetSnapshotWriter.swift:16`、`Utilities/DateHelpers.swift:6`、`ViewModels/DashboardViewModel.swift:35`，另有 1 处 `NSLog`：`Services/DashboardWindowController.swift:573`）。macOS 的 `EmbeddedServer` 在仓库内**没有源码**（`**/EmbeddedServer/*` glob 零文件），故「以仓库为准」的判定可以闭环：主文档该条可从「未能确认」改为「以仓库为准不存在」。
- **证据**：

`src/lib/tracker-paths.js:4-11`
```
 4: async function resolveTrackerPaths({ home = os.homedir() } = {}) {
 5:   const rootDir = path.join(home, ".tokentracker");
 6:   return {
 7:     rootDir,
 8:     trackerDir: path.join(rootDir, "tracker"),
 9:     binDir: path.join(rootDir, "bin"),
10:     cacheDir: path.join(rootDir, "cache"),
11:   };
```

`TokenTrackerWin/Diag.cs:11-13`
```
 11:     private static readonly string LogPath = Path.Combine(
 12:         Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
 13:         "TokenTracker", "windows-host.log");
```

`TokenTrackerLinux/src-tauri/src/server.rs:587-604`
```
587: /// Candidate log paths in preference order: `$XDG_STATE_HOME/tokentracker/`,
588: /// then `$HOME/.local/state/tokentracker/`, then `/tmp`.
592:         paths.push(state_home.join("tokentracker").join("server.log"));
598:             .join("tokentracker")
604:     paths.push(PathBuf::from("/tmp").join("tokentracker-server.log"));
```

- **残余不确定性（不影响判定）**：发行包内嵌的构建产物（非仓库文件）理论上可自行创建该目录；但本轮判定条件是「读三个壳源码检索日志目录常量」，该项已完成且为零命中。

---

## 4 本地 API 端点字段结构（主文档 B8：三问三答，其中一问前提被推翻）

- **判定**：已解决（`syncResult` 键名一项**前提不成立**，已给替代结论）

### 4a `/functions/tokentracker-local-sync` 成功响应

- **结论**：**全库不存在 `syncResult` 键**（`syncResult` 仅在 `dashboard/src/lib/cloud-sync.test.ts:9` 作为测试局部变量名出现，生产代码零命中）。成功响应是把同步子进程结果 **展开在顶层** 再加两个字段：`json(res, { ok: true, ...result, ...cloudAccess })` —— `result` 来自 `runSyncCommand` 的 resolve 值 `{ code, stdout, stderr }`（`local-api.js:794-796`），`cloudAccess` 初始为 `{ uploaded: false, pending: true }`，能从 `upload.throttle.json` / `queue.state.json` 读出时补 `next_allowed_at`（ISO）并按「本批 attemptId 是否记为本轮成功」重算 `uploaded`、按队列偏移重算 `pending`（`:2313-2321`）。故成功形态 = `{ ok: true, code, stdout, stderr, uploaded, pending, next_allowed_at? }`；失败形态见 `:2324`（`ok:false, error, code, recovery_url, next_allowed_at, retry_after_seconds, stdout, stderr`）。前端消费侧逐字对应：`{ok, code, stdout, stderr}`（`cloud-sync.ts:67,90`）与 `next_allowed_at` / `uploaded` / `pending`（`:159-162,180-183`）。授权：POST + `x-tokentracker-local-auth` 头（或 `tokentracker_local_auth` cookie）等于进程内 `localAuthToken` 且 origin/referer 为 loopback（`:1629-1637`、`:691-703`）；壳侧发送体为 `{"auto":true,"background":true,"allLocalSources":true,"publishAccount":true,"nativeOnlyWsl":true}`（`TokenTrackerWin/LocalSyncPublisher.cs:15-16,42-44`），逐一映射到 `runSyncCommand` 的 `--auto/--background/--all-local-sources/--publish-account` 与 `TOKENTRACKER_WSL_MODE=native-only`（`local-api.js:747-752`、`:2202-2204`）。
- **证据**：

`src/lib/local-api.js:2313-2322`
```
2313:         let cloudAccess = { uploaded: false, pending: true };
2316:           if (state.nextAllowedAtMs) cloudAccess.next_allowed_at = new Date(state.nextAllowedAtMs).toISOString();
2317:           cloudAccess.uploaded = state.lastSuccessAttemptId === extraEnv.TOKENTRACKER_LOCAL_SYNC_ATTEMPT_ID &&
2318:             Number(state.lastSuccessMs || 0) >= syncStartedAt;
2320:           cloudAccess.pending = fs.statSync(qp).size > Number(queueState.offset || 0);
2322:         json(res, { ok: true, ...result, ...cloudAccess });
```

`src/lib/local-api.js:791-796`（`result` 的形状）
```
794:       const r = { code: code ?? 1, stdout: trimOutput(stdout), stderr: trimOutput(stderr) };
795:       if (code === 0) {
796:         finish(resolve, r);
```

### 4b `/functions/tokentracker-context-health` 返回结构

- **结论**：该端点**直接回写 `computeContextHealth(...)` 的返回值**，无包装、无额外键（`local-api.js:2526-2530`）。结构：`generated_at`(ISO)、`estimated_fixed_tokens`(整数)、`severity`(「high」≥50000 / 「medium」≥20000 / 否则「low」)、`breakdown{instruction_files, instruction_tokens, skills, skill_tokens, mcp_servers, mcp_schema_tokens}`、`largest_items`（按估算 token 倒序取前 **10**，元素 `{kind, name, bytes, estimated_tokens}`，`kind ∈ {instruction, skill}`）、`provenance{confidence:"inferred", source:"local-config-metadata", mcp_assumption:{tools_per_server, tokens_per_tool_schema}, content_retained:false}`。两个 MCP 假设值的默认与覆盖来源：`TOKENTRACKER_MCP_TOOL_SCHEMA_TOKENS`（默认 400）、`TOKENTRACKER_MCP_TOOLS_PER_SERVER`（默认 5，下限 1），`mcp_schema_tokens = mcp_servers × per_tool × per_server`。进程内缓存 5 分钟且最多 8 键（`context-health.js:64,67,119-120`）。
- **证据**：

`src/lib/local-api.js:2526-2530`
```
2526:     if (p === "/functions/tokentracker-context-health") {
2527:       const { computeContextHealth } = require("./context-health");
2528:       json(res, computeContextHealth({ home: os.homedir(), cwd: process.cwd(), env: process.env }));
```

`src/lib/context-health.js:91-118`
```
 94:   const perToolSchemaTokens = Math.max(0, Number(env.TOKENTRACKER_MCP_TOOL_SCHEMA_TOKENS || 400));
 95:   const assumedToolsPerServer = Math.max(1, Number(env.TOKENTRACKER_MCP_TOOLS_PER_SERVER || 5));
 96:   const mcpSchemaTokens = mcpServers * perToolSchemaTokens * assumedToolsPerServer;
 98:   const severity = total >= 50_000 ? "high" : total >= 20_000 ? "medium" : "low";
 99:   const result = {
100:     generated_at: new Date().toISOString(),
101:     estimated_fixed_tokens: total,
102:     severity,
103:     breakdown: {
111:     largest_items: files.sort((a, b) => b.estimated_tokens - a.estimated_tokens).slice(0, 10),
112:     provenance: {
113:       confidence: "inferred",
114:       source: "local-config-metadata",
```

### 4c `/api/auth/*` 完整转发规则

- **结论**：转发规则共 8 条，全部在 `local-api.js:1644-1657` 与 `:1792-1955` 内可逐字核对。①**入口预检**（先于 OPTIONS）：`origin`/`referer` 必须是 `http:` 且主机 ∈ {127.0.0.1, localhost, ::1}，或 `sec-fetch-site: cross-site` → 403 `untrusted_auth_origin`；预检通过才放行 OPTIONS（允许 `GET, POST, OPTIONS` 与 `Content-Type, Authorization`）。②**cookie relay**：浏览器自带 cookie 存在时用「本实例捕获的 relay 集」**覆盖**，不存在时 cookie 头置空；最终 merged 为空则删除该头。③**csrf 优先级**：客户端 `x-csrf-token` 与 relay 捕获的 csrf 值不等 → 直接删除；`/api/auth/refresh` 且 relay csrf 存在且不在回退分支 → **强制改用 relay csrf**。④**relay 注入条件**：`p !== "/api/auth/refresh" || hasClientCookie || hasEffectiveCsrfHeader`。⑤**relay 回退**（cookie-less 客户端）：`/api/auth/refresh` + 无客户端 cookie + 存在持久化 `insforge_refresh_token` → 改打 `${base}/api/auth/refresh?client_type=mobile`，body 换成 `{refresh_token}`、`content-type: application/json`，并**禁用** relay cookie 注入。⑥**stale-CSRF 救援**：refresh 得 403 且响应体匹配 `/invalid csrf token/i` 时，去掉 cookie 与 csrf 头用 mobile 流重放一次，成功则替换响应。⑦**响应侧**：`set-cookie` 的 `Domain=` 一律重写为 `Domain=localhost`，且**仅 2xx 才 `captureSetCookies`**（3xx/4xx 的删除型 set-cookie 不得破坏仍在有效期内的 relay 会话）；2xx 时 `logout` 清空 relay、其余分支从响应体捕获 token（`newLogin = p !== "/api/auth/refresh"`）；若最终仍是上述 stale-CSRF 403 且本轮确实注入了 relay cookie 且客户端没带 cookie → 清 relay cookie。⑧**一致性闸门**：`apikey` 必须等于 runtime `anonKey`（否则 409 `instance_changed`）；非官方 base + 带 bearer 但无 `x-tokentracker-instance` → 409；转发前后 runtime 的 baseUrl/anonKey 变化 → 409 `instance_changed`；refresh/logout 期间账号会话代际变化 → 409 `auth_session_changed`。
- **证据**：

`src/lib/local-api.js:1644-1657`（入口预检）
```
1644:     if (p.startsWith("/api/auth/") || nativeMarkerRead) {
1645:       if (!hasAllowedLoopbackOrigin(req.headers || {}) || req.headers?.["sec-fetch-site"] === "cross-site") {
1646:         json(res, { code: "untrusted_auth_origin", error: "Authentication requests must come from the local dashboard." }, 403);
1649:       if (String(req.method).toUpperCase() === "OPTIONS") {
```

`src/lib/local-api.js:1809-1841`（cookie relay / csrf 优先级 / 回退条件）
```
1811:         const hadBrowserCookies = Boolean(normalizeCookieHeader(proxyHeaders.cookie).trim());
1812:         proxyHeaders.cookie = hadBrowserCookies ? buildRelayCookieHeader("") : "";
1813:         if (proxyHeaders["x-csrf-token"] !== getRelayCookieValue(csrfRelayCookieName)) delete proxyHeaders["x-csrf-token"];
1829:         const shouldUseRelayRefreshFallback =
1830:           p === "/api/auth/refresh" && !hasClientCookie && relayRefreshToken;
1831:         if (p === "/api/auth/refresh" && relayCsrfToken && !shouldUseRelayRefreshFallback) {
1832:           proxyHeaders["x-csrf-token"] = relayCsrfToken;
1837:         let shouldInjectRelayCookies =
1838:           p !== "/api/auth/refresh" || hasClientCookie || hasEffectiveCsrfHeader;
1839:         if (shouldUseRelayRefreshFallback) {
1840:           shouldInjectRelayCookies = false;
```

`src/lib/local-api.js:1864-1869`、`:1884-1907`、`:1922-1942`
```
1865:           effectiveTargetUrl = `${insforgeBase.replace(/\/$/, "")}/api/auth/refresh?client_type=mobile`;
1868:           proxyBody = Buffer.from(JSON.stringify({ refresh_token: relayRefreshToken }), "utf8");
1884:         const isStaleCsrf403 =
1885:           p === "/api/auth/refresh"
1886:           && proxyRes.status === 403
1887:           && /invalid csrf token/i.test(resBody.toString("utf8"));
1888:         if (isStaleCsrf403 && relayRefreshToken && !shouldUseRelayRefreshFallback) {
1890:           delete rescueHeaders["cookie"];
1891:           delete rescueHeaders["x-csrf-token"];
1922:         const allowRelayCapture = proxyRes.status < 400;
1927:               const rewritten = v.replace(/;\s*[Dd]omain=[^;]*/g, "; Domain=localhost");
1928:               if (allowRelayCapture) captureSetCookies(rewritten);
1935:           if (p === "/api/auth/logout") {
1936:             clearRelayCookies("sign out");
```

- **口径说明（供主文档改写）**：主文档 B8 原文把 local-sync 的响应写成「`syncResult` 键名」，实为**顶层展开**（无该键）；引用时应改为「子进程结果三元组 `code/stdout/stderr` + 云端访问三字段」。

---

## 5 `usage-limits` 的 provider 数量口径（主文档 B6）

- **判定**：已解决
- **结论**：**18 个并发位 ↔ `data` 17 家**的关系成立，差额的去向已定位——`claudeServiceStatus` 不是独立 provider，而是挂到 `claude.service_status` 的**子字段**，且仅在 `claude.configured === true` 且 `indicator !== "none"` 时挂载（`usage-limits.js:4279-4280`）；状态探针本身只对已配置 Claude 账号发起，无 token 时直接 `Promise.resolve(null)`（`:4183-4185`）。`data`（`:4406-4440`）= `fetched_at` + claude、codex、cursor、kimi、gemini、kiro、antigravity、copilot、grok、zcode、opencodeGo、commandCode、devin、qoder、qoderCn、codingPlan、agentPlan = **17 家**。**Ark 两家共用同一凭据**（同一 `arkcli` 二进制、同一命令 `usage plan --format json`、同一 `~/.arkcli` 配置目录族，仅按 `product` 过滤区分）；**Qoder 两家不共用**（appDir / envPrefix / 手工 cookie 键 / 缓存命名空间 / CLI 凭据模块全部分离）。源码不可判的仅剩「两家 Ark 与两家 Qoder 是否指向同一账号」这一账号层面的同一性。
- **证据**：

`src/lib/usage-limits.js:4055`（18 位解构）、`:4279-4280`（`claudeServiceStatus` 去向）
```
4055:   const [claudeResult, codexResult, cursor, kimi, gemini, kiro, antigravity, copilot, grok, zcode, opencodeGoRaw, qoder, qoderCn, codingPlan, agentPlan, commandCodeRaw, devinRaw, claudeServiceStatus] = await Promise.all([
4279:   if (claude.configured && claudeServiceStatus && claudeServiceStatus.indicator !== "none") {
4280:     claude.service_status = claudeServiceStatus;
```

`src/lib/ark-coding-plan-limits.js:410-413`、`:65`、`:297-301` 与 `src/lib/ark-agent-plan-limits.js:330-333`、`:51`、`:257-261`
```
（coding）413:     ["usage", "plan", "--format", "json"],
（coding） 65: const ARK_PRODUCT_PRIORITY = ["coding-plan"];
（coding）297:     path.join(home, ".arkcli"),
（coding）298:     path.join(home, ".config", "arkcli"),
（coding）301:     candidates.push(path.join(home, "AppData", "Roaming", "arkcli"));
（agent） 333:     ["usage", "plan", "--format", "json"],
（agent）  51: const ARK_PRODUCT_PRIORITY = ["agent-plan"];
（agent） 120:     const item = items.find((entry) => entry?.product === product && entry?.subscribed === true);
```

`src/lib/qoder-limits.js:819-827`（CN 分离口径）
```
819: // CN has its own CLI credentials, legacy desktop root and cache namespace.
820: function fetchQoderCnLimits(opts = {}) {
823:     appDir: "QoderCN",
824:     envPrefix: "QODER_CN",
825:     site: QODER_SITES.china,
827:     cacheNamespace: "cn",
```

- **附带口径**：`data` 里的 `provenance` 是统一后加的派生字段（`source`/`confidence`/`captured_at`/`stale`/`age_seconds`，`usage-limits.js:4442-4461`），其中 `ageMs > 10 * 60 * 1000` 会强判 `stale` —— 面板「未过期」窗口是 10 分钟，与 Claude 侧 5 分钟磁盘缓存（`:4044-4052`）不是同一口径，若主文档要写「新鲜度」应分开表述。

---

## 6 榜单/社区表 SQL（主文档 B9）

- **判定**：已解决（三项追问全部有源码答案；另附 5 项**仓库外**未确认，不影响 B9 结案）
- **总口径（一句话）**：没有 SQL 视图；链路是「`tokentracker_hourly` → 小时级设备/机器簇去重函数 `leaderboard_hourly_dedup_v2` → UTC 日物化表 `tokentracker_leaderboard_rollup_daily_v2` → 全时段物化表 `..._rollup_total_v2` → RPC `leaderboard_usage_grouped{,_total_shard}` / `leaderboard_usage_compact*` 返回 jsonb」；**聚合层不查反作弊表**，异常排除走四条外部路径（封禁物理搬移/删除、快照协调删除、社区统计整账号排除、edge 内存过滤）。

### 6.1 排行榜按天 / 按设备聚合

- **结论**：**设备维度只出现在小时级去重，最终榜单不含设备。** 小时级键 = `(user_id, COALESCE(dm.machine_cluster_id, device_id::text), source, model, hour_start)`：先在**同一物理机器簇内**用 `DISTINCT ON ... ORDER BY total_tokens DESC, updated_at DESC` 取「整行规范行」（不是逐列 MAX，以保成本内部一致），再跨簇 `SUM(...)` 相加（注释明说「两台真正不同机器跑同一模型同一半小时只算一次，远好于系统性 ~2x」）。日物化表主键 `(user_id, source, model, day)`，日按 UTC 截断；全时段表键 `(user_id, source, model, pricing_tier)`。账号级来源（`cursor`、`trae-cn`）不走设备过滤：`cursor` 整行 MAX 去重、`trae-cn` 改从 `tokentracker_account_session_states` 按 `(user_id, source, model, bucket_start)` SUM——注释明确 **trae-cn 的 hourly 行永不进榜**。
- **证据**：

`migrations/20260817120000_account-session-states.sql:166-172`（口径注释）、`:202-238`、`:248`（当前生效版）
```
166: -- Leaderboard: account-level sources now aggregate from SESSION STATES (the
167: -- canonical cross-device truth), machine-level sources keep the cluster
168: -- dedup. trae-cn hourly rows (per-device local reconciliations) never enter
169: -- the leaderboard - the session-state branch replaces both the previous
202:   WITH cfg AS (
203:     SELECT ARRAY['cursor', 'trae-cn']::text[] AS account_sources
215:     SELECT DISTINCT ON (
216:       h.user_id,
217:       COALESCE(dm.machine_cluster_id, h.device_id::text),
233:     JOIN tokentracker_devices d
234:       ON d.id = h.device_id AND d.revoked_at IS NULL
238:       AND NOT (h.source = ANY(cfg.account_sources))
245:     h.total_tokens DESC, h.updated_at DESC
```

`migrations/20260804043427_align-leaderboard-machine-clusters.sql:1-13`（日表形状）、`migrations/20260904064000_cache-leaderboard-total-rollup.sql:11-22`（总表形状）
```
  1: CREATE TABLE IF NOT EXISTS public.tokentracker_leaderboard_rollup_daily_v2 (
  5:   day date NOT NULL,
 12:   PRIMARY KEY (user_id, source, model, day)
 11: CREATE TABLE IF NOT EXISTS public.tokentracker_leaderboard_rollup_total_v2 (
 15:   pricing_tier text NOT NULL,
 22:   PRIMARY KEY (user_id, source, model, pricing_tier)
```

### 6.2 去重与异常过滤落在哪

- **结论**：**榜单 SQL 层不读 `tokentracker_leaderboard_anomaly_flags`**——聚合路径上的过滤只有两条：设备未吊销（`d.revoked_at IS NULL`，仅 machine 分支）与账号级来源 `NOT (source = ANY(account_sources))`。反作弊结果通过**四条外部路径**影响榜单：①**封禁迁移**（判定条件按 `status='auto_excluded'` + 检测时间窗 + `cohort_ratio >= 10`，先**整行拷进** `tokentracker_hourly_quarantine` 且行数必须相等否则 `RAISE`，再删 `tokentracker_hourly`（按 user）、`..._rollup_daily_v2`、`tokentracker_leaderboard_snapshots`、`agentmeter_leaderboard_snapshots`，最后把 flag 置 `status='banned'`）；②**快照协调函数**按 `status IN ('auto_excluded','banned')` 删快照行并用队列 CAS（`last_queue_changed_at` 不匹配即返回 `applied=false`）；③**社区统计**按 `status IN ('auto_excluded','banned')` 整账号 `NOT EXISTS` 排除；④**edge 刷新**在内存 aggMap 上删 `status='auto_excluded'` 的用户（失败 fail-open，`dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:1202-1231`）。误报清除迁移只 `UPDATE ... status='cleared'`，不删任何用量行。**另一个口径要点**：榜单聚合链**完全不读 `billable_total_tokens`**（该列在 `migrations/` 只出现在隔离拷贝、归档与云端去重三处，未见任何 dedup/rollup/grouped 函数引用）——主文档若把榜单口径与 `billable_total_tokens` 绑定需改。
- **证据**：

`migrations/20260821060000_ban-confirmed-leaderboard-manipulation.sql:19-23`、`:49-78`、`:98-111`
```
 19:   WHERE f.status = 'auto_excluded'
 20:     AND f.reviewed_at IS NULL
 21:     AND f.detected_at >= '2026-08-20 23:41:00+00'::timestamptz
 22:     AND f.detected_at <  '2026-08-20 23:42:00+00'::timestamptz
 23:     AND f.cohort_ratio >= 10;
 49:   INSERT INTO public.tokentracker_hourly_quarantine (
 79:   IF v_quarantined_rows <> v_hourly_rows THEN
 98:   DELETE FROM public.tokentracker_leaderboard_rollup_daily_v2 r
102:   DELETE FROM public.tokentracker_leaderboard_snapshots s
111:   SET status = 'banned',
```

`migrations/20260825053000_reconcile-anticheat-snapshot-exclusions.sql:39-49`、`docs/ops/leaderboard-anomaly-detector-runtime.sql:5-11`
```
 39:   DELETE FROM public.tokentracker_leaderboard_snapshots s
 44:       AND f.status IN ('auto_excluded', 'banned')
 48:   UPDATE public.tokentracker_anticheat_run_state
  5: ALTER FUNCTION public.detect_leaderboard_anomalies()
  6:   SET work_mem TO '16MB';
  7: ALTER FUNCTION public.detect_leaderboard_anomalies()
  8:   SET statement_timeout TO '45s';
```

`migrations/20260817120000_account-session-states.sql:205-213`（簇内先取整行、跨簇再 SUM）
```
205:   -- Deduplicate device-id drift/replays inside one physical machine cluster,
206:   -- then add genuinely distinct machines for the same user/hour/model.
207:   SELECT mac.user_id, mac.source, mac.model, mac.hour_start,
208:     SUM(mac.total_tokens)::bigint                AS total_tokens,
```

### 6.3 社区统计（Global Metrics）

- **结论**：缓存表 `tokentracker_community_stats` 是**单行表**（`id text PRIMARY KEY CHECK (id = 'total')`），由 `refresh_tokentracker_community_stats()` 计算，**pg_cron 每小时 17 分**触发（调度刻意不放进可移植迁移）。当前生效版（2026-09-23）读 `..._rollup_daily_v2` 的**闭合日** + `leaderboard_hourly_dedup_v2` 的活尾，并做三重过滤：`model <> ''`、`lower(model) <> 'auto'`、整账号 `NOT EXISTS`（`status IN ('auto_excluded','banned')`）；注释给出两条动因（无 flag 过滤曾让单个注入桶 75.4T 占公共总量 76%；读已停更的 v1 日表导致每轮实时去重 5+ 周并被 SIGKILL）。**增长口径的落库值 = 「最近 7 个已完成 UTC 日 vs 更前 7 个已完成日」**：刷新函数自身按含当日窗口算过一遍，但 `ON CONFLICT ... DO UPDATE SET daily_growth = EXCLUDED.daily_growth` 会命中 `BEFORE INSERT OR UPDATE OF daily_growth` 触发器重算覆盖为已完成日口径。
- **证据**：

`scripts/ops/community-stats-refresh-cron.sql:8-12`、`migrations/20260716110044_add-community-stats-cache.sql:5-12`
```
  8: SELECT cron.schedule(
  9:   'tokentracker-community-stats-refresh',
 10:   '17 * * * *',
 11:   'SELECT public.refresh_tokentracker_community_stats()'
  5: CREATE TABLE IF NOT EXISTS public.tokentracker_community_stats (
  6:   id text PRIMARY KEY CHECK (id = 'total'),
```

`migrations/20260923093000_community-stats-v2-rollup-exclusions.sql:49-79`、`:90-95`
```
 49:   v_cut := date_trunc('day', LEAST(v_through, v_to) AT TIME ZONE 'UTC') AT TIME ZONE 'UTC';
 51:   -- Same exclusion the leaderboard applies: one flagged day makes the whole
 56:   WITH excluded_users AS MATERIALIZED (
 59:     WHERE f.status IN ('auto_excluded', 'banned')
 67:     FROM public.tokentracker_leaderboard_rollup_daily_v2 r
 68:     WHERE r.day < (v_cut AT TIME ZONE 'UTC')::date
 78:     FROM public.leaderboard_hourly_dedup_v2(v_cut, v_to) d
 90:     WHERE p.model <> ''
 91:       AND lower(p.model) <> 'auto'
```

`migrations/20260718082000_use-completed-days-for-community-growth.sql:20-24`、`:59-63` 与 `migrations/20260923093000_...sql:352-359`
```
 20:     COALESCE(SUM(d.tokens) FILTER (
 21:       WHERE d.day BETWEEN v_today - 7 AND v_today - 1
 24:       WHERE d.day BETWEEN v_today - 14 AND v_today - 8
 59: CREATE TRIGGER normalize_tokentracker_community_growth
 60: BEFORE INSERT OR UPDATE OF daily_growth
352:   ON CONFLICT (id) DO UPDATE SET
359:     daily_growth = EXCLUDED.daily_growth,
```

- **口径提醒**：上述「触发器覆盖」结论依赖 PostgreSQL 语义（`BEFORE ... UPDATE OF daily_growth` 对 SET 列表含该列的 `ON CONFLICT DO UPDATE` 同样触发）；刷新函数内部的两段窗口（`:158-173` 的 `d.day >= v_today - 6` 与 `v_today-13..v_today-7`）**不是**最终展示值。主文档若要引用「社区增长」应取触发器口径。

### 6.4 榜单总量缓存、分片读与机器聚类键

- **结论**：`..._rollup_total_v2` 由 `..._rollup_daily_v2` 上的**三条语句级转移表触发器**增量维护（AFTER INSERT 累加、AFTER DELETE 递减后清理全零行、AFTER UPDATE 先减 `old_rows` 再加 `new_rows`）；全时段读再按 **UUID 区间切 8 片**（`leaderboard_usage_grouped_total_shard(p_to, p_user_from, p_user_to)`，`statement_timeout 8s`，未初始化即 `RAISE EXCEPTION 'leaderboard v2 rollup is not initialized'`）；机器聚类键统一为 `COALESCE(dm.machine_cluster_id, h.device_id::text)`。
- **证据**：`migrations/20260904064000_cache-leaderboard-total-rollup.sql:53-85`、`:87-129`、`:131-198`、`:200-222`；`migrations/20260904064500_shard-leaderboard-total-read.sql:7-11`、`:16`、`:53-55`
```
  7: CREATE OR REPLACE FUNCTION public.leaderboard_usage_grouped_total_shard(
  9:   p_user_from uuid,
 10:   p_user_to uuid
 28:     RAISE EXCEPTION 'leaderboard v2 rollup is not initialized';
 53:       FROM public.tokentracker_leaderboard_rollup_total_v2 r
 54:       WHERE (p_user_from IS NULL OR r.user_id >= p_user_from)
 55:         AND (p_user_to IS NULL OR r.user_id < p_user_to)
```

### 6.5 四个 ops 脚本（易误用项）

- **结论**：`usage-daily-rollup-backfill.sql` 重建的是**另一张旧表** `tokentracker_daily_rollup`（**无** `revoked_at` 过滤、**无**机器簇去重、**无**账号级来源分支、**无** `cache_creation_input_tokens` 列，直接对 hourly 原始行 `GROUP BY 1,2,3,4`）——不可与榜单日表混用；`billable-total-tokens-migration.sql` **全文 7 行**只加两列 + 一个部分索引、不写值，且 `billable_rule_version` 全仓**无写入方**；`leaderboard-usage-grouped-rpc.sql` 文件头自标 **SUPERSEDED**（重新应用会把生产退化成 >10s 超时的全扫描版），引用时不得当作现行实现；`account-usage-grouped-rpc.sql` 是当前账号视图 RPC（7 参数 → jsonb，返回列含 `conversations`）。self-host 侧 `scripts/self-host/aggregation.sql` 是**同名不同实现**的 `account_usage_grouped`（多 `p_tz`/`p_offset_min` 兜底链、DeepSeek 峰谷判定与托管版不同）。
- **证据**：

`scripts/ops/usage-daily-rollup-backfill.sql:5-11`、`:23-26`
```
  5:   delete from public.tokentracker_daily_rollup
  8:   insert into public.tokentracker_daily_rollup (
  9:     user_id, day, source, model,
 10:     total_tokens, input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens, updated_at
 23:   from public.tokentracker_hourly
 26:   group by 1,2,3,4;
```

`scripts/ops/leaderboard-usage-grouped-rpc.sql:1-4`、`scripts/ops/billable-total-tokens-migration.sql:1-7`
```
  1: -- ⚠️ SUPERSEDED (2026-07-04, issue #263): leaderboard-rollup-daily.sql replaces
  3: -- Re-applying THIS file reverts prod to the full-scan version that times out
  4: -- (>10s SDK limit). Kept only as rollback reference.
  1: alter table public.tokentracker_hourly
  2:   add column if not exists billable_total_tokens bigint,
  3:   add column if not exists billable_rule_version smallint;
  5: create index concurrently if not exists tokentracker_hourly_billable_null_idx
  7:   where billable_total_tokens is null;
```

### 6.6 本节未确认（仓库外，列出以便主文档标注）

1. 托管库基础表 DDL 不在本仓：`tokentracker_hourly`、`tokentracker_leaderboard_rollup_daily`（v1）、`tokentracker_leaderboard_snapshots`、`agentmeter_leaderboard_snapshots`、`tokentracker_leaderboard_anomaly_flags`、`tokentracker_hourly_quarantine`、`tokentracker_anticheat_config`（本仓 migrations 起于 20260714，self-host `baseline.sql` 只是镜像）。
2. `..._rollup_daily_v2.pricing_tier` 列的 DDL 不在本仓：建表（`migrations/20260804043427:1-13`）无该列、全 migrations 对这张表的 `ALTER` 只有 `ENABLE ROW LEVEL SECURITY`（`:22`），但 `migrations/20260904064000:38-51` 直接 `SELECT ... pricing_tier` ⇒ 生产库该列由仓库外变更添加。**这是 B9 里唯一会被误判成「代码不一致」的点，引用时必须标注为仓库外变更而非缺陷。**
3. `detect_leaderboard_anomalies()` 正文与 `scripts/ops/leaderboard-anomaly-detection.sql` 不在本仓（后者被 edge 注释引用但文件不存在）⇒「检测器如何写 flags」无法逐字核对，只能核对运行预算（`docs/ops/...runtime.sql:5-8`）与它消费的配置键。
4. `machine_cluster_id` 的写入方/聚类算法不在本仓（本仓只 `JOIN` 读，或设备合并时 `DELETE`）。
5. `billable_total_tokens` / `billable_rule_version` 的服务端回填语句不在本仓（只在客户端与 ingest 赋值）。

---

## 7 dev 中间件的路径前缀集合

- **判定**：已解决
- **结论**：`dashboard/vite.config.js` **只有一个** `configureServer`（`:1200`）与一次 `server.middlewares.use(...)`（`:1202`），它按三档分流：**①仓库内实现的精确路径**（走本 checkout 的 `createLocalApiHandler`，`handleRepoLocalApi`，`:1195-1196`）共 14 条——`/api/local-auth`、`/functions/tokentracker-pets`、`/functions/tokentracker-subscription-manager`、`/api/pets/import`、`/api/pets/local/`（前缀）、`/api/pets/codex/`（前缀）、`/functions/tokentracker-project-usage-summary`、`/functions/tokentracker-project-usage-detail`、`/functions/tokentracker-achievements`、`/functions/tokentracker-session-insights`、`/functions/tokentracker-context-health`、`/functions/tokentracker-outcomes`、`/functions/tokentracker-sessions`、`/functions/tokentracker-skills`（`:1208-1242`）；**②其余 `/functions/` 前缀**（`:1249` 起）先交给 dev mock `handleLocalApi`，未识别则 `proxyToLocalCli` 原样转发到 `http://127.0.0.1:7680`（`:1254`、`:1141-1151`），不可达时回 502 并给出启动提示（`:1161-1168`）；**③其余路径** `next()`（`:1259`）。**该中间件对外前缀集合即 `/functions/`（全量前缀）+ 4 条非 `/functions/` 路径**（`/api/local-auth`、`/api/pets/import` 两条精确路径，与 `/api/pets/local/`、`/api/pets/codex/` 两条前缀）；`/api/auth` 不在其内，由 vite 原生 `server.proxy` 转发到云端 `backendConfiguration.baseUrl`（`changeOrigin: true`、`cookieDomainRewrite: ""`，`:1341-1347`），另有 `/proxy/ipcheck` → `https://ip.net.coffee`（`:1332-1336`）；`historyApiFallback` 另把 `/functions/*` 重写为自身以避免 SPA 兜底吞掉 API（`:1321-1325`）。
- **证据**：

`dashboard/vite.config.js:1200-1216`、`:1242-1259`
```
1200:     configureServer(server) {
1202:       server.middlewares.use((req, res, next) => {
1208:         const isRepoPetApi = url.pathname === "/api/local-auth"
1209:           || url.pathname === "/functions/tokentracker-pets"
1213:           || url.pathname === "/functions/tokentracker-subscription-manager"
1214:           || url.pathname === "/api/pets/import"
1215:           || url.pathname.startsWith("/api/pets/local/")
1216:           || url.pathname.startsWith("/api/pets/codex/");
1232:           || url.pathname === "/functions/tokentracker-context-health"
1242:         const isRepoSkillsApi = url.pathname === "/functions/tokentracker-skills";
1249:         if (req.url.startsWith("/functions/")) {
1254:               return proxyToLocalCli(req, res);
1259:         next();
```

`dashboard/vite.config.js:1141-1142`、`:1327-1347`
```
1142:   const target = `http://127.0.0.1:7680${req.url}`;
1341:         proxies["/api/auth"] = {
1342:           target: backendConfiguration.baseUrl,
1343:           changeOrigin: true,
1346:           cookieDomainRewrite: "",
```

---

## 8 macOS 小组件快照 schemaVersion 2 与系统刷新策略（主文档 B 类补充登记）

- **判定**：部分解决（**迁移分支 = 已解决（明确不存在）**；**系统刷新策略 = 源码侧已给全，系统侧缺证据**）
- **结论**：**不存在 v1 → v2 兼容迁移分支**。`WidgetSnapshot.read()` 对版本做**严格相等**过滤（`:362`），函数注释明说「decoding fails (e.g. schema mismatch from an old install)」即返回 `nil`（`:356`）；`WidgetSnapshot` 是合成 Codable，`Shared/` 下无 `CodingKeys` / `decodeIfPresent` / 自定义 `init(from:)`（三项检索零命中），因此旧版本快照只要缺一个必填键就整体解码失败 → `read()` 返回 nil → provider 回落到 `.placeholder` / `.empty`（`StaticSnapshotProvider.swift:20,25`）。刷新策略分三层且源码均已写明：写侧每次成功写盘即 `WidgetCenter.shared.reloadAllTimelines()`（`WidgetSnapshotWriter.swift:108-110`）；另有 4 处设置桥消息也触发全量重载（`locale` / `currency` / `currencySymbol` / `exchangeRate`，`NativeBridge.swift:452,457,468,484`）；写侧节奏被注释标为「pre-existing five-minute widget freshness contract」（`WidgetSnapshotWriter.swift:36-38`），而小组件自身时间线请求的下一次刷新是 `.after(now + 15 分钟)`（`StaticSnapshotProvider.swift:24-29`）。**源码中没有任何注释说明 WidgetKit 系统侧的刷新预算/节流**——「`reloadAllTimelines` 之后系统实际多久刷新一次」在仓库内无证据。
- **证据**：

`TokenTrackerBar/Shared/WidgetSnapshot.swift:354-367`、`:29-31`
```
 29:     /// Schema version — bump when the wire format changes incompatibly.
 30:     public static let snapshotSchemaVersion = 2
354:     /// Reads the most recent snapshot. Tries shared container first, then
355:     /// the per-user fallback. Returns `nil` if neither file exists or
356:     /// decoding fails (e.g. schema mismatch from an old install).
357:     public static func read() -> WidgetSnapshot? {
360:             guard let data = try? Data(contentsOf: url),
361:                   let snapshot = try? decoder().decode(WidgetSnapshot.self, from: data),
362:                   snapshot.schemaVersion == WidgetSharedConstants.snapshotSchemaVersion
363:             else { continue }
364:             return snapshot
```

`TokenTrackerBar/TokenTrackerWidget/Providers/StaticSnapshotProvider.swift:24-29`
```
 24:     func getTimeline(in context: Context, completion: @escaping (Timeline<StaticEntry>) -> Void) {
 25:         let snap = WidgetSnapshotStore.read() ?? .empty
 26:         let entry = StaticEntry(date: Date(), snapshot: snap)
 27:         let next = Date().addingTimeInterval(15 * 60)
 28:         completion(Timeline(entries: [entry], policy: .after(next)))
```

`TokenTrackerBar/TokenTrackerBar/Services/WidgetSnapshotWriter.swift:36-38`、`:108-110`
```
 36:     /// Preserve the pre-existing five-minute widget freshness contract only
 37:     /// for users who actually placed a TokenTracker widget. Everyone else can
 38:     /// stay on the lightweight hidden refresh path.
108:         if ok {
109:             WidgetCenter.shared.reloadAllTimelines()
110:             logger.debug("Widget snapshot written and timelines reloaded")
```

- **若维持（仅刷新策略一项）**：需要运行时观测（真机小组件实际刷新间隔 / WidgetKit 控制台日志），或 Apple 官方文档关于 `reloadAllTimelines` 预算的条款；仓库内既无注释也无常量能给出该值。
- **附注**：`hasConfiguredWidgets()` 只用于决定是否维持 5 分钟「新鲜度契约」的重路径（`WidgetSnapshotWriter.swift:36-51`，失败方向为「宁可刷新」：查询失败返回 `true`）。

---

## 9 `hookStatus` 传参与消费不一致（主文档 C13）

- **判定**：已解决（C13 三条断言全部成立，另补两处更精确的口径）
- **结论**：`status.js:1024-1036` 传入 **11 键**（`codex_notify`、`acode_notify`、`every_code_notify`、`claude`、`gemini`、`opencode_plugin`、`openclaw_session_plugin`、`codebuddy`、`workbuddy`、`workbuddy-ai`、`grok`），`passive-mode.js` 实际消费 **8 键**：`claude`(:100)、`gemini`(:112)、`codex_notify`(:125)、`acode_notify`(:136)、`every_code_notify`(:148)、`codebuddy`(:159)、`workbuddy`(:170)、`workbuddy-ai`(:180)；**未消费 3 键 = `opencode_plugin`、`openclaw_session_plugin`、`grok`**，与 C13 原文一致（全文件 `hookStatus` 只出现 10 次，除 JSDoc 与函数签名外全部列于上表，无第二处读取）。`passive-mode.js:88-90` 的 JSDoc 与实现**双向不一致**成立：JSDoc 列出 9 个名字 `{claude, gemini, codex, every_code, opencode, openclaw, codebuddy, workbuddy, grok}`，其中 `opencode` / `openclaw` **从不被读取**（实现只读 `*_plugin` 形态于 status 侧传参、且根本没读这两个键），而实现真正读取的 `workbuddy-ai`、三个 `*_notify` 变体与三个别名键（`codex`/`acode`/`every_code`）**全都不在 JSDoc 里**；此外实现签名还有第三个形参 `env`（`:93` 使用，`:132,176` 参与路径决策）而 JSDoc 未记录。**补充口径**：三个别名键（`hookStatus?.codex` 等）是**只存在于实现、从未被 status.js 传入**的死回退——即「传 11 键」与「消费 8 键」的关系是「8 家有效 + 3 家冗余」，而非「11 家中有 3 家漏读」。
- **证据**：

`src/commands/status.js:1024-1036`
```
1024:     hookStatus: {
1025:       codex_notify: notifyConfigured,
1026:       acode_notify: acodeConfigured,
1027:       every_code_notify: everyCodeConfigured,
1028:       claude: claudeHookConfigured,
1029:       gemini: geminiHookConfigured,
1030:       opencode_plugin: opencodePluginConfigured,
1031:       openclaw_session_plugin: Boolean(openclawSessionPluginState?.configured),
1032:       codebuddy: Boolean(codebuddyHookConfigured),
1033:       workbuddy: Boolean(workbuddyHookConfigured),
1034:       "workbuddy-ai": Boolean(workbuddyAiHookConfigured),
1035:       grok: Boolean(grokHookState?.configured),
```

`src/lib/passive-mode.js:88-90`（JSDoc 自述形状）
```
 88:  * @param {Object} opts.hookStatus - per-provider hook-installed booleans
 89:  *   (already collected by status.js); shape: { claude, gemini, codex,
 90:  *   every_code, opencode, openclaw, codebuddy, workbuddy, grok }
```

`src/lib/passive-mode.js` 全部读取点（`:100,112,125,136,148,159,170,180`）
```
100:     hookInstalled: Boolean(hookStatus?.claude),
112:     hookInstalled: Boolean(hookStatus?.gemini),
125:     hookInstalled: Boolean(hookStatus?.codex_notify ?? hookStatus?.codex),
136:     hookInstalled: Boolean(hookStatus?.acode_notify ?? hookStatus?.acode),
148:     hookInstalled: Boolean(hookStatus?.every_code_notify ?? hookStatus?.every_code),
159:     hookInstalled: Boolean(hookStatus?.codebuddy),
169:     hookInstalled: Boolean(hookStatus?.workbuddy),
180:     hookInstalled: Boolean(hookStatus?.["workbuddy-ai"]),
```

- **口径提醒**：`passive-mode.js` 只产出 **8 条** entry（claude / gemini / codex / acode / every_code / codebuddy / workbuddy / workbuddy-ai，`:97-184`），与「消费 8 键」严格对齐；主文档 §12 补充登记里「只覆盖 8 个 hook 驱动 provider」的说法此处得到逐条确认。

---

## 附：本轮判定与主文档条目映射

| 本轮编号 | 主文档条目 | 判定 | 主文档需要改动的点 |
| --- | --- | --- | --- |
| 1 | B7 | 已解决 | 补「兜底仅 Windows 壳存在」这一反向口径 |
| 2 | B 类补充登记 | 已解决 | 结论改为「本地 API 取数 + 轮询器推送，组件零取数」 |
| 3 | §L192 | 已解决 | 「未能确认」→「以仓库为准不存在」 |
| 4 | B8 | 已解决 | `syncResult` 键名前提推翻，改为顶层展开 |
| 5 | B6 | 已解决 | Ark 共用凭据 / Qoder 不共用，可写死 |
| 6 | B9 | 已解决 | 改为「函数+物化表」链路；补「聚合层不读反作弊表/不读 `billable_total_tokens`」两条口径 |
| 7 | 无（新增） | 已解决 | 可将 dev 中间件前缀集合落表 |
| 8 | B 类补充登记 | 部分解决 | 迁移=不存在可写死；刷新频率维持运行时才能判 |
| 9 | C13 | 已解决 | 补「3 个别名键是死回退」与「JSDoc 漏记 env 形参」 |
