# Claude 跨会话复制请求去重与旧库纠错计划

> 已获批准并实施（2026-11-07）。执行记录见 §5：C01–C05 的落点、验证与未覆盖边界。
> 本计划不授权修改用户真实数据库、自动合并或发布；真实历史库始终未被写入。

**Goal:** 同一次 Claude 模型调用被复制到不同会话时只计一次，分支后的新调用照常计入，并安全纠正有充分证据的旧重复事实。

**Architecture:** 保留 `(session_id, message.id)` 作为会话内身份；额外保留 Claude 原始请求与转写标识作为跨会话复制证据。在内存去重与历史写入层使用同一套判定，旧库通过正常采集补证据后事务合并，不靠 token 相同猜测请求身份。

**Tech Stack:** Rust、serde、SQLite/rusqlite、现有合成 fixture 与隔离测试入口。

## 1. 已确认的问题与范围

2026-11-07 只读复核：按每个会话/消息取最终记录后，174 组跨会话记录的 message.id、requestId、消息 UUID、时间、模型与四桶完全一致；历史库保存为 348 条。可确认的额外用量为 input 384、output 144649、cache_write 412613、cache_read 40493949，合计 41051595 token。原始日志没有入库或复制为 fixture；数字只描述该次快照，不作为未来固定验收值。

当前 `dedupe.rs` 与 `UsageEvent::native_identity` 都包含会话 ID；`source/claude.rs` 尚未保留顶层 requestId/uuid。跨会话副本因此各自成为事实。不能把所有副本归因于某一次 `/fork`，也不能由单条 token 正确推导总量正确。

本批仅修 Claude：不改 Codex/WorkBuddy 身份、不做 `/btw` 补采、不修改 CCS 日汇总三向选择、不重写计价算法、不引入新桌面验收框架。

## 2. 不变量与身份规则

1. 新调用即使时间、模型、token 完全相同，也不能仅因此合并。
2. 原来的会话内消息身份继续保留，用于流式终值、CCS 请求别名和旧数据兼容；不能简单删除 session_id。
3. 跨会话合并的第一版采用严格复制证据：同应用且非空 `message.id + requestId + uuid` 一致，并有相同模型、精确事件时间与四桶的观察记录作为相互印证。它标识一条被复制的原始观察，不宣称 requestId 或 message.id 单独全球唯一。
4. 新字段缺失、空白、非字符串时不破坏旧日志解析，退回原会话内身份；同标识出现矛盾观察时不强行合并，保留事实并给可定位的诊断。
5. 同一消息可有多条流式观察，各观察 UUID 可能不同；不能把每个 UUID 单独计成新请求。先保持会话内消息归并，再利用共同复制观察连接请求事实，连接后的终值沿用现有确定性规则，绝不逐字段取最大拼造用量。
6. 必须检查整个合并组的证据冲突，不能因 A→B、B→C 的桥接盲目把矛盾的 A/C 合并。
7. 复制后的新增调用使用新的请求身份，必须额外计入。重复采集、根移动、重启、全量/增量顺序变化均幂等。
8. 一次请求只贡献一次 token、请求数与费用；来源会话关联均保留。展示项目/会话采用固定代表规则，不随发现顺序翻转，不编造“原始父会话”。代表选择与现有项目归属关系需明确成测。
9. 缺少原始证据的旧数据保留原值。不能只按历史库中相同 message.id、时间或 token 批量去重，不能要求用户清库。

## 3. 实施批次

### C01：合成反例与证据承载

文件：`crates/tokenscope-core/src/source/claude.rs`、`src/model.rs`（均相对核心包）；新增 `crates/tokenscope-core/tests/claude_cross_session_dedupe.rs`。

先写两会话复制同一请求的最小 fixture：四桶 10/2/3/4，旧实现得到两请求/38 token，预期一请求/19 token。再加分支新请求，预期计入第二个真实调用。确认旧实现断言失败后才改解析器。

