# v0.9.10：任意精度汇总、双源展示名、稳定滚动锚点与图表长标签

- 分支：`release/v0.9.10`，基线 `origin/main` `33b8686`（v0.9.9 合并提交）。
- 状态：**已合并发布**。PR [#83](https://github.com/ywltby/tokenscope/pull/83) 以 `--match-head-commit` 绑定复核 HEAD `53c47c0e87d4273c1458c279e7cef1b2d8c563fa`，merge commit `79ba722733468926109c0d651400a343fcce845b`。
- 发行版本：四处产品输入改动，按仓库工具取 patch **0.9.10**（`previous=0.9.9`、`release: true`）；视图快照 25 → 26。
- 升级基线：`v0.9.9` 已随发版 PR 登记（资产逐个下载核对哈希），`select --current-version 0.9.10` 起点与链均含 0.9.9；`v0.9.10` 在本记录 PR 登记。

## 1. 范围与不变量

本批把此前四处未提交改动合为一次 patch 发版，均不改变请求身份、计价口径与 CCS 覆盖：

1. **大整数汇总**：`crates/tokenscope-core/src/bigint.rs` 新增零依赖 `BigUint`（$10^{18}$ 小端分片，`Small(u64)` 快路径）与 `BigTokenCounts`。`Group.tokens` / `unknown_tokens`、`RollupCoverage` 的覆盖 token、`WriteSummary.inserted_tokens`、`HistoryDb::totals`、CCS 导入净增与请求费用明细改为任意精度累计；单条 `UsageEvent` 仍是 `u64` 受检校验，请求数仍 `checked_add`、金额仍 finite 检查。IPC：`≤ 2^53−1` 仍是 JSON 整数，超过后为十进制字符串，前端以原生 `BigInt` 无损解析。
2. **双源展示名**：`model_display.rs` 在随包公开目录（models.json）与 OpenRouter 同步快照间补名；无可用名称与来源不可用分别记 `no_identity_evidence` / `source_unavailable`。严格区分显式 canonical 与隐式同末段自 ID（见 §3 修复）。名称不进入 `Group.key`、`model_stats_key`、`canonical_identity_key`、下钻指纹、请求原始 ID、CCS 覆盖或计价。
3. **刷新滚动锚点**：`EventRow.row_id` 为跨查询稳定的明细行身份（原生 `event:<id>`、CCS 日汇总四元组），与会话分页游标解耦；快照 v25 → v26 忽略旧首帧、按筛选偏好与历史库原样重建。
4. **图表长标签与三维度布局**：非时间维度（模型 / 应用 / 项目）共用逐行自适应布局，按当前可见类别逐一测量轴标签并以最大宽度预留 `grid.left`，右侧截断加 `…`、tooltip 给全名；按日维度保持原布局。`frontend/src/lib/chartLabelLayout.ts`、`formatTokens.ts`、`eventRowKey.ts` 为新增纯函数库。

## 2. 定向回归（本次实跑）

- Rust 单测：`bigint::tests` 5 例、`token_overflow_contract` 8 例（含 u64 以上聚合、CCS 同桶下钻、单请求费用明细 > 2^53−1）。
- Rust 模型命名：`model_display_name_catalog` 22 例、`model_identity_contract` 等三组共 57 例。
- 前端：13 个相关文件 312 例（含 `TrendChart` 61、`SummaryCards` 23、`EventTable` 33、`useUsageQuery` 53、`Dashboard` 71）。
- 独立代码复核四个领域未发现正确性缺陷。

## 3. 复核补修：原生 ui-identity 真实回归（红 → 绿）

PR #83 首轮必需检查中 `native-ui` 失败：原生场景 `model_variants_merge_with_friendly_name` 断言发布方名 `Claude Opus 5.5` 可见、夹具渠道名 `Synthetic Opus 5.5` 不可见，实测聚合标签为原始 ID `Claude_Opus_5.5`。

取证（`evidence.json`，run 46032686505）：`reason: ConflictingSources`，models.dev 与 OpenRouter 双路均 `AmbiguousOrConflict`。根因：随包公开目录已确认 `anthropic/claude-opus-5-5` 的发布方身份，而 OpenRouter 快照只有 `synthetic/claude-opus-5.5` 同末段**隐式自 ID**（无 `canonical_model_id`），被误判为跨源身份冲突。

- 红测试 `embedded_catalog_name_wins_over_implicit_openrouter_same_leaf`（DN-C18）先复现 `ConflictingSources` / name=None。
- 修复：`OrBareMatch::Unique` 携带 `explicit` 标志；隐式自 ID 不是身份证据，随包目录已确认发布方身份时按其取名且不判冲突；models.dev 侧同为快照自 ID、或 OpenRouter 有显式 canonical 指向不同目标时仍保留真实冲突（现有 `split-target` / `ambig-m` 两组回归保持通过）。
- 修复提交 `53c47c0` 的正常所选钩子通过；PR #83 最终 HEAD 的 `ui-identity` 22/22、`ui-refresh` 17/17、`ui-system` 26/26 与其它场景全部通过。

## 4. 门禁与合并

- 主代码批次正常完整钩子（full 模式）通过：两模式 workspace clippy `-D warnings`、默认与 acceptance 全量 Rust 测试、前端 1286 单测 / typecheck / format / build / 浏览器契约、脚本 427 项与自测套件、文档检查；无 `--no-verify`、无跳项。
- PR #83 必需检查（最终 HEAD `53c47c0`）：`rust`、`rust-acceptance`、`contracts`、`frontend`、`gate`、`install-test`、`native-ui` 与 12 组单跳升级全绿；`upgrade-chain` 按无连续升级链需求跳过。
- 期间两次首轮失败如实取证：① `native-ui` 的产品回归见 §3（已按缺陷修复，未以重跑代替原因）；② 重跑后 `frontend` 的 `native-ac06-contracts.test.mjs` 夹具用例 5s 超时（无断言失败）与 `ui-collection-status` 的 `.app-nav` 启动超时——两者同代码在上一轮/本地均通过，属时序抖动，保留观察、未降低阈值。
- 合并前复核：HEAD 无漂移（本地、`origin/release/v0.9.10`、PR head 均为 `53c47c0`）、`mergeable=MERGEABLE`、`mergeStateStatus=CLEAN`、版本契约 `0.9.9 → 0.9.10 / patch / release: true`，以 `--match-head-commit` 合并为 `79ba722`。

## 5. 发布与核验

- [Release 46047024474](https://github.com/ywltby/tokenscope/actions/runs/46047024474) 19 作业全成功无跳过：`gate`、`plan-upgrades`、`build`、`install-test`、12 组单跳（含 0.9.9 present/deleted）、present/deleted 两条连续升级链、`publish`。[同提交 CI 46047024242](https://github.com/ywltby/tokenscope/actions/runs/46047024242) 五作业全绿；[Native UI E2E 46047024240](https://github.com/ywltby/tokenscope/actions/runs/46047024240) 通过。
- [v0.9.10](https://github.com/ywltby/tokenscope/releases/tag/v0.9.10) 公开预发布已发布，标签解引用与 `build-metadata.json` 的 `commit` 均指向 `79ba722`、`version=0.9.10`。
- 持久格式指纹由合并提交源码 `print_persistent_formats` 实测：`cache 12 / history 5 / parser 3 / identity 1 / pricing index 11 / settings 3`，与 `build-metadata.json` 及 v0.9.9 一致。
- 四项资产齐全；下载后重算：`TokenScope_v0.9.10_windows-x64-setup.exe` SHA256 `a8a44824a263ef5ce7b83f9f7f095910328911b37ccb556467ed2f501232692f`，与 `SHA256SUMS.txt` 及 GitHub 资产 digest 三方一致；`build-metadata.json` `93e393ab…e07a8be0`、`provenance.sigstore.json` `03a52e34…90da92`、`SHA256SUMS.txt` `50143481…37758` 均与 GitHub digest 一致。
- 哈希核对与签名验证分别报告：in-toto DSSE 载荷声明的 commit / 版本 / 安装包摘要 / 仓库均正确，但本机未安装 `cosign` 且未授权本机安装，**未做密码学签名验证**。
- 本机未安装、未触碰真实 `history.db`、CCS 数据或用户缓存；安装与升级验证全部在 GitHub-hosted Runner 隔离环境完成。
