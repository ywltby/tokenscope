# Qoder CN 会话存储与用量计量逆向（候选适配器前置研究）

> 只读核验：全部结论来自对**本机已安装程序**（`Qoder CN` 1.1.66 / worker runtime `f08e6f7`）
> 与**本机真实数据** `~/.qoder-cn` 的静态与动态分析，未写入、未移动任何被扫描数据。
> 文档只保留结构化字段、算法与聚合计数，不含真实会话路径、提示词、工具输出或用户内容；
> 示例中的路径一律符号化。探针脚本只在临时目录运行，不入库。

## 1. 结论摘要

1. Qoder CN 的会话转录存放在 `~/.qoder-cn/projects/<项目 slug>/<sessionId>.jsonl`，是
   Claude Code 血统的**逐行追加 JSONL**。
2. **转录里的 token 计数是被客户端主动抹零的**，不是服务端不返回。抹零函数为
   `dv(usage, {exposeTokenCounts})`，仅保留 `context_usage_ratio`。
3. 真正的会话级用量账本存放在**加密的 `projects/<slug>/<sessionId>/state.json`**，
   算法为 AES-256-GCM + **硬编码主密钥**，AAD 由 `sessionId` / `projectHash` / 段键构成。
4. `context_usage_ratio` 与解密后的 `total.input_tokens` **精确闭合**
   （4/4 会话、0 差额，含 767 条请求记录的长会话）→ **主链**逐请求 input token 可从
   **明文**转录无损还原。**闭合仅限主链**：子代理请求不在 `total` 内，其转录也没有
   可用的窗口值（见第 6 条与 §6 复核注）。
5. Qoder 的 `input_tokens` 沿用 OpenAI 语义，**包含** `cache_read_input_tokens`；
   接入前必须做桶归一，否则与 Claude / Codex 口径混算会双计。
6. **本计划选定实现路径：解密 `state.json` 取会话级四桶权威值，配合转录
   `context_usage_ratio` 还原逐请求 input 明细；子代理是必须单独处理的第三块。**
   见 §8。

## 2. 存储布局

| 路径 | 内容 | 明文 |
| --- | --- | --- |
| `projects/<slug>/<sessionId>.jsonl` | **主转录**，逐行追加 | 是 |
| `projects/<slug>/<sessionId>/state.json` | **会话用量账本**（`items.<段键>`） | 否，AES-256-GCM |
| `projects/<slug>/<sessionId>/compression-v2/state.json` | 压缩状态（已用 function response id、替换决策） | 是 |
| `projects/<slug>/<sessionId>/subagents/agent-<agentId>.jsonl` | 子代理转录 | 是 |
| `projects/<slug>/<sessionId>/subagents/agent-<agentId>.meta.json` | 子代理元数据 | 是 |
| `projects/<slug>/<sessionId>/subagents/task-<taskId>.json` | 子代理任务结果 | 是 |
| `tasks/<sessionId>/N.json`、`.highwatermark`、`.lock`、`.task-locks/N.lease` | 待办任务与租约 | 是 |
| `logs/sessions/<slug>/<sessionId>/segments/<ts>-<rand>-p<pid>.jsonl` | CLI 遥测事件流 | 是（token 同样抹零） |
| `logs/runs/<run_id>/manifest.json` + `qodercli.log` | 进程级运行记录（manifest 含完整 argv） | 是 |
| `file-history/<sessionId>/<uuid>@v1` | 文件检查点 | 是 |
| `host-actions/result-index/*.json` | 宿主动作结果索引 | 是 |
| `app/bundled-resources/v1/state.json` | 插件资源版本台账 | 是 |
| `.models/<uid>/catalog-v6` | 模型目录 | 否，密文 |

- 项目目录名（slug）：`projectRoot.replace(/[^a-zA-Z0-9]/g, "-")`（长度超限时截断并追加
  djb2 变体 base36 后缀）。示例：`C:\work\alpha\beta` → `C--work-alpha-beta`。
- 全目录**没有任何 SQLite / 索引库**；会话列表来自目录扫描或服务端。
- **「段」在本目录有两个互不相干的含义，勿混**：① `logs/sessions/<slug>/<sessionId>/segments/`
  下的**日志文件分段**（每次 CLI 进程启动写一个文件）；② `state.json` 的
  `items.<段键>`（本机仅 `s0`），是加密**逻辑段**，同时是 §5.1 里必须进 AAD 的
  `segmentKey`。`projects/**` 下每个会话只有**一个**主转录 `.jsonl`，不按段拆分。

## 3. 转录记录格式（明文 JSONL）

逐行一个对象，以 `type` 区分。记录类型白名单（自二进制还原，共 18 种）：

`user`、`assistant`、`system`、`attachment`、`summary`、`custom-title`、`ai-title`、
`last-prompt`、`tag`、`workspace-directories`、`runtime-config`、`mode`、
`content-replacement`、`file-history-snapshot`、`token-stats`、`active-leaf`、
`relocated`、`worktree-state`。

本机实际出现的记录类型（**全量口径，含子代理**；逐条计数如下）：

| type | 合计 | 主转录 | 子代理 | 备注 |
| --- | --- | --- | --- | --- |
| `assistant` | 2558 | 2009 | 549 | 每请求 N 条（N=2–9），仅其中一条带 `message.usage` |
| `user` | 1347 | 1030 | 317 | 带 `message`（人类输入或 `tool_result`） |
| `active-leaf` | 2078 | 2078 | 0 | 分支叶子指针（`leafUuid` / `explicit`） |
| `attachment` | 124 | 124 | 0 | 用 `attachment` 字段，**不带 `message`** |
| `file-history-snapshot` | 45 | 45 | 0 | 分布在 10-06 / 10-07 / 10-08 三个会话；**无 `timestamp`** |
| `runtime-config` | 16 | 16 | 0 | **子代理文件 0 条**——该差异决定窗口取值，见 §6 复核注 |
| `last-prompt` | 14 | 6 | 8 | 供 UI 恢复输入框 |
| `workspace-directories` | 10 | 10 | 0 | 含尾部重写重复项 |
| `worktree-state` | 5 | 5 | 0 | git worktree 状态 |
| `system` | 4 | 1 | 3 | 状态事件，`subtype` ∈ {`model_queue_status`, `api_retry`, `compact_boundary`}；用 `content` + `subtype`，**不带 `message`** |

