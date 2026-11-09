# Codex 正常请求身份与漏计修复实施计划

> 执行者使用 executing-plans，按 K01–K05 逐项实施并记录红绿证据。
> 状态：**已实施（2026-11-08），执行记录见 §5**。2026-11-08 用户决定：
> Codex `/btw` 只登记 TODO，暂不实现；先修可从会话日志取得用量的普通请求。
> 前置说明：Claude 跨会话修复当时仍停留在暂存区未提交，用户明确指示
> 不等待、直接实施；本任务只改自己的文件集合，未覆盖其暂存内容。本计划
> 不授权自动合并或发布。

**Goal:** 不同 Codex 模型响应即使用量完全相同也分别计入，同一响应的重复记录与 fork 副本只计一次，并在有可靠来源证据时补回旧库误合并的请求。

**Architecture:** 在现有 JSONL 适配器中保留单响应身份，用同一身份规则贯穿解析、全局去重、缓存和历史合并。新格式的响应身份与旧格式的弱重播指纹分开；不引入遥测服务，不重做采集框架，不靠累计用量差分猜请求。

**Tech Stack:** Rust、serde、SQLite/rusqlite、现有 CodexSource / summary / 冻结查询、隔离 Rust 测试入口。

## 1. 已确认事实与范围

研究对象：真实 Codex 0.166.0，通过本机合成 Responses 接口生成真实 JSONL；TS 受测已提交源码为 `6dc054d462dbcbe1f8e2e0ddb572b2fe8cd2c0b3`，不包含另一个会话未提交的 Claude 改动。所有用量均为模拟服务预设真值，无真实模型调用或用户数据写入。

本地研究材料（gitignored，只供复核，不作为 CI 文件依赖）：

- `qa-artifacts/codex-normal-d011823e/README.md`、`run_normal.py`、`operations-result.json`、`comparison.json`、`asserted-results.json`。
- `qa-artifacts/codex-operations-1b1ad785/README.md`：clear、paginated/legacy fork、revert 的后端实跑与四模式对账。
- 两目录的 `runs/*/stages/*/source/` 为真实 Codex 写出的合成 JSONL，未手改；`codex_ops_probe.rs` 调用实际 TS 解析与历史管线。

| 已知真值 | 旧实现实计 | 修复后必须得到 |
| --- | ---: | ---: |
| 普通三轮，各自不同响应、四桶相同，各1100 | 1100 / 1请求 | 3300 / 3请求 |
| 普通三轮1100、2200、1100，身份各不同 | 3300 / 2请求 | 4400 / 3请求 |
| 一轮工具往返，两次响应各1100，turn_id相同、response_id不同 | 1100 / 1请求 | 2200 / 2请求 |
| A1100、B2200，回退B后新C1100 | 3300 / 2请求 | 4400 / 3请求 |
| legacy fork复制A1100后新B1100 | 1100 / 1请求 | 2200 / 2请求 |

正常不同用量、相同总量但四桶不同、paginated fork 的对照均正确。新日志的 `token_usage_record` 已有 response_id、thread_id、session_id、turn_id 与单次 usage；TS 解析能产出足量事件，后续弱去重让独立请求消失。当前解析未保留响应身份，legacy fork 内复制的 session_meta 还可能把子线程新请求归到父线程。

仅修 Codex JSONL 请求身份、相关持久化与必要重解析。不改 Claude/WorkBuddy 的规则，不改 CCS 日汇总三向选择、费用公式、语言/安装器；不自动读取真实日志取证，不要求用户删库或重建历史。

## 2. 必须成立的规则

