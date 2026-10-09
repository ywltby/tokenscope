# 模型名称统一分组与定价匹配实施计划

> 执行时使用 executing-plans 技能，按任务逐项实施、验证并记录结果。

**Goal:** 模型身份不区分 ASCII 大小写，直接删除 `-`、`.`、`_`；使 `claude-opus-5-5`、`claude-opus-5.5` 在按模型图表和聚合列表中合为一项，下钻包含全部等价请求，定价使用相同等价匹配规则。

**Architecture:** 在公共模型身份模块构建等价键，聚合、下钻与价格匹配共用；原始模型名不覆盖。models.dev 的可信模型级 name 作为展示元数据，不能充当身份或价格键。费用逐请求计算后汇总；前缀回退仅用于定价，不能用来合并不同模型版本。

**Tech Stack:** Rust、serde、现有价格索引与查询快照；Vue/TypeScript 图表、聚合表及下钻标签。

**状态：** 待确认实施；本次只编写计划。2026-10-10 根据用户补充，将范围从仅定价扩为模型分组、展示和定价统一。基线 `ec23d4a`，实施前核对最新代码与版本号。

## 1. 现状与范围

- `src/pricing.rs::normalize_model_id` 当前为 trim + ASCII lowercase + `.` 转 `-`；下划线与省略分隔符尚不等价。
- `match_key` 提取最后一个 `/` 后的模型名称；`collect_candidates` 依次执行完整匹配、变体回退完整匹配、有边界前缀匹配、变体回退前缀匹配。
- `by_prefix` 的每个键保留多个候选；`apply_model_policies`、`entries`、价格索引也依赖这个键。
- 当前 `INDEX_VERSION = 8`，前端 `SNAPSHOT_VERSION = 8`。版本应在实施时按实际基线递增。

目前 `src/aggregate.rs` 按 `e.model` 原文分组，`src/report.rs` 下钻按原文精确比较，必须同时修改。`src/modelsdev.rs::ApiModel.name` 是模型显示名，但同步当前使用 `m.name.or(entry.name.clone())`，快照 name 可能实际是供应商名称。

范围包括模型维度分组、图表/表格显示、下钻过滤、定价匹配及派生快照失效。原始日志、请求明细中的原始模型名、事件去重身份保持不变。不新增别名字典或模糊拼写纠错；`opus55` 与 `claudeopus55` 不因名称相似而自动等价。普通输入、缓存写入、缓存命中继续分别计费。

## 2. 设计决策与不变量