合计 10 种（初稿枚举遗漏 `file-history-snapshot` 与 `system`；首轮修订把 `system`
误记为 1 条，实为 4 条）。**注意口径**：主转录与子代理各自的类型集合不同——
子代理只有 `user` / `assistant` / `last-prompt` / `system` 四型，**没有
`runtime-config`**（该差异直接影响 §5.3 的窗口取值，见 §6 复核注）。

其中 `system` 的三种 `subtype` 对解析有意义：`compact_boundary`（对话压缩边界）、
`api_retry`（带 `attempt` / `max_retries` / `retry_delay_ms`）、
`model_queue_status`（带 `queue_type` / `wait_time_ms`）。

要点：

- 公共上下文字段（`uuid`、`parentUuid`、`timestamp`、`cwd`、`sessionId`、`isSidechain`、
  `entrypoint`、`version`、`gitBranch`）出现在 `assistant` / `user` / `attachment` /
  `system` **四型**；其中**只有 `assistant` 与 `user` 带 `message` 字段**，
  `attachment` 用 `attachment` 字段、`system` 用 `content` + `subtype`。
  （2026-10-11 四次通读补正：初稿写成"三型带公共字段"，既漏了 `system`，
  又误把 `attachment` 归入带 `message` 的一类。）
- `runtime-config` 提供 `model`（本机恒为 `qfmodel`）与 `contextWindow`（本机 1000000）。
- `active-leaf` 是分支叶子指针（`leafUuid` / `explicit`）。
- **一次 API 请求会产生 N 条 `assistant` 记录**（每个 content block 一条：
  `thinking` / `text` / `tool_use`），共享同一 `message.id` 与同一
  `requestTokenAnchor`；其中**恰好一条**（`stop_reason` 非空的那条）携带
  `message.usage`。本机实测 N 分布 2–9（中位 2），2558 条带锚记录
  （主转录 2009 + 子代理 549）/ 1166 条带 `usage` 记录 / 1166 个唯一 `requestId`
  → 严格 1:1，且带 `usage` 的记录 100% 是 `assistant` 类型、`stop_reason` 100% 非空。
- **分组键必须用 `requestId` 或 `message.id`，不能用 `timestamp`**：同一请求的各块
  逐块落盘，`timestamp` 存在**毫秒级抖动**——实测 1166 组中 71 组（6.1%）组内
  `timestamp` 相差 1 ms（最大跨度 1 ms），其余 1095 组全同。
  （2026-10-11 三次复核更正：初稿称三字段"同一"不成立。）
- **口径注意**：上述计数**含子代理转录**；§6 表的「带 ratio 记录数」仅统计主转录
  （合计 935），相差的 231 条为子代理请求。主/子各类型的条数拆分见上表。
- 元数据采用**尾部重写**策略（`metadataTailDirty` + `appendCachedMetadataEntriesToTail`），
  因此 `runtime-config`、`workspace-directories` 会在文件尾部重复出现——解析时须按
  "末次生效"处理，不能按出现次数计数。
- `token-stats`（字段 `sessionId` / `promptTokenCount` / `timestamp`）在二进制中**只有
  定义、没有任何调用方**，本机全盘搜索零命中 → 该记录类型实际从不落盘。

## 4. 用量字段抹零机制（关键）

### 4.1 服务端 → 本地映射

服务端为 gRPC 服务（`proto/chat.proto`，包 `model.chat`，`ChatService.ChatCompletion{,Stream}`），
响应 `Usage` 采用 OpenAI 形状：

```proto
message Usage {
  int32 prompt_tokens = 1;
  int32 completion_tokens = 2;
  int32 total_tokens = 3;
  PromptTokensDetails prompt_tokens_details = 4;    // cached_tokens / cacheable_tokens
  CompletionTokensDetails completion_tokens_details = 5;  // reasoning_tokens
}
```

映射函数（简化后语义等价）：

```js
function jb(resp) {
  let e = emptyUsage();
  if (!resp) return e;
  e.input_tokens                = resp.prompt_tokens ?? 0;
  e.output_tokens               = resp.completion_tokens ?? 0;
  e.cache_read_input_tokens     = resp.prompt_tokens_details?.cached_tokens ?? 0;
  e.cache_creation_input_tokens = resp.cache_creation_input_tokens ?? 0;
  setHidden(e, "total_tokens", resp.total_tokens);          // 不可枚举
  setHidden(e, "thinking_tokens", resp.completion_tokens_details?.reasoning_tokens);
  if (isFiniteNum(resp.credits))          e.credits = resp.credits;
  if (isFiniteNum(resp.original_credits)) e.original_credits = resp.original_credits;
  if (typeof resp.billable === "boolean") e.billable = resp.billable;
  return e;
}
```

注：`total_tokens` / `thinking_tokens` / `thoughts_token_count` 被定义为**不可枚举属性**，
因此**永远不会出现在序列化后的 JSONL 里**，但在内存中确实存在。

### 4.2 抹零

```js
function dv(usage, opts) {
  let t = normalize(usage);
  if (opts?.exposeTokenCounts || exposeTokenUsageEnv()) {         // 保留分支
    let out = { ...t, iterations: [...t.iterations] };
    if (opts?.contextUsageRatio !== undefined) out.context_usage_ratio = opts.contextUsageRatio;
    return out;
  }
  let out = { ...t,
    input_tokens: 0, output_tokens: 0,
    cache_creation_input_tokens: 0, cache_read_input_tokens: 0,
    server_tool_use: { ...t.server_tool_use },
    cache_creation: { ...t.cache_creation, ephemeral_1h_input_tokens: 0, ephemeral_5m_input_tokens: 0 },
    iterations: [...t.iterations] };
  for (const k of ["total_tokens", "prompt_tokens", "completion_tokens"])
    if (k in out) out[k] = 0;
  if (opts?.contextUsageRatio !== undefined) out.context_usage_ratio = opts.contextUsageRatio;
  return out;
}
```

即：**token 字段被归零，`context_usage_ratio` 被刻意保留**——它是明文转录里唯一的
真实 token 泄漏点。

