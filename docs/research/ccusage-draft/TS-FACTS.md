# TokenScope 对照事实清单

范围与方法：本清单只记录在 `C:\Users\admin\Desktop\项目\tokenscope` 仓库源码中**实际读到**的代码事实，供「ccusage 与 TokenScope 对照」章节引用。每条给出 `文件:行号`（行号为该文件内的起始行）与一句客观描述；未读出结论的项集中在最后一节。未做任何文件修改、未运行 cargo。仓库当前版本 `0.3.0`（`Cargo.toml:2`）。

## A. 读取链

### A.1 Claude Code 适配器

- 事实 1：默认采集根是 `~/.claude/projects`；`--features acceptance` 且给了隔离根时改用隔离根下的 `projects/`，不回退真实目录。
  `证据：src/source/claude.rs:97-105`
- 事实 2：默认账户配置路径是 `~/.claude.json`（验收模式走隔离根下 `claude.json`），仅用于项目 slug → 真实路径的身份映射。
  `证据：src/source/claude.rs:107-115`
- 事实 3：文件发现 = 递归遍历根下全部 `*.jsonl`（按扩展名大小写不敏感判断），结果按路径排序；子目录不可读记入发现异常列表而不中断。
  `证据：src/source/mod.rs:60-63, 159-176`、`src/source/claude.rs:165-175`
- 事实 4：读文件是**整份读入内存**（`std::fs::read` + `String::from_utf8_lossy` 容错），再 `text.lines()` 逐行处理——没有流式逐行读盘。
  `证据：src/source/mod.rs:178-181`、`src/source/claude.rs:204-218`
- 事实 5：无「读取前关键字预过滤」这类优化；每行都先做 `serde_json` 结构体反序列化，再按 `type` 分派。
  `证据：src/source/claude.rs:245-251, 279-281`
- 事实 6：只用 `type == "assistant"` 行的 `message.usage`；非 assistant 行仍参与 cwd 身份采集（不进用量）。
  `证据：src/source/claude.rs:252-281`
- 事实 7：坏行容错为「计数 + 跳过单行」：JSON 解析失败、缺 `message`、缺 `usage`、缺/非法 `timestamp`、空 `message.id`、空模型名各计一次 `bad_lines` 并 `return`，不影响同文件其余行。
  `证据：src/source/claude.rs:245-251, 282-317`
- 事实 8：`<synthetic>` 模型行跳过并计 `skipped_synthetic`（比较前 `trim`）；子代理（`isSidechain`）用量**计入**，但其 cwd 不推进主链项目根。
  `证据：src/source/claude.rs:286-291, 320-345`
- 事实 9：事件字段为 `ts / agent / model / session_id / project / session_initial_cwd / event_cwd / record_id( = message.id ) / line / source_path / 四桶 token`；`record_id` 供全局去重使用。
  `证据：src/source/claude.rs:351-366`、`src/model.rs:38-65`
- 事实 10：四桶 token 在 source→model 边界做受检校验（总数与 prompt 基数溢出即计坏行跳过，不回绕不饱和）。
  `证据：src/source/claude.rs:367-373`、`src/model.rs:72-92`
- 事实 11：Claude 项目映射（`~/.claude.json` 的 `projects`）用**正向编码**生成 slug 候选，不做反向解码、不做前缀猜测；同 slug 多路径冲突则该 slug 保持独立身份。
  `证据：src/source/claude_projects.rs:1-14, 86-110`
- 事实 12：slug 编码规则 = 每个非 ASCII 字母数字字符替换为一个 `-`。
  `证据：src/source/claude_projects.rs:181`
- 事实 13：映射按 `(size, mtime)` 指纹进程内缓存；读取失败（Unusable）结果**不进缓存**。
  `证据：src/source/claude_projects.rs:122-146`、`src/source/claude_projects.rs:201`
- 事实 14：映射状态与内容摘要构成确定性 revision（`context_revision`），变化即使文件缓存与查询复用键失效。
  `证据：src/source/claude.rs:160-163`、`src/source/claude_projects.rs:157-176`

### A.2 Codex 适配器

- 事实 15：默认采集两个根：`$CODEX_HOME/sessions` 与 `$CODEX_HOME/archived_sessions`（`CODEX_HOME` 缺失或空值回退 `~/.codex`）；显式自定义根只在选定目录内递归。
  `证据：src/source/codex.rs:58-78, 44-56`
- 事实 16：发现阶段排除 `history.jsonl`（Codex CLI 的提示词历史，含非用量内容），只把其余 jsonl 当 rollout 候选；路径排序后去重。
  `证据：src/source/codex.rs:194-200, 219-242`
