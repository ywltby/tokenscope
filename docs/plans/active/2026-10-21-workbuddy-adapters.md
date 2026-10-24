# 新增 WorkBuddy（国内版）与 WorkBuddy AI（国际版）用量来源

> 状态：**WB01–WB09 已实施（2026-10-24），PR 待用户复核合并；用户手动安装验证与 main 发布验收在合并后执行。** 分支 `docs/workbuddy-plan` 已合入 main `a527441`（v0.5.13）。§1 保留 2026-10-21 的真实样本记录，当前代码复核见 §1.1；原“等验收收口完成后实施”的串行依赖已由本次授权解除，见 §1.2。整合计划登记为 N17。
> 用户 2026-10-21 决定：
> - 国内版与国际版**分成两个应用**；
> - 隐私政策**增加修订号**，老用户升级后重新询问同意；
> - 原定等 Codex 完成收口计划后再实施；**2026-10-24 用户确认改为本专项独立推进**。
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

代码记录（2026-10-21 的历史时点；当前复核见 §1.1，不把旧行号当实施定位依据）：
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

### 1.1 2026-10-24 当前代码复核

核查基线：main `a527441`，应用 0.5.13；计划分支合入该 main 的提交为 `83e057c`。只读核对仓库代码，没有运行真实来源采集，也未重跑 §1 的真实数据统计；2881 / 656 等数字仍是历史样本，不是本次实测或未来必须凑齐的预期。

| 项目 | 当前事实与实施影响 |
| --- | --- |
| 两应用抽象 | `crates/tokenscope-core/src/model.rs` 的 `AgentKind` 仍只有 ClaudeCode / Codex；`settings.rs:276` 的 `source_config(bool)`、`acceptance.rs:107` 的 `source_dir_override(bool)` 与 `AgentSources { claude, codex }` 仍待 WB01 泛化。`report.rs` 的写死列表在 `1080`、`1267`，按符号查找全部调用方，不按旧行号替换。 |
| WB01 旧测试 | `settings.rs:1101/1109` 的既有测试直接传入 `true/false`。改签名后需要仅适配调用参数；原“全部现有测试无需修改”不能按字面执行，修订边界见 WB01。 |
| 原生统一验收 | PR #31 / v0.5.9 已交付 `ui-identity`、`ui-ccs-import`、`ui-pricing`，入口在 `frontend/scripts/contracts/native-*.mjs`，共用 `frontend/scripts/lib/native-unified.mjs`。WB07 在现有场景追加覆盖，不新建同名场景，不重做 B2。 |
| 验收根保护 | `acceptance.rs` 的验收根拒绝范围目前包括真实 `~/.tokenscope`、`~/.claude`、`~/.codex`。WB04 必须把两个 WorkBuddy 真实目录也纳入拒绝范围，并证明别名路径不能绕过；普通构建仍无覆盖通道。 |
| 隐私接线 | `settings::read_consent` 仍只判断布尔同意；`src-tauri/src/privacy.rs` 已有 `consent_saved` / `business_ready` 追踪接线。WB05 复用它核对“同意原子落盘先于业务读取”，不另建日志体系，不把现有 N10 / P06 标为完成。 |
| 历史核验工具 | `scripts/inspect_history.py:136` 的期望事件 app 白名单只有 `claude-code` / `codex`。WB02 追加两项及反例测试；未知 app 仍失败，不能删除这条校验。 |
| 现有规则版本 | `report::PARSE_RULES_REVISION = 3`、`history::PARSER_REVISION = 2`、`IDENTITY_REVISION = 1`、`SETTINGS_FORMAT_REVISION = 2`。Codex 压缩补计及模型纠正已经合入，WB01 等价基线必须包含这两条路径。 |
| 测试入口 | PR #42 / #46 已交付 `scripts/run_rust_tests.py`；所有 Rust 验证经该入口隔离临时根，测试失败保留证据。不得回退为裸 Cargo 或运行写真实缓存的 `perf_real_data`。 |
| 发布与升级 | v0.5.13 已发布并登记原资产基线；现有发布单跳矩阵为四起点 × present/deleted（八个作业）。未来合入以 `upgrade_baselines.py select` 的实际选择为准，不写死六组或八组。 |

