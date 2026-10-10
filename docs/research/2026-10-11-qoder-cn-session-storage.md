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
   （4/4 会话、0 差额，含 767 条请求记录的长会话）→ 逐请求 input token 可从**明文**转录
   无损还原。
5. Qoder 的 `input_tokens` 沿用 OpenAI 语义，**包含** `cache_read_input_tokens`；
   接入前必须做桶归一，否则与 Claude / Codex 口径混算会双计。
6. **本计划选定实现路径：解密 `state.json` 取会话级四桶权威值，配合转录
   `context_usage_ratio` 还原逐请求 input 明细。** 见 §8。

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
| `app/bundled-resources/v1/state.json`、`.models/<uid>/catalog-v6` | 插件资源与模型目录 | 目录为密文 |

- 项目目录名（slug）：`projectRoot.replace(/[^a-zA-Z0-9]/g, "-")`（长度超限时截断并追加
  djb2 变体 base36 后缀）。示例：`C:\work\alpha\beta` → `C--work-alpha-beta`。
- 全目录**没有任何 SQLite / 索引库**；会话列表来自目录扫描或服务端。
- 每个会话可有多个 `segments` 文件（每次进程启动一个 segment）。

## 3. 转录记录格式（明文 JSONL）

逐行一个对象，以 `type` 区分。记录类型白名单（自二进制还原，共 18 种）：

`user`、`assistant`、`system`、`attachment`、`summary`、`custom-title`、`ai-title`、
`last-prompt`、`tag`、`workspace-directories`、`runtime-config`、`mode`、
`content-replacement`、`file-history-snapshot`、`token-stats`、`active-leaf`、
`relocated`、`worktree-state`。

本机实际出现：`user`、`assistant`、`attachment`、`workspace-directories`、
`runtime-config`、`active-leaf`、`last-prompt`、`worktree-state`。

要点：

- `assistant` / `user` / `attachment` 三型带 `uuid`、`parentUuid`、`timestamp`、
  `cwd`、`sessionId`、`isSidechain`、`entrypoint`、`version`、`gitBranch`。
- `runtime-config` 提供 `model`（本机恒为 `qfmodel`）与 `contextWindow`（本机 1000000）。
- `active-leaf` 是分支叶子指针（`leafUuid` / `explicit`）。
- **一次 API 请求会产生 N 条 `assistant` 记录**（每个 content block 一条：
  `thinking` / `text` / `tool_use`），共享同一 `message.id`、同一 `timestamp`、
  同一 `requestTokenAnchor`；只有**最后一条**（`stop_reason` 非空）携带 `message.usage`。
  实测 2558 条带锚记录 / 1166 条带 `usage` 记录 / 1166 个唯一 `requestId` → 严格 1:1。
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
本机 `contextWindow = 1000000` 与之吻合。

**因此 `Math.round(context_usage_ratio × contextWindow)` 可无损还原该请求的
`input_tokens`**（窗口 1e6 与 ratio 的 6 位小数刚好互补，整除无余数）。

## 6. 交叉验证记录

逐请求 `context_usage_ratio × contextWindow` 求和 vs 解密后的 `total.input_tokens`：

| 会话（前 8 位） | 窗口 | 带 ratio 记录数 | Σ(ratio×窗口) | total.input_tokens | 差额 |
| --- | --- | --- | --- | --- | --- |
| `bd703276` | 1000000 | 28 | 1241572 | 1241572 | **0** |
| `c78f5543` | 1000000 | 47 | 4880510 | 4880510 | **0** |
| `d912f9c3` | 1000000 | 767 | 218454955 | 218454955 | **0** |
| `f67f12fd` | 1000000 | 93 | 10925529 | 10925529 | **0** |

四条独立证据互相印证：代码路径（`dv` 抹零 / `jb` 映射 / `L7A` 累加）、GCM 实际解密成功、
ratio 求和与解密值**逐会话精确闭合**、以及"`token-stats` 无调用方"与本机零命中的一致。

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

