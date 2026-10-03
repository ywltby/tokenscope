# M7：逐请求明细视图（汇总 → 下钻）

- 状态：**已确认（用户 2026-10-04 指示"继续推进plan"；按候选清单推进，选型理由见下）**
- 创建：2026-10-04

## 选型说明（为何不是 Gemini/OpenCode 适配器）

候选清单前两项（Gemini CLI / OpenCode 适配器）本机均无数据源（实测 `~/.gemini`、`~/.opencode`、`~/.qwen` 等不存在；cc-switch `session_log_sync` 追踪的 1770 个文件全部属于 .claude/.codex），无真实日志即无法按本项目"数据事实先行 + 真实验收"的流程交付。**暂缓至用户安装相应工具或提供样例日志后立项**。本里程碑取第三候选：数据已在缓存 `events` 表，纯本地可验证，补全"汇总 → 下钻"的使用闭环。

## 目标

1. **共用采集路径**：report 管线拆出 `collect_all`（源发现 → 缓存增量 → 全局去重 → 回填统计），`summary()` 与新增 `list_events()` 共用——明细与汇总数字永远同源（M3 建立的原则延伸到行级）。
2. **`list_events`**：过滤（模型 / 项目 / 自然日 + 既有 agent/days/时区）→ 按时间倒序 → limit 截断；输出 `EventRow`（时间、agent、模型、会话、项目、四类 token、费用；无价格模型费用为 `null`，不按 0）。
3. **CLI `events` 子命令** + **GUI 下钻**：点击汇总表行（日期/模型/项目/Agent）即按该键过滤下方明细表，一键清除筛选。

## 设计决策

- **去重口径不变**：明细行即去重后的事件（与汇总计数一致，总额核对 = 明细求和）。
- **过滤顺序**：`days`（沿用）→ `day`（精确自然日，按解析时区）→ `model` / `project`（精确匹配）→ 排序 → `limit`。总数返回"过滤后、截断前"条数。
- **limit**：默认 200，上限 1000；本地 SQLite 数据量下 offset 分页无必要（M7 不做翻页，总数可见即可）。
- **时间展示**：`EventRow.ts` 以解析时区格式化 RFC3339（M6 单次转换原则），存储仍 UTC。
- **GUI 交互**：汇总表行点击 → Dashboard 状态 `{type, key}` → `list_events` 过滤；类型映射：日期行 → `day`、模型行 → `model`、项目行 → `project`、Agent 行 → 传 `agent` 过滤。明细区显示筛选条件与"清除筛选"。

## 非目标

- 明细分页/无限滚动（总数 + 上限 1000 已够 M7）
- 按会话聚合视图、请求耗时/流式信息（日志里没有）
- 明细导出 CSV

## 任务清单（代码位置 / 测试名（前缀）/ 验证命令）

| # | 任务 | 代码位置 | 测试名（前缀） | 验证命令 |
| --- | --- | --- | --- | --- |
| 1 | `collect_all` 抽取 + summary 复用 | `src/report.rs` | 既有 66 测试回归 | `cargo test` |
| 2 | `list_events` + `EventRow` + 过滤/排序/limit | `src/report.rs` | `test_list_events_`（过滤/倒序/limit/去重后一致/unknown 费用） | `cargo test list_events` |
| 3 | CLI `events` 子命令（表格 + JSON） | `src/cli.rs`、`src/render/mod.rs` | `test_events_cmd_` | `cargo run -- events --json` |
| 4 | GUI 下钻（行点击 → 明细表） | `frontend/src/*`、`src-tauri/src/commands.rs` | `test_parse_` 回归 | `vue-tsc` + `pnpm build` |
| 5 | 真实数据验收 + 文档 | — | — | 总额核对 = 明细求和 |

## 验收

1. 全量门禁全绿（fmt / clippy / test / vue-tsc / pnpm build）。
2. 真实数据：`summary` 合计的请求数 = `list_events`（同过滤、limit 上限内）返回行数；抽样一天，明细四类 token 求和与汇总行一致。
3. 去重后明细行数与汇总一致（codex fixture 上同请求重发行不出现）。
4. GUI 冒烟（行点击下钻、清除筛选）留用户确认。

## 风险

- 行点击与表格刷新的状态耦合 → 明细筛选状态独立持有，清除按钮显式复位。
- limit 截断导致"看到的和 ≠ 总和"→ 明细区明示"共 N 条，显示前 X 条"。
