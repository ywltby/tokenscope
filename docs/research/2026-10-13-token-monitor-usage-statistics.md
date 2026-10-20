# Token Monitor 用量统计实现拆解（源码级）

> **研究对象**
> - **TM** = [`Javis603/token-monitor`](https://github.com/Javis603/token-monitor)，HEAD
>   `ac4eeb2c4aea239dd94b67f074c73f69aafc46f8`（2026-10-13，`chore: release v0.70.0`，
>   `package.json` version `0.70.0`）。Electron 43 桌面小部件，纯 JavaScript。
> - **TK** = 它 vendored 的 Rust 扫描器 tokscale 的下游 fork
>   [`Javis603/tokscale`](https://github.com/Javis603/tokscale)，pin
>   `fa5f0c13324838d1a988fda317597f9b90da7ad0`（2026-10-10，基线 v4.18.0，release tag
>   `token-monitor-fa5f0c13`）；上游为 [`junhoyeo/tokscale`](https://github.com/junhoyeo/tokscale)。
> - 两仓均浅克隆到临时目录后逐文件精读；此后又在**隔离环境**里跑过 pin 的
>   `tokscale-win32-x64.exe` 做端到端实证（范围与隔离方式见下），**未改动本机任何数据目录**。
>
> **证据分档**：**实证**（在隔离环境里运行 pin 的 tokscale 二进制得到的结果，定义见下）／
> **已验证**（源码直读，给出 `路径:行号`）／**一致解释**（多条代码证据指向同一
> 结论，无单点直证）／**未见实现**（明确检索后判定「本 pin 不存在该机制」的否定结论）／
> **仍存的边界**（无法用现有手段定论的部分，集中列在 §9 的第三层，见 §9.3）。TM 内路径相对
> token-monitor 根，TK 内路径相对 tokscale 根。
>
> **「实证」的定义与范围**：在隔离环境（临时 `TOKSCALE_CONFIG_DIR` + 临时 home + 合成 fixture）
> 里运行 **pin 的 tokscale 二进制**得到的结果。被验证的二进制是 fork release
> `token-monitor-fa5f0c13` 的 `tokscale-win32-x64.exe`（26,491,904 字节，sha256
> `5887f4fae41d5ae4ca684c4a50af96dff9b633fbf9942cd827eca9736765d97a`，与 manifest 声明、release 的
> `SHA256SUMS` 三方一致，tag 解引用到 manifest 记录的同一 commit，`--version` 回
> `tokscale 4.18.0` 且不暴露 fork/commit 标识）。隔离方式：每次调用注入临时
> `TOKSCALE_CONFIG_DIR` 与 `TOKSCALE_PRICING_CACHE_ONLY=1`（其边界见 §7.2：只约束本地解析，
> `pricing` 子命令不受约束），把 `HOME`/`USERPROFILE` 指向临时 home，删除 `DSH_HOME`/
> `CLAUDE_CONFIG_DIR`/`CODEX_HOME`/`GEMINI_CLI_HOME`/`XDG_DATA_HOME`/`TOKSCALE_EXTRA_DIRS` 等
> 21 个可能把扫描指回真实 profile 的变量，扫描类命令一律显式带 `--home <临时 home>`（`pricing`
> 子命令不接受 `--home`）。覆盖范围：Claude / Codex / Zed 三个客户端的端到端扫描、`.jsonl.zst`、
> SQLite 只读与 WAL 侧车、错误路径与 `--help` 取值表、`pricing` 子命令的联网行为。
>
> **一处隔离偏差（如实披露）**：交叉验证扫描路径时执行过 `clients`（不带 `--home`），进程内残留的
> `DSH_HOME` 因此生效，该命令列出了**真实** `C:\Users\admin\.dsh\sessions` 的路径并显示
> `messages: 13.3K` 计数；读数仅到路径存在性与消息计数，未读取任何会话内容。发现后已清除全部
> `*_HOME` 类变量，其后所有命令（含后续扫描与压缩会话实验）均未再触达真实目录；沿用该隔离环境的
> 人都必须保持这批变量被清除，否则 `clients` 与不带 `--home` 的扫描仍会读到真实 profile。
>
> **本文只记录结构、字段名、路径规则与计算口径，不含任何真实会话内容。**

## 1. 结论速览

1. **Token Monitor 自己不解析绝大多数 agent 的日志**。36 个受管客户端（`src/shared/clientCatalog.js:51-90`
   的 `CLIENT_CATALOG`，逐条计数 36；id 顺序冻结于 `tests/shared/clientCatalog.test.js:54`）
   的 token 用量统一由 vendored 的 Rust CLI tokscale 扫描；TM 是「数据源发现 + 监视 + 增量 +
   归档 + 跨设备同步 + UI + 限额」的外壳。
2. 采集方式是**子进程 + JSON**：TM spawn tokscale 二进制，读它的 stdout JSON。主扫描 argv 固定为
   `--json --client <csv> --group-by <grouping> [--today|--month|--since D]`
   （`src/shared/collector.js:572`）；另有 `graph --client <csv> --no-spinner`（`:615`，TM 用于热力图）
   与 `pricing <id> --json --no-spinner`（`:636`，单模型查价）。
3. TK 的 `--json` 报告**不是** `commands/report.rs` 产出的（那是 `tokscale report` 的 wiki/任务归因
   路径），而是 `crates/tokscale-cli/src/main.rs:2324-2477` 内联的 serde 结构体；顶层含
   `groupBy/entries/sessions/workspaces/6 个 total*/processingTimeMs`（`main.rs:2383-2389`），entry 携带
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
8. **每个 agent 的读取方言都在 TK 的 `crates/tokscale-core/src/sessions/` 里**（该目录 62 个 `.rs` 文件，
   含 `mod.rs`/`utils.rs`/`synthetic.rs` 等公用模块），每个客户端一份解析器：
   JSONL 流式解析（Claude/Codex/Gemini/Kimi/Qwen/Grok/DSH/Muse/…，另有 Cherry Studio 的
   Claude Code 转录，用专用解析器按稳定 request/message id 去重）、
   SQLite 只读查询（OpenCode/Zed/Kilo/Hermes/WorkBuddy/Devin/…，另有 Cherry Studio 的自有
   账本 `CherryStudio/Data/cherrystudio.sqlite`）、
   API 或本地缓存导出（Cursor 的账户级 usage 缓存、Antigravity 的 protobuf 生成库、
   Copilot 的 OTEL / data.db / session-store.db / VS Code chatSessions 四路）。
9. TK 注册了 **56 个 client id**（`crates/tokscale-core/src/clients.rs:320-1106`，`ClientId::COUNT == 56`
   见 `:1220`），其中 TM 的 catalog 只启用 36 个；fork 另加 3 个上游不认的客户端
   （`proma`、`catpaw`、`qodercn`，见 `crates/tokscale-core/src/token_monitor/`），**它们不在
   `ClientId` 注册表内**（`token_monitor/mod.rs:1-18`、`:44-57`，因此既不进连续索引也不出现在
   CLI 取值表里），并给 `mcode` 补了上游缺失的本地运行时库解析（supplement，同文件 `:110-113`）。
   CLI 的 `--client` 取值数因此不是 56 而是 58（与注册表、与这三者的关系见 §4.1）。
10. **金额在 TK 里逐条消息算好**（`apply_pricing_if_available`，`crates/tokscale-core/src/lib.rs:5823-5845`），
    聚合阶段只做求和（`--json` 路径 `crates/tokscale-core/src/lib.rs:4644-4650`，
    `graph` 路径 `crates/tokscale-core/src/aggregator.rs:289`）。
11. 价格来源是**三份远程数据集 + 一份用户自定义 + 四张编译进二进制的硬编码表**：
    LiteLLM、OpenRouter、models.dev（`crates/tokscale-core/src/pricing/mod.rs:416-427`，缓存文件
    `<config_dir>/cache/pricing-{litellm,openrouter,models-dev}.json`，TTL 3600s，
    `pricing/cache.rs:6`），加上 Cursor / Sakana / archive / self-hosted 的兜底表
    （`pricing/mod.rs:123,227,285`、`pricing/self_hosted.rs:7`）。本 pin 的**生产路径没有**编译期打包的
    价格数据集（`include_str!`/`include_bytes!` 全仓仅 4 处，全在测试与 fixture 里，且无 `build.rs` 注入）。
12. 匹配优先级：**自定义价（命中即返回）→ self-hosted 零价 → 路由标签拒赔 → 别名 → 精确/归一化/
    前缀（三源仲裁）→ Cursor/Sakana 兜底 → archive → 模糊 → 未命中（None）**
    （`pricing/mod.rs:570-586`、`pricing/lookup.rs:619-815`、`:821-1156`、`:3637-3691`）。
13. **自定义价格是 TM 唯一写进 tokscale 的输入**：写到 `<tokscale config dir>/custom-pricing.json`
    （`src/shared/tokscaleConfig.js:32-56`），五档键名与 TK 的反序列化结构一一对应
    （`src/shared/tokscaleCustomPricing.js:40-52`）。TM 运行时不传价格相关 CLI 参数、不设价格环境变量；
    写价一律经这个文件（唯一例外在构建期：`npm run ensure:tokscale` 的 vendored 二进制校验脚本为离线冒烟
    会设 `TOKSCALE_PRICING_CACHE_ONLY=1`，`scripts/verify-vendored-tokscale.js:459`、`package.json:14`，
    与运行时无关）。**实证（边界）**：`TOKSCALE_PRICING_CACHE_ONLY=1` 并不能保证「不联网」：它只让
    本地解析（扫描）走 `load_cached_any_age()`，`pricing` 子命令不受它约束，空 config 下仍会联网抓价
    并落盘（详见 §7.2）；把该开关当作离线隔离手段不成立。
14. **价格/二进制一变，成本锚即失效**：`pricingFingerprint()` 把价格文件字节 + 二进制
    identity + 二进制 `stat(size/mtime/ctime)` 拼进 `configFingerprint`，每个 tick 开头比对，
    变化即清锚点、重置全扫与 history 时间戳（`src/shared/collector.js:2151-2186`、`:2774-2788`）。
    扫描途中价格变化（混合 revision）时本轮**不发布**，最多自动重放一次（`:2949-2953`、`:3183-3199`）。
15. **TM 自己算钱的唯一路径是 Codex DOTS 本地账本**：订阅本机 Codex 执行器的 tokenUsage
    （WebSocket），落自有 SQLite，再用 `lookupModelPricing` + 本地价格表算成本
    （`src/shared/providers/codex/localUsageSource.js`、`localUsageStore.js`、`localUsage.js`）。
16. **TM 自己读的其余数据都不是用量**：会话标题、项目归属、上下文占用、prompt cache 时窗、
    live 活动状态、限额。它们通过与 tokscale 同名的路径表（`src/shared/clientSources.js`）做
    watch/健康/诊断，由 provider 级解析器补齐元数据。唯一例外是**会话详情层**：按需打开单个
    会话时 TM 自己解析转录并算 token（`src/shared/sessionDetail.js:60-67` 的 `makeTokens`，
    客户端侧如 CodeBuddy（`src/shared/providers/codebuddy/transcript.js:142-170` 的
    `buddyUsageTokens`）、DSH（`src/shared/providers/dsh/sessionDetail.js:70-90`）、
    Reasonix（`src/shared/providers/reasonix/sessionDetail.js:262-294`）），结果只经按需详情
    IPC 回给 renderer（`session:getDetail`，`src/electron/main.js:8023-8028` 经
    `src/shared/sessionDetailResolver.js` 与 `sessionDetailWorker.js`），不进周期统计。
17. **未知模型不得按 0 静默丢弃**：TK 对未命中价格的模型保留 `cost_source`（`Unknown`）且成本 0，
    由上层展示「未知」；只有提交（leaderboard）路径才强制要求价格覆盖
    （`crates/tokscale-core/src/sessions/mod.rs:71-78`、`pricing/mod.rs:659-669`）。**实证**：无价源时
    扫描照常 exit 0、`cost`/`totalCost` 记 `0.0` 且不告警，`--json` 的输出里**没有 `cost_source` 键**，
    上层只能由 `cost == 0` 推断，因此 `0` 只能读作「未知」、不能读作「免费」（见 §7.6）。
18. 缓存写价分档：`cache_write_1h` 与 5 分钟档分开计价，1h 档使用
    `cache_creation_input_token_cost_above_1hr`，阈值 200k
    （`crates/tokscale-core/src/pricing/lookup.rs:2337-2459`）。
19. **本 pin 未见峰谷/时间窗定价**，也不存在外置 `[[model.segment]]` 配置或「多候选取最高价」
    逻辑（条目内仍有上下文分档，见 §7.4）；对照 TokenScope 的三来源候选计价，这是两者最显著的
    口径差异——见 §7 与 §9。
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
                                   └─ spawn tokscale graph --client ...（仅 History 到期/强制时才跑）
        ↓ stdout JSON
  usage.js collectUsageRows()（深挖）→ extractUsageBundleFromTokscale()
        ↓
  period{today,month,allTime} + byClient 分区
        ↓ provider 元数据补齐（标题/项目/上下文/prompt cache/父会话）
  applySessionMetadata() / sessionLive
        ↓
  onUpdate(summary) → usageTransform（worker 线程）→ 设备记录 / 归档 / Hub 上传 / UI
```

图中 `graph` 与三个窗口扫描**不同级**：它不是每个 tick 都跑，由 `includeHistory` 决定——
History 间隔到期（默认 `historyIntervalMs = 15 * 60 * 1000`，`src/shared/collector.js:2462`）、
显式强制刷新、本地跨日、或价格/来源指纹变化时才会带上
（判定 `shouldIncludeHistory()` `:865-869`，每 tick 计算 `:2794-2800`；消费点 `:1333-1335`，
即 `collectHistoryOnce()` 只在 `options.includeHistory` 为真时被调用）。

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
- **空白环境变量整键删除**：**恰好**这三个键 `XDG_DATA_HOME`/`XDG_CONFIG_HOME`/`TOKSCALE_HEADLESS_DIR`
  若为空白，会按平台大小写规则删除后再传给子进程（列表 `TOKSCALE_BLANK_SENSITIVE_ENV_KEYS`，
  `src/shared/collector.js:179-183`；删除实现 `:191-207`；`TOKSCALE_CONFIG_DIR` **被刻意排除**，
  因为两侧对它的语义已经一致——空白即未设、非空（含空白字符）即覆盖，理由见 `:166-170`），
  避免下游把「设了但为空」当成显式重定向。

**未知 client 与 `--group-by` 的降级链路**（命令面之外的另一半，此前未记）：
① 判据是**两个条件**——exit code 2 **且** stderr 命中 `/--client/i`（`isUnknownTokscaleClientError()`，
`src/shared/collector.js:474-478`；常量 `TOKSCALE_UNKNOWN_CLIENT_EXIT_CODE = 2`，`:318`）。
只要 exit code 2 不够：那是 clap 的通用参数错误码，别的 flag 写坏也是它（注释 `:446-450`）。
**实证两个条件都必要**：未知 `--client definitely-not-a-client` 得 exit 2，stderr 首行是
`error: invalid value 'definitely-not-a-client' for '--client <CLIENTS>'` 后跟
`[possible values: …]`（58 项），stdout 全空；换成完全不存在的选项（`--not-a-flag-at-all`）同样
exit 2，但 stderr 是 `error: unexpected argument '--not-a-flag-at-all' found` 加 `Usage: …` 与
`For more information, try '--help'.` 三行、不含 `--client`；两段合取因此不可省，只看退出码会把无关的
参数错误误判成「未知 client」。
② 命中后做能力探测：先跑 `tokscale --help`，**锚定 `--client` 自己的 stanza** 解析
`[possible values: …]`（`src/shared/tokscaleCapabilities.js:10-34`，扫描截到下一个选项行为止，
免得把别的 flag 的取值表读成 `--client` 的；探测超时 10s，`collector.js:314`、`:320-321`），
再用空 home 加 `--home` 的最小扫描确认 fork 专属 id 是否被接受（`:387-409`、`:411-426`，
因为 fork 自有 id 走的是另一条通路：它们在 clap 解析前被 CLI 从 `--client` 值里摘出、从不进取值表，
见 §4.1 末段）。
③ 结果按**二进制 identity** 缓存，**成功与失败都缓存**：resolver 把探测结果写进 `settled`（成功）、
失败写 `null` 并只 warn 一次（`tokscaleCapabilities.js:51-93`、`:70-78`），进程内按 identity 复用；
本 tick 用过滤后的 client 集重试一次，之后的 tick 直接按缓存**主动**过滤、不再先撞一次错
（`collector.js:514-532`、`:534-537`、注释 `:504-513`）。
`--group-by` 有同构回退，但**判据与退出码都不同族**：fork 的 `client,workspace,session,model`
被旧二进制拒绝时，TM 只匹配 stderr 文本、不读退出码（`isUnknownTokscaleGroupByError()` 用
`/invalid group-by value/i`，`src/shared/collector.js:470-472`，注释 `:467-469`），命中才记该
identity 不支持并立刻改用 `client,session,model` 重跑一次（`:584-595`、`:586`），拒绝结论同样按
identity 缓存（`:458-461`、`:461-465`），换二进制后 identity 变化即换缓存槽；
`resetTokscaleCapabilityCache()`（`:439-444`）只被测试调用
（`tests/shared/collectorForceLimits.test.js:417`），生产路径不主动清缓存。
**实证两者不是同一个错误族**：坏 `--group-by definitely-not-a-grouping` 得 **exit 1**（不是 2），
stderr 为 `Error: Invalid group-by value: 'definitely-not-a-grouping'. Valid options: model,
client,model, client,provider,model, workspace,model, session,model, client,session,model,
client,workspace,session,model`（大写 `Error:` 前缀，区别于 clap 的 `error:`；该值是应用层
`GroupBy::from_str` 的拒绝，CLI 侧落点 `crates/tokscale-cli/src/main.rs:683-686`，错误文本常量
`crates/tokscale-core/src/lib.rs:257`）。TM 只认文本、不认退出码，因此这一层判据本身是稳的，
但任何「坏参数一律 exit 2」的假设会被这条推翻。

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
- 两处函数级后处理（都在 `deltaValue` 之外，`:1901-1913`）：base/fresh/anchor 任一方缺
  `modelThroughput` 即从结果中删除该字段（`:1903-1905`）；结果里的 `sessions` 再过
  `filterReasonixSyntheticSessions`（定义 `src/shared/providers/reasonix/sessionGuard.js:32`），
  旧版 Reasonix stats-path 合成行不得借暖 tick 复活（`:1906-1911`）。

跨天（stale 锚点）时本轮不再走 `todayOnly` 路径，退回三窗口全扫（`src/shared/collector.js:3534`
附近的循环分支），这就是「本地日期键」必须与 history 的日键同源的原因
（`const localTodayKey = localDayKey`，`:643`）。

**每小时强制全量对账**：增量之外还有一道时间维度的兜底——`FULL_SCAN_INTERVAL_MS = 60 * 60 * 1000`
（`src/shared/collector.js:2235`，注释 `:2232-2234`）。间隔循环每轮先算
`fullScanDue = lastFullScanAt === 0 || Date.now() - lastFullScanAt >= FULL_SCAN_INTERVAL_MS`（`:3516`），
到期就不再走 `todayOnly`（`anchorToday` 判假，`:3534`），改为把 month/allTime 重新全扫一遍；
成功捕获后刷新 `lastFullScanAt`（`:2980`），冷启动或时间戳不可信时它保持 0，直接强制全扫（`:3530-3533`）。
只实现「跨日全扫」会漏掉「未跨日但来源被改写」的漂移。

### 3.4 定向分区扫描与安全阀

watch 事件只会命名「变化的路径」，TM 把它们映射回 client 集合，在一次 union 的 `--today` 扫描里
覆盖（`src/shared/collector.js:1008-1012`）。结果按 client 切成 `todayPartitions`，只替换变化分区，
再 `mergeTodayPartitions()` 合成公开的 `today`（`:1047-1050`、`:2121-2130`）。

安全阀（`:1018-1040`）：整套判定只在**本轮确实是定向扫描**（`targetRequested`）时生效（`:1032`）。
触发条件是三者之一：分区里出现**有用量的未归属行**（`periodHasUsage()`，调用点 `:1023`、定义 `:2132-2137`）；
出现非目标 client 的已归属行；或**多个目标**中某个分区缺失（`targetClients.length > 1`，`:1018-1021`）。
触发后不做逐 client 修补，而是立刻用全量 client 重扫一次 `--today`，整轮替换（`:1037-1040`）。
单个目标的分区缺失**不**触发退化：`replaceTodayPartitions()` 会把它置为 `emptyPeriod()`
（`:2121-2126`，置位在 `:2123`）。

### 3.5 监视策略与降级

- chokidar 只在**隔离宿主**里跑，不在采集/UI 线程上（进程内回退时除外）：生产默认 fork
  子进程（`src/shared/watcherHost.js:56`），
  理由是 macOS 上每文件一个描述符，填满 `OPEN_MAX`（10240）后新建管道的编号在它之上、`posix_spawn` 直接拒绝，
  后续 tokscale spawn 全部 `EBADF`（issue #520，`:10-16`）。但宿主是可替换的接缝，不是唯一路径：
  同一模块也能由 **worker 线程**承载（`src/shared/watcherWorker.js:17-22` 用 `parentPort` 兼容两种边界），
  子进程装载失败或意外退出时回退到**进程内** watcher（`watcherHost.js:30-34`、`:226-238`、`:272-290`），
  `TOKEN_MONITOR_WATCH_IN_PROCESS` 可强制进程内（`:108-112`、`:362-366`）；主线程默认不加载 chokidar
  （`createInProcessWatcherHost()` 内惰性 `require`，`:115-117`）。
- debounce 会被 `watchMaxWaitMs` 封顶，避免「写个不停时永远不触发」：默认
  `WATCH_MAX_WAIT_MS = 5000`（`src/shared/collector.js:2246`），取值被
  `Math.max(watchDebounceMs, clampTimerDelayMs(...))` 抬到不低于 debounce（`:2472-2475`），
  封顶在事件调度里生效（`:3282-3287`）。
- 描述符耗尽（`ENOSPC`/`EMFILE`/`ENFILE`，集合 `:2298`）→ 整个进程切换为轮询（sticky，
  `src/shared/collector.js:3348-3357`，置位 `:3350`，诊断码 `watcher-polling-fallback` `:3352-3356`）；
  轮询需先统计覆盖条目数，超过 `WATCH_POLLING_ENTRY_LIMIT`（20000，`:2307`）时抛
  `watch-polling-limit`、**直接不装 watcher**（`:2403-2406`；紧邻的 `:2398-2402` 是另一个拒绝码
  `watch-polling-unavailable`，两者同属 `WATCH_REFUSAL_CODES`，`:2373-2375`）；抛出后经宿主转到
  collector 的错误处理，在 `:3344-3347` 命中该集合即转 `enterIntervalFallback()`，报诊断码
  `watcher-interval-fallback` 并退化为纯固定间隔采集（`:3324-3337`，发出点 `:3329`）。
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
  （`tokscale cursor …` / `tokscale antigravity sync`，`src/shared/collector.js:1424` 的
  `SELF_SYNCED_CLIENTS`，名单字面 `SELF_SYNC_KINDS = ['cursor','antigravity']` 在
  `src/shared/selfSyncThrottle.js:21`；`src/shared/providers/antigravity/selfSync.js:195`、
  `src/shared/providers/cursor/auth.js:201-202`），
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
`Config`（解析顺序 `TOKSCALE_CONFIG_DIR` → Linux `$XDG_CONFIG_HOME/tokscale` → Windows
`%APPDATA%\tokscale` → 兜底 `<home>/.config/tokscale`（Linux 未设 `XDG_CONFIG_HOME`、macOS
等前置分支未命中的情形都落到这里）；`--home` 关闭 env 根时 Windows 落到
`<home>/AppData/Roaming/tokscale`，`:139-169`）、`AppData`（平台 per-user app data）、
`ReasonixHome`（专用多环境规则）、`EnvVar { var, fallback_relative }`（例如 `CODEX_HOME` → `.codex`）。
路径拼接刻意逐段 push，避免 Windows 混用分隔符（`:20-29`、`#1048`）。

**上游客户端主路径没有查表式的 dispatch**：真正决定「哪个上游 client 走哪个解析器」的是
`crates/tokscale-core/src/lib.rs:5948` 的 `parse_local_clients` 内逐 lane 手写调用
（opencode 从 `:5984` 起，claude / codex 等各一段），注册表只提供 id 与路径元数据
（`clients.rs:374-401`）。表驱动的 dispatch 在本 pin 只出现在两处局部例外：fork 自有客户端
`crates/tokscale-core/src/token_monitor/mod.rs:36-57`（`Client { id, parse }` 表，另见 `SUPPLEMENTS`
`:110-113`），以及 Antigravity 家族三源表 `crates/tokscale-core/src/lib.rs:7332-7345`
（`[(ClientId, AntigravitySourceParser); 3]`，desktop 缓存 / CLI 库 / IDE 扩展库，消费点 `:7348`）。

**补充（路径解析口径）**：`--home <dir>` 会**关闭全部环境变量根**——CLI 侧
`use_env_roots(home_dir) = home_dir.is_none()`（`crates/tokscale-cli/src/main.rs:1838-1840`，
接线点 `:2182`、`:4365`），使 `CLAUDE_CONFIG_DIR` / `CODEX_HOME` / `GEMINI_CLI_HOME` /
`XDG_DATA_HOME` / `XDG_CONFIG_HOME` / `TOKSCALE_EXTRA_DIRS` 等一律不再读取
（`scanner.rs:2182-2203`，口径注释见 `:2851`）；`--home` 只被本地报告命令接受，其他子命令显式拒绝
（`main.rs:1827-1836`）。这解释了 WSL 扫描（§3.1）为何不继承宿主的环境变量根。

**补充（CLI 的 `--client` 取值表比注册表多两项；源码 + 实证）**：`--help` 里 `--client <CLIENTS>`
的 `[possible values: …]` 有 **58 项**，不是 56：多出的两项是 CLI 层独有的 `9router` 与
`synthetic`（`crates/tokscale-cli/src/main.rs:1091-1159` 的 `ClientFilter` 枚举，其余 56 项与
`tokscale_core::ClientId` 同序、`9router` 插在 `gjc` 之前、`synthetic` 追加在末尾，顺序注释见
`:1084-1088`）。实证：`--help` 的取值表与未知 client 的 stderr
`[possible values: …]` 都是 58 项、逐字同序。两者都不是独立的本地扫描源：

- `9router`（`ClientFilter::NineRouter`，`main.rs:1119-1120`）落到 core 的 `ClientId::Gjc`
  （`to_client_id()` `:1263`），即「9Router bridge 脚本产出、gjc 格式」的那批数据
  （`scanner.rs:1947-1950`）；消息级过滤仍分得开：请求 `gjc` 会连带保留 `client == "9router"`
  的消息（gjc 是超集请求），反向不成立（`crates/tokscale-core/src/lib.rs:189-204`），而
  `sessions/gjc.rs:51` 允许行内 `source` 字段把 client 改写成 `9router`（`gjc.rs:524-533` 的测试
  即此用法）。
- `synthetic`（`ClientFilter::Synthetic`，`main.rs:1158`）是**没有扫描路径的 meta-client**：
  `to_client_id()` 返回 `None`（`:1293`），`default_set()` 把 `Synthetic` 与 `NineRouter` 两者
  一并排除（`:1388-1394`；函数上方注释 `:1377-1381` 说明 synthetic 检测一直是 opt-in，因为它会把
  别的 client 的消息重新归因）。它做的是「在既有 client 的消息里识别 synthetic.new 网关」的再归因
  （`crates/tokscale-core/src/sessions/synthetic.rs:1-4`
  的检测函数，core 侧过滤条件在 `lib.rs:202-203`）；`default_submit_clients()` 会把它加进提交集合
  （`main.rs:1818-1825`）。

fork 自有的 `proma`/`catpaw`/`qodercn` **一个都不在这 58 项里**（实证核对 `--help` 列表），机制上
也不会进：它们是 `token_monitor::CLIENTS` 的条目（`token_monitor/mod.rs:44-57`），主路径由 CLI 在
clap 解析前从 `--client` 值里摘出（`crates/tokscale-cli/src/token_monitor.rs:55-115` 的
`split_cli_clients`，入口 `parse_cli` `:21-37`），且只在 `models`/`monthly`/`hourly`/`graph`/`tui`/
`wrapped`/`time-metrics` 与无子命令形态上被接受（`:39-53`），其余子命令（如 `submit`）回退到原始
argv、按 clap 自己的错误失败（`:8-11`）。这正是 §3.1 的能力探测必须用「空 home 最小扫描」而不是
解析 `--help` 来发现它们的原因。

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
  取大合并（合并规则 `:988-1045`，`merge_claude_duplicate` 只处理 token 桶与 duration，不涉及
  `session_id`）。
- **会话身份取文件名 stem，不取文件内的 `sessionId` 字段**（源码 + 实证）：`session_id` 由
  `path.file_stem()` 得出（`claudecode.rs:444-448`）；JSONL 行内的 `sessionId` 只在**子代理
  （sidechain）文件**里被采信：首个可解析行的 `is_sidechain` 为真时改用该行的 `session_id`
  （parent id）修正会话计数（`:526-541`，赋值点 `:533`）。**实证**：文件 `session-1111.jsonl` 内部
  `sessionId` 写着另一个 UUID，`--group-by client,session,model` 输出的仍是 `session-1111`；另一
  文件内部写 `internal-only-zzz`，输出的是它的文件名 `2222…6666`。影响面：会话桶边界锚在**文件名**
  上，重命名文件即换一套身份、同一份内容会被切成两个会话；因此「`(session_id, message.id)` 式」
  的原生去重键（本仓 TokenScope 口径的原生身份：Claude 取 `(session_id, message.id)`）若
  `session_id` 取自 TK 输出，其稳定性只到「文件不改名」为止，要稳就得自己从转录内容里取
  `sessionId`。相应的 `sessions[]` 输出键名也已实证为 camelCase
  `firstActiveMs`/`lastActiveMs`（`title`/`parentSessionId` 需带标题与父子关系的 fixture 才会出现，
  本次未触发）。
- 不按字符估算 token：tool_result 之类的文本即使缺 usage 也不进估算
  （`claudecode.rs:450-458`；对照 TK issue #1011）。
- Advisor / 迭代用量从 `usage.iterations` 拆出（`:829-871`）。

#### Codex CLI（`sessions/codex.rs`）

- 数据面：`<CODEX_HOME|~/.codex>/{sessions,archived_sessions}/**/rollout-*.jsonl*`
  （注册表条目 `clients.rs:427-439` 只声明 `relative: "sessions"` 与 `pattern: "*.jsonl*"`；
  `archived_sessions` 由 `scanner.rs:2334-2341` 追加为同一 client 的第二个扫描根，
  `codex.rs:3-10` 同述。`rollout-` 是 Codex 的命名约定、不是筛选条件，`*.jsonl*` 兼容整文件
  压缩 `.zst`，压缩文件有 256 MB 上限，`sessions/utils.rs:544-551`）。
- 桶字段：`input_tokens / output_tokens / cached_input_tokens / reasoning_output_tokens`
  （`codex.rs:156-160`，结构体 `CodexTokenUsage` 为 `:153-162`）；同一结构还带 `service_tier`
  （`:155`）、`cache_read_input_tokens`（`:159`）与 `total_tokens`（`:161`）。后两者不直接成桶：
  `cache_read_input_tokens` 只在 `CodexTotals::from_usage` 里与 `cached_input_tokens` 取较大值后
  进入 `cache_read` 桶（`:183-194`），`total_tokens` 只参与 fork 子进程继承基线的上报总量判定
  （`reported_total_tokens` `:1455-1457`，消费点 `:1467`、`:1478`）。`reasoning` 在 Codex 里是
  output 的**子集**，解析时必须拆出（`:253-279`）。
- 增量读法主路径只采信 `last_token_usage`（累计快照 `total_token_usage` 用于去重与单调性校验，
  `:914-917`、`:954-970`）；若某行只有累计值而无 `last`，存在 **total-delta 降级分支**（`:973-985`）。
- 去重键形如 `codex:token_count-total:…`（`:1371-1380`），fork 额外做父作用域处理（`:1072-1090`）。
- **不存在独立的「压缩会话」解析路径**（源码）：`.zst` 与 `.jsonl` 共用同一个
  `parse_codex_reader`，差别只在解压入口（`codex.rs:1498-1559`；`is_zst_path` 是纯后缀判断，
  `sessions/utils.rs:547-551`）。解压有三道边界与一条拒绝：① zstd 解压窗口 ≤ 2^26
  （`window_log_max(26)`，`:1539`）；② 解码后字节数 ≤ `MAX_ARCHIVE_BYTES` = 256 MiB
  （`utils.rs:544-545`，用 `take(MAX_ARCHIVE_BYTES + 1)` 探测越界，`:1551`）；③ 超限或解析未完成即
  **清空全部结果**并置 `parse_succeeded = false`（`:1553-1557`，不截断、不部分保留）；④ 打开失败、
  decoder 初始化失败、`window_log_max` 失败三类都退化为「warn + 空结果」、不 panic 不重试
  （`:1510-1549`）。**实证**：277 字节的 `.jsonl.zst` fixture 被正常发现并解析（exit 0、
  `messageCount=1`；`input=800` 是 Codex 口径的 `input_tokens − cached_input_tokens`）。
- **`.zst` 不支持增量续读**（源码）：非零 `start_offset` 直接返回空结果 +
  `parse_succeeded = false`（`codex.rs:1499-1508`）：zst 分支里 `consumed_offset` 是解码流内偏移、
  与压缩文件偏移不是同一坐标，解析器选择拒绝而不是换算；压缩会话因此每次都走**整文件重解析**
  （`parse_codex_file` 固定以 0 偏移做入口，`:1450-1453`）。
- **compaction 靠「增量源选择 + 回退分档」去重，没有独立的事件分支**（源码）：`total_token_usage`
  被明确降级为基线/单调性校验，`last_token_usage` 是唯一直接增量源（注释 `codex.rs:914-917`）；
  判定基础是 `CodexTotals::delta_from`，任一桶小于 previous 即 `None`（`:196-211`）。四档处理：
  ① 快照与基线完全相同 → 跳过、不推进基线（`:958-960`）；② total 回退但幅度小
  （`looks_like_stale_regression`：`current * 100 >= previous * 98` 或
  `current + 2 * last >= previous`，`:236-251`）→ 判为乱序陈旧快照并跳过，避免把 `last_token_usage`
  重复计一次（注释 `:245-248`）；③ total 大幅回退（compaction 重写快照的典型形态）→ 把 `last` 计为
  本次增量、把回退后的 total 设为**新基线**（`:966`，测试
  `test_compaction_total_drop_uses_last_as_increment` `:4039`）；④ 全零快照跳过且**不推进基线**，
  防 post-compaction 的零 total 抬高后续 delta（`:996-1004`）。首个事件即带 total 时也只用 `last`，
  避免把 resume 继承的历史 token 全量计入（`:968-970`）。全仓没有名为 `resets` 的字段或独立事件
  类型：`resets` 的命中只落在注释、测试名、额度窗口字段 `resets_at` 与展示文案上，去重不依赖
  任何「resets 语义」，靠的就是上面这套「增量源 + 回退分档 + 基线不推进」的组合。

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
- **未见 ledger reconciliation**：上传/对账式的 `usageLedger` 机制在 `sessions/amp.rs`
  （`parse_amp_ledger_records` `:125`、`find_matching_ledger_record` `:218`、`merge_amp_records` `:243`，
  由 `parse_amp_file` `:275` 的 `:302-338` 循环完成对账），与 Antigravity 无关。

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
- 使用者集中在少数解析器（下列为生产调用点）：`commandcode.rs:317-318`（字符数来自
  `content_chars`，它把内容按 canonical JSON 序列化后计数，`:574-591`；`:303` 只是算字符）、
  `zcode.rs:159-160`、`freebuff.rs:184-185`、`kiro.rs:381`、`:410-412`、`:835`、`:843`、
  `:899-900`、`:1149-1150`、`:1299-1300`、`:1399-1400`、`:1597`、`:1620`
  （`kiro.rs:606` 是 IDE 路径的说明性注释，不是调用；`claudecode.rs:1365` 是被开关关掉的那个，
  见下条）。它们只在源**完全没有** usage 时触发；已上报的「全零桶」不会被估算覆盖
  （`commandcode.rs:305-327`，全零且无正成本的整行走丢弃路径 `:336-358`）。
  `kiro.rs:1944` 之后与 `commandcode.rs:1355` 起的调用属测试代码，不计入使用者。
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

**补充（解析口径）**：JSONL 解析**以逐行 lossy 解码为主流**（`sessions/utils.rs:15-33` 的
`lossy_lines`，经 `for_each_json_line` `:103-121`、`for_each_json_line_with_bytes` `:153` 铺给
大多数 JSONL 解析器）——按换行切出原始字节后对非法 UTF-8 做替换解码，坏字节只影响它自己那一行
（该行通常在 JSON 解析阶段计为坏行被跳过）。源码注释记录了反例：若改成朴素的 `BufRead::lines()`
配 `map_while(Result::ok)`，首个坏字节会让其后**所有**记录消失（issue #1031 实测：83 MB 的 Grok
`updates.jsonl` 只剩约 2%）。但异常处理并非同一套，哪些客户端会丢数据要按实现分档：

- `claudecode.rs:295-299`（父文件 subagent 类型查询）与 `:511-515`（主解析）走 `BufRead::lines()`
  配 `Err(_) => continue`：坏行整行跳过，其后各行照常读取。
- `codex.rs:546-552` 走 `read_line` 且 `Err(_) => { parse_succeeded = false; break; }`：**该行及其后
  全部记录不再被读取**（夹具 `:2082-2094` 专门模拟行解码失败），`parse_succeeded=false` 还让本轮
  结果不进缓存（`lib.rs:1255-1266`），尾部数据在后续冷读里同样取不回来。
- 另一批解析器整文件 `read_to_string` 加 `str::lines()`（如 `kimi.rs:211-214` 读
  `session_index.jsonl`、`gemini.rs:119-123` 推导 session id）：文件任一字节非法 UTF-8 即整文件失效，
  逐行容错不存在。复刻读取器时至少要保证「坏字节不吞掉后续记录」这一条。

**补充（文件级归属去重）**：适配器各自的去重之上还有一层**扫描期归属裁决**（`scanner.rs:2929-2993`）：
同一 `(ClientId, 规范化绝对路径)` 只保留一份（`:2933-2952`）；Cherry Studio 的 V1/V2 根会产出同名转录，
V2 根（`Data/Agents/.claude/projects`）因收到新写入而在冲突中胜出、V1 同名文件被剔除
（`:1900-1927`、`:2963-2968`）；OpenClaw agent 树内的 Codex rollout 归 OpenClaw——命中
`classify_openclaw_jsonl == CodexRollout` 的文件会从 codex lane 的清单里剔除（`:2970-2993`），
因此同一份 rollout 不会在两个 client 下各计一遍。

## 5. TM 自己读的那部分

### 5.1 数据源发现：一张表管 watch / 健康 / 诊断

`src/shared/clientSources.js`（514 行）不是读取实现，而是**路径表**：`clientSourceRoots()`
为每个已启用客户端列出根，规整为 `{id, dir, sourcePath?, optional?, custom?}`
（`:149-165`）；`sourcePath` 表示「真实来源是单个文件、但要 watch 它的父目录」。用到它的来源如下
（共 11 处代码位置，CatPaw / Qoder CN / Devin 的多候选目录会各自展开一条）：
Copilot 的 `data.db` 与 `session-store.db`（`:259-260`，父目录同是 `~/.copilot`）、Copilot 的自定义
OTEL exporter 文件（`:268`，由 `copilotExporterWatch()` 单文件派生，`:120-127`）、`grok` 的
`logs/unified.jsonl`（`:242`）、`kilo` 的 `kilo.db`（`:293`）、`zcode` 的 `.zcode/cli/db/db.sqlite`（`:313`）、
CatPaw 的 db（`:366`）、Qoder CN 的 db（`:374`）、Cherry Studio 的 `Data/cherrystudio.sqlite`（`:459`）、
`unsloth` 的 `studio.db`（`:467`）、Devin CLI 的 `sessions.db`（`:484`）。

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

观察值 TTL 30 秒、运行窗口 10 分钟（`src/shared/sessionLive.js:41-42`）；续期/对账定时分三档
（`src/shared/sessionActivityScheduler.js:19-23`）：原生事件未就绪 3 秒，需要续期时 10 秒
（30 秒租期的三分之一），否则 60 秒一次对账。
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
  mcode→minimax，`:74-80`）。**9 个 provider 只有限额、没有用量**：TypeSafe、OpenRouter、
  Volcengine、Ollama、Trae CN、Alibaba Cloud、StepFun、「第三方 API」与 GLM Team
  （`zaiteam`，`:50`）。前 8 个就是 README.zh-CN.md 里用量列为 `—` 的那批、代码侧连续列出
  （`limits/providers.js:57-64`）；`zaiteam` 在 README 表里没有行，既不在
  `src/shared/clientCatalog.js:50-95` 的 36 个客户端内，也不在上述 5 条改名映射里，只出现在
  限额通道（`:50`、`src/shared/providers/zaiteam/`）；GLM 本身的用量由 `zcode` 行承载
  （`zcode→zai`）。

## 6. 聚合

### 6.1 tokscale 侧：解析期计价 + 分组聚合

- **聚合键**：`GroupBy` 枚举（`crates/tokscale-core/src/lib.rs:207-223`）决定分组维度。两类字符串
  取值不同：**回显/输出值**来自 `Display`（`:225-237`，`--json` 里的 `groupBy` 场即此），为
  `model` / `client,model`（默认）/ `client,provider,model` / `workspace,model` / `session,model` /
  `client,session,model` / `client,workspace,session,model`；**CLI 接受值**来自 `FromStr`
  （`:239-261`），在输出形态之外还接受连字符写法，以及两个简写——`session` → `Session`、
  `client,session` → `ClientSession`（即 `session` 输出时会回显为 `session,model`）。
  聚合时按各维度拼接 key（`:4567-4584`）。
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
- **缓存**：256 分片 bincode 缓存（分片与指纹采样常量在 `message_cache.rs:65-76`，
  `CACHE_SHARD_COUNT` / `FINGERPRINT_SAMPLE_BYTES` / `FINGERPRINT_SAMPLE_POINTS` 具体在 `:72-75`）；
  采样哈希实现是 `compute_sample_hashes`（`:3424-3438`）配 `sample_offsets`（`:3440-3465`，每文件最多
  5 个采样点、各 4 KiB）与 FNV-1a 的 `hash_bytes`（`:3467-3474`）；条目 API `get`（`#[cfg(test)]`，
  `:2754-2755`，仅供测试断言）与 `take`（生产扫描的唯一入口，`:2788`）。另有 `recovery.rs` 作为
  本地灾难恢复叠加层，只补缺口（`recovery.rs:1-24`）。
- **补充（消息层二次过滤）**：`--client` 不是简单相等匹配——扫描后再过
  `retain_for_requested_clients`（`lib.rs:189-204`）：请求 `claude` 时额外保留 `cc-mirror/` 前缀的
  镜像行；请求 `gjc` 时额外保留 `9router` 桥接行（反向不成立，`--client 9router` 不保留原生 gjc 行）；
  `synthetic` 走 `sessions::synthetic::matches_synthetic_filter` 专表匹配。复刻过滤逻辑若按字面
  实现，会漏掉镜像/桥接来源的用量。

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
- **reasoning 是否并入总量由客户端白名单决定**：`TOKSCALE_DISJOINT_REASONING_CLIENTS`
  （`src/shared/history.js:103`）恰好 12 个客户端——`reasonix`、`antigravity`、`codex`、`droid`、`dsh`、
  `zcode`、`opencode`、`muse`、`codebuddy`、`workbuddy`、`cherrystudio`、`hermes`。只有这些客户端的
  `reasoning` 是**独立桶**（TK 把与 output 的重叠减掉了），所以这里要把 reasoning 加回输出家族桶，
  总量才闭合（`history.js:105-119`、`src/shared/usage.js:100-115`，导入点 `:10`）；
  白名单之外 reasoning 已被含在 output 里，照抄这套并桶就会重复计数。

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
| 客户端用量归档 | 设置内 JSON（`src/shared/usage/clientUsageArchive.js`） | 被用户取消勾选的客户端的历史用量 | **捕获/剪除的唯一入口**是 `settings.clients` 变化（`src/electron/main.js:7711` 调用、`:2773-2780` 实现）：被移除的客户端被捕获进归档，被启用的客户端其归档条目被剪除（`clientUsageArchive.js:511-525`）；该字段另在读取设置与设置补丁时被规范化（`:2638`、`:7710`），这两处都是重建式规范化、只能丢弃条目而不能新增：`normalizeArchivedClientUsage`（`clientUsageArchive.js:97-130`）会跳过畸形条目与三期皆无用量的条目（`:103-105`、`:120-126`），既不捕获也不按启用状态剪除 |
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

细节（**已验证**）：stale 阈值 = `max(staleAfterMs, 2 × syncUploadIntervalMs)`
（公式在 `src/shared/syncUploadInterval.js:21-29`，核心 `:28`；调用点 `src/shared/usage.js:1810`、
判定 `:1811`——`:1783` 的 `isPeriodExpired` 与 `:1869` 的 `toFixed(6)` 都是另外的机制）；
today/month 是否过期取决于设备自带的 `periodWindows.endsAt`（由设备用本地时区算好，
`src/shared/collector.js:654-665`），这样离线设备不会把昨天的「今天」永久累加。
呈现层把本机记录覆盖到 Hub 快照上（同 deviceId 替换），并保留 `hubReceivedAt` 以便区分
「本机采集成功」与「上传成功」（`src/electron/syncDisplayStats.js:121`、`:140`）。

## 7. 金额是怎么算出来的

### 7.1 TL;DR

```text
解析器给出 token 桶 + 可能的供应商上报成本
  ├─ 有权威成本（cost_source = ProviderReported）→ 直接用，不再定价
  └─ 否则 → PricingService 定价：
        custom-pricing.json → unpriced 命名空间拒赔 → self-hosted 零价 → 路由标签拒赔 → 别名
        → 精确/归一化/前缀（LiteLLM/OpenRouter/models.dev 三源仲裁）
        → Cursor/Sakana 兜底 → archive → 模糊匹配
        → 三段剥离重试（/ 前缀回退 → 后缀侵蚀（禁 archive）→ 前缀剥离）
        → 未命中（成本 0，标记 Unknown）
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

OpenRouter 源带**作者前缀硬过滤**：`get_author_provider_name`（`pricing/openrouter.rs:78-96`）只认
12 个作者前缀（`z-ai`、`x-ai`、`anthropic`、`openai`、`google`、`meta-llama`、`mistralai`、
`deepseek`、`qwen`、`cohere`、`perplexity`、`moonshotai`），抓取时整条丢弃其余模型
（过滤点 `:330-333`，此后只为命中的模型发起 endpoints 请求 `:339-358`）——未命中的模型连
`/models` 的列表价都不进数据集（`:307-324` 的列表价只会作为命中者的 fallback 参与）。

三源并发抓取（`tokio::join!`，`pricing/mod.rs:416-427`），单源失败不致命：逐源降级到
**任意年龄**的旧缓存（`pricing/mod.rs:435-489`）；缓存 TTL 3600 秒、原子替换写
（`pricing/cache.rs:6`、`:43-49`、`:86-96`）；抓取超时 30 秒 / 连接 10 秒（`pricing/fetch.rs:9-10`），
最多 3 次尝试，两次尝试之间只退避 200ms 与 400ms（`:4` 为 `MAX_RETRIES = 3`，`:22` 的
`for attempt in 0..MAX_RETRIES` 配 `:40-42` 的 `INITIAL_BACKOFF_MS * (1 << attempt)` 被
`attempt < MAX_RETRIES - 1` 截断，**800ms 分支不可达**）。

离线开关 `TOKSCALE_PRICING_CACHE_ONLY=1` 的边界要说准（源码 + 实证）：它只作用于**本地解析
（扫描）**路径：`load_pricing_for_local_parse()` 命中该变量即改走 `load_cached_any_age()`
（`crates/tokscale-core/src/lib.rs:5896-5911`，判定 `:5897-5902`），空缓存下不抓取、不落价格文件、
`cost` 记 0（实证：该路径只写出 `cache/source-message-cache*`，不产出 `pricing-*.json`）。
**`pricing` 子命令不受它约束**：全仓生产代码检索该变量只有 `lib.rs:5897` 一处命中，而 `pricing`
走 `PricingService::get_or_init()`（`crates/tokscale-cli/src/main.rs:3857`，即三源并发抓取
`pricing/mod.rs:416-427`）。实证：全新空 config + 该变量为 1 时执行
`pricing claude-sonnet-4-5 --json --no-spinner` 仍 exit 0 并返回 `source: LiteLLM`，同时往
`<config>/cache/` 写入 `pricing-litellm.json`（2,741,427 B）、`pricing-models-dev.json`
（6,934,570 B）、`pricing-openrouter.json`（257,108 B）；TK 自己的测试注释也写明「`pricing`
intentionally bypasses TOKSCALE_PRICING_CACHE_ONLY」（`crates/tokscale-cli/tests/cli_tests.rs:1027-1029`）。
结论：把该开关当作「离线/隔离」的保证**不成立**，任何包含 `pricing` 调用的链路仍会联网并落盘。

本 pin 的**生产路径没有**编译期打包的价格数据集：全仓 `include_str!` / `include_bytes!` 只有 4 处，
且全部位于测试或 fixture——`pricing/mod.rs:770`（在 `#[cfg(test)]` 模块内，该模块自 `:718` 起，
加载 `crates/tokscale-core/tests/fixtures/models_dev_pricing.json`）、`sessions/codex.rs:1844`、`:1846`、
`crates/tokscale-core/tests/anthropic_catalog.rs:31`；`tokscale-core` 没有 `build.rs`，
唯一的 `crates/tokscale-cli/build.rs` 只处理 macOS `apple-fm` 动态库，与价格无关。
`pricing/mod.rs:470` 注释里的「custom and bundled pricing」指的是**编译期硬编码兜底表**
（Cursor `:123`、Sakana `:227`、archive `:285`，加 `pricing/self_hosted.rs:7`），不是打包的数据集；
`lib.rs:5708` 的「bundled LiteLLM dataset」是同一措辞的遗留用法（见 §9 第 15 条）。

### 7.3 模型匹配与别名

匹配顺序（`pricing/mod.rs:570-586`、`pricing/lookup.rs:619-815`、`:821-1156`、`:3637-3691`）：

1. 自定义价（`lookup_custom`，命中即返回 `source: "Custom"`）；
2. provider 命名空间标为 unpriced 的**直接拒赔**：`unpriced` 或 `unpriced:*` 前缀
   （`pricing/lookup.rs:631-633`，判据 `:3165-3168`）；fork 侧用它标注那些「看着像模型、
   实为路由档位」的行，宁可保持成本未知也不声称权威 $0（`token_monitor/qodercn.rs:30` 的
   `unpriced:qodercn`；档位集合 `:71` 含 `efficient`，`:53` 是它到显示名的映射，闸门在
   `:188-197`，注释注明 Unsloth 同走这个约定）；
3. self-hosted 的零价表（`lookup.rs:635-639`）；
4. 路由占位标签拒赔：裸 `auto` 与 `agent_review` 是「router 的名字，不是模型」
   （`ROUTING_LABELS` `:3150`），命中即不给价（`:641-646`）；层级后缀归一化之后
   还要按同一集合再挡一次（`:663-668`）——**与 reseller 无关**，转售商身份只在第 6 条
   的仲裁里参与决胜；
5. 别名表（`pricing/aliases.rs` 里是**两张独立的表**，消费点不同）：通用模型别名
   `MODEL_ALIASES`（`:22` 起；别名解析在 `lookup.rs:649` 触发，查询键的小写折叠在被调方
   `aliases.rs:269` 的 `model_id.to_lowercase()`，`lookup.rs:652` 的 `to_lowercase` 作用于解析后的
   canonical id、供后续精确/归一化查找）——例 `big-pickle`→`glm-4.7`（`:24`）、
   `k2p6`→`kimi-k2.6`（`:29`）、`kimi-for-coding`→`kimi-k2.7-code`（`:36`）、
   `k3-256k`→`kimi-k3`（`:42`）、`minimax-m3`→`minimax/MiniMax-M3`（`:63`）、
   `model_placeholder_m26`→`claude-opus-4-6`（`:65`，源码键为小写，靠 `aliases.rs:269` 的折叠命中）；
   Cursor 层级映射 `CURSOR_PRICING_ALIASES`（`:4-20`）——例 `cursor-grok-4.6-xhigh`→`grok-4.6`
   （`:13` 列表项、`:15` 插入）、`cursor-grok-4.6-medium-fast`→`grok-4.6`（`:12`）、
   `grok-composer-2.5`→`composer-2.5`（`:17`）、`grok-composer-2.5-fast`→`composer-2.5-fast`（`:18`），
   仅当 `aliases::uses_cursor_pricing` 命中时才走 `exact_match_cursor`（`lookup.rs:692-696`）；
6. 精确 → 归一化 → 前缀匹配；多源命中时分**两层**仲裁：`choose_best_source_result(litellm, openrouter,
   provider_id)` 只比较 LiteLLM 与 OpenRouter 两源（`pricing/lookup.rs:3637-3691`，依次按 provider hint、
   key root、原始供应商/转售商身份决胜，最后默认取 LiteLLM），models.dev 由第二层
   `choose_best_source_result_with_models_dev(...)` 纳入（`:3696-3715`：先跑第一层，仅当 models.dev
   命中 provider root **且**第一层胜者是跨供应商别名时才改由 models.dev 接管）；
7. Cursor / Sakana 兜底表 → archive → 模糊匹配（`:1089-1096`、`:1102-1109`、`:1116-1120`、
   `:1122-1155`）；
8. **三段「剥离重试」**（步骤 6/7 整链未命中后，用逐段剥出的候选再跑一遍上面的查找，
   `lookup.rs:769-812`）：① `/` 前缀回退——取最后一个 `/` 之后的一段重试（`cx/gpt-5.5` →
   `gpt-5.5`，`strip_generic_provider_prefix` `:2806-2812`），终结段是路由标签即直接拒赔
   （`:784-786`），否则先按终结段查、再叠加后缀侵蚀（`:788-800`）；它存在的理由是 router/proxy
   分配的 `/` 前缀不在 curated `PROVIDER_PREFIXES` 里，两半各自能命中而组合起来计 $0
   （`:769-778`、`:792-797`）；② 后缀侵蚀——按 `-` 从尾部逐段剥离（`gpt-5.5-xhigh` →
   `gpt-5.5`，`try_strip_unknown_suffix` `:2968-2999`，最多 `MAX_SUFFIX_STRIP_SEGMENTS = 4`
   段 `:103`，候选长度须 ≥ `MIN_MODEL_NAME_LEN = 2` `:95`，另带 claude 4.x minor 保护
   `:2972-2974`、`:3001-3019`）；③ 前缀剥离——按 `-` 从头部逐段剥离
   （`antigravity-claude-…` → `claude-…`，`try_strip_unknown_prefix` `:3060-3094`，最多
   `MAX_PREFIX_STRIP_SEGMENTS = 2` 段 `:99`，每个前缀候选再叠加一次后缀侵蚀 `:3086-3089`）。
   **侵蚀候选禁走 archive**：②与③叠加的后缀侵蚀把 `allow_archive` 置 false（`eroded_lookup`
   `:765-767`，理由 `:817-820`、`:2960-2967`——archive 存的是某个模型自己最后公布的价目，
   不是家族价，拿它回答一个谁都没发布过的 id，会把这个未知 SKU 按邻居的费率计价）；前缀剥离的
   候选仍保留 archive（它本身是有人真实发布过的 id、只是被客户端打了前缀，`:3057-3059`）。

`crates/tokscale-core/src/provider_identity.rs` 负责供应商拼写归一（阻止把区域变体（如 `alibaba-cn`）折叠进总部键），
影响选价与证据等级；`crates/tokscale-core/src/model_alias.rs` 明确只做**展示合并**、不影响计价。
分层仲裁的调用点：`lookup.rs:871`、`:910`、`:961`、`:1016` 走两层；
`:1148`（模糊匹配分支）与 `:1348`（provider-scoped 路径）只调第一层，因为该两处的候选集合只有
LiteLLM 与 OpenRouter。复刻这套优先级时两层都要处理。

### 7.4 费率结构、分档与缓存写价

- 费率是 **per-token USD** 的 17 字段结构（`pricing/litellm.rs:10-28`，`all_rates()` 恰为
  `[Option<f64>; 17]`，`:40`）。字段**不是四类各四档**：input 与 output 各有 base +
  128k / 200k / 256k / 272k 共 5 个（计价走 `lookup.rs:2344-2387`）；cache read 只有 base +
  200k / 272k 共 3 个（`:2398-2411`，注释 `:2388-2397` 说明上游不声明 128k / 256k 缓存读价）；
  cache write（cache_creation）为 base + 200k 共 2 个，另加 2 个 **1 小时专用**字段
  `cache_creation_input_token_cost_above_1hr` 与 `..._above_1hr_above_200k_tokens`
  （`litellm.rs:21-24`）——「4 个」是 cache_creation 系统的字段总数，不是 1 小时字段数。
  **三份远程数据集里只有 LiteLLM 提供长上下文分档**（自定义价格文件同样支持，见本条后段）：
  models.dev 的 `cost_to_pricing` 只映射 input / output / cache_read / cache_write 四个基础
  字段（`pricing/models_dev.rs:95-106`，其余 `..Default::default()`）；OpenRouter 的 `/models`
  兜底路径只映射 input/output（`pricing/openrouter.rs:311-321`，`:317-318` 显式把两个 cache
  字段置 `None`），而抓取时真正生效的 endpoints 路径会多产 cache read / cache write
  （`:158-174`，生产消费点 `:152` 与 `select_endpoint_pricing` `:216-252`）——两条路径都不映射
  `*_above_*` 字段（该文件全文检索 `above_` 零命中），所以 128k 至 272k 档位不会来自这两源；
  另注意该源的覆盖面受作者前缀过滤限定：只有 §7.2 列出的 12 个前缀下的模型才进入数据集
  （`pricing/openrouter.rs:330-333`），endpoints 路径也只服务这批模型。
  自定义价格**支持**档位：`CustomModelPricing` 声明了 input/output 的
  `above_{128k,200k,256k,272k}`、cache_creation 的 `above_200k` 与两个 1 小时档、cache_read 的
  `above_200k` 与 `above_272k`（`pricing/custom.rs:38-73`），`into_model_pricing` 两者逐档写进
  `ModelPricing`（`:106-199`）；只是 TM 设置页只写 §7.5 表里那五个基础键，档位只会在手工编辑
  `custom-pricing.json` 时出现。
  models.dev 与自定义价格的 per-million 输入会除以 1e6（`pricing/models_dev.rs:8`、`:108-113`；
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
- **补充（属自定义价格限制，§7.5 口径的读取侧）**：自定义价格文件有硬上限——超过 16 MiB
  直接退化为空表并记警告（`pricing/custom.rs:13`、`:222-232`）；模型条数上限 10 000 只作
  `HashMap::with_capacity` 的容量上界（`:14`、`:248`、`:307`、`:408`），既不截断也不丢弃
  超限条目；文件缺失/读取失败/JSON 解析失败同样退化为空表且不阻断
  扫描，单条 malformed 只跳过该条（`:213-241`、`:297-324`）。
- **补充（「全零费率视为覆盖任意用量」）**：某行的 base input 与 output 都已公布且**所有**已公布
  费率（含各档）都是 0 时，`quotes_zero_for_every_published_rate()` 为真（`pricing/litellm.rs:124-136`），
  `covers_usage()` 因此直接判为覆盖任意已填充桶（含 cache 桶，`:141-154`）——这把「显式免费」与
  「未定价」分开，只影响覆盖/提交判定，不改变总价（缺失费率本就按 0 读）。一行里「零基础价 + 付费
  高档」属促销行，不算全零。

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
  刷新前有 `ownsUsageRuntime()` 前置判断（`src/electron/main.js:6508`）：没有本地 usage runtime
  时只落盘文件、不触发刷新。

**TK 侧**：在构造 `PricingService` 时按默认路径读取该文件（`pricing/mod.rs:430`），custom 表置于
所有目录源之前（`:560`、`:692`）。覆盖语义是「整条消息成本替换，但要求价格覆盖该行的**全部**
已填充桶」——`calculate_custom_cost()` 先做 `covers_usage()` 检查，不覆盖则返回 `None`
（保留来源成本），覆盖成功才写 `message.cost` 并标记为估算，且**不叠加**服务档/托管倍率
（`pricing/mod.rs:636-657`、`lib.rs:5803-5821`）。显式 0 是合法价
（`pricing/litellm.rs:141`、`pricing/custom.rs:90-104`）。

### 7.6 未知模型与「不猜」

- 未命中价格：解析期成本为 0，`cost_source` 保持 `Unknown`（`crates/tokscale-core/src/sessions/mod.rs:71-78`），
  聚合只把 0 加进去；上层展示时必须按「未知」呈现，而不是当作免费。
- **无价源时同样是 0 且不报错（实证）**：隔离 config + `TOKSCALE_PRICING_CACHE_ONLY=1`（缓存为空）
  下扫描 exit 0，`cost` 与 `totalCost` 都是 `0.0`、stdout 无任何告警；同一 fixture 写入
  `custom-pricing.json` 后同一条命令立刻给出 `43100.0`（与手算逐位相同），移除自定义价改由目录源
  （LiteLLM）计价则是 `0.04515`。也就是说 `cost == 0` 同时覆盖三种情形：模型未收录、价格源整体
  缺失、显式免费价，而 `--json` 的 entry **不输出 `cost_source` 键**（实证，与 §1 第 3 条的字段
  清单一致），上层从输出里拿不到「这 0 是未知还是免费」的标记。落地口径：`cost = 0` 只读作「未知」，
  展示与聚合都不得把它当免费或当精确值；要区分就得在 TK 之外自己记录价源是否在场（TM 的做法是
  `pricingFingerprint`，见 §7.7）。
- 只有提交（leaderboard/submit）路径才强制要求价格覆盖，并把不完整成本记为
  `incomplete_cost_dates`（`lib.rs:5378-5443`、`pricing/mod.rs:659-669` 的
  `covers_usage_with_provider`）。
- `model_attribution_conflicted` 在 `tokscale-core` 的定价路径里**只出现于自定义价覆盖分支**：
  `apply_custom_pricing_if_available`（`lib.rs:5803-5821`）在 `:5809` 处「conflicted 或无正 token
  用量即返回」，理由是模型证据矛盾时无法认定用户手填的覆盖价该套到哪个 id。常规定价路径
  `apply_pricing_if_available`（`:5823-5845`）**不检查**该字段——它只跳过
  `has_authoritative_cost()` 为真的行（`:5827-5829`），因此 conflicted 行在常规路径下仍按
  `model_id` 查价格表；未命中、或命中但算出 0（显式免费价、全零费率行，见 §7.4）都不写成本，
  保持 `Unknown` + 0（`:5841` 只在 `calculated_cost > 0.0` 时写入并标记估算）。但这不等于全仓
  只有一道闸门：CLI 的 wiki 记录路径另有同义判断——`compute_msg_cost`
  （`crates/tokscale-cli/src/commands/report.rs:1572-1577`）函数体开头即
  `if msg.model_attribution_conflicted { return msg.cost; }`（`:1575-1577`），
  由 `record_new_sessions`（`:132-136`，消费点 `:167`）调用，同样直接保留解析器原始成本。

### 7.7 TM 侧与成本相关的三处规则

1. **成本锚失效**（§3.1 提到的 `pricingFingerprint`）：价格文件字节 + 二进制 identity + 二进制
   `stat(size/mtime/ctime)` 变化 → 锚点作废、强制全量重扫（`src/shared/collector.js:2151-2186`、
   `:2774-2788`）。
2. **混合 revision 不发布**：扫描过程中价格文件变化时本轮结果丢弃并最多自动重放一次
   （`:2949-2953`、`:3183-3199`，单次闸门 `:3186`）。
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
   若来源日志会被重写/回收，则该恒等式不成立（注释给的就是这条边界，
   `src/shared/usage.js:1894-1900`；数值核心在函数体 `:1943-1944`）。
4. **分区键是客户端 id，缺失的语义按目标数量分档**：定向扫描按 client 分片，重建时只替换变化分区。
   **单个目标**的分区缺失按空周期处理，不是错误：`replaceTodayPartitions()` 先把它置为
   `emptyPeriod()` 再用 fresh 覆盖（`src/shared/collector.js:2121-2126`，置位在 `:2123`）。
   只有**多个目标**中某个分区缺失（`targetClients.length > 1`，`:1018-1021`）且本轮确实是定向扫描
   （`:1032`）时，才视为结果不可信并整轮重扫全量 client（`:1037-1040`）。
   把「任何一个分区缺失都不可信」当成规则会与实现相反（§3.4 同口径）。
5. **价格或二进制变化必须让成本锚失效**：否则会出现「旧价算旧数、新价算新数」混合展示
   （`:2151-2186`、`:2774-2788`）。
6. **扫描期间的价格或来源变化不允许发布**：整轮结果丢弃并最多自动重放一次
   （`:2949-2953`、`:3183-3199`，单次闸门 `:3186`）。
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
    （口径级证据：`docs/API.md:305`，来源文件删除后的会话用量保留对**任何** client 生效、
    含被跟踪的 client；归档模块内只见到三类删除，都不以来源为条件：按日期裁剪与 prune 类删除
    （`usage/sessionUsageArchive.js:125-155` 的 `pruneExpiredSessionUsagePeriods`、
    `usage/clientUsageArchive.js:511-525`、`dailyHistoryArchive.js:343-347` 与 `:560-563`
    只删**晚于今天**的日期）、按 revision 追增量时的内存映射删除
    （`sessionUsageArchiveStore.js:133-136` 把解析不出的条目从 `archive.sessions` 映射里
    `delete`，不动任何文件）、以及文件替换/清空用的 unlink（同文件 `:20-22` 与 `:186` 的
    legacy 清理、`:392-393` 的 `clear`、`dailyHistoryArchive.js:724-726`、
    `usage/sessionUsageArchive.js:459-461`），未见「按来源删除归档条目」的代码路径）。
15. **同步记录里不含会话正文**：设备记录只带用量与身份，`sessions[]` 在跨进程前被剥离，
    renderer 按需单独拉取（`src/shared/usage.js:184-224` 的 `stripSessionTextFromPeriod` /
    `stripSessionTextFromDeviceRecord`）。

## 9. 实证结论、源码定论与仍存的边界

本节分三层：**已实证**（在隔离环境里真实运行 pin 的二进制得到的行为，附执行要点与源码对照）、
**已由源码定论**（只读源码即可判定、没有未决分支）、**仍存在的边界**（证据不足或运行面未覆盖，
显式保留而不下结论）。三层条目连续编号，每条标注证据档位。

**实证环境与隔离方式**：全部命令在临时根目录
`C:\Users\admin\AppData\Local\Temp\tm-research\empirical\` 内执行。被验证的二进制是 release tag
`token-monitor-fa5f0c13` 的 `tokscale-win32-x64.exe`（26,491,904 字节），其 sha256 与实际下载
文件、manifest 声明、Release `SHA256SUMS` 三方一致
（`5887f4fae41d5ae4ca684c4a50af96dff9b633fbf9942cd827eca9736765d97a`），tag 也解引用到 manifest
记录的同一 commit。每次调用前注入 `TOKSCALE_CONFIG_DIR`（配置与 `cache\` 全部落在临时目录）、
`TOKSCALE_PRICING_CACHE_ONLY=1`，把 `HOME`/`USERPROFILE` 重定向到临时 home，并删除 `DSH_HOME`、
`CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`GEMINI_CLI_HOME`、`XDG_DATA_HOME`、`TOKSCALE_EXTRA_DIRS`
等 21 个 ambient 变量；扫描类命令一律显式带 `--home <临时 home>`；fixture 全部手工合成，不含
任何真实会话内容。

**隔离偏差（如实记录）**：执行 `clients` 的一次调用没有带 `--home`，进程内 ambient 的
`DSH_HOME=C:\Users\admin\.dsh` 生效，该命令列出了真实 `C:\Users\admin\.dsh\sessions` 的路径并
显示 `messages: 13.3K` 计数。这只是一次路径存在性加消息计数的读取，没有读到任何会话内容；
发现后立即清除该类变量（含全部 `*_HOME`），其后所有命令（含实验 7、8）均未再触达真实目录。
复现该环境时必须保持这些变量被清除，否则 `clients` 与不带 `--home` 的扫描仍会读到真实 profile。

### 9.1 已实证（第 1 至 9 条）

1. **未知 `--client` 的判据成立，两段判据都不可省（实证）**。执行
   `--client definitely-not-a-client --today` 得 `exit 2`，stderr 首行为
   `error: invalid value 'definitely-not-a-client' for '--client <CLIENTS>'`，stdout 全空，
   TM 的「exit 2 且 stderr 命中 `/--client/i`」合取判据（`src/shared/collector.js:474-478`，常量
   与注释 `:316-318`）因此成立。但 `--not-a-flag-at-all` 同样 `exit 2` 而 stderr 不含 `--client`
   （首行是 `error: unexpected argument '--not-a-flag-at-all' found`，其后为 usage 与 `--help`
   提示两行），证明 exit 2 是 clap 的通用参数错误码（TM 注释 `:446-450` 判断正确），只看退出码
   会把无关的参数错误误判成「未知 client」。
   坏 `--group-by` 走**应用层** `GroupBy::from_str`：文本由
   `crates/tokscale-core/src/lib.rs:256-259` 产出，CLI 在
   `crates/tokscale-cli/src/main.rs:683-686` 打印 `Error: <文本>` 后 `std::process::exit(1)`，
   实测**退出码 1**（不是 2）。因此 TM 对 group-by 只匹配文本、不匹配退出码
   （`collector.js:467-472`）是必要写法；「坏参数一律 exit 2」在此被推翻。探测与重试链路见 §3.1。
2. **`--version` 不暴露 fork 身份（实证）**。输出为 `tokscale 4.18.0`，与 manifest `baseVersion`
   一致；只有基线版本号，不含 fork/commit 标识，无法用它区分 fork 与上游同版本构建。资产
   sha256 三方一致见本节导语。
3. **`--client` 可用值实为 58 项（实证 + 源码）**。`--help` 的 `[possible values]` 与未知 client
   的 stderr 列表逐项有序相同（58 = 58，输出为 UTF-8 无 BOM）。构成是 core 的 56 个 `ClientId`
   （`crates/tokscale-core/src/clients.rs:320-1106`，`ClientId::COUNT == 56` 断言在 `:1218-1221`）
   加 CLI 层独有的两项：`9router`（`main.rs:1119-1120` 的 `#[value(name = "9router")]`，`:1263`
   把它映射到 `ClientId::Gjc`，是筛选别名）与 `synthetic`（`main.rs:1158` 声明、`:1224` 给出
   `"synthetic"` 值映射的 meta-client，`:1293` 没有对应 `ClientId`，扫描期 opt-in，默认集
   `main.rs:1388-1394` 排除两者）。fork 独有的
   `proma`/`catpaw`/`qodercn` 一个都不在 help 里；TM 自己的 catalog 是另一张 36 项的表
   （`src/shared/clientCatalog.js:50-95`，含这三个 `forkOnly: true` 项），所以发现 fork-only
   客户端不能靠解析 help。
4. **Claude fixture 端到端（实证 + 源码）**。合成 JSONL（1 条 user、2 条 assistant）扫描结果：
   `input=1500`、`output=300`、`cacheRead=3000`、`cacheWrite=7000`（写入的 1 小时子集 4000 计入
   总数）、`reasoning=0`、`messageCount=2`、`provider=anthropic`，顶层 `total*` 与 entries 一致；
   `--group-by client,session,model` 下 `sessions[]` 的键为 camelCase
   `client`/`sessionId`/`firstActiveMs`/`lastActiveMs`。输出 JSON 全文**没有** `cacheWrite1h` 键
   （core 的 `TokenBreakdown` 有该字段，`--json` 不导出）。会话身份取**文件名 stem**：文件
   `session-1111.jsonl` 内部 `sessionId` 写 UUID 时输出仍是 `session-1111`，另一文件内部写
   `internal-only-zzz`、输出仍是文件名 UUID，与 `sessions/claudecode.rs:444-448` 的
   `path.file_stem()` 一致（只有子代理回放分支改用 parent id）。TK 的 Claude 去重键是
   `"<message.id>:<requestId>"`（`claudecode.rs:632-636`），而会话身份来自文件名，所以重命名文件
   即换身份。
5. **自定义价格的换算、优先级与 1 小时档（实证 + 源码）**。写入 `<config>/custom-pricing.json`
   （键名取自 `crates/tokscale-core/src/pricing/custom.rs:39-72`）后，`pricing <model> --json`
   返回 `source: Custom`、`inputCostPerToken = 1.0`，即「每百万价 ÷ 1e6」精确成立；同一 fixture
   扫描得 `cost = 43100.0`，与手算（1500×1 + 300×2 + 3000×3 + 3000×4 + 4000×5）逐位相同，说明
   1 小时档独立计价且仍并入 `cacheWrite` 输出。移除该文件后回落到 `source: LiteLLM`、
   `cost = 0.04515`，反解出的 1 小时档价 6e-6 与 `cache/pricing-litellm.json` 里该模型的
   `cache_creation_input_token_cost_above_1hr` 逐位一致（5 分钟档 3.75e-6）。此时 `cache\` 已由
   先前的 `pricing` 调用落盘三份目录源快照，所以这是「自定义价优先于目录源」的直接证据（优先级
   判定在查找顺序上：`pricing/mod.rs:559-563` 未强制来源时先查 `lookup_custom`、命中即返回；
   `custom.rs:76-88` 只是条目到 `ModelPricing` 的换算，不是判定点）。`pricing` 子命令的返回体
   只暴露 4 个价字段，1 小时档价只能由扫描结果反推。
6. **`.jsonl.zst` 能被正常发现并解压解析（实证）**。Codex 风格 `.jsonl.zst`（277 字节）扫描成功：
   `input=800`（Codex 口径的 `input_tokens − cached_input_tokens`）、`cacheRead=200`、`output=50`、
   `messageCount=1`、`cost > 0`（价格缓存已在场）。解压实现见第 10 条。
7. **无价源时 `cost=0` 且不报错（实证）**。空 config 加 `TOKSCALE_PRICING_CACHE_ONLY=1` 时扫描仍
   成功（exit 0），`cost`/`totalCost` 记 `0.0`，stdout 无告警；同一 fixture 一旦有价格缓存立刻
   变成 0.04515。上层不能把 `cost=0` 读作「免费」，只能读作「未知」。
8. **`TOKSCALE_PRICING_CACHE_ONLY=1` 只约束本地解析路径（实证 + 源码）**。空 config 下扫描不抓价、
   不落价格文件，与 `lib.rs:5896-5902`（命中该变量即改走 `load_cached_any_age()`）一致。但
   `pricing` 子命令**不受该开关约束**：空 config 加该变量为 1 时仍联网抓取并写入
   `cache\pricing-litellm.json`（2.7 MB）、`pricing-models-dev.json`（6.9 MB）、
   `pricing-openrouter.json`（257 KB），然后返回 `source: LiteLLM`。任何「离线/隔离」假设只要
   包含 `pricing` 调用就不成立。
9. **只读扫描不改动 `.db` 与 `-wal`，但会重写 `-shm`（实证 + 源码）**。Zed 的 WAL 库
   （`wal_autocheckpoint=0`，写入 1 行后保持连接不 checkpoint）在扫描前后做同进程快照：
   `threads.db` 与 `threads.db-wal` 的 size/mtime_ns/sha256 三者全同（未 checkpoint、未追加、
   未截断），`threads.db-shm` 的 sha256 变化（`f8677332…` 变 `c8b41a24…`）而 size/mtime_ns 不变；
   不扫描的对照组三者全不变，证明 `-shm` 改写由本次读取引起。代码侧只保证扫描层不吃侧车文件
   （`scanner.rs:676-679` 排除 `.db-wal`/`.db-shm`/`.db-journal`），只读打开会按 SQLite 语义回放
   `-wal`（`sessions/cherrystudio.rs:647-651` 注释）。判「来源未变」时，对 `.db` 与 `-wal` 用
   size+mtime 或内容哈希都安全，对 `-shm` 用内容哈希会在每次扫描后误判为「已变」。附带实测：
   扫描会写 TK 自己的配置目录缓存（`cache\source-message-cache-v2\<client>\shard-*.bin` 与空
   锁文件）；只读原则覆盖的是各 agent 的来源目录，不是 TK 自己的 config 目录。

### 9.2 已由源码定论（第 10 至 19 条）

10. **Codex 的 `.zst` 与 compaction 机制（精化原第 4 条）**。`.zst` 是整文件压缩，解压在
    `parse_codex_file_incremental` 内联完成（`crates/tokscale-core/src/sessions/codex.rs:1498-1559`），
    不是逐帧、也不是独立模块。两道硬上限：zstd 解压窗口 ≤ 2^26
    （`decoder.window_log_max(26)`，`:1539`），解码后字节数 ≤ 256 MiB（`MAX_ARCHIVE_BYTES`，
    `sessions/utils.rs:544-545`，配 `take(MAX_ARCHIVE_BYTES + 1)` 探测越界，`codex.rs:1551`）；
    超限或解析未完成即清空全部结果（`:1553-1557`），是丢弃而不是截断；`File::open`、
    `Decoder::new`、`window_log_max` 三种失败都退化为「空结果 + `parse_succeeded: false` + warn」
    （`:1510-1549`），不 panic、不重试。**`.zst` 不支持增量续读**：非零 `start_offset` 直接返回
    失败（`:1499-1508`），因为分支内 `consumed_offset` 是解码流偏移，与压缩文件偏移不是同一
    坐标，解析器选择拒绝而不是换算，`.zst` 因此只能整文件重解析。compaction 没有专门的事件分支：
    `(?i)compact` 在 `codex.rs` 只命中注释与一个测试名（`:883`、`:915`、`:997`、`:4279` 注释，
    `:4039` 测试名）。防重复计数靠「增量源选择 + 回退分档 + 基线不推进」三件事：`last_token_usage`
    是唯一增量源、`total_token_usage` 降级为基线校验（`:914-919`）；`total == previous` 直接跳过
    （`:958-960`）；小回退（`current×100 ≥ previous×98` 或 `current + 2×last ≥ previous`，
    `:236-251`）判为乱序陈旧快照并跳过，避免把 `last` 重复计两次；大回退把 `last` 计为本次增量、
    把回退后的 total 设为新基线（`:961-966`）；零快照跳过且不推进基线（`:996-1004`）；首个事件
    即带 total 时也只用 `last`（`:968-970`）。另需与 TM 自己的
    `src/shared/providers/codex/transcript.js:7-13` 多帧解压实现区分（32 MiB 输入 / 64 MiB 解码 /
    4096 帧上限），它与 TK 的 `.zst` 不是同一条链路。
11. **Antigravity 没有 ledger，跨 surface 去重靠 responseId 家族级（精化原第 5 条）**。
    `sessions/antigravity*.rs` 与 `src/token_monitor/` 检索 `ledger|reconcil` 零命中；ledger 对账
    只属于 Amp（`sessions/amp.rs:125` 的 `parse_amp_ledger_records`、`:218` 的
    `find_matching_ledger_record`、`:243` 的 `merge_amp_records`，对账循环 `:302-338`）。用量来自
    三条物理通道、各有独立 client id：IDE 腿吃 RPC 同步落下的 JSONL 缓存（`clients.rs:619-628`，
    `relative: antigravity-cache/sessions`），CLI 腿与 IDE 扩展腿直读各自的 `conversations/*.db`
    （`clients.rs:738-756`、`:1091-1105`），两条 DB 腿共用同一解析器，读数是
    `SELECT idx, data FROM gen_metadata`（SQL 在 `antigravity_cli.rs:113`；模块头与解析入口见
    `:1-14`、`:84-92`）里的 protobuf wire 字段。跨 surface 去重是两半互补：有 `responseId` 的行
    走家族级去重（`lib.rs:7362-7395`；缺 ID 的行不合并，注释理由是「token 数与时间相同不足以
    证明是同一次调用」）；缺 `responseId` 时靠
    TM「一个 custom root 只交给一个解析器」的交付策略（`src/shared/tokscaleClientMapping.js:8-13`，
    `customScanIds: ['antigravity', 'antigravity-cli']`）。
12. **SQLite 只读口径修正（精化原第 9 条）**。「TK 统一 `READ_ONLY | NO_MUTEX`」的准确范围是
    **core 侧来源库解析**：唯一 opener 在 `sessions/utils.rs:296-306`（`Connection::open_with_flags`
    在生产码仅此一处），`sqlite_for_each_row`（`:477-498`）等调用点全部复用它。需要列出的例外：
    `wiki.rs:94-96`（自家库，可创建、可写、显式设 WAL）；`crates/tokscale-cli/src/cursor.rs:417-425`
    （独立的 URI 只读 opener，注释明说**刻意不带** `NO_MUTEX`）；`token_monitor/catpaw.rs:386-390`
    （只读 opener 之上追加 `busy_timeout(3s)` 与 `PRAGMA query_only=ON`）；
    `sessions/antigravity_cli.rs:88-92`（`busy_timeout(100ms)` 加显式 `BEGIN`）、
    `sessions/openclaw.rs:624`（`busy_timeout` 常量）、`sessions/micode.rs:475-483`
    （`BEGIN DEFERRED`）。`journal_mode` 的写入除 wiki 自家库外全部落在测试段或测试文件；全仓没有
    SQL `ATTACH`。WAL 回放与 `-shm` 副作用见第 9 条。
13. **`cost_source` 恢复路径精化（精化原第 13 条）**。`recovered-usage-v1.json` 是本机灾难恢复
    overlay，触发面很窄：文件存在、`TOKSCALE_RECOVERY_DISABLE` 未设（`recovery.rs:19-24`），且
    文件内 `version == 1`、`home` 与本机一致（`applicable()` `:94-109`）。它只把**日兜底行**的
    `cost_source` 改写为 `Estimated`（`:487`），同时覆写 `agent`（`:489`）、
    `session_id = dedup_key`（`:493`、`:495`）、`timestamp = 0`（`:498`）；**请求级恢复行不改
    `cost_source`**，只加 `local-recovery:` 前缀并把 `agent` 写成 `Recovered request archive`
    （`:390-392`）。因此仅凭 `cost_source == Estimated` 无法与正常计价区分，判定要用结构标记
    `recovery::is_daily`（要求 `session_id == dedup_key` 且键以 `local-recovery:daily:` 开头，
    `:51-58`，注释解释了为什么不能只做朴素前缀判断）。日兜底行只在 native 合计低于台账 floor
    时才生成（`gap = (target - current).max(0)`，`:443-446`），所以「overlay 生效」不等于「一定
    有兜底行」。生产码没有该文件的写入者：该文件名的读取只有 `recovery.rs:20` 一处，其余命中都
    在说明文档与测试里（`README.md:1046`/`:2090` 与各语种译本的同段、`lib.rs:5531` 的注释、
    `cli_tests.rs:2005`/`:2039`），唯一构造它的是测试辅助 `cli_tests.rs:2005-2043`；提交与
    「本机是否已上报」路径明确不看 overlay（`lib.rs:5527-5535`），`Lenient` 模式遇到可用 overlay
    时改走非流式收集（`:5061-5076`），TUI 用 `is_daily` 区别对待
    （`crates/tokscale-cli/src/tui/data/mod.rs:833`、`:1196`、`:1285`）。
14. **03 报告 8 条推断的处置（精化原第 12 条）**。6 条可定论。`trae` 在 TM 侧未启用：36 项
    catalog 无 trae、`clientSources.js` 检索 `trae` 零命中、README 只给它额度列不给用量列；但
    TK 侧有完整通道且默认会扫：`clients.rs:649-658` 注册 `root: PathRoot::Config` 加
    `trae-cache/sessions/*.json`、`parse_local: true`，缓存由 CLI 自建
    （`crates/tokscale-cli/src/trae.rs:126-129`），`main.rs:1115-1116` 是合法 `--client trae`，
    默认全量走 `ClientId::ALL` 加 `synthetic`（`lib.rs:5044-5051`），而 `scanner.rs:1998-2029`
    的特殊跳过表不含 Trae。Hermes、Devin、Zed、Reasonix、Kiro 的 Windows 兜底在 TK 全部找到
    实现（`scanner.rs:1131-1167`、`:1546-1562`、`:2613-2646`、`:2758-2770`、`clients.rs:101-129`）。
    openclaw 三个历史拼写确实会被扫（`scanner.rs:2382-2392`，`clients` 命令输出同样列出，
    `main.rs:4502-4508`）。qoder、zaiteam、thirdparty 全走网络、没有本地库读取分支（只有
    env/settings 与 HTTP 端点）。micode 与 micode-desktop 是同库两身份，按 `session.version` 的
    `desktop-` 前缀拆分（`sessions/micode.rs:26-38`、`clients.rs:728-737`、`:1060-1075`），TM 侧
    同步点为 `tokscaleClientMapping.js:26-29`。仍存疑三点：`providers/antigravity/probe.js`
    （996 行）只拆读到 `:123`，额度端点、CSRF 与端口探测段未亲读；`providers/cursor/auth.js`
    （422 行）只确认 spawn 形状（`:202` 的 `[...prefixArgs, 'cursor', ...args]`）与两个调用点
    （`:396` 的 `sync --json`、`:404` 的 `status`），其余调用点未穷举；
    `micode`/`micode-desktop`/`antigravity-cli`/`antigravity-extension` 等 id 的跨仓同步是
    **版本契约**，本 pin 上一致，但静态分析无法排除上游未来漂移（TM 侧的防护是
    `tokscaleClientMapping.js` 单点同步）。
15. **「bundled」措辞与打包数据无关（保留原第 2 条）**：`crates/tokscale-core/src/lib.rs:5708`
    的「bundled LiteLLM dataset」是描述运行时那套 LiteLLM 数据的注释措辞；
    `crates/tokscale-core/src/pricing/mod.rs:470` 的「custom and bundled pricing」指的是**编译期
    硬编码兜底表**（Cursor `:123`、Sakana `:227`、archive `:285`，加 `self_hosted.rs:7`）。生产
    路径确实没有打包数据集：全仓 `include_str!` / `include_bytes!` 仅 4 处且全在测试/fixture
    （`pricing/mod.rs:770` 在 `#[cfg(test)]` 内、`sessions/codex.rs:1844`/`:1846`、
    `crates/tokscale-core/tests/anthropic_catalog.rs:31`），`tokscale-core` 无 `build.rs`，唯一构建
    脚本 `crates/tokscale-cli/build.rs` 只处理 macOS `apple-fm` 动态库。结论：**已验证**「无编译期
    价格数据集」，不必为「构建特性注入数据」保留余地。
16. **Claude 的 5 分钟缓存写档在计价层显式成立（保留原第 3 条）**：解析层只取 1 小时子集
    （`sessions/utils.rs:738-744` 的 `cache_write_1h_raw()` 读 `ephemeral_1h_input_tokens`，经
    `:730` 的 `min(cache_write)` 截断；全仓检索 `ephemeral_5m_input_tokens` 只命中
    `claudecode.rs:2016` 起的测试 fixture，生产代码不读它）。计价层按差额还原两档：
    `pricing/lookup.rs:2341-2342` 算 `cache_write_5m = cache_write − cache_write_1h`，
    `cache_write_1h` 先被 `min` 截断到不超过 `cache_write`，再各自分档计价后相加（`:2431-2459`，
    求和 `:2461`）。即 `cache_write` 桶是「5 分钟档 + 1 小时档」的合计，1 小时桶是其子集。
17. **`graph` 子命令的完整输出形状已逐字段核对（保留原第 6 条）**：13 个 `serde::Serialize`
    结构全在 `main.rs:4851-4992`，除 `DateRange`（`:4897-4901`）外都带
    `rename_all = "camelCase"`。顶层 `TsTokenContributionData`（`:4975-4992`：`meta`/`summary`/
    `years`/`contributions` 必填，`device`/`scanScope`/`timeMetrics`/`mcpServers`/`provenance`
    走 `skip_serializing_if` 可省），子结构 `TsTokenBreakdown`（`:4851-4859`）、
    `TsSourceContribution`（`:4861-4871`）、`TsDailyTotals`（`:4873-4883`）、
    `TsDailyContribution`（`:4885-4895`）、`DateRange`（`:4897-4901`）、`TsYearSummary`
    （`:4903-4910`）、`TsDataSummary`（`:4912-4923`）、`TsExportMeta`（`:4925-4931`）、
    `TsSubmitDevice`（`:4933-4939`）、`TsTimeMetrics`（`:4941-4948`）、`TsScanScope`（`:4957-4962`）、
    `TsSubmissionProvenance`（`:4964-4973`）；组装在 `to_ts_token_contribution_data`
    （`:4994-5099`：日汇总的 `costIsComplete` 由 `incomplete_cost_dates` 反推 `:5045-5048`、
    `mcpServers` 来自本地 MCP 发现 `:5089-5096`），输出路径 `run_graph_command`
    （`:5561-5653`：`:5584-5588` 生成、`:5597-5598` 序列化、`:5600-5601` 落盘、`:5649` 无
    `--output` 时打印到 stdout）。TM 侧对它只做 `contributions` + `summary` 的容错消费
    （`src/shared/collector.js:812-819`）。
18. **`crates/tokscale-core/src/token_monitor/js.rs` 不是 JS 引擎（保留原第 10 条）**：模块头注释
    即自述这是「ports of Token Monitor's JavaScript adapters」共用的值强制层（`:1-6`）；实现是
    JS 弱类型语义的 Rust 复刻：`MAX_SAFE_INTEGER` 上界（`:14`）、`Date` 上界（`:17`）、
    truthiness（`:20-28`）、`a || b || …` 取首个真值（`:31-35`）、
    `sha256(path.normalize(path)).slice(0, 12)` 的 session 命名空间（`:158-166`，取 6 字节即 12 个
    十六进制字符），用于复现 TM 侧既有解析行为与既有 session id。
19. **扫描并发上限已定论：`min(可用核数, 4)`（保留原第 11 条）**：常量与理由在
    `scanner.rs:1985-1989`（目录遍历阻塞在文件系统上，超过少数线程只会停摆争用），使用点
    `:2915-2916` 为 `available_parallelism().map_or(2, |cores| cores.get().min(SCAN_WORKER_CEILING))`，
    并据此建 rayon 线程池；池构建失败时退回全局池（`:2922-2926`，只多争用、不影响结果）。运行
    环境只影响「核数」，4 这个上限本身是常量。

### 9.3 仍存在的边界（第 20 至 22 条）

20. **shim 链路只有 TK 仓库的同源文件为证（保留原第 7 条）**：TM 工作副本没有 `node_modules`
    （也没有 `packages/` 目录），无法读取实际安装后的 `tokscale/bin.js`；
    `packages/tokscale/bin.js:1-2` 与 `packages/cli/src/index.ts:226-238` 只是同一发布物的源文件，
    这条链路属**一致解释**，不是实测。这条边界的性质是「安装产物不可读」，与第 21 条的
    「运行未启动」不是同一件事。
21. **TM 本体（Electron 应用）没有真正跑起来（更新原第 8 条）**：TK 侧已覆盖到二进制与源码两个
    层面，TM 侧仍只有源码证据。未启动 Electron 应用，因此采集 tick 的 IPC、渲染层行为、归档写盘
    （会话 SQLite、客户端/日历史 JSON）、self-sync 的实际落盘，以及 `clients` 之外的运行时发现
    路径都还没有实测；涉及「运行时实际写出什么」的字段（如 `codex-local-usage.sqlite` 的表结构）
    仍以代码中的建表语句为准。
22. **实证覆盖面的明确缺口（新增）**：未实测 `9router` 与 `synthetic` 的扫描语义差异（两者在 help
    里是独立值，`9router` 是 `gjc` 的筛选别名、`synthetic` 是 opt-in 后处理，但没有对照跑）；
    `-shm` 改写只在 Zed 一个 WAL 库上复现过，其它 client 与 `journal_mode=DELETE` 的库未重复
    实验；`sessions[]` 的 `title`/`parentSessionId` 与 `workspaces[]` 的键名未实测（fixture 没有
    标题与父子关系，也没触发 workspace 分组）；`--since`/`--until` 的边界语义未单独实测（只跑了
    `--today`，按本机分桶时区命中）；release 的另外 8 个平台资产未下载校验；压缩型来源只构造了
    Codex 的 `.jsonl.zst`；SQLite 类来源只实测了 Zed 一个 client。

### 9.4 复核方式

第一层 9 条来自隔离环境真实执行 pin 二进制（命令与输出留痕在
`C:\Users\admin\AppData\Local\Temp\tm-research\findings\empirical-1.md`），并与源码行逐一对照；
第二层为源码直读：第 10 至 14 条另经一轮独立源码复核（同目录 `source-recheck-1.md`，逐条核到
行）；第 15 至 19 条中，第 15、17、19 条属该报告的抽查范围，第 16、18 条另经补核，五条引用行均
未发现偏移；第三层是显式列出的未覆盖面，不下结论。
三层之间未发现冲突；若未来出现冲突，以可复现的实证结果为准。

## 10. 与 TokenScope 的对照要点

同样做「本地 AI 用量统计」，两者的架构路线几乎相反，列出可直接对照的维度（本仓口径见
`docs/stats-semantics.md` 与 `CLAUDE.md`）：

| 维度 | Token Monitor | TokenScope |
| --- | --- | --- |
| 解析引擎 | 外部 Rust CLI（tokscale fork）+ Electron 外壳 | 仓内 Rust 适配器（`source → model → aggregate → render`） |
| 覆盖范围 | 36 个客户端（TK 侧支持 56 个 id） | Claude Code、Codex（其余待排期） |
| 计价 | 单一匹配结果 + 编译期硬编码兜底表；无峰谷、无多来源候选（条目内仍有上下文分档） | 三来源独立候选，完整候选优先、同价决胜取**最高费用**；支持分段与峰谷规则 |
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