1. 推荐单一等价键。仅追加一层宽松兜底会使同一模型不同写法获得不同候选，不满足本需求；直接删除所有符号再做前缀匹配则会丢失边界，均不采用。
2. 先解析 `/` 渠道和 `:` 变体结构，再对模型基名和变体各自 lowercase、移除 ASCII `-._`。`/`、`:` 必须保留语义；渠道身份和 `model_policy.channel/source` 限定不参与删符号归并。外围空白沿用 trim；内部空格及其他字符不新增等价规则。
3. 模型基名为空或删符号后为空（例如 `-._`、`provider/---`）不得成为通配价格；拒绝该条目并给现有诊断，查询返回未收录。显式冒号后的空变体也不得悄悄变成基础模型。
4. `opus-5-5`、`opus5.5`、`Opus5_5` 的完整匹配键均为 `opus55`。完整等价匹配优先于任何前缀；不能因为价格条目与查询恰好原文相同就排除其他等价候选。
5. 保留变体隔离：`opus55:free` 不能成为普通 `opus55` 的候选，`free` 与 `thinking` 不等价。带变体查询无精确候选时，沿用显式标记的基名回退。
6. 前缀回退保留查询的结构化边界：记录原名称分隔符对应的压缩键偏移，仅允许在这些位置截断基名，再按现有四阶段收集候选。例如 `opus55-20261010` 可回退到 `opus-5-5`；`opus550` 不得因 starts_with 命中 `opus55`，`gpt-50` 不得命中 `gpt-5`。不猜测无分隔符的后缀边界。此限制只针对前缀回退，不影响完整等价匹配。
7. 多个原名称归一相同仍保留各自完整 PricePlan、来源、渠道；沿用现有完整候选优先、逐候选按请求上下文计算并选择最高费用及稳定并列裁决。不得后写覆盖，不得从多个候选拼四项单价。排序中的前缀长度应使用等价键长度，不能让多写分隔符获得优势。
8. 原始名称仍用于显示与追溯，breakdown 保留实际候选、模式和候选数。模型政策按等价模型键命中，但显式价格不被政策覆盖。
9. 费用规则变化必须使价格索引和旧费用视图失效；不修改事件解析口径，不升级事件 SQLite schema，不重扫或修改用户源日志。新查询采用新规则，已有不可变查询快照不混入新价格。
10. 公共 `model_identity_key` 对模型原标识转小写并删除三种符号，保留 `/` 命名空间和 `:` 变体。聚合只合并完整身份相同的事件，不额外剥除模型前缀或命名空间。价格匹配在同一转换规则上继续取末段（现有跨渠道价格候选规则）；渠道元数据保留原文。分组键与价格末段键层次不同，但删符号规则只有一份。
11. `claude-opus-5-5` 与 `claude-opus-5.5` 的分组 key 都为 `claudeopus55`。大小写/符号不同的跨应用事件也合并；`:free`、`:thinking`、不同日期后缀或真正不同的版本保持不同身份。即使它们前缀回退命中同一价格，也不能因此合并用量。
12. `Group.key` 是稳定身份；`Group.label` 是展示名。优先选完全等价 ID 对应的可信 models.dev 模型 name；多个 provider 的 name 不一致时按原始 ID、name 稳定排序选一，不按本次计费胜出的渠道选名，不随事件输入顺序变化。仅前缀匹配到的旧版本 name 不可冒充本模型。没有可信 name 时从该组原始模型名按稳定顺序选代表写法，不能显示压缩成一串的 key。两个不同 ID 即使 name 相同也不合并。
13. 名称元数据随查询快照冻结。价格同步可改变后续新查询的 label，但不能改变身份、正在分页的结果集合或计费选择。无价格/离线时仍能按身份合并。
14. 合并只改变分组：请求数和四项 token 守恒；每个请求按自身时间、上下文分段和原始模型匹配后的候选计算费用，分组费用为这些请求费用之和。不能先合并 token 再触发上下文价格档位，也不能因名称等价去重真实请求。

## 3. 任务清单

### MP01：等价键与边界解析

**文件：** 新增 `src/model_identity.rs`，在 `src/lib.rs` 注册；`src/pricing.rs`（normalize_model_id、match_key、split_variant 及测试）。

- [ ] 先写 `model_equivalence_key_examples`、`model_equivalence_preserves_structure`、`model_equivalence_rejects_empty_base`、`model_equivalence_boundary_offsets`，验证三种名称、混合大小写、连续符号、渠道、变体、空键、中文以及 UTF-8 安全边界。
- [ ] 执行 `cargo test model_equivalence`，记录新行为的失败断言。
- [ ] 实现专用结构，分别保留原文、等价基名/变体键和基名合法前缀偏移。等价片段函数可以是 `s.chars().filter(|c| !matches!(c, '-' | '.' | '_')).map(|c| c.to_ascii_lowercase()).collect::<String>()`；结构分割必须先于片段转换。
- [ ] 复跑上述测试；审查所有 normalize_model_id 调用，避免把显示名、渠道元数据也压缩。
- [ ] 独立提交：`feat(pricing): 统一模型名称等价键并保留边界`。

### MP02：候选匹配与冲突处理

**文件：** `src/pricing.rs`（Entry、by_prefix、collect_candidates、tie_rank、estimate、apply_model_policies、entries）；新增 `tests/model_name_equivalence.rs`。

