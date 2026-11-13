# 定价接入、CCS 来源费用与价格列表滚动修复（2026-11-13）

状态：实施中。用户已明确授权本批代码实现（2026-11-13 指令），停在可审查 PR，不自动合并/发布/安装。

## 背景（已核实的事实）

1. **claude-fable-5-2 缓存分档**：本机 OpenRouter 快照已有该模型四价（prompt 8e-6、completion 4e-5、cache_read 2e-7、cache_write 1e-5 USD/token，即 $8/$40/$0.20/$10 /M）；models.dev 快照（2026-11-07 同步）没有该模型（同步曾超时）。当前 `estimate_entry` 遇到「事件有可信 5m/1h 分档、候选只有合并写价」时把缓存写整体记为未计价（pricing.rs:1069-1083「候选未声明分档：写总量按未知处理」）——OpenRouter 有效价格被无效化。本机 124 条明细中 38 条带可信分档受影响。官方价（platform.claude.com/docs/en/about-claude/pricing）：输入 8、输出 40、5m 写 10、1h 写 16、缓存读 0.20。
2. **claude-opus-4-7 CCS 来源费用**：该桶只有 CCS 日汇总（无请求明细），`source_cost_usd` 已有非零值，但 `push_rollup_into`（aggregate.rs:486）统一置 `unknown_pricing=true`、费用不累计——费用展示/聚合规则问题，不是模型无价或 token 丢失。
3. **价格列表滚动**：相同内容刷新后 `pricing.entries` 对象被替换，`ScrollList` 只观察首行对象引用（ScrollList.vue:50-58），引用变化即恢复 200 行并 `scrollTop=0`；隔离浏览器已复现（刷新前 innerTop=1000 → 刷新后 0）。980×620 时列表固定 420px 导致外层溢出 233px，内层归零后滚轮接管外层。复现材料 `qa-artifacts/pricing-scroll-analysis/`（gitignored，正式回归不得依赖）。

## 任务

### T1 修复模型缓存分档定价（OpenRouter 有效候选接入分档）

- 在候选价格依据满足校验时派生 5m/1h 分档价：**OpenRouter 官方渠道**（id 前缀恰为 `anthropic/`）且发布价自证（`cache_write == 1.25 × prompt`，相对误差 1e-9，与 models.dev 的 Anthropic 派生同规则）→ 5m = cache_write、1h = 2 × prompt。不按模型名猜价、不跨渠道/候选/规则层拼接。
- Fable 5.2 缓存读价 0.20 是快照显式值，保持原值（不得套旧 0.1×input=0.80）。
- 合并写价存在但缺分档字段 ≠「网站没有缓存价」，不得按零处理。
- 四桶与分档不重复计入；已有分层解析（seg/time/base）语义不变。
- 合成回归：Fable 5.2 的 5m/1h 各 100 万 token → 写费 10 + 16 = 26；再加输入/输出/缓存读各 100 万 → 合计 74.20。覆盖模型等价写法（`claude-fable-5.2` / `claude-fable-5-2`）、分档为零、来源缺失、其他模型既有行为不变。

### T2 展示已有 CCS 来源费用

- 仅对来源选择算法最终纳入统计的日汇总（rollup_only 桶、B 类桶）使用其有效 `source_cost_usd`；A/C 类明细优先路径不追加汇总费用。
- 汇总费用**单独披露**（`Group.source_cost_usd` 新字段 + 汇总页说明），明确标识「CCS 来源记录费用」，不冒充 TokenScope 请求级估算或实际账单；请求级 breakdown 通道不变。
- 汇总、总计、各分组费用口径一致并保留来源依据；来源费用缺失时如实说明「来源未提供费用」（不套 T3 的按零规则）。
- 不伪造请求/项目/事件时间/分档；不改历史事实、导入边界、会话解析修订。
- 合成回归：两条来源费用 7.25 + 1.75 = 9；覆盖显式 0、NULL、重复导入、A/B/C 选择、筛选、重启一致性。真实用户金额/日期/库行/日志不进公开 fixture。

### T3 落实「网站缺失缓存价按 0 估算」产品规则

- 线上来源（models.dev/OpenRouter）确实未提供 `cache_read`/`cache_write` 单价时按 0 参与估算；原始快照缺失值保留，计算时记录「缺失缓存价按 0 估算」（与来源显式 0 区分）。
- 已有有效非零价不被零假设覆盖；不把缺失输入/输出价、整模型无匹配、非法价、溢出、规则阻断变成免费。
- 费用详情展示估算依据；不加采集警告、不反复弹通知。
- 完整候选优先级不变；按零估算不冒充完整官方价。价格索引失效随规则版本（`INDEX_VERSION` 10 → 11）自动重建派生缓存。
- 不修改会话解析修订、不要求清库/重扫/重导/手改价格文件。

### T4 修复价格列表滚动

- `ScrollList` 用行身份键（而非首行对象引用）判定内容变化：相同内容刷新保留可见行锚点、相对位置与已展开批次数量；内容真变（row-key 序列不同）才重置。
- 价格区按剩余窗口高度布局（980×620 无双层纵向滚动、滚轮不转交外层）；保持工具区、错误提示、列表可访问；不破坏其他分区、明细分页与真实筛选变化的重置规则。
- 真实浏览器验证（Chrome 隔离实例，非 happy-dom/正则）：滚动中刷新相同内容、更新单价、渐进追加、切走返回、连续滚轮遇刷新；1280×820、980×620、明暗主题；断言可见行位置稳定、无意外归零、滚轮不转交外层、导航可达。