1. response_id 非空且格式有效时，构造带命名空间的强身份。首选有证据的**原始响应所属线程 + response_id**（应用/身份版本另作命名空间）；不能只用裸 response_id 全局归并，也不能把文件路径、当前 fork 文件头或 token 值作为强身份。
2. token_usage_record 中的原始线程身份优先于复制历史后遗留的解析状态。thread_id/session_id 的含义须用本轮真实合成日志核对；缺失或互相矛盾时不能擅自认作同一请求。provider 有可靠记录时可纳入命名空间；不读取当前全局配置给旧记录补造 provider。
3. 同一响应的 token_usage_record 与 token_count 是两种观察，合计只产一条事实；一轮 turn 可有多个响应。禁止按 turn_id 去重，也不能把累计 thread/turn 用量当每次用量相加。
4. 不同强身份，即使模型、时间、四桶、reasoning 全相同，也必须保留。相同强身份且内容一致的副本只计一次，新增 fork 响应照常计入。
5. 相同强身份出现相互矛盾内容时须有显式冲突处理与测试，不静默以最后到达者覆盖、不逐字段取最大、不因一个冲突拖掉其他正常请求。执行时记录采用的最小规则与证据，不拓展通用身份框架。
6. 单次用量沿用当前四桶口径和受检算术：input=raw_input−cache_read−cache_write；output 不再叠 reasoning；负值/越界/异常类型不能饱和成合法数。保留 compaction、reasoning 兼容等已验证分支。
7. 没有强身份的旧日志保留既有兼容策略及其限制。不同强身份之间永远不能通过同一个弱别名再次合并；强/弱记录仅在可靠来源观察对应等证据成立时关联，不能仅凭四桶相等判同。
8. 所有采集顺序、全量/增量/重扫/重启与缓存命中结果一致；复制历史不会复活为新消费；日志删除或来源停用不删除已存事实。

## 3. 实施步骤

### K01：固化正常会话与操作反例（先红）

新增：`crates/tokenscope-core/tests/codex_request_identity.rs`，必要最小 fixture 放 `crates/tokenscope-core/tests/fixtures/` 的专用子目录。

1. 从上述合成 JSONL 提取最小结构，保留关键记录顺序、字段和两类用量记录；路径改为合成路径，不提交原始 app-server 全协议、绝对机器路径或大工件。
2. 先覆盖 §1 五个反例及正常不同用量、同总量不同桶、reasoning 子项不同但四桶相同的对照。
3. 增加同响应两类记录、重复副本、一个 turn 多个 response、非相邻相同用量等用例；断言四桶、请求数和身份，不只断言总和。
4. 经真实 CodexSource → summary/history 跑通，记录旧实现预期失败；失败必须落在漏计断言，不以 fixture 无法解析冒充红测。

命令：`python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test codex_request_identity`。

### K02：解析并承载响应身份

修改：`crates/tokenscope-core/src/source/codex.rs`、`src/model.rs`；测试：上述新测试及 Codex 解析单测。

1. 复核实际 token_usage_record、token_count、session_meta、turn_context 的先后关系。以单次 record 的 usage 与身份为依据；对应 token_count 仅作为同次兼容观察，不二次产数。配对必须有明确边界，不能“相邻就一定相同”。
2. 以最小可选字段/专用结构承载 Codex 响应身份与原始线程身份；不要借用 Claude copy 字段。若复用 record_id 保存 response_id，必须同步修改语义说明并单独保存所需命名空间，不把缺身份伪造成空强身份。
3. 测试 token_count-only、record-only、两类同时存在、重复 token_count、无关通知插入、跨 turn、截断文件下次追加、非法/空身份，以及 compaction 的零 last_usage 分支。不能依赖文件结尾或刚好一次完整读取才能计数正确。
4. 修正 legacy fork 的复制旧请求/子线程新请求归属；验证父请求副本仍保留父身份，不能把整份子文件都改挂子线程而造成复制双计。

定向执行 K01 命令及 `python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope source::codex`，逐项确认解析与归属。

### K03：统一去重与持久化身份

修改：`crates/tokenscope-core/src/dedupe.rs`、`src/model.rs`、`src/history.rs`、`src/report.rs`、`src/cache.rs`；按需修改现有 `tests/codex_parse_revision.rs`、`tests/history_overlap.rs`、`tests/source_overlap.rs`。

1. 建立单一身份构造函数，内存去重、native identity、历史分类/预览/写入/别名使用同一规则。强身份路径优先；不同强身份不进入同一个弱重播组。
2. 缓存保存并恢复新证据；内容指纹、来源观察与旧值兼容一起核对。内存去重不得丢弃历史升级所需的来源关联。
3. 测试相同 response_id 但不同原始线程、同一响应跨文件副本、两份副本先后顺序反转、根移动、先弱后强/先强后弱、不同强身份同弱指纹、同身份矛盾观察。
4. 补实际管线断言：全量、分段追加增量、强制重扫、重新打开数据库均满足 §1；正常汇总与冻结查询、明细条数、日期/项目/模型分组一致。合成价格下费用随真实请求数累加，不重复消费也不丢消费。

