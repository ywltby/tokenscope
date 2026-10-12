# 首次启动隐私同意：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-09-first-launch-privacy-consent.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| P01 | 同意字段严格读取与原子保存已实现 | — |
| P02 | 统一业务 command guard 已实现；当前 27 个而非旧 23 个 | — |
| P03 | 全部启动业务副作用延后至 Ready 已接线 | — |
| P04 | 首屏隔离、动态业务加载、本地政策已实现 | — |
| P05 | 拒绝与各退出路径已实现 | — |
| P06 | 文档/局部自动化已有；强原生取证与当前完整矩阵未完成 | [N10](../active/2026-10-11-consolidated-remaining-work.md#n10)、[N12](../active/2026-10-11-consolidated-remaining-work.md#n12) |

## 原审计更正

- 原审计 P03–P05 名称/职责错配，本表按原计划恢复；“P01–P06 全部验收完成”过宽。
- 新增 CCS 四命令后当前 guard 数量为 27；23 只描述先前版本。
- 现有 native 工具只观察 1500ms、businessFiles 漏 history.db/WAL/SHM；缺业务读取/后端网络计数与保存先于 I/O 的顺序证据。
- 浏览器 denied IPC 场景不是真实调用后端；当前局部产物为 1 场景/3 契约/41 断言，required 列表不能证明完整矩阵执行。
- 这些是验收工具/覆盖缺口，不声称已经证明生产隐私闸门失效。

## 核对证据与边界

- `src-tauri/src/privacy.rs`、`commands.rs`、`lib.rs`；App/MainApp 与 `theme-boot.js`。
- `frontend/scripts/native-acceptance.mjs` / `interaction-contracts.mjs`，当前隐私 measurements。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
