//! 计划 A3（F02）：明细必须复用主时间过滤——前端已向后端传 from/to，
//! 但 list_events 此前只应用 days，选择历史区间后明细与汇总范围不一致。
//! 数字期望沿用 e2e 合成 fixture（claude 07-17 1 条 + 07-18 2 条；
//! codex 07-17 1 条 + 07-18 3 条，去重后共 7 条）。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

/// SF04：一次性明细读取 = 建会话 + 读一次（本文件用例不跨调用翻页）。
fn list_events(
    opts: &SummaryOptions,
    filter: &EventFilter,
) -> anyhow::Result<tokenscope::report::EventList> {
    let snap = tokenscope::query::begin_query(opts)?;
    tokenscope::query::query_events(&snap.query_id, filter)
}

fn fixture(agent: &str, p: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(agent)
        .join(p)
}

fn hermetic(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-events-range-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn opts(
    dir: &std::path::Path,
    from: Option<&str>,
    to: Option<&str>,
    days: Option<u32>,
) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(fixture("claude", "basic")),
        codex_dir: Some(fixture("codex", "basic")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        from: from.map(str::to_string),
        to: to.map(str::to_string),
        days,
        ..Default::default()
    }
}

#[test]
fn test_events_range_matches_summary() {
    let dir = hermetic("parity");
    // 单日：汇总 requests 必须等于明细 total（同区间同口径）。
    for (from, to) in [
        ("2026-07-17", "2026-07-17"),
        ("2026-07-18", "2026-07-18"),
        ("2026-07-16", "2026-07-18"),
    ] {
        let o = opts(&dir, Some(from), Some(to), None);
        let s = summary(&o).unwrap();
        let l = list_events(&o, &EventFilter::default()).unwrap();
        assert_eq!(
            l.total, s.totals.requests,
            "区间 {from}..{to}：明细 total 必须与汇总 requests 一致"
        );
        // 返回行的时间（解析时区日期）全部落在区间内。
        for row in &l.rows {
            let d = &row.ts[0..10];
            assert!(d >= from && d <= to, "行日期 {d} 超出区间 {from}..{to}");
        }
    }
    // 区间外为空：汇总 0 请求，明细 0 行。
    let empty = opts(&dir, Some("2026-01-01"), Some("2026-01-31"), None);
    assert_eq!(summary(&empty).unwrap().totals.requests, 0);
    assert_eq!(
        list_events(&empty, &EventFilter::default()).unwrap().total,
        0
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_events_invalid_range_rejected() {
    let dir = hermetic("invalid");
    // days 与 from/to 互斥（与 summary 同一口径校验）。
    let both = opts(&dir, Some("2026-07-17"), Some("2026-07-18"), Some(7));
    assert!(list_events(&both, &EventFilter::default()).is_err());
    // 起始晚于结束。
    let reversed = opts(&dir, Some("2026-07-18"), Some("2026-07-17"), None);
    assert!(list_events(&reversed, &EventFilter::default()).is_err());
    // 非法日期格式。
    let bad = opts(&dir, Some("2026/07/17"), None, None);
    assert!(list_events(&bad, &EventFilter::default()).is_err());
    std::fs::remove_dir_all(&dir).ok();
}
