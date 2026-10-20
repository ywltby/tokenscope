## 9. 消费端：CLI、本地 API、Dashboard、原生壳、云

样本仓库 `xiufengsun/TokenTracker`，本地检出 `C:\Users\admin\AppData\Local\Temp\tt-research`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`（v1.3.14）。本章全程只读，未改样本仓文件、未执行其脚本。

范围：统计结果从「算出来」到「呈现给人」的全部通路——CLI 输出面、本地 HTTP API、Dashboard 前端消费与二次聚合、导出/分享衍生面、云上传与云端再聚合、三端原生壳取数，以及与用量统计相邻但不属于它的边角面（限额、成就、宠物、技能）。不覆盖各 agent 日志解析内部与定价算法推导。

引用约定：`相对路径:行号`（相对样本仓根），行号取自本次实际读取；未亲自确认者标 `【未能确认】` 并在 9.7 汇总判定条件。有一个贯穿本章的结构性事实先说：**同一份 `queue.jsonl` 在仓内至少存在四份独立聚合实现**——本地 API（`src/lib/local-api.js`）、年度聚合器（`src/lib/wrapped-aggregator.js`）、Vite dev 插件（`dashboard/vite.config.js`）、云端 `account-*` 系列（`dashboard/edge-patches/`）。本章的分歧点几乎都源于这四份实现之间没有共享代码约束。

### 9.1 CLI 的对外输出面

#### 9.1.1 命令清单

入口 `src/cli.js`，**无参数时直接进 `serve`**（起本地服务 + Dashboard），所以「打开界面」是 CLI 的默认行为：`if (!command) { await cmdServe(argv); return; }`（`src/cli.js:43-47`）。命令分派见 `src/cli.js:32-65`：

| 命令 | 实现文件 | 输出形态 |
| --- | --- | --- |
| `serve`（默认） | `src/commands/serve.js` | 起本地 HTTP + 静态 Dashboard，打印 URL 与数据路径 |
| `init` | `src/commands/init.js`（2427 行） | 安装引导（不在本章范围） |
| `sync` | `src/commands/sync.js`（7251 行） | 进度与汇总计数 |
| `status` | `src/commands/status.js`（1729 行） | 人读列表 / `--json` / `--light` ASCII 表 |
| `diagnostics` | `src/commands/diagnostics.js` | 纯 JSON（`{ok, version, env, paths, ...}`） |
| `doctor` / `uninstall` / `device-login` | 各自同名文件 | 人读（`doctor` 支持 `--json` / `--out`） |
| `wrapped` | `src/commands/wrapped.js` | ASCII 年框 / `--json` |
| `sessions` | `src/commands/sessions.js` | JSON / CSV，可 `--out` |

`-v/--version`、`-h/--help` 在分派前处理（`src/cli.js:21-30`）。关键事实：**CLI 里没有「用量报表」命令**——`status` 报环境与上传健康度，`wrapped` 是唯一直接计算 token 用量的命令，`sessions` 导出会话级元数据。

#### 9.1.2 `status`：环境与上传健康度，不解析队列内容

`status` 读取四个状态文件：`tracker/queue.jsonl`、`queue.state.json`、`cursors.json`、`upload.throttle.json`（`src/commands/status.js:200-206`）。**它只用 `safeStatSize` 拿队列字节数，减去 `queue.state.json` 的上传偏移，得到「待上传字节」，从不解析队列行内容**（`src/commands/status.js:278-288`：`const pendingBytes = Math.max(0, queueSize - (queueState.offset || 0));`）。本行独立复核：全文件检索 `total_tokens|total_cost|"cost"` 为 **0 命中**。时间线来自 `upload.throttle.json` 的 `lastSuccessMs` / `nextAllowedAtMs` / `backoffUntilMs`（`:341-347`），`last_parse` 取 `cursors.json` 的 `updatedAt`（`:1050`）。

`status --json` 顶层字段（`src/commands/status.js:1040-1259`）：

```js
// src/commands/status.js:1040-1059（节选）
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
      last_upload: lastUpload || null,
      next_upload_after: nextUpload || null,
      backoff_until: backoffUntil || null,
      last_upload_error: lastUploadError || null,
