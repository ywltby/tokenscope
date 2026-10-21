# 价格同步 TLS 信任修复：eMudhra 中间 CA 私钥泄露

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

（实施时回填。）

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