- 事实 17：读文件同样整份读 + 逐行 `lines()`；每行先反序列化为宽松结构再按顶层 `type` 分派（`session_meta` / `turn_context` / `token_usage_record` / `event_msg`+`token_count`）。
  `证据：src/source/codex.rs:259-273, 275-328`
- 事实 18：只取 `event_msg`/`token_count` 的 `info.last_token_usage`；`token_usage_record`（累计回显）一律忽略并计 `ignored_token_usage_record`（双计防线）。
  `证据：src/source/codex.rs:4-6, 314-325`
- 事实 19：累计型语义处理 = `last_token_usage.input_tokens` 是总量桶，`cached_input_tokens` 与 `cache_write_input_tokens` 为其子集；归一化 `input = raw - cached - cache_write`、`cache_read = cached`、`cache_write = cache_write`，展示总量守恒 `input+output+cw+cr = raw total`。
  `证据：src/source/codex.rs:7-13, 417-432`
- 事实 20：`cached + cache_write > input` 或 `total != input + output` 视为不可解释组合 → 计坏行跳过；零分量占位行（只升 total）跳过并计 `skipped_zero_usage`。
  `证据：src/source/codex.rs:14-16, 375-397`
- 事实 21：模型按时间序最近的 `turn_context` 归属，遇 `session_meta` 重置（新会话不继承旧会话的模型、项目根与初始目录）；无有效 cwd 时项目记 `(未知)`。
  `证据：src/source/codex.rs:17-21, 293-313, 406-414`
- 事实 22：Codex 事件 `record_id` 为空串（rollout 的 `token_count` 不带请求标识），即该来源没有可证实的跨来源原生身份。
  `证据：src/source/codex.rs:24-28, 425`、`src/model.rs:139-153`

### A.3 剪枝与并发

- 事实 23：**没有**「按文件 mtime/since 剪枝」这一类优化：发现阶段枚举根下全部 jsonl，没有时间窗过滤。
  `证据：src/source/mod.rs:159-176`、`src/source/claude.rs:165-175`、`src/source/codex.rs:219-242`
- 事实 24：存在的优化是**文件级指纹缓存**：`(size, mtime_ms)` 与来源上下文（agent + 规范化根集合 + 解析规则修订）一致时本轮不重解析，仅沿用已登记统计。
  `证据：src/report.rs:1262-1284`、`src/cache.rs:5, 67-78`
- 事实 25：解析前后各取一次指纹做复核，采集期间仍在变化的文件本轮不登记成功指纹（避免旧内容配新指纹后永久陈旧命中）；读取失败的文件同样不登记。
  `证据：src/report.rs:1285-1323`
- 事实 26：采集为**串行**：逐个来源、逐文件顺序处理，来源之间没有线程/任务并行；同一来源内也不并行。
  `证据：src/report.rs:1219-1222, 1262-1267`
- 事实 27：跨入口存在「统一串行闸门」与「单飞」协调：不同参数的采集排队执行，同参数的并发请求合并复用同一次采集快照。
  `证据：src/report.rs:533, 643-660, 860`

### A.4 项目路径归一化

- 事实 28：归一化是纯函数，不做文件系统访问、不读环境变量；不可安全解释的输入返回 `None`，由调用方走兜底（绝不相对 TokenScope 的 cwd 补全）。
  `证据：src/source/project_path.rs:19-22`
- 事实 29：Windows 绝对路径规则＝盘符大写 + 反斜杠统一为 `/` + 去尾随分隔符，不做整体小写；根语义保留（`C:/`、POSIX `/`、UNC 主机与共享）。
  `证据：src/source/project_path.rs:7-9, 191-214`
- 事实 30：`file://` 走 URI 解析器并只做**一次**百分号解码；普通路径里的 `%20` 是字面值；`.`/`..` 组件与解码后含 NUL 一律拒绝（解析器会折叠点段，故检查在解析之前）。
  `证据：src/source/project_path.rs:10-12, 69-119, 140-147`
- 事实 31：主机名大小写不敏感（UNC 与 URI 同口径小写）；不做整体 trim（尾空格是合法文件名字符）；编码反斜杠 `%5C` 是字面字符。
  `证据：src/source/project_path.rs:13-14, 26-31, 231-253`
- 事实 32：相对路径、`C:foo`、非 file scheme、非法百分号编码、`.`/`..` 组件判为「不可用」；不折叠符号链接或别名。
  `证据：src/source/project_path.rs:16-17, 33-51, 255-269`
- 事实 33：会话内项目归属用 `ProjectRootTracker` 状态机：进入当前根的子目录仍归该根，越出当前根即视为新项目；判定按分量边界前缀（`C:/testx` 不算 `C:/test` 的子目录），Windows 大小写不折叠。
  `证据：src/source/project_path.rs:271-326`
