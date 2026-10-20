## 5. 45 家 provider：逐家读取方式

本章只回答一个问题：**TokenTracker 到底从哪里、以什么格式、按什么口径把各家的 token 读出来**。所有结论都锚定 `src/lib/rollout.js`（`parse*Incremental` 的唯一真源，共 45 个函数定义，行号区间 `251`–`24458`）与各家的专用模块（`sqlite-reader.js`、`cursor-config.js`、`trae-*.js`、`devin-usage.js`、`grok-usage.js`、`dumate.js`、`install-resolver.js`）。读者可只看 §5.1 的总表，再按需下钻；§5.6 是给下游（TokenScope 侧）的价值最高的一节。

**行号口径**：全部为文件实际行号（UTF-8、按 `\n` 分行，与编辑器 / `read` 工具一致）。本仓库有多行以多字节字符结尾，PowerShell 5.1 的 `Get-Content`（不指定编码）按 GBK 解码会吞掉换行，导致行号最多漂移约 15 行——**不要用 `Get-Content` 复核本章行号**，请用 `[System.IO.File]::ReadAllLines(path,[Text.Encoding]::UTF8)`。

---

### 5.1 总表（最重要）

**读表说明**：`source` 列写的是**真正进队列桶键的字面量**（`getHourlyBucket` / `bucketKey` 的 `source` 段），不是 provider 显示名——两者有 6 处不一致，见 §5.1.1。「hook/插件」列只回答「有没有主动上报通道」；**没有任何一家把 hook 当作解析前提**：hook 只让同步更早发生（`src/lib/passive-mode.js:4` 起模块头明确写「解析器可以直接读那些日志，唯一损失只是‘数据在数秒内是新鲜的’」），唯一例外是「上游要用户先开导出才有文件可读」的 Copilot OTEL 与 trae-cn 的强制 opt-in（后者另计）。

