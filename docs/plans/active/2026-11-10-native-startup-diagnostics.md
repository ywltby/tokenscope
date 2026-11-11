# 原生启动 about:blank 取证实施计划

> 用户已于 2026-11-10 确认继续推进。按任务顺序实施，先补失败测试再实现。

> 交付状态：PR #60 已合并（`46f0dc7`），[v0.7.4](https://github.com/ywltby/tokenscope/releases/tag/v0.7.4) 已发布；[Release 42767766041](https://github.com/ywltby/tokenscope/actions/runs/42767766041) 的 19 个作业（含安装、12 组单跳与双链）成功，主线 CI/Native UI 最终通过。主线 contracts 首轮 tasklist 0.5 秒探针超时，仅重跑失败作业后恢复，探针稳定性未宣称修复。本轮没有复现真实启动空白，根因仍开放。当前用户优先任务见[筛选与图表修复](2026-11-10-filter-query-chart-performance.md)。
>
> **2026-11-12 根因定位（PR #64，待审查）**：PR #62 失败轮 artifact 13822755444 的完整证据链给出同次故障时序——2209 ms 导航请求（NavigationStarting）、2012–22012 ms Playwright 只见 about:blank、20 秒页面预算耗尽判失败、~22.9 s 失败诊断显示 raw target url 已是应用地址而 frame 仍是 about:blank、**23953 ms 提交（ContentLoading）→ 24149 ms 完成 → 24159 ms 应用页 CSP console error**：提交比失败判定晚 1.9 秒，页面真实运行。最小转储（20 线程）显示主线程 GetMessage 泵正常、无线程停在 wry `wait_with_pump`、无 renderer 崩溃——**排除既有无限等待假设，排除观察器失同步，排除崩溃**。机制：WebView2 在 NavigationStarting 即改写 `/json/list` target url（暂挂提交），frame URL 等提交完成才更新；慢提交轮（实测 21.7 s）里 Playwright `page.url()` 全程 about:blank，驱动按 frame URL 归属在 20 秒预算处误杀健康启动。三次已知失败（58487a4、3eba0ba、09c71b8）同型。**修复（PR #64，驱动归属语义，预算与失败断言不变）**：页面阶段并行读取权威 `/json/list`（同一 `/tauri/i` 匹配、同一 20 秒预算），原始列表已挂应用地址时按 CDP `targetId` 定位 Playwright Page 附加（可先于提交）；target 不存在仍在预算内按原形态失败，列表请求失败回落 Playwright 判定。红→绿：`rawTargetDecision` 5 项反例先红后绿；前端全量 1121 项通过；PR #64 最新 HEAD `57aade9` 20 项检查全成功，且 CI 实跑证据显示 9 个场景中 6 个经新路径附加（`page_target_attachment.via=raw-target-list`、`attached_before_commit=true`）全部通过。版本不变（`release: false`）。**未自动合并**：PR 侧的通过不构成 main 上的修复完成宣告，合并后连续链与发布验证另按仓库约定执行。

**目标**：下一次启动空白时，在清理进程前留下足够证据，区分原生窗口创建挂起、真实导航失败与 Playwright 观察失同步；本批不宣称已修复空白根因。

**架构**：仅验收构建向 stderr 输出阶段标记；Node 启动器在原失败判定之后、有界收取独立 HTTP/CDP 只读快照和所属宿主的最小线程转储，再执行原有清理。原始失败不被诊断成功或失败覆盖。

**技术栈**：Rust/Tauri acceptance feature、Node 内置 fetch/WebSocket/child_process、PowerShell/.NET/DbgHelp、Vitest 与 Rust 测试。

## 已核实的事实

- main `58487a4` 的 Native UI run `42722004078` attempt 1：CDP 3,566 ms 就绪，随后页面阶段 20 秒只有 `about:blank`；宿主存活，空业务 trace，清理后才退出。
- 当前依赖 wry 0.57.0 / webview2-com 0.39.1：初始化脚本注册发生在 Navigate 前；回调通道断开后 `wait_with_pump` 仍会无限等待，独立最小探针已验证。但本项目尚无同次故障的原生调用栈，不能据此认定根因。
- 同一干净 main 本机 27 次隔离启动均未复现。研究资料在 gitignored `qa-artifacts/aboutblank-research-20261110/`。

## 不变量

1. CDP 45 秒、页面 20 秒、总启动 65 秒与现有退出判定保持原样。先确定失败，再进入单独有界诊断，不把诊断时间算成启动成功机会。
2. 不执行 Page.navigate/reload、自动重启或调试器恢复指令，不用重跑成功冒充根因修复。
3. 未同意前不初始化业务日志、不读写真实用户文件；阶段标记仅 stderr，且只编译进 acceptance。
4. HTTP/CDP 只读诊断仅访问本次已确认归属的本地端口；原始页面状态不用 Playwright 的缓存 URL 代替。
5. 转储仅针对本次宿主，核对父 PID、可执行文件路径和进程创建时刻；进程已退出、归属不符或权限不足均记 unavailable，不碰其他进程。
6. 转储采用最小栈/模块信息，不收取完整进程内存；保留匹配 EXE/PDB 与构建身份供故障后离线展开线程栈。产物复制/哈希也放在有界 helper 内，诊断自身出错不得阻止原有清理。
7. 用户要求：人工文件/目录清理一律送入 Windows 回收站；失败保留现场，不回退永久删除。