- 事实 34：Claude 完全无可信 cwd 时回落文件身份（唯一正向映射命中的真实路径 > slug）；Codex 无有效 cwd 记 `(未知)`。
  `证据：src/source/claude.rs:117-148, 319-345`、`src/source/codex.rs:406-414`

## B. 身份与去重

- 事实 35：全局去重（`dedupe_events`）在 report 管线里对未去重事件统一执行，使「无缓存 / 缓存命中 / 强制刷新」三条路径数字一致。
  `证据：src/dedupe.rs:1-3`、`src/report.rs:1376-1388`
- 事实 36：Claude 去重键 = `(session_id, record_id)`；保留规则 = 时间戳最晚的一条，时间戳相同则行序靠后者胜。
  `证据：src/dedupe.rs:9, 26-42`
- 事实 37：Codex 去重键 = `(session_id, 原始模型, input, output, cache_write, cache_read)` 六元组；保留规则 = 保**首条**（同请求重发必然同模型，首条即请求完成时刻）。
  `证据：src/dedupe.rs:10-13, 44-58`
- 事实 38：去重键不含日期，跨本地日界的重播仍合并；同模型同用量的两个真实请求与重播不可区分，按保守规则合并（已在代码注释中明示为已知限制）。
  `证据：src/dedupe.rs:13-15`、`src/dedupe.rs:326-350, 373-377`
- 事实 39：去重丢弃数按 agent 分别统计（只列非零项），并按 Claude→Codex 顺序返回。
  `证据：src/dedupe.rs:18, 66-72`
- 事实 40：模型等价身份键构造 = 解析结构（最后一个 `/` 之前为 namespace，末段中 `:` 之后为 variant），再对各片段删除 ASCII `-` `.` `_` 并转小写；`/` 与 `:` 保留结构语义。
  `证据：src/model_identity.rs:4-11, 38-68, 12-22`
- 事实 41：末段等价键 `leaf_key` = 基名键（+ `:` 变体键）；`opus55:` 与 `opus55` 不等同（显式空变体保留）。
  `证据：src/model_identity.rs:50-57, 90-108`
- 事实 42：分组身份键 `identity_key` 保留 `/` 与 `:` 结构、压缩其余字符并折叠 ASCII 大小写（`nano-gpt/qwen/qwen3.8-27b:thinking` → `nanogpt/qwen/qwen3827b:thinking`）。
  `证据：src/model_identity.rs:125-138`
- 事实 43：基名压缩后为空（`-._`、`---`、`provider/---`）不是通配价格键，按未收录处理。
  `证据：src/model_identity.rs:110-113`、`src/pricing.rs:64-68, 2249-2251`
- 事实 44：记录基名中分隔符位置对应的压缩键偏移（`boundaries`），用于前缀回退只在词元边界截断；`opus550`、`gpt50` 无边界，不得命中更短条目。
  `证据：src/model_identity.rs:141-157, 224-247`
- 事实 45：只对 ASCII 符号与 ASCII 大小写生效，中文等多字节字符原样保留，边界按字符计数。
  `证据：src/model_identity.rs:10, 196-197, 242-247`

## C. 计价

### C.1 三来源加载与取舍

- 事实 46：三层来源＝外置 `~/.tokenscope/pricing.toml`（`TIER_EXTERNAL=0`）、models.dev（`TIER_MODELSDEV=1`）、OpenRouter（`TIER_OPENROUTER=2`）。
  `证据：src/pricing.rs:9-13, 26-28`
- 事实 47：加载顺序固定为 models.dev → OpenRouter → 外置，全部叠加在**空表**上；条目不是覆盖式替换，而是按归一化匹配键并存为独立候选，`tier` 保留来源身份。
  `证据：src/pricing.rs:1847-1858, 1884-2135, 1752-1757`
- 事实 48：来源文件缺失静默跳过该层；解析失败则警告并跳过该层；读取失败（存在但读不到 / 非法 UTF-8）与「不存在」分属不同健康状态，且本次结果**不写缓存**。
  `证据：src/pricing.rs:1507-1531, 1641-1650, 1673-1717`
- 事实 49：默认空价格表不含任何静态兜底（`Pricing::default()` = `empty()`）。
  `证据：src/pricing.rs:1737-1750`
- 事实 50：加载签名 `sig` 由「`rules:9` + 每来源路径/健康状态（成功为内容摘要）」构成；内容或规则版本变化即重建候选索引。
  `证据：src/pricing.rs:1652-1670, 1619-1626`
- 事实 51：外置条的非法数值拒绝整条候选并给出诊断；models.dev / OpenRouter 条目数值非法同样拒绝整条，分段**结构**非法只在数值合法的前提下降级为仅基础价。
  `证据：src/pricing.rs:1930-1960, 2044-2063, 1313`