| # | 显示名 | `source` 字面量 | 读取方式 | 数据路径 | hook / 插件 | 联网 / 凭据 | 关键字段映射（一句话） |
|---|--------|------------------|----------|----------|-------------|-------------|------------------------|
| 1 | Gemini CLI | `gemini` | 整份 JSON（`session-*.json`） | `~/.gemini/tmp/<project>/chats/session-*.json`（`GEMINI_HOME` 可覆盖） | 有（`gemini-config.js` 的 hook，仅新鲜度） | 离线 | `tokens.input`/`cached`/`output`+`tool`/`thoughts`；`total=max(上报, 分桶和)` |
| 2 | OpenCode（磁盘消息支路） | `opencode` | 单消息 JSON 目录 | `$OPENCODE_HOME`（默认 `$XDG_DATA_HOME/opencode`）`/storage/message/**/msg_*.json` | 无 | 离线 | `tokens.input`/`output`/`reasoning`/`cache.read`/`cache.write`，input 已是纯非缓存 |
| 3 | OpenClaw | `openclaw` | JSONL 转录（+ agent SQLite） | `~/.openclaw/agents/<agent>/sessions/*.jsonl` | 有插件（`openclaw-session-plugin` / `openclaw-hook`） | 离线 | `message.usage.input`（先减 `cacheRead`）/`cacheRead`/`cacheWrite`/`output` |
| 4 | Kiro（IDE） | `kiro` | SQLite（另有 JSONL 兼容支路） | `%APPDATA%\Kiro\User\globalStorage\kiro.kiroagent\dev_data\devdata.sqlite` | 无 | 离线 | `tokens_prompt`→input、`tokens_generated`→output；缓存/推理列恒 0 |
| 5 | Hermes | `hermes` | SQLite | `~/.hermes/state.db`（Windows 另探 `%LOCALAPPDATA%\hermes`；含 `profiles/*/state.db`） | 无 | 离线 | 列名即桶名；`message_count` 差分 → `conversation_count` |
| 6 | Kiro CLI | `kiro`（与 IDE 合并） | SQLite（字符估算）+ legacy 会话 `.json`（真实值）+ v2 `messages.jsonl`（估算） | `~/.kiro/kiro-cli/data.sqlite3` + `~/.kiro` 下的会话文件 | 无 | 离线 | legacy 优先 `input_token_count`/`output_token_count`，皆为 0 才回退字符数 ÷ 4 |
| 7 | Kimi | `kimi` | JSONL `wire.jsonl` | `~/.kimi/sessions/<workspace>/<session>/wire.jsonl` | 无（源码声明不需要） | 离线 | `token_usage.input_other`/`input_cache_read`/`input_cache_creation`/`output` |
| 8 | Kimi Code | `kimi`（与 Kimi 同源） | JSONL `wire.jsonl`（递归 ≤5 层） | `~/.kimi-code/sessions/**` | 无 | 离线 | `usage.inputOther`（或 Anthropic / OpenAI 两种旧形状）；reasoning 恒 0 |
| 9 | CodeBuddy | `codebuddy` | JSONL + IDE 扩展日志（双源） | `~/.codebuddy/projects/**/*.jsonl` + IDE 日志根 | 有（Claude 系 `settings.json` hook） | 离线 | 多镜像优先级取 input/cache；`reasoningTokens` 独立桶 |
| 10 | WorkBuddy（含 WorkBuddy AI） | `workbuddy` / `workbuddy-ai`（按 variant） | JSONL + traces JSON + SQLite（三级） | `~/.workbuddy{,-ai}/projects/**/*.jsonl`（+`traces/`） | 有（两个 variant 各装一次） | 离线 | `rawUsage.prompt_tokens−缓存` → input；`completion−reasoning` → output |
| 11 | Roo Code | `roocode` | `ui_messages.json`（IDE 任务目录） | `<IDE 根>/User/globalStorage/rooveterinaryinc.roo-cline/tasks/**` | 无 | 离线 | `payload.tokensIn`/`tokensOut`/`cacheReads`/`cacheWrites` |
| 12 | Cline | `cline` | 会话转录 JSON | `~/.cline/data/sessions/**`（Windows 双安装 / WSL 探测） | 无 | 离线 | `metrics.inputTokens−缓存`、`cacheRead/WriteTokens`、`reasoningTokenCount` 只上报；**唯一解析器自产 `cost`** |
| 13 | Zed | `zed` | SQLite BLOB（`json` / `zstd`） | `~/.local/share/Zed/threads/threads.db`（macOS `~/Library/Application Support/...`） | 无 | 离线 | 线程级 `input_tokens`/`output_tokens`/`cache_read`/`cache_creation` 四元组差分 |
| 14 | LM Studio | `lmstudio` | 文本日志内嵌 JSON | `~/.lmstudio/server-logs/**/*.log` | 无 | 离线 | `prompt_tokens`/`completion_tokens`/`total_tokens` + `*_details.cached_tokens`；input 由 total 反推 |
| 15 | Unsloth Studio | `unsloth` | SQLite | `~/.unsloth/studio/studio.db` | 无 | 离线 | `prompt_tokens`/`completion_tokens`/`total_tokens`（共用本地推理归一化器） |
| 16 | AnythingLLM | `anythingllm` | SQLite | `%APPDATA%\anythingllm-desktop\storage\anythingllm.db` | 无 | 离线 | `response.metrics.prompt_tokens`/`completion_tokens`；**差额被判为 reasoning** |
| 17 | Goose | `goose` | SQLite | `%APPDATA%\goose\sessions\sessions.db`（macOS/Linux 走 `Application Support` / `XDG`） | 无 | 离线 | `accumulated_input/output/total_tokens` 差分；`model_config_json.model_name` 缺则整条丢弃 |
| 18 | Droid (Factory) | `droid` | `*.settings.json` | `~/.factory/sessions/**/*.settings.json`（`DROID_SESSIONS_DIR` 可覆盖） | 无 | 离线 | `tokenUsage.inputTokens`/`outputTokens`/`cacheCreation`/`cacheRead`/`thinkingTokens` + total 兜底填充 |
| 19 | Kilo Code | `kilo-code` | `ui_messages.json`（IDE 任务目录） | `<IDE 根>/User/globalStorage/kilocode.kilo-code/tasks/**` | 无 | 离线 | `payload.tokensIn`/`tokensOut`/`cacheReads`/`cacheWrites`；模型只报 `provider:<slug>` |
| 20 | OpenCode（SQLite 支路） | `opencode` | SQLite（四种库形态） | `$OPENCODE_HOME/opencode.db` | 无 | 离线 | 同一 `normalizeOpencodeTokens`；`data.tokens.*` |
| 21 | Qoder（旧库） | `qoder` / `qoder-cn` | SQLite（三表 JOIN） | `…/Qoder/SharedClientCache/cache/db/local.db`（CN 版独立路径） | 无 | 离线 | `token_info.prompt_tokens`（**含缓存，需减**）+`completion_tokens` |
| 22 | Qoder（新 JSONL） | `qoder`（与旧库**同名同源**） | 自有 JSONL 快照 | `~/.qoder/projects/**/*.jsonl`（CN 版 `~/.qoder-cn`） | 无 | 离线 | `usage.input_tokens`/`cache_read_*`/`cache_creation_*`/`output_tokens`；`credits` 原样挂载不换算 |
| 23 | Cursor | `cursor` | 远程 API（CSV 导出）+ SQLite 取凭据 | `Cursor/User/globalStorage/state.vscdb` + `cursor.com` API | 无 | **需联网 + 需凭据** | CSV 列名按表头匹配；`total` 由四分项求和，不用 CSV 的 `Total Tokens` |
| 24 | Devin CLI | `devin` | SQLite | `$XDG_DATA_HOME/devin/cli/sessions.db`（Windows 仅经 WSL） | 无 | 离线 | `metrics.input_tokens` 等四分项（互斥列）+ `request_id` 全安装规范的账本 |
| 25 | oh-my-pi (OMP) | `omp` | JSONL | `~/.omp/agent/sessions/**/*.jsonl` | 有（extension hook `tokentracker-notify.ts`，可选） | 离线 | `usage.input`/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens` |
| 26 | OmO | `omo` | JSONL（与 OMP 同一实现） | `~/.omo/agent/sessions/**/*.jsonl` | 无 | 离线 | 同上，但 `reasoningTokens`/`reasoning` **已并入 output**，求和时排除 |
| 27 | （共享实现壳） | —（由 OMP/OmO 注入） | — | — | — | — | `parseOmpLikeIncremental`：不为自身产出任何桶 |
| 28 | pi（路由型 agent） | `pi` / `pi-<provider>` | JSONL | `~/.pi/agent/sessions/**/*.jsonl` + `~/.multica/pi-sessions/*.jsonl` | 无 | 离线 | `usage.input`/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens`（reasoning 不并入 output） |
| 29 | Prime Agent | `prime-agent` / `prime-agent-<provider>` | JSONL | `~/.prime/agent/sessions/*.jsonl` | 无 | 离线 | 同 pi 家族字段表 |
| 30 | MiniMax Code | `minimax-code` | JSONL | `~/.minimax/v2/sessions/YYYY/MM/DD/<ts>-session_<id>/messages.jsonl` | 无 | 离线 | 同 pi 家族；**唯一可用时间源是 `message.timestamp`（entry 级恒 `null`）** |
| 31 | （共享实现壳） | —（由 pi 家族三家注入） | — | — | — | — | `parsePiLikeIncremental`：不为自身产出任何桶 |
| 32 | Craft Agents | `craft` | JSONL（只读首行预聚合头） | `~/.craft-agent/**/sessions/<sid>/session.jsonl` + `config.json` | 无 | 离线 | 会话头 `usage.inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheCreationTokens` 差分 |
| 33 | Reasonix | `reasonix` | JSONL 遥测旁车（无正文） | `~/.reasonix/**`、`%APPDATA%\reasonix\**`（`*.jsonl.telemetry.json`） | 无 | 离线 | `promptTokens`/`cacheHitTokens`/`cacheMissTokens`/`cacheWriteTokens`/`completionTokens`/`reasoningTokens` |
| 34 | Copilot CLI / Chat 扩展（OTEL） | `copilot` | OTEL JSONL | `~/.copilot/otel/*.jsonl`、`~/.copilot-otel/*.jsonl` | 无 | 离线（**前提：用户先开启 Copilot 侧 OTEL 文件导出**） | `gen_ai.usage.input_tokens`（CLI span 含读+写缓存，扩展只含读）+ `output_tokens` |
| 35 | Copilot 本地运行时 session-store | `copilot` | SQLite | `~/.copilot/session-store.db` | 无 | 离线 | `assistant_usage_events` 列 + `token_details_json`（`exact`/`fallback` 两档精度） |
| 36 | Copilot App | `copilot` | SQLite（会话级累计） | `~/.copilot/data.db` | 无 | 离线 | `sessions.total_input/output/cached/reasoning_tokens` 差值；**无 cache write 列** |
| 37 | Grok Build (xAI) | `grok` | JSONL + `signals.json`/`summary.json` | `~/.grok/sessions/<encodedCwd>/<sid>/{updates.jsonl,signals.json,summary.json}` | 有（SessionEnd hook，仅新鲜度） | 离线 | `inputTokens`（camelCase 含缓存，需双扣）/`cachedRead`/`cacheCreation`/`outputTokens`/`reasoningTokens` + 上游 ticks 成本 |
| 38 | Google Antigravity | `antigravity` | JSONL 转录 + SQLite（protobuf 二进制列） | `<geminiHome>` 下 `antigravity` / `antigravity-ide` / `antigravity-cli` 三目录的 `brain/**/transcript.jsonl`，配对 `conversations/<uuid>.db` | 无 | 离线 | protobuf `f4.1/f4.2`→input、`f4.5`→cached、`f4.9`→output、`f4.10`→reasoning；兜底走字符估算 |
| 39 | TRAE SOLO（国际版） | `trae` | **SQLCipher 加密 SQLite + WASM 内存快照** | `%APPDATA%` 下 `Trae` 或 `TRAE SOLO` 的 `ModularData/ai-agent` 与 `ModularData/ai-chat` 两处 `database.db` | 无 | 离线 | `chat_turn.context.token_usage.*`（11 个候选列名，唯一解判定后落桶） |
| 40 | TRAE Work CN（国内版） | `trae-cn` | 远程 API + 加密凭据 blob | `%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json` + `api.trae.cn` | 无 | **需联网 + 需凭据 + 需显式 opt-in** | `input_token`（含缓存，需减）/`output_token`/`cache_read_token`/`cache_write_token` |
| 41 | DeepSeek Harness (DSH) | `dsh` | 会话日志 JSONL 或 **zstd 拼接帧** | `~/.dsh/sessions/<project>/<session>/session[.vN].jsonl[.zstd]` | 无 | 离线 | `usage.inputTokens`/`outputTokens`/`cacheReadTokens`/`cacheWriteTokens`/`reasoningTokens`（互斥列） |
| 42 | Command Code (`cmd`) | `command-code` | JSONL 转录 | `~/.commandcode/projects/<cwd-slug>/<session>.jsonl` | 无 | 离线 | `usage.inputTokens`（含缓存，需双扣）/`cacheRead`/`cacheWrite`/`outputTokens`；reasoning 恒 0 |
| 43 | Codex 家族（Codex / AStudio / Every Code） | `codex` / `acode` / `every-code` | rollout JSONL（逐行事件） | `~/.codex/sessions/**/rollout-*.jsonl`（+ 扁平的 `archived_sessions/`）；`~/.acode/sessions`；Every Code 的 `sessions/` | 有（Codex/AStudio/Every Code 各自的 `notify` 改写） | 离线 | 见第 6 章（`parseRolloutIncremental` 一个函数按文件条目自带的 `source` 分流三家） |
| 44 | Claude Code | `claude` | 会话 JSONL | `~/.claude/projects/**/*.jsonl`（`CLAUDE_CONFIG_DIR` / 外置 `scanRoots` 可加根） | 有（`settings.json` 的 usage hook） | 离线 | 见第 6 章 |
| 45 | Claude Science | `claude-science` | SQLite（`operon-cli.db` / 旧名 `operon.db`） | `~/.claude-science`（`CLAUDE_SCIENCE_HOME` 可覆盖，`src/lib/rollout.js:6877`–`6883`） | 无 | 离线 | `input_tokens`/`cache_read_tokens`/`cache_write_tokens`/`output_tokens`（含 `aux_*` 辅助计数）；reasoning 恒 0 |

#### 5.1.1 口径说明：45 个函数名 / 39 个唯一实体 / 2 个共享实现 / 语义重叠

**这四组数字不是同一个东西，混用会得出互相矛盾的结论。** 逐条对齐：

| 口径 | 数字 | 含义与来源 |
|------|------|------------|
| `parse*Incremental` 函数名 | **45** | `src/lib/rollout.js` 内 `function parse*Incremental` 的定义条数（行号区间 `251`–`24458`）。这是「函数」计数，不是「产品」计数 |
| 唯一数据源实体 | **39** | 文本 / JSONL 类 **19** 家 + 数据库 / API 类 **21** 家 − **1** 家重叠（`opencode`：19 家侧记磁盘消息支路、21 家侧记 `opencode.db` 支路）= 19 + 21 − 1 = **39** |
| 共享实现（「Like」壳） | **2** | `parseOmpLikeIncremental`（`src/lib/rollout.js:15753`，服务 `parseOmpIncremental` / `parseOmoIncremental`）与 `parsePiLikeIncremental`（`src/lib/rollout.js:16319`，服务 pi / Prime Agent / MiniMax Code）。这两个名字本身**不产出任何桶**，只承接注入参数 |
| 合并口径的算术闭合 | 45 = 39 + 1 + 2 + 3 | 本章的 **39** 个实体由 **40** 个「自有实现」函数名服务（其中 `opencode` 占两个名：磁盘消息支路 + `opencode.db` 支路）；再加 **2** 个共享实现壳；再加 **3** 个归入第 6 章的函数名（`parseClaudeIncremental`、`parseClaudeScienceIncremental`、`parseRolloutIncremental`）。40 + 2 + 3 = 45 |

**语义重叠（一个 `source` 字面量对应多个解析入口，或一个入口对应多个 source）**——这是总表与「家数」最容易对不上的地方：

| 重叠形态 | 涉及入口 | 说明 |
|----------|----------|------|
| 两来源共用 `kimi` | `parseKimiIncremental` + `parseKimiCodeIncremental` | 两个函数都写 `source="kimi"`，只靠 `seenIds` / 文件偏移各自隔离 |
| 两来源共用 `kiro` | `parseKiroIncremental`（IDE）+ `parseKiroCliIncremental`（CLI） | 源码注释明说要让两者「collapse」到同一 `source='kiro'` 与规范化模型名下 |
| 两来源共用 `qoder` | `parseQoderDbIncremental`（旧库）+ `parseQoderNewIncremental`（新 JSONL） | 靠 `messageKey` 前缀 `row:` / `jsonl:` 与独立游标命名空间隔离，但**上传时同源** |
| 三通道共用 `copilot` | OTEL / session-store / App DB 三份实现 | 靠「用量认领」「会话基线」「`observeOnly`」三重机制分工，见 §5.3 |
| 一入口两 source | `parseWorkbuddyIncremental` → `workbuddy` / `workbuddy-ai` | variant 直接当 `source`（`src/lib/rollout.js:10651`、`11004`） |
| 一入口三 source（第 6 章） | `parseRolloutIncremental` → `codex` / `acode` / `every-code` | 按文件条目自带的 `source` 分流（`src/lib/rollout.js:281`、`494`、`521`、`3227`） |
| 一入口多 source | `parsePiIncremental` → `pi` / `pi-<provider>`，`parsePrimeAgentIncremental` → `prime-agent` / `prime-agent-<provider>` | 路由器形态的 agent，按 `provider` 字段派生 source，避免不同后端的 turn 合并进同一桶 |

**另有 3 个真实来源不在 45 个函数名里，读表时不要当成遗漏**：`parseDumateIncremental`（`src/lib/dumate.js:32`，DuMate 桌面端，入口函数定义在 `dumate.js` 而非 `rollout.js`）、ZCode 与 Mimo（**没有独立入口函数**，复用 `parseOpencodeDbIncremental`，`source` 分别为 `zcode` / `mimo`）。三者的读取细节见 §5.3.3。

---

#### 5.1.2 函数名 ↔ 显示名 ↔ `source` 索引（按 `src/lib/rollout.js` 定义行号排序）

给从代码侧进入本文的读者：**这一节与 §5.1 的总表是同一批 45 行，只是换了排序键**，两表可互相校验。行号即函数定义行。

| # | 函数名 | 定义行 | 显示名 | `source` |
|---|--------|--------|--------|----------|
| 1 | `parseRolloutIncremental` | `251` | Codex 家族（Codex / AStudio / Every Code） | `codex` / `acode` / `every-code` |
| 2 | `parseClaudeIncremental` | `954` | Claude Code | `claude` |
| 3 | `parseGeminiIncremental` | `1242` | Gemini CLI | `gemini` |
| 4 | `parseOpencodeIncremental` | `1412` | OpenCode（磁盘消息支路） | `opencode` |
| 5 | `parseOpenclawIncremental` | `1654` | OpenClaw | `openclaw` |
| 6 | `parseOpencodeDbIncremental` | `5761` | OpenCode（SQLite 支路） | `opencode` |
| 7 | `parseQoderDbIncremental` | `6218` | Qoder（旧库） | `qoder` / `qoder-cn` |
| 8 | `parseQoderNewIncremental` | `6552` | Qoder（新 JSONL） | `qoder` / `qoder-cn` |
| 9 | `parseClaudeScienceIncremental` | `7051` | Claude Science | `claude-science` |
| 10 | `parseCursorApiIncremental` | `7172` | Cursor | `cursor` |
| 11 | `parseKiroIncremental` | `7472` | Kiro（IDE） | `kiro` |
| 12 | `parseHermesIncremental` | `7900` | Hermes | `hermes` |
| 13 | `parseKiroCliIncremental` | `8886` | Kiro CLI | `kiro` |
| 14 | `parseKimiIncremental` | `9528` | Kimi | `kimi` |
| 15 | `parseKimiCodeIncremental` | `9723` | Kimi Code | `kimi` |
| 16 | `parseCodebuddyIncremental` | `10164` | CodeBuddy | `codebuddy` |
| 17 | `parseWorkbuddyIncremental` | `10803` | WorkBuddy / WorkBuddy AI | `workbuddy` / `workbuddy-ai` |
| 18 | `parseRoocodeIncremental` | `11824` | Roo Code | `roocode` |
| 19 | `parseClineIncremental` | `12200` | Cline | `cline` |
| 20 | `parseZedIncremental` | `12650` | Zed | `zed` |
| 21 | `parseLmstudioIncremental` | `13267` | LM Studio | `lmstudio` |
| 22 | `parseUnslothIncremental` | `13691` | Unsloth Studio | `unsloth` |
| 23 | `parseAnythingllmIncremental` | `13938` | AnythingLLM | `anythingllm` |
| 24 | `parseDevinIncremental` | `14207` | Devin CLI | `devin` |
| 25 | `parseGooseIncremental` | `14604` | Goose | `goose` |
| 26 | `parseDroidIncremental` | `15137` | Droid (Factory) | `droid` |
| 27 | `parseKilocodeIncremental` | `15545` | Kilo Code | `kilo-code` |
| 28 | `parseOmpLikeIncremental` | `15753` | **共享实现壳**（OMP / OmO 注入用） | —（不产桶） |
| 29 | `parseOmpIncremental` | `16087` | oh-my-pi (OMP) | `omp` |
| 30 | `parseOmoIncremental` | `16100` | OmO | `omo` |
| 31 | `parsePiLikeIncremental` | `16319` | **共享实现壳**（pi 家族注入用） | —（不产桶） |
| 32 | `parsePiIncremental` | `16689` | pi | `pi` / `pi-<provider>` |
| 33 | `parsePrimeAgentIncremental` | `16699` | Prime Agent | `prime-agent` / `prime-agent-<provider>` |
| 34 | `parseMinimaxCodeIncremental` | `16775` | MiniMax Code | `minimax-code` |
| 35 | `parseReasonixIncremental` | `17050` | Reasonix | `reasonix` |
| 36 | `parseCraftIncremental` | `17080` | Craft Agents | `craft` |
| 37 | `parseCopilotIncremental` | `17878` | Copilot CLI / Chat 扩展（OTEL） | `copilot` |
| 38 | `parseCopilotSessionStoreIncremental` | `18579` | Copilot 本地运行时 session-store | `copilot` |
| 39 | `parseCopilotAppDbIncremental` | `19381` | Copilot App | `copilot` |
| 40 | `parseGrokBuildIncremental` | `20084` | Grok Build (xAI) | `grok` |
| 41 | `parseAntigravityIncremental` | `20800` | Google Antigravity | `antigravity` |
| 42 | `parseTraeIncremental` | `22033` | TRAE SOLO（国际版） | `trae` |
| 43 | `parseTraeCnApiIncremental` | `22598` | TRAE Work CN（国内版） | `trae-cn` |
| 44 | `parseDshIncremental` | `23678` | DeepSeek Harness (DSH) | `dsh` |
| 45 | `parseCommandCodeIncremental` | `24458` | Command Code (`cmd`) | `command-code` |

**表外入口（复用上述解析器，不占 45 个函数名）**：`parseDumateIncremental`（`src/lib/dumate.js:32` → `source: "dumate"`）、ZCode（`source: "zcode"`）、Mimo（`source: "mimo"`）——三家都走 `parseOpencodeDbIncremental`（`src/lib/rollout.js:5761`）与各自的判别器，见 §5.3.3。

### 5.2 文本 / JSONL 类（19 家）

**本节覆盖 19 家**，顺序按「先常用、后长尾」排列（Gemini、OpenCode、OpenClaw、WorkBuddy、CodeBuddy、Kiro / Kiro CLI、Kimi / Kimi Code、Cline、Zed、Goose、Droid、Unsloth、LM Studio、AnythingLLM、Hermes、Roo Code、Kilo Code）。**Every Code 与 Grok Build 不在本节**：前者是 Codex 家族（`parseRolloutIncremental`，见总表第 43 行与第 6 章）的一个 `source`，后者是 JSONL + 估算路径（总表第 37 行，估算细节见 §5.5）。另外要说明：「文本 / JSONL 型」这个分类只对了一半——**这 19 家里有 7 家其实是 SQLite 读取**（Kiro、Hermes、Kiro CLI、Zed、Unsloth、AnythingLLM、Goose），OpenClaw 另有 agent SQLite 支路。

#### 5.2.0 所有家共享的落盘机制（读各家前必须先知道）

- **桶键与默认值**：`server/source/model/hour_start` 三段拼键（`src/lib/rollout.js:4222`–`4226`），`DEFAULT_SOURCE = "codex"`、`DEFAULT_MODEL = "unknown"`（`src/lib/rollout.js:55`–`56`）——「模型未识别」落成字符串 `unknown`，不是空串。`source` 统一小写化（`src/lib/rollout.js:4254`）。
- **累加器**：`addTotals`（`src/lib/rollout.js:4099`）里 `total_tokens` 必须由各家解析器自己算，只有 `total_tokens` 的 delta 也会被原样累加并写进队列行；`billable_total_tokens` 用 `?? total_tokens` 兜底（同处）。
- **UTC 半小时桶**：`toUtcHalfHourStart`（`src/lib/rollout.js:4173`）把时间戳按 UTC 取整到 `:00` / `:30`，`hour_start` 形如 `2026-10-13T21:30:00.000Z`；无效时间返回 `null`，各家普遍 `continue` **跳过事件**（不落回 now）。兜底到 mtime / now 的只有少数几家（Droid 用 mtime、LM Studio 用 mtime、Zed 与 Goose 用 `now`、Grok 用 `Date.now()`）。
- **队列行契约**：字段集 `source/model/hour_start/input_tokens/cached_input_tokens/cache_creation_input_tokens/output_tokens/reasoning_output_tokens/total_tokens/billable_total_tokens/total_cost_usd`（`src/lib/rollout.js:3328`）。队列是**绝对值快照流**而非增量事件流：同桶内容不变则跳过（`src/lib/rollout.js:3327`），消费端按 `(source, model, hour_start)` 覆盖。
- **零用量闸门**：`isAllZeroUsage`（`src/lib/rollout.js:4945`）把 `total_tokens` 也算作「非零」——这是 §5.6 里 $0 退化风险的判据来源。
- **本地推理共用归一化器**：`normalizeLocalStudioTokens`（`src/lib/rollout.js:12994`，返回块 `13039`–`13049`）服务 LM Studio 与 Unsloth；落桶入口 `reconcilePassiveUsageEvent`（`src/lib/rollout.js:13242`）**同 key 先减旧再加新**。

#### 5.2.1 Gemini（`parseGeminiIncremental`，`src/lib/rollout.js:1242`）

- `source`：调用方显式传 `"gemini"`（`src/commands/sync.js:1272`），解析器兜底 `normalizeSourceInput(source) || "gemini"`（`src/lib/rollout.js:1265`），并允许逐文件覆盖（`src/lib/rollout.js:1275`）。
- 路径：官方目录 `~/.gemini`（`GEMINI_HOME` 可覆盖）+ `tmp`（`src/commands/sync.js:1224`、`1234`）；目录约定 `tmp/<project>/chats/session-*.json`（`src/lib/rollout.js:226`）。
- 格式：整份 JSON，事件粒度是 `session.messages` 数组元素（`src/lib/rollout.js:2916`）；文件身份含 inode（`src/lib/rollout.js:1284`，判等 `1287`–`1291`）。
- 五桶映射（`normalizeGeminiTokens`，`src/lib/rollout.js:4734`）：`tokens.input`→input；`tokens.cached`→cached；`tokens.output + tokens.tool`→output；`tokens.thoughts`→reasoning（**不并入 output**）；`tokens.total`→total 取 `max(上报值, 分桶和)`（`4743`）；`cache_creation_input_tokens` 恒 0。
- 模型：逐条消息 `msg.model`，会话内沿用上次值（`src/lib/rollout.js:2952`）；无 Gemini 专属兜底，缺失落 `unknown`。
- 时间与桶：`msg.timestamp`，缺失即跳过（`src/lib/rollout.js:2956`、`2964`）。
- 增量判据：数组下标 + 累计基线（`cursors.files[key] = { lastIndex, lastTotals, lastModel }`，`src/lib/rollout.js:1361`）；数组被截断即整段重置（`2917`）。差分用 `diffGeminiTotals`（`286`），`conversation_count` 计「产生非零差分的消息条数」（`2974`）。
- 特殊：**19 家里唯一有 $0 退化路径的一家**（`total` 取 max + 闸门把 `total_tokens` 当非零，`src/lib/rollout.js:2971`、`4945`）——若日志只写 `tokens.total`，会写出 input/output/cache 全 0 的队列行；成立前提与计价侧条件见 §5.6。

#### 5.2.2 OpenCode（磁盘消息支路，`parseOpencodeIncremental`，`src/lib/rollout.js:1412`）

- `source`：`"opencode"`（`src/commands/sync.js:1440`），兜底 `src/lib/rollout.js:1438`。
- 路径：`OPENCODE_HOME || $XDG_DATA_HOME/opencode`（`src/commands/sync.js:1339`），再拼 `storage`（`src/commands/sync.js:1400`）→ `<storage>/message/**`（`walkOpencodeMessages` 递归，只收 `msg_*.json`，`src/lib/rollout.js:5025`、`5033`；`listOpencodeMessageFiles` 只拼一层 `<storage>/message`，`243`）。
- 五桶映射（`normalizeOpencodeTokens`，`4755`）：`tokens.input`→input（已是**纯非缓存输入**，不做减法）；`tokens.cache.read`→cached；`tokens.cache.write`→cache_creation；`tokens.output`→output；`tokens.reasoning`→reasoning（不并入 output）；`total` 由五分项重算。
- 模型：兼容 `modelID` / `model`(字符串) / `model: { id }`（`src/lib/rollout.js:4272`）；落桶兜底 `DEFAULT_MODEL`（`3143`）。
- 时间与桶：`msg.time.completed` → `msg.time.created`，两者皆无则不产事件（`src/lib/rollout.js:3122`）。
- 增量判据：`cursors.files[key]`（`1513`）存 `lastTotals/fingerprint/messageKey/dedupedForkCopy`；差分复用 `diffGeminiTotals`（`3109`）；fork 副本去重（`3805`），副本「复活」时整份重加而非只补差（`3105`）。
- 特殊：同寄主另有 **SQLite 支路**（`src/lib/rollout.js:5038` 起，见 §5.3），两路结果在 `src/commands/sync.js:1384` 相加；纯 total 行会被静默丢弃（差分函数对全 0 返回 `null`）。

#### 5.2.3 OpenClaw（`parseOpenclawIncremental`，`src/lib/rollout.js:1654`）

- `source`：`"openclaw"`（`src/commands/sync.js:1109`），逐文件可覆盖（`src/lib/rollout.js:1698`）。
- 路径：`~/.openclaw`（三档 env 覆盖）+ `agents/<agent>/sessions/*.jsonl`（`src/lib/rollout.js:1582`、`1622`）；子目录集合 `["sessions","session-sqlite-import-archive"]`（`1612`），另有 agent SQLite（`1638`）。Windows 额外探测 WSL。
- 五桶映射（`src/lib/rollout.js:2094`）：`message.usage.input` **先减** `cacheRead` 再进 input；`cacheRead`→cached；`cacheWrite`→cache_creation；`output`→output；reasoning 恒 0；`total` = 四桶和。
- 关键点：`totalTokens` **只用于行级预筛，从不入库**（`src/lib/rollout.js:2055`：`if (!line.includes('"usage"') || !line.includes("totalTokens")) continue;`）。
- 模型：`normalizeModelInput(msg.model) || DEFAULT_MODEL`（`2074`）。时间：`obj.timestamp`，缺失即跳过（`2071`）。
- 增量判据：字节偏移（`1710`）+ 事件身份计数表（`2114`）+ 时间闸门 `aggregateAfterMs`（`2128`）。
- 特殊：**项目维度不支持**（`src/lib/rollout.js:2141`：`// Project-level OpenClaw attribution is not supported yet (no stable cwd info).`）；有插件但解析完全被动（`src/commands/sync.js:1102` 注释）。

#### 5.2.4 Kiro（IDE，`parseKiroIncremental`，`src/lib/rollout.js:7472`）

- `source`：硬编码 `"kiro"`（`src/lib/rollout.js:7567`）。
- 路径：`%APPDATA%\Kiro\User\globalStorage\kiro.kiroagent`（suffix 见 `7282`）→ `dev_data/devdata.sqlite` 与 `dev_data/tokens_generated.jsonl`（`resolveKiroDbPath` / `resolveKiroJsonlPath`，`7298`–`7304`）。
- 五桶映射（`7545`）：`tokens_prompt`→input、`tokens_generated`→output；**两列皆 0 即跳过**（`7547`）；`cached_input_tokens` / `reasoning_output_tokens` 写死 0，`cache_creation_input_tokens` 键缺失。
- 模型：`.chat` 时间线反查，兜底 `"kiro-agent"`（`7555`）。
- 时间与桶：`row.timestamp` 按 **UTC** 解释 naive 串（`ts.replace(" ","T") + "Z"`，`7549`）——SQLite `CURRENT_TIMESTAMP` 语义。
- 增量判据：自增 `id` 水位（`7541`、`7572`）；JSONL 兼容支路用行号（`countKiroJsonlLines`，`7338`）。
- 特殊：`source='kiro'` 与 Kiro CLI 合并（`8112` 注释），规范模型名后两者「collapse」到同一桶。

#### 5.2.5 Hermes（`parseHermesIncremental`，`src/lib/rollout.js:7900`）

- `source`：`"hermes"`（`src/lib/rollout.js:8018`）。
- 路径：`TOKENTRACKER_HERMES_HOME` → `~/.hermes` / `%LOCALAPPDATA%\hermes`（`7604`、`7610`）；DB 为 `state.db`，外加 `profiles/*/state.db`（`7659`、`7676`）。UNC 路径先本地快照（`7722`）。
- 五桶映射：SQL 列名即桶名（SQL 原文 `7718`），落桶在 `8008`：`input_tokens`/`cache_read_tokens`→cached/`cache_write_tokens`→cache_creation/`output_tokens`/`reasoning_tokens`；`total` 为五项之和；**`message_count` 差分 → `conversation_count`**（`dMessageCount`）。
- 模型：`normalizeModelInput(row.model) || "hermes-agent"`（`8006`）。
- 时间与桶：**取 `ended_at`，回落 `started_at`**（`7999`）——长会话的全部增量会堆在「结束时刻」的那个半小时桶里。
- 增量判据：`started_at` 水位 + 未完成会话强制重读（`7707`、`7708`）；SQL 层 `WHERE` 要求至少一列 > 0，天然不产 $0 行。
- 特殊：`conversation_count` 语义是「消息数差分」，与会话/请求都不同源（见 §5.6）。

#### 5.2.6 Kiro CLI（`parseKiroCliIncremental`，`src/lib/rollout.js:8886`）

- `source`：**`"kiro"`**（`src/lib/rollout.js:9191`、`9210`），与 IDE 共享。
- **三条支路、两种精度**：legacy 会话 `.json` **优先读真实用量** `turn.input_token_count` / `turn.output_token_count`（`8371`、`8372`），**只有两者皆为 0 才回退**字符估算（闸门 `8374`，估算点 `8386`、`8387`）；SQLite `data.sqlite3` 支路为纯字符估算（常量 `KIRO_CLI_CHARS_PER_TOKEN = 4` 在 `8118`，使用点 `9152`–`9159`，乘法点 `9155`）；v2 `messages.jsonl` 支路也是纯估算（该文件不含 token 字段，注释 `8475`–`8481`）。
- 路径：`kiro-cli/data.sqlite3`（三平台分叉 `8132`、`8138`、`8140`）；会话文件由 `resolveKiroCliSessionFiles` 给出（`8165`），根为 `KIRO_HOME || ~/.kiro`（`8960`）。
- 五桶映射（SQLite 支路，落桶 `9209`）：`floor(promptChars/4)`→input、`floor(responseChars/4)`→output、`floor(reasoningChars/4)`→reasoning，两个缓存桶 0；闸门为 `approxInput > 0 || approxOutput > 0 || approxReasoning > 0`（`9209`）。
- 模型：`r.model_id || r.session_model_id` → 规范化 → 兜底 `"kiro-cli-agent"`（`9167`）。时间：`request_start_timestamp_ms`，非正即跳过（`9161`）。
- 增量判据：per-request 指纹（`${promptChars}:${responseChars}:${reasoningChars}:${model}:${tsMs}`，`9172`）——**先减旧再加新**，回撤是把负数 delta 交给 `addTotals`（`9192`），`conversation_count` 随之 −1（`9203`）；另有「会话文件 → SQLite 迁移后撤回」通道（`9036`）。
- 特殊：**credits 不进 token 桶**，只写侧车 `kiro-credits.json`（`8119`、`8955`）。会话支路的零值闸门只检 4 列、**不含 reasoning**（`9414`），随后 delta 含 `total_tokens: … + reasoning`（`9446`）→ 可产出 `input=0/output=0/reasoning>0/total>0` 的行（见 §5.6 B 类）。

#### 5.2.7 Kimi（`parseKimiIncremental`，`src/lib/rollout.js:9528`）

- `source`：硬编码 `"kimi"`（`src/lib/rollout.js:9611`）。
- 路径：`~/.kimi/sessions/<workspace>/<session>/wire.jsonl`（`9501`）。
- 五桶映射（`9586`）：`msg.payload.token_usage.input_other`→input、`input_cache_read`→cached、`input_cache_creation`→cache_creation、`output`→output；**四列全 0 提前跳过**（`9590`）；reasoning 恒 0，`total` 重算。只处理 `message.type === "StatusUpdate"` 且带 `token_usage` / `message_id` 的记录（`9575`）。
- 模型：**来自配置而非日志**（`src/lib/rollout.js:9540`：`model || resolveKimiDefaultModel(env || process.env)`），兜底 `"kimi-for-coding"`（`8080`）。
- 时间与桶：`entry.timestamp ?? payload.timestamp`（**秒**），缺失即跳过（`9595`）。
- 增量判据：文件 `size` / `ino` 偏移 + `message_id` 集合（`9557`、`9542`）；size 回退或 inode 变化则从 0 重读（`9557`）。
- 特殊：源码明确不需要 hook（`src/lib/rollout.js:8078`：`// No hook installation needed; Kimi writes wire.jsonl automatically.`）。

