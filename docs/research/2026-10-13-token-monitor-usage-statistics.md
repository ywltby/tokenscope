# Token Monitor 用量统计实现拆解（源码级）

> **研究对象**
> - **TM** = [`Javis603/token-monitor`](https://github.com/Javis603/token-monitor)，HEAD
>   `ac4eeb2c4aea239dd94b67f074c73f69aafc46f8`（2026-10-13，`chore: release v0.70.0`，
>   `package.json` version `0.70.0`）。Electron 43 桌面小部件，纯 JavaScript。
> - **TK** = 它 vendored 的 Rust 扫描器 tokscale 的下游 fork
>   [`Javis603/tokscale`](https://github.com/Javis603/tokscale)，pin
>   `fa5f0c13324838d1a988fda317597f9b90da7ad0`（2026-10-10，基线 v4.18.0，release tag
>   `token-monitor-fa5f0c13`）；上游为 [`junhoyeo/tokscale`](https://github.com/junhoyeo/tokscale)。
> - 两仓均浅克隆到临时目录后逐文件精读；**未运行任何二进制、未改动本机任何数据目录**。
>
> **证据分档**：**已验证**（源码直读，给出 `路径:行号`）／**一致解释**（多条代码证据指向同一
> 结论，无单点直证）／**未见实现**（明确检索后判定「本 pin 不存在该机制」的否定结论）／
> **存疑**（无法从静态源码定论，集中列在 §9）。TM 内路径相对 token-monitor 根，TK 内路径相对
> tokscale 根。
>
> **本文只记录结构、字段名、路径规则与计算口径，不含任何真实会话内容。**

## 1. 结论速览

1. **Token Monitor 自己不解析绝大多数 agent 的日志**。37 个受管客户端（`src/shared/clientCatalog.js:50-95`）
   的 token 用量统一由 vendored 的 Rust CLI tokscale 扫描；TM 是「数据源发现 + 监视 + 增量 +
   归档 + 跨设备同步 + UI + 限额」的外壳。
2. 采集方式是**子进程 + JSON**：TM spawn tokscale 二进制，读它的 stdout JSON。主扫描 argv 固定为
   `--json --client <csv> --group-by <grouping> [--today|--month|--since D]`
   （`src/shared/collector.js:572`）；另有 `graph --client <csv> --no-spinner`（`:615`，TM 用于热力图）
   与 `pricing <id> --json --no-spinner`（`:636`，单模型查价）。
3. TK 的 `--json` 报告**不是** `commands/report.rs` 产出的（那是 `tokscale report` 的 wiki/任务归因
   路径），而是 `crates/tokscale-cli/src/main.rs:2324-2477` 内联的 serde 结构体；顶层含
   `groupBy/entries/sessions/workspaces/6 个 total*/processingTimeMs`（`main.rs:2383-2388`），entry 携带
   `client, mergedClients（恒输出、可为 null）, workspaceKey?, workspaceLabel?, sessionId?, model,
   provider, input, output, cacheRead, cacheWrite, reasoning, messageCount, cost, performance`。
4. **六桶 token 模型**：TK 的 `TokenBreakdown`（`crates/tokscale-core/src/lib.rs:264-301`）为
   `input / output / cache_read / cache_write / cache_write_1h / reasoning`，其中 `cache_write_1h`
   是 `cache_write` 的子集（Anthropic 1 小时缓存 TTL 价，不重复计入总量）。
5. 一次**完整 tick** 串行扫三个窗口：`--today` → `--month` → `--since <allTimeSince>`
   （`src/shared/collector.js:1060-1078`）；源码注释给出理由——并发会把峰值 CPU/IO 抬到三倍
   （`:1061-1062`，issue #15 曾把 tokscale 顶到 500% CPU）。
6. **watch tick 靠增量恒等式省两次扫描**：只扫 `--today`，再用
   `applyPeriodDelta(base, freshToday, anchorToday) = Σ max(0, base + fresh − anchor)`
   （`src/shared/usage.js:1901-1968`，数值核心 `:1944`）推出 month/allTime。追加型日志下这是**恒等**
   而非估计；锚点只在其日期键与当天一致时可用（`:1895-1899`、`src/shared/collector.js:957-961`）。
7. 锚点持久化在 `sharedDataDir()/collector-anchor.json`（`src/shared/collector.js:2666`、`:2714`），
   含 `dateKey / configFingerprint / fullScanAt / today / month / allTime / wslBundle`；日期键或
   配置指纹不符即整体失效、退回全扫。
8. **每个 agent 的读取方言都在 TK 的 `crates/tokscale-core/src/sessions/` 里**（约 70 个模块），
   每个客户端一份解析器：JSONL 流式解析（Claude/Codex/Gemini/Kimi/Qwen/Grok/DSH/Muse/…）、
   SQLite 只读查询（OpenCode/Zed/Kilo/Hermes/WorkBuddy/Cherry Studio/Devin/…）、
   API 或本地缓存导出（Cursor 的账户级 usage 缓存、Antigravity 的 protobuf 生成库、
   Copilot 的 OTEL / data.db / session-store.db / VS Code chatSessions 四路）。
9. TK 注册了 **56 个 client id**（`crates/tokscale-core/src/clients.rs:320-1106`，`ClientId::COUNT == 56`
   见 `:1220`），其中 TM 的 catalog 只启用 37 个；fork 新增 3 个上游不认的客户端
   （`proma`、`catpaw`、`qodercn`，见 `crates/tokscale-core/src/token_monitor/`），并给
   `mcode` 补了上游缺失的本地运行时库解析。
10. **金额在 TK 里逐条消息算好**（`apply_pricing_if_available`，`crates/tokscale-core/src/lib.rs:5823-5845`），
    聚合阶段只做求和（`--json` 路径 `crates/tokscale-core/src/lib.rs:4644-4650`，
    `graph` 路径 `crates/tokscale-core/src/aggregator.rs:289`）。
11. 价格来源是**三份远程数据集 + 一份用户自定义 + 四张编译进二进制的硬编码表**：
    LiteLLM、OpenRouter、models.dev（`crates/tokscale-core/src/pricing/mod.rs:416-427`，缓存文件
    `<config_dir>/cache/pricing-{litellm,openrouter,models-dev}.json`，TTL 3600s，
    `pricing/cache.rs:6`），加上 Cursor / Sakana / archive / self-hosted 的兜底表
    （`pricing/mod.rs:123,227,285`、`pricing/self_hosted.rs:7`）。本 pin **没有**编译期打包的价格数据集
    （未见 `include_str!` 价格数据）。
12. 匹配优先级：**自定义价（命中即返回）→ self-hosted 零价 → 路由标签拒赔 → 别名 → 精确/归一化/
    前缀（三源仲裁）→ Cursor/Sakana 兜底 → archive → 模糊 → 未命中（None）**
    （`pricing/mod.rs:570-586`、`pricing/lookup.rs:619-815`、`:821-1156`、`:3637-3691`）。
13. **自定义价格是 TM 唯一写进 tokscale 的输入**：写到 `<tokscale config dir>/custom-pricing.json`
    （`src/shared/tokscaleConfig.js:32-56`），五档键名与 TK 的反序列化结构一一对应
    （`src/shared/tokscaleCustomPricing.js:40-52`）。TM 不传 CLI 参数、不设价格环境变量。
14. **价格/二进制一变，成本锚即失效**：`pricingFingerprint()` 把价格文件字节 + 二进制
    identity + 二进制 `stat(size/mtime/ctime)` 拼进 `configFingerprint`，每个 tick 开头比对，
    变化即清锚点、重置全扫与 history 时间戳（`src/shared/collector.js:2151-2186`、`:2774-2779`）。
    扫描途中价格变化（混合 revision）时本轮**不发布**，最多自动重放一次（`:2949-2951`、`:3185-3190`）。
15. **TM 自己算钱的唯一路径是 Codex DOTS 本地账本**：订阅本机 Codex 执行器的 tokenUsage
    （WebSocket），落自有 SQLite，再用 `lookupModelPricing` + 本地价格表算成本
    （`src/shared/providers/codex/localUsageSource.js`、`localUsageStore.js`、`localUsage.js`）。
16. **TM 自己读的其余数据都不是用量**：会话标题、项目归属、上下文占用、prompt cache 时窗、
    live 活动状态、限额。它们通过与 tokscale 同名的路径表（`src/shared/clientSources.js`）做
    watch/健康/诊断，由 provider 级解析器补齐元数据。
17. **未知模型不得按 0 静默丢弃**：TK 对未命中价格的模型保留 `cost_source`（`Unknown`）且成本 0，
    由上层展示「未知」；只有提交（leaderboard）路径才强制要求价格覆盖
    （`crates/tokscale-core/src/sessions/mod.rs:71-78`、`pricing/mod.rs:659-669`）。
18. 缓存写价分档：`cache_write_1h` 与 5 分钟档分开计价，1h 档使用
    `cache_creation_input_token_cost_above_1hr`，阈值 200k
    （`crates/tokscale-core/src/pricing/lookup.rs:2337-2459`）。
19. **本 pin 未见峰谷/时间窗定价**，也不存在外置 `[[model.segment]]` 配置或「多候选取最高价」
    逻辑（对照 TokenScope 的三来源候选计价，这是两者最显著的口径差异）——见 §7 与 §9。
20. 多设备场景下，TM 把每台设备压成一条 `DeviceRecord`（周期数据 + 限额），Hub 读时做
    `normalizeDeviceRecord → 跳过过期周期 → 逐项相加 → cost toFixed(6)`
    （`src/shared/usage.js:1800-1900`）；本地模式不经过 Hub。

## 2. 总体架构：Electron 外壳 + vendored Rust 扫描器

### 2.1 两层职责

| 层 | 载体 | 职责 | 关键实现 |
| --- | --- | --- | --- |
| 采集/解析/计价/分组 | Rust CLI `tokscale`（fork，pin 到具体 commit） | 发现各 agent 的本地数据、解析方言、逐条消息计价、按维度聚合、输出 JSON | `crates/tokscale-core/src/{scanner,sessions,pricing,aggregator}.rs` |
| 编排/增量/归档/同步/展示 | Electron（主进程 + worker 线程 + renderer） | 源发现与 watch、增量刷新、归档、Hub/iCloud 同步、限额、UI | `src/shared/collector.js`、`src/shared/usage*.js`、`src/electron/*` |

TM 通过 `ensure:tokscale`（`package.json:14`）在 `start/agent/pack/dist:*` 前把 npm 安装的
tokscale 平台包替换为 pin 的 fork 构建（`scripts/ensure-vendored-tokscale.js`），运行时用
`resolvePlatformBinary()` 解析到原生二进制（优先 `@tokscale/cli-<platform>/bin/tokscale[.exe]`，
缺失时回退 `tokscale/bin.js` shim 并以 `ELECTRON_RUN_AS_NODE=1` 执行）
（`src/shared/collector.js:96`、`:108-136`、`:210-229`）。

> **已验证**：`scripts/vendor/tokscale.json` 里 `mode: "override"`、`fork: Javis603/tokscale`、
> `commit: fa5f0c13…`、`baseVersion: 4.18.0`；pin 的校验（sha256、`--version` 冒烟、原子改名）
> 只发生在安装/打包路径，运行时读到的是「声明的构建元数据」而非字节校验
> （`scripts/ensure-vendored-tokscale.js:137-165`、`src/shared/collector.js:131-136`）。

### 2.2 一次采集的端到端数据流（Local 模式）

```text
文件系统变化 / 定时 tick
  └─ collector.startCollector()  ──┬─ spawn tokscale --json --client ... --group-by ... --today
                                   ├─ spawn tokscale ... --month（完整 tick 才有）
                                   ├─ spawn tokscale ... --since <allTimeSince>（完整 tick 才有）
                                   └─ spawn tokscale graph --client ...
        ↓ stdout JSON
  usage.js collectUsageRows()（深挖）→ extractUsageBundleFromTokscale()
        ↓
  period{today,month,allTime} + byClient 分区
        ↓ provider 元数据补齐（标题/项目/上下文/prompt cache/父会话）
  applySessionMetadata() / sessionLive
        ↓
  onUpdate(summary) → usageTransform（worker 线程）→ 设备记录 / 归档 / Hub 上传 / UI
```

### 2.3 与「一个应用一份统计」的区别

TM 不做 per-agent 的统计分离：所有客户端解析完都变成同一种 `UnifiedMessage`
（`crates/tokscale-core/src/sessions/mod.rs:81-125`），之后聚合、计价、分组的代码路径完全共用。
新增一个 agent = 在 TK 里加一个 parser + 在 `clients.rs` 注册 + （可选）在 TM 里加数据源路径与
元数据解析器。

## 3. 采集管道（TM 侧）

### 3.1 命令面与参数

**已验证**（`src/shared/collector.js`）：

| 用途 | argv | 行号 |
| --- | --- | --- |
| 周期扫描 | `--json --client <csv> --group-by <grouping> --today` | `:572`、`:1012`、`:1063` |
| 同上（本月 / 全部） | `--month` ／ `--since <allTimeSince>` | `:1070`、`:1075` |
| 热力图 | `graph --client <csv> --no-spinner` | `:615` |
| 单模型查价 | `pricing <modelId> --json --no-spinner`（默认 15s 超时） | `:633-637` |
| WSL 扫描 | 在上述基础上追加 `--home <distro-home>` | `src/shared/wslUsage.js:163-167` |

注意 `runTokscale` 的 argv **不带** `--no-spinner`（只有 `graph`/`pricing` 带）；TM 依赖
`parseJsonOutput()` 容错：先整体 `JSON.parse`，失败则从第一个 `{` 或 `[` 截断重试
（`src/shared/collector.js:231-241`）。

`tokscaleCommand()` 生成 `{bin, prefixArgs, env, identity}`，并做两件环境处理
（`src/shared/collector.js:210-229`）：

- `TOKSCALE_EXTRA_DIRS`：把用户自定义扫描根规范成 `client:dir` 逗号串（`src/shared/customScanPaths.js:129`）。
- **空白环境变量整键删除**：`XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`TOKSCALE_HEADLESS_DIR` 等键若为空白，
  会按平台大小写规则删除后再传给子进程（`src/shared/collector.js:179-208`），避免下游把
  「设了但为空」当成显式重定向。

### 3.2 一次完整 tick：串行三窗口

`collectUsageOnce()`（`src/shared/collector.js:870`）在同一个时钟快照 `collectedAt`（`:873-877`）
下依次执行：

```js
const todayJson = await runTokscaleFn({ clients: normalizedClients, flags: ['--today'], ... });   // :1063
const monthJson = await runTokscaleFn({ clients: normalizedClients, flags: ['--month'], ... });   // :1070
const allTimeJson = await runTokscaleFn({ clients: normalizedClients, flags: ['--since', allTimeSince], ... }); // :1075
```

串行是刻意的：`// Serial on purpose: concurrent scans triple the peak CPU/IO load, which is what let
the issue #15 self-trigger loop spike tokscale past 500% CPU.`（`:1061-1062`）。每次扫描完成后
`emitProgress` 让 UI 递增式刷新（`:1068-1074`）。

`--since` 的值来自 `allTimeSince`（`:872`、`:1075`），TM 侧把它固定成一个足够早的日期，
使「全部时间」在三窗口模型里可表达。

### 3.3 watch tick：增量恒等式

锚点可用时（`anchorUsed`，条件：`anchor.dateKey === localTodayKey(collectedAt)` 且分区可寻址，
`:957-961`），tick 只做一次 `--today` 扫描，然后：

```js
month   = applyPeriodDelta(anchor.month,   today, anchor.today);   // :1058
allTime = applyPeriodDelta(anchor.allTime, today, anchor.today);   // :1059
```

`applyPeriodDelta` 的语义（`src/shared/usage.js:1901-1968`）：

- 数值字段：`Math.max(0, base + fresh − anchor)`（`:1943-1944`）——因为日志是追加型的，
  「基线 + （新今天 − 旧今天）」是恒等式，不是估计（注释 `:1895-1899`）。
- 递归作用于 base/fresh/anchor 键的并集（`:1946-1966`），所以新增字段不需要逐字段登记。
- 特例：`promptCache` 是快照语义（`fresh === undefined` 保留 base，显式 null 才清空，`:1918`）；
  `tokenComponents` 需要 base 与 fresh **两方**都为真（`:1919-1924`），`throughput` 需要
  base/fresh/anchor **三方**都为真（`:1925-1930`）；
  `startedAt` 取更早、`lastUsedAt` 取更晚（`:1931-1942`）；`parentSessionId` + `parentClient`
  作为一条原子关系处理（`:1957-1964`）。

跨天（stale 锚点）时本轮不再走 `todayOnly` 路径，退回三窗口全扫（`src/shared/collector.js:3534`
附近的循环分支），这就是「本地日期键」必须与 history 的日键同源的原因
（`const localTodayKey = localDayKey`，`:643`）。

### 3.4 定向分区扫描与安全阀

watch 事件只会命名「变化的路径」，TM 把它们映射回 client 集合，在一次 union 的 `--today` 扫描里
覆盖（`src/shared/collector.js:1008-1012`）。结果按 client 切成 `todayPartitions`，只替换变化分区，
再 `mergeTodayPartitions()` 合成公开的 `today`（`:1047-1050`、`:2121-2130`）。

安全阀（`:1018-1040`）：整套判定只在**本轮确实是定向扫描**（`targetRequested`）时生效（`:1032`）。
触发条件是三者之一：分区里出现**有用量的未归属行**（`periodHasUsage()`，`:1016`、`:2132-2137`）；
出现非目标 client 的已归属行；或**多个目标**中某个分区缺失（`targetClients.length > 1`，`:1018-1021`）。
触发后不做逐 client 修补，而是立刻用全量 client 重扫一次 `--today`，整轮替换（`:1037-1040`）。
单个目标的分区缺失**不**触发退化：`replaceTodayPartitions()` 会把它置为 `emptyPeriod()`
（`:2121-2125`）。

### 3.5 监视策略与降级

- chokidar **只在 fork 出的子进程**里运行（`src/shared/watcherHost.js:56`）：macOS 上每文件
  一个描述符，达到 `OPEN_MAX` 会让后续 tokscale spawn 全部 `EBADF`（issue #520）；子进程随 IPC
  断开退出。
- debounce 会被 `watchMaxWaitMs`（默认 5s，下限等于 debounce）封顶，避免「写个不停时永远不触发」
  （`src/shared/collector.js:2246`、`:3285`）。
- 描述符耗尽（`ENOSPC`/`EMFILE`/`ENFILE`）→ 整个进程切换为轮询（sticky，`:3350`）；
  轮询需先统计覆盖条目数，超过 `WATCH_POLLING_ENTRY_LIMIT`（20000）时**直接不装 watcher**，
  报 `watcher-interval-fallback` 退化为固定间隔（`:2307`、`:2403`）。
- self-sync 写出的 tokscale 缓存目录**不被 watch**（`:1422-1471`）：只有 TM 自己会写它们，
  watch 会自激。watch 的是它们的原生来源根。
- 自定义扫描根会剪掉依赖/VCS 目录（`node_modules`、`.git` 等）——只影响 watch 覆盖面，
  tokscale 全扫仍会走进去（`:1667-1694`）。

### 3.6 WSL 与 self-sync

- **WSL**（仅 Windows）：以 `HKCU\…\Lxss` 注册表键为门控，`wsl --list --quiet --running` 枚举
  **运行中**的发行版，逐个扫 `\\wsl$\<distro>\home\*` 与 `\root`（`src/shared/wslUsage.js:35-120`），
  每个 home 的 today/month/allTime 三次扫描仍串行（`:163`）。结果在 `deriveClientStatus` 之前
  合并回 Windows periods（`src/shared/collector.js:1115-1160`），因此「只在 WSL 里出现」的客户端
  也能被正确判为 active。watch tick 复用冻结的 WSL 快照，避免每次几秒都重扫 WSL。
- **self-sync**：只有 `cursor` 与 `antigravity` 两个客户端需要 TM 主动同步
  （`tokscale cursor …` / `tokscale antigravity sync`，`src/shared/collector.js:1424`；
  `src/shared/providers/antigravity/selfSync.js:195`、`src/shared/providers/cursor/auth.js:201-202`），
  由 `selfSyncThrottle`（进程级单例，跨 collector 重建共享）限流。

### 3.7 子进程生命周期与失败语义

`spawnTokscaleJson()`（`src/shared/collector.js:243-312`）对每个 tokscale 子进程：

- 超时或 abort → 请求终止；`SIGTERM` 后仍不退 → `SIGKILL`；再确认不到 `close` →
  发出 `subprocess-termination-unconfirmed` 诊断（`:880-888`，终止链实现
  `src/shared/subprocessTermination.js:92-95`）。
- 非 0 退出时抛带 `tokscaleExitCode`/`tokscaleStderr` 的错误（`:301-306`），上层据此区分
  「未知 client（exit 2）」「非法 group-by」等可恢复情况。
- 输出上限：stderr 只保留 64 KiB（`:315`），stdout 在终止后停止累积（`:285-288`）。

**失败即不发布**：tick 失败时不产出新 summary（`src/shared/collector.js:3108-3133` 附近的错误路径），
已发布的快照由 `src/shared/usage/deviceState.js` 按字段 carry，避免「一次失败把界面清零」。

## 4. 多个 agent 应用的本地数据是怎么被读的（tokscale 侧）

### 4.1 客户端注册表：56 个 id

`crates/tokscale-core/src/clients.rs:320-401` 用 `define_clients!` 宏声明注册表，每个条目是
`{id, root, relative_path, pattern, headless, parse_local, submit_default}`，宏同时生成
`ClientId` 枚举、`ClientId::ALL`、显示名与 logo 常量；条目数量由测试固定为 56
（`:1220`，`assert_eq!(ClientId::COUNT, 56)`）。

`root` 有 6 种语义（`:1-18`、`:97-214`）：`Home`（用户 home）、`XdgData`（`$XDG_DATA_HOME`）、
`Config`（`$XDG_CONFIG_HOME`）、`AppData`（平台 per-user app data）、`ReasonixHome`（专用多环境规则）、
`EnvVar { var, fallback_relative }`（例如 `CODEX_HOME` → `.codex`）。路径拼接刻意逐段 push，
避免 Windows 混用分隔符（`:20-29`、`#1048`）。

**没有查表式的 dispatch**：真正决定「哪个 client 走哪个解析器」的是
`crates/tokscale-core/src/lib.rs:5948` 附近的逐 lane 手写调用（例如 claude/codex/opencode 各一段），
注册表只提供 id 与路径元数据（`:374-401`）。

### 4.2 统一记录模型

所有解析器产出同一种记录（**已验证**）：

```rust
// crates/tokscale-core/src/sessions/mod.rs:81-125
pub struct UnifiedMessage {
    pub client: String, pub model_id: String, pub provider_id: String,
    pub session_id: String, pub workspace_key: Option<String>, pub workspace_label: Option<String>,
    pub timestamp: i64, pub date: String, pub tokens: TokenBreakdown, pub cost: f64,
    pub cost_source: CostSource, pub service_tier: Option<String>, pub duration_ms: Option<i64>,
    pub message_count: i32, pub agent: Option<String>, pub dedup_key: Option<String>,
    pub session_title: Option<String>, pub parent_session_id: Option<String>,
    pub is_turn_start: bool, pub model_attribution_conflicted: bool,
}
```

`TokenBreakdown`（`crates/tokscale-core/src/lib.rs:264-301`）六桶，`total()` 用饱和加法；
`cache_write_1h` 明确是 `cache_write` 的**子集**，不重复计入。`CostSource`
（`sessions/mod.rs:71-78`）= `Unknown | ProviderReported | Estimated`，是「这条成本从哪来」的
唯一权威标记（解析器上报 vs 价格表估算）。

### 4.3 主流客户端逐个

#### Claude Code（`sessions/claudecode.rs`）

- 数据面：`<CLAUDE_CONFIG_DIR|~/.claude>/projects/**/*.jsonl`（`clients.rs:414-426`）。
- 桶映射：`message.usage.input_tokens / output_tokens / cache_read_input_tokens /
  cache_creation_input_tokens`（`claudecode.rs:696-709`）。
- 1 小时缓存写档单独取 `cache_creation.ephemeral_1h_input_tokens`（`sessions/utils.rs:677`、`:738-744`）。
- 去重键：`"<message.id>:<requestId>"` 组合（`claudecode.rs:632-636`），跨文件重复观测按字段
  取大合并（合并规则 `:988-1045`）。
- 不按字符估算 token：tool_result 之类的文本即使缺 usage 也不进估算
  （`claudecode.rs:450-458`；对照 TK issue #1011）。
- Advisor / 迭代用量从 `usage.iterations` 拆出（`:829-871`）。

#### Codex CLI（`sessions/codex.rs`）

- 数据面：`<CODEX_HOME|~/.codex>/{sessions,archived_sessions}/**/rollout-*.jsonl*`
  （`clients.rs:427-439`，`*.jsonl*` 兼容整文件压缩 `.zst`，压缩文件有 256 MB 上限，
  `sessions/utils.rs:544-551`）。
- 桶字段：`input_tokens / output_tokens / cached_input_tokens / reasoning_output_tokens`
  （`codex.rs:153-162`）；`reasoning` 在 Codex 里是 output 的**子集**，解析时必须拆出（`:257-278`）。
- 增量读法主路径只采信 `last_token_usage`（累计快照 `total_token_usage` 用于去重与单调性校验，
  `:914-917`、`:954-970`）；若某行只有累计值而无 `last`，存在 **total-delta 降级分支**（`:973-985`）。
- 去重键形如 `codex:token_count-total:…`（`:1371-1380`），fork 额外做父作用域处理（`:1072-1090`）。
- **不存在独立的「压缩会话」文件格式**：`.zst` 是整文件压缩；compaction 只是改写
  `total_token_usage` 快照 —— 这一点纠正了「压缩 transcript 是另一种解析路径」的常见误解
  （存疑项见 §9）。

#### OpenCode（`sessions/opencode.rs` + `opencode_schema.rs`）

- 两代 SQLite 布局：v1 `message` + `session`，v2 `session_message` + `session_v2`，两代都读
  `data` 列里的 JSON（`opencode_schema.rs:345-420`）。
- token 路径：`$.tokens.{input,output,reasoning}` 与 `$.tokens.cache.{read,write}`（`:155-173`）；
  模型取 `modelID` 或嵌套 `model.id`（`:94-109`）。

#### Cursor（`sessions/cursor.rs` + TM 的 self-sync）

- **不是**解析 Cursor 本地日志，而是 Cursor 服务端的账户级用量导出：TM/TK 侧通过
  `tokscale cursor …` 同步到 `~/.config/tokscale/cursor-cache/`（`usage*.json` / `usage*.csv`），
  再由 `cursor.rs` 读取（`cursor.rs:1-5`；端点见 `crates/tokscale-cli/src/cursor.rs:119-120`）。
- JSON 用 `conversationId` 归会话；CSV 每行合成一个 `cursor-{account}-{date}` 的「日会话」
  （`:377-387`、`:530-535`）；成本优先取 `totalCents`（`:391-401`）。

#### Antigravity（`sessions/antigravity.rs`、`antigravity_cli.rs`）

- CLI 面：`<GEMINI_CLI_HOME|~/.gemini>/antigravity-cli/conversations/*.db`
  （`clients.rs:744-756`），解析器自写 protobuf reader 读 `gen_metadata`
  （`antigravity_cli.rs:312-411`）：`input = #1 + #2`、`cache_read = #5`、`output = #9`、
  `reasoning = #10`。
- IDE 扩展面：`~/.gemini/antigravity/conversations/*.db`（`clients.rs:1095-1105`），
  TM 侧另有摘要库读取（`src/shared/providers/antigravity/sessionMetadata.js`）。
- **未见 ledger reconciliation**：上传/对账式的 `usageLedger` 机制在 `sessions/amp.rs:125-241`，
  与 Antigravity 无关。

#### GitHub Copilot（四路来源）

| 来源 | 路径 | 解析 | 关键点 |
| --- | --- | --- | --- |
| OTEL JSONL | `<COPILOT_OTEL_FILE_EXPORTER_PATH>` 或 `~/.copilot/otel/*.jsonl` | `copilot.rs:553-577` | 读 `gen_ai.usage.*` 属性 |
| CLI `data.db` | `~/.copilot/data.db` | `copilot_desktop.rs:45-51`、`:241-257` | shutdown 累计值差分 |
| `session-store.db` | `~/.copilot/session-store.db` | `copilot_session_store.rs:16-100` | 会话存储 |
| VS Code chatSessions | `workspaceStorage/*/chatSessions` | `copilot_vscode.rs:271-346` | 编辑器侧记录 |

四路结果按 `session_id` 与 `(session, timestamp)` 过滤合并（`lib.rs:2579-2644`）。

#### Gemini CLI / Zed / Kiro / Droid

- Gemini（`gemini.rs`）：`<GEMINI_CLI_HOME|~/.gemini>/tmp/**`，键名容忍多种别名，
  cache 桶要判断方向（`:51-81`、`:517-543`）。注意：TM 的 catalog **不管理** gemini
  （只有标签预留，`src/shared/clientCatalog.js:104-114`），但 TK 会解析。
- Zed（`zed.rs`）：只统计 `provider == "zed.dev"` 的行，dedup 键 `zed:{id}`（`:8-10`、`:163`）。
- Kiro（`kiro.rs`）：以**估算**为主（没有真实逐轮计数时按字符估算 + credit 折算），
  credit × 0.04 的换算见 `:36-41`；估算逻辑 `:381`、`:410-412`、`:606`。
- Droid（`droid.rs`）：会话级累计值按轮次分摊到各消息（`:474-481`、`:486-545`）；
  TM 另外读 `session-discovery-index.json` 拿标题/项目。

### 4.4 全量客户端表（56 个 id）

下表的 `id`、默认路径与文件模式逐字来自 `crates/tokscale-core/src/clients.rs:403-1106`
（`root` 缩写：H = `Home`，XDG = `XdgData`，CFG = `Config`，APP = `AppData`，ENV = 环境变量根）；
「token 来源」来自 `sessions/` 下对应解析器（**真实** = 源里就有 usage 字段，
**估算** = 由文本长度或会话级总额推导，**无** = 恒零 token）。

| id | 显示名 | 默认根 | 相对路径 | 模式 | token 来源 |
| --- | --- | --- | --- | --- | --- |
| `opencode` | OpenCode | XDG | `opencode/storage/message` | `*.json` | 真实（SQLite v1/v2） |
| `claude` | Claude Code | ENV `CLAUDE_CONFIG_DIR`｜H `.claude` | `projects` | `*.jsonl` | 真实 |
| `codex` | Codex CLI | ENV `CODEX_HOME`｜H `.codex` | `sessions` | `*.jsonl*` | 真实 |
| `cursor` | Cursor IDE | H | `.config/tokscale/cursor-cache` | `usage*.json\|usage*.csv` | 真实（账户导出） |
| `gemini` | Gemini CLI | ENV `GEMINI_CLI_HOME`｜H `.gemini` | `tmp` | `*.json\|*.jsonl` | 真实 |
| `amp` | Amp | XDG | `amp/threads` | `T-*.json` | 真实 + 账本对账 |
| `droid` | Droid | H | `.factory/sessions` | `*.settings.json` | 真实（会话级分摊） |
| `openclaw` | OpenClaw | H | `.openclaw/agents` | `*.jsonl*` | 真实（SQLite `transcript_events` + 旧 JSONL） |
| `pi` | Pi | H | `.pi/agent/sessions` | `*.jsonl` | 真实 |
| `kimi` | Kimi CLI | H | `.kimi/sessions` | `wire.jsonl` | 真实 |
| `qwen` | Qwen CLI | H | `.qwen/projects` | `*.jsonl` | 真实（`usageMetadata`） |
| `roocode` | Roo Code | H | `.config/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks` | `ui_messages.json` | 真实 |
| `kilocode` | Kilo Code | H | `.config/Code/User/globalStorage/kilocode.kilo-code/tasks` | `ui_messages.json` | 真实 |
| `mux` | Mux | H | `.mux/sessions` | `session-usage.json` | 真实 |
| `kilo` | Kilo CLI | XDG | `kilo/kilo.db` | `kilo.db` | 真实 |
| `crush` | Crush | XDG | `crush/projects.json` | `projects.json` | **无**（仅 session 成本按日摊） |
| `hermes` | Hermes Agent | ENV `HERMES_HOME`｜H `.hermes` | `state.db` | `state.db` | 真实（五桶 + 实际/估算成本） |
| `copilot` | Copilot CLI | H | `.copilot/otel` | `*.jsonl` | 真实 |
| `goose` | Goose | XDG | `goose/sessions/sessions.db` | `sessions.db` | 真实 + 推断 reasoning |
| `codebuff` | Codebuff | ENV `CODEBUFF_DATA_DIR`｜H `.config/manicode` | `projects` | `chat-messages.json` | 真实 |
| `antigravity` | Antigravity | CFG | `antigravity-cache/sessions` | `*.jsonl` | 真实（自同步缓存） |
| `zed` | Zed Agent | XDG | `zed/threads/threads.db` | `threads.db` | 真实（仅 `provider=zed.dev`） |
| `kiro` | Kiro | H | `.kiro/sessions/cli` | `*.json` | **估算**为主 + credit 折算 |
| `trae` | Trae | CFG | `trae-cache/sessions` | `*.json` | 真实（API 缓存） |
| `warp` | Warp | CFG | `warp-cache` | `usage*.json` | **无**（请求数 + 花费分） |
| `cline` | Cline | H | `.config/Code/User/globalStorage/saoudrizwan.claude-dev/tasks` | `ui_messages.json` | 真实 |
| `gjc` | Gajae-Code | ENV `GJC_CODING_AGENT_DIR`｜H `.gjc/agent` | `sessions` | `*.jsonl` | 真实 |
| `grok` | Grok Build | ENV `GROK_HOME`｜H `.grok` | `sessions` | `updates.jsonl` | 真实（`logs/unified.jsonl` 优先） |
| `jcode` | Jcode | ENV `JCODE_HOME`｜H `.jcode` | `sessions` | `session_*.json` | 真实 |
| `commandcode` | Command Code | H | `.commandcode/projects` | `*.jsonl` | **估算**（transcript 无 usage 时） |
| `micode` | MiMo Code | XDG | `mimocode` | `*.db` | 真实（OpenCode schema） |
| `antigravity-cli` | Antigravity CLI | ENV `GEMINI_CLI_HOME`｜H `.gemini` | `antigravity-cli/conversations` | `*.db` | 真实（protobuf） |
| `junie` | Junie | H | `.junie/sessions` | `events.jsonl` | 真实 |
| `zcode` | ZCode | H | `.zcode/projects` | `*.jsonl` | 真实（缺失时估算） |
| `opencodereview` | OpenCodeReview | H | `.opencodereview/sessions` | `*.jsonl` | 真实 |
| `codebuddy` | CodeBuddy | H | `.codebuddy/projects` | `*.jsonl` | 真实 |
| `workbuddy` | WorkBuddy | H | `.workbuddy` | `workbuddy.db` | 真实（SQLite 回退把单值记 input） |
| `devin-cli` | Devin CLI | XDG | `devin/cli/sessions.db` | `sessions.db` | 真实（`$.metadata.metrics`） |
| `devin-desktop` | Devin Desktop | H | `Library/Application Support/Devin/User/acp-events` | `*.ndjson` | 多数行无 usage |
| `senpi` | Senpi (OmO Native) | ENV `SENPI_CODING_AGENT_DIR`｜H `.senpi/agent` | `sessions` | `*.jsonl` | 真实 |
| `augment` | Augment Code | H | `.augment/sessions` | `*.json` | 真实（只有 finishedAt，末端锚定） |
| `kimchi` | Kimchi | ENV `KIMCHI_CODING_AGENT_DIR`｜H `.config/kimchi/harness` | `sessions` | `*.jsonl` | 真实 |
| `reasonix` | Reasonix | `ReasonixHome` | `stats` | `*.jsonl` | 真实（transcript 故意不扫） |
| `prime-agent` | Prime Agent | ENV `PRIME_AGENT_CODING_AGENT_DIR`｜H `.prime/agent` | `sessions` | `*.jsonl` | 真实 |
| `freebuff` | Freebuff | ENV `FREEBUFF_DATA_DIR`｜H `.config/manicode` | `projects` | `chat-messages.json` | 真实（缺失时估算） |
| `cherrystudio` | Cherry Studio | APP | `CherryStudio/.claude/projects` | `*.jsonl` | 真实（专用去重解析器） |
| `dsh` | DeepSeek Harness | ENV `DSH_HOME`｜H `.dsh` | `sessions` | `dsh-session-log` | 真实（`.zstd` 按帧魔数判压缩） |
| `mcode` | MiniMax Code | CFG | `headless/mcode` | `*.jsonl` | 真实（headless 捕获；fork 另补运行时库） |
| `fx` | Fx | H | `.fx/sessions` | `usage-v2.json` | 真实 |
| `omp` | Oh My Pi | H | `.omp/agent/sessions` | `*.jsonl` | 真实（根固定，避免与 Pi 双计） |
| `lmstudio` | LM Studio | ENV `LM_STUDIO_HOME`｜H `.lmstudio` | `server-logs` | `*.log` | 真实（本地推理，零货币成本） |
| `unsloth` | Unsloth | ENV `UNSLOTH_STUDIO_HOME`｜H `.unsloth/studio` | `studio.db` | `studio.db` | 真实 |
| `hindsight` | Hindsight | ENV `HINDSIGHT_HOME`｜H `.hindsight` | `usage` | `*.jsonl` | 真实（`hindsight sync` 镜像账本） |
| `micode-desktop` | Xiaomi MiMo AI | XDG | `mimocode` | `*.db` | 真实（按 `session.version` 前缀拆分） |
| `muse` | Muse Code | XDG | `muse/sessions` | `session.jsonl` | 真实 |
| `antigravity-extension` | Antigravity IDE Extension | H | `.gemini/antigravity/conversations` | `*.db` | 真实（protobuf） |

**一致解释**：`micode` 与 `micode-desktop` 指同一批 `mimocode*.db`，解析时按 `session.version`
是否以 `desktop-` 开头拆成两个 client（`sessions/micode.rs:27-38`、`clients.rs:1060-1075`）；
TM 侧把两者合并成一行 `mimo`（`src/shared/tokscaleClientMapping.js:14-29`）。

### 4.5 估算与零 token

- **唯一的字符估算公式**：`chars.div_ceil(4)`（`crates/tokscale-core/src/sessions/utils.rs:540-542`），
  即 1 token ≈ 4 字符、向上取整。
- 使用者集中在少数解析器：`commandcode.rs:303`（内容按 canonical JSON 序列化后计字符）、
  `zcode.rs:159-160`、`freebuff.rs:184-185`、`kiro.rs:606`。它们只在源**完全没有** usage 时触发；
  已上报的「全零桶」不会被估算覆盖（`commandcode.rs:309-311`、`:345-353`）。
- Claude Code 解析器显式**关闭**了字符估算（`claudecode.rs:559`、`:1359-1365`）。
- `goose` 的 reasoning 不是估算 token，而是桶推断 `max(total − input − output, 0)`，
  源码自标 `INFERRED, not a real field`（`goose.rs:118-126`）。
- 恒零 token 的解析器：`crush`（`crush.rs:7-12`）与 `warp`（`warp.rs:67-118`），
  它们只贡献成本/请求数。

### 4.6 去重与跨源合并

- Claude：跨文件重复观测按「字段取大」合并（`claudecode.rs:988-1045`）；
  Cherry Studio 用专用解析器按稳定 request/message id 去重（`cherrystudio.rs`，注释在
  `clients.rs:919-935`）。
- Codex：`last_token_usage` 增量（缺 `last` 时退化为 total-delta，`codex.rs:973-985`）
  + `token_count-total` 级去重键（`codex.rs:914-917`、`:1371-1380`）。
- Copilot：四源按 session/timestamp 过滤（`lib.rs:2579-2644`）。
- Antigravity：CLI 与扩展两面对同一 generation 的 responseId 做跨面去重
  （`clients.rs:1091-1094`）。
- OpenClaw：同一 agent 的 SQLite 与旧 JSONL 双来源合并（`openclaw.rs:603`、`:736-777`）。

## 5. TM 自己读的那部分

### 5.1 数据源发现：一张表管 watch / 健康 / 诊断

`src/shared/clientSources.js`（514 行）不是读取实现，而是**路径表**：`clientSourceRoots()`
为每个已启用客户端列出根，规整为 `{id, dir, sourcePath?, optional?, custom?}`
（`:149-165`）；`sourcePath` 表示「真实来源是单个文件、但要 watch 它的父目录」（Copilot 的
`data.db`/`session-store.db`、`kilo.db`、`zcode` 的 `db.sqlite`、`grok` 的 `unified.jsonl`、
`unsloth` 的 `studio.db`、Devin 的 `sessions.db`）。

要点：

- **刻意镜像** TK 的 `PathRoot` 语义，包括 XDG 归属硬规则：只有 TK 声明为 `XdgData` 的根才跟
  `XDG_DATA_HOME`；Kiro CLI 的 SQLite **故意不跟**（TK 写成 home 相对字面量）
  （`src/shared/clientSources.js:181-196`）。
- 一个 client 可以对应多条路径、多种拼写（Copilot 的 `workspaceStorage` 每平台一种、
  Kiro globalStorage 四种），因为「这一处缺失」本身就是要展示的诊断事实（`:142-148`）。
- Windows 上的 canonical 化是**防崩溃**：8.3 短路径/junction 会让 libuv 的
  `ReadDirectoryChangesW` 断言直接 abort 整个进程，所以交给 chokidar 的必须是
  `realpathSync.native` 的长路径；其他平台该函数是恒等（`:24-45`）。
- 忽略/剪枝策略集中在 `watchPolicyEntries()` 与 `watchIgnoreMatcher()`
  （`src/shared/collector.js:1694-1933`）：按客户端白名单保留（例如 Hermes 只保留 SQLite
  三件套、opencode 只保留 `storage/message/*/*.json`），Kiro IDE globalStorage 这类大树
  **只做区间扫描、不挂 watcher**（`:1394-1401`）。
- 自定义扫描根上限：全局 64 个、每客户端 16 个，拒绝含 `,`/`\0`/换行的路径，因为
  `TOKSCALE_EXTRA_DIRS` 是逗号分隔且无转义（`src/shared/customScanPaths.js:10-12`、`:69-71`）。
  fork-only 客户端（proma/catpaw/qodercn）**不认** `TOKSCALE_EXTRA_DIRS`（`:17-37`）。

### 5.2 会话元数据解析（TM 自己实现）

TK 的用量报告带有 `sessions[]`（title/firstActiveMs/lastActiveMs/parentSessionId）与
`workspaces[]`，但 TM 仍然自己补齐一批本地元数据，因为要覆盖 TK 不解析的维度
（上下文占用、prompt cache 时窗、目录归属、turn 状态）：

| 客户端 | TM 侧实现 | 读什么 |
| --- | --- | --- |
| Claude Code | `src/shared/providers/claude/sessionMetadata.js` | `~/.claude/projects` 转录：标题、项目归属、上下文、prompt cache 观测 |
| Codex | `src/shared/providers/codex/sessionMetadata.js`、`transcript.js` | SQLite `threads` 表 + rollout 转录：标题/上下文/turn 状态 |
| Droid | `src/shared/providers/droid/sessionMetadata.js` | `session-discovery-index.json`：标题与项目 |
| DSH | `src/shared/providers/dsh/sessionFiles.js` | `.zstd`/`.jsonl` 会话文件：标题、上下文 |
| Reasonix | `src/shared/providers/reasonix/sessions.js` | native sidecar 元数据（不是 token 来源） |
| Kimi / Grok / Codebuddy / Workbuddy / Devin / Qoder CN … | 各自的 `providers/<id>/` 目录 | `state.json`、`summary.json`、扩展日志等 |

设计约束（`docs/architecture.md` 的「Collector pipeline」段，属**文档口径**）：Claude 与 Codex
的 prompt-cache 观测与既有会话元数据同一遍读取完成，不新增独立转录读取器；元数据解析失败按会话
跳过而不是整轮失败。

### 5.3 live 活动探测（只覆盖两个客户端）

`src/shared/sessionActivityProviders.js:10-13` 声明只有 `claude` 与 `codex` 有 live 适配器：

- Claude：读 `~/.claude/sessions/<pid>.json` PID 注册表 + 进程启动时间，反 PID 复用
  （`src/shared/providers/claude/sessionActivity.js:45-105`）。
- Codex：SQLite `threads` 表的 `rollout_path` + 文件 mtime + 转录尾部 `turnEnded`
  （`src/shared/providers/codex/sessionActivity.js:45-103`、`:139-147`）。

观察值 TTL 30 秒、运行窗口 10 分钟（`src/shared/sessionLive.js:41-42`），续期间隔 10 秒。
活动是**独立的覆盖层**（`period.sessionActivity`），不进入用量记账，也不随用量上传
（`docs/architecture.md` 口径；实现见 `sessionLive.sessionWithActivity()`）。

### 5.4 Codex DOTS 本地账本：TM 唯一自算成本的地方

TM 通过 WebSocket（`providers/codex/localUsageSource.js:16` 的
`wss://codex-cloud-backend.chatgpt.com/`）订阅本机 Codex 执行器上报的 `tokenUsage`，
写入自己的 `codex-local-usage.sqlite`（`providers/codex/localUsageStore.js:8-10`），
再用 `lookupModelPricing()`（`src/shared/collector.js:633-637`）查到的单价在本地算成本
（`providers/codex/localUsage.js:7-20`）。`lookupModelPricing()` 在本仓只有三个使用点，都是这条通道：
设置页价格预填（renderer `app.js:16749` → `main.js:7468-7474`）、Dots tick 内的账本计价
（`src/shared/collector.js:1221-1228` 注入 → `providers/codex/localUsage.js:111`）、
Dots 会话详情计价（`main.js:8023-8028` → `sessionDetailResolver.js:119-125` →
`localUsage.js:153-184`）。

这条路径与 tokscale 的扫描**并存**：DOTS 账本在原生/WSL 扫描之后合并，且不参与原生增量锚点与
分区扫描（`docs/architecture.md` 口径）。它是「TM 自己产生 token 计数 + 自己算钱」的唯一实现，
也是理解 TM 与纯扫描工具差异的关键。

### 5.5 客户端健康与限额目录

- 健康判定：`source.state = checkedCount === 0 ? 'unknown' : detectedCount > 0 ? 'detected' : 'missing'`
  （`src/shared/collector.js:2044-2048`）；`clientHealth.overall` 的顺序固定为
  「sync 失败（attention）→ 源缺失（unavailable）→ 有用量（healthy）→ waiting」，
  未知值落到 `unknown`（`src/shared/clientHealth.js:313-333`）。
- 「检测到但 0 用量」是独立诊断码 `no-usage-observed`（`src/shared/collector.js:2074-2080`）。
- 限额是另一套目录：29 个 `LIMIT_PROVIDER_CATALOG`（`src/shared/limits/providers.js:35-65`），
  与用量客户端有 5 处 id 不同名（droid→factory、zcode→zai、qodercn→qoder、dsh→deepseek、
  mcode→minimax，`:74-80`）。**8 个 provider 只有限额、没有用量**：TypeSafe、OpenRouter、
  Volcengine、Ollama、Trae CN、Alibaba Cloud、StepFun 与「第三方 API」
  （README.zh-CN.md 的用量列为 `—`，代码侧 `limits/providers.js:57-64`）。

## 6. 聚合

### 6.1 tokscale 侧：解析期计价 + 分组聚合

- **聚合键**：`GroupBy` 枚举（`crates/tokscale-core/src/lib.rs:207-261`）决定分组维度，字符串形如
  `client,model`（默认）、`session`、`client,session`、`workspace,model`、
  `client,workspace,session,model` 等；聚合时按各维度拼接 key（`:4567-4584`）。
- **求和**：桶用饱和加法、cost 直接相加（`:4644-4650`），条目按 cost 降序（`:4674-4682`）。
  **cost 在解析期就算好**（见 §7），聚合阶段没有任何重算或多候选取价。
- **时间过滤**在消息层完成：`date >= since && date <= until`（`lib.rs:5658-5673`）；
  CLI 先把 `--today/--week/--month` 换算成本地日串（`crates/tokscale-cli/src/main.rs:1914-1949`）。
  「全部时间」由 TM 传一个足够早的 `--since` 表达，而不是特殊 enum。
- **会话化**（`sessionize.rs`）把消息按 3 分钟空闲切成会话区间，**只服务 time metrics**，
  不影响用量记账（`sessionize.rs:8`、`:155-226`；调用点 `lib.rs:5228-5240`）。
- **graph 子命令**（`main.rs:4851-4992`、`:5561-5653`）恒输出 JSON：按日的
  `contributions` + `summary`/`years`/`timeMetrics`；TM 用它渲染热力图/趋势
  （`src/shared/collector.js:812-819`）。这一路的日聚合由 `aggregator.rs` 的 rayon
  `fold/reduce` 完成（`crates/tokscale-core/src/aggregator.rs:14-44`）。
- **扫描调度**：任务列表 + rayon `into_par_iter`，worker 数取 `min(核数, 4)`
  （`scanner.rs:1989`、`:2906-2927`）；坏路径丢弃（`:459-474`）、JSONL 坏行 `continue`
  （`sessions/claudecode.rs:312-315`）、SQLite 坏行跳过（`sessions/utils.rs:373-376`）。
- **缓存**：256 分片 bincode 缓存（`message_cache.rs:49-74`、`:2735-2817`），带指纹与采样哈希；
  另有 `recovery.rs` 作为本地灾难恢复叠加层，只补缺口（`recovery.rs:1-24`）。

### 6.2 TM 侧归一化：深挖 + 别名 + 分区

- **深挖**：`collectUsageRows()` 递归遍历 tokscale JSON，`looksLikeUsageRow()` 判定叶子
  （token 或 cost 非零，且带任一身份字段），命中即停、不再下钻（`src/shared/usage.js:563-583`）。
  这使 TM 不依赖 TK 的固定布局：entry 数组、`contributions`、`sessions` 映射都能被同一段代码吃下。
- **字段别名表**（`src/shared/usage.js:17-46`）：token 总量 7 个别名、分量 26 个（camelCase 与
  snake_case 双写，如 `cacheRead`/`cache_read_tokens`/`cachedTokens`）、cost 6 个、会话 id 7 个、
  起始/最后使用时间戳分别 4/7 个；`asNumber()` 能解析 `"$1,234"` 这类字符串（`:55-62`），
  `firstNumber()` 遇到 0 会继续找下一个键（`:64-73`）。
- **一次遍历两条通路**：`extractUsageBundleFromTokscale()` 同时产出公开 period 与内部
  `byClient` 分区（键为客户端 id，未归属用 `__unattributed`，`src/shared/usage.js:1097`、`:1217-1237`）。
- **period 形状**由 `emptyPeriod()` 唯一决定（`:226-274`）：四桶（cacheRead/cacheWrite/output/
  unclassified）+ 总量 + cost + client 维度映射 + model 维度映射 + client×model 交叉映射 +
  `projects` + `sessions`。`unpricedTokens` 不是预置字段，由 `addUnpricedTokens()` 懒添加
  （`:365-368`、`:1153-1154`）。
- **会话主键**：`${client}:${sessionId}`（`:559-561`）；同会话跨行合并走 `mergeSession()`
  （`:638-751`），保留最早的 `startedAt` 与最新的 `lastUsedAt`。
- **空结果的诚实表达**：`fallbackUsagePeriod()` 只保留总量与 cost，并把
  `capabilities.tokenComponents/throughput` 置为 `totalTokens === 0`，`unclassifiedTokens` 记下
  无法分类的量（`:1200-1211`）——避免把「聚合级输出」误当成分桶精确值。

### 6.3 三个时间窗与增量

三个窗口 `PERIODS = ['today','month','allTime']`（`src/shared/usage.js:5`）是 TM 的对外形状：
collector 在顶层发布 `today/month/allTime`（`src/shared/collector.js:1258-1276`），
设备记录归一化时再收进 `periods.*`（`src/shared/usage.js:1330-1334`）。

增量规则见 §3.3；补充两条：

- **锚点信任**：`collectorAnchorTrust()` 校验锚点的日期键、配置指纹与结构完整性，任何不符即
  弃用（`src/shared/collector.js:2216-2260`）。
- **时钟与跨日**：一次 tick 只取一次 `collectedAt`，同时用于 today 的日键与 `updatedAt`，
  防止跨午夜的两个窗口配对（`:873-877`）；History 的日键与该实现同源（`:643`）。

### 6.4 归档：三套，各管一件事

| 归档 | 载体 | 内容 | 写入时机 |
| --- | --- | --- | --- |
| 会话用量归档 | SQLite `session-usage-archive.sqlite`（`src/shared/usage/sessionUsageArchiveStore.js:206-218` 建表） | 每会话行的用量快照，供历史会话列表/详情 | 每个 tick 增量写；游标是单调 revision（写 `storedRevision + 1`，`:308-347`；读 `WHERE revision > ?`，`:121-137`） |
| 客户端用量归档 | 设置内 JSON（`src/shared/usage/clientUsageArchive.js`） | 被用户取消勾选的客户端的历史用量 | 唯一写入时机是取消勾选（`src/electron/main.js:2773-2780`） |
| 日历史归档 | JSON `daily-history-archive.json`（`src/shared/dailyHistoryArchive.js`） | 热力图/趋势按日留存（`days` 与 `liveDays` 两套） | 每次 tick 追加；无头 agent 活跃时让出写入权（`:746-750`、`src/shared/usage/usageWorker.js:62-64`） |

**已验证**（复核 A）：`sessionUsageArchiveStore.js` 是唯一 SQLite；`clientUsageArchive` 与
`dailyHistoryArchive` 都是 JSON，不是数据库。

History（热力图/连续天数）数据来源 = tokscale `graph` 扫描 + 日归档留存
（`src/shared/collector.js:786-863`）；streak 计算在 `src/shared/history.js:316-339`。

### 6.5 跨设备聚合（可选路径）

只有 Hub / iCloud 模式才涉及多设备；Local 模式完全不经过这一层。Hub 侧聚合是**读时**发生：

```js
// src/shared/usage.js:1800-1900（aggregateDevices）
每台设备 → normalizeDeviceRecord() → 跳过过期周期 → 数值逐项相加 → cost 用 toFixed(6)
```

细节（**已验证**）：stale 阈值 = `max(staleAfterMs, 2 × syncUploadIntervalMs)`（`:1783`、`:1869`
附近）；today/month 是否过期取决于设备自带的 `periodWindows.endsAt`（由设备用本地时区算好，
`src/shared/collector.js:654-665`），这样离线设备不会把昨天的「今天」永久累加。
呈现层把本机记录覆盖到 Hub 快照上（同 deviceId 替换），并保留 `hubReceivedAt` 以便区分
「本机采集成功」与「上传成功」（`src/electron/syncDisplayStats.js:121`、`:140`）。

## 7. 金额是怎么算出来的

### 7.1 TL;DR

```text
解析器给出 token 桶 + 可能的供应商上报成本
  ├─ 有权威成本（cost_source = ProviderReported）→ 直接用，不再定价
  └─ 否则 → PricingService 定价：
        custom-pricing.json → self-hosted 零价 → 路由标签拒赔 → 别名
        → 精确/归一化/前缀（LiteLLM/OpenRouter/models.dev 三源仲裁）
        → Cursor/Sakana 兜底 → archive → 模糊匹配 → 未命中（成本 0，标记 Unknown）
   然后 × 服务档倍率（Zed 1.1、OpenAI fast 2.0）
聚合阶段：cost 直接求和，不再重算
```

### 7.2 价格来源

| 来源 | 载体 | 位置 |
| --- | --- | --- |
| LiteLLM | 远程数据集（HTTP） | 缓存 `<config_dir>/cache/pricing-litellm.json` |
| OpenRouter | 远程数据集 | 缓存 `…/pricing-openrouter.json` |
| models.dev | 远程数据集 | 缓存 `…/pricing-models-dev.json` |
| 用户自定义 | `custom-pricing.json` | TM 写入，TK 读取 |
| Cursor / Sakana / archive / self-hosted | **编译进二进制**的硬编码表 | `pricing/mod.rs:123`、`:227`、`:285`；`pricing/self_hosted.rs:7` |

三源并发抓取（`tokio::join!`，`pricing/mod.rs:416-427`），单源失败不致命：逐源降级到
**任意年龄**的旧缓存（`pricing/mod.rs:435-489`）；缓存 TTL 3600 秒、原子替换写
（`pricing/cache.rs:6`、`:43-49`、`:86-96`）；抓取超时 30 秒 / 连接 10 秒，退避
200/400/800ms 最多 3 次（`pricing/fetch.rs:4-13`）。离线开关
`TOKSCALE_PRICING_CACHE_ONLY=1` 只读缓存（`lib.rs:5896-5911`）。
本 pin **没有**编译期打包的价格数据集（未见价格 `include_str!`）。

### 7.3 模型匹配与别名

匹配顺序（`pricing/mod.rs:570-586`、`pricing/lookup.rs:619-815`、`:821-1156`、`:3637-3691`）：

1. 自定义价（`lookup_custom`，命中即返回 `source: "Custom"`）；
2. self-hosted 的零价表；
3. 路由标签拒赔（例如某些 reseller 键不允许作为定价依据）；
4. 别名表（`pricing/aliases.rs`，例：`big-pickle`→`glm-4.7`、`k2p6`→`kimi-k2.6`、
   `kimi-for-coding`→`kimi-k2.7-code`、`k3-256k`→`kimi-k3`、`minimax-m3`→`minimax/MiniMax-M3`、
   `MODEL_PLACEHOLDER_M26`→`claude-opus-4-6`、`cursor-grok-4.6-xhigh`→`grok-4.6`，
   `aliases.rs:13-18`、`:24`、`:29`、`:36`、`:42`、`:63`、`:65`）；
5. 精确 → 归一化 → 前缀匹配，三源同时命中时用 `choose_best_source_result()` 仲裁；
6. Cursor / Sakana 兜底表 → archive → 模糊匹配 → `None`。

`provider_identity.rs` 负责供应商拼写归一（阻止把区域变体（如 `alibaba-cn`）折叠进总部键），
影响选价与证据等级；`model_alias.rs` 明确只做**展示合并**、不影响计价。

### 7.4 费率结构、分档与缓存写价

- 费率是 **per-token USD** 的 17 字段结构（`pricing/litellm.rs:10-28`）：input / output /
  cache read / cache write 各有 128k / 200k / 256k / 272k 分档，加上 4 个「1 小时缓存写」字段。
  models.dev 与自定义价格的 per-million 输入会除以 1e6（`pricing/models_dev.rs:8`、`:95-114`；
  `pricing/custom.rs:375-378`）。
- 分档计价默认**分段累加**（`tiered_cost`，`pricing/lookup.rs:2310-2335`）；仅 OpenAI 272k 与
  xAI 200k 例外——它们按高档位对**整段**重算（`:2220-2298`）。272k 的判定用
  `input + cache_read + cache_write` 合计。某一档的费率缺失时是**沿用当前（低）档价**，
  不是按 0 计（`pricing/lookup.rs:2317-2319`）。
- **缓存写分档**：`cache_write_1h` 是 `cache_write` 的子集（`min` 截断），1h 部分用
  `*_above_1hr` 价、阈值 200k；1h 桶在缺少 `above_1hr` 费率时，**只有当 `above_1hr_above_200k`
  档存在**才会借用普通 cache-write 价，否则整段按一次普通价计（`pricing/lookup.rs:2337-2459`、
  `:2425-2430`）。`reasoning` 在计价时并入 output（同段代码）。
- 倍率：Zed 托管 ×1.1、OpenAI fast/priority ×2.0（`lib.rs:5702-5731`、`:5762-5784`）。

### 7.5 自定义价格（TM → TK 的唯一写路径）

**TM 侧**（**已验证**）：

- 路径 `<tokscale config dir>/custom-pricing.json`；目录优先级 `TOKSCALE_CONFIG_DIR` >
  macOS `~/.config/tokscale` > Windows `%APPDATA%\tokscale` > Linux
  `$XDG_CONFIG_HOME|~/.config/tokscale`（`src/shared/tokscaleConfig.js:32-56`）。
- 五档键名（`src/shared/tokscaleCustomPricing.js:40-52`）：

  | TM 设置字段 | 写出的 TK 键 |
  | --- | --- |
  | `inputPerM` | `input_cost_per_million_tokens` |
  | `outputPerM` | `output_cost_per_million_tokens` |
  | `cacheReadPerM` | `cache_read_input_token_cost_per_million_tokens` |
  | `cacheWritePerM` | `cache_creation_input_token_cost_per_million_tokens` |
  | `cacheWrite1hPerM` | `cache_creation_input_token_cost_per_million_tokens_above_1hr` |

- 三态单价：`''`/`null`/缺失 = 未设置（不写键）；有限且 ≥0 保留（`0` = 显式免费）；
  其他值**整条丢弃**；且每条至少要有 input 或 output（`:9-13`、`:30-31`）。
- 写盘是「合并 + sidecar 记账」：保留用户在同一个文件里手写的其他模型条目，只删除本机上次
  管理、本次不再拥有的 id；sidecar 为 `userData/tokscale-managed-pricing.json`
  （`:56-63`；`src/electron/main.js:6491`）。无覆盖且无历史状态时**不创建文件**（`:79-81`）。
- 写入链：renderer 保存设置 → `settings:update` → `applySettingsPatch` → 价格数组变化时
  `regenerateTokscalePricing()` 并触发一次 `tick('manual', { forceHistory: true })`
  （`src/electron/renderer/app.js:16789`、`src/electron/main.js:7488`、`:7736`、`:6495`）。

**TK 侧**：在构造 `PricingService` 时按默认路径读取该文件（`pricing/mod.rs:430`），custom 表置于
所有目录源之前（`:560`、`:692`）。覆盖语义是「整条消息成本替换，但要求价格覆盖该行的**全部**
已填充桶」——`calculate_custom_cost()` 先做 `covers_usage()` 检查，不覆盖则返回 `None`
（保留来源成本），覆盖成功才写 `message.cost` 并标记为估算，且**不叠加**服务档/托管倍率
（`pricing/mod.rs:636-657`、`lib.rs:5803-5821`）。显式 0 是合法价
（`pricing/litellm.rs:141`、`pricing/custom.rs:90-104`）。

### 7.6 未知模型与「不猜」

- 未命中价格：解析期成本为 0，`cost_source` 保持 `Unknown`（`crates/tokscale-core/src/sessions/mod.rs:71-78`），
  聚合只把 0 加进去；上层展示时必须按「未知」呈现，而不是当作免费。
- 只有提交（leaderboard/submit）路径才强制要求价格覆盖，并把不完整成本记为
  `incomplete_cost_dates`（`lib.rs:5378-5443`、`pricing/mod.rs:659-669` 的
  `covers_usage_with_provider`）。
- `model_attribution_conflicted` 的行（供应商证据互相矛盾）**不做定价**，保持未定价
  （`lib.rs:5803-5809`），避免用错误模型名猜价。

### 7.7 TM 侧与成本相关的三处规则

1. **成本锚失效**（§3.1 提到的 `pricingFingerprint`）：价格文件字节 + 二进制 identity + 二进制
   `stat(size/mtime/ctime)` 变化 → 锚点作废、强制全量重扫（`src/shared/collector.js:2151-2186`、
   `:2774-2779`）。
2. **混合 revision 不发布**：扫描过程中价格文件变化时本轮结果丢弃并最多自动重放一次
   （`:2949-2951`、`:3185-3190`）。
3. **归档成本与实时成本的边界**：只存在于归档里的记录保留其记录成本；`graph` 扫描出的新价只在
   价格 revision 变化或显式 reprice 且 token 数一致时替换归档值
   （`src/shared/dailyHistoryArchive.js:486-498`）；被取消勾选客户端的归档用量则直接把自己的
   `costUsd` 加回周期（`src/shared/usage/clientUsageArchive.js:243-244`）。

## 8. 关键不变量（实现者视角）

以下是这套实现**必须保持**的约束，破坏任一条都会产生静默错误：

1. **一次 tick 一个时钟**：`collectedAt` 在扫描前取一次，同时用于 today 日键与 `updatedAt`
   （`src/shared/collector.js:873-877`）。
2. **今天窗口与增量锚点必须同源**：`localTodayKey = localDayKey`（`:643`），跨日必须全扫
   （`src/shared/usage.js:1895-1899`）。
3. **增量恒等式只在追加型日志下成立**：`max(0, base + fresh − anchor)` 的语义是「新今天」，
   若来源日志会被重写/回收，则该恒等式不成立（`:1901-1913`）。
4. **分区键是客户端 id**：定向扫描按 client 分片，重建时只替换变化分区；分区缺失不能用
   「零」冒充（`src/shared/collector.js:2121-2130`、`replaceTodayPartitions()`）。
5. **价格或二进制变化必须让成本锚失效**：否则会出现「旧价算旧数、新价算新数」混合展示
   （`:2151-2186`、`:2774-2779`）。
6. **扫描期间的价格变化不允许发布**：必须整轮重放（`:2949-2951`、`:3185-3190`）。
7. **未归属用量不能静默丢弃**：`__unattributed` 分区是显式兜底（`src/shared/usage.js:1097`、
   `:1220-1226`）。
8. **聚合级输出不能伪装成分桶精确值**：`capabilities.tokenComponents/throughput` 是布尔
   provenance，`tokenComponents` 需要 base 与 fresh 都为真（`src/shared/usage.js:1919-1924`），
   `throughput` 需要 base/fresh/anchor 三方都为真（`:1925-1930`）。
9. **未知模型不得按 0 计**：`cost_source` 三态必须一路传到展示层
   （`crates/tokscale-core/src/sessions/mod.rs:71-78`）。
10. **自定义价格只覆盖它声明覆盖的行**：`covers_usage()` 不满足时保留来源成本
    （`crates/tokscale-core/src/pricing/mod.rs:636-657`）。
11. **source 目录不得重复计费**：`micode`/`micode-desktop` 是同一批 db 的两个身份，靠
    `session.version` 前缀拆分（`sessions/micode.rs:27-38`）；`omp` 故意不认 `PI_CODING_AGENT_DIR`
    以防与 `pi` 双计（`clients.rs:990-1006`）。
12. **self-sync 产出的缓存目录不能被 watch**：否则形成自激循环（`src/shared/collector.js:1422-1471`）。
13. **扫描串行**：并发扫描会成倍抬高峰值 CPU/IO，破坏「3–5 秒感知延迟」的稳定性（`:1061-1062`）。
14. **归档不因来源消失而删数据**：来源文件被删、客户端被停用，历史归档仍保留
    （`docs/architecture.md` 口径 + 三套归档实现）。
15. **同步记录里不含会话正文**：设备记录只带用量与身份，`sessions[]` 在跨进程前被剥离，
    renderer 按需单独拉取（`src/shared/usage.js:184-225` 的 `stripSessionTextFromPeriod` /
    `stripSessionTextFromDeviceRecord`）。

## 9. 未验证与存疑

静态阅读无法定论、或上下游契约依赖外部实现的地方，逐条列出：

1. **`exit code 2` 的语义没有在 TK 源码中找到显式设置**：TM 把它当作「未知 client」的判据
   （`src/shared/collector.js:316-318`），但 TK 侧 `--client` 是 clap `value_enum`
   （`crates/tokscale-cli/src/main.rs:1401-1412`），同文件只见到 `exit(1)`
   （`main.rs:683-685`）。**一致解释**：exit 2 由 clap 的参数校验失败产生；上游若改错误码，
   TM 的能力探测与重试会整体失配。
2. **`crates/tokscale-core/src/lib.rs:5708` 的注释提到「bundled LiteLLM dataset」，与本 pin
   未见打包价格数据矛盾**（疑为历史遗留注释）。本文按「无编译期价格数据集」表述，
   若上游另有构建特性注入数据，此处需要修正。
3. **Claude 的 5 分钟缓存写档是否单独建模尚未确认**：已确认 1 小时档取
   `cache_creation.ephemeral_1h_input_tokens`（`sessions/utils.rs:677`、`:738-744`），
   但 `cache_creation_input_tokens` 与 1h 子集的差额是否被显式当作 5 分钟档，未逐行确认。
4. **Codex 不存在独立的「压缩会话」解析路径**：`.zst` 是整文件压缩、compaction 只改写
   `total_token_usage` 快照；此前文档口径里的「compressed transcripts」指的是这一现象而不是
   第二种格式。
5. **Antigravity 没有 ledger reconciliation**：该机制属于 `sessions/amp.rs:125-266`；把
   「ledger」按字面安到 Antigravity 上会得出错误结论。
6. **`graph` 子命令的完整输出形状未逐字段核对**（`main.rs:4851-4992`、`:5561-5653` 只读到
   结构轮廓）；TM 侧对它只做 `contributions` + `summary` 的容错消费
   （`src/shared/collector.js:812-819`）。
7. **shim 链路证据取自 TK 仓库的同名包源文件**：TM 工作副本没有 `node_modules`，
   无法读取实际安装后的 `tokscale/bin.js`；`packages/tokscale/bin.js:1-2` 与
   `packages/cli/src/index.ts:226-238` 是同一发布物的源文件，属**一致解释**。
8. **所有结论均为静态分析**：未运行 TM/TK 任何二进制，未实测任何 JSON 输出、缓存文件与
   归档数据库；涉及「运行时实际写出什么」的字段（如 `codex-local-usage.sqlite` 的表结构）以
   代码中的建表语句为准。
9. **SQLite 只读打开口径**：TK 统一 `READ_ONLY | NO_MUTEX`（`sessions/utils.rs:301-306`），
   不写 `journal_mode`；WAL 侧车文件仍会被只读回放（`sessions/cherrystudio.rs:647-651` 注释），
   少数解析器额外设 `busy_timeout`/`query_only`。对「读会不会扰动来源库」这一点，代码层面是
   只读的，但对 WAL 副作用没有实测。
10. **`crates/tokscale-core/src/token_monitor/js.rs` 不是 JS 引擎**：它是 JS 弱类型语义
    （`||`、`Number()`、`Date.parse`、`path.normalize`、sha256 前 12 位十六进制命名空间）的
    Rust 复刻层，用于复现 TM 侧既有解析行为。
11. **`SCAN_WORKER_CEILING = 4` 只做了检索命中**（`scanner.rs:1989`），未展开其所在函数的
    完整上下文；扫描线程池的真实上限可能受运行环境影响。
12. **03 报告中的少量路径映射属推断**（例如某些 Windows 兜底根、`trae` 是否真正启用用量），
    已在对应报告的存疑节列出；本文只采纳其中可由代码常量直接支撑的部分。

## 10. 与 TokenScope 的对照要点

同样做「本地 AI 用量统计」，两者的架构路线几乎相反，列出可直接对照的维度（本仓口径见
`docs/stats-semantics.md` 与 `CLAUDE.md`）：

| 维度 | Token Monitor | TokenScope |
| --- | --- | --- |
| 解析引擎 | 外部 Rust CLI（tokscale fork）+ Electron 外壳 | 仓内 Rust 适配器（`source → model → aggregate → render`） |
| 覆盖范围 | 37 个客户端（TK 侧支持 56 个 id） | Claude Code、Codex（其余待排期） |
| 计价 | 单一匹配结果 + 编译期硬编码兜底表；无峰谷、无分段候选 | 三来源独立候选，完整候选优先、同价决胜取**最高费用**；支持分段与峰谷规则 |
| 未知模型 | 成本 0 + `cost_source=Unknown`，展示层不得当免费 | 未收录模型始终「未知」，不得按 0 静默吞掉 |
| 增量 | TK 内建消息缓存 + TM 的锚点增量 `max(0, base+fresh−anchor)` | `history.db` 事实表 + 查询快照（`query_begin`/`query_id`） |
| 归档 | 会话 SQLite + 客户端/日历史 JSON（三套） | 单一 `~/.tokenscope/history.db` |
| 时间口径 | 设备本地时区（`periodWindows` + `localDayKey`），跨设备各自算 | `settings.toml` 的 `timezone` + 四组互斥范围参数，查询一次冻结 |
| 自定义价格 | 写入 tokscale 的 `custom-pricing.json`（TM 只是编辑器） | 外置 `pricing.toml` + 快照文件，本仓自行匹配与计价 |
| 只读原则 | 明确只读各 agent 数据目录（只写自己的配置/归档） | 同样只读；只写 `~/.tokenscope/` |

可借鉴的两点：①「二进制 revision 变更让成本锚失效」的做法（避免混合价展示），TokenScope 的
价格索引版本轴与之同源思路；②「聚合级输出用布尔 provenance 标记」避免把不完整数据当作精确值。
需要对冲的一点：Token Monitor 的计价**没有跨来源候选与峰谷语义**，若直接照搬其口径，会与
`docs/stats-semantics.md` §4 的权威口径冲突。

<!-- END -->
