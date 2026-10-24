# 价格同步 TLS 信任修复：eMudhra 中间 CA 私钥泄露

> **2026-10-22 归档复核：TF01–TF03 及本专项自身验收已完成。** [PR #29](https://github.com/ywltby/tokenscope/pull/29) 普通合并 `999424d`，v0.5.7 已发布，main 的 CI、Native UI E2E、Release 均成功。OpenRouter 解析故障由 [PR #30](https://github.com/ywltby/tokenscope/pull/30) 修复并发布 v0.5.8；在同时包含 TLS 更新和解析修复的 main `cb35f83` 上，现有显式联网用例两项均通过，OpenRouter 461 条、models.dev 8733 条，临时快照读回成功。v0.5.7 已登记为升级基线，v0.5.8 基线由同批的 `7f0618f` 单独登记。该专项无独立待验项；OpenRouter 安装后设置页人工同步属于其专项及[整合计划 N11](../../active/2026-10-11-consolidated-remaining-work.md#n11最新项目模型与-ccs-的原生统一验收)，未借本用例标为通过。

## 归档前原文

> 状态：**用户 2026-10-22 要求先写计划再执行**。本计划是一次独立的安全依赖更新，不占整合计划的责任编号。合并并验证发布后归档到 `archive/implemented/`，并在计划总账登记。
> 本次依赖更新由用户明确批准，不属于 CLAUDE.md 所说的“顺手更新第三方依赖”。

**Goal：** TokenScope 的价格同步不再信任 eMudhra 泄露私钥的 8 张中间 CA 证书及其根证书，从而排除“伪造 models.dev / OpenRouter 证书、投喂假价格”的风险。除依赖版本与应用版本号外，产品行为不变。

**Architecture：** 价格同步走 `ureq` → `rustls` → `rustls-webpki` 校验证书链，信任锚来自内置的 `webpki-roots`，不使用 Windows 证书库，因此操作系统的屏蔽对它不起作用。修复方式是把这两个库升到事件公告给出的修复版本，不换 TLS 实现，也不改信任来源。

**Tech Stack：** Cargo 锁文件更新；Rust 集成测试（忽略式，需显式开启的真实联网用例）；现有发布流水线。

## 1. 已核实事实（2026-10-22）

- **事件**：[eMudhra Intermediate CA Compromise](https://emudhra.we-have-a-problem.com/)（2026-10-20 公布）。8 张 eMudhra 中间 CA 的私钥可以从证书本身推算出来，这些中间 CA 挂在 Comodo AAA Services root 与 emSign ECC Root CA - G3 之下。公告给出的 Rust 修复版本：`rustls-webpki` 0.103.16，`webpki-roots` / `webpki-root-certs` 1.0.10。
- **本仓现状**（`Cargo.lock`）：`rustls-webpki` 0.103.15、`webpki-roots` 1.0.9、`rustls` 0.23.45、`ureq` 3.4.2。`cargo tree -i` 显示这两个库只由 `ureq` 引入，也就是只用于价格同步；`reqwest` 不在依赖树中。`ureq` 只开启 `json` 特性，用的是默认的 rustls 加 webpki-roots。
- **两个新版本的实际改动**（下载 crates.io 包后与旧版逐文件对比）：
  - `webpki-roots` 1.0.10：只删除了一个根证书，即 emSign ECC Root CA - G3。Comodo AAA Services root 仍然保留。
  - `rustls-webpki` 0.103.16：新增 `DISTRUSTED_SPKIS`，按公钥写死 8 张中间证书（`EMDVTLSCAG3B/C`、`EMEVTLSCAG3B/C`、`EMOVTLSCAG3B/C`、`EMOVCSCAG3B/C`）。证书链中只要出现其中任何一个，就以 `CertDistrusted` 拒绝；挂在 Comodo AAA 下的交叉签名路径正是靠这一条挡住的。
  - 结论：两个库**都要升级**，只升其中一个都不完整。
- **更新范围**：`cargo update --dry-run -p webpki-roots --precise 1.0.10` 与 `-p rustls-webpki --precise 0.103.16` 各自只改动对应的这一个包。
- **价格源不受影响**：models.dev 与 openrouter.ai 当前的证书链都是 `WE1` → `GTS Root R4` → `GlobalSign Root CA`（Google Trust Services），不经过 eMudhra 或 emSign，也不是被删除的根证书。
- **上一版本**：v0.5.6 已在 `scripts/fixtures/upgrade_baselines.json` 登记。本次是 0.5.6 之后第一个产品改动，按规则递增 patch 到 0.5.7。

## 2. 范围与不做

做：
- 更新上述两个依赖。
- 新增一个需显式开启的真实联网价格同步用例。
- 递增 patch 版本。
- 发布 v0.5.7 并验证。

不做：
- 不升级 `ureq`、`rustls` 及其他依赖，不改 `ureq` 特性。
- 不改为使用系统证书库（如 `rustls-platform-verifier`）。这会改变信任来源与离线行为，需要单独评估。
- 不改价格同步逻辑与快照格式。
- 不处理本机其他软件（Node.js、rustup 等）。这些由用户自行升级，见 §6。

## 3. 必须保持的不变量

1. **改动面最小**：`Cargo.lock` 的差异只有 `webpki-roots`、`rustls-webpki` 两个包的版本与校验和，以及版本递增带来的 `tokenscope`、`tokenscope-app` 两个本地包版本号；`Cargo.toml` 的依赖声明不变。
2. **价格同步可用**：升级后对 models.dev 与 OpenRouter 的真实同步都成功，快照能解析，条数大于 0。
3. **普通门禁全绿**：普通构建与 acceptance 构建的 clippy、测试都通过。`scripts/build-inputs.json` 的 `--check` 通过；若新版 `rustls-webpki` 用 `include_bytes!` 引入的数据文件被清单收录，就按规则更新并提交清单。
4. **测试密闭**：联网用例默认忽略，需设置环境变量才运行；快照只写临时目录，不触碰真实 `~/.tokenscope`。

## 4. 任务

### TF01 更新两个依赖

- 设置代理（`HTTP_PROXY`/`HTTPS_PROXY=http://127.0.0.1:7897`）后执行：
  - `cargo update -p webpki-roots --precise 1.0.10`
  - `cargo update -p rustls-webpki --precise 0.103.16`
- 核对 `git diff Cargo.lock` 符合不变量 1，再用 `cargo tree -i` 确认两个新版本已经生效。
- 这一改动无法在仓库内写出“先红”的测试：拒绝信任的逻辑在上游库里，上游已有测试 `distrusted_spki`。证据改为记录升级前后的 `cargo tree` 版本对比，以及上游改动的出处。

### TF02 真实联网价格同步用例

- 新增 `crates/tokenscope-core/tests/pricing_live_sync.rs`，标记 `#[ignore]`，并且必须设置 `TOKENSCOPE_LIVE_PRICING=1` 才执行：
  - 在临时目录分别调用 `modelsdev::sync` 与 `openrouter::sync`；
  - 断言两者都成功、快照可以被 `load_snapshot` 读回、条目数大于 0。
- 本地带代理运行一次，结果记入 §5。不进 CI（CI 不访问真实价格源）。以后再更新 TLS 相关依赖时可以复用。

### TF03 版本与交付

- 先 fetch 最新 main，执行 `python -B scripts/bump_version.py --bump patch --summary "价格同步 TLS 信任库更新：拒绝信任 eMudhra 泄露私钥的中间 CA"`，再用 `--check --base-ref origin/main` 自查。
- 本地门禁：
  - `cargo fmt --all -- --check`；
  - 两套 clippy 与测试（普通与 acceptance）；
  - `python -B scripts/build_inputs.py --check`；
  - `python -B -m unittest discover -s scripts -p "test_*.py"`。
- 一个任务一个提交，开普通 PR，等 `gate`、`native-ui`、Install test 全部成功。**合并由用户决定。**
- 合并后核对 main 的 Release：构建、安装、六个单跳升级、两条连续升级链、发布全部成功，v0.5.7 公开；随后把 v0.5.7 登记为升级基线（下一次产品 PR 合入前完成），本计划归档。

## 5. 执行记录

2026-10-22 在分支 `fix/emudhra-tls-trust` 实施：

| 任务 | 提交 | 证据 |
| --- | --- | --- |
| TF01 更新两个依赖 | `baa78df` | `Cargo.lock` 仅 `rustls-webpki` 0.103.15 → 0.103.16、`webpki-roots` 1.0.9 → 1.0.10 两处版本与校验和变化；两个新校验和与从 crates.io 下载的包的 SHA-256 一致。由于 `Cargo.lock` 触发完整模式，提交钩子跑了完整检查，全部通过 |
| TF02 真实联网用例 | `ca4d1f2` | 本地带代理运行：models.dev 同步成功，8778 条。OpenRouter 的 TLS 握手与下载成功，但解析失败（`invalid type: integer 0, expected a string`），原因是上游把 override 的 `utc_start`/`utc_end` 改成了整数，与本次依赖更新无关。本机日志显示 2026-10-12 起自动同步就因此失败。按用户决定另立计划和 PR 修复（分支 `fix/openrouter-override-conditions`） |
| TF03 版本 | `e57f994` | 先合并最新 main（`02955d1`），再 `--bump patch` 到 0.5.7；`--check --base-ref origin/main` 输出 `release: true`；`upgrade_baselines.py check` 无警告；完整提交检查全部通过 |

与计划的偏离：
- 不变量 2 要求“两个价格源的真实同步都成功”。本 PR 只能证明 OpenRouter 的 TLS 与下载成功，解析修复由 OpenRouter 修复 PR 完成，届时用同一个联网用例复验。
- `build_inputs.py --check` 第一次运行时与联网用例的构建抢占文件锁，没有得到结论。完整模式的提交钩子不包含这项检查，所以在版本递增后单独重跑：退出码 0，`missing`、`stale`、`changed` 均为空，清单不需要更新。新版 `rustls-webpki` 用 `include_bytes!` 引入的数据文件在依赖包内部，不在本仓清单的收录范围内。

合并后验收（2026-10-22 回填）：

- PR #29 合并 `999424d`，main 的 [CI 39719161825](https://github.com/ywltby/tokenscope/actions/runs/39719161825)、[Native UI E2E 39719161890](https://github.com/ywltby/tokenscope/actions/runs/39719161890)、[Release 39719161921](https://github.com/ywltby/tokenscope/actions/runs/39719161921) 全成功。Release 的构建、安装、六组单跳升级、两条连续升级链、publish 均实际成功，[v0.5.7](https://github.com/ywltby/tokenscope/releases/tag/v0.5.7) 已公开；基线登记提交 `4e83895` 的同一记录已随 PR #32 合入 main。
- 原 TF02 的 OpenRouter 解析失败已由 PR #30 修复。受测提交 `cb35f832b0113756fb3ce35ec785df2b3ecf219a`，命令为 `TOKENSCOPE_LIVE_PRICING=1 cargo test --locked -p tokenscope --test pricing_live_sync -- --ignored --nocapture`（PowerShell 以环境变量设置开关），两项通过；2026-10-22 13:53（Asia/Shanghai）OpenRouter 同步 461 条、models.dev 同步 8733 条，并各自读回快照核对条数。快照只写临时目录，不触碰真实 `~/.tokenscope`。
- 测试二进制 SHA-256：`c5355789da3cd97047492afdecbe38a43eab68b49f346a2263809e5e71213872`。日志与源码/产物身份保留在本机 `collection-status-timing` 工作树的 `qa-artifacts/timing-fix/combined-pricing-live.log`、`combined-pricing-live-identity.json`。此证据证明 TLS 与解析修复的组合可用，不替代安装后 GUI 操作。
- PR #30 的 main CI、原生及 Release 亦全成功，v0.5.8 已公开，详见其[归档记录](../partial/2026-10-22-openrouter-override-conditions.md)。本专项自身验收闭合，归档到 `implemented/`。

## 6. 本机其他软件（用户自行处理，不属于本仓改动）

2026-10-22 只读检查结果：

| 组件 | 本机版本 | 公告修复版本 | 处理 |
| --- | --- | --- | --- |
| Node.js | 24.14.0 | 24.22.1 | 需要升级 |
| rustup | 1.29.1 | 1.29.2 | 运行 `rustup self update` |
| Python certifi、pip、uv | 2026.10.19、26.2.2、0.13.4 | 相同 | 已是修复版本 |
| Go | 1.27.3 | 1.27.3 | 已是修复版本 |
| Java | 25.0.3 | 修复版尚未发布 | 等待修复版 |
| Git for Windows | 2.53.0 | 公告未列出 | 本机配置为 `schannel`，跟随 Windows 证书库 |
| Windows | 根证书自动更新最后同步 2026-10-21 | 公告未列版本 | 建议手动运行一次 Windows 更新 |
