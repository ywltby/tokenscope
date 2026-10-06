# 关闭确认弹窗与设置配置文件（关闭行为三态 + 高级配置）

> **For Claude:** 计划经用户确认后执行；按任务顺序 TDD（先失败测试后实现），每任务独立提交。

**Goal:** 主界面关闭程序时弹出「最小化到托盘 / 直接退出」确认弹窗，可勾选记忆；记忆结果可在设置页修改（含恢复"每次询问"）。同时把设置落盘文件升级为带注释、可直接手编的正式配置文件（`~/.tokenscope/settings.toml`），设置页底部提供「打开设置配置文件」入口。

**现状（调研结论）：**

- 关窗行为硬编码在 `src-tauri/src/lib.rs` 的 `CloseRequested` 处理器：保存窗口状态 → `hide()` → `prevent_close()`（无条件缩托盘）；托盘菜单「退出 TokenScope」走 `app.exit(0)`。
- 设置存储在 `~/.tokenscope/settings.json`（serde JSON，无注释，字段 `price_auto_sync` + `sources`）；`settings_get` 每次从盘读取——**外部手改立即对下一次读取生效**（好性质，保留）。
- `open_pricing_file` 已有「缺文件先写模板再打开」模式（`PRICING_TEMPLATE`）；根库已依赖 `toml = "1.1.6"`（pricing.toml 解析），迁移无新依赖。
- 前端对 `settings_get` 是松类型消费（`Record<string, unknown>`）。

**设计决策：**

1. **配置文件迁移到 TOML**：`settings.json` → `settings.toml`。理由：与外置价格表 `pricing.toml` 一致、支持注释（手编是本需求的核心）、serde 同构。`load()`：toml 缺失但遗留 `settings.json` 存在 → 解析导入（向前兼容，旧字段语义不变）；`save()` 落 TOML（带中文注释头）并把遗留 json 改名 `settings.json.bak`（一次性，保留可回滚，防"改了没生效"的困惑）。窗口状态 `window-state.json` 是机器管理的状态文件，**不**并入（out of scope）。
2. **关闭三态**：`close_action: Option<CloseAction>`（`minimize` / `quit`，缺省 None = 每次询问）。`CloseRequested` 时后端读盘判定：`Some(Minimize)` → 隐藏窗口；`Some(Quit)` → `app.exit(0)`；`None` → `prevent_close` + 向前端 emit `close-requested` 事件，由前端弹 NModal 询问。用户选择经 `close_resolve(minimize, remember)` 回传后端：`remember=true` 时持久化 `close_action`，然后执行隐藏或退出。**取消/Escape = 不关闭**（窗口保持打开）。
3. **托盘「退出」旁路**：托盘退出 = 显式退出，不经询问、不受 `close_action` 影响（保持现状）。
4. **设置页**：「应用」组新增「关闭窗口时」行（每次询问 / 最小化到托盘 / 直接退出，改选即写盘——这就是修改记忆选项的入口）；页面最底部新增「高级配置」组：说明配置文件路径与手改生效规则 + 「打开设置配置文件」按钮（缺文件先写带注释模板）。

**不变量：**

- I1 `settings_get`/`settings_set_*` 每次读盘写盘——外部手改配置文件对下一次读取立即生效；GUI 写回不得静默丢弃用户手加的未知字段（serde 未知字段忽略 + 保存时以内存态全量落盘，手改的未知字段会丢——**接受**：TOML 头注释声明"以 GUI 保存为准，自定义注释会丢失"）。
- I2 遗留 `settings.json` 的所有字段语义在导入后不变；改名 `.bak` 前必须已成功写出 toml。
- I3 `close_resolve(remember=true)` 只写 `close_action`，不动其他字段；写入前重读盘上最新设置（避免覆盖并发改动）。
- I4 弹窗路径必须在 `CloseRequested` 中 `prevent_close`，避免询问期间窗口真关；`Ask` 分支 emit 事件后若前端未就绪（极早关闭），窗口保持打开，不静默退出。
- I5 托盘退出、单实例唤起、自启行为不变。

---

## Task 1：根库 settings.rs —— CloseAction 字段 + TOML 存储迁移

**Files:**
- Modify: `src/settings.rs`（含既有测试迁移）
- Test: 同文件 `#[cfg(test)]`

