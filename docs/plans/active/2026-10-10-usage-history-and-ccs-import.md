# TokenScope 统一用量数据库与 CCS 历史导入实施计划

> 执行时按任务逐项使用 executing-plans，先写失败测试，再实现和记录验证结果。

**Goal:** 将已观察到的有效 token 用量长期保存到 TokenScope 自己的数据库，纳入所有代理的用量，并支持从 cc-switch 导入请求明细和历史日汇总。

**Architecture:** 使用一个 `~/.tokenscope/history.db`，同时保存用量事实、采集指纹和导入元信息。项目身份和 cwd 是记录的列，不按项目分库；逐请求数据与只有日粒度的外部汇总分表保存。查询从持久库建立不可变快照，日志采集和 CCS 导入只更新 TokenScope 自己的数据。

**Tech Stack:** Rust、现有 rusqlite/SQLite、Tauri 2、Vue 3、现有 ModelIdentity/项目路径规范化/四桶 token 契约。

- 日期：2026-10-10
- 状态：设计方案；需求已确认，产品实现尚未开始。
- 核对基线：`0c18e85`；交付基线：`82dd7a4`，接线时须遵守已实施的首次启动隐私闸门。
- 用户已确定：统一数据库放在 `~/.tokenscope/`；只保存关键 token 和 cwd 等元数据，不复制完整 JSONL；所有代理都关注用量，不建立主代理与子代理关系。

## 1 当前事实与设计选择

Codex 本机 `sessions/` 中仍有 2026-03-24 开始的 weixiao 会话，距本次核对正好 200 天，其中有真实 `token_count` 用量。隔离真实数据探针经现有 TokenScope 管线查到该项目 2,146 条去重记录。因此不能将 Claude Code 的 30 天清理规则泛化成 Codex 也按该期限清理。