以可选的 Claude 专用证据结构承载 requestId/uuid，不复用 record_id 装多种含义，不保存对话正文。覆盖缺字段、非法类型、空值与相同 token 不同身份的负例。

### C02：统一内存与历史身份判定

文件：`crates/tokenscope-core/src/dedupe.rs`、`src/model.rs`、`src/report.rs`、`src/history.rs`。

先测：跨文件/跨根、顺序反转、先副本后原文件、单文件增量、流式多个 UUID 的共同观察、标识碰撞，以及三会话桥接冲突。再实现共享判定，避免内存去重和数据库各写一套口径。

补证据不能仅发生在“用量发生变化”分支；内容未变的重观察也应补齐关联。内存去重丢弃副本展示项时，不得连同该副本会话别名和来源证据一起丢弃。

### C03：旧历史事实合并与升级

文件：`crates/tokenscope-core/src/history.rs`、`src/report.rs`、`src/cache.rs`；新增 `crates/tokenscope-core/tests/claude_cross_session_migration.rs`。

旧库没有 requestId/uuid，schema 迁移只负责准备存储，不直接猜测合并。优先复用身份别名与来源表；若缺少表达“观察证据与冲突校验”的存储，增加最小专用表并按现有机制升级 schema、创建可用备份。

提高实际需要的解析修订，并处理缓存中的新证据字段，保证未变化文件下一次正常采集也会重新解析补证据，不能只 bump 一个常量却继续读缺字段缓存。不会自动读取停用或不可用的来源。

当可靠证据连接多个已有事实时，在同一写事务内：

1. 检查完整合并组及来源优先级，选择确定性代表与终值。
2. 将所有旧会话消息别名、CCS 请求别名、native/legacy/CCS 来源关联转移到代表；同一新旧别名再次出现均命中代表。
3. 在关联完整迁移后移除重复的事实贡献；不得删除来源文件或清空历史库。
4. 更新 generation、统计缓存和写入摘要；不能把“合并掉一条旧重复”伪报成新增消费。
5. 失败整批回滚；纠错前复用/建立可恢复备份，即使最后不需要 schema 变化，也不能省略事实纠错的恢复保障。

合并发生在后续采集阶段的，应明确备份与本次事务的对应关系；不要把较早的 schema 备份冒充涵盖后续新增数据的纠错前备份。复用既有备份机制，不另造自动清理系统。

没有足够证据的旧行不动；不承诺修复来源已被清理的全部重复。

### C04：查询与兼容回归

文件：上述新增测试、`crates/tokenscope-core/tests/history_overlap.rs`、相关查询/项目分组测试。

至少覆盖：

| 类别 | 验收 |
| --- | --- |
| 已存双份 → 补证据 | 合成旧库两事实合一，四桶/请求数/费用只贡献一次，别名与来源不丢 |
| 一侧来源已删除 | 无既存跨会话证据不猜合并；已证实合并后删除来源不让副本复活 |
| CCS 前后顺序 | native→CCS、CCS→native、合并后再导入旧会话身份均不重复 |
| 流式与新增调用 | 同请求终值正确、复制不重复、新请求继续增加 |
| 旧查询快照 | 旧快照维持冻结，新快照读到纠错结果，不出现半次合并 |
| 查询一致性 | 普通汇总、冻结汇总、明细分页、按项目/模型/日期统计一致 |
| 故障与并发 | 中途失败无半迁移，串行写入边界有效，重启/重扫重复执行幂等 |
| 其他应用 | Codex、两版 WorkBuddy 既有结果不变 |
| CCS 日汇总 | 保留 PR #56 的 A/B/C 选择与补历史行为，不以其掩盖重复事实 |

不需要为此重跑真实用户数据压测或改安装器。新增测试使用独立临时目录，SummaryOptions 的历史库、缓存、价格索引均显式隔离。

### C05：文档、门禁与交付

