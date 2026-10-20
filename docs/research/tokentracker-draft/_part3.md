## 6. Claude Code 与 Codex 深度解剖

本章引用均相对 TokenTracker 仓库根（`xiufengsun/TokenTracker`，HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`），行号经两轮逐字复核。两条主线共用 `rollout.js` 这一副骨架，但身份、去重、缓存语义与缺陷形态几乎处处对立——这个对立本身就是本章最有信息量的部分。

### 6.1 Claude Code

数据流一条：发现 `~/.claude/projects/**/*.jsonl` → 逐行筛 `usage` → 归一成五桶 → 按 `message.id`（可选叠加 `requestId`）去重 → 用 `uuid`/`forkedFrom` 折叠 fork 会话 → 写进键为 `claude|<model>|<UTC 半小时>` 的桶。

#### 6.1.1 日志发现路径与 slug 编码

```js
// src/commands/sync.js:685
const claudeNativeHome = path.join(home, ".claude");
const wslClaudeHome = process.platform === "win32" && wsl.shouldProbeWsl(process.env)
  ? wsl.discoverWslHome(".claude")
  : null;
const claudeInstallHomes = [];
if (process.platform !== "win32" || wsl.shouldProbeNative(process.env)) {
  claudeInstallHomes.push(claudeNativeHome);
}
if (wslClaudeHome) claudeInstallHomes.push(wslClaudeHome);
```

再叠加 `CLAUDE_CONFIG_DIR` 与 `config.scanRoots.claude`（`:694-699`），取各安装根的 `projects/` 子目录并按 realpath 去重（`:700-703`）。**与 Codex 的立场相反**：Claude **UNION 全部安装**（native + WSL），不做 native-first/WSL-first 单选；同段注释（`:676-684`）给出理由——单选会让 WSL 抢占 native，而随后"磁盘即真值"的一次性修复会把 native 历史**抹掉**。

发现函数 `listClaudeProjectFiles` → `walkClaudeProjects`：递归任意深度、只认 `.jsonl`（`src/lib/rollout.js:5013-5023`）。slug 编码**不被解析**：目录名 `<dash-encoded-cwd>` 既是"不在真实项目 git checkout 内"（靠目录走永远找不到项目），本身也不可逆解码（真实路径里的连字符与路径分隔符编码冲突）——注释逐字见 `src/lib/rollout.js:4600-4606`，落点是常量 `CLAUDE_CWD_SCAN_MAX_BYTES = 65536`（`:4607`）。**结论：slug 只是目录名，不参与任何身份判定**；项目身份一律来自内容里的 `cwd`（6.1.8）。

子代理转录识别靠路径：`const isMainSession = !/[\\/]subagents[\\/]/.test(filePath);`（`:2803`）同时接受正反斜杠，理由是 WSL 经 `\\wsl$` 访问时 `path.join` 会产出反斜杠（`:2800-2802` 注释，issue #307）。跨根同文件两层去重：同一次运行内 `seenClaudeFiles` Set 按路径去重（`src/commands/sync.js:1137-1145`），跨环境同内容副本另由 `claudeFileIdentityHash` 处理（`src/lib/rollout.js:1021-1107`）。`claude-mem` 观察者目录有专属身份：常量 `CLAUDE_MEM_OBSERVER_PATH_SEGMENT = "--claude-mem-observer-sessions"`（`:58`），命中即赋予固定 `projectRef = "https://local.tokentracker/claude-mem/observer-sessions"`（`:59-60`、`:4297-4301`）——它以 URL 形式混进"远程仓库地址"那套项目标识；另有一次性迁移 `reincludeClaudeMemObserverFiles`（`src/commands/sync.js:1148`、`:7100`）。

一处常见误判需纠正：`parseClaudeScienceIncremental` **不是** Claude Code 的第二个解析器。它读 SQLite 的 `frames` 表（`src/lib/rollout.js:6841-6871`），库在 `~/.claude-science/operon-cli.db` 或 `operon.db`（`:6875`、`:6882`），多组织走 `orgs/<slug>/`（`:6892-6906`），Windows 无原生版须探 WSL（`:6910-6929`）；映射独立——`claudeScienceUncachedInput = input - cacheRead - cacheWrite`（`:6999`），`reasoning_output_tokens` 恒 0（`:7023`），合并用幂等减法（`:7097-7117`），`conversation_count` 只记根帧（`:7079-7085`），`source` 是字面量 `"claude-science"`（`:7101`）。

#### 6.1.2 逐行解析与五桶映射

```js
// src/lib/rollout.js:2841
if (!line.includes('"usage"')) continue;
let obj;
try {
  obj = JSON.parse(line);
} catch (_e) {
  continue;
}

if (isOmOClaudeSdkMirror(obj)) continue;

