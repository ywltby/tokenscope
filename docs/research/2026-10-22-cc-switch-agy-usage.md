# cc-switch v4.0.18 的 Antigravity（agy）用量接入拆解

> 研究目标：cc-switch 在 v4.0.18 把 **Antigravity CLI（命令 `agy`）** 接入为第 11 个应用，
> 本文拆解它**怎么识别 agy**、**token 用量怎么算**、**怎么保存**、**怎么统计**。
>
> 样本：`farion1231/cc-switch` @ `cb90a244`（2026-10-21，`package.json` 版本 **4.0.18**），
> 浅克隆只读分析，未运行其二进制、未改动任何本机数据。
>
> 证据分档：**已验证**（源码/仓库文档可证）／**未验证推论**。
> 本文只记录字段名、表名、路径规则与算法，不含任何真实会话内容。

## 1. 结论摘要

- **识别方式**：固定路径 + 多级探测，不做注册表/进程扫描。应用身份是独立的 `AppType::Agy`，
  与 Gemini CLI 共用表单结构但分开管理（路径、开关、会话、用量互不混用）。
- **token 用量**：**直取** agy 导出的 `transcript_full.jsonl` 里"已完成模型响应"的四个 token
  字段，**不做累计差分**；语义被明确标成 **FRESH**（input 不含缓存、output 含 thinking），
  所以统计侧**不再扣缓存**。
- **保存**：写入统一的 `proxy_request_logs`（`app_type='agy'`、`data_source='agy_session'`），
  另用 `session_usage_dedup` 墓碑账本做幂等；**没有文件级增量游标**，每轮全量扫会话 transcript。
- **统计**：复用通用聚合链路；但它**不在**跨源去重白名单里，也**不在**缓存包含白名单里——
  这两点是接入新来源时最容易漏的地方。

## 2. 怎么识别 agy

### 2.1 应用身份（已验证）

- 独立枚举 `AppType::Agy`：`src-tauri/src/app_config.rs` 的 `pub agy: bool`（第 13 行）与
  `AppType::Agy => self.agy`（第 44 行）、启用列表（第 83 行）。
- 数据库版本 **20 → 21**，只给 `mcp_servers`、`skills` 各加一列
  `enabled_agy BOOLEAN NOT NULL DEFAULT 0`（`docs/agy-integration-zh.md:53`）。
- 用量侧占位供应商 `_agy_session` → 展示名 **"Antigravity (Session)"**
  （`src-tauri/src/services/usage_stats.rs:389`）。
- 前端用量页的应用筛选白名单已包含 `"agy"`（`src/types/usage.ts:227-236` 的 `KNOWN_APP_TYPES`）。

### 2.2 配置路径（已验证）

| 用途 | 路径 | 证据 |
| --- | --- | --- |
| 实时配置（真实） | `~/.gemini/antigravity-cli/settings.json` | `agy_config.rs:31-42`（`agy_dir()` / `settings_path()`） |
| 全局定制目录 | `~/.gemini/config`（MCP `mcp_config.json`、Skills、提示词） | `agy_config.rs:36-38` |
| Key 模式私有 gemini_dir | `~/.cc-switch/agy/<供应商>/` | `agy_config.rs:45-53`；其内 `antigravity-cli/settings.json` 在第 224 行 |
| 旧版包装脚本标记 | `cc-switch-agy-wrapper` | `agy_config.rs:444` |

关键设计（**已验证**，来自 `docs/agy-integration-zh.md:38-43`）：agy **只从进程环境**
读 `GEMINI_API_KEY` / `GOOGLE_GEMINI_BASE_URL`，不读任何文件里的 Key。因此：

- 官方登录卡：只改真实 `settings.json` 的 `model`，并去掉 `modelProvider`；
- 第三方 Key 卡：不写真实配置，改用 `agy --gemini_dir=<私有目录>` 启动，Key 只交给这一个进程
  （Unix 走 0600 临时文件 + `env`，读完即删；Windows 走 `.bat` 的 `setlocal`）。