### 1.2 已确认的实施起点

- 用户 2026-10-24 确认本次修订，解除“等验收收口完成后实施”的串行依赖。本专项按 N17 独立推进；验收收口的 B3 待修订，B4 / B5 尚未执行，仍由 N10–N13 的原责任承接，不因此变成已完成。
- 按 §5 的四个内部阶段连续实现，先保持一个产品 PR，避免用户拿到已扩充来源但尚无隐私修订或完整界面的中间版本。若 WB01 确需单独交付，先说明具体规模与额外发布成本，再由用户决定。
- 开始 WB01 前补一次只读样本核查：只读取两版 `projects/**/*.jsonl` 与必要的版本元数据，输出计数、字段类型与四桶合计到忽略的隔离目录；不打开数据库、凭据、traces 或用户代码快照。发现与解析口径相矛盾的新形态，先记录脱敏的字段计数和原因并回到计划复核，不据旧统计强行归一。
- WB01 的接口调用适配、WB02 核验工具白名单追加、WB04 验收根保护补齐及 WB05 旧同意验证顺序均已纳入本次确认的范围；其他既有断言保持原边界。

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

新增行为与修复先写会失败的测试（解析类必须带“合成样例 → 期望事件”的 fixture），在未改的代码上确认断言级红灯，再实现。WB01 是行为不变的重构，按下述基线与变异方式证明断言有效，不把编译错误算作行为红灯。一个提交只解决一类问题。fixture 一律手工合成，不复制真实转写的任何内容。任何构造 `SummaryOptions` 的测试同时注入临时 `cache_dir` 与 `pricing_index`；所有临时目录由创建者核实清理，失败证据放在隔离目录。

实施定位清单（执行前再按符号核对，不因新增枚举顺手改无关实现）：

| 任务 | 主要文件 |
| --- | --- |
| WB01 | `crates/tokenscope-core/src/{model,settings,acceptance,report}.rs`，以及这些接口的现有测试调用方；新增 `crates/tokenscope-core/tests/workbuddy_existing_equivalence.rs` |
| WB02 | `crates/tokenscope-core/src/{model,dedupe,history,report}.rs`、`crates/tokenscope-core/src/import/ccs.rs`、`src-tauri/src/commands.rs`、`scripts/inspect_history.py` 与 `scripts/test_inspect_history.py` |
| WB03 | 新增 `crates/tokenscope-core/src/source/workbuddy.rs`、`crates/tokenscope-core/tests/workbuddy_source.rs`、`crates/tokenscope-core/tests/fixtures/workbuddy/`；接入 `src/source/mod.rs` |
| WB04 | `crates/tokenscope-core/src/{settings,acceptance,report}.rs`、`crates/tokenscope-core/src/source/discovery.rs`、`crates/tokenscope-core/tests/source_paths_collection.rs`、`crates/tokenscope-core/tests/acceptance_closure.rs`、`scripts/fixtures/settings_migrations.json` |
| WB05 | `crates/tokenscope-core/src/settings.rs`、`src-tauri/src/privacy.rs`、`frontend/src/lib/privacyGate.ts`、`frontend/src/components/PrivacyConsentDialog.vue`、`docs/privacy.md`、升级夹具及其 Python / PowerShell 契约 |
| WB06 | `frontend/src/types.ts`、`frontend/src/lib/viewFilters.ts`、`frontend/src/views/{Settings,Dashboard}.vue`、相关 fixture / 契约、`DESIGN.md` |
| WB07 | `scripts/prepare-native-acceptance.ps1`、`frontend/scripts/contracts/native-identity.mjs`、`frontend/scripts/lib/native-unified.mjs`、`frontend/scripts/native-acceptance.mjs` 及相关纯函数测试 |
| WB08–WB09 | 真实对账脚本与忽略的证据目录；`docs/stats-semantics.md`、`CLAUDE.md`、本计划与计划总账；版本与基线由既有脚本生成 |

