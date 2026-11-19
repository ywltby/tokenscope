# v0.9.4：模型跨渠道别名合并 + CCS 日汇总下钻行

- 日期：2026-11-19
- 分支：`fix/v094-model-identity-rollup-drilldown`（自 `origin/main` = 2a48ea9）
- 版本：0.9.3 → **0.9.4**（`python -B scripts/bump_version.py --bump patch`）
- 状态：实施中

## 0. 背景与已核实根因

用户在本机 v0.9.3 核实两个问题：

1. **同一模型被拆成多行**：`anthropic/claude-fable-5.2` 与 `claude-fable-5-2`
   都显示为「Claude Fable 5.2」，却是两个分组行。
   根因：分组键 `ModelIdentity::identity_key()` 保留 `/` 渠道结构
   （`anthropic/claudefable52` vs `claudefable52`），而展示名
   `Pricing::display_name_for` 按**末段** `leaf_key` 查 models.dev——两侧
   命中同一条目，标签相同、身份不同。覆盖桶（`collect_detail_buckets` /
   `select_rollup_contributions`）同样按 `identity_key` 分桶：原生请求与
   CCS 汇总若写法不同（裸名 vs 渠道前缀），会落入不同桶——汇总桶被误判
   「无明细」而整桶采用，原生明细又照常计入，**重叠用量双计**。
2. **CCS 日汇总下钻为空**：`claude-opus-4-7` 无逐请求记录，聚合采用两条
   CCS 日汇总（2026-05-28：223 请求；2026-05-29：11 请求；合计
   110,198,123 token），但 `query_events` 只返回逐请求事件
   （`query.rs` 快照行 = 事件索引），下钻 0 行。

## 1. 不变量（先于代码成立）

- **I1 事实与计价依据不变**：原始事件、请求去重（写入路径
  `event_write_of` / `lookup_overlap_candidate` 仍用结构 `identity_key`）、
  逐请求 `estimate` 的候选/分段/时间档规则一律不改。
- **I2 查询时派生，升级零迁移**：规范身份是**查询快照内**由冻结价格表派生
  的视图；历史库任何列（含 `model_identity` / `identity_revision`）不改写、
  不 bump `IDENTITY_REVISION` / `INDEX_VERSION` / `HISTORY_SCHEMA_VERSION`、
  不改 `persistent_format` 指纹。已导入数据升级后直接生效，无需清库或重导。
  无 models.dev 快照时行为与今天完全一致（保守降级，不合并）。
- **I3 确认边界（宁可少合并，不可误合并）**：只有两类关系可确认别名——
  (a) 完整标识与某 models.dev 条目 ID 等价（含大小写/`-._` 压缩）；
  (b) **无渠道前缀**的裸标识，其 `leaf_key`（含变体结构）**精确**命中
  **唯一一个** models.dev 模型。其余一律保持结构身份：
  - 不同版本（日期后缀等）不合并——**前缀回退不参与身份确认**；
  - 不同变体（`:free` / `:thinking` / 显式空变体）不合并；
  - 同名不同 ID（仅展示名相同）不合并；
  - 裸末段命中多个 models.dev 模型（歧义）不合并；
  - 带未确认渠道前缀的写法（如 `zenmux/…`）不合并；
  - 压缩后为空的基名不进入确认。
- **I4 展示与明细**：聚合行/图表/下钻共用同一规范键；聚合行标签优先可信
  models.dev 模型名，缺失用确定性原始代表写法（绝不展示压缩键）；请求明细
  行继续显示各自**原始 ID**。
- **I5 覆盖与分组同一身份**：覆盖桶键、`covered_detail`、聚合跳过判定、
  下钻匹配全部使用同一规范键；三向选择（A/B/C）规则本身不变，别名不得
  造成双计或漏计。