### 2.3 可执行文件定位（已验证）

`real_agy_bin()`（`agy_config.rs:335-402`）的探测顺序：

1. `PATH` 里各目录；
2. `~/.local/bin`；
3. 平台目录：Windows `%LOCALAPPDATA%\agy\bin`（`install.ps1` 的安装位）；
   Unix `/opt/homebrew/bin`、`/usr/local/bin`；
4. 仍未命中时，用**登录 shell** 跑 `command -v agy`（`$SHELL -l -c`，限时 2 秒）。

候选名：Windows `agy.exe` / `agy.cmd` / `agy`，Unix `agy`。判定还要求"是文件、有执行位（Unix）、
且**不是旧版 CCS 包装脚本**"（`is_real_binary`，第 387-402 行；包装标记在第 444 行）。

### 2.4 会话与日志（已验证）

`session_manager/providers/agy.rs:28-44` 的三个根：

- `~/.gemini/antigravity-cli/brain`（CLI）
- `~/.gemini/antigravity/brain`（Antigravity IDE，布局相同）
- `~/.cc-switch/agy/<供应商>/antigravity-cli/brain`（每个 Key 供应商一份）

每个根下：

- 同级 **`conversation_summaries.db`**（只读 SQLite，列
  `conversation_id, title, workspace_uris, preview, last_modified_time`，第 159 行）——用于列表/标题，
  **不承载 token**；
- 会话目录 `<sessionId>/`，正文在 **`.system_generated/logs/transcript_full.jsonl`**（第 108-110 行）。

识别规则（`scan_root`，第 85-150 行）：

- `sessionId` = 目录名，必须非空且只含 ASCII 字母数字与 `-`（`valid_session_id`，第 81-83 行）；
- 该 jsonl 必须存在，且 `canonicalize()` 后仍在根内（防链接越界，第 111-119 行）；
- 跨根同名会话只算一次（first wins，第 68-79 行）；
- **有 transcript 就一定给 `source_path`**（第 143 行无条件赋值）；只有二进制记录、没有可读日志的会话
  会以 `source_path: None` 留在列表里（第 148 行），用量导入时被跳过——不伪造正文。

解析：`records()` 逐行 `serde_json`，按 `step_index` 收进 `BTreeMap`，**同步骤后写覆盖前写**
（应对 agy 边写边改），坏行跳过（第 46-61 行）。

## 3. token 用量怎么算

### 3.1 只取"完成的模型响应"（已验证）

`services/session_usage_agy.rs:131`：只处理
`type == "PLANNER_RESPONSE"` **且** `status == "DONE"` 的记录。未完成步骤不计数，也**不消耗去重 ID**
（测试 `agy_incomplete_steps_do_not_consume_dedup_ids`，第 293-301 行）。

### 3.2 四个 token 字段直取，语义标 FRESH（已验证）

```rust
let input = token("input_tokens");
let output = token("output_tokens");
let cache = token("cache_read_tokens");
let write = token("cache_write_tokens");
if input == 0 && output == 0 && cache == 0 && write == 0 { continue; }   // 第 147 行
```

写入时 `input_token_semantics` 固定为 `INPUT_TOKEN_SEMANTICS_FRESH`（第 179 行的 `?9`）。
官方文档给了实测口径（`docs/agy-integration-zh.md:69-73`）：输入 100 / 缓存 20 / 回答 5 / 思考 7 时，
agy 导出为 **input=80、cache_read=20、output=12**——即

- **input 已排除缓存** → 统计侧**不再减** cache_read；
- **output 已包含 thinking** → **不再加**思考 token。

代码注释与单测把这条钉死（第 170-172 行注释；`agy_native_fixture_preserves_fresh_input_and_combined_thinking_output`
断言 `(80, 12, 20, FRESH)`，第 191-201 行）。