### WB01 来源层泛化（无行为变化）

- 把 `source_config(claude: bool)`、`source_dir_override(claude: bool)` 及类似的布尔分支改为按 `AgentKind` 取值；`AgentSources` 改为可按 `AgentKind` 访问，但**序列化格式保持不变**（仍是 `claude` / `codex` 两个键）。
- `report.rs` 中写死的应用列表改为来自一处 `AgentKind::ALL`；前端的 `AGENT_IDS` 等暂不改动（放在 WB06）。
- 验收：
  - 既有测试保留所有行为断言；仅允许把旧布尔实参换成同义 `AgentKind`，逐处记录文件与原因，不删除用例、不改阈值、不改期望总量；
  - 先在未泛化代码上为同一份合成 Claude/Codex 输入冻结事件集合、四桶、去重、费用、来源配置与设置序列化的期望，覆盖 Codex 压缩补计和线程设置模型归属；临时交换两应用的配置或验收根映射，确认对应断言会红，再恢复并实施泛化，前后逐项相同；
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
- `AgentKind::parse`、`scripts/inspect_history.py` 与验收汇总中的应用标签映射同时追加两项；先用合成的新 app 期望事件让现有白名单断言红，再实现。未知 app 的拒绝断言保留。
- 版本常量先核对读取方与影响范围，再在 §6 写明是否变化及依据；不能为新增 app 顺手改 Claude/Codex 的身份或触发已有事实纠正。WB04 的设置格式 2→3 已是本计划明确的格式变化，持久指纹与起点选择都必须实测。
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
- `~` 表示当前登录用户的主目录；自动发现使用当前用户的固定候选。界面显示实际解析路径，其他位置经用户手动追加纳入。
- **设置格式**：
  - `sources` 增加 `workbuddy`、`workbuddy_ai` 两个键（缺失即默认：启用加自动发现）；
  - 连同 WB05 的同意修订号，`SETTINGS_FORMAT_REVISION` 升到 3；
  - `scripts/fixtures/settings_migrations.json` 增加 `1->3`、`2->3`、`3->3` 的规则（缺失字段的默认值）。
- **验收隔离**：acceptance 构建的来源覆盖扩展到 `<root>/sources/workbuddy`、`<root>/sources/workbuddy-ai`；验收根拒绝范围追加真实 `~/.workbuddy`、`~/.workbuddy-ai`，覆盖父子包含、规范化别名及 junction 等已有边界。测试使用伪造主目录与隔离根，不把真实目录当可写 fixture。
- **全部测试的来源密闭**：启用两个新默认来源之前，审计所有 `SummaryOptions` 与 `DiscoveryContext` 构造点。旧测试仅设置 Claude / Codex 路径还不够；应为新增来源注入临时缺失根或显式禁用，确保默认发现不会读到真实 WorkBuddy 文件。因新字段而需要调整的既有 fixture 只补密闭配置，原断言保留，并在 §6 列出文件和原因。
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
  - 历史版本夹具保持其原始同意格式；安装新版本后，**先保留旧同意状态并验证会被新闸门拒绝、没有业务读取，再执行当前修订的测试同意**，不能预先补修订号绕过重新询问路径；
  - `upgrade_fixture.py` 的授予同意和确保同意在需要当前修订授权时同时写入修订号，升级链按每一跳的受测版本 / 格式取值，不能把最新修订硬塞给所有历史版本；
  - 升级后的核对分开保存证据：测试同意前旧同意记录原样保留且新程序未进入业务；通过用户同意动作或显式测试授权后，当前修订号同意能进入业务。原有来源路径、设置、四桶和历史事件核对不减；
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
- 复用现有 I/O 追踪、计数代理、`evidence.json` 与 `launches[]`，四个应用的任何读取都必须落在隔离根；出现隔离外路径、外部连接或真实 `~/.tokenscope` 指纹变化时立即停止原生运行并报告。
- 修改过的原生场景各在固定提交本地连续通过三次并记录耗时；原阈值、超时和必需检查不放宽，CI 时长增长先测量再决定是否接入。若需要加等待补偿、改 CSP 或放宽断言，先停下修订计划。

