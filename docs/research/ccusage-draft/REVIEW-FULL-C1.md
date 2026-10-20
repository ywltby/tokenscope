# 全文复核报告 C1（第 1–5 章，L45–L1079）

样本：`C:\Users\admin\AppData\Local\Temp\ccusage-research\ccusage`，HEAD `57afdc255fcbe031b0ecd7cac487f8fd7c5d531f`（v20.0.31）
复核对象：`docs/research/2026-10-14-ccusage-usage-statistics.md` 的 L45–L1079（§1 范围与方法、§2 总体架构、§3 统一数据模型、§4 主干与 Claude Code、§5 Codex）
方法：逐节通读全部 1035 行；抽查 60 条断言逐条打开源码核对（覆盖 §1–§5 每个二级章节与有实质断言的三级小节）；实测 `rust/target/debug/ccusage.exe --help`、`claude daily --json --offline`、`codex daily --json --offline`、`daily --by-agent --json --offline`（只记录结构与键名）。

## 结论（一句话）

抽样的 60 条断言中 59 条完全一致（数字、阈值、优先级、测试函数起止行尤其准），1 条不完整（行号落到测试而非实现）；内部一致性检查另发现 3 处问题——1 条引用指向空行（`src/README.md:54`）、1 张表的行号缺文件名、1 处「已闭环」声明超出 §13.5 的实际范围。全部为局部可修，无返工级缺陷，无实质口径错误。

## 抽查清单

