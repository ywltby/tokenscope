# 用户协议更新后的重新同意实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 协议更新后，新用户和已同意旧版的用户都须明确同意当前隐私政策与用户许可协议，才能进入业务界面。

**Architecture:** 沿用后端原子保存的同意闸门，把现有修订号作为两份文件的共同同意版本，本次升级为 3。界面明确说明点击同意覆盖两份可离线阅读的文件；历史修订 2 只用于旧版本，不预先授权新版。协议文件再次变化时，版本检查必须要求同意修订号递增。

**Tech Stack:** Rust/Tauri、Vue、Python、PowerShell、原生隔离验收。

---

### Task 1: 固定准入行为

**Files:** `crates/tokenscope-core/src/settings.rs`、`src-tauri/src/privacy.rs`、`frontend/src/components/PrivacyConsentDialog.vue` 与对应测试。

1. 测试修订 2 的旧同意保持闸门关闭；成功同意后原子保存当前修订并允许进入，拒绝或保存失败不解锁。
2. 测试同意界面明确说明隐私政策与用户许可协议，并保证两份文本可离线查看。
3. 提升共同同意修订号并更新用户可见文案与协议文字；复验前述测试。

### Task 2: 防止后续协议更新遗漏修订

**Files:** `scripts/bump_version.py`、`scripts/upgrade_baselines.py`、`scripts/test_bump_version.py`。

1. 用隔离 Git 仓库测试协议变更但共同同意修订号不变时，版本检查必须拒绝；递增时通过。
2. 基于真实基线协议和实际 Rust 常量接入检查，不改变已发布基线的历史修订号。

### Task 3: 更新升级与原生验收

**Files:** `scripts/prepare-native-acceptance.ps1`、`scripts/test_install.ps1`、`scripts/upgrade_test_helpers.ps1`、`scripts/test_upgrade_helpers.ps1`、`frontend/scripts/native-acceptance.mjs` 与对应测试。

1. 历史 0.6.0 继续使用修订 2；升级到本版后先观察闸门，再模拟同意修订 3、验证业务与重启。
2. 原生同意证据与终态检查使用当前修订，保留严格 TOML、类型与字节判定。
3. 执行隔离测试、前端及升级契约；条件允许时以最终构建运行原生场景并核对证据。

### Task 4: 收口

1. 更新计划记录与相关版本说明，保持 AC07 暂缓、N10 开放及版本 0.6.1。
2. 完成仓库要求的检查后提交并推送当前 PR；不合并 main。

---

## 执行记录（2026-10-28）

**实现（含 2026-10-28 复核修复）**：

- 共同同意修订号 `PRIVACY_POLICY_REVISION` 由 2 升为 **3**；`Settings` 新增
  `user_agreement_accepted` / `user_agreement_revision` 两个字段。
  `read_consent` 对隐私政策与协议两份记录都要求**精确等于当前修订**：
  低于当前（旧版）、高于当前（用户降级到旧版）、两份修订不一致、缺协议
  记录或修订行，一律 `StaleRevision`；`accept_privacy_policy` 在同一次
  原子保存中写四项。
- 设置模板与设置页说明补充两个协议字段的含义，并改为精确匹配口径。
- 前端同意弹窗：新增「点击『同意并继续』即表示您同意隐私政策和用户许可
  协议」说明；协议正文缺失或政策正文缺失时**禁用同意**并提示重新检查；
  协议可离线展开查看。`POLICY_UPDATED_DETAIL` 改为覆盖两份文件的通用提示。
- 政策文本（`docs/privacy.md`）与协议（`docs/terms.md`）同步：修订历史新增
  「共同同意修订 3」，协议明确「下载、安装或保留程序本身不视为同意；首次
  运行或更新后须在应用内明确同意两份文件」。README 同步该表述。
- 版本守卫：`scripts/bump_version.py --check` 在 `docs/terms.md` **或**
  `docs/privacy.md` 变化时，要求当前共同同意修订号高于基线；基线无常量时
  拒绝。`upgrade_baselines.extract_privacy_revision` / `privacy-revision`
  子命令成为 PowerShell、安装测试与原生验收共用的常量提取口径。