### WB08 真实数据只读对账（本地）

- 用隔离输出目录（历史库、缓存、价格索引都放在临时目录），只读运行解析器，分别读取真实 `~/.workbuddy/projects` 与 `~/.workbuddy-ai/projects`。
- 另写一个独立的 Python 只读脚本，按研究口径计算同一批文件的请求数与四桶总和。两边逐应用比对：
  - 请求数等于带用量且有 `messageId` 的行数（截至 2026-10-21 分别约为 2881 与 656，以运行时为准）；
  - 四桶总和完全一致；坏行为 0，或逐条给出原因。
- 只记录计数与总和，不输出任何会话内容；运行前后，真实 `~/.tokenscope` 与两个 WorkBuddy 目录的指纹不变（两个目录可能被应用自身写入，只比对本次读取的文件集合的修改时间）。

### WB09 文档与交付

- **CLAUDE.md**：项目目标中“双 agent 统计”改为四个应用；“候选 agent”一节注明 WorkBuddy 两版已于 2026-10-21 由用户排期；适配器约定补充 WorkBuddy 的口径要点。
- **`docs/stats-semantics.md`**：新增 WorkBuddy 的桶归一、身份、失败调用、子 agent、云端会话等口径。
- **计划总账**：维护已登记的 N17，随实施和交付同步整合计划与 `docs/plans/README.md`。
- **版本**：新功能加设置格式变化，递增 **minor**；`SETTINGS_FORMAT_REVISION` 变化须同步 `settings_migrations.json`；合入前按 CLAUDE.md 登记上一发布基线。

## 5. 交付方式与验收

- **一个 PR**，按 WB01 → WB09 逐任务提交，避免发布只完成一半的版本。如果 WB01 的泛化改动过大、审阅困难，可以先单独合入 WB01（行为不变；因为改了产品输入，需要递增一次 patch），再合入其余部分。具体由执行时的改动规模决定，并向用户说明。
- **内部实施阶段**（按 §1.2 已确认的独立排期执行；阶段完成不等于已获合并批准）：

  | 阶段 | 范围 | 进入下一阶段的证据 |
  | --- | --- | --- |
  | A 现有来源等价 | WB01；先冻结双来源结果与设置字节，再做变异与接口泛化 | 两种 Rust 构建通过，配置 / 来源未错接，原断言保留；旧接口测试仅作同义调用适配 |
  | B 新来源核心与同意 | WB02–WB05；身份、解析、发现、设置格式及隐私修订 | 合成 fixture 断言级红绿；四应用互不双计；旧同意在新版本下零业务读取，新同意原子落盘后才解锁 |
  | C 界面与验收装配 | WB06、WB07 的代码与纯函数测试 | 四应用界面、项目归并、原生检查函数及隐私升级场景先红后绿；现有用例不减 |
  | D 固定受测物交付 | WB09 取号后构建固定产物，完成 WB07 原生三连跑、WB08 真实只读对账与文档回填 | 同一受测提交与资产哈希对应全部证据，PR 完整门禁通过；用户决定合并后才执行 main 发布验收与归档 |

