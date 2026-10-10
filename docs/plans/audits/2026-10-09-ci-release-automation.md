# CI 与发布自动化：实现事实复核

> 2026-10-11 复核；[原计划](../archive/implemented/2026-10-09-ci-release-automation.md) 已按“已实现”归档。唯一剩余任务入口：[整合计划](../active/2026-10-11-consolidated-remaining-work.md)。

**结论：** 已实现。按下表区分实现、验证和待验；不用函数/测试/提交存在推导全部通过。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 方案 1–6 的工程交付 | workflow/actions、metadata、双 manifest 门禁和发布链已实现 | — |
| 真实 PR / main CI 与 main 预发布 | 远程成功运行及三附件有直接证据 | — |
| 当前版本正式 v* tag、安装与签名消费 | 发布批次待验；不属于造一个 tag 才算有实现 | [N12](../active/2026-10-11-consolidated-remaining-work.md#n12) |

## 原审计更正

- 原文未回写执行状态属实；但不能只靠文件/修复分支存在断言链路成功，本次补实际远程成功记录。
- main 使用 `pre-完整SHA`，正式 tag 从触发 ref 取值并要求与四个版本文件一致；不是写死 0.1.0，也不把 main 预发布改成重复版本 tag。
- 没有当前 add8319 的远程 run，也没有正式 v* 发布记录；附件存在不等于已下载、验签或安装。

## 核对证据与边界

- `.github/workflows/ci.yml`、`release.yml`、`.github/actions/{setup,checks}`、`scripts/release_metadata.py`。
- [PR #1](https://github.com/ywltby/tokenscope/pull/1) 与 [PR CI 成功](https://github.com/ywltby/tokenscope/actions/runs/37845997403)。
- main `8d1d90de29aa00d10b9a7e4b8f906d909726446b`：[CI](https://github.com/ywltby/tokenscope/actions/runs/37958892542)、[Release](https://github.com/ywltby/tokenscope/actions/runs/37958892567)。
- [实际预发布](https://github.com/ywltby/tokenscope/releases/tag/pre-8d1d90de29aa00d10b9a7e4b8f906d909726446b) 含 exe、SHA256SUMS.txt、provenance.sigstore.json。

只读查询公开 GitHub API 核对成功 run/附件；metadata 的 7 个 Python 测试本轮通过。未触发发布、创建 tag、下载/安装 exe 或独立验签。

核对基线为提交 `add8319` 与 2026-10-11 正在修改的工作区；未提交修复仅按已经观察到的子项记账。当前版本交付状态、实际门禁失败与独立探针反例统一见新计划执行账。
