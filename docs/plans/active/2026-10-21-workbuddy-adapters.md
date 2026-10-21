# 新增 WorkBuddy（国内版）与 WorkBuddy AI（国际版）用量来源

> 状态：**草案，待用户确认后实施**。2026-10-21 按用户要求移入 `docs/plans/active/`（此时 main 只合入了[剩余验收收口计划](2026-10-21-acceptance-closure.md)本身，代码未变，§1 行号仍有效）。收口计划的 B2 会改动来源路径与验收接线，**开始实施前必须按当时的 main 重新核对 §1 的事实与行号**；用户确认后再在整合计划登记责任编号。
> 用户 2026-10-21 决定：
> - 国内版与国际版**分成两个应用**；
> - 隐私政策**增加修订号**，老用户升级后重新询问同意；
> - 等 Codex 完成收口计划后再实施。
> 本计划解除 CLAUDE.md「候选 agent 暂缓排期」中对 WorkBuddy 的暂缓，由用户本次明确排期。

**Goal：** TokenScope 读取 WorkBuddy 国内版（`~/.workbuddy`）与国际版 WorkBuddy AI（`~/.workbuddy-ai`）本地会话转写中的逐请求用量，作为两个独立应用纳入统一历史库、汇总、筛选、下钻和计价。现有 Claude Code / Codex 的统计结果不变。

**Architecture：**
- 两个应用共用一个解析实现 `source/workbuddy.rs`，以 `AgentKind` 区分实例；只读取 `projects/` 下的 JSONL，**不打开** `workbuddy.db`，也不读取 `traces/`、`file-history/`、`models.json` 等其他目录。
- 先把来源层“两个应用用 `bool` 区分”的写法泛化为按 `AgentKind` 索引，再加入两个新应用。
- 隐私同意增加修订号：已同意旧修订的用户在升级后重新看到政策；同意前不读取任何来源。
- 前端、设置、来源目录、验收接线按四个应用扩展。

**Tech Stack：** Rust（核心库、Tauri 壳）、Vue 3 + Naive UI、SQLite 历史库、PowerShell/Python 验收脚本、Node 原生验收驱动。

## 1. 背景与已核实事实（2026-10-21）

研究依据：[国际版核验](../../research/2026-10-11-workbuddy-ai-session-storage.md)、[国内版核验](../../research/2026-10-11-workbuddy-session-storage.md)。本日只读复核本机数据（只统计字段与计数，不读内容）：

| 项 | `~/.workbuddy`（国内版） | `~/.workbuddy-ai`（国际版） |
| --- | --- | --- |
| 版本（`last-launch.json`） | 5.8.3（研究时 5.7.7） | 5.7.6 |
| `projects/**/*.jsonl` | 48 个，其中子 agent 文件 19 个 | 1 个 |
| 带 `providerData.rawUsage` 的行 | 2881（子 agent 112） | 656 |
| 最近更新 | 2026-10-21 | 2026-10-08 |
| `rawUsage` 形态 | 旧形态 1939（含 hit/miss/credit）、新形态 942 | 旧形态 656 |

2026-10-21 用当前数据（国内版 5.8.3、国际版 5.7.6）重跑研究断言，全部成立，零反例：

