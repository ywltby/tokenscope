# dsh（DeepSeek Harness）会话存储与 token 用量核验（候选适配器前置研究）

> 只读核验：全部结论来自**本机已安装实现**（npm 全局 `@deepseek-ai/dsh@0.2.0-rc.2`，实现在
> `~/.dsh/profiles/node_modules/@deepseek-ai/` 可读）、**本机真实数据** `~/.dsh`，以及上游仓库
> [`deepseek-ai/deepseek-harness`](https://github.com/deepseek-ai/deepseek-harness)
> （默认分支为 **`master`**，非 `main`）的架构 Agent Note。未写入、未移动、未删除任何被扫描数据。
>
> 文档只保留结构化字段、算法与聚合计数，不含真实会话路径、提示词、工具输出或用户内容；
> 示例路径一律符号化。探针脚本只在系统临时目录运行，不入库。

## 1. 结论摘要

1. 会话转录存放在 `<DSH_HOME>/sessions/<项目slug>/<会话id>/session[.vN].jsonl.zstd`，
   是**仅追加、zstd 压缩的 JSONL**：首行为会话头，其后每行一个事件。
2. 根目录解析优先级：**显式配置 > `$DSH_HOME` > `~/.dsh`**（空白环境变量视为未设置）。
   本机 `DSH_HOME=C:\Users\admin\.dsh`；`sessions` 是其子目录。
3. **token 用量可完整获取，且精确到逐请求**：每次模型调用落一条 `assistant/message`，
   其 `data.usage` 携带分桶 token 增量。会话级汇总可由事件累加无损还原（已交叉验证）。
4. **四类 token 桶（未缓存输入 / 输出 / 缓存读 / 缓存写）是并列相加关系**，
   `inputTokens` 的语义是**未缓存输入**，**不包含** `cacheReadTokens`。
   证据：`inputTokens + cacheReadTokens + outputTokens = totalTokens` 严格成立（§6.3）。
5. **Rust 侧不存在解码障碍**：`zstd` crate 的 `stream::read::Decoder` 会**自动续读串联帧**，
   解压结果与 `zstd -d` 逐字节一致（§9.1 已实测）。上游警告的"一次性解压只读首帧"在本机
   **Node 侧复现**（Node v24.14.0：`zstdDecompressSync` 对 4.37 MB 文件只返回 **212 字节 / 1 行**，
   即仅头行）；**Rust crate 各 API 均无此问题**，但仍须用 fixture 回归锁定（§9.1）。
6. 压缩产物是**多帧串联**，首帧只含头行；实测（采样时 88 个文件）帧数
   **min=2 / p50=228 / max=97421**。
   首帧无事件，故**任何只解首帧的实现都会静默丢失全部事件**。
7. `assistant/message.usage` 的**键集不固定，且无严格世代规律**：实测 v0 为三键
   （并出现两条 key 的极端样本）、v3 为四键但同文件可混入五键、v4 为五键；
   同一文件内也可混用（593 行四键 + 63 行五键并存）。适配器必须按**字段存在性**容错，
   **缺失不得按 0 静默吞掉**，也不得按世代硬编码键集。
8. **同一会话目录可并存多个世代文件**（`session.jsonl.zstd` 与 `session.v3.jsonl.zstd` 等；
   实测 3 个目录存在该情况）。它们承载**逐位相同的用量事实**，
   因此**按文件累加会精确双计**（§9.2）。必须以「会话 id + 最高世代」去重。
9. **子代理会话独立落盘、独立计量**，父会话汇总**不含**子代理用量（已做闭合验证）；
   全量纳入**不会双计**。本机实测（采样时）89 个会话目录中 **45 个是子代理**（含 1 个二层嵌套），
   占比过半，**不可忽略**。子代理**只能**用头行字段判定，不能用 id 形态（§5）。
10. **`tokenUsage.totals` 只累计 `assistant/message`**；`compaction/summary` 携带的真实用量
    （样本 `inputTokens: 521641`）**不计入**（§6.5）。这是口径缺口，也意味着
    **扫日志比读投影缓存更完整**。
11. `session_projcache` 是**缓存而非权威**：日志权威、缓存跟随、绝不领先，记录带 `seq` 水位，
    且无淘汰策略。可作为快速路径，不能当事实源。
12. **项目身份必须取头行 `cwd`，不能反解目录 slug**：slug 编码**有损**（上游明确承认
    `/a/b-c` 与 `/a-b/c` 会同名）。`workspace.json` 也不是完整清单（§7）。
13. 全量扫描本机语料（采样时 88 文件 / 138.96 MB）实测 **8883 ms**、**0 坏行**，
    横向可比 TokenScope 既有基准（Claude 1.2 GB 冷启动约 5.5 s）——**可行，但建议并行化 + 惰性 JSON 解析**。
14. **重试与 attempt 的用量是三态状态机**（§6.6）：同 `(turn, step)` 的后续样本**替换**前者；
    `llm/retry-started` 清空替换槽后转为**累加**；`assistant/attempt` 的用量取自其 `data.stream`。
    只累加 `assistant/message.data.usage` 的朴素做法会在真实重试场景偏离官方数字。

## 2. 存储布局

| 路径 | 内容 | 权威性 |
| --- | --- | --- |
| `sessions/<项目slug>/<会话id>/session[.vN].jsonl.zstd` | **会话转录**（头行 + 事件流） | **权威事实源** |
| `sessions/.../<会话id>/` | 会话自有目录（后端可放自有产物） | 布局归属 |
| `storages/workspace.json` | workspace→会话索引：`path`、`title`、`sessionIds`、时间戳、`archivedSessionIds` | 派生索引，**不完整** |
| `storages/session_projcache/sessions/<会话id>.json` | 逐会话投影检查点（含 `tokenUsage` 等） | **缓存，非权威** |
| `storages/session_projcache.json` | 投影缓存域表 | 缓存 |
| `attachments/v1/objects/<2位分片>/` | 附件对象存储 | 非用量 |
| `settings.yaml*`、`.credentials.yaml` | 设置与凭据 | 非用量 |
| `profiles/` | profile 与依赖 `node_modules`（**实现源码可读**） | 非用量 |

本机规模实测（2026-10-11 **单次采样**，同一时刻自洽；语料随使用持续增长）：

- `sessions`：**89 个会话目录 / 92 个 `.zstd` 文件 / 143.75 MB**（最大单文件 49.67 MB）。
- 其中 **44 个主会话目录 + 45 个子代理目录**；**3 个目录**存在多世代文件（§9.2）。
- `.dsh` 全目录约 160.5 MB，其中 `profiles`（依赖）仅 11.56 MB。

> 文中另有少数规模数字（帧数分布、性能表）来自**更早的 88 文件批次**，均以"采样时"标注口径。
> 语料在持续增长，不同批次的分母**不必彼此吻合**。

项目目录（slug）命名规则：路径分隔符与驱动器分隔符转 `-`，不安全代码单元编码为 `~XXXX`
（4 位大写十六进制 UTF-16 码元），前后包 `--`，超长截断。例：
`<盘>:\...\项目\tokenscope` → `--C-Users-admin-Desktop-~9879~76EE-tokenscope--`（`~9879~76EE` = 项目）；
`~0020` 即空格。无 cwd 的会话使用 `_no-cwd`。

> 上游明确：该编码**有损**，不同 cwd 可能归一化到同名目录。**只能当目录分桶，不能当项目身份。**

## 3. 根路径解析与发现规则

上游 `dsh-home-paths` 的解析顺序：

1. 显式配置的路径（最高）
2. `$DSH_HOME`
3. `~/.dsh`（默认）

空或仅空白的 `$DSH_HOME` 视为未设置（避免把根解析到当前工作目录）；路径经波浪号展开并规范化
为绝对路径。面向用户的展示一律用符号形式（`~/.dsh` / `$DSH_HOME`），不泄露机器路径。

源码注意：`dshHomePath(...)` **不接受**显式根参数（内部调用无参 `resolveDshHome()`），
只有 `dshCachePath({ dshHome }, …)` 支持显式覆盖。

### 3.1 落盘根可被配置覆写（重要）

`sessions` 根**不是硬编码**，而是插件配置项：

```yaml
# dsh-base/cordis.patch.yml（bundle 默认层）
- id: session-persistence-jsonl
  name: '@deepseek-ai/dsh-session-persistence-jsonl'
  config:
    root: !!js dshHomePath('sessions')     # 默认；运行时求值
```

因此存在**三条偏移途径**：

| 途径 | 影响范围 | 方式 |
| --- | --- | --- |
| 环境变量 `DSH_HOME` | **全局**（sessions / storages / attachments / cache 一并偏移） | 进程环境 |
| 配置覆写 `session-persistence-jsonl.config.root` | **仅 sessions** | profile 的 `cordis.patch.yml`，按 `id` 定位覆写 |
| CLI `--patch <path>` | 依 overlay 内容 | 可重复的额外 patch 层，应用在 profile 层之后 |

`storage-json.config.root`（投影缓存等地）同理，默认 `!!js dshHomePath('storages')`。

**本机实测**：4 个 profile（`default` / `dsh-tui` 的 patch 层为空数组 `[]`；
`desktop` / `web` 有覆写但**均未触及**持久化根）→ 全部落 `$DSH_HOME/sessions`
= `C:\Users\admin\.dsh\sessions`。
`dsh --profile web --dump-config` 的输出中 `root: !!js dshHomePath('sessions')` **保持表达式原文**
（`!!js` 在运行时才求值，转储不展开），即**未被覆写**。

**发现规则**：

1. 解析根：显式配置项 > 非空白 `DSH_HOME` > 用户主目录下 `.dsh`。
2. 递归收集 `<root>/sessions/**/session*.jsonl.zst[d]`，**不要**假设层级固定，也不要假设
   "一会话一文件"（§9.2）。
3. 项目身份取头行 `cwd` 并规范化；**不要**用目录 slug，也**不要**依赖 `workspace.json`。
4. **允许用户手动指定 sessions 根**（部署可能经 §3.1 偏移）；若要自动判定，可解析
   `dsh --profile <name> --dump-config` 里该 `id` 的 `root`——**字面路径 = 被覆写**，
   仍是 `!!js dshHomePath('sessions')` = 跟随 `$DSH_HOME`。
5. 全程只读打开。

## 4. 文件格式与世代

- 物理编码：**zstd**，后缀 `.jsonl.zstd`；配置 `compression: 'none'` 时为原始 `.jsonl`。
- 逻辑格式：**JSONL**，一行一条记录（先头行，后事件行）。
- 世代命名：v0 使用**无版本后缀** `session.jsonl[.zstd]`，正世代使用**小写**
  `session.vN.jsonl[.zstd]`。本机实测存在 **v0（无后缀）、v3、v4**。
- 上游约定：一份持久化根目录**只归属一种编码**（zstd vs 原始 JSONL），发现阶段拒绝相反后缀；
  不提供压缩转换、双重读取或基于扩展名的 fallback。
  **注意这说的是"编码"而非"世代"**：同一目录下 v0 与 v3 等**世代**文件允许并存（§9.2），二者不矛盾。
- **迁移保留源 generation**：逻辑版本迁移不删除旧文件，而是**并排发布**新世代文件——
  这就是 §9.2 多文件共存的成因。

本机世代分布（按会话目录、取最高世代，与 §2 同一次采样）：**89 个目录中 v0 = 54、v3 = 7、v4 = 28**
（v0 仍占多数，即大部分会话尚未迁移到正世代）。

## 5. 会话头（SessionHeader，文件第一行）

主会话实测：

```json
{
  "type": "session",
  "version": 3,
  "id": "session-41a3771a-6370-45b0-8346-907153e55332",
  "createdAt": 1790340996900,
  "cwd": "C:\\Users\\admin\\Desktop\\tenp2",
  "isSeeded": false,
  "delegationDepth": 0,
  "agentPreset": "standard"
}
```

子代理会话实测（注意 `origin` / `parentSession` / `delegationDepth`）：

```json
{
  "type": "session",
  "version": 4,
  "id": "757ca482-c4e9-4b34-8eb6-bb10f67058b5",
  "createdAt": 1791660041843,
  "cwd": "C:\\Users\\admin\\Desktop\\项目\\tokenscope",
  "parentSession": "session-98b66522-88f1-4644-b16c-1e66ca33bab1",
  "isSeeded": false,
  "origin": "subagent",
  "delegationDepth": 1,
  "agentPreset": "standard"
}
```

要点：

- `id`：本机观测到主会话为 `session-<uuid>`、子代理为**裸 uuid**（无前缀），且**目录名与 `id` 一致**。
  但 id 形态**不是**身份判据，见下方警告。
- `createdAt`：Unix epoch **毫秒**，非负安全整数，**拒绝小数值**。
- `cwd`：会话工作目录，**项目身份的正源**。
- `origin` / `parentSession` / `delegationDepth`：子代理谱系；`delegationDepth: 0` 为主会话。
- 元数据在**日志之外**（头行），不进入可回放事件流，也**不到达**消息投影。
- 头行位于**首个 zstd 帧**内，且该帧**只含头行** → **仅解首帧即可低成本枚举元数据**（列表场景）。

**子代理识别规则（必须用头行字段）**：`origin == "subagent"` / 存在 `parentSession` /
`delegationDepth > 0`，三者实测**完全等价**（本机 45/45，0 反例）。
等价地可用投影缓存中 `subagent.identity.label` 非空。

> **不要用 id 形态或目录名判定子代理**。虽然本机 45 个子代理会话恰好都是裸 uuid，
> 但源码中 `dsh-acp` 创建的会话同样是裸 uuid（`brandString(randomUUID())`），
> `dsh-agent-loop` 还存在 `<id>-session-<uuid>` 形态。裸 uuid **不蕴含**子代理身份。

## 6. 事件流与 token 用量字段

事件信封统一为 `{ type, seq, time, data }`；**头行不含 `seq`**，事件行从 **0** 连续递增
（上游不变式 `events[i].seq === i`，其中 `i` 是**事件索引**、不含头行）。
本机实测两个文件（3888 行 / 178 行）均满足 `seqs[i] == i`、**0 重复、0 坏行**。
单会话类型分布如下：

| 事件类型 | 条数 | 说明 |
| --- | --- | --- |
| `tool/call` / `tool/result` | 773 / 773 | 工具调用与结果 |
| `step/start` / `step/end` / `assistant/message` | 656 each | 步边界与**模型调用** |
| `turn/start` / `turn/end` | 63 each | 轮边界 |
| `user/message` | 68 | 用户输入 |
| `agent/inbox/spliced` | 133 | 代理收件箱拼接 |
| `compaction/start` / `summary` / `end` | 1 each | 压缩（**`summary` 携带真实用量**） |
| `request/header` / `request/context` | 3 / 1 | 请求信封快照；`header` 为 `{config, adapterDefaults, tools}`，**不含 usage 也不含 model**；`context` 为 `{provider, model, contextWindow}` |

### 6.1 `assistant/message` 结构（token 用量的常规来源）

```json
{
  "type": "assistant/message",
  "seq": 13,
  "time": 1790354630496,
  "surfaceOp": "…",
  "data": {
    "turn": 1,
    "step": 1,
    "message": {
      "role": "assistant",
      "content": [ { "type": "text", "text": "…" } ],
      "source": { "kind": "model", "provider": "commandcode", "model": "deepseek/deepseek-v4.1-flash" },
      "id": "…"
    },
    "usage": { "inputTokens": 16297, "outputTokens": 156, "cacheReadTokens": 10496, "cacheWriteTokens": 0 },
    "stream": [ { "type": "…", "time": 1790354630500, "chunk": { "type": "…", "index": 0, "blockType": "…" } } ]
  }
}
```

| 字段路径 | 类型 | 用途 |
| --- | --- | --- |
| `time` | int（epoch ms） | **事件时间**，聚合维度正源 |
| `seq` | int | 日志内序号，稳定排序与水位 |
| `data.turn` / `data.step` | int | 轮 / 步坐标 |
| `data.usage.inputTokens` | int | **未缓存输入** token |
| `data.usage.outputTokens` | int | 输出 token |
| `data.usage.cacheReadTokens` | int | 缓存读 token |
| `data.usage.cacheWriteTokens` | int | 缓存写 token（本机恒 0） |
| `data.message.source.provider` | str | 提供方（本机 `commandcode`） |
| `data.message.source.model` | str | **模型标识** |
| `data.message.id` | str | 消息 id |
| `data.stream[]` | array | 带时间戳的提供方流分片（可还原 TTFT / 解码时序） |

上游不变式：**每个进入的步恰好一条 `step/end`**；`assistant/message` 对**无内容调用**与
**达到 token 上限的调用**同样落盘（后者内容为空、被排除在表层之外，但**计入用量**）。

**易错点**：同一会话可**跨多个模型**（本机最大样本单会话出现 8 种模型标识），
且同一模型存在多种写法（如 `deepseek/deepseek-v4-flash`、`deepseek-v4-flash`、
`DeepSeek-V4-Flash`）。模型身份必须走 TokenScope 既有的**等价键归一**（小写 + 删除 `-` `.` `_`），
不能按原始字符串分组。

### 6.2 usage 键集不固定：世代间无严格规律，且同一文件内可混用

**观测 1（跨世代，本机逐世代抽样实测）**：`dsh-session-format-catalog` 定义
`currentVersion: 4` 与 v0→v4 迁移链，但实测的 `assistant/message.data.usage` 键集
**没有严格世代规律**（括号内为受检条数）：

| 世代 | 实测 usage 键集 | 备注 |
| --- | --- | --- |
| v0（`session.jsonl.zstd`） | **三键** `inputTokens, outputTokens, cacheReadTokens`（60 条中 59 条）；**另有 1 条两键** `inputTokens, outputTokens` | **无** `cacheWriteTokens`；有原始 delta 事件（`assistant/chunk` 等），消息**无** `stream` |
| v3 | **四键**（含 `cacheWriteTokens`）：176 条、57 条两例一致；但另一 v3 文件为 593 四键 + 63 五键**混用** | 消息带 `stream`；首次出现 `assistant/attempt` |
| v4（当前） | **五键** `inputTokens, outputTokens, cacheReadTokens, reasoningTokens, totalTokens`（**无** `cacheWriteTokens`）：39 / 21 / 73 条三例一致 | — |

**关键**：v0 内部就出现「两键 → 三键」变化（同一会话，`seq=105` 的行只有
`{inputTokens, outputTokens}`），说明连"同世代同键集"都不成立。
因此**唯一可靠做法是按字段存在性读取**，缺失桶保留为**未知**——
若把缺失当 0，那次调用的缓存读会被错记为 0（实际是**未上报**）。

`compaction/summary` 的键集同样不固定（受检 v3 文件内为四键）。

**观测 2（同一文件内混用，本机实测）**：一个 `header.version = 3` 的文件内，
`assistant/message` 出现 **593 行四键形态**（seq 13–3535）与 **63 行五键形态**
（seq 3540–3884）并存——即**单个文件不完全等同于单一键集**，疑与该会话跨越 dsh 应用版本
升级期有关（未最终归因）。

**适配器要求（无论采用哪种解释都必须满足）**：

- **按字段存在性读取，不得按世代硬编码键集**，也不得假设"一个文件一种键集"。
- 缺失桶保留**未知**而非 0（与 TokenScope 既有"单价三态 / 不静默按 0 吞掉"原则一致）。
- `reasoningTokens` / `totalTokens` 在 v4 出现：`totalTokens` 是**派生校验值**（见 §6.3），
  `reasoningTokens` 目前**不参与**四桶求和（上游明确"不会再次加入推理计数"）。

> 另外注意 `assistant/attempt` 也参与用量核算（其用量取自 `stream`），且重试语义为累加；
> 完整的三态状态机与实测见 **§6.6**。

### 6.3 桶语义与等式证据（最关键的口径）

五键形态样本（键集见 §6.2）：`inputTokens: 435`、`cacheReadTokens: 716928`、`outputTokens: 355`、
`totalTokens: 717718`。

```
435 + 716928 + 355 = 717718 = totalTokens   ✓ 严格成立
```

**结论**：`inputTokens` 是**未缓存输入**，与 `cacheReadTokens` / `cacheWriteTokens` /
`outputTokens` **并列相加**；它**不是**"含缓存的总输入"。

> 本文档统一用"四桶 / 四类语义桶"指这四类 token；其**字面键集随世代变化**（§6.2）——
> 例如 v4 缺 `cacheWriteTokens`，多出 `reasoningTokens` / `totalTokens`。

> 注意与本机 Claude Code / Qoder 路径的既有认知**相反**（那些是 OpenAI 语义：
> `input_tokens` **包含** cache read）。**不得**套用"input 含 cacheRead"的桶归一，否则漏计。

事件侧 ↔ 投影侧命名对照（同义，已验证）：

| 事件侧（`assistant/message.data.usage`） | 投影侧（`tokenUsage.totals`） | 语义 |
| --- | --- | --- |
| `inputTokens` | `uncachedInputTokens` | 未缓存输入 |
| `outputTokens` | `outputTokens` | 输出 |
| `cacheReadTokens` | `cacheReadTokens` | 缓存读 |
| `cacheWriteTokens` | `cacheWriteTokens` | 缓存写 |

### 6.4 汇总闭合验证（已做，delta 严格为 0）

| 会话 | `assistant/message` 行数 | Σ 事件累加 vs 投影 totals |
| --- | --- | --- |
| 主会话（63 轮 / 656 步） | 656 | **完全相等**（input 3132751 / output 950512 / cacheRead 338143104 / cacheWrite 0） |
| 主会话 | 73 | **完全相等**（input 281762 / output 69986 / cacheRead 12454144 / cacheWrite 0） |
| 子代理 | 21 | **完全相等**（input 94234 / output 27620 / cacheRead 1390976 / cacheWrite 0） |

另有（采样时）85 条投影记录 `decodeTokens == totals.outputTokens` **85/85 全匹配**。

**独立复现**：用 Rust + `zstd` crate 流式解析同一会话，得到
`input=3132751 / output=950512 / cacheRead=338143104 / cacheWrite=0`、656 条
`assistant/message`、**0 坏行**——**第三方实现可逐位复现 dsh 自身汇总**。

**推论**：TokenScope **无需依赖投影缓存**即可从日志还原权威值；缓存只应作性能捷径。

### 6.5 口径缺口：压缩摘要的用量不计入汇总

同一会话内实测 `compaction/summary`（`seq=1976`）携带
`usage = {inputTokens: 521641, outputTokens: 3508, cacheReadTokens: 0, cacheWriteTokens: 0}`，
但该会话 `totals.outputTokens` **严格等于** Σ`assistant/message`.outputTokens（950512），
**未包含** 3508。

即：

- **`tokenUsage` 投影 = Σ`assistant/message`.usage**；`compaction/summary` 这类
  **额外 LLM 调用不在其中**。
- 只读投影缓存 → 漏计压缩摘要消耗（本样本漏 521641 输入 token：相对该会话主对话未缓存输入
  3132751 为 **16.7%**，相对两者之和为 14.3%）。
- **扫日志可以追回 `compaction/summary`**（它带真实 usage），口径因此比 dsh 自身汇总更完整。
- **但并非所有辅助调用都能追回**：`session/title-llm-request` 与 web 搜索调用同样是真实消耗，
  **本地日志不记录其 usage**（§12），扫日志也拿不到数字，只能标记"未上报/未知"。

**这是实现前必须定下的产品决策**：是否把 `compaction/summary` 计入（标题与搜索类调用本地
根本没有数字，只能标"未上报"，无从选择）。
建议**纳入并单独标记事件类别**，让用户可分辨"主对话用量"与"辅助开销"。

### 6.6 重试与 `assistant/attempt` 的用量状态机（源码级，已实测）

`dsh-token-meter` 的 `tokenUsage` 投影（`stateVersion: 2`）是该口径的**权威定义**：

```js
// usageOf(event)：样本来源
if (event.type === "assistant/message" && event.data.usage !== undefined) return event.data.usage;
if (event.type !== "assistant/message" && event.type !== "assistant/attempt") return undefined;
return lastAssistantStreamChunk(event.data.stream, "usage")?.usage;   // 回落到 stream

// apply(state, event)：三态
if (event.type === "llm/retry-started")
  return state.last?.turn === event.data.turn && state.last.step === event.data.step
    ? { ...state, last: null } : state;                              // ① 清空替换槽
const sample = usageOf(event); if (sample === undefined) return state;
const previous = (state.last !== null && state.last.turn === turn && state.last.step === step)
  ? state.last.buckets : undefined;
if (previous !== undefined && bucketsEqual(previous, buckets)) return state;
return { totals: addReplacing(state.totals, previous, buckets), last: { turn, step, buckets } };
// addReplacing(totals, prev, next) = totals - (prev ?? 0) + next
```

三条语义：

1. **同 `(turn, step)` 的后续样本「替换」前一样本**（`totals - previous + next`）——
   用于同一次请求的多次样本（先 attempt 增量、后 message 结算）。
2. **`llm/retry-started`（同 turn/step）清空替换槽** → 下一次样本取不到 `previous`
   → 走**纯累加**：重试的用量**全额计入**。
3. `assistant/message` 缺 `data.usage` 时**回落到它自己的 `stream`**；
   `assistant/attempt` 则**只能**从 `stream` 取（`lastAssistantStreamChunk(stream, "usage")`）。

**本机实测（唯二两次重试）**：

| turn/step | 事件序列 | attempt 的 stream 含 usage chunk？ |
| --- | --- | --- |
| 16/2 | `step/start` → `assistant/attempt` → `llm/retry` → `llm/retry-started` → `assistant/message`(带 usage) → `step/end` | **否**（stream 仅含 chunk / reasoning-chunks / tool-call-chunks） |
| 21/8 | 同上 | **否**（stream 仅 1 个 chunk） |

即失败尝试**未上报提供方 usage**——**观测到的事实**是其 `stream` 内不含 usage chunk；
"请求在收到 usage 之前就已失败"是对该事实的**推测**（未逐行确认）。
因此 `usageOf(attempt)` 为 `undefined`、投影不改动状态——这就是 §6.4 闭合验证 delta 全 0 的原因。

**对适配器的要求（重要）**：朴素地只累加 `assistant/message.data.usage` 会在两种真实场景偏离官方数字：

- attempt **带** usage 且**无** `retry-started`：官方走**替换** → 朴素做法**多计**；
- attempt **带** usage 且**有** `retry-started`：官方走**累加** → 朴素做法**漏计**。

要**精确复刻 dsh 官方数字**，必须实现上述三态状态机；若要的是"真实总账"，
则应全量累加并**显式说明与官方数字的差额来源**（与 §6.5 同类的口径选择）。

## 7. 子代理（subagent）计量独立性

- 子代理会话是**独立文件、独立会话 id、独立目录**，头行标 `origin: "subagent"` +
  `parentSession` + `delegationDepth ≥ 1`。
- **父会话 `tokenUsage.totals` 不含子代理用量**（§6.4：父/子各自闭合，父不含子）。
- 因此**全量纳入不会双计**；本机（采样时）45 个子代理会话，占 89 个会话目录的过半，**不可忽略**。
- 建议：全部纳入，用 `parentSession` 建归属关系供 UI 区分"主会话 / 子代理"。
- **`workspace.json` 不是完整清单**：实测（与 §2 同一次采样，89 个会话目录）中 39 个在
  `tables.workspaces[].sessionIds`、18 个在 `archivedSessionIds`（两者交集 15、并集 42），
  **其余 47 个目录完全不在索引中**（绝大多数是子代理会话）。发现流程**必须基于文件系统扫描**。
- **归档 ≠ 删除**：`archivedSessionIds` 的 18 个会话文件**全部仍在** `sessions/` 下，
  用量继续可读。

### 7.1 嵌套深度与归属（源码 + 实测）

源码 `dsh-subagent`：

```js
function delegationDepthOf(agent) {           // 持久化 header 是单调下限
  const runtime = agent.options.subagentDepth;
  return Math.max(agent.session.header.delegationDepth ?? 0, runtime ?? 0);
}
function resolveChildDepth(parent, maxDepth) {
  const childDepth = delegationDepthOf(parent) + 1;      // 每层 +1，可递归
  if (maxDepth !== undefined && childDepth > maxDepth) throw new SubagentDepthError(childDepth, maxDepth);
  return childDepth;
}
function childSessionMeta(parent, childDepth, isSeeded) {
  return { ...cwd 继承父, parentSession: parentHeader.id,
           isSeeded, origin: "subagent", delegationDepth: childDepth };
}
```

- **深度可递归增长**：`childDepth = 父深度 + 1`，上限由 `dsh-subagent` 服务配置的
  `maxDepth` 控制，**默认 10**（`maxActiveSubagents` 默认 8）。
- **子会话继承父的 `cwd`**（项目身份因此与父一致，不因嵌套而改变）。
- **归属靠 `parentSession` 递归**，不是靠目录名或 id 形态。

本机全量 header 实测（与 §2 同一次采样，89 个会话目录，按最高世代取一个）：

| `delegationDepth` | 目录数 | 说明 |
| --- | --- | --- |
| 0 | **44** | 主会话（`origin` 缺失、无 `parentSession`、id 带 `session-` 前缀） |
| 1 | **44** | 一层子代理 |
| 2 | **1** | **二层嵌套（本机确实存在）** |

一致性校验：`origin == "subagent"` ⟺ 存在 `parentSession`，**45/45 全等，0 反例**。
`delegationDepth == 0` 与「id 带 `session-` 前缀」在本机亦完全重合，但那是**观测到的相关性**，
**不可反过来当判据使用**（§5 的 ACP 反例已证明：裸 uuid ⇏ 子代理）。

> 会话 id 的生成点（源码）：主会话由 `dsh-api-session-controller`
> （`session-${randomUUID()}`）或 `dsh-headless` 生成；子代理由 `dsh-subagent`
> （`spec.childId ?? brandString(randomUUID())`）或 `dsh-subagent-in-process-driver`
> 生成——**裸 uuid**。另注意 `dsh-acp` 同样生成裸 uuid（见 §5 的警告）。

## 8. 投影缓存（`session_projcache`）作为快速路径

路径：`<root>/storages/session_projcache/sessions/<会话id>.json`。记录形态：

```json
{
  "version": 7,
  "record": {
    "identity": { "createdAt": 1787152054737, "cwd": "…" },
    "rows": {
      "sessionStats":    { "ver": 1, "seq": 10601, "val": { "turns": 3, "steps": 60, "llmMs": 1825730,
                            "toolMs": 41080, "ttftMs": 609133, "ttftSteps": 60,
                            "decodeMs": 1216597, "decodeTokens": 99720, "lastTurn": 3 } },
      "tokenUsage":      { "ver": 1, "seq": 10601, "val": {
                             "totals": { "uncachedInputTokens": 565013, "outputTokens": 99720,
                                         "cacheReadTokens": 10294144, "cacheWriteTokens": 0 },
                             "last": { "turn": 3, "step": 2, "buckets": { "…": 0 } } } },
      "title":           { "…": "…" },
      "contextPressure": { "…": { "surfaceTokens": 197079, "contextWindow": 500000 } },
      "subagent":        { "…": { "identity": { "mode": "continuable", "label": "…" } } }
    }
  }
}
```

> 上例中的记录为**旧版遗留**（`tokenUsage.ver = 1`，而当前安装包该单元 `stateVersion = 2`；
> 其 `identity` 亦**缺 `formatVersion`**），仅用于展示结构；实际采信必须通过下方校验。
> 当前记录的 `identity` 键为 `formatVersion` / `createdAt` / `cwd` / `isSeeded` /
> `inheritedEventCount`（实测最新记录 `formatVersion = 4`，与日志头行一致）。

语义要点（上游包文档 + 代码核验 + 实测）：

- **日志权威，缓存跟随**：活会话检查点先把事件持久化，再写缓存记录；崩溃可让缓存**落后**
  于日志，但**绝不领先**。源码原话更明确：缓存行「**possibly stale (its `seq` says how stale)
  but never wrong**」——即**陈旧但绝不错误**，`seq` 只表达陈旧程度。
- `asOfSeq` = **所有值共同反映到的最后一个事件的 `seq`**；空日志为 **`-1`**。
  持久缓存记录的 `asOfSeq` 属**该存储记录自身的水位**，**不可**与连接期活动会话的值直接比较
  （上游以 `kind: 'cached' | 'sequenced'` 区分）。
- 三条**必写点**：会话创建、`turn/end`、会话释放；其间由 `writeEveryEvents`（实测 base 配置
  **200**）与 `writeIntervalMs`（**5000**）节流 → **缓存最多滞后 200 事件或 5 秒**。
- 投影键由各单元注册：`tokenUsage`(sv 2) / `contextPressure`(sv 5) / `contextBreakdown`(sv 5)
  ← `dsh-token-meter`；`sessionStats`(sv 1) ← `dsh-session-stats`。域 `session_projcache`
  **version 7 / `compatibleVersions [3,4,5,6]` / `invalidRecords: backup-and-skip`**；
  行内 `ver` 不匹配即**丢弃重折（不迁移）**。
  以上四项**均已对照 `dsh-session-projection-cache` 源码逐字核对**（另有 `layout: "per-record"`，
  即每会话一份文档）。
  **实测**（本机最新写入的记录）与上述 `stateVersion` 逐一吻合：`tokenUsage`=2、
  `contextPressure`=5、`contextBreakdown`=5、`sessionStats`=1。
- **无淘汰/保留接口**——记录随会话持续累积。
- **`commandCodeCost` 不是官方投影**：它来自本机第三方 provider 插件
  （`@mars-sea/dsh-commandcode-provider`），按 `contextTokens` 分段 + `pricingKey` 内容指纹
  组织 `facts.groups[].tokens`。可作对照参考，**不可当官方语义**。
- 另有**旧布局遗留** `storages/session_projcache.json`（unit version 3）：仍含
  `tokenUsage` / `sessionStats` 行，但本机实测**最后写入于 2026-09-02，已停止更新**，
  只能作历史兜底。
- **行的集合随部署/插件演进**：本机当前记录含 **24 个 row 键**（`tokenUsage`、`sessionStats`、
  `contextPressure`、`contextBreakdown`、`title`、`goal`、`todos`、`plan`、`subagent`、
  `subagentTiming`、`subagentCatalog`、`sandboxMode`、`modelSelection` 等），
  而早期采样中曾出现 6 种不同的键组合。解析时必须容忍缺失键
  （**与 §6.2 的 usage 键集是两回事**）。
- **`commandCodeCost` 的 `ver` 是大数**（实测 `2696707491`，即 `pricingKey`），
  与官方单元的 `stateVersion`（小整数）风格不同——再次印证它来自第三方 provider 插件。

**读取校验（若采用缓存，以下缺一不可）**：

1. `identity.formatVersion` 存在，且**等于**日志头行的 `version`
   （缺该字段的旧记录不可用作冷折叠种子）。
2. 每个键的 `ver` **等于**当前安装包该单元的 `stateVersion`（否则官方都会丢弃重折）。
3. `seq` 作为水位读取；滞后 ≤ 200 事件或 5 秒可接受。
4. 不一致时**回落到日志解析**，绝不静默采信。

**取舍**：读缓存成本低（单文件 JSON），但有三类问题——口径不完整（§6.5）、可能落后于日志、
只有会话级汇总（无逐请求明细）。建议：**以日志为权威源**；若引入缓存，仅作"冷启动加速"
的可选路径，并显式标注其口径与水位。

### 8.1 官方数据出口（结论：无历史汇总命令）

`dsh` CLI 顶层除 profile 启动（`dsh <name>`）与 `plugin`（转发 pnpm）外，只有三个**互斥**的
配置转储选项：`--dump-config`、`--dump-default-config`、`--dump-config-schema`
——**没有 token/用量统计子命令**。

可拿到 usage 的官方途径，均为**运行期**而非**历史汇总**：

| 出口 | 内容 | 局限 |
| --- | --- | --- |
| `dsh headless "…" --json` | NDJSON 事件流，`step/end` 状态事件带**本步累计 usage**（跨 attempt 合并；任一 attempt 缺 usage 时**整步省略** usage） | **仅覆盖本次运行** |
| Web `/export` 或 `GET /api/session.export?sessionId=<id>&includeDescendants=true` | 下载含**规范 JSONL** 与附件的 ZIP | 仍是事件流，usage 需自行解析 |
| Typert Remote `session/projections` | 一次读全部已注册投影值（WebSocket mux） | 唯一"官方汇总"式出口，但需跑 host 并作 typert/WS 客户端，第三方成本高 |

相关包的口径澄清（避免误用）：

- `dsh-token-meter`：**启发式估算**（每 token 4 字符 + 角色框架 4），上游 README 自认对
  CJK 与 JSON schema **严重低估**。仅在请求 envelope 完全一致时才复用提供方 usage
  （`baseline.kind = 'usage'`）。**无 `bin`、无 HTTP 出口**。→ **不可**当作提供方 usage。
- `dsh-session-telemetry`（+otel）：导出**逐条会话事件**（`event.data` 全量，因而含 usage），
  但默认 `FEEDBACK_ONLY`（需用户显式反馈才释放前缀）、本地无落盘副本 → **不适合**作本地统计源。
- `dsh-session-query-sqlite`：表为 `search_state` / `persisted_sessions` / FTS5 `persisted_docs`，
  **无任何 token/usage 列**；官方 base 默认 `path: ':memory:'` + `openAt: never`，
  **本机不存在任何落盘索引** → 不可作统计来源。
- `dsh-session-log-deepseek`：方向相反（把规范日志**上传**给官方 API），与 usage 上报无关；
  本机已被本地补丁置为 `enabled: false`。

**结论**：第三方独立只读工具应以 `<root>/sessions/` 的日志为**唯一权威数据面**；
投影缓存仅作可选加速，其余出口要么只覆盖运行期、要么需 host 进程内集成。

## 9. 实现陷阱

### 9.1 多帧 zstd（最高优先级，但 Rust 侧已排除）

上游 Agent Note《Zstandard JSONL 会话日志》明确：产物是**标准独立 Zstandard 帧的串联**，
**第一个带校验和的帧只包含头部行**，后续每个持久追加批次各占一帧；并且——
> 外部工具必须理解串联的 Zstandard 帧……**Node 通用的一次性解压只读取第一个独立帧**。

本机实测帧数分布（`zstd -l` 权威值，采样时 88 文件）：

| 指标 | 数值 |
| --- | --- |
| 文件数 | 88（采样时） |
| 帧数 min / p50 / max | **2 / 228 / 97421** |
| 帧数合计 | 188608 |
| 每帧字节 min / p50 | 139 / 1242 |

- **最小值恒为 2**（头帧 + 至少一个数据帧），**不存在单帧文件** → 只解首帧 = 只拿到会话头，
  **事件全部丢失且不报错**。
- 极端样本：17.5 MB 文件**仅 2 帧**（单帧内海量事件）；50.9 MB 文件**97421 帧**（批次极多）。
  **两个方向都要能扛**。

**Rust 实测结论（本次已验，消除该风险）**：

| API | 结果 |
| --- | --- |
| `zstd::stream::read::Decoder::new(f)` + `read_to_end` | **decoded_bytes = 17248360**（与 `zstd -d` 逐字节一致）、**lines = 3888** |
| `zstd::decode_all(&mut BufReader)` | 同上，**17248360 / 3888** |
| `zstd::bulk::decompress`（底层 `ZSTD_decompress`） | **同样续读全部帧**（同一文件解出 17248360 字节）；对 49.67 MB 文件则因超过给定容量报错，可见它**不是**只解首帧 |
| 分块读取 | 同上，**17248360** |

即 **Rust 的 `zstd` crate 各 API 都会自动续读串联帧**（该文件 2156 帧），无需手工遍历帧头。

**帧结构实测（自写帧边界解析器，两个文件互证）**：

| 文件 | 解析出帧数 | `zstd -l` 帧数 | 首帧压缩 / 明文 | 首帧行数 | 帧大小之和 vs 文件 |
| --- | --- | --- | --- | --- | --- |
| v3（4.37 MB） | **2156** | 2156 | 179 B / 212 B | **1** | 4477516 = 4477516（leftover 0） |
| v0（49.67 MB） | **97421** | 97421 | 190 B / 208 B | **1** | 52086085 = 52086085（leftover 0） |

- **首帧确实只含头行**：两个文件的首帧都恰为 1 行 `{"type":"session",…}` ——
  这是"仅解首帧即可低成本枚举元数据"（§11 第 1 点）的**实测依据**。
- **帧精确拼接、无空洞**：帧大小之和等于文件长度，`leftover = 0`。
- **逐帧可独立解压**：两文件分别逐帧解出 **3888 / 159847 行**，与全量行数一致、**0 失败**
  → **帧边界恰好落在记录边界上，不存在跨帧半行**。这正是"按帧增量解析"可行的前提（§10、§11）。
- **要"只解首帧"必须自己解析帧边界再截取切片**：没有任何 crate API 会自动只解第一帧
  （`bulk::decompress` 也续读全部）。帧结构（魔数 + 帧头 + 块头 + 可选校验和）见 RFC 8878。

仍须执行的事项：

1. **写回归测试锁定该行为**：用**双帧 fixture**（首帧仅头行）断言解析到全部事件；
   crate 升级若改变语义会立刻红灯。
2. **断言首行必须是 `{"type":"session"…}` 头行**，作为"只解一帧"的廉价防线。
3. **帧数一律以 `zstd -l` 为准**。曾担心"魔数计数"会高估（压缩负载内可能偶然出现
   `28 B5 2F FD` 序列），但实测 **3/3 样本与权威帧数完全一致**（2156/2156、156/156、2/2），
   **未观察到误报**。该顾虑属**理论风险**：实现仍应解析帧结构而非扫描 magic，
   因为帧可以是 skippable 帧，且负载不可假定不含该序列。
4. **不要 `read_to_end` 整份明文**：实测解压比约 **3.9×**（17.9 MB → 119.9 MB；
   52 MB → 150 MB），最大文件展开后超 150 MB。用 `BufReader` + 逐行流式消费。

### 9.2 同一会话的多世代文件（**双计风险，必须处理**）

上游「迁移保留源 generation」的落地形态：**同一会话目录可并存多个世代文件**。
实测 **3 个目录**存在该情况（采样时共 89 个会话目录）：

| 目录 | 并存文件 | 实测关系 |
| --- | --- | --- |
| `session-a34130b3-…` | `session.jsonl.zstd`(49.67 MB/97421 帧) + `session.v3.jsonl.zstd`(17.07 MB/2 帧) | 6047 条 `assistant/message`；usage **逐位相同**（input 59106956 / output 3393700 / cacheRead 2314697920） |
| `session-ff28cefb-…` | `session.v3.jsonl.zstd` + `session.v4.jsonl.zstd` | 176 条；usage **逐位相同**（input 593241 / output 116249 / cacheRead 34429568）；v4 仅多 1 行收尾事件 |
| `session-c36404d2-…` | `session.v3.jsonl.zstd` + `session.v4.jsonl.zstd` | 空会话，无用量 |

**结论**：多世代文件承载**相同用量事实**，逐位一致 →
**按文件累加会精确双计**（首行样本会双计 5910 万输入 token / 23 亿缓存读 token）。

**规则**：**以会话 id（= 目录名 / 头行 `id`）为主键，仅取最高世代文件**
（有版本号的最大者优先，无后缀视为 v0 最低）。低世代是迁移源残留，**只读不删**（只读原则）。

### 9.3 其他

- **不要反解项目 slug**（有损）。项目身份取头行 `cwd`。
- **不要依赖 `workspace.json`**（采样时缺 47/88，§7）。
- **不要假设只存在一种 usage 键集**（§6.2），也不要假设 `cacheWriteTokens` 一定存在。
- **不要假设模型名唯一或写法一致**（§6.1）。
- **时间按 `seq` 稳定排序更安全**（`time` 是事件时间，不保证文件内单调）。
- **不要依赖投影缓存作为唯一源**（§6.5、§8）。
- 原始模式（`.jsonl`）与压缩模式不能在同一根目录共存；若遇到 `.jsonl`，按同一逻辑格式解析。

## 10. 性能实测（release 构建，本机）

| 场景 | 压缩体积 | 明文体积 | 行数 | `assistant/message` | 耗时 |
| --- | --- | --- | --- | --- | --- |
| 中等会话（2156 帧） | 4.48 MB | 17.24 MB | 3888 | 656 | **333 ms** |
| 大会话 | 17.90 MB | 119.86 MB | 35688 | 6047 | **1752 ms** |
| 最大会话（97421 帧） | 52.09 MB | 150.05 MB | 159847 | 6047 | **2621 ms** |
| **全量语料（88 文件）** | **138.96 MB** | — | **364462** | **21458** | **8883 ms** |

- **0 坏行**：全部行 `serde_json` 解析成功，无畸形行。
- 后两行是**同一会话的两个世代文件**（§9.2 表中的 `session-a34130b3`，v3 与 v0）：
  `assistant/message` 条数相同（6047）、用量完全相同——顺带印证了世代的**用量等价性**。
- 表中数字为**单次采样**（当时语料 88 文件 / 138.96 MB）；会话持续增长会使后续采样略有出入。
- 全量 8.9 s **含 88 次进程启动开销**（约 0.5–0.9 s）；同进程内批量处理会更快。
- 当前探针**对每一行都做完整 JSON 解析**，包括占比 90%+ 的非 `assistant/message` 行——
  这是主要成本。**优化方向**：先做廉价的字节级判别（是否含 `"assistant/message"`），
  仅对命中行做完整解析，预计显著提速。
- 参照 TokenScope 既有基准（本机 Claude 1.2 GB 冷启动约 5.5 s）：dsh 语料规模小得多，
  **串行 + 上述优化即可满足冷启动预期**；并行仅作可选加速。
- **可增量**：日志仅追加，且帧**自包含、可独立解压、边界落在记录边界上**（§9.1 实测：
  帧大小之和 = 文件长度、逐帧解出全部行且 0 失败），因此可做**按帧前缀增量解析**
  （只解新增帧）——这是 dsh 格式相对其他 agent 的一个**结构性优势**。

## 11. 接入方案建议（TokenScope 视角）

1. **发现**：解析根（配置 > `DSH_HOME` > `~/.dsh`）→ 扫 `<root>/sessions/**/session*.jsonl.zst[d]`
   → 按**会话 id 分组**、**选最高世代**（§9.2）→ 仅解**首帧**读头行，得到
   `id` / `cwd` / `createdAt` / `version` / `parentSession` / `origin` / `delegationDepth`。
   这一步即可低成本完成列表与元数据枚举（首帧实测仅 **1 行**、约 180–190 字节压缩）。
   **注意**：crate 没有"只解首帧"的 API，需**自己解析帧边界后截取第一帧**（§9.1）。
2. **解析**：逐帧流式解压 → 逐行判别 → 仅对 `assistant/message` 完整解析，
   提取 `data.usage` 的**四类语义桶**（字面键集可变，§6.2）+
   `data.message.source.{provider,model}` + `time` + `data.turn/step`。
   **绝不读取** `data.message.content` 与 `data.stream`（只读工具，避免大量无谓内存与隐私暴露面）。
3. **归一化**：`inputTokens` 作为**未缓存输入**桶，与 `cacheReadTokens` / `cacheWriteTokens` /
   `outputTokens` **并列**（**不要**套用 Claude/Qoder 的"input 含 cache"归一）。
4. **身份**：
   - 会话身份 = 头行 `id`（本机观测：主会话含 `session-` 前缀、子代理为裸 uuid；
     **形态只作观察、不作判据**，见 §5）。
   - 项目身份 = 规范化后的头行 `cwd`（沿用既有项目身份规则，不复用 slug）。
   - 逐请求指纹建议 `(会话id, seq)` 或 `(会话id, data.message.id)`；dsh 的 `seq` 在日志内
     连续且唯一，是天然稳定序号。
   - 模型身份走**等价键归一**（§6.1）。
5. **增量与缓存**：以「文件路径 + 文件长度 + 内容指纹」判增量；利用**按帧前缀增量解析**
   （§10）只处理新增批次。注意**活跃会话仍在写入**：读到的最后一个帧可能是**未完成结算的
   追加**——上游语义是该帧内**没有任何记录被确认为持久**，修复时从该帧起始截断、
   丢弃其全部内容（不可当作部分有效数据）。因此**只解到最后一个完整帧**是安全边界。
6. **可选**：把 `session_projcache` 作为**冷启动加速**读会话级 totals，
   但必须通过 §8 的四项读取校验（`identity.formatVersion`、逐键 `ver`、`seq` 水位、滞后窗口），
   并标注口径（**不含 `compaction/summary`**）；任何不一致都要**回落到日志解析**。

## 12. 待验证项与风险

| 项 | 状态 | 说明 |
| --- | --- | --- |
| Rust `zstd` crate 跨帧续读 | **已验证通过** | `Decoder` / `decode_all` / `bulk::decompress` 均完整解出 2156 帧；**仍需 fixture 回归测试锁定** |
| 「首帧只含头行」 | **已实测** | 两个文件（2156 / 97421 帧）的首帧均为 **1 行**头行；帧大小之和 = 文件长度、逐帧独立解压 0 失败（§9.1） |
| 多世代文件等价性 | **已验证** | 3/3 目录 usage 逐位一致；规则取最高世代 |
| 归档会话可达性 | **已验证（保留）** | 18 个归档文件全部仍在 `sessions/` 下 |
| usage 键集形态 | **已确认多变** | 实测 v0 三键（并有单条**两键**样本）、v3 四键、v4 五键，且**同文件内可混用**；`compaction/summary` 键集亦不固定。fixture 须覆盖"字段三角"（各桶可选，含极端两键） |
| `compaction/summary` 是否稳定带 usage | 样本少 | 本机 1 条样本带 usage；类型定义确认其为真实 LLM 调用（含 `llmStreamCall: true` 分支）。是否每次压缩都落 usage 待更多样本 |
| 标题生成 / web 搜索调用是否落 usage | **已确认：不落** | `session/title-llm-request` 带 `route{provider,model}` 但实测 11/11 无 `data.usage`；`web/deepseek-search-llm-request` 亦无。属真实消耗但**本地无数字**，只能标"未上报" |
| `assistant/attempt` 的 usage 贡献 | **已验证（源码 + 实测）** | 样本取自其 `data.stream` 的最后一个 usage chunk；本机两次重试的 attempt **均无** usage，故 delta 全 0。三态状态机见 §6.6 |
| 更深子代理嵌套 | **已验证** | `childDepth = 父深度 + 1`，`maxDepth` 默认 10；本机实测 depth 分布 **44 / 44 / 1**（含 1 个 `delegationDepth: 2`）。归属按 `parentSession` 递归（§7.1） |
| 自定义根落盘偏移 | **已验证** | sessions 根是**可覆写配置项**（profile patch / `--patch`），另有全局 `$DSH_HOME`；本机 4 个 profile 均未覆写（§3.1） |
| dsh 官方 CLI/遥测出口 | **已确认：无历史汇总命令** | 详见 §8.1 |
| 本地补丁对计量语义的影响 | **已验证未削弱** | 本机安装被 `dsh-purge` 改过（见 §15）；usage 传递链完好，但 `request/header.adapterDefaults` 归一化被改，是否影响 token-meter 的 envelope 复用判定**未证明** |
| 真实日志永不入库 | **强制** | 按既有约定，测试只用**手工合成的最小 fixture** |

## 13. 与 TokenScope 既有约束的对齐

- **只读原则**：全部数据取自 `sessions/`（含低世代残留文件与归档会话，**只读不删**）；
  如需缓存，只写 TokenScope 自己的 `~/.tokenscope/`。
- **统一用量事件模型**：dsh 适配器对外只需产出既有 `UsageEvent`（时间戳、agent、模型、
  输入/输出/缓存 token、会话与项目标识）——`data.usage` 四类语义桶（§6.3）、`source.model`、`time`、
  头行 `cwd` / `id` 一一对应，**聚合与渲染层无需改动**。
- **桶语义差异显式化**：`inputTokens` 为未缓存输入；模型标识按等价规则归一。
- **未知不吞 0**：usage 缺失桶保留未知态，计价走既有三态规则。
- **测试约定**：新增 dsh 适配器的三件套测试（路径发现含目录不存在、典型日志解析、
  畸形行容错）之外，必须**额外**覆盖：
  多帧压缩 fixture（≥2 帧，首帧仅头行）、**多世代同目录 fixture（断言不双计）**、
  usage 键集字段三角（各桶可选）与世代混用、
  **重试状态机 fixture**（`assistant/attempt` + `llm/retry-started` + `assistant/message`，
  断言与官方"替换/累加"语义一致）、
  **深度 ≥ 2 的子代理归属链**（按 `parentSession` 递归，断言主/子不双计且可归属）。

## 14. 证据与复现要点

上游关键 Agent Note（默认分支 `master`，均在 `.agents/notes/archived/architecture/`）：

| 文档 | 要点 |
| --- | --- |
| `2026-06-14-session-persistence.zh.md` | 持久化能力 seam、`SessionEvent` 原样落盘、头行在日志之外、仅追加与崩溃恢复 |
| `2026-07-19-zstandard-jsonl-session-logs.zh.md` | zstd 帧结构、**首帧只含头行**、**一次性解压只读首帧**的警告、按帧恢复与列举 |
| `2026-07-24-project-session-directories.zh.md` | 项目目录布局、**slug 编码有损** |
| `2026-08-05-large-session-jsonl-restore-pipeline.zh.md` | 帧遍历解码、增量 JSONL 扫描、协作式让出、撕裂尾部恢复 |
| `2026-07-15-replay-token-meter-service.zh.md` | **token meter 是启发式估算**（每 token 4 字符），不等于提供方 usage |
| `2026-07-23-unified-session-query-service.zh.md` | SQLite 仅为**可丢弃派生索引**，精确读取以权威源为准 |
| `2026-08-08-bounded-session-persistence-write-batching.zh.md` | 写入批量与节流：`writeEveryEvents` / `writeIntervalMs`，帧边界的来源（据 base 配置与交叉引用归纳） |

### 14.1 证据强度分级

复核时逐条追溯过来源，按可信度分三档：

**第一档 · 本机直接实测**（自写探针 / 本机数据，可复现）：
存储布局与规模、会话头字段与深度分布、`assistant/message` 结构与 usage 键集（含三键/两键样本）、
桶语义等式与投影闭合验证（delta 全 0）、多世代文件用量等价性、帧数与帧结构（首帧 1 行、
无空洞、逐帧独立可解）、Rust zstd 行为、性能数字、projcache 的域版本与各单元 `stateVersion`
与 `identity` 键集、归档会话仍在盘、`workspace.json` 覆盖缺口。

**第二档 · 上游 Agent Note 原文**（已逐篇阅读，见上表）：
持久化 seam 与仅追加语义、zstd 帧编码约定与"一次性解压只读首帧"的警告、项目目录布局与
slug 有损性、token-meter 的启发式定位、查询服务的派生索引定位、恢复流水线的帧遍历与撕裂尾部语义。

**第三档 · 仍属转述、尚未自验**：
`dsh-session-log-deepseek` 的**上传语义**（本机已确认该包存在且带 `[dsh-purge]` 补丁痕迹，
但其上传行为本身未自验）。

> **第二轮复核升级记录**（2026-10-11，逐条对照源码后由转述**升为自验**）：
> `dsh-token-meter` 的 `CHARS_PER_TOKEN = 4` 与 `ROLE_OVERHEAD = 4`；
> projcache 域 spec 的 `version: 7` / `compatibleVersions: [3,4,5,6]` /
> `invalidRecords: "backup-and-skip"` / `layout: "per-record"`；
> `asOfSeq` 的 `cursorBefore()` 语义（空日志 `-1`）；
> `dsh-session-query-sqlite` 的表名（`search_state`、`persisted_sessions`、虚拟表 `persisted_docs`）
> 与 `PRAGMA user_version = 8`；`dsh-headless` 的 `step/end` 仅在
> `complete && usage !== undefined` 时发布 usage；
> telemetry 的 `SessionTelemetryMode` 枚举仅含 `FEEDBACK_ONLY`（默认）与 `DISABLED`；
> `dsh` CLI 的**三个互斥**转储选项（本报告此前误写为"四类模式"）；
> §15 的两条补丁证据（`live.usage` 透传与 `updateUsage()` 四键映射）；
> 以及 `seq` 连续不变式的本机双文件实测。

**未逐字通读**：`2026-08-08-bounded-session-persistence-write-batching.zh.md`
（其内容据 base 配置与交叉引用归纳）。

本机实测口径（可复现）：

- **帧数**：`zstd -l -v <file>.jsonl.zstd`（权威值；实测魔数计数与之相符，但解析仍应走帧结构）。
- **帧结构**：按 RFC 8878 自写帧边界解析器（魔数 + 帧头 + 块头 + 可选校验和）逐帧切分；
  单个帧切片可用 `zstd::bulk::decompress` 独立解压，帧数须与 `zstd -l -v` 一致。
- **解压对照**：`zstd -d -o <tmp>.jsonl <file>.jsonl.zstd`（CLI 会遍历全部帧，作为真值）。
- **汇总闭合**：`Σ assistant/message.data.usage.*` 对比
  `storages/session_projcache/sessions/<id>.json` 的 `rows.tokenUsage.val.totals`。
- **本地实现源码**：`~/.dsh/profiles/node_modules/@deepseek-ai/`
  （`dsh-session-persistence-jsonl`、`dsh-session-projection-cache`、`dsh-session-stats`、
  `dsh-token-meter`、`dsh-session-format*` 等，均含 `README.zh.md`）。
  该目录是**符号链接**，真实源在
  `%APPDATA%\npm\node_modules\@deepseek-ai\dsh\node_modules\@deepseek-ai\`；
  **ripgrep 等工具默认不跟随符号链接**，直接对其下包目录检索会**返回空结果**，须改用真实路径。

探针脚本仅在系统临时目录运行，未入库、未写入 token 仓库；本文档不含任何真实会话内容。

## 15. 环境观察：本机安装存在本地补丁

本机的 dsh 安装**并非原版**：`dsh-purge`（profile `default` 的依赖）修改了安装树。
实测：`@deepseek-ai` 包目录下有 **54 个 `*.dshpurge.bak`**（`profiles` 下 0 个）；
`dsh/lib/bin.js` 第 2 行为 `import "./dsh-purge-hide-console.js"; // [dsh-purge] hide-console`。

- 受影响且与统计相关的包：`dsh-session`、`dsh-agent-loop`、`dsh-llm-deepseek`、
  `dsh-session-log-deepseek`（`enabled` 由 `true` 改为 `false`）等。
- **usage 传递链完好**（下列两处已逐行核对，非抽检转述）：
  `dsh-agent-loop` 中 `...live.usage === undefined ? {} : { usage: live.usage }` 仍在，
  `dsh-llm-deepseek` 中 `updateUsage()` 的四个 wire→local 键映射
  （`input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens`
  → `inputTokens` / `outputTokens` / `cacheReadTokens` / `cacheWriteTokens`）仍在。
  → **token 计量语义未见被削弱**。
- **存疑项**：`dsh-session` 中 `request/header.adapterDefaults` 的归一化路径被改过，
  而 `adapterDefaults` 正是 token-meter 判定 envelope 复用的一部分——
  **是否改变 envelope 相等性判定未逐行证明**，不影响本研究的日志侧结论（§6.3/§6.4
  的等式与闭合验证均为实测，与 token-meter 内部判定无关）。
- 安装根为**符号链接**：`~/.dsh/profiles/node_modules/@deepseek-ai/` 指向
  `%APPDATA%\npm\node_modules\@deepseek-ai\dsh\node_modules\@deepseek-ai\`。

**含义**：本文档中的**本机实测数字**（存储布局、事件结构、闭合验证、帧数、性能）可信；
但若后续以"官方 rc.2 行为"为契约做兼容，应知本机为打过补丁的安装，
`dsh-session` 侧的行为以原版为准复验更稳妥。