const usage = obj?.message?.usage || obj?.usage;
if (!usage || typeof usage !== "object") continue;
```

三点：字符串预筛省掉无 `usage` 行的解析；`usage` 双位置回退（`message.usage` 优先，顶层 `usage` 兜底，`:2851`）；**OmO 镜像行被剥离**（`:2849`，判定 `entrypoint === "sdk-ts" && userType === "external"`，`:4910-4912`）——那是 OmO Native 经 Claude Agent SDK 的镜像拷贝，剥离以避免与 `source=omo` 双计（`:4906-4909`，issue #726）。

五桶归一（`src/lib/rollout.js:4921`，节选）：

```js
const reasoningTokens = Math.min(outputTokens, toNonNegativeInt(
  u?.output_tokens_details?.thinking_tokens ?? u?.output_tokens_details?.reasoning_tokens,
));
const totalTokens = inputTokens + outputTokens + cacheCreation + cacheRead;
return {
  input_tokens: inputTokens,
  cached_input_tokens: cacheRead,
  cache_creation_input_tokens: cacheCreation,
  // Claude rows price reasoning separately; it is already included in usage.output_tokens.
  output_tokens: outputTokens - reasoningTokens,
  reasoning_output_tokens: reasoningTokens,
  total_tokens: totalTokens,
};
```

| 输出桶 | 来源字段 | 变换 |
|---|---|---|
| `input_tokens` | `usage.input_tokens` | 直取（Claude 原生语义已是"不含缓存"） |
| `cached_input_tokens` | `usage.cache_read_input_tokens` | 直取（缓存读） |
| `cache_creation_input_tokens` | `usage.cache_creation_input_tokens` | 直取（缓存写） |
| `output_tokens` | `usage.output_tokens` | **减去 reasoning** |
| `reasoning_output_tokens` | `output_tokens_details.thinking_tokens` 或 `.reasoning_tokens` | 取先存在者，`Math.min(outputTokens, …)` 封顶防倒挂 |
| `total_tokens` | — | `input + output(原始含推理) + cacheCreation + cacheRead` |

关键不对称：**`total_tokens` 用未减 reasoning 的 output 计算**，而 output 列已拆走推理——两列互补，保证 `input + cached + cacheCreate + output + reasoning == total`（`test/claude-reasoning-usage.test.js:44` 显式断言）。`toNonNegativeInt`（`:4964`）把非有限值与负数吸收为 0、其余 `Math.floor`，所以小数/负 token 都落成确定值。缓存字段只认两个名字：Claude 路径读 `cache_read_input_tokens` / `cache_creation_input_tokens`（`:4927-4928`），**不兼容** `cache_write_input_tokens` 别名；别名兜底只存在于 Codex 路径（`src/lib/codex-token-usage.js:25-27`）。

【未能确认】跨 provider（Kimi/DeepSeek/Mimo/MiniMax 等 anthropic 兼容端点）若用 `cache_write_input_tokens` 写缓存写量，Claude 路径会静默丢为 0。判定条件：统计 Claude 会话行里该键的出现率——同行为 `cache_write_input_tokens` 非 0 且缺 `cache_creation_input_tokens` 才成立；两者同时出现或同时缺失则实际影响为零。现无真实样本，只记为条件性风险。

零用量与写入判定（`:2861-2865`）：`normalizeClaudeUsage` 后 `isAllZeroUsage(delta)` 为真则整行跳过（`:4945` 定义，六桶全 0 才算），且 **assistant 行的 `conversation_count` 恒置 0**。会话计数只来自 user 文本行（`:2810-2831`）：仅主会话、`type === "user"`、`message.content` 是字符串或含 `{type:"text"}` 块之一 → `conversation_count += 1`，并记进 `unknown` 模型桶（`:2831` 的 `const userModel = DEFAULT_MODEL;`），与 token 桶分离；只有 `tool_result` 块的工具结果行被明确排除（`:2807-2809` 注释）。

#### 6.1.3 去重身份：`message.id` / `requestId`

```js
// src/lib/rollout.js:4895 区块（注释 + claudeMessageDedupKey）
// Stable dedup key for one Claude jsonl entry. Anthropic's official protocol
// guarantees `message.id` is globally unique per response, so msgId alone is a
// valid dedup key. Older code required both msgId AND requestId, which short-
// circuited dedup entirely for jsonl entries where `requestId` is absent ...
function claudeMessageDedupKey(obj) {
  const msgId = typeof obj?.message?.id === "string" && obj.message.id ? obj.message.id : null;
  if (!msgId) return null;
  const reqId = typeof obj?.requestId === "string" && obj.requestId ? obj.requestId : null;
  return reqId ? `${msgId}:${reqId}` : msgId;
}
```

规则与后果：无 `message.id` → 返回 `null` → **完全不去重**，该行必计；有 `requestId` 时键为 `msgId:reqId`，否则为裸 `msgId`，两种格式共用一个 Set 而不冲突（`msgId` 串不含 `:`）；`uuid` / `parentUuid` **不参与 token 去重**，只服务会话身份与 fork 折叠（6.1.4）。

注释里那段历史是本设计最硬的实证：旧代码要求 `msgId` **且** `reqId`，对缺 `requestId` 的行直接短路，导致受影响 provider **1.6–3.7 倍超算**（issue #64）。缺 `requestId` 的来源有两类——DeepSeek/Kimi/Mimo/MiniMax 的 anthropic 兼容端点不返回 `request-id` HTTP 头；Claude Code 的子代理与 thinking 传输路径也会丢该字段。持久化与上限（`src/lib/rollout.js:1233-1237`）：哈希集写回 `cursors.claudeHashes`，**上限 100k 且保留最后 100k**；超出时最早的哈希被挤出，理论上是批量重扫旧文件的重计窗口——注释（`:1021-1025`）承认该风险并因此加了文件级 `fileId` 去重层，而那层只在"同一次运行内同时见到 `\\wsl$` 与本地路径"时启用（`sawUncFile && sawLocalFile`，`:1035`）。

#### 6.1.4 fork 会话折叠：`uuid → forkedFrom.messageUuid`

```js
// src/lib/claude-user-identity.js:5
function noteClaudeFork(aliases, obj) {
  const uuid = obj?.uuid;
  const parent = obj?.forkedFrom?.messageUuid;
  if (typeof uuid === "string" && uuid && typeof parent === "string" && parent && uuid !== parent) {
    aliases[uuid] = parent;
  }
}
```

`claudeUserIdentity`（`:921-933`）沿别名链上溯到根，用 `u:<root>` 作身份；环（畸形别名）也能确定性折叠，且**与文件顺序无关**。约束四条：别名来自**顶层 `uuid` + `forkedFrom.messageUuid`**，且**只采信 `type === "user"` 的行**（`src/lib/claude-user-identity.js:44`）；子代理路径排除（`:31`）；身份前缀 `u:` 与消息键区分，历史哈希折叠只对 `startsWith("u:")` 生效（`src/lib/rollout.js:997-1003`）；别名表持久化在 `cursors.claudeForkAliases`（`:1234`）。

**预扫描 + 净值修正**（`src/lib/rollout.js:987-1015`）：`collectClaudeForkAliases` 在计数**之前**遍历完整批文件（"fork 文件可能排在前面"，`:987-988` 注释），随后把历史计数中"同一族多于 1 条"的部分从 `conversation_count` 里**减掉**，并有下界保护（`:952` 的 `bucket.totals.conversation_count < count - 1` 则跳过）。关键约束：**只修 `conversation_count`，不动 token**（`:992-994` 注释：`Preserve all token totals and history whose source files were removed.`）。

`test/claude-fork-conversations.test.js` 把语义钉死：

| 断言位置 | 含义 |
|---|---|
| `:37-39` | 三层链 `original ← copy ← nested` 只算 1 次会话、token 仍 30；结果与文件顺序无关 |
| `:40-43` | 游标序列化/反序列化后再跑，计数仍是 1/30（幂等） |
| `:44-45` | 与独立实现 `computeClaudeGroundTruthBuckets` 的会话数对账一致 |
| `:48-59` | 同一 fork 文件后追加的新 user 轮次独立计数（1 → 2），全量重跑不增长 |
| `:61-78` | 复现旧版污染态（计数 4 + 三个 `u:` 哈希混入），重扫后修复为 2、`total_tokens` 仍 30 |
| `:80-86` | 删掉原始文件、只剩 fork 副本时仍只算 1 次 |
| `:88-95` | 父文件**后来**才出现时，已计过的嵌套 fork 被回溯修正 |

#### 6.1.5 sidechain 与子代理：全部由路径表达

`sidechain` 字段在 token 统计路径上**零命中**——全库 `grep sidechain` 只命中 `src/lib/session-analytics.js:2660`，那是会话浏览器的注释。"子代理"概念完全由**文件路径**表达：会话计数排除 `.../subagents/...`（`src/lib/rollout.js:2803`、`:2810`）；fork 别名索引排除同路径（`src/lib/claude-user-identity.js:31`）；**token 计数不排除**——子代理转录里的 assistant usage 行照常计入，只是归属到该子代理文件所属的 cwd 项目。即：子代理的 token 是算的，子代理的"会话"是不算的；两个决定落在两个文件里，改一处不牵动另一处。

#### 6.1.6 模型名来源与哨兵值

```js
// src/lib/rollout.js:2857
const model = normalizeModelInput(obj?.message?.model || obj?.model) || DEFAULT_MODEL;
```

先 `message.model`，后顶层 `model`；`normalizeModelInput`（`:4260`）**只做 trim**，不改大小写、不做别名归一；无值 → `DEFAULT_MODEL = "unknown"`（`:56`）。因此 opus/sonnet/haiku 各变体、带日期后缀的版本串、非 Anthropic 路由串（如 `grok-4.5-build`）在这里**全部原样落桶**——归一化不在解析层，而在定价/分组链路，属另一条独立版本轴。

哨兵值 `<synthetic>` 在**解析层没有任何特判**，全库 3 处出现都在会话浏览器：`src/lib/session-analytics.js:101` 的集合、`:621-624` 注释（`They are not a billable model and can appear after the real assistant messages in the same session.`）、`:1968-1971` 注释（Claude Memory 观察者转录）。集合逐字：

```js
// src/lib/session-analytics.js:101
const PLACEHOLDER_MODELS = new Set(["<synthetic>", "synthetic", "<unknown>", "unknown"]);
```

后果：若某行 `message.model === "<synthetic>"` 且 usage 非零，它会照常生成 `claude|<synthetic>|<hour>` 桶（`:"<synthetic>"` 是真值字符串）；usage 全零则被 `isAllZeroUsage` 丢弃（`src/lib/rollout.js:2862`）。

【未能确认】真实 Claude Code 是否会为 `<synthetic>` 行写非零 usage——代码层无法判定，测试 fixture 也不含该形态。判定条件：在本机 `~/.claude/projects/**/*.jsonl` 中找 `"model":"<synthetic>"` 的行，看同行 usage 是否非零。若存在非零 usage，面板会出现名为 `<synthetic>` 的模型行——纯展示层后果，总量不变。

#### 6.1.7 时间戳、时区与半小时桶

| 环节 | 代码 | 说明 |
|---|---|---|
| 时间戳取值 | `src/lib/rollout.js:2858-2859` | 非字符串一律丢弃整行 |
| 桶生成 | `src/lib/rollout.js:2867` | `toUtcHalfHourStart(tokenTimestamp)`，非法 → `null` → `continue` |
| 桶键 | `src/lib/rollout.js:2872` | `bucketKey(source, model, bucketStart)` → `claude\|<model>\|<ISO>` |
| user 会话计数 | `src/lib/rollout.js:2828-2835` | 同一时间戳路径，模型固定 `unknown` |

桶算法（`src/lib/rollout.js:4173`）：全 UTC 对齐，分钟数 `>= 30` 取 `:30` 否则 `:00`，秒与毫秒清零，输出 `toISOString()`，非法时间戳返回 `null`。该算法在仓库内**存在两份实现**：`src/lib/rollout.js:4173`（主实现，被所有增量 parser 使用）与 `src/lib/claude-categorizer.js:1175`（真值修复专用）。两份**语义等价、文本不同**，且**没有任何测试锁定二者等价**——`test/` 下引用 `toUtcHalfHourStart` 的两处（`test/sync-droid-dup-session-repair.test.js:29`、`test/trae-entitlement.test.js:130-137`）都 `require("../src/lib/rollout")`，`claude-categorizer.js` 那份无人测；改动任一处不会让另一处失败，一旦分叉，增量解析与真值修复会写出两套时间分桶。

时区**事实**：解析层**不读取任何时区字段**，完全信任 `timestamp` 字符串自带偏移；仓库内**没有**"Claude 日志时区"配置项参与 `hour_start`；唯一与此沾边的显式代码是 WSL 路径映射 `wsl.mapWslCwdToUnc`（`:1161`），那管 cwd 不管时间。跨日**只发生在读侧**——写入侧桶只有 UTC 半时划分，`hour_start` 的日期部分永远是 UTC 日期，`rowDayKey`（`src/lib/local-api.js:213`）才把 UTC 桶映射到展示时区的自然日。**"桶归属"（写侧）与"展示日"（读侧）是两个正交阶段**，任何"跨日漂移"都必须在这两层之间解释——这是最容易误判的一点。

#### 6.1.8 项目上下文与 cwd 的角色

```js
// src/lib/rollout.js:4609
async function resolveClaudeFileCwd(filePath) {
  const stream = fssync.createReadStream(filePath, {
    encoding: "utf8", start: 0, end: CLAUDE_CWD_SCAN_MAX_BYTES,
  });
  const rl = readline.createInterface({ input: stream, crlfDelay: Infinity });
  try {
    for await (const line of rl) {
      if (!line || !line.includes('"cwd"')) continue;
      ...
      if (typeof obj?.cwd === "string" && obj.cwd.trim()) return obj.cwd.trim();
```

只扫**前 64KB**（`:4607`），命中即返回、**取第一条**；结果缓存在游标 `claudeCwd`（`:1200`），"确认不存在"用 `null` 显式表示，与 `undefined`（未探过）语义不同（`:1132` 的 `projectKnownAbsent`）。路径 → 项目 key：`resolveProjectContextForPath`（`:4549`）内部走 `resolveProjectMetaForPath`（`:4293`），自下而上找 `.git`；worktree/submodule 支持（`.git` 是文件时读 `gitdir:` 再读 `commondir`，`:4634-4668`）；远端 URL 优先 `origin`、否则取第一个（`:4690-4692`）；`canonicalizeProjectRef`（`:4695`）与 `deriveProjectKeyFromRef`（`:4429`）产出稳定 key；缓存按目录回填（`:4303-4312`）；WSL 侧 cwd 先过 `mapWslCwdToUnc`（`:1161`）再解析。

Claude 的**特殊之处**：`projectRef`/`projectKey` 是**文件级**的——一次解出后写进文件游标（`:1200-1203`），会话中途换目录不改变归属；Codex 相反，是**行级推进**（`turn_context.cwd` 变更即切，`:2496-2511`）。归属粒度不同，聚合时必须知道。

一处假设需纠正：`src/lib/claude-config.js`（279 行）与统计**零关系**，全部是 Claude Code `settings.json` 的 hook 增删改查——`CLAUDE_USAGE_EVENTS = ["Stop", "SessionEnd"]`（`:7-8`）、`upsertClaudeHook`（`:10`）、写盘前 `fs.copyFile` 备份成 `${settingsPath}.bak.<ISO>`（`:252-256`）；唯一与"解析"沾边的注释是泛化说明：CodeBuddy CLI 是 Claude Code fork、共用同一 hook schema（`:107-109`）。项目归属完全在 `rollout.js` 与根发现侧。

#### 6.1.9 已知缺陷 A：增量游标没有行边界提交

`const endOffset = st.size;` 在 `rollout.js` 中共 **3 处**（`:2291`、`:2682`、`:2793`），**只有第 3 处落到 token 游标**。三处分工必须分清，否则会把局部问题夸大成全局问题：

| 站点 | 所属 | 形态 | 是否落到 token 游标 |
|---|---|---|---|
| `:2291` | `parseRolloutFile`（`:2264`，Codex 单文件主解析） | 等于 `st.size`，仅用于 `startOffset >= endOffset` 的"无新字节"早退（`:2294-2302`）；数据没读就返回，返回值不可能大于入参 | **否**（正式返回走 `committedEndOffset`，`:2652`） |
| `:2682` | `scanRolloutProjectFileContexts`（`:2666`，Codex 项目上下文扫描） | `return { endOffset: committedEndOffset, … }`（`:2764-2765`） | **否**（只推进 `projectOffset`） |
| `:2793` | `parseClaudeFile`（`:2776`，Claude 单文件主解析） | `return { endOffset, eventsAggregated }`（`:2889`）→ `cursors.files[key].offset = result.endOffset`（`:1194`） | **是——唯一一处** |

**非对称性**（缺陷本体）：Codex 的 token 路径提交 `committedEndOffset`（`:2652`），只推进到最后一个完整行；Codex **连项目上下文扫描位都做了同样的提交保护**（`:2765`）；OpenClaw 另有显式保护 `openclawOffsetEndsAtLineBoundary()`（`:1949`），OMP 族有 `maxRecordBytes`；**只有 Claude 这条路径完全没有**。

后果链：同步瞬间文件正在被写、尾行未闭合时，`readline` 会把未闭合尾行也 `yield` 出来（`crlfDelay: Infinity`），`JSON.parse` 失败被 `continue` 吞掉（`:2843-2847`），而 `offset` 仍推进到 `st.size`。**确切范围**（必须收窄，不能写成"永久丢失"）：该行的字节**在本代文件内不可恢复**；该文件下一次 inode 变化或截断时会走三态判定重置路径 `startOffset = 0` 全量重扫（`:488-504`），届时该行会被补上。

复核动作：构造"写入半行 JSONL → 跑 `parseClaudeIncremental` → 补全该行 → 再跑"的定向测试，看补全后的那行是否被计入；若未计入，即为确定性数据丢失 bug，不是理论风险。

#### 6.1.10 已知不一致 B：一次性真值修复与增量解析的口径分叉

`src/lib/claude-categorizer.js` 有两个截然不同的职责，只有一个影响统计。**职责一**是 Context Breakdown 分类视图（读侧，不写队列）：文件头注释逐字 `Computes on demand — no queue schema changes, no parser changes, no sync changes.`（`:2-13`），七个分类键（`:33-41`）为 `system_prefix, conversation_history, user_input, tool_calls, subagents, reasoning, assistant_response`，分配靠字符长度比例（`splitOutputByContent`，`:367`）与"首个 cache_creation 视为 system_prefix"（`:378-380`），由 `local-api.js:69` 引入给面板。**职责二**是 `computeClaudeGroundTruthBuckets`——覆盖统计的一次性真值修复（`:1193`）。

触发与破坏面：只在**全量扫描**分支调用（`src/commands/sync.js:1183-1190`），且**只跑一次**（迁移键 `"claudeGroundTruthRepair_2026_05_v4"`，`src/commands/sync.js:248`）；**原子重写 `queue.jsonl`**——保留非 claude 行、删除所有 `source === "claude"` 与 `"claude-mem"` 行、追加真值行，`tmp + rename`（`:6871-6921`）；**清空** `cursors.hourly.buckets` 与 `groupQueued` 中所有 `claude|` / `claude-mem|` 前缀键（`:6930-6943`）；整体替换 `cursors.claudeHashes`（注释 `:6858-6861`：`clears all claude buckets and replaces claudeHashes wholesale`）；防护是"若某配置根当前不可读/不存在而历史上供过文件，则放弃本次修复"（迁移键不写、留待后续，避免把历史抹成空，`:1149-1182`，判定函数 `rootPreviouslySuppliedFiles`，`:1163-1166`）。

口径差异（"不一致"的全部内容）：

| 维度 | `parseClaudeFile`（增量，日常路径） | `computeClaudeGroundTruthBuckets`（一次性修复） |
|---|---|---|
| usage 取值 | `obj?.message?.usage \|\| obj?.usage`（`src/lib/rollout.js:2851`） | **只有** `obj?.message?.usage`（`src/lib/claude-categorizer.js:1267`） |
| reasoning 来源 | `usage.output_tokens_details.thinking_tokens ?? .reasoning_tokens`（`:4924-4926`） | `usage.reasoning_output_tokens`（`:69`）——**另一个字段名** |
| output 与 reasoning | `output_tokens = 原始 - reasoning`（拆分） | `output_tokens = 原始`（不拆），reasoning 另记 |
| `total_tokens` | `in + out(含推理) + cacheC + cacheR` | `in + cacheR + cacheC + out`（`:70-71`）——数值一致 |
| 身份折叠 | 用 `forkAliases` 折叠 user 身份（`:996-1013`） | 同样折叠（`:1214`、`:1246`），但**不看历史游标**，从零重建 |
| 未知模型行 | `DEFAULT_MODEL`（`"unknown"`） | 字面量 `"unknown"`（`:1277`），同值 |

结论：**总量口径相容**（分母相同：input + 原样 output + 两个缓存桶），但**列分布不同**——修复后 `reasoning_output_tokens` 通常变成 0、`output_tokens` 相应变大（等于把推理并回 output）。由于 `computeRowCost` 只依赖五列之和（`test/claude-reasoning-usage.test.js:45-47` 用 `< 1e-12` 断言列分布可换、费用不变），**费用不变**；两个副作用则是确定的：**"推理 token"这一展示列会在修复后归零**，且 `obj.usage` 兜底路径的数据在真值修复中**会消失**。修复不可逆（原子重写 + 哈希全替换），受影响安装无法回到修复前的列分布。复核动作：同一份合成 fixture 跑两条路径逐列比对（预期 total 相同、`output_tokens` 与 `reasoning_output_tokens` 不同）。

---

### 6.2 Codex

Codex 的世界模型与 Claude 相反：**没有"响应级 usage 行"，只有会话级累计快照**，所有增量靠减法恢复；一份 rollout 里还可能同时存在**多条累计谱系**。

#### 6.2.1 日志发现路径与扫描调度

```js
// src/commands/sync.js:645
const codexNativeValue = resolveEnvRoot("codex", { env: process.env, home }) || path.join(home, ".codex");
```

```js
// src/commands/sync.js:782
const codexPaths = resolveInstallPaths({
  nativeValue: codexNativeValue,
  wslDir: ".codex",
  requireAnyChild: ["sessions", "archived_sessions"],
  union: true,
});
```

随后按"根 × (sessions | archived_sessions)"推入 sources（`:788-810`）：`sessions` 总是扫，`archived_sessions` 带 `deep: true`（`:885` 的 `listRolloutFilesDeep`，因为归档目录是扁平的，`src/lib/rollout.js:195-200` 注释，issue #187），且**后台轻量同步默认不扫归档**（除非首轮历史迁移，`:790`、`:807`）。两个要点：`union: true` 表示 native + WSL 都要（与 Claude 的 UNION 同构，但走 `resolveInstallPaths`）；`requireAnyChild: ["sessions", "archived_sessions"]` 防"空壳 `~/.codex` 抢占"（`:762-772` 注释，issue `codex-wsl-shadow`）。

命名与日期：文件名 `rollout-<ISO 时间戳>-<uuid>.jsonl`；日期取文件名前缀（`src/lib/rollout.js:4192` 的 `rolloutDateFromPath`，正则 `^rollout-(\d{4}-\d{2}-\d{2})T`）；会话 id 从文件名尾部 uuid 抓（`codexSessionIdFromPath`，`:2256-2262`）；目录是严格 `YYYY/MM/DD`，年/月/日各自用 `^[0-9]{4}$` / `^[0-9]{2}$` 校验（`:107-131`）。

**冷文件跳过**（`filterColdCodexRolloutFiles`，`src/lib/rollout.js:775-857`）是 Codex 独有的调度优化，五步：名字日期落在活跃窗口内（默认今天起 2 天 + 全库最大日期，`activeCodexRolloutDates`，`:893-921`）→ 必扫；`auditDue`（每天一次冷审计）→ 必扫；近 30 天内的冷文件 `fs.stat` 一次、**比游标 offset 大就扫**（`:808-822`）；否则问游标 store `canSkipCodexDay`（`:824-827`），可跳则 `skipped += 1`；**cursor store 重启时整批回退**（`if (cursorStoreRestarted) { … return { rolloutFiles: files, skipped: 0, restarted: true }; }`，`:859-869`）。回归点写在文件注释里：`#592`（会话跨天仍在增长）与 `#187`（归档搬迁）（`:63-69`、`:806-807`）。

#### 6.2.2 事件类型清单与 `token_count` 真相事件

```js
// src/lib/rollout.js:4838
function extractTokenCount(obj) {
  const payload = obj?.payload;
  if (!payload) return null;
  if (payload.type === "token_count") {
    return { info: payload.info, timestamp: obj?.timestamp || null };
  }
  const msg = payload.msg;
  if (msg && msg.type === "token_count") {
    return { info: msg.info, timestamp: obj?.timestamp || null };
  }
  return null;
}
```

**两种信封**：`payload.type === "token_count"`（主流）与 `payload.msg.type === "token_count"`（旧/嵌套形态）。按解析路径实证的事件清单：

| 事件 | 匹配方式 | 作用 | 行号 |
|---|---|---|---|
| `event_msg/token_count` | `line.includes('"token_count"')` | **唯一用量来源** | `src/lib/rollout.js:2427`、`:4838` |
| `turn_context` | 含 `"model"`/`"cwd"`/`"current_date"`/`"forked_from_id"` 之一 | 推进模型、cwd、当前日期 | `:2431-2437`、`:2485-2513` |
| `session_meta` | 同上 | fork 标记（`forked_from_id`）、弱模型回退 | `:2490-2492` |
| `event_msg/thread_settings_applied` | `line.includes('"thread_settings_applied"')` | service tier（`default`/`priority`/`fast`） | `:2438-2440`、`:2479-2483` |
| `token_usage_record` | `line.includes('"token_usage_record"')` | **只用于 compaction 计费** | `:2441-2446`、`:2515-2520` |
| `compacted` | 紧随 record 的下一条完整行 | compaction 判定 | `:2410-2425` |
| `task_complete` / `turn_aborted` | `event_msg.payload.type` | 结束 record 的等待态 | `src/lib/codex-usage-record.js:82-86` |
| `model/rerouted` | 8 种信封形态枚举 | **前瞻**（真实语料 0 命中） | `src/lib/codex-model-attribution.js:33-66` |
| `response_item/function_call`、`event_msg/exec_command_end` | 仅在 breakdown 采集时 | 工具归因（不影响 token） | `src/lib/codex-rollout-parser.js:1049-1060` |

性能设计是"先字符串包含、后 `JSON.parse`"，并有一个兜底分支保证未匹配行只要合法 JSON 也推进提交位（`:2447-2464`）：

```js
// src/lib/rollout.js:2454
if (invalidRecordPolicy === "throw" || !record.terminated) {
  try {
    JSON.parse(line);
    committedEndOffset = scannedEndOffset;
  } catch (error) {
    if (invalidRecordPolicy === "throw") throw error;
    break;
  }
}
```

注意 `break`：**遇到未闭合的不合法行会停止本轮读取**，保留可提交前缀——这正是 Claude 侧缺失的那层保护。落桶字段路径（`:2530-2554`）：

```js
// src/lib/rollout.js:2530
const info = token.info;
if (!info || typeof info !== "object") continue;
...
const lastUsage = info.last_token_usage;
const totalUsage = info.total_token_usage;
if (totalUsage && typeof totalUsage === "object") latestTotal = totalUsage;

const rawDelta = consumeUsageDelta(usageDeltaState, lastUsage, totalUsage);
```

即真相事件的两个字段：**`payload.info.last_token_usage`（本轮增量）+ `payload.info.total_token_usage`（会话累计）**。规范形状由测试 fixture 固化（`test/codex-union-cursor-divergence.test.js:79-85`），其中 `usageOf`（`:76-78`）给出原生字段名：`input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens, total_tokens`（外加 `cache_creation_input_tokens` 及其别名 `cache_write_input_tokens`）。

#### 6.2.3 差值引擎：`last_token_usage` vs `total_token_usage`

```js
// src/lib/codex-token-usage.js:3
// A rollout can contain TokenCount events from more than one Codex SessionState
// (for example, a parent agent and an embedded reviewer). Each SessionState has
// its own cumulative total, but the event does not carry a stable stream id.
// Recover the stream from Codex's invariant: total - last = that stream's prior
// total. Keeping a small LRU of stream heads also makes repeated rate-limit
// snapshots idempotent when the streams are interleaved.
```

状态与上限（`:10-18`）：六桶字段表 + `const MAX_USAGE_BASELINES = 32;`——**LRU 流头谱系最多 32 条**。判定流程（`:98-158`）四分支：**重复快照**时 `total` 命中已知流头 → 返回 `null`（**不重复计**）并置 `sawInterleaved`；**正常增长**时 `last` 存在且 `total - last` 命中某个流头 → 返回 `last`（把 `last_token_usage` 当本轮增量）；**`total - last` 不命中任何流头但仍可减** → 置 `sawDivergentCumulative` 后返回 `last`（偏向"少算"而非虚高）；**无 `last` 或流断裂** → 用 `diffUsage(total, activeHead)` 取差，若差值比 `last` 还大则判为新流（`:150-153` 注释：`A cumulative jump larger than last usage cannot come from the normal append_last_usage path`）。`subtractUsage` 的硬约束（`:49-59`）：任一字段 `total < last` 即返回 `null`——对溢出/回绕的**显式拒绝**，不是饱和。

测试固化（`test/codex-token-usage.test.js`）：交错两谱系时增量序列为 `[100, 200, null, 50, 30]`（第三个重复快照不计）且 `sawInterleaved === true`、基线数 2（`:24-38`）；单谱系 `[100, 50, 25]`、两个告警位 false、基线恒 1（`:40-48`）；`last` 与累计不一致时取**较小的累计差**（`:50-57`）；基线数被截断到 32（构造 37 个流头，`:59-67`）。

`total` 与 `last` 的选择顺序：`total_token_usage` 非空对象时刷新 `latestTotal`（`src/lib/rollout.js:2547`）——它是**文件级权威累计**；增量取 `consumeUsageDelta(state, lastUsage, totalUsage)`；若事件推进了累计谱系但被判重复，返回 `null` 且**游标/基线仍已推进**（注释 `:2570-2571`：`usageDeltaState is already advanced above`），保证后续行血缘正确。测试语义（`test/rollout-parser.test.js:104-158`）：4 条 `token_count`、第 2 与第 4 条是同一累计值的重复推送，断言 `eventsAggregated === 2`、`bucketsQueued === 1`、`model === "unknown"`、队列合计等于 `usage1 + usage2` 的 `total_tokens` 之和；注释逐字：`The cumulative total_token_usage value is authoritative for a file; if it did not move, the repeated last_token_usage must not be counted again.`（`:105-107`）

#### 6.2.4 compaction 计费：`token_usage_record`

```js
// src/lib/codex-usage-record.js:10
// Compaction. Codex bills the compaction model call in a record that is
// immediately followed by a top-level `compacted` line, and the next
// token_count repeats the previous cumulative total (its last_token_usage is
// the post-compaction context size, not a response). The cumulative delta is
// zero, so without the record the call is lost.
```

规则：计数条件 = `compacted` 紧邻 + 下一个 `token_count` 的 `total_token_usage` 与上一次**完全相等**（`sameUsage(previousTotal, totalUsage)`，`:150`）；去重键是 `payload.response_id`，**跨 fork 重放与文件复制稳定**（`:19-21`），上限 20000（`:60`）；无 `response_id` 的 record **永不计**（`:20-21`、`src/lib/rollout.js:2356`）；未被任何 `token_count` 匹配的文件被标 `recordOnly` 并留在 `cursors.codexUsageRecordOnlyFiles` 供 sync 告警（`src/lib/rollout.js:649-655`；`codex-usage-record.js:174-177`）；`payload.turn_token_usage` / `thread_token_usage` **从不读取**（`:31-32`）。这是一处易漏语义：compaction 那次模型调用的 token 在累计曲线上**不可见**（累计没动），只能靠"记录 + 下一条 total 相等"这条侧信道捞回；漏掉即少算、重复捞即多算，所以去重键必须跨文件复制稳定。

#### 6.2.5 归一化：`input -= cached` 与零值重置哨兵

```js
// src/lib/rollout.js:4864
// Codex rollouts (and Every Code, which shares the format) report
// `input_tokens` as the TOTAL prompt, with `cached_input_tokens` as the
// cached subset — i.e. the cached slice is INSIDE the input count. Our
// queue schema (CLAUDE.md → Token Normalization Convention) stores
// `input_tokens` as pure non-cached input and `cached_input_tokens`
// separately. Without this subtraction the cost formula bills the cached
// bytes twice: once at the full input rate and again at the cache_read
// rate, producing ~6–7x cost inflation on cache-heavy Codex sessions
// (verified against ccusage's per-day numbers on the same rollouts).
...
out.input_tokens = Math.max(0, out.input_tokens - out.cached_input_tokens);
```

这段注释是本章最硬的一处定量证据：不做减法会在缓存重的会话上产生 **约 6–7 倍的费用膨胀**（同一批 rollout 与 ccusage 的按日数字对账）。`Math.max(0, …)` 保下界，`normalizeUsage`（`:4851-4887`）对六桶只做 `Math.floor` 与非负吸收。

| 队列列 | 来源 | 变换 |
|---|---|---|
| `input_tokens` | `input_tokens` | **减 `cached_input_tokens`** |
| `cached_input_tokens` | `cached_input_tokens` | 直取（缓存读） |
| `cache_creation_input_tokens` | `cache_creation_input_tokens` **或** `cache_write_input_tokens` | 直取（写法兼容见 `src/lib/codex-token-usage.js:25-27`） |
| `output_tokens` | `output_tokens` | 直取，**不减 reasoning** |
| `reasoning_output_tokens` | `reasoning_output_tokens` | 直取、不拆列（子集标记，见 6.2.6） |
| `total_tokens` | `total_tokens` | 直取（保留上游口径，不重算） |

**零值重置哨兵**（`:4881-4893`）：Codex 偶发写入"五个分量全 0、仅 `total_tokens` 非 0"的伪事件，在归一化**之前**即被拒绝，避免污染 `input - cached`：

```js
// src/lib/rollout.js:4881（节选）
const componentTotal = (usage) => [
  usage?.input_tokens,
  usage?.cached_input_tokens,
  usage?.cache_creation_input_tokens ?? usage?.cache_write_input_tokens,
  usage?.output_tokens,
  usage?.reasoning_output_tokens,
].reduce((sum, value) => sum + Math.max(0, Number(value) || 0), 0);
return Number(lastUsage?.total_tokens || 0) > 0
  && Number(totalUsage?.total_tokens || 0) === 0
  && componentTotal(lastUsage) === 0
  && componentTotal(totalUsage) === 0;
```

用法在 `src/lib/rollout.js:2550-2553`：**仅在 `source === "codex"` 时启用**。**会话计数的语义分叉**同样在此落定：Codex 侧每个通过零值闸门并被接受的 delta 都写 `delta.conversation_count = 1`（`:2554`），Claude 侧 assistant 行恒 0、只有主会话 user 文本行 +1（`:2865`、`:2807-2831`）——两条主线在桶里用同一列名、求和方式相同，但**含义不可互换**：Codex 的累计值实际是"被接受的 token 事件数"，Claude 的才是"用户轮次数"。

#### 6.2.6 reasoning 是 output 的子集标记（与 Claude 相反）

解析侧对 `reasoning_output_tokens` **没有任何算术改写**（`src/lib/rollout.js:4851-4879`，没有 `output -= reasoning` 这类行）；注释 `:4873-4876` 逐字写着保留 `total` 是为了兼容 `older Codex / Every Code shapes where output_tokens can exclude reasoning`——即上游默认 **output 含 reasoning**，本工具只是原样保留。计价侧对此显式置 0（`src/lib/pricing/index.js:248-256`）：

```js
// src/lib/pricing/index.js:248
const reasoningIncludedInOutput =
  row.source === "codex" ||
  row.source === "acode" ||
  row.source === "every-code" ||
  row.source === "omo" ||
  row.source === "cline";
const reasoningCost = reasoningIncludedInOutput
  ? 0
  : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
```

变量名本身就是判据："reasoning **已含于** output"。`computeRowCost` 中 reasoning 相关项共 3 处（`:254` 基础费、`:297-299` 长上下文溢价、`:325-327` priority 溢价），**三处全部用同一个门控**——没有漏掉、也没有反向叠加，因此不存在双计路径。

Claude 侧写法**相反**（`src/lib/rollout.js:4930-4939`，逐字节选）：

```js
// Claude rows price reasoning separately; it is already included in usage.output_tokens.
output_tokens: outputTokens - reasoningTokens,
reasoning_output_tokens: reasoningTokens,
```

Claude 原生 output **同样**包含推理，但工具选择**显式拆出**，拆出后 reasoning 列**照 output 单价计费**（白名单不含 `claude`），`total_tokens` 仍用未减的 output 计算。两侧写法相反，恰好互证"不拆" = 已含在内：Codex 是"不拆列 + 计价置 0"（子集标记），Claude 是"拆列 + 照 output 价"（独立加成项）。**统一口径**：喂给 `computeRowCost` 的都是**五列之和**，`total_tokens` **永不参与计费**（`CLAUDE.md:60-64`；`test/claude-reasoning-usage.test.js:45-47` 用 `< 1e-12` 断言列分布可换、费用不变）。

一处真实风险：Codex 侧"不双计"依赖 `source` 白名单**硬编码**（`codex`/`acode`/`every-code`/`omo`/`cline`），一旦上游新增 `*-code` 变体而白名单漏加，就会**静默双计**每一条 reasoning token，且现有测试全绿。判定与加固：补一条对等断言——`computeRowCost({source:"codex", output_tokens:518, reasoning_output_tokens:163})` 与同参数但 reasoning 为 0 的结果差 `< 1e-12`；并把白名单改为结构性判定（如 `source.endsWith("-code") || source === "codex"`）。

#### 6.2.7 模型唯一权威：`turn_context.model`

```js
// src/lib/codex-model-attribution.js:3
// Codex token_count events do not carry a model id. Every usage event is
// therefore attributed to the model named by the most recent turn_context,
// which Codex emits at the start of each turn and which tracks mid-session
// model switches exactly. That per-turn path is what actually runs today.
// ...
// Deliberately NOT used for attribution: event_msg/thread_settings_applied.
// It carries thread_settings.model, but it fires when the user picks a model
// in the UI - seconds to minutes before the switch takes effect - so usage
// still streaming from the in-flight turn belongs to the previous model.
// turn_context is the only signal that marks where a turn actually begins.
```

规则：`turn_context.payload.model` → `selectedModel`，并清 `rerouted`（`:82-92`）；`session_meta.payload.model` **只在 `selectedModel` 为空时**采纳，且**永不覆盖** `turn_context`（`:96-102`，注释指出 fork 会重放父级 `session_meta`）；`thread_settings_applied` **被刻意排除在模型归属之外**（`:11-15`）——UI 选择领先生效数秒到分钟，正在流出的用量仍属上一模型。实测量级（注释给出）：`0 of 10310 session_meta rows across 5849 local rollouts carry the field (codex-cli 0.151.0)`（`:18-20`）。调用点在 `src/lib/rollout.js:2476-2477`，模型状态**持久化进文件游标**（`modelAttributionState`，`:639`），跨增量续用，初值由 `lastModel` 注入（`:2320-2322`）。

【未能确认】`model/rerouted` 分支从未在真实语料验证（`src/lib/codex-model-attribution.js:47-51` 自述命中 0）；一旦被真实触发，其 8 种信封形态与字段名（`fromModel/toModel/from_model/to_model`）都属推断。判定条件：检索本机 rollout 中是否出现 `model/rerouted` / `model_rerouted`。

#### 6.2.8 provider / base_url 不进桶键

队列行**没有 provider 列**（行格式为 `source, model, hour_start, 六桶, total, billable, cost, conversation_count`）；`source` 常量对 Codex 家族固定为 `"codex"` / `"acode"` / `"every-code"`，由 `sync.js` 组装文件条目时决定（`src/commands/sync.js:894`、`:789`）。`session_meta.payload.model_provider`（如 `"openai"`）在**用量路径**上完全未被读取——`src/lib/codex-model-attribution.js:19-23` 明确称其为 `provenance rather than a model`；使用 `model_provider` 的是**会话浏览器 / breakdown** 侧（`src/lib/codex-rollout-parser.js:1023`、`src/lib/session-analytics.js:822-834`），且带防护：

```js
// src/lib/session-analytics.js:822
// Older Codex rollouts can omit turn_context.model. The shared parser then
// falls back to model_provider (for example "openai"), which is provenance
// rather than a model and must not become a model-table row.
```

全库对 `azure` 的检索在 `src/lib/` 下**零命中**；`base_url` 命中都属网络/遥测配置（`src/lib/doctor.js:62`、`src/lib/runtime-config.js:78`），与 Codex 日志解析无关。**结论**：第三方 base_url / Azure 路由的 Codex 会话，用量**按 `turn_context` 声明的模型串原样落进 `codex|<model串>|<hour>`**，没有任何 provider 维度参与身份或去重，同模型的跨渠道请求合并到同一桶——这与"能区分不同渠道"的直觉预期相反，且当前 schema **没有位置可放**该维度。

#### 6.2.9 service tier：子集列，不参与求和

```js
// src/lib/codex-service-tier.js:17
const CODEX_SERVICE_TIER_MARKER = '"thread_settings_applied"';
const PRIORITY_SERVICE_TIERS = new Set(["priority", "fast"]);
```

归属规则：`a token_count row belongs to the most recent preceding thread_settings_applied`（`:12-13`）；无前置记录即 `null`，`null` **按 Standard 计**，从不猜（`:12-15`）。落桶是**子集标记**而非独立桶（`src/lib/rollout.js:4073-4088` 的 `PRIORITY_SUBSET_FIELDS`：`priority_input_tokens` / `priority_cached_input_tokens` / `priority_cache_creation_input_tokens` / `priority_output_tokens` / `priority_reasoning_output_tokens`）；注释（`:4067-4072`）强调这些列**永不参与任何求和**，`total_tokens` 仍等于五桶之和，为零时整列省略。

【未能确认】覆盖率数字（1213 个本地文件中 167 个带该事件、覆盖 25% 的 `token_count`；`default` 814 / `priority` 58）出自上游源码注释里的**维护者本机语料观测**，非本次实测，不能外推为任意用户环境。判定条件：在本机 `~/.codex/sessions/**` 重跑同样计数（分母 = 含 `token_count` 的文件数，分子 = 含 `thread_settings_applied` 的文件数）。代码侧规则与结构（归属、`null` 按 Standard、子集列不参与求和）本次逐行核对通过。

#### 6.2.10 家族关系：Every Code / OmO / acode / cline

Codex rollout 不是 Codex 独占的格式：归一化注释直接点名 `Codex rollouts (and Every Code, which shares the format)`（`src/lib/rollout.js:4864-4865`），`:4873-4876` 又提到兼容 `older Codex / Every Code shapes where output_tokens can exclude reasoning`；计价白名单把 `codex`、`acode`、`every-code`、`omo`、`cline` **并列**为"reasoning 已含于 output"（`src/lib/pricing/index.js:248-256`）；OmO 在 Claude 侧还有镜像防护——`entrypoint === "sdk-ts" && userType === "external"` 的行被解析层剥离，避免与 `source=omo` 双计（`src/lib/rollout.js:4906-4912`，issue #726）。意义：**语义只有一份实现**，差值、模型归属、档位、compaction 四个核心状态机被整个家族复用，家族成员只在 `source` 常量上分流——这也解释了白名单漏加成员为何会静默错账（6.2.6）。

需区分的是：`src/lib/codex-rollout-parser.js`（52451 字节）与 `src/lib/codex-context-breakdown.js`（27929 字节）默认**不影响统计**——两者对 `queuePath` / `appendFile` / `enqueueTouchedBuckets` 的检索零命中（"不写队列"已闭合），消费者是只读视图（`src/lib/local-api.js:71`、`src/lib/session-analytics.js:32`），`codex-rollout-parser.js:1-4` 自述 `Does NOT hold any aggregation state`。

【未能确认】"不影响统计"这一更强结论依赖前提：`src/commands/sync.js:198` 借用其 `extractTokenCount` 的那条路径是**一次性迁移**（6.2.11），而该迁移是否在某些安装上**永不触发**取决于运行时游标状态（迁移键是否已置位、文件是否被判受影响），源码层判定不了。判定条件：读一个真实 `~/.tokenscope/cursors.json`，看该迁移键是否已置位；置位则本条影响面为零。

#### 6.2.11 血缘修复三类：rescan inflation / fork replay / interleaved usage

Codex 侧所有"少算/多算"防线都在修补同一件事：**累计谱系的连续性**。三类触发、修法、对账方式如下。

| 类别 | 触发 | 修法 | 对账方式 |
|---|---|---|---|
| **rescan inflation**（重扫膨胀） | inode 变化导致从 0 重扫，所有 delta 会重新加进**持久化桶**；外部工具（Codex-Manager 切换渠道）用原子重写改 inode，没有防线就每次切换翻倍（`src/lib/rollout.js:2608-2618` 注释） | 事件层去重键 `sessionUUID:eventTimestamp`（`:2626`），配两级追踪器：`getAppendOnlyEvents()`（本次运行新写，稳态快路径）与 `getHistoricalEvents()`（持久化集合或分片 store，重扫/换路径/union 用）（`:302-380`）；选择器 `needsHistoricalRolloutDedup()` 的判定顺序（`:428-461`）至关重要——**同会话换游标路径的判定必须在稳态短路之前** | e2e：`test/codex-union-cursor-divergence.test.js` 两个用例真跑 `cmdSync([], {})`，读真实 `queue.jsonl` 后按 `source\|model\|hour_start` **取最后一条**求和（`:104-112`）；断言 120 → 220 → **仍是 220**（滞后根不得重放，`:118-153`、`:155-189`） |
| **fork replay**（fork 重放） | `isForkedRollout` 且回放旧轮次：跨日场景 `currentDate < rolloutDate`；同日场景为毫秒级爆发推送 | 两重机制（`:2556-2603`）：跨日日期守卫 `isForkedReplayToken({isForkedRollout, rolloutDate, currentDate})`（`:4212`）；同日子毫秒爆发检测 `FORK_REPLAY_GAP_MS = 500`（`:4210`），只跳过**起始连续密集段**，首次出现 >500ms 间隔即**永久关闭**（`:2588-2590`） | 阈值依据（`:4203-4209` 注释）：重放 flush 行距 sub-ms~几 ms、真实轮次 ≥~4.6s、重放→真实断点 ≥11s；500ms 偏向"宁可少算也不多算"。首行无法前视，故仍有 **<1% 的残余 over-count**（`:2567-2569`）——被显式承认的残留，不是待修 bug |
| **interleaved usage**（交错谱系） | 一份 rollout 含多个 `SessionState` 的累计总数（父 agent + 内嵌 reviewer），事件不带流 id | LRU 流头（上限 32）+ 不变量 `total - last = 该流此前的 total`（`src/lib/codex-token-usage.js:3-8`、`:10-18`），四分支判定见 6.2.3 | 单测四组断言（`test/codex-token-usage.test.js:24-38`、`:40-48`、`:50-57`、`:59-67`）；另有**一次性修复**用同一状态机做受影响判定，见下 |

interleaved 那条的修复路径值得单列：`src/commands/sync.js:198` 从视图模块借用 `extractTokenCount`，用于 codex 一次性修复的**受影响判定**（`:6233-6252`）：

```js
// src/commands/sync.js:6246
const token = extractTokenCount(obj);
const info = token?.info;
if (!info || typeof info !== "object") continue;
consumeUsageDelta(state, info.last_token_usage, info.total_token_usage);
if (state.sawInterleaved || state.sawDivergentCumulative) {
  affected = true;
}
```

该修复会**重写历史桶**——原子重建到临时 queue，成功才替换、失败完全不动（`src/commands/sync.js:5750-5804`）；进入重建前还有一道闸门：若 project 队列检出畸形行则放弃本次修复以免丢数据（`countMalformedCodexProjectQueueRows` 定义在 `:6436`，**全库唯一调用点** `:5824-5830`）。所以它是"影响统计的"——但它是**迁移**，不是日常解析路径。

union 安装的意义也要看清：同一会话文件在两条路径各有游标时，"滞后根"的增量会被判成新数据。e2e 头注释把这处历史缺陷写得很直白（`test/codex-union-cursor-divergence.test.js:13`）：`needsHistoricalCodexDedup` 曾在 `startOffset > 0` 上短路，**先于**它自己的"同会话在另一路径下"判定，于是用了只知本次运行的 append-only 追踪器；当已同步的根无新内容时它一个键都不贡献，滞后根的重放就落进持久化桶与队列。修法是调整判定顺序，并把持久化事件集由 `codexEventStore` 注入（`src/commands/sync.js:1049-1051`）。

---

### 6.3 两家的对照表

| 维度 | Claude Code | Codex |
|---|---|---|
| **读取路径** | `~/.claude/projects/**/*.jsonl`，递归任意深度；UNION native + WSL + `CLAUDE_CONFIG_DIR` + `scanRoots.claude`（`src/commands/sync.js:685`、`:700-703`；`src/lib/rollout.js:5013-5023`） | `~/.codex/{sessions,archived_sessions}/YYYY/MM/DD/rollout-*.jsonl`，union native + WSL，归档目录扁平需 `deep` 扫描（`src/commands/sync.js:645`、`:782`、`:789-810`、`:885`） |
| **去重键** | `message.id`；有 `requestId` 时为 `message.id:requestId`；无 `message.id` 则完全不去重（`src/lib/rollout.js:4895` 区块） | 三层：累计未变即丢弃（`src/lib/codex-token-usage.js:109-119`）；`sessionUUID:eventTimestamp`（`src/lib/rollout.js:2626`）；compaction 用 `response_id`（`src/lib/codex-usage-record.js:60-80`） |
| **缓存字段** | 读 `cache_read_input_tokens`、写 `cache_creation_input_tokens`，**只认这两个名**（`src/lib/rollout.js:4927-4928`） | `cached_input_tokens` + `cache_creation_input_tokens`，兼容 `cache_write_input_tokens` 别名（`src/lib/codex-token-usage.js:25-27`） |
| **reasoning 处理** | **独立加成项**：解析显式拆出（`output -= reasoning`），计价照 output 单价，`total` 用未减 output（`src/lib/rollout.js:4930-4939`） | **子集标记**：解析不拆列，计价置 0（`src/lib/pricing/index.js:248-256`）；两侧都以五列之和进 `computeRowCost`，`total_tokens` 不参与计费 |
| **`conversation_count` 语义** | assistant 行恒 0；主会话 user 文本行 +1 且记入 `unknown` 模型桶；fork 折叠后再做净值修正（`src/lib/rollout.js:2865`、`:2810-2831`、`:987-1015`） | 每个被接受的 delta 恒写 1（`src/lib/rollout.js:2554`）——实际含义是"被接受的 token 事件数"，与 Claude 的"用户轮次数"不可互换 |
| **已知缺陷** | A：增量游标无行边界提交（`endOffset = st.size`，`:2793` 是唯一落 token 游标的站点；确切范围是本代文件内不可恢复）；B：一次性真值修复与增量解析口径分叉（usage 取值 / reasoning 字段名 / 是否拆分，费用不变但展示列改变且不可回退） | ① reasoning 不双计依赖 `source` 白名单硬编码，漏加即静默双计且测试全绿；② 模型串无 provider 维度，跨渠道同模型合并；③ `model/rerouted` 分支未在真实语料验证；④ 语义修复的触发依赖运行时迁移状态，无法从源码判定 |

最后一条共同底座：队列消费端按 `source|model|hour_start` **取最后一条（不是求和）**（`test/codex-union-cursor-divergence.test.js:104-112`；对照 `src/lib/local-api.js:207` 的 `readQueueData`），因此任何"同键数值回退"或"重复追加"都会直接改变最终读数——这正是两家在去重、折叠、血缘三条线上都写得异常小心的原因。