| 断言 | 国内版 | 国际版 |
| --- | --- | --- |
| 用量行都带 `messageId`、`model`、`requestModelId`、`cwd`，`timestamp` 为整数毫秒 | 2881/2881 | 656/656 |
| `messageId` 在两个目录合计 3537 个，全部唯一（跨目录零交集） | — | — |
| `providerData.usage`、`message.usage` 与 `rawUsage` 的输入、输出相等 | 2881/2881 | 656/656 |
| `prompt_tokens_details.cached_tokens` 存在 | 2881/2881 | 656/656 |
| 旧形态 `hit + miss == prompt`，且 `cached_tokens == hit` | 1939/1939 | 656/656 |
| `message.usage.cache_read_input_tokens` 存在时等于 `cached_tokens` | 1975/1975（其余 906 行缺该键） | 654/654（2 行缺键） |
| `total == prompt + completion` | 2881/2881 | 656/656 |
| `reasoning_tokens <= completion_tokens`（有该字段时） | 2880/2880 | 656/656 |
| `status: incomplete` 的行都不带用量 | 48/48 | 3/3 |
| 用量行都不是压缩摘要行 | 2881/2881 | 656/656 |
| 用量行所在的行类型 | `function_call` 2721、`message` 160 | 650、6 |
| `cwd` 为小写盘符加反斜杠 | 2881/2881 | 656/656 |
| `cache_creation_input_tokens` 非 0 | 0 | 0 |

文件与字段细节：
- **目录形态只有两种**：`projects/<slug>/<sessionId>.jsonl`，以及 `projects/<slug>/<sessionId>/subagents/agent-*.jsonl`。国内版的 19 个子 agent 文件中，有 14 个带用量。
- **子 agent 文件的会话 ID 是独立的**：每个文件只有一个 `sessionId`，是独立的 UUID，既不等于父会话 ID，也不等于文件名；主文件的 `sessionId` 都等于文件名（2769/2769）。
- **空文件与末行**：有 1 个 0 字节文件；有 1 个文件末尾没有换行，但最后一行是完整的 JSON。现有适配器是整文件读取、文件指纹变化就重新解析，写入中途的半行会暂时计为坏行，下一轮自然恢复，与 Claude 的现有行为一致。
- **模型字段**：`model` 都是干净的模型名（如 `deepseek-v4.1-flash`、`glm-5.2`、`kimi-k2.6`、`hy4-preview`）；`requestModelId` 有 942 行带冒号前缀、6 行与 `model` 不同。这证实应以 `model` 为准。

研究已验证的口径（实施以此为准，复核时以新样本重跑断言）：

- **一次调用一行**：带 `rawUsage` 的行与 `messageId` 一一对应，跨文件全局唯一；主文件与子 agent 文件的 `messageId` 零交集。
- **三处用量容器取值恒等**：`rawUsage`、`usage`、`message.usage` 三处只能取一处。
- **`prompt_tokens` 含缓存命中**（OpenAI 语义）：
  - 缓存真值只在 `prompt_tokens_details.cached_tokens`，缺失时回退 `prompt_cache_hit_tokens`；
  - `rawUsage` 侧的 `cached_tokens`、`cache_read_input_tokens` 等是死字段；
  - `message.usage.cache_read_input_tokens` 可能缺键，不得单独使用。
- **`completion_tokens` 已包含推理 token**，不另加 `reasoning_tokens`。
- **失败调用不记用量**：`status: incomplete` 的行带 `error`，不带用量和 `messageId`。
- **模型字段**：`requestModelId` 常带渠道前缀（如 `custom-local:`）或别名，取模型应以 `providerData.model` 为准。
- **时间与 cwd 格式**：`timestamp` 是 Unix 毫秒（UTC）；`cwd` 是小写盘符加反斜杠，现有路径规范化已经覆盖这种形态。
- **云端会话在本机读不到用量**（2026-10-21 复核）：国内版 `workbuddy.db` 中有云端会话 41 个（35 个来源为 `scheduler` 的定时任务，6 个来源为 `workbuddy-app`），本地会话 29 个；国际版没有云端会话。逐项核对的结果如下：
  - 云端会话没有任何 JSONL；`session_usage` 表的 21 行全部属于本地会话，云端会话一行都没有；
  - 在 `~/.workbuddy` 中，除安装程序、插件、技能等目录外的 7883 个文件里逐个搜索这 41 个会话 ID，只在 `workbuddy.db` 与 `logs/` 中出现；`traces/`、`local_storage/` 等目录都没有；
  - `logs/<日期>/sdk/conversations/__cloud__.log` 只记录列表同步（`fetchList` 34307 次、`cloud.automation.cacheRead` 340 次、`listByIds` 2 次），字段只有 `total`、`ids`、`count`、`url` 和任务样本的 ID 与标题，没有任何 token、用量或 credit 字段；其他提到云端会话 ID 的日志行里也找不到这类字段。
  - 结论：云端会话的用量只存在于服务端，本机没有可读的记录。要拿到这部分用量，只能带上用户凭据去调用服务端接口，这违反只读、本地的原则，本计划不做。