- **I6 下钻汇总行 = 聚合最终采用的贡献**：只从同一冻结快照的
  `RollupSelection.contributions` 取数；选中模型每个 CCS 统计日一行，
  同日多渠道（provider/批次）合并；请求数、四桶、费用与聚合贡献**逐项一致**
  （共用同一费用计算函数，不得另写一套）。
- **I7 日粒度诚实**：应用列显示「CCS」；日期 = CCS 来源统计日；不补造
  时分秒、请求 ID、会话或项目归属。
- **I8 既有规则保持**：`detail_only`、时刻范围（instant）、时区不匹配
  （按日敏感时）不产生汇总行；被覆盖请求仍可查看并**标记**其用量已计入
  当日汇总（不重复进入总量）；A/C 类桶不产生汇总行。
- **I9 混合分页稳定**：事件行与汇总行在同一有序列表分页，游标绑定
  会话/主指纹/下钻指纹/行位置；无漏行、无重复行、顺序确定；旧事件游标
  在升级后仍可用（v2 游标向后兼容扩展）。
- **I10 测试密闭**：全部合成夹具 + 临时目录（`cache_dir` / `pricing_index`
  等注入），不触碰真实 `~/.tokenscope`、真实 history.db 或 CCS 数据。

## 2. 方案 A：规范模型身份（查询时归一）

### 2.1 新函数 `Pricing::canonical_identity_key(model: &str) -> String`

```text
id = ModelIdentity::parse(model); structural = id.identity_key()
若 id.is_empty_key() → structural
group = by_prefix[id.leaf_key()]          # 精确末段键（含变体），不做前缀回退
trusted = { identity_key(e.display) : e ∈ group, e.tier == TIER_MODELSDEV }
若 trusted 恰有一个键 K：
    structural == K        → structural   # 本身即规范（渠道官方写法）
    id.namespace() 为 None → K            # 裸别名经唯一可信模型确认
    其余                   → structural   # 未确认渠道前缀，保守不合并
否则（0 个或多个）→ structural
```

- `by_prefix` 键本就是 `match_key = leaf_key`，`Entry.display` 保留完整
  `provider/model` 原文——无需改索引结构或版本。
- 幂等：`canonical(structural) == canonical(raw)`（`identity_key` 保持
  `/`、`:` 结构同态），因此对库内存储键或原始写法归一结果一致。
- 规范键仍是结构形式（`anthropic/claudefable52`），不是压缩展示名。

### 2.2 应用点（全部查询侧）

| 位置 | 改动 |
| --- | --- |
| `aggregate::push_view` | 以 `pricing.canonical_identity_key(e.model)` 作为覆盖判定与 `GroupBy::Model` 分组键 |
| `aggregate::push_rollup_into` | `GroupBy::Model` 键 = `canonical(r.model)` |
| `aggregate::covered_by_rollup` | 事件侧用规范键（桶键已是规范，见下） |
| `report::collect_detail_buckets` | 桶键 = `canonical(r.model)`；**SQL 收窄集合扩展**：新增 `ReadSnapshot::distinct_model_identities(app)`（`SELECT DISTINCT model_identity`，走 `idx_usage_model_time`），逐一经 `canonical` 归一后保留命中汇总规范键者，作为 `for_each_summary_chunk_for_models` 的 IN 集合；回调内事件键同样归一 |
| `report::select_rollup_contributions` | 增加 `pricing` 参数；桶键 `model_identity` = 规范键（`CoveredBucket` 随之携带规范键） |
| `report::query_events_from` | 模型筛选键 = `canonical(filter.model)`；事件匹配 `canonical(e.model) == key` |
| `query::drill_fingerprint` | 增加 `pricing` 参数，模型分量 = 规范键（两种写法同一子查询指纹，MP04 语义扩展） |
| `query::begin_query_with` | 向上述两函数传入 `collection.pricing` |

写入路径（`event_write_of`、CCS 导入、重叠候选查询）**不改**。
`model_label` / 代表写法逻辑不改：合并后代表 = 组内最小原始写法
（`anthropic/claude-fable-5.2`），`display_name_for` 命中可信名。