遥测日志同策：`model.response.completed` 事件发射时带
`preserveSessionTokenUsage: "custom" === provider`，仅 BYOK 自定义模型保留计数；
内置 `qoder` provider 一律归零。

### 4.3 打开真值的环境变量

CN 版变量前缀由 `prefix = isCN ? "QODERCN_" : "QODER_"` 决定（本机 `isCN === true`，
与 `gj = ".qoder-cn"`、`build.productName = "qoderclicn"` 一致）。取值判定接受
`1` / `true` / `yes`（不区分大小写）。

| 环境变量 | 作用 |
| --- | --- |
| `QODERCN_EXPOSE_TOKEN_USAGE` | 走保留分支，转录落真实 token |
| `QODERCN_SESSION_STATE_PLAINTEXT` | 关闭 `state.json` 加密，直接明文 |
| `QODERCN_EXPOSE_REQUEST_ID` | 保留请求 id |

> 这三项是**代码静态验证**结论，未实际带变量运行 CLI 复核（会消耗账号额度并在
> `~/.qoder-cn` 落新会话）。TokenScope 不得依赖它们，只作为排障知识登记。

## 5. 真实账本：加密 `state.json`

### 5.1 解密配方

| 要素 | 取值 |
| --- | --- |
| 算法 | `aes-256-gcm` |
| 主密钥 | **硬编码 32 字节常量**（见下） |
| nonce `n` | 12 字节，base64 |
| 认证标签 `t` | 16 字节，base64 |
| 密文 `p` | base64 |
| AAD | `JSON.stringify({sessionId, projectHash, segmentKey})` |

硬编码主密钥（十六进制）：

```
2c06609f4389ac8698349df80fdb3172e111c3254797915fd6e209a9fa607de8
```

各字段推导：

- `projectHash = sha256(normalize(projectRoot), "utf8").hex`
  其中 `normalize(p) = path.win32.resolve(p).replace(/\\/g, "/").toLowerCase()`
  （win32/darwin 转小写；posix 不转）。
  例：`C:\work\alpha` → `c:/work/alpha`。
- `segmentKey` = `state.json` 中 `items` 的**键名本身**（本机为 `s0`）。
  **它必须出现在 AAD 里**，否则 GCM 认证失败。
- `state.json` 路径 = `<配置目录>/projects/<slug>/<sessionId>/state.json`。

`state.json` 顶层结构：`{ sessionId, revision, createdAt, updatedAt, data, items }`；
`items.<段键>` 即上述 `{c, u, n, p, t}` 五元组（`c`/`u` 为创建/更新时间）。

参考实现（Node，仅用内置模块）：

```js
const KEY = Buffer.from("2c06609f4389ac8698349df80fdb3172e111c3254797915fd6e209a9fa607de8", "hex");
const norm = (p) => path.win32.resolve(p).replace(/\\/g, "/").toLowerCase();
const projHash = (p) => crypto.createHash("sha256").update(norm(p), "utf8").digest("hex");

function decryptSegment(item, sessionId, projectRoot, segmentKey) {
  const aad = Buffer.from(JSON.stringify({ sessionId, projectHash: projHash(projectRoot), segmentKey }), "utf8");
  const d = crypto.createDecipheriv("aes-256-gcm", KEY, Buffer.from(item.n, "base64"));
  d.setAAD(aad);
  d.setAuthTag(Buffer.from(item.t, "base64"));
  return Buffer.concat([d.update(Buffer.from(item.p, "base64")), d.final()]).toString("utf8");
}
```

### 5.2 解密后结构

```json
{
  "updatedAt": "…Z",
  "latest":  { "input_tokens": 0, "cache_creation_input_tokens": 0,
               "cache_read_input_tokens": 0, "output_tokens": 0 },
  "latestRequestId": "<uuid>",
  "total":   { "input_tokens": 0, "cache_creation_input_tokens": 0,
               "cache_read_input_tokens": 0, "output_tokens": 0 },
  "credits": { "total": 0, "byModel": { "<model>": { "credits": 0 } } }
}
```

- `latest` / `total` 由 `L7A(sessionId, message.usage)` 就地累加，其**输入对象与写转录
  用的是同一个** `message.usage`。
- `credits.total` 与转录内 `credits` 不是同一口径：本机 4/4 会话解密后均为 `0`，
  而转录内 `credits` 合计 683.44 且 `billable` 恒 `false`。**不要把 `credits` 当作
  计费口径使用。**
- 本机 4/4 会话全部解密成功（`revision` 58 / 98 / 189 / 1535）。

### 5.3 `context_usage_ratio` 的精确含义

```js
function setContextUsageRatio(usage, maxInputTokens) {
  if (typeof maxInputTokens !== "number" || !Number.isFinite(maxInputTokens) || maxInputTokens <= 0) return;
  if (Number.isFinite(usage.input_tokens))
    usage.context_usage_ratio = Math.min(1, Math.max(0, usage.input_tokens / maxInputTokens));
}
```

调用点为流式收尾：`ratio = input_tokens / (显式传入值 ?? model_config.max_input_tokens)`。
本机 `contextWindow` 末次生效值恒为 1000000（个别中间记录为 null，按 §3 的
「末次生效」口径处理），与之吻合。

**因此 `Math.round(context_usage_ratio × contextWindow)` 可无损还原该请求的
`input_tokens`**（只要 `input_tokens ≤ contextWindow`，见 §10 的钳位说明）。

关于精度（2026-10-11 二次复核更正）：初稿把成因写成"窗口 1e6 与 ratio 的 6 位小数
刚好互补"，**这个解释不成立**。实测 ratio 的小数位数并不固定——分布为
2 位×1 / 4 位×8 / 5 位×118 / 6 位×1039，6 位只是观测上界（JSON 序列化 `number`
用的是最短往返表示，位数随数值自然变化）。真正保证还原的是 IEEE-754 double 的
约 16 位有效数字：除法引入的相对误差在 1e-16 量级，乘回窗口后的绝对误差远小于
0.5，取整必然落在正确整数上。**该结论因此对任意窗口值成立，不限于 10 的幂**。

## 6. 交叉验证记录

逐请求 `context_usage_ratio × contextWindow` 求和 vs 解密后的 `total.input_tokens`
（**仅主转录**口径：带 ratio 记录数合计 935，子代理 231 条见文末复核注）：

