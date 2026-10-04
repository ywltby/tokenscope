# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **面向用户一律用简体中文回复。**

## 项目目标

**TokenScope** —— 本地 AI agent 使用量统计工具。Rust + Tauri 2 桌面应用（Vue 3 前端）：扫描各 AI 编程工具落在本地的会话日志，解析为统一的用量事件，统计 token 用量、请求数、缓存命中与估算花费，支持按应用 / agent / 模型 / 项目 / 时间段聚合查看。对标 [cc-switch](https://github.com/farion1231/cc-switch) 的 Usage Statistics 功能，但做成独立工具，且覆盖更多 agent（cc-switch 目前支持 Claude Code、Codex、Gemini CLI、Grok Build、OpenCode、Pi、MiniMax Code 的用量统计）。

**当前状态：M1–M11 已落地**——Tauri 2 + Vue 3 桌面 GUI（唯一形态），双 agent 统计、四层价格合并（models.dev 主源 + OpenRouter 备份 + 外置 + 内置）、SQLite 缓存、时间区间筛选、逐请求明细、单实例/窗口记忆/自启可用；各里程碑交付与验收见 `docs/plans/`（里程碑总览与滚动状态以 `docs/plans/README.md` 为准，本文件不追写）。本机已装 cc-switch（`~/.cc-switch`，含 `cc-switch.db` 与 `model-pricing.json`），其 `usage_daily_rollups` 可作统计口径对照源。**本机 Claude Code 经 cc-switch 路由到非 Anthropic 模型（模型串任意，如 `grok-4.5-build`），解析与计价不得假设模型名形态。**

## 设计方向（M1–M4 已按此落地；后续里程碑沿用）

- **只读原则**：TokenScope 只读取各 agent 的本地数据目录，绝不写入、移动或清理它们；自身缓存（SQLite `~/.tokenscope/cache.db`，按文件指纹增量失效、故障自动降级全量扫描）、外置价格表（`~/.tokenscope/pricing.toml`）、OpenRouter 价格快照（`~/.tokenscope/pricing-openrouter.json`）、models.dev 价格快照（`~/.tokenscope/pricing-modelsdev.json`）与价格索引/settings/窗口状态等派生文件只写 TokenScope 自己的数据目录 `~/.tokenscope/`。
- **适配器架构**：每个 agent 一个 source 适配器，职责是「发现日志文件 → 解析为统一用量事件」。agent 特有的 JSONL / JSON / SQLite 细节全部封在适配器内；对外只产出统一的 `UsageEvent`（时间戳、agent、模型、输入 / 输出 / 缓存 token、会话与项目标识）。
- **分层**：`source`（发现+解析）→ `model`（归一化事件）→ `aggregate`（聚合）→ `render`（输出）。层间只经 model 类型交互；新增 agent = 新适配器 + 合成 fixture 测试，聚合与渲染层零改动。
- **时间口径（M6）**：存储层（SQLite）一律 UTC RFC3339 原样持有，全链路只做一次时区转换；聚合/展示时区按解析链取值——显式传入（`--tz`/GUI 下拉，`local`=本机）> 默认 Asia/Shanghai，跨日界与去重规则属于必须先写成不变量的部分。
- **费用估算**：按可配置价格表计算；默认内置常见模型定价，允许用户覆盖（cc-switch 的 `model-pricing.json` 可作参照格式）。无价格的模型明确显示"未知"，不得按 0 静默吞掉。
- **输出（用户已拍板）**：**Tauri 2 + Vue 3 桌面 GUI 是唯一产品形态**（Naive UI、明暗双模式、托盘常驻、关窗缩托盘）。CLI 已于 2026-10-04 移除（用户决策）：`report.rs` 管线保留，数据经 serde 直达前端。
- **GUI 主线程纪律**：Tauri v2 的同步 command 在主线程执行；扫描/解析/缓存/网络等重活一律 `async` + `spawn_blocking` 丢后台线程池，主线程零阻塞（启动卡顿的根因与修法）。
- **候选 agent（用户 2026-10-04 排期决策）**：当前**专注 Claude Code 与 Codex**（本机有真实日志可实测）；Gemini CLI、OpenCode 等其他工具暂缓排期——待安装使用或拿到样例日志、经用户明确排期后再立项（详见 `docs/plans/README.md`）。

## Rust 环境（本机现状）

- **rustup 默认 stable 工具链，Rust 1.99.0**（cargo/rustc 1.99.0，`stable-x86_64-pc-windows-msvc`，含 clippy 与 rustfmt；2026-10-03 安装确认）。升级工具链后回写此处。
- Cargo 工程用 **edition 2024**；不引入 nightly-only 特性。
- 本仓是可执行应用，**Cargo.lock 入库**（勿 gitignore）。
- 格式化以仓库根 `rustfmt.toml` 为准（钉 `newline_style = "Unix"`，与 .gitattributes / .editorconfig 一致）。

## 常用命令

仓库根 `C:\Users\admin\Desktop\项目\tokenscope` 下：

```powershell
cargo build                        # 构建（workspace：根 crate + src-tauri）
cargo fmt                          # 格式化（提交前必须跑过，--check 必须干净）
cargo clippy --all-targets         # 静态检查（提交前必须干净）
cargo test                         # 全部测试
cargo test usage_event             # 按名过滤单个测试
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
- **格式化分工**：Rust 用 `cargo fmt`（`rustfmt.toml` 钉 LF）；前端用 Prettier（`frontend/.prettierrc.json`，双引号/分号/2 空格/printWidth 100）。**提交钩子**（`.githooks/pre-commit`，克隆后执行一次 `git config core.hooksPath .githooks` 启用）会自动跑 fmt --check + clippy + test + 前端 typecheck + format:check。

- 终端是 **Windows PowerShell**：多条命令分开执行或用 `;`，**不要用 `&&`**。
- Bash 工具里 cargo 若不在 PATH（会话早于安装启动），用绝对路径 `/c/Users/admin/.cargo/bin/cargo.exe` 调用。
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
- **先写不变量，再写代码**：涉及聚合口径、时间边界、去重规则时，先列出必须保持的不变量再动手。
- **文件统一 UTF-8**；写中文文档/文案后回读校验，不要用 PowerShell 内联脚本写中文。
- 若用户的问题基于错误前提，**明确指出**——你是协作者，不是执行机器。
- **不在代码 / commit / 注释 / 文档中声明或暗示由 AI 生成；commit message 无 AI 署名尾缀。**
- 先理解现有代码再改；定位实现先用搜索工具粗定位，再读文件精确核对；不为"整理"顺手重构无关内容。
- 终端输出表格用标准 markdown 表格（表头下一行分隔符）即可，**不要在数据行之间额外加分割线**。

## 提交流程

- **默认自动提交推送**：完成任何文件修改后，除非用户明确说"不 commit / 暂不提交 / 先别提交 / 不要 push"等同义要求，或存在验证失败/未解决阻断，自动执行验证 → commit → push，不再等待用户额外提醒。文档修改也算一批文件修改。
- 完成修改后：先自查（`cargo fmt --check` + `cargo clippy` + `cargo test` + 前端 `typecheck`/`format:check` 全绿；pre-commit 钩子会再自动跑一遍）→ `git add`（优先具体文件名，避免目录级 add）→ `git commit` → `git push`，**严格串行执行，禁止并行**。
- commit message 遵循 **Conventional Commits 且必须有中文正文**，说明改了什么、为什么、影响范围（例：`feat(解析): 接入 Claude Code 会话日志适配器`）。
- 一个 commit 只解决一类问题。
- **严禁 `--no-verify`**；严禁未授权 push 到非当前分支。
