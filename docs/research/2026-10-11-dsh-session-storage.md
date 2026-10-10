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
4. **四桶是并列相加关系**，`inputTokens` 的语义是**未缓存输入**，**不包含** `cacheReadTokens`。
   证据：`inputTokens + cacheReadTokens + outputTokens = totalTokens` 严格成立（§6.3）。
5. **Rust 侧不存在解码障碍**：`zstd` crate 的 `stream::read::Decoder` 会**自动续读串联帧**，
   解压结果与 `zstd -d` 逐字节一致（§9.1 已实测）。上游警告的"一次性解压只读首帧"在本机
   **Node 侧已复现**（`zstdDecompressSync` 单次仅得头行）；**Rust crate 未见该问题**，
   但仍须用 fixture 回归锁定（§9.1）。
6. 压缩产物是**多帧串联**，首帧只含头行；实测 88 个文件帧数 **min=2 / p50=228 / max=97421**。
   首帧无事件，故**任何只解首帧的实现都会静默丢失全部事件**。
7. `assistant/message.usage` 的**键集不固定**：跨世代不同（v0/v3 四键；v4 五键含
   `reasoningTokens`/`totalTokens` 而无 `cacheWriteTokens`），且本机实测**同一文件内也可混用**
   （593 行四键 + 63 行五键并存）。适配器必须按**字段存在性**容错，
   **缺失不得按 0 静默吞掉**，也不得按世代硬编码键集。
8. **同一会话目录可并存多个世代文件**（`session.jsonl.zstd` 与 `session.v3.jsonl.zstd` 等，
   实测 3/85 个目录）。它们承载**逐位相同的用量事实**，因此**按文件累加会精确双计**（§9.2）。
   必须以「会话 id + 最高世代」去重。
9. **子代理会话独立落盘、独立计量**，父会话汇总**不含**子代理用量（已做闭合验证）；
   全量纳入**不会双计**。本机 88 个文件中 43 个是子代理会话，占比近半，**不可忽略**。
10. **`tokenUsage.totals` 只累计 `assistant/message`**；`compaction/summary` 携带的真实用量
    （样本 `inputTokens: 521641`）**不计入**（§6.5）。这是口径缺口，也意味着
    **扫日志比读投影缓存更完整**。
11. `session_projcache` 是**缓存而非权威**：日志权威、缓存跟随、绝不领先，记录带 `seq` 水位，
    且无淘汰策略。可作为快速路径，不能当事实源。
12. **项目身份必须取头行 `cwd`，不能反解目录 slug**：slug 编码**有损**（上游明确承认
    `/a/b-c` 与 `/a-b/c` 会同名）。`workspace.json` 也不是完整清单（§7）。
13. 全量扫描本机语料（88 文件 / 138.96 MB）实测 **8883 ms**、**0 坏行**，
    横向可比 TokenScope 既有基准（Claude 1.2 GB 冷启动约 5.5 s）——**可行，但建议并行化 + 惰性 JSON 解析**。

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

本机规模实测：

- `sessions`：**85 个会话目录 / 88 个 `.zstd` 文件 / 138.58 MB**（最大单文件 49.67 MB）。
- `.dsh` 全目录约 160.5 MB，其中 `profiles`（依赖）仅 11.56 MB。

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

发现建议：

1. 解析根：显式配置项 > 非空白 `DSH_HOME` > 用户主目录下 `.dsh`。
2. 递归收集 `<root>/sessions/**/session*.jsonl.zst[d]`，**不要**假设层级固定，也不要假设
   "一会话一文件"（§9.2）。
3. 项目身份取头行 `cwd` 并规范化；**不要**用目录 slug，也**不要**依赖 `workspace.json`。
4. 全程只读打开。

## 4. 文件格式与世代

- 物理编码：**zstd**，后缀 `.jsonl.zstd`；配置 `compression: 'none'` 时为原始 `.jsonl`。
- 逻辑格式：**JSONL**，一行一条记录（先头行，后事件行）。
- 世代命名：v0 使用**无版本后缀** `session.jsonl[.zstd]`，正世代使用**小写**
  `session.vN.jsonl[.zstd]`。本机实测存在 **v0（无后缀）、v3、v4**。
- 上游约定：一份持久化根目录**只归属一种编码**，发现阶段拒绝相反后缀；不提供压缩转换、
  双重读取或基于扩展名的 fallback。
- **迁移保留源 generation**：逻辑版本迁移不删除旧文件，而是**并排发布**新世代文件——
  这就是 §9.2 多文件共存的成因。

