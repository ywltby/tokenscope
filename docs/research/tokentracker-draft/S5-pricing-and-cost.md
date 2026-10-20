# S5 · TokenTracker 金额计算全链路（定价 / 计价 / 聚合 / 云端一致性）

样本仓库：`https://github.com/xiufengsun/TokenTracker`
检出：`C:\Users\admin\AppData\Local\Temp\tt-research`
HEAD：`b19d27b0ccd59ce621f4fa17207b06358e67987c`（`git describe` = `v1.3.14`）
研究方法：只读（`read` / `grep` / `glob` / PowerShell 读取 + 用 Node 只读解析 JSON 结构）。**未修改仓库任何文件，未执行仓库脚本，未 `npm install`。**
本文件所有路径均为相对仓库根；所有数字来自源码或 JSON 原文，未做任何估算。

---

## 0. 基线校正：已知情报与实测不符的两处

研究开始前给出的情报中两处行数与实测不一致，先记录，避免后续引用错位：`src/lib/pricing/index.js` 情报写 347 行、实测 **378** 行；`src/lib/pricing/matcher.js` 情报写 404 行、实测 **439** 行。验算方式是 Node 逐字节统计 LF（而不是 PowerShell `Get-Content`——后者在本机对该仓库文件少算 4–6 行）：两份文件的 LF 数分别恰为 378 与 439。文件字节数与情报一致（`seed-snapshot.json` 256869 字节、`curated-overrides.json` 29754 字节），说明盘上的文件就是目标修订，只是行数情报有误。**本文所有 `路径:行号` 以实测 LF 编号为准。**

情报中被证实为真的关键点（逐条已核）：`CLAUDE.md:64` 声称「成本只由 `input_tokens + output_tokens + cached_input_tokens + cache_creation_input_tokens + reasoning_output_tokens` 计算，绝不用 `total_tokens`」——**属实**，但需补三个前提（见 §1.2、§1.4）；实现函数确为 `computeRowCost`，位置确在 `src/lib/pricing/index.js`；「某 provider 只填 `total_tokens` 而 input/output 为 0 → dashboard 显示 $0」也属实，与 `CLAUDE.md:64` 原文及代码一致。

---

## 1. `computeRowCost` 逐行语义

### 1.1 签名、参数、返回结构

`src/lib/pricing/index.js:227`：

```js
function computeRowCost(row) {
  if (LOCAL_INFERENCE_SOURCES.has(String(row?.source || "").toLowerCase())) return 0;
  if (PI_SUBSCRIPTION_SOURCES.has(String(row?.source || "").toLowerCase())) return 0;
  const reportedCost = Number(row?.total_cost_usd);
  if (
    SOURCES_WITH_AUTHORITATIVE_COST.has(row?.source) &&
    Number.isFinite(reportedCost) &&
    reportedCost > 0
  ) return reportedCost;
  const pricing = getRowPricing(row);
```

- **签名**：`computeRowCost(row)` —— 单参数，无第二参数，无 options 对象（`src/lib/pricing/index.js:227`）。
- **参数**：`row` 是「队列行 / 模型用量行 / 技能用量行」的鸭子类型对象，只读 `source`、`model`、`total_cost_usd`、`hour_start`、`pricing_tier`、`input_tokens`、`output_tokens`、`cached_input_tokens`、`cache_creation_input_tokens`、`reasoning_output_tokens`、`long_context_*`、`priority_*`（`src/lib/pricing/index.js:228-350`）。**不要求 `total_tokens`**。
- **返回结构**：纯 `number`（USD），不是对象、不是字符串。无命中价格时返回 `0`，也就是说「未定价」与「免费」在这一层**不可区分**——需要区分时走 `getModelPricingInfo`（`src/lib/pricing/index.js:141-176`）。
- 导出：`src/lib/pricing/index.js:371`；上游 `src/lib/local-api.js` 在 `:57-64` 从 pricing 模块 **import 解构**（`:60` 即 `computeRowCost,`），并在 `:3527-3531` 的 `module.exports` 段**再导出**同一符号（`:3530`）。两者一个是入口、一个是出口，不是两处独立定义。

### 1.2 三条前置短路（在算钱之前就返回）

**(a) 本地推理 = 0**

```js
const LOCAL_INFERENCE_SOURCES = new Set(["lmstudio"]);
```

`src/lib/pricing/index.js:27`。命中即 `return 0`（`:228`）。注释明确了语义：「LM Studio's server logs describe inference served by the local developer server (including LM Link). Secure Cloud usage has a separate billing path and is not present in these logs.」（`:24-26`）。

**(b) Pi 的订阅制 Copilot 路由 = 0**

```js
const PI_SUBSCRIPTION_SOURCES = new Set([
  "pi-github-copilot",
  "pi-copilot",
  "prime-agent-github-copilot",
  "prime-agent-copilot",
]);
```

`src/lib/pricing/index.js:18-23`。命中即 `return 0`（`:232`）。注意这个集合比云端 edge 版本**多两个成员**（`prime-agent-*`），见 §7.4。

**(c) 服务端上报成本优先（仅两个 source）**

```js
const SOURCES_WITH_AUTHORITATIVE_COST = new Set(["grok", "cline"]);
```

`src/lib/pricing/index.js:32`。命中条件是 `Number.isFinite(reportedCost) && reportedCost > 0`（`:237-241`）。**`> 0` 而不是 `>= 0`**：注释写明 `zero remains the legacy "unreported" sentinel`（`:234-235`），即 0 被视为「没上报」，会继续走模型定价。

`src/lib/pricing/index.js:228-241` 这 14 行是 CLAUDE.md 那条「成本只由五桶计算」结论的**第一个反例**：对 `grok` / `cline` 两个来源，金额可以完全绕开价格表，直接用 `row.total_cost_usd`。

### 1.3 五个桶各自乘哪个价格字段（核心算式）

`src/lib/pricing/index.js:254-264`：

```js
  const reasoningCost = reasoningIncludedInOutput
    ? 0
    : (row.reasoning_output_tokens || 0) * (pricing.output || 0);
  const baseCost = (
    ((row.input_tokens || 0) * (pricing.input || 0) +
      (row.output_tokens || 0) * (pricing.output || 0) +
      (row.cached_input_tokens || 0) * (pricing.cache_read || 0) +
      (row.cache_creation_input_tokens || 0) * (pricing.cache_write || 0) +
      reasoningCost) /
    1_000_000
  );
```

映射表（**源码唯一来源**：`src/lib/pricing/index.js:257-264`）：

| token 桶 | 价格字段 | 位置 |
|---|---|---|
| `input_tokens` | `pricing.input` | `index.js:258` |
| `output_tokens` | `pricing.output` | `index.js:259` |
| `cached_input_tokens` | `pricing.cache_read` | `index.js:260` |
| `cache_creation_input_tokens` | `pricing.cache_write` | `index.js:261` |
| `reasoning_output_tokens` | `pricing.output`（条件性，见 §1.4） | `index.js:256` |

三个必须同读的细节：

1. **单位是 USD / 百万 token**，除数硬编码为 `1_000_000`（`index.js:263`）。价格表内部一律以 MTok 计价（`curated-overrides.json:4` 的 `"units": "usd_per_million_tokens"`）。
2. **缺失字段不报错、不推导、不插值**，一律 `(pricing.x || 0)` 落到 0。所以「价格表没写 `cache_write`」≡「缓存写入免费」。这一条是理解整份价格表的关键（见 §5）。
3. **不使用 `total_tokens`**，也不使用 `billable_total_tokens`（后者只服务展示层与热度图，见 `src/lib/local-api.js:284`、`:614`、`:2565`）。

### 1.4 推理 token：是否重复计费、是否被并入 output 后二次计费

这是全链路里唯一「按 source 分支」的语义，`src/lib/pricing/index.js:248-256`：

```js
  // OmO, like Codex, reports reasoning as a subset of `output` (its own
  // usage.cost bills no separate reasoning component), so charging it again
  // here would double-bill every reasoning token. Cline is the same: its
  // `outputTokens` is the AI SDK total (text + reasoning), so
  // reasoning_output_tokens is a subset marker, never a second line item.
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

结论：

- **折叠集合**（`reasoning ∈ output`，不再单独计费）：`codex`、`acode`、`every-code`、`omo`、`cline`——共 5 个 source（`index.js:248-253`）。
- **非折叠集合**（`reasoning` 与 `output` 互斥，按 output 单价单独计费）：其余所有 source，包括 `claude`、`gemini`、`cursor`、`opencode` 等等（`index.js:256`）。这里「单独计费」不等于「重复计费」：只有当该 provider 的 `output_tokens` 本身不含推理时才是正确的，反之就是双计。
- **判断依据是 source 字符串，不是模型**。因此同一个模型名走不同 source 会得到不同价格。已被测试锁定：
  - `test/gpt-6-astra-pricing.test.js:44-48`：同一行 `source: "codex"` → 73.5，`source: "every-code"` → 73.5，`source: "gemini"` → **98.5**（差 25 = 50 万 reasoning × $50/MTok ÷ 1e6）。
  - `test/pricing.test.js:1164-1183`：「`computeRowCost` on non-Codex source DOES bill reasoning tokens」。
  - `test/pricing.test.js:1111-1134`：Codex 行不双计推理。
  - `test/model-breakdown.test.js:427-446`：非 Codex source 仍按 output 价计推理。
- **不存在「先并入 output 再二次计费」的路径**：`reasoningCost` 是 `baseCost` 的加项（`index.js:262`），而 `output_tokens` 在 `index.js:259` 只乘一次；没有任何代码把 `reasoning_output_tokens` 加进 `output_tokens` 再计价。全局搜索 `src/`，`reasoning_output_tokens` 出现在定价代码里只有 `index.js:256,299,327,348` 四处（全部是加项或子集标注）。
- **`reasoning_output_tokens` 也不是 `total_tokens` 的组成部分**：`CLAUDE.md:61` 与 `test/gpt-6-astra-pricing.test.js:162-178` 都断言 `total_tokens = input + output + cache_creation + cache_read + reasoning`（对不折叠的 source）；对折叠的 5 个 source，`reasoning` 是「信息性子集」。这直接影响 `scripts/audit-token-correctness.cjs` 的审计口径（见 §9.3）。

### 1.5 `total_tokens` 是否真的从不参与：源码证据

在 `src/lib/pricing/index.js` 全文 378 行中搜索 `total_tokens`：

- `:236` —— `const reportedCost = Number(row?.total_cost_usd);`。**这是 `total_cost_usd`（金额字段），不是 `total_tokens`。** 文件名与字段名的视觉相似是唯一的「疑似命中」。
- 除此之外，`src/lib/pricing/` 目录下三个 JS 文件（`index.js` / `matcher.js` / `litellm-fetcher.js`）中 `total_tokens` 这个标识符总共只出现 **1 次**，而且是在注释里（`index.js:283-284`：`total_tokens stays the sum of the base columns whether or not they are present`）——**没有任何一处读取 `row.total_tokens`**。（`matcher.js` 与 `litellm-fetcher.js` 中该标识符出现 0 次。）

反向验证（排除「被间接喂入」的可能）：`computeRowCost` 的唯一计价分支是 `baseCost` 与两个溢价块，全部只读 `pricing.*` 与五个 token 列（`index.js:257-354`）。`getRowPricing`（`:209-222`）只读 `model` / `source` / `pricing_tier` / `hour_start`。

同一个不变量在 `src/lib/cursor-config.js:528` 被再次书面确认：

```
// per-column tokens × MODEL_PRICING (see computeRowCost — it never
```

所以 `CLAUDE.md:64` 的「绝不用 `total_tokens`」是**准确的**。但要把这条结论的边界说清楚，否则会误读为「成本只看五桶」——实际上 `computeRowCost` 的返回值有三个可能来源：

1. 常数 `0`（`lmstudio` / Pi 订阅源，`index.js:228`、`:232`）；
2. `row.total_cost_usd`（`grok` / `cline` 且为正，`index.js:236-241`）；
3. 五桶算式（其余情况，`index.js:254-354`）——其中 `reasoning` 那一桶对 5 个 source 恒为 0，等价于四桶。

### 1.6 OpenAI 长上下文溢价（>272K 输入，2× 输入 / 1.5× 输出）

阈值常量：

```js
const OPENAI_LONG_CONTEXT_INPUT_THRESHOLD = 272_000;
```

`src/lib/pricing/index.js:31`。**注意：这个常量在 `index.js` 内部只被定义（`:31`）和导出（`:375`），从未参与计算。** 真正做阈值判断的是解析期的另一份同名常量 `src/lib/codex-rollout-parser.js:51`，在 `:733` 使用：

```js
    const isLongContext =
      Number(rawRequestUsage?.input_tokens || 0) > OPENAI_LONG_CONTEXT_INPUT_THRESHOLD;
```

这是一个**双份常量、无测试守护**的漂移点（本文 §11 列为待复核项）。**【R1 复核状态：已验证】**`index.js` 全文 378 行逐行读过，该常量只有 `:31` 定义与 `:375` 导出、**零内部读取**；生效点确为 `src/lib/codex-rollout-parser.js:51`（注释 `:49-50` 自述「Match src/lib/pricing/index.js」）→ `:733`；`src/lib/session-analytics.js:245` 是注释引用。

适用模型（硬编码白名单，`src/lib/pricing/index.js:267-269`）：

```js
  const usesOpenAILongContextTier =
    model === "gpt-5.6" || model.includes("gpt-5.6-sol") || model.includes("gpt-6-astra") ||
    model.includes("gpt-6-sol") || model.includes("gpt-6.1-sol");
```

溢价是 `baseCost + longContextPremium`，`longContextPremium` 为 0 时直接返回 `baseCost`（`:275`、`:308`）。溢价公式（`src/lib/pricing/index.js:300-306`）逐项为 `longInput×input + longCached×cache_read + longCacheWrite×cache_write + 0.5×longOutput×output + 0.5×longReasoning×output`，再 `/1e6`。

语义要点：

- 溢价读的是**子集列** `long_context_*`，不是总列。`long_context_input_tokens` 等五列由解析器只对「原始（含缓存）输入 > 272K 的请求」累加（`src/lib/codex-rollout-parser.js:732-741`）。
- 每一个子集列都被 `bounded(value, total)` 夹在 `[0, 对应基础列]` 之间（`src/lib/pricing/index.js:277-296`），即「子集不可能大于全集」，超报被截断而不是外推。
- 溢价**恰好叠加一次**在原价之上：`baseCost + longContextPremium`（`:308`）。
- 不折叠推理的 source（如 `gemini`）才会计长上下文推理的溢价；折叠集合恒为 0（`:297-299`）。
- 模型不在白名单 → `longContextPremium = 0`，直接 `return baseCost`（`:275`）。
- 被测试锁定：`test/pricing.test.js:1136-1162`（Sol 长上下文只对观测子集生效）、`test/gpt-6-astra-pricing.test.js:50-62`（「large aggregate is not a long request」——100 万 token 的行聚合不触发长上下文）。

### 1.7 Astra / Sol 的 Fast（priority）档：乘子而非替换

价格表字段 `priority_multiplier`（`src/lib/pricing/index.js:273-275`）：

```js
  const priorityMultiplier = Number(pricing.priority_multiplier);
  const hasPriorityRates = Number.isFinite(priorityMultiplier) && priorityMultiplier > 1;
  if (!usesOpenAILongContextTier && !hasPriorityRates) return baseCost;
