# 模型展示名：独立目录查询与裸 ID 正式名

- 分支：`fix/model-display-name-catalog`，基线 `origin/main` `fe07171`（v0.9.8）。
- 状态：**实施完成，待审查**（提交钩子与最新 HEAD 必需 CI 另行回填）。
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
   渠道数量、字典序、名称相似度、末段相同与首字母大小写都不是证据。
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
