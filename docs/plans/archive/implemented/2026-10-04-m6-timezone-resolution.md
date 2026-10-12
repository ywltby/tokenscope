# M6：时区解析链（存储一律 UTC，展示按 解析时区 一次转换）

> **2026-10-11 归档复核：已实现。** 本文保留历史目标与执行记录。归档不等于未验项目通过；当前待办只在 [整合计划](../../active/2026-10-11-consolidated-remaining-work.md) 登记，状态总账见 [计划索引](../../README.md)。
> 用户明确要求优先，其次采用较新计划；本次用户已要求全部/部分完成均归档，下方旧“必须保留 active”或“唯一活跃入口”不再作为执行规则。

| 原任务 / 范围 | 当前状态 | 剩余任务承接 |
| --- | --- | --- |
| 任务与自身验收 1–4 | 原 UTC/时区/往返/文档验收已有记录；下拉位于 Settings | 无独立待办 |
| 当前 GUI 时区 | 可在共同界面批次复用；不宣称历史安装已验 | [N11](../../active/2026-10-11-consolidated-remaining-work.md#n11)（共用验收） |

## 归档前原文（历史记录）

> 以下状态、版本号、命令和验收记录描述当时阶段；与上表或新计划冲突时，采用上表及新计划。

- 状态：**已完成（2026-10-04），GUI 时区下拉冒烟待用户安装确认**
- 创建：2026-10-04

## 现状与目标

现状：缓存已存 UTC（jiff `Timestamp::to_string()` 即 RFC3339 `Z` 形态），聚合在 `local_tz()`（硬编码 Asia/Shanghai）一次性落日——架构上已是"单次转换"，但时区不可配置。

目标：

1. **固化 UTC 存储不变量**：`cache.db` 事件表 `ts` 列一律 UTC RFC3339；加载解析回 `Timestamp` 后才做唯一一次时区转换。以测试固化。
2. **时区解析链**（展示与聚合用，全链路只解析一次）：
   - **显式传入**优先（CLI `--tz <IANA>` / GUI 下拉），非法名 → 参数错误；
   - `--tz local` / GUI「本机时区」→ 系统时区（jiff `TimeZone::system()`）；
   - **兜底**：系统时区不可得时按仓库约定 Asia/Shanghai（本机即 Asia/Shanghai，行为不变）。
3. JSON 报告新增 `timezone` 字段（解析后的时区标识），供机器消费方核对口径。

## 兼容性

- 本机系统时区 = Asia/Shanghai，默认解析结果与 M1–M5 完全一致，历史验收记录与对照数据全部有效。
- e2e 与 report 测试统一显式钉 `tz: Some("Asia/Shanghai")`（与 openrouter_path 同样的确定性策略），并新增跨时区日界测试（同一 16:00Z 事件在 Asia/Shanghai 与 UTC 下分属不同日）。

## 非目标

- 每行明细按事件各自时区展示（按会话来源时区）——后续明细视图再说
- 时区数据库在线更新（jiff 内置 tzdb）

## CLI / GUI 面

- CLI：`tokenscope summary --tz <local|IANA名>`（缺省 = 本机系统时区）。
- GUI：Dashboard 过滤栏新增时区下拉（本机时区 / Asia/Shanghai / UTC），随查询传入。
- 设置页不做时区持久化（查询级选项）。

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | `resolve_tz` 替换 `local_tz`；SummaryOptions/Report 增 tz | `src/aggregate.rs`、`src/report.rs` | `test_resolve_tz_`（指定/local/非法名）| `cargo test resolve` |
| 2 | 跨时区日界测试 + 测试钉缺省时区 | `src/aggregate.rs`、`tests/*`、`src/report.rs` | `test_aggregate_tz_boundary_` | `cargo test aggregate` |
| 3 | 缓存 UTC 不变量测试 | `src/cache.rs` | `test_cache_stores_utc_` | `cargo test cache` |
| 4 | CLI `--tz` | `src/cli.rs` | `test_summary_flags_` 扩展 | `cargo run -- summary --tz UTC` |
| 5 | GUI 下拉 + invoke 传参 + JSON timezone | `src-tauri/src/commands.rs`、`frontend/src/*` | `test_parse_` 回归 | `vue-tsc` + `pnpm build` |

## 验收

1. 全量门禁全绿（fmt / clippy / test / vue-tsc / pnpm build）。
2. 真实数据：默认（本机）与 `--tz Asia/Shanghai` 输出逐字段一致（本机时区即上海）；`--tz UTC` 日界变化符合预期（16:00Z 事件归当日）。
3. 缓存 UTC 不变量：往返后 `ts` 保持 UTC 形态（单测固化）。
4. 文档：CLAUDE.md 时间口径不变量、README 统计口径同步更新。

## 风险

- jiff `TimeZone::system()` 取不到系统时区时内部回退 UTC（非上海）→ 文档注明；Windows 常规环境均可取到。
- JSON 新增字段对既有消费方 → 纯增量，无破坏。

## 验收记录（2026-10-04）

1. 全量门禁：fmt / clippy（0 警告）/ `cargo test --workspace`（66 测试）/ vue-tsc / pnpm build 全绿。
2. 真实数据验收（Claude 全量）：`--tz Asia/Shanghai` 与 `--tz local` 输出逐字段一致（本机时区即上海，M1–M5 记录全部有效）；`--tz UTC` 日界变化生效；非法时区（`Mars/Olympus`）报参数错误。
3. 缓存 UTC 不变量：往返后 ts 保持 `2026-07-17T08:00:00.123456Z` 原样（单测 `test_cache_stores_utc` 固化）；三路径一致性维持。
4. JSON 报告新增 `timezone` 字段（解析后标识）；e2e/report 测试统一钉 `tz: Some("Asia/Shanghai")`，不再依赖运行机器系统时区。

## 实现要点

- `resolve_tz(Option<&str>)`：`"local"` → `TimeZone::system()`；IANA 名 → `TimeZone::get`（非法名报错）；None → Asia/Shanghai。时区标识用 `iana_name()`（jiff 的 `TimeZone` 未实现 `Display`）。
- 转换时机不变（M1 即如此）：缓存/加载全程 UTC，聚合落日 + `generated_at` 是仅有的两处转换点。
- 修正计划口径：解析链落地为「显式传入 > 本机（`--tz local`）> 默认 Asia/Shanghai」——Asia/Shanghai 保留为缺省值而非系统时区兜底（零行为变化，历史记录全部有效）。
