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
6. 第二来源：agent 本地会话文件（`internal/sessions`）
7. 三源合并与去重：同一调用不能算两遍
8. 聚合：`Totals` / `Summary` / 时间桶 / 维度
9. 计价：价格从哪来、怎么乘、怎么处理未知
10. 金额展示、预算与跨机共享
11. 对外接口与导出
12. 支持的 Agent：三层清单
13. 与 TokenScope 的对照：可抄的与要避的
14. 复核记录与未决问题
- 附录 A：关键常量速查
- 附录 B：模块与入口索引

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

## 6. 第二来源：agent 本地会话文件（`internal/sessions`）

会话文件在两个地方被消费，必须分开看（**已验证**）：

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
    Session    string    // agent 发给网关的会话头
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

### 6.2 读者注册表（权威清单）

`internal/sessions/call_cache.go:205-251` 是**唯一权威**的「能读哪些 agent」表。代码注释写死：
「Every agent in `callSources` that is not in `callFiles` needs an entry here, in
`lineCallReaders` or in `resumeCallReaders`, or its calls are never read.」

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

### 6.3 各 agent 的解析要点（只记口径差异，不记内容）

- **Claude Code**（`calls.go:632-835`）：一个 `message.id` 可能有多行（流式块各占一行），
  以**最后一行**的 usage 为准（`m.accept`）；`isApiErrorMessage` 行算错误、无 token；
  `attachment.identity.modelId` 提供 `Requested`（如 `claude-opus-5[1m]`）；`compact_boundary`
  的 `durationMs` 记到下一次调用上。耗时用「上一次 user 行 → 本次回复行」近似，注释承认会「多
  一点或少一点」。
- **Codex**（`calls.go:852-1131`）：rollout 里 `token_count`（`info.total_token_usage` +
  `last_token_usage`）与 `token_usage_record` 都是**累计或差分**，由 `codexUsageState` 做差分，
  `spent()` 转换（`internal/sessions/codex.go:87`）；`turn_context` 推进 model/effort；
  `compacted` 标记压缩轮次；`task_complete` 给出整轮 `duration_ms` 与 `time_to_first_token_ms`，
  仅在「一轮只有一次调用」时回填到该调用。
- **OpenCode**（`calls_opencode.go:17-63`）：一条 assistant 消息 = 一次调用；走 magpie 网关的
  记录（provider 为 `magpie` 或旧名 `dial`）**直接跳过**，不做后续配对。
- **WorkBuddy / CodeBuddy**（`calls_workbuddy.go:19-48`）：用量在回复行
  `providerData.usage`，以 `providerData.messageId` 标识；一行可对应一次调用。
- **dsh**（`calls_dsh.go:9-53`）：`assistant/message` 行带 usage，一行一次；文件只追加，
  从上次偏移续读（帧级）；走网关的调用跳过。
- **Kimi Code**（`calls_kimi.go:15-50`）：`usage.record` 一行一次；`usageScope: "session"`
  表示轮次外的调用（压缩）；fork 会话会**原样复制**父会话记录，故按
  `(agent, time, model, usage)` 生成 `kimiCallKey` 去重。
- **agy**（`calls_agy.go:16-46`）：transcript 的 `PLANNER_RESPONSE` 步骤带 `input_tokens` /
  `cache_read_tokens` / `output_tokens`；无模型、无账号，所以「模型未知」；走网关的步骤靠
  token+时间匹配（§7.3）。

### 6.4 增量与缓存工程

- 会话文件变更判定：`prepareCalls()`（`calls.go:374-409`）要求满足「文件只增长、头部未变、
  前缀哈希一致」才续读，否则整份重读；`prefixHash()`（`calls.go:1138-1172`）用 **16 × 4 KiB
  采样**代替全文哈希，注释写明「重写若落在采样窗口之外会漏检」，是有意的成本取舍。
- 并发：`Calls()` 走 `sharedCalls`（`calls.go:190-231`），同一 `(since, agent)` 的并发读者共享
  一次飞行（flight），且全局 `callsReading` 保证「同一时刻只有一次读文件」——注释记了事故：
  Sessions 页多个请求同时进来各持一份全量调用，「进程涨到 85 GB」。
- 目录列举：`readDirectory()`（`discovery.go:33-56`）按目录 mtime+size 缓存，且对「2 秒内的
  列举」不信任（`racyDirectory`），防止「同刻新增条目被漏掉」。
- 磁盘分片：`ReadCallSource`（`calls.go:1204-1225`）读单个来源后立刻 `keepCallContinuation` +
  释放完整行，避免「解析结果与磁盘分片双份常驻内存」。
- 冷启动保护：肌 `f.cold || wslCold` 时不打开文件（会启动 WSL），直接返回旧快照或空。

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