- [ ] 先写集成测试 `model_equivalence_three_spellings_same_candidates_and_cost`，对查询与价格表三种拼写做 3×3 交叉组合，核对完整匹配、候选、分项单价、金额，而非仅断言 lookup 非空。
- [ ] 增加 `model_equivalence_full_precedes_prefix`、`model_equivalence_prefix_keeps_boundary`、`model_equivalence_variant_isolation`，覆盖 §2 正反例及原有带日期/后缀模型。
- [ ] 增加 `model_equivalence_collisions_keep_all_candidates`、`model_equivalence_candidate_order_independent`、`model_equivalence_policy_respects_channel`：外置、models.dev、OpenRouter 的等价条目同时存在，交换加载顺序仍选同一完整候选；覆盖不完整价格、显式零、上下文分段及变体回退。
- [ ] 运行 `cargo test --test model_name_equivalence`，记录失败后再接线所有入口；保留候选 Vec，不用单值 HashMap 替代。
- [ ] 审查 `entries()` 的 OpenRouter 对照价：当前单值 HashMap 会丢弃同键条目。对照候选选择必须稳定且取自一条完整记录；可沿用明确的稳定排序择一，并显示该候选原名称，不能展示成唯一价格或拼接分项。测试 `model_equivalence_price_view_comparison_is_deterministic` 核对交换输入顺序后视图一致。
- [ ] 复跑集成测试及 `cargo test pricing`；独立提交：`fix(pricing): 等价名称共享候选且保持变体与前缀边界`。

### MP03：可信模型展示名

**文件：** `src/modelsdev.rs`（ApiModel、Snapshot、sync_with）、`src/pricing.rs`（模型元数据查询及索引）、新增 `tests/model_identity_contract.rs`。

- [ ] 先写 `model_display_name_uses_model_not_provider`、`model_display_name_is_not_identity`、`model_display_name_is_order_independent`、`model_display_name_requires_full_id_match`、`model_display_name_legacy_snapshot_falls_back`，验证 name 缺失、供应商兜底、同名不同 ID、等价 ID 不同 name 和离线旧快照。
- [ ] 执行 `cargo test model_display_name` 记录失败后再实现。模型同步不再把 provider name 填为模型 name；将 models.dev 快照由当前 v3 递增至 v4，明确 v4 name 仅来自模型字段。v1–v3 继续离线读价格，但不把来源不明的 name 用作聚合名称；无需强制联网升级。
- [ ] 在价格/目录对象提供只读展示名解析器，与价格赢家选择独立；同一等价 ID 下按稳定原始 ID/name 顺序选取，未命中返回 None，由聚合稳定选取代表原文。
- [ ] 如需保留无 cost 模型的 name，用单独可选元数据集合纳入快照；不得为获得名称而制造零价 PricePlan。明确是否纳入本轮：本轮允许无 cost 时退回原始代表名，不阻塞身份合并。
- [ ] 复跑测试；独立提交：`fix(modelsdev): 区分模型展示名与供应商名称`。

### MP04：模型分组、下钻与前端名称一致

**文件：** `src/aggregate.rs`、`src/report.rs`、`src/query.rs`（下钻指纹）、`tests/model_identity_contract.rs`、`frontend/src/types.ts`、`frontend/src/views/Dashboard.vue` 及其测试、`frontend/src/components/UsageTable.vue` 及其测试、`frontend/src/lib/chartData.ts`/`chartTooltip.ts` 及其测试。

- [ ] 先写 `model_identity_groups_aliases_across_agents`、`model_identity_preserves_usage_and_requests`、`model_identity_keeps_versions_and_variants_separate`、`model_identity_group_cost_sums_per_request`、`model_identity_drill_pages_include_all_spellings`。超过 200 条合成数据跨页混合两种名称，断言不漏不重、金额与汇总一致；以跨上下文价档和不同时间档请求验证逐请求计费。
- [ ] 执行 `cargo test --test model_identity_contract` 记录失败后，再让聚合使用公共身份键和稳定 label；下钻事件比较及游标筛选指纹使用同一身份规则。不覆盖 UsageEvent.model 或 EventRow.model，不调整 dedupe 的模型字段。
- [ ] 前端统一用 label 显示图表、聚合首列与筛选标签，传 key 查询；查明图表 tooltip 当前可能优先展示 rawKeys，模型维度应显示友好 label，原始名称仍在请求明细可查。不得把 `Claude Opus 5.5` 这样的 name 当后端筛选 key。
- [ ] 新增前端 `model_group_label_and_drill_key_are_distinct`、`model_chart_uses_friendly_label`，核对图表与表格只出现一项、点击行请求传 canonical key、筛选提示不出现压缩键。
- [ ] 运行上述 Rust 测试与 `pnpm --dir frontend test -- src/views/Dashboard.test.ts src/components/UsageTable.test.ts src/lib/chartData.test.ts src/lib/chartTooltip.test.ts`；独立提交：`feat(model): 合并等价模型用量并统一下钻身份`。

