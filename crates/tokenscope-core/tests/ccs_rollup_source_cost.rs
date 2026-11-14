//! T2（2026-11-13）：CCS 日汇总来源费用的展示与聚合口径。
//!
//! 口径（用户指令）：
//! - 只对来源选择算法**最终纳入统计**的日汇总（rollup_only 桶 / B 类桶）使用
//!   其有效 `source_cost_usd`；明细优先的 A 类桶不追加汇总费用，C 类回退不
//!   加入未选中汇总的费用；
//! - 来源费用单独披露为「CCS 来源记录费用」，不冒充 TokenScope 请求级估算
//!   或实际账单；汇总、总计、各分组口径一致；
//! - 来源费用缺失（NULL / 非法文本）时如实按无来源费用处理，**不套用**
//!   「缺失缓存价按 0」规则；
//! - 显式 0 是合法值（来源记录了零费用），必须与缺失区分；
//! - 重复导入不重复累计（重导零增量，与既有导入幂等一致）。
//!
//! 全部 fixture 合成、路径与历史库在临时目录，不触碰真实用户数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::history::{HistoryDb, RollupConflictPolicy};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-t2-source-cost-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 合成 CCS 库结构（与 ccs_rollup_coverage.rs 同形）。
const CCS_SCHEMA: &str = r"
CREATE TABLE proxy_request_logs (
    request_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    status_code INTEGER NOT NULL DEFAULT 200,
    session_id TEXT,
    created_at INTEGER NOT NULL,
    data_source TEXT NOT NULL DEFAULT 'proxy',
    pricing_model TEXT,
    input_token_semantics INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE usage_daily_rollups (
    date TEXT NOT NULL,
    app_type TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '',
    pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT,
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
";

/// 日汇总行：(日期, 应用, 模型, 请求数, 输入, 输出, total_cost_usd 文本)。
/// 费用列在本 fixture 中可空（真实 CCS 为 NOT NULL DEFAULT '0'，但 NULL 的
/// 语义测试需要显式可空列——缺失费用 ≠ 0 是被测口径）。
type RollupRow<'a> = (&'a str, &'a str, &'a str, i64, i64, i64, Option<&'a str>);

fn seed_rollups(path: &Path, rollups: &[RollupRow<'_>]) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(CCS_SCHEMA).unwrap();
    conn.pragma_update(None, "user_version", 20).unwrap();
    for (date, app, model, count, input, output, cost) in rollups {
        conn.execute(
            "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
                request_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, input_token_semantics)
             VALUES(?1, ?2, 'p1', ?3, '', ?4, ?5, ?6, 0, 0, ?7, 0)",
            rusqlite::params![date, app, model, count, input, output, cost],
        )
        .unwrap();
    }
}

fn history_of(dir: &Path) -> HistoryDb {
    HistoryDb::open(&dir.join("data").join("history.db")).unwrap()
}

fn import_all(dir: &Path, history: &HistoryDb) {
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, history, "Asia/Shanghai").unwrap();
    ccs::commit(
        &preview.plan_id,
        history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
}

fn opts(dir: &Path, by: GroupBy) -> SummaryOptions {
    SummaryOptions {
        by,
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        // 与 detail 测试的日志目录一致（该目录不存在时只是来源缺失警告，
        // 不影响被测口径；需要明细的用例先创建并写入合成日志）。
        claude_dir: Some(dir.join("claude")),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 合成一条 Claude assistant 行（写入 `dir/claude/proj/s.jsonl`）。
fn write_claude_lines(dir: &Path, lines: &[String]) {
    let path = dir.join("claude").join("proj").join("s.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, lines.join("\n")).unwrap();
}

fn claude_line(message: &str, ts: &str, input: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": "s1",
        "message": {
            "id": message,
            "model": "claude-opus-4-7",
            "usage": {
                "input_tokens": input,
                "output_tokens": 0,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        }
    })
    .to_string()
}

/// 验收例：两条来源费用 7.25 + 1.75 = 9（rollup_only 桶，无明细）。
/// 分组（按日）各带自己的费用，总计 = 9；主估算 cost_usd 保持 0（不重估），
/// unknown_pricing 照常置位。
#[test]
fn rollup_source_costs_sum_per_group_and_total() {
    let dir = temp_dir("sum");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                10,
                1000,
                0,
                Some("7.25"),
            ),
            (
                "2026-09-21",
                "claude",
                "claude-opus-4-7",
                5,
                500,
                0,
                Some("1.75"),
            ),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);

    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    let d1 = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-20")
        .unwrap();
    let d2 = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-21")
        .unwrap();
    assert!(
        (d1.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "day1 来源费用 7.25，实际 {:?}",
        d1.source_cost_usd
    );
    assert!(
        (d2.source_cost_usd.unwrap() - 1.75).abs() < 1e-9,
        "day2 来源费用 1.75，实际 {:?}",
        d2.source_cost_usd
    );
    assert!(
        (report.totals.source_cost_usd.unwrap() - 9.0).abs() < 1e-9,
        "总计来源费用 9.00，实际 {:?}",
        report.totals.source_cost_usd
    );
    // 主估算不变：日汇总不按当前价格重估，费用仍是未知披露。
    assert_eq!(d1.cost_usd, 0.0);
    assert!(d1.unknown_pricing);
    assert_eq!(d1.unknown_reason, Some("rollup_only"));
    // 来源费用缺失≠0：费用为 0 的组不允许来自「缺失按 0」。
    assert_eq!(report.rollup_coverage.rollup_buckets, 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// 显式 0：来源记录了零费用（免费/订阅抵扣），必须如实显示 0 而不是缺失。
#[test]
fn explicit_zero_source_cost_is_kept_as_zero() {
    let dir = temp_dir("zero");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            3,
            300,
            0,
            Some("0"),
        )],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.source_cost_usd,
        Some(0.0),
        "显式 0 是合法记录值"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// NULL / 非法文本 → 来源未提供费用：`source_cost_usd` 为 None，不按 0 冒充。
#[test]
fn missing_source_cost_stays_absent_not_zero() {
    let dir = temp_dir("null");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[("2026-09-20", "claude", "claude-opus-4-7", 3, 300, 0, None)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.source_cost_usd, None,
        "来源未提供费用必须保持缺失，不得套用按 0 规则"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A 类（明细逐项覆盖汇总）：汇总不参与统计 → 来源费用不得出现。
#[test]
fn class_a_detail_selected_does_not_add_source_cost() {
    let dir = temp_dir("class-a");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            1,
            100,
            0,
            Some("5.55"),
        )],
    );
    // 一条 TS 原生明细，四桶与请求数均 ≥ 汇总（=）→ A 类。
    write_claude_lines(
        &dir,
        &[claude_line("m1", "2026-09-20T10:00:00.000+08:00", 100)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert_eq!(report.rollup_coverage.detail_selected_buckets, 1);
    assert_eq!(
        report.totals.source_cost_usd, None,
        "A 类按明细统计，汇总来源费用不得追加"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// B 类（汇总补足明细）：来源费用随被采用的汇总计入一次；快照后新增明细
/// 不改变来源费用（它来自汇总行本身，不重复相加）。
#[test]
fn class_b_rollup_supplements_keeps_single_source_cost() {
    let dir = temp_dir("class-b");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            10,
            1000,
            0,
            Some("7.25"),
        )],
    );
    // 快照内 1 条明细（100 token < 汇总 1000）→ B 类；再放一条快照后的明细。
    write_claude_lines(
        &dir,
        &[
            claude_line("m1", "2026-09-20T10:00:00.000+08:00", 100),
            claude_line("m2", "2026-09-22T10:00:00.000+08:00", 200),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(
        report.totals.tokens.input, 1200,
        "1000（汇总）+ 200（快照后新增）"
    );
    assert!(
        (report.totals.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "来源费用只来自被采用的汇总，恰一次"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// C 类（交叉冲突）：按明细回退，未选中的汇总费用不得混入。
#[test]
fn class_c_fallback_does_not_add_unselected_source_cost() {
    let dir = temp_dir("class-c");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            1,
            100,
            500,
            Some("9.99"),
        )],
    );
    // 明细 input 200 > 汇总 100 且 output 0 < 汇总 500 → 交叉冲突（C 类）。
    write_claude_lines(
        &dir,
        &[claude_line("m1", "2026-09-20T10:00:00.000+08:00", 200)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert_eq!(report.rollup_coverage.unresolved_buckets, 1);
    assert_eq!(
        report.totals.source_cost_usd, None,
        "C 类按明细回退，未选中汇总的费用不得计入"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 重复导入：内容相同的快照重导零增量，来源费用不翻倍。
#[test]
fn identical_reimport_does_not_double_source_cost() {
    let dir = temp_dir("reimport");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            10,
            1000,
            0,
            Some("7.25"),
        )],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert!(
        (report.totals.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "重导零增量：来源费用仍 7.25，实际 {:?}",
        report.totals.source_cost_usd
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 筛选一致性：日期筛选后，分组与总计只含范围内的来源费用。
#[test]
fn date_filter_scopes_source_cost() {
    let dir = temp_dir("filter");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                10,
                1000,
                0,
                Some("7.25"),
            ),
            (
                "2026-09-21",
                "claude",
                "claude-opus-4-7",
                5,
                500,
                0,
                Some("1.75"),
            ),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let mut o = opts(&dir, GroupBy::Day);
    o.from = Some("2026-09-21".to_string());
    o.to = Some("2026-09-21".to_string());
    let report = summary(&o).unwrap();
    assert!(
        (report.totals.source_cost_usd.unwrap() - 1.75).abs() < 1e-9,
        "筛选后只含范围内的来源费用，实际 {:?}",
        report.totals.source_cost_usd
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 按模型分组：来源费用随模型身份分组聚合（等价写法合并为一组）。
#[test]
fn source_cost_groups_by_model_identity() {
    let dir = temp_dir("by-model");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                10,
                1000,
                0,
                Some("7.25"),
            ),
            (
                "2026-09-20",
                "claude",
                "claude-opus-4.7",
                5,
                500,
                0,
                Some("1.75"),
            ),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Model);
    let report = summary(&o).unwrap();
    // `claude-opus-4-7` 与 `claude-opus-4.7` 等价 → 同一组，费用 7.25 + 1.75。
    assert_eq!(report.groups.len(), 2, "一组 + 合计行");
    let g = report
        .groups
        .iter()
        .find(|g| g.key != "合计")
        .expect("只有一个数据组");
    assert!(
        (g.source_cost_usd.unwrap() - 9.0).abs() < 1e-9,
        "等价写法合并后来源费用 9.00，实际 {:?}",
        g.source_cost_usd
    );
    assert!((report.totals.source_cost_usd.unwrap() - 9.0).abs() < 1e-9);
    std::fs::remove_dir_all(&dir).ok();
}

/// 重启一致性：来源费用持久保存于历史库，重启后再次查询结果一致。
#[test]
fn source_cost_survives_restart() {
    let dir = temp_dir("restart");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            10,
            1000,
            0,
            Some("7.25"),
        )],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    drop(history);
    // 「重启」= 重新打开库再查询。
    let reopened = history_of(&dir);
    drop(reopened);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    assert!(
        (report.totals.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "重启后来源费用保持一致，实际 {:?}",
        report.totals.source_cost_usd
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 明细视图不受影响：来源费用只进汇总展示通道，明细行费用仍来自请求级估算。
#[test]
fn detail_rows_are_unaffected_by_source_cost() {
    let dir = temp_dir("detail");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            10,
            1000,
            0,
            Some("7.25"),
        )],
    );
    // 快照后的独立明细（2026-09-22，不在汇总覆盖范围内）。
    write_claude_lines(
        &dir,
        &[claude_line("m2", "2026-09-22T10:00:00.000+08:00", 200)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let snapshot = tokenscope::query::begin_query(&o).unwrap();
    let list =
        tokenscope::query::query_events(&snapshot.query_id, &EventFilter::default()).unwrap();
    assert_eq!(list.rows.len(), 1, "明细视图照常列出请求");
    // 该明细模型无价格来源 → 行费用 None（未知），不受来源费用影响。
    assert_eq!(list.rows[0].cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

// ── T-C（2026-11-14，v0.9.1 缺陷 C）：cost_detail 费用构成字段 ─────────
//
// 用户指令：把「没有请求级估算」「来源已提供费用」「模型真正缺价」分开。
// 展示层依据 cost_detail 分流；unknown_pricing/unknown_tokens/unknown_reason
// 的既有语义不变（真缺价仍如实披露），本组测试锁这些不变量。

/// T-C1：纯日汇总组 → cost_detail = "rollup_only"；未知语义照旧。
#[test]
fn cost_detail_rollup_only_for_pure_rollup_group() {
    let dir = temp_dir("cd-rollup");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            10,
            1000,
            0,
            Some("7.25"),
        )],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    let d1 = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-20")
        .unwrap();
    assert_eq!(
        d1.cost_detail,
        Some("rollup_only"),
        "纯汇总组的费用构成必须标为 rollup_only"
    );
    assert_eq!(report.totals.cost_detail, Some("rollup_only"));
    // 未知语义不变（缺价披露不因 cost_detail 引入而消失）。
    assert!(d1.unknown_pricing);
    assert_eq!(d1.unknown_reason, Some("rollup_only"));
    assert_eq!(d1.unknown_tokens.total(), 1000);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-C2：明细与汇总并存（B 类桶 + 快照后新增明细）→ cost_detail = "mixed"。
#[test]
fn cost_detail_mixed_when_detail_and_rollup_coexist() {
    let dir = temp_dir("cd-mixed");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[(
            "2026-09-20",
            "claude",
            "claude-opus-4-7",
            10,
            1000,
            0,
            Some("7.25"),
        )],
    );
    // 快照内 1 条明细（100 < 1000 → B 类汇总补足）+ 快照后新明细（独立计入）。
    write_claude_lines(
        &dir,
        &[
            claude_line("m1", "2026-09-20T10:00:00.000+08:00", 100),
            claude_line("m2", "2026-09-22T10:00:00.000+08:00", 200),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    // 按模型分组：同一模型组同时含明细请求与汇总贡献 → mixed。
    let o = opts(&dir, GroupBy::Model);
    let report = summary(&o).unwrap();
    let g = report
        .groups
        .iter()
        .find(|g| g.key != "合计")
        .expect("单模型组");
    assert_eq!(g.cost_detail, Some("mixed"), "明细 + 汇总并存 → mixed");
    assert_eq!(report.totals.cost_detail, Some("mixed"));
    std::fs::remove_dir_all(&dir).ok();
}

/// T-C3：纯明细组不输出 cost_detail（serde skip；旧 IPC 消费方逐字节兼容）。
#[test]
fn cost_detail_absent_for_pure_detail_group() {
    let dir = temp_dir("cd-detail");
    write_claude_lines(
        &dir,
        &[claude_line("m1", "2026-09-22T10:00:00.000+08:00", 200)],
    );
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    let d = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-22")
        .expect("明细组");
    assert_eq!(d.cost_detail, None, "纯明细组无 cost_detail 字段");
    // serde：序列化后不含该键（向前兼容旧前端/旧快照）。
    let json = serde_json::to_value(d).unwrap();
    assert!(
        json.get("cost_detail").is_none(),
        "纯明细组的 JSON 不得出现 cost_detail 键"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// T-C4：totals 聚合——全部组纯汇总 → rollup_only；混合 → mixed。
#[test]
fn cost_detail_totals_aggregate_over_groups() {
    let dir = temp_dir("cd-totals");
    // 模型 A（opus）：纯汇总（rollup_only）；模型 B（mimo）：汇总 + 快照后
    // 明细（mixed）。
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                10,
                1000,
                0,
                Some("7.25"),
            ),
            ("2026-09-20", "claude", "mimo-v2-5", 4, 400, 0, Some("1.75")),
        ],
    );
    // mimo 的快照后明细（2026-09-22）→ mimo 组 mixed；opus 无明细。
    let path = dir.join("claude").join("proj").join("s2.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": "2026-09-22T11:00:00.000+08:00",
        "sessionId": "s2",
        "message": {
            "id": "mm1",
            "model": "mimo-v2-5",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 0,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        }
    })
    .to_string();
    std::fs::write(&path, line).unwrap();
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Model);
    let report = summary(&o).unwrap();
    let opus = report
        .groups
        .iter()
        .find(|g| g.key.contains("opus"))
        .expect("opus 组");
    let mimo = report
        .groups
        .iter()
        .find(|g| g.key.contains("mimo"))
        .expect("mimo 组");
    assert_eq!(opus.cost_detail, Some("rollup_only"));
    assert_eq!(mimo.cost_detail, Some("mixed"));
    assert_eq!(
        report.totals.cost_detail,
        Some("mixed"),
        "任一组 mixed → totals mixed"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// T-C5：来源费用缺失的 rollup_only 组也标 rollup_only（展示层负责区分
/// 「来源未提供」与「来源记录 0」——字段只描述构成，不描述费用值）。
#[test]
fn cost_detail_rollup_only_even_without_source_fee() {
    let dir = temp_dir("cd-nofee");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[("2026-09-20", "claude", "claude-opus-4-7", 10, 1000, 0, None)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    let d1 = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-20")
        .unwrap();
    assert_eq!(d1.cost_detail, Some("rollup_only"));
    assert_eq!(d1.source_cost_usd, None, "费用缺失仍如实为 None");
    assert!(d1.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 8.1（2026-11-14 第二轮复核）：跨分组合计的费用构成 ─────────────────
//
// 复核探针复现：明细与汇总分属不同模型/日期时，totals.cost_detail 被错标
// rollup_only，CostCell/SummaryCards 只显示来源金额、请求估算被隐藏。
// 修复口径（用户要求）：合计以实际贡献构成判定——存在明细贡献与汇总贡献
// 即为 mixed，不要求同一分组；判定不看金额是否 > 0；仅切换分组维度时合计
// 构成与金额展示必须一致。

/// 底层聚合合成助手：与复核探针同路径（aggregate_with_rollups）。
fn review_event(model: &str, ts: &str, input: u64) -> UsageEvent {
    UsageEvent {
        ts: ts.parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: model.into(),
        session_id: "review-session".into(),
        project: "review-project".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: format!("review-{model}"),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 1,
        source_path: "synthetic-only.jsonl".into(),
        input_tokens: input,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn review_rollup(model: &str, day: &str, input: u64, fee: Option<f64>) -> RollupContribution {
    RollupContribution {
        day: day.into(),
        source_tz: "UTC".into(),
        app: AgentKind::ClaudeCode,
        model: model.into(),
        requests: 1,
        tokens: TokenCounts {
            input,
            ..Default::default()
        },
        source_cost_usd: fee,
        source: "synthetic-source".into(),
    }
}

fn review_pricing(dir: &Path) -> Pricing {
    // 外置价格表：review-detail 模型可估价（1M input = $1.0）；review-rollup
    // 故意不提供价格（不参与估算）。
    let path = dir.join("review-pricing.json");
    std::fs::write(
        &path,
        r#"{"v":4,"synced_at":"2026-11-14T00:00:00Z","entries":[{"id":"review/review-detail","name":null,"input":1.0,"output":2.0,"cache_write":0.0,"cache_read":0.0}]}"#,
    )
    .unwrap();
    let (pricing, warnings) = Pricing::load(None, Some(&path), None);
    assert!(warnings.is_empty(), "{warnings:?}");
    pricing
}

/// T-8.1a：明细与汇总分属不同模型 → 按模型聚合 totals 必须是 mixed，
/// cost_usd 与 source_cost_usd 并存（旧实现错标 rollup_only → $1.00 被隐藏）。
#[test]
fn cross_group_totals_are_mixed_when_detail_and_rollup_coexist() {
    let dir = temp_dir("t81-model");
    let pricing = review_pricing(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    let events = [review_event(
        "review-detail",
        "2026-11-14T00:00:00Z",
        1_000_000,
    )];
    let rollups = [review_rollup(
        "review-rollup",
        "2026-11-13",
        2_000_000,
        Some(7.25),
    )];
    for by in [GroupBy::Model, GroupBy::Day, GroupBy::Agent] {
        let result = aggregate_with_rollups(&events, &rollups, by, &tz, &pricing).unwrap();
        let t = &result.totals;
        assert_eq!(
            t.cost_detail,
            Some("mixed"),
            "{by:?}: 明细与汇总并存（跨组）→ totals mixed，实际 {:?}",
            t.cost_detail
        );
        assert_eq!(t.cost_usd, 1.0, "{by:?}: 请求估算金额不丢失");
        assert_eq!(t.source_cost_usd, Some(7.25), "{by:?}: 来源费用不丢失");
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// T-8.1b：同一分组混合（B 类桶 + 快照后明细）与跨组混合给出**相同**的
/// totals 构成——只切换分组维度不改变合计费用构成与金额展示。
#[test]
fn totals_composition_is_dimension_independent() {
    let dir = temp_dir("t81-dim");
    let pricing = review_pricing(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // 同模型：明细 + 采用汇总（同组 mixed，等价于按 Agent 聚合的合并组）。
    let events = [review_event("review-m", "2026-11-14T00:00:00Z", 1_000_000)];
    let rollups = [review_rollup(
        "review-m",
        "2026-11-13",
        2_000_000,
        Some(7.25),
    )];
    let same_group =
        aggregate_with_rollups(&events, &rollups, GroupBy::Agent, &tz, &pricing).unwrap();
    // 不同模型：明细 review-detail（可估价 $1.0）+ 汇总 review-b（按模型
    // 聚合时跨组）。
    let events2 = [review_event(
        "review-detail",
        "2026-11-14T00:00:00Z",
        1_000_000,
    )];
    let rollups2 = [review_rollup(
        "review-b",
        "2026-11-13",
        2_000_000,
        Some(7.25),
    )];
    let cross_group =
        aggregate_with_rollups(&events2, &rollups2, GroupBy::Model, &tz, &pricing).unwrap();
    assert_eq!(same_group.totals.cost_detail, Some("mixed"));
    assert_eq!(
        cross_group.totals.cost_detail, same_group.totals.cost_detail,
        "相同贡献集合在不同分组维度下合计构成一致"
    );
    assert!((cross_group.totals.cost_usd - 1.0).abs() < 1e-9);
    assert!((cross_group.totals.source_cost_usd.unwrap() - 7.25).abs() < 1e-9);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-8.1c：零金额明细（模型无价格 → cost=0）仍算明细贡献——判定不看金额。
/// 纯明细 + 纯汇总（均无费用）→ totals 仍是 mixed 而不是 rollup_only。
#[test]
fn zero_amount_detail_still_counts_for_mixed_totals() {
    let dir = temp_dir("t81-zero");
    // review-detail 在价格表里不存在 → 请求级估算 cost=0（unknown_pricing）。
    let pricing = review_pricing(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    let events = [review_event(
        "unpriced-model",
        "2026-11-14T00:00:00Z",
        1_000_000,
    )];
    let rollups = [review_rollup(
        "review-rollup",
        "2026-11-13",
        2_000_000,
        None,
    )];
    let result = aggregate_with_rollups(&events, &rollups, GroupBy::Model, &tz, &pricing).unwrap();
    assert_eq!(
        result.totals.cost_detail,
        Some("mixed"),
        "零金额明细贡献仍构成 mixed（不能依赖金额 > 0 判定）"
    );
    // 分组行各自如实：明细组 unknown_pricing、汇总组 rollup_only。
    assert!(result.totals.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 8.2（2026-11-14 第二轮复核）：来源费用覆盖完整性 ───────────────────
//
// 复核复现：同模型两条采用汇总，一条 Some(7.25) 一条 NULL → 聚合只有
// Some(7.25)，前端当完整费用展示。修复口径：新增 source_cost_state
// （full / partial / absent）区分「已知合计」与「是否覆盖全部采用的汇总」；
// 显式 0 属于已提供费用，与 NULL 严格区分。

/// T-8.2a：部分提供 → source_cost_state = partial（旧行为只给 Some(7.25)）。
#[test]
fn partial_source_fee_coverage_is_disclosed() {
    let dir = temp_dir("t82-partial");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                10,
                1000,
                0,
                Some("7.25"),
            ),
            ("2026-09-21", "claude", "claude-opus-4-7", 5, 500, 0, None),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Model);
    let report = summary(&o).unwrap();
    let g = report.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "已知费用照常累计"
    );
    assert_eq!(
        g.source_cost_state,
        Some("partial"),
        "部分 NULL → 覆盖不完整必须披露，实际 {:?}",
        g.source_cost_state
    );
    assert_eq!(report.totals.source_cost_state, Some("partial"));
    // 未知语义不动（不靠清零掩盖）。
    assert!(g.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-8.2b：全部提供（含全部显式 0）→ full；显式 0 与 NULL 混合 → partial。
#[test]
fn explicit_zero_is_provided_full_when_complete() {
    let dir = temp_dir("t82-zero");
    // 两条都显式 0 → full，合计 $0.00。
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                3,
                300,
                0,
                Some("0"),
            ),
            (
                "2026-09-21",
                "claude",
                "claude-opus-4-7",
                2,
                200,
                0,
                Some("0"),
            ),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Model);
    let report = summary(&o).unwrap();
    let g = report.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.source_cost_state, Some("full"));
    assert_eq!(g.source_cost_usd, Some(0.0));
    std::fs::remove_dir_all(&dir).ok();

    // 一条 0 一条 NULL → partial（0 是已提供，NULL 是缺失）。
    let dir2 = temp_dir("t82-zero-null");
    seed_rollups(
        &dir2.join("cc-switch.db"),
        &[
            (
                "2026-09-20",
                "claude",
                "claude-opus-4-7",
                3,
                300,
                0,
                Some("0"),
            ),
            ("2026-09-21", "claude", "claude-opus-4-7", 2, 200, 0, None),
        ],
    );
    let history2 = history_of(&dir2);
    import_all(&dir2, &history2);
    let o2 = opts(&dir2, GroupBy::Model);
    let report2 = summary(&o2).unwrap();
    let g2 = report2.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g2.source_cost_state, Some("partial"));
    assert_eq!(g2.source_cost_usd, Some(0.0), "已知部分（0）照常累计");
    std::fs::remove_dir_all(&dir2).ok();
}

/// T-8.2c：全部缺失 → absent（聚合口径如实记录；展示层「来源未提供费用」）。
/// 纯明细组不输出该字段（JSON 兼容）。
#[test]
fn absent_and_full_states_and_json_compatibility() {
    let dir = temp_dir("t82-absent");
    seed_rollups(
        &dir.join("cc-switch.db"),
        &[
            ("2026-09-20", "claude", "claude-opus-4-7", 3, 300, 0, None),
            ("2026-09-21", "claude", "claude-opus-4-7", 2, 200, 0, None),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(&dir, GroupBy::Day);
    let report = summary(&o).unwrap();
    let d1 = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-20")
        .unwrap();
    assert_eq!(d1.source_cost_state, Some("absent"));
    assert_eq!(d1.source_cost_usd, None);
    assert_eq!(report.totals.source_cost_state, Some("absent"));
    std::fs::remove_dir_all(&dir).ok();

    // 纯明细组：字段缺省不序列化（旧契约逐字节兼容）。
    let dir2 = temp_dir("t82-json");
    write_claude_lines(
        &dir2,
        &[claude_line("m1", "2026-09-22T10:00:00.000+08:00", 200)],
    );
    let o2 = opts(&dir2, GroupBy::Day);
    let report2 = summary(&o2).unwrap();
    let d = report2
        .groups
        .iter()
        .find(|g| g.key == "2026-09-22")
        .unwrap();
    assert_eq!(d.source_cost_state, None);
    let json = serde_json::to_value(d).unwrap();
    assert!(
        json.get("source_cost_state").is_none(),
        "纯明细组 JSON 不得出现 source_cost_state 键"
    );
    std::fs::remove_dir_all(&dir2).ok();
}
