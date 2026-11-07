//! 2026-11-05：CCS 历史补充与日汇总统计口径。
//!
//! 口径（用户定稿）：
//! 1. Claude Code 的原生请求明细优先；能确认是同一请求的 CCS 明细不重复计入；
//! 2. 可信 CCS 日汇总覆盖同一日期/模型/统计范围、而 TS 只有**部分**明细时，
//!    用汇总作为该范围总量（合计 1,000：明细 100 不额外相加），明细仍可查看；
//! 3. 导入快照之后新增的原生请求不得被旧汇总盖掉，也不重复计算；无法确认
//!    覆盖关系时保留明细并明确提示；
//! 4. CCS 只补充 Claude Code——Codex/WorkBuddy 等其他应用以 TS 自身采集与
//!    已保存的原生历史为准；已导入的旧数据保留事实但不参与统计。
//!
//! 全部 fixture 合成、路径与历史库在临时目录，不触碰真实用户数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{
    DailyRollupWrite, EventAlias, EventOrigin, EventWrite, HistoryDb, ImportRunRecord,
    ORIGIN_KIND_CCS_REQUEST, RollupConflictPolicy, WritePrecedence,
};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-ccs-rollup-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 合成 CCS 库结构（与 ccs_import.rs 同形，本文件独立持有）。
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
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
";

#[derive(Clone)]
struct Row {
    request_id: &'static str,
    app_type: &'static str,
    model: &'static str,
    session_id: Option<&'static str>,
    data_source: &'static str,
    created_at: i64,
    input: i64,
    output: i64,
    semantics: i64,
}

impl Default for Row {
    fn default() -> Self {
        Self {
            request_id: "r1",
            app_type: "claude",
            model: "claude-opus-5-5",
            session_id: Some("s1"),
            data_source: "proxy",
            created_at: 1_790_000_000,
            input: 100,
            output: 0,
            semantics: 0,
        }
    }
}

/// 合成日汇总的一行：(日期, 应用, 模型, 请求数, 输入, 输出)。
type RollupRow<'a> = (&'a str, &'a str, &'a str, i64, i64, i64);

fn seed_ccs(path: &Path, rows: &[Row], rollups: &[RollupRow<'_>]) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(CCS_SCHEMA).unwrap();
    conn.pragma_update(None, "user_version", 20).unwrap();
    for r in rows {
        conn.execute(
            "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd, status_code, session_id, created_at, data_source, pricing_model,
                input_token_semantics)
             VALUES(?1, 'p1', ?2, ?3, ?3, ?4, ?5, 0, 0, '0.25', 200, ?6, ?7, ?8, ?3, ?9)",
            rusqlite::params![
                r.request_id,
                r.app_type,
                r.model,
                r.input,
                r.output,
                r.session_id,
                r.created_at,
                r.data_source,
                r.semantics,
            ],
        )
        .unwrap();
    }
    for (date, app, model, count, input, output) in rollups {
        conn.execute(
            "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
                request_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, input_token_semantics)
             VALUES(?1, ?2, 'p1', ?3, '', ?4, ?5, ?6, 0, 0, '1.5', 0)",
            rusqlite::params![date, app, model, count, input, output],
        )
        .unwrap();
    }
}

fn history_of(dir: &Path) -> HistoryDb {
    HistoryDb::open(&dir.join("data").join("history.db")).unwrap()
}

/// 导入全部（明细 + 日汇总）并断言提交成功。
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

fn opts(dir: &Path, claude_dir: Option<PathBuf>, by: GroupBy, tz: &str) -> SummaryOptions {
    SummaryOptions {
        by,
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        claude_dir,
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some(tz.to_string()),
        ..Default::default()
    }
}

/// 一条 Claude assistant 行（原生采集 fixture）。
fn claude_line(
    session: &str,
    message: &str,
    ts: &str,
    model: &str,
    input: u64,
    output: u64,
) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "message": {
            "id": message,
            "model": model,
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        }
    })
    .to_string()
}

fn write_claude_log(root: &Path, rel: &str, lines: &[String]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, lines.join("\n")).unwrap();
}

fn detail_rows(opts: &SummaryOptions) -> usize {
    let snapshot = query::begin_query(opts).unwrap();
    let list = query::query_events(&snapshot.query_id, &EventFilter::default()).unwrap();
    list.rows.len()
}

/// 验收例：CCS 汇总 1,000 token / 10 请求，TS 只有 1 条 100 token 明细。
/// 总量必须是 1,000（不是 100，也不是 1,100）；明细仍可查看。
#[test]
fn rollup_1000_over_partial_detail_100_counts_once() {
    let dir = temp_dir("partial");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-1",
            input: 100,
            created_at: 1_789_866_000,
            ..Default::default()
        }],
        &[("2026-09-20", "claude", "claude-opus-5-5", 10, 1000, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    assert_eq!(history.event_count().unwrap(), 1, "明细照常入库");
    assert_eq!(history.rollup_count().unwrap(), 1, "日汇总照常入库");

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(report.totals.tokens.input, 1000, "汇总 1000 是该范围总量");
    assert_eq!(report.totals.requests, 10, "请求数取汇总口径");
    assert_eq!(report.totals.unknown_reason, Some("rollup_only"));
    assert_eq!(
        report.totals.unknown_tokens.input, 1000,
        "汇总 token 无法按当前价格重估 → 计未知"
    );
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 100,
        "被替代的明细 token 如实披露"
    );
    assert!(
        !report.warnings.iter().any(|w| w.contains("无法确认")),
        "已按规则处理的覆盖不再报未解决：{:?}",
        report.warnings
    );
    assert_eq!(detail_rows(&o), 1, "明细视图仍可查看被覆盖的那条请求");
    std::fs::remove_dir_all(&dir).ok();
}

