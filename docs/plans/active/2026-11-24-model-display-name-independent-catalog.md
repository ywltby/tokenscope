# 模型展示名：独立目录查询与裸 ID 正式名

- 分支：`fix/model-display-name-catalog`，基线 `origin/main` `fe07171`（v0.9.8）。
- 状态：**实施完成，PR [#82](https://github.com/ywltby/tokenscope/pull/82) 待审查**；
  `79ed3b7` 的必需 CI 全绿，见 §8；复核补修批次见 §9。
- 发行版本：产品输入改动，按仓库工具取 patch **0.9.9**；视图快照 24 → 25。
- 发布准备：上一版 `v0.9.8` 已用 `scripts/upgrade_baselines.py add` 登记，
  `check --base-ref origin/main` 通过（资产逐个下载核对哈希）。

## 1. 问题与已核实的根因

v0.9.8 的 `Pricing::display_name_for` 只在报价索引的末段组内取词，并由
`unique_modelsdev_identity` 要求「同末段只有一个 models.dev 完整身份」才允许裸 ID
解析。本机 v4 目录带渠道前缀的完整 ID 极多，11 个历史裸 ID 因此全在比较名称之前
被拒绝，聚合表显示原始 ID（本机 v24 视图快照 2026-11-24 22:34 实测）。例如
`claude-opus-4-8` 命中 52 个身份，其中 47 个 name 为 `Claude Opus 4.8`，
`anthropic/claude-opus-4-8` 也有正式名。

第二处缺口在 `modelsdev::sync_with`：`let Some(cost) = m.cost else { continue }`
使无价条目的 `name` 与 `canonical_model_id` 一并丢弃。

本机 `~/.tokenscope/pricing-modelsdev.json` 只读复核：v4、9578 条、
`canonical_model_id` 全空；11 个裸 ID 均有至少一个带正式名的目录条目。

## 2. 上游查证（只读，2026-11-24）

- `https://models.dev/models.json`：638 条**规范模型**（`id` + `name`，无价格）。
  覆盖 11 个必验型号中的 10 个；`doubao-seed-2-0-pro-260215` 不在规范清单。
- `https://models.dev/api.json`：326 provider / 10181 条 provider 模型，含
  `canonical_model_id` 关系。`doubao-seed-2-0-pro-260215` 的两条目录记录
  （`nano-gpt`、`volcengine`）都明确指向 `bytedance-seed/seed-2.0-pro`；
  规范模型页 <https://models.dev/models/bytedance-seed/seed-2.0-pro>
  显示名称 `Seed 2.0 Pro`，并在 Providers 表列出该精确模型 ID。
- `/catalog.json` 在本机代理下反复 TLS 中断，仅作补充核对，不作为唯一依据。
- 发布方前缀与日期变体分别有独立规范条目，例如 `deepseek/deepseek-v4-flash`
  = `DeepSeek V4 Flash`、`deepseek/deepseek-v4-flash-0731` =
  `DeepSeek V4 Flash 0731`，不互相借名。

## 3. 不变量

1. **只影响展示**：`Group.key`、`model_stats_key`、`canonical_identity_key`、
   下钻指纹、请求明细原始 ID、CCS 覆盖、逐条计价与费用 breakdown 全部不变。
2. **身份优先于名称**：完整结构 ID / 有来源的 canonical 映射才可作为身份证据；
   渠道数量、字典序、名称相似度、末段相同与首字母大小写都不是证据。真实 ID 的
   **发布方规范名**（随包目录）优先于本地快照里的渠道名；同末段的近似名不得覆盖
   它（原生身份契约 `model_variants_merge_with_friendly_name` 同时断言两者：
   发布方名可见、渠道名不可见）。
3. **规则与数据分离**：Rust 只实现通用查询规则；规范名称与可核实别名放在可更新
   的数据文件（由公开 `models.json` / `api.json` 生成并注明来源与抓取日期）。
4. **离线可用**：无快照、仅 v0.9.8 旧快照（无 canonical 字段）、同步失败保留旧
   快照时均能解析已收录名称；上游有更权威的完整身份时优先采用。
5. **无据保留**：同名异模、真实命名冲突、无公开名称时返回缺失，聚合保留原始 ID，
   并给出结构化原因供审计；不猜名称、不按渠道众数取名。
6. **三处同源**：聚合表、`Group.label`、模型图表与 CCS 日汇总行共用
   `Pricing::display_name_for` → `aggregate::model_label`。

## 4. 实施与验收顺序

1. 先写红测试：合成多渠道有价有名、无价有名、乱序、虚假同末段、同名异模、日期
   变体、离线与真正无名称，确认旧实现失败。
2. 增加独立名称目录（规则模块 + 数据文件）与同步侧无价名称保留；名称条目不进入
   报价候选索引，计价口径零改动。
3. 全量审计可用历史模型清单（本机历史/缓存只读，公开测试只用合成或公开数据），
   逐条给出修复前后标签与依据/回退原因。
4. Rust、前端、浏览器契约与三处标签一致性实测；旧 v24 快照首屏/刷新验证。
5. 同步 `docs/stats-semantics.md` 与本计划，按仓库工具取号，提交钩子、推送、
   PR 与最新 HEAD 必需 CI 全绿；停在可审查状态。

## 5. 上游覆盖逐条核对（11 个必验型号）

本机 v24 视图快照（2026-11-24 22:34）显示全部 11 行 `label` 为原始 ID；
下列“修复后”来自真实 `Pricing::display_name_resolution_for` 在**本机 v4 缓存 +
离线公开目录**下的实测（`source=models.json` 表示公开规范目录）。

| 原始 ID | 修复前 | 修复后 | 规范完整 ID / 依据 |
| --- | --- | --- | --- |
| `gpt-5.3-codex-spark` | `gpt-5.3-codex-spark` | `GPT-5.3 Codex Spark` | `openai/gpt-5.3-codex-spark`，`/models.json` 规范条目；`/api.json` 的 opencode 条目 canonical 指向该 ID |
| `claude-opus-4-8` | `claude-opus-4-8` | `Claude Opus 4.8` | `anthropic/claude-opus-4-8`；本机末段 52 条、47 条同名 |
| `claude-opus-5-5` | `claude-opus-5-5` | `Claude Opus 5.5` | `anthropic/claude-opus-5-5`；本机末段 56 条 |
| `deepseek-v4.1-flash` | `deepseek-v4.1-flash` | `DeepSeek V4.1 Flash` | `deepseek/deepseek-v4.1-flash`；本机 107 条 |
| `deepseek-v4.1-flash-1017` | `deepseek-v4.1-flash-1017` | `DeepSeek V4.1 Flash 1017` | `deepseek/deepseek-v4.1-flash-1017`；与不带日期版独立 |
| `deepseek-v4-flash` | `deepseek-v4-flash` | `DeepSeek V4 Flash` | `deepseek/deepseek-v4-flash`；本机 `adaption` 渠道误写成 0731 名，规范名不借 |
| `deepseek-v4-flash-0731` | `deepseek-v4-flash-0731` | `DeepSeek V4 Flash 0731` | `deepseek/deepseek-v4-flash-0731`；日期后缀保留 |
| `deepseek-v4-pro` | `deepseek-v4-pro` | `DeepSeek V4 Pro` | `deepseek/deepseek-v4-pro`；另有 `deepseek-v4-pro-0813` = `DeepSeek V4 Pro 0813` |
| `doubao-seed-2-0-pro-260215` | `doubao-seed-2-0-pro-260215` | `Seed 2.0 Pro` | `/models.json` **无**该 ID；`/api.json` 与 `/catalog.json` 的 volcengine、nano-gpt provider 条目均显式 canonical → `bytedance-seed/seed-2.0-pro`，其规范名为 `Seed 2.0 Pro`（[官方模型页](https://models.dev/models/bytedance-seed/seed-2.0-pro)列出该精确 ID）。nano-gpt 渠道名 `Doubao Seed 2.0 Pro` 存在，但发布方与规范名优先 |
| `glm-5.2` | `glm-5.2` | `GLM-5.2` | `zhipuai/glm-5.2`；本机 94 条 |
| `glm-5.3` | `glm-5.3` | `GLM-5.3` | `zhipuai/glm-5.3`；本机 132 条 |

`/models.json`（638 条规范模型）覆盖其中 10 条；唯一不覆盖的 Doubao 有可核实的
canonical 关系与发布方页面，**不列为缺来源**。`catalog.json` 的 models 段与
`/models.json` 同名，providers 段给出上述 Doubao 关系。

## 6. 46 个历史 ID 只读全量审计

范围：`history.db.usage_events.model_raw`（34）与 `ccs_daily_usage.model`（24）的
并集，恰为 46 个原始 ID；本机数据只读，未写用户目录。真实解析链结果：
**37 个解析到名称，9 个保留原始 ID**。仍回退的 9 个及具体原因：

| ID | 回退原因 | 查证结果 |
| --- | --- | --- |
| `glm-5.2-x` | `no_identity_evidence` | `/models.json`、`/api.json` 均无该 ID 或同末段规范模型；不借用 `glm-5.2` |
| `gpt-6-astra-cc-format` | `no_identity_evidence` | 两份公开目录均无；自定义变体不借 `gpt-6-astra` |
| `grok-4.5-build` | `no_identity_evidence` | 公开目录无该完整 ID；`grok-4.5` 不覆盖 build 变体 |
| `grok-4.6-build` | `no_identity_evidence` | 同上；历史契约亦要求保留原 ID |
| `hy4-free` | `no_identity_evidence` | 公开目录无该 ID；不借 `hy4-flash` 或 `tencent/hy3` |
| `hy4-preview-f` | `no_identity_evidence` | 公开目录无该 ID；不借 `hy4-preview` |
| `kimi-k2.7` | `no_identity_evidence` | 公开目录无该 ID；不借 `kimi-k2.6` |
| `kimi-k3-1` | `no_identity_evidence` | 公开目录无该 ID；不借 `kimi-k3.1-pro` |
| `nvidia/nemotron-3-ultra-550b-a55b:free` | `no_trusted_name` | 有 5 个渠道条目且 canonical 指向 `nvidia/nemotron-3-ultra-550b-a55b`，但渠道名出现 `Nemotron 3 Ultra (free)` 与 `Nemotron 3 Ultra 550B A55B` 两种正式名，无法确认单一名称；不按字典序猜测 |

另有 `hy4-flash-free` 解析为 `Hy4 flash`（规范身份 `tencent/hy4-flash`）：api.json
的两个 provider 条目都显式 canonical 指向该基础模型，`-free` 属计费档；请求明细
仍保留原始 ID，统计分组与价格口径未变。其余解析项均给出 `source=models.json`
或 `source=models.dev-snapshot` 与规范完整 ID，详见审计输出（探针跑后已送回收站）。

## 7. 兼容与对照复核

- 旧 v24 视图快照：`SNAPSHOT_VERSION` 24 → 25，旧快照首屏忽略并按新名称重建；
  筛选偏好、时区、历史库、已导入 CCS、价格快照与索引均不改写，无需清库/重导。
- 离线/同步失败：`Pricing::load(None, None, None)` 与仅 v0.9.8 v4 快照（无
  canonical_model_id、名称全为 ID 抄写）均解析 11 个 ID；`gpt-6-astra-cc-format`
  等仍保留原 ID。
- 无价有名：同步把无 cost 条目写入快照 `names` 段；报价候选与 `estimate` 不变。
- 目录乱序、虚假同末段渠道、同名异模、真实命名冲突、日期变体：均由
  `model_display_name_catalog` 的合成夹具先红后绿覆盖；原因以结构化枚举记录。
- 三处标签：`bare_id_with_many_channels_shared_by_aggregate_chart_and_rollup`
  对 12 个渠道的 `claude-opus-4-8` 断言聚合行、图表共用的 `Group.label` 与 CCS
  日行标签一致；原始 ID 下钻命中，请求数/四桶/费用经 `reconcile` 守恒。

## 8. 交付与门禁

- 分支 `fix/model-display-name-catalog`，PR
  [#82](https://github.com/ywltby/tokenscope/pull/82)（基线 `origin/main` `fe07171`）。
- 提交：`beb88c7` 独立目录与解析；`ae3b8b1` 登记 v0.9.8 升级基线；`79ed3b7`
  原生身份契约回归修复。本文件回填为 docs-only 提交，不改变代码。
- 本地提交钩子（完整模式）：两模式 clippy `-D warnings`、默认与 acceptance
  全量测试、前端 typecheck / format / 1222 单测 / build / 浏览器契约、脚本契约
  与文档检查全部通过。
- `beb88c7` 的 PR CI：`rust`、`rust-acceptance`、`contracts`、`frontend`、`gate`
  通过；`install-test` 与含 0.9.8 的全部 `upgrade-test` 通过；`native-ui` **失败**。
- 该失败是真实回归，不是环境抖动：原生场景 `model_variants_merge_with_friendly_name`
  断言标签等于夹具渠道名 `Synthetic Opus 5.5`，而新规则对真实 ID `claude-opus-5-5`
  取发布方规范名 `Claude Opus 5.5`。已在 `79ed3b7` 改为断言发布方名可见、
  渠道名不可见，并补一条反例单测（标签退回渠道名时判定必须失败）；夹具渠道名保留
  作为同末段近似名的对照。
- `79ed3b7` 的 PR CI：`rust`、`rust-acceptance`、`contracts`、`frontend`、`gate`、
  `install-test`、`native-ui` 全部通过。
- 前端超时（本批复核后还原）：曾把 `testTimeout` 5s → 20s，理由记为 U06 用例含
  1.5s 队列等待。复核发现 `settingQueue` 以微任务排空、测试用立即 resolve 的保存
  函数，并无固定 1.5s 等待；5s 默认下 `Settings.test.ts` 61 项最慢 1.59s、全量
  82 文件 1225 项通过。已删除该全局覆盖，恢复 Vitest 默认 5s，不设逐用例覆写，
  用例集合与行为断言未改。

## 9. 复核补修批次（基线 `a9f509e`）

### 9.1 完整 ID 被渠道别名覆盖（红 → 绿）

根因在 `NameCatalog::resolve`：渠道别名（`aliases`）与快照 `claims` 先于完整 ID
自身的规范条目被查询。随包目录离线实测的修复前后：

| 完整 ID | 修复前 | 修复后 |
| --- | --- | --- |
| `deepseek/deepseek-v4-flash` | `DeepSeek V4.1 Flash 1017` | `DeepSeek V4 Flash` |
| `deepseek/deepseek-v4-pro` | `DeepSeek V4 Pro 0813` | `DeepSeek V4 Pro` |
| `deepseek/deepseek-v4-flash-vision-exp` | `DeepSeek V4.1 Flash 1017` | `DeepSeek V4 Flash Vision Exp` |
| `moonshotai/kimi-k2.7-code-highspeed` | `Kimi K2.7 Code` | `Kimi K2.7 Code Highspeed` |

修复后的优先级：已确认基础模型（统计分组身份）→ 完整 ID 自身规范名 → 渠道别名 →
快照 canonical 关系；后三者只补充自身没有规范名的条目。快照 canonical 关系也不得
覆盖自身规范名（`zhipuai/glm-5.2` 保持 `GLM-5.2`，不取 `GLM-5.2 0901`）。
无自身规范名的条目仍可按确认别名取名（`doubao-seed-2-0-pro-260215` →
`Seed 2.0 Pro`、`opencode/deepseek-v4-flash` → `DeepSeek V4 Flash 0731`）。
裸 ID 路径与 11 个必验型号保持原规则，完整 ID 与裸 ID 各自取日期变体/base 名。
展示名仍不进入 `Group.key`、下钻、原始请求 ID、CCS 覆盖或计价。

红测试：新增 `full_id_own_catalog_name_beats_channel_alias`、
`snapshot_canonical_claim_does_not_override_own_catalog_name`；修复前分别失败于
上表四个 ID 与 `zhipuai/glm-5.2`，修复后连同既有 DN-C01..C11 全部通过。

### 9.2 生成器保留同末段显式别名

`scripts/update_model_display_catalog.py` 原先丢弃末段相同的显式
`canonical_model_id`，导致 `relay/demo-v2 → publisher/demo-v2` 在重建后消失。
去掉该过滤后只保留仍成立的三项约束：目标是规范清单成员、关系显式、目标不等于
自身。新增 `scripts/test_update_model_display_catalog.py`（正向/倒序输入、真实同
末段关系、同名异模、目标缺失与自指、排序与顺序无关），并纳入 contracts 组。

用仓库脚本与公开目录重新生成 `model_display_catalog.json`（抓取日期
2026-11-25）：规范模型 **640** 条、别名 **8679** 条、文件 985,296 字节。相对
HEAD：规范模型 +6 / −4 / 0 改名；别名 +6964 / −21，另有 2 条改指向
（`databricks/…gemini-3-1-flash-lite`、`fireworks-ai/…kimi-latest`）。最终目录中
5584 条别名与来源同末段（本次新增的 6964 条里同末段者也是 5584 条），且**没有
任何同末段来源同时是规范条目**，因此不改变完整 ID 自身规范名优先与裸 ID 唯一
末段规则。生成器测试与真实目录离线回归
（`302ai/MiniMax-M2` → `MiniMax-M2`，裸 `minimax-m2` 同名）均通过。

### 9.3 v24 → v25 首屏回归

`Dashboard.test.ts` 新增两个用例：挂起首个查询时，v24 快照的旧裸 ID 标签不得
回到首屏，筛选偏好保持不变；放行后显示规范名并落盘 v25，且筛选偏好原样保存。
首个查询失败时同样不回显旧标签、不写快照。把 `SNAPSHOT_VERSION` 临时改回 24 时
两个用例均失败，恢复 25 后通过。验证层级为 Vitest 组件级 + 模拟 `invoke`；未读取
或改写本机真实历史/缓存。真实 WebView2 验收已用受控 acceptance 构建在本机执行：
`ui-identity` 22 项、`ui-refresh` 17 项检查全部通过（含
`model_variants_merge_with_friendly_name`、`model_detail_keeps_raw_names`、
`real_data_untouched`）。

### 9.4 门禁结果（`c49e5a1`）

- 本地完整提交钩子通过：两模式 workspace clippy `-D warnings`、默认 327 项与
  acceptance 328 项 Rust 测试、前端 1225 项单测 / typecheck / format / build /
  17 个浏览器契约、脚本 419 项与文档检查。
- CI run [45585034323](https://github.com/ywltby/tokenscope/actions/runs/45585034323)：
  `rust` 6m30s、`rust-acceptance` 6m19s、`contracts` 8m36s、`frontend` 8m47s、
  `gate` 全部通过。
- Install test run [45585034280](https://github.com/ywltby/tokenscope/actions/runs/45585034280)：
  `install-test` 12m24s、`plan-upgrades` 11s、全部单跳升级用例（含 0.9.8
  present/deleted）通过；`upgrade-chain` 按无连续升级链需求跳过。
- Native UI E2E run [45585034318](https://github.com/ywltby/tokenscope/actions/runs/45585034318)：
  `native-ui` 9m40s 通过。`gh pr checks 82` 在最新 HEAD 上退出码 0。

## 10. 二次复核补修批次（基线 `24c2871`）

目标版本仍为 **0.9.9**，沿用 PR #82，不重新取号、不改 `release-change.json`。
复核基线本地与 `origin/fix/model-display-name-catalog` 均为 `24c2871`；其 CI
[45587706315](https://github.com/ywltby/tokenscope/actions/runs/45587706315)、
Install test [45587706311](https://github.com/ywltby/tokenscope/actions/runs/45587706311)
与 Native UI E2E [45587706307](https://github.com/ywltby/tokenscope/actions/runs/45587706307)
全部通过（`upgrade-chain` 按无连续升级链需求跳过）。

### 10.1 同末段统计映射不再覆盖完整 ID 规范名

红测试用两侧有价的合成 v4 快照：`deepseek/deepseek-v4-flash` 自身规范名为
`DeepSeek V4 Flash`，其 `canonical_model_id` 指向同末段
`sample-lab/deepseek-v4-flash`，目标快照名为 `Sample Lab Flash`。修复前直接查询与
聚合标签均实测为 `Sample Lab Flash`；统计键本身已正确为
`samplelab/deepseekv4flash`，证明问题在展示链而非分组链。

完整调用链：`Pricing::display_name_resolution_for` 把
`ModelStatsGrouping::confirmed_base_for` 传入 `NameCatalog::resolve`，后者原先先取
该基础键；聚合又与 CCS 日行共用 `aggregate::model_label`，并先查询 `Group.key`。
因此只调换解析器顺序仍会让界面取到渠道目标名。

修复规则：完整 ID 自身规范名 → 已确认基础模型 → 显式别名 → 快照 canonical →
裸 ID 唯一目录身份。标签先取统计组键自身的规范名，再从已确认指向该组的别名中
稳定选择自身规范名；组键无规范名时才取当前原始型号自身规范名，最后回退分组键和
普通解析。CCS 日行复用同一规则，跨日保持同一标签且不依赖单日代表。单别名、多别名与
“仅 CCS 日行有别名”三种场景均断言聚合表/图表共用标签、每条 CCS 日行标签为
`DeepSeek V4 Flash`，多别名输入倒序喂入；`Group.key`、实际分组、下钻、原始明细 ID、
CCS 覆盖、逐条计价与四桶汇总保持原断言。四个完整 ID、11 个裸 ID 与仍需显式别名的
Doubao 2.0 对照继续通过。该查询回归在独立子进程运行，避免新增会话与既有迁移测试
并行争用进程级查询额度；默认并行度连续五轮通过。

### 10.2 目录回退审计与可复现历史覆盖层

用脚本按 Rust 的等价键、规范末段集合与别名目标集合复核 `a9f509e`、
`24c2871` 和最终目录。`24c2871` 本身只改文档；`a9f509e → 24c2871` 的目录变化
来自 `c49e5a1` 重新生成，而非 revert。生成器取消同末段过滤后新增 6964 条别名，
但当时删除的 21 条均是上游 provider 条目退休/同步移除，**不是被同末段过滤**。
最终公开抓取（2026-11-25）又删除 5 条 Eden AI / EmpirioLabs 别名；两类删除补丁
均能在删除前 TOML 核实显式 `base_model`，故纳入有证据的历史覆盖层。

`scripts/fixtures/model_display_catalog_history.json` 逐条记录 26 条历史别名与
4 个 `v2_only` 规范模型的来源、原因与证据提交；生成器只补入这些条目，当前上游
名称与改指向优先，目标缺失或自指直接报错，不自动合并整份旧目录。目标校验在
当前上游改指向之后执行，避免合法新指向被已失效的历史目标阻断。26 条别名包括：

- Databricks 5 条：`databricks-claude-sonnet-4`、`databricks-gemini-2-5-flash`、
  `databricks-gemini-2-5-pro`、`databricks-gemini-3-pro`、`databricks-kimi-k2-7-code`。
- DeepInfra 1 条：`meta-llama/Llama-4-Maverick-17B-128E-Instruct-FP8`。
- Fireworks 1 条：`kimi-k3-spot`。删除前上游显式 `base_model`，但注释承认 Spot
  路由未独立探测、身份为 assumed；夹具标为 `upstream_assumed` 弱证据，不伪装成已独立验证。
- NanoGPT 1 条：`Kimi-K2-Instruct-0905`；ThinkingMachines 1 条：
  `NVIDIA-Nemotron-3-Nano-30B-A3B-BF16`。
- TogetherAI 5 条：`Qwen2.5-7B-Instruct-Turbo`、
  `Qwen3-235B-A22B-Instruct-2507-tput`、`Qwen3-Coder-480B-A35B-Instruct-FP8`、
  `Qwen3-Coder-Next-FP8`、`Llama-3.3-70B-Instruct-Turbo`。
- Volcengine / coding-plan 6 条：`doubao-seed-1-6-251015`、
  `doubao-seed-1-6-flash-250828`、`doubao-seed-1-6-vision-250815`、
  `doubao-seed-1-8-251228`、`doubao-seed-2.0-lite`、`doubao-seed-2.1-turbo`、
  `doubao-seed-2-0-code-preview-260215`（共 7 条，含 coding-plan 两条）。
- Eden AI 4 条：`gemini-4-argon`、`gemini-4-argon@eu`、`gemini-4-argon@us`、
  `gemini-pro-latest`；EmpirioLabs 1 条：`gemma-3-27b`（删除前已标 deprecated）。

`a9f509e` 中失去名称的 15 个裸 ID 已全部恢复：

| 裸 ID | 恢复名称 |
| --- | --- |
| `doubao-seed-1-6-251015` | `Seed 1.6` |
| `doubao-seed-1-6-flash-250828` | `Seed 1.6 Flash` |
| `doubao-seed-1-8-251228` | `Seed 1.8` |
| `kimi-k3-spot` | `Kimi K3` |
| `databricks-claude-sonnet-4` | `Claude Sonnet 4` |
| `databricks-gemini-2-5-flash` | `Gemini 2.5 Flash` |
| `databricks-gemini-2-5-pro` | `Gemini 2.5 Pro` |
| `databricks-gemini-3-pro` | `Gemini 3 Pro Preview` |
| `databricks-kimi-k2-7-code` | `Kimi K2.7 Code` |
| `Qwen2.5-7B-Instruct-Turbo` | `Qwen2.5 7B Instruct` |
| `Qwen3-235B-A22B-Instruct-2507-tput` | `Qwen3 235B-A22B Instruct 2507` |
| `mimo-v2-tts` | `MiMo-V2-TTS` |
| `mimo-v2.5-tts` | `MiMo-V2.5-TTS` |
| `mimo-v2.5-tts-voiceclone` | `MiMo-V2.5-TTS-VoiceClone` |
| `mimo-v2.5-tts-voicedesign` | `MiMo-V2.5-TTS-VoiceDesign` |

最终目录为 **644 个规范模型、8700 条别名**。相对 `a9f509e`：模型 +6 / −0 /
0 改名；别名 +6964 / −0；2 条改指向均保留当前上游值：
`databricks/databricks-gemini-3-1-flash-lite` → `google/gemini-3.1-flash-lite`、
`fireworks-ai/accounts/fireworks/routers/kimi-latest` → `moonshotai/kimi-k3.1-pro`。
裸 ID 无丢失、无新增歧义；新增 76 个可解析裸 ID，上述 Databricks 裸 ID 随上游
更正从 `Gemini 3.1 Flash Lite Preview` 改为 `Gemini 3.1 Flash Lite`。相对
`24c2871`：模型 +4、别名 +21，无删除/改名/改指向，恰好恢复 15 个丢名裸 ID，
无其它裸 ID 解析变化。目录有 3 组等价键重复（标点差异），目标完全相同，不构成冲突。

真实冲突 `gemini-pro-latest` 保持歧义：Eden AI 两条显式指向
`google/gemini-4-argon`，Google 自身条目指向 `google/gemini-3.1-pro-preview`；
不以字典序猜名。无证据撤回或更正旧映射；退休/deprecated/v2_only 是可用性变化，
不是身份否定。生成器测试覆盖历史层优先级、乱序、真实同末段关系、缺失目标与自指；
Rust 离线回归覆盖 15 个代表项、上述真实冲突、目录乱序、同末段显式别名与两条改指向。