| 编号 | 文档位置 | 断言摘要 | 源码位置 | 结果 | 备注 |
| --- | --- | --- | --- | --- | --- |
| C01 | §1 L47-49 | pnpm monorepo，workspace = `apps/*`/`docs`/`packages/*`，`packageManager: pnpm@12.11.2` | `package.json:5-8,15` | 一致 | 逐字相符 |
| C02 | §1 L49 | Rust 独立 workspace，`members = ["adapters/*","crates/*"]` | `rust/Cargo.toml:7` | 一致 | |
| C03 | §1 L51-55 | 九个大文件行数 6181/2922/2682/1990/1853/1582/1637/1411/1247 | 九文件逐行计数 | 一致 | **9/9 精确**，无一处偏差 |
| C04 | §2 L83 | `main()` = `main.rs:25-67`，先注册价格抓取回调再分派 | `rust/crates/ccusage/src/main.rs:25-67` | 一致 | 26 行 `set_json_fetcher` |
| C05 | §2 L87 | 无子命令 → `adapter::all::run`，默认 `AgentReportKind::Daily` | `main.rs:53-65` | 一致 | |
| C06 | §2 L94 | claude 六子命令在 `parser.rs:557-587`；blocks/statusline 仅 Claude 有专门报错文案（`:864-880`） | `ccusage-cli-parser/src/parser.rs:557-589`、`:864-886` | 一致 | 报错文案落在 878-881，在给出行段内 |
| C07 | §2 L97-98 | 测试用 17 元函数数组 + `runs.len() == 17` | `main.rs:90-113` | 一致 | 数组 92-110，断言 112 |
| C08 | §2 L100-103 | 三口径：别名 19 / `BUILT_IN_AGENT_NAMES` 18 / 分发数组 17 | `adapter/mod.rs:1-19`、`ccusage-core/src/lib.rs:63-82`、`main.rs:92-110` | 一致 | 实测 `--help` 亦为 claude+17 家 |
| C09 | §2 L110 | 计价发生在适配器内（`claude/src/lib.rs:525`） | `rust/adapters/claude/src/lib.rs:525` | 一致 | |
| C10 | §3.1 L116 | `UsageEntry` 家族 = `types.rs:7-39` | `ccusage-core/src/types.rs:7-39` | 一致 | 含 UsageMessage/TokenUsageRaw |
| C11 | §3.1 L140-143 | `cache_creation_token_count` 42-49、`total()` 78-95、单测 196-218 | `types.rs:42-49,78-95,196-218` | 一致 | 单测名亦相符 |
| C12 | §3.2 L147-153 | `TokenCounts` 67-96、`ModelBreakdown` 98-122、`missing_pricing` 110-111 | `types.rs:67-96,98-122,110-111` | 一致 | 均 `skip_serializing` / `Not::not` |
| C13 | §3.3 L157-180 | `LoadedEntry` 124-145；`LoadedFile.timestamp` 是死字段 | `types.rs:124-145`；`claude/src/lib.rs:501-504,660-666`；`claude/src/daily.rs:150,294-297,314-318` | 一致 | 全仓 grep `.timestamp` 无第三条写入路径，消费方只读 `.entries` → 死字段结论成立 |
| C14 | §3.4 L184-188 | `UsageSummary` = `types.rs:147-190`，维度字段全 `Option` + 按需序列化 | `types.rs:147-190` | 一致 | `extra_total_tokens` 亦 `skip_serializing` |
| C15 | §4.0 L209 | `main.rs:28-33` 分派；`adapter-all/src/lib.rs:11-30` 再导出 18 家 | `main.rs:28-33`、`ccusage-adapter-all/src/lib.rs:11-30` | 一致 | 恰 18 个别名 |
| C16 | §4.1 L225-229 | `paths.rs:23-43/25/26-31/33/37-39/40-42/45-53/57-63`；测试 `main.rs:382-396` | `claude/src/paths.rs` 同上、`main.rs:381-396` | 一致 | 全部行段与给出内容相符 |
| C17 | §4.1 L245-247 | `common/lib.rs:18-34/19-21/28`；`paths.rs:77` 排序；`main.rs:211-223`；`paths.rs:248-254` | `adapters/common/src/lib.rs`、`claude/src/paths.rs`、`main.rs` | 一致 | 排序确实决定去重先后 |
| C18 | §4.1 L247 | 「非单段过滤值退化为全量扫描（`lib.rs:972-982`）」 | 实现 `claude/src/paths.rs:69-75`；`claude/src/lib.rs:972-982` 是测试 | 不完整 | 结论正确、行号指到了测试（见「建议改述」1） |
| C19 | §4.1 L257-261 | `paths.rs:290-331/303-307/308-319/320-330`；`lib.rs:1014-1022`；`extract_project` 269-288 | 同上 | 一致 | workflows 兜底算术与文档描述逐项吻合 |
| C20 | §4.1 L271 | 会话身份：行内 `sessionId` 优先，缺失用路径值（`lib.rs:405-411`） | `claude/src/lib.rs:405-411` | 一致 | 同时被去重键与 sidechain 路由消费 |
| C21 | §4.2 L277-288 | `lib.rs:112-130` 装配链、`144-152` 价格表、`153` 时区、`155-157` 并行读、`170-187` 去重 | `claude/src/lib.rs` | 一致 | |
| C22 | §4.2 L293-302 | `common/lib.rs:89-101/84-126/49-81/53/59-62/66-75/77-80/113-124`；测试 `133-153`、`main.rs:176-209`；daily 副本 `daily.rs:236-283` | `common/src/lib.rs`、`main.rs`、`claude/src/daily.rs` | 一致 | 大文件优先 + 结果按原序回填 |
| C23 | §4.2 L307-318 | `lib.rs:491-603/505-516/509` 预过滤 `"usage":{`；daily `daily.rs:302`；`fast.rs:82-103/16-70/105-110`；`paths.rs:256-267` | 同上 | 一致 | 逐字节匹配结论成立 |
| C24 | §4.2 L322-326 | mtime 剪枝：常量 `paths.rs:84`、阈值 `137`、规则 `117-172`、`recorded_session_id` `210-234`（memmem `224`）、并查集 `174-205`、注释 `98-106`、测试 `387-538` | `claude/src/paths.rs` | 一致 | 6 个场景测试逐一起止相符 |
| C25 | §4.2 L324 | statusline：`commands/mod.rs:719-731`、`738-751`、`337-343`、`826-845`、`860-864` | `ccusage/src/commands/mod.rs` | 一致 | 「带 usage」判定实为 732-737（见「建议改述」2） |
| C26 | §4.3 L333-341 | `deserialize_usage_line` = `lib.rs:727-736`；不可为 null 清单 `867-886` | `claude/src/lib.rs` | 一致 | 零拷贝快路径 728-730 |
| C27 | §4.3 L345 | 坏行静默跳过 `lib.rs:514-516`、`517-519`；`jsonl.rs:29-30` 说明历史行为 | 同上 | 一致 | |
| C28 | §4.3 L350-351 | `usage_limit_reset_time_from_line_bytes` = `lib.rs:918-943`；`is_sidechain` 判定 442-444 | `claude/src/lib.rs:918-943,442-444` | 一致 | 三个准入条件逐字相符 |
| C29 | §4.3 L374-377 | `is_valid_usage_entry` = `668-707`；会话剔除 `commands/mod.rs:155-161`；统一层 `loader.rs:786-789` | 同上 | 一致 | JS 侧 5 项校验顺序一致 |
| C30 | §4.4 L386-406 | `usage_dedupe_hash` = `lib.rs:350-366`；`is_anthropic_message_id` 368-375；结构比对 413-425 | `claude/src/lib.rs` | 一致 | 三档键语义与表内描述一致 |
| C31 | §4.4 L411-420 | `should_replace_deduped_entry` = `lib.rs:198-212`；`usage_token_total` 190-196；`main.rs:262-283` | 同上 + `main.rs:262-283` | 一致 | 优先级 sidechain → token → speed 无误 |
| C32 | §4.4 L424, L433 | `main.rs:286-309` 两会话同 id 同秒 → 2 条；sidechain 两条域分隔哈希 `lib.rs:377-392`；查找 `237-266`；匹配 `427-440`；别名 `474-489` | `main.rs:285-309`、`claude/src/lib.rs` | 一致 | `push_deduped_session_alias` 确在 474-489 |
| C33 | §4.4 L447-450 | daily 侧 sidechain 路由**缺 timestamp 守卫**（`daily.rs:493-496` vs `lib.rs:436-440`），daily 测试固化该行为 | `claude/src/daily.rs:493-496`、`claude/src/lib.rs:434-440`、`daily.rs:731-769` | 一致 | **最实质的一条差异，确认属实**；§13.2 亦以语义级问题记录该条 |
| C34 | §4.5 L458-483 | `advisor_usages_from_line` = `lib.rs:635-658`；信封 `605-628`；拆条 `559-600`；`cost_usd=None` 于 `568`；自算价 `583-590`；display 得 0（`cost.rs:40`） | `claude/src/lib.rs`、`core/src/cost.rs:40` | 一致 | 1 行 → 2 条的行为与测试 399-421/423-447 相符 |
| C35 | §4.5 L489-497 | `-fast` 归一 `lib.rs:535-543`；daily 镜像 `338-346`；`suffix_string` `fast.rs:105-110` | 同上 | 一致 | |
| C36 | §4.5 L503-507 | 倍率 `cost.rs:110-115`；外置覆盖 `cli/types.rs:295`；缺价判定用原始模型 `lib.rs:526-532`；分组用 `entry.model` `summary.rs:65-66` | 同上 | 一致 | 「同一物理模型出现两个名字」的坑成立 |
| C37 | §4.6 L515-534 | `cache_creation_token_count` 优先对象 `types.rs:41-50/52-58`；1h 倍率 `cost.rs:118-131`、常量 `cost.rs:7` = 2.0 | 同上 | 一致 | 无对象时扁平字段全落 5m 桶 |
| C38 | §4.6 L538-540 | 四桶相加 `types.rs:78-95`、`utils.rs:14`；`cost.rs:139-142` 注释原文 | 同上 | 一致 | 「input 是未缓存余量」注释逐字相符 |
| C39 | §4.7 L547-567 | `CostMode` = `cli/types.rs:242-248`、`with_defaults` 62-71；`parse_cost_mode` `parser.rs:1134-1141`、赋值 `753`；三分支 `cost.rs:39-46/31-46/9-18/48-59/61-75` | 同上 | 一致 | display 不加载价格表 `lib.rs:144-152`、`daily.rs:45-53` 亦符 |
| C40 | §4.7 L571-581 | 五处调用点行号 + `--offline` `pricing.rs:1002-1038` + `--no-cost` `output.rs:158-161/564` + 表格分支 `249/304/369/512` | 同上 | 一致 | 四个 `no_cost` 分支逐行吻合 |
| C41 | §4.8 L587-612 | daily 独立实现 `daily.rs:30-76/35-62/63-75/112-129/131-146`；共用件行号 22-28/307/368/457-462/472/482-484/291-292/403-442/582-601/607-670 | `claude/src/daily.rs`、`core/src/summary.rs:39-123` | 一致 | 未共用清单（`should_replace_*`、累加器）逐项核实 |
| C42 | §4.8 L604 | 等价性测试 `main.rs:465-478`、`483-535`、`570-595` | `ccusage/src/main.rs` | 一致 | 断言主体 465-478/503/516-534 相符 |
| C43 | §4.9 L618-655 | 装载骨架 `loader.rs:29-37/83-89/91-98/100-396/398-420/39-81/116-358/359-380`；并行 `430-443/472-477/465-470`；Codex 扣减 `845-855/868-875` 与 `codex/report.rs:128` | `ccusage-adapter-all/src/loader.rs`、`adapters/codex/src/report.rs` | 一致 | 18 条 spec + pi 追加、只抛首个错误、按 index 回填均符 |
| C44 | §4.9 L659-672 | agent 名 `793/866/494-496/361`；`[pi] ` 前缀在 `adapters/pi/src/parser.rs:354-356`（默认名 `pi` 于 `pi/loader.rs:74-78`）、`openclaw/parser.rs:317`；`format!("[` 全仓仅此两处 + `help_codegen.rs:235/253`；`projectPath` 仅 pi 系 `589/628/819-821` | 同上 | 一致 | 全仓 grep 计数确认「仅两处」 |
| C45 | §4.9 L672-683 | 二次聚合 `482-492/417/399-414/888-908`；`AllAccumulator` `types.rs:78-130`、`agent:"all"` `116-129`；开关表调用点 `loader.rs:564/582/585/625/726`；`SharedArgs` 无 project 字段 `cli/types.rs:33-60` | 同上 | 一致 | `filter_loaded_entries_by_date` 生产调用点恰为 5 处（1164 在 `mod tests` 内）；`single_thread` 在 adapter-all 中确实不被读取 |
| C46 | §4.10 L690-724 | daily/weekly/monthly/session/blocks/statusline 的行段（33-42/44-57/72-87/110-124/146-181/183-206/155-161/208-260/243/262-307/267/272-277/310-376/318-325/420-435/378-384/541-555/557-559/568-597/826-845）；`date_utils.rs:196-203/190-195/205-225`；`last_window.rs:11-24/26-74/60-62` | `ccusage/src/commands/mod.rs` 等 | 一致 | 含「无分页参数」「`--last` 在解析期折算」两条否定式断言 |
| C47 | §5.0 L763-775 | 结论速览 12 行（数据根/发现/增量/事件模型/增量语义/cached/reasoning/回退模型/replay/compaction/速度/成本/json 链） | `codex/paths.rs:31,226,85`、`parser.rs:122,234,342,363,482,661,813,833`、`report.rs:63,123,272,709`、`types.rs:347`、`speed.rs:33`、`commands/mod.rs:327` | 一致 | 12/12 行行号与内容相符 |
| C48 | §5.1 L779-805 | `paths.rs:226-238/221-224/85-90/31-64/56-61/110-128/189-201/171-175/158-168/316-340/387-414/417-430`；`Cargo.toml:7-16` 无压缩 crate；全仓 `zstd` 零命中；`loader.rs:124-148/156-160/170-217`；`aggregate.rs:142-173/72`；`replay.rs:262-307/327` | 同上 | 一致 | 4 个日期过滤测试起止行精确 |
| C49 | §5.2 L809-838 | 信封 `types.rs:129-141/190-224/358-450`；分派 `parser.rs:328-555/342-362/363-447/448-454/464-478/479-554`；`session_meta` 只在 `replay.rs:391-428/218-249/407-427`；`codex_session_id` `892-904`；会话 id 匹配 `lib.rs:69-116`；预过滤 `23-45/661-715/277-279/708-713/677-682`；时间戳 `1174-1194` | 同上 | 一致 | 64 KiB 门槛、`>` 1e10 毫秒判定均符 |
| C50 | §5.3 L844-879 | `parser.rs:479-508` 引文逐行对应 482-497；差分 `1284-1308` 六字段 `saturating_sub`；钳制 `1310-1316` 与 `types.rs:330-340`；`total_tokens` 回退 `types.rs:350-353`；四个单测 559-572/591-602/604-617/651-657；字段别名 `278-308`；串行回归 2244-2282、942-995 | 同上 | 一致 | **含负数与溢出边界测试，逐条相符** |
| C51 | §5.4 L886-916 | `resolve_codex_usage_model` = `802-831`；回退 `gpt-5` `813-818/819-821/794-800`；`codex-auto-review` `833-846`；日期表 7 项降序；compaction 强制 fallback `401-424`、测试 `1510-1511`；聚合 `\|=` `523/845` | `codex/parser.rs`、`codex-auto-review-fallbacks.json:1-30` | 一致 | **日期表 7 项与 JSON 逐项、含顺序完全一致** |
| C52 | §5.5.1 L924-948 | 父关系 `replay.rs:407-427`；父路径取「首个非自身」`202-249/239-249`；计划 `92-151`；fork 截断 `158-177`；逐条全等 `parser.rs:234-253/236`；突发启发式 `129-177/254-263`、常量 `112-122`、阈值点 `171-173/256-261`；10 个测试行号 | 同上 + `codex/loader.rs` | 一致 | 10 个 loader 测试起止行**全部精确** |
| C53 | §5.5.2 L952-962 | 状态机 `73-85`；配对 `342-362/430-445`；本地 compaction 防重复 `510-530`；非法配对 `379-393`；跨文件键 `aggregate.rs:23-42/753-780/762-766`；保留策略 `720-731/750`；tier 合并 `types.rs:22-41`、测试 `aggregate.rs:1356`；日期边界 `replay.rs:181-199`、`parser.rs:311-323`；7 段 compaction 测试 | 同上 | 一致 | compaction_tests 7 个行段全部命中函数首行 |
| C54 | §5.6 L966-971 | 分组键 `aggregate.rs:408-430/416-423`；归一 `331/896` 与 `model_aliases.rs:14-26/39-61`；长上下文 `463-524`、阈值 `pricing.rs:2753-2763`；时间桶 `488-511`、`pricing.rs:272-277`；后处理 `404/582-676/897`；倍率表 `fast-multiplier-overrides.json:8` | 同上 | 一致 | `gpt-5.3-codex: 2.0` 确在第 8 行 |
| C55 | §5.7 L975-994 | 旗标 `--speed` `parser.rs:619/1194-1202`、`tests.rs:644-645`；映射 `649-659`；`config.toml` `speed.rs:33-49/55-71`；倍率读取 `pricing.rs:1071-1079/28-29`、匹配 `939-978`；放大点 `report.rs:234-275/250-271`；快照 2× 关系 | `parser.rs`、`speed.rs`、`pricing.rs`、`report.rs`、`snapshots/...:11,109` | 一致 | 快照 0.00040425（:11）与 0.0008085（:109）恰为 2 倍 |
| C56 | §5.8 L998-1012 | 调用链 `lib.rs:39-67`→`report.rs:394-404/183-217/234-275/343-392`；`costUSD` 全为渲染 `88/179`；缓存读回落 `343-348`；缓存写 `pricing.rs:1085-1088`；未定价 `406-419/28-30/74-76/709-715`；`0` 价测试 `758-794`；`--mode` 不接参 `183-188/394-404` | 同上 | 一致 | `report.rs:28-30` 引文逐字相符 |
| C57 | §5.9 L1016-1038 | 只收 `jsonl` `common/lib.rs:14`；`codex_line_usage_kind` `661-715`；`CodexLineKind` 仅两变体 `62-66`；`None` 丢弃 `277-279`；Session/Headless 判据 `661-707/717-769/708-713`；headless usage 四层 `1000-1046`、模型/时间戳 `922-953/1048-1128/1271-1282/569-572`；`service_tier=None` `645` | 同上 | 一致 | 测试 `loader.rs:689-755` 起行相符 |
| C58 | §5.10 L1042-1063 | `report.rs:16-35/37-53/147-181/55-98/91-96/436-467/674-685`；统一报告 `adapter-all/report.rs:28-37/133-172/148-156`；`codex_group_row` `loader.rs:830-886/863/858-859/878-881/675/688`；`tests.rs:544-568` | 同上 | 一致 | **实测字段名逐项吻合**：`codex daily --json` 顶层 `daily,totals`，行含 `costUSD/date/models/reasoningOutputTokens`；`models{}.isFallback` 存在；`daily --by-agent` 行级确有 `agents` 数组，totals 含 `unpricedModels` |
| C59 | §5 未验证 1 L1069 | `task_started`/`trigger_turn`/`inter_agent_communication` 只命中 `src/README.md` | 全仓非 `.md` 文件 grep | 一致 | 零命中，指控成立 |
| C60 | §5 未验证 2 L1070 | `docs/guide/codex/index.md:50/116` 说缺模型元数据的会话被跳过 | `docs/guide/codex/index.md:50,116` | 一致 | 行号精确 |