- 事实 52：外置 `[[model_policy]]` 只把匹配候选的 `Unknown` cache_read 改写为 `same_as_input`，不覆盖已有数字/显式 0、不生成新候选，无命中给 warning。
  `证据：src/pricing.rs:2151-2213`

### C.2 匹配链与候选裁决

- 事实 53：匹配键取模型标识**末段**（最后一个 `/` 之后）的等价键——不同渠道同名模型汇入同一候选组，由估价裁决。
  `证据：src/pricing.rs:37-44`
- 事实 54：候选收集是四阶段分阶段回退，阶段之间不混匹配方式：① 完整等价匹配 → ② 完整匹配 + variant 回退 → ③ 边界前缀匹配（变体需一致）→ ④ 边界前缀 + variant 回退；任一阶段有结果即返回。
  `证据：src/pricing.rs:2236-2302`、`src/pricing.rs:826-838`
- 事实 55：前缀回退只枚举查询基名自身 `boundaries` 中的偏移（倒序），不在任意位置截断。
  `证据：src/pricing.rs:2273-2300`、`src/model_identity.rs:141-157`
- 事实 56：计费入口是 `estimate`：先按请求条件对**每个候选**计价，分「完整候选集 / 不完整候选集」；完整候选集非空时在其中取最高费用，否则回退到不完整候选中已知费用最高者（reason 分别为 `highest_complete_cost` / `highest_partial_cost`）。
  `证据：src/pricing.rs:2304-2313, 2334-2400`
- 事实 57：同费用并列的决胜序 = 来源优先级（外置 > models.dev > OpenRouter）> 完整匹配 > 前缀更长 > 原始键字典序。
  `证据：src/pricing.rs:542-549, 2401-2415`
- 事实 58：禁止跨候选拼价——最终四类单价全部来自同一选中候选。
  `证据：src/pricing.rs:2310, 2333`
- 事实 59：完整候选胜出但存在被排除的不完整候选时，结果携带结构化提示，明示「不是所有渠道的严格上界」。
  `证据：src/pricing.rs:2421-2432, 926-936`
- 事实 60：存在适用峰谷时间规则时，规则之间按「完整优先 → 费用降序 → 声明顺序（稳定）」择优；未命中且无显式声明的空规则整体跳过，不重引基础价。
  `证据：src/pricing.rs:2344-2384`
- 事实 61：`lookup`（不含请求条件的快速判定）同样按候选与 tie_rank 择一，注释声明其仅用于无请求上下文场景；计费入口是 `estimate`。
  `证据：src/pricing.rs:2215-2234`

### C.3 缓存价、分段与三态

- 事实 62：单价载体 `PriceRates` 只有四个字段：`input / output / cache_write / cache_read`，缓存写只有一个桶。
  `证据：src/pricing.rs:200-210`
- 事实 63：Claude 适配器只读 `cache_creation_input_tokens` 单一字段映射到 `cache_write_tokens`——源码中不存在 5 分钟 / 1 小时缓存写的分桶或倍率。
  `证据：src/source/claude.rs:58-68, 364`
- 事实 64：上下文分段数据结构为 `[min_tokens, max_tokens)` 左闭右开、`max_tokens=None` 为无上限，段内缺项沿用基础价。
  `证据：src/pricing.rs:250-260`
- 事实 65：分段选择是「唯一命中段」，不做最近档位猜测；档位依据值 = prompt token（`input + cache_write + cache_read`），输出不参与档位选择。
  `证据：src/pricing.rs:485-495, 2324-2326`、`src/model.rs:208-212`
- 事实 66：单价解析层次固定为「分段显式值 > 时间规则值 > 基础值」逐字段回退；`Unknown` 才回退下一层，`Fixed(0)` 与 `SameAsInput` 都是显式声明。
  `证据：src/pricing.rs:497-531`
- 事实 67：models.dev 快照的分段在加载时转换为规范 `[min, max)`；OpenRouter `overrides` 按 inclusive 下界转换为 `[min, next_min)`，并把 USD/token ×1e6 统一为 USD/百万 token；外置 `[[model.segment]]` 直接构造分段。
  `证据：src/pricing.rs:1899-1929, 1996-2021, 1125-1158`
- 事实 68：峰谷时间规则（`PriceSchedule`/`SchedulePeriod`）带自身时区、时段、星期限制与可选规则内分段，其价格作为分段之后的回退层。
  `证据：src/pricing.rs:262-308`
- 事实 69：三态单价在代码里的表示 = `RateSpec::{Unknown, Fixed(f64), SameAsInput}`；serde 线格式为 `null` / 数字 / `"same_as_input"`。
  `证据：src/pricing.rs:82-108`
- 事实 70：分项解析结果再用 `RateKind::{Fixed, SameAsInput, Unknown}` 标注，缺价分项 `priced = false`、小计 0 不计入总价。
  `证据：src/pricing.rs:654-663, 876-896`