- **删除标记不删文件**：在 WorkBuddy 内删除的本地会话，转写文件仍留在原位，照常计入。
- **敏感文件**：`models.json` 明文保存自定义端点的 `apiKey`；`traces/` 含完整提示词与模型响应；`file-history/` 是用户代码快照。

代码现状（以 2026-10-21 的 main 为准，移入计划时重新核对）：
- **`AgentKind` 只有两个值**（`model.rs:8`）。以下位置用穷举 `match` 或写死的两元素列表处理它：
  - 核心库：`dedupe.rs`、`report.rs`（多处，含 `1079` 与 `1266` 两处写死的应用列表）、`import/ccs.rs`、`history.rs` 的 `parse_agent`；
  - 壳：`commands.rs` 的字符串映射。
- **来源配置按 `bool` 区分两个应用**：
  - `settings::source_config(claude: bool)`（`settings.rs:276`）；
  - `acceptance::source_dir_override(claude: bool)`；
  - `AgentSources { claude, codex }`。
- **设置格式修订号** `SETTINGS_FORMAT_REVISION = 2`，迁移规则在 `scripts/fixtures/settings_migrations.json`。
- **同意记录只有布尔值** `privacy_policy_accepted`，没有修订号。`docs/privacy.md` 写明“当前支持 Claude Code 与 Codex”，并由程序打包。
- **升级测试会直接写入同意**：`scripts/upgrade_fixture.py`、`upgrade_test_helpers.ps1` 写入 `privacy_policy_accepted = true` 来授予测试同意，并在升级后核对同意保留。
- **前端写死两个应用**：
  - `types.ts` 的 `AgentFilter`、`SourcePathRow.agent`、`AGENT_LABEL`；
  - `viewFilters.ts` 的 `AGENTS`；
  - `Settings.vue` 的 `AGENT_IDS`；
  - `Dashboard.vue` 的来源图标选项。
  - DESIGN.md 规定汇总页来源控件只显示图标。
- **跨应用身份**：`model.rs` 的 `same_source_identity` 中，Claude 用原生强身份，Codex 用保守重播指纹 `codex-usage`。

## 2. 范围与不做

做：
- WB01 来源层泛化。
- WB02 两个新应用的身份、存储与去重。
- WB03 共享解析器。
- WB04 来源发现与设置。
- WB05 隐私政策修订与重新同意。
- WB06 前端与设计规范。
- WB07 验收接线与原生场景。
- WB08 真实数据只读对账。
- WB09 文档与交付。

不做：
- 不打开 `workbuddy.db`（不读会话标题、云端会话列表、`session_usage`），不读 `traces/`、`file-history/`、`changes-*`、`workspace/`、`audit-log/`、`models.json`、`settings.json`。
- 不使用 `credit` 作为费用（单位是服务端点数，不是美元），也不展示它；费用仍按现有三来源候选估算，未收录模型显示“未知”。
- 不统计云端会话（本机没有用量记录，见 §1），也不调用 WorkBuddy 服务端接口；不按 WorkBuddy 内的删除标记过滤（与“来源删除不删已保存用量”同向）。
- 不提供主/子 agent 关系展示；子 agent 用量照常纳入。
- 不接入 CodeBuddy（`~/.codebuddy`，同族格式），留作以后单独排期。
- 不为 WorkBuddy 增加环境变量派生根（研究未发现对应的环境变量）。
- 不改 Claude Code / Codex 的解析与统计口径。

