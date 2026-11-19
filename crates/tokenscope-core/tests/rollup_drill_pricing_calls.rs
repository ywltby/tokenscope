//! v0.9.4 独立复核补修三：CCS 日汇总下钻行的分组/顺序/游标定位索引绑定
//! 冻结查询快照，费用只对**落到当前页**的日行按需计算并在同快照内复用。
//!
//! 计划：docs/plans/active/2026-11-19-v094-model-alias-rollup-drilldown.md §8.3。
//! 红-first：旧实现每页 `drill_rollup_rows` 重建并逐条计价全部日行，带游标
//! 时 `locate_cursor` 再建一次——用 `rollup_cost_parts` 的实际调用次数证明。
//!
//! 计数是进程级全局量，本文件所有测试用同一把锁串行执行（其他测试文件是
//! 独立进程，互不干扰）。全部 fixture 合成、临时目录，不触碰真实用户数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use tokenscope::aggregate::{GroupBy, take_rollup_cost_calls_for_tests};
use tokenscope::history::{
    DailyRollupWrite, EventWrite, HistoryDb, ImportRunRecord, RollupConflictPolicy, WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::query;
use tokenscope::report::{EventFilter, EventRow, SummaryOptions};

static SEQ: AtomicU64 = AtomicU64::new(0);
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-rollup-calls-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn opts(dir: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Model,
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        claude_dir: Some(dir.join("no-claude")),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".into()),
        ..Default::default()
    }
}