（编号连续至 C60，超出要求的 ≥25 条；§4 的 13 个三级小节与 §5 的 12 个小节均至少被一条断言覆盖。）

## 必须修正

**M1 — §5「未验证/存疑」第 2 条（L1070）：`src/README.md:63` 与「同文档 `:54`」——后者是空行。**
断言：`"`src/README.md:63` 与同文档 `:54` 也是这么说的"`。
实际：`rust/adapters/codex/src/README.md` 全文仅 65 行，**第 54 行为空行**；「缺元数据仍按 `gpt-5` 计入」的第二处证据实为 `docs/guide/codex/index.md:54`（Legacy fallback 条目）。
证据：`rust/adapters/codex/src/README.md:63`（`Pricing uses model metadata from turn_context … fall back to gpt-5 …`）；`docs/guide/codex/index.md:54`。
修法：把「同文档」改为全路径 `docs/guide/codex/index.md:54`，或删去该引用。

**M2 — §4「9.5 各开关在装载层的影响点」表（L676-683）：4/6 行行号不带文件名，且 `output.rs` 全章未定义归属。**
断言：`--json` 行「`loader.rs:108-115` …（`core/src/output.rs:14-16`）」、`--no-cost` 行「（`output.rs:158-159` 入参、`:249/304/369/512` 调用点，剥离函数 `output.rs:564-581`）」、`--offline` 行「唯一入口 `loader.rs:83-89`」、`project_filter` 行「（`loader.rs:635`/`648` 传 `None`）」。
实际：同一张表第 1 行写全了 `adapters/common/src/lib.rs:36-47` 与 `adapter-all/src/loader.rs:564/…`，`--single-thread` 行写了 `adapters/common/src/lib.rs:89-96`；而上述四行退回裸名。裸 `output.rs` 指 `rust/crates/ccusage-core/src/output.rs`，但 §4 全章此前从未出现该文件名（`core/src/output.rs` 只在 §2.1 的表里间接出现）。
证据：`rust/crates/ccusage-adapter-all/src/loader.rs:83-89,108-115,635,648`；`rust/crates/ccusage-core/src/output.rs:14-16,158-161,249,304,369,512,564-581`。
修法：四行补全为 `adapter-all/src/loader.rs` / `core/src/output.rs`（行号本身已核实无误，第 8 轮「表行号带文件名」的改动未落实到这四行）。