## 3. 必须保持的不变量

1. **现有统计零变化**：同一份 Claude Code / Codex 输入，改动前后的事件集合、四桶与费用逐项相同（WB01 必须证明这一点）。
2. **一次调用只计一次**：只有同时带 `providerData.rawUsage` 与 `providerData.messageId` 的行才产生事件，每行恰好一个；三处容器从不相加。
3. **桶互斥**：
   - `input = prompt_tokens − cached`；
   - `cache_read = cached`，其中 `cached` 取 `prompt_tokens_details.cached_tokens`，缺失时回退 `prompt_cache_hit_tokens`；
   - `cache_write = rawUsage.cache_creation_input_tokens`，缺失时为 0；
   - `output = completion_tokens`。
   - 全程受检算术：`cached > prompt`、负数、非整数或溢出都计坏行跳过，不饱和、不回绕。
4. **不补造请求**：失败调用、没有 `messageId` 的用量行不产生事件，也不按 0 凑数；没有模型时计入 `skipped_no_model`。
5. **两个应用相互独立**：身份按 `(app, sessionId, messageId)`；同样的内容出现在两个应用的根下时，作为两个应用的事件分别计入，不跨应用合并，也不触发跨应用去重。
6. **重复扫描幂等**：重扫、文件追加、重启都不增加已有事件；来源文件删除或停用后，已保存的用量仍可查询。
7. **只读、最小读取面**：只遍历两个应用的 `projects/` 根；测试证明根外的敏感文件从不被打开；不写入任何 WorkBuddy 目录。
8. **同意先于读取**：同意记录的修订号低于当前修订号时，按未同意处理，零业务读取；重新同意后才读取四个应用。拒绝新政策时按现有逻辑退出。
9. **项目身份一致**：同一规范化路径在四个应用中得到同一项目 key，子目录归当前根的规则与 Claude 相同。
10. **时间口径**：毫秒时间戳按 UTC 存储一次，展示沿用现有时区链路。

## 4. 任务

每项先写会失败的测试（解析类必须带“合成样例 → 期望事件”的 fixture），在未改的代码上确认红，再实现。一个提交只解决一类问题。fixture 一律手工合成，不复制真实转写的任何内容。

### WB01 来源层泛化（无行为变化）

- 把 `source_config(claude: bool)`、`source_dir_override(claude: bool)` 及类似的布尔分支改为按 `AgentKind` 取值；`AgentSources` 改为可按 `AgentKind` 访问，但**序列化格式保持不变**（仍是 `claude` / `codex` 两个键）。
- `report.rs` 中写死的应用列表改为来自一处 `AgentKind::ALL`；前端的 `AGENT_IDS` 等暂不改动（放在 WB06）。
- 验收：
  - 全部现有测试无需修改即全绿；
  - 新增一条等价测试：同一份合成 Claude/Codex 输入，泛化前后的汇总结果逐项相同；
  - `settings.toml` 的读写往返结果逐字节不变。

### WB02 应用枚举、历史库身份与去重

- `AgentKind` 增加 `WorkBuddy`（serde 与字符串 `workbuddy`，显示名 “WorkBuddy”）和 `WorkBuddyAi`（`workbuddy-ai`，“WorkBuddy AI”）。
- 补齐所有穷举分支：
  - `history::parse_agent`；
  - `dedupe.rs`：两个新应用都按 `(session_id, record_id)` 强身份去重，同一身份行序靠后者胜，与 Claude 规则一致；
  - `same_source_identity`：新身份方案 `workbuddy-message`，值为 `sessionId|messageId`，`app` 字段区分两个应用；
  - `import/ccs.rs`：CCS 没有这两个应用，映射为不支持；
  - `commands.rs` 的字符串映射；
  - `report.rs` 的显示名。