### K04：旧库误合并恢复（先红后修，不能仅 bump 修订）

新增：`crates/tokenscope-core/tests/codex_request_identity_migration.rs`；修改：`src/history.rs`、`src/report.rs`、`src/cache.rs` 及必要迁移 fixture。

1. 合成旧库中“多个独立响应已合成一个旧弱身份事实”的状态，保留真实旧来源/别名形态。升级读取仍存在的合成日志后，验证恢复为多条请求，不是旧一条加新三条变四条。
2. 提升实际需要的解析/缓存修订，使未变化的启用来源重解析一次；持久字段若要扩展，按现有 schema/备份机制迁移。版本数值以 Claude 稳定提交为基线决定，不覆盖它的新修订。
3. 强身份升级必须审计旧弱别名及 origins 的归属：可明确对应的旧事实升为其中一个响应，其余真实响应补入；不能让唯一弱别名把新增响应全吸回旧行，也不能把同一弱别名复制为多个强响应的合并入口。
4. 无可靠映射的旧事实保持原值，不凭数值删除；不得把同一旧事实对多个新请求分别重复“升级”。来源部分缺失、全部缺失、标识矛盾分别成测，披露无法恢复范围，不承诺全部历史纠正。
5. 在写事务内完成纠错、来源/别名更新、generation 与缓存失效；会改写旧事实的纠错复用现有可恢复备份机制，失败完整回滚，不另造备份清理框架。
6. 至少测试：旧一条→两/三条、重复重扫不增长、仅一部分来源还在、旧库无新增字段、重启幂等、失败回滚、旧查询快照冻结、Claude 已修复的复制去重与 WorkBuddy/CCS 结果不变。

命令：`python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test codex_request_identity_migration`，并复跑相关 history/Claude/CCS 定向回归。

### K05：收尾与交付验证

修改：`docs/stats-semantics.md` 的 Codex 身份、兼容与升级段；`CLAUDE.md` 中旧 Codex 弱身份概述；本计划、整合计划与 `docs/plans/README.md`。版本及 release-change、格式元数据/构建输入清单按实际产品变化更新。

1. 将上面的反例及对照全部转为可在 CI 独立运行的合成测试，不依赖本机 qa-artifacts 或已安装 Codex。
2. 必要时复用研究驱动做一次真实 Codex→本机合成接口→新 TS 的隔离验证。普通同用量3300、工具循环2200、回退4400、legacy fork2200必须全部正确；记录实际受测提交/工作树与 Codex 版本。禁止真实模型调用、读取真实会话或修改用户配置。
3. 运行 `cargo fmt --all -- --check`；clippy 普通与 acceptance 双模式；通过 `scripts/run_rust_tests.py` 跑 `cargo test --locked --workspace` 普通与 acceptance 双模式。按实际变更跑前端/脚本相关检查，并执行仓库要求的完整提交门禁。
4. 用 `python -B scripts/bump_version.py --check --base-ref origin/main`、`python -B scripts/build_inputs.py --check` 核对版本与清单。以当时最新 main/现有未发布批次取号，不硬编码本计划研究版本，不因同 PR 补修反复加号。
5. 提交只包含本任务及其必要文档。推送/PR 后核对最新 HEAD 的必需 CI，失败读日志和工件、修根因；不以旧 HEAD 绿灯代替，不绕钩子、不跳真实断言、不盲目重跑、不关闭用户实例。
6. 完成定义：K01–K04 行为验收、历史恢复/无法恢复边界与本地/CI 结果可复查，报告“已修待合并”或实际状态。默认不合并 main、不发布；人工安装/其他开放项不因本计划自动关闭。

## 4. TODO：Codex `/btw` 用量获取（用户决定暂缓）