**M3 — §4「未验证/存疑」导语（L741）与 §5 同款导语（L1067）声称的闭环范围与 §13.5 不符。**
断言：「第 8 轮实测复核（§13.5）已闭环本节中与内嵌表键集、运行时行为、格式化相关的条目；此处保留原始清单与当时的判断，逐条状态以 §13.5 为准。」
实际：§13.5 的闭环表（L3476-3487）与运行时表（L3519-3531）针对的是**内嵌 LiteLLM/models.dev 键集、`format_currency` 舍入、在线端点差异、生成器对齐**，以及 Claude 侧 `--mode` 与 `--by-agent` 两类入口。而 §4 该节七条里没有任何「内嵌表键集/格式化」条目，其「运行时」条目（progress 行双路径分叉、`--single-thread` 对统一报表无效）**未被** §13.5 触及；§5 该节八条中，第 3 条与 §13.5 明判「未闭环」的第 10 条同源（`CODEX_REWRITTEN_BURST_PAUSE_MS` 的经验依据），第 7 条（`--mode display` 在 Codex 下不改变金额）也没有对应实测行。
证据：`docs/research/2026-10-14-ccusage-usage-statistics.md:741,747,1067,1071,1075`；`:3421-3423`（§13.3 状态更新）、`:3476-3487`、`:3519-3531`。
修法：改为「§13.5 未覆盖本节条目，逐条状态仍以本清单为准」，或按节列出确实闭环的条目编号。