- 历史库 `usage_events.app` 是 TEXT，不改表结构。核对 `persistent_format` 是否需要随新的 app 值记录一个修订；需要的话按 CLAUDE.md 同步升级基线规则。
- 测试：
  - 新 app 值写入、读出往返；
  - 两个应用的同一 `sessionId|messageId` 互不合并；
  - 用旧版本历史库升级后，读取不受影响。

### WB03 共享解析器 `source/workbuddy.rs`

- **发现**：在给定根（`projects/`）下递归收集 `*.jsonl`，覆盖 `<slug>/<sessionId>.jsonl` 与 `<slug>/<sessionId>/subagents/agent-*.jsonl`（子 agent 文件名的 hex 位数不固定，按 `agent-*.jsonl` 匹配）。忽略 `.ndjson`、`.meta.json`、`tool-results/*.txt`。根不存在时返回零结果并记一条诊断。
- **解析**：逐行用 serde 解析，只处理同时带 `providerData.rawUsage` 与 `providerData.messageId` 的行：
  - 模型取 `providerData.model`，缺失时回退 `requestModelId`，原样保存，不裁剪前缀；
  - `session_id` 取行内 `sessionId`，子 agent 文件按其自身的 `sessionId`；
  - `record_id` 取 `messageId`；
  - 时间由 `timestamp` 毫秒值转为 UTC；
  - 四桶按不变量 3 计算；
  - `event_cwd` 取行内 `cwd`；项目根按 Claude 同款规则沿行序推进（进入子目录仍归当前根，越出当前根才切换）；`session_initial_cwd` 取文件中第一条带 `cwd` 的行。
- **计数**：沿用现有统计项（`files_scanned`、`lines_seen`、`events`、`bad_lines`、`skipped_no_model`、`io_errors`）；不新增前端统计项，除非实测确有需要。
- **fixture 用例**（`crates/tokenscope-core/tests/` 下新建 `workbuddy_source.rs`，fixture 放在 `tests/fixtures/workbuddy/`）：
  - 路径发现：根不存在；只有空文件；主文件加子 agent 文件；同目录下的 `.file-rollback.ndjson`、`.meta.json`、`tool-results/*.txt` 不被当作转写。
  - 典型解析：
    - 旧形态行（含 hit/miss/credit）与新形态行（无这三项）各一条；
    - 一次调用横跨 `function_call`、`reasoning`、`message` 多行，只有一行带用量；
    - 三处容器同时存在时只计一次；
    - 不带用量的 `message` 行。
  - 口径定向：
    - `rawUsage.cache_read_input_tokens` 为 0、`message.usage.cache_read_input_tokens` 为非 0 时，以 `prompt_tokens_details.cached_tokens` 为准；
    - `message.usage` 缺 `cache_read_input_tokens` 时结果不变；
    - 缺 `prompt_tokens_details` 时回退 `prompt_cache_hit_tokens`；
    - `completion_tokens` 已含推理 token，不重复相加；
    - `requestModelId` 为 `custom-local:<model>` 时，事件模型取 `model`。
  - 不补造：`status: incomplete` 带 `error` 的行、没有 `messageId` 的用量行、`file-history-snapshot` 行、压缩摘要行（只有 `logicalParentId`），都不产生多余事件。
  - 畸形容错：非 JSON 行、`cached > prompt`、负数、非整数、u64 溢出，都计坏行跳过，不 panic；各自合法但累计溢出时，查询明确报错。
  - 项目：小写盘符加反斜杠的 `cwd`，与 Claude 事件中大写盘符、正斜杠的同一路径得到同一项目 key；子目录归根、越界切换。
  - 时间：毫秒时间戳落在 UTC 跨日边界附近时，按 Asia/Shanghai 统计的结果正确。
  - 幂等：同一文件扫两次；文件追加新行后增量读取，事件数只增加新行。

### WB04 来源发现与设置

- **默认根**：
  - WorkBuddy：`~/.workbuddy/projects`；
  - WorkBuddy AI：`~/.workbuddy-ai/projects`；
  - 两者都纳入现有的固定候选集。
