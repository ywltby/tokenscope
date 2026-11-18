# v0.9.2 汇总页费用显示与说明入口收口（2026-11-18）

> 用户指令：移除 `†` 与独立未知列、用颜色区分费用完备性、把表后两组说明移到「聚合」标题浮层。
> 本批只改展示与说明入口；金额、请求数、token、计价规则、采集行为不变。

## 目标与不变量

1. **金额事实不变**：`total_cost_usd`、`cost_usd`、来源费用、`unknown_tokens` 等仍由后端给出；前端不重算、不猜费率。零价仍显示 `$0.00`，缺价 ≠ 免费。
2. **交互契约不变**（DESIGN.md §6 / RC06）：费用触发器与「聚合」标题触发器都用原生 `button type=button`，hover / focus / click 固定三态，Escape 与固定态外点关闭，`aria-describedby` 只在打开时指向真实节点。
3. **无备注符号**：界面不再出现 `†`（含标题、金额后缀、「未知†」与中英文文案）；不替换成星号、图标或胶囊。
4. **两栏只移动**：「来源采集统计」与「统计说明」从表格后移入「聚合」标题浮层，各保留小标题与内容；原有可操作项保留。其他说明（表格提示、通知条）不动。

## 显示规则（颜色）

统一纯函数判定，费用列、合计行与总览卡同源：

| 状态 | 条件 | 显示 |
| --- | --- | --- |
| 全部未定价 | `total_cost_usd == null` | 橙色「未知」 |
| 部分未定价 | `total_cost_usd != null` 且 `unknown_tokens` 四桶之和 > 0 | 橙色金额（如 `$2.00`） |
| 无未定价用量 | `unknown_tokens` 四桶之和 = 0 | 普通文字色；显式零价 `$0.00` |
| 仅按零估算标记 | 同上（`cache_price_zero_estimated=true` 但 unknown=0） | 普通文字色 |

橙色统一用现有 `--ts-warning` 语义 token（亮色 `#B25000`、暗色 `#FF9F0A`，DESIGN.md §2 已按 WCAG AA 校核），不新增色值。

## 费用浮层内容

沿用 `GroupCostBreakdownTooltip.vue`：结果区（本行费用单一金额）→ 构成区（请求估算、来源记录费用及其覆盖完整性、分档/按零估算说明）→ 未获费用覆盖区（未定价 token 合计 + 分项）。新增「可用的缺价原因」：`unknown_reason === "rollup_only"` 时显示既有 `dashboard.rollupOnly` 文案；无专门原因时沿用「无匹配价格或分项缺价」。浮层不显示 `†`。

## 聚合标题浮层

新增共享组件 `AggregateInfoTooltip.vue`，由 `UsageTable.vue` 卡头「聚合」标题挂载：同一浮层内两段小标题（`sourceStats` / `selectionStatsNote`）及内容，数据复用现有 `sourceLines` / `selectionNotes` 计算。浮层宽度 ≤ 480px、高度 ≤ 60vh，内容超长时**浮层内部**滚动；鼠标可从标题移入浮层（浮层自身维持 hover 态），键盘 Tab / Enter / Space / Escape 可用。

## 涉及文件

- 新增：`frontend/src/lib/costDisplay.ts`、`frontend/src/components/AggregateInfoTooltip.vue`
- 组件：`CostCell.vue`、`SummaryCards.vue`、`GroupCostBreakdownTooltip.vue`、`UsageTable.vue`
- 文案：`locales/zh-CN.ts`、`locales/en-US.ts`（去掉脚注符号与无用列键）
- 文档：`DESIGN.md`、`docs/stats-semantics.md`、本计划
- 回归：组件单测、浏览器契约 `interaction-contracts.mjs` 与夹具、原生 `native-pricing-checks.mjs` 及其单测
- 版本：产品输入变化 → `bump_version.py --bump patch`（0.9.2 → 0.9.3），不合并、不发布、不安装

## 验收矩阵

普通金额 / 部分未定价 / 全部未定价 / 显式零价 / 仅按零估算 × 浅色 + 深色 × 1280×820 + 980×620；两个浮层分别验证内容归属、鼠标移入、键盘与 Escape；表后无重复栏目；组件、浏览器、Native 三层断言各自实跑。

## 实施与验证记录（2026-11-18）

- **实现**：新增 `lib/costDisplay.ts`（金额/色态单一来源）与 `AggregateInfoTooltip.vue`；
  费用浮层按 `unknown_tokens` 四桶之和上警告色、补缺价原因、移除脚注；`UsageTable`
  删除未知列与表后两组栏目，标题挂共享浮层；`SummaryCards` 去掉标题旁的短标记；
  中英文资源去掉脚注符号与无用列键，新增标题浮层可访问名称。