- **状态：deferred，未排期，不属于 K01–K05 或本轮发版阻断。** 不把它写成已支持，也不把未采到的用量记为已验证0。
- 已知限度：研究版本0.166.0的临时线程不保存普通 rollout；仅完善 JSONL 解析无法恢复所有 `/btw` 用量。已有 OTLP 单响应 usage 的合成研究，但实时接收、断线丢失、父链归属、跨来源去重与隐私仍有成本。
- 本轮不实现 OTLP 接收器、常驻服务、代理抓包、真实配置修改、全局遥测或 UI 接入向导；不顺带增加每轮采集告警。
- 研究留档：`qa-artifacts/codex-btw-research-20261107-g8e157ns/README.md`。其中方案仅是候选，不构成实施决定。
- 重新启动条件：用户重新排期，或上游提供可可靠读取的最小持久用量记录。届时先验证单响应真值、主线程继承累计量排除、幂等与数据最小化，再确定最小实现；不直接沿用版本相关推测。

## 5. 执行记录

2026-11-08 实施，K01–K05 完成；受测工作树包含另一个 Claude 会话的未提交改动
（`fix/claude-cross-session-dedupe` 暂存区，Codex 身份逻辑未被其触碰），用户
明确指示不等其提交、直接实施；本任务文件与其文件是不同的路径集合，提交时
分批登记，未覆盖、未重置、未替其提交任何暂存内容。

- **K01（红→绿）**：`crates/tokenscope-core/tests/codex_request_identity.rs`
  14 用例，从 qa-artifacts 研究材料最小结构重建（合成 ID/路径，不依赖本机
  Codex 与 qa-artifacts 运行）。红测实录：普通三轮同用量 1100/1（应 3300/3）、
  非相邻 3300/2（应 4400/3）、工具循环 1100/1（应 2200/2）、回退 3300/2
  （应 4400/3）、legacy fork 1100/1（应 2200/2）、reasoning 1100/1（应
  2200/2）；失败全部落在漏计断言。record/count 时间差按真实落盘 1–3ms
  建模；配对边界（无关通知、record-only、count-only、连续响应、截断追加）
  全部通过（旧挂起兼容保留）。
- **K02**：`source/codex.rs`——`token_usage_record` 带 `thread_id +
  response_id` 时立即出数（会话 = 原始线程，`record_id` = response_id），
  配对 token_count 按最近强观察抑制；缺身份记录保持挂起兼容。`model.rs`
  新增 `CodexResponseIdentity`。
- **K03**：`dedupe.rs` 强/弱分键（强 = `(thread, response_id)`，弱保持
  `(session, model, 四桶)`）；`model.rs` 的 `native_identity`/
  `same_source_identity` 对强事件返回 `codex-response`
  （`codex|1|<thread>|<resp>`，恢复路径从 record_id 派生），强记录不携带
  弱 `codex-usage` 别名；`PARSE_RULES_REVISION` 6→7 驱动未变文件重解析。
- **K04（先红后修）**：
  `crates/tokenscope-core/tests/codex_request_identity_migration.rs` 3 用例。
  红测实录：时间差场景 4 条（应 3）、legacy fork 1 条（应 2）。实现：解析器
  向前看重建**镜像来源键**（旧解析器出数行的来源键逐字节重建，无时间窗口、
  无模糊匹配）；`history.rs` 经镜像键命中旧行后在纠错前备份 + 事务内原行
  升级（复用 id、补强别名、ts 变 record 行时刻）；升级重放多命中收敛到
  确定性代表；强响应写入不与旧模型纠正候选路径交叉。绿测：旧 1 条 → 恰好
  3 条/2 条，旧行 id 保留、会话归属纠正、无法映射的旧事实原值保留。
  `codex_parse_revision.rs` 的 v2→v7 升级用例同步改造（预置旧 count 行
  表示，断言升级后强身份与 record 行时刻）。
- **K05**：`docs/stats-semantics.md` §3.2/§3.3.1 重写（强响应身份、弱记录
  边界、升级映射与不可恢复披露）；`CLAUDE.md` 身份概述更新；本计划与
  `docs/plans/README.md`、整合计划状态更新。版本 0.7.2 递增为 0.7.3
  （产品输入变化），`release-change.json` 增补本轮摘要；
  `python -B scripts/build_inputs.py --check` 干净。
- **回归**：`cargo fmt --check` 干净；clippy 双模式 -D warnings 干净；
  `cargo test --locked --workspace -j 2` 55 组 test result 全绿，仅
  `source_discovery` 的 2 个 Windows symlink 特权用例失败——干净 HEAD
  （无本改动）上同样失败（os error 1314，本机无符号链接特权），GitHub
  runner 有特权，非本任务回归。