**Step 1 失败测试：**
- TOML roundtrip：`save` 后文件含 `close_action = "minimize"`，`load` 还原 `Some(Minimize)`；`quit` 同理；缺省 `None`。
- 注释头：`save` 产物首行为 `#` 注释且包含「直接编辑」字样；`load` 解析带注释文件成功。
- 遗留导入：目录只有 `settings.json`（含 `price_auto_sync:false` + sources）→ `load(toml_path)` 返回等价设置；随后 `save` → toml 存在且 json 被改名为 `settings.json.bak`。
- 模板：`SETTINGS_TEMPLATE` 能被 `toml` 解析为默认设置（全注释/空段）。
- 既有 JSON fixture 测试改为 TOML 内容（roundtrip/corrupt/unknown-fields 等）。

**Step 2 实现：**
- `CloseAction { Minimize, Quit }`（`serde(rename_all = "snake_case")`，Copy）。
- `Settings` 增 `#[serde(default)] close_action: Option<CloseAction>`。
- `settings_path()` → `settings.toml`；`load` 支持 toml + 遗留 json 导入；`save` = 注释头 + `toml::to_string_pretty` + 遗留 json 改名 bak。
- `pub const SETTINGS_TEMPLATE: &str`：全字段中文注释模板（键全部注释掉，解析即默认）。

**Step 3 门禁**：`cargo test`、`clippy`、`fmt`。**提交**：`feat(设置): 设置文件迁移 TOML 并支持关闭动作字段`

## Task 2：src-tauri —— 关闭决策、close_resolve 与配置文件打开命令

**Files:**
- Modify: `src-tauri/src/commands.rs`、`src-tauri/src/lib.rs`
- Test: `commands.rs #[cfg(test)]`、`lib.rs` 可测纯函数

**Step 1 失败测试：**
- `close_decision_from(&Settings)`：None→Ask、Some(Minimize)→Minimize、Some(Quit)→Quit。
- `settings_set_close_action_impl`：合法值 "minimize"/"quit"/None 写盘；非法值报错且文件不变。
- `close_resolve_impl(remember)`：写 `close_action` 前重读盘上设置（不覆盖并发改动）；remember=false 不写盘。

**Step 2 实现：**
- `commands::settings_set_close_action(Option<String>)`、`commands::close_resolve(app, minimize, remember)`（remember→持久化；minimize→隐藏主窗口；否则 `app.exit(0)`）、`commands::open_settings_file(app)`（镜像 `open_pricing_file`：缺文件写 `SETTINGS_TEMPLATE` → opener 打开 → 返回路径）。注册进 `invoke_handler`。
- `lib.rs` `CloseRequested` 分支：`save_window_state_now` → 按 `close_decision` 执行（Minimize=hide+prevent；Quit=`app.exit(0)`；Ask=emit `close-requested`+prevent）。日志：决策落 info（生命周期决策），Ask 高频路径 debug。

**Step 3 门禁 + 提交**：`feat(设置): 关闭行为三态与设置文件打开命令`

## Task 3：前端 —— 关闭确认弹窗与 App 接线

**Files:**
- Create: `frontend/src/components/CloseConfirmDialog.vue`
- Modify: `frontend/src/App.vue`、`frontend/src/types.ts`（`CloseAction`、事件类型）
- Test: `frontend/src/components/CloseConfirmDialog.test.ts`、`frontend/src/App.test.ts`

**Step 1 失败测试：**
- 组件：打开态渲染标题/说明/「最小化到托盘」「直接退出」「取消」；NCheckbox 默认不勾选；点「最小化」emit `resolve {minimize:true, remember:勾选态}`；「直接退出」同理；「取消」emit `cancel` 且不带 remember。
- App 集成：mock `@tauri-apps/api/event` 的 `listen`，手动触发 `close-requested` 回调 → 弹窗可见；`resolve` → `invoke("close_resolve", {minimize, remember})` 参数正确；`cancel` → 无 invoke。`listen` 返回的 unlisten 在 unmount 时被调用。

**Step 2 实现：**
- 组件：NModal（elevated 表面、`--ts-radius-popover`、宽度 ~420px），文案「关闭 TokenScope / 选择窗口关闭按钮的默认行为」，checkbox「记住我的选择，以后不再询问（可在设置页修改）」。按钮语义：主操作「最小化到托盘」quaternary、「直接退出」`type="error"`（破坏性语义），Escape/遮罩点击 = 取消。
- App.vue：`onMounted` 注册 listen（守卫：弹窗已开则忽略重复事件）；处理 resolve/cancel。

**Step 3 门禁 + 提交**：`feat(界面): 关闭确认弹窗与记忆选项`

## Task 4：前端设置页 —— 关闭行为行与「高级配置」组

**Files:**
- Modify: `frontend/src/views/Settings.vue`、`frontend/src/types.ts`（如需）
- Test: `frontend/src/views/Settings.test.ts`

