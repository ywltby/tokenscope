//! 2026-11-07：CCS 日汇总桶的**统计来源三向选择**回归。
//!
//! 口径见 `docs/stats-semantics.md` §5.1。三个真实桶只用于定位，本文件全部
//! 使用缩小后的**合成**数字、合成模型名、合成日期与临时目录；不复制真实日志、
//! 会话标识、来源路径，也不读写用户真实 history.db / CCS 库。
//!
//! 三向选择（同日 / 同应用 / 等价模型 / 来源时区 / 快照范围内明细统计）：
//! - A 明细逐项覆盖汇总（含完全相等）→ 按请求明细统计，汇总不叠加；
//! - B 汇总补足快照内明细的历史 → 信任汇总，快照之后新增照常计入；
//! - C 请求数与四桶方向交叉 → 按请求明细回退，给出明确冲突类型。
//!
//! 断言同时覆盖普通查询 `summary()` 与冻结快照查询
//! `query_begin` / `query_summary` / `query_events` 两条实际入口，避免口径分叉。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{
    DailyRollupWrite, EventOrigin, EventWrite, HistoryDb, ImportRunRecord, ORIGIN_KIND_CCS_REQUEST,
    RollupConflictPolicy, WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

const TZ: &str = "Asia/Shanghai";
const DAY: &str = "2026-09-18";
const MODEL: &str = "synth-model-a";
/// 来源快照建立时刻（UTC）；快照内 = 事件时间不晚于该时刻。
const SNAPSHOT_AT: &str = "2026-09-18T06:00:00Z";
const PROJECT: &str = "/synth/project-a";

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-rollup-select-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn history_of(dir: &Path) -> HistoryDb {
    HistoryDb::open(&dir.join("data").join("history.db")).unwrap()
}

/// 密闭查询选项：cache_dir / pricing_index 等一律注入临时目录，并为模型写入
/// 合成价格表，使"按明细统计"与"降为日粒度"在费用上可区分。
fn opts(dir: &Path, by: GroupBy) -> SummaryOptions {
    // 空目录：避免"来源目录不存在"这类**真实采集警告**混入被测断言
    // （真实 IO 错误/坏行仍提示由前端用例独立覆盖）。
    std::fs::create_dir_all(dir.join("no-claude-logs")).unwrap();
    std::fs::write(
        dir.join("pricing.toml"),
        format!(
            "[[model]]\nprefix = \"{MODEL}\"\ninput = 3.0\noutput = 15.0\n\
             cache_write = 3.75\ncache_read = 0.3\n"
        ),
    )
    .unwrap();
    SummaryOptions {
        by,
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        claude_dir: Some(dir.join("no-claude-logs")),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some(TZ.to_string()),
        ..Default::default()
    }
}

