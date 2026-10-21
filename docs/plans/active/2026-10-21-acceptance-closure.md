# 剩余验收收口：来源权限、原生统一验收、隐私强取证、真实性能与系统签字

> 状态：**用户 2026-10-21 确认，按 B1–B5 连续执行**。承接[整合计划](2026-10-11-consolidated-remaining-work.md)全部剩余开放项：N14 第 4 项、N11、N10、N13、N12。
> 本计划只排验收与取证所需的改动。验收中暴露的产品缺陷按缺陷流程单独修复，不在本计划里预先设计修法。

**Goal：** 把整合计划剩下的 5 个开放责任编号全部收口，结束后 `active/` 只剩整合计划的历史记录和独立系统待办（跨午夜、长时间休眠、跨显示器，用户 2026-10-13 决定不排期）。

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

### AC06 N10 隐私原生取证

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

### AC07 N10 长时间观察与运行期同步（本地一次）

- 场景 `privacy-long-observe`，本地运行，不进 CI：
  - 未同意观察 130 秒以上，断言同 AC06 `privacy-preseeded`；
  - 同意后，自动同步线程在 `business_ready` 之后 ≥120 秒才发出第一次 `pricing_fetch`；
  - 关闭“自动同步价格”后再观察一个周期，没有新的 `pricing_fetch`；重新开启后恢复。这一条承接 N12 中 “M11 运行验收：Ready 后跨过 120 秒观察价格检查/同步”。
- 记录受测提交、产物哈希、实际等待时长与追踪日志摘录。

### AC08 N10 完整浏览器矩阵（本地一次）

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
- 全部完成后，本计划归档到 `archive/implemented/`。整合计划的状态改为全部关闭，只保留指向独立系统待办的说明。
- D5 清单 4.9–4.12、5.6–5.8 的结果栏按当次证据回填。

## 5. 交付批次与验收

| 批次 | 任务 | PR 性质 | 版本 |
| --- | --- | --- | --- |
| B1 | AC01 | 测试 | 不递增 |
| B2 | AC02 + AC03–AC05 + CI 接入 | 产品（acceptance 接线） | patch |
| B3 | AC06 + CI 接入；AC07、AC08 本地取证 | 脚本与测试（若 B2 的追踪事件不够用而需补产品接线，则递增 patch） | 视改动 |
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

### 起步复核与计划合入