```

只有 `curated-overrides.json` 中显式带 `priority_multiplier` 的条目才会进入这个分支。实测只有 3 条（`curated-overrides.json` 的 `exact` 节点）：`gpt-6-astra` = 2、`gpt-6.1-sol` = 2、`gpt-6-sol` = 2。

溢价公式（`src/lib/pricing/index.js:328-354`）分两段：

```js
  const priorityBase = (
    priorityInput * (pricing.input || 0) +
    priorityCached * (pricing.cache_read || 0) +
    priorityCacheWrite * (pricing.cache_write || 0) +
    priorityOutput * (pricing.output || 0) +
    priorityReasoning * (pricing.output || 0)
  ) / 1_000_000;

  const priorityLong = usesOpenAILongContextTier ? (
    bounded(row.priority_long_context_input_tokens, Math.min(priorityInput, longInput)) *
      (pricing.input || 0) +
    ...
  ) / 1_000_000 : 0;

  const priorityPremium = (priorityMultiplier - 1) * (priorityBase + priorityLong);
  return baseCost + longContextPremium + priorityPremium;
```

关键语义（都有测试）：

- **乘子只作用于观测子集**：`priority_*` 也是「注解列」，不是加数（`index.js:282-283` 注释原文：`They are annotations, never addends`；整段注解说明为 `:281-285`）。
- **长上下文 × Fast 是求交集，不是把两个溢价相加**：`priority_long_context_*` 列由解析器显式记录（`src/lib/codex-rollout-parser.js:742-762`）；缺少交集列时（例如云端队列行不带上下文子集）溢价回落到短上下文档乘子，`priorityLong = 0`（`index.js:336-351`，注释 `:311-317`）。
- `service_tier` 标量、`-fast` 模型后缀**都不会**改变价格，只有列存在才改变：`test/gpt-6-astra-pricing.test.js:115-122`（`assert.equal(..., { ...issueRow, service_tier: "priority" }), 1.55)`）。
- 交集列同样被 `Math.min` 夹紧（`index.js:337-350`），超报不会抬价（`test/gpt-6-astra-pricing.test.js:153-159`）。
- Fast 档的申报依据写在价格表注释里：`curated-overrides.json:98` 明确「The 2.5x figure in the Codex speed docs is a subscription-credit rate and must not be used here.」

`service_tier` 的来源是 Codex 的独立 rollout 记录，不在 `token_count` 行上（`src/lib/codex-service-tier.js:1-15`），观测值只有 `"default"` 与 `"priority"`，接受集合为 `["priority","fast"]`（`codex-service-tier.js:26`）。**未记录 tier 的请求一律按 Standard 计费，不猜测**（`codex-service-tier.js:12-15`）。

### 1.8 `getRowPricing`：DeepSeek 峰谷折扣的唯一入口

`computeRowCost` 并不直接调 `getModelPricing`，而是调 `getRowPricing`（`src/lib/pricing/index.js:242`）。

`src/lib/pricing/index.js:209-222`：

```js
function getRowPricing(row) {
  const pricing = getModelPricing(row?.model, { source: row?.source });
  // AStudio uses iFlytek MaaS fixed prices and does not inherit DeepSeek public API
  // time-based discounts.
  if (String(row?.source || "").toLowerCase() === "acode") return pricing;
  if (!isDeepSeekTimePricedModel(row?.model) || !isDeepSeekOffPeak(row)) return pricing;
  return {
    ...pricing,
    input: (pricing.input || 0) * 0.5,
    output: (pricing.output || 0) * 0.5,
    cache_read: (pricing.cache_read || 0) * 0.5,
    cache_write: (pricing.cache_write || 0) * 0.5,
  };
}
```

- 折扣是**四个价格字段全部 × 0.5**（不区分桶），不是「折扣价单独一张表」。
- 折扣命中的模型名单是硬编码的 4 个：`deepseek-v4-flash`、`deepseek-v4.1-flash`、`deepseek-flash`、`deepseek-v4-pro`（`src/lib/pricing/index.js:34-39`），匹配方式是 `lower.includes(name)`（`:180`）。
- `acode`（AStudio / iFlytek MaaS）**显式豁免**（`:213`）。
- 详细峰谷判定见 §4.3。

---

## 2. 价格表来源与优先级

### 2.1 体系总览：三层

定价有**三个互不相同的表**，加载顺序与优先级不同：

| 层 | 文件 / 来源 | 载体 | 优先级 |
|---|---|---|---|
| L0 | 内置「策展表」 | `src/lib/pricing/curated-overrides.json`（29754 字节，775 行，84 条 `exact`） | **最高**，`require` 进内存，永不被网络覆盖 |
| L1 | LiteLLM 主表（运行期） | 磁盘缓存 `~/.tokentracker/cache/pricing.json` → 上游 URL → 陈旧缓存 → 内置 seed | 次之 |
| L2 | LiteLLM seed 快照 | `src/lib/pricing/seed-snapshot.json`（256869 字节，2284 条模型） | 最低（离线兜底） |

`src/lib/pricing/matcher.js:3-12` 把完整解析顺序写成文档注释，共 8 步（见 §3）。

**没有 models.dev、没有 OpenRouter、没有用户可编辑的外置价格文件。** L1 的唯一上游是 LiteLLM（`src/lib/pricing/litellm-fetcher.js:10-11`）。

### 2.2 `seed-snapshot.json`：来源、结构、生成脚本

**来源**：LiteLLM 官方价目表，构建期抓取。

`src/lib/pricing/litellm-fetcher.js:10-11`：

```js
const LITELLM_PRICING_URL =
  "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
```

**生成脚本**：`scripts/build-pricing-seed.cjs`（71 行）。触发方式两处：

- `package.json` 的 `prepublishOnly` → `node scripts/build-pricing-seed.cjs`（`scripts/build-pricing-seed.cjs:3` 注释：`Runs from \`npm prepublishOnly\``）。
- 手工 `npm run pricing:build-seed`（`package.json` scripts 中的 `"pricing:build-seed": "node scripts/build-pricing-seed.cjs"`）。

**结构**（脚本 `scripts/build-pricing-seed.cjs:33-59` 与实测一致）：

- 顶层是一张**扁平的 `模型名 → 条目` 映射**，另加一个 `_meta` 键。
- `_meta` 只有三个字段：`source`、`generated_at`、`kept_models`。
- 每条记录**只保留 4 个价格字段**（`scripts/build-pricing-seed.cjs:14-19`）：`input_cost_per_token`、`output_cost_per_token`、`cache_read_input_token_cost`、`cache_creation_input_token_cost`。这是一份白名单——LiteLLM 里的其它字段（上下文长度、模态、`input_cost_per_image` 等）在生成期就被丢弃。

**实测原文（`src/lib/pricing/seed-snapshot.json`）**：

- 字节数 256869；顶层键 2285 个，其中 `_meta` 1 个、模型 2284 个，与 `_meta.kept_models = 2284` 一致。
- `_meta` 原文：`{"source":"https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json","generated_at":"2026-05-26T11:12:29.761Z","kept_models":2284}`。
- 字段直方图（2284 条记录中出现次数）：`input_cost_per_token` 2274、`output_cost_per_token` 2273、`cache_read_input_token_cost` 566、`cache_creation_input_token_cost` **188**。
- 全部字段名集合恰为上述 4 个，无其它字段（脚本的 `KEEP_FIELDS` 白名单生效）。
- 首 5 键：`sample_spec`、`ai21.j2-mid-v1`、`ai21.j2-ultra-v1`、`ai21.jamba-1-5-large-v1:0`、`ai21.jamba-1-5-mini-v1:0`。末 5 键：`bedrock_mantle/openai.gpt-oss-safeguard-20b`、`bedrock/us-east-1/zai.glm-5`、`bedrock/us-west-2/zai.glm-5`、`bedrock/us-gov-east-1/anthropic.claude-haiku-4-5-20251001-v1:0`、`bedrock/us-gov-west-1/anthropic.claude-haiku-4-5-20251001-v1:0`。

`sample_spec` 是 LiteLLM 自带的样例条目，被原样保留（值可在文件中直接检视，非 TokenTracker 添加）。

**数值语义**：LiteLLM 的字段是 **USD / 单 token**，不是 MTok。内部换算在 `src/lib/pricing/matcher.js:398-414`：

```js
function convertLitellmEntry(entry) {
  if (!entry || typeof entry !== "object") return null;
  const out = {};
  if (typeof entry.input_cost_per_token === "number") {
    out.input = roundToTenDecimals(entry.input_cost_per_token * 1_000_000);
  }
  if (typeof entry.output_cost_per_token === "number") {
    out.output = roundToTenDecimals(entry.output_cost_per_token * 1_000_000);
  }
  if (typeof entry.cache_read_input_token_cost === "number") {
    out.cache_read = roundToTenDecimals(entry.cache_read_input_token_cost * 1_000_000);
  }
  if (typeof entry.cache_creation_input_token_cost === "number") {
    out.cache_write = roundToTenDecimals(entry.cache_creation_input_token_cost * 1_000_000);
  }
  return Object.keys(out).length ? out : null;
}
```

注意三件事：

1. 字段名映射：`cache_read_input_token_cost → cache_read`、`cache_creation_input_token_cost → cache_write`（`matcher.js:407-412`）。
2. `roundToTenDecimals` 是为了消除浮点漂移：`1e-7 * 1e6 = 0.09999999999999999`（`matcher.js:391-396`）。
3. **没有任何价格字段被推导**（例如没有 `read = input × 0.1`）。缺就是缺。`buildLitellmPerMillionMap` 还会丢弃「一个价格字段都没有」的条目（`matcher.js:418-427`）。

**seed 加载是同步的、require 期就完成**（`src/lib/pricing/index.js:40-55`）：

```js
function loadSeedSync() {
  try {
    const raw = fs.readFileSync(SEED_SNAPSHOT_PATH, "utf8");
    const parsed = JSON.parse(raw);
    delete parsed._meta;
    return parsed;
  } catch (e) {
    return {};
  }
}
```

`src/lib/pricing/index.js:44-53`。注释（`:40-43`）说明了动机：`Done at require-time so callers that haven't awaited ensurePricingLoaded() (e.g. tests, vite mock startup, edge functions) still get LiteLLM-backed pricing instead of all-zero.` **seed 读取后立刻 `delete parsed._meta`**——所以运行期内存里的 raw map 不含 `_meta`（`:48`）；`buildLitellmPerMillionMap` 里对 `_` 前缀键的跳过（`matcher.js:421-422`）是为磁盘缓存路径准备的冗余防护。

### 2.3 `curated-overrides.json`：结构、语义、优先级

**顶层结构**（实测 5 个键）：`_meta`、`exact`（84 条）、`alias`（1 条）、`source_exact`（1 个 source：`acode`，64 条）、`fuzzy`（47 条）。

**语义**：`curated-overrides.json:3` 的 `_meta.note` 原文（这是全表最权威的自述）：

> Curated price overrides. Always wins over LiteLLM. Two reasons to live here: (1) self-defined alias names that LiteLLM will never carry (kiro-*, hy3-*, composer-*, kimi-for-coding, free-tier OpenRouter routes); (2) prices we want to pin even if LiteLLM has the model (e.g. cache_write fields LiteLLM often omits). Units: USD per million tokens. Edit this file to override pricing without redeploying.

`curated-overrides.json:4` 另有一行机器可读单位声明：`"units": "usd_per_million_tokens"`。

所以：**curated 是覆盖（override），不是补充（supplement）**，且优先级严格高于 LiteLLM（`matcher.js:288-300` 的 step 0/1 早于 `:302-313` 的 step 2）。

**逐条原文（3–5 个真实条目）**

条目一 —— 手工修正的 Anthropic 顶配 SKU，明确写了官方核价日期（`curated-overrides.json:9-15`）：

```json
    "claude-opus-5-5": {
      "input": 4,
      "output": 20,
      "cache_read": 0.2,
      "cache_write": 5,
      "note": "Official Anthropic pricing verified 2026-10-11: https://platform.claude.com/docs/en/about-claude/pricing"
    },
```

条目二 —— 手工补上 LiteLLM 尚未收录的模型，并说明缓存价比（`curated-overrides.json:50-56`）：

```json
    "claude-fable-5": {
      "input": 10,
      "output": 50,
      "cache_read": 1,
      "cache_write": 12.5,
      "note": "Fable 5 \u2014 Anthropic's top tier above Opus. Not yet in LiteLLM. $10/$50 per MTok; cache_read 0.1x, cache_write 1.25x. Remove once LiteLLM carries it."
    },
```

条目三 —— 带 `priority_multiplier` 的 OpenAI 旗舰档，注释同时说明了长上下文与「不要用 2.5x」的坑（`curated-overrides.json:92-99`，节选首尾）：

```json
    "gpt-6-astra": {
      "input": 10,
      "output": 50,
      "cache_read": 1,
      "cache_write": 12.5,
      "priority_multiplier": 2,
      "note": "Official Standard USD/MTok rates verified 2026-09-07: https://developers.openai.com/api/docs/models/gpt-6-astra. Requests above 272K input tokens use 2x input/cache and 1.5x output rates; computeRowCost applies this only to observed long-context request subsets. priority_multiplier is the Fast tier, whose published table (20/2/25/100 short, 40/4/50/150 long, verified 2026-09-12) is exactly 2x the Standard rate of the same context tier ... The 2.5x figure in the Codex speed docs is a subscription-credit rate and must not be used here."
    },
```

条目四 —— source 级覆盖，防止把 Cline 自家网关的免费额度按公共价误计（`curated-overrides.json:43-49`，节选）：