- 每个应用都有总开关、自动发现开关、按根排除、手动追加目录；跨应用的根相同或互为父子时，沿用现有冲突判定（例如有人把 `~/.workbuddy` 追加给 Claude）。
- **设置格式**：
  - `sources` 增加 `workbuddy`、`workbuddy_ai` 两个键（缺失即默认：启用加自动发现）；
  - 连同 WB05 的同意修订号，`SETTINGS_FORMAT_REVISION` 升到 3；
  - `scripts/fixtures/settings_migrations.json` 增加 `1->3`、`2->3`、`3->3` 的规则（缺失字段的默认值）。
- **验收隔离**：acceptance 构建的来源覆盖扩展到 `<root>/sources/workbuddy`、`<root>/sources/workbuddy-ai`。
- 测试：发现层三件套（目录存在、不存在、冲突）；设置读写往返与旧格式迁移；`source_paths_*` 现有用例扩展到新应用；跨应用冲突用例。

### WB05 隐私政策修订与重新同意

- 核心库增加常量 `PRIVACY_POLICY_REVISION`（本次为 2；没有修订号字段的旧同意视为 1）。设置增加 `privacy_policy_revision`。
- `read_consent`：只有 `privacy_policy_accepted = true` 且修订号不低于当前值时才视为同意；其余情况进入 `NeedsConsent`，并带上“政策已更新”标记。
- 同意时把 `true` 与当前修订号在**同一次原子保存**中写入。
- **引导界面**：政策已更新时，弹窗顶部显示一句“隐私政策已更新：新增读取 WorkBuddy 与 WorkBuddy AI 的本地会话日志”；其余交互（同意 / 不同意 / 退出确认）不变。
- **`docs/privacy.md`**：
  - 更新日期与适用版本；
  - 支持来源改为四个应用，并写明两个新默认根；
  - 明确不读取的文件（`workbuddy.db`、`traces/`、`file-history/`、`models.json` 等）；
  - 写明云端会话不统计；
  - “政策更新”一节说明修订号会触发重新询问。
- **升级与验收工具**：
  - `upgrade_fixture.py` 的授予同意和确保同意同时写入修订号；
  - 升级后的核对改为两条：旧同意记录原样保留；当前修订号的测试同意可以进入业务；
  - `prepare-native-acceptance.ps1` 与浏览器 mock（`privacySnapshot`）同步修订号。
  - 这些都会影响 Release 的升级矩阵，本地先跑 `scripts/test_*.py` 与升级脚本契约。
- 测试：
  - Rust：缺修订号、修订号偏低、等于当前、高于当前（视为同意）、类型错误、损坏文件（原文件逐字节不变）；同意保存原子性。
  - 原生：在 `privacy-consent-gate` 系列新增“旧修订同意 → 重新询问、零业务读取 → 同意后写入新修订号 → 重启不再询问”。

### WB06 前端与设计规范

- **类型与筛选**：`types.ts` 的 `AgentFilter`、`SourcePathRow.agent` 与 `AGENT_LABEL`，以及 `viewFilters.ts` 的 `AGENTS`、`sourceIdOf` 的映射，都扩展到四个应用。视图快照中的旧筛选值继续有效。
- **汇总页来源控件**：变为五个图标选项（全部 + 四个应用）。
  - 两个新应用没有可直接使用的品牌图标：使用仓库自绘的中性字母徽标（如 “WB” / “WA”），保留可访问名称与悬停提示，不引入第三方商标素材；
  - 980×620 视口下不得换行溢出。