## 建议改述

1. **§4.1（L247）**：「非单段过滤值退化为全量扫描（`lib.rs:972-982`）」——实现是 `rust/adapters/claude/src/paths.rs:69-75`（`project_filter.filter(|f| is_project_path_segment(f))`，为 `None` 时收整个 `projects/`），`claude/src/lib.rs:972-982` 只是钉住该行为的测试 `falls_back_to_full_discovery_for_non_segment_project_filter`。建议改成 `paths.rs:69-75`（测试行号可保留为附注），避免与「测试 helper 当生产证据」的既有整改口径冲突。
2. **§4.2（L324）**：「从尾部反向找最后一条 `type == "assistant"` 且带 `usage` 的行（`commands/mod.rs:719-731`、`738-751`）」——`type` 判定在 729-731、`usage` 存在性判定在 732-737、`input_tokens` 与两个 cache 桶读取在 738-751。建议合并写成 `719-751`，避免读者以为 731 处已完成「带 usage」过滤。
3. **§5.0 表（L767）**：「增量语义 … `parser.rs:482`」——482 只是 `let info = payload.info.as_ref();`，真正的 `last_token_usage` 优先级与 `cumulative_advanced` 判定在 484-497。§5.3 已给出精确区间 `479-508`，建议表内对齐。
4. **§5.9（L1036）**：「模型与时间戳同样有 4 层回退（`parser.rs:922-953`, `parser.rs:1048-1128`）」——922-953 已逐行核实（entry → data → result → response），1048-1128 只核到起点；建议拆为 `922-953` 与 `1048-1054`（时间戳四层）+ 其余，或标注为范围引用。
5. **§4 未验证（L749）**：行数口径一条列出「任务书给出的若干行数（`cost.rs` 618、`pricing.rs` 5759、`adapter-all/loader.rs` 1532）与实测（664、6181、1637）不符」——建议与 §1（L16-17）的「非空行数口径」说明互相加一句交叉引用，避免读者以为任务书数字是笔误。