同步 `docs/stats-semantics.md` 的 Claude 身份、跨来源别名、旧库纠错与项目代表规则，更新计划总账入口。若调整持久格式，更新对应格式指纹/迁移 fixture 与构建清单。待用户确认实施后才分配版本；跟随最新 main 和 PR #56 实际合并状态取号，不提前固定 patch 数字。

定向验证（从仓库根执行）：

```powershell
python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test claude_cross_session_dedupe
python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test claude_cross_session_migration
python -B scripts/run_rust_tests.py -- cargo test --locked -p tokenscope --test history_overlap
```

每个实现批次先确认反例因目标缺陷而红，再确认转绿；随后运行相关普通/acceptance 测试、fmt/clippy、实际提交钩子和 PR 必需检查。不绕过门禁、不调整 CPU 亲和性或进程可见性凑绿；环境阻断先给证据。

修复分支独立于未合并的 CCS PR；如 PR #56 尚未合并，在整合后的树上补兼容验证，不混淆两项修复。测试绿不等于安装版已更新；最终交付报告列出受测 HEAD、旧库纠错边界和升级行为，合并/发布另行授权。

## 4. 完成判据与停止条件

- 合成原始记录及旧库都证明：副本一次、新调用额外一次、相同 token 的不同调用不误合并。
- 现有别名和来源完整保留，重复采集/导入/重启不复活重复，费用与用量同源。
- 不把本机 174 组及 41051595 写成通用或未来固定值；实施阶段先在隔离样本验收。真实历史库不得作为开发试验对象。
- 仅新增标识缺失且证据无法补齐的边界，不扩展到 `/btw`、Codex fork 或全部代理的身份重设计。
- 如发现现有数据无法支持拟定的严格证据判定，回到本计划修订，不放宽为“message.id 一样就合并”来求通过。

## 5. 执行记录（2026-11-07）

基线：分支自最新 `main`（`32bf7bc`）起，独立修复分支
`fix/claude-cross-session-dedupe`；未创建 worktree，未混入 CCS PR #56
（当时 OPEN）的改动，未改写已有提交。

### C01 证据承载与合成反例（已实施）

- `source/claude.rs`：读取顶层 `requestId` 与 `uuid`（只接受非空字符串；
  缺失/空白/类型异常 → 无证据，不计坏行）。`model.rs` 新增
  `ClaudeCopyEvidence`、`UsageEvent::claude_copy`、`copy_identity()`、
  `observation_digest()`（时间 + 模型 + 四桶 + 分档，不含会话与路径）。
- 先红：`tests/claude_cross_session_dedupe.rs` 的两会话复制用例在旧实现上
  得 3 条请求（期望 2），四桶 38+6（期望 19+6）；修复后转绿。
- 负例：同 token 不同身份不合并、缺 `requestId`/`uuid` 不合并、同标识不同
  四桶不合并；解析层另有空白/`null`/非字符串/对象四类非法类型用例。

### C02 内存与历史共用身份判定（已实施）

- `dedupe.rs`：先按既有规则做会话内终值收敛，再按 `copy_identity` 合并
  跨会话副本；代表取会话 ID 字典序最小（其次来源路径、行序），与文件发现
  顺序无关。副本不丢弃证据——`DedupeResult.copies` 把副本事件带出，写入层
  （`report::append_copy_evidence`）把副本的会话内身份、复制身份与来源位置
  一并登记到保留事件。
- `history.rs`：`lookup_hits` 返回全部命中及来源（自身位置 / 复制证据）；
  仅由复制证据命中的写入只补证据、不改代表；命中多条事实时在同一事务内按
  固定代表规则合并（`merge_facts_in_tx`：先迁别名与来源，再删重复行）。
- 写入排序改为 `(ts, session_id, source_path, line)`：库内代表与内存代表
  共用同一固定规则。

### C03 旧库纠错与升级（已实施）