### 3.3 模型从哪来（已验证）

响应行本身**通常没有模型字段**，cc-switch 用两条路：

1. **行内 `model`** 优先（第 154-155 行）；
2. 否则从 `USER_INPUT` 的文本里抠：agy 会把切换写成
   ``changed setting `Model Selection` from X to Y``，按 `step_index` 顺序继承"当时最近一次切换"
   （第 116-125 行）。提取器 `model_from_settings_change` 只把"句号+空格"当句子结束
   （因为模型名里本身有点，如 `Gemini 3.8`），换行或反引号也截止（第 39-56 行）；
3. 都没有 → `"unknown"`（第 158 行），**绝不从当前供应商反推历史模型**（文件头注释第 1 行）。

模型名归一（`model_id_of`，第 60-71 行）：显示名转成定价表认的 ID——

`Gemini 3.8 Flash (High)` → `gemini-3.8-flash-high`；`Gemini 4 Argon (Low)` → `gemini-4-argon-low`；
已经是 ID（无空格）的原样返回；`unknown` 保持 `unknown`（测试第 258-291 行）。

`model` 列存 **ID**（便于对定价表与跨应用统计），`request_model` 列存**显示名**（第 153 行注释）。

### 3.4 金额（已验证）

```rust
match find_model_pricing(conn, model) {           // 第 86 行
    Some(pricing) => CostCalculator::calculate_for_app("agy", &usage, &pricing, Decimal::from(1)),
    None => 五项全 "0",
}
```

- `calculate_for_app("agy", …)`：`agy` **不在** `CACHE_INCLUSIVE_APP_TYPES`
  （`services/sql_helpers.rs:23` 只有 `codex/gemini/grokbuild`），所以按 FRESH 语义**不扣缓存**再计价。
- 定价走通用两阶段匹配（精确 → 分隔符边界前缀回退，见既有研究文档）。
- 查不到价 → 成本 0，官方文档明确要求页面解释"**0 不代表免费**"
  （`docs/agy-integration-zh.md:77`）。
- 单价是表内 API 价；**不是** agy 的实际账单（原生日志没有账单信息）。

## 4. 怎么保存

### 4.1 落库位置与字段（已验证）

写统一的 `proxy_request_logs`（`session_usage_agy.rs:173-179`），关键列值：

| 列 | 值 |
| --- | --- |
| `request_id` | `agy:{session_id}:{step_index}`（第 150 行） |
| `provider_id` | `_agy_session` |
| `app_type` | `agy` |
| `model` | 归一后的模型 ID |
| `request_model` | 显示名（或 `unknown`） |
| `input_tokens` / `output_tokens` / `cache_read_tokens` / `cache_creation_tokens` | 直取（cache 写进 `cache_creation_tokens`） |
| `input_token_semantics` | `2`（FRESH） |
| `latency_ms` | `0` |
| `first_token_ms` / `duration_ms` / `error_message` / `pricing_model` | 不写（NULL） |
| `status_code` | `200` |
| `session_id` | agy 的会话 id |
| `provider_type` / `data_source` | `agy_session` |
| `is_streaming` | `1` |
| `cost_multiplier` | `'1'` |
| `created_at` | `agy::timestamp(row) / 1000`（毫秒转秒；`timestamp` 取记录的 `created_at` 字段，`providers/agy.rs` 内 `pub(crate) fn timestamp`） |

### 4.2 幂等：请求 ID + 墓碑账本（已验证）

- 先写账本：`INSERT OR IGNORE INTO session_usage_dedup(data_source, request_id, semantic_id, has_entry_id)`
  值 `('agy_session', 'agy:{session}:{step}', 同 request_id, 1)`（第 152 行）；