本机世代分布（按文件）：主会话 `session-<uuid>` 目录 **45 个**（v0 24 / v3 8 / v4 13），
子代理裸 `<uuid>` 目录 **43 个**（v0 31 / v3 1 / v4 11）。

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

- `id`：主会话为 `session-<uuid>`；子代理为**裸 uuid**（无前缀）。**目录名与 `id` 一致**。
- `createdAt`：Unix epoch **毫秒**，非负安全整数，**拒绝小数值**。
- `cwd`：会话工作目录，**项目身份的正源**。
- `origin` / `parentSession` / `delegationDepth`：子代理谱系；`delegationDepth: 0` 为主会话。
- 元数据在**日志之外**（头行），不进入可回放事件流，也**不到达**消息投影。
- 头行位于**首个 zstd 帧**内，且该帧**只含头行** → **仅解首帧即可低成本枚举元数据**（列表场景）。

**子代理识别规则**（任一等价）：头行 `origin == "subagent"` / 存在 `parentSession` /
`delegationDepth > 0` / 目录名为裸 uuid。等价地可用投影缓存中 `subagent.identity.label` 非空。

## 6. 事件流与 token 用量字段

事件信封统一为 `{ type, seq, time, data }`；`seq` 从 0 连续递增且 `events[i].seq === i`
（上游不变式）。本机单会话实测类型分布（3888 行）：

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

### 6.2 usage 键集不固定：跨世代不同，且同一文件内可混用

**观测 1（跨世代，来自包内 codec 核验）**：`assistant/message.data.usage` 的键随
`header.version` 分派，`dsh-session-format-catalog` 定义 `currentVersion: 4` 与 v0→v4 迁移链：

| 世代 | `assistant/message.data.usage` 键 | 其他 |
| --- | --- | --- |
| v0（`session.jsonl.zstd`） | **四键** `inputTokens, outputTokens, cacheReadTokens, cacheWriteTokens` | 有原始 delta 事件（`assistant/chunk` 等）；assistant 消息**无** `stream` |
| v3 | **四键**（同 v0） | 消息带 `stream`；首次出现 `assistant/attempt` |
| v4（当前） | **五键** `inputTokens, outputTokens, cacheReadTokens, reasoningTokens, totalTokens`（**无** `cacheWriteTokens`） | — |

`compaction/summary` 同样随世代变化（v0/v3 三键无 `cacheWriteTokens`；v4 五键）。

**观测 2（同一文件内混用，本机实测）**：一个 `header.version = 3` 的文件内，
`assistant/message` 出现 **593 行四键形态**（seq 13–3535）与 **63 行五键形态**
（seq 3540–3884）并存——即**单个文件不完全等同于单一键集**，疑与该会话跨越 dsh 应用版本
升级期有关（未最终归因）。

**适配器要求（无论采用哪种解释都必须满足）**：

- **按字段存在性读取，不得按世代硬编码键集**，也不得假设"一个文件一种键集"。
- 缺失桶保留**未知**而非 0（与 TokenScope 既有"单价三态 / 不静默按 0 吞掉"原则一致）。
- `reasoningTokens` / `totalTokens` 在 v4 出现：`totalTokens` 是**派生校验值**（见 §6.3），
  `reasoningTokens` 目前**不参与**四桶求和（上游明确"不会再次加入推理计数"）。

> 另外注意 `assistant/attempt`：上游 token meter 会同时消费 `assistant/message` 的
> `data.usage` **与** `assistant/attempt` 的 stream 内最后一个 usage chunk。
> 本机样本中只用 `assistant/message` 即可闭合（§6.4，delta 全 0），
> 但适配器应**同时检查 `assistant/attempt`**，否则在"失败重试"场景可能漏计
> （上游语义：`llm/retry-started` 关闭替换槽，同一步重试会**累加**）。

### 6.3 桶语义与等式证据（最关键的口径）

变体 B 样本：`inputTokens: 435`、`cacheReadTokens: 716928`、`outputTokens: 355`、
`totalTokens: 717718`。

```
435 + 716928 + 355 = 717718 = totalTokens   ✓ 严格成立
```

**结论**：`inputTokens` 是**未缓存输入**，与 `cacheReadTokens` / `cacheWriteTokens` /
`outputTokens` **并列相加**；它**不是**"含缓存的总输入"。

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

另有 85 条投影记录 `decodeTokens == totals.outputTokens` **85/85 全匹配**。

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