/// 旧库迁移 / 早期导入留下的日汇总没有批次关联（`import_run_id` 为空）：没有
/// 可核对的快照时刻，整桶明细按"信任 CCS 汇总"口径处理，并披露可能少算。
///
/// 2026-11-07：重导**内容完全相同**的快照不得把该行绑到新批次。重读同一数值
/// 只证明"此刻来源仍写着这个数"，不证明该快照在本次读取时建立——绑定会把原本
/// 无法确认的覆盖边界当成新版可信边界（边界随之后移，两次导入之间到达的原生
/// 请求被静默吞掉，不确定性提示同时消失）。内容确实被替换时才重绑，见后半段。
#[test]
fn legacy_rollup_without_import_run_keeps_unknown_boundary_after_identical_reimport() {
    let dir = temp_dir("legacy-null-batch");
    let tz = "Asia/Shanghai";
    let day = "2026-09-20";
    let db = dir.join("cc-switch.db");
    // 来源只剩日汇总（10 请求 / 1,000 token），TS 侧没有与之对应的 CCS 明细。
    seed_ccs(&db, &[], &[(day, "claude", "claude-opus-5-5", 10, 1000, 0)]);
    let history = history_of(&dir);
    import_all(&dir, &history);
    // v1 迁移 / 早期导入遗留：该行没有批次关联 → 边界无法确认。
    {
        let conn = rusqlite::Connection::open(dir.join("data").join("history.db")).unwrap();
        conn.execute("UPDATE ccs_daily_usage SET import_run_id = NULL", [])
            .unwrap();
    }
    // TS 原生请求：日志里读得到，但是否曾被旧汇总包含无法证实。
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            "2026-09-20T09:00:00.000+08:00",
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    let o = opts(&dir, Some(claude), GroupBy::Day, tz);
    let before = summary(&o).unwrap();
    assert_eq!(
        before.totals.tokens.input, 1000,
        "旧行边界不可确认：整桶按信任口径，不把明细再加一次"
    );
    assert_eq!(before.totals.requests, 10);
    assert_eq!(before.rollup_coverage.covered_buckets, 1);
    let note = before
        .rollup_coverage
        .legacy_boundary
        .clone()
        .expect("边界不可确认时必须披露可能少算");

    // 重新导入内容完全相同的汇总（真实 open → preview → commit）。
    let generation_before = history.generation().unwrap();
    let source = CcsSource::open(&db).unwrap();
    let preview = ccs::preview(&source, &history, tz).unwrap();
    assert_eq!(preview.rollups_unchanged, 1, "同一份快照：预览计未变化");
    assert_eq!(preview.rollups_new, 0);
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    drop(source);
    assert_eq!(report.rollups_snapshotted, 0, "内容未变：不计新增或替换");

    // 重读同一数值不等于证实边界：该行仍无批次关联。
    let stored = history.daily_rollups(None).unwrap();
    assert_eq!(stored.len(), 1);
    assert!(
        stored[0].import_at.is_none(),
        "内容未变的重导不得把旧行绑定到新批次"
    );
    assert!(!stored[0].boundary_known, "没有批次关联就不可能确认边界");
    assert_eq!(
        history.generation().unwrap(),
        generation_before,
        "重复导入仍幂等（不因重读而生成新版本）"
    );

    let after = summary(&o).unwrap();
    assert_eq!(
        after.totals.tokens.input, 1000,
        "重导幂等：总量不因重读同一数值而变化"
    );
    assert_eq!(after.totals.requests, 10);
    assert_eq!(
        after.rollup_coverage.legacy_boundary.as_deref(),
        Some(note.as_str()),
        "不确定性提示必须保留"
    );

    // 对照：内容确实被替换（用户选择"以来源为准"）→ 重绑到本次批次。
    update_ccs_rollup_row(&db, day, "p1", "claude-opus-5-5", 12, 1200, 0);
    let source = CcsSource::open(&db).unwrap();
    let preview = ccs::preview(&source, &history, tz).unwrap();
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::TakeSource,
        true,
    )
    .unwrap();
    drop(source);
    assert_eq!(report.rollups_snapshotted, 1, "内容真的变了才计入替换");
    let repaired = summary(&o).unwrap();
    assert_eq!(repaired.totals.tokens.input, 1200, "新汇总已含那 200");
    assert!(
        repaired.rollup_coverage.legacy_boundary.is_none(),
        "内容替换后边界可确认，旧版披露随之消失"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 仅 CCS 历史：没有明细时直接用汇总（既有行为不回归）。
#[test]
fn rollup_only_history_counts_rollup() {
    let dir = temp_dir("rollup-only");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[("2026-09-20", "claude", "claude-opus-5-5", 7, 700, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(report.totals.tokens.input, 700);
    assert_eq!(report.totals.requests, 7);
    assert_eq!(report.rollup_coverage.rollup_buckets, 1);
    assert_eq!(report.rollup_coverage.covered_buckets, 0);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(detail_rows(&o), 0, "没有明细行可展示");
    std::fs::remove_dir_all(&dir).ok();
}

/// 仅原生：没有日汇总时按明细统计，覆盖诊断全 0。
#[test]
fn native_only_history_counts_detail() {
    let dir = temp_dir("native-only");
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            "2026-09-20T09:00:00.000+08:00",
            "claude-opus-5-5",
            200,
            20,
        )],
    );
    let o = opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(report.totals.tokens.input, 200);
    assert_eq!(report.totals.requests, 1);
    assert_eq!(report.rollup_coverage.covered_buckets, 0);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    std::fs::remove_dir_all(&dir).ok();
}

