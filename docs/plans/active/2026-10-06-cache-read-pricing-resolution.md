# 缓存读取定价解析与候选选择修复实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 修复因渠道候选缺少 `cache_read` 单价而产生的大量未计价 token，同时支持明确声明“缓存读取按普通输入价计费”的模型，并保持“缺价不等于免费”的审计语义。

**Architecture:** 将缓存读取价格从“只有数值或缺失”扩展为三态：明确单价、明确沿用输入价、未知。候选比较只在价格可完整计算的候选中选最高费用；若所有候选都不完整，才保留当前的部分计价与 `unknown_tokens`。对被排除的不完整候选记录诊断信息，避免把不完整数据静默当成免费或完全可靠的最高价。

**Tech Stack:** Rust 2024、serde/TOML/JSON、Jiff、Tauri DTO、Vue 3、TypeScript、Vitest。

---

## 不变量与决策

1. `cache_read` 仍然是独立的真实 token 桶，不能并入 `input`。
2. `None`/缺少字段继续表示“未知”，不能自动解释为免费，也不能全局解释为输入价。
3. 只有显式声明的 `same_as_input` 才能让 `cache_read` 使用同一请求条件下最终生效的输入单价。
4. 显式 `0` 仍表示免费；`same_as_input` 不等于 `0`。
5. 分段和峰谷规则中，`same_as_input` 必须跟随同一档位、时间规则和倍率解析出的输入价。
6. 候选比较不能把缺价分项按 0 与完整候选直接比较。优先在“实际产生 token 的分项都有可解析价格”的完整候选中取最高费用。
7. 如果没有完整候选，才从部分候选中取已知费用最高者；未知 token 继续进入 `unknown_tokens`，`complete=false`。
8. 完整候选被选中时，公式可以完整计算，但如果存在被排除的不完整候选，明细必须提示“有不完整候选未参与主估算”，不能声称结果是所有渠道的严格上界。
9. 价格策略只允许通过外置配置或价格源明确数据声明“按输入价”；不能为 models.dev/OpenRouter 的所有缺失 `cache_read` 字段统一套输入价。
10. 旧版价格索引必须可读；旧索引中的 `Some(number)` 转为明确单价，缺失值转为未知，不改变既有结果语义。

## Task 1: 先写回归测试，锁定当前问题和三态语义

**Files:**
- Modify: `src/pricing.rs`（在现有缺价、候选选择测试附近增加测试）
- Modify: `src/report.rs`（增加请求级 gpt-5.4 类场景的回归 fixture）

**Step 1: 写失败测试**

增加以下测试场景：

- 一个候选的 input/output 更高但 `cache_read` 缺失，另一个候选四项完整；当请求包含 `cache_read` token 时，主结果选择完整候选，并记录被排除的不完整候选数量。
- 所有候选都缺少 `cache_read` 时，仍返回已知 input/output 小计，`unknown.cache_read` 保持非零。
- `cache_read` 明确声明 `same_as_input` 时，缓存读 token 使用当前分段/时间档的 input 单价，`complete=true`。
- 显式 `cache_read=0` 仍为免费且 `complete=true`。
- `cache_read` 缺失且请求中 cache_read token 为 0 时，不因该字段把请求标为不完整。

**Step 2: 运行测试确认失败**

Run: `cargo test pricing --lib`

Expected: 新增测试因三态字段和完整候选选择逻辑尚未实现而失败；现有测试应继续通过。

## Task 2: 建立可扩展的价格引用表示

**Files:**
- Modify: `src/pricing.rs`
- Modify: `src/modelsdev.rs`
- Modify: `src/openrouter.rs`

**Step 1: 增加运行时价格状态**

将四类价格的运行时表示扩展为可表达三种状态：

```rust
enum RateSpec {
    Unknown,
    Fixed(f64),
    SameAsInput,
}
```

`PriceRates` 的 input/output/cache_write/cache_read 都使用该表示；保留转换辅助方法，让现有计算代码可以得到 `Option<f64>`，但禁止在转换时把 `Unknown` 变成 `0`。

`SameAsInput` 只允许用于需要引用输入价的分项。当前只开放给 `cache_read`，其他字段遇到该值时在加载阶段报诊断并忽略相关规则。

**Step 2: 定义解析顺序**

在 `effective_rates` 之后先解析最终 input 价格，再解析 `cache_read`：

- `Fixed(v)` → 使用 `v`；
- `SameAsInput` → 使用同一计划/分段/时间规则得到的 input 价格；
- `Unknown` → 保持未知。

这样分段、峰谷、未来倍率扩展都只需要改变 input 的最终解析，不需要复制缓存逻辑。

**Step 3: 保持旧索引兼容**

将价格索引版本提升到 v5。读取 v4 索引时：

- 数值字段转为 `Fixed(number)`；
- `null`/缺失转为 `Unknown`；
- 不推断 `SameAsInput`。

新增 v5 round-trip 测试，确保 `Unknown`、`Fixed(0)`、`SameAsInput` 不互相混淆。