- 事实 71：未收录模型通过 `estimate` 返回 `None` 表达（不是 0 费用）；部分缺价时 `complete = false`，缺价分项的 token 进 `unknown` 并要求按「未知」呈现。
  `证据：src/pricing.rs:2304-2323, 789-805`
- 事实 72：分项小计按「先除后乘」计算并做有限性检查；超 f64 表示范围时该分项不计金额、保留为未计价并标记 `overflow`，不夹成 0 或伪装 complete。
  `证据：src/pricing.rs:754-787, 891-895`
- 事实 73：不支持 `SameAsInput` 用于 cache_read 之外的分项（加载校验整体拒绝），运行期也防御性视为未知。
  `证据：src/pricing.rs:82-91, 471-483, 706-718`

### C.4 是否存在内嵌价格表

- 事实 74：模块注释明确「编译期内置表已移除，未收录模型始终 unknown，无任何静态 fallback」；价格表持久化形态注释为「Task 1 起无内置层」。
  `证据：src/pricing.rs:9-15, 1360`
- 事实 75：全仓 `builtin` 相关命中仅出现在测试（旧索引被判定失效的回归测试）与描述性注释，未发现任何生产代码中的价格常量表。
  `证据：src/pricing.rs:2990, 3827-3866`
- 事实 76：`PRICING_TEMPLATE` 是「外置文件不存在时创建模板」写入的文本（含注释与示例条目 `claude-opus-5`），不是参与计价的编译期表。
  `证据：src/pricing.rs:1317-1354`
- 事实 77：价格索引快照版本 `INDEX_VERSION = 9`（`rules:9`），版本不符的旧索引一律按来源离线重建。
  `证据：src/pricing.rs:1381-1395, 1652-1656`

## D. 聚合与查询

### D.1 聚合维度与排序

- 事实 78：分组维度四种：`day`（按统计时区的自然日）/ `model`（按等价身份键）/ `project`（项目根 key）/ `agent`（应用）。
  `证据：src/aggregate.rs:53-81, 240-256`
- 事实 79：分组容器是 `BTreeMap<String, GroupAccum>`，因此分组输出顺序 = key 的字典序升序（未再做显式排序）。
  `证据：src/aggregate.rs:204-217, 403-415`
- 事实 80：收口时把「合计」组 `push` 到 `groups` 末尾；合计逐字段受检累加，非有限费用直接报错。
  `证据：src/aggregate.rs:423-460`
- 事实 81：模型组的展示名 `label` 优先取可信价格表的显示名，否则退回该组原始模型名的最小字典序代表写法；`key` 只用于筛选与下钻。
  `证据：src/aggregate.rs:16-30, 266-274, 407-412`
- 事实 82：项目组展示名 = 路径末段；单应用数据不标注 `agents`（与早期输出保持一致）。
  `证据：src/aggregate.rs:391-402, 416-422`
- 事实 83：每条事件在聚合时调用一次 `Pricing::estimate`（带事件时间戳），费用累积非有限即报错；`unknown_pricing` 是组内累积标记，后续完整计价事件不能清掉它。
  `证据：src/aggregate.rs:287-322`
- 事实 84：只有日粒度的外部汇总（CCS 日汇总）按 `RollupContribution` 处理：四桶整体计入未知 token，原因标注为「历史数据只有汇总」，不伪造成请求事件，也不按当前档位重估。
  `证据：src/aggregate.rs:106-126, 329-332`
- 事实 85：桶覆盖诊断 `RollupCoverage` 记录明细覆盖桶数、改用日汇总桶数、未解决覆盖桶数与其 token、时区不匹配提示，以及时刻范围下 `rollups_excluded_for_instant_range` 提示位。
  `证据：src/aggregate.rs:128-153`

### D.2 查询会话与游标

- 事实 86：会话上限常量：`MAX_ACTIVE_QUERIES = 8`、`MAX_RETAINED_QUERY_BYTES = 256 MiB`（保留数据字节预算）、`QUERY_IDLE_TTL = 600s`。
  `证据：src/query.rs:13-27, 48-51`
- 事实 87：`begin_query` 冻结的对象包括：采集快照（事件集）、价格表与价格修订号、解析一次的 as_of/时区/时间边界、主查询指纹、固定的行序（只存行索引）、与历史库的只读事务快照、日汇总桶选择。
  `证据：src/query.rs:1-11, 72-102, 628-646`
- 事实 88：快照行序 = 主过滤后按 `(ts, record_id)` **降序**排序，同一 `(ts, record_id)` 组内计算 `seq`（0 起），排序与 seq 只算一次。
  `证据：src/query.rs:104-116, 564-582`