/// 导入之后新增的原生请求：不得被旧汇总盖掉，也不与汇总重复。
#[test]
fn requests_added_after_import_are_not_swallowed_by_old_rollup() {
    let dir = temp_dir("post-import");
    // 汇总落在"明天"：导入时刻（现在）早于该日，可构造"快照之后新增"。
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let today = jiff::Timestamp::now().to_zoned(tz.clone()).date();
    let tomorrow = today.tomorrow().unwrap();
    let day = tomorrow.to_string();
    let new_ts = tomorrow
        .at(1, 0, 0, 0)
        .to_zoned(tz.clone())
        .unwrap()
        .timestamp()
        .to_string();

    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[(day.as_str(), "claude", "claude-opus-5-5", 10, 1000, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);

    // 导入之后（事件时间晚于导入时刻）写入一条原生请求。
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s2.jsonl",
        &[claude_line("s2", "m2", &new_ts, "claude-opus-5-5", 200, 0)],
    );
    let o = opts(&dir, Some(claude.clone()), GroupBy::Day, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "汇总 1000 + 导入后新增 200"
    );
    assert_eq!(report.totals.requests, 11, "10 + 1，不重复");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(detail_rows(&o), 1, "新增请求在明细里可见");

    // 重启 + 重复采集：口径稳定，不因为第二轮而改变。
    let second = summary(&opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai")).unwrap();
    assert_eq!(second.totals.tokens.input, 1200);
    assert_eq!(second.totals.requests, 11);
    std::fs::remove_dir_all(&dir).ok();
}

/// 快照范围内的明细不超过汇总声称的数量：按"信任 CCS 日汇总"口径以汇总为
/// 总量，明细不重复计入（明细视图仍可查看）。
#[test]
fn details_within_rollup_request_count_trust_rollup_total() {
    let dir = temp_dir("unresolved");
    let rows: Vec<Row> = ["r1", "r2", "r3"]
        .into_iter()
        .map(|id| Row {
            request_id: id,
            created_at: 1_789_866_000,
            input: 100,
            ..Default::default()
        })
        .collect();
    seed_ccs(
        &dir.join("cc-switch.db"),
        &rows,
        &[("2026-09-20", "claude", "claude-opus-5-5", 3, 500, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 500,
        "3 条明细不超过汇总声称的 3 个请求 → 取汇总总量"
    );
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 300,
        "被替代的明细量如实披露"
    );
    assert_eq!(detail_rows(&o), 3, "明细视图仍可查看三条请求");
    std::fs::remove_dir_all(&dir).ok();
}

/// 快照范围内的明细与汇总**方向交叉**（请求数更多、用量更少）：保留明细并
/// 给出明确冲突类型，不猜造、不相加。
///
/// 语义变化（2026-11-07）：这条用例过去断言 `report.warnings` 里出现
/// "无法确认汇总能覆盖这些明细 / 汇总共 N token 未纳入"。该文案把统计来源
/// 冲突塞进采集警告通道，前端会把它算成"本轮采集存在部分问题 / N 条采集
/// 警告"，并用"未纳入若干 token"暗示同量数据全部丢失——这是本次修复的
/// 误报。反例保留（保留明细、汇总不叠加、冲突仍须准确诊断），断言改为
/// `RollupCoverage` 的结构化字段与明确冲突类型；界面独立显示，见
/// `useUsageQuery.test.ts` 的 source_conflict_is_independent 用例。
#[test]
fn snapshot_era_details_exceeding_rollup_stay_unresolved_with_warning() {
    let dir = temp_dir("unresolved-exceed");
    // 4 条原生明细（真实采集路径）vs 汇总声称 3 个请求。
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[
            claude_line(
                "s1",
                "m1",
                "2026-09-27T01:00:00Z",
                "claude-opus-5-5",
                100,
                0,
            ),
            claude_line(
                "s1",
                "m2",
                "2026-09-27T02:00:00Z",
                "claude-opus-5-5",
                100,
                0,
            ),
            claude_line(
                "s1",
                "m3",
                "2026-09-27T03:00:00Z",
                "claude-opus-5-5",
                100,
                0,
            ),
            claude_line(
                "s1",
                "m4",
                "2026-09-27T04:00:00Z",
                "claude-opus-5-5",
                100,
                0,
            ),
        ],
    );
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-09-27",
        "claude-opus-5-5",
        3,
        500,
        "2026-09-27T12:00:00Z",
    );
    let o = opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 400,
        "保留明细统计，不把汇总相加"
    );
    assert_eq!(report.rollup_coverage.unresolved_buckets, 1);
    assert_eq!(report.rollup_coverage.covered_buckets, 0);
    assert_eq!(
        report.rollup_coverage.detail_selected_buckets, 0,
        "交叉冲突不得被 A 类分支吞掉"
    );
    assert_eq!(
        report.rollup_coverage.conflict_kinds,
        vec!["opposed_requests_tokens".to_string()],
        "4 请求多于 3、400 token 少于 500：请求数与用量方向相反"
    );
    assert!(
        report.warnings.is_empty(),
        "统计来源冲突不并入采集警告：{:?}",
        report.warnings
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 不同日期/模型各自独立：汇总与明细只要不落同一桶就互不覆盖。
#[test]
fn different_day_or_model_buckets_do_not_interfere() {
    let dir = temp_dir("buckets");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-1",
            model: "claude-opus-5-5",
            created_at: 1_789_866_000,
            input: 100,
            ..Default::default()
        }],
        &[
            // 另一天（无明细）：用汇总。
            ("2026-09-19", "claude", "claude-opus-5-5", 5, 500, 0),
            // 另一模型（无明细）：用汇总。
            ("2026-09-20", "claude", "claude-sonnet-4-5", 4, 400, 0),
        ],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(report.totals.tokens.input, 100 + 500 + 400);
    assert_eq!(report.rollup_coverage.rollup_buckets, 2);
    assert_eq!(report.rollup_coverage.covered_buckets, 0);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 重复导入幂等：同一份 CCS 再导入一次，统计总量不变。
#[test]
fn repeated_import_is_idempotent() {
    let dir = temp_dir("reimport");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-1",
            input: 100,
            created_at: 1_789_866_000,
            ..Default::default()
        }],
        &[("2026-09-20", "claude", "claude-opus-5-5", 10, 1000, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let before = summary(&opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    ))
    .unwrap()
    .totals
    .tokens
    .input;
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.rollups_unchanged, 1);
    assert_eq!(preview.would_insert, 0);
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    let after = summary(&opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    ))
    .unwrap()
    .totals
    .tokens
    .input;
    assert_eq!(before, 1000);
    assert_eq!(after, before, "重复导入不改变总量");
    std::fs::remove_dir_all(&dir).ok();
}