Claude Code 默认对终端会话及其子代理记录执行 30 天保留期清理；具体是否执行还受配置读取、运行模式及会话来源等条件影响。历史文件不在本机只能证明当前不可读，不能单凭此确定每个文件的删除原因。[官方保留规则](https://code.claude.com/docs/en/claude-directory#cleaned-up-automatically)

本次只读核对 CCS 数据库得到以下结构：

| 数据 | 本机现状 | 可恢复的粒度 |
| --- | --- | --- |
| `proxy_request_logs` | 1,909 行，包含 Claude/Codex 会话导入记录 | 请求时间、模型、token、原生会话或来源请求标识；没有 cwd/project 列 |
| `usage_daily_rollups` | 109 行，按日期、应用、渠道和模型等维度保存 | 日级请求数与 token 汇总；没有逐请求时间、会话、cwd 或项目 |
| mimo 历史 | 23 行日汇总，共 16,324 次请求，2026-05-08 至 2026-06-26 | 可恢复历史模型用量，不能还原请求明细或项目归属 |
| `session_log_sync` | 文件同步游标 | 不是用量事件库，不能用文件列表反推出日汇总属于哪个项目 |

CCS 自身会将旧明细归并成日汇总，导入必须兼顾两张表。[CCS 数据表说明](https://github.com/farion1231/cc-switch/blob/main/docs/user-manual/en/5-faq/5.1-config-files.md)

选用统一数据库，按项目建立索引。每项目一个库会让跨项目汇总、迁移、备份和项目移动更复杂；仅保存每日 rollup 无法支持历史请求下钻。统一库保留精简请求事实，同时利用源文件指纹增量采集，避免长期保存两份同内容的请求缓存。

## 2 必须保持的不变量

1. 所有有效用量按来源应用的解析规则纳入，Claude 的 `isSidechain` 不再作为用量排除条件；主/子代理关系、层级、昵称不入库、不增加界面分类。
2. 来源日志及 CCS 库只读。所有持久写入均在 TokenScope 自己的数据目录。
3. 源文件删除、移动、来源停用、改采集目录、重扫或清采集指纹，都不能删除已保存的用量。停用来源只停止新采集，已存历史仍可经应用筛选查看。
4. 同一请求的流式更新或重复来源不能重复累计；同一 CCS 记录再次导入执行更新，不执行累计加法。
5. 四桶仍互斥：非缓存输入、输出、缓存写、缓存读。保留原始模型名，模型等价键只作派生索引。
6. 时间精度不降低；原生事件持有 UTC 秒与纳秒。CCS 请求的秒级精度、日汇总的原始日期与来源时区如实标记。
7. 持久库不是可丢弃缓存。迁移失败事务回滚，版本不支持或库损坏明确报错，不执行删表重建，不静默用现存日志代替完整历史。
8. 日汇总不能伪造成请求事件，不能构造 midnight 时间、平均 token 请求或猜测 cwd；无法准确判断的重叠不能直接相加。
9. 一次查询冻结数据快照、价格、时间边界和来源选择；采集或导入提交后只影响新查询，旧分页保持一致。
10. 测试与验收注入隔离数据目录和价格索引。新增历史路径必须沿用测试的 `cache_dir` 隔离根，不能因新增缺省路径触碰真实 `~/.tokenscope`。

## 3 数据库结构

长期只使用一个用量数据库 `history.db`；原来的 `cache.db` 仅作为升级时可读取的遗留文件。价格快照、设置和窗口文件继续沿用各自现有格式。

| 表 | 保存内容 | 唯一性和用途 |
| --- | --- | --- |
| `history_meta` | schema/parser/身份规则版本、提交 generation、迁移标记 | 迁移和查询失效依据 |
| `source_files` | 应用、采集根、规范化文件路径、指纹、解析上下文修订、最近成功采集时间、诊断 | `(app, root, path)` 唯一；缺失只更新状态，不级联删除事件 |
| `usage_events` | 一次已归一化请求的时间、原始模型、四桶、session/request 标识、项目 key、初始 cwd/event cwd | 稳定事件 key 唯一；同请求更新终值，独立请求独立保存 |
| `event_origins` | 事件对应的源文件位置或 CCS 来源记录 key、导入批次、解析版本；CCS 原模型/计价模型及来源费用、时间精度 | 保留去重后的来源依据，不是代理关系表 |
| `ccs_daily_usage` | CCS 完整行主键、原始日期、来源时区、请求数、四桶、输入口径版本、来源费用 | 日汇总单独保存；缺失的项目/session 为未知 |
| `import_runs` | 逻辑来源、来源路径及 schema、时间、插入/更新/重叠/拒绝数量、导入修订 | 导入结果可核查，批次与数据同事务提交 |

`usage_events` 核心字段如下，实施时将建表 SQL 放在版本化迁移内：

```sql
CREATE TABLE usage_events (
    id INTEGER PRIMARY KEY,
    event_key TEXT NOT NULL UNIQUE,
    app TEXT NOT NULL,
    ts_seconds INTEGER NOT NULL,
    ts_nanos INTEGER NOT NULL CHECK (ts_nanos BETWEEN 0 AND 999999999),
    model_raw TEXT NOT NULL,
    model_identity TEXT NOT NULL,
    identity_revision INTEGER NOT NULL,
    session_id TEXT,
    record_id TEXT,
    project_key TEXT,
    session_initial_cwd TEXT,
    event_cwd TEXT,
    input_tokens TEXT NOT NULL,
    output_tokens TEXT NOT NULL,
    cache_write_tokens TEXT NOT NULL,
    cache_read_tokens TEXT NOT NULL,
    parser_revision INTEGER NOT NULL,
    observed_at_utc TEXT NOT NULL
);
CREATE INDEX idx_usage_time ON usage_events(ts_seconds, ts_nanos, id);
CREATE INDEX idx_usage_app_time ON usage_events(app, ts_seconds, ts_nanos, id);
CREATE INDEX idx_usage_project_time ON usage_events(project_key, ts_seconds, ts_nanos, id);
CREATE INDEX idx_usage_model_time ON usage_events(model_identity, ts_seconds, ts_nanos, id);
```

Token 字段为经过 Rust 校验的十进制 `u64` 文本，避免 SQLite 有符号整数上限截断有效数值；禁止 SQLite 隐式转 REAL 汇总。聚合继续在 Rust 中使用受检算术，读取也重新校验，不将负数或坏值钳成 0。普通时间和行位置使用 SQLite INTEGER。

数据库开启 WAL、外键、有限 busy timeout 和持久事务同步。用量与来源提交完成后再更新采集指纹，崩溃不会出现“指纹已成功但用量没落库”。事件与源文件之间不使用 `ON DELETE CASCADE`。不保存 prompt、代码、工具输出、完整原始行或 CCS 的账户/密钥表。

## 4 采集、去重与历史查询

启动业务、手动刷新以及应用运行期间的定时采集都进入同一个后台单飞协调器。定时周期初定 5 分钟，按指纹只解析变化文件，退出时允许等待已开始的提交；不另造 daemon，也不修改上游清理设置。必须说明：TokenScope 未运行且源日志在首次采集前就被清理的数据，无法由本库自动追回。

Claude 继续发现所有层级的 JSONL，移除 sidechain 的用量跳过逻辑。文件内混合执行上下文的 cwd 解析仍需隔离，不能因纳入一条 sidechain 用量而推进另一上下文的项目根；这是来源解析正确性，不建立代理关系产品。Codex 当前已经递归发现各 rollout，不因会话来源是子代理而排除；默认根按 `CODEX_HOME`（缺失时 `~/.codex`）发现其 `sessions/` 和 `archived_sessions/`。显式自定义根保持只在选定目录内递归，不扩大到目录之外；发现只覆盖用量 rollout，不能误把 `history.jsonl` 当作 rollout，并保留来源冲突校验。

存储时沿用已经验证的原生请求口径：Claude 同 `(session_id, message.id)` 保留最终用量；Codex 使用现有 `(session_id, 原始模型, 四桶)` 重播规则。不同源文件保存同一原生请求时记录多个 origin，而不是复制用量。Codex 同模型同用量的独立请求仍有被保守合并的既有限制；引入数据库不会自动解决这项身份缺失，本期不借导入改写原生计量算法。

改变解析规则时，存在的源文件重解析；已经消失的记录以保存的字段继续可查，保留其解析版本，不声称能够恢复未保存的原始上下文。追加和同请求终值更新应幂等；文件截短、替换或暂时不可读不能清空旧事实，需保留旧记录并给出诊断。身份规则升级仅重建派生 key/索引，不删除 token 事实。

查询不再把所有历史加载进 `CollectionSnapshot.events`。每个 query_id 持有一个有期限的 SQLite 只读事务快照及冻结价格；汇总按筛选条件流式读取，分页按 `(ts_seconds, ts_nanos, id)` 稳定排序。沿用现有查询数量、闲置超时、游标归属检查与预算，及时释放过期读事务，避免长期阻塞 WAL checkpoint。全部历史汇总也必须使用流式读取。

界面中的“重建缓存”改为“重扫日志”：重置采集指纹并重新解析当前文件，不清 `usage_events` 或 `ccs_daily_usage`。历史删除不是缓存操作，本期不增加自动保留天数或自动清理用量功能。

## 5 CCS 导入

### 5.1 只读预览与原子导入

设置页提供“从 CCS 导入用量”，默认路径 `~/.cc-switch/cc-switch.db`，可选择数据库文件。先只读打开，在一致读事务中检查 `PRAGMA user_version`、实际表列和支持的应用，仅读取用量白名单列；不要只复制主文件而漏掉 WAL，也不要复制整个 CCS 库到 TokenScope。

预览展示支持的数据范围、请求明细数、日汇总数、可补充的历史、重叠数量、无项目记录数量和来源日时区。预览基于已读出的用量导入计划；提交时校验该计划的修订和有效期，过期则重新预览，避免预览和导入使用两份不同数据。大导入采用有容量限制的流式暂存，仅持有用量白名单字段，暂存位置属于 TokenScope。

用户点击导入后，在 TokenScope 的一个事务中完成记录 upsert、批次结果和 generation 更新。取消、错误和崩溃回滚；不修改 CCS 的同步游标或价格配置。错误通过结构化结果显示，不写入任何账户字段。

### 5.2 字段与 token 口径

| CCS 字段 | TokenScope 处理 |
| --- | --- |
| `app_type` | 本期仅支持 Claude/Codex，其他应用在预览列出但不导入 |
| `model` / `request_model` / `pricing_model` | 保留原值；应用模型身份规则生成派生 key，费用口径不从客户端别名猜测 |
| `created_at` | Unix 秒作为秒级事件时间，不伪造纳秒 |
| `session_id` / `request_id` / `data_source` | 保存为来源元数据；仅可证实的原生身份用于跨来源去重 |
| `input_tokens` / `input_token_semantics` | 按版本化兼容规则转换为非缓存输入 |
| `output_tokens` / `cache_creation_tokens` / `cache_read_tokens` | 分别映射输出、缓存写、缓存读，经过受检校验 |
| 缺 cwd/project | 保持 NULL，项目视图显示“项目未知”；不能按模型、日期或同步文件路径猜分配 |
| `total_cost_usd` | 作为 CCS 来源费用保留，与 TokenScope 当前费率估算分开 |

已核对 CCS 主源口径：`input_token_semantics=0` 是 legacy，`1` 是 total-inclusive，`2` 是 fresh；旧 Codex 行需要扣缓存读，新 total-inclusive 行扣缓存读与写，fresh 不再扣。Claude legacy 输入本身不含缓存。[CCS 口径实现](https://github.com/farion1231/cc-switch/blob/main/src-tauri/src/services/sql_helpers.rs)

兼容规则必须有独立版本和 fixture。未知枚举、legacy 中无法确认的缓存写组合、负数、溢出和矛盾桶不能猜测归一；预览显示具体拒绝原因，默认不提交含未解决错误的导入计划。CCS 原有“有效用量”过滤和 proxy/session 重叠规则也需从支持版本核对，不把所有 SQL 行无条件视为独立请求。

### 5.3 幂等与重叠

本期支持一个逻辑 CCS 导入来源。文件路径和导入时刻不充当用量身份；换成同一库的备份再导入仍更新同一逻辑来源。多设备独立 CCS 数据集的合并另行设计，避免把两个不同机器的 `_session` 日汇总误认成一份。

请求级导入键包含来源应用、`data_source` 和 CCS 原始 `request_id`；日汇总键保持 CCS 的完整主键 `(date, app_type, provider_id, model, request_model, pricing_model)`。同键的日汇总是一个完整快照，重新导入覆盖该来源值，不将新总量再次加到旧总量。CCS 已清掉的请求明细在 TokenScope 保持可查。

若能核对原生 request 标识，CCS 与原生日志指向同一请求时只保留一份统计，并保存来源依据。时间、模型和 token 恰好相同只能作为核对线索，不能直接宣称是同一个真实请求。

只有日粒度的历史无法精确求出与部分明细的交集。为此在每个 `(CCS 原始统计日, 来源时区, 应用, 等价模型)` 桶内选取一份统计来源，不能相加或用金额/token 差额制造“剩余请求”：

- 有 TokenScope 原生明细时，默认使用原生明细，保存重叠 CCS 数据作为核对依据；预览明确列出是否有更大 CCS 数值，不能将部分覆盖称为已补全。
- 没有原生明细，CCS 请求与日汇总的请求数和四桶可证实完整一致时，使用请求明细。
- 没有原生明细但 CCS 明细只覆盖日汇总的一部分时，主统计使用完整日汇总；保存已有明细，明确“该统计桶只有部分请求可查”。
- 仅有 CCS 请求或仅有日汇总时，使用对应粒度。

桶的来源选择在项目/下钻过滤之前固定，保证不同视图不因筛选条件改变统计优先级。无法判断的重叠在预览标记为未解决；用户如选择 CCS 数值，必须替换整桶来源，不能把它作为额外用量加上去。

### 5.4 日汇总的边界

CCS 按 `date(created_at, 'unixepoch', 'localtime')` 生成日汇总，库内日期不携带时区。导入预览要求明确来源统计时区，默认本机时区，记录此假设；不能自动宣称它是 UTC。[CCS 日汇总代码](https://github.com/farion1231/cc-switch/blob/main/src-tauri/src/database/dao/usage_rollup.rs)

原始日汇总不能重新切到任意时区。有日期筛选或按日分组且查询时区不同于来源日时区时，返回可恢复的 `rollup_timezone_mismatch`，提供切回来源时区或仅查看请求明细的选择；不限日期的模型/应用总量不受重切日期影响。不能把源日期当成午夜事件后重新换日。

日汇总缺少逐请求 prompt 规模和事件时间，不能按当前上下文档位、峰谷规则精确重新估价。主估算把这部分费用标记未知并计入未知 token，原因明确为“历史数据只有汇总”，不能误报“模型价格未收录”；CCS 原费用仅作为来源值。历史请求数保留 CCS 的 `request_count` 与来源单位说明，不声称它经过 TokenScope 的逐请求去重验证。

## 6 升级与备份

首次迁移先以只读方式检查旧 `cache.db`，在任何旧 `Cache::open` 的版本重建或 purge 之前，将可验证的已缓存用量迁入新库；再扫描当前全部来源补齐，包括此前跳过的 sidechain。整批迁移提交后写一次性标记，重复启动不重复导入。旧缓存未存过的 sidechain 或已经被 purge 的历史无法由此次迁移恢复。

迁移失败保持旧缓存和新库事务前状态，允许重试。成功后旧 `cache.db` 不再参加运行时查询，也不自动删除它；稳定运行时只有 `history.db` 一份活动用量存储。

新增版本升级使用事务迁移，变更前通过 SQLite backup API 在 TokenScope 目录生成可恢复备份，避免仅复制主文件丢掉 WAL。未知新 schema 保留文件并报错，禁止按缓存的 DROP TABLE 模式处理。备份包含已归一化用量与元数据，不包含源 JSONL。

## 7 实施任务与验收

以下每项按“失败测试 → 运行确认失败 → 最小实现 → 定向测试通过 → 记录验证结果”执行。涉及统计口径的任务先更新 `docs/stats-semantics.md` 中的不变量；完成后按仓库门禁提交，不混入其他工作区改动。

| 任务 | 文件 | 定向验收 |
| --- | --- | --- |
| H01 持久库与受检读写 | 新增 `src/history.rs`、`tests/history_storage.rs`；修改 `src/lib.rs` | 幂等写入、终值更新、u64 边界、事务失败回滚、schema 升级保留历史、来源缺失不删除；`cargo test --offline --test history_storage` |
| H02 纳入所有代理用量 | `src/source/claude.rs`、`src/source/codex.rs`、`src/source/mod.rs`、`src/dedupe.rs`；新增 `tests/all_usage_sources.rs` 与合成 fixtures | 独立请求相加、同请求跨主文件/子目录去重、流式终值、混合 cwd 不串项目、Codex 归档发现；`cargo test --offline --test all_usage_sources` |
| H03 遗留缓存迁移 | `src/history.rs`、`src/cache.rs`、`src/report.rs`；新增 `tests/history_migration.rs` | v10 缓存先迁再扫、仅迁合法字段、已消失源记录保留、失败可重试、重复迁移幂等；`cargo test --offline --test history_migration` |
| H04 单飞增量采集 | `src/report.rs`、`src/settings.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/commands.rs`；新增 `tests/history_collection.rs` | 源删除/移动/停用不删历史、append/截短/替换、定时与手动并发、退出中止与提交；未同意不打开历史库；`cargo test --offline --test history_collection` |
| H05 CCS 兼容与导入 | 新增 `src/import/ccs.rs`、`src/import/mod.rs`、`tests/ccs_import.rs`、合成 SQLite fixtures；修改 `src/lib.rs` | 两种粒度、输入枚举 0/1/2、缺列/坏值拒绝、有效记录过滤、同库/备份重复导入、来源 WAL 一致读取、取消/失败零提交；`cargo test --offline --test ccs_import` |
| H06 重叠与历史聚合 | `src/history.rs`、`src/aggregate.rs`、`src/model.rs`、`src/report.rs`；新增 `tests/history_overlap.rs` | 完整/部分/无法确定重叠均不双加，未知项目、模型等价、多渠道日键、查询前固定桶选择、日时区边界、未知费用；`cargo test --offline --test history_overlap` |
| H07 冻结查询与流式读取 | `src/query.rs`、`src/report.rs`；新增 `tests/history_queries.rs`；更新查询预算和游标测试 | 导入或采集后旧查询不变、新查询见新 generation、同时间戳分页无重不漏、百万合成事件流式汇总、读事务超时释放、TTL 与预算仍有效；`cargo test --offline --test history_queries` |
| H08 GUI 接线与文档 | `frontend/src/views/Settings.vue`、`Dashboard.vue`、`frontend/src/types.ts`、IPC 封装、视图快照及对应测试；`src-tauri/src/commands.rs`；`CLAUDE.md`、`docs/stats-semantics.md`、`docs/privacy.md`、`DESIGN.md` | 导入预览/确认/取消/失败重试、历史未知项目/粒度提示、重扫不清历史、来源时区选择、仅请求明细查询、快照版本失效；`pnpm --dir frontend test`、typecheck、format:check、build |

新增所有集成测试在临时目录构造来源日志、历史库、CCS 库和价格文件。真实数据只做用户授权的隔离只读核验，真实数据或数据库不入 Git。

最终验收至少覆盖：

1. 首次扫描后关闭应用，删除合成源日志，重启并查询，历史请求、四桶、模型和项目明细完全保留。
2. 清采集指纹、升级 schema、价格和模型身份规则变更后，历史 token 与请求身份不丢失。
3. 子目录日志中的独立用量正常入库；重复/重播只计一次。界面不增加代理关系页或主/子代理开关。
4. 合成旧 CCS 日汇总导入后缺失历史模型可见；项目保持未知，不能下钻为伪造请求。
5. 导入同一 CCS 库两次或改选其备份，总用量不增长；新源快照的变更只更新相应记录。
6. CCS 与原生日志重叠的完整桶和部分桶均有定向断言，部分桶的未补齐范围可见。
7. 首次同意前，定时采集、导入预览和历史查询均不打开数据文件；所有测试写入路径隔离。
8. Rust 两个 manifest 的 fmt/clippy/test、前端 typecheck/format/test/build 通过；原生 GUI 导入流程与真实 CCS 只读核对单独记录，未执行项不声明通过。

## 8 本次交付范围

本次交付为数据库和导入方案，不实施产品功能。用户已经确认数据目录、精简记录与全部代理用量范围；具体实施应以本计划的不变量、兼容 fixture 和重叠策略为准。
