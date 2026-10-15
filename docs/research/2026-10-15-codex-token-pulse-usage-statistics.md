# codex-token-pulse 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`puppnn/codex-token-pulse`](https://github.com/puppnn/codex-token-pulse)，
> main 分支 HEAD `4e6e3150e0c5a074f6c8700265a0bb34744f97fd`（2026-09-08，
> "显示一位小数缓存命中率并隔离预览测试数据写入"）。克隆到临时目录后逐段精读源码，
> 并在隔离的合成日志沙箱里实际运行导出器复算数值。
> **数据边界**：全程未读取本机任何真实会话内容；唯一的本机接触是第 6 轮为核对 schema 假设
> 而执行的**只读** `PRAGMA table_info`（`~/.codex/state_5.sqlite`、`~/.codex/logs_2.sqlite`、
> `~/.cc-switch/cc-switch.db`，仅取列名，不读任何行）；未写入或改动上述任何路径。
>
> 证据分档：**已验证**（源码逐行可证，或已在隔离沙箱复现数值）／**一致解释**（多条代码
> 证据指向同一结论，无单点直证）／**未验证推论**（结构推断或未能从仓内取证，列入 §附录 C）。
>
> 本文只记录字段名、表名、路径规则与计算口径，不含任何真实会话内容。

## 1. 范围与方法

### 1.1 项目形态与代码规模

Token Pulse 是一个 **Windows 桌面 Token 悬浮窗**（Python + Tkinter，默认 Orbit 竖向界面，
可选液态玻璃材质），同时附带一个本地历史分析网页。它不是库、不是服务端插件，而是
"窗口进程 + 导出器脚本 + 分析服务"三件套（`README.md:3`、`README.md:217-227`）：

| 文件 | 行数 | 职责（README `217-227` 的文件说明 + 源码口径；测试类为本文补注） |
| --- | ---: | --- |
| `monitor.py` | 15107 | 窗口、刷新调度、Sub2API 读取和本地统计整合；实时增量监听 |
| `client_usage_export.py` | 14200 | 本地客户端 JSONL/SQLite 用量扫描器（**统计口径的唯一权威实现**） |
| `analytics_server.py` | 1009 | 本地历史分析控制台（HTTP，`127.0.0.1:8765`） |
| `monitor_ui.py` | 1318 | Orbit 布局、交互与状态展示 |
| `monitor_window.py` / `monitor_refraction.py` / `monitor_capture.py` / `monitor_glass.py` / `monitor_materials.py` | 合计约 63 KB | 窗口合成、GPU 材质、背景采集（与统计无关） |
| `tests/test_usage_state_boundaries.py` | 18534 | 状态边界与口径的权威行为说明（766 KB，最大的测试文件） |
| `tests/test_hybrid_usage_reconciliation.py` | 826 | 去重与对账行为 |
| `tests/test_opencodex_account_attribution.py` | 705 | OpenCodex 账号归属行为 |
| `tests/test_unpriced_model_accounting.py` / `tests/test_monitor_unpriced_cost.py` | 27 KB / 11 KB | 未收录模型计价行为 |

### 1.2 方法

1. `git clone` 到临时目录，冻结 HEAD；用 `Select-String` 提取全部顶层符号建立函数地图。
2. 按主题分段精读：来源发现 → 解析 → 去重/对账/归属 → 计价 → 聚合/输出/历史 → 界面。
   每段读完后在**隔离沙箱**（`USERPROFILE` 指向临时 HOME、`TOKEN_PULSE_DATA_DIR` 指向
   临时数据目录、价格源指向不可达地址以强制离线）实际跑导出器复算（见附录 A）。
3. 交叉核对：五路独立分节研究各自成文，再逐条对照源码行号审查（见附录 C 复核记录）。

**行号约定**：正文中的裸行号（如 `` `3646-3708` ``）默认指 `client_usage_export.py`；
指向其他文件时写全名（`monitor.py:7793`、`monitor_ui.py:632`、`analytics_server.py:52`），
或在段首显式声明"本节行号均指 X"。

---

## 2. 总体架构：两个进程、三类数据面

### 2.1 进程与文件职责

**已验证**：Token Pulse 的统计不发生在窗口进程里，窗口只读取导出器写出的 JSON。

```
Codex / Claude / Grok / Cockpit / OpenCodex / Sub2API
        │（本地文件 / 本地 SQLite / 本机 HTTP）
        ▼
client_usage_export.py  ──写──►  client_usage_today.json   （今日权威快照）
   （发现→解析→去重→归属→计价）      usage_history.json          （历史日粒度库）
                                    client_usage_*.json/.jsonl  （缓存/账本/账本副本）
        ▲                                        │
        │ subprocess（超时 90s）                  │ 读
monitor.py（Tk 悬浮窗，实时层）◄─────────────────┘
        │
        ▼ HTTP
analytics_server.py（历史分析网页，只读 usage_history.json + cc-switch.db）
```

- 窗口调用导出器的两处：常规刷新 `monitor.py:4809-4830`（`subprocess.run`，
  `timeout=CLIENT_USAGE_EXPORT_TIMEOUT_SECONDS` 默认 90）、额度专用刷新
  `monitor.py:14847-14878`（`--quota-only`）。
- 实时补扫调用 `monitor.py:12621-12645`（`--live-since/--live-through`）。
- 命令构造 `monitor.py:4785-4790`（`.exe/.com/.bat/.cmd` 直跑，否则用解释器）；可执行体与输出路径由 `CLIENT_USAGE_EXPORT` /
  `CLIENT_USAGE_JSON` 覆盖（`monitor.py:264-280`）。
- 导出器主入口 `client_usage_export.py:13751`（`main`），参数：`--output`、`--date`、
  `--include-30d`、`--backfill-history-details`、`--offline-backfill`、
  `--offline-backfill-run-id`、`--quota-only`、`--live-since`、`--live-through`（`13753-13761`）。

### 2.2 一次常规刷新发生了什么

`main()`（`client_usage_export.py:13751-14196`）的执行顺序**已验证**：

| 步 | 动作 | 位置 |
| ---: | --- | --- |
| 1 | 解析参数、确定统计日 `day` 与窗口 `[start, end)`（当天 `scan_end = min(now, end)`） | 13764-13820 |
| 2 | 记录账号快照（官方 auth + OpenCodex），载入 attribution ledger 与 verdicts | 13823-13827 |
| 3 | 读速度配置（Cockpit 侧车 → `speed_by_account` → `cost_multiplier_by_label`） | 13828-13832 |
| 4 | `scan_all_codex_events`：rollout 扫描（含 fork 剔除）→ logs_2 → 路由标记 → 通用去重 → OpenCodex 对账 → 账号 hint | 13835-13848 |
| 5 | 桌面日志失败事件、速度标记回填、账号切换/请求 marker、Cockpit affinity 扫描 | 13850-13877 |
| 6 | 归属：`attribute_codex_events_by_account` → mirror 补漏 → `resolve_api_service_event_accounts` → 分桶 | 13879-13921 |
| 7 | Claude / Grok 独立扫描与分桶；三者合并小时序列；失败小时标记 | 13923-13945 |
| 8 | 窗口统计（5h/7d/30d/cycle，仅当天）；window_only 标签补零；写 ledger | 13955-13976 |
| 9 | 构建 `output` 字典（today / providers / latest_request / active_sessions / dashboard） | 13978-14126 |
| 10 | 落盘收口：镜像聚合 → 同日高水位 → 历史回填 → 未归因缺口 → 原子写（**5 个动作**） | **14158-14162** |
| 11 | 离线回填 worker 入队（可能在 14163-14183 二次落盘） | 14127-14183 |

### 2.3 `providers` 行的构成与账号口径

**已验证**：`output.providers` 是**按归属标签分桶**的列表，包含以下类型：

- Codex 账号行（`Codex local - <email>`、`Codex local - OpenCodex-pXXXXXX` 等）：
  名字含 `@` 时额外注入 `window_5h/window_7d/window_rolling_7d/window_cycle`（`14002-14005`）。
- 外部子代理行 `Grok subagent` / `OpenCode subagent`（`client_usage_export.py:366-372`、
  `1655-1691`；中文展示名见 `monitor.py:1024-1025`）。
- `Claude local` 与 `Grok local` 两条固定行（`14007-14010`，`GROK_LOCAL_LABEL="Grok local"` @366）。
- 未归属兜底行 `Unassigned local`（`UNASSIGNED_CODEX_LABEL`）/ 聚合行
  `Codex local - api-service-local`（`API_SERVICE_AGGREGATE_LABEL` @491）/ 今日缺口行
  `Codex local - 历史缺口未归属`（`HIGH_WATER_UNATTRIBUTED_LABEL` @480-483，环境变量未设置时
  在 `489-490` 被覆盖为该值）。

沙箱实测输出（附录 A）中即可看到 `Codex local`、`Unassigned local`、`Claude local`、
`Grok local` 四行并存，而 `dashboard.hourly_today` 与 `today` 是全量合计。

### 2.4 时间基准

**已验证**：全链路只有**一个**时区常量参与计算——`LOCAL_TZ = timezone(timedelta(hours=8))`
（`client_usage_export.py:453`，固定 UTC+8，不读系统时区）。窗口进程侧另有
`CN_TZ`/`DISPLAY_TIMEZONE = "Asia/Shanghai"`（`monitor.py:360-361`）用于展示与
Sub2API 请求参数；分析服务同样固定 UTC+8（`analytics_server.py:51`）。

---

## 3. 数据来源与发现规则（"多个 Agent 应用分别怎么读"）

### 3.1 来源总表

| 应用 | 根路径（默认） | 载体 | 时间过滤 | 增量缓存 |
| --- | --- | --- | --- | --- |
| Codex 官方客户端 | `~/.codex/sessions` | `YYYY/MM/DD/*.jsonl` | `state_5.sqlite` 索引 + mtime ≥ start−2h；日期目录不按 mtime | `client_usage_codex_event_cache.json` |
| Codex 索引库 | `~/.codex/state_5.sqlite`、`~/.codex/sqlite/state_5.sqlite` | 表 `threads(id, rollout_path)` | SQL 无时间条件，Python 侧做 | 同上（路径级） |
| Codex 日志库 | `~/.codex/logs_2.sqlite` | 表 `logs(ts, feedback_log_body)` | 秒级 ±300 s + 行内时间戳 | 无（每次全查窗口） |
| Codex Desktop | `%LOCALAPPDATA%\...\Codex\Logs` 等多类候选（详见 §3.4） | `YYYY/MM/DD/codex-desktop-*.log` | **按 UTC 日期目录**推导 | 无（只产失败事件） |
| Claude Code | `~/.claude/projects` | 递归 `*.jsonl` | mtime ≥ start−2h | 无（每次重扫窗口内文件） |
| Grok CLI | `~/.grok/sessions` | `**/updates.jsonl` | mtime ≥ start（**无 −2h 缓冲**） | 无 |
| Antigravity Cockpit | `~/.antigravity_cockpit` | `codex_local_access_logs.sqlite` 表 `request_logs`；`logs/codex-api.log*`；`logs/app.log*`；清单/侧车 JSON | 毫秒时间戳区间；日志按文件名日期 + mtime | 进程内标签缓存、switch-log 缓存；**SQL 全窗口 fetchall，无事件级增量** |
| OpenCodex 代理 | `~/.opencodex`（可 `CLIENT_USAGE_OPENCODEX_DIR` 覆盖） | `usage.jsonl`、`config.json*` | 外层扫描窗口 `[start−6h, end+6h]`；marker 内层准入 ±180 s（`4497-4498`、`4514`） | `client_usage_opencodex_usage_cache.json` |
| Sub2API 服务端 | `SUB2API_BASE_URL`（默认 `http://127.0.0.1:8080`） | 管理端 REST | 服务端按 `days`/`time_range` 参数 | 进程内三档 TTL 缓存 |
| ccSwitch | `~/.cc-switch/cc-switch.db` | 表 `proxy_request_logs`、`usage_daily_rollups` | 全量读，按日聚合 | 文件签名缓存（mtime_ns+size） |

### 3.2 Codex 官方客户端（主来源）

**已验证**：

- home 取自 `Path(os.path.expanduser("~"))`（`client_usage_export.py:13766`），
  sessions 根硬拼 `home/".codex"/"sessions"`（`13822`）。全仓 **不读 `CODEX_HOME`**
  （全仓 grep 零命中），也不读 `%APPDATA%\.codex`。
