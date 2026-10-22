# 剩余验收收口：来源权限、原生统一验收、隐私强取证、真实性能与系统签字

> 状态：**用户 2026-10-21 确认，按 B1–B5 连续执行**。承接[整合计划](2026-10-11-consolidated-remaining-work.md)全部剩余开放项：N14 第 4 项、N11、N10、N13、N12。
> 进度（2026-10-22）：**B1 / AC01 已交付，N14 关闭**；B2–B5 继续承接 N11、N10、N13、N12，尚未标为通过。
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

- AC05 发现并局部修复产品展示缺陷：设置页档位浮层列出了峰谷时间，却只显示规则级费率，没有逐时段覆盖价。最小新增回归 `峰谷时段覆盖价格逐一显示，不能只列规则级未知` 在未改代码上 1 失败、3 通过（`ac05-period-display-product-red.log`）；首次原生 `ac05-dev-01/ui-pricing/evidence.json` 同样在 `settings_segment_tags_expand_all_tiers_and_schedule` 失败，完整三模型期望来自隔离 TOML fixture，实际浮层缺少 12/35 与 2/10 的覆盖价。修复仅在既有规则行之后追加每个 period 的覆盖价行，保留原行与缺价“未知”；不改变计价、统计、隐私或设置格式，属于用户已授权的最小回归加局部修复，独立提交。修后定向连同既有费用浮层测试 19/19 通过（`ac05-period-display-product-green.log`），原生须重建后重新验证。
- AC05 首次原生其余两个失败已完成归因：重试按钮初次 notice 可见时取样过早，但后续实际点击成功；保留初次值，并在实际点击前以既有条件预算核可见/可用。离线跨 model/agent 的汇总浮点累计为 `2.274514` / `2.2745140000000004`，逐请求费用、来源与快照哈希完全一致；新判定精确对照完整请求身份、逐请求费用/来源以及页面合计原文，不加金额容差，两个汇总原数继续留证据。新增反例修前 2 失败、45 通过，修后 47/47。两次 launches 均有退出/端口释放证据、无隔离违例，真实目录指纹一致。未用重跑替代归因；此时 AC05 尚未三连跑或接入 CI。

- AC04 已实现九条具名断言：初始纯判定 9 项断言失败、18 项通过（`ac04-checks-red.log`），实现后通过；复核另加“按日时区冲突冒充 detail_only”与“DOM 未提交”反例，修前明确失败，修后 29/29 通过。最终在同一按应用维度验证 auto 14 → detail_only 11 → auto 14，参与模式取同一 query_id 的真实 query_begin；CCS 只读 fixture 前后哈希、取消不写、三次手动提交幂等、来源时区变化、失败后新预览恢复均有页面/IPC/隔离历史库证据。规格与质量复核通过，收尾哈希、探针和监听释放各自独立执行，保留原失败。
- AC04 本地命令 `node frontend/scripts/native-acceptance.mjs --scenario ui-ccs-import --port 9510 --events 8 --out qa-artifacts/acceptance-closure/b2/ac04-stable-N`；三次间无代码修改，耗时 17.489 / 17.462 / 17.181 秒，各 15/15（九条业务与六条基础检查）通过。受测 EXE 与 AC03 相同，场景基于 `6f50af1` 续行；端口 9511，证据含 launches[]，外连计数零、路径均在隔离根、真实目录指纹一致。初次开发试跑 `ac04-dev-01` 亦通过；全部证据在 `qa-artifacts/acceptance-closure/b2/`，耗时表 `ac04-stable-times.json`，辅助树的原始红绿日志复制到 `qa-artifacts/acceptance-closure/ac04-source-evidence/`。尚待 AC05 后合算 CI 新增预算。