- 升级编排按**目标版本自身语义**装配与判断：
  - `Get-TargetConsentRevision`：当前版本读受测源码，历史格式 3 读登记表
    的 `privacy_revision`（0.6.0 = 2），格式 < 3 返回空。
  - `Grant-UpgradeConsent`：历史 0.6.0 只写它自己认识的隐私同意修订 2，
    不写协议字段；当前版本写受测源码修订 + 协议两项（同一次写入）。
  - `Test-ConsentGateApplies`：通过 `consent-state` 读取**实际落盘记录**，
    当前版本要求四项精确匹配；0.6.0 只要求隐私同意且修订不低于它自己的
    修订 2（它不认识协议字段）。删除硬编码 `ConsentRevision=2`，门禁
    判断不再依赖编排自己记的应然状态。
  - `upgrade_fixture.consent-state` 用 tomllib 严格类型读取记录，供门禁
    判断；`ensure-consent --agreement` 仅目标版本要求协议时使用。
  - 2026-10-29 复核：目标版本能力来自登记表/受测源码，不按「是否当前版本」
    或设置格式号推断。登记表为 0.6.1 起的版本记录 `agreement_revision`；
    `Get-TargetConsentExpectation` 据此装配：0.6.0 仍是隐私修订 2-only，
    0.6.1 历史目标缺协议记录时门禁关闭、授权写四项修订 3，从已同意 0.6.1
    到协议未变的后续版本不重复询问，协议修订变化则重新询问。
- 升级设置核对 `verify_settings`：修订号必须是 TOML 整数（浮点、布尔、
  字符串都不行）；旧隐私/协议修订行不得删除或下调，旧 accepted 不得丢失；
  其余用户设置继续逐项保留。
- 原生验收：`strictConsentState` 统一严格 UTF-8 解码 + smol-toml /
  toml-eslint-parser 双解析 + AST 顶层字面量类型/值，保存字节证据、业务后
  终态与重新同意三处共用；新增注释假字段、子表/点号键、`30` 前缀、浮点、
  非法 UTF-8 五类负例（真实实现抽取执行，非复制断言）。
- 2026-10-29 复核新增原生场景 `privacy-revision2-consent`：以隐私
  accepted=true、revision=2、无协议字段的真实旧用户记录启动，15 秒门禁
  观察（预置业务文件与 settings 字节不变、零业务追踪），再真实界面点击
  「同意并继续」，核验四项原子落盘、业务在同意后初始化、重启不再询问。
  CI 升级链的 `Grant-UpgradeConsent` 明确标注为**脚本模拟授权**，不冒充
  真实界面点击。
- 原生验收的 `CURRENT_CONSENT_REVISION`、`Get-ConsentRevision` 与安装测试
  均复用上述 Python 常量剥离器；块注释中的假常量有回归覆盖。

**验证（本地，本轮复核修复后）**：

- 核心库 303 项、壳 acceptance 70 项（1 ignored）、fmt/clippy 由钩子/CI
  复核；前端 885 项（含新增严格同意断言 6 项）、typecheck 全绿。提交钩子
  full 模式 17 项（含 workspace 普通/acceptance 两套测试、前端 build+e2e）
  通过，未跳过钩子。
- Python 脚本 355 项全绿；升级契约 99 项（新增按目标版本能力、真实 0.6.0
  中间跳、块注释假常量、settings 大小写字节反例等回归）；升级夹具 25 项；
  安装 helper 26 项；PowerShell 语法 12 个脚本通过。
- 45 秒门禁观察：settings 按**原始字节**逐字节比对并记录 SHA256，仅大小写
  变化也会被拒；证据新增 `snapshot_scope`，说明 logs/history 快照只是
  路径/大小/时间戳边界，不冒充逐字节未写入证明。原有运行证据与局限保留。
- 先红证据（保留在本地运行记录）：精确匹配修复前 2 项核心测试收到
  `Granted`；门禁适用性修复前「当前版本四项精确匹配」被判适用（实际应为
  不适用）；设置核对修复前浮点 3.0 与删除修订行均未抛错；原生正则修复前
  注释/子表/30 前缀/浮点误放行、非法字节触发 `text.split is not a function`。
- 此前本地原生三连的 privacy-preseeded **实际写入修订 1**（v2 布尔同意
  形态），不是修订 2；原记录已更正。修订 2 的真实旧用户由 0.6.0 单跳与
  连续链覆盖，不重写原三连证据。

**证据轮次与勘误（按 API/artifact 核实）**：

