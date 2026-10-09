# 模型定价名称等价匹配实施计划

> 执行时使用 executing-plans 技能，按任务逐项实施、验证并记录结果。

**Goal:** 定价识别不区分 ASCII 大小写，忽略模型名称中的 `-`、`.`、`_`，使 `opus-5-5`、`opus5.5`、`Opus5_5` 命中相同候选集合。

**Architecture:** 在价格层统一构建等价匹配键，保留原始名称、渠道与变体结构。完整匹配使用等价键，前缀回退另存分隔符边界，不能直接对删符号后的字符串做任意 starts_with。所有价格来源、策略覆盖、索引恢复与查询共用规则。

**Tech Stack:** Rust、serde、现有价格索引；Vue/TypeScript 仅处理旧费用视图快照失效。

**状态：** 待确认实施；本次只编写计划。基线 `ec23d4a`，实施前核对最新代码与版本号。

## 1. 现状与范围

- `src/pricing.rs::normalize_model_id` 当前为 trim + ASCII lowercase + `.` 转 `-`；下划线与省略分隔符尚不等价。
- `match_key` 提取最后一个 `/` 后的模型名称；`collect_candidates` 依次执行完整匹配、变体回退完整匹配、有边界前缀匹配、变体回退前缀匹配。
- `by_prefix` 的每个键保留多个候选；`apply_model_policies`、`entries`、价格索引也依赖这个键。
- 当前 `INDEX_VERSION = 8`，前端 `SNAPSHOT_VERSION = 8`。版本应在实施时按实际基线递增。

目标是修复定价识别。原始日志、模型显示名、模型维度分组、下钻过滤和事件去重身份不改；同名归组不是本计划范围。不新增模型别名字典、模糊拼写纠错或供应商前缀猜测。普通输入、缓存写入、缓存命中继续分别计费。

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

## 3. 任务清单

### MP01：等价键与边界解析

**文件：** `src/pricing.rs`（normalize_model_id、match_key、split_variant 及测试）。

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

### MP03：索引、价格修订与视图失效

**文件：** `src/pricing.rs`（INDEX_VERSION、索引序列化/恢复、来源签名）、`tests/pricing_index_restart.rs`、`frontend/src/lib/viewSnapshot.ts`、`frontend/src/lib/viewSnapshot.test.ts`、`frontend/src/views/Dashboard.test.ts`。

- [ ] 先写 `model_equivalence_old_index_rebuilds_offline`、`model_equivalence_restart_matches_cold_build`、`model_equivalence_revision_changes_with_rules`；所有价格源、索引及缓存路径注入临时目录。
- [ ] 用含旧键的 v8 合成索引验证失败；价格来源字节保持不变，升级后也必须重新建立候选索引，价格修订值必须包含匹配规则版本。旧索引失效时从本地价格快照离线重建，不强制联网。
- [ ] 递增价格索引版本（当前预计 8 → 9）；索引恢复必须恢复边界所需信息，或从保留的原始 display 重建，不能从丢失符号的键反推边界。
- [ ] 递增前端费用视图快照版本（当前预计 8 → 9）；新增 `old_matching_rules_snapshot_is_ignored`，确认启动时不闪现旧匹配规则算出的金额，首次新响应后正常保存/复用。
- [ ] 运行 `cargo test --test pricing_index_restart`、`pnpm --dir frontend test -- src/lib/viewSnapshot.test.ts src/views/Dashboard.test.ts`；独立提交：`fix(cache): 使旧模型匹配索引与费用视图失效`。

### MP04：文档与最终验收

**文件：** `docs/stats-semantics.md` §4、`CLAUDE.md` 费用估算说明、本计划及 `docs/plans/README.md`。

- [ ] 更新等价匹配、边界回退、冲突保留和版本迁移说明，核对现有文档中的索引版本陈述与代码；明确不改变模型展示/聚合身份和各 token 分项计费。
- [ ] 执行门禁：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test`；壳分别用 `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`cargo test --manifest-path src-tauri/Cargo.toml`。
- [ ] 前端执行 `pnpm --dir frontend typecheck`、`pnpm --dir frontend format:check`、`pnpm --dir frontend test`、`pnpm --dir frontend build`。
- [ ] 隔离 fixture 验收：同一单价、相同 token/时间/上下文的三种拼写分别取得同金额；聚合与明细金额一致；费用浮层显示真实候选及原始名称。无价格、缺部分价格和免费变体不能变成假零价。
- [ ] 冷启动和索引热恢复各执行一轮；产物放 `qa-artifacts/model-name-equivalence/`，不得存真实会话日志。未跑的原生场景明确标待验，不以合成 IPC 冒充真实后端。
- [ ] 回写测试数、命令退出码、提交及验收证据；独立提交文档。全部必需验收完成后再归档计划。

## 4. 完成标准

三种拼写在任意价格来源都识别为同一模型候选集合；完整等价、前缀边界、变体隔离、重复候选、政策覆盖、离线索引恢复和旧费用快照失效均有真实断言。费用仍由后端按互斥 token 桶计算，用户日志与模型原始显示名不变。不得仅凭现有测试全绿勾选完成。
