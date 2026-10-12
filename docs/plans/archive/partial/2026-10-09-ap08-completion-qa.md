# AP07 / AP08 补齐验收记录

> **2026-10-11 归档复核：部分完成。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 既有六类原生场景 / 完整浏览器矩阵 | 有直接成功产物，失败尝试与环境限制保留 | — |
| 125% / 跨显示器、系统深色首帧 / D5 | 未全部执行，最新隐私与功能批次另待验 | [N10](../../active/2026-10-11-consolidated-remaining-work.md#n10)、[N11](../../active/2026-10-11-consolidated-remaining-work.md#n11)、[N12](../../active/2026-10-11-consolidated-remaining-work.md#n12) |

逐份事实核对与原审计更正见 [复核报告](../../audits/2026-10-09-ap08-completion-qa.md)。

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

关联：[全计划终态复核与遗留修复](2026-10-09-all-plans-final-recheck.md)。本轮核对基线 `9db0bbb`，补齐退出保存与验收入口；AP01–AP06 没有新增实现修改。以下原生结果来自 Windows 10.0.19045.6466、WebView2 154.0.4258.62、release + `acceptance` 构建和独立临时数据根，不能代替安装包或真实日志性能验收。

## 1. 实现补齐

- AP07：`9222aff`。关闭弹窗和托盘的直接退出原来绕过最终保存，现在与已记忆退出共用后台保存后退出的入口。定时保存和最终保存串行，先取得保存锁再取快照；保存期间的新事件不再被清脏覆盖，写入失败保留待重试标记。
- AP08：`6332f4f`。仅验收构建提供同源 CSP 哨兵资源，真实按钮点击后插入同源、未授权内联及外源脚本。没有添加 IPC command，没有放宽生产 CSP。
- AP07 原生失败恢复：仅验收构建可用的一次性隐藏故障，需显式设置 `TOKENSCOPE_ACCEPTANCE_HIDE_FAILURE_ONCE=1`；失败后的重试调用真实窗口隐藏 API。
- 驱动器：显式传递 WebView 调试参数，默认不再强制设备缩放；新增同进程完整 TTL 场景，专用 750 秒超时，结束时清理超时定时器；分页恢复同时断言实际加载行数增加及后端会话身份改变。

## 2. 原生实测

产物（本机 gitignore）：`qa-artifacts/ap08-completion-2026-10-09/`。每轮记录二进制 SHA-256、PID、临时根、WebView 版本、断言、截图及真实应用数据目录前后指纹。

| 场景 | 结果 | 证据范围 |
| --- | --- | --- |
| `csp` | 14/14 通过 | 同源正向对照执行；内联未执行；DOM 记录 2 个 `script-src-elem` 拒绝事件（inline / 外源）；真实 IPC、图表、日期浮层、主题切换可用 |
| `close-final-save` | 4/4 通过 | 弹窗打开后等待定时保存完成，仅删除隔离根的窗口状态派生文件；直接退出后文件重新生成且状态一致，进程 exit 0 |
| `close-remember-failure` | 6/6 通过 | 已记忆最小化触发一次真实壳侧故障，界面显示 `acceptance-hide-once`；点击重试后主窗口隐藏，进程存活、设置字节不变 |
| `combo`，不传 `--scale` | 18/18 通过 | `GetDpiForWindow=144` 与 WebView dpr=1.5 一致；真实键盘费用浮层、图表切换、日期外壳、长路径与窄窗布局通过。仅证明当前显示器 150%，不证明跨显示器切换 |
| `expire-cycle` | 12/12 通过 | `qa-artifacts/ap08-completion-2026-10-09-final/expire-cycle/`：真实空闲 630.684 秒，错误文案含 `query_expired`；重试从 `qe37a5dd736d5a828b739115557543e61-0-g0` 切到 `…-1-g1`，下一页仍用新会话，明细实际从 200 加载到 242 条 |
| `expire-cycle` 补录错误区域 | 12/12 通过 | `qa-artifacts/ap08-completion-2026-10-09-visible/expire-cycle/`：再次真实空闲 630.460 秒，`q4724d4101c10f5f57bde7c894ad3a9f8-0-g0` → `…-1-g1`，200→242 行；`expire-error.png` 已包含过期原因和重试按钮 |

六轮均 `real_untouched=true`。没有操作真实来源日志、重建真实缓存或修改系统 DPI/主题。验收结束后，本次原生实例及预览服务均已关闭。

验收窗口中的“1 行解析失败（已跳过）”是预期 fixture：`scripts/prepare-native-acceptance.ps1:158` 在临时 `broken-*.jsonl` 中写入缺失右花括号的 JSON，验证坏行跳过与部分数据有效的提示；不代表真实用户日志新增解析错误。

第一轮 TTL 的 `expire-error.png` 位于页面底部，没有包含页面上方的错误条；错误文案、真实重试点击及新会话/分页由 JSON 与应用日志证明，不把该截图当作错误可见的证据。驱动补充截图前滚动到错误条后，第二轮已取得包含错误区域的截图并完成同样的恢复断言。该截图由验收驱动滚动后取得，不代表产品会在分页失败时自动滚动。

### CSP 噪声口径

本轮 CSP 场景包含页面重载，共采集 8 条控制台/CDP 记录：4 条预期脚本拒绝、4 条已知 data-URI 图片噪声、0 条未知违规。DOM 哨兵实际记录的脚本违规是 **2 个事件**，不能把两个采集通道的记录算成 4 个独立脚本。其他已完成原生场景各记录 2 条图片噪声。

噪声来源仍为 `naive-ui` 2.45.3 的 `es/tree/src/utils.mjs` 顶层 `new Image()` 和 data-URI 预热赋值；内层 `emptyImage` 遮蔽外层同名常量。此次已定位依赖源码，但未修改依赖、未放宽 `img-src`，也没有把历史“固定 2 条”写成跨重载通用计数。

### 未通过的尝试

早期启动两次没有调试端口；显式传递 WebView 环境选项后恢复。一次直接 `cargo build --release` 产物未启用 Tauri 自定义协议，页面指向开发地址；之后均使用 `tauri build --features acceptance --no-bundle`。

分段 `expire-start` 后进程消失，未确认终止者。第一次连续 `expire-cycle` 被驱动器原有 240 秒总超时主动终止，失败证据保留在第一产物目录，不能算过期通过。随后采用最终连续场景及独立产物目录，真实等待仍为 630 秒，没有缩短后端 TTL。

## 3. 自动化与构建

- 根库 fmt/clippy、178 单元及全部常规集成测试通过；真实数据性能测试保持 ignored。
- 壳普通构建 28 测试、acceptance 构建 33 测试通过；两种 feature 的 clippy 通过。
- 前端 typecheck、format:check、24 文件 293 测试及 build 通过。
- 浏览器矩阵重新执行：Chromium 148.0.7778.96，60 场景、17 条必需契约、243 条契约断言，零失败。产物 `qa-artifacts/ap08-completion-2026-10-09/browser/measurements.json`。
- 新测试 `quit_waits_for_final_window_save`、`csp_probe_is_self_hosted_without_relaxing_policy`、`hide_failure_is_opt_in_and_consumed_once` 均实际执行先失败再通过；退出测试用挂起保存证明未提前退出。CSP 另测外站响应不被修改。
- 普通及 acceptance 发布构建均成功。普通二进制中不含 CSP 资源名、哨兵按钮名和单次隐藏故障环境变量；验收二进制包含，作为对照。

| 构建 | SHA-256 |
| --- | --- |
| 普通 release | `15fa6442af1cebade4fc9bb620d6e3c6c61131dd0009627bd98cd3bad4fb6e1c` |
| 最终 acceptance release | `085741802fb8238da2c2c879b09c0999efda7e122b45e8ac5fa95a9bee152b98` |

## 4. 剩余边界与复跑

Windows 125% 与跨显示器切换、系统深色应用主题下的“跟随系统”首帧仍待验。本机 `AppsUseLightTheme=1`，`SystemUsesLightTheme=0`；任务栏深色不等于应用首选深色，本轮没有修改它们。D5 安装/升级/卸载、自启注册、休眠及隐私时序仍按原计划后延，见 [D5 清单](../../d5-acceptance-checklist.md)。

```powershell
.\frontend\node_modules\.bin\tauri build --features acceptance --no-bundle
node frontend/scripts/native-acceptance.mjs --scenario csp --out qa-artifacts/ap08-rerun
node frontend/scripts/native-acceptance.mjs --scenario close-final-save --out qa-artifacts/ap08-rerun
node frontend/scripts/native-acceptance.mjs --scenario close-remember-failure --out qa-artifacts/ap08-rerun
node frontend/scripts/native-acceptance.mjs --scenario combo --theme system --out qa-artifacts/ap08-rerun
node frontend/scripts/native-acceptance.mjs --scenario expire-cycle --events 240 --out qa-artifacts/ap08-rerun
```

原生场景顺序执行，不并发运行同一验收 identifier。完整 TTL 场景会持续十分钟以上，其 750 秒超时是保护上限，不是用等待代替结果断言。