## Task 3: 接入价格源和外置“按输入价”声明

**Files:**
- Modify: `src/pricing.rs`
- Modify: `src/modelsdev.rs`
- Modify: `src/openrouter.rs`
- Modify: `docs/stats-semantics.md`

**Step 1: 保持源数据的保守默认值**

- models.dev 数字 `cache_read` → `Fixed`；缺字段 → `Unknown`。
- OpenRouter 数字 `cache_read` → `Fixed`；缺字段 → `Unknown`。
- 不因为某个来源只有 input/output 就自动生成 `SameAsInput`。

**Step 2: 增加外置配置语法**

在 `~/.tokenscope/pricing.toml` 支持：

```toml
[[model_policy]]
prefix = "some-model"
channel = "some-channel"      # 可选；省略表示匹配所有同末段候选
cache_read = "same_as_input"
```

`model_policy` 只表达语义，不生成一条没有 input/output 价格的新候选。加载时把策略应用到匹配候选的 `Unknown cache_read` 上；已有明确数字或显式 0 的候选不得被覆盖。

策略支持末段模型匹配、可选 channel/source 限定，并在没有任何候选命中时给出 warning，不静默制造价格。

如果未来 models.dev 明确提供“缓存读沿用输入价”的结构化字段，快照同步层将该字段转换为 `SameAsInput`；缺字段仍保持 `Unknown`。

**Step 3: 更新统计口径文档**

在 `docs/stats-semantics.md` 增加：

- `Unknown`、`Fixed(0)`、`SameAsInput` 的区别；
- `same_as_input` 在分段和峰谷规则中的解析方式；
- 不完整候选被排除时的警告语义；
- 外置 `model_policy` 的配置示例和禁止全局推断的规则。

## Task 4: 修复候选选择，避免不完整高价候选制造可避免的 unknown

**Files:**
- Modify: `src/pricing.rs`
- Modify: `src/report.rs`

**Step 1: 拆分候选评估状态**

为每个候选保留：

- 已知小计 `cost`；
- `unknown` token 数；
- 是否完整；
- 命中的分段、时间规则和最终四类单价。

`SameAsInput` 成功解析后视为已计价，不进入 unknown。

**Step 2: 实现两阶段选择**

候选选择规则：

1. 先收集所有完整候选，按本次请求的实际 token、分段、事件时间和时间规则计算，取费用最高者。
2. 若没有完整候选，再从部分候选中取已知费用最高者，并保留 unknown token。
3. 同价时继续使用现有来源优先级、完整匹配、前缀长度和原始键 tie-break。
4. 禁止跨候选拼接单价；最终公式仍来自同一个候选。

`MatchedCandidate` 增加诊断字段：

- `complete_candidate_count`；
- `incomplete_candidate_count`；
- `incomplete_candidates_excluded`；
- 更新后的 `reason`（例如 `highest_complete_cost`、`highest_partial_cost`）。

当完整候选胜出但存在被排除的不完整候选时，设置结构化 warning；这不是所选公式的 `unknown_tokens`，而是“其他候选缺少价格、未参与主估算”的不确定性提示。

**Step 3: 保持聚合语义一致**

`aggregate.rs` 不改变四桶 token 聚合逻辑，只读取 `CostEstimate.unknown`。完整候选胜出时，gpt-5.4 这类事件的 cache_read 不再进入 unknown；所有候选都缺价时，旧的 unknown 行为保持不变。

## Task 5: 扩展请求级 breakdown 和前端悬浮公式

**Files:**
- Modify: `src/pricing.rs`
- Modify: `src/report.rs`
- Modify: `frontend/src/types.ts`
- Modify: `frontend/src/lib/costBreakdown.ts`
- Modify: `frontend/src/components/EventTable.vue`

**Step 1: 让每个分项说明单价来源**

`CostLine` 增加 `rate_kind`：

- `fixed`：明确数值单价；
- `same_as_input`：使用 input 单价；
- `unknown`：没有可用单价。

对 `same_as_input`，`unit_price` 输出解析后的实际数值，确保金额仍由后端统一计算；前端只负责显示。

**Step 2: 更新公式文案**

悬浮明细中将：

```text
缓存读 50,000 × 输入价 $2.50/M = $0.125000
```

与普通固定价、未知价格区分显示。来源区域增加：

```text
有 3 个候选因缺少缓存读价格未参与主估算
```

未知行仍显示“缺少单价”，不能显示为“免费”。

**Step 3: 更新 TypeScript 与测试**

为 `rate_kind`、候选诊断字段增加类型；补充 `costBreakdown.test.ts` 和 `EventTable.test.ts`：

- same-as-input 公式可见且单价正确；
- unknown 仍高亮；
- 完整候选胜出时显示候选排除 warning；
- `Fixed(0)` 显示 `$0`，不显示未知标记。

## Task 6: 增加真实问题回归 fixture 与索引迁移测试