- **设置页**：来源分区扩展为四个应用；来源状态、目录管理、诊断沿用现有组件。两个 WorkBuddy 分区各加一行固定说明：“只统计本机会话；云端会话与定时任务的用量只保存在服务端，本机无法读取。”避免用户以为漏算。
- **DESIGN.md**：同步来源控件的选项数、徽标规则与配色语义（按用户需求改动设计约束）。
- 测试：
  - 现有前端单测中写死“两个应用”的断言，属于产品口径变化，可以更新，但每处要在提交说明里列出原因；不得借机放宽其他断言。
  - 新增：四个应用的筛选、下钻、标签、设置分区；`check-ui-contracts` 中涉及来源控件的契约在两个视口、两种主题下通过。

### WB07 验收接线与原生场景

- `prepare-native-acceptance.ps1` 为两个新应用各生成合成转写，包括：
  - 一个子 agent 文件；
  - 一条失败调用行；
  - 新旧两种形态；
  - 与 Claude 事件同路径的 `cwd`（用于验证项目跨应用合并）。
- 在 `ui-identity` 场景（收口计划 AC03，届时已存在）中追加断言：
  - 四个应用都能在来源筛选中独立选中，数值与合成数据一致；
  - 同一路径跨四个应用合为一个项目；
  - 子 agent 用量已计入；
  - 失败调用没有被计入。
- 在 `ui-source-paths` 中追加：停用 WorkBuddy AI 后，其已保存用量仍可查询。

### WB08 真实数据只读对账（本地）

- 用隔离输出目录（历史库、缓存、价格索引都放在临时目录），只读运行解析器，分别读取真实 `~/.workbuddy/projects` 与 `~/.workbuddy-ai/projects`。
- 另写一个独立的 Python 只读脚本，按研究口径计算同一批文件的请求数与四桶总和。两边逐应用比对：
  - 请求数等于带用量且有 `messageId` 的行数（截至 2026-10-21 分别约为 2881 与 656，以运行时为准）；
  - 四桶总和完全一致；坏行为 0，或逐条给出原因。
- 只记录计数与总和，不输出任何会话内容；运行前后，真实 `~/.tokenscope` 与两个 WorkBuddy 目录的指纹不变（两个目录可能被应用自身写入，只比对本次读取的文件集合的修改时间）。

### WB09 文档与交付

- **CLAUDE.md**：项目目标中“双 agent 统计”改为四个应用；“候选 agent”一节注明 WorkBuddy 两版已于 2026-10-21 由用户排期；适配器约定补充 WorkBuddy 的口径要点。
- **`docs/stats-semantics.md`**：新增 WorkBuddy 的桶归一、身份、失败调用、子 agent、云端会话等口径。
- **计划总账**：整合计划新增一个责任编号（建议 N17），并同步 `docs/plans/README.md`。
- **版本**：新功能加设置格式变化，递增 **minor**；`SETTINGS_FORMAT_REVISION` 变化须同步 `settings_migrations.json`；合入前按 CLAUDE.md 登记上一发布基线。

## 5. 交付方式与验收

- **一个 PR**，按 WB01 → WB09 逐任务提交，避免发布只完成一半的版本。如果 WB01 的泛化改动过大、审阅困难，可以先单独合入 WB01（行为不变；因为改了产品输入，需要递增一次 patch），再合入其余部分。具体由执行时的改动规模决定，并向用户说明。
- **本地门禁**：
  - `cargo fmt`、clippy、两个成员的测试，均包含普通构建与 acceptance 构建；
  - 前端 typecheck、format、test、build；
  - `python -B -m unittest discover -s scripts -p "test_*.py"`。
- **PR 检查**：`gate`、`native-ui`、Install test 全部成功。
- **合并后**：
  - main 的 Release 中，安装、六个单跳升级与两条连续升级链全部成功，其中升级后重新询问同意的路径由升级脚本核对；
  - 新版本发布后，在本机安装验证：
    - 首次启动弹出“政策已更新”；
    - 同意后四个应用都出现在来源筛选中；
    - WorkBuddy 的数值与 WB08 的对账结果一致。
- **执行记录**：每个任务的提交、红绿摘要、对账结果写入本计划 §6。

## 6. 执行记录

（实施时回填。）