- **取号**：本次新功能按 minor；若合入时 main 仍是 0.5.13，预期为 **0.6.0**。先 fetch 并核验上一发布基线，再调用版本工具，同一产品 PR 只取号一次；本草案自身不取号。D 阶段的最终原生与真实对账都使用取号后的同一受测提交，不借用旧二进制的结果。
- **本地门禁**：
  - `cargo fmt --all -- --check`；`cargo clippy --locked --workspace --all-targets -- -D warnings`，另跑带 `--features acceptance` 的同一命令；
  - 普通测试用 `python -B scripts/run_rust_tests.py -- cargo test --locked --workspace`，另跑带 `--features acceptance` 的同一入口；定向测试也经过该包装；
  - `pnpm --dir frontend typecheck`、`format:check`、`test`、`build`、`test:e2e`；
  - `python -B -m unittest discover -s scripts -p "test_*.py"`；
  - 原生三连跑示例：`node frontend/scripts/native-acceptance.mjs --scenario ui-identity --events 8 --port 9500 --out qa-artifacts/workbuddy/identity-run1`，各次使用独立输出目录；`ui-source-paths` 与新增的隐私修订场景同样记录提交、产物哈希、命令与耗时，场景名以 WB05 最终登记为准。
- **PR 检查**：`gate`、`native-ui`、Install test 全部成功。
- **合并决定**：完成十条不变量自审及全部门禁后交用户复核，不自行合并，不改变现有 CI 分档、必需检查、CSP 或超时策略。
- **合并后**：
  - main 的 CI、Native UI E2E 与 Release 均需实际成功；Release 中五场景安装、`upgrade_baselines.py select` 实际选出的全部单跳与两条连续升级链成功，其中升级后重新询问同意的路径由升级脚本核对；不写死单跳作业数；
  - 自动化安装 / 卸载 / 升级只在 GitHub-hosted 一次性 Runner 执行。新版本发布后，交用户在本机手动安装验证，执行模型不在本机自动运行安装脚本或改真实数据目录：
    - 首次启动弹出“政策已更新”；
    - 同意后四个应用都出现在来源筛选中；
    - WorkBuddy 的数值与 WB08 的对账结果一致。
  - 用户手动验证若暂不执行，按归档规则明确保留待验并由整合计划承接，不写成全验收通过。
- **执行记录**：每个任务的提交、红绿摘要、对账结果写入本计划 §6。

## 6. 执行记录

### 6.1 2026-10-24 分支准备与代码复核

- 用户要求清理已合并分支并切到 `docs/workbuddy-plan` 研究实施顺序。已删除远端 `docs/test-temp-cleanup-b1-closeout`（PR #45）与 `test/rust-temp-cleanup`（PR #43），删除前核对其 tip 已合入 main；旧 worktrees 目录核对为空后删除。保留当前主工作树与 WorkBuddy 计划分支。
- `83e057c` 合入 main `a527441`，无冲突，保留原计划提交 `aae449f`；当前产品版本仍为 0.5.13。此合并是分支同步，不是 WB01–WB09 的产品实现。
- `dbfb051` 澄清 README 的当前用户默认路径、环境变量与手动追加含义，与设置页现有实现一致；只补正向说明，文档提交钩子通过。
- 复核补齐两应用旧接口的测试调用、真实来源根拒绝范围、历史期望事件白名单、已上线统一原生场景及当前解析 / 设置版本，见 §1.1；修订后的实施安排见 §5。
- `bump_version.py --check --base-ref origin/main` 实测版本 0.5.13→0.5.13，`release: false`；离线运行 `upgrade_baselines.py select --current-version 0.6.0` 选出 0.1.0 / 0.4.0 / 0.5.9 / 0.5.13 四起点、八个单跳项。后者只是当前登记表的规划结果，不能代替未来 0.6.0 的构建或发布验收。
- 初次准备时只核对代码并修订文档，没有运行真实 WorkBuddy 采集或原生场景；旧样本数字未标为复核通过。当时尚待用户确认文本和前置排期，未分配新 N 编号；其后授权与 N17 登记见 §6.2，N01–N16 的历史状态保持。