### 2.3 升级与降级

- 旧库存储键不变 → 新代码查询时归一即合并；无需任何迁移。
- 价格快照缺失/未同步 → `trusted` 为空 → 全部保持结构键（与 v0.9.3 行为
  一致）；同步后自动合并。会话内价格冻结 → 同一分页会话身份稳定。

## 3. 方案 B：CCS 日汇总下钻行

### 3.1 行模型与 DTO

`EventRow` 增加两个可选字段（`skip_serializing_if = None`，旧前端兼容）：

```rust
/// Some = 本行是 CCS 日汇总行（日粒度，非逐请求）。
pub rollup: Option<RollupRowMeta>,
/// Some(day) = 本请求用量已计入该 CCS 统计日的日汇总（B 类覆盖）。
pub covered_by_rollup: Option<String>,
```

`RollupRowMeta`（镜像 `Group` 费用字段，供前端复用既有费用浮层）：
`requests`、`day`（来源统计日）、`source_tz`、`label`（可信展示名或确定性
原始代表）、`models`（合并的原始渠道 ID，去重排序）、`cost_usd`（估算部分）、
`source_cost_usd`、`source_cost_state`、`total_cost_usd`（显示金额）、
`unknown_tokens`、`unknown_pricing`、`cost_partially_unknown`、
`has_estimated_cost`、`cost_detail`（恒 `rollup_only`）、
`cache_write_split_estimated`、`cache_price_zero_estimated`。

汇总行的 `EventRow` 常规字段：`ts = "YYYY-MM-DD"`（无时分秒）、
`record_id = ""`、`session_id = ""`、`project = ""`（前端显示「—」）、
`agent = "claude-code"`（前端应用列渲染「CCS」）、四桶 = 合并 token、
`cost_usd = total_cost_usd`（None = 完全无费用依据 → 既有「未知」标签）。

### 3.2 取数与合并（与聚合逐项一致）

- 来源：`snapshot.rollup_selection().contributions`（冻结、仅最终采用）。
- 合并键 = `(day, canonical_model)`；跨 provider / logical_source 合并。
- 排序：**day 降序 → 规范模型键升序**（确定、与筛选无关）。
- 费用：从 `push_rollup_into` 抽出共享函数
  `aggregate::rollup_cost_parts(r, pricing) -> RollupCostParts`
  （来源费用已提供 → 该值、无 unknown；缺失 → `estimate_rollup`，
  `covers_used_tokens` 判定依据；无候选 → 全 unknown），聚合与下钻行
  **共用**，逐条计算后受检累加。`total_cost_usd` 合成规则与 `finish` 相同。
- 请求数/四桶：受检累加（SF08）。

### 3.3 筛选对齐

- 模型下钻：`canonical(filter.model)` 匹配汇总行规范键；
- 日下钻：`filter.day == row.day`（来源统计日；按日敏感时区不一致的汇总
  本就不在 contributions 里）；
- 项目下钻：汇总行无项目归属 → **一律不返回汇总行**；
- 无下钻：返回全部采用汇总的合并日行（每模型每日一条）；
- `agent` 主筛选、`detail_only`、instant、时区规则由 contributions 为空
  天然保证（I8）。

### 3.4 混合分页与游标

- 位置空间：事件行 `[0, E)`（`E = snapshot.row_count()`，沿用绝对行序），
  汇总行 `[E, E + R)`（R = 当前筛选下合并日行数）。
- `PageCursor`（v2，`deny_unknown_fields`）向后兼容扩展：
  `#[serde(default = "event")] kind: String`、`#[serde(default)] ridx: Option<usize>`。
  旧游标缺省 `kind = "event"` 照常定位；汇总行游标 `kind = "rollup"`、
  `ridx = i`（ts/rid/seq 置空占位）。