**Step 1 失败测试：**
- 「应用」组出现「关闭窗口时」行：默认「每次询问」；选择「最小化到托盘」→ `invoke("settings_set_close_action", {action:"minimize"})`；「直接退出」→ `"quit"`；「每次询问」→ `action:null`。
- 页面底部「高级配置」组：说明含配置文件路径；按钮「打开设置配置文件」点击 → `invoke("open_settings_file")` 且成功提示。

**Step 2 实现：**
- 「应用」组 `setting-row`：label「关闭窗口时」+ help「最小化后可从托盘恢复；记忆后仍可在此修改或恢复每次询问」+ NSelect（32px 控件族）。
- 底部 `settings-group`「高级配置」：`.ts-card`，help 三行（路径 `~/.tokenscope/settings.toml`；可直接编辑、保存后对下一次读取生效；GUI 保存会重写文件——自定义注释以文件内说明为准），按钮「打开设置配置文件」（`open_settings_file`，成功 `msg.info` 返回路径）。
- `loadSettings` 读取 `close_action` 并映射 `null→"ask"`。

**Step 3 门禁 + 提交**：`feat(界面): 设置页关闭行为与高级配置入口`

## Task 5：文档与收尾

**Files:**
- Modify: `CLAUDE.md`（「关窗缩托盘」表述更新为三态行为；只读原则中 settings 文件名如提及则同步）
- Modify: 本文件（执行记录）

**Step 1**：全量门禁（前端四件套 + Rust 双 manifest）。**Step 2**：执行记录 + 最终提交：`feat(设置): 落地关闭确认与高级配置文件`

## 完成定义

1. 关闭主窗口：未记忆 → 弹窗（最小化/退出/取消 + 记忆勾选）；已记忆 → 直接执行且不再弹窗。
2. 设置页「关闭窗口时」三选一实时生效；改回「每次询问」即清除记忆。
3. `~/.tokenscope/settings.toml` 为唯一设置事实源：带字段注释、可手编、手改对下一次读取生效；旧 `settings.json` 自动导入并改名 `.bak`。
4. 设置页底部可一键打开配置文件；托盘退出旁路不受影响。
5. 全部既有与新增测试绿；门禁全过。

## 风险与回滚

- **TOML 迁移丢字段**：serde 往返测试覆盖全部字段；导入失败（坏 JSON）保持现有「报错不覆盖」策略（Task 7.1 不变量），不自动改名。
- **Quit 误触发**：`app.exit(0)` 仅出现在 `close_resolve` 的退出分支与托盘菜单；`CloseRequested` 的 Quit 分支同样走 `exit(0)`，路径唯一可审计。
- **前端未就绪时关闭**：Ask 分支 prevent_close 后窗口保留（I4），用户可再次点击关闭。
- **弹窗期间二次关闭**：前端守卫去重；后端不排队事件。

## 执行记录（2026-10-06）

| 任务 | 内容 | 提交 |
| --- | --- | --- |
| 任务 1 | settings.rs：`CloseAction`（minimize/quit）+ `close_action` 字段；存储迁移 `settings.json` → `settings.toml`（中文注释头、遗留 json 只读导入、首次保存改名 `.bak`、坏内容报错不覆盖、toml 优先）；`SETTINGS_TEMPLATE` 全字段注释模板 | 353c8da |
| 任务 2 | src-tauri：`close_decision_from` 三态决策；`CloseRequested` 分支（Ask=emit close-requested / Minimize=hide / Quit=exit(0)）；新命令 `settings_set_close_action`、`close_resolve`（remember 先重读合并写盘再隐藏/退出）、`open_settings_file`（经 `ensure_toml`：遗留先迁移，否则写模板） | e32be7c |
| 任务 3 | 前端：`CloseConfirmDialog`（NModal elevated 玻璃 + 最小化/退出/取消 + 记忆勾选）；App 监听 `close-requested`（重复去重、unmount 取消监听、非 Tauri 环境 `.catch` 兜底）；resolve 经 `close_resolve` 回传 | 91105dd |
| 任务 4 | 设置页：「应用」组「关闭窗口时」三选一（写盘参数 minimize/quit/null，记忆值回显）；底部「高级配置」组（settings.toml 说明 + 打开设置配置文件） | 6eec435 |
| 任务 5 | CLAUDE.md 关窗行为描述更新；全量门禁；执行记录 | 见最终提交 |

门禁：前端 typecheck / format:check / 15 文件 145 用例 / build 全绿；
根库 cargo fmt/clippy/test 与 src-tauri fmt/clippy/test 全绿（任务 1-5 每次提交均过 pre-commit 双 manifest 门禁）。