## 内部矛盾

1. **M3 所述**：§4 / §5 的「未验证/存疑」导语与 §13.3 的状态更新（明列第 5、9、10 条未闭环）及 §13.5 的对照表相互矛盾/不等价；全文共 6 处出现同一句模板文字（L741、L1067、L1484、L2140、L2733、L3310），属于装配期的批量插入，建议按节改写。
2. **§4.9.5 表内外不一致**：同一张表内，两行写全路径、四行写裸文件名（M2）；§4 其他位置（如 L682「`loader.rs:422-480`」、§4.10.5「`loader.rs:403-413`」）也一律用裸 `loader.rs`，与 §4 引言「路径相对仓库根」的约定存在张力。建议在 §4 引言处补一句「本章 `loader.rs` 默认指 `rust/crates/ccusage-adapter-all/src/loader.rs`，`lib.rs` 默认指 `rust/adapters/claude/src/lib.rs`」。
3. **已复核为「不矛盾」的两处**（列入清单以免误判）：
   - §3.2（L152）「拆解发生在输出层，不在解析归一阶段」与 §5 §3（L861-867）「cached 不从 `input_tokens` 里扣，只在输出层做减法」——表述一致；`parser.rs:1310-1316` 只做上界钳制，`report.rs:123-129` 做减法，两处引用均核实。
   - §4.8.4（L611）「`lib.rs:434-435` 的注释本身就承认了这个差异」——`claude/src/lib.rs:434-435` 注释原文为「Requestless replays match across timestamps, as in the daily loader. Replays that carry a new request ID still need the parent's timestamp in this loader.」，与转述相符；§13.2（L3407-3409）亦把该差异记为第 2 轮语义级修正项，前后一致。