fn write_rollup(
    history: &HistoryDb,
    day: &str,
    requests: u64,
    tokens: TokenCounts,
    source_cost_usd: Option<f64>,
) {
    let rollup = DailyRollupWrite {
        logical_source: "ccs".into(),
        day: day.into(),
        source_tz: "Asia/Shanghai".into(),
        app: AgentKind::ClaudeCode,
        provider_id: "p1".into(),
        model: "claude-opus-4-7".into(),
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
        started_utc: format!("{day}T12:00:00Z"),
        status: "committed".into(),
        detail: None,
        import_revision: 2,
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

fn write_detail(history: &HistoryDb, key: &str, ts: &str, tokens: TokenCounts) {
    let ts: jiff::Timestamp = ts.parse().unwrap();
    history
        .write_batch(&[EventWrite {
            event_key: key.into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: "claude-opus-4-7".into(),
            model_identity: ModelIdentity::parse("claude-opus-4-7").identity_key(),
            session_id: Some("s1".into()),
            record_id: None,
            project_key: None,
            session_initial_cwd: None,
            event_cwd: None,
            tokens,
            precedence: WritePrecedence::NativeLog,
            observed_at: ts,
            aliases: Vec::new(),
            origins: Vec::new(),
        }])
        .unwrap();
}

/// 3 条原生请求（快照后，独立计入）+ 4 天 CCS 日汇总（**无来源费用**，
/// 因此每条日行都要真正走估算计价）。合计 7 行：3 事件 + 4 日行。
fn setup(dir: &Path) {
    std::fs::write(
        dir.join("md.json"),
        serde_json::json!({
            "v": 4,
            "synced_at": "2026-11-19T00:00:00Z",
            "entries": [{
                "id": "anthropic/claude-opus-4-7",
                "name": "Claude Opus 4.7",
                "input": 3.0, "output": 15.0, "cache_write": 3.75, "cache_read": 0.3
            }]
        })
        .to_string(),
    )
    .unwrap();
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    for (i, day) in ["2026-05-25", "2026-05-26", "2026-05-27", "2026-05-28"]
        .iter()
        .enumerate()
    {
        let n = (i as u64 + 1) * 10;
        write_rollup(
            &history,
            day,
            n,
            counts(n * 1000, n * 100, n * 10, n * 20),
            None,
        );
    }
    for (i, ts) in [
        "2026-05-29T01:00:00Z",
        "2026-05-29T02:00:00Z",
        "2026-05-29T03:00:00Z",
    ]
    .iter()
    .enumerate()
    {
        write_detail(
            &history,
            &format!("native|claude|/logs/a.jsonl|{i}|fp"),
            ts,
            counts(100, 0, 0, 0),
        );
    }
}

fn is_rollup(row: &EventRow) -> bool {
    serde_json::to_value(row).unwrap().get("rollup").is_some()
}

fn cursor_field(cursor: &str, field: &str) -> serde_json::Value {
    serde_json::from_str::<serde_json::Value>(cursor).unwrap()[field].clone()
}

/// 事件首页不为未展示的日行计价：limit=2/3 都只返回事件行 → 0 次计价。
/// 旧实现每页重建并计价全部 4 条日行（红：4）。
#[test]
fn event_first_page_does_not_price_hidden_rollup_rows() {
    let _g = serial();
    let dir = temp_dir("first-page");
    setup(&dir);
    let o = opts(&dir);
    let snapshot = query::begin_query(&o).unwrap();
    // 建快照只做行索引与覆盖选择（含日行分组索引），不为任何日行计价。
    let at_build = take_rollup_cost_calls_for_tests();
    assert_eq!(at_build, 0, "建快照阶段不得计价日行");

    for limit in [2usize, 3] {
        let list = query::query_events(
            &snapshot.query_id,
            &EventFilter {
                limit: Some(limit),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(list.total, 7, "3 事件 + 4 日行");
        assert_eq!(list.rows.len(), limit);
        assert!(list.rows.iter().all(|r| !is_rollup(r)), "首页只有事件行");
        assert_eq!(
            take_rollup_cost_calls_for_tests(),
            0,
            "limit={limit} 的事件首页不得为未展示日行计价"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 跨入日行段只为页内行计价；重复取同一页复用已算结果（0 次）；
/// 汇总游标定位本身不触发计价。
#[test]
fn rollup_pages_price_only_visible_rows_and_reuse_results() {
    let _g = serial();
    let dir = temp_dir("cross-and-reuse");
    setup(&dir);
    let o = opts(&dir);
    let snapshot = query::begin_query(&o).unwrap();
    take_rollup_cost_calls_for_tests();
    let page = |f: EventFilter| query::query_events(&snapshot.query_id, &f).unwrap();

    // 前 3 行是事件；取完事件段拿到事件游标。
    let events = page(EventFilter {
        limit: Some(3),
        ..Default::default()
    });
    assert_eq!(events.rows.len(), 3);
    assert_eq!(take_rollup_cost_calls_for_tests(), 0);
    let event_cursor = events.rows.last().unwrap().cursor.clone();

    // 跨入日行段：limit=2 → 只为这 2 条日行计价。
    let cross = page(EventFilter {
        limit: Some(2),
        before: Some(event_cursor.clone()),
        ..Default::default()
    });
    assert_eq!(cross.rows.len(), 2);
    assert!(cross.rows.iter().all(is_rollup));
    assert_eq!(cross.rows[0].ts, "2026-05-28");
    assert_eq!(cross.rows[1].ts, "2026-05-27");
    assert_eq!(
        take_rollup_cost_calls_for_tests(),
        2,
        "只为落到当前页的 2 条日行计价"
    );

    // 重复取同一页：同快照内已算结果直接复用。
    let again = page(EventFilter {
        limit: Some(2),
        before: Some(event_cursor),
        ..Default::default()
    });
    assert_eq!(again.rows.len(), 2);
    assert_eq!(again.rows[0].ts, cross.rows[0].ts);
    assert_eq!(again.rows[1].ts, cross.rows[1].ts);
    assert_eq!(
        take_rollup_cost_calls_for_tests(),
        0,
        "重复同页不得重算日行费用"
    );
    for (a, b) in again.rows.iter().zip(cross.rows.iter()) {
        assert_eq!(a.cost_usd, b.cost_usd, "复用结果金额一致");
    }

    // 汇总游标定位：只读索引，不计价。
    let rollup_cursor = cross.rows.last().unwrap().cursor.clone();
    assert_eq!(
        cursor_field(&rollup_cursor, "kind").as_str(),
        Some("rollup")
    );
    let dfp = cursor_field(&rollup_cursor, "dfp")
        .as_str()
        .unwrap()
        .to_string();
    let pos = query::locate_cursor(&snapshot, &rollup_cursor, &dfp).unwrap();
    assert_eq!(pos, 4, "事件段 3 行 + 段内下标 1");
    assert_eq!(
        take_rollup_cost_calls_for_tests(),
        0,
        "汇总游标定位不得触发计价"
    );

    // 从汇总游标续页：只取下一条未算日行 → 恰 1 次。
    let next = page(EventFilter {
        limit: Some(1),
        before: Some(rollup_cursor),
        ..Default::default()
    });
    assert_eq!(next.rows.len(), 1);
    assert!(is_rollup(&next.rows[0]));
    assert_eq!(next.rows[0].ts, "2026-05-26");
    assert_eq!(
        take_rollup_cost_calls_for_tests(),
        1,
        "只计价新落入页的日行"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// limit=1 走完 7 行：每页计价次数 [0,0,0,1,1,1,1]（累计 4）——每条日行恰
/// 计价一次，无漏行/无重复；旧格式事件游标与 query_expired 行为保持兼容。
#[test]
fn single_row_walk_prices_each_day_row_once() {
    let _g = serial();
    let dir = temp_dir("walk");
    setup(&dir);
    let o = opts(&dir);
    let snapshot = query::begin_query(&o).unwrap();
    take_rollup_cost_calls_for_tests();
    let page = |f: EventFilter| query::query_events(&snapshot.query_id, &f).unwrap();

    let expected: Vec<(String, bool)> = vec![
        ("2026-05-29 11:00:00".into(), false),
        ("2026-05-29 10:00:00".into(), false),
        ("2026-05-29 09:00:00".into(), false),
        ("2026-05-28".into(), true),
        ("2026-05-27".into(), true),
        ("2026-05-26".into(), true),
        ("2026-05-25".into(), true),
    ];
    let mut got: Vec<(String, bool)> = Vec::new();
    let mut calls: Vec<u64> = Vec::new();
    let mut before: Option<String> = None;
    let mut event_cursor: Option<String> = None;
    for _ in 0..expected.len() {
        let list = page(EventFilter {
            limit: Some(1),
            before: before.clone(),
            ..Default::default()
        });
        assert_eq!(list.total, 7);
        assert_eq!(list.rows.len(), 1, "不得死循环或漏行");
        let row = &list.rows[0];
        got.push((row.ts.clone(), is_rollup(row)));
        calls.push(take_rollup_cost_calls_for_tests());
        if !is_rollup(row) && event_cursor.is_none() && got.len() == 3 {
            event_cursor = Some(row.cursor.clone());
        }
        before = Some(row.cursor.clone());
    }
    assert_eq!(got, expected, "顺序：事件降序 → 日行降序");
    let mut dedup = got.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), 7, "无重复行");
    assert_eq!(
        calls,
        vec![0, 0, 0, 1, 1, 1, 1],
        "每条日行只在首次落入页时计价一次"
    );
    // 末页之后无更多行。
    let tail = page(EventFilter {
        limit: Some(1),
        before: before.clone(),
        ..Default::default()
    });
    assert!(tail.rows.is_empty());
    assert_eq!(take_rollup_cost_calls_for_tests(), 0);

    // 旧格式游标（无 kind/ridx）仍可定位事件行。
    let mut old =
        serde_json::from_str::<serde_json::Value>(event_cursor.as_ref().unwrap()).unwrap();
    old.as_object_mut().unwrap().remove("kind");
    old.as_object_mut().unwrap().remove("ridx");
    let from_old = page(EventFilter {
        limit: Some(1),
        before: Some(old.to_string()),
        ..Default::default()
    });
    assert_eq!(from_old.rows.len(), 1);
    assert_eq!(from_old.rows[0].ts, "2026-05-28", "旧事件游标续到日行段首");

    // 跨会话游标显式过期，不静默换第一页。
    let other = query::begin_query(&o).unwrap();
    take_rollup_cost_calls_for_tests();
    let err = query::query_events(
        &other.query_id,
        &EventFilter {
            limit: Some(1),
            before: before.clone(),
            ..Default::default()
        },
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("query_expired"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// 日行金额与聚合贡献同口径：逐页取到的日行费用之和 == 分组估算金额，
/// 且与一次性整页读取完全一致（按需计价不改变结果）。
#[test]
fn lazy_pricing_matches_aggregate_and_full_page() {
    let _g = serial();
    let dir = temp_dir("amount-parity");
    setup(&dir);
    let o = opts(&dir);
    let snapshot = query::begin_query(&o).unwrap();
    take_rollup_cost_calls_for_tests();
    let page = |f: EventFilter| query::query_events(&snapshot.query_id, &f).unwrap();

    let full = page(EventFilter {
        limit: Some(100),
        ..Default::default()
    });
    assert_eq!(full.rows.len(), 7);
    let full_calls = take_rollup_cost_calls_for_tests();
    assert_eq!(full_calls, 4, "整页一次读取：4 条日行各计价一次");
    let rollup_rows: Vec<&EventRow> = full.rows.iter().filter(|r| is_rollup(r)).collect();
    assert_eq!(rollup_rows.len(), 4);
    let full_sum: f64 = rollup_rows.iter().filter_map(|r| r.cost_usd).sum();
    assert!(full_sum > 0.0, "无来源费用时必须给出估算金额");

    // 逐页（limit=1）取同一批日行：金额逐项一致，总计价次数仍为 4。
    let mut before: Option<String> = None;
    let mut seen: Vec<(String, Option<f64>)> = Vec::new();
    for _ in 0..7 {
        let list = page(EventFilter {
            limit: Some(1),
            before: before.clone(),
            ..Default::default()
        });
        let row = &list.rows[0];
        if is_rollup(row) {
            seen.push((row.ts.clone(), row.cost_usd));
        }
        before = Some(row.cursor.clone());
    }
    assert_eq!(seen.len(), 4);
    for (i, (ts, cost)) in seen.iter().enumerate() {
        assert_eq!(ts, &rollup_rows[i].ts);
        assert_eq!(cost, &rollup_rows[i].cost_usd, "按需计价与整页金额一致");
    }
    let paged_sum: f64 = seen.iter().filter_map(|(_, c)| *c).sum();
    assert!((paged_sum - full_sum).abs() < 1e-9);
    assert_eq!(
        take_rollup_cost_calls_for_tests(),
        0,
        "同快照内已算日行全部复用，不重复计价"
    );
    std::fs::remove_dir_all(&dir).ok();
}