**Files:**
- Modify: `src/pricing.rs`
- Modify: `src/report.rs`
- Modify: `src/modelsdev.rs`
- Modify: `docs/stats-semantics.md`

**Step 1: 添加 gpt-5.4 风格 fixture**

构造至少三个同末段候选：

- `zenmux/...`：input/output 更高，cache_read 缺失；
- `cortecs/...`：四项完整；
- `openai/...`：官方 input/output/cache_read 完整，cache_write 显式 0 或按源语义处理。

验证带 cache_read token 的请求选择最高完整候选，并报告排除的不完整候选；验证无 cache_read token 时不误报 unknown。

**Step 2: 添加“按输入价”模型 fixture**

通过 `model_policy` 声明 `cache_read = "same_as_input"`，验证：

- 普通基础价生效；
- 高上下文分段命中时使用高档 input 价；
- 峰谷时间规则命中时使用该时间档 input 价；
- 同一末段下已有明确 cache_read 数字的候选不被策略覆盖。

**Step 3: 验证 v4 → v5 索引迁移**

用旧索引 fixture 验证旧数据继续产生相同费用；用 v5 fixture 验证三态价格能完整保存和恢复。

## Task 7: 全量验证、文档回读与提交

**Files:**
- Verify: `src/pricing.rs`, `src/report.rs`, `src/aggregate.rs`, `src/modelsdev.rs`, `src/openrouter.rs`, `frontend/src/types.ts`, `frontend/src/lib/costBreakdown.ts`, `frontend/src/components/EventTable.vue`, `docs/stats-semantics.md`

**Step 1: 运行 Rust 检查**

Run:

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test pricing --lib
cargo test report --lib
cargo test
```

Expected: 全部通过；测试不得读写真实 `~/.tokenscope`，价格索引和 cache_dir 使用临时目录。

**Step 2: 运行前端检查**

Run:

```powershell
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test -- --run
pnpm --dir frontend build
```

Expected: 全部通过，未知价格、same-as-input 和候选 warning 的文案无类型错误。

**Step 3: 真实缓存只读验收**

用当前 `~/.tokenscope` 的快照和日志执行一次只读汇总，确认：

- `gpt-5.4` 的可完整计价请求不再因为 zenmux 缺 cache_read 而进入 unknown；
- 所有候选都缺价的模型仍显示 unknown；
- 明细悬浮显示实际命中的候选、单价来源和被排除候选 warning。

**Step 4: 回读文档并提交**

确认 UTF-8 编码、计划中的字段名与实现一致后，按项目约定提交：

```powershell
git add src/pricing.rs src/report.rs src/aggregate.rs src/modelsdev.rs src/openrouter.rs frontend/src/types.ts frontend/src/lib/costBreakdown.ts frontend/src/components/EventTable.vue docs/stats-semantics.md
git commit -m "fix(计价): 区分缓存未知价格与按输入价计费"
```


## 执行记录（2026-10-06）

| 任务 | 内容 | 提交 |
| --- | --- | --- |
| Task 1+2 | `RateSpec` 三态（Unknown/Fixed/SameAsInput，serde 线格式与旧 `Option<f64>` 兼容）；解析顺序 = 先 input 后 cache_read 引用同层输入价；SameAsInput 白名单仅 cache_read（其余加载拒绝）；索引 v5 且兼容读 v4（v2/v3 仍失效）；CostLine.rate_kind；回归测试 6 项 | b86d09a |
| Task 3 | `[[model_policy]]` 外置声明（末段匹配 + 可选 channel/source，只改写 Unknown cache_read，不覆盖显式值、不生成候选、未命中告警）；models.dev/OpenRouter 缺失保持 Unknown；PRICING_TEMPLATE 与 stats-semantics.md §4 增补 | 9a97909 |
| Task 4 | 两阶段候选选择：完整候选优先取最高（缺价不按 0 比较），无完整候选回退部分最高；MatchedCandidate 三诊断计数 + reason 两阶段命名；CostEstimate.excluded_candidate_warning 结构化提示；tie-break 与禁拼价不变；cost_breakdown 场景更新为新语义 | 684829e |
| Task 5+6 | EventCostBreakdown 透传排除提示；前端类型/公式行「输入价 $x/M」/来源区两阶段说明与估算范围行；report 级 gpt-5.4 三渠道 fixture（zenmux 排除、cortecs 完整胜出） | 7719d7a |
| Task 7 | 全量门禁（Rust 双 manifest + 前端四件套）全绿；真实数据只读验收：gpt-5.4 2384 请求 / 3.08 亿 cache_read token 全部完整计价 $136.26、unknown_pricing=false（修复前进 unknown）；临时 example 已删除 | 见最终提交 |

不变量对照：缺价 ≠ 免费（Unknown 保持 unknown）；显式 0 = 免费；
SameAsInput 仅 cache_read 且跟随同层（分段/峰谷档）输入价；聚合四桶
逻辑零改动（aggregate.rs 未触碰）。