- **`tokenUsage` 投影 = Σ`assistant/message`.usage**；`compaction/summary`、
  `session/title-llm-request` 这类**额外 LLM 调用不在其中**。
- 只读投影缓存 → 漏计压缩摘要消耗（本样本漏 521641 输入 token，约占该会话输入量 16%）。
- **扫日志 → 可以捕获**这些额外调用，口径比 dsh 自身汇总更完整。

**这是实现前必须定下的产品决策**：是否纳入压缩/标题等辅助调用。
建议**纳入并单独标记事件类别**，让用户可分辨"主对话用量"与"辅助开销"。

## 7. 子代理（subagent）计量独立性

- 子代理会话是**独立文件、独立会话 id、独立目录**，头行标 `origin: "subagent"` +
  `parentSession` + `delegationDepth ≥ 1`。
- **父会话 `tokenUsage.totals` 不含子代理用量**（§6.4：父/子各自闭合，父不含子）。
- 因此**全量纳入不会双计**；本机 88 个文件中 43 个（近半）是子代理会话，**不可忽略**。
- 建议：全部纳入，用 `parentSession` 建归属关系供 UI 区分"主会话 / 子代理"。
- **`workspace.json` 不是完整清单**：实测 85 个会话目录中，仅 37 个在
  `tables.workspaces[].sessionIds`、18 个在 `archivedSessionIds`，**其余 45 个（子代理）
  完全不在索引中**。发现流程**必须基于文件系统扫描**。
- **归档 ≠ 删除**：`archivedSessionIds` 的 18 个会话文件**全部仍在** `sessions/` 下，
  用量继续可读。

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

> 上例中的记录为**旧版遗留**（`tokenUsage.ver = 1`，而当前安装包该单元 `stateVersion = 2`），
> 仅用于展示结构；实际采信必须通过下方校验。

语义要点（上游包文档 + 代码核验 + 实测）：

- **日志权威，缓存跟随**：活会话检查点先把事件持久化，再写缓存记录；崩溃可让缓存**落后**
  于日志，但**绝不领先**。
- `asOfSeq` = **所有值共同反映到的最后一个事件的 `seq`**；空日志为 **`-1`**。
  持久缓存记录的 `asOfSeq` 属**该存储记录自身的水位**，**不可**与连接期活动会话的值直接比较
  （上游以 `kind: 'cached' | 'sequenced'` 区分）。
- 三条**必写点**：会话创建、`turn/end`、会话释放；其间由 `writeEveryEvents`（实测 base 配置
  **200**）与 `writeIntervalMs`（**5000**）节流 → **缓存最多滞后 200 事件或 5 秒**。
- 投影键由各单元注册：`tokenUsage`(sv 2) / `contextPressure`(sv 5) / `contextBreakdown`(sv 5)
  ← `dsh-token-meter`；`sessionStats`(sv 1) ← `dsh-session-stats`。域 `session_projcache`
  **version 7 / `compatibleVersions [3,4,5,6]` / `invalidRecords: backup-and-skip`**；
  行内 `ver` 不匹配即**丢弃重折（不迁移）**。
- **无淘汰/保留接口**——记录随会话持续累积。
- **`commandCodeCost` 不是官方投影**：它来自本机第三方 provider 插件
  （`@mars-sea/dsh-commandcode-provider`），按 `contextTokens` 分段 + `pricingKey` 内容指纹
  组织 `facts.groups[].tokens`。可作对照参考，**不可当官方语义**。
- 另有**旧布局遗留** `storages/session_projcache.json`（unit version 3）：仍含
  `tokenUsage` / `sessionStats` 行，但本机实测**最后写入于 2026-09-02，已停止更新**，
  只能作历史兜底。
- 键集随版本演进（实测 6 种 row-key 组合）；解析时必须容忍缺失键。

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

`dsh` CLI 顶层仅四类模式：profile 启动（`dsh <name>`）、`plugin`（转发 pnpm）、
`--dump-config`、`--dump-config-schema`——**没有 token/用量统计子命令**。

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

本机实测帧数分布（`zstd -l` 权威值，88 文件）：

| 指标 | 数值 |
| --- | --- |
| 文件数 | 88 |
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
| 分块读取 | 同上，**17248360** |

即 **Rust 的 `zstd` crate 会自动续读全部串联帧**（该文件 2156 帧），无需手工遍历帧头。

仍须执行的事项：

1. **写回归测试锁定该行为**：用**双帧 fixture**（首帧仅头行）断言解析到全部事件；
   crate 升级若改变语义会立刻红灯。