- **反向索引优先**：`codex_state_rollout_paths`（`2726`）直接打开
  `~/.codex/state_5.sqlite` 或 `~/.codex/sqlite/state_5.sqlite`（`2737-2740`），
  以 `PRAGMA table_info(threads)` 做结构门禁（要求 `id` 与 `rollout_path` 两列，`2752-2762`；
  本机 `~/.codex/state_5.sqlite` 实测确有这两列，见 §C.3 第 1 条），
  SQL 原文 `SELECT id, rollout_path FROM threads WHERE rollout_path IS NOT NULL AND rollout_path != ''`（`2762-2767`）——
  **不带时间条件**，范围完全由 Python 侧三段过滤决定：
  1. 路径必须落在 sessions 根内（`resolved.relative_to(sessions_root)`，失败 `continue`，`2785-2789`）；
  2. 剥 Windows 扩展前缀（`\\?\UNC\`、`\\?\`，`2776-2779`）；
  3. `mtime_ms >= (start-2h) 的毫秒`（`2741-2743` + `2792-2798`）。
- **日期目录兜底**：即使索引可用，仍显式遍历 `sessions/YYYY/MM/DD/*.jsonl`，
  最多 62 个自然日（`2855-2870`），注释说明动机是"新 rollout 可能先于 sqlite 提交"
  （`2855`）。该分支**不按 mtime 过滤**。
- **archive 与"文件已消失"两条补漏**：`sessions` 的兄弟目录 `archived_sessions`，
  mtime ≥ start−2h 才收（`2884-2902`）；以及缓存里记录过、磁盘上已删除的路径
  （`_CODEX_EVENT_ROW_CACHE.cached_missing_paths`，`2903-2907`），注释说明这是为了
  "已完成的子任务从 sessions 与 archive 双双消失后，历史 token 仍留在今日总量"。
- 仅当索引库不可用时才 `root.rglob("*.jsonl")` + mtime 过滤（`2872-2882`）。
- 统一排除路径分量含 `backup-` 前缀或 `.tmp` 的文件（`2822-2824`）；
  同 session_id 已存在可用路径时不重复收（"就地优先"，`2835-2843`）。
- 文件名 UUID 约束 `CODEX_SESSION_FILE_ID_RE`（`475-478`），供父子会话路径反查。
- 会话表头取 `session_meta`：`(payload.id 或 payload.session_id, payload.forked_from_id)`
  （`2636-2645`）；文件按"父先于子"拓扑排序（`2920-2944`）。

### 3.3 Codex `logs_2.sqlite`（第二来源）

**已验证**：`scan_codex_logs2_events`（`3444`）读 `~/.codex/logs_2.sqlite` 的 `logs` 表：

```sql
SELECT ts, feedback_log_body FROM logs
WHERE ts >= ? AND ts < ?
  AND (feedback_log_body LIKE '%response.completed%' OR feedback_log_body LIKE '%service_tier: Some(%')
ORDER BY ts ASC, ts_nanos ASC
```

（`3458-3470`；窗口 = 秒级 `start-300s` 到 `end+300s`。本机 `~/.codex/logs_2.sqlite` 的
`logs` 表实测列名一致 —— `ts:INTEGER`/`ts_nanos:INTEGER`/`feedback_log_body:TEXT`）。正文用
`message = parse_json_after_marker(text, marker)`（`3485`，marker = `"Received message "` @3449）
解出 message，再从 `message["response"]` 取 `response.usage` 的
`input_tokens`/`output_tokens`/`input_tokens_details.cached_tokens`
（`3516`、`3519`、`3543-3545`）；同一 `response.id` 去重（`3513-3515`）；行内
`internal_tier`/`internal_speed` 先预扫描成 `speed_by_request`/`tier_by_request`
再回填到事件（`3479-3484`、`3502-3506`）。时间戳按 `response.completed_at` 二次窗口过滤
（`3539-3540`）。产出事件带 `usage_provenance="codex-logs2"`（`3541-3553`）。

这条来源与 rollout 的关系由 §5.1 的跨来源别名规则桥接。

### 3.4 Codex Desktop 日志（只产失败事件）

**已验证**：`default_codex_desktop_log_roots`（`2949`）先看
`CLIENT_USAGE_CODEX_DESKTOP_LOG_ROOT`（`os.pathsep` 分隔，设置后**完全取代**自动探测），
否则枚举 9 类候选（`%LOCALAPPDATA%\Codex\Logs`、`Programs\Codex\Logs`、
`Packages\OpenAI.Codex_*\LocalCache\...\Logs`、`%APPDATA%\Codex\Logs`、
`%PROGRAMFILES%\WindowsApps\OpenAI.Codex_*\Logs` 等，`2960-3012`）。
`iter_codex_desktop_logs`（`3016`）把本地窗口换算成 UTC 日期目录后 glob
`codex-desktop-*.log`（`3019-3028`）。只解析 `sa_server_request_failed` + `net::ERR_*`
（`DESKTOP_NETWORK_ERROR_CODES` @457），5 分钟一簇、簇内 ≥3 次才产一条
`kind="desktop_network"` 失败事件（`3032-3080`），最终只影响小时标记（§4.8）。

### 3.5 Claude Code

**已验证**：根目录 `~/.claude/projects`（`13923`，历史重建用 `11758`）。

- 发现规则 `iter_recent_claude_jsonl`（`8585`）：`rglob("*.jsonl")` + 排除
  `backup-*`/`.tmp` + `mtime >= start-2h`（`8588-8608`）。
- **没有任何 slug→项目路径的解码**：全文件 `grep slug` 只命中 Codex 缓存行字段（`1993`），
  `grep cwd` 只命中子进程 `cwd=`（`12376`）；`ClaudeUsageEvent`（`717-734`）不含项目字段。
  ⇒「按项目聚合 Claude 用量」在本仓**无实现**（已验证）。
- 解析见 §4.4。

### 3.6 Grok CLI

**已验证**：根目录 `~/.grok/sessions`（`13926`）。发现规则
`iter_recent_grok_updates`（`8985`）只认文件名 `updates.jsonl`，`mtime >= start`
（**没有 Codex/Claude 那样的 −2h 缓冲**，`8988-8994`）。默认模型从
`~/.grok/config.toml` 的 `[models]` 段 `default` 键读取（`8842-8858`）。

### 3.7 Antigravity Cockpit

**已验证**：

| 载体 | 路径 | 用途 |
| --- | --- | --- |
| 请求日志库 | `~/.antigravity_cockpit/codex_local_access_logs.sqlite`，表 `request_logs` | 用量行（`input/output/total/cached_tokens`、`estimated_cost_usd`、`account_id`、`email`、`api_key_label`、`model_id`、`latency_ms`、`request_id`）与归属 marker |
| affinity 日志 | `~/.antigravity_cockpit/logs/codex-api.log*` | 选号/绑定事件（结构化 `auth_result` 行 + 文本正则行） |
| 切换日志 | `~/.antigravity_cockpit/logs/app.log*` | `SWITCH_LOG_RE` 提取 `account_id` 与时间 |
| 账号清单 | `~/.antigravity_cockpit/codex_accounts.json`、`codex_accounts/*.json*` | 账号标签、`plan_type`、`app_speed`、`quota` |
| sidecar 认证 | `.../codex_local_access_sidecar/auths/{account_id}.json` | JWT、`tokens.access_token`、代理配置 |
| 额度 reserve / 备份 | `.../quota-reserve.json`、`backups/cockpit_auto_backup_full_*.json` | 本地额度与回退 |

- SQL 准入谓词 `COCKPIT_RECORDED_USAGE_SQL`（`7448-7453`）：四桶任一 >0；
  主查询无 GROUP BY、全窗口 `fetchall`（`7468-7486`）；毫秒时间戳由
  `local_epoch_ms`/`ms_to_local_datetime` 互转（`3936`/`3940`）。
- 只读连接 `connect_cockpit_sqlite_readonly`（`153-157`）：`file:...?mode=ro`，`timeout=5.0`，
  无 WAL/immutable PRAGMA；失败按 `context:异常类名` 去重告警（`160-165`）。
- affinity 采集 `scan_cockpit_codex_affinity_events`（`8118-8202`）：三层时间过滤
  （文件名日期 → mtime → 起点前推），逐行 JSON 或正则解析，去重键
  `(when, request_id, action, account_id, session_key)`；**无 offset 续读**（`read_text().splitlines()`）。
- switch log 有进程内缓存，键 = `(normcase(path), mtime_ns, size)`（`8210-8221`）。
- 该来源**不走** `CodexEventRowCache` 持久缓存（该缓存被 `_is_default_codex_path`
  限定在 `~/.codex` 下，`2298-2305`）⇒ 每次运行重新查询（已验证）。

### 3.8 OpenCodex 代理

**已验证**：

- 根目录 `opencodex_root`（`4218-4220`）：`CLIENT_USAGE_OPENCODEX_DIR` 或 `~/.opencodex`。
- 数据是 **JSONL 而非 SQLite**：`opencodex_usage_rows`（`4460-4475`）读
  `<root>/usage.jsonl`；只有"home 是真实主目录"或显式设置了该环境变量时才走持久缓存。
- 来源实例标识 `opencodex_source_instance`（`4223-4228`）= 根路径 sha256 前 12 位，
  用于隔离不同根下相同的 `requestId`。
- 账号映射读 `<root>/config.json*`（glob，按 `mtime_ns` 降序，`4326-4334`），
  取 `codexAccounts[].logLabel/email/planType`，跳过 `main` 与非可用邮箱
  （`usable_cockpit_identity(email)`，`4350`）；**同一 logLabel 命中多个账号时整条丢弃**
  （`4361-4363`），输出 `Codex local - <email>`；持久副本
  `client_usage_opencodex_accounts.json`（schema 1，写前持 ledger 锁，`4412-4450`；
  与持久层冲突时同样 pop，`4407-4408`）。
- 匿名兜底只接受 `p[0-9a-f]{6}` 形态（`4453-4457`）。
- **`main` 路由不使用 `~/.opencodex/auth.json`**：注释明确说明 `main` 用
  `~/.codex/auth.json` 的凭据执行，因此账号快照只读后者（`4231-4246`）。

### 3.9 Sub2API 服务端

**已验证**（客户端侧反推；**本节行号均指 `monitor.py`**）：`Sub2APIClient`（`5693`）配置优先级
"进程环境变量 > `.env` 文件 > 默认"（`5696-5708`；`.env` 搜索路径 `98-107`）。
调用面：

| 端点 | 方法 | 关键参数 | 取用字段 |
| --- | --- | --- | --- |
| `/api/v1/auth/login` | POST | `{email,password}` | `access_token`；`requires_2fa` 直接报错（`5905-5919`） |
| `/api/v1/admin/dashboard/stats` | GET | — | `today_requests/today_tokens/today_actual_cost`（字段取用在 `6153-6155`；`5963` 只发请求） |
| `/api/v1/admin/accounts` | GET | `page=1&page_size=1000&platform=openai&sort_by=priority&sort_order=asc` | `items[]`（`5964-5970`） |
| `/api/v1/admin/ops/concurrency` | GET | `platform=openai` | `account` 映射（`5973-5976`） |
| `/api/v1/admin/ops/requests` | GET | `time_range=30d&kind=all&platform=openai&page=1&page_size=1&sort=created_at_desc` | `items[0]`（`5979-5993`） |
| `/api/v1/admin/dashboard/trend` | GET | `start_date=近7天&granularity=day&timezone=Asia/Shanghai` | `trend[]`（`5892-5903`） |
| `/api/v1/admin/accounts/today-stats/batch` | POST | `{"account_ids":[...]}` | `stats[str(id)]`（`6004-6011`） |
| `/api/v1/admin/accounts/{id}/usage` | GET | — | `five_hour/seven_day/cycle`（`5805-5810`） |
| `/api/v1/admin/accounts/{id}/stats` | GET | `days=30` | `summary.total_requests/total_tokens/total_cost`（`5825-5837`） |

请求超时固定 10 s（`5874`），401 自动重登一次（`5877-5879`）；
平台过滤恒为 `openai`；30 天账号统计用 `ThreadPoolExecutor(max_workers=min(8,n))`（`5847`）。
进程内缓存 TTL：客户端用量 10 s、账号窗口 60 s、账号 30 天 300 s
（`110-113`）。**endpoint 门禁** `same_endpoint`（`544-562`）只比 hostname（小写）与端口，
本机别名互通（`LOCAL_HOSTS` @362）；三态 `True/False/None`（`5729-5737`）决定
`auto` 模式走服务端还是本地日志（`5921-5956`）。

### 3.10 ccSwitch（只在分析网页里回填）

**已验证**：全仓 `cc-switch|ccSwitch|cc_switch` 命中分布在 `analytics_server.py`（读取实现）、
其前端页面 `analytics/index.html`、`README.md:253` 与 `tests/test_analytics_server.py`
——**读取实现只在分析服务**，导出器（`client_usage_export.py`）与窗口进程（`monitor.py`）
零命中。路径 `CCSWITCH_DB`（`analytics_server.py:45-49`）：`TOKEN_PULSE_CCSWITCH_DB` /
`CCSWITCH_DB` / `~/.cc-switch/cc-switch.db`；`mode=ro`、`timeout=2`（`248`）。
两张表：

- `proxy_request_logs`（LEFT JOIN `providers`，按 `date(created_at,'unixepoch','+8 hours')`
  分组，`250-271`；本机 `cc-switch.db` 实测三张表的列名与 join 条件全部吻合，
  且 `total_cost_usd` 为 **TEXT** ⇒ 代码的 `CAST(... AS REAL)` 是必要而非冗余）；
- `usage_daily_rollups`（只补前者缺失的 `(date, app, provider, model)` 组合，`303-306`）。

回填规则两级：表级"rollups 只在 proxy 无同组合时采用"（`305-306`）；
日级"ccSwitch 只补主历史**缺失日期**，主历史显式 0 优先"（`358-362`）。
缓存签名 = 两文件 `(path, mtime_ns, size)`（`345-348`、`96-101`）。
测试锁定行为：`tests/test_analytics_server.py:97-100`（主历史 0 压住 ccSwitch 的 900）。

### 3.11 增量缓存与原子写

**已验证**：

- 缓存文件：`client_usage_codex_event_cache.json`（env `CLIENT_USAGE_CODEX_EVENT_CACHE`）与
  `client_usage_opencodex_usage_cache.json`（env `CLIENT_USAGE_OPENCODEX_USAGE_CACHE`）
  （`186-193`）。同一变量 `CLIENT_USAGE_CODEX_EVENT_CACHE` 身兼两职：既覆盖文件路径，
  又作为"强制持久化任意路径"的布尔开关（`2298-2305`）。
- 行体只保留统计所需字段（`compact_codex_cache_row` @2061 的注释：
  "never cache conversation text"），序列化为 `base64(zlib(json))`；
  加载与合并两处都做同一道门禁 `payload["schema"] != CODEX_EVENT_CACHE_SCHEMA` → 整体弃用
  （`2323-2324` 的 `_load`、`2561-2562` 的 `_merge_entries_from_disk`；schema 常量 `288-290`，
  flush 的 payload 组装在 `2589-2600`、`os.replace` 写盘在 `2601`）。
- 失效四态 `rows_for_path`（`2444-2505`）+ 锚点校验（`2395-2404`）：
  schema 过期 → 全量；`size+mtime_ns` 一致 → 命中；`size` 增长且 head/boundary 哈希一致 →
  从 `processed_length` 偏移续读；其余（含截断）→ 从 0 全量。
  残行不消费（无 `\n` 结尾的行回退到行首等待下次追加，`2425-2434`）。
- 原子写三件套：`write_json_atomic`（`34-44`，`os.replace`）、
  `recover_corrupt_json`（`105-127`，改名 `.corrupt-<时间戳>` 后从 `.bak` 恢复）、
  `refresh_json_backup`（`130-147`，默认 1 h 才刷新备份）。
- 刷写持跨进程锁 `attribution_ledger_write_lock`（Windows 命名 mutex / POSIX flock，`220-285`），
  合并磁盘条目后按 `CODEX_EVENT_CACHE_MAX_ENTRIES`（默认 4096）截断（`2540-2611`），
  `atexit` 注册 flush（`2626-2629`）。
- 沙箱实测：第二次运行 0.56 s（缓存命中），追加一条事件后 0.57 s，缓存文件随之更新
  （附录 A.4）。

### 3.12 来源目录与边界规则小结

- **不做目录白名单校验之外的越权读取**：Token Pulse 只读上述固定路径与 env 覆盖路径。
- **时间缓冲不对称**（已验证）：Codex sessions/archived 与 Claude 用 `start−2h`，
  Grok 用 `start`，桌面日志按 UTC 日目录，Cockpit 用毫秒区间，OpenCodex 用 `[start−6h, end+6h]`。
  这不是遗漏，而是各来源写入延迟特征不同（Grok 每次 turn 重写 `updates.jsonl`，
  文件 mtime 天然新）。
- **同一文件跨来源认领**：没有任何"先扫描者得"逻辑；rollout 与 logs_2 的同一物理请求
  由 §5.1 的 alias 规则合并。

---

## 4. 解析层：从原始行到统一用量事件

### 4.1 统一数据结构

**`UsageBucket`（`client_usage_export.py:546-601`）是唯一的计账单元**，同时容纳 Codex 三桶
与 Claude 两桶：

```python
@dataclass
class UsageBucket:
    requests: int = 0
    input_tokens: int = 0            # 非缓存输入
    cached_input_tokens: int = 0     # Codex 缓存读
    output_tokens: int = 0
    cache_creation_input_tokens: int = 0
    cache_read_input_tokens: int = 0 # Claude 缓存读
    cost: float = 0.0
    models: dict[str, int] = field(default_factory=dict)
    unpriced_tokens: int = 0
    unpriced_models: dict[str, int] = field(default_factory=dict)
    latest_at: datetime | None = None
    latest_model: str = ""
    latest_app_speed: str = ""
    latest_cost_multiplier: float | None = None
    latest_speed_badge: str = ""
```

`total_tokens` 把五桶全加（`564-572`）；`add_model` 空名归一 `"unknown"`（`574-576`）；
`add_unpriced_model` 只在 token>0 时累计（`578-584`）；`mark_latest` 只在时间前进时改写
`latest_*`（`586-601`）。

**`UsageEvent`（`604-632`）** 是 Codex / Grok / OpenCodex 共用的解析产物：

| 字段 | 语义 |
| --- | --- |
| `when` | 本地 naive 事件时间（`LOCAL_TZ` 换算后剥 tzinfo） |
| `input_tokens` / `cached_tokens` / `output_tokens` | **`input_tokens` 已在 `make_codex_event` 里减掉 cached**（非缓存输入） |
| `app_speed` / `cost_multiplier` / `pricing_tier` | 速度档与计价档（Codex 专有） |
| `session_id` / `conversation_id` / `request_key` / `source_request_key` | 会话与请求身份（三级） |
| `route` | `official` / `sub2api` / `grok-local` / `opencodex`（`4713`） |
| `request_at` / `account_at` | 请求时刻与**轮次起点**（账号归属的时间基准） |
| `quota_fingerprints` | `(window_minutes, resets_at)` 集合，额度窗口指纹 |
| `pricing_model` | 计价键覆盖（Grok/OpenCodex 用） |
| `usage_provenance` | `codex-rollout` / `codex-logs2` / `opencodex_usage` |
| `canonical_id` / `supersedes_event_ids` / `reconciliation_status` | 由 OpenCodex 对账层回填 |
| `account_label_hint` / `account_hint_source` | 归属提示与来源（`opencodex_usage` / `opencodex_turn` / `quota_fingerprint` 等） |

注意 `UsageEvent.total_tokens` 只含三桶（`630-632`），与 `UsageBucket` 的五桶口径**不同**——
前者是"一次模型调用"的物理量，后者是"一个统计单元"的合计（一致解释）。

其余类型：`ClaudeUsageEvent`（`717-734`，四字段：input/output/cache_creation/cache_read）、
`SessionLifecycle`（`756-762`）、`CodexFailureEvent`（`765-771`，`kind` 默认 `"task"`）、
`AccountMarker`（`773-786`）、`CockpitAffinityEvent`（`789-800`）、
`CockpitAffinitySegment`（`802-813`）、`SpeedMarker`（`816-819`）、`RouteMarker`（`822-827`）、
`OpenCodexUsageMarker`（`642-671`）、`OpenCodexReconciliationDiagnostics`（`674-695`）。

身份函数 `live_usage_event_id`（`737-753`）：有 `canonical_id` 用它，否则
`session_id|微秒时间戳|(input+cached)|cached|output`（第三段由实现自行把两桶相加）。
`monitor.py:2575-2592` 有另一份**结构不同**的实现：无 `canonical_id` 短路、回退 `CN_TZ`、
第三段直接取 `input_tokens`（要求调用方预先求和 input+cached）；两者只有在调用侧按该约定
传参时才产出同一字符串，测试 `tests/test_usage_state_boundaries.py:13370-13384` 覆盖的正是这一等价调用（已验证）。

### 4.2 Codex rollout JSONL 解析

**进缓存的白名单**（`compact_codex_cache_row`，`2061-2146`）：`session_meta`（只留
`id/session_id/parent_thread_id/forked_from_id/timestamp`）、`turn_context`（`model`）、
以及 payload.type ∈ `CODEX_RELEVANT_EVENT_TYPES`（`2048-2058`：`error`、`task_complete`、
`task_started`、`token_count`、`turn_aborted`、`turn_complete`、`turn_started`）的 `event_msg`。
`token_count` 只留 `info.total_token_usage` / `info.last_token_usage` 的
`CODEX_TOKEN_USAGE_FIELDS` 五字段（`2040-2046`）与 `rate_limits` 的
`plan_type` + 每窗口 `used_percent/window_minutes/resets_at`（`2099-2130`）。
函数文档串明确："Keep only fields needed by the usage scanner; **never cache conversation text**"。
落盘时行体序列化为 `base64(zlib(json))`：编码 `2361-2371`
（`base64.b64encode(zlib.compress(raw, level=6))`）、解码 `2341-2351`；
`288-298` 是缓存 schema 与容量常量，`2589-2601` 是 flush 的 payload 组装与写盘。

**签名**：`codex_token_count_signature`（`2675-2693`）生成 11 元组——
`[0]`=本行是否有 `last_token_usage`，`[1:6]`=`total_token_usage` 五字段，
`[6:11]`=`last_token_usage` 五字段。用于行去重、累计点去重、fork 前缀比对。

**逐行主循环**（`scan_codex_events`，`3082-3360`）：

- `turn_context` 行只推进 `current_model`（`3136-3140`）。
- 非 `event_msg` 行直接跳过（`3141-3142`）；生命周期事件只写 `session_lifecycle`（`3145-3161`）；
  `task_started/turn_started` 记录 `active_turn_id` 与 `active_turn_started_at`（`3162-3167`，
  后者成为事件的 `account_at`）。
- `token_count` 行是唯一出事件的类型（`3207` 起）：
  - **有 `last_token_usage`**（`signature[0] == 1`）→ 三桶直接取 signature[6..8]（`3280-3283`），
    不做差分；
  - **无** → `delta = current − last_total`，任一维为负则整行回退为 current 全量（覆盖计数器重置，
    `3311-3317`）；三桶全 ≤0 则跳过（`3318-3319`）。
   - 无论哪条**出事件**的路径末尾都 `last_total = current`（`3309`、`3345`）；
     唯一例外是"三桶全 ≤0"的跳过路径（`3318-3319`，`continue` 前不更新）。
- **范围过滤**：`ts is None or ts < start` → 只更新 `last_total` 后跳过（`3248-3250`）；
  `ts >= end` → 跳过且**不**更新（`3251-3252`）。
- **模型名优先级**：行内 `row.model` / `payload.model` → `current_model`（最近 turn_context）
  → `CODEX_DEFAULT_MODEL`（默认 `gpt-5.5`，`359`）（`3253-3254`、`1634-1638`）。
- `reasoning_output_tokens` **只进去重键**，不进任何 token 桶（`3218`、`3234-3239`）。

**一行 rollout → 一个事件（字段映射表，已验证）**

| 原始 JSON 表达式 | 事件字段 | 说明 |
| --- | --- | --- |
| `row.type=="event_msg"` ∧ `payload.type=="token_count"` | 触发条件 | `3133-3207` |
| `row.timestamp`（ISO，可带 `Z`） | `when` | `parse_dt` → 本地 naive（`3144`、`3209`） |
| `payload.info.last_token_usage.{input,cached_input,output}_tokens` | 三桶 | 仅当该行带 `last_token_usage`（`3280-3283`） |
| `payload.info.total_token_usage.{input,cached_input,output}_tokens` | 三桶（差分源） | 无 last 时差分（`3311-3317`） |
| `payload.info.total_token_usage.reasoning_output_tokens` | 仅去重键 | `3218`、`3234-3239` |
| `payload.info.total_token_usage.total_tokens` | 仅签名第 5 位 | `2687`（全仓无 `signature[5]` 读取点） |
| `row.model` / `payload.model` / 最近 `turn_context.model` | `model` | `3253-3254`、`3136-3139` |
| `session_meta.payload.id` 或 `.session_id` | `session_id` | `2636-2645` |
| `session_meta.payload.{session_id,parent_thread_id,id}` 的 sha256[:32]（优先序即此，`2653-2658`） | `conversation_id` | `2648-2661` |
| 当前轮 `task_started`/`turn_started` 的 `row.timestamp` | `account_at` | `3162-3166` → `3303` |
| `payload.rate_limits.*.{window_minutes,resets_at}` | `quota_fingerprints` | `3210`、`1901-1922` |
| 常量 `"codex-rollout"` | `usage_provenance` | `3305` |
| `session_id` | `request_key` | `3302` |

**实证**：沙箱会话 A（两条累计行 1000/200/100 → 3000/600/400）产出 2 个事件，
落桶后 `input=2400, cached=600, out=400`，成本 0.0243（附录 A.2 手算一致）。

### 4.3 Codex `logs_2.sqlite` 解析

`scan_codex_logs2_events`（`3444-3560`）：从 `feedback_log_body` 文本里抓
`Received message ` 之后的 JSON，取 `response.usage`：

```python
        response = message.get("response") or {}
        if not isinstance(response, dict):
            continue
        response_id = str(response.get("id") or "")
        if response_id and response_id in seen_response_ids:
            continue
        usage = response.get("usage") or {}
        if not isinstance(usage, dict):
            continue
        details = usage.get("input_tokens_details") or {}
```
（`3510-3519`，含 3519 的 `details`）。三桶取 `usage.input_tokens` / `details.cached_tokens` /
`usage.output_tokens`（`3543-3545`）；`when` 按 `completed_at` → `created_at` → 行 `ts`
三级回退（`3522-3526`）；二次窗口过滤 `when < start or when >= end`
（`3539-3540`）；`route` 用 `detect_codex_route(text)`；`usage_provenance="codex-logs2"`（`3553`）。
另有文本正则路径 `codex_event_from_log_fields`（`1976-2006`），要求行含
`event.kind=response.completed` 与 `input_token_count=`，字段名
`input_token_count`/`cached_token_count`/`output_token_count`（`1977-1982`）。

### 4.4 Claude Code 解析

**行准入**：`message.role == "assistant"` 且 `message.usage` 是非空 dict，且 `row.timestamp`
可解析（`8611-8620`）。

**token 映射**（`8622-8633`）：

```python
    input_tokens = claude_usage_int(usage, "input_tokens")
    output_tokens = claude_usage_int(usage, "output_tokens")
    cache_creation = claude_usage_int(usage, "cache_creation_input_tokens")
    if cache_creation <= 0 and isinstance(usage.get("cache_creation"), dict):
        cache_creation = sum(
            claude_usage_int(usage["cache_creation"], key)
            for key in ("ephemeral_5m_input_tokens", "ephemeral_1h_input_tokens")
        )
    cache_read = claude_usage_int(usage, "cache_read_input_tokens")
    total = input_tokens + output_tokens + cache_creation + cache_read
    if total <= 0 or total > MAX_SINGLE_EVENT_TOKENS:
        return None
```

⚠️ **口径差异（本文最重要的跨来源发现之一）**：Claude 的 `input_tokens`
**不减去** `cache_read`/`cache_creation`（Anthropic 语义里三者本就独立），而 Codex/Grok 的
`input_tokens` 已经减过 cached。两者在 `today.input_tokens` 里相加，因此该字段是
"未缓存输入 + Claude 全量输入"的混合口径；只有 `cached_input_tokens`
（`bucket_to_dict` 把 `cached_input_tokens + cache_read_input_tokens` 合并输出，`12924`）
与 `cache_creation_input_tokens` 才是可跨来源比较的。**已验证**（源码 + 沙箱实测 11520
= Codex 8800 + Claude 2120 + Grok 600，附录 A.3）。

**合并**：`event_id` 三级（`message:<id>` → `row:<session>:<uuid>` → `line:<sha256>`，`8635-8643`）；
按 `event_id` 分组，`claude_event_snapshot_score` = `(total_tokens, 非零桶数, when)` 字典序取大
（`8660-8670`、`8704-8711`）；分组时间取**最早** `first_when`，以它做范围过滤与排序
（`8713-8729`）。这正是 README "按 thinking/text/tool-use 重复写入 → 保留最完整快照"的实现。

**sidechain / 子代理**：`sidechain`（含 `isSidechain`）全仓**零命中** ⇒ Claude 解析层完全忽略
子代理字段（`claude_event_from_row` `8611-8657` 与 `scan_claude_events` `8673-8730` 都不读
任何子代理键，`ClaudeUsageEvent` 也无对应字段，已验证）。注意 `subagent` 一词另有约 28 处命中，
那是 Grok/OpenCode 的**展示标签**（`366-372`、`1655-1660`、`monitor.py:1024-1025`）与测试引用，
与 Claude 无关。

### 4.5 Grok CLI 解析

`grok_usage_events_from_update_row`（`8876-8982`）：

- 准入：`params.update.sessionUpdate == "turn_completed"` 且 `update.usage` 是 dict（`8882-8890`）。
- `prompt_id` 缺失时用 `sha256({session_id, timestamp, usage})` 前加 `fallback-`（`8896-8910`）。
- 用量分布在 `usage.modelUsage`（按模型分组）；缺失时退化为单模型（`8911-8919`）。
- **token 反推**（`8928-8943`）：

```python
        output_tokens = min(output_tokens, total_tokens)
        input_tokens = max(0, total_tokens - output_tokens)
        cache_read = max(0, usage_int(raw_detail, "cachedReadTokens"))
        cache_creation = max(0, usage_int(raw_detail, "cacheCreationTokens"))
        if cache_read <= 0 and cache_creation <= 0:
            cache_read = max(0, usage_int(raw_detail, "cachedTokens"))
        cached_tokens = min(input_tokens, cache_read + cache_creation)
        uncached_input = max(0, input_tokens - cached_tokens)
```

- **一次 turn 拆多次调用**：`modelCalls` 条事件，token 用 `split_grok_token_count`
  均分且余数前置（`8828-8832`、`8950-8981`），时间每条 +1 微秒，`request_key` =
  `grok:{session}:{prompt_id}:{model}:{call_index}`（`8972-8974`）。
- 模型归一 `grok_canonical_model_name`（`8835-8839`，只做小写 + 剥 `xai/`）；
  计价键 `grok_local_pricing_model`（`8862-8873`）——仅当用量模型是 `grok-4.6-build`
  且 `~/.grok/config.toml` 默认模型 ∈ {`grok-4.6`,`xai/grok-4.6`} 时返回 `xai/grok-4.6`。
- 去重：`scan_grok_events`（`9004-9039`）按 `request_key or live_usage_event_id` 分组，
  取 token 总量大者、时间取最早。
- **批处理侧无单事件上限过滤**：`client_usage_export.py` 的 Grok 路径不检查
  `MAX_SINGLE_EVENT_TOKENS`（全仓该常量只在 `1741`/`1789`/`8632` 三处）；但 `monitor.py` 的
  实时链路用**同一 env** 定义 `LIVE_USAGE_MAX_SINGLE_EVENT_TOKENS`（`monitor.py:179-182`）并在
  `13471-13472` 过滤，Grok live 事件经 `12702`→`12707` 汇聚后同样受检（一致解释 → 已验证）。

### 4.6 Cockpit request_logs 解析

`scan_cockpit_codex_accounts`（`7456-7529`）用准入谓词 `COCKPIT_RECORDED_USAGE_SQL`
（四桶任一 >0，`7448-7453`）与毫秒区间查询（`7468-7486`），字段
`timestamp/account_id/email/api_key_label/model_id/input_tokens/output_tokens/total_tokens/cached_tokens/estimated_cost_usd`。
入桶走 `add_cockpit_usage_to_bucket`（`7402-7445`）：

```python
    multiplier = max(1.0, cost_multiplier)
    calculated_cost, _price_resolved = estimate_cost_with_resolution(
        model,
        max(0, input_tokens - cached_tokens),
        cached_tokens,
        output_tokens,
        pricing_tier="priority" if multiplier > 1 else "standard",
    )
    upstream_cost = max(0.0, cost * multiplier)
    effective_cost = calculated_cost if calculated_cost > 0 else upstream_cost
```

即：**优先用本地价格重算，重算不出（0）才采信上游 `estimated_cost_usd`**，
`×multiplier` 校正 speed 倍率；两者都为 0 则记 unpriced（`7430-7443`）。

### 4.7 异常、坏行与失败事件

| 机制 | 行为 | 位置 |
| --- | --- | --- |
| 单事件 token 上限 | `MAX_SINGLE_EVENT_TOKENS`（默认 2 000 000，env `CLIENT_USAGE_MAX_SINGLE_EVENT_TOKENS`）在 `make_codex_event:1789`、`add_codex_usage:1741`、`claude_event_from_row:8632` 三处过滤；**批处理侧 Grok 路径没有**（`monitor.py` 实时链路另有同名上限，见 §4.5） | `client_usage_export.py:360` |
| 未写完的行 | JSON 解析失败且行尾无 `\n` → 回退 `processed = line_start` 等下次追加；完整坏行跳过 | `2425-2434` |
| 非 dict 行 | 跳过（`continue`） | `2435-2436` |
| 目录过滤 | 路径分量含 `backup-*` 前缀或 `.tmp` → 整文件跳过 | `2822-2824` |
| 失败事件 | `error` / `turn_complete(error)` 登记 `CodexFailureEvent`；`NON_TURN_ERROR_KINDS = {"active_turn_not_steerable","thread_rollback_failed"}` 不算轮次失败 | `1694-1718`、`3168-3196` |
| 失败事件的作用面 | **只影响小时标记**：`mark_codex_failure_hours`（`9080-9141`）写 `failure/failure_count/failure_at/failure_kind`；空小时的 `desktop_network` 失败被显式丢弃；**同一小时内、失败时点之后**仍有活动则不打标（`9130-9134`） | `9080-9141`、`13939-13945` |
| 桌面网络失败 | `sa_server_request_failed` + `net::ERR_*`，5 分钟一簇、簇内 ≥3 次 | `3032-3080`、`457-465` |

### 4.8 时间与时区

- `LOCAL_TZ = timezone(timedelta(hours=8))`（`453`），**固定 UTC+8，无 DST**。
- `parse_dt`（`1863-1873`）：`Z`→`+00:00`，带 tz 的值 `astimezone(LOCAL_TZ)` 后**剥掉 tzinfo**；
  `epoch_to_local_datetime`（`1876-1886`）、`ms_to_local_datetime`（`3940-3947`）的时区换算语义相同，
  但**异常兜底不对称**：epoch 版对 `fromtimestamp` 有 `except (OSError, OverflowError, ValueError)`，
  ms 版没有（只有 `int()` 的 `TypeError/ValueError`）。
- 窗口一律左闭右开：`start = day 00:00`、`end = start + 1 天`、当天 `scan_end = min(now, end)`
  （`13814-13820`）；所有扫描比较都是 `start <= t < end`。
- 日桶按本地 `date`：`scan_claude_daily_buckets`（`11645-11654`）、
  `scan_grok_daily_buckets`（`9050-9059`）。
- 跨午夜保护：`scan_all_codex_events` 给 OpenCodex 对账窗口两侧各加 6 小时 padding，
  并把失败事件按 `[start, end)` 回裁（`3824-3875`），注释说明动机是"午夜后迟到写入的本地行
  不应改写前一天的代理请求"。

---

## 5. 去重、对账与账号归属

这是本仓库最复杂、也最值得借鉴的部分。四个机制按流水线出现：
**rollout 内 fork 剔除 → 通用去重（含跨来源 alias）→ OpenCodex 双向对账 → 账号归属（Cockpit affinity / ledger）**。

### 5.1 三层身份键与通用去重

**三层身份**（`3563-3598`）：

```python
def usage_event_strong_identity_keys(event):      # canonical_id / request_key / source_request_key
def usage_event_physical_identity_keys(event):    # 强身份去掉"request_key == session/conversation"的伪区分
def usage_event_weak_identity_keys(event):        # conversation_id / session_id / sha256(session_id)[:32]
```

**跨来源 alias**（`3613-3643`）是唯一允许"身份键不相等仍合并"的通道，四个条件缺一不可：
签名（模型 + 三桶 + 总量，`opencodex_event_signature` @4607）相同、时间差 ≤1.0 s、
provenance 恰为 `{codex-rollout, codex-logs2}`、弱身份有交集；此后若**强身份也有交集**
直接判定别名（`3631-3632`），否则才要求**只有一端把 session 当 request_key**：

```python
    return bool(
        (first_key and first_key == first_session and second_key != second_session)
        or (second_key and second_key == second_session and first_key != first_session)
    )
```
（`3640-3643`，注释："Two distinct non-session request ids remain separate concurrent requests."）

**`dedupe_usage_events`（`3646-3708`）**：按 `(model, input, cached, output)` 分桶
（时间不参与分桶），按 `when` 升序处理；`duplicate_indexes = alias_indexes or identity_indexes`
（**alias 优先**）；identity 路径要求**同一整数秒** + 物理身份交集，或 `when` 完全相等 + 共享身份；
命中后按 `usage_event_info_score`（`3783-3801`）取分高者，alias 路径则 `codex-logs2` 无条件胜出
（`3699-3702`）；alias 命中会把旧索引记入 `cross_source_matched_indexes`，实现 **1:1 配对**
（防止"多对一塌缩"，测试 `tests/test_hybrid_usage_reconciliation.py:799-822`）。

**打分权重**（`3783-3801`）：`quota_fingerprints` 16、`route` 8、`session_id` 4、
`conversation_id` 2、`request_key` 2、`cost_multiplier`/`request_at`/`account_at` 各 1。
设计意图：配额指纹与路由之和（24）大于其余全部（11）。

### 5.2 Codex fork replay 去重

**判据**：`session_meta.payload.forked_from_id` 非空即视为 fork；cutoff =
该 `session_meta.timestamp + 2 s`（`2664-2672`；文本版 `2025-2037` 无调用点）。

**三重过滤**（均在 `scan_codex_events` 内，`3117-3279`）：

1. **父签名前缀继承**：父会话文件若也被本次扫描发现（经 `session_paths` 按文件名 UUID 反查），
   取父会话的 token_count 签名序列；当前会话签名与之**逐位单调比较**，命中即
   `inherited_replay=True` 且不做事件、只推进 `last_total`；一旦不匹配则永久
   `parent_prefix_open = False`（`3219-3247`）。
2. **cutoff 兜底**：`parent_prefix_index == 0`（父前缀完全没匹配上）且
   `ts <= fork_replay_cutoff` → 记入 `seen_totals` 后跳过（`3262-3270`）。
3. **跨会话累计值去重**：`total_key = (model, input, cached, output, reasoning)` 已见过时，
   只有"确认过的 fork（`parent_prefix_index > 0`）且签名已知"才允许并存，否则跳过
   （`3271-3279`，注释："Only confirmed forks may contain distinct branch requests at one cumulative total."）。

**降级行为（一致解释）**：若父会话文件早于扫描窗而未被发现，第 1 重失效，仅剩第 2、3 重兜底。
README 第 30 行的"保留 Codex fork replay 去重"语义成立，但作用点是 rollout 扫描内部，
不在 `dedupe_usage_events` 阶段。

**实证**：沙箱会话 C（fork A，重放 A 两条累计行 + 一条新行 9000/2000/1500）最终只计入
`delta = 9000−3000 = 6000`（input），即 `input=4600, cached=1400, out=1100`，
与"重放被剔除、只算新尾部"完全一致（附录 A.2）。

### 5.3 OpenCodex 双向对账

**解决什么**：同一次请求可能同时被本地 Codex 记录（rollout/logs_2）与 OpenCodex 代理记录
（`usage.jsonl`）。要同时做到配对、补漏（代理有本地无）、供账号证据。

**三级匹配**（`reconcile_opencodex_usage_events`，`4830-5007`）：

| 级 | 条件 | 窗口 | 备注 |
| --- | --- | --- | --- |
| 1 | `request_key`/`source_request_key` == `marker.request_id` | `[−60 s, +21600 s]` | 排序优先"签名相等"，再比时间距离；显式排除 `route == "official"` |
| 2 | 签名相等 + `conversation_id` 相等 | 21600 s | 端点双向歧义（≤0.05 s）→ 双方都标 `ambiguous`；**配对仍然发生并合并为 canonical 事件**，只是状态记 `ambiguous` 并单独计数（`4926-4930`） |
| 3 | 签名相等 + 两侧 conversation 不同时存在（`4792-4794`）+ marker 有路由证据 | 30 s | 要求双向唯一；event 侧 conversation 由 `opencodex_conversation_id_for_event` 兜底生成，故常态是 marker 侧为空 |

**冲突规则**（`4844-4859`）：同键 marker 保留时间最新者；同键签名不同 → 进入 `conflict_keys`，
之后无论配对还是 `proxy_only` 都标 `conflict`；配对成功后签名二次校验不一致也标 `conflict`
（`4902-4907`）。**合并出口** `opencodex_canonical_usage_event`（`4679-4758`）：
**三桶以代理侧为准**（`4728-4730`），时间取代理完成时刻（`4726`），`canonical_id` 成为新身份，
原事件 id 记入 `supersedes_event_ids`。

**补漏与剔除**：未配对的 marker 若通过 `opencodex_proxy_rejection_reason`（`4664-4676`，
只剔除"非 `responses` 入站协议"与"无会话的健康检查/模型探针"）则补成 `proxy_only` 事件，
否则标 `rejected` 且**不计 token**（`4971-4982`）。

**诊断**：`OpenCodexReconciliationDiagnostics`（`674-695`）固定六状态
`matched / proxy_only / local_only / ambiguous / conflict / rejected`，请求数与 token 数各一份；
结果先写入进程级变量（`3867-3869`，读取函数 `3807-3811`），最终出现在输出
`dashboard.reconciliation`（`14122-14125`）。沙箱实测：无任何 marker 时 `local_only = 5`、
tokens 13120（附录 A.3）。

**账号 hint 二次传播**（`apply_opencodex_account_hints`，`5010-5178`）三轮：
① 签名 + 会话窗口配对写 `opencodex_usage` hint（标签为空则降级为聚合标签 +
`opencodex_usage_unresolved`）；② 同会话路由确认（模型一致 + 完成时刻 ≤30 s，或
turn 起点与 `marker.request_at` ≤5 s）仍标 unresolved；③ 同 turn 内传播
（`opencodex_turn`，`bisect_right(...) - 1` 取最近锚）。

### 5.4 Cockpit affinity 与 turn anchor

**动作白名单与失败片段**（`513-537`）：`COCKPIT_CONFIRMED_AFFINITY_ACTIONS`
（`binding confirmed` / `spillover binding confirmed` / `confirmed binding hit`）、
`COCKPIT_STABLE_NATIVE_AFFINITY_ACTIONS`（5 条 cache/binding 相关）、
`COCKPIT_FAILED_AFFINITY_ACTION_FRAGMENTS`（6 条子串：`released after failure`、
`suspended for failover`、`auth unavailable`、`reselected`、`failure recovery`、`quarantined auth`）。

**失败路由过滤**是子串匹配（`7853-7857`），随后三处收紧：
`cockpit_affinity_event_concrete_account_id`（`7860-7872`，失败事件直接返回空账号）、
`cockpit_affinity_segment_native_account_ids`（`7897-7908`）、
`cockpit_affinity_segment_route_is_trusted`（`7911-7924`，失败段一律不信任；失败后新段
只认"原生执行会话 + 稳定动作"；正常情况下段内 account_id 必须唯一）。

**分段算法**（`7927-7993`）解决"同一 `request_id` 被复用于多次选号"：失败事件在
"当前段已有可用路由"时**封段**并开 `started_after_failure=True` 新段；
排序键 `(when, action, account_id)`。

**`auth_result` 确认条件**（`7829-7849`）：`success is True and authAvailable is not False`。

**七个 anchor 函数**（策略表，已验证）：

| 函数 | 行号 | 窗口 | 判据 |
| --- | --- | --- | --- |
| `cockpit_request_start_turn_anchors` | 9576-9687 | 2.0 s | marker 请求开始时间（`when − latency_ms`）对 turn start；须模型一致 + 被 `account_marker_covers_event_time` 覆盖；歧义 0.2 s |
| `cockpit_near_time_turn_anchors` | 9690-9764 | 2.0 s | token 总量不等时的时序回退；须唯一；次优距离差 <0.2 s 放弃 |
| `cockpit_affinity_turn_anchors` | 9767-9854 | 段约束 | 依赖 `cockpit_account_markers_by_segment` 的段绑定 |
| `cockpit_nearest_turn_start_affinity_anchors` | 9992-10127 | 0.5 s | 稳定原生白名单动作 + `execution_session_id` 前缀来源 + 段内同账号 marker 验证 |
| `cockpit_final_request_event_markers` | 10130-10251 | 0.5 s | 权威终局 usage 行；段先绑 turn，再由段内 marker 候选投票 |
| `cockpit_confirmed_auth_result_event_markers` | 10253-10339 | 0.35 s | 仅结构化 `auth_result` + confirmed + 账号可用；歧义阈值 **0.01 s**（最严） |
| `cockpit_consistent_temporal_affinity_turn_anchors` | 10342-10678 | 0.25–0.35 s | 唯一配对的事件级 anchor |

窗口常量集中在 `410-433`（`CLIENT_USAGE_COCKPIT_*` 系列，含
`AFFINITY_TURN_MATCH_SECONDS=2.0`、`AFFINITY_TURN_AMBIGUITY_SECONDS=0.2`、
`AFFINITY_EVENT_MATCH_SECONDS=0.25`、`FINAL_TURN_START_MATCH_SECONDS=0.5`、
`AFFINITY_MIN_STABLE_EVENTS=2`、`FALLBACK_GRACE_SECONDS=45.0`）。

### 5.5 账号归属：时间线、决策链、ledger

**两类 marker**：`kind="switch"`（账号切换时间线）与 `kind="request"`（Cockpit 请求行）。
`load_account_timeline`（`4136-4175`）合并 `client_usage_account_timeline.json` 与
`client_usage_auth_switch_events.jsonl`；`record_account_timeline_snapshot`（`4178-4215`）
有三重夹取：120 天截止、未来 +5 分钟容忍、不得早于上一条（`4188-4194`；
`4204-4209` 是写回 `records[]` 的序列化段）。
`AccountMarker` 的 `total_tokens` 用容错取法 `max(total_tokens, input+output, cached+output)`
（`7756-7760`），因为 Cockpit 各版本对 total 的定义不一致。

**认证身份**（`codex_auth_identity`，`4039-4078`）：`email`（含 JWT `id_token` 的
`email`/`preferred_username`）→ `account_id`（含 JWT
`https://api.openai.com/auth` 的 `chatgpt_account_id`）→ 紧凑 provider 元数据。
末段有守卫 `usable_cockpit_identity`（`4007-4016`）：含空白或 `:;{}[]` 的诊断文本一律拒绝，
避免把路由日志当成账号名。`active_codex_model_provider`（`4081-4098`）只读 `config.toml`
**第一个 `[section]` 之前**的 `model_provider`，决定读 `auth.json` 还是 `.cockpit_codex_auth.json`
（`4110-4134`）——即 README 第 75 行"卸载后的遗留文件不会覆盖官方账号身份"的实现。
⚠️ `monitor.py:666-693` 的同名函数**没有** `usable_cockpit_identity` 守卫（两侧不一致，
列入附录 C）。

**时间轴查询 `account_label_at_time`（`8356-8386`）**：先二分 `switch` markers
（最后一个 ≤ 事件时间即采用），没有再二分 `request` markers，最近距离 ≤
`CODEX_ACCOUNT_MATCH_WINDOW_SECONDS = 600` 才采用，否则 `Unassigned local`
（`UNASSIGNED_CODEX_LABEL`）。时间基准 = `account_at or request_at or when`（`3775-3780`）。
最外层兜底：最近 30 分钟内（`CODEX_CURRENT_ACCOUNT_RECENT_SECONDS = 1800`）且无任何证据时，
用"当前账号"——无 markers 分支在 `8503-8511`（判定 `8504-8509`、写 ledger `8510-8511`），
markers 分支在 `8563-8571`（判定 `8565-8571`）。

**`resolve_api_service_event_accounts`（`10693-11058`）优先级链**（命中即停）：

```
阶段一（OpenCodex 证据先于一切 Cockpit 证据）
  0a. hint 来源 = opencodex_usage 且标签可用非 mirror → 该标签，tier = opencodex_usage_row（rank 4）
  0b. hint 来源 = opencodex_usage_unresolved → 聚合标签 + unresolved += 1
  0c. xai/ 或 opencode-go/ 模型 → Grok subagent / OpenCode subagent 标签
阶段二（Cockpit 证据链）
  1. 精确命中 Cockpit usage 行（token 总量相等 + 活动窗内） → tier = cockpit_usage_row（rank 3），
     并用 marker.model 覆盖事件模型
  2. 结构化 auth_result marker（0.35 s 内、唯一） → tier = affinity_confirmed（rank 2）
  3. 同 turn + when 完全相等 + 标签唯一 → tier = affinity_confirmed
  4. quota_fingerprint hint → confirmed，无 tier（不归档）
  5. opencodex_turn hint → confirmed（mirror 则降级聚合标签），无 tier
  6. 模糊命中 Cockpit usage 行（token 差 ≤ max(256, 0.5%) 且 30 s 内） → tier = temporal（rank 1）
  7. turn anchor 继承（≤ when 的最近一条） → confirmed，无 tier
  8. cockpit_mode 或原标签即 mirror → 聚合标签 + unresolved；例外见 preserve_direct_label
  9. 其余 → 保留原标签
```

`preserve_direct_label`（`11009-11015` 的局部布尔变量，`11016` 使用）是第 8 步的例外：`preserve_direct_official_usage`
**开启、标签不是 mirror 而是具体账号、模型属官方配额模型（`is_official_codex_quota_model`
要求**不含 `/`** 且前缀属 `gpt-/o1/o3/o4/o5/codex-/chatgpt-`，`1641-1652`）、且 300 s 内无
Cockpit 证据 → 保留直连账号。

**归档（verdict）分级**（`495-511`、`10679-10690`）：
`API_SERVICE_VERDICT_TIER_RANKS = {temporal:1, affinity_confirmed:2, cockpit_usage_row:3, opencodex_usage_row:4}`，
归档下限 `API_SERVICE_VERDICT_ARCHIVE_MIN_TIER` 默认 2 ⇒ **只有 rank ≥2 才可能持久化**，
"纯粹按时间窗猜出来的 temporal 结论永远冻结不进档案"（注释 `10679-10690`）。
写归档前还要 `concrete`（非 mirror 标签）+ `confirmed`（`11024-11046`）。

**ledger 双层**：`events`（宽口径标签映射）与 `verdicts`（严口径 + tier + 事件时间）。
保护机制三处（一致解释）：

1. **读稳定性**：`attribution_ledger_document`（`5206-5237`）读前读后戳比对 + 一次重试；
2. **缩水守卫**：`save_attribution_ledger`（`5399-5483`）在"旧 events >1000 条且新 < 50%"
   时拒绝写入标签（但 verdicts 仍要落盘，注释 `5429-5431`）；
3. **只覆盖本轮改过的键**：`_LEDGER_WRITES` 追踪（`3746-3751`、`5441-5448`），
   与并发 exporter 的合并冲突由 `merge_attribution_verdicts`（`5336-5371`）按
   "本轮现场判定胜出 → tier 高者胜 → 同 tier 时戳新者胜"解决。

**事件 ID**：`codex_event_id`（`3711-3720`）=
`session_id|when_ms|model|input|cached|output`（`legacy_codex_event_id` @3723 用 `request_at`
替代 `when`）；`ledger_label_for_event`（`3754-3772`）稳定 ID → 遗留 ID 读并回写。
沙箱实测可见 ledger 内容（附录 A.3）：5 条事件键，其中 2 条归 `Codex local`、3 条归
`Unassigned local`。

### 5.6 mirror 合并与"不再重复扣除"

**README 第 30-31 行核验结论：与代码一致（已验证）**。

- 三个 mirror 标签：`API_SERVICE_MIRROR_LABELS = {api-service-local, api service local,
  codex_local_access_runtime}`（`484-488`）。
- `collapse_api_service_mirror_providers`（`11416-11471`）把它们求和为单行
  `Codex local - api-service-local`，并在末尾 **`output.pop("api_service_mirror_deduction", None)`**
  （`11470`）——历史上用于"重复扣除"的字段被主动清除。调用点三处：历史重建 `11788`、
  同日高水位比较 `12438`、导出收尾 `14158`。
- 展示层的扣除只剩 `monitor.py`：`subtract_sub2api_routed_client_usage`（`5119-5138`，
  只处理 `routed_to_sub2api is True` 或名字恰为 `Codex via Sub2API` 的行）与
  `subtract_sub2api_mirrored_api_key_usage`（`5382-5405`）——后者是**两分支**：
  `if` 要求 `server_tokens > 0` + 名字是 `Codex local - api-key-*` + 命中
  `route_labels["sub2api_mirrored"]`；`elif` 只要名字属 `api-service-local` 且 `tokens > 0`
  就**无条件**扣除（不看 route_labels，`5401-5404`；测试
  `tests/test_usage_state_boundaries.py:1827-1854` 传空 route_labels 仍削减）。
  注意 `is_local_api_service_provider_name`（`5145-5147`）只认 `api-service-local` /
  `api service local`，**不含 `codex_local_access_runtime`**（两侧不对称，列入附录 C）。
- `restore_today_from_usage_history`（`12722-12724`）在存在聚合行时**直接放弃**历史回填，
  避免与聚合行重叠。

### 5.7 Cockpit 请求折叠：已废弃

`reconcile_cockpit_request_usage_events`（`9559-9573`）现在是**恒等函数**（`del account_markers,
affinity_events` + `return events`），文档串（首行原文 `Preserve each distinct Codex
``last_token_usage`` event.`）给出了理由：

> A Cockpit request can span multiple model calls while Codex handles tool results.
> Every token_count row advances total_token_usage by exactly that row's last_token_usage
> and is therefore independently billable. Cockpit request ids remain attribution evidence,
> but they must not collapse those model calls into the final response usage row.

旧实现 `_collapse_cockpit_request_usage_events_legacy`（`9390-9556`）全仓**无调用点**（遗留死代码）。
测试从反向锁定新行为：`tests/test_usage_state_boundaries.py:3085-3095`（280 000 token 长请求全保留）、
`3122-3132`（inflight 250 000 全保留）、`3168-3179`（5 条事件各自的 token 数
`[12345, 50000, 60000, 80000, 90000]` 全部保留、一条不折叠）。

### 5.8 不确定性如何披露（不丢弃、不猜）

- 未归属事件**不丢弃**，落到聚合标签桶继续计入今日总量（`11047`），"待归因期间 Token 仍计入总量，
  但不会借用上一个账号"（README 第 174 行）。
- `add_unattributed_provider_gap`（`12869-12915`）额外披露"总量 − provider 求和"的残差行
  `Codex local - 历史缺口未归属`（`is_unattributed_gap: True`）。
- `same_day_output_high_water`（`12423-12431`）在账号切换瞬间保护同日总量不被拼掉（文档串说明）。
- `unresolved_api_service_events` / `cockpit_fallback_events` 作为顶层字段输出（`14120-14121`）。
- 沙箱实测中，无 `auth.json` 的归属失败被如实拆成 `Codex local`（2 请求）与
  `Unassigned local`（3 请求）两行，today 总量仍为 9 请求（附录 A.3）。

---

## 6. 计价与金额计算

### 6.1 单位约定（先立住这一点）

**内部一律"每百万 token 美元"**，尽管字段名保留 `*_per_token`：

- 在线源（LiteLLM / models.dev）per-token → **×1 000 000** 进内存（`1044-1049`）；
- 计算末尾 **÷1 000 000**（`1544-1549`）；
- 缓存落盘再 **÷1 000 000** 还原 per-token（`1110-1116`），供 `monitor.py` 侧按 per-token 使用。

### 6.2 价格字段模型

`ONLINE_TOKEN_COST_FIELDS`（`842-865`）共 22 项，四桶：
input 6（标准 / `above_272k` / `batches` / `flex` / `priority` / `above_272k_priority`）、
cache_read 5、cache_creation 5（两者都**没有** `batches`）、output 6。
唯一读取点 `token_price_for_request`（`1484-1490`）只拼 `{base}_{priority|flex|batches}`。

**关键结论（已验证）**：`*_above_272k_tokens*` 系列**从不参与计价**——全仓命中只在清单本身与
测试桩（`tests/test_usage_state_boundaries.py:1022-1035`）；README 第 194 行
"超过 272K 输入上下文时仍沿用当前 tier 的普通价格，不应用在线价格表中的长上下文加价字段"
与代码一致。cache_read/cache_creation 缺 `_batches` ⇒ batch 档下缓存桶必然回退标准价
（测试断言 batch = 0.7625）。

`CACHE_READ_COST_ALIASES`（`876-879`）= `cache_read_input_token_cost` →
`input_cost_per_token_cache_hit`，按序取第一个 >0（`1067-1072`），
测试 `tests/test_unpriced_model_accounting.py:250-267`。
`MODELS_DEV_COST_FIELD_MAP`（`880-885`）= `input/output/cache_read/cache_write` →
四个目标字段，且 **input 与 output 必须同时 >0，否则整条模型被丢弃**（`974-975`）。

### 6.3 来源优先级与缓存

| 项 | 值 | 位置 |
| --- | --- | --- |
| 主源 | `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json` | `299-302` |
| 次源 | `https://models.dev/api.json` | `303-306` |
| 缓存文件 | `client_usage_model_prices.json`（schema 2，`models` 为 per-token 明细） | `183-185`、`1140-1149` |
| 缓存有效期 | `CLIENT_USAGE_MODEL_PRICE_CACHE_SECONDS` 默认 86400 | `307` |
| 抓取超时 | `CLIENT_USAGE_MODEL_PRICE_FETCH_TIMEOUT_SECONDS` 默认 4 | `308` |
| 强制刷新冷却 | `CLIENT_USAGE_MODEL_PRICE_REFRESH_COOLDOWN_SECONDS` 默认 300（下限 30） | `309-312` |

合并规则：`_merge_online_price_details`（`1198-1205`）用 `setdefault` ⇒
**主源优先、次源只补缺**；`_refresh_online_price_table`（`1248-1280`）在新抓结果之上
再并入旧缓存的 provider-exact 键（注释 1269-1270："成功的刷新不得抹掉暂时不可用次源的精确价"），
全败则沿用旧缓存并只推进 `last_attempt_at`。

`load_online_price_table`（`1283-1329`）六级决策（内存注入态 → 内存路径匹配 →
强制+冷却 → 缓存新鲜 → 强制+冷却 → 联网刷新）；`_ONLINE_PRICE_CACHE_PATH is None`
（测试注入态）不做二次刷新（`1376-1379`）。

### 6.4 查找键归一与裸键来源（含实测）

`model_price_candidates`（`920-929`）：原样优先；仅当第一个 `/` 前是
`{openai, anthropic, xai, deepseek}` 之一才追加"去前缀"候选；**绝不剥 `opencode-go/`**。

`extract_online_price_details`（`1052-1083`）：
`prices[name] = detail`（同一键后写覆盖前写），并
`prices.setdefault(name.split("/",1)[1], detail)`——仅当
`provider ∈ {openai, anthropic, xai, deepseek}` 且 `name` 带该前缀（`1079-1080`），
或 `litellm_provider` 缺失但 name 以白名单前缀开头（`1081-1082`，专为**缓存往返重建别名**）。

**实测（附录 A.5）**：真实 LiteLLM 表 4123 键中，`claude-opus-4-6`（idx 694,
provider=anthropic, 5e-6/2.5e-5/cache 5e-7）、`gpt-5.5`（idx 1788, provider=openai,
5e-6/3e-5/cache 5e-7）都是**字面存在的裸键**，而 `anthropic/claude-opus-4-6`、
`openai/gpt-5.5` **不存在** ⇒ 沙箱里这些模型命中的是字面裸键，不是派生别名。
反过来 `grok-4.5-build`、`xai/grok-4.5-build` 都不存在（`xai/grok-4.6` 存在），
这解释了代码为何要把 Grok 的计价键显式映射到 `xai/grok-4.6`。

### 6.5 本地回退与"未收录模型"

内置表 `PRICE_PER_MILLION`（`830-839`，8 条，子串匹配、按列表顺序命中）：
`gpt-5.5 (5.0, 0.5, 30.0)`、`gpt-5.4-mini (0.75, 0.075, 4.5)`、`gpt-5.4/5.3/5.2 (2.5, 0.25, 15.0)`、
`opus (15.0, 1.5, 75.0)`、`sonnet (3.0, 0.3, 15.0)`、`haiku (0.8, 0.08, 4.0)`。

两条使用路径的**顺序相反**（已验证）：

- `model_price`（`1397-1404`）：**硬编码 → 在线 → `gpt-5` 家族回退 → (0,0,0)**；
- `resolve_model_price_details`（`1428-1473`，实际计价入口）：**在线明细 → 在线三元组 →
  硬编码 → 家族回退 → `({}, False)`**。

本地 profile（`price_profile_from_rates`，`1407-1421`）固定性质：`cache_creation == input`、
priority = 标准 × `CODEX_FAST_COST_MULTIPLIER`（默认 2.0，`450`）、**没有 flex/batches 键**。

查不到价格时：`resolved=False` + 空 profile → `cost = 0.0` + token 记入
`unpriced_tokens`/`unpriced_models`（`1757-1759`、`1834-1836`、`8748-8750`、`7438-7442`），
**绝不借均价或邻近模型价**（测试 `tests/test_unpriced_model_accounting.py:54-78`）。
沙箱实测：`grok-4.5-build` 1000 token 落为 `unpriced_tokens=1000`、该 provider 成本 0.0（附录 A.3）。

### 6.6 单请求成本公式

```python
profile, resolved = resolve_model_price_details(model, pricing_model, when)   # 6.5
tier = normalize_pricing_tier(pricing_tier)                                   # 1475-1481
suffix = {"priority": "priority", "flex": "flex", "batch": "batches"}.get(tier, "")
rate(base) = profile[f"{base}_{suffix}"] if suffix and that value else profile.get(base, 0)

input_price          = rate("input_cost_per_token")
cache_price          = rate("cache_read_input_token_cost")
cache_creation_price = rate("cache_creation_input_token_cost")
output_price         = rate("output_cost_per_token")
if cache_price <= 0:          cache_price = input_price            # 1540-1541
if cache_creation_price <= 0: cache_creation_price = input_price   # 1542-1543

cost_usd = (input_count * input_price + cached_count * cache_price
          + cache_creation_count * cache_creation_price
          + output_count * output_price) / 1_000_000               # 1544-1549
```

tier 取值链：事件 `pricing_tier` → 缺失时按 `cost_multiplier > 1` 判 `priority`
（`1748`、`1824`）→ Cockpit 上游 row 的 `pricing_tier`（`4580`、`4733`）→
logs_2 行的 `service_tier: Some(...)` 预扫描回填（`3479-3484`、`3502-3506`）→
`apply_codex_speed_fallback` 用 config.toml 回填（`5881-5890`）。

**数值验算（测试档）**：`tests/test_usage_state_boundaries.py:1019-1063` 用
`input=100k, cached=100k, output=10k, cache_creation=50k` 断言 standard = 1.1625、
priority = 2.325、flex = 0.58125、batch = 0.7625、长上下文 = 1.35。

**priority 不被二次乘**（`1747-1748`：乘数只用于选 tier）；
唯一"乘数直接乘金额"是 Cockpit 上游只有总价时（`7430-7443`）。

### 6.7 service tier / speed

- `codex_speed_cost_multiplier`（`1553-1557`）：`fast/quick/turbo` → `max(1.0, COST_MULTIPLIER)`，
  否则 1.0；`speed_badge`（`1560-1565`）产出 `FAST x{mult}`。
- `codex_service_tier_to_speed`（`1568-1578`）：`priority|fast`→`fast`，
  `flex|batch|batches`→对应档，`standard`→`standard`，未知/空→`""`（**不猜**）。
- `normalize_codex_speed`（`5652-5662`）/`codex_speed_meta`（`5663-5670`）。
- 覆盖来源优先级（按 `cockpit_codex_speed_by_label` 的**逐级覆盖**顺序，`5913-5944`）：
  账号级 `speed_overrides`（按 label/email/id/name 匹配，最后写入、**优先级最高**，
  `5940-5942`）> `current_speed`（`CLIENT_USAGE_CODEX_FORCE_SPEED` @`5730-5736` 优先，
  否则 `config.toml` 的 `service_tier` @`5739-5760`，再否则 `.codex-global-state.json` 的
  `default-service-tier` @`5776-5790`）> 账号 JSON 的 `app_speed` > `"standard"`
  （`5937-5939`）。
- `codex_speed_history`（`5844-5866`）用 config.toml 的 mtime 作为切换时刻生成
  `SpeedMarker`；`codex_speed_at`（`5869-5878`）取"最后一个 ≤ 事件时间的 marker"；
  `apply_codex_speed_fallback`（`5881-5890`）的跳过条件是"**乘数非空且 tier 非空**"，
  因此 tier 非空但 `cost_multiplier is None` 的事件**仍会被 marker 的速度档重写 tier**
  （回填值来自 `normalize_pricing_tier` 与 `codex_speed_cost_multiplier`）。

### 6.8 OpenCode Go 专属计价（隔离设计）

- 内置官方价 `OPENCODE_GO_OFFICIAL_PRICES`（`889-905`）：`kimi-k3 (3.0, 0.30, 15.0)`、
  `deepseek-v4-pro (0.66, 0.022, 1.98)`、`deepseek-v4-flash (0.22, 0.007, 0.66)`；
  峰价 `OPENCODE_GO_PEAK_PRICES`（`906-917`）是 `pro (1.32, 0.044, 3.96)`、
  `flash (0.44, 0.014, 1.32)`（均为谷价 ×2），`kimi-k3` 无峰谷变体。
- **峰谷判定**（`1221-1226`）：`UTC 01:00–03:59` 或 `UTC 06:00–09:59`（UTC 小时 1/2/3 与 6/7/8/9，
  **共 7 小时/天**——第 6 轮复核修正：原文误写 6 小时），
  `when is None` → 谷价；无 tzinfo 按 UTC+8 解释再转 UTC。边界测试
  `tests/test_unpriced_model_accounting.py:574-594`。
- **已缓存在线价套峰**（`1229-1245`）：峰时段且模型在峰价表时，按 `peak/standard` 比例
  放大当前价（不替换为固定值）。
- **裸价隔离**（`1434-1457`）：任何以 `opencode-go/` 开头的模型只查
  ①精确在线键 → ②强制刷新后同键 → ③内置官方价 → ④`({}, False)`，
  路径里**没有**裸键查找，也没有硬编码子串匹配。测试
  `tests/test_unpriced_model_accounting.py:80-112`、`492-572`、
  `tests/test_monitor_unpriced_cost.py:109-126`。
- **实测交叉印证（附录 A.5）**：真实 LiteLLM 表里 `deepseek-v4-pro` 与
  `deepseek/deepseek-v4-pro` 的价恰好是 `1.32e-6 / 3.96e-6`（即内置**峰价**），
  若允许回退裸键就会把谷时段请求按峰价计价——这正是隔离分支存在的理由。

### 6.9 第三方路由标签与计价键

- `external_codex_provider_label`（`1655-1661`）：`xai/` → `Grok subagent`，
  `opencode-go/` → `OpenCode subagent`；`assign_external_codex_provider_label`（`1677-1691`）
  同时把标签写进 ledger 的**稳定 id 与 legacy id 双键**。
- `pricing_model` 注入点：Cockpit 行（`4580-4586`，`pricing_model or resolved_model or model`）、
  marker 事件（`4718`、`4742`）、Grok 的 `grok_local_pricing_model`（`8862-8873`）。
- `is_official_codex_quota_model`（`1641-1652`）要求模型名**不含 `/`** 且前缀属
  `gpt-/o1/o3/o4/o5/codex-/chatgpt-` ⇒ 第三方路由不消耗官方 5h/7d 配额统计。

### 6.10 成本聚合口径

- **逐事件 float 累加，无重算**（`1812-1839`、`8733-8752`、`7402-7445`）；
  `add_bucket`（`1842-1860`）把各桶与 `unpriced_*` 逐项相加（含 `models` 字典合并）。
- **只有总额，无分项金额**：`bucket_to_dict`（`12918-12945`）输出
  `cost`（6 位小数）+ 四类 token + `unpriced_*`；没有 input/cache/output 分项成本。
  想分项只能前端按同规则重算（一致解释）。
- `cached_input_tokens` 输出时**把两个来源合并**：`bucket.cached_input_tokens +
  bucket.cache_read_input_tokens`（`12924`、`12953`）。
- `models` / `unpriced_models` 只保留前 8（`12928-12932`），不影响成本合计。
- 实时增量（`build_live_catchup_payload` @`13497-13748`）里事件级成本保留 **12 位小数**，
  `price_resolved = (unpriced_tokens == 0)`（`13697-13699`）。
- 历史行成本（`build_historical_usage_rows` @11667-11830）为
  `round(cost, 6)`，并纳入 `history_row_signature`（`11831-11844`）⇒ 价格变化触发历史重写。
- 高水位防退化（`11930-12016`）：重算 token 更少时保留旧 token，`cost = max(existing, rebuilt)`。
- `backfill_usage_history_details`（`11291-11413`）**只补 providers/models/detail_tokens**，
  不重算行级 cost 与 token 总量（幂等；已有 providers+models 的行跳过）。
- 两类账实不符用 `max(0.0, 差值)` 补差 + 归零下限：today 与历史的 gap（`12795`）、
  gap 行补差（`12847-12852`）、`today − provider 求和` 残差（`12894-12915`）；
  镜像折叠（`11416-11471`）只做求和展示，**不含**减法补差。

---

## 7. 聚合、窗口与输出契约

### 7.1 聚合的三个层次

| 层 | 函数 | 粒度 | 备注 |
| --- | --- | --- | --- |
| 事件 → 桶 | `add_codex_event_to_bucket`（`1812-1839`）、`add_claude_event_to_bucket`（`8733-8752`）、`add_cockpit_usage_to_bucket`（`7402-7445`） | 单请求 | **计价在这一步完成**（不是渲染期） |
| 桶 → 桶 | `add_bucket`（`1842-1860`） | 合计 | 五类 token 桶 + `requests`/`cost`/`unpriced_*`/`models` 全量相加 |
| 事件 → 小时 | `codex_hourly_from_events`（`8793-8816`）、`claude_hourly_from_events`（`8766-8787`）、`merge_hourly_buckets`（`9062-9077`） | 小时 | 只有 `requests/tokens/cost` 三个量，缺数据小时为 0 |

`merge_hourly_buckets` 先构造 24 个 `{hour, requests, tokens, cost}` 空行，再逐源累加，
`cost` 最后 `round(..., 6)`（`9062-9077`）。失败小时由 `mark_codex_failure_hours` 追加
`failure/failure_count/failure_at/failure_kind` 四键（`9080-9141`）。

**缓存命中率不在导出层计算**（一致解释 → 已验证）：三处独立算且口径一致
`cached / (input + cached + cache_create)`：
`monitor.py:8911-8912`（保留一位小数，最新提交的主题）、`monitor_ui.py:1095-1097`、
`analytics_server.py:562` / `:633`（两位小数百分比）。

### 7.2 输出契约：`client_usage_today.json`

**顶层字段（沙箱实测 + 源码双向确认）**：

| 字段 | 类型 | 含义 |
| --- | --- | --- |
| `schema` | int | 固定 1（`14094`） |
| `usage_accounting_schema` / `claude_usage_schema` / `cockpit_usage_schema` / `grok_usage_schema` / `opencodex_attribution_schema` | int | 五个独立口径版本轴（`14095-14099`） |
| `source` | str | 固定 `"client-jsonl"`（`14100`） |
| `updated_at` / `date` | ISO str | 本地时间（`14101-14102`） |
| `scan_status` | obj | `state/from/through/source`（`14103-14108`） |
| `today` | obj | `bucket_to_dict("Client local", total)`：`requests/tokens/input_tokens/cached_input_tokens/cache_creation_input_tokens/output_tokens/cost/models/unpriced_tokens/unpriced_models/latest_at/latest_model/show_zero`（`14109`、`12918-12945`）；另有**条件字段** `app_speed`/`cost_multiplier`/`speed_badge`，仅当 `latest_app_speed` 非空时注入（`12937-12944`） |
| `providers` | array | 按归属标签的明细；`name` 含 `@` 时附带 `window_5h/window_7d/window_rolling_7d/window_cycle`（+ 可选 `window_30d`）（`13993-14010`、`14002-14005`；`window_rolling_7d` 由 `13478` 生成、仅被 `monitor.py:5564`/`6207` 透传） |
| `latest_request` | obj | `provider/model/created_at/kind`（`14111-14116`） |
| `active_sessions` / `unresolved_active_sessions` | array | 会话态（`14117-14118`） |
| `api_service_routed` | bool | 当前 route 是否指向 Cockpit 服务（`13889`、`14119`） |
| `unresolved_api_service_events` / `cockpit_fallback_events` | int | 未归属与补漏计数（`14120-14121`） |
| `dashboard.hourly_today` | array[24] | 小时序列（`14122-14125`） |
| `dashboard.reconciliation` | obj | OpenCodex 对账六状态 × (requests, tokens) |
| `offline_catchup` | obj | 离线回填状态（仅当天且启用时，`14127-14150`） |
| `account_30d_updated_at` | ISO str | 仅 `--include-30d`（`14151-14156`） |
| `api_service_aggregate` | obj | mirror 聚合行存在时注入（`11416-11471`） |

收口顺序固定（**已验证**）：`collapse_api_service_mirror_providers` →
`same_day_output_high_water` → `restore_today_from_usage_history` →
`add_unattributed_provider_gap` → `write_json_atomic`（`14158-14162`），
随后可能因离线回填入队而二次写盘（`14163-14183`）。

### 7.3 窗口统计（5h / 7d / 30d / cycle）

`build_codex_window_stats`（`13085-13483`，仅当天调用，`13956-13965`）：

- 窗口时长来自**上游**：`quota_window_duration`（`7394-7399`）读 `window_minutes`；
  起点由 `quota_window_start`（`7375-7391`）从 `resets_at` 反推（含过期周期分支）；
  对齐起点有 −10 s 容差（`440`）。
- 窗口类别判定（`7057-7062`）：主窗口分钟数 `<7d` → 5h、`==7d` → 7d、`>7d` → cycle；
  plus 型账号无短窗口时 5h 置 `quota_unlimited`（`7072-7077`）。
- 每账号产出 `window_5h/window_7d/window_cycle`（`bucket_to_window_dict` @12948-12969），
  再叠加配额 payload（`13445-13483`）；配额只覆盖
  `quota_available/quota_stale/resets_at/window_minutes/window_days/remaining_percent/utilization`，
  **不含 cost**；未覆盖的窗口显式置零（`13435-13444`）——缺数据不沿用上一窗口成本。
- 另有滚动窗口 `window_rolling_7d` 与官方对齐窗口并存；`window_rolling_7d` 无 quota 覆盖，
  目前只被 `monitor` 透传（`5564`、`6207`）。
- 倒计时状态机：`apply_quota_countdown_state`（`12972-13046`）、
  `apply_5h_countdown_state`（`13049-13052`）。

### 7.4 历史库 `usage_history.json` 与高水位

**结构**（沙箱实测 + 源码）：`{schema: 2, days: {"YYYY-MM-DD": 日行}, offline_sync: {...}}`。
日行字段含 `date/source/source_date/usage_accounting_schema/claude_usage_schema/
cockpit_usage_schema/grok_usage_schema/opencodex_attribution_schema/requests/tokens/
input_tokens/cached_input_tokens/cache_creation_input_tokens/output_tokens/cost/models/
providers/detail_tokens/updated_at`（+ 可选 `source_gap`、`offline_reconciled_at`、
`accounting_migration_high_water_guard`）。**沙箱复现**：导出器在当天运行时会顺手把
昨天（10-14）以 0 值补进 `days` 并写 `offline_sync`（附录 A.3）。

**高水位合并** `merge_rebuilt_history_day`（`11886-12051`）四分支。注意"schema 升级"有**两条
独立轴**：`usage_accounting_schema`（`11916-11919`）与 `cockpit_usage_schema`（`11920-11927`），
`schema_upgrade` 取二者之或（`11928`），但下面②③的替换/残余守卫**只由 accounting 轴触发**，
仅 cockpit 轴升级走④的合并分支：
① 无既有行 → 直取重建值；② schema 升级且重建 ≥ 既有 → 全量替换；
③ schema 升级但重建更低 → 保留既有 token 与 `cost` 差值，并追加**字面标签**
`"Historical high-water"` 的残余 provider（`11859`、`11957-12006`；注意它与今日输出中的
`HIGH_WATER_UNATTRIBUTED_LABEL`（`480-490`，"Codex local - 历史缺口未归属"）**不是同一个标签**）；
④ 其余 → `requests/tokens/cost` 逐项取 `max`。
`history_row_signature`（`11831-11844`）把 `round(cost,6)` 纳入签名 ⇒ 价格变化触发重写。
`monitor.py` 侧同日高水位阈值是 **5%**（`2206`），跨日不写（`2142-2143`）。

⚠️ **同日高水位的副作用（第 6 轮实验确证）**：本函数按 **provider 名逐行**取高水位，
并在旧行消失时整行恢复，最后用 provider 求和覆盖 `today`。当同一批事件在两次运行中
归属到**不同**的 provider 行时（待归因 → 证据到达的升级、30 分钟窗口滑动等），
两行都会被保留 ⇒ 总量被推高。机制、受控实验与影响范围见 §8.2 第 10 条。

### 7.5 离线回填 worker

- 默认回看由 env `CLIENT_USAGE_OFFLINE_BACKFILL_MAX_DAYS` 控制（Python 常量
  `OFFLINE_HISTORY_BACKFILL_MAX_DAYS`，默认 31、clamp 下限 0，`445-448`），设 0 关闭。
- lease 默认 300 s、检查间隔默认 3600 s（两者都是 `max(60, …)`，`210-217`）；worker 用
  **文件锁 + run_id 去重**（`12173-12337`）；状态文件
  `client_usage_offline_backfill_status.json`（状态文件本身另写 `total_days` @12325 等，
  而 `14134-14150` 是导出到 `output["offline_catchup"]` 的**白名单**：
  `state/run_id/queued_at/started_at/heartbeat_at/completed_at/next_check_at/retry_after/
  scanned_days/updated_days`）。
- 复用既有解析/去重/归属/计价规则，并对历史总量保留高水位（README 第 176-181 行与
  `merge_rebuilt_history_day` 一致）。
- `backfill_usage_history_details`（`11291-11413`）只补 providers/models/detail_tokens。
- **沙箱实测**：worker 被 spawn（`.offline-history-backfill.lock` 出现），10-14 以
  `source: "local-backfill"`、`requests: 0` 写入，状态转为 `complete` 且
  `next_check_at = +1h`（附录 A.3）。

### 7.6 实时增量链路（GUI 侧，不改写权威快照）

**本节行号均指 `monitor.py`**（导出器侧的对应逻辑见 §3.11）。架构（一致解释 → 已验证）：

1. **发现**：`WindowsDirectoryChangeSignal`（`monitor.py:2595-2749`）用
   `ReadDirectoryChangesW`（64 KiB 缓冲、子树、5 种 change filter、守护线程同步阻塞）
   把变化文件名塞进候选集；失败/溢出记 `_mark_overflow` 触发补扫。
   `TOKEN_PULSE_NATIVE_FILE_WATCH` 默认 `auto`（非默认根需显式开启）。
2. **判定**：真正的"变没变"永远是 `(st_size, st_mtime_ns)` 比对（`3040-3052`），
   通知只负责提前给出候选路径。
3. **读取**：热文件集合上限 64，分片轮询上限 32，单次最多 256 KiB、带 8 KiB overlap
   避免跨写断行（`159-178`、`2811-2815`）；事件去重按 `(path, 累计四桶)` 或
   `(path, 时间戳, 三桶)`，上限 4096（`2968-3001`）。
4. **降频**：热 100 ms（≤10 s 空闲）→ 250 ms（≤60 s）→ 500 ms（更久）（`141-154`、`2920-2927`）。
5. **补扫**：每 30 s 全目录，另外"首次未 primed"或"原生通知溢出"时也会触发
   （`3026-3038`）；再加今日/昨日目录级兜底（`2872-2908`）。
6. **即时层不改写权威快照**：即时增量只进 overlay 与 `state.today_*`（`13901-13913` 是 overlay→state
   的内存高水位写入），`client_usage_today.json` 始终由导出器重建；但**即时事件路径收尾
   （`13795-13797`）也会调 `_persist_live_usage_checkpoint` 与 `_update_live_usage_history_if_due`**，
   后者按 **60 s 节流**（`11423`，初值 `-inf` @6467）用当前 `state.today_account_cost/today_tokens`
   推进 `usage_history.json` 的今日条目（`11426` → `update_usage_history` @2339 →
   `_update_usage_history_unlocked` @2092 的 `2104-2106`）；历史写的另一调用点是核对完成路径
   （`12583-12584`）。超过 `LIVE_USAGE_VERIFY_THRESHOLD_TOKENS`（默认 50 000 000，`183-186`）
   才触发只读全扫描核对（另有 `12597`、`13538`、`14224-14229`、`14832` 等触发点）。
7. **检查点**：`client_usage_live_checkpoint.json`（schema 5，写频率 1 s）保存 overlay；
   重启时 `_restore_live_usage_checkpoint`（`11623` 起）按 `target = base + delta`
   **按比例补齐当前显示总量**（ratio 缩放在 `11702-11741`），不重放 JSONL 区间。

### 7.7 分析网页 `analytics_server.py`

（本节裸行号均指 `analytics_server.py`。）

- 监听 `127.0.0.1:8765`（`52-53`，可环境变量覆盖）；只读 `usage_history.json` 与
  `~/.cc-switch/cc-switch.db`。
- 路由：`/api/health`、`/api/dashboard`（meta/options/summary/daily/previous_daily/
  accounts/models/sources/composition/weekly/monthly/anomalies/quality）、`/api/day`、
  `/api/account`、`/api/export.csv|json?kind=daily|accounts|models`、`POST /api/refresh`。
- 聚合细节：7 日均线 = 含当日的尾部 7 日窗口（`519-524`）；累计 = 前缀和；
  模型筛选时按当日占比缩放并打 `estimated_fields=["requests","cost","token_composition"]`
  标记（`463-475`）；异常检测阈值：spike 2.5×中位且增量 ≥1M、drop ≤0.25×、
  cost_rate >2.2×且 cost>1、detail_gap coverage <70（一致解释，测试可证）。
- ccSwitch 回填：表级 + 日级两级缺口（§3.10），`origin` 三态
  `primary`（`normalize_day` 默认参数 @`163`、落键 @`170`）/ `ccswitch_backfill`（`331`，
  对应行内 `source` 字面量 @`320`）/ `missing`（该值由 `blank_day()` → `normalize_day(..., "missing")`
  产生，`analytics_server.py:414-415`；`786` 只是 `anomalies` 里同名的消费点，另有 `117`/`784`）。

### 7.8 界面展示口径

（本节裸行号均指 `monitor.py`，`monitor_ui.py` 会显式标注。）

- 三页：实时（accounts）/ 统计（stats）/ 账号（library）（`monitor_ui.py:632`）。
- 环形钟数据链：`client_usage_today.json` → `dashboard.hourly_today` →
  `_usage_range_summary("24h").series` → `hourly()`（`monitor_ui.py:660-673`）；
  72 段（每小时 3 段），失败小时整段标红。
- Token 构成四色条 + 一位小数缓存命中率（命中率计算 `monitor.py:8911-8912`；
  条状绘制 `8946-8964`；出现"未归类"时 `token_items` 变 5 段，`8919-8921`）。
- 账号页：5h 页只保留有官方 5h 额度的账户（`monitor.py:9180-9184`）；7d 页并入历史回退行
  （`9209-9210`、`7857-7879`）；30d 在无 SUB 账户时改用历史聚合（`9164-9166`）。
- 导出 → 界面唯一入口 `load_client_usage`（`monitor.py:4793-5031`）：
  文件缺失/坏 JSON/无 `today` → 全零 stale；日期不符 → 丢总数保 schema；
  成功返回含 `export_state` 五值（`cached`/`ok`/`error`/`timeout`/`unavailable`），
  另有 `partial`/`stale` 两个覆盖态（`partial` 由 `offline_catchup` error 或
  timeout/error + output_changed 触发）。

---

## 8. 边界、局限与设计权衡

### 8.1 明确"不做"的事（源码可证）

| 不做的事 | 证据 |
| --- | --- |
| 不输出 input/output/cache 分项**金额** | 汇总层只有 `cost` 单值（`12927`、`12956`、`11821`、`13697`） |
| 不做 Claude 的**项目维度**聚合 | 无 slug→cwd 解码；`ClaudeUsageEvent` 无项目字段（`717-734`） |
| 不处理 Claude **sidechain/子代理** | 全仓零命中（§4.4） |
| 不用长上下文（>272K）加价字段 | `*_above_272k*` 从不被读取（§6.2） |
| 不按 0 静默吞掉未知模型 | `unpriced_tokens`/`unpriced_models` 全程可见（§6.5） |
| 不重放 JSONL 区间做"补缺口" | 检查点是**按比例补齐**当前显示总量（`monitor.py:11702-11741`） |
| 导出层不再做镜像扣除（`api_service_mirror_deduction` 键全仓只在 `11470` 被 `pop`，无写入方） | `output.pop("api_service_mirror_deduction", None)`（`11470`）；折叠只做求和（`11416-11471`） |
| 不给"时间窗猜出来"的归属写归档 | tier 门槛 2（`API_SERVICE_VERDICT_ARCHIVE_MIN_TIER` @`501-504`） |
| 不把会话正文写入缓存 | `compact_codex_cache_row` 文档串 + 字段白名单（`2061-2146`） |
| 不读 `CODEX_HOME` / `%APPDATA%\.codex` | 全仓 grep 零命中；sessions 硬拼（`13822`） |

### 8.2 已知精度边界（做同类工具必须知道的坑）

1. **`input_tokens` 是混合口径**：Codex/Grok 已减缓存，Claude 未减（§4.4）。
   跨来源求和时该字段不可直接与"未缓存输入"语义对齐；可跨来源比较的是
   `cached_input_tokens`（两来源合并输出）与 `cache_creation_input_tokens`。
2. **`models` 字典只保留前 8**（`12928`）：按模型下钻到第 9 名以后会从 provider 行缺失，
   但总量不受影响。
3. **两处不共享代码的"同一件事"**：`live_usage_event_id`（导出器 vs GUI，有测试锁定）
   与 `codex_auth_identity`——测试只锁定"具体账号/JWT 路径下两者返回相同值"
   （`tests/test_usage_state_boundaries.py:156-169`）以及**导出器侧**的自由文本守卫
   （`171-176`）；**monitor 侧的自由文本分支没有断言**，而 `monitor.py:666-693` 确实缺
   `usable_cockpit_identity`，会把路由诊断文本当作账号身份。
4. **mirror 标签两侧不对称**：`API_SERVICE_MIRROR_LABELS` 有 3 项，
   而 `monitor.is_local_api_service_provider_name` 只认 2 项
   （`codex_local_access_runtime` 在展示层不被该分支扣除）。
5. **Grok 单事件上限只在导出器侧缺失**：`MAX_SINGLE_EVENT_TOKENS` 只作用于 Codex/Claude
   路径（§4.7）；但 `monitor.py` 的实时链路用同源 env 常量
   `LIVE_USAGE_MAX_SINGLE_EVENT_TOKENS`（`monitor.py:179-182`）对合并后的
   `[*codex_events, *grok_events]`（`12707`）统一过滤（`13471`），故 Grok 在 GUI 侧同样受限。
6. **Grok 无 −2h mtime 缓冲**：其 `updates.jsonl` 若长期不重写而事件时间落在今日窗口内，
   可能被漏扫（一致解释）。
7. **Cockpit 无事件级增量**：每次运行全窗口 `fetchall`，且 `request_id`/`latency_ms`
   依赖版本（`PRAGMA table_info` 探列后回退）。
8. **`client_usage_account_timeline.json` 的切换边界**依赖 `auth.json` mtime；
   快速来回切号可能压缩成同一个边界（`record_account_timeline_snapshot` 的夹取规则）。
9. **`opencode-go` 在线条目若是峰价会被重复放峰**（第 6 轮实验确证，**导出器与 GUI 双侧同源**）：
   `apply_opencode_go_time_pricing`（`1229-1245`）无条件按 `peak/standard` 比例乘，唯一的守卫是
   `standard_price > 0 and current_price > 0`（`1243`）——**全仓没有任何"当前价是否已是峰价"的判定**。
   对"已是峰价"的条目会**再乘一次比例**（单次倍数 = `peak/standard` = 2，相对标准价共 ×4）：
   实测峰价 `1.32/0.044/3.96` 在峰时段返回 `2.64/0.088/7.92`；把结果再喂一次得 `5.28/0.176/15.84`
   （乘性、无幂等）。`monitor.py:1461-1480` 的 `adjust_live_opencode_go_time_price` 是**同构实现**
   （门禁 `1469`、缩放 `1479`，`live_opencode_go_peak_at` `1450-1458` 同判据），GUI 实时链路同样重复放峰。
   当前 `models.dev` 存的是标准价（`deepseek-v4-pro` = `0.66/1.98/0.022`）且 LiteLLM 里没有任何
   `opencode-go/*` 键，故**未触发**；上游改峰价或手工写入峰价缓存即会重复计费（§C.3 第 3 条）。
   另注：`deepseek-v4-flash` 的在线值（`0.15/0.6/0.003`）与内置标准价（`0.22/0.007/0.66`）不同源，
   放大后（`0.30/1.2/0.006`）也不等于内置峰价（`0.44/1.32`）——在线路径的峰价并不等于内置峰价。
10. **同日高水位在"归属漂移"时会把同一批量计两次**（第 6 轮实验确证 + 独立复核，影响最大的一条）：
    `same_day_output_high_water` 的 provider 合并是**按名字逐行取高水位**（`merge_cumulative` `12538-12565`、
    对齐入口 `12643-12696`，旧文件里新结果没有的行会**整行恢复** `12683-12693`），随后 `today`
    又被 **provider 求和覆盖**（`12700-12719`）。于是当同一批事件在两次运行中落到**不同的 provider 行**时，
    两行的量都被保留 ⇒ `today.requests/tokens/cost/unpriced_*` 全部被推高。
    - **触发门槛比"账号切换"低得多**：`ledger_label_for_event`（`3754-3772`，`3762-3763`/`3768-3769`）
      把 `Unassigned local` 与 `Codex local` **当空标签丢弃**，这两类归属**每轮重算**，只由
      `CLIENT_USAGE_CURRENT_ACCOUNT_RECENT_SECONDS`（`434`，默认 1800 s）窗口决定 ⇒
      **仅时间流逝**（某事件在 30 分钟内被导出过一次、之后超过 30 分钟再导出）就会双计。
    - 独立复核的最小实验（真实值恒为 `2 请求 / 3250 tokens`）：对照连跑稳定 2/3250；
      人为换行 → `3/4350`；把会话时间前移进窗（走整行恢复）→ `3/4350`；
      **零干预只等 3 分钟**让一个会话越过 1800 s → `3/5400`；再跑不变（**不自愈**）；
      整批事件一次性换行 → `4/6500`，即**最坏 2×**。
    - 本报告沙箱的对应观察：正确快照 `9 请求 / 17810 tokens`，旧快照被换行后重跑得
      `10 请求 / 20110 tokens`（`Codex local` 3/9720 + `Unassigned local` 3/5700，同一份 2300 tokens 两行各留一份）。
    - **不会在 gap 行上暴露**：派生缺口行按 `today − Σ 其他行` 计算（`12869-12915`），双计后差值为负、
      该行反被移除（`12891-12893`）。
    - **限定**：只有 `current_api_account_routing`（`12483-12484`）为真时才在 `12677-12682` 跳过行级合并；
      纯本地官方账号路径没有任何防护。反之，**具体账号行**（`Codex local - <identity>`）不在 ledger 的
      忽略名单里、一旦固化就稳定，因此"具体账号 ↔ 具体账号"的漂移需走 mirror 升级（`8542-8562`）或
      cockpit verdict 路径（该分支只做过代码阅读，未实验）。
    - 另一条**独立**的推高路径是 `restore_today_from_usage_history`（`12722` 起）：它把差额持久化为
      `days[<今天>].source_gap` 写回历史库（`12801-12805`）——复核类似问题时需单独隔离。
    - 最小修复方向（供参考）：把 `Unassigned local` 一并纳入 `is_derived_gap_provider`（`12494-12501`）
      或对齐循环的跳过集合；注意该标签可由 `CLIENT_USAGE_UNASSIGNED_CODEX_LABEL`（`449`）改名，
      修复不能写死字面量。

### 8.3 死代码与遗留路径（当前 HEAD 无调用点）

| 符号 | 位置 | 说明 |
| --- | --- | --- |
| `add_codex_usage` | `client_usage_export.py:1728` | 被 `make_codex_event` + `add_codex_event_to_bucket` 取代 |
| `codex_fork_replay_cutoff`（文本行版） | `2025` | 活路径用 `codex_fork_replay_cutoff_from_rows`（`2664`） |
| `_collapse_cockpit_request_usage_events_legacy` | `9390` | 折叠口径已被废弃（`reconcile_cockpit_request_usage_events` 现在是恒等函数） |
| `load_opencodex_account_timeline` / `OPENCODEX_ACCOUNT_TIMELINE_RETENTION_DAYS` | `4253` / `398` | 生产路径无调用者（一致解释） |
| `merge_codex_account_fallback_events` | `8323-8342` | 按 cutoff 过滤后重新入桶的"补漏"函数，全仓无调用点（第 6 轮复核新增确认） |
| `_models_dev_route_names` 的 `elif provider in MODEL_ROUTE_PREFIXES: return ()` | `994-995` | 前两分支已枚举 `MODEL_ROUTE_PREFIXES` 全部成员 → 不可达 |

---

## 9. 对 TokenScope 的可复用结论

（本节只列**与本仓已有设计对照**后值得记录的差异点，不构成排期。）

| 维度 | TokenScope 现状（CLAUDE.md 口径） | Token Pulse 做法 | 值得借鉴的点 |
| --- | --- | --- | --- |
| 架构 | Rust 单进程 + SQLite 事实库 | Python 双进程 + JSON 快照 + 历史 JSON | 快照与事实库分离；GUI 只读快照 |
| 事件身份 | Claude `(session_id, message.id)`；Codex `(session_id, 模型, 四桶)` | 三层身份键 + **跨来源 alias**（签名+时间窗+provenance+弱身份） | alias 的"1:1 配对"防塌缩设计与放下条件 |
| fork 去重 | 保守重播指纹（非强身份） | 父签名**单调前缀匹配** + cutoff + 跨会话累计键；缺父文件时自动降级 | 父签名前缀法比纯指纹更精确，但依赖父文件同窗可见 |
| 未知价格 | 未收录模型始终"未知"，不按 0 吞 | 同策略 + `unpriced_tokens/unpriced_models` 全链路披露 | unpriced 计数是比"成本 0"更好的 UI 提示（本仓已有 unpriced 语义可对照） |
| 多来源计价 | 三来源独立候选 + 取最高费用 | 单链优先级（在线明细 → 在线三元组 → 内置表 → 家族回退），OpenCode Go 走隔离分支 | "渠道专属价与裸模型价隔离"的显式分支值得借鉴 |
| 峰谷定价 | 外置峰谷时间规则（按事件时间） | 内置峰谷比例（UTC 01–04 / 06–10，×2） | 按比例放大在线价而不是替换固定值 |
| 时间口径 | 唯一权威 `settings.toml` 时区，全链路只做一次转换 | 固定 UTC+8，两套常量（导出器 `LOCAL_TZ` / GUI `CN_TZ`） | 固定时区更简单但不可配置——本仓保留可配置是更强的取舍 |
| 归属不确定性 | 弱身份如实披露 | 未归属 token 计入总量 + gap 行 + verdict tier 归档门槛 | **归档门槛（rank ≥2）** 与"live 证据覆盖陈旧归档"是防止错误记忆的好模式 |
| 成本分项 | 请求级明细（分项单价/小计）随 `cost_usd` 同源输出 | 只有总额，无分项 | 本仓的请求级明细更完整，无需回退 |

---

## 附录 A：隔离沙箱实验（可复现）

### A.1 实验设置

```powershell
# 1) 临时 HOME + 临时数据目录，物理隔离真实数据
$lab = "$env:TEMP\ctp-lab"
# 2) 环境变量（关键三项）
$env:USERPROFILE = "$lab\home"          # 导出器用 expanduser("~") 定位日志
$env:HOME        = "$lab\home"
$env:TOKEN_PULSE_DATA_DIR = "$lab\state" # APP_DIR：所有缓存/账本/历史都落这里
# 3) 强制离线：价格源指向不可达地址（验证内置回退链）
$env:CLIENT_USAGE_MODEL_PRICE_URL      = "http://127.0.0.1:9/none.json"
$env:CLIENT_USAGE_MODELS_DEV_PRICE_URL = "http://127.0.0.1:9/none.json"
$env:CLIENT_USAGE_OFFICIAL_QUOTA_REFRESH = "0"
python .\client_usage_export.py --output "$lab\out.json"
```

合成 fixture（`%TEMP%\ctp-lab\make_fixtures.py`）覆盖：Codex 累计型 / last 型 / **fork 重放型** /
超限与坏行型四类会话、Claude 同一 `message.id` 重复两行（第二行含 cache 细分，另有一条独立
`msg_solo_2`）、Grok `turn_completed` 且 `modelCalls=2`。

**隔离边界与一处修正（第 6 轮发现）**：上面三个变量隔离了"日志根 / 数据目录 / 价格源"，
但 **Codex Desktop 日志根当时未隔离**——`default_codex_desktop_log_roots()` 走的是
`%LOCALAPPDATA%`/`%APPDATA%`/`%PROGRAMFILES%`，**指向真实目录**（本机 `%LOCALAPPDATA%\Codex\Logs`
存在，内含 2026-09-30 的 `codex-desktop-*.log`）。因此早期几轮实验确实读取了这些**应用日志**
（只读、且只匹配 `sa_server_request_failed` 失败行，不产出 token、不影响统计结论），
但没有读取任何**会话日志**。第 6 轮起改为把 `CLIENT_USAGE_CODEX_DESKTOP_LOG_ROOT`
指向 lab 内空目录；补测显示这**不改变**请求数/成本（往返对照：不隔离 9 请求 / 隔离 9 请求），
也**不是**耗时主因（§A.4）。

### A.2 离线（内置回退链）结果与手算复算

导出器输出：`requests=9, tokens=17810, input_tokens=11520, cached_input_tokens=3800,
cache_creation_input_tokens=30, output_tokens=2460, cost=0.160445,
unpriced_models={"grok-4.5-build": 1000}`。

手算逐项（**全部与程序输出精确一致**）：

| 来源 | 事件 | 桶（uncached/cached/out） | 成本（USD） |
| --- | --- | --- | --- |
| Codex A（累计型，2 行） | 1000/200/100 → 3000/600/400 | 2400 / 600 / 400 | (2400×5 + 600×0.5 + 400×30)/1e6 = **0.0243** |
| Codex B（last 型） | last 2000/500/300 | 1500 / 500 / 300 | gpt-5.4：(1500×2.5 + 500×0.25 + 300×15)/1e6 = **0.008375** |
| Codex C（fork A） | 重放 2 行被剔除；9000/2000/1500 → delta 6000/1400/1100 | 4600 / 1400 / 1100 | (4600×5 + 1400×0.5 + 1100×30)/1e6 = **0.0567** |
| Codex D（超限 + 坏行） | 2.5M 被上限剔除，保留 300/0/20 | 300 / 0 / 20 | (300×5 + 20×30)/1e6 = **0.0021** |
| Claude | `msg_shared_1` 两行合并取最全快照 + `msg_solo_2` | 2120 / 1000(read) / 540，cache_creation 30 | shared 0.00132 + opus 0.06765 = **0.06897** |
| Grok | 1 turn，`modelCalls=2` 拆 2 事件 | 600 / 300 / 100 | **0.0（unpriced）** |

合计成本 = 0.091475（Codex）+ 0.06897（Claude）= **0.160445** ✓
合计请求 = 5 + 2 + 2 = **9** ✓；总 token = **17810** ✓。

### A.3 输出结构与副作用（沙箱实拍）

- `providers` 四行：`Codex local`（2 请求，0.0588 = C+D）、`Unassigned local`
  （3 请求，0.032675 = A+B）、`Claude local`（2 请求，0.06897）、`Grok local`
  （2 请求，0.0，`unpriced_tokens=1000`）→ **未归属事件计入总量、按标签分桶披露**。
- `dashboard.hourly_today`：16 点 4 请求 / 17 点 5 请求（与 fixture 时间相符）。
- `dashboard.reconciliation`：`local_only = 5`（无代理标记时的降级）。
- `latest_request`：`{"provider":"Codex local","model":"gpt-5.5",...,"kind":"success"}`。
- `$TOKEN_PULSE_DATA_DIR` 下生成：`client_usage_codex_event_cache.json`（+`.bak`）、
  `client_usage_attribution_ledger.json`（5 条事件键，值为账号标签）、
  `client_usage_speed_history.json`、`usage_history.json`（+`.bak`，
  `days["2026-10-14"]` 以 0 值 + `source: "local-backfill"` 写入）、
  `.offline-history-backfill.lock`、`client_usage_offline_backfill_status.json`（`state: complete`）。
- **无残留进程**（离线回填 worker 自行退出）。

### A.4 增量缓存与追加

| 运行 | 动作 | 耗时（墙钟 / 导出器自报） | 结果 |
| --- | --- | --- | --- |
| 第 1 次 | 冷启动（**该轮用在线价格态 `state2/`**） | — / — | 见 A.5（0.115367） |
| 第 2 次 | 复用缓存（行缓存命中） | **0.56 s** | 输出与第 1 次一致 |
| 第 3 次 | 向会话 A 追加一条累计行（12000/3000/2000） | **0.57 s** | `requests=10 tokens=28410 cost=0.197568`（增量被正确并入） |

> ⚠️ **两次实验的价格态不同**：A.4 的耗时与第 3 次数字取自**在线价格态**目录（`state2/`，
> 价格缓存在 24 h 内复用，§6.3 决策第 4 级）。在该目录：第 1 次即 A.5 的 **0.115367**；
> 追加事件的增量 = delta `6600/2400/1600` × gpt-5.5 在线价 `5/0.5/30` = **0.0822**，
> 合计 **0.197568**，与 tokens `17810 + 10600 = 28410` 自洽。**离线态**（`state/`）的对应
> 总量则是 A.2 的 **0.160445**——两者都同时被复算验证过。

> **耗时归因（第 6 轮补测与复现）**：冷启动 = **2.54 s 墙钟 / 2.2 s 自报**；热缓存 =
> **0.65 s 墙钟 / 0.2 s 自报**（与原文 0.56/0.57 s 同量级）。差异来源是
> `client_usage_codex_event_cache.json` 是否命中，**与价格源是否可达无关**——
> 离线（价格源不可达）2.5 s ≈ 在线首次 2.6 s，而在线第二次即降到 0.65 s。

### A.5 在线价格链（真实网络，仅取价格表）

同一 fixture、放开网络后：成本从 **0.160445 → 0.115367**，并生成
`client_usage_model_prices.json`（`fetched_at` + **15859** 条模型键）。
逐键核对（脚本读取缓存）：

| 查询键 | 是否命中 | 值（per-token） |
| --- | --- | --- |
| `gpt-5.5` | ✅ 字面键 | in 5e-06 / cache_read 5e-07 / out 3e-05 |
| `claude-opus-4-6` | ✅ 字面键 | in 5e-06 / cache_creation 6.25e-06 / cache_read 5e-07 / out 2.5e-05 |
| `claude-sonnet-4-5` | ✅ 字面键 | in 3e-06 / cache_creation 3.75e-06 / cache_read 3e-07 / out 1.5e-05 |
| `grok-4.5-build` / `xai/grok-4.5-build` | ❌ MISSING | 仍 `unpriced` |

原始 payload 复核（4123 键）：`claude-opus-4-6` 在 idx 694（`litellm_provider=anthropic`）、
`gpt-5.5` 在 idx 1788（`provider=openai`）都是**字面键**；`anthropic/claude-opus-4-6`、
`openai/gpt-5.5` 不存在；`xai/grok-4.6` 存在而 `grok-4.5-build` 不存在；
`deepseek-v4-pro` 与 `deepseek/deepseek-v4-pro` 的裸键价恰为
`1.32e-06 / 3.96e-06`（= 代码内置的 **OpenCode Go 峰价**），印证 §6.8 的隔离分支必要性。

---

## 附录 B：关键常量与默认值速查

| 常量 | 默认 | 位置（未标文件名者 = `client_usage_export.py`） |
| --- | --- | --- |
| `LOCAL_TZ` | UTC+8 | `453` |
| `CODEX_DEFAULT_MODEL` | `gpt-5.5` | `359` |
| `MAX_SINGLE_EVENT_TOKENS` | 2 000 000 | `360` |
| `CODEX_FAST_COST_MULTIPLIER` | 2.0 | `450` |
| `MODEL_PRICE_CACHE_SECONDS` | 86400 | `307` |
| `MODEL_PRICE_FETCH_TIMEOUT_SECONDS` | 4 | `308` |
| `MODEL_PRICE_REFRESH_COOLDOWN_SECONDS` | 300（≥30） | `309-312` |
| `CODEX_EVENT_CACHE_SCHEMA` / `CODEX_COMPACT_ROW_SCHEMA` / `OPENCODEX_COMPACT_ROW_SCHEMA` | 2 / 3 / 101 | `288-290` |
| `CODEX_EVENT_CACHE_HASH_BYTES` / `CODEX_EVENT_CACHE_MAX_ENTRIES` | 4096（≥1024）/ 4096（≥128） | `291-298` |
| Codex/Claude mtime 缓冲；Grok | −2 h；−2 h；0 | `2741`、`8588`、`8988` |
| 日期目录回看 | 62 天 | `2859` |
| `OPENCODEX_*` 窗口 | 180 / 21600 / 60 / 0.05 / 30 / 5（秒） | `374-397` |
| `CODEX_ACCOUNT_MATCH_WINDOW_SECONDS` / `CODEX_CURRENT_ACCOUNT_RECENT_SECONDS` | 600 / 1800 | `373`、`434` |
| `API_SERVICE_VERDICT_ARCHIVE_MIN_TIER` | 2（`max(2, env)`） | `501-504` |
| Cockpit affinity 窗口 | 2.0 / 0.2 / 0.25 / 0.5 / 45.0 | `410-433` |
| OpenCode Go 峰时段 | UTC 01–04、06–10 | `1221-1226` |
| `OFFLINE_HISTORY_BACKFILL_MAX_DAYS`（env `CLIENT_USAGE_OFFLINE_BACKFILL_MAX_DAYS`） | 31（≥0） | `445-448` |
| GUI 轮询三档 | 100 / 250 / 500 ms | `monitor.py:141-154` |
| 全目录补扫 | 30 s（≥5） | `monitor.py:155-158` |
| `LIVE_USAGE_VERIFY_THRESHOLD_TOKENS` | 50 000 000（下限 = 单事件上限） | `monitor.py:183-186` |
| 备份刷新间隔 | 3600 s | `130-147` |
| 分析服务 | `127.0.0.1:8765` | `analytics_server.py:52-53` |

---

## 附录 C：复核记录与待确认清单

### C.1 复核方式

本文的每条断言都经历过两轮独立核对：

1. **分节研究**：五个独立任务各自通读源码成文（来源发现 / 解析 / 去重归属 / 计价 / 聚合输出），
   逐条附 `文件:行号` 与代码摘录，无证据项单列。
2. **交叉复核**：对以下高危断言做了**独立重读或独立实测**（结果见正文）：
   - 成本公式与四桶口径 → 沙箱手算复现（A.2）；
   - fork 重放剔除 → 沙箱 C 会话 delta 验证（A.2）；
   - `input_tokens` 跨来源口径差异 → 源码 + 沙箱 11520 拆解（§4.4、A.2）；
   - 内置价表 vs 在线价表 → 两次沙箱运行 + 原始 payload 键序核对（A.5）；
   - `add_codex_usage`、`_collapse_cockpit_request_usage_events_legacy`、
     文本版 `codex_fork_replay_cutoff` 无调用点 → 全仓 grep（§8.3）；
   - Claude sidechain 不存在 → 全仓 grep（§4.4）；
   - 镜像扣除字段被移除 → `output.pop(...)` 原文（§5.6）。
3. **自动化核对**：脚本提取全文所有「符号名 +（行号）」引用与裸行号做机器校验 ——
   118 个符号引用中 108 个精确命中定义行（其余 10 个是变量赋值/JSON 键/方法调用等
   预期"非定义引用"）；912 个引用行号全部落在对应文件真实行数内（无越界）；
   附录 A.2 的全部数字（17810 token / 0.160445 USD / 9 请求 / 0.091475 + 0.06897 小计）
   经独立 Python 脚本按内置价表重算一致；`client_usage_today.json` 实测 20 个顶层键与
   §7.2 表格一一对应。
4. **对抗性审计**（3 路独立 subagent，覆盖 §1-3 + 附录 B / §4-5 / §6-附录 C）：逐条打开源码
   核对，第 1 路约 179 条断言核对 170 条（≈95%），第 2 路覆盖约 120 处行号引用，第 3 路
   覆盖率 100%；发现的问题与修正如 §C.2。
5. **修正后复审**（第 2 轮，2 路 subagent）：一路验证 §1-§5 的修正落地并重新抽样，
   一路验证 §6-附录的修正落地并重新抽样；同时由主线程重跑符号行号/行号范围/交叉引用/
   重复段落四项自动化核对，结果见 §C.2 末。

### C.2 复核修正记录

**第 1 轮（§1-§3 + 附录 B，约 95% 覆盖）**

| # | 位置 | 复核发现 | 修正 |
| --- | --- | --- | --- |
| 1 | §2.3 | `API_SERVICE_AGGREGATE_LABEL` 标 @493 | 改 @491 |
| 2 | 附录 B | `API_SERVICE_VERDICT_ARCHIVE_MIN_TIER` 标 @508-511（那其实是 `ARCHIVE_LIMIT`） | 改 @501-504 |
| 3 | §3.3 | `parse_json_after_marker` 与三桶行号错位 | 改 @3485 与 `3516`/`3519`/`3543-3545` |
| 4 | §3.8 | 多账号丢弃误标 4391-4409（那是持久副本合并） | 改 `4361-4363`，另标 `4407-4408` |
| 5 | §3.11 | schema 门禁误标 2589-2598（那是 flush 写盘） | 改 `2323-2324`、`2561-2562` |
| 6 | §3.10 | "仅命中 analytics_server.py"漏报 README/前端提及 | 收窄为"读取实现只在分析服务" |
| 7 | §2.1 | 命令构造区间外扩到 4802 | 收窄为 `4785-4790` |
| 8 | 附录 B / §7.6 | 验证阈值区间外扩到 195 | 收窄为 `183-186` |
| 9 | §3.9 | dashboard 字段取用只标了请求行 | 补 `6153-6155` |
| 10 | §1.1 | 表头口径标注过宽 | 限定为 README `217-227` + 源码口径 |
| 11 | §3.8 | 跳过条件不全 | 补 `usable_cockpit_identity(email)` @4350 |

另按复核建议上调 3 处证据等级（§3.5 / §3.7 / §3.12：均有单点直证，原标"一致解释"）。

**第 2 轮（§4-§5，覆盖约 120 处行号引用）**

| # | 位置 | 复核发现 | 修正 |
| --- | --- | --- | --- |
| 1 | §4.2 | `base64(zlib(json))` 三处行号中 `288-298` 指向常量区 | 改为编码 `2361-2371`、解码 `2341-2351`、flush `2589-2601` |
| 2 | §4.1 | "必须逐字节一致的并行实现"性质夸大 | 改为"两侧结构不同（monitor 版无 `canonical_id` 短路、第三段要求调用方预求和），调用侧对齐后才等价" |
| 3 | §5.3 | 级 2 "本次不合并"与代码方向相反 | 改为"**仍合并**为 canonical 事件，状态记 `ambiguous` 并单独计数" |
| 4 | §5.5 | 三重夹取标 `4204-4209`（实为序列化段） | 改 `4188-4194` |
| 5 | §5.6 | monitor 扣除漏掉 `elif` 无条件分支 | 补双分支说明与测试证据（`5401-5404`） |
| 6 | §4.2 | "无论哪条路径都更新 `last_total`" | 补"三桶全 ≤0 的跳过路径除外" |
| 7 | §4.2 | `total_tokens` 签名位误标 `3260` | 删 `3260`，注明全仓无 `signature[5]` 读取点 |
| 8 | §4.3 | 摘录区间与 `when` 回退链 | 改 `3510-3519`、补 `created_at` 二级回退（`3522-3526`） |
| 9 | §4.5/§4.7 | "Grok 无上限"未覆盖 GUI 实时链路 | 补 `monitor.py:179-182`/`13471-13472` 的同名上限 |
| 10 | §4.7 | 失败标条件、非 dict 行行号、位置列缺文件名 | 逐条修正（`9130-9134`、`2435-2436`、`client_usage_export.py:360`） |
| 11 | §4.8 | `ms_to_local_datetime` 异常兜底不对称 | 补述 |
| 12 | §5.1 | alias 强身份短路条件未写 | 补 `3631-3632` |
| 13 | §5.3 | 级 3 "两侧均无 conversation_id"偏严；诊断写入行号 | 改"不同时存在"；写入 `3867-3869` |
| 14 | §5.5 | `preserve_direct_label` 是局部变量；缩水守卫注释行号 | 修正（`11009-11015`、`5429-5431`） |
| 15 | §4.1/§4.4 | `route` 值域漏 `opencodex`；sidechain 命中集合表述 | 补 `4713`；改为"`sidechain` 零命中，`subagent` 另有约 28 处展示标签/测试引用" |
| 16 | §5.7 | docstring 引用非逐字；测试断言措辞 | 补首行原文；改"5 条事件各自的 token 数" |

**第 3 轮（§6-§9 + 附录 A/B/C，覆盖率 100%）**

| # | 位置 | 复核发现 | 修正 |
| --- | --- | --- | --- |
| 1 | §7.6 | "历史成本只在核对成功路径写"与代码方向相反（即时路径 `13797` 也写，仅 60 s 节流） | 重写第 6 点：区分"不改写权威快照"与"会推进 `usage_history.json` 今日条目" |
| 2 | §7.4 | 残余 provider 标签张冠李戴（是硬编码 `"Historical high-water"`，不是 `HIGH_WATER_UNATTRIBUTED_LABEL`） | 修正并注明两者不同 |
| 3 | §6.7 | "不覆盖日志自带 tier"不成立（`cost_multiplier is None` 而 tier 非空时会重写） | 改为按跳过条件精确表述 |
| 4 | §7.1 / §7.3 / §6.7 | 区间端点外扩（`13085-13486`、`12972-13048`、`1568-1589`、`9080-9143`） | 收紧为实际函数范围 |
| 5 | §7.5 | env 名与 Python 常量名混用；"最小检查间隔 3600"漏 clamp 下限；状态文件字段与导出白名单混同 | 逐项校正（含 `total_days` @12325） |
| 6 | §7.6 | 检查点行号偏窄（实际 `_restore_live_usage_checkpoint` @11623、ratio `11702-11741`）；补扫触发条件不全 | 修正 |
| 7 | §7.7 | `origin` 三态行号（`320` 实为 `source` 字面量） | 改为 `163`/`170`/`331`/`786` |
| 8 | §7.8 | 四色条绘制行号；"sync 五态"实为 export_state 五值 + 两个覆盖态 | 修正（`8946-8964`、`8919-8921`） |
| 9 | §8.1 | 镜像扣除的排他性表述无源码支持；两处裸行号实为 `monitor.py` | 改为"键只在 11470 被 pop、折叠只做求和"；补文件名 |
| 10 | §6.10 | "三处账实不符…镜像聚合"标签与行号不对应 | 改为两类补差 + 折叠只求和 |
| 11 | 附录 B | 简写常量名不精确（`…ROW_SCHEMA`、`…RECENT_…`）；两个文件常量混排 | 用全名；表头声明"未标文件名者 = client_usage_export.py"；GUI 行补 `monitor.py:` |
| 12 | §8.2 | Grok 上限只在导出器侧缺失（GUI 实时链路同源受限） | 补 `monitor.py:179-182`/`13471` 与汇聚点 `12707` |

**第 4 轮（第 2 轮复审：修正落地验证 + 全新抽样）**

两路独立 subagent 复核修正后的文档：一路覆盖 §1-§5（27 项修正逐条验证 → 26 项正确应用；
20 条全新抽样全部一致），一路覆盖 §6-附录（12 项修正 → 11 项正确应用；24 组全新抽样 +
附录 B 全表全部一致）。本轮新发现并修复：

| # | 位置 | 复核发现 | 修正 |
| --- | --- | --- | --- |
| 1 | §5.5 | 兜底行号 `8511-8513` 指向写入/落桶段，判定实为 `8504-8509`（markers 分支 `8565-8571`） | 改为 `8503-8511` / `8563-8571` 并分段标注 |
| 2 | §3.10 | cc-switch 命中计数与实测不符（不是"8 处/4 处"） | 去掉具体数字，改为"多处出现但读取实现只在分析服务" |
| 3 | §5.5 / §6.9 | `is_official_codex_quota_model` 只写了"不含 `/`"，漏前缀白名单 | 补 `gpt-/o1/o3/o4/o5/codex-/chatgpt-` |
| 4 | §2.2 | "收口四步"却列了 5 个动作 | 改"落盘收口…（5 个动作）" |
| 5 | §8.2 / §C.4 | "`codex_auth_identity` 无测试锁定"被仓内测试证伪（`tests:156-169` 同时断言两侧） | 改为"monitor 侧自由文本分支无断言" |
| 6 | §6.7 | 覆盖优先级链方向与代码相反（账号级 override 最后写入 = 最高优先） | 按 `5913-5944` 的逐级覆盖顺序重写 |
| 7 | 附录 A.4 | 第 3 次成本 `0.197568` 属**在线价态**，与表中"第 1 次 = 离线"序列不自洽 | 标注两次实验的价格态并给出两种态的对照数字 |
| 8 | 附录 B | "Cockpit affinity 窗口 `410-442`"上限外扩 | 收紧为 `410-433` |
| 9 | §6.10 | `build_live_catchup_payload` 上限偏窄 | 改 `13497-13748` |
| 10 | §7.7 | `missing` 态引 `786`（那是 anomalies 消费点） | 改引产生点 `analytics_server.py:414-415`，另注消费点 |
| 11 | §7.2 / §7.4 | `today` 未提条件字段；高水位"schema 升级"未区分两条独立轴 | 补 `12937-12944`；标注 accounting 轴才是②③的触发条件 |

**第 5 轮（第 3 轮收敛复核：验证 + 对抗搜索）**

两路 subagent 分别复核 §1-§5 与 §6-附录：第 4 轮的 11 项修正 **11 项全部正确落地**；
对抗抽样共 62 条（30 + 32），**未发现任何 P1**，两路均判定"事实可靠性已收敛"。
本轮修完的残余 P2/P3：

| # | 位置 | 复核发现 | 修正 |
| --- | --- | --- | --- |
| 1 | §3.1 | OpenCodex 行只给了外层 ±6h 扫描窗，漏内层 ±180 s 准入 | 补 `4497-4498`/`4514` |
| 2 | §3.10 | cc-switch 命中范围还含 `README.md:253` 与测试 | 补述 |
| 3 | §3.2 | SQL 行号 `2763-2769` | 改 `2762-2767` |
| 4 | §3.11 | flush 组装与写盘未分 | 补 `2589-2600` / `2601` |
| 5 | §2.2 | 步 8 描述含"镜像行归拢"（实际在收口步） | 改为"window_only 标签补零" |
| 6 | §C.4 | 仍引旧的 `8511-8513` 兜底行号 | 同步为 `8504-8509` / `8565-8571` |
| 7 | 附录 A.1/A.2 | "`message.id` 重复三行"与 fixture 不符（实际 2 行） | 改为"重复两行" |
| 8 | §2.3/§7.2 | `providers` 窗口字段漏 `window_rolling_7d` | 补 `13478`/`14002-14003` 与透传点 |
| 9 | 附录 B | `OPENCODEX_*` 区间与 Cockpit affinity 区间各宽 4 行 / 1 行 | 收紧为 `374-397` 与 `410-433` |

**收敛结论**：第 3 轮两路均给出"**无 P1**"，抽样 62 条全部与源码一致，判定文档在
§1-§9 与附录范围内的事实可靠性已收敛；剩余不确定项仅为附录 C.3 列出的
"上游/环境事实"（仓外可证）与附录 A.4 的两项不可复现量（耗时、聚合统计）。

**第 6 轮（存疑项专项复核）**

针对原 §C.3 的 9 条待确认项与附录 A.4 的两项不可复现量（现合并为 §C.3 的 10 条）逐项攻击
（结果见 §C.3 重写版）。
方法：**本机只读 `PRAGMA` 取真实 schema**、下载并统计 LiteLLM / models.dev 真实 payload、
直接 import 源码函数做纯函数实验、`git log -S` + diff 判定实现意图、以及受控沙箱实验。
实质产出：

| # | 对象 | 结论 |
| --- | --- | --- |
| 1 | `apply_opencode_go_time_pricing` | **会重复放峰**：峰价条目在峰时段被再乘一次比例（`1.32 → 2.64`，再喂一次 `5.28`）；**GUI 侧 `monitor.py:1461-1480` 同构实现同样如此**；当前 models.dev 存标准价故未触发 |
| 11 | §6.8 峰时段 | 原文"共 6 小时/天"算错，实测 UTC 小时 1/2/3 + 6/7/8/9 = **7 小时/天**（已改） |
| 2 | 同日高水位 | **归属漂移时重复计入**：受控实验把旧快照的同一份量换到另一行后，`today` 从 9 请求/17810 tokens 跳到 10 请求/20110 tokens。**独立复核复现并加严**：触发门槛实际是"仅时间流逝"（ledger 丢弃 `Unassigned local`/`Codex local`），最坏 2×、污染不自愈、gap 行不会暴露 |
| 3 | 三套 sqlite schema | 本机真实样本证实与代码反推一致（含 `total_cost_usd` 为 TEXT，故 `CAST` 必要） |
| 4 | LiteLLM / models.dev 字段 | 22 项读取名单中的 `above_272k`/`flex`/`batches` 上游**确实提供**（125/244/392 条） |
| 5 | `codex_auth_identity` 两侧差异 | git 历史判定为**同步遗漏**（守卫在 `5178394` 只加在导出器侧，该提交同时改过 `monitor.py`） |
| 6 | 事件缓存 env 一名两用 | `0fb4590` 同一提交内的有意设计 |
| 7 | 检查点转换器 | 无字段白名单，是 datetime→ISO + dict/list 全量递归 |
| 8 | `monitor.py` 降级分支 | 静态确证不可达（模式集合已被前一段消费） |
| 9 | 死代码外部调用 | 仓内（含打包脚本/CI/文档）无引用；另发现第 4 个死代码 `merge_codex_account_fallback_events` |
| 10 | 附录 A.4 耗时 | 冷启动 2.2 s 与热缓存 0.2 s 的差异来自**行缓存是否命中**，与价格源可达性无关 |

**本轮两大发现均经独立复核（2 路 subagent，各自重写脚本、自建目录、不复用报告脚本）**：
重复放峰**未被证伪**（数值逐项吻合，且复核员进一步证实在 `monitor.py:1461-1480` 有同构实现、
补充了 `kimi-k3`/未知模型不缩放等边界）；高水位重复**成立且触发条件比原报告更宽松**
（零干预等 3 分钟即触发、最坏 2×、不自愈），复核员另指出 `restore_today_from_usage_history`
是第二条独立推高路径。复核同时纠正了本文一处算错（§6.8 峰时段为 **7 小时/天**）。

**复算复核**：第 3 轮独立用源码内置价表复算了附录 A.2 的全部数字
（A 0.0243 / B 0.008375 / C 0.0567 / D 0.0021 / Codex 小计 0.091475 / Claude 0.06897 /
合计 0.160445 / 9 请求 / 17810 token），**逐项一致，未发现复算偏差**。

### C.3 存疑项复核结果（第 6 轮：逐项攻击原"待确认清单"）

> 本轮专门针对原先列出的 9 条"上游/环境事实"与附录 A.4 的两项不可复现量逐条复核。
> ✅ 已解决 / ◐ 部分解决 / ⭕ 仍不可证。

1. **上游写入方不在仓内** ◐
   - ✅ **三套读取侧 schema 已用本机真实样本核实**（只读 PRAGMA，未读任何行内容）：
     `~/.codex/state_5.sqlite` 的 `threads` 表确有 `id:TEXT` / `rollout_path:TEXT`（共 42 列，
     还有 `tokens_used`、`cwd`、`model` 等代码未用的列）；`~/.codex/logs_2.sqlite` 的 `logs` 表
     确有 `ts:INTEGER` / `ts_nanos:INTEGER` / `feedback_log_body:TEXT`（12 列）；`~/.cc-switch/cc-switch.db`
     的 `proxy_request_logs`（27 列）/`usage_daily_rollups`（15 列）/`providers`（18 列）列名与
     `LEFT JOIN providers p ON p.id = l.provider_id AND p.app_type = l.app_type` 全部吻合。
     其中 `total_cost_usd` 是 **TEXT** ⇒ 代码必须 `CAST(... AS REAL)`；`request_logs` 另有
     `input_token_semantics` 列（与本文 §8.2 的跨来源口径问题呼应）。
   - ⭕ Sub2API 管理端、Cockpit `request_logs`、OpenCodex `usage.jsonl` 本机无样本
     （`~/.antigravity_cockpit`、`~/.opencodex` 不存在），仍只能由读取侧反推。
2. **LiteLLM / models.dev 字段完备性** ✅ 实测解决：
   - LiteLLM 真实 payload（4123 键）确实提供 `input/output_cost_per_token_above_272k_tokens`（各 125 条）、
     `*_flex`（244/240）、`*_batches`（372/392）、`cache_*_priority` 等 —— 22 项读取名单不是凭空假设；
     另有名单外的 `*_ultrafast`、`*_decisions`、`above_200k`/`above_100k` 变体（被代码忽略）。
   - models.dev（230 provider）的 `cost` 子字段为 `input/output/cache_read/cache_write/tiers/
     context_over_200k/reasoning/input_audio/output_audio` —— 代码只映射前四项。
3. **`apply_opencode_go_time_pricing` 是否重复放峰** ✅ 实验确证（**会重复**）：
   - 传入峰价 `1.32/0.044/3.96` + UTC 01:30 → 返回 **`2.64/0.088/7.92`**（应为 1.32）；传入标准价
     `0.66/…` 才正确放大到 1.32。即实现无条件按 `peak/standard` 比例乘。
   - **当前生产不触发**：`models.dev` 的 `opencode-go/deepseek-v4-pro` 存的是标准价
     `0.66/1.98/0.022`（`deepseek-v4-flash` 为 `0.15/0.6/0.003`），且 LiteLLM 里**没有任何
     `opencode-go/*` 键**（0 个）。只有上游把该渠道条目改成峰价、或用户手工写入峰价缓存才会重复计费。
   - **双侧同源**：`monitor.py:1461-1480` 的 `adjust_live_opencode_go_time_price` 门禁与缩放逐行同构
     （同样没有"已是峰价"判定），GUI 实时链路在用 per-token 峰价时会放大成 `2.64e-06/8.8e-08/7.92e-06`。
   - 对照路径：内置兜底 `opencode_go_official_price_details` 不放大，峰价正确（1.32）。
4. **两侧 `codex_auth_identity` 差异是否有意** ✅ git 历史解决：`usable_cockpit_identity` 守卫由
   `5178394`（"Fix Cockpit attribution and live usage consistency"）引入，只加在
   `client_usage_export.py`；该提交**同时修改了 `monitor.py`（34 行）**却没同步这个函数 ⇒
   判定为**同步遗漏**（两份实现自 `ace7620` 起同源分叉），非有意分叉。
5. **`CLIENT_USAGE_CODEX_EVENT_CACHE` 一名两用** ✅ 已解决：`0fb4590` 的 diff 显示"路径覆盖"
   与"`_is_default_codex_path` 布尔判断"在**同一提交**引入 —— 有意的"自定义路径即强制持久化"设计。
6. **检查点 overlay 字段白名单** ✅ 已解决：`_checkpoint_json_value`（`11071-11081`）**没有白名单**，
   它是"`datetime` → ISO（`CN_TZ`，微秒）+ dict/list 递归"的全量转换器；字段集由 overlay 构造侧决定。
7. **"按比例补齐"的边界** ◐ 检查点比例补齐本身未发现异常；但本轮顺带确证了一个**同族但更严重的
   问题**（见 §8.2 第 10 条：同日高水位在归属漂移时重复计入总量）。
8. **`monitor.py` 降级分支可达性** ✅ 静态确证不可达：`{"fallback","auto-sub2api"}` 已被前面的
   `if self.mode in {"", "auto", "fallback", "auto-sub2api"}` 分支消费并提前返回，故 `5949` 的
   `if` 恒为 False —— 到达 `5946` 的 mode 只剩 `sub2api`/`server` 等，`fetch_sub2api_state`
   抛异常时只会 `raise`。
9. **死代码是否被外部脚本按名调用** ◐ 全仓（含 `build-windows.ps1`、`packaging/*.iss`、
   `.github/**`、`docs/**`、`tests/**`）对 `add_codex_usage`、`_collapse_cockpit_request_usage_events_legacy`、
   文本版 `codex_fork_replay_cutoff` 的引用**只有定义行**；仓外调用不可证。本轮另发现
   第 4 个死代码：`merge_codex_account_fallback_events`（`8323-8342`，无调用点）。
10. **附录 A.4 的两项不可复现量** ✅ 已复现/归因：
    - 耗时：热缓存 0.65 s（墙钟）/ 0.2 s（导出器自报）与原文 0.56/0.57 s 同量级；
      **冷启动**补测 2.5 s（墙钟）/ 2.2 s（自报，含首次建立行缓存）。差异来源是**行缓存是否命中**，
      与价格源可达性无关（离线 2.5 s ≈ 在线首次 2.6 s）。
    - 聚合统计（118 符号 / 108 命中 / 912 行号）属本文自评，可由文档内脚本复现，非仓内事实。

### C.4 与 README 声明的逐条对照

| README 声明 | 代码核验 |
| --- | --- |
| "去重逻辑：保留 Codex fork replay 去重"（第 30 行） | ✅ 成立，作用点在 `scan_codex_events` 内（§5.2） |
| "Sub2API mirror 扣除；`api-service-local`/`codex_local_access_runtime` 仅合并为一个展示项，不再重复扣除"（第 30-31 行） | ✅ 成立：`collapse_api_service_mirror_providers` + `output.pop("api_service_mirror_deduction")`（§5.6） |
| "使用稳定的 `message.id` 合并…缺少 `message.id` 时只使用行 UUID 去重，不按相同 Token 数模糊合并"（第 134-137 行） | ✅ 成立（`8635-8643`、`8660-8670`） |
| "`CLIENT_USAGE_MAX_SINGLE_EVENT_TOKENS` 用来过滤异常大的单次 token 事件"（第 164 行） | ✅ 成立，但**只覆盖 Codex/Claude**（§4.7） |
| "超过 272K 输入上下文时仍沿用当前 tier 的普通价格，不应用长上下文加价字段"（第 194 行） | ✅ 成立（`*_above_272k*` 从不被读取） |
| "Models.dev 只对 openai/anthropic/xai/deepseek 建立无前缀别名；opencode-go 始终使用独立计价键"（第 185-186 行） | ✅ 成立（`981-1003`、`1079-1082`） |
| "待归因期间 Token 仍计入总量，但不会借用上一个账号"（第 174 行） | ✅ 成立（`11047`；注意 `8504-8509` / `8565-8571` 有一处"最近 30 分钟内用当前账号"的例外窗口） |
| "只有活动 provider 明确为 Cockpit API 服务时，才读取 `.cockpit_codex_auth.json`"（第 75 行） | ✅ 成立（`4081-4116`） |

> 结论：README 的统计口径声明与代码一致；唯一的补充是
> `input_tokens` 的跨来源混合语义（§4.4）与 Grok 缺少上限过滤（§4.7）这两点 README 未提及。