- 事实 89：查询身份 `query_id` = 进程启动时随机 128 位命名空间 + 进程内单调计数 + 历史库 generation；随机源失败或计数溢出一律明确报错，不回退。
  `证据：src/query.rs:29-33, 254-262, 586`
- 事实 90：主查询指纹由「采集键 + days/from/to/rolling_hours/start_at/end_at/until_now + 时区」构成；`collect` 不参与指纹（新鲜度由 generation 表达）。
  `证据：src/query.rs:227-247`
- 事实 91：`query_summary` / `query_events` 必须携带 query_id 从同一快照读取；会话不存在或已淘汰返回结构化错误 `query_expired`，不静默换页。
  `证据：src/query.rs:659-676, 713-721`
- 事实 92：游标版本 `CURSOR_VERSION = 2`，字段为 `v / qid / mfp / dfp / ts / rid / seq`（`deny_unknown_fields`），定位时逐项校验版本、会话、主指纹、下钻指纹，再按 `(ts, rid, seq)` 在快照行序中查位置。
  `证据：src/query.rs:68-70, 723-759`
- 事实 93：下钻指纹只含 `model / project / day`（不含 limit/cursor），模型筛选按**等价身份键**归一，使同一模型不同写法可在同一子查询内续用游标。
  `证据：src/query.rs:212-225`
- 事实 94：汇总与分页都按批从只读事务快照读取事件（`for_each_event` 批 4096、明细扫描批 256），不整份物化事件本体。
  `证据：src/query.rs:118-171`、`src/report.rs:2297-2301`
- 事实 95：明细过滤 = `model / project / day / limit(默认 200，上限 1000) / before(游标)`；`total` 为过滤后截断前总条数，同一快照内恒定。
  `证据：src/report.rs:2209-2221, 2316-2336`

### D.3 SQLite 用量历史库

- 事实 96：库路径 `~/.tokenscope/history.db`，定位是持久事实存储（不是可丢弃缓存）；来源文件删除、停用、重扫都不删除已存用量。
  `证据：src/history.rs:1-17`、`src/report.rs:203-210`
- 事实 97：当前结构版本 `HISTORY_SCHEMA_VERSION = 4`（`PRAGMA user_version`）；未知更高版本直接报错并保留文件，绝不删表重建。
  `证据：src/history.rs:19-20, 31-35`
- 事实 98：v1 表：`history_meta(key,value)`、`source_files(唯一键 app+root+path，含 fingerprint/size_bytes/mtime_ms/context_revision/state/各 skip 计数/parsed_events)`、`usage_events`。
  `证据：src/history.rs:58-86`
- 事实 99：`usage_events` 关键列：`id`、`event_key`（UNIQUE）、`app`、`ts_seconds`/`ts_nanos`、`model_raw`、`model_identity`、`identity_revision`、`session_id`、`record_id`、`project_key`、`session_initial_cwd`、`event_cwd`、四桶 token（`TEXT` 十进制文本）、`origin_rank`、`observed_at_*`、`parser_revision`。
  `证据：src/history.rs:87-110`
- 事实 100：四桶以文本存储的原因 = SQLite 有符号整数上限不得截断有效数值，也禁止隐式转 REAL 汇总。
  `证据：src/history.rs:8-10, 101-104`
- 事实 101：`usage_events` 四个时间索引：`(ts_seconds, ts_nanos, id)`、`(app, ts_seconds, ts_nanos, id)`、`(project_key, ...)`、`(model_identity, ...)`。
  `证据：src/history.rs:111-118`
- 事实 102：v2 表：`event_aliases(event_id, app, identity_scheme, identity_value, UNIQUE(app, identity_scheme, identity_value))`、`event_origins(event_id, origin_kind, origin_key, app, import_run_id, parser_revision, source_model_raw, pricing_model, source_cost_usd, ts_precision_seconds, UNIQUE(origin_kind, origin_key))`、`import_runs`、`ccs_daily_usage`。
  `证据：src/history.rs:121-193`
- 事实 103：`ccs_daily_usage` 单独保存只有日粒度的外部汇总（含 `day` / `source_tz` / `app` / `model` / `request_model` / `pricing_model` / 四桶 / `source_cost_usd` / `revision`），不与请求明细混表。
  `证据：src/history.rs:171-192`、`src/aggregate.rs:106-126`
- 事实 104：原生事件身份字段：Claude = `native_identity()` 返回 `("claude-message", "session_id|record_id")`；Codex = `native_identity()` 返回 `None`。
  `证据：src/model.rs:130-153`
- 事实 105：保守重播指纹 `same_source_identity()`：Claude 与原生身份相同；Codex = `("codex-usage", "session_id|原始模型|input|output|cache_write|cache_read")`，注释明示这是沿用去重规则的保守合并，不宣称强跨来源身份。
  `证据：src/model.rs:155-186`
