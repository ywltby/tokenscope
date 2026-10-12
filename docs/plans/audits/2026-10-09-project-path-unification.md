# 跨工具项目路径统一：实现事实复核

> 2026-10-11 复核；[原计划](../archive/partial/2026-10-09-project-path-unification.md) 已按“部分完成”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 部分完成。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| A01 纯路径 / A02 Claude / A03 映射 / A04 Codex / A05 聚合下钻 | 实现与后续边界修复已完成 | — |
| A06 文档/验收 | 自动化已有，当前原生链尚待验 | [N11](../active/2026-10-11-consolidated-remaining-work.md#n11) |
| B01 语义证据与根规则 / B02 两个 cwd 字段 | 已完成 | — |
| B03 两侧项目根/切换联动 | 实现和自动化完成，原生链待验 | [N11](../active/2026-10-11-consolidated-remaining-work.md#n11) |

## 原审计更正

- 原审计 A/B 编号与职责明显错配，B01 `419a965` 为核验/决策文档，A05 `1ffee42` 不是“parser v10 升级”。
- `72541d6` 是 parser 7→8、view 6→7，不是 view 10。parser/index/view 是独立版本轴，不将后续版本改回旧计划。
- 点段被拒绝，未做点段折叠；只规范 Windows 盘符与 UNC 主机大小写，不统一所有路径分量大小写。
- 旧 sidechain 排除及日志事实库删记录规则，被最新全部代理/历史保留要求替代；不恢复主子关系产品。

## 核对证据与边界

- `src/source/project_path.rs`、`src/source/{claude,codex}.rs`、`src/source/claude_projects.rs`、aggregate/report 与项目测试。
- `docs/research/2026-10-09-session-cwd-semantics.md`、A/B 原执行表及当前边界回归。

静态核对当前实现、历史提交及实际测试内容；未把测试存在视作本次通过，也未执行本计划的安装/系统验收。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
