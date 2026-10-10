# TokenScope 首次启动隐私同意实施计划

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| P01 | 同意字段严格读取与原子保存已实现 | — |
| P02 | 统一业务 command guard 已实现；当前 27 个而非旧 23 个 | — |
| P03 | 全部启动业务副作用延后至 Ready 已接线 | — |
| P04 | 首屏隔离、动态业务加载、本地政策已实现 | — |
| P05 | 拒绝与各退出路径已实现 | — |
| P06 | 文档/局部自动化已有；强原生取证与当前完整矩阵未完成 | [N10](../../active/2026-10-11-consolidated-remaining-work.md#n10)、[N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-09-first-launch-privacy-consent.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 日期：2026-10-09
- 状态：**已实施（P01–P06 代码与自动化验收完成，2026-10-10）；原生时序与发布验收待验，计划保持 active**
- 只读核对基线：main / 9b4c29b0bc2838108f8b52acdbff1bed0ab39d42
- 拟入库位置：docs/plans/archive/partial/2026-10-09-first-launch-privacy-consent.md
- 仓库规范：已阅读 AGENTS.md、CLAUDE.md、DESIGN.md 与 docs/plans/README.md。实际实施先写失败测试再实现；各任务记录测试名、验证命令与证据（见文末执行记录）。**未验项在文末单列，不写作通过**

## 一 目标

应用每次启动先检查 settings.toml 中的同意记录。文件不存在、记录缺失或为 false 时，主窗口只显示隐私政策与选择界面；用户明确同意并成功保存记录后，才允许业务读取、缓存、诊断日志及价格网络请求。

用户拒绝时进入“是否退出 TokenScope”的确认。确认退出则结束进程；取消退出则回到隐私政策，仍然不能进入业务页面或后台运行。

本计划把“没有配置文件”视为“没有有效同意记录”，不声称它一定是首次安装。用户删除设置、升级旧版、复制文件或恢复备份，也可能进入同一流程。

## 二 非目标和边界

1. 不引入账户、云端同意记录、遥测、在线政策下载、自动更新或完整协议版本管理体系
2. 本期使用一个布尔字段，不增加接受时间、用户标识、政策哈希、版本号或历史流水。未来若确需政策变更后重新同意，另立需求
3. 不承诺“启动前绝对不读任何文件”。运行程序、加载本地 UI/字体/WebView 以及判断已有同意必然涉及必要读取；明确允许的应用引导例外只有定位并读取 settings.toml、加载随程序打包的政策和 UI 资源、维护纯内存状态和窗口/单实例基础设施
4. 未同意时不主动读取 agent 来源目录、统计数据库、视图快照、价格文件、窗口状态或前端持久化偏好，不创建 TokenScope 业务日志/缓存/窗口文件，不请求价格服务，不读取或修改开机自启状态
5. WebView2、操作系统、安装器依赖下载及其独立诊断/更新不在应用业务闸门的控制承诺内。使用 IPC/本地资源协议不等同于外部价格网络请求
6. 本期同意状态按进程启动检查，在启动后由后端持有。外部编辑器改成 false 或删除配置在下一次启动生效；不新增实时撤回、监视文件或中止所有在途操作的复杂机制。普通设置写入必须保留同意字段
7. 本地布尔记录是偏好机制，不是防篡改的法律证明。同一用户手工写入 true 也会在下次启动被接受；不把这个文件当作强安全认证

### 必须保持的不变量

1. 未验证同意记录或保存未成功时，后端闸门始终关闭；UI 可见性不构成授权
2. 首次同意的文件原子提交必须先于任意业务读取、写入和价格请求
3. 所有应用内设置写者继续共享同一事务锁，保存其他选项不得丢失同意记录
4. 读取或保存失败不覆盖坏文件、不回退成已同意；旧 JSON 和默认设置不构成同意
5. 拒绝后没有托盘常驻或后台业务；取消退出只返回协议界面
6. 重复点击、重载及并发 IPC 不重复初始化，晚到响应不复活退出中的应用
7. 所有测试隔离数据目录、价格索引和 WebView 数据，不读取真实来源日志

## 三 当前代码和需要截断的位置

源码链接均固定到本次核对的提交，避免后续 main 行号变化。

| 位置 | 当前行为 | 计划变化 |
| --- | --- | --- |
| [src/settings.rs:82–94、134–154](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src/settings.rs#L82-L154) | 缺 TOML 时尝试遗留 JSON，再回默认值；读取本身不建文件 | 新增默认 false 的同意项；引导检查必须区分“真实 TOML 存在且有效”与默认值/旧 JSON |
| [src-tauri/src/lib.rs:20–95](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src-tauri/src/lib.rs#L20-L95) | 构建窗口前初始化日志；setup 恢复窗口、创建托盘、启动状态保存及价格线程 | 拆成最小引导初始化和同意后的单次业务初始化 |
| [src/logging.rs:40–71](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src/logging.rs#L40-L71) | 初始化会创建 logs 目录并打开文件 | 延迟到同意成功；引导失败只通过内存 DTO/UI 显示，不提前建立永久 stderr subscriber 阻挡后续 logger |
| [src-tauri/src/lib.rs:135–235、400–421](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src-tauri/src/lib.rs#L135-L235) | 读取 window-state.json；每秒/退出时落盘 | 未同意时不恢复、不启动 saver；退出直接退出，不走最终保存 |
| [src-tauri/src/lib.rs:429–484](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src-tauri/src/lib.rs#L429-L484) | 启动后 120 秒检查价格同步，此后每小时检查 | 线程仅在同意后启动；120 秒从本次业务解锁开始计时 |
| [src-tauri/src/lib.rs:504–535](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src-tauri/src/lib.rs#L504-L535) | 启动即托盘，退出旁路会最终保存窗口状态 | 托盘同意后才创建，未同意不能隐藏到托盘 |
| [frontend/src/App.vue:28–41、75–84](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/src/App.vue#L28-L84) | 启动调用颜色/主题，挂载立即预读设置及诊断 | 根组件改为无业务副作用引导壳；正常应用组件在后端 ready 后动态导入 |
| [frontend/src/lib/settingsPreload.ts:3–31](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/src/lib/settingsPreload.ts#L3-L31) | 自动读 source_status/cache_stats/pricing_entries/settings_get/autostart_status | 预读只在业务组件挂载后开始 |
| [Dashboard.vue:438–476、551–580](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/src/views/Dashboard.vue#L438-L580) | setup 就读视图快照、来源并建立查询 | 同意前不挂载，也不能通过静态导入执行相关模块初始化 |
| [theme.ts](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/src/composables/theme.ts)、[timezone.ts](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/src/composables/timezone.ts)、[tokenColors.ts](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/src/composables/tokenColors.ts) | 模块求值会读取 localStorage；前两者还有立即写入的 watchEffect | 推迟持久化初始化；引导壳只用系统主题和默认样式 |
| [frontend/public/theme-boot.js](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/frontend/public/theme-boot.js) | HTML head 阶段读存储主题 | 首帧只解析系统主题，不读存储；同意状态确认后、业务挂载前再恢复保存偏好 |
| [commands.rs:354–409](https://github.com/ywltby/tokenscope/blob/9b4c29b0bc2838108f8b52acdbff1bed0ab39d42/src-tauri/src/commands.rs#L354-L409) | close_resolve 可记忆/最小化；open_settings_file 可首建 | 两者不能作为未同意时的旁路 |

## 四 配置和读取规则

新增顶层字段 privacy_policy_accepted: bool，serde 缺省值 false；Settings::default() 也为 false。

保存后的示例：

```toml
privacy_policy_accepted = true
price_auto_sync = true

[sources]
```

具体序列化排版沿用现有 TOML writer，不要求精确输出上面的空表。价格同步、来源、关闭行为等现有值全部保留，不因同意而重置。模板/字段说明写清“缺失或 false 时下次启动需要同意；只在用户同意且成功保存后由程序设为 true”。

引导判定：

| 磁盘状态 | 引导状态 | 是否允许业务 |
| --- | --- | --- |
| 无 settings.toml，无旧 JSON | 显示政策 | 否 |
| 有有效 TOML，同意字段缺失或 false | 显示政策 | 否 |
| 有有效 TOML，同意字段 true | 进入同意后的初始化 | 初始化完成后允许 |
| 无 TOML，有旧 settings.json | 仍显示政策，不将旧文件或默认值视为已同意 | 否 |
| TOML 损坏、字段类型错误、路径为目录、无权限读取 | 显示“无法确认隐私设置”及原因、重试和退出入口 | 否 |
| settings 路径无法定位 | 显示错误 | 否 |

不要在判定函数中调用 ensure_toml()，也不要把 load().unwrap_or_default() 当成授权。使用严格读取结果，仅 NotFound 才按真正缺失处理；PermissionDenied、I/O 错误等不能被 Path::exists() 吞成“新安装”。

遗留 JSON 只在用户点击同意后的保存事务中按既有规则读取，保留其中设置并写成 TOML。缺 TOML 时，即使 JSON 中人为加入同意字段，也必须先询问。损坏 JSON 会使同意保存失败，保留原文件并继续阻断；不静默覆盖。

损坏配置的恢复方式先提供明确路径、原因和“修复后重试”，允许用户自行处理后重新检查。此次不添加自动删除、重置或备份损坏文件的流程，也不在未同意时调用“打开设置文件”命令创建空模板。

## 五 后端强制闸门

### 5.1 生命周期

建议新增 src-tauri/src/privacy.rs，集中管理进程内状态：

- Checking：正在检查必要设置，默认封闭
- NeedsConsent：等待选择
- SavingConsent：用户已经点击同意，但原子保存还未成功
- Starting：已有可靠同意记录，正在单次初始化业务
- Ready：允许业务命令和后台任务
- BlockedError：读取/保存失败，继续封闭
- 退出确认由引导流程管理；确认退出后标记退出中，拒绝新的同意请求

未经后端 Ready，前端布尔值、DOM 变化、事件伪造或设置页调用都不能让业务执行。UI 状态只是展示结果。

先创建必要 managed state，包括空的窗口内存容器和未初始化的日志状态，再接入窗口事件，避免把 restore_window_state 延后之后出现 app.state 尚未注册的 panic。状态读取和磁盘操作放后台线程；锁只保护短时转换，不跨 await 或窗口主线程调度持有。

### 5.2 最小引导命令

命名可在实现时统一，语义固定：

1. privacy_bootstrap：只检查必要设置并返回状态及打包政策；已有同意时通过单次初始化器进入 Ready。重复调用、前端重载不重复创建线程、托盘或 logger
2. privacy_accept：只对应用户明确点击“同意并继续”。后端串行化，调用 settings::update，在锁内读取最新设置、仅把同意字段设 true、原子保存成功，再进入业务初始化并返回状态
3. privacy_exit_resolve：只处理未解锁时的“退出/返回政策”，不读 close_action、不写设置、不最小化、不最终保存窗口状态
4. 必要的 privacy_retry 可以复用 bootstrap；不开放任意文件路径、任意命令调用或通用 opener

政策内容在构建时由 docs/privacy.md 编译/打包进入本地资源，建议用 include_str!，不要运行时访问 GitHub。引导接口不返回来源目录清单、缓存内容、历史查询或价格状态。

### 5.3 保存顺序与并发

1. 后端单次转换取得 SavingConsent；重复提交返回同一结果/当前状态，不能并行写入
2. 在现有 settings::update 事务锁内基于最新磁盘值合并字段
3. 沿用 fsutil::atomic_write：同目录临时文件、sync_all、原子替换；任何失败保持 gate 关闭，不得只在内存勾成 true
4. 保存成功后才进入 Starting；成功持有的同意记录不能被稍后普通设置写入覆盖丢失
5. 初始化只做一次；成功后变为 Ready，前端才挂载业务组件。重复 accept、重复 bootstrap 和晚到响应不能重复启动
6. 保存前进程中断：下次仍询问。保存成功而响应丢失：下次或 bootstrap 重查已有 true 后恢复，不再要求重复记录；不要因超时回滚已成功的文件
7. SavingConsent 期间界面按钮禁用；窗口 X/重复拒绝不穿插执行另一条关闭动作，显示正在保存并在操作结束后可继续关闭。后端同时拒绝相冲突请求，不能只靠禁用按钮
8. 文件锁毒化、磁盘满、目录不可写、原子替换失败均作为可重试错误展示；失败不能启动后台任务

业务初始化须区分非致命降级与关键失败。日志不可用沿用原有非阻断降级；托盘/窗口恢复等失败需显示可操作诊断，不能伪装成功，也不能盲目重试导致重复线程。每个一次性资源有明确成功标记，重试只补未完成部分；开始业务读取/价格工作必须建立在同意已经成功保存的事实之上。

### 5.4 全部业务入口受保护

审计 lib.rs 的 generate_handler! 全表，在任何文件读取、路径扫描、系统自启检查、写入、opener 或网络执行前调用统一 guard。建议通过清晰的受保护命令包装器或逐命令公共 guard 实现，并用命令覆盖测试避免新增命令漏门。

必须覆盖：

- summarize、list_events、query_begin、query_summary、query_events
- source_status、source_config_set
- cache_stats、refresh_cache
- view_cache_load、view_cache_save
- pricing_entries、pricing_status、open_pricing_file、sync_pricing_openrouter
- settings_get、settings_set_price_auto_sync、settings_set_close_action、open_settings_file
- autostart_status、autostart_set
- startup_diagnostics、现有 close_resolve

未同意时返回稳定错误码 privacy_consent_required；Checking/Saving/Starting 不得当作 Ready。被拒绝的命令不能先调用 run_blocking 的业务闭包，也不能通过错误日志触发文件 logger 初始化。

后台价格同步、窗口保存、托盘回调也必须通过同一生命周期约束，不能仅保护 IPC。保留单实例插件，只负责把同一引导窗口唤到前台。审计 opener/autostart 插件初始化是否有提前读写；凡有业务副作用的初始化都延迟，或保持未开放的纯注册部分。保持现有 capabilities 最小权限，不为政策展示增加通用文件/网络权限。

## 六 前端引导和关闭体验

### 6.1 结构

- App.vue 保留为最小根引导壳；将现有主应用内容移为 MainApp.vue（或同义名称）
- 根壳只使用 Vue、必要 UI 基础样式、本地政策展示与引导 IPC；不能静态导入有模块级副作用的主题/时区/颜色模块、Dashboard、Settings 或价格横幅
- 后端 Ready 后才动态 import 主应用，恢复持久化主题/颜色/时区，再挂载主内容和启动预读
- 注意仅 v-if 不足以避免静态 import 的模块级 localStorage 副作用；必要时将持久化初始化改为显式函数，确保测试可证明何时发生
- Checking 显示短暂“正在检查隐私设置”，没有统计页背景或旧视图闪现；无记录时显示政策
- 不使用固定延迟、先启动后遮罩、先挂 Dashboard 再移除的方式
- 引导主题默认跟随系统，不读取本地偏好；保存偏好在业务挂载前恢复，更新首帧主题相关测试，记录这项有意行为变化

### 6.2 政策弹窗

新增 PrivacyConsentDialog.vue，沿用 DESIGN.md 的语义 token、浮层材质和焦点规则。显示标题、政策日期、简短处理摘要和可滚动全文；全文可在离线环境完整阅读。

操作为“不同意”和“同意并继续”，不预选同意，不把关闭弹窗或继续使用当作同意。不新增强制滚到底或倒计时机制。用户明确点击后展示保存中；失败保留弹窗和原因，可重试或退出。

政策来源只有 docs/privacy.md，构建期打包一份；不要另写一份会漂移的正文。展示使用安全文本或禁用原始 HTML 的确定性渲染，不引入远程图片、字体、脚本、iframe、预取或自动打开第三方链接。未同意时第三方 URL 只作文本展示，避免从同意页产生外部访问。

### 6.3 拒绝和退出

“不同意”、隐私弹窗关闭意图、窗口 X、Alt+F4，均进入专属确认：

- 标题：退出 TokenScope？
- 说明：尚未同意隐私政策，应用不会开始读取使用数据或同步价格
- 按钮：返回隐私政策 / 退出程序
- 不提供“最小化到托盘”或“记住选择”
- 取消、Escape、遮罩关闭退出确认，只回到政策；仍然阻断
- 隐私弹窗自身 Escape/遮罩不能消失后露出可操作主页面，可保持不关闭或进入同一退出确认；统一测试固定行为

未同意时不能调用 close_decision_now 读取和执行旧 close_action，也不能复用 quit_with_final_save。应有直接结束进程的引导退出分支。已同意并 Ready 后恢复既有三态关闭和托盘逻辑，回归 AP07 最终保存及失败提示。

窗口关闭事件在前端事件监听尚未就绪时也要记录待处理的退出询问状态，由 bootstrap 响应恢复显示，不能只 emit 一次后丢失。重复 X 只出现一个确认，不能堆叠弹窗。确认退出后若发生晚到异步响应，不能重新挂载主应用。

## 七 实施任务

### P01 配置字段与严格引导读取

- 代码：src/settings.rs；tests/settings_transactions.rs；必要时 src/fsutil.rs 的已有测试补充
- 新增布尔字段、默认值和模板说明；引导只以真实有效 TOML 为准；NotFound 与其他 I/O 错误分开
- 同意保存复用事务锁，保留现有设置；旧 JSON 迁移只在用户同意后发生
- 拟新增测试：
  - privacy_missing_toml_requires_consent
  - privacy_missing_field_and_false_require_consent
  - privacy_valid_true_allows_bootstrap
  - privacy_legacy_json_never_grants_bootstrap
  - privacy_corrupt_or_unreadable_settings_fail_closed
  - privacy_accept_preserves_existing_settings
  - privacy_accept_migrates_legacy_after_user_action
  - privacy_accept_save_failure_keeps_gate_closed
  - privacy_concurrent_updates_preserve_acceptance
- 验证：cargo test --locked --workspace privacy；cargo test --locked --test settings_transactions

### P02 后端生命周期和所有入口闸门

- 代码：新增 src-tauri/src/privacy.rs；src-tauri/src/lib.rs；src-tauri/src/commands.rs
- 管理引导状态、最小 IPC、Ready guard 和单次初始化；逐项审计所有已注册命令与插件直接能力
- 拟新增测试：
  - privacy_all_business_commands_reject_before_ready
  - privacy_guard_rejects_without_calling_business_closure
  - privacy_accept_publishes_ready_only_after_atomic_save
  - privacy_double_accept_starts_runtime_once
  - privacy_reload_bootstrap_is_idempotent
  - privacy_late_response_cannot_reopen_exiting_app
- 验证：cargo test --locked --manifest-path src-tauri/Cargo.toml privacy

### P03 延后所有启动副作用

- 代码：src-tauri/src/lib.rs；src/logging.rs；src-tauri/src/window_state.rs；必要时 src-tauri/src/acceptance.rs
- logger、窗口恢复/saver、托盘、价格线程统一迁入同意后的初始化；先注册空内存 holder
- WorkerGuard 生命周期调整为进程托管，不能在局部函数返回后提前丢弃；日志诊断 DTO 的初始/更新状态要一致
- 自动同步保留“默认开启、按快照到期判断、每小时轮询”，初次等待从业务解锁后开始
- 拟新增测试：
  - privacy_bootstrap_has_no_business_io
  - privacy_window_events_before_ready_do_not_persist
  - privacy_timer_cannot_sync_while_waiting_for_consent
  - privacy_runtime_initialization_runs_once
  - privacy_saved_consent_then_restart_recovers
- 验证：cargo test --locked --manifest-path src-tauri/Cargo.toml privacy；cargo test --locked --workspace logging

### P04 首屏隔离与本地政策

- 代码：frontend/src/App.vue；新增 frontend/src/MainApp.vue、frontend/src/components/PrivacyConsentDialog.vue、前端引导状态模块及测试；frontend/public/theme-boot.js；相关 theme/timezone/tokenColors 初始化；src-tauri/src/privacy.rs
- 先显示引导，Ready 后动态挂载；政策构建期打包；UI 不发业务预读；不读取持久化偏好
- 拟新增测试：
  - bootstrap_does_not_import_or_mount_business_app
  - consent_screen_makes_only_bootstrap_calls
  - consent_screen_does_not_access_local_storage
  - policy_is_complete_and_available_offline
  - accept_waits_for_backend_ready
  - failed_accept_keeps_policy_visible
  - repeated_accept_is_single_flight
  - accepted_start_restores_preferences_before_main_mount
- 验证：pnpm --dir frontend test；pnpm --dir frontend typecheck；pnpm --dir frontend build

### P05 拒绝和全关闭路径

- 代码：src-tauri/src/privacy.rs、lib.rs、commands.rs；PrivacyConsentDialog.vue；新增退出确认组件或 CloseConfirmDialog.vue 的独立受限模式
- 引导退出完全避开旧记忆动作和最终保存；取消回政策；监听前 X、保存中 X 和重复事件有确定性行为
- 拟新增测试：
  - reject_opens_exit_confirmation
  - cancel_exit_returns_to_blocked_policy
  - preconsent_close_ignores_remembered_minimize_and_quit
  - preconsent_exit_skips_final_window_save
  - close_before_listener_is_recovered_by_bootstrap
  - close_while_saving_does_not_race_accept
  - ready_close_preserves_existing_behavior
- 验证：cargo test --locked --manifest-path src-tauri/Cargo.toml；pnpm --dir frontend test

### P06 文档和验收工具

- 代码/文档：docs/privacy.md；docs/plans/README.md；本计划；docs/plans/d5-acceptance-checklist.md；必要时 DESIGN.md；frontend/scripts/check-ui-contracts.mjs、frontend/scripts/native-acceptance.mjs、相应 fixtures 和 scripts/prepare-native-acceptance.ps1
- 实现并验证后再更新政策“尚无程序内同意”表述，准确写明先同意后读取/同步、必要引导例外及第三方边界。同步起算由“应用启动”改为“确认同意并进入业务运行”，不要提前发布为已实现
- 测试 fixtures 明确区分“已经同意的常规业务回归”和“真实未同意的引导测试”；不能为让旧测试通过而在公共 mock 中无条件返回已同意
- 拟新增验收契约：
  - privacy_no_business_effects_before_acceptance
  - privacy_rejection_never_leaves_tray_process
  - privacy_acceptance_survives_restart
  - privacy_denied_ipc_has_no_filesystem_or_network_effect
  - privacy_packaged_policy_matches_source
- 验证：全量门禁；隔离原生 release+acceptance 测试；Windows 正式 NSIS 冒烟。所有尚未执行项必须标为未验

## 八 全量验证命令

以下是实施阶段应运行的命令，本计划编写阶段未执行这些构建或测试。根库和 Tauri 壳为独立 manifest，必须分别验证。

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
pnpm --dir frontend typecheck
pnpm --dir frontend format:check
pnpm --dir frontend test
pnpm --dir frontend build
cargo clippy --locked --workspace --all-targets --features acceptance -- -D warnings
cargo test --locked --workspace --features acceptance
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets --features acceptance -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --features acceptance
python -m unittest discover -s scripts -p "test_release_metadata.py" -v
node ./frontend/node_modules/@tauri-apps/cli/tauri.js build --ci --bundles nsis --target x86_64-pc-windows-msvc -- --locked
```

UI/原生验收按已有脚本的实际参数运行，扩展具名契约并记录完整命令，不猜测不存在的 flags。引导政策更新后继续检查生产 CSP；不为展示政策放宽脚本/外部连接权限。

## 九 验收矩阵

所有测试只使用临时隔离根、合成 agent 日志与受控网络/文件 spy。不能读取真实会话日志。原生 acceptance 模式沿用隔离根和独立 WebView 数据目录，不污染实际用户配置。

1. 全新空目录：窗口先出现政策；至少等待超过原来的 120 秒门槛，仍无价格请求、来源扫描、数据库、价格快照、日志、视图及窗口状态写入
2. 预置已有缓存/日志来源和旧视图但无同意：不能读取或闪现旧业务数据；不能仅以“没有新文件”证明没有读取，必须用 I/O 追踪/注入计数验证
3. 缺字段、false、仅旧 JSON、TOML 破损/类型错误/不可读：全部不解锁；破损原文件逐字节保持
4. 用户同意：先观察同意 TOML 原子保存完成，再观察第一条业务读取/网络请求；文件路径和字段正确
5. 保存失败：磁盘/权限/目标目录等注入故障后仍显示政策，业务计数零；恢复后重试能正常继续
6. 拒绝后确认退出：进程结束，无托盘残留、无同意 true、无窗口最终保存。拒绝后取消退出：回到政策，等待期间仍零业务操作
7. 预置 close_action=minimize 或 quit，但同意 false：X/Alt+F4 仍询问退出，不能隐藏或自动退出，不能绕过引导
8. 双击同意、重复 bootstrap、前端刷新、重复 X、保存中 X：只一份写事务和一套 runtime；不出现多个线程/托盘/弹窗
9. 重启：有有效 true 时不再询问；false/字段删除/文件删除后重新询问；没有 TOML 的旧 JSON 不视为授权
10. 人工直接调用每一条受保护 IPC：未 Ready 时统一拒绝，业务闭包未运行，不产生文件/网络/注册表/opener 副作用
11. 政策完全离线可读，键盘 Tab 焦点圈在弹窗内，按钮和滚动区域可达；浅/深系统主题、980×620 最小窗口、Windows 100%/125%/150% 缩放不裁字
12. 已同意用户回归：来源配置、视图恢复、价格同步、自启、主题偏好、正常记忆关闭、托盘退出及最终状态保存均保持有效
13. 正式 NSIS 新装与保留配置升级分别验收；安装器/WebView2 独立行为单列证据，不把它们误计成价格请求，也不宣称已被本功能阻止

自动化测试应使用可控时钟覆盖 120 秒门槛，不让每个单元测试真实等待；另做一次有真实时间及进程归属记录的原生等待验收。记录“必要 settings 读取 → 用户点击 → 原子保存完成 → Ready/业务初始化 → 第一条业务读取/网络”的时间顺序，不能仅凭弹窗截图或全量测试绿判通过。

## 十 完成条件和交付

- P01–P06 每项都有对应代码、具名测试及命令结果；失败/未验项单独列出
- 未同意时只允许本计划明确列出的引导例外，其他业务入口和后台路径均被后端拦截
- 同意记录先落盘后解锁；读取、保存失败保持封闭，不覆盖损坏文件
- 拒绝/关闭/取消路径与预先记忆的关闭设置隔离
- 政策说明与实际打包行为一致，Windows 原生时序证据成立
- 经用户确认实施后才改产品代码；提交与发布仍按当时用户指示执行，本计划不构成发布授权
- 完成验收后再按仓库规范归档计划，更新 docs/plans/README.md 状态与完成时间，不提前标完成

## 十一 执行记录（2026-10-10）

实施基线 `78d3266`（工作区含用户其他计划未提交改动，本计划只提交自己的文件）。

### P01 配置字段与严格引导读取（完成）

- 代码：`src/settings.rs`（顶层 `privacy_policy_accepted`（serde 默认 false）、`ConsentRead`、`read_consent`（NotFound 与其他 I/O 分道）、`accept_privacy_policy`（复用 SF02 事务锁）、模板字段说明）、`tests/settings_transactions.rs`
- 测试（全部通过）：`privacy_missing_toml_requires_consent`、`privacy_missing_field_and_false_require_consent`、`privacy_valid_true_allows_bootstrap`、`privacy_legacy_json_never_grants_bootstrap`、`privacy_corrupt_or_unreadable_settings_fail_closed`、`privacy_accept_preserves_existing_settings`、`privacy_accept_migrates_legacy_after_user_action`、`privacy_accept_save_failure_keeps_gate_closed`、`privacy_concurrent_updates_preserve_acceptance`
- 命令：`cargo test --locked --workspace privacy`（9 项，含 4 项事务用例）；`cargo test --locked --test settings_transactions`

### P02 后端生命周期和所有入口闸门（完成）

- 代码：新增 `src-tauri/src/privacy.rs`（Phase 状态机、单飞门 + Condvar 等待落定、`require_ready`、`privacy_bootstrap` / `privacy_accept` / `privacy_exit_resolve`、`BOOTSTRAP_COMMANDS` / `PROTECTED_COMMANDS` 分类表、真值 DTO）；`src-tauri/src/lib.rs` 注册引导命令；`src-tauri/src/commands.rs` 全部 23 个业务命令首行调用统一 guard（拒绝时**不进入** `run_blocking` 业务闭包）
- 测试：`privacy_all_business_commands_reject_before_ready`（扫描 `generate_handler!` 全表：每个命令必须在分类表内、受保护命令必须含 guard 调用）、`privacy_guard_rejects_without_calling_business_closure`、`privacy_accept_publishes_ready_only_after_atomic_save`、`privacy_double_accept_starts_runtime_once`、`privacy_reload_bootstrap_is_idempotent`、`privacy_late_response_cannot_reopen_exiting_app`
- 命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml privacy`

### P03 延后所有启动副作用（完成）

- 代码：`src-tauri/src/lib.rs`（`LogState` holder + `initialize_business_runtime`（只补 `RuntimeFlags` 未完成步骤）、`install_window_state_holder`、窗口事件按阶段分流、价格线程改为同意后启动且每轮经 `price_sync_due` 判定）；`privacy.rs`（`RuntimeFlags`/`RuntimeStep`、`WindowEventAction`、`price_sync_due`）
- 行为：未同意时不创建 `logs` 目录、不恢复/保存窗口状态、不建托盘、不起价格线程；日志状态 `pending` 只在 Ready 后经 `startup_diagnostics` 读取
- 测试：`privacy_bootstrap_has_no_business_io`、`privacy_window_events_before_ready_do_not_persist`、`privacy_timer_cannot_sync_while_waiting_for_consent`、`privacy_runtime_initialization_runs_once`、`privacy_saved_consent_then_restart_recovers`
- 命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml privacy`；`cargo test --locked --workspace logging`

### P04 首屏隔离与本地政策（完成）

- 代码：`frontend/src/App.vue`（引导壳：检查/政策/阻塞三态 + Ready 后 `defineAsyncComponent` 动态挂载）、`frontend/src/MainApp.vue`（原 App.vue 整体迁入）、`frontend/src/components/PrivacyConsentDialog.vue`（遮罩与 Esc 不关闭、不预选同意）、`frontend/src/lib/privacyGate.ts`（引导状态 + 单飞）、`frontend/src/lib/policyMarkdown.ts`（安全确定性渲染，链接降级为纯文本）、`frontend/public/theme-boot.js`（首帧只跟随系统，不读偏好）、`src-tauri/src/privacy.rs`（`include_str!("../../docs/privacy.md")` + `PolicyInfo`）
- 测试：`bootstrap_does_not_import_or_mount_business_app`、`consent_screen_makes_only_bootstrap_calls`、`consent_screen_does_not_access_local_storage`、`policy_is_complete_and_available_offline`、`accept_waits_for_backend_ready`、`failed_accept_keeps_policy_visible`、`repeated_accept_is_single_flight`、`accepted_start_restores_preferences_before_main_mount`（偏好读取早于业务挂载）；另 `renders_headings_list_and_paragraphs_in_source_order` 等渲染用例
- 命令：`pnpm --dir frontend test`（31 文件 / 348 测试通过）、`pnpm --dir frontend typecheck`、`format:check`、`build` 全绿
- 有意行为变化：首帧主题不再读持久化偏好（引导期跟随系统），偏好恢复改为 Ready 后、业务挂载前的显式 `applyResolvedThemeAttribute()`；`theme.test.ts` 的 `preference_resolution_is_shared` 已按新语义改写

### P05 拒绝和全关闭路径（完成）

- 代码：`frontend/src/components/PrivacyExitDialog.vue`（专属确认：无记忆、无最小化；Esc/遮罩=取消）、`privacyGate.ts`（`requestExitPrompt`/`confirmExit`/`cancelExit`，保存期间只记待处理）、`src-tauri/src/privacy.rs`（`privacy_exit_resolve`：`exit=true` 直接 `app.exit(0)`，不读 `close_action`、不写设置、不做窗口状态最终保存）、`lib.rs`（非 Ready 的 `CloseRequested` 只置待处理标记并 emit，前端监听前到达的请求由 bootstrap 响应的 `exitPromptPending` 恢复）
- 测试：`reject_opens_exit_confirmation`、`cancel_exit_returns_to_blocked_policy`、`close_before_listener_is_recovered_by_bootstrap`、`close_while_saving_does_not_race_accept`、`privacy_window_events_before_ready_do_not_persist`、`privacy_late_response_cannot_reopen_exiting_app`、`ready_close_preserves_existing_behavior`（原 19 项关闭回归迁入 `MainApp.test.ts` 后仍全绿）
- 命令：`cargo test --locked --manifest-path src-tauri/Cargo.toml`；`pnpm --dir frontend test`

### P06 文档和验收工具（完成，含 1 处真实缺陷）

- 文档：`docs/privacy.md`（先同意后读取/同步的实际行为、引导期必要例外、同步计时起算点、控制手段与本地性说明、程序内同意机制已实现）、`docs/plans/d5-acceptance-checklist.md`（新增 5.6–5.8，改写 2.2 / 3.4 / 5.4）
- 契约与夹具：`frontend/scripts/fixtures/ui-contracts.mjs`（引导命令基线 + `privacy-consent-pending` fixture + `PRIVACY_PROTECTED_COMMANDS`）、`frontend/scripts/contracts/interaction-contracts.mjs`（`privacy_no_business_effects_before_acceptance`、`privacy_denied_ipc_has_no_filesystem_or_network_effect`；`prepaint_theme_has_correct_canvas` 按 P04 新语义改写为"首帧只跟随系统、同意后恢复偏好"）、`frontend/scripts/native-acceptance.mjs`（场景 `privacy-consent-gate`、`privacy-accept-restart` + 启动前删除同意记录的 mutate）、`scripts/prepare-native-acceptance.ps1`（隔离根 settings.toml 预置 `privacy_policy_accepted = true` 作为已同意基线，未同意场景由驱动脚本删除该行构造）、`frontend/scripts/check-app-scroll.mjs`（样式抽取改指向 `MainApp.vue`）、Rust 侧 `privacy_packaged_policy_matches_source`（打包政策与源文件逐字一致 + 九章齐全 + 标题日期可解析）
- 命令与结果：
  - `node frontend/scripts/check-ui-contracts.mjs --url http://127.0.0.1:1437 --phase verify --output qa-artifacts/privacy-consent --fixture normal --theme light --viewport 1280x820 --contract privacy_no_business_effects_before_acceptance,privacy_denied_ipc_has_no_filesystem_or_network_effect` → exit 0，两条契约各 8 条断言通过，`normal-light-1280x820` 硬约束全过
  - `node frontend/scripts/check-ui-contracts.mjs … --viewport 980x620 --contract prepaint_theme_has_correct_canvas,privacy_no_business_effects_before_acceptance,privacy_denied_ipc_has_no_filesystem_or_network_effect` → exit 0（`prepaint_theme_has_correct_canvas` 25 条断言、两条隐私契约各 8 条）
  - 首次运行契约时捕获真实回归：引导壳与主应用各自渲染 `NGlobalStyle`，naive 报 "More than one n-global-style exist"（`no_console_error` 硬失败）。已改为全局样式只在引导壳注入一份，复跑转绿
  - 改造 `prepaint_theme_has_correct_canvas` 时另修正两处量测口径：首帧在 dev 下入口被拦住、CSS 未加载，画布颜色改为"样式可用才判定"；终态 `body` 背景会被 naive 全局样式覆盖，改用主题语义 token `--ts-canvas` 判定
  - `node --check` 三个验收脚本通过；`cargo test --locked --manifest-path src-tauri/Cargo.toml privacy_packaged` 通过

### 未验项（不得写作通过）

1. 原生（release + `acceptance` 构建）场景 `privacy-consent-gate`、`privacy-accept-restart` 未执行——需构建隔离产物并真实驱动窗口/CDP
2. 验收矩阵第 1 项"超过原有 120 秒门槛仍无价格请求"的真实等待观察未执行（已有 `price_sync_due` 单测与线程启动时序保证，但缺少原生计时证据）
3. 浏览器矩阵全量复跑（60 场景）未执行：本轮只跑 `normal-light-1280x820` 单场景 + 两条新契约
4. Windows NSIS 安装/升级（D5 2.x）与 100%/125%/150% 缩放下的政策弹窗排版现场未验
5. `pnpm --dir frontend build` 与 `tauri build` 的打包产物内政策文本核对（`include_str!` 已由 Rust 测试守住源文件一致性，但未在安装包内二次核对）