- **边界（如实披露）**：record/count 非零不一致对、来源已删除且无既有映射
  的旧事实不升级不合并（保持原值）；旧格式弱记录的同模型同用量合并限制
  保留（§3.2）；`/btw` 遥测按用户决定继续暂缓（§4）。

### 6. 复审修复（2026-11-09，PR #57 定向复核 88d0580 后）

独立复核在 HEAD 88d0580 复现三个影响 token 数字的缺陷与一个测试架缺陷；
先红后修，全部转为正式仓库测试（不依赖 qa-artifacts）：

- **红测实录**：多来源旧事实 1100/1（应 2200/2，`review_old_merged_fact_
  with_multiple_origins_splits`）；compaction 镜像错位 2200/2（应 1100/1，
  `review_compaction_legacy_mapping_no_extra_fact`）；悬挂强观察 1100/1
  （应 6600/3，`dangling_strong_observation_does_not_swallow_followup_
  counts`）。实现修复后 8/8 + 16/16 全绿。
- **修复 1（多来源拆分）**：`history.rs`——镜像键命中已升级为**其他响应**
  的行时，镜像来源（属于本响应：旧解析器在本响应的 count 行记账）从该行
  **转移**到本响应的新事实，旧行保持已升级表示；单命中与多命中两条路径都
  禁止把异响应行并入合并组。覆盖两/三响应、顺序反转、分批升级、refresh
  重扫、重启幂等（`review_three_origins_old_fact_splits_into_three`、
  `review_reversed_upgrade_order_and_replays_stay_exact`）。
- **修复 2（compaction 行号）**：`source/codex.rs` 的 `legacy_mirror` 零
  last 分支返回 **record 行**（旧解析器从挂起记录本体出数，来源证据记在
  record 行）；正常非零配对仍取 count 行（对照用例
  `review_normal_pair_mapping_still_uses_count_line` 钉住）。
- **修复 3（悬挂强观察）**：count 与最近强观察不一致 → 清除观察并照常出数；
  `turn_context`（新 turn 开始）终结配对窗口。不引入时间窗口/累计差分；
  相同响应 record/count 仍只计一次（`record_and_token_count_are_one_
  observation` 等既有用例钉住），不同响应同四桶分别计入。
  **同日二次更正（2026-11-09）**：同用量 count-only 的边界此前描述为
  "照常出数、可能双计"有误——合成回归实测为**两层漏计**（解析层抑制 +
  写库镜像 origin 吸收，机理见 stats-semantics §3.2"同用量配对的不可区分
  边界"）。本轮不改算法（"全部保留"会把漏计换成正常配对双计），以**边界
  行为测试**钉住两层抑制与两个对照（`same_usage_count_only_after_strong_
  record_without_boundary_is_suppressed`、`same_usage_count_only_absorbed_
  by_mirror_origin_despite_boundary`、`different_usage_count_only_after_
  new_turn_boundary_counts`、`normal_same_usage_pair_still_counts_once`），
  并删除"正常落盘节奏不会走到这里"的无证据保证。
- **修复 4（symlink 探针）**：`tests/source_discovery.rs`——只把确证的
  os error 1314 归为缺权限，其他错误如实 panic；CI（`CI` 已设置）无特权
  直接失败（runner 必须真实构造 junction），本地无特权显式跳过并打印
  `SYMLINK-TESTS-SKIPPED:` 可识别标记；探针只创建/清理本次自建且确认归属
  的路径，清理失败如实报告。实测：本地 28/28（2 例显式跳过带标记），
  模拟 `CI=1` 无特权 → 测试 FAILED（不再冒充通过）。
  **同日二次收窄（2026-11-09）**：判定此前把 `ErrorKind::PermissionDenied`
  （如普通访问拒绝，原始码 5）也归为缺特权——与"只识别确证 1314"的口径
  不符。修正：抽取纯判定函数 `symlink_exemption_decision(错误, CI 形态)`，
  **仅原始码 1314** 进入本地豁免；探针拆为 `windows_symlink_probe_result`，
  创建失败保留原始创建错误为主、清理错误附加打印不被覆盖。最小测试 5 例
  （1314 本地豁免、原始码 5 失败、其他错误失败、CI 下 1314 失败、原始创建
  错误不被清理覆盖），全部注入驱动，不改系统环境。