/// CCS 不再影响其他应用：Codex 明细/汇总行在预览里计为不支持、不入库。
#[test]
fn ccs_no_longer_imports_other_agents() {
    let dir = temp_dir("other-agents");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            Row {
                request_id: "codex-1",
                app_type: "codex",
                model: "gpt-5.6-sol",
                session_id: None,
                input: 1000,
                ..Default::default()
            },
            Row {
                request_id: "session:msg-1",
                input: 100,
                ..Default::default()
            },
        ],
        &[("2026-09-20", "codex", "gpt-5.6-sol", 12, 9000, 0)],
    );
    let history = history_of(&dir);
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.requests_importable, 1, "只有 Claude 行可导入");
    assert_eq!(preview.requests_skipped_other_app, 1);
    assert_eq!(preview.rollups_rejected, 1, "Codex 日汇总也不导入");
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    assert_eq!(
        history
            .stored_events_filtered(Some(AgentKind::Codex))
            .unwrap()
            .len(),
        0,
        "不写入 Codex 事实"
    );
    assert_eq!(history.rollup_count().unwrap(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 已导入的旧 Codex CCS 事实：保留在库中但不参与统计；同事件有原生来源时
/// 按原生事实保留并计入。
#[test]
fn legacy_ccs_codex_facts_are_kept_but_not_counted() {
    let dir = temp_dir("legacy-codex");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-20T01:00:00Z".parse().unwrap();
    let codex_alias = |value: &str| EventAlias {
        app: AgentKind::Codex,
        scheme: "codex-usage".into(),
        value: value.into(),
    };
    let tokens = TokenCounts {
        input: 100,
        output: 10,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let write = |key: &str, precedence: WritePrecedence| EventWrite {
        event_key: key.into(),
        app: AgentKind::Codex,
        ts,
        model_raw: "gpt-5.6-sol".into(),
        model_identity: "gpt56sol".into(),
        session_id: Some("s1".into()),
        record_id: None,
        project_key: None,
        session_initial_cwd: None,
        event_cwd: None,
        tokens,
        precedence,
        observed_at: ts,
        aliases: vec![codex_alias("s1|gpt-5.6-sol|100|10|0|0")],
        origins: vec![EventOrigin {
            origin_kind: "ccs_request".into(),
            origin_key: key.into(),
            app: AgentKind::Codex,
            import_run_id: None,
            parser_revision: 3,
            source_model_raw: Some("gpt-5.6-sol".into()),
            pricing_model: Some("gpt-5.6-sol".into()),
            source_cost_usd: Some(8.5),
            ts_precision_seconds: false,
        }],
    };
    // 纯 CCS 的 Codex 旧事实：入库（不删除事实）。
    history
        .write_batch(&[write(
            "ccs|codex|proxy|legacy-1",
            WritePrecedence::CcsImport,
        )])
        .unwrap();
    // 原生事实（同一会话/模型/四桶，原生优先级更高）→ 合并并保留原生。
    history
        .write_batch(&[write(
            "native|codex|/logs/a.jsonl|0|fp",
            WritePrecedence::NativeLog,
        )])
        .unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "同一事件只保留一条事实");

    // 只有 CCS 来源的 Codex 事件（另一会话）：不参与统计，但事实仍在。
    let mut other = write("ccs|codex|proxy|legacy-2", WritePrecedence::CcsImport);
    other.session_id = Some("s2".into());
    other.aliases = vec![codex_alias("s2|gpt-5.6-sol|100|10|0|0")];
    history.write_batch(&[other]).unwrap();
    assert_eq!(history.event_count().unwrap(), 2);

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Agent,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(report.totals.requests, 1, "只统计有原生来源的那一条");
    assert_eq!(report.totals.tokens.input, 100);
    assert_eq!(
        history
            .stored_events_filtered(Some(AgentKind::Codex))
            .unwrap()
            .len(),
        2,
        "旧数据事实保留"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 来源时区与展示时区不同：不重切日期、不采用汇总，保留明细并给出时区提示。
#[test]
fn timezone_mismatch_keeps_detail_and_hint() {
    let dir = temp_dir("tz");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-1",
            created_at: 1_789_866_000,
            input: 100,
            ..Default::default()
        }],
        &[("2026-09-20", "claude", "claude-opus-5-5", 10, 1000, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    // 展示时区 UTC + 按日分组 → 日汇总不参与（来源按 Asia/Shanghai 生成）。
    let o = opts(&dir, Some(dir.join("no-claude")), GroupBy::Day, "UTC");
    let report = summary(&o).unwrap();
    assert_eq!(report.rollup_coverage.covered_buckets, 0);
    assert_eq!(report.rollup_coverage.rollup_buckets, 0);
    assert!(
        report.rollup_coverage.timezone_mismatch.is_some(),
        "时区不匹配必须给出可恢复提示"
    );
    assert_eq!(report.totals.tokens.input, 100, "回退到明细，不重切日期");
    std::fs::remove_dir_all(&dir).ok();
}

/// 汇总与明细口径在**分组视图**中同样成立：按日分组时该桶总量 = 汇总。
#[test]
fn grouped_view_uses_same_rollup_total() {
    let dir = temp_dir("grouped");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-1",
            created_at: 1_789_866_000,
            input: 100,
            ..Default::default()
        }],
        &[("2026-09-20", "claude", "claude-opus-5-5", 10, 1000, 0)],
    );
    let history = history_of(&dir);
    import_all(&dir, &history);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    let group = report
        .groups
        .iter()
        .find(|g| g.key == "2026-09-20")
        .expect("按日分组必须包含该日");
    assert_eq!(group.tokens.input, 1000, "分组与总计同源");
    assert_eq!(group.requests, 10);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 2026-11-05：来源证据驱动的覆盖判定（缺陷 1 / 缺陷 2 / 多批次汇总） ──

/// 直接写入一行来源日汇总（`started_utc` = 来源快照建立时刻，可任意指定）。
fn write_rollup(
    history: &HistoryDb,
    provider: &str,
    day: &str,
    model: &str,
    requests: u64,
    input: u64,
    started_utc: &str,
) {
    // 2 = 快照建立时刻语义（新版批次）；旧版 v0.7.0 批次为 1，见
    // `write_rollup_with_revision` 的定向用例。
    write_rollup_with_revision(
        history,
        provider,
        day,
        model,
        requests,
        input,
        started_utc,
        2,
        None,
    );
}

/// 指定批次语义版本的日汇总写入：`import_revision` 1 = v0.7.0 旧语义
/// （started_utc 在读取完成后生成，边界不可信），>= 2 = 快照建立时刻语义。
#[allow(clippy::too_many_arguments)]
fn write_rollup_with_revision(
    history: &HistoryDb,
    provider: &str,
    day: &str,
    model: &str,
    requests: u64,
    input: u64,
    started_utc: &str,
    import_revision: i64,
    source_cost_usd: Option<f64>,
) {
    let tokens = TokenCounts {
        input,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let rollup = DailyRollupWrite {
        logical_source: "ccs".into(),
        day: day.into(),
        source_tz: "Asia/Shanghai".into(),
        app: AgentKind::ClaudeCode,
        provider_id: provider.into(),
        model: model.into(),
        request_model: String::new(),
        pricing_model: String::new(),
        request_count: requests,
        tokens,
        input_semantics: 2,
        source_cost_usd,
        revision: 0,
    };
    let run = ImportRunRecord {
        logical_source: "ccs".into(),
        source_path: "synthetic".into(),
        source_schema: "test".into(),
        started_utc: started_utc.into(),
        status: "committed".into(),
        detail: None,
        import_revision,
    };
    history
        .apply_import_batch(
            &[],
            &[rollup],
            RollupConflictPolicy::KeepExisting,
            &run,
            None,
        )
        .unwrap();
}

/// 直接写入一条 Claude 明细事件：`ccs = true` 表示带 CCS 导入来源证据。
fn write_detail(history: &HistoryDb, key: &str, ts: &str, model: &str, input: u64, ccs: bool) {
    let ts: jiff::Timestamp = ts.parse().unwrap();
    let origins = if ccs {
        vec![EventOrigin {
            origin_kind: ORIGIN_KIND_CCS_REQUEST.into(),
            origin_key: key.into(),
            app: AgentKind::ClaudeCode,
            import_run_id: None,
            parser_revision: 3,
            source_model_raw: Some(model.into()),
            pricing_model: Some(model.into()),
            source_cost_usd: None,
            ts_precision_seconds: true,
        }]
    } else {
        Vec::new()
    };
    history
        .write_batch(&[EventWrite {
            event_key: key.into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: model.into(),
            model_identity: ModelIdentity::parse(model).identity_key(),
            session_id: Some("s1".into()),
            record_id: None,
            project_key: None,
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input,
                output: 0,
                cache_write: 0,
                cache_read: 0,
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
            },
            precedence: if ccs {
                WritePrecedence::CcsImport
            } else {
                WritePrecedence::NativeLog
            },
            observed_at: ts,
            aliases: Vec::new(),
            origins,
        }])
        .unwrap();
}

/// 缺陷 1：汇总 1,000 / 2 次请求，TS 有一条已覆盖的旧明细 100，快照之后
/// 新增一条原生 200。旧判定拿“明细总数 2 == 汇总 2”当未解决，只剩 300；
/// 新判定只把**带来源证据**的旧明细算进覆盖比较 → 1,000 + 200 = 1,200。
#[test]
fn covered_old_detail_plus_new_native_request_counts_rollup() {
    let dir = temp_dir("snapshot-era-plus-new");
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-09-20",
        "claude-opus-5-5",
        2,
        1000,
        "2026-09-20T02:00:00Z",
    );
    // 已覆盖的旧明细（CCS 来源证据）与快照之后新增的原生请求。
    write_detail(
        &history,
        "ccs|claude|proxy|c1",
        "2026-09-20T01:00:00Z",
        "claude-opus-5-5",
        100,
        true,
    );
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|0|fp",
        "2026-09-20T03:00:00Z",
        "claude-opus-5-5",
        200,
        false,
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "汇总 1,000 覆盖旧明细，快照后新增 200 照常计入"
    );
    assert_eq!(report.totals.requests, 3, "2（汇总）+ 1（新增明细）");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 100,
        "被替代的只有带证据的旧明细"
    );
    assert!(
        !report.warnings.iter().any(|w| w.contains("无法确认")),
        "证据充分时不再报未解决：{:?}",
        report.warnings
    );
    assert_eq!(detail_rows(&o), 2, "明细视图仍可查看两条请求");
    std::fs::remove_dir_all(&dir).ok();
}