#### 5.2.8 Kimi Code（`parseKimiCodeIncremental`，`src/lib/rollout.js:9723`）

- `source`：**`"kimi"`**（`src/lib/rollout.js:9844`），与 Kimi 同源。
- 路径：`~/.kimi-code/sessions/**` 递归收集 `wire.jsonl`，深度 ≤ 5（`9677`）。
- 五桶映射（**三种历史形状**，`9802`–`9817`）：新版 camelCase（`usage.inputOther` 存在）→ `inputOther`/`inputCacheRead`/`inputCacheCreation`/`output`；Anthropic 风格 → `input_tokens`/`cache_read_input_tokens`/`cache_creation_input_tokens`/`output_tokens`；OpenAI 兼容 → `input_tokens − details.cached_tokens`/`details.cached_tokens`/`cache_creation_input_tokens`/`output_tokens`。
- 只读 `step.end` 且带 `usage` 与 `evt.uuid` 的事件（`9775`、`9782`），`config.update` 只更新模型——避免与 `usage.record` 双计（注释 `9796`）。
- 模型：`config.update.modelAlias` 覆盖（`9769`），默认来自 `~/.kimi-code/config.toml`，兜底 `"kimi-for-coding"`（函数头 `resolveKimiCodeDefaultModel` 在 `9703`，兜底在 `9704`）。
- 时间与桶：`entry.time ?? evt.time`（**毫秒**），缺失即跳过（`9828`）。
- 增量判据：**仅 `seenIds`（uuid），无字节偏移**（`9783`）——文件被整体重写时依赖 uuid 而非位置。
- 特殊：四桶全 0 提前跳过（`9823`）。

#### 5.2.9 CodeBuddy（`parseCodebuddyIncremental`，`src/lib/rollout.js:10164`）

- `source`：`"codebuddy"`（`src/lib/rollout.js:10384`）。
- **双源设计**：主源 `~/.codebuddy/projects/**/*.jsonl`（`CODEBUDDY_HOME` 可覆盖，`10028`、`10035`）；兜底源为 IDE 扩展日志，平台分叉在 `10069`（`CodeBuddy CN/logs`、`Code/logs`、`CodeBuddyExtension/Logs/CodeBuddyIDE`）。
- 五桶映射：按优先级取镜像（`10278`、`10293`）——input：`cachedMissTokens`/`cacheMissTokens`/`input_tokens`/`inputTokens`/`prompt_tokens`；output：`output_tokens`/`outputTokens`/`completion_tokens`；cached：`cache_read_input_tokens`/`cacheReadInputTokens`/`cacheTokens`/`prompt_cache_hit_tokens`/`cached_tokens`；cache_creation：`cache_creation_input_tokens`/`cacheCreationInputTokens`/`cachedWriteTokens`/`prompt_cache_write_tokens`。**非缓存输入修正**（`10337`–`10343`）：当 input 来自「总量字段」且 cacheRead > 0 时，`inputTokens = max(0, inputTokens − cacheRead)`。
- 落桶（`10374`）：含 `reasoningTokens`，`total` 为五分量和，`conversation_count: 1`。
- 模型：`modelsByAgent` → `logModelsByAgent` → `fallbackModel`（`10355`），兜底 `"codebuddy-unknown"`（函数头 `9923`，兜底 `9924`）。
- 时间与桶：`entry.timestamp`（毫秒）/ 日志行时间，缺失即跳过。
- 增量判据：`seenIds`（`messageId`）+ 用量指纹（`10360`、`10369`），指纹表上限 10k（`capCodebuddyUsageFingerprints`，`10022`）。
- 特殊：**两条支路都有零值闸门**——扩展日志支路要求 input/completion/cacheRead/cacheCreation 至少一个 > 0（`10345`，**该判定不含 reasoning**，故「只有 reasoning」的行会被丢）；JSONL 支路是含 `reasoningTokens` 的五桶零闸门（`10476`–`10485`）。纯 total 行被静默丢弃，不产 $0 行。hook 属 Claude 系（`src/lib/passive-mode.js:156`，安装点 `src/commands/init.js:577`）。

#### 5.2.10 WorkBuddy / WorkBuddy AI（`parseWorkbuddyIncremental`，`src/lib/rollout.js:10803`）

- `source`：**唯一随 variant 变化的实现**——`WORKBUDDY_VARIANTS` 定义 `workbuddy`（`.workbuddy`，`WORKBUDDY_HOME`）与 `workbuddy-ai`（`.workbuddy-ai`，`WORKBUDDY_AI_HOME`）两个 variant（`10651`），落桶直接用 variant 当 source（`11004`：`getHourlyBucket(hourlyState, variant, model, bucketStart)`）；调用侧传 `source: "workbuddy-ai"`（`src/commands/sync.js:2679`）。
- **三类数据源，优先级严格**（注释 `10694`）：`projects/**/*.jsonl`（含 `subagents/agent-*.jsonl`）> `traces/<pid>/trace_*.json` > 同寄主 SQLite；压制关系在 `11013`、`11088`、`11164`–`11168`。
- 五桶映射：入口是 `providerData.rawUsage`（`10907`）：input = `prompt_tokens − cacheRead − cacheCreation`（`911`，避免缓存双计）；cached = 三个镜像取最大（`cache_read_input_tokens` / `prompt_tokens_details.cached_tokens` / `prompt_cache_hit_tokens`）；cache_creation = `cache_creation_input_tokens`；output = `completion_tokens − reasoning`；reasoning = `completion_tokens_details.reasoning_tokens`。
- traces 支路**不同形**：`modelInfo.totalInputTokens`/`totalOutputTokens`/`totalCachedTokens`（`11085`–`11087`）+ `startedAtRaw`（`11081`，秒/毫秒自适应）+ `modelInfo.models[0]`（`11103`）；`input = totalInput − totalCached`（`11105`），两个缓存/推理桶恒 0（`11109`、`11111`），`conversation_count: 1`（`11113`）。
- 模型：`provider.model` → `provider.requestModelId` → `entry.model` → `fallbackModel`（`10987`），兜底 `"auto"`（函数头 `10677`，兜底 `10678`，且 `auto` 会逐字上报，`10632` 注释）。
- 时间与桶：`entry.timestamp`（毫秒），缺失即跳过（`10919`、`10979`）。
- 增量判据：`seenIds`（消息 ID，`11007`）+ 旧版 entry-id 双写（`11008`）；`detailedSessions` 压制 traces 兜底（`11013`）。
- 特殊：五桶全 0 跳过（`10968`）；hook 装在两个 variant 上（`src/lib/passive-mode.js:166`、`177`；`src/commands/init.js:581`、`585`）。

#### 5.2.11 Roo Code（`parseRoocodeIncremental`，`src/lib/rollout.js:11824`）

- `source`：`"roocode"`（`src/lib/rollout.js:11939`）。
- 路径：**复用 Kilo Code 的 IDE 根列表**（`11767`），再拼 `<IDE 根>/User/globalStorage/rooveterinaryinc.roo-cline/tasks`（`11767` 起）。
- 五桶映射（`11906`）：`payload.tokensIn`→input、`cacheReads`→cached、`cacheWrites`→cache_creation、`tokensOut`→output；reasoning 恒 0。
- 入口只认 `msg.say ∈ {api_req_started, api_req_deleted}`（`11892`）；**零值占位故意不入 `seen`**，以便后续回填（`11910` 注释）。
- 模型：真实模型藏在兄弟文件 `api_conversation_history.json` 的 `<environment_details><model>` 里（注释 `11749`，正则 `11805`）；三级兜底（`11814`）：真实模型 → `protocol:<apiProtocol>` → `"unknown"`。
- 时间与桶：`msg.ts`（毫秒，`11899`）。增量判据：`taskUuid:ts`（`11902`）+ 文件 mtime 闸门。
- 特殊：Kilo Code 与 Roo Code 字段同形、只有 `source` 与目录不同。

#### 5.2.12 Cline（`parseClineIncremental`，`src/lib/rollout.js:12200`）

- `source`：`"cline"`（`src/lib/rollout.js:12396`）。
- 路径：`resolveClineSessionsDirs`（`12054`，Windows 双安装 + WSL 探测在 `12069`–`12073`）→ `scanClineSessionFiles`（`12086`）；带错误/完成态的包装 `resolveClineSessionFilesWithStatus`（`12136`）。
- 五桶映射（`12356`）：input = `inputTokens − cacheRead − cacheWrite`（上游 `inputTokens` **含缓存**）；`cacheReadTokens` / `cacheWriteTokens` 各自成桶；output = `outputTokens`（**含** reasoning）；reasoning = `reasoningTokenCount`（只上报，**不加入 total_tokens**，也不在 output 之上再计费）。
- **唯一自产成本的解析器**：`metrics.cost` → `total_cost_usd`（差分后入桶，`12356`）。
- 模型：`msg.modelInfo.id` → 会话 sidecar `model` → `provider:<name>` → `DEFAULT_MODEL`（`normalizeClineModel`，函数头 `12178`，兜底链 `12179`–`12189`）。
- 时间与桶：`msg.ts`，另有**导入闸门** `if (importedAtMs !== null && ts <= importedAtMs) continue;`（`12350`）——导入历史不被重复计。
- 增量判据：三键兼容游标（`12369`）+ **五路逐指标差分**（`12383`：`deltaInput/deltaCached/deltaCreation/deltaOutput/deltaReasoning`，各 `max(0, …)`）；`conversation_count = previous ? 0 : 1`（`12405`），只计首次。
- 特殊：跳过条件包含 `cost`（`12381`）——**只有成本变化、token 无增量的行也会入队**。

#### 5.2.13 Zed（`parseZedIncremental`，`src/lib/rollout.js:12650`）

- `source`：`"zed"`（`src/lib/rollout.js:12782`）。
- 路径：`threads/threads.db`（darwin `~/Library/Application Support/Zed/...`、win32 `%LOCALAPPDATA%\Zed\...`，`12490`）；Windows 探测 WSL（`12501`），sync 侧双安装合并（`src/commands/sync.js:2062`）。
- 格式：`threads` 表 BLOB，`data_type` 为 `json` 或 `zstd`（`12518`）——`zstdDecompressSync` 优先、`@mongodb-js/zstd` 回落。
- 五桶映射（`12553`）：`input_tokens`→input、`output_tokens`→output、`cache_read_input_tokens`→cached、`cache_creation_input_tokens`→cache_creation；reasoning 恒 0。取值优先级 `thread.request_token_usage`（逐请求和）→ `thread.cumulative_token_usage`（`12597`、`12601`）。
- 模型：`thread.model.model`（`12594`）；跳过 `thread.imported === true`（`12585`）与 `ZED_DOUBLE_COUNTED_PROVIDERS`（`12593`）。
- 时间与桶：`updated_at ?? created_at ?? thread.updated_at ?? new Date().toISOString()`（`12764`）——**末级兜底是 now**。
- 增量判据：SQL `updated_at > cursor`（`12636`）+ 线程累计差分；`isReset = currSum > 0 && currSum < prevSum`（`12742`，`12732` 是差分基线，不是 reset 判据）；`threadTotals` 上限 10k（`12816`）。
- 特殊（限定条款）：`totalDelta` 只由 input/output/cache_read/cache_write 求和（`12751`）且 `totalDelta <= 0` 即跳过（`12752`），因此**只有缓存列增长**时仍会落出 `input=0 / output=0 / total>0` 的行——「结构上不可能出现六列全零」不等于「不可能出现 input/output = 0」。

#### 5.2.14 LM Studio（`parseLmstudioIncremental`，`src/lib/rollout.js:13267`）

- `source`：`const LMSTUDIO_SOURCE = "lmstudio";`（`12852`）。
- 路径：`TOKENTRACKER_LMSTUDIO_HOME` / `LM_STUDIO_HOME` / `~/.lmstudio`（`12859`）+ `server-logs` 递归 `*.log`（`12871`）。
- 格式：文本日志内嵌 JSON（模块注释 `12845`，段头 `12843`），解析靠 `"usage"` 字节标记 + 花括号配平（`12897`、`12917`），带 8MB 窗口 / 64KB 分块上限（`LMSTUDIO_WINDOW_BYTES` at `12853`、`LMSTUDIO_CHUNK_BYTES` at `12854`）。
- 五桶映射：复用 `normalizeLocalStudioTokens`（`12994`，返回块 `13039`–`13049`）——`total` 取 `total_tokens`/`totalTokens` 与 `prompt+completion` 的较大者；cached ≤ prompt；cache_creation ≤ prompt − cacheRead；reasoning ≤ completion；**input 由 `total − completion − cacheRead − cacheWrite` 反推**。
- 模型：`lastLmstudioJsonStringField(metadataBuffer, "model") || carried.model`（`13091`），兜底 `DEFAULT_MODEL`。
- 时间与桶：日志行 `[YYYY-MM-DD HH:MM:SS]`（**本地时间**，`12979`）→ 跨窗口携带 → **文件 mtime**（`13094`、`13129`）。
- 增量判据：文件 `dev/ino/size/mtime` 三态（`unchanged` / `canResume` / 整源重建，`13325`）+ 事件 key（`lmstudio:<responseId>` 或哈希，`13099`）。
- 特殊：`total_cost_usd: 0` 与 `conversation_count: 1` 在共用归一化器里写死（返回块 `13039`–`13049`）。

#### 5.2.15 Unsloth Studio（`parseUnslothIncremental`，`src/lib/rollout.js:13691`）

- `source`：`const UNSLOTH_SOURCE = "unsloth";`（`13467`）；落桶经 `reconcilePassiveUsageEvent`（`13730`）。
- 路径：`<studioHome>/studio.db`，默认 `~/.unsloth/studio/studio.db`（`13482`）。
- 五桶映射：`normalizeUnslothUsageRow`（`13667`）把 `prompt_tokens`/`completion_tokens`/`total_tokens`/`cached_tokens`/`cache_write_tokens`/`reasoning_tokens` 交给共用归一化器；**`api_usage_events` 表的投影把缓存/推理列写死 0**（`0 AS cached_tokens, 0 AS cache_write_tokens, 0 AS reasoning_tokens`，`13634`–`13636`）。
- 模型：`response_model` → `requested_model` → `fallback_model` → `DEFAULT_MODEL`（`13650`），再加前缀：本地 → `local/…`、未计费 provider → `unpriced/<provider>/…`、已知 provider → `<provider>/<model>`（白名单 `13470`）。
- 时间与桶：`coerceEpochMs(created_at) || parseIsoTimestampMs(created_at)`（`13677`）。
- 增量判据：扫描窗口 `overlap/latest`（`created_at` + `id` 二元组）+ DB 侧车指纹；**只在扫描有界时才走快路径**（判定 `13714` `hasBoundedScan`、短路返回 `13717`），尾部保留末 256 行（`13469`）。
- 特殊：与 LM Studio 同用「先减旧再加新」的被动对账入口，避免整源重建时重复累加。

#### 5.2.16 AnythingLLM（`parseAnythingllmIncremental`，`src/lib/rollout.js:13938`）