```

其余顶层键：`extra_scan_roots[]`（`{provider, origin, path, exists, error}`）、`hooks{}`（键名逐字：`codex_notify`、`acode_notify`、`every_code_notify`、`claude`、`gemini`、`opencode_plugin`、`openclaw_session_plugin`、`openclaw_session_plugin_conversation_access`、`openclaw_legacy`、`codebuddy`、`workbuddy`、`workbuddy-ai`、`grok`）、`providers{}`（约 40 家，每家 `{installed, files?, detail?, error?}`，`:1081-1238`）、`copilot{}`（`:1239-1246`）、`passive_mode{}`（`:1247-1250`）、Windows 专有 `wsl_mode`/`wsl_mode_invalid`/`wsl_distros[]`（`:1251-1257`）、`subscriptions`（`:1258`）。

三种渲染（`src/commands/status.js:1261-1266`）：`--json` 输出 `JSON.stringify(summary, null, 2)`；`--light` 输出固定宽度 ASCII 表，注释自述定位为「for AI agents and CI: deterministic columns, no emoji or spinner side effects, easy to grep」（`:1590-1593`）；无 flag 输出人读列表（首行 `TokenTracker v...`，含 `- Queue: N bytes pending`，`test/status-json-light.test.js:77-83` 锁定）。`--diagnostics` 走完全不同的形状（`{env, paths}`），测试要求 summary 的键不得泄漏进来（`test/status-json-light.test.js:68-75`）。参数仅 `--json`、`--light`、`--diagnostics`、`--probe-keychain`、`--probe-keychain-details`，未知 flag 直接抛错（`:1576-1584`）。

#### 9.1.3 `wrapped`：唯一直接算用量的 CLI

`cmdWrapped` **自己读 `queue.jsonl`，不经过本地 API**（`src/commands/wrapped.js:122-128`，`const rows = await readQueueRows(queuePath);`）。坏行容错是「静默跳过」（逐行 `JSON.parse` 失败即忽略，`:14-22`，注释说明生产队列的「半行」会在下次读取时自愈）。参数只有 `--json` 与 `--year`（年份校验 2000–2100，`:26-41`）。

聚合口径（`src/lib/wrapped-aggregator.js`）：

| 维度 | 规则 | 位置 |
| --- | --- | --- |
| 去重 | 按 `source\|model\|hour_start` 建 Map，后写胜出，并逐行应用 `normalizeQueueRow()` 旧行修正 | `:77-86` |
| 年份选择 | 未指定 `--year` 时取「数据中最新的一年」，不是当前年 | `:91-99` |
| 切天/切年/切小时 | `hour_start.slice(0,10)` 切天、前 4 位切年、正则 `/T(\d{2}):/` 取小时——**全 UTC** | `:37-53`；`:66-67` JSDoc 明写 "calendar year (UTC)" |
| 连续天数 | `new Date(iso + "T00:00:00Z")` 按日步进（UTC） | `:148-152` |
| token 口径 | `billable_total_tokens` 优先，回落 `total_tokens` | `:29-35` |
| 成本 | **无**——全程不触碰 `total_cost_usd`，不 require 定价模块 | `:11-15` 模块头注释 |

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

`top.sources`/`top.models` 各取 5 并附 `share` 占比，`top.days` 取 5（`:177-187`）；`highlights` 是四条硬编码英文句式（顶层模型占比、最忙一天 token、峰值小时、连续天数）+ 一条「工具数 ≥ 4」（`:189-207`）。ASCII 渲染（`src/commands/wrapped.js:43-120`）为 60 宽 `╔═╗` 框；`Total tokens` 走 `formatCompact`（T/B/M/K，`src/lib/wrapped-aggregator.js:226-234`）；**`Active days` 硬写 `/ 365`**（`:67`）；`Peak hour` 显示 `NN:00 UTC`（`:70-77`）——时区基准在输出上被显式标注。

#### 9.1.4 `sessions`：会话级元数据导出

`cmdSessions` 是**独立于 `wrapped` 的第二条统计链**（`session.queue` 侧车 + 直接扫会话文件，主体在 `src/lib/session-analytics.js`）：`buildSessionAnalytics({force})` → 可选 `buildGitOutcomes` → `summarizeSessions({from,to})`（`src/commands/sessions.js:12-27`）。CSV 列定义在 `src/lib/session-analytics.js:2909`：`session_hash, source, project_key, model, model_usage_json, started_at, ended_at, duration_ms, turns, edit_turns, retry_turns, subagent_calls, total_tokens, cost_usd, productive, first_pass, one_shot`。写文件时 `mode: 0o600`（`src/commands/sessions.js:22`）。

`summarizeSessions()` 返回 `available`、`sessions[]`（默认剥离 `project_ref`/`session_id`/`parent_session_id`/`title`，`src/lib/session-analytics.js:2526-2543`）、`session_count`、`summary{}`（含 `productive_rate`、`one_shot_rate`、`edit_session_rate`、`tokens_per_edit`、`cost_per_edit`，`:2545-2555`）、`by_model[]`、`subagents[]`、`provenance{source:"local-session-log", confidence:"observed", privacy:"metadata-only", methodology:"edit-turn-v2", ...}`（`:2558-2565`）。**源码自带口径警告**：`by_model[].sessions` 是「触碰过该模型的会话数」，**不可求和**为总会话数；只有 token/成本列可求和（`:2421-2427`）。

#### 9.1.5 时区口径冲突：`wrapped` 与 Dashboard 的两套切天基准

这一条必须**按路径分两级**描述，不能简化为「不带 tz 就回 UTC」：

| 路径 | 未显式传时区时的基准 | 证据 |
| --- | --- | --- |
| `wrapped`（CLI 与 `/functions/tokentracker-wrapped` 共用聚合器） | **UTC** 切年/切天/切小时/算连续天数，无回退分支 | `src/lib/wrapped-aggregator.js:37-53`、`:148-152`、`:170` |
| 本地 API 日聚合一族（`rowDayKey`：daily 与 summary 的日聚合） | 带 `tz`/`tz_offset_minutes` 时按该时区切天；**未带则回落 UTC 日** | `src/lib/local-api.js:213-225`（`:224` 即 `hs.slice(0,10)`） |
| 本地 API `getZonedParts` 系：hourly 的日过滤与小时键、summary 的 `todayStr`、heatmap 的 `todayStr`、`night_owl` 徽章 | 三级回退的**第三级是运行环境本机时区**（这几处无 tz 守卫） | `src/lib/local-api.js:585-592`、`:606`、`:608`、`:2382`、`:2538`、`:482` |

由此得两条强度不同的结论：**对真实产品路径**（前端与三端壳一律显式传 tz：`dashboard/src/lib/api.ts:244-249`、`TokenTrackerBar/.../APIClient.swift:241-254`、`TokenTrackerWin/UsagePoller.cs:321`、`TokenTrackerLinux/.../extension.js:997`），非 UTC 时区下同一用户同一年会看到两个不同的「活跃天数」与「最忙一天」——Wrapped 按 UTC、Dashboard 按所选时区，**该冲突在真实前端路径上成立**；**对裸调 API 不传 tz 的调用者**，与 `wrapped` 的 UTC 基准真正分歧的是 `getZonedParts` 一系（本机时区），其中 `hourly` 的 `day` 默认值又是 UTC 今天（`:3147`），与它自己的过滤谓词不同基准。

### 9.2 本地 API 全清单

#### 9.2.1 服务骨架与响应约定

`serve` 的 HTTP 服务按前缀三分派：`/functions/`、`/api/`、`/proxy/` 交给 `handleApi`，其余走静态资源与 SPA fallback（`src/commands/serve.js:171-189`）。

| 项 | 取值 | 证据 |
| --- | --- | --- |
| 绑定 | 仅 `127.0.0.1` | `src/commands/serve.js:34` |
| 端口 | 默认 7680；WSL 下 7681；非显式端口时最多顺延 20 个 | `src/commands/serve.js:14-22`、`:202-215` |
| CORS | 任何 `OPTIONS`（`/api/auth/` 前缀除外）回 204 并放行 `GET, POST, OPTIONS` | `src/commands/serve.js:162-169` |
| 队列路径 | `~/.tokentracker/tracker/queue.jsonl`（`resolveQueuePath()`） | `src/lib/local-api.js:83-86` |
| 静态资源 | `dashboard/dist`，两层候选路径 | `src/commands/serve.js:716-725` |
| 后台任务 | Windows 专有每分钟 `sync --auto --background --all-local-sources`；匿名心跳启动一次 + 每 6 小时复查 | `src/commands/serve.js:294-366`（`:303-309` 条件）、`:266-273` |
| 响应头 | 所有 JSON 统一 `Cache-Control: no-store` | `src/lib/local-api.js:914-927` |

鉴权三层：**① 本地令牌** `crypto.randomBytes(24).toString("hex")`（`src/lib/local-api.js:1016`），经 `GET /api/local-auth` 暴露（`:1681-1695`），`serve.js` 用同一端点做重复实例与健康判定（`src/commands/serve.js:508-534`）；**② 写操作**校验头 `x-tokentracker-local-auth`（`isAuthorizedLocalMutation`，`src/lib/local-api.js:1629-1637`）；**③ `/api/auth/*` 与 `GET /api/auth-bridge/verifier`** 额外要求同源/回环来源，否则 403 `untrusted_auth_origin`（`:1643-1648`）。前端取令牌见 `dashboard/src/lib/local-api-auth.ts`（`getLocalApiAuthToken()` / `getLocalApiAuthHeaders()`，进程内缓存）。

#### 9.2.2 通用查询参数（用量类端点）

| 参数 | 语义 | 证据 |
| --- | --- | --- |
| `from` / `to` | `YYYY-MM-DD`，按**日键闭区间**过滤（`day >= from && day <= to`） | `src/lib/local-api.js:2359-2364`、`:130-134` |
| `tz` / `tz_offset_minutes` | IANA 名与分钟偏移（fallback） | `getTimeZoneContext` `:512` 起；键生成 `:115-117` |
| `scope` / `include_account_level` | 默认 `all`；`personal\|local` 排除账号级来源；`include_account_level=1` 强制 `all` | `:398-401`、`src/lib/source-metadata.js:15-26` |
| `source` / `model` / `device_id` | 前端拼进查询串 | `dashboard/src/lib/api.ts:254-263` |
| `account=1` | 触发账号（跨设备）视图代理 | `src/lib/local-api.js:2347-2355` |
| `weeks` / `day` / `limit` | 热力图周数默认 52；hourly 的日（默认 UTC 今天）；项目汇总默认 10（`PROJECT_USAGE_MAX_ENTRIES`）、会话浏览器上限 2000 | `:2534`、`:3147`、`:95`、`:2504-2505` |

账号级来源白名单**只有两家**：`cursor` 与 `trae-cn`（`src/lib/source-metadata.js:1`），其用量在账号视图里是「跨设备去重而非相加」，一致性由 `test/account-source-parity.test.js:3-18` 强制（4 处硬编码必须同步，漂移即重复计数 bug 回归）。

#### 9.2.3 `/functions/tokentracker-*` 端点总表（27 个）

路由字面量清点所得，**共 27 个**（独立复核确认）。「定义行」为 `if (p === ...)` 所在行。

| # | 路径 | 方法 | 主要参数 | 返回顶层字段（摘要） | 用途 | 定义行 |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | `-usage-summary` | GET | `from`,`to`,`tz*`,`scope`,`account` | `from,to,days,scope,excluded_sources,totals{},rolling{}` | 概览 | `:2358` |
| 2 | `-usage-daily` | GET | `from`,`to`,`tz*` | `from,to,scope,excluded_sources,data[]` | 逐日 | `:2425` |
| 3 | `-usage-hourly` | GET | `day` | `day,scope,excluded_sources,data[]` | 逐小时 | `:3146` |
| 4 | `-usage-monthly` | GET | `from`,`to` | `data[]` | 逐月 | `:3156` |
| 5 | `-usage-heatmap` | GET | `weeks` | `from,to,scope,excluded_sources,week_starts_on,active_days,streak_days,weeks[][],total_cost_usd` | 热力图 | `:2533` |
| 6 | `-usage-model-breakdown` | GET | `from`,`to` | `from,to,days,scope,excluded_sources,sources[],pricing{}` | 模型维度 | `:2596` |
| 7 | `-usage-category-breakdown` | GET | `from`,`to`,`source` | 类别分解（可带降级标注） | 语义类别 | `:2668` |
| 8 | `-wrapped` | GET | `year` | `scope,excluded_sources,` + 年度结构 | 年度报告 | `:2330` |
| 9 | `-outcomes` | GET | `from`,`to` | `available,by_model,by_tool,totals` | 产出质量 | `:2441` |
| 10 | `-context-health` | GET | — | `computeContextHealth({home,cwd,env})` 原样 | 上下文健康 | `:2526` |
| 11 | `-session-insights` | GET | `from`,`to`,`refresh`,`include_sessions`,`format=csv` | `from,to,` + 会话摘要 | 会话效率 | `:2470` |
| 12 | `-sessions` | GET | `from`,`to`,`refresh`,`limit` | `from,to,` + 会话浏览结果 | 会话浏览器（本地专用） | `:2500` |
| 13 | `-project-usage-summary` | GET | `from`,`to`,`tz*`,`limit` | `generated_at,entries[]` | 项目汇总 | `:2731` |
| 14 | `-project-usage-detail` | GET | `project_key`（必填，缺失回 400 `missing_project_key`） | `generated_at,from,to,project_key,project_ref,totals{},days_active,range_total_tokens,daily[],sources[]` | 项目详情 | `:2828` |
| 15 | `-usage-limits` | GET | `refresh`,`devin` | 各 provider 键对象 + `fetched_at` | 限额窗口 | `:3482` |
| 16 | `-subscription-manager` | GET/POST | POST 动作 `create\|update\|delete` | `subscriptions[]` | 手填订阅（续费/到期日） | `:3240` |
| 17 | `-achievements` | GET | `tz*` | `generated_at,achievements[]` | 徽章 | `:3470` |
| 18 | `-skills` | GET/POST | GET `mode=installed\|repos\|discover\|search\|popular\|updates\|activity\|skill_usage`；POST 多动作 | 随 mode 变化 | 技能 | `:3294` |
| 19 | `-pets` | GET/POST | GET `scope=codex`；POST `install_url\|remove\|import_codex` | `pets[],hiddenBuiltinIds[]` 或 `importable[],codexDetected` | 桌宠 | `:3192` |
| 20 | `-user-status` | GET | — | `user_id,email,name,is_public,pro{},account{}` | 本地 stub 用户态 | `:2920` |
| 21 | `-cloud-session` | POST | `{clear?,session_id?,expectedOwnerId,insforgeBaseUrl?}` | `session_id,device_id,created_at` | 云会话 | `:2137` |
| 22 | `-local-sync` | POST | 多个布尔开关（`auto`/`background`/`lightweight`/`drain`/`publishAccount`/`allLocalSources`/…） | 成功 `ok,uploaded,pending,next_allowed_at?`；失败 `ok:false,error,code,recovery_url,next_allowed_at,retry_after_seconds,stdout,stderr` | 触发同步 | `:2166` |
| 23 | `-cloud-sync-pref` | GET/POST | POST `{enabled}` | GET `enabled,account_available`；POST `ok,enabled` | 云同步开关 | `:2940` |
| 24 | `-machine-id` | GET | — | `machineId,deviceName`（deviceName = 主机名，截断 128） | 设备标识 | `:3137`、`:31-41` |
| 25 | `-telemetry-pref` | GET | — | `disabled` | 心跳偏好 | `:3121` |
| 26 | `-proxy-config` | GET/POST | POST 经 `parseProxyPayload` | `mode,protocol,host,port,effective,applyError` | 代理配置 | `:2982` |
| 27 | `-proxy-test` | POST | `{mode:"manual",...}` | `runProxyConnectivityTest(...)` 结果 | 代理测试 | `:3074` |

（表中路径省略统一前缀 `/functions/tokentracker`。）21–27 与 15–19 的 POST 均需本地鉴权头。

#### 9.2.4 `/api/*` 与 `/proxy/*`

| 端点 | 方法 | 参数 / body | 返回 | 定义行 |
| --- | --- | --- | --- | --- |
| `/api/runtime-config.js` | GET | — | `window.__TOKENTRACKER_RUNTIME_CONFIG__={baseUrl,anonKey,dashboardUrl,configurationError}`（`application/javascript`，转义 `<` 与 U+2028/9） | `:1670-1679` |
| `/api/local-auth` | GET | — | `{token, serverVersion?}` | `:1681-1695` |
| `/api/pets/local/:id/spritesheet.webp`、`/api/pets/codex/:id/spritesheet.webp` | GET/HEAD | — | `image/webp` + `no-cache` + `nosniff` | `:1698`、`:1724` |
| `/api/pets/import` | POST | body 上限 `pets.MAX_PACKAGE_BYTES` | 导入结果 | `:1747` |
| `/api/auth-bridge/verifier` | GET/PUT/POST | body `{native:boolean}` | `{native}`（一次性读取，TTL 5 分钟） | `:1767-1788` |
| `/api/auth/*` | 任意 | 透传到 InsForge；`refresh\|logout` 有 csrf/relay 特例 | 上游响应原样 | `:1792` 起 |
| `/api/avatar-proxy` | GET | `?url=` | 图片字节（内存 LRU：1h TTL、512 KiB/条、64 条） | `:2038`、`:43-48` |
| `/api/geoip/`、`/api/geoip-batch`、`/api/iprisk/`、`/api/dns/result/` | 任意 | 代理到 `ip.net.coffee` 对应接口 | 上游响应 | `:1973-1976` |
| `/proxy/ipcheck` | 任意 | 透传到 `https://ip.net.coffee`，剥离 hop-by-hop 头 | 上游响应 | `:939-1001` |

#### 9.2.5 聚合链：谁共用主链，谁各自遍历

**共用主链**：`-usage-summary`、`-usage-daily`、`-usage-hourly`、`-usage-monthly`、`-usage-heatmap`、`-usage-model-breakdown` 六个端点都建立在「读 `queue.jsonl` → 逐行去重与旧行修正 → 按时区切天聚合」上（`readQueueData` 的 `parseRow` 完成去重与修正，`src/lib/local-api.js:170-211`），差别只在粒度与投影字段。注意 daily 的桶结构里 `models` 映射按 **`total_tokens`** 聚合：`a.models[model] = (a.models[model] || 0) + (row.total_tokens || 0);`（`:297`）——而 `wrapped` 的份额按 billable 算（`src/lib/wrapped-aggregator.js:182-186`），两处「模型占比」分母不同源。

| 端点族 | 数据来源 | 关系 |
| --- | --- | --- |
| `-project-usage-summary` / `-detail` | 派生队列 `project.queue.jsonl`（`:135`），剔除 claude-mem 伪项目（`:136-138`） | 独立文件、独立聚合；全空时降级为「按 source 聚合」的伪条目且 `project_ref` 留空（`:2803-2818`） |
| `-session-insights` / `-sessions` | `buildSessionAnalytics`（会话文件 + `session.queue` 侧车） | 完全独立的第二条链 |
| `-outcomes` | `outcomes.jsonl` 侧车 + 会话分析 + `computeQualityPerDollar`；未开启时回 `{available:false, by_model:[], by_tool:[], totals:null}`（`:2460`） | 独立；注释强调不改 queue schema、不碰同步链（`:2435-2440`） |
| `-usage-limits` | provider 官方 API + 本地磁盘缓存 | **与队列零关系**（见 9.7.1） |
| `-wrapped` | `aggregateWrapped(rows, {year})`（`:2333-2336`） | 复用主链的**行**，但用**独立的 UTC 聚合器** |
| `-achievements` / `-skills` / `-pets` | 各自模块（徽章回放主链行；`skill_usage` 另有计价段） | 部分复用主链行 |

#### 9.2.6 `account=1` 包装层与失败标记

账号（跨设备）视图**不是新端点，而是前置包装层**：

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

映射覆盖六个 slug（`src/lib/cloud-account.js:20-27`）：`usage-summary→account-summary`、`-daily`、`-hourly`、`-monthly`、`-heatmap`、`-model-breakdown`。`project-usage`、`usage-limits`、`category-breakdown` 被源码注释明确排除（「remain local-only and are intentionally absent here」，`:17-19`）。成功时打头 `X-TokenTracker-Account-View: 1`（`src/lib/local-api.js:1565`）；失败打 `: 0` 并附加原因串，**继续走本地 handler**——调用者拿到本机单机数据 + 一个「这不是跨设备数据」的标注。失败冷却 2 分钟（`:27`）。

#### 9.2.7 若干返回字段的准确语义（易踩）

| 字段 | 准确语义 | 证据 |
| --- | --- | --- |
| `usage-summary.rolling.last_7d/30d.active_days` | 只统计「确有日汇总的活跃日」（跳过空日），**不等于 7/30** | `src/lib/local-api.js:2390-2404` |
| `usage-model-breakdown.pricing.source` | **硬编码标签 `"litellm"`**，不代表本行实际命中来源（命中 `curated:*` 时仍写 litellm）；真实来源看每行 `cost_source`（`provider_reported`/`model_pricing`/`mixed`） | `:2658`；对照 `:2624-2626`；dev mock 同类字段硬编码 `"default"`（`dashboard/vite.config.js:1067`） |
| 成本求和顺序 | **必须逐行计价后再求和**；源码解释「先加列再计价」会吞掉长上下文/加急档位（#653 的修法） | `src/lib/local-api.js:2634-2640` |
| `project-usage-summary.entries[].total_tokens` | **字符串**（大整数过 JSON 的显式处理）；排序按 `billable_total_tokens` 降序 | `:2787-2794` |
| `usage-heatmap.streak_days` | 本地端点**硬编码 0**（见 9.3.3） | `:2588` |
| `category-breakdown` 降级 | `codex` 分支总量为 0 时改用队列行降级口径，返回带 `breakdown_status:"queue_fallback"`、`fallback:"queue_totals"`，并声明 `includes_content=false`/`includes_inputs=false` | `:2692-2698`、`:329-395`（`:376-379`、`:386-389`） |
| `-sessions` 的本地性 | 注释写明 LOCAL ONLY 且 never proxied to the cloud account view；出错时把 fs 错误挡在响应体外（避免绝对路径泄漏），只回 `{available:false, error:"Session browser failed"}` | `:2495-2499`、`:2509`、`:2516-2521` |

### 9.3 Dashboard 如何消费

#### 9.3.1 唯一 API 层与本地/云端双路由

`dashboard/src/lib/api.ts` 集中端点表（`:141-158`）与「本地 usage-* → 云端 account-*」映射（`:166-173`）；注释说明云端 `account-*` 的 schema **逐字段镜像**本地 `tokentracker-usage-*`，「so the dashboard components render identically off either source」。

| 行为 | 规则 | 证据 |
| --- | --- | --- |
| 本地路径 | `new URL(\`/functions/${slug}\`, window.location.origin)`，`cache: "no-store"` | `:222-234` |
| 云端路径触发条件 | 非 localhost + 有 accessToken + 有 accountSlug 映射 | `:191-199` |
| 非 localhost 且无 token | 抛 `LOCAL_API_UNAVAILABLE`（404） | `:216-221` |
| 账号响应缓存 | summary 120 s、其余 300 s、`stale-if-error` 5 分钟、上限 64 条；键 `(origin, path+排序后 query, token sub/anon key)` | `:29-31`、`:75-129` |
| in-flight 合并 | `coalesceJsonGet()`，键为完整 URL | `:62-73` |
| 热力图传输 | 账号路径带 `format=compact`，响应过 `expandHeatmapCompact` | `:199`、`:212` |

#### 9.3.2 页面 → 端点映射

| 页面 / 组件 | 消费的端点 | 证据 |
| --- | --- | --- |
| `DashboardPage.jsx` | usage-summary / daily / hourly / monthly / model-breakdown / heatmap / project-usage-* / sessions / category-breakdown | `dashboard/src/pages/DashboardPage.jsx`（示例 `:565`、`:602`、`:819`）；端点常量 `dashboard/src/lib/api.ts:141-158` |
| `SessionsPage.jsx` | `-sessions`、`-session-insights` | `dashboard/src/lib/api.ts:681`、`:157` |
| `WrappedPage.jsx` | `-wrapped?year=` | `dashboard/src/pages/WrappedPage.jsx:43-45` |
| `LimitsPage.jsx` | usage-limits（+ subscription-manager） | `dashboard/src/pages/LimitsPage.jsx:49-66`、`:123-133` |
| `AchievementsPage.jsx` | achievements | `dashboard/src/lib/api.ts:151`、`:328-334` |
| `LeaderboardPage.jsx` / `LeaderboardProfilePage.jsx` | 云端 leaderboard / leaderboard-profile / profile-likes | `dashboard/src/lib/api.ts:383-583` |
| `DevicePage.jsx` | account-devices（云）/ device-rename | `dashboard/src/lib/api.ts:840-860`、`:1040-1063` |
| `CloudPage.jsx` / `CloudCheckoutPage.jsx` | billing / cloud-session / checkout | `dashboard/src/lib/cloud-checkout.js`、`dashboard/src/lib/api.ts:157` |
| `PetPage.jsx` / Linux 桌宠宿主 | pets、usage-summary、heatmap、model-breakdown、usage-limits | `dashboard/src/lib/pets-api.js:21`、`:39`、`:66`；`dashboard/src/lib/pet-linux-host.js:194`、`:211`、`:223`、`:237` |
| `SkillsPage.jsx` / `SkillDetailPanel.jsx` | skills（多 mode） | `dashboard/src/lib/api.ts:151`；端点 `src/lib/local-api.js:3294` |
| 设置页账号区 | user-status / cloud-sync-pref / telemetry-pref / proxy-config | `dashboard/src/hooks/use-proxy-settings.js:4-5`、`dashboard/src/lib/cloud-sync-prefs.ts:98`、`:157`、`dashboard/src/lib/analytics.js:67` |

桌宠那一路值得单列：`pet-linux-host.js` 是**独立于 React 页面的取数宿主**，自己拼 `?weeks=52&tz=...&account=1` 并读 `data.streak_days`、`sources[].models[]`、`totals`（`dashboard/src/lib/pet-linux-host.js:194-237`）。

#### 9.3.3 后端优先 + 前端兜底：`activeDays` 与 `streak_days`

`activeDays` 的策略是「后端优先、缺失才自算」：

```js
// dashboard/src/pages/DashboardPage.jsx:819-850（节选）
  const activeDays = useMemo(() => {
    if (!signedIn && !mockEnabled && !publicMode && !isLocalMode) return 0;
    const serverActive = Number(heatmap?.active_days);
    if (Number.isFinite(serverActive)) return serverActive;

    let count = 0;
    const seen = new Set();
    const considerDay = (day, value, level) => {
      if (typeof day !== "string" || !day) return;
      if (seen.has(day)) return;
      if (!hasUsageValue(value, level)) return;
      seen.add(day);
      count += 1;
    };
```

拿不到 `heatmap.active_days` 时才遍历 `heatmapDaily` 与 `heatmap.weeks` 自行点数（`:835-847`）。

`streak_days` 更尖锐——**服务端在本地端点里恒为 0，前端自算并覆盖**：`src/lib/local-api.js:2587-2588` 同一响应里 `active_days: activeDays.length`（真实计算）与 `streak_days: 0,`（常量）；前端 `dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295` 四处调用 `computeActiveStreakDays({...})`（导入 `:4`）写入自己的值。消费者分布：

| 消费者 | 连续天数来源 | 后果 |
| --- | --- | --- |
| Dashboard 热力图 | 前端 `computeActiveStreakDays` 覆盖 | 正确 |
| Linux 桌宠宿主 | **直读 `r.body?.streak_days`**（`dashboard/src/lib/pet-linux-host.js:198`） | 本地模式下拿到 0 |
| Windows 托盘 | `UsagePoller.cs:314-328` 读 `streak_days`，注释却写「streak is server-computed」 | **本地模式下预计显示 0** |

`【未能确认】` Windows 托盘在本地模式下的实际显示值（源码可证「本地端点恒 0 + 托盘直读该字段」，界面表现需实测；判定条件：启动 Windows 壳 → 非账号视图 → 观察托盘/小组件）；`【未能确认】` 云端 `tokentracker-account-heatmap.ts` 是否返回真实 `streak_days`（判定条件：读该文件全篇，或账号视图下抓响应体查 `streak_days`）。

#### 9.3.4 紧凑热力图的前端展开

账号视图下热力图走稀疏线格式，前端还原（`dashboard/src/lib/heatmap-compact.js:20-75`）：

```js
// dashboard/src/lib/heatmap-compact.js:49-72（节选）
  const maxValue = Number(payload.max_value) || 0;
  const levelOf = (v) => {
    if (v <= 0) return 0;
    if (maxValue === 0) return 1;
    const r = v / maxValue;
    if (r <= 0.25) return 1;
    if (r <= 0.5) return 2;
    if (r <= 0.75) return 3;
    return 4;
  };
  ...
    cells.push({
      day: cursor,
      total_tokens: total,
      billable_total_tokens: total,
      level: levelOf(total),
      models: hit ? hit.models : null,
    });
```

三点结论：**① 展开会重算 `level`**，用的是与服务端相同的「按窗口最大值归一、四档 0.25/0.5/0.75」规则（服务端规则见 `src/lib/local-api.js:2547-2557`）——规则在两处各写一遍，靠数值约定而非共享代码保持一致；**② 紧凑路径下 `billable_total_tokens` 被赋成 `total`**（`:69`，稀疏格式只带一个 `total`），因此账号视图热力图单元格**丢失 billable 与 raw total 的差异**，而 `activeDays` 兜底路径用 `getBillableTotal(row)` 读数（`dashboard/src/pages/DashboardPage.jsx:837`）在紧凑数据上等价于读 total；**③ 防御性设计明确**：非紧凑或形状不合规时原样返回（`:20-28`，同时让新 CLI 对旧 edge 保持正确），缺 `model_names` 字典时拒绝展开而不是静默丢数据（`:24-27`）。

#### 9.3.5 前端是否存在二次计价：**不存在**

独立检索 `dashboard/src` 全目录（`*.js/jsx/ts/tsx`）中 `computeRowCost` 与 `getModelPricing` 的引用：**0 命中**。结论：前端从不重新计价，`total_cost_usd`、逐行成本、档位与分项一律由后端给出。后端侧计价只出现三处：`buildDailyAggregates`（`src/lib/local-api.js:285`）、`usage-model-breakdown`（`:2640`）、`skill_usage` 定价段（`:3381`）。

#### 9.3.6 前端二次聚合清单

| 前端行为 | 位置 | 性质 |
| --- | --- | --- |
| 活跃天数后端优先、缺失才重算 | `dashboard/src/pages/DashboardPage.jsx:819-850` | 后端优先 + 前端兜底 |
| 连续天数重算并覆盖 | `dashboard/src/hooks/use-activity-heatmap.ts:206`、`:220`、`:284`、`:295` | 纯前端（服务端本地值为 0） |
| 紧凑热力图展开（含重算 level） | `dashboard/src/lib/heatmap-compact.js:20-75` | 传输优化的还原 |
| 趋势缩放统计（总量/成本/活跃桶/峰值） | `dashboard/src/lib/trend-stats.js:34-79` | 前端二次汇总 |
| 桶标签本地化与「一律 `timeZone:"UTC"`」格式化 | `dashboard/src/lib/trend-stats.js:106-138`、`:152-180` | 纯展示层 |
| 明细表排序/分页 | `dashboard/src/pages/DashboardPage.jsx:681-750` | 前端（后端用量端点不分页） |
| 显示币种换算 | `dashboard/src/hooks/useCurrency.js:3-15` | 前端换算（服务端只给 USD） |
| token 数字格式 | `dashboard/src/hooks/useTokenFormat.js:2-20`、`dashboard/src/lib/token-format.js` | 前端展示 |
| 榜单列宽/排序/分页配置 | `dashboard/src/lib/leaderboard-columns.js`、`leaderboard-ui.js` | 前端 |
| 份额/占比 | `-project-usage-detail` 已给 `range_total_tokens`（`src/lib/local-api.js:2908-2910`） | 后端给分子分母，前端只做除法 |

**只在后端算、前端一律不重算**：逐行成本与档位、去重（`source|model|hour_start` last-wins）与旧行修正、项目归属与 claude-mem 伪项目剔除、热力图服务端 level、本地徽章阈值与达成时间（`src/lib/local-api.js:419-423`、`:434-511`）、账号级来源跨设备去重语义、会话效率整套定义（`src/lib/session-analytics.js:2385-2566`）。

#### 9.3.7 轮询、缓存与刷新触发

| 机制 | 参数 | 证据 |
| --- | --- | --- |
| 进程内 in-flight 合并 | 键为完整 URL | `dashboard/src/lib/api.ts:62-73` |
| 账号响应 TTL 缓存 | summary 120 s、其余 300 s、stale-if-error 5 min、上限 64 | `dashboard/src/lib/api.ts:75-129` |
| 预取 | `DASHBOARD_PRELOAD_TARGETS = ["limits", "leaderboard"]`；20 条榜单内存 LRU | `dashboard/src/lib/dashboard-preload.js:3`、`:23-71` |
| macOS 事件驱动 | 监听 `queue.jsonl` 写入与 `queue.state.json`，突发写 settle 后再刷一次 | `TokenTrackerBar/TokenTrackerBar/Services/QueueActivityMonitor.swift:3-5` |
| Windows | 健康检查 30 s；用量轮询在 `UsagePoller` | `TokenTrackerWin/Constants.cs:14-15`、`TokenTrackerWin/UsagePoller.cs:85-131` |
| Linux 扩展 | `Soup.Session({timeout: 20})`；菜单打开与固定周期都取数 | `extension.js:784`、`:830-832` |
| 埋点 | PostHog，`autocapture: false`；本地/原生环境先问 `telemetry-pref`，「答不出来就关」（fail-closed） | `dashboard/src/lib/analytics.js:24-45`、`:13`、`:67` |

#### 9.3.8 Vite dev 的 mock 路径（第四份聚合实现）

仅 `npm run dev` 时存在：插件**直接读 `~/.tokentracker/tracker/queue.jsonl`**（`dashboard/vite.config.js:418` 注释、路径常量 `:570`），复刻主链的 last-wins 去重与 codex 旧行修正（`:572-604`：`seen.set(\`${row.source||""}|${row.model||""}|${row.hour_start||""}\`, normalizeQueueRow(row))`），并自带日聚合（`:610-639`）。**它不复制价格表**——通过 `createRequire(import.meta.url)` 直接 require 同一份 `src/lib/pricing`（`getModelPricing`/`computeRowCost` 均来自该模块），`:508-514` 注释逐字写明「Per-model pricing — delegated to `src/lib/pricing/` (CJS)」。唯一已知差异：dev mock 对**按模型聚合后的 totals 只计一次价**（`:1052-1057`），与 `src/lib/local-api.js:2634-2640` 修掉的写法相同，仅影响 dev。

`【未能确认】` dev 中间件对外暴露的具体路径前缀集合（判定条件：读 `configureServer` 的 `server.middlewares.use(...)` 注册串）。

### 9.4 数据导出与分享、成就与宠物等衍生面

#### 9.4.1 Share Card：两套口径拼在一张图上

组装链：页面状态 → `useShareCardData()` → `buildShareCardData()` → `ShareCard.jsx` → 截图。`handle`、`startDate`、`activeDays`、`summary`、`topModels`、`heatmap` 全部来自当前页面已拉到的数据，**唯有 `rank` 单独向云端榜单请求**（`dashboard/src/ui/share/use-share-card-data.ts:11-26`、`:49-80`；`leaderboardPeriod` 把 `day`/`custom` 一律按 `week` 取，`:55`）。

| 细节 | 规则 | 证据 |
| --- | --- | --- |
| 排名可见性 | 只在第一页（`limit: 100`）里找 `is_me`——**排名超过 100 时不显示** | `use-share-card-data.ts:70-72` |
| token 口径 | `billable_total_tokens ?? total_tokens` | `dashboard/src/ui/share/build-share-card-data.ts:47-50` |
| 成本 / topModels / level | `total_cost_usd`；topModels 上限 3；热力图 level 夹在 0–4 | `:118-119`、`:62-85` |
| 截图 | 动态 `import("html-to-image")`，注入 Google Fonts（PRIVACY 已披露） | `dashboard/src/ui/share/capture-share-card.ts:53`、`:4-5`；`docs/PRIVACY.md:86` |
| 契约测试 | 回落 `—`、topModels 截断 3、非法 rank 归 null、千分位边界 | `test/share-card-data.test.js:23-138` |

**结论**：同一张卡片上 token/成本是单机（或账号去重后）口径，rank 是平台全体已上传用户口径——两个数字都不错，但不能相互印证。

#### 9.4.2 徽章与嵌入卡片 SVG（云端产物）

| 产物 | 端点与参数 | 要点 | 证据 |
| --- | --- | --- | --- |
| 徽章 | `/functions/tokentracker-badge-svg?user_id=<uuid>&metric=tokens\|cost\|rank`，缓存 60 s | 自绘 SVG（含 `flat-square` 分支）；数字 `compactNumber`、金额 `formatCost`；先解析榜单窗口 `windowBounds(client, period)` 再读快照行（`rank`、`total_tokens`）；封禁账号走黑名单 | `dashboard/edge-patches/tokentracker-badge-svg.ts:7-21`、`:136-148`、`:68`、`:78`、`:106-129`、`:37` |
| 嵌入卡片 | `tokentracker-embed-svg.ts`（320 行） | 同样窗口计算；头像由显示名哈希生成；主题 light/dark；占位文案 `placeholderSvg` | `:149-159`、`:109-123`、`:124` |

两者都依赖账号与榜单快照，与本地统计无关。契约测试是非 Deno 的 CommonJS 移植镜像，注释声明「Deno copy 是 source of truth，改渲染必须同步」（`test/badge-svg-render.test.js:1-14`）。

#### 9.4.3 Wrapped 的三条消费路径（同一份计算）

| 路径 | 取数 | 渲染 |
| --- | --- | --- |
| CLI `wrapped` | 直接读 `queue.jsonl`（`src/commands/wrapped.js:126-128`） | ASCII 框（`:43-120`）/ `--json` |
| `/wrapped` 页 | `fetch("/functions/tokentracker-wrapped")`（`dashboard/src/pages/WrappedPage.jsx:43-45`） | React，`highlights` 用 copy 模板重排（`:100-121`） |
| 本地 API | `aggregateWrapped(rows, {year})`（`src/lib/local-api.js:2333-2336`） | JSON |

三者共用 `src/lib/wrapped-aggregator.js`（模块头注释自称 sources of truth），因此**内部口径一致，包括同一处 UTC 偏向**。

#### 9.4.4 成就、宠物、技能

本地徽章只有三枚，且**阈值只存在于服务端**：

```js
// src/lib/local-api.js:419-423
const LOCAL_BADGE_THRESHOLDS = {
  project_hopper: [3, 5, 10, 20], // distinct projects
  project_devotion: [1000000, 10000000, 100000000, 1000000000], // max tokens in one project
  night_owl: [5, 20, 60, 150], // active hour buckets between 00:00–05:59 local
};
```

注释同时写明「the dashboard renders whatever the payload says and embeds none of these numbers」（`:416-418`）。达成时间按 `hour_start` 顺序回放、取首次跨阈值的桶（`test/local-achievements.test.js` 锁定）；`night_owl` 按**本地** 00:00–05:59 判定（`:482` 一族）。宠物（`-pets` + `/api/pets/*` 资产路由）与用量统计无关，只是 usage 端点的消费者；技能（`-skills` 的 `skill_usage` 模式）把技能调用量与安装列表 join，按 `source:"claude"` 计价，返回 `{generatedAt, scannedFiles, totalInvocations, cached, skills[{skill, invocations, lastUsedAt, tokens, cost, installed, skillId, directory}], unusedInstalled[]}`（`src/lib/local-api.js:3337-3408`）——这是用量统计向技能维度的一次外延，但不进主链。

#### 9.4.5 营销口径 vs 统计口径

**源码里不存在「节省金额」类指标**。全仓检索 `savings|amount_saved|saved_amount|total_saved` 共 **9 处命中 / 7 个文件**，其中 `amount_saved|saved_amount|total_saved` 为 **0 命中**；`savings` 的 9 处全部与用量统计无关：`dashboard/src/pages/CloudPage.jsx:95`、`:195`（`cloudAnnualSavings(monthly, annual)`＝**订阅年付相对月付的折扣百分比**）、`dashboard/src/content/i18n/zh/dashboard.json:650`、`zh-TW/dashboard.json:709`、`dashboard/src/content/copy.csv:1699`、`DESIGN.md:17`（订阅折扣徽标）、`scripts/cloud-sandbox/hosted-archive-acceptance.cjs:377`、`:450`（`bill_savings_claimed: false` 免责声明）、`docs/cloud-usage-archive.md:70`。

真正存在**口径错配风险**的是三处：**① Wrapped 的 UTC 偏向**（`peak_hour`/`longest_streak`/`active_days`/`top.days` 全按 UTC，`src/lib/wrapped-aggregator.js:37-53`、`:142-168`）与页面按时区切天的 heatmap 冲突，同一用户同年出现两个「活跃天数」；**② Share card 的 rank 与 token 不同源**（`use-share-card-data.ts:55-63`），未登录时 rank 为 `null`（`:51`）；**③ `wrapped` 用 billable 优先且份额按同一 billable 值算**（`src/lib/wrapped-aggregator.js:29-35`、`:182-186`），而 `usage-daily` 的 `models{}` 用 `total_tokens`（`src/lib/local-api.js:297`）。

### 9.5 云上传与聚合

#### 9.5.1 上传内容（字段级）

客户端请求体（`src/commands/sync.js:4367-4374`）：`{upload_id, hourly: result.buckets, ...(sessionStates.length > 0 ? {account_session_states: result.sessionStates} : {})}`。ingest edge 把 `hourly` 逐列映射为数据库行：

```ts
// dashboard/edge-patches/tokentracker-ingest.ts:176-194（节选）
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

**实际随桶上传的业务字段（12 个）**：`hour_start`、`source`、`model`、`input_tokens`、`cached_input_tokens`、`cache_creation_input_tokens`、`output_tokens`、`reasoning_output_tokens`、`total_tokens`、`billable_total_tokens`、`total_cost_usd`、`conversation_count`（服务端另补 `user_id`、`device_id`）。批内去重：按 `(hour_start, source, model)` 取 `total_tokens` **最大**者（`:196-211`），因为 CLI 队列追加式、同一逻辑桶随会话填充被重复写入且每次携带累计值。

`account_session_states` 是**条件上传的第二种记录**：

| 项 | 规则 | 证据 |
| --- | --- | --- |
| 仅接受来源 | `trae-cn`（其余一律 400 拒收）；客户端侧同样只接受 `trae-cn` | `:139-152`；`src/lib/upload-queue.js:9-18` |
| 字段 | `source`、`session_id`、`model`、`bucket_start`、`snapshot_verified_at` + 六类 token | `:153-160` |
| 身份 | `(user, source, session_id)`——**device_id 不是身份**，同账号多设备经 LWW upsert 收敛到同一行 | `:103-109` |
| 校验 | `total_tokens === input + cached + cacheCreation + output` 双向强制 | `:148-149`、`upload-queue.js:16` |
| 批内冲突 | 同 `(source, session_id)` 只保留最后一条（否则 ON CONFLICT 对同一行触发两次） | `:162-167` |
| 单批上限 | 会话状态 500、桶 500；请求体 1 MiB | `:111-113`、`:172-174`、`:47-49` |
| 只有状态没有桶 | 合法上传，不落 `no-buckets` 拒绝分支 | `:108-109`、`:169-171` |

#### 9.5.2 频率与节流

| 参数 | 值 | 来源 |
| --- | --- | --- |
| 基础间隔 / 抖动 / 积压阈值 | 30 分钟 / ≤60 s / 1,000,000 字节（与 `upload-queue.js:5` 的 `BACKLOG_BYTES` 同值） | `src/lib/upload-throttle.js:2-4` |
| 批量 | 客户端读取默认 200；后端 batch 300；ingest 上限 500 | `:5`、`src/commands/sync.js:4439-4441`、`tokentracker-ingest.ts:172-174` |
| 批量档位 | 小 2 批 / 大 4 批 | `:6-7`、`:57` |
| 失败退避 | 60 s 起、上限 30 分钟、指数 `2^step`、step 上限 10（记录时 20） | `:8-9`、`:109-110`、`:123` |
| 服务端截止 | `Retry-After` 优先于配置上限；`cloud_sync_throttled` + 429 时以服务端时间为 `nextAllowedAtMs` | `:104-107`、`:114-116` |
| 生效判定 / 云侧下发 | `max(nextAllowedAtMs, backoffUntilMs)` 未到即拒绝；云侧 `sync_interval_seconds` 与 `next_allowed_at` 取更晚者 | `:46-55`、`:72-77` |
| 心跳 | 24 小时 + 60 s 抖动 | `src/lib/telemetry.js:41-46`、`:116-124` |

上传是**按字节偏移的追加流**（`queue.state.json` 记偏移，`status` 的 `pending_bytes` 即「文件字节数 − 偏移」），重复上传靠云端批内 MAX 与 RPC 幂等兜底。

#### 9.5.3 边缘函数与云端再聚合

`dashboard/edge-patches/` 共 **28 个文件**（全部 `.ts`），关键项：

| 文件 | 角色与要点 |
| --- | --- |
| `tokentracker-ingest.ts`（225 行） | 用量写入。Bearer = device token → sha256 → 查 `tokentracker_device_tokens`（`:69-82`）；封禁用户 403（`:92-94`）；调 `cloud_ingest_usage` RPC 带 `p_upload_id`（`:214-218`） |
| `tokentracker-telemetry.ts`（85 行） | 匿名心跳；`machine_hash` 必须匹配 `/^[0-9a-f]{64}$/`（`:25`、`:53`）；注释声明「one row per (machine_hash, day)」且「cannot be joined to device rows」（`:6-9`） |
| `tokentracker-account-summary.ts`（954 行） | 云端概览，**显式声明镜像本地响应 schema**（`:3`、`:12`）；用 `account_summary_wire` RPC（`:690`）并解码紧凑线格式（`:623`）；**自带一份定价表与 `getModelPricing`**（`:398`、`:524`、`:718`）；跨设备去重（`:22`）；交叉设备 30 s 共享缓存（`:662`） |
| `-account-daily.ts`（894）/ `-hourly.ts`（333）/ `-monthly.ts`（261） | 日/小时/月镜像 |
| `-account-heatmap.ts`（391） | 热力图镜像；`format=compact`（`:114-147`）；30 s 缓存 + 5 分钟 stale-if-error（`:145-148`） |
| `-account-model-breakdown.ts`（956）/ `-account-devices.ts`（268）/ `-account-skills.ts`（251） | 模型/设备/技能维度镜像 |
| `tokentracker-leaderboard.ts`（262） | 读榜单快照；`period=week\|month\|total` 决定 `(from_day, to_day)`（`:97`、`:131-152`）；按 `rank` 升序分页（`:167-181`）；附徽章数据 RPC（`:220`） |
| `-leaderboard-refresh.ts`（1449）/ `-leaderboard-profile.ts`（1074） | 榜单快照生成（含异常账号排除）；个人公开档案页数据 |
| `-community-models.ts`（150） | 社区模型统计，**读预计算表 `tokentracker_community_stats`**（`:84-85`），测试要求不得回退全表扫描（`test/community-stats-cache.test.js:16-20`） |
| `-public-visibility.ts`（210）/ `-profile-likes.ts`（354） | 公开可见性与点赞 |
| 设备令牌族 / 计费族 | `-device-token-issue.ts`（183）+ 3 个 device-flow + `-device-rename.ts`（164）；`-billing.ts`（492）+ 4 个 webhook（支付宝/Paddle/Waffo/微信，不在统计范围） |

**云端另有一份 schema 镜像与一份定价实现**。`test/edge-pricing-parity.test.js`（420 行）就是为防止两份定价漂移，但其第一层只做 edge↔edge 比对，不含「edge 覆盖本地 curated 全量」的断言——「本地算得对」推不出「云端也对的」。

#### 9.5.4 社区/排行榜与本地口径的五处差异

1. **数据总体不同**：本地看本机 `queue.jsonl`；榜单看「所有已上传设备」的快照表，未登录/未同步的历史在云端不存在，云端也可能含本机未见的设备。
2. **去重语义不同**：账号级来源（`cursor`、`trae-cn`）跨设备**去重不求和**，本地单机视角没有这个问题；一致性由 `test/account-source-parity.test.js:3-18` 强制。
3. **时间窗口不同**：榜单按 `(period, from_day, to_day)` 快照查询（`tokentracker-leaderboard.ts:131-152`），窗口由服务端刷新任务写入；本地端点按请求参数实时算。
4. **成本口径可能不同**：云端自带定价实现（`-account-summary.ts:398`、`:524`），与本地 `src/lib/pricing/` 是两份代码。
5. **可见性不同**：榜单只展示开公开资料的用户；关闭后从榜单移除并把徽章变成占位（`docs/PRIVACY.md:113`）。

#### 9.5.5 用量归档

热表保留小时级原始行；冷档按 `(user, device, UTC day)` 打成私有 PostgreSQL JSONB 包（`docs/cloud-usage-archive.md:3`）。读取路径 `cloud_usage_hourly(user, device, from, to)` 先按用户/设备/UTC 日筛 manifest，再展开 JSONB，并按 `(user_id, device_id, hour_start, source, model)` + 服务端修订号降序合并冷热（`:17`），仅 `project_admin` 可执行。prepare/commit 两步：`cloud_prepare_usage_archive`（单包上限 10,000 条 / 8 MiB）与 `cloud_commit_usage_archive(generation)`，后者只在「修订号仍等于快照」时删热行（`:35-39`）。**可见性限制 ≠ 删除**（个人小时级 90 天、日/月级 24 个月，`:13`）。本地 runner `scripts/cloud-usage-archive.cjs` 默认 dry-run，`--apply` 才写，凭据不入 checkpoint（`:48-53`）；文档末尾明确**尚未启用任何调度**、未迁移或删除生产数据（`:5`、`:74-83`）。

#### 9.5.6 隐私边界核验（逐字段）

| 实际上传项 | 内容 | 结论 |
| --- | --- | --- |
| 小时桶 | 9.5.1 所列 12 个业务字段 + `user_id`、`device_id` | 与文档「小时桶」方向一致，**字段枚举未逐字覆盖** |
| `billable_total_tokens` | 「计费口径 token」 | **文档未列**（`tokentracker-ingest.ts:188`） |
| `total_cost_usd` | 金额 | **文档未列**（`:189`） |
| `account_session_states`（仅 trae-cn） | 含 `session_id`、`model`、`bucket_start`、`snapshot_verified_at` + 六类 token | **文档「Never sent: Local session records」一句与此存在字面张力**；字段中无 prompt/对话正文 |
| 心跳 | `machine_hash`、`app_version`、`platform`、`shell`（`src/lib/telemetry.js:75-82`） | 与文档一致；`machine_hash` 版本化、无法反推 device 行（`:34`） |
| 设备注册 | `machineId` + **主机名作为 `device_name`**（`src/lib/local-api.js:3137-3143`、`:31-41`，截断 128 字符） | **文档写了 machine id，未写「主机名会作为设备显示名」**；云端设备页与榜单会出现该名称（`dashboard/src/lib/device-label.js`、`dashboard/src/lib/api.ts:1054-1063`） |
| 不上传项核验 | `session.queue.jsonl` / `project.queue.jsonl` 不在上传路径内（`src/commands/sync.js:3656` 注释逐字「Only the main queue is uploaded」） | 一致 |

需要披露的三处文档未覆盖实现细节：**① `billable_total_tokens` 与 `total_cost_usd` 随桶上传**；**② `account_session_states` 携带 `session_id`**（仅 trae-cn，需本地已登录该应用，无对话内容）；**③ 设备注册上传 `machineId` + 主机名作为设备显示名**。另有三条实现比文档更严（不构成风险）：心跳可由 `TOKENTRACKER_NO_TELEMETRY`/`DO_NOT_TRACK`/`config.telemetry=false` 任一关闭（`src/lib/telemetry.js:48-53`），前端「问不到偏好就关」（`dashboard/src/lib/analytics.js:13`、`:67`）；会话浏览器端点仅本地（`src/lib/local-api.js:2495-2499`）；`summarizeSessions` 默认剥离 `project_ref`/`session_id`/`title` 且 `provenance.privacy = "metadata-only"`（`src/lib/session-analytics.js:2526-2543`、`:2561`）。

### 9.6 三端原生壳如何拿到统计

#### 9.6.1 端口矩阵与理由

| 端 | 端口 | 证据 |
| --- | --- | --- |
| CLI `serve` | 7680，忙则顺延（最多 20 次） | `src/commands/serve.js:14`、`:22`、`:208-215` |
| CLI under WSL | 7681（7680 被 Windows DoSvc 占用） | `src/commands/serve.js:15-21`、`:202-206` |
| macOS app | 固定 7680（`NativeQAProfile` 可覆盖） | `TokenTrackerBar/TokenTrackerBar/Utilities/Constants.swift:4-5` |
| Windows app | 运行时挑空闲回环端口，**优先 17680** | `TokenTrackerWin/ServerManager.cs:469-476`、`:157` |
| Linux app | 固定 17680 | `TokenTrackerLinux/gnome-extension/tokentracker@tokentracker.cc/extension.js:18-19`；`TokenTrackerLinux/src-tauri/src/server.rs` |

Windows 偏好 17680 的理由是三条约束的交集（源码注释逐字，`TokenTrackerWin/ServerManager.cs:471-476`）：OAuth 回调 `http://127.0.0.1:<port>/auth/callback` 必须在 InsForge 白名单里（动态端口做不到，故用固定端口，与 macOS 的 `:7680` 并列登记）、落在 IANA registered 段（10000–49151，Windows 不会当临时端口发出去）、避开 DoSvc 占用的 `:7680`。

#### 9.6.2 macOS：spawn CLI serve + 原生视图

启动内嵌 server：找 CLI 入口后 spawn `[entryPath, "serve", "--port", "<7680>", "--no-sync", "--no-open"]`（`TokenTrackerBar/.../ServerManager.swift:195`），走登录 shell 的兜底分支为 `zsh -lc "<bin> serve --port <7680> --no-sync"`（`:228`），启动前用 `lsof -ti tcp:<port> -sTCP:LISTEN` 清占用者（`:83-99`）。**数据入口是 HTTP，不是直读文件**：

| 方法 | 路径 | 行号 |
| --- | --- | --- |
| `fetchSummary` / `fetchDaily` | `-usage-summary?from&to` / `-usage-daily` | `:43-52` / `:66-69` |
| `fetchHeatmap` | `-usage-heatmap?weeks=52`（`tz`/`tz_offset_minutes`/`account=1` 由 helper 追加） | `:73-77` + `:241-254` |
| `fetchModelBreakdown` / `fetchProjectUsage` | `-usage-model-breakdown` / `-project-usage-summary` | `:79-82` / `:86-90` |
| `fetchMonthly` / `fetchHourly` | `-usage-monthly` / `-hourly?day=` | `:94-97` / `:101-105` |
| `fetchUsageLimits` / `fetchSubscriptions` | `-usage-limits`（`devin=1` 可选）/ `-subscription-manager` | `:110-113` / `:119-121` |
| `triggerSync` / `checkServerHealth` | `-local-sync` / `-user-status`（只判 200） | `:127-143` / `:148-150` |

所有请求都经查询串 helper：`withTimeZoneQueryItems` 追加 `tz` + `tz_offset_minutes`（`APIClient.swift:241-246`），`withAccountQueryItems` 在其上再追加 `account=1`（`:252-254`）；`fetchSummary`/`fetchHeatmap`/`fetchModelBreakdown`/`fetchMonthly`/`fetchHourly` 用后者，`fetchProjectUsage` 用前者——**macOS 壳不缺 tz**（这是判断 9.1.5 冲突是否在真实路径生效的前提）。`fetchHourly(day:)` 的 `day` 来自 `DateHelpers.dayRange`（`DateHelpers.swift:44-50`，`.current` 时区的 `yyyy-MM-dd`），调用点 `DashboardViewModel.swift:192-194`/`:311`，与随请求发出的 `tz` 同基准。

菜单栏/弹出层由 `StatusBarController`（1507 行）与 `DashboardViewModel`（1151 行）驱动，后者是唯一取数状态机。小组件（`TokenTrackerWidget/`）**不联网**，读 App Group 里的快照：容器 `group.com.tokentracker.bar`、文件 `widget-snapshot.json`、schema 版本 2（`TokenTrackerBar/Shared/WidgetSnapshot.swift:18-30`）；字段含 `serverOnline`、`today`/`last7d`/`last30d`/`total`/`selected` 的 `PeriodTotals`、`dailyTrend[]`、`topModels[]`、`sources[]`、`heatmap`、`limits[]`（`:35-66`）；写入时机为 `DashboardViewModel.loadAll()` 后 `WidgetSnapshotWriter.update(from:capturedAt:)` 原子写入并 `WidgetCenter.reloadAllTimelines()`（`WidgetSnapshotWriter.swift:54-110`）。**唯一直接触碰本地文件的路径**是活动监听（不解析内容）：`QueueActivityMonitor` 以 `O_EVTONLY` 打开 `queue.jsonl` 监听写入/替换，第二实例监听 `queue.state.json` 发布「上传完成」（`QueueActivityMonitor.swift:1-36`）。

#### 9.6.3 Windows：宿主定端口，托盘自建 HttpClient

端口由宿主决定：`Port = PickServerPort()`、`BaseUrl => http://127.0.0.1:{Port}`（`ServerManager.cs:12-20`、`:34-38`、`:157`）；健康循环 30 s、启动等待 20 s（`Constants.cs:14-18`）。托盘取数用 `UsagePoller`（自建 `HttpClient`，`UseProxy=false`，6 s 超时，`:38-40`）：`FetchLimitsAsync` → `-usage-limits`（`:182-187`）；`FetchAsync` → `-usage-summary?from&to...`（`:226-250`）；`FetchHeatmapAsync` → `-usage-heatmap?weeks=52&tz...&account=...`（`:316-328`）；`FetchTopModelsAsync` → `-usage-model-breakdown?...`（`:340-354`）。UI 与桥接：`DashboardWindow.cs`（WebView2，1235 行）、`WidgetWebViewEnvironment.cs`、`QuotaWidgetWindow.cs`（配额小组件）、`TrayApplicationContext.cs`（托盘与菜单）、`LocalSyncPublisher.cs`（同步结果广播）。宿主还驱动后台同步（`ServerManager.cs:313` 在服务器就绪后发布），与 CLI 侧 `TOKENTRACKER_NATIVE_SYNC_OWNER=windows-host` 判定互斥（`src/commands/serve.js:305-309`），避免双份自动采集。

`【未能确认】` `TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源（判定条件：读该文件；推测同为本地 API 或经 `UsagePoller` 推送，未验证）。

#### 9.6.4 Linux：Tauri 壳 + GNOME 扩展

Tauri 壳 `src-tauri/src/server.rs` 负责 spawn/记录/回收内嵌 server（每进程一个 `server-{owner_pid}.json`，含 pid/port/boot_id/进程启动时间，`:138-160`、`:246-257`；`reap_orphaned_servers()` 启动时清理孤儿，`:343-372`）；`main.rs`、`tray.rs`、`pet.rs`、`cloud_export.rs` 分别负责窗口/托盘/桌宠/云导出。GNOME 扩展直连 HTTP（`const BASE_URL = 'http://127.0.0.1:17680'`，`extension.js:18-19`）：

| 用途 | 路径 | 行号 |
| --- | --- | --- |
| 周期数据 | `-usage-model-breakdown`、`-hourly?day=`（`day` = `dayString(new Date())`，本机时区当日，`:1044`/`:273-275`）、`-monthly` | `:1046-1051` |
| 今日摘要 / 全量视图 | `-usage-summary?from=today&to=today`；`-usage-summary`（total）、`-daily`、`-heatmap?weeks=52` | `:1086`；`:1110-1116` |
| 限额 / 触发同步 | `-usage-limits`；`POST -local-sync` | `:1137`；`:1025` |

扩展把「非 404 的 HTTP 错误」与「端口被别的服务占用（404 → `ForeignServerError`）」区分开（`:229-245`），避免读到别人的服务；所有 `/functions/` 读取都经 `_getJson`，自动拼 `tz`、`tz_offset_minutes` 与 `account=1`（`:997`）。

#### 9.6.5 是否存在绕过 serve 直读队列

| 途径 | 是否直读 `queue.jsonl` | 证据 |
| --- | --- | --- |
| macOS 壳 | 只做文件事件监听，**不解析内容** | `QueueActivityMonitor.swift:1-36` |
| Windows 壳 | 未发现；文件级接触限于 `~/.tokentracker/pets` | `TokenTrackerWin/PetWindow.cs:1042`、`TrayApplicationContext.cs:566` |
| Linux 壳 / GNOME 扩展 | 未发现；全部经 HTTP | `extension.js` 的 `_getJson` 一族 |
| **Vite dev 插件** | **是**——直读、自行去重、自行日聚合 | `dashboard/vite.config.js:418`、`:570`、`:583-639` |

### 9.7 与用量统计的边界

#### 9.7.1 限额链路与 token 统计完全独立

`-usage-limits` 走「provider 官方 API + 本地磁盘缓存」，**从不进入 `queue.jsonl`**，在数据源、聚合、缓存、展示四层都不相交：

| 维度 | 事实 | 证据 |
| --- | --- | --- |
| 抓取入口 | `fetchUsageLimitsUncached()` 并行拉取多家 provider | `src/lib/usage-limits.js:3976`、`:4055` |
| 进程内缓存 | 按「选择键」分区，TTL 2 分钟（`CACHE_TTL_MS`） | `:64` |
| 选择键 | 二元组 `${devin}-${antigravity}`：`devin` 由 `devinEnabled === true` 决定，`antigravity` 由 `isAntigravityQuotaDisabled()` **取反**决定；分区枚举 `off-on`/`off-off`/`on-on`/`on-off` | `:56-63` |
| 并发合并 | single-flight（in-flight 表），同一键只抓一次 | `:3954-3970`、`:3962-3968` |
| 磁盘缓存 | 每家 provider 一份 `*-usage-limits-cache.json`，最长保留 7 天；Claude 另有新鲜度 10 分钟与 429 冷却 | `:71`、`:118-129`、`:120`、`:141-144`；`docs/PRIVACY.md:43` |
| provider 数 | `Promise.all` 解构 18 个、`data` 收录 17 家（`claudeServiceStatus` 未进 `data`） | `:4055`、`:4406-4440` |
| 可信度元数据 | 注入 `stale`（显式或 `cached_at` 早于 10 分钟）、`source`（显式，否则 `disk-cache`/`provider-api`）、`confidence`（来源串匹配 `estimate\|inferred` → `inferred`；匹配 `local\|database\|sqlite\|cache` → `observed`；否则 `official`），另附 `captured_at`、`age_seconds` | `:4442-4461` |
| 窗口结构 | 统一 `buildWindow({usedPercent, resetAt, windowSeconds})`，百分比经 `clampPercent`；Codex 有 5 小时（18000 s）与每周（604800 s） | `:161`、`:146`、`:332-333` |
| 前端展示与一致性 | `LimitsPage.jsx` 直接 `Object.entries(data)` 铺平 provider 行，用 `provenance.stale` / `source==="disk-cache"` 判定「全部来自缓存」；前后端 provider 清单由 `test/limits-providers-parity.test.js` 锁定 | `dashboard/src/pages/LimitsPage.jsx:49`、`:59-66` |

**结论**：限额视图是「配额百分比与重置时间」，用量统计是「token 与成本」；两者唯一耦合是共用同一个 `serve` 进程与前缀路由。

#### 9.7.2 其它相邻但不属于统计的边角面

| 面 | 归属 | 与用量统计的关系 |
| --- | --- | --- |
| 成就/徽章 | 本地阈值在服务端、达成时间按 `hour_start` 回放；云端徽章读榜单快照 | 消费主链行，不改用量口径 |
| 宠物（桌宠） | UI 与资源包 | 只是 usage 端点的消费者 |
| 技能（`-skills` / `skill_usage`） | 独立聚合 + `source:"claude"` 计价 | 一次 token 外延统计，不进主链 |
| 会话效率（`-sessions` / `-session-insights`） | 独立第二条链（`session-analytics.js`） | 产出 token 与成本，但**与主链不同源**，不可直接对账 |
| 计费/订阅/webhook | `tokentracker-billing.ts` + 4 个 webhook | 商业链路，与 token 统计无关 |
| 代理/geoip/iprisk/avatar | 纯网络工具端点 | 与统计无关 |
| SCIP 代码图索引 | `graph-auto-index-*` | 与统计无关 |

#### 9.7.3 口径对账顺序（推荐）

要与其他工具对账 TokenTracker 的数字，按此顺序定位分歧命中率最高：**① 时区基准**（`wrapped` 恒 UTC；Dashboard 与三端壳按传参 tz；裸调 API 分两族）；**② 去重语义**（本地 last-wins vs 云端批内 MAX）；**③ token 口径**（`billable_total_tokens` vs `total_tokens`）；**④ 来源范围**（`scope` 是否含账号级来源 `cursor`/`trae-cn`；账号视图是跨设备去重而非相加）；**⑤ 是否账号视图**（看 `X-TokenTracker-Account-View` 与 `-Fallback` 头）；**⑥ 成本来源**（本地逐行计价求和 vs 云端自带定价实现）。

#### 9.7.4 本章未能确认项与判定条件

| # | 未确认内容 | 判定条件 |
| --- | --- | --- |
| 1 | Windows 托盘在本地模式下「连续天数」的实际显示值（源码可证本地端点恒 0、托盘直读该字段） | 运行 Windows 壳 → 非账号视图 → 观察托盘/小组件；若显示真实值则说明另有来源 |
| 2 | 云端 `tokentracker-account-heatmap.ts` 是否返回真实 `streak_days` | 读该文件全篇，或账号视图下抓响应体查 `streak_days` |
| 3 | `-local-sync` 成功响应中 `syncResult` 的具体键名 | 读 `src/commands/sync.js` 的 drain 返回值；本章只确认 `{ok, uploaded, pending, next_allowed_at?}` 与失败形状（`src/lib/local-api.js:2322`、`:2324`） |
| 4 | `-context-health` 的字段结构 | 读 `src/lib/context-health.js` |
| 5 | `/api/auth/*` 的完整转发规则（cookie relay、csrf 优先级、relay 回退条件） | 读 `src/lib/local-api.js:1792-1940` 全段 |
| 6 | `src/lib/git-outcomes.js` 的「只用 subject 检测 revert」是否逐行成立 | 读该模块并与 `docs/PRIVACY.md:21` 对照 |
| 7 | Vite dev 中间件对外暴露的路径前缀集合 | 读 `dashboard/vite.config.js` 中 `configureServer` 的中间件注册串 |
| 8 | `usage-limits` 的 provider 计数口径；`ark` 两家与 `qoder` 两家是否共用凭据 | 读 `:4055` 解构段与 `:4406-4440`、各 `*-limits.js` 凭据读取 |
| 9 | 榜单/社区表的 SQL 细节 | 读 `scripts/ops/*.sql`、`migrations/` |
| 10 | 「compressed usage metrics 可长期保留」的保留期限数值 | 源码/文档未见最终数值；`docs/cloud-usage-archive.md:74-83` 明确未启用 |
| 11 | macOS 小组件刷新频率与快照 schemaVersion 2 的兼容迁移细节 | 读 `WidgetSnapshotWriter.swift` 与 Widget 侧读取段 |
| 12 | `TokenTrackerWin/QuotaWidgetWindow.cs` 的数据来源 | 读该文件；推测同为本地 API 或经 `UsagePoller` 推送，未验证 |

上述 12 项均不影响本章已给结论：每条结论要么有源码行号支撑，要么已在本表标明边界。
