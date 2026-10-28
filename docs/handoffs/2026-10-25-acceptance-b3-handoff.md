# 剩余验收 B3 会话交接（2026-10-25）

> 当前停点：B3 二轮修订稿已推送，尚未实施；最新复核又提出四项必须写清的约束。AC07 推荐 D，但尚未收到用户明确选择 D 或开始实施的指令。本文件是事实快照与复核交接，不构成实施或合并授权。

## 1. 下一会话先做什么

1. 阅读根目录 [AGENTS.md](../../AGENTS.md)、[CLAUDE.md](../../CLAUDE.md)，所有文件按 UTF-8 读写，面向用户用简体中文。
2. 阅读本交接，再读[剩余验收收口计划 §4b](../plans/active/2026-10-21-acceptance-closure.md)、[整合计划](../plans/active/2026-10-11-consolidated-remaining-work.md)与[计划总账](../plans/README.md)。
3. 执行 `git status --short`、`git log -5 --oneline`、`git fetch origin`，确认是否已有更新；不要覆盖新会话开始前其他执行方留下的修改。
4. 默认继续复核执行方的修订稿、实现或汇报，并整理可转交的执行提示词。用户最后只要求写交接文档；不要据此启动 B3、长测、安装或合并。
5. 继续推进的直接入口是本文 §4：将四项约束落实到计划，并取得用户对 AC07 形态及实施范围的明确确认。若用户在新会话已给出这些确认，直接按其授权继续，不重复询问。

## 2. 分支与版本快照

工作目录：`C:/Users/admin/Desktop/项目/tokenscope`，使用现有工作目录，不必另建 worktree。

| 项目 | 2026-10-25 写作起点 |
| --- | --- |
| 当前分支 | `docs/acceptance-b3-revision`，跟踪同名 origin 分支 |
| 修订稿起点提交 | `b11f50a4c1d1a96de1e9af18bdecc33812922ded` |
| 上一轮修订 | `b003ade` |
| origin/main | `fd39cdcc5828287c802fe2436fe77d8073eefcf0`，PR #51 合并 |
| 最新已核实发布 | `v0.6.0`，预发布；升级基线已登记 |
| B3 实施 PR | 尚未开；产品/测试代码尚未实施 |
| 本交接交付 | 在当前修订分支追加一笔纯文档提交；实际 HEAD 以 `git log` 为准 |

`b003ade` 与 `b11f50a` 只修改三个计划文档，版本检查报告 `release: false`。不要把这些文档提交当成 B3 已实现或已完成原生验证的证据。

## 3. 已交付内容与未关闭责任

### WorkBuddy 已合并发布