- `source`：`"anythingllm"`（`src/lib/rollout.js:14053`）。
- 路径：`%APPDATA%\anythingllm-desktop\storage\anythingllm.db`（`13808`）；数据来自 `workspace_chats.response.metrics`（`src/commands/sync.js:1908`；SELECT 本体 `13874`–`13887`，SQL 层只过滤游标/`include`，**不做零值过滤**）。
- 五桶映射（`14013`、`14040`）：`prompt_tokens`→input、`completion_tokens`→output，两者皆 0 则该行丢弃（`14016`）；**`reasoning = max(0, total_tokens − input − output)`**——「未解释差额」被判为 reasoning，**不保证真的是推理量**（本章里最反直觉的一处）；两个缓存桶恒 0。
- 模型：`row.model || "anythingllm-unknown"`（`14040`）。
- 时间与桶：`createdAt ?? lastUpdatedAt`，解析器同时吃 epoch 毫秒/秒、数字串、`YYYY-MM-DD HH:MM:SS`（**按 UTC**，`13856`）。
- 增量判据：自增 `id` 水位（`14010`）+ `pendingChatIds` 补扫（`14024`）。
- 特殊：纯 total 行会被丢弃（不产 $0 行），但**「差额即 reasoning」会让 reasoning 桶变成一个语义垃圾桶**，下游按 output 单价计费 reasoning 时会被高估。

#### 5.2.17 Goose（`parseGooseIncremental`，`src/lib/rollout.js:14604`）

- `source`：`"goose"`（`src/lib/rollout.js:14730`）。
- 路径：`GOOSE_PATH_ROOT` → `<root>/data/sessions/sessions.db`（`14489`）；Windows 为 `%APPDATA%\goose\sessions\sessions.db`（`14503`），macOS/Linux 走 `Application Support` / `$XDG_DATA_HOME`（含 `Block/goose` 备用）；sync 侧双安装合并（`src/commands/sync.js:1993`）。
- 五桶映射（`14670`、`14708`）：优先 `accumulated_input_tokens` / `accumulated_output_tokens`（回退单回合列）差分进 input/output；`reasoningDelta = max(0, dTotal − dInput − dOutput)`（`14711`，Goose 把推理并进 `total_tokens`）；两个缓存桶恒 0。
- 模型：`parseGooseModelName(row.model_config_json)`（函数头 `14529`）**无兜底常量**，解析不到就 `continue`。
- 时间与桶：`parseGooseCreatedAt(row.created_at) || new Date().toISOString()`（`14713`）。
- 增量判据：DB mtime 短路（`14639`）+ 会话累计差分 + `isReset`（`14683`）。
- 特殊（$0 弱化版）：只要 `dTotal > 0` 就落桶，continue 条件是三项皆 0（`14686`）；若 input/output 差分为 0（只有 total 在涨），会产出 `input=0 / output=0 / reasoning>0` 的行。因 `goose` 不在「推理并入 output」集合内，该行按 output 单价计费，**不是 $0**（见 §5.6）。

#### 5.2.18 Droid（Factory，`parseDroidIncremental`，`src/lib/rollout.js:15137`）

- `source`：`"droid"`（`src/lib/rollout.js:15381`）。
- 路径：`DROID_SESSIONS_DIR`（逗号分隔）→ `FACTORY_DIR/sessions` → `~/.factory/sessions`（`14797`，Windows 带 WSL 分支）；文件筛选 `*.settings.json`（`14836`）；sync 侧去重 `dedupeDroidSettingsFilesBySession`（**定义在 `src/lib/rollout.js:15069`**，调用 `15165`、导出 `24839`；`src/commands/sync.js:6595` 只是一条注释），另有 `prune: true`（`src/commands/sync.js:2032`）。
- 五桶映射（`15272`）：`tokenUsage.inputTokens`/`outputTokens`/`cacheCreationTokens`/`cacheReadTokens` 各自成桶；`thinkingTokens`→reasoning；`totalTokens` 只做兜底。
- **本家独有的「total 兜底填充」**（`applyDroidTotalFallback`，`14972`）：算 `known = input + output + cacheCreation + cacheRead + thinking`，若 `total > known`，缺口优先填 `output`（当 output 为 0），否则并入 `thinking`——**结构性规避「只有 total」的 $0 退化**。
- 模型：`resolveDroidModel`（定义 `15046`–`15057`）三级链：`settings.model` → sidecar `<id>.jsonl` 抓取 → `providerLock` / 模型片段推断 → `defaultDroidModelForProvider`；调用点 `15348`，上方注释 `15343`–`15346` 与实现一致；`normalizeDroidModelName` 在 `14858`。
- 时间与桶：**唯一用文件 mtime 的家**（`15210`、`15328`）：`toUtcHalfHourStart(new Date(mtimeMs || Date.now()).toISOString())`。
- 增量判据：会话累计 + mtime（`15243`）；`conversation_count = isFirstSeenSession || isReset ? 1 : 0`（`15379`）。
- 特殊：因时间来自 mtime，**历史会话被复制/触碰会把用量搬到「触碰时刻」的桶**，这是与其它家的本质差异。

#### 5.2.19 Kilo Code（`parseKilocodeIncremental`，`src/lib/rollout.js:15545`）

- `source`：**`"kilo-code"`（带连字符）**（`src/lib/rollout.js:15651`）。
- 路径：`<IDE 根>/User/globalStorage/kilocode.kilo-code/tasks`（`11706`）；sync 侧双安装合并（`src/commands/sync.js:1949`）。
- 五桶映射（`15624`、`15640`）：`payload.tokensIn`→input、`tokensOut`→output、`cacheReads`→cached、`cacheWrites`→cache_creation；reasoning 恒 0；`total` = 四桶和。
- **模型：只报 provider，不报模型**（`normalizeKilocodeProviderToModel`，`11730`）——slug 化后返回 `provider:<slug>`，全分隔符的 slug 返 `provider:unknown`；原因是真实 model id 存在 workspace state、不归属到单轮（`11724` 注释）。
- 时间与桶：`msg.ts`（`15617`）。增量判据：`taskUuid:ts` + 文件 `{size, mtimeMs, ino}`。
- 特殊：零值占位不入 `seen`（与 Roo Code 同族），后续回填；无 $0 退化。

---

#### 5.2.20 19 家横向对照（时间源 / 游标形状 / 模型来源 / 纯 total 处置）

| Provider | 时间源 | 桶起点兜底 | 增量游标形状 | 模型来源 | 纯 total 行处置 |
|----------|--------|------------|--------------|----------|-----------------|
| Gemini | `msg.timestamp` | 否（缺失即跳过，`2956`、`2964`） | 数组下标 + 累计基线（`1361`） | `msg.model`，会话内沿用（`2952`） | **落桶 → $0**（`2971`） |
| OpenCode（磁盘） | `msg.time.completed ?? created` | 否（`3122`） | 文件 key：`lastTotals`/`fingerprint`/`messageKey`（`1513`） | `modelID` / `model` / `model.id`（`4272`），兜底 `unknown`（`3143`） | 丢弃（`3109`） |
| OpenClaw | `obj.timestamp` | 否（`2071`） | 字节偏移 + 事件身份计数（`1710`、`2114`） | `msg.model`，兜底 `unknown`（`2074`） | 丢弃（`2109`） |
| Kiro（IDE） | `row.timestamp`（按 UTC 补 `Z`） | 否（`7549`） | 自增 `id` 水位（`7541`）/ JSONL 行号（`7338`） | `.chat` 反查 → `kiro-agent`（`7555`） | 丢弃（`7547`） |
| Hermes | `ended_at ?? started_at` | 否（`7999`） | `started_at` 水位 + 未完会话强制重读（`7707`） | `row.model` → `hermes-agent`（`8006`） | SQL 层拦掉（`7718`） |
| Kiro CLI | `request_start_timestamp_ms` | 否（`9161`） | per-request 指纹（`9172`） | `model_id` / `session_model_id` → `kiro-cli-agent`（`9167`） | 丢弃；会话支路可产 reasoning-only 行（`9414`） |
| Kimi | `entry.timestamp ?? payload.timestamp`（秒） | 否（`9595`） | `size`/`ino` 偏移 + `message_id`（`9557`） | **来自配置文件**（`9540`）→ `kimi-for-coding`（`8080`） | 丢弃（`9590`） |
| Kimi Code | `entry.time ?? evt.time`（毫秒） | 否（`9828`） | 仅 `seenIds`，无偏移（`9783`） | `config.update.modelAlias`（`9769`）→ `kimi-for-coding`（`9704`） | 丢弃（`9823`） |
| CodeBuddy | `entry.timestamp`（毫秒） | 否 | `seenIds` + 用量指纹（`10360`） | 三级链（`10355`）→ `codebuddy-unknown`（`9924`） | 丢弃（`10345`、`10476`–`10485`） |
| WorkBuddy | `entry.timestamp`（毫秒） | 否（`10919`） | `seenIds` + 旧版 entry-id（`11007`、`11008`） | `provider.model` → `requestModelId` → `entry.model`（`10987`）→ `auto`（`10678`） | 丢弃（`10968`） |
| Roo Code | `msg.ts`（毫秒） | 否（`11899`） | `taskUuid:ts`（`11902`）+ 文件 mtime 闸门 | 兄弟文件 `<environment_details><model>`（`11805`） | 丢弃（`11910`） |
| Cline | `msg.ts` + 导入闸门（`12350`） | 否 | 三键兼容（`12369`）+ 五路差分（`12383`） | `modelInfo.id` → sidecar → `provider:<名>`（`12178`） | 丢弃（`12381`） |
| Zed | `updated_at ?? created_at ?? thread.updated_at` | **是（now）**（`12764`） | SQL `updated_at > cursor`（`12636`）+ 线程差分 | `thread.model.model`（`12594`） | 六列全零不可能；**缓存-only 仍可 input=0**（`12751`、`12752`） |
| LM Studio | 日志行本地时间（`12979`）→ **文件 mtime**（`13129`） | **是（mtime）** | `dev/ino/size/mtime` 三态（`13325`） | `metadataBuffer` 的 `model`（`13091`） | 丢弃（归一化器由 total 反推 input） |
| Unsloth | `created_at`（`13677`） | 否 | 扫描窗口 `(created_at, id)` + 侧车指纹（`13714`） | `response_model` → …（`13650`） | 丢弃 |
| AnythingLLM | `createdAt ?? lastUpdatedAt`（`13856`） | 否 | 自增 `id` 水位（`14010`）+ `pendingChatIds`（`14024`） | `row.model` → `anythingllm-unknown`（`14040`） | 丢弃（`14016`） |
| Goose | `created_at` | **是（now）**（`14713`） | DB mtime 短路（`14639`）+ 会话累计差分 | `model_config_json.model_name`（`14529`，**无兜底**） | 可产 reasoning-only 行（`14686`） |
| Droid | **文件 mtime**（`15210`、`15328`） | **是（mtime）** | 会话累计 + mtime（`15243`） | `resolveDroidModel` 三级链（`15046`–`15057`） | total 兜底填充（`14972`） |
| Kilo Code | `msg.ts`（`15617`） | 否 | `taskUuid:ts` + `{size, mtimeMs, ino}` | 只报 `provider:<slug>`（`11730`） | 丢弃（`15628`） |

**读表要点**：时间链存在**兜底到「当前时刻」或「文件触碰时刻」**的有 4 家（Zed、LM Studio、Goose、Droid）+ Grok（§5.3.6）；这 5 家的历史数据可能被记进「触碰时刻」的桶，与其余家「严格按日志时间戳落桶」的口径不可混用。**模型来源**列还揭示一件事：19 家里只有 Kimi 一族的模型名来自**非日志来源**（配置文件），其余全部来自会话/消息记录本身，或来自上游改写过的展示名。

### 5.3 SQLite / 数据库类

本节写清**读取后端**与**各家的库级形态差异**。文本类小节里已经逐家写过的 7 家 SQLite（Kiro、Hermes、Kiro CLI、Zed、Unsloth、AnythingLLM、Goose）不重复展开，只在下表列出它们的读法与特例。

#### 5.3.1 共用读取层：只有两级后端（系统 CLI + `node:sqlite`）

- **一级（系统 `sqlite3` CLI）**：`execFileSync("sqlite3", ["-readonly","-json", dbPath, sql])`（`src/lib/sqlite-reader.js:33`–`45`），靠 `-readonly` 参数保证只读。
- **二级（Node 22+ `node:sqlite`）**：`new DatabaseSync(dbPath, { readOnly: true })`（`src/lib/sqlite-reader.js:67`–`79`）。
- 同步版 `readSqliteJsonRows`（`sqlite-reader.js:81`–`128`）；异步孪生 `readSqliteJsonRowsAsync`（CLI 走 `execFile`，`node:sqlite` 回落仍是同步，`sqlite-reader.js:148`–`195`）。
- 默认 `timeout = 30_000`、`maxBuffer = 50 * 1024 * 1024`（`sqlite-reader.js:91`–`92`）；两级都不可用时**只打印一次**警告（进程级 Set 去重，`17`–`31`），只有调用方传 `throwOnReadFailure` 才抛错（`119`–`126`）。
- **生产路径不使用 JS 内嵌引擎**：`src/**` 对 `sql.js` / `better-sqlite3` 的命中数为 0；`package.json` 的 `dependencies` 只有 `@journeyapps/wa-sqlite`、`@mongodb-js/zstd`、`smol-toml`、`undici`、`yauzl`（`engines.node >= 20`）。（`sql.js@1.14.2` 与 `@electric-sql/pglite` 只存在于 devDependencies，供测试构造 fixture。）
- **没有显式 `BEGIN`**：一致性靠「WAL 边车指纹 + 快照复制 + 读后重算指纹」实现（各家 `sqliteSidecarFingerprint`）。
- **UNC / WSL 例外**：`\\wsl$` / `\\wsl.localhost` 上的库直接开会被 Plan 9/SMB 桥拒锁（注释 `src/lib/rollout.js:7693`–`7696`），所以先 `snapshotSqliteDb(dbPath)` 复制到本地临时目录再读。命中该分支的至少包括 opencode（`5194`–`5199`）、kiro（`7315`–`7320`）、devin（`14184`–`14189`）、antigravity（`21573`–`21581`）、copilot session-store（`18681`）。
- **zstd 的真实用途（防误读依赖）**：`@mongodb-js/zstd` 只服务两家——Zed 线程 blob（`12514`–`12533`）与 **DSH 会话日志**（`23066`–`23072`）；DSH 优先用 Node 内置 `zlib.zstdDecompressSync`，只在 Node 20 回落该依赖。`@journeyapps/wa-sqlite` 只服务 Trae（`src/lib/trae-sqlite.js:7`–`15`）。全仓**没有任何 LZ4 代码路径**。

#### 5.3.2 逐库要点

| Provider | 库与表 | 读法要点 | 关键行号 |
|----------|--------|----------|----------|
| OpenCode（DB 支路） | `opencode.db`，`message`(v1) / `session_message`(v2) + `session`/`session_v2` | **四种库形态**须分别处理（纯 v1 / beta-17887 / upstream v2 / transitional）；一次往返同时探测「v2 有无数据 + 哪张 session 表存在」；`data` 是纯 JSON 文本，无压缩列 | `5041`–`5045`、`5108`–`5133`、`5143`–`5147`、`5215` |
| Qoder（旧库） | `local.db`，`chat_message` JOIN `chat_record` JOIN `chat_session` | 三表 JOIN，`role='assistant' AND token_info 非空`；走**异步**读取版（热路径不阻塞事件循环）；CN 版**刻意不复用国际版 `QODER_HOME`**（否则同一库双计） | `6030`–`6050`、`6129`–`6135`、`6112`–`6127` |
| Devin CLI | `sessions.db`，`message_nodes`(+`sessions`) | 先探测表存在再拼 JOIN；用 `json_extract`/`json_valid` 就地投影；**只投影 token 与归属字段**，正文/`cogs_json`/凭据状态从不选取 | `devin-usage.js:25`–`58`、`14172`–`14181`、`devin-usage.js:11`–`13` |
| Copilot session-store | `session-store.db`，`assistant_usage_events`(+`schema_version`) | 先探测表与 `schema_version`；`id > lastId` 增量；**读前整库快照**（`snapshotSqliteDb`）避免读到半写状态；`token_details_json` 决定 `exact`/`fallback` 两档精度 | `18264`–`18278`、`18414`–`18447`、`18681`、`18534`–`18576` |
| Copilot App | `data.db`，`sessions` | 先 `PRAGMA table_info(sessions)` 判定可用列，再按需生成 `NULL AS <col>` 占位（兼容老 schema）；含内容列（`title` 等）**从不进查询** | `19255`–`19286`、`19270`–`19271` |
| Antigravity | `conversations/<uuid>.db`，`gen_metadata` | 用 `quote(data)` 把 **BLOB 转十六进制文本**，再手工解 protobuf（字段号硬编码）——不是压缩列，是二进制 protobuf 列；「读失败」与「读成功但为空」严格区分（`null` = 可重试失败，空 Map = 真无元数据） | `21586`–`21593`、`21598`–`21599`、`21609`–`21611` |
| TRAE（国际版） | `ModularData/{ai-agent,ai-chat}/database.db`，`chat_turn` | **绕开系统 sqlite3 与 `node:sqlite`**：先逐页解密成内存快照（SQLCipher 4 raw key / AES-256-CBC / 4096 字节页 / SHA-512 HMAC），再用 `@journeyapps/wa-sqlite` 的只读 VFS 查内存快照，全程不落盘；写操作一律拒绝；带 3 次重试 + 稳定性校验，**绝不发布混合快照** | `trae-snapshot.js:38`–`45`、`trae-sqlite.js:19`–`32`、`74`–`76`、`116`、`trae-db.js:102`–`125` |
| Claude Science | 行由上层数据库读取层给出（`dbRows`） | `normalizeClaudeScienceTokens` 处理「主计数 + `aux_*` 辅助计数」两组，各自独立 clamp 后再相减（避免一组缓存掩盖另一组缺口）；`total <= 0` 返回 `null` | `7003`–`7027` |
| Zed | `threads.db`（BLOB） | `data_type === "zstd"` 才解压（Node 内置优先） | `12514`–`12533` |
| DSH | 会话日志（非 SQLite） | 自研 **zstd 拼接帧**解析：先扫 `contentSize` 声明做解压炸弹防护，再逐帧解码；单文件压缩 64MB / 解压后 128MB 上限 | `23060`–`23072`、`22818`–`22819` |
| DuMate / ZCode / Mimo | OpenCode **派生库**（同一 `message` 表 schema） | 见 §5.3.3 | `dumate.js:32`、`5380`–`5446`、`5477`–`5533` |

#### 5.3.3 DuMate / ZCode / Mimo：三家 OpenCode 派生库（词表在 `source` 里，但解析器复用）

