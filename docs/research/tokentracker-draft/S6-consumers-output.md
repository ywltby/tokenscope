# S6 研究报告：TokenTracker 统计结果的消费端与输出形态

- 样本仓库：`https://github.com/xiufengsun/TokenTracker`
- 本地检出：`C:\Users\admin\AppData\Local\Temp\tt-research`
- HEAD：`b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）
- 研究方法：只读。全程使用 `read` / `grep` / `glob` / PowerShell 只读命令，未修改样本仓任何文件，未执行其中任何脚本。
- 本文只覆盖「统计结果的消费端与输出形态」——CLI 报表、本地 API 端点全清单、Dashboard 消费与二次聚合、导出/分享、限额视图、云上传与聚合、三端原生壳、隐私边界核验、测试契约。**不覆盖**解析器内部（各 agent 日志格式）与定价金额算法的内部推导（另有分工，见 S1/S4）。
- 引用格式：`相对路径:行号`，路径相对样本仓库根。所有行号取自本次实际读取到的内容；行号以 `read` 工具返回的编号为准（注意：`Measure-Object -Line` 与 `read` 对同一文件的行数统计不同，本文一律采用后者）。
- 凡未亲眼确认者一律标注 `【未能确认】`，并在末节汇总。

---

## 0. 结论速览

按与本议题的相关度排序，最有分量的十二条：

1. **CLI 里根本没有"用量报表"这个命令。** `tracker status` 输出的是"环境探测 + 上传/积分状态"（版本、base URL、设备令牌、队列待上传字节数、last parse/notify/upload、hooks、providers），**不含任何 token 用量或成本数字**（`src/commands/status.js:200`、`src/commands/status.js:278-288`、`src/commands/status.js:1040-1259`）。唯一输出 token 汇总的 CLI 是 `tracker wrapped`（`src/commands/wrapped.js:122-148`），第三支是 `tracker sessions`（`src/commands/sessions.js:12-27`）。
2. **`wrapped` 的统计口径与 Dashboard 口径不一致**：它把 `hour_start` 一律按 **UTC** 切年、切天、切"峰值小时"、算连续天数（`src/lib/wrapped-aggregator.js:37-53`、`src/lib/wrapped-aggregator.js:96-101`、`src/lib/wrapped-aggregator.js:142-168`），而 Dashboard 的所有用量端点按请求侧 `tz`/`tz_offset_minutes` 切天（`src/lib/local-api.js:213-225`）。跨时区用户在同一份数据上会看到两个不同的"最忙一天"。**【R1 复核状态：已验证（B14）】**两侧源码逐行属实（`wrapped-aggregator.js:37-53`、`:66-67`「calendar year (UTC)」、`:148-152` UTC 步进、`:170` 峰值小时 UTC；对照 `local-api.js:213-225` + `dashboard/src/lib/api.ts:244-249`）。**前提修正（R2 再修正，E1）**：缺省基准**按路径分两级**——① **日聚合一族**（`rowDayKey`，`src/lib/local-api.js:213-225`）在请求未带 `tz`/`tz_offset_minutes` 时回落 **UTC 日**（`hs.slice(0,10)`，`:224`）；② **`getZonedParts` 系**（hourly 的日过滤与小时键 `:606`/`:608`、summary 的 `todayStr` `:2382`、heatmap 的 `todayStr` `:2538`、`night_owl` 徽章 `:482`）三级回退的第三级是**运行环境本机时区**（`:585-592`，本地 getter），因为这几处没有 tz 守卫。所以「同一份数据两个最忙一天」在真实前端路径（前端与三端壳都显式传 tz：`APIClient.swift:241-254`、`extension.js:997`、`UsagePoller.cs:321`）成立；而裸调 API 且不传 tz 时，与 `wrapped` 的 **UTC** 年/天/小时真正分歧的是 `hourly` 的小时键与 `todayStr` 一系（**本机时区**），其中 `hourly` 的 `day` 默认值又是 UTC 今天（`:3147`），与它自己的过滤谓词就不同基准。详见 S4 §2.4。
3. **本地 API 是一个前缀三分的单进程 HTTP 服务**：`/functions/`（27 个 `tokentracker-*` 端点，本文 §2.10 逐个列出）、`/api/`（鉴权、宠物资产、geoip/iprisk 代理、avatar 代理）、`/proxy/ipcheck`（`src/commands/serve.js:172-179`、`src/lib/local-api.js:939-940`）。所有 JSON 响应统一 `Cache-Control: no-store`（`src/lib/local-api.js:914-927`）。
4. **消费端有至少四条独立路径**：① Dashboard（`dashboard/src/lib/api.ts` 统一封装，本地 `/functions/x` ↔ 云端 `account-*` 双路由，`dashboard/src/lib/api.ts:182-242`）；② 三端原生壳（macOS/Windows/Linux 各自 HTTP 调同一批端点）；③ **Vite dev server 的 `localDataApi` 插件直接读 `queue.jsonl`**（`dashboard/vite.config.js:570-608`）；④ macOS 壳用 FSEvents 监听 `queue.jsonl` 只做"活动信号"，不解析内容（`TokenTrackerBar/TokenTrackerBar/Services/QueueActivityMonitor.swift:1-36`）。
5. **前端确实做二次聚合**，且存在"后端给的用、后端不给就自己算"的双轨：`activeDays` 优先取 `heatmap.active_days`，缺失时前端遍历热力图单元格重算（`dashboard/src/pages/DashboardPage.jsx:819-850`）；`streak_days` 服务端在本地热力图端点里被**硬编码为 0**（`src/lib/local-api.js:2588`），前端用 `computeActiveStreakDays()` 自己重算并覆盖（`dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295`）。**【R1 复核状态：已验证（B15）】**`local-api.js:2588` 确为硬编码 `streak_days: 0,`（同响应 `:2587` 服务端算真实 `active_days`）；`dashboard/src/lib/pet-linux-host.js:198` 也直读 `r.body?.streak_days`，本地模式拿到 0；**Windows 托盘是否真显示 0 仍未实测**（见 §10 第 1 条）。
6. **分享卡片的数据是"拼起来的两套口径"**：token/成本/热力图取自当前页面的本地（或账号）用量数据，`rank` 单独向云端 leaderboard 拉取（`dashboard/src/ui/share/use-share-card-data.ts:49-80`），因此卡片上的"token 总量"是单机（或账号去重后）口径，而"排名"是平台上所有已上传用户的口径。
7. **源码里不存在"saved amount / 节省金额"这类营销指标**。全仓 grep `savings|amount_saved|saved_amount|total_saved` 共 **9 处命中（分布在 7 个文件）**，其中 `amount_saved|saved_amount|total_saved` 为 0 命中，`savings` 的 9 处全部与用量统计无关：`dashboard/src/pages/CloudPage.jsx:95`、`:195`（`cloudAnnualSavings`，**订阅年付相对月付的折扣百分比**）、`dashboard/src/content/i18n/zh/dashboard.json:650`、`zh-TW/dashboard.json:709`（`cloud.price.savings` 文案）、`dashboard/src/content/copy.csv:1699`、`DESIGN.md:17`（"savings badges"）、`scripts/cloud-sandbox/hosted-archive-acceptance.cjs:377`、`:450`（`bill_savings_claimed: false` 免责声明）、`docs/cloud-usage-archive.md:70`（"does not establish a production savings percentage"）。
8. **限额视图与用量统计完全解耦**：`/functions/tokentracker-usage-limits` 走的是"provider 官方 API + 本地磁盘缓存"的 17 家 provider 并行抓取（`src/lib/usage-limits.js:4055-4440`），产出百分比窗口与重置时间，**从不进入 `queue.jsonl`**；quota 缓存单独落在 `tracker/*-usage-limits-cache.json`（`docs/PRIVACY.md:43`）。缓存 TTL 2 分钟（`src/lib/usage-limits.js:64`）。
9. **云上传是"按字节偏移的队列追加流"**：每次 POST `{upload_id, hourly:[...], account_session_states?}`（`src/commands/sync.js:4367-4374`），字段由 ingest edge 逐列映射到 `tokentracker_hourly`（`dashboard/edge-patches/tokentracker-ingest.ts:176-194`），批内按 `(hour_start, source, model)` 取 `total_tokens` 最大者去重（`dashboard/edge-patches/tokentracker-ingest.ts:203-211`），单批上限 500 桶 / 500 会话状态（`dashboard/edge-patches/tokentracker-ingest.ts:111-113`、`:172-174`）。
10. **节流是"客户端 30 分钟窗口 + 服务端下发的 next_allowed_at"取较晚者**：默认 `intervalMs 30min`、抖动 ≤60s、积压 1 MB 以上允许 4 批（否则 2 批）、批量 300、失败退避 60s 起、上限 30 分钟、指数步进封顶 10→20（`src/lib/upload-throttle.js:1-10`、`:57`、`:109-110`、`:123`）。
11. **三端壳端口不同且都有原因**：macOS 固定 `7680`（`TokenTrackerBar/TokenTrackerBar/Utilities/Constants.swift:4-5`），Windows 选**优先 17680**（避开 Windows Delivery Optimization 占用的 7680，且要落在 InsForge OAuth 白名单里，`TokenTrackerWin/ServerManager.cs:469-476`），Linux Tauri + GNOME 扩展固定 `17680`（`TokenTrackerLinux/gnome-extension/tokentracker@tokentracker.cc/extension.js:18-19`）。CLI 自身默认 7680、WSL 下默认 7681（`src/commands/serve.js:14-22`）。
12. **PRIVACY 的 "token counts only" 断言大体成立，但有两处文档未逐字覆盖的实现细节**：① 云端上传除小时桶外还包含 `account_session_states`（`source, session_id, model, bucket_start, snapshot_verified_at, 六类 token`），且**仅当 source 为 `trae-cn` 时才被接受**（`dashboard/edge-patches/tokentracker-ingest.ts:110-161`、`src/lib/upload-queue.js:9-18`）——`docs/PRIVACY.md:100-111` 的"Sent when signed in"清单未列出该结构；② 心跳除 `machine_hash` 外还含 `app_version`、`platform`、`shell`（`src/lib/telemetry.js:75-82`），这一条 PRIVACY 已写明（`docs/PRIVACY.md:56`）。

---

## 1. CLI 的对外输出面

### 1.1 命令清单

入口是 `src/cli.js`，无参数时直接进 `serve`：

```js
// src/cli.js:12-19
async function run(argv) {
  const [command, ...rest] = argv;

  // No args → launch dashboard
  if (!command) {
    await cmdServe(argv);
    return;
  }
```

命令分派与实现文件对应关系（`src/cli.js:32-65`）：

| 命令 | 处理函数 | 实现文件 | 输出形态 |
| --- | --- | --- | --- |
| `serve`（默认） | `cmdServe` | `src/commands/serve.js` | 启动本地 HTTP + 静态 Dashboard，打印 URL/数据路径 |
| `init` | `cmdInit` | `src/commands/init.js`（2427 行） | 安装引导（不在本文范围） |
| `sync` | `cmdSync` | `src/commands/sync.js`（7251 行） | 进度条 + 汇总计数 |
| `status` | `cmdStatus` | `src/commands/status.js`（1729 行） | 默认人读列表 / `--json` / `--light` ASCII 表 |
| `diagnostics` | `cmdDiagnostics` | `src/commands/diagnostics.js` | 纯 JSON（`{ok, version, env, paths, ...}`） |
| `doctor` | `cmdDoctor` | `src/commands/doctor.js` | 人读检查表 / `--json` / `--out` |
| `uninstall` | `cmdUninstall` | `src/commands/uninstall.js` | 人读 |
| `device-login` | `cmdDeviceLogin` | `src/commands/device-login.js` | 人读 / `--json` |
| `wrapped` | `cmdWrapped` | `src/commands/wrapped.js` | ASCII 年框 / `--json` |
| `sessions` | `cmdSessions` | `src/commands/sessions.js` | JSON / CSV，可 `--out` |

`-v/--version`、`-h/--help` 在分派之前处理（`src/cli.js:21-30`）。帮助文本里的输出面自述（`src/cli.js:85-86`）：

```
  npx tokentracker [--debug] wrapped [--year 2026] [--json]
  npx tokentracker sessions [--from YYYY-MM-DD] [--to YYYY-MM-DD] [--format json|csv] [--out file] [--refresh] [--no-git]
```

### 1.2 `status`：环境与上传状态，不是用量报表

`cmdStatus` 的第一件事是 `--diagnostics` 短路（原样输出诊断 JSON，走的是完全不同的形状）：

```js
// src/commands/status.js:189-195
async function cmdStatus(argv = []) {
  const opts = parseArgs(argv);
  if (opts.diagnostics) {
    const diagnostics = await collectTrackerDiagnostics();
    process.stdout.write(JSON.stringify(diagnostics, null, 2) + "\n");
    return;
  }
```

它读取的状态文件（这是它"如何读取队列与缓存"的准确答案）：

```js
// src/commands/status.js:200-206
  const queuePath = path.join(trackerDir, "queue.jsonl");
  const queueStatePath = path.join(trackerDir, "queue.state.json");
  const cursorsPath = path.join(trackerDir, "cursors.json");
  const uploadThrottlePath = path.join(trackerDir, "upload.throttle.json");
```

```js
// src/commands/status.js:278-288
  const queueState = (await readJson(queueStatePath)) || { offset: 0 };
  const uploadThrottle = normalizeUploadState(
    ...
  const queueSize = await safeStatSize(queuePath);
  const pendingBytes = Math.max(0, queueSize - (queueState.offset || 0));
```

也就是说：`status` **不解析 `queue.jsonl` 的内容**，只用 `statSync` 拿字节数与 `queue.state.json` 的偏移量相减算"待上传字节"。上传时间线来自 `upload.throttle.json`：

```js
// src/commands/status.js:341-347
  const lastUpload = uploadThrottle.lastSuccessMs
    ...
  const nextUpload = parseEpochMsToIso(uploadThrottle.nextAllowedAtMs || null);
  const backoffUntil = parseEpochMsToIso(uploadThrottle.backoffUntilMs || null);
```

`last_parse` 来自 `cursors.json` 的 `updatedAt`（`src/commands/status.js:1050`：`last_parse: cursors?.updatedAt || null`）。

### 1.3 `status --json` 顶层字段（逐字）

```js
// src/commands/status.js:1040-1059
    const summary = {
      version: pkg.version,
      generated_at: new Date().toISOString(),
      base_url: config?.baseUrl || null,
      device_token_set: Boolean(config?.deviceToken),
      queue: {
        pending_bytes: pendingBytes,
        size_bytes: queueSize,
        offset: queueState.offset || 0,
      },
      last_parse: cursors?.updatedAt || null,
      last_notify: lastNotify || null,
      last_openclaw_sync: lastOpenclawSync || null,
      last_notify_spawn: lastNotifySpawn || null,
      last_upload: lastUpload || null,
      next_upload_after: nextUpload || null,
      backoff_until: backoffUntil || null,
      last_upload_error: lastUploadError || null,
      last_sync_skipped: syncSkip?.at ? syncSkip : null,
      auto_retry: autoRetry || null,
```

其余顶层键（`src/commands/status.js:1063-1259`）：`extra_scan_roots[]`（`{provider, origin, path, exists, error}`）、`hooks{}`（键名逐字：`codex_notify`、`acode_notify`、`every_code_notify`、`claude`、`gemini`、`opencode_plugin`、`openclaw_session_plugin`、`openclaw_session_plugin_conversation_access`、`openclaw_legacy`、`codebuddy`、`workbuddy`、`workbuddy-ai`、`grok`）、`providers{}`（约 40 家，每家 `{installed, files?, detail?, error?}`，`src/commands/status.js:1081-1238`）、`copilot{}`（`token_set`、`otel_has_files`、`otel_path`、`otel_enabled` + store/app DB 状态，`src/commands/status.js:1239-1246`）、`passive_mode{}`（`{active, providers[]}`，`:1247-1250`）、Windows 专有的 `wsl_mode`/`wsl_mode_invalid`/`wsl_distros[]`（`:1251-1257`）、`subscriptions`（`:1258`）。

**逐字确认：没有 `total_tokens`、没有 `cost`、没有任何按 source/model 的分布。** `status` 不是用量报表。

### 1.4 `status` 的三种渲染

```js
// src/commands/status.js:1261-1266
    if (opts.json) {
      process.stdout.write(JSON.stringify(summary, null, 2) + "\n");
      return;
    }
    process.stdout.write(renderLightTable(summary) + "\n");
    return;
```

- `--json`：`JSON.stringify(summary, null, 2)`。
- `--light`：`renderLightTable()` 生成的固定宽度 ASCII 表（`src/commands/status.js:1594-1702`），列头是 `Key` / `Value`，行由 `push(k, v)` 组装，空值显示为 `—`（`:1596`）。它的定位写在注释里：

```js
// src/commands/status.js:1590-1593
// Pure renderer: turn the structured summary into a fixed-width ASCII table.
// "light" output is for AI agents and CI: deterministic columns, no emoji or
// spinner side effects, easy to grep. Returns a string (caller adds trailing
// newline).
```

- 无 flag：人读列表，首行 `TokenTracker v...`，含 `Status:` 与 `- Queue: N bytes pending`（由 `test/status-json-light.test.js:77-83` 锁定）。
- `--diagnostics`：与 summary **形状完全不同**（`env`/`paths`），且测试明确要求 summary 的键不得泄漏进来（`test/status-json-light.test.js:68-75`）。

参数解析（`src/commands/status.js:1576-1584`）：`--json`、`--light`、`--diagnostics`、`--probe-keychain`、`--probe-keychain-details`，未知 flag 直接 `throw new Error("Unknown option: ...")`。

### 1.5 `wrapped`：唯一的"用量报表"CLI

`cmdWrapped` 自己读 `queue.jsonl`（不经过本地 API）：

```js
// src/commands/wrapped.js:122-128
async function cmdWrapped(argv = []) {
  const opts = parseArgs(argv);
  const home = os.homedir();
  const { trackerDir } = await resolveTrackerPaths({ home });
  const queuePath = path.join(trackerDir, "queue.jsonl");

  const rows = await readQueueRows(queuePath);
```

坏行容错：逐行 `JSON.parse`，解析失败**静默跳过**（`src/commands/wrapped.js:14-22`，注释说明生产队列中"半行"会在下次读取时自愈）。参数只有 `--json` 与 `--year`（2000–2100 校验，`src/commands/wrapped.js:26-41`）。

聚合口径（`src/lib/wrapped-aggregator.js`）：

- **去重**：按 `source|model|hour_start` 建 Map，`Map.set` 覆盖 = 后写胜出，与 Dashboard 读端一致；并逐行应用 `normalizeQueueRow()` 的旧行修正（`src/lib/wrapped-aggregator.js:77-86`）。
- **年度选择**：未指定 `--year` 时取"数据中最新的一年"，而不是当前年（`src/lib/wrapped-aggregator.js:91-99`）。
- **全 UTC**：`rowDay` = `hour_start.slice(0,10)`（`:37-40`）、`rowYear` 取该串前 4 位（`:42-46`）、`isoHour` 用 `/T(\d{2}):/` 正则取小时（`:48-53`）、连续天数用 `new Date(iso + "T00:00:00Z")` 步进（`:148-152`）。
- **token 口径**：`billable_total_tokens` 优先，回落 `total_tokens`（`:29-35`）。
- **输出结构**：

```js
// src/lib/wrapped-aggregator.js:209-223
  return {
    year,
    range: { from: earliest, to: latest },
    totals: {
      tokens: totalTokens,
      conversations: totalConvs,
      sources: tokensBySource.size,
      models: tokensByModel.size,
      active_days: activeDays.length,
    },
    top: { sources: topSources, models: topModels, days: topDays },
    peak_hour: peakHour,
    longest_streak: longestStreak,
    highlights,
  };
```

- `top.sources` / `top.models` 各取 5 并附 `share`（占比），`top.days` 取 5（`:177-187`）。
- `highlights` 是四条硬编码英文句式（顶层模型的占比、最忙一天的 token、峰值小时、连续天数）+ 第 5 条"工具数量 ≥ 4"（`:189-207`）。
- **没有成本字段**：`aggregateWrapped` 全程不触碰 `total_cost_usd`，也不 require 定价模块（模块头注释明确说"no UI or stringification concerns leak in here"，`:11-15`）。

ASCII 渲染（`src/commands/wrapped.js:43-120`）：60 宽 ╔═╗ 框，`Total tokens` 用 `formatCompact`（T/B/M/K，`src/lib/wrapped-aggregator.js:226-234`），`Active days` 硬写 `/ 365`（`src/commands/wrapped.js:67`），`Peak hour` 显示 `NN:00 UTC`（`:70-77`）。

### 1.6 `sessions`：会话效率导出

```js
// src/commands/sessions.js:12-27
async function cmdSessions(args = []) {
  const format = option(args, "--format") || (args.includes("--csv") ? "csv" : "json");
  const out = option(args, "--out");
  const sessions = await buildSessionAnalytics({ force: args.includes("--refresh") });
  if (!args.includes("--no-git")) await buildGitOutcomes(sessions);
  const summary = summarizeSessions(sessions, { from: option(args, "--from"), to: option(args, "--to") });
  const content = format === "csv"
    ? sessionsToCsv(summary.sessions)
    : `${JSON.stringify(summary, null, 2)}\n`;
```

注意三件事：① 这是**独立于 `wrapped` 的第二条统计链**（`session.queue` 侧车 + 直接扫会话文件），见 `src/lib/session-analytics.js`；② CSV 列定义在 `src/lib/session-analytics.js:2909`：

```
session_hash, source, project_key, model, model_usage_json, started_at, ended_at, duration_ms, turns, edit_turns, retry_turns, subagent_calls, total_tokens, cost_usd, productive, first_pass, one_shot
```

③ 写文件时 `mode: 0o600`（`src/commands/sessions.js:22`）。

`summarizeSessions()` 的返回（`src/lib/session-analytics.js:2521-2566`）：`available`、`sessions[]`（默认剥离 `project_ref`/`session_id`/`parent_session_id`/`title` 等字段，`:2526-2543`）、`session_count`、`summary{...}`（含 `productive_rate`、`one_shot_rate`、`edit_session_rate`、`tokens_per_edit`、`cost_per_edit`，`:2545-2555`）、`by_model[]`、`subagents[]`、`provenance{source:"local-session-log", confidence:"observed", privacy:"metadata-only", methodology:"edit-turn-v2", ...}`（`:2558-2565`）。

**关键口径警告（源码自己写的）**：`by_model[].sessions` 是"触碰过该模型的会话数"，**不可求和**为总会话数；token/成本列才可求和（`src/lib/session-analytics.js:2421-2427`）。

### 1.7 三者分工

| 维度 | `status` | `wrapped` | `sessions` |
| --- | --- | --- | --- |
| 数据源 | 状态文件（queue 字节数、cursors、upload.throttle、config、各 provider 安装痕迹） | `queue.jsonl` 全量行 | 会话文件 + `session.queue` 侧车（`buildSessionAnalytics`） |
| 时间口径 | 不涉及时间分桶 | UTC 年/天/小时 | `from`/`to` + 可选 tz（`src/lib/session-analytics.js:2343-2364`） |
| 指标 | 环境/上传健康度 | token 总量、会话数、活跃天、工具/模型数、峰值小时、连续天数、Top5 | 会话效率（productive/one-shot/edit_turns/retry_turns、per-model、per-subagent） |
| 成本 | 无 | 无 | 有（`cost_usd`，来自会话级定价） |
| 机器可读 | `--json` / `--light` | `--json` | `--format json|csv` + `--out` |
| 对应 Dashboard | Settings/Banner 里的集成状态 | `/wrapped` 页（同一聚合器） | Sessions 页（同一 `session-analytics`） |

---

## 2. 本地 API 全清单

### 2.1 服务骨架

`serve` 启动的 HTTP 服务（`src/commands/serve.js:69`）按前缀分派：

```js
// src/commands/serve.js:171-189
      // API routes
      if (
        url.pathname.startsWith("/functions/")
        || url.pathname.startsWith("/api/")
        || url.pathname.startsWith("/proxy/")
      ) {
        const handled = await handleApi(req, res, url);
        if (handled) return;
      }

      // Static files
      const served = await serveStaticFile(dashboardDir, url.pathname, res, { localRuntimeConfig: true });
      if (served) return;

      // SPA fallback
      if (shouldServeSpaFallback(req, url)) {
```

- 绑定：`127.0.0.1`（`src/commands/serve.js:34`），端口默认 7680、WSL 7681、非显式端口时最多顺延 20 个（`src/commands/serve.js:14-22`、`:202-215`）。
- CORS：**任何** `OPTIONS`（`/api/auth/` 前缀除外）都回 204 并放行 `GET, POST, OPTIONS`（`src/commands/serve.js:162-169`）。
- handler 由 `createLocalApiHandler({ queuePath, serverVersion })` 构造（`src/commands/serve.js:148-155`），队列路径来自 `resolveQueuePath()` = `~/.tokentracker/tracker/queue.jsonl`（`src/lib/local-api.js:83-86`）。
- 静态资源从 `dashboard/dist` 提供，两层候选路径（`src/commands/serve.js:716-725`）。
- 后台任务：Windows 专属每分钟 `sync --auto --background --all-local-sources` 子进程（`src/commands/serve.js:294-366`，仅在 `TOKENTRACKER_APP_SHELL=windows` 且不是 Windows 宿主自己负责时启用，`:303-309`）；匿名心跳启动时一次 + 每 6 小时复查（`src/commands/serve.js:266-273`）。

### 2.2 鉴权与响应约定

- 每次进程启动生成一次性本地令牌：`const localAuthToken = crypto.randomBytes(24).toString("hex")`（`src/lib/local-api.js:1016`），经 `GET /api/local-auth` 暴露（`src/lib/local-api.js:1681-1695`），响应体 `{token, serverVersion?}`。`serve.js` 用同一端点做"重复实例/健康"判定（`src/commands/serve.js:508-534`）。
- 写操作（"local mutation"）校验头 `x-tokentracker-local-auth`：

```js
// src/lib/local-api.js:1629-1637
  function isAuthorizedLocalMutation(req) {
    const headerToken = req?.headers?.["x-tokentracker-local-auth"];
    ...
    if (!token || token !== localAuthToken) return false;
```

- 前端取令牌的方式（拿到后进程内缓存）：`dashboard/src/lib/local-api-auth.ts` 内 `getLocalApiAuthToken()` 调 `/api/local-auth`，`getLocalApiAuthHeaders()` 返回 `{"x-tokentracker-local-auth": token}`。
- `/api/auth/*` 与 `GET /api/auth-bridge/verifier` 额外要求同源/回环来源，否则 403 `untrusted_auth_origin`（`src/lib/local-api.js:1643-1648`）。
- 所有 JSON 响应统一 `no-store`：

```js
// src/lib/local-api.js:914-927
function json(res, data, status) {
  ...
  res.writeHead(status || 200, {
    "Content-Type": "application/json",
    "Cache-Control": "no-store",
  });
  res.end(JSON.stringify(data));
}
```

### 2.3 通用查询参数（用量类端点）

| 参数 | 取值 | 读取位置 |
| --- | --- | --- |
| `from` / `to` | `YYYY-MM-DD`，按**日键闭区间**过滤（`day >= from && day <= to`） | `src/lib/local-api.js:2359-2364`、`:130-134` |
| `tz` | IANA 时区名 | `getTimeZoneContext` `src/lib/local-api.js:512` 起 |
| `tz_offset_minutes` | 分钟偏移（fallback） | 同上；键生成见 `src/lib/local-api.js:115-117` |
| `scope` / `include_account_level` | 默认 `all`；`personal|local` 排除账号级来源；`include_account_level=1` 强制 `all` | `src/lib/local-api.js:398-401`、`src/lib/source-metadata.js:15-26` |
| `source` / `model` / `device_id` | 前端把过滤参数拼进查询串 | `dashboard/src/lib/api.ts:254-263` |
| `account=1` | 触发账号（跨设备）视图代理 | `src/lib/local-api.js:2347-2355` |
| `weeks` | 热力图周数，默认 52 | `src/lib/local-api.js:2534` |
| `day` | hourly 端点用 | `src/lib/local-api.js:3147` |
| `limit` | 项目汇总上限（默认 10，`PROJECT_USAGE_MAX_ENTRIES`）/ 会话浏览器（上限 2000） | `src/lib/local-api.js:95`、`:2737-2740`、`:2504-2505` |

账号级来源（`account` scope）白名单**只有两家**：`cursor` 与 `trae-cn`（`src/lib/source-metadata.js:1`）。它们的用量在账号视图里是"跨设备去重而非相加"，这条语义被 `test/account-source-parity.test.js:3-18` 锁死（该测试要求 4 处硬编码必须一致，包括 SQL 与两个 edge 函数）。

### 2.4 用量 / 成本组端点

#### 2.4.1 概览：`/functions/tokentracker-usage-summary`

- 方法与参数：GET；`from`、`to`、`tz`、`tz_offset_minutes`、`scope`、`account=1`。
- 定义行号：`src/lib/local-api.js:2358`。
- 计算：`scopedQueueRows()` → `aggregateByDay()` → 过滤 `from..to` → 对 9 个字段累加（`src/lib/local-api.js:2365-2379`）。
- 返回顶层字段（`src/lib/local-api.js:2413-2420`）：`from`、`to`、`days`、`scope`、`excluded_sources`、`totals{total_tokens, billable_total_tokens, total_cost_usd(string, toFixed(6)), input_tokens, output_tokens, cached_input_tokens, cache_creation_input_tokens, reasoning_output_tokens, conversation_count}`、`rolling{last_7d:{from,to,active_days,totals{billable_total_tokens,conversation_count}}, last_30d:{...,avg_per_active_day}}`。
- 注意 `rolling` 的窗口**只统计"确有日汇总的活跃日"**（`collectDays` 跳过空日，`src/lib/local-api.js:2390-2404`），`active_days` 因此不等于 7/30。

#### 2.4.2 按天：`/functions/tokentracker-usage-daily`

- GET；`from`、`to`、tz 参数；定义 `src/lib/local-api.js:2425`。
- 返回 `{from, to, scope, excluded_sources, data[]}`（`:2431`），`data` 项字段见 `buildDailyAggregates()` 的桶结构：

```js
// src/lib/local-api.js:268-297（节选）
      byDay.set(day, {
        day,
        total_tokens: 0,
        billable_total_tokens: 0,
        total_cost_usd: 0,
        input_tokens: 0,
        ...
      });
      const a = byDay.get(day);
      a.total_cost_usd += computeRowCost(row);
      ...
      a.models[model] = (a.models[model] || 0) + (row.total_tokens || 0);
```

即每天额外带一个 `models{s: tokens}` 映射（按 `total_tokens`，不是 billable）。

#### 2.4.3 按小时：`/functions/tokentracker-usage-hourly`

- GET；`day`（默认 UTC 今天）；定义 `src/lib/local-api.js:3146`。
- 返回 `{day, scope, excluded_sources, data[]}`（`:3151`），`data` 由 `aggregateHourlyByDay()` 产出（`:600` 起）。

#### 2.4.4 按月：`/functions/tokentracker-usage-monthly`

- GET；`from`、`to`；定义 `src/lib/local-api.js:3156`。
- 月的键是**日键前 7 位**（`const month = day.slice(0, 7)`，`:3168`），即"月"归属由时区日键决定；返回 `data[]` 每项 `{month, total_tokens, billable_total_tokens, input_tokens, output_tokens, cached_input_tokens, cache_creation_input_tokens, reasoning_output_tokens, conversation_count, models{}}`（`:3170-3185`）。

#### 2.4.5 热力图：`/functions/tokentracker-usage-heatmap`

- GET；`weeks`（默认 52）；定义 `src/lib/local-api.js:2533`。
- 返回（`:2581-2591`）：`from`、`to`、`scope`、`excluded_sources`、`week_starts_on:"sun"`、`active_days`、`streak_days`、`weeks[][]`（每格 `{day, total_tokens, billable_total_tokens, level(0-4), models|null}`）、`total_cost_usd`。
- 分级规则：按"窗口内最大值"归一，`r<=0.25→1, <=0.5→2, <=0.75→3, else 4`（`:2547-2557`）。

> **`streak_days` 在本地热力图里恒为 0**（`src/lib/local-api.js:2588`：`streak_days: 0,`）。前端不依赖它：`dashboard/src/hooks/use-activity-heatmap.ts:206`、`:284`、`:295` 自行调用 `computeActiveStreakDays()` 计算并覆盖；Windows 托盘的 `UsagePoller` 读的是 `streak_days`（`TokenTrackerWin/UsagePoller.cs:314-328`，注释写成"streak is server-computed"），因此**Windows 托盘的"连续天数"在本地模式下会显示 0**，除非账号视图返回真实值。此点列入待复核。

#### 2.4.6 按模型：`/functions/tokentracker-usage-model-breakdown`

- GET；`from`、`to`；定义 `src/lib/local-api.js:2596`。
- 返回 `{from, to, days:0, scope, excluded_sources, sources[], pricing{model:"per-model", pricing_mode:"per_token_type", source:"litellm", effective_from}}`（`:2656-2659`）。**【R2 补限定（I8）】其中 `pricing.source: "litellm"` 是硬编码标签**（`:2658`），**不代表本行实际命中的来源**——命中 `curated:*` 条目时该字段仍写 `litellm`（对照 dev mock 的 `dashboard/vite.config.js:1067` 硬编码 `"default"`）；需要真实命中来源时应看每行的 `cost_source`（取值 `provider_reported` / `model_pricing` / `mixed`，`:2624-2626`）。
- `sources[]` 项：`{source, source_scope, totals{...8 项 + total_cost_usd(string, 6 位)}, models[]}`；`models[]` 额外带 `model_id`、`pricing`（`getModelPricingInfo`）、`cost_source`（取值 `provider_reported` / `model_pricing` / `mixed`，`:2624-2626`）。
- 成本按**逐行计价后再求和**，源码专门解释了"不能先加列再计价"的原因（长上下文/加急档位会被吞掉，`src/lib/local-api.js:2634-2640`）。排序：按 `total_tokens` 降序（`:2650`）。

#### 2.4.7 按语义类别：`/functions/tokentracker-usage-category-breakdown`

- GET；`from`、`to`、`source`（`claude` 默认 / `codex` / `grok`）；定义 `src/lib/local-api.js:2668`。
- `claude` 走 `computeClaudeCategoryBreakdown`（`:2674`），失败返回带 `error:"compute_failed"` 的 500 但**仍带 `unsupportedCategoryPayload`**（`:2678`）；`codex` 走 `computeCodexContextBreakdown`，若总量为 0 则改用**基于队列行的降级口径**（`:2692-2698`）。
- 降级返回结构（`src/lib/local-api.js:329-395`）明确标注隐私：`message_breakdown.privacy.includes_content=false`（`:376-379`）、`tool_calls_breakdown.privacy.includes_inputs=false`（`:386-389`），并带 `breakdown_status:"queue_fallback"`、`fallback:"queue_totals"`。

#### 2.4.8 年度报告：`/functions/tokentracker-wrapped`

- GET；`year`（可选）；定义 `src/lib/local-api.js:2330`。
- 返回 `{scope, excluded_sources, ...aggregateWrapped(rows, {year?})}`（`:2336`）——**与 CLI `wrapped` 共用同一个聚合器**（模块头注释：sources of truth，"the same aggregator powers the CLI and the React Wrapped page"，`src/lib/wrapped-aggregator.js:11-15`）。

#### 2.4.9 明细类

- `/functions/tokentracker-outcomes`（GET，`from`/`to`，`src/lib/local-api.js:2441`）：可选 sidecar `~/.tokentracker/tracker/outcomes.jsonl`；未开启时返回 `{available:false, by_model:[], by_tool:[], totals:null}`（`:2460`）；开启时先刷新两个"仅元数据"侧车（`buildSessionAnalytics` + `buildGitOutcomes`，`:2450-2457`），再 `computeQualityPerDollar(rows, outcomes, {from,to})` 展开（`:2464-2465`）。注释强调该路径**不改 `queue.jsonl` schema、不碰同步链**（`:2435-2440`）。
- `/functions/tokentracker-context-health`（GET，无参，`src/lib/local-api.js:2526`）：返回 `computeContextHealth({home, cwd, env})` 的原样结果；字段结构在 `src/lib/context-health.js`，本文未展开【未能确认具体字段】。

### 2.5 会话组端点

#### 2.5.1 `/functions/tokentracker-session-insights`

- GET；`from`、`to`、`refresh`（`1|true`）、`include_sessions`（`1|true`）、`format=csv`；定义 `src/lib/local-api.js:2470`。
- JSON 返回 `{from, to, ...summarizeSessions(...)}`（`:2488`）——字段同 §1.6。
- `format=csv` 直接返回 `text/csv` 附件，文件名固定 `tokentracker-sessions.csv`，并强制包含会话数组（`:2477-2487`）。

#### 2.5.2 `/functions/tokentracker-sessions`（本地专用）

```js
// src/lib/local-api.js:2495-2509（节选）
    // --- metadata-only session browser (LOCAL ONLY) ---
    // Unlike session-insights this returns per-session rows that retain the
    // raw session_id + local project path so the dashboard can offer one-click
    // resume. It is intentionally served only from the local API and never
    // proxied to the cloud account view.
    ...
      res.setHeader("Cache-Control", "no-store");
```

- 参数：`from`、`to`、`refresh`、`limit`（上限 2000，`:2504-2505`）。
- 返回 `{from, to, ...listSessionsForBrowser(...)}`（`:2515`），其中 `listSessionsForBrowser` 给出 `{available, session_count, returned_count, skipped_files, sessions[], provenance{source:"local-session-log", confidence:"observed", privacy:"metadata-only", scope:"local-only"}}`（`src/lib/session-analytics.js:2893-2905`）。
- 出错时把 fs 错误挡在响应体外（避免绝对路径泄漏），只回 `{available:false, error:"Session browser failed"}`（`src/lib/local-api.js:2516-2521`）。

### 2.6 项目组端点

#### 2.6.1 `/functions/tokentracker-project-usage-summary`

- GET；`from`、`to`、`tz*`、`limit`；定义 `src/lib/local-api.js:2731`。
- 数据源是**派生队列** `project.queue.jsonl`（`path.join(path.dirname(qp), "project.queue.jsonl")`，`src/lib/local-api.js:135`），并剔除 claude-mem 伪项目（`:136-138`）。
- 返回 `{generated_at, entries[]}`（`:2820`）；`entries[]` 项：`{project_key, project_ref, total_tokens(string), billable_total_tokens(string), input_tokens, output_tokens, cached_input_tokens, cache_creation_input_tokens, reasoning_output_tokens, conversation_count, sources[{source, total_tokens}]}`（`:2787-2794`）。**token 字段被显式转成字符串**（大整数过 JSON 的原因）。
- 排序按 `billable_total_tokens` 降序，条数上限默认 10（`PROJECT_USAGE_MAX_ENTRIES`，`src/lib/local-api.js:95`）。
- 全空时降级为"按 source 聚合"的伪条目，且 `project_ref` 留空（注释解释了历史上伪造 `https://codex.ai` 之类的 href 是 bug，`:2803-2818`）。

#### 2.6.2 `/functions/tokentracker-project-usage-detail`

- GET；`project_key`（**必填**，缺失回 400 `missing_project_key`）；定义 `src/lib/local-api.js:2828`。
- 返回（`:2900-2915`）：`generated_at`、`from`、`to`、`project_key`、`project_ref`、`totals{8 项}`、`days_active`、`range_total_tokens`（同范围内**全项目**总量，供前端算"占比"而无需二次请求）、`daily[]`（每天 `{day, ...measures, models{source: tokens}}`）、`sources[]`（每源 `{source, total_tokens, conversation_count, days_active}`）。
- 注释明确：**全部来自本地项目桶日志，无外部请求、无 git 调用**（`:2826-2827`）。

### 2.7 限额 / 订阅组端点

- `/functions/tokentracker-usage-limits`（GET；`refresh=1|true` 强制穿透缓存；`devin=1|true` 需本地鉴权，否则 401；定义 `src/lib/local-api.js:3482-3517`）→ 直接回 `getUsageLimits(...)` 的结果。详见 §5。
- `/functions/tokentracker-subscription-manager`（GET/POST；POST 动作 `create|update|delete`，需本地鉴权；定义 `src/lib/local-api.js:3240-3291`）。GET 返回 `{subscriptions:[...]}`；这是**用户手填**的续费/到期日，与限额窗口重置、与自动探测的 subscriptions 是三个不同概念（注释 `:3236-3239`）。
- `/functions/tokentracker-user-status`（GET；定义 `src/lib/local-api.js:2920`）：**本地 stub**，固定 `user_id:"local-user"`、`email:"local@localhost"`、`name:"Local User"`、`is_public:false`，`pro{active:true, sources:["local"], expires_at:null, partial:false, as_of}`，以及跨设备弹层的真实状态 `account{available, cloud_sync_enabled, account_view}`（`:2921-2932`）。macOS 壳用它做健康检查（`TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift:148-150`）。

### 2.8 成就 / 技能 / 宠物组端点

- `/functions/tokentracker-achievements`（GET；`tz*`；定义 `src/lib/local-api.js:3470`）→ `{generated_at, achievements:[...]}`（`:3474-3477`）。本地徽章阈值只有三枚，且**阈值只存在于服务端**：

```js
// src/lib/local-api.js:419-423
const LOCAL_BADGE_THRESHOLDS = {
  project_hopper: [3, 5, 10, 20], // distinct projects
  project_devotion: [1000000, 10000000, 100000000, 1000000000], // max tokens in one project
  night_owl: [5, 20, 60, 150], // active hour buckets between 00:00–05:59 local
};
```

  注释明确："the dashboard renders whatever the payload says and embeds none of these numbers"（`src/lib/local-api.js:416-418`）。
- `/functions/tokentracker-skills`（GET `mode=installed|repos|discover|search|popular|updates|activity|skill_usage`；POST 动作 `install|update_all|uninstall|restore|set_targets|import_local|delete_local|add_repo|remove_repo`；写操作需本地鉴权；定义 `src/lib/local-api.js:3294-3467`）。其中 `skill_usage` 会把技能调用量与安装列表 join，并按 `source:"claude"` 计价，返回 `{generatedAt, scannedFiles, totalInvocations, cached, skills[{skill, invocations, lastUsedAt, tokens, cost, installed, skillId, directory}], unusedInstalled[]}`（`:3337-3408`）。
- `/functions/tokentracker-pets`（GET（`scope=codex` 分支）/POST（`install_url|remove|import_codex`，需本地鉴权）；定义 `src/lib/local-api.js:3192-3233`）→ `{pets[], hiddenBuiltinIds[]}` 或 `{importable[], codexDetected}`。
- 宠物资产路由：`/api/pets/local/:id/spritesheet.webp`（`:1698`）与 `/api/pets/codex/:id/spritesheet.webp`（`:1724`），仅 GET/HEAD，回 `image/webp` + `no-cache` + `nosniff`；导入 `POST /api/pets/import`（`:1747`，body 上限 `pets.MAX_PACKAGE_BYTES`）。

### 2.9 设备 / 账号 / 云组端点

| 端点 | 方法 | 参数 / body | 顶层返回 | 行号 |
| --- | --- | --- | --- | --- |
| `/api/runtime-config.js` | GET | — | 输出 `window.__TOKENTRACKER_RUNTIME_CONFIG__={baseUrl, anonKey, dashboardUrl, configurationError}`（`application/javascript`，转义 `<` 与 U+2028/9） | `src/lib/local-api.js:1670-1679` |
| `/api/local-auth` | GET | — | `{token, serverVersion?}` | `src/lib/local-api.js:1681-1695` |
| `/api/auth-bridge/verifier` | GET/PUT/POST | body `{native:boolean}` | `{native}`（一次性读取，TTL 5 分钟）；写需本地鉴权 | `src/lib/local-api.js:1767-1788` |
| `/api/auth/*` | 任意 | 透传到 InsForge；`refresh|logout` 有 csrf/relay 特例 | 上游响应原样 | `src/lib/local-api.js:1792` 起 |
| `/functions/tokentracker-cloud-session` | POST | `{clear?, session_id?, expectedOwnerId, insforgeBaseUrl?}`；需本地鉴权 | `{session_id, device_id, created_at}`；失败码 `CLOUD_SYNC_DISABLED|auth_required|auth_session_changed|auth_rejected` | `src/lib/local-api.js:2137-2163` |
| `/functions/tokentracker-local-sync` | POST | `{auto?, background?, lightweight?, drain?, publishAccount?, allLocalSources?, nativeOnlyWsl?, recheckCloudAccess?, cloudSessionId?, expectedOwnerId?, insforgeBaseUrl?}`；需本地鉴权 | 成功 `{ok:true, ...syncResult, uploaded, pending, next_allowed_at?}`；失败 `{ok:false, error, code, recovery_url, next_allowed_at, retry_after_seconds, stdout, stderr}` | `src/lib/local-api.js:2166-2327`（返回在 `:2322`、`:2324`） |
| `/functions/tokentracker-cloud-sync-pref` | GET/POST | POST body `{enabled:boolean, changedAtMs?}`；写需本地鉴权 | GET `{enabled, account_available}`；POST `{ok, enabled}` | `src/lib/local-api.js:2940-2977` |
| `/functions/tokentracker-machine-id` | GET | — | `{machineId, deviceName}`（deviceName = 主机名，截断 128 字符） | `src/lib/local-api.js:3137-3143`、`:31-41` |
| `/functions/tokentracker-telemetry-pref` | GET | — | `{disabled:boolean}` | `src/lib/local-api.js:3121-3131` |
| `/functions/tokentracker-proxy-config` | GET/POST | POST body 经 `parseProxyPayload`；写需本地鉴权 | `{mode, protocol, host, port, effective, applyError}`（POST 另加 `ok`、`unprotected`） | `src/lib/local-api.js:2982-3072` |
| `/functions/tokentracker-proxy-test` | POST | body `{mode:"manual", ...}`；需本地鉴权 | `runProxyConnectivityTest(...)` 结果 | `src/lib/local-api.js:3074-3114` |
| `/proxy/ipcheck` | 任意 | 透传到 `https://ip.net.coffee`，剥离 hop-by-hop 头 | 上游响应 | `src/lib/local-api.js:939-1001` |
| `/api/geoip/`、`/api/geoip-batch`、`/api/iprisk/`、`/api/dns/result/` | 任意 | 代理到 `ip.net.coffee` 对应接口 | 上游响应 | `src/lib/local-api.js:1973-1976` |
| `/api/avatar-proxy` | GET | `?url=` | 图片字节（内存 LRU：1h TTL、512 KiB/条、64 条） | `src/lib/local-api.js:2038`、`:43-48` |

账号视图（`account=1`）是一个**包装层**而不是新端点：命中时把 `/functions/tokentracker-usage-*` 六个 slug 映射到云端 `account-*` 并代理，失败则打头降级：

```js
// src/lib/local-api.js:2347-2355
    if (url.searchParams.get("account") === "1") {
      const usageSlug = p.startsWith("/functions/") ? p.slice("/functions/".length) : "";
      if (accountSlugFor(usageSlug)) {
        const result = await tryServeAccountView(usageSlug, url, res);
        if (result === "served") return true;
        res.setHeader("X-TokenTracker-Account-View", "0");
        res.setHeader("X-TokenTracker-Account-Fallback", result);
      }
    }
```

映射表在 `src/lib/cloud-account.js:20-27`（summary/daily/hourly/monthly/heatmap/model-breakdown），成功时打头 `X-TokenTracker-Account-View: 1`（`src/lib/local-api.js:1565`）。失败冷却 2 分钟（`src/lib/local-api.js:27`）。

### 2.10 端点总表（按分组，含定义行号）

| 组 | 端点 | 行号 |
| --- | --- | --- |
| 用量/成本 | `/functions/tokentracker-usage-summary` | `src/lib/local-api.js:2358` |
| 用量/成本 | `/functions/tokentracker-usage-daily` | `src/lib/local-api.js:2425` |
| 用量/成本 | `/functions/tokentracker-usage-hourly` | `src/lib/local-api.js:3146` |
| 用量/成本 | `/functions/tokentracker-usage-monthly` | `src/lib/local-api.js:3156` |
| 用量/成本 | `/functions/tokentracker-usage-heatmap` | `src/lib/local-api.js:2533` |
| 用量/成本 | `/functions/tokentracker-usage-model-breakdown` | `src/lib/local-api.js:2596` |
| 用量/成本 | `/functions/tokentracker-usage-category-breakdown` | `src/lib/local-api.js:2668` |
| 用量/成本 | `/functions/tokentracker-wrapped` | `src/lib/local-api.js:2330` |
| 用量/成本 | `/functions/tokentracker-outcomes` | `src/lib/local-api.js:2441` |
| 用量/成本 | `/functions/tokentracker-context-health` | `src/lib/local-api.js:2526` |
| 会话 | `/functions/tokentracker-session-insights` | `src/lib/local-api.js:2470` |
| 会话 | `/functions/tokentracker-sessions` | `src/lib/local-api.js:2500` |
| 项目 | `/functions/tokentracker-project-usage-summary` | `src/lib/local-api.js:2731` |
| 项目 | `/functions/tokentracker-project-usage-detail` | `src/lib/local-api.js:2828` |
| 限额/订阅 | `/functions/tokentracker-usage-limits` | `src/lib/local-api.js:3482` |
| 限额/订阅 | `/functions/tokentracker-subscription-manager` | `src/lib/local-api.js:3240` |
| 成就/技能/宠物 | `/functions/tokentracker-achievements` | `src/lib/local-api.js:3470` |
| 成就/技能/宠物 | `/functions/tokentracker-skills` | `src/lib/local-api.js:3294` |
| 成就/技能/宠物 | `/functions/tokentracker-pets` | `src/lib/local-api.js:3192` |
| 成就/技能/宠物 | `/api/pets/local/:id/spritesheet.webp`、`/api/pets/codex/:id/spritesheet.webp`、`/api/pets/import` | `:1698`、`:1724`、`:1747` |
| 设备/账号/云 | `/functions/tokentracker-cloud-session`、`-local-sync`、`-cloud-sync-pref`、`-machine-id`、`-telemetry-pref`、`-proxy-config`、`-proxy-test`、`-user-status` | `:2137`、`:2166`、`:2940`、`:3137`、`:3121`、`:2982`、`:3074`、`:2920` |
| 运行时/鉴权 | `/api/runtime-config.js`、`/api/local-auth`、`/api/auth-bridge/verifier`、`/api/auth/*` | `:1670`、`:1681`、`:1767`、`:1792` |
| 代理 | `/proxy/ipcheck`、`/api/geoip/*`、`/api/iprisk/*`、`/api/dns/result/*`、`/api/avatar-proxy` | `:939`、`:1973-1976`、`:2038` |

---

## 3. Dashboard 如何消费

### 3.1 唯一的 API 层

`dashboard/src/lib/api.ts` 把端点名集中成一张表（`dashboard/src/lib/api.ts:141-158`），并额外维护"本地 usage-* → 云端 account-*"的映射（`:166-173`，注释明确"cloud `account-*` edge functions mirror the local `tokentracker-usage-*` response schema exactly … so the dashboard components render identically off either source"）。

### 3.2 本地 / 云端双路由

```js
// dashboard/src/lib/api.ts:191-199（节选）
  if (!isLocalhostHost() && accessToken && accountSlug) {
    const base = getInsforgeRemoteUrl().replace(/\/$/, "");
    const url = new URL(functionUrlFor(base, accountSlug));
    ...
    if (accountSlug === "tokentracker-account-heatmap") url.searchParams.set("format", "compact");
```

- 本地路径 `new URL(\`/functions/${slug}\`, window.location.origin)`，且 `cache: "no-store"`（`:222-234`）。
- 非 localhost 且无 token 时抛 `LOCAL_API_UNAVAILABLE`（404）（`:216-221`）。
- 账号响应缓存：summary 120 s、其余 300 s、`stale-if-error` 5 分钟、上限 64 条、按 `(origin, path+排序后的 query, token sub/anon key)` 建键（`:29-31`、`:75-129`）。
- 热力图走"紧凑线格式"再前端展开：请求带 `format=compact`（`:199`），响应过 `expandHeatmapCompact`（`:212`）；同一展开逻辑在服务端代理里也有一份（`src/lib/cloud-account.js:140`、`:392`、`src/lib/heatmap-compact.js:108-109`）。

### 3.3 页面 → 端点映射

| 页面/组件 | 消费的端点 | 证据 |
| --- | --- | --- |
| `DashboardPage.jsx` | usage-summary / daily / hourly / monthly / model-breakdown / heatmap / project-usage-* / sessions / category-breakdown | `dashboard/src/pages/DashboardPage.jsx`（导入与 useMemo 段，示例 `:565`、`:602`、`:819`）；端点常量 `dashboard/src/lib/api.ts:141-158` |
| `SessionsPage.jsx` | `/functions/tokentracker-sessions`、session-insights | `dashboard/src/lib/api.ts:681`（`getSessionInsights`）、`:157` |
| `WrappedPage.jsx` | `/functions/tokentracker-wrapped?year=` | `dashboard/src/pages/WrappedPage.jsx:43-45` |
| `LimitsPage.jsx` | usage-limits（+subscription-manager） | `dashboard/src/pages/LimitsPage.jsx:49-66`、`:123-133` |
| `AchievementsPage.jsx` | achievements | `dashboard/src/lib/api.ts:151`、`:328-334` |
| `LeaderboardPage.jsx` / `LeaderboardProfilePage.jsx` | 云端 leaderboard / leaderboard-profile / profile-likes | `dashboard/src/lib/api.ts:383-583` |
| `DevicePage.jsx` | account-devices（云） / device-rename | `dashboard/src/lib/api.ts:840-860`、`:1040-1063` |
| `CloudPage.jsx` / `CloudCheckoutPage.jsx` | billing / cloud-session / checkout | `dashboard/src/lib/cloud-checkout.js`、`dashboard/src/lib/api.ts:157` |
| `PetPage.jsx` / Linux 桌宠宿主 | pets、usage-summary、heatmap、model-breakdown、usage-limits | `dashboard/src/lib/pets-api.js:21`、`:39`、`:66`；`dashboard/src/lib/pet-linux-host.js:194`、`:211`、`:223`、`:237` |
| `SkillsPage.jsx` / `SkillDetailPanel.jsx` | skills（多 mode） | `dashboard/src/lib/api.ts:151` 同族；端点定义 `src/lib/local-api.js:3294` |
| 设置页账号区 | user-status / cloud-sync-pref / telemetry-pref / proxy-config | `dashboard/src/hooks/use-proxy-settings.js:4-5`、`dashboard/src/lib/cloud-sync-prefs.ts:98`、`:157`、`dashboard/src/lib/analytics.js:67` |

桌宠那一路值得单列：`pet-linux-host.js` 是一个**独立于 React 页面的取数宿主**，自己拼 `?weeks=52&tz=...&account=1` 并读 `data.streak_days`、`sources[].models[]`、`totals`（`dashboard/src/lib/pet-linux-host.js:194-237`）。

### 3.4 前端二次聚合清单（有证据的）

| 前端行为 | 位置 | 性质 |
| --- | --- | --- |
| 活跃天数优先取后端、缺失才重算 | `dashboard/src/pages/DashboardPage.jsx:819-850`（`const serverActive = Number(heatmap?.active_days); if (Number.isFinite(serverActive)) return serverActive;` 之后才遍历 `weeks`） | 后端优先 + 前端兜底 |
| 连续天数（streak）前端重算并覆盖 | `dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295` | 纯前端（服务端本地值为 0） |
| 紧凑热力图展开 | `dashboard/src/lib/heatmap-compact.js:20-70`（含 `levelOf` 重算 level） | 传输优化后的前端还原，**会重算 level** |
| 趋势缩放统计（总量/成本/活跃桶/峰值） | `dashboard/src/lib/trend-stats.js:34-79` | 前端二次汇总（数据来自 daily/hourly/monthly 三端点之一） |
| 桶标签的本地化与时区无关格式化（一律 `timeZone:"UTC"`） | `dashboard/src/lib/trend-stats.js:106-138`、`:152-180` | 前端展示层 |
| 明细表排序/分页 | `dashboard/src/pages/DashboardPage.jsx:681-750`（`detailsRows`/`sortedDetails`/`pagedDetails`/`sortedDailyBreakdownRows`） | 前端（后端用量端点不分页） |
| 显示币种换算 | `dashboard/src/hooks/useCurrency.js:3-15`（`rate`、`symbol`、`rates`、`rateSource`） | 前端换算（服务端只给 USD） |
| token 数字格式 | `dashboard/src/hooks/useTokenFormat.js:2-20`、`dashboard/src/lib/token-format.js` | 前端展示 |
| 榜单列宽/排序/分页配置 | `dashboard/src/lib/leaderboard-columns.js`、`leaderboard-ui.js` | 前端 |
| 份额/占比 | `project-usage-detail` 已给 `range_total_tokens`（`src/lib/local-api.js:2908-2910`），前端只做除法 | 后端给分子分母 |

### 3.5 只在后端算的东西

- 逐行成本 `computeRowCost(row)`：只出现在 `buildDailyAggregates`、`usage-model-breakdown`、`skill_usage` 定价段（`src/lib/local-api.js:285`、`:2640`、`:3381`）。前端从不重新计价。
- 去重（`source|model|hour_start` last-wins）、旧行修正在 `readQueueData` 的 `parseRow` 内完成（`src/lib/local-api.js:202-211`、`:170-200`）；前端拿到的是已去重序列。
- 项目归属、claude-mem 伪项目剔除（`src/lib/local-api.js:92-94`、`:136-138`）。
- 热力图的 level（服务端按窗口最大值归一，`:2549-2557`；前端展开紧凑格式时另算一份，见上表）。
- 本地徽章阈值与达成时间（`src/lib/local-api.js:419-423`、`:434-511`）。
- 账号级来源的跨设备去重语义（`src/lib/source-metadata.js:22-40` + 云端 RPC）。
- 会话效率的一整套定义（edit-turn-v2、first-pass、one-shot、subagent 归属）：`src/lib/session-analytics.js:2385-2566`。

### 3.6 轮询、缓存与去重

- 进程内 in-flight 合并：`coalesceJsonGet()`（`dashboard/src/lib/api.ts:62-73`），键是完整 URL；账号路径再套一层 TTL 缓存（`:75-129`）。
- 预取：`DASHBOARD_PRELOAD_TARGETS = ["limits", "leaderboard"]`（`dashboard/src/lib/dashboard-preload.js:3`），预取状态机与 20 条榜单内存 LRU（`:23-71`）。
- 本地事件驱动刷新：macOS 壳监听 `queue.jsonl` 写入与 `queue.state.json`（`TokenTrackerBar/TokenTrackerBar/Services/QueueActivityMonitor.swift:3-5`），突发写入 settle 后再触发一次刷新。
- Windows 托盘轮询：健康检查 30 s（`TokenTrackerWin/Constants.cs:14-15`），用量轮询在 `UsagePoller`（`TokenTrackerWin/UsagePoller.cs:85-131`）。
- Linux GNOME 扩展：`Soup.Session({timeout: 20})`（`extension.js:784`），菜单打开与固定周期都会取数（`:830-832`）。
- 分析埋点：PostHog（`dashboard/src/lib/analytics.js:24-45`），`autocapture: false`，且在本地/原生环境先问 `/functions/tokentracker-telemetry-pref` 且"答不出来就关"（`:13`、`:67`）。

---

## 4. 数据导出与分享

### 4.1 Share Card

数据组装链：页面状态 → `useShareCardData()` → `buildShareCardData()` → `ShareCard.jsx` → 截图。

- 输入与来源（`dashboard/src/ui/share/use-share-card-data.ts:11-26`、`:49-80`）：`handle`、`startDate`、`activeDays`、`summary`、`topModels`、`heatmap` 均来自当前页面已拉到的数据；**`rank` 单独请求**：

```ts
// dashboard/src/ui/share/use-share-card-data.ts:55-63（节选）
        const leaderboardPeriod = period === "day" || period === "custom" ? "week" : period;
        const payload = await getLeaderboard({
          accessToken,
          userId,
          period: leaderboardPeriod,
          metric: "all",
          limit: 100,
          offset: 0,
        } as any);
```

  注意 `period==="day"` 或 `"custom"` 时榜单按 `"week"` 取，`limit: 100` 且只在第一页里找 `is_me`——**排名超过 100 时不显示**（`:70-72`）。
- 数值口径：`totalTokens = billable_total_tokens ?? total_tokens`、`totalCost = total_cost_usd`（`dashboard/src/ui/share/build-share-card-data.ts:47-50`、`:118-119`）；`topModels` 上限 3；热力图单元格 `level` 夹在 0–4（`:62-85`）。
- 截图：`capture-share-card.ts` 动态 `import("html-to-image")`（`:53`），并注入 Google Fonts（`:4-5`）——这一条 PRIVACY 已披露（`docs/PRIVACY.md:86`）。
- 交互：复制 / 下载 / 分享到 X，以及原生壳回传（`dashboard/src/ui/share/ShareModal.tsx:124`、`:152`、`:180`、`:220`）。
- 契约测试锁：`test/share-card-data.test.js:23-138`（完整数据、缺失回落 `—`、topModels 截断到 3、非法 rank 归 null、`formatCost` 千分位规则）。

### 4.2 徽章 SVG（云端）

- 文件：`dashboard/edge-patches/tokentracker-badge-svg.ts`（295 行）。
- 用法（源码头注释，`:7-21`）：`/functions/tokentracker-badge-svg?user_id=<uuid>&metric=tokens|cost|rank`，缓存 60 s。
- 渲染：`renderBadgeSvg()`（`:136-148` 起）自绘 SVG（含 `flat-square` 圆角分支），数字格式化 `compactNumber`（`:68`）、金额 `formatCost`（`:78`）。
- 数据：先解析榜单窗口 `windowBounds(client, period)`（`:106-129`），再读快照行（`rank`、`total_tokens`）；被封禁账号走黑名单（`:37`、`:25-26` 对应的 blocked 集合）。
- 契约测试是非 Deno 的 CommonJS 移植镜像：`test/badge-svg-render.test.js:1-14`（注释声明"Deno copy 是 source of truth，改渲染必须同步"）。

### 4.3 个人嵌入卡片 SVG（云端）

`dashboard/edge-patches/tokentracker-embed-svg.ts`（320 行）：同样的窗口计算（`:149-159`）、头像由显示名哈希生成（`avatarSvg`，`:109-123`）、主题 light/dark，占位文案 `placeholderSvg`（`:124`）。它面向"把个人卡片嵌到 README/主页"的场景。

### 4.4 Wrapped（三条消费路径同一份计算）

| 路径 | 取数 | 渲染 |
| --- | --- | --- |
| `tracker wrapped`（CLI） | 直接读 `queue.jsonl`（`src/commands/wrapped.js:126-128`） | ASCII 框（`:43-120`）/ `--json` |
| `/wrapped` 页 | `fetch("/functions/tokentracker-wrapped")`（`dashboard/src/pages/WrappedPage.jsx:43-45`） | React，`highlights` 用 copy 模板重排（`:100-121`） |
| 本地 API | `aggregateWrapped(rows, {year})`（`src/lib/local-api.js:2333-2336`） | JSON |

三者共用 `src/lib/wrapped-aggregator.js`，因此**口径一致（含同一处 UTC 偏向）**。

### 4.5 营销口径 vs 统计口径

- **不存在 "节省金额" 类指标**：全仓 `savings|amount_saved|saved_amount|total_saved` 共 9 处命中（7 个文件），唯一与"折扣"有关的是 `dashboard/src/pages/CloudPage.jsx:95` 的 `cloudAnnualSavings(monthly, annual)`——订阅年付 vs 月付的折扣百分比，展示在价格行（`:195`）；其余为 i18n 文案（`dashboard/src/content/i18n/zh/dashboard.json:650`、`zh-TW/dashboard.json:709`）、`dashboard/src/content/copy.csv:1699`、`DESIGN.md:17` 与两处 `bill_savings_claimed: false` 免责声明（`scripts/cloud-sandbox/hosted-archive-acceptance.cjs:377`、`:450`），全部与用量统计无关。
- 真正存在**口径错配**风险的是这三处：
  1. **Wrapped 的 UTC 偏向**：`peak_hour`、`longest_streak`、`active_days`、`top.days` 全部按 UTC 计算（`src/lib/wrapped-aggregator.js:37-53`、`:142-168`），而页面上"活跃天数"取自按本地时区切天的 heatmap（`src/lib/local-api.js:213-225`）。同一用户同一年会看到两个不同的"活跃天数"。
  2. **Share card 的 rank 与 token 口径不同源**：token/成本来自本地或账号视图，rank 来自云端榜单（`dashboard/src/ui/share/use-share-card-data.ts:55-63`）；未登录时 rank 为 null（`:51`）。
  3. **`wrapped` 用 billable 优先**（`src/lib/wrapped-aggregator.js:29-35`）而 `top.models` 份额按同一 billable 值算（`:182-186`）；而 `usage-daily` 里 `models{}` 用的是 `total_tokens`（`src/lib/local-api.js:297`）。两处"模型占比"分母不同。

---

## 5. 限额视图（与用量统计相邻，但完全独立的链路）

### 5.1 数据来自 provider API + 本地磁盘缓存，不来自队列

- 抓取入口：`fetchUsageLimitsUncached()`（`src/lib/usage-limits.js:3976`）并行拉取 18 个 provider（`src/lib/usage-limits.js:4055`）。
- 缓存：进程内按"选择键"分区，TTL 2 分钟（`CACHE_TTL_MS`，`src/lib/usage-limits.js:64`）。**【R2 修正（I5）】选择键不是"由 `{devinEnabled, ...}` 生成"，而是二元组 `${devin}-${antigravity}`**（`usageLimitsSelectionKey`，`src/lib/usage-limits.js:59-63`）：`devin` = `options.devinEnabled === true ? "on" : "off"`，`antigravity` = `isAntigravityQuotaDisabled() ? "off" : "on"`（即"禁用开关取反"）；分区枚举恰四种 `off-on` / `off-off` / `on-on` / `on-off`（`:56-58`）。同一键并发请求合并（single-flight，`getUsageLimits` `:3954-3970`，in-flight 表 `:3962-3968`）。
- 磁盘缓存：每家 provider 一份 `*-usage-limits-cache.json`（如 `CLAUDE_LIMITS_CACHE_FILE`、`CODEX_LIMITS_CACHE_FILE`，`src/lib/usage-limits.js:118`、`:128`；Antigravity 走 `ANTIGRAVITY_LIMITS_CACHE_FILE`，`:70`），最长保留 7 天（`:71`、`:119`、`:129`）。Claude 另有"新鲜度 10 分钟"与 429 冷却（`:120`、`:141-144`）。
- **与 token 用量的界线**：限额数据不写 `queue.jsonl`；PRIVACY 把缓存文件单列一行（`docs/PRIVACY.md:43`）。限额端点由 `/functions/tokentracker-usage-limits` 暴露（`src/lib/local-api.js:3482`），参数 `refresh`、`devin`。

### 5.2 Provider 清单与关键函数

| 输出键 | 抓取函数 | 数据来源（源码证据） |
| --- | --- | --- |
| `claude` | `fetchClaudeUsageLimits`（`src/lib/usage-limits.js:281`） | Claude OAuth 用量接口 + 本地缓存 |
| `codex` | `fetchCodexUsageLimits`（`:601`） | Codex auth bundle + token 刷新（`:4005-4037`） |
| `cursor` | `fetchCursorLimits`（`:762`） | Cursor 本地凭据 |
| `kimi` | `fetchKimiLimits`（`:927`） | Kimi 凭据（`:798-909`） |
| `gemini` | `fetchGeminiLimits`（`:1251`） | Gemini CLI 凭据/配额（`:974-1250`） |
| `kiro` | `fetchKiroLimits`（`:2008`） | CLI 输出解析（`:1429-1536`）+ 积分侧车（`:1536`） |
| `antigravity` | `fetchAntigravityLimits`（`:3666`） | 进程探测 + 本地回环端口 + 凭据（`:2158`、`:2809`、`:3259`） |
| `copilot` | `fetchCopilotLimits`（`:1954`） | keychain / auth DB 解密（`:1583-1880`） |
| `grok` | `fetchGrokLimits`（`src/lib/grok-limits.js`） | 独立模块 |
| `zcode` | `fetchZcodeLimits`（`src/lib/zcode-limits.js`） | 独立模块 |
| `opencodeGo` | `fetchOpencodeGoLimits`（`src/lib/opencode-go-limits.js`）+ 本地估算（`src/lib/usage-limits.js:1414` 附近常量） | 官方窗口 + 本地估算 |
| `commandCode` | `*-limits` 模块（`src/lib/commandcode-limits.js`） | 独立模块 |
| `devin` | `fetchDevinLimits`（`src/lib/devin-limits.js`） | 默认关闭，需显式开关 + 本地鉴权 |
| `qoder` / `qoderCn` | `src/lib/qoder-limits.js`、`src/lib/qoder-cn-cli-limits.js` | 独立模块 |
| `codingPlan` / `agentPlan` | `src/lib/ark-coding-plan-limits.js`、`src/lib/ark-agent-plan-limits.js` | 独立模块 |

### 5.3 输出结构与 provenance

```ts
// src/lib/usage-limits.js:4406-4440（节选）
  const data = {
    fetched_at: new Date(nowMs).toISOString(),
    claude: withPlanLabel(claude, claudePlanType, "Claude"),
    ...
    grok,
    zcode: withPlanLabel(zcode, zcode.plan_label, "ZCode"),
    opencodeGo: withPlanLabel(opencodeGo, opencodeGo?.plan_label, "OpenCode Go"),
    commandCode: commandCodeObj,
    devin: withPlanLabel(devinObj, devinObj?.plan_label, "Devin"),
    qoder: withPlanLabel(qoder, qoder?.plan_label, "Qoder"),
    ...
  };
```

每家 provider 随后被统一注入可信度元数据（`src/lib/usage-limits.js:4442-4461`）：

- `stale`：显式 `stale===true` 或 `cached_at` 早于 10 分钟。
- `source`：显式来源，否则 `disk-cache`（stale）/ `provider-api`。
- `confidence`：来源串匹配 `estimate|inferred` → `inferred`；匹配 `local|database|sqlite|cache` → `observed`；否则 `official`。
- 另外附带 `captured_at`、`age_seconds`。

窗口结构由 `buildWindow({usedPercent, resetAt, windowSeconds})` 统一构造（`src/lib/usage-limits.js:161`），百分比经 `clampPercent`（`:146`）。Codex 有两类窗口常量：5 小时 = 18000 s、每周 = 604800 s（`:332-333`）。

### 5.4 前端展示

- `LimitsPage.jsx` 直接从响应对象 `Object.entries(data)` 铺平 provider 行（`dashboard/src/pages/LimitsPage.jsx:49`），并用 `provenance.stale` / `provenance.source==="disk-cache"` 判定"全部来自缓存"提示（`:59-66`）。
- provider 展示规格与顺序：`dashboard/src/ui/dashboard/components/usage-limits-provider-specs.js`；展示偏好：`dashboard/src/hooks/use-limits-display-prefs.js`。
- 提醒与"节奏预测"：`dashboard/src/lib/limit-alerts.js`、`limit-pace.js`（原生壳亦有对应实现：`TokenTrackerBar/TokenTrackerBar/Models/LimitPace.swift`、`TokenTrackerBarTests/LimitPaceTests.swift`）。
- 一致性测试：`test/limits-providers-parity.test.js`（36 行）锁定前后端 provider 清单一致。

---

## 6. 云上传与聚合

### 6.1 上传内容（字段级）

客户端请求体（`src/commands/sync.js:4367-4374`）：

```js
        body: JSON.stringify({
          upload_id: uploadId,
          hourly: result.buckets,
          ...(result.sessionStates.length > 0
            ? { account_session_states: result.sessionStates }
            : {}),
        }),
```

ingest edge 把它逐列映射为数据库行（`dashboard/edge-patches/tokentracker-ingest.ts:176-194`）：

```ts
  const mappedRows = buckets.map((b: Record<string, unknown>) => ({
    user_id: userId,
    device_id: deviceId,
    hour_start: b.hour_start,
    source: b.source || "unknown",
    model: b.model || "unknown",
    input_tokens: b.input_tokens || 0,
    cached_input_tokens: b.cached_input_tokens || 0,
    cache_creation_input_tokens: b.cache_creation_input_tokens || 0,
    output_tokens: b.output_tokens || 0,
    reasoning_output_tokens: b.reasoning_output_tokens || 0,
    total_tokens: b.total_tokens || 0,
    billable_total_tokens: b.billable_total_tokens || 0,
    total_cost_usd: Number(b.total_cost_usd) || 0,
    conversations: b.conversation_count ?? b.conversations ?? 0,
  }));
```

对照 `docs/PRIVACY.md:102` 的"hourly usage buckets，每个恰好包含 hour_start/source/model/六类 token/conversation_count"——**实现多了 `billable_total_tokens` 与 `total_cost_usd` 两个字段**（金额与"计费口径 token"），文档清单未列。

会话状态（仅 `trae-cn`）字段（`dashboard/edge-patches/tokentracker-ingest.ts:153-160`）：`source`、`session_id`、`model`、`bucket_start`、`snapshot_verified_at` + 六类 token；客户端侧同样只接受 `source==="trae-cn"` 且要求 `total = input + cached + cacheCreation + output`（`src/lib/upload-queue.js:9-18`）。

### 6.2 频率与节流

| 参数 | 值 | 来源 |
| --- | --- | --- |
| 基础间隔 | 30 分钟 | `src/lib/upload-throttle.js:2` |
| 抖动 | ≤ 60 s | `:3` |
| 积压阈值 | 1,000,000 字节 | `:4`（与 `src/lib/upload-queue.js:5` 的 `BACKLOG_BYTES` 同值） |
| 批量 | 300（后端 batch 上限）/ 客户端读取 batch 默认 200、ingest 上限 500 | `:5`、`src/commands/sync.js:4439-4441`、`dashboard/edge-patches/tokentracker-ingest.ts:172-174` |
| 批量档位 | 小 2 批 / 大 4 批 | `src/lib/upload-throttle.js:6-7`、`:57` |
| 失败退避 | 60 s 起、上限 30 分钟、指数 `2^step`、step 上限 10（记录时上限 20） | `:8-9`、`:109-110`、`:123` |
| 服务端截止 | `Retry-After` 优先于配置上限；`cloud_sync_throttled` + `429` 时把服务端时间作为 `nextAllowedAtMs` | `:104-107`、`:114-116` |
| 生效判定 | `max(nextAllowedAtMs, backoffUntilMs)` 未到即拒绝 | `:46-55` |
| 心跳节流 | 24 小时 + 60 s 抖动 | `src/lib/telemetry.js:41-46`、`:116-124` |

云侧还会下发 `sync_interval_seconds` 与 `next_allowed_at`，客户端取更晚者（`src/lib/upload-throttle.js:72-77`）。

### 6.3 边缘函数清单与要点（`dashboard/edge-patches/`）

共 28 个文件（本文只列与统计口径相关的；目录内全部为 `.ts`）：

> 行数口径（R1 复核后统一）：本节及全文所有「（N 行）」均为**总行数**，取 `[System.IO.File]::ReadAllLines(...,UTF8).Length`，与 `read` 工具的分行一致；不再使用 `Measure-Object -Line`（那是非空行数口径，两者相差 24–35 行，先前版本按该口径抓取的值已据此刷新）。

| 文件 | 角色与关键点 |
| --- | --- |
| `tokentracker-ingest.ts`（203→225 行） | 用量写入。Bearer = device token → sha256 → 查 `tokentracker_device_tokens`（`:69-82`）；封禁用户 403（`:92-94`）；批内 `(hour_start, source, model)` 取 `total_tokens` 最大（`:203-211`）；调 `cloud_ingest_usage` RPC 并带 `p_upload_id`（`:214-218`）；请求体上限 1 MiB（`:47-49`） |
| `tokentracker-telemetry.ts`（85 行） | 匿名心跳；`machine_hash` 必须匹配 `/^[0-9a-f]{64}$/`（`:25`、`:53`）；注释声明"one row per (machine_hash, day)"（`:8-9`）、"cannot be joined to device rows"（`:6-7`） |
| `tokentracker-account-summary.ts`（954 行） | 云端概览，**显式声明镜像 `local-api.js` 的响应 schema**（`:3`、`:12`）；用 `account_summary_wire` RPC（`:690`）并解码紧凑线格式（`:623`）；自带模型定价表与 `getModelPricing`（`:398`、`:524`、`:718`）；跨设备去重（`:22`）；交叉设备 30 s 共享缓存（`:662`） |
| `tokentracker-account-daily.ts`（894 行） | 日汇总镜像 |
| `tokentracker-account-hourly.ts`（333 行） / `-monthly.ts`（261） | 小时/月镜像 |
| `tokentracker-account-heatmap.ts`（391 行） | 热力图镜像；`format=compact` 紧凑线格式（`:114-147`）；30 s 缓存 + 5 分钟 stale-if-error（`:145-148`） |
| `tokentracker-account-model-breakdown.ts`（956 行） | 模型维度镜像 |
| `tokentracker-account-devices.ts`（268 行） | 设备维度（榜单/账号页的设备过滤） |
| `tokentracker-account-skills.ts`（251 行） | 技能维度 |
| `tokentracker-leaderboard.ts`（262 行） | 读榜单快照；`period=week|month|total` 决定 `(from_day, to_day)`（`:97`、`:131-152`）；按 `rank` 升序分页（`:167-181`）；附徽章数据 RPC（`:220`） |
| `tokentracker-leaderboard-refresh.ts`（1449 行） | 榜单快照生成（含异常账号排除逻辑，见 ingest 注释 `:84-91`） |
| `tokentracker-leaderboard-profile.ts`（1074 行） | 个人公开档案页数据 |
| `tokentracker-community-models.ts`（150 行） | 社区模型统计，**读预计算表 `tokentracker_community_stats`**（`:84-85`），并被测试要求不得回退到全表扫描（`test/community-stats-cache.test.js:16-20`） |
| `tokentracker-public-visibility.ts`（210 行） | 公开资料可见性开关 |
| `tokentracker-profile-likes.ts`（354 行） | 点赞 |
| `tokentracker-device-token-issue.ts`（183 行） / `-device-flow-*.ts`（3 个） / `-device-rename.ts`（164 行） | 设备令牌与设备管理 |
| `tokentracker-badge-svg.ts` / `-embed-svg.ts` | 分享徽章/嵌入卡（见 §4.2、§4.3） |
| `tokentracker-billing.ts`（492 行）+ 4 个 webhook（alipay/paddle/waffo/wechat） | 计费（不在本议题范围） |

### 6.4 community-stats / leaderboard 与本地口径的差异

1. **数据总体不同**：本地看的是本机 `queue.jsonl`；榜单看的是"所有已上传设备"的快照表。未登录/未同步的历史在云端根本不存在。
2. **去重语义不同**：账号级来源（`cursor`、`trae-cn`）跨设备**去重不求和**，本地单机视角没有这个问题；一致性由 `test/account-source-parity.test.js:3-18` 强制（4 处硬编码必须同步，否则"跨设备重复计数 bug 回归"）。
3. **时间窗口不同**：榜单按 `(period, from_day, to_day)` 快照查询（`dashboard/edge-patches/tokentracker-leaderboard.ts:131-152`），窗口由服务端刷新任务写入；本地端点按请求参数实时算。
4. **成本口径可能不同**：云端自带一份定价实现（`tokentracker-account-summary.ts:398`、`:524`），与本地 `src/lib/pricing/` 是两份代码；`test/edge-pricing-parity.test.js`（420 行）存在的意义就是防止两者漂移（但其第一层只做 edge↔edge 比对，不含 `edge ⊇ curated` 断言——见 S5 §7.4）。
5. **可见性**：榜单只展示开公开资料的用户；关闭后从榜单移除并把徽章变成占位（`docs/PRIVACY.md:113`）。

### 6.5 归档逻辑（`docs/cloud-usage-archive.md` 摘要）

- 热表保留小时级原始行；冷档把完整小时记录按 `(user, device, UTC day)` 打成私有 PostgreSQL JSONB 包（`:3`）。
- 读取路径 `cloud_usage_hourly(user, device, from, to)`：先按用户/设备/UTC 日筛 manifest，再展开 JSONB，并按 `(user_id, device_id, hour_start, source, model)` + 服务端修订号降序合并冷热（`:17`）；仅 `project_admin` 可执行。
- prepare/commit 两步：`cloud_prepare_usage_archive(user, device, day)`（单包上限 10,000 条 / 8 MiB）与 `cloud_commit_usage_archive(generation)`，后者只在"修订号仍等于快照"时删热行（`:35-39`）。
- 可见性限制与保留策略：个人小时级 90 天、日/月级 24 个月属于**可见性**限制，不是删除（`:13`）。
- 本地 runner `scripts/cloud-usage-archive.cjs` 默认 dry-run，支持 `--action archive|restore|cleanup|erase-user`，`--apply` 才写，凭据不入 checkpoint（`:48-53`）。
- 该文档末尾明确"**尚未启用任何调度**、未迁移或删除生产数据"（`:5`、`:74-83`）。

---

## 7. 三端原生壳如何拿到统计

### 7.1 端口矩阵

| 端 | 端口 | 证据 |
| --- | --- | --- |
| CLI `serve`（默认） | 7680，忙则顺延（最多 20 次） | `src/commands/serve.js:14`、`:22`、`:208-215` |
| CLI under WSL | 7681（7680 被 Windows DoSvc 占用，见 issue #267） | `src/commands/serve.js:15-21`、`:202-206` |
| macOS app | 固定 7680（`NativeQAProfile` 可覆盖） | `TokenTrackerBar/TokenTrackerBar/Utilities/Constants.swift:4-5` |
| Windows app | 运行时挑一个空闲回环端口，**优先 17680** | `TokenTrackerWin/ServerManager.cs:469-476`、`:157` |
| Linux app | 固定 17680 | `TokenTrackerLinux/gnome-extension/tokentracker@tokentracker.cc/extension.js:18-19`；`TokenTrackerLinux/src-tauri/src/server.rs`（PREFERRED_PORT/记录/回收） |

### 7.2 macOS

- 启动内嵌 server：找 CLI 入口后 spawn `[entryPath, "serve", "--port", "<7680>", "--no-sync", "--no-open"]`（`TokenTrackerBar/TokenTrackerBar/Services/ServerManager.swift:195`）；走登录 shell 的兜底分支是 `zsh -lc "<bin> serve --port <7680> --no-sync"`（`:228`）。启动前会 `lsof -ti tcp:<port> -sTCP:LISTEN` 清掉占用者（`:83-99`）。
- 数据入口是 HTTP，不是直读文件：`APIClient` 逐端点调本地 API（`TokenTrackerBar/TokenTrackerBar/Services/APIClient.swift`）：

| 方法 | 路径 | 行号 |
| --- | --- | --- |
| `fetchSummary` | `/functions/tokentracker-usage-summary?from&to` | `:43-52` |
| `fetchDaily` | `/functions/tokentracker-usage-daily` | `:66-69` |
| `fetchHeatmap` | `/functions/tokentracker-usage-heatmap?weeks=52`（`tz`/`tz_offset_minutes`/`account=1` 由 helper 追加） | `:73-75` + `:241-254` |
| `fetchModelBreakdown` | `/functions/tokentracker-usage-model-breakdown` | `:79-82` |
| `fetchProjectUsage` | `/functions/tokentracker-project-usage-summary` | `:86-90` |
| `fetchMonthly` / `fetchHourly` | `/functions/tokentracker-usage-monthly` / `-hourly?day=` | `:94-97` / `:101-103` |
| `fetchUsageLimits` | `/functions/tokentracker-usage-limits`（`devin=1` 可选） | `:110-113` |
| `fetchSubscriptions` | `/functions/tokentracker-subscription-manager` | `:119-121` |
| `triggerSync` | `/functions/tokentracker-local-sync` | `:127-143` |
| `checkServerHealth` | `/functions/tokentracker-user-status`（只判 200） | `:148-150` |

- 表内所有请求都经查询串 helper：`withTimeZoneQueryItems` 追加 `tz` + `tz_offset_minutes`（`APIClient.swift:241-246`），`withAccountQueryItems` 在其上再追加 `account=1`（`:252-254`）；`fetchSummary`/`fetchHeatmap`/`fetchModelBreakdown`/`fetchMonthly`/`fetchHourly` 用后者、`fetchProjectUsage` 用前者。**【R2 补（E1 + U1）】即 macOS 壳不缺 tz**；`fetchHourly(day:)` 的 `day` 来自 `DateHelpers.dayRange`（`DateHelpers.swift:44-50`，用 `localDayFormatter` = `.current` 时区），调用点 `DashboardViewModel.swift:192-194`/`:311`——是**本机时区**的 `yyyy-MM-dd`，与随请求发出的 `tz` 同基准。
- 菜单栏/弹出层：`StatusBarController`（1507 行）与 `DashboardViewModel`（1151 行）驱动，后者是唯一的取数状态机。
- 小组件（`TokenTrackerWidget/`）：**不联网**，读 App Group 里的快照文件。
  - 容器与文件名：`group.com.tokentracker.bar`、`widget-snapshot.json`、schema 版本 2（`TokenTrackerBar/Shared/WidgetSnapshot.swift:18-30`）。
  - 快照字段：`serverOnline`、`today`/`last7d`/`last30d`/`total`/`selected` 的 `PeriodTotals`、`dailyTrend[]`、`topModels[]`、`sources[]`、`heatmap`、`limits[]`（`:35-66`）。
  - 写入时机：`DashboardViewModel.loadAll()` 之后由 `WidgetSnapshotWriter.update(from:capturedAt:)` 原子写入并 `WidgetCenter.reloadAllTimelines()`（`TokenTrackerBar/TokenTrackerBar/Services/WidgetSnapshotWriter.swift:54-110`）。
- **唯一直接触碰本地文件的路径**是活动监听（不解析内容）：`QueueActivityMonitor` 以 `O_EVTONLY` 打开 `~/.tokentracker/tracker/queue.jsonl` 监听写入/替换，用于"实时活动"与"突发写完后 settle 再刷新"；第二实例监听 `queue.state.json` 以发布"账号上传完成"（`TokenTrackerBar/TokenTrackerBar/Services/QueueActivityMonitor.swift:1-36`）。

### 7.3 Windows

- 端口由宿主决定：`Port = PickServerPort()`（优先 17680，避开 DoSvc 的 7680，并满足 OAuth 回调白名单），`BaseUrl => http://127.0.0.1:{Port}`（`TokenTrackerWin/ServerManager.cs:12-20`、`:34-38`、`:157`、`:469-476`）；健康循环 30 s、启动等待 20 s（`TokenTrackerWin/Constants.cs:14-18`）。
- 托盘取数用 `UsagePoller`（自建 `HttpClient`，`UseProxy=false`，6 s 超时，`:38-40`）：

| 方法 | 路径 | 行号 |
| --- | --- | --- |
| `FetchLimitsAsync` | `/functions/tokentracker-usage-limits` | `:182-187` |
| `FetchAsync`（主汇总） | `/functions/tokentracker-usage-summary?from&to...` | `:226-250` |
| `FetchHeatmapAsync` | `/functions/tokentracker-usage-heatmap?weeks=52&tz...&account=...` | `:316-328` |
| `FetchTopModelsAsync` | `/functions/tokentracker-usage-model-breakdown?...` | `:340-354` |

- UI 与桥接：`DashboardWindow.cs`（WebView2，1235 行）、`WidgetWebViewEnvironment.cs`、`QuotaWidgetWindow.cs`（配额小组件）、`TrayApplicationContext.cs`（托盘与菜单）、`LocalSyncPublisher.cs`（同步结果广播）、`WebViewActivity.cs`。
- 宿主还会驱动后台同步（`ServerManager.cs:313` 里 `LocalSyncPublisher` 在服务器就绪后发布），与 CLI 侧的 `TOKENTRACKER_NATIVE_SYNC_OWNER=windows-host` 判定互斥（`src/commands/serve.js:305-309`）。

### 7.4 Linux

- Tauri 壳：`src-tauri/src/server.rs` 负责 spawn/记录/回收内嵌 server（每进程一个 `server-{owner_pid}.json`，含 pid/port/boot_id/进程启动时间，`:138-160`、`:246-257`；`reap_orphaned_servers()` 在启动时清理孤儿，`:343-372`）；`main.rs`、`tray.rs`、`pet.rs`、`cloud_export.rs` 分别负责窗口/托盘/桌宠/云导出。
- GNOME 扩展直连 HTTP：`const BASE_URL = 'http://127.0.0.1:17680'`（`extension.js:18-19`），用 `Soup` 请求：

| 用途 | 路径 | 行号 |
| --- | --- | --- |
| 周期数据 | `/functions/tokentracker-usage-model-breakdown`、`-hourly?day=`（`day` = `dayString(new Date())`，**本机时区**当日，`extension.js:1044`/`:273-275`）、`-monthly` | `:1046-1051` |
| 今日摘要 | `/functions/tokentracker-usage-summary?from=today&to=today` | `:1086` |
| 全量视图 | `/functions/tokentracker-usage-summary`（total）、`-daily`、`-heatmap?weeks=52` | `:1110-1116` |
| 限额 | `/functions/tokentracker-usage-limits` | `:1137` |
| 触发同步 | `POST /functions/tokentracker-local-sync` | `:1025` |

  扩展还把"非 404 的 HTTP 错误"与"端口被别的服务占用（404 → `ForeignServerError`）"区分开（`:229-245`），避免读到别人的服务。所有 `/functions/` 读取都经 `_getJson`，它自动把 `tz`、`tz_offset_minutes` 与 `account=1` 拼进查询串（`extension.js:997`）——【R2 补（E1）】即 Linux 壳同样带 tz。

### 7.5 是否存在"绕过 serve 直读队列"的路径

- 三端原生**壳**未发现解析 `queue.jsonl` 的代码：唯一的文件级接触是 macOS 的文件事件监听（§7.2）。Windows 只碰 `~/.tokentracker/pets`（`TokenTrackerWin/PetWindow.cs:1042`、`TrayApplicationContext.cs:566`）。
- **例外是 Vite dev server 插件**（仅 `npm run dev` 时存在）：`dashboard/vite.config.js:418` 注释"本地数据 API 插件 - 直接读取 ~/.tokentracker/tracker/queue.jsonl"，路径常量在 `:570`，`readQueueData()` 复刻了 last-wins 去重与 codex 旧行修正（`:583-608`），`aggregateByDay()` 复刻日聚合（`:610-639`）。这是**第四份**聚合实现（local-api.js、wrapped-aggregator.js、vite 插件，以及云端 edge 的 account-* 系列）。

---

## 8. 隐私边界核验

### 8.1 文档声明

- `docs/PRIVACY.md:17-26`：prompt/响应/对话正文、文件内容、commit message 与 diff、provider 凭据"永不保留或上传"；路径/项目名/仓库名"本地记录、永不上传"（`project.queue.jsonl`、`session.queue.jsonl`），并提供 `TOKENTRACKER_DISABLE_GIT_ATTRIBUTION=1`。
- `docs/PRIVACY.md:100-111`：登录后发送"小时桶（10 个字段）" + 设备注册时的 machine id + OAuth 的 email/display name；明确"Never sent"包括 prompt、路径、会话记录、项目明细、provider 凭据。
- `docs/PRIVACY.md:56-62`：默认开启的网络请求清单（心跳、PostHog、provider 配额读取、TRAE CN 读取、GitHub star、更新检查、定价刷新），以及 3.2 节里"需用户显式触发"的清单（Devin 配额、云同步/榜单、计费、汇率、桌宠下载、IP 检查页、服务状态页、分享卡字体）。

### 8.2 逐项核对（源码侧）

| 声明 | 源码证据 | 结论 |
| --- | --- | --- |
| 心跳只发 4 个字段 | `src/lib/telemetry.js:75-82`（`machine_hash`、`app_version`、`platform`、`shell`）；`MACHINE_HASH_NAMESPACE` 版本化、无法反推 device 行（`:34`） | **一致** |
| 心跳可关 | `TOKENTRACKER_NO_TELEMETRY` / `DO_NOT_TRACK` / `config.telemetry=false` 三选一即关（`src/lib/telemetry.js:48-53`）；前端问不到偏好就关闭埋点（`dashboard/src/lib/analytics.js:13`、`:67`） | **一致（fail-closed）** |
| 上传只含小时桶 | 客户端构造 `{upload_id, hourly, account_session_states?}`（`src/commands/sync.js:4367-4374`）；`session.queue.jsonl`/`project.queue.jsonl` 不在上传路径内（`src/commands/sync.js:3656` 注释"Only the main queue is uploaded"） | **一致** |
| 上传字段清单 | ingest 映射见 §6.1；**多出 `billable_total_tokens` 与 `total_cost_usd`** | **文档未逐字覆盖（实现更宽）** |
| 不发送会话记录 | 但 `account_session_states` 会上传 `session_id`（仅 `trae-cn`，`dashboard/edge-patches/tokentracker-ingest.ts:110-161`、`src/lib/upload-queue.js:9-18`） | **文档未列该项（局部不覆盖）** |
| 会话浏览器的本地性 | `/functions/tokentracker-sessions` 注释写明"LOCAL ONLY … never proxied to the cloud account view"（`src/lib/local-api.js:2495-2499`），并回 `Cache-Control: no-store`（`:2509`） | **一致** |
| 会话导出剥离身份 | `summarizeSessions` 默认删除 `project_ref`/`session_id`/`title` 等（`src/lib/session-analytics.js:2526-2543`），`provenance.privacy = "metadata-only"`（`:2561`） | **一致** |
| prompt 不进任何 payload | 解析侧只在内存里提取 prompt 用于指纹与标题（`src/lib/session-analytics.js:410-436`、`:445`），`summarizeSessions` 把 `title` 一并剥离；Codex 降级路径显式声明"message text is never returned"（`src/lib/local-api.js:376-379`） | **一致** |
| git 归因只用 subject | `docs/PRIVACY.md:21`；实现见 `src/lib/git-outcomes.js`（本文未逐行核对细节，仅确认存在独立模块与开关 `TOKENTRACKER_DISABLE_GIT_ATTRIBUTION`） | **基本一致**（细节待复核） |
| 项目/会话记录不上传 | 上传批只取 `hourly` 与控制记录（`src/commands/sync.js:4422-4442` 只收集 bucket 行与 `account_session_state`） | **一致** |

### 8.3 需要向用户披露的三处差异

1. **`billable_total_tokens` / `total_cost_usd` 随桶上传**（`dashboard/edge-patches/tokentracker-ingest.ts:188-189`）——文档的字段枚举未列。
2. **`account_session_states` 会带 `session_id`（仅 trae-cn 且要求本地已登录该应用）**（同上 `:110-161`）——文档"Never sent: Local session records"一句与实现存在字面张力；但字段里没有 prompt/对话内容。
3. **设备注册会上传 `machineId` + 主机名作为 `device_name`**（`src/lib/local-api.js:3137-3143`、`src/lib/local-api.js:31-41`）；文档写了 machine id，未写"主机名会作为设备显示名"，而云端设备页与榜单会出现该名称（`dashboard/src/lib/device-label.js`、`dashboard/src/lib/api.ts:1054-1063`）。

---

## 9. 测试佐证（锁定契约的用例）

本节表格中的"（N 行）"一律为**总行数**（`[System.IO.File]::ReadAllLines(...,UTF8).Length`，与 `read` 工具一致）；先前版本混用了 `Measure-Object -Line`（非空行数）口径，已按 R1 复核结果整体刷新（两者相差 9–157 行，见各条目）；引用行号同以 `read` 口径为准。

| 测试文件 | 锁定的契约（来自文件头注释与断言） |
| --- | --- |
| `test/status-json-light.test.js`（96 行） | `status --json` 必须含 `version/generated_at/base_url/device_token_set/queue/hooks/providers/copilot/subscriptions`，`queue.pending_bytes`、`hooks.claude`、`hooks.openclaw_session_plugin_conversation_access` 都存在；`--light` 必须是 `+-+` 分隔的 ASCII 表且无 ANSI/emoji，含 `| Version`/`| Queue pending`/`| Hook · claude` 行；`--diagnostics` 保持 `{env, paths}` 形状且不得泄漏 summary 键；未知 flag 必须非零退出（`:26-89`） |
| `test/status.test.js`（725 行） | `status` 从 `upload.throttle.json` 读最近上传时间戳等行为（`:16` 起；覆盖平台 mock、SQLite 探测等） |
| `test/wrapped-aggregator.test.js`（133 行） | 空输入→空摘要；年份默认取"数据中最新年"；per-source/per-model 占比和 ≤1；峰值小时；最长连续天数；`--year` 过滤；billable 优先（文件头 `:1-11`） |
| `test/wrapped-aggregator-dedup.test.js`（41 行） | `queue.jsonl` 追加式重发必须按 `(source, model, hour_start)` 先去重再求和，否则会按同步次数重复计数（`:1-7`） |
| `test/wrapped-entry.test.js`（28 行） | `/wrapped` 入口已从 Dashboard 头部移除（防止回归加回旧入口） |
| `test/local-api-queue-cache.test.js`（250 行） | 本地 API 对同一队列的读取/聚合并发共享同一份不可变视图（缓存键含文件身份签名） |
| `test/local-api-source-scope.test.js`（339 行） | `scope`/`include_account_level` 的过滤语义与 `excluded_sources` 结构（`src/lib/source-metadata.js` 的对外契约） |
| `test/local-api-security.test.js`（1561 行） | 本地 API 的鉴权/来源校验/令牌绑定（`/api/local-auth`、`x-tokentracker-local-auth`、auth 代理来源限制、实例指纹切换） |
| `test/local-api-account-view.test.js`（1386 行） | `account=1` 代理的 `X-TokenTracker-Account-View` 头与降级路径、cookie relay、偏好持久化 |
| `test/local-achievements.test.js`（126 行） | `computeLocalAchievements` + `LOCAL_BADGE_THRESHOLDS` 的达成时间语义（按 `hour_start` 顺序回放，取首次跨阈值的桶） |
| `test/account-model-wire.test.js` / `account-summary-model-wire.test.js`（135 / 219 行） | 云端 `account-*` 的紧凑线格式解码：含 `__proto__`、空模型名、超大整数（`9007199254740993`）等恶意/边界载荷不得污染对象原型或丢精度；且 summary 的 SQL 不得回退到全表（`account-summary-model-wire.test.js:213`） |
| `test/account-source-parity.test.js`（107 行） | 账号级来源清单在 4 处硬编码（`src/lib/source-metadata.js`、account RPC SQL、profile edge、devices edge）必须一致，漂移即跨设备重复计数 bug 回归（文件头 `:3-18`） |
| `test/community-stats-cache.test.js`（102 行） | 社区模型统计必须读预计算表 `tokentracker_community_stats`，不得回退到 `leaderboard_usage_grouped` 全表扫描（`:16-20`） |
| `test/badge-svg-render.test.js`（147 行） | 徽章 SVG 的尺寸数学、数字格式化与转义；Deno 端为唯一真源，此测试是同步镜像（`:1-11`） |
| `test/share-card-data.test.js`（138 行） | `buildShareCardData` 的回落（`—`/0/null）、topModels 截断 3、非法 rank 归 null、`formatCost` 千分位边界（`:23-138`） |
| `test/usage-limits-singleflight.test.js`（165 行） | `getUsageLimits` 的进程内并发合并与缓存分区（同一 selection key 只抓一次） |

---

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
| R1-8 [I5] | §0-2「两个最忙一天」未给前提 | 补前提：本地 API 请求不带 tz 时回落 UTC；前端一直传 tz，故真实前端路径的分歧成立（**该前提已被 R2 再限定见文末「R2 修正」：回落 UTC 只属 `rowDayKey` 一族，`getZonedParts` 系回退本机时区**） | `src/lib/local-api.js:512-519`、`:213-225`、`dashboard/src/lib/api.ts:244-249` |

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

### R2 修正（第二轮复核后）

依据 `REVIEW-R2-3-aggregation-pricing-consumers.md`（E=2 / I=8 / U=6；样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，全程只读，未修改样本仓）。本轮 **采纳 3 条、反驳 1 条（E1 的"macOS 为触发面"部分）**，均独立打开源码复核后落实。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| E1（§0-2，连带 §7.2/§7.3/§7.4） | 「本地 API 请求不带 tz 时回落 UTC」（把两条路径写成一条） | 按路径分两级写清：`rowDayKey` 一族 → **UTC 日**（`:224`）；`getZonedParts` 系（hourly `:606`/`:608`、summary `todayStr` `:2382`、heatmap `todayStr` `:2538`、`night_owl` `:482`）→ 三级回退的第三级是**运行环境本机时区**（`:585-592`）。后果：不带 tz 时 `hourly` 的 `day` 默认值（**UTC 今天** `:3147`）与其过滤谓词（本机时区日键）不同基准；`todayStr` 一系窗口终点同理。**§0-2 的"两个最忙一天"结论不变**（真实前端始终带 tz） | `local-api.js:213-225`、`:224`、`:482`、`:585-592`、`:606`、`:608`、`:2382`、`:2538`、`:3147` |
| E1-反驳（R2-3 的暴露面判定） | R2-3 判定「macOS 壳 heatmap 不传 tz（`APIClient.swift:73-75`），是实际触发面」；分片 §7.2 表也照此只写 `?weeks=52` | **不成立**：`:73-75` 只是 `queryItems` 字面量，整数组传给 `withAccountQueryItems`，而它是 `withTimeZoneQueryItems` + `account=1` → **必带 `tz` 与 `tz_offset_minutes`**。三端壳与前端都带 tz（macOS `:241-254`；Linux `extension.js:997`；Windows `UsagePoller.cs:321`），故触发面只剩不带 tz 的调用者（裸 HTTP/脚本/测试）。§7.2 表已补 helper 说明 | `APIClient.swift:73-77`、`:101-105`、`:241-254`；`extension.js:997`；`UsagePoller.cs:321` |
| E2（§2.3 参数表） | `scope` / `include_account_level` 取值列以残留字符「`3000`：」开头 | 删除 `3000`，该列改为「默认 `all`；`personal|local` 排除账号级来源；`include_account_level=1` 强制 `all`」（与 `local-api.js:398-401`、`source-metadata.js:15-26` 一致） | `local-api.js:398-401`；`src/lib/source-metadata.js:15-26` |
| I5（§5.1） | 「选择键由 `{devinEnabled, ...}` 生成」 | 改为二元组 `${devin}-${antigravity}`（`usageLimitsSelectionKey`，`:59-63`）：`devin` 由 `devinEnabled === true` 决定，`antigravity` 由 `isAntigravityQuotaDisabled()` **取反**决定；分区枚举 `off-on`/`off-off`/`on-on`/`on-off`（`:56-58`）；single-flight 在 `getUsageLimits` `:3954-3970` | `usage-limits.js:56-63`、`:3954-3970` |
| I8（§2.4.6） | 只照抄响应字段 `pricing.source: "litellm"`，未说明它是标签 | 补限定：该字段是**硬编码标签**（`local-api.js:2658`），命中 `curated:*` 时并不代表真实来源；要真实来源看每行 `cost_source`（`:2624-2626`） | `local-api.js:2658`、`:2624-2626`；对照 `vite.config.js:1067` |
| U1（解除） | 「macOS `fetchHourly(day:)`、Linux `-hourly?day=` 的 `day` 取值来源未逐行核」 | **解除**：macOS `day` = `DateHelpers.dayRange`（`.current` 时区的 `yyyy-MM-dd`，`DateHelpers.swift:44-50`），调用点 `DashboardViewModel.swift:192-194`/`:311`，且请求带 tz（`:241-254`）；Linux `day` = `dayString(new Date())`（**本机时区**当日，`extension.js:1044`、`:273-275`），请求同样带 tz（`:997`）。两者均"当地日 + 当地 tz"，自洽 | 同上 |

保留未动：U2（`LimitsPage.jsx` / `usage-limits-provider-specs.js`）、U3（Windows 托盘 streak 实测）、U4（cloud-session 等入参与返回）、U5（`git-outcomes.js` revert 细节）、U6（限额磁盘缓存落盘目录，S5 已部分结案：`<home>/.tokentracker/tracker/`）——判定条件照旧；§10 第 1 条仍保留 `【未能确认】`。