- `locate_cursor` 增加汇总行总数参数：`kind = rollup` 校验
  `ridx < R` → 返回 `E + ridx`；否则按现有 (ts, rid, seq) 定位。
  归属校验（qid/mfp/dfp）不变；dfp 已绑定筛选 → 同一 dfp 下 R 确定。
- `query_events_from`：
  - 无下钻快路径：`total = E + R`；事件段按现有区间批读，页跨入汇总段时
    追加构建对应日行；
  - 有下钻：事件遍历照旧计 total/收页；随后遍历汇总日行，
    `position = E + i`，同一 `position >= after && rows.len() < limit` 闸门；
    `total += R`。
  - 汇总行按需构建（先算合并键列表定位，再只为页内行算费用），内存与
    页大小成正比，不绕过会话预算（RC05）。
- `build_event_row`：以 `selection.covered_detail` + 规范键 + 来源时区日 +
  `ts <= since` 判定覆盖（与聚合跳过判定**同一规则**），命中写
  `covered_by_rollup = Some(day)`。

### 3.5 前端

- `types.ts`：`RollupRowMeta`、`EventRow.rollup?` / `covered_by_rollup?`。
- `EventTable.vue`：
  - 时间列：汇总行 = 日期 + 「日汇总」标签 + 「N 次请求」标签；
    被覆盖请求 = 时间 + 「已计入日汇总」标签（浮层说明计入哪一天、
    不重复进入总量）；
  - 应用列：汇总行渲染「CCS」；
  - 模型列：汇总行 = `meta.label`，浮层列出合并的原始渠道 ID；
    请求行保持原始 ID；
  - 项目列：汇总行「—」；
  - 费用列：汇总行用 `meta` 构造 pseudo-Group 复用
    `GroupCostBreakdownTooltip`（来源费用/估算构成/未知 token 披露与
    聚合行同一契约）；`cost_usd == null` → 既有「未知」标签。
- i18n：zh-CN / en-US 同步新增键（日汇总标签、请求数、CCS、覆盖标签与
  说明、无项目占位、渠道合并浮层标题）。
- `useUsageQuery` / `ScrollList` / 快照恢复：游标不透明、rows 追加、
  total 含汇总行——无需改动；视图快照新增可选字段向后兼容。

## 4. 红-first 复现与测试矩阵（合成夹具）

新集成测试 `tests/model_alias_rollup_drilldown.rs`（临时目录 + 注入
`cache_dir`/`pricing_index`/`modelsdev_path` 等；models.dev 快照用 **v4**
模型级 name；CCS 库用 `ccs_rollup_coverage.rs` 同形合成 schema）：

先写测试并在当前代码上运行，确认按预期**红**（问题 1 两行、问题 2 零行、
别名覆盖双计），再实施。

1. **Fable 合并**：原生 Claude 日志两种写法（`anthropic/claude-fable-5.2`
   ×2、`claude-fable-5-2` ×3）+ models.dev 条目（name「Claude Fable 5.2」）
   → by=Model 恰 1 组、label 可信名、requests=5、四桶/费用=两侧之和；
   下钻传 `Group.key` 与两种原始写法均命中 5 行，行内模型仍是原始 ID。
2. **边界不合并**：日期后缀版本、`:free`/`:thinking`/显式空变体、
   同名不同 ID、裸末段歧义（两个 provider 同末段）、未确认渠道前缀
   （`zenmux/…`）各自独立成行；前缀回退命中同价也不合并。
3. **覆盖别名分桶**：裸名原生请求（快照内）+ `anthropic/…` 写法 CCS 日汇总
   同日 → B 类：总量 = 汇总 + 快照后新增，不双计；`covered_detail` 命中；
   下钻中该请求带覆盖标记。
4. **Opus 纯汇总下钻**（用户数值）：两条日汇总 2026-05-28（223 请求）/
   2026-05-29（11 请求），四桶合计 110,198,123，无原生请求 → 聚合
   requests=234；下钻 2 行 CCS 日行，day/请求数/四桶/费用与聚合逐项一致。