- 事实 106：原生写入的来源键（`event_key`）= `native|{app}|{规范化路径}|{行序}|{内容指纹}`；内容指纹为 FNV-1a 64（跨进程稳定），同一行内容变化得到新来源键、旧事实保留。
  `证据：src/report.rs:2053-2063`、`src/model.rs:94-128`
- 事实 107：原生写入同时登记两类别名（可证实身份 + 保守重播身份），别名 UNIQUE 约束保证同一身份只绑定一个事件行。
  `证据：src/report.rs:2064-2082`、`src/history.rs:123-131`
- 事实 108：库内处置判定 `classify_against`：内容相等→升级 origin_rank 或 Unchanged；不同内容时优先级（原生 > CCS）先于观察时间决定 Update/Conflict；同优先级下观察时间更早判 Stale（保留较新值）。
  `证据：src/history.rs:2014-2042, 376-393`
- 事实 109：观察时间取**事件时间**而非采集时刻，使重新解析较旧副本不会回退已保存的较新终值。
  `证据：src/report.rs:2096-2099`、`src/history.rs:440-441`
- 事实 110：写库前按事件时间升序排序并逐条 `can_write` 预检，弃写条数进诊断（单条坏记录不让整批回滚）。
  `证据：src/report.rs:1390-1419`
- 事实 111：解析规则版本 `PARSER_REVISION = 1`、身份规则版本 `IDENTITY_REVISION = 1` 与事件一并存储（供重解析与派生键重建判定）。
  `证据：src/history.rs:37-41, 95, 109`

## E. 与 ccusage 的差异面（仅事实）

- 事实 112：统计时区的**唯一权威来源**是 `settings.toml` 的 `timezone`（`local` 或 IANA 名；None = 默认 `Asia/Shanghai`）；写入前校验，非法值拒绝落盘。
  `证据：src/settings.rs:69-75, 128-156`
- 事实 113：时区解析优先级 = 显式传入（GUI 下拉）> `local`（本机）> 默认 `Asia/Shanghai`；全链路只在此解析一次。
  `证据：src/aggregate.rs:31-51`
- 事实 114：时间范围是四组互斥参数：自然日预设 `days`、自然日区间 `from`/`to`、滚动窗口 `rolling_hours`（只接受 24）、自定义时刻 `start_at` + (`end_at` 或 `until_now`)；组冲突与缺项在解析时明确报错。
  `证据：src/report.rs:86-101, 1517-1522, 1593-1696`
- 事实 115：两种边界模式：自然日模式按统计时区日期闭区间且 `ts ≤ as_of`；时刻模式按 `[start, end_exclusive)` 且 `ts ≤ as_of`；「全部时间」不设范围也不截断。
  `证据：src/report.rs:1524-1560, 1761-1800`
- 事实 116：自定义整日对齐（00:00 起 + 23:59 或至今止）归入自然日模式；时刻模式下日汇总一律不参与，所涉自然日确有日汇总时回传 `rollups_excluded_for_instant_range` 提示键。
  `证据：src/report.rs:1700-1714`、`src/aggregate.rs:148-152`
- 事实 117：本地时刻 → 时刻换算统一用 jiff `compatible` 规则（夏令时不存在时刻往后取、重复时刻取较早）；`start_at` 晚于 as_of 明确报错。
  `证据：src/report.rs:1562-1585, 1715-1722`
- 事实 118：时间范围只解析一次并冻结在查询快照里；界面显示的范围取自后端冻结回传的 `range`（`RangeView`，两种粒度 `day` / `instant`），前端不自行计算边界。
  `证据：src/report.rs:1494-1515`、`src/query.rs:90-94, 173-187, 688-689`
- 事实 119：命令层把四组时间参数原样传给内核（分组互斥、取值校验、边界换算全在 report 层，命令层零业务逻辑）。
  `证据：src-tauri/src/commands.rs:116-155`
- 事实 120：**未实现** 5 小时 block（或任何「会话块 / 计费窗口」）概念：全 `src/` 无 block 相关类型、字段或计算；命中的 `block` 字样仅出现在无关测试与依赖名（`blocking`、`blocked` 文件名、`non_blocking` 日志）。
  `证据：全 src/**/*.rs 检索无 block 语义命中；仅 `src/fsutil.rs:119`（测试用 blocked 文件）、`src/logging.rs:16`（non_blocking 日志）、`src/settings.rs:196`（spawn_blocking 注释）出现无关同形词`
- 事实 121：唯一与「窗口」相关的概念是滚动 24 小时范围 `rolling_hours = 24`，其取值只接受 24。
  `证据：src/report.rs:90-92, 1653-1660`