### 6.2 2026-10-24 用户确认修订与独立排期

- 用户确认本次修订，并允许 WorkBuddy 独立推进；顶部状态与 §1.2 已据此更新，整合计划登记 N17，计划总账同步。
- 原验收收口 B3 的两项前提差异继续待修订；B4 真实规模性能和 B5 单实例 / 人工签字尚未执行。它们与 N17 分别记账，本次授权不修改 AC06 / AC07 的方案或替代人工签字。
- 本次仍只修改计划，WB01–WB09 的产品实现尚未开始；功能 PR 的合并继续由用户决定。

### 6.3 产品实施记录（2026-10-24 实施；PR 待用户复核）

分支 `feat/workbuddy-adapters`（自 `docs/workbuddy-plan` e35fe26 新建）。逐任务提交与证据摘要如下；除注明外全部 Rust 验证经 `scripts/run_rust_tests.py`，测试临时根清理核实。

| 任务 | 受测提交 | 红灯 / 变异 | 绿灯证据 |
| --- | --- | --- | --- |
| WB01 泛化 | `2a6e8c2` | 新增 `tests/workbuddy_existing_equivalence.rs`（管线冻结 / 事件快照 / 设置字节三测）先在旧实现冻结；变异：`source_config` 交换两应用分支 → 设置断言红；`make_sources` Claude 分支错接 Codex 解析器 → 事件快照红（均断言级，恢复后泛化） | 普通 + acceptance 两轮 workspace 全绿；既有测试仅布尔实参适配为同义 AgentKind |
| WB02 枚举/身份/去重 | `3135746` | 身份三断言在未实现态红（native_identity 返回 None、WorkBuddy 暂借 Codex 指纹规则）；`inspect_history.py` 白名单先红后绿 | 历史库往返与跨应用隔离测试；版本常量核对后不递增（见下） |
| WB03 解析器 | `41f4c7d` | 零行为骨架跑出 10 个断言级红灯 | `workbuddy_source.rs` 13 用例全绿（fixture 手工合成 + json 自检） |
| WB04 来源与设置 v3 | `04a59cf` | 迁移表缺 1->3 规则红；default 语义「缺键拒绝」红（改为缺失与默认值等价） | 迁移表 1->3/2->3/3->3；发现层四用例；验收根保护扩两真实目录 + 大小写别名；96 处 SummaryOptions 构造点注入禁用 |
| WB05 隐私修订 | `ddd84dc` | 修订号六分支语义红（缺号/偏低/等于/高于/false/类型错误）；原子落盘与损坏文件封闭红 | `read_consent` 只认不低于当前的修订；accept 同一次写 true+修订号；前端弹窗提示行 |
| WB06 界面 | `0e9a4a1` | —（UI 契约） | 五图标契约 9 断言 × 四视口（明暗 × 1280x820/980x620）通过、零违例；自绘徽标用 path 不用 `<text>`（保住「纯图标无文字」契约） |
| WB07 原生验收 | `d8c3039` + `9a32dc3` | prepare fixture 转义污染（BEL/TAB）三次修正；clickAgent 过渡态等待收紧 | 见下方三连跑 |
| WB08 对账 | `9a32dc3`（解析器实现主体在 `41f4c7d`） | 对账测试自身的 events 计数 bug（add_file 契约） | Rust 与独立 Python 逐应用逐字段一致：国内版 2,881 条 / 国际版 656 条，合计 in 146,055,367 / out 3,300,068 / cw 0 / cr 619,000,607，坏行 0；证据 `qa-artifacts/workbuddy/{rust-reconciliation.json,reconciliation.json}` |
| WB09 取号 | `324e739` | — | `bump_version.py --bump minor`：0.5.13 → 0.6.0；`--check --base-ref origin/main` = release: true；v0.5.13 基线已登记 |