2. **断言首行必须是 `{"type":"session"…}` 头行**，作为"只解一帧"的廉价防线。
3. **不要用"魔数计数"当帧计数**：压缩负载内可能偶然出现 `28 B5 2F FD` 序列——
   实测该计数与权威值严重不符（同一文件 magic 计数 2 vs `zstd -l` 2156）。
4. **不要 `read_to_end` 整份明文**：实测解压比约 **3.9×**（17.9 MB → 119.9 MB；
   52 MB → 150 MB），最大文件展开后超 150 MB。用 `BufReader` + 逐行流式消费。

### 9.2 同一会话的多世代文件（**双计风险，必须处理**）

上游「迁移保留源 generation」的落地形态：**同一会话目录可并存多个世代文件**。
实测 **3/85** 个目录存在该情况：

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
- **不要依赖 `workspace.json`**（缺 45/85，§7）。
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
- 全量 8.9 s **含 88 次进程启动开销**（约 0.5–0.9 s）；同进程内批量处理会更快。
- 当前探针**对每一行都做完整 JSON 解析**，包括占比 90%+ 的非 `assistant/message` 行——
  这是主要成本。**优化方向**：先做廉价的字节级判别（是否含 `"assistant/message"`），
  仅对命中行做完整解析，预计显著提速。
- 参照 TokenScope 既有基准（本机 Claude 1.2 GB 冷启动约 5.5 s）：dsh 语料规模小得多，
  **串行 + 上述优化即可满足冷启动预期**；并行仅作可选加速。
- **可增量**：日志仅追加**且帧边界对应持久化批次**，因此可做**按帧前缀增量解析**
  （只解新增帧）——这是 dsh 格式相对其他 agent 的一个**结构性优势**。

## 11. 接入方案建议（TokenScope 视角）

1. **发现**：解析根（配置 > `DSH_HOME` > `~/.dsh`）→ 扫 `<root>/sessions/**/session*.jsonl.zst[d]`
   → 按**会话 id 分组**、**选最高世代**（§9.2）→ 仅解**首帧**读头行，得到
   `id` / `cwd` / `createdAt` / `version` / `parentSession` / `origin` / `delegationDepth`。
   这一步即可低成本完成列表与元数据枚举。
2. **解析**：逐帧流式解压 → 逐行判别 → 仅对 `assistant/message` 完整解析，
   提取 `data.usage` 四桶 + `data.message.source.{provider,model}` + `time` + `data.turn/step`。
   **绝不读取** `data.message.content` 与 `data.stream`（只读工具，避免大量无谓内存与隐私暴露面）。
3. **归一化**：`inputTokens` 作为**未缓存输入**桶，与 `cacheReadTokens` / `cacheWriteTokens` /
   `outputTokens` **并列**（**不要**套用 Claude/Qoder 的"input 含 cache"归一）。
4. **身份**：
   - 会话身份 = 头行 `id`（主会话含 `session-` 前缀；子代理为裸 uuid）。
   - 项目身份 = 规范化后的头行 `cwd`（沿用既有项目身份规则，不复用 slug）。
   - 逐请求指纹建议 `(会话id, seq)` 或 `(会话id, data.message.id)`；dsh 的 `seq` 在日志内
     连续且唯一，是天然稳定序号。
   - 模型身份走**等价键归一**（§6.1）。
5. **增量与缓存**：以「文件路径 + 文件长度 + 内容指纹」判增量；利用**按帧前缀增量解析**
   （§10）只处理新增批次。
6. **可选**：把 `session_projcache` 作为**冷启动加速**读会话级 totals，
   但必须通过 §8 的四项读取校验（`identity.formatVersion`、逐键 `ver`、`seq` 水位、滞后窗口），
   并标注口径（漏压缩/标题用量）；任何不一致都要**回落到日志解析**。

## 12. 待验证项与风险

