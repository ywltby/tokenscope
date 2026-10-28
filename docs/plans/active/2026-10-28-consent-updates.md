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

**实现（已按最终方案完成）**：

- 共同同意修订号 `PRIVACY_POLICY_REVISION` 由 2 升为 **3**；`Settings` 新增
  `user_agreement_accepted` / `user_agreement_revision` 两个字段。
  `read_consent` 在隐私政策检查之外要求**协议同意记录存在、为 true 且修订
  不低于当前**——仅有隐私政策同意的旧记录（含修订号更高的形态）一律
  `StaleRevision`；`accept_privacy_policy` 在同一次原子保存中写四项。
- 设置模板与设置页说明补充两个协议字段的含义。
- 前端同意弹窗：新增「点击『同意并继续』即表示您同意隐私政策和用户许可
  协议」说明；协议正文缺失或政策正文缺失时**禁用同意**并提示重新检查；
  协议可离线展开查看。`POLICY_UPDATED_DETAIL` 改为覆盖两份文件的通用提示。
- 政策文本（`docs/privacy.md`）与协议（`docs/terms.md`）同步：修订历史新增
  「共同同意修订 3」，协议明确「下载、安装或保留程序本身不视为同意；首次
  运行或更新后须在应用内明确同意两份文件」。README 同步该表述。
- 版本守卫：`scripts/bump_version.py --check` 在 `docs/terms.md` **或**
  `docs/privacy.md` 变化时，要求当前共同同意修订号高于基线；基线无常量时
  拒绝。`upgrade_baselines.extract_privacy_revision` 供两处共用。
- 升级编排：`Grant-UpgradeConsent` 与 `Test-ConsentGateApplies` 改为从受测
  源码读取当前修订；`upgrade_fixture._consent_prefix` 只为历史基线写它
  自己认识的隐私政策同意（0.6.0 只有修订 2、没有协议字段），当前版本模拟
  重新同意时由 `ensure-consent --agreement` 同一次写四项。升级后设置核对
  把同意四项当作受测迁移单独验证（当前隐私修订、协议同意存在且同修订、
  不下调），其余用户设置仍逐项保留。**0.6.0 历史基线登记保持 revision 2，
  不改历史**。
- 原生验收：`CURRENT_CONSENT_REVISION` 从受测源码提取；终态与同意字节证据
  均校验四项（strict TOML + 字面量类型）；preseeded 构造「旧修订 2 + 无协议
  记录」的真实旧用户形态；accept-restart 显式结束上一实例后重启。

**验证（本地）**：

- 核心库 300 项、壳 acceptance 70 项（含壳层四项落盘断言）、clippy 双模式、
  fmt 全绿。
- 前端 879 项、typecheck、prettier 全绿；同意弹窗 4 项（含两份文件说明、
  协议缺失禁用、硬换行）。
- 升级契约 80 项（含 0.6.0→0.6.1 旧同意失效→观察→授权顺序）；升级脚本
  34 项；bump_version 35 项（含协议变更必须递增的共同同意守卫）。升级夹具
  22 项新增同意迁移用例：旧修订 2 无协议记录 → 当前四项通过，缺协议 /
  协议 false / 协议修订错 / 隐私修订下调 / 其他用户设置改动逐项拒绝。
- PR CI 40670128332 的 0.6.0 单跳红证据（`旧设置未保留：
  privacy_policy_revision`）暴露了「把同意迁移当旧设置损坏」的校对错误；
  修复后本地演练 0.6.0（`prepare --consent-revision 2`）→ `ensure-consent
  --revision 3 --agreement` → `settings --consent-revision 3` 通过，
  0.5.13 布尔同意路径同样通过。
- 原生隔离场景（受测 EXE `97c5cfec…7dab0`，提交 `0313bc6` + 本次改动）：
  preseeded（旧修订 2 + 无协议：15 秒观察、双快照零变化）、denied-ipc、
  accept-order（四项落盘 + 终态严格判定）、variants-corrupt、variants-close
  三连 15/15；另跑 consent-gate（新用户）、accept-restart（同意后重启不再
  询问）、policy-update（再次更新触发重新同意）均通过。
- 真实 0.6.0 → 本版单跳与连续链由 PR 的 Install test（单跳）与合并后
  Release（两条链）执行；本机不运行真实安装/升级。

**未解决/边界**：AC07 暂缓；N10 开放；真实自启出现/移除仍待 D5 3.4 人工
签字；WorkBuddy 双版本与历史 CI 偶发失败根因不受本批影响。