- 账本写入成功（`fresh != 0`）→ 插明细；否则 → 走**更新**分支（第 162-169 行）。
- **为什么要墓碑**：明细行 30 天后会被 `rollup_and_prune` 汇总并删除，而 agy 没有游标可依赖；
  账本独立于明细存活，因此"已剪枝的历史不会被重新导入"（第 151 行注释 + 测试
  `agy_usage_is_exact_deduplicated_and_survives_retention`，第 224-238 行：删光明细后重导仍 `skipped`）。

### 4.3 没有文件级增量游标（已验证）

`session_usage_agy.rs` 全文**没有** `session_log_sync` / `update_sync_state` / cursor 相关调用
（全文件 grep 无命中）。`sync_agy_usage` 每轮直接
`for session in agy::scan_sessions()` 再逐个 `records()` 解析（第 14-35 行）。

含义与代价：

- 好处：实现简单，文件被改写/截断也能自愈，不必处理游标失效；
- 代价：**每轮（默认 60 秒）全量解析所有 agy 会话的 transcript**，会话多或文件大时是热路径；
  真正的"省"来自 `session_usage_dedup` 的 `INSERT OR IGNORE`（不重复落明细），而不是少读文件。

### 4.4 改写、失败与保留期（已验证）

- **已完成步骤被改写**（agy 会重写 transcript）：只 UPDATE 仍保留的明细，并对计数变化做条件判断
  （第 165 行）；已剪掉的行靠墓碑拒绝复活。
- **未完成步骤**不写账本、不计数（第 293-301 行）。
- **解析失败**按会话记进 `errors`，不影响其他会话（第 27-32 行）。
- **事务**：整会话的记录在同一事务里提交（第 128、182 行）。
- **保留期**：明细同样受全局 30 天 `rollup_and_prune` 支配，汇总进 `usage_daily_rollups`。

## 5. 怎么统计

### 5.1 调度与归属（已验证）

- `services/session_usage.rs:169-174` 在串行调度里新增一步：`"Antigravity"`（排在 MCode 之后），
  失败只进 `errors`。
- 展示名走 `provider_name_coalesce`：`_agy_session` → "Antigravity (Session)"
  （`usage_stats.rs:389`），因此它像其他会话来源一样出现在"供应商"维度里。

### 5.2 **不在跨源去重白名单**（已验证，重要）

`usage_stats.rs:521-522` 的去重来源列表仍然只有四个：

```rust
const DEDUP_SESSION_SOURCES_SQL: &str =
    "'session_log', 'codex_session', 'gemini_session', 'opencode_session'";
```

`agy_session` **不在其中**，所以 agy 的会话行不参与"代理行 vs 会话行"的 ±10 分钟指纹去重。
当前这没有造成双计——因为 cc-switch **没有给 agy 做本地代理**（官方文档第 98 行明确不提供
agy 代理/故障转移）。但这意味着：**一旦将来给 agy 加代理记账，必须把它加进这个列表**，否则同一请求会被记两次。

### 5.3 不做缓存扣减（已验证）

`CACHE_INCLUSIVE_APP_TYPES = ["codex", "gemini", "grokbuild"]`（`sql_helpers.rs:23`）不含 `agy`，
与写入侧的 FRESH 语义一致：`fresh_input_sql` 对它原样返回，`real_total_tokens` 由
`fresh_input + output + cache_read + cache_write` 得出（缓存四桶各计一次，不重复扣）。

### 5.4 成功率、速度与"未定价"（已验证）

- **成功率恒 100%**：所有行 `status_code=200`，聚合层的成功率按 2xx 计。
- **不参与速度统计**：`latency_ms=0`、`first_token_ms=NULL`——精确速度要求
  `first_token_ms` 非空，估算速度要求 `latency_ms ≥ 1000ms`，两条都不满足。
- **成本可能为 0**：模型 `unknown` 或定价表未收录时五列全 0；前端 `isUnpricedUsage`
  （2xx + 有 token + 倍率非 0 + 成本 0）会把它标成"**未定价**"，不是"$0.00"。