| 项 | 状态 | 说明 |
| --- | --- | --- |
| Rust `zstd` crate 跨帧续读 | **已验证通过** | `Decoder`/`decode_all` 均完整解出 2156 帧；**仍需 fixture 回归测试锁定** |
| 多世代文件等价性 | **已验证** | 3/3 目录 usage 逐位一致；规则取最高世代 |
| 归档会话可达性 | **已验证（保留）** | 18 个归档文件全部仍在 `sessions/` 下 |
| usage 键集形态 | **已确认多变** | 跨世代不同（v0/v3 四键、v4 五键），且**同文件内可混用**；`compaction/summary` 另有独立键集。fixture 须覆盖"字段三角"（各桶可选） |
| `compaction/summary` 是否稳定带 usage | 样本少 | 本机 1 条样本带 usage；类型定义确认其为真实 LLM 调用（含 `llmStreamCall: true` 分支）。是否每次压缩都落 usage 待更多样本 |
| 标题生成 / web 搜索调用是否落 usage | **已确认：不落** | `session/title-llm-request` 带 `route{provider,model}` 但实测 11/11 无 `data.usage`；`web/deepseek-search-llm-request` 亦无。属真实消耗但**本地无数字**，只能标"未上报" |
| `assistant/attempt` 的 usage 贡献 | 本机未观测到 | 上游 token meter 同时消费它与 `assistant/message`；本机样本仅凭后者即闭合（delta 0），但**失败重试场景需实测** |
| 更深子代理嵌套 | 未验证 | 本机仅见 `delegationDepth: 1`；更深嵌套需按 `parentSession` 递归归属 |
| 多根/自定义根场景 | 未验证 | 若用户显式配置根，`DSH_HOME` 与实际落盘可能不一致，须以实际路径为准 |
| dsh 官方 CLI/遥测出口 | **已确认：无历史汇总命令** | 详见 §8.1 |
| 本地补丁对计量语义的影响 | **已验证未削弱** | 本机安装被 `dsh-purge` 改过（见 §15）；usage 传递链完好，但 `request/header.adapterDefaults` 归一化被改，是否影响 token-meter 的 envelope 复用判定**未证明** |
| 真实日志永不入库 | **强制** | 按既有约定，测试只用**手工合成的最小 fixture** |

## 13. 与 TokenScope 既有约束的对齐

- **只读原则**：全部数据取自 `sessions/`（含低世代残留文件与归档会话，**只读不删**）；
  如需缓存，只写 TokenScope 自己的 `~/.tokenscope/`。
- **统一用量事件模型**：dsh 适配器对外只需产出既有 `UsageEvent`（时间戳、agent、模型、
  输入/输出/缓存 token、会话与项目标识）——`data.usage` 四桶、`source.model`、`time`、
  头行 `cwd` / `id` 一一对应，**聚合与渲染层无需改动**。
- **桶语义差异显式化**：`inputTokens` 为未缓存输入；模型标识按等价规则归一。
- **未知不吞 0**：usage 缺失桶保留未知态，计价走既有三态规则。
- **测试约定**：新增 dsh 适配器的三件套测试（路径发现含目录不存在、典型日志解析、
  畸形行容错）之外，必须**额外**覆盖：
  多帧压缩 fixture（≥2 帧，首帧仅头行）、**多世代同目录 fixture（断言不双计）**、
  双 usage 变体 fixture（覆盖各桶可选、"A 有 B 无"的字段三角）、子代理与主会话的归属。

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
| `2026-08-08-bounded-session-persistence-write-batching.zh.md` | 写入批量与节流（帧边界来源） |

本机实测口径（可复现）：

- **帧数**：`zstd -l -v <file>.jsonl.zstd`（权威；勿用魔数计数替代）。
- **解压对照**：`zstd -d -o <tmp>.jsonl <file>.jsonl.zstd`（CLI 会遍历全部帧，作为真值）。
- **汇总闭合**：`Σ assistant/message.data.usage.*` 对比
  `storages/session_projcache/sessions/<id>.json` 的 `rows.tokenUsage.val.totals`。
- **本地实现源码**：`~/.dsh/profiles/node_modules/@deepseek-ai/`
  （`dsh-session-persistence-jsonl`、`dsh-session-projection-cache`、`dsh-session-stats`、
  `dsh-token-meter`、`dsh-session-format*` 等，均含 `README.zh.md`）。

探针脚本仅在系统临时目录运行，未入库、未写入 token 仓库；本文档不含任何真实会话内容。

## 15. 环境观察：本机安装存在本地补丁

本机的 dsh 安装**并非原版**：`dsh-purge`（profile `default` 的依赖）修改了安装树，
`dsh/lib/bin.js` 内有注入语句，安装根下存在约 40 个 `*.dshpurge.bak` 备份。

- 受影响且与统计相关的包：`dsh-session`、`dsh-agent-loop`、`dsh-llm-deepseek`、
  `dsh-session-log-deepseek`（`enabled` 由 `true` 改为 `false`）等。
- 抽检 diff 结论：改动均带 `[dsh-purge]` 注释；**usage 传递链完好**——
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