5. **混合场景**：原生请求 + 被覆盖请求 + 快照后新增 + 同日双渠道汇总
   （合并为一行，请求数/四桶/费用=两渠道之和）；对账：
   Σ(未覆盖请求) + Σ(汇总行) == 分组 requests/四桶/total_cost_usd。
6. **筛选**：日下钻只回当日事件+当日汇总行；模型下钻只回该模型；
   项目下钻无汇总行；agent=codex 无汇总行。
7. **分页**：limit=1/2 走完混合列表——无漏无重、顺序稳定；事件游标续页
   跨入汇总段正确；两种写法共享下钻指纹（游标可续用）；旧格式（无 kind）
   游标仍可定位事件行。
8. **既有规则**：`detail_only` / instant 范围 / 按日敏感时区不匹配 →
   无汇总行（相应提示字段保持）。
9. **升级零迁移**：同一临时 history.db，先无 models.dev 快照查询（2 行，
   与旧行为一致）→ 放入快照后**不重导**再查（1 行）；库内
   `model_identity` 列值前后不变。
10. **费用一致**：来源费用提供（含显式 0）/缺失可估算/部分缺价/无候选
    四态下，汇总行金额与聚合 `total_cost_usd` 贡献一致（共享函数保证）。

另：`pricing.rs` 单元测试锁定 `canonical_identity_key` 规则（含幂等、
歧义、变体、空键）；前端 `EventTable` 组件测试（汇总行/覆盖标记渲染）、
i18n 键 parity、`chartData` 单行断言；真实浏览器走查（qa-artifacts
夹具 + `scripts/check-ui-contracts.mjs` 契约，必要时新增下钻汇总行契约）。

## 5. 文档与版本

- `docs/stats-semantics.md`：
  - §4「模型身份分组与展示名」段补**规范身份确认规则**（两类可确认关系、
    保守边界、查询时派生、升级零迁移）；
  - §3.5 游标段补 v2 可选 `kind`/`ridx` 与混合行位置空间；
  - §5.1 补「桶键使用规范身份」一句；新增 §5.2「下钻日汇总行」
    （取数/合并/日粒度/筛选/覆盖标记/分页/费用一致）。
- 本计划随实施回填「交付状态」。
- 版本：实施完成后 `python -B scripts/bump_version.py --bump patch`
  （0.9.3 → 0.9.4），`--check --base-ref origin/main` 校验。

## 6. 验收与交付

对应用户验收标准：

- [x] 两个 Fable 渠道 ID 表格/图表只出现一次；下钻见两侧请求；四桶与费用
      对账一致；不同版本/变体/仅同名模型仍分开。
- [x] 纯 CCS 汇总模型下钻按两天显示两条 CCS 行；混合/覆盖/同日多渠道/
      筛选/跨页全部验证。
- [x] 已导入数据升级后直接生效（测试 9 证明，无需清库/重导）。
- [x] Rust 全量（`scripts/run_rust_tests.py`）、前端 typecheck/format/
      test/build、真实浏览器回归通过。
- [x] 口径与本计划更新；版本 0.9.4。
- [ ] 正常钩子提交 → 推送 → PR → 最新 HEAD 必需 CI 绿。
- [ ] 汇报修复前后行数、对账结果、PR 与 HEAD；**停在可审查状态**
      （合并、发布、本机安装另行授权）。

## 7. 交付状态

### 7.1 修复前后行数与对账（合成夹具，红-first 已确认）