- **DuMate（千帆桌面端）**：`parseDumateIncremental`（`src/lib/dumate.js:32`）先自己按 `providerID === "qianfanpersonalquota"` 过滤（`dumate.js:37`–`47`），再做**跨安装最新副本选择**（`timeUpdated` 比较，`dumate.js:52`–`56`），最后把消息交给 `parseOpencodeDbIncremental`（`source: "dumate"`、`cursorKey: "dumate"`，`dumate.js:80`–`82`）。库路径 `%APPDATA%\qianfan-desktop-app\qianfan_desk_xdg\…\data\opencode\opencode.db`（`dumate.js:7`–`30`，候选含多安装目录并用 `realpath` 去重）。**已知差异**：DuMate 的 OpenAI 兼容适配器把 reasoning 留在 output 里，因此该家把 output 减去 reasoning 后再交给共用解析器（`dumate.js:70`–`75`）。
- **Mimo（mimocode）**：库 `~/.local/share/mimocode/mimocode.db`（`MIMO_HOME` 覆盖；Windows 另探 `%APPDATA%`），读取器 `readMimoDbMessages` **按 `providerID ∈ {mimo, xiaomi}` 过滤**（`src/lib/rollout.js:5396`–`5407`）。原因为 mimocode 会把用户的 Claude Code / claude-mem 历史镜像进自己的 `message` 表：开发机上镜像量约 39 亿 token、真实 mimo 用量约 2200 万——**不过滤会把 Claude 用量双计并错标成 mimo**，因此有一次性修复（清空 `source=mimo` 的队列行与桶，`src/commands/sync.js:4777` 起）。
- **ZCode（Z.ai / 智谱）**：库 `~/.zcode/cli/db/db.sqlite`（`src/lib/install-resolver.js:52`–`79`，含 `%APPDATA%\.zcode\...` 与 WSL 分支）。**判别方向与 Mimo 相反**：ZCode 允许用户自定义 provider（providerID 是随机 UUID），因此**不能用白名单**，改用**黑名单**排除 `anthropic` / `openai` / `google` 三个直连厂商（`isZcodeNativeMessage`，`src/lib/rollout.js:5430`–`5439`，理由见 `5410`–`5429`：那些 turn 会写进 `~/.claude` / `~/.codex` / `~/.gemini`，由各自解析器计数）。另有两处库级差异：**ZCode 的父计数器是「包含式」的**（cache read/write 含在 input 里、reasoning 含在 output 里），故先用 `normalizeZcodeInclusiveTokens` 拆开再算队列行与成本（`5441`–`5464`）；同时它还有新版原生用量表 `model_usage`（`ZCODE_NATIVE_USAGE_COLUMNS` + `detectZcodeNativeUsageLayout`，`5477`–`5533`），空库/不可读的判定分成三态（`zcodeDbHasContent`，`5555`–`5590`）。
- 三家的 `source` 都进桶键（`dumate` / `mimo` / `zcode`），因此**在最终统计里它们与 `opencode` 是四家不同的来源**，共用的是读取实现而不是身份。

#### 5.3.4 Copilot 三通道：谁负责计数、谁只做观测

三份实现共用 `source = "copilot"`，靠三层机制避免重复计：

1. **用量认领（OTEL ↔ session-store）**：OTEL CLI span 结算后写 `recentUsageEvents`，session-store 通道消费时不重复计；`usageClaims` 保留 7 天、上限 10000 条（`src/lib/rollout.js:17375`–`17405`、`18058`–`18064`）。
2. **App DB 的基线采纳与跳过**：`shouldCountCopilotAppSession` 明确跳过「可识别的 Copilot CLI / VS Code 扩展会话」（`19213`–`19224`），注释直说这是为了避免 GitHub 把这些会话镜像进 `data.db` 后产生同源重复桶；当 session-store 通道已激活时，App DB **只观测不计数**（`observeOnly`，调用点 `src/commands/sync.js:3207`–`3221`）。
3. **首见不计数（adoption）语义**：新发现的库首见只建基线、不计数，除非 `backfillOnFirstRun`（`18717`–`18732`）；与 App DB 的残留对账只补差量（`legacyResiduals`，`18834`–`18856`、`18942`–`18949`）。

另需注意：session-store 是**跨 App/CLI 都完整**的本地源（`18122`–`18125` 注释），App DB 只保留 App 归属的那部分；行身份用 `sha256(session_id + 模型 + 五桶 + 时间戳)` 指纹（`18188`–`18211`），**重建/重索引时按历史指纹多重集逐一核销**（`18755`–`18806`），这是 45 家里最复杂的去重设计。

#### 5.3.5 Qoder 旧库 / 新 JSONL：同名同源的两套通道

- 旧库通道是**全量快照 + 幂等重算**（不是水位）：每次同步重算全部行，未变则跳过，变了就**先减旧桶再加新桶**（`6307`–`6363`）；`conversation_count` 由「整份有序快照重算的 request 归属」决定，而不是沿用上次归属（`6277`–`6286`，原因：Qoder 可能把 `token_info` 补挂到更早的 assistant 行上，沿用旧归属会把一个 request 抬成两个会话）。
- 新 JSONL 通道是**集合差量**：每次重建 `currentByKey`，与游标逐键比对，消失或变化的先撤回、再补加（`6701`–`6711`、`6751`–`6773`）。
- 两通道上传时**同 `source="qoder"`**（`6412`–`6414` 注释），靠 `messageKey` 前缀 `row:` / `jsonl:` 与游标命名空间 `qoder` / `qoderNew` 隔离；CN 版复用同一函数、换路径与 cursor 命名空间（`qoder-cn` 走旧库、`sourceKey` 覆盖）。

---

#### 5.3.6 数据库 / 自有快照类的逐家细节补全

（本节补总表第 20–42 行里 §5.2 未展开的家。格式统一为：库/文件 → 字段 → 模型 → 时间 → 游标与重置 → 特例。）

**OpenCode（SQLite 支路）** — 路径 `$OPENCODE_HOME/opencode.db`（`src/commands/sync.js:1369`、`1348`–`1355`）；无压缩列（`JSON.parse(row.data)`，`5215`）。字段复用 `normalizeOpencodeTokens`（`4755`，与磁盘支路同一张表）；模型 `dbModelId || DEFAULT_MODEL`（`5905`–`5906`），**一条 assistant 消息只落一个模型桶、不为多模型拆分**。时间 `msg.time.completed ?? msg.time.created`，无则丢弃（`5892`–`5903`）。游标 `cursors.<ns>.dbCursor = {version, v1:{maxRowId,maxUpdatedAt,anchor}, v2:{…}}`（`5054`–`5062`、`5314`–`5324`），谓词 `rowid > maxRowId OR time_updated >= maxUpdatedAt`（`5097`–`5106`）；**头部锚点**（读前后各取头部行 sha256）防「库被原地替换」（`5088`–`5095`、`5287`–`5308`），两次仍不稳定则返回行但**不推进游标**（`5327`–`5328`）。差分是**带符号**的（允许负值做校正，`4815`–`4836`）。

**Qoder（旧库）** — 路径优先级 `QODER_DB_PATH` → `QODER_HOME` → 平台默认（`6064`–`6077`）；CN 版用 `QODER_CN_DB_PATH`/`QODER_CN_HOME` 且**刻意不复用国际版变量**（`6112`–`6127`）。字段 `token_info.prompt_tokens`（含缓存，`input = prompt − cached`）、`cached_tokens`、`completion_tokens`（`6151`–`6178`）；模型三级回落 → `"qoder-agent"`（`6185`–`6195`）；时间 `gmt_create`（`6263`–`6266`），无 key/无 tokens 的行同时跳过（`6267`–`6270`）。`messageKey = session_id|id`，缺失回落 `row:<rowid>`（`6198`–`6205`）；`project_uri` 以 `file://` 开头时做一次 `decodeURIComponent(new URL(raw).pathname)`（`6207`–`6216`）。跨设备/多安装靠**独立游标命名空间**隔离，避免不同机的 `rowid` 互撞（`6114`–`6119`）。

**Qoder（新 JSONL）** — 目录 `~/.qoder/projects`（CN 版 `~/.qoder-cn`，`6432`、`6454`），覆盖变量 `QODER_PROJECTS_DIR` / `QODER_CN_PROJECTS_DIR`（`6417`–`6419`、`6436`–`6438`），文件经 `listQoderNewSessionFiles`（`6457`–`6474`）。只解析 `record.type === "assistant"` 且 `message.role === "assistant"` 的行（`6622`–`6624`）；字段见 §5.5.2。模型剥掉 CN BYOK 的「安装本地 UUID 前缀」（`qoder-custom-<uuid>/glm-5.3-flash` → 裸 id），使桶键跨重装稳定（`6479`–`6487`）；时间取 `record.timestamp` 或 `record.message.timestamp`（`6505`–`6511`、`6632`–`6633`）。一次性迁移把旧 `qoder` 游标里的 `jsonl:` 条目搬到 `qoderNew`（`6564`–`6590`）。

**Devin CLI** — 路径 `~/.local/share/devin/cli/sessions.db`（`TOKENTRACKER_DEVIN_DB` 覆盖，`14106`–`14126`）；**Windows 无原生路径，只能经 WSL 探测**（`14120`–`14125`）。表探测 `message_nodes`/`sessions`（`src/lib/devin-usage.js:25`–`26`），主查询 `json_extract` 投影 + `json_valid` + `role='assistant'` + `request_id` 非空（`38`–`58`）。字段四分项**互斥**（input 不做缓存扣减，源码给依据 `21`–`23`）；`input`/`output` 为 NULL 表示**记录尚未完成 → 整行不计数**，缓存列 NULL 当 0（`76`–`87`、`113`–`119`）；时间优先「原始生成时间」，注释明确 **`n.created_at`（节点插入时间）永不作兜底**（`121`–`126`）。增量两级：库级 `sqliteSidecarFingerprint`（`-shm` 被刻意剔除，避免只读打开 bump 触发重扫，`14130`–`14133`、`19297`–`19314`）；行级用**全安装规范的 `request_id` 账本**，`cursors.devin.requests[requestId]` 首次观察为权威，replay/fork/compaction 产生的多行同 request 记录由 `compareCandidates` 裁决（`devin-usage.js:149`–`156`、`14307`–`14347`）；账本用 `Object.create(null)` 防 `__proto__` 污染（`14136`–`14147`），两个队列都 append 成功才发布游标（`14149`–`14169`、`14239`–`14246`）。

