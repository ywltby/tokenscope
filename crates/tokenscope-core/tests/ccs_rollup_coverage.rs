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
    EventAlias, EventOrigin, EventWrite, HistoryDb, RollupConflictPolicy, WritePrecedence,
};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::{AgentKind, TokenCounts};
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

/// 无法确认覆盖：明细请求数不少于汇总时保留明细、明确提示，不加也不猜。
#[test]
fn detail_not_fewer_than_rollup_stays_unresolved_with_warning() {
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
    assert_eq!(report.totals.tokens.input, 300, "保留明细，不把汇总相加");
    assert_eq!(report.rollup_coverage.unresolved_buckets, 1);
    assert_eq!(report.rollup_coverage.covered_buckets, 0);
    assert!(
        report.warnings.iter().any(|w| w.contains("无法确认")),
        "无法确认覆盖必须如实提示：{:?}",
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
