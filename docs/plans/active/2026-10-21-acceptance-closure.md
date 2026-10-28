# 剩余验收收口：来源权限、原生统一验收、隐私强取证、真实性能与系统签字

> 状态：**用户 2026-10-21 确认，按 B1–B5 连续执行**。承接[整合计划](2026-10-11-consolidated-remaining-work.md)全部剩余开放项：N14 第 4 项、N11、N10、N13、N12。
> 进度（2026-10-25，范围调整）：**用户授权 B3 分批实施——本批获准实施 AC06（四个隐私原生场景、自启隔离替身、回归与反例验证，以及满足预算后的 CI 接入）与 AC08（实际 fixture 覆盖记录、覆盖汇总与回归验证、本地完整浏览器矩阵取证）；AC07 整项暂缓（C/D 均不在本批执行，不新增其专用价格线程探针，不改变产品同步周期），进程内恢复目标继续保留 N10 待验。** 实施依据 a75de15 及后续最新版本的收口计划；AC06 含产品输入，实施 PR 递增 patch（若 main 仍为 0.6.0 则预期 0.6.1）。
> 进度（2026-10-22）：**B1 / AC01 已交付，N14 关闭；B2 / AC02–AC05 随 PR #31（`c9de40e`）交付，main 三条运行全部成功，v0.5.9 已公开并登记原资产基线**。N11 的安装后人工同步尾项按用户决定保留待验；**B3 修订稿已拟并经三轮修订（2026-10-25，§4b；AC06 选 A、AC07 推荐 D 待用户确认、实施授权未给，未实施）**，B4、B5 未执行，不提前关闭 N10–N13。
> 进度（2026-10-25，本批实施完成）：**按上方范围调整授权，AC06 与 AC08 已实施完毕（提交 `0191f78` / `17fb37e` / `af784bf`，版本递增 0.6.1；[PR #52](https://github.com/ywltby/tokenscope/pull/52) 首跑暴露的 0.6.0 升级跳装配缺口已随 `b9b2480` 修复，修复后全部必需检查通过，待用户复核合并）；AC07 未实施，价格线程探针未添加，产品同步周期未改，进程内恢复继续保留 N10 待验。** 执行记录见 §6「B3 分批实施」。
> 进度（2026-10-26，PR #52 复盘修复）：**用户六项复核意见全部修复（提交 `8530b49` / `8355117` / `a17185d` / `1737a6a` / `c5738a6` 等，逐项红先转绿），并完成自启命令层路由补证与三项证据勘误（超时根因未定位如实记录、AC08 旧证据提交词勘误、覆盖口径勘误）；修后在固定受测树 `c5738a6` 重新取证——AC06 三连 15/15（45s）、AC08 全矩阵 23/23。AC07 仍整项暂缓，N10 保持开放。** 执行记录见 §6「PR #52 复盘修复」。
> 本计划只排验收与取证所需的改动。验收中暴露的产品缺陷按缺陷流程单独修复，不在本计划里预先设计修法。
> 排期补记（2026-10-24）：用户确认 WorkBuddy 专项按 N17 独立推进，本计划 B3–B5 不再作为它的启动前置；本计划原待验范围继续保留，完成本计划也不代替 N17 的实现与验收。

**Goal：** 收口本计划承接的 N14 第 4 项、N11、N10、N13、N12；其中 N14 已随 B1 关闭，其余按后续批次验收。新增 N17 由 WorkBuddy 独立专项承接，跨午夜、长时间休眠、跨显示器仍保留在独立系统待办（用户 2026-10-13 决定不排期）。

**Architecture：**
- 先补一层**只在 acceptance 构建存在**的验收接线：CCS 默认路径隔离、价格源地址可指向本地桩、打开文件改为记录、按时间顺序的 I/O 追踪日志。普通构建没有任何新的环境变量通道。
- 原生驱动 `frontend/scripts/native-acceptance.mjs` 增加本地价格桩、计数代理和三个统一验收场景，确定性场景进入 CI 的 `native-ui`；长时间等待和全量矩阵只在本地取证一次。
- 隐私、性能与系统签字复用同一套接线与证据格式（`evidence.json`、`launches[]`、具名断言）。

**Tech Stack：** Rust（`tokenscope` 核心库、`tokenscope-app` 壳，`acceptance` feature）；Node + playwright-core（CDP 驱动真实 WebView2）；PowerShell 隔离根准备脚本；Python 3.12（SQLite 合成 fixture）；GitHub Actions `ui-e2e.yml`。

## 1. 背景与已核实事实（2026-10-21）

- **剩余项**（整合计划 §3）：
  - N14 第 4 项：采集层读取失败 fixture（用户修订为共享冲突）；
  - N11：项目/模型/CCS/价格原生统一验收；
  - N10：隐私 P06 强取证与完整浏览器矩阵；
  - N13：真实规模性能；
  - N12：D5 系统验收与签字（安装、递增、单跳和连续升级已有发布证据）。
- **N14 第 4 项只差测试**：`crates/tokenscope-core/src/source/mod.rs:267` 起，`read_dir`、子项、文件类型读取失败都会记诊断并 `continue`；文件级读取失败计 `io_errors`。现有 `tests/source_paths_collection.rs` 已有 Windows junction 与断链诊断用例，可沿用其辅助函数。
- **原生验收现状**：
  - 驱动入口 `frontend/scripts/native-acceptance.mjs`（约 2700 行），场景表在 1354 行起。
  - CI `ui-e2e.yml` 只跑 `ui-refresh`、`ui-system`（含 1.5/2 两种缩放）、`ui-collection-status`、`ui-source-paths`，PR #25 上 `native-ui` 用时 8 分 20 秒，作业上限 45 分钟。
  - 没有任何 CCS、价格同步成功路径、项目/模型下钻、单实例的原生场景。
- **CCS 隔离缺口**：`import::ccs::default_source_path()` 固定解析为真实 `~/.cc-switch/cc-switch.db`，acceptance 构建也不例外。原生场景如果调用 `ccs_import_defaults` 或用默认路径预览，就会暴露或读取用户真实 CCS 库。**必须先补隔离，再写 CCS 原生场景。**
- **价格源不可控**：`modelsdev.rs:14`、`openrouter.rs:12` 的 `API_URL` 是常量，ureq 默认读环境代理并支持 `NO_PROXY`。现有 `sync-failure` 场景靠把代理指向 `127.0.0.1:9` 制造失败，无法验证同步成功、单源失败的条数与时间。
- **打开文件会拉起系统程序**：`open_pricing_file` 通过 opener 插件打开文件（`commands.rs:1625`），原生场景直接点击会在 Runner 上启动外部编辑器。
- **隐私取证缺口**（N10 原文，已复核）：
  - `privacy-consent-gate` 的 `businessFiles` 只匹配 `logs/`、`cache.db`、`window-state.json`、`view-cache.json`、`pricing-`，漏了 `history.db` 及 `-wal`/`-shm`；
  - 未同意时只等 1500ms；
  - 没有预置业务文件的读取计数和网络计数；
  - “同意后读到 true”只证明最终值，不证明落盘先于业务 I/O；
  - 浏览器契约里的 denied IPC 是 mock，不是真实后端。
- **受保护命令现为 35 个**：以 `src-tauri/src/privacy.rs` 的 `PROTECTED_COMMANDS` 为单一事实源。整合计划写的 27 个已过时，取证时从源码读取清单，不写死数量。
- **浏览器矩阵**（2026-10-21 执行前复核修订）：`check-ui-contracts.mjs` 一次运行包含两部分。
  - **逐 fixture 巡检**：默认 15 个 fixture × 2 主题 × 2 视口，每个会话都要等 `.app-shell` 与 `.page-head` 出现。
  - **契约**：`REQUIRED_CONTRACTS` 每条只跑一次，由契约自己打开所需的 fixture、主题和视口，不与巡检的 fixture 相乘。
  - `fixtures/ui-contracts.mjs` 的 `FIXTURE_NAMES` 共注册 23 个 fixture；默认巡检不含的 8 个（`large-pricing`、`settings-deferred-race`、`sync-partial-failure`、`query-expired-on-page2`、`status-read-failure`、`source-paths-management`、`privacy-consent-pending`、`privacy-bootstrap-deferred`）各自至少被一条契约使用。
  - 两个隐私 fixture 按设计没有业务骨架，放进巡检会因等不到 `.app-shell` 而失败。这是测试架的设计限制，不是产品缺陷，所以不能简单地把 23 个全部传给 `--fixture`。
  - CI 的 `test:e2e` 只跑其中 7 个契约，巡检只跑 `normal` 一个 fixture。
- **性能测试会写真实目录**：`tests/perf_real_data.rs` 用 `SummaryOptions::default()`，冷轮写入真实 `~/.tokenscope`。这是它作为预热工具的设计用途，但不满足 N13 “输出与价格索引放到临时目录”的要求，所以 N13 另写隔离的新测试。
- **真实来源目录不能做指纹**：`~/.claude`、`~/.cc-switch` 在本机正被 Claude Code 与 cc-switch 持续写入，前后指纹必然不同。隔离证明改用追踪日志：所有来源、历史库、CCS 路径都必须落在隔离根内。

## 2. 范围与不做

做：
- AC01 采集层读取失败测试（共享冲突，N14 第 4 项）。
- AC02 acceptance 构建验收接线与驱动基础设施。
- AC03–AC05 N11 三个原生场景并进入 CI。
- AC06–AC08 N10 隐私原生取证、长时间观察、完整浏览器矩阵。
- AC09 N13 真实规模性能（本地、只读来源、隔离输出）。
- AC10–AC11 N12 单实例自动化与人工签字批次。
- AC12 文档与总账收口。

不做：
- 不做真实跨午夜、长时间休眠、跨显示器（[独立系统待办](../system-acceptance-todo.md)，保持未排期）。
- 不新增适配器，不改产品统计口径；不为验收放宽 CSP、不改必需检查与 ruleset。
- 不在验收中访问真实价格源或任何外网；不读取真实 `~/.claude`、`~/.codex`、`~/.cc-switch`、`~/.tokenscope`（AC09 只读来源日志除外，且输出全部隔离）。
- 不把 20 轮重启压测、120 秒以上的等待、全量浏览器矩阵放进 CI。
- 不删改、不放宽任何现有断言；只追加。不追溯签旧版本的 D5。

## 3. 必须保持的不变量

1. **普通构建零变化**：新增接线全部在 `#[cfg(feature = "acceptance")]` 之内。普通构建读不到任何新环境变量，价格地址、CCS 默认路径、opener 行为与现在逐字节同义；要有一条测试证明普通构建忽略这些变量。
2. **真实数据零接触**：每次原生运行的追踪日志中，所有来源、历史库、CCS、设置、价格快照路径都在隔离根内；计数代理记录到的外部连接尝试为 0；真实 `~/.tokenscope` 前后指纹一致（沿用现有检查）。
3. **CCS 只手动**：用户点击预览之前，追踪日志里没有任何 `ccs_open`；重复导入不增加用量；CCS fixture 文件前后哈希一致（只读）。
4. **历史不随来源消失**：删除来源日志、停用来源、重扫、重建派生索引之后，已保存的请求数与四桶不变。
5. **同意先于业务**：未同意时追踪日志没有业务事件、价格桩命中 0、代理尝试 0、预置文件字节与 mtime 不变；同意后严格按“同意落盘 → 业务解锁 → 首个业务 I/O”的顺序，双击同意只落盘一次。
6. **证据可复查**：每项结论是一个具名断言加实测值，写入 `evidence.json`；失败先按 `evidence.json` 与日志说明原因，测试架缺陷先修测试架，产品问题按缺陷处理，**不以重跑通过代替原因**。
7. **CI 预算**：加入 `native-ui` 的新场景都是确定性的（本地桩、合成数据），合计新增时长目标 ≤ 8 分钟，作业上限 45 分钟不改。
8. **版本规则**：改动 `crates/*/src`、`src-tauri/src` 等产品输入的 PR 递增一次 patch；只改测试、脚本、文档的 PR 不递增、不发布（`release: false`）。

## 4. 任务

每项先写会失败的断言或测试，在未改的代码上确认红（失败摘要记入 §6），再实现变绿。一个提交只解决一类问题。

### AC01 采集层读取失败 fixture（N14 第 4 项）

- 文件：`crates/tokenscope-core/tests/source_paths_collection.rs`（追加）。
- 构造方式（用户 2026-10-21 修订）：Git `sh` 的 MSYS 运行时会启用备份/恢复特权，ACL 拒绝因此不能稳定阻止目录读取。改用共享冲突，不改线程令牌、不加依赖；B1 保持只有测试与文档、`release: false`。
- 前置探针：只用 `std` 编译独立程序，分别在 PowerShell 与 Git `sh` 下运行，目录与文件都必须稳定读取失败；任一种在任一环境下不生效就停下报告，不退回 ACL 或放宽断言。
- 用例 `unreadable_subdir_and_file_are_diagnosed_without_stopping_the_round`：
  - 根下三个子目录各放一条 Claude 事件，另在其中一个可读子目录放一条需要制造读取失败的 `.jsonl`。
  - 目录用 `OpenOptions` + Windows `OpenOptionsExt`，设置 `custom_flags(FILE_FLAG_BACKUP_SEMANTICS = 0x02000000)` 与 `share_mode(0)` 打开；文件用 `share_mode(0)` 打开。两个句柄在整个采集期间持续持有，全部路径限于用例临时根。
  - 断言：其余事件全部计入；目录拒绝出现“无法读取目录”诊断；文件拒绝计入 `io_errors`（或对应诊断）；`summary` 返回 `Ok`，不中断。
  - 清理：用 `File` 的 RAII 保证正常结束或 panic 时释放句柄；正常结束先确认目录和文件恢复可读，再回收本用例查询并严格删除临时目录。
  - `#[cfg(windows)]`。持锁后先用 `std::fs::read_dir` 和真实文件读取自检，确认两者都失败再采集；自检失败时测试明确失败并输出原因，不静默跳过。实测错误码写入证据。
- 红：因为实现已存在，“先红”的做法是把被测代码临时改成遇错 `return`，确认测试会失败，再恢复，并记录这次变异验证。
- 交付：独立 PR，`release: false`。合入后在整合计划关闭 N14。

### AC02 验收接线与驱动基础设施

产品侧（全部 `cfg(feature = "acceptance")`，普通构建保持原样）：

- **CCS 默认路径隔离**：acceptance 构建下，`default_source_path()` 解析为 `<root>/sources/cc-switch/cc-switch.db`，与 `source_dir_override` 同一规则；未 bootstrap 时拒绝，不回退真实目录。
- **价格源地址**：读取 `TOKENSCOPE_ACCEPTANCE_PRICING_BASE`（只接受 `http://127.0.0.1:<port>`），models.dev 改为 `<base>/modelsdev/api.json`，OpenRouter 改为 `<base>/openrouter/api/v1/models`。非法值拒绝启动。
- **打开文件记录**：`TOKENSCOPE_ACCEPTANCE_OPENER=record` 时，`open_pricing_file`、`open_settings_file` 只在文件创建或确认之后记一条追踪事件，不调用系统 opener。
- **追踪日志** `<root>/acceptance-trace.jsonl`：单写者加锁，逐行追加并 flush。每行包含 `seq`、单调毫秒、UTC 时间、`event` 及字段。事件只设断言需要的这些：
  - `consent_saved`（原子替换完成后，附 settings 文件哈希）；
  - `business_ready`；
  - `collect_round_start`；
  - `source_root_read{agent,path}`；
  - `history_open{path}`；
  - `ccs_open{path}`；
  - `pricing_fetch{source,url}`；
  - `opener{path}`；
  - `settings_write{path}`。
- **测试**：
  - 核心库与壳各一条 acceptance 测试：设置变量后路径与 URL 被改写，追踪日志按序写出；
  - 一条普通构建测试：设置同名变量后行为不变（路径仍为默认、URL 仍为常量、不生成追踪文件）。

驱动侧（`frontend/scripts/`、`scripts/`，非产品）：

- **本地价格桩**：驱动在 `127.0.0.1` 随机端口提供两个端点，数据来自新增的合成 fixture `frontend/scripts/fixtures/pricing/`。每个端点可切换 `ok`/`500`/`down`，并记录命中次数。
- **计数代理**：设置 `HTTPS_PROXY`/`HTTP_PROXY` 指向驱动内一个只记录、一律返回 403 的代理，`NO_PROXY=127.0.0.1,localhost`。证据记录“外部连接尝试数”，必须为 0。
- **CCS fixture**：`scripts/make_ccs_fixture.py <输出路径>`，按 `tests/ccs_import.rs` 的 `proxy_request_logs` 与 `usage_daily_rollups` 结构生成，包含：
  - 与原生 Claude 事件重叠的请求（验证并集）；
  - 仅 CCS 有的请求；
  - 日汇总（验证日粒度未知与时区）。
- **隔离根 profile**：`prepare-native-acceptance.ps1` 增加 `-Profile unified`，在现有项目身份日志之外再生成：
  - 跨应用模型异写（如 Claude `claude-opus-5-5` 与 Codex `Claude_Opus_5.5`）；
  - 跨过 272000/272001 边界的长上下文请求；
  - 峰谷两个时段的事件；
  - 外置 `pricing.toml`：含 `[[model.segment]]`、峰谷 schedule、`model_policy` 的 `SameAsInput`、同键不完整候选；
  - 调用 `make_ccs_fixture.py`。
- **驱动通用断言**：
  - 每次运行结束读取追踪日志，断言全部路径在隔离根内、代理尝试为 0；
  - 隐私白名单改为“隔离数据目录中除 `settings.toml` 以外的任何新增或变更文件都算业务文件”，覆盖 `history.db`、`-wal`、`-shm`。
- 交付：与 AC03–AC05 同一 PR（避免发一个只有验收接线的版本），按产品改动递增 patch。

### AC03 原生场景 `ui-identity`（项目、模型、下钻、历史保留）

具名断言：

- `project_same_path_merges_across_agents`：同一路径的 Claude 与 Codex 用量合为一行，两侧 agent 都可见。
- `project_subdir_stays_in_root` / `project_out_of_root_switches`：子目录归当前根；越出当前根成为新项目，其子目录归新项目。
- `project_label_is_last_segment`：显示路径末段；同名不同路径不合并。
- `model_variants_merge_with_friendly_name`：异写模型合为一组，标签取桩数据 models.dev 的 `name`；压缩键不出现在界面。
- `model_detail_keeps_raw_names`：下钻后的明细保留各自原始模型名。
- `drill_from_day_model_project_rows`：从按日、按模型、按项目各点一行进入明细，筛选正确，汇总与明细是同一查询（同 `query_id`），清除后恢复。
- `source_file_deletion_keeps_history`：删除一个合成日志，等下一轮采集后请求数与四桶不变。
- `rescan_and_rebuild_keep_history`：设置页重扫、重建派生索引后，总量不变且不重复。
- `source_disable_keeps_history`：停用 Claude 后状态显示停用，已保存的 Claude 用量仍可按应用筛选查到。

### AC04 原生场景 `ui-ccs-import`（CCS 手动导入）

具名断言：

- `no_ccs_access_before_click`：进入设置页之前和之后、点击预览之前，追踪日志都没有 `ccs_open`；默认路径显示为隔离根内的路径。
- `preview_counts_match_fixture`：预览的请求数、日汇总数与 fixture 一致。
- `cancel_discards_without_writing`：取消后历史总量不变，预览计划被清除。
- `confirm_imports_one_batch_as_union`：确认后形成一个批次；与原生重叠的请求不重复计数；只在 CCS 中的请求被计入。
- `repeat_import_adds_nothing`：再次预览、确认，用量不变。
- `source_timezone_change_is_idempotent`：改选来源时区后重导，请求数与汇总行数不变。
- `failure_recovers_by_new_preview`：把路径指向损坏文件或缺失文件，错误可见、计划被清除；改回正确路径后重新预览可以成功。
- `detail_only_hides_rollups_and_discloses_granularity`：开启“只看请求”后日汇总不参与，界面披露日粒度未知；关闭后恢复。
- `ccs_fixture_unchanged`：fixture 文件前后哈希一致。

### AC05 原生场景 `ui-pricing`（价格同步与计价展示）

具名断言：

- `first_run_banner_without_snapshots`：没有快照时出现全局横幅，费用显示“未知”，不按 0 计。
- `manual_sync_both_sources`：点同步后两源各命中一次，设置页显示各自条数与同步时间，横幅消失，费用出现。
- `single_source_failure_is_partial`：OpenRouter 返回 500、models.dev 正常时，出现部分失败提示和可点击的重试；成功一侧的条数与时间正确。
- `offline_restart_uses_snapshots`：价格桩下线后重启，费用仍由快照计出，无横幅，无崩溃。
- `segment_boundary_272000_272001`（D5 4.9）：边界两侧请求命中不同档位，明细浮层显示档位与依据值。
- `candidate_highest_without_cross_source_mix`（D5 4.10）：浮层显示候选数与“取最高费用”；四类单价来自同一候选。
- `schedule_by_event_time`（D5 4.11）：峰谷两时段的请求命中不同时间档，并按统计时区换算。
- `settings_segment_tags_expand_all_tiers_and_schedule`（D5 4.12）：设置页每个分段模型都有“分段”标记，逐一展开后档位与峰谷规则齐全，缺价分项显示“未知”。
- `same_as_input_and_excluded_incomplete_in_tooltip`：费用浮层显示 `SameAsInput` 补充语义，以及“已排除不完整候选”。
- `open_pricing_file_creates_template_in_root`：点击“打开价格文件”后，在隔离根内创建模板，追踪日志记录 `opener` 事件，没有拉起外部程序。

AC03–AC05 的 CI 接入：

- 在 `ui-e2e.yml` 中各加一步，端口段互不重叠，证据目录放在 `qa-artifacts/ui-adjustment/native-e2e/` 下，由现有上传步骤一并上传。
- 本地每个场景先连续通过 3 次，记录单次耗时，确认符合不变量 7 的预算后再接入 CI。

### AC06 N10 隐私原生取证（2026-10-25 修订：方案以 §4b.1 为准，本节保留为历史）

在现有 `privacy-consent-gate`、`privacy-accept-restart` 基础上追加：

- **`privacy-preseeded`**：
  - 隔离根预置：一份由合成数据真实采集生成的 `history.db`（含 WAL）、日志、视图缓存、价格快照，但没有同意记录；
  - 启动后观察 15 秒（CI）；
  - 断言：追踪日志中没有业务事件；价格桩命中 0；代理尝试 0；预置文件字节与 mtime 不变；没有业务 DOM。
- **`privacy-denied-ipc`**：
  - 未同意状态下，从 `privacy.rs` 源码解析 `PROTECTED_COMMANDS`，经 CDP 调用 `window.__TAURI_INTERNALS__.invoke`，以最小合法参数逐个调用全部受保护命令；
  - 断言：每条都返回隐私拒绝；追踪日志为空；文件不变；
  - `autostart_status`/`autostart_set` 前后**只读**比对 HKCU Run 中 acceptance 标识的值。若意外被写入，场景失败，并只删除这一条 acceptance 标识的值，同时记录在证据里。
- **`privacy-accept-order`**：
  - 点击“同意并继续”，断言追踪日志按序出现 `consent_saved` → `business_ready` → 首个 `history_open`/`source_root_read`；`consent_saved` 记录的文件哈希对应含 `privacy_policy_accepted = true` 的内容；
  - 快速双击同意，断言 `consent_saved` 恰好一次、只启动一套运行时。
- **设置变体与关闭预设**：
  - 先列出 `privacy.rs` 与 `settings` 现有 Rust 测试已覆盖的情形（缺字段、false、只有旧 JSON、TOML 损坏、类型错误、不可读）；
  - 原生只补 Rust 层无法证明的两项：损坏 TOML 启动后仍询问且原文件逐字节不变；预置 `close_action = "minimize"` 但未同意时，点关闭仍弹出隐私退出确认而不是隐藏到托盘。
- AC06 的四个场景都是确定性的，进入 CI。

### AC07 N10 长时间观察与运行期同步（本地一次）（2026-10-25 修订：方案以 §4b.2 为准，本节保留为历史）

- 场景 `privacy-long-observe`，本地运行，不进 CI：
  - 未同意观察 130 秒以上，断言同 AC06 `privacy-preseeded`；
  - 同意后，自动同步线程在 `business_ready` 之后 ≥120 秒才发出第一次 `pricing_fetch`；
  - 关闭“自动同步价格”后再观察一个周期，没有新的 `pricing_fetch`；重新开启后恢复。这一条承接 N12 中 “M11 运行验收：Ready 后跨过 120 秒观察价格检查/同步”。
- 记录受测提交、产物哈希、实际等待时长与追踪日志摘录。

### AC08 N10 完整浏览器矩阵（本地一次）（2026-10-25 修订：表述以 §4b.3 为准，本节保留为历史）

- 用当次构建的 `vite preview` 运行一次 `check-ui-contracts.mjs`：巡检使用默认 15 个 fixture × light/dark × 1280x820/980x620，契约跑全部 `REQUIRED_CONTRACTS`。不改脚本的默认列表。
- **fixture 覆盖核对**：从 `FIXTURE_NAMES` 读出全部注册的 fixture，对每一个记录它被哪种方式实际运行过（巡检，或者打开它的契约名）。以本次运行产物为准，不以源码引用为准。凡是两者都没有覆盖到的，列为缺口并上报，不靠临时修改列表补齐。
- 记录：版本、参数、产物散列；巡检的每个 fixture × 主题 × 视口的结果；每条契约的结果与断言数；覆盖核对表。不能用 JSON 里的 required 列表代替实际结果。
- 失败逐项归因：测试架问题先修测试架；产品问题按缺陷处理。

### AC09 N13 真实规模性能（本地）

- **测试改造**（非产品）：
  - 新增 `crates/tokenscope-core/tests/perf_history_real.rs`（`#[ignore]`，需 `TOKENSCOPE_REAL_PERF=1`）；
  - `cache_dir`、`pricing_index`、历史库全部注入临时目录；来源计划只读指向真实 `~/.claude`、`~/.codex`；
  - 旧 `perf_real_data.rs` **不改不删**：它是 CLAUDE.md 记载的真实缓存预热工具，写入真实 `~/.tokenscope` 是其用途 b，不是缺陷。N13 的结论只取自新测试，不引用旧测试的数字。
- **测量**：
  - 冷采集（空历史库）、热采集（第二轮无新数据）、`query_begin` 加首页汇总、明细前两页的耗时；
  - 事件数与文件数，阶段耗时（沿用 collect_all 日志）；
  - Rust 堆峰值（复用 N05 的计数分配器），进程峰值工作集（外部 PowerShell 轮询，作为参考值）。
- **一致性**：冷、热两轮的请求数与四桶完全一致。同批测多根场景：默认根加一个与默认根重叠的追加根，结果与单根一致，耗时如实记录（承接 PR3 §4.4）。
- **结论**：只记录，不设新门槛。热查询明显超过 1 秒参考值时，在整合计划登记为后续优化项，不在本批优化。
- 运行前后真实 `~/.tokenscope` 指纹不变。

### AC10 N12 单实例原生自动化

- 场景 `ui-single-instance`：同一隔离根启动第二个进程。
  - 断言第二个进程在限定时间内退出；第一个窗口恢复为可见并获得前台；`launches[]` 正确记录两个进程的归属与退出；托盘只有一个。
- 确定性场景，进入 CI；只改脚本，不递增版本。

### AC11 N12 人工签字批次（用户执行）

- **受测物**：AC02 合入后当前最新 main 发布的 NSIS 安装包（以 Release 资产哈希为准）。
- **执行**：我按 [D5 清单](../d5-acceptance-checklist.md)生成一份当次签字表，由用户在本机执行，结果回填。项目如下：
  - 3.3 托盘退出（日志有退出痕迹）
  - 3.4 开机自启（出现/移除；未同意时不读写）
  - 3.5 窗口记忆（含最大化还原）
  - 3.6 真实系统 125% 与 150% 缩放（主界面与隐私政策都可读、点击准确）
  - 3.7 系统深色/浅色切换（含隐私引导首帧、Ready 后偏好恢复）
  - 5.3 离线启动
- **已有证据直接引用**：1.x、2.x 由 Release 流水线的安装与升级证据覆盖；3.1 由 AC10 覆盖；3.2 由 `ui-system` 覆盖。只引用，不重做。
- **排除**：3.8 休眠恢复留在独立系统待办。
- **签字**：D5 §6 只签当次版本；不追签旧版本。

### AC12 文档与总账收口

- 每个任务交付后，回填本计划 §6，并同步整合计划对应 N 行、§7 执行账和 `docs/plans/README.md`。
- 全部完成后，本计划归档到 `archive/implemented/`，关闭本计划承接的责任项；另行新增的 N17 按自己的专项进度记账，独立系统待办继续保留。
- D5 清单 4.9–4.12、5.6–5.8 的结果栏按当次证据回填。

## 4b. B3 修订稿（2026-10-25，待用户确认——未确认前不实施）

> 本节按用户 2026-10-25 指令对 AC06–AC08 做逐项修订，取代 §4 中 AC06–AC07 的原方案与 AC08 的原表述；原文保留作历史， headings 已加指针。修订只排验收方案；实施 PR、CI 接线与本地取证均待本稿获用户确认后另行执行。
>
> **四轮补修（同日，按复核定值口径）**：D 阶段 ④⑤ 的起算改为驱动侧单调时刻（t4 = 阶段③全部成功条件首次确认；t5 = 首个有效关闭态轮询事件），截止固定为起算 + 3660 秒——3660 秒是验收超时预算而非产品调度保证；关闭态零请求检查覆盖「关闭生效至重开生效」完整区间（`pricing_fetch` 与价格桩命中增量均为零），取得关闭态轮询证据即可推进、无须等到截止。
>
> **三轮修订（同日，按最新复核意见）**：①自启替身按**构建模式**隔离——普通构建保持真实插件行为，acceptance 构建必须取得有效隔离根（`root()`，不能只用 `bootstrapped()`），未初始化/根无效/替身读写失败一律报错、绝不进入真实插件；②AC07 保留 C/D 待选，**D 为推荐方案待用户确认**，补齐五阶段完整定义（未同意观察、同进程同意、首查、真实设置关闭 + 轮询证明、同进程重开恢复，证据关联同一宿主 PID）；③探针字段区分 `snapshot_due` 与 `will_sync`（不改变生产判据）；④接线位置校正（核心 `acceptance.rs`/`acceptance_trace.rs`、壳 `acceptance.rs`、`commands.rs`、`lib.rs`、`privacy.rs`、`acceptance-io.mjs`）；⑤AC08 记录点定位到契约循环内实际生效的 `ctx.open` 包装；⑥授权分三层记账，选型 ≠ 实施授权。
>
> **二轮修订（同日，按用户复核意见）**：①AC06 改选方案 A（acceptance 隔离替身）——A0 否决：denied-ipc 经真实 IPC 调用 `autostart_set`，闸门若回归即执行真实插件，事后只读快照只能发现损害、不能防止损害；②AC07 的「零 debug 日志证明未启动线程」不成立（未同意时日志系统尚未初始化，线程即使错误启动也可能无日志），改用 acceptance 线程启动/轮询探针 + 正向控制，同意后的 debug 跳过日志降为辅助证据；③C 明确为部分取证——进程内「关闭→重开→下轮恢复」要么选 D 完整长测、要么保留为 N10 待验责任；④原「删旧场景正则验新场景快照」的变异无因果关系，改为真实反例注入。

### 4b.0 复核事实（与原方案的前提差异，附源码位置）

**AC06 自启动（原方案三处前提不成立）：**

- 名称来源：`src-tauri/src/lib.rs:93` 以 `tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None)` 初始化，未自定义 `app_name`；插件 `tauri-plugin-autostart` 2.7.0（`Cargo.lock:3803`）的 `Builder::build`（其 `src/lib.rs:225-229`）在 `app_name` 为 `None` 时回落 `app.package_info().name`，即 `tauri.conf.json:3` 的 `productName` = **"TokenScope"**。**注册表值名与真实安装共用同一名字，当前不存在任何 acceptance 专属标识。**
- 写入模式：插件从不调用 `set_windows_enable_mode`，`auto-launch` 0.6.0 的 `Default` 为 `WindowsEnableMode::Dynamic`（其 `src/lib.rs:355-358`）——`enable()` 先写 `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Run`，仅 `E_ACCESSDENIED` 才回退 `HKCU`（其 `src/windows.rs` `enable`）。GitHub Windows Runner 以管理员运行 → **值大概率落在 HKLM，只监测 HKCU 必然漏检**。
- 覆盖面：`enable()` 同时在 `HKLM/HKCU\...\Explorer\StartupApproved\Run` 写同名 12 字节启用值（键不存在时跳过）；`disable()` 删两个蜂巢的 Run 值但**不删 StartupApproved**；`is_enabled()` =（LM 或 CU 已注册）且（LM 与 CU 的 StartupApproved 均为启用）（`auto-launch/src/windows.rs`）。
- 隔离风险：本机若装有真实 TokenScope，acceptance 构建调用 `autostart_set` 会以同名 "TokenScope" 覆盖/删除用户真实自启项（Run 与 StartupApproved 同名互踩）。**修订后的任何方案都不得在验收中调用真实自启接口。**
- 结论：原 AC06「`autostart_status`/`autostart_set` 前后只读比对 HKCU Run 中 acceptance 标识的值」三处不成立——标识不存在、目标可能在 HKLM、StartupApproved 未覆盖；废弃该做法。

**AC07 价格同步（原 300 秒预算不成立）：**

- 线程仅在业务解锁后启动（`src-tauri/src/lib.rs:352-356`，`RuntimeStep::PriceSync` 单次保护）；首查固定延迟 **120 秒**（`lib.rs:996`）；每轮（同步或跳过后）睡眠 **3600 秒**（`lib.rs:1058`）。
- 判据 `price_sync_due`（`src-tauri/src/privacy.rs:272-279`）= `Ready` 且 `auto_sync` 且（快照缺失或年龄 ≥ 24h）；设置读取失败按离线处理（`lib.rs:1006-1010`）。纯函数已有单测（`privacy.rs:1141-1149`）。
- `settings_set_price_auto_sync`（`src-tauri/src/commands.rs:1349-1361`）只做 SF02 事务写设置，**不唤醒同步线程**——进程内重新开启后，最早在下一个 ≤1 小时的轮询边界生效；这是当前产品行为，不是缺陷，修订稿如实记录。
- 证据粒度：追踪层只有 `pricing_fetch` 事件（真正发起同步才写）；轮询决策日志为 debug 级（`lib.rs:1019-1023`「价格同步轮询：… → 跳过」），默认 info 不可见。**因此「没有 pricing_fetch」不能证明「关闭状态下完成了一次检查周期」**——要么以 `RUST_LOG=debug`（产品既有诊断通道，非新接线）取得跳过日志，要么让 due 条件为真后观察确无 fetch 并另行证明轮询确已发生。
- 预算算术：进程内完整「开启 → 关闭 → 重开」证据链 = 130 s（首查）+ ≥3,660 s（关闭态跨一个完整轮询）+ ≤3,660 s（重开后等下一轮询）≈ **最长约 2 小时 5 分**；原 300 秒只够覆盖首次同步，无法覆盖关闭态检查周期与恢复。

**AC06 复用现状（WorkBuddy 专项已交付、N10 不重做的部分）：**

- 受保护命令当前 **35 条**（`src-tauri/src/privacy.rs:59-97` 的 `PROTECTED_COMMANDS`，取证时从源码提取，不写死数量）。
- 原生场景 `privacy-policy-update`（`frontend/scripts/native-acceptance.mjs`，随 PR #49 合入）已交付：业务文件清单计入 `history.db`（含 `-wal`/`-shm` 形态的文件名前缀）、同意前零业务追踪事件（`no_business_trace_before_reconsent`）、同意落盘严格早于业务初始化（`business_trace_starts_after_reconsent`，consent_saved 的 seq < business_ready）。`privacy-consent-gate` 的业务文件清单已同步补 `history.db`。
- 升级矩阵的两段式闸门观察段（`scripts/upgrade_test_helpers.ps1`，含 WAL/SHM 三文件快照与「设置未改写、日志零新增」断言）属升级验收侧，方法论（absent 哨兵快照、按实际结果保存证据）可复用，代码不复用。
- 浏览器侧 mock 契约 `privacy_no_business_effects_before_acceptance`、`privacy_denied_ipc_has_no_filesystem_or_network_effect` 保持现状（mock 层）；原生 denied-ipc 走真实 IPC，二者不互相替代。

**AC08 机制现状：**

- `frontend/scripts/check-ui-contracts.mjs:43-59`：默认巡检 15 个 fixture × 2 主题（light/dark）× 2 视口（1280x820 / 980x620）；`frontend/scripts/fixtures/ui-contracts.mjs:1236` 注册 fixture 共 **23** 个；`frontend/scripts/contracts/interaction-contracts.mjs:24-61` 的 `REQUIRED_CONTRACTS` 共 **28** 条。
- 不在默认巡检的 8 个 fixture（large-pricing、settings-deferred-race、sync-partial-failure、query-expired-on-page2、status-read-failure、source-paths-management、privacy-consent-pending、privacy-bootstrap-deferred）中，两个隐私 fixture 按设计没有业务骨架，放入巡检会因等不到 `.app-shell` 失败——测试架设计限制，保持「不全部塞进巡检」的原判断。
- CI `test:e2e`（`frontend/package.json:15` → `scripts/run-ui-refresh-e2e.mjs`）只跑部分契约与 `normal` 巡检；AC08 的全矩阵仍为本地一次取证。
- 产物缺口：`measurements.json` 已记录巡检每格（fixture×主题×视口）与每条契约的结果，但**未记录每条契约实际打开了哪些 fixture**——`runContracts` 的 `ctx.open`（`interaction-contracts.mjs:2464` 起）是全部契约会话的唯一汇集点，可作为运行时记录的挂点。**不能以源码引用或 `required` 列表冒充实际运行结果。**

### 4b.1 AC06 修订：隐私原生取证（方案 A = acceptance 隔离替身，用户 2026-10-25 改选）

四个确定性场景复用既有 acceptance 基础设施（隔离根、追踪日志、价格桩、计数代理、`evidence.json`）；`PROTECTED_COMMANDS` 清单在场景初始化时经 Node 侧正则从 `privacy.rs` 提取，不写死 35。产品侧仅新增下述 acceptance 接线。

**接线（全部 `#[cfg(feature = "acceptance")]`；按构建模式隔离，普通构建零变化）**：

- 文件与位置（沿用既有体系，不另起追踪系统）：替身分支在 `src-tauri/src/commands.rs` 的 `autostart_status`/`autostart_set`（`commands.rs:1430` / `:1441`）命令体内；追踪经核心库公开接口 **`tokenscope::acceptance::record_path("autostart_read"|"autostart_write", <root>/autostart-state.json, …)`** 写入既有 `crates/tokenscope-core/src/acceptance_trace.rs` 的顺序化 JSONL（逐行 flush、seq 连续校验，`acceptance_trace.rs:146-183`）——**必须用 `record_path` 而非 `record`**：只有 `record_path` 会规范化路径、把绝对 `path` 写入事件并在 Rust 侧做隔离根包含校验（`acceptance_trace.rs:216-233`），`record` 不带路径、路径证据不完整。壳侧 `src-tauri/src/acceptance.rs` 只负责桌面验收启动（`start/start_with` → `tokenscope::acceptance::bootstrap_with`），不是追踪或替身的实现位置。**壳内没有 `crate::acceptance::bootstrapped()`**——壳调用的是核心库 `tokenscope::acceptance::`。
- **按构建模式隔离（用户 2026-10-25 三轮修订）**：
  - **普通构建**：替身代码不编译（cfg 限定），真实插件行为逐字节保持；不读取任何新增验收变量、不产生替身状态文件或探针事件；
  - **acceptance 构建**：`autostart_status`/`autostart_set` **必须取得有效隔离根**——以核心库 `tokenscope::acceptance::root()` 返回 `Some(root)` 且根存在为准，只操作 `<root>/autostart-state.json`；
  - **报错路径**：未初始化、`root()` 为 `None`、根缺失/非法、替身状态文件读写失败，一律返回明确错误并**绝不进入真实插件**。
- **`bootstrapped()` 不能替代 `root()` 校验**：核心库存在「已初始化但 root 为 `None`」的状态（`acceptance.rs` 的 `bootstrap_with(None)` 把 `ROOT` 置为 `Some(None)`，此时 `bootstrapped()` 为 true 而 `root()` 为 `None`——例如只跑单测的 acceptance 构建）。替身判定只看 `root()` 的有效值，不看 `bootstrapped()`。
- 防线关系：命令入口的隐私闸门（`privacy::require_ready`）保持第一道防线不变；替身是**闸门回归时的第二道防线**——即使闸门回归放行命令体，也只影响隔离状态文件。
- 新追踪事件同时覆盖三处：生产方（核心 `acceptance_trace.rs` 经 **`record_path`** 写入——路径规范化与隔离根包含校验在 Rust 侧完成）；校验方（`frontend/scripts/lib/acceptance-io.mjs:8-9` 的 **`EVENTS` 与 `PATH_EVENTS` 两个集合都追加** `autostart_read`/`autostart_write`——只进 `EVENTS` 不进 `PATH_EVENTS` 时，缺 `path` 的事件会被校验器放行、路径证据不完整；加入 `PATH_EVENTS` 后，缺 `path`、非绝对 `path`、越出隔离根的 `path` 均判校验失败，见 `acceptance-io.mjs:107-112`）；判定方（场景断言读取事件及其 `path`）。

- **`privacy-preseeded`（预置业务文件 + 未同意观察）**
  - 隔离根预置：合成数据真实采集产出的一份 `history.db`（含 WAL/SHM 文件，用例内先同意→等待采集→重置同意记录后保留文件，或直接以 SQLite 合成）、`logs/` 内一条历史日志、`view-cache.json`、两份价格快照；settings 只含旧修订布尔同意（触发重新询问）。
  - 启动后观察 **15 秒**（CI）；断言（具名，全部入 evidence）：①追踪日志零业务事件（`business_ready`/`collect_round_start`/`source_root_read`/`history_open`/`pricing_fetch` 均无，沿用 policy-update 的判法）；②价格桩命中 0、代理尝试 0；③预置文件清单逐文件「字节 + 大小 + mtime」快照前后一致（快照哨兵沿用「不存在≠变化」教训，PowerShell 侧 `Get-PathSnapshot` 语义在 Node 侧等价重实现）；④无业务 DOM 节点。
  - 与 policy-update 的分工：policy-update 验「旧修订同意 → 重新询问 → 同意解锁」的流程链；preseeded 验「已有业务数据躺在盘上时，未同意状态对它们零读取零写入」。二者互补，不合并且不删除。
- **`privacy-denied-ipc`（全量受保护命令拒绝 + 全蜂巢注册表只读快照）**
  - 未同意状态下，从 `privacy.rs` 提取 `PROTECTED_COMMANDS` 全表，经 CDP 调 `window.__TAURI_INTERNALS__.invoke` 以每命令的最小合法参数逐个调用；断言：①每条都 reject 且错误码为 `privacy_consent_required`；②追踪日志为空；③预置与新增文件快照不变。
  - **自启动部分（方案 A 双防线）**：第一道防线是闸门拒绝（`require_ready`）；第二道防线是替身——**即使闸门回归放行了命令体，acceptance 构建下 `autostart_set` 也只写隔离状态文件，真实注册表不可达**（见 4b.1 接线；这正是 A0 被否决的原因：事后快照只能发现损害、不能防止损害）。全蜂巢只读快照保留为**辅助证据**（不是隔离措施）：运行前后各枚举一次 `HKLM\...\Run`、`HKCU\...\Run` 与两个 `...\Explorer\StartupApproved\Run` 的**全部值**（名称+数据逐值比对，不假设标识名），断言逐字节一致；只读、不删除任何值。真实 Windows 自启的出现/移除继续由 D5 3.4 人工验收。
- **`privacy-accept-order`（同意顺序 + 双击单次落盘）**
  - 断言：追踪按序出现 `consent_saved` → `business_ready` → 首个 `history_open`/`source_root_read`（policy-update 已覆盖前两跳，本场景补第三跳与完整三元组）；`consent_saved` 附带 的 settings 哈希对应含 `privacy_policy_accepted = true` 与当前修订号的内容。
  - 双击同意：以 CDP 连续两次 click（间隔 <100ms），断言 `consent_saved` 恰好一次、只启动一套运行时（`business_ready` 恰好一次）、**同意保存只经历一次原子替换**（`settings_write` 在 `consent_saved` 之前恰一次且相邻成对）——业务初始化后的规范化重写是合法的第二次 `settings_write`，单独记录并披露，不计入双击判定（十七轮复核对齐：CI 追踪中两次 `settings_write` 的第二次发生在业务解锁后）。
- **`privacy-variants`（设置变体与关闭预设）**
  - 先列表核对 `privacy.rs` 与 `settings.rs` 既有 Rust 测试已覆盖的同意变体（缺字段 / false / 旧 JSON / TOML 损坏 / 类型错误 / 修订号偏低偏高——WB05 已补齐单测），原生只补两项 Rust 层证明不了的：①损坏 TOML 启动后仍弹政策且**原文件逐字节不变**；②预置 `close_action = "minimize"` 但未同意时，点窗口关闭弹的是隐私退出确认而非隐藏到托盘（既有 `privacy-consent-gate` 已有同型断言，复用其选择器）。

**自启动自动化的三个候选（用户 2026-10-25 改选 A；取舍如下）：**

| 方案 | 内容 | 能证明 | 不能证明 | 产品输入 | 结论 |
| --- | --- | --- | --- | --- | --- |
| **A（选定）** | 4b.1 接线的自启替身：acceptance 构建下 `status/set` 必须取得有效隔离根（`root()`），只读写隔离状态文件 + `autostart_read/write` 追踪事件；未初始化/根无效/替身读写失败一律报错、绝不进入真实插件 | 闸门正常**与回归**两种情况下真实注册表都不可达（配全蜂巢只读快照作辅助证据）；同意后开关操作的隔离状态机与序列化 | 真实 auto-launch 注册表语义 | 有（src-tauri cfg，实施 PR 递增 patch → 预期 0.6.1） | **选定**；真实「出现/移除」留 D5 3.4 人工签字 |
| A0（否决） | 零接线 + 全蜂巢只读快照；验收不在同意后调用 `autostart_set` | 事后发现损害 | denied-ipc 经真实 IPC 调用 `autostart_set`——**闸门若回归，命令体执行的就是真实插件**；快照只能发现、不能防止 | 无 | 否决（用户 2026-10-25）：防线必须在命令体一侧，不能只靠事后比对 |
| B（备选，不采用） | acceptance 专属名（cfg 内 `app_name("TokenScope-Acceptance")`）+ 一次性 GitHub Runner 上真实写/删该名 | auto-launch 真实写路径（HKLM 优先 + StartupApproved + disable 双蜂巢删除） | 与生产名 "TokenScope" 相关的任何行为；本地运行须禁止（Runner 守卫） | 有（cfg + 守卫，patch） | 保留为记录；真实写路径的语义差异仍以 D5 3.4 人工签字收口 |

**D5 3.4「系统自启动项出现/移除；未同意时既不读也不写」的最终签字保持人工**（AC11 批次），自动化证据只作为辅助。

**红绿与变异（方案 A 下；反例注入按用户 2026-10-25 修订，替换原「删旧场景正则」的无效变异）：**

- 四场景的判定函数先以纯 Node 测试红（沿用 AC03–AC05 的判定套件模式：缺证据、顺序错、文件被改动、命令未被拒等反例必须失败）。
- **真实反例注入（验证完整取证与判定链能拒绝真实违反）**：
  - ①文件变化：preseeded 观察窗口内实际修改一个预置文件（隔离根内追加一字节）→ 场景必须红——证明「快照采集 → 逐文件比对 → 判定」全链能检出真实变化，而不是只在构造样本上通过；
  - ②业务追踪：向证据样本注入一条业务追踪事件（构造含 `history_open` 的假追踪行喂给判定）→ 判定红；
  - ③对照：denied-ipc 以「故意调用一条未受保护命令（`privacy_bootstrap`）」作对照，证明断言能区分被拒与放行。
- **替身接线变异（哨兵后端/调用计数，不触真实后端）**：①给 `autostart_set` 的调用路径临时挂一个**会立即失败的哨兵后端**（真实插件调用点替换为直接 `panic!`/计数的桩），替身在隔离初始化失败时若试图回退即触发哨兵 → 「绝不回退」断言红；②给真实插件调用点挂调用计数器 → 场景全程计数为 0，证明未进入真实后端；③普通构建测试中临时让替身符号可见 → 忽略变量测试红。变异逐字节恢复并记录，**禁止为证明隔离而操作用户真实注册表**。
- 每场景本地三连跑（固定受测提交与产物哈希）后接入 CI `native-ui`；预算**估计**：preseeded ≈ 40 s、denied-ipc ≈ 60 s、accept-order ≈ 45 s、variants ≈ 60 s，合计 ≈ 3.5 分钟——**以三连跑实测耗时为准，实测合计低于 8 分钟上限才接入 CI**；45 分钟作业上限与既有步骤不变。

### 4b.2 AC07 修订：价格同步长观察（D = 推荐方案待用户确认；C = 部分取证备选）

**接线（随方案 A 同一实施 PR，均 `#[cfg(feature = "acceptance")]`）**：追踪事件 `price_sync_thread_start`（线程入口，`src-tauri/src/lib.rs:994` 区域）与 `price_sync_poll`（每轮决策点，`lib.rs:1018` 区域附近）——壳侧经核心库公开接口 `tokenscope::acceptance::record` 写入既有追踪 JSONL（直写、**不依赖日志系统**，未同意阶段也可靠；`crates/tokenscope-core/src/acceptance_trace.rs:146-183`）。三处同步覆盖：生产方（上述 record 调用）、校验方（`frontend/scripts/lib/acceptance-io.mjs:8-9` 的 `EVENTS` 集合追加两个事件名，未知事件会被校验拒绝）、判定方（场景断言读取事件字段）。

**探针字段（区分快照过期与本轮最终决策；不得为满足断言改变生产判据）**——`price_sync_poll` 记录：

- `phase`：业务阶段（`privacy.rs` 的 `Phase`）；
- `age_hours`：快照新鲜度依据（快照缺失/年龄未知时为 null）；
- `snapshot_due`：快照缺失、年龄未知或达到现有 24 小时阈值（独立布尔，不含 Ready/auto）；
- `auto`：设置开关当前值；
- `will_sync`：现有 `price_sync_due()`（`privacy.rs:272-279` = `phase.allows_business() && auto && snapshot_due`）的最终结果。

- **关闭态有效证据** = 真实轮询事件 `phase=ready`、`snapshot_due=true`、`auto=false`、`will_sync=false`；
- **开启态有效证据** = 真实轮询事件 `phase=ready`、`snapshot_due=true`、`auto=true`、`will_sync=true`，随后紧跟双源 `pricing_fetch`。
- `price_sync_thread_start` 的**正向控制** = 线程真启动的场景中该事件必然在场；取得正向控制后，「无事件」才可解读为「线程未启动」。

**形态 D（推荐方案，待用户确认）：本地真实周期长测，同一进程内完整闭环，预算约 2 小时 15 分钟，仅本地取证，不进 CI。**

同一隔离根、同一宿主进程（证据关联同一宿主 PID 与连续 trace 序列），全程本地价格桩（`TOKENSCOPE_ACCEPTANCE_PRICING_BASE`）与计数代理；产品 120 秒首查延迟与 3600 秒轮询周期保持原样，**不增加补偿等待、不改产品时序**；每个阶段以实际探针事件划分，各阶段写明有限截止时间（超时即场景失败并留证据，不静默延长）：

1. **未同意观察（≥130 秒窗口，截止 150 秒）**：全新根、无同意记录启动。断言：`price_sync_thread_start`/`price_sync_poll`/`pricing_fetch` 均无事件（线程未随未同意启动，P03）；价格桩命中 0、外连尝试 0；业务文件（含 history.db/WAL/SHM）快照不变；无业务 DOM。
2. **同一进程内同意**：点击「同意并继续」。**起算** = CDP 实际点击时刻；**固定预算 ≤ 30 秒**（覆盖同意落盘、业务初始化、线程启动与窗口挂载）；**完整成功条件** = `consent_saved` 的 seq 早于 `business_ready` 与 `price_sync_thread_start`（同意落盘先于业务解锁与线程启动）、`price_sync_thread_start` 在场（**探针正向控制取得**）、业务 DOM 挂载。**超时不续期**：预算内未全部满足即场景失败并留证据。
3. **首次双源同步**：**起算** = `business_ready`；**固定预算 = 130 秒观察窗口 + 60 秒同步落盘余量（截止 = `business_ready` + 190 秒）**——窗口只保证覆盖 120 秒首查延迟，**不是「请求必须晚于 130 秒」的断言**。**完整成功条件** = 双源 `pricing_fetch` 各恰一次、**两份快照文件均已写出且 `synced_at` 更新**、「价格自动同步完成」日志在场、首个请求距 `business_ready` ≥120 秒、外连尝试 0。**超时不续期**。
4. **经真实设置操作关闭自动同步**：设置页关闭 `price_auto_sync`（SF02 事务写）；仅在隔离根内把 models.dev 快照 `synced_at` 改写为 25 小时前。**起算 t4 = 驱动首次确认阶段 ③ 全部成功条件满足的单调时刻**（`pricing_fetch` 已观察、两份快照已落盘、完成日志在场）；**截止 = t4 + 3660 秒**——这是验收超时预算，**不是产品调度保证**（源码为每轮工作结束后 sleep 3600 秒，`lib.rs:1058`；跳过同步的轮次没有完成事件或完成日志，轮询决策的精确时刻不可预测，也不需要预测）。**完整成功条件** = 截止内观察到至少一条 `phase=ready`、`snapshot_due=true`、`auto=false`、`will_sync=false` 的 `price_sync_poll`。**关闭态零请求区间** = 「关闭生效（设置落盘）至重开生效（重开设置落盘）」的完整 trace 区间——区间内 `pricing_fetch` 计数与价格桩命中增量均为零，**不能只检查跳过轮询之后**；该区间的最终核对列入阶段 ⑤ 成功条件（重开生效时刻在阶段 ⑤ 才确定）。取得关闭态轮询证据后即可推进阶段 ⑤，**无须等到截止**。**超时不续期**。
5. **同一进程内重新开启**：设置页真实重新开启（**不得用重启替代**）。**起算 t5 = 驱动首次观察到有效关闭态 `price_sync_poll`（`phase=ready`、`snapshot_due=true`、`auto=false`、`will_sync=false`）的单调时刻**；**截止 = t5 + 3660 秒**（同上，验收超时预算，非产品调度保证）。**完整成功条件** = 截止内观察到 `phase=ready`、`snapshot_due=true`、`auto=true`、`will_sync=true` 的 `price_sync_poll` 并随后完成双源 `pricing_fetch` 各一次、**两份快照文件均已写出且 `synced_at` 再次更新**、「价格自动同步完成」日志在场（双源成功落盘）；**关闭态零请求区间核对通过**——自关闭生效至重开生效的 trace 区间内 `pricing_fetch` 计数为零、价格桩命中增量为零。**超时不续期**。

证据：受测提交与 acceptance 产物 SHA256、单次 launch（同一宿主 PID）、连续 trace 序列摘录（探针事件原文 + seq）、桩命中计数、各阶段起算与截止的实测值；`RUST_LOG=debug` 的「价格同步轮询」行仅作辅助对照。**任一阶段超过固定预算即场景失败并留证据，不续期、不重试、不加等待补偿。**

**形态 C（备选，部分取证，约 12 分钟）**：三次重启取证——Run 1（正向控制 + 首查 ≥120 s + 首次双源同步）、Run 2（阶段 A 未同意零线程 + 阶段 B 关闭态检查周期跳过）、Run 3（重启后恢复），结构与 D 各阶段同型但跨进程。C 只证明首查、关闭配置及重启后恢复；**不证明**进程内「关闭→重开→下一轮轮询恢复」——该项**保留为 N10 待验责任**（登记进整合计划 N10 行），原 M11 目标不据此关闭。

E（否决为默认）：acceptance 专用可控时钟/缩短轮询间隔，≈3 分钟——改了被测对象（不再是真实 120 s/3600 s 调度），只能重证 `price_sync_due` 纯函数已覆盖的闸门逻辑（`privacy.rs:1141-1149`）；若用户仍想要一条 CI 级冒烟，可单独评审，不替代 C/D。

原 AC07「未同意观察 130 秒」对应 D 的阶段 ① 或 C 的 Run 2 阶段 A，不再单列场景。

### 4b.3 AC08 修订：完整浏览器矩阵（本地一次）

- **运行方式（保持默认，不塞列表）**：`vite preview` + `check-ui-contracts.mjs` 默认参数全矩阵一次 = 巡检 15 fixture × 2 主题 × 2 视口 + 全部 28 条 `REQUIRED_CONTRACTS`（契约各自打开所需 fixture/主题/视口，不与巡检相乘）。两个隐私 fixture 不进巡检的设计限制维持原判断。
- **fixture 覆盖表（从当次运行产物生成，补一处测试架挂点）**：
  - 唯一改动（脚本、非产品）：记录点必须在**契约循环内实际生效的 `ctx.open` 包装**上——`runContracts`（`interaction-contracts.mjs:2464` 起）先在 ctx 字面量里定义 `ctx.open`，再在 `for (const contract of only)` 循环体内把它覆盖为带 `contract` 的版本；只改初始定义会被随后覆盖而无效。在该生效包装内把每次实际打开的 `opts.fixture` 连同 `contract` 记入结果条目；`check-ui-contracts.mjs` 汇总 `measurements.json`（约 `:1062`）时新增 `fixture_coverage` 节：对 `FIXTURE_NAMES` 全部 23 个，逐个记录 `{ patrol: 巡检是否实际运行过（以巡检结果行为准）, contracts: [实际打开过它的契约名] }`；两列皆空者列入 `uncovered` 并在汇总行打印。
  - **报告口径**：巡检行 = 该 fixture 在真实浏览器完整启动；契约行 = 该 fixture 被该契约实际执行；`uncovered` = 当次运行两者皆无，作为缺口上报，**不**靠临时改 `--fixture` 补齐、更不引用源码 `FIXTURE_NAMES` 或契约实现文本冒充运行结果。
  - 红绿：覆盖聚合逻辑做成纯函数（新建 `frontend/scripts/lib/fixture-coverage.mjs` + Vitest node 环境测试，沿用 AC02 `acceptance-io.test.mjs` 接入先例）——先红：喂入不含某 fixture 行的样例产物 → 期望它进 `uncovered`；变异：把巡检行缺失的 fixture 标成已覆盖 → 测试红；再实现转绿。`ctx.open` 记录点的变异：临时不记录 fixture → 一条已知名契约的 contracts 列为空 → 汇总测试红。
- 记录：版本、命令、产物散列；巡检每格结果；每条契约结果与断言数；`fixture_coverage` 表与 `uncovered` 清单。失败逐项归因（测试架先修测试架、产品按缺陷），不以重跑代替原因。

### 4b.4 授权记录、边界、版本与依赖

**授权与决定分三层记账（避免「选定形态即自动开始实施」的歧义）**：

| 层 | 内容 | 状态 |
| --- | --- | --- |
| 已有方案决定 | AC06 选 **A**（acceptance 隔离替身；按构建模式隔离、`root()` 有效校验、失败报错绝不回退真实插件）；AC08 方向保留（循环内生效的 `ctx.open` 记录点 + `fixture_coverage` 纯函数）；AC06 四场景 CI 预算以本地三连跑实测为准 | 用户 2026-10-25 复核指令确认，**已定** |
| 复核建议 | AC07 推荐 **D**（单进程完整闭环 ≈2h15m） | 最新复核（2026-10-25 交接 §4.2 与本轮用户提示）建议形态，**待用户明确选择** |
| 尚待用户确认 | ①AC07 形态（D 完整闭环，或 C + 进程内恢复保留 N10 待验）；②**B3 实施范围授权**——确认 AC07 形态后仍需用户明确同意才开始实施（选型确认 ≠ 实施授权）；实施 PR 递增 patch 至 **0.6.1**（以实施时最新 main 为准，若仍为 0.6.0） | 未确认；本稿不预支 |

- **普通构建与 acceptance 边界**：AC06 方案 A 的自启替身与 AC07 的两个探针事件均为 `src-tauri` 内 `#[cfg(feature = "acceptance")]` 产品输入（自启替身在 `commands.rs` 命令体、探针在 `lib.rs` 价格线程）；AC08 两处为测试架脚本、零产品输入；本轮修订稿纯文档不取号、不改 `release-change.json`。普通构建零变化由 cfg 边界保证（AC02 先例），并扩展既有普通构建忽略变量测试覆盖自启替身与探针事件。
- **CI 与本地划分**：AC06 四场景进 CI `native-ui`（预算**估计**合计 ≈3.5 分钟——以本地三连跑实测耗时为准，实测低于 8 分钟上限才接入，见 4b.1）；AC07（C ≈12 分钟或 D ≈2h15m，视 4b.5 待确认项）与 AC08（全矩阵，约 15–20 分钟）仅本地一次取证；既有 CI 步骤、检查分组与 45 分钟上限不变。
- **B4/B5 依赖与剩余人工项**：B4（AC09）与 B3 互不依赖、状态不变；B5 的 AC10（单实例）不依赖本修订；AC11 人工签字清单中 D5 3.4（真实自启）、3.3/3.5–3.7、5.3 保持人工，受测物按最新发布即 **v0.6.0 安装包**（原稿写「AC02 合入后最新 main」已过时）。原停点历史（B2 §6「B3 只读准备发现…」段、2026-10-22 记录）全部保留，不将未执行项改写为通过。
- **本稿不做的**：不改产品周期/等待/断言/CSP/必需检查/ruleset/阈值；不访问真实价格源；不运行安装卸载脚本；不操作真实用户注册表（denied-ipc 的全蜂巢枚举为只读比对）。

### 4b.5 待用户确认的具体方案（2026-10-25 三轮修订后）

**已定（历史决定，按既有记录保留）**：AC06 选 **A**（acceptance 隔离替身；按构建模式隔离、`root()` 有效校验、失败报错绝不回退真实插件；真实 Windows 自启行为留 D5 3.4 人工签字）；AC08 方向保留（循环内生效的 `ctx.open` 记录点 + `fixture_coverage` 纯函数，两处测试架脚本）；AC06 四场景 CI 预算以本地三连跑实测为准，实测低于 8 分钟上限才接入。

**复核建议（待用户确认）**：AC07 推荐 **D**——本地真实周期长测、同一进程内完整闭环（五阶段见 4b.2，预算约 2 小时 15 分钟，不进 CI）。备选 **C + 待验**（≈12 分钟；进程内「关闭→重开→下轮恢复」保留 N10 待验责任，原 M11 目标不据此关闭）。

**明确两点授权边界**：
- 选择 C 或 D 只是确认 AC07 验收形态，**不构成 B3 实施授权**；实施开始以用户对「B3 实施范围」的明确同意为准；
- B3 实施范围 = AC06 四场景与替身接线、AC07 所选形态的场景、AC08 两处测试架改动及 CI 接入（AC06），均为 acceptance cfg 产品接线 + 测试/脚本，实施 PR 递增 patch（以实施时最新 main 为准，若仍为 0.6.0 则预期 0.6.1）。

**2026-10-25 用户决定（本批范围调整）**：**AC07 整项暂缓**——C/D 均不在本批执行，不新增其专用价格线程探针，不改变产品同步周期；D 的五阶段定义与预算保留在 §4b.2 供后续单独确认。**本批获准实施 AC06 + AC08**（范围见顶部进度行）；进程内恢复目标**继续保留 N10 待验**，N10 不因本批部分实施而关闭。方案 A 的自启替身照常实施（AC07 暂缓不涉及自启）；价格线程探针（`price_sync_thread_start`/`price_sync_poll`）随 AC07 暂缓一并推迟，本批不加。

实施口径（沿用已复核约束）：自启替身按构建模式隔离、`root()` 有效校验、失败报错绝不回退真实插件；`record_path` 与 `EVENTS`/`PATH_EVENTS` 同步；真实自启后端验证用哨兵/调用计数，不操作真实注册表；受保护 IPC 从源码提取、合法最小参数；文件快照覆盖预置、新增及 history.db/WAL/SHM；先红后绿，不删改既有断言；AC06 固定代码与产物各场景本地三连跑、实测新增 CI 总耗时低于 8 分钟才接入；AC08 覆盖来自实际运行产物；失败先归因。

## 5. 交付批次与验收

| 批次 | 任务 | PR 性质 | 版本 |
| --- | --- | --- | --- |
| B1 | AC01 | 测试 | 不递增 |
| B2 | AC02 + AC03–AC05 + CI 接入 | 产品（acceptance 接线） | patch |
| B3（2026-10-25 调整：AC07 暂缓） | AC06（含自启替身接线）+ CI 接入；AC08 本地取证 | 产品（acceptance 接线：自启替身，均在 cfg 内） | patch（预期 0.6.1，以实施时最新 main 为准） |
| B4 | AC09 | 测试与文档 | 不递增 |
| B5 | AC10 + CI 接入；AC11 人工签字；AC12 收口 | 脚本与文档 | 不递增 |

- B1 可以立即开始；B2 是 B3、B5 的前置；B4 与 B2、B3 互不依赖，可以并行。
- 每个 PR：
  - 先登记上一发布基线（若有新发布），再按 `bump_version.py --check --base-ref origin/main` 自查；
  - 必需检查（`gate`、`native-ui`）与 Install test 全绿后交给用户复核，**合并由用户决定**。
- 验收中发现的产品缺陷：
  - 先写最小回归测试并确认红，再修；
  - 小缺陷在当前 PR 内单独提交，较大的拆成独立 PR，并先向用户报告范围；
  - 不为让场景通过而改断言或加等待补偿。
- 本地取证（AC07、AC08、AC09、AC11）的结果写进 §6，含受测提交、产物哈希、环境、命令、结论与证据位置；证据文件不入库（`qa-artifacts/` 已忽略）。

## 6. 执行记录

### B3 分批实施：AC06 + AC08（2026-10-25，AC07 暂缓）

范围依据本计划顶部 2026-10-25 范围调整记录与 §5 B3 批次行：本批只实施 AC06（四个隐私原生场景、自启隔离替身、回归与反例验证、满足预算后的 CI 接入）与 AC08（实际 fixture 覆盖记录、覆盖汇总与回归验证、本地完整浏览器矩阵取证）；AC07 整项暂缓，未新增价格线程探针，未改变产品同步周期；N10 保持开放。提交：`0191f78`（AC06）、`17fb37e`（AC08 覆盖表）、`af784bf`（AC08 全矩阵归因修复）；版本递增 patch 至 **0.6.1**（`--check --base-ref origin/main` 实测 release=true，base `fd39cdc`）。证据文件不入库（`qa-artifacts/` 已忽略），关键数值记入本节，原始产物保留在 `qa-artifacts/workbuddy/b3/`。

红先（在未改代码上确认失败）：

- AC06 校验方：`acceptance-io.test.mjs` 新增用例「autostart shim trace events require an in-root path」先于 EVENTS/PATH_EVENTS 登记运行——两个事件未登记时校验器按未知事件放行，越根/相对/缺路径断言全部失败（红）；两集合同时登记后同一用例转绿，缺 path、相对 path、越出隔离根三种均判校验失败。
- AC08 覆盖表：`fixture-coverage.test.mjs` 四用例先红（模块缺失）后绿，覆盖「未覆盖不虚报已覆盖」「契约 fixture 去重」「失败巡检仍算实际执行覆盖（但巡检失败本身使总结果失败——覆盖口径勘误见下方复盘记录）」「未注册浮出」四个反例。

AC06 执行记录：

- 实现：自启隔离替身（acceptance cfg，`src-tauri/src/commands.rs`）——`autostart_status/set` 必须取得有效隔离根（`tokenscope::acceptance::root()` 返回 Some 且根存在；`bootstrapped()` 不能替代该校验），只读写 `<root>/autostart-state.json`，未初始化、根缺失/非法、状态文件损坏或读写失败一律显式报错，绝不回退真实插件；替身经 `record_path` 记录 `autostart_read/autostart_write`（带绝对路径）。普通构建不编译替身，真实插件行为逐字节保持（`ManagerExt` 导入按构建模式门控）。替身状态机另附 5 个 Rust 单测（缺文件读为关、roundtrip、损坏报错、根必需、写失败报错）。
- 五个确定性场景进 CI native-ui（各 1 次启动，起始端口 9530/9533/9536/9539/9542）：privacy-preseeded（预置业务文件未同意观察：追踪零业务事件、价格桩与外连零命中、业务目录 + settings.toml 双快照不变）、privacy-denied-ipc（从 privacy.rs 提取全部 35 条受保护命令、最小合法参数经真实 IPC 逐条调用、全部返回 privacy_consent_required；四蜂巢注册表只读快照前后一致）、privacy-accept-order（双击同意 consent_saved 恰一次且早于 business_ready 与首个业务读取；同意与修订版 2 原子落盘）、privacy-variants-corrupt（损坏 TOML 阻断弹窗且文件逐字节不变）、privacy-variants-close（close_action 记忆预设不得绕过隐私退出确认）。端口登记契约期望同步为 14 段互不重叠。
- 稳定性三连跑：受测物为 AC06 实施树（`0191f78`）构建的 release/acceptance EXE，SHA256 `b3e6f9592326094567a919655404ee6572f4cb4125658007560d5821d36a78b4`；固定代码五场景 × 3 次全部通过（15/15），最差单次之和 74 秒（preseeded 18 / denied-ipc 8 / accept-order 32 / variants-corrupt 7 / variants-close 9），低于 8 分钟新增 CI 预算。命令 `node frontend/scripts/native-acceptance.mjs --scenario privacy-<name> --output qa-artifacts/workbuddy/b3/ac06-final-<name>-<n>`；逐次日志 `ac06-final-*-1..3.log`，汇总 `ac06-stable-times.json`。真实 `~/.tokenscope` 指纹一致、外连为零；真实注册表仅只读快照；真实自启后端未进入（替身隔离），出现/移除留 D5 3.4 人工签字。
- CI 接入：`.github/workflows/ui-e2e.yml` 新增五个 Native 步骤（输出 `qa-artifacts/ui-adjustment/native-e2e/privacy-*`），端口契约期望同步更新。

AC08 执行记录：

- 实现：`lib/fixture-coverage.mjs` 纯函数按注册清单聚合巡检行与契约实际打开记录，输出每 fixture 的 `{patrol, contracts}` 行、uncovered 缺口与 unregistered 异常；`runContracts` 在契约循环内实际生效的 ctx.open 包装上记录每次打开的 `opts.fixture`（初始定义会被循环内包装覆盖，改初始定义无效），返回值新增 `contract_fixtures`；`check-ui-contracts.mjs` 的 measurements 新增 `fixture_coverage` 节——全量运行（28/28 契约）时 uncovered 非空进入失败清单，部分选择（CI test:e2e 7/28）如实打印不判失败；unregistered 一律失败；汇总行打印两个计数。
- 本地完整浏览器矩阵取证（归因修复后终跑）：命令 `node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1480 --phase verify --output qa-artifacts/workbuddy/b3/ac08-matrix-final4`，exit=0；60 个巡检场景（15 fixture × 2 主题 × 2 视口）与 28 条契约全部通过、失败断言 0；`fixture_coverage.rows` 23/23（15 巡检 + 22 契约引用，去重）、`uncovered=[]`、`unregistered=[]`。矩阵驱动为前端脚本（受测提交 `af784bf`），应用本体同 0.6.1 树。
- 首跑暴露的过时装配归因修复（`af784bf`，全部为测试装配、非产品缺陷）：long-text fixture 补 `sourceRows:"merged"`（PR3 起默认行是短路径，长路径行缺失）；占位符断言更新为 PR3 后实际值「绝对路径（如 D:\history\claude\projects）」；effective_directory 改按文本定位（四应用行加入后 eff[0] 不再是长路径行）；settings_failure_retry 与 late_failed_settings_read 改用开关 aria-checked 与真实输入值断言（旧 aria-label 输入框已不存在）；date_outer_surface 更新为操作浮层单层实色断言（DESIGN.md §5，C06 `e34495b` 起产品用 ts-popover-solid），并补面板守卫。三次中间跑失败日志（`ac08-matrix-final*.log`）与终跑证据均保留，未以重跑抹掉失败。

门禁与版本：`cargo fmt --check`、clippy 普通与 acceptance 双模式 `-D warnings`、钩子全链（前端 789 测试 + typecheck + build + e2e、Python 336、cargo test 四套、PowerShell 安装 26 项 + 升级契约 69 项、node 语法 50 脚本）全部通过；前端 prettier 门禁只覆盖 `src/**`，scripts/ 下 17 个文件的既有格式差异与本批无关（main 版本同状）。

PR #52 CI 首跑与修复（2026-10-25）：

- CI 组（run 40226439454）：rust、rust-acceptance、contracts 全过；frontend 首跑在首个浏览器格（normal-light-1280x820）`#app .app-shell` 等待 10 秒超时——同一 run 其余三格与本地两轮全过、本批改动不触及启动路径、main 近五次该作业全绿，归因为 CI 冷启动抖动；失败 run 原样保留，重跑失败作业后通过、gate 随之通过。
- Native UI E2E（run 40226439465）通过：AC06 五个隐私场景在 CI 真实 WebView2 上首次实跑成功。
- Install test（run 40226439492）：安装五场景与 plan-upgrades 通过；八组单跳中 0.1.0/0.4.0/0.5.9/0.5.13 全过，**0.6.0 的 present/deleted 两跳失败**（等待「汇总完成」超时 180s）。证据（作业工件 upgrade-evidence-…-0.6.0-present）显示失败发生在**旧版阶段**——registered_version 仍为 0.6.0、settings.toml 只有布尔同意。归因：0.6.0 是登记表里第一个 settings_format 3 基线（认识隐私修订号），而升级夹具对 history 起点只播种旧式布尔同意，0.6.0 按 StaleRevision 拒绝、隐私门不开放；此前所有 history 基线都是格式 ≤ 2（布尔同意有效），缺口首次暴露。测试装配问题，非产品缺陷。
- 修复 `b9b2480`：登记表新增 `privacy_revision`（0.6.0 = 2，validate 强制「格式 3 起必须登记、格式 < 3 不得登记」）；`upgrade_fixture.prepare` 支持 `--consent-revision`（旧版阶段按基线自己的修订号预置同意，不带参保持布尔形态）；`Prepare-OldUsage` 在基线格式 ≥ 3 时传入登记值（缺失即报错）；新增 `Test-ConsentGateApplies`——WB05 闸门观察只对「旧同意对新版已失效」的跳适用，同修订基线跳（0.6.0→0.6.1，revision 2 = 当前 2）旧同意仍有效、观察段跳过（否则有效同意下 45 秒观察必然看到业务而误报），单跳与链路两处同步。红先：CI 两跳失败为跳级红证；本地 3 个 Python 用例与 5 项 PS 契约（含 0.6.0→0.6.1 场景 trace 与闸门适用性四例）先红后绿，升级契约 69 → 74 项。
- 修复后终跑（受测 HEAD `978c513`，全部必需检查通过）：[CI 40230407211](https://github.com/ywltby/tokenscope/actions/runs/40230407211)（rust / rust-acceptance / contracts / frontend / gate 全过）、[Native UI E2E 40230407410](https://github.com/ywltby/tokenscope/actions/runs/40230407410)（五场景再跑通过）、[Install test 40230407568](https://github.com/ywltby/tokenscope/actions/runs/40230407568)（安装、plan-upgrades、十格升级单跳全过——**0.6.0 present 58s / deleted 55s**，链路按 PR 契约 skipped）。

边界保留：AC07 整项未实施（C/D 均未执行），价格线程探针未添加，产品同步周期未改；N10 保持开放，进程内恢复继续待验；真实自启出现/移除留 D5 3.4 人工签字；B4（AC09）、B5 未执行。

### PR #52 复盘修复（2026-10-26，用户六项复核意见 + 证据勘误）

复核反例与原始结果保留在 `qa-artifacts/workbuddy/b3/review-pr52/`（脚本退出 0 表示「成功复现缺陷」，不是产品验证通过）。逐项修复均先有揭示缺陷的失败用例再转绿；一个提交解决一类问题：

1. **格式 3 基线 add 登记产生不可读登记表（P1）**——`8530b49`。原反例 `probe_baseline_registration.py`：add_entry 构造条目不含 privacy_revision，register 直接写盘，登记「成功」后 load_registry 拒读整表。修复：add_entry 在设置格式 ≥ 3 时从**发布标签自身源码**读 `PRIVACY_POLICY_REVISION`（`git show <tag-commit>:settings.rs`，新旧两处路径；实测 `44c026d`（v0.6.0）读得 2，不读当前工作树），拿不到即拒绝登记；register 写盘前 `parse_registry` 整表校验，失败时原表逐字节不变。回归：格式 3 add → reload → resolved_entry → select → check 全链、标签缺修订号干净失败且原表不变、格式 1 add 不携带该键。修后原反例探针按预期转为干净拒绝。
2. **privacy-preseeded 缺观察窗口与文件不变断言（P1）**——提交见修复 2 与实跑对齐提交。原反例：场景体无任何 wait、无双快照比对，真实运行存在约 1 秒结束的场景。修复：隐私框出现后实际观察 15 秒（`performance.now` 单调起止入 evidence），before 取 prepareRoot 在 mutate 后、launch 前的预置快照（新增 prelaunchSettings），after 观察后重采；复用 `snapshotDataDir`/`dataDirChanges` 双向比对（预置/新增/删除/修改，含 history.db/-wal/-shm 与 settings.toml 逐字节）。维护回归四用例先红后绿：满窗+未变才通过；观察不足失败；窗口内修改/新增/删除业务文件判定链失败；窗口内改写 settings 失败。修后三连实测观察 15008–15015ms、changes=[]。
3. **同意顺序断言漏检（P1）**——`8355117`。原反例：旧断言只查首个读取 seq > max(consent, ready)，business_ready(seq1)→consent_saved(seq2)→history_open(seq3) 与 consent 缺失均放行。修复：新增 `consent_saved_precedes_business_ready`（consent<ready，缺任一事件即失败）与 `business_ready_precedes_first_business_read`，原合并断言保留兜底；新增 `consent_saved_single_atomic_replace` 核对同意保存事件自带指纹与相邻成对的 settings_write（不能只凭最终字段宣称原子顺序）。回归四用例先红后绿（正例成序、颠倒失败、缺事件失败、指纹畸形失败）。
4. **双击可能只发一次输入（P2）**——`a17185d` + 实跑对齐 `c5738a6`。原反例：第二次 locator.click 失败被 catch 吞掉、delay:20 非点击间隔。修复：坐标级 `page.mouse.click` 真实输入 + 页面捕获计数器（pointerdown/click + timeStamp + clientX/Y）证明输入到达；第二次下发失败显式失败。**实跑修正两处**：并发排队会在 CDP 输入管线丢第二次完整点击（改顺序下发）；同意过渡替换光标下 DOM 使浏览器不合成第二次 click（实测 pointerdown#2 于 21.6ms 到达正确坐标、click 仅合成 1 次）——送达判定按 pointerdown（输入到达与产品抑制解耦），click 合成次数入 evidence。修后实测 delivered=2、间隔 22.4ms、synthesizedClicks=1。
5. **日期浮层材质断言放过错误外壳（P2）**——提交见修复 5。原反例：量测含 body 页面背景且只要求 solidLayers≥1，双层实心浮层与全透明外壳靠页面背景通过（两个 falsePositive 复现）。修复：量测边界从 .range-panel 到 body/html 前（hitPageBackground 必须为真且层级≥3），边界内**恰好一层实色且必须 n-popover 外壳**，其余层（含内层与 v-binder 包装）透明无模糊、边界内零 elevated。维护回归以内存量测驱动真实 runContracts：正例链取自真实取证形态，双层实色/全透明/边界未达页面背景三反例先红后绿。
6. **闸门观察误判已有失效修订号（P2）**——`1737a6a`。原反例 `probe_stale_consent_gate.py`：观察函数把「privacy_policy_revision 字段存在」判为程序新写入，启动前已有 revision=1 且文件未变的合法旧同意被误拒。修复：观察前后**按行内容比对**——新增（前无后有）或改写（值变化）才违规，内容未变通过；其余变化仍由整文件比对拦截。真实观察函数完整路径六用例（外部动作惰性桩化、快照/比对真实实现）先红后绿：未变通过、新增拒、改写拒、改其他设置拒、新增业务日志拒、历史库写入拒。升级契约 74→80 项。0.6.0→0.6.1 同修订跳观察段本就跳过，本反例不涉及十格升级的既有通过事实。

补证与勘误：

- **自启命令层路由证据**（提交见补证提交）：acceptance 构建下经真实 IPC 调用 autostart_set/status，断言替身返回一致、隔离根出现 autostart-state.json、追踪 write/read 路径在根内、四蜂巢注册表只读快照不变（真实后端不可达，全程零注册表写入）；另附 `scripts/test_autostart_command_routing.py` 源码契约（require_ready 先于 run_blocking、acceptance cfg→替身→not cfg→真实插件顺序、全部替身引用在 acceptance 区域内=普通构建编译不到、autolaunch 仅在 not 区），契约自检三类突变必须被检出。修后实跑该路由段 19/19 断言全过。
- **两次启动超时根因：未定位**。复查复核方提供的完整证据（native denied-ipc launches：宿主 4294967295 退出、CDP 2.3s 就绪但 20s 内只有 about:blank、无 tauri target、同 run 其余场景正常；frontend 首格 `#app .app-shell` 10 秒超时、同 run 其余三格正常）：现有证据只有症状（首轮浏览器/页面未就绪），没有控制台采集与可归因的错误输出，**无法确定「环境抖动」为根因**；失败证据原样保留（CI run 40226439454 frontend 首跑、40231825406 native-ui 首跑及 review-pr52 的两份 zip），以重跑通过作为恢复事实而非归因结论。未增加任何超时掩盖。
- **AC08 旧证据提交词勘误**：ac08-matrix-final4 的 measurements.json commit 字段为 `17fb37e`（运行时 HEAD），此前文档写 `af784bf` 不实——矩阵运行发生在 af784bf 提交之前，工作树含未提交的归因修复，**无法证明干净提交对应关系**；该旧证据保留但降级为开发期参考。修后已在固定受测树重新取证（见下）。
- **覆盖口径勘误**：实际实现是「失败巡检仍计入实际执行覆盖（不虚报为通过），但巡检失败本身使总结果失败」；此前「失败巡检不算覆盖」的表述与实现不符，红先清单已更正。

修后固定受测树重新取证（HEAD `c5738a6`，工作树干净）：

- 受测 acceptance EXE SHA256 `8f48a293d7cfd359a7f330ba82e14a052b35041fe3a27428cc91df3e9a833df5`（cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol）；驱动脚本散列：native-acceptance.mjs `f2d3a8ab…01559c`、interaction-contracts.mjs `b0d62fe3…f5739`、check-ui-contracts.mjs `cf4b581c…18d71`。
- AC06 三连（`qa-artifacts/workbuddy/b3/ac06-review2-final*`）：五场景 × 3 全部通过（15/15），逐场景秒数 preseeded 20/19/19、denied-ipc 5/5/5、accept-order 7/8/7、variants-corrupt 6/4/5、variants-close 5/5/6，最差单次之和 **45 秒**（旧 74 秒为修复前口径，preseeded 观察窗口原缺失现补足 15 秒）；五步并行 CI 墙钟约等于最慢场景 20 秒，仍远低于 8 分钟预算。preseeded 实际观察 15008/15015/15008ms、双快照 changes=[]；accept-order 双击 delivered=2、间隔 22.4ms、产品正确抑制第二次 click 合成；自启路由 set/status 一致、状态文件在根、注册表四蜂巢不变；real_untouched 全过。
- AC08 完整浏览器矩阵（`qa-artifacts/workbuddy/b3/ac08-matrix-review2`）：exit=0，measurements.json commit=`c5738a6`；60 巡检场景 + 28 契约全部通过、失败断言 0、fixture_coverage 23/23（uncovered=[]、unregistered=[]）。

复盘修复后的 CI 收口（三个补充提交 + 最终全绿）：

- 4910660 / a98dfad / `4c9c71b`（CI 实跑揭示的第三处装配缺陷）：CI Runner 的 TEMP 为 8.3 短名（`C:\Users\RUNNER~1\...`）而追踪路径经 Rust canonicalize 是长名（`...\runneradmin\...`），自启追踪前缀比较失配（12/13）；第一轮 realpathSync 修复无效——Node 的 **JS 版 realpath 不解析 8.3 短名**（40258699869 的诊断 detail：rootNorm 仍为 runner~1），改用 `fs.realpathSync.native`（libuv `GetFinalPathNameByHandle` 展开）+ 双侧统一斜杠/小写后通过。三次失败 run（40254310229 / 40256321675 / 40258699869）原样保留作归因链。
- 最终受测 HEAD `4c9c71b` 全部必需检查通过：[CI 40260524738](https://github.com/ywltby/tokenscope/actions/runs/40260524738)（rust / rust-acceptance / contracts / frontend / gate）、[Native UI E2E 40260524789](https://github.com/ywltby/tokenscope/actions/runs/40260524789)（五隐私场景 + 其余原生场景全过）、[Install test 40260524737](https://github.com/ywltby/tokenscope/actions/runs/40260524737)（安装、plan-upgrades、十格升级单跳全过——0.6.0 present 55s / deleted 65s 量级，链路按 PR 契约 skipped）。EXE 与 Rust 源在本轮复盘未再变更（仍为 `8f48a293…`）；本地三连取证于 `c5738a6` 树（驱动脚本在自启比较三处修复前），CI 五场景实跑为最终脚本代码的通过证据，两者口径已在本节写明。

### 二轮复核补修：同意保存字节证据内容核对（2026-10-26，`bba12f9`）

复核方独立复现：consent_saved 只带 settings_hash、判定方仅查格式——事件哈希填 `0000000000000000`、实际文件哈希 `0acec52d49477739` 且无后续重写时场景 13/13 全过，「保存事件哈希对应包含同意值与当前修订号的内容」未被取证（取证缺口，非产品缺陷）。

- `acceptance_trace::consent_saved`：保存时字节以 `settings_bytes_hex` 随事件内嵌（JSONL 只追加即不可变证据；hex 手写编码零新依赖），业务期随后的合法重写不影响这份保存时快照。
- 判定方：`consent_saved_single_atomic_replace` 解码证据重算 fnv1a64 与事件指纹比对（缺证据/畸形 hex/不匹配均失败）；新增 `consent_saved_capture_content` 解析证据内容必须含 `privacy_policy_accepted = true` 与 `privacy_policy_revision = 2`；不与终态文件硬比（业务期重写正例保留）。
- 红绿：壳侧 AC02 测试加强先红（缺 `settings_bytes_hex` 即 panic）后绿（解码重算一致 + 内容断言）；维护回归 25 项——负例四条（格式合法假哈希、哈希对但缺修订号、哈希对但 accepted=false、缺字节证据字段）先红后绿。
- 修后固定受测树 `bba12f9` 干净构建 EXE SHA256 `64250a33ce6658a1333fbfa690fb2af20c9278c1bb0a15129fe5347dec4f0204`；五场景三连 15/15 全绿（preseeded 22/21/21、denied-ipc 7/6/9、accept-order 9/8/7、variants-corrupt 5/5/5、variants-close 8/7/7，最差单次之和 **53 秒**）；accept-order 实测保存事件带字节证据、重算哈希与事件指纹一致（`36fd8552…`）、内容解析通过、双击 2 次按下（间隔 18.8ms）、preseeded 实际观察 15006ms。

**双击归因更正**（按实际输入次数、合成次数与单次初始化分别陈述）：实测两次真实输入均到达同意按钮坐标（pointerdown × 2，间隔 18.8–22.4ms）；第二次 pointerdown 之后浏览器未合成第二个 click 事件——同意过渡此时已替换光标下的按钮元素，这是 DOM 被替换时浏览器不再派发 click 合成的行为，**不是产品主动抑制**；产品侧以 `consent_saved` 恰一次、`business_ready` 恰一次回应全部输入（无二次落盘、无二次初始化）。此前「产品正确抑制第二次 click 合成」的表述不准确，证据字段（delivered/synthesizedClicks/intervalMs）如实入 evidence。

**运行标签勘误**：本轮此前汇报将 1438f44（文档轮）的运行标签写反——正确对应为 CI 40261768046、Install test 40261767973（均通过），Native UI E2E 为 40261768083；4c9c71b 轮为 CI 40260524738 / Native 40260524789 / Install 40260524737（均通过）。

### 三轮复核补修：字节证据内容改用真正 TOML 解析（2026-10-26，`33e41e4`）

复核方复现（`qa-artifacts/workbuddy/b3/review-pr52-round3/native-ac06-content-probes-same-bytes.json`，哈希正确、保存字节与终态一致仍 14/14 全过）：`consent_saved_capture_content` 用正则搜索字符串，被三种形态误放行——注释写 `accepted = true` 而实际字段为 false；同意字段只在 `[notes]` 子表、顶层缺失；`revision = 20` 被 `/revision\s*=\s*2/` 前缀匹配成当前值。

- 判定方改用 **smol-toml**（TOML 1.0 解析器，devDependencies）：解析失败必须拒绝；顶层 `privacy_policy_accepted` 严格 `=== true`（布尔）、顶层 `privacy_policy_revision` 严格 `=== 2`（整数）——不搜索子表、不做字符串包含、不再叠加正则补丁。
- 壳侧 AC02 断言同步替换：字符串 contains 改为 `toml::Value` 解析 + 顶层键严格断言（`src-tauri` dev-dependencies 加 `toml = "1.1.6"`，与核心库同一解析器；Cargo.lock 相应一行）。
- 红绿：维护回归 29 项——负例四条（三种误放行形态 + 非法 TOML）先红（前三条在正则实现下全放行）后绿；负例哈希与字节自洽，失败必须来自内容解析层。
- 修后固定受测树 `33e41e4` 干净构建 EXE SHA256 `9f9f18ab45d47f5cdf60aba5574e36d72e3854eceba5a427098ebb7427ceb6bf`；五场景三连 15/15 全绿（preseeded 22/20/21、denied-ipc 5/6/5、accept-order 8/9/8、variants-corrupt 6/5/6、variants-close 7/7/7，最差单次之和 **49 秒**）；accept-order 实测 `accepted=true revision=2`（顶层严格判定通过）、双击 2 次输入（间隔 24.7ms、合成 click 1 次）；四种对抗形态经同一解析器直接验证全部拒绝。

### 四轮复核补修：字面量类型保留与非法日期语义（2026-10-26，`c6faca4`）

复核方探针（`qa-artifacts/workbuddy/b3/review-pr52-round4/native-ac06-type-probes.json`，哈希正确、字节与终态一致仍 14/14 全过；产品 Rust `read_consent` 对同类输入均返回 Unreadable）暴露 smol-toml 单解析器判定的两处类型/语义缺口：

- smol-toml 把整数/浮点统一解析为 JS number：`revision = 2.0` / `2e0` / `2.0000000000000001`（超出 double 精度折回 2）经 `=== 2` 全部放行——`Number.isInteger` 无法区分，值比较层无解；
- smol-toml 日期处理宽松：`notes = 2026-02-30` 被接受，Rust toml 明确拒绝。

修复：判定改为**双解析器交叉**——smol-toml 保留整篇值语义合法性（抛错即拒）；新增 toml-eslint-parser（devDependencies）用 **AST 保留字面量类型**做内容判定：顶层 `privacy_policy_accepted` 必须 `kind === "boolean"` 且值为 true、顶层 `privacy_policy_revision` 必须 `kind === "integer"` 且值为 2；子表键、内联表、多段键路径一律不算顶层；语义非法日期在严格解析时直接拒绝（`2026-02-30` REJECT、`2026-02-28` 通过）。不使用任何正则/字符串包含补丁。

红绿：维护回归 33 项——新增四负例（浮点 2.0、2e0、超精度折回、非法日期）先红（smol-toml 判定下全放行）后绿；对照矩阵 12 形态全部按预期（含 `accepted = "true"` 字符串、内联表、`revision = 20` 等历史形态不回退）。壳侧 AC02 的 `toml::Value as_integer/as_bool` 断言本就类型严格，无需变更。

修后受测树 `c6faca4` 重跑五场景三连 15/15 全绿（preseeded 21/20/20、denied-ipc 5/6/6、accept-order 7/8/7、variants-corrupt 5/4/4、variants-close 6/6/7，最差单次之和 **46 秒**；EXE 未变仍为 `9f9f18ab…`，本轮仅判定脚本变更）；accept-order 实测 detail 为 `accepted kind=boolean value=true；revision kind=integer value=2`。

**3744891 轮运行标签勘误**（此前汇报再次写反）：正确对应为 **CI 40280864502、Native UI E2E 40280864366、Install test 40280864372**（均 success）。

### 五轮复核补修：严格 UTF-8 解码与带引号顶层键（2026-10-26，`2e20fa5`）

复核方探针（`qa-artifacts/workbuddy/b3/review-pr52-round5/native-ac06-byte-probes.json`，原始字节哈希均正确、场景 14/14 全过）暴露两处：

- **P2 非法 UTF-8 误放行**：`toString("utf8")` 默认把坏字节替换成 U+FFFD，两个解析器检查的是替换后文本——注释中的 FF、截断的多字节编码、过长编码（C0 80）、代理项编码（ED A0 80）全被误放行，而产品 `String::from_utf8` 对同样字节报 InvalidData。修复：`TextDecoder("utf-8", { fatal: true })` 严格解码，失败即拒绝（detail 标注与产品同口径）；合法编码的 U+FFFD 字符不受影响（EF BF BD 正例守护全程通过）。
- **P3 合法带引号顶层键误拒**：AST 键名读取只取 `TOMLBare.name` 与不存在的 `content` 属性——`TOMLQuoted`（单引号/双引号/转义形式）的文本属性是 `value`，合法同意字段被误判缺失（Rust 对同样文件返回 Granted）。修复：键名读取改 `name ?? content ?? value`（当前产品生成的裸键行为不变）。

红绿：维护回归 41 项——四条非法字节负例先红（误放行）后绿；三条带引号键正例先红（误拒）后绿；合法 U+FFFD 正例全程通过。字节级对照矩阵八形态（四非法字节、合法 U+FFFD、三种引号键）与 Rust 行为逐一对齐。

修后受测树 `2e20fa5` 重跑五场景三连 15/15 全绿（preseeded 22/25/33、denied-ipc 7/8/7、accept-order 10/11/10、variants-corrupt 8/8/8、variants-close 13/13/10，最差单次之和 **75 秒**，仍远低于 8 分钟预算；EXE 未变仍为 `9f9f18ab…`，本轮仅判定脚本变更）；accept-order 实测重算哈希与事件指纹一致、顶层严格类型判定通过。

五轮补修后的 CI 收口（受测 HEAD `c2ce609`）：首轮 Native UI E2E 的 privacy-preseeded 再次出现「没有 tauri 页面 target」（与此前两次同型症状，**根因仍未定位**，失败 run 40348043583 原样保留，重跑通过仅说明恢复）；Install test 的 0.5.13 deleted 跳因 GitHub API 连接被远端重置（WinError 10054，日志支持，**环境级网络失败**，run 40348043558 首轮证据保留）——Native 的症状**不归为环境级**（同一轮两者失败性质不同），重跑失败作业后全部通过。最终全部必需检查 18 pass + 1 skipped、0 fail：CI 40348043575（success）、Native UI E2E 40348043583（重跑 success）、Install test 40348043558（重跑 success，十格升级单跳含 0.6.0 present 58s / deleted 1m4s）。

### 六轮复核：测试装配更正与 2edeafc 收口核查（2026-10-27）

复核方指出两处回归装配偏差并确认代码修复本身正确（41/41、四坏字节严格拒绝、三带引号键与合法 U+FFFD 通过、既有 15 份原生证据核对成立）：

- **坏字节位置**：五轮负例把坏字节放在注释结束后（`# note\n` 已闭合），落在裸键前——旧版 TOML 解析本就拒绝，「先红」不真实。更正：坏字节放**注释行内部**（`# <坏字节>\n`，替换成 U+FFFD 后注释仍合法）——实测旧版 `toString("utf8")` 判定下确实放行、严格解码 InvalidData，红真实性成立。
- **TOML 转义**：`"privacy_polic\u0079_accepted"` 在 JS 普通字符串里被预转义成 `y`，TOML 转义路径未被测试。更正：`String.raw` 保留字面反斜杠（标签与内容同步），测的是 TOML 层转义。
- **归因矛盾更正**：五轮收口段同时写「根因仍未定位」与「两者均为环境级失败」矛盾——更正为 Install 的连接重置（WinError 10054）有日志支持属环境级网络失败；Native 的「无页面 target」症状保持**根因未明**，重跑通过只说明恢复，不作为归因。
- **2edeafc 收口状态核查（复核方留证 `qa-artifacts/workbuddy/b3/review-pr52-round6/native-head-failure.json`）**：该轮 CI 为 **40353024051**（success）——此前误写为前一轮（c2ce609）的 40348043575；该轮 Native UI E2E **40353023758 仅一次尝试且失败**，与「无页面 target」不同型——privacy-accept-order 已连接 Tauri 页面（page_target_url 非空、cdp/page_target 共 1.4s），隐私框出现后等待 `.app-nav` 60 秒超时，`acceptance_trace.records` 为空、无点击计数留存、host/app 日志尾为空，根因**未明**（PR CI 的 merge-checkout SHA `b1af5ac`、EXE `559a1484…`，与本地 `9f9f18ab…` 不同属正常）；**该轮 Native 未重跑通过**（此前误写「重跑后通过」——当时监控遗漏了这轮，随后 b73c025 修复轮覆盖）。此处如实保留为未定位失败，不用重跑替代归因。

六轮收口过程（2026-10-27，全部失败原样留证）：

- 装配更正与归因修正随 `b73c025` 推送后，该轮（[Native 40358956806](https://github.com/ywltby/tokenscope/actions/runs/40358956806)）privacy-accept-order **再次** `.app-nav` 60 秒超时——同型第二次复现；因失败现场取证缺口（场景在 waitForSelector 抛错时点击捕获未读取），仍无法区分「输入未发出 / 输入未处理 / 保存失败」。
- 取证能力补齐 `ef77836`：解锁超时的 catch 先抓现场再原样抛错——页面捕获的全部点击（类型/时间戳/坐标）、隐私框是否仍在、阻断错误文案、settings.toml 快照、已发生追踪事件序列、整页截图，入 evidence 的 `acceptFailureDiagnostics`；取证自身失败不掩盖原错误；正常路径与 41 项维护回归不变。
- `ef77836` 轮（[Native 40363363800](https://github.com/ywltby/tokenscope/actions/runs/40363363800)）失败场景变为 **ui-ccs-import**（「无页面 target」同型）；该轮 **privacy 五场景为 skipped 而非通过**（首轮作业失败后 GitHub 跳过同作业剩余场景，第二次尝试 attempt 因新推送触发并发取消而 cancelled）——此处此前误写「隐私五场景全部通过」，已在七轮复核更正（复核方经 API 核实运行矩阵）。失败的进一步归因不足：失败分布（denied-ipc、preseeded、accept-order×2、ccs-import，无 target ×3、解锁超时 ×2）与本地未复现**不足以证明环境根因**，五轮 Native 失败**继续记为根因未定位**，全部失败 run 原样保留，不增加超时掩盖。
- 最终受测 HEAD `c831b74`（复核方追加的研究文档提交，docs-only 不动验收代码）全部必需检查通过：[CI 40366682692](https://github.com/ywltby/tokenscope/actions/runs/40366682692)、[Native UI E2E 40366682655](https://github.com/ywltby/tokenscope/actions/runs/40366682655)、[Install test 40366682837](https://github.com/ywltby/tokenscope/actions/runs/40366682837)——18 pass + 1 skipped、0 fail，覆盖确认 ef77836 的验收代码检查。

七轮复核补修（2026-10-27）：取证逐项有界

- **取证失败清空现场（P2）**：解锁超时的诊断段原先整体 try/catch——任何单项失败（截图异常、CDP 拒绝）都会用 `{errorText, diagError}` 覆盖已取得的现场。改为**逐项采集、失败只追加**：每项独立写入 diagnostics，失败记为 `<label>Error`，其余项照常。
- **取证无时限（P2）**：诊断段的 `page.evaluate` 直接 await——CDP 挂起 8.1 秒仍未抛回原错误（复核方探针 `review-pr52-round7/accept-failure-probes.json`）。每项经 `withTimeout(…, 8000)` 有界（复用既有原语），超时跳过并记原因；挂计数器的 evaluate（点击前，正常路径）同样补 8 秒上限——否则 CDP 挂起时它先于取证段挂住。
- 红绿：维护回归 44 项——三反例（截图失败不清空现场、CDP 拒绝只记该项错误、CDP 挂起在上限内跳过且不拖垮场景）先红后绿；本地实跑 20/20 断言通过。
- **取证首次捕获完整现场（`8a239ea` 轮，[Native 40416338270](https://github.com/ywltby/tokenscope/actions/runs/40416338270)，accept-order 第三次解锁超时）**：`acceptFailureDiagnostics` 完整落盘——捕获层记录到 pointerdown+click 各 ×2（时间戳 1382/1387ms、间隔 5.5ms、坐标约 (731,627)）；隐私框 count=1、无阻断错误文案、settings.toml 快照为预置内容（同意未落盘）。~~结论：根因收窄到产品前端事件处理偶发未生效~~（八轮复核撤回：document 级捕获计数器只记坐标、未记录目标元素，不能证明事件落在同意按钮——点击其他元素同样得到完整两组事件；失败点击坐标 (731,627) 与超时截图中按钮位置（x≈744–834、y≈681–709，DPR=1）不一致，但截图晚于点击，既不能断定坐标偏移、也不能声称按钮"仍在原位"；坐标先取后点，动画/布局变化未排除）。**结论回退为根因未定位**：输入是否到达同意按钮、坐标是否失效、产品前端是否未响应，三层均未证实。

### 八轮复核补修：点击目标证据与按钮稳定性（2026-10-27，本节随补修提交更新）

- **P2 点击目标证据**：计数器事件增加 `target`（tagName/className/text 截断）；点击前**按钮稳定性等待**——连续两次 boundingBox 采样一致才下发输入（动画移动期间每次采样位置都不同，不会提前通过）；点击时按钮位置（`buttonBoxAtClick`）与解锁后按钮位置（`buttonBoxAfterClick`）都入 evidence——下次失败即可直接判断坐标是否失效、事件目标是否为按钮。
- **P2 吞错**：`privacyBlockedDetail` 的内层 `.catch(() => [])` 移除——查询失败由外层 bounded 记 `privacyBlockedDetailError`，不再伪装成「没有文案」。
- 红绿：维护回归 47 项——三反例（布局动画期点击用了偏移位坐标、阻断文案查询失败吞成空列表、计数事件缺目标元素）先红后绿；本地实跑 22/22 断言通过。产品侧归因待下次失败用新证据再判断；本轮不改变「根因未定位」。

### 九轮复核补修：稳定性判定实效化与位置证据保留（2026-10-27，`9c4ec0f`）

复核方探针（`qa-artifacts/workbuddy/b3/review-pr52-round9/review-probes.json`）暴露三处：

- **P2 稳定性虚报**：稳定性循环耗尽 20 次采样后直接继续点击，`button_position_stable_before_click` 固定为 true；首次两采样间无等待，同帧相同不能证明动画结束。复现「始终移动」「首次可见随后消失」两形态均下发两次点击并记绿。修复：**跨帧采样**（先等 100ms 再取下一次）、stable 显式判定入断言；采样耗尽或按钮消失直接失败（不稳定时零点击）。
- **P2 位置证据被覆盖**：正常路径末尾整体重写 `ev.acceptClicks`，清掉 buttonBoxAtClick/buttonBoxAfterClick（复核方核对最新 CI 产物两字段均不存在而断言为绿）。改为展开保留；失败路径补 `buttonBoxAfterFailure`（与 buttonBoxAtClick 对比即可判断布局是否在点击后变化）。
- **P3 目标元素回归未验证实现**：测试桩跳过监听器注册与执行、自行塞 target——受测源码退回无 target 记录的 5715da8 测试仍全绿，「先红后绿」不成立。改为**真实执行受测监听器**（new Function 注入 window/document 捕获注册回调；点击时按注册类型真实派发——修正桩先前的双计形态，pointerdown 监听器不收 click）；实测移除 target 记录后两用例变红（红真实性成立）。

维护回归 53 项全绿；本地实跑 22/22 断言通过。修复过程中的过程性失误如实记录：九轮场景实现一度被 `git checkout` 误恢复为 HEAD 版本（工作树未提交改动被撤销），发现后按既定设计重建并经 53 项回归与实跑复核，无残留。

### 十轮复核补修：位置查询错误区分与等待驱动回归（2026-10-27）

复核方探针（`qa-artifacts/workbuddy/b3/review-pr52-round10/review-probes.json`、CI 产物核对 `ci-evidence-audit.json`）暴露两处：

- **P2 位置查询吞错**：正常路径 `buttonBoxAfterClick` 的 `.catch(() => null)` 把所有查询异常变成合法 null——注入 CDP 查询失败后场景全绿，证据与「成功查询、按钮不存在」完全相同，无法归层；最新 CI 该字段也是 null，来源不可判。修复：区分合法 null（按钮不存在）与采集异常（记 `buttonBoxAfterClickError`）。负例先红（吞成 null 无错误字段）后绿。
- **P3 跨帧等待回归无牙齿**：位置桩按采样次数变位置、waitForTimeout 空桩——移除受测 `await page.waitForTimeout(100)` 测试仍通过。改用**虚拟时钟驱动**：waitForTimeout 累计时钟、时钟未达稳定阈值前每次采样位置都不同——移除等待即时钟不走、永不稳定、场景失败。红真实性实测：移除等待后该用例即红；恢复后绿。

维护回归 55 项全绿；本地实跑 22/22 断言通过。

### 十一轮复核更正：位置记录断言移至采集后（2026-10-27，`4f7d52c` 链）

`button_position_recorded` 此前在事后位置查询**前**执行且固定 true——注入查询失败后错误字段已正确保留，断言仍绿并声称「双位置均已入 evidence」（复核方探针 `review-pr52-round11/review-probes.json`）。断言移至 `buttonBoxAfterClick` 采集之后，按实际结果生成状态并区分三形态：查询失败（如实失败、detail 标注）、合法 null（解锁后按钮已随隐私框移除）、成功（两位置俱全）。实跑发现 Playwright `boundingBox` 对已移除元素自动等待至超时——解锁后先 `count()` 区分。维护回归 57 项全绿；本地实跑 22/22 断言通过。**本小节为补记**：`4f7d52c` 提交时漏写此处，已在十三轮补修提交一并补入。

### 十三轮复核补修：存在性与位置读取合并为单次页面内采集（2026-10-27）

复核方以真实 Edge 复现（`qa-artifacts/workbuddy/b3/review-pr52-round13/review-probes.json`）：`count()` 与 `boundingBox()` 分开执行——正常退出动画恰好在两次查询之间完成时，count=1 而 boundingBox 等待超时（TimeoutError），合法移除被误判为查询失败、位置断言失败（新偶发红灯源）。修复：解锁后位置采集合并为**单次** `page.evaluate`——单次读取内同时判定存在性与位置（元素缺失 `{exists:false}` → 合法 null；读取到坐标 → 位置对象；**通信异常整次抛错** → `buttonBoxAfterClickError`），不再存在两步窗口。红先：时序回归（count=1、采集时已移除、两步查询抛超时）在旧两步实现下误判查询失败（红），合并后判合法 null（绿）。维护回归 58 项全绿；本地实跑 22/22 断言通过，真实页面采集到解锁后按钮位置对象。

### 十四轮复核补修：事后采集定位同一同意按钮（2026-10-27）

复核方以真实 Edge 复现（`qa-artifacts/workbuddy/b3/review-pr52-round14/review-probes.json`）：单次采集的 `querySelector(".privacy-actions button")` 取**第一个**按钮——组件顺序（`PrivacyConsentDialog.vue:68`）中第一个是「不同意」。点击前记录的是同意按钮位置、事后采集的是不同意按钮位置，两份位置不可比较且断言仍绿（无动画双按钮页面上同意按钮 x=700、事后证据记录 x=100）。

修复：单次页面内采集保留，选择器改为与点击定位同一口径——`querySelectorAll(".privacy-actions button")` 按「同意并继续」文本过滤（`find`），前后是同一按钮；「正在保存…」等中间态文本不含该串时按不存在处理（合法 null），不会误取不同意按钮。

红先：回归桩注入含双按钮（不同意 x=100、同意并继续 x=700）的假 DOM 并**真实执行采集回调**（new Function 注入假 document，回调内选择器行为真实运行）——旧实现取第一个按钮 x=100（红），新实现取同意按钮 x=700（绿）。维护回归 59 项全绿；本地实跑 22/22 断言通过（解锁后同意按钮已移除 → buttonBoxAfterClick=null 合法形态，不误取不同意按钮位置）。

### 十五轮复核补修：按钮身份改为不随文案变化的结构标记（2026-10-27）

复核方以真实 Edge 复现（`qa-artifacts/workbuddy/b3/review-pr52-round15/review-probes.json`，受控反例、不据此归因历史 CI 失败）：同意按钮保存中会从「同意并继续」变为「正在保存…」而按钮仍在——两条取证路径都会失去它：正常采集按旧文案过滤，按钮仍在 x=700 却记 null 且断言记绿（与已移除无法区分）；失败取证用点击前按旧文案建立的 locator，卡在保存中·未解锁时按钮仍在却被记为取证超时。

修复：挂计数器注入时给同意按钮打**结构标记**（`.privacy-actions` 内最后一个按钮 → `dataset.ac06Consent`，不依赖文案），两条路径共用同一标识：

- 正常采集：`querySelector("[data-ac06-consent]")` 单次页面内读取（保留）；
- 失败取证：`buttonBoxAfterFailure` 改为同一标识的页面内采集（保留 8 秒 bounded 超时；不再用旧文案 locator）；
- 「正在保存…」等中间态：按钮仍在即取到位置；按钮移除即合法 null；通信异常记错误——三态与此前口径一致。

红先（真实 DOM 回归）：假 DOM 双按钮带 dataset，挂计数器与采集回调**真实执行**（标记逻辑与选择器行为真实运行）——保存中形态（同意按钮文案=「正在保存…」、位置 x=700）在旧文本过滤实现下记 null（红）、按标记取到 x=700（绿）；卡保存中·未解锁的失败取证在旧 locator 形态下记超时（红）、按标记取到位置（绿）；已移除形态记合法 null（守护）。红真实性经临时回退三处实现实测（两用例红）。维护回归 62 项全绿；本地实跑 22/22 断言通过。

### 十六轮复核：十五轮修复通过，PR #52 复盘收口（2026-10-27）

复核方以真实 Edge 受控反例独立验证（`qa-artifacts/workbuddy/b3/review-pr52-round16/review-probes.json`）：保存中采到同一按钮位置、移除返回 null、通信失败记录错误；失败取证挂起约 8 秒后跳过并保留原始超时与其他现场；最新 Native 产物保留前后位置。维护回归 62/62；HEAD `9a12ce2` 检查 18 pass + 1 skipped，CI / Native / Install 均首轮成功。复核范围内未发现新的阻断问题。

**PR #52 复盘收口状态**：版本 0.6.1，未合并（合并决定归用户）；AC07 继续暂缓，N10 保持开放；历史 Native CI 失败（无页面 target ×3、accept-order 解锁超时 **×3**——含取证能力补齐后 `8a239ea` 轮的 40416338270 首次尝试失败，取证能力补齐后未再出现）根因仍未定位，失败 run 与现场证据全部保留。

**隐私政策修订号决策（2026-10-28，记录待用户确认）**：docs/privacy.md 更新为 2026-10-28 版后仍为修订号 2——已同意旧版修订 2 的用户升级后不会重新看到新文本。按本次变更性质（措辞重写、CCS 预览访问时机与价格同步入口的表述更正，未改变数据收集范围与本地优先原则），**建议保持修订号 2**，仅作为文本澄清与实现对齐；若用户认为需要重新征得同意，应将 `PRIVACY_POLICY_REVISION` 提为 3 并走完整重新询问路径（同时更新政策文本修订号、Rust 常量与升级测试基线）。本 PR 未擅自提修订号。

**合并与发布条件（用户要求随收口记录）**：当前版本检查已返回 release=true；合并到 main 后，Release 会运行 **present、deleted 两条连续升级链**，构建与安装、升级检查全部成功后才进入发布——PR 的 Install 检查中 upgrade-chain 当前为 skipped（按 PR 契约），合并前的链路验证由合并后 Release 承担。

### 十八轮复核补修（2026-10-28，`0d09e13` + `d14a90b`）

复核方四项 P2 与五项 P3 逐项处理（受控反例 `review18-independent/`）：

- **P2 预置夹具非产品数据**：历史库改按 `history.rs` 的 `SCHEMA_V1` 常量生成（单一事实源；user_version=1、三行完整列事件），两份价格快照按产品加载器格式（`synced_at` 必填、modelsdev/openrouter 各自字段、各含一条带价条目）。实跑经 `scripts/inspect_history.py` 读出 schema 版本 1、`PRAGMA integrity_check=ok`；夹具回归升级为产品口径（六列存在、user_version=1、双快照 synced_at/entries/价格）。
- **P2 settings 快照非逐字节**：补 `bytesHex`（原始字节 hex）并纳入比较——同 size/mtime 的非法字节改写此前不可辨（`denied-byte-probe.json`）；回归先红后绿。
- **P2 政策与实现不符**：CCS 导入更正式为「点击预览即只读打开、确认后才写导入」；价格同步更正式为「同意本政策后自动周期 + 手动入口并存，关闭自动同步仅停周期」。
- **P2 政策渲染修复未入 PR**：复核方恢复的硬换行渲染修复（行尾反斜杠，含行内代码内的反斜杠不误判、粗体跨行）、顶部日期提取与 `policy-source` 抽取契约随 `0d09e13` 一并提交；**最终树三连截图确认**顶部日期无反斜杠、正文硬换行正确、无合并行。
- **terms 可见性**：`docs/terms.md` 打包为安装资源（`tauri.conf.json` resources）、`install-test` workflow paths 补该文件、构建输入清单同步（`build_inputs.py --check` 通过）。协议与 MIT 许可的关系、政策修订号是否提升留待用户确认（见 §6 收口段的修订号决策记录）。
- **P3 终态浮点缺牙齿**：新增「终态 revision = 2.0 必须失败且 detail 标 kind=float」回归——移除整数类型检查时该用例如实变红（69 项维护回归）。
- **P3 重复契约虚报**：rc09 输出按**唯一名称**计数并显示全量布尔，重复+漏选不再显示 28/28。
- **P3 修订号正则**：新增 `strip_rust_comments_and_strings` 词法剥离（嵌套块注释/行注释/普通与原始字符串），再对剥离结果匹配常量声明；0.6.0/HEAD 实测仍读得 2，假常量源解析为 2。
- **P3 研究文档比例**：两处「最多低估 60%」更正为相对正确费用低估 37.5%（即正确费用比估算高 60%）。
- **P3 政策更新提示双显**：`needs_consent` 阶段的 detail 只作 updateNotice，`blockedDetail` 不再重复返回；新增 privacyGate 回归。

**受测树与取证**：干净树 `d14a90b`（工作树干净）构建 EXE `ca6cdcd4ec180836a7796d1c0846f70fc74b7cca195946893891d66dea13705c`；五场景三连 **15/15 全绿**（preseeded 22/21/21、denied-ipc 6/6/6、accept-order 9/8/8、variants-corrupt 6/6/5、variants-close 7/7/7，最差单次之和 53 秒）；preseeded 观察 15009ms、双快照零变化；前端全量 869 项、升级测试全绿。此前汇报的运行名映射更正为 CI 40560715280 / Native 40560714923 / Install 40560714998（九轮次），本批以最终推送后的实际运行名为准。

### B2 合并与发布验收（2026-10-22）

- [PR #31](https://github.com/ywltby/tokenscope/pull/31) 最终 HEAD `7664be9`，普通合并为 `c9de40ec3ded2485d2991079f209699ae6ff6107`；两父提交分别为最新 main `9c77cef` 与该 PR HEAD，合并文件树和已验证 HEAD 完全一致。AC03 / AC04 / AC05 分别提交 `6f50af1` / `16ec813` / `db43355`；局部峰谷费率展示修复 `4159633`、经批准的端口和 CI 契约登记 `317573e`、版本登记 `bfb01ed`、短路径取证补修 `7664be9` 各自独立。
- 最终完整钩子全通过：普通与 acceptance 两套 workspace、前端 772 项、浏览器契约、Python 310 项、PowerShell 安装 26 项和升级 60 项等；`pricing-canonical-full-hook.log` 保留完整复验，原端口失败、短路径 CI 失败与本地漏嵌入特征的启动失败均单独保留。具名业务断言首次先红为 10/9/10，三个判定套件最终 36/29/47，模板取证装配另 5 项；没有删除其他旧用例、放宽阈值或增加等待。
- 最终本地三连跑：AC03 为 31.282 / 31.648 / 29.381 秒，AC04 为 17.489 / 17.462 / 17.181 秒，短路径补修后的 AC05 为 25.534 / 24.246 / 23.979 秒，各次断言为 16/15/16。固定代码三次之间无改动，最长单次之和 74.671 秒。受测提交、0.5.8 / 0.5.9 acceptance 产物 SHA256、命令、失败与复验位置均在下方逐任务记录，汇总为 `qa-artifacts/acceptance-closure/b2/local-stable-summary.json`。
- PR 原生产物 `12415284614` 下载 ZIP SHA256 `268df1800c226b6f00531666c9b879a7ff3b15d2c06a7878e7050375a9d5e67d` 与 GitHub 一致；全部九份场景 evidence 检查通过，路径违例与代理尝试为零、真实目录指纹一致、launches 全部退出释放。实际合并测试提交 `1cae8cc` 的 0.5.9 acceptance EXE SHA256 为 `ad8259d2ff52d60641288e51d8ed7b80ea7f805dd7abe895ee07e201060961ae`，证据在 `pr-7664be9-native/`。其中真实 CI 短路径展开后的期望、返回和 opener 路径一致。三个新增步骤实际为 60/24/32 秒，合计 116 秒，低于 8 分钟；45 分钟上限和旧步骤不变。

| 阶段 | 运行 | 结论 |
| --- | --- | --- |
| PR #31 / `7664be9` | [CI 39756585501](https://github.com/ywltby/tokenscope/actions/runs/39756585501) | success；四组和 gate 全通过 |
| PR #31 / `7664be9` | [Native UI E2E 39756585449](https://github.com/ywltby/tokenscope/actions/runs/39756585449) | success；三个新增步骤合计 116 秒 |
| PR #31 / `7664be9` | [Install test 39756585577](https://github.com/ywltby/tokenscope/actions/runs/39756585577) | success；安装五场景、六组单跳通过，连续链按 PR 契约 skipped |
| main / `c9de40e` | [CI 39758431511](https://github.com/ywltby/tokenscope/actions/runs/39758431511) | success；四组和 gate 全通过 |
| main / `c9de40e` | [Native UI E2E 39758431422](https://github.com/ywltby/tokenscope/actions/runs/39758431422) | success；三个新增步骤为 30/17/21 秒，合计 68 秒 |
| main / `c9de40e` | [Release 39758431613](https://github.com/ywltby/tokenscope/actions/runs/39758431613) | success；生产构建、安装五场景、六组单跳、两条连续链及 publish 全通过 |

- main 原生产物 `12416837710` 的 ZIP SHA256 `9776123b47147721df8c94be864520f2af5efe2e6db07349b5f4670ee31b61ed` 已下载核对；九份 evidence 全通过，全部 launches 退出释放，路径违例/代理尝试为零、真实目录指纹一致。`c9de40e` 的 acceptance EXE SHA256 为 `095e3546df80031b4e12c7a841452465106ac408d359373ea027edcf8e7cb720`，证据在 `main-c9de40e-native/`。main 新增 CI 68 秒与 PR 116 秒均在 8 分钟内。
- 本 PR 只递增一次 patch 到 0.5.9；v0.5.8 已在 main 登记，合并前 `--check --base-ref origin/main` 实测 release=true。[v0.5.9](https://github.com/ywltby/tokenscope/releases/tag/v0.5.9) 于 2026-10-22 09:59:15 UTC 公开（按现有发布流程标为 prerelease），标签解引用为合并提交 `c9de40ec3ded2485d2991079f209699ae6ff6107`。四项原资产经 `upgrade_baselines.py add --tag v0.5.9` 逐个下载核对后登记，未重构或覆盖旧资产；安装包 `TokenScope_v0.5.9_windows-x64-setup.exe` SHA256 为 `6d2dbf3f769e83c17dee0629301433584f67cce22bdd2dc4935f2e4f69b18de0`。
- 原资产另三项 SHA256：`build-metadata.json` 为 `d9e5dbde5d6e47301387cb696eb21ca7a716470dda127aec65ade4c7ff01563d`，`SHA256SUMS.txt` 为 `4c0c77c66ac0b599f4933e04ef14bd13fda288d8a2c19bddaf691c5528ac15b2`，`provenance.sigstore.json` 为 `de319a3c4c1cd9a34af88b25f24046b5f57fd564e9ef7fc6c27ae0bdf7dd66b9`。基线登记日志在结果 worktree 的 `qa-artifacts/acceptance-closure/b2-results/add-v0.5.9.log`，PR/main 原生、红绿和三连跑证据仍保留在 `acceptance-b2` worktree 的 `qa-artifacts/acceptance-closure/b2/`。
- N11 的自动化组合验收由 B2 交付，安装正式版本后手动同步 OpenRouter 的人工尾项仍按用户决定待验；N11 不关闭。B3 的两项事实差异已在下方记录，尚待用户修订；N10、N13 和 N12 系统/签字没有因本批自动化变成完成。N01–N09、N14–N16 仍已完成，四个开放责任编号不变。

结果回填 PR 的 §3 不变量自审（只含上述证据回填和 v0.5.9 基线登记）：

| 编号 | 结论 | 依据 |
| --- | --- | --- |
| 1 普通构建零变化 | 成立 | 只修改计划、验收表和升级基线登记，没有产品输入或构建接线差异 |
| 2 真实数据零接触 | 成立 | 本批仅下载 GitHub 原资产和读取既有隔离证据，不启动新的原生运行；B2 PR/main 的路径、代理与真实目录检查均通过 |
| 3 CCS 只手动 | 不适用 | 不修改或运行 CCS 导入；此前 AC04 结果按原证据回填 |
| 4 历史不随来源消失 | 不适用 | 不修改历史库行为或来源文件；此前 AC03 结果按原证据回填 |
| 5 同意先于业务 | 不适用 | 不修改同意流程，也不代签 B3 的缺口 |
| 6 证据可复查 | 成立 | 逐项核对 PR/main 运行、九份 evidence 和 launches、产物与四项原资产 SHA256；失败日志与修复后复验分别保留 |
| 7 CI 预算 | 成立 | 不新增 CI 步骤或等待；B2 新增步骤 PR 为 116 秒、main 为 68 秒，45 分钟上限不变 |
| 8 版本规则 | 成立 | 保持 0.5.9，release-change.json 与基线逐字节一致，版本检查 release=false；19 个已发布基线复查通过、无警告 |

### 起步复核与计划合入

- 计划提交：`49b7b06`、`1e56163`，以及用户修订 `4d27471`（保留旧预热工具）、`6a4dcab`（区分默认巡检与独立契约）。[PR #26](https://github.com/ywltby/tokenscope/pull/26) 普通合并为 `316bd70`；PR 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39622736925) 与 [Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39622736784) 均成功。
- main 同提交的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39623896792)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39623896684)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39623896703) 均成功；版本保持 0.5.6，Release 六个下游全部 skipped。
- §1 已按源码与历史运行元数据复核：受保护命令 35 个、驱动 2716 行、默认巡检 15 个 fixture/注册 23 个、独立契约引用额外 8 个 fixture；PR #25 原生作业 09:29:25–09:37:45 UTC，实际 8 分 20 秒。确认 CCS 默认路径未隔离、价格 URL 常量与 ureq 环境代理/NO_PROXY 支持、opener 真实调用、隐私取证缺口及旧预热工具用途。未运行旧预热工具，未读取真实 CCS 库。AC08 的实际覆盖仍须后续运行产物证明。
- 执行授权以用户本次指令为准：批次自审成立且检查全绿后用普通 merge commit 合并，批次之间不等待确认；明确停点与 AC11 人工签字仍须停下。

### B1 / AC01：读取失败 fixture（共享冲突，已交付，N14 关闭）

初次 ACL 取证（历史，已停用）：

- 基线：最新 main `316bd70`，分支 `test/acceptance-b1-permission`，原有采集套件 16/16 通过。只在 `source_paths_collection.rs` 末尾追加 ACL guard 与一个 Windows 用例；既有用例和断言未删改，产品源码无最终差异。
- 真实 ACL：在用例自己的临时根对子目录与 JSONL 文件执行 `icacls /deny <当前用户>:(RX)`，直接从 PowerShell 启动的定向测试中，真实 `read_dir`/文件读取均确认为 `PermissionDenied`。guard 在正常结束与 panic 时撤销 deny；此前失败现场的四组临时目录/文件经只读复核均恢复可读（`panic-acl-restoration.json`）。提交钩子的环境差异另见下方阻断记录。
- 变异红灯：目录读取错误分支临时改为直接 `return`，用例因缺少“无法读取目录”诊断失败（`red-directory.log`）；Claude 文件读取错误分支临时不累计 `io_errors`，用例明确报实际 0、期望 1（`red-file.log`）。两个变异均已逐字节恢复，未带入提交。
- 绿色：普通与 acceptance 构建的采集套件均 17/17。实测两个可读文件完整入账：2 请求、input/output/cache_write/cache_read = 40/20/0/0，`io_errors = 1`，目录与文件拒绝均有可见诊断。测试将实测值写到 `AC01_OBSERVED` 日志，并汇入 `evidence-acl-history.json`。
- 失败归因：新用例首次在删除临时库时遇到 Windows code 32，原因是查询注册表按设计保留只读事务；仅回收本用例会话后再严格清理，未忽略删除错误。变异恢复曾因 `Copy-Item` 保留旧 mtime，使 Cargo 复用变异产物；核对恢复文件 SHA256 与备份一致、刷新源码 mtime 并明确重新编译后通过。两项均属本地测试取证流程问题，不是产品缺陷，没有用无归因重跑替代处理。
- **提交钩子阻断**：`git commit` 的 `cargo test --locked --workspace` 在新增用例的目录拒绝自检失败（16 通过、1 失败）：实际 `None`，期望 `Some(PermissionDenied)`，位置 `source_paths_collection.rs:826`。失败发生在 `summary` 调用之前，没有产品缺陷证据。钩子拒绝提交，未生成 B1 提交、未 push、未开 B1 PR。
- 环境取证：同一账户、同一份独立合成 ACL 探针，在 PowerShell 下目录读取被拒绝，经 Git `sh` 启动后目录可列举；文件读取在两边都被拒绝。`powershell-privileges.log` 中 `SeBackupPrivilege`/`SeRestorePrivilege` 均 Disabled，`git-sh-privileges.log` 中均 Enabled，吻合目录枚举绕过 deny 的现象。探针结果分别保存在 `acl-environment-powershell.json`、`acl-environment-git-sh.json`；未把再次运行通过作为结论。
- 此前停点（已由用户修订）：AC01 的管理员绕过自检在钩子中明确失败，依“计划某一步无法按原样执行”停下。用户不采用关闭线程备份特权的建议，明确改用只依赖 `std` 的共享冲突；未改线程令牌、全局权限或钩子。
- 清理与证据：钩子失败用例的目录与文件均恢复可读；独立探针的四个 deny ACE 已通过仅删除对应规则恢复，两个探针根已清理（`acl-probe-cleanup.json`）。失败测试二进制 SHA256 为 `df9a1a12c73a73beda92c274808db4a86748387d1df46bb45679d9de2c4f2347`，日志为 `commit-checks.log`。本批没有原生应用启动，`launches[]` 为空。
- 命令：`cargo test --locked -p tokenscope --test source_paths_collection -- --show-output`，以及同命令增加 `--features acceptance`；均设置 `CARGO_NET_OFFLINE=true`。证据位于本 worktree 的 `qa-artifacts/acceptance-closure/b1/`。普通/acceptance 测试二进制 SHA256 分别为 `9c1ff0a2ce79c6e0e377ba2a7a8d8903fec9129870fa85318f25ba115832666a` / `462739934d067e5be400c17d260c58b30512b642799a6c4a51c46fc8a2c38d28`；新增测试源码 SHA256 为 `e5d24d07fdb6be408ed6eeabd078acbd8151c4cf8634dba80010e1b5c4754f7b`。这两份产物来自该基线加本批追加测试，完整信息见 `evidence-acl-history.json`。
- 当时尚未进入远端：没有新增原生场景，本地新增 ACL 用例约 0.08 秒；最终交付改以以下共享冲突实现及其 PR/main 运行结果为准。

共享冲突取证（本次交付依据）：

- 独立探针 `shared-lock-probe.rs` 仅用 `std`，通过 `rustc --edition 2024` 编译；PowerShell 与 Git `sh` 各运行一次，每次连续三轮，目录与文件共 12 次读取均因 Windows error 32 失败，释放句柄后全部恢复，临时根均已删除。日志为 `shared-lock-probe-powershell.log` / `shared-lock-probe-git-sh.log`；探针二进制 SHA256 为 `90577eaad25e19b4e3eec5ffede6dd5b06a512b3046739766162da66165548f4`。
- 新用例改名 `unreadable_subdir_and_file_are_diagnosed_without_stopping_the_round`，用 `OpenOptionsExt::share_mode(0)` 持有两个 `File`；目录另加 `FILE_FLAG_BACKUP_SEMANTICS`。持锁期间真实读取必须失败，两个句柄持续覆盖 `summary`，正常与 panic 都由 RAII 释放；原有采集结果断言保留。
- 两个变异重做：目录读错分支提前 `return`，因缺少“无法读取目录”诊断失败（`sharing-red-directory.log`）；文件读错分支漏计 `io_errors`，实际 0、期望 1（`sharing-red-file.log`）。均退出 101，随后恢复产品源码，`git diff HEAD -- crates/tokenscope-core/src/source/{mod,claude}.rs` 无差异。
- 恢复后普通与 acceptance 采集套件均 17/17（`sharing-green-default.log` / `sharing-green-acceptance.log`）。实测：目录与文件读取错误码均 32，2 请求，四桶 40/20/0/0，`io_errors = 1`，两种诊断可见。基线 `316bd70` 加本批测试；源码 SHA256 `843dac044b4d877031935ec0f5d9ac06ca3f4688b081d459b70d5a56248c8c6d`，普通/acceptance 测试二进制 SHA256 `47edfb0bbb9f0eb7aa5c61c098e42bc61af2d088f4bea2a6cde376d8c5586553` / `d8d4c5501701dd74720f521416f8c5a906a5cd309ec1cffb4af4e2924edb89a1`。完整实测值及命令在 `evidence.json`，旧 ACL 证据单列保留。
- 钩子首次检查发现新增目录自检写成 `.err().expect()`，触发 Clippy `err_expect`；改为等价的 `.expect_err()` 后重新验证，两套定向测试仍各 17/17。该修正仅涉及新用例的语法，未放宽或删除断言，原失败日志保存为 `sharing-clippy-initial.log`。
- 既有 16 个用例保持原样；没有产品输入、Cargo 清单或锁文件变更，没有新增原生 CI 场景。最终提交 `69ce2f4` 的完整钩子通过普通与 acceptance 两套 workspace Clippy/test、fmt 和文档检查，工作区干净后推送。独立复核无 Critical/Important；提出的旧证据链接与用语已修正。
- [PR #27](https://github.com/ywltby/tokenscope/pull/27) 普通合并为 `4198051e0408126dc978c2781937974c05ba6ecc`；合并前最新 main `316bd70` 是 HEAD 祖先，八条不变量结论已写入 PR 正文及下表。合并提交与最终 PR HEAD 的文件树完全一致（`0b18802c724b52951de2ccb7c1719fdc38eea19e`）。

| 阶段 | 运行 | 结果 |
| --- | --- | --- |
| PR #27 | [CI 39630887221](https://github.com/ywltby/tokenscope/actions/runs/39630887221) | success；四组及 gate 全通过 |
| PR #27 | [Native UI E2E 39630887255](https://github.com/ywltby/tokenscope/actions/runs/39630887255) | success；8 分 26 秒 |
| PR #27 | [Install test 39630887250](https://github.com/ywltby/tokenscope/actions/runs/39630887250) | success；安装五场景 12 分 22 秒，起点规划 11 秒，六组单跳各 58–67 秒；连续链按契约 skipped |
| main `4198051` | [CI 39633188944](https://github.com/ywltby/tokenscope/actions/runs/39633188944) | success；四组及 gate 全通过 |
| main `4198051` | [Native UI E2E 39633188991](https://github.com/ywltby/tokenscope/actions/runs/39633188991) | success；7 分 10 秒 |
| main `4198051` | [Release 39633189036](https://github.com/ywltby/tokenscope/actions/runs/39633189036) | success；gate 通过，六个下游全部 skipped |

- 版本未变，`--check --base-ref origin/main` 在交付前输出 `release: false`；合并后发布列表最新仍为 [v0.5.6](https://github.com/ywltby/tokenscope/releases/tag/v0.5.6)，没有新发布。新增原生 CI 时长为 0，新增 Rust 用例定向运行约 0.05 秒，未改变 45 分钟上限。
- N14 第 4 项按用户修订的共享冲突验收通过，N14 四项全部关闭；整合计划 N 行、§7 执行账与总账同步回填。N10、N11、N12、N13 仍开放。
- B1 回填自审：只有本计划、整合计划和总账三份文档；不变量 1、2、6、7、8 成立（无产品或数据访问变化、运行结论与链接逐项核对、无新增 CI 场景、`release: false`），3、4、5 不适用。历史执行账保留原时点结论，当前状态统一为 12 完成、1 部分完成、3 专项待验。

合并前 §3 不变量自审（PR 正文与本节均已记录）：

| 编号 | 结论 | 依据 |
| --- | --- | --- |
| 1 普通构建零变化 | 成立 | 最终只追加测试与文档，两个临时产品变异已还原 |
| 2 真实数据零接触 | 成立；新增追踪/代理接线不适用 | 新用例的来源、历史库、价格与索引均注入临时根，探针与新用例未启动原生应用；既有隔离原生 CI 全通过 |
| 3 CCS 只手动 | 不适用 | 本批没有 CCS 调用或代码变更 |
| 4 历史不随来源消失 | 不适用 | 本批只验读取失败，不改变历史保留逻辑 |
| 5 同意先于业务 | 不适用 | 没有隐私接线或原生启动变更 |
| 6 证据可复查 | 成立 | 具名断言、两种环境的共享冲突探针、自检实测值、两个新变异失败日志与产物哈希均保存；旧 ACL 失败保留归因 |
| 7 CI 预算 | 成立 | 原生场景与 45 分钟上限均未改，新增用例定向运行约 0.05 秒 |
| 8 版本规则 | 成立 | 最终改动只有测试与文档；已实测 `--check --base-ref origin/main` 输出 `release: false` |

### B2 / AC02：验收接线（进行中）

- 基线：最新 main `87d97db`（B1 回填已合入），分支 `feat/acceptance-b2-unified`，独立 worktree `acceptance-b2`；构建产物使用该树自己的 `target/`，避免与正在其他修复分支工作的主工作区共用产物。前端 frozen/offline 安装 121 个缓存包、下载 0，构建通过；现有 `native_acceptance` 基线为 3 通过、2 个由父用例调用的子阶段 ignored。
- 首次红灯：先新增 `acceptance_closure.rs` 的两条子进程路径断言，再运行 `cargo test --locked -p tokenscope --features acceptance --test acceptance_closure -- --show-output`（`CARGO_NET_OFFLINE=true`）。结果 2 失败、2 子阶段 ignored，退出 101：bootstrap 后 CCS 默认路径仍为真实主目录路径；未 bootstrap 时也返回 `Ok`，未拒绝。证据 `qa-artifacts/acceptance-closure/b2/ac02-red-ccs.log`。此阶段只比较路径与返回值，未调用 `source_exists`、未打开 CCS、未请求网络；产品源码尚未改动。
- 后续先补齐同一任务的价格地址、追踪序列与普通构建忽略新变量断言，再实现 acceptance 接线；AC03–AC05 及本地三连跑尚未执行，N11 不提前关闭。
- CCS 合成器先红：`python -B -m unittest discover -s scripts -p test_make_ccs_fixture.py` 失败 1 项，缺少 `make_ccs_fixture.py`（子进程退出 2）；断言要求重叠请求、独有请求、日汇总、只读访问与拒绝覆盖。证据 `ac02-red-ccs-maker.log`；该断言先于合成器实现，全部路径在测试临时目录。
- 壳侧先红：`cargo test --locked -p tokenscope-app --features acceptance shell_consent_trace_precedes_business_io_in_an_isolated_process -- --show-output` 退出 101；实际追踪已有 `settings_write`、`history_open`、`opener`，但缺少 `consent_saved`，故顺序断言失败。证据 `ac02-red-shell.log`，此时尚未修改隐私门禁接线；本测试没有启动 GUI、没有发起价格请求。
- 合成器新增测试首次实现后的失败属于测试架连接生命周期：Python SQLite 事务上下文不负责关闭连接，Windows 清理报共享冲突；改用 `contextlib.closing` 显式关闭，原断言全部保留，定向测试通过。它只处理新增合成器与新增测试的临时库，未触碰任何现有测试。
- 驱动 I/O 基础设施先红：`node --test frontend/scripts/lib/acceptance-io.test.mjs` 两项因尚无该模块而失败（`ERR_MODULE_NOT_FOUND`，退出 1），证据 `ac02-red-driver-io.log`。待实现断言包括本地端点命中与 ok/500/down、零代理尝试、追踪顺序与不完整追加行的区分。
- unified profile 先红：`python -B -m unittest discover -s scripts -p test_acceptance_unified_profile.py` 失败 1 项，准备脚本尚不接受 `-Profile`（退出 1）；证据 `ac02-red-profile.log`。新断言核对 8 条合成 Claude 事件、模型异写、272000/272001 边界、分段/峰谷/显式策略、不完整候选与两张 CCS 表，全部输出在临时根。
- AC02 定向绿灯：核心库 4 通过、4 子阶段由父用例调用（ignored）；两源本地桩各命中 1 次、代理尝试 0，8 条追踪按序且所有文件路径在隔离根内；壳侧顺序与重复同意测试通过，普通构建忽略变量测试通过，驱动 I/O 两项与合成器/profile 各一项通过。普通构建再做临时变异（让 URL getter 读取新变量），新增测试退出 101 并显示 `invalid-and-must-be-ignored`；随后逐字节恢复源码，尚待最终完整门禁。证据依次为 `ac02-green-core.log`、`ac02-green-shell.log`、`ac02-green-default.log`、`ac02-green-driver-io.log`、`ac02-green-ccs-maker.log`、`ac02-green-profile.log`、`ac02-red-default-mutation.log`。
- 落盘设置指纹明确使用 `fnv1a64`（既有依赖内实现、标准向量校验，随事件写算法名），仅证明设置字节身份；发布资产与受测二进制仍使用 SHA256。`business_ready` 记录的是同意已允许初始化的边界，附 `privacy_phase: starting` 与 `scope: runtime_initialization`，UI 的 Ready 发布时点未改。
- 完整脚本自查：`python -B -m unittest discover -s scripts -p "test_*.py"` 304 项全通过。首次 acceptance clippy 发现新增测试 Drop 中可合并的嵌套条件，已等价修正，后续提交钩子仍按 `-D warnings` 执行。最新 fetch 的 main 仍为 `87d97db`、最新发布仍为 v0.5.6，上一版已登记；B2 按规则只递增一次到 0.5.7，锁文件只有两个本地包的版本变化，没有新增或升级依赖。
- 提交前只读复核发现三处新增验收基础设施缺口：继承的 `ALL_PROXY/all_proxy` 可绕过计数代理；隔离违例先取消会挡住既有 `stop` 的完整收尾；缺失 path 的路径类追踪可被假通过。此时没有原生运行。先追加回归断言，`ac02-red-review-regressions.log` 实测 3 失败、2 通过（ALL_PROXY 未覆盖、history_open 缺 path 被接受、尚无可等待的隔离收尾入口）；另 `ac02-red-all-proxy.log` 保留纯配置探针的失败。修法只补新接线与新断言，不放宽旧取消规则。
- 首次完整提交钩子已完成两套 Rust fmt/clippy/test、前端 typecheck/格式检查，但被新增测试框架接入错误拦住：`acceptance-io.test.mjs` 使用 node:test，既有 Vitest 发现该文件却没有可运行 suite；其余 49 个文件 / 620 项通过，提交未产生。已将新增文件接入项目既有的 Vitest node 环境，不修改既有测试或 Vitest 配置；该测试随 frontend 门禁执行，取消本次新增的重复 node:test CI 命令。完整失败日志保留 `ac02-commit-hook.log`，后续另存复验日志。
- 追踪追加边界先红：新子进程测试预置“完整 JSON、没有末尾换行”的旧日志，要求启动拒绝且原字节不变；旧接线返回 `Ok`，定向运行退出 101（`ac02-red-trace-newline.log`）。同时追加双进程重启续号与单调时间断言，避免只验证单次进程内顺序。
- 复核补修定向结果：`ac02-green-review-regressions.log` 记录 5 项 Vitest 通过；`ac02-green-core-reviewed.log` 为 6 通过、6 子阶段由父用例调用（ignored），包含末尾换行拒绝与重启续号，两源仍各 1 次本地命中、代理尝试 0。路径类事件现在必须带绝对 path，`pricing_fetch` 也附真实快照路径；进入业务的 UI 场景要求必要追踪存在，未同意场景仍允许空。隔离失败的终止与既有完整 stop 在取消前进入，随后禁止业务/重启，最终等待两项收尾；旧 stop 取消检查未改。另追加强杀失败仍等待退出确认的断言与 sync-failure 全代理注入核对，随完整门禁复验。
- 第二轮只读复核指出首次强杀也必须先补录后代：宿主退出后，既有保护会停止从宿主 PID 继续发现后代，不能先杀再补录。新增冻结后代的回归先红（`ac02-red-descendant-freeze.log`，1 失败、6 通过），再补强杀前刷新、终止后等待完整 stop；若探测失败仍终止已知归属进程，但必须报告后代身份未确认，不得把退出确认为成功。保留旧宿主退出保护、超时预算与取消规则。
- 上述驱动回归最终定向 7 项全通过（`ac02-green-descendant-freeze.log`）；准备重新执行完整提交门禁。此前没有产生 AC02 提交，也没有原生运行或真实价格请求。
- 完整提交门禁复跑时被既有安装契约拦住（`ac02-commit-hook-reviewed.log`：26 项中“观察期提前退出（退出码 23）”未抛出）。归因：退出替身 `pwsh -Command "exit N"` 冷启动可能超过 1 秒观察期，观察结束时仍存活；受控反例（子进程延迟 1.5 秒）复现该现象（`install-observation-probe.json`），没有产品或辅助函数缺陷证据。按“测试架缺陷先修测试架”单独提交 `dc9f241`：替身先写就绪文件、在等待/观察的同一调用块内放行后才退出，与 `4cde2b3` 的 GUI 替身就绪握手同类；断言、1 秒观察期与 10 秒等待期、`install_test_helpers.ps1` 均未改，不属于等待补偿。复验：PS7 契约 26/26；临时副本把子进程启动额外拖慢 1.5 秒、3 秒，均 26/26；该提交的选择性钩子（脚本契约）通过。
- 随后 AC02 完整提交门禁的两套 Rust、前端 627 项与浏览器契约均通过，只有新增 `test_acceptance_unified_profile.py` 报错（`ac02-commit-hook-final.log`）：重定向时 pwsh 按控制台代码页（本机 936）输出中文，测试以 UTF-8 文本解码，读线程抛 `UnicodeDecodeError`，`stdout` 为 `None` 掩盖了结果。准备脚本单独运行退出码为 0、产物正确；此前通过的运行处于 65001 代码页。该测试是本批新增，改为按字节捕获、只在诊断文本中以 `errors="replace"` 解码（同 `test_upgrade_workflow.py`），断言未改，既有准备脚本未改。修前在 936 下定向复现报错，修后 936 与 65001 下均通过。

### B2 续行：主线同步与场景交付（2026-10-22）

- `bfb01ed` 的 [CI 39752754503](https://github.com/ywltby/tokenscope/actions/runs/39752754503) 与 [Install test 39752754511](https://github.com/ywltby/tokenscope/actions/runs/39752754511) 成功，[Native UI E2E 39752754517](https://github.com/ywltby/tokenscope/actions/runs/39752754517) 的唯一失败为 `open_pricing_file_creates_template_in_root`。原生产物 `12414554016` 下载 SHA256 与 GitHub 一致（`ae1c5cd59aba9857e4ee961f25715e7b9358545656f5a4d81969065b016f4318`），取证在 `pr-bfb01ed-native/pricing/ui-pricing/evidence.json`，整份作业日志为 `pr-bfb01ed-native-ui.log`。
- 归因属于测试装配：隔离根期望字符串含 `C:\Users\RUNNER~1`，后端返回与 opener 追踪含 `C:\Users\runneradmin`，二者是 Windows 短名与展开长名；文件已创建、record 模式与进程探针正确，外部进程列表为空。既有追踪校验使用真实路径规范化，路径违例与代理尝试均为零、真实目录指纹不变；两次 launches 都确认退出并释放端口，第二次的 `FFFFFFFF` 对应 `scenario-end / terminate=true` 主动清理。没有产品缺陷证据，不重跑旧 CI。
- 最小装配修复只从已创建文件的固定预期位置调用 `fs.realpathSync.native`，保留 `rawExpectedPath`；后端返回值、opener 路径与原 `pricingVerdict` 独立核对，原判定、既有用例、阈值和窗口无改动。新增测试直接执行场景实际的 expectedPath 属性表达式：短名/长名、缺文件、规范化报错、错误文件/越根 opener 与真实临时文件。修前 4 失败、1 通过（`pricing-short-path-red.log`），修后连同原 47 项共 52/52（`pricing-short-path-green.log`）；独立复核无重要问题。
- 本地重建第一次漏带 CI 的 `tauri/custom-protocol`，首轮在启动前置只得到 `chrome-error://chromewebdata/`，没有进入业务，后两轮立即停止；隔离检查通过，失败 evidence/launches 保留在 `ac05-short-stable-1`，该轮不作为红灯或场景稳定性证据。核对 CI 原命令后以 `cargo build --locked -p tokenscope-app --release --features acceptance,tauri/custom-protocol --bin tokenscope` 重建（`pricing-short-path-rebuild-embedded.log`），没有增加等待或改断言。
- 随后固定代码重新三连跑 `ui-pricing --port 9520 --events 8`，证据目录 `ac05-canonical-stable-1/2/3`，耗时 25.534 / 24.246 / 23.979 秒，均 16/16；每轮两次启动全部退出释放，所有隔离检查通过。受测产品为 `bfb01ed` 的 0.5.9 acceptance 嵌入构建加本批测试装配，EXE SHA256 `c9b7bbe0e1da394bc2edb7d0be58f745009654fa68884d679409506f35f6068c`，三轮场景与 EXE 哈希逐轮一致（`ac05-canonical-stable-times.json`）。与 AC03、AC04 各自最长单次之和为 74.671 秒；此前 AC05 三连跑保留历史，最终依据更新为本段。最终远端检查仍须在补修提交上重新取得成功。

- CI 装配最终补修独立提交 `317573e`：首次完整交付钩子在前端 766 通过、1 失败后退出，旧端口清单只登记六个端口，新增 9500/9510/9520 不在期望中；原失败保存 `final-delivery-commit-hook.log`。用户明确批准仅追加三端口及 ui-identity / ui-ccs-import / ui-pricing 的 2/1/2 次启动注释，`stepSize = 3` 和后续两两不重叠循环逐字保留，不属于放宽断言。定向端口套件 28/28（`port-registry-green.log`）。
- 独立复核另发现新工作流契约未被远端 contracts 组枚举调用：追加接线断言先为 1 失败、5 通过（`contracts-dispatch-red.log`），按现有写法仅追加指定模块调用和退出码传播后 6/6（`contracts-dispatch-green.log`），其他调用逐字保留。`317573e` 将三场景 CI 步骤、契约和两项登记补修作为同一测试装配提交，不混入版本或产品改动；完整钩子 `ci-assembly-full-hook.log` 全通过：普通/acceptance 两套 workspace、前端 767 项、浏览器契约、脚本 310 项、PowerShell 安装 26 项与升级 60 项等。修复经过独立静态复核，无剩余重要问题，没有以重跑替代归因。
- 版本和本节执行记录独立提交前再次 fetch，main 仍为 `9c77cef` / 0.5.8；恢复此前已统一取好的 0.5.9，没有再 bump，版本检查仍为 `release: true`。待最终提交全部必需检查成功后转为 Ready 并普通合并，远端与 main 运行结论另行回填。
- B3 只读准备发现原计划 AC06/AC07 的前提不符，已报告并停止，未执行原生、注册表操作或补接线。acceptance 只改 identifier，autostart 默认名仍来自 productName `TokenScope`，锁定依赖先尝试 HKLM 再退 HKCU，且涉及同名 StartupApproved；仅监测 acceptance identifier 的 HKCU 值无法覆盖实际目标。价格线程先等待 120 秒，后续间隔 3600 秒，快照年龄须未知或至少 24 小时，重新开启设置不唤醒线程；`pricing_fetch` 也不能证明已发生一个关闭状态的检查周期。原 300 秒场景预算下无法按原 AC07 顺序完成。待用户修订 AC06/AC07，B2 独立继续，不自行改验收方案。

- B2 CI 接入的工作流契约先在旧 YAML 上失败：缺少三场景的独立步骤，均为明确断言失败（`ac-ci-red.log`）；追加三步后 5/5 通过，真实 PowerShell 替身验证原 run 体的 0/37 退出码和参数传递。替身裸数字类型的初始装配错误另存 `ac-ci-stub-type-failure.log`，不作为先红证据。新增 Python 调用按字节捕获 pwsh，仅诊断以 `errors="replace"` 解码，936 与 65001 两种实际代码页各 5/5（`ac-ci-cp936.log`、`ac-ci-cp65001.log`）。CI 各一步、端口段 9500/9510/9520，证据子目录为既有 native-e2e 根下的 identity/ccs-import/pricing，原 45 分钟及 always 上传不变。
- 最终取号前再次 fetch，origin/main 仍为 `9c77cef` / 0.5.8，上一版原资产基线已经在 main 登记，无须重复 add。B2 统一递增一次至 0.5.9；`python -B scripts/bump_version.py --check --base-ref origin/main` 返回 previous=0.5.8、kind=patch、release=true（`final-version-check.log`）。草稿中间提交 `4159633` 的 CI contracts 失败已读日志确认为尚未取号（要求 0.5.9，实际 0.5.8），其 Rust、acceptance、frontend 和原生均通过；最终提交仍须重新取得全部必需检查成功，不重跑旧提交掩盖结果。

B2 合并前八条不变量自审（远端检查与合并后运行另行记录）：

| 不变量 | 结论 | 依据 |
| --- | --- | --- |
| 1 普通构建零变化 | 成立（验收接线范围） | 新变量、路径、价格地址与 opener 接线受 acceptance cfg 限定，普通构建忽略变量的独立子进程测试通过。另有已授权局部 UI 缺陷修复 `4159633`，仅追加费率展示，不改统计或计价 |
| 2 真实数据零接触 | 成立 | 九次最终稳定原生运行追踪路径全部在隔离根、代理尝试零、真实目录指纹前后一致；没有访问真实价格源或真实来源库 |
| 3 CCS 只手动 | 成立 | AC04 预览前无 ccs_open，取消不写，重复/改时区不增用量，来源库哈希前后一致，三连跑通过 |
| 4 历史不随来源消失 | 成立 | AC03 删除一个真实合成文件后新后端轮次成功，重扫、重建索引及停用来源后请求与四桶保持，三连跑通过 |
| 5 同意先于业务 | 本批强取证不适用 | B2 原生 fixture 预置同意；AC02 已有核心/壳侧顺序回归和普通隐私门禁回归。未同意预置文件、真实拒绝 IPC 与双击的强取证属于 AC06/AC07，不提前声称通过 |
| 6 证据可复查 | 成立 | 29 条具名业务断言、真实 DTO/DOM、回调、trace、价格/CCS 哈希和 launches[] 入证据；失败均先归因并补反例，没有以重跑代替原因 |
| 7 CI 预算 | 成立（接入前实测） | 每场景固定代码三连跑，最长单次之和 75.048 秒，新增三步目标小于 8 分钟；45 分钟上限及原步骤不变，实际 CI 时长待回填 |
| 8 版本规则 | 成立 | 基于最新 main，v0.5.8 基线已登记，本 PR 只递增一次 patch 到 0.5.9，版本检查 release=true |

- AC05 十条具名断言最终完成：初始纯判定 10 项断言失败、20 项通过（`ac05-verdict-red.log`）；完整判定及缺探针/错误来源/页面零费用/四桶混价/费率前缀/金额右侧/整个规则漏项等反例逐项红绿。最终 47/47，独立规格与质量复核通过。设置规则期望独立解析隔离 TOML，不从被测 DTO 推导；同步与离线费用都先移除外置规则，核实真实快照来源；四桶公式与固定请求、逐项金额/合计原文严格对照，不引入容差。
- AC05 三连跑（同一代码、期间无修改）：`node frontend/scripts/native-acceptance.mjs --scenario ui-pricing --port 9520 --events 8 --out qa-artifacts/acceptance-closure/b2/ac05-stable-N`，25.911 / 24.879 / 24.004 秒，各 16/16 通过。受测产品 `4159633` 的 acceptance release 构建（0.5.8），EXE SHA256 `a560d9b263b39a864a862593d09fef0f3f3c3efed073a1176c574d48250d7d34`；两次启动端口 9521/9522，证据含 launches[]、快照哈希、页面文字与 IPC。九次最终稳定运行全部路径在隔离根、外连计数零、真实目录指纹一致。三个场景最长单次之和 75.048 秒，小于新增 CI 8 分钟预算；随后各接独立 CI 步骤，作业上限仍为 45 分钟。耗时表 `ac03/ac04/ac05-stable-times.json`，实际 CI 耗时待远端结果回填。

- AC05 发现并局部修复产品展示缺陷：设置页档位浮层列出了峰谷时间，却只显示规则级费率，没有逐时段覆盖价。最小新增回归 `峰谷时段覆盖价格逐一显示，不能只列规则级未知` 在未改代码上 1 失败、3 通过（`ac05-period-display-product-red.log`）；首次原生 `ac05-dev-01/ui-pricing/evidence.json` 同样在 `settings_segment_tags_expand_all_tiers_and_schedule` 失败，完整三模型期望来自隔离 TOML fixture，实际浮层缺少 12/35 与 2/10 的覆盖价。修复仅在既有规则行之后追加每个 period 的覆盖价行，保留原行与缺价“未知”；不改变计价、统计、隐私或设置格式，属于用户已授权的最小回归加局部修复，独立提交。修后定向连同既有费用浮层测试 19/19 通过（`ac05-period-display-product-green.log`），原生须重建后重新验证。
- AC05 首次原生其余两个失败已完成归因：重试按钮初次 notice 可见时取样过早，但后续实际点击成功；保留初次值，并在实际点击前以既有条件预算核可见/可用。离线跨 model/agent 的汇总浮点累计为 `2.274514` / `2.2745140000000004`，逐请求费用、来源与快照哈希完全一致；新判定精确对照完整请求身份、逐请求费用/来源以及页面合计原文，不加金额容差，两个汇总原数继续留证据。新增反例修前 2 失败、45 通过，修后 47/47。两次 launches 均有退出/端口释放证据、无隔离违例，真实目录指纹一致。未用重跑替代归因；此时 AC05 尚未三连跑或接入 CI。

- AC04 已实现九条具名断言：初始纯判定 9 项断言失败、18 项通过（`ac04-verdict-red.log`），实现后通过；复核另加“按日时区冲突冒充 detail_only”与“DOM 未提交”反例，修前明确失败，修后 29/29 通过。最终在同一按应用维度验证 auto 14 → detail_only 11 → auto 14，参与模式取同一 query_id 的真实 query_begin；CCS 只读 fixture 前后哈希、取消不写、三次手动提交幂等、来源时区变化、失败后新预览恢复均有页面/IPC/隔离历史库证据。规格与质量复核通过，收尾哈希、探针和监听释放各自独立执行，保留原失败。
- AC04 本地命令 `node frontend/scripts/native-acceptance.mjs --scenario ui-ccs-import --port 9510 --events 8 --out qa-artifacts/acceptance-closure/b2/ac04-stable-N`；三次间无代码修改，耗时 17.489 / 17.462 / 17.181 秒，各 15/15（九条业务与六条基础检查）通过。受测 EXE 与 AC03 相同，场景基于 `6f50af1` 续行；端口 9511，证据含 launches[]，外连计数零、路径均在隔离根、真实目录指纹一致。初次开发试跑 `ac04-dev-01` 亦通过；全部证据在 `qa-artifacts/acceptance-closure/b2/`，耗时表 `ac04-stable-times.json`，辅助树的原始红绿日志复制到 `qa-artifacts/acceptance-closure/ac04-source-evidence/`。尚待 AC05 后合算 CI 新增预算。

- AC03 已实现十条具名断言：初次纯判定测试为 10 项断言失败、14 项通过（不是模块或启动错误，`qa-artifacts/acceptance-closure/ac03-source-evidence/ac03-checks-red.log`），逐项实现后通过；旁路 IPC、只读 Tauri 属性、旧 DOM、错行、删除前旧轮次晚送达等反例分别先红后绿。最终定向 36 项通过。请求/响应只旁路监听，DOM 前八列、条数与同一 query_id 的真实响应逐项对应；删除一个合成文件后以已 flush 的后端 trace seq 为界，核实新轮次成功，再核对采集查询代次。独立规格和代码质量复核均无剩余重要问题。
- AC03 原生开发失败均先取证后修装配：`ac03-dev-01/ui-identity/evidence.json` 中重启恢复到按时间，探针晚于首屏查询，重复点击选中项未发起新查询；改为按真实 aria-checked 选择另一维度，新增反例先红后绿。`ac03-trial-2/ui-identity/evidence.json` 中新 Claude 查询参数误配旧全部来源快照；实际新查询随后正确返回 8 条。改为按页面 query_id 关联 query_begin，不取最后发起调用，反例先红后绿。两项均未改断言、阈值或 20 秒窗口，没有等待补偿。失败运行的 launches[]、截图、应用日志和探针均保留，外连计数为零、真实目录指纹一致，宿主和后代退出确认通过。
- AC03 最终三连跑（同一代码、期间无修改）：`node frontend/scripts/native-acceptance.mjs --scenario ui-identity --port 9500 --events 8 --out qa-artifacts/acceptance-closure/b2/ac03-stable-N`，耗时 31.282 / 31.648 / 29.381 秒；各 16/16 通过（十条业务与六条基础检查），每次两次启动端口 9501/9502，各证据含 launches[]。受测产品为 `7c83d9c` 的 acceptance release 构建（0.5.8），EXE SHA256 `aaceec4ce42b58a5cbffdc2bac7eab302d15f15d3728421962bd7255ee0cef26`；场景源码随本任务提交固定。证据根 `qa-artifacts/acceptance-closure/b2/`，耗时表 `ac03-stable-times.json`；尚未接入 CI，需 AC04/AC05 同样三连跑后合算新增预算。

- 从 `81c94f9` 合入 main `9c77cef`，普通合并提交 `cc4404a`；版本冲突采用主线 0.5.8，拿到精确采集计时修复及 v0.5.8 已发布基线，AC03–AC05 完成后再统一取号。完整合并钩子通过：普通/acceptance 两套 Rust、前端 651 项、浏览器契约、304 项脚本及 PowerShell 契约；日志 `qa-artifacts/acceptance-closure/b2/main-merge-hook.log`。
- 原生运行前只读复核发现装配缺陷：`native-collection-status.mjs` 重启时会把 `opts.env` 替换为采集注入或空对象，原 launch 只装配继承环境与场景环境，丢掉 AC02 的价格桩、opener 与全部计数代理覆盖。新增契约直接执行真实 launch 环境表达式，修前 3 项断言全部失败（缺少固定隔离通道，`restart-environment-red.log`），不是启动失败。集中在 launch 中先补固定 `controlledIo.env`、再保留场景末级覆盖，修后 3 项通过（`restart-environment-green.log`）。未改任何旧断言、阈值或窗口；此次为用户已授权的测试装配修复，独立提交。此时仍未运行原生应用。
- 该补修的首次完整钩子被既有异步契约拦住（`restart-environment-hook.log`：653 通过、1 失败）：宿主退出探针传入 `Date.now()+30000`，内部再取时推进 1ms，正确得到剩余 29999ms，既有断言却要求精确 30000ms。受控独立探针固定推进 1ms，实测同一断言失败（`probe-budget-race-red.log`；最初探针导入路径笔误的启动错误另存，不充当红灯）。独立修复 `317eb74` 仅对齐该测试首次取时与预算锚点，后续时钟、真实子进程、真实计时器、30000ms 断言和 `<1500ms` 上限不变；定向 22 项及完整钩子通过（`probe-budget-green.log`、`probe-budget-hook-clean.log`）。属于测试装配取时竞争，不是产品缺陷，未以重跑碰运气替代归因。两项补修均经过独立只读复核，继续执行前保留其失败原证据。