**OMP / OmO（共享实现）** — 会话文件 `<ompHome>/agent/sessions/--<cwd-encoded>--/<ts>_<sessionId>.jsonl`（注释 `11405`–`11407`），主会话与子 agent 两类文件分开发现（`11352`–`11499`）；家目录覆盖 OMP 用 `OMP_HOME`/`PI_CONFIG_DIR`（`11355`–`11356`）、OmO 用 `TOKENTRACKER_OMO_HOME`（`11582`–`11595`）；游标命名空间 `cursors.omp` / `cursors.omo`。字段（`15868`–`15872`）：`input`（**互斥列，不扣缓存**）/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens`；**OmO 的 `reasoningIncludedInOutput = true`**（`16110`），因此 total 兜底求和时必须排除 reasoning（`15897`–`15915` 与注释 `15898`）；`reasoningFields` 差异：omp 只认 `reasoningTokens`、omo 也认 `reasoning`（`16096`、`16109`）。时间 `msg.timestamp`（毫秒）或 entry ISO 串，**带 2020-01-01 下限**以防「占位小数字落进 1970 桶」（`15701`–`15710`），无时间戳即跳过（`15885`–`15891`）；增量用文件偏移 + inode 变化检测（`15838`–`15847`），**只提交到最后一个完整换行**（撕裂尾巴下次重读，`15724`–`15747`），`seenIds` 上限 10000（`16041`–`16050`），单记录上限 64MB（`15712`–`15715`）；项目归属用**独立游标**（`projectSeenIds`/`projectFileOffsets`）以便升级后回填历史而不影响总量（`15941`–`15946`）。OMP 是唯一带 extension hook 主动上报的一家（`src/lib/omp-hook.js:16`、`48`、`66`、`117`、`135`、`139`、`143`；安装点 `src/commands/init.js:806`），OmO 纯被动。另有防撞车守卫：OMP 与 OmO / pi 的 agent 目录相同时跳过对应 provider，防止同一批会话被两个 source 各记一次（`16157`–`16169`；`src/commands/sync.js:2735`–`2741`、`2772`–`2774`）。

**pi / Prime Agent / MiniMax Code（共享实现）** — `source` 由 provider 派生（`piSourceForProvider`，`16286`–`16295`；`primeAgentSourceForProvider`，`16297`–`16305`；MiniMax 恒 `"minimax-code"`，常量 `16722`、注入 `16775`–`16785`），设计意图是「pi 是路由器，同一会话可能发给 Anthropic / Copilot / 其它后端，不能合并进一个桶或继承错误价格」（`16281`–`16285`）。路径：pi 为 `~/.pi/agent/sessions/**/*.jsonl`（`16171`–`16223`）+ `~/.multica/pi-sessions/*.jsonl`（`16200`–`16220`）；Prime 为 `~/.prime/agent/sessions/*.jsonl`（`16235`–`16275`）；MiniMax Code 为 `~/.minimax/v2/sessions/YYYY/MM/DD/<HH-MM-SS-mmm>-session_<id>/messages.jsonl`（注释 `16711`，覆盖 `16728`）。字段三家同表（`16449`–`16453`）：`input`（不扣缓存）/`output`/`cacheRead`/`cacheWrite`/`reasoningTokens`，total 优先 `usage.totalTokens`、否则四分项 + reasoning（`16482`–`16485`）。时间 `msg.timestamp`（毫秒）优先、回落 entry ISO 串（`16466`–`16471`）——**MiniMax Code 例外：其记录包装器把外层 `timestamp` 恒置 `null`，ISO 回落分支永不触发，唯一有效路径是 `message.timestamp`**（`16766`–`16773`、`16467`、`16473`–`16476`）。增量与 OMP 同构（偏移 + inode + 只提交完整行 + `seenIds` 上限 10000，`16405`–`16426`、`16680`），解析前复制 `hourlyState.groupQueued` 与自有桶以保证队列写失败不污染调用方状态（`16369`–`16386`）。MiniMax Code 的 `usage.cost` 恒 0 被显式忽略、改由下游价格表算（`16716`–`16717`），历史迁移会话以 `model="historical-transcript"` + 全零 usage 重放并被零 token 护栏丢弃（`16717`–`16718`）。

**Craft Agents** — 磁盘布局 `~/.craft-agent/{config.json, workspaces/<id>/sessions/<sid>/session.jsonl}` + 自定义工作区根（注释 `16812`–`16816`）；**必须读 `config.json` 枚举自定义工作区**，不能只 glob 默认目录（`16818`–`16820`、`16862`–`16876`）。只读首行的预聚合头（读取上限 1 MiB，`17125`–`17135`；首行不是合法 JSON 即跳过，`17151`–`17158`）；字段 `usage.inputTokens`（**纯非缓存输入**，注释 `16822`–`16824`）/`outputTokens`/`cacheReadTokens`/`cacheCreationTokens`（`17172`–`17179`），delta 由会话头快照差分得出并 clamp 负值（`17189`–`17195`、`17236`–`17244`）。时间三级 `header.lastMessageAt` → `lastUsedAt` → `createdAt`，**末级兜底文件 mtime**（`17214`–`17223`）。游标 `cursors.craft.sessionTotals[sessionId]`（`17251`–`17258`），**上限 5000 条、按 `lastSeenAt` 最近优先淘汰**——注释说明用插入顺序淘汰会把长活会话的整份总量当增量重记（`17271`–`17275`）。`conversation_count` 是**会话级**且源码明确警告跨 provider 不可比（`17232`–`17235`）。

**Reasonix** — 只在 `~/.reasonix` / `%APPDATA%\reasonix` 下读 `*.jsonl.telemetry.json` 与其同名 `.meta`（`16959`、`16982`–`16985`、`16925`–`16934`），**刻意只读「无正文」文件以保证 prompt 与工具输出不外流**（`16907`–`16908`）；扫描深度上限 8（`16949`）。字段经 `normalizeReasonixTotals`（`16989`–`17005`）：`promptTokens` 按 `cacheMissTokens` 是否存在分两种语义算纯输入，`cacheHit`/`cacheMiss`/`cacheWriteTokens` 归缓存桶，`output = completionTokens − reasoningTokens`，**`conversation_count = requestCount`**（`17044`）。时间 `meta.updated_at` → `created_at` → 文件 mtime（`17016`–`17022`）；增量是累计快照差值且**只 clamp 不做负向回撤**（`17008`–`17014`），差值全 0 且请求数未增则整文件跳过（`17067`–`17068`）。**文件级身份只按路径，没有跨安装去重**（多份同名文件在不同 home 下会各算一次）。

**Copilot（OTEL 通道）** — 需要用户在 Copilot 侧开启 `COPILOT_OTEL_ENABLED` + 文件导出（注释 `17301`–`17306`）；读侧目录 `~/.copilot/otel`、`~/.copilot-otel` 或显式 `COPILOT_OTEL_FILE_EXPORTER_PATH`（`17309`–`17341`，Windows 另探 WSL `17328`–`17335`）。字段（`17501`–`17521`）：`gen_ai.usage.input_tokens`/`output_tokens`/`cache_read.*`（四镜像）/`cache_write.*`（四镜像）/`reasoning.*`（三镜像）；**CLI span 的 input 同时含 cache read 与 write（双扣），Chat 扩展的 LogRecord 只含 read（单扣）**——同一 provider 两代导出形态语义不同，是本项目唯一按代分别处理的地方（`17523`–`17535`）；reasoning 从 output 拆出（`17521`–`17522`、`17578`–`17579`）。模型 `gen_ai.response.model` → `gen_ai.request.model`（`17367`–`17373`）→ `normalizeCopilotAppModel`（`19244`–`19252`）→ `"github-copilot"`（`19033`）。时间是 OTEL 的 `[秒, 纳秒]` 数组（`17358`–`17365`，字段顺序 `endTime`→`startTime`→`hrTime`→`hrTimeObserved`，`17545`–`17549`）。去重三重：文件偏移 + inode（`17983`–`17992`，读后再 `stat` 吸收解析期间的追加，`18078`–`18086`）、记录级 `traceId:spanId` 或 `resp:<response.id>`（`17447`–`17460`）、解析器版本迁移（`COPILOT_PARSER_VERSION = 3`，`17375`；v1→v2 按行哈希跳过已计前缀 `17917`–`17927`，v2→v3 因 v2 误用 `spanContext` 作键而需先修复、修不了就**保持 v2 游标下次重试** `17930`–`17951`）。

**Copilot（session-store 通道）** — 覆盖变量 `TOKENTRACKER_COPILOT_SESSION_STORE_DB` / `COPILOT_HOME`（`18162`、`18146`–`18153`），文件名 `session-store.db`。字段**两档精度**（`18534`–`18563`）：若 `token_details_json` 的 `input + cacheRead + cacheWrite` 与 `output` 完全等于行内总量 → 采用明细、精度记 `exact`；否则按行内列扣减、精度记 `fallback`（`precision` 只用于诊断/披露，不进 totals，`1373` 段）；reasoning 被 clamp 到 output 内再拆出（`18565`–`18576`）。时间不可解析的行**登记进 `pendingMalformedEvents` 留待下次重试而不是丢弃**（`18800`–`18802`、`18870`–`18876`）。游标 `cursors.copilotStore.dbs[dbPath]` 含 `lastId`/`lastRowKey`（游标行 sha256）/`seenEventCounts`/`pendingMalformedEvents`/`dbIno`/`adoptedAt`；**id 回退或游标行指纹变化即判库被重建/重索引**并走全量对账（`18698`–`18720`）；同一物理库的多路径别名先合并状态防双计（`18610`–`18617`）。

**Copilot（App DB 通道）** — 路径 `~/.copilot/data.db`（覆盖 `TOKENTRACKER_COPILOT_APP_DB`，`19190`–`19198`）。字段是**会话级累计的差值**（`19357`–`19379`）：`total_input_tokens` 含缓存，故 `cachedDelta` 先 clamp 到 `inputDelta` 内再扣除；`reasoning` 从 output 拆出；**`cache_creation_input_tokens` 恒 0**（App DB 没有 cache write 列）。时间用排序后的 `updated_at`/`created_at`（`19266`–`19269`）经 `parseCopilotAppTimestamp`（兼容秒/毫秒数字与可解析字符串，`19227`–`19241`）——**由于是会话级累计，一条会话的全部历史 token 会落在「本次观测到的那个半小时桶」里**（与 Grok 的估算路径同类风险）。游标：库级指纹未变即跳过（`19439`）、会话级 `sessionTotals` 差值（`19419`–`19422`），会话总数上限 10000 且**非零会话优先保留**、零值会话按 `updatedAt` 倒序补位（`19334`–`19355`）。

**Grok Build** — 会话目录 `~/.grok/sessions/<encodedCwd>/<sid>/`，同时存在 `signals.json` 或 `updates.jsonl` 才纳入（`19566`–`19605`），家目录 `TOKENTRACKER_GROK_HOME` → `GROK_HOME` → `~/.grok`（`19553`–`19564`）。字段首选 `normalizeGrokUsage`（`src/lib/grok-usage.js:44`–`61`）：**camelCase 时 `inputTokens` 含缓存（要双扣），snake_case 时已是纯输入（不扣）**（语义注释 `grok-usage.js:3`–`10`），`reasoningTokens` 从 `outputTokens` 拆出，`totalTokens` 为 0 时返回 `null`（`grok-usage.js:63`–`90`）；**一家一次 turn 可拆成多个模型事件**（`usage.modelUsage` 逐模型，`19822`–`19839`），这是 45 家里唯一支持单请求多模型的实现。模型兜底 `"grok-build"`（`19675`–`19682`），免费 SKU 规范化为 `"grok-build-free"` 以免与付费 `grok-4.5` 模糊匹配（`19780`–`19791`）。时间三兜底 `event.timestamp` → `lastActive` → `Date.now()`（`20214`、`19684`–`19693`）。增量：文件偏移 + inode（`19870`–`19883`）且**只推进到最后一个换行处**（`19886`–`19900`、`grokFileEndsWithNewline` `19758`–`19777`）；会话快照 `cursors.grok.sessionSnapshots` 上限 10000（`19645`–`19649`）；游标版本 `GROK_CURSOR_VERSION = 5`（`19551`），升级时**清空 grok 桶**并从 `turn_completed.usage` 重建（`20099`–`20123`）。

**Antigravity** — 转录路径在 `<geminiHome>` 下的 `antigravity` / `antigravity-ide` / `antigravity-cli` 三个 `brain` 目录（`resolveAntigravityBrainDirs`，`20559`–`20566`），每个会话目录下的 `.system_generated/logs/transcript.jsonl`（`20604`–`20615`）；配对库由转录路径正则反推 `conversations/<uuid>.db`（`21558`–`21563`）。SQLite 路径字段号硬编码（`21505`–`21535`、落桶 `21794`–`21808`）；兜底路径用字符估算（见 §5.5.4）。模型三级（转录内「设置变更」文本 → `settings.json` → SQLite `f1.19`）并做 slug 归一：剥括号与 thinking/xhigh/high/medium/low/fast 词，再按 `gemini|claude|gpt` 截断，否则加 `antigravity-` 前缀（`21929`–`21949`；默认模型 `21904`–`21918`，桶内兜底 `"antigravity-unknown"` `21785`）。时间取转录事件 `parsed.created_at`，缺失则该事件**只累加上下文、不落桶**（`21771`–`21783`）。游标：`cursors.files[filePath]` 记 `lastLine`/`size`/`inode`/`contributions`/`cursorVersion`/`usageSource`（`20641`、`20645`），归属账本按 `source|model|bucketStart|projectKey` 聚合、贡献上限 4096（`20646`、`21847`–`21858`）；账本被截断且需要撤回时触发**源级重建**，但发现不完整（`inventoryComplete=false`）时改为**推迟该文件**以免抹掉未发现会话的贡献（`20858`–`20908`）；SQLite 读失败且上次来源是 sqlite 时整文件推迟、**不回退到估算**（`21682`–`21697`）。

**TRAE（国际版）** — 见 §5.3.2 的读法要点；补字段与游标：`normalizeTraeUsage` 先做「候选唯一解」判定（含/不含缓存与推理的 4 种组合必须只有一种与 `total_tokens` 自洽，否则整行放弃，`src/lib/trae-usage.js:63`–`81`），Gemini 行有两处显式修复（缓存双写误判与 thoughts 缺失残差，均标 `estimated`，`45`–`56`），另有一类「知道计量但无法拆分」的差额只进 `total_tokens` 并挂 `unpriced_input_tokens`（`83`–`92`）。模型归一：剥 `__` 后缀、小写、空格转 `-`，并把 `"claude-4-sonnet"` 映射到 `claude-sonnet-4-20250514` 以免模糊匹配丢缓存价（`98`–`105`）。时间 `created_at` 优先、`updated_at` 用于冲突裁决，**无兜底**（`rollout.js:22094`、`22107`、`22097`–`22100`）。游标用 128 位摘要身份（`digest`，`22062`）：`turn_id` 存在时用 `[session, turn_id]`、否则用 `[databaseKey, rowId]`（`22104`–`22111`），**跨两家国际版商店的副本靠 `T_STORE` 与 `T_MODIFIED` 裁决**（不让陈旧库回撤更新的用量，`22104`–`22111`）；版本 `TRAE_CURSOR_VERSION = 2`（`22002`），不匹配时先清 trae 桶重建（`22049`–`22054`），重建要求所有库都读成功否则整轮回退（`22159`–`22164`），重建后还要**撤回旧桶对应的队列行**（`22168`–`22173`）。

**DSH** — 路径 `~/.dsh/sessions/<project-key>/<session-id>/session[.v3].jsonl[.zstd]`（注释 `22878`–`22880`；家目录 `TOKENTRACKER_DSH_HOME` → `DSH_HOME` → `~/.dsh`，`22826`–`22833`，Windows 另探 WSL `22839`–`22865`）；同名文件多版本时按「最近写入 > 更高版本 > zstd」择一（`22901`–`22925`）。字段为五个**互斥**列（`23345`–`23365`，注释明确「DISJOINT…no cache subtraction, unlike Codex」）；模型回落链「事件内 `data.message.source.model` → request/header 配置 → 事件自身」，并只取最后一个 `/` 之后的名字（`23649`–`23652`、`23336`–`23342`），无模型即跳过（`23658`）。时间取事件级 `time`/`timestamp`（epoch 毫秒），缺失即跳过（`23660`–`23666`）。游标：文件身份 `cursors.dsh.files[path]`（inode/size/mtime，`23761`–`23793`）+ **记录级 `seq` 水位**（`23628`–`23645`），撕裂行要求「行自身是完整 JSON」才推进水位（`23610`–`23615`）；会话贡献账本支持「同会话换文件路径」的迁移（新路径接管先撤回旧路径贡献，`23717`–`23746`、`23766`–`23786`）。**隐私设计**：刻意不做整行 JSON 解码（用切片取字段），避免物化消息正文（`23368`–`23372`、`24244`–`24248`）；zstd 是自研**拼接帧**解析，先扫 `contentSize` 做解压炸弹防护，单文件压缩上限 64MB、解压后 128MB（`23060`–`23072`、`22818`–`22819`）。

**Command Code** — 路径 `~/.commandcode/projects/<cwd-slug>/<session-id>.jsonl`（注释 `24129`–`24132`，家目录 `24085`–`24089`、WSL `24095`–`24127`），排除 `.prompts.` 与 `.checkpoints.jsonl`（`24074`–`24081`）。字段：`usage.inputTokens` **含缓存 → 双扣**、`cacheReadTokens`/`cacheWriteTokens`/`outputTokens`，reasoning 恒 0、成本恒 0 哨兵（`24187`–`24215`）；**显式全零视为「校正」**（能撤回既有观测），空占位不算使用（`24199`–`24204`）。模型取行字段最后一个 `/` 之后（`24176`–`24185`），缺失落 `DEFAULT_MODEL`（`24283`）。时间戳必须是能被 `new Date(...)` 解析的字符串，否则该记录被丢弃（`24262`–`24276`）。游标**刻意不用字节偏移**（注释说明 resume/compaction 会重写文件）：重建每文件快照并与持久观测账本按 `recordId|timestamp` 对账（`24060`–`24066`、`24542`；另有 legacy 键形 `24544`），版本常量 `COMMAND_CODE_STATE_VERSION`/`FILE_CACHE_VERSION = 3`（`24068`–`24069`、`24473`–`24475`）；同一记录出现在非拥有者文件时**重读该文件**解析冲突而不是双记账本（`24614`–`24627`）；零 token 记录只在能撤回既有计数时保留（`24606`–`24612`）；成本无关的变化不触发入队（`24629`–`24641`）。

#### 5.3.7 多安装 / WSL / 跨设备处理速查

| 机制 | 涉及家 | 源码 |
|------|--------|------|
| UNC 路径先快照再读（避开 Plan 9/SMB 桥的锁失败） | `opencode`、`devin`、`antigravity`、`copilot`(store) | `5194`–`5199`、`14184`–`14189`、`21573`–`21581`、`18681` |
| WSL 家目录探测 + 原生/WSL 双路径解析 | `qoder`(两套)、`devin`、`dsh`、`command-code`、`copilot`、`omp`/`omo`/`pi`/`prime`/`minimax`/`craft`、`grok` | `6091`–`6109`、`6424`–`6452`、`14120`–`14125`、`22839`–`22865`、`24095`–`24127`；`pickWin32ProviderPath`（`7650`–`7653`） |
| 多安装游标命名空间隔离 | `opencode`（`native`/`wsl`/`flat`）、`zcode`（保留首次选择的安装，避免两个库累进同一个 flat 游标） | `src/commands/sync.js:1429`、`5788`；`rollout.js:5600`–`5605` |
| 路径相同时跳过第二家（防双计） | `omp` vs `omo`、`omp` vs `pi` | `16157`–`16169`；`src/commands/sync.js:2735`–`2741`、`2772`–`2774` |
| DB 别名路径合并（同物理库多路径） | `copilot`(store)、`copilot`(App) | `18610`–`18617`、`3153`–`3157` |
| 会话级跨设备对账（云端 LWW） | `trae-cn` | `22532`–`22595` |
| 副本/分叉去重（本地） | `opencode`（#426 指纹）、`devin`（`request_id` 账本）、`trae`（turn 摘要账本）、`command-code`（`recordId\|timestamp`）、`claude`（fork 别名） | `5847`–`5888`、`14303`–`14347`、`22104`–`22111`、`24542`、`989`–`1013` |
| 复制型安装的「最新副本优先」 | `dumate`（跨安装同消息取最新 `timeUpdated`） | `src/lib/dumate.js:52`–`56` |

#### 5.3.8 读取参数与容量上限速查（内存 / 延迟相关）

| 常量 / 参数 | 值 | 位置 | 作用 |
|-------------|----|------|------|
| SQLite 读取层默认超时 / `maxBuffer` | 30s / 50MB | `src/lib/sqlite-reader.js:91`–`92`（异步同值 `158`–`159`） | 所有 SQLite 读取的默认值 |
| 逐家上调的读取缓冲 | Devin 32MB/30s；Copilot store 64MB/60s；Copilot App 64MB/60s；Antigravity 64MB/15s | `14172`–`14181`、`18430`–`18437`、`19287`–`19294`、`21586`–`21593` | 大库专项参数 |
| OMP / pi 家族单记录上限 | 64MB | `15712`–`15715` | 防单条超大记录拖垮解析 |
| `seenIds` 去重集合上限 | 10000 | `16041`–`16050`（OMP/OmO）、`16680`（pi 家族）、`18089`–`18091`（Copilot OTEL） | 超限后先入先出淘汰 |
| CodeBuddy 用量指纹表上限 | 10k | `10022` | 同上 |
| Craft `sessionTotals` 上限 | 5000，按 `lastSeenAt` 最近优先淘汰 | `17271`–`17275` | 防止淘汰长活会话后把整份总量当增量重记 |
| Zed `threadTotals` 上限 | 10000 | `12816` | 线程累计差分表 |
| Grok 会话快照上限 | 10000 | `19645`–`19649` | 会话级基线 |
| Copilot App 会话表上限 | 10000（非零会话优先保留） | `19334`–`19355` | 保留策略 |
| Copilot `usageClaims` 窗口 | 7 天 / 10000 条 | `17375`–`17405`、`18058`–`18064` | 跨通道用量认领 |
| Antigravity 归属贡献上限 | 4096 | `20646` | 超限且需撤回 → 源级重建；发现不完整 → 推迟 |
| DSH 单文件上限 | 压缩 64MB / 解压后 128MB | `22818`–`22819` | 解压炸弹防护 |
| LM Studio 扫描窗口 | 8MB 窗口 / 64KB 分块 | `12853`、`12854` | 文本日志字节扫描 |
| Craft 单文件读取上限 | 1 MiB | `17125`–`17135` | 只读首行头 |
| Unsloth 尾部保留 | 末 256 行 | `13469` | 扫描窗口回落区间 |

### 5.4 API / 凭据类

45 家里**只有两家**必须联网且必须读取本机登录凭据才能产出任何数据，且两家都是**账号级来源**（`ACCOUNT_LEVEL_SOURCES = new Set(["cursor","trae-cn"])`，`src/lib/source-metadata.js:1`，用于把「仅本地」视图过滤掉它们）。

#### 5.4.1 `cursor` — 远程 CSV 导出 + `state.vscdb` 取 JWT

- **触发条件**：`sourceAllowed("cursor")` 即尝试；未安装 Cursor 或取不到凭据则**整段 try/catch 跳过**，只打印一行错误，不影响其它 provider（`src/commands/sync.js:2158`–`2194`）。
- **凭据来源**：SQLite `Cursor/User/globalStorage/state.vscdb`，`SELECT value FROM ItemTable WHERE key = 'cursorAuth/accessToken'`（`src/lib/cursor-config.js:46`–`47`），走 §5.3.1 的两级后端（`timeout: 5000`、`maxBuffer: 1MB`，`56`–`66`）。`userId` 取 `~/.cursor/cli-config.json` 的 `authInfo.authId`，失败则从 JWT 解出（`102`–`107`）；最终组装 `WorkosCursorSessionToken=<userId>%3A%3A<jwt>` cookie（`109`–`111`），并有 WorkOS 主体前缀白名单（`114`–`116`）。
- **端点与超时**：`https://cursor.com/api/dashboard/export-usage-events-csv?strategy=tokens`、`https://cursor.com/api/usage-summary`、`https://api2.cursor.sh`（`150`–`152`）；CSV 与 summary 各 30s、RPC 15s（`196`、`231`、`367`）。
- **失败降级**：没有 refresh token 流程、也**没有「仅本地估算」开关**——CSV 拿不到就是没有数据。与其它家的差别是**解析器本身不碰网络**：它接收的是上游已经解析好的 CSV 记录（`7164`–`7170` 注释）。
- **增量语义（与所有本地家根本不同）**：CSV 是账号级导出、不是追加型本地日志，因此按「先清桶再重建」处理，且清桶范围有护栏——只清「本次导出最早记录」之后的桶；**一条日期都解析不出来就完全不清桶**（避免把整段历史写成 0），`7184`–`7210`。游标 `cursors.cursorApi.lastRecordTimestamp` 只作展示/诊断，**不参与过滤**（`7244`–`7247`、`7263`–`7268`）。
- **成本与计数**：`total_cost_usd` 从不写入（成本交下游价格表，`cursor-config.js:525`–`536` 注释）；`conversation_count` 每条 CSV 记录 +1（`rollout.js:7232`）——**是请求级计数而非会话级**。历史坑：早期版本因 Cursor 新增 `Cloud Agent ID` / `Automation ID` 列而把记录写成 `model="unknown"`，有一次 purge + 游标重置（`src/commands/sync.js:2144`–`2152`）。

#### 5.4.2 `trae-cn` — AES 加密凭据 blob + 强制 opt-in

- **触发条件（三重）**：非轻量后台同步 **且** `TOKENTRACKER_TRAE_CN_USAGE` 显式为 `1`/`true` **且** `sourceAllowed("trae-cn")`（`src/commands/sync.js:2234`–`2238`）；调用侧固定 30 天滚动窗口并对齐半小时（`2241`–`2250`）。
- **隐私边界（唯二的强制闸门）**：源码注释写明「该读取会把本机存储的登录 JWT 发给 TRAE 官方端点，因此严格 opt-in：除非用户显式设置该标志，否则什么都不会发出；除 1/true 外任何值都视为关闭」（`src/lib/trae-cn-config.js:24`–`33`）。凭据来自 `%APPDATA%\TRAE SOLO CN\User\globalStorage\storage.json` 的 `iCubeAuthInfo://icube.cloudide`，可能是明文 JSON，也可能是 Base64 的 tc v5 密文（`[6 字节 magic][32 字节 salt][AES-128-CBC 密文]`，key/iv 由 `SHA-512(salt)` 与 `JG xor KG` 两次 KDF 导出；`trae-cn-config.js:11`–`16`、`35`）。
- **端点与请求**：`https://api.trae.cn/trae/api/v1/pay/query_user_usage_group_by_session`，POST + `Authorization: Cloud-IDE-JWT <jwt>`，body 含 `usage_type: [7]`、分页 20 / 最多 100 页 / 页间 300ms / 超时 30s（`37`–`45`、`332`–`344`）。`usage_type` 常量取自「官方客户端用量页实际发出的请求」，注释明确该枚举服务端无文档、其它取值未验证。
- **失败降级**：**401/403 时重读并重新解密 `storage.json` 后重试恰好一次**，不做 refresh token 流程，且**从不持久化或打印 JWT / refresh token**（`486`–`521`）；容量超限时递归二分窗口（最多 8 层），二分仍超限则 fail-closed（`46`–`49`、`465`–`483`）。
- **快照契约（fail-closed）**：空响应是**成功空操作**（不当作「全部删除」）；单个畸形行让**整份快照失败**，不作为权威状态入队——源码给出证据说明「会话集合稳定」但「响应中缺席 ≠ 权威删除」未被证明，故缺席契约必须保持对称（`22626`–`22634`）。持久化贡献在减去前逐字段校验（安全整数、`reasoning=0`、`conversation_count=1`、`total=input+cached+cacheWrite+output`、`billable=total`、桶起点必须是标准 UTC 半小时），撤回前还要断言旧贡献不超过当前桶（`22402`–`22413`、`assertTraeCnBucketCovers` `22513`–`22530`）。跨设备另写 `kind: "account_session_state"` 队列记录，带 `snapshot_verified_at` 逻辑戳做云端 LWW 收敛（`22532`–`22595`）。
- **语义判断的来源**：`input_token` 是「含缓存」这一点，是**用 TRAE CN 自家积分账单反推验证**出来的（`credits = fresh_input*p + cache_read*(p/4) + output*q` 对全体观测行零残差），而**不是**文档（`rollout.js:22467`–`22474`）。若 TRAE 调整积分权重，该结论需重验。

#### 5.4.3 两家账号级来源对照

| 维度 | `cursor` | `trae-cn` |
|------|----------|-----------|
| 触发条件 | `sourceAllowed("cursor")` 即尝试；未安装 / 无凭据整段跳过（`src/commands/sync.js:2158`–`2194`） | 非轻量同步 **且** `TOKENTRACKER_TRAE_CN_USAGE` 为 `1`/`true` **且** `sourceAllowed("trae-cn")`（`src/commands/sync.js:2234`–`2238`） |
| 凭据位置 | `state.vscdb` 的 `cursorAuth/accessToken`（`src/lib/cursor-config.js:46`–`47`）+ `cli-config.json` 的 `authInfo.authId`（`102`–`107`） | `storage.json` 的 `iCubeAuthInfo://icube.cloudide`（明文或 Base64 的 tc v5 密文）（`src/lib/trae-cn-config.js:35`、`11`–`16`） |
| 鉴权形式 | `WorkosCursorSessionToken=<userId>%3A%3A<jwt>` cookie（`109`–`111`，含 WorkOS 主体前缀白名单 `114`–`116`） | `Authorization: Cloud-IDE-JWT <jwt>` 头（`332`–`344`） |
| 端点与超时 | CSV 导出 + usage-summary（各 30s）、RPC（15s）（`150`–`152`、`196`、`231`、`367`） | 单一 usage POST（30s；分页 20、最多 100 页、页间 300ms）（`37`–`45`） |
| 失败降级 | 无 refresh 流程、**无本地估算兜底**——拿不到就是没有数据 | 401/403 时**重读并重新解密一次**；容量超限递归二分窗口（≤8 层），仍超限 fail-closed（`486`–`521`、`46`–`49`、`465`–`483`） |
| 凭据落盘/打印 | 不落盘，仅内存 cookie | **从不持久化或打印 JWT / refresh token**（`486`–`521` 注释） |
| 增量语义 | 账号级快照替换 + 清桶护栏（只清「本次导出最早记录」之后的桶；一条日期都解析不出则**完全不清桶**）（`src/lib/rollout.js:7184`–`7210`） | 会话级快照对账，全部 fail-closed；空响应 = 成功空操作；单行畸形 → **整份快照失败**（`22626`–`22634`） |
| 计数口径 | `conversation_count` = CSV 记录数（请求级，`7232`） | 每会话行恒 1（`22500`–`22508`） |
| 隐私边界 | 账号级来源，被排除在「仅本地」视图外（`src/lib/source-metadata.js:1`） | 同上，且是**唯一需要显式 opt-in 才会发出网络请求**的来源 |

#### 5.4.4 opt-in 开关与路径覆盖的边界

**真正意义上的「功能 opt-in 开关」只有 `TOKENTRACKER_TRAE_CN_USAGE` 一个。** Copilot 的三个 `COPILOT_OTEL_*` 是「让上游工具开始导出」的前置条件（对 TokenTracker 而言是数据存在性条件，不是自身开关）；其余 `TOKENTRACKER_*` 变量全部是**路径覆盖**，不具备关闭能力。`TOKENTRACKER_TRAE_SQLCIPHER_KEY` 也不是 opt-in，而是 Trae 库密钥的可选覆盖（默认使用内嵌共享密钥，`src/lib/trae-snapshot.js:9`–`10`；覆盖与格式校验 `38`–`40`；状态页会披露用的是哪个来源，`src/commands/status.js:1215`）。

---

### 5.5 估算与非 token 单位

本节把「不是真实 token 计数」的取值全部列出，含**反例**（明确拒绝把非 token 单位当 token 的家）。

#### 5.5.1 Kiro CLI：字符估算（何时用真实值、何时回退）

- **先真实、后估算**：legacy 会话 `.json` 支路取 `turn.input_token_count` / `turn.output_token_count`（`src/lib/rollout.js:8371`、`8372`），**只有两者皆为 0 才回退**字符估算（闸门 `8374`，估算点 `8386`、`8387`）。
- **纯估算支路两条**：SQLite `data.sqlite3`（`9152`–`9159`，乘法点 `9155`）与 v2 `messages.jsonl`（该文件不含 token 字段，注释 `8475`–`8481`）；换算率统一 `KIRO_CLI_CHARS_PER_TOKEN = 4`（`8118`，其余使用点 `8386`/`8387`/`8429`/`8430`）。
- **落桶闸门**：`approxInput > 0 || approxOutput > 0 || approxReasoning > 0`（`9209`）——**六列全零 + total>0 的形态不出现**。
- credits 不进 token 桶，只写侧车 `kiro-credits.json`（`8119`、`8955`）。

#### 5.5.2 Qoder：`credits` 进 totals 但不换算

- 全项目唯一把 `credits` 写进 totals 的地方：`rollout.js:6541`（`credits: Number.isFinite(credits) && credits > 0 ? credits : 0`），来源 `usage.credits ?? usage.original_credits`（`6515`）。
- **不换算、不计入 total**：`addTotals` 只识别九个字段（`4099`–`4115`），`credits` 不在其中，因此**当前不构成数值污染**。真正的风险是下游若按 totals 的任意字段做泛化求和会误用 credits。
- **悬空注释（注释与实现不一致，须登记）**：`6726` 写着「credits cost 很小，确保它被累积」，但下一行 `6727` 就是 `touchedBuckets.add(...)`——**上下文里没有任何 credits 累积语句**。同家注释明确拒绝换算（`6405`–`6411`：没有第一方证据支持 credit→token 汇率，`total_cost_usd` 保持 0）。

#### 5.5.3 Grok Build：上下文窗口 token 按 0.8 硬拆 input/output

- **触发条件**：该会话历史上从未出现 `turn_completed`（`rollout.js:20238`–`20241`）。
- **数据源是上下文窗口的瞬时大小，不是可计费累计**（源码自己的警告 `19704`–`19706`）：水印取自 `signals.totalTokens` / `totalTokensBeforeCompaction` / `contextTokensUsed`（`grokEffectiveTotalFromSignals`，`19707`–`19718`）。
- **硬拆比例**：`GROK_ESTIMATED_INPUT_RATIO = 0.8`（`19549`），`input = round(total × 0.8)`、`output = total − input`，且**该路径的 cached 与 reasoning 恒为 0**（`19945`–`19961`）。
- **免责披露**：该路径写 `usage_precision = "estimated"` 并参与桶级精度合流（`20255`、`20268`、`20286`–`20289`）；同一桶内混有 reported 与 estimated 时标记为 `"mixed"`（`19964`–`19968`）。因为缓存与推理恒 0，**估算行的价格会系统性地与真实行不同**。
- 上游成本（唯一直接用上游美元 ticks 的本地 provider）：`USD_TICKS_PER_USD = 10_000_000_000`（`grok-usage.js:12`），`ticks / 1e10` 得美元；`costIsPartial` / `usageIsIncomplete` 为真时返回 `null`（不写假精确的 0），`grok-usage.js:26`–`39`。
- 版本迁移的教训值得抄：`GROK_CURSOR_VERSION = 5`（`19551`），注释说明 v3 及更早把「上下文窗口总量」当「累计消费」、v4 仍重叠 output/reasoning、丢弃 cache creation、忽略上游成本，因此 v5 升级时**清空 grok 桶并从 `turn_completed.usage` 重建**（`20099`–`20123`）。

#### 5.5.4 Antigravity：字符估算当 token，且不写 `usage_precision`

- 兜底路径的估算器 `estimateAntigravityTokens`（`21971`–`21983`）：CJK 码点按 1 token、其它按 `ceil(n/4)`（`isCjkCodePoint` 覆盖 `0x3400`–`0x4dbf`、`0x4e00`–`0x9fff`、`0x3040`–`0x30ff`）。
- 该路径把估算值直接写进 input/output/reasoning（`21821`–`21833`），且 `input` 是**上下文增量**而非真实 prompt 长度（上一轮 planner 的历史在下一轮被计为 input，注释 `21710`–`21713`、`21881`–`21887`）；两个缓存桶保持 0。
- **披露缺口**：来源标记 `usageSource: "estimated"` **只记在游标里**（`21898`），**没有任何 `usage_precision` 写进队列行**——`usage_precision` 的全部写入点清单里没有 antigravity（清单见 §5.6 末）。因此**下游无法区分 antigravity 的 SQLite 精确行与字符估算行**，这是本章范围内最明确的披露缺口。
- 与之对照，Trae 会把精度写进队列行的 `usage_precision`（`22144`–`22158`）。

#### 5.5.5 非 token 单位清单（含反例）

| 家 | 非 token 量 | 处理方式 | 是否污染 token 列 | 行号 |
|----|-------------|----------|------------------|------|
| `qoder`（新 JSONL） | `credits` / `original_credits` | 原样挂到 totals 的 `credits` 键；不换算、不入 total | 否（`addTotals` 白名单外） | `6515`、`6541`、`4099`–`4115` |
| `reasonix` | `requestCount`（请求数） | **当 `conversation_count` 用**——请求数冒充会话数 | 否（不是 token，但污染「会话数」指标） | `17004`、`17044` |
| `grok` | 上下文窗口 token | 按 0.8 硬拆 input/output，标 `estimated` | 是（估算 token 进桶，但被标精度） | `19549`、`19945`–`19961`、`20255` |
| `grok` | 上游美元 ticks | 走独立字段 `total_cost_usd`，不碰 token 列 | 否 | `grok-usage.js:12`、`26`–`39` |
| `antigravity` | 字符数 | 估算成 token 写进三桶 | 是（且**不标精度**） | `21971`–`21983`、`21821`–`21833`、`21898` |
| `kiro`（CLI） | 字符数 | 估算成 token（legacy 支路优先真实值） | 是（标不到精度） | `8371`–`8387`、`9152`–`9159` |
| `trae`（国际版，Gemini 特例） | 「知道计量但无法拆分」的残差 | 反推 `reasoning`（仅 Gemini 模型 + 缺 thoughts + 残差为正），标 `estimated`；另一类差额进 `unpriced_input_tokens` | 是（被标精度；不可定价部分**只进 total 不进可定价列**） | `trae-usage.js:49`–`56`、`88`–`91`、`94` |
| **反例** `cursor` | CSV 的 `Cost` 列 | 只记 `record.cost`，**从不写 `total_cost_usd`**、不参与 token | 否 | `cursor-config.js:498`、`512`–`538` |
| **反例** `command-code` | 上游 `costUsd` | **主动丢弃**并写零成本哨兵 `total_cost_usd: 0`；注释说明那是「显示价」不是账单价 | 否 | `24055`–`24058`、`24213` |
| **反例** `qoder`（旧库） | —（无 credits 概念） | 只有 token 字段 | 否 | `6148`–`6179` |

**净结论**：真正把**非 token 计费单位**（credits）当 token 的只有半家（`qoder` 新通道），且因为累加器白名单而**没有实际数值污染**；其余「估算」都是 token→token 的推算，风险集中在**精度披露**（antigravity 完全不标、kiro 无标记）而非单位混淆。

---

#### 5.5.6 精度标记（`usage_precision`）逐家覆盖

| 家 | 是否写 `usage_precision` | 取值 | 依据 |
|----|--------------------------|------|------|
| `grok` | 是 | 兜底路径 `"estimated"`；同桶混装时升为 `"mixed"` | `20255`、`20268`、`20286`–`20289`、`19964`–`19968` |
| `trae` | 是 | `"estimated"` 或缺省 `null` | `22144`–`22158`、`src/lib/trae-usage.js:94` |
| `qoder`（新 JSONL） | 是（显式写 `null`） | `null` | `6542` |
| `antigravity` | **否**（只在游标里记 `usageSource`） | — | `21898` |
| `kiro`（CLI 字符估算） | **否** | — | `8371`–`8387`、`9152`–`9159` |
| 其余各家 | 否（无估算路径，无需标注） | — | 写入点清单见下 |

`usage_precision` 的全部写入点集中在 `src/lib/rollout.js` 的 **16 行 / 15 个独立语句**：`3325`、`3341`、`3403`、`3422`、`6542`、`6648`、`6725`、`6731`、`6755`、`6760`、`20286`–`20287`（跨行一条赋值）、`22011`（`22009`–`22011` 数组字面量续行）、`22025`、`22113`、`22157`。清单里**没有 `antigravity` 与 `kiro`**——这是 §5.6.6 里「披露缺口」那一条的否定性证据。统计口径限 `src/lib/rollout.js`；不含 `src/lib/session-analytics.js`（5 行）、`src/lib/trae-usage.js`（1 行）、`dashboard/src/lib/sessions-api.ts`（1 行）与测试用例。

#### 5.5.7 字段名速查（19 家的日志字段原文）

给需要写解析器或写 fixture 的读者：这一列是**上游日志里的原始键名**，不是桶名。

| Provider | 日志字段（原文） | 备注 |
|----------|------------------|------|
| Gemini | `tokens.input` / `.cached` / `.output` / `.tool` / `.thoughts` / `.total` | `tool` 并入 output |
| OpenCode | `tokens.input` / `.output` / `.reasoning` / `.cache.read` / `.cache.write` | 嵌套 cache |
| OpenClaw | `message.usage.input` / `.cacheRead` / `.cacheWrite` / `.output`（`totalTokens` 仅预筛） | input 先减 cacheRead |
| Kiro | `tokens_prompt` / `tokens_generated` | 无缓存列 |
| Hermes | `input_tokens` / `output_tokens` / `cache_read_tokens` / `cache_write_tokens` / `reasoning_tokens` / `message_count` | 会话数 = 消息数差分 |
| Kiro CLI | SQLite：`user_prompt_length` / `response_size` / `reasoning_size`（字符 ÷4）；legacy 会话 `.json`：`input_token_count` / `output_token_count`（**真实值**，皆为 0 才回退估算）；v2：`messages.jsonl` 字符数 ÷4 | 另有 `request_start_timestamp_ms` / `model_id`；credits 只写 sidecar |
| Kimi | `token_usage.input_other` / `.output` / `.input_cache_read` / `.input_cache_creation` | 秒级时间戳 |
| Kimi Code | `usage.inputOther` / `.inputCacheRead` / `.inputCacheCreation` / `.output`（或 Anthropic / OpenAI 形状） | 毫秒时间戳 |
| CodeBuddy | `cachedMissTokens` / `input_tokens` / `prompt_tokens`；`output_tokens`；`cache_read_input_tokens` / `cached_tokens` | 多镜像优先级 |
| WorkBuddy | `rawUsage.prompt_tokens` / `.completion_tokens` / `.cache_read_input_tokens` / `.cache_creation_input_tokens` / `completion_tokens_details.reasoning_tokens` | prompt 减双向缓存 |
| Roo Code / Kilo Code | `payload.tokensIn` / `.tokensOut` / `.cacheReads` / `.cacheWrites` | 同形状，source 不同 |
| Cline | `metrics.inputTokens` / `.outputTokens` / `.cacheReadTokens` / `.cacheWriteTokens` / `.reasoningTokenCount` / `.cost` | 唯一产 cost |
| Zed | `input_tokens` / `output_tokens` / `cache_read_input_tokens` / `cache_creation_input_tokens` | 线程四元组 |
| LM Studio | `prompt_tokens` / `completion_tokens` / `total_tokens` / `*_details.cached_tokens` … | 字节扫描 |
| Unsloth | `prompt_tokens` / `completion_tokens` / `total_tokens` / `cached_tokens` / `cache_write_tokens` / `reasoning_tokens` | 共用归一化器 |
| AnythingLLM | `prompt_tokens` / `completion_tokens` / `total_tokens` | 差额判为 reasoning |
| Goose | `accumulated_total_tokens`（回退 `total_tokens`）/ `model_config_json` | 无缓存列 |
| Droid | `tokenUsage.inputTokens` / `.outputTokens` / `.cacheCreationTokens` / `.cacheReadTokens` / `.thinkingTokens` / `.totalTokens` | total 兜底 |
| Claude Science | `input_tokens` / `cache_read_tokens` / `cache_write_tokens` / `output_tokens`（各含 `aux_*` 辅助组） | input 为 cache-inclusive，需减去两组缓存 |

### 5.6 风险清单（对下游最有价值）

#### 5.6.1 「只有 total 没有分桶 → $0 成本」：逐家判定

**计价侧前提（先读这条）**：`src/lib/pricing/index.js:248`–`256` 只把 `codex` / `acode` / `every-code` / `omo` / `cline` **五家**的 `reasoning_output_tokens` 视为 output 的子集（`reasoningIncludedInOutput`），集合外 source 的推理**按 output 单价单独计费**。下文所有「非 $0」判定都以此为前提。

| 类别 | 家 | 判定 | 依据 |
|------|----|------|------|
| **A. 可从源码推出 $0 退化路径（唯一一家）** | `gemini` | `total = max(上报值, 分桶和)`（`4743`）+ 闸门把 `total_tokens` 当非零（`4945`、`2971`）→ 若日志只写 `tokens.total`，产出 input/output/cache 全 0 的队列行；此时 `thoughts` 同为 0，故该行**确实为 $0**。**唯一性依据**：19 家的 `total_tokens` 赋值点独立比对后，只有 Gemini 采用 `max(上报, 计算)` 且前置闸门把 total 算作非零 | `4734`、`4743`、`2971`、`4945` |
| **B. 弱化版（2 家）**：可产出 `input=0/output=0/reasoning>0`；因不在上述五家集合内，**不为 $0** | `goose` | `reasoningDelta = max(0, dTotal − dInput − dOutput)`（`14711`），`dTotal > 0` 即落桶，continue 条件为三项皆 0（`14686`） | `14711`、`14686` |
| ↑ 同 B 类 | `kiro`（CLI 会话支路） | 会话支路零值闸门只检 4 列、**不含 reasoning**（`9414`），随后 delta 含 `total_tokens: … + reasoning`（`9446`）→ 同形；SQLite 支路有 `approx* > 0` 闸门（`9209`），不产该形态 | `9414`、`9446`、`9209` |
| **C. 明确规避（3 家）** | `droid` | `applyDroidTotalFallback`（`14972`）把 total 缺口填进 `output` 或 `thinking`，结构性规避 | `14972`–`14979` |
| ↑ 同 C 类 | `lmstudio`、`unsloth` | `input_tokens = max(0, total − completion − cacheRead − cacheWrite)`（共用归一化器 `13040`） | `12994`、`13039`–`13049` |
| **D. 不产 $0 行，但把纯 total 行静默丢弃（10 家）** | `kimi`(9590)、`kimi`(KimiCode, 9823)、`workbuddy`(10968)、`openclaw`(2109)、`roocode`(11910)、`kilo-code`(15628)、`cline`(12381)、`anythingllm`(14016)、`kiro`(IDE, 7547)、`codebuddy`（扩展日志闸门 `10345`；JSONL 支路五桶零闸门 `10476`–`10485`） | 队列行不会被写出，代价是**丢失纯 total 的用量** | 括号内行号 |
| **E. 结构上不可能出现「六列全零 + total>0」（3 家）** | `zed`（`12598`、`12752`）、`hermes`（SQL `WHERE`，`7718`）、`opencode`（`3109`） | 限定：仅指上述六列全零形态；**Zed 在只有缓存列增长时仍会落出 `input=0/output=0/total>0` 的行**（`totalDelta` 只由 4 桶求和 `12751`、`<= 0` 跳过 `12752`），Cline 同理（`12403` 的 `total_tokens: deltaTotal`）——下游不得把 E 类读成无条件安全 | 括号内行号 |
| **F. 未判定** | — | **0 家**（原唯一存疑的 Codebuddy 两条支路都有零值闸门，已并入 D 类） | `10345`、`10476`–`10485` |

**数据库 / API 子集的对照结论**：这一子集里**没有任何一家会「只填 total 而不填 input/output」**——所有 normalizer 都是「input 与 output 至少一个被赋值」或「整体判空丢弃」。18 家的判空条件（逐家源码行）覆盖 `opencode`(4755/5222)、`qoder`(6154/6526)、`cursor`(7230)、`devin`(`devin-usage.js:76`/`113`)、`omp`/`omo`(15874)、pi 家族(16455)、`craft`(17199)、`reasonix`(17067)、`copilot` 三通道(17536/18863/19369)、`grok`(`grok-usage.js:71`)、`antigravity`(21808/21833/21837)、`trae`(`trae-usage.js:60`)、`trae-cn`(22477/22749)、`dsh`(23354)、`command-code`(24204)。

#### 5.6.2 「全零 token 但仍落桶」：两处，且只有两处

| 家 | 构造点 | 说明 |
|----|--------|------|
| `qoder`（新 JSONL） | `6639`–`6660` | `billable !== false` 时显式构造 `input/cached/cache_creation/output/reasoning/total = 0`、`billable_total_tokens: 0`、`total_cost_usd: 0`、**`conversation_count: 1`** 的 totals 并 `addTotals` 进桶；注释直说这是「允许可计费的零 token 消息仍然计一次会话」 |
| `trae-cn` | `22483`–`22492`（校验）、`22500`–`22508`（写全零 totals）、`22749`–`22751`（落桶） | 字段校验只要求 `Number.isSafeInteger(value) && value >= 0`——**`0` 合法**；落桶处**无任何零值护栏**，只跳过「未变化」的条目 |

**反例（易误报为第三处）**：`craft` **不会**产生该形态——`17199`–`17212` 在四分量 delta 全 0 时直接 `continue`，走到 `17243`（`conversation_count: prev.total === 0 ? 1 : 0`）时至少一个 token 分量 > 0。

影响：该桶的 `conversation_count` 会增加，但所有 token 列为 0 → 该会话行的成本贡献为 $0。该行是否最终入队还取决于 `enqueueTouchedBuckets` 的 `queuedKey` 比较（`3325`–`3347`），但**桶 totals 确实被改动了**。

`【未能确认】` **trae-cn 真实响应里是否真会出现 `input_token=0` 且 `output_token=0` 的会话行**：源码层「允许」已确认，真实频率未知。**判定条件**：取得一次真实 `query_user_usage_group_by_session` 响应样本（含全部 token 字段为 0 的会话行），或用同形 fixture 跑完整 `parseTraeCnApiIncremental` 并检查队列是否出现全零 `total_tokens` 且 `conversation_count = 1` 的桶行。

#### 5.6.3 缓存字段（`cache_read` / `cache_write`）落空的分布

| 形态 | 家 | 说明与行号 |
|------|----|------------|
| **两个缓存桶都恒 0** | `kiro`(IDE)、`goose`、`anythingllm` | Kiro 连 `cache_creation_input_tokens` 键都缺失（`7545`）；Goose 无缓存列（`14670`/`14708`）；AnythingLLM 的 metrics 只有 prompt/completion/total（`14013`/`14040`） |
| **有 cache_read、无 cache_write（恒 0）** | `gemini`（`4743` 区域，`cache_creation` 恒 0）、`qoder`（旧库，`6177`–`6178` 写 `cache_creation_input_tokens: 0`）、`copilot`（App DB，注释明确「App DB 没有 cache write 列」，`19357`–`19379`）、`antigravity`（两条路径都没有 cache write 来源，走 `initTotals` 默认 0） | 缓存写入量在这些家里**不可得**，不是零 |
| **表级投影写死** | `unsloth` | `api_usage_events` 的 SELECT 直接写 `0 AS cached_tokens, 0 AS cache_write_tokens, 0 AS reasoning_tokens`（`13634`–`13636`）——即便上游有列也不读 |
| **reasoning 恒 0（无推理桶）** | `kimi`、`kimi`(KimiCode)、`openclaw`、`zed`、`roocode`、`kilo-code`、`kiro`(IDE)、`qoder`（新旧两套）、`cursor`、`devin`、`trae-cn`、`craft`、`command-code`、`claude-science` | 列在 `normalize*` 的返回块里写死 0。`devin` 更极端：**SQL 投影只选四个 metrics 字段**（`src/lib/devin-usage.js:48`–`51`），未来若上游新增 reasoning 列会被静默丢弃 |
| **reasoning 是「差额」而非推理量** | `anythingllm`（`max(0, total − input − output)`，`14040`）、`goose`（`max(0, dTotal − dInput − dOutput)`，`14711`） | 语义不保证，跨家不可比 |
| **上游把 reasoning 并进 output（求和须排除）** | `omo`（`reasoningIncludedInOutput: true`，`16110`；求和在 `15897`–`15915`）、`cline`（`outputTokens` 含 reasoning，注释 `12356`） | 若下游定价表只认 output，会低估或高估 |

**通用判据**：`billable_total_tokens` 只影响费用口径，缓存列落空影响的是**缓存命中率与缓存单价**；对本地推理两家（`lmstudio`/`unsloth`），缓存是真的没有；对 `gemini`/`qoder`/`copilot App`/`antigravity`，缓存**存在但不可得**——下游不能把「0」当作「零缓存」。

#### 5.6.4 `billable_total_tokens` 覆盖情况（8 显式 + 13 兜底）

**显式写入的 8 家**（全部在数据库 / API 子集）：

| 家 | 值 | 行号 |
|----|----|------|
| `qoder`（旧 DB） | `prompt + output`（含缓存） | `6177` |
| `qoder`（新 JSONL） | `inp + cach + cc + out`（零 token 分支写 0） | `6540`、`6657` |
| `devin` | 四分量之和 | `devin-usage.js:197` |
| `cursor` | 四分量之和（注释解释为何不能写 0：会把非计费用量整体隐藏） | `cursor-config.js:536` |
| `grok` | `totalTokens`（含估算路径） | `grok-usage.js:83`、`rollout.js:19959` |
| `antigravity` | `delta.total_tokens`（两条路径都写） | `21806`、`21831` |
| `trae-cn` | `rawInput + output` | `22507` |
| `command-code` | 四分量之和 | `24212` |

**不写、依赖 `addTotals` 兜底的 13 项**（按解析项计）：`opencode`、`omp`、`omo`、`pi`、`prime-agent`、`minimax-code`、`craft`、`reasonix`、`copilot`（OTEL / session-store / App DB 三条通道）、`trae`、`dsh`。合计 **8 + 13 = 21 项**，与 21 家数据源实体完全对齐（`qoder-cn` 复用 `qoder` 的写入路径，不额外计）。兜底规则：`target.billable_total_tokens += delta.billable_total_tokens ?? delta.total_tokens ?? 0`（`4110`），聚合侧同样兜底（`src/lib/local-api.js:2615`、`2628`）。

**文本 / JSONL 子集的覆盖**：19 家里能确认**显式写入**的只有 `lmstudio` 与 `unsloth`（共用归一化器返回块 `13039`–`13049` 含 `billable_total_tokens: total`）；其余家在各自函数区段内没有 `billable_total_tokens` 赋值语句，一律靠 `4110` 兜底。

**历史坑（必读）**：早期 Cursor 行把 `billable_total_tokens` 写成 0，导致聚合侧「把非计费 Cursor 用量整体隐藏」，因此有一次性的局部修复与重算（`src/lib/local-api.js:179`–`182` 注释）。**结论：这个字段是「可计费口径」的开关，任何「写 0 而不是留空」的实现都会污染聚合结果。**

#### 5.6.5 `conversation_count` 语义不可比（各家口径清单）

同一列名称在 45 家里至少有 **7 种语义**，dashboard 若直接当「会话数」展示会同时高估与低估：

| 语义 | 家 | 行号 |
|------|----|------|
| 产生差分的消息条数 | `gemini`、`opencode`（磁盘支路） | `2974`、`12405` 区域 |
| 每条 assistant 消息 +1（**请求级**） | `kimi` 一族、`codebuddy`、`workbuddy`、`roocode`、`kilo-code`、`opencode`(DB 仅首次)、`omp`/`omo`、pi 家族、`dsh`、`antigravity`、`command-code`、`cursor`（每条 CSV 记录） | `15914`、`17586`、`21807`、`23364`、`24214`、`7232` |
| 仅首次（首见该记录记 1，之后 0） | `cline`（`previous ? 0 : 1`）、`trae`（会话首次）、`trae-cn`（恒 1）、`copilot` session-store（按 session）、`devin`（按 session） | `12405`、`22115`–`22118`、`22506`、`18886`、`14340`–`14346` |
| 仅首次会话或重置 | `droid` | `15379` |
| 恒 1 | `goose`、`lmstudio`、`unsloth`（共用归一化器写死）、`craft`（会话级之外的历史路径） | `13039`–`13049` |
| DB `message_count` 差分 | `hermes` | `8008`、`7718` |
| **用请求数冒充会话数** | `reasonix`（`conversation_count: delta.requests`） | `17004`、`17044` |
| 会话级（首见记 1，源码明确警告不可比） | `craft`（`17232`–`17235` 注释：与 omp/Claude 的「每条消息 +1」不同，跨 provider 的会话总数**不可直接相加**） | `17232`–`17244` |

**下游处置建议**（依据源码事实，不是猜测）：按 `source` 分组展示、把该列标注为「计数单位随来源而变」，或在 TokenScope 侧**不要**用它做跨工具汇总。

#### 5.6.6 其余需要登记的破坏面

- **精度披露不完整**：`antigravity` 完全不在队列行写 `usage_precision`（只在游标里记 `usageSource`，`21898`）；`kiro` 的字符估算路径也没有精度标记。相对地 `grok` / `trae` 会写 `"estimated"`/`"mixed"`（`20255`、`22144`–`22158`）。`usage_precision` 的全部写入点集中在 `src/lib/rollout.js` 的 16 行 / 15 个独立语句（`3325/3341/3403/3422/6542/6648/6725/6731/6755/6760/20286-20287/22011/22025/22113/22157`），清单里没有 `antigravity` 与 `kiro`。
- **时间兜底最激进的一家是 `grok`**：桶起点依次为 `event.timestamp` → `lastActive` → `Date.now()`（`20214`），且 `lastActive` 自身也会兜底到 `new Date().toISOString()`（`19684`–`19693`）——**历史用量可能被记进「当前」半小时桶**。同类（用 now）还有 `zed`（`12764` 末级兜底）与 `goose`（`14713`）。
- **`droid` 用文件 mtime 定桶**（`15210`、`15328`）：任何触碰文件的操作都会把该会话的增量搬到触碰时刻。
- **累计快照 clamp 差值的重放风险**：`craft`（`17189`–`17195`）与 `reasonix`（`17008`–`17014`）都把负差 clamp 到 0 而不做负向回撤；若上游先重置又快速累积回原量级，**这段「重放」会被再次计入**。两家都没有 Qoder 式「先减旧再加新」的回撤路径。
- **`copilot` 三通道「同时活跃时互斥完备」未被证明**：`【未能确认】`。判定条件：给出 `test/copilot-app-parser.test.js` 与 `test/copilot-session-store-parser.test.js` 中交叉通道用例（含「同时活跃」场景）的断言清单；缺该清单则维持「未确认」。
- **成本口径一列三种处理方式**：`command-code` 明确丢弃上游成本写 0 哨兵（`24055`–`24058`）、`qoder` 声明成本永不估算（`6405`–`6411`）、`grok` 反之直接采信上游 ticks（`grok-usage.js:26`–`39`）——下游聚合必须按 `source` 区分，不能把 `total_cost_usd` 当同质列。
- **`config.update` 类旁路字段的可用性**：`kimi-code` 的模型别名、`antigravity` 的模型 slug 归一（`21929`–`21949`）、`trae` 的显示名到 API id 修正（`claude-4-sonnet` → `claude-sonnet-4-20250514`，`trae-usage.js:98`–`105`）都说明**模型名在多数家里是「被上游改写过的展示名」**，不是稳定的计价键——下游必须套等价规则归一后再匹配价格。

#### 5.6.7 给下游（TokenScope 侧）的检查清单（按优先级）

1. **不要把 `conversation_count` 当可加指标**：45 家里至少 7 种语义（§5.6.5），跨来源汇总必然同时高估与低估；需要「会话数」时按 `source` 分别定义，或自行从请求明细派生。
2. **`total_tokens > 0` 不等于「有分桶」**：Gemini 是唯一可产 $0 行的来源（§5.6.1 A 类），另有两处「全零 token 仍落桶」（§5.6.2）。解析侧应把「非零桶全为 0 而 total > 0」显式判为异常行并计数——TokenTracker 自己只做丢弃/放行，**不做标记**。
3. **缓存列 0 有两种含义（真无缓存 vs 存在但不可得）**：`gemini`、`qoder`(旧)、`copilot`(App DB)、`antigravity` 属后者（§5.6.3）。若用「缓存命中率」做展示，须按来源区分，否则会给出「该模型从不缓存」的错误结论。
4. **`billable_total_tokens` 必须走兜底而不是写 0**：`?? total_tokens` 兜底（`4110`）与 Cursor 的历史修复（`src/lib/local-api.js:179`–`182`）都指向同一结论（§5.6.4）。
5. **模型名是展示名，不是计价键**：`kilo-code` 只报 `provider:<slug>`（`11730`）、`kimi` 一族来自配置文件（`9540`）、`trae` 与 `antigravity` 会做别名改写（`trae-usage.js:98`–`105`、`rollout.js:21929`–`21949`）——必须先做等价归一（小写 + 去 `-` / `.` / `_`）再匹配价格。
6. **时间归属有五家会漂到「触碰时刻」**：`zed`、`lmstudio`、`goose`、`droid`、`grok`（§5.2.20、§5.3.6）。做日粒度看板时，这几家的历史数据可能整段落在错误日期上。
7. **估算行必须可识别**：`grok` 与 `trae` 会标精度，`antigravity` 与 `kiro` **不标**（§5.5.6）。若需要「只统计真实值」的视图，这两家只能按来源整体排除或整体保留。
8. **成本列不可同质处理**：`cline` 自产 `cost`、`grok` 采信上游 ticks、`command-code` 主动丢弃上游成本、`qoder` 明确拒绝估算（§5.6.6）——导入后必须按来源决定「用上游值还是用自己的价格表」。
9. **重复计数防线要照抄的三样**：fork / 副本去重（`opencode` #426 指纹、`devin` `request_id` 账本、`trae` turn 摘要、`command-code` `recordId|timestamp`）、镜像历史过滤（`mimo` 的 providerID 白名单、`zcode` 的三厂商黑名单）、跨通道认领（`copilot` 三通道）（§5.3.3、§5.3.4）。
10. **只读与隐私纪律**：这 45 家的 SQLite 读取统一走 `-readonly` / `readOnly: true`、且**没有显式 `BEGIN`**；`reasonix`、`copilot`(store/app)、`trae`、`devin` 都刻意排除了正文列（`16907`–`16908`、`19270`–`19271`、`trae-db.js:71`–`84`、`devin-usage.js:11`–`13`）。若要复用这些读取器，应保持同一投影纪律，不要为了「顺手多取一列」而把正文带进本地库。

---

**本章未闭合项（仍需外部证据）**：`trae-cn` 的真实全零行频率（§5.6.2，判定条件已写）、`copilot` 三通道互斥完备性（§5.6.6，判定条件已写）、`antigravity` 估算行是否真的对下游不可见（本章依据「`usage_precision` 全部写入点清单里没有 antigravity」这一**否定性证据**成立；若云端 ingest 侧能从 `cursors.files[].usageSource` 反推，该披露缺口不成立——判定条件：取得云端 ingest 侧读取该游标字段的代码或运行证据）。