- [PR #49](https://github.com/ywltby/tokenscope/pull/49) 普通合并 `44c026d`，发布 [v0.6.0](https://github.com/ywltby/tokenscope/releases/tag/v0.6.0)。
- [Release 40175762245](https://github.com/ywltby/tokenscope/actions/runs/40175762245) 实际通过构建、五场景安装、八组单跳和两条连续升级链，再执行 publish；对应 main 的 CI 与 Native UI 也成功。
- [PR #50](https://github.com/ywltby/tokenscope/pull/50) 合并 `1c7c1f8`，回填执行记录并登记 v0.6.0 升级基线。
- [PR #51](https://github.com/ywltby/tokenscope/pull/51) 合并 `fd39cdc`，勘正 N17 状态、责任表、12/5 计数和样本基线口径。
- `fd39cdc` 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/40184514760)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/40184514818)、[Release](https://github.com/ywltby/tokenscope/actions/runs/40184514761) 均为首轮 success；Release 仅 gate 执行，六个下游 skipped。

当前责任口径：**12 项关闭（N01–N09、N14–N16），5 项开放（N10–N13、N17）**。

| 编号 | 尚未完成的范围 |
| --- | --- |
| N10 | B3 / AC06–AC08 隐私强取证、长观察和完整浏览器矩阵；当前仅修订稿 |
| N11 | B2 自动化已交付；安装后在设置页手动同步 OpenRouter 的人工尾项待验 |
| N12 | 发布安装/升级已有自动化证据；单实例、系统矩阵及人工签字等仍待验 |
| N13 | B4 / AC09 隔离真实规模性能尚未执行 |
| N17 | WorkBuddy 实现及自动化交付完成，仅剩用户本机安装验证，未关闭 |

N17 手动验证关注：升级首启重新询问政策、同意后四应用可筛选、同数据范围用量一致。WB08 的国内版 2,881 条、国际版 656 条及四桶合计是 **2026-10-24 当次只读样本基线**；日志增长后不要求当前总数仍固定相等。四桶合计为 input 146,055,367 / output 3,300,068 / cache_write 0 / cache_read 619,000,607。

## 4. B3 最新复核：四项尚未回填的约束

以下是对 `b11f50a` 的最新复核结论，尚未写回正式计划，也尚未据此实施。

### 4.1 自启动替身必须按构建模式隔离，不能按 bootstrap 状态决定回退

二轮稿虽然选了 A，仍写着 `bootstrapped() == false` 时走原插件。这与“隔离失败绝不回退”存在缺口。

应明确：

- **普通构建**保持现有真实插件行为，不读取新增验收变量、不产生替身文件或探针。
- **acceptance 构建**的 `autostart_status` / `autostart_set` 必须取得有效隔离根，操作 `<root>/autostart-state.json`；未初始化、根缺失、根非法或替身读写失败均报错，绝不能进入真实插件。
- 既有隐私闸门仍在命令体入口；替身是独立的第二道防线，保证闸门发生回归时也只影响隔离文件。
- `tokenscope::acceptance::bootstrapped()` 只表示初始化是否完成，不能代替有效 `root()` 校验；核心库存在“已初始化但 root 为 None”的状态。
- 变异验证中不能真的尝试回退真实插件。使用会立即失败的哨兵替身/调用计数证明没有进入真实后端，禁止为证明隔离而操作用户真实注册表。
- 全蜂巢只读快照只属于辅助证据，不是隔离措施；真实 Windows 自启出现/移除仍由 D5 3.4 人工验收。

### 4.2 AC07 推荐 D，但要补齐其完整阶段与时序语义

最新建议是选择 **D：本地真实周期长测，约 2 小时 15 分钟**，一次完成原定进程内恢复目标。用户尚未明确确认 D。

若获确认，D 至少应明确以下阶段：

1. 未同意状态观察 **≥130 秒**；线程启动/轮询/请求探针均无事件，价格桩命中与外连尝试为零，业务文件不变。
2. 同一进程内同意，保存同意先于业务解锁和线程启动；取得探针正向控制。
3. 首次真实同步：首个请求距业务解锁 **≥120 秒**，双源同步成功且快照更新。**130 秒是观察窗口，不能误写成请求必须晚于 130 秒。**
4. 通过真实设置操作关闭自动同步；仅在隔离根内把快照改成足够陈旧。以实际轮询探针证明该轮已检查、开关为关、快照满足同步年龄条件、没有请求。
5. 同一进程内重新开启，等下一次真实轮询完成双源同步。整个关闭/重开过程不能靠重启替代；证据关联同一宿主 PID 与 trace 序列。

原稿把未同意 130 秒阶段只放在 C 的 Run 2；若选 D，需要显式补回。使用实际探针界定阶段和有限截止时间，保持 120/3600 秒产品周期，不添加等待补偿、不改产品时序；长测不进 CI。

若用户选择 C，则只证明首查、关闭配置及重启后恢复，进程内恢复保留为明确待验责任，不能据此关闭原目标。

### 4.3 探针字段要区分快照过期与本轮是否应同步

当前生产函数 `price_sync_due()` 包含 `auto` 与 Ready 判定。因此不能同时把它的返回值命名为 `due`，又断言 `auto=false && due=true`。

建议记录并清楚定义：

- `phase`；
- `age_hours` 或等价的新鲜度依据；
- `snapshot_due`：快照缺失/年龄未知或年龄达到现有 24 小时阈值；
- `auto`；
- `will_sync`：按现有 `price_sync_due()` 得出的最终决策。

关闭态有效证据应为 Ready、`snapshot_due=true`、`auto=false`、`will_sync=false`，并有实际轮询事件。探针只能记录真实值，不能为满足断言改变生产判据。

### 4.4 接线位置需要校正

二轮稿写的 `src-tauri/src/acceptance_trace.rs` 不存在，不能按此另起一套追踪体系。

| 用途 | 现有位置 |
| --- | --- |
| 启动隔离根、bootstrapped/root、对外 record 接口 | [核心 acceptance.rs](../../crates/tokenscope-core/src/acceptance.rs) |
| 追踪写入、顺序、逐行 flush | [核心 acceptance_trace.rs](../../crates/tokenscope-core/src/acceptance_trace.rs) |
| 桌面壳验收启动与 WebView 配置 | [壳 acceptance.rs](../../src-tauri/src/acceptance.rs) |
| 自启命令与入口隐私闸门 | [commands.rs](../../src-tauri/src/commands.rs) |
| 价格同步线程与真实 120/3600 秒周期 | [lib.rs](../../src-tauri/src/lib.rs) |
| 业务阶段、受保护命令清单、最终价格同步判据 | [privacy.rs](../../src-tauri/src/privacy.rs) |
| 追踪事件及路径事件白名单、校验、文件快照 | [acceptance-io.mjs](../../frontend/scripts/lib/acceptance-io.mjs) |

新事件包括自启读写、价格线程启动及轮询；必须同时核对追踪生产方、校验白名单和判定侧。壳内调用的是 `tokenscope::acceptance`；不要照抄稿中的 `crate::acceptance::bootstrapped()`，壳模块没有该函数。

## 5. 已保留的 B3 方向与实施边界

- AC06 的四类场景：预置业务文件未同意观察、全部受保护 IPC 拒绝、同意原子落盘顺序与双击单次初始化、损坏 TOML 与关闭预设变体。保留已有场景与断言。
- `PROTECTED_COMMANDS` 从当前源码提取，不把现有 35 条当永久固定值；每个命令使用合法最小参数，避免把参数错误误当隐私拒绝。
- 预置/新增文件统一快照检查，覆盖 history.db、WAL、SHM 等，复用现有 `snapshotBusinessFiles` / `changedBusinessFiles` 的能力；只比较旧文件清单不足以发现新增文件。
- 红绿既要有判定函数反例，也要有真实采集链路：例如观察期内在隔离根追加一字节，证明快照采集→比对→判定会失败。不要用改动无关正则冒充变异验证。
- AC08 保持默认巡检与全部具名契约分开运行；两个隐私 fixture 不强塞进要求业务骨架的巡检。
- AC08 从 `interaction-contracts.mjs` 的 `ctx.open` 记录实际打开的 fixture，再生成覆盖表。不能用源码引用、required 名单或伪造汇总数据冒充实际覆盖。当前 15 个默认巡检、23 个注册 fixture、28 条契约均应在实施时重新核对。
- AC06 的约 3.5 分钟仅是估计；固定代码与产物各场景本地连续三次通过，实测满足新增 CI ≤8 分钟后才接入。45 分钟作业上限与既有步骤保持。
- 方案 A 和价格探针涉及产品输入，即使只存在于 acceptance cfg，也要在实施 PR 递增一次 patch；若 main 仍为 0.6.0，预期 0.6.1。同一 PR 补修不重复取号。
- 实施前重新 fetch，核对上一发布基线；v0.6.0 已登记，不要重复登记或覆盖原资产。最终 `--check --base-ref origin/main` 应符合实际产品变更。
- 最终 PR 的 gate、native-ui、Install test 等必需检查全绿后交用户复核，合并由用户决定。PR 的连续升级链按现有规则跳过，合并后 Release 才实际验证。

## 6. 本地证据与已完成复核

以下目录位于忽略的 `qa-artifacts/`，不入库，换机器后不保证存在；不要把它们当成可在远端自动获取的仓库文件。

| 本地路径 | 内容 |
| --- | --- |
| `qa-artifacts/workbuddy/review-49-codex-current/` | 早期解析器非法缓存/时间戳、同意闸门漏报及原生清单探针 |
| `qa-artifacts/workbuddy/review-49-round2/` | 七模式闸门探针，曾复现 WAL/SHM 漏报；解析器反例转绿 |
| `qa-artifacts/workbuddy/review-49-round3/` | 三文件修复后七模式符合预期，契约 69 项通过的相关复核现场 |
| `qa-artifacts/workbuddy/` | 执行方真实对账汇总及原生三连证据，具体以文件元数据和 evidence 为准 |

WorkBuddy 的旧阻断已关闭，不重复立项：非法缓存值入账、u64 时间戳回绕、空 hashtable 快照误报、失败阶段保存 pass=true、WAL/SHM 漏检等均已在 PR #49 修复并通过后续发布。B3 二轮稿只经文档验证，不能借用 WorkBuddy 的通过记录宣称 B3 通过。

## 7. 共同红线与后续批次

- 不删改现有测试、不放宽断言、阈值、CSP、必需检查、ruleset 或升级基线选择规则；先红后绿，失败看日志、`evidence.json` 和完整 `launches[]`，重跑通过不等于归因成立。
- 本地不运行安装/卸载/真实升级脚本，不写真实用户数据目录或真实注册表。原生场景只用隔离来源、本地价格桩与计数代理；外连尝试为零，真实 `.tokenscope` 前后指纹一致。
- 任一隔离根外追踪、外连或真实数据指纹变化，立即停止原生运行并报告，不绕过。
- Rust 测试经 `python -B scripts/run_rust_tests.py -- cargo test ...`；`SummaryOptions` 的 cache、pricing index 与 history 必须隔离，四个应用默认根也要核对，防止新增应用绕过旧密闭夹具。
- 一个提交解决一类问题，Conventional Commits 且中文正文；禁止 `--no-verify`，提交与推送串行；不在代码、提交或文档标注生成来源。
- B4 / AC09 可独立排期：只新增隔离的 `perf_history_real.rs`，只读真实 Claude/Codex 来源，输出全部临时化；旧 `perf_real_data.rs` 是真实缓存预热工具，不改不删，不引用旧工具数字作为 N13 结论。
- B5：AC10 单实例自动化；AC11 到具体 Release 资产哈希的人工签字是必须停点；AC12 在实际结果回填后收口。N11、N17 的人工尾项保持，不能自动代签。
- 本次未扩展到根 README 的历史文案清理；其中仍有“两来源”、旧版本与旧发布规则表述。涉及当前行为时以代码、CLAUDE.md 和现行计划为准，该文档债可后续单独处理。

## 8. 新会话可直接使用的开场

> 请先读取 `docs/handoffs/2026-10-25-acceptance-b3-handoff.md`，继续复核 B3 修订稿。起点是 `docs/acceptance-b3-revision` 的 `b11f50a` 及其后的交接提交，main 基线为 `fd39cdc`。重点核对交接 §4 的四项尚未回填约束。AC07 的 D 是复核建议，尚不能当成用户已确认；先结合我在本会话给出的最新决定继续。没有实施授权时保持计划复核范围，不运行原生或注册表操作，不合并。