- 事实 122：输出形态是 Tauri 2 + Vue 3 桌面 GUI（唯一产品形态）：后端以 `#[tauri::command]` 暴露 `summarize / list_events / query_begin / query_summary / query_events / pricing_* / source_* / settings_* / ccs_import_* / cache_stats / refresh_cache` 等命令。
  `证据：src-tauri/src/commands.rs:47-48, 187-188, 228-229, 241-242, 1005-1006, 1020-1021, 1132-1133`
- 事实 123：仓库源码树中**没有** `src/render/` 模块，也没有 CLI 入口（根 crate 无 `src/main.rs`、`Cargo.toml` 无 `[[bin]]`）；输出/契约层由 `src/report.rs` + `src-tauri` 命令层 + `frontend/` 承担。
  `证据：src/lib.rs:1-17`、`Cargo.toml:1-8`、`src-tauri/src/main.rs:1-5`
- 事实 124：前端只有展示与排版，不重算金额（费用明细 tooltip 与分段价格展示均标注「只排版、不重算」）。
  `证据：frontend/src/lib/costBreakdown.ts:1-3`、`frontend/src/lib/tieredPrice.ts:1-2`
- 事实 125：汇总 JSON 契约 `SummaryReport` 字段为 snake_case：`query_id / pricing_revision / timezone / by / groups / totals / sources / warnings / rollup_coverage / generated_at / range`；另有桌面形态设置项 `close_action`（关窗三态）、`price_auto_sync`（默认开，每 24h）、`collect_interval_secs`（5/10/30/60/300/600 秒，默认 300）。
  `证据：src/report.rs:139-158`、`src/settings.rs:54-89`
- 事实 126：会话句柄 `QueryHandle` 用 `rename_all = "camelCase"` 输出（`queryId / generation / pricingRevision / timezone / asOf / range`），与 `SummaryReport` 的蛇形字段并存。
  `证据：src/query.rs:678-690`
- 事实 127：`RangeView` 是带内部标签的枚举（`tag = "granularity"`，`day` 或 `instant`），内部字段保持蛇形；`instant` 同时给出本地时刻与 UTC 时刻。
  `证据：src/report.rs:1494-1515`
- 事实 128：明细 JSON 契约 `EventList`（`query_id / pricing_revision / rows / total / warnings`）与 `EventRow`（含 `cost_usd: Option<f64>` 与同源的 `cost_breakdown`；未收录模型两者皆 None）。
  `证据：src/report.rs:2279-2289, 2248-2277, 2222-2246`
- 事实 129：请求级费用明细（命中候选、渠道、来源、匹配方式、计价依据值、命中分段、四行分项单价/小计）由后端在**同一次** estimate 中产出，与 `cost_usd` 同源。
  `证据：src/report.rs:2337-2345`、`src/pricing.rs:911-936, 2433-2450`

## 未找到 / 不确定

- 未找到 `src/render/` 模块：本仓不存在该目录；任务描述里的「render/report.rs」对应实际文件 `src/report.rs`（见事实 123）。不确定该文段指的是历史分层命名还是别仓结构。
- 未找到 CLI 入口的当前实现：根 crate 无 `src/main.rs`、`Cargo.toml` 无 `[[bin]]`；`report.rs` 管线保留但仅被 GUI 命令与测试调用（事实 123）。仓库内也未检索到 CLI 相关帮助文本。
- 未实现「5 小时 block」/计费窗口：全 `src/` 无对应类型或计算（事实 120）；与 ccusage 的 block 口径无法在本仓源码中得到印证。
- 未实现 1 小时缓存价（或 5 分钟 / 1 小时缓存写倍率）：Claude 适配器只读 `cache_creation_input_tokens` 单一字段（事实 63），`PriceRates` 也只有一个 `cache_write` 槽位；源码中不存在区分逻辑。
- 未读出「按文件 mtime / since 剪枝」的实现在本仓存在（事实 23）：只有文件级指纹缓存（事实 24），没有按时间窗口跳过旧文件的逻辑。
- 未确认 ccusage 侧任何行为：本清单未阅读 ccusage 源码，E 节全部为 TokenScope 侧事实，不含 ccusage 实现细节。
- 未确认 `matching_time_rules`（`src/pricing.rs:607`）内部时区换算与命中细节的逐行语义：仅读到其调用点与外部契约（事实 60、68），函数体内部分支未逐行读完。
- 未确认 `history.rs` 中 CCS 导入写入路径的全部去重分支（`alias_claim_key`、`classify_batch` 等，`src/history.rs:2329-2560`）：仅读到键与冲突语义的注释与判定入口（事实 104-108），未逐行核对全部 SQL。
- 未确认前端 `frontend/src` 之外（如 `src-tauri` 窗口状态、`acceptance` 验收模式）对统计数字的任何影响：本清单未展开这些模块。