## 执行与交付约束

- 每个缺陷先有行为红测再最小修复；不重写计价系统/来源选择/存储架构；不扩修 about:blank、/btw 等独立任务。
- 真实库与价格文件只读核验；测试全部用隔离合成数据；保留其他会话改动（stash@{0} 不触碰）；不新建 worktree；人工清理送回收站。
- 版本：本批含计价策略变化，按 minor 处理，从 main 基线（0.8.0）起 `bump_version.py` 分配，同一 PR 不重复递增；合入 main 前登记上一发布版升级基线（`upgrade_baselines.py add --tag v0.8.0`）。
- 完整检查：Rust 普通/acceptance 测试与 clippy、前端 typecheck/format/test/build、浏览器与 Native 契约、构建输入清单。正常提交推送建 PR；修复已定位的 CI 阻断；不绕过钩子、不放宽断言、不以重跑代替定位。
- 停在可审查 PR；不自动合并、发布或本机安装。

## 实施记录

### T1 OpenRouter 缓存写分档（红→绿）

- 红测：`openrouter_cache_tier_pricing.rs` 5 例先红（分档写费 0 而非 26；等价写法 0）。
- 修复：`openrouter_cache_write_tiers` 只对渠道前缀恰为 `anthropic` 且发布价自证
  （`cache_write == 1.25 × prompt`，相对误差 1e-9）的条目派生 5m/1h；非官方渠道/
  自证不通过保持分档未知。接入 `build_from_reads` 的 OpenRouter 基础价。
- 绿测：5m/1h 各 1M = $26；输入/输出/缓存读各 1M 合计 $74.20；等价写法命中；
  非官方渠道不派生；显式 0 缓存读保留；缺缓存价条目转入 T3 口径。
- 口径边界：`split_known` 且分档不可用时，**有**合并写价（Fixed）仍记未计价
  （N18「不用总量单价冒充分档」），**连合并价都没有**的线上候选才按 0（T3）。

### T2 CCS 来源费用（红→绿）

- 红测：`ccs_rollup_source_cost.rs` 11 例（7.25 + 1.75 = 9；显式 0；NULL；A/B/C；
  重复导入；日期筛选；按模型等价分组；重启；明细不受影响）。
- 修复：`Group.source_cost_usd` 新字段；`push_rollup_into` 只对来源选择最终采用的
  汇总贡献累计（rollup_only 与 B 类）；`finish()` 受检汇总到总计；非法值报错。
- 前端：汇总区「CCS 来源记录费用」披露（金额 + 不是请求级估算/账单说明）；
  来源缺失显示「来源未提供费用」；`UsageTable` 三例新增断言（79 项通过）。

### T3 线上缺失缓存价按 0 估算（红→绿）

- 红测：`assumed_zero_cache_pricing.rs` 6 例先红（编译缺 `RateKind::AssumedZero`，
  修复后覆盖：按 0 计入、显式 0 区分、非零价保留、完整候选优先、输入价缺失仍未知、
  外置 TOML 不套用）。
- 修复：`estimate_entry` 对 models.dev/OpenRouter 基础缓存合并价 Unknown 解析为
  `RateKind::AssumedZero`（金额 0、token 不进 unknown、complete=false）；
  分档档位缺价与合并价存在但缺分档保持 N18 未知；新增
  `CostEstimate/Group/EventCostBreakdown.cache_price_zero_estimated` 披露标志；
  前端费用明细行与结果行展示「缺失缓存价按 0 估算」，汇总卡新增独立胶囊。
- 索引：`INDEX_VERSION` 10 → 11（rules:v11），升级后自动按来源重建，无需清库。
- 既有测试更新：`partial_pricing` 中快照缺缓存价从「未计价 token」改为「按 0」；
  N18 三例边界保持通过。

### T4 价格列表滚动（红→绿）

- 红测：`ScrollList.test.ts` 新增 3 例（同 resetKey 刷新保留 400 行与 scrollTop；
  resetKey 变化重置；未提供 resetKey 保持旧语义）。
- 修复：`ScrollList` 新增 `resetKey`（内容身份 = 行键序列；价格更新/相同内容刷新
  不归零）；`Settings.vue` 价格区按剩余窗口高度计算列表上限（220–420px），
  窗口 resize/分区切换/重新读取后重算。
- 真实浏览器（Playwright，large-pricing fixture）新契约
  `pricing_scroll_refresh_preserves_position`：首批 200 → 渐进 400；锚点
  1500 → 同步在线价格重读后仍 1500；已展开批次不回退；980×620 列表底边
  596 ≤ 620；列表内滚轮外层 `scrollTop` 0→0。1280×820 / 980×620 × 明暗
  四组合全部通过。

### 交付状态

- 版本 `0.9.0`（minor，`release: true`）；`v0.8.0` 升级基线已登记（资产哈希以
  下载核对为准）。
- 待回填：完整本地门禁、PR 最新 HEAD 检查与证据。
