# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **面向用户一律用简体中文回复。**

## 项目目标

**TokenScope** —— 本地 AI agent 使用量统计工具。Rust + Tauri 2 桌面应用（Vue 3 前端）：扫描各 AI 编程工具落在本地的会话日志，解析为统一的用量事件，统计 token 用量、请求数、缓存命中与估算花费，支持按应用 / agent / 模型 / 项目 / 时间段聚合查看。对标 [cc-switch](https://github.com/farion1231/cc-switch) 的 Usage Statistics 功能，但做成独立工具，且覆盖更多 agent（cc-switch 目前支持 Claude Code、Codex、Gemini CLI、Grok Build、OpenCode、Pi、MiniMax Code 的用量统计）。

**当前状态：M1–M11 主体已落地**——Tauri 2 + Vue 3 桌面 GUI（唯一形态），双 agent 统计、三来源独立候选计价（2026-10-06 起无编译期内置表）、统一 SQLite 用量历史库、时间区间筛选、逐请求明细、单实例/窗口记忆/自启接线；自身验收与后续功能仍有未闭合项。滚动状态以 `docs/plans/README.md` 为准，唯一待办是 `docs/plans/active/2026-10-11-consolidated-remaining-work.md`，本文件不追写阶段结果。本机已装 cc-switch（`~/.cc-switch`，含 `cc-switch.db` 与 `model-pricing.json`），其日汇总可作对照；读取/导入必须由设置页手动点击触发。**本机 Claude Code 经 cc-switch 路由到非 Anthropic 模型（模型串任意，如 `grok-4.5-build`），解析与计价不得假设模型名形态。**

## 设计方向（M1–M4 已按此落地；后续里程碑沿用）

**界面实现前必须阅读根目录 `DESIGN.md`**。其中的玻璃质感、明暗主题、颜色语义、布局、可访问性和费用明细展示规则是 GUI 的统一约束；若与用户明确的产品需求冲突，以用户需求为准，并同步更新该文档。

- **只读原则**：TokenScope 只读取各 agent 的本地数据目录，绝不写入、移动或清理它们；**统一用量历史库**（SQLite `~/.tokenscope/history.db`，事实源：用量事件、跨来源身份别名、来源证据、CCS 日汇总与导入批次）、外置价格表（`~/.tokenscope/pricing.toml`）、OpenRouter 价格快照（`~/.tokenscope/pricing-openrouter.json`）、models.dev 价格快照（`~/.tokenscope/pricing-modelsdev.json`）与价格索引/settings/窗口状态等派生文件只写 TokenScope 自己的数据目录 `~/.tokenscope/`。旧 `~/.tokenscope/cache.db` 只在首次升级时被**只读**迁移一次（`migrate_legacy_cache`），之后不再参与运行时查询。
- **用量历史与导入（用户定稿，2026-10-10）**：一个 `history.db` 保存精简 token/cwd 与身份元数据，不保存完整 JSONL、不按项目拆库；来源文件删除、来源停用、改采集目录、重扫都不删除已保存用量。全部代理有效用量纳入，不增加主子关系产品。原生身份不含路径：Claude `(session_id, message.id)`，Codex `(session_id, 原始模型, 四桶)` 是保守重播指纹，不能宣称独立请求强身份。CCS **只在设置页手动触发一次**（预览→确认→一个批次），来源库只读一致事务，不自动同步/重试；目标是与已有事实求并集，重复导入不增加用量。会话/proxy 去重时转移原生别名；日汇总单独保存并披露日粒度未知，`detail_only` 可只看请求。查询持有冻结只读事务，事件分批读取，但完整行索引仍随命中请求数增长；保留预算与百万规模由整合计划 N05 验证。采集统一串行；迁移整批原子、升级前备份。旧污染事实、非法迁移、弱身份、暂存容量、退出等待和当前交付尚有 N01–N09，不把 H01–H08 一概标完成。
- **适配器架构**：每个 agent 一个 source 适配器，职责是「发现日志文件 → 解析为统一用量事件」。agent 特有的 JSONL / JSON / SQLite 细节全部封在适配器内；对外只产出统一的 `UsageEvent`（时间戳、agent、模型、输入 / 输出 / 缓存 token、会话与项目标识）。
- **分层**：`source`（发现+解析）→ `model`（归一化事件）→ `aggregate`（聚合）→ `render`（输出）。层间只经 model 类型交互；新增 agent = 新适配器 + 合成 fixture 测试，聚合与渲染层零改动。
- **时间口径（M6；SF05，U01 扩展为两种边界模式）**：存储层（SQLite）一律 UTC RFC3339 原样持有，全链路只做一次时区转换；聚合/展示时区按解析链取值——GUI 下拉显式传入（`local`=本机）> 默认 Asia/Shanghai，跨日界与去重规则属于必须先写成不变量的部分。时区的**唯一权威来源**是 `settings.toml` 的 `timezone`（旧 localStorage 键 `tokenscope-tz` 只在启动时一次性迁移，视图快照不携带时区）。一次查询（汇总/明细/分页）只在入口解析一次「今天」并冻结。时间范围是**四组互斥参数**——自然日预设 `days`、自然日区间 `from`/`to`、滚动窗口 `rolling_hours`（只接受 24）、自定义时刻 `start_at` +（`end_at` 或 `until_now`）：自然日模式按统计时区闭区间，时刻模式（24h 与分钟级自定义）按 `[start, end_exclusive)`，其中 `end_at` = 用户选中的最后一分钟（含）、本地时刻换算用 jiff `compatible` 规则；**两种受限模式都排除晚于 `as_of` 的记录**，「全部时间」不设范围也不截断；自定义整日对齐（00:00 起、23:59 或至今止）归入自然日模式；时刻模式下日汇总不参与，所涉自然日确有日汇总时回传提示键 `rollups_excluded_for_instant_range`。界面显示的范围取自后端冻结回传的 `range`（`QueryHandle`/`SummaryReport`），前端只发预设、不自行计算边界。
- **费用估算**：三来源（外置 `pricing.toml`、models.dev 主源、OpenRouter 补充）保持独立候选，来源等级不作无条件价格覆盖；**无编译期内置价格**——未收录模型始终"未知"，不得按 0 静默吞掉；首次启动无快照时全局横幅提示联网同步。匹配键取模型标识末段（最后一个 `/` 之后），**先按等价规则归一**——ASCII 转小写并删除 `-` `.` `_`（`claude-opus-5-5` / `claude-opus-5.5` / `Claude_Opus_5_5` 同为 `claudeopus55`；`/` 渠道与 `:` 变体结构保留语义）；**完整等价匹配优先，前缀回退只能在查询基名的分隔符边界截断**（`opus55-20261010` 可回退 `opus55`；`opus550`、`gpt-500` 不得命中更短条目）；同等价键的多渠道条目作为独立候选，完整候选优先，再按请求的 prompt token 与事件时间逐一计价取**最高费用**（来源等级仅同价决胜）。估算的是候选渠道而非实际路由，已排除不完整候选时不是所有未知渠道的严格上界。单价三态（数字/显式 0/未知）与外置 `model_policy`（显式补充缺失的缓存读价语义，非用户账单覆盖模式）贯穿展示与估算。支持上下文分段（models.dev tiers / OpenRouter overrides / 外置 `[[model.segment]]`）与外置峰谷时间规则（按事件时间换算）；请求级明细（命中候选、档位、四类分项单价/小计）随 `cost_usd` 同源输出，前端不重算。权威口径见 `docs/stats-semantics.md` §4。
- **模型身份（2026-10-10）**：按模型分组使用**等价身份键**（`Group.key`，同一模型的多种写法合并、跨应用也合并；不同版本/变体保持独立），展示名（`Group.label`）优先取可信 models.dev 模型级 `name`，否则退回该组原始模型名的稳定代表写法——压缩键只用于筛选与下钻匹配，绝不展示；下钻按同一身份规则匹配，请求明细保留原始模型名。价格索引 v9 与签名 `rules:9` 对应当前匹配规则；解析、价格索引、前端快照是独立版本轴，各按自身变更失效，不退回旧计划固定数字。
- **查询会话（SF04）**：GUI 查询 = `query_begin` 建短生命周期不可变快照（冻结事件/价格修订/as_of），`query_summary`/`query_events` 携带 query_id 读取；游标 v2 绑定 query_id/主指纹/下钻指纹/行位置，过期或错会话结构化拒绝（`query_expired`），不静默换页。
- **来源目录（SF09；PR3 多根修订）**：来源按**固定候选集**解析——默认根、启动环境派生根（`CODEX_HOME` 追加为 Codex 额外根、`CLAUDE_CONFIG_DIR` 追加为 Claude 额外根且映射禁用）与手动追加目录；不枚举磁盘、不扫描主目录。每应用有总开关与自动发现开关，自动根可逐条停用（排除项），手动行可启停/移除。每轮采集先冻结根集合再整轮遍历：同应用嵌套/重复/别名只解析一次（最小规范化根认领，子根行带"与 X 重叠"诊断）；**跨应用根相同或互为父子 → 发现层双方标记冲突、本轮都不读取**（无"先扫描者得"；冲突判定集中在 `resolve_sources`，采集层台账只留兜底诊断）；手动新增在设置事务锁内校验（同应用已存在/嵌套、跨应用冲突、相对路径、整个主目录拒绝；合法缺失根可保存）。旧 v1 自定义 `dir` 在启动时自动迁移为追加目录并开启自动发现——默认目录用量随之纳入，**统计总量可能增加**；相对环境路径不再读取（忽略并诊断）。
- **项目身份（2026-10-09，阶段 A + 阶段 B）**：同一**规范化绝对路径**跨工具得到同一项目 key（Windows 分隔符/盘符、根与 UNC 语义、`file://` URI 一次百分号解码在 source 层统一；不可安全解释的输入走兜底）。归属按**项目根**归并、两侧一致：进入当前根的**子目录**仍归该根（`/test` → `/test/123/456` 都是 `/test`），**越出当前根**才算切换项目（`/test` → `/bee`，其子目录归 `/bee`）；判定按分量边界前缀，不做文件系统访问、不推断仓库根。Claude 按行序推进根（sidechain 不参与身份；无可信 cwd 时按 `~/.claude.json` 的 projects 正向编码唯一命中，冲突/缺失保持 slug 身份；映射按根绑定——手动追加根与 `CLAUDE_CONFIG_DIR` 根不读映射），Codex 由 `session_meta` 重置、`turn_context` 推进。事件另带 `session_initial_cwd` 与 `event_cwd`（保留子目录细节）。展示名（路径末段）不是身份；仅同名不合并。权威口径见 `docs/stats-semantics.md` §3.4。
- **异常 token（SF08）**：适配器/缓存恢复/聚合全链路受检算术——异常行计 bad_lines 跳过；各自合法但累计溢出 → 查询明确报错，不回绕不饱和。
- **设置事务（SF02）**：全部应用内设置写者走 `settings::update` 读改写锁（原子替换保留）；外部编辑器不受锁约束，勿与 GUI 同时写。日志初始化可失败（SF06）：降级 stderr 不阻断启动，状态经 `startup_diagnostics` 暴露。
- **输出（用户已拍板）**：**Tauri 2 + Vue 3 桌面 GUI 是唯一产品形态**（Naive UI、明暗双模式、托盘常驻、关窗三态——未配置默认动作时弹窗询问「最小化/直接退出 + 记忆勾选」，记忆值存 `~/.tokenscope/settings.toml` 的 `close_action`，设置页可改回每次询问；托盘「退出」旁路不受影响）。CLI 已于 2026-10-04 移除（用户决策）：`report.rs` 管线保留，数据经 serde 直达前端。
- **GUI 主线程纪律**：Tauri v2 的同步 command 在主线程执行；扫描/解析/缓存/网络等重活一律 `async` + `spawn_blocking` 丢后台线程池，主线程零阻塞（启动卡顿的根因与修法）。
- **候选 agent（用户 2026-10-04 排期决策）**：当前**专注 Claude Code 与 Codex**（本机有真实日志可实测）；Gemini CLI、OpenCode 等其他工具暂缓排期——待安装使用或拿到样例日志、经用户明确排期后再立项（详见 `docs/plans/README.md`）。

## Rust 环境（本机现状）

- **rustup 默认 stable 工具链，Rust 1.99.0**（cargo/rustc 1.99.0，`stable-x86_64-pc-windows-msvc`，含 clippy 与 rustfmt；2026-10-03 安装确认）。升级工具链后回写此处。
- Cargo 工程用 **edition 2024**；不引入 nightly-only 特性。
- **workspace 布局（2026-10-15）**：仓库根是**虚拟 workspace**（没有 `[package]`、不写应用版本），成员为 `crates/tokenscope-core`（核心库，package 名仍是 `tokenscope`，原根 `src/`、`tests/` 与全部 fixture 已迁入）与 `src-tauri`（桌面壳 `tokenscope-app`，唯一应用二进制 `tokenscope.exe`）。应用版本写在**核心 manifest**。根目录普通 `cargo build/test/clippy` 覆盖两个默认成员；只查核心库用 `cargo test -p tokenscope`，只构建 GUI 用 `cargo build -p tokenscope-app --bin tokenscope`。
- **单一锁与 target**：受版本管理的锁只有根 `Cargo.lock`（恰好含 `tokenscope`、`tokenscope-app` 两个无 source 的本地包，版本与四份版本配置一致），不再有 `src-tauri/Cargo.lock`；默认输出统一到根 `target/`。
- 本仓是可执行应用，**Cargo.lock 入库**（勿 gitignore）。
- 格式化以仓库根 `rustfmt.toml` 为准（钉 `newline_style = "Unix"`，与 .gitattributes / .editorconfig 一致）。
- **构建输入清单**：`scripts/build-inputs.json` 由 `python -B scripts/build_inputs.py --update` 从 Cargo 依赖信息生成，记录被编译进程序的非 Rust 输入及其消费包与构建模式；改 `include_str!`/`include_bytes!`/构建脚本输入后必须更新并提交清单，CI 两种构建后跑 `--check` 校验。

## 常用命令

仓库根 `C:\Users\admin\Desktop\项目\tokenscope` 下：

```powershell
cargo build                        # 构建两个默认成员（核心库 + 桌面壳）
cargo fmt --all                    # 格式化（提交前必须跑过，--check 必须干净）
cargo clippy --locked --workspace --all-targets   # 静态检查（提交前必须干净）
cargo test --locked --workspace    # 全部测试（两个成员）
cargo test -p tokenscope usage_event   # 只查核心库；按名过滤单个测试
cargo test -p tokenscope-app       # 只查壳
```

GUI（前端在 `frontend/`，Tauri 壳在 `src-tauri/`；Tauri CLI 装于 frontend devDependencies）：

```powershell
pnpm --dir frontend install        # 前端依赖（首次）
pnpm --dir frontend typecheck      # vue-tsc 类型检查（提交前必须干净）
pnpm --dir frontend format:check   # Prettier 风格检查（提交前必须干净；format 为写入）
pnpm --dir frontend build          # 前端产物（提交前必须通过）
```

GUI（Tauri CLI 在仓库根调用——CLI 只向下搜索 src-tauri，在 frontend 目录执行会找不到配置）：

```powershell
.\frontend\node_modules\.bin\tauri dev     # 开发窗口（会弹出 GUI）
.\frontend\node_modules\.bin\tauri build   # 生产构建（NSIS 安装包，首次较慢）
```

- 前端 TypeScript 钉 TypeScript 5.x（vue-tsc 与 TS 7 不兼容，勿升级）；Naive UI 组件库、ECharts 图表（直接用 echarts，未包 vue-echarts）。
- **格式化分工**：Rust 用 `cargo fmt`（`rustfmt.toml` 钉 LF）；前端用 Prettier（`frontend/.prettierrc.json`，双引号/分号/2 空格/printWidth 100）。**提交钩子**（`.githooks/pre-commit`，克隆后执行一次 `git config core.hooksPath .githooks` 启用）**按暂存区选择检查**（`scripts/pre_commit_checks.py`）：纯文档提交只跑文档检查；Rust 代码按包与构建模式选择 workspace 或壳的 clippy+test；前端改动跑 typecheck/format:check/test/build；构建与门禁入口（Cargo 清单与锁、`build.rs`、`.cargo/**`、`.github/**`、`.githooks/**`、清单与选择器自身）强制完整检查，未识别路径同样完整兜底。用 `TOKENSCOPE_PRECOMMIT_MODE=full` 强制完整模式，`python -B scripts/pre_commit_checks.py --explain` 只打印选中检查。CI（`.github/workflows/ci.yml`）在 push/PR 上跑**不减的完整门禁**（普通与 acceptance 两套 workspace clippy/test、构建输入清单校验、前端 build 与 E2E），并按检查分组成 `rust`、`rust-acceptance`、`contracts`、`frontend` 四个并行作业（步骤同源于 `.github/actions/checks`，用必填输入 `group` 选择），另有一个只做汇总的 `gate` 作业作为必需检查——四条全成功它才通过；`tauri build` 属发布验收不进 CI。
- **测试密闭性（2026-10-05 事故修复，必须遵守）**：任何测试（单元/e2e）构造 `SummaryOptions` 时，`cache_dir` 与 `pricing_index` **必须注入临时目录**，禁止落回默认 `~/.tokenscope`——否则测试会把用户真实缓存 purge 成 fixture，GUI 每次启动都全量冷扫描（用户 1.2 GB 日志，分钟级加载）。新测试评审时先看这两项。
- **真实数据性能验收/缓存预热**（`#[ignore]`，需显式环境变量，会全量解析本机 `~/.claude`、`~/.codex` 并写入 `~/.tokenscope/history.db`）：

```powershell
$env:TOKENSCOPE_REAL_PERF = "1"; cargo test --release --test perf_real_data -- --ignored --nocapture
```

  本机基准（release）：冷启动全量解析 1.2 GB 约 5.5 s，热启动（缓存命中）约 0.1–0.2 s。collect_all 各阶段耗时已落 `~/.tokenscope/logs/`（价格加载 / 逐 agent 采集 / 去重 / 聚合），启动慢先看日志。
- **日志规约（2026-10-05 完善，改代码时维持这个分工）**：落盘 `~/.tokenscope/logs/`（每日滚动），默认 info，`RUST_LOG=debug` 提级。**info** = 生命周期里程碑（GUI 初始化完成）、影响行为的决策（缓存降级、同步开始/结果、自启与开关切换）、关键路径端到端耗时（汇总/明细/价格表加载/缓存重建，含参数摘要）；**warn** = 降级与可恢复异常（缓存打开/读取/写入失败按次聚合、设置读取失败、视图快照损坏、同步部分失败）；**error** = 命令失败（`run_blocking` 统一落错误链 + 堆栈）与自动同步失败。**debug** = 高频低价值（自动同步每小时轮询决策、逐命令耗时、视图快照存取、价格索引重建）。**禁止**逐文件/逐行/逐窗口事件刷屏日志；新功能先想清楚"排障时需要看到什么"，再决定级别。**自动触发路径例外（U06）**：后台定时采集按 `Trigger::Auto` 触发时，采集与查询路径上的进度日志一律按 debug 输出——自动读取间隔最短 5 秒，按轮次 info 会刷屏；唯一例外是本轮历史库确有写入时保留一条 info 摘要（新增 / 更新条数），排障仍能看到"哪一轮有新数据"。

- 终端是 **Windows PowerShell**：多条命令分开执行或用 `;`，**不要用 `&&`**。
- cargo 不在 PATH（终端会话早于 rustup 安装启动，报 `cargo ... program not found`；注册表用户 PATH 已含 `%USERPROFILE%\.cargo\bin`）：**重开终端即恢复**；当前窗口可临时修 `$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path`；Bash 工具里用绝对路径 `/c/Users/admin/.cargo/bin/cargo.exe` 调用。
- **crates.io 需走本地代理**（用户 2026-10-04 指示）：cargo/pnpm 联网操作前设置 `HTTP_PROXY`/`HTTPS_PROXY=http://127.0.0.1:7897`，否则 registry 更新超时（tauri dev 亦同——改依赖后需带代理重启 dev）。
- 搜索文件名和代码优先使用 `rg` 或 `git grep`。
- 解析 JSONL / JSON 一律走 serde 等真实解析库；禁止脆弱的字符串拼接与文本替换。

## 测试与 fixture 约定

- **真实 agent 日志永不入库**：会话日志含用户代码与对话内容，属敏感数据；`.gitignore` 已兜底，测试一律使用手工合成的最小 fixture。
- **先写失败测试，再写实现**：每个 bug 修复先有能红的最小回归测试；解析类改动必须带「样例日志 → 期望事件」的 fixture 用例。
- 新适配器测试三件套：路径发现（含目录不存在的情形）、典型日志解析、畸形行容错（坏行跳过并计数，不 panic）。
- 全量 `cargo test` 绿只能证明已有测试通过，不能证明本次设计目标被覆盖；关键边界（跨日聚合、去重、时区）必须有定向测试。

## 协作约定

- **先计划后执行**：新增功能 / 跨文件改动，先在 `docs/plans/active/` 写计划并等用户确认再动手。可跳过 plan 直接修复的仅限：程序报错、测试红灯、文案拼写、格式化。用户说"直接改"则跳过。
- **计划优先级与归档（用户 2026-10-11 要求）**：用户明确要求 > 较新计划 > 较旧计划 > 未确认的设计建议。全部/部分完成的旧计划均归档并标明状态，剩余任务只维护在当前整合计划；历史“保持 active”要求不阻止此归档，归档也不把待验标成通过。
- **先写不变量，再写代码**：涉及聚合口径、时间边界、去重规则时，先列出必须保持的不变量再动手。
- **文件统一 UTF-8**；写中文文档/文案后回读校验，不要用 PowerShell 内联脚本写中文。
- 若用户的问题基于错误前提，**明确指出**——你是协作者，不是执行机器。
- **不在代码 / commit / 注释 / 文档中声明或暗示由 AI 生成；commit message 无 AI 署名尾缀。**
- 先理解现有代码再改；定位实现先用搜索工具粗定位，再读文件精确核对；不为"整理"顺手重构无关内容。
- 终端输出表格用标准 markdown 表格（表头下一行分隔符）即可，**不要在数据行之间额外加分割线**。

## 提交流程

- **main 版本递增（用户 2026-10-21 修订）**：只有会进入安装包的产品输入改动才要求递增一次 `0.x.y`，暂不发 `1.x`；修复/维护选 patch，新功能或不兼容变化选 minor，混合取更高等级。产品输入判定复用 `scripts/build-inputs.json`：登记了 default 构建消费方的输入（如 `docs/privacy.md`）即使落在文档或测试目录也算产品，其余路径按明确的非产品清单排除，未知路径一律按产品处理。明确清单中的纯文档、CI、脚本、测试装配，以及根目录 `.gitignore`、`.gitattributes`、`.editorconfig` 的合入**不递增、不发布**；其他配置（如 `rustfmt.toml`、`frontend/.prettierrc.json`、`frontend/vitest.config.ts`、`frontend/tsconfig.json`）默认仍算产品。`release-change.json` 必须与基线逐字节一致；非产品改动也可以主动递增一次（例如要发布流水线修复），此时照常发布。同一个 PR 的补修不重复加号。递增时先 fetch 最新 main，再执行 `python -B scripts/bump_version.py --bump minor --summary "本轮变更说明"`（修复改用 patch），工具同步四份版本配置（核心 manifest、壳 manifest、`frontend/package.json`、`src-tauri/tauri.conf.json`）、唯一根 `Cargo.lock` 与声明；用 `--check --base-ref origin/main` 自查，不发布的合入输出 `release: false`。历史基线的版本按该 ref 自身布局读取（迁移前的提交读根 `[package]`，迁移后读核心 manifest）。不要单改版本字符串或顺手更新第三方依赖。尚待确认的方案文档不分配发行版本。
- **发布与升级**：Release 的 gate 只做版本与发布身份校验（`bump_version.py --check` 与 `release_metadata.py`），完整测试由 PR 必需检查（`gate`、`native-ui`，要求与最新 main 同步）保证；**版本未变的 main 推送，Release 只跑 gate，构建、安装、升级与发布全部跳过**，不建标签、不改已有发布。main 上的 CI 与 Native UI E2E 照常运行，与发布并行，不阻断发布。main 的 build/五场景安装/真实起点升级矩阵/两条连续升级链通过后由同一流水线创建 `v0.x.y` 和预发布，标签与资产不可覆盖；历史 0.1.0 固定 8d1d90de 的原资产，不重新构建历史包。新 main 提交不取消另一个提交的发布。真实安装、卸载和升级脚本只在 GitHub-hosted 一次性 Runner 执行，本机仅测试契约，不能删除真实 `~/.tokenscope`；不恢复可选旧包变量跳过分支。实际来源以标签解引用的提交和固定资产哈希判断，已有标签时 Release 的 target_commitish 只是创建提示。
- **升级起点矩阵**：`scripts/fixtures/upgrade_baselines.json`（schema 2）登记每个已发布版本的标签、解引用提交、原资产哈希与持久格式指纹；`scripts/upgrade_baselines.py select` 离线给出起点集合（固定 0.1.0 + 上一版 + 每种持久格式的代表，最多 6 个，超出要求重新评审），两个工作流的 `upgrade-test` 用它的 `include` 作矩阵，`upgrade-chain` 用同一份有序版本列表。**准备合入 main 前必须先用 `python -B scripts/upgrade_baselines.py add --tag v<上一版>` 登记上一版**（资产逐个下载核对后才写入）；`--check --base-ref <事件 base SHA>` 在 CI gate 校验基础版本已登记、已登记版本在 GitHub 上仍存在且标签/资产哈希未变，GitHub 出错即失败（只对代理 TLS 断开、连接重置等传输层瞬断有限重试 3 次，HTTP 状态错误立即失败；本机未设 `GH_TOKEN` 时借用已登录 `gh` 的令牌，避免匿名限流）。撤回顺序：取消该版本在途发布作业 → 提交 PR 标为 `withdrawn` 并合入 main → 删除发布页；`publish` 会拒绝登记为 withdrawn 的版本。持久格式指纹由核心库示例程序 `cargo run --locked --release -p tokenscope --example print_persistent_formats` 从真实常量输出，构建作业经 `release_assets.py print-persistent-format` 写进 `build-metadata.json`（schema 2）；`SETTINGS_FORMAT_REVISION` 变化时必须同步 `scripts/fixtures/settings_migrations.json`。
- **提交与压测例外**：Native UI E2E 失败时先按 `evidence.json`（含 `launches[]`）说明原因——属于测试架缺陷的先修测试架，属于产品问题的按缺陷处理，**不能用重跑通过代替原因**。原生验收的 20 轮重启压测（`ui-relaunch --relaunch-cycles 20`）只在本地取证，不进 CI（用户 2026-10-16 决定）。
- **默认自动提交推送**：完成任何文件修改后，除非用户明确说"不 commit / 暂不提交 / 先别提交 / 不要 push"等同义要求，或存在验证失败/未解决阻断，自动执行验证 → commit → push，不再等待用户额外提醒。文档修改也算一批文件修改。
- 完成修改后：先自查（本地按暂存区选择——提交钩子只跑选中检查；要一次跑全可用 `TOKENSCOPE_PRECOMMIT_MODE=full`，未知变更、清单缺失或损坏一律完整兜底；PR CI 的完整门禁不减）→ `git add`（优先具体文件名，避免目录级 add）→ `git commit` → `git push`，**严格串行执行，禁止并行**。验证失败不得提交；禁止 `--no-verify`，不得关闭 CI 或降低验收阈值。
- commit message 遵循 **Conventional Commits 且必须有中文正文**，说明改了什么、为什么、影响范围（例：`feat(解析): 接入 Claude Code 会话日志适配器`）。
- 一个 commit 只解决一类问题。
- **严禁 `--no-verify`**；严禁未授权 push 到非当前分支。