**原生三连跑（最终受测提交 `9a32dc3`）**：三场景各三次（`qa-artifacts/workbuddy/{identity,sp,policy}-final{1..3}`），exe 在脚本修正提交后重建；九次全部通过、`real_untouched=true`、外部连接 0、CSP 违例 0、各两次 launch：

- `ui-identity`：19/19 × 3（34 s / 32 s / 32 s）。
- `ui-source-paths`：17/17 × 3（36 s / 37 s / 37 s）。
- `privacy-policy-update`：12/12 × 3（11 s / 12 s / 11 s）。

首轮尝试的失败均先取证后修：fixture 转义污染（BEL/TAB）、`matchEventDom` 应用列缺新映射（产品级发现）、`clickAgent` 过渡态等待与「全部」IPC 实参匹配、privacy-policy-update 的 mutate 分支与业务态窗口优雅关闭——每次修正后重跑，不以重跑代替原因；取号前构建（0e9a4a1）上的三次首跑证据保留但不计入三连。

**版本常量决策（WB02 核对结论）**：`HISTORY_SCHEMA_VERSION=4`、`PARSER_REVISION=2`、`IDENTITY_REVISION=1` 均不递增——历史库 `usage_events.app` 为 TEXT、表结构不变；新增适配器不改变 Claude/Codex 的解析语义与模型身份规则；新事件以当前 PARSER_REVISION 标记写入，读取方（升级矩阵按 persistent_format 分组挑起点）不受影响。`SETTINGS_FORMAT_REVISION` 2→3 属计划明确的变化（sources 新键 + 同意修订号），迁移规则与 `settings_migrations.json` 同步。

**十条不变量自审**：

1. 现有统计零变化：WB01 冻结测试（管线/事件快照/设置字节）在泛化前后逐项相同；两轮 workspace 全绿。
2. 一次调用只计一次：仅 rawUsage+messageId 行产事件（解析器门槛）；三容器只取 rawUsage。
3. 桶互斥：input=prompt−cached；cached 只取 details.cached_tokens（回退 hit）；受检算术坏行跳过；WB08 对账坏行 0。
4. 不补造请求：失败调用、无 messageId 用量行、摘要行均不产事件；无模型计 skipped_no_model。
5. 两应用独立：身份键含 agent；dedupe 强身份 map 按 agent 分桶；跨应用同身份不合并（测试钉住）。
6. 重扫幂等：同文件重扫事件一致；历史库来源键幂等由既有机制承接（WB 测试写入读回验证）。
7. 只读最小采集面：只遍历 projects/ 下 *.jsonl；验收根保护覆盖真实 `~/.workbuddy{,-ai}` 与大小写别名；子进程密闭检查四应用默认根。
8. 同意先于读取：修订号低于当前 → StaleRevision → NeedsConsent；privacy-policy-update 场景验证旧同意零业务文件、同意原子落盘后解锁。
9. 项目身份一致：WB 与 Claude 事件同路径同 key（跨应用对照测试）；子目录归根、越界切换；原生场景四应用合并断言通过。
10. 时间口径：毫秒时间戳 UTC 一次转换；跨日 Asia/Shanghai 测试钉住。

**偏离与缺陷登记**：无计划外语义变更。实施中发现并修复：验收 fixture 生成脚本经工具层转义污染（BEL/TAB，三轮修正）；`matchEventDom` 应用列显示名缺新应用映射（产品级发现，测试库补齐）；`clickAgent` 过渡态等待条件过松；本机中文控制台代码页导致升级契约测试假失败（`10c90fc`，显式声明子进程 UTF-8 输出，不放宽断言）。`ui-source-paths` 纳入 unified profile（计划 WB07 授权范围：复用统一 fixture）。

**待验（不因本 PR 标完成）**：用户手动安装验证（首启「政策已更新」弹窗、四应用出现在来源筛选、WorkBuddy 数值与 WB08 一致）按 §5 交用户执行；B3/B4/B5 继续由 N10–N13 原责任承接。