| 会话（前 8 位） | 窗口 | 带 ratio 记录数 | Σ(ratio×窗口) | total.input_tokens | 差额 |
| --- | --- | --- | --- | --- | --- |
| `bd703276` | 1000000 | 28 | 1241572 | 1241572 | **0** |
| `c78f5543` | 1000000 | 47 | 4880510 | 4880510 | **0** |
| `d912f9c3` | 1000000 | 767 | 218454955 | 218454955 | **0** |
| `f67f12fd` | 1000000 | 93 | 10925529 | 10925529 | **0** |

四条独立证据互相印证：代码路径（`dv` 抹零 / `jb` 映射 / `L7A` 累加）、GCM 实际解密成功、
ratio 求和与解密值**逐会话精确闭合**、以及"`token-stats` 无调用方"与本机零命中的一致。

> **2026-10-11 复核注（新发现）**：把子代理的 231 条 ratio 计入求和则闭合被打破
> （c78f5543 多出 6,148,437、d912f9c3 多出 7,745,383）——即 `state.json.total`
> **只累计主链请求，不含子代理请求的 input**。因此 §8 的会话级四桶（来源
> `total`）不含子代理用量：子代理明细必须单独采集；且子代理转录没有
> `runtime-config`，其 `context_usage_ratio` **没有本文件内的窗口值可依**——
> 严格还原做不到，只能按 §8.1 通道 C 借用主链窗口（未验证假设）。

## 7. 消息映射证明：`input_tokens` 含 `cache_read`

二进制内的统计聚合代码：

```js
n.tokens.prompt     += r.input_tokens;
n.tokens.candidates += r.output_tokens;
n.tokens.total      += r.input_tokens + r.output_tokens;
n.tokens.cached     += r.cache_read_input_tokens;
n.tokens.input       = Math.max(0, n.tokens.prompt - n.tokens.cached);   // ★
```

"非缓存输入"必须由 `prompt - cached` 推出，说明 **`input_tokens` 是含缓存的 prompt 总量**
（OpenAI 语义），而非 Claude 的互斥桶。

**接入铁律：**

| TokenScope 目标桶（互斥） | Qoder 源字段取值 |
| --- | --- |
| `input_tokens` | `q.input_tokens − q.cache_read_input_tokens`（下限 0） |
| `cache_read_input_tokens` | `q.cache_read_input_tokens` |
| `cache_creation_input_tokens` | `q.cache_creation_input_tokens`（本机恒为 0） |
| `output_tokens` | `q.output_tokens` |

`q.` 前缀指 Qoder 侧原字段——**两侧 `input_tokens` 同名但语义不同**：Qoder 的是
含缓存的 prompt 总量，TokenScope 的是非缓存增量。

归一前后四桶之和的关系：`cache_read` 只是**换桶**（从 `input_tokens` 挪进自己的桶），
不改变总量，因此

```
归一后 Σ四桶 = q.input_tokens + q.cache_creation_input_tokens + q.output_tokens
```

只有当 `q.cache_creation_input_tokens` 非零时才会比 Qoder 的 `input + output` 多出
该部分；本机 `cache_creation` 恒为 0，故两者相等。**不要**把 `cache_read` 误当成
求和差额——这是与 Claude / Codex 侧既有口径一致的前提，属预期而非错误。

不归一就直接并入 Claude / Codex 汇总会**把缓存读双计一次**。

## 8. 实现路线（选定：解密 `state.json`）

### 8.1 取数设计

三条取数通道（**二次复核后拆为三块**，覆盖范围互不重叠）：

| 通道 | 来源 | 覆盖范围 | 粒度 | 可得字段 |
| --- | --- | --- | --- | --- |
| A（权威） | 解密 `state.json` 的 `total` | **仅主链** | 会话 | Qoder 四桶全部 |
| B（明细） | 主转录 `ratio × contextWindow` | 主链 | 请求 | `input_tokens`（含缓存） |
| C（子代理） | 子代理转录 `ratio` × **借用的**窗口 | 子代理 | 请求 | `input_tokens`（含缓存，且窗口值未验证） |

由 §6 的零差额可得：**主链上通道 B 求和恒等于通道 A 的 `input_tokens`**（两侧同为
「含缓存」口径）。因此 A 与 B 不能相加，必须选定记账粒度。

子代理是**独立的第三块**：它既不在 `state.json.total` 里（§6 复核注已证），
其转录里又没有 `runtime-config`（§3 已证），因此**没有可用的窗口值**。通道 C
只能借用主转录的 `contextWindow` 代入——这是一个**未验证假设**（理由：同一会话
同一模型，窗口应一致；但无直接证据）。子代理也因此拿不到 `output_tokens` 与
`cache_read_input_tokens`。

**建议：按会话记账为事实（通道 A），按请求记账为明细（通道 B），子代理单独成块（通道 C）。**
理由：`output_tokens` 与 `cache_read_input_tokens` 只有会话粒度，任何"逐请求四桶"
的构造都是伪造；而会话级四桶（§5.2 的 `L7A` 累加值）是唯一具备权威粒度的口径
（其上游来源的取证状态见 §10）。

具体落法：

- 主链每会话产出 **1 条事件**，四桶取自 `state.json.total`，**入库前按 §7 做桶归一**
  （即事件里的 `input_tokens` 是 `total.input_tokens − total.cache_read_input_tokens`，
  不是原字段值）。请求数用**该会话主链**带 `usage` 的记录数。**口径修正（2026-10-11）**：
  `total` 只覆盖主链请求（本机 935 条），请求数须按「主链」与「子代理」分开统计，
  不得把 1166 全量与 `total` 混同（见 §6 复核注）。
- 逐请求明细另存为**只带 input 的子条目**，或在明细视图标注"仅 input 精确、
  其余为会话级"。
- 子代理按通道 C 单独产出 input 明细，并在数据模型或 UI 上**显式标注窗口值为借用**；
  在拿到证据之前，不应把子代理 input 与会话级四桶混合成同一条事件。
- 会话级事件的时间戳：起点用 `state.json.createdAt`（或转录首个 `message` 记录时间），
  终点用 `updatedAt`；区间筛选按既有约定作用于会话区间。

### 8.2 必须保持的不变量