fn tc(input: u64, output: u64, cache_write: u64, cache_read: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write,
        cache_read,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

/// 合成一行日汇总（四桶、请求数、批次语义版本均可指定）。
#[allow(clippy::too_many_arguments)]
fn write_rollup(
    history: &HistoryDb,
    day: &str,
    requests: u64,
    tokens: TokenCounts,
    started_utc: &str,
    import_revision: i64,
) {
    let rollup = DailyRollupWrite {
        logical_source: "ccs".into(),
        day: day.into(),
        source_tz: TZ.into(),
        app: AgentKind::ClaudeCode,
        provider_id: "p1".into(),
        model: MODEL.into(),
        request_model: String::new(),
        pricing_model: String::new(),
        request_count: requests,
        tokens,
        input_semantics: 2,
        source_cost_usd: None,
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

/// 合成一条 Claude 请求明细；`ccs` 仅用于反例（口径不依赖来源标记）。
fn write_detail(history: &HistoryDb, key: &str, ts: &str, tokens: TokenCounts, ccs: bool) {
    let ts: jiff::Timestamp = ts.parse().unwrap();
    let origins: Vec<EventOrigin> = if ccs {
        vec![EventOrigin {
            origin_kind: ORIGIN_KIND_CCS_REQUEST.into(),
            origin_key: key.into(),
            app: AgentKind::ClaudeCode,
            import_run_id: None,
            parser_revision: 3,
            source_model_raw: Some(MODEL.into()),
            pricing_model: Some(MODEL.into()),
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
            model_raw: MODEL.into(),
            model_identity: ModelIdentity::parse(MODEL).identity_key(),
            session_id: Some("synthetic-session".into()),
            record_id: Some(key.into()),
            project_key: Some(PROJECT.into()),
            session_initial_cwd: Some(PROJECT.into()),
            event_cwd: Some(PROJECT.into()),
            tokens,
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

/// 把汇总行的批次关联清空（旧库迁移 / 早期导入遗留：边界不可确认）。
fn detach_batch(dir: &Path) {
    let conn = rusqlite::Connection::open(dir.join("data").join("history.db")).unwrap();
    conn.execute("UPDATE ccs_daily_usage SET import_run_id = NULL", [])
        .unwrap();
}

fn detail_rows(o: &SummaryOptions) -> u64 {
    let snapshot = query::begin_query(o).unwrap();
    query::query_events(&snapshot.query_id, &EventFilter::default())
        .unwrap()
        .total
}

/// 1. 请求数相同、明细四桶逐项不低于汇总且部分更大 → 采用明细，无冲突警告。
#[test]
fn detail_dominates_each_bucket_selects_detail_without_conflict() {
    let dir = temp_dir("dominates");
    let history = history_of(&dir);
    // 汇总：2 请求 / (100, 10, 20, 30)；明细：2 请求 / (100, 14, 25, 36)。
    write_rollup(&history, DAY, 2, tc(100, 10, 20, 30), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|a|1",
        "2026-09-18T01:00:00Z",
        tc(50, 7, 12, 18),
        false,
    );
    write_detail(
        &history,
        "native|a|2",
        "2026-09-18T02:00:00Z",
        tc(50, 7, 13, 18),
        false,
    );

    let o = opts(&dir, GroupBy::Day);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 2, "请求数取明细");
    assert_eq!(
        r.totals.tokens,
        tc(100, 14, 25, 36),
        "四桶取明细，汇总不叠加"
    );
    assert_eq!(r.totals.unknown_reason, None, "明细可按请求级计价");
    assert_eq!(
        r.rollup_coverage.detail_selected_buckets, 1,
        "计入 A 类选择"
    );
    assert_eq!(r.rollup_coverage.unresolved_buckets, 0, "A 类不计交叉冲突");
    assert!(r.rollup_coverage.conflict_kinds.is_empty());
    assert_eq!(r.rollup_coverage.covered_buckets, 0, "不产生被替代提示");
    assert_eq!(r.rollup_coverage.covered_detail_tokens.total(), 0);
    assert!(r.rollup_coverage.legacy_boundary.is_none());
    assert!(r.rollup_coverage.multi_batch_boundary.is_none());
    assert!(
        !r.warnings
            .iter()
            .any(|w| w.contains("未纳入") || w.contains("无法确认")),
        "正常来源选择不得报覆盖冲突：{:?}",
        r.warnings
    );
    assert_eq!(detail_rows(&o), 2, "明细请求全部保留");
    std::fs::remove_dir_all(&dir).ok();
}

/// 2. 四桶完全相同、明细含额外零 token 请求 → 保留明细请求数，不误报 token 冲突。
#[test]
fn equal_tokens_with_extra_zero_usage_requests_keep_detail_count() {
    let dir = temp_dir("zero-extra");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 3, tc(50, 5, 10, 15), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|z|1",
        "2026-09-18T01:00:00Z",
        tc(30, 3, 6, 9),
        false,
    );
    write_detail(
        &history,
        "native|z|2",
        "2026-09-18T02:00:00Z",
        tc(20, 2, 4, 6),
        false,
    );
    for n in 3..=5 {
        write_detail(
            &history,
            &format!("native|z|{n}"),
            &format!("2026-09-18T0{n}:00:00Z"),
            tc(0, 0, 0, 0),
            false,
        );
    }

    let o = opts(&dir, GroupBy::Day);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 5, "零用量请求也是真实请求，不被删除");
    assert_eq!(r.totals.tokens, tc(50, 5, 10, 15), "四桶与汇总一致");
    assert_eq!(r.rollup_coverage.detail_selected_buckets, 1);
    assert_eq!(r.rollup_coverage.unresolved_buckets, 0, "同量数据不是冲突");
    assert!(r.rollup_coverage.conflict_kinds.is_empty());
    assert_eq!(r.rollup_coverage.covered_buckets, 0);
    assert!(r.rollup_coverage.legacy_boundary.is_none());
    assert!(
        !r.warnings.iter().any(|w| w.contains("未纳入")),
        "不得误报 token 冲突：{:?}",
        r.warnings
    );
    assert_eq!(detail_rows(&o), 5);
    std::fs::remove_dir_all(&dir).ok();
}

/// 3. 请求数、四桶完全相同 + 旧批次 / 无批次关联 → 选明细，无旧边界与被替代提示。
///
/// 分别覆盖 revision=1（旧版批次语义）与 `import_run_id` 为空两种历史形态。
#[test]
fn identical_bucket_prefers_detail_for_legacy_and_unlinked_batches() {
    for (tag, revision, detach) in [("old-revision", 1, false), ("no-batch", 2, true)] {
        let dir = temp_dir(tag);
        let history = history_of(&dir);
        write_rollup(&history, DAY, 1, tc(20, 2, 3, 4), SNAPSHOT_AT, revision);
        write_detail(
            &history,
            "native|eq|1",
            "2026-09-18T01:00:00Z",
            tc(20, 2, 3, 4),
            false,
        );
        if detach {
            detach_batch(&dir);
        }
        let o = opts(&dir, GroupBy::Day);
        let r = summary(&o).unwrap();
        assert_eq!(r.totals.requests, 1, "{tag}：请求数取明细");
        assert_eq!(r.totals.tokens, tc(20, 2, 3, 4), "{tag}：四桶取明细");
        assert_eq!(
            r.totals.unknown_reason, None,
            "{tag}：完全相等不得降为日粒度（费用未知）"
        );
        assert_eq!(r.rollup_coverage.detail_selected_buckets, 1, "{tag}");
        assert_eq!(r.rollup_coverage.unresolved_buckets, 0, "{tag}");
        assert_eq!(
            r.rollup_coverage.covered_buckets, 0,
            "{tag}：不产生被替代提示"
        );
        assert_eq!(r.rollup_coverage.covered_detail_tokens.total(), 0, "{tag}");
        assert!(
            r.rollup_coverage.legacy_boundary.is_none(),
            "{tag}：不产生旧边界提示"
        );
        assert!(r.rollup_coverage.multi_batch_boundary.is_none(), "{tag}");
        assert!(
            r.warnings.is_empty(),
            "{tag}：正常来源选择不告警：{:?}",
            r.warnings
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// 4. 完全相等但明细可计价 → 费用、项目、请求详情不因选择汇总而降级。
#[test]
fn identical_bucket_keeps_request_level_pricing_and_project() {
    let dir = temp_dir("priced");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 1, tc(20, 2, 3, 4), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|priced|1",
        "2026-09-18T01:00:00Z",
        tc(20, 2, 3, 4),
        false,
    );

    let o = opts(&dir, GroupBy::Project);
    let r = summary(&o).unwrap();
    assert!(
        r.totals.cost_usd > 0.0,
        "请求级计价必须保留：{}",
        r.totals.cost_usd
    );
    assert_eq!(r.totals.unknown_reason, None);
    assert_eq!(
        r.totals.unknown_tokens.total(),
        0,
        "汇总的未知 token 不得混入"
    );
    assert!(
        r.groups.iter().any(|g| g.key == PROJECT),
        "项目分组必须保留：{:?}",
        r.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
    );

    // 请求详情（费用、分项、项目）逐字段可核对。
    let snapshot = query::begin_query(&o).unwrap();
    let page = query::query_events(&snapshot.query_id, &EventFilter::default()).unwrap();
    assert_eq!(page.total, 1);
    let row = &page.rows[0];
    assert_eq!(row.project, PROJECT);
    assert!(row.cost_usd.is_some_and(|c| c > 0.0), "明细行费用未降级");
    assert!(row.cost_breakdown.is_some(), "请求级计价明细必须保留");
    std::fs::remove_dir_all(&dir).ok();
}

/// 5. B 类保持：汇总补足部分明细；快照之后新增请求照常叠加。
#[test]
fn rollup_supplements_partial_detail_and_post_snapshot_request() {
    let dir = temp_dir("supplement");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 10, tc(1000, 100, 200, 300), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|old|1",
        "2026-09-18T01:00:00Z",
        tc(100, 10, 20, 30),
        false,
    );
    write_detail(
        &history,
        "native|new|1",
        "2026-09-18T07:00:00Z",
        tc(200, 20, 40, 60),
        false,
    );

    let o = opts(&dir, GroupBy::Day);
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 11, "10（汇总）+ 1（快照后新增）");
    assert_eq!(
        r.totals.tokens,
        tc(1200, 120, 240, 360),
        "汇总 1000 + 快照后新增 200，明细不重复计入"
    );
    assert_eq!(r.rollup_coverage.covered_buckets, 1, "B 类仍信任汇总");
    assert_eq!(
        r.rollup_coverage.covered_detail_tokens,
        tc(100, 10, 20, 30),
        "被替代的仅快照内明细"
    );
    assert_eq!(r.rollup_coverage.detail_selected_buckets, 0);
    assert_eq!(r.rollup_coverage.unresolved_buckets, 0);
    assert!(r.rollup_coverage.conflict_kinds.is_empty());
    assert_eq!(detail_rows(&o), 2, "明细视图仍可查看两条请求");
    std::fs::remove_dir_all(&dir).ok();
}

/// 5b. 快照之后新增请求**不得**用来证明旧汇总已被明细覆盖：
/// 全量明细四桶已经大于汇总，但快照内明细仍明显不足 → 仍属 B 类。
#[test]
fn post_snapshot_requests_do_not_prove_old_rollup_is_covered() {
    let dir = temp_dir("no-post-proof");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 10, tc(1000, 100, 200, 300), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|in|1",
        "2026-09-18T01:00:00Z",
        tc(10, 1, 2, 3),
        false,
    );
    // 快照之后新增 3 条，使全量明细四桶均超过汇总。
    for (n, t) in [
        tc(600, 60, 120, 180),
        tc(600, 60, 120, 180),
        tc(600, 60, 120, 180),
    ]
    .into_iter()
    .enumerate()
    {
        write_detail(
            &history,
            &format!("native|post|{n}"),
            &format!("2026-09-18T{:02}:00:00Z", 7 + n),
            t,
            false,
        );
    }

    let o = opts(&dir, GroupBy::Day);
    let r = summary(&o).unwrap();
    assert_eq!(
        r.totals.requests, 13,
        "10（汇总）+ 3（快照后新增），不是全量明细的 4"
    );
    assert_eq!(
        r.totals.tokens,
        tc(2800, 280, 560, 840),
        "汇总 1000 + 快照后 1800，旧汇总不得被丢弃"
    );
    assert_eq!(r.rollup_coverage.covered_buckets, 1);
    assert_eq!(r.rollup_coverage.detail_selected_buckets, 0);
    assert_eq!(r.rollup_coverage.unresolved_buckets, 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 6. C 类交叉冲突：四桶方向交叉、请求数与用量方向相反，均须保留明细并给出明确类型。
#[test]
fn crossed_buckets_conflict_keeps_explicit_typed_diagnostic() {
    // (a) 明细 input 更大、output 更小。
    let dir = temp_dir("crossed");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 2, tc(100, 10, 20, 30), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|cross|1",
        "2026-09-18T01:00:00Z",
        tc(120, 5, 20, 30),
        false,
    );
    let r = summary(&opts(&dir, GroupBy::Day)).unwrap();
    assert_eq!(
        r.totals.tokens,
        tc(120, 5, 20, 30),
        "回退到明细，不逐字段取最大"
    );
    assert_eq!(
        r.rollup_coverage.unresolved_buckets, 1,
        "交叉冲突不得被新分支吞掉"
    );
    assert_eq!(
        r.rollup_coverage.conflict_kinds,
        vec!["crossed_token_buckets".to_string()]
    );
    assert_eq!(r.rollup_coverage.detail_selected_buckets, 0);
    assert_eq!(r.rollup_coverage.covered_buckets, 0);
    assert!(
        !r.warnings
            .iter()
            .any(|w| w.contains("未纳入") || w.contains("无法确认汇总能覆盖")),
        "不得用『未纳入若干 token』暗示全丢：{:?}",
        r.warnings
    );
    std::fs::remove_dir_all(&dir).ok();

    // (b) 请求数更多但用量更少。
    let dir = temp_dir("opposed");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 3, tc(500, 0, 0, 0), SNAPSHOT_AT, 2);
    for n in 1..=4 {
        write_detail(
            &history,
            &format!("native|opp|{n}"),
            &format!("2026-09-18T0{n}:00:00Z"),
            tc(100, 0, 0, 0),
            false,
        );
    }
    let r = summary(&opts(&dir, GroupBy::Day)).unwrap();
    assert_eq!(r.totals.requests, 4);
    assert_eq!(
        r.totals.tokens,
        tc(400, 0, 0, 0),
        "按明细统计，不捏造差额请求"
    );
    assert_eq!(r.rollup_coverage.unresolved_buckets, 1);
    assert_eq!(
        r.rollup_coverage.conflict_kinds,
        vec!["opposed_requests_tokens".to_string()]
    );
    assert_eq!(r.rollup_coverage.detail_selected_buckets, 0);
    assert!(!r.warnings.iter().any(|w| w.contains("未纳入")));
    std::fs::remove_dir_all(&dir).ok();
}

/// 8. 普通查询与冻结快照查询、分组、明细分页、会话重建与重复导入保持一致。
#[test]
fn selection_is_consistent_across_both_query_paths_and_restart() {
    let dir = temp_dir("consistency");
    let history = history_of(&dir);
    write_rollup(&history, DAY, 2, tc(100, 10, 20, 30), SNAPSHOT_AT, 2);
    write_detail(
        &history,
        "native|c|1",
        "2026-09-18T01:00:00Z",
        tc(50, 7, 12, 18),
        false,
    );
    write_detail(
        &history,
        "native|c|2",
        "2026-09-18T02:00:00Z",
        tc(50, 7, 13, 18),
        false,
    );

    let expected_tokens = tc(100, 14, 25, 36);
    let expected = |r: &tokenscope::report::SummaryReport| {
        assert_eq!(r.totals.requests, 2);
        assert_eq!(r.totals.tokens, expected_tokens);
        assert_eq!(r.rollup_coverage.detail_selected_buckets, 1);
        assert_eq!(r.rollup_coverage.unresolved_buckets, 0);
        assert_eq!(r.rollup_coverage.covered_buckets, 0);
    };

    // 普通查询路径：summary() 内部建快照后聚合。
    let o = opts(&dir, GroupBy::Day);
    expected(&summary(&o).unwrap());

    // 冻结快照路径（GUI 实际入口）：一次 begin，多次读汇总/明细。
    let snapshot = query::begin_query(&o).unwrap();
    let frozen = query::query_summary(&snapshot.query_id).unwrap();
    expected(&frozen);
    let page1 = query::query_events(
        &snapshot.query_id,
        &EventFilter {
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, 2, "明细分页 total 与汇总同源");
    let page2 = query::query_events(
        &snapshot.query_id,
        &EventFilter {
            limit: Some(1),
            before: Some(page1.rows[0].cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.total, 2, "翻页不重新筛选、total 不变");
    assert_ne!(page1.rows[0].record_id, page2.rows[0].record_id);

    // 各分组口径一致（桶选择在快照建立时固定一次）。
    for by in [GroupBy::Model, GroupBy::Project, GroupBy::Agent] {
        expected(&summary(&opts(&dir, by)).unwrap());
    }

    // 会话重建（模拟重启后新建查询）：不复用旧 query_id，数字一致。
    query::clear_query_registry_for_tests();
    let reopened = history_of(&dir);
    drop(reopened);
    expected(&summary(&opts(&dir, GroupBy::Day)).unwrap());

    // 同内容重复导入不得重新绑定批次、不得改变统计来源。
    write_rollup(&history, DAY, 2, tc(100, 10, 20, 30), SNAPSHOT_AT, 2);
    expected(&summary(&opts(&dir, GroupBy::Day)).unwrap());
    std::fs::remove_dir_all(&dir).ok();
}