### MP05：索引、价格修订与视图失效

**文件：** `src/pricing.rs`（INDEX_VERSION、索引序列化/恢复、来源签名）、`tests/pricing_index_restart.rs`、`frontend/src/lib/viewSnapshot.ts`、`frontend/src/lib/viewSnapshot.test.ts`、`frontend/src/views/Dashboard.test.ts`。

- [ ] 先写 `model_equivalence_old_index_rebuilds_offline`、`model_equivalence_restart_matches_cold_build`、`model_equivalence_revision_changes_with_rules`；所有价格源、索引及缓存路径注入临时目录。
- [ ] 用含旧键的 v8 合成索引验证失败；价格来源字节保持不变，升级后也必须重新建立候选索引，价格修订值必须包含匹配规则版本。旧索引失效时从本地价格快照离线重建，不强制联网。
- [ ] 递增价格索引版本（当前预计 8 → 9）；索引恢复必须恢复边界所需信息，或从保留的原始 display 重建，不能从丢失符号的键反推边界。
- [ ] 递增前端费用视图快照版本（当前预计 8 → 9）；新增 `old_matching_rules_snapshot_is_ignored`，确认启动时不闪现旧匹配规则算出的金额，首次新响应后正常保存/复用。
- [ ] 运行 `cargo test --test pricing_index_restart`、`pnpm --dir frontend test -- src/lib/viewSnapshot.test.ts src/views/Dashboard.test.ts`；独立提交：`fix(cache): 使旧模型匹配索引与费用视图失效`。

### MP06：文档与最终验收

**文件：** `docs/stats-semantics.md` §4、`CLAUDE.md` 费用估算说明、本计划及 `docs/plans/README.md`。

- [ ] 更新等价身份分组、展示名来源、下钻规则、定价边界回退、冲突保留和版本迁移说明，核对索引版本陈述与代码；明确保留原始请求模型名、去重规则和各 token 分项计费。
- [ ] 执行门禁：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`；壳分别用 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`cargo test --manifest-path src-tauri/Cargo.toml`。
- [ ] 前端执行 `pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`、`pnpm --dir frontend test`、`pnpm --dir frontend build`。
- [ ] 隔离 fixture 验收：图表和聚合表的两种 Claude Opus 拼写合为一项，展示可信 models.dev name；下钻展示两种原始名称且分页完整。同一单价、相同 token/时间/上下文的三种拼写分别取得同金额；聚合与明细金额一致；费用浮层显示真实候选及原始名称。无价格、缺部分价格和免费变体不能变成假零价。
- [ ] 冷启动和索引热恢复各执行一轮；产物放 `qa-artifacts/model-name-equivalence/`，不得存真实会话日志。未跑的原生场景明确标待验，不以合成 IPC 冒充真实后端。
- [ ] 回写测试数、命令退出码、提交及验收证据；独立提交文档。全部必需验收完成后再归档计划。

## 4. 完成标准

两种 Claude Opus 拼写在按模型图表和聚合表中合并，下钻包含全部等价请求，原始明细名称可追溯。三种拼写在任意价格来源都识别为同一模型候选集合；完整等价、前缀边界、变体隔离、重复候选、政策覆盖、逐请求计费、展示名兜底、离线索引恢复和旧视图快照失效均有真实断言。费用仍由后端按互斥 token 桶逐请求计算，用户日志不变。不得仅凭现有测试全绿勾选完成。