1. **只读**：绝不写入、移动、清理 `~/.qoder-cn` 任何文件。
2. **桶归一**：入库前必须执行 §7 的 `input - cache_read`（下限 0），并在明细中
   保留原始值以便核对。
3. **不重复记账**：`logs/sessions/**` 与 `projects/**` 的 `request_id` 完全重叠
   （本机交集 1166/1166），**不得同时采集**。
4. **主链与子代理不得混算**：`state.json.total` 只含主链（§6 复核注）。会话级四桶
   事件与其下的子代理 input 明细是**两块独立事实**，任何把两者相加、或把子代理
   input 计入会话 `total` 的做法都会重复或错配。
5. **请求身份**：唯一键取带 `usage` 记录的 `request_id`（=`requestTokenAnchor.requestId`）。
   按"每行一条"计数会虚增一倍以上（本机 2558 条锚定记录对应 1166 次请求）。
6. **解密失败即降级**：主密钥随版本重编译可能变化。认证失败（
   `SessionStateDecryptionError`）时该会话标记为"用量未知"，不得静默补 0、
   不得回退到已抹零的转录 token 字段当作真值。
7. **时间口径（2026-10-11 二次复核更正）**：`state.json` 的 `createdAt`/`updatedAt`
   为 ISO8601 UTC；转录内的 `timestamp` **按记录类型是两种格式**——
   `user` / `assistant` / `attachment` / `system` 为 **ISO8601 UTC 字符串**，
   `active-leaf` / `runtime-config` 为 **epoch 毫秒数字**（`file-history-snapshot`
   等元数据记录不带 `timestamp`）。实现必须按类型分别解析，不得假定统一格式。
   **首轮修订 4 曾笼统写成「转录 `timestamp` 为 epoch 毫秒」，对上列四型都是错的**
   （注意 `system` 没有 `message` 字段，但它同样用 ISO8601）——照此实现会让全部
   请求时间戳解析失败。存储层原样 UTC 持有，只做一次时区转换（沿用既有约定）。
8. **元数据尾部重写**：`runtime-config` / `workspace-directories` 按末次出现生效，
   不按出现次数计数。

### 8.3 任务清单（待立项后细化到测试名与验证命令）

| 编号 | 任务 | 关键点 |
| --- | --- | --- |
| Q01 | 路径发现 | 扫描 `~/.qoder-cn/projects/*/*/state.json` 与同名 `.jsonl`（含 `subagents/`）；目录不存在时静默零结果 |
| Q02 | `projectHash` 复现 | win32/posix 分支、大小写归一、`resolve` 语义；用本机 4 个会话做对照 |
| Q03 | GCM 解密 + 结构校验 | 严格校验 `{c,u,n,p,t}` 五元组；失败给结构化诊断而非抛栈 |
| Q04 | 时间戳双格式解析 | `message` 类按 ISO8601、`active-leaf`/`runtime-config` 按 epoch 毫秒；各写一条定点断言 |
| Q05 | 会话级事件产出 | 四桶取自 `total` 且**先按 §7 归一**；请求数只数该会话主链带 `usage` 的记录 |
| Q06 | 主链逐请求 input 还原 | `round(ratio × contextWindow)`；窗口取该会话 `runtime-config` 的末次生效值 |
| Q07 | 子代理 input 明细 | 窗口借用主链并**显式标注未验证**；与会话级四桶分开存放，不合并 |
| Q08 | 闭合断言 | **主链** `Σ(ratio×窗口) == total.input_tokens`，不等则报诊断（子代理不参与该断言） |
| Q09 | 合成 fixture 三件套 | 路径发现 / 典型解析 / 畸形行容错，含加密 `state.json` fixture；**真实日志永不入库** |
| Q10 | 版本漂移探测 | 密钥失效时明确降级，不污染统计 |

### 8.4 验收标准

- 本机 4 个真实会话全部解密并产出事件，且**逐会话主链** `Σ(ratio×窗口)` 与
  `total.input_tokens` 差额为 0。
- 桶归一后与"未归一"版本的费用/用量对比可解释（差值 = 缓存读被双计的部分）。
- 双格式时间戳各有定点用例：`assistant.timestamp` 与 `active-leaf.timestamp`
  都能解析成正确时刻，且两种格式不会被同一个解析器混判。
- 子代理 input 明细与会话级四桶**并存但不相加**；任何把两者合并的构造必须在
  测试中失败（本机对照量：c78f5543 子代理 input 6,148,437、d912f9c3 为 7,745,383）。
- 篡改 `state.json` 任一字段（`n` / `p` / `t` / `sessionId` / `projectHash` / 段键）必须
  认证失败并降级，不产生任何事件。
- 解密失败会话不计入未知为 0 的静默吞掉——沿用"未知不按 0 处理"的既有原则。

## 9. 备选路径与取舍

| 路径 | 可行性 | 结论 |
| --- | --- | --- |
| 解密 `state.json` | 可行，已实测 4/4 | **选定**：唯一能拿到四桶与 output 的通道 |
| 转录 `context_usage_ratio` | 可行，精确闭合 | 作为逐请求 input 的补充通道 |
| 转录 `usage` token 字段 | 默认配置下恒为 0 | 不可依赖：只有打开 §4.3 的开关才落真值，而 TokenScope 不得要求用户改启动环境；不必再探测 |
| `credits` / `billable` | 口径不清（`state` 为 0、转录非 0、`billable` 恒 false） | 不作为计量口径 |
| 依赖 `QODERCN_*` 环境变量 | 需用户改启动环境 | 不作依赖，仅登记 |
| `logs/sessions/**` | token 同样抹零 | 不作数据源（且与转录重叠） |

## 10. 风险与未知

- **主密钥稳定性**：硬编码常量随发布重新编译可能改变。探测手段是解密认证是否通过；
  失败即降级。当前基于 1.1.66 单版本样本，未做跨版本对照。
- **服务端语义未抓包验证**：推断 `jb` 的 `resp.prompt_tokens` 来自服务端 gRPC 响应
  （proto 确有该字段），但未做流量级取证。`jb` 是全包中 `input_tokens` 的唯一赋值点，
  链条紧密，但严格说属于"一致解释"而非"已验证"。