- 无新增表/列：复用 `event_aliases`（新 `claude-copy` 方案）与
  `event_origins`，`HISTORY_SCHEMA_VERSION` 保持 5，持久格式指纹不变。
- `PARSE_RULES_REVISION` 5 → 6：未变化的来源文件在下次正常采集重解析一次；
  实测（`unchanged_sources_are_reparsed_to_supply_evidence`）文件字节与
  指纹不变时 `observations` 仍 +1，随后合并为一条。
- 纠错前备份：`history.db.correction-backup-<UTC>`（`VACUUM INTO`），与升级
  备份分开命名；无备份不允许合并，事务失败整批回滚（含已完成的合并）。
- 未变内容的重观察也补来源证据；`WriteSummary.merged` 单独计数，不伪报新增。

### C04 查询与兼容（已实施）

- `tests/claude_cross_session_migration.rs`：旧库双份事实合一（别名/来源/
  备份齐全）、来源删除与放回不复活、纠错失败整批回滚、CCS 前后顺序与合并后
  重导、流式观察、三会话桥接冲突、旧查询快照冻结与新快照一致、项目分组代表。
- 既有 Codex / 两版 WorkBuddy / CCS 三向选择用例保持通过（全量测试见 §5.5）。

### C05 文档与交付（已实施）

- `docs/stats-semantics.md`：§3.1（跨会话复制与代表规则）、§3.3.1
  （`claude-copy` 身份）、§6（纠错、备份、证据边界）、新增 §6.5.4。
- 版本：patch 递增（`0.7.1` → `0.7.2`），并登记上一版 `v0.7.1` 升级基线。
- 持久格式未变，无需迁移 fixture 或格式指纹更新；构建输入清单无新增非 Rust
  输入。

### 验证结果与本地限制（2026-11-07）

- 定向：`claude_cross_session_dedupe` 4/4、`claude_cross_session_migration` 8/8、
  `history_overlap` 12/12；`cargo test -p tokenscope --lib source::claude` 21/21；
  去重单测（`dedupe::tests`）全部通过。
- 全量：`cargo test --locked --workspace --no-fail-fast` 共 66 个测试目标通过；
  唯一失败是 `source_discovery` 的两条既有用例（`std::os::windows::fs::symlink_dir`
  返回 OS 1314「客户端没有所需的特权」）——本进程未提权、`mklink /D` 同样失败，
  属本机环境限制，与本次改动无关（干净 HEAD 复现同一错误）。
  `history_storage::concurrent_writers_do_not_duplicate_events` 在本机出现一次
  `SQLITE_READONLY` 竞态、重跑通过，同样在干净 HEAD 复现，非本次引入。
- 门禁：`cargo fmt --all -- --check` 干净；`cargo clippy --workspace --all-targets
  -- -D warnings` 干净；未改任何非 Rust 构建输入（`scripts/build-inputs.json`
  不变）。因上述符号链接权限限制，本机**未跑完**提交钩子的 full 模式（它会跑
  两轮全量 workspace 测试），改由用户在本机决定提交方式；PR CI 不受影响。
- 版本：0.7.1 → 0.7.2（patch）；`upgrade_baselines.py add --tag v0.7.1` 已登记，
  `select --current-version 0.7.2` 给出 6 个起点（0.1.0/0.4.0/0.5.9/0.5.13/
  0.6.0/0.7.1），未超上限。

### 未覆盖 / 证据不足

- 只抄到流式中间观察（终值不同的另一条 `uuid`）的副本、以及来源已被删除的
  一侧：没有共同终值证据，按不同事实保留（不猜）。
- 不扩展到 `/btw` 补采、Codex fork、WorkBuddy 身份或 CCS 日汇总三向选择。
- 本机 174 组 / 41,051,595 token 只是 2026-11-07 的只读快照，不是验收阈值；
  验收全部使用合成数据与隔离临时目录。
- 代码修复不等于用户安装版已更新：需升级到含本修复的版本，并在正常采集
  触发后才会重解析补证据。