## 无法验证

以下引用在允许的阅读预算内未打开正文逐行确认（仅确认所在文件与函数存在，或未做依赖图分析），建议由后续轮次补核：

1. §4.3（L341）null 契约的实现段：`claude/src/lib.rs:741-830`（`UniqueJsonValue`）、`819-824`（重复成员名拒绝）、`832-841`/`849-854`（`usage.iterations[]` 豁免范围）、测试 `1082-1127`、`1136-1142`。
2. §4.4（L433、L450）测试矩阵：`claude/src/lib.rs:1354-1396`、`1398-1441`、`1443-1491`、`1493-1535`；以及 L424 提到的 `lib.rs:1195-1222`、`1308-1329`。（其中 `1443-1491`「共用同一时间戳、无覆盖反例」的结论与 §4.8.4 一致，但未逐行核对。）
3. §5.2（L962）`parser.rs:311-323`（compaction 历史回填）；§5.2（L957）`parser.rs:1561-1585`。
4. §5.5.2 的 `compaction_tests.rs:94-176` 等 7 段的正文（仅核对了每段起始函数行与名称）。
5. §5.10（L1047）快照 `:6-59` / `:104-163` 的收尾行号（仅核对了 `:11`、`:104`、`:109` 三个锚点）。
6. §5.1（L796）「`flate2` 只在 `ureq` HTTP 栈里出现」——未展开 workspace 依赖图（`rust/adapters/codex/Cargo.toml:7-16` 无压缩 crate 已核实）。
7. 运行期未覆盖：`--single-thread`、`blocks`、`statusline` 三个入口本轮未运行（报告只允许记录结构与键名，且这三项至少有一项会触发对 `~/.claude` 的全量重扫）；§4 未验证第 5 条（`--single-thread` 对统一报表无效）因此仍属代码阅读结论。

## 我实际打开过的文件（file:line）