- AC03 已实现十条具名断言：初次纯判定测试为 10 项断言失败、14 项通过（不是模块或启动错误，`qa-artifacts/acceptance-closure/ac03-source-evidence/ac03-checks-red.log`），逐项实现后通过；旁路 IPC、只读 Tauri 属性、旧 DOM、错行、删除前旧轮次晚送达等反例分别先红后绿。最终定向 36 项通过。请求/响应只旁路监听，DOM 前八列、条数与同一 query_id 的真实响应逐项对应；删除一个合成文件后以已 flush 的后端 trace seq 为界，核实新轮次成功，再核对采集查询代次。独立规格和代码质量复核均无剩余重要问题。
- AC03 原生开发失败均先取证后修装配：`ac03-dev-01/ui-identity/evidence.json` 中重启恢复到按时间，探针晚于首屏查询，重复点击选中项未发起新查询；改为按真实 aria-checked 选择另一维度，新增反例先红后绿。`ac03-trial-2/ui-identity/evidence.json` 中新 Claude 查询参数误配旧全部来源快照；实际新查询随后正确返回 8 条。改为按页面 query_id 关联 query_begin，不取最后发起调用，反例先红后绿。两项均未改断言、阈值或 20 秒窗口，没有等待补偿。失败运行的 launches[]、截图、应用日志和探针均保留，外连计数为零、真实目录指纹一致，宿主和后代退出确认通过。
- AC03 最终三连跑（同一代码、期间无修改）：`node frontend/scripts/native-acceptance.mjs --scenario ui-identity --port 9500 --events 8 --out qa-artifacts/acceptance-closure/b2/ac03-stable-N`，耗时 31.282 / 31.648 / 29.381 秒；各 16/16 通过（十条业务与六条基础检查），每次两次启动端口 9501/9502，各证据含 launches[]。受测产品为 `7c83d9c` 的 acceptance release 构建（0.5.8），EXE SHA256 `aaceec4ce42b58a5cbffdc2bac7eab302d15f15d3728421962bd7255ee0cef26`；场景源码随本任务提交固定。证据根 `qa-artifacts/acceptance-closure/b2/`，耗时表 `ac03-stable-times.json`；尚未接入 CI，需 AC04/AC05 同样三连跑后合算新增预算。

- 从 `81c94f9` 合入 main `9c77cef`，普通合并提交 `cc4404a`；版本冲突采用主线 0.5.8，拿到精确采集计时修复及 v0.5.8 已发布基线，AC03–AC05 完成后再统一取号。完整合并钩子通过：普通/acceptance 两套 Rust、前端 651 项、浏览器契约、304 项脚本及 PowerShell 契约；日志 `qa-artifacts/acceptance-closure/b2/main-merge-hook.log`。
- 原生运行前只读复核发现装配缺陷：`native-collection-status.mjs` 重启时会把 `opts.env` 替换为采集注入或空对象，原 launch 只装配继承环境与场景环境，丢掉 AC02 的价格桩、opener 与全部计数代理覆盖。新增契约直接执行真实 launch 环境表达式，修前 3 项断言全部失败（缺少固定隔离通道，`restart-environment-red.log`），不是启动失败。集中在 launch 中先补固定 `controlledIo.env`、再保留场景末级覆盖，修后 3 项通过（`restart-environment-green.log`）。未改任何旧断言、阈值或窗口；此次为用户已授权的测试装配修复，独立提交。此时仍未运行原生应用。
- 该补修的首次完整钩子被既有异步契约拦住（`restart-environment-hook.log`：653 通过、1 失败）：宿主退出探针传入 `Date.now()+30000`，内部再取时推进 1ms，正确得到剩余 29999ms，既有断言却要求精确 30000ms。受控独立探针固定推进 1ms，实测同一断言失败（`probe-budget-race-red.log`；最初探针导入路径笔误的启动错误另存，不充当红灯）。独立修复 `317eb74` 仅对齐该测试首次取时与预算锚点，后续时钟、真实子进程、真实计时器、30000ms 断言和 `<1500ms` 上限不变；定向 22 项及完整钩子通过（`probe-budget-green.log`、`probe-budget-hook-clean.log`）。属于测试装配取时竞争，不是产品缺陷，未以重跑碰运气替代归因。两项补修均经过独立只读复核，继续执行前保留其失败原证据。