- **窗口值来源**：主链的 `contextWindow` 取转录 `runtime-config` 的**末次生效**值
  （存在 `contextWindow: null` 的中间记录，不能取首条）；若某会话完全缺失该记录，
  主链 ratio 还原不可用，须降级。子代理**天然**没有该记录（§3），只能按 §8.1 通道 C
  借用主链值——该借用是未验证假设，不得当作已验证事实。
- **`input_tokens` 上限与钳位**：`Zet` 用 `Math.min(1, …)` 把 ratio 钳到 `[0,1]`，
  因此当 `input_tokens ≥ contextWindow` 时 ratio 恒为 `1.0`，真实值不可恢复。
  本机最大 ratio 0.596408（对应 596,408 / 1,000,000），未触及该边界，但实现必须
  把「ratio == 1.0」当作**饱和标志**而非精确值处理。还原精度本身不受小数位数限制
  （见 §5.3 的 double 精度说明）。
- **时间戳双格式**：转录内的对话类（`assistant` / `user` / `attachment`）与状态类
  （`system`）记录是 ISO8601 字符串，`active-leaf` / `runtime-config` 是 epoch
  毫秒数字（详见 §8.2 不变量 7）——单一解析器会失败。
- **多段键**（`items.<段键>`，与 §2 提到的日志 `segments` 文件无关）：本机只见 `s0`，
  多段语义未验证。实现时若遇到多段，应明确拒绝或对全部段键求和，不可只取首键。

## 11. 附录：逆向脚手架

本机临时目录（不入库）中的工具：

| 脚本 | 用途 |
| --- | --- |
| `asar.js` | asar 解析。头部偏移要点：`jsonSize = u32@12`、`dataStart = 8 + u32@4` |
| `unxor.js` | 还原 base64+XOR 字符串（密钥 `EI4szwWSfoFN`，解出 15630 条） |
| `ctx.js` | 按字符偏移窗口反查代码上下文（注意字符索引 ≠ 字节索引） |
| `decrypt-state.js` | 单文件解密 + 段键爆破 |
| `decrypt-all.js` | 批量解密 + ratio 交叉验证 |
| `verify-closure.js` | §6 闭合性验证 |

分析对象：**注意存在两份副本**——安装根 `resources/` 与
`.qoder-versions/<版本>/resources/`。两处的 `app.asar` 与 worker runtime 都是
**不同文件**（SHA256 各不相同），说明二者是同产品的**不同构建**，不可互相替代。
本机实际生效的是后者，§4.3 的环境变量前缀与 §12 的复核命中均以生效副本为准：

| 文件 | 生效副本（`.qoder-versions/0.4.4/resources/`） | 安装根（`resources/`） | 差值 |
| --- | --- | --- | --- |
| `app.asar` | **141,362,911 字节 ≈ 134.81 MiB** | 141,361,471 字节 ≈ 134.81 MiB | 1,440 字节 |
| `app.asar.unpacked/.../_worker/qoder-worker-runtime.obf.mjs` | **33,097,850 字节 ≈ 31.56 MiB / 781 行** | 33,191,711 字节 ≈ 31.65 MiB | 93,861 字节 |

- `app.asar` 顶层为 `node_modules` / `out` / `package.json`（解析要点见上表 `asar.js` 行）
- 同目录 `proto/chat.proto`（gRPC 协议定义）、`runtime-info.json`（版本 / commit / 构建目标）
- 混淆只加密**字符串字面量**，标识符与逻辑全部明文——这是本文全部代码级结论的前提

补充：本地服务端错误码表可用于诊断映射——`loginExpired:105`、
`todayUsageLimitExceeded:110`、`usageLimitExceeded:113`、`freeTrialAccountsExceeded:114`、
`freeUserQuotaLimit:115`、`personalCreditsDrainedOut:118`、
`billingGroupCreditsLimitReached:122`、`inputContentTooLong:80411`。配额由服务端强制。

## 12. 复核修订记录（2026-10-11）

- 核心复算全部通过：4/4 `state.json` 按 §5.1 配方解密成功，revision {58, 98,
  189, 1535} 与段键 `s0` 逐项一致；§6 表逐格复现，`Σ(ratio×窗口)` 与
  `Σ round(ratio×窗口)` 两种口径差额均为 0；XOR 密钥 `EI4szwWSfoFN` 在生效副本
  （`.qoder-versions/0.4.4/` 的 `qoder-worker-runtime.obf.mjs`，33,097,850 字节 /
  781 行）中原文命中；`proto/chat.proto` 的 `Usage` 字段逐字吻合；遥测
  `request_id` 与转录唯一 requestId 交集 1166/1166；`token-stats` 全库零命中；
  转录 credits 合计 683.4423、`billable` 恒 false、`credits.total` 全 0 均复现。
- 修订 1：§3 记录类型补全为 10 种（漏 `file-history-snapshot`×45、`system`×1）。
- 修订 2：§3/§6 标注「主+子代理 1166」与「仅主转录 935」的口径差。
- 修订 3（新发现）：§6/§8 补记 `state.json.total` 不含子代理请求 token，
  §8.1 请求数口径同步修正。
- 修订 4：§8.2 转录 `timestamp` 更正为 epoch 毫秒。→ **已被二次复核推翻，见下**
- 修订 5：§5.3 `contextWindow` 改按「末次生效」口径表述（存在 null 中间记录）。

### 二次复核（2026-10-11，全文逐项机器复算）

复核方式：对全文每条可验证断言重跑独立脚本——记录类型全量计数、`timestamp`
字段类型分布、ratio 小数位分布、子代理文件类型集合、闭合性复算、密钥逐字节比对。