- 计划提交：`49b7b06`、`1e56163`，以及用户修订 `4d27471`（保留旧预热工具）、`6a4dcab`（区分默认巡检与独立契约）。[PR #26](https://github.com/ywltby/tokenscope/pull/26) 普通合并为 `316bd70`；PR 的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39622736925) 与 [Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39622736784) 均成功。
- main 同提交的 [CI](https://github.com/ywltby/tokenscope/actions/runs/39623896792)、[Native UI E2E](https://github.com/ywltby/tokenscope/actions/runs/39623896684)、[Release](https://github.com/ywltby/tokenscope/actions/runs/39623896703) 均成功；版本保持 0.5.6，Release 六个下游全部 skipped。
- §1 已按源码与历史运行元数据复核：受保护命令 35 个、驱动 2716 行、默认巡检 15 个 fixture/注册 23 个、独立契约引用额外 8 个 fixture；PR #25 原生作业 09:29:25–09:37:45 UTC，实际 8 分 20 秒。确认 CCS 默认路径未隔离、价格 URL 常量与 ureq 环境代理/NO_PROXY 支持、opener 真实调用、隐私取证缺口及旧预热工具用途。未运行旧预热工具，未读取真实 CCS 库。AC08 的实际覆盖仍须后续运行产物证明。
- 执行授权以用户本次指令为准：批次自审成立且检查全绿后用普通 merge commit 合并，批次之间不等待确认；明确停点与 AC11 人工签字仍须停下。

### B1 / AC01：读取失败 fixture（共享冲突，本地定向通过，待交付）

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
- CI 耗时：没有新增原生场景；本地新增用例约 0.08 秒。远端耗时与交付结论待本批 PR/main 实跑后记录，N14 不提前关闭。

共享冲突取证（本次交付依据）：

- 独立探针 `shared-lock-probe.rs` 仅用 `std`，通过 `rustc --edition 2024` 编译；PowerShell 与 Git `sh` 各运行一次，每次连续三轮，目录与文件共 12 次读取均因 Windows error 32 失败，释放句柄后全部恢复，临时根均已删除。日志为 `shared-lock-probe-powershell.log` / `shared-lock-probe-git-sh.log`；探针二进制 SHA256 为 `90577eaad25e19b4e3eec5ffede6dd5b06a512b3046739766162da66165548f4`。
- 新用例改名 `unreadable_subdir_and_file_are_diagnosed_without_stopping_the_round`，用 `OpenOptionsExt::share_mode(0)` 持有两个 `File`；目录另加 `FILE_FLAG_BACKUP_SEMANTICS`。持锁期间真实读取必须失败，两个句柄持续覆盖 `summary`，正常与 panic 都由 RAII 释放；原有采集结果断言保留。
- 两个变异重做：目录读错分支提前 `return`，因缺少“无法读取目录”诊断失败（`sharing-red-directory.log`）；文件读错分支漏计 `io_errors`，实际 0、期望 1（`sharing-red-file.log`）。均退出 101，随后恢复产品源码，`git diff HEAD -- crates/tokenscope-core/src/source/{mod,claude}.rs` 无差异。
- 恢复后普通与 acceptance 采集套件均 17/17（`sharing-green-default.log` / `sharing-green-acceptance.log`）。实测：目录与文件读取错误码均 32，2 请求，四桶 40/20/0/0，`io_errors = 1`，两种诊断可见。基线 `316bd70` 加本批测试；源码 SHA256 `843dac044b4d877031935ec0f5d9ac06ca3f4688b081d459b70d5a56248c8c6d`，普通/acceptance 测试二进制 SHA256 `47edfb0bbb9f0eb7aa5c61c098e42bc61af2d088f4bea2a6cde376d8c5586553` / `d8d4c5501701dd74720f521416f8c5a906a5cd309ec1cffb4af4e2924edb89a1`。完整实测值及命令在 `evidence.json`，旧 ACL 证据单列保留。
- 钩子首次检查发现新增目录自检写成 `.err().expect()`，触发 Clippy `err_expect`；改为等价的 `.expect_err()` 后重新验证，两套定向测试仍各 17/17。该修正仅涉及新用例的语法，未放宽或删除断言，原失败日志保存为 `sharing-clippy-initial.log`。
- 既有 16 个用例保持原样；没有产品输入、Cargo 清单或锁文件变更，没有新增原生 CI 场景。提交钩子、PR 与 main 的实际运行结论仍须随后回填，N14 尚未关闭。

阶段 §3 不变量自查（尚未达到合并条件）：

| 编号 | 结论 | 依据 |
| --- | --- | --- |
| 1 普通构建零变化 | 成立 | 最终只追加测试与文档，两个临时产品变异已还原 |
| 2 真实数据零接触 | 成立；原生追踪/代理部分不适用 | 来源、历史库、价格与索引均显式注入用例临时根；没有启动原生应用或读真实业务目录 |
| 3 CCS 只手动 | 不适用 | 本批没有 CCS 调用或代码变更 |
| 4 历史不随来源消失 | 不适用 | 本批只验读取失败，不改变历史保留逻辑 |
| 5 同意先于业务 | 不适用 | 没有隐私接线或原生启动变更 |
| 6 证据可复查 | 成立 | 具名断言、两种环境的共享冲突探针、自检实测值、两个新变异失败日志与产物哈希均保存；旧 ACL 失败保留归因 |
| 7 CI 预算 | 成立 | 原生场景与 45 分钟上限均未改，新增用例定向运行约 0.05 秒 |
| 8 版本规则 | 成立 | 最终改动只有测试与文档；已实测 `--check --base-ref origin/main` 输出 `release: false` |