| TokenScope 桶（互斥） | Qoder 来源 |
| --- | --- |
| `input_tokens` | `input_tokens - cache_read_input_tokens`（下限 0） |
| `cache_read_input_tokens` | `cache_read_input_tokens` 原值 |
| `cache_creation_input_tokens` | 原值（本机恒为 0） |
| `output_tokens` | 原值 |

不归一就直接并入 Claude / Codex 汇总会**把缓存读双计一次**。

## 8. 实现路线（选定：解密 `state.json`）

### 8.1 取数设计

两条取数通道，粒度互补且**已被证明闭合**：

| 通道 | 来源 | 粒度 | 可得字段 |
| --- | --- | --- | --- |
| A（权威） | 解密 `state.json` 的 `total` | 会话 | 四桶全部 |
| B（明细） | 转录 `context_usage_ratio × contextWindow` | 请求 | `input_tokens`（含缓存） |

由 §6 的零差额可得：**通道 B 求和恒等于通道 A 的 `input_tokens`**。于是二者不能相加——
必须选定记账粒度。

**建议：按会话记账为事实，按请求记账为明细。** 理由：`output_tokens` 与
`cache_read_input_tokens` 只有会话粒度，任何"逐请求四桶"的构造都是伪造；而会话级
四桶是服务端给出的权威值。

具体落法：

- 每个会话产出 **1 条完整四桶事件**（来源 = `state.json.total`），请求数取
  `state.json` 的 `latest`/`total` 之外由转录锚定记录数统计（= 带 `usage` 的记录数）。
- 逐请求明细另存为**只带 input 的子条目**，或在明细视图标注"仅 input 精确、
  其余为会话级"。
- 会话级事件的时间戳：起点用 `state.json.createdAt`（或转录首个 `message` 记录时间），
  终点用 `updatedAt`；区间筛选按既有约定作用于会话区间。

### 8.2 必须保持的不变量

1. **只读**：绝不写入、移动、清理 `~/.qoder-cn` 任何文件。
2. **桶归一**：入库前必须执行 §7 的 `input - cache_read`（下限 0），并在明细中
   保留原始值以便核对。
3. **不重复记账**：`logs/sessions/**` 与 `projects/**` 的 `request_id` 完全重叠
   （本机交集 1166/1166），**不得同时采集**。
4. **请求身份**：唯一键取带 `usage` 记录的 `request_id`（=`requestTokenAnchor.requestId`）。
   按"每行一条"计数会虚增一倍以上。
5. **解密失败即降级**：主密钥随版本重编译可能变化。认证失败（
   `SessionStateDecryptionError`）时该会话标记为"用量未知"，不得静默补 0、
   不得回退到已抹零的转录 token 字段当作真值。
6. **时间口径**：`state.json` 的 `createdAt`/`updatedAt` 与转录 `timestamp` 均为 ISO8601
   UTC；存储层原样 UTC 持有，只做一次时区转换（沿用既有约定）。
7. **元数据尾部重写**：`runtime-config` / `workspace-directories` 按末次出现生效，
   不按出现次数计数。

### 8.3 任务清单（待立项后细化到测试名与验证命令）

| 编号 | 任务 | 关键点 |
| --- | --- | --- |
| Q01 | 路径发现 | 扫描 `~/.qoder-cn/projects/*/*/state.json` 与同名 `.jsonl`；目录不存在时静默零结果 |
| Q02 | `projectHash` 复现 | win32/posix 分支、大小写归一、`resolve` 语义；用本机 4 个会话做对照 |
| Q03 | GCM 解密 + 结构校验 | 严格校验 `{c,u,n,p,t}` 五元组；失败给结构化诊断而非抛栈 |
| Q04 | 会话级事件产出 | 四桶来自 `total`；请求数来自转录锚定记录 |
| Q05 | 逐请求 input 还原 | `round(ratio × contextWindow)`，窗口取自转录 `runtime-config` |
| Q06 | 桶归一与闭合断言 | `Σ(ratio×窗口) == total.input_tokens`，不等则报诊断 |
| Q07 | 合成 fixture 三件套 | 路径发现 / 典型解析 / 畸形行容错；**真实日志永不入库** |
| Q08 | 版本漂移探测 | 密钥失效时明确降级，不污染统计 |