/// 缺陷 2：快照建立于 02:00（预览读取），提交发生在之后；03:00 的原生请求
/// 不在快照里。旧实现用提交时刻当边界，把它当“已覆盖”静默排除 → 1,000；
/// 新实现只跳过带 CCS 来源证据的明细 → 1,000 + 200 = 1,200。
#[test]
fn native_request_after_preview_is_not_swallowed() {
    let dir = temp_dir("preview-window");
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-09-21",
        "claude-opus-5-5",
        2,
        1000,
        "2026-09-21T02:00:00Z",
    );
    write_detail(
        &history,
        "native|claude|/logs/b.jsonl|0|fp",
        "2026-09-21T03:00:00Z",
        "claude-opus-5-5",
        200,
        false,
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "预览之后到达的原生请求必须计入"
    );
    assert_eq!(report.totals.requests, 3);
    assert_eq!(report.rollup_coverage.covered_buckets, 1);

    // 诊断字段取**来源快照建立时刻**（预览读取），不是提交时刻。
    let rollups = history.daily_rollups(None).unwrap();
    assert_eq!(
        rollups[0].import_at,
        Some("2026-09-21T02:00:00Z".parse().unwrap()),
        "覆盖边界记录的是来源快照建立时刻"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 同一桶含**不同时刻导入**的多行汇总：覆盖起点取**最早**的来源快照建立
/// 时刻；早于两批的明细被替代、晚于两批的新增照常计入，中间的歧义另行披露。
#[test]
fn multi_batch_rollup_rows_use_earliest_snapshot_boundary() {
    let dir = temp_dir("multi-batch");
    let history = history_of(&dir); // 两批导入的同桶汇总（provider 不同 → 两行），来源快照建立时刻不同。
    write_rollup(
        &history,
        "p1",
        "2026-09-22",
        "claude-opus-5-5",
        1,
        400,
        "2026-09-22T01:00:00Z",
    );
    write_rollup(
        &history,
        "p2",
        "2026-09-22",
        "claude-opus-5-5",
        2,
        600,
        "2026-09-22T03:00:00Z",
    );
    // 早于第一批的 CCS 明细；晚于第二批的原生请求。
    write_detail(
        &history,
        "ccs|claude|proxy|c2",
        "2026-09-22T00:30:00Z",
        "claude-opus-5-5",
        80,
        true,
    );
    write_detail(
        &history,
        "native|claude|/logs/c.jsonl|0|fp",
        "2026-09-22T04:00:00Z",
        "claude-opus-5-5",
        300,
        false,
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1300,
        "汇总 400 + 600 覆盖快照内明细 80，快照后新增 300 照常计入"
    );
    assert_eq!(report.totals.requests, 4, "1 + 2（汇总）+ 1（原生）");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(report.rollup_coverage.covered_detail_tokens.input, 80);
    let note = report
        .rollup_coverage
        .multi_batch_boundary
        .as_deref()
        .expect("多批次边界必须如实披露");
    assert!(
        note.contains("不同时刻导入的批次"),
        "披露要说明边界来源：{note}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── 2026-11-05（第二轮）：信任 CCS 日汇总——原生明细也可能已在汇总里 ──
//
// CCS 清理逐条明细只留日汇总时，经 CCS 路由的请求在 TS 侧只剩**原生采集**
// 的记录（没有 ccs_request 标记）。这类请求与"独立新增"在数据上不可区分，
// 按用户约定**信任 CCS 日汇总**：以来源快照建立时刻（started_utc，预览读取）
// 为界，界内的明细视为已包含在汇总中，界外的请求照常计入。

/// 反例：CCS 只留日汇总 1,000；其中一条请求 TS 仍能从本地日志读到（原生
/// 采集、无 CCS 标记）。总量应为 1,000，不是 1,100。
#[test]
fn native_detail_inside_snapshot_is_covered_by_trusted_rollup() {
    let dir = temp_dir("trusted-snapshot-native");
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-09-25",
        "claude-opus-5-5",
        10,
        1000,
        "2026-09-25T02:00:00Z",
    );
    // 真实采集路径：本地会话日志里仍能读到这条请求（不经任何人工来源标记）。
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            "2026-09-25T01:00:00Z",
            "claude-opus-5-5",
            100,
            0,
        )],
    );
    let o = opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1000,
        "信任 CCS 汇总：快照建立之前的原生明细不与汇总相加"
    );
    assert_eq!(report.totals.requests, 10, "请求数取汇总口径");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 100,
        "被汇总替代的明细量如实披露"
    );
    assert_eq!(report.totals.unknown_reason, Some("rollup_only"));
    assert_eq!(detail_rows(&o), 1, "明细视图仍可查看该请求");
    std::fs::remove_dir_all(&dir).ok();
}

