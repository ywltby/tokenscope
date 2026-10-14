# UsageMesh 用量统计实现拆解（源码级）

> 研究对象：[`Atingaii/UsageMesh`](https://github.com/Atingaii/UsageMesh) 公开源码，
> 克隆 HEAD `8cfb77f19f731616fea38a1b5f6672a41e644e9a`
> （`fix(quota): retain official observations during refresh failures`，提交时间 2026-09-11 23:17 +0800；
> `rust-cli/Cargo.toml` 版本 `2.7.0`，`web-ui/package.json` 版本 `2.4.0`）。
> 其内嵌扫描器为 `tokscale-core`，由 `Cargo.lock:1879-1881` 钉死为
> `git+https://github.com/junhoyeo/tokscale?tag=v4.14.0#1ec865d4e9e5adf157efb9f5c2dfaf29630d71e5`；
> 本文按该精确提交检出（`1ec865d chore: bump version to 4.14.0`）逐文件核对，
> **不以 tokscale 主干（v4.18.0）行为替代 v4.14.0 行为**。
>
> 研究方法：浅克隆两个仓库到临时目录后逐文件精读（符号定位 → 分段读原文 → 交叉核对调用点与常量），
> 抽取出 `文件:行号` 级证据；关键断言由独立复核代理做对抗性验证（见附录 C 复核记录）。
> **未运行任何二进制**（未 `cargo build`、未执行 `usagemesh` 子命令）、**未读取任何真实用户会话数据**
> （未扫描本机 `~/.claude`、`~/.codex`、`~/.config/tokscale` 等目录内容），全部结论来自源码与仓库内文档。
>
> 证据分档：**【已核实】**（源码直证，且经交叉复核）／**【代核实】**（由二级取证代理逐行读取源码并给出行号，一级代理抽查后收录）／
> **【未验证】**（结构推断、未实测，正文中显式标注）。
>
> 行号口径：一律为**文件实际行号**（编辑器 / `read` 工具口径）。`docs/` 内联的英文原文只在必要处引用。
>
> 本文只记录字段名、表名、路径规则与计算口径，**不包含任何真实会话内容**；示例中的模型名、渠道名均来自源码注释与测试固件。

## 目录

- [1. 范围与方法](#1-范围与方法)
- [2. 总架构与端到端数据流](#2-总架构与端到端数据流)
  - [2.1 定位与账本链路概览](#21-定位与账本链路概览)
  - [2.2 一次 `sync` 的主链路调用顺序](#22-一次-sync-的主链路调用顺序)
  - [2.3 端到端数据流](#23-端到端数据流)
- [3. 多 Agent 读取：内嵌 tokscale-core v4.14.0](#3-多-agent-读取内嵌-tokscale-core-v4140)
  - [3.1 本章要回答的问题](#31-本章要回答的问题)
  - [3.2 公开 API 与事件模型](#32-公开-api-与事件模型)
  - [3.3 客户端注册表](#33-客户端注册表)
  - [3.4 parse_local_clients 主链路](#34-parse_local_clients-主链路)
  - [3.5 逐客户端读取实现（分小节）](#35-逐客户端读取实现分小节)
  - [3.6 token 桶与去重语义](#36-token-桶与去重语义)
  - [3.7 缓存与增量](#37-缓存与增量)
  - [3.8 与 v4.18.0 差异](#38-与-v4180-差异)
  - [3.9 对 UsageMesh 的实际影响汇总（跨节收口）](#39-对-usagemesh-的实际影响汇总跨节收口)
  - [3.10 未验证项](#310-未验证项)
- [4. 设备侧采集、账本结构与聚合口径](#4-设备侧采集账本结构与聚合口径)
  - [4.1 CLI 命令面与调用链](#41-cli-命令面与调用链)
  - [4.2 采集触发链：`sync` → `collect`、`since` 来源与全量重扫条件](#42-采集触发链sync-collectsince-来源与全量重扫条件)
  - [4.3 账本结构（`model.rs` 全文语义）与数值语义](#43-账本结构modelrs-全文语义与数值语义)
  - [4.4 `collect()` 的分组维度、分钟桶与时间归一](#44-collect-的分组维度分钟桶与时间归一)
  - [4.5 `merge_incremental` 边界语义与缺陷评估](#45-merge_incremental-边界语义与缺陷评估)
  - [4.6 聚合口径总纲（七层并列）](#46-聚合口径总纲七层并列)
- [5. 金额计算与渠道身份](#5-金额计算与渠道身份)
  - [5.1 `PriceBook::load()`：数据来源、回退链、缓存与过期、policy 标识](#51-pricebookload数据来源回退链缓存与过期policy-标识)
  - [5.2 `quote_on_date(model, date, tier, metrics)` 完整语义](#52-quote_on_datemodel-date-tier-metrics-完整语义)
  - [5.3 `lower_bound` 的成立条件、传播路径与「未知」是否被静默吞掉](#53-lower_bound-的成立条件传播路径与未知是否被静默吞掉)
  - [5.4 `provider.rs::classify(raw_provider, model, explicit, hint)` 完整规则](#54-providerrsclassifyraw_provider-model-explicit-hint-完整规则)
  - [5.5 `evidence.rs`：路由证据来源、结构、匹配键、优先级、effort 提取](#55-evidencers路由证据来源结构匹配键优先级effort-提取)
  - [5.6 `codex_tier.rs`：增强解析、tier 归属、日级对账门、cache_write_known、时间与 effort 来源](#56-codex_tierrs增强解析tier-归属日级对账门cache_write_known时间与-effort-来源)
  - [5.7 `docs/PRICING.md` 与代码实现逐条对照](#57-docspricingmd-与代码实现逐条对照)
  - [5.8 「一条用量消息 → 最终展示金额」完整链与重复/漏价风险](#58-一条用量消息-最终展示金额完整链与重复漏价风险)
  - [5.9 计价公式与匹配链](#59-计价公式与匹配链)
  - [5.10 路由身份规则表](#510-路由身份规则表)
  - [5.11 文档-代码一致性对照](#511-文档-代码一致性对照)
  - [5.12 未验证项](#512-未验证项)
- [6. 云端仪表盘：解密、多设备合并与展示](#6-云端仪表盘解密多设备合并与展示)
  - [6.1 解锁与解密](#61-解锁与解密)
  - [6.2 数据获取（GitHub 分支读取）](#62-数据获取github-分支读取)
  - [6.3 `data.ts` 的合并与聚合](#63-datats-的合并与聚合)
  - [6.4 `analytics.ts` / `insights.ts` 的算法](#64-analyticsts-insightsts-的算法)
  - [6.5 配额周期（`quotaCycles.ts` + `subscriptionPeriods.ts` + `subscriptions.ts` + `views/QuotaCycles.tsx`）](#65-配额周期quotacyclests-subscriptionperiodsts-subscriptionsts-viewsquotacyclestsx)
  - [6.6 金额：web-ui 是否重算成本？](#66-金额web-ui-是否重算成本)
  - [6.7 持久化与偏好](#67-持久化与偏好)
  - [6.8 测试固化的边界断言（10 条）](#68-测试固化的边界断言10-条)
  - [6.9 聚合口径细节](#69-聚合口径细节)
  - [6.10 配额周期规则](#610-配额周期规则)
  - [6.11 未验证项](#611-未验证项)
- [7. 本地工作台与官方配额](#7-本地工作台与官方配额)
  - [7.1 HTTP 接口表](#71-http-接口表)
  - [7.2 HTTP 服务与安全边界【已核实】](#72-http-服务与安全边界已核实)
  - [7.3 `activity.rs` 的本地活动视图【已核实】](#73-activityrs-的本地活动视图已核实)
  - [7.4 配额观测的数据结构、采样、周期与 `stale`【已核实】](#74-配额观测的数据结构采样周期与-stale已核实)
  - [7.5 `quota_official.rs` 读取已登录 `codex app-server`【已核实】](#75-quota_officialrs-读取已登录-codex-app-server已核实)
  - [7.6 `configure.rs` 的 guarded 读写（与「只读」对照）【已核实】](#76-configurers-的-guarded-读写与只读对照已核实)
  - [7.7 `network.rs` 只读探测【已核实】](#77-networkrs-只读探测已核实)
  - [7.8 `store.rs` / `alerts.rs`：落盘与提醒【已核实】](#78-storers-alertsrs落盘与提醒已核实)
  - [7.9 前端聚合、配额渲染与预测【已核实】](#79-前端聚合配额渲染与预测已核实)
  - [7.10 配额与预测算法](#710-配额与预测算法)
  - [7.11 与云端口径差异](#711-与云端口径差异)
  - [7.12 未验证项](#712-未验证项)
- [8. 同步、加密、调度与自动更新](#8-同步加密调度与自动更新)
  - [8.1 `crypto.rs`：密钥、KDF 与信封格式](#81-cryptors密钥kdf-与信封格式)
  - [8.2 `github.rs`：分支命名、覆盖提交、心跳与发布去重](#82-githubrs分支命名覆盖提交心跳与发布去重)
  - [8.3 `scheduler.rs`：常驻循环与 OS 监督](#83-schedulerrs常驻循环与-os-监督)
  - [8.4 `update.rs`：自更新检测、校验、替换与迁移重扫](#84-updaters自更新检测校验替换与迁移重扫)
- [9. 与 TokenScope 的对照](#9-与-tokenscope-的对照)
- [10. 风险总览与未验证项索引](#10-风险总览与未验证项索引)
  - [10.1 跨章节风险总览](#101-跨章节风险总览)
  - [10.2 设备侧与传输链风险（原始清单）](#102-设备侧与传输链风险原始清单)
  - [10.3 各章未验证项索引](#103-各章未验证项索引)
- [附录 A：关键常量与阈值速查](#附录-a关键常量与阈值速查)
- [附录 B：关键符号索引（符号 → 文件:行号）](#附录-b关键符号索引符号-文件行号)
  - [B.1 UsageMesh 设备侧（`rust-cli/src/`）](#b1-usagemesh-设备侧rust-clisrc)
  - [B.2 UsageMesh 仪表盘侧（`web-ui/src/`、`rust-cli/local-web/`）](#b2-usagemesh-仪表盘侧web-uisrcrust-clilocal-web)
  - [B.3 tokscale-core v4.14.0（`tokscale-core/src/`）](#b3-tokscale-core-v4140tokscale-coresrc)
- [附录 C：多轮复核记录](#附录-c多轮复核记录)
  - [C.1 复核流程](#c1-复核流程)
  - [C.2 第 1 轮发现与处置](#c2-第-1-轮发现与处置)
  - [C.3 第 2 轮发现与处置（修正落地审计）](#c3-第-2-轮发现与处置修正落地审计)
  - [C.4 第 3 轮（冻结稿抽检）与第 4 轮（覆盖度回填）](#c4-第-3-轮冻结稿抽检与第-4-轮覆盖度回填)
  - [C.5 残留不确定性与已知边界](#c5-残留不确定性与已知边界)
  - [C.6 复核产物清单（可在本机临时目录复核）](#c6-复核产物清单可在本机临时目录复核)

## 1. 范围与方法

| 项 | 内容 |
| --- | --- |
| 研究对象版本 | UsageMesh `8cfb77f`（v2.7.0）+ 内嵌 tokscale-core `v4.14.0`（`1ec865d`） |
| 仓库形态 | Rust CLI（`rust-cli/`，单 bin `usagemesh`）+ React 静态仪表盘（`web-ui/`）+ 本机 Web 工作台（`rust-cli/local-web/`）+ GitHub Actions（`.github/workflows/`） |
| 读取范围 | `rust-cli/src/**`（21 个 Rust 文件）、`rust-cli/local-web/**`、`web-ui/src/**`、`web-ui/tests/**`、`docs/**`、`README*.md`、`.github/workflows/**`、以及 tokscale v4.14.0 的 `crates/tokscale-core/src/**` |
| 不读范围 | 任何真实设备数据、任何线上账户、`~/.config/usagemesh` 运行态、GitHub 远端分支内容 |
| 方法 | 符号级 grep 定位 → 逐函数精读 → 调用链闭合 → 常量/阈值交叉核对 → 独立代理对抗性复核 |
| 证据密度 | 每条结论附 `文件:行号`；文件路径缩写规则见下 |

引用口径：
- UsageMesh 侧一律用**仓库根相对路径**，如 `rust-cli/src/collector.rs:211`、`web-ui/src/lib/data.ts:15`、`docs/PRICING.md:13`；
- tokscale 侧一律用 **crate 根相对路径**，如 `tokscale-core/src/lib.rs:4334`，指 `crates/tokscale-core/src/lib.rs`
  （固定于 tag `v4.14.0`，提交 `1ec865d`）；出现歧义处会显式写出完整路径。
- 附录 A 给出「关键常量与阈值」速查，附录 B 给出「关键符号 → 文件:行号」索引，附录 C 记录多轮复核流程与发现台账。

## 2. 总架构与端到端数据流

### 2.1 定位与账本链路概览

UsageMesh 是"零服务器"跨设备用量统计工具：本机用嵌入式 Tokscale 扫描器（`parse_local_clients`）解析 AI 编程客户端日志——客户端注册表共 **50 个 `ClientId`**，**实际进入统计的是 47 个通道 A 分派客户端**（外加 1 个 `synthetic` 归一化段；`parse_local()==true` 者 49 个，不产出的三者是 `cursor`（`parse_local: false`）与 `micode`、`fx`（两者 `parse_local: true` 却在通道 A 无分派），见 §3.3.2、§3.5.12）——把聚合结果写成本地账本缓存，再用工作区级 AES-256-GCM 密钥加密成单文件信封，通过 GitHub REST（blob → tree → commit → ref）覆盖式推送到**每设备一条孤立分支** `um-ledger-<device_hash>`；云端仪表盘是 GitHub Pages 静态站点，用密码换取工作区密钥后直接在浏览器解密，不做二次计价。旁路还有三类小分支：`um-presence-<device_hash>`（心跳）、`um-index`（设备分支索引）、`um-dashboard`（密码包裹的密钥清单）。另有完全独立的本地工作区（`serve`），它**不走**账本链路，直接扫 90 天本机数据渲染本地页面。

### 2.2 一次 `sync` 的主链路调用顺序

主链路调用顺序（一次 `sync`）：自更新检查 → 调度器迁移 → 单实例锁 → 读取上次缓存账本 → 判定全量/增量 → `collector::collect` → `merge_incremental`（增量时）→ 官方配额合并 → 变更判定 → 写本地缓存 → 加密 → 推送 ledger 分支 → 刷新设备索引 → 清除待上传标记 → presence 心跳。

### 2.3 端到端数据流

1. 本机日志 → 统一事件：`collector::collect` 传入 `LocalParseOptions`（`collector.rs:211-221`），`since` 决定扫描窗口；`evidence::scan(incremental)` 补路由/推理强度证据（`collector.rs:223`、`evidence.rs:774-805`；增量时文件 mtime 3 天窗口 `evidence.rs:53-64`）。
2. 事件 → 定价与路由身份：逐条 `canonical_model_id` + `route_for_message`（`collector.rs:238-239` → `collector.rs:138-162` → `provider::classify` `provider.rs:229`），`priced_metrics_from_message` 按 `date` 计价（`collector.rs:113-125` → `pricing.rs:308-320`，未收录模型返回 `lower_bound=true` 且成本 0）。
3. 事件 → 行与明细：`RequestDetail`（`collector.rs:241-261`）+ `RowKey` 分钟桶聚合（`collector.rs:262-278`）；Codex 增强行走同一路径（`collector.rs:281-339`）；`totals` 由行汇总（`collector.rs:342-359`）；明细裁剪（`collector.rs:361`）。
4. 账本 → 本地缓存：`Ledger` 构造（`collector.rs:362-372`）→ `config::write_cached_ledger`（`main.rs:415` → `config.rs:219-225`）→ `<config_dir>/ledger-cache.json`（权限 0600，`config.rs:250-276`）。
5. 账本 → 加密信封：`crypto::encrypt_ledger(&ledger, &config.dashboard_key)`（`main.rs:448` → `crypto.rs:40-66`）。
6. 信封 → GitHub 分支：`github.replace_snapshot(&config.device_id, &envelope)`（`main.rs:449`）→ `um-ledger-{hash}/ledger.json`（`github.rs:367-376` + `244-314`）；紧随 `refresh_dashboard_index()` 写 `um-index/index.json`（`main.rs:450-452` → `github.rs:351-365`）；`publish_presence_if_due` 写 `um-presence-{hash}/presence.json`（`main.rs:437/455` → `main.rs:264-280` → `github.rs:382-399`）。
7. 密码清单 → `um-dashboard/access.json`：仅在 `setup`/`join`/`password` 时发布（`main.rs:181-191, 535, 561` → `github.rs:401-409`）。
8. 云端仪表盘（GitHub Pages 静态站）：部署工作流 `pages.yml:1-120`（上游仓库直接用自己的 `web-ui`，fork 则稀疏克隆上游 `web-ui`，`pages.yml:31-47`；部署前用 `um-index` 覆盖构建产物里的 `device-index.json` 作为回退，`pages.yml:90-98`）。浏览器侧：常量与 AAD 前缀 `web-ui/src/lib/data.ts:41-48`；密码 → 工作区密钥 `workspaceKey`（`data.ts:492-556`）；设备分支发现 `loadDeviceIndex`（优先 `um-index/index.json`，回退 `um-dashboard/device-index.json` 与部署内静态文件，`data.ts:558-605`）；账本信封校验（`kind="usagemesh-encrypted-ledger"`、`schemaVersion=2`、`AES-256-GCM`、`branch === um-ledger-{envelope.deviceHash}`，`data.ts:626-633`）；AES-GCM 解密用同一 AAD 前缀（`data.ts:48,543-544`）；前端**不重算价格**，直接用设备端 `storedCost`（`data.ts:15-39`）；presence 与账本并发拉取（`data.ts:616-625`）。
9. 本机 `serve` 本地工作区（与账本链路完全并行，不读写 GitHub）：`local::serve`（`local/mod.rs:395-542`）绑定 `127.0.0.1`（`local/mod.rs:417-420`），启动时生成一次性 bearer token 并输出带 `#token=` 的 URL（`local/mod.rs:421-423`）；`State::scan` 调 `activity::scan`（`local/mod.rs:39-60` → `local/activity.rs:8-33`，用 `parse_local_clients` 扫**最近 90 自然日**、`use_env_roots=!isolated`）；产物写 `activity.json`/`alerts.json`/`events.json`/`settings.json`（`local/mod.rs:43-46,101-104,109,292-295`；路径定义 `local/store.rs:94-143`）；请求校验 Host/Origin/Bearer（`local/mod.rs:372-389, 476-492`），响应带 CSP 等安全头（`local/mod.rs:390-394`）；静态页面内嵌 `rust-cli/local-web/*`（`local/mod.rs:445-475`）；按 `preferences.scan_minutes` 定期重扫（`local/mod.rs:427-432`，默认 5 分钟 `local/store.rs:58-69`）。

（文字链，箭头两侧标注代码位置）

```
本机客户端日志(JSONL/SQLite…)
  →[collector.rs:211-221  tokscale-core parse_local_clients(since=今天-2天)]
  → 统一 ParsedMessage 流
  →[collector.rs:138-162 + provider.rs:229 路由身份；collector.rs:113-125 + pricing.rs:308 逐条计价]
  →[collector.rs:230-278 按 RowKey(10 字段) + 分钟桶聚合；241-261 明细；361 明细裁到最新 1000]
  → Ledger{rows,requests,totals,pricing}[collector.rs:362-372]
  →[config.rs:219-225  ledger-cache.json，0600 原子写]
  →[crypto.rs:40-66  AES-256-GCM，AAD=usagemesh-ledger-v2:{device_hash}]
  →[github.rs:244-314 blob→tree→commit(parents:[])→ref(强制 PATCH) ]
  → GitHub: um-ledger-{hash}/ledger.json
            um-index/index.json（github.rs:351-365）
            um-presence-{hash}/presence.json（github.rs:382-399）
            um-dashboard/access.json（github.rs:401-409，仅密码变更时）
  →[web-ui/src/lib/data.ts:492-556 密码→PBKDF2(600k)→AES-GCM 解出工作区密钥]
  →[data.ts:558-605 读 um-index 发现分支；607-649 解密 ledger.json]
  → 静态仪表盘渲染（直接用设备端 cost，data.ts:15-39）

旁路（不联网、不写账本）：
本机日志 →[local/activity.rs:8-33 最近 90 天] → activity.json →[local/mod.rs:395-542 tiny_http 127.0.0.1]
          → 内嵌 local-web 页面（local/mod.rs:445-475）
```

## 3. 多 Agent 读取：内嵌 tokscale-core v4.14.0

> **引用体例（全章统一）**：裸写的 `xxx.rs:行号` 中，`lib.rs` / `clients.rs` / `scanner.rs` / `bucket_tz.rs` / `paths.rs` / `message_cache.rs` / `provider_identity.rs` 等指 `crates/tokscale-core/src/` 下的同名文件；解析器文件（`claudecode.rs`、`codex.rs`、`kiro.rs`、`opencode.rs`、`opencode_schema.rs` …）指 `crates/tokscale-core/src/sessions/` 下的同名文件；`aliases.rs` 指 `crates/tokscale-core/src/pricing/aliases.rs`。已实测：这些名字在 `crates/tokscale-core/src` 顶层**不存在**同名文件，故省略前缀不会与顶层文件冲突。用法面（UsageMesh 侧）的 `collector.rs` / `evidence.rs` / `provider.rs` / `main.rs` / `model.rs` / `codex_tier.rs` 指 `usagemesh-research\rust-cli\src\` 下的同名文件，`activity.rs` 指 `rust-cli\src\local\activity.rs`（全文以 `local/activity.rs` 或 `activity.rs` 两种写法出现），行号对 HEAD `8cfb77f1` 成立。带 `v4.18.0:` 前缀的行号一律对**标签 `v4.18.0` 解析出的 commit**（`d4d1c751856e25913bce97bfbd7b254308863239`；实测 `git cat-file -t` 对该哈希返回 `commit`——`v4.18.0` 是轻量标签，不存在独立 tag 对象）成立，**不对 `tokscale-research` 工作树（main）成立**；本报告的 v4.14.0 基线是 `1ec865d4e9e5adf157efb9f5c2dfaf29630d71e5`。跨节续引的裸 `:NNN` 沿用最近一次显式给出的文件名。

### 3.1 本章要回答的问题

UsageMesh 的本地用量统计**不自己扫描任何 AI 工具的日志目录**。它把「本地事实采集」整条链外包给 tokscale-core，自己在外面套一层路由/计价/证据层：

- `rust-cli/src/collector.rs:209-221` 注释原文：“Tokscale remains the sole canonical local accounting pass: discovery, parsing, deduplication, token-bucket semantics and model attribution.”
- 调用面只有三个符号族：
  - `parse_local_clients(LocalParseOptions)` → `ParsedMessages`（`collector.rs:211-221`、`local/activity.rs:12-20`）
  - `canonical_model_id`（`collector.rs:238`、`collector.rs:286`）
  - `ClientId`（`collector.rs:197` 枚举全部客户端；`evidence.rs:736`）
  - 另加 `BucketTimezone`、`ScannerSettings`（`codex_tier.rs:19`）
- 因此本部分就是「v4.14.0 里这一整条读取链的取证」。

#### 3.1.1 一句话结构

```
LocalParseOptions
  └─ parse_local_clients()            lib.rs:4334
        ├─ get_home_dir_string()      lib.rs:719
        ├─ 客户端清单解析（默认 = 所有 parse_local() 的 ClientId + "synthetic"） lib.rs:4339-4346
        ├─ scanner::scan_all_clients_with_scanner_settings()   scanner.rs:1385
        │     └─ scan_all_clients_with_env_strategy_inner()    scanner.rs:1439
        ├─ 逐客户端分派解析（47 个 ClientId 分派段 + 1 个 synthetic 段） lib.rs:4367-5246
        ├─ 客户端过滤 retain_for_requested_clients()           lib.rs:186-201 / 5249-5254
        ├─ synthetic 网关字段归一化                            lib.rs:5256-5263
        ├─ rebucket_parsed_days()（钉时区时重算 date）          lib.rs:5273 / 5286
        └─ filter_parsed_messages()（year/since/until 字符串比较） lib.rs:5275 / 5392
      → ParsedMessages { messages, counts, processing_time_ms } lib.rs:359-363 / 5277-5281
```

【已核实】

#### 3.1.2 与 v4.14.0 的两条独立读取通道

v4.14.0 里存在**两套互不相同的本地读取实现**，UsageMesh 只吃到其中一套：

| 通道 | 入口 | 内部函数 | 是否走持久缓存 | 产出类型 | 是否带 cost |
|---|---|---|---|---|---|
| **A（UsageMesh 用的）** | `parse_local_clients` `lib.rs:4334` | 自己一大段内联分派（47 个 ClientId 段 + 1 个 synthetic 段）`lib.rs:4367-5246` | **否** | `ParsedMessage`（无 cost 字段） `lib.rs:340-357` | 否 |
| B | `parse_local_unified_messages` `lib.rs:5343` | `parse_all_messages_with_pricing_with_cache_policy` `lib.rs:766` | 是（`SourceCachePolicy::Persistent`） `lib.rs:745-747` | `UnifiedMessage`（含 cost/cost_source） `sessions/mod.rs:75-109` | 是 |

通道 A 里 `lib.rs:5248-5254` 的注释直接说明了它是另一条实现路径：

```
5248:    // Filter BEFORE normalization (see parse_all_messages_with_pricing).
5269:    // This path builds `ParsedMessage` straight from the parsers instead of
5270:    // going through `parse_all_messages_with_pricing_with_env_strategy`, so it
5271:    // needs its own rebucket — `tokscale report` and `tokscale wrapped` read
5272:    // day keys from here.
```

**这条区分对本研究至关重要**：通道 A 复用了同样的 `sessions::*` 解析器、同样的 `dedup_key`、同样的日期推导，但**不复用**通道 B 才有的三样东西：

1. `message_cache`（持久 bincode 分片缓存）—— 通道 A 一行都不碰；
2. `merge_claude_cross_file_duplicate` `lib.rs:4242` + `merge_message_completeness` `sessions/claudecode.rs:923`（跨文件重复的「逐字段取大」合并）—— 通道 A 用裸 `HashSet::insert` 先到先得；
3. `apply_pricing_if_available` `lib.rs:4212` / `pricing_multiplier`（Zed +10%）`lib.rs:4190`—— 通道 A 拿到的是 `cost = 0.0`。

此外通道 A 里 Codex 与部分客户端**没有**调用 `apply_headless_agent` `lib.rs:4184`（对比 `lib.rs:4454-4461` 传了 `is_headless` 却只用于…见下），而通道 B 有。见 E.9.2 与第 5.5 节。

---

### 3.2 公开 API 与事件模型

#### 3.2.1 `pub mod` 与再导出面（`crates/tokscale-core/src/lib.rs`）

【已核实】`lib.rs:3-34`：

| 行 | 内容 |
|---|---|
| `lib.rs:3-21` | 模块声明：`mod aggregator; pub mod bucket_tz; mod cc_mirror; pub mod clients; pub mod content_extractor; pub mod fs_atomic; pub mod mcp; mod message_cache; pub mod model_alias; pub mod opencode_model_name; mod parser; pub mod paths; pub mod pricing; mod provider_identity; pub mod scanner; pub mod sessionize; pub mod sessions; pub mod tui_signal; pub mod wiki;` |
| `lib.rs:23` | `pub use aggregator::*;` |
| `lib.rs:24` | `pub use bucket_tz::BucketTimezone;` |
| `lib.rs:25` | `pub use clients::{ClientCounts, ClientDef, ClientId, PathRoot};` |
| `lib.rs:26` | `pub use message_cache::parser_generation;` |
| `lib.rs:27` | `pub use model_alias::ModelAliasMap;` |
| `lib.rs:28-29` | `pub use parser::*; pub use scanner::*;` |
| `lib.rs:30-33` | `pub use sessionize::{compute_daily_active_time, compute_daily_active_time_in, compute_time_metrics, sessionize, SessionInterval, TimeMetrics, DEFAULT_IDLE_GAP_MS};` |
| `lib.rs:34` | `pub use sessions::{CostSource, UnifiedMessage};` |

要点：
- **`message_cache` 是私有模块**（`lib.rs:10` `mod message_cache;`），只再导出 `parser_generation()`（`lib.rs:26`）——外部无法直接操纵缓存，只能读它的代数号。
- `bucket_tz::BucketTimezone` 是公开的（`lib.rs:24`），UsageMesh 的 `codex_tier.rs:19` 正是这么拿到的。
- `ScannerSettings` 通过 `pub mod scanner`（`lib.rs:17`）+ `pub use scanner::*`（`lib.rs:29`）得到。

#### 3.2.2 `LocalParseOptions`（`lib.rs:428-439`）

【已核实】定义原文：

```rust
428: #[derive(Debug, Clone, Default)]
429: pub struct LocalParseOptions {
430:     pub home_dir: Option<String>,
431:     pub use_env_roots: bool,
432:     pub clients: Option<Vec<String>>,
433:     pub since: Option<String>,
434:     pub until: Option<String>,
435:     pub year: Option<String>,
436:     /// Persistent scanner config loaded from `~/.config/tokscale/settings.json`.
437:     /// Defaults to empty when callers don't care about user-configured paths.
438:     pub scanner_settings: scanner::ScannerSettings,
439: }
```

| 字段 | 语义（已核实行号） | 默认值 |
|---|---|---|
| `home_dir` | `None` → `lib.rs:4337` 走 `get_home_dir_string(&None)` `lib.rs:719`：`None` 时退到 `crate::paths::home_dir()`（`lib.rs:722`），再失败才 `Err("HOME directory not specified and could not determine home directory")`（`lib.rs:723-725`）。`paths::home_dir()` 在 Windows 上优先 `%HOME%`（当且仅当它是绝对路径）`paths.rs:50-59` + `paths.rs:67-75`，否则 `dirs::home_dir()`。 | `None` |
| `use_env_roots` | 交给扫描层决定「是否读取各客户端自己的环境变量根」（`CODEX_HOME`/`CLAUDE_CONFIG_DIR`/`GEMINI_CLI_HOME`/`OPENCODE_DB` 等）。见 `clients.rs:98` `resolve_with_env_strategy`、`scanner.rs:1389-1391`。 | `false`（`Default` 派生） |
| `clients` | `None` → 默认取**全部 `parse_local()==true` 的 ClientId** 的 `as_str()`，再额外 push `"synthetic"`（`lib.rs:4339-4346`）。空 `Some(vec![])` 会走 `include_all = true` 分支（`lib.rs:4347`），但 scanner 端的 `include_all` 判断在 `scanner.rs:1447`（`clients.is_empty()`）。 | `None` |
| `since` | 字符串日期 `YYYY-MM-DD`，**对 `msg.date` 做字典序 `>=` 比较**（`lib.rs:5403-5405`）。不是时间戳。 | `None` |
| `until` | 同上，`<=`（`lib.rs:5407-5409`）。 | `None` |
| `year` | 字符串年份，转成 `"{year}-"` 前缀做 `starts_with`（`lib.rs:5398-5401`）。 | `None` |
| `scanner_settings` | 反序列化自 `~/.config/tokscale/settings.json` 的 `scanner` 键；`Default` 即全空。UsageMesh 自己在 `collector.rs:78-93` 实现了这份加载器（读 `dirs::config_dir()/tokscale/settings.json` 的 `scanner` 子对象）。 | `Default`（空） |

**UsageMesh 的实参**（`rust-cli/src/collector.rs:211-219`）：

```rust
211:    let parsed = parse_local_clients(LocalParseOptions {
212:        home_dir: None,          // → 系统真实 HOME
213:        use_env_roots: true,     // → 吃各客户端自己的 env 覆盖
214:        clients: None,           // → 全部 parse_local 客户端 + synthetic
215:        since: since.clone(),    // → 纯字符串日期上界/下界
216:        until: None,
217:        year: None,
218:        scanner_settings: scanner_settings.clone(),
219:    })
```

第二处调用 `rust-cli/src/local/activity.rs:12-20` 不同：`home_dir: Some(paths.home)`、`use_env_roots: !paths.isolated`、`since` 硬编码为本地今天减 89 天（`activity.rs:9-11`）、`scanner_settings: ScannerSettings::default()`（`activity.rs:19`）。

#### 3.2.3 `ParsedMessage`（`lib.rs:339-357`）

【已核实】

```rust
339: #[derive(Debug, Clone)]
340: pub struct ParsedMessage {
341:     pub client: String,
342:     pub model_id: String,
343:     pub provider_id: String,
344:     pub session_id: String,
345:     pub workspace_key: Option<String>,
346:     pub workspace_label: Option<String>,
347:     pub timestamp: i64,
348:     pub date: String,
349:     pub input: i64,
350:     pub output: i64,
351:     pub cache_read: i64,
352:     pub cache_write: i64,
353:     pub reasoning: i64,
354:     pub duration_ms: Option<i64>,
355:     pub message_count: i32,
356:     pub agent: Option<String>,
357: }
```

| 字段 | 含义与取值来源 |
|---|---|
| `client` | 客户端标识字符串。**不保证等于注册表 `id`**：Claude 的 cc-mirror 变体会写成 `"cc-mirror/<name>"`（`sessions/claudecode.rs:68` `format!("cc-mirror/{}", sanitize_cc_mirror_segment(&self.name))`，`:437-440` 用于 `UnifiedMessage::new_with_dedup` 的 client）。过滤时用 `retain_for_requested_clients` `lib.rs:186-201` 做前缀兼容。 |
| `model_id` | 解析器产出的**原始 model 串经客户端级 canonicalize**（如 Claude 走 `canonicalize_claude_model`，`claudecode.rs:684`）。注意：`ParsedMessage` 里的 `model_id` **未**经过 `canonical_model_id` 归一化；UsageMesh 在 `collector.rs:238` 自己再调一次 `canonical_model_id(&message.model_id)`。 |
| `provider_id` | 客户端级 provider 解析（如 Claude 走 `claude_provider_choice` `claudecode.rs:675-682`，Codex 用 `session_provider` 或按模型推断，缺省 `"openai"` `codex.rs:628-632`）。 |
| `session_id` | 客户端级会话身份。Claude：文件 stem，sidechain 时被父 session id 覆盖（`claudecode.rs:444-448`、`:524-537`）。Codex：文件 stem（`sessions/utils.rs:496-501`），fork 场景另有 `dedup_scope_id` 只进 dedup key。 |
| `workspace_key` / `workspace_label` | 归一化后的项目路径 / 展示名，由各解析器 `set_workspace()` 填（`sessions/mod.rs:390-397`）。Claude 从路径里取 `.claude/projects/<slug>`（`claudecode.rs:771-802`）。 |
| `timestamp` | **Unix 毫秒**。证据链：`sessions/mod.rs:1017` 注释 “Convert Unix milliseconds to a local YYYY-MM-DD date string.”；`sessions/mod.rs:1018-1020` `timestamp_to_date(timestamp_ms)`；`claudecode.rs:875-879` `dt.timestamp_millis()`；`codex.rs:909-913` 同样是 `timestamp_millis()`。⚠️ 个别冷门解析器可能写秒，见「未验证项」。UsageMesh 防御性地处理了两种单位（`collector.rs:38-44`）。 |
| `date` | `timestamp` 在**当时本机时区**下的 `%Y-%m-%d`，构造期由 `new_full` 得出（`sessions/mod.rs:367` `let date = timestamp_to_date(timestamp);`），之后可能被 `rebucket_date` / `rebucket_parsed_days` 覆写（`sessions/mod.rs:411-430`、`lib.rs:5286-5306`）。 |
| `input`/`output`/`cache_read`/`cache_write`/`reasoning` | 五桶 i64，**语义因客户端而异**，见 §3.6。 |
| `duration_ms` | `Option<i64>`，解析器算出的「本轮耗时」。Claude：`duration_between_ms(pending_request_start, completion)`（`claudecode.rs:689-690`、`:881-884`）。Codex 同构（`codex.rs:617-620`）。**仅 >0 才有值**（`(duration > 0).then_some(duration)`）。UsageMesh 用 `duration_ms.filter(\|v\| *v >= 0)` 再过滤一次（`collector.rs:258`）。 |
| `message_count` | `i32`，默认 **1**（`sessions/mod.rs:111-113` `const fn default_message_count() -> i32 { 1 }`，`mod.rs:381` 用于构造）。各解析器可改写为「这一行代表多少次请求」，见 §3.6.6 与 §3.5.13 的横向对照表。 |
| `agent` | `Option<String>`，子代理/角色名。Claude sidechain 从父文件解析（`claudecode.rs:531-536`）；Codex headless 会话置 `"headless"`（`codex.rs:622-626`）。 |

**⚠️ v4.14.0 的 `ParsedMessage` 没有 `cost`、没有 `cost_source`、没有 `dedup_key`、没有 `cache_write_1h`。** 前三个是 `UnifiedMessage` 独有（`sessions/mod.rs:86-88`、`:94`），第四个到 v4.18.0 才加到 `ParsedMessage`（见 §3.8）。

#### 3.2.4 `ParsedMessages`（`lib.rs:359-390`）

【已核实】

```rust
359: pub struct ParsedMessages {
360:     pub messages: Vec<ParsedMessage>,
361:     pub counts: ClientCounts,
362:     pub processing_time_ms: u32,
363: }
```

- `counts: ClientCounts`（`clients.rs:1009-1034`）：`counts: [i32; ClientId::COUNT]` 定长数组，`get/set/add`（`clients.rs:1020-1031`）。
- **`counts[c]` 与 `messages` 中 `client == c` 的元素个数不一定相等**，因为：
  - 部分客户端用 `should_keep_deduped_message` 先过滤再计数（如 Codex `lib.rs:4464-4471`）；
  - 部分客户端用 `summed_parsed_message_count`（`lib.rs:5385-5390`，把 `message_count.max(0)` 累加）计数，与元素条数不同（如 Reasonix `lib.rs:4685-4688`、Zcode `lib.rs:4806`）；
  - Devin Desktop 用**去重前**计数（`lib.rs:5142-5147`、`:5165` 注释：“Count before dedup so the `clients` command reflects how many Desktop sessions were actually found”）；
  - Kiro 是两个 lane 相加（`lib.rs:5017-5018` 与 `:5027` `counts.add(...)`）。
- `processing_time_ms` 是整段 `parse_local_clients` 的墙钟（`lib.rs:4335` `Instant::now()`，`:5280` `start.elapsed().as_millis() as u32`）。
- `Clone` 是手写的（`lib.rs:365-378`），因为 `ClientCounts` 没派生 `Clone`；`Debug` 也是手写的（`lib.rs:380-390`），逐客户端打印计数。

#### 3.2.5 `UnifiedMessage`（通道 B 的产出，`sessions/mod.rs:75-109`）

【已核实】UsageMesh **不直接消费**它，但理解它有助于解释「为什么 `ParsedMessage` 少了什么」：

```rust
 75: #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
 76: pub struct UnifiedMessage {
 77:     pub client: String,
 78:     pub model_id: String,
 79:     pub provider_id: String,
 80:     pub session_id: String,
 81:     pub workspace_key: Option<String>,
 82:     pub workspace_label: Option<String>,
 83:     pub timestamp: i64,
 84:     pub date: String,
 85:     pub tokens: TokenBreakdown,
 86:     pub cost: f64,
 87:     #[serde(default)]
 88:     pub cost_source: CostSource,
 89:     #[serde(default)]
 90:     pub duration_ms: Option<i64>,
 91:     #[serde(default = "default_message_count")]
 92:     pub message_count: i32,
 93:     pub agent: Option<String>,
 94:     pub dedup_key: Option<String>,
 95:     /// Human-readable session title/name ...
100:     pub session_title: Option<String>,
104:     pub is_turn_start: bool,
108:     pub model_attribution_conflicted: bool,
109: }
```

- `dedup_key: Option<String>`（`mod.rs:94`）：去重键，解析器自定格式，见 §3.6.5。
- `is_turn_start`（`mod.rs:104`）：本条是否为「用户轮次后的第一条 assistant 回复」。Claude 用 `pending_turn_start`（`claudecode.rs:494`、`:717-720`），Codex 用 `state.pending_turn_start`（`codex.rs:504`、`:648-651`）。**`parse_local_clients` 会把 `is_turn_start` 丢掉**（`unified_to_parsed` `lib.rs:5357-5376` 不拷贝该字段），所以 UsageMesh 拿不到轮次信息。
- `session_title`（`mod.rs:100`）、`model_attribution_conflicted`（`mod.rs:108`）同样在 `unified_to_parsed` 里被丢掉。
- `cost_source: CostSource`（`mod.rs:66-73`）：

```rust
 66: #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
 67: #[serde(rename_all = "camelCase")]
 68: pub enum CostSource {
 69:     #[default]
 70:     Unknown,
 71:     ProviderReported,
 72:     Estimated,
 73: }
```

  状态迁移只有一个入口：`mark_provider_reported_cost()` `mod.rs:437-439`、`mark_estimated_cost()` `mod.rs:441-443`、`has_authoritative_cost()` `mod.rs:445-447`（等价于 `== ProviderReported`）。

- `unified_to_parsed()`（`lib.rs:5357-5376`）是两条通道的桥：

```rust
5357: fn unified_to_parsed(msg: &UnifiedMessage) -> ParsedMessage {
5358:     ParsedMessage {
5359:         client: msg.client.clone(),
5360:         model_id: msg.model_id.clone(),
5361:         provider_id: msg.provider_id.clone(),
5362:         session_id: msg.session_id.clone(),
5363:         workspace_key: msg.workspace_key.clone(),
5364:         workspace_label: msg.workspace_label.clone(),
5365:         timestamp: msg.timestamp,
5366:         date: msg.date.clone(),
5367:         input: msg.tokens.input,
5368:         output: msg.tokens.output,
5369:         cache_read: msg.tokens.cache_read,
5370:         cache_write: msg.tokens.cache_write,
5371:         reasoning: msg.tokens.reasoning,
5372:         duration_ms: msg.duration_ms,
5373:         message_count: msg.message_count,
5374:         agent: msg.agent.clone(),
5375:     }
5376: }
```

  即：`cost`、`cost_source`、`dedup_key`、`session_title`、`is_turn_start`、`model_attribution_conflicted` **全部丢弃**。

反向桥 `parsed_to_unified(msg, cost)`（`lib.rs:5413-5440`）用于「把已经解析过的 `ParsedMessage` 拿去计价」：它塞 `cost_source: CostSource::Unknown`（`lib.rs:5431`）、`dedup_key: None`（`lib.rs:5435`）。这个函数只被聚合/报告路径用，`parse_local_clients` 内部不调用。

#### 3.2.6 `ClientId`（`clients.rs:322-372`，由宏生成）

【已核实】`define_clients!` 宏（`clients.rs:320-401`）生成：

- `pub enum ClientId`，`#[repr(usize)]`，每个变体绑定显式索引（`clients.rs:322-326`）；
- `pub const COUNT: usize`（`clients.rs:329`）、`pub const ALL: [ClientId; COUNT]`（`clients.rs:330`）；
- `data()` 返回 `&CLIENTS[*self as usize]`（`clients.rs:332-334`）；
- **`as_str()` = `self.data().id`，即注册表里 `id:` 字面量**（`clients.rs:336-338`）；
- `iter()`（`clients.rs:364-366`）、`from_str(s)` 用 `as_str()` 精确匹配（`clients.rs:368-371`，注意是 `#[allow(clippy::should_implement_trait)]` 的固有方法，不是 `std::str::FromStr`）；
- 编译期断言索引连续（`clients.rs:392-399` `assert!($index == i, "ClientId indices must be sequential")`）。

`as_str()` 的取值按下表（**50 个**，`clients.rs:403-1007`）。

#### 3.2.7 `canonical_model_id`（`lib.rs:87-89`）

【已核实】实现是 `normalize_syntactic` 的直通：

```rust
 87: pub fn canonical_model_id(model_id: &str) -> String {
 88:     normalize_syntactic(model_id)
 89: }
```

`normalize_syntactic`（`lib.rs:129-167`）的规则，逐步：

1. `to_lowercase()`（`lib.rs:130`）；
2. 剥掉 CLIProxyAPI 风格的 `(reasoning-tier)` 后缀，tier 白名单 `minimal|low|medium|high|xhigh|auto|none`（`lib.rs:57-73` + `:132-134`）；
3. 剥掉尾部 `-YYYYMMDD`：要求去掉末 8 个数位后前一字节是 `-`，且总长 > 9（`lib.rs:135-142`）；
4. 若名字含 `claude`：把「数字 `.` 数字」里的 `.` 改写成 `-`（`lib.rs:144-160`），例如 `claude-opus-4.1` → `claude-opus-4-1`；
5. `anthropic/claude-<major>-<minor>-<family>` 折成 `claude-<family>-<major>-<minor>`，family 只认 `opus|sonnet|haiku`，且后面不能再有段（`lib.rs:162-164` + `:169-184`）。

**关键语义：`canonical_model_id` 不做别名折叠（alias fold）**。文档字符串（`lib.rs:75-86`）明确：「never consults the user's machine-local `modelAliases`」，因为它用于「离开本机的模型身份」。带别名折叠的是 `normalize_model_for_grouping`（`lib.rs:100-102`，= `model_alias::global().apply(normalize_syntactic(...))`），那是**仅本地展示**用的，`parse_local_clients` 不调用它。

UsageMesh 在 `collector.rs:238` / `:286` 主动调用 `canonical_model_id`，即它选择的是「不带别名的结构归一化」这一档。

#### 3.2.8 `ScannerSettings`（`scanner.rs:38-82`）

【已核实】

```rust
 38: #[derive(Debug, Clone, Default, Serialize, Deserialize)]
 39: #[serde(default, rename_all = "camelCase")]
 40: pub struct ScannerSettings {
 53:     #[serde(default)]
 54:     pub opencode_db_paths: Vec<PathBuf>,
 59:     #[serde(default)]
 60:     pub extra_scan_paths: BTreeMap<String, Vec<PathBuf>>,
 80:     #[serde(default)]
 81:     pub bucket_timezone: Option<String>,
 82: }
```

| 字段 | 语义 | 默认值 |
|---|---|---|
| `opencode_db_paths` | 额外的 OpenCode SQLite 绝对路径，合并进自动发现的 `ScanResult::opencode_dbs`，按 canonical path 去重、不存在的静默跳过、拒绝 WAL/SHM 旁文件（`scanner.rs:41-54`） | `vec![]` |
| `extra_scan_paths` | 按**公开 client id**（`codex`/`gemini`/`openclaw`…）为键的额外扫描根（`scanner.rs:55-60`） | `{}` |
| `bucket_timezone` | **本机把用量归到哪一天的 IANA 时区名**，如 `"Asia/Seoul"`（`scanner.rs:61-81`） | `None` = 未钉 |

`#[serde(default)]` 在**结构体和字段两级**都有（`scanner.rs:38-39` + 每个字段），保证旧 settings.json（无 `scanner` 键）反序列化不报错（`scanner.rs:35-37` 注释）。

##### `BucketTimezone`（`bucket_tz.rs:36-128`）

【已核实】

- 枚举两态：`Local`（默认，day key 跟 `chrono::Local`，每次扫描重读）与 `Pinned(chrono_tz::Tz)`（`bucket_tz.rs:35-45`）。
- `from_pinned_name(Option<&str>)`：空/未设/解析失败 → `Local`；失败会 `tracing::warn!`（`bucket_tz.rs:54-70`）。
- `from_scanner_settings(settings)` = `from_pinned_name(settings.bucket_timezone.as_deref())`（`bucket_tz.rs:73-75`）。
- `is_pinned()`（`bucket_tz.rs:87-89`）。
- `day_key(timestamp_ms) -> String`，格式 `%Y-%m-%d`（`bucket_tz.rs:108-113` + `:137-146`）；无法表示时返回**空串**。
- `hour_key(timestamp_ms) -> Option<String>`，格式 `%Y-%m-%d %H:00`（`bucket_tz.rs:122-127` + `:150-159`）。
- `today()` 按同一时区取当天（`bucket_tz.rs:99-105`），注释解释它是为了 `--today/--week/--month` 与桶键一致。

**默认值链**：UsageMesh 若没读到 settings.json 就传 `ScannerSettings::default()`（`collector.rs:79-84`）→ `bucket_timezone = None` → `BucketTimezone::Local` → **不开钉，`rebucket_parsed_days` 直接 return**（`lib.rs:5291-5293`），`date` 就是解析器构造时按 `chrono::Local` 算的那个值。这意味着 UsageMesh 的「日」边界**跟随采集当时机器的本地时区**，且同一批历史数据在不同时区机器上重扫会得到不同的 `date`（这正是 `bucket_tz.rs:1-28` 模块注释描述的、钉时区要解决的 bug）。

---

### 3.3 客户端注册表

#### 3.3.1 `define_clients!` 宏的九列（`clients.rs:320-401`）

【已核实】`clients.rs:321` 的宏臂列出每个客户端 9 个字段：

```
$variant = $index => { id, display, logo, root, relative, pattern, headless, parse_local, submit_default }
```

映射到 `ClientDef`（`clients.rs:284-293`）与访问器：

| 宏列 | `ClientDef` 字段 | 访问器 | 含义 |
|---|---|---|---|
| `id` | `id: &'static str` | `as_str()` `clients.rs:336-338` | **对外标识符**，也是 `LocalParseOptions.clients` 里要写的值 |
| `display` | —（进 `CLIENT_DISPLAY_NAMES`） | `display_name()` `clients.rs:340-342` | 人类可读名 |
| `logo` | —（进 `CLIENT_LOGO_URLS`） | `logo_url()` `clients.rs:344-346` | 图标 URL，可为 `None` |
| `root` | `root: PathRoot` | — | 根目录基（`Home`/`ReasonixHome`/`XdgData`/`Config`/`AppData`/`EnvVar{var,fallback_relative}`，`clients.rs:2-18`） |
| `relative` | `relative_path` | — | 根之下的相对路径 |
| `pattern` | `pattern` | `file_pattern()` `clients.rs:348-350` | 扫描用的文件名模式（会与 `scan_directory` 的分支匹配，`scanner.rs:406-500`） |
| `headless` | `headless` | `supports_headless()` `clients.rs:352-354` | 是否支持 headless 采集 |
| `parse_local` | `parse_local` | `parse_local()` `clients.rs:356-358` | **是否参与 `parse_local_clients` 的默认客户端集合** |
| `submit_default` | `submit_default` | `submit_default()` `clients.rs:360-362` | 是否默认提交到远端（对 UsageMesh 无影响） |

路径解析：`ClientDef::resolve_path_with_env_strategy(home, use_env_roots)`（`clients.rs:296-313`）= `root.resolve_with_env_strategy()` + **逐组件 `push`** 拼接（`clients.rs:308-312`）。注释解释了为什么不能直接 `format!("{root}/{relative}")`：`Path::join` 只规范化连接处，相对段的 `/` 会漏到 Windows 上产生混合分隔符（issue #1048）。

`PathRoot::resolve_with_env_strategy` 各分支（`clients.rs:98-209`）：
- `Home` → `home_dir` 原样（`clients.rs:100`）；
- `ReasonixHome` → env 优先 `REASONIX_STATE_HOME` 再 `REASONIX_HOME`（`clients.rs:101-111`，含 `${VAR:-default}` 与 `~` 展开 `clients.rs:216-282`）；
- `XdgData` → `XDG_DATA_HOME`（`use_env_roots` 时）否则 `<home>/.local/share`（`clients.rs:120-…`）；
- `Config` → `XDG_CONFIG_HOME` 或 <code>dirs::config_dir()</code>（`clients.rs:185-195`）；
- `AppData` → `dirs::config_dir()`，但在 Windows 上当 `home_dir` 与真实 profile 不同（`app_data_follows_home` `clients.rs:54-71`）时改为从 `home_dir` 派生（`clients.rs:196-203`）；
- `EnvVar{var,fallback_relative}` → env 有值时取 env，否则 `join_home(home, fallback_relative)`（`clients.rs:192-207`）。

#### 3.3.2 完整客户端清单（v4.14.0，`clients.rs:403-1007`）

【已核实】共 50 个（索引 0–49 连续，编译期断言保证，`clients.rs:392-399`）。

| # | `as_str()` | display | root | relative | pattern | headless | parse_local | submit_default |
|---|---|---|---|---|---|---|---|---|
| 0 | `opencode` | OpenCode | XdgData | `opencode/storage/message` | `*.json` | false | true | true |
| 1 | `claude` | Claude Code | EnvVar `CLAUDE_CONFIG_DIR` / `.claude` | `projects` | `*.jsonl` | false | true | true |
| 2 | `codex` | Codex CLI | EnvVar `CODEX_HOME` / `.codex` | `sessions` | `*.jsonl` | **true** | true | true |
| 3 | `cursor` | Cursor IDE | Home | `.config/tokscale/cursor-cache` | `usage*.csv` | false | **false** | true |
| 4 | `gemini` | Gemini CLI | EnvVar `GEMINI_CLI_HOME` / `.gemini` | `tmp` | `*.json\|*.jsonl` | false | true | true |
| 5 | `amp` | Amp | XdgData | `amp/threads` | `T-*.json` | false | true | true |
| 6 | `droid` | Droid | Home | `.factory/sessions` | `*.settings.json` | false | true | true |
| 7 | `openclaw` | OpenClaw | Home | `.openclaw/agents` | `*.jsonl*` | false | true | true |
| 8 | `pi` | Pi | Home | `.pi/agent/sessions` | `*.jsonl` | false | true | true |
| 9 | `kimi` | Kimi CLI | Home | `.kimi/sessions` | `wire.jsonl` | false | true | true |
| 10 | `qwen` | Qwen CLI | Home | `.qwen/projects` | `*.jsonl` | false | true | true |
| 11 | `roocode` | Roo Code | Home | `.config/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks` | `ui_messages.json` | false | true | true |
| 12 | `kilocode` | Kilo Code | Home | `.config/Code/User/globalStorage/kilocode.kilo-code/tasks` | `ui_messages.json` | false | true | true |
| 13 | `mux` | Mux | Home | `.mux/sessions` | `session-usage.json` | false | true | true |
| 14 | `kilo` | Kilo CLI | XdgData | `kilo/kilo.db` | `kilo.db` | false | true | true |
| 15 | `crush` | Crush | XdgData | `crush/projects.json` | `projects.json` | false | true | **false** |
| 16 | `hermes` | Hermes Agent | EnvVar `HERMES_HOME` / `.hermes` | `state.db` | `state.db` | false | true | true |
| 17 | `copilot` | Copilot CLI | Home | `.copilot/otel` | `*.jsonl` | false | true | true |
| 18 | `goose` | Goose | XdgData | `goose/sessions/sessions.db` | `sessions.db` | false | true | true |
| 19 | `codebuff` | Codebuff | EnvVar `CODEBUFF_DATA_DIR` / `.config/manicode` | `projects` | `chat-messages.json` | false | true | true |
| 20 | `antigravity` | Antigravity | Config | `antigravity-cache/sessions` | `*.jsonl` | false | true | true |
| 21 | `zed` | Zed Agent | XdgData | `zed/threads/threads.db` | `threads.db` | false | true | true |
| 22 | `kiro` | Kiro | Home | `.kiro/sessions/cli` | `*.json` | false | true | true |
| 23 | `trae` | Trae | Config | `trae-cache/sessions` | `*.json` | false | true | **false** |
| 24 | `warp` | Warp | Config | `warp-cache` | `usage*.json` | false | true | **false** |
| 25 | `cline` | Cline | Home | `.config/Code/User/globalStorage/saoudrizwan.claude-dev/tasks` | `ui_messages.json` | false | true | true |
| 26 | `gjc` | Gajae-Code | EnvVar `GJC_CODING_AGENT_DIR` / `.gjc/agent` | `sessions` | `*.jsonl` | false | true | true |
| 27 | `grok` | Grok Build | EnvVar `GROK_HOME` / `.grok` | `sessions` | `updates.jsonl` | false | true | true |
| 28 | `jcode` | Jcode | EnvVar `JCODE_HOME` / `.jcode` | `sessions` | `session_*.json` | false | true | true |
| 29 | `commandcode` | Command Code | Home | `.commandcode/projects` | `*.jsonl` | false | true | true |
| 30 | `micode` | MiMo Code | XdgData | `mimocode` | `*.db` | false | true | true |
| 31 | `antigravity-cli` | Antigravity CLI | EnvVar `GEMINI_CLI_HOME` / `.gemini` | `antigravity-cli/conversations` | `*.db` | false | true | true |
| 32 | `junie` | Junie | Home | `.junie/sessions` | `events.jsonl` | false | true | true |
| 33 | `zcode` | ZCode | Home | `.zcode/projects` | `*.jsonl` | false | true | true |
| 34 | `opencodereview` | OpenCodeReview | Home | `.opencodereview/sessions` | `*.jsonl` | false | true | true |
| 35 | `codebuddy` | CodeBuddy | Home | `.codebuddy/projects` | `*.jsonl` | false | true | true |
| 36 | `workbuddy` | WorkBuddy | Home | `.workbuddy` | `workbuddy.db` | false | true | true |
| 37 | `devin-cli` | Devin CLI | XdgData | `devin/cli/sessions.db` | `sessions.db` | false | true | true |
| 38 | `devin-desktop` | Devin Desktop | Home | `Library/Application Support/Devin/User/acp-events` | `*.ndjson` | false | true | true |
| 39 | `senpi` | Senpi (OmO Native) | EnvVar `SENPI_CODING_AGENT_DIR` / `.senpi/agent` | `sessions` | `*.jsonl` | false | true | true |
| 40 | `augment` | Augment Code | Home | `.augment/sessions` | `*.json` | false | true | true |
| 41 | `kimchi` | Kimchi | EnvVar `KIMCHI_CODING_AGENT_DIR` / `.config/kimchi/harness` | `sessions` | `*.jsonl` | false | true | true |
| 42 | `reasonix` | Reasonix | ReasonixHome | `stats` | `*.jsonl` | false | true | true |
| 43 | `prime-agent` | Prime Agent | EnvVar `PRIME_AGENT_CODING_AGENT_DIR` / `.prime/agent` | `sessions` | `*.jsonl` | false | true | true |
| 44 | `freebuff` | Freebuff | EnvVar `FREE BUFF_DATA_DIR`→ 实际 `FREEBUFF_DATA_DIR` / `.config/manicode` | `projects` | `chat-messages.json` | false | true | true |
| 45 | `cherrystudio` | Cherry Studio | **AppData** | `CherryStudio/.claude/projects` | `*.jsonl` | false | true | true |
| 46 | `dsh` | DeepSeek Harness | EnvVar `DSH_HOME` / `.dsh` | `sessions` | `dsh-session-log` | false | true | true |
| 47 | `mcode` | MiniMax Code | Config | `headless/mcode` | `*.jsonl` | **true** | true | true |
| 48 | `fx` | Fx | Home | `.fx/sessions` | `usage-v2.json` | false | true | true |
| 49 | `omp` | Oh My Pi | Home | `.omp/agent/sessions` | `*.jsonl` | false | true | true |

（第 44 行的 env 名为 `FREEBUFF_DATA_DIR`，见 `clients.rs:909-912`。）

**只有 `cursor` 一个 `parse_local: false`（`clients.rs:440-449`）。** 后果：默认客户端集合（`lib.rs:4340-4343` 过滤 `parse_local()`）**不含 cursor**；`parse_local_clients` 里也**没有任何 cursor 分派分支**，所以它的 `usage*.csv` 永远不会进入 `ParsedMessage` 流。这与工作区已存在的 v4.18.0 文档若声称「cursor 被扫描」需按版本区分——v4.18.0 仍是 `parse_local: false`（`git diff` 只改了 `pattern` 为 `usage*.json|usage*.csv`）。

`headless: true` 只有两个：`codex`（`clients.rs:436`）与 `mcode`（`clients.rs:970`）。这个标志影响 `ScanResult` 里 headless 目录的归属与 `apply_headless_agent`；UsageMesh 侧只间接体现在 `agent` 字段。

`submit_default: false` 三个：`crush`（`clients.rs:571`）、`trae`（`clients.rs:657`）、`warp`（`clients.rs:667`）——对 UsageMesh 无影响（UsageMesh 从不提交）。

#### 3.3.3 `ClientCounts`（`clients.rs:1009-1034`）

【已核实】

```rust
1009: pub struct ClientCounts {
1010:     counts: [i32; ClientId::COUNT],
1011: }
1012:
1013: impl ClientCounts {
1014:     pub fn new() -> Self { Self { counts: [0; ClientId::COUNT] } }
1020:     pub fn get(&self, client: ClientId) -> i32 { self.counts[client as usize] }
1024:     pub fn set(&mut self, client: ClientId, value: i32) { self.counts[client as usize] = value; }
1028:     pub fn add(&mut self, client: ClientId, value: i32) { self.counts[client as usize] += value; }
1033: impl Default for ClientCounts { fn default() -> Self { Self::new() } }
```

注意 `add` 是**非饱和加法**（`+=`，`clients.rs:1030`），只有 Kiro 用（`lib.rs:5027`）；其余用 `set` 覆盖。

#### 3.3.4 注册表里有条目、但通道 A 不产出的客户端（v4.14.0 的重要陷阱）

【已核实】`sessions/mod.rs:5-59` 声明的解析器模块共 **55 个**（53 个 `pub mod` + 2 个 `pub(crate) mod`：`tencent_buddy` `sessions/mod.rs:53`、`utils` `sessions/mod.rs:55`；含 `opencode_schema`、`copilot_*` 等辅助），注册表条目 **50 项**（`clients.rs:403-1007`）。**先纠一条易错点：`MiMoCode = 30` 确实在 `ClientId` 里（`clients.rs:728-737`，`id: "micode"`「MiMo Code」）——变体名大小写敏感，`grep "Micode"` 在源码下无命中，极易误判为「枚举里没有」；`TencentBuddy` 则确非 `ClientId`（`tencent_buddy` 只是各 Cline/CodeBuddy 系共用的 `pub(crate)` 模块）。** 对应事实：

- **`sessions::micode`（MiMo Code SQLite）不在 `parse_local_clients` 里**。`scan_result.micode_dbs` 被扫描填充（`scanner.rs:113-114`、`:837`、`:893`），但只有**通道 B** 消费：`lib.rs:1694-1736`（注释 “Parse MiMo Code: SQLite database(s)”）。通道 A 分派区段（`lib.rs:4367-5246`）完全没有 micode 分支。⇒ **UsageMesh 完全读不到 MiMo Code 的用量。**
- **`sessions::fx`（Fx）同样只在通道 B 被调用**。`ClientId::Fx` / `sessions::fx` 在 `lib.rs:4367-5246` 内**零命中**，只出现在通道 B：`lib.rs:2510` `let fx_paths = scan_result.get(ClientId::Fx);`、`:2520` `sessions::fx::parse_fx_file,`、`:2531` `sessions::fx::apply_session_titles(&mut fx_lane);`。而注册表里它明明是 `Fx = 48 => { id: "fx", …, parse_local: true, … }`（`clients.rs:979-989`）——**`parse_local: true` 却不被通道 A 分派**。⇒ **UsageMesh 完全读不到 Fx 的用量。**
- `sessions::cursor`（`sessions/cursor.rs:157`）同样只在通道 B（`lib.rs:1925-1926` `ClientId::Cursor` + `sessions::cursor::parse_cursor_file`）被调用。
- `sessions::tencent_buddy` 是 `pub(crate)`（`sessions/mod.rs:53`），只在 Cline/CodeBuddy 系内部被调用。
- **`synthetic` 不是 `ClientId`**：它是一组「网关归一化 + Octofriend SQLite」规则，用字符串 `"synthetic"` 参与过滤（`lib.rs:4344`、`:4348`、`:5237-5246`、`:5256-5263`），实现于 `sessions/synthetic.rs`。`matches_synthetic_filter`（`synthetic.rs:99-101`）= `client == "synthetic"` **或**「模型/供应商看起来是 synthetic 网关」（`is_synthetic_gateway` `synthetic.rs:50-52`）。

---

### 3.4 parse_local_clients 主链路

#### 3.4.1 完整调用序列（【已核实】，行号紧跟）

**Step 0 — 计时与 home 解析**（`lib.rs:4334-4337`）

```rust
4334: pub fn parse_local_clients(options: LocalParseOptions) -> Result<ParsedMessages, String> {
4335:     let start = Instant::now();
4336:
4337:     let home_dir = get_home_dir_string(&options.home_dir)?;
```

**Step 1 — 客户端清单与「包含开关」**（`lib.rs:4339-4356`）

```rust
4339:     let clients: Vec<String> = options.clients.clone().unwrap_or_else(|| {
4340:         let mut clients: Vec<String> = ClientId::iter()
4341:             .filter(|c| c.parse_local())
4342:             .map(|c| c.as_str().to_string())
4343:             .collect();
4344:         clients.push("synthetic".to_string());
4345:         clients
4346:     });
4347:     let include_all = clients.is_empty();
4348:     let include_synthetic = include_all || clients.iter().any(|c| c == "synthetic");
4349:     let include_devin_cli = include_synthetic || clients.iter().any(|c| c == "devin-cli");
4350:     let include_devin_desktop = include_synthetic || clients.iter().any(|c| c == "devin-desktop");
4355:     let include_codebuff = include_all || clients.iter().any(|c| c == "codebuff");
4356:     let include_freebuff = include_all || clients.iter().any(|c| c == "freebuff");
```

四个「跨客户端共享扫描桶」的开关：
- `devin-cli` / `devin-desktop` 谁被请求谁解析（`:4349-4350`），因为两者在 `scan_result` 里的关系见 §3.4.4；
- `codebuff` / `freebuff` 共享 `manicode` 扫描桶，各自只在被显式请求时解析（`:4351-4356` 注释：“Each product parses and counts only when it was actually requested, so a codebuff-only filter cannot pick up estimated Freebuff rows and vice versa.”）。

**Step 2 — 文件发现**（`lib.rs:4358-4365`）

```rust
4358:     let scan_result = scanner::scan_all_clients_with_scanner_settings(
4359:         &home_dir,
4360:         &clients,
4361:         options.use_env_roots,
4362:         &options.scanner_settings,
4363:     );
4364:     let headless_roots =
4365:         scanner::headless_roots_with_env_strategy(&home_dir, options.use_env_roots);
```

`headless_roots`（`scanner.rs:333-353`）= `TOKSCALE_HEADLESS_DIR`（仅 `use_env_roots`）否则 `<home>/.config/tokscale/headless` + `<home>/Library/Application Support/tokscale/headless` 两条候选。

**Step 3 — 逐客户端分派**（`lib.rs:4367-5246`）

口径：该区段内共出现 **47 个不同的 `ClientId::*`**（注册表 50 个中缺 `cursor`、`micode`、`fx` 三者），外加末尾 1 个 `synthetic` 归一化段（`lib.rs:5237-5246`）。下表 47 行中，第 42 行（Devin）一段覆盖 `devin-cli` 与 `devin-desktop` 两个 ClientId。

顺序（严格按源码）：

| 顺序 | 客户端/lane | 行号 | 特殊处理 |
|---|---|---|---|
| 1 | OpenCode（SQLite 优先，再 JSON 补，跨 db 去重） | `4373-4418` | 手写：`seen` 集合按 `dedup_key` 跨 `opencode.db` 与 `opencode-<channel>.db` 去重（`:4382-4392`），JSON 仅补 SQLite 没有的键（`:4408-4412`） |
| 2 | Claude（`sessions/claudecode.rs`） | `4420-4448` | `map_init(HashMap::new, ...)` 给每个 rayon 线程一个父会话缓存（`:4424`），再用 `seen_keys` 先到先得去重（`:4440-4445`） |
| 3 | Codex | `4450-4472` | 计算 `is_headless`（`:4454`）并对每条调 `apply_headless_agent`（`:4458`），再用 `should_keep_deduped_message` 去重（`:4467`） |
| 4 | Copilot（OTel JSONL + Desktop SQLite + VS Code chatSessions 三源合并） | `4474-4524` | Desktop 按 `session_id` 排除已被 OTel 覆盖的（`:4484-4492`），VS Code 再按 `dedup_key` ∪ `(session_id, timestamp)` 双条件排（`:4495-4518`） |
| 5 | Gemini | `4526-4538` | — |
| 6 | Amp | `4540-4552` | — |
| 7 | Codebuff（受 `include_codebuff` 门控） | `4554-4570` | 复用 `ClientId::Codebuff` 的扫描桶 |
| 8 | Freebuff（受 `include_freebuff` 门控，**同一桶**） | `4572-4590` | 注释：“Freebuff shares the manicode scan; the estimated parser runs over the same file set” |
| 9 | Droid | `4592-4604` | — |
| 10 | OpenClaw | `4606-4618` | 调 `parse_openclaw_transcript` |
| 11 | Pi | `4620-4632` | — |
| 12 | Prime Agent | `4634-4658` | 先 `parse_prime_agent_file_with_accounting`，再**跨文件** `reconcile_prime_agent_messages`（`:4648-4655`） |
| 13 | Kimchi | `4660-4673` | `should_keep_deduped_message` |
| 14 | Reasonix | `4675-4689` | 计数用 `message_count` 累加（`:4685-4687`） |
| 15 | Senpi | `4691-4703` | — |
| 16 | Omp | `4705-4717` | — |
| 17 | Augment | `4719-4732` | `should_keep_deduped_message` |
| 18 | CommandCode | `4734-4746` | — |
| 19 | Gjc | `4754-4767` | `should_keep_deduped_message`；注释明确“This non-cached path produces ParsedMessage (no cost field) and has no pricing service in scope, so the A1 cost guard is a no-op here”（`:4748-4753`） |
| 20 | Junie | `4772-4785` | `should_keep_deduped_message`；注释：“ParsedMessage has no pricing service in scope, but Junie parser already preserves the embedded session costs”（`:4769-4771`——即 cost 在 `UnifiedMessage` 里，转换时被丢） |
| 21 | ZCode（SQLite v2 + 遗留 JSONL） | `4787-4808` | 两条来源合并 |
| 22 | Cherry Studio | `4810-4823` | — |
| 23 | Dsh | `4827-4840` | `should_keep_deduped_message`（注释解释 fork 会重放父行，`:4825-4826`） |
| 24 | Mcode | `4842-4850` | — |
| 25 | OpenCodeReview | `4852-4864` | — |
| 26 | Kimi（双解析器分支） | `4866-4883` | `is_kimi_code_path(path)` 决定 `parse_kimi_code_file` 还是 `parse_kimi_file`（`:4871-4875`） |
| 27 | Qwen | `4885-4898` | — |
| 28 | RooCode | `4900-4912` | — |
| 29 | KiloCode | `4914-4926` | — |
| 30 | Cline | `4928-4941` | `should_keep_deduped_message` |
| 31 | Mux | `4943-4955` | — |
| 32 | Kilo（SQLite） | `4957-4969` | 变量名 `_kilo_count`（用下划线前缀） |
| 33 | Hermes（多 profile DB） | `4971-4983` | `scan_result.hermes_db_paths()`（`scanner.rs:181-…`）给出默认 + extra 的去重集合；`should_keep_deduped_message` |
| 34 | Goose（SQLite） | `4985-4993` | — |
| 35 | Zed（SQLite） | `4995-5005` | `zed_db_paths()` |
| 36 | Kiro（JSON 一批 + SQLite 一批，计数相加） | `5007-5029` | `suppress_snapshots_covered_by_executions`（`:5012-5016`）；SQLite 用 `counts.add`（`:5027`） |
| 37 | Crush（多 DB + workspace 归属） | `5031-5049` | 用 `BucketTimezone::from_scanner_settings(options.scanner_settings)`（`:5032-5033`）并把 `CrushDbSource` 的 workspace 打到消息上（`:5041`） |
| 38 | Antigravity | `5051-5063` | — |
| 39 | Antigravity CLI | `5065-5077` | — |
| 40 | Trae | `5079-5094` | `dedupe_latest_trae_messages`（`:5080-5086`） |
| 41 | Warp | `5096-5108` | — |
| 42 | Devin（CLI + Desktop） | `5110-5169` | 见 §3.4.4 |
| 43 | CodeBuddy | `5171-5189` | 内联 `dedup_key` 过滤（`:5179-5184`，与 `should_keep_deduped_message` 语义相同） |
| 44 | WorkBuddy | `5190-5207` | `partition_workbuddy_paths` → 两批解析 → `merge_workbuddy_messages` |
| 45 | Grok | `5209-5220` | `prefer_unified_log_messages`（`:5214`） |
| 46 | Jcode | `5222-5235` | `should_keep_deduped_message` |
| 47 | Synthetic（Octofriend SQLite） | `5237-5246` | 受 `include_synthetic` 门控 |

**Step 4 — 客户端过滤（在归一化之前）**（`lib.rs:5248-5254`）

```rust
5248:     // Filter BEFORE normalization (see parse_all_messages_with_pricing).
5249:     if !include_all {
5250:         let requested: HashSet<&str> = clients.iter().map(String::as_str).collect();
5251:         messages.retain(|msg| {
5252:             retain_for_requested_clients(&msg.client, &msg.model_id, &msg.provider_id, &requested)
5253:         });
5254:     }
```

`retain_for_requested_clients`（`lib.rs:186-201`）三条等价规则：

```rust
192:     requested.contains(client)
193:         || (requested.contains("claude") && client.starts_with("cc-mirror/"))
198:         || (requested.contains("gjc") && client.eq_ignore_ascii_case("9router"))
199:         || (requested.contains("synthetic")
200:             && sessions::synthetic::matches_synthetic_filter(client, model_id, provider_id))
```

- 请求 `claude` 时**保留** `cc-mirror/*`（`:193`）；
- 请求 `gjc` 时**保留** `9router`（`gjc` 是超集，反向不成立，注释 `:195-197`）；
- 请求 `synthetic` 时按网关特征保留（另外 `synthetic` 不在注册表里，这个分支是它唯一能进来的方式之一）。

**Step 5 — synthetic 网关归一化**（`lib.rs:5256-5263`）

```rust
5256:     if include_synthetic {
5257:         for msg in &mut messages {
5258:             sessions::synthetic::normalize_synthetic_gateway_fields(
5259:                 &mut msg.model_id,
5260:                 &mut msg.provider_id,
5261:             );
5262:         }
5263:     }
```

即默认走 `clients: None` 时**会**归一化：模型名去掉 `hf:` / `accounts/<provider>/models/` 前缀（`synthetic.rs:61-82`），provider 为空或 `unknown` 时写成 `"synthetic"`（`synthetic.rs:91-93`）。注意它改变的是 `model_id` / `provider_id` 本身（不是另存字段）。

**Step 6 — 按钉时区重算 date**（`lib.rs:5265-5273`）

```rust
5265:     // Before the date filter, not after: `--since`/`--until` compare against
5266:     // `date`, so filtering first would select rows by the machine's live zone
5267:     // and then relabel them with the pinned one.
5273:     rebucket_parsed_days(&mut messages, &options.scanner_settings);
```

`rebucket_parsed_days`（`lib.rs:5286-5306`）：

```rust
5286: fn rebucket_parsed_days(
5287:     messages: &mut [ParsedMessage],
5288:     scanner_settings: &scanner::ScannerSettings,
5289: ) {
5290:     let timezone = bucket_tz::BucketTimezone::from_scanner_settings(scanner_settings);
5291:     if !timezone.is_pinned() {
5292:         return;
5293:     }
5295:     for message in messages.iter_mut() {
5298:         if message.timestamp <= 0 {
5299:             continue;
5300:         }
5301:         let key = timezone.day_key(message.timestamp);
5302:         if !key.is_empty() {
5303:             message.date = key;
5304:         }
5305:     }
5306: }
```

两点【已核实】：
- `message.timestamp <= 0` 是「无可用时间」哨兵，被跳过（与 `UnifiedMessage::rebucket_date` `sessions/mod.rs:411-420` 的注释一致）；
- 空 key（时区无法表示该瞬时）保留旧 `date` 而非写成空串（对比 `sessions/mod.rs:423-429` 的注释解释）。

**Step 7 — 时间范围过滤**（`lib.rs:5275`）+ 组装（`lib.rs:5277-5281`）

```rust
5392: fn filter_parsed_messages(
5393:     messages: Vec<ParsedMessage>,
5394:     options: &LocalParseOptions,
5395: ) -> Vec<ParsedMessage> {
5396:     let mut filtered = messages;
5397:
5398:     if let Some(year) = &options.year {
5399:         let year_prefix = format!("{}-", year);
5400:         filtered.retain(|m| m.date.starts_with(&year_prefix));
5401:     }
5402:
5403:     if let Some(since) = &options.since {
5404:         filtered.retain(|m| m.date.as_str() >= since.as_str());
5405:     }
5406:
5407:     if let Some(until) = &options.until {
5408:         filtered.retain(|m| m.date.as_str() <= until.as_str());
5409:     }
5410:     filtered
5411: }
```

**决定性结论：`since` / `until` 是「本地日期字符串」的字典序比较，不是时间戳比较。**（同构实现在通道 B 的 `filter_messages_for_report` `lib.rs:4159-4178`。）因此：
- `since = "2026-08-01"` 的语义是「`date >= 2026-08-01`」，其中 `date` 是**当时机器时区**（未钉）或**钉住时区**（已钉）下的日键；
- 时区钉定改变 `date`，因此**同一 `since` 在不同时区下筛出的集合不同**；
- 没有任何「宽松边界」——比 `since` 早一天的那条记录确实会被丢掉，不会因为含跨日区间而例外。

#### 3.4.2 与 `parse_all_messages_*` / `parse_local_unified_messages*` 的关系

【已核实】结论：**同一套解析器与去重键，但两条独立实现的主控流。**

| 维度 | 通道 A `parse_local_clients` | 通道 B `parse_all_messages_with_pricing*` |
|---|---|---|
| 定义 | `lib.rs:4334-5282` | `lib.rs:766`（`_with_cache_policy`）/ `:749`（`_with_env_strategy`，加 `#[allow(dead_code)]`）/ `:729`（`parse_all_messages_with_pricing`） |
| 解析器 | 直接 `sessions::xxx::parse_xxx_*` | 通过 `load_or_parse_source*` 包装器 + 缓存 |
| 去重 | 逐 lane 裸 `HashSet` 先到先得（`lib.rs:5378-5383`） | 同上，但对 Claude 有 `retain_observed_messages` `lib.rs:942-966` 与 `merge_claude_cross_file_duplicate` `lib.rs:4242-4258` |
| 计价 | 无（`ParsedMessage` 无 cost 字段） | `apply_pricing_to_messages` `lib.rs:782-790` |
| 时间过滤 | `filter_parsed_messages` `lib.rs:5392` | `filter_messages_for_report` `lib.rs:4159`（**同一套字符串比较**） |
| 钉时区重算 | `rebucket_parsed_days` `lib.rs:5286` | `UnifiedMessage::rebucket_date` `sessions/mod.rs:411`（同一 `BucketTimezone`） |
| 公开异步入口 | 无 | `parse_local_unified_messages` `lib.rs:5343`、`_with_pricing` `lib.rs:5309`（`#[doc(hidden)]`）、`_with_pricing_uncached` `lib.rs:5329`（`#[doc(hidden)]`） |

`resolve_local_parse_request`（`lib.rs:4302-4315`）是通道 B 的客户端清单解析，逻辑与 A 的 `lib.rs:4339-4346` **逐字同构**（唯一差别是 A 内联、B 抽函数）。

#### 3.4.3 文件发现层（`scanner.rs`）

【已核实】

- 公开入口 `scan_all_clients_with_scanner_settings`（`scanner.rs:1385-1392`）→ `scan_all_clients_with_env_strategy_inner`（`scanner.rs:1439`）。
- 客户端集合换算：`include_all = clients.is_empty()`（`scanner.rs:1447`）；`include_synthetic`（`:1448`）；`enabled: HashSet<ClientId>`（`:1450-…`）里有一条特例映射——字符串 `"9Router"` 影射到 `ClientId::Gjc`（`scanner.rs:1456-1459`）。
- `ScanResult`（`scanner.rs:92-123`）除 `files: [Vec<PathBuf>; ClientId::COUNT]` 外，还带一批**用类型槽代替 ClientId 位**的专用槽：
  - `opencode_dbs: Vec<PathBuf>`（`:102`，含 channel 后缀变体，注释引用 opencode 的 `getChannelPath`）
  - `copilot_desktop_db: Option<PathBuf>`（`:103`）、`synthetic_db`（`:104`）、`kilo_db`（`:105`）、`hermes_db`（`:106`）、`goose_db`（`:107`）、`zed_db`（`:108`）、`kiro_db`（`:109`）
  - `crush_dbs: Vec<CrushDbSource>`（`:110`，`CrushDbSource` 带 `db_path/workspace_key/workspace_label`，`:84-89`）
  - `zcode_db`（`:112`）、`micode_dbs`（`:114`）、`opencode_json_dir`（`:116`）、`devin_dbs`（`:119`）、`copilot_vscode_sessions`（`:122`）
- 目录遍历 `scan_directory(root, pattern)`（`scanner.rs:378-…`）：先 `Path::exists()` 短路（`:379-381`），`WalkDir` + 只收普通文件（符号链接再 stat 一次，`:392-396`），然后**手写 pattern 匹配**（`:406-…`），例如 `"*.jsonl*"` 也接 OpenClaw 的归档名 `.jsonl.deleted.` / `.jsonl.reset.`（`:424-430`）、`"usage*.csv"` 排除 `archive` 目录与 `usage.backup*`（`:432-452`）、`"kiro-ide-session"` 要求 `session.json` 且父目录是 `sess_*`（`:485-496`）。
- 额外扫描根：`parse_extra_dirs`（`scanner.rs:551`）、`extra_scan_paths_for`（`:574`）、`built_in_extra_scan_paths_for`（`:616`）。越出 home 的额外路径只 `tracing::warn!` 不阻止（`scanner.rs:15-26`）。
- headless 根：`headless_roots_with_env_strategy`（`scanner.rs:333-353`）。
- Copilot 导出器路径：env `COPILOT_OTEL_FILE_EXPORTER_PATH`（`scanner.rs:359-371`）。

#### 3.4.4 三条「跨客户端协同」的细节

**Devin（`lib.rs:5110-5169`）**

```
5114:     let mut devin_cli_seen = HashSet::new();
5115:     let devin_cli_messages: Vec<UnifiedMessage> = if include_devin_cli { ... } else { Vec::new() };
5125:     let cli_session_ids: HashSet<String> = devin_cli_messages.iter().map(|m| m.session_id.clone()).collect();
5144:     let devin_desktop_raw_count: i32 = devin_desktop_messages_raw.iter().map(|msg| msg.message_count.max(0)).sum();
5148:     let devin_desktop_messages: Vec<UnifiedMessage> = devin_desktop_messages_raw
5149:         .into_iter()
5150:         .filter(|message| !cli_session_ids.contains(&message.session_id))
5151:         .collect();
5165:     let devin_desktop_count = devin_desktop_raw_count;
5167:     counts.set(ClientId::DevinDesktop, devin_desktop_count);
```

- Desktop 消息若其 `session_id` 已出现在 CLI 库中就丢弃（`:5150`）；
- 但**计数**用去重前的 `devin_desktop_raw_count`（`:5165`，注释 `:5142-5143` 与 `:5162-5164`）。
- ⇒ `counts[DevinDesktop]` 可能**大于** `messages` 里 `client == "devin-desktop"` 的条数。这是 `ParsedMessages.counts` 不可作为行数使用的直接反例。

**Copilot 三源（`lib.rs:4474-4519`）**

- OTel JSONL（`sessions::copilot::parse_copilot_file`，`:4478`）
- Desktop SQLite（`sessions::copilot_desktop::parse_copilot_desktop_db`，`:4489`），用 OTel 的 `session_id` 集合排除（`:4484-4492`）
- VS Code `chatSessions`（`sessions::copilot_vscode::parse_copilot_vscode_sessions`，`:4504`），用两个集合双重排重：`existing_dedup_keys`（`:4495-4498`）与 `(session_id, timestamp)` 二元组（`:4499-4502`、`:4514-4516`）

**Kiro 两源（`lib.rs:5007-5029`）**

- JSON 一批先跑 `suppress_snapshots_covered_by_executions`（`:5012-5016`）压掉被 execution 覆盖的 snapshot；
- SQLite 一批独立追加，计数 `counts.add`（`:5027`），所以 Kiro 的计数是两批之和。

---

### 3.5 逐客户端读取实现（分小节）

> 标注规则：每条后面给 `文件:行号`。**覆盖度诚实标注**——
> - 【全文】= 该解析器文件正文（非测试区）被逐行读完；
> - 【精读】= 实际核验范围已列出；
> - 【抽样】= 只读了入口与关键分支。
>
> 覆盖口径：Claude Code 与 Codex CLI 为逐行精读；其余客户端由二级取证代理按统一模板逐行读取源码并给出行号，本节抽查关键行号后收录，凡未经一级抽查的行号在该条末尾标 `【代核实】`。

#### 3.5.1 Claude Code（`claude`）

**模块与入口**【已核实】
- `crates/tokscale-core/src/sessions/claudecode.rs:407-410` `parse_claude_file(path)` → 取 `crate::paths::home_dir()` 后转发；
- `claudecode.rs:412-415` `parse_claude_file_with_home(path, home_dir)` → 新建一个空的 `ParentSubagentTypeCache`（`claudecode.rs:413`）后转发；
- `claudecode.rs:425-769` `parse_claude_file_with_cache_and_home(path, parent_cache, home_dir)` ← **`parse_local_clients` 实际调用的就是这个**（`lib.rs:4425-4429`，home 传的是本次解析的 `home_dir` 而非进程 home）；
- **`ParentSubagentTypeCache` 是逐文件的**：`lib.rs:4424` 用 `map_init(std::collections::HashMap::new, ...)` 给每个 rayon 工作线程一份缓存，注释未解释，但从签名看它跨文件复用（同一线程处理多个文件时命中）。

**数据位置与格式**【已核实】
- 根：`PathRoot::EnvVar { var: "CLAUDE_CONFIG_DIR", fallback_relative: ".claude" }` + `relative: "projects"` + `pattern: "*.jsonl"`（`clients.rs:414-426`）。Windows 上默认 `%USERPROFILE%\.claude\projects\**\*.jsonl`。
- `claudecode.rs:430-433`：workflow 编排日志（`is_workflow_journal(path)` 为真）**直接返回空**，不是 transcript。
- `claudecode.rs:461-474`：若扩展名是 `json`，先走 `parse_claude_headless_json`（`claudecode.rs:1329-…`），**非空就直接返回**，不再读 JSONL。
- `claudecode.rs:507-752`：主循环逐行 `simd_json::from_slice::<ClaudeEntry>`（`:521`），**每个 JSON 值都先克隆进 `buffer` 再解析**（`:518-521`），所以解析器不持有整行字符串的所有权。

**会话身份**【已核实】
- `claudecode.rs:444-448`：初值 = 文件 stem，取不到用 `"unknown"`；
- `claudecode.rs:524-537`：**首条可解析行**若 `entry.is_sidechain == true`，则 `session_id` 被 `entry.session_id`（父会话 id）**覆盖**，并解析出 `sidechain_agent`。注意 `sidechain_detected` 只置一次（`:524-525`），所以同一文件后续行不再改变判定。

**workspace**【已核实】`claudecode.rs:771-802` `claude_workspace_from_path`：
1. 先找三段窗口 `[".claude", "projects", <slug>]`（`:777-783`）；
2. 再找五段窗口 `[".cc-mirror", ?, "config", "projects", <slug>]`（`:785-791`）；
3. 最后**反向**找任意相邻两段 `["projects", <slug>]`（`:793-799`）；
4. 全部经 `normalize_workspace_key`（`sessions/mod.rs:450-493`）+ `workspace_label_from_key`（`sessions/mod.rs:525-529`）。
都找不到返回 `(None, None)`（`:801`）。

**客户端标识（重要）**【已核实】`claudecode.rs:436-443`：
- `cc_mirror_metadata = cc_mirror_variant_metadata_from_path(path, home_dir)`（`:436`），若有则 `client_id = format!("cc-mirror/{}", sanitize_cc_mirror_segment(&self.name))`（`claudecode.rs:68`，调用点 `:437-440`），否则 `"claude"`；
- `metadata_provider_hint` 来自同一份元数据（`:441-443`，取值见 `:847-872`，其中 `cc_mirror_provider_id` `:836-845` 会把字面量 `"mirror"` 映射成 `"anthropic"`，其余走 `provider_identity::canonical_provider`）。
⇒ **`ParsedMessage.client` 可能是 `cc-mirror/<variant>`**。UsageMesh 直接把它写进 `RowKey.client`（`collector.rs:237` 只做 `normalize_text`，不识别前缀），而受影响的匹配点是 **`evidence::for_message` 的键匹配**（构建侧以字面 `"claude"` 建键 `evidence.rs:258-266`，查询侧按原样 `client` 字符串查表 `evidence.rs:807-815` ⇒ `cc-mirror/...` 不会命中）与 **`parser_provider_is_explicit`**（`collector.rs:95-99` 用 `matches!(client, "codex" | "opencode" | "micode")`，前缀串同样不命中；调用点 `collector.rs:151`）。过滤侧 tokscale 自己用 `retain_for_requested_clients` 特判（`lib.rs:193`），但 UsageMesh 不用 `clients` 过滤（传 `None`），所以会**原样接收**。

**时间戳**【已核实】
- `claudecode.rs:875-879` `parse_claude_entry_timestamp`：`DateTime::parse_from_rfc3339(ts)` → `timestamp_millis()`（**毫秒**）；
- `claudecode.rs:459` `fallback_timestamp = file_modified_timestamp_ms(path)`（`sessions/utils.rs:278-280`，取不到 mtime 时回退 `chrono::Utc::now()`）；
- `claudecode.rs:686-688`：`timestamp = pending_request_start_timestamp_ms(或) parsed_timestamp(或) fallback`。
  - `pending_request_start_timestamp_ms` 由**前一个 user/tool_result 行**设置（`:559-562`），即「本轮请求开始时刻」；
  - `:729` 用完即清空（避免背靠背 assistant 复用导致时长虚高）。
- `claudecode.rs:689-690` `duration_ms = duration_between_ms(pending, parsed)`，实现 `:881-884`：**要求 `end > start`，否则 `None`**。

**token 桶**【已核实】`claudecode.rs:703-709`：

| 桶 | 来源字段 |
|---|---|
| `input` | `usage.input_tokens`（`.max(0)`） |
| `output` | `usage.output_tokens`（`.max(0)`） |
| `cache_read` | `usage.cache_read_input_tokens`（`.max(0)`） |
| `cache_write` | `usage.cache_creation_input_tokens`（`.max(0)`） |
| `reasoning` | **恒 0**（Claude 侧不填） |

**tool_result 行**【已核实】`claudecode.rs:1031-1117`：
- token 桶只有 `input = usage.input_tokens`，其余全 0（`:1101-1107`）；
- `message_count = 0`（`:1113`）——即 tool_result 行**不计请求数**；
- `agent` 与 `workspace` 从上下文带上（`:1114-1115`）；
- `claudecode.rs:555` `allow_char_estimate: false`：**Claude Code 路径永久禁用「按字符估算」**，注释 `:450-458` 解释了原因（API 报告的 `input_tokens` 已包含 tool_result 文本，字符估算会双计，对应 tokscale#1011）。**显式的 tool-result token 元数据仍然被采纳**（`:1026-1028` 注释）。

**provider / model**【已核实】
- `model = canonicalize_claude_model(&raw_model)`（`claudecode.rs:684`）；实现 `claudecode.rs:1308-1312`：

```rust
1308: fn canonicalize_claude_model(model: &str) -> String {
1309:     pricing::aliases::resolve_alias(model)
1310:         .unwrap_or(model)
1311:         .to_string()
1312: }
```

  ⇒ **Claude 的 `model_id` 会被一张内置别名表改写**（`pricing/aliases.rs:22-…` `MODEL_ALIASES`，解析器 `:186-201`）。与 Claude 相关的条例如 `claude-opus-4.6` → `claude-opus-4-6`（`aliases.rs:135`）、`claude-opus-4-0` → `claude-opus-4`（`:142`）、`anthropic/claude-4-5-opus` → `claude-opus-4-5`（`:153`）。别名先在 `MODEL_ALIASES` 里查，再在 `CURSOR_PRICING_ALIASES` 里查（`:188-193`），最后对 `kimi-code/` 前缀做一次剥壳再查（`:199-200`）。
  **这与 `canonical_model_id` 的「不做别名折叠」是两回事**：别名折叠发生在解析器内部（把「用户看到的别名」折成「数据集里的键」），`canonical_model_id` 只在最后做结构归一。UsageMesh 在 `collector.rs:238` 追加的 `canonical_model_id` 是在**别名折过的结果**上再折一次。
- provider 走 `claude_provider_choice(&raw_model, provider_hint)`（`claudecode.rs:675-682`，实现 `:1587-…`，另有 `claude_provider_choice_from_parts` `:1577-…` 与 `claude_provider_choice_from_hint` `:1605-…`），带**置信度**：`CLAUDE_PROVIDER_DEFAULT_CONFIDENCE = 1`、`INFERRED = 2`、`EXPLICIT = 3`（`claudecode.rs:1569-1571`）。重复行合并时 confidence 更高者胜（`:638-644`、`:657-663`，经 `update_claude_provider_id`）。

**dedup_key**【已核实】

| 场景 | 键 | 行号 |
|---|---|---|
| 有 `message.id` 且有 `entry.request_id` | `"{message.id}:{request_id}"` | `claudecode.rs:629-631` |
| 有 `message.id` 无 `request_id` | `"message:{message.id}"` | `claudecode.rs:649-650` |
| 两者皆无 | `None`（**不参与去重**） | `claudecode.rs:668` |
| tool_result | `"{client_id}:tool_result:{session_id}:{usage_key}"`，`usage_key` 形如 `tool_result:{toolu_id}` | `claudecode.rs:1005-1007`、`:1111`、`:983` |

`dedup_key_is_globally_stable`（`claudecode.rs:998-1000`）= **不含 `:tool_result:` 就稳定**。注释（`:985-997`）说明：assistant 键来自 API 响应、可跨 fork 折叠；tool_result 键内嵌文件 stem，换文件名即不同键。

**文件内重复合并**【已核实】`claudecode.rs:886-914` `merge_claude_duplicate`：
- **四个桶逐字段 `max`**（`:892-900`：只有 `input` / `output` / `cache_read` / `cache_write` 四行 `.max(...)`，**没有 reasoning**——因为 Claude 不填 reasoning，所以这里不是「五个桶」）；
- `duration_ms` 单调不减：只有新时间戳 `>= existing.timestamp` 且差值 > 0 时才更新（`:902-913`，注释解释「重复可能乱序到达」）。

tool_result 的合并 `merge_claude_tool_result_duplicate`（`claudecode.rs:964-973`）：`input` 取 max；时间戳**直接推进到较新值**（`:970-972`，与 assistant 的单调 duration 逻辑不同）。

**静默占位**【已核实】`claudecode.rs:498-502` 与 `:595-602`：模型为 `<synthetic>` 的本地通知行会被识别（`is_claude_synthetic_placeholder_model`），并把 `suppress_unattributed_tool_results` 置真——之后**没有自己模型的 tool result 不会继承占位模型**，避免把字符估算变成无法计价的用量。另有 `remove_synthetic_placeholder_messages`（`claudecode.rs:1302-1306`）供缓存迁移使用。

#### 3.5.2 Codex CLI（`codex`）

**模块与入口**【已核实】
- `crates/tokscale-core/src/sessions/codex.rs:1011-1028` `parse_codex_file(path)`：`File::open` 失败 → 空 `Vec`（静默）；`session_id = session_id_from_path(path)`（`codex.rs:1017` → `sessions/utils.rs:496-501` = 文件 stem）；`fallback_timestamp = file_modified_timestamp_ms(path)`（`:1018`）；然后交给 `parse_codex_reader(reader, session_id, fallback_timestamp, 0, CodexParseState::default())`（`:1020-1026`）。
- `codex.rs:1065-…` `parse_codex_file_incremental(path, start_offset, state)` 是给持久缓存用的增量入口（**通道 A 不用它**，通道 A 通过 `parse_codex_file` 内部以 `start_offset = 0` 调用同一个 reader，`codex.rs:1020-1026`）。

**数据位置与格式**【已核实】根 `EnvVar CODEX_HOME` / `.codex` + `relative: sessions` + `pattern: *.jsonl`（`clients.rs:427-439`），`headless: true`。

**记录模型**【已核实】`codex.rs:25-110`：
- `CodexEntry { type, timestamp: Option<String>, payload: Option<CodexPayload> }`（`:27-32`）
- `CodexPayload`（`:35-72`）关键字段：`id`、`forked_from_id`、`type`、`model`、`model_name`、`model_info.slug`（`:90-92`）、`info`、`turn_id`、`started_at`（**秒级 Unix**，`:45-56` 注释明确 “documented as Unix timestamp (in seconds)” 并给出 codex-rs 依据）、`source`、`thread_source`、`cwd`、`model_provider`、`agent_nickname`、`message`；
- `CodexInfo { model, model_name, last_token_usage, total_token_usage }`（`:94-100`）；
- `CodexTokenUsage { input_tokens, output_tokens, cached_input_tokens, cache_read_input_tokens, reasoning_output_tokens, total_tokens }`（`:102-110`）。

**event type 分派**【已核实】
| `entry.type` | 处理 | 行号 |
|---|---|---|
| `session_meta` | `source == "exec"` → headless；`id` → `state.session_id_from_meta`；`forked_from_id`（或 `source.forked_from_id`）→ fork 状态机；`model_provider` → `state.session_provider`；`agent_nickname` → `state.session_agent`；`cwd` → workspace | `codex.rs:431-471` |
| `turn_context` | 更新 `state.current_model`、`state.current_turn_start_ms`、`last_accepted_token_timestamp_ms`；刷新 pending 消息的模型 | `codex.rs:473-487` |
| `event_msg` + `payload_type == "user_message"` | 人类轮次判定（`codex_message_is_human_turn`，`:1263-1273`，**只有以已知系统注入前缀集合 `CODEX_SYSTEM_INJECTED_PREFIXES`（`codex.rs:1246-1252`）开头才判为注入；任意 `<` 开头的人类 prompt 仍计人类轮次**，注释 `:1254-1262` 明确说明了这个设计）；置 `pending_turn_start` | `codex.rs:500-516` |
| `event_msg` + `payload_type == "token_count"` | 真正的 token 解析 | `codex.rs:519-694` |
| `event_msg` + `payload_type == "task_started"` | 只用于 fork 判定（`started_at` 与 `turn_id`） | `codex.rs:370-392` |
| 其它 | 交给 `parse_codex_headless_line`（`:714-751`），失败则标记 `parse_succeeded = false`（`:754-757`） | — |

**token 增量推导（最关键的一段）**【已核实】`codex.rs:538-611`：
- `total_usage = info.total_token_usage → CodexTotals`（`:542`）、`last_usage = info.last_token_usage → CodexTotals`（`:543`）；
- **`last_token_usage` 是唯一增量来源**（注释 `:538-541`：total 是可被压缩/上下文截断改写的可变快照，只用于去重与单调性检查）；
- 七分支匹配（`:559-599`）：`(total,last,prev)` 都全 → 先判 `total == previous` 跳过（`:563`）、再判「回退疑似过期」跳过（`:566-570`），否则取 `last`；首次事件取 `last`（**不用 full total**，避免续接会话的历史 token 被重复计入，`:573-575`）；只有 total 且有 prev → 取 delta（`:578-587`）；只有 total 且首次 → 取 total（`:590`）；只有 last 且有 prev → `previous + last`（`:592-594`）；只有 last 无 prev → 取 last（`:596`）；都没有 → 跳过（`:598`）；
- 零值快照跳过且**不推进 baseline**（`:601-609`，注释解释压缩后的零总量不能抬高后续 delta）；
- `CodexTotals::delta_from`（`:134-149`）四个桶任一下降即返回 `None`；
- `looks_like_stale_regression`（`:174-189`）：当前总量 ≥ 上一总量 × 98% 或 `current + 2×last ≥ previous` 时判为「乱序的过期快照」而非硬重置，避免把 `last_token_usage` 计两次。

**token 桶映射**【已核实】`codex.rs:191-216` `CodexTotals::into_tokens`：

```rust
209:         TokenBreakdown {
210:             input: (self.input - clamped_cached).max(0),
211:             output: (self.output.max(0) - clamped_reasoning).max(0),
212:             cache_read: clamped_cached,
213:             cache_write: 0,
214:             reasoning: clamped_reasoning,
215:         }
```

- `clamped_cached = min(cached, input).max(0)`（`:194`）——防「cached > input」的畸形数据放大总量；
- `clamped_reasoning = max(0, reasoning).min(output)`（`:208`）；
- 注释 `:195-208` 原文解释了为什么不减就会双计双价，并**明确限定这是 Codex 专属修正**。
- **`cached` 的取值优先级**（`:121-132`）：`cached_input_tokens.unwrap_or(0).max(cache_read_input_tokens.unwrap_or(0)).max(0)`。

**headless 路径**【已核实】`codex.rs:1127-1182`：
- 从任意行的 `usage` / `data.usage` / `result.usage` / `response.usage` 里取（`:1184-1189`）；
- 字段别名：input ← `input_tokens|prompt_tokens|input`（`:1191-1194`），output ← `output_tokens|completion_tokens|output`（`:1195-1198`），cached ← `cached_input_tokens|…`（`:1199-…`）；
- 桶映射 `:1170-1176`：`cache_write = 0`、**`reasoning = 0`**（reasoning 被丢弃）；
- `input==0 && output==0 && cached==0` → 返回 `None`（`:1150-1152`）；
- 时间戳取 `usage.timestamp_ms`，缺失则 `fallback_timestamp`（`:1148`），并把「是否用了 fallback」作为第二个返回值上报（`:1180`），供上层登记 `fallback_timestamp_indices`（`:687-689`、`:747-749`）。

**provider / model**【已核实】
- `agent`：headless 会话 → `Some("headless")`，否则 `state.session_agent`（`codex.rs:622-626`）；
- `provider`：`state.session_provider`（来自 `session_meta.model_provider`）→ 按模型推断（`inferred_provider_from_model` `provider_identity.rs:260`）→ 兜底 `"openai"`（`codex.rs:628-632`）；
- `model`：`payload_model`（`turn_context`/`token_count` 的 `model`）→ `info_model`（`info.model`/`model_name`/`model_info.slug`）→ `state.current_model`；最终 `model.clone().unwrap_or("unknown")`（`codex.rs:634-636`）。
- **Codex 的 model_id 不做别名折叠**（`codex.rs` 无 `resolve_alias` 调用，已由 grep 确认）。

**dedup_key**【已核实】`codex.rs:920-954`：

```
931:     return format!(
932:         "codex:token_count-total:{}:{}:{}:{}:{}:{}:{}",
933:         upstream_session_id,
934:         message.provider_id,
935:         model,
936:         total.input,
937:         total.output,
938:         total.cached,
939:         total.reasoning
940:     );
...
943:     format!(
944:         "codex:token_count:{}:{}:{}:{}:{}:{}:{}:{}",
945:         message.timestamp, ...
```

- 有累计总量时用**累计四元组**做身份（注释 `:927-930`：fork/子代理会把同一份上游 token_count 历史重放进很多子文件，累计总量才是稳定身份；时间戳只是老数据的兜底）；
- `upstream_session_id` 取 `session_forked_from_id` → `session_id_from_meta` → 文件 stem（`codex.rs:661-665`）；
- **首次赋值后不再覆盖**（`set_codex_dedup_key` `:956-970` 的 `if message.dedup_key.is_none()`）；
- 只有 `parsed_timestamp.is_some() || total_usage.is_some()` 时才设键（`:652-672`），否则该消息**无键**、不参与跨文件去重。

**message_count**【已核实】Codex 不改写 `message_count` ⇒ 每条 `token_count` 消息计 **1** 次请求（`sessions/mod.rs:111-113` / `:381`）。一条 `token_count` 对应一次上游 API 调用，因此这个语义是「API 请求数」。`is_turn_start`（`codex.rs:648-651`）反映的是「人类轮次后第一条」，但会被 `unified_to_parsed` 丢弃。

**fork / 子代理状态机**【精读】`codex.rs:56-72`（`thread_source` 注释）、`:356-416`（`forked_child_waiting_for_turn_context` 分支）、`:790-879`（`forked_child_turn_starts_own_session` / `forked_child_task_starts_own_session` / `codex_uuid_v7_order_key`）、`:1034-1064`（继承基线记忆与跳过判定）。这一整套是为解决「fork 日志把父会话历史原样重放导致重复计费」。**细节未逐行穷尽**，只核验了上述行号处的分支存在与判定输入。

#### 3.5.3 OpenCode（`opencode`）

**模块与入口**【代核实】`crates/tokscale-core/src/sessions/opencode.rs:26-28`：`parse_opencode_sqlite(db_path)` 只是转发到 `parse_opencode_schema_sqlite(db, OpenCodeSchemaConfig::opencode())`。Legacy JSON 入口 `opencode.rs:30-109` `parse_opencode_file(path) -> Option<UnifiedMessage>`（**单文件最多一条消息**，与其它解析器的 `Vec` 形态不同）。

**数据位置与格式**【代核实】`opencode.rs:4-5` 注释：SQLite `~/.local/share/opencode/opencode.db`（含 channel 后缀变体，`scanner.rs:96-101` 注释引用 opencode 的 `getChannelPath`），legacy JSON `~/.local/share/opencode/storage/message/`。SQLite 的 schema 与驱动被抽到 `sessions/opencode_schema.rs`，`opencode.rs:14-17` 导入 `parse_opencode_schema_sqlite / reported_cost / set_workspace_from_root / OpenCodeSchemaConfig / OpenCodeSchemaMessage`。**「OpenCode / MiMo Code / Kilo 共用一套反序列化类型与 ingest 循环」这条点名声明的出处是 `opencode_schema.rs:1-17`**（`:3-8` “OpenCode, MiMo Code, and Kilo all persist assistant turns as a JSON payload…this module owns a single set of `Deserialize` types and a single row-ingest loop for all three”、`:13-14` “`OpenCodeSchemaConfig`, which is `Copy` and built from a per-client `const fn` constructor”）；`opencode.rs:7-13` 只说 “shared with the other clients that adopted it”，**未点名**。

**共用 schema 驱动**【代核实】`opencode_schema.rs:849-875` `parse_opencode_schema_sqlite`：`open_readonly_sqlite_opt`（`:853`，`utils.rs:296-298` 用 `SQLITE_OPEN_READ_ONLY|NO_MUTEX`）打不开就返回空；按 `OPENCODE_QUERY_GROUPS`（`:403`）顺序尝试，首个 `prepare` 成功即 break（`:864-872`）。查询按版本分组：
- v2（`:329-369`）：表 `session_message`，列 `id/session_id/data/type`，`LEFT JOIN session_v2`（`:331-345`）或 `session`（`:347-361`）或**不 JOIN**（`:362-368`），`WHERE type='assistant' AND json_extract(data,'$.tokens') IS NOT NULL`；
- v1（`:374-398`）：表 `message` + `LEFT JOIN session`，`WHERE json_extract(data,'$.role')='assistant'` 且 tokens 非空。

**payload 字段**【代核实】`opencode_schema.rs:42-76`：`id` / `sessionID` / `session_id`（snake）/ `role` / `modelID` / `providerID` / 嵌套 `model` / `cost` / `tokens` / `time` / `agent` / `mode` / `path`；`tokens`（`:139-157`）`input`、`output` 必填，`reasoning`、`cache{read,write}` 可选；`time`（`:161-165`）`created` 必填 f64、`completed` 可选。

**token 桶**【代核实】`opencode_schema.rs:663-665`（`input/output/reasoning` 各自 `.max(0)`）+ `:618-622`（`resolve_cache` `:483-493`）。**OpenCode 的 `strict_cache` 默认 true（`:260`）** ⇒ `cache` 结构缺失或 `read`/`write` 只给一个就**整行丢弃**（`:483-493`），`cache_read`/`cache_write` 在 `:486-488` 被 clamp。`cache_write` 不做「1h 子集」区分（v4.14.0 无该概念）。

**时间戳与日桶**【代核实】`normalize_epoch` `opencode_schema.rs:500-506`：当 `normalize_epoch_seconds = true` 且值 ≤ 1e12 时按秒 ×1000。**OpenCode 的该开关为 false（`:261`）**，即原值直接当毫秒用。`$.time` 缺失时用 `cfg.fallback_timestamp`（`:638-648`），而 OpenCode 的 `fallback_timestamp = None`（`:262`、`:271-277` 未覆写）⇒ **整行丢弃**。`duration_ms` `:510-517`、`:702-704`。

**session_id / provider / model**【代核实】
- `session_id`：`:669-673` 默认取行 `session_id` 列；`payload_session_id = true` 的客户端改取 payload 的 snake 字段；OpenCode 自身该开关为 false（`:257`）。
- `provider`：`:634` → `resolve_provider`（`:519-541`）：显式 `providerID` → 模型推断（仅 `infer_provider_from_model = true` 的客户端）→ 兜底 `"unknown"`（OpenCode 为 `"unknown"`），末步 `:540` 过 `canonical_provider`（`provider_identity.rs:62-64`）。
- `model`：`:624-632`（`dual_schema` 时 `resolve_model_id` 顶层 `modelID` 优先于嵌套 `model.id`；**不做别名折叠**）。

**dedup_key**【代核实】`opencode_schema.rs:676-684`：有内嵌 `id` 就用 `id` 原样（跨库合并）；无 `id` 用行 `rowid`，命名空间开关打开时模板 `"{db_namespace}:{row_id}"`（`:682`，`db_namespace` = 数据库路径，`:857-861`）。OpenCode 的 `DedupMode = MergeUnlessIdConflict`（`:267`），合并逻辑 `:738-810`：内嵌 id 冲突则不并（`:771-776`）、首次带 id 的提升 `dedup_key`（`:792-795`）、workspace 冲突则**双方 workspace 置空**（`:796`、`:457-475`）。
- `parse_local_clients` 侧另有一层按 `dedup_key` 的跨 DB 去重（`lib.rs:4382-4392`），注释解释了「同一 session 可能同时存在于 `opencode.db` 与 `opencode-<channel>.db`」（`:4383-4386`）。

**session_title / agent / workspace / cost**【代核实】
- `session_title`：SQL 取 `s.title`（`:331`/`:347`/`:376`）→ `:731-736` trim 非空后写入。**这个字段在 `unified_to_parsed` 里被丢掉**（`lib.rs:5357-5376`）⇒ UsageMesh 拿不到会话标题。
- `agent`：`:650-661`，`mode` 优先或 `agent`，`normalize_agent = true` 时过 `normalize_opencode_agent_name`（`sessions/mod.rs:140-152`，含 `normalize_oh_my_opencode_agent_name` `:190-225` 与零宽字符清洗 `:230-237`）。
- `workspace`：`:720-729` 优先行 `workspace_root`（= `session.directory`），回退 payload `path.root`（`:103-105`），经 `set_workspace_from_root`（`:452-455`）。
- `cost`：`:544-546` `reported_cost`（有限且 ≥0），`:666-667`；provenance 开关 `WhenPositive`（`:705-717`）——**通道 A 不消费它**。

**message_count**：该驱动不赋值 ⇒ 每条 = 1（`sessions/mod.rs:111-113`）。

#### 3.5.4 Gemini CLI（`gemini`）

【代核实】
- 入口：`gemini.rs:100-102` `parse_gemini_file`；带缓存状态的实现 `:149-225`（JSONL 直接分派 `:152-154`）；会话 id 推导 `:112-147`。
- 路径过滤 `:156-191`：文件名以 `session-` 开头，**或**路径必须出现 `tmp/<id>/chats/<file>` 三段结构，否则返回空。
- 三种格式：legacy `session-*.json`、`chats/` 下 UUID 命名文件、当前 `session-*.jsonl`（注释 `:3-5`）。
- **session_id**：JSON 会话取 `session.sessionId`（`:229`）；JSONL 先取文件 stem（`:320-324`），再被 `init` 行（`:362-370`）或任意行的 `session_id`/`sessionId` 覆盖（`:374-377`）；headless JSON 用文件 stem（`:210-214`）。
- **时间戳**：JSON 会话用 `msg.timestamp` 字符串走 RFC3339 → `timestamp_millis`（`:243-247`），失败回退 `fallback_timestamp`（`:150` = 文件 mtime）；headless 走 `extract_timestamp_from_value`（`:610-615`，字段 `timestamp` 或 `created_at`）→ `utils.rs:203-221`（数字 ≥1e12 当毫秒，否则按秒 ×1000），仍缺失回退 mtime。
- **token 桶**（JSON 会话路径）：`deserialize_tokens`（`:51-81`）把多套别名归成一个中间结构——`input` ← `input|prompt|input_tokens|prompt_tokens|promptTokenCount`；`output` ← `output|candidates|output_tokens|completion_tokens|candidatesTokenCount`；`cached` ← `cached|cached_tokens|cachedContentTokenCount`；`thoughts` ← `thoughts|reasoning|thoughts_tokens`；`tool` ← `tool|tool_tokens`。映射 `build_gemini_token_message`（`:259-291`）：

| 桶 | 来源 |
|---|---|
| `input` | 归一化后 `input + tool`（`:283`） |
| `output` | `output`（`:284`） |
| `cache_read` | `cached`（`:285`） |
| `cache_write` | **恒 0**（`:286`） |
| `reasoning` | `thoughts`（`:287`） |

⚠️ **`thoughtsTokenCount` 这个字面量在 `gemini.rs` 里不存在**（限定到 `crates/tokscale-core/src`：它只出现在 `qwen.rs` 的 serde rename 与测试 fixture（`:32`，`:211` 起多处）以及 `codex.rs:206` 的注释；`README*.md` 与 `crates/tokscale-cli/tests/cli_tests.rs:840` 另有出现，原写的「全仓仅两处」不成立）。Gemini 侧真正的别名字段是上列 `thoughts|reasoning|thoughts_tokens`（`:77`）与 headless 侧的 `thoughts|thoughts_tokens|reasoning|reasoning_tokens`（`:588-592`）。若把 `thoughtsTokenCount` 当作 Gemini 的 reasoning 字段名，需要按实现改写为「别名集合」。
- **headless token 桶**：`extract_gemini_usage_from_value`（`:571-608`）；全 0 返回 `None`（`:594-596`）；**是否存在 input/cache 重叠由 `:604-606` 判定，重叠时用 `subtract_cached_overlap`（`:502-513`）扣除**；映射 `:483-497`（`cache_write` 恒 0）；会话侧对应 `normalize_gemini_session_input_and_cache`（`:515-541`）。多模型 stats 走 `extract_gemini_usages`（`:552-569`，`models` 对象每模型一条，否则用 `model_hint` 或 `"unknown"`）。
- **provider**：硬编码 `"google"`（`:279`、`:486`）——**不过 `canonical_provider`**。
- **model**：JSON 会话取 `msg.model`（`:238-241`，缺失即跳过）；headless 取 stats 的 `models` 键或 `model_hint`（缺省 `"unknown"`）。**不做别名折叠**。
- **dedup_key**：**没有**（该文件无 `dedup_key` 赋值，grep 无匹配）。JSONL 内同 `id` 记录用**替换语义**（`:390-399`：已见 id 覆盖前一条，否则 push）——这是「去重」但发生在解析器内部、不产出键。
- **message_count / agent / workspace**：均未设置（默认 1 / `None` / `None`）。`gemini.rs` 全文无 `set_workspace` 调用 ⇒ **Gemini 的用量在 UsageMesh 里没有项目归属**（`collector.rs` 不读 workspace，但 `activity.rs:34` 会退化成 `"unknown"`）。
- 覆盖度：`gemini.rs:1-622` 精读；测试区 `:623-1398` 仅 grep 抽样。

#### 3.5.5 Grok Build（`grok`）

【代核实】
- 入口：`parse_grok_updates_file` `grok.rs:339-519`、`parse_grok_unified_log_file` `:526-537`（内含 `parse_grok_unified_log_snapshot` `:548-767`）、分派 `parse_grok_file` `:825-831`（**只接受 `updates.jsonl` / `unified.jsonl`，其它返回空**）、关联路径 `grok_related_paths` `:838-883`、跨格式择优 `prefer_unified_log_messages` `:888-1014`。
- 数据：注释 `:3-11`——`~/.grok/sessions/<urlencoded-workspace>/<session-id>/updates.jsonl`（JSON-RPC session updates）与同级 `signals.json`（含 `totalTokensBeforeCompaction` / `contextTokensUsed`）；新版本另写每推理明细到 `~/.grok/logs/unified.jsonl`；`:7-9` 说明 legacy 只有累计 `totalTokens`、无稳定 in/out 拆分。注册 `clients.rs:692-704`（`EnvVar GROK_HOME` / `.grok`，`relative: sessions`，`pattern: updates.jsonl`）。
- 常量：`CLIENT_ID = "grok"`（`:24`）、`PROVIDER_ID = "xai"`（`:25`）、`UNKNOWN_MODEL = "grok-unknown"`（`:26`）、`UNIFIED_LOG_DEDUP_PREFIX = "grok-unified:"`（`:27`）。
- **session_id**：updates 取目录名（`read_metadata` `:1302-1309`，空则 `"unknown"`），可被 `events.jsonl` 的 `session_id` 覆盖（`:1389-1393`）；unified 取行 `sid`（`:658-661`），`:761` 再赋给消息。
- **时间戳**：updates `:380` 走 `extract_timestamp_ms`（`:1443-1456`，依次 `params._meta.agentTimestampMs`、`params.update._meta.agentTimestampMs`、`params.timestamp`、`timestamp`、`ts`）→ `parse_timestamp_value`；缺省回退 `metadata.timestamp`（初值 = 文件 mtime `:1319`，可被 `summary.json` 的 `updated_at`/`created_at` `:1367-1373` 或 `events.jsonl` 的 `ts` `:1394-1396` 覆盖，单位毫秒）。unified `:696-699` 取行 `ts`，缺省回退 mtime（`:553`）。
- **token 桶**：updates 权威 usage `GrokUsage::from_update`（`:180-252`）——`input` ← `inputTokens|input_tokens|promptTokens`；`output` ← `outputTokens|output_tokens|completionTokens`；`cache_read` ← `cachedReadTokens|cacheReadTokens|cache_read_input_tokens`；`cache_write` ← `cachedWriteTokens|cacheWriteTokens|cacheCreationTokens|cache_creation_input_tokens`；`reasoning` ← `reasoningTokens|thoughtTokens|thinkingTokens`；`total` ← `totalTokens|total_tokens`。**重叠拆分** `:227-246`：报告 total 缺省或等于 `input+output` 时 `input -= cache_read`、`output -= reasoning`。
  - 累计 `totalTokens` 的 per-turn 正增量**只记 input**（`ActiveTurn::into_message` `:310-336`）；`signals.json` 对账余额也只记 input（`append_signals_reconciliation` `:1233-1300`，`:1285-1291`）。
  - unified 路径 `:750-756`：`input = prompt_tokens - cached_prompt_tokens`、`output = completion_tokens - reasoning`、`cache_read = cached_prompt_tokens`、`cache_write = 0`、`reasoning = min(reasoning_tokens, completion_tokens)`（`:700` 算 clamped，`:682` 先把 cached clamp 到 prompt）。
- **provider**：硬编码 `"xai"`（`:25`、`:277`、`:1282`），**无 `canonical_provider`**（grep 无匹配）。
- **model**：updates 优先级 `extract_model_id`（`:1408-1425`：`params.update._meta.modelId` → `params._meta.modelId` → `params.modelId` → `model_id` → `modelId` → `model`）→ `usage.modelUsage` 单键（`:401-404`）→ `metadata.model_id`（signals 的 `primaryModelId` 或 `modelsUsed[0]` `:1223-1231`、`summary.json` 的 `current_model_id`/`model_id` `:1362-1365`、`events.jsonl` 的 `model_id` `:1386-1388`）→ `UNKNOWN_MODEL`（`"grok-unknown"`）。unified 用 `model_by_pid_and_session` / `fallback_model_by_pid`（`:576-652` 由 `msg` 事件建立），取值顺序 `:723-745`；**冲突时置 `UNKNOWN_MODEL` 并标 `model_attribution_conflicted = true`**（`:726-727`、`:760`）——后者同样会被 `unified_to_parsed` 丢掉。
- **dedup_key**：`"grok:{session_id}:{turn_index}"`（`:333`）；`"grok:{session_id}:usage:{usage_index}:{event_id}"`（`:427-430`，`event_id` 取 `params._meta.eventId` 否则 `turn-{usage_index}`，`:408-410`）；`"grok:{session_id}:signals"`（`:1293`）；unified 走 `unified_log_dedup_key`（`:1087-1118`），模板 `"{grok-unified:}{session_id}:{identity}"`，`identity` = `"id:{event_id}"`（`:1088-1103`）或 `"row:{整行序列化}"`（`:1110-1113`）。
- **`prefer_unified_log_messages` 语义【代核实】**`grok.rs:889-1014`：先收集 unified 行覆盖的 session（`:889-897`，无 unified 则原样返回）→ 统计 legacy 行的 model 与 workspace（`:899-936`，同 session 内不一致记 `None`）→ 把一致的 model/workspace 回填进 unified 行的空缺（`:938-954`）→ **覆盖判定**：unified 行按 `(session_id, timestamp, tokens.total())` 计数（`:962-976`），命中的 legacy 行丢弃；legacy fallback 行另按 `(session_id, timestamp)` 匹配（`:997-1007`）；**未覆盖的 legacy 行保留**，避免部分迁移丢历史。
- **message_count**：unified 路径 `:762` `message_count = i32::from(message.is_turn_start)`，而该路径传入的 `is_turn_start` 实际就是 `loop_index == 1`（`grok.rs:758`；`ctx.loop_index` 缺失时按 1 处理，`grok.rs:684-692`）⇒ **取值域恰为 `{0,1}`：`loop_index == 1` 的行记 1、其余记 0**（测试 fixture 4 条中 1 条为 1：`grok.rs:1572-1598`）。updates/signals 路径不设（默认 1）。
- **agent**：未设置（该文件无 `agent` 赋值）。
- **workspace**：updates/signals 经 `message_from_tokens`（`:284-287`）与 `:1295-1298`，值来自 `read_metadata`（`:1311-1317`，workspace 目录名 percent-decode 后过 `normalize_workspace_key`）；unified 先用行所属 session 的 metadata（`:706-709`，`read_unified_session_metadata` `:1130-1171`）。
- 覆盖度：`grok.rs:1-1470` 精读；`:1471-2141` 仅 grep 抽样。

⚠️ **对 UsageMesh 的关键后果**：Grok 的 `message_count` 在「unified log」来源下**不是「每行 1」**——该路径把它严格写成 `i32::from(loop_index == 1)`（`grok.rs:758`、`:762`），只有 `loop_index == 1` 的行记 1、其余记 0（缺 `ctx.loop_index` 的行按 1 处理，`grok.rs:684-692`）。UsageMesh 用 `message.message_count.max(0)` 当请求数（`collector.rs:108`），因此**若本机 Grok 走 unified 路径，UsageMesh 的请求计数会低于实际**，而 token 总量不受影响。

#### 3.5.6 Kimi（`kimi`）

【代核实】
- 入口：`parse_kimi_code_file` `kimi.rs:175-253`、`parse_kimi_file` `:256-338`；分派判定 `is_kimi_code_path` `:124-129`（取 `wire.jsonl` 父目录的父目录名是否等于 `agents`）；调用点在 `lib.rs:4871-4875`。
- 数据：注释 `:1-9`——`~/.kimi/sessions/[GROUP_ID]/[SESSION_UUID]/wire.jsonl`（token 来自 `StatusUpdate` 消息）与 `~/.kimi-code/sessions/[WORKSPACE]/[SESSION]/agents/[AGENT]/wire.jsonl`（token 来自 `usage.record` 行）。注册 `clients.rs:503-512`（`Home` + `.kimi/sessions` + `pattern: wire.jsonl`）——**注意注册表里只有 legacy 路径，kimi-code 靠 `is_kimi_code_path` 在解析阶段区分**。
- **session_id**：legacy `:109-115` 取路径父目录名（SESSION_UUID），缺省 `"unknown"`；kimi-code `:133-142` 上溯三级目录名。
- **时间戳**：legacy `:302-309`——`wire_line.timestamp`（f64）按 **Unix 秒 ×1000**，`<=0`/NaN 回退 mtime；kimi-code `:236-239`——`time` 字段按 **Unix 毫秒**，注释 `:230-235` 明确**不走 `parse_timestamp_value`**（避免 1e12 以下的秒/毫秒启发式误判）。
- **token 桶**：`TokenUsage`（`:45-54`）字段 `input_other`（别名 `inputOther`）/ `output` / `input_cache_read`（别名 `inputCacheRead`）/ `input_cache_creation`（别名 `inputCacheCreation`）；`to_breakdown`（`:59-77`）四桶各自 `.max(0)`，**`reasoning` 恒 0**（`:74-75` 注释：wire 协议不暴露 reasoning，全部并入 output），全 0 返回 `None`（`:65-67`）。
- **provider**：硬编码 `"moonshot"`（`:82`、`:244`、`:321`）——不过 `canonical_provider`。
- **model**：legacy `:92-105` 读 `~/.kimi/config.json` 的 `"model"`（路径由 `kimi_config_path` `:86-89` 推导），否则 `"kimi-for-coding"`（`:81`）；kimi-code `:191-199`/`:223-228` 取记录里最近的 `llm.request` 模型与自身 `model`，经 `normalize_kimi_code_model`（`:145-150`，剥 `kimi-code/` 前缀）与 `concrete_kimi_code_model`（`:154-160`，排除 `__...__` 符号引用），再回退。
  ⚠️ 但注意：`"kimi-for-coding"` 会被 **Claude 之外的路径**改写吗？——`kimi.rs` 自身不做别名折叠，但 `pricing/aliases.rs:36-37` 里有 `kimi-for-coding` → `kimi-k2.7-code` 的条目，那是**计价匹配**用的，不在解析器内生效。⇒ **`ParsedMessage.model_id` 会是 `kimi-for-coding` 这种「订阅名」。**
- **dedup_key**：legacy = `payload.message_id`（`:316`、`:326`，无前缀模板）；同键去重规则 `push_or_replace_status_update` `:381-415` + `should_replace_status_update` `:357-379`（token 总和用 i128 比较、大者胜 `:340-346`；总和相同先比时间来源 `Wire` > `FileMtime` `:374-376`，再比 `timestamp` `:378`）；`message_id` 缺失/空时**直接 push**（`:394-398`）。kimi-code 路径无 `dedup_key`。
- **message_count / agent / workspace**：三个入口都不设 ⇒ 默认 1 / `None` / `None`。
- 覆盖度：`kimi.rs:1-422` 精读；`:423-898` 仅 grep 抽样。

#### 3.5.7 Pi（`pi`）

【代核实】
- 入口：`parse_pi_file` `pi.rs:326-328`（client `"pi"`、fallback_provider `"pi"`）→ `parse_pi_format_file` `:335-349` → `parse_pi_format_file_with_dedup` `:354-368`（`dedup_namespace = Some(client)`）；RLM 变体 `parse_pi_format_rlm_file_with_observer` `:395-409`；主体 `parse_pi_format_file_inner` `:524-740`。
- 数据：注释 `:3-9`——`~/.pi/agent/sessions/<encoded-cwd>/*.jsonl`，首行 session header，之后逐条记录；OMP 可能先写 title 记录（`PRE_SESSION_METADATA_TYPES` `:170`）；**Pi 派生客户端（Senpi、Oh My Pi）复用同一记录格式**。
- 追加式解析：`:546` `for_each_json_line_with_bytes`；header 之前只接受 `"session"`（`:571-577`，title 等可跳过 `:572-574`），其它类型视为 malformed 并 Break（`:575-577`、`:735-737` 返回空）；standard 模式丢弃非 UTF-8 行（`:552-554`），prime/RLM 模式走 lossy 读取（`:431-437`）。
- **session_id**：`:596-602` 取 header `id`；若 id 含替换字符（损毁）改用手写占位 `"unknown:path:{sha256(source canonical path)}"`（`damaged_session_placeholder` `:452-457`）。
- **时间戳**：`:667-672` `entry.timestamp` 字符串 → RFC3339 → `timestamp_millis`；失败或缺失回退 `file_modified_timestamp_ms`（`:532`）。RLM 子会话额外**拒绝损毁时间戳**（`:494-503`）。
- **token 桶**：`PiUsage`（`:233-248`）字段 `input/output/cacheRead/cacheWrite/totalTokens/reasoning`；`to_breakdown`（`:259-267`）四桶 `.max(0)`、**`reasoning` 恒 0**（`:250-258` 注释：Pi 的 reasoning 是 output 子集，映射会双计）。`totalTokens` 只反序列化、不参与求和。
- **provider**：`:655-665`——记录里的 `message.provider` 非空且未损毁则原样用；否则 `provider_identity::inferred_provider_from_model(model)`，再回退 `"pi"`。**未过 `canonical_provider`**。
- **model**：`:510` 必填（缺失即不产出记录），损毁时置字面量 `"unknown"`（`:644-648`）。**不做别名折叠**。
- **dedup_key**：

| 模式 | 键 | 行号 |
|---|---|---|
| standard（`dedup_namespace = None`） | `None` | `:341-348`、`:684` |
| 有 namespace、非跨会话 | `"{namespace}:{session_id}:{message_id}"`（`message_id` = `entry.id`） | `:726` |
| 跨会话（prime/RLM）优先 | `"{namespace}:response:{responseId}"` | `:693` |
| 跨会话兜底 | `"{namespace}:message:{id}:{stable_timestamp}:{provider}:{model}:{input}:{output}:{cache_read}:{cache_write}"`（时间戳缺失写 `"missing"`） | `:703-709`、`:700-702` |
| id 损毁 | `"{namespace}:damaged:{sha256 hex(raw line bytes)}"` | `:444-450`、`:712-722` |

  跨会话去重的意图注释在 `:385-394`（fork 会复制历史条目）。
- **agent**：`:613-629` 取 `session_info` 记录的 `name`；`rlm_session_name_as_agent = true` 且 `header.rlmDepth > 0` 时直接用该名字（`:614-622`），否则过 `pi_subagent_name`（`:312-323`，剥离 `subagent-` 前缀与尾部生成 id，`:287-310`）；`:682` 进 `new_with_agent`。RLM 判定：llm 模式 `rlm_depth > 0`（`:605`）。
- **workspace**：`:592-604` 取 `header.cwd`（损毁则不用）过 `normalize_workspace_key`，`:729` 写入每条消息。
- **message_count**：不设 ⇒ 默认 1。
- 覆盖度：`pi.rs:1-750` 精读；`:751-1136` 仅 grep 抽样。

#### 3.5.8 其余客户端（按 parser 分节，全部【代核实】）

##### Kilo CLI（`kilo`）
- `kilo.rs:16` `parse_kilo_sqlite`、`:24` `parse_kilo_sqlite_with_fallback`；驱动 `opencode_schema.rs:849`；调用点 `lib.rs:4958-4969`（**无去重 pass**）。
- 数据：注释 `kilo.rs:4` `~/.local/share/kilo/kilo.db`；SQLite 单表 `message`，payload 在 `data` 列，走 OpenCode schema（`kilo.rs:6-9`）；查询 `opencode_schema.rs:432-438`（`json_valid` + `role=='assistant'` + tokens 非空）。
- `session_id`：payload snake `session_id`（`payload_session_id=true` `opencode_schema.rs:306`），取值 `:669-673`，缺失退行 `session_id` 列。
- 时间戳：`$.time.created`（f64）`opencode_schema.rs:161`；`normalize_epoch_seconds` 基类默认 false（`:261`），kilo 未覆写（`:301-316`）；fallback = DB 文件 mtime（`kilo.rs:17` + `opencode_schema.rs:638-648`）。
- token：`input/output/reasoning` = `tokens.*` 各 `.max(0)`（`:663-665`）；`cache_read/write` = `tokens.cache.read/write`（`:618-619`、`:692-698`）；`strict_cache` 默认 true（`:260`）⇒ cache 不完整整行丢弃（`:483-493`）。
- provider/model：provider 走 `resolve_provider`（`:519-541`，兜底 `"kilo"` `:304` + `canonical_provider` `:540`）；model = `$.modelID`，无归一化（`:624-632`）。
- dedup_key：优先 `$.id` 否则行 `id` 列（namespace 未开，基类默认 false `:266`；`:675-684`、`:718`）；模块内 `DedupMode::Off`（`:313`、`:738-741`）。
- message_count：不设 ⇒ 1。agent：设置（`new_with_agent` `:686-701`；`$.agent` 优先于 `$.mode`，`normalize_agent=false` `:307` ⇒ 原样）。workspace：**无**（`capture_workspace=false` `:312`、`:720-729` 跳过）。

##### Goose（`goose`）
- `goose.rs:48` `parse_goose_sqlite`；调用点 `lib.rs:4985-4993`（无去重）。
- 数据：注释 `goose.rs:4-7` 四路径（`~/.local/share/goose/sessions/sessions.db`；macOS Library Application Support；legacy `Block/goose`；`$GOOSE_PATH_ROOT/data/...`）；SQLite `sessions` 表，SQL `:49-64`。
- `session_id`：`sessions.id`（`:51`、`:68`）。
- 时间戳：`created_at` TEXT → `parse_created_at`（`:31-46`：RFC3339 → ms；`"%Y-%m-%d %H:%M:%S"` 按 UTC；`"%Y-%m-%d"` 当日 00:00 UTC；全失败 = `0.0`）→ `timestamp_secs_to_ms`（`:111`，`utils.rs:518-529`）。**无 mtime fallback，失败即 0**。
- token：`input = accumulated_input_tokens` 否则 `input_tokens`（`:88-91`）；`output = accumulated_output_tokens` 否则 `output_tokens`（`:92-95`）；`cache_read = 0`、`cache_write = 0`（`:115-116`）；`reasoning = total - (input + output)`（`:96-99`、`:121-125`），注释**自标 “INFERRED, not a real field”**（`:117-120`）。
  ⚠️ **这是「推导出来的 reasoning」**，不是供应商上报值；且 `total` 用的是 `accumulated_total_tokens` 否则 `total_tokens`（`:96-99`）。
- provider/model：provider 走 `resolved_provider(provider_name, model_id, "goose")`（`:105` → `utils.rs:534-546`，过 `canonical_provider`）；model = `model_config_json.model_name`（`:20-29`、`:82-84`）。
- dedup_key：`Some(session_id)` 本身，无模板（`:129`）⇒ 相邻重复调用会因键相同而被跨文件去重（同一 session 只留一条）。
- message_count / agent / workspace：均不设。

##### Crush（`crush`）
- `crush.rs:47` `parse_crush_sqlite`、`:60` `parse_crush_sqlite_in(db_path, &BucketTimezone)`；调用点 `lib.rs:5034-5049`（**唯一接收时区的 lane**，`lib.rs:5032-5033`）。
- 数据：注释 `crush.rs:3`「per-project SQLite (`crush.db`)」；发现经 `projects.json`（`scanner.rs:945-…`）与候选目录（`:976-…`）；注册 `clients.rs:563-572`（`XdgData` + `crush/projects.json`）。
- **会话身份**：`format!("{}:{}", db_namespace, session.id)`（`crush.rs:78`，`db_namespace` = DB 路径 `:74`）；成本挂在 root session（`parent_session_id IS NULL`，`:146-147`），助理消息用**递归 CTE** 取（`:172-188`）。
- 时间戳：`messages.created_at` → `normalize_crush_timestamp_ms`（`:221-231`：`<=0` → `None` 丢弃；`>= 1e11` 当毫秒；否则 `checked_mul(1000)`）；日桶 `local_day_key`（`:233-240`）**用传入的 `BucketTimezone::day_key`**（`:234`）；无助理消息时回退 `updated_at` 再 `created_at`（`:242-244`、`:115-119`）。
- token：**全零** `TokenBreakdown::default()`（`:101`、`:127`）——模块头自述 **COST-ONLY**（`:7-12`）。⇒ Crush 在 UsageMesh 里只会贡献 `message_count`（+ 它自己不算的 cost），token 全 0。
- provider/model：常量 `CRUSH_PROVIDER_ID = "crush"`（`:23`）、`CRUSH_MODEL_ID = "session-total"`（`:22`）。
- dedup_key：无。message_count：按天桶 `bucket.message_count.max(0)`（`:104`），无助理桶的会话记 0（`:130`），桶计数 `:206-211`。
- workspace：**`crush.rs` 自身没有**（grep `set_workspace|workspace` 命中 0）；由 `lib.rs:5041` 用 `CrushDbSource.workspace_key/label` 注入（来源 `scanner.rs:85-89`）。

##### Trae（`trae`）
- `trae.rs:112` `parse_trae_file(client, path)`；调用点 `lib.rs:5080-5094`，随后 `dedupe_latest_trae_messages`。
- 数据：注释 `trae.rs:3-4` `trae-cache/sessions/*.json`（usage API 原始 dump）；注册 `clients.rs:649-658`（`Config` + `trae-cache/sessions` + `*.json`，`submit_default: false`）。
- `session_id`：字段 `session_id`，空/缺失即丢弃（`:69`、`:71-73`）。
- 时间戳：`usage_time`（注释标 epoch **秒**，`:74-77`）→ `checked_mul(1000)`（`:78`）；缺失/`<=0` 丢弃（`:70-73`），溢出拒绝（`:289-303`）。**无 mtime fallback**。
- token：`extra_info.input_token/output_token/cache_read_token/cache_write_token`（`:82-85`）；**`reasoning` 恒 0**（`:104`）；四桶全 0 丢弃（`:87-89`）。
- provider/model：`provider_for_model(&model_id)`（`:64`），实现按显示名 `contains` 匹配（GPT→openai / Claude→anthropic / Gemini→google / GLM→zhipu / 否则 trae，`:35-47`），**未走 `canonical_provider`**；model 走 `normalize_trae_model` 查表（`:15-32`、`:57-58`），空则 `trae-{mode}`（小写）再空 `trae-unknown`（`:59-62`）。
- dedup_key：`Some(format!("trae:{}:{}", session_id, usage_time))`（`:91`）。
- **`dedupe_latest_trae_messages`（`lib.rs:2922-2954`）语义**：按 `session_id` 分组，**每组只保留 `timestamp` 最大的一条**（`:2929-2936`，平局比 `dedup_key` 字典序取更大），结果按 `(session_id, timestamp)` 排序（`:2948-2952`）；调用处用返回值**整体替换** trae 消息（`lib.rs:5080-5090`）。⇒ **同一会话较早的 usage 记录被丢弃**，Trae 的用量只反映「每会话最后一次快照」。

##### Zed（`zed`）
- `zed.rs:36` `parse_zed_sqlite`；调用点 `lib.rs:4995-5005`（无去重）。
- 数据：注释 `zed.rs:4-6` 三路径（`$XDG_DATA_HOME/zed/threads/threads.db`；macOS `Library/Application Support/Zed/threads/threads.db`；Windows `%LOCALAPPDATA%\Zed\threads\threads.db`）；SQLite `threads` 表，payload 在 `data` 列，`data_type ∈ {json, zstd}`（`:179-204`）。
- `session_id`：行 `threads.id`（`:53`、`:159`）。
- 时间戳：列 `created_at`（表无此列时 SQL 填 NULL，`:70-87`、`:89-104`）→ 列 `updated_at` → payload `$.updated_at` 字符串（`:277-288`）；统一走 `parse_timestamp_str`（`utils.rs:223-257`：RFC3339 → ms；无时区 ISO 按 UTC；纯数字 ≥1e12 当 ms 否则 ×1000）；**三者皆失败整条跳过**（`:153`）。
- token：`input/input_tokens`、`output/output_tokens`、`cache_read=cache_read_input_tokens`、`cache_write=cache_creation_input_tokens`、`reasoning` **恒 0**（`:253-261`，注释说明 Zed 无 reasoning 字段 `:250-252`）；`usage_field` 兼容 i64/u64/字符串并 `.max(0)`（`:263-275`）；来源优先 `request_token_usage`（`:209-212`、`:222-248`），为零则 `cumulative_token_usage`（`:214-219`）。
- provider/model：provider 常量 `ZED_HOSTED_PROVIDER = "zed.dev"`（`:22`，使用 `:158`）；**只有 `model.provider` 等于它才计入**（`eq_ignore_ascii_case` `:143`），`imported:true` 跳过（`:133-139`）；model = `$.model.model` trim 直用（`:147-150`）。
- dedup_key：`Some(format!("zed:{}", row.id))`（`:163`）。
- message_count：来自 `thread_usage`（`:152`、赋值 `:165`）；request 路径 = 该线程 request 计数 `.max(1)`（`:211`），cumulative 路径恒 1（`:216`）。
- workspace：有。`workspace_key_from_folders`（`:290-306`、`:308-319`，列 `folder_paths` 按行切分 + `folder_paths_order` 取排序最小）→ `normalize_workspace_key`（`:305`）→ `set_workspace`（`:171-172`）。
- ⚠️ Zed 的价格有 **×1.1 加成**，但只在通道 B 生效（`pricing_multiplier` `lib.rs:4190-4210`，注释引用 zed.dev 文档）。**UsageMesh 走通道 A，拿不到这个加成。**

##### Warp（`warp`）
- `warp.rs:32` `parse_warp_file`；调用点 `lib.rs:5096-5108`。
- 数据：**`warp.rs` 全文（153 行）无模块注释与路径字面量**；格式 = Warp usage cache JSON，结构 `WarpUsageCache { syncedAt, usage, workspaces }`（`:7-30`，camelCase `:8`）。路径线索只在注册表：`clients.rs:659-668`（`Config` + `warp-cache` + `usage*.json`）。
- session_id：聚合行字面量 `"warp-aggregate-account"`（`:78`）；workspace 行 `format!("warp-aggregate-{workspace_id}")`（`:104`），`workspace_id` 经 `sanitize_id`（`:94-99`、`:138-153`：trim/小写/非 `[a-zA-Z0-9._-]` → `-`/去首尾 `-`），缺失或空用 `"unknown"`（`:99`）。
- 时间戳：`syncedAt` 字符串 → `parse_rfc3339_millis`（`:41-45`、`:120-124`）；**无 fallback：无效或 ≤0 直接返回空向量**（`:46-48`）。
- token：**全零**（`:80`、`:106`）。provider/model：字面量 `"warp"` / `"aggregate-requests"`（`:74-82`、`:100-108`）。
- dedup_key：无。message_count：`requests_used`（`:83`、`:109`，`non_negative_i32` 非负并 clamp `i32::MAX` `:126-132`）。workspace 行 `set_workspace(workspace.id, workspace.name)` **原样**（仅过滤空串，`:110-116`），**未走 `normalize_workspace_key`**。

##### WorkBuddy（`workbuddy`）
- 入口 `workbuddy.rs:14` `parse_workbuddy_file`、`:43` `parse_workbuddy_sqlite`、`:29` `is_detailed_workbuddy_source`。
- 分支：详细来源内先判 extension log → `tencent_buddy::parse_extension_log_file("workbuddy", DEFAULT_MODEL, path)`（`:16-22`），否则 `parse_jsonl_file`（`:23`）；非详细来源走 SQLite（`:26`）。`DEFAULT_MODEL = "workbuddy"`（`:12`）。
- 数据：注释 `:3-5` `~/.workbuddy/projects/**/*.jsonl` 与旧版 `~/.workbuddy/workbuddy.db`。
- `session_id`：SQLite 取 `su.session_id`（`:46`、`:52`、`:62`）；JSONL/日志取 `$.sessionId`，缺失用文件名 stem（`tencent_buddy.rs:254-256`、`:190-194`）。
- 时间戳：SQLite `su.updated_at` → `normalize_timestamp_ms`（`:91`、`:113-119`：`>1e10` 当毫秒原样，否则 `saturating_mul(1000)`），SQL 已过滤 `updated_at > 0`（`:55-56`），**无二级 fallback**；JSONL `$.timestamp` 原值、缺失用文件 mtime（`tencent_buddy.rs:257`、`:195`）；extension log 行首时间字符串走 `parse_log_timestamp_ms`（`:344`、实现 `:426-462`），失败用 mtime。
- token：SQLite **只填 `input = used`**，其余全 0（`:92-98`）⚠️；JSONL/日志走 `BuddyUsage::to_breakdown`（`tencent_buddy.rs:92-125`）：`cache_read` ← `cache_read_input_tokens|cacheReadInputTokens|cacheTokens|prompt_cache_hit_tokens|cached_tokens` 首个正值（`:93-99`）；`output` ← `output_tokens|outputTokens|completion_tokens`（`:100-104`）；`cache_write` ← `cache_creation_input_tokens|cacheCreationInputTokens|cachedWriteTokens|prompt_cache_write_tokens`（`:105-110`）；`reasoning` ← `completion_thinking_tokens|completionThinkingTokens|reasoningTokens`（`:111-115`）；`input` ← `cachedMissTokens|cacheMissTokens` 优先，否则 `input_tokens|inputTokens|prompt_tokens`，**仅当报告 total 证明含 cache 时才减 `cache_read`**（`:130-163`）。
- provider/model：SQLite provider = `inferred_provider_from_model` 否则 `"workbuddy"`（`:82-84`），model = `sessions.model` trim 非空否则 `"auto"`（`:75-81`）；JSONL/日志 provider = `inferred_provider_from_model` 否则 `"tencent"`（`tencent_buddy.rs:251-253`、`:11`），model ← `providerData.model` → `requestModelId` → `message.model` → 默认名（`:240-250`）。
- dedup_key：SQLite `Some(format!("workbuddy:{}:{}", session_id, updated_at))`（`:103`，注释 `:101-102` 说明含 `updated_at` 是为防同会话多行被折叠）；JSONL `"{client}:{session_id}:{messageId|traceId|line.id}"`（`tencent_buddy.rs:274-279`）；extension log `"{client}:extension-log:{agent_id}:{dedup_second}:…"`（`:368-378`）。
- workspace：有。SQLite 用 `cwd` 归一化（`:105-108`）；JSONL 同逻辑（`tencent_buddy.rs:269-272`）；日志路径推导（`:380`、`:464-471`）。
- **`lib.rs` 侧编排**：`partition_workbuddy_paths`（`lib.rs:2956-2960`，按 `is_detailed_workbuddy_source` 划分）→ 两批解析 → `merge_workbuddy_messages`（`lib.rs:2962-2992`）：先取详细消息的 `session_id` 集合（`:2976-2980`），**fallback 中 session 已被详细覆盖的整条丢弃**（`:2987-2990`），两侧均先经 `dedup_key` 首见保留（`:2984`、`:2989` + `lib.rs:5378-5383`）。调用点 `lib.rs:5190-5207`。

##### CodeBuddy（`codebuddy`）
- `codebuddy.rs:12` `parse_codebuddy_file`；调用点 `lib.rs:5171-5189`，`lib.rs:5179-5184` 按 `dedup_key` 首见保留（内联实现，等价 `should_keep_deduped_message`）。
- 数据：注释 `:3-5` `~/.codebuddy/projects/<project-key>/*.jsonl` 与 IDE/VS Code 扩展日志。
- 分支：扩展日志 → `tencent_buddy::parse_extension_log_file("codebuddy", DEFAULT_MODEL, path)`（`:13-15`），否则 `parse_jsonl_file`（`:17`）；`DEFAULT_MODEL = "codebuddy"`（`:10`）。
- `session_id`：JSONL `$.sessionId`，缺失用文件名 stem（`tencent_buddy.rs:254-256`、`:190-194`）；extension log 以 `$.sessionId` 为主、stem 仅空时回填（`:303-307`、`:382-384`）。
- 时间戳：JSONL `$.timestamp` **毫秒原值（不换算）**（`tencent_buddy.rs:257`），缺失用 mtime（`:195`）；extension log 行首时间字符串走 `parse_log_timestamp_ms`（**本地 naive 优先**，退回 `parse_timestamp_str`，`:426-462`），失败用 mtime。
- token：同 `tencent_buddy.rs:92-125`；**input 是「独占」语义**（`:149-162`，示例 `:41-44`：`24486` 含 `14720` cache_read ⇒ input = `9766`）。
- provider/model：provider = `inferred_provider_from_model` 否则 `"tencent"`（`tencent_buddy.rs:251-253`）；model ← `providerData.model` → `requestModelId` → `message.model` → 默认名（`:240-250`）；extension log 从 `"Model prepared:"` 行取（`:313-314`、`:404-414`）。
- dedup_key：前缀 `"codebuddy"`，格式同 `tencent_buddy.rs:274-279`、`:368-378`。workspace：有（`cwd` 归一化 `:269-272`；日志路径推导 `:380`、`:464-471`）。message_count / agent：不设。

##### Droid（`droid`）
- `droid.rs:419` `parse_droid_file`；调用点 `lib.rs:4592-4604`。
- 数据：注释 `:3` `~/.factory/sessions/`；注册 `clients.rs:473-482`（`Home` + `.factory/sessions` + `pattern: *.settings.json`）；同目录同名的 `*.jsonl` 是 transcript，由 `droid_jsonl_path` 换名得到（`:145-149`）。
- `session_id`：文件名去扩展后 `replace(".settings", "")`（`uuid.settings.json` → `uuid`），取不到用 `"unknown"`（`:447-453`）。
- 时间戳：有 transcript 时用各 turn 的 `$.timestamp` 走 `parse_timestamp_value` 并**过滤非正值**（`:313-319`）；无 transcript 时单条兜底 `resolve_usage_timestamp(lock, mtime)`（`:550-551`、`:186-193`：两者都取 max；只有一个取该值；都无取 `chrono::Utc::now()`）；`lock = providerLockTimestamp` 走 RFC3339 → ms 且要求 >0（`:161-165`）。
- token：`tokenUsage.inputTokens/outputTokens/cacheReadTokens/cacheCreationTokens/thinkingTokens` → 五桶，各 `.max(0)`（`:474-480`，字段声明 `:27-39`）；总数非零才继续（`:437-445`）；**分片按权重摊分**（`:485-503`、`:399-416`、`:321-328`：`input/cache_read/cache_write` 用 `context_weight`，`output/reasoning` 用 `output_weight`）。
- provider/model：provider = `providerLock` 原样（**未过 `canonical_provider`**），缺失由模型推断否则 `"unknown"`（`:456-458`、`:93-95`）；model = `settings.model` → `normalize_model_name`（`:460-461`、`:45-91`：去 `custom:`、删 `[..]`、去尾连字符、小写、`.` → `-`、合并连续 `-`）；缺失从 JSONL 前 500 行找 `"Model:"`（`:464-467`、`:113-140`）；再缺失用 `{provider}-unknown` / `claude-unknown` / `gpt-unknown` / `gemini-unknown` / `grok-unknown`（`:468-471`、`:98-109`）。
- dedup_key：**无**（全文件无赋值）。message_count：分片路径 `if index == 0 { session_replies } else { 0 }`（`:540`，`session_replies` 为各 turn `replies.max(1)` 之和 `:507-509`；注释 `:530-539` 解释「权威回复数只挂第一条，以免 sessionize 把分片当独立会话」）；兜底单条不设 ⇒ 默认 1。agent：不设。workspace：无（grep 命中 0）。

##### OpenClaw（`openclaw`）
- `openclaw.rs:80` `parse_openclaw_transcript`（内部 `parse_openclaw_session` `:116`）；调用点 `lib.rs:4606-4618`（**该 lane 无去重过滤**）。
- `parse_openclaw_index`（`:55`）**在生产代码里没有调用点**（grep 只命中定义与同文件测试）。
- 数据：注释 `:3-4` transcript JSONL + legacy `sessions.json` 索引；注册 `clients.rs:483-492`（`Home` + `.openclaw/agents` + `pattern: *.jsonl*`，`scanner.rs:424-430` 说明该模式也接 `.jsonl.deleted.<ts>` / `.jsonl.reset.<ts>` 归档名）。
- `session_id`：文件名 `split_once(".jsonl")` 取前段，空则整文件丢弃（`:81-92`；测试 `:287-321` 确认归档名归同一 id）；索引路径用 `$.sessionId`（`:19-24`、`:72`），文件定位 `:97-114`。
- 时间戳：`message.timestamp`（i64）**原值直接用，不做秒/毫秒判定**（`:191`），缺失用 transcript 文件 mtime ms（`:118-123`）。
- token：`usage.to_breakdown()`（`:200` → `utils.rs:611-622`：`input/output/cacheRead/cacheWrite` 各 `.max(0)`，**`reasoning` 恒 0**；`totalTokens` 只反序列化不参与求和 `:604-607`）；`usage` 缺失的 assistant 行丢弃（`:167-170`）；cost 取 `usage.cost.total`（`:192`）。
- provider/model：provider = `message.provider` → 先前 `currentProvider`（`model_change` 行 `:139-146`，或 `custom + customType == "model-snapshot"` `:147-160`）→ 兜底 `"unknown"`（`:177-182`），**直传字符串、无 `canonical_provider`**；model = `message.model` → `currentModel`，皆空则整条丢弃（`:172-176`、`:184-187`）。
- dedup_key：无。message_count / agent / workspace：不设。

#### 3.5.9 批三：其余 19 个客户端（【代核实】，生产代码区逐行读过）

> 这一批的共同点：**多数是「JSON/JSONL/SQLite 三选一 + 一套 token 桶映射」的独立实现**，其中 3 个（Senpi / Omp / Kimchi）直接复用 Pi 的解析器，只是换了 namespace 与兜底 provider。

##### fx（`fx`，Zig 写的 coding agent）

> ⚠️ **通道 A 不产出任何 fx 消息**：`ClientId::Fx` / `sessions::fx` 在 `parse_local_clients` 分派区段（`lib.rs:4367-5246`）**零命中**，`fx.rs` 只被通道 B 调用（`lib.rs:2510` `scan_result.get(ClientId::Fx)`、`:2520` `parse_fx_file`、`:2531` `apply_session_titles`）。⇒ **以下字段事实对 UsageMesh 全部不可达**，仅作 v4.14.0 解析器清单存档（`parse_local: true` 却无通道 A 分派，见 §3.3.4）。
- 入口：`fx.rs:263` `parse_fx_file`；另有 `apply_session_titles`（`fx.rs:214`）与 `fx_session_meta_path`（`fx.rs:157`）。
- 数据：注释 `fx.rs:3-6`——`~/.fx/sessions/<sessionId>/usage-v2.json`，配兄弟 `session.json`（workspace 根 + 权威时间戳）与共享 `~/.fx/sessions/index.json`（标题）；`~/.fx/usage.jsonl` 存在但**不扫描**（`fx.rs:15-17`）。`index.json` 结构 `{"sessions":[{"id","title"}]}`（`fx.rs:164-167`、`:178-186`）。
- `session_id`：顶层 `session_id` 非空优先，否则目录名，否则空串（`fx.rs:277-286`）。
- 时间戳：`updated_at_ms` 次选 `created_at_ms`（字段 `fx.rs:118`、`:120`；取值 `:292-294`），**单位毫秒直用**；fallback = 文件 mtime（`:295-296`）。
- token：model 级 `input_tokens/output_tokens/cache_read_tokens/cache_write_tokens/reasoning_tokens` → 五桶（`fx.rs:305-309`，字段 `:95-106`）；无 per-model 条目时用顶层同名聚合（`:358-362`）。**这是少数五个桶都填的客户端。**
- provider/model：`split_model` 按首个 `/` 拆（`:250-257`）；provider 先 `canonical_provider`（`:316-318`）再 `inferred_provider_from_model`（`:319-321`），兜底字面量 `"zai"`（`:322`）；合成兜底消息硬编码 `"zai"`（`:370`）。
- dedup_key：`format!("fx:{session_id}:{model_id}")`（`:324`）；兜底 `format!("fx:{session_id}:{UNKNOWN_MODEL}")`（`:382`，`UNKNOWN_MODEL = "fx-unknown"` `:46`）。
- **message_count：`model_usage.request_count.unwrap_or(0).max(0) as i32`（`:340`）**——语义是「该模型在这个会话里的请求次数」。**但这条 lane 不在通道 A 内，UsageMesh 收不到它**，所以它不是 UsageMesh 的请求数口径来源（此前写成「重要来源」是错的）。
- agent：显式 `None`（`:341`、`:381`）。workspace：`session.json.workspace_root` → 归一化（`:291`、`:298-299`、`:332-333`）。

##### mux
- 入口：`mux.rs:47` `parse_mux_file`。数据：`~/.mux/sessions/<workspaceId>/session-usage.json`（`mux.rs:1-3`）。
- 结构：`MuxSessionUsage{version,byModel,lastRequest}`（`:12-20`）、`MuxModelUsage{input,cached,cacheCreate,output,reasoning}`（`:22-30`）、`MuxTokenBucket{tokens,cost_usd}`（`:32-36`）、`MuxLastRequest{model,timestamp}`（`:38-43`）。
- `session_id`：`session-usage.json` 的父目录名（workspaceId）（`:63-67`），**取不到时 `unwrap_or_default()` 可为空串**。
- 时间戳：`lastRequest.timestamp`（Option<i64>，`:42`、`:57-60`）；**源码无单位说明**，测试值 `1700000000000` 且断言原样保留（毫秒量级，`:170`、`:189`）；fallback 文件 mtime（`:61`）。
- token：`byModel.<key>.input.tokens` → input、`.cached.tokens` → cache_read、`.cacheCreate.tokens` → cache_write、`.output.tokens` → output、`.reasoning.tokens` → reasoning（`:81-85`、映射 `:121-127`）；负值 `.max(0)`（`:78`）。**也是五桶齐全的客户端之一。**
- provider/model：modelKey 内首个 `:` 用 `splitn(2, ':')` 拆（`:105-112`）；provider 过 `canonical_provider`（`:113`）；**无 `:` 时 provider = 空串**（`:111`，测试 `:249`）。
- dedup_key：`format!("mux:{session_id}:{model_key}")`（`:102`）。message_count：不设 ⇒ 1。workspace：无（workspaceId 只进 `session_id` 与 dedup_key）。
- cost：五个 bucket 的 `cost_usd` 求和后**未做 provenance 判定**直接传进构造器（`:79-90`、`:128`）——即 cost 有值但 `cost_source` 保持默认 `Unknown`。**通道 A 反正会丢掉这个 cost。**

##### Senpi / Omp / Kimchi（Pi 派生三件套）
| 客户端 | 入口 | 委派目标 | dedup namespace | 兜底 provider |
|---|---|---|---|---|
| Senpi | `senpi.rs:20-21` | `parse_pi_format_file(path, "senpi", "senpi")` | `None` ⇒ 无 dedup_key | `"senpi"` |
| Omp | `omp.rs:27-28` | `parse_pi_format_file(path, "omp", "omp")` | `None` ⇒ 无 dedup_key | `"omp"` |
| Kimchi | `kimchi.rs:11-12` | `parse_pi_format_file_with_dedup(path, "kimchi", "kimchi")` | `"kimchi"` ⇒ `"kimchi:{session_id}:{message_id}"`（`pi.rs:726`，非跨会话分支 `:723-727`） | `"kimchi"` |

- Senpi 专属注释 `senpi.rs:5-9`：**明确不把 `usage.reasoning` 计入**（与 Pi 的 reasoning 恒 0 一致，测试 `:71-97`）。
- Omp 专属注释 `omp.rs:7-20`：子代理写在自己同名目录下的 `<timestamp>_<uuid>/<AgentName>.jsonl`，advisor 写 `__advisor.<name>.jsonl`；**扫描根固定、刻意不读 `PI_CODING_AGENT_DIR`**，理由是与 Pi 共用该变量会导致同一棵树被两个客户端各计一次（重复计费）。
- Kimchi 的路径注释只写「Pi-compatible JSONL format under its own agent directory」（`kimchi.rs:3-4`），**未给出具体 `~` 路径**——路径线索在注册表 `clients.rs:859-871`（`EnvVar KIMCHI_CODING_AGENT_DIR` / `.config/kimchi/harness` + `sessions`）。
- 三者的 session_id / 时间戳 / token 桶 / agent / workspace 全部继承 `pi.rs`（见 §3.5.7）。

##### reasonix
- 入口：`reasonix.rs:81` `parse_reasonix_file`。数据：`<REASONIX_HOME>/stats/YYYY-MM-DD.jsonl`，**每行一个 provider 请求**（`reasonix.rs:3-4`）；transcript JSONL **刻意不扫描**（`:4-5`）。
- `session_id`：**没有原生字段**，用 `format!("reasonix-stats:{}", path.display())`（`:120`）⇒**一条会话 = 一个日文件**。
- 时间戳：`ts`（`serde_json::Value`，`:21`）→ `utils::parse_timestamp_value`（`:97`）；**解析失败该行丢弃**（`?`），无 mtime fallback。
- token 桶（`:99-115`）：`prompt` → input；`cache_hit` → cache_read；`cache_miss`（存在且非 0）→ input，否则 `input = prompt - cache_hit`；`reasoning` → reasoning（**≤ completion**）；`completion` → `output = completion - reasoning`；**cache_write 恒 0**。
- provider/model：`split_model_ref`（`:51-75`）——含 `/` 时 provider 过 `canonical_provider`（`:64`），模型段损坏用 `inferred_provider_from_model_delimited` 并兜底 `"reasonix"`（`:59-63`）；无 `/` 时 `inferred_provider_from_model` 否则 `"reasonix"`（`:71-74`）。
- dedup_key：`format!("reasonix:{}:{}:{}:{}", path.display(), line_index, record.requests, record.total)`（`:124-130`）。
- **message_count：`record.requests.clamp(1, i32::MAX as i64) as i32`（`:132`）**——下限 1。
- 过滤：`record.turn` 为真跳过（`:91`）；`total<=0 && requests<=0` 跳过（`:93`）。
- agent / workspace：不设。

##### cherrystudio
- 入口：`cherrystudio.rs:248` `parse_cherrystudio_file`。数据（`:3-9`）：`%APPDATA%\CherryStudio\Data\Agents\.claude\projects\<workspace>\<session>.jsonl`（macOS `~/Library/Application Support/...`，Linux `$XDG_CONFIG_HOME/...`），**V1 根省略 `Data/Agents`，两处根都扫描**；格式是标准 Claude Code 转录，用量取 assistant 事件 `message.usage`（`:25-27`）。
- `session_id`：文件名 stem（`:253-257`），兜底 `"unknown"`。
- 时间戳：顶层 `timestamp` 字符串 → `parse_timestamp_str`（`:344-347`）；fallback 文件 mtime（`:249`、`:348`）；流式合并以「最大且存在的 event timestamp」所在行的元数据为准（`:185-195`）。
- token：`usage.input_tokens/output_tokens/cache_read_input_tokens/cache_creation_input_tokens` → 四桶（读数 `:273-303`，组装 `:350-356`）；**合并逐桶取最大**（`:198-205`）；`reasoning` 不填。
- provider：`provider_for_model` 按关键字猜（`:210-226`：deepseek→`"deepseek"`、claude→`"anthropic"`、gpt/o1/o3/o4/`sol-` 结尾→`"openai"`、否则 `"unknown"`）；model 取 `message.model`（`:284-289`），**空串 / `<synthetic>` / 大小写不敏感 `"unknown"` 一律丢弃**（`:290-292`）。
- dedup_key：**不设**；跨行去重走**别名并查集** `dedupe_usage_records`（`:112-174`），别名前缀 `request:` / `message:` / `uuid:`（`:333-342`）。
- workspace：有——路径窗口 `.claude/projects/<slug>` → `workspace_from_path`（`:231-244`、写入 `:368-370`）。message_count / agent：不设。

##### gjc（Gajae-Code）
- 入口：`gjc.rs:96` `parse_gjc_file`。数据：`~/.gjc/agent/sessions/<project-slug>/*.jsonl`（depth-2 子代理在 `<slug>/<session>/N-*.jsonl`）（`:3-4`）；行类型 `session`（header）、`service_tier_change`（跳过）、`message`（仅 assistant）（`:6-12`）。
- `session_id`：`session` 记录 `id`（`:114-118`）；无 header 用文件名 stem（`:183-188`，注释 `:173-182`）。
- 时间戳：优先 message `timestamp`（**unix 毫秒**，字段 `:54`）（`:161`）；否则记录级 `timestamp`（ISO-8601，字段 `:38`）→ RFC3339 → `timestamp_millis`（`:162-166`）；再否则文件 mtime（`:97`、`:166`）。
- token：`usage.{input,output,cacheRead,cacheWrite}` 走 `CamelUsage::to_breakdown`（`utils.rs:599-622`，**reasoning 恒 0** `utils.rs:620`），调用 `gjc.rs:169`。
- provider：message `provider`（非空）否则 `inferred_provider_from_model`，兜底 `"gjc"`（`:152-156`），**无 `canonical_provider`**；model：message `model`（缺失跳过）（`:144-147`）。**client 可被 message 的 `source` 字段覆盖**（`:194-198`，空/空白回落 `"gjc"`）——这是「9Router 桥」数据以 client 值进入的机制，与 `lib.rs:198` 的 `client.eq_ignore_ascii_case("9router")` 特判呼应。
- dedup_key：有 message id → `format!("{session}:{msg_id}")`（`:189-190`）；否则 `"gjc:{session_id}:{ts}:{model}:{provider}:{i}-{o}-{cr}-{cw}-{r}:{line_hash:x}"`（`:82-89`，`line_hash` = 整行 sha256 `:80`）。
- workspace：有（`session.cwd` → 归一化 `:119-121`、写入 `:212`）。cost：`embedded_cost` 仅 present+finite+≥0 时标 `ProviderReported`（`:60-65`、`:209-211`）。

##### junie
- 入口：`junie.rs:21` `parse_junie_file`。数据：`~/.junie/sessions/<session-id>/events.jsonl`（`:3`）；kind 常量 `LlmResponseMetadataEvent`（`:13`）、`UserPromptEvent`（`:14`）、跳过列表（`:15-19`）。
- `session_id`：`events.jsonl` 的父目录名（`:163-170`，兜底 `"unknown"`）。
- 时间戳：`timestampMs`（数值，需 > 0，**毫秒直用**）（`:67-68`）；fallback 先由 session id 解析 `session-YYMMDD-HHMMSS`（**Local 时区**转毫秒）（`:23-24`、`:172-193`），再否则文件 mtime（`:24`）；**并在显式时间戳与 `usage.time` 同时存在时用 `back_anchor_timestamp(end, duration)` 回锚**（`:126-130` → `utils.rs:479-483`），`duration_ms` 写入 `:143`。
- token（`:224-245`，取值器 `:255-280`）：input ← `inputTokens|input`；output ← `outputTokens|output`；cache_read ← `cacheInputTokens|cacheReadInputTokens|cacheRead`；cache_write ← `cacheCreateTokens|cacheCreationInputTokens|cacheWrite`；reasoning ← `reasoningTokens|reasoningOutputTokens|thinkingTokens`。
- provider：`usage.provider` → `canonical_provider`，否则 `inferred_provider_from_model`，兜底 `"junie"`（`:217-222`）；**model 走 `pricing::aliases::resolve_alias`**（`:88-90`）。
- dedup_key：`"junie:{session_id}:{timestamp}:{model_id}:{i}:{o}:{cr}:{cw}:{r}:{cost:.12}:{usage_index}"`（`:100-108`），文件内再 `seen` 二次去重（`:109-111`）。
- agent：有——`agentEvent.agent.name` 否则 `.id`（`:210-215`）。is_turn_start：`UserPromptEvent` 置位（`:49-52`），首个 usage 消费（`:147-150`）。cost：`usage.cost` 有限且 ≥0 时标 `ProviderReported`（`:93-95`、`:144-146`）。

##### mcode（MiniMax Code）
- 入口：`mcode.rs:29` `parse_mcode_file`。数据：`mcode exec --output-format stream-json` 的输出 JSONL（由 tokscale 自己捕获），逐行投影事件 + 一条 `exec.result`；**usage 在 assistant `message` 事件内，模型/供应商只在 `exec.result` 出现**，所以先缓存后配对（`:3-8`、`:81-85`、`:122-147`）。
- `session_id`：`exec.result.sessionId`（非空）（`:95-102`）——usage 事件本身不带 session id。
- 时间戳：`message.timestamp` → `normalize_timestamp`（`:170-176`：`0 < t < 1e10` 视为秒 ×1000，否则毫秒原样）；缺字段用文件 mtime（`:33`、`:72-77`）；`<=0` 丢弃（`:78-80`）。
- token：`usage.inputTokens/outputTokens/cacheReadTokens/cacheWriteTokens` → 四桶，**reasoning 恒 0**（`:156-164`；负值取 0 `:166-168`）。
- provider/model：`exec.result.model.providerId` / `.modelId` **原样**（`:103-121`），无归一化。
- dedup_key：`"mcode:{session_id}:{turn_id}:{index}:{input}:{output}:{cache_read}:{cache_write}"`（`:127-133`）。
- agent：**显式 `Some("headless")`**（`:144`）；is_turn_start：`index == 0`（`:145`）。

##### opencodereview
- 入口：`opencodereview.rs:15`。数据：`~/.opencodereview/sessions/<encoded-repo-path>/<session-id>.jsonl`（`:3-4`）；`session_start` 带 `cwd`，`llm_response` 带 usage/timestamp/duration_ms（`:34-47`、`:73-76`）。
- `session_id`：文件名 stem（`:145-151`，兜底 `"unknown"`）。
- 时间戳：记录 `timestamp` → `parse_timestamp_value`（`:58`）；缺省文件 mtime（`:17`、`:59`）；**有显式 timestamp 且 `duration_ms > 0` 时用 `back_anchor_timestamp` 回锚**（`:91-94`），`duration_ms` 写入 `:130`。
- token：`usage.prompt_tokens/completion_tokens/cache_read_tokens/cache_write_tokens` → 四桶，**reasoning 恒 0**（`:153-161`；取值器 clamp ≥0、u64 溢出取 `i64::MAX` `:163-172`）。
- model：记录 `model`（默认 `"unknown"`）**走 `resolve_alias`**（`:61-67`）；provider 由 `inferred_provider_from_model` 否则 `"opencodereview"`（`:69-71`）。
- dedup_key：`"opencodereview:{session_id}:{recorded_timestamp}:{model_id}:{in}:{out}:{cr}:{cw}"`，无显式 timestamp 时追加 `:line{index}`（`:108-115`）+ `seen` 去重（`:116-118`）。workspace：有（`session_start` 的 `cwd`，只取首个，`:34-38`、`:132-137`）。

##### cline
- 入口：`cline.rs:15` `parse_cline_file`（先判 `is_cline_cli_messages_path` 分发，`:16-18`），任务格式委托 `parse_roo_kilo_file(path, "cline")`（`:20`）；CLI 入口 `parse_cline_cli_file`（`:136`）。
- 数据：与 Roo Code / Kilo 同源的 VS Code `globalStorage` 任务日志（`:3-5`）；CLI 路径 `~/.cline/data/sessions/<session>/<session>.messages.json`（`:134-135`），判别「文件名以 `.messages.json` 结尾」（`:87-91`），manifest 路径推导（`:93-102`）。
- `session_id`：CLI = `file.session_id` → `manifest.session_id` → 文件名 stem 去 `.messages` → `"unknown"`（`:147-155`）；任务格式 = `ui_messages.json` 父目录名（`roocode.rs:87-94`）。
- 时间戳：CLI 条目 `ts` → `parse_timestamp_value`（`:227-231`），fallback 文件 mtime（`:156`）；任务格式 `ts` → `parse_timestamp_str`（`roocode.rs:165-176`），**解析失败跳过条目、无 mtime 兜底**（`roocode.rs:54-57`）。
- token：CLI `metrics.inputTokens/outputTokens/cacheReadTokens/cacheWriteTokens`，且 **`input = input_tokens - cache_read - cache_write`**（`:201-215`），reasoning 恒 0（`:248`）；任务格式内嵌 JSON `tokensIn/tokensOut/cacheReads/cacheWrites`（`roocode.rs:192-195`、映射 `:72-78`）。
- provider/model：CLI 先取 manifest 的 `model`/`provider`（空则 `"unknown"`）（`:170-173`），每条 assistant 的 `modelInfo.id`/`modelInfo.provider` 覆盖（`:189-196`），无归一化；任务格式 provider 取 `apiProtocol` **原文**（`roocode.rs:220-226`），model 取 `api_conversation_history.json` 里 `<environment_details>` 的 `<model>`（`roocode.rs:113-148`）。
- dedup_key：CLI 有 id → `"cline-cli:{session_id}:{id}"`，否则 `"cline-cli:{session_id}:{assistant_index}"`（`:232-236`）；任务格式 `None`。workspace：CLI 有（manifest `workspace_root` 否则 `cwd`，`:157-162`、`:256`），任务格式无；session_title：CLI 取 `manifest.metadata.title`（`:163-168`、`:255`）——**会被 `unified_to_parsed` 丢掉**。

##### kilocode
- 入口：`kilocode.rs:9-10`，委托 `parse_roo_kilo_file(path, "kilocode")`。数据：源码注释只写「与 Roo Code 相同的任务日志格式并复用同一 helper」（`:3`），**未给具体路径**；实际读 `ui_messages.json` 并与同目录 `api_conversation_history.json` 配对（`roocode.rs:96-111`）。
- `session_id`：`ui_messages.json` 父目录名（taskId）（`roocode.rs:87-94`）。时间戳：条目 `ts` → `parse_timestamp_str`（`roocode.rs:165-176`），**无 fallback**。token：`tokensIn/tokensOut/cacheReads/cacheWrites` → 四桶（`roocode.rs:187-209`、`:72-78`）。
- provider：`apiProtocol` 原文否则 `"unknown"`（`roocode.rs:220-226`）；model：`<model>`（`roocode.rs:113-148`）。dedup_key：`None`。agent：`<slug>` 优先其次 `<name>`（`roocode.rs:135-147`）。workspace：无。

##### codebuff
- 入口：`codebuff.rs:27`。数据：`~/.config/manicode/projects/<project>/chats/<chatId>/chat-messages.json`（用量在 `metadata.usage`、`metadata.codebuff.usage` 或 `metadata.runState.sessionState.mainAgentState.messageHistory[*].providerOptions`），`run-state.json` **不消费**（`:3-10`）；dev/staging 根 `manicode-dev`/`manicode-staging`（`:12-13`）；**chatId 是 ISO-8601 把 `:` 换成 `-`**（`:13-14`）。
- `session_id`：`format!("{channel}/{project_basename}/{chat_id}")`（`:43`，三段由 `derive_context_from_path` `:152-180` 推导，缺失时 `"unknown"/"unknown"/"manicode"`）。
- 时间戳：`message_timestamp`（`:191-203`，依次 `timestamp`、`createdAt`、`metadata.timestamp`）→ chatId 解析 `parse_chat_id_to_millis`（`:136-145`，只在 `T` 后还原前两个 `-`）→ 文件 mtime（`:46`、`:59-66`）。
- token：`extract_assistant_usage`（`:249-276`）按 `metadata.usage` → `metadata.codebuff.usage` → RunState messageHistory（倒序）依次尝试（`:281-326`）；别名 input ← `inputTokens|input_tokens|promptTokens|prompt_tokens`；output ← `outputTokens|output_tokens|completionTokens|completion_tokens`；cache_read ← `cacheReadInputTokens|cache_read_input_tokens|cachedTokensCreated|cached_tokens_created`（另有 `promptTokensDetails.cachedTokens` 回退）；cache_write ← `cacheCreationInputTokens|cache_creation_input_tokens|cacheCreationTokens|cache_creation_tokens`（`pick_number` 只接受 > 0）（`:330-410`）；**reasoning 恒 0**（`:88`）。
- provider/model：model 取 `usage.model` 否则 `"codebuff-unknown"`（`:68-71`、常量 `:24`）；provider = `inferred_provider_from_model` 否则 `"unknown"`（`:72`）。
- dedup_key：优先上游 `msg.id`（`:74`、`:100-105`）；否则 `"codebuff:{session_id}:{ts}:{model}:{ordinal}:{i}:{o}:{cr}:{cw}"`（`:111-125`）。

##### freebuff（与 codebuff 共享扫描桶的免费构建）
**这是「同一份文件被两个客户端解析」的判别机制**，对理解 `lib.rs:4351-4356` 的 `include_codebuff` / `include_freebuff` 门控很关键：
- `freebuff.rs:52` 常量 `FREEBUFF_ROOT_AGENT_PREFIX = "base2-free"`；该 id 从 `metadata.runState.sessionState.mainAgentState.agentType` 读（`root_agent_id` `:58-66`）；**任一条消息的该字段以 `base2-free` 开头即判为 Freebuff**（`is_freebuff_chat` `:69-74`）。
- **无标记（无已完成回合 / 旧版 CLI）⇒ 直接返回空、不猜测**（`:132-137`）。
- 反向护栏：聊天已存在带信号的 assistant usage 时同样返回空，避免与 codebuff 双计（`:139-147`；`is_assistant_role` `codebuff.rs:182-189`、`has_signal` `codebuff.rs:216-222`）。
- 判定**不基于位置、也不基于「无用量」推断**（`:8-11`）。
- token：**完全估算** —— `estimate_tokens(chars)`（`utils.rs:487-489`，约 4 字符 = 1 token 向上取整）；`input` 为该 assistant 之前累计的 user/tool 文本字符（输出后清零），`output` 为该 assistant 消息文本（`:163-186`）；**cache_read / cache_write / reasoning 恒 0**（`:206-208`，注释 `:27-30`：本地不存 token 用量、只有恒 0 的 credits）。
- model 从 channel 根 `settings.json` 的 `freebuffModel` 读（`model_from_settings`，上溯五级目录）（`:96-112`、`:155`），兜底 `"freebuff-unknown"`（`:47`）；provider = 推断否则 `"unknown"`（`:156`）。
- dedup_key：`format!("{session_id}:{assistant_index}")`（`:211`）；**`message_count` 显式 = 1**（`:213`）；`is_turn_start` 由 `pending_turn_start`（`:214`）。

##### qwen
- 入口：`qwen.rs:75`；另有 `extract_session_id_with_fallback`（`:47`）。数据：`~/.qwen/projects/{projectPath}/chats/*.jsonl`，token 来自带 `usageMetadata` 的 assistant 消息（`:3-4`）。
- `session_id`：行内 `sessionId` 非空优先（`:49-53`），否则 `"{project_name}-{filename}"`（项目名取 `chats` 的祖父目录名）（`:57-71`）。
- 时间戳：行 `timestamp` → `parse_timestamp_str`（`:102-104`），fallback 文件 mtime（`:76`、`:105`）。
- token：`usageMetadata.promptTokenCount` → input、`candidatesTokenCount` → output、**`thoughtsTokenCount` → reasoning**、`cachedContentTokenCount` → cache_read（`:108-111`），**cache_write 恒 0**（`:112`）；全零跳过（`:115-117`）。
  ⚠️ **`thoughtsTokenCount` 这个字面量只出现在 `qwen.rs`（`:32` 的 serde rename 起，至 `:499` 的测试 fixture 止）与 `codex.rs:206` 的注释**——§3.5.4 里说的「Gemini 用 thoughtsTokenCount」需要改写为 Qwen 才有该字段名。
- provider：常量 `DEFAULT_PROVIDER = "qwen"`（`:40`、`:132`，不推断不归一化）；model：行 `model` 否则 `"unknown"`（`:39`、`:120`）。dedup_key：`"qwen:{line_session_id}:{message_index}"`（`:126`）。workspace：有（反向扫 4 元素窗口匹配 `projects/<x>/chats`，`:152-167`、写入 `:145`）。

##### amp
- 入口：`amp.rs:273`。数据：`~/.local/share/amp/threads/` 下线程 JSON（`:3`）。
- `session_id`：`thread.id`，缺失用文件名 stem（`:292-297`，兜底 `"unknown"`）。
- 时间戳：ledger 事件 `timestamp`（RFC3339）→ `parse_amp_timestamp`（`:107-112`）；fallback `thread.created` → 文件 mtime（`:114-123`）；messages 记录时间 = base（created 或 mtime）**+ `messageId * 1000`**（`:179-195`）——即用人造偏移把同线程的多条消息摊开。
- token：ledger（`:157-163`）与 message（`:203-209`）两路，字段 `input/output/cacheReadInputTokens/cacheCreationInputTokens`；**reasoning 恒 0**（`:162`、`:208`）。
- provider：`get_provider_from_model` → `inferred_provider_from_model`，兜底 `"anthropic"`（`:74-76`）；model 缺失则不产出（`:140`、`:193`）。**dedup_key：`None`**（用 `UnifiedMessage::new` `:94-104`）。cost：`credits`（`:164`、`:210`，合并规则 `:241-270`）。

##### roocode
- 入口：`roocode.rs:23`（source `"roocode"`）；共享实现 `parse_roo_kilo_file(path, source)`（`:27`，被 `cline.rs:20` 与 `kilocode.rs:10` 复用）。数据：VS Code `globalStorage` 的 `tasks/<taskId>/ui_messages.json` + `api_conversation_history.json`（`:3-5`）。
- 其余字段见上「cline / kilocode」两条（同一实现）。**dedup_key 为 `None`**、**无 workspace**、**agent 取 `<slug>` 优先其次 `<name>`**、cost 取内嵌 JSON `cost`（`:191`、`:79`）。

##### antigravity（IDE 侧）
- 入口：`antigravity.rs:6` `parse_antigravity_file`；辅助 `parse_usage_row`（`:48`）。
- **数据路径：未查到**——该文件无模块级注释与路径说明（第 1 行即 `use super::UnifiedMessage;` `:1`）。路径线索只在注册表 `clients.rs:619-628`（`PathRoot::Config` + `antigravity-cache/sessions` + `*.jsonl`）。
- 格式：JSONL，行 `type` 为 `session_meta`（`sessionId`/`modelId`）与 `usage`（用量行）（`:25-42`）。
- `session_id`：`usage` 行的 `sessionId`（`:49`，缺失整行跳过）。时间戳：`timestamp` 数值经 `to_safe_i64`（i64/u64/字符串，负数 clamp 0）（`:50`、`:111-121`），`<= 0` 丢弃（`:51-53`）；**无 mtime fallback、无秒/毫秒换算**（测试值 `1711200000000` 毫秒 `:130`）。
- token：`input/output/cacheRead/cacheWrite/reasoning` → 五桶（`:74-78`），五项全 0 丢弃（`:79-81`）。
- provider：`providerId` 原样经 `canonical_provider`（`:66-72`），缺失时 `inferred_provider_from_model` 兜底 `"antigravity"`（`:107-109`）；model：usage 行 `modelId` → session_meta 行 `modelId` → `"unknown"`（`:55-61`），**再过 `resolve_alias`**（`:62-64`，占位名解析测试 `:146-157`）。
- dedup_key：取 `responseId` 原样（trim 非空），缺失即 `None`（`:83-87`）。agent / workspace：不设。

#### 3.5.10 批四：此前的 13 个空白客户端（【代核实】）

##### zcode（ZCode）
- 入口：`parse_zcode_file:106`（JSONL）、`parse_zcode_sqlite:279`；路径 `~/.zcode/projects/<slug>/<session>.jsonl`（`:3`），SQLite 在 `~/.zcode/cli/db/db.sqlite`（`scanner.rs:111-112`、发现 `scanner.rs:2095-2097`），注册 `clients.rs:767-776`。
- 常量：`CLIENT_ID` / `PROVIDER_ID = "zhipu"` / `UNKNOWN_MODEL = "glm-5.2"`（`:27-29`）——**provider 固定为 `zhipu`**。
- `session_id`：JSONL 行内 `sessionId`（`:42-43`、`:126-130`）否则 stem（`:108`、`:177-179`）；SQLite `:392`。
- 时间戳：JSONL `timestamp` → `parse_rfc3339_ms:586-590`（调用 `:180-184`），fallback mtime（`:107`）；SQLite `resolve_zcode_timestamp:528-544`（`started_at > 0` 优先，否则 `completed_at − duration_ms`（`:539`），否则 `completed_at`，否则 mtime），duration 写 `:473`。
- token：JSONL 别名块 `:47-73` → `to_breakdown:76-103`，含**包含式归一**（`:246-277`）与**估算回退**（`:156-173`）；SQLite `:405-451`（legacy 无条件相减 `:428-434`）。
- model：`canonicalize_model`（**只做 `to_lowercase`**）`:569-571`（调用 `:132-138`、`:393-397`）。
- dedup：JSONL `"{session}:{idx}"`（`:194`）；SQLite `"zcode-sqlite:{id}"`（`:472`）。`message_count`：JSONL = 1（`:196`）、SQLite 默认 1。agent：**仅 SQLite**（`row.agent` 否则 `row.mode`，`:457-471`）。workspace：JSONL `:109-110`/`:198`、SQLite `:475-478`。

##### dsh（DeepSeek Harness）
- 入口：`parse_dsh_file:193`；路径 `<DSH_HOME>/sessions/<encoded-cwd>/<session-id>/session.jsonl.zstd`（`compression: none` 时 `session.jsonl`），按 zstd magic 分派（`:3-8`、magic `:36`、`:139-149`）；magic 判定 `:36`，上限 `:55`/`:73`。
- 事件：`session`（`:234-242`）、`request/header`（`:243-253`）、`assistant/message` + `compaction/summary`（`:264-400`）；usage 在 `/data/usage`（`:281`）。
- **token 桶**【核验】：`tokens_from_usage:423-433` —— `input ← inputTokens`；**`output ← outputTokens − reasoningTokens`**；`cache_read ← cacheReadTokens`；`cache_write ← cacheWriteTokens`；`reasoning ← reasoningTokens`。⇒ **DSH 是唯一把 `cacheWriteTokens` 直接映射进 `cache_write` 桶的非 Claude 客户端之一**。
- `session_id`：`session.id`（`:235`）否则目录名（`:199-207`、`:309-311`）。时间戳：事件 `time`（i64）**直接使用**，缺失或 ≤0 跳过（**无 mtime 回退、无换算**）（`:288-293`）。provider/model：优先 `/data/message/source`，否则 header config，最后 `"unknown"`（`:295-307`，**无归一化**）。
- dedup：`"dsh:{kind}{identity}:{ts}:{provider}:{model}:{in}:{out}:{cr}:{cw}:{rs}"`（`:370-377`），identity = `msg:{id}` / `seq:{n}` / `sid:{sid}`（`:351-363`），kind = `summary:`（`:369`），另有 `seen`（`:378-380`）。workspace 来自 `session.cwd`（`:236`、`:393-398`）。

##### jcode
- 入口：`parse_jcode_file:293`；journal 路径 `jcode_journal_path:189-200`；数据 `~/.jcode/sessions/session_*.json` + `*.journal.jsonl`（`:3`、`:185-188`）。
- `session_id`：`id` 否则 stem（`:311-316`）。时间戳：message `timestamp` → `parse_timestamp_str`（`:246`），**仅当显式时间戳存在时**用 `back_anchor_timestamp(end, tool_duration_ms)`（`:261-265`），fallback 快照 mtime（`:317`）。注释记录了「把时间锚回轮次开始」的修正（对应 `message_cache.rs:1069-1071` 的 v4→v5 版本说明）。
- token：`tokens_from_usage:110-130` —— `input ← input_tokens`（**OpenAI 式：减掉 `cache_read` 子集**，判定 `:101-108`）；`output ← output_tokens`；`cache_read ← cache_read_input_tokens`；`cache_write ← cache_creation_input_tokens`；`reasoning ← reasoning_output_tokens`。**五桶齐全**。
- provider `:82-90`（过 `canonical_provider`）、model `:92-99`，journal meta 可覆盖（`:166-180`）。dedup：`"jcode:{session_id}:{message_id}"`（`:219`），id 缺失用 `scope:ordinal`（`:213-218`）。workspace：`working_dir`（`:150-154`、`:284-287`）。

##### prime-agent（Prime Agent）
- 入口：`parse_prime_agent_file:112` → `..._with_accounting:277`；`analyze_prime_agent_accounting:432`；`reconcile_prime_agent_messages:845`。路径 `~/.prime/agent/sessions/*.jsonl` + `session-artifacts` RLM 子会话（`:3-9`）。
- **消息构造完全复用 `pi.rs:524-739`**（namespace `"prime-agent"` `:757`）：`session_id ← header.id`（`pi.rs:596-602`）、时间戳 RFC3339 → ms 且 fallback mtime（`pi.rs:667-672`）、token 走 `PiUsage::to_breakdown`（**reasoning 恒 0**，`pi.rs:254-258`/`:259-267`）、provider 记录值 → 模型推断 → `"prime-agent"`（`pi.rs:655-665`）。
- dedup：优先 `"response:{id}"`（`pi.rs:686-693`），否则 `"message:{id}:{ts|missing}:{provider}:{model}:{in}:{out}:{cr}:{cw}"`（`pi.rs:694-711`），损坏 id 走 `:712-722`。
- **`reconcile_prime_agent_messages` 语义**（`lib.rs:4648-4655` 调用）：容差 1000 ms（`:849`）；可用子响应桶（`:851-861`）；归因按 `(lineage, id)` 去重（`:871-894`）；三个匹配条件（lineage + 尺寸、（双有 1 秒内时间戳 或 双无）、一对一）（`:896-941`）；**最小代价最大匹配**（`:942-986`）；代表消息与加回（`:1039-1073`）、字段级 max（`:1033-1034`）、减算/加法（`:324-329`/`:308-310`）、key 重写（`:1075-1077`）。
- `agent`：`session_info.name`（prime 路径**直接用，不剥 `subagent-` 前缀**）（`pi.rs:613-629`、`:431-434`）。

##### antigravity-cli（Antigravity CLI）
- 入口：`parse_antigravity_cli_file:70`；数据 `~/.gemini/antigravity-cli/conversations/<uuid>.db`，**内容是 protobuf blob**（字段表 `:3-60`）；查询 `SELECT data FROM gen_metadata ORDER BY idx`（`:90-99`）与 `SELECT data FROM trajectory_metadata_blob LIMIT 1`（`:395-402`）。
- `session_id` = 文件 stem（`:75-79`）。时间戳：`chatModel.#9`（`:299-301`）、`generation_timestamp_ms:463`、`proto_timestamp_ms:646`（**秒 + 纳秒 → 毫秒**），fallback `session_timestamp`（= `created-at` 否则 mtime）（`:419-421`），1.1.18 的推断读法受会话窗口约束（`:493-515`）。
- token（protobuf 字段号）：`input = #1 + #2`（`:303-312`）、`cache_read = #5`（`:313`）、`output = #9`（`:314`）、`reasoning = #10`（`:315`）、**`cache_write = 0`**（`:359`）；u64 饱和钳制（`:307-312`）。
- provider/model：`#19` → `SessionModels` 恢复 → `#21` 显示名 → `"unknown"`，**再过 `resolve_alias`**；provider 推断否则 `"antigravity"`（`:329-347`）。dedup：`#11` responseId（可为 `None`），同库去重（`:320-327`）。workspace：`#1.#1` URI（`:411-415` → `:117-119`）。

##### kiro（重点：token 全是估算）
- 入口 `parse_kiro_file:116`（分派 `:117-123`）；六条来源路径：IDE `:392`、globalStorage `:838`、execution `:921`、workspace-session `:1067`、suppress `:1172`、sqlite `:1233`；来源说明 `:3-10`；常量 `:27-29`。
- **token 走三级链（显式值 > context 占比推算 > 字符/4 兜底），`cache_read`/`cache_write`/`reasoning` 恒 0**：`kiro.rs:234-252` —— `input` 优先 `explicit_input > 0` 的显式上报，否则 `context_window × context_usage_percentage / 100`（`:236-247`），再兜底 `estimate_tokens(prompt_chars)`；`output` 优先显式值，否则 `estimate_tokens(assistant_chars)`（`:248-252`）。注释 `:229-233` 原文「Kiro currently reports zero, input is derived from context_usage_percentage * context_window and output from char_count / 4」描述的是**显式计数缺失的常见情形**。各路径 `:229-252`（CLI）、`:569-574`（IDE）、`:878-879`（globalStorage）、`:1027-1028`（execution）、`:1272-1280`（sqlite）；`DEFAULT_CONTEXT_WINDOW = 200_000`（`:435`）；模块注释 `:13` 写明 input 是按 `context_usage_percentage × context_window` 推的。
  ⚠️ **对 UsageMesh 的直接后果：Kiro 的 token 与费用是「推出来/估算的」**（显式上报存在时才是实测），`price_book.quote_on_date` 会照常给出一个非零成本，且 UsageMesh 无法从 `ParsedMessage` 区分它与真实上报值——**通道 A 丢掉了 `UnifiedMessage.cost_source`（`lib.rs:5357-5376`）**。
- `session_id`：CLI `:137-139`、IDE `:408-412`、execution `:930-934`、workspace-session `:1084-1091`、globalStorage `:869-872`、sqlite `:1239`。时间戳：CLI 秒 → ms（`:300-311`、`:189-193`）、turn end 走 `parse_timestamp_value`（`:318-333`）；IDE RFC3339（`:467-471`）或 back-anchor（`:594-601`）；execution `startTime`（`:938-939`）；sqlite `request_start_timestamp_ms` → `stream_end_timestamp_ms` → 0（`:1290-1293`）；workspace-session 用 mtime（`:1138`）。
- provider 常量 `amazon-bedrock`；model 各路径回退 `"auto"`（`:140-148`、`:564`、`:873`、`:1036`、`:1251-1257`）。dedup 模板：`:278`、`:617`、`:667`、`:894-897`、`:1058`、`:1147`、`:1309`。`message_count`：`:280`、`:619`、`:669`、`:915`、`:1060`、`:1149`、`:1311`。
- **`suppress_snapshots_covered_by_executions` 语义**（`:1172-1231`，`lib.rs:5012-5016` 调用）：建 execution 前缀索引（`:1181-1192`）；无 execution 则原样返回（`:1193-1195`）；`:globalstorage:exec:` 精确匹配（`:1204-1207`）；`:workspace-session` 按 session UUID 全局匹配（`:1215-1217`）；`:globalstorage` 按 stem 做 legacy 匹配（`:1218-1228`）；意图见 `:1155-1171`。

##### devin（Devin CLI + Devin Desktop）
- 入口：`parse_devin_cli_sqlite:151`、`parse_devin_desktop_ndjson:396`、`_with_lookup:406`、`load_devin_desktop_session_lookup:108`；路径注释 `:3-5`（`~/.local/share/devin/cli/sessions.db`、`~/Library/Application Support/Devin/User/acp-events/*.ndjson`）。
- `session_id`：CLI `m.session_id`（`:164-175`、`:183`）；Desktop 文件 stem（`:584-589`）→ **lookup 命中后改成 CLI 的 id**（`:361-364`）。lookup 语义 `:80-100`、`:119-124`（对应 `lib.rs:5125-5151` 的按 `session_id` 排除）。
- 时间戳：CLI `created_at * 1000`（`:163`、`:169`）+ back-anchor `total_time_ms`（`:252-273`）+ fallback mtime（`:152`）；Desktop `notification_timestamp`（`:327-335`）+ fallback mtime（`:539`、`:574`）。
- token：CLI `:220-229`（**reasoning 恒 0**）、`num_tokens` **全记 output**（`:231-244`）⚠️；Desktop ACP `_meta`（`:452-482`，**input 减 `cache_read`** `:552-563`）、legacy `:495-529`。
- provider = 推断否则 `"devin"`（`:216-218`、`:372-374`）；model CLI `:202-214`、Desktop `:365-371`、路由模式 `:25-27`。dedup：`"devin-cli:{session}:{row_id}"`（`:274`）、`"devin-desktop:{path}:{suffix}"`（`:375-385`、`:548`、`:577`）。workspace `:288-291`、`:387-390`。

##### micode（MiMo Code）—— 确认「通道 A 不解析」
- 入口 `parse_micode_sqlite:15-17`（转发 `parse_opencode_schema_sqlite(..., OpenCodeSchemaConfig::micode())`）；路径 `~/.local/share/mimocode/mimocode.db`，schema 归 OpenCode（`:3-9`）；发现 `scanner.rs:837`、文件名过滤 `scanner.rs:911`；注册 `clients.rs:728-737`。
- **生产调用点只有 `lib.rs:1711`**，位于 `parse_all_messages_with_pricing_with_cache_policy`（`lib.rs:766` 起）的缓存路径（`load_or_parse_sqlite_source` + `CacheIdentity::for_client(ClientId::MiMoCode)`，`lib.rs:1702-1712`；cost 保护 `lib.rs:1714-1731`）。`parse_local_clients` 区段（`lib.rs:4373-5246`）grep 无 micode 调用 ⇒ **v4.14.0 只在 cached 路径解析，UsageMesh 读不到**（与 §3.3.4 的结论一致，此处给出生产调用点证据）。
- 配置差异 `opencode_schema.rs:279-297`：`strict_cache = false`、`normalize_epoch_seconds = true`、`cost_provenance = WhenReported`、`namespace_rowid_dedup_key = true`、`DedupMode::Merge`（基类默认 `:250-269`）；时间戳 `$.time.created` + `normalize_epoch`（`:500-508`、`:638-643`），**无 fallback（缺则丢行 `:644-647`）**。

##### copilot 三件套
**(a) `copilot`（Copilot CLI，OTel JSONL）**：入口 `parse_copilot_file:15`；注释 `:1-5`、默认目录 `clients.rs:586-595`（`Home` + `.copilot/otel` + `*.jsonl`）。
- `session_id`：`best_session_attr:830-842`（优先级表 `:590-600`）否则 `trace_id` 否则 `"unknown-session"`（`:448-453`）。
- 时间戳链（`:859-883`、`:924-949`）：`startTime` 数组 → `endTime − duration` → `hrTime` → 标量（量级判断）→ `observed` → `timeUnixNano`；fallback mtime（`:16`、`:454-455`）。
- token：`:403-427` + `normalize_input_tokens:798-817`；provider/model `:441-447`、`:589`；**dedup 有 4 套模板**（`:734-777`）；`agent` = 记录级 `gen_ai.agent.id` 否则 trace 级（`:478-501`、`:117`），**core 内不归一**（归一在 CLI 层 `tokscale-cli/src/tui/cache.rs:728`、`tui/data/mod.rs:595`；定义 `sessions/mod.rs:154`——⇒**UsageMesh 拿到的 `agent` 是原始 id，不是展示名**）；workspace 未设置。

**(b) `copilot_desktop`**：入口 `:241`、行转换 `:298`；路径 `~/.copilot/data.db` + `~/.copilot/session-state/{id}`（`:1-4`）、`events.jsonl`（`:440-443`）、发现 `scanner.rs:2350-2352`。`session_id = sessions.id`（`:267`、`:331`）；时间戳 `parse_iso8601_timestamp_ms:592-624`（shutdown 用自身 `:571`，残余用 `created_at` `:309-320`、`:416-421`）；token `:247-257`、`:269-272` + `:563-579` + 差分 `:148-239` + 预算 `:350-372` + 归一 `:373-379`、`:407-413`（**`cache_write` 仅来自 sidecar** `:347-349`）；`message_count` **仅首条为 1、其余为 0**（`:424-431`）。

**(c) `copilot_vscode`**：入口 `:9`/`parse_file:13`；路径注释 `:251-254`、发现 `scanner.rs:1320`、`:2355`；`session_id` = 文件 stem（`:14-17`）；**时间戳 `timestamp` 直用、缺失即 0（无解析函数、无 mtime 回退）**（`:179`）⚠️；token `:157-173`、`:212-229`（**cache 恒 0**）、非 copilot 记录丢弃（`:198-206`）；dedup `"copilot-vscode:{session}:{ts}"`（`:231`）；workspace 取 `workspace.json`（`:251-277`、`:244-246`）。

##### hermes
- 入口 `parse_hermes_sqlite:179`；路径 `~/.hermes/state.db`、`$HERMES_HOME/state.db`（`:3-5`），profile 库 `scanner.rs:665-666`、`:684`；查询 `:35-63`、`:68-92`，探测 `:160-177`。
- `session_id`：`:111`、`:143`。时间戳：`started_at`（**秒，f64**）（`:99`、`:114`）→ `timestamp_secs_to_ms`（`:144` → `utils.rs:518-529`）。token `:42-46`、`:75-79`、`:145-151`；cost `:47`、`:80`、`:152`。provider 走 `resolved_provider:138`。
- dedup：`"hermes:{session}:{model}:{provider}"`（**SQL NULL 转 `"<null>"`**）（`:230-233`、`:19-21`、`:229`）；session-totals 行用裸 `session_id`（`:254`）。
- **`message_count = sessions.message_count`**（`:41`、`:100`、`:115`、`:155`），**每个 session 只有第一行携带**（`:208-223`）⇒ 与 Droid / Copilot Desktop 同族的「计数挂一行」模式。**`agent = Some("Hermes Agent")`**（`:17`、`:139-153`，硬编码）。

##### augment
- 入口 `parse_augment_file:154`；路径 `~/.augment/sessions/<sessionId>.json`（`:3`）、注册 `clients.rs:844-856`。`session_id`：`sessionId` 否则 stem（`:164-174`、`:121-125`）。
- 时间戳：`finishedAt` → `parse_timestamp_str`（`:213-217`），fallback mtime（`:182`）；**end-anchored 说明 `:21-25`**（⇒ 时间戳是「轮次结束」而非开始，与 Claude/Codex 的「开始」口径不同）。
- token：**取最后一个非零 usage（不求和）**（`:112-119`、`:199-203`）；五桶来自共享 `AnthropicUsage::to_breakdown:571`（未逐行读）。
- provider `:100-106`、model `:91-98`/`:205-211`；dedup `"augment:{sid}:req:{request_id}"` / `":seq:{seq}"` / `":turn:{index}"`（`:127-147`）。

##### commandcode
- 入口 `parse_commandcode_file:66`；路径 `~/.commandcode/projects/<slug>/<session>.jsonl`（**无本地 token**，`:3-11`）；config `~/.commandcode/config.json`（`:36-37`、`:252-262`）；注册 `clients.rs:718-727`；跳过 `*.checkpoints.jsonl`（`:67-76`）。
- `session_id`：`:119-123`、`:92`。时间戳：`timestamp` → `parse_rfc3339_ms:246-250`（调用 `:144-148`），fallback mtime（`:78`）。
- **token 全为估算**（`:125-134`、`:156-165`；input 是每轮增量，设计说明 `:17-33`；字符计数 `:197-206`），**cache/reasoning 恒 0**（`:156-162`）。
- model = `canonicalize_model`（**去 org 前缀、剥 `-free`**）`:216-234`；provider = `provider_hint_for_model` 否则 `"command-code"`（`:85-88`、`:242-244`）。dedup `"{session}:{assistant_index}"`（`:164`）；`message_count = 1`（`:166`）；workspace = 目录 slug（`:93-98`、`:168`）。

覆盖度总结表（一级精读 / 二级取证 / 仍未读三行）的唯一权威版见 §3.5.12，本节不再重复。

⇒ **v4.14.0 的通道 A 共分派 47 个 ClientId（外加 1 个 `synthetic` 归一化段），全部有行号级证据**（含 `synthetic` 与 `micode`、`fx` 三条「不产出/只走通道 B」的负结论）。注册表 50 个 ClientId 中 `parse_local()==true` 者 49 个，未进入通道 A 分派的三个是 `cursor`（`parse_local: false`）、`micode`、`fx`。

#### 3.5.11 估算式 token 的「污染面」（新增结论）

这一组客户端**没有可靠的真实 token 上报，tokscale 用「字符数 / 4」或等价推断估算**（`utils.rs:487-489` `estimate_tokens`）。它们在 `ParsedMessage` 里与真实上报值**长得一模一样**：

| 客户端 | 估算范围 | 证据 |
|---|---|---|
| `kiro` | **输入与输出全部走估算/推断**（六条来源路径全部）：显式上报存在时以显式为准，否则 input 由 `context_usage_percentage × context_window` 推算、output 走 `字符数 / 4`；`cache_read`/`cache_write`/`reasoning` 恒 0 | `kiro.rs:234-252`、`:435`、`:569-574`、`:878-879`、`:1027-1028`、`:1272-1280` |
| `commandcode` | **全部输入与输出**，`cache`/`reasoning` 恒 0 | `commandcode.rs:125-134`、`:156-165`、`:197-206` |
| `zcode` | **仅在无 usage 时**回退估算 | `zcode.rs:156-173` |
| `freebuff` | **全部**（且用累计字符 + 输出后清零的重置逻辑） | `freebuff.rs:163-186`、`:206-208` |
| `goose` | 只有 `reasoning` 是推导值（`total − input − output`，源码自标 “INFERRED”） | `goose.rs:117-125` |

**UsageMesh 无从区分**：`ParsedMessage` 的五个 i64 桶没有「来源可信度」字段，而 `UnifiedMessage.cost_source`（只有 `ProviderReported` / `Estimated` / `Unknown` 三态，`mod.rs:66-73`）在 `unified_to_parsed`（`lib.rs:5357-5376`）里被丢掉。⇒ **Kiro / CommandCode / Freebuff 的 token 与由它推出的费用，在 UsageMesh 里无法被标注为「估算」。**

#### 3.5.12 覆盖度总结（诚实标注）

先把三个数字对齐（三者互斥，此前写混了）：注册表 **50 个 `ClientId`**（`clients.rs:403-1007`）；其中 `parse_local()==true` 者 **49 个**（唯一 `false` 是 `cursor`，`clients.rs:447`）；**通道 A 实际分派的 ClientId 是 47 个**（`lib.rs:4367-5246` 内 47 个不同的 `ClientId::*` 引用；未分派的是 `cursor`、`micode`、`fx`）**+ 1 个 `synthetic` 归一化段**（`lib.rs:5237-5246`）。

| 覆盖程度 | 客户端 |
|---|---|
| 一级逐行精读（Claude、Codex） | `claude`、`codex` + 全部公共基座（`sessions/mod.rs`、`utils.rs`、`lib.rs` 主链路） |
| 二级取证代理精读、一级抽查行号 | 其余 45 个通道 A 分派客户端 + `synthetic`：`opencode`、`gemini`、`grok`、`kimi`、`pi`、`kilo`、`goose`、`crush`、`trae`、`zed`、`warp`、`workbuddy`、`codebuddy`、`droid`、`openclaw`、`mux`、`senpi`、`omp`、`kimchi`、`reasonix`、`cherrystudio`、`gjc`、`junie`、`mcode`、`opencodereview`、`cline`、`kilocode`、`codebuff`、`freebuff`、`qwen`、`amp`、`roocode`、`antigravity`、`zcode`、`dsh`、`jcode`、`prime-agent`、`antigravity-cli`、`kiro`、`devin`（`devin-cli` / `devin-desktop`）、`copilot` 三件套（`copilot` / `copilot_desktop` / `copilot_vscode`）、`hermes`、`augment`、`commandcode`；另有 `micode`（§3.5.10）与 `fx`（§3.5.9）两条「只走通道 B」的负结论 |
| 本节未展开（仅注册表 / 主链路列举） | `cursor`（`parse_local: false`，通道 A 不产出）、无其它 |

后一行只有 `cursor` 一项，且它**不是「读到一半」，而是设计上就不参与通道 A**：`cursor.rs` 只在通道 B 的缓存路径被调用（`lib.rs:1925-1926` `ClientId::Cursor` + `sessions::cursor::parse_cursor_file`）。**本表此前把 `hermes`、`copilot` 三件套、`antigravity-cli`、`jcode`、`zcode`、`dsh`、`devin`、`augment`、`prime-agent`、`micode`、`mux` 列为「完全未读」是错的**——这些都在 §3.5.9 / §3.5.10 有逐条 `文件:行号` 级字段证据（`mux` 更在 §3.5.9），当时的「完全未读」栏与同表内的展开互相矛盾。

#### 3.5.13 跨客户端的横向对照（UsageMesh 会用到的那几列）

本表在原表五列之外补两列——「时间戳单位」（原散在 §3.10 未验证项 1）与「token 是否含估算」（原在 §3.5.11）——并加「证据档」列，使每行的证据强度与出处同格可见。

| 客户端 | 证据档 | 数据形态 | `message_count` 语义 | `dedup_key` 有无 | `agent` | workspace | 时间戳单位（秒 / 毫秒，是否归一） | token 是否含估算 |
|---|---|---|---|---|---|---|---|---|
| claude | 【已核实】 | JSONL | 1（tool_result 为 0） | 有（`messageId:requestId` / `message:{id}` / tool_result 路径相关） | 有（sidechain） | 有 | 毫秒（RFC3339 → `timestamp_millis`，`claudecode.rs:875-879`） | 无（`allow_char_estimate: false`，`claudecode.rs:555`） |
| codex | 【已核实】 | JSONL | 1 | 有（累计四元组 / 时间戳兜底） | 有（headless 或昵称） | 有 | 毫秒（`timestamp_millis`，`codex.rs:909-913`；headless 取 `usage.timestamp_ms`、缺则回退 mtime，`:1148`） | 无（token 全部由上报快照增量推导；headless 只丢弃 `reasoning`，`codex.rs:1170-1176`） |
| opencode | 【代核实】 | SQLite + legacy JSON | 1 | 有（内嵌 id / rowid） | 有 | 有 | 毫秒（`$.time.created` **原值直用**，`normalize_epoch_seconds=false`；缺 `$.time` 整行丢弃，`opencode_schema.rs:500-506`、`:261`） | 无（`tokens.*` 直取；cache 不完整整行丢弃而非估算，`opencode_schema.rs:483-493`） |
| gemini | 【代核实】 | JSON / JSONL | 1 | **无** | 无 | **无** | 毫秒（RFC3339 → ms，`gemini.rs:243-247`；headless 数字按 1e12 阈值二判，`utils.rs:203-221`） | 无 |
| grok | 【代核实】 | JSONL ×2 | **unified 路径严格等于 `i32::from(loop_index == 1)`（`grok.rs:758`、`:762`；缺 `ctx.loop_index` 按 1 处理，`:684-692`）**；updates 为 1 | 有（`grok:…`） | 无 | 有 | 毫秒（`extract_timestamp_ms` / 行 `ts`，回退 metadata = mtime，`grok.rs:380`、`:696-699`、`:1443-1456`） | 无 |
| kimi | 【代核实】 | JSONL | 1 | legacy 有（`message_id`）；kimi-code 无 | 无 | **无** | **分歧**：legacy 秒 ×1000（`kimi.rs:302-309`）；kimi-code 毫秒原值（`:236-239`） | 无 |
| pi | 【代核实】 | JSONL | 1 | standard 无；kimchi/prime 有 | 有（`subagent-*`） | 有 | 毫秒（RFC3339 → ms，失败回退 mtime，`pi.rs:667-672`） | 无 |
| kilo | 【代核实】 | SQLite | 1 | 有 | 有 | **无** | 毫秒（`$.time.created` 原值直用；回退 DB 文件 mtime，`opencode_schema.rs:500-506`、`kilo.rs:17`） | 无 |
| goose | 【代核实】 | SQLite | 1 | 有（= `session_id`） | 无 | **无** | `created_at` 文本 → RFC3339 / `%Y-%m-%d %H:%M:%S`（UTC）/ `%Y-%m-%d`；**全失败 = 0、无 mtime 回退**（`goose.rs:31-46`、`:111`） | **部分**：`reasoning = total − (input + output)` 差值推导（源码自标 “INFERRED”，`goose.rs:117-125`） |
| crush | 【代核实】 | SQLite | 按天桶计数 | **无** | 无 | 由 `lib.rs` 注入 | 毫秒（≥1e11 原值、否则 ×1000；`≤0` 丢弃，`crush.rs:221-231`） | 无（token 全零、非估算，`crush.rs:101`、`:127`） |
| trae | 【代核实】 | JSON | 1 | 有（`trae:{sid}:{usage_time}`） | 无 | **无** | **秒 ×1000**（`usage_time` epoch 秒，溢出拒绝、无 mtime 回退，`trae.rs:74-78`） | 无 |
| zed | 【代核实】 | SQLite | 请求数（`.max(1)`）或 1 | 有（`zed:{row_id}`） | 无 | 有 | 毫秒（`created_at` → `updated_at` → payload `$.updated_at` 皆走 `parse_timestamp_str`；三者皆失败跳过，`zed.rs:70-104`、`:277-288`） | 无 |
| warp | 【代核实】 | JSON | `requests_used` | **无** | 无 | 有（**未归一化**） | 毫秒（`syncedAt` RFC3339 → ms；**无效即返回空、无回退**，`warp.rs:41-48`） | 无（token 恒 0，`warp.rs:80`、`:106`） |
| workbuddy | 【代核实】 | JSONL / 扩展日志 / SQLite | 1 | 有 | 无 | 有 | 毫秒（SQLite `updated_at` >1e10 原值、否则 ×1000；JSONL `$.timestamp` 原值 + mtime，`workbuddy.rs:113-119`、`tencent_buddy.rs:257`） | 无 |
| codebuddy | 【代核实】 | JSONL / 扩展日志 | 1 | 有 | 无 | 有 | 毫秒（JSONL `$.timestamp` **原值不换算**；扩展日志行首时间本地 naive 优先，`tencent_buddy.rs:257`、`:426-462`） | 无 |
| droid | 【代核实】 | `.settings.json` + `.jsonl` | 整会话回复数挂第一条，其余 0 | **无** | 无 | **无** | 毫秒（transcript turn `$.timestamp` 过 `parse_timestamp_value`；兜底 lock / mtime 取 max，`droid.rs:313-319`、`:186-193`） | 无（显式 `tokenUsage`；分片按权重摊分，`droid.rs:474-480`、`:485-503`） |
| openclaw | 【代核实】 | JSONL | 1 | **无** | 无 | **无** | 毫秒（`message.timestamp` **原值直用、无秒/毫秒判定**；缺失用 transcript mtime，`openclaw.rs:191`、`:118-123`） | 无 |
| fx（**通道 A 无分派 ⇒ UsageMesh 收不到整条 lane**） | 【代核实】 | JSON | `request_count`（对 UsageMesh 不可达） | 有（`fx:{sid}:{model}`） | 无 | 有 | 毫秒（`updated_at_ms` → `created_at_ms` 原值直用，回退文件 mtime，`fx.rs:292-296`） | 无（五桶均取自上报字段，`fx.rs:305-309`） |
| mux | 【代核实】 | JSON | 1 | 有（`mux:{sid}:{model_key}`） | 无 | **无** | **未验证**：`lastRequest.timestamp` 原样使用、源码无单位说明，仅测试值 `1700000000000` 为毫秒量级（`mux.rs:57-61`、`:170`、`:189`） | 无（五桶取自 `byModel` 上报值，`mux.rs:81-85`） |
| reasonix | 【代核实】 | JSONL（日文件） | **`requests.clamp(1, …)`** | 有 | 无 | **无** | 毫秒（`ts` 走 `utils::parse_timestamp_value`；**解析失败该行丢弃、无 mtime 回退**，`reasonix.rs:97`） | 无 |
| qwen | 【代核实】 | JSONL | 1 | 有（`qwen:{sid}:{idx}`） | 无 | 有 | 毫秒（行 `timestamp` 走 `parse_timestamp_str`，fallback mtime，`qwen.rs:102-105`） | 无 |
| amp | 【代核实】 | JSON | 1 | **无** | 无 | **无** | 毫秒（ledger `timestamp` RFC3339 → ms；messages 记录 = base（`thread.created` 或 mtime）**+ `messageId × 1000`** 人造偏移，`amp.rs:107-123`、`:179-195`） | 无 |
| freebuff | 【代核实】 | JSON（**估算 token**） | **显式 1** | 有 | 无 | **无** | 毫秒（继承 `codebuff` 的 `message_timestamp` → chatId 解析 → mtime，`freebuff.rs:193`、`:153`） | **全部估算**（`estimate_tokens(字符)`，`freebuff.rs:163-186`、`:206-208`） |
| antigravity | 【代核实】 | JSONL | 1 | 有（`responseId`） | 无 | **无** | 毫秒（`timestamp` 数值经 `to_safe_i64` **原值直用、无换算**，`≤0` 丢弃、无 mtime 回退，`antigravity.rs:50`、`:111-121`） | 无 |

**表注**：
- `证据档`：【已核实】= 一级逐行精读（只有 `claude`、`codex` 两行，§3.5.1–§3.5.2）；【代核实】= 二级取证代理逐行读源码并给出行号、一级抽查关键行号后收录（原始报告 `usagemesh-notes/raw/group-A.md`…`group-D.md`，标注规则见 §3.5 开头）。⇒ **本表除前两行外强度一律为【代核实】，勿按【已核实】读。**
- `时间戳单位` 取自 §3.10 未验证项 1 的清单与各客户端小节；标「**未验证**」的格子是本文件未取得源码级证据的那一处（本表内是 `mux` 的单位），**不要按推断补齐**；表外同类项见 §3.10-1 的「仍未逐一确认」段（`zcode` SQLite 的 `started_at`、`kiro` workspace-session 直接用 mtime、`workbuddy`/`codebuddy` 扩展日志行首时间）。
- `token 是否含估算` 取自 §3.5.11：写「无」= 该客户端 token 全部来自上报字段、且未被 §3.5.11 列入估算名单。**§3.5.11 名单里的 `kiro` / `commandcode` / `zcode` 不在本表 24 行之列**——`kiro` 六条来源路径的 input/output 全走「显式 > context 占比推算 > 字符/4」三级链（`kiro.rs:234-252`、`:435`），`commandcode` 全量估算（`commandcode.rs:125-134`），`zcode` 仅在无 usage 时估算（`zcode.rs:156-173`）。

**三条对 UsageMesh 最要命的横向结论**：
1. **无 `dedup_key` 的客户端只能逐 lane 枚举，不能用「约 1/3」概括**。**原先把 `trae` 与 `mux` 列为无键是错的**：`trae.rs:91` `Some(format!("trae:{}:{}", session_id, usage_time))`、`mux.rs:102` `Some(format!("mux:{session_id}:{model_key}"))`，两者都有键（§3.5.8 / §3.5.9 与本节表格也已这么写）。已由构造器路径确证无键的是：`gemini`（`gemini.rs:276`、`:483` 均走 `UnifiedMessage::new`）、`amp`（`amp.rs:95`）、`warp`（`warp.rs:74`、`:100`）、`droid`（`droid.rs:515`、`:553`）、`openclaw`（`openclaw.rs:194`）、`crush`（`crush.rs:95`、`:121`）、`cherrystudio`（`cherrystudio.rs:359`），外加 §3.5.8 的 `roocode` / `kilocode` 任务格式与 §3.5.9 的 `senpi` / `omp`（走 Pi 的 standard 分支）。无键消息在同一棵树里出现两份相同转录时会**双计**，且判定函数只对有键消息生效——`lib.rs:5378-5383` `dedup_key.is_none_or(|key| seen_keys.insert(key))`；通道 A 的实际 `seen` 集合在各 lane 内联处（`lib.rs:4374` OpenCode、`:4440` Claude、`:4464` Codex 等；`lib.rs:1632-1720` 那类是**通道 B** 的同类集合，UsageMesh 不走）。
2. **workspace 缺失的面很宽**（gemini / kimi / kilo / goose / trae / mux / reasonix / amp / freebuff / roocode 系 / droid / openclaw / antigravity / warp 部分），UsageMesh 的 `activity.rs:34` 会把它们全部归到 `"unknown"` 项目。
3. **`message_count` 不是「行数」**：Grok unified 路径、Droid、Crush、Reasonix、Warp、Zed 六家各有自己的语义（**Fx 虽有 `request_count` 语义，但通道 A 不产出它，故不计入 UsageMesh 会遇到的集合**，上表已标注）。UsageMesh 用 `.max(0)` 直接当请求数（`collector.rs:108`、`activity.rs:60`），所以**请求数口径在客户端之间并不同质**。

#### 3.5.14 复刻速查表（路径 / 时间戳 / 桶 / 去重键 / 证据档）

> 收录规则：只收录本文件已给出证据的客户端（§3.3.2 注册表 + §3.5.1–§3.5.10 各小节）；行序 = §3.4.1「Step 3 逐客户端分派」的顺序；每格行号沿用对应小节，**未取得证据的格子写「未验证」**，不引入本文件之外的断言。「路径」三分量 = 注册表 `root` + `relative` + `pattern`（§3.3.2 表，源 `clients.rs:403-1007`）。

| 客户端 | 路径 pattern | 时间戳单位 | 桶映射要点 | 去重键模板 | 证据档 |
|---|---|---|---|---|---|
| `opencode` | `XdgData/opencode/storage/message/*.json`（§3.3.2）；SQLite 侧 `opencode.db`（含 channel 变体，`scanner.rs:96-101`） | 毫秒（`$.time.created` **原值直用**，`normalize_epoch_seconds=false`；缺 `$.time` 丢行，`opencode_schema.rs:500-506`、`:261`） | `tokens.*` 各 `.max(0)`；`cache_read/write` 取 `tokens.cache`，`strict_cache=true` 缺一即丢行（`opencode_schema.rs:663-665`、`:483-493`） | 内嵌 `id` 原样，否则行 `rowid`（`opencode_schema.rs:676-684`）；`DedupMode::MergeUnlessIdConflict`（`:267`） | 【代核实】§3.5.3 |
| `claude` | `EnvVar(CLAUDE_CONFIG_DIR)/.claude` + `projects/*.jsonl`（`clients.rs:414-426`） | 毫秒（RFC3339 → `timestamp_millis`，`claudecode.rs:875-879`） | `usage.*` 四桶、`reasoning` 恒 0；tool_result 行只填 `input`（`claudecode.rs:703-709`、`:1101-1107`）；字符估算永久禁用（`:555`） | `{message.id}:{request_id}` / `message:{id}` / `{client}:tool_result:{sid}:{usage_key}`（`claudecode.rs:629-631`、`:649-650`、`:1005-1007`） | 【已核实】§3.5.1 |
| `codex` | `EnvVar(CODEX_HOME)/.codex` + `sessions/*.jsonl`（`clients.rs:427-439`，`headless: true`） | 毫秒（`timestamp_millis`，`codex.rs:909-913`） | `input − min(cached, input)`、`output − min(reasoning, output)`、`cache_write = 0`（`codex.rs:191-216`）；headless 丢弃 `reasoning`（`:1170-1176`） | `codex:token_count-total:{scope}:{provider}:{model}:{四桶}`，否则 `codex:token_count:{ts}:…`（`codex.rs:931-940`、`:943-953`） | 【已核实】§3.5.2 |
| `copilot`（三解析器合一） | `Home/.copilot/otel/*.jsonl`（`clients.rs:586-595`）；另 `~/.copilot/data.db` + `session-state/{id}`（`copilot_desktop.rs:1-4`）与 VS Code chatSessions（`copilot_vscode.rs:251-254`） | 毫秒（起点/终点/hrTime/标量/observed 链 + mtime 兜底，`copilot.rs:859-883`、`:924-949`；Desktop `parse_iso8601_timestamp_ms:592-624`；**VS Code `timestamp` 直用、缺失即 0**，`copilot_vscode.rs:179`） | `copilot.rs:403-427` + `normalize_input_tokens:798-817`；Desktop 差分 `:148-239`；VS Code **cache 恒 0**（`copilot_vscode.rs:212-229`） | `copilot.rs:734-777` 四套模板 / `copilot-vscode:{sid}:{ts}`（`copilot_vscode.rs:231`） | 【代核实】§3.5.10 |
| `gemini` | `EnvVar(GEMINI_CLI_HOME)/.gemini` + `tmp/*.json\|*.jsonl`（§3.3.2） | 毫秒（RFC3339 → ms，`gemini.rs:243-247`；headless 数字按 1e12 阈值二判，`utils.rs:203-221`） | `input ← input + tool`、`cache_write` 恒 0、`reasoning ← thoughts`（`gemini.rs:283-287`）；headless 有 overlap 扣除（`:502-513`） | **无**（`gemini.rs:276`、`:483` 均走 `UnifiedMessage::new`） | 【代核实】§3.5.4 |
| `amp` | `XdgData/amp/threads/T-*.json`（§3.3.2） | 毫秒（ledger `timestamp` RFC3339 → ms；messages 记录 = base（`thread.created` 或 mtime）**+ `messageId × 1000`**，`amp.rs:107-123`、`:179-195`） | ledger / message 两路显式字段，`reasoning` 恒 0（`amp.rs:157-163`、`:203-209`） | **无**（`amp.rs:95`） | 【代核实】§3.5.9 |
| `codebuff` | `EnvVar(CODEBUFF_DATA_DIR)/.config/manicode` + `projects/*/chats/*/chat-messages.json`（§3.3.2） | 毫秒（`message_timestamp` → chatId 解析 → mtime，`codebuff.rs:191-203`、`:136-145`） | `metadata.usage` → `metadata.codebuff.usage` → RunState messageHistory；`reasoning` 恒 0（`codebuff.rs:249-276`、`:330-410`、`:88`） | 上游 `msg.id` 优先，否则 `codebuff:{sid}:{ts}:{model}:{ordinal}:{四桶}`（`codebuff.rs:100-105`、`:111-125`） | 【代核实】§3.5.9 |
| `freebuff` | `EnvVar(FREEBUFF_DATA_DIR)/.config/manicode` + `projects/*/chats/*/chat-messages.json`（`clients.rs:909-912`；**与 codebuff 同桶、按请求门控** `lib.rs:4351-4356`） | 毫秒（同 codebuff：`freebuff.rs:193`、`:153`） | **全量字符估算**：`estimate_tokens(字符)`；`cache_read/cache_write/reasoning` 恒 0（`freebuff.rs:184-185`、`:206-208`） | `{session_id}:{assistant_index}`（`freebuff.rs:211`） | 【代核实】§3.5.9 |
| `droid` | `Home/.factory/sessions/*.settings.json`（`clients.rs:473-482`；同名 `.jsonl` 是 transcript，`droid.rs:145-149`） | 毫秒（turn `$.timestamp` 过 `parse_timestamp_value`；兜底 `providerLockTimestamp`/mtime 取 max，`droid.rs:313-319`、`:186-193`） | `tokenUsage.*` 五桶、各 `.max(0)`；**分片按权重摊分**（`droid.rs:474-480`、`:485-503`） | **无**（`droid.rs:515`、`:553`） | 【代核实】§3.5.8 |
| `openclaw` | `Home/.openclaw/agents/*.jsonl*`（`clients.rs:483-492`；模式也接归档名，`scanner.rs:424-430`） | 毫秒（`message.timestamp` **原值直用、无秒/毫秒判定**；缺失用 transcript mtime，`openclaw.rs:191`、`:118-123`） | `usage.to_breakdown()` 四桶、`reasoning` 恒 0（`openclaw.rs:200`、`utils.rs:611-622`） | **无**（`openclaw.rs:194`） | 【代核实】§3.5.8 |
| `pi` | `Home/.pi/agent/sessions/*.jsonl`（§3.3.2） | 毫秒（RFC3339 → ms，失败回退 mtime，`pi.rs:667-672`） | `PiUsage::to_breakdown` 四桶、各 `.max(0)`、`reasoning` 恒 0（`pi.rs:259-267`、`:250-258`） | standard = `None`；`{ns}:{sid}:{message_id}` / `{ns}:response:{id}` / `{ns}:message:{…}`（`pi.rs:341-348`、`:726`、`:693`、`:703-709`） | 【代核实】§3.5.7 |
| `prime-agent` | `EnvVar(PRIME_AGENT_CODING_AGENT_DIR)/.prime/agent` + `sessions/*.jsonl`（§3.3.2）+ `session-artifacts` RLM 子会话（`prime_agent.rs:3-9`） | 毫秒（同 `pi.rs:667-672`） | 复用 `PiUsage::to_breakdown`（`pi.rs:259-267`）；lane 末跨文件 `reconcile_prime_agent_messages`（调用 `lib.rs:4648-4655`，实现 `:845-1077`） | `response:{id}` 优先，否则 `message:{id}:{ts\|missing}:{provider}:{model}:{四桶}`（`pi.rs:686-693`、`:694-711`） | 【代核实】§3.5.10 |
| `kimchi` | `EnvVar(KIMCHI_CODING_AGENT_DIR)/.config/kimchi/harness` + `sessions/*.jsonl`（`clients.rs:859-871`） | 毫秒（继承 `pi.rs:667-672`） | 继承 `pi.rs:259-267` | `kimchi:{sid}:{message_id}`（`pi.rs:726`） | 【代核实】§3.5.9 |
| `reasonix` | `ReasonixHome/stats/YYYY-MM-DD.jsonl`（§3.3.2；每行一个 provider 请求，`reasonix.rs:3-4`） | 毫秒（`ts` 走 `utils::parse_timestamp_value`；**解析失败丢行、无 mtime 回退**，`reasonix.rs:97`） | `prompt → input`；`cache_hit → cache_read`；`cache_miss` 存在且非 0 时进 input，否则 `input = prompt − cache_hit`；`completion − reasoning → output`；`cache_write` 恒 0（`reasonix.rs:99-115`） | `reasonix:{path}:{line}:{requests}:{total}`（`reasonix.rs:124-130`） | 【代核实】§3.5.9 |
| `senpi` | `EnvVar(SENPI_CODING_AGENT_DIR)/.senpi/agent` + `sessions/*.jsonl`（§3.3.2） | 毫秒（继承 `pi.rs:667-672`） | 继承 `pi.rs:259-267`；**明确不计 `usage.reasoning`**（`senpi.rs:5-9`） | **无**（standard 分支，`senpi.rs:20-21`、`pi.rs:341-348`） | 【代核实】§3.5.9 |
| `omp` | `Home/.omp/agent/sessions/*.jsonl`（§3.3.2；**刻意不读 `PI_CODING_AGENT_DIR`**，`omp.rs:7-20`） | 毫秒（继承 `pi.rs:667-672`） | 继承 `pi.rs:259-267` | **无**（`omp.rs:27-28`） | 【代核实】§3.5.9 |
| `augment` | `Home/.augment/sessions/*.json`（`clients.rs:844-856`） | 毫秒（`finishedAt` 走 `parse_timestamp_str`；**end-anchored**，`augment.rs:213-217`、`:21-25`） | **取最后一个非零 usage、不求和**（`augment.rs:112-119`、`:199-203`）；五桶来自共享 `AnthropicUsage::to_breakdown`（`:571`，该函数未逐行读） | `augment:{sid}:req:{request_id}` / `:seq:{seq}` / `:turn:{index}`（`augment.rs:127-147`） | 【代核实】§3.5.10 |
| `commandcode` | `Home/.commandcode/projects/*/*.jsonl`（`clients.rs:718-727`；**无本地 token**） | 毫秒（`parse_rfc3339_ms`，回退 mtime，`commandcode.rs:246-250`、`:78`） | **全量字符估算**（input 为每轮增量），`cache/reasoning` 恒 0（`commandcode.rs:125-134`、`:156-165`、`:197-206`） | `{session}:{assistant_index}`（`commandcode.rs:164`） | 【代核实】§3.5.10 |
| `gjc` | `EnvVar(GJC_CODING_AGENT_DIR)/.gjc/agent` + `sessions/*.jsonl`（§3.3.2） | 毫秒（message `timestamp` 毫秒优先；记录级 ISO-8601 次之；再 mtime，`gjc.rs:161-166`） | `CamelUsage::to_breakdown`，`reasoning` 恒 0（`utils.rs:599-622`）；**`client` 可被 message `source` 覆盖**（`gjc.rs:194-198`，与 `lib.rs:198` 的 9router 特判呼应） | `{session}:{msg_id}`，否则 `gjc:{sid}:{ts}:{model}:{provider}:{五桶}:{line_hash}`（`gjc.rs:189-190`、`:82-89`） | 【代核实】§3.5.9 |
| `junie` | `Home/.junie/sessions/*/events.jsonl`（§3.3.2） | 毫秒（`timestampMs`；回退 session id 的 `session-YYMMDD-HHMMSS`（Local）→ mtime；有显式时间戳时 `back_anchor`，`junie.rs:67-68`、`:172-193`、`:126-130`） | 五桶别名（`junie.rs:224-245`）；model 过 `resolve_alias`（`:88-90`）；`is_turn_start` 由 `UserPromptEvent` 置位（`:49-52`） | `junie:{sid}:{ts}:{model}:{五桶}:{cost}:{usage_index}`（`junie.rs:100-108`） | 【代核实】§3.5.9 |
| `zcode` | `Home/.zcode/projects/*/*.jsonl`（`clients.rs:767-776`）+ SQLite `~/.zcode/cli/db/db.sqlite`（`scanner.rs:111-112`） | JSONL 毫秒（`parse_rfc3339_ms` + mtime，`zcode.rs:180-184`）；SQLite `started_at` → `completed_at − duration_ms` → `completed_at` → mtime（`:528-544`），**`started_at` 单位未验证**（§3.10-1） | 别名块 + 包含式归一（`zcode.rs:47-73`、`:246-277`）；SQLite legacy 无条件相减（`:428-434`）；**缺 usage 时字符估算**（`:156-173`） | `{session}:{idx}` / `zcode-sqlite:{id}`（`zcode.rs:194`、`:472`） | 【代核实】§3.5.10 |
| `cherrystudio` | `AppData/CherryStudio/.claude/projects/*/*.jsonl`（§3.3.2；V1 根省略 `Data/Agents`，`cherrystudio.rs:3-9`） | 毫秒（顶层 `timestamp` 走 `parse_timestamp_str` + mtime，`cherrystudio.rs:344-347`） | Claude 式四桶，流式合并逐桶取 max（`cherrystudio.rs:350-356`、`:198-205`） | **无**（跨行别名并查集 `dedupe_usage_records:112-174`；构造器 `:359`） | 【代核实】§3.5.9 |
| `dsh` | `EnvVar(DSH_HOME)/.dsh` + `sessions/<encoded-cwd>/<sid>/session.jsonl(.zstd)`（§3.3.2、`dsh.rs:3-8`） | 毫秒（事件 `time` **原值直用**；缺失或 `≤0` 跳过、无回退，`dsh.rs:288-293`） | `output ← outputTokens − reasoningTokens`；`cache_write ← cacheWriteTokens`；`reasoning ← reasoningTokens`（`dsh.rs:423-433`） | `dsh:{kind}{identity}:{ts}:{provider}:{model}:{五桶}`（`dsh.rs:370-377`） | 【代核实】§3.5.10 |
| `mcode` | `Config/headless/mcode/*.jsonl`（§3.3.2；`stream-json` 输出，`mcode.rs:3-8`） | 毫秒（`normalize_timestamp`：`0 < t < 1e10` 视为秒 ×1000，`mcode.rs:170-176`） | 四桶、`reasoning` 恒 0（`mcode.rs:156-164`）；`agent = "headless"`（`:144`） | `mcode:{sid}:{turn_id}:{index}:{四桶}`（`mcode.rs:127-133`） | 【代核实】§3.5.9 |
| `opencodereview` | `Home/.opencodereview/sessions/<repo>/<sid>.jsonl`（§3.3.2、`opencodereview.rs:3-4`） | 毫秒（`parse_timestamp_value` + mtime；有显式时间戳且 `duration_ms > 0` 时 `back_anchor`，`opencodereview.rs:58-59`、`:91-94`） | 四桶、`reasoning` 恒 0（`opencodereview.rs:153-161`） | `opencodereview:{sid}:{ts}:{model}:{四桶}`，无显式时间戳时追 `:line{index}`（`opencodereview.rs:108-115`） | 【代核实】§3.5.9 |
| `kimi` | `Home/.kimi/sessions/<GROUP>/<SESSION>/wire.jsonl`（`clients.rs:503-512`；kimi-code 靠 `is_kimi_code_path` 在解析期区分，`kimi.rs:124-129`） | legacy **秒 ×1000**（`kimi.rs:302-309`）；kimi-code 毫秒原值（`:236-239`，注释明确不走启发式） | `TokenUsage::to_breakdown` 四桶、`reasoning` 恒 0（`kimi.rs:59-77`） | legacy = `payload.message_id`；kimi-code 无（`kimi.rs:316`、`:326`） | 【代核实】§3.5.6 |
| `qwen` | `Home/.qwen/projects/*/chats/*.jsonl`（§3.3.2） | 毫秒（`parse_timestamp_str` + mtime，`qwen.rs:102-105`） | `usageMetadata.*`：`promptTokenCount` / `candidatesTokenCount` / **`thoughtsTokenCount`** / `cachedContentTokenCount`，`cache_write` 恒 0（`qwen.rs:108-112`） | `qwen:{line_session_id}:{message_index}`（`qwen.rs:126`） | 【代核实】§3.5.9 |
| `roocode` | `Home/.config/Code/User/globalStorage/rooveterinaryinc.roo-cline/tasks/*/ui_messages.json`（§3.3.2） | `parse_timestamp_str`；**失败跳过条目、无 mtime 兜底**（`roocode.rs:165-176`、`:54-57`） | 内嵌 JSON `tokensIn/tokensOut/cacheReads/cacheWrites`（`roocode.rs:192-195`、`:72-78`） | **无**（`roocode.rs:27` 委托路径） | 【代核实】§3.5.9 |
| `kilocode` | `Home` + `.config/Code/User/globalStorage/kilocode.kilo-code/tasks` + `ui_messages.json`（`clients.rs:533-542`；§3.3.2） | 同 `roocode.rs:165-176`（无兜底） | 同 `roocode.rs:192-195` | **无** | 【代核实】§3.5.9 |
| `cline` | `Home/.config/Code/User/globalStorage/saoudrizwan.claude-dev/tasks/*/ui_messages.json`（§3.3.2）+ CLI `~/.cline/data/sessions/<session>/<session>.messages.json`（`cline.rs:134-135`） | CLI `ts` → `parse_timestamp_value` + mtime；任务格式 `parse_timestamp_str`、无兜底（`cline.rs:227-231`、`roocode.rs:165-176`） | CLI `input = input_tokens − cache_read − cache_write`、`reasoning` 恒 0；任务格式四桶（`cline.rs:201-215`、`:248`） | CLI `cline-cli:{sid}:{id}`，否则 `cline-cli:{sid}:{assistant_index}`；任务格式 `None`（`cline.rs:232-236`） | 【代核实】§3.5.9 |
| `mux` | `Home/.mux/sessions/<workspaceId>/session-usage.json`（§3.3.2、`mux.rs:1-3`） | **未验证**（`lastRequest.timestamp` 原值直用、源码无单位说明；测试值 `1700000000000`，`mux.rs:57-61`、`:170`） | 五桶齐全：`input/cached/cacheCreate/output/reasoning`（`mux.rs:81-85`、`:121-127`）；负值 `.max(0)`（`:78`） | `mux:{sid}:{model_key}`（`mux.rs:102`） | 【代核实】§3.5.9 |
| `kilo` | `XdgData/kilo/kilo.db`（§3.3.2；payload 在 `data` 列，`kilo.rs:4-9`） | 毫秒（`$.time.created` 原值直用；回退 DB 文件 mtime，`opencode_schema.rs:500-506`、`kilo.rs:17`） | 同 OpenCode schema：`tokens.*` 各 `.max(0)`、`strict_cache` 默认 true（`opencode_schema.rs:663-665`、`:483-493`） | `$.id` 优先，否则行 `id` 列；模块内 `DedupMode::Off`（`opencode_schema.rs:675-684`、`:313` 的 `kilo` 配置） | 【代核实】§3.5.8 |
| `hermes` | `EnvVar(HERMES_HOME)/.hermes` + `state.db`（§3.3.2；profile 库 `scanner.rs:665-666`、`:684`） | **秒**（f64 `started_at`）→ `timestamp_secs_to_ms`（`hermes.rs:99`、`:144`、`utils.rs:518-529`） | `hermes.rs:42-46`、`:75-79`、`:145-151`；`message_count = sessions.message_count`（**仅首行携带**，`:208-223`）；`agent` 硬编码 `Some("Hermes Agent")`（`:17`、`:139-153`） | `hermes:{session}:{model}:{provider}`（SQL NULL → `"<null>"`）；session-totals 行用裸 `session_id`（`hermes.rs:230-233`、`:254`） | 【代核实】§3.5.10 |
| `goose` | `XdgData/goose/sessions/sessions.db`（§3.3.2；四候选路径见 `goose.rs:4-7`） | `created_at` 文本 → RFC3339 / `%Y-%m-%d %H:%M:%S`（UTC）/ `%Y-%m-%d`；**全失败 = 0、无 mtime 回退**（`goose.rs:31-46`、`:111`） | `input/output` 取 accumulated 优先；`cache_read/cache_write = 0`；**`reasoning = total − (input + output)` 推导**（源码自标 “INFERRED”，`goose.rs:88-99`、`:115-125`） | `Some(session_id)` 本身（无模板，`goose.rs:129`） | 【代核实】§3.5.8 |
| `zed` | `XdgData/zed/threads/threads.db`（§3.3.2；三候选路径见 `zed.rs:4-6`） | `created_at` → `updated_at` → payload `$.updated_at`，皆走 `parse_timestamp_str`；三者皆失败整条跳过（`zed.rs:70-104`、`:277-288`、`utils.rs:223-257`） | `request_token_usage` 优先，为零则 `cumulative_token_usage`；`reasoning` 恒 0（`zed.rs:209-219`、`:253-261`）；**×1.1 加价只在通道 B 生效**（`lib.rs:4190-4210`） | `zed:{row.id}`（`zed.rs:163`） | 【代核实】§3.5.8 |
| `kiro` | `Home/.kiro/sessions/cli/*.json`（§3.3.2）；另有 IDE / globalStorage / execution / workspace-session / sqlite 五条来源（`kiro.rs:392`、`:838`、`:921`、`:1067`、`:1233`） | CLI 秒 ×1000（`kiro.rs:300-311`）；IDE RFC3339 或 back-anchor（`:467-471`、`:594-601`）；execution `startTime`（`:938-939`）；sqlite 两列 → 0（`:1290-1293`）；workspace-session 用 mtime（`:1138`） | **三级链：显式 > `context_window × context_usage_percentage` > `字符/4`**；`cache_read/cache_write/reasoning` 恒 0；`DEFAULT_CONTEXT_WINDOW = 200_000`（`kiro.rs:234-252`、`:435`） | 六套模板（`kiro.rs:278`、`:617`、`:667`、`:894-897`、`:1058`、`:1147`、`:1309`） | 【代核实】§3.5.10 |
| `crush` | `XdgData/crush/projects.json`（`clients.rs:563-572`；per-project `crush.db` 发现 `scanner.rs:945-`、`:976-`） | 毫秒（`normalize_crush_timestamp_ms`：≥1e11 原值、否则 ×1000；`≤0` 丢弃）（`crush.rs:221-231`）；**通道 A 唯一接收 `BucketTimezone` 的 lane**（`lib.rs:5032-5033`） | **全零（COST-ONLY）**（`crush.rs:101`、`:127`、`:7-12`） | **无**（`crush.rs:95`、`:121`） | 【代核实】§3.5.8 |
| `antigravity` | `Config/antigravity-cache/sessions/*.jsonl`（`clients.rs:619-628`）；**解析器内无路径说明 ⇒ 未验证**（`antigravity.rs:1` 即 `use` 行） | 毫秒（`timestamp` 经 `to_safe_i64` **原值直用、无换算**；`≤0` 丢弃、无回退，`antigravity.rs:50`、`:111-121`） | 五桶 `input/output/cacheRead/cacheWrite/reasoning`，全 0 丢弃（`antigravity.rs:74-81`）；model 过 `resolve_alias`（`:62-64`） | `responseId` 原样（trim 非空），缺失 `None`（`antigravity.rs:83-87`） | 【代核实】§3.5.9 |
| `antigravity-cli` | `EnvVar(GEMINI_CLI_HOME)/.gemini` + `antigravity-cli/conversations/*.db`（§3.3.2；protobuf blob，字段表 `antigravity_cli.rs:3-60`） | **秒 + 纳秒 → 毫秒**（`antigravity_cli.rs:646`）；回退 `session_timestamp`（= `created-at` 否则 mtime，`:419-421`） | protobuf 字段：`#1 + #2 → input`、`#5 → cache_read`、`#9 → output`、`#10 → reasoning`、**`cache_write = 0`**（`antigravity_cli.rs:303-315`、`:359`）；u64 饱和钳制（`:307-312`） | `#11` responseId（可为 `None`）（`:320-327`） | 【代核实】§3.5.10 |
| `trae` | `Config/trae-cache/sessions/*.json`（`clients.rs:649-658`；usage API 原始 dump） | **epoch 秒 ×1000**（`checked_mul`，溢出拒绝、无 mtime 回退；缺失/`≤0` 丢弃）（`trae.rs:74-78`） | `extra_info.input_token/output_token/cache_read_token/cache_write_token`；`reasoning` 恒 0；四桶全 0 丢弃（`trae.rs:82-89`、`:104`） | `trae:{sid}:{usage_time}`（`trae.rs:91`）；lane 末再跑 `dedupe_latest_trae_messages`（每 session 只留最新一条，`lib.rs:2922-2954`） | 【代核实】§3.5.8 |
| `warp` | `Config/warp-cache/usage*.json`（`clients.rs:659-668`；解析器内无路径字面量，`warp.rs` 全文 153 行） | 毫秒（`syncedAt` RFC3339 → ms；**无效或 `≤0` 直接返回空向量、无回退**，`warp.rs:41-48`） | **全零**（`warp.rs:80`、`:106`）；provider/model 为字面量 `"warp"` / `"aggregate-requests"` | **无**（`warp.rs:74`、`:100`） | 【代核实】§3.5.8 |
| `devin-cli` / `devin-desktop` | `XdgData/devin/cli/sessions.db` 与 `Home/Library/Application Support/Devin/User/acp-events/*.ndjson`（§3.3.2、`devin.rs:3-5`） | CLI `created_at × 1000` + back-anchor + mtime；Desktop `notification_timestamp` + mtime（`devin.rs:163-169`、`:252-273`、`:327-335`、`:539`） | CLI 五桶（`reasoning` 恒 0）、**`num_tokens` 全记 output**；Desktop ACP `_meta`（input 减 `cache_read`）、legacy 分支（`devin.rs:220-244`、`:452-482`、`:552-563`） | `devin-cli:{session}:{row_id}` / `devin-desktop:{path}:{suffix}`（`devin.rs:274`、`:375-385`） | 【代核实】§3.5.10 |
| `codebuddy` | `Home/.codebuddy/projects/<project-key>/*.jsonl`（§3.3.2；另 IDE/VS Code 扩展日志） | JSONL `$.timestamp` **毫秒原值不换算**、缺则 mtime；扩展日志行首时间本地 naive 优先（`tencent_buddy.rs:257`、`:195`、`:426-462`） | `BuddyUsage::to_breakdown`；**`input` 是「独占」语义**（报告 total 证明含 cache 时才相减）（`tencent_buddy.rs:92-125`、`:149-162`） | `{client}:{sid}:{messageId\|traceId\|line.id}`（`tencent_buddy.rs:274-279`） | 【代核实】§3.5.8 |
| `workbuddy` | `Home/.workbuddy` + `workbuddy.db`（§3.3.2；另 `projects/**/*.jsonl`，`workbuddy.rs:3-5`） | SQLite `updated_at`：>1e10 原值、否则 ×1000（SQL 已过滤 `>0`）；JSONL `$.timestamp` 原值 + mtime（`workbuddy.rs:113-119`、`:55-56`、`tencent_buddy.rs:257`） | SQLite **只填 `input = used`**、其余全 0；JSONL 走 `BuddyUsage::to_breakdown`（`workbuddy.rs:92-98`、`tencent_buddy.rs:92-125`） | `workbuddy:{sid}:{updated_at}` / `{client}:{sid}:{…}` / 扩展日志模板（`workbuddy.rs:103`、`tencent_buddy.rs:274-279`、`:368-378`） | 【代核实】§3.5.8 |
| `grok` | `EnvVar(GROK_HOME)/.grok/sessions` + `updates.jsonl`（`clients.rs:692-704`；新版本另写 `logs/unified.jsonl`，`grok.rs:3-11`） | 毫秒（`extract_timestamp_ms` / 行 `ts`，缺省回退 metadata（= 文件 mtime），`grok.rs:380`、`:696-699`、`:1443-1456`） | updates：别名 + 重叠拆分（`input −= cache_read`、`output −= reasoning`）；unified：`input = prompt − cached`、`output = completion − reasoning`、`cache_write = 0`（`grok.rs:180-252`、`:750-756`） | `grok:{sid}:{turn_index}` / `grok:{sid}:usage:{i}:{event_id}` / `grok:{sid}:signals` / `grok-unified:{sid}:{identity}`（`grok.rs:333`、`:427-430`、`:1293`、`:1087-1118`） | 【代核实】§3.5.5 |
| `jcode` | `EnvVar(JCODE_HOME)/.jcode` + `sessions/session_*.json`（§3.3.2；另 `*.journal.jsonl`，`jcode.rs:3`、`:185-188`） | 毫秒（message `timestamp` 走 `parse_timestamp_str`；**仅显式时间戳存在时** `back_anchor`；回退快照 mtime，`jcode.rs:246`、`:261-265`、`:317`） | `tokens_from_usage` 五桶齐全；`input` 为 OpenAI 式（减 `cache_read` 子集）（`jcode.rs:110-130`） | `jcode:{sid}:{message_id}`，id 缺失用 `scope:ordinal`（`jcode.rs:219`、`:213-218`） | 【代核实】§3.5.10 |
| `fx`（**通道 A 不产出**） | `Home/.fx/sessions/<sessionId>/usage-v2.json`（`fx.rs:3-6`；注册 `clients.rs:979-989`）——`ClientId::Fx` 在 `lib.rs:4367-5246` 零命中（§3.3.4、§3.5.9） | 毫秒（`updated_at_ms` → `created_at_ms` 原值直用；回退文件 mtime，`fx.rs:292-296`） | 五桶齐全（`fx.rs:305-309`；无 per-model 条目时用顶层聚合 `:358-362`） | `fx:{sid}:{model_id}` / 兜底 `fx:{sid}:fx-unknown`（`fx.rs:324`、`:382`） | 【代核实】§3.5.9 |
| `synthetic`（**不是 `ClientId`**） | Octofriend SQLite 槽（`scanner.rs:104`）+ 网关字段归一化（`lib.rs:5237-5246`、`:5256-5263`） | 毫秒（`timestamp > 1e12` 视为毫秒、否则 ×1000，`synthetic.rs:164-167` 与 `:206-209`） | 五桶各 `.max(0)`（`synthetic.rs:177-181`、`:219-`） | `Some(id)`（`synthetic.rs:185`、`:227`；过滤函数 `matches_synthetic_filter` `synthetic.rs:99-101`） | 【代核实】§3.3.4 / §3.9.3 |

表注：`cursor`（`parse_local: false`）与 `micode`（`parse_local: true` 但通道 A 无分派）不在本表——两者都不产出任何 `ParsedMessage`，见 §3.3.4 与 §3.9.3。

---

### 3.6 token 桶与去重语义

#### 3.6.1 `TokenBreakdown`（`lib.rs:249-286`）

【已核实】

```rust
249: #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
250: pub struct TokenBreakdown {
251:     pub input: i64,
252:     pub output: i64,
253:     pub cache_read: i64,
254:     pub cache_write: i64,
255:     pub reasoning: i64,
256: }
```

**v4.14.0 只有五个桶。没有 `cache_write_1h`、没有 `cache_write_5m`。** `cache_write_1h`（snake_case）在 `crates/` 全仓**零命中**；最接近的写法是 camelCase `cacheWrite1h`，只出现在 `sessions/senpi.rs` 的注释与**测试 fixture 字符串**里（一个 Pi 会话 JSON 示例，不是结构字段）：

```
crates/tokscale-core/src/sessions/senpi.rs:41:  // nested cost object and cacheWrite1h/reasoning fields.
crates/tokscale-core/src/sessions/senpi.rs:44:  {"type":"message",...,"usage":{...,"cacheWrite1h":0,"reasoning":16,...}}
```

⇒ **「`cache_write_1h` 是 `cache_write` 的子集」这类约定在 v4.14.0 不存在**；它是 v4.18.0 才引入的（见 §3.8）。

#### 3.6.2 `total()` 是否含 `reasoning`

【已核实】**含。** `lib.rs:271-279`：

```rust
271:     pub fn total(&self) -> i64 {
272:         // saturating so clamped (i64::MAX) buckets from a corrupt source can't
273:         // overflow the sum.
274:         self.input
275:             .saturating_add(self.output)
276:             .saturating_add(self.cache_read)
277:             .saturating_add(self.cache_write)
278:             .saturating_add(self.reasoning)
279:     }
```

五个桶全部相加，且用 `saturating_add` 防溢出。

#### 3.6.3 饱和累加

【已核实】`add_assign_saturating`（`lib.rs:263-269`）逐桶 `saturating_add`；`+=` 运算符实现直接委托它（`lib.rs:282-286`）。文档注释解释了动机（`:258-262`）：“Use this for whole-breakdown aggregation so adding a new bucket cannot silently leave one hand-written accumulation site incomplete.”

注意：`TokenBreakdown` 是**i64 且允许负值**——测试 `token_breakdown_add_assign_saturates_each_field`（`lib.rs:5497-5519`）明确断言 `i64::MIN` 桶能保持并在加 `-1` 时不再下溢。所以「饱和」只防溢出，**不防负值**。

#### 3.6.4 各桶的客户端级语义（模板）

解析器写入桶的方式差异很大，必须逐客户端区分：

- **Claude（`claudecode.rs:703-709`）**：`input = usage.input_tokens`、`output = usage.output_tokens`、`cache_read = usage.cache_read_input_tokens`、`cache_write = usage.cache_creation_input_tokens`、`reasoning = 0`（**Claude 不填 reasoning**）。全部 `.max(0)`。
- **Codex（`codex.rs:191-216`）**：`input = raw_input - min(cached, raw_input)`、`output = raw_output - min(reasoning, raw_output)`、`cache_read = min(cached, raw_input)`、`cache_write = 0`、`reasoning = min(reasoning, raw_output)`。
  - `cached` 的取值：`cached_input_tokens` 与 `cache_read_input_tokens` 取较大（`codex.rs:125-129`）。
  - 注释 `codex.rs:195-208` 解释了为什么要从 output 里**减掉** reasoning：`reasoning_output_tokens` 是 `output_tokens` 的子集，而 `TokenBreakdown::total()` 把两者相加、`compute_cost` 也按 output 单价为两者之和计价，所以不减会双计双价。同时注释明确这是 **Codex 专属修正**，Gemini 那种「reasoning 与 output 不相交」的供应商必须原样喂。
  - **Codex 的 `cache_write` 永远是 0**（`:213`、`:1174`）。UsageMesh 对此有专门处理：`collector.rs:302-305` 把「没有 cache-write 证据且 input>0」的 Codex 行标为 `cost_lower_bound`。
- **Codex headless 路径（`codex.rs:1170-1176`）**：`input`/`output`/`cache_read` 直接来自 headless usage，`cache_write = 0`、`reasoning = 0`（**reasoning 被丢弃**）。

#### 3.6.5 去重键：`dedup_key`

【已核实】`dedup_key` 是 `UnifiedMessage` 字段（`sessions/mod.rs:94`），由各解析器用 `UnifiedMessage::new_with_dedup(...)`（`sessions/mod.rs:332-353`）写入。它**不进 `ParsedMessage`**——通道 A 只把它当作临时去重集合的成员。

通道 A 的统一消费点：

```rust
5378: fn should_keep_deduped_message(seen_keys: &mut HashSet<String>, message: &UnifiedMessage) -> bool {
5379:     message
5380:         .dedup_key
5381:         .as_ref()
5382:         .is_none_or(|key| seen_keys.insert(key.clone()))
5383: }
```

⇒ **`dedup_key == None` 的消息一律保留且不参与去重**；有键的只在**第一次**出现时保留（先到先得，**不做「取最大值/取最完整」合并**）。这与通道 B 的 Claude 路径形成显著差异（`merge_message_completeness` `claudecode.rs:923-962` 逐字段取 max）。

各解析器的键格式（此处只列一级取证确认过的，其余见逐客户端小节）：

- **Claude assistant**：`"{message.id}:{entry.request_id}"`（`claudecode.rs:629-631`），无 `request_id` 时 `"message:{message.id}"`（`:649-650`），都没有则 `None`（`:668`）。
- **Claude tool_result**：`tool_result_dedup_key(client_id, session_id, usage_key)` = `"{client_id}:tool_result:{session_id}:{usage_key}"`（`claudecode.rs:1005-1007`，常量 `PATH_SCOPED_KEY_MARKER = ":tool_result:"` `:983`）。**路径相关**：包含 session id（= 文件 stem），因此同一 tool result 换文件名就不折叠（`claudecode.rs:985-1000` 的 `dedup_key_is_globally_stable` 只认不带该标记的键）。
- **Codex**：有累计总量时 `"codex:token_count-total:{scope_id}:{provider}:{model}:{in}:{out}:{cached}:{reasoning}"`（`codex.rs:931-940`），否则 `"codex:token_count:{timestamp}:{provider}:{model}:{in}:{out}:{cache_read}:{cache_write}:{reasoning}"`（`:943-953`）。`scope_id` = `session_forked_from_id` → `session_id_from_meta` → 文件 stem（`:661-665`）。

Claude 的**文件内**去重与跨文件去重是两套：

- 文件内：`processed_hashes: HashMap<String, usize>`（`claudecode.rs:489`），命中即 `merge_claude_duplicate`（`:633-637`、`:886-914`，逐字段 `max`，并单调更新 `duration_ms`）；
- 跨文件：`lib.rs:4440-4445` 的裸 `HashSet`（**无合并**）。

这也是 #994 那类「流式重复行更完整」的收益在通道 A 里**只发生在同一文件内**的原因。

#### 3.6.6 `message_count` 语义

【已核实】默认 1（`sessions/mod.rs:111-113`、`:381`），可由解析器改写为「这一行代表多少次请求/轮次」。已知改写点：

| 文件:行 | 语义 |
|---|---|
| `claudecode.rs:1113` | `message.message_count = 0`（某类合成/占位行不计请求数） |
| `codex.rs`（默认 1，未改写） | — |
| `reasonix.rs:132` | `record.requests.clamp(1, i32::MAX)`（Reasonix 的日记录里带 `requests` 字段） |
| `kiro.rs:280` | `turn.total_request_count.unwrap_or(1).max(1)` |
| `kiro.rs:669` | `flat_assistant_turns.max(1)` |
| `kiro.rs:1149` | `prompt_log_count.max(1)` |
| `kiro.rs:619`/`:915`/`:1060`/`:1311` | 固定 1 |
| `droid.rs:540` | `if index == 0 { session_replies } else { 0 }`（整段会话的回复数挂在第一条上） |
| `copilot_desktop.rs:430` | `i32::from(index == 0)`（同理，挂在第一条） |
| `grok.rs:762` | `i32::from(message.is_turn_start)`（只把轮次起点算作「请求」） |
| `crush.rs:104` | `bucket.message_count.max(0)`（按桶聚合后的计数） |
| `crush.rs:130` | `0`（聚合行不再计） |
| `hermes.rs:155` | `row.message_count.max(0)` |
| `hermes.rs:222` | `0` |
| `warp.rs:83`/`:109` | `requests` |
| `zed.rs:165` | `message_count` |
| `zcode.rs:196` | `1` |
| `commandcode.rs:166` | `1`（显式置 1） |
| `freebuff.rs:213` | `1` |

聚合端的计数函数 `summed_parsed_message_count`（`lib.rs:5385-5390`）就是对 `message_count.max(0)` 求和——**所以它可被用作「请求数」而非「行数」**。UsageMesh 走的是这个语义：`collector.rs:108` `messages: message.message_count.max(0)`，且在 `activity.rs:60` 同样。

#### 3.6.7 会话身份（session_id）

【已核实】

- 通用工具 `sessions_id_from_path`（`sessions/utils.rs:496-501`）= 文件 stem，取不到则 `"unknown"`。文档注明「会话 id 不是文件 stem 的客户端保留自己的解析器」（`utils.rs:491-495`）。
- Claude：初值文件 stem（`claudecode.rs:444-448`）；若首行 `isSidechain == true`，则**改写成父 session id**（`:524-537`），以修掉「子代理文件各算一次会话」的膨胀。
- Codex：`session_id_from_path(path)`（`codex.rs:1017`）；记录里的 `session_meta.id` 另存 `state.session_id_from_meta`（`:435-437`），只用于 dedup scope 与 fork 判定，**不覆盖** `UnifiedMessage.session_id`。
- **`session_id` 没有全局归一化**（没有 trim/小写/去前缀），跨客户端也不统一。

#### 3.6.8 重复出现的处理（同一消息多次出现）

三类机制，作用域不同：

1. **`dedup_key` 集合**（跨文件、逐 lane）：`should_keep_deduped_message`（`lib.rs:5378`）与内联等价实现（OpenCode `:4387`/`:4410`、Claude `:4443`、Devin CLI `:5120`、CodeBuddy `:5180`、Hermes `:4977`）。**先到先得。**
2. **解析器内合并**（单文件）：Claude `merge_claude_duplicate`（`claudecode.rs:886-914`，逐字段 max + duration 单调）、`merge_claude_tool_result_duplicate`（`:964-973`，input 取 max，时间戳取较新）；Codex 用累计总量 `previous_totals` 与 `state` 做增量推导（`codex.rs:559-611`），根本不产出重复行。
3. **框架级合并**（仅通道 B）：`retain_observed_messages`（`lib.rs:942-966`）、`merge_claude_cross_file_duplicate`（`lib.rs:4242-4258`）——**UsageMesh 用不到**。

#### 3.6.9 `date` 的时区来源与跨日边界

【已核实】三层：

1. **构造期**：`UnifiedMessage::new_full` → `timestamp_to_date(timestamp)`（`sessions/mod.rs:367`）→ `chrono::Local`（`sessions/mod.rs:1018-1020`）。即「解析时的机器本地时区」。
2. **重算期**：`rebucket_parsed_days`（`lib.rs:5286`）在 `bucket_timezone` 非空且可解析时用 `BucketTimezone::day_key` 覆盖（`lib.rs:5301-5304`）；时区无法表示该瞬时则返回空串并被忽略（保留旧值）。
3. **筛选期**：`filter_parsed_messages`（`lib.rs:5392-5411`）对**字符串**比较，不做任何时间戳换算。

⇒ **跨日边界**= 该时区下的本地午夜。**若 `bucket_timezone` 为 `None`（UsageMesh 读不到 settings.json 的情形），边界是「采集时机器本地时区」，且历史数据会随机器时区变化而重新分桶**（这正是 `bucket_tz.rs:1-28` 记录的原始 bug 的成因）。

另一处时区相关：Crush lane 显式把 `BucketTimezone` 传进解析器（`lib.rs:5032-5038`；`sessions::crush::parse_crush_sqlite_in(db, &tz)`，`crush.rs:60`），注释 `lib.rs:5031` 指向通道 B 的同名处理。Crush 是**唯一**在通道 A 里接收时区参数的解析器。

#### 3.6.10 `since` 边界是字符串还是时间戳

【已核实】**字符串日期比较**（`lib.rs:5403-5405`），与 `year`（前缀）、`until`（`<=`）同一族。没有时间戳参与。

---

### 3.7 缓存与增量

#### 3.7.1 `parse_local_clients` 完全不走持久缓存

【已核实】通道 A 的源码里没有任何 `message_cache` 引用：`lib.rs:4334-5282` 整段只调用 `scanner::*`、`sessions::*`、`bucket_tz::*` 与本地 helper。对比通道 B 的 `parse_all_messages_with_pricing_with_cache_policy`（`lib.rs:766-…`）大量使用 `message_cache::SourceMessageCache`、`CachedSourceEntry`、`SourceFingerprint`、`FingerprintStatus`。

**对 UsageMesh 的后果：**

- **不存在「增量扫描」**。每次 `parse_local_clients` 都是**全量重解析**：遍历 home 下所有目标目录、打开每个匹配文件、逐行解析。`since` 只在**解析完之后**按 `date` 过滤（`lib.rs:5275`），**不减少 IO 与解析量**。
- **不读也不写 tokscale 的缓存目录**。因此：
  - UsageMesh 不会污染、不会失效、也不会受益于 tokscale CLI 自己的缓存；
  - 反过来，`TOKSCALE_CONFIG_DIR`、`~/.config/tokscale/cache/source-message-cache-v2/` 里的内容对 UsageMesh 完全无影响；
  - UsageMesh 的 `since` 增量「省下」的只是它自己下游的聚合与计价，**磁盘扫描成本与 `since=None` 相同**。
- **每条消息的 `date` 更无法通过缓存稳定**：通道 B 会把解析结果按文件指纹缓存起来，重复扫描时复用同一批 `date`；通道 A 每次都按**当次** `chrono::Local` 重新推导（除非钉了时区），所以**同一份历史在不同时刻/不同时区重扫，日键可能变化**。

#### 3.7.2 缓存机制本身（仅作对照，UsageMesh 不涉及）

【已核实】`message_cache.rs`：

- 版本常量：`CACHE_FORMAT_VERSION: u32 = 5`（`message_cache.rs:31`），`LEGACY_CACHE_FORMAT_VERSION = 4`（`:32`），分片目录名 `"source-message-cache-v2"`（`:35`），锁文件名 `"source-message-cache.lock"`（`:36`）。
- 旧单体文件 `source-message-cache.bin`（`:41`）会被**一次性删除**（`remove_legacy_monolith_cache` `:86-95`，`Once` 包装 `:100-103`），探三个历史位置：`cache_dir()`、`paths::legacy_dirs_cache_dir()`、`paths::legacy_dot_cache_tokscale_dir()`（`:87-94`），且在后两者上受 `TOKSCALE_CONFIG_DIR` 隔离门控（`paths.rs:145-150`、`:162-167`）。
- 位置：`cache_dir()`（`message_cache.rs:53-62`）→ `paths::get_cache_dir()`（`paths.rs:120-122`）= `get_config_dir()/cache`；`get_config_dir()`（`paths.rs:94-111`）= `TOKSCALE_CONFIG_DIR` 优先，macOS `$HOME/.config/tokscale`，其余 `dirs::config_dir()/tokscale`（Windows: `%APPDATA%\tokscale`），最后兜底 `./.tokscale`。
- 分片：256 片（`CACHE_SHARD_COUNT = 256` `:42`），单分片上限 256 MiB（`:43`）。
- 指纹：采样 5 个点、每点 4096 字节（`FINGERPRINT_SAMPLE_BYTES = 4096` `:44`、`FINGERPRINT_SAMPLE_POINTS = 5` `:45`）。
- 代数号 `parser_generation()`（`message_cache.rs:1044-1056`）：把每个客户端的 `parser_version(client)` 用 FNV-1a 混成一个 u64，供「派生缓存」（TUI 聚合缓存）失效使用；注释说明刻意遍历 `ClientId::ALL` 而非启用集（`:1036-1040`），并解释为何不用 `DefaultHasher`（`:1042-1043`）。
- 逐客户端解析器版本（`parser_version(client)` `:1058-…`）：Codex = 7（`:1068`，注释记录 v6→v7 就是上面那条 reasoning 子集修正）、Jcode = 7（`:1079`）、Copilot = 9（`:1089`）、Pi = 2（`:1092`）、DevinCli = 3（`:1097`）、DevinDesktop = 2（`:1100`）；Claude 有一条**警告**（`:1101-1109`）：随便 bump 会丢掉不可恢复的保留历史（对应 #994 的 `RetainObserved`）。
- 保留策略 `HistoryRetention`（`lib.rs:907-924`）：`LiveFileOnly` vs `RetainObserved { key_is_globally_stable }`；后者用于「原地重写 transcript」的客户端（Claude），保留条件是 dedup key 全局稳定（`lib.rs:942-966`）。
- 失效判定 `FingerprintStatus::{Unchanged, Changed}`（`lib.rs:1014-1033`）；条目若缺 `retention_provenance` 标记会被**升级重建一次**（`lib.rs:994-1000`，注释 `:985-993`）。

**结论重申**：上面这一整套对 `parse_local_clients` **全是死代码路径**。可以用一行验证：`rg "message_cache" crates/tokscale-core/src/lib.rs` 的命中，除模块声明/再导出（`lib.rs:10` `mod message_cache;`、`:26` `pub use message_cache::parser_generation;`）与两处通道 B 辅助类型（`lib.rs:398` `related_files: Vec<message_cache::RelatedFileFingerprint>`、`:412` `fingerprint: &message_cache::SourceFingerprint`）之外，其余全部落在 `lib.rs:766-…` 的通道 B 区段与测试模块里；`4334-5282` 区间（含分派段 `4367-5246`）没有任何命中。

---

### 3.8 与 v4.18.0 差异

> 差异必须给证据。以下全部来自同一克隆的两个标签之间的 `git diff`（`git diff v4.14.0 v4.18.0 -- crates/tokscale-core/src`），或对 `v4.18.0` blob 的直接读取。无法确认的进 §3.10。

#### 3.8.1 客户端注册表：+6

【已核实】`git diff -U0 v4.14.0 v4.18.0 -- crates/tokscale-core/src/clients.rs` 的新增项：

| 索引 | `id` | display | relative | pattern |
|---|---|---|---|---|
| 50 | `lmstudio` | LM Studio | `server-logs` | `*.log` |
| 51 | `unsloth` | Unsloth | `studio.db` | `studio.db` |
| 52 | `hindsight` | Hindsight | `usage` | `*.jsonl` |
| 53 | `micode-desktop` | Xiaomi MiMo AI | `mimocode` | `*.db` |
| 54 | `muse` | Muse Code | `muse/sessions` | `session.jsonl` |
| 55 | `antigravity-extension` | Antigravity IDE Extension | `.gemini/antigravity/conversations` | `*.db` |

六个新客户端全是 `parse_local: true` / `submit_default: true` / `headless: false`。⇒ **v4.18.0 的 `ClientId::COUNT` 从 50 变 56**，`parse_local_clients` 的默认客户端集合随之扩大 6 个。UsageMesh 侧用 `ClientId::iter()` 枚举（`collector.rs:197`）与按 `ClientId::iter()` 逐客户端遍历（`evidence.rs:736` `for client in ClientId::iter() {`，函数 `scan_client_effort_sources` `evidence.rs:734`），所以**升级依赖版本会改变 UsageMesh 报告的客户端全集与输出行数**。

补充【已核实，直接读 v4.18.0 blob】：
- `v4.18.0:clients.rs:1012-…` `LmStudio` 的根是 `PathRoot::EnvVar { var: "LM_STUDIO_HOME", fallback_relative: ".lmstudio" }`；
- `v4.18.0:clients.rs:441-449` **`cursor` 仍是 `parse_local: false`**（只有 `pattern` 变成 `usage*.json|usage*.csv`）。

唯一属性改动：`Cursor` 的 `pattern` 从 `usage*.csv` 改为 `usage*.json|usage*.csv`（`parse_local` 仍是 `false`）。

**签名稳定性**【已核实】两个版本的关键 API 逐字相同：
- `v4.18.0:lib.rs:5380` `pub fn parse_local_clients(options: LocalParseOptions) -> Result<ParsedMessages, String> {`（v4.14.0 在 `lib.rs:4334`）；
- `v4.18.0:lib.rs:448-457` 的 `LocalParseOptions` 字段集与 v4.14.0 `lib.rs:428-439`（含 `#[derive(...)]`，`:429` 起为结构体本体）**完全一致**（含 `scanner_settings`）。

⇒ UsageMesh 的调用代码（`collector.rs:211-219`、`activity.rs:12-20`）在 v4.18.0 下**仍可编译、语义不变**；版本差异全部落在「客户端集合」「`ParsedMessage` 字段」「逐客户端解析结果」三处，而不在 API 形状。

#### 3.8.2 `TokenBreakdown` 与 `ParsedMessage`：加了 `cache_write_1h`

【已核实】`v4.18.0:crates/tokscale-core/src/lib.rs` 的 `TokenBreakdown` 在 `cache_write` 与 `reasoning` 之间多一个字段：

```rust
    /// Subset of `cache_write` billed at Anthropic's 1-hour cache TTL rate
    /// (2x input) rather than the default 5-minute TTL rate (1.25x input).
    /// Never exceeds `cache_write` and is not added again in `total()`.
    #[serde(default)]
    pub cache_write_1h: i64,
```

- **「cache_write_1h 是 cache_write 的子集，且不加进 `total()`」这个约定是 v4.18.0 的**。v4.14.0 的 `total()`（`lib.rs:271-279`）五桶相加，没有第六桶。
- `ParsedMessage` 在 v4.18.0 也加了同名字段（`v4.18.0:lib.rs:363`，位于 `cache_write` 之后、`reasoning` 之前，doc 注释指向 `TokenBreakdown::cache_write_1h`）。
- 行号已回源补齐（`git show v4.18.0:crates/tokscale-core/src/lib.rs`）：`TokenBreakdown.cache_write_1h` 在 `v4.18.0:lib.rs:261`，`ParsedMessage.cache_write_1h` 在 `v4.18.0:lib.rs:363`。

#### 3.8.3 `ParsedMessage` 加了 cost / cost_source / service_tier（16 → 20 字段）

【已核实】`v4.18.0:crates/tokscale-core/src/lib.rs` 的 `ParsedMessage` 尾部多出三个字段（`:373` / `:374` / `:375`）：

```rust
    /// Cost in USD as the parser reported it. This lane applies no pricing, so
    /// the figure is only meaningful when `cost_source` is
    /// [`CostSource::ProviderReported`]; consumers price every other row from
    /// its tokens, exactly as the submit lane reprices only rows without an
    /// authoritative cost.
    pub cost: f64,
    pub cost_source: CostSource,
    pub service_tier: Option<String>,
```

⇒ v4.18.0 的 `ParsedMessage` 字段数从 v4.14.0 的 **16 个**（`lib.rs:341-356`）增到 **20 个**（`v4.18.0:lib.rs:349-375`），新增四项：`cache_write_1h`（`v4.18.0:lib.rs:363`）、`cost`（`:373`）、`cost_source`（`:374`）、**`service_tier`（`:375`，`Option<String>`，原报告漏列）**。

**对 UsageMesh 的影响**：v4.18.0 的 `ParsedMessage` **带 cost 语义**（仅当 `ProviderReported` 时可信，`v4.18.0:lib.rs:368-374`）。UsageMesh 写于 v4.14.0 时代，所以它无条件自行计价（`collector.rs:113-125` 的 `priced_metrics_from_message`）。**若把依赖升到 v4.18.0，UsageMesh 的「解析器上报价」会被忽略**（它不读 `cost`/`cost_source`），但一旦上游把 `cost` 语义接进 `parse_local_clients` 的输出契约，本地计价与 provider 上报价就可能出现**双轨不一致**。

#### 3.8.4 解析器与扫描器的大规模改动

【已核实】`git diff --stat v4.14.0 v4.18.0 -- crates/tokscale-core/src`（节选，未列全——该 diff 共 70 个文件，其中 8 个为新增文件、单列于下段；下表为其中**既有文件**按变更量降序的前 20 项，变更量取 `git diff --numstat` 的「增 + 删」行数）：

| 文件 | 变更 |
|---|---|
| `lib.rs` | 6302 |
| `kiro.rs` | 4072 |
| `message_cache.rs` | 4023 |
| `scanner.rs` | 2314 |
| `openclaw.rs` | 2089 |
| `commandcode.rs` | 1970 |
| `pricing/lookup.rs` | 1691 |
| `sessions/opencode_schema.rs` | 1221 |
| `sessions/opencode.rs` | 993 |
| `codex.rs` | 935 |
| `pricing/mod.rs` | 932 |
| `cherrystudio.rs` | 921 |
| `antigravity_cli.rs` | 822 |
| `cursor.rs` | 705 |
| `micode.rs` | 693 |
| `copilot.rs` | 633 |
| `copilot_vscode.rs` | 491 |
| `pi.rs` | 470 |
| `kimi.rs` | 451 |
| `dsh.rs` | 433 |

新增文件（v4.18.0 有、v4.14.0 无，共 8 个，行数 = 该 diff 的插入数）：`crates/tokscale-core/src/recovery.rs`（643 行）、`sessions/copilot_session_store.rs`（457）、`sessions/hindsight.rs`（465）、`sessions/lmstudio.rs`（882）、`sessions/muse.rs`（468）、`sessions/unsloth.rs`（535）、`sessions/micode_io_tests.rs`（411）、`crates/tokscale-core/src/http.rs`（106，**原报告漏列**）。

⇒ **在 UsageMesh 的使用面上，「同一客户端解析结果是否一致」无法靠版本号判断**。凡是依赖逐客户端字段映射的结论，必须锁 `1ec865d4`（或重新核对 v4.18.0 代码）。

#### 3.8.5 未变的部分

【已核实】`git diff --stat v4.14.0 v4.18.0` 的输出里**没有** `bucket_tz.rs`，也没有 `model_alias.rs` ⇒ 这两个文件在两版本间**零改动**。

但证据链要分两段说清楚：**`canonical_model_id` 与 `normalize_syntactic` 并不定义在 `model_alias.rs`，而在 `lib.rs`**（`v4.14.0:lib.rs:87-89` / `v4.18.0:lib.rs:89`、`v4.18.0:lib.rs:131`；`lib.rs` 两版差异 6302 行）。所以「归一化规则两版一致」**不能**由 `model_alias.rs` 未变推出，只能由函数体逐字比对支撑：两版 `normalize_syntactic` 体（`v4.14.0:lib.rs:129-167` ≡ `v4.18.0:lib.rs:131-169`）经 `git show` 抽取后字符串完全相等。
⇒ **`BucketTimezone` 的两态语义、`day_key` / `hour_key` 格式、`from_scanner_settings` 的降级行为，以及 `canonical_model_id` / `normalize_syntactic` 的归一化规则，在 v4.14.0 与 v4.18.0 完全一致**（前者由 `bucket_tz.rs` 零改动支撑，后者由函数体比对支撑）。这是本报告里「版本无关」结论的安全区。

---

### 3.9 对 UsageMesh 的实际影响汇总（跨节收口）

这一节把前面所有发现收成「UsageMesh 会因此出什么错/受什么限」，供正文引用。

#### 3.9.1 可安全依赖的部分

| 事实 | 依据 |
|---|---|
| `LocalParseOptions` 的七个字段与 `parse_local_clients` 的签名在 v4.14.0→v4.18.0 未变 | `lib.rs:428-439` / `v4.18.0:lib.rs:448-457`；`lib.rs:4334` / `v4.18.0:lib.rs:5380` |
| `canonical_model_id` 的归一化规则（小写、剥 tier、剥 `-YYYYMMDD`、claude `.`→`-`、`anthropic/claude-*` 前缀折叠） | `lib.rs:87-89` + `:57-73` + `:129-184` |
| `BucketTimezone` 的两态与 `day_key` 格式 | `bucket_tz.rs:35-45`、`:108-113`、`:137-146`；`git diff` 证实两版本零改动 |
| `since`/`until` 是字符串日期比较 | `lib.rs:5392-5411` |
| 五桶语义 + `total()` 含 reasoning + 饱和累加 | `lib.rs:249-286` |
| `message_count` 默认 1 | `sessions/mod.rs:111-113`、`:381` |

#### 3.9.2 必须防御的部分

| 风险 | 机制 | 证据 |
|---|---|---|
| **`timestamp` 单位不保证是毫秒** | 多数解析器产出毫秒，但 `parse_timestamp_value` 会把 < 1e12 的值按秒 ×1000（`utils.rs:203-221`），而 `activity.rs:49-53` 与 `collector.rs:38-44` 又各做一次启发式归一 ⇒ **存在双重 ×1000 的可能路径**：若某解析器直接产出秒值且未被上游归一，UsageMesh 的 `abs() < 1e11` 判定会把它乘成毫秒；但若某解析器产出的是「已被上游乘过的毫秒」，则正确。**当前代码是对的，但它是靠两层启发式相撞得到的，不是靠契约。** | `utils.rs:203-221`、`collector.rs:38-44`、`activity.rs:49-53` |
| **`client` 可能是 `cc-mirror/<variant>`** | `claudecode.rs:68`、`:437-440` ⇒ UsageMesh 的 `RowKey.client` 会出现 `cc-mirror/...` 这种非注册表值，`supported_clients()`（`collector.rs:196-202`）里没有它 ⇒ 前端按「已知客户端」做过滤/图标映射时会漏 | `collector.rs:196-202`、`:237` |
| **请求数在不同客户端不同质** | 见 §3.5.13 表；`collector.rs:108` 用 `message_count.max(0)` 当 `Metrics.messages` | `collector.rs:108`、`activity.rs:60` |
| **部分客户端无 `dedup_key`**（比例无法用「约 1/3」概括，无键名单见 §3.5.13） | `lib.rs:5378-5383`：`dedup_key == None` 的消息一律保留且不参与去重 ⇒ 同树双份转录会双计 | §3.5.13 |
| **`since` 增量扫描不省 IO** | 通道 A 不走 `message_cache`（§3.7.1）；`since` 只在解析后过滤（`lib.rs:5275`） | `lib.rs:4334-5282` 无 `message_cache` 引用 |
| **UsageMesh 的 `since` 取自本机 `Local`，而 tokscale 的 `date` 可能被 settings 钉住** | `main.rs:394-396` 用 `Local::now()`；`parse_local_clients` 会读 `scanner_settings.bucket_timezone` 并 rebucket（`lib.rs:5273`）+ 按其过滤（`lib.rs:5404`）。若本机 `~/.config/tokscale/settings.json` 钉了非本机时区，**UsageMesh 的「最近 2 天」窗口与它拿到的 `date` 处于两个不同时区**，`merge_incremental`（`collector.rs:375-401`）的 `row.date < since` 裁剪就会出现边界日误差 | `main.rs:364-401`、`lib.rs:5273`、`lib.rs:5404`、`collector.rs:375-401` |
| **`counts` 不能当行数** | Devin Desktop / Reasonix / Zcode / Kiro 等（§3.4.4） | `lib.rs:5142-5167`、`:4685-4688`、`:4806`、`:5017-5027` |
| **两次调用 `parse_local_clients` 的成本不会共享任何结果** | `collector::collect`（`collector.rs:211`）与 `local::activity::scan`（`activity.rs:12`）各自独立全量扫描 | 两处调用点 |
| **token 数在 5 类客户端上是「猜」的** | Kiro（无显式上报时走 context 占比推算 / 字符估算）、CommandCode 全量字符估算；zcode 无 usage 时估算；freebuff 全量估算；goose 的 `reasoning` 是差值推导（源码自标 INFERRED）。**`ParsedMessage` 无来源可信度字段，UsageMesh 无法标注** | §3.5.11 |
| **Kiro 的「请求数」也可能不可比** | `message_count` 共**七处赋值、覆盖六条来源路径**（IDE 一条路径两处）：`total_request_count.unwrap_or(1).max(1)` / 1 / `flat_assistant_turns.max(1)` / 1 / 1 / `prompt_log_count.max(1)` / 1 | `kiro.rs:280`、`:619`、`:669`、`:915`、`:1060`、`:1149`、`:1311` |
| **Zed 的 ×1.1 加成拿不到** | 只在通道 B 的 `pricing_multiplier`（`lib.rs:4190-4210`）生效 | 同左 |

#### 3.9.3 UsageMesh 完全没有覆盖的客户端

v4.14.0 的注册表里有、但 UsageMesh 收不到任何消息的客户端（**注意不是「`parse_local_clients` 会产出」——`micode` 与 `fx` 恰恰是 `parse_local: true` 却在通道 A 无分派**）：
- `micode`（MiMo Code）——**通道 A 根本不解析**（§3.3.4）；`MiMoCode = 30` 在注册表里（`clients.rs:728-737`），但分派区段 `lib.rs:4367-5246` 无该 lane；
- `fx`（Fx）——同理，**`parse_local: true`（`clients.rs:979-989`）但通道 A 无分派**，只在通道 B 被调用（`lib.rs:2510` / `:2520` / `:2531`；§3.3.4、§3.5.9）；
- `cursor`——`parse_local: false`（`clients.rs:447`），任何版本都不产出，只在通道 B 被调用（`lib.rs:1925-1926`）；
- `synthetic`——不是 `ClientId`，client 值是 `"synthetic"` 或原客户端名（`synthetic.rs:85-96` 只改 model/provider，**不改 client**），UsageMesh 若按 client 分组会看到「原客户端 + synthetic 模型名」的混合形态。

#### 3.9.4 桶口径的一致性与不一致（重要正面结论）

**一致的部分**【已核实】UsageMesh 的 `Metrics`（`model.rs:16-45`）与 tokscale 的 `TokenBreakdown`（`lib.rs:249-256`）**逐桶一一对应**：`input` / `output` / `cache_read` / `cache_write` / `reasoning` 五个 i64，`total_tokens()`（`model.rs:29-35`）与 `total()`（`lib.rs:271-279`）**都是五桶 `saturating_add` 求和**，`Metrics::add`（`model.rs:37-45`）与 `add_assign_saturating`（`lib.rs:263-269`）**逐桶语义相同**。

⇒ **UsageMesh 的「token 桶语义与 tokscale 一致」是代码级成立的**，UsageMesh 的注释也这么声明（`collector.rs:209-210`：“Tokscale remains the sole canonical local accounting pass: discovery, parsing, deduplication, token-bucket semantics and model attribution”）。**桶定义一致；不一致的是各解析器往桶里装了什么**（§3.5.11 的估算、Codex 的 `cache_write = 0`、Gemini 的 `input += tool` 等）。

**不一致的部分**：`Metrics.cost_usd`（`model.rs:25`）由 UsageMesh 自己按 `price_book.quote_on_date(model, date, tier, metrics)` 填（`collector.rs:119`、`:298-300`），而 tokscale 在 `ParsedMessage` 层面**根本没有 cost**（v4.14.0，`lib.rs:340-357`）。⇒ 不存在「两套 cost 打架」的问题，但也意味着**UsageMesh 的 cost 完全建立在「桶语义正确」这个前提上**——一旦上游把估算值塞进桶（如 Kiro/CommandCode/Freebuff），UsageMesh 会给出一个**由估算 token 推出的、看起来精确的美元数**。

---

### 3.10 未验证项

1. **`ParsedMessage.timestamp` 是否真的对通道 A 分派的 47 个 ClientId 全部都是毫秒——大部分已核实，少数仍未确认**。
   已核实为毫秒：Claude（`claudecode.rs:875-879`）、Codex（`codex.rs:909-913`）、Grok（`grok.rs:380`、`:696-699`、`:1319`、`:1443-1456`）、Kimi legacy（**秒 ×1000**，`kimi.rs:302-309`）与 kimi-code（毫秒，`kimi.rs:236-239`）、gjc（`gjc.rs:161-166`）、junie（`junie.rs:67-68`）、mcode（`mcode.rs:170-176`）、trae（**秒 ×1000**，`trae.rs:74-78`）、warp（`warp.rs:41-45`）、hermes（**秒 → ms**，`hermes.rs:99`/`:114`/`:144`、`utils.rs:518-529`）、dsh（事件 `time` i64 **直用不换算**，`dsh.rs:288-293`）、devin CLI（SQL 内 `created_at * 1000`，`devin.rs:163`/`:169`）、copilot_desktop（`parse_iso8601_timestamp_ms` `:592-624`）、copilot_vscode（**`timestamp` 直用、缺失即 0、无解析函数**，`copilot_vscode.rs:179`）、antigravity（`antigravity.rs:50`/`:111-121`）、antigravity_cli（秒+纳秒 → ms，`antigravity_cli.rs:646`）、kiro CLI（**秒 ×1000**，`kiro.rs:300-311`）、qwen / zed / claude 系均走 `parse_timestamp_str`（`utils.rs:223-257`）、goose（`parse_created_at` + `timestamp_secs_to_ms`，`goose.rs:31-46`/`:111`）。
   **仍未逐一确认**：`zcode` SQLite 的 `started_at`（`zcode.rs:528-544` 注释标「按毫秒使用」但无换算）、`kiro` workspace-session（直接用 mtime）、`workbuddy`/`codebuddy` 的 extension log 行首时间（`tencent_buddy.rs:426-462`，本地 naive 优先）。
   结论：**UsageMesh 保留 `abs() < 1e11 ⇒ *1000` 的归一仍是必要的**，因为上游既有「秒 ×1000 后产出毫秒」也有「原值直用」两种写法，且 `parse_timestamp_value`（`utils.rs:203-221`）本身还会按 1e12 阈值二判。
2. **客户端覆盖与 v4.14.0 注册表的口径差异**：`kilo`/`goose`/`crush`/`trae`/`zed`/`kimi`/`grok`/`zcode`/`dsh`/`droid`/`pi`/`jcode`/`openclaw`/`workbuddy`/`codebuddy` 全部存在；`copilot` 是**一个注册项**（`clients.rs:586`）但**三个解析器**（`copilot.rs` / `copilot_desktop.rs` / `copilot_vscode.rs`，由 `lib.rs:4474-4519` 合成）；`micode` 的注册名是 `MiMoCode` → `as_str() == "micode"`（`clients.rs:728-737`）；`antigravity` 与 `antigravity-cli` 是**两个不同客户端**（`clients.rs:619` 与 `:744`）；**`fx` 有 `ClientId::Fx`（`clients.rs:979`）而 `synthetic` 没有 `ClientId`**（纯字符串，§3.3.4）。
3. **`ClientId::MiMoCode` 的变体名 vs 文档所写的 `Micode`**：grep `Micode`（不加 `o`）在整个 `crates/tokscale-core/src` 下**无命中**；`lib.rs:1707` 用 `ClientId::MiMoCode` 合法，`clients.rs:728` 的变体是 `MiMoCode`。⇒ 正确写法是 `MiMoCode`，`as_str()` 才是 `"micode"`。
4. **`ScannerSettings` 的实际取值未验证**。UsageMesh 的 `load_scanner_settings`（`collector.rs:78-93`）会读本机 `dirs::config_dir()/tokscale/settings.json`。本会话**未读取、也未打印任何真实用户配置文件内容**（遵守约束）。⇒ **本机 `bucket_timezone` 究竟是 `None` 还是已钉、`extra_scan_paths` 里有什么，全部未验证。** 这一项直接决定 §10.1 的风险 R2/R9 在当前机器上是否已实际发生。
5. **一级取证的阅读覆盖率**（二级代理的覆盖标注见各条末尾）。`lib.rs` 14921 行、`claudecode.rs` 3473 行、`codex.rs` 2967 行、`message_cache.rs` 5473 行。完整/近完整读过的区段：`lib.rs:1-560`、`:715-1240`、`:4140-5440`、`:5468-5599`；`claudecode.rs:400-610`、`:610-850`、`:865-1050`、`:1078-1137`；`codex.rs:20-240`、`:308-780`、`:900-1060`、`:1120-1200`；`clients.rs:1-120`、`:200-1020`；`scanner.rs:1-190`、`:320-500`、`:1380-1460`；`bucket_tz.rs:1-160`；`paths.rs:40-169`；`sessions/mod.rs:1-330`、`:330-500`、`:1000-1040`；`synthetic.rs:30-140`；`utils.rs:200-260`、`:488-547`；`aliases.rs:1-110`、`:126-175`、`:183-227`；`kiro.rs` 只 grep 了 `estimate_tokens` 相关行。其余（尤其 `message_cache.rs` 5.4k 行、`pricing/*`、`aggregator.rs`、`wiki.rs`、`content_extractor.rs`、`sessionize.rs`、`mcp.rs`）**只做了定向 grep**。
6. **`counts` 与 `messages` 的差异集合**未穷举。已确证至少 Devin Desktop（`lib.rs:5142-5165`）、Reasonix（`:4685-4687`）、Zcode（`:4806`）、Kiro（`:5017-5027`）、Copilot Desktop（`message_count` 仅首条 1）、Hermes（`hermes.rs:208-223` 只在第一行带计数）、以及所有用 `summed_parsed_message_count`（`:5385-5390`）的 lane 存在差异；**是否还有别的 lane 也存在差异，未逐一验证。**
7. **`parse_local_clients` 里 `lib.rs:4454` 计算的 `is_headless` 是否真的生效**。`apply_headless_agent`（`lib.rs:4184-4188`）只在 `message.agent.is_none()` 时写 `"headless"`。Codex 解析器自身在 headless 会话里已写 `Some("headless")`（`codex.rs:622-626`），所以这里的调用可能是冗余的。**未验证在哪些实际路径下它才是唯一的写入者。**（通道 B 的同等逻辑在 `lib.rs:857-874`。）
8. **`cache_write_1h` 在 v4.18.0 的具体填充点**未验证（只确认了字段与注释存在，未追谁写它、哪几个客户端填、`apply_pricing_if_available` 是否按 1h 档计价）。
9. **v4.15.0 / v4.16.0 / v4.17.0 的中间差异**未逐版本追溯。本克隆只有 `v4.14.0`、`v4.17.0`、`v4.18.0` 三个标签，`v4.15.0` 不存在（`git rev-parse v4.15.0` 实报 `fatal: ambiguous argument 'v4.15.0': unknown revision or path not in the working tree.`，退出码 128；`Needed a single revision` 是 `rev-parse --verify` 系的报错文本，与本命令不符）。⇒ **「v4.14.0 vs v4.18.0」的差异清单里，无法区分哪些是 v4.17.0 引入的。**
10. **UsageMesh 的 `since` 实参格式已追到，但语义链未端到端验证**。已核实：`main.rs:394-396` 生成 `(Local::now().date_naive() - Duration::days(2)).format("%Y-%m-%d")`，`main.rs:389` 全量分支传 `None`；`merge_incremental`（`collector.rs:375-401`）按 `row.date.as_str() < since` 裁剪旧行、并按 `partial.requests` 的最小 `timestamp_ms` 裁剪旧请求明细。格式确认是 `YYYY-MM-DD`。**未验证**：`overlap_start`/`stale_cache` 判定（`main.rs:364-367`）与 `activity.rs:9-11` 的 89 天窗口在跨时区/跨月边界上的行为。
11. **`error` 语义**：`parse_local_clients` 返回 `Result<ParsedMessages, String>`。已核实唯一的 `Err` 来源是 `get_home_dir_string` 失败（`lib.rs:4337` + `:723-725`）。**其余客户端解析失败是否全部静默**（读文件失败返回空 `Vec`，如 `claudecode.rs:476-479`、`codex.rs:1012-1015`；SQLite 打不开返回空，如 `opencode_schema.rs:853`、`synthetic.rs:113-115`）未穷举，但抽样到的都是静默。⇒ UsageMesh 的 `.context("Tokscale failed to scan local AI coding-tool sessions")`（`collector.rs:220-221`）几乎只在「找不到 home」时触发。
12. **`sessions/mod.rs` 与 `lib.rs` 里那些 `#[cfg(test)]` 区段**（`sessions/mod.rs:1030-2067`、`lib.rs:5442-14921`）**未逐行阅读**。本报告的所有行号断言都落在生产代码区；引用的测试只用作「行为佐证」（如 `lib.rs:5497-5519` 的饱和断言），且已注明是测试。
13. **`kiro.rs` 的 token 估算结论经二次核验**（grep 到 `estimate_tokens` 调用点与 `DEFAULT_CONTEXT_WINDOW = 200_000`），但**该文件 2235 行的其余部分（尤其 704-837、1373-2235）未逐行读**；Kiro 的六条来源路径的取舍与 `suppress_snapshots_covered_by_executions` 的边界行为以二级取证报告为准，一级抽查仅覆盖 `kiro.rs:230-251`、`:385-386`、`:435` 三处注释与常量。
14. **`provider_identity.rs` 的 `canonical_provider`（`:62-64`）与 `inferred_provider_from_model`（`:260`）实现体未逐行读**（只读了 grep 定位到的签名行）。所有「过/不过 canonical_provider」的结论都建立在「某解析器文件里有没有该符号调用」的 grep 证据上，而非对归一化规则本身的核验。
15. **跨客户端汇总口径未经运行验证**。本报告全部结论来自**只读源码阅读与 grep**，**没有编译、没有运行任何测试、没有构造 fixture 复现**（任务约束禁止 `cargo build`/`cargo test`）。任何「某客户端在真实数据上产出什么」的问题，都需要另行用真实或合成数据实测。

## 4. 设备侧采集、账本结构与聚合口径

### 4.1 CLI 命令面与调用链

- 命令枚举 `CommandKind` 定义于 `rust-cli/src/main.rs:40-110`，共 10 个子命令；分发在 `main.rs:640-684`（`real_main`），错误统一 `eprintln + exit(1)`（`main.rs:686-691`）。
- `serve`（默认端口 43821，`main.rs:42-52`）→ `local::serve(port, !no_open, home, data_dir)`（`main.rs:658-663`）→ `local/mod.rs:395-542`。**不依赖 GitHub 配置**（注释 `main.rs:41`）。
- `setup` → `setup()`（`main.rs:513-539`）：`resolve_token`（`main.rs:112-137`，顺序为显式 `--token` → `USAGEMESH_GITHUB_TOKEN`/`GITHUB_TOKEN`/`GH_TOKEN` → `gh auth token` → 隐藏提示）→ `github::ensure_user_fork`（`github.rs:128-175`）→ `config::new_config`（`config.rs:107-121`）→ `GithubClient::validate`（`github.rs:190-208`）→ `resolve_dashboard_password`（`main.rs:150-168`）→ `config::save` → `publish_dashboard_access`（`main.rs:181-191`）→ `finish_onboarding(&config, no_schedule)`（`main.rs:480-511`：`:482` 先 `run_sync(true, false)` 做首次全量同步；`:483-496` **仅当 `!no_schedule`** 才调 `scheduler::install(config.interval_seconds)` 并回写 `scheduler_revision`（`:487-489`），安装失败只 `eprintln` 告警、不阻断引导（`:491-494`）；`:505-509` 按该开关打印常驻代理 `disabled (--no-schedule)` 或 `enabled`）。
- `--no-schedule`（`Setup` 的定义 `main.rs:67-69`、`Join` 的定义 `main.rs:80`；命令面区间 `main.rs:64-95`）：setup/join 用它决定**是否安装常驻采集代理**——两个子命令都把它透传给 `finish_onboarding`（`main.rs:647-648`、`:653-654`），带该开关时引导流程只保留一次全量同步、不向 OS 注册常驻任务（`main.rs:483-496,505-509`）。上游 `README.md:88` 写作 "installs an OS-supervised **resident sync agent** unless you pass `--no-schedule`"，与代码一致；漏掉它会把"UsageMesh 一定有常驻调度"当成前提。
- `join` → `join()`（`main.rs:541-556`）：口令码经 `config::from_join`（`config.rs:123-152`）还原 repo + `dashboard_key`，然后校验远端 → 保存 → 首次全量同步。
- `password` → `set_dashboard_password`（`main.rs:558-567`）：重新包裹并覆盖 `um-dashboard` 清单。
- `sync` → `run_sync(full, quiet)`（`main.rs:282-478`）；`status` → `status()`（`main.rs:569-613`，纯本地、不发网络请求；仅额外执行 OS 查询命令探测常驻任务是否安装——Windows `schtasks.exe /Query`、macOS plist 文件探测、Linux 单元文件/`crontab -l`，`scheduler.rs:79-127`）。
- `clients` → `collector::supported_clients()`（`main.rs:664-669`）→ `collector.rs:196-202`：`ClientId::iter()` 转字符串并排序，即**厂商扫描器支持的全部客户端**；测试断言 ≥20 个且含 codex/claude/opencode/gemini（`collector.rs:496-508`）。
- `invite`/`dashboard` → `config::join_code`（`config.rs:154-162`）/ `config::dashboard_url`（`config.rs:164-176`）；`uninstall` → `scheduler::uninstall` + 可选删远端分支 + 可选清本地目录（`main.rs:615-638`）。

### 4.2 采集触发链：`sync` → `collect`、`since` 来源与全量重扫条件

- 每次 `sync` 先做自更新检查（`main.rs:289-297`）：`Ok(Restarted)` 时**直接返回**，本轮不再采集（新进程由 `USAGEMESH_UPDATE_RESUME` 续跑）。
- 调度器迁移：`interval_seconds` 被强制为 `scheduler::SYNC_INTERVAL_SECONDS`（`main.rs:304-307`）；仅当 `scheduler::is_installed() && scheduler_revision < scheduler::revision()` 时重装并标记清理旧调度（`main.rs:308-335`）。
- 单实例锁：`wait_for_lock = !quiet || USAGEMESH_UPDATE_RESUME 存在`（`main.rs:340`），锁文件 `config_dir()/sync.lock`（`main.rs:244-246`），10 分钟等待上限（`main.rs:203`）、250ms 重试（`main.rs:204`）、15 分钟视为陈旧可抢占（`main.rs:221-229`）。
- `previous = config::read_cached_ledger()`（`main.rs:352` → `config.rs:203-217`）：读 `<config_dir>/ledger-cache.json`；**若缓存 `schema_version < CURRENT_LEDGER_SCHEMA_VERSION` 直接返回 `None`**（`config.rs:209-211`），因此 `initial_scan = previous.is_none()`（`main.rs:354`）会连带覆盖"旧 schema 升级"的情形。
- 全量重扫的六个触发条件（`main.rs:355-373`）：`pricing_migration`（缓存 `pricing.policy != pricing::PRICING_POLICY`，`main.rs:355-357`；策略常量 `pricing.rs:24`）、`schema_migration`（`ledger.schema_version != CURRENT_LEDGER_SCHEMA_VERSION`，`main.rs:358-360`）、`version_migration`（`ledger.device.app_version != env!("CARGO_PKG_VERSION")`，`main.rs:361-363`）、`stale_cache`（`generated_at` 解析失败，或换算本机时区后日期 **早于 `今天-2天`**，`main.rs:365-367`）、`initial_scan`、显式 `--full`；合并为 `effective_full`（`main.rs:368-373`）。
- 增量窗口：`overlap_start = Local::now().date_naive() - Duration::days(2)`（`main.rs:364`）；增量 `since = (Local::now().date_naive() - 2天).format("%Y-%m-%d")`（`main.rs:394-396`）。**`since` 由"本机今天的日期"推导，不读取上次同步时间戳**；上次同步时间只通过 `generated_at` → `stale_cache`（>2 天未成功扫描 → 全量）间接参与。
- 调用点：全量 `collector::collect(device_info(&config), None)`（`main.rs:389`）；增量 `collect(..., Some(since))` 后 `collector::merge_incremental(previous, partial, &since)`（`main.rs:397-401`）。全量模式字符串区分 6 种取值：5 个具名原因 `full/initial`｜`full/version-migration`｜`full/schema-migration`｜`full/pricing-migration`｜`full/offline-recovery`，外加 `full/manual` 兜底（`main.rs:376-388`）。
- `collect` 内部（`collector.rs:204-229`）：`parse_local_clients(LocalParseOptions{home_dir:None, use_env_roots:true, clients:None, since, until:None, year:None, scanner_settings})`（`collector.rs:211-221`）→ `evidence::scan(incremental)`（`collector.rs:223`）→ `PriceBook::load()`（`collector.rs:224`）→ Codex tier 增强 `codex_tier::collect(since.as_deref(), &scanner_settings)`（`collector.rs:227`，实现 `codex_tier.rs:333-386`，内部用 `since.is_some()` 决定 `recent_enough` 文件过滤 `codex_tier.rs:337,357,362`）。`recent_enough` 的增量阈值是 **5 天**（`5 * 24 * 3600` 秒，`codex_tier.rs:320-331`，过滤点 `codex_tier.rs:357`）：mtime 更旧的 jsonl 在增量模式下整份跳过，该阈值与 2 天 `since` 窗口相互独立。
- 扫描器设置来源：`<config_dir>/tokscale/settings.json` 的 `scanner` 字段，缺失即 `ScannerSettings::default()`（`collector.rs:78-93`）。

### 4.3 账本结构（`model.rs` 全文语义）与数值语义

- `CURRENT_LEDGER_SCHEMA_VERSION = 8`（`model.rs:3`）。信封版本与之独立：`EncryptedLedger.schema_version = 2`（`crypto.rs:58`）、`DashboardAccessEnvelope.schema_version = 1`（`crypto.rs:126`）。
- `Metrics`（`model.rs:16-26`）：`input/output/cache_read/cache_write/reasoning: i64`、`messages: i32`、`cost_usd: f64`；`serde(rename_all="camelCase")`。
- `Metrics::total_tokens()`（`model.rs:29-35`）= `input + output + cache_read + cache_write + reasoning` 的**饱和加法**，**包含** reasoning 与 cache 两个维度。`Metrics::add` 五桶 + `messages` 全部 `saturating_add`，**`cost_usd` 为浮点直接累加**（`model.rs:37-45`）：整数桶饱和、代价不舍入也不饱和。进入聚合前还有一层**负值钳制**：`token_metrics_from_message` 对五桶与 `messages` 一律 `.max(0)`（`collector.rs:101-111`）——上游负数被截为 0，既不饱和也不报错；`cost_usd` 在该函数固定置 0.0（`collector.rs:109`），随后由计价写入（canonical 路径 `collector.rs:118-120`）。
- `UsageRow`（`model.rs:48-76`）：`date`、`timestamp_ms`（"本机请求桶起点，毫秒"，注释 `model.rs:52-53`）、`client`、`provider`（解析器原始串，供审计）、`upstream_vendor`（归一厂商）、`route_provider`（归一路由/计费方）、`route_type`（`official|cloud|aggregator|relay|inference-provider|self-hosted|custom|unknown`，注释 `model.rs:63`）、`billing_channel`（`official-api|official-subscription|third-party|unknown`，默认 `unknown`，`model.rs:9-11,66`）、`model`、`tier: Option<String>`、`cost_lower_bound: bool`（默认 false 且不序列化，`model.rs:72-73`）、`#[serde(flatten)] metrics`。
- `DeviceInfo`（`model.rs:80-87`）：`id/name/platform/arch/hostname/app_version`；构造于 `main.rs:170-179`（`app_version` 取 `CARGO_PKG_VERSION`）。
- `PricingInfo`（`model.rs:91-100`）：`policy/source/source_url/compatibility`，由 `PriceBook::metadata()` 生成（`pricing.rs:281-289`）。
- `RequestDetail`（`model.rs:104-127`）：在 use 行维度上增加了 `reasoning_effort/agent/duration_ms`（均 `skip_serializing_if = "Option::is_none"`），同样 flatten `Metrics`。
- `Ledger`（`model.rs:131-145`）：`official_quota`（`Value`，默认 null 且不序列化）、`schema_version`、`generated_at`（RFC3339 UTC，`collector.rs:365`）、`device`、`rows`、`requests`、`totals`、`scan_ms`、`pricing`。
- `EncryptedLedger`（`model.rs:149-157`）与 `DashboardAccessEnvelope`（`model.rs:165-175`）为两个线上信封结构；后者注释明确"仅包裹工作区随机密钥，绝不含提示词或用量数据"（`model.rs:159-162`）。

### 4.4 `collect()` 的分组维度、分钟桶与时间归一

- 分组键 `RowKey`（`collector.rs:16-28`）共 10 字段，**全部参与行身份**：`date`、`timestamp_ms`、`client`、`provider`、`upstream_vendor`、`route_provider`、`route_type`、`billing_channel`、`model`、`tier`。`BTreeMap<RowKey, RowAccumulator>`（`collector.rs:230`）→ 输出时按 BTreeMap 的 `Ord` 顺序展开（`collector.rs:343-359`），即天然按上述字段字典序排序。
- `normalized_timestamp_ms`（`collector.rs:38-44`）：`|ts| < 100_000_000_000` 判定为**秒**并 `saturating_mul(1000)`，否则原样当毫秒。阈值 1e11 ms ≈ 1973 年，秒级时间戳（1e9~1e10）会命中。
- `minute_bucket`（`collector.rs:46-48`）：`timestamp_ms.div_euclid(60_000).saturating_mul(60_000)`，即**向下对齐到分钟起点**（负数按欧几里得除法，不会向零点取整）；`UsageRow.timestamp_ms` 用分钟桶（`collector.rs:266, 326`），`RequestDetail.timestamp_ms` 用原始归一化毫秒（`collector.rs:242`）；单测 `collector.rs:519-523`。
- `trim_request_details`（`collector.rs:50-67`，上限常量 `MAX_REQUEST_DETAILS = 1000` 在 `collector.rs:36`）：先按 `(timestamp_ms, client, model)` 升序排序；再按 `(timestamp_ms, client, model, tier, metrics)` **全等去重**（含整个 `Metrics` 相等）；若长度 >1000 则 `drain(0..len-1000)`，即**丢弃最旧、保留最新 1000 条**。
- 明细与行共用同一份归一化/桶化时间戳（`collector.rs:241-266`），保证行与明细的时间轴一致；`cost_lower_bound` 在行级为按位或累积（`collector.rs:135`）。
- Codex 特殊路径：canonical 日总量与 tier 增强日总量**必须完全相等**才允许用增强行替换（`collector.rs:182-194`，被 `collector.rs:234-236` 跳过、`281-285` 替换）；该门只比 token，不比记录数（注释 `collector.rs:177-181`）。
- 该替换链路的失败是静默的：`codex_tier::collect(...).unwrap_or_default()`（`collector.rs:226-227`）把 tier 增强的错误吞成空结果——无日志、无标记；此后 `accepted_codex_days` 恒为空（`collector.rs:228,182-194`），canonical Codex 行原样保留，表现为 Fast/Priority 归属静默消失。
- 计价档位在两条路径上来源不同：canonical 行固定以 `Some("standard")` 询价（`collector.rs:119`，理由见注释 `collector.rs:121-123`），而 Codex 增强行用实际 tier（`collector.rs:299` 的 `Some(&enhanced.tier)`），并落在行与明细的 `tier` 字段上（`collector.rs:315,334`）。

### 4.5 `merge_incremental` 边界语义与缺陷评估

实现（`collector.rs:375-401`）：

- `previous.rows.retain(|row| row.date.as_str() < since)`（`collector.rs:376`）：**字符串字典序**比较日期；保留窗口外的旧行。
- `previous.rows.extend(partial.rows)`（`collector.rs:377`）：窗口内行整体以 `partial` 为准（同一 (date, 全维度) 不会既留旧又加新，因为旧侧已按 `date < since` 剔除）。
- 明细切分：`cutoff = partial.requests 的最小 timestamp_ms`（`collector.rs:378`），`previous.requests.retain(|row| row.timestamp_ms < cutoff)`，随后拼接 `partial.requests`（`collector.rs:379-381`）。**窗口外判据是"毫秒截断点"，不是 `since` 日期**；且 `partial.requests` 为空时 `retain` 整段跳过，旧明细全保留（`collector.rs:378`）。
- 行排序（`collector.rs:382-389`）：`date → client → route_provider → model → tier`。
- 元数据整段被 `partial` 覆盖：`schema_version/generated_at/device/scan_ms/pricing`（`collector.rs:390-394`）——注意 `device.app_version` 从此变为当前版本。
- `totals` 全量重算：`Metrics::default()` 后逐行 `add`（`collector.rs:395-398`），不做增量加减，避免漂移。
- 最后再 `trim_request_details`（`collector.rs:399`）。

【未验证】潜在缺陷：

1. 日期比较依赖 `date` 为定长零填充 ISO（`YYYY-MM-DD`）。任何非该形态（含时区后缀、非零填充）都会让 `"2026-9-1" < "2026-08-30"` 之类的比较失真。已核对 tokscale-core v4.14.0（rev `1ec865d4`）：pinned 路径确定产出定长 `%Y-%m-%d`（`bucket_tz.rs:137-146`），未 pin 路径由各解析器生成（Amp 路径以机器本地断言佐证 `lib.rs:13910-13919`；「`date` 的构造来源」已在 §3.6.9 核实为一体走 `timestamp_to_date` → `chrono::Local`，故不再逐解析器核对该形态）；唯一非定长输出是 `format_day_key` 的空串兜底（`bucket_tz.rs:144`），上游已用 `!key.is_empty()` 挡住（`lib.rs:5302`），但本仓对空 `date` 无防护——`"" < since` 恒真，这类行永不被 `retain` 裁掉（`collector.rs:376`）。
2. 两个时间口径在**默认（未 pin `scanner.bucketTimezone`）时同源**：canonical 行的 `date` 由各解析器按机器本地日期生成（tokscale `lib.rs:5290-5293` 未 pin 时不重写、`lib.rs:13910-13919` 对 Amp 路径佐证，其余解析器未逐一核对，见本条 1），与基于**本机 Local 时区**的 `since` / `cutoff`（`main.rs:364,394`）一致；只有显式 pin 了与机器时区不同的 `bucketTimezone` 时，`date` 才由 `day_key`（pinned zone）决定（`codex_tier.rs:290,338` 用 `BucketTimezone::from_scanner_settings`；tokscale `lib.rs:5301-5304`），此时 `since` 仍是机器 Local，跨日界才有错位空间。2 天重叠窗口能吸收大部分偏差，但边界日存在**重复计数或漏计**的理论空间（`since` 的过滤边界已在 §3.6.10 核实为**字符串日期比较**、且在解析完成后才过滤——`lib.rs:5403-5405`、`lib.rs:5275`，全程无时间戳参与；与本仓侧的「`since` 由本机 Local 今天推导」合观，错位只在 `date` 生成时区 ≠ 机器 Local 时成立，即上句 pinned `bucketTimezone` 的情形，见 §3.6.9 第 3 点）。
3. 明细上限 1000 与增量合并叠加：每次同步都会 `trim`（`collector.rs:399`），因此 `requests` 语义是"**最近 1000 条明细**"，不是历史全量；而 `rows` 会随时间无限累积（无截断逻辑）。二者口径不对称。
4. 无官方配额字段处理：`merge_incremental` 不动 `official_quota`，随后被 `run_sync` 统一覆盖（`main.rs:404-407`），顺序正确但依赖调用方；单独复用该函数（如测试）会保留旧配额。
5. `previous.device` 被覆盖（`collector.rs:392`）：若某次增量里设备名/主机名变化，旧快照的设备元数据立即丢失，无法审计历史设备标识。

### 4.6 聚合口径总纲（七层并列）

Q4（设备侧 / 跨设备 / 时间桶 / 增量）的答案分散在本章 §4.4–§4.5、§3.6.9–§3.6.10（`date` 桶时区与 `since` 边界）与第 6 章 §6.3、§6.5、§6.9–§6.10（浏览器合并与周期桶）。下表把七处口径并列：**各层相互独立、都不复用下一层的中间结果**——设备端折叠出账本三件套（`rows`/`requests`/`totals`）→ 账本落盘并加密上传 → 浏览器对多设备账本做拼接与展示侧重算（两层分工的权威表述见 §6.9）。

| 层 | 聚合口径（代码事实） | 源码位置 |
| --- | --- | --- |
| 设备侧 · 聚合行 | 十维度 `RowKey`（`date`/`timestamp_ms`/`client`/`provider`/`upstream_vendor`/`route_provider`/`route_type`/`billing_channel`/`model`/`tier`）分桶进 `BTreeMap`；时间维度取**分钟桶**（`timestamp_ms.div_euclid(60_000).saturating_mul(60_000)`），同键累加；`totals` **不独立累计**，而是由输出行逐条 `add` 汇总 | `collector.rs:16-28`、`:46-48`、`:230`、`:262-278`、`:341-359` |
| 明细 · 请求 | `RequestDetail.timestamp_ms` 取**归一化原始毫秒**（不桶化）；先按 `(timestamp_ms, client, model)` 排序、再按含整个 `Metrics` 全等的键去重、最后 `drain` 掉最旧者——**恒为最近 `MAX_REQUEST_DETAILS = 1000` 条**，而 `rows` 无此上限 | `collector.rs:36,50-67,242,361,399` |
| 增量 · 合并 | `merge_incremental`：旧行只保留 `date < since`（字符串比较），窗口内行整段由本次 `partial` 顶替；明细以本次最小 `timestamp_ms` 为 cutoff、保留更旧者后拼接；`totals` 从合并结果**全量重算**；`since` = 本机今天 − 2 天 | `collector.rs:375-401`、`main.rs:394-401` |
| 跨设备 · 浏览器 | 各设备账本 `flatMap` **直接拼接**：聚合行不排序不去重、请求行仅按 `timestampMs` 降序排序，**不做同键合并、不跨设备相加**；行唯一性靠 `deviceId` 前缀而非全局去重 | `web-ui/src/lib/data.ts:1018-1027` |
| 展示层 · 范围/筛选/分页/导出 | 时间范围谓词（`all`/`today`/`7d`/`30d`/`month`/自定义毫秒区间，末端 `+59_999 ms`）与维度**全等**筛选（`row[key] === value`）在浏览器逐行计算；表格再叠加列排序、全列子串搜索、客户端分页（默认 25 条/页）与 CSV 导出——均作用于"全部匹配行"，不改写账本 | `web-ui/src/lib/analytics.ts:126-160,170-176`、`components/UsageTable.tsx:155-156,178-220` |
| 官方周期 · 桶归属 | 只把**完整落在**周期区间内的分钟桶计入（`bucketStart >= bounds.from && bucketEnd <= bounds.to`）；与区间**相交**的桶单列 `boundaryTokens`/`boundaryBuckets` 并从主统计排除；日粒度旧行另计 `legacyTokens`/`legacyBuckets`；周期内跨设备求和、`deviceCount` 取去重设备数 | `web-ui/src/lib/quotaCycles.ts:190-202,207-218`（互引 §4.4 与 §6.5） |
| 本地工作台 · 旁路 | 独立于上述六层：`since = Local::now() − 89 天`（近 90 个自然日），按源记录 `date` 落**自然日** `daily` 桶；会话按 `startedAt`/`lastAt` 取 min/max 归并、请求明细排序后裁到最近 20 000 条，产物 `note` 自带口径声明 | `rust-cli/src/local/activity.rs:9,36-93,97` |

一句话对照：**分钟桶只在设备端账本（`rows`）与浏览器周期统计两处成立**，明细与本地工作台分别退化为原始毫秒与自然日；`totals` 与周期统计都是"重算派"而非"增量加减派"。

## 5. 金额计算与渠道身份


UsageMesh 的金额是**设备端一次性算出**的「兼容价卡估算」，随后加密上传；设备端 `costUsd` 是唯一账本金额来源，浏览器不按单价重算（`web-ui/src/lib/data.ts:18-29`、`data.ts:25`），只做汇总、格式化与少量展示层推算（订阅周期外推与占比/均价除法，见 5.8）。计价链的三个入口都在 Rust 侧：

- 云端账本主链：`collector::collect()`（`rust-cli/src/collector.rs:204-373`），逐条消息计价 → 聚合到 `UsageRow` / `RequestDetail`。
- 本地工作台链：`local::activity::scan()`（`rust-cli/src/local/activity.rs:8-99`），独立第二次扫描并独立计价，产物是本机 JSON，不进加密账本。
- 策略迁移闸门：`main.rs:355-373`，`pricing.policy` 字符串一变就强制全量重扫重价。

路由/渠道身份（`provider.rs`）与金额**完全解耦**：`classify()` 只产出 `upstream_vendor` / `route_provider` / `route_type` / `billing_channel` 四个字符串维度，不参与任何价格计算；它只作为 `RowKey` 的分组维度（`collector.rs:16-28`、`264-276`）。

---


### 5.1 `PriceBook::load()`：数据来源、回退链、缓存与过期、policy 标识

**【已核实】** 价格目录**不是**编译期内置表，唯一通用来源是远程 `https://models.dev/api.json`（`pricing.rs:22`），GPT-5.6 家族由硬编码守卫卡片覆盖（`pricing.rs:361-397`）。

缓存文件路径 = `dirs::config_dir()/usagemesh/models-dev-pricing.json`（`pricing.rs:93-95`）；写入前 `create_dir_all` 父目录，写失败被 `let _ =` 静默吞掉（`pricing.rs:215-225`）。Windows 上 `dirs::config_dir()` 预期为 `%APPDATA%\usagemesh\models-dev-pricing.json`（【未验证】，依赖 `dirs` crate 的平台语义）。

过期规则：文件 mtime 距今 ≤ 24h 视为新鲜（`pricing.rs:26`、`97-103`）。回退链四级，逐级记入 `catalog_state`（`pricing.rs:242-279`）：

| 顺序 | 条件 | `catalog_state` |
| --- | --- | --- |
| 1 | 新鲜缓存且解析成功 | `models.dev cache`（`pricing.rs:249`） |
| 2 | 在线拉取成功（12s 超时、要求 2xx）并回写缓存 | `models.dev live`（`pricing.rs:256-263`、`227-239`） |
| 3 | 存在过期/陈旧缓存且解析成功 | `models.dev stale cache`（`pricing.rs:266-273`） |
| 4 | 全部失败 | 空目录 + `guarded fallbacks only`（`pricing.rs:275-278`） |

**离线后果【已核实（代码路径）】**：第 4 级下 `catalog` 为空，非 GPT-5.6 模型在 `lookup()` 全部落空 → `quote_on_date` 返回 `cost_usd: 0.0, lower_bound: true`（`pricing.rs:315-320`、测试 `503-511`）。即费用按 0 记账但被标记为下界，不是静默的。

`PricingInfo` 四字段（`model.rs:89-100`）由 `metadata()` 生成（`pricing.rs:281-289`）：`policy` = 常量 `PRICING_POLICY`；`source` = 固定英文前缀 `GPT-5.6 Sol undiscounted relay compatibility card + models.dev fallback · ` 拼 `catalog_state`；`source_url` = OpenAI Sol 文档 URL（`pricing.rs:23`）；`compatibility` = 固定说明串。

**policy 参与全量重扫迁移触发【已核实】**：`main.rs:355-357` 比较 `ledger.pricing.policy != pricing::PRICING_POLICY`；`pricing_migration` 进入 `effective_full`（`main.rs:368-373`），full 分支调用 `collector::collect(device, None)`（`main.rs:389`，`None` = 无 `since` 全量），并在日志输出「Pricing migration: rebuilt full local history with the current policy」（`main.rs:465-467`）。当前 policy 串为 `gpt56-sol-undiscounted-relay-compat-v5`（`pricing.rs:24`）。

附带效应【已核实】：整个 `PricingInfo` 结构参与 `same_accounting`（`collector.rs:461-469`，逐字段比较在 `collector.rs:467`；`PricingInfo` 派生全字段 `PartialEq`，`model.rs:89`），因此仅 `catalog_state` 变化（例如下次启动从 `cache` 变 `live`、`source` 串随之不同）就会让账本判定为「有变化」，触发重新加密上传（`main.rs:408-414`、`448-455`），但**不会**触发全量重扫。测试 `591-609` 只构造了 `schema_version`/`device.id`/`policy`/`official_quota` 差异，`source`/`catalog_state` 差异由该 `PartialEq` 推导。

### 5.2 `quote_on_date(model, date, tier, metrics)` 完整语义

**【已核实】** 签名与返回：`pricing.rs:308-358`；返回 `PriceQuote { cost_usd, lower_bound }`（`pricing.rs:87-91`）。`quote()` 是 `date = "9999-12-31"` 的包装（`pricing.rs:304-306`）。

匹配链（严格按序，`pricing.rs:291-302`）：

1. `normalize_model_id`：取**最后一个** `/` 之后 → 再取 `:` 之前 → `@` 替换为 `-` → ASCII 小写 → 若以 `[1m]` 结尾则截掉再去空白（`pricing.rs:105-114`）。注意 `@`→`-` 是**替换**而非删除。
2. `lookup_aliases`：基础名 + 去掉 `-ioa` 后缀的变体 + 对上述每个值再剥掉 6/8 位纯数字后缀（只校验位数与 `is_ascii_digit`，不校验是否为真实日期，`pricing.rs:116-120`）；别名列表构造后执行 `sort()+dedup()`（`pricing.rs:133-134`），由于剥除变体恒为完整名的前缀，字典序使其**先于精确名命中**（`deepseek-v4-flash` 排在 `deepseek-v4-flash-ioa` 之前），同族双条目并存时命中的是剥除版（`pricing.rs:116-136`，测试 `475-500` 只覆盖精确名缺失时的回退，与 `docs/PRICING.md:56` 的「Exact first」不一致）。
3. 逐个别名先查 `guarded_pricing`，再查 models.dev 目录（`pricing.rs:293-300`）——**守卫卡片优先于目录**，即使目录里有更便宜的同名条目。
4. 无命中 → `cost 0 / lower_bound true`。

**不存在前缀回退**：没有任何 `starts_with` 式模糊匹配，别名只有 `-ioa` 与日期后缀两种受控形态（`pricing.rs:122-136`）。

models.dev 目录的构建（`pricing.rs:171-209`）：按 `provider_id`、`model_id` 双排序遍历，对同一归一化模型名，按 `provider_preference`（`pricing.rs:138-169`）选「规范厂商」条目，同偏好时取 `provider_id` 字典序较小者（`pricing.rs:191-200`，测试 `514-522`）。`input<=0 && output<=0` 的条目被丢弃（`pricing.rs:66-68`）。目录条目的长上下文阈值恒为 `None`、乘数恒为 1.0（`pricing.rs:74-76`）。

**tier 参数不参与定价【已核实】**：`lookup()` 收下 tier 并透传给守卫卡片（`pricing.rs:291`、`pricing.rs:294`），而 `guarded_pricing` 的形参是未使用的 `_tier: Option<&str>`（`pricing.rs:369`）——tier 不进入任何乘数或分支。`collector.rs:119` 对 canonical 行硬编码 `Some("standard")`，`collector.rs:299` 才传 Codex 增强行的真实 tier；代码内注释（`collector.rs:121-123`）与测试（`pricing.rs:445-457`）都明确「tier 是元数据，不是 USD 乘数」。

**日期生效区间**只对 Terra/Luna 存在：分界常量 `GPT56_TERRA_LUNA_REPRICE_EFFECTIVE = "2026-07-30"`（`pricing.rs:25`），比较方式是把 date 截前 10 字符后做**字符串比较**（`pricing.rs:375-381`）；`date.len() < 10` 时视为 `9999-12-31`（命中新价）。Sol 卡片显式与日期无关（测试 `429-442`）。

**四类桶单价来源与倍数关系**（`pricing.rs:361-397`）：

| 模型 | input($/1M) | cache_read($/1M) | cache_write($/1M) | output($/1M) | 备注 |
| --- | ---: | ---: | ---: | ---: | --- |
| `gpt-5.6-sol`（含 `gpt-5.6`/`-low`/`-medium`/`-high`/`-xhigh`/`-minimal`/`-max` 别名，`pricing.rs:371-374`） | 5.0 | 0.5 | 1.25 × input = 6.25（`pricing.rs:387`） | 30.0 | `pricing.rs:377` |
| `gpt-5.6-terra`（date ≥ 2026-07-30） | 2.0 | 0.2 | 1.25 × input | 12.0 | `pricing.rs:378` |
| `gpt-5.6-terra`（更早） | 2.5 | 0.25 | 1.25 × input | 15.0 | `pricing.rs:379` |
| `gpt-5.6-luna`（date ≥ 2026-07-30） | 0.2 | 0.02 | 1.25 × input | 1.2 | `pricing.rs:380` |
| `gpt-5.6-luna`（更早） | 1.0 | 0.1 | 1.25 × input | 6.0 | `pricing.rs:381` |

models.dev 条目则直接取 `cost.input / cost.output / cost.cache_read / cost.cache_write`，缺字段按 `0.0`（`pricing.rs:63-78`，尤其 `72-73` 的 `unwrap_or(0.0)`）。

**长上下文规则：阈值是 272K，不存在 200k 规则【已核实】**（`pricing.rs:27-29`：`GPT56_LONG_CONTEXT_THRESHOLD = 272_000`；任务描述里的「>200k」在代码中无对应）。触发条件与作用范围（`pricing.rs:322-339`、`341-348`）：

- `total_input = max(input,0) + max(cache_read,0) + max(cache_write,0)`，**严格大于** 272_000 才触发（`pricing.rs:327-329`）。
- 触发后 `input`、`cache_read`、`cache_write` 三个输入侧桶统一乘 `long_input_multiplier = 2.0`；`output` 桶（连同 `reasoning`）乘 `long_output_multiplier = 1.5`（`pricing.rs:330-348`，测试 `460-472`）。
- 阈值/乘数挂在 `EffectivePricing` 上（`pricing.rs:51-60`），守卫卡片对 Sol/Terra/Luna 三者都设置（`pricing.rs:393-395`）；models.dev 条目没有（`pricing.rs:74-76`）——所以长上下文只对 GPT-5.6 家族生效。

**单位与舍入**：所有单价在加载时除以 `1_000_000`，即内部为 **USD/token**（`pricing.rs:70-73`、`384-386`）；成本累加过程**没有任何舍入或小数截断**，只有一次 `cost.max(0.0)` 保底（`pricing.rs:355`）。金额格式化在浏览器：概览 2 位小数（`analytics.ts:89-99`，`money()` 默认 `digits = 2`），请求级表格 4 位（`UsageTable.tsx:224`）。

### 5.3 `lower_bound` 的成立条件、传播路径与「未知」是否被静默吞掉

**【已核实】** 五类触发条件：

1. 模型在守卫卡片与目录中都查不到 → 直接返回 `cost 0 / lower_bound true`（`pricing.rs:315-320`，测试 `503-511`）。
2. 某桶 token > 0 而该桶单价 ≤ 0，四个条件任一成立即 true：`input`、`output+reasoning`、`cache_read`、`cache_write`（`pricing.rs:350-353`）。
3. Codex 增强行：`!cache_write_known && metrics.input > 0`（`collector.rs:301-305`）。
4. 行级/分钟桶聚合时按**逻辑或**合并：`entry.cost_lower_bound |= cost_lower_bound`（`collector.rs:127-136`）。
5. 前端聚合层继续向上或：明细行 → 组 → 概览，设备行（`data.ts:924`、`data.ts:948`）与本月汇总（`app.tsx:265`、`app.tsx:340`）同样参与，周期对比还做前后期合并（`PeriodComparison.tsx:28`）并在下界时禁用比率与驱动分析（`PeriodComparison.tsx:89`）（`analytics.ts:214`、`quotaCycles.ts:155`、`quotaCycles.ts:210`、`Analytics.tsx:85`）。

传播链【已核实】：`quote.lower_bound` → `priced_metrics_from_message` 返回 `(metrics, lower_bound)`（`collector.rs:113-125`）→ `RequestDetail.cost_lower_bound`（`collector.rs:259`、序列化字段 `costLowerBound`，`model.rs:123-124`）与 `UsageRow.cost_lower_bound`（`collector.rs:356`，`model.rs:71-73`，`false` 时省略序列化）→ 加密账本 → 浏览器 `data.ts:795` / `data.ts:849` → 展示层加 `≥` 前缀（`Overview.tsx:36`、`Overview.tsx:292`、`Devices.tsx:167`、`Analytics.tsx:103`、`Analytics.tsx:114`、`Analytics.tsx:296`、`PeriodComparison.tsx:63`、`PeriodComparison.tsx:69`、`UsageTable.tsx:224`、`UsageTable.tsx:447-448`）。

**「未知模型」不静默**：金额记 0，但下界位置位，且前端有专门文案——数据质量为「N / M 个桶为下界或未定价」（`DataQuality.tsx:47-49`）、分析页「费用下界，部分价格尚未解析」（`UsageTable.tsx:447-448`）、周期对比直接拒绝计算比率「费用为下界，暂不计算变化」（`insights.ts:83-93`）、概览保留 `≥`（`Overview.tsx:36`）；配额页不显示 `≥`，只有布尔 `partialPricing`（`QuotaCycles.tsx:397-399`）与文字告警「部分记录未完成计价，金额可能偏低」（`QuotaCycles.tsx:491-497`）。`pricingResolved` 在前端是 `!costLowerBound` 的派生值（`data.ts:26`）。

**「未知桶」不存在**：计价只认四个数值桶 + `reasoning`，`reasoning` 被并入 output 桶计价（`pricing.rs:341-348`）；没有丢弃或忽略额外桶的代码路径。唯一语义模糊处是 models.dev 的 `cache_read`/`cache_write` **缺失**与**显式 0** 都归约为 `0.0`（`pricing.rs:72-73`），二者不可区分；但只要该桶有 token，就落进 `pricing.rs:350-353` 判为下界，所以仍不是静默。

### 5.4 `provider.rs::classify(raw_provider, model, explicit, hint)` 完整规则

**【已核实】** 产出 `ProviderIdentity`（`provider.rs:1-7`），四字段取值集合：

- `upstream_vendor`：由**模型串**字符串包含判断，取值 `anthropic` / `openai` / `google` / `xai` / `deepseek` / `minimax` / `mistral` / `meta` / `qwen` / `moonshotai` / `zai` / `xiaomi` / `cohere` / `amazon` / `unknown`（`provider.rs:20-76`，命中顺序即优先级，`unknown` 兜底 `75`）。注意它**不读 provider 字段**，只看 model。
- `route_provider`：`official`（hint 证明一方）、`azure-openai`、`aws-bedrock`、`google-vertex`、`openrouter`、`local`、`newapi`、`oneapi`、`litellm`、`cliproxyapi`、`custom-relay`、`custom-gateway`、`custom-proxy`、`together-ai`、`fireworks-ai`、`groq`、`siliconflow`、`cloudflare-workers-ai`、`replicate`、`cerebras`、`sambanova`、`zenmux`、`nano-gpt`、`novita`，或**原样保留的 raw provider**，或 `unknown`。
- `route_type`：`official` / `cloud` / `aggregator` / `relay` / `inference-provider` / `self-hosted` / `custom` / `unknown`（枚举在 `model.rs:63` 注释与 `provider.rs:280-372` 各处赋值）。
- `billing_channel`：`official-api` / `official-subscription` / `third-party` / `unknown`（`model.rs:65` 注释与 `provider.rs:163-167`、`170-224`、`282-372`）。

决策顺序（`provider.rs:229-373`）：

1. 先算 `upstream_vendor`（`provider.rs:235`）。
2. **有 hint 时 hint 全权决定路由**：`hint.route_type == "official"` → 四字段归并为 `official / official / <hint 的 channel>`（`provider.rs:236-239`、`95-108`）；否则照抄 hint 三元组（`provider.rs:240-245`）。
3. 无 hint：raw provider 经 `norm()`（trim、小写、空格与下划线→连字符，`provider.rs:16-18`）后为空或 `unknown` → 三元组全 `unknown`（`provider.rs:247-257`）。
4. 云别名含 `aws` / `bedrock` / `amazon-bedrock` → `aws-bedrock`；`azure` / `azure-ai` / `azure-openai` → `azure-openai`；`vertex` / `vertex-ai` / `google-vertex` / `gcp-vertex` → `google-vertex`，三者皆 `cloud` + `third-party`（`provider.rs:259-285`，匹配式 `raw == alias || raw.contains(alias)`）。
5. 含 `openrouter` → `openrouter` / `aggregator` / `third-party`（`provider.rs:286-293`）。
6. 中转关键字：`newapi`、`new-api` → `newapi`；`oneapi`、`one-api` → `oneapi`；`litellm`；`cli-proxy`、`cliproxy` → `cliproxyapi`；`relay` → `custom-relay`；`gateway` → `custom-gateway`；`proxy` → `custom-proxy`，全部 `relay` + `third-party`（`provider.rs:294-314`）。
7. 推理服务关键字：`together` / `fireworks` / `groq` / `siliconflow` / `cloudflare` / `replicate` / `cerebras` / `sambanova` / `zenmux` / `nano-gpt` / `novita` → `inference-provider` + `third-party`（`provider.rs:315-336`）。
8. `ollama` / `llama.cpp` / `localhost` 为子串匹配、`local` 为**精确匹配**（`raw == "local"`）→ `route_provider = raw`、`self-hosted`、billing `unknown`（`provider.rs:337-348`）；`local-proxy` 之类会先被中转关键字 `proxy` 截获为 `custom-proxy`（`provider.rs:294-314`）。
9. `explicit == true` 且 raw 属于官方名表（`official_provider_name`，`provider.rs:78-93`）→ 保留 `route_provider = raw`，但 `route_type`/`billing_channel` 都是 `unknown`（`provider.rs:349-359`，测试 `394-400`）。**这是一处刻意设计：provider 名字本身永远不能证明一方路由**（注释 `provider.rs:350-351`、`provider.rs:226-228`）。
10. `explicit == true` 且非官方名 → `custom` + `third-party`（`provider.rs:360-365`，测试 `402-407`）。
11. `explicit == false` → 保留 raw，`unknown` / `unknown`（`provider.rs:367-372`，测试 `387-392`）。

**`explicit` 的判定来源【已核实】**（`collector.rs:138-162`）：

- 会话证据里存在 `explicit_provider` → true（`collector.rs:148-151`，来源于 `evidence::SessionEvidence.explicit_provider`）。
- 否则 `parser_provider_is_explicit(client, raw_provider)`（`collector.rs:95-99`）：要求 raw 非空、不等于 `unknown`，且 client ∈ {`codex`, `opencode`, `micode`}。该比较不经 `norm()`，大小写/空白差异只影响 `explicit` 中间标志（`"Unknown"` 会得到 `explicit = true`），`classify` 内部归一后仍落回 `unknown` 三元组（`provider.rs:247-257`）。测试明确 `kimi`、`gemini` 的 parser provider 不算 explicit，`opencode` 的 `unknown` 也不算（`collector.rs:510-517`）。
- Codex 增强行另有独立口径：`raw_provider != "unknown"` 即视为 explicit，不分 client（`collector.rs:291-296`）。
- `route_hint_from_base_url` 内部调用 `classify(id, "unknown", true, None)`（`provider.rs:206`），即 URL 推断路径固定按 explicit 处理。

`route_hint_from_base_url` 的 host 判定（`provider.rs:110-224`）。official 分支经 `official_vendor_for_host`（`provider.rs:127-150`）做边界感知匹配（`host_is`，`provider.rs:123-125`），测试 `431-434` 用 `api.openai.com.evil.example` 反证；但 `host_is` **并非只用于 official**——`provider.rs:170`（azure）为 `host.contains("openai.azure.com") || host_is(&host, "azure.com")`、`provider.rs:177`（bedrock）为 `host.contains("bedrock") || host_is(&host, "amazonaws.com")`、`provider.rs:191`（openrouter，`aggregator`，非 official）为 `host_is(&host, "openrouter.ai")`；其中 azure 与 bedrock 的第一判据、以及 `provider.rs:184`（vertex）的 `host.contains("vertex")` 仍是**子串匹配**（`provider.rs:184` 的另一判据 `aiplatform.googleapis.com` 为精确 `==`），因此 `bedrock.evil.example`、`vertex.evil.example`、`openai.azure.com.evil.example` 会被判为云路由（已知误判，与 `docs/PRICING.md:46` 的 lookalike 声明冲突）：

| host 形态 | route_provider / route_type / billing_channel |
| --- | --- |
| `openai.com`、`chatgpt.com`、`anthropic.com`、`generativelanguage.googleapis.com`、`deepseek.com`、`x.ai`、`mistral.ai`、`moonshot.ai`、`moonshot.cn`（`provider.rs:127-150`） | `official` / `official` / `official-api`，其中 `chatgpt.com` → `official-subscription`（`provider.rs:159-168`） |
| 含 `openai.azure.com` 或 `azure.com` | `azure-openai` / `cloud` / `third-party`（`provider.rs:170-176`） |
| 含 `bedrock` 或 `amazonaws.com` | `aws-bedrock` / `cloud` / `third-party`（`provider.rs:177-183`） |
| `aiplatform.googleapis.com` 或含 `vertex` | `google-vertex` / `cloud` / `third-party`（`provider.rs:184-190`） |
| `openrouter.ai` | `openrouter` / `aggregator` / `third-party`（`provider.rs:191-197`） |
| `localhost` / `127.0.0.1` / `0.0.0.0` / `::1`（`::1` 分支不可达，见段落注） | `local` / `self-hosted` / `unknown`（`provider.rs:198-204`） |
| 其它 | 先试 `classify(id,"unknown",true,None)`，若得到 cloud/relay 类则沿用（`provider.rs:206-213`）；否则 `custom-relay`（id 为空/`unknown`/官方名时）或 id 本身，`relay` + `third-party`（`provider.rs:214-223`，测试 `436-443`） |

URL 解析失败时会补 `https://` 前缀重试（`provider.rs:115-117`）；取 `host_str()` 后先剥尾点再转小写（`provider.rs:118-121`），匹配只用 host——端口与路径不参与，因此 `https://api.openai.com:8443/v1` 仍判 `official`，`https://api.openai.com./v1` 的尾点被剥后同样判 `official`。**`::1` 分支不可达【已核实（静态，依据本机 `url` 2.5.8 源码副本）】**：`host_str()` 对 IPv6 返回带方括号的形式（`url` 2.5.8 `src/host.rs:58-63` 对 `Host::Ipv6` 明写「`Url::host_str` returns the serialization of that address between `[` and `]` brackets」，同义文档注见 `url` 2.5.8 `src/lib.rs:965`），故 `http://[::1]:8080` 取到 `"[::1]"`，永不命中 `provider.rs:198` 的字面量 `"::1"`（需写成 `"[::1]"`）；这类本地 IPv6 端点会落入 `provider.rs:206-223` 兜底，被记为第三方中转。raw URL 不入账本，只保留归一化标签（注释 `provider.rs:156-158`、`evidence.rs:638-639`）。

### 5.5 `evidence.rs`：路由证据来源、结构、匹配键、优先级、effort 提取

**【已核实】** 扫描来源共五类：

1. Codex 配置：`$CODEX_HOME`（缺省 `~/.codex`）下的 `config.toml`（`[model_providers.*]` 的 `base_url`/`baseUrl`）与 `auth.json` 的 `auth_mode`/`authMode`（`evidence.rs:85-147`）。
2. Codex 会话：`~/.codex/sessions`、`~/.codex/archived_sessions` 的 `*.jsonl`，只解析含 `session_meta` 的行（`evidence.rs:174-216`、`782-791`）。注意此处根目录**硬编码** `home/.codex/...`（`evidence.rs:782-791`），同一模块的 config/auth 读取才走 `$CODEX_HOME`（`evidence.rs:85-97`）；而 `codex_tier.rs` 用 `$CODEX_HOME`（回退 `~/.codex`，`codex_tier.rs:312-318`）——设置 `CODEX_HOME` 后两模块可能扫到不同目录，日级对账门的输入也随之不同。
3. Claude 会话：`$CLAUDE_CONFIG_DIR` 或 `~/.claude` 下的 `projects/**/*.jsonl`（`evidence.rs:218-267`、`793-801`）。
4. CodeBuddy 全局设置：`~/.codebuddy/settings.json` 然后 `~/.codebuddy/settings.local.json`（后者优先），再被环境变量 `CODEBUDDY_BASE_URL` 覆盖（`evidence.rs:619-648`，注释 `620`、`638-639`）。
5. 通用兜底：遍历 `ClientId::iter()` 的 `data().resolve_path()`（`evidence.rs:734-772`），扩展名限定 `json/jsonl/ndjson/log/toml`（`evidence.rs:723-732`），单文件上限 32 MiB（`evidence.rs:43`、`665-667`），单 client 上限 2500 文件（`evidence.rs:44`、`757-759`），目录深度 10（`evidence.rs:752`），增量模式只取近 3 天修改的文件（`evidence.rs:53-64`）。JSON 整读、TOML 转 JSON、其它按行：行内出现 `reasoning`/`thinking` 或 `base_url`/`baseurl`/`api_url`/`apiurl`/`endpoint` **任一**关键字即尝试解析，并按命中项分别注册 effort 与 route 证据（`evidence.rs:698-720`，判定逻辑 `evidence.rs:702-711`）；超过 2 MiB 的行在解析前被整行跳过，该行的路由/effort 证据随之丢失（`MAX_EVIDENCE_LINE_BYTES`，`evidence.rs:45`、`evidence.rs:699-701`）。

`EvidenceBundle` 结构（`evidence.rs:30-41`）：

- `sessions: HashMap<(client, session_id), SessionEvidence>` — 键是**二元组**（`evidence.rs:32`），`SessionEvidence { explicit_provider, route_hint }`（`evidence.rs:18-22`）。
- `provider_hints: HashMap<String /*provider 小写*/, RouteHint>`（`evidence.rs:33`）。
- `client_route_hints: HashMap<String /*client*/, RouteHint>`（`evidence.rs:34-36`）。
- `reasoning_efforts: HashMap<(client, session_id), Vec<EffortPoint>>`（`evidence.rs:37-40`）。

匹配键与关联方式【已核实】：会话证据按 `(client, session_id)` **精确匹配**（`evidence.rs:807-815`），调用点是 `collector.rs:144`。Codex 侧 session_id 取 `session_meta.payload.id`，缺失时退回文件 stem（`evidence.rs:178-181`、`195-197`）；Claude 侧**一次注册多个候选 id**（文件 stem + 文件内所有 `sessionId`/`session_id`，`evidence.rs:227-241`、`258-266`），用来提高命中率。Claude 的 `explicit_provider` 只在文件内 provider 取值**唯一**时给出（`evidence.rs:253-257`），否则留空。

优先级【已核实】（`collector.rs:144-160`，三层 `or_else`）：

1. 会话级 `route_hint`（来自该 client+session 日志中出现的 `base_url`/`apiBase`/`endpoint` 等，`evidence.rs:546-590`）；
2. `provider_hints[raw_provider 小写]`（Codex `config.toml` 的 provider 表 / CodeBuddy 无 session id 时的退化写入，`evidence.rs:108-147`、`571-582`）；
3. `client_route_hints[client]`（CodeBuddy 全局设置/环境变量，`evidence.rs:619-648`）。

`explicit_provider` 的取值优先级同样在 `collector.rs:145-147`：会话证据优先，否则用 parser 的 `message.provider_id`。

ChatGPT 一方订阅的证据链（`evidence.rs:126-169`）：`auth_mode=chatgpt` + `requires_openai_auth=true` + `wire_api=responses` 且该 provider 条目无 `base_url` → 直接判 `official-subscription`；此外对内置 id `openai` / `openai-codex` / `openai-http`，只要 `config.toml` 未显式配置同 id 条目，就补 `official-subscription`（`evidence.rs:154-169`，测试 `902-933`、`936-960`）。显式 `base_url` 永远优先且不被覆盖（注释 `evidence.rs:149-153`，测试 `962-989`）。

`reasoning_effort_for_message(bundle, client, session_id, timestamp)`（`evidence.rs:821-863`）【已核实】：

- 先按 `(client, session_id)` 取点集，取不到直接 `None`（`evidence.rs:827-832`）。
- 三级选择：① 时间戳**不晚于**事件且在 12h 内（`EFFORT_LOOKBACK_MS`，`evidence.rs:46`）的最近一点（`835-846`）；② 否则取时间差 ≤ 5min（`EFFORT_NEARBY_MS`，`evidence.rs:47`）的最近一点（`848-859`）；③ 否则若整个 session 只有唯一标签则直接采用，**不受时间约束**（`861-862`）。
- 字段形态识别（`evidence.rs:325-367`）：键名归一化后匹配 `reasoningEffort` / `reasoningLevel` / `reasoningMode` / `reasoningBudget` / `thinkingEffort` / `thinkingLevel` / `thinkingMode` / `thinkingBudget` / `thinkingBudgetTokens`，以及 `reasoning`/`thinking` 容器下的 `effort`/`level`/`mode`/`budget`/`budgetTokens`/`thinkingBudget` 子键；递归深度上限 10（`evidence.rs:369-385`）。
- 值归一化（`evidence.rs:276-323`）：`minimal|min|lowest`→`minimal`，`low|lite`→`low`，`medium|med|balanced|normal`→`medium`，`high`，`xhigh|extra-high|very-high|max|maximum`→`xhigh`，`auto|adaptive|dynamic`→`auto`，`off|none|disabled|false`→`off`，`on|enabled|true`→`enabled`；数字键名含 `budget` → `budget:N`（≤0 → `off`），其它数字 → `level:N`（`evidence.rs:305-323`）；其余 ≤32 字符且仅 `[a-z0-9-.:]` 的值原样保留。
- 时间戳来源：值为字符串/数字的 `timestamp|timestampMs|createdAt|startTime|eventTime`，递归深度 6（`evidence.rs:444-475`）；session id 候选键 `sessionId|conversationId|threadId|chatId|turnId`，深度 8（`evidence.rs:387-415`）；文件兜底 id 用 stem 与父目录名，排除 `wire`/`events`/`updates`/`ui_messages`（`evidence.rs:477-494`）。
- 注册时对每个 candidate id 冗余写入同一 `EffortPoint`，点集去重且最终按时间排序（`evidence.rs:506-518`、`769-771`）。

### 5.6 `codex_tier.rs`：增强解析、tier 归属、日级对账门、cache_write_known、时间与 effort 来源

**【已核实】** 增强解析只读 Codex 本机 JSONL：根目录为 `$CODEX_HOME` 或 `~/.codex` 下的 `sessions` 与 `archived_sessions`（`codex_tier.rs:312-318`、`343-347`），扩展名必须 `.jsonl`（`codex_tier.rs:356`）；增量模式仅取 mtime 在 **5 天**内的文件（`codex_tier.rs:320-331`、`357-358`），比 evidence 的 3 天更宽，mtime 读取失败时按「足够新」处理、不跳过（`codex_tier.rs:324-326`）；文件内记录再按 `date < since` 逐条跳过并按去重键去重（`codex_tier.rs:362-363`），故 mtime 窗口与 `since` 日期过滤是两道独立闸门。模块自述「改编自 MIT 许可的 `falyx6851-byte/codex-monitor`」「计价集中在 `pricing.rs`，必须发生在日聚合之前，才能保证逐请求长上下文阈值正确」（`codex_tier.rs:1-8`）。

tier 归属【已核实】：从每行 `payload` 的 10 个候选 pointer 提取 `service_tier`/`serviceTier`（`codex_tier.rs:155-174`），命中即更新「当前 tier」（`codex_tier.rs:256-258`），后续行沿用直到再次出现；归一化 `priority|fast → "fast"`、`default|standard|"" → "standard"`、其它原样保留（`codex_tier.rs:147-153`，测试 `392-403`）。初始值为 `standard`（`codex_tier.rs:244`）。行内模型来自 `turn_context.model`（`codex_tier.rs:272-276`），provider 来自 `session_meta.model_provider`（`codex_tier.rs:268-270`）。

**与两份 README 的「不猜测」声明的对照【已核实】**：`docs/PRICING.md:31` 对 tier 声明「UsageMesh never guesses Fast from model name or performance」，`README.md:167`／`README.zh-CN.md:159` 对 effort 同样声明「does not infer effort from model names or token counts」／「不会根据模型名或 Token 数猜测」。实现与声明**不矛盾**——tier/effort 的值确实只来自来源记录的显式字段（上段的 10 个 pointer 与 §5.5 的递归识别）——但两者都是**状态式继承**而非逐条显式：命中即更新当前值、后续行沿用直到再次出现（`codex_tier.rs:256-258` 为 tier，`codex_tier.rs:259-261` 为 effort）；`evidence.rs` 侧三级选择的第 ③ 级更把「会话内唯一标签」在**无时间约束**下套用到该 session 的全部消息（`evidence.rs:861-862`）。后果是**标签出现之前的行也会带上该标签**：复刻若按「逐条显式」实现，effort/tier 的覆盖集会比 UsageMesh 小。

**日级对账门 `reconciled_codex_days`【已核实】**（`collector.rs:182-194`）：对每个日期，只有当增强侧当日累计 tokens **与 canonical（tokscale）完全相等**时才接受该日（`candidate.tokens == upstream.tokens`，`collector.rs:191`）；被接受的日里，canonical 行**整体跳过**（`collector.rs:233-236`），增强行取而代之（`collector.rs:281-339`）。理由写在代码注释里（`collector.rs:177-181`）：「要求 message 计数也相等」正是 **v1.0 的 bug**，会静默丢弃合法的 Fast/Priority 证据，因为两个成熟解析器暴露的记录粒度不同；因此 message 数不参与门控（测试 `525-545` 验证 3 vs 7 条记录仍通过，`547-567` 验证 tokens 999 ≠ 1000 时整天被拒）。拒绝日的后果只是 tier 信息丢失（`tier` 保持 `None`，前端显示 `Standard`，`collector.rs:250`、`data.ts:701-709`），金额仍由 canonical 行计价。**接受日的副作用不止 tier 替换【已核实】**：

- **请求数口径随之切换**：canonical 的 `messages` 取 `message.message_count`（`collector.rs:108`，按日累加 `collector.rs:172`），增强行恒为「1 条记录 1 次请求」（`codex_tier.rs:46`），而前端「请求数」直接取 `row.messages`（`data.ts:792`）；同一自然日在门控前后的请求数与「平均每请求费用」不可直接比较（门控只比 token：`collector.rs:191`；测试 `525-545` 用 canonical 3 条 vs 增强 7 条记录证明记录数不参与判定）。
- **增强行的路由证据面比 canonical 行窄**：canonical 走三层 `or_else`（会话 `route_hint` → `provider_hints[raw]` → `client_route_hints[client]`，`collector.rs:152-159`），增强行只查 `provider_hints[raw]`（`collector.rs:288-296`），`explicit` 也只看 `raw_provider != "unknown"`（`collector.rs:294`）；接受日之后，只存在于会话日志或全局设置里的 base_url 证据（`evidence.rs:546-590`、`evidence.rs:619-648`）不再参与该日渠道判定，`route_provider`/`route_type`/`billing_channel` 可能退回 raw/`unknown`（`provider.rs:367-372`）。

**`cache_write_known` 与「cache 写按 1.25x 计为下界」【已核实】**：`cache_write_known = request.usage.cache_write.is_some()`（`codex_tier.rs:379`），即源记录里是否出现 `cache_write_input_tokens` / `cache_write_tokens` / `input_tokens_details.cache_write_tokens` 任一（`codex_tier.rs:123-128`）。归一化时若该值缺失，则 `cache_write = 0` 且「未缓存 input」保留原值（`codex_tier.rs:36-41`，测试 `425-438`）。`collector.rs:301-305` 的注释解释了判为下界的原因：Codex 未单独记录 cache 写时，归一化后的 fresh input 里**可能已经含有 cache 创建量**，而 GPT-5.6 官方文档按未缓存输入的 1.25× 计 cache 写，因此这条记录的成本只能视为下界（`cost_lower_bound = quote.lower_bound || missing_cache_write_evidence`）。

归一化口径（`codex_tier.rs:35-49`）：`cached` 截断到 `input`；`uncached = input - cached`；`cache_write` 截断到 `uncached`；`reasoning` 先截断到 `output`（`codex_tier.rs:39`）；最终 `input = uncached - cache_write`（`codex_tier.rs:41`）、`output = output - reasoning`（饱和减法，`codex_tier.rs:44`；reasoning 单列，避免与计价侧 `pricing.rs:341-344` 重复）。去重键 = `session_id | last_token_usage | total_token_usage` 六元组串（`codex_tier.rs:207-213`），session id 未知时追加文件路径 sha256 前 12 位（`codex_tier.rs:215-219`、`303-308`）。

**`timestamp_ms` 来源【已核实】**：取整行的 `timestamp` 字段（RFC3339）解析为毫秒（`codex_tier.rs:194-198`、`278`）；当日归属用 `BucketTimezone::day_key(ms)`（`codex_tier.rs:290`），时区来自 `ScannerSettings`（`codex_tier.rs:338`），而 `ScannerSettings` 由 `collector.rs:78-93` 从 `config_dir()/tokscale/settings.json` 的 `scanner` 段读取。无效时间戳的记录被丢弃（`codex_tier.rs:278-280`）；`total<=0` 或 `input`/`cached`/`output` 三桶全零的 `token_count` 记录同样跳过（`codex_tier.rs:283-286`）。

**`reasoning_effort` 来源【已核实】**：`extract_reasoning_effort` 的 7 个候选 `effort` / `reasoning_effort` / `reasoningEffort` / `/reasoning/effort` / `/reasoning/config/effort` / `/thread_settings/reasoning_effort` / `/thread_settings/reasoningEffort`（`codex_tier.rs:176-192`），命中即更新当前 effort（`codex_tier.rs:259-261`），行内沿用。注意这与 `evidence.rs` 的递归扫描是**两套独立提取器**，前者随增强行写入 `RequestDetail`（`collector.rs:316`），后者用于 canonical 行（`collector.rs:251-256`）。

### 5.7 `docs/PRICING.md` 与代码实现逐条对照

结论：**核心声明与代码一致，无直接矛盾项；但有 4 处实现细节文档未覆盖（缓存回退、文件限额/扫描窗口、Codex 日级对账、请求明细上限），且 1 处「1h cache 写」表述在代码中无对应实体。** 详见后文《文档-代码一致性对照》表。

### 5.8 「一条用量消息 → 最终展示金额」完整链与重复/漏价风险

计算链【已核实】：

1. `parse_local_clients(...)` 产出 `ParsedMessage`（`collector.rs:211-221`）；UsageMesh **丢弃 parser 侧任何成本**：`token_metrics_from_message` 显式把 `cost_usd` 置 `0.0`（`collector.rs:101-111`，尤其 `109`）。
2. 模型标识过 `canonical_model_id`（`collector.rs:238`，core 函数）。
3. `priced_metrics_from_message` 调 `price_book.quote_on_date(model, message.date, Some("standard"), metrics)`，把结果写回 `metrics.cost_usd`（`collector.rs:113-125`）。
4. 行级聚合 `add_grouped`：同 `RowKey`（date + 分钟桶 + client + provider + upstream_vendor + route_provider + route_type + billing_channel + model + tier）累加 token 与金额（`collector.rs:127-136`、`262-278`）。时间戳先经 `normalized_timestamp_ms` 归一到毫秒（绝对值 < 100_000_000_000 视为秒并乘 1000，`collector.rs:38-44`），再 `minute_bucket` 向下取整到分钟（`collector.rs:46-48`、`collector.rs:266`）；请求明细同样存放归一后的毫秒时间（`collector.rs:242`），`evidence.rs:436-442` 有一份同名实现。
5. `Ledger.totals` 由各 `UsageRow.metrics` 再累加（`collector.rs:341-359`）。
6. 加密上传（`main.rs:448-455`），浏览器解密后 `toRecord` / `toRequestRecord` 直接取 `row.costUsd`（`data.ts:793-794`、`data.ts:848`），`applyDynamicPricing` 只是赋值 `record.cost = record.storedCost` 并派生出 `pricingResolved`（`data.ts:15-39`）。
7. 展示层只做求和与格式化（`analytics.ts:201-216`、`analytics.ts:89-99`）。

**无重复计价【已核实】**：浏览器端没有任何单价、乘数或重价逻辑，`costUsd` 是唯一账本金额来源；派生展示除法有平均单价（`Analytics.tsx:103`、`Analytics.tsx:114`）与预算占比（`Overview.tsx:296`、`Settings.tsx:168`、`Settings.tsx:177`）。**唯一的金额推算是订阅配额页的周期比例外推【已核实】**：`quotaCycles.ts:271` 用 `recorded / usedPercent × 100` 把设备端已记录金额按官方配额百分比线性外推为「本周期总金额估算」（展示 `QuotaCycles.tsx:456`、计算式 `QuotaCycles.tsx:461-462`），该值不是账本字段、由浏览器计算，`recorded` 只累计计价完整（已结束）的分钟（`quotaCycles.ts:246`、`quotaCycles.ts:264`），并要求账本同步不落后于配额快照（`cycleCostSyncPending`，`quotaCycles.ts:257`）；外推条件不满足时整块降级为文案提示（`QuotaCycles.tsx:464-466`、条件块 `quotaCycles.ts:248-259`）。`data.ts:35` 的 `fastMultiplier: 1.0` 是残留字段，全仓检索无任何消费点。

风险点（按严重度）：

1. **第二条计价链存在**：`local/activity.rs:27`、`:63` 独立 `PriceBook::load()` + `quote_on_date(&m.model_id, &m.date, Some("standard"), ...)`，90 天窗口、最多保留最近 20000 条（`activity.rs:9`、`91-93`、`97`）。它与 collector 的差别是传**原始** `model_id`（未过 `canonical_model_id`），同一模型的金额理论上可能与本机工作台显示不一致；两者产物流向不同（工作台 JSON vs 加密账本），不会叠加。
2. **请求明细被截断**：`MAX_REQUEST_DETAILS = 1000`（`collector.rs:36`），`trim_request_details` 先按 (时间, client, model, tier, metrics) 去重，再只保留**最后** 1000 条（`collector.rs:50-67`、`361`、`399`）。后果是「请求明细金额合计」会小于设备总计金额——展示层若按明细求和会低估；行级 `rows` 与 `totals` 不受影响。去重键里的 `metrics` 含 `cost_usd`（`collector.rs:57-63`），因此同分钟、同 client/model/tier 且四个 token 桶与金额完全相同的记录会被合并，而仅金额不同者不会——去重粒度随金额变化。
3. **增量合并按日期字符串切分**：增量扫描固定回看 2 天（`main.rs:394-401`），合并时按 `row.date < since` 保留旧行、`requests` 按最小时间戳切（`collector.rs:375-401`）。若 core 的 `since` 过滤语义返回了早于 `since` 的行，同一自然日会出现新旧两批行 → 重复计费。**【已核实（见 §3.6.10）：字符串日期比较、解析后过滤、不省 IO】**——core 侧 `filter_parsed_messages` 用 `m.date.as_str() >= since.as_str()` 做字典序裁剪（`lib.rs:5403-5405`；`until` 为 `<=`，`lib.rs:5407-5409`），过滤发生在 `rebucket_parsed_days`（`lib.rs:5273`）与**全部文件解析完毕之后**（`lib.rs:5275`）；通道 A 的整段实现 `lib.rs:4334-5282` 不引用 `message_cache`（§3.7.1），因此 `since` 只裁剪结果、**不减少磁盘 IO**。由此本条的触发面收窄为**时区口径不一致**：本机 `since` 由 `Local::now()` 生成（`main.rs:394-396`），而 core 的 `date` 可被扫描器钉定时区后重分桶并按同一钉定时区过滤（`lib.rs:5273`、`lib.rs:5404`）——扫描器钉了非本机时区时，`merge_incremental`（`collector.rs:375-401`）的 `row.date < since` 裁剪才会出现边界日误差（§3.9.2、§10.1）。
4. **离线/无缓存的首次运行**：所有非 GPT-5.6 模型金额为 0，仅靠 `lower_bound` 标记（`pricing.rs:275-278`、`315-320`）。前端虽有「下界」提示，但总额会显示为 `$0.00 ≥` 这类形态。
5. **`output` 与 `reasoning` 是否重叠【未验证】**：canonical 路径的成本公式把 `output + reasoning` 一起按 output 单价计（`pricing.rs:341-348`）。若 tokscale-core 的 `ParsedMessage.output` 本身已包含 reasoning token，则会重复计费；Codex 增强路径已显式剥离（`codex_tier.rs:39-45`）。此项需 core 侧语义确认。
6. **Codex 日级对账拒绝时的静默降级**：整天退回 canonical 行 → Fast/Priority 标签丢失（`collector.rs:182-194`、`233-236`）；金额不受影响，但 tier 维度的筛选/分组会显示 `Standard`（`data.ts:701-709`）。
7. **models.dev 缺失字段被归零**：`cache_read`/`cache_write` 缺失与显式 0 无法区分（`pricing.rs:72-73`），会在有 token 时触发下界标记，不产生静默错价，但会让大量模型常驻「下界」状态。

---

### 5.9 计价公式与匹配链

```
normalize(model) → aliases(model) → [guarded_pricing(alias, date) | catalog[alias]] → EffectivePricing

total_input = max(input,0) ⊕ max(cache_read,0) ⊕ max(cache_write,0)  // ⊕ = saturating_add
long        = threshold.is_some() && total_input > threshold      // 仅 GPT-5.6 家族，阈值 272_000
in_mult     = long ? long_input_multiplier : 1.0                  // 2.0
out_mult    = long ? long_output_multiplier : 1.0                 // 1.5
output_tok  = max(output,0) ⊕ max(reasoning,0)                    // 饱和加法

cost = max(input,0)       × P_input     × in_mult                 // 每个桶先 max(0)
     + max(cache_read,0)  × P_cache_read× in_mult
     + max(cache_write,0) × P_cache_write× in_mult
     + output_tok         × P_output    × out_mult
cost_usd = max(cost, 0.0)                                        // 无舍入

lower_bound = 未命中模型
           || (input>0        && P_input<=0)
           || (output_tok>0   && P_output<=0)
           || (cache_read>0   && P_cache_read<=0)
           || (cache_write>0  && P_cache_write<=0)
           || (Codex 增强行 && !cache_write_known && input>0)
```

来源：`pricing.rs:291-302`（匹配链）、`pricing.rs:322-357`（公式与下界；`322-326` 输入侧饱和加法、`341-348` 逐桶 `max(0)` 与 `output+reasoning` 合并、`350-353` 下界条件）、`pricing.rs:361-397`（守卫卡片）、`codex_tier.rs:35-49`（Codex 桶归一）、`collector.rs:301-305`（Codex 额外下界条件）。

---

### 5.10 路由身份规则表

| 输入证据 | upstream_vendor | route_provider | route_type | billing_channel | 证据 |
| --- | --- | --- | --- | --- | --- |
| 本地 base_url 命中一方域（非 chatgpt.com） | 由模型推断 | `official` | `official` | `official-api` | `provider.rs:159-168` |
| 本地 base_url = chatgpt.com | 由模型推断 | `official` | `official` | `official-subscription` | `provider.rs:163-167` |
| ChatGPT 认证 + `requires_openai_auth` + `wire_api=responses`、无 base_url | 由模型推断 | `official` | `official` | `official-subscription` | `evidence.rs:126-146` |
| ChatGPT 认证且内置 id `openai`/`openai-codex`/`openai-http` 未被显式配置 | 由模型推断 | `official` | `official` | `official-subscription` | `evidence.rs:154-169` |
| 显式第三方 base_url（任何形态） | 由模型推断 | 由 host 决定（relay/cloud/aggregator…） | 同左 | `third-party`（local 例外为 `unknown`） | `provider.rs:170-224` |
| raw provider 含 aws/bedrock | 由模型推断 | `aws-bedrock` | `cloud` | `third-party` | `provider.rs:259-285` |
| raw provider 含 azure | 由模型推断 | `azure-openai` | `cloud` | `third-party` | 同上 |
| raw provider 含 vertex | 由模型推断 | `google-vertex` | `cloud` | `third-party` | 同上 |
| raw provider 含 openrouter | 由模型推断 | `openrouter` | `aggregator` | `third-party` | `provider.rs:286-293` |
| raw provider 含 newapi/oneapi/litellm/cliproxy/relay/gateway/proxy | 由模型推断 | `newapi` / `oneapi` / `litellm` / `cliproxyapi` / `custom-relay` / `custom-gateway` / `custom-proxy` | `relay` | `third-party` | `provider.rs:294-314` |
| raw provider 含 together/fireworks/groq/… | 由模型推断 | `together-ai` 等 | `inference-provider` | `third-party` | `provider.rs:315-336` |
| raw provider 含 ollama/llama.cpp/localhost，或精确等于 local | 由模型推断 | 原样保留 | `self-hosted` | `unknown` | `provider.rs:337-348` |
| explicit 且为官方 provider 名（openai、anthropic、…） | 由模型推断 | 原样保留 | `unknown` | `unknown` | `provider.rs:349-359` |
| explicit 且为自定义名 | 由模型推断 | 原样保留 | `custom` | `third-party` | `provider.rs:360-365` |
| 非 explicit（parser 猜测的 provider） | 由模型推断 | 原样保留 | `unknown` | `unknown` | `provider.rs:367-372` |
| raw provider 为空或 `unknown` | 由模型推断 | `unknown` | `unknown` | `unknown` | `provider.rs:247-257` |

`upstream_vendor` 关键字表（`provider.rs:20-76`，按序匹配，全部为子串判断）：`claude|anthropic|sonnet|opus|haiku`→anthropic；`gpt|openai|o1*/o3*/o4*|codex`→openai；`gemini|google`→google；`grok`→xai；`deepseek`→deepseek；`minimax`→minimax；`mistral|mixtral`→mistral；`llama` 或 `meta-` 前缀→meta；`qwen`→qwen；`kimi` 或 `k2`/`k3` 前缀→moonshotai；`glm`→zai；`mimo`→xiaomi；`command-r|cohere`→cohere；`nova-` 或 `amazon.` 前缀→amazon；否则 `unknown`。

`official_provider_name` 归一映射（`provider.rs:78-93`）：`openai|openai-codex`→openai；`anthropic`；`google|gemini|google-ai`→google；`xai|x-ai`→xai；`deepseek|deepseek-ai`→deepseek；`moonshot|moonshotai|kimi|kimi-for-coding`→moonshotai；`zai|z-ai|zhipu|bigmodel`→zai；`minimax|minimax-ai`→minimax；`mistral|mistralai`→mistral；`cohere`；`qwen|dashscope|alibaba|aliyun`→qwen。

---

### 5.11 文档-代码一致性对照

| # | `docs/PRICING.md` 声明 | 代码事实 | 判定 |
| --- | --- | --- | --- |
| 1 | 优先级：守卫卡片 > models.dev > 下界标记（`PRICING.md:9-11`） | `lookup()` 逐别名先 `guarded_pricing` 再目录（`pricing.rs:293-300`）；未命中置 `lower_bound`（`pricing.rs:315-320`） | 一致 |
| 2 | 浏览器不重价（`PRICING.md:13`） | `applyDynamicPricing` 仅 `cost = storedCost`（`data.ts:18-29`），无单价/乘数 | 一致 |
| 3 | Sol 卡 $5/$0.50/$6.25/$30，长上下文 $10/$1/$12.50/$45（`PRICING.md:17-23`） | `pricing.rs:377` + `387` + `393-395`（1.25× 与 2×/1.5× 组合即得该表） | 一致 |
| 4 | 「Cache write (1h)」单列一行同价（`PRICING.md:22`） | 代码只有**一个** `cache_write` 单价（`pricing.rs:56`、`387`），无 1h/5m 桶区分 | 表述冗余（不矛盾） |
| 5 | Sol 卡不随 OpenAI 促销价漂移、与请求日期无关（`PRICING.md:15`、`25`） | Sol 无日期分支（`pricing.rs:377`）；测试 `429-442` | 一致 |
| 6 | Terra/Luna 保留日期感知官方价（`PRICING.md:25`） | `pricing.rs:378-381`，分界 2026-07-30 | 一致 |
| 7 | >272K 输入按 2× 输入侧 + 1.5× 输出侧作用于**整笔请求**（`PRICING.md:25`、`33`） | 阈值判断用三桶之和，乘数作用于 input 侧三桶与 output（含 reasoning），不分段（`pricing.rs:322-348`，测试 `460-472`） | 一致 |
| 8 | Tier（Standard/Fast/Priority）不改变金额（`PRICING.md:31`） | canonical 行硬编码 `"standard"`，`tier` 参数未参与定价（`collector.rs:119`、`pricing.rs:369`、测试 `445-457`） | 一致 |
| 9 | 路由优先级：request/session endpoint > provider base_url > ChatGPT 认证传输 > raw provider label（`PRICING.md:37-43`） | `or_else` 链三层（`collector.rs:152-159`）；ChatGPT 认证是 `provider_hints` 的补全项而非独立第四层（`evidence.rs:154-169`） | 基本一致（层级表述比代码更细） |
| 10 | ChatGPT 认证 + 无自定义 URL 即 `official-subscription`，即使 raw id 是 `custom`/`openai-http`（`PRICING.md:44`） | `evidence.rs:154-169` + 测试 `936-960`；有 base_url 的条目在 `evidence.rs:115-124` 提前返回，故「无自定义 URL」条件成立 | 一致 |
| 11 | 显式第三方 Base URL 永远胜出（`PRICING.md:44`） | `evidence.rs:115-124` 先行 + 注释 `149-153`，测试 `962-989` | 一致 |
| 12 | 原始 base URL 绝不写入账本/配对码/索引/仓库（`PRICING.md:46`） | 只保留归一化 hint（`evidence.rs:638-639` 注释、`provider.rs:156-158` 注释）；结构体无 URL 字段（`provider.rs:9-14`） | 一致（静态可核） |
| 13 | host 匹配是边界感知的，`api.openai.com.evil.example` 不算官方（`PRICING.md:46`） | official 分支 `host_is`（`provider.rs:123-125`）+ 测试 `provider.rs:431-434`；azure/bedrock/vertex 三个 cloud 分支的**第一判据**为子串匹配（`provider.rs:170`、`177`、`184`；其第二判据 `azure.com`/`amazonaws.com` 与 openrouter `:191` 仍用 `host_is`） | 部分一致（cloud 分支存在 lookalike 误判） |
| 14 | 无一方证据时保留 unverified/unknown（`PRICING.md:48`） | `provider.rs:367-372`、测试 `387-400` | 一致 |
| 15 | 政策变更 → 首次同步全量重扫重价（`PRICING.md:50-52`） | `main.rs:355-357`、`368-373`、`389`、`465-467` | 一致 |
| 16 | 别名回退：精确优先，`deepseek-v4-flash-ioa` 可退化到 `deepseek-v4-flash`，未知模型是下界而非猜测（`PRICING.md:54-56`） | `lookup_aliases`（`pricing.rs:122-136`）+ 测试 `475-500`、`503-511`；别名 `sort()+dedup()`（`pricing.rs:133-134`）使剥除变体先命中 | 基本一致（「精确优先」在同族双条目并存时不成立） |
| 17 | （文档未写）价格缓存与离线回退 | 24h TTL + 四级回退 + 空目录（`pricing.rs:26`、`242-279`） | 文档缺口（实现细节缺失） |
| 18 | （文档未写）文件限额 / 扫描窗口 | evidence 32MiB、2500 文件、3 天；codex_tier 5 天（`evidence.rs:43-47`、`codex_tier.rs:320-331`） | 文档缺口 |
| 19 | （文档未写）Codex 日级对账与 v1.0 bug | `collector.rs:177-194`（含注释） | 文档缺口（重要机制未公开） |
| 20 | （文档未写）请求明细上限 1000 与去重 | `collector.rs:36`、`50-67` | 文档缺口 |

---

### 5.12 未验证项

1. `dirs::config_dir()` 的跨平台落点（`pricing.rs:93-95`）；Windows 预期 `%APPDATA%\usagemesh\models-dev-pricing.json`，本轮未执行代码验证。
2. `models.dev/api.json` 的实际结构与字段名（如 `cost.cache_read`、`cost.cache_write` 是否普遍存在，`provider_id` 命名是否与 `provider_preference` 的期望一致），未联网抓取；`pricing.rs:31-49` 只是代码侧假设。
3. `https://developers.openai.com/api/docs/models/gpt-5.6-sol` 的真实内容未核对；文档与代码都把它当作「结构性规则」的依据。
4. tokscale-core v4.14.0（`Cargo.lock:1879-1881`）的 `ParsedMessage` 语义：`output` 是否已含 `reasoning`；`date` 桶日使用的时区是否与本模块 `ScannerSettings`（`collector.rs:78-93`、`codex_tier.rs:338`）一致；`since` 过滤是否包含边界（影响增量合并的重复计费风险）。**这三项已在 §3.6.9、§3.6.10 与 §3.9 核实**：`date` 由构造期 `chrono::Local` 推出、`bucket_timezone` 未钉时不重分桶、`since/until` 为字符串日期比较且在解析后过滤。
5. `canonical_model_id`（`collector.rs:238`、`:286`）的归一规则**已在 §3.2.7 与 §3.9.1 回源核实**：它是 `normalize_syntactic` 的直通（`lib.rs:87-89`），只做**语法归一**——小写 → 剥 `(reasoning-tier)` 后缀（tier 白名单 `lib.rs:57-73`）→ 剥尾部 `-YYYYMMDD` → `claude` 名内「数字`.`数字」改写为 `-` → `anthropic/claude-<major>-<minor>-<family>` 折叠为 `claude-<family>-<major>-<minor>`（`lib.rs:129-184`）；**不做别名折叠**（`modelAliases` 折叠只出现在本地展示用的 `normalize_model_for_grouping`（`lib.rs:100-102`），通道 A 不调用它）。UsageMesh 侧因此只有 `collector.rs:238`/`:286` 两处调用与下游 `pricing.rs:105-114` 这一层，无其他归一。
6. `CURRENT_LEDGER_SCHEMA_VERSION = 8`（`model.rs:3`）与浏览器端解析的字段兼容性未做运行时验证。
7. 前端展示结论（`≥` 前缀、下界文案）来自源码文本检索，未启动 `web-ui` 实际渲染确认。
8. 「请求明细合计小于总额」是由代码路径推导（`collector.rs:36`、`64-66`）得出，未用真实账本数据复现。
9. `MAX_EVIDENCE_LINE_BYTES` 与 `parse_json_candidate` 的截取行为（`evidence.rs:650-659`、`698-720`）在超长行场景下的实际影响未验证。

## 6. 云端仪表盘：解密、多设备合并与展示


云端仪表盘是**纯静态 React 应用**（`web-ui/`，Vite `base: "./"`——`web-ui/vite.config.ts:8`），无自建服务端、无数据库、无第三方账号：所有事实数据来自 GitHub 上四个分支族的**公开加密产物**，浏览器用工作区密码本地解密后在前端聚合渲染。

| 分支 | 文件 | 用途 | 证据 |
| --- | --- | --- | --- |
| `um-index` | `index.json` | 设备分支索引（主源） | `web-ui/src/lib/data.ts:45,564-568` |
| `um-dashboard` | `access.json` | 密码→工作区密钥的信封（KDF 参数 + 密文） | `web-ui/src/lib/data.ts:44,496-500` |
| `um-dashboard` | `device-index.json` | 旧/静态索引回退 | `web-ui/src/lib/data.ts:44,572-575` |
| `um-ledger-<deviceHash>` | `ledger.json` | 设备加密账本（聚合行 `rows` + 近期请求 `requests`） | `web-ui/src/lib/data.ts:616-620,623-624` |
| `um-presence-<deviceHash>` | `presence.json` | 独立心跳（不加密，与账本解耦） | `web-ui/src/lib/data.ts:46,621-625` |

关键结构判断（与设备侧 Rust 定义的分工）：账单**事实**（token 桶、`costUsd`）在设备端逐请求算好后加密上传；浏览器只做**解密 → 拼接 → 再聚合展示 → 换算订阅等价金额**，**从不重新定价**（`data.ts:15-39`）。设备在线状态由**心跳分支**而非账本时间戳判定（`data.ts:944,951`）。


### 6.1 解锁与解密

**密码 → 密钥（PBKDF2）【已核实】**

- 仓库定位 `repoFromLocation()`：`?repo=OWNER/REPO`（正则 `/^[a-z0-9-]+\/[a-z0-9._-]+$/i` 且禁含 `..`）→ GitHub Pages 路径推断 `owner.github.io/<repo>` → 兜底常量 `Atingaii/UsageMesh`（`data.ts:394-406`、`data.ts:43`）。
- 信封拉取：`workspaceKey()` 读 `um-dashboard/access.json`（`data.ts:492-500`）；信封结构 `AccessEnvelope`（`data.ts:49-59`）字段为 `schemaVersion | kind | kdf | iterations | salt | algorithm | nonce | ciphertext | updatedAt?`。
- **上传侧口径对照（文档写法 vs 实现）【已核实】**：`README.zh-CN.md:192`（本地工作台段）写「配置备份、密钥与本地管理事件不上传云端」——在原语境下该句指配置备份只存本机私有目录、不上传 GitHub（`docs/LOCAL-WORKSPACE.md:36`），但「密钥」二字易被读成「工作区密钥不上传」；实现上 `um-dashboard/access.json` 上传的正是**密码包裹的工作区密钥**（设备侧 AES-256-GCM 包裹 `crypto.rs:119`，推送 `github.rs:401-409`；浏览器侧读取即本行开头 `data.ts:492-500`），`join_code()` 更把密钥原值序列化进 pair code 后仅做 base64url 编码（`config.rs:154-162`；`README.zh-CN.md:169` 自陈「Pair Code：不含 GitHub PAT，但含 workspace key」）。准确口径是「**明文密钥不上传、只上传密码包裹形态**」。
- 强校验（`data.ts:501-516`）：`kind === "usagemesh-dashboard-access"`、`schemaVersion === 1`、`kdf === "PBKDF2-HMAC-SHA256"`、`algorithm === "AES-256-GCM"`；`iterations` 必须为整数且落在 **[100_000, 5_000_000]** 闭区间（越界抛 "Dashboard 访问配置的 KDF 参数无效"）。
- 派生（`data.ts:518-536`）：`importKey("raw", utf8(password), "PBKDF2", extractable=false, ["deriveKey"])` → `deriveKey({name:"PBKDF2", salt: b64url(envelope.salt), iterations: envelope.iterations, hash:"SHA-256"}, ..., {name:"AES-GCM", length:256}, extractable=false, ["decrypt"])`。
- 密钥长度：明文即工作区密钥本体（base64url 文本），解密后必须满足 `b64url(plaintext).length === 32`（`data.ts:550-551`）。

**AES-GCM 解密与 AAD 绑定【已核实】**

| 目标 | 算法/IV | AAD | 失败语义 |
| --- | --- | --- | --- |
| `access.json` | AES-256-GCM，`iv = b64url(nonce)` | `usagemesh-dashboard-access-v1:<repo 小写>`（`data.ts:47,543-546`） | `WorkspacePasswordError("Dashboard 密码不正确")`（`data.ts:425-430,553-555`） |
| `um-ledger-*/ledger.json` | AES-256-GCM，`iv = b64url(envelope.nonce)` | `usagemesh-ledger-v2:<deviceHash>`（`data.ts:48,654-658`） | `设备账本解密失败 (<branch>)`（`data.ts:663-665`） |
| 浏览器会话（工作区密钥二次包装） | AES-256-GCM-256 随机包装密钥，12 字节随机 IV | `usagemesh-browser-session-v2:<repo 小写>:<keyId>`（`session.ts:172-179,215-217`） | 清会话并返回 null（`session.ts:228-231`） |

账本信封校验还包含**分支名与设备哈希一致性**：`branch !== "um-ledger-" + envelope.deviceHash` 即抛 `设备账本格式不受支持`（`data.ts:627-633`；测试 `web-ui/tests/data.test.ts:102-109`）。

**v1 兼容路径【已核实】**：`access.json` 是**迭代驱动**的——注释明写旧 310k 清单继续有效，新/改密码用设备端更强的默认值（`data.ts:508-509`）；前端不硬编码次数，只做区间约束。测试对 `[310_000, 600_000]` 参数化验证成功路径与错误密码拒绝（`data.test.ts:44-58`），并断言 `99_999` 被拒且错误类型是 KDF 而非密码（`data.test.ts:59-67`）。

**密钥只驻内存 —— 逐介质核实【已核实】**

全库 grep `localStorage|sessionStorage|indexedDB|document.cookie` 命中如下（无 `document.cookie`，**不存在 cookie 写入**；下表只列产品代码的落点，`tests/` 另有夹具命中——`tests/session.test.ts:14,25,32,35,41,43,46,48`、`tests/savedViews.test.tsx:16,26`、`tests/setup.ts:10-11`、`tests/workspaceQA.test.tsx:96,128`，均为合成夹具、不含真实密钥）：

| 介质 | 键/库 | 内容 | 是否含明文密钥 |
| --- | --- | --- | --- |
| `localStorage` | `usagemesh:preferences:v1`（`app.tsx:95`、`preferences.ts:30`） | 主题/字号/刷新间隔/月度阈值 | 否 |
| `localStorage` | `usagemesh:theme`（旧键，只读 `preferences.ts:32-39`） | 旧主题值 | 否 |
| `localStorage` | `usagemesh:sidebar`（`app.tsx:75,98`） | `collapsed`/`expanded` | 否 |
| `localStorage` | `usagemesh:views:v2:<repo>`、旧 `usagemesh:view:<repo>`（`savedViews.ts:15,61,70`） | 命名筛选视图 | 否 |
| `sessionStorage` | `usagemesh:dashboard-session:v2`（`session.ts:3,99,136,190`） | `{version:2, repo, keyId, iv, ciphertext, createdAt, lastActivityAt}` | **仅密文**（`session.ts:9-17`） |
| `IndexedDB` | 库 `usagemesh-dashboard-session-v2` / 表 `session-keys`（`session.ts:4-5,43-62`） | 包装密钥 `CryptoKey`（`extractable=false`）+ `createdAt` | 否（不可导出对象） |

明文工作区密钥只存在于 `useDashboard` 的 `useRef`（`web-ui/src/hooks/useDashboard.ts:27`），`lock()` 同时清空密钥、缓存与派生状态（`useDashboard.ts:41-58`）。测试锚定 `sessionStorage` 明文不含密钥、恢复后可读、锁定后失效（`web-ui/tests/session.test.ts:12-18`）。

> 全 `web-ui/` 目录检索（含 `index.html`、`tests/`、`qa/`、`public/`）`cookie|document.cookie|If-None-Match|If-Modified-Since|etag` 仅命中 `web-ui/package-lock.json:2806,3725,3727`（jsdom 的传递依赖 `tough-cookie`），**产品代码零命中** —— 即：不使用 Cookie、不使用 HTTP 条件请求/ETag，全部读取恒为全量下载。

**会话生命周期【已核实】**：闲置 **30 分钟**（`session.ts:6`）、绝对 **12 小时**（`session.ts:7`）；`dashboardSessionExpired()` 还拒绝未来时间戳与不可解析时间（`session.ts:116-129`）。`useDashboard` 每 15 秒轮询过期并用内存 `sessionTimes` 二次校验（`useDashboard.ts:207-209,184-187`），可见性变化与交互事件都会刷新活动时间（`useDashboard.ts:188-206`）。**不存在 v1 会话格式兼容路径**——`readDashboardSession()` 只接受 `version === 2`（`session.ts:102-109`），旧版兼容仅体现在偏好键 `usagemesh:theme`（`preferences.ts:32-39`）。

**文档与实现矛盾：`README.md:177`【已核实】**

- **文档写法**（`README.md:177`，英文版）：「After unlock, the decrypted workspace key is kept only in the current page's memory — **not** in `localStorage`, `sessionStorage`, IndexedDB, cookies or the URL. Refreshing or closing the page requires the same password again.」——宣称工作区密钥只驻内存、三种存储介质全部未落，且刷新即需重输密码。
- **实测实现**：工作区密钥以 AES-GCM **密文信封**存入 `sessionStorage`（键 `usagemesh:dashboard-session:v2`，`session.ts:3`；读 `session.ts:99`、滑动续期写 `session.ts:136`、清除 `session.ts:145`、首次落盘 `session.ts:190`；信封结构与字段 `session.ts:9-17`），包装密钥以**不可导出**（`extractable = false`）的 `CryptoKey` 存入 IndexedDB（库名/表名 `session.ts:4-5`，`indexedDB.open` 于 `session.ts:30`，读写 `session.ts:43-62`、`:64-78`）；密钥材料由 `generateKey({name:"AES-GCM",length:256}, false, …)`（`session.ts:164-168`）生成，工作区密钥本体经 AES-GCM 加密后只以密文形式落盘（`session.ts:171-179`）。**刷新后可自动恢复**：`restoreDashboardSession()`（`session.ts:197-232`）解回密钥并续期，仅在闲置 30 分钟 / 绝对 12 小时（`session.ts:6-7`、`session.ts:116-129`）或 `forgetDashboardSession()`（`session.ts:142-156`）后失效。
- **结论**：该英文段落**与实现矛盾（安全模型写反）**，不能作为复刻依据——照它实现会得到「刷新必弹密码」，与 `UnlockScreen.tsx` 的 `verified` 态「不再索要密码」（`UnlockScreen.tsx:47-62`）直接冲突。**两版 README 亦自相矛盾**：`README.zh-CN.md:167` 的对应段落与实现一致（明写「workspace key 只以 AES-GCM 密文形式保存在当前标签页的 `sessionStorage`」「随机包装密钥以不可导出的 WebCrypto `CryptoKey` 存入 IndexedDB」「普通刷新可自动恢复；闲置 30 分钟、12 小时绝对期限、手动锁定或关闭标签页后需要重新输入原密码」）。**以中文版与 `session.ts` 为准。**

**`session.ts` 与 `UnlockScreen.tsx` 的职责划分【已核实】**

- `session.ts`（232 行，全部有副作用）：IndexedDB 包装密钥的 open/put/get/delete（`session.ts:28-95`）、`sessionStorage` 元数据读写（`session.ts:97-114`）、过期判定（`session.ts:116-129`）、`remember/restore/forget` 三个异步原语（`session.ts:142-231`）。它**不接触密码、不做 PBKDF2**。
- `UnlockScreen.tsx`（277 行，纯展示）：表单与密码可见性切换、错误/提示渲染、`verified` 态下按钮改为"重新读取数据"且**不再索要密码**（`UnlockScreen.tsx:47-62,214-237`）、页脚声明"密码仅在浏览器内用于解密，不会上传／闲置 30 分钟后自动锁定"（`UnlockScreen.tsx:242-249`）。它不调用 `workspaceKey`。
- 编排在 `useDashboard.unlock()`（`useDashboard.ts:125-180`）：先 `workspaceKey()` 验密码 → 写内存 key → 异步持久化会话（失败仅提示"当前浏览器无法保存加密会话"，不影响本次解锁，`useDashboard.ts:142-157`）→ 再加载数据集；只有"尚未持有密钥且错误类型为 `WorkspacePasswordError`"才标记密码无效（`useDashboard.ts:168-171`）。

### 6.2 数据获取（GitHub 分支读取）

**API 端点与回退顺序【已核实】**（常量见 `data.ts:41-46`）

1. 设备索引 `loadDeviceIndex()`（`data.ts:558-605`）按序尝试：① `https://raw.githubusercontent.com/<repo>/um-index/index.json`；② `<repo>/um-dashboard/device-index.json`；③ **仅当 URL 不带 `?repo=`** 时读站点相对 `device-index.json`（`new URL("device-index.json", document.baseURI)`）。每级都过滤 `/^um-ledger-[a-f0-9]+$/i` 并 `[...new Set(...)]` 去重（`data.ts:589-592`）。全失败时按是否有 transient 错分别抛"工作区索引读取失败…请检查网络后重试"或"暂无设备索引，请先在任意设备执行 usagemesh sync"（`data.ts:599-604`）。
2. 静态回退文件 `web-ui/public/device-index.json` 内容为 `{schemaVersion:1, kind:"usagemesh-device-index", branches:[], updatedAt:null}`；CI 部署时若 `um-index` 可达，会用实时内容覆盖 `dist/device-index.json`（`.github/workflows/pages.yml:90-99`）。
3. 账本/心跳：`decryptLedger()` 对每台设备同时发起 `um-ledger-<hash>/ledger.json` 与 `um-presence-<hash>/presence.json`（`data.ts:616-625`）。

**传输、超时与降级【已核实】**

- `json()` 统一使用 `cache:"no-store"`、`credentials:"omit"`、`AbortSignal.timeout(timeoutMs)`，默认超时 20 秒（`data.ts:432-443`）。**未使用 ETag / `If-None-Match` / 条件请求**（全库检索仅命中 `cache: "no-store"` 一处）。
- `githubJson()`：RAW 直读超时 **8 秒**；仅当失败被判定为 transient 才改走 GitHub Contents API（`Accept: application/vnd.github.raw+json`）超时 **12 秒**，注释说明"正常轮询不消耗 API 速率限制；RAW 媒体还支持 >1 MB 的加密账本"（`data.ts:467-490`）。
- transient 归类（`data.ts:444-462`）：HTTP **408 / 429 / 5xx**；`TypeError`（"网络读取失败"）、`AbortError` / `TimeoutError`（"数据读取超时"）。**JSON 解析失败与信封/校验失败不计入 transient**，因而不享受"沿用上次快照"的回退（注释 `data.ts:452-454`；测试 `data.test.ts:552-558`）。
- 端到端超时行为有测试固化：RAW 无响应时 8 秒切换、合计 20 秒结束、不无限重试（`data.test.ts:325`）。

**并发与限流【已核实】**：`loadDashboardWithKey()` 用 `Promise.allSettled` 对**索引中的全部设备一次性并发**（`data.ts:975-977`）；每台设备内部账本与心跳并行（`data.ts:616-625`）。**代码中没有并发上限、退避重试或速率限制器**——限流只体现在"正常轮询不走 API、失败才走 API"这一条降级策略上。

**心跳阈值与在线判定【已核实】**：`deviceSyncStatus()`（`data.ts:893-903`）以年龄分档：`≤ 3 分钟 → online`，`≤ 10 分钟 → syncing`，超过 → `offline`，时间戳不可解析直接 `offline`。设备行优先用 `presenceAt`，缺失时回退账本 `generatedAt`（`data.ts:944,951`）；老设备无 presence 分支时显式回退账本时间（`data.ts:677`）。presence 校验要求 `kind==="usagemesh-device-presence"`、`schemaVersion===1`、`deviceHash` 与账本信封一致（`data.ts:671-675`），且心跳刷新不改动已缓存的账本对象（`const ledger = { ...source }`，`data.ts:669`）。设备页文案与阈值一致（`web-ui/src/views/Devices.tsx:202`）。

**`/build-info.json` 的作用【已核实】**：由 Pages 流水线在构建产物中生成，字段 `workspaceRepository` / `dashboardSource` / `dashboardSourceSha` / `builtAt`（`.github/workflows/pages.yml:78-88`）；前端仅在**设置页**读取并展示"网页构建版本（源码 SHA 链接）"和"构建时间"（`web-ui/src/views/Settings.tsx:30-43,223-242`）。它**不参与任何统计或校验**，只用于部署溯源。

**缓存策略【已核实】**（`DashboardLoadCache` 定义 `data.ts:141-155`）

- 缓存由 hook 持有、**从不写入浏览器存储**（`docs/FRONTEND-PERFORMANCE.md:37`）；作用域 `scope = "<repo>:<key>"`，换 repo 或换密钥立即重建并丢弃快照（`data.ts:966-971`）。
- 账本级命中要求**信封 nonce 与 ciphertext 完全一致**（`data.ts:634-641`）；整批快照命中要求 `sources` 数组引用逐项全等（`data.ts:1006-1016`）。
- 索引每次刷新都重读（`data.ts:974`）；索引中已消失的分支被淘汰出缓存（`data.ts:995-998`）；缓存只在归一化成功后发布，避免"解密成功但格式非法"的载荷覆盖上一份可用快照（`data.ts:1090-1102`，测试 `data.test.ts:559-581,644-663`）。
- **在途读取与 cache 的分离保护**：加载开始时把整个 cache 浅拷贝成 `loadCache`（`data.ts:972-973`，注释 "Detach an in-flight load from any later repo/key switch on this cache."），本次加载只读写这份副本；发布前再校验 `cache.scope === scope && cache.ledgers === loadCache?.ledgers`（`data.ts:1090`），因此换 repo/换密钥后晚到的旧读取**写不进新作用域的 cache**（测试 `tests/data.test.ts:609-642`）。

### 6.3 `data.ts` 的合并与聚合

**多设备合并：简单拼接，不去重、不跨设备相加【已核实】**

```ts
records  = ledgers.flatMap(l => (l.rows     || []).map((row,i) => toRecord(l,row,i)));
requests = ledgers.flatMap(l => (l.requests || []).map((row,i) => toRequestRecord(l,row,i)))
                  .sort((a,b) => b.timestampMs - a.timestampMs);
```

（`data.ts:1018-1027`）。行唯一性靠 ID 前缀：聚合行 `"<deviceId>:<date>:<client>:<model>:<index>"`（`data.ts:771`）、请求行 `"<deviceId>:<timestampMs>:<client>:<model>:<index>"`（`data.ts:822`）。**分桶语义完全由设备端决定**，浏览器不做同键合并或按行去重。

**设备维度保留【已核实】**：`deviceLabelsFor()` 对同名设备追加 `deviceId` 去除非字母数字字符后的末 8 位（`data.ts:874-891`）；配额视图用同规则的独立实现（`quotaCycles.ts:159-169`）。设备行按 deviceId 累加 tokens/cost/requests，`sharePercentage` 以**全部设备 tokens 之和**为分母（分母为 0 时取 1 防除零，`data.ts:927-928,950`），排序为 tokens 降序、名称 zh-CN numeric 升序（`data.ts:954-958`）。

**时间范围过滤【已核实】**（`analytics.ts:126-160`）

- `all`：`() => true`，完全不解析时间戳（性能注释 `analytics.ts:168-169`）。
- `rowDate()`：`timestampMs > 0` 用**浏览器本地日期**，否则退化为账本 `date` 前 10 字符（`analytics.ts:106-113`）。
- `today` 同日；`7d`/`30d` = 今天 + 前 6/29 天（`analytics.ts:133-141`）；`month` = 同一 `YYYY-MM` 且 `≤ 今天`，故未来日期被排除（`analytics.ts:142-148`；测试锚定 `analytics.test.ts:32-43` 的「7天范围包含今天与前6天，排除未来数据」断言的是 `7d` 分支——`month` 分支**无独立断言**，仅由 `analytics.test.ts:132-175` 的「批量谓词 == 逐行谓词」自洽对比覆盖）。
- `custom`：`datetime-local` 解析成本地毫秒，**结束边界 = 所选最后一分钟 + 59_999 ms**（含结束分钟最后一毫秒，`analytics.ts:151`）；有 `timestampMs` 的行按毫秒比较，否则回退日期字符串闭区间（`analytics.ts:154-159`）。区间非法（缺失/逆序）时谓词恒 false（`analytics.ts:149`，错误文案 `analytics.ts:114-123`，测试 `analytics.test.ts:44-78`）。

**分钟桶与旧日粒度的回退逻辑【已核实，结论与提问前提不同】**：浏览器**不读取账本内部 `schemaVersion`**（全库 `schemaVersion` 校验只针对三个信封：`data.ts:503,629,673`）。粒度区分完全靠 **`row.timestampMs > 0`**：

- 通用视图：日粒度行照常参与聚合与筛选，界面提示"部分旧账本只有日期，暂按设备记录的日期筛选；升级设备端并同步后可精确到分钟"（`app.tsx:299-303`），质量卡单列"仅有日期"桶数（`insights.ts:108-109`；`components/DataQuality.tsx:52-57`）。
- 配额周期视图：日粒度行**完全排除**并单列计数（`quotaCycles.ts:189-194`）。
- 因此"schema v7 分钟桶 vs 旧日粒度"这一区分在 web-ui 侧是**按行字段能力**实现的，与版本号无关【已核实】；账本版本号的实际序列（测试中出现 2 与 8，`data.test.ts:69-70,799`）在 web-ui 侧**未被消费**【已核实】。

**筛选维度全集【已核实】**：`FILTER_LABELS` 定义 8 个维度（`analytics.ts:20-29`）——`device` 设备 / `tool` 客户端 / `model` 模型 / `vendor` 模型厂商（映射自 `upstreamVendor`，`data.ts:780`）/ `routeProvider` 路由提供商 / `routeType` 路由类型 / `rawProvider` 原始 Provider / `tier` 速率-Tier。筛选为**全等比较**（`row[key] === value`，`analytics.ts:172-176`），选项由 `records ∪ requests` 去重后按 zh-CN numeric collator 排序（`analytics.ts:184-200`），已选但当前无数据的值仍保留为占位项（`components/FilterBar.tsx:53-55`）。**`billingChannel` 不是筛选维度**，只作为明细列"计费渠道"与 CSV 列出现（`components/UsageTable.tsx:79-84`）。

`routeProvider` 会被规范化：`routeType === "official"` 时按 `billingChannel` 与 `upstreamVendor` 替换为中文标签——`official-subscription` → "官方 · ChatGPT 订阅"；`official-api` 且厂商为 OpenAI → "官方 · OpenAI API"；`official-api` 且厂商非 OpenAI → 有厂商时 "官方 API · <厂商>"、**厂商为空时输出裸「官方 API」**（`data.ts:736-739`）；其余 billing 值有厂商 → "官方 · <厂商>"，无厂商 → "官方"（`data.ts:729-743`），其他类型保留原始 provider 小写；注释明确"浏览器绝不把原始 provider 名（如 `openai`）提升为 official，只有设备端本地 base-URL 映射能置 `routeType=official`"（`data.ts:711-753`，测试 `data.test.ts:664-685`）。

**分页 / 搜索 / 导出【已核实】**

- 客户端分页：页大小 25/50/100（`UsageTable.tsx:178-220,406-410`）；搜索为全列子串匹配且**不改变已排序顺序**（`UsageTable.tsx:195-214`）；分页与 CSV 都作用于"全部匹配行"而非当前页（`docs/FRONTEND.md:33`）。
- CSV：`makeCsv()` 输出 UTF-8 BOM + CRLF + 全字段双引号包裹 + `""` 转义（`analytics.ts:266-271`）；`csvCell()` 对 `^[\s]*[=+@\-\t\r]` 前缀加 `'` 防电子表格公式注入，且保留负数数值（`analytics.ts:260-265`，测试 `analytics.test.ts:113-130`）。页面级导出列为「表格列 + 定价状态（`Lower bound`/`Ledger estimate`）+ `Device ID` + `Timestamp (ms)`」，文件名 `usagemesh-ledger|requests-YYYY-MM-DD.csv`（`UsageTable.tsx:153-165`）；配额周期另有一套列（`views/QuotaCycles.tsx:82-106`）。
- 请求明细渲染字段（`columnsFor(true)`，`UsageTable.tsx:33-152`）：请求时间、设备、客户端、模型、路由、Tier、思考强度、总 Tokens、估算费用、模型厂商、路由类型、计费渠道、原始 Provider、输入、缓存读取、缓存写入、输出、Reasoning、请求数、耗时(ms)、Agent；费用格渲染为 `≥ `（下界时）+ **4 位小数**（`UsageTable.tsx:221-228`）；点击模型打开详情弹窗，附"定价状态"与"设备 ID"（`UsageTable.tsx:435-458`）。

**数据质量态判定【已核实】**（`web-ui/src/lib/insights.ts:94-128`）

| 态 | 判定 |
| --- | --- |
| 缺账本 `missing` | `max(0, expectedDevices - devices.length)` |
| 心跳延迟 `delayed` | `deviceSyncStatus(presenceAt) !== "online"` 的设备 |
| 账本过期 `stale` | `lastSync` 不可解析或 `now - lastSync > 24h` |
| 下界/未定价 `unresolved` | `!pricingResolved \|\| costLowerBound` 的**桶数** |
| 日粒度 `dated` | `!timestampMs` 的桶数 |
| 可见请求范围 `earliest/latest` | 仅由 `requests[].timestampMs` 求得，无明细则为 `null` |

渲染层明确"缺失记录不等于零消耗"、无明细时"无法证明请求历史覆盖"（`components/DataQuality.tsx:62-63,70`）。

**部分失败路径【已核实】**：`Promise.allSettled` 的 rejection 生成 `warnings` 文案（`data.ts:1065-1071`）；若该分支**本会话曾成功读取**且本次失败为 transient，则沿用旧账本并登记 `retainedDeviceIds`，文案附带旧账本时间（`data.ts:978-993,1081-1087`）；只有"索引非空但全部失败"才整体抛错（`data.ts:999-1004`）。`syncStatus` 由 `warnings.length` 决定 `partial|synced`（`useDashboard.ts:39`），横幅区分"N 台设备读取失败"与"部分设备更新失败"（`app.tsx:234-244`）。

### 6.4 `analytics.ts` / `insights.ts` 的算法

**集中度【已核实】**：`groupRows()` 按任意维度求和，`share = value / 全部行该指标之和 × 100`，降序（`analytics.ts:217-235`）；集中度取 `groups.slice(0,3)` 的 share 之和（`views/Analytics.tsx:32-34`）。**分母不因图表只展示前 14 项而缩小**（`Analytics.tsx:193-198`），测试锚定 15 项等量时前三项 = 20%（`tests/analytics.test.ts:94-104`）。

**效率指标【已核实】**（`Analytics.tsx:92-117`，基础量在 `analytics.ts:204-216`）

- `inputSide = input + cacheRead + cacheWrite`；缓存命中率 `= cacheRead / inputSide × 100`（无输入侧为 0）。
- 平均 Tokens/请求 `= totalTokens / requestsCount`（并给出 `cost / requestsCount` 的 4 位小数）。
- 输出/输入侧 `= (outputTokens + reasoningTokens) / inputSide × 100`（注明输出含 Reasoning）。
- 每百万 Tokens 费用 `= cost / totalTokens × 1e6`（下界时前缀 `≥`）。
- `lowerBound = rows.some(r => r.costLowerBound)` 会向上传播到汇总（`analytics.ts:214`，测试 `analytics.test.ts:105-111`）。

**趋势【已核实】**：`trend()` 按本地日期聚合成天并升序输出，**不为缺失日期补 0 点**（`analytics.ts:236-257`）。

**模型成本变化归因（cost-change attribution）【已核实】**（`insights.ts:33-93`）

- 窗口：`comparisonRanges(period)`（`insights.ts:5-32`）——`7d`/`30d` 的 `end` 是**今天 00:00**（即排除今天），`start = end − 7|30 天`，`previous = start − 7|30 天`；`month` 为「上月 1 日 00:00 → 本月 1 日 00:00」，`previous` 为「上上月 1 日」。跨年/闰月用 `Date` 的分月递减实现，测试锚定不会溢出（`tests/insights.test.ts:40-51`）。
- 归因：仅沿用**维度筛选**（`timeRange` 被强制为 `all`，`insights.ts:40`），按 `row.model` 对各期 `cost` 求和，`delta = current − previous`，按 `|delta|` 降序**取前 3**（`insights.ts:72-75`；UI `components/PeriodComparison.tsx:90-104`）。
- 阈值与抑制（`changeLabel()`，`insights.ts:78-93`）：任一期无记录 → `记录不足，暂不计算变化`；`lowerBound` → `费用为下界，暂不计算变化`；`previous === 0 && current !== 0` → `前期为 0，不计算百分比`（不给无穷）；两者皆 0 → `与前期持平`；否则给出 1 位小数百分比。
- 另有一层整体抑制：`warnings.length > 0 || devices.length < expectedDevices` 时整个对比区间隐藏百分比与模型归因（`PeriodComparison.tsx:25-28,73-74,89`，测试 `insights.test.ts:73-88`）。

### 6.5 配额周期（`quotaCycles.ts` + `subscriptionPeriods.ts` + `subscriptions.ts` + `views/QuotaCycles.tsx`）

**官方配额载荷准入校验【已核实】**：账本里的 `officialQuota` 不照单全收，进入视图前先经 `sanitizeOfficialQuota()`（`data.ts:368-392`）逐层过滤；返回 `null` 时**该设备的配额数据整块被静默丢弃**、不参与归并（调用点 `data.ts:1033-1038` 的 `return value ? [value] : []`），其它设备不受影响。

- 信封：`version !== 1` 整体拒绝（`data.ts:372`）；`latest` 只取前 256 条（`:373-375`）、`cycles` 只取前 10_000 条（`:379-381`）。
- 观测快照 `latest`：必须同时满足 `provider === "codex"`、`source === "codex-app-server"`、`status ∈ {observed, stale}`（`data.ts:211-217`）——**`status="unavailable"` 的条目整条丢弃**；`windows` 只取前 32 条（`:224-226`），单个窗口要求 `name`/`usedPercent`/`remainingPercent` 齐备且两个百分比都落在 `[0, 100]`（`:192-207`）；另有两条**可选**字段的硬约束——`resetsAt` 若存在必须可解析（RFC3339 且 `Date.parse` 有限），`windowMinutes` 若存在必须落在 `[1, 5_256_000]`（上界 `10 × 365 × 24 × 60`），违反者只**丢弃该窗口**（`quotaWindow()` 返回 `null` 后被 `filter` 剔除），**不使整条快照失效**（`:198-205`）。
- 周期 `cycles`：`closureReason` **只接受显式 `null`**、`"reset-adjustment"`、`"window-changed"`，其余取值被归一为 `undefined` 并触发 `return null`，即**整条 cycle 丢弃**（`data.ts:273-278`；`:303` 的 `closureReason === undefined` 判定）。**键缺失与非法值同归丢弃**——判定式写作 `row.closureReason == null || … ? row.closureReason : undefined`，`== null` 对 `undefined`（键缺失）同样成立，于是键缺失时返回 `undefined` 并被 `:303` 判为非法；**只有显式 `null` 能通过**。真实产物不受影响：`Cycle` 的 `closure_reason: Option<String>` 无 `skip_serializing_if`（`rust-cli/src/local/quota_history.rs:34-35`），`export()` 走 serde 序列化（`quota_history.rs:100-110`），故序列化**恒带 `"closureReason": null`**；`samples` 只取前 10_000 条（`:281`）。
- 官方用量 `officialUsage`：`status ∈ {observed, stale, unavailable}`（此处**允许** `unavailable`，与 `latest` 的白名单不同，`data.ts:334-339`）；日用量桶只取前 4_000 条（`:344`）。
- 行为锚点：喂 1 条合法 `stale` 与 5 条非法状态（`unavailable` / `fresh` / `inferred` / `""` / `null`）后只保留 `stale` 一条，`updatedAt`、`windows` 原样保留，`cycles` 不受影响（`web-ui/tests/data.test.ts:122-140`，用例名"未知状态不能冒充官方观测"）。

**周期身份【已核实】**：族键 = `[accountKey, limitId, windowName, windowMinutes]`（`subscriptionPeriods.ts:15-21`；`subscriptions.ts:18-19` 的 `windowIdentity` 只用前三项），归并时再加 `resetsAt`（`quotaCycles.ts:287-295`）；**5 秒容差**内视为同一 reset（`tolerance = 5000`，`subscriptionPeriods.ts:3,53`；产品侧同规则的另一处实现是 `subscriptions.ts:20-24` 的 `sameReset()`，被 `views/QuotaCycles.tsx:729` 使用；`quotaCycles.ts:81,121` 的容差比较**仅被测试引用**，见下段）。源窗口时长不同则**不合并**（测试 `quotaCycles.test.ts:323-329`）。

**仅测试引用的导出（不参与产品渲染）【已核实】**：`quotaCycles.ts` 的 `displayQuotaCycles()`（`:64-101`）、`quotaCycleStatus()`（`:104-125`）、`fullQuotaWindow()`（`:128-130`）在整个 `web-ui/` 内**只被 `tests/quotaCycles.test.ts` 引用**（`:5-7` 导入；调用点 `:796,797,806,824,827,830,833`），产品视图的 import 列表（`views/QuotaCycles.tsx:18-31`）不含三者；`quotaCycleStatus` 的 `current/replaced/ended/unconfirmed` 状态机不参与任何渲染路径。

**跨设备如何相加【已核实】**：`aggregateQuotaCycle()`（`quotaCycles.ts:178-219`）遍历**全部设备的 records**，仅保留 `isOfficialSubscription(row)` 的行——即 `tool.trim().toLowerCase() === "codex"` **且** `routeType === "official"` **且** `billingChannel === "official-subscription"`（`quotaCycles.ts:132-138`）——对 tokens / requests / cost **直接相加**（`quotaCycles.ts:207-209`），设备数取去重 `deviceId` 计数（`quotaCycles.ts:211`）。

**账户配额百分比为何只取最新快照【已核实】**：`mergeOfficialQuotaData()` 按 `[accountKey, limitId]` 归并 `latest`，只保留 `updatedAt` 更新者（`newer()` `quotaCycles.ts:301-303`），并且**失败刷新产生的 `stale` 不会被同时间戳的更旧副本"恢复"成 fresh**（`quotaCycles.ts:317-323`）；源码注释直接写明 "Quota percentages are observations, never totals"（`quotaCycles.ts:305`）。理由与实现一致：百分比是**同一账号同一额度窗口的瞬时观测**，多设备只是复制同一观测，相加会得到 >100% 的假值。测试锚定：同账户同额度 `latest` 只保留 1 条且取 `usedPercent = 25`（较新观测），而 `cycles[0].lastUsedPercent` 取较新设备的 35%（`quotaCycles.test.ts:278-309`）；不同 `accountKey` 即使 `limitId`/cycle id 相同也严格隔离（`quotaCycles.test.ts:311-321`）；同账户每日用量由较新快照覆盖（09-07 由 20 覆盖为 25、09-06 保留 10），**不把设备副本相加**（`quotaCycles.test.ts:426-457`）。

**被排除的渠道【已核实】**：`official-api`（billingChannel 不符）、`relay` 等中转（`routeType` 不符）、`unknown`/来源不明（两字段都不符）、非 Codex 客户端（`tool` 不符）全部排除。测试用 5 行夹具验证只取 Codex 官方订阅两行：`totalTokens = 300`、`requests = 5`、`cost ≈ 0.3`、`deviceCount = 2`（`quotaCycles.test.ts:83-138`）。文档口径一致（`README.zh-CN.md:196`、`web-ui/src/views/Guide.tsx:23`、`docs/QUOTA-FORECAST.md:40`）。

**跨精确 reset 边界的分钟桶【已核实】**：桶定义为 `[timestampMs, timestampMs + 60_000)`；**只有整桶落在 `[from, to)` 内才计入**（`quotaCycles.ts:196-198`）；与之相交但不被完全包含的桶累加进 `boundaryTokens` / `boundaryBuckets` 单列，UI 提示"已排除 N 个跨边界分钟桶及 M 个仅有日期的记录"（`quotaCycles.ts:199-202`；`views/QuotaCycles.tsx:123-128`）。测试锚定：左跨界 11 + 右跨界 33 → `boundaryTokens = 44`、`boundaryBuckets = 2`，仅 `inside` 行入账（`quotaCycles.test.ts:140-178`）。
【按代码推演，测试未直接断言】当桶起点 `>= to` 时（测试里 `at-end` 那行，起点等于 `to`），它既不入账也**不进入 boundary 计数**（`quotaCycles.ts:197-202` 两个分支都不命中），即被静默丢弃——该行的 44 tokens 恰好与断言值 44 相同，读测试时易误判来源。

**文档待核对项已可关闭（文档写法 vs 实现）【已核实】**：`docs/LOCAL-WORKSPACE.md:87` 留了一条「云端分钟桶跨越**秒级**边界时单列待核对；不会用有保留上限的请求列表替代完整账本统计」。实现里跨界判据不是「秒级」，而是**分钟桶是否完整落在周期 `[from, to)` 内**（`quotaCycles.ts:195-198` 的 `bucketStart >= bounds.from && bucketEnd <= bounds.to`），部分重叠者单列进 `boundaryTokens`/`boundaryBuckets`（`quotaCycles.ts:199-202`）——「边界」指**周期起止**（另见 §6.10 第 3 条、§7.11 的「时间桶」行）；后半句「不用请求列表替代完整账本统计」与实现一致（周期金额只累计完整聚合桶，§6.8-2 的 3000 桶/$1,150 对 1000 条/$150 即此断言）。

**周期边界【已核实】**：`cycleBounds()`（`quotaCycles.ts:43-61`）——`from = resetsAt − windowMinutes×60_000`（缺 `windowMinutes` 时退回 `nominalStartAt`）；`segment > 0` 时 `from = max(nominalStartAt, firstObservedAt)`；`to = min(resetsAt, closedAt)`（无 `closedAt` 则 `resetsAt`）。

**金额换算公式【已核实】**：`forecastCycleCost()`（函数体 `quotaCycles.ts:230-281`；`:222-227` 是同文件的 `cycleCostSyncPending()`、`:229` 为文档注释）

```
recorded  = Σ rows.cost   （只取 timestampMs + 60_000 ≤ floor(observedAt / 60_000) × 60_000 的完整分钟）
total     = recorded / usedPercent × 100
remaining = max(0, total − recorded)
```

- 前置条件：`usedPercent ∈ (0, 100]`、`to > from`、`now < to`、`observedAt ∈ [from, now]`、`now − observedAt ≤ 15 分钟`、且账本已覆盖观测前最后一个完整分钟（`cycleCostSyncPending()`，`quotaCycles.ts:222-227,244-259`）。
- 测试锚定：$50 / 25% → `total = 200`、`remaining = 150`，**与周期已过时间无关**（`quotaCycles.test.ts:689-700`）；观测之后的消费被排除（`recorded` 仍为 50，`quotaCycles.test.ts:701-708`）；100% 时 `remaining = 0` 且保留 `partialPricing`（`quotaCycles.test.ts:779-785`）；零用量/非法比例/无计价金额一律返回 `null`（`quotaCycles.test.ts:758-768`）。
- 暂停换算的触发条件（UI 层聚合成 `unavailable` 文案，`views/QuotaCycles.tsx:394-425`）：快照 `stale` 或 >15 分钟、窗口已到期、多账号（`hasConflictingAccounts()` `subscriptions.ts:152-172`）、非 `codex` limitId、周期内出现 `quota-adjustment`（百分比下降）、账本尚未覆盖完整分钟，以及两条常见路径——**该窗口已用百分比为 0** → "额度尚未使用，暂无法估算"（`used === 0`，`QuotaCycles.tsx:410-411`）、**本周期尚无任何用量行** → "等待本周期用量记录"（`!result?.rows.length`，`:418-419`）；全部条件都不命中且无计价记录时兜底为"暂无可用于估算的计价记录"（`:425`）。UI 直接把公式显式呈现为 `{money(recorded)} ÷ {usedPercent} × 100 = {money(total)}`（`QuotaCycles.tsx:459-463`）。

**主额度区的 `limitId` 硬编码（文档写法 vs 实现）【已核实】**：`docs/QUOTA-FORECAST.md:5` 写「主额度按行同时显示，只出现官方实际返回的窗口；……Spark 等专用额度作为面板底部的紧凑分组」；实现是**硬编码筛选**——主额度区只取 `item.snapshot.limitId === "codex"`（`views/QuotaCycles.tsx:269`，另见 `:702-703` 的 `primary` 与 `:393` 的 `supported` 开关），官方返回的其他 `limitId`（如 `codex_plus`）整块移入底部「专用额度」分组（`views/QuotaCycles.tsx:271-275`）。即**非 `codex` 的 limitId 不会出现在主额度区**——这正是上一条「非 `codex` limitId → 暂停换算」条件块的由来，按「任意 limitId 都可作主额度」复刻会与产品口径不符。

**周期归档规则【已核实】**（`buildSubscriptionPeriods()`，`subscriptionPeriods.ts:34-237`）

- 仅接受 `windowMinutes > 0` 且 `resetsAt` 可解析的观测（`:42-47`）；同族内 reset 相差 ≤ 5 秒者合为一组（`:51-57`）。
- 权威来源：优先"仍存在于当前快照列表中的那一条"，否则取最近观测（`:69-72`）；`nominalStartAt` 被重算为 `reset − windowMinutes × 60000`（`:74,104`）。
- 采样重采样：合并 `firstObservedAt`、`samples`、`lastObservedAt`，仅保留落在 `[start, reset)` 且 `≤ 最后一次观测` 的点（`:76-88`）。
- 百分比下降**只记为周期内 `quota-adjustment` 变化，不新开周期**（`:92-101`，测试 `subscriptionPeriods.test.ts:99-104`）。
- 提前关闭（`closureReason === "window-changed"`）或"另一设备在旧窗口到期前就观测到新 reset"时，旧 reset 不成为历史行，只以 `reset-time` 变化留在后继周期内（`:130-192,194-211`）；被替换节点不输出（`:219-233`）。
- 历史过滤 `isElapsedSubscriptionPeriod()`：`resetsAt ≤ now` **且**（`lastUsedPercent > 0` 或存在正百分比样本），即"从未使用的漂移 0% 窗口不归档"（`subscriptionPeriods.ts:240-248`；`docs/QUOTA-FORECAST.md:29`）。
- 当前窗口视图由**最新快照**驱动（`currentSubscriptions()`，`subscriptions.ts:27-128`），包含全新的 0% 窗口（`:26`），并把最新快照的百分比并入样本序列（`:90-93`）。

**耗尽快照预测【已核实（调用点）/ 未验证（算法细节）】**：`views/QuotaCycles.tsx:39` 从 **Rust 侧共享的 JS 模块** `rust-cli/local-web/quota-forecast.js` 引入 `forecastQuotaCycle`，用于"预计 X 耗尽"提示（`QuotaCycles.tsx:185-197,248-250`）；其类型声明为 `method: "stable-linear-regression-v1"`，状态机 `ready|partial|insufficient|expired|invalid`，原因枚举含 `stale-snapshot|rapid-jump|low-confidence` 等（`rust-cli/local-web/quota-forecast.d.ts:1-76`）。**该 JS 文件未逐行阅读**（见未验证项）。

### 6.6 金额：web-ui 是否重算成本？

**结论：不重算，直接消费账本 `costUsd`。【已核实，单点直证 + 测试锚定】**

> 摘录改写，语义等价；原文见 `data.ts:24-29`（源码为两行 `if`/`else`，且无行内注释）。

```ts
// web-ui/src/lib/data.ts:15-39
for (const record of records) {
  record.cost = record.storedCost;                    // storedCost <- row.costUsd (data.ts:793)
  record.pricingResolved = !record.costLowerBound;    // 只做下界标记传播
  if (record.costLowerBound) fallbackRows += 1; else resolvedRows += 1;
}
```

- 源码注释即为设计意图："Device-side pricing is the accounting source of truth… Keep the signed-in dashboard on the exact cost that was calculated request-by-request before encryption."（`data.ts:18-21`）。
- 返回的 `PricingStatus` 是**常量**：`source: "设备端账本 · 兼容价卡估算"`、`sourceUrl` 指向 `docs/PRICING.md`、`updatedAt: null`、`fastMultiplier: 1.0`（`data.ts:30-38`）——没有 `fastMultiplier` 之类的倍率运算被应用。
- 请求级同理：`cost: Number(row.costUsd || 0)`（`data.ts:848`）。
- 全库无价卡下载、无汇率/货币换算、无 `fast` 倍率乘算代码；`pricing` 只用于展示（`Settings.tsx:220-222` 的"定价未完整解析 N 个聚合桶"、`views/Overview.tsx:38` 的"设备端兼容价卡 · 非实际账单"）。
- 测试锚定：账本 `costUsd: 12.345` → `records[0].cost === 12.345`，`fallbackRows === 1`（`tests/data.test.ts:68-101`）。
- 口径文档一致（`docs/FRONTEND.md:77,107`；`docs/FRONTEND-PERFORMANCE.md:61`：合成夹具中 3000 桶周期总额 $1,150 而 1000 条留存明细只有 $150，证明明细不能替代账本计价）。

**唯一的"金额推导"**是订阅周期的比例外推（§6.5 的 `recorded / usedPercent × 100`），其分子仍是**设备端 `costUsd` 之和**，不是重新定价；另有若干"API 等价"措辞的展示（`QuotaCycles.tsx:478`）。【已核实】

**未被挂载的情景估算组件【已核实】**：`components/QuotaCapacityScenario.tsx:26` 实现 `capacity = tokens / delta × 100` 的用户自校准线性估算，但全库检索显示**只有它自身与它的测试引用它**（`tests/quotaCapacity.test.tsx:4`），任何视图都未挂载——属预留/死代码。

### 6.7 持久化与偏好

- `preferences.ts`（53 行）：`Preferences = { textSize: "standard"|"large", theme: "light"|"dark"|"system", refreshSeconds: 0|10|30|60, monthlyBudget: number }`，默认 `{standard, system, 10, 0}`（`preferences.ts:7-12`）；键 `usagemesh:preferences:v1`（`:30`）；逐项白名单校验，非法值回退默认（`:34-48`）；旧键 `usagemesh:theme` 仅用于推导主题（`:32-39`）；读写都包 `try/catch`，存储不可用时静默降级（`:13-26`）。侧边栏折叠状态独立键 `usagemesh:sidebar`（`app.tsx:75,97-99`）。
- `savedViews.ts`（75 行）：键 `usagemesh:views:v2:<repo>`（`:15,70`）；读取时**最多保留 20 条**（`:58`）、名称 trim 后截断 40 字符（`:56`）、重复 `id` 丢弃（`:47`）、非法 filters 丢弃（`:53-54`）；无 v2 时回退旧单视图键 `usagemesh:view:<repo>`，标记 `id: "legacy"`、名称"原有保存视图"（`:60-63`）；写入失败返回 `false`，UI 提示"变更仅在本次页面停留期间有效"（`components/SavedViews.tsx:24-28`）；新增前检查重名与 20 条上限（`SavedViews.tsx:80-91`）。
- 刷新间隔语义：**仅前端轮询**（`useDashboard.ts:210-217`，标签页隐藏时暂停），设置页明确"此设置不会改变设备端采集频率"（`views/Settings.tsx:107`）。
- `docs/FRONTEND-PERFORMANCE.md` 性能约束要点（`:33-38`）：批处理编译时间边界与维度谓词、`all` 范围跳过日期解析；筛选选项不再两边拼接；请求过滤只在分析页执行、非数据页跳过；复用 `Intl` 格式化器与 collator、表格排序键每行只读一次；搜索复用已排序结果与缓存列文本（`WeakMap`，`UsageTable.tsx:197-214`）；未变更的加密载荷复用已解密的数组（避免重复 AES/JSON/归一化）；缓存命中要求 nonce/ciphertext 匹配、换 repo/key/锁定即失效且从不写浏览器存储；memo 组件在账本未变时不重建子树。基准（`:19-29`，5 万行合成、函数级非整页）：all-time 过滤 18.13→1.14 ms、自定义分钟 37.92→1.02 ms、模型名排序 2753.17→115.62 ms、1 万个时间戳格式化 306.13→8.65 ms；网络侧心跳并行化在"账本 100 ms + 心跳 100 ms"合成场景下 202.92→101.81 ms（`:50-52`）。

### 6.8 测试固化的边界断言（10 条）

1. **费用不在浏览器重算 + 请求不额外截断**：账本 `costUsd: 12.345` → `records[0].cost === 12.345`、`pricing.fallbackRows === 1`、5100 条请求全部保留（`web-ui/tests/data.test.ts:68-101`）。
2. **周期金额只用完整聚合桶，明细留存不能替代账本**：3000 桶 → `aggregation.cost ≈ 1150`、`models` 3 个；请求明细仅 1000 条且合计 `≈ 150`；`forecastCycleCost(79%)` → `recorded ≈ 1150`、`total ≈ 1150 / 0.79`（`tests/data.test.ts:773-846`）。
3. **PBKDF2 双迭代数兼容 + 失败语义区分**：`[310_000, 600_000]` 均能解出同一密钥、错误密码抛"密码不正确"；`99_999` 抛 KDF 错；无网络时抛"数据读取失败"（`tests/data.test.ts:44-67`）。
4. **分支与设备哈希强绑定**：`um-ledger-ccdd` 分支配 `deviceHash: aabb` 的信封 → `格式不受支持`（`tests/data.test.ts:102-109`）。
5. **心跳与账本下载并行**：账本响应挂起期间 presence 请求已发出，最终 `devices[0].presenceAt` 取心跳时间（`tests/data.test.ts:848-884`）。
6. **跨设备求和与渠道排除**：仅 Codex 官方订阅两行入账 → `totalTokens 300` / `requests 5` / `cost ≈ 0.3` / `deviceCount 2`，设备名排序 `["PC","Mac"]`（`tests/quotaCycles.test.ts:83-138`）。
7. **左闭右开窗口与跨边界单列**：仅 `inside` 入账，左右跨界合计 `boundaryTokens 44` / `boundaryBuckets 2`（`tests/quotaCycles.test.ts:140-178`）；日粒度旧行 `totalTokens 0` / `legacyTokens 88`（`:180-195`）。
8. **百分比不跨设备相加、每日用量取较新覆盖**：`latest` 仅 1 条取 25%、`cycles[0].lastUsedPercent` 取 35%（`tests/quotaCycles.test.ts:278-309`）；不同 account 保持 2 条（`:311-321`）；每日桶 09-07 由 20 覆盖为 25 且 09-06 保留 10（`:426-457`）。
9. **金额公式与边界**：$50 / 25% → `total 200`、`remaining 150`、与已过时间无关（`tests/quotaCycles.test.ts:689-700`）；观测之后的消费被排除（`:701-708`）；100% → `remaining 0`、`partialPricing true`（`:779-785`）。
10. **会话与筛选边界**：`sessionStorage` 不含明文密钥、跨 repo 不能恢复、闲置 30 分钟与 12 小时绝对期限均失效、损坏密文/缺 `createdAt` 不能恢复（`tests/session.test.ts:12-50`）；自定义范围含结束分钟最后一毫秒、排除 `+60_000 ms`（`tests/analytics.test.ts:44-61`）；集中度分母含未展示的第 15 项（`analytics.test.ts:94-104`）；`5 秒 reset 漂移` 归同一周期且采用当前窗口时间、无 `reset-time` 变化（`tests/subscriptionPeriods.test.ts:107-118`）。

### 6.9 聚合口径细节

- **两层聚合分工**：设备侧把原始请求折叠成"聚合行"（`ledger.rows`，字段见 `data.ts:79-98`：`date/timestampMs/client/provider/upstreamVendor/routeProvider/routeType/billingChannel/model/tier/input/output/cacheRead/cacheWrite/reasoning/messages/costUsd/costLowerBound`）；浏览器把多设备聚合行**拼接**后再按维度重算分布视图（`data.ts:1018-1027` + `analytics.ts:201-257`）。
- **总数口径**：`totalTokens = input + cacheRead + cacheWrite + output + reasoning`（`data.ts:791`、`data.ts:846`）；请求数用 `messages`（聚合行 `Number(row.messages || 0)`，`data.ts:792`；请求行 `Math.max(1, Number(row.messages || 1))`，`data.ts:847`）。
- **客户端/标签映射**：`tool <- row.client`、`vendor <- row.upstreamVendor`、`platform/architecture` 做 macOS/Windows/Linux 与 **Apple Silicon / Intel / x86_64 / ARM64** 归一——`macOS` 下 `aarch64`/`arm64` → "Apple Silicon"、`x86_64`/`x64` → **"Intel"**（不是 "x86_64"），非 macOS 才输出 `ARM64`/`x86_64`（`data.ts:681-699`，平台与架构分支 `:691-697`）；`tier` 归一为 `Fast/Priority/Standard`（`data.ts:701-709`）。
- **请求明细的 `date` 由时间戳现算**（`new Date(ts).toISOString().slice(0,10)`，即 **UTC 日期**），与聚合行 `date`（设备端）可能不是同一口径（`data.ts:824-825`）；但筛选/趋势一律走 `rowDate()` 的**本地日期**规则（`analytics.ts:109-113`）。

### 6.10 配额周期规则

（凝练版，逐条均可回源到 §6.5）

1. 只统计 `tool=codex` + `routeType=official` + `billingChannel=official-subscription` 的**分钟桶**；`official-api`、中转、来源不明、非 Codex 客户端一律排除。
2. 跨设备对 tokens / requests / cost 求和；配额百分比只取**同一 [accountKey, limitId] 的最新快照**，绝不跨设备相加或跨账号混算。
3. 周期边界 `[resetsAt − windowMinutes×60_000, min(resetsAt, closedAt))`；整分钟桶左闭右开，跨界桶与日粒度旧行分别单列计数并从总量排除。
4. 周期身份 = `[accountKey, limitId, windowName, windowMinutes, resetsAt(±5s)]`；百分比下降、同一 reset 的分段、秒级漂移都归入同一周期并记录为周期内 `changes`，不产生额外历史行。
5. 历史只收录**已到期**且有正用量痕迹的周期；被替换/提前关闭的旧 reset 不复活；跨过多个空周期不补造。
6. 等价金额 = `已记录完整分钟金额 ÷ 官方已用百分比 × 100`，且仅在观测新鲜（≤15 分钟）、单账号、无周期内额度调整、账本已覆盖观测前完整分钟时给出。

### 6.11 未验证项

1. **未执行任何测试或构建**：任务约束禁止 `npm install` / `npm run build`，所有测试结论来自静态阅读；`npm test` 的实际绿/红状态未验证。
2. **设备端 `access.json` 生成细节**：实际默认迭代数（注释只说"设备端更强默认值"，测试用 310k/600k）、盐长度、`updatedAt` 语义均未读 Rust 侧代码。
3. **账本版本号语义**：`schemaVersion` 真实序列（测试里出现过 2 与 8）与"v7 分钟桶迁移"的历史边界未验证——web-ui 不读该字段，无法从本侧证实；本文对"分钟 vs 日粒度"的结论只覆盖浏览器行为。
4. **线上响应特征**：`um-index/index.json`、`ledger.json`、`presence.json` 的真实 HTTP 头（是否带 ETag / cache-control / CDN 缓存）未实测；前端确实不使用条件请求（已核实），但 GitHub 端行为未验证。
5. **设备端心跳写入频率**与 3/10 分钟阈值的匹配关系未与 Rust 侧核对。
6. **GitHub API 备用路径的真实速率限制阈值**未验证；代码只保证正常轮询不消耗 API 配额。
7. **`rust-cli/local-web/quota-forecast.js` 算法细节**未逐行阅读（只读了 `.d.ts` 与相关测试标题）；"预计耗尽时间"的回归窗口与置信阈值属【未验证】。
8. **`expectedDevices` 与实际设备数的差异来源**（例如索引残留已注销设备）在 web-ui 侧无测试覆盖。
9. **`?repo=` 缺省时的静态回退**在 fork 场景下的实际可用性未实测（代码路径已核实，`pages.yml:90-99` 的覆盖逻辑未运行验证）。
10. **CSP 与 `no-referrer`**（`web-ui/index.html:6-7`：CSP 在 `:6`、referrer 在 `:7`，`:5` 是 viewport；CI 断言 `pages.yml:55-64` 内第 `:62-63` 行分别 grep 两者）在部署环境中的实际生效情况未实测；本轮只核实了文件内容。

## 7. 本地工作台与官方配额


本地工作区是 `usagemesh serve` 单进程的 loopback HTTP 服务（tiny_http，绑定 `127.0.0.1`），内嵌一份静态前端（`include_bytes!` 编译进二进制，无需 Node）。它做四件事：扫描本机客户端日志（Tokscale 口径，90 天）→ 读取 Codex 官方额度与官方每日 Tokens（`codex app-server --stdio`）→ 周期性采样并落盘观测历史（`quota-history.json`）→ 提供提醒、供应商配置受控写入、代理只读探测。其本地数据与云端账本**并存但独立**：`usagemesh sync` 上传的加密账本以**本机用量行与请求明细为主体**（`rust-cli/src/model.rs:138`、`:140`），整体 AES-256-GCM 加密（`rust-cli/src/crypto.rs:40-66`）、写入设备分支 `ledger.json`（`rust-cli/src/github.rs:367-376`），由 `rust-cli/src/main.rs:448-449` 触发；官方额度只是同一账本中的一个字段（注入点 `rust-cli/src/main.rs:404-407`，字段定义 `rust-cli/src/model.rs:132-134`）。**加密 ≠ 不上传**：本机活动明细随账本加密上传，账本不含提示词/响应正文（请求明细字段见 `rust-cli/src/collector.rs:241-261`）与账号明文标识（只存哈希）。

关键坐标：
- 服务入口与路由：`rust-cli/src/local/mod.rs:395-542`（路由表在 `State::api`，`rust-cli/src/local/mod.rs:106-300`）。
- 默认端口 `43821`：`rust-cli/src/main.rs:43`；CLI 参数 `--port` / `--no-open` / `--home` / `--data-dir`：`rust-cli/src/main.rs:42-52`；调用点 `rust-cli/src/main.rs:658-663`。
- 本地数据目录：非隔离时 `config_dir()/local`，隔离（给了 `--home`）时 `<home>/.usagemesh-local`：`rust-cli/src/local/store.rs:120-124`。

### 7.1 HTTP 接口表

鉴权模型（先于路由生效）：
- 每次启动生成 32 字节随机令牌，hex 编码 64 字符：`rust-cli/src/local/mod.rs:421`；访问链接 `http://{host}/#token={token}`：`rust-cli/src/local/mod.rs:422`；host 取实际监听地址（`--port 0` 时为内核分配端口）：`rust-cli/src/local/mod.rs:417-420`。
- `authorized()` 要求三条同时成立：`Host == expected_host`、`Origin` 缺省或等于 `http://{expected_host}`、`Authorization == "Bearer {token}"`：`rust-cli/src/local/mod.rs:379-389`。
- `Host` 不匹配直接 403 `Forbidden host`：`rust-cli/src/local/mod.rs:437-440`；鉴权失败返回 401 JSON `{"error":"访问凭据无效，请使用终端显示的本次启动链接"}`：`rust-cli/src/local/mod.rs:476-492`。
- 前端凭据来源：URL fragment → `sessionStorage["usagemesh-local-token"]`，随后清 fragment：`rust-cli/local-web/app.js:26-31`；401 时清 token：`rust-cli/local-web/app.js:462-467`。运行期换凭据：`hashchange` 监听从新 fragment 取 token、`history.replaceState` 清地址、重新拉取并回写 sessionStorage：`:1080-1094`；`lock` 动作清空内存 token 与 sessionStorage 后回到登录页：`:770-777`。前端所有 `/api/*` 请求带 180 秒超时（`AbortSignal.timeout(180000)`）：`rust-cli/local-web/app.js:453`。

| 方法 | 路径 | 入参 | 响应 / 副作用 | 证据 |
|---|---|---|---|---|
| GET | `/api/state` | 无 | 一次返回 `version, settings, activity, scanError, quotas, quotaConnection, quotaHistory, officialUsage, events, alerts, tools, backups, dataDir, platform` | `rust-cli/src/local/mod.rs:108-110` |
| POST | `/api/scan` | `{}` | 触发 `State::scan()`：写 `activity.json`、刷新官方额度、评估提醒；失败保留上次快照并置 `scanError` | `rust-cli/src/local/mod.rs:111-114`、`:39-60` |
| POST | `/api/quota/official` | `{}` | 手动刷新官方额度；按连接状态返回「已更新」或「未成功，已保留历史」文案 | `rust-cli/src/local/mod.rs:115-121` |
| POST | `/api/quota/cycle` | `{"id": <cycle id>}` | 从保留的 `cycles` 找周期，返回 `quota_history::details`（周期 + 同期本机官方订阅记录） | `rust-cli/src/local/mod.rs:122-131`、`rust-cli/src/local/quota_history.rs:304-347` |
| POST | `/api/quota` | `{"provider":"codex"\|"claude"\|"gemini"}` | 调 CodexBar CLI，替换该 provider 的 `codexbar:*` 额度 | `rust-cli/src/local/mod.rs:132-144`、`rust-cli/src/local/quota.rs:129-151` |
| POST | `/api/preferences` | `Preferences`（camelCase） | 保存提醒设置；校验 `budget≥0`、`threshold∈[1,100]`、`cooldown∈[1,1440]`、`scan∈[1,60]`、`spike∈[1.1,20]` | `rust-cli/src/local/mod.rs:145-160` |
| POST | `/api/notify-test` | `{}` | 发一条系统通知，返回可用性文案 | `rust-cli/src/local/mod.rs:161-163` |
| POST | `/api/project` | `{"id":..,"meta":{alias,tags,budget}}` | 保存项目别名/标签/预算，校验见 `validate_project` | `rust-cli/src/local/mod.rs:164-172`、`rust-cli/src/local/activity.rs:100-110` |
| POST | `/api/provider` | `Provider{id,name,tool,baseUrl,model,keyEnv}` | 保存方案（最多 50），**不修改工具配置** | `rust-cli/src/local/mod.rs:173-195` |
| POST | `/api/provider/delete` | `{"id":..}` | 删除保存的方案 | `rust-cli/src/local/mod.rs:196-201` |
| POST | `/api/provider/preview` | `{"id":..}` | 返回 `tool,path,beforeHash,afterHash,before,after(脱敏),note` | `rust-cli/src/local/mod.rs:202-235`、`rust-cli/src/local/configure.rs:118-122` |
| POST | `/api/provider/apply` | `{"id":..,"beforeHash":..,"afterHash":..}` | 校验哈希 → 再读比对 → 备份 → 原子写目标配置 | `rust-cli/src/local/configure.rs:123-149` |
| POST | `/api/provider/probe` | `{"id":..,"generate":bool}` | 模型目录或生成探测；事件入 `events.json`（上限 10000） | `rust-cli/src/local/mod.rs:212-223`、`rust-cli/src/local/network.rs:14-80` |
| POST | `/api/rollback` | `{"id": <backupId>}` | 仅当当前文件哈希等于备份 `after_hash` 时恢复 | `rust-cli/src/local/configure.rs:162-181` |
| POST | `/api/connector` | `Connector{id,name,baseUrl,keyEnv,kind∈cliproxy\|litellm\|openai}` | 保存连接器（最多 20） | `rust-cli/src/local/mod.rs:239-259` |
| POST | `/api/connector/delete` | `{"id":..}` | 删除连接器 | `rust-cli/src/local/mod.rs:260-264` |
| POST | `/api/connector/inspect` | `{"id":..}` | 只读读取账号元数据或模型目录（各上限 500 项） | `rust-cli/src/local/mod.rs:265-271`、`rust-cli/src/local/network.rs:81-115` |
| POST | `/api/events` | JSON 数组（≤10000 条） | 字段级硬约束（单条校验在 `rust-cli/src/local/activity.rs:116`）：`id` 非空且 ≤128 字符、`channel` 非空且 ≤100、`status` 100..=599、`latencyMs` 有限且 0..=3_600_000、`at` 必须 RFC3339；只保留白名单字段（`id`/`channel`/`status`/`latencyMs`/`at`/`retry`/`kind:"imported"`）后按 `id` 去重合并、按 `at` 排序、截断到 10000，写 `events.json` | `rust-cli/src/local/mod.rs:272-297`、`rust-cli/src/local/activity.rs:111-117` |
| GET | `/`、`/app.js`、`/style.css`、`/icons.js`、`/quota-forecast.js` | 无 | 静态资源，**在鉴权之前放行**；内容来自 `include_bytes!("../../local-web/*")` | `rust-cli/src/local/mod.rs:445-475` |
| GET | `/favicon.ico` | 无 | 204 | `rust-cli/src/local/mod.rs:441-444` |
| 其他 | 任意路径 | — | `bail!("接口不存在")` → HTTP 400 `{"error":...}` | `rust-cli/src/local/mod.rs:298`、`:532-540` |

统一响应与请求约束：
- 响应头固定 `Content-Type`、`Cache-Control: no-store`、`X-Content-Type-Options: nosniff`、`Referrer-Policy: no-referrer`、严格 CSP（`default-src 'none'` 起）：`rust-cli/src/local/mod.rs:390-394`。
- POST 必须 `application/json` 且长度 ≤2MB：`rust-cli/src/local/mod.rs:494-506`；实际读取上限 2_000_001 字节：`rust-cli/src/local/mod.rs:508-516`。
- 依赖注入的 `Instant` 节流：每 `scan_minutes` 自动扫描一次（默认 5 分钟）：`rust-cli/src/local/mod.rs:427-432`、`rust-cli/src/local/store.rs:58-68`。
- 浏览器打开：Windows 走 `rundll32.exe url.dll,FileProtocolHandler`：`rust-cli/src/local/mod.rs:543-552`。


### 7.2 HTTP 服务与安全边界【已核实】
见上表。补充：服务显式不做 CORS、不提供远程绑定（仅 `Ipv4Addr::LOCALHOST`），无 TLS；`--home` 隔离模式让 `CODEX_HOME`/`CLAUDE_CONFIG_DIR` 失效，避免测试污染真实目录：`rust-cli/src/local/store.rs:106-119`。
【未验证】静态资源在鉴权前放行意味着未持令牌的本地进程可读取前端 JS/CSS（不含数据）；未实测这一放行的浏览器侧表现。

### 7.3 `activity.rs` 的本地活动视图【已核实】
- 时间窗：`since = 本机当地日期 - 89 天`（约 90 个自然日），`until=None`：`rust-cli/src/local/activity.rs:9-11`、`:17`。
- 参数：`home_dir = paths.home`，`use_env_roots = !paths.isolated`，`clients = None`（全客户端），`scanner_settings = ScannerSettings::default()`：`rust-cli/src/local/activity.rs:12-20`。
- 证据与路由：非隔离时 `crate::evidence::scan(false)`，隔离时用空 `EvidenceBundle`：`rust-cli/src/local/activity.rs:22-26`（`evidence::scan` 定义在 `rust-cli/src/evidence.rs:774-805`）；逐条走 `crate::collector::route_for_message(&evidence, m, &m.client, &m.model_id)` 得到 `routeType` / `billingChannel`：`rust-cli/src/local/activity.rs:33`、`rust-cli/src/collector.rs:138-162`。
- 计价：`PriceBook::load()` + `prices.quote_on_date(&m.model_id, &m.date, Some("standard"), &metrics)`，未知保持未知并用 `lowerBound` 标记：`rust-cli/src/local/activity.rs:27`、`:63-64`、`:82`。
- 返回维度：`sessions`（`id = sha256(client:project:session)`、`projectId = sha256(project)`、`client`、`sessionKnown = 来源 session_id 非空`、`startedAt/lastAt/tokens/cost/monthCost/requests/lowerBound`，字段构造在 `rust-cli/src/local/activity.rs:73`）：`rust-cli/src/local/activity.rs:36-85`；`daily`（日期 → 费用）：`:86`；`requests`（含 `routeType`/`billingChannel`/`durationMs`）：`:87`；`sourceRows/retainedRows`：`:97`。
- 截断：请求明细仅保留最近 20000 条（会话汇总覆盖全窗）：`rust-cli/src/local/activity.rs:89-93`。

与云端账本口径的**一致**项：同一 Tokscale 解析入口、同一 `evidence + route_for_message` 路由、同一 `PriceBook`（`rust-cli/src/collector.rs:211-224`）。限定：`scanner_settings` 来源不同——本地固定 `ScannerSettings::default()`（`rust-cli/src/local/activity.rs:19`），云端 `load_scanner_settings()`（`rust-cli/src/collector.rs:207`、`:218`，定义在 `:78-93`：读 `config_dir()/tokscale/settings.json` 的 `scanner` 键，文件缺失或畸形回落默认）；用户自定义 scanner 配置时两端扫描结果分叉。
**差异**项：① 本地不做 Codex tier 增强（云端有 `codex_tier::collect` 与按天对账覆盖，`rust-cli/src/collector.rs:226-237`）；② 本地只有 90 天 + 20000 条请求上限，云端账本按设备保留自己的行与请求裁剪策略（`rust-cli/src/collector.rs:399`）；③ 本地无设备维度（云端有 `deviceId`）。详见末节。

### 7.4 配额观测的数据结构、采样、周期与 `stale`【已核实】
- 结构：`Sample{at, used_percent}`：`rust-cli/src/local/quota_history.rs:11-16`；`Cycle{id, account_key, account, limit_id, limit_name, window_name, window_minutes, resets_at, nominal_start_at, first_observed_at, last_observed_at, first_used_percent, last_used_percent, samples, closed_at, closure_reason, segment}`：`rust-cli/src/local/quota_history.rs:17-37`；`History{version, cycles, latest, official_usage, last_success_at, last_attempt_at}`：`rust-cli/src/local/quota_history.rs:38-47`。
- 存储：`<data>/quota-history.json`，原子写：`rust-cli/src/local/quota_history.rs:88-98`；版本必须为 1：`:90`；跨进程排他锁文件 `quota-history.lock`（`try_lock`）：`:287-303`。
- 采样触发点：服务启动（`rust-cli/src/local/mod.rs:415`）、周期扫描（`rust-cli/src/local/mod.rs:49`/`:55`）、手动刷新（`rust-cli/src/local/mod.rs:116`）、常驻同步 `usagemesh sync`（`rust-cli/src/main.rs:404-407` 调 `local::official_metadata()`）；常驻同步复用最近 5 分钟内**任意一次尝试**——节流键是 `last_attempt_at`，在 fetch（`rust-cli/src/local/mod.rs:313`）**之前**写入（`:305`），源码注释即 "Failed attempts also cool down"（`:338-339`），命中后**直接返回历史导出、不重试**，其状态按上次尝试的成败分路径：**成功**则为 `observed`（成功路径在 `record()` 中把 `latest` 覆写为本次快照，`rust-cli/src/local/quota_history.rs:279`；状态字面量 `"status":"observed"` 见 `rust-cli/src/local/quota_official.rs:589`），**失败**才为 `stale`（失败路径把 `latest` 全部置 `stale`，`rust-cli/src/local/mod.rs:306-308`、`:355-370`）：`rust-cli/src/local/mod.rs:326-332`、`:338-352`。**注（以代码为准）**：上游 `docs/LOCAL-WORKSPACE.md:83` 写作"常驻同步复用最近 5 分钟内**成功读取**的快照"，与本机实现相反（节流键是 `last_attempt_at`，失败同样冷却）——该文档措辞有误，勿按它把本节"改回"只认成功读取。
- 周期识别与重置边界：只接受 `source == "codex-app-server"` 的观测：`rust-cli/src/local/quota_history.rs:128`；`updatedAt` 不得比当前时间超前 2 分钟：`:129`；窗口合法性 `1..=525600` 分钟、百分比 `0..=100`、`reset > observed`：`:142-148`；`start = reset − minutes` 且不得晚于 `observed + 2min`：`:149-155`；同一 (account, limit, window) 只与最近周期比较且观测时间必须递增：`:156-169`；同重置时刻且 `minutes` 相同、未关闭、`used ≥ last` 才追加样本，否则关闭旧周期、若重置时刻相同则 `segment+1`：`:190-220`；关闭原因只有 `reset-adjustment` / `window-changed`：`:176-186`、`:204-218`。
- 0% 特例：`used == 0` 不建历史周期，只更新 `latest`（避免滑动窗口伪边界）；但**会关闭旧周期**——旧周期未关闭且（reset 时刻变化 或 前段 `last_used_percent > 0`）时置 `closed_at` 并标 `closure_reason`（同结束时刻 → `reset-adjustment`，否则 `window-changed`）：`rust-cli/src/local/quota_history.rs:111-117`、`:170-189`（关闭逻辑 `:171-187`）；单测 `:403-448`——同 reset 时保持开启（`:431`），reset 变化后断言 `closure_reason == Some("window-changed")` 与 `closed_at.is_some()`（`:442-446`）。
- 保留/压缩：周期保留 90 天、最多 400 个：`rust-cli/src/local/quota_history.rs:274-278`（常量 `:9-10`）；每周期样本超 256 时均匀抽稀并保留首尾：`:230-240`；`/api/state` 只返回最近 200 个周期：`:99-104`。
- `stale` 生成条件（HEAD 提交语义的核心）：
  1. 每次刷新**先**把 `history.latest` 全部置 `stale` 并落盘（失败时即保留旧观测）：`rust-cli/src/local/mod.rs:302-312`；
  2. 拉取失败时丢弃 `local-cli-telemetry` 来源、把 `codex-app-server` 观测置 `stale`，随后用回退通道重建本机额度视图：`rust-cli/src/local/mod.rs:77-83`、推送点 `:87`；该回退通道（`quota::codex`）扫描 `~/.codex/sessions` 与 `~/.codex/archived_sessions`，取修改时间最新的最多 120 个 JSONL，每文件只读尾部 8MB，仅接受 `type=event_msg` + `payload.type=token_count` 且带 `rate_limits` 的行，产出 `source:"local-cli-telemetry"` 快照（从未有记录时 `status="unavailable"`）：`rust-cli/src/local/quota.rs:12-82`；
  3. 失败同时把 `official_usage["status"]` 置 `stale`：`rust-cli/src/local/mod.rs:84-86`；
  4. `quotaConnection` 变为 `unavailable`（无任何历史）或 `stale`：`rust-cli/src/local/mod.rs:88`；
  5. 同步路径失败同样保留并置 `stale`：`rust-cli/src/local/mod.rs:355-370`；
  6. `merge_usage`：官方每日 Tokens 返回 `unavailable` 时，仅当 `accountKey` 相同且旧状态为 `observed|stale` 才保留旧快照、置 `stale`、写入 `error="官方 Token 活动暂时不可用，显示上次成功快照"`：`rust-cli/src/local/quota_history.rs:70-86`；单测 `:449-502`（含"换账号则不保留"分支）。
- 【未验证-提交级】克隆是浅克隆（`.git/shallow` 存在，`git rev-list --count HEAD == 1`），HEAD 提交是把整个仓库作为单次导入的快照，无法用 `git show` 隔离该修复的原始 diff；上述第 1、2、5、6 条只能作为**当前工作树**中实现该语义的证据，不是提交级 diff 证据。

### 7.5 `quota_official.rs` 读取已登录 `codex app-server`【已核实】
- 进程发现：候选目录 = 继承 PATH + `~/.local/bin`、`~/.codex/packages/standalone/current/bin`、`~/.npm-global/bin`、`~/.volta/bin`、`~/.fnm/current/bin`、`~/.nvm/current/bin`、`~/.bun/bin`、`~/.local/share/pnpm`、`~/Library/pnpm`，Windows 追加 `~/AppData/Roaming/npm`，Unix 追加 Homebrew 等系统路径：`rust-cli/src/local/quota_official.rs:171-203`；可执行名 Windows `codex.exe|codex.cmd|codex.bat|codex`，其他平台 `codex`：`:205-214`；取第一个存在的可执行文件：`:234-246`。
- 启动命令：`codex app-server --stdio`，stdin/stdout 管道、stderr 丢弃、覆盖子进程 PATH；隔离模式额外设 `CODEX_HOME`：`rust-cli/src/local/quota_official.rs:54-71`；Unix 用独立进程组、Windows 用 `CREATE_NEW_PROCESS_GROUP`：`:74-84`；析构时 Unix 杀进程组、Windows `taskkill /T /F`：`:248-272`。
- 协议：JSON-RPC 风格、按 `id` 匹配、忽略通知（`method` 非空的消息）：`rust-cli/src/local/quota_official.rs:372-391`；调用序列 `initialize(clientInfo=usagemesh)` → `initialized` 通知 → `account/read{refreshToken:false}` → `account/rateLimits/read` → `account/usage/read`（可选）→ 再 `account/read` 复核：`:278-330`。
- **鉴权 token 来源**：源码中没有读取 `auth.json`、没有注入令牌的路径；全程依赖 `codex app-server` 自身已登录的凭据（请求体只有 `clientInfo` 与 `refreshToken:false`）：`rust-cli/src/local/quota_official.rs:278-330`。文档亦声明不修改 `auth.json`：`docs/LOCAL-WORKSPACE.md:24`。
- 超时与错误：总 25s、初始化 5s、账号 5s、额度 8s、用量 4s：`rust-cli/src/local/quota_official.rs:17-21`；每步取 `min(总截止, now+额度)`：`:364-370`；单行输出上限 2MiB、总输出上限 8MiB，超限即 `Failure`：`:22-23`、`:92-130`、`:729-756`；EOF/超时/断开分别报错：`:385-388`。
- 失败保留旧观测的代码路径：任何 `Err` 冒泡到 `refresh_official_at` → `rust-cli/src/local/mod.rs:302-325` → `refresh_official()` 的 `Err` 分支 `rust-cli/src/local/mod.rs:77-90`（置 `stale` 且不删除周期）；另有账号一致性校验 `anyhow::ensure!`（`rust-cli/src/local/mod.rs:314-321`）。多账号标识不一致属直接失败、不回退：`limits_account_id` 汇总 `rateLimits`/`rateLimitsByLimitId` 中出现的账号标识并去重，>1 个即 `bail!("官方额度返回了不一致的账号标识")`（`rust-cli/src/local/quota_official.rs:462-489`，拦截点 `:485-487`）；账号读取另有 `"Codex 当前未登录 ChatGPT 账号"`（`:412`）与 `"Codex 当前不是 ChatGPT 订阅账号"`（`:418`）两条硬约束。
- 账号 / 限额身份：
  - 账号：`account.type ∈ {chatgpt, chatgptAuthTokens}`，标识按 `accountId → chatgptAccountId → id → email` 顺序取第一个非空（≤1024）：`rust-cli/src/local/quota_official.rs:408-451`；email 转小写。
  - `accountKey`：`sha256("usagemesh:codex-account:v1\0" + kind + "\0" + identifier)` 的 hex：`rust-cli/src/local/quota_official.rs:453-460`；额度返回的 `accountId` 存在时优先用它派生 key：`:343-346`；**原文不上传、不落盘**（周期里只存 hash），文档明示"脱敏后的账号标识…保存在本机"：`docs/LOCAL-WORKSPACE.md:83`；单测断言 key 不含明文 email：`rust-cli/src/local/quota_official.rs:780-789`。
  - `limitId`：`rateLimitsByLimitId` 存在时以其 map 为权威（键排序），否则回落单桶 `rateLimits`（`limitId` 缺省 `"codex"`）：`rust-cli/src/local/quota_official.rs:517-550`；空 map 不回落 legacy：`:851-857`。
  - 窗口：`primary|secondary|tertiary`，`usedPercent|used_percent`（0..100）、`windowDurationMins|windowMinutes|window_minutes`（>0 且 ≤525600，允许整数浮点）、`resetsAt|resets_at`（数字/字符串数字/RFC3339，≥1e11 视为毫秒）：`rust-cli/src/local/quota_official.rs:595-657`。
  - 额度对象形状：`{provider:"codex", source:"codex-app-server", accountKey, account, limitId, limitName, planType, updatedAt, status:"observed", windows[], note}`：`rust-cli/src/local/quota_official.rs:580-592`。`updatedAt` 为拉取时刻：`:350`。
- 官方日 token 总量（`officialUsage`）：`parse_usage` 只保留 `summary{lifetimeTokens, peakDailyTokens, longestRunningTurnSec, currentStreakDays, longestStreakDays}` 与 `dailyUsageBuckets[{startDate,tokens}]`（严格 `YYYY-MM-DD`、非负整数、重复日期以最后一条为准）：`rust-cli/src/local/quota_official.rs:659-699`；不可用时字段全 `null` + `error`：`:701-716`。
- **是否参与周期用量**：不参与。周期明细（`quota_history::details`）只统计本机请求行，与 `officialUsage` 无数据交集：`rust-cli/src/local/quota_history.rs:304-346`；前端也把二者分卡展示：`rust-cli/local-web/app.js:383-421` 与 `:842-852`。

### 7.6 `configure.rs` 的 guarded 读写（与「只读」对照）【已核实】
- 目标路径：Codex → `<codex>/config.toml`；Claude → `<claude>/settings.json`（`codex`/`claude` 由 `CODEX_HOME`/`CLAUDE_CONFIG_DIR` 或 `~/.codex`、`~/.claude` 决定）：`rust-cli/src/local/store.rs:106-119`、`:136-142`。
- 读取守卫：符号链接直接拒绝（提示在原管理工具修改）；>2MB 拒绝；不存在 → `None`：`rust-cli/src/local/configure.rs:23-37`。
- 方案生成：Codex 用 `toml_edit` 保留注释与无关配置，只写 `model`、`model_provider = usagemesh_<id>`、`model_providers.<id>{name, base_url, wire_api="responses", env_key?}`：`rust-cli/src/local/configure.rs:52-76`；Claude 写 `env.ANTHROPIC_BASE_URL`（去尾 `/` 与 `/v1`）、`env.ANTHROPIC_MODEL`，并在 `key_env` 有值时把**进程环境里的密钥值**写进 `env.ANTHROPIC_AUTH_TOKEN` 且移除 `ANTHROPIC_API_KEY`：`:77-99`。
- 预览：返回 `beforeHash`/`afterHash`（sha256）与脱敏后的前后文（`redact` 规则：键名含 key/token/secret/password/authorization → `••••••`，`*_env_key` 除外）：`rust-cli/src/local/configure.rs:118-122`、`rust-cli/src/local/store.rs:197-216`。
- 应用：哈希必须与预览一致 → **再次读盘比对**（并发保护）→ 备份到 `<data>/backups/<id>.json`（原内容 base64）→ 原子写目标：`rust-cli/src/local/configure.rs:123-149`。
- 回滚：仅当前文件 sha256 等于备份 `after_hash` 才恢复；原文件本来不存在则删除：`rust-cli/src/local/configure.rs:162-181`。
- 与「只读原则」对照：**UsageMesh 会写 agent 目录**——`apply`/`rollback` 直接写 `.codex/config.toml` 或 `.claude/settings.json`，且 Claude 分支会把密钥值写入该文件。这是 UsageMesh 的显式功能（受哈希/备份/回滚三重保护），与 TokenScope 的「只读、绝不写 agent 目录」是相反的设计选择，研究文档中应明确标注为差异而非缺陷。证据：`rust-cli/src/local/configure.rs:143-148`、`:171-179`；文档承认密钥落盘：`docs/LOCAL-WORKSPACE.md:35-36`。
- 单测覆盖：注释保留（`rust-cli/src/local/configure.rs:216-236`）、并发拒绝（`:238-252`）、回滚拒绝外部修改（`:254-267`）、Claude 保留 hooks + 预览脱敏（`:269-282`）、畸形配置不替换（`:284-290`）、symlink 拒绝（`:292-301`）。

### 7.7 `network.rs` 只读探测【已核实】
- HTTP 客户端：20 秒超时、**不跟随重定向**：`rust-cli/src/local/network.rs:8-13`。
- URL 约束：HTTPS 或 loopback HTTP；禁止内嵌凭据、查询参数、fragment：`rust-cli/src/local/store.rs:155-172`。
- `probe`：默认只 `GET {base}/models`（Claude 工具且 base 不以 `/v1` 结尾时补 `/v1`），显式 `generate=true` 才 `POST /messages`（Claude）或 `/responses`；Claude 带 `anthropic-version: 2023-06-01` 与 `x-api-key`，其他用 bearer；响应读上限 2MB，模型目录 ≤500 项；返回 `status/latencyMs/models/kind(generation-probe|models-probe)/message`（状态码文案含 401/403/404/429/0）：`rust-cli/src/local/network.rs:14-80`。
- `inspect`：`cliproxy` → `GET /v0/management/auth-files`，仅取 `files[].{name,provider,status,disabled}`；`openai|litellm` → `GET /models`，仅取 `data[].{id,owned_by}`；非 2xx 直接报错，各上限 500 项，返回 `note="只读连接：不切换账号、不下载认证文件、不修改代理配置。"`：`rust-cli/src/local/network.rs:81-115`。
- **渠道质量指标不在本文件**：`network.rs` 只产单次事件；P50/P95、错误率、429、重试统计由前端在内存中计算（`rust-cli/local-web/app.js:675-703`）。问题清单里"channel quality 指标计算"的位置即此处，勿在 Rust 侧寻找。
- 密钥来源：只接受**环境变量名**，运行时从服务进程环境读取：`rust-cli/src/local/store.rs:185-196`；未设置时报错提示重启服务。

### 7.8 `store.rs` / `alerts.rs`：落盘与提醒【已核实】
- 落盘文件（均在 `Paths::data`，默认 `config_dir()/local`）：`settings.json`（`rust-cli/src/local/store.rs:133-135`）、`activity.json`（`rust-cli/src/local/mod.rs:43-46`）、`events.json`（`:218-221`、`:292-295`）、`alerts.json`（`:101-104`）、`quota-history.json` 与 `quota-history.lock`（`rust-cli/src/local/quota_history.rs:89`、`:299`）、`backups/<id>.json`（`rust-cli/src/local/configure.rs:143-146`）。
- 写入方式：`private_write` = 建父目录 → Unix 0700 → 临时文件 → `sync_all` → 原子 `persist`：`rust-cli/src/local/store.rs:15-28`；读取缺失返回默认值：`:29-36`。
- 提醒规则（`alerts::conditions`）：
  - 月度成本：`activity.daily` 中当月前缀求和 ≥ `prefs.monthly_budget`（需 >0）：`rust-cli/src/local/alerts.rs:41-49`；
  - 费用突增：过去 7 天日均 ≥ $1 且今日 > 日均 × `spike_factor`：`:20-39`；
  - 额度阈值：跳过 `status=="stale"`、`updatedAt` 超出 `[-2, +15]` 分钟、`source` 含 `estimate` 或等于 `codexbar:local`、`resetsAt` 已过期的窗口；`usedPercent ≥ quota_threshold` 时生成键 `quota:{provider}:{accountKey}:{limitId}:{windowName}:{resetsAt}`：`:50-98`；
  - 项目预算：按 `projectId` 汇总 `monthCost` ≥ 项目预算：`:100-119`。
- 冷却与恢复语义：`cooldown_minutes` 内同键不重复；条件消失写 `state="changed"`，文案明确"这不代表额度已恢复"：`rust-cli/src/local/alerts.rs:144-178`；历史上限 300 条：`:175-177`。
- 通知实现：macOS `osascript`、Linux `notify-send`、Windows PowerShell `NotifyIcon`（5 秒后销毁），仅 `prefs.notifications == true` 时发送：`rust-cli/src/local/alerts.rs:122-143`、`:171`。

### 7.9 前端聚合、配额渲染与预测【已核实】
- 前端不重算成本：全部来自 `/api/state`，`money()` 仅格式化：`rust-cli/local-web/app.js:12`；金额展示点在概览卡 `:534`、最近会话 `:548`、项目表 `:638-674`（`projectTable` 直接累加 `s.cost`/`s.monthCost`）、会话与请求弹窗 `:829`/`:839`（`:515-517` 的 `metrics()` 是通用卡片 helper，不参与金额计算）；会话下钻只是 `requests.filter(sessionId)`：`:833-841`。
- 时间范围：概览 7 天柱状图与本月均为前端按**本地日期**（`toLocaleDateString("sv-SE")`）过滤后端的 `daily` 映射：`rust-cli/local-web/app.js:519-532`。
- 配额新鲜度：连接 `fresh` 要求 `lastSuccessAt` 落在 `[-2min, 15min]`：`rust-cli/local-web/app.js:102-109`；单条额度 `quotaIsCurrent` 还要求 `status` 非 `stale/unavailable` 且至少一个窗口的 `usedPercent` 有效且 `resetsAt` 未过：`:118-130`；卡片据此在"当前额度 / 旧快照 / 附加来源"间切换：`:368-382`。
- 周期历史：取 `quotaHistory.cycles` 前 200 条，显示端点用 `cycleEndAt`（`closedAt` 与 `resetsAt` 取早）：`rust-cli/local-web/app.js:422-437`、`:136-151`；周期明细弹窗调用 `/api/quota/cycle` 并同屏渲染预测：`:842-852`。
- 当前周期的匹配条件：账号 + 额度类别 + 窗口名 + 窗口时长**完全相同**且 `resetsAt` **精确相等**、周期未关闭：`rust-cli/local-web/app.js:157-177`（注意：这里**没有**云端使用的 5 秒容差）。
- 预测算法（`forecastQuotaCycle`，版本 `stable-linear-regression-v1`）：
  - 常量：`MIN_RATE=0.001`、`STALE_MS=15min`、`MIN_READY_SPAN_MS=10min`、`ROUNDING_TOLERANCE=1pp`、`RAPID_JUMP_POINTS=5pp/10min`：`rust-cli/local-web/quota-forecast.js:1-6`。
  - 窗口推导：`start = resetsAt − windowMinutes`（`segment>0` 时取 `max(nominalStartAt, firstObservedAt)`），`end = min(resetsAt, closedAt)`：`:102-124`。
  - 样本清洗：时刻必须落在 `[start, end)`、不晚于 now，百分比 0..100，同刻去重后按时间排序：`:126-144`。
  - 稳定段：从**最后一次下降**（变化 < −1pp）之后开始：`:159-166`。
  - 新鲜度：最新样本距今 > 15 分钟 → `isStale`，并跳过投影：`:168-169`、`:196`、`:246`。
  - 节奏对比（pace）：`expected = 周期进度 × 100`，`delta = 实际 − expected`，分级阈值 ∓2 / ∓6：`:170-187`、`:91-97`。
  - 近段窗口：`horizon = max(15min, min(24h, 窗口时长×0.1))`；样本 <2 时回退最后 5 个：`:202-210`；最小二乘线性回归求斜率与 R²：`:52-81`；`rate < 0.001` 视作 0；置信度 `= min(n/5,1) × R² × 跨度因子 × (突增?0.6:1)`：`:228-244`。
  - 投影：`usedPercentAtReset = latest + rate × (resetsAt − latest)/1h`；`reaches90AtMs`/`reaches100AtMs` 线性外推；`exhaustsBeforeReset`；可持续速度 `= (100 − latest) / (resetsAt − now)`；`speedRatio = rate / sustainable`：`:246-265`、`:83-89`。
  - 状态机：`expired` → `stale-snapshot`(partial) → 稳定样本 <3（low-confidence）→ 跨度 <10 分钟（short-span）→ 变化 <1pp（flat-usage/insufficient-change）→ 进度 <8%（early-window）→ 突增（rapid-jump）→ 置信度 <0.35（low-confidence）→ 否则 `ready`：`:267-294`。
- 展示映射：`state → 徽标`（ready/partial/insufficient/expired/invalid）与中文原因文案：`rust-cli/local-web/app.js:178-213`；趋势 SVG 与"均匀消耗参考"虚线：`:257-322`；指标卡（近段速度 / 重置时已用 / 耗尽时间 / 可持续速度 / 趋势评分）：`:323-344`。
- 本地导出通道：`export-sessions` 生成带 BOM 的 UTF-8 CSV（`"\uFEFF"` 前缀、CRLF 行分隔、文件名 `usagemesh-sessions.csv`、`text/csv;charset=utf-8`）：`rust-cli/local-web/app.js:854-873`；`csvCell` 对以 `= + - @ \t \r` 开头的单元格前置 `'` 中和公式注入：`rust-cli/local-web/app.js:746-750`；`export-events` 直接导出 `state.events` 的 JSON：`:874-878`。
- 类型契约：`rust-cli/local-web/quota-forecast.d.ts:1-76`（`QuotaForecastState`、`QuotaForecastReason`、完整字段）。

与 `docs/QUOTA-FORECAST.md` 的一致性核对：
- 一致：样本下降后重算速度、`ready` 才提示耗尽时间 → `docs/QUOTA-FORECAST.md:46`；15 分钟新鲜度门槛 → 同文档 `:9`（"超过 15 分钟的快照只显示末次观测"）与 `:21`（"即使观测时间仍在 15 分钟内，也不当作最新额度"）；两侧实现见 `rust-cli/local-web/quota-forecast.js:2`、`:159-166`、`:246-265`。
- **范围错位（需在文档中澄清）**：`docs/QUOTA-FORECAST.md` 描述的是**云端面板**（订阅用量/周期历史、金额公式、5 秒漂移合并、每页 12 条），不是本地工作台；本地侧对应文档是 `docs/LOCAL-WORKSPACE.md:79-94`，后者只承诺"本地、云端共用算法"。
- 已验证该承诺：云端 `web-ui/src/views/QuotaCycles.tsx:39` 与测试 `web-ui/tests/quotaCycles.test.ts:19` **直接 import** `rust-cli/local-web/quota-forecast.js`，即同一份实现。
- **本地没有的东西**（云端独有）：金额换算 `forecastCycleCost`（已用 API 等价金额 ÷ 已用百分点 × 100，含"账本需覆盖观测前最后一个完整分钟"的同步门槛）：`web-ui/src/lib/quotaCycles.ts:229-281`、`:221-227`；周期归并的 5 秒容差与"替换/回填"逻辑：`web-ui/src/lib/subscriptionPeriods.ts:3`、`:151-211`。

### 7.10 配额与预测算法

采样到投影的完整链路（本地）：
1. `codex app-server --stdio` RPC 取账号、`rateLimitsByLimitId`、可选 `account/usage/read`：`rust-cli/src/local/quota_official.rs:274-362`。
2. 包裹为 `{source:"codex-app-server", accountKey:sha256(...), limitId, windows[...]}`：`rust-cli/src/local/quota_official.rs:517-593`。
3. `History::record` 校验窗口、按 (account, limit, window) 归并周期、0% 不入史、下降或重置变化分割周期并标注关闭原因：`rust-cli/src/local/quota_history.rs:111-284`。
4. 失败时置 `stale` 并保留旧观测（见结论 3 的条件列表）。
5. 前端把 `cycles` 与 `latest` 关联出开放周期，交给 `forecastQuotaCycle` 得速度/进度/投影：`rust-cli/local-web/app.js:157-177`、`:345-350`。
6. 周期明细附"同期本机官方订阅记录"（`scope:"local-official-subscription-unattributed"`，只统计 codex + `official-subscription` + `official`，左闭右开）：`rust-cli/src/local/quota_history.rs:304-346`。

方法论要点（写入文档时保留）：
- 百分比是**快照**，任何跨窗口/跨周期相加都无意义（`rust-cli/src/local/quota_history.rs:1` 的模块注释即此约束）。
- 本地预测**只用百分比**，不做 token→额度换算；官方日 Tokens 与价卡估算费用是并列的第三个指标，不混算（`docs/LOCAL-WORKSPACE.md:87-89`）。
- 周期起点是"重置时间 − 窗口时长"的推算值，不是服务端证明的真实起点：`rust-cli/src/local/quota_history.rs:258`、`rust-cli/local-web/app.js:436`。
- 0% 滑动窗口无稳定边界，故只留在 `latest`：`rust-cli/src/local/quota_history.rs:111-117`。

### 7.11 与云端口径差异

| 维度 | 本地工作区（`serve`） | 云端仪表盘（`web-ui` + 加密账本） | 证据 |
|---|---|---|---|
| 数据来源 | 单机现场扫描：Tokscale 90 天 + `evidence::scan(false)` 路由 + 即时价卡 | 各设备 `collector::collect` 生成的加密账本快照，GitHub 分支分发、前端解密聚合 | `rust-cli/src/local/activity.rs:9-27`；`rust-cli/src/main.rs:375-407`；`rust-cli/src/collector.rs:204-373` |
| 时间桶 | `daily` 按**自然日**（本地日期字符串键）+ 概览 7 天；请求粒度到毫秒 | 账本行以**分钟桶**（`timestampMs` + 60_000）参与周期汇总；完全落在窗口内的桶计入、相交桶单列（已核实，见 §6.5） | `rust-cli/src/local/activity.rs:86`；`rust-cli/local-web/app.js:519-532`；`web-ui/src/lib/quotaCycles.ts:196-203` |
| 成本来源 | 扫描时即时 `quote_on_date(..., "standard", ...)`，未知=未知 + `lowerBound` | 账本内 `row.cost`（采集时写入）+ 官方金额换算 `forecastCycleCost`（÷已用百分点×100） | `rust-cli/src/local/activity.rs:63-64`；`web-ui/src/lib/quotaCycles.ts:229-281` |
| 配额窗口 | 每设备本地观测；周期 id 含 `segment`；`resetsAt` 匹配要求**精确相等** | 多设备按账号/额度/窗口/时长家族合并，5 秒容差，"旧副本 stale 优先"，替换关系重建为周期内变更 | `rust-cli/src/local/quota_history.rs:242-248`；`rust-cli/local-web/app.js:157-177`；`web-ui/src/lib/quotaCycles.ts:306-434`；`web-ui/src/lib/subscriptionPeriods.ts:3` |
| 周期历史 | 未到期周期也在列表（标注"持续观测中"），最多 200 条 | 只收录已到期且曾非零的周期（`isElapsedSubscriptionPeriod`） | `rust-cli/local-web/app.js:131-151`、`:422-437`；`web-ui/src/lib/subscriptionPeriods.ts:239-249` |
| 官方日 Tokens | 直接读 `officialUsage`，缺失不补 0 | 多设备按 `accountKey` 合并日桶（同日期后者覆盖），缺失保持 null | `rust-cli/src/local/quota_official.rs:659-699`；`web-ui/src/lib/quotaCycles.ts:410-432` |
| 请求明细 | 最近 20000 条、90 天，服务端截断 | 账本 `row` 全量 + `trim_request_details` 受控裁剪，前端分页/检索 | `rust-cli/src/local/activity.rs:89-93`；`rust-cli/src/collector.rs:399` |
| 设备维度 | 无 | `deviceCount`/`expectedDevices`，可判部分覆盖 | `rust-cli/local-web/app.js:638-674`；`web-ui/src/views/QuotaCycles.tsx:363-383` |
| 预测算法 | `forecastQuotaCycle`（本地模块） | **同一模块**（直接 import） | `rust-cli/local-web/quota-forecast.js:99`；`web-ui/src/views/QuotaCycles.tsx:39` |
| 敏感数据边界 | 账号标识仅哈希、密钥只在进程环境/Claude 本机配置 | 账本上传前整体 AES-256-GCM 加密；`quota_update_for_sync` 以语义快照比较决定是否复用旧账本：先移除 `latest[].updatedAt`、`usedPercent == 0` 窗口的 `resetsAt`、cycles 的 `lastObservedAt`/`samples`、`officialUsage.updatedAt`，且仅当旧 `latest` 全部观测落在 **600 秒**内且语义快照相等时才沿用旧值（"语义相等"含**排序归一**：比较前 `latest` 按 `accountKey:limitId`（`rust-cli/src/collector.rs:426`）、`cycles` 按 `id`（`rust-cli/src/collector.rs:435`）排序后再比，故设备间数组顺序差异不影响复用判定） | `rust-cli/src/local/quota_official.rs:453-460`；`rust-cli/src/main.rs:448`；`rust-cli/src/collector.rs:405-459`（字段移除点 `:414`、`:418-421`、`:431-432`、`:437-439`，600 秒门槛 `:442-453`） |

一句话总结：**本地是"现场单机快照 + 百分比预测"，云端是"跨设备加密账本 + 分钟桶汇总 + 百分比金额换算"；两者共享同一预测实现与同一账号/额度身份哈希，但周期归并容差、到期口径、金额换算三者只在云端存在。**

### 7.12 未验证项

1. 【提交级证据缺失】克隆为浅克隆（`rev-list --count HEAD = 1`，`.git/shallow` 存在），无法用 diff 证明 HEAD 提交 `fix(quota): retain official observations during refresh failures` 的具体改动；文中"刷新失败保留官方观测"的实现位置（`rust-cli/src/local/quota_history.rs:70-86`、`rust-cli/src/local/mod.rs:302-312`、`:355-370`）是**当前工作树**证据。
2. 【未运行】未执行 `cargo test`/`cargo build`，也未实机运行 `usagemesh serve`；所有行为结论来自静态阅读，未经运行时验证（例如自动扫描节流的实际表现、`codex app-server` 真实响应字段）。
3. 【跨模块边界】`tokscale_core::parse_local_clients` 的字段语义与去重规则详见 §3，本节只记录调用方式与参数（`rust-cli/src/local/activity.rs:12-21`）。
4. 【外部依赖行为】`codex app-server` 的 `account/read`、`rateLimitsByLimitId`、`account/usage/read` 真实字段形态未实机验证；`limits_account_id` 与 `account/read` 的交叉校验依赖服务端返回，未确认真机是否总能通过。
5. 【远端实现细节】云端 `data.ts` 的账本解析、`QuotaCapacityScenario`（Tokens 容量情景）、`Guide.tsx` 中"周期用量"文案未逐一读取；本章的云端结论限定于 `quotaCycles.ts` / `subscriptionPeriods.ts` / `subscriptions.ts` / `views/QuotaCycles.tsx`（详见 §6）。
6. 【统计意义】前端渠道质量 P50/P95 取位法（`Math.ceil(n*p)-1`）在小样本（n<20）下的稳定性未验证：`rust-cli/local-web/app.js:696-697`。
7. 【未核对】`docs/LOCAL-WORKSPACE.md:83` 所述"保留最近 90 天"用的是滑动窗口删除（`rust-cli/src/local/quota_history.rs:274-278`），但 `MAX_CYCLES=400` 与 90 天谁先触发取决于采样频率，未做数值推演。
8. 【未验证的引用】`docs/QUOTA-FORECAST.md` 引用的外部项目（CodexBar、onWatch、ccusage）只作交互参考，其内容未核验。
9. 【范围外，规则已在 §6.5 取证】`web-ui/src/lib/data.ts` 的 `sanitizeOfficialQuota` 完整校验规则见 §6.5（已取证，本章不重复）；云端每页 12 条的展示分页未取证。

## 8. 同步、加密、调度与自动更新

### 8.1 `crypto.rs`：密钥、KDF 与信封格式

- 工作区密钥：`generate_key()` = `OsRng` 32 字节 → `URL_SAFE_NO_PAD` base64url（`crypto.rs:16-20`）；`decode_key()` 强制解码后恰为 32 字节（`crypto.rs:22-32`）。生成点：`config::new_config` 的 `dashboard_key`（`config.rs:115`）；`join` 时直接复用携带的密钥（`config.rs:139`）。
- 设备指纹：`device_hash()` = `Sha256(device_id)` 的**前 16 个 hex 字符**（`crypto.rs:34-38`），直接进分支名。
- 账本加密（`crypto.rs:40-66`）：AES-256-GCM，12 字节随机 nonce（`crypto.rs:43-44`），AAD = `"usagemesh-ledger-v2:" + device_hash`（前缀常量 `crypto.rs:12`，拼装 `crypto.rs:46`），明文 = `serde_json::to_vec(ledger)`（`crypto.rs:47`），`nonce`/`ciphertext` 均 base64url 无填充（`crypto.rs:63-64`），信封 `kind="usagemesh-encrypted-ledger"`、`algorithm="AES-256-GCM"`、`device_hash`、`updated_at = ledger.generated_at`（`crypto.rs:57-65`）。
- 解密（`crypto.rs:68-87`）：校验 nonce 长度必须为 12（`crypto.rs:72-74`），用信封里的 `device_hash` 重建 AAD（`crypto.rs:76`），认证失败即报 `ledger authentication failed`（`crypto.rs:85`）。因此**分支名、信封 `device_hash`、AAD、密钥四者绑死**。
- dashboard 清单（`crypto.rs:98-136`）：`ACCESS_PBKDF2_ITERATIONS = 600_000`（`crypto.rs:14`）；`PBKDF2-HMAC-SHA256`（`crypto.rs:91,128`）；16 字节随机 salt（`crypto.rs:108`）、12 字节 nonce（`crypto.rs:109`）；AAD = 前缀 `"usagemesh-dashboard-access-v1:"` + `repo.trim()` 后**仅 ASCII 转小写**（前缀 `crypto.rs:13`；封装侧 `crypto.rs:114`、解包侧 `crypto.rs:163`）。前端为模板串 `ACCESS_AAD_PREFIX + repo.toLowerCase()`（`data.ts:544`，无 trim、Unicode 小写）——GitHub owner/repo 限定 ASCII 且无空白，实际等价，但两边不是同一函数；明文即 base64url 的工作区密钥本身（`crypto.rs:119`）；信封 `schema_version=1`、`kind="usagemesh-dashboard-access"`、`iterations` 显式落盘（`crypto.rs:125-135`）。
- 新旧兼容：解包接受 `iterations ∈ [100_000, 5_000_000]`（`crypto.rs:150-154`），显式保留旧 310k 清单可用（注释 `crypto.rs:150-151`；回归测试 `crypto.rs:238-272`）；新密码写 600k。前端同规则：`web-ui/src/lib/data.ts:510-516`（同一区间）与 `data.ts:518-536`（WebCrypto `deriveKey`）。
- 解包侧另有两道结构与长度校验：salt 必须恰 16 字节、nonce 恰 12 字节（`crypto.rs:155-159`），解密出的明文再经 `decode_key` 强制 32 字节（`crypto.rs:173-175`）。
- 密码策略：≥12 字节、≤256 字节（`main.rs:139-148`）；`wrap_dashboard_key` 二次强制 ≥12 字节（`crypto.rs:104-106`）；环境变量 `USAGEMESH_DASHBOARD_PASSWORD` 可绕过交互（`main.rs:154-158`）。
- pair code 内容（`config.rs:28-38, 154-162`）：`{version:2, repo, dashboard_key, interval_seconds, interval_minutes}` 的 JSON **base64url 明文编码**（非加密）；`from_join` 校验 `version==2` 且密钥可解码（`config.rs:123-131`）。【未验证】把它贴在公开渠道等于泄露工作区密钥。

### 8.2 `github.rs`：分支命名、覆盖提交、心跳与发布去重

- 常量（`github.rs:15-19`）：`UPSTREAM_REPO = "Atingaii/UsageMesh"`、`DASHBOARD_BRANCH = "um-dashboard"`、`DEVICE_INDEX_BRANCH = "um-index"`、`LEDGER_BRANCH_PREFIX = "um-ledger-"`（私有常量）、`PRESENCE_BRANCH_PREFIX = "um-presence-"`。
- 分支名派生：`snapshot_branch = um-ledger-{device_hash}`（`github.rs:236-238`）、`presence_branch = um-presence-{device_hash}`（`github.rs:240-242`）——同一 hash 后缀，互不重叠（测试 `github.rs:444-455`）。
- 快照写入 `replace_root_snapshot`（`github.rs:244-314`）：`POST git/blobs`（`github.rs:251-260`）→ `POST git/trees` 只含**一个根文件**（`github.rs:261-272`，`mode=100644`）→ `POST git/commits` 且 **`parents: []`**（`github.rs:273-282`，即每次提交都是无父的孤立提交）→ `GET git/ref/heads/{branch}`：404 则创建 ref（`github.rs:286-298`），否则读取当前 sha，**仅当不同才 `PATCH ... {"force": true}`**（`github.rs:299-312`）。
- 提交/替换条件：`parents` 恒为空（`github.rs:278`）、`tree` 与 `message` 由内容决定，作者预期内容相同的再次发布会产生相同 commit sha 并跳过 ref 更新（`github.rs:301-311`）——这是第一层发布去重；但请求体只有 `message`/`tree`/`parents`，未指定 `author`/`committer`（`github.rs:273-282`），"相同 (tree, message, parents) → 相同 sha"属【未验证】假设（GitHub 是否在缺省时注入当前时间未知）。且三种载荷都含每次变化的发布时间戳（`index.json`/`presence.json` 的 `updatedAt`，`github.rs:356,389`；`ledger.json` 信封 `updated_at` = `ledger.generated_at`，`crypto.rs:61`、`collector.rs:365`），实际运行中触发条件极窄；即便假设不成立，行为仅退化为多一次幂等 PATCH。`message` 里含 `device_hash`（`github.rs:373`）。
- 设备索引 `refresh_dashboard_index`（`github.rs:351-365`）：`GET git/matching-refs/heads/um-ledger-` 列表（`github.rs:331-345`）→ 归一化只保留 `um-ledger-` 前缀并排序去重（`github.rs:113-126`）→ 写 `um-index/index.json`（`kind="usagemesh-device-index"`，`schema_version=1`）。注释明确"只含分支名，浏览器不必花匿名 REST 配额"（`github.rs:347-350`）。
- presence 心跳 `replace_presence`（`github.rs:382-399`）：写 `um-presence-{hash}/presence.json`，除信封字段 `schema_version=1`、`kind="usagemesh-device-presence"` 外，内容仅 `device_hash`、`updated_at`、`app_version`（`github.rs:70-78,385-391`）；与账本分离的理由写在注释（`github.rs:378-381`）——"设备活着"与"用量变化"解耦，避免为心跳重写大文件。
- 心跳与账本的关系（`main.rs:264-280, 429-455`）：`PRESENCE_INTERVAL = 60s`（`main.rs:205`），由 `<config_dir>/presence.stamp` 的 mtime 判定是否到点（`main.rs:248-262`）；**两条分支上都会调用**（会计无变化的分支 `main.rs:437`、有变化的分支 `main.rs:455`）。
- fork 上游同步 `sync_main_with_upstream`（`github.rs:215-234`）：自身即上游时直接返回；否则 `POST /repos/{repo}/merge-upstream {"branch":"main"}`，409 冲突时给出"手动解决后重跑 `sync --full`"的错误（`github.rs:227-231`）。调用点：`sync --full`（`main.rs:417-427`）与自更新前（`update.rs:382-384`）。**只动 main，不碰 `um-*` 数据分支**（注释 `github.rs:210-214`）。
- fork 发现/创建 `ensure_user_fork`（`github.rs:128-175`）：`GET /user` → 期望 `{login}/usagemesh`；已存在则要求是上游 fork 且 public（`github.rs:139-146`）；不存在则 `POST /repos/{UPSTREAM_REPO}/forks {"default_branch_only": true}`（`github.rs:152-162`），轮询 20 次 × 750ms（`github.rs:164-173`）。`validate` 同样拒绝 private 仓库（`github.rs:197-206`）。
- 发布去重语义（会计侧）：
  - `same_accounting`（`collector.rs:461-469`）：比较 `schema_version`、`device.id`、`rows`、`requests`、`totals`、`pricing`、`official_quota` **全等**，**不含** `generated_at` 与 `scan_ms`（测试 `collector.rs:591-609` 显式断言改 `generated_at`/`scan_ms` 仍算"无变化"）。
  - `quota_update_for_sync`（`collector.rs:405-459`）：对 `latest`/`cycles`/`officialUsage` 做**语义归一**（去掉 `updatedAt`/`lastObservedAt`/`samples`；`usedPercent==0` 的窗口去掉 `resetsAt`；`latest` 按 `accountKey:limitId`、`cycles` 按 `id` 排序，`collector.rs:409-441`）；仅当上一份 `latest` 全部条目 `updatedAt` 落在 **过去 0~600 秒**（`collector.rs:442-453`）且语义相同，才复用上一份值（避免"心跳式"无意义刷新），否则提交新值（`collector.rs:454-458`）。
  - 是否上传的判定（`main.rs:408-414, 429-454`）：`pending_upload || !accounting_unchanged || version/schema/pricing 迁移` 时置 `sync-pending` 标记（`main.rs:412-414`），本地缓存 `ledger-cache.json` 则在**同一流程中无条件重写**（`main.rs:415`）——即未上传时本地缓存也已前进；随后若"会计无变化且无待上传且无迁移"，只（在 `--full` 时）刷索引 + 发心跳后返回（`main.rs:429-446`）；否则加密 → `replace_snapshot` → `refresh_dashboard_index` → **两次远端写都成功才** `clear_sync_pending`（注释 `main.rs:453`）。
  - 本地标记文件：`sync-pending`（`config.rs:227-248`）；本地缓存 `ledger-cache.json`（`config.rs:48-50, 219-225`）；Unix 下 `0600` 权限与临时文件原子替换（`config.rs:250-276`）。

### 8.3 `scheduler.rs`：常驻循环与 OS 监督

- 常量：`SYNC_INTERVAL_SECONDS = 30`（`scheduler.rs:7`，测试 `scheduler.rs:694-697`）；`SCHEDULER_REVISION = 5`（`scheduler.rs:8`，测试 `scheduler.rs:699-702`）。
- Windows（`scheduler.rs:226-265`）：把 `resident-agent.ps1` 写入 `config_dir()`，内容为 `while($true){ & 'usagemesh.exe' sync --quiet ; Start-Sleep -Seconds 30 }`（`scheduler.rs:232-237`）；注册计划任务 `UsageMeshResidentAgent`（`scheduler.rs:11`）以 **1 分钟 MINUTE 触发器当看门狗**（注释 `scheduler.rs:243-246`），并立即 `/Run`（`scheduler.rs:261-263`）。
- macOS（`scheduler.rs:388-419`）：`~/Library/LaunchAgents/io.atingaii.usagemesh.agent.plist`（`scheduler.rs:303-307`），`ProgramArguments=[runner,--run]`，`RunAtLoad=true`、`KeepAlive=true`、`ThrottleInterval=5`、`ProcessType=Background`（`scheduler.rs:395-407`），并把 `HTTP(S)_PROXY/NO_PROXY` 等 8 个变量转义后写入 `EnvironmentVariables`（`scheduler.rs:327-366,394`）；未加载才 `launchctl bootstrap gui/{uid}`（`scheduler.rs:413-418`）。
- Linux（`scheduler.rs:623-640`）：优先 `systemd --user` 服务 `usagemesh-agent.service`，`Type=simple`、`Restart=always`、`RestartSec=5s`、`Nice=10`、`IOSchedulingClass=idle`、`WantedBy=default.target`（`scheduler.rs:515-519`），`daemon-reload` → `loginctl enable-linger` 尝试（`scheduler.rs:493-507,521-529`）；systemd 路径失败则回退 cron：重写 crontab 时保留其他行，插入 `@reboot ... --ensure` 与 `* * * * * ... --ensure` 两行，同时删除含 `# usagemesh-usage-sync` 的旧行（`scheduler.rs:548,617`；实现 `scheduler.rs:534-575`），并立即执行一次 `--ensure`（`scheduler.rs:618`）。
- 单实例与并发保护（三层）：
  - Unix runner 脚本用 pid 文件 `resident-agent.pid`：`run_loop` 启动前检查 `kill -0`，已在运行则 `exit 0`（`scheduler.rs:173-181`）；`ensure_loop` 用 `nohup "$0" --run &` 拉起（`scheduler.rs:190-199`）；`trap` 清理 pid（`scheduler.rs:182`）。
  - Windows 依赖 Task Scheduler 的单实例语义 + 常驻进程存活即忽略触发（注释 `scheduler.rs:243-246`）。
  - 业务层 `sync.lock`（`main.rs:207-242`）：`create_new` 独占创建，交互式等待、常驻式为非阻塞（`main.rs:337-349`）。
- 失败处理：Unix runner 对 `sync` 用 `|| true` 吞掉失败（`scheduler.rs:184`）；Windows PowerShell `try {} catch {}` + `$ErrorActionPreference='Continue'`（`scheduler.rs:233`）；macOS/Linux 由 `KeepAlive` / `Restart=always` 拉起（`scheduler.rs:404`、`scheduler.rs:516`）。
- 旧调度退场 `cleanup_legacy`（`scheduler.rs:47-59`）：Windows 删 `UsageMeshUsageSync`（`scheduler.rs:267-281`）、macOS `bootout` 旧 plist（`scheduler.rs:442-455`）、Linux 停用旧 timer/service 并只清理旧 cron 注释行（`scheduler.rs:642-657`）；新旧原生名不同是刻意设计（注释 `scheduler.rs:47-51`，测试 `scheduler.rs:704-714`），并且在 `revision` 持久化之后才清理（`main.rs:308-335`）。
- 状态探测 `is_installed`（`scheduler.rs:87-127`）：Windows 查任务名、macOS 查 plist 文件、Linux 查 systemd 单元文件或 crontab 中 `# usagemesh-resident-agent` / `# usagemesh-usage-sync` 标记。

### 8.4 `update.rs`：自更新检测、校验、替换与迁移重扫

- 触发：每次 `sync` 开头调用 `update::maybe_auto_update(&config, full, quiet)`（`main.rs:289-297`）；`USAGEMESH_AUTO_UPDATE ∈ {0,false,off,no}` 时禁用（`update.rs:69-79`）。
- 检测：`GET /repos/{UPSTREAM_REPO}/releases/latest`（`update.rs:119-140`），draft/prerelease 直接报错；版本比较为去 `v` 前缀的**分段数字比较**（`update.rs:81-113`，测试 `update.rs:427-434`）。
- 并发保护：`update.lock`（`update.rs:29-54`），陈旧阈值 15 分钟（`update.rs:17`）；拿不到锁即按"当前"返回，不阻塞正常同步（注释 `update.rs:373-375`）。陈旧抢占只尝试一次：`for attempt in 0..2`（`update.rs:31`），仅 `attempt == 0` 时删锁重试（`update.rs:44-46`），第二次仍冲突即放弃并返回 `None`（`update.rs:48`）。
- 更新前同步 fork main（`update.rs:380-384`），保证 Pages 工作流是最新的。
- 资产与校验：按 OS/arch 拼 `usagemesh-{os}-{arch}.{zip|tar.gz}` 与**同一 stem** 的 `usagemesh-{os}-{arch}.sha256`（去掉归档扩展名，不存在 `.tar.gz.sha256`；`update.rs:142-175,386-392`），下载到 `config_dir()/updates/{版本}-{pid}`（`update.rs:394-404`）；`verify_checksum` 取校验文件首个空白分隔 token，要求 **64 位十六进制**，与本地 `Sha256::digest` 十六进制比对（`update.rs:191-208`）。
- 解压与预执行校验：Windows 用 PowerShell `Expand-Archive`（`update.rs:212-234`），其它用 `tar -xzf`（`update.rs:235-248`）；随后 `--version` 输出必须包含目标版本号（`update.rs:251-272`）。
- 替换与重启：Windows 把新二进制拷成相邻的 `*.update.exe`，用 PowerShell 循环等父进程退出 → `Move-Item -Force` 覆盖 → 设 `USAGEMESH_UPDATE_RESUME=1` 并执行 `sync [--full] [--quiet]`（`update.rs:291-336`）；Unix 直接 `rename` 原子覆盖后同步执行（`update.rs:339-358`）。返回 `Restarted`，本轮 `sync` 立即结束（`main.rs:290`）；更新目录随后清除（`update.rs:419`）。
- 迁移重扫的落点：新进程带 `USAGEMESH_UPDATE_RESUME`，因此 `wait_for_lock=true`（`main.rs:340`）；`app_version` 与缓存账本不一致 → `version_migration` → `effective_full` → `collect(..., None)` 全量重建（`main.rs:361-373, 389`），并在输出里打印 `Version migration: ...`（`main.rs:471-473`）。

## 9. 与 TokenScope 的对照

TokenScope 与本项目同属「读本地日志 → 统一事件 → 聚合 → 计价展示」赛道，但约束不同：TokenScope 追求
**多来源独立候选计价 + 只读原则 + 单一桌面 GUI**，UsageMesh 追求**零服务器跨设备 + 端侧计价 + 静态站点**。
下表只列源码可对照的差异，供借鉴与规避。

| 维度 | UsageMesh（本文取证） | TokenScope（本仓 `CLAUDE.md` / `docs/stats-semantics.md` 口径） | 借鉴判断 |
| --- | --- | --- | --- |
| 事件来源 | 内嵌 tokscale-core v4.14.0，一次调用扫全部客户端（`rust-cli/src/collector.rs:211-221`） | 自研适配器（Claude Code、Codex），层间只经 `UsageEvent` | 借鉴：统一事件 + 单入口扫描；差异：TokenScope 不引入第三方扫描器 |
| 采集身份 | `(client, session_id)` + 行级去重键，明细只留最近 1000 条（`collector.rs:50-67`） | 原生身份 `(session_id, message.id)`、`(session_id, 原始模型, 四桶)`，历史库保留全部用量 | 规避：UsageMesh 明细裁剪会丢「逐请求」可追溯性 |
| 聚合粒度 | 十维度 RowKey + **分钟桶**（`collector.rs:16-28`、`:46-48`） | 事件级明细 + 四组互斥时间参数（自然日预设 `days` ／ 自然日区间 `from`+`to` ／ 24h 滚动窗口 `rolling_hours` ／ 自定义时刻 `start_at`+…），展示端聚合 | 借鉴：分钟桶让自定义时间范围与明细同源 |
| 跨设备 | 每设备一分支、浏览器求和（`web-ui/src/lib/data.ts`） | 本机单库 | 借鉴：账本即事实源，展示端不重算 |
| 计价时点 | **端侧逐条计价**，写进加密账本；浏览器不重算（`docs/PRICING.md:13`、`web-ui/src/lib/data.ts:15-39`） | 查询期按三来源候选重算，等价钱键取最高费用 | 借鉴：计价一次、展示不漂移；规避：端侧计价无法在价格更新后回溯 |
| 计价来源 | 内置 GPT-5.6 兼容卡片 + models.dev 目录（`pricing.rs:22-29,369-397`） | 外置 `pricing.toml` + models.dev + OpenRouter 三来源候选 | 差异明确：TokenScope 显式拒绝「来源等级无条件覆盖」 |
| 未收录模型 | 记为**下界**（`lower_bound=true`，`pricing.rs:315-320,350-353`），展示端区分（`data.ts:26-28`） | 未收录模型保持「未知」，不得按 0 静默吞掉 | 同构：都必须显式暴露不确定 |
| 路由身份 | 八类 `route_type`（含 `unknown`）+ 四类 `billing_channel`，需本地证据（`provider.rs:229-373`、`rust-cli/src/model.rs:63-65`） | 未建模路由/渠道维度 | 借鉴：把「用量」与「计费渠道」分离，官方/中转不混算 |
| 只读原则 | **不遵守**：`configure.rs` 会写 `<codex>/config.toml`、`<claude>/settings.json`（`local/store.rs:136-142`、`local/configure.rs`） | 硬约束：绝不写入 agent 目录 | 规避：TokenScope 保持只读，配置写入交给 cc-switch 等专用工具 |
| 时间口径 | 采集窗口用**本机 Local 日期**（`main.rs:364,394`），行日期桶由扫描器时区决定（`codex_tier.rs:338`） | 存储 UTC、全链路一次时区转换，`settings.toml` 是唯一权威 | 规避：两套时区来源会在跨日界产生重复/漏计（见 §10.1 的风险表 R2/R9） |
| 数据删除语义 | 快照覆盖式、无 git 历史（`github.rs:307`）；本地 `rows` 无上限累积 | 来源删除/停用/改目录/重扫都不删已保存用量 | 借鉴：用量事实与来源解耦 |
| 明细上限 | `MAX_REQUEST_DETAILS = 1000`（`collector.rs:36`） | 明细随命中请求数增长，保留预算由整合计划验证 | 差异：用途不同，UsageMesh 是"近实时流"，TokenScope 是"历史账" |
| 官方配额 | 读 `codex app-server` 观测窗口 + 周期历史（`local/quota_official.rs`、`local/quota_history.rs`） | 未接入（当前只做用量与费用） | 借鉴：配额百分比**不跨设备求和**、只取最新快照（`web-ui/src/lib/quotaCycles.ts`） |
| 订阅周期 | 按官方 reset 窗口聚合，跨设备 tokens/requests/成本相加（`web-ui/src/lib/quotaCycles.ts`） | 未接入 | 借鉴：同一窗口内「加性指标相加、比率指标取最新」的分离口径 |
| 测试策略 | 前端 vitest 固化聚合断言（`web-ui/tests/*.test.ts`）、Rust 单测覆盖分组/对账门 | 合成 fixture + 定向边界测试，禁止真实日志入库 | 同构 |

## 10. 风险总览与未验证项索引

> 本章只做收口：风险条目**全部来自前文取证**，每条给出证据位置；详细论证见对应章节。第 10.2 节是设备侧/传输链的原始清单，第 10.3 节把各章「未验证项」统一索引。

### 10.1 跨章节风险总览

| # | 风险 | 影响 | 证据 |
| --- | --- | --- | --- |
| R1 | 请求明细只保留**最近 1000 条**（`MAX_REQUEST_DETAILS`），且先去重再截断；而 `rows` 无上限 | 「明细金额合计」长期小于「设备总额」；按明细求和的分析会低估 | `collector.rs:36`、`50-67`、`361`、`399` |
| R2 | 增量合并按 **`date` 字符串**切分（`row.date < since`），`since` 由**本机今天 −2 天**推导；而行日期桶由扫描器时区决定 | 时区/时钟不一致时，边界日可能重复计数或漏计 | `collector.rs:375-401`、`main.rs:364,394-401`；扫描器侧 rebucket 见 §3.6.9 |
| R3 | 扫描器的 `since/until` 是**字符串日期比较**且**不省 IO**（每次全量重解析后过滤） | 「增量」只是结果过滤；性能成本与全量相当（对 1.2 GB 级日志差异显著） | §3.7.1、§3.6.10 |
| R4 | 端侧计价：价目缓存缺失/离线时目录为空，非 GPT-5.6 模型一律 `cost_usd = 0`，仅靠 `lower_bound` 标记 | 首启离线或长期无网设备的金额近似为 0，图表上表现为「$0.00 ≥」 | `pricing.rs:242-279`、`315-320`、测试 `pricing.rs:503-511` |
| R5 | 部分客户端的 token 是**估算**（Kiro「字符数/4」、CommandCode、Freebuff 全部估算，zcode 缺 usage 时估算），Goose 的 reasoning 是差值推导 | 在估算基数上乘价得到「看起来精确」的美元数；`ParsedMessage` 无来源可信度字段可用于标注 | §3.5.11 |
| R6 | `message_count` 语义**不同质**（Grok unified 路径取值域恰为 `{0,1}` 且等于 `i32::from(loop_index == 1)`、Droid 全挂首条、Reasonix = `requests`、Fx = `request_count`、Warp = `requests_used`、Crush 按天桶） | UsageMesh 把它当「请求数」累加，跨工具的「请求数」不可直接横比 | §3.6.6、§3.5.13、`collector.rs:108` |
| R7 | 部分客户端**无 `dedup_key`**（无键名单逐 lane 枚举见 §3.5.13，**不能用「约 1/3」概括**） | 同一会话被双份转录时会计两次；有键客户端则首见保留 | §3.6.5、§3.5.13、§3.9.2 |
| R8 | 客户端覆盖与产品声称不一致：`micode` 在通道 A **不解析**；`supported_clients()` 列出 50 个（含 `parse_local: false` 的 `cursor`），且实际 `client` 可能是 `cc-mirror/<variant>` 而不在清单内 | `usagemesh clients` 的输出 ≠ 实际统计范围 | §3.3.4、§3.9.3、`collector.rs:196-202` |
| R9 | 账本时间桶依赖「机器本地时区 + 扫描器 `bucket_timezone` 未钉即不重分桶」 | 同一份历史在不同时区/不同机器上重扫会重新分桶，跨设备对比存在偏移空间 | §3.6.9、§3.2.8 之 `BucketTimezone` |
| R10 | 快照是**覆盖式**提交（`parents: []` + 强制 PATCH），无 git 历史 | 误上传无法按分支历史回滚，只能靠下一次正确快照覆盖 | `github.rs:278`、`299-311` |
| R11 | `um-presence-*` 为明文、设备指纹仅 64 bit（SHA-256 前 16 hex）且进分支名与 AAD；pair code 是含工作区密钥的**明文 base64url** | 分支名可被匿名枚举；pair code 贴在公开渠道等于泄露工作区密钥 | 结构定义 `github.rs:70-78`、写入点 `github.rs:382-399`、`crypto.rs:34-38`、`config.rs:154-162` |
| R12 | 本地缓存只在 `schema_version < CURRENT` 时判空；**更高版本**缓存会被接受并参与增量合并 | 降级运行（回退二进制）时会用未来版本账本做增量，字段语义不保证兼容 | `config.rs:209-211` |
| R13 | `same_accounting` 把 `official_quota` 计入相等性；`quota_update_for_sync` 会剥离 `updatedAt`/`lastObservedAt`/`samples`，以及 `usedPercent == 0` 窗口的 `resetsAt`（`collector.rs:418-423`）与 `officialUsage.updatedAt`（`:437-439`）再比语义，仅在上一份 `latest` 全部条目 `updatedAt` 落在过去 0–600 s 内且语义相等时才复用旧值 | 配额观测变化会触发账本重新上传（金额不变），使「上传次数」不完全等于「用量变化次数」 | `collector.rs:405-459`、`461-469` |
| R14 | 官方配额周期在本地要求 `resetsAt` **精确相等**才归并，云端则允许 `±5 s` | 同一份数据在本地工作台与云端的周期归属可能不同 | §7.11、`local-web/app.js:157-177`、`subscriptionPeriods.ts:3` |
| R15 | 跨边界分钟桶：完全落在 `[from, to)` 之外的**桶起点 ≥ `to`** 的行既不计数也不列入 boundary | 周期边界附近可能有被静默丢弃的 token（云端「周期用量」视图） | §6.5、`quotaCycles.ts:196-202` |
| R16 | 浏览器**不读账本内部 `schemaVersion`**，分钟/日粒度完全靠 `row.timestampMs > 0` 判断 | README 中「schema-v7 设备改造成分钟桶」的表述与实现不符（`README.md:189`）：其 `v7` 与代码 `CURRENT_LEDGER_SCHEMA_VERSION = 8`（`model.rs:3`）亦不一致；旧账本只表现为「无时间戳行」 | §6.3 |
| R17 | 本地工作台存在**第二条独立计价链**，且传**原始 `model_id`**（未过 `canonical_model_id`） | 同一模型在本机工作台与云端账本上的金额理论上可能不一致 | §5.8、`local/activity.rs:27,63` |
| R18 | Codex tier 增强的日级对账门被拒时**静默降级**为 canonical 行 | `Fast/Priority` 标签丢失（金额不变），tier 维度的筛选与分组出现 `Standard` 假象 | `collector.rs:177-194`、`233-236` |
| R19 | `evidence::scan` 增量按文件 mtime **3 天**过滤，而采集窗口是 **2 天** | 两个窗口不同步；陈旧文件里的路由证据可能缺席 | `evidence.rs:53-64`、`main.rs:394` |
| R20 | 本地工作台 `configure.rs` 会**写入 agent 配置**（`<codex>/config.toml`、`<claude>/settings.json`，Claude 分支写入 `ANTHROPIC_AUTH_TOKEN`） | 与「只读」原则相反；属 UsageMesh 显式设计，但对以只读为约束的工具是风险面 | `local/store.rs:136-142`、`local/configure.rs:77-99` |
| R21 | 英文 `README.md:177` 的**安全模型写反**：宣称解锁密钥只驻内存、不在 `sessionStorage`/IndexedDB、刷新必须重输密码；实现是密文信封入 `sessionStorage`（`web-ui/src/lib/session.ts:3,99,136,145,190`）+ 不可导出 `CryptoKey` 入 IndexedDB（`session.ts:30,164-168`）+ 刷新自动恢复（`session.ts:197-232`，30 min 闲置 / 12 h 绝对） | 以英文 README 为准的读者会误判浏览器的密钥持久化与恢复行为；注意中文版 `README.zh-CN.md:167` 与实现一致，即中英文档自相矛盾 | 详见 §6.1 的「文档与实现矛盾」判定 |

### 10.2 设备侧与传输链风险（原始清单）

#### 10.2.1 边界与陷阱

1. `requests` 只用最新 1000 条（`collector.rs:36,64-66,399`），`rows` 无上限：明细与汇总的时间覆盖不同，长期运行后"看明细"看不到早期请求，而"总额"仍在。
2. 增量窗口是**"今天减 2 天"而非"上次同步时间"**（`main.rs:364,394`），因此本机时钟回拨/时区改动会直接改变窗口；`stale_cache` 也以本机本地日期为准（`main.rs:365-367`）。
3. 增量边界混用字符串日期（`collector.rs:376`）与毫秒截断（`collector.rs:378`）；对 `date` 格式与扫描器时区有隐含依赖（见 §4.5 的潜在缺陷 1、2）。
4. `read_cached_ledger` 只在 `schema_version < CURRENT` 时返回 `None`（`config.rs:209-211`）：**更高版本**的缓存（降级运行、或从未来版本回退二进制）会被当成有效并继续增量合并，缺少版本上限校验。
5. `same_accounting` 把 `official_quota` 计入相等性（`collector.rs:468`），因此官方配额语义变化（见 `quota_update_for_sync` 的 10 分钟窗口 `collector.rs:442-453`）会让"会计无变化"判定失效并触发一次上传。
6. `cost_usd` 是 f64 直接累加（`model.rs:44`），无舍入与溢出策略；整数桶用饱和（`model.rs:38-43`），两者语义不同。
7. 只有 16 个 hex（64 bit）的设备指纹进分支名与 AAD（`crypto.rs:37`），分支名可被匿名枚举；presence 为明文（不含用量，`github.rs:70-78`）。
8. pair code 是明文包含工作区密钥（`config.rs:154-162`），与"密码不入命令行历史"的设计取向（`main.rs:64,84` 的提示语）形成落差。
9. 上传是"覆盖式"：台账分支每次强制 PATCH（`github.rs:307`），没有历史版本；误上传无法通过分支历史回滚，只能靠下一次正确快照覆盖。
10. 本地缓存在远端写成功之前就已落盘（`main.rs:415`），`sync-pending` 标记保证失败可重试（`config.rs:227-248`、`main.rs:412-414,454`）；但若进程在两次远端写之间被强杀，会出现"索引已更新、部分设备账本未更新"的短窗口。

#### 10.2.2 未验证项（设备侧）

1. `tokscale-core 4.14.0` 内部实现（`since` 的日粒度/时区语义、`ParsedMessage.date` 是否定长 ISO、`timestamp` 单位与去重规则）**未在本仓核实**；本仓只证明传参与消费（`collector.rs:5-8,211-221`、`Cargo.toml:33`、`Cargo.lock:1879-1881`）。§4.5 的潜在缺陷 1、2 需 tokscale 侧源码确认（该确认见 §3.6.9、§3.6.10）。
2. `evidence::scan` 的增量文件过滤是"mtime 3 天内"（`evidence.rs:53-64`）而采集窗口是"2 天"，两者不同步；是否会造成"数据仍在写入但证据缺失"未验证。
3. `PriceBook::load()` 的目录/缓存策略与 `pricing.rs` 全文未逐行核对；本次只核实 `PRICING_POLICY`（`pricing.rs:24`）与 `metadata()`（`pricing.rs:281-289`）、`quote_on_date` 未收录模型的 `lower_bound=true`（`pricing.rs:315-320`）。
4. 云端仪表盘的多设备聚合、缓存（`DashboardLoadCache`）与轮询节奏未逐行核对，仅核实解锁与单设备解密路径（`data.ts:492-649`）。
5. Linux cron 回退路径、macOS launchd 路径**未在真实系统上运行验证**，仅静态阅读（`scheduler.rs:534-575,388-419`）。
6. Windows 自更新"等父进程退出后覆盖"在父进程被杀/无权限时的残留行为未验证（`update.rs:291-336`）。
7. `Metrics.messages: i32`（`model.rs:24`）在极端量级下的溢出行为未做压力验证；`scan_ms` 有 `min(u64::MAX)` 保护（`collector.rs:370`）。
8. 本地工作区的 `quota_official` / `quota_history` 抓取与存储细节未逐行核对（仅核实调用链 `local/mod.rs:302-325,334-371`）。

### 10.3 各章未验证项索引

| 主题 | 位置 | 条目数 |
| --- | --- | --- |
| tokscale-core v4.14.0 内部语义（客户端解析细节、估算面、v4.18 差异） | §3.10 | 15 |
| 计价与路由身份（价格目录实测、PDF/文档页、跨平台目录落点） | §5.12 | 9 |
| 云端仪表盘（未跑测试、线上响应头、限流阈值、fork 回退） | §6.11 | 10 |
| 本地工作台（浅克隆限制、未实跑、app-server 实机字段） | §7.12 | 9 |
| 设备侧与传输链 | §10.2.2 | 8 |

## 附录 A：关键常量与阈值速查

| 常量 / 阈值 | 值 | 位置 | 语义 |
| --- | --- | --- | --- |
| 账本 schema | `8` | `rust-cli/src/model.rs:3` | 本地缓存/加密账本的 `schemaVersion`，不一致触发 `schema_migration` 全量重扫 |
| 加密信封 schema | `2` | `rust-cli/src/crypto.rs:58` | `um-ledger-*` 上的信封版本 |
| 密码清单 schema | `1` | `rust-cli/src/crypto.rs:126` | `um-dashboard/access.json` |
| 账本 AAD 前缀 | `usagemesh-ledger-v2:` | `rust-cli/src/crypto.rs:12` | 与设备指纹拼接 |
| 清单 AAD 前缀 | `usagemesh-dashboard-access-v1:` | `rust-cli/src/crypto.rs:13` | 与仓库名小写拼接 |
| PBKDF2 迭代（新） | `600_000` | `rust-cli/src/crypto.rs:14` | HMAC-SHA256；解包接受区间 `[100_000, 5_000_000]`（`crypto.rs:150-154`） |
| 设备指纹 | SHA-256 前 16 hex | `rust-cli/src/crypto.rs:34-38` | 分支名后缀 + 账本 AAD |
| 请求明细上限 | `1000` | `rust-cli/src/collector.rs:36` | 超出丢最旧（`collector.rs:64-66`） |
| 分钟桶 | `60_000 ms` | `rust-cli/src/collector.rs:46-48` | 行级 `timestampMs` 向下对齐 |
| 秒/毫秒判定阈值 | `100_000_000_000` | `rust-cli/src/collector.rs:38-44` | 小于此值视为秒级时间戳并 ×1000 |
| 同步周期 | `30 s` | `rust-cli/src/scheduler.rs:7` | 常驻循环间隔 |
| 调度器版本 | `5` | `rust-cli/src/scheduler.rs:8` | 变更即重装原生调度单元 |
| 心跳间隔 | `60 s` | `rust-cli/src/main.rs:205` | presence 分支写入节流 |
| 增量重叠窗口 | `2 天` | `rust-cli/src/main.rs:364,394` | `since = 本机今天 − 2 天` |
| 同步锁等待 / 重试 | `10 min` / `250 ms` | `rust-cli/src/main.rs:203-204` | 陈旧锁 `15 min` 可抢占（`main.rs:221-229`） |
| 更新锁陈旧阈值 | `15 min` | `rust-cli/src/update.rs:17` | 自更新互斥 |
| 价格来源 | `https://models.dev/api.json` | `rust-cli/src/pricing.rs:22` | 唯一通用目录源 |
| 价格缓存 TTL | `24 h` | `rust-cli/src/pricing.rs:26` | `models-dev-pricing.json` 的 mtime 新鲜度 |
| 计价策略串 | `gpt56-sol-undiscounted-relay-compat-v5` | `rust-cli/src/pricing.rs:24` | 变化触发全量重扫重价 |
| 长上下文阈值 | `272_000` 输入侧 token | `rust-cli/src/pricing.rs:27` | 仅 GPT-5.6 家族；严格大于触发 |
| 长上下文乘数 | 输入侧 `2.0` / 输出侧 `1.5` | `rust-cli/src/pricing.rs:28-29` | 作用于整笔请求 |
| GPT-5.6 Sol 卡价 | `$5 / $0.50 / $30` 每 1M | `rust-cli/src/pricing.rs:377` | cache_write = input×1.25（`pricing.rs:387`） |
| Terra/Luna 改价日 | `2026-07-30` | `rust-cli/src/pricing.rs:25` | 之前/之后两套官方价 |
| 证据文件上限 | `32 MiB` / `2500 文件/client` / 单行 `2 MiB` | `rust-cli/src/evidence.rs:43-45,752` | 递归深度 10 |
| effort 回看窗口 | `12 h` 前值 / `5 min` 近邻 | `rust-cli/src/evidence.rs:46-47` | `reasoning_effort` 关联 |
| Codex 增强文件窗口 | mtime `≤5 天` | `rust-cli/src/codex_tier.rs:320-331` | 增量模式 |
| 本地活动窗口 | `now − 89 天` 起点（约 90 天窗口）/ 明细 `20000` 条 | `rust-cli/src/local/activity.rs:9,91-92,97` | 本地工作台 |
| 配额历史保留 | `90 天` / `400 周期` / 每周期 `256` 样本 | `rust-cli/src/local/quota_history.rs:9-10,274-278,230-240` | 超限抽稀保留首尾 |
| 本地服务端口 | `43821` | `rust-cli/src/main.rs:42-52` | 仅绑 `127.0.0.1` |
| 设备在线阈值 | `3 min` online / `10 min` syncing | `web-ui/src/lib/data.ts:893-903` | presence 心跳判定 |
| 会话过期 | 闲置 `30 min` / 绝对 `12 h` | `web-ui/src/lib/session.ts:6-7` | 浏览器解锁态 |
| 周期匹配容差 | `±5 s` | `web-ui/src/lib/subscriptionPeriods.ts:3` | 官方 reset 时刻归并 |
| 预测算法常量 | stale `15 min`、最小跨度 `10 min`、容差 `1 pp`、突增 `5 pp/10min` | `rust-cli/local-web/quota-forecast.js:1-6,20` | `stable-linear-regression-v1`（类型声明 `quota-forecast.d.ts:23`） |
| 保存视图上限 | `20` 条 | `web-ui/src/lib/savedViews.ts:58` | 每仓库 |

## 附录 B：关键符号索引（符号 → 文件:行号）

> 行号口径同正文：UsageMesh 侧为仓库根相对路径（`rust-cli/...`、`web-ui/...`），tokscale 侧为 crate 根相对路径（`tokscale-core/src/...`，固定于 tag `v4.14.0`）。
> 本表所列行号均由脚本在源码中直接定位（`grep` 首命中行），用于快速回源；详细论证见对应章节。

### B.1 UsageMesh 设备侧（`rust-cli/src/`）

| 符号 | 位置 | 语义 |
| --- | --- | --- |
| `collector::collect` | `rust-cli/src/collector.rs:204` | 采集与计价主入口（调用 tokscale 扫描 → 路由身份 → 计价 → 分钟桶聚合 → Ledger） |
| `collector::merge_incremental` | `rust-cli/src/collector.rs:375` | 增量合并（按 `date < since` 保留旧行 + 明细按最小时间戳切分 + `totals` 全量重算） |
| `collector::reconciled_codex_days` | `rust-cli/src/collector.rs:182` | Codex 日级对账门（当日 token 完全相等才接受增强行） |
| `collector::trim_request_details` | `rust-cli/src/collector.rs:50` | 明细排序/去重/截断到最近 1000 条 |
| `model::CURRENT_LEDGER_SCHEMA_VERSION` | `rust-cli/src/model.rs:3` | 账本 schema 版本（当前 8） |
| `Metrics::total_tokens` | `rust-cli/src/model.rs:29` | 五桶饱和求和（含 reasoning 与 cache） |
| `Metrics::add` | `rust-cli/src/model.rs:37` | 整数桶饱和加、`cost_usd` 浮点直加 |
| `PriceBook::load` | `rust-cli/src/pricing.rs:242` | 四级价格加载回退链（新鲜缓存 → 在线 → 陈旧缓存 → 空目录） |
| `PriceBook::quote_on_date` | `rust-cli/src/pricing.rs:308` | 按日期计价（长上下文乘数、下界判定） |
| `guarded_pricing` | `rust-cli/src/pricing.rs:369` | GPT-5.6 家族兼容价卡（Sol/Terra/Luna，日期感知） |
| `provider::classify` | `rust-cli/src/provider.rs:229` | 路由身份四字段判定（hint 优先，双分支） |
| `provider::route_hint_from_base_url` | `rust-cli/src/provider.rs:152` | 由 base_url host 推断路由（official/azure/bedrock/vertex/openrouter/local/relay） |
| `evidence::scan` | `rust-cli/src/evidence.rs:774` | 路由与推理强度证据扫描 |
| `evidence::reasoning_effort_for_message` | `rust-cli/src/evidence.rs:821` | 三级 effort 选择（12h 前值 → 5min 近邻 → 会话唯一标签） |
| `codex_tier::collect` | `rust-cli/src/codex_tier.rs:333` | Codex 档位增强解析（Fast/Priority + 桶归一） |
| `crypto::encrypt_ledger` | `rust-cli/src/crypto.rs:40` | AES-256-GCM 账本加密（AAD 绑定设备指纹） |
| `crypto::wrap_dashboard_key` | `rust-cli/src/crypto.rs:98` | 密码包裹工作区密钥（PBKDF2-HMAC-SHA256 600k） |
| `github::replace_root_snapshot` | `rust-cli/src/github.rs:244` | 设备分支覆盖式提交（blob → 单文件 tree → 无父 commit → ref PATCH） |
| `github::refresh_dashboard_index` | `rust-cli/src/github.rs:351` | 刷新 `um-index/index.json`（仅列 `um-ledger-*` 分支名） |
| `github::replace_presence` | `rust-cli/src/github.rs:382` | 心跳分支写入（`device_hash`/`updated_at`/`app_version`/`schemaVersion`/`kind`） |
| `update::maybe_auto_update` | `rust-cli/src/update.rs:360` | 自更新检测 → 校验 → 替换 → 迁移重扫 |
| `local::serve` | `rust-cli/src/local/mod.rs:395` | 本地工作台 HTTP 服务（仅 127.0.0.1 + bearer + Host/Origin 校验） |
| `activity::scan` | `rust-cli/src/local/activity.rs:8` | 本地活动视图（89 天窗口、明细上限 20000） |
| `quota_official::fetch` | `rust-cli/src/local/quota_official.rs:274` | 读取已登录 `codex app-server` 观察官方配额 |
| `quota_history::record` | `rust-cli/src/local/quota_history.rs:111` | 观测归并进周期历史（0% 观测只更新 latest） |

### B.2 UsageMesh 仪表盘侧（`web-ui/src/`、`rust-cli/local-web/`）

| 符号 | 位置 | 语义 |
| --- | --- | --- |
| `applyDynamicPricing` | `web-ui/src/lib/data.ts:15` | 不重价的"重价"函数（`cost = storedCost`，下界标记转 `pricingResolved`） |
| `loadDashboardWithKey` | `web-ui/src/lib/data.ts:961` | 多设备账本并发拉取/解密/合并主流程（含快照隔离与缓存作用域） |
| `aggregateQuotaCycle` | `web-ui/src/lib/quotaCycles.ts:178` | 官方订阅周期跨设备聚合（渠道过滤 + 加性指标求和 + `deviceCount`） |
| `forecastCycleCost` | `web-ui/src/lib/quotaCycles.ts:230` | 周期金额比例外推 `recorded / usedPercent × 100` |
| `buildSubscriptionPeriods` | `web-ui/src/lib/subscriptionPeriods.ts:34` | 周期身份与归档（`resetsAt ±5s` 容差） |
| `quota-forecast.js` | `rust-cli/local-web/quota-forecast.js:1-296` | `stable-linear-regression-v1` 预测（云端周期视图与本地工作台共用） |

### B.3 tokscale-core v4.14.0（`tokscale-core/src/`）

| 符号 | 位置 | 语义 |
| --- | --- | --- |
| `parse_local_clients` | `tokscale-core/src/lib.rs:4334` | UsageMesh 使用的扫描入口（通道 A） |
| `LocalParseOptions` | `tokscale-core/src/lib.rs:429` | 七字段入参（`home_dir`/`use_env_roots`/`clients`/`since`/`until`/`year`/`scanner_settings`） |
| `ParsedMessage` | `tokscale-core/src/lib.rs:340` | 通道 A 的事件结构（16 字段，无 `cost`/`cost_source`/`dedup_key`） |
| `TokenBreakdown` | `tokscale-core/src/lib.rs:250` | 五桶（`input`/`output`/`cache_read`/`cache_write`/`reasoning`） |
| `canonical_model_id` | `tokscale-core/src/lib.rs:87` | 模型标识语法归一（不做别名折叠） |
| `unified_to_parsed` | `tokscale-core/src/lib.rs:5357` | 通道 B → 通道 A 结构转换（丢弃 `cost`/`cost_source`/`dedup_key` 等） |
| `should_keep_deduped_message` | `tokscale-core/src/lib.rs:5378` | 去重策略（首见保留、无键不去重） |
| `define_clients!` | `tokscale-core/src/clients.rs:320` | 客户端注册表宏（50 项、九列） |
| `ScannerSettings` | `tokscale-core/src/scanner.rs:40` | 扫描器设置（含 `bucket_timezone`、`extra_scan_paths`） |
| `BucketTimezone` | `tokscale-core/src/bucket_tz.rs:36` | 日桶时区（未 pin 即 `Local`；pin 后重分桶） |

## 附录 C：多轮复核记录

> 本文的每条实质结论都由**独立代理**在**隔离上下文**中回源核验，核验者看不到取证者的推理过程，只能看到文档与源码。以下是可复现的流程与发现台账。

### C.1 复核流程

| 轮次 | 执行者 | 覆盖对象 | 方法 | 结论门槛 |
| --- | --- | --- | --- | --- |
| 取证（第 0 轮） | 5 个独立取证代理 | ① tokscale-core v4.14.0 全量读取面；② 设备侧采集/账本/同步；③ 计价与路由身份；④ 云端仪表盘；⑤ 本地工作台 | 符号定位 → 分段精读 → 调用链闭合；输出 `文件:行号` 级草稿 | 无行号不给结论 |
| 第 1 轮（对抗性复核） | 5 个独立复核代理 | 按章节分工：ch03 / ch04+ch08 / ch05 / ch06+ch07 / 收口章+全文一致性 | 逐条回源 + 引用抽检 + 过度断言扫描 + 跨章矛盾检索 + 元信息核对 | 必须给出"实际核验过的断言清单"，不接受无证据的"没问题" |
| 第 2 轮（修正落地审计） | 3 个独立复核代理 | ① ch03+ch04+ch05+ch08 的修正落地；② ch06+ch07 的修正落地；③ 收口章 + 附录 + 全文一致性（含 TOC） | 逐条比对修正日志与文件现状，回源确认修正后的文本与源码一致，并做回归抽样 | 修正必须"既落地又与源码一致"，且不得引入新错 |
| 第 3 轮（冻结稿抽检） | 2 个独立复核代理：V9（事实与结构终检）、V10（覆盖度与可读性） | 定稿全文 + 附录 A/B/C | 随机引用审计 + 附录 B 全量核对 + 修正回退检查 + 结构终检 + 研究问题覆盖度矩阵 | 无阻断级、无重要级残留方可定稿；覆盖度缺口回填后需第 4 轮修正 + 第 5 轮确认 |

### C.2 第 1 轮发现与处置

第 1 轮合计 **102 条发现**（阻断 1 / 重要 28 / 轻微 73），全部经修正代理**逐条回源验证后确认成立**，反驳 **0 条**。

| 复核代理 | 覆盖 | 发现（阻断/重要/轻微） | 代表性发现 | 处置 |
| --- | --- | --- | --- | --- |
| V1 | ch04 设备侧采集 / ch08 同步链路 | 12（0/3/9）+ 10 条遗漏 + 4 条无法判定 | ①「同内容 ⇒ 同 commit sha」是未验证的服务端假设（`github.rs:278` 未设 author/committer，且三种载荷都含发布时间戳，`github.rs:356,389`、`crypto.rs:61`）；②「date 桶由扫描器时区决定」过度概括——未 pin 时 `BucketTimezone` 即 `Local`（`bucket_tz.rs:54-75`、`lib.rs:5290-5304`）；③全量模式字符串实为 6 种（`main.rs:377-387`） | 12/12 修正 + 6 条遗漏补入 |
| V2 | ch05 计价与路由 | 18（0/8/10） | ①「浏览器除平均单价外不做金额运算」不成立——订阅周期比例外推 `recorded / usedPercent × 100`（`quotaCycles.ts:271`，展示 `QuotaCycles.tsx:456,461-462`）；②`lookup()` 实际接收 tier 参数（`pricing.rs:291,294`），真正忽略它的是 `guarded_pricing` 的 `_tier`（`:369`）；③别名列表被 `sort()+dedup()`（`pricing.rs:133-134`），「精确优先」在同族双条目并存时不成立；④云路由分支用 `host.contains(...)`（`provider.rs:170,177,184`），边界感知只对 official 分支成立 | 18/18 已覆盖 + 6 条新增 |
| V3 | ch06 云端仪表盘 / ch07 本地工作台 | 22（0/2/20）+ 8 条遗漏 | ①云端缺少官方配额载荷准入校验这一关键前提（`data.ts:368-392` 等）；②ch07 章首「本机活动明细不上传」错误且与本章 §7.11 自相矛盾（`model.rs:138,140`、`crypto.rs:40-66`）；③配额节流键是 `last_attempt_at`（`mod.rs:305`），失败尝试同样冷却 | ch06 13 条、ch07 12 条修正 + 7 条遗漏/伴随项补入 |
| V4 | ch03 tokscale 章 | 25（**1 阻断**/9/15） | ①**阻断**：`fx` 在通道 A 无任何分派（`lib.rs:4367-5246` 零命中，只在 `lib.rs:2510-2531`），文档却把它当作 UsageMesh 的请求数来源；②`trae.rs:91` / `mux.rs:102` 均有 `dedup_key`，原"无键"名单错误；③lane 数三处互斥，实测通道 A 为 47 个 ClientId 分派段 + 1 个 `synthetic` 段；④v4.18.0 新增字段实为 4 个（含 `service_tier`，`v4.18.0:lib.rs:375`） | 25/25 修正 |
| V5 | 收口章 + 索引 + 附录 A + 全文一致性 | 25（0/6/19） | ①§10.1 R7 与 §3.5.13 把 `mux`/`trae` 当无键，与源码及本文其它小节三重矛盾；②§9 引用不存在的 §11、§10.1 引用不存在的 §6.15；③附录 A 数值 34/34 全对，但有 3 处引用位置不完整 | 15 条修正 + 2 条核验后无需改动，其余转交对应章节代理 |

引用抽检命中率：ch03 抽 30 条 **30/30 成立**；ch04+ch08 抽 235 条 **0 条偏移、0 条错位**；ch06 逐条回源 44 组、ch07 179 个引用点抽验；附录 A 数值 34/34、位置 31/34（3 处已补齐）。

### C.3 第 2 轮发现与处置（修正落地审计）

第 2 轮的目标不是重查全文，而是审计「第 1 轮的修正是否真的落地、是否引入新错」。合计 **24 条发现（0 阻断 / 5 中 / 18 低 / 1 无法判定）**，**零条证伪修正**。

| 复核代理 | 覆盖 | 落地命中率 | 回归检查 | 发现（中/低/其他） | 代表性发现与处置 |
| --- | --- | --- | --- | --- | --- |
| V6 | ch03 / ch04 / ch05 / ch08 的修正落地 | 61 条抽样：完全命中 59、部分命中 2、未落地 0（96.7%） | 20/20 通过 | 4（0/4/0） | ①ch03「按变更量降序只列前 19 项」的排序承诺不成立（被跳过的 6 个更大项实为新增文件）→ 改为「既有文件按 `git diff --numstat` 降序前 20 项」并补入 `copilot_vscode.rs`；②ch05「`host_is` 只用于 official 分支」过度概括 → 逐分支写实（`provider.rs:170/177` 为 `contains ‖ host_is`、`:184` 为 `== ‖ contains`、`:191` 用 `host_is`）；③ch03 表格裸竖线 2 处 → 转义为 `\|`；④`d4d1c751` 是轻量标签所指 commit，非「标签对象」→ 术语修正 |
| V7 | ch06 / ch07 的修正落地 | **28/28 = 100%** | 14/14 通过 | 8（0/7/1） | ①`closureReason` 只接受**显式 null**（键缺失同样致整条 cycle 被丢弃）；②ch07 的「准入校验未取证」与 ch06 新增小节冲突 → 互引 §6.5；③配额节流命中后的状态分路径（成功 `observed` / 失败 `stale`）；④official 标签补裸「官方 API」分支；⑤窗口准入补两条可选约束；⑥600 秒门槛的语义相等含排序归一；⑦记录上游文档不一致：`docs/LOCAL-WORKSPACE.md:83` 的「成功读取」措辞与实现相反，本文以代码为准 |
| V8 | 收口章 / 附录 A / 全文一致性 + 组装保真 | F5 的 15 条修正全部落地；组装文档与分文件**逐字零差异** | R1–R20 抽 12 条引用全部成立 | 12（5/6/1） | ①§10.1 的 R7 仍保留被 §3.5.13 否定的「约 1/3」量化 → 改为「部分客户端（名单见 §3.5.13）」；②§9 表「四组互斥参数」只列 3 类的枚举瑕疵 → 补全四组；③§10.1 的 R13 剥离清单不完整（漏 0% 窗口 `resetsAt` 与 `officialUsage.updatedAt`）→ 补齐；④R11 证据改为「结构定义 `github.rs:70-78` + 写入点 `:382-399`」；⑤§7.11「边界桶单列待核对」措辞过时 → 改为「已核实，见 §6.5」；⑥附录 A 的 `activity.rs:9,97` → `:9,91-92,97`；⑦章首引用的附录 B/C 原本不存在 → 本轮补齐附录 B（符号索引）与附录 C（本记录） |

一致性结论（V8 独立重算，**该轮时点**）：目录 **81 条 = 实际 81 条**（文本/顺序/锚点零差异，含本轮补齐的附录 B/C）；124 个编号标题序号连续无断号；74 个代码围栏全部成对；49 个表格块列数一致；`§` 引用共 37 个唯一编号，**35 个有效**（另 2 个 `§11`/`§6.15` 只出现在 §C.2 的历史台账里，用于描述"曾引用不存在章节"的修复记录）；附录 A 36/36 行回源成立；元信息 8 项逐项确证。

**定稿组装后的最终状态**（第 4/5 轮回填完成、重新拼装后由一级代理实测）：顶层 `##` 14 个（含「目录」自身，故实际章节标题 13 个）+ `###` 70 个，对应**目录 83 条 = 14 + 70 − 1**（第 4 轮新增 §3.5.14 与 §4.6）；代码围栏 74 行 = 37 对；**52 个表格块列数一致**（口径：允许行首缩进，行内 `\|` 视为转义）；`文件:行号` 引用（`:\d+` 形态）**4207 条**；占位符扫描 0（`待补`/`TODO`/`<!--F`）；失效引用扫描 0——`§11`/`§6.15` 只出现在本附录的历史台账里（第 5 轮复核代理核对），`待核对` 只出现在 §6.5「文档待核对项已可关闭」这一小节标题中。

### C.4 第 3 轮（冻结稿抽检）与第 4 轮（覆盖度回填）

**第 3 轮**由两个独立代理执行：V9 做事实与结构终检，V10 做覆盖度与可读性复核。

| 检查项 | 方法 | 结果 |
| --- | --- | --- |
| 抽样引用审计 | 从全文按章均匀抽取 54 条 `文件:行号`，回源比对「文件存在 + 行号落在语义处 + 该处支持说法」 | **54/54 命中** |
| 附录 A 全量 | 36 行逐行回源 | **34/34 数值行成立**（另 2 行为表头/说明） |
| 附录 B 全量 | 41 行符号 → 行号逐行 `grep`/`read` 复核（含「首命中行」口径） | **41/41 成立** |
| 修正回退检查 | 抽查第 1/2 轮已修正点是否仍在修正后状态 | **7/7 未回退、0 回退** |
| 结构终检 | 目录一致性（含锚点重算）、编号连续性、代码围栏配对、表格列数、失效引用与占位残留扫描 | 目录 81=81；124 个编号标题连续；74 行围栏 = 37 对；49 个表格块 0 偏差；0 占位；失效引用 0 |
| 深挖 3 项 | 通道 A 分派枚举、§6.5 配额准入 15 处数字、§3.8 的 v4.18.0 对照 | 全部成立（47 个 `ClientId`、五个上限数字、`ClientId::COUNT` 50→56、`normalize_syntactic` 39 行逐字相同） |

V9 结论：**无阻断级、无重要级残留**；5 条发现全为精度级（附录 C 台账数字过期、§C.4 自引用、产物清单漏列、§6.6 代码块为摘录），已在本轮回填。V10 结论：六问覆盖判定为「整体链路／多 Agent 读取／代码实现／金额计算 = 充分，聚合与可追溯 = 部分」，并给出 4 条必修项（附录 C 空壳、§3.5.13 缺证据档与未表化、`README.md:177` 矛盾未点名、`--no-schedule` 全文未提）。

**第 4 轮回填**（3 个修正代理，均先回源后编辑，0 条反驳）：

| 补强项 | 落点 | 结果 |
| --- | --- | --- |
| 聚合总纲 | 新增 §4.6「聚合口径总纲（七层并列）」，把设备侧分钟桶／明细／增量／跨设备／展示层／官方周期／本地工作台的七处口径并列成表 | 7 行 × 3 列，每行带 `文件:行号` |
| 复刻速查 | 新增 §3.5.14「复刻速查表」（6 列 × 48 行 = 46 个客户端名（47 个通道 A 分派 ClientId，`devin-cli`/`devin-desktop` 合一行）+ `fx` + `synthetic`） | 未取证格显式写「未验证」（第 5 轮复核把 `synthetic` 三格回填为 `synthetic.rs:164-167/177-181/185`；`kilocode` 路径格补 `clients.rs:533-542`；`kilo` 的去重配置引用由 `kilo.rs:313` 纠正为 `opencode_schema.rs:313`） |
| 横向表补列 | §3.5.13 由 6 列扩为 9 列（+证据档、+时间戳单位、+token 是否含估算） | 24 行逐格填源；`claude`/`codex` 为【已核实】，其余 22 行为【代核实】 |
| `grok` 计数口径 | 「多为 0」改为严格表述：取值域恰为 `{0,1}`、等于 `i32::from(loop_index == 1)` | `grok.rs:746-763`、测试 `:1558-1599` |
| 去重 | 删除 §3.5.10 末尾与 §3.5.12 重复的覆盖度表，改为交叉引用 | 零 `文件:行号` 丢失 |
| CLI 补漏 | 补 `--no-schedule`（决定是否安装常驻采集代理）与 `finish_onboarding` 的分支描述 | `main.rs:64-95`、`:480-511` |
| 内部同步 | ch05 §5.8-3 的 `since`【未验证】→ 指向 §3.6.10 已核实结论；§5.12-5 的 `canonical_model_id`「未读 core」→ 指向 §3.2.7／§3.9.1 | 本仓侧独立行号保留 |
| 文档矛盾 | ch06 §6.1 新增「文档与实现矛盾：`README.md:177`」（英文版安全模型写反，中文版与实现一致），并升格为 §10.1 的 R21 | `session.ts:3,30,99,136,145,164-179,197-232` |
| 摘录标注 | ch06 §6.6 代码块注明「摘录改写，语义等价；原文见 `data.ts:24-29`」 | — |

**第 5 轮（终稿确认）**：由独立代理**只针对第 4 轮新增内容**做对抗性核验（新增内容最容易引入新错）。

| 检查项 | 抽样 | 结果 |
| --- | --- | --- |
| §3.5.14 复刻速查表 | 20/48 行 | 18 行零缺陷；2 行有缺陷（`kilocode` 路径误标"未验证"、`kilo` 去重配置行号越界）→ 本轮回填/纠正 |
| §3.5.13 九列表 | 12/24 行 | 12/12 成立（`grok` 行已为严格表述） |
| §4.6 聚合总纲 | 7/7 行 | 全部回源成立 |
| `--no-schedule` 与 `finish_onboarding` | 8 组断言 | 全部成立 |
| ch06 §6.1/§6.5/§6.6 + §10.1 R21 | 11 组 + 6 组 | 全部成立（`session.ts` 20 处行号零偏移） |
| 「未验证」格对抗性复核 | 3 例 | 2 例确认无源码依据（`mux` 时间戳单位、`zcode` SQLite `started_at`），1 例可回填（`synthetic`，本轮已补） |
| 附录 C 台账数字 | 6 项 | 全部可复现（并对"表格块 51"追查确认口径为"允许行首缩进"） |

V11 结论：**无阻断级错误**；1 条重要 + 1 条中等 + 6 条轻微，全部为单格/单句改动，已在本轮修正并重新组装。

### C.5 残留不确定性与已知边界

1. **未运行任何代码**：两个仓库只做静态阅读，未 `cargo build`/`cargo test`/`npm test`，因此「测试是否通过」「运行时是否报错」类问题不在本文结论范围内（多处测试仅作为**语义锚点**引用其断言，而非作为已执行结果）。
2. **未触碰真实数据**：未读取本机 `~/.claude`、`~/.codex`、`~/.config/tokscale` 等目录内容，因此涉及「本机 scanner 设置实际取值」「真实日志字段形态」的判断一律落在未验证项。
3. **远端行为未实测**：GitHub REST/Pages/限流/ETag 等线上行为、`codex app-server` 真机字段、`models.dev` 实际 JSON 结构均未联网验证。
4. **平台路径未验证**：Linux cron / systemd、macOS launchd 路径仅静态阅读，未在真机运行。
5. **版本漂移**：本文锚定 UsageMesh `8cfb77f`（2026-09-11）与 tokscale-core `v4.14.0`；上游后续提交可能改变行号与行为（v4.18.0 的差异清单见 §3.8，其中「只认标签对象、勿用主分支工作树」的引用体例见 §3.1 的说明）。

### C.6 复核产物清单（可在本机临时目录复核）

| 产物 | 路径 |
| --- | --- |
| 第 1 轮复核报告 | `%TEMP%\usagemesh-notes\verify\V1-findings.md` … `V5-findings.md`（含逐条源码证据） |
| 引用落点审计原文 | `%TEMP%\usagemesh-notes\verify\_lineref-audit.txt`（235 条引用的源码原文） |
| 第 2 轮复核报告 | `%TEMP%\usagemesh-notes\verify\V6-findings.md`（落地审计）、`V7-findings.md`（落地审计）、`V8-findings.md`（一致性/附录） |
| 第 3 轮复核报告 | `%TEMP%\usagemesh-notes\verify\V9-findings.md`（冻结稿终检）、`V10-review.md`（覆盖度与可读性）、`V10-officialdoc-diff.md`（官方文档差异全集 51 条） |
| 冻结稿审计过程数据 | `%TEMP%\usagemesh-notes\verify\V9-appB-raw.txt`、`V9-refs-index.csv`（2176 条引用索引）、`V9-sampling.txt` |
| 修正日志（共 11 份） | `F1-fixlog.md`（ch04/ch08）、`F2-fixlog.md`（ch05）、`F3-ch06-fixlog.md`、`F3-ch07-fixlog.md`、`F4-fixlog.md`（ch03）、`F5-fixlog.md`（收口章/附录）、`F6-fixlog.md`（ch03/ch05 精度）、`F7-fixlog.md`（ch06/ch07 精度）、`F8-fixlog.md`（ch02/ch03 覆盖度）、`F9-fixlog.md`（ch04 覆盖度）、`F10-fixlog.md`（ch05/ch06 同步与文档矛盾） |
| 二级取证原始报告（【代核实】条目的底层证据） | `%TEMP%\usagemesh-notes\raw\group-A.md` … `group-D.md`（逐模块原始输出） |
| 章节源文件（拼装输入） | `%TEMP%\usagemesh-notes\final\*.md`、`%TEMP%\usagemesh-notes\S0-front.md`、`S9-tail.md` |
| 源码克隆（只读） | UsageMesh：`%TEMP%\usagemesh-research`（HEAD `8cfb77f`）；tokscale v4.14.0：`%TEMP%\tokscale-v4.14.0`（`1ec865d`）；对照 v4.18.0 用标签 `v4.18.0` 解析出的 commit `d4d1c751`（轻量标签，无独立 tag 对象） |

> 复核的独立性边界：所有复核代理都只读源码与文档，**未运行任何二进制或测试**（`cargo`/`npm` 全程禁用），因此所有结论都是**源码级**而非运行时级；凡涉及"实际运行时会怎样"的判断，本文一律落在 §3.10、§5.12、§6.11、§7.12、§10.2.2 的未验证项里，不写成事实。
