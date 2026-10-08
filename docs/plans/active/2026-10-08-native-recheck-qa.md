# 原生生产验收记录（RC10 入口 / RC11 取证）

本文件记录 **release + `acceptance` feature** 构建下、跑在隔离根里的原生
WebView2 验收。它只证明"这套配置下的原生契约"，不证明安装包、不证明真实
1.2 GB 数据的性能，也不代替浏览器自动化结论。

- 关联计划：[复核遗留缺陷与验收补齐](2026-10-08-recheck-remediation.md) RC10 / RC11
- 状态口径：**已验证 / 待验 / 环境缺失**三态，缺证据就写待验，不写"全部通过"
- 产物目录：`qa-artifacts/native-recheck-2026-10-08/`（gitignore，本机）

## 0. 复现步骤（本次唯一使用的命令序列）

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
$acceptanceRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('tokenscope-native-' + [guid]::NewGuid().ToString('N'))
.\scripts\prepare-native-acceptance.ps1 -Root $acceptanceRoot
$env:TOKENSCOPE_ACCEPTANCE_ROOT = $acceptanceRoot
.\frontend\node_modules\.bin\tauri build --features acceptance --no-bundle
.\src-tauri\target\release\tokenscope.exe     # 记录 PID；不调用 tauri dev
# 结束后：Remove-Item Env:TOKENSCOPE_ACCEPTANCE_ROOT；只关闭本次记录的 PID
```

每轮必须填写：commit、feature、隔离根绝对路径、`manifest.json` 内文件数与
SHA256 起点、Windows 版本、WebView2 版本、系统 DPI、窗口逻辑/物理尺寸、
主题偏好、产物路径。**任一栏缺失即写"未记录"**，不得事后补写推测值。

| 轮次 | commit | feature | 隔离根 | Windows / WebView2 | DPI | 窗口（逻辑/物理） | 主题偏好 | 结果 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| R1 | 待填 | acceptance | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |

## 1. 入口与隔离（RC10）

| 项 | 断言 | 状态 | 证据 |
| --- | --- | --- | --- |
| `native_acceptance_paths_are_hermetic` | 数据目录、cache.db、settings.toml、双源快照、pricing-index、view-cache、日志、两个来源根全部落在隔离根内；真实 `~/.tokenscope` 前后指纹完全一致 | 已验证（子进程） | `cargo test --offline --features acceptance --test native_acceptance` → 3 passed |
| `acceptance_mode_never_falls_back_to_real_sources` | 未显式指定来源目录时用隔离根下默认根；目录缺失只报 `missing`；空隔离根采集 requests=0 | 已验证（子进程） | 同上 |
| `acceptance_without_root_fails_before_collection` | 缺失 / 空 / 相对 / 不存在 / 等于真实数据目录的验收根一律在采集前拒绝，并点名 `TOKENSCOPE_ACCEPTANCE_ROOT` | 已验证（子进程，退出码 3） | 同上 + 壳侧 `start_rejects_missing_relative_and_missing_dir` |
| 普通构建不受影响 | 无 feature 时不读该环境变量，路径行为与验收前一致 | 已验证（编译期分支 + 默认门禁全绿） | `cargo test --workspace --offline`、`cargo test --manifest-path src-tauri/Cargo.toml --offline` |
| 准备脚本产物可解析 | 合成 Claude 日志 files=2 events=4 bad_lines=1（含一条故意畸形的行）；Codex files=1 events=2 bad_lines=0；`totals.requests=6`、`totals.cost_usd=0.0358`、`unknown_pricing=true`（mystery-model 保持未知） | 已验证（临时探针，已删除；数值为 2026-10-08 本机实测） | 准备脚本 + 上表探针；数值待 RC11 正式轮次在启动日志中复核 |
| acceptance 构建仍用生产前端与 CSP | release 构建 `frontendDist=../dist`、`csp` 生效；不使用 `devUrl`/`devCsp`（仅 dev 用） | 部分待验 | 见 §4 生产 CSP；两构建的 `dist` 产物一致性 |

## 2. 原生首帧与背景（RC11 / 原 UX09）

必须用连续帧或录像在**首个窗口画面**取证，并核对实际主题与背景色；
常规"加载完成"截图不算证据。

| 场景 | 期望 | 状态 | 产物 |
| --- | --- | --- | --- |
| 深色偏好冷启动首帧 | 第一帧即深色画布，无先浅后深闪烁 | 待验 | 待填（帧序列目录 + 首帧时间戳） |
| 浅色偏好冷启动首帧 | 第一帧即浅色 | 待验 | 待填 |
| 跟随系统（系统深色） | 第一帧深色 | 待验 | 待填 |
| WebView profile 隔离生效 | 首次启动无真实 localStorage 残留（`tokenscope-theme` 缺失时回落系统偏好） | 待验 | 待填 |

浏览器侧的预绘制取证已通过（`prepaint_theme_has_correct_canvas`，21 条断言，
含 storage 不可用场景），但它是 Chromium + 合成 IPC，**不等于**原生首帧。

## 3. 六组主题 × DPI 的键盘与布局

Windows 实际缩放（100% / 125% / 150%）× 浅色 / 深色，共 6 组：真实组件键盘、
可访问名称、费用浮层内容与四边、日期外壳、长路径、图表、通知/重试可操作性、
导航滚动。

| 组合 | 键盘/可访问名称 | 浮层四边 | 日期外壳 | 长路径 | 图表 | 通知/重试 | 导航滚动 | 状态 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 浅色 100% | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |
| 浅色 125% | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |
| 浅色 150% | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |
| 深色 100% | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |
| 深色 125% | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |
| 深色 150% | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待填 | 待验 |

无障碍工具输出被截断时只记录"工具输出截断"，不得写成 WebView2 的固定上限。
费用触发器 button 与同名表格行分开计数。

## 4. 生产 CSP（原生）

`native_production_csp_enforced`：确认页面实际加载的策略，正常脚本 / IPC /
Naive / ECharts / 主题 / 日期均可用；在页面**实际文档**中插入无害 script 哨兵，
验证内联与外源脚本被禁止执行，记录 `securitypolicyviolation` 或原生日志。
DevTools/调试器的脚本求值不算证据；不得为了让测试通过而放宽策略。
无 CSP 的图表安全测试（`check-chart-tooltip-security.mjs`）是另一条独立证据，
两者都要通过。

| 项 | 状态 | 证据 |
| --- | --- | --- |
| 实际生效的 CSP 文本 | 待填 | 待填 |
| 内联脚本哨兵被拒 | 待验 | 待填（violation 记录） |
| 外源脚本哨兵被拒 | 待验 | 待填 |
| IPC/Naive/ECharts 在 CSP 下正常 | 待验 | 待填 |

## 5. 故障恢复（原生，仅作用于隔离根）

| 场景 | 期望 | 状态 |
| --- | --- | --- |
| 临时 `logs` 路径写成普通文件 | 窗口仍出现，且有启动降级通知（SF06） | 待验 |
| 临时 `pricing.toml` 被共享锁占用 | 显示降级提示；释放后恢复读取 | 待验 |
| 合成日志/价格变化 | 旧 query 稳定不变，新 query 反映变化（SF04） | 待验 |
| 会话过期 | 错误条可见，点其按钮恢复（RC02） | 待验 |
| 补充源同步失败 | 错误条有可点击的重试同步（RC04） | 待验 |

正常路径的日志初始化/价格加载耗时**不能**代替以上故障验收。

## 6. 关窗三态最小回归

`native_close_cancel_preserves_settings`：显示弹窗 → Escape → 弹窗消失且进程
存活；取消后临时 `settings.toml` 字节不变（前后各存一份截图与文件散列）。
记忆/最小化/退出只在隔离实例验证，截图必须对应动作之后。

| 动作 | 弹窗 | 进程存活 | settings 字节 | 状态 |
| --- | --- | --- | --- | --- |
| Escape 取消 | 待填 | 待填 | 待填（前后 SHA256） | 待验 |

## 7. 明确后延（不计入本轮完成）

- D5 安装 / 升级 / 卸载与自启注册（用户此前决定后延，本计划不自动执行）；
- 真实 1.2 GB 数据的性能与缓存预热（`#[ignore]` 测试，需显式环境变量）；
- 安装包签名与发布。

## 8. 结论口径

只有本文件 §1–§6 全部落到"已验证 + 产物路径"后，才允许把 RC11 标为完成；
任一项缺环境或缺证据，就在计划 §5 执行账里保持"待验 + 缺失证据"，
整体状态不得写成完成。