/// 对照：同一桶里快照建立之后的独立新增 200 照常计入 → 1,200。
/// 两条明细都来自真实采集路径；边界只看来源快照建立时刻。
#[test]
fn snapshot_era_native_plus_post_snapshot_native_counts_rollup_plus_new() {
    let dir = temp_dir("trusted-plus-new");
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-09-26",
        "claude-opus-5-5",
        2,
        1000,
        "2026-09-26T02:00:00Z",
    );
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[
            claude_line(
                "s1",
                "m1",
                "2026-09-26T01:00:00Z",
                "claude-opus-5-5",
                100,
                0,
            ),
            claude_line(
                "s2",
                "m2",
                "2026-09-26T03:00:00Z",
                "claude-opus-5-5",
                200,
                0,
            ),
        ],
    );
    let o = opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "汇总 1,000（已含快照内原生 100）+ 快照后新增 200"
    );
    assert_eq!(report.totals.requests, 3, "2（汇总）+ 1（新增）");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.unresolved_buckets, 0);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 100,
        "只有快照内的明细被替代"
    );
    assert_eq!(detail_rows(&o), 2, "两条明细都可在明细视图查看");
    std::fs::remove_dir_all(&dir).ok();
}

/// `ccs_request` 只证明请求曾从 CCS 导入，**不自动证明它属于当前日汇总**：
/// 事件时间晚于该汇总的快照建立时刻时（例如来自更新的备份），即便带标记也
/// 必须计入总量，不能被汇总吞掉。
#[test]
fn ccs_marked_detail_after_snapshot_is_counted_not_swallowed() {
    let dir = temp_dir("ccs-marked-after-snapshot");
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-09-28",
        "claude-opus-5-5",
        2,
        1000,
        "2026-09-28T02:00:00Z",
    );
    // 带 CCS 导入来源标记，但事件时间晚于这份汇总的快照时刻。
    write_detail(
        &history,
        "ccs|claude|proxy|c9",
        "2026-09-28T03:00:00Z",
        "claude-opus-5-5",
        200,
        true,
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "晚于快照起点的明细不在汇总里，必须计入"
    );
    assert_eq!(report.totals.requests, 3, "2（汇总）+ 1（明细）");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 0,
        "没有任何明细被替代"
    );
    assert_eq!(detail_rows(&o), 1);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 2026-11-05（第三轮）：覆盖边界必须是**来源快照真正建立**的时刻 ──

/// 追加一行 CCS 日汇总（只插入，不重建库结构；模拟"稍后的第二个导入批次"）。
#[allow(clippy::too_many_arguments)]
fn add_ccs_rollup_row(
    path: &Path,
    day: &str,
    app: &str,
    provider: &str,
    model: &str,
    count: i64,
    input: i64,
    output: i64,
) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute(
        "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
            request_count, input_tokens, output_tokens, cache_read_tokens,
            cache_creation_tokens, total_cost_usd, input_token_semantics)
         VALUES(?1, ?2, ?3, ?4, '', ?5, ?6, ?7, 0, 0, '1.5', 0)",
        rusqlite::params![day, app, provider, model, count, input, output],
    )
    .unwrap();
}

