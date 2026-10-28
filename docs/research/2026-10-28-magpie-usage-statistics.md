# magpie 用量统计实现拆解（源码级）

> 研究对象：开源项目 [`yetone/magpie`](https://github.com/yetone/magpie)，`main` 分支 HEAD
> `6bec526f7a8b09a33100abe62bb4725167f92393`（提交时间 2026-10-28 16:08 +0800，
> `gui: a key's rename saves once on Enter and never on Escape (#3944)`）。
>
> 研究方式：浅克隆到临时目录后逐文件精读（`read` / `grep`），关键结论一律落 `文件:行号`。
> 全程只读源码，未运行 magpie，未读取本机任何 magpie 数据目录或会话内容。
>
> 证据分档：**已验证**（源码可证且行号已复核）／**一致解释**（多条代码证据指向同一结论）／**未验证**（结构推断，未实测）。
>
> 行号口径：`read` 工具的实际行号。

## 目录

1. 范围与方法
2. 架构立场：magpie 为什么是「网关记账」而不是「日志扫描」
3. 计量上游：一次请求的 token 从哪来
4. 事件模型：`usage.Record` 一行账本
5. 落盘与索引：`usage.jsonl` 与压缩块快照
6. 本地文件读取：`internal/sessions` 全链路（发现→变更判定→解析→分片缓存→网关排他）
7. 三源合并与去重：同一调用不能算两遍
8. 聚合：`Totals` / `Summary` / 时间桶 / 维度
9. 计价：价格从哪来、怎么乘、怎么处理未知
10. 金额展示、预算与跨机共享
11. 对外接口与导出
12. 支持的 Agent：三层清单
13. 与 TokenScope 的对照：可抄的与要避的
14. 复核记录与未决问题
15. 逐文件实现清单（子代理分片复核）
- 附录 A：关键常量速查
- 附录 B：模块与入口索引
- 附录 C：本地读取代码文件索引

## 1. 范围与方法

- 仓库形态（**已验证**）：单 Go module（根 `go.mod`），无 monorepo、无 Rust 侧。业务代码在
  `internal/`（约 90 个包）与 `plugins/`（15 个插件包）；`native/` 是隐私过滤的原生组件，
  `site/`、`docs/` 是站点与文档。桌面壳与前端不在本仓（`plugins/usage/usage.go` 只声明前端
  资源名 `client.js` / `page.js`，文件本身未入库）。
- 规模（read 工具实际行号，仅本文涉及的部分）：`internal/gateway/gateway.go` 7708 行、
  `internal/gateway/claude_subscription.go` 4563 行、`internal/sessions/sessions.go` 2461 行、
  `internal/usage/request_page.go` 2083 行、`internal/provider/models.go` 3173 行、
  `internal/catalog/catalog.go` 1880 行、`internal/sessions/calls.go` 1225 行、
  `internal/usage/usage.go` 1217 行。
- 方法：先读入口与类型层建立主干，再按「抽取 → 落盘 → 索引 → 合并 → 聚合 → 计价 → 输出」
  逐段精读；符号定位用 `grep`，结论落 `文件:行号`。
- 术语：
  **调用（call/request）** = 一次向上游发起、可计量的模型请求；一个 request 可以有多
  try（`Tries`）。
  **记录（Record）** = `usage.jsonl` 的一行，一个可计量单元。
  **来源（source）** = 一条记录的来路，共三种：网关自己转发的、从 agent 会话文件补读的、
  客户端主动上报的（§7）。

## 2. 架构立场：magpie 为什么是「网关记账」而不是「日志扫描」

这是本文最需要先扭正的认知差异（**已验证**）：magpie 不是「扫各工具日志」的统计器，它是
**本地 AI 网关 + 桌面客户端**——把各家 agent 的请求收进自己的 `/v1/...`，再转发给真正的
provider。因此它的用量统计主线是：**凡经过我的请求，我逐条记账**。

三个直接后果：

- 事实源是 `usage.jsonl`（`internal/usage/usage.go:1-5` 的包注释写明：
  「keeps the token count of every call the gateway serves … Records go to one JSON-lines file
  next to providers.json」）。
- 记账**绝不阻断请求**（`usage.go:263-265`：`Append` 的错误一律吞掉——"accounting must never
  break a call"）。
- 为了覆盖「没走网关的调用」，才额外去读 agent 自己的会话文件（§6），并明确区分两种来源，
  绝不混算（§7）。

模块分工（**已验证**）：

| 层 | 包 | 职责 |
| --- | --- | --- |
| 转发与计量 | `internal/gateway` | 协议转换、路由、把上游回复里的 usage 抽出来（§3） |
| 账本与聚合 | `internal/usage` | 记录模型、落盘、索引、合并（`ledger.go`）、直连来源单独汇总（`direct.go`）、汇总、计价调用（§4/§5/§7/§8/§9） |
| 本地会话 | `internal/sessions` | 各 agent 会话文件的发现与解析，产出 `sessions.Call`（§6） |
| 价格目录 | `internal/catalog` | models.dev / labs / OpenRouter / Gemini 价目与模型能力（§9） |
| provider 与有效价 | `internal/provider` | provider/账号/密钥/分组、有效价解析、重命名与别名（§9） |
| HTTP 输出 | `internal/usageapi` | `/api/usage` 与 `/v1/magpie/usage` 的同一份 JSON（§11） |
| 导出 | `internal/otlp` | OTLP 导出（§11） |
| 预算 | `internal/budget` | 网关密钥按窗口限额（§10） |
| 跨机 | `internal/davsync` + `usage/shared.go` | 同步各机器的日用量（§10） |

`internal/usage` 是**唯一**同时认识「网关记录」和「会话文件调用」的层，合并逻辑（§7）就落在
这里；上层（`usageapi`、TUI、CSV）只消费合并后的结果。

## 3. 计量上游：一次请求的 token 从哪来

### 3.1 抽取器：一边转发一边嗅

`internal/gateway/sniff.go` 是核心（**已验证**）：`usageSniffer`（`sniff.go:11-35`）在把上游
回复原样抄给客户端的同时，旁路解析 usage。

- 非 SSE（整包 JSON）：缓冲全部字节，上限 `8<<20`，超限置 `over` 放弃解析（`sniff.go:38-45`）。
- SSE：按行切，`event:` 行识别终止事件（`message_stop` / `response.completed` /
  `response.incomplete` / `response.failed` / `error`，`sniff.go:245-252`），`data:` 行累积；
  单个事件超过 `1<<20` 直接丢弃（`sniff.go:73-88`），单行超过 `1<<20` 也丢弃（`sniff.go:55-58`）。
- 正文结束调用 `usage()`（`sniff.go:286-295`），把最后看到的模型名写进 `Usage.Served`。

### 3.2 四种协议形状与它们的语义差异

`parse()`（`sniff.go:98-236`）按 `provider.Protocol` 分四路，各自把厂商字段映射到统一的
`gateway.Usage`（`internal/gateway/ir.go:373-423`）：

| 形状 | 位置 | prompt 的语义 | 缓存读 | 缓存写 | 推理 token |
| --- | --- | --- | --- | --- | --- |
| Gemini `usageMetadata` | `sniff.go:139-164` | `promptTokenCount` **含**缓存 | `cachedContentTokenCount` | — | `thoughtsTokenCount` 同时计入输出与推理 |
| Chat Completions | `sniff.go:165-181` + `chat.go:987-1051` | `prompt_tokens` **含**缓存（OpenAI 约定），但要判断是否真"含" | 三级回退 | `prompt_tokens_details.cache_write_tokens`（OpenRouter） | `completion_tokens_details.reasoning_tokens` |
| Responses | `sniff.go:182-207` + `responses.go:1381-1401` | `input_tokens` **含**缓存与写入 | `input_tokens_details.cached_tokens` | `input_tokens_details.cache_write_tokens` | `output_tokens_details.reasoning_tokens` |
| Messages（Anthropic） | `sniff.go:208-235` + `anthropic.go:1357-1387` | `input_tokens` **不含**缓存 | `cache_read_input_tokens` | `cache_creation_input_tokens` + `cache_creation.ephemeral_{5m,1h}` | 无独立字段 |

内部统一口径（**已验证**）：`Usage.Input` 永远是「**未命中缓存的输入**」，
`Usage.prompt()` = `Input + CacheRead + CacheWrite`（`ir.go:447-452`）。这与 TokenScope 的
四桶语义一致，但 magpie 是在**协议转换层**做归一的，不是在读取时。

Chat 语义判定是这里最容易写错的一处，值得单独记（`chat.go:1019-1051`）：

```
out.CacheRead  ← prompt_tokens_details.cached_tokens
               ← 若仍为 0，回退 prompt_cache_hit_tokens（DeepSeek）
               ← 再回退顶层 cached_tokens（Moonshot）
out.CacheWrite ← prompt_tokens_details.cache_write_tokens
若两者皆 0，改用 Anthropic 字段名 cache_read_input_tokens / cache_creation_input_tokens，
   并判断 whole = prompt_tokens >= cacheRead+cacheWrite：
       whole  → Input = prompt - cacheRead - cacheWrite（OpenAI 语义，prompt 含缓存）
       !whole → Input = prompt（Anthropic 语义，prompt 不含缓存）
```

即：**同一份 `prompt_tokens` 字段，在不同上游的语义不同，magpie 用「是否够减」来猜**，并把
这个猜测标在 `whole` 上。这是典型的不完美启发式，也是「用户账单对不上」的常见来源。

### 3.3 `add()` 的合并语义：后来的非零值覆盖

`Usage.add()`（`ir.go:454-502`）**不是求和**，而是「新值非零就覆盖」——因为流式回复会在多个
事件里重复上报**同一请求的累计值**（`message_start` 报初始、`message_delta` 报最终）。同一
字段若这次为 0 而上次非零，保留上次。这个设计对「累计型字段」正确，对「增量型字段」会错，
所以 magpie 在会话文件侧（§6）另做了一套差分逻辑。

### 3.4 四个容易被忽略的字段

- `Deferred`（`ir.go:415-418`、`chat.go:1046-1049`）：插件把**更早请求迟到上报的用量**塞进
  当前响应（`magpie_deferred`）。处理原则是「计费时并入（`Usage.billed()`，`ir.go:434-445`），
  但不计入 prompt」——因为客户端读 `prompt_tokens` 判断上下文占用。这是很实用的一个区分：
  **计费口径 ≠ 上下文口径**。
- `CacheUnknown`（`ir.go:390-392`、`aUsage.CacheUnknown`）：厂商收缓存费但只报积分（Kiro），
  此时 `CacheRead = 0` 不代表未命中。命中率计算会把这些 prompt 排除（§8.4）。
- `Credits`（`ir.go:419-422`）：WorkBuddy 的 `usage.credit`，即「套餐积分」，与 USD 并行存在，
  **不做汇率换算**，两者都原样记录。
- `Tier{Asked,Sent,Served}`：服务档位（`priority` / `flex` / `ultrafast`）。`Served` 只取
  厂商回复自报值，绝不由请求值推断（`usage.go:90-101` 的注释）。

顺带一个工程细节：除了 `usageSniffer`，还有 `chattidy.openAIUsage()`
（`internal/gateway/chattidy.go:169-216`），把 Anthropic/DeepSeek/Moonshot 的缓存字段**回填**
成 OpenAI 名字再发给客户端，理由是下游客户端（pi-ai、AI SDK）只认 `prompt_tokens_details.cached_tokens`。
这解释了为什么「同一请求在不同工具里显示的缓存命中率不同」——是网关做了补偿。

### 3.5 落盘点

一次请求结束时 `gateway.go:3878-3895` 组装 `usage.Record` 并调用 `appendUsage`
（`internal/gateway/lan.go:476`）。注意三处细节（**已验证**）：

- 计数用 `call.Usage.billed()`（含 Deferred），行里同时记 `Served`（回复自报模型）、
  `Requested`（客户端要的模型）、`Upstream`（聚合商背后的真实上游）。
- `Tries > 1` 才写 `Tries`，即「首次失败后重试成功」的记录仍算**一个 request**。
- 本地拒绝（未联系上游）走 `Rejected: true`，不参与总量（§8.3）。

## 4. 事件模型：`usage.Record` 一行账本

`Record` 定义在 `internal/usage/usage.go:29-232`，共 50 余字段。按语义分组（**已验证**）：

| 组 | 字段 | 说明 |
| --- | --- | --- |
| 时间与路由 | `Time`、`RouteID`、`Operation` | `RouteID` 把一个 request 的多次 try 串起来 |
| agent 身份 | `Agent`、`Session`、`NativeSession`、`Subagent`、`ParentAgent`、`Kind` | `Session` 是 agent 自己发的会话头；`Subagent` 是子代理 id |
| provider 身份 | `Provider`、`Host`、`ProviderKeyID/Name`、`ProviderAccount`、`CallerKeyID/Name` | `Host` 记录"请求实际去哪了"，含 `host as account` 形式 |
| 会话历史身份 | `SessionProvider`、`SessionAccount`、`SessionOfficialLogin` | 会话文件里的历史声明，**不是**本次计费证据（`usage.go:46-53`） |
| 模型四元组 | `Requested`、`Model`、`Served`、`Upstream` | 见下 |
| 令牌 | `Input`、`Output`、`CacheRead`、`CacheWrite`、`CacheWrite1h`、`CacheUnknown`、`Reasoning` | 语义同 §3.2 |
| 时序 | `Millis`、`TTFT`、`FirstText`、`FirstReasoning`、`Flow`、`Sent`、`Own`、`Masking`、`Retried`、`Queued` | 后四个用于把耗时归因到 magpie 自己 |
| 结果 | `Status`、`Error`、`ErrType`、`RequestID`、`ResponseID`、`Stop`、`Rejected`、`Tries`、`HandedOn` | `HandedOn` = 这次 try 失败但把请求转交给下一次，**不算 request** |
| 档位与积分 | `TierAsked`、`TierSent`、`TierServed`、`Credits`、`OwnCost` | `OwnCost` 是上报调用自报的美元成本 |
| 来源与归属 | `Source`、`Computer`、`Via`、`Archive` | 见 §7 |

模型四元组是这份 schema 最有价值的部分（`usage.go:54-63`）：

- `Requested`：客户端要的（可能是别名、分组 `group/xxx`、`provider/model`）。
- `Model`：provider 侧真实模型 id，**计价用它**。
- `Served`：厂商回复自报的模型（可能是另一个模型，含"偷换"与"改名"两种，见 §9.6）。
- `Upstream`：聚合商（OpenRouter）自报的背后真实上游（DeepInfra、Novita）。

不持久化字段用 `json:"-"` 标出：`Watched`、`Local`、`BodyIn`、`BodyOut`，以及小写的
`pricedAs`（`usage.go:218-231`）。`pricedAs` 解决「provider 删了又合并」时的计价身份：
记录展示在合并后的 provider 下，但**按原 provider 的价计价**（`usage.go:234-252`）。

有两个布尔式语义陷阱，实现时值得抄：

- `IsRejected()` = `Rejected || (Provider == "" && Status >= 400)`（`usage.go:342-343`），
  兼容了「老记录没有显式 Rejected 标记」。
- `IsReported()` = `Source == SourceReported`（`usage.go:351`），并且代码在多处显式声明：
  上报的调用**只进 Usage 页面**，不参与路由、密钥预算、窗口预留（`usage.go:345-351`）。

## 5. 落盘与索引：`usage.jsonl` 与压缩块快照

### 5.1 写入路径

- 文件位置：`Path()` = provider 配置文件同目录的 `usage.jsonl`（`usage.go:254-255`），
  即 XDG 感知的 magpie 配置目录。
- 写入：全局 `sync.Mutex` + 追加写（`usage.go:277-296`）。为了省掉每请求的 open/close，
  非 Windows 平台**长期持有句柄**（`holdLog`，`usage.go:298-309`），并在每次写前用
  `sameLogFile(before, now)` 校验「路径还指向同一个文件」，被移走/替换则重开
  （`usage.go:313-340`）。Windows 上不持有，因为「打开着就无法删除或改名」（注释原话）。
- 每次成功追加后 `noteLogAppend(path, before, after)`（`usage.go:359-369`）把文件状态记到
  `logAppends`，供索引判断「这段前缀是不是我自己顺序写的」。

### 5.2 索引：块压缩 + 增量指纹

`internal/usage/log_index.go` 是性能核心（**已验证**）：

- 立场写在文件头：JSONL 是账本本身，索引是**可丢弃**的；「appending copies at most the last
  block」，读者只解与时间窗重叠的块（`log_index.go:1-6`）。
- 结构：`logSnapshot`（`log_index.go:36-61`）持有 `blocks []*rowChunk`、`off`（已解析字节）、
  `hash`/`digest`（已解析前缀的 SHA-256 及其可续算状态）、`first`/`firstOwn`、
  `keyProviders`、`probes`、`hosts`。
- 分块：每块 512 行（`logBlockRows = 512`，`log_index.go:23`），列式编码 + 字符串字典 +
  「shape」复用（`request_page.go:67-215`），压缩后约 **80 字节/调用**（`log_index.go:31-34`）。
- 增量：`readLogSnapshot()`（`log_index.go:98-230`）按三条证据判断能否续算——
  ①文件 inode/size/mtime 未变 → 直接复用；②`logAppends` 记录的「我的上次写入」与当前状态衔接
  （`trusted`）→ 可信续算；③否则重算前缀哈希比对 `old.hash`。
- 时间戳可信度：`settledLogStamp()`（`log_index.go:336-353`）区分「整秒时间戳」（HFS+/FAT，
  需 1 秒沉降）与「毫秒级时间戳」（APFS/NTFS，只需 100 ms），避免「同一时钟刻内写入后读」的
  误判。注释里留了血泪教训：每请求重算全量 SHA-256 曾让 100 MB 历史每次请求多花 180 MB 哈希。
- 访问：`visit(since, fn)`（`log_index.go:263-279`）跳过整块——判据是块内**最晚**时间早于
  `since`。`handedOn()`（`log_index.go:232-261`）在索引期就地补 `HandedOn`/`Tries`，
  兼容老记录。

**反面教训（值得写进任何自研实现的设计约束）**：注释明确记了「曾经按大小预算丢块」，结果是
每次读都重新解析整文件（750k 调用 ≈ 6 秒 CPU），所以现在**块永不淘汰**（`log_index.go:31-34`）。

## 6. 本地文件读取：`internal/sessions` 全链路

会话文件在三个地方被消费，必须分开看（**已验证**）：

- `ledgerMatched()` 内部合并出全量 ledger（§7.2）——用于 Requests 页与 CSV。
- `usage.Direct()`（`internal/usage/direct.go:11-33`）**只汇总直连调用**，用于 Usage 页与托盘
  「和网关的并列计数」。它的实现刻意绕开「把每条网关记录都变成带价的行」：只把 ledger 索引的
  块解出来参与匹配，再走同一个 `ledgerMatched`（`direct.go:41-83`）。注释记了动机：750k 调用的
  历史里，「这笔开销占 TUI 与 `magpie usage` 等待时间的大半」。
- `usage.Summaries()`（`direct.go:23-33`）一次取「网关摘要 + 直连摘要」，理由是「两次调用之间
  可能跨过午夜，把一天的走网关调用与另一天的直连调用摆在一起」。

### 6.1 统一调用模型

`sessions.Call`（`internal/sessions/calls.go:22-64`）是会话文件侧的「事件」：

```go
type Call struct {
    ResponseID string    // Codex 的 response_id，用于与网关记录配对
    Time       time.Time
    Agent      string    // claude / claude-desktop / codex / ...
    Session    string    // 会话 id：Claude 取 sessionId 字段、Codex 取 rollout 文件名、Kimi 取目录名……
    ThreadID   string    // Codex 线程
    Model      string    // 文件所记模型
    Requested  string    // Claude Code 的 identity 块声明的运行模型
    Tokens               // 内嵌 sessions.Tokens
    Reasoning  int
    Effort     string
    Upstream, AccountID, UserID string   // Codex 的会话元数据（历史身份，非计费证据）
    ...
}
```

`Tokens`（`internal/sessions/sessions.go:45-54`）与网关侧同构：`Input` 不含缓存，
`CacheRead`/`CacheWrite`/`CacheWrite1h` 分列。`ccTokens()`（`sessions.go:79-87`）把 Claude Code 的
`cache_creation.ephemeral_5m/1h` 拆出来，并取 `CacheWrite = max(总写, 5m+1h)` 防厂商字段不一致。

### 6.2 读者注册表与发现层

`internal/sessions/call_cache.go:205-251` 是**唯一权威**的「能读哪些 agent」表。代码注释写死：
「Every agent in `callSources` that is not in `callFiles` needs an entry here, in
`lineCallReaders` or in `resumeCallReaders`, or its calls are never read.」

**注意：这是 calls 侧的表，与汇总侧的 `parserFor`（`sessions.go:1614-1645`）是两套独立分发**——
后者决定 Sessions 页怎么汇总，前者决定 Usage 页能看到谁。两者的 agent 覆盖范围不同：
`parserFor` 的 `default` 分支落在 `claudeLine`，所以没有独立 reader 的 agent 也会被按 Claude 格式
汇总（但不进 Usage）；完整对照见 §15.9。

| 读取模式 | agent | 代表文件/格式 |
| --- | --- | --- |
| `callFiles`（行式，可续读） | `claude`、`claude-desktop` | `~/.claude/projects/**/*.jsonl`；Desktop/Cowork 各自 `.claude` 目录 |
| 同上 | `codex` | Codex rollout `*.jsonl`（含 `session_meta` / `turn_context` / `token_count` / `token_usage_record` / `compacted`） |
| `wholeCallReaders`（整份重读） | `opencode` | SQLite（`session_v2`/`session_message`）或 `storage/` 下 JSON |
| 同上 | `zcode` | 自有 JSON/数据库 |
| 同上 | `workbuddy`、`codebuddy` | `projects/<cwd>/<id>.jsonl` 的 `providerData.usage` |
| 同上 | `minimax-code` | 消息文件（后续世代可能替换） |
| 同上 | `agy` | `~/.gemini/antigravity-cli/brain/<conv>/.system_generated/logs/transcript_full.jsonl` |
| `resumeCallReaders`（可续读的打包格式） | `dsh` | zstd 帧打包的追加文件 |
| `lineCallReaders`（行式，非 callFiles） | `kimi`、`pi`、`omp` | Kimi `agents/<agent>/wire.jsonl` 的 `usage.record`；Pi/omp 行式 |

另有 `wslFiles("claude","codex")`、`wslFiles("kimi")`（`calls.go:303`、`calls.go:343`）把 WSL 发行版
里的同名 agent 也纳入——通过 `\\wsl.localhost` 读，且「停机的发行版不读，以免把它启动」
（`call_cache.go:281-287`）。

结论（**已验证**）：magpie 的「本地会话统计」覆盖 **13 个 agent**：`claude`、`claude-desktop`、
`codex`、`opencode`、`zcode`、`workbuddy`、`codebuddy`、`minimax-code`、`agy`、`dsh`、`kimi`、
`pi`、`omp`。

#### 发现层：先找到「什么文件」

统一状态是 `file`（`sessions.go:380-392`）：`agent`、`key`（`agent:session id`，一个会话可跨多个
文件）、`path`、`size`、`mod`、`main`（是否会话自己的文件，区别于子代理文件）、`readOnly`、
`cold`（WSL 未运行、当前不该打开）、`more *fileMore`（只有部分 agent 用到的附加信息，如
OpenCode 的 store、agy 的 summary、WSL 发行版名）。`fileMore` 单独用指针（`:394-414`），
注释给了理由：一次轮询要复制 ~20k 个 file 五到六次，「96 字节而不是 232 字节」。

各家的发现方式（**已验证**）：

| agent | 发现逻辑 | 位置 |
| --- | --- | --- |
| `claude` / `claude-desktop` | `<dir>/projects/<project>/<id>.jsonl`（`main: true`）+ `<id>/subagents/*.jsonl` + `<id>/subagents/workflows/<run>/agent-*.jsonl`（后两者的 key 都归到会话 id）；`journal.jsonl` 不是 transcript，不读 | `sessions.go:564-612` |
| `codex` | 遍历 `CodexDirs()`（多个 `CODEX_HOME`，一个 ChatGPT 账号一个目录）下的 `sessions/` 与 `archived_sessions/`，用 `rolloutName` 正则取 id；同目录存在未压缩孪生时**优先明文**（压缩可能还没写 trailer）；最后 `distinctCodexFiles` 按流式内容比对去掉重复副本 | `sessions.go:700-826`、`codex_discovery.go:101-130` |
| `opencode` | 有 DB 就按 DB（`session_v2`/`session_message` 或老的 `session`/`message`），否则 `storage/` 下的 JSON；`file.path` 用 `<dbpath>#<sid>` 或 `<…>/<sid>.json` 编码 | `calls_opencode.go:41-63`、`151-202` |
| `zcode` | 同一套表结构，`zcodeDB()` + `<dbpath>#<sid>` | `calls_zcode.go:39-107` |
| `workbuddy` / `codebuddy` | `walkLinked(projects/)` 下所有 `.jsonl`；父目录是 `subagents` 时 key 归到会话 id 且 `main: false`；`$CODEBUDDY_CONFIG_DIR` 指向 WorkBuddy 目录时直接返回 nil（同一批文件不读两遍，`#2932`） | `workbuddy.go:41-93`、`calls_workbuddy.go:31-48` |
| `minimax-code` | `v2/sessions/<yyyy>/<mm>/<dd>/<ts>-session_<base64 id>/messages*.jsonl`，id 从目录名 base64 解出 | `calls_minimax.go:44-85` |
| `kimi` | `sessions/<workdir slug>/<sid>/agents/<agent>/wire.jsonl`（每个 agent 一个文件） | `calls_kimi.go:61-69` |
| `pi` / `omp` | `piFiles()` / `ompFiles()`，另加 `wslFiles("pi","omp")` | `calls_pi.go:42-46` |
| `dsh` | `dshFiles()`（打包帧文件） | `calls_dsh.go:27-35` |
| `agy` | `~/.gemini/antigravity-cli/brain/<conv>/.system_generated/logs/transcript_full.jsonl` | `calls_agy.go:39-46` |
| WSL 内的 claude/codex/kimi/pi/omp | `wslFiles(...)` 经 `\\wsl.localhost` 读，停机的发行版标 `cold` | `wsl.go:135` |

四个「发现层」的性能机制值得单列（**已验证**）：

- **一次列目录代替逐文件 stat**：`statFiles()`（`sessions.go:482-525`）先用 `dirent.Stats` 把同一
  目录一次性读出（`fs.ReadDir` 语义的批量封装），拿不到的再用 8 个协程 `Stat`/`Lstat` 补齐；
  注释记了「~21k 文件逐个 stat 是那次遍历的大头」（`#3493`）。
- **目录列举缓存**：`readDirectory()`（`discovery.go:33-56`）按「目录 mtime + size」缓存条目，
  并规定「列举时间距目录 mtime 不足 `racyDirectory = 2s` 的不可信，要重列」——因为 mtime 精度
  粗，同一刻新增的条目会让 mtime 不变而漏掉。
- **预热期列举复用**：`codexFiles()`（`sessions.go:700-717`）在 `quiet()`（只有预热在读）时把
  rollout 列表缓存 `warmListingFor = 10s`；`OneListing()`/`heldFiles()`（`one_listing.go:29-96`）
  则在一次页面请求内共享各目录列举，并让并发读者等同一份结果而不是各自重列。
- **Codex 副本去重**：`distinctCodexFiles()`（`codex_discovery.go:101-130`）对同名 rollout 两两做
  **常数内存的流式比对**（32 KiB 分块，`readCodexCopies`，`:173-209`），判定 `equal` /
  `firstPrefix` / `secondPrefix` 后只留最长或唯一的那份。缓存条件是「change time 早于本次比对
  1 秒以上」（`codexStampSettle`，`:44`）+ size/mtime/ctime/`os.SameFile` 全一致，
  且比对前后各 stat 一次确认文件没被改（`:151-156`）。注释特别强调：
  「只有 proven decoded prefix 才算冗余副本，共享 thread/name 不能证明归属」。

「一个会话多个文件」由 `key` 表达，读取时按 `mod` 升序排列文件（`calls.go:241-246`），
注释说明「更早的文件拥有被复制进续接会话的消息」——这是 Claude Code 会话续接（resume）留下的
重复消息靠 `Msg` 去重（`calls.go:266-271`）的前提。

### 6.3 各 agent 的解析口径（只记口径差异，不记内容）

**先看哪些行值得解**——这是全部 reader 的共同结构，也是「本地读取」性能的真正来源（**已验证**）：

- `claudeLine()`（`claude.go:68-115`）按行头 1024 字节分四种走向：Claude Code 自己的行
  （ai-title / custom-title / summary → 整读）；`attachment` 在 `message` 之前 → 只取时间戳；
  无 `message` → 整读；有 `message` 时看它的前两个字段——`model` 开头且 `role:assistant` → `ccReply`，
  `role` 开头且 `role:user` → 若不是 tool_result 就整读（提示词），否则只取时间戳。
- `ccReply()`（`claude.go:279-311`）只解 outer 身份（`gjson` 取 `requestId`，避免工具输入里
  的同名字段冒用），用 `bytes.LastIndex(b, ccUsage)` 定位 usage，再用手写括号匹配
  `objectEnd()`（`claude.go:250-275`）切出 usage 对象。注释给了动机：`json.Decoder` 会把整行复制
  一遍，「冷启动头 2 分钟 11.6 GB 分配里有 1.6 GB 是这个」。
- `codexHead()`（`codex.go:124-175`）按 `type` 与 `payload.type` 判断：`session_meta` 只在
  `s.ID == ""` 时值得读；`turn_context`/`token_usage_record`/`compacted` 必读；
  `event_msg` 只读 `token_count`/`task_started`/`thread_settings_applied`/`user_message`（且后者
  只在还没有标题时）；工具调用直接记 tool 名不读全文。两处会清 `Codex.Pending = 0`
  （`:132-140`）：`response_item + message + role:user` 与 `event_msg + user_message`——这正是
  防止「下一次累计值被配到上一次调用」的机制。
- `workbuddyLine()`（`workbuddy.go:154-162`）与 `piParse()`（`pi.go:158-171`）都是同一套
  「先 `bytes.Contains` 关键字再决定要不要 `json.Unmarshal`」的预筛；`piParse` 还多一层
  `copied` 判断（时间早于 fork 起点 → 不算）。

**Claude Code / Claude Desktop**（`calls.go:632-835` + `claude_usage.go`）：

- 一个 `message.id` 可能有多行（流式块各占一行、工具块重复上报同一 usage），
  `claudeUsageState.message(id, request)`（`claude_usage.go:123-158`）按
  `message:<id>` + `request` **分支出贡献**；只有 request id 兼容才合并，缺 id 时仅当
  「唯一已知分支」才收养——注释写明「neither ID → no evidence that two lines are the same call」。
- `accept()`（`claude_usage.go:202-228`）只接受「未见过且更晚」的修订：同 model + 同四桶 → 视为重复
  （仍推进 `Updated` 作为修订序）；命中 `Previous` → 忽略；时间不更晚 → 忽略。所以「回放的旧快照
  不能撤销后来的块」。
- `attachment.identity.modelId` 提供 `Requested`（如 `claude-opus-5[1m]`）；`compact_boundary` 的
  `durationMs` 记到下一次成功调用上；`isApiErrorMessage` 行算错误、token 为 0。
- **修订窗口过期后仍能续读**（`calls.go:709-723`）：当 `m.Call < 0`（该 message 已被窗口淘汰）
  但 `st.Msgs` 里还留着它的行号时，代码用**旧调用**重建 `m.Call`/`m.Usage`/`m.Began`，
  前提是 request id 兼容（`m.Request == ""` 或与旧调用的 `RequestID` 相等）。没有这一段，
  一个长会话读超过窗口就必然把旧消息当成新消息再记一次。原文注释：
  「Materialized rows already carry the last known usage beyond the recent replay window.
  Reuse the original row lookup when that history expires.」
- **块的两种身份**（`calls.go:725-742`）：有 `uuid` 时 `blockID = "uuid:" + uuid`；
  老写入没有 uuid，则退回 `"content:" + sha256(message.content)`。`newBlock`（第一次见到该块）
  用来判定「usage 没变但来了新块」时是否推进该调用的时间与 `To`，同时**不跨日移动用量**
  （`calls.go:801-806`：要求 `dateOf(at) == dateOf(previous.Time)`）。
- 耗时用「上一次 user 行（或上次回复结束）→ 本次回复行」近似（`calls.go:743-750`），
  注释承认会「多一点或少一点」。
- 磁盘上另存**修订窗口**（`claudeRevisionEntries = revisionEntries / revisionFiles = 1024`，
  `claude_usage.go:21`），并用 `MarshalJSON`/`UnmarshalJSON` 写进分片（`:102-111`），
  使「重启后继续读」不必从第一行重放（`#3493`：一个 2 GB 的活跃会话曾每次更新都要整读）。

**Codex**（`calls.go:852-1131` + `codex_usage.go` + `codex.go:69-93`）——这是全仓最复杂的差分：

- `cxUsage` 用 `Known uint8` 位图区分「字段缺失」与「显式 0」：`UnmarshalJSON`
  （`codex_usage.go:230-257`）用 `*int` 逐个字段解码，缺哪个就不置位。`matches()`（`:283-295`）
  要求 input/output 两位都必须 known、且所有共同 known 字段相等。
- `spent()`（`codex.go:87-93`）：Codex 的 `input_tokens` 是**整个 prompt**（含缓存读写），
  所以 `Input -= CacheRead + CacheWrite`，负值夹 0。
- 两条来源：`token_usage_record`（带 `response_id`，走 `record()`，`codex_usage.go:388-495`）与
  `token_count`（带累计 `total_token_usage` + 本次 `last_token_usage`，走 `count()`，`:497-646`）。
- `record()`：入口先把上一次的 `Pending` 取出并清零（`codex_usage.go:389-390`），再在覆盖前算好
  `ExpectedCount`/`Epoch`（`:405`）；去重键 `session\x00response_id`；命中则比较 usage/model，
  冲突计 `Conflicts++` 并丢弃；重放（时间不更晚）不撤销已接受更新；无 id 时用 `Intervals` 索引按
  `(session, epoch, total input/output, usage input/output)` 找唯一同区间，再退到 `Pending`
  做「一次观测只消费一次」的配对。
- `count()`：优先用 `last`（本次请求的精确用量），缺 `last` 才用 `total - prev`；
  `Epoch` 表示「计数器域」（runtime 重启、周目切换），`CounterBases` 记录重置前的基线，
  `High` 是单调高水位。注释点名两类坑：`#2234`（Codex Desktop 的 `thread_spawn` 会重放父线程
  checkpoint、`last=0`，若照算差分会把父线程的量再记一遍）与「累计值回退但时间戳前跳」。
- `compacted()`（`codex_usage.go:116-156`）：压缩调用只在 `compaction_response_id` 与
  `latest_token_usage_record.response_id` 一致、usage 有效时计入；它同时参与
  `projectCount()` 的偏移计算，避免压缩量与后续累计重复。
- `task_complete` 给出整轮 `duration_ms` / `time_to_first_token_ms`，**仅当该轮只有一次调用**时
  回填到那次调用（`calls.go:980-991`）。

**OpenCode / ZCode**（`calls_opencode.go:151-250`、`calls_zcode.go:65-107`）：一条 assistant 消息
= 一次调用。但两家 tokens 语义不同，代码显式分流（`calls_opencode.go:229-240`）：

- OpenCode 自己的写法按「Codex 式」读：`Output += Reasoning`，input 原样（不含缓存）。
- **ZCode 用的是 AI SDK 的计数**：input 含缓存、output 已含 reasoning；照搬会「缓存算两次、
  命中率读成一半」（`#1947`，注释给了 50.8% vs 96.9% 的实测），所以单独做
  `Input = input - cacheRead - cacheWrite`、`Output = output`。
- 二者的「走网关」判定：`ocGatewayProvider(id)` = `magpie` 或旧名 `dial`
  （`calls_opencode.go:29-30`），直接跳过、不留待配对。

**WorkBuddy / CodeBuddy**（`calls_workbuddy.go:155-241` + `workbuddy.go:154-209`）：一行
`providerData.usage`，以 `providerData.messageId` 标识；**同一 messageId 会随回复进行重复出现**，
所以「最后一行覆盖前一行」，但仅当 id 相同且非空（`calls_workbuddy.go:186-196`）——无 id 的旧历史
回灌行各自算一次调用。三处口径细节：

- 字段有两代名字：新 `inputTokens`/`outputTokens`/`inputTokensDetails`/`cacheCreationTokens`，
  旧 `input_tokens`/`output_tokens`/`input_tokens_details`/`cache_creation_input_tokens`，
  用 `cmp.Or` 逐个回退（`calls_workbuddy.go:223-225`、`workbuddy.go:196-198`）；
  `inputTokensDetails` 可能是**列表也可能单对象**，`wbDetails.UnmarshalJSON`（`workbuddy.go:99-110`）
  两种都收，再 `sum("cached_tokens")`。
- **input 已含缓存读与写**，所以 `Input = max(0, in - read - write)`（`calls_workbuddy.go:232`、
  `workbuddy.go:199`）。
- 汇总侧的「覆盖」是**撤销再重计**：`s.unuse(s.MsgDay, s.MsgModel, s.MsgUse)` 后重新
  `s.use(date, model, t)`（`workbuddy.go:202-208`），与 calls 侧的「数组内替换」同源同口径。
- 排他用的模型 id 是 `requestModelId`（`calls_workbuddy.go:212`），不是 `model`；
  另外 `codebuddyInWorkBuddy()`（`workbuddy.go:53-57`）在 `$CODEBUDDY_CONFIG_DIR` 指向 WorkBuddy
  目录时直接返回 nil，避免同一批文件被读两遍（`#2932`）。

**MiniMax Code**（`calls_minimax.go:100-156`）：`messages.jsonl`（以及 `history-catalog.json`
保留的其它世代 `messages*.jsonl`）里的 assistant 行，`message_id` 作 `Msg` 去重；
provider 为 `custom_provider:magpie` 的调用跳过。

**Kimi Code**（`calls_kimi.go:111-145`）：`usage.record` 一行一次；`Input = inputOther`，
缓存读/写分列（`inputCacheRead` / `inputCacheCreation`），output 含 thinking；
`model` 前缀 `magpie/` 的跳过（`:64`）。fork 会把父会话记录**逐字节复制**，因此 `Msg` 用
`kimi:<agent>:<time>:<model>:<四桶>` 组合键（`:141-145`），跨文件去重。

**Pi / omp**（`calls_pi.go:96-185` + `pi.go:100-135`）：行式读取，`piCarry` 保存「会话 cwd /
fork 起点 / 当前 provider+model」，`model_change` 与 assistant `message` 都会推进它；`usage`、
`model_usage`、`compaction`、`branch_summary` 四类行都算调用（压缩与分支摘要的用量记在当时的
模型上）；provider 为 `magpie`/`dial` 跳过。两个容易踩的点：

- **`piUsage` 只有四桶**（`pi.go:100-105`：`input`/`output`/`cacheRead`/`cacheWrite`），
  **没有 reasoning 字段**——注释说明 output 里已含 thinking，所以不要再单独拆一次。
- fork 的判定靠 header 的 `parentSession` + `since`（`calls_pi.go:107-113`、`:127`），
  汇总侧则是 `copied := at.Before(s.Since)`（`pi.go:162-168`）：「写在 header 之前的行是父会话的」。
  `Msg` 用 `agent:id:timestamp` 组合（`calls_pi.go:182`），因此同一行不会被两个 reader 各记一次。

**dsh（DeepSeek Harness）**（`calls_dsh.go:37-219`）：`assistant/message` 行带 usage；
文件是「zstd 帧打包的追加文件」，从上次的 **帧内偏移** 续读（§6.4）。两个额外的坑：
① `session/end-seed` 事件在 `Inherited && version >= 2 && IsSeeded` 三者同时成立时，
意味着「此前的行是从别处 seed 来的」，代码把 `st.Calls` 清空并 `kept = 0`（`:82-91`）；
② format 2 之前没有这个标记，靠 `seq < SeedLength` 过滤 seed（`:94-96`）。
走网关的判定是 provider 名（`magpie`/`magpie-responses`/`magpie-anthropic`/`dial`，`:209-219`），
注释自承「只看名字：文件里没有 baseURL，用户自建的同名 provider 会被误当作网关」。

**agy**（`calls_agy.go:16-46`）：transcript 的 `PLANNER_RESPONSE` 步骤带
`input_tokens`（已扣除缓存读）/ `cache_read_tokens` / `output_tokens`（含 thoughts）；
无模型、无账号，故「模型未知」；`created_at` 是步骤**开始**时间（不是结束）。
走网关的步骤无法在文件层排除，只能靠 §7.3 的 token+时间配对。

### 6.4 变更判定：三层证据，任一不成立就整份重读

`readCalls()`（`call_cache.go:261-374`）在读之前先做**零 IO 判定**：

1. 内存里有该文件、且 `Size == f.size && Mod == f.mod.UnixNano()` → 直接复用（`:273-280`，
   还要求 `callGeneration` 未变，否则视为整体失效）。
2. 否则 `loadKept(f)` 读磁盘分片，再走 `prepareCalls()`（`calls.go:374-409`）判断能否续读：
   要求「不是打包文件、文件只增长（`f.size >= old.Size`）、`old.Off <= f.size`、头部 256 字节
   哈希一致、已读前缀的采样哈希一致」。
3. 三者任一不成立 → 丢弃旧快照，从第一行整读。

采样哈希 `prefixHash()`（`calls.go:1138-1172`）是这套机制的成本核心：前缀 ≤ 64 KiB 时全读；
更长时取 **16 个 4 KiB 窗口** 均匀采样并打上 `sample-v1:` 版本前缀。注释明说「重写若落在
采样窗口之外会漏检」，是有意取舍（每份指纹最多 64 KiB 读，与文件长度无关）。

打包文件另走 `tail`（`tail.go:16-50`）：`Off`（帧起点）+ `Skip`（帧内偏移）+ `Size`（该帧结束处）
+ `Head`/`Hash`；`holds()` 要求文件未缩短、头部一致、`prefixHash(path, Size)` 一致。

冷源（`f.cold`、`wslCold(path)`）**不打开文件**：已读过的返回旧快照，否则返回空 callFile
（`call_cache.go:281-287`）。注释写明原因是「打开一个停机的 WSL 发行版会把它启动」。

**`packed()` 一个判定、两条路**（`sessions.go:2045-2047`：后缀是 `.zst` 或 `.zstd`）：

- **dsh 走帧级续读**：`resumeCallReaders["dsh"]` → `readDshCalls` → `scanFrames`
  （`frames.go:186-270`）。它自己按 RFC 8878 读帧头（magic、帧描述符长度、逐块 3 字节块长 +
  last 标志、可选 4 字节校验和），`frameSize`（`:154-184`）只读头就推出整帧长度，一次分配好缓冲。
  续读位置是「帧起点 + 帧内字节偏移」（`tail.Off`/`tail.Skip`）。
- **Codex 的 `.zst` rollout 走流式解压**：`openLines()`（`sessions.go:2050-2064`）返回一个
  `zstdFile`，`Read` 直接转给解码器——**不可 seek**，所以只能从头读。`packed()` 的注释把结论写死了：
  「A packed rollout is written whole, so it is read whole again whenever it changes.」
  这也是 `prepareCalls()` 里第一个条件就是 `!packed(f.path)` 的原因：打包文件**没有**「读一半」的可能。

**行级 helper 速查**（逐行确认过，全部为**已验证**）：

| helper | 位置 | 行为 |
| --- | --- | --- |
| `maxLine = 32 << 20` | `sessions.go:1716` | 超过即判为「不是我们用的行」，跳过 |
| `scanBuffer(f, off)` | `sessions.go:1979-1987` | 缓冲区按「剩余长度」在 16 KiB–1 MiB 之间取 |
| `lineReader(f, size)` | `sessions.go:2008-2031` | 7 个 2 的幂尺寸档，`shelf` 优先、`sync.Pool` 兜底；交还时 `Reset(nil)`，注释警告「行只在下一次 release 前有效」 |
| `tsAt(b, last)` | `sessions.go:2110-2126` | 从 `"timestamp":"` 后取 RFC3339Nano；`last` 决定取首个还是末个；长度 > 40 直接判无效 |
| `numTS(b)` | `grok.go:278-296` | 数字时间戳：`> 1e11` 判为毫秒，否则按秒 |
| `ms(n)` | `opencode.go:133-138` | 毫秒转 `time.Time`，`n <= 0` 返回零值 |
| `dateOf(t)` | `sessions.go:290-295` | **本地时区**的 `YYYY-MM-DD`，零值返回 `""` |
| `use/unuse` | `sessions.go:306-324` | 汇总侧记账/撤销；`unuse` 用 `find` 定位后 `sub`，找不到就跳过 |
| `objectEnd(b)` | `claude.go:250-275` | 手写括号匹配，只认字符串外的括号；动机是 `json.Decoder` 会复制整行 |
| `typeAfter(b, key)` | `codex.go:501` | 从 `"key` 后读到下一个 `"`，用于行头快速分型 |

### 6.5 磁盘分片：`calls-v15` 的自定义帧格式

`sessions.CachePath()` = `<cache>/sessions.json`（会话汇总，`sessions.go:916`，当前
`cacheVersion = 24`，`sessions.go:939`）；请求行则写进**另一个目录**：
`callCacheDir() = <cache>/calls-v15`，每文件一个 `<hashHead(path)>.gob`（`call_cache.go:71-72`）。
`call_cache.go:5` 那句「Sessions summaries never load these files」就是这套分工的声明。

帧格式（`call_cache.go:456-503`、`:658-739`）：

```
[4B 长度 little-endian][4B CRC32 IEEE][gob(callFrame)]
```

- `callFrame{State, Strings, Patches, More, Calls}`：`State` 是**纯标量续读状态**
  （`ContentHash/Size/Mod/Off/Head/HeadSize/Path/Agent/At/End/AsstEnd/Session/Requested/
  LastUser/LastAsst` + dsh 的 carry JSON + Pi 的 carry 扁平字段）。注释特别说明：即使运行期
  索引为 nil，gob 也会把递归类型描述写进每一帧，所以这些类型**必须留在 wire schema 之外**。
- `Patches []callPatch`：每个 patch 是一次调用，13 个文本字段被换成
  `Text [13]uint32` 的**帧内字符串字典下标**，`File` 由 `State.Path` 隐含（`:457-462`、`:533-535`）。
  这样一帧里重复的 `Agent`/`Model`/`Session` 只存一次。
- 每帧最多 `framePatches = 2048` 个 patch（`:482`）：注释记了一次实测——把 5.2 万调用的 Codex
  会话写成**单帧** 8 MB，读取时一次分配 ~100 MB、常驻 ~70 MB（`#3493`）。
- `More` 标记「同一写入的后续帧」，文件以 `More=true` 结尾说明**写到一半被截断**，整片作废
  （`loadCalls` 在 `:638-640` 检查）。
- 写入策略：只有「从已有偏移继续」时才 `O_APPEND`，否则写临时文件再 `rename` 原子替换
  （`:719-738`）；`writeCalls` 只写 `dirty` 的旧调用 + 新调用（`:704-711`），所以一次追加的
  代价与新增量成正比。

读取校验（`loadCalls`，`:561-644`）逐项失败即返回 `nil`（整片按源重建）：
帧长 0 或 > 256 MiB、CRC 不符、`State.Path`/`State.Agent` 与请求不符、`Off` 倒退、patch 下标越界、
字符串下标越界。这是一套「宁可重读源文件，也不用可疑缓存」的策略。

两个实现细节（本轮原文确认）：

- `calls = slices.Grow(calls, max(len(frame.Patches), frame.Calls-len(calls)))`（`:612`）——
  按**帧里声明的 `Calls`** 一次性预留容量，而不是 append 反复翻倍。注释记了代价：
  「a warm start paid ~500 MB for across every cached file」。
- `frame.State.Path != f.path || frame.State.Agent != f.agent` 直接整片作废（`:600`）——
  分片是按路径哈希命名的，这一步是防「路径碰撞/串文件」的最后防线。

**版本史即口径史**（`call_cache.go:23-46`，v3→v15）：v4 Codex 的 input 不再含缓存写、v8 Claude 的
1 小时缓存写分离、v9 dsh 带 provider/应答模型且排除网关调用、v10 ZCode/WorkBuddy 开始读、
v12 ZCode 的 input 去缓存 / output 去重复 reasoning（`#1947`）、v13 压缩耗时、v14 Codex 线程、
v15 压缩标记独立于耗时。版本号是**目录名的一部分**：`pruneCalls()` 一旦发现根目录变了，
就 `Glob(calls-v*)` 把其它版本目录整个删掉（`:142-150`）。换句话说，**改解析口径 = 换缓存目录**，
不做迁移也不做兼容。

### 6.6 两级缓存与配额

- 内存：`callCache map[string]*callFile` + `callOrder` 近似 LRU；`keepCalls()`（`call_cache.go:177-203`）
  在插入前按 `maxKeptCalls = 131072` 条与 `maxKeptFiles = 64` 个文件做淘汰，并从最旧的开始删。
  `callCounts` 只存「每条文件多少调用」作为分配提示，注释强调它**不存调用本身**（`:58`）。
- `pruneCalls()`（`:96-175`）：先按当前文件集合剔除内存里已消失的路径；再清孤儿分片
  （`.gob` / `.rev` / 超 1 小时的 `.tmp`），节流 `callSweepEvery = time.Minute`——注释记了
  「每次都扫 ~16k 分片并哈希全部路径」曾占一次热页面的 1/10（`#3493`）。
- 会话汇总（`sessions.json`）是**另一套**缓存，按 `cacheVersion` 整体失效；`zone()` 变了也会重读
  （`sessions.go:947-949`）。

### 6.7 修订窗口的预算：8 个文件、8192 条索引

`revision_cache.go` 管的是「Claude 的 message 修订表 + Codex 的差分索引」这些**运行期索引**的
内存预算。规则写得很清楚（`:8-30`）：最多 `revisionFiles = 8` 个文件、合计
`revisionEntries = 8192` 条；最近 `revisionIdle = 5min` 内变过的优先，其次按源大小（重解析成本）。
两个例外：正在写的 Codex 会话可以独占（最多 `revisionActiveFiles = 2`，每个最多
`revisionActiveEntries = 16 × 8192`），因为它「约六条索引/调用」，超过 ~1400 条调用就会丢窗口，
于是每次追加都从第一行重解析、占满一核（`yetone/magpie-releases#24`）。
`revisionWorking = time.Hour` 之外的工作会话不再优先，但「空闲的 Claude 窗口只在确实需要容量时才丢」。

`trimCallRevisions()`（`:181-208`）对 `callCache` 与 `callContinuations` 各跑一遍同一套预算，
不保留时用 `withoutRevisions()` 换成「只有调用、没有索引」的副本——注意是**替换快照**而不是原地改，
因为「解析器可能还在锁外读上一份快照」。

### 6.8 并发与节流：谁在读、读多快

- **同一文件**：`callLock(path)` 用 FNV-1a 把路径散到 64 个锁上（`call_cache.go:253-259`），
  保证两个读者不会同时读同一文件（不同文件可并行）。
- **同一次查询**：`sharedCalls()`（`calls.go:190-231`）按 `(since, agent)` 建 flight，
  并发读者等同一个结果，且全局 `callsReading` 保证「同一时刻只有一次全量读」。
  注释记了事故：Sessions 页多个请求同时进来各持一份全量调用，「进程涨到 85 GB」。
- **读多快**：`quiet.go` 把「没人等的启动预热」与「页面在等」分开。预热期间 `readSlot()`
  （`quiet.go:54-71`）用 50 ms ticker 轮询、只允许 `quietReaders() = NumCPU/4` 个读者同时读；
  一旦有页面在等（`Ask()`），`quiet()` 立刻为假，`readSlot()` 直接放行，不再限流。
  另一条并行的轴是 **Sessions 索引扫描**：`sessions.go:1175` 按 `scanReaders() = NumCPU/2`
  （`quiet.go:33`）开协程，注释记了「22/32 核被读满、别的应用变慢」的实测（`#3238`）。
  两者是不同路径，不要混为一谈。`Warm()` 结束时释放预热列举缓存。
- **列表只列一次**：`OneListing()` / `heldFiles()`（`one_listing.go:29-96`）在一次请求内共享
  各目录的列举结果（`~21k` 文件被一次轮询列两遍的问题），并且「正在列举时后来的读者等同一个
  channel」，不会各自再列一遍。
- **打包文件的一次解压两次使用**：`callsRide`（`call_cache.go:376-454`）让 Sessions 摘要扫描
  顺手产出 calls。注释给了动机：Codex App 在 10-20 重打包了 11,591 个 rollout，
  「预热为 Sessions 解析一遍、Requests 再读一遍，约 48 GB 解压两次」。
- **缓冲区复用**：`shelf[T]`（`shelf.go:11-96`）在 GC 之间保留最多 4 个缓冲/解码器，
  `shelfIdle = 10s` 后全放；理由是冷启动「约每秒一次 GC」，`sync.Pool` 在头 2 分钟里
  611 次取空、重复分配约 1.9 GB。

### 6.9 网关排他：每个 agent 的判据不一样，可靠性也不一样

这是本地读取最容易出错、也最值得抄的一条经验：**同一次调用如果既被网关记账、又被会话文件读到，
就会双计**。magpie 的解法是「读会话文件时先把走网关的调用排除」，但排除依据逐 agent 不同：

| agent | 排他判据 | 可靠性 |
| --- | --- | --- |
| OpenCode | provider id 为 `magpie` 或 `dial`（`calls_opencode.go:29-30`） | 高：id 由 magpie 写入 |
| ZCode | 同 OpenCode（`calls_zcode.go:92`），注释说明「magpie 写的路由就是 `magpie`，可信」 | 高 |
| Pi / omp | provider 为 `magpie`/`dial`（`calls_pi.go:48-49`） | 高 |
| Kimi Code | model 前缀 `magpie/`（`calls_kimi.go:63-64`） | 高 |
| dsh | provider 名集合（`magpie`/`magpie-responses`/`magpie-anthropic`/`dial`，`calls_dsh.go:209-219`） | 中：注释自承「只看名字，用户自建同名 provider 会被误判」 |
| WorkBuddy / CodeBuddy | **读当前 `models.json`**，把 `vendor == "magpie"` 的 id 视为网关（`calls_workbuddy.go:101-153`） | 低：注释自承「models.json 是用户的配置、不是调用记录；把 magpie 的模型删掉，那些当时走网关的调用就不再被排除，会再算一遍」 |
| agy | 无法排除（`calls_agy.go:34-37`） | 无：只能靠 §7.3 的 token+时间配对 |
| Claude / Codex | 不在文件层排除：这两家的文件里带着请求 id（Claude 的 `requestId`、Codex 的 `response_id`），走网关的调用由 §7.3 的 id 配对消除，而不是丢弃 | 高（有 id 即可判唯一） |

`#589`、`#1947`、`#2234`、`#2512`、`#2776`、`#3493` 这一串 issue 号说明：本地读取的口径问题
几乎全部来自「同一事实在两个文件里以不同形状出现」。

## 7. 三源合并与去重：同一调用不能算两遍

### 7.1 三种来源

`internal/usage/ledger.go:88-118` 定义并区分：

| 常量 | 值 | 行上 `Row.Source` | 含义 |
| --- | --- | --- | --- |
| `SourceGateway` | `"through"` | `""` | 网关自己转发并记账 |
| `SourceSession` | `"direct"` | `"log"` | 从 agent 会话文件补读，网关没见过 |
| `SourceReported` | `"reported"` | `"reported"` | 客户端通过 `POST /v1/magpie/usage/requests` 上报 |

`SourceOf()`（`ledger.go:107-118`）额外把「本地拒绝」和「探针」判为 `""`——即两类来源过滤器
都不含它们，与页面「本地拒绝不计入总量」的口径一致。

### 7.2 合并流程

`ledgerWithShared()` → `ledgerMatched()`（`ledger.go:415-505`）：

1. 先算匹配集（`gatewayMatchesDetails`，`ledger.go:377-406`），得到「哪些会话文件调用已经被网关
   记录覆盖」。
2. 网关记录逐条 `add()`，`Source` 保持原值（`""` 或 `"reported"`）。
3. 会话文件调用中**未匹配**的，转成 `logRecord()`（`ledger.go:351-367`）后 `add()`，`Source = "log"`。
4. 跨机同步来的记录（`SharedCall`）再追加。
5. 全部按时间倒序（`ledger.go:501`）。

`usage.Direct()` 走的是同一台匹配机器的「轻量版」（`direct.go:41-83`）：它先把会话调用装成一个
`rowChunk`，再把 ledger 索引里**可能与窗口重叠**的块解出来做 `matchedBlocks`，最后仍调
`ledgerMatched(..., recs=nil, ...)`——网关侧记录传 nil，所以只留下直连行。这解释了为什么
「Requests 页的直连行」与「Usage 页的直连摘要」口径一致：用的是同一份匹配结果与同一个
`logRecord()`。

`logRecord()` 的关键约定（**已验证**）：

- provider 记作 `UnknownProvider = "session-unknown"`（`ledger.go:342`），**不推断**计费方；
  会话文件里的 provider 只写进 `SessionProvider` 作历史。
- `Requested` 与 `Served` 的取值随 agent 不同：Claude Code 的文件同时给出「要的」和「答的」，
  Codex/OpenCode 只给「要的」，所以后者 `Served = ""`（`ledger.go:344-361`）。
- `Model` 从 `Requested` 去掉 `[1m]` 这类上下文尾标（`contextTail`，`served.go:33-35`）。

### 7.3 去重算法（`matchedBlocks`，`request_page.go:1284-1446`）

三级匹配，**优先级严格**（**已验证**）：

1. **`response_id` 配对**（`codex_identity.go:43-68`）：网关记录的 `ResponseID` 与会话调用的
   `ResponseID` 相等，且两侧都**唯一**、都属于 codex、都不是拒绝/上报来源。这是最强证据。
2. **`request_id` 配对**：网关记录的 `rid` 与会话调用的 `request_id` 相等，按时间排序后一一对应
   （`request_page.go:1295-1349`）。
3. **兜底配对**（`request_page.go:1350-1446`）：键为
   `matchKey{session, matchTokens(tokens), failed, hasID}`——
   `matchTokens` = `(input + cacheWrite, output, cacheRead)`（`request_page.go:1252-1258`），
   刻意把 cacheWrite 并入 input，因为两侧对「写入是否单独列」口径不同；
   时间窗 = 会话调用时间 ±2 秒内、网关记录的**结束时间**（`Time + Millis`）。
   要求**双向唯一**：候选组内该时间窗只含 1 条，且该条在全组也只被 1 条指向。
   实现用差分计数数组（`g.counts[lo]++ ; g.counts[hi]--`）避免「上千次相同重试的笛卡尔积」。

两个特例：

- `agy` 不发会话头，两侧会话都记作 `agyMatchSession`，时间窗放宽到 30 分钟
  （`agyStepLongest`，`request_page.go:1269-1278`），因为 transcript 的 `created_at` 是步骤**开始**
  时间，别处是调用**结束**时间。
- 有 request id 的会话调用只能匹配「网关侧也没有 id」的记录（`request_page.go:1403-1408`）——
  两个不同 id 绝不互相顶替。

设计准则（注释明说）：**歧义就保留两条**（"Fallback matches must be unique in both logs: an
ambiguous direct call stays visible"）。这与「宁可少算也不错算」的立场一致，代价是会重复计数，
但重复是「可见的」（页面能按 through/direct 分开看），而静默吞掉是不可见的。

### 7.4 Codex 身份链

`codexIdentityRow()`（`codex_identity.go:14-34`）把 Codex 的 `thread_id` 解析成
`SessionRole`（`main` / `subagent` / `unknown`）+ `Subagent` + `ParentAgent`，并把 `Session`
统一到 `RootSession`，使「子代理的调用计入其所属会话」。无法确认时显式写 `unknown`，而不是猜。

## 8. 聚合：`Totals` / `Summary` / 时间桶 / 维度

### 8.1 `Totals`

`internal/usage/usage.go:470-521`。语义要点（**已验证**）：

- `Calls` = request 数，**不含** `HandedOn` 的 try（`count()`，`usage.go:776-795`）；
  `Errors` = `Failed()` 且非 HandedOn；`Retried` = 成功但 `Tries > 1`；`Loops` = `ErrType == "reply_loop"`。
- 令牌四桶 + `CacheWrite1h` + `CacheUnknown`（含 `CacheUnknownCalls`）。
- `Cost` 只累计**已计价**调用；`Unpriced` 单独计数，**绝不按 0 计入 Cost**（`addFrom()`，
  `usage.go:740-767`）。这是本文最值得抄的一条语义：**未知价 → 计数不计钱**。
- 速度相关：`Timed`/`TTFT`/`DecodeMs`/`DecodeOut`/`DecodeN`，以及 `FloorOut`/`FloorMs`/`FloorN`
  （会话文件推算的「下限速度」，见 §8.5）。

### 8.2 两条加和路径

- `add(r, price)`：给原始 `Record` 计价后累加（`usage.go:736-767`）。
- `addRow(r Row)`：给**已经有 `Cost`** 的行累加（`ledger.go:724-754`），用于去重后的二次过滤，
  避免重复计价。

注释解释了这个设计：一个 row 先算一次自身合计（`addRow`），再加进它所属的很多分组，取代
「每个分组重新读一遍 row」（`ledger.go:756-758`，issue #3493）。

### 8.3 时间口径

`Period` 有四种写法（**已验证**）：

| 形态 | 例 | 语义 |
| --- | --- | --- |
| 预设 | `today` / `7d` / `30d` / `all` | `Since()` 从本地午夜起算；`all` 为零时刻（`ledger.go:196-218`） |
| 自定义日区间 | `2026-10-01..2026-10-07` | 两端整日、闭区间；`Until()` = 末日次日 0 点（`period_range.go:12-26`、`90-95`） |
| 滚动窗口 | `last:24h` / `last:90m` | 从 `now - d` 到 now（`period_range.go:28-56`） |
| 热力图 | `heatmap` | 53 周，周一起（`heatmap.go:16-28`） |

- 「本地」由 `Filter.Zone` 决定，`usage.Zone(name)`（`usage/zone.go:21`）解析浏览器传来的 IANA 名，
  缺省为本机。
- `shown()`（`period_range.go:102-107`）保证「摘要自称的区间」与「实际读的区间」一致。

### 8.4 桶与维度

`timeline()`（`ledger.go:624-706`）决定桶粒度：

- 滚动窗口 ≤ 48 小时 → 按小时；否则按天。
- 自定义日区间：1 天 → 小时；> 60 天 → 周（且起点回退到周一）。
- `all`：从最早调用起算，> 60 天 → 周。

`bucketIndex()`（`ledger.go:713-722`）把时间映射到桶下标；周桶用「自起点起的天数 / 7」，所以
周桶对齐取决于起点是否周一，`timeline()` 已保证。

维度（`ledger.go:877-882`）：`provider`、`agent`、`model`、`modelAt`（`provider/model`）、
`callerKey`。`Without(d)` 用于「某维度的份额统计要清掉该维度自己的筛选」，这样切换维度时其他
选项仍在。

`Summarize()` 产出七个分组（`usage.go:897-916`、`summarizeFrom` `usage.go:942-1121`）：

- `Agents`（按 `ReportedAs(Agent)`，即用户可把某客户端归并进某 agent）
- `Models`（键 `provider/model`，必要时追加 `@ host`；模型名先经 `provider.UsageAlias()` 归并，
  但**计价仍用原始 id**，`usage.go:1029-1042`）
- `ProviderKeys`（`provider#keyId`）、`Accounts`（`provider@account`）、`CallerKeys`
- `Sessions`（`agent|session`）
- `Series`（时间桶）

排序统一为「token 多者在前 → 调用多者 → id 升序」（`usage.go:1090-1099`）。
图表叠加时只保留各点 Top 24（`seriesKeep`，`ledger.go:1045-1048`），其余并入该点合计，
避免前端渲染几百条曲线。

热力图口径（`heatmap.go:59-109`）：按本地日聚合调用数、四桶 token 总和、已计价成本与未计价计数；
`HandedOn` 不计入调用数。

### 8.5 速度口径（附带但很典型）

magpie 对「输出速度」的处理是本文档里最精细的部分，值得单列，因为它示范了**如何避免把厂商
的"一次性推送"当成高速生成**：

- `DecodeWindow()`（`usage.go:565-579`）：窗口 < 100 ms 或「token/秒 > 10000」→ 判定不是真实
  写入速率，返回 0。
- `DecodeOf()`（`usage.go:616-640`）：推理型回复只计「答案」（`output - reasoning`）从其首个
  文本起算；若 `flow`（内容到达耗时的占比）与窗口比值超过 `HeldShare = 4`（即内容在窗口的
  1/4 内到达），判为「被压住后爆发」，不给速度；推理与答案的节奏差超 `ThinkPace = 20` 倍同样放弃。
- 会话文件来源另有 `Floor()`（`ledger.go:857-875`）：输出 ≥ 1000 token 且 ≥ 1 秒时，给一个
  「不高于此」的下限速率；Codex 单调用轮次另用 `CodexWindowMs = 1000` / `CodexBodyTokens = 100`
  过滤。

## 9. 计价：价格从哪来、怎么乘、怎么处理未知

### 9.1 价格结构

`catalog.Price`（`internal/catalog/catalog.go:181-198`）：**USD / 百万 token**，五项
（`Input`、`Output`、`CacheRead`、`CacheWrite`、`CacheWrite1h`）+ `Tiers`。

`Tier`（`catalog.go:200-210`）：`Above`（输入阈值，**含缓存读与缓存写**）与整单的五项价格。

解析兼容 models.dev 的两种写法（`catalog.go:212-260`）：`above` 直接给，或
`{"tier":{"type":"context","size":N}}`；`type` 不是 `context` 的条目丢弃；缺项回退基础价。

### 9.2 计算

```go
// catalog.go:299-307
func (p Price) At(prompt int) Price {
    flat := 基础价
    for _, t := range p.Tiers { if prompt > t.Above { flat = t 的整单价 } }
    return flat
}

// catalog.go:328-334
func (p Price) CostSplit(input, output, cacheRead, cacheWrite, cacheWrite1h int) float64 {
    cacheWrite1h = clamp(cacheWrite1h, 0, max(cacheWrite,0))
    f := p.At(input + cacheRead + cacheWrite)          // 档位由整单 prompt 决定
    return (input*f.Input + output*f.Output +
            cacheRead*f.CacheRead + (cacheWrite-cacheWrite1h)*f.CacheWrite +
            cacheWrite1h*f.OneHour()) / 1e6
}
```

要点（**已验证**）：

- **整单按一档计价**（不是分段计价）。档位判定用 `input+cacheRead+cacheWrite`，与 OpenAI 的
  「输入含缓存」口径一致。
- `OneHour()`（`catalog.go:309-316`）：1 小时缓存写有独立价，否则回退 5 分钟写入价。
- `OneHourFor()`（`catalog.go:262-278`）：Claude 系模型没给 1 小时价时，按 **2× 输入价**补齐
  （Anthropic 的定价事实），且只对 id 含 `claude` 的模型做——注释特意说明「gpt-6-astra 曾被
  错误套上 $20 的 1 小时价」。
- 推理 token **按输出计价**，因为它已经包含在 `Output` 里（`catalog.go:318-320`）。
- 计价入口：`Record.CostAt(p)`（`usage.go:460-468`）——若 `OwnCost != nil`（上报调用自报了成本）
  则直接用自报值，否则 `CostSplit`。

**已知口径不一致（很重要）**：请求级（`block_price.go:186`、`usage.go:463-468`）用**带档**的
`CostSplit`；而**会话级汇总**用 `p.At(0).CostSplit(...)`（`internal/sessions/sessions.go:1576-1580`，
同样 `sessions/stats.go:336`、`:410`），理由写在注释里：「不知道哪一次调用越过了档」。后果是
**同一个模型、同一批 token，在「请求明细」与「会话汇总」两处可能给出不同金额**——这是取舍，
不是 bug，但任何做对照的人都必须知道。

### 9.3 有效价解析链（最重要的一节）

`provider.EffectivePriceIn` → `priceOf`（`internal/provider/models.go:2187-2229`）的顺序是**严格优先**的：

1. **用户显式设置的价**（`settings.ModelPrices`），三把键依次尝试
   （`models.go:2202-2210`）：
   `providerID/model` → `providerID/*`（该 provider 全部模型）→ `AnyPriceKey(model)`（`*/model`，
   任意 provider）。
   外置价里 Claude 模型若缺 1 小时价，同样按 `OneHourFor` 补 2× 输入。
   **显式 0 视为有效设置**（`models.go:2159-2160`：「false only when no price is known at all,
   which is not the same as a price of zero: that one is set, deliberately」）。
2. **provider 自己的目录价**（`Provider.ListPrice`，`models.go:1823-1879`），内部还有子序：
   远程 magpie 报价 → Cline/Kilo 免费模型（记 0）→ Cursor/Factory/Grok 的 fast 变体价 →
   provider 目录（`catalog.PricedBy(priceCatalogs(cats), name)`，模型名有多个候选写法）→
   maker 目录 → Anthropic 的 Claude Code 价（`claudeCodePrice`）→ 插件报价 → 插件声明免费。
3. **厂商价**（`MakerPrice`，`models.go:2128-2138`）：按预设厂商顺序，先命中先得。
4. **用户声明的「同价模型」**（`SamePriceIn`，`models.go:2140-2153`）：`settings.ModelSameAs[provider/model]`
   指向另一个模型 id，取其厂商价。这是「provider 自有 id（如 `kimi-k3-1`）无人报价」时的最后手段。
5. **乘以 provider 的价格倍率** `PriceRate`（`models.go:2224-2227`）：用于「中转按官方价打折/加价」。

`EffectivePriceIn` 存在的意义是「一次读 settings、给整张模型表定价」，避免每模型读一次文件
（`models.go:2168-2181`）。这与 TokenScope 的「查询级价格快照」是同一动机。

#### 目录内部的匹配顺序（`catalog.PricedBy`，`catalog.go:810-852`）

拿到「provider 的候选目录集合」后，具体按以下顺序找条目（**已验证**）：

1. 原样 id 精确匹配（所以 `vendor/model` 形式的键可以原样命中）；
2. 剥掉 Bedrock 地理前缀（`us.`、`eu.`、`apac.`、`global.` 等，`catalog.go:1505`）；
3. `bareID` 之后的几种截断：原名、`(` 前、`:` 前（`catalog.go:1450-1456`、`:1494-1501`）；
4. 点换横线（`dashed`，`catalog.go:945-947`）；
5. `canonical_model_id`（models.dev 声明的规范 id，如 `deepseek/deepseek-v4.1-flash`），
   但**只认「厂商自己」列出的 canonical**（`catalog.go:901-943`），且同 canonical 有多个不同价
   时视为未知；
6. 上述都失败时，按 `renamed()` 去掉受限前缀后重试（`catalog.go:848-850`）。

`renamed()` 的三种剥离（`catalog.go:1254-1279`）都要求「剥完后必须是一个已知模型」才会采用：
去窗口标记 `[1m]`/`[200k]`、去 `claude-` 前缀（剩余须含数字）、去频道词 `^[a-z]{2,24}-`。
**没有通用的前后缀模糊回退**——日期后缀的回退不命中（`renamed_test.go` 用例专门锁死这点）。

**两个与 TokenScope 直接相关的差异**：

- magpie **没有紧凑等价键**（不删 `-` `._`），等价靠 `bareID` + `dashed` + `canonical` 三件；
  所以 `claudeopus55` 这种压缩写法在它这里永远不会命中。
- 同一模型在多个 provider 目录里都有价时，`PricedBy` **返回第一个命中，不比较、不择优**
  （`catalog.go:812-827`）。TokenScope 现行规则是「多渠道候选逐一计价取最高费用」，两者结论
  会不同——这不是谁对谁错，而是「按 provider 自己的 tariff」与「取最坏情况上界」两种立场。

### 9.4 未知价格的处置

`usage.Pricer()`（`internal/usage/ledger.go:522-551`）返回一个函数 `func(Record) *catalog.Price`：

- 每次查询新建一个 `prices map[[3]string]*catalog.Price` 缓存，键为
  `(priceID, Model, Served)`（`ledger.go:534`）。
- 先按 `Model` 求有效价；**若失败且 `Served != Model`，再按 `Served` 求一次**
  （`ledger.go:540-542`）——理由：中转把 `kimi-k3` 卖成 `moonshot-kimi-k3`，回复却报 `kimi-k3`，
  而后者才在目录里（issue #1498）。
- 都没有 → 调用 `catalog.Missing()`（触发后台补拉 models.dev，`ledger.go:546`），返回 `nil`。
- `nil` 的后果：`Totals.Unpriced++`，`Cost` 不变（§8.1）。**永不按 0 静默吞掉。**
- `Pricer` 是「一次查询一个快照」：`NewPricer()`（`ledger.go:556-566`）明确要求「下一页重读
  价格，让改价反映到历史」。

**缺价自愈有个前提**：`catalog.Missing()` 只在 `KeepFresh` 正在跑的长驻进程里才真正发起抓取，
且距上次 ≥ 6 小时、源文件也是旧的（`fresh.go:51-61`）。所以 CLI 的单次查询「发现无价」并不会
自己去补价（`fresh_test.go:43-48` 的用例名即 `TestMissingDoesNothingWithoutKeepFresh`）。
换句话说：**magpie 把「补价」交给了常驻进程，把「未知」如实留在报告里**。

### 9.5 价格数据来源与刷新

`internal/catalog`（**已验证**）。先给结论：**没有任何编译期内置价格表**——`catalog.go:971-974`
的注释直接写「nothing is compiled in」，全仓 `go:embed` 只用于提示词、GUI 资源、插件与 PII 模型。

- 主源 **models.dev**：`modelsDevURL = "https://models.dev/api.json"`（`catalog.go:478`），
  缓存 `CachePath()` = `appdir.Cache()/models.json`（`catalog.go:525`；Cache 目录按
  portable → `$XDG_CACHE_HOME/magpie` → `~/.cache/magpie` 解析，`internal/appdir/appdir.go:256-266`）。
  额外还会读 `~/.cache/opencode/models.json`，且 **magpie 自己的缓存优先**（`catalog.go:527-530`、`:534`）。
  加载用代际号 `loadGen`（`catalog.go:549-707`），`Reset()` 清空并自增（`catalog.go:713-723`），
  保证「读期间同步不影响本代」。
- 同步本身：`Sync`/`syncLock`（`catalog.go:727-781`）HTTP GET，带 `User-Agent: magpie`，
  **不带任何鉴权头**（`catalog.go:751`），响应上限 64 MB，JSON 非空才落盘，落盘后
  `syncLabs` + `Reset` + `Touched`。
- **labs 渠道只补能力、不补价格**：`labs.json` 与主缓存同目录（`labs.go:28`），内容是模型名、
  上下文/输出上限、thinking、输入模态、发布日（`labs.go:3-11`、`:164-165`），**没有价格字段**。
  并入条件也严格：仅当「厂商目录与 canonical 都找不到该模型」时才并（`catalog.go:655-671`）。
  写入用临时文件 + rename 原子替换，失败保留旧文件（`labs.go:61-114`），且每进程只尝试一次
  （`labsTried`，`labs.go:33`、`:66`）。
- **OpenRouter 在这个仓里不是价格输入源**：`openrouter.go` 全文只有两个输出函数
  `OpenRouterPricing(Price) map[string]string`（`openrouter.go:16-25`）与
  `PerToken(perMillion float64) string`（`openrouter.go:30-45`）——它们把 magpie 自己的价**写成**
  OpenRouter 的「每 token」格式给客户端看；OpenRouter 只作为 models.dev 里的一个 provider 参与
  同一个 `PricedBy`。`liveModel` 里也没有 `pricing` 字段（`live.go:682-731`）。
- **Gemini 官方 list 只给能力**：`FetchGemini()`（`gemini_list.go:23`）取可用模型与上下文窗口，
  同样不带价格（`gemini_list.go:52-58`、`:95-105`）。
- **实时模型列表**（每个 provider 自己的 `/v1/models`）：`live.go` 整包，含分页（20 页上限、
  每页 1000）、双鉴权探测（`authheader.go:11-53`：默认同时发 `Bearer` 与 `x-api-key`，被判
  「两头并存」时按 `OneAuthOrder` 逐个单头重试并记住）、用户自定义头（`userheader.go:19-29`，
  保留原大小写）、8 秒超时 / 8 MB 上限。列表里可自带价（`live.go:876-894` 的 `Price.sane()`
  拒绝 NaN、负数、>1e6、档 `Above <= 0`）。
- **另一台 magpie 的价**：live 列表里的 `magpie_price` 字段（`live.go:643-646`、`:717-720`），
  仅供 `remote magpie` provider 使用（`internal/provider/remote_magpie.go:150-160`，头
  `X-Magpie-Prices`）。这是「多机 magpie 之间报价一致」的机制，与 TokenScope 无关但值得知道。
- 刷新节奏（`fresh.go:19-28`）：`staleAfter = 24h`、`missingAfter = 6h`、`freshEvery = 1h`
  （`KeepFresh` 每小时巡检，`fresh.go:37-45`）；`fetch` 单飞、30 秒超时（`fresh.go:64-74`）。
  `Stale()` = 无源 / labs 缺失 / mtime 超 24h（`catalog.go:785-792`）。
- **降级行为**：models.dev 读不到时 `PriceOf`/`PricedBy` 一律 `false`（`catalog.go:555-559`、
  `:801`、`:851`），同步失败只记日志、保留旧缓存（`fresh.go:72-74`）。注意这里**没有日志提示
  「缓存缺失导致全部未定价」**，只能靠 `Stale()` / `Source()` 排查——是 magpie 侧的一处可观测性
  缺口。
- `catalog.Missing()`（`fresh.go:51`）在「查价失败」时被调用，是「缺价自愈」的触发点，
  但受 §9.4 末尾的常驻进程前提约束。

### 9.6 `Swapped`：模型被偷换还是只是改名

`internal/usage/served.go` 处理「回复自报的模型 ≠ 请求的模型」，这是金额与信任的关键：

- `bareModel()`（`served.go:44-55`）逐层剥离：`[1m]` 上下文尾标、`@256k` 窗口尾标、
  `/` 前的 vendor 路径、Bedrock 的 `us.anthropic.` 前缀、版本尾巴
  （日期 / `v1:0` / `latest` / `preview`）。
- `Swapped(a,b)`（`served.go:68-71`）在 `squash`（去 `.`/`-`/`_`/空格）后比较，并且**排除**四类
  「看着像换模型其实不是」的情况：路由分组转发（`GroupRouted`）、厂商自己的部署名
  （`ServingName`：Gemini 的 `-n`、Grok 的 `-build`、Cline 的 `vmc/`）、中转冠名
  （`RelayRenamed`：`claude-deepseek-v4.1` → `deepseek-v4.1`）、模型列表里的别名
  （`ListedAs`，按 provider 报价列表，1 分钟缓存）。
- 与计价的关系：`Pricer` 的 `Served` 回退（§9.4）会让「被偷换成一个目录里有价的更便宜模型」
  的记录按**回复模型**计价；反之若回复模型无价，则仍按请求模型计价并标 `Swapped` 供人工核对。

## 10. 金额展示、预算与跨机共享

### 10.1 展示

`FormatCost(amountUSD, currency, rate)`（`usage.go:661-691`）：

- 内部一律 USD 存储；`currency == "cny"` 且 `rate > 0` 时乘汇率、显示 `¥`。
- 分级：`≥ 100` 取整、`≥ 1` 两位小数、其余三位（「不足一分也显示成非零」）。
- 汇率来源 `internal/fx`（`fx.go:1-8`）：`open.er-api.com` 免费接口，`TTL = 12h`，
  失败后 `retryAfter = 10min` 内不再重试；从没联网过则用 `Fallback = 7.2`。
  `Soon()`（`fx.go:136-158`）先返回旧值再后台刷新，保证页面不被网络阻塞。
- `FormatCredits()`（`usage.go:661-669`）：`Credits` 单独格式化，`<0.01` 显示为 `<0.01 credits`。

### 10.2 预算（把用量反过来当限流依据）

`internal/budget/budget.go:1-31` 的包注释把规则写得极清楚，是本文档里「用量数据二次利用」的最佳
范例（**已验证**）：

- 窗口：本地日历窗口，或「从设置限额起 N 天」的周期；`Reset` 后从重置点重算。
- 令牌计数：`input + output + cacheWrite`，`CacheRead` 可选（`sums.tokens`，`budget.go:69-75`）。
- 成本：用与 Usage 页**同一套有效价**（`usage.NewCoster()`，`budget.go:134`）。
- 未知价：不加成本（与 §8.1 同口径）。
- 在途预留（`budget.go:20-23`）：`body 字节数 / 4` + 该密钥本窗口平均输出；「并行请求会超出约
  一次调用」被显式承认。
- 只对带网关密钥的请求生效；上报来源（`IsReported`）不计入（`budget.go:98-101`）。

### 10.3 跨机共享

`usage/shared.go`：

- 开同步后，每台机器把自己 Usage 页能看到的调用按天写成文件，放到服务器
  （`SharedDay` / `SharedCall`，`shared.go:32-45`），本地拉到 `usage-others/<computerId>/<day>.json`
  （`shared.go:49-50`）。保留 `SharedDays = 90` 天（`shared.go:30`）。
- 本机身份 `usage-computer.json`（`shared.go:60-87`），**不参与备份与同步**，损坏时先另存再重写。
- 分享内容**只含账本字段**（时间、token、模型、provider、状态），不含请求内容
  （`shared.go:1-8` 的注释明确）。
- 读回时按天拼接（`sharedRecords()`，`shared.go:282-300`），`Computer` 字段标源，页面可切
  「本机 / 其他机器」。

## 11. 对外接口与导出

- **同一份 JSON 两个入口**（`internal/usageapi/usageapi.go:1-5`）：GUI 的 `/api/usage` 与网关的
  `/v1/magpie/usage` 都由 `usageapi` 生成。
- **摘要**（`State`/`Page`/`PageIn`，`usageapi.go:57-84`）：`Page` 只带 Top 100 会话
  （`PageSessions`），因为「750k 调用的历史曾一次回传 84k 会话、22 MB」，且每次都重算。
- **明细分页**（`LedgerPage`，`usageapi.go:303-436`）：默认 100 行、上限 500；同一响应里带
  `Totals`、`Through`、`Direct`、`Reported` 三份合计，前端因此能同时显示「总和 / 网关见到的 /
  会话文件补的 / 上报的」。
- **每行附带的展示信息**：`AgentName`、`Icon`、`ProviderName`、`Access`（`api` / `subscription`）、
  `PricingModel`（实际用来计价的模型，与展示模型不同才填，`usageapi.go:351-355`）、
  `CallerKeyLabel`、`ComputerName`。
- **上报接口** `POST /v1/magpie/usage/requests`：外部客户端把「自己直连厂商、magpie 没看到」的调用
  报进来（`SourceReported`），可带 `cost`（`OwnCost`）。这类记录**只进 Usage 页**（§4）。
- **CSV**（`ledger.go:568-623`）：47 列固定表头，`cost_usd` 保留 6 位小数、未知价留空；
  另有 `cache_write_5m_tokens` / `cache_write_1h_tokens` 拆分列。
- **OTLP**（`internal/otlp/otel.go:37-49`）：仅在插件开启时导出；队列 128、批 32，
  「整包 body 额外限 16 MB」；未开启时内核完全不导出。这就是包注释里那句「Metadata leaves the
  machine only when OTLP export is explicitly enabled」（`usage.go:3-4`）的实现。
- **插件**：`plugins/usage/usage.go` 只是页面注册（`Essential: true`，依赖 `window`），前端资源
  名 `i18n.js` / `client.js` / `page.js` 未入库——所以页面细节无法从本仓核实（**未验证**）。

## 12. 支持的 Agent：三层清单

这是本题要求「把它支持的 Agent 都分析」的答案。必须分三层看，因为**接入的 agent 远多于能本地
统计的 agent**：接入 68 个（`onMachine`）+ omp profile 展开 + WSL 各发行版实例；而能读本地会话
历史的只有 13 个；剩下的只能靠「经网关」或「客户端上报」被看到。

### 12.1 第一层：接入层（`internal/agent`）

`agent.All()` = `builtins(home, cfg)` + 插件新增（`agents.go:120-126`）；
`builtins()` = `onMachine(...)` + `wslAgents()`（`agents.go:138-140`）。

`onMachine()`（`agents.go:153-224`）一口气构造 **68 个**内置 agent（按源码顺序）：

`claude`、`claudeDesktop`、`claudeScience`、`codex`、`gemini`、`agy`、`opencode`、`openChamber`、
`mimocode`、`deveco`、`pi`、`stepcode`、`mothx`、`penguin`、`aside`、`omo`、`primeAgent`、`goose`、
`cursor`、`cursorLocal`、`zed`、`vscode`、`vscodeInsidersAgent`、`vscodium`、`zooCode`、
`copilotJetBrains`、`air`、`copilot`、`crush`、`dsh`、`reasonix`、`commandCode`、`fx`、`omp`、
`devin`、`hermes`、`morph`、`kimi`、`qwen`、`muse`、`empryo`、`ante`、`astrbot`、`jcode`、
`miniMax`、`droid`、`cline`、`qoder`、`qoderCN`、`grok`、`grokApp`、`zcode`、`workbuddy`、
`workbuddyAI`、`codebuddy`、`pencil`、`openPencil`、`newMax`、`raycast`、`kiroCrew`、`t3code`、
`hanako`、`atomcode`、`snow`、`alma`、`cindy`、`aionui`、`lody`；另加 `ompProfiles(home)`
（每个 omp profile 一个虚拟 agent）与 `wslAgents()`（WSL 发行版内的同名 agent）。

`Agent` 结构（`agent/agent.go:117-180`）说明「接入」到底是什么：

- `Dir` / `Path`：它的配置目录与 magpie 会编辑的配置文件；
- `Fields []Field`：模型、effort 等可改字段，`Set` 为空串即「恢复 agent 自身默认」；
- `Aliases []string` + `UA []string`：网关识别请求归属**主要靠这两个**，且**名字/别名优先于
  UA 前缀**（§12.3）。注意约 20 余个内置 agent 并未赋 `UA`（子代理逐个核对后标注），它们只能靠
  产品名 fallback 或专属 key/宿主信息区分——所以「UA 是唯一入口」这个说法不成立；
- `Unwire` / `Restore`：断开时把 magpie 写进去的东西撤掉并还原备份（`beforeMagpie` 的 stash）；
- `Sync` / `ListsModels` / `NoNames`：模型列表写进它自己文件的几种形态；
- `LastUsed` / `Reached`（`agent.go:227-234`）：**只服务 drift 诊断**（「配置写着 magpie 但请求
  实际没走 magpie」），实现是读 `~/.claude/history.jsonl` 与 `~/.codex/history.jsonl` 的时间戳，
  **不产出任何用量行**。这一点容易与 §6 的会话文件读取混淆，必须分清。

此外 `others`（`agents.go:96-104`）声明三个「不走接入、只被请求认出来」的客户端：
`magpie`、`curl`、`pi-desktop`（PI-Desktop 跑 Pi 的库，UA 是 `pi (…)`，所以给它自己的
`pi-desktop/` 前缀才不会与 Pi 混淆）。`Clients()`（`agents.go:109-118`）再把这些与内置 agent 合起来，
并**剔除 WSL 实例**——理由是「WSL 里 agent 的请求按 UA 算它 Windows 双胞胎的」。

**插件通道（容易被误解的一点）**：内置的 15 个 `plugins/` 包**没有一个新增 agent 或 provider**，
它们都是页面或网关横切能力（`plugins/usage/usage.go:1-16`、`plugins/gateway/gateway.go:1-8`、
`plugins/routing/routing.go:1-5` 都是页面注册；`pluginrt.Manifest` 的字段里也没有 agent/provider）。
真正新增 agent 的是**第三方插件**：`package.json` 里写 `magpie.agent` 指向 `*.agent.js`
（`internal/plugin/agent.go:10-42`）→ `internal/agentplug` 用 moejs 在进程内执行并取出 `Desc`
（`internal/agentplug/agentplug.go:1-92`）→ `internal/agent/plugged.go:85-113` 包装成普通 `Agent`，
并要求「ID 或别名撞上内置 agent（含 magpie 自身）就跳过并报错」。插件 agent 同样注册进
`usage.PluginAgents`（`plugged.go:31-42`），所以也能被 `AgentOf` 认出来。
插件新增 provider 走 `internal/plugin/providers.go`，运行时表现为
`Provider.Account.plugin != nil`（`IsPlugin()`），id 会加 `-plugin` 后缀避免撞名。

### 12.2 第二层：本地会话统计层（`internal/sessions`）

见 §6.2：**13 个 agent** 有真正的会话文件解析器。

### 12.3 第三层：网关观测层（不限 agent）

任何带 User-Agent 打到网关的客户端都会被记账，归属由
`usage.AgentOf(ua)`（`usage.go:422-443`）判定：

1. 先精确匹配已知 agent 的**名字/别名**（小写）；
2. 再按 **UA 前缀**匹配（注释举例：`cursor-local` 先命中，避免被 `cursor` 前缀吃掉）；
3. 都不中 → 取 UA 的 `/` 前、空格前的产品名；空 → `"other"`。

再经 `usage.ReportedAs(ua)`（`usage/client_agents.go:41-63`）允许用户把某客户端
（`settings.ClientAgents`）**归并计到某个 agent 名下**，且保留原客户端名在 `Row.Client`。
`settings.ClientAgents` 变了会让缓存键失效（`clientAgentsKey`，`client_agents.go:38-39`）。

会话标识的解析（`internal/gateway/gateway.go`，**已验证**）：

- magpie 自有头 `X-Magpie-Session` 优先（`gateway.go:7034`）。
- 其次是 9 个客户端自带头，按序尝试（`gateway.go:7011-7015`）：`x-opencode-session`、
  `x-session-affinity`、`x-session-id`、`session_id`、`session-id`、
  `x-claude-code-session-id`、`x-conversation-id`、`x-deepseek-harness-session-id`。
- `sessionOf()` 依序取值并截断到 128 字符（`gateway.go:7043-7054`）；`nativeSessionOf()`
  只看客户端头，所以 magpie 头覆盖时仍保留原生 id（`gateway.go:7076-7084`）；完全没有会话头时
  用首条 user 消息派生 `conversationID`（`gateway.go:7086-7100`）。
- 子代理：`x-claude-code-agent-id` + `x-claude-code-parent-agent-id`（`gateway.go:7056-7068`）。
- **Codex 没有会话头**：它的会话 id 来自 rollout 文件名正则（`internal/sessions/calls.go:398-402`），
  `Session` 与 `ThreadID` 同取该值。这解释了为什么 Codex 的去重只能靠 `response_id`/token+时间，
  而不能靠 session 头对齐。

### 12.4 汇总表

| agent | 接入（`internal/agent`） | 本地会话统计（`internal/sessions`） | 备注 |
| --- | --- | --- | --- |
| Claude Code (`claude`) | 是 | 是（`projects/**/*.jsonl`） | 会话文件侧唯一有 `cache_creation` 5m/1h 拆分与 `identity.modelId` 的 agent |
| Claude Desktop / Cowork (`claude-desktop`) | 是 | 是（各自 `.claude` 目录） | 通过 `entrypoint == "claude-desktop"` 与 CLI 区分 |
| Codex (`codex`) | 是 | 是（rollout+压缩） | 有 `response_id`，配对最强 |
| Gemini CLI | 是 | 否 | 无法从文件读用量（**未验证**是否有其他路径） |
| OpenCode | 是 | 是（SQLite/JSON） | 网关来源的记录直接跳过 |
| Antigravity (`agy`) | 是 | 是（transcript） | 无 session 头，靠 token+时间配对 |
| WorkBuddy / WorkBuddy AI | 是（两个） | 是（同一 reader，两目录） | `providerData.usage` + `credit` 积分 |
| CodeBuddy Code | 是 | 是（复用 WorkBuddy reader） | |
| MiniMax Code | 是 | 是 | 消息文件可能被后续世代替换 |
| DeepSeek Harness (`dsh`) | 是 | 是（zstd 帧续读） | |
| Kimi Code | 是 | 是（`usage.record`） | fork 复制记录，按 key 去重 |
| Pi / omp | 是 | 是（`lineCallReaders`） | omp profile 视为多个 agent |
| ZCode | 是 | 是 | 自有 JSON/DB |
| 其余内置 agent（`droid`、`cline`、`qoder`、`qoder-cn`、`grok`、`grok-app`、`hermes`、`qwen`、`devin`、`crush`、`goose`、`zed`、`vscode` 系、`cursor` 系、`copilot` 系等，约 55 个） | 是 | 否（本仓无 reader） | 只能通过网关观测 |
| 任意未知客户端 | — | — | 网关按 UA 记账，可被 `ClientAgents` 归并 |

**结论**：magpie 的「支持」在统计意义上分三档——①能读本地历史（13 个 agent，可覆盖「安装
magpie 之前」的用量）；②只能读到经自己网关的请求（其余 ~55 个接入 agent + 任意客户端）；
③外部客户端主动上报（`reported`）。三档在页面上用 `through` / `direct` / `reported` 明确区分，
不合账。

## 13. 与 TokenScope 的对照：可抄的与要避的

### 13.1 结论先行

两者的根本差异不在技术栈，而在**事实源**：magpie 是「网关记账 + 会话补录」，TokenScope 是
「纯只读扫描本地会话」。因此 magpie 的**采集层**大部分不可直接搬（我们没有请求可用），
但它的**语义层、聚合层、计价的边界处理**价值极高，且恰好补上 TokenScope 的几个薄弱点。

### 13.2 值得抄的（按价值排序）

1. **未知价必须显式计数，绝不按 0 计**（`Unpriced`）。TokenScope 已有「未收录模型始终未知」的
   原则，magpie 把这条落成了**可展示的计数器**（每层分组都带 `unpriced`），值得对齐到
   `Group` 级别而不只是全局横幅。
2. **计费口径 ≠ 上下文口径**：`Deferred` 的处理（计费并入、prompt 不并入）与 `CacheUnknown`
   （厂商不报缓存时不算 miss）是两条现成的语义区分。TokenScope 的缓存命中率应显式排除
   「厂商未报缓存」的 prompt。
3. **来源三分、不合账**：`through` / `direct` / `reported` 各自有合计，页面可独立筛选。若
   TokenScope 将来接入「外部上报」或「网关旁路」，这套分源模型可直接用。
4. **双向唯一去重，歧义保留**（`matchedBlocks`）：`response_id` → `request_id` →
   `(session, tokens, failed)` + ±2s。TokenScope 目前按 `(session_id, message.id)` 与
   `(session_id, model, 四桶)` 做指纹，缺的正是「优先级分层」和「歧义显式保留」。
   **`matchTokens` 把 cacheWrite 并入 input** 的做法尤其值得抄——不同工具的「写入是否单列」
   不一致，强行逐桶比较会漏配。
5. **计价快照的粒度**：`NewPricer()` 要求「下一页重读价格」，`EffectivePriceIn` 要求
   「一次读 settings 给整表定价」。与 TokenScope 的 `query_begin` 冻结价格修订等价，可作交叉
   验证。
6. **`Pricer` 的 `Served` 回退**：模型无价时按「回复自报的模型」再查一次。TokenScope 已有
   `Served` 概念不足的场景——如果将来解析到「回复模型名」，这是现成的兜底顺序。
7. **`FormatCost` 的分级与 `fx` 的降级**：`>=100`/`>=1`/其余 三档、汇率 TTL 12h + 失败回退、
   `Soon()` 先返回旧值。TokenScope 若要显示 CNY，这套降级链可直接照抄。
8. **`bareModel` + `Swapped` 的排除清单**：`[1m]` 尾标、`@256k` 窗口尾标、Bedrock 地域前缀、
   版本尾巴（日期/`v1:0`/`latest`/`preview`）、`-n` / `-build` / `vmc/` 部署名。这套规则可以
   直接用作 TokenScope「同一模型多写法」的归一补充（我们现在用的是「删 `-._` + 分隔符边界
   前缀回退」，magpie 的剥离顺序是「先去尾标 → 再取末段 → 再去 vendor 前缀 → 去版本尾巴」，
   顺序更稳）。
9. **索引的可丢弃性与「块永不淘汰」的教训**：magpie 记下了「按预算丢块 → 每次全量重解析 6 秒」
   的坑。TokenScope 的 SQLite 事实源没这个问题，但**缓存/派生文件的淘汰策略**应引以为戒。
10. **`handed_on` 语义**：一次失败但被转交的 try，**token 计入、request 不计、错误不计**。
    多 try/多重试的工具（Codex）值得引入等价字段。
11. **「直连摘要」与「网关摘要」用同一个匹配结果**：`Summaries()` 一次取两份（`direct.go:23-33`），
    避免跨午夜导致「一天的走网关 + 另一天的直连」并列。TokenScope 若同时展示两类来源，
    应把「同一时刻冻结」做成硬约束，而不是两次查询。
12. **`bareModel` 的剥离顺序**：先去上下文尾标（`[1m]`）→ 再去窗口尾标（`@256k`）→ 取最后一个
    `/` 之后 → 去 Bedrock 厂商/地域前缀 → 去版本尾巴。这个顺序比「先删分隔符」更稳，因为
    `[1m]` 里含字母 `m`、`@256k` 里含数字，先删分隔符会把它们粘进模型名。
13. **缓存版本号当目录名，不做迁移**（`calls-v15`，§6.5）：解析口径一改就换目录、旧目录整体删除。
    TokenScope 的 `SETTINGS_FORMAT_REVISION` 管的是设置，**解析口径变更**这一轴值得照搬
    「显式版本 + 旧派生数据整体作废」，而不是写兼容分支。
14. **`Known` 位图区分「缺失」与「显式 0」**（§6.3 的 Codex）：用一个 `uint8` 记哪些字段真的
    出现在日志里，匹配时只比较双方都 known 的字段。TokenScope 现在对各字段缺省一律按 0，
    遇到「厂商只报部分字段」的工具会误判相等/不等，值得引入同构机制。
15. **修订窗口的预算 + 例外**（§6.7）：8 个文件 / 8192 条，但「正在写的 Codex 会话」允许
    独占 2 个窗口、各 16× 上限。TokenScope 若做解析缓存，需要同类的「预算 + 活跃源例外」，
    否则一个大会话会把所有小文件挤出去，退化成每次全量重解析。
16. **读取编排的三层共享**（§6.8）：同文件加锁（64 桶）、同查询共享 flight、同请求共享目录列举。
    TokenScope 是全量扫描、单一入口，这三层目前都不需要；但若引入增量扫描，这是最小可用的骨架。

### 13.3 要避的

1. **行级 JSONL 无 schema 版本**：magpie 靠「字段缺失 = 老记录」+ 大量 `omitempty` 兼容，导致
   需要 `HandedOn` 的后验重建（`log_index.go:242-261`）、`IsRejected` 的兼容式
   （`usage.go:342-343`）。TokenScope 已有 `SETTINGS_FORMAT_REVISION` 与迁移台账，应坚持。
2. **自研列式压缩 + shape 编码**（`request_page.go:67-215`）：省内存但可读性极差、调试成本高。
   TokenScope 用 SQLite 是对的，不要为了「80 字节/行」重造。
3. **agent 特例散落**：`served.go` 里 Gemini/Grok/Cline 的命名特例、`chat.go` 的 `whole` 启发式、
   `kimiCodeAPI` 映射表——这些都是长期 issue 驱动的补丁堆。若 TokenScope 要做等价能力，应把
   「模型名归一」做成**单一规则表 + fixture 驱动测试**，而不是散在解析器里。
4. **内存索引规模**：`~80 B/调用` 意味着 750k 调用 ≈ 60 MB 常驻，再加会话文件调用与磁盘分片，
   注释里出现过 85 GB 的事故（§6.4）。TokenScope 已经踩过同类坑（缓存 purge 事故），继续
   保持「分批读 + 不留全量」。
5. **`Provider == "" && Status >= 400` 这类隐式判定**：可读性差且易误判，新实现应显式建模。
6. **同一批数据两套计价口径**：请求级带档、会话级 `At(0)`（§9.2 末尾）。TokenScope 若引入
   上下文分段，**绝不能在不同视图用不同档位判定**——这类不一致一旦落到用户账单上就无法解释。
7. **多渠道价「首个命中」而不是「取最高」**：magpie 的立场是「这个 provider 的 tariff」，
   TokenScope 的立场是「候选渠道的上界」。若将来要导入 magpie 的价格数据，必须记住两者的
   `cost` 不可直接比对。
8. **缺失缓存静默降级**：models.dev 缓存缺失时全部模型变「未定价」且无日志（§9.5 末）。
   TokenScope 已有「首次启动无快照 → 全局横幅提示」，属于同类问题的正确处理方式，应保持。
9. **用「当前配置」反推「历史调用归属」**：WorkBuddy 的网关排他靠读现在的 `models.json`
   （§6.9），作者已在注释里承认「把 magpie 的模型删掉，当时走网关的调用就会被再算一遍」。
   TokenScope 判定任何来源归属时，只应使用日志内的证据，不用「现在装了什么」推断过去。
10. **只靠 provider 名字判定网关**：dsh 的排他是四个固定名字（§6.9），用户自建同名 provider
    会被误判为「两边都不算」。要做同类判定，必须找到可验证的结构证据（baseURL、端口、
    请求 id 前缀），而不是字符串相等。
11. **按「最新一行覆盖」处理重复用量行**：WorkBuddy/CodeBuddy 的 `messageId` 重复行靠「后行覆盖
    前行」，但**无 id 的行各自算一次**（§6.3）。这是可接受的妥协，代价是旧历史回灌会重复计数；
    TokenScope 遇到同类结构，应优先找稳定 id，找不到就显式标注不确定性，而不是静默累加。

### 13.4 若要把 magpie 当数据源（可选方向，未验证）

magpie 暴露了 `GET /v1/magpie/usage`（与 GUI `/api/usage` 同构）与 `POST /v1/magpie/usage/requests`
（上报）。这意味着**存在一条低成本集成路径**：TokenScope 若检测到本机装了 magpie，可以在设置页
（手动触发，符合本仓「读取必须由用户点击触发」的约定）拉一次 usage JSON，作为额外的
`reported` 来源导入。代价与收益需要单独立项评估，本文不展开。

## 14. 复核记录与未决问题

### 14.1 本文的复核方式

- 每条结构性结论都指到具体 `文件:行号`，其中被两处以上代码交叉印证的已标「已验证」。
- 与 TokenScope 对照的结论基于 `docs/stats-semantics.md` 与 `CLAUDE.md` 的现行口径，不基于
  记忆。
- 未运行 magpie，因此「页面实际显示什么」类结论一律未验证（前端资源未入库）。
- 第 6 章（本地文件读取）为**逐文件通读**：`call_cache.go`（739 行）全文、`calls.go` 全文、
  `frames.go`、`tail.go`、`revision_cache.go`、`quiet.go`、`thin.go`、`shelf.go`、`one_listing.go`、
  `codex_usage.go`（664 行）全文、`claude_usage.go` 全文、全部 `calls_*.go`（opencode / zcode /
  workbuddy / minimax / kimi / pi / dsh / agy），以及 `sessions.go` 的发现段（374-612、700-950、
  1740-1859）。凡未通读的段落，正文均标「未验证」。
- 第二轮又逐行补读了各家 reader 的**依赖文件**：`pi.go`（`piUsage`/`piLine`/`piParse`）、
  `workbuddy.go`（`wbLine`/`wbDetails`/`workbuddyLine`）、`claude.go`（`claudeLine`/`ccReply`/
  `objectEnd`/`ccLink`/`ccRandomUUID`）、`codex.go`（`codexHead`/`cxUsage`/`spent`/`codexSource`）、
  `codex_discovery.go`（`distinctCodexFiles`/流式比对/ctime 缓存）、`opencode.go`（`ocStore`/`ms`）、
  `grok.go`（`numTS`），以及 `sessions.go` 的 helper 段（`scanBuffer`/`lineReader`/`openLines`/
  `tsAt`/`title`/`dateOf`/`use`/`unuse`/`distinctPaths`/`Dirs`）。本轮新标出的未通读项见 §14.2
  第 13、14 条（`ccCopies` 与 `readAgyCalls` 函数体）。
- 第三轮把最核心的四段原文重新取出并逐行核对：`calls.go:632-766`（`ccCall`/`claudeCallLine` 全文，
  含窗口过期重建与 `blockID` 双形态）、`codex.go:69-93`（`cxUsage`/`raw`/`spent`）、
  `codex_usage.go:388-440` 与 `:620-646`（`record()` 入口与 `count()` 收尾）、
  `call_cache.go:456-482` 与 `:592-639`（`callPatch`/`callFrame`/`framePatches` 与 `loadCalls` 校验）。
  结论：前两轮的行号与语义均成立，本轮另补入三处此前遗漏的实现（见 §6.3 与 §6.5）。
- 第四轮（第 15 章）改为**子代理分片复核**：5 个只读子代理按文件组并行通读，各自输出带
  `文件:行号` 的报告，由本文整合。分片表见 §15 开头；子代理标为「未验证」的条目一律原文保留在
  §15 各节末尾，不由本文代为推断。另有一条枢纽事实（`parserFor` 分发表）由本文一手读出并先写入
  §15.9，避免与子代理结论冲突时无从判断。

### 14.2 未决问题

1. **`plugins/usage` 的前端**（`client.js` / `page.js`）不在本仓，页面级的聚合/排序/展示口径
   无法核实。本文的聚合结论全部来自后端 JSON（`usageapi`）。
2. **`Usage.add()` 的「非零覆盖」在增量型厂商字段上是否会错**：代码只对 `Deferred` 有显式处理，
   其余依赖「同请求重复上报累计值」的假设。未见针对「上游发送增量而非累计」的防御（**未验证**
   是否有厂商如此）。
3. **`chat.go` 的 `whole` 启发式**（`prompt_tokens >= cacheRead + cacheWrite`）在「部分命中 +
   部分写入 + 上游已把 prompt 归一」的情形下可能判错。代码没有回退或标记传播到 `Record`，
   因此无法事后审计。这是 magpie 侧的一处已知脆弱点。
4. **`PriceRate` 与 `Tiers` 的交互**：`priceOf` 在取到价后统一 `Times(PriceRate)`
   （`models.go:2225-2227`），`Times` 会连档一起缩放（`catalog.go:281-287`）；未见「倍率只作用
   于基础价」的开关。
5. **跨机同步的去重**：`sharedRecords()` 只按时间过滤，未见「同一调用被两台机器各自记录」的
   防护。注释只说「a call another magpie passed on here is left to that computer」
   （`shared.go:105-108`），依赖 `Via` 字段，边界**未验证**。
6. **热力图与 `all` 区间的内存上限**：`heatmapOf` 复用 `QueryPage` 的块缓存，
   `request_page.go` 有 `requestDiskLeastRows` 与 `trim(bound)`，但 `bound` 的取值来源未逐行确认。
7. **models.dev 的真实形态**：仓库里只有 fixture（`price_tiers_test.go:17-21`）。真实的
   `canonical_model_id` 条目是否同时带上下文分档与 1 小时缓存写价，**未验证**。
8. **UA 为空的 agent 如何归属**：约 20 余个内置 agent 没有赋 `UA`（子代理逐个核对 `UA:` 字段后
   标注），它们要么靠专属 key、要么靠宿主信息，网关侧无法按 UA 区分——具体清单与影响面
   **未验证**（本文 §12.4 因此按「能读到本地历史 / 只能经网关 / 只能上报」三档归类，不按 UA 归类）。
9. **`calls-v15` 的版本史缺 v11**：`call_cache.go:23-46` 从 v10 直接跳到 v12，v11 改了什么没有记录。
   不影响当前行为（旧版本目录一律作废），但「哪一次口径变更对应哪个 issue」无法完整复原，
   **未验证**是否有意省略。
10. **`minimaxCallFiles` 未走批量 stat**：它用 `walkLinked` 逐文件 `stat()`（`calls_minimax.go:53-70`），
    而 Claude/Codex 走 `statFiles` 的「一次列目录 + 并行 stat」。messages 目录很大时的成本
    **未验证**（作者显然知道这套优化，可能是遗漏）。
11. **WorkBuddy 无 `messageId` 的历史行**：注释说「各自算一次调用」（`calls_workbuddy.go:155-163`），
    但在「回灌历史 + 网关记录」并存的场景下是否会重复计数，`calls_workbuddy_test.go` 未见对应用例，
    **未验证**。
12. **Codex 副本比对的缓存淘汰是随机的**：`maxCodexCopyPairs = 4096`，满时
    `for old := range codexCopyCache.Pairs { delete(...); break }`（`codex_discovery.go:159-163`）——
    Go 的 map 遍历顺序随机，等于不确定淘汰。多个 `CODEX_HOME` 下 rollout 很多时是否会退化成反复
    流式比对，**未验证**（没有实测数据）。
13. **Claude fork 复制的 calls 侧去重未闭环核实**：汇总侧有 `ccLink`/`Root`/`Leaf` + `ccCopies`
    识别「被复制进新文件的会话」（`claude.go:119-167`）；子代理 B 确认了 `ccLink`/`link`/
    `ccRandomUUID` 与跨文件去重靠 `Msgs` 的 msg id 全局去重（`calls.go:266-271`，文件按 mod 升序
    先到先得），但 **`ccCopies` 函数本身仍未逐行读**，「fork 复制在 calls 侧除了 `Msg` 去重之外
    是否还有第二道防线」仍未验证。
14. ~~`readAgyCalls` 的函数体未逐行读~~ **已解除**（第四轮）：子代理 D 读到了 `calls_agy.go:61-114`
    全文——无 token 步骤由 `"output_tokens"` 预筛加指针门槛双重挡掉且不留错误行；已见 step
    整体替换（后写覆盖先写）；模型与账号留空；时间用 `created_at`（步骤开始时刻、秒级）。
    详见 §15.8。

## 附录 A：关键常量速查

| 常量 | 值 | 位置 | 含义 |
| --- | --- | --- | --- |
| `logBlockRows` | 512 | `usage/log_index.go:23` | 索引块行数 |
| 压缩块开销 | ≈80 B/调用 | `usage/log_index.go:31-34` | 注释给出 |
| `logStampSettle` | 1s | `usage/log_index.go:24` | 整秒时间戳沉降 |
| `logFineSettle` | 100ms | `usage/log_index.go:28` | 毫秒级时间戳沉降 |
| `MinDecodeMs` / `MaxDecodeSpeed` | 100 / 10000 | `usage/usage.go:566-567` | 速度有效性 |
| `HeldShare` / `ThinkPace` | 4 / 20 | `usage/usage.go:647-648` | 爆发式推送判定 |
| `CodexWindowMs` / `CodexBodyTokens` | 1000 / 100 | `usage/ledger.go:806-807` | Codex 速度下限 |
| `SeriesKeep`（`seriesKeep`） | 24 | `usage/ledger.go:1048` | 图表保留系列数 |
| `HeatmapWeeks` | 53 | `usage/heatmap.go:17` | 热力图周数 |
| `SharedDays` | 90 | `usage/shared.go:30` | 跨机共享保留天数 |
| `PageSessions` | 100 | `usageapi/usageapi.go:72` | 页面会话上限 |
| 分页上限 | 500 | `usageapi/usageapi.go:303-305` | 明细页 |
| `staleAfter` / `missingAfter` / `freshEvery` | 24h / 6h / 1h | `catalog/fresh.go:19-28` | 目录刷新 |
| `fx.TTL` / `fx.Fallback` | 12h / 7.2 | `fx/fx.go:37-43` | 汇率 |
| 网关 body 上限 | `8<<20` / `1<<20` | `gateway/sniff.go:39,75` | 整包/单事件 |
| `agyStepLongest` | 30min | `usage/request_page.go:1278` | agy 配对时间窗 |
| 兜底配对时间窗 | ±2s | `usage/request_page.go:1398` | 其余 agent |
| Claude 1h 缓存写价 | 2× 输入价 | `catalog/catalog.go:272` | `OneHourFor` 补齐 |
| 外部价合法性上限 | 1e6 USD/M | `catalog/live.go:879` | `sane()`：NaN/负数/超限/档 `Above<=0` 均拒 |
| 目录响应上限 | 64MB | `catalog/catalog.go:760`、`labs.go:80` | models.dev 同步 |
| 实时列表分页 | 20 页 × 1000 | `catalog/live.go:411-415` | 另 8s 超时 / 8MB |
| 上下文窗口下限 | 2048 | `catalog/catalog.go:466` | 小于此视为未知 |
| Effort 等级序 | none→minimal→low→medium→high→xhigh→max→ultra | `catalog/catalog.go:1825` | `NearestEffort` 平局向上 |
| 预算在途预留 | body 字节 / 4 + 均值输出 | `budget/budget.go:20-23` | 4 字节 ≈ 1 token |

## 附录 B：模块与入口索引

| 关心什么 | 从这里读 |
| --- | --- |
| 一次请求怎么被抽取 | `internal/gateway/sniff.go:98-236` |
| 四种协议 usage 归一 | `gateway/{chat,anthropic,responses,gemini}.go` 的 `*.usage()` |
| 统一 Usage 结构 | `internal/gateway/ir.go:373-502` |
| 一行账本的全部字段 | `internal/usage/usage.go:29-232` |
| 落盘与句柄复用 | `internal/usage/usage.go:254-340` |
| 索引与增量 | `internal/usage/log_index.go:98-279` |
| 会话文件读者注册表 | `internal/sessions/call_cache.go:205-251` |
| 会话文件发现与并发 | `internal/sessions/calls.go:190-348` |
| 去重 | `internal/usage/request_page.go:1284-1446`、`usage/codex_identity.go:43-68` |
| 合并成 ledger | `internal/usage/ledger.go:415-505` |
| 直连来源单独汇总 | `internal/usage/direct.go:11-115` |
| 汇总 | `internal/usage/usage.go:942-1121` |
| 时间口径 | `internal/usage/{ledger,period_range,heatmap}.go` |
| 计价 | `internal/catalog/catalog.go:181-334`、`internal/provider/models.go:2187-2229`、`internal/usage/ledger.go:522-551` |
| 换模型判定 | `internal/usage/served.go` |
| 预算 | `internal/budget/budget.go:1-175` |
| HTTP/CSV/OTLP | `internal/usageapi/usageapi.go`、`internal/usage/ledger.go:568-623`、`internal/otlp/otel.go` |
| 支持的 agent | `internal/agent/agents.go:96-224`、`internal/agent/agent.go:117-180` |

## 15. 逐文件实现清单（子代理分片复核）

本章由 5 个只读子代理按文件组并行复核后整合而成，全部结论带 `internal/sessions/文件:行号`。
分片与主理的对应关系：

| 分片 | 覆盖文件 | 对应本章小节 |
| --- | --- | --- |
| A 公共骨架与缓存 | `calls.go`、`call_cache.go`、`call_frame.go`、`call_revisions.go`、`call_continuation.go`、`one_listing.go`、`quiet.go`、`shelf.go`、`discovery.go`、`frames.go`、`tail.go`、`thin.go`、`revision_cache.go` | §15.1–§15.4 |
| B Claude 与 Codex | `calls.go` 的 claude/codex 段、`claude_usage.go`、`claude.go`、`codex_usage.go`、`codex.go`、`codex_discovery.go` | §15.5–§15.6 |
| C OpenCode/ZCode/WorkBuddy | `calls_opencode.go`、`opencode.go`、`calls_zcode.go`、`zcode.go`、`calls_workbuddy.go`、`workbuddy.go` | §15.7 |
| D MiniMax/Kimi/Pi/omp/dsh/agy | `calls_minimax.go`、`calls_kimi.go`、`calls_pi.go`、`pi.go`、`omp.go`、`calls_dsh.go`、`dsh.go`、`calls_agy.go` | §15.8 |
| E reader 分发与汇总 | `sessions.go` 其余部分、`stats*.go`、`alias.go`、`named.go`、`rollup.go`、`carry*.go` | §15.9 |

> 以下小节内容待子代理返回后回填。占位中。

### 15.1 数据模型与调用链

**数据模型**（子代理 A）：`Call`（`calls.go:23-64`）分四组——身份（Agent/Session/ThreadID）、
模型三元组（Model/Requested/Effort）、Tokens+Reasoning、时序与错误；`callFile`
（`calls.go:68-103`）分三组——续读状态（`Off/Size/Mod/Head/ContentHash` + 各 agent carry）、
已产出数据（`Calls/Msgs/Began/Strs`）、携带态（`Claude/CX/Pi/Dsh`）。一会话多文件由
`file.key = agent:session id` 表达（`sessions.go:382`），Claude 的主文件、subagents、workflows
三处共用同一 key（`sessions.go:584/592/604`）。

**调用链**：`Calls(since)` → `sharedCalls`（`calls.go:190-231`）按
`callsKey{since.UnixNano(), zero, agent}` 做并发合并（同 key 等同一个 flight），内部
`callsReading` 全局互斥保证「同一时刻只有一次全量读」；`callsFor`（`:233-287`）按 mod 升序排
文件（`:241-246`，较早文件拥有被续接复制的消息）、**文件级 since 用 `f.mod` 预筛、call 级
since 用 `c.Time` 精筛**、按 `Msg` 全局去重先到先得（`:266-271`）；结果按 Time 降序 → File
降序 → To 降序（`:277-285`）。

### 15.2 `readCalls` 分支树与续读条件

`readCalls`（`call_cache.go:261-374`）的完整分支树：

```
fnv(path) % 64 文件锁
→ 内存命中（Size+Mod 相等且 generation 未变）
→ loadKept 读磁盘分片（owned=true 时整片归本读者）
→ f.cold / wslCold → 不打开文件
→ resumeCallReaders（dsh）→ writeCalls(st, kept, kept > 0)
→ wholeCallReaders（6 agent）→ writeCalls(st, 0, false)   ← 整份重写
→ lineCallReaders / codex / claude 续读 → writeCalls(st, start, continued)
```

三条路的差别只在 `writeCalls` 的两个参数：`start`（旧调用里要重写的脏行起点）与
`appendOnly`（是否 O_APPEND）。续读路径的 `st.ContentHash` 用**实读到的 `off`** 而非列表
size（`:355`）——列表可能把别的文件的大小折进来（Kimi 的 `state.json`），哈希越过文件尾会
留下无处续读的哈希。

`prepareCalls`（`calls.go:388`）续读条件 = 两个前置丢弃（codex 缺 `CX.Usage`、claude 缺
`Claude` → old 置 nil）+ 六条布尔：`old != nil`、`!packed(f.path)`、`f.size >= old.Size`、
`old.Off <= f.size`、`sameHead`（头 256 字节哈希一致）、`prefixHash == old.ContentHash`；
`owned` 参数只决定 `clone`/`cloneState` 的深拷贝深度（是否复制 Calls 切片）。

### 15.3 分片 wire 格式、校验与配额

帧 = `[4B 小端长度][4B CRC32][gob(callFrame)]`；13 个文本字段由 `callText`
（`call_cache.go:533-535`）定序映射到 `frame.Strings` 字典（`Agent/Session/ThreadID/Model/
Requested/Effort/Cwd/Upstream/AccountID/UserID/Error/ErrorText/File`），**`File` 恒写空串
（下标 0）**，读取时由 `State.Path` 补上——所以「File 由 State.Path 隐含」是 wire 层的硬约束，
不是优化建议。`More` 标记同批写的后续帧（读到 `More=true` 结尾 = 写到一半被截断，整片作废）；
`Calls` 字段让读取端按最终调用数一次性扩容（避免 append 翻倍，热启动曾为此多付 ~500 MB）。

`loadCalls` 的校验链（`:561-644`）共十类：size 为 0 或 > 256 MiB、CRC、decodeFrame 失败、
`State.Path`/`State.Agent` 不符、`Off` 倒退、`Index` 越界、`Text` 下标越界、以 `More=true`
结尾、`st == nil`。

配额与清理（`keepCalls`/`pruneCalls`）：内存 `maxKeptCalls = 131072` 条 / `maxKeptFiles = 64`
文件；孤儿判定 = `.gob` 无对应源文件 / `.rev` 无对应 `.gob` / `calls-*.tmp` 超 1 小时；
**版本目录变化时 `Glob(calls-v*)` 删除全部旧目录**（`:142-150`）。

### 15.4 并发节流、长行裁剪与帧续读

**`callsRide`**（`call_cache.go:394-454`）：仅 `codex + packed + 非 cold` 时创建；先持
`callLock` 让并发的 `readCalls` 排队等 ride 的结果；`release()` 只解锁（`parseWith` 的
defer），`finish(off, err)` 落盘——**`err != nil` 不写**（下次 readCalls 自己重读），且恒
`writeCalls(st, 0, false)` 全量写（ride 是从头解压的，没有增量可言）。

**修订窗口预算**（`revision_cache.go`）：`eligible` 分两叉——Claude 是 bounded（只看权重
`<= claudeRevisionEntries`），Codex 是 recent（5 分钟内变过且权重受限）；`retainedRevisions`
排序 working（1 小时内）→ size 降 → mod 降 → path；两个分支——共享 `revisionEntries = 8192`
预算，或 Codex 活动会话走 `revisionActiveFiles = 2` 独立配额（每个上限 16384）。
`trimCallRevisions` 的一个不对称：对 `callCache` 用 `withoutRevisions()` **换快照**（锁外
读者安全），对 `callContinuations` **直接删除**。

**quiet 与 shelf**：`quiet() = warming > 0 && asking == 0`；`readSlot` 在 quiet 时抢
`quietReaders = max(1, CPU/4)` 个槽位、50 ms 轮询让出；`shelf` 与 `sync.Pool` 的差异是
**跨 GC 保留 4 个、空闲 10s 清空**——冷启动约每秒一次 GC，`sync.Pool` 在头 2 分钟被清空
611 次、重复分配约 1.9 GB。

**长行裁剪**（`thin.go`）：`thinKeep = 4 KiB`；`thinner` 是跨 chunk 状态机（`in/cut/n/at/esc`），
`cuts` 是策略点——`urls=false` 全裁（Claude 工具结果），`urls=true` 只裁 `data:` 前缀
（Codex 只裁图片，文本保留）。

**zstd 帧续读**（`frames.go`）：magic `0xFD2FB528` / skippable `0x184D2Ax0`；
`frameHeaderLen` 按帧描述符位域解（single-segment、window、dict id、content size 四种组合）；
`blockLen` 解 size/last/type（RLE 块 size=1，type 3 保留值直接报错）；
`scanFrames` 返回 `next/nextSkip/end` 三值——「帧恰好读尽」`next = 帧末`，「帧内有半行」
`next = 该帧起点、skip = 已消费字节数`，跨帧残余进 pending 拼行。

**`tail.holds`**（`tail.go:41-50`）四条判定：文件未缩水、`Off <= Size`、头部哈希一致、
`prefixHash(path, Size)` 与记录一致。

（A 组未验证项，原文保留：usage 层如何渲染 `Requested`/`Compact`；`f.cold` 的置位方；
`keepCalls` 与 `keepCallContinuation` 的淘汰是否同步；`prefixHash` 采样漏检的真实影响；
`scanFrames` 两种失败路径对下次续读起点的差异；dsh 失败路径是否可能写无 tail 状态。）

### 15.5 Claude Code / Claude Desktop

本节由子代理 B 全文通读 `calls.go` 的 claude 段、`claude_usage.go`（322 行）、`claude.go`（436 行）
后整合，并由本文交叉核对。

**双读者架构（B 组核心结论）**：`claudeLine`（汇总用量，`claude.go:68`）与 `claudeCallLine`
（逐次调用，`calls.go:663`）**各自持有一份 `claudeUsageState`**，靠 `message(id, request)` 的同一套
分支规则保持等价；`claude_usage_test.go:92-127` 的身份矩阵同时验证两条路径的调用数一致。

**行路由（`claude.go:68-115`）四分支**：① 行首 `{"type":"` 为 `ai-title`/`custom-title`/`summary`
→ `claudeFull`；② `attachment` 在 `message` 之前 → 只取时间戳 + `ccWhere`；③ 无 `"message":{`
→ `claudeFull`；④ 有 message 时按其首字段分流——`model` 开头且 `role:assistant` → 快路径
`ccReply`；`role` 开头且 `role:user` → 是 tool_result 就只记时间，否则 `claudeFull`（提示词要全解）。

**`ccReply`（`claude.go:279-311`）只解 usage 不解 content**，三处细节：

- 顶层 `requestId` 用 `gjson` 取（`:283`），注释明确「工具输入里也可能含 requestId，绝不能成为
  该响应的身份」。
- usage 用 `bytes.LastIndex(b, ccUsage)` 定位（`:291`）——行内更早处可能有嵌套的 `"usage":{`
  （content 里的对象），**行尾那个才是消息级 usage**。
- `objectEnd()`（`claude.go:250-275`）手写括号匹配切边界；`ccTools` 只在 usage 之前搜工具块
  （`:295`），且逐块跨越整对象（`:236`），防止工具输入里的嵌套 tool 形状被重复计数。
  `strAt`（`:181-200`）同理用 LastIndex 取 `cwd`/`sessionId`——嵌套结构里它们会出现多次，
  最后一个是顶层字段。

**状态机（`claude_usage.go`）**：

- `message(id, request)`（`:123-158`）：两者皆空返回 `{Call: -1}`；key 为 `message:<id>` 或
  `request:<request>`；「唯一已知分支收养」仅当 `len(branches) == 1`——缺 request 的行归并到
  唯一分支，或把空 request 分支**升级为具名**（`:147-152`，复用同一指针）。
- `accept`（`:202-228`）四条返回路径：同 model+tokens → 只推进 `Updated`（后续块常重复同一
  usage，时间戳仍约束修订序）；命中 `Previous` → 忽略（历史快照复现不算新修订）；时间不更晚 →
  忽略（旧快照不能撤销较新修订）；否则旧值入 `Previous`、新值落位、返回 changed。
- `finish`（`:163-189`）：权重 `2 + len(Previous) + len(Tools) + len(Blocks)` + 各 tool 的
  Previous；单条超长消息超窗时**只保留其最新贡献**（`:168-173`），然后 LRU 淘汰到
  `claudeRevisionEntries = 1024` 以内。
- 窗口持久化走 `.rev` 文件（`call_revisions.go:32-55`）：gob 编码
  `callRevisions{Off, ContentHash, CompactMs, Window}`，临时文件 + rename 原子替换；
  `withCallRevisions`（`:59-74`）校验 `Off` 与 `ContentHash` 一致才采用。冷重建条件在
  `prepareCalls`（`calls.go:377-384`）：codex 缺 `CX.Usage`、claude 缺 `Claude` → old 置 nil 强制
  整读。

**`claudeCallLine` 的收尾三分支（`calls.go:794-834`）**：`accept` 未变时，补 `RequestID` 进行内；
「新块完成回复」的条件是 `newBlock && 同 model && 同 tokens && at 更晚 && **同一天**`——
不跨日移动用量（`:802-806`）。changed 且已有行 → 保留 `From`、覆写整行。全新 → 登记
`Msgs`/`Began`，把待发 `CompactMs` 贴到本条并清零（`:831-833`）。

**agent 改判**（`calls.go:765-767`）：仅当 `st.Agent == "claude"` 且 `entrypoint == "claude-desktop"`
才改判为 `claude-desktop`（Desktop 的 Code 标签页与 CLI 落在同一 projects 目录）；
**Cowork 不走此改判**——它的文件由 `ccFiles("claude-desktop", …)` 直接列举
（`calls.go:296-302`，glob `local-agent-mode-sessions/*/*/local_*/.claude`）。

**发现层的 key 归属（`sessions.go:573-612`）**：`projects/<project>/<id>.jsonl` 为
`main: true`，key = 文件名去后缀；`<id>/subagents/*.jsonl` 与
`<id>/subagents/workflows/<run>/agent-*.jsonl` 的 key 都是**会话目录名**（同一 session id），
不设 main；`journal.jsonl` 因不匹配 `agent-` 前缀被过滤（`sessions.go:601`）。

**B 组指出的两处实现细节（原文保留）**：`calls.go:737` 的 `if blockID != ""` 因 `:725` 初值
必含 `"uuid:"` 前缀而**恒真（防御不可达）**；`defer st.Claude.finish(m)`（`:708`）与 `:716` 的
重新赋值依赖 `message()` 升级分支复用同一指针（`claude_usage.go:147-152`）——隐式耦合，
测试未直接覆盖。

### 15.6 Codex

本节由子代理 B 全文通读 `codex_usage.go`（664 行）、`codex.go`（760 行）、`codex_discovery.go`
（228 行）后整合。

**命名澄清（B 组修正，重要）**：`internal/sessions/codex_identity.go` **不存在**——会话身份推导
在 `codex.go:188-498`（`CodexIdentity` 结构体在 `sessions.go:110-115`）。本文 §7.4 引用的
`internal/usage/codex_identity.go` 是 **usage 包**的另一个文件（`SessionRole`/`Subagent` 行级
标注与 response_id 配对），两者重名但职责不同：sessions 包管「这个 thread 是 main 还是 subagent、
父是谁」，usage 包管「这一行在页面上算谁的会话」。

**`cxUsage` 与 Known 位图**（`codex.go:69-76` + `codex_usage.go:230-257`）：五字段
`Input/Cached/CacheWrite/Output/Reasoning` + `Known uint8`；位序 **Input=bit0、Output=bit1、
Cached=bit2、CacheWrite=bit3、Reasoning=bit4**（`UnmarshalJSON` 按
`values := [5]*int{Input, Output, Cached, CacheWrite, Reasoning}` 平行遍历），全知为 31。
`valid()` 要求非负且 `Cached+CacheWrite <= Input`；`matches()` 要求双方 `Known&3 == 3`
（Input/Output 必须已知），可选字段缺失不证明为 0。`spent()`（`codex.go:87-93`）做
`Input -= CacheRead + CacheWrite`（#589），`Reasoning` 不从 Output 拆出（已在其中）。

**`codexUsageState` 全字段**（`codex_usage.go:14-34`）：`Pending`（最后一条未配对观测的
index+1，输入/回合边界清零）、`Entries`、`Intervals`（区间键倒排）、`Responses`
（`session\x00responseID` → index）、`High`/`HighAt`（计数器高水位）、`Epoch`（计数器域代数）、
`Models`（turn → model）、`Conflicts`、`Compactions`、`CounterBases`（重置基线）、
`SnapshotAt/Thread/Total/Offset`（分页子代理历史快照四元组）。

**`record()`（`codex_usage.go:388-495`）**：入口 `pending := s.Pending; s.Pending = 0`；
`projectCount` 前置（`:405`）；Responses 命中后四条路径——①内容完全相同：纯重放 return nil，
否则只推进 `Updated`；②时间序拒绝：旧快照不能撤销已接受更新，时间相同即歧义 → `Conflicts++`；
③更晚 → `v.At = old.At` 覆写；④`Intervals` 匹配（`sameCodexInterval` 要求同 Epoch/Session/
相容 Turn/checkpoint 与 Usage 双 `matches`，两个不同匹配即 `match = -2` 计冲突）→
`Pending` 兜底配对（`old.ResponseID == ""` 且 turn/session/usage 全 matches，「一次观测只消费
一次」）→ 配对成功把本地观测的 `Total` 提升为检查点并 `confirmBase`；都没命中才追加，
compaction 不占 Pending 槽。

**`count()`（`:497-646`）**：入口校验 → `alignSnapshot`（分页子代理的 zero-last 检查点对齐，
`:51-70`）→ `last` 半已知计冲突置 nil → `Intervals` 命中（已证明的旧区间不能变成新 epoch；
**Epoch 递增**的完整条件在 `:524-529`，含 `firstAfterReset`——`total == last` 也能证明计数器
是新的）→ `Pending` 命中（把 `High` 直接抬到 `total`，这是本地计数器边界）→ reset 判定
（`!total.covers(prev) || firstAfterReset` → `Epoch++`，并向前的 Confirmed CounterBase 累加
base）→ reset 后 reconcile（只认新域中唯一精确检查点）→ `total.equal(prev)` 早退 →
**Usage 三选一**：`last` 有效用 `last`（即使为 0 也精确），缺失才回退 `total - prev`，再缺失用
`total`（`:631-637`，#2234 的注释在此）。

**`compacted()`（`:116-156`）**：内嵌记录的 response id 必须与外层 `compaction_response_id`
一致且 `Known&3 == 3`；`Compactions[key]` 幂等；`Responses` 命中且 usage 不等 → 冲突；
usage 同时出现在内嵌快照与顶层 RECORD 时只加一次。`projectCount()`（`:158-204`）做两级偏移
——快照偏移 + compaction 偏移（compaction 之前到达的迟响应属于更早的域），再对同 session 的
CounterBase 取 base；无任何偏移证据时不投影。`confirmBase()`（`:206-215`）：token_count 检查点
与投影一致才确认基线。

**行处理（`codexHead`，`codex.go:124-175`）返回值语义：true = 交给 `codexBody` 全解，
false = 行首已处理完。** 两处 `Pending` 清零（`:132-140`）；`session_meta` 只读第一条；
工具调用直接 `s.tool` 记名不进 body，**唯一例外是 `spawn_agent`**（`:159-166`）；
assistant message 行只计 `Replies++`。

**`codexCallLine`（`calls.go:887-1005`）八类行**：`cxAnswerItem` 记应答区间
（`Answering/AskedAt/Answered`）；`session_meta`/`turn_context`/`task_started`/
`thread_settings_applied` 推进上下文（`saveContext` 写 `Contexts[turn]`）；`task_complete`
仅当 `turn.N == 1` 才把 `FirstToken/Duration` 回填那次调用；`compacted`/`token_usage_record`/
`token_count` 各走状态机对应入口。`codexChanged`（`:1021-1097`）构造 Call 时
`change.Old != nil` 的覆写把 `To` 收为 `min(st.Calls[i].To, st.End)`——响应晚于下一次调用的
内容到达时，不得把物理范围扩进那次调用（`:1041-1044`）；`codexTiming` 的 `Millis` 有
**2 小时上限**（`:1111`），单调用回合直接采用 Codex 自报的 `Duration/FirstToken`（`:1114-1117`）；
`codexCompact` 把 compaction 调用的耗时记为等待时长（`:1125-1131`）。

**发现层**：`codexFilesIn`（`sessions.go:785-826`）固定顺序 `sessions` → `archived_sessions`，
先试 `listedRollouts`（dirent 一次列目录 + 容量按上次 +1/16 预分配，`:837-883`），失败回退
`walkLinked` + `statFiles(follow=false)`；明文孪生优先（压缩中 `.zst` 可能还没写 trailer）。
`walkLinked`（`:767-782`）对 junction/symlink 用 `proc.RealPath` 取真实路径，再把回调路径
**映射回 root 前缀**——agent 的配置要从会话路径反推。`distinctCodexFiles`
（`codex_discovery.go:101-130`）按去 `.zst` 文件名分组两两比对，`Equal/SecondPrefix` 丢当前、
`FirstPrefix` 丢已留项；`codexCopyRelation`（`:132-169`）双 stat → 缓存（ctime 早于
`start - 1s` 且四项快照一致）→ 32 KiB 流式比对（一侧先结束则 `io.Copy(io.Discard)` 验证
trailer 再判长短）→ 比对后再双 stat 确认未变。

**身份推导（`codex.go:188-498`）**：`codexSource` 把 `source` 字段解为
`cli/vscode/exec/mcp` → main、`custom` → main、`subagent.thread_spawn.parent_thread_id` →
subagent；`codexIdentityFromMeta` 的冲突早退（父子不一致、main 却有父）；
`codexIdentitySnapshot`（`:419-474`）跨文件合并（冲突记 conflicts），main 的 `RootSession = id`，
subagent 沿 `codexRoot` 上溯（`seen` 防环，父 unknown 或成环返回 ""）。
**`rolloutName` 有两个**：portable 的 `codexRolloutName`（`codex_portable.go:210`）是严格 UUID
形式（portable 改写用），发现层的（`sessions.go:695`）字符集宽松——两者用途不同。

（B 组未验证项，原文保留：两个 `rolloutName` 对同一批真实文件名的判定边界未在测试中验证；
本次为纯静态分析未执行构建或测试。）

### 15.7 OpenCode / ZCode / WorkBuddy / CodeBuddy

本节由子代理 C 全文通读 `calls_opencode.go`、`opencode.go`（552 行）、`calls_zcode.go`、
`zcode.go`、`calls_workbuddy.go`、`workbuddy.go` 后整合。

**store 抽象（`opencode.go:127-131`）**：`ocStore` 三方法——`info(sid)`、`messages(sid)`
（**无固定顺序**）、`parts(mid)`（有序）。三实现：`ocDB`（老表，`:221`，`message` 表无
ORDER BY）；`ocV2DB`（OpenCode 2，`:330`，`session_message` 按 `seq` 排序，且读三列后
**自己重铸成老格式 `ocMessage`**——`type=='compaction'` 继承上一条 assistant 的
model/provider/variant 三元组，`type=='user'` 清空模型与 tokens，`:368-376`）；`ocFiles`
（storage JSON，`:403`，**按文件名排序而非内容序**，`:449-462`）。

**size 指纹的语义（本节最重要的机制）**：一条 SQL 同时拿列表+size+mod
（`opencode.go:239-240`），`size` 由调用方传入——Sessions 列表侧传 `COUNT(m.id)`（`:229`），
**调用侧传 `COUNT(m.id) + COALESCE(SUM(m.time_updated), 0)`**（`calls_opencode.go:56`、
`calls_zcode.go:52`）。原因：OpenCode 的 usage 是**回复进行中原地写回**消息行
（`opencode.go:420-421`、`:466-467`），文件 size/mtime 对原地改写不敏感，所以用
「消息数 + 时间戳和」当会话指纹；`mod` 取 `max(s.time_updated, MAX(m.time_updated))`。

**`readOpenCodeCalls`（`calls_opencode.go:153-202`）**：`.json` 分支（storage）不经 stamp、
不经连接池；`<db>#<sid>` 分支走 `withOCCallDB`——全局句柄池，`provider.OpenReadOnly`
（`mode=ro&_pragma=busy_timeout(3000)`），`SetMaxOpenConns(4)`，空闲 `ocCallIdle = 10s`
后锁内二次确认才关闭（`:95-131`）。逐消息：`Role != "assistant"`、`ocGatewayProvider`
（`magpie`/`dial`）跳过；`ocCall`（`:207-250`）的时间取 completed、零则 created、仍零整条丢弃；
`Effort` 取 `Variant`（`""`/`"default"` 不设）；`Output += Reasoning` 并另存 `c.Reasoning`；
**有 token 但模型为空仍丢弃**（`:241-243` 的返回条件 `c, c.Model != ""`）；token 全零走
`ocError`，kind 为空/`MessageAbortedError`/`aborted` 一律丢弃（`:244-247`）。

**ZCode 差异**：`zcodeDB()` 固定 `~/.zcode/cli/db/db.sqlite`（`zcode.go:28`），
**没有 JSON 分支**（`zcode.go:30-35`）；`readZCodeCalls` 只处理 `<db>#<sid>`；
`ocCall` 里 `Agent:"opencode"` 是硬编码（`calls_opencode.go:216`），`readZCodeCalls` 事后改写为
`"zcode"`（`calls_zcode.go:96`），而 **`ocCall` 的 zcode token 特判读的是 `st.Agent`**
（`calls_opencode.go:231`）——`st` 在进入前已建为 zcode，所以特判生效。模型字段靠
`modelID` tag + Go json 大小写不敏感回退匹配新写法 `modelId`（`opencode.go:84-86`）；
`zcDB` 只覆写 `info`（多读 `title_source`）。汇总侧同一修正在 `opencode.go:513-516`。

**WorkBuddy**：发现用 `walkLinked(projects/)`，`<id>/subagents/*.jsonl` 的 key 归到会话
`<id>` 且 `main: false`（`workbuddy.go:83-86`）；`codebuddyInWorkBuddy` 用 `os.SameFile`
判同一文件夹（链接/大小写不敏感都算真，`workbuddy.go:57-69`，测试三种形态
`calls_codebuddy_test.go:65-114`）。

**`readWorkBuddyCalls` 的替换是双条件**（`calls_workbuddy.go:190-194`）：
`c.Msg == last && st.Calls[n-1].Msg == c.Msg` 才替换数组最后一个，否则 append——
第二个条件是必须的，因为 `workbuddyCall` 对 token 全零行返回 false 不产生 call，
此时仅凭 `c.Msg == last` 会误把上一行（可能是别的回复）替换掉（边界测试
`calls_workbuddy_test.go:176-209`）。`Model = cmp.Or(p.Model, p.RequestModel)`
（`:230`）——**没有汇总侧的 `s.Model`/`"auto"` 回退**（`workbuddy.go:200` 是四级回退），
两路对「有 usage 无模型名」的行会给出不同模型串。汇总侧「覆盖」= `unuse` 四字段
（`Msg/MsgModel/MsgUse/MsgDay`）后 `use` 重计（`workbuddy.go:200-208`）。

**排他判据（C 组补充）**：`buddyGateway` 两步（`calls_workbuddy.go:101-108`）——
①`custom-local:` 前缀剥掉后 `provider.IsGroupRef(id)` 命中即网关，**不依赖任何配置**
（`"group/<id>"` 是只有 magpie 会响应的名字）；②读当前 `models.json` 收
`vendor=="magpie"` 的 id（`ocStamp` 缓存）。注释原文承认后果（`:91-96`）：
「models.json is the user's own configuration, not a record of the call: take magpie's
models out of it and the calls made while they were in stop being read as the gateway's
here」，且判据用 `RequestModel`（`:212`）不判 `Model`。该行为被
`calls_workbuddy_test.go:150-170` 钉住（删 magpie 条目后 `custom-local:deepseek/v4`
不再算网关，`custom-local:group/auto-…` 仍算）。

**变更判定对比（为何 wholeCallReaders 不能续读）**：`ocStamp`（db+wal 的
size+mtimeNano，`calls_opencode.go:67-77`）只做**列举级**缓存；会话级靠 size/mod 指纹。
整读分支 `writeCalls(st, 0, false)`（`call_cache.go:318`）——`start=0` 且
`appendOnly=false` 意味着**整份重写分片、不追加**，整读路径从不设 `st.Off`，偏移续读机制
根本不进入；且 `keepCallContinuation` 只对 codex/claude/claude-desktop 保留续读 continuation
（`call_continuation.go:21`），这四家连内存 continuation 都没有。语义理由分散三处：
原地写回没有可续偏移（`opencode.go:420-421`）、同回复 usage 行重复出现且「最后一行才算数」
（`calls_workbuddy.go:156-157`）、从中间续读会留下不完整的旧值。

（C 组未验证项，原文保留：`calls_zcode.go:28-31` 注释说 zcCallList "Its lock is the one
ocCallList uses"，但代码里是两个各自持锁的包级变量，注释与代码不符，按代码事实陈述；
`resetCalls` 清 `zcCallList`/`wbGateway` 但不清 `ocCallList`（后者经 `resetOCCalls` 单独清，
`call_cache.go:83-93`）；`provider.IsGroupRef` 的 `auto/<model>` 归一是否受 `autoRefsOn()`
开关影响只读到定义；`buddyGateway` 只判 `RequestModel` 不判 `Model`，若某版本 WorkBuddy
只把网关名写在 model 字段会漏判，无 fixture 佐证；两路 Model 回退差异未在测试中对拍。）

### 15.8 MiniMax / Kimi / Pi / omp / dsh / agy

本节由子代理 D 全文通读 `calls_minimax.go`、`calls_kimi.go`、`calls_pi.go`、`pi.go`、`omp.go`、
`calls_dsh.go`、`dsh.go`、`frames.go` 后整合；dsh.go 按 read 口径为 463 行。

**MiniMax Code**（`calls_minimax.go`）：

- 发现：`MiniMaxDir()` 取 `MINIMAX_DATA_DIR` 否则 `~/.minimax`（`:36-42`）；
  `minimaxCallFiles()` 用 `walkLinked` 递归 `v2/sessions`，只收 `messages*.jsonl`
  （历史目录保留的其它世代也在内），`stat` 确认常规文件（`:53-70`）。
- 会话 id：目录名 `-session_` 之后的部分，按 RawURLEncoding → URLEncoding → RawStdEncoding →
  StdEncoding 四种 base64 依次解码，第一个成功且非空者胜，全败退回原始串（`:74-85`）。
- 预筛：整文件读，逐行要求同时含 `"role":"assistant"` 与 `"usage":{`（`:98`、pi.go:152）。
- 记账：`Message` 非 nil、`Role=="assistant"`、`Usage` 非 nil、`Provider != "custom_provider:magpie"`
  （`:47`、`:134-136`）；时间 `ms(Timestamp)`；`Msg = "minimax-code:" + message_id`；
  **无负数校验、无 Requested**。四桶由 `piUsage.tokens()` 恒等映射（`pi.go:107-112`）。

**Kimi Code**（`calls_kimi.go` + `kimi.go`）：

- 发现：`~/.kimi-code`（`KIMI_CODE_HOME` 可覆盖）下 `sessions/*/*/agents/*/wire.jsonl`，
  每 agent 一个文件；key 取会话目录名；`main` 当且仅当 agent 目录名为 `main`；`ses_*` 迁移会话
  同样命中；**main 文件的 size/mod 把同目录 `state.json` 折入**（`kimi.go:61-83`）——
  所以改名/重排 agents 目录能被感知。
- 续读：`lineCallReaders` 中 kimi 的 `kept` 为 `nil`（`call_cache.go:248`），即**无跨行状态**；
  `kimiCallsBegin` 仅设 Session（`:91-93`）。
- 记账：`Type=="usage.record"`、模型不以 `magpie/` 开头（`:64`、`:114-116`）；
  四项 token 任一负即丢弃（`:117-119`）。**四桶是全仓唯一非恒等映射**：
  `Input=inputOther, Output=output, CacheRead=inputCacheRead, CacheWrite=inputCacheCreation`
  （`:129`）。不设 `Upstream`。
- 去重键：`kimiCallKey = kimi:<agent>:<time>:<model>:<in>/<out>/<cr>/<cc>`（`:141-145`），
  因 fork 会逐字节复制父记录（时间都在）。**代价**：同 agent + 同毫秒 + 同模型 + 同四桶的两次
  真实调用会并成一条（key 相等即合并是代码事实；真实数据中是否发生，未验证）。

**Pi / omp**（`calls_pi.go` + `pi.go` + `omp.go`）：

- 发现：`PiDir()` 取 `PI_CODING_AGENT_DIR` 否则 `~/.pi/agent`；`piSessionDir()` 取
  `PI_CODING_AGENT_SESSION_DIR`，未设则读 `settings.json`（经 `jsonc` 容忍注释）取 `sessionDir`
  （`pi.go:45-60`）。两处 glob：`<dir>/sessions/*/*.jsonl` 与 `sessionDir/*.jsonl`；
  文件名 `<time>_<id>.jsonl` 按 `_` 切取 id，无 `_` 跳过（`pi.go:74-91`）。
  omp：`~/.omp/agent`（`omp.go:25-28`），roots = 自身 sessions + `~/.omp/profiles/*/agent/sessions`
  + 非 Windows 的 `$XDG_DATA_HOME/omp/sessions`（`omp.go:33-47`；Windows 上第三处不生效）；
  再 `WalkDir` 递归同名目录收 subagent/advisor 的 `.jsonl`（共享 key，`btw-history` SkipDir，
  `omp.go:108-127`）。
- carry：`piCarry{Header, Cwd, Since, Provider, Model}`（`calls_pi.go:78-83`）；
  `piCallsKept` 判 `st.Pi != nil`——旧分片没有 carry 就整篇重读（`call_cache.go:327-329`）。
- 记账：`session` 行仅 `!Header` 时置 Cwd、`parentSession != ""` 时解析时间存 `Since`
  （`:107-113`）；`model_change` 优先 provider+modelId，空则按 `/` 切 model（`:114-121`）；
  `message` 仅 assistant 且 Model 非空时覆盖 carry（`:122-126`）。`piCall` 五类来源：
  `message` 用消息自带 usage（仅 assistant 用自带 provider/model，`Millis=max(DurationMs,Duration)`），
  `usage`/`model_usage` 用行自身，`compaction`/`branch_summary` 只用 usage、模型沿用 carry；
  `u==nil || model=="" || piGateway(provider)` 任一丢弃；四项负即丢弃；
  **`!c.Time.Before(p.Since)` 才入列——fork 复制的父条目在 calls 侧由此挡掉**（`:127-130`）；
  `Msg = <agent>:<id>:<timestamp 原串>`（`:181-183`）。
- 汇总侧对照：`piParse` 的 fork 判定唯一来源是 `session` 行的 `parentSession`（`pi.go:180-182`），
  `copied = at.Before(s.Since)`（`pi.go:160-168`）——calls 侧与汇总侧共用同一 `Since` 语义。

**dsh（DeepSeek Harness）**（`calls_dsh.go` + `dsh.go`）：

- 发现：`DshDir()` 取 `DSH_HOME` 否则 `~/.dsh`；`dshFiles()` glob `sessions/*/*`，正则
  `^session(?:\.v(\d+))?\.jsonl(\.zstd)?$` 筛名，**版本号大者优先、同版本取 mod 晚者，
  只选一个最新文件**（`dsh.go:50`、`:86-97`）；`dshReadHead` 读首行校验 `Type=="session"` 且 ID
  非空并按 path 缓存（`:101-107`、`:181-193`）；`DelegationDepth > 0` 记 parent（`:110-112`）；
  **key 上卷到顶层根（最多 64 跳防环）**，`main` 判据是无 parent（`:120-127`）。
- 记账：`headed` 初值 = `c.Head.Type == "session"`；未 headed 时首行必须是 session 头，否则整读失败；
  headed 后只放行含 `"assistant/message"` 或 `"session/end-seed"` 的行（`:74-76`）。
  seed 清空三条件 `Inherited && Version >= 2 && IsSeeded` → `st.Calls, kept = nil, 0`
  （`:89-91`，`kept=0` 会触发分片全量重写）；旧格式 `Version < 2` 靠 `seq < SeedLength` 过滤
  （`:94-96`）。
- `dshCall`：`Model = dshServed(src)`（优先 `replayState.response.responseModel`，空则
  `response.model`，与 `src.Model` 相同则置空表示未换模型），为空回退 `src.Model`；
  `Requested = src.Model`；`Upstream = src.Provider`，空则 `replayState.response.provider`；
  四桶来自 `inputTokens/outputTokens/cacheReadTokens/cacheWriteTokens`；
  **不设 Msg、无负数校验**（`:124-166`、`:171-191`）。
- 续读精度是**帧**而非行：`scanFrames` 返回 `next/nextSkip/end` 三值，跨帧残余行进 pending 拼接，
  「帧恰好读尽」时 `next = 帧末`，「帧内有半行」时 `next = 该帧起点、skip = 已消费字节数`
  （`frames.go:213-268`）；`markTail` 把三者写进 `tail{Off, Skip, Size=end, Head, Hash}`，
  `holds` 校验未缩水、`Off <= Size`、头部与采样前缀哈希一致（`tail.go:27-50`）。

**agy（Antigravity CLI）**（`calls_agy.go` 全文 114 行，本轮读到函数体）：

- 发现：`~/.gemini/antigravity-cli` 的 `brain/<id>/.system_generated/logs/transcript_full.jsonl`
  （`agy.go:178-193`）。`agyFilesIn`（`agy.go:195-229`）走 conversation_summaries.db 补列，
  属 Sessions 页路径，不进 calls。
- **上一版标注的「未验证」在此解除**：`readAgyCalls`（`:61-94`）整文件读，无续读、无 carry、
  无 `st.str` 驻留；`at := map[int]int` 记 step_index → Calls 下标，**已见过的 step 整体替换**
  （后写覆盖先写，`:79-86`）。预筛关键字是 `"output_tokens"`（`:57`）——旧版无 token 的行在解码前
  就被挡掉，**不留错误行**；`agyCall` 再用 `Source != "MODEL" || Step == nil || Input == nil ||
  Output == nil` 双保险（`:98-100`），三个 token 字段用指针区分缺失与 0。
- 模型与账号均不记（`:105`），注释明说「模型与 provider 未知」（`:33-37`）；
  时间 `time.Parse(RFC3339, created_at)` 失败即丢弃（`:101-104`），
  **`created_at` 是步骤开始时刻、秒级精度**，与别家「调用结束时刻」不同（`:28-30`）。
  `Msg = agy:<session>:<step_index>`；无负数校验；无本地网关排他。

**网关排他判据对照（D 组排序）**：MiniMax（整串 `custom_provider:magpie`，高）＝
Kimi（前缀 `magpie/`，高）＝ Pi/omp（`magpie`/`dial`，高）＞ dsh（四个名字，中——同名自建
provider 会被误排他，注释自承）＞ agy（无判据，结构性最弱）。

**读取模式归类的根因**（子代理 D 结论，与 §6.2 表格互证）：分界是「一行是否自洽」+「文件能否
被就地改写」。MiniMax 的代复制与 agy 的行重写破坏「一行只写一次」→ 整份读；Kimi/Pi/omp 的纯追加
→ 字节级 Off 续读；dsh 的帧打包让行级 Off 失效但帧级 Off 成立 → `resumeCallReaders` 并额外返回
`kept` 供 `writeCalls(st, kept, kept > 0)` 决定增量或全量（`call_cache.go:295-309`）。

（D 组未验证项，原文保留：dsh 的 `dshSegment`/`dshCacheRecord`/`jsString` 无真实样本比对；
六家 `*_test.go` 未读，「行为符合注释」仅代码自证；MiniMax/dsh/agy 缺负数校验是否会在真实异常
数据中触发，无样本可验。）

### 15.9 `parserFor` 分发表与汇总 reader 差异

**先落一条已由本文一手确认的枢纽事实**（`internal/sessions/sessions.go:1603-1645`）：

汇总侧的分发是 `parserFor(agent) sessionParser`，结构体有三个槽位加一个布尔
（`sessions.go:1605-1612`）：`whole`（整份解析）、`grow`（可续读）、`line`（逐行）、
`claude`（是否走 Claude 兼容路径）。原文注释点名它是
「the source of truth for both parsing and window admission」。

| agent | 槽位 | 函数 |
| --- | --- | --- |
| `hermes` | `whole` | `parseHermes` |
| `alma` | `whole` | `parseAlma` |
| `opencode`、`zcode` | `whole` | `parseOpenCode` |
| `cursor` | `whole` | `parseCursor` |
| `dsh` | `grow` | `parseDsh` |
| `cline` | `grow` | `parseCline` |
| `grok` | `grow` | `parseGrok` |
| `kimi` | `grow` | `parseKimi` |
| `droid` | `grow` | `parseDroid` |
| `agy` | `grow` | `parseAgy` |
| `codex` | `line` | `codexBody` |
| `pi`、`omp` | `line` | `piParse` |
| `workbuddy`、`codebuddy` | `line` | `workbuddyLine` |
| 其余（含 Qoder 两个变体） | `line` + `claude: true` | `claudeLine` |

两个必须记住的结论：

- **这是汇总侧的表，与 §6.2 的 calls 侧注册表（`call_cache.go:205-251`）是两套独立分发。**
  同一个 agent 在两侧可能走不同函数（例如 `codex` 汇总走 `codexBody`，calls 走 `codexCallLine`；
  `workbuddy` 汇总走 `workbuddyLine`，calls 走 `readWorkBuddyCalls`）。两侧口径由各自的测试守住，
  本文在 §15.9 末汇总其差异。
- `default` 落在 `claudeLine`，所以**没有独立 reader 的 agent 仍会被按 Claude Code 的格式汇总**
  （注释原话："A default line reader is Claude-compatible, including both Qoder variants"）。
  但这只影响 Sessions 页汇总，**不影响 Usage 页**——Usage 只认 §6.2 的 13 个 agent。

**两侧为何能共用一次扫描**（`sessions.go:1647-1653`，本文一手确认）：汇总入口是
`parse(f, old)` → `parseWith(f, old, calls *callsRide)`，第三个参数非空时，
**同一次行扫描会同时喂给汇总 reader 与 calls reader**（`callsRide.ride` 把 head/line 各包一层）。
这就是 §6.8「一次解压两次使用」的实现根：不是为了省一次 open，而是为了让两条解析路径
**看到完全相同的字节序列**，从而不出现「汇总算过、calls 没算」的漂移。

（本节其余部分待子代理 E 返回后回填。）

## 附录 C：本地读取代码文件索引

按「谁读哪个文件」列出的完整索引（与 §12.4 的三档清单互补——这里只列**代码路径**）。

### C.1 公共骨架

| 职责 | 文件 |
| --- | --- |
| 统一事件与 reader 注册、分发、续读 | `internal/sessions/calls.go`、`internal/sessions/call_cache.go` |
| 分片帧编解码与修订续读 | `internal/sessions/call_frame.go`、`call_revisions.go`、`call_continuation.go` |
| 目录列举共享 | `internal/sessions/one_listing.go`、`discovery.go` |
| 并发节流 | `internal/sessions/quiet.go` |
| 缓冲复用 | `internal/sessions/shelf.go` |
| zstd 帧续读 | `internal/sessions/frames.go`、`tail.go` |
| 长行裁剪 | `internal/sessions/thin.go` |
| 修订窗口预算 | `internal/sessions/revision_cache.go` |
| 文件发现 / stat / 扫描 helper | `internal/sessions/sessions.go` |
| Codex 副本去重 | `internal/sessions/codex_discovery.go` |
| WSL 读取 | `internal/sessions/wsl.go` |

### C.2 按 Agent

| Agent | 发现与目录 | 行解析（calls） | 差分/状态机 | 汇总解析 |
| --- | --- | --- | --- | --- |
| Claude Code / Claude Desktop | `sessions.go`（`ccFiles`/`listCCFiles`） | `calls.go`（`claudeCallLine`） | `claude_usage.go` | `claude.go`（`claudeLine`/`ccReply`） |
| Codex | `sessions.go`（`codexFiles`/`codexFilesIn`）、`codex_discovery.go` | `calls.go`（`codexCallLine`） | `codex_usage.go` | `codex.go`（`codexHead`/`codexLine`） |
| OpenCode | `opencode.go`（`openCodeDB`/`openCodeDBFilesIn`/`openCodeJSONFiles`） | `calls_opencode.go` | — | `opencode.go`（`ocStore` 族） |
| ZCode | `zcode.go`（`zcodeDB`） | `calls_zcode.go` | — | `zcode.go`（`zcDB`） |
| WorkBuddy / CodeBuddy | `workbuddy.go`（`workbuddyFiles`/`buddyFiles`） | `calls_workbuddy.go` | — | `workbuddy.go`（`workbuddyLine`） |
| MiniMax Code | `calls_minimax.go`（`minimaxCallFiles`） | 同左 | — | — |
| Kimi Code | `calls_kimi.go`（`kimiCallFiles`） | 同左 | — | — |
| Pi / omp | `pi.go`（`piFilesIn`）、`omp.go`（`OmpDir`） | `calls_pi.go` | `pi.go`（`piParse`/`piCarry`） | 同左 |
| dsh | `dsh.go`（`dshFiles`） | `calls_dsh.go` | `dsh.go`（`dshCarry`） | `dsh.go` |
| agy | `calls_agy.go`（`agyCallFiles`） | 同左 | — | — |
| WSL 内的上述 | `wsl.go` | 各 agent 自身 | — | — |