- **组件**：81 文件 / 1165 项全绿（含新增 `AggregateInfoTooltip.test.ts` 4 项、
  `costDisplay.test.ts` 5 项、费用浮层新增原因断言）。
- **浏览器**：`pnpm --dir frontend test:e2e` EXIT=0，12 个具名契约全过；新增
  `cost_display_consolidation` 25 条断言，1280×820 与 980×620 两视口实测：
  全部未定价 `{"text":"未知","warning":true}`、部分未定价 `$7.25/warning=true`、
  显式零价 `$0.00/warning=false`、按零估算 `$2.00/warning=false`、所有行文本无
  脚注符号；标题浮层同层含「来源采集统计」与「统计说明」、`overflowY:auto`、
  高度上限 480px/372px，费用浮层含构成、来源记录费用与未定价 token 明细。
- **原生**：受控 acceptance 构建登记后跑 `ui-pricing` 场景，19 项检查 0 失败；
  真实 WebView2 中 `summaryCost="未知"`、`groupCostTexts` 全部「未知」；
  `native-pricing-checks` 纯逻辑单测 49 项全过。
- 版本按仓库工具递增 patch 至 **0.9.3**（`--check --base-ref origin/main`：
  version 0.9.3 / previous 0.9.2 / release true）。本批停在可审查 PR，不合并、
  不发布、不安装。

### 交付状态（发布，2026-11-19 用户授权合并）

- PR #72 经三轮独立复核补修后，以 `--match-head-commit` 绑定已复核 HEAD
  `33efbb20cd1191147887f9d90ad465fa6c35e066` 合并为
  [bc92783](https://github.com/ywltby/tokenscope/commit/bc92783363904976444f3e914338a3c0beebb3c6)
  （merge commit）；main 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/44419334037)、
  [Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/44419334087)、
  [Release](https://github.com/ywltby/tokenscope/actions/runs/44419334105)
  三条流水线在该合并提交上全部实际成功。
- 三轮复核反例均先红后绿（正式组件回归 + 真实浏览器鼠标/键盘时序）：
  ① 「聚合」浮层鼠标离开与键盘失焦共用定时器互相清除 → 离开定时器只清
  hover、blur 只清 focus；② 费用按钮 Enter 冒泡到行处理器被 `preventDefault`
  吞掉并误发 day 下钻 → 行处理器只响应行自身键盘激活与非按钮来源点击，
  金额按钮保留原生一次 click；③ 关闭延迟内到达的焦点取消离开定时器致
  hover 滞留、失焦后浮层永不关闭 → `@focus` 只更新焦点状态，仅鼠标重新
  进入标题或浮层才取消待执行关闭。既有交互（间隙阅读、浮层内失焦、
  Escape、点击固定、外点关闭、行下钻）回归全过。
- [v0.9.3](https://github.com/ywltby/tokenscope/releases/tag/v0.9.3) 预发布
  已公开：Release 19 项作业全部成功且无跳过——gate、plan-upgrades、build、
  五场景 install-test、12 组单跳升级矩阵（0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 /
  0.6.0 / 0.9.2 × present/deleted）、present 与 deleted 两条连续升级链、
  publish。标签 `v0.9.3` 解引用即合并提交 `bc92783`。
- `build-metadata.json` 确认 commit `bc92783`、version `0.9.3`、
  persistent_format 与 v0.9.2 相同（cache_schema 12 / history_schema 5 /
  parser_revision 3 / identity_revision 1 / pricing_index 11 /
  settings_format 3——本批不改持久格式）；安装包 SHA256
  `874a00216dad102ee6083a2e16b1f50ecc531be0537c533d066a9a3ff321a4ce`
  下载后本机重算，与 `SHA256SUMS.txt` 及 GitHub 资产 digest 三方一致
  （`build-metadata.json` 哈希 `d904432a…` 同样三方一致）。本机不安装
  正式包，安装覆盖由 CI install-test / upgrade-test / upgrade-chain 完成。
- **v0.9.3 已登记升级基线**（`upgrade_baselines.py add --tag v0.9.3`，资产
  逐个下载核对后写入，status published）；`select --current-version 0.9.4`
  起点集合含 0.9.3（替代 0.9.2 的上一版位置，MAX_ORIGINS=6 不变）。
- 提交链：`e7857df`（批主体）→ `3a38f42`（第二轮：状态隔离与行/按钮隔离）
  → `33efbb2`（第三轮：焦点不取消离开定时器）→ 合并 `bc92783`。
- 版本保持 0.9.3，本记录合入不递增、不发布：`bump_version.py --check`
  显示 release: false，合并后 Release 只跑 gate。