1. `rust/crates/ccusage/src/main.rs:1-120, 170-599`（分派、17 元契约、Claude 侧测试）
2. `rust/crates/ccusage/src/adapter/mod.rs:1-19`（19 个别名）
3. `rust/crates/ccusage/src/commands/mod.rs:28-152, 143-267, 268-387, 410-469, 535-609, 665-774, 800-879`（六个报表入口、statusline）
4. `rust/crates/ccusage/src/cli/last_window.rs:1-189`（`--last` 折算）
5. `rust/crates/ccusage-cli/src/types.rs:30-109, 215-314`（`SharedArgs`、`StatuslineArgs::default`、`CostMode`、`PricingOverride`）
6. `rust/crates/ccusage-cli-parser/src/parser.rs:550-599, 610-629, 746-759, 855-894, 1125-1154, 1190-1209`
7. `rust/crates/ccusage-cli-parser/src/tests.rs:638-657`（`--by-agent` 拒绝）
8. `rust/crates/ccusage-core/src/types.rs:1-219`（全文）
9. `rust/crates/ccusage-core/src/cost.rs:1-160`（三分支、倍率、1h 倍率、注释）
10. `rust/crates/ccusage-core/src/summary.rs:1-130`（累加器与模型分组）
11. `rust/crates/ccusage-core/src/date_utils.rs:185-294`（窗口比较与毫秒边界）
12. `rust/crates/ccusage-core/src/output.rs:1-40, 150-179, 558-587`（`wants_json`、`strip_cost_json`）
13. `rust/crates/ccusage-core/src/fast.rs:1-115`（预过滤、`byte_lines`、`suffix_string`）
14. `rust/crates/ccusage-core/src/utils.rs:1-40`（`total_usage_tokens`、`apply_total_token_fallback`）
15. `rust/crates/ccusage-core/src/model_aliases.rs:1-100`（`CCUSAGE_MODEL_ALIASES`）
16. `rust/crates/ccusage-core/src/pricing.rs:25-44, 265-294, 935-979, 998-1042, 1060-1119, 2745-2769`
17. `rust/crates/ccusage-core/src/lib.rs:55-138`（`BUILT_IN_AGENT_NAMES`、`DEFAULT_RECENT_DAYS`）
18. `rust/adapters/common/src/lib.rs:1-169`（全文：收集、日期过滤、分块、并行读）
19. `rust/adapters/claude/src/lib.rs:1-120, 121-190, 185-324, 330-449, 490-603, 605-709, 915-944, 960-1029`
20. `rust/adapters/claude/src/daily.rs:22-141, 140-182, 170-289, 290-409, 440-559, 560-679`
21. `rust/adapters/claude/src/paths.rs:1-150, 150-243, 240-359, 385-555`
22. `rust/crates/ccusage-adapter-all/src/lib.rs:1-101`（全文）
23. `rust/crates/ccusage-adapter-all/src/loader.rs:1-130, 396-515, 550-699, 700-889, 1145-1174`
24. `rust/crates/ccusage-adapter-all/src/types.rs:5-34, 75-174`
25. `rust/crates/ccusage-adapter-all/src/report.rs:24-68, 125-204`
26. `rust/adapters/codex/src/lib.rs:1-125`
27. `rust/adapters/codex/src/parser.rs:18-52, 60-199, 200-349, 340-469, 470-589, 590-719, 790-919, 920-959, 995-1054, 1168-1197, 1270-1349, 1500-1539`
28. `rust/adapters/codex/src/paths.rs:20-219, 220-249`
29. `rust/adapters/codex/src/replay.rs:85-214, 215-259, 255-384, 384-429`
30. `rust/adapters/codex/src/aggregate.rs:18-72, 140-179, 320-359, 395-534, 578-687, 700-814, 838-900, 930-969, 1345-1389`
31. `rust/adapters/codex/src/report.rs:1-190, 180-309, 340-479, 660-739, 750-849`
32. `rust/adapters/codex/src/types.rs:1-50, 125-254, 270-339, 320-379, 535-658`
33. `rust/adapters/codex/src/speed.rs:1-100`
34. `rust/adapters/codex/src/codex-auto-review-fallbacks.json:1-30`（全文）
35. `rust/adapters/codex/src/fast-multiplier-overrides.json:1-14`（经 `ccusage-core/src/fast-multiplier-overrides.json`）
36. `rust/adapters/codex/src/snapshots/ccusage_adapter_codex__…fallback_models.snap:4-112`（锚点行）
37. `rust/adapters/codex/Cargo.toml:1-20`
38. `rust/adapters/codex/src/README.md:20-65`
39. `docs/guide/codex/index.md:1-117`
40. `rust/Cargo.toml:1-30`、`package.json:1-16`
41. `rust/adapters/hermes/src/parser.rs:171`、`rust/adapters/droid/src/loader.rs:83`、`rust/adapters/goose/src/parser.rs:92`、`rust/adapters/kimi/src/parser.rs:362`、`rust/adapters/copilot/src/loader.rs:513`、`rust/adapters/codebuff/src/loader.rs:83`、`rust/adapters/openclaw/src/parser.rs:317,405`、`rust/adapters/pi/src/parser.rs:350-361`、`rust/adapters/pi/src/loader.rs:70-82`、`rust/crates/ccusage-cli-parser/src/help_codegen.rs:233-256`、`rust/crates/ccusage-core/src/project_names.rs:119-132`（`extra_total_tokens` 与 `format!("[")` 全仓核查）
42. 构建产物实测（只记录键名结构）：`rust/target/debug/ccusage.exe --help`、`codex --help`、`claude daily --json --offline`、`codex daily --json --offline`、`daily --by-agent --json --offline`