| 断言 | 结论 |
| --- | --- |
| §5.1 主密钥 hex（64 字符 / 32 字节） | ✅ 与字节数组 `[44,6,96,159,67,137,…]` 逐字节一致 |
| §3 锚定记录 2558 = 主 2009 + 子 549 | ✅ 精确 |
| §3 usage 1166 = 主 935 + 子 231；唯一 requestId 1166 | ✅ 精确 |
| §3 本机出现记录类型共 10 种（含分型条数） | ✅ 精确：`assistant`×2558 / `active-leaf`×2078 / `user`×1347 / `attachment`×124 / `file-history-snapshot`×45 / `runtime-config`×16 / `last-prompt`×14 / `workspace-directories`×10 / `worktree-state`×5 / `system`×4 |
| §6 四条会话闭合 0 差额 | ✅ 逐格复现 |
| §6 复核注子代理超额 6,148,437 / 7,745,383 | ✅ 精确复现 |
| §6 复核注「子代理无 `runtime-config`」 | ✅ 8/8 子代理文件均为 0 条 |
| §8.2 首轮修订 4「转录 timestamp 为 epoch 毫秒」 | ❌ **错误，已推翻** |
| §3「`system` … 1 条」 | ❌ 实为 4 条（主转录 1 + 子代理 3），已更正 |
| §5.3「窗口 1e6 与 ratio 的 6 位小数刚好互补」 | ❌ 成因不成立（位数分布 2/4/5/6 位不等）；结论保留，解释已改写 |

推翻「修订 4」的证据——`timestamp` 字段类型分布（全量）：

| 记录类型 | `timestamp` 类型 | 条数 |
| --- | --- | --- |
| `assistant` | ISO8601 字符串 | 2558 |
| `user` | ISO8601 字符串 | 1347 |
| `attachment` | ISO8601 字符串 | 124 |
| `system` | ISO8601 字符串 | 4 |
| `active-leaf` | epoch 毫秒数字 | 2078 |
| `runtime-config` | epoch 毫秒数字 | 16 |
| `file-history-snapshot` / `last-prompt` / `workspace-directories` / `worktree-state` | 无 `timestamp` 字段 | — |

即首轮的「更正」把原本正确的表述改错了：`message` 类记录（实现最依赖的请求时间戳
所在）**全部是 ISO8601**。已按类型重写 §8.2 不变量 7，并在 §10 列为独立风险。

本轮同时修正的其他问题：

- §3 记录类型表改为带条数的表格，并显式标注主转录与子代理的**类型集合差异**
  （子代理仅 4 型、无 `runtime-config`），避免读者把两处口径当同一件事。
- §1 结论 4 补上「闭合仅限主链」的限制，与 §6 复核注对齐（原摘要与正文不一致）。
- §2 澄清「段」的两种含义（日志 `segments` 文件 vs `state.json` 的 `items.<段键>`），
  §10 同步加限定词；原表述下两处同名概念会互相误读。
- §7 表格改用 `q.` 前缀消歧——原表左右两侧同写 `input_tokens` 但语义不同；并更正
  本轮改写时一度写错的求和关系（曾误称差额来自 `cache_read`，实际 `cache_read`
  只是换桶、不改总量，差额只可能来自 `cache_creation`）。
- §8.1 由「两条通道」改为**三条通道**（新增子代理通道 C），并把「按会话记账」
  明确为「按会话记账为事实 + 主链明细 + 子代理独立成块」。
- §8.2 由 7 条不变量扩为 8 条，新增「主链与子代理不得混算」。
- §8.3 任务清单由 Q01–Q08 扩为 **Q01–Q10**，拆出「时间戳双格式解析」（Q04）与
  「子代理 input 明细」（Q07），闭合断言（Q08）限定为主链。
- §9「转录 usage token 字段 永久不可用」改为「默认配置下恒为 0，不可依赖」——
  与 §4.3 的开关说明原本存在轻微矛盾。
- §11 统一体积单位（MiB 与字节并列），并补记 `resources/` 与 `.qoder-versions/<版本>/`
  两份副本的存在。

### 三次复核（2026-10-11，换实现方式重写全部核验脚本）

方法：不复用前两轮脚本，全部核验逻辑重写（`JSON.parse` 逐行 + 严格类型判断 + 分组
聚合），并新增前两轮**未验过**的强断言。

新增验证（全部通过）：

| 断言 | 结果 |
| --- | --- |
| `requestTokenAnchor` 只出现在 `assistant` 上 | ✅ 2558 = 2558，其他类型 0 条 |
| 每组（同 `requestId`）恰好 1 条带 `usage` | ✅ 1166/1166，分布仅 {1} |
| 每组 `message.id` 唯一 | ✅ 1166/1166 |
| 带 `usage` 的记录 100% 为 `assistant` 且 `stop_reason` 非空 | ✅ 各 1166，反例 0 |
| 每组记录数 N 的分布 | ✅ 2–9（中位 2） |
| 主链 / 子代理 ratio 条数 | ✅ 主 935（28+47+767+93）、子 231（89+142） |
| 全部 jsonl 无坏行 | ✅ 6201 条记录、坏行 0 |
| 子代理超额（再算一次） | ✅ 6,148,437 / 7,745,383 |
| ratio 无饱和（`>= 1` 的条数） | ✅ 0，最大 0.596408 |
| 目录内 SQLite 类文件 | ✅ 0 个 |
| `token-stats` / `promptTokenCount` 全盘命中 | ✅ 0（含 `logs/`） |
| `proto` 的 `Usage` 六字段逐字 | ✅ 与正文一致 |
| XOR 密钥 `EI4szwWSfoFN` 命中次数 | ✅ 1 |
| 转录 credits 合计 | ✅ 683.442261161 |

本轮新查出的错误（已修正）：

- **§3「共享同一 `timestamp`」不成立**。实测 1166 组中 **71 组（6.1%）组内
  `timestamp` 相差 1 ms**（最大跨度 1 ms），仅 1095 组全同——逐块落盘存在毫秒抖动。
  这是**会影响实现**的错误：按 `timestamp` 分组去重会在 6.1% 的请求上失效。
  已改为「分组键必须用 `requestId` 或 `message.id`」。
- **§11 两份副本的体积全部配错**。上一轮改写时把安装根 `app.asar` 的 141,361,471
  字节配到了生效副本的路径上，并把安装根的 worker runtime 写成「同路径同名」。
  实测两处都是**不同文件**：
  `app.asar` 生效副本 141,362,911 / 安装根 141,361,471（差 1,440 字节）；
  worker runtime 生效副本 33,097,850 / 安装根 33,191,711。
  即安装根与 `.qoder-versions/` 是同产品的**不同构建**，不可互相替代。
  已改为两列对照表，并显式标注以生效副本为准。

### 四次通读（2026-10-11，逐行读完 + 补验存疑处）