### 8.4 验收标准

- 本机 4 个真实会话全部解密并产出事件，且逐会话 `Σ(ratio×窗口)` 与 `total.input_tokens`
  差额为 0。
- 桶归一后与"未归一"版本的费用/用量对比可解释（差值 = 缓存读被双计的部分）。
- 篡改 `state.json` 任一字段（`n` / `p` / `t` / `sessionId` / `projectHash`）必须
  认证失败并降级，不产生任何事件。
- 解密失败会话不计入未知为 0 的静默吞掉——沿用"未知不按 0 处理"的既有原则。

## 9. 备选路径与取舍

| 路径 | 可行性 | 结论 |
| --- | --- | --- |
| 解密 `state.json` | 可行，已实测 4/4 | **选定**：唯一能拿到四桶与 output 的通道 |
| 转录 `context_usage_ratio` | 可行，精确闭合 | 作为逐请求 input 的补充通道 |
| 转录 `usage` token 字段 | 恒为 0 | 永久不可用，不必再探测 |
| `credits` / `billable` | 口径不清（`state` 为 0、转录非 0、`billable` 恒 false） | 不作为计量口径 |
| 依赖 `QODERCN_*` 环境变量 | 需用户改启动环境 | 不作依赖，仅登记 |
| `logs/sessions/**` | token 同样抹零 | 不作数据源（且与转录重叠） |

## 10. 风险与未知

- **主密钥稳定性**：硬编码常量随发布重新编译可能改变。探测手段是解密认证是否通过；
  失败即降级。当前基于 1.1.66 单版本样本，未做跨版本对照。
- **服务端语义未抓包验证**：推断 `jb` 的 `resp.prompt_tokens` 来自服务端 gRPC 响应
  （proto 确有该字段），但未做流量级取证。`jb` 是全包中 `input_tokens` 的唯一赋值点，
  链条紧密，但严格说属于"一致解释"而非"已验证"。
- **窗口值来源**：`contextWindow` 取自转录 `runtime-config`；若某会话缺失该记录，
  ratio 还原不可用，须降级。
- **`input_tokens` 上限**：ratio 有 6 位小数，配合 1e6 窗口可无损还原 0–999,999；
  超过窗口的极端值会被 `Math.min(1, …)` 钳到 1.0（即 1e6），此时信息丢失。
  本机最大 ratio 0.596408，未触及该边界，但实现须处理。
- **多段（segments）**：`items` 可能有多个段键，本机只见 `s0`。多段语义未验证，
  实现时应对全部段键求和或明确拒绝。

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

分析对象：

- `app.asar`（约 141 MB）→ 顶层 `node_modules` / `out` / `package.json`
- `app.asar.unpacked/node_modules/@qoder-ai/qoder-cn-agent-sdk/dist/_worker/qoder-worker-runtime.obf.mjs`
  （31.6 MB / 781 行；**混淆只加密字符串字面量，标识符与逻辑全部明文**）
- 同目录 `proto/chat.proto`（gRPC 协议定义）、`runtime-info.json`（版本 / commit / 构建目标）

补充：本地服务端错误码表可用于诊断映射——`loginExpired:105`、
`todayUsageLimitExceeded:110`、`usageLimitExceeded:113`、`freeTrialAccountsExceeded:114`、
`freeUserQuotaLimit:115`、`personalCreditsDrainedOut:118`、
`billingGroupCreditsLimitReached:122`、`inputContentTooLong:80411`。配额由服务端强制。