- **同刻升级例外**：compaction 旧行与新事件同为 record 行时刻，旧代码落
  入同刻键比较（字典序随机胜负）→ 随机丢弃升级。修正：镜像例外在**同刻**
  也强制 Update（`classify_against` 提取 `codex_upgrade_forced`）。
- **回归**：`cargo fmt --check` 干净；clippy -D warnings 干净；workspace
  69 组 test result 全绿（含 Claude 跨会话 12 例、WorkBuddy、CCS、
  codex_parse_revision 升级用例）；build-inputs 清单 --check 干净。本机
  `cargo test --no-run` 全并行仍受页面文件限制（os 1455），CI 不受影响。
- 已知限制（登记不扩修）：`nick-fields/retry` 包裹完整 tauri build，按
  复审指出会重试任意构建失败而非仅 NSIS 下载 504——收窄到传输失败分类
  需要更完整的日志解析，本轮不扩展；502/503/504 脚本层重试已按根因修复。

### 7. 同用量 count-only 跨 turn 误合并修复（2026-11-09，PR #57 追加）

本轮只修"旧解析映射跨越新 turn，导致独立同用量请求被误合并"，不扩展其他
任务；`/btw` 继续暂缓。

- **红测实录取证**：把 `same_usage_count_only_absorbed_by_mirror_origin_
  despite_boundary` 的预期从 1100/1 改为 2200/2 后，在当前实现上失败于
  数字断言：`requests left: 1, right: 2`。解析层已正确产出 A、B 两条事件，
  问题在写库层。
- **根因（准确位置）**：强记录 A 的镜像来源键 = 旧解析器记账的 count 行；
  新 turn 后的独立同用量 count-only B 的事件键与它逐字节相同。`lookup_hits`
  把该镜像 origin（以及旧行 `event_key`、升级后残留的 `codex-usage` 弱别名）
  当作 B 的身份，命中已升级的 A 行；`classify_against` 按 Codex"更早胜出"
  判 Stale，B 被丢弃。旧解析器在该行记账只证明"A 的旧表示在哪"，不证明
  "B 与 A 同属一次调用"。
- **修复条件**：
  1. 镜像来源键只作**首次升级**的定位证据：本响应已有强身份（来源键/强
     别名）命中时不再查镜像；插入时也不重复登记镜像 origin。
  2. Codex 弱写（无 response_id）不得命中强事实——来源位置/弱别名重合
     不构成请求同一性；旧事实升级时行键改为当前 record 行，旧弱别名与
     来源由该位置的当前弱事实接手，旧事实保留强身份。
  3. 解析器的 turn 配对边界与写库身份空间保持一致：新 turn 边界后的
     同用量 count-only 是独立请求；无边界且无其他身份证据的歧义仍按
     配对观察抑制，不引入时间窗口/累计差分。
- **新增/更新回归**（合成日志 + 隔离数据库）：
  - `same_usage_count_only_absorbed_by_mirror_origin_despite_boundary`
    改为 2200/2（重启一致）；
  - `same_usage_count_only_after_boundary_with_existing_unrelated_fact`
    （已有无关事实库：额外贡献 2200/2，总计 3300/3）；
  - `same_usage_count_only_after_boundary_batched_and_rescanned_stays_exact`
    （先 A 后追加边界+B；重扫、重启均 2200/2）；
  - `migration_same_usage_count_only_after_new_turn_restores_two_requests`
    （旧库 1 条弱事实按镜像升级为 A，B 独立计入；2200/2，旧行 id 保留，
    重扫/重启不增不减）；
  - 既有多来源拆分、compaction 映射、legacy fork 复制去重与子请求计入、
    正常 record/count 配对 1100/1、同一 turn 两个不同强响应 2200/2 均
    保持通过；无边界抑制用例保留为诚实的边界说明。
- **门禁**：`cargo fmt --all -- --check`、clippy `-D warnings`、全量
  `cargo test --locked --workspace -j 2`（经 `scripts/run_rust_tests.py`）
  通过；未新增持久格式/构建输入，不重复递增同一未发布批次版本（仍 0.7.2）。
- **文档更正**：撤回 `stats-semantics` §3.2"该已分界场景只能等上游增加
  response_id"的结论；保留真正缺证据（无新 turn 边界）的限制说明。历史
  汇报不改写，本节为追加更正。