方法：不再依赖脚本汇总，逐行读完全文，对每处表述追问"是否有机器证据"，并补验
两处此前只用过二手推断的地方（`system` 记录结构、`message` 字段的存在性）。

新补验的事实：

| 断言 | 结果 |
| --- | --- |
| `message` 字段出现在哪些类型 | ✅ 仅 `assistant`（2558）与 `user`（1347）；`attachment` / `system` 等 8 型**均无** |
| `system` 记录的字段构成 | ✅ 有 `uuid` / `cwd` / `sessionId` / `isSidechain` / `timestamp`（ISO8601），用 `content` + `subtype`，无 `message` |
| `system` 的 `subtype` 取值 | ✅ 4 条为 `model_queue_status`×2 / `api_retry`×1 / `compact_boundary`×1（主转录那条是 `compact_boundary`） |
| `system` 各分型的附带字段 | ✅ `api_retry` 带 `attempt` / `max_retries` / `retry_delay_ms`；`model_queue_status` 带 `queue_type` / `wait_time_ms` |

本轮修正的问题：

- **§3「三型带公共字段」漏了 `system`**：`system` 同样带 `uuid` / `cwd` /
  `sessionId` / `isSidechain` / `timestamp` 等公共字段，应为四型。已改。
  > **撤销说明（五次元复核）**：本条初次撰写时还附带指控「原文把 `attachment`
  > 归进带 `message` 的一类」——**该指控不成立**。初稿只列了公共字段，从未把
  > `attachment` 与 `message` 关联；这是核对时的误读，已撤销。
- **§3「主/子条数拆分见上表」指向不存在的内容**：类型表当时只有合计列。已补齐
  主转录 / 子代理两列（并顺带给出 `system` 的三种子型，对解析压缩边界有意义）。
- **§8.2 / §10 用「`message` 类记录」指代 ISO8601 组不准**（此措辞系二次修订
  引入，非初稿）：该组含 `system`，而 `system` 没有 `message` 字段。
  已改为「对话类 + 状态类」。
- **§11「体积相差 1,440 字节」以偏概全**：1,440 只是 `app.asar` 的差值，
  worker runtime 差 93,861 字节。已改为逐行列出差值列。
- **§11 两处编辑残留**：段首「分析对象。」句子破碎；`app.asar` 那行写「见下」但
  下方已无该内容（实际在 §11 表格的 `asar.js` 行）。已一并修正。
- **§6 复核注与 §8.1 的张力**：前者称子代理「无法按 §5.3 还原」，后者称可借用主链
  窗口。已改为明确区分「严格还原做不到」与「借用窗口的未验证近似」。
- **§8.1「会话级四桶是服务端给出的权威值」与 §10「服务端语义未抓包验证」冲突**：
  已改为「客户端累加值、粒度上唯一可信」，把上游取证状态交回 §10。

### 五次元复核（2026-10-11，审计核验方法本身）

前四轮都是「验文档」，这一轮反过来「验验法」。方法三步：

1. **交叉验证核验器本身**：同一指标（`assistant` 记录数）用三种独立实现计算——
   逐行 `JSON.parse`、正则匹配、`Buffer` 按换行切分——三法均得 2558，一致。
   说明前几轮脚本无系统性偏差。
2. **穷尽重验**：把正文当成数据源，逐条抽出全部数字与事实断言，用单一脚本
   对照实测，**不做选择性抽样**。
3. **审计自己的历史指控**：把历轮报告过的每条「错误」与初稿原文（`git show
   HEAD:<file>`）逐条比对，判断指控是否真的成立。

结果：

| 批次 | 核验条数 | 失败 |
| --- | --- | --- |
| 第一批（记录类型 / anchor / 分组 / timestamp / ratio / credits） | 61 | **0** |
| 第二批（解密 / 闭合 / 文件属性 / 静态断言） | 67 | **0** |
| **合计** | **128** | **0** |

**即：文档中全部 128 条数据断言与实测逐条相符，无一例数字错。**

那么历轮「找到的错误」是什么？归因如下：

| 类别 | 条目 | 性质 |
| --- | --- | --- |
| 初稿真实缺陷 | `§3` 类型枚举漏 `file-history-snapshot` 与 `system`、`system` 计数 1→4、`§3`「三型」漏 `system`、`§5.3` 精度成因解释错、`§9`「永久不可用」与 `§4.3` 矛盾、`§8.1`「服务端给出的权威值」措辞过强、`§7` 左右同名歧义、`§2`「段」同词两义、`§8.1` 未覆盖子代理 | 真实，但多数是**引用与归属**问题，不是数字 |
| 依次修订**引入**的缺陷 | 首轮把 `§8.2` 时间戳由对改错；二轮把 `§11` 两份副本体积配错、`§3`「见上表」指向不存在的列、`§11`「见下」指向已删内容 | **改一次引入一次** |
| 核对时的**误读指控** | 「原文把 `attachment` 归进带 `message` 的一类」——初稿只列了公共字段，从未提及 `message`；该指控已撤销 | 核验方（我）的缺陷 |

**结论（方法论层面的教训）**：

- 「每次核实都能找到错误」的直接原因是**我一直在改同一份文档**。数据层从第一轮
  起就是稳的（128/128），增长的是编辑残留与引用失效——每轮改动都有概率引入新缺陷，
  而下一轮核对必然把它找出来。若文档冻结不再编辑，错误增量会立刻归零。
- 更该警惕的是第三类：**核对时带着「预期存在的错误模型」去读文本**，会把原文
  没说过的意思读成说过了。这类「错误」比真错更有害，因为它指向一个并不存在的
  缺陷，会诱导后续修订改坏原本正确的内容（首轮把 `§8.2` 改错即属此列）。
- 因此本文的核对原则定为：**只认机器证据能证伪的断言**；纯措辞与风格问题不计入
  「错误」，另行标注为「表述优化」。

### 仍未闭合的项（不声称已验证）

- **子代理窗口值**：通道 C 借用主链 `contextWindow` 只是**未验证假设**，无直接证据；
  拿到证据前不应把子代理 input 与会话级四桶合成同一条事实。
- **服务端语义**：`jb` 的 `resp.prompt_tokens` 来自服务端 gRPC 响应属推断，未抓包取证。
- **跨版本密钥**：仅 1.1.66 单版本样本，未做版本间对照。
