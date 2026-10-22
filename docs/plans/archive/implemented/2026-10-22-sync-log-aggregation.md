# 价格同步告警汇总

> **2026-10-23 归档复核：LA01–LA04 与 §5 验收已完成。** [PR #37](https://github.com/ywltby/tokenscope/pull/37) 合并 `0fabf24`，v0.5.11 已发布且已登记升级基线，main 的 CI、Native UI E2E、Release 均成功。隔离程序取证计数与 §1.1 预期一致：models.dev WARN 0 / 兼容明细 415，OpenRouter WARN 2（两个类别）/ 明细 23。用户 2026-10-23 复核 PR 后确认合并，无阻断项；复核提出的三个小项随归档处理——README「待复核/实施中」措辞随移档消除、§6 补 PR 号、WARN 汇总行措辞由「同步丢弃数据汇总」改为中性「同步告警汇总」（冲突类并未丢弃数据，随 v0.5.12 发布）。开发版手动同步的人工复核按任务修订保留给用户，属既有 N11 口径，未借本专项标为通过。

## 归档前原文

> 状态：**实施中（2026-10-22，分支 fix/sync-log-aggregation）。** 用户 2026-10-22 确认计划文本（含兼容字段分类修订）。 本计划修复一处日志规约违背，是独立的维护项，不占整合计划的责任编号；合并并完成发布验收后归档，并在计划总账登记。来源：用户 2026-10-22 在 0.5.10 开发版手动同步价格后发现日志刷屏。

**Goal：** 一次价格同步在默认日志级别下，每个来源每类告警只写一条汇总；逐条明细保留在 debug 级别，同一告警不再写两遍；models.dev 的兼容字段不再误报为冲突。

**Architecture：** models.dev 的档位转换不再在循环里逐条写日志，改为把告警收集到同步报告中，与 OpenRouter 的做法一致。两个来源的告警由同一个汇总函数按类别分组，手动同步和自动同步都只在调用方记录一次。

**Tech Stack：** Rust、`log`；`modelsdev.rs`、`openrouter.rs` 的单元测试，以及壳的同步命令。

## 1. 已核实事实（2026-10-22）

### 1.1 本机实测

一次手动同步（`~/.tokenscope/logs/tokenscope.log.2026-10-22`，14:43:53–14:43:56）共写了约 460 条 WARN：

| 来源 | 告警 | 条数 | 原因 |
| --- | --- | --- | --- |
| models.dev | `{provider}/{model}: context_over_200k 与 tiers 冲突，优先 tiers` | 415 | 逐模型逐条 |
| OpenRouter | `OpenRouter 同步丢弃数据: {model}: …`（时间条件档跳过 / 非法单价整条拒绝） | 23 × 2 | 逐条，并且写了两遍 |

同步本身正常：models.dev 8734 条、OpenRouter 461 条，3020 ms。

### 1.2 代码位置

- `crates/tokenscope-core/src/modelsdev.rs:211-213`：`sync_with` 在逐模型循环里，对 `convert_tiers` 返回的每条告警都 `log::warn!`。`convert_tiers`（第 265 行起）可能产生 4 类告警：
  - 非 context 类型的档位；
  - 档位大小非法；
  - 重复的档位大小；
  - `context_over_200k` 与 tiers 冲突。

  `modelsdev::SyncReport`（第 159 行）不携带告警。
- `crates/tokenscope-core/src/openrouter.rs:356-358`：`sync_with` 逐条 `log::warn!`，同时把同一批告警放进 `SyncReport.warnings`（第 150 行）。
- `src-tauri/src/commands.rs:1582-1584`：手动同步再遍历 `r.warnings` 逐条 `log::warn!`。这就是同一告警写两遍的原因。
- `src-tauri/src/lib.rs:1027-1035`：自动同步（24 小时间隔）不读取 `r.warnings`，只靠核心库那一遍逐条日志。
- 违背的规约：CLAUDE.md 日志规约要求 warn 用于降级与可恢复异常，并「按次聚合」，禁止逐条事件刷屏。

### 1.3 「tiers 冲突」的实际形态

models.dev 用两种字段描述长上下文加价：
- `tiers`：新写法，带任意分界点 `tier.size`；
- `context_over_200k`：旧写法，字段名写死了 200k 分界。

上游为兼容旧客户端，把档位价格同时填进旧字段，例如 `302ai/gpt-5.4`：

```json
"tiers": [{ "input": 5, "output": 22.5, "tier": { "type": "context", "size": 272000 } }],
"context_over_200k": { "input": 5, "output": 22.5 }
```

`convert_tiers`（`modelsdev.rs:293-296`）只比较分界点：两个字段同时存在、且 tiers 里没有 size=200000 的档，就记「冲突」，不比较价格。2026-10-22 对 models.dev 公开接口做了全量统计（只读，脚本不入库）：

| 项目 | 结果 |
| --- | --- |
| 被判为冲突的模型 | 415 个，每个都只有一个 context 档 |
| 旧字段已给出的单价与首档全部相同 | 415 个（全部） |
| 首档分界点 | 272000：322；256000：56；200001：12；512000：10；272001：10；其他 5 |

所以这 415 条全是兼容字段，没有一条是价格矛盾。现有计价采用 tiers，这是对的：如果按旧字段的 200k 分界计价，上下文在 200k 到 272k 之间的请求会被多算。要修的只是告警分类。

## 2. 范围与不做

- **做**：
  - models.dev 与 OpenRouter 同步告警的收集、分类汇总；
  - 手动、自动两条同步路径的日志调用。
- **不做**：
  - 不改同步语义：快照内容、条数、拒绝规则、失败判定都不变；
  - 不改前端契约 `SyncOutcome`，也不在界面展示告警；
  - 不改价格索引、外置价格表 `model_policy` 等其他告警。这些来源条目数少，本机日志里没有刷屏记录。
- 不新增日志依赖，不改日志文件格式与滚动规则。

## 3. 必须保持的不变量

1. **明细不丢**：以 `RUST_LOG=debug` 运行时，每条原始告警仍逐条出现，并含来源、模型 id 与原因。除兼容字段（不变量 8）改用新的描述文本外，其余告警的文本与修改前相同。
2. **默认级别有界**：在 info 级别，一次同步每个来源每类告警至多一条 WARN。WARN 的条数只取决于告警类别数，与模型数无关。
3. **每条告警只记一次**：任何一条告警都只由一个调用点写入日志；核心库同步函数内部不再写告警日志。
4. **汇总可排障**：每条汇总 WARN 都写明来源、类别、受影响的模型数与告警条数，以及按字典序取的前 3 个模型 id 作为示例。同一输入得到的输出相同。
5. **手动、自动一致**：两条同步路径对同一批告警输出相同的汇总。自动同步以前不读 OpenRouter 告警，改后与手动同步一样各记一次。
6. **无告警不出 WARN**：没有告警时只有原有的 INFO 成功行。
7. **同步结果不变**：同一份上游响应得到的快照逐字节相同，`count` 不变，`openrouter::SyncReport.warnings` 的内容与顺序不变。
8. **兼容字段不算冲突**：两个字段同时存在且 tiers 里没有 size=200000 的档时，按单价是否一致分两种情况：
   - **一致**：旧字段里给出的每一项单价（input、output、cache_read、cache_write 中出现的项），都与分界点最小的 context 档对应项完全相等。这种情况属于兼容，只写 debug 明细，不进入 WARN 汇总。
   - **不一致**：任一项不等，属于真冲突，保留原文本「context_over_200k 与 tiers 冲突，优先 tiers」，进入 WARN 汇总。

   两种情况的计价都只用 tiers，分段结果与修改前相同。

## 4. 任务

### LA01 失败测试

先写测试并确认红灯，失败原因必须是断言本身：

- `modelsdev::sync_with` 使用含 `context_over_200k` 冲突与重复档位的合成响应时，`SyncReport` 带回对应的结构化告警（现状无此字段，断言红）。
- 汇总函数（纯函数，输入告警列表，输出待写的汇总行）：
  - 按类别分组，每类一行；
  - 写明模型数与告警条数；
  - 示例按字典序取前 3 个，超出时注明「等 N 个」；
  - 输入为空时输出为空。
- 同一模型的多条同类告警（例如 OpenRouter 一个模型被跳过 6 个时间条件档）按 1 个模型、6 条告警计数。
- 同一份响应在改动前后的快照逐字节相同（不变量 7）。
- 兼容与真冲突分类（不变量 8）：
  - 旧字段单价与首档全部相同时，不产生冲突类告警，只产生兼容类明细（现状会产生冲突告警，断言红）；
  - 任一项单价不同时，产生冲突类告警；
  - 旧字段只给出部分单价时，只比较已给出的项；
  - 上述情况的 `segments` 与修改前相同。

### LA02 收集与分类

- 告警携带稳定的类别键，加上模型 id 和原有文本。类别键在产生告警的地方确定，不靠解析文本猜测。
- models.dev：
  - `convert_tiers` 的 4 类告警各有类别，`context_over_200k` 一类按不变量 8 再拆为兼容（只写 debug）与冲突（进入 WARN 汇总）；
  - `sync_with` 不再写日志，`SyncReport` 新增告警字段。
- OpenRouter：
  - 时间条件档跳过、非法单价整条拒绝、结构异常等现有原因各有类别；
  - `sync_with` 内部不再写日志；
  - `SyncReport.warnings` 的字符串内容保持不变（不变量 7），结构化类别另加字段。

### LA03 统一记录

- 核心库提供一个记录函数：
  - 对每条汇总行写 `log::warn!`；
  - 对每条原始告警写 `log::debug!`，文本与修改前一致。
- `commands.rs` 的手动同步与 `lib.rs` 的自动同步，对两个来源都只调用这一个函数，删除 `commands.rs:1582-1584` 的逐条循环。
- 逐条检查两条同步路径上其他在循环里调用 `log::warn!` 的地方，结论写进执行记录。

### LA04 文档与交付

- `docs/stats-semantics.md` 若有同步告警的描述，同步修改。
- 版本按产品改动递增 patch。
- 本地取证：用开发版手动同步一次，统计这次同步的 WARN 条数，并按类别列出；再以 `RUST_LOG=debug` 同步一次，确认明细条数与 §1.1 同量级。只输出计数与类别，不贴模型列表全文。

## 5. 验收

- LA01 全部用例先红后绿；`cargo test --locked --workspace` 两套（普通与 acceptance）通过，clippy、fmt 干净。
- 本地取证：
  - 默认级别下单次同步的 WARN 条数不超过告警类别数：models.dev 按当前数据预期为 0（415 条全为兼容），OpenRouter 为个位数；
  - debug 级别下明细齐全；
  - 两项结果都写进执行记录。
- 必需检查（`gate`、`native-ui`）与 Install test 全绿后交用户复核，合并由用户决定。

## 6. 执行记录

**实施完成（2026-10-22，分支 `fix/sync-log-aggregation`，[PR #37](https://github.com/ywltby/tokenscope/pull/37) 已合并 `0fabf24`，v0.5.11 已发布）。** 提交：`97ab5cc`（计划转实施中）、`2668085`（实现主体）、`a944a22`（统计口径文档）、`582cf47`（版本 0.5.11）、`e6b5c93`（执行记录回填）、`0fabf24`（PR #37 合并）。

### 交付形态

- 新增核心库 `sync_diag` 模块：`SyncWarning { category, model, detail, for_summary }`（类别键在产生告警的位置确定）；`summarize` 纯函数按类别分组输出汇总行（`{source} 同步告警汇总[{category}]: 模型 N 个 / 告警 M 条，示例: a、b、c（等 N 个）`，类别与示例均按字典序、BTree 确定性；初版措辞「同步丢弃数据汇总」经用户复核指出冲突类并未丢弃数据，随 v0.5.12 改为中性表述）；`log_sync_warnings` 每类一条 `log::warn!`、每条原始告警一条 `log::debug!`（明细文本与改造前逐条 warn 一致）。
- models.dev：`convert_tiers` 返回 `Vec<TierWarning>`；`context_over_200k` 按不变量 8 分类（`given_rates_match` 只比较旧字段已给出的单价项、浮点 `==`）：全等为 `over200k_compatible`（`for_summary=false`，只 debug），任一项不等或首档缺失对应项为 `over200k_conflict`（原文本保留）。`sync_with` 不再写日志，`SyncReport.warnings` 携带结构化告警。
- OpenRouter：`sync_with` 经 `push_warning` 把告警平行写入既有 `warnings: Vec<String>`（内容与顺序不变）与新增 `diagnostics: Vec<SyncWarning>`；类别 `malformed_model` / `invalid_price_rejected` / `override_time_skipped` / `override_invalid_threshold` / `override_invalid_price` / `duplicate_override_threshold` / `base_price_rejected`（后两类为防御性产生点，真实响应未触发）。内部 warn 循环删除。
- 手动（`commands.rs`）与自动（`lib.rs`）同步各只调用一次 `log_sync_warnings`；`commands.rs` 原 1582–1584 的逐条 warn 循环删除；自动路径此前不读 OpenRouter 告警，现与手动一致（不变量 5）。

### 红绿与测试

- 先红后绿：8 条新测试在空桩（`summarize` 恒空、`SyncReport.warnings`/`diagnostics` 恒空、兼容分支未实现）下确认红灯，失败全部是断言本身——`summarize_*` 5 条（`lines.len()` 0≠期望）、`sync_report_carries_structured_warnings`（告警缺 prov-a/compat）、`sync_report_carries_parallel_diagnostics`（diagnostics 0≠4）、兼容分类 2 条中 `over200k_matching_first_tier_is_compatible_not_conflict` 与 `over200k_partial_rates_only_compare_given_items`（现状误报冲突）。
- 护栏在改动前即为绿：真冲突分类 2 条（价格不等、首档缺项）断言原文本与原分类；两个 golden 字节测试（`golden_snapshot_bytes_unchanged`、`golden_snapshot_bytes_and_warnings_unchanged`）用同一份合成响应把**改动前实现**的快照字节固定为基准 fixture（`synced_at` 归一为 `SYNCED_AT` 占位符），实现后逐字节相同——不变量 7。OpenRouter 的 `warnings` 内容与顺序也由 golden 测试固定断言。
- `cargo test --locked --workspace` 普通与 acceptance 两套全绿（普通 290+ 个测试文件全 ok，acceptance 291 ok），`cargo clippy --locked --workspace --all-targets`（含 `--features acceptance -- -D warnings`）与 `cargo fmt --check` 干净。pre-commit 钩子按暂存区完整跑过（含前端 vitest 772 条、E2E 四场景、310 条脚本测试）。
- 既有测试类型适配 1 处：`convert_tiers` 返回类型变化使 `test_modelsdev_tier_conversion_rules` 的 `warnings[0].contains("time")` 改为 `warnings[0].text.contains("time")`，断言语义（条数与文本内容）未删改、未放宽。

### 两条同步路径的循环 warn 排查（LA03 结论）

| 位置 | 结论 |
| --- | --- |
| `commands.rs` 手动同步 OpenRouter 逐条循环（原 1582–1584） | 本次删除 |
| `lib.rs` 自动同步「价格索引重建警告」循环 | 保留——属计划 §2 范围外（价格索引告警），本机日志无刷屏记录 |
| `modelsdev.rs` `parse_snapshot_text` 的 v1 快照降级 warn | 保留——加载路径单条事件，不在同步循环 |
| `pricing.rs` 加载告警（外置表/model_policy 等） | 保留——加载路径非同步路径，同上范围外 |
| `lib.rs`/`commands.rs` 其余 warn | 单条降级/失败事件，符合规约，保留 |

### 本地取证（隔离程序）

计划 §4 原定「用开发版手动同步一次」；改用一次性隔离程序（`qa-artifacts/sync-log/probe/`，不入库）：直接调用核心库 `modelsdev::sync` / `openrouter::sync` 拉真实公开源，快照写 `qa-artifacts/sync-log/tmp/`（不触碰真实 `~/.tokenscope`），并按壳两条路径的接线方式调用统一记录函数，用计数 logger（独立进程内 `log::set_logger`）统计 WARN/debug。改为隔离程序的原因：开发版手动同步会覆盖用户真实 `~/.tokenscope` 的价格快照并写入真实日志文件，且 GUI 操作无法程序化断言级别与条数；隔离程序同一代码路径可复跑、产物可复核。开发版的人工复核由用户在 PR 转 Ready 后进行（联网走 `HTTP_PROXY/HTTPS_PROXY=127.0.0.1:7897`）。

| 指标 | 修复前（2026-10-22 实测） | 修复后（取证实测） | 预期 |
| --- | --- | --- | --- |
| models.dev 默认级别 WARN | 415（逐模型误报冲突） | **0** | 0 ✓ |
| models.dev debug 兼容明细 | — | **415** | 约 415 ✓ |
| models.dev 快照条数 | 8734 | 8735（上游 +1） | 同量级 ✓ |
| OpenRouter 默认级别 WARN | 46（23 条 × 写两遍） | **2**（`override_time_skipped` 1、`invalid_price_rejected` 1） | 个位数 ✓ |
| OpenRouter debug 明细 | — | **23** | 约 23 ✓ |
| OpenRouter 快照条数 | 461 | 461 | 一致 ✓ |

WARN 条数只取决于类别数（models.dev 全兼容不计、OpenRouter 2 类），与模型数无关（不变量 2）。取证程序与复跑说明留 `qa-artifacts/sync-log/`（被 `.gitignore` 忽略）；注意 `scripts/test_build_paths.py` 以 `rglob` 断言仓库树内只有根 `Cargo.lock`，probe 跑完必须删除其 `Cargo.lock`（本次已踩过并记录在案）。

### 其他偏离与补充

- OpenRouter 新增 `#[doc(hidden)] pub fn sync_with_body_for_tests`（与 modelsdev 同名辅助对称），供 golden 集成基准生成注入固定响应体。
- `docs/stats-semantics.md` §4 补两条口径：`context_over_200k` 兼容/真冲突判定与同步告警的收集记录方式。
- 版本按产品改动递增 patch 至 0.5.11，`bump_version.py --check --base-ref origin/main` 输出 `release: true`。

### 归档时后续动作（2026-10-23）

- v0.5.11 发布验收通过后登记升级基线（`upgrade_baselines.py add --tag v0.5.11`，同批归档提交）。
- 用户复核的汇总行措辞问题随 v0.5.12 修正：`summarize` 行前缀「同步丢弃数据汇总」→「同步告警汇总」（产品 WARN 文本，按规则递增 patch 发布），OpenRouter 逐条明细的历史前缀「OpenRouter 同步丢弃数据: 」按不变量 1 保持不变；取证程序的计数解析同步更新（qa-artifacts，不入库）。