/// 缺陷：`preview.generated_at` 在**读完**来源请求与日汇总之后才生成，晚于
/// 来源 SQLite 快照真正建立的时刻。快照建立之后才到达的原生请求不在已读汇总
/// 里，却被这个更晚的时刻当成"快照内"静默排除。
///
/// 复现走真实 `CcsSource::open` → `ccs::preview` → `ccs::commit` 路径，并把
/// 日志时间取在 `preview.generated_at`（旧口径声称的覆盖边界）上。
#[test]
fn request_arriving_after_snapshot_but_during_preview_is_not_swallowed() {
    let dir = temp_dir("preview-boundary");
    let history = history_of(&dir);
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let day = jiff::Timestamp::now()
        .to_zoned(tz.clone())
        .date()
        .to_string();
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[(day.as_str(), "claude", "claude-opus-5-5", 10, 1000, 0)],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    let generated: jiff::Timestamp = preview.generated_at.parse().unwrap();
    assert_eq!(
        generated.to_zoned(tz).date().to_string(),
        day,
        "事件与汇总必须落在同一来源日"
    );
    // 真实采集路径：日志时间 = 预览读完才生成的时刻（快照建立之后）。
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            &preview.generated_at,
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();

    let report = summary(&opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai")).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "快照建立之后到达的请求不在已读汇总里，必须计入（旧口径 1,000）"
    );
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(
        report.rollup_coverage.covered_detail_tokens.input, 0,
        "没有任何明细可以被这条更晚的时刻替代"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 缺陷：同一日期/模型有**不同时刻导入**的两条汇总时，整桶边界取最晚时刻，
/// 会把落在两次快照之间、且不属于任一汇总的请求整桶吞掉。
#[test]
fn request_between_two_import_snapshots_is_not_swallowed() {
    let dir = temp_dir("two-snapshots");
    let history = history_of(&dir);
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let day = jiff::Timestamp::now()
        .to_zoned(tz.clone())
        .date()
        .to_string();
    let db = dir.join("cc-switch.db");
    seed_ccs(
        &db,
        &[],
        &[(day.as_str(), "claude", "claude-opus-5-5", 1, 400, 0)],
    );
    // 第一批：provider p1 的汇总。
    let s1 = CcsSource::open(&db).unwrap();
    let p1 = ccs::preview(&s1, &history, "Asia/Shanghai").unwrap();
    ccs::commit(
        &p1.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    // 释放第一批的只读事务，否则来源库无法写入第二批汇总。
    drop(s1);
    // 第一批快照建立之后：这条请求不属于第一批的汇总。
    let between = jiff::Timestamp::now();
    // 第二批：同日期/模型、另一个 provider 的行，稍后导入（第二个快照）。
    std::thread::sleep(std::time::Duration::from_millis(5));
    add_ccs_rollup_row(&db, &day, "claude", "p2", "claude-opus-5-5", 2, 600, 0);
    let s2 = CcsSource::open(&db).unwrap();
    let p2 = ccs::preview(&s2, &history, "Asia/Shanghai").unwrap();
    ccs::commit(
        &p2.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();

    let starts: Vec<jiff::Timestamp> = history
        .daily_rollups(None)
        .unwrap()
        .iter()
        .filter_map(|r| r.import_at)
        .collect();
    assert_eq!(starts.len(), 2, "两个批次的汇总各一行");
    let first = *starts.iter().min().unwrap();
    let last = *starts.iter().max().unwrap();
    assert!(
        first < between && between < last,
        "事件必须落在两次快照之间：{first} < {between} < {last}"
    );

    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            &between.to_string(),
            "claude-opus-5-5",
            100,
            0,
        )],
    );
    let report = summary(&opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai")).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1100,
        "两次快照之间到达的请求必须计入（旧口径 1,000）"
    );
    assert_eq!(report.totals.requests, 4, "1 + 2（汇总）+ 1（明细）");
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.covered_detail_tokens.input, 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 更新来源库里的日汇总行（同主键 UPDATE），模拟 CCS 汇总随时间增长。
fn update_ccs_rollup_row(
    path: &Path,
    day: &str,
    provider: &str,
    model: &str,
    count: i64,
    input: i64,
    output: i64,
) {
    let conn = rusqlite::Connection::open(path).unwrap();
    let n = conn
        .execute(
            "UPDATE usage_daily_rollups SET request_count = ?5, input_tokens = ?6,
                output_tokens = ?7
             WHERE date = ?1 AND app_type = 'claude' AND provider_id = ?3 AND model = ?4
               AND request_model = '' AND pricing_model = ''",
            rusqlite::params![day, "claude", provider, model, count, input, output],
        )
        .unwrap();
    assert_eq!(n, 1, "来源汇总行必须存在且被更新");
}

/// 重新导入且内容变化（用户明确"以来源为准"）：该行必须重新绑定到**本次**
/// 批次，边界随新快照前移——已含于新汇总的 200 不再重复计入。
#[test]
fn reimport_replacing_rollup_content_rebinds_boundary_to_new_batch() {
    let dir = temp_dir("reimport-rebind");
    let history = history_of(&dir);
    let tz = "Asia/Shanghai";
    let day = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get(tz).unwrap())
        .date()
        .to_string();
    let db = dir.join("cc-switch.db");
    seed_ccs(
        &db,
        &[],
        &[(day.as_str(), "claude", "claude-opus-5-5", 1, 1000, 0)],
    );
    // 第一次导入：汇总 1,000 / 1 请求。
    let s1 = CcsSource::open(&db).unwrap();
    let p1 = ccs::preview(&s1, &history, tz).unwrap();
    ccs::commit(
        &p1.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    drop(s1);
    let first = history.daily_rollups(None).unwrap()[0].import_at.unwrap();
    // 之后 TS 采集一条 200 的原生请求（晚于第一次快照）。
    let after_first = jiff::Timestamp::now();
    assert!(after_first > first, "事件时间必须晚于第一批快照");
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            &after_first.to_string(),
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    // CCS 汇总增长为 1,200 / 2 请求（已包含这条请求），再次导入并选"以来源为准"。
    std::thread::sleep(std::time::Duration::from_millis(5));
    update_ccs_rollup_row(&db, &day, "p1", "claude-opus-5-5", 2, 1200, 0);
    let s2 = CcsSource::open(&db).unwrap();
    let p2 = ccs::preview(&s2, &history, tz).unwrap();
    ccs::commit(
        &p2.plan_id,
        &history,
        RollupConflictPolicy::TakeSource,
        true,
    )
    .unwrap();
    drop(s2);

    let rollups = history.daily_rollups(None).unwrap();
    assert_eq!(rollups.len(), 1, "同主键替换，不新增行");
    let second = rollups[0].import_at.unwrap();
    assert!(
        second > after_first,
        "内容被替换后必须绑定本次批次（边界前移）：{first} / {second}"
    );
    let report = summary(&opts(&dir, Some(claude), GroupBy::Day, tz)).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "新汇总已包含这条 200，不能再加一次（旧实现 1,400）"
    );
    assert_eq!(report.totals.requests, 2);
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// 内容未变的重复导入**不得**把旧汇总的边界改到更晚：否则批次之间到达的
/// 请求会被新边界吞掉。边界保持第一次导入的时刻，200 仍按明细计入。
#[test]
fn reimport_with_same_content_keeps_original_boundary() {
    let dir = temp_dir("reimport-same");
    let history = history_of(&dir);
    let tz = "Asia/Shanghai";
    let day = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get(tz).unwrap())
        .date()
        .to_string();
    let db = dir.join("cc-switch.db");
    seed_ccs(
        &db,
        &[],
        &[(day.as_str(), "claude", "claude-opus-5-5", 1, 1000, 0)],
    );
    let s1 = CcsSource::open(&db).unwrap();
    let p1 = ccs::preview(&s1, &history, tz).unwrap();
    ccs::commit(
        &p1.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    drop(s1);
    let first = history.daily_rollups(None).unwrap()[0].import_at.unwrap();
    let after_first = jiff::Timestamp::now();
    assert!(after_first > first);
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            &after_first.to_string(),
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    // 原样再次导入（快照内容不变）：即便选"以来源为准"也不应改写边界。
    std::thread::sleep(std::time::Duration::from_millis(5));
    let s2 = CcsSource::open(&db).unwrap();
    let p2 = ccs::preview(&s2, &history, tz).unwrap();
    assert_eq!(p2.rollups_unchanged, 1, "内容未变应计入幂等");
    ccs::commit(
        &p2.plan_id,
        &history,
        RollupConflictPolicy::TakeSource,
        true,
    )
    .unwrap();
    drop(s2);

    assert_eq!(
        history.daily_rollups(None).unwrap()[0].import_at,
        Some(first),
        "内容未变时边界不能被改成更晚的批次时间"
    );
    let report = summary(&opts(&dir, Some(claude), GroupBy::Day, tz)).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1200,
        "批次之间到达的 200 仍按明细计入"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// v0.7.0 的批次 `started_utc` 在读取结束后才生成，语义与新版"快照建立时刻"
/// 不同，而旧库无法事后区分 → 不能当可靠边界：整桶按信任口径统计并如实披露。
#[test]
fn legacy_v070_boundary_is_unknown_and_disclosed() {
    let dir = temp_dir("legacy-boundary");
    {
        let history = history_of(&dir);
        // 旧版批次：import_revision = 1，时刻是"读取完成"时间。
        write_rollup_with_revision(
            &history,
            "p1",
            "2026-10-03",
            "claude-opus-5-5",
            10,
            1000,
            "2026-10-03T02:00:00Z",
            1,
            None,
        );
    }
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            "2026-10-03T03:00:00Z",
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    let report = summary(&opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai")).unwrap();
    assert_eq!(
        report.totals.tokens.input, 1000,
        "旧批次时间不是可靠边界：不拿它判定（旧实现会算成 1,200）"
    );
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    let note = report
        .rollup_coverage
        .legacy_boundary
        .as_deref()
        .expect("旧版批次边界不可确认时必须披露");
    assert!(note.contains("旧版导入"), "披露要说明来源：{note}");

    // 重启（重新打开历史库）后结论与披露一致，不因进程重启漂移。
    let again = summary(&opts(
        &dir,
        Some(dir.join("claude")),
        GroupBy::Day,
        "Asia/Shanghai",
    ))
    .unwrap();
    assert_eq!(again.totals.tokens.input, 1000);
    assert!(again.rollup_coverage.legacy_boundary.is_some());
    std::fs::remove_dir_all(&dir).ok();
}

/// 对照：同一时间布局但批次为新语义（`import_revision = 2`）→ 边界可靠，
/// 快照之后到达的 200 照常计入、且不产生旧版披露。
#[test]
fn known_boundary_counts_request_after_snapshot() {
    let dir = temp_dir("known-boundary");
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-10-03",
        "claude-opus-5-5",
        10,
        1000,
        "2026-10-03T02:00:00Z",
    );
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            "2026-10-03T03:00:00Z",
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    let report = summary(&opts(&dir, Some(claude), GroupBy::Day, "Asia/Shanghai")).unwrap();
    assert_eq!(report.totals.tokens.input, 1200, "快照之后的 200 必须计入");
    assert!(report.rollup_coverage.legacy_boundary.is_none());
    std::fs::remove_dir_all(&dir).ok();
}

/// 旧版批次只有在**来源内容真的变化**后才重新绑定批次（边界转为可确认）；
/// 内容未变的重复导入不改写边界，披露继续存在。
#[test]
fn legacy_boundary_becomes_known_only_when_content_changes() {
    let dir = temp_dir("legacy-repair");
    let history = history_of(&dir);
    let tz = "Asia/Shanghai";
    let day = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::get(tz).unwrap())
        .date()
        .to_string();
    let db = dir.join("cc-switch.db");
    // 来源行与历史行同形态（request_model / pricing_model 为空），才能被判定为
    // 同一行、走"内容未变"分支。
    seed_ccs(&db, &[], &[]);
    add_ccs_rollup_row(&db, &day, "claude", "p1", "claude-opus-5-5", 1, 1000, 0);
    // 历史库里先有同内容的旧版批次（v0.7.0 导入遗留）。
    write_rollup_with_revision(
        &history,
        "p1",
        &day,
        "claude-opus-5-5",
        1,
        1000,
        "2026-10-03T02:00:00Z",
        1,
        Some(1.5),
    );
    let claude = dir.join("claude");
    let after = jiff::Timestamp::now();
    write_claude_log(
        &claude,
        "proj/s.jsonl",
        &[claude_line(
            "s1",
            "m1",
            &after.to_string(),
            "claude-opus-5-5",
            200,
            0,
        )],
    );
    // 内容未变的重复导入：不改边界，仍是"不可确认"。
    let s1 = CcsSource::open(&db).unwrap();
    let p1 = ccs::preview(&s1, &history, tz).unwrap();
    assert_eq!(p1.rollups_unchanged, 1);
    ccs::commit(
        &p1.plan_id,
        &history,
        RollupConflictPolicy::TakeSource,
        true,
    )
    .unwrap();
    drop(s1);
    let same_content = summary(&opts(&dir, Some(claude.clone()), GroupBy::Day, tz)).unwrap();
    assert_eq!(same_content.totals.tokens.input, 1000);
    assert!(same_content.rollup_coverage.legacy_boundary.is_some());

    // 来源内容更新后重新导入（以来源为准）：该行绑定新批次，边界可确认。
    std::thread::sleep(std::time::Duration::from_millis(5));
    update_ccs_rollup_row(&db, &day, "p1", "claude-opus-5-5", 2, 1200, 0);
    let s2 = CcsSource::open(&db).unwrap();
    let p2 = ccs::preview(&s2, &history, tz).unwrap();
    ccs::commit(
        &p2.plan_id,
        &history,
        RollupConflictPolicy::TakeSource,
        true,
    )
    .unwrap();
    drop(s2);
    let repaired = summary(&opts(&dir, Some(claude), GroupBy::Day, tz)).unwrap();
    assert_eq!(repaired.totals.tokens.input, 1200, "新汇总已含 200");
    assert!(
        repaired.rollup_coverage.legacy_boundary.is_none(),
        "重绑新批次后不再有旧版披露"
    );
    std::fs::remove_dir_all(&dir).ok();
}
