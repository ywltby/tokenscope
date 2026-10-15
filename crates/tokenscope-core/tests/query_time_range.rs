//! U01：查询时间范围的两种边界模式（自然日 / 时刻）与回传契约。
//!
//! 覆盖（`docs/plans/active/2026-10-13-ui-adjustment.md` U01「先写的测试」2–7）：
//! 1. `days` 预设同样排除晚于本次冻结 `as_of` 的记录（U01 的关键行为变化）；
//! 2. 24h 滚动窗口只含 `[as_of-24h, as_of]`，按天分组的组和等于总量；
//! 3. 整日自定义（00:00–23:59 / 00:00 至今）与等价自然日区间结果完全相同
//!    （含日汇总参与情况）；
//! 4. 时刻模式不选取任何日汇总桶，`rollups_excluded_for_instant_range` 只在
//!    所涉自然日确有日汇总时为 true；
//! 5. `range` 回传在四组参数下正确（全部时间为 `None`，序列化为 null）；
//! 6. `QuerySnapshot::time_bounds()` 在时刻模式返回区间覆盖的自然日两端。
//!
//! 全部使用合成日志与临时目录（`cache_dir`/`pricing_index`/`history_path`
//! 一律显式注入），不触碰真实 `~/.tokenscope` 与 agent 日志。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{HistoryDb, RollupConflictPolicy};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::query;
use tokenscope::report::{RangeView, SummaryOptions, SummaryReport, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn hermetic(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-query-time-range-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 采集选项：来源=合成 Claude 根，数据/索引/价格/历史库全部落临时根。
fn opts(dir: &Path, claude_root: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(claude_root.to_path_buf()),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        history_path: Some(dir.join("data").join("history.db")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 合成一条 Claude assistant 行（时间戳由调用方给定，可精确构造晚于本次
/// 查询基准的记录）。
fn write_claude(root: &Path, session: &str, id: &str, ts: jiff::Timestamp) {
    let file = root.join("proj-a").join(format!("{session}.jsonl"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": ts.to_string(),
        "sessionId": session,
        "message": {
            "id": id,
            "model": "claude-opus-5-5",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 10,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        }
    });
    let mut body = std::fs::read_to_string(&file).unwrap_or_default();
    body.push_str(&line.to_string());
    body.push('\n');
    std::fs::write(&file, body).unwrap();
}

/// 合成 CCS 日汇总（只有日粒度的历史），并导入统一历史库。
/// `proxy_request_logs` 必须存在：来源库版本校验按表集判定，缺表即拒绝。
const CCS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS proxy_request_logs (
    request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL,
    model TEXT NOT NULL, request_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', status_code INTEGER NOT NULL DEFAULT 200,
    session_id TEXT, created_at INTEGER NOT NULL, data_source TEXT NOT NULL DEFAULT 'proxy',
    pricing_model TEXT, input_token_semantics INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS usage_daily_rollups (
    date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL, model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '', pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0, success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
";

fn import_rollup(dir: &Path, date: &str, request_count: i64, input: i64, output: i64) {
    let db = dir.join("cc-switch.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(CCS_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
                request_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, input_token_semantics)
             VALUES(?1, 'claude', 'p1', 'claude-opus-5-5', '', ?2, ?3, ?4, 0, 0, '9.0', 2)",
            rusqlite::params![date, request_count, input, output],
        )
        .unwrap();
    }
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let source = CcsSource::open(&db).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
}

/// 数字比较用：会话身份、冻结基准与时间范围本身每次查询必然不同，
/// 不参与"结果完全相同"的比较。
fn numbers(r: &SummaryReport) -> serde_json::Value {
    let mut v = serde_json::to_value(r).unwrap();
    for k in ["query_id", "generated_at", "range"] {
        v[k] = serde_json::Value::Null;
    }
    v
}

fn tz_shanghai() -> jiff::tz::TimeZone {
    jiff::tz::TimeZone::get("Asia/Shanghai").unwrap()
}

// ── 1. 晚于 as_of 的记录一律排除 ────────────────────────────────

#[test]
fn days_preset_excludes_records_after_frozen_as_of() {
    let dir = hermetic("as-of");
    let root = dir.join("claude");
    let t0 = jiff::Timestamp::now();
    let past = t0.checked_sub(jiff::Span::new().minutes(1)).unwrap();
    let future = t0.checked_add(jiff::Span::new().hours(1)).unwrap();
    write_claude(&root, "s1", "m-past", past);
    write_claude(&root, "s1", "m-future", future);
    let base = opts(&dir, &root);

    // 无时间参数（全部时间）：两条都已入库——全部时间不设范围，也不按
    // as_of 截断（无范围 ≠ 有范围）。
    let all = summary(&base).unwrap();
    assert_eq!(all.totals.requests, 2, "两条事件都已入库");

    let r = summary(&SummaryOptions {
        days: Some(1),
        ..base.clone()
    })
    .unwrap();
    let as_of: jiff::Zoned = r.generated_at.parse().unwrap();
    assert!(
        future > as_of.timestamp(),
        "用例前提：未来事件确实晚于本次查询冻结的 as_of"
    );
    // past 与 as_of 是否同一统计自然日决定了 days=1 是否纳入它（本机时钟
    // 恰好落在当地午夜附近时两者不同日）；两种情况都必须排除 future。
    let expected = u64::from(past.to_zoned(tz_shanghai()).date() == as_of.date());
    assert_eq!(
        r.totals.requests, expected,
        "days=1 只纳入今天且不晚于 as_of 的记录（expected={expected}）"
    );
    assert!(
        r.totals.requests < all.totals.requests,
        "晚于 as_of 的记录必须被排除（自然日模式同样生效）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── 2. 24h 滚动窗口 ────────────────────────────────────────────

#[test]
fn rolling_24h_window_bounds_and_day_groups_sum_to_total() {
    let dir = hermetic("rolling");
    let root = dir.join("claude");
    let t0 = jiff::Timestamp::now();
    let outside = t0.checked_sub(jiff::Span::new().hours(30)).unwrap();
    let edge = t0
        .checked_sub(jiff::Span::new().hours(24))
        .unwrap()
        .checked_add(jiff::Span::new().seconds(30))
        .unwrap();
    let near = t0.checked_sub(jiff::Span::new().minutes(1)).unwrap();
    let future = t0.checked_add(jiff::Span::new().hours(1)).unwrap();
    write_claude(&root, "s1", "m-outside", outside);
    write_claude(&root, "s1", "m-edge", edge);
    write_claude(&root, "s1", "m-near", near);
    write_claude(&root, "s1", "m-future", future);
    let base = opts(&dir, &root);

    let r = summary(&SummaryOptions {
        rolling_hours: Some(24),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(
        r.totals.requests, 2,
        "窗口 [as_of-24h, as_of] 内 2 条：30 小时前与晚于 as_of 的各排除一条"
    );
    // 对照：全部时间不设范围，四条都在（含 30 小时前与晚于基准的那条）。
    assert_eq!(summary(&base).unwrap().totals.requests, 4, "四条都在库里");
    // 按天分组：各自然日组之和 = 总量（合计行在末尾，不计入）。
    let days = &r.groups[..r.groups.len() - 1];
    let sum: u64 = days.iter().map(|g| g.requests).sum();
    assert_eq!(sum, r.totals.requests, "按天分组之和必须等于总量");
    assert!(
        days.iter().all(|g| !g.key.is_empty()),
        "按天分组必须有自然日键"
    );
    // 窗口横跨的自然日数由两条边界内事件的本地日期决定（本机时钟靠近
    // 午夜时会落在同一天）。
    let tz = tz_shanghai();
    let expected_days = if edge.to_zoned(tz.clone()).date() != near.to_zoned(tz).date() {
        2
    } else {
        1
    };
    assert_eq!(days.len(), expected_days, "窗口内事件按统计自然日分组");
    std::fs::remove_dir_all(&dir).ok();
}

// ── 3. 整日对齐：自定义 == 等价自然日区间 ──────────────────────

#[test]
fn whole_day_custom_range_matches_equivalent_natural_day_range() {
    let dir = hermetic("whole-day");
    let root = dir.join("claude");
    write_claude(&root, "s1", "d17", "2026-07-17T02:00:00Z".parse().unwrap());
    write_claude(&root, "s1", "d18", "2026-07-18T02:00:00Z".parse().unwrap());
    import_rollup(&dir, "2026-07-19", 5, 1000, 100);
    let base = opts(&dir, &root);

    let aligned = summary(&SummaryOptions {
        start_at: Some("2026-07-17T00:00".to_string()),
        end_at: Some("2026-07-17T23:59".to_string()),
        ..base.clone()
    })
    .unwrap();
    let natural = summary(&SummaryOptions {
        from: Some("2026-07-17".to_string()),
        to: Some("2026-07-17".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(
        serde_json::to_value(&aligned.range).unwrap(),
        serde_json::to_value(&natural.range).unwrap(),
        "整日自定义与等价自然日区间的回传范围必须一致"
    );
    assert_eq!(
        numbers(&aligned),
        numbers(&natural),
        "整日自定义 00:00–23:59 与等价自然日区间结果必须完全相同"
    );
    assert_eq!(aligned.totals.requests, 1, "该日只有一条明细");

    // 00:00 至今 == [start, 今天]（今天之后没有 ts ≤ as_of 的记录，两者等价）
    let until = summary(&SummaryOptions {
        start_at: Some("2026-07-17T00:00".to_string()),
        until_now: true,
        ..base.clone()
    })
    .unwrap();
    let from_only = summary(&SummaryOptions {
        from: Some("2026-07-17".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(
        numbers(&until),
        numbers(&from_only),
        "00:00 至今与 [start, 今天] 结果必须相同"
    );
    assert_eq!(until.totals.requests, 7, "两条明细 + 一条日汇总的请求数");
    assert_eq!(
        until.rollup_coverage.rollup_buckets, 1,
        "自然日模式包含日汇总（既有语义）"
    );
    assert!(!until.rollup_coverage.rollups_excluded_for_instant_range);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 4. 时刻模式与日汇总 ────────────────────────────────────────

#[test]
fn instant_range_excludes_rollups_and_flags_only_when_they_exist() {
    let dir = hermetic("instant-rollup");
    let root = dir.join("claude");
    write_claude(&root, "s1", "d18", "2026-07-18T02:00:00Z".parse().unwrap());
    import_rollup(&dir, "2026-07-17", 5, 1000, 100);
    let base = opts(&dir, &root);

    // 所涉自然日（07-17）确有日汇总 → 提示键为 true，且日汇总桶一个都不参与。
    let hit = summary(&SummaryOptions {
        start_at: Some("2026-07-17T09:00".to_string()),
        end_at: Some("2026-07-17T18:00".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert!(
        hit.rollup_coverage.rollups_excluded_for_instant_range,
        "所涉日期确有日汇总时必须提示"
    );
    assert_eq!(
        hit.rollup_coverage.rollup_buckets, 0,
        "时刻模式不选日汇总桶"
    );
    assert_eq!(hit.totals.requests, 0, "该时刻区间内没有明细");
    assert_eq!(hit.totals.unknown_tokens.total(), 0, "日汇总不按比例估算");

    // 所涉自然日（07-18）没有日汇总 → 不提示；区间内的明细照常统计。
    let miss = summary(&SummaryOptions {
        start_at: Some("2026-07-18T09:00".to_string()),
        end_at: Some("2026-07-18T18:00".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert!(
        !miss.rollup_coverage.rollups_excluded_for_instant_range,
        "所涉日期没有日汇总时不得提示"
    );
    assert_eq!(miss.rollup_coverage.rollup_buckets, 0);
    assert_eq!(miss.totals.requests, 1, "07-18 10:00（本地）的明细在区间内");

    // 跨天时刻区间：只要覆盖到的任一天有日汇总就提示。
    let span = summary(&SummaryOptions {
        start_at: Some("2026-07-17T09:00".to_string()),
        end_at: Some("2026-07-18T18:00".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert!(span.rollup_coverage.rollups_excluded_for_instant_range);
    assert_eq!(span.totals.requests, 1);

    // 对照：自然日模式同一天，日汇总照常参与（既有语义不被"修正"）。
    let day = summary(&SummaryOptions {
        from: Some("2026-07-17".to_string()),
        to: Some("2026-07-17".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(day.rollup_coverage.rollup_buckets, 1);
    assert_eq!(day.totals.requests, 5, "日汇总的请求数进入统计");
    assert!(!day.rollup_coverage.rollups_excluded_for_instant_range);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 5. range 回传（四组参数 + all） ────────────────────────────

#[test]
fn range_view_reports_frozen_bounds_for_every_group() {
    let dir = hermetic("range");
    let root = dir.join("claude");
    write_claude(&root, "s1", "d17", "2026-07-17T02:00:00Z".parse().unwrap());
    let base = opts(&dir, &root);

    // 全部时间：None，且序列化为 null（不是空对象）。
    let h = query::begin_query_handle(&base).unwrap();
    assert!(h.range.is_none(), "全部时间不返回范围");
    let all = query::query_summary(&h.query_id).unwrap();
    assert!(all.range.is_none());
    let json = serde_json::to_value(&all).unwrap();
    assert!(json.get("range").is_some(), "字段必须存在");
    assert!(json["range"].is_null(), "全部时间必须序列化为 null");

    // 自然日预设：冻结的 [今天-(N-1), 今天]。
    let h = query::begin_query_handle(&SummaryOptions {
        days: Some(7),
        ..base.clone()
    })
    .unwrap();
    let as_of: jiff::Zoned = h.as_of.parse().unwrap();
    let expect_from = as_of
        .date()
        .checked_sub(jiff::Span::new().days(6))
        .unwrap()
        .to_string();
    match h.range.clone().expect("days 预设必须有范围") {
        RangeView::Day { from, to } => {
            assert_eq!(from.as_deref(), Some(expect_from.as_str()));
            assert_eq!(to.as_deref(), Some(as_of.date().to_string().as_str()));
        }
        other => panic!("days 预设应为自然日模式，实际 {other:?}"),
    }
    let s = query::query_summary(&h.query_id).unwrap();
    assert_eq!(
        serde_json::to_value(&h.range).unwrap(),
        serde_json::to_value(&s.range).unwrap(),
        "句柄与汇总必须回传同一份冻结范围"
    );
    // IPC 形状：外壳 camelCase 不得改到枚举内部字段（前端按 snake_case 读）。
    let hj = serde_json::to_value(&h).unwrap();
    assert_eq!(hj["range"]["granularity"], "day");
    assert!(
        hj["range"]["from"].is_string(),
        "内部字段保持 snake_case：{hj}"
    );
    assert!(hj["range"]["to"].is_string());

    // 滚动窗口：start = as_of-24h、end = as_of；end_local = as_of 所在分钟。
    let h = query::begin_query_handle(&SummaryOptions {
        rolling_hours: Some(24),
        ..base.clone()
    })
    .unwrap();
    let as_of: jiff::Zoned = h.as_of.parse().unwrap();
    match h.range.clone().expect("24h 必须有范围") {
        RangeView::Instant {
            start_local,
            end_local,
            start,
            end,
        } => {
            assert_eq!(
                start,
                as_of
                    .timestamp()
                    .checked_sub(jiff::Span::new().hours(24))
                    .unwrap()
                    .to_string()
            );
            assert_eq!(end, as_of.timestamp().to_string(), "24h 的 end = as_of");
            assert_eq!(
                end_local,
                as_of.strftime("%Y-%m-%dT%H:%M").to_string(),
                "24h 的 end_local = as_of 所在分钟"
            );
            assert_eq!(
                start_local,
                as_of
                    .timestamp()
                    .checked_sub(jiff::Span::new().hours(24))
                    .unwrap()
                    .to_zoned(as_of.time_zone().clone())
                    .strftime("%Y-%m-%dT%H:%M")
                    .to_string()
            );
        }
        other => panic!("24h 应为时刻模式，实际 {other:?}"),
    }

    let hj = serde_json::to_value(&h).unwrap();
    assert_eq!(hj["range"]["granularity"], "instant");
    for key in ["start_local", "end_local", "start", "end"] {
        assert!(
            hj["range"][key].is_string(),
            "时刻模式必须回传 snake_case 字段 {key}：{hj}"
        );
    }

    // 自定义整日 → 自然日模式，两端为用户选中的日期。
    let h = query::begin_query_handle(&SummaryOptions {
        start_at: Some("2026-07-16T00:00".to_string()),
        end_at: Some("2026-07-17T23:59".to_string()),
        ..base.clone()
    })
    .unwrap();
    match h.range.clone().expect("自定义整日必须有范围") {
        RangeView::Day { from, to } => {
            assert_eq!(from.as_deref(), Some("2026-07-16"));
            assert_eq!(to.as_deref(), Some("2026-07-17"));
        }
        other => panic!("整日自定义应为自然日模式，实际 {other:?}"),
    }

    // 自定义时刻 → 本地字段与用户选中的分钟一致，时刻字段与本地时刻往返一致。
    let h = query::begin_query_handle(&SummaryOptions {
        start_at: Some("2026-07-17T09:30".to_string()),
        end_at: Some("2026-07-17T18:45".to_string()),
        ..base.clone()
    })
    .unwrap();
    let tz = as_of_timezone(&h);
    match h.range.clone().expect("自定义时刻必须有范围") {
        RangeView::Instant {
            start_local,
            end_local,
            start,
            end,
        } => {
            assert_eq!(start_local, "2026-07-17T09:30");
            assert_eq!(
                end_local, "2026-07-17T18:45",
                "end_local = 用户选中的最后一分钟"
            );
            let start_ts: jiff::Timestamp = start.parse().unwrap();
            let end_ts: jiff::Timestamp = end.parse().unwrap();
            assert_eq!(
                start_ts
                    .to_zoned(tz.clone())
                    .strftime("%Y-%m-%dT%H:%M")
                    .to_string(),
                start_local,
                "start 必须换算回用户选中的本地时刻"
            );
            assert_eq!(
                end_ts
                    .to_zoned(tz.clone())
                    .strftime("%Y-%m-%dT%H:%M")
                    .to_string(),
                end_local,
                "end 必须换算回用户选中的本地时刻"
            );
            assert!(start_ts < end_ts);
        }
        other => panic!("自定义时刻应为时刻模式，实际 {other:?}"),
    }

    // 至今：end_local 为 as_of 所在分钟、end 为 as_of 本身。
    let h = query::begin_query_handle(&SummaryOptions {
        start_at: Some("2026-07-17T09:30".to_string()),
        until_now: true,
        ..base.clone()
    })
    .unwrap();
    let as_of: jiff::Zoned = h.as_of.parse().unwrap();
    match h.range.clone().expect("至今必须有范围") {
        RangeView::Instant {
            start_local,
            end_local,
            end,
            ..
        } => {
            assert_eq!(start_local, "2026-07-17T09:30");
            assert_eq!(end_local, as_of.strftime("%Y-%m-%dT%H:%M").to_string());
            assert_eq!(end, as_of.timestamp().to_string());
        }
        other => panic!("自定义至今应为时刻模式，实际 {other:?}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

fn as_of_timezone(h: &tokenscope::query::QueryHandle) -> jiff::tz::TimeZone {
    let as_of: jiff::Zoned = h.as_of.parse().unwrap();
    as_of.time_zone().clone()
}

// ── 6. parts()/time_bounds() 在时刻模式下的语义 ────────────────

#[test]
fn instant_snapshot_time_bounds_report_covered_local_days() {
    let dir = hermetic("parts");
    let root = dir.join("claude");
    write_claude(&root, "s1", "d17", "2026-07-17T02:00:00Z".parse().unwrap());
    let base = opts(&dir, &root);

    let snap = query::begin_query(&SummaryOptions {
        start_at: Some("2026-07-17T09:30".to_string()),
        end_at: Some("2026-07-17T18:45".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(
        snap.time_bounds(),
        (
            Some("2026-07-17".to_string()),
            Some("2026-07-17".to_string())
        ),
        "时刻模式返回区间覆盖的自然日两端（不是时刻本身）"
    );

    let span = query::begin_query(&SummaryOptions {
        start_at: Some("2026-07-17T22:00".to_string()),
        end_at: Some("2026-07-18T02:00".to_string()),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(
        span.time_bounds(),
        (
            Some("2026-07-17".to_string()),
            Some("2026-07-18".to_string())
        )
    );

    let empty = query::begin_query(&base).unwrap();
    assert_eq!(empty.time_bounds(), (None, None), "全部时间两侧都不限");
    std::fs::remove_dir_all(&dir).ok();
}