| 场景 | 修复前 | 修复后 |
| --- | --- | --- |
| Fable 两渠道写法（by=Model） | 2 组（标签同为 Claude Fable 5.2） | **1 组**，requests=5，四桶/费用=两侧之和；下钻 5 行（两种原始写法均命中），行内模型仍为原始 ID |
| 图表 Fable 类目 | 2 条 | **1 条** |
| Opus 纯 CCS 汇总下钻 | **0 行** | **2 行 CCS 日行**：2026-05-28（223 请求）/2026-05-29（11 请求）；聚合 requests=234、四桶合计 110,198,123；日行请求数/四桶/费用与聚合贡献逐项一致 |
| 覆盖别名分桶（裸名原生 + 渠道写法汇总同日） | 重叠双计 | B 类：总量=汇总+快照后新增（12=10+1+1），`covered_detail` 命中，下钻带覆盖标记 |
| 混合对账 | — | Σ(未覆盖请求)+Σ(汇总行)==分组 requests/四桶/total_cost_usd |
| 混合分页 limit=1/2 | — | 无漏行/无重复行、顺序稳定；事件游标跨入汇总段正确；两写法共享下钻指纹；旧格式游标兼容 |
| 升级零迁移 | — | 无快照 2 组（旧行为）→ 放入快照**不重导** 1 组；库内 `model_identity` 列值不变 |
| 边界不合并 | — | 日期后缀/`:free`/`:thinking`/显式空变体/同名不同 ID/裸末段歧义/未确认渠道前缀 各自独立 |

### 7.2 回归与门禁

- Rust 全量 `scripts/run_rust_tests.py`：**1030 passed / 85 ok binaries，EXIT=0**
  （新增 `model_alias_rollup_drilldown` 12 例；`ccs_rollup_coverage` 26、
  `model_identity_contract` 14、`ccs_rollup_source_cost` 22、
  `ccs_rollup_source_selection` 8 全绿）。
- 前端门禁链 typecheck → format:check → test → build → test:e2e：
  **GATE_EXIT=0**；vitest 81 文件 / 1179 例 passed；build ✓；
  e2e 13 契约含新增 `rollup_drilldown_rows`（13 断言）✓，巡逻无 console error。
- 真实浏览器走查截图：`qa-artifacts/v094-rollup-drill/`（gitignored，不入库）。
- 版本：`python -B scripts/bump_version.py --bump patch` → **0.9.4**；
  `--check --base-ref origin/main` = {previous 0.9.3, kind patch,
  base_sha 2a48ea9, release true}。

### 7.3 提交 / PR / CI

- 提交链（均经正常 pre-commit 全量钩子，无 `--no-verify`）：
  `ee589ed`（主修复：模型跨渠道身份归一 + CCS 日汇总下钻行）→
  `621ef42`（原生身份契约改用规范模型键）→
  `e963c3b`（原生 DOM 与并集判定纳入日汇总下钻行）→ 本回填提交（仅文档）。
- PR：[#74](https://github.com/ywltby/tokenscope/pull/74)
  （`fix/v094-model-identity-rollup-drilldown` → `main`）。
- 必需 CI 于 `e963c3b` 全绿：
  [CI](https://github.com/ywltby/tokenscope/actions/runs/44473976522)
  rust 5m57s / rust-acceptance 6m16s / frontend 6m48s / contracts 9m6s / gate；
  [Install test](https://github.com/ywltby/tokenscope/actions/runs/44473976489)
  install-test 12m9s / plan-upgrades / 12 组单跳升级矩阵；
  [Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/44473976487)
  native-ui 12m7s。native-ui 首跑在 AC10 因 runner WebView2 SmartScreen
  临时文件走查竞态（ENOENT，与本批改动无关）失败一次，
  `gh run rerun --failed` 后全绿；本机同脚本复跑 ui-identity 22/22、
  ui-ccs-import 18/18、ui-pricing 19/19、privacy-preseeded 15/15。
- 本回填提交仅改文档；推送后必需 CI 以 PR 最新 HEAD 实跑为准，结论在最终汇报贴出。
- 停在可审查状态：不合并、不发布、不安装（另行授权）。

### 7.4 未验证项

- 合并、发布、本机安装：**未执行**（另行授权）。
- 真实 `~/.tokenscope` / history.db / CCS 数据：未触碰（全部合成夹具+临时目录）。