### 5.5 汇总与日粒度（已验证）

agy 的行和别的会话来源完全同构，因此自动进入：

- `get_usage_summary` / `by_app` / `provider` / `model` / `trends` 的"明细 + 日汇总"相加；
- 30 天后被 `rollup_and_prune` 汇总进 `usage_daily_rollups`
  （`date` 按 `created_at` 的本地自然日，`input_token_semantics` 归一为 FRESH）；
- 汇总后失去逐步骤明细，只能按 `(date, app_type, provider_id, model, request_model, pricing_model)` 看总量。

## 6. 已知限制（官方文档 + 代码，已验证）

- **供应商不写进 agy 自身配置**：第三方卡只在"使用此供应商打开终端"启动的 agy 里生效；
  其他终端直接跑 `agy` 仍是官方登录。
- **不做代理/故障转移/Stack**（`docs/agy-integration-zh.md:98`）——这也是它不需要进跨源去重白名单的原因。
- **原生日志没有历史模型与真实账单**：模型缺失记 `unknown`、成本只能是表价估算。
- **OAuth 凭据与订阅额度不托管**（macOS 授权中心的多账号管理是另一条链路）。
- **不删除 agy 会话**（正文/二进制/索引需由 agy 一起维护）。
- **每轮全量解析 transcript**（无游标），会话多时是固定开销。

## 7. 对 TokenScope 的启示

1. **"固定路径 + 多根去重"比"扫描磁盘"稳**：三个 `brain` 根、跨根同 id 只算一次，
   和我们现有的"固定候选集 + 最小规范化根认领"是同一思路。
2. **语义必须在写入侧一次性钉死**：agy 把"input 不含缓存、output 含 thinking"写进注释、单测和文档三处，
   统计侧才敢不扣缓存。TokenScope 也应把这类语义作为适配器的**契约测试**，而不是靠展示层补丁。
3. **没有游标时，墓碑账本是必要补件**：agy 靠 `session_usage_dedup` 让"全量重扫"安全，
   并保证剪枝后不复活。TokenScope 的 history.db 已有事件身份，但"来源文件删除/重扫不删已存用量"
   这条等价保障同样需要在设计里显式写清。
4. **模型名可能只存在于文本里**：agy 从 `Model Selection` 那句话里抠模型，这是"日志没有结构化字段"
   时的现实做法；我们接入类似来源时应准备"文本提取 + 明确 unknown 兜底"，不要从当前配置反推。
5. **新增来源要更新三张白名单**：缓存包含语义、跨源去重来源、前端应用筛选。agy 只更新了后两者中的
   前两项中的两项（前端加了、去重没加，因为它没有代理面）——这类"白名单是否该加"必须有明确理由，
   否则就是静默错账。

## 8. 未验证项（不得外推）

- 未运行 cc-switch 或 agy；结论全部来自源码与仓库文档。
- `transcript_full.jsonl` 的字段在真实 agy 版本间是否稳定、`created_at` 是否总是毫秒，
  本文只依据代码中的读取方式与合成 fixture（`tests/fixtures/sessions/agy.jsonl`）。
- `conversation_summaries.db` 的 schema 版本演进未核；代码对上表缺失时已做容错（读不到就不建索引）。
- 未实测 Windows 上 `--gemini_dir` 私有目录的目录联接（junction）行为与 `%LOCALAPPDATA%\agy\bin` 探测。
- 30 天汇总后"按步骤去重"是否仍与 agy 侧的会话改写一致，需要真实长周期数据验证。

---

**样本与检索入口**：`src-tauri/src/agy_config.rs`、`src-tauri/src/services/session_usage_agy.rs`、
`src-tauri/src/session_manager/providers/agy.rs`、`docs/agy-integration-zh.md`、
`src-tauri/src/services/usage_stats.rs`、`src-tauri/src/services/sql_helpers.rs`、
`src/types/usage.ts`。