```json
    "cline-gateway-models": {
      "input": 0,
      "output": 0,
      "cache_read": 0,
      "cache_write": 0,
      "note": "Cline's own gateway namespaces (`cline-free/*` free tier, `cline-pass/*` flat-rate Cline Pass). Cline's shipped model catalog declares pricing 0 for every id in both namespaces, so a turn there has no marginal per-token cost — same convention as PI_SUBSCRIPTION_SOURCES. The fuzzy entries at the top of `fuzzy` route both prefixes here BEFORE the model names can match a public rate ..."
    },
```

条目五 —— DeepSeek 官方峰值档（折扣由代码施加，表里只存峰值价），见 `curated-overrides.json:5` 的 `deepseek_v4_pricing_note` 与 `exact.deepseek-v4-pro`：

```json
    "deepseek-v4-pro": { "input": 1.32, "output": 3.96, "cache_read": 0.044, "cache_write": 1.32 },
```

**哪些模型被手工修正过**（按 `note` 字段自述分类）：

- 手写别名、LiteLLM 永不会有：`kiro-*`、`hy3-*`、`hy4-*`、`composer-*`、`kimi-for-coding`、`k3`、`k3-256k`、`cline-gateway-models`、以及全部 `*-free` 条目（`glm-4.7-free`、`nemotron-3-super-free`、`mimo-v2-pro-free`、`minimax-m2.1-free`、`kimi-k2.5-free`、`grok-build-free`、`grok-4.5-build-free`）。
- 有 LiteLLM 条目但被「钉住」以消除缓存差异：`claude-haiku-4-5`（note：`Pin to the same rates as claude-haiku-4-5-20251001 so local cost does not depend on which dated LiteLLM aliases are present in the current cache.`）、`claude-sonnet-5`（note：`pin explicitly so pricing is correct immediately rather than depending on the 24h local pricing-cache refresh cycle`）。
- 补齐 `cache_write`（LiteLLM 常缺）：`claude-opus-5*`、`claude-sonnet-5*`、`claude-fable-5`、`gpt-6-*`、`gpt-5.6-*`、`deepseek-*`、`hy3/hy4`、`MiniMax-M2.7*`、`qwen3.8-flash`、`step-*`、`longcat-2.0`、`sakana/fugu-ultra`。
- 按汇率从 RMB 换算：`hy3/hy4`（Tencent TokenHub，`~7.2 RMB/USD`，见 `curated-overrides.json:6` 与 `test/pricing.test.js:1313`）、`longcat-2.0`、`step-*`、以及 `source_exact.acode` 全部 64 条。

**`fuzzy` 的语义**：`{ match, ref }` 数组，`ref` 必须是 `exact` 里存在的键（`matcher.js:335` 会 `if (!curated.exact[ref]) continue;` 跳过失效引用）。**顺序即优先级**，因为匹配是「第一个命中的就返回」（`matcher.js:331-343`）。顺序错误已被测试锁死，例如 `test/edge-pricing-parity.test.js:159-160` 断言 `glm-5.3-flash` 必须排在 `glm-5.3` 之前、`glm-5.3` 必须排在 `glm-5` 之前，注释给出量化后果：「or flash rows bill at 6.7x」（`test/edge-pricing-parity.test.js:158`）。

**`source_exact` 的语义**：按 source 分域的精确表，只有 `acode` 一个 source。命中即可直接返回（`matcher.js:280-286`），`sourceExact` 表的查找也是大小写不敏感（`lookupExactCaseInsensitive`，`matcher.js:282`）。

**`alias` 的语义**：字面映射，实测只有 1 条：

```json
{
 "auto": "composer-1"
}
```

`matcher.js:316-322` 会额外校验 `curated.exact[alias[model]]` 存在才返回，避免别名指向空条目。

**代码里硬编码的 `curated.exact` 依赖**（改这张表会破坏这些路径）：

- `src/lib/pricing/index.js:363`：`const MODEL_PRICING = curatedOverrides.exact;` —— 向后兼容导出，注释（`:357-362`）说明它的用途与「LiteLLM 条目不在这里」的原因。
- `matcher.js:291`：Qwen3.8 Flash 的 provider 前缀/日期变体走了单独特例正则，条件里直接检查 `curated.exact?.["qwen3.8-flash"]`。

### 2.4 `litellm-fetcher.js`：何时调用、端点、缓存位置、失败降级

**何时被调用：运行期，且是「后台不阻塞启动」的方式。**

`src/commands/serve.js:121-129`：

```js
  // 2.1 Refresh LiteLLM pricing data in the background. The seed snapshot is
  //     already loaded synchronously at require-time, so cost calculation is
  //     functional right now; ensurePricingLoaded() only upgrades to fresh
  //     disk cache or upstream data. Awaiting it here would block startup
  //     for the full 10s fetch timeout when offline / behind a firewall.
  const { cacheDir } = await resolveTrackerPaths();
  ensurePricingLoaded({ cachePath: path.join(cacheDir, "pricing.json") }).catch(
    (e) => process.stdout.write(`Pricing refresh warning: ${e?.message || e}\n`),
  );
```

构建期路径是另一条：`scripts/build-pricing-seed.cjs` 在 `prepublishOnly` 抓一次上游，写进仓库内的 `seed-snapshot.json`（`scripts/build-pricing-seed.cjs:11`、`:61`）。

**网络端点**：见 §2.2 的 `LITELLM_PRICING_URL`（`litellm-fetcher.js:10-11`）。

**超时与 TTL**（`litellm-fetcher.js:12-13`）：

```js
const DEFAULT_TTL_MS = 24 * 60 * 60 * 1000; // 24h
const DEFAULT_FETCH_TIMEOUT_MS = 10_000;
```

**缓存文件位置与形状**（`litellm-fetcher.js:68-105`）：写盘前会把上游条目**裁成 4 个价格字段**，并写一个 `_meta`（`litellm-fetcher.js:95-103`）：

```js
  const payload = {
    _meta: { source: LITELLM_PRICING_URL, cached_at: new Date().toISOString(), kept_models: kept },
    ...slim,
  };
  await fsp.writeFile(cachePath, JSON.stringify(payload) + "\n");
```

注意与 seed 的 `_meta` 字段名不同：磁盘缓存是 `cached_at`，seed 是 `generated_at`（`scripts/build-pricing-seed.cjs:55`）。读取时两者都被 `delete data._meta` 剥掉（`litellm-fetcher.js:132`、`:162`；`index.js:48`）。

**四步降级链**（`litellm-fetcher.js:107-164`，注释 `:107-111` 就是权威描述）：

1. **新鲜磁盘缓存**（`mtime < ttl`）→ `source: "disk-cache"`（`:128-137`）。
2. **抓上游并写缓存** → `source: "upstream"`（`:139-146`）。
3. **陈旧磁盘缓存**（网络失败时）→ `source: "stale-cache"`（`:148-158`）。
4. **内置 seed 快照** → `source: "seed-snapshot"`（`:160-163`）。

`state.source` 会被记录下来（`src/lib/pricing/index.js:83`），初始值是 `"seed-snapshot:sync"`（`:63`、`:103`）。这四步都有测试：`test/pricing.test.js:508-582`（fresh cache 不触发 fetch / stale 触发 fetch / 无 cache 失败回落 seed / 有 stale cache 优先 stale 而非 seed）。

**注意不存在的东西**：没有重试、没有 ETag / If-None-Match、没有超时后的二次尝试。一次失败直接进下一步。

### 2.5 运行期加载：从哪读、是否落 `~/.tokentracker/cache/`、能否被覆盖

**默认路径**（`src/lib/pricing/index.js:69-71`）：

```js
function defaultCachePath() {
  return path.join(os.homedir(), ".tokentracker", "cache", "pricing.json");
}
```

**实际传入路径**来自 `resolveTrackerPaths()`，结果相同（`src/lib/tracker-paths.js:4-12`）：`rootDir = ~/.tokentracker`、`cacheDir = <rootDir>/cache`，`home` 只接显式传参 `{ home = os.homedir() }`。即：**是，落 `~/.tokentracker/cache/pricing.json`**。

**加载是「一次性、幂等、可并发共享」的**（`src/lib/pricing/index.js:73-94`）：

```js
async function ensurePricingLoaded(opts = {}) {
  if (state.loaded) return state;
  if (state.loadingPromise) return state.loadingPromise;
  state.loadingPromise = (async () => {
    try {
      const cachePath = opts.cachePath || defaultCachePath();
      const { data, source } = await loadLitellmData({ ...opts, cachePath });
      state.litellmRawMap = data || {};
      state.litellmPerMillionMap = buildLitellmPerMillionMap(state.litellmRawMap);
      state.source = source;
      state.loaded = true;
      state.revision += 1;
      state.negativeCache.clear();
      return state;
    } finally {
      state.loadingPromise = null;
    }
  })();
  return state.loadingPromise;
}
```

同一段代码同时说明三件事：并发调用共享一次 fetch（`test/pricing.test.js:1287-1308` 断言 `fetchCalls === 1`）、成功后 `revision += 1`、`negativeCache` 在每次重载后被清空（`index.js:86`，字段声明在 `:64-66`）。

**可覆盖的接口只有编程接口，没有环境变量。**
`ensurePricingLoaded(opts)` 会把 `opts` 透传给 `loadLitellmData`（`index.js:80`），而 `loadLitellmData` 接受 `{ cachePath, ttlMs, fetchTimeoutMs, fetchImpl, url, logger }`（`litellm-fetcher.js:112-119`）。**生产代码**对 `ensurePricingLoaded` 的调用只有两处：`src/commands/serve.js:127`（传 `cachePath`）与 `src/lib/local-api.js:3340`（不传参，走默认路径）。**【R2 补口径（I3）】"全仓 27 命中"的拆分如下**——正则 `/ensurePricingLoaded\(/` 命中 **27 行** = 生产调用 **2** + 函数定义 **1**（`src/lib/pricing/index.js:73`）+ 注释 **4**（`index.js:4`/`:41`/`:42`、`src/commands/serve.js:123`）+ 测试调用 **20**（`test/pricing.test.js` 16 处，即 `:586`/`:602`/`:629`/`:755`/`:784`/`:819`/`:1049`/`:1063`/`:1083`/`:1097`/`:1114`/`:1167`/`:1297`/`:1298`/`:1299`/`:1306`；`test/local-api-queue-cache.test.js:149`/`:220`；`test/grok-parser.test.js:301`；`test/omo-parser.test.js:229`）。若用**裸标识符**口径（含 import/export 与测试名）则共 **36 行**，多出的 9 行是 `serve.js:9`、`local-api.js:61`/`:3531`、`index.js:97`/`:366`、`grok-parser.test.js:13`、`omo-parser.test.js:15`、`pricing.test.js:583`/`:1287`。因此下句"其余 20 行全部在测试里"是**测试调用行**口径，不等于"27 减 2"。

**`TOKENTRACKER_*` 定价相关变量盘点（全仓库 grep 结果）**

结论：**一个都没有。** 对 `src/` 全目录搜索 `TOKENTRACKER_[A-Z_]+` 得到 250+ 命中，逐条核对后与定价相关的为 **0**：

- 与定价最接近的只有 `TOKENTRACKER_OPENCODE_GO_LOCAL_ESTIMATE`（`src/lib/opencode-go-limits.js:20`、`:51`），但它控制的是 **opencode-go 额度**的本地估算口径，不参与 `computeRowCost`。
- 其它命中全部是路径覆盖（`TOKENTRACKER_*_HOME`）、调试开关（`TOKENTRACKER_DEBUG`）、网络与同步（`TOKENTRACKER_INSFORGE_*`、`TOKENTRACKER_DEVICE_TOKEN`）、遥测（`TOKENTRACKER_NO_TELEMETRY`）之类。
- `~/.tokentracker` 根目录本身也没有环境变量覆盖：`resolveTrackerPaths` 只接 `{ home }`（`src/lib/tracker-paths.js:4`），而 `serve.js:126` 调用时不传 `home`。

**配置文件覆盖：不存在。** 仓库里没有 `pricing.toml` 之类的外置价格表；`~/.tokentracker/` 下**与价格有关**的派生物只有 `cache/pricing.json`（运行期磁盘缓存）。**【R2 限定（I7）】限定词"与价格有关"是必需的**：该数据目录并非"只有价格缓存"——限额链另有多份磁盘缓存常量（`src/lib/usage-limits.js:70` `usage-limits-cache.json`、`:118` `claude-usage-limits-cache.json`、`:128` `codex-usage-limits-cache.json`、`:134` `opencode-go-usage-limits-cache.json`、`:141` `claude-usage-rate-limit.json`），本轮逐字核到其中两份的路径拼接为 `<home>/.tokentracker/tracker/<文件名>`（`usage-limits.js:2219`、`:2299`）；它们与定价无关，故不影响本段结论（其余几份同形态常量未逐个核其拼接点，列为未核项 U6，见文末修正记录）。想改价格只能改 `curated-overrides.json`（需要重新发布或就地改源码）——这一点与 `curated-overrides.json:3` 的「Edit this file to override pricing without redeploying」是一致的（改的是仓库文件，不是用户配置）。

---

## 3. 模型名匹配算法（`matcher.js`）

### 3.1 解析顺序（源码注释即权威规格）

`src/lib/pricing/matcher.js:3-12` 原文：

```
// Resolve order:
//   0. CURATED source exact match (source-specific pricing)
//   1. CURATED exact match (self-defined aliases like kiro-*, hy3-*)
//   2. LiteLLM exact match (mainstream claude/gpt-5/gemini)
//   3. CURATED alias (e.g. "auto" -> "composer-1")
//   4. CURATED fuzzy substring (e.g. "kiro-future-xyz" matches via "kiro")
//   5. LiteLLM suffix-strip (gpt-5-codex-high-fast -> gpt-5-codex)
//   5b. LiteLLM provider-prefix strip (mimo-v2.5-pro -> openrouter/xiaomi/mimo-v2.5-pro)
//   6. LiteLLM reverse substring (longest-key first)
//   7. null  (caller decides what to do — typically zero-pricing + negative cache)
```

对应实现行号：

| 步骤 | 行号 | 返回的 `source` 标签 |
|---|---|---|
| 空值守卫 | `matcher.js:253-255` | `"empty"` |
| 早期 source 特例（WorkBuddy `auto` / Cline `:free` / AStudio `auto`） | `:265-279` | `"miss"` / `"curated:cline-free-suffix"` |
| 0 CURATED source exact | `:280-286` | `"curated:source-exact"` |
| 1 CURATED exact（含 Qwen 特例 + dot 形态） | `:291-300` | `"curated:exact"` / `"curated:exact-dot"` |
| 2 LiteLLM exact（含 dot 形态 + curated dot 包含匹配） | `:302-313` | `"litellm:exact"` / `"litellm:exact-dot"` / `"curated:exact-dot"` |
| 3 CURATED alias | `:315-322` | `"curated:alias"` |
| 4 CURATED fuzzy | `:331-343` | `"curated:fuzzy"` |
| 5 LiteLLM 后缀剥离 | `:345-351` | `"litellm:strip"` |
| 5b LiteLLM provider 前缀剥离 | `:361-370` | `"litellm:prefix-strip"` |
| 6 LiteLLM 反向子串（长键优先） | `:372-383` | `"litellm:fuzzy"` |
| 7 未命中 | `:385` | `"miss"` |

### 3.2 归一化规则

**(a) 大小写**：两处。主路径靠 `lookupExactCaseInsensitive`（`matcher.js:226-234`）：先试 `table[model]`，否则把 key 逐个 `toLowerCase()` 线性比对。子串路径靠先 `toLowerCase()` 再比（`matcher.js:259`、`:376-379`）。**没有预建的小写索引**，未命中时是 O(表大小) 的线性扫描——这是 `negativeCache` 存在的理由（`index.js:64-66`、`test/pricing.test.js:1094-1109`）。

**(b) `-` / `.` / `_` 互换**：**没有全局互换**。只有一条通用规则 `buildDotRestoredModel`（`matcher.js:219-224`）：把 `lower.replace(/(\d+)-(\d+)/g, "$1.$2")`，即**只把「数字-数字」还原成「数字.数字」**（`glm-5-1-0` → `glm-5.1.0`），非数字段不动，`_` 完全不处理；还原结果与原文相同则返回空串（表示「无 dot 形态可试」）。注释解释了动机（`matcher.js:324-330`：Droid 输出 `glm-5-1-0`，curated 用 `glm-5.1`）。测试：`test/pricing.test.js:191-205`（还原成功）、`:313-329`（`does NOT corrupt models without digit-digit pairs`，明确列举 `claude-3-7-sonnet` / `gpt-5-codex` / `gemini-2-5-pro` 不受影响）。

**(c) 日期后缀**：没有通用的「去日期」步骤。能命中是因为两个方向：LiteLLM 表里**本身就同时存在**带日期与不带日期的键（例如 seed 里 `vertex_ai/claude-opus-4-6` 与 `vertex_ai/claude-opus-4-6@default` 并存），以及 step 6 的反向子串（`model 包含 key`，`matcher.js:379`）。唯一针对日期的正则特例是 Qwen（`matcher.js:291`）：

```js
  if (/(?:^|\/)qwen3[.-]8-flash(?:-\d{4}-\d{2}-\d{2})?$/.test(lower.trim()) && curated.exact?.["qwen3.8-flash"]) {
    return { hit: true, source: "curated:exact", value: curated.exact["qwen3.8-flash"] };
  }
```

该正则被 `test/edge-pricing-parity.test.js:87-107` 用 7 个变体（含 `qwen3.8-flash-2026-09-01`）同时验证本地与 5 个云端文件。

**(d) provider 前缀（`anthropic/` 等）**：两条独立机制。

第一条是 **Claude 专用归一化**，先于所有查表执行（`matcher.js:89-95`）：

```js
function normalizeClaudeModel(model) {
  if (!model || typeof model !== "string") return model;
  // Drop the provider path prefix relays prepend ("anthropic/...",
  // "openrouter/anthropic/...") so only the bare Claude id reaches the cleanup
  // and lookups below. Without this the "/" is flattened to "-" by the regex
  // step, breaking both the exact and provider-prefix-strip matches.
  const base = model.includes("/") ? model.slice(model.lastIndexOf("/") + 1) : model;
```

它还会修复「版本号在 tier 前面」的网关写法（`matcher.js:116-118`）：

```js
  if (/^claude-(?:[4-9]|\d{2,})[.-]\d+-(?:sonnet|opus|haiku)/.test(m)) {
    return m.replace(/^claude-(\d+)[.-](\d+)-(sonnet|opus|haiku)/, "claude-$3-$1-$2");
  }
```

即 `claude-4.6-opus` → `claude-opus-4-6`，而 3.x 的版本优先写法（`claude-3-5-sonnet`）**必须保持不动**（`matcher.js:112-115` 注释明确）。

第二条是 **通用 5b 前缀剥离**（`matcher.js:361-370`）：

```js
  if (litellm) {
    const suffix = "/" + lower;
    let best = null;
    for (const key of Object.keys(litellm)) {
      if (key.length > suffix.length && key.toLowerCase().endsWith(suffix)) {
        if (best === null || key < best) best = key;
      }
    }
    if (best) return { hit: true, source: "litellm:prefix-strip", value: litellm[best] };
  }
```

**歧义处理是确定性的**：多个 provider 暴露同名模型时取**字典序最小**的键，与 JSON 顺序无关（`:357-360` 注释 + `test/pricing.test.js:246-265`）。

**(e) `:free`**：只有 `cline` source 走这个语义（`matcher.js:268-274`）：

```js
  if (sourceKey === "cline" && lower.endsWith(":free")) {
    return {
      hit: true,
      source: "curated:cline-free-suffix",
      value: { input: 0, output: 0, cache_read: 0, cache_write: 0 },
    };
  }
```

注意它**只对 `cline` 生效**，且必须在 curated 查表之前（`:262-267` 的注释：`Source-specific prices apply only to their source, preventing collisions with public prices for same-named models from other CLIs.`）。Cline 的网关命名空间（`cline-free/`、`cline-pass/`）则由 `fuzzy` 的前两条接管（`curated-overrides.json` 的 `fuzzy[0]` / `fuzzy[1]`），云端版本改成内联判断（`tokentracker-leaderboard-refresh.ts:432`）。

**(f) `-latest`**：**没有特殊处理**。`xai/grok-4-latest`、`xai/grok-3-mini-fast-latest` 这类键在 LiteLLM 表里真实存在（seed 原文可见），靠 step 1/2 精确命中；`curated.exact` 里也有 `grok-4-latest` 一条（与 `grok-4` 同价）。带 `-latest` 的模型名若不在表里，只会退到 step 6 反向子串。

**(g) 推理强度后缀剥离**（`matcher.js:14-31`）：

```js
const SUFFIX_STRIP_PATTERNS = [
  /-xhigh-fast$/,
  /-high-fast$/,
  /-medium-fast$/,
  /-low-fast$/,
  /-xhigh$/,
  /-high$/,
  /-medium$/,
  /-low$/,
  /-fast$/,
];
```

只用于 step 5，且**只在剥离后的键在 LiteLLM 表里存在时**才接受（`matcher.js:347-350`）。顺序重要：`-xhigh-fast` 必须排在 `-fast` 前，否则 `gpt-5-codex-high-fast` 会先被削成 `gpt-5-codex-high`。测试：`test/pricing.test.js:330-340`。

### 3.3 per-source 归一化表（`matcher.js:194-204`）

```js
const SOURCE_MODEL_NORMALIZERS = {
  acode: normalizeIFlytekMaasModel,
  antigravity: normalizeAntigravityModel,
  claude: normalizeClaudeModel,
  cursor: normalizeCursorModel,
  "pi-anthropic": normalizeClaudeModel,
  "prime-agent-anthropic": normalizeClaudeModel,
  zed: normalizeZedModel,
  unsloth: normalizeUnslothModel,
  workbuddy: normalizeWorkbuddyModel,
};
```

**归一化只作用于计价查找，原始模型名照样存储与展示**（`matcher.js:191-193` 注释原文：`Per-source model-name normalizers, applied at pricing-lookup time only (the raw model name is preserved for storage/display).`）。云端也有同样的约束，被测试守死：`test/edge-pricing-parity.test.js:223-233` 断言所有 edge 文件都不得包含 `getModelDisplayName` / `IFLYTEK_MAAS_MODEL_NAMES`。

各归一化器的可验证要点：

- `normalizeAntigravityModel`（`matcher.js:33-58`）：剥括号、剥 thinking/effort 词、把 Gemini 3.x 系列**特意降级映射到 2.5 档**（`:49-51`，例如 `gemini-3.x-pro` → `gemini-2.5-pro`），`gpt-oss-120b*` → `antigravity-gpt-oss-120b`（`:55`）。「别名只对 antigravity 生效」被 `test/pricing.test.js:364-398` 与 `:1060-1078` 双重锁定（同一行 `gpt-oss-120b`，`source: "openrouter"` 得 0，`source: "antigravity"` 得 0.0125）。
- `normalizeCursorModel`（`matcher.js:126-150`）：按空格/符号切分后**只剔除 `thinking|xhigh|high|medium|low`**，`fast` 保留（`:146` 注释：`fast` 可能是独立 SKU）。Grok 4.5 有专门 SKU 分流（`:138-140`）。
- `normalizeZedModel`（`matcher.js:68-81`）：**不剥 `fast`**（因为 `grok-code-fast-1` 里 `fast` 是名字的一部分，`:64-67` 注释），并把 Claude 的点号 minor 连字符化（`claude-opus-4.8` → `claude-opus-4-8`）。
- `normalizeWorkbuddyModel`（`matcher.js:161-166`）：`auto` → `hy3-preview-agent`，且**必须与 Cursor 的 `auto`（→ `composer-1`）区分**。`matcher.js:265-267` 另有一道守卫：source 为 `workbuddy-ai` 且模型是 `auto` 时直接返回 `miss`。测试：`test/pricing.test.js:1320-1347`、`test/edge-pricing-parity.test.js:374-385`。
- `normalizeUnslothModel`（`matcher.js:182-189`）：`local/` 或 `unpriced/` 前缀 → 哨兵 `__tokentracker_unpriced_unsloth_model__`，防止反向子串把本地模型误配到云端 SKU。

### 3.4 结果对象与「未命中」的行为

`lookupPricing` 的返回值恒为三字段对象：

- `{ hit: true, source: "<标签>", value: {input,output,cache_read,cache_write} }`
- `{ hit: false, source: "empty", value: null }`（空模型名，`matcher.js:253-255`）
- `{ hit: false, source: "miss", value: null }`（`:385`）

未命中在上层的两级后果：

1. `getModelPricing`（`index.js:112-132`）：

```js
  const result = lookupPricing(model, {
    curated: curatedOverrides,
    litellm: state.litellmPerMillionMap,
    source: lookupSource,
  });
  if (result.hit) return result.value;

  state.negativeCache.add(cacheKey);
  return ZERO_PRICING;
```

`ZERO_PRICING` 是共享常量 `{ input: 0, output: 0, cache_read: 0, cache_write: 0 }`（`index.js:17`）。负缓存键是 `` `${lookupSource}\0${model}` ``（`:120`）——**按 source 分域**，因为同一模型名在不同 source 下可能一个命中一个未命中。

2. `getModelPricingInfo`（`index.js:157-176`）把三态显式化：

```js
  return {
    status: !result.hit ? "unpriced" : [rates.input, rates.output, rates.cache_read, rates.cache_write]
      .some((value) => Number(value) > 0) ? "priced" : "free",
    source: result.hit ? result.source : null,
    ...
```

即 `unpriced`（表里没有）/ `free`（表里有且四价全 0）/ `priced`（至少一个价 > 0）。这正是 §0 提到的「$0 的两种含义」在 API 层的区分方式，前端据此显示不同文案（`dashboard/src/pages/DashboardPage.jsx:1253-1259`、`dashboard/src/ui/dashboard/components/CostAnalysisModal.jsx:173-183`）。

`getModelPricingInfo` 另有两点：结果按 `pricing revision` 记忆化，并在每次返回时做浅拷贝（`index.js:137-155`，注释 `:134-136`：`Copies go out so a caller that mutates its object cannot poison the cache.`）；本地推理与 Pi 订阅源直接返回 `{status:"free", source:"local_inference"|"subscription"}`（`:158-160`）。

---

## 4. 分段定价 / 峰谷 / 特殊档位：逐项判定

### 4.1 按上下文长度分段的「溢价」：存在，但形态特殊

**不存在**通用的「>200K 输入整体换价目表」机制。存在的是**OpenAI 长上下文溢价**，且只对 5 类模型生效（§1.6）。适用范围之外的所有模型，无论输入多长，都按同一个单价算。

判定依据（`src/lib/pricing/index.js:267-269` 的白名单 + `:300-306` 的公式 + `:308` 的叠加）。触发条件是「单请求原始（含缓存）输入 > 272_000」（`src/lib/codex-rollout-parser.js:729-733`），并且**只对观测到的请求子集加价**，不做整桶外推（`src/lib/pricing/index.js:286-289` 注释、`test/gpt-6-astra-pricing.test.js:50-62`）。

因此：**没有**「>200k token 溢价」这样的通用阈值；阈值是 272K，且只属于 OpenAI 白名单模型。

### 4.2 批处理折扣：不存在

对 `src/` 全目录搜索 `batch`，命中 37 处，**全部**是上传/同步的批处理（`src/lib/upload-throttle.js:5`、`src/commands/sync.js:4315` 等）或注释里的「batch」字样，无一与定价相关。`computeRowCost` 没有任何 `batch` 维度；价格表里也没有 `batch_*` 字段（seed 的字段集合恰为 4 个，见 §2.2）。

**判定：不存在批处理折扣。**

### 4.3 峰谷价：存在，仅 DeepSeek 一家，且是 5 折

模型白名单（`src/lib/pricing/index.js:34-39`）：

```js
const DEEPSEEK_TIME_PRICED_MODELS = [
  "deepseek-v4-flash",
  "deepseek-v4.1-flash",
  "deepseek-flash",
  "deepseek-v4-pro",
];
```

峰段窗口（`src/lib/pricing/index.js:199-207`）：

```js
function isDeepSeekOffPeak(row) {
  if (row?.pricing_tier === "off_peak") return true;
  if (row?.pricing_tier === "peak") return false;
  const timestamp = Date.parse(String(row?.hour_start || ""));
  if (!Number.isFinite(timestamp)) return false;
  if (isBeijingWeekendOffPeak(timestamp)) return true;
  const hour = new Date(timestamp).getUTCHours();
  return !((hour >= 1 && hour < 4) || (hour >= 6 && hour < 10));
}
```

优先级：**显式 `pricing_tier` > 北京周末规则 > UTC 峰段判定 > 默认峰值**（时间戳非法时返回 `false`＝按峰值算，fail closed）。

北京周末规则（`src/lib/pricing/index.js:186-197`）：

```js
// From 00:00 Beijing time on 2026-08-23 — this instant — DeepSeek bills whole
// Beijing weekends off-peak, peak hours included.
// https://api-docs.deepseek.com/quick_start/pricing/
const DEEPSEEK_WEEKEND_OFF_PEAK_FROM_MS = Date.UTC(2026, 7, 22, 16, 0, 0);

function isBeijingWeekendOffPeak(timestamp) {
  if (timestamp < DEEPSEEK_WEEKEND_OFF_PEAK_FROM_MS) return false;
  const beijingDay = new Date(timestamp + 8 * 60 * 60 * 1000).getUTCDay();
  return beijingDay === 0 || beijingDay === 6;
}
```

生效起点 `Date.UTC(2026, 7, 22, 16, 0, 0)` = 2026-08-22T16:00Z = 北京时间 2026-08-23 00:00。折算是「+08:00 固定偏移」（`:188-192` 注释解释为何不能直接对原始瞬时取 `getUTCDay()`）。

价格表里存的是**峰值价**，折扣由代码施加（`curated-overrides.json:5` 的 `deepseek_v4_pricing_note`：`Exact entries store peak rates. computeRowCost applies the time-of-use discount from each row's UTC hour_start.`）。

折扣幅度：四个价格字段各 ×0.5（`src/lib/pricing/index.js:215-221`）。

测试覆盖非常完整：`test/pricing.test.js:891-983`（10 个时间点断言，含「非法时间戳 fail closed 到峰值」「显式 tier 覆盖周末规则」「legacy id 保持静态价」）与 `:985-1016`（展示型模型名不触发折扣）、`:1349+`（v4.1 Flash）。

### 4.4 `cache_creation` 5m / 1h 的差异：不存在

价格表里只有一个 `cache_write` 字段（`matcher.js:410-412`；`curated-overrides.json` 的所有条目也只有单一 `cache_write`）。**没有任何字段、参数或分支区分 5 分钟与 1 小时缓存写入。** 判定：**不存在**（Anthropic 官方对 1h 缓存有更高价，TokenTracker 未建模）。

### 4.5 图片 / 音频等非文本计费：不存在

队列行的 token 桶固定为五列（`src/lib/pricing/index.js:257-262` 读的四列 + 条件性的 reasoning）。对 `src/` 搜索 `audio_token` / `image_token` / `image_generation`，**0 命中**。

LiteLLM 表里确实有音频/图像模型条目（例如 seed 中可见 `gemini-2.5-flash-native-audio-latest`、`gpt-4o-mini-tts-2025-03-20`、`chatgpt-image-latest`），但它们只是被当作普通「输入/输出 token 单价」收录；`convertLitellmEntry` 只取 4 个成本字段（`matcher.js:398-414`），LiteLLM 的 `input_cost_per_image` 之类字段**在 seed 生成阶段就被丢弃**（`scripts/build-pricing-seed.cjs:14-19` 的 `KEEP_FIELDS` 白名单）。

判定：**不存在独立的图片/音频计费通道。** 若非文本模态的用量被记进了 input/output token，会按文本单价计费；如果被记成「张/秒」，则完全无处安放（`total_tokens` 不参与计价，见 §1.5）。

### 4.6 其它特殊档位汇总

| 档位 | 是否存在 | 证据 |
|---|---|---|
| OpenAI 长上下文（>272K） | 存在，2× input/cache_read、1.5× output，仅 5 类模型 | `index.js:267-306` |
| Astra/Sol Fast（priority） | 存在，2× 乘子，仅 3 条 curated 条目 | `index.js:273-275`、`:336-354` |
| DeepSeek 峰谷 | 存在，5 折，4 个模型 | `index.js:34-39`、`:199-222` |
| DeepSeek 北京周末全天谷价 | 存在（2026-08-23 起） | `index.js:186-197` |
| 批处理折扣 | 不存在 | §4.2 |
| cache 5m/1h 差异 | 不存在 | §4.4 |
| 图片/音频/视频计费 | 不存在 | §4.5 |
| 订阅制 → 0 元 | 存在（lmstudio / Pi Copilot / Cline 网关） | `index.js:18-27`、`:228-232`；`curated-overrides.json:43-49` |
| 服务端上报价优先 | 存在（grok / cline） | `index.js:32`、`:236-241` |

---

## 5. 缓存价语义

### 5.1 两个字段、两条价格

| token 列 | 价格字段 | 语义 | 行号 |
|---|---|---|---|
| `cached_input_tokens` | `pricing.cache_read` | 缓存**读** | `src/lib/pricing/index.js:260` |
| `cache_creation_input_tokens` | `pricing.cache_write` | 缓存**写** | `src/lib/pricing/index.js:261` |

在价格表里的命名映射是 `cache_read_input_token_cost → cache_read`、`cache_creation_input_token_cost → cache_write`（`src/lib/pricing/matcher.js:407-412`）。curated 表直接用内部名（`cache_read` / `cache_write`）。

### 5.2 缺失时的兜底：**没有兜底，直接按 0**

确切算式（`src/lib/pricing/index.js:260-261`）：

```
cached_input_tokens        × (pricing.cache_read  || 0)
cache_creation_input_tokens × (pricing.cache_write || 0)
```

`pricing.cache_read === undefined` 时 `undefined || 0 === 0`。**不存在「用 input 价 ×0.1」之类的推导**——全仓库没有任何形如 `cache_read ?? input * k` / `cache_write || input` 的算式；本文逐行读过 `computeRowCost` 全 129 行（`index.js:227-355`）与 `getRowPricing` 全 14 行（`:209-222`），没有价格字段之间的派生关系。

真正存在「用 input 价」的地方是**人手写进 JSON 的取值约定**，不是代码逻辑。原文见 `curated-overrides.json:6`（`iflytek_maas_pricing_note`）：

> Models without a dedicated cache-hit price use the normal input rate; cache writes also use the normal input rate. iFlytek MaaS prices are source-scoped and do not inherit DeepSeek time-of-use discounts.

可以逐条验证这个约定被执行了：`source_exact.acode` 全部 64 条**都带完整的 4 个字段**（实测：缺 `cache_write` 的条目数 = 0），且 `xopkimik27code` 的 `cache_read = cache_write = input = 0.9`、`xsparkx2` 四价全为 0.42。

### 5.3 数据缺口有多大（实测计数）

`curated-overrides.json` 的 `exact`（84 条）：缺 `cache_write` **41 条**（几乎全部是 GLM 系、Kimi 系、Composer 系、Grok 系与 `*-free` 条目）；缺 `cache_read` **0 条**；缺 `input` / `output` **0 条**。

`seed-snapshot.json`（2284 条）：有 `input_cost_per_token` 2274 条、`output_cost_per_token` 2273 条、`cache_read_input_token_cost` **566 条**、`cache_creation_input_token_cost` **188 条**。

也就是说 **LiteLLM 侧约 75% 的模型没有缓存读价、约 92% 没有缓存写价**。直接推论：走 LiteLLM 路径的模型，只要 `cached_input_tokens` 非零而该模型没有 `cache_read_input_token_cost`，这部分缓存读取就会被算成 **$0**。这是 §5.2 的算式与上表计数的直接结论。`curated-overrides.json:3` 把「cache_write fields LiteLLM often omits」列为该文件存在的第二个理由，与此吻合。

### 5.4 这条语义曾被当成 bug 修过

`test/edge-pricing-parity.test.js:87` 的测试名直说：

```
test("Qwen3.8 Flash local and all cloud paths share full cache pricing (#715)", () => {
```

断言（`:99-101`）：

```js
      const cost = computeRowCost({ model, input_tokens: 1e6, output_tokens: 1e6,
        cached_input_tokens: 1e6, cache_creation_input_tokens: 1e6 });
      assert.ok(Math.abs(cost - 0.836) < 1e-12, `${model}: cache cost must not be omitted`);
```

验算（价格 `{input:0.15, output:0.47, cache_read:0.016, cache_write:0.2}`，来自 `test/edge-pricing-parity.test.js:89`，也即 `curated-overrides.json` 的 `qwen3.8-flash`）：`0.15 + 0.47 + 0.016 + 0.2 = 0.836` ✓。四个桶各 100 万 token、价格单位是 MTok，所以直接相加。同一文件还断言「不同 SKU 不得共用这张价」（`:103-105`：`qwen3.8-flash-next`、`qwen3.8-max`、`qwen3.7-flash`）。

另一个相关回归：`test/cache-token-accounting.test.js:9-13` 的注释（`Any normalizer or diff that drops the cache_creation_input_tokens field silently under-reports cache writes — the bug we just fixed for OpenCode / OpenClaw / Gemini.`），以及 `:45-65` 对 `diffGeminiTotals` 保留 `cache_creation_input_tokens` 增量的断言。**缓存写的丢失可以发生在计价层，也可以发生在解析/差分层**，两处都有测试。

---

## 6. 聚合层的金额汇总

### 6.1 汇总发生在后端，且是「逐行计价后相加」，不是「先加 token 再计价」

这是全链路最重要的一条聚合不变量。原文（`src/lib/local-api.js:2634-2640`）：

```js
        // Price each row, exactly as aggregateByDay does for the hero (#653).
        // computeRowCost is not linear for models with a Fast tier or
        // long-context rates: priority_* / long_context_* mark a subset of the
        // base columns rather than adding volume, so they survive only while the
        // row is intact. Summing the base columns first and pricing once silently
        // dropped every premium — same tokens, fewer dollars than the hero.
        ma.totals.total_cost_usd = Number(ma.totals.total_cost_usd || 0) + computeRowCost(row);
```

`computeRowCost` 对 `priority_*` / `long_context_*` 是**非线性**的（缩放到 `Math.min` / `bounded`），所以「先按 (source, model, day) 汇总 token 再计价一次」会丢溢价。这条被 `test/model-breakdown.test.js:764-791` 锁死（标签就是 `#653`），注释写得极清楚：`the per-model row must carry the same dollars the hero counted for these rows`。

### 6.2 各聚合点的位置

| 用途 | 位置 | 形态 |
|---|---|---|
| 每日（hero / 趋势 / 汇总的底座） | `src/lib/local-api.js:260-301`，累加在 `:285` | `a.total_cost_usd += computeRowCost(row)` |
| `usage-summary` 端点 | `src/lib/local-api.js:2358-2415`，累加在 `:2369` | 对 daily 的 `total_cost_usd` 求和，`toFixed(6)` 输出（`:2415`） |
| 热度图 `usage-heatmap` | `src/lib/local-api.js:2574-2590` | 同上，`:2577` 累加 |
| 模型分解 `usage-model-breakdown` | `src/lib/local-api.js:2596-2661`，逐行计价 `:2640` | 每模型 `toFixed(6)`（`:2648`），source 汇总 = 其模型之和（`:2651-2652`） |
| 技能用量（skill_usage） | `src/lib/local-api.js:3378-3382` | `cost += computeRowCost({ ...tokens, model, source: "claude" })` |
| 会话（sessions） | `src/lib/session-analytics.js:269-308` | `repriceSessionRecord`，逐 bucket / 逐 model_usage 行计价（`:272`、`:279`、`:302`） |
| 会话聚合 | `src/lib/session-analytics.js:2414`、`:2695` | 对已算好的 `cost_usd` 求和 |
| quality-per-dollar | `src/lib/outcomes-engine.js:207` | 逐行计价后按模型 / 工具分组（`:209-210`） |
| 云端排行榜快照 | `dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:1181` | `agg.estimated_cost_usd += computeRowCost(row)`，输出时取整到分（`:1349`） |

### 6.3 前端是否存在二次计算

**不存在重新计价**，前端只做「把后端给的金额加起来」这一件事：

- `dashboard/src/lib/model-breakdown.ts:116-117`：`target.total_cost_usd = String(Number(target.total_cost_usd || 0) + Number(add?.total_cost_usd || 0))`。
- `dashboard/src/lib/model-breakdown.ts:179`、`:217`：`toFiniteNumber(entry?.totals?.total_cost_usd)` / 优先用显式模型成本。
- `dashboard/src/lib/sessions-insights.js:19-30`：对 bucket 的 `cost_usd` 求和。
- 展示层：`dashboard/src/pages/DashboardPage.jsx:1256-1257`、`dashboard/src/ui/dashboard/components/CostAnalysisModal.jsx:183`。

全文搜索 `dashboard/src/` 下 `MODEL_PRICING` / `computeRowCost` / `getModelPricing`：**0 命中**。也就是说在**生产路径与 `dashboard/src/` 范围内**，价格表只存在于后端与 edge 两处，前端没有任何价格副本（R1 复核已独立复算：这四类标识符在 `dashboard/src/` 下确为 0 命中；金额只在后端逐行计价后相加——`src/lib/local-api.js:285`、`:2640`、`:3381`，`src/lib/session-analytics.js:272`、`:279`、`:302`，`src/lib/outcomes-engine.js:207`，edge `:1181`；前端只做求和与格式化）。唯一的例外是 dev/mock 通道，见 §7.4 注与 §11.2 第 7 条。

唯一例外是 mock 数据（`dashboard/src/lib/mock-data.ts:391` 的 `formatUsdFromTokens(totals.total_tokens)`），只用于离线演示，不参与真实路径。

### 6.4 两个防「混版本」的机制

1. **时区缓存键带上 pricing revision**（`src/lib/local-api.js:115-117`）：

```js
function timeZoneContextKey({ timeZone, offsetMinutes } = {}) {
  return `${timeZone || ""}|${Number.isFinite(offsetMinutes) ? offsetMinutes : ""}|p${getPricingRevision()}`;
}
```

价格表重载（`revision += 1`，`src/lib/pricing/index.js:85`）会使所有缓存的日聚合失效。

2. **长循环中途重载价格则整个请求重做**（`src/lib/local-api.js:2601`、`:2642`）：

```js
      const pricingRevision = getPricingRevision();
      ...
      if (pricingRevision !== getPricingRevision()) return handleLocalApi(req, res, url);
```

注释（`src/lib/local-api.js:244-245`）说明动机：`Pricing can refresh while a large aggregation yields. Rebuild consistently with the current catalog instead of publishing a mix of two revisions.`

### 6.5 金额的输出精度与类型

同一份数据在不同端点精度不同，跨端点对账必须按各自口径比较：日聚合内部是 `number`（`src/lib/local-api.js:273` 初始化为 `0`）；模型分解的 per-model 与 source 汇总是 `toFixed(6)` 字符串（`:2648`、`:2652`）；`usage-summary` 的 `totals.total_cost_usd` 同样 `toFixed(6)`（`:2415`）；会话侧是浮点原值求和（`src/lib/session-analytics.js:289`）；云端排行榜快照取整到**分**（`Math.round(agg.estimated_cost_usd * 100) / 100`，`tokentracker-leaderboard-refresh.ts:1349`）。

---

## 7. edge-patches 的五份文件

### 7.1 是哪五份，谁是权威

`test/edge-pricing-parity.test.js:25-31`：

```js
const CANONICAL = "tokentracker-leaderboard-refresh.ts";
const MIRRORS = [
  "tokentracker-account-daily.ts",
  "tokentracker-account-summary.ts",
  "tokentracker-account-model-breakdown.ts",
  "tokentracker-leaderboard-profile.ts",
];
```

路径前缀 `dashboard/edge-patches/`（`test/edge-pricing-parity.test.js:23`）。注意 `dashboard/edge-patches/` 下共 28 个 `.ts` 文件（R1 复核 `Get-ChildItem -File` 实测 28），**除这 5 份外另有 23 个**（`tokentracker-ingest.ts`、`tokentracker-billing.ts`、`tokentracker-account-hourly.ts`、`tokentracker-account-monthly.ts` …），**其中只有这 5 份内嵌价格表**。`tokentracker-account-hourly.ts` / `tokentracker-account-monthly.ts` 出现在 `MODEL_API_CALL_SITES`（`test/edge-pricing-parity.test.js:43-44`）只是因为它们要「原样透传模型 ID」，不是因为它们有价格表。

### 7.2 为什么必须「逐字复制」

`test/edge-pricing-parity.test.js:3-12` 的注释是整个设计的自述，也是唯一的动机记录：

```
// The cloud edge functions each embed a hand-maintained copy of the model
// pricing table (MODEL_PRICING + getModelPricing fuzzy chain). These copies
// MUST be byte-identical across all five files — drift silently prices the
// same row differently per endpoint (real incident: leaderboard-profile
// missed kimi-k2.6 + the mimo-v2.5 family until 2026-06, so the profile
// modal showed $0 / -60% cost for those models while the leaderboard list
// priced them correctly). This test fails loudly on any divergence.
//
// To change pricing: edit tokentracker-leaderboard-refresh.ts (canonical),
// then copy the identical block into the other four files.
```

结构性原因：这 5 个 edge 函数是**独立部署单元**，没有共享模块导入价格表的能力（或至少设计上没有这样做），且它们不读 LiteLLM、不读 seed、不读 `curated-overrides.json`——它们各自持有一份**手抄表**。

实测块大小与哈希（对 `const MODEL_PRICING ... function getModelPricing(...) {...}` 整块取 SHA-256，前 16 位）：

| 文件 | 块行数 | `sha256[:16]` | 块内 `MODEL_PRICING` 键数 |
|---|---|---|---|
| `tokentracker-leaderboard-refresh.ts` | 394 | `37840dc53f7b6e2f` | 119 |
| `tokentracker-account-daily.ts` | 394 | `37840dc53f7b6e2f` | 119 |
| `tokentracker-account-summary.ts` | 394 | `37840dc53f7b6e2f` | 119 |
| `tokentracker-account-model-breakdown.ts` | 394 | `37840dc53f7b6e2f` | 119 |
| `tokentracker-leaderboard-profile.ts` | 394 | `37840dc53f7b6e2f` | 119 |

键数口径（R1 复核重算，花括号配平逐键清点）：`MODEL_PRICING` 对象字面量为 `tokentracker-leaderboard-refresh.ts:142`（声明）～`:332`（表结束），**119 键**；同一抽取块内另有独立常量 `IFLYTEK_MAAS_MODEL_PRICING`（`:339`～`:404`）**64 键**。两者相加 183——此前版本把 183 记作 `MODEL_PRICING` 的键数，属口径混记，现已更正为「块内定价键总数 183 = 119 + 64」。

**当前修订下五份完全一致**（哈希相同），说明约定在被遵守。

`getRowPricing` 是**第二个**同样被要求逐字一致的手抄块（`test/edge-pricing-parity.test.js:403-413`），注释解释原因：`Substring checks alone let one file's window drift while every assertion still passes.`

### 7.3 校验方式

三层，全部在 `test/edge-pricing-parity.test.js`：

**第一层：整块字节相等**（`:76-85`）：

```js
test("MODEL_PRICING + getModelPricing are byte-identical across all 5 edge files", () => {
  const canonical = extractBlock(CANONICAL);
  for (const name of MIRRORS) {
    assert.strictEqual(
      extractBlock(name),
      canonical,
      `${name} pricing block drifted from ${CANONICAL} — copy the canonical block over verbatim`,
    );
  }
});
```

抽取用正则（`:55-56`）：

```js
const BLOCK_RE =
  /const MODEL_PRICING[\s\S]*?\nfunction getModelPricing\(model: string(?:, source = "")?\) \{[\s\S]*?\n\}/;
```

**第二层：抽样行为等价**（把 TS 块用 `esbuild.transformSync` 转成 CJS，塞进 `vm.runInNewContext` 执行，与本地 `src/lib/pricing` 的同一批断言对照）。覆盖面：

- Qwen3.8 Flash 全量缓存价（`:87-107`，7 个模型名变体 × 5 个文件，并显式断言 `cost === 0.836`）。
- 权威表回归条目与 matcher 顺序（`:109-161`）：10 个「翻车过的键」必须存在（`kimi-k2.6`、`mimo-v2.5-pro`、`mimo-v2.5`、`mimo-v2-flash`、`cursor-grok-4.5`、`cursor-grok-4.5-fast`、`glm-5.3`、`glm-5.3-flash`、`deepseek-v4.1-flash`、`deepseek-flash`），7 组顺序断言（`gpt-5.4-pro` 先于 `gpt-5.4`；`gpt-5.1-codex-mini` 先于 `gpt-5.1`；`gemini-3`+`pro` 先于单独 `gemini-3`；`kimi-k2.6` 先于 `kimi`；`mimo-v2.5-pro` 先于 `mimo-v2.5`；`grok-4.5`+`fast` 先于 `grok-4.5` 先于 `grok-4`；`glm-5.3-flash` < `glm-5.3` < `glm-5`），并断言 `gpt-5.4-medium` **不得**作为 exact 条目存在（`:128-133`，注释：`a stale exact entry used to bill it at 60% of the real gpt-5.4 rate`）。
- iFlytek MaaS 表与 `curated-overrides.json` 的 `source_exact.acode` **深度相等**（`:163-191`，`assert.deepEqual` 整表）。
- 未定路由保持未定价（`:193-221`）、推理不双计（`:235-248`）、Pi 订阅为 0（`:250-269`）、Unsloth 哨兵（`:271-290`）、上报成本白名单（`:292-308`）、Command Code 逐模型等价（`:310-358`，8 个模型 × 3 种 `total_cost_usd`）、Cline 网关为 0（`:360-372`）、WorkBuddy `auto` 未定价（`:374-385`）、DeepSeek 峰谷档（`:387-419`）。

**第三层：专项配对测试**（不在 parity 文件里，但守同一批文件）：

- `test/gpt-6-astra-pricing.test.js:64-87`：5 个文件 × 7 个模型名变体，价格必须等于 `{input:10, output:50, cache_read:1, cache_write:12.5}`，且 `computeRowCost` 必须等于 73.5。
- `test/gpt-6-sol-pricing.test.js` / `test/gpt-6-1-sol-pricing.test.js`（各 90 行）：同构。
- `test/hy4-preview-pricing.test.js:32-51`：5 个文件 × 4 个变体，价格必须等于 `{input:0.833, output:2.5, cache_read:0.042, cache_write:0.833}`。
- `test/model-breakdown.test.js:465-478`：以字符串包含方式钉住 4 个条目 + 3 个 matcher 片段。
- `test/model-breakdown.test.js:480-503`：以「用本地值重建云端行文本再 `includes`」的方式钉住 Kiro 两条目，注释：`so byte-drift between the two tables will fail this test`。

### 7.4 两套价格不同步的风险：**存在，且是当前修订下的实态**

层面一：**edge 的表与 `curated.exact` 不是同一张表**。实测两向差集（本次研究用 Node 逐键比对）：

- 在 `curated.exact` 但**不在** edge `MODEL_PRICING` 的 9 个键：
  `claude-opus-5.5`、`claude-sonnet-5.5`、`cline-gateway-models`、`claude-opus-4-8`、`claude-haiku-4-5`、`antigravity-gpt-oss-120b`、`grok-4.5-build`、`grok-build-free`、`grok-4.5-build-free`。
- 在 edge `MODEL_PRICING` 但不在 `curated.exact` 的键（R1 复核量化：**44 个**）：全部主流条目（`claude-opus-4-6`、`gpt-5.4`、`gpt-5.5`、`gemini-2.5-pro`、`gemini-3-pro-preview`、`gpt-5-codex`、`mimo-v2.5*` ……）。**iFlytek MaaS 的 64 条不计入此数**——它们在云端被拆到独立的 `IFLYTEK_MAAS_MODEL_PRICING` 常量里（`tokentracker-leaderboard-refresh.ts:339`），不属于 `MODEL_PRICING`。

那 9 个缺口里，多数被云端用「更粗的匹配器」意外兜住：

| 缺失键 | 本地 (curated) | 云端 edge 的实际走向 | 是否等价 |
|---|---|---|---|
| `claude-opus-5.5` / `claude-sonnet-5.5` | 4/20/0.2/5、2/10/0.1/2.5 | 正则 `/opus-5[.-]5(?:$\|[^0-9.])/` 等（edge `:436-437`）显式覆盖 | 等价 |
| `claude-haiku-4-5` | 1/5/0.1/1.25 | `lower.includes("haiku")` → `claude-haiku-4-5-20251001`（edge `:442`） | 等价 |
| `claude-opus-4-8` | 5/25/0.5/6.25 | `lower.includes("opus")` → `claude-opus-4-6`（edge `:441`），同为 5/25/0.5/6.25 | 数值巧合下等价 |
| `cline-gateway-models` | 0/0/0/0 | 内联 `cline-free/`、`cline-pass/` 判断（edge `:432`） | 等价（实现方式不同） |
| `grok-4.5-build` | 2/6/0.5/0 | `lower.includes("grok-4.5")` → `cursor-grok-4.5`（edge `:480`），同为 2/6/0.5/0 | 数值巧合下等价 |
| **`grok-build-free`** | **0/0/0/0** | `lower.includes("grok-build")` → `grok-build` = `{input:1.25, output:2.5, cache_read:0.2}`（edge `:273`、`:481`） | **不等价** |
| **`grok-4.5-build-free`** | **0/0/0/0** | `lower.includes("grok-4.5")` → `cursor-grok-4.5` = 2/6/0.5 | **不等价** |
| `antigravity-gpt-oss-120b` | 2.5/10/0 | edge `getModelPricing` 无 antigravity 归一化；`gpt-oss-120b` 不含任何匹配片段 → `ZERO_PRICING`（edge `:534`） | 不等价（但 antigravity 是否走云端未知） |

**`grok-build-free` 的具体差距（源码原文对照，非估算）：**

- 本地：`curated-overrides.json` 的 `exact["grok-build-free"] = {"input":0,"output":0,"cache_read":0,"cache_write":0}`；`test/grok-parser.test.js:302` 断言 `getModelPricing("grok-build-free", { source: "grok" }).input === 0`，`:342-343` 断言队列行的模型名就是 `grok-build-free` 且 `computeRowCost(rows[0]) === 0`。
- 云端：`tokentracker-leaderboard-refresh.ts:273` 的 `"grok-build": { input: 1.25, output: 2.50, cache_read: 0.20 }` + `:481` 的 `if (lower.includes("grok-build")) return MODEL_PRICING["grok-build"];`。
- 前提条件：只有当该行 `total_cost_usd <= 0` 或缺省时差异才显现——`grok` 在 `SOURCES_WITH_AUTHORITATIVE_COST` 里（本地 `src/lib/pricing/index.js:32` / 云端 `tokentracker-leaderboard-refresh.ts:8`），上报价为正时两端都直接采用上报价。上面引用的 `test/grok-parser.test.js:343` 恰好是 `total_cost_usd` 未触发短路的一行。
- 量化：100 万 input token → 本地 $0，云端 $1.25。

**parity 测试为何没抓到它**：`test/edge-pricing-parity.test.js:76-85` 只比 edge↔edge。**【R2 修正（I4）】与 `curated-overrides.json` 的对照只有 4 处，且全在同文件内**——`:170-172`（iFlytek MaaS 全表与 `curatedPricing.source_exact.acode` deepEqual）、`:175`（`curatedPricing.source_alias?.acode === undefined`）、`:208`/`:209`（`curatedPricing.source_exact.acode.xsparkx2` / `.xopglm53` 取值 deepEqual）。原写的"**6 处**"是**跨文件混合口径**，其中两处**并非** curated 对照：`test/model-breakdown.test.js:480-503`（"Kiro entries are byte-equivalent"——比的是本地 `local-api.MODEL_PRICING` ↔ leaderboard-refresh edge 补丁的逐字节等价，`require` 里没有 curated）、`test/grok-parser.test.js:302`/`:342-343`（只断言**本地侧**取值，不与 edge 或 curated 对照）；而 `:87-107` 的 Qwen 用例比的是**硬编码字面量** `{input:0.15, output:0.47, cache_read:0.016, cache_write:0.2}`（`:89`）与 edge 实现，同样不涉及 curated。**没有任何测试断言「edge 表 ⊇ curated.exact」或「逐键价格相等」。** 这是漂移的实际入口。（R1 复核已独立读完全文件 420 行确认：第一层 `:76-85` 确实只做 edge↔edge 字节比对；差集恰 9 键，逐字一致。）

层面二：**本地与云端的长上下文/Fast 溢价能力不对等**。edge 的 `computeRowCost`（`tokentracker-leaderboard-refresh.ts:569-613`）只有纯四桶 + 条件性推理折叠，**没有** `long_context_*` 与 `priority_*` 分支。价格表注释对此有书面说明：`Cloud aggregates use Standard short-context estimates because they do not retain per-request context or service tier.`（`curated-overrides.json:98`）。所以同一份 GPT-6 Astra 用量，本地（有子集列）与云端（无子集列）必然出不同金额——这是**已知的、被文档化的差异**，不是 bug。

层面三：**成员集合差异**。`PI_SUBSCRIPTION_SOURCES` 本地有 4 个成员（`src/lib/pricing/index.js:18-23`），edge 只处理 2 个（`tokentracker-leaderboard-refresh.ts:575` 的 `pi-github-copilot` / `pi-copilot`；parity 测试 `:250-269` 也只断言这两个字符串存在于所有 5 份文件里）。`prime-agent-github-copilot` / `prime-agent-copilot` 这两行在云端**不会**被短路为 0，会落到模型定价。

---

## 8. 成本的其他用途

### 8.1 `wrapped-aggregator.js`：**不涉及金额**

`src/lib/wrapped-aggregator.js`（236 行）全文只处理 token 与计数：`tokensBySource` / `tokensByModel` / `tokensByDay` / `tokensByHourOfDay`（`:116-119`），返回值 `totals` 只有 `tokens / conversations / sources / models / active_days`（`:212-218`）。文件内没有 `cost` 字样，也没有 `require("./pricing")`（唯一的 require 是 `./local-api` 的 `normalizeQueueRow`，`：21-23`）。

**结论：`tracker wrapped` 与 `/wrapped` 页面不含任何金额。**

### 8.2 `quality-per-dollar`：消费成本，但不重新定义

文档 `docs/quality-per-dollar.md`（67 行）第 18 行原文：

> Costs come entirely from TokenTracker's existing pricing. The sidecar only supplies the *accepted/total* counts.

实现 `src/lib/outcomes-engine.js`：

- `:24` `const { computeRowCost } = require("./pricing");`
- `:207` `const cost = computeRowCost(row) || 0;`——逐行计价，与主链路同源。
- `:154` `const quality_per_dollar = cost_usd > 0 && outcomes > 0 ? accepted / cost_usd : null;`
- `:156-157` `effective_tokens = total_tokens × acceptance_rate`、`effective_cost_usd = cost_usd × acceptance_rate`。

注意 `:208` 用的是 `row.total_tokens`（`const tokens = Number((row && row.total_tokens) || 0);`）——**token 数用 `total_tokens`，钱用 `computeRowCost`**，两者在同一函数里各走各的。这是 CLAUDE.md「绝不用 total_tokens」那条规则的适用边界示例：它约束的是**成本**，不是 token 计数。

### 8.3 订阅节省估算：**不存在**

- `src/lib/subscriptions.js`（508 行）：全文件没有任何 `cost` / `price` / 金额比较。它的 grep 命中的 `token` 全部是 OAuth 的 `access_token` / `id_token` / `refresh_token`（`:59-60`、`:130-131`、`:453`、`:472-473`、`:489`），`price` 0 命中。
- `src/lib/subscription-manager.js`（293 行）：管理订阅的 provider 配置，不含金额。
- 全仓库搜索 `savings` / `subscription_savings` / `monthly_cost` / `plan_price`：命中的 `savings` 只有两类——`dashboard/src/pages/CloudPage.jsx:95`（**Cloud 年付相对月付的折扣百分比**，`cloudAnnualSavings(monthly, annual)`）与无关文案。**没有任何「订阅费 vs token 成本」的对比计算。**

**判定：不存在订阅节省估算。**

### 8.4 `usage-limits.js`：**不使用成本**

`src/lib/usage-limits.js` 是 4500+ 行的大文件，但成本相关命中为 0：`computeRowCost` 0 命中、`pricing`（作为价格）0 命中。它工作的量纲是**百分比与剩余额度**（`:165` `used_percent: pct`、`:348-352`、`:1484-1486` 从进度条文本里正则抠 `█+\s*(\d+)%`、`:1910-1911` `usedPercent = 100 - percentRemaining`）。文件里唯一的 `cost` 字样出现在注释（`:693`、`:4105`），说明本地 `opencode.db` 的 cost 只作为显式开关下的估算来源，且该路径属于 `opencode-go-limits.js`（`src/lib/opencode-go-limits.js:20`、`:51` 的 `TOKENTRACKER_OPENCODE_GO_LOCAL_ESTIMATE`）。

**判定：额度限额与成本换算无关，它读的是 provider 上报的百分比。**

### 8.5 云端排行榜的金额

`tokentracker-leaderboard-refresh.ts:1181` 逐行计价进入 `agg.estimated_cost_usd`；`:1349` 以 `Math.round(... * 100) / 100` 写入快照。前端展示带免责声明（`dashboard/src/pages/LeaderboardPage.jsx:817` 的 `title="Based on estimated API pricing, not actual billing"`，另见 `:927`、`:978`）。`tokentracker-account-model-breakdown.ts:906` 用 `ma.totalCostUsd += subscriptionBacked ? 0 : ...`，确保订阅行不进金额。

---

## 9. 测试佐证：锁定的不变量与已知陷阱

### 9.1 计价核心不变量（`test/pricing.test.js`，1392 行）

按 matcher 步骤逐条对应：

| 不变量 | 测试行号 |
|---|---|
| CURATED 优先于 LiteLLM；无条目时回落 LiteLLM；alias 字面映射（含 Cursor `auto`） | `:102-121`、`:123-133`、`:266-279` |
| fuzzy 子串（`kiro-future-xyz` → `kiro-cli-agent`） | `:135-145` |
| 版本档不落到更粗的基档（GPT-5.6 codex 不落 `gpt-5`；Kimi K3 不落 `kimi-k2.5`；GLM-5.3/-Flash 各用自己价） | `:146-190`、`:206-235` |
| 数字-数字 → 数字.数字 还原（Droid GLM）及不破坏无 digit-digit 的模型 | `:191-205`、`:291-329` |
| provider 前缀剥离 + 确定性（最小键胜、与 JSON 顺序无关）+ LiteLLM exact 优先 | `:236-290` |
| 推理强度后缀剥离；反向子串取最长键；完全未知模型 `hit: false` | `:330-363` |
| 各 per-source 归一化的 source 作用域 | `:364-486` |
| `convertLitellmEntry` 消除浮点漂移（`1e-7 × 1e6 → 0.1`）；`buildLitellmPerMillionMap` 跳过 `_meta` 与无价条目 | `:487-507` |
| fetcher 四步降级 | `:508-582` |
| `negativeCache` 按 source 分域 + 短路 | `:1080-1109` |

### 9.2 成本语义不变量（下表行号均属 `test/pricing.test.js`）

| 不变量 | 测试行号 | 断言值 |
|---|---|---|
| Claude 5.5 SKU 官方价 + Claude thinking 已在 output 内 | `:18-34` | opus-5-5/5.5 = 29.2；sonnet-5 = 14.7；sonnet-5-5 = 14.6；`output=5e5+reasoning=5e5` 与 `output=1e6+reasoning=0` 同价 |
| Codex 不双计推理；非 Codex 计推理 | `:1111-1134`、`:1164-1183` | `0.125 + 0.2375 + 0.15`；`withReasoning > withoutReasoning` |
| 权威来源上报价优先（Grok / Cline）；非权威来源忽略 | `:1185-1197`、`:1199-1216`、`:1230-1246` | `0.130486`、`0.25`、与非上报时相等 |
| Cline `:free` = 0；Pi Copilot = 0 但 token 保留；`pi-anthropic` 正常计价 | `:1218-1228`、`:1264-1278` | — |
| Command Code 用表价、忽略 CLI 显示估算 | `:1248-1262` | 3 模型 × 3 种 `total_cost_usd`（含 999）结果恒定 |
| 空模型名 → ZERO；`ensurePricingLoaded` 幂等、只 fetch 一次、revision 只加一次 | `:1280-1285`、`:1287-1308` | — |

Astra / Sol 专项（`test/gpt-6-astra-pricing.test.js`）：

- `:44-48`：Codex 折叠推理 73.5；`gemini` 98.5（差 25）。
- `:50-62`：长上下文溢价只对子集，混合桶 6.475；100 万 token 的聚合行仍是 73.5（`large aggregate is not a long request`）。
- `:115-122`：Fast = 2×Standard（1.55 → 3.1）；`service_tier` 标量与 `-fast` 后缀**都不生效**。
- `:124-136`：混合桶按比例（半 Fast = 2.325）；子集超报被夹紧（`10_000_000` → 仍是 3.1）。
- `:138-160`：长 × Fast 是**相乘**而非相加（5.7，且显式断言 `notEqual(1.55*2 + (2.85-1.55))`）；无交集列时回落（2.85 + 1.55）；`Math.min` 夹紧。
- `:162-193`：`priority_*` 不参与任何 token 求和（`total_tokens` 恒等式在新列存在时依然成立）；无 `priority_multiplier` 的模型（`gpt-5.6-sol`、`claude-sonnet-4-6`）带 `priority_*` 列也不加价。
- `:242-382`（5 条）：解析器把 `token_count` 归到最近一次 `thread_settings_applied`；无 tier 记录的行保持 Standard 且不带 priority 列；功能上线前写入的队列行价格不变；恢复续读保持已应用的 tier；会话侧车记录长上下文 × Fast 的交集。

DeepSeek 峰谷（`test/pricing.test.js:891-983`）：

- 峰值 01:00-04:00Z、06:00-10:00Z（`2.214` vs `1.107`，`6.644` vs `3.322`）。
- 非法时间戳 fail closed 到峰值（`:931-935`）。
- 北京周末规则边界（`:937-968`）：2026-08-22T06:00Z 仍峰值；2026-08-23T01:00Z 转谷；2026-08-23T09:59Z 仍在周末内；2026-08-24T01:00Z 回峰；2026-08-29T06:00Z 谷。
- 显式 tier 覆盖周末规则（`:969-976`）。
- legacy id 保持静态价（`:978-982`）。

### 9.3 审计脚本 `audit:tokens`：**只审计 token，不审计金额**

`package.json` 的 `"audit:tokens": "node scripts/audit-token-correctness.cjs"`。脚本 176 行，全文没有任何 `pricing` / `cost` / `computeRowCost`。它做的是**总量恒等式审计**（`scripts/audit-token-correctness.cjs:52-157`）：

- 按 `(source, model, hour_start)` 去重取最新行（`:54-60`、`:26-27`）。
- 计算 `input`（对 legacy codex 行减去 cached，`:42-50`、`:94`）。
- `baseParts = input + output + cacheRead + cacheWrite`（`:95`）。
- 推理是否计入 `sumParts` 取决于 source（`:34-40`、`:96-99`）：

```js
function isReasoningInformational(source) {
  const normalized = String(source || "").trim().toLowerCase();
  // Codex and OmO both fold reasoning into output_tokens, so their
  // reasoning_output_tokens column is informational and must not be summed
  // into the total-token invariant.
  return normalized === "codex" || normalized === "omo";
}
```

- `diff = total - sumParts`，非零即 `invariant_failures`，并留样（`:100`、`:117-129`）。

**注意这个名单与 `computeRowCost` 的折叠名单不一致**：审计只把 `codex` / `omo` 当作信息性，而计价折叠的是 `codex` / `acode` / `every-code` / `omo` / `cline` 五个（`src/lib/pricing/index.js:248-253`）。两侧各有自己的理由（审计的是 token 恒等式，计价的是金额），但**同一份数据在「token 是否重复」这件事上有两套口径**。这是一个真实存在的、无测试守护的不一致点，列入 §11。

对应用例：`test/token-audit.test.js`（114 行）4 条测试，名称分别是 `auditRows reports source totals, duplicate buckets, and invariant failures`（`:6`）、`auditRows treats Codex reasoning as informational for total-token invariants`（`:54`）、`auditRows normalizes legacy Codex input that already includes cache reads`（`:75`）、`auditRows accepts Codex totals where reasoning is not folded into output`（`:96`）。**四条全部与金额无关。**

### 9.4 已知陷阱（从测试注释里摘出的「翻车史」）

| 陷阱 | 出处 |
|---|---|
| `gpt-5.4-medium` 不是 SKU，曾作为 exact 条目把价格算成真价的 60% | `test/edge-pricing-parity.test.js:128-133` |
| GLM-5.3 Flash 误匹配 `glm-5.3` 会贵 6.7 倍；`grok-4-1-fast` 落到 `grok-4` 会高 15–30 倍 | `test/edge-pricing-parity.test.js:156-160`；`tokentracker-leaderboard-refresh.ts:483-488` |
| `leaderboard-profile` 曾漏掉 `kimi-k2.6` 与 `mimo-v2.5` 家族，profile 弹窗显示 $0 / -60% | `test/edge-pricing-parity.test.js:5-9` |
| 先汇总 token 再计价会静默丢掉 Fast / 长上下文溢价 | `src/lib/local-api.js:2634-2639`、`test/model-breakdown.test.js:764-791` |
| 汇总对象上不存在 `total_cost_usd` / `usage_precision`，导致 Grok 上报价在列表里显示 $0.00 | `test/model-breakdown.test.js:835-888` |
| 归一化/差分丢掉 `cache_creation_input_tokens` 会静默低报缓存写 | `test/cache-token-accounting.test.js:9-13` |
| Codex/every-code 的 `input` 本身含 cached，直接复制会虚高 6–7 倍 | `CLAUDE.md:165` |
| 未记录 service tier 的请求不得猜测为 Fast；DeepSeek 展示型名称不得继承分时折扣 | `src/lib/codex-service-tier.js:12-15`；`test/pricing.test.js:985-1016` |

---

## 10. 实际数值抽查与手工演算

### 10.1 三个真实模型的完整价格字段原文

**模型一：`claude-opus-4-6`（走 LiteLLM 路径）**

`src/lib/pricing/seed-snapshot.json` 原文条目：

```json
"claude-opus-4-6":{"input_cost_per_token":0.000005,"output_cost_per_token":0.000025,"cache_read_input_token_cost":5e-7,"cache_creation_input_token_cost":0.00000625}
```

`curated-overrides.json` 的 `exact` 中**没有**该键（实测缺席）。云端手抄表里有：`tokentracker-leaderboard-refresh.ts` 的 `"claude-opus-4-6": { input: 5, output: 25, cache_read: 0.5, cache_write: 6.25 }`。

换算后的内部单位（USD/MTok，`×1e6`）：`input = 5`、`output = 25`、`cache_read = 0.5`、`cache_write = 6.25`。

命中的解析步骤：`matcher.js:303` 的 `if (litellm && litellm[lookupModel])` → `source = "litellm:exact"`。

**模型二：`gpt-5.4`（LiteLLM 有 input/output/cache_read，**没有** cache_write）**

`src/lib/pricing/seed-snapshot.json` 原文条目：

```json
"gpt-5.4":{"input_cost_per_token":0.0000025,"output_cost_per_token":0.000015,"cache_read_input_token_cost":2.5e-7}
```

换算后：`input = 2.5`、`output = 15`、`cache_read = 0.25`、`cache_write = undefined`（→ 计 0）。云端手抄表同值：`"gpt-5.4": { input: 2.5, output: 15, cache_read: 0.25 }`。

**模型三：`gemini-3-pro-preview`（LiteLLM 有 input/output/cache_read，没有 cache_write）**

`src/lib/pricing/seed-snapshot.json` 原文条目：

```json
"gemini-3-pro-preview":{"input_cost_per_token":0.000002,"output_cost_per_token":0.000012,"cache_read_input_token_cost":2e-7}
```

换算后：`input = 2`、`output = 12`、`cache_read = 0.2`、`cache_write = undefined`。云端同值：`"gemini-3-pro-preview": { input: 2, output: 12, cache_read: 0.2 }`。

### 10.2 示例请求的手工演算

统一用一组 token：`input_tokens = 10000`、`cached_input_tokens = 200000`、`output_tokens = 5000`、`cache_creation_input_tokens = 1500`。

算式骨架（`src/lib/pricing/index.js:257-264`）：

```
cost = ( input×p.input + output×p.output + cached×p.cache_read
         + cache_creation×p.cache_write + reasoning×p.output_or_0 ) / 1_000_000
```

**演算 A：`claude-opus-4-6`，`source = "claude"`，`reasoning_output_tokens = 0`**

- `reasoningIncludedInOutput`：`source === "claude"` 不在五个折叠 source 中（`index.js:248-253`）→ `false`。
- `reasoningCost = 0 × 25 = 0`。
- `usesOpenAILongContextTier`：`"claude-opus-4-6"` 不满足 `index.js:268-269` 任一条件 → 在第 `:275` 行 `return baseCost`（不再进入 Fast/长上下文分支）。
- 逐项：

```
input          10000 × 5     =  50 000
output          5000 × 25    = 125 000
cache_read    200000 × 0.5   = 100 000
cache_write     1500 × 6.25  =   9 375
reasoning          0         =       0
                       合计    = 284 375
cost = 284375 / 1_000_000    = 0.284375 USD
```

**演算 B：`gpt-5.4`，`source = "codex"`，`reasoning_output_tokens = 0`**

- `reasoningIncludedInOutput`：`source === "codex"` → `true`（`index.js:249`）→ `reasoningCost = 0`。
- `pricing.cache_write` 缺失 → `(undefined || 0) = 0`。
- `usesOpenAILongContextTier`：`"gpt-5.4"` 不在白名单（`index.js:268-269`）→ `:275` 提前返回。
- 逐项：

```
input          10000 × 2.5   =  25 000
output          5000 × 15    =  75 000
cache_read    200000 × 0.25  =  50 000
cache_write     1500 × 0     =       0   ← LiteLLM 无 cache_creation 价
reasoning          0         =       0
                       合计    = 150 000
cost = 150000 / 1_000_000    = 0.150000 USD
```

**同一行在云端得到完全相同的 0.15**：`tokentracker-leaderboard-refresh.ts:455` 命中 `gpt-5.4` 表项（值同为 2.5/15/0.25），`:605-612` 的算式与本地一致。

**演算 C：`gemini-3-pro-preview`，`source = "gemini"`，先取 `reasoning_output_tokens = 0`，再取 `= 800`**

- `reasoningIncludedInOutput`：`"gemini"` 不在折叠集合 → `false`（`index.js:248-253`）。
- `pricing.cache_write` 缺失 → 0。
- 基准（`reasoning = 0`）：

```
input          10000 × 2     =  20 000
output          5000 × 12    =  60 000
cache_read    200000 × 0.2   =  40 000
cache_write     1500 × 0     =       0
reasoning          0         =       0
                       合计    = 120 000
cost = 120000 / 1_000_000    = 0.120000 USD
```

- 加 800 个推理 token 后（`index.js:256`：`800 × 12 = 9 600`）：`合计 = 129 600` → `cost = 0.129600 USD`。

**演算 D（对照用）：演算 B 的同一行改成 `source = "gemini"`**。推理折叠不再生效，但 `reasoning_output_tokens = 0` 时结果仍是 `0.15`——说明「折叠与否」只在推理 token 非零时才产生金额差异。若 `reasoning_output_tokens = 800` 且该 provider 的 `output_tokens` 已含这 800，则本地会多收 `800 × 15 / 1e6 = 0.012`，即 `0.162`。这正是 `test/gpt-6-astra-pricing.test.js:44-48` 用 73.5 vs 98.5 锁定的那类差异。

### 10.3 从测试原文直接引用的三个锚点（非本次演算，用于交叉核对）

- Qwen3.8 Flash 四桶各 100 万：`0.836`（`test/edge-pricing-parity.test.js:89`、`:99-101`）。手工核对：`0.15 + 0.47 + 0.016 + 0.2 = 0.836` ✓
- GPT-6 Astra 四桶各 100 万 + 推理 50 万（Codex）：`73.5`（`test/gpt-6-astra-pricing.test.js:45`）。手工核对：`10 + 50 + 1 + 12.5 + 0 = 73.5` ✓（推理折叠，不加项）
- Command Code 小请求：`input=100, output=20, cached=600, cache_creation=50`，`claude-sonnet-4-6` → `0.0009675`（`test/pricing.test.js:1249`、`test/edge-pricing-parity.test.js:334`）。手工核对：`100×3 + 20×15 + 600×0.3 + 50×3.75 = 300 + 300 + 180 + 187.5 = 967.5`；`967.5 / 1e6 = 0.0009675` ✓。其中 `claude-sonnet-4-6` 的 seed 原文为 `{"input_cost_per_token":0.000003,"output_cost_per_token":0.000015,"cache_read_input_token_cost":3e-7,"cache_creation_input_token_cost":0.00000375}`，换算即 `3 / 15 / 0.3 / 3.75`

---

> **§10 复核结论（R1）**：以下四段手工演算与三个锚点，已由复核方用 `seed-snapshot.json` / `curated-overrides.json` 原文独立复算，逐值一致：演算 A `0.284375`、B `0.150000`、C `0.129600`、D `0.162`；锚点 `0.836` / `73.5` / `0.0009675`。三锚点所用条目（`claude-opus-4-6`、`gpt-5.4`、`gemini-3-pro-preview`）在 `curated.exact` 中确实缺席（seed 命中），`claude-sonnet-4-6` 的 seed 原值与 edge `:443` 同值。

## 11. 存疑与待复核点

以下条目按「证据强度」从高到低排列。凡未能从源码或测试原文确认的，一律标 `【未能确认】`。

### 11.1 已由源码坐实、但没有测试守护的不一致

1. **`OPENAI_LONG_CONTEXT_INPUT_THRESHOLD` 双份定义**。`src/lib/pricing/index.js:31` 定义并 `:375` 导出，但**在本文件内从未被读取**；真正生效的是 `src/lib/codex-rollout-parser.js:51` 的另一份同名常量（用于 `:733`）。两处目前都是 `272_000`，改一处不改另一处不会有测试失败。`src/lib/session-analytics.js:245` 的注释还把这个常量当作权威（`in src/lib/pricing/index.js`），实际上它只被注释引用。**【R1 复核状态：已验证】**（`:31` 定义 / `:375` 导出 / `index.js` 全程零读取 / `codex-rollout-parser.js:51` → `:733` 生效）。

2. **推理折叠名单两套**。计价侧 5 个（`codex`/`acode`/`every-code`/`omo`/`cline`，`src/lib/pricing/index.js:248-253`）；token 审计侧 2 个（`codex`/`omo`，`scripts/audit-token-correctness.cjs:34-40`）。两者口径不同，注释各自成理，但没有任何测试把两者绑在一起。**【R1 复核状态：已验证】**（两套集合逐条核对属实，确无测试绑定）。

3. **云端与本地对 9 个 `curated.exact` 键的覆盖不同**（§7.4）。其中 `grok-build-free`（100 万 input token：本地 $0 vs 云端按 `grok-build` 计 $1.25）与 `grok-4.5-build-free`（本地 $0 vs 云端 $2）是**价格层面的实际分歧**，只在 `total_cost_usd` 未上报时才显现。parity 测试不含「edge ⊇ curated」这类断言，抓不到。**这是本次研究中最值得上报的一条。** **【R1 复核状态：已验证，行号采信】**——`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 字节比对；9 键差集逐字重算一致；云端走向 `tokentracker-leaderboard-refresh.ts:481`（`grok-build-free` → `grok-build` 1.25/2.5/0.2）与 `:480`（`grok-4.5-build-free` → `cursor-grok-4.5` 2/6/0.5）逐行确认。

4. **`PI_SUBSCRIPTION_SOURCES` 本地 4 个成员、云端 2 个成员**（`src/lib/pricing/index.js:18-23` vs `tokentracker-leaderboard-refresh.ts:575`）。`prime-agent-*` 两条在云端不会短路为零。**【R1 复核状态：已验证】**

5. **云端 `computeRowCost` 没有长上下文 / Fast 溢价**（`tokentracker-leaderboard-refresh.ts:569-613` 无 `long_context_*` / `priority_*` 分支）。价格表注释把这一点写成有意设计（`curated-overrides.json:98`），但没有测试断言「云端结果必然 ≤ 本地结果」或给出量化允许差。**【R1 复核状态：已验证】**（edge `computeRowCost` `:569-613` 确无 `long_context_*` / `priority_*` 分支）。

6. **金额精度在三处不同**：`toFixed(6)`（`src/lib/local-api.js:2415`、`:2648`）、浮点原值（`src/lib/session-analytics.js:289`）、取整到分（`tokentracker-leaderboard-refresh.ts:1349`）。跨端对账需按端点选择比较口径。**【R1 复核状态：已验证】**

### 11.2 需要外部事实核对、本次未联网确认的点

7. **LiteLLM 上游价的时效性**：`seed-snapshot.json` 的 `_meta.generated_at` 是 `2026-05-26T11:12:29.761Z`（`seed-snapshot.json` 原文），而运行期磁盘缓存的 TTL 是 24 小时（`litellm-fetcher.js:12`）。也就是说：**离线/首启状态下使用的价格表年龄可能远大于 24h**。上游 LiteLLM 仓库当前是否仍以 `model_prices_and_context_window.json` 为唯一价目文件、字段名是否仍为那 4 个成本字段【未能确认】——本次未联网核对上游仓库。

8. **`curated-overrides.json` 注释里引用的第三方定价页**（`platform.claude.com`、`developers.openai.com`、`api-docs.deepseek.com`、`maas.xfyun.cn` 等）与表内数字是否一致【未能确认】——这些是项目自己在 note 里声明的核价来源与日期，本次研究未逐条复核外部页面。

9. **edge 五个文件的部署状态**：本仓库的 `dashboard/edge-patches/` 是「待部署的补丁源」，实际线上函数是否为同一修订【未能确认】。`CLAUDE.md:42` 提到「Deploy the touched edge functions after editing」，说明部署是人工步骤。

### 11.3 明确的否定结论（研究过程中排除的可能性）

10. **不存在** `models.dev` 或 OpenRouter 价源（`src/` 全目录 0 命中；价格加载链只有 seed + LiteLLM + curated 三层）。
11. **不存在**任何定价相关的 `TOKENTRACKER_*` 环境变量（全量盘点后为 0）。
12. **不存在**用户可编辑的外置价格文件（无 `pricing.toml` 之类；`~/.tokentracker/` 下与价格相关的只有 `cache/pricing.json` 这个派生物）。
13. **不存在**批处理折扣、cache 5m/1h 差异、图片/音频独立计费（§4.2、§4.4、§4.5）。
14. **不存在**订阅节省估算（§8.3）。
15. **前端不重复计价**（`dashboard/src/` 下 `MODEL_PRICING` / `computeRowCost` / `getModelPricing` 均 0 命中）。

### 11.4 方法学留痕（便于复核）

行号一律以 `read` 工具的 LF 编号为准（PowerShell `Get-Content` 在本机对该仓库个别文件少算 4–6 行，已用 Node 逐字节验证：`local-api.js` LF=3543、`session-analytics.js` LF=2934、`tokentracker-leaderboard-refresh.ts` LF=1449）。JSON 结构统计（键数、字段直方图、差集、块哈希）由 Node `JSON.parse` + `crypto.createHash` 只读完成；五份 edge 文件的抽取正则与 `test/edge-pricing-parity.test.js:55-56` 完全一致。`git rev-parse HEAD` = `b19d27b0ccd59ce621f4fa17207b06358e67987c`，`git describe --tags --always` = `v1.3.14`。

---

## 修正记录（R1 复核后）

依据：`REVIEW-R1-F-pricing-consumers.md`（样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`）。下列每条都在本轮**独立重算**后落实，未修改样本仓。行号/键数一律用 `[System.IO.File]::ReadAllLines(...,UTF8)` 或 Node 只读探针重取。

| 编号 | 原表述 | 修正后 | 依据（重算方式 / 样本仓位置） |
|---|---|---|---|
| R1-1 [E1] | §7.2 表头「`MODEL_PRICING` 键数」= **183** | **119**；并注明「块内定价键总数 183 = 119 + 64」 | 花括号配平逐键清点：`dashboard/edge-patches/tokentracker-leaderboard-refresh.ts:142-332` = 119 键、`:339-404`（`IFLYTEK_MAAS_MODEL_PRICING`）= 64 键 |
| R1-2 [E2] | `index.js:284-285` | `index.js:283-284` | `src/lib/pricing/index.js:283` = `// annotations, never addends — total_tokens stays the sum of the base columns` |
| R1-3 [E3] | `index.js:288-295`（引 `They are annotations, never addends`） | `index.js:282-283`（整段注解 `:281-285`） | `:282` 行末 `They are`、`:283` 起 `annotations, never addends…`；`:288-289` 实为「The parser records only usage from requests whose cache-inclusive input exceeded 272K」 |
| R1-4 [E4] | `local-api.js:615` | `local-api.js:614` | `:614` = `billable_total_tokens: 0,`；`:615` = `input_tokens: 0,` |
| R1-5 [E5] | `test/pricing.test.js` 1391 行 | **1392** 行 | `ReadAllLines` = 1392 |
| R1-6 [E6] | `src/lib/subscriptions.js` 507 行 | **508** 行 | `ReadAllLines` = 508 |
| R1-7 [I1] | 「与 `curated-overrides.json` 的对照只有 4 处」 | **6 处**（补 `:175` 的 `source_alias?.acode === undefined`、`:207-216` 的 `xsparkx2/xopglm53` deepEqual）；结论不变 | `test/edge-pricing-parity.test.js` 全文 420 行逐处核对 |
| R1-8 [I2] | 「在 edge 有、`curated` 没有的**大量**键」 | 量化为 **44 键**；并声明 iFlytek 64 条不属 `MODEL_PRICING` | 双向 Set 差集：curated-only **9**（逐字一致）/ edge-only **44** |
| R1-9 [I3] | 「全仓库对 `ensurePricingLoaded` 的调用只有两处」 | 「**生产代码**两处」（另有 20 行测试调用） | 全仓 grep 27 命中：`src/commands/serve.js:127`、`src/lib/local-api.js:3340` + `test/pricing.test.js` 16 处、`test/local-api-queue-cache.test.js:149`/`:220`、`test/grok-parser.test.js:301`、`test/omo-parser.test.js:229` |
| R1-10 [I4] | 「价格表只存在于后端与 edge 两处」 | 作用域写明「**生产路径 + `dashboard/src/`**」，并注明 dev/mock 例外 | `dashboard/src/` 下 4 类标识符 0 命中；例外：`dashboard/vite.config.js:508-514`（require 同一 pricing 模块）、`:1052-1057`、`:1067` |
| R1-11 [I7] | 「上游再导出到 `src/lib/local-api.js:60` 与 `:3530`」 | `:60` 是 **import 解构**入口、`:3530` 是 `module.exports` 再导出 | `src/lib/local-api.js:57-64` / `:3527-3531` |
| R1-12 [同源] | §7.1「`edge-patches/` 下另有 20+ 个 `.ts` 文件」 | 共 **28** 个文件（全部 `.ts`），除 5 份价格表外另有 **23** 个 | `Get-ChildItem -File` = 28 |

### 已确认靶子结论（R1 复核后标为已验证，行号已写入正文）

- **B9**：`test/edge-pricing-parity.test.js:76-85` 只做 edge↔edge 字节比对，**无** `edge ⊇ curated` 断言；差集恰 **9 键**（`claude-opus-5.5`、`claude-sonnet-5.5`、`cline-gateway-models`、`claude-opus-4-8`、`claude-haiku-4-5`、`antigravity-gpt-oss-120b`、`grok-4.5-build`、`grok-build-free`、`grok-4.5-build-free`，本轮逐字重算一致）；`grok-build-free` 云端落 `tokentracker-leaderboard-refresh.ts:481` → `:273` 的 `grok-build` = 1.25/2.5/0.2，本地 $0；`grok-4.5-build-free` 落 `:480` → `cursor-grok-4.5` = 2/6/0.5，本地 $0。
- **B10**：`src/lib/pricing/index.js:31` 定义、`:375` 导出、全文**零内部读取**；生效者 `src/lib/codex-rollout-parser.js:51` → `:733`；`src/lib/session-analytics.js:245` 仅为注释引用。
- **B16**：`dashboard/src/` 零价格副本；逐行计价点 `src/lib/local-api.js:2634-2640`（注释与实现逐字属实）。
- **§10 手工演算**：四段演算 + 3 锚点已用 JSON 原文独立复算，逐值一致。

### 未落实 / 保留为未确认

- **U1**（edge 五份函数的线上部署修订）：仓库内无部署产物或版本指针，仅 `CLAUDE.md:42` 提示人工部署，§11.2 第 9 条保留 `【未能确认】`。
- **U2**（`curated-overrides.json` note 引用的第三方定价页）：该表运行在 2026 年时间线（`claude-opus-5-5`、`gpt-6-astra`、`deepseek-v4` 等），不存在可核对的外部真实价页，联网核对无事实源，§11.2 第 8 条保留。
- 其余 E/I 项之外的分片内容本轮未改动；`index.js` 378、`matcher.js` 439、`curated-overrides.json` 775、`local-api.js` 3543、`session-analytics.js` 2934 等行数断言重算后**逐字命中**，保持原样。

### R2 修正（第二轮复核后）

依据 `REVIEW-R2-3-aggregation-pricing-consumers.md`（E=2 / I=8 / U=6；样本仓 HEAD `b19d27b0ccd59ce621f4fa17207b06358e67987c`，全程只读，未修改样本仓）。本轮 **采纳 3 条、无反驳**，全部由本轮独立打开源码/重算后落实。

| 编号 | 原表述 | 修正后 | 依据行号 |
| --- | --- | --- | --- |
| I3（§2.5） | 「全仓 grep 共 27 命中，其余 20 行全部在测试里」——未写口径，且与列举的 22 行不自洽 | 写明两种口径：正则 `/ensurePricingLoaded\(/` = **27 行** = 生产调用 2（`serve.js:127`、`local-api.js:3340`）+ 定义 1（`pricing/index.js:73`）+ 注释 4（`index.js:4`/`:41`/`:42`、`serve.js:123`）+ 测试调用 20（`pricing.test.js` 16 行逐行列出 + `local-api-queue-cache.test.js:149`/`:220` + `grok-parser.test.js:301` + `omo-parser.test.js:229`）；**裸标识符** = **36 行**（多出 9 行为 import/export 与测试名） | 本轮重跑正则计数（27 / 36 逐行核对） |
| I4（§7.4） | 「与 `curated-overrides.json` 的对照只有 **6 处**」，并把 Qwen 用例与两个跨文件断言都算进去 | 拆分后写为 **4 处 curated 对照**（`:170-172`、`:175`、`:208`、`:209`，全在 parity 文件内）；Qwen `:87-107` 比的是**硬编码字面量**（`:89`）不是 curated；`model-breakdown.test.js:480-503` 是**本地↔edge** Kiro 逐字节等价；`grok-parser.test.js:302`/`:342-343` 只断言本地侧——结论（无 `edge ⊇ curated` 断言）不变 | `edge-pricing-parity.test.js:20`/`:89`/`:170-175`/`:201-216`；`model-breakdown.test.js:480-503`；`grok-parser.test.js:302`/`:342-343` |
| I7（§2.5） | 「`~/.tokentracker/` 下的 JSON 只有 `cache/pricing.json`（缓存）这一类派生物」——字面不成立 | 限定为「**与价格有关**的派生物只有 `cache/pricing.json`」，并列出反例：限额链的 5 个磁盘缓存常量（`:70`/`:118`/`:128`/`:134`/`:141`），其中两份路径逐字核到为 `<home>/.tokentracker/tracker/<文件名>`（`:2219`、`:2299`）；它们与定价无关，本段结论不变 | `usage-limits.js:70`、`:118`、`:128`、`:134`、`:141`、`:2219`、`:2299` |
| U6（部分解除） | 限额磁盘缓存落盘目录未确认 | **部分结案**：`usage-limits-cache.json` 与 `claude-usage-limits-cache.json` 的路径拼接点已读实（`<home>/.tokentracker/tracker/`）；codex / opencode-go / rate-limit 三份同形态常量未逐个核拼接点，保留为残余未核项 | `usage-limits.js:2219`、`:2299` |

保留未动：U1（edge 五份函数线上部署修订）、U2（`curated-overrides.json` note 外部价页）不变，判定条件照旧。