## D01：有界快照与判定测试

文件：新增 `frontend/scripts/lib/native-startup-diagnostics.mjs` 与对应 `.test.mjs`；修改 `native-instance.mjs` / `.test.mjs` 的 evidence 输出。

- 先验证旧 evidence 丢失启动诊断的红测。
- 用真实本地 HTTP 服务验证版本与 targets 读取、错误/超时可见；注入 CDP/转储边界验证全部步骤有界、失败互不覆盖。
- 原始 frame/location 已到应用而 Playwright 仍 blank 时，只记“观察不一致”；双方 blank 时只记“应用页未导航”，不自行猜根因。
- `buildLaunchesEvidence` 保留版本、阶段、页面观察与失败诊断。

命令：`pnpm --dir frontend exec vitest run scripts/lib/native-startup-diagnostics.test.mjs scripts/lib/native-instance.test.mjs scripts/lib/native-instance.async.test.mjs`。

## D02：验收宿主阶段与最小转储

文件：`src-tauri/src/acceptance.rs`；新增 `scripts/capture-native-startup-dump.ps1` 及受控进程契约测试。

- stderr 标记窗口构建前/后、构建失败、导航请求与页面开始/结束；标记无用户内容，仅记录阶段、窗口标签、PID、相对耗时与 URL 类别。
- 转储 helper 核对句柄对应的程序与创建时刻、CIM 父 PID；CreateNew 禁止覆盖文件。错误如实返回，输出编码固定 UTF-8。
- 用本测试创建的睡眠进程验证转储有效及错误身份拒绝；fixture 清理用回收站。

## D03：驱动接线与真实隔离验证

文件：`frontend/scripts/native-acceptance.mjs` 与必要契约。

- 页面 console/pageerror/crash 观察在 CDP 连接后立即安装，不再等到应用 URL 出现。
- 已拿到的 runtime 版本立即保留；页面轮询保留有界状态变化。
- failLaunch 在 cleanup 前收取快照/转储，诊断总预算固定，所有错误成为证据字段，原异常原样返回。
- 受控构建后跑一个正常隔离场景；用受控诊断测试证明挂起请求仍有界且清理继续。只对真实新失败分析，不人为制造“根因已复现”。

## D04：门禁与交付

- 同步计划总账入口、人工清理约定及本次实测；保留未定位结论。
- 按现有路径判定，Rust 验收模块改动仍触发产品版本校验，使用工具分配本未发布批次 patch，不放宽版本判定规则。
- 完成本地要求的检查、正常提交推送与 PR 最新 HEAD CI；禁止 `--no-verify`、跳过测试或放宽预算。未获额外指令不合并、不发布。
- 如 CI 捕获真实空白，先读阶段/独立 CDP/转储定位，再决定最小修复；如未捕获，交付为“取证能力已补齐，根因仍待同次故障栈确认”。

## 实施与本地证据（2026-11-10）

- 旧 `buildLaunchesEvidence` 确实丢失 runtime/诊断字段：新增断言先红（undefined），补字段后绿。只读 HTTP/CDP、超时取消、端口归属拒绝、三态观察、阶段解析及既有生命周期/环境装配共 65 项定向测试通过。
- Rust acceptance 阶段序列与 URL 脱敏单测通过；acceptance workspace clippy `-D warnings` 通过。受控睡眠进程的最小转储、三类错误身份拒绝和禁止覆盖契约通过，fixture 清理已送回收站。
- 真实隔离验证：`privacy-preseeded` 10/10，阶段含 build 前后及 navigation/page-load，独立原始 frame/location 为 `http://tauri.localhost/`、readyState=complete、Tauri 注入存在；最小转储 187,465 字节且宿主 PID 匹配。证据在 gitignored `qa-artifacts/aboutblank-diagnostics/real-exercise-2/`。
- 首轮独立端口复核超时记 unavailable，转储成功；调整诊断内部配额后第二轮快照成功。**没有改启动的 45/20/65 秒或诊断总计 8 秒上限**，首轮证据保留。
- 该本地验收 EXE SHA256 `ba71c8243b7a80e1027bb7d9f072ab48a122d5c69dd9c3a6eb63014863af82c8`，构建时 HEAD 为 `7fe4c7a`、工作树含本批未提交改动，记录为 local/assumed、tree_clean=false，不称干净提交实跑；CI 将在提交后的树重建。
- 上述实跑是正常场景的取证能力验证，**没有复现真实 about:blank，也不构成其根因修复证明**。完整提交钩子已通过（普通/acceptance 两套 workspace、前端 1104、E2E、Python/PowerShell 契约）；自审另补两条状态红测，防止把 helper 明确返回 unavailable 的转储包装成 captured。PR 最新 HEAD 的检查与根因状态以 PR 描述/检查记录为准。