- 40673593494（起点提交 `8f04739`）：0.6.0 present 作业
  `122771980843`、deleted 作业 `122771980642`；升级结果 EXE
  `938420d1c5aa1b9ab645693269c112d5ec38c6aa3e863b532d865b9a2847a1be`。
  这是**上一轮**证据，不是最终轮。
- 40676126853（PR 头 `b8648d3`，产物记录 `current_commit`
  `b05536c…`）：0.6.0 present 作业 `122780516355`、deleted 作业
  `122780516352`；两份升级结果的 EXE 均为
  `e711b0b5ec0481ad84a274e968a075b52544346b322bb6e3733bcada29f0e081`。
  两轮 0.6.0 的旧设置都只有 `privacy_policy_accepted = true` 与
  `privacy_policy_revision = 2`，没有协议字段；45 秒闸门观察均为
  settings 不变、业务日志/历史库零新增。
- 此前把 40673593475 / 40673593484 与 40673593494 混作同一“最终轮”、并
  把 `938420d1…` 写成最终轮 EXE 不实；上述两轮分别标注。
- 本轮修复代码提交 `b3ea0a6`（最终 PR 头 `60f790a` 仅回填本文档）：
  - PR 必需检查 run 40694348796：0.6.0 present 作业 `122843466196`、deleted
    作业 `122843466000`，升级结果 EXE
    `0a9de532ca058c4c7821c8afaa1344f5b00e282691372d2e26082c07eb796d49`
    （产物记录 `current_commit` `525d2c4…`，GitHub 的 PR 合并引用）；18 项
    通过、`upgrade-chain` 按设计跳过、0 失败。CI run 40694348702 通过；
    Native UI E2E run 40694348785 首跑在 `ui-identity` 报
    `没有 tauri 页面 target（about:blank）`，同 run 失败作业重跑通过——失败
    证据保留，未用重跑替代归因，该启动超时与本批同意逻辑无直接关系。
  - 手动触发的隔离升级 run 40694379602（同一 PR 头 `b3ea0a6`）：两条连续链
    present 作业 `122843941301`、deleted 作业 `122843941430` 均通过；链
    `0.1.0 → 0.4.0 → 0.5.9 → 0.5.13 → 0.6.0 → 0.6.1 → restart` 逐跳与重启
    全绿，受测 EXE `b89cc501806bd37926cd9fdc7541c75ace41883116f512c36889a16da9338e13`。
    链内 `chain-04`（0.6.0 中间跳）与 `chain-05`（当前版本）各保留 45 秒
    闸门证据：settings 不变、修订未写入、业务日志 0、历史库 0；0.6.0 授权
    后当前版本仍需重新同意，当前版本授权后重启复核通过。同一 run 的单跳
    矩阵也全部通过。
  - 最终 HEAD `60f790a`（仅文档）：CI 40698748262、Install test
    40698748313（单跳全过，`upgrade-chain` 按设计跳过）、Native UI E2E
    40698748680 均通过；代码与 `b3ea0a6` 相同，两条链证据仍取
    40694379602。
  - 2026-10-29 二轮复核 HEAD `0bbdeed`：PR 必需检查 run 40716869841
    （CI）、40716869894（Install test，0.6.0 present 作业
    `122922582940`、deleted `122922582948`，EXE
    `462b4e0dfc3db98cce5e3284564a4999142de7eb0103c850c768796efced311b`）、
    40716869939（Native UI E2E，新增 `privacy-revision2-consent`
    场景断言全 true：修订 2 旧用户、门禁观察零读写、真实点击后四项修订 3、
    重启不询问）18 项通过、`upgrade-chain` 跳过。手动链 run 40716879803：
    present 作业 `122924293784`、deleted `122924293455`，两条链全过，
    EXE `a2debaa95968fc7058ef40ad507b608870c67fc97615d1ecbdb92734b5326147`；
    chain-04/05 门禁证据改为 `settings_bytes_unchanged` + SHA256 +
    `snapshot_scope`（字节比对，logs/history 仅路径/大小/时间戳边界）。
    同一 run 的 0.6.0 单跳作业 `122924424407` / `122924423861` 也通过。

**未解决/边界**：AC07 暂缓；N10 开放；真实自启出现/移除仍待 D5 3.4 人工
签字；WorkBuddy 双版本与历史 CI 偶发失败根因不受本批影响。
