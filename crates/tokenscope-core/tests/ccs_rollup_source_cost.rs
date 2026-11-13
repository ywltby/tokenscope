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

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{HistoryDb, RollupConflictPolicy};
use tokenscope::import::ccs::{self, CcsSource};
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
