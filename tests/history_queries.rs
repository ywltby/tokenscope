//! H07：冻结查询与流式读取（计划 §4、§7 H07）。
//!
//! 覆盖：采集/导入提交后旧查询不变、新查询见新 generation、冻结的时间边界、
//! 同时间戳分页无重不漏、大历史流式汇总（保留字节与事件数无关）、只读事务
//! 随会话淘汰释放（且可显式释放）、预算仍然生效。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::HistoryDb;
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-history-queries-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts(dir: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(dir.join("no-claude")),
        codex_dir: Some(dir.join("no-codex")),
        claude_enabled: Some(false),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    }
}

/// 直接向历史库写入 `count` 条合成事件（跳过采集管线，专测查询路径）。
fn seed_events(history: &HistoryDb, prefix: &str, count: usize, same_ts: bool) {
    use tokenscope::history::{EventWrite, WritePrecedence};
    for chunk_start in (0..count).step_by(2000) {
        let end = (chunk_start + 2000).min(count);
        let batch: Vec<EventWrite> = (chunk_start..end)
            .map(|i| {
                let ts = if same_ts {
                    "2026-09-15T08:00:00Z".to_string()
                } else {
                    // 每条事件相差一秒，保证 (ts, id) 全序稳定。
                    let secs = 1_789_545_600 + i as i64;
                    jiff::Timestamp::new(secs, 0).unwrap().to_string()
                };
                EventWrite {
                    event_key: format!("{prefix}|{i}"),
                    app: AgentKind::Codex,
                    ts: ts.parse().unwrap(),
                    model_raw: "gpt-5.6-sol".into(),
                    model_identity: "gpt56sol".into(),
                    session_id: Some("s".into()),
                    record_id: Some(format!("{prefix}-r{i}")),
                    project_key: Some("/work/app".into()),
                    session_initial_cwd: None,
                    event_cwd: None,
                    tokens: TokenCounts {
                        input: 100,
                        output: 10,
                        cache_write: 0,
                        cache_read: 0,
                    },
                    precedence: WritePrecedence::NativeLog,
                    observed_at: ts.parse().unwrap(),
                    aliases: Vec::new(),
                    origins: Vec::new(),
                }
            })
            .collect();
        history.write_batch(&batch).unwrap();
    }
}

#[test]
fn committed_writes_do_not_change_an_open_query() {
    let dir = temp_dir("frozen");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    seed_events(&history, "first", 3, false);
    let generation_before = history.generation().unwrap();

    let o = opts(&dir);
    let snap = query::begin_query(&o).unwrap();
    let first = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(first.totals.requests, 3);
    assert_eq!(snap.generation, generation_before);
    // 冻结的时间边界：未限定范围时两端都为空。
    assert_eq!(snap.time_bounds(), (None, None));

    // 采集/导入式的新提交（直接用历史库写入模拟，键前缀不同 → 确实是新事件）。
    seed_events(&history, "second", 2, false);
    let after_generation = history.generation().unwrap();
    assert!(after_generation > generation_before);

    // 旧查询不受影响：数字、行序、generation 全部保持冻结。
    let again = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(again.totals.requests, 3, "旧查询不因新提交改变");
    let events = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    assert_eq!(events.total, 3);

    // 新查询看到新数据与新 generation。
    let fresh = query::begin_query(&o).unwrap();
    assert!(fresh.generation > snap.generation);
    assert_eq!(
        query::query_summary(&fresh.query_id)
            .unwrap()
            .totals
            .requests,
        5
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn same_timestamp_paging_covers_every_row_exactly_once() {
    let dir = temp_dir("paging");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    // 全部同一时间戳：分页必须靠 (ts, record_id, seq) 稳定决序，不重不漏。
    seed_events(&history, "same", 25, true);
    let o = opts(&dir);
    let snap = query::begin_query(&o).unwrap();

    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = query::query_events(
            &snap.query_id,
            &EventFilter {
                limit: Some(7),
                before: cursor.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 25, "同一快照内 total 恒定");
        if page.rows.is_empty() {
            break;
        }
        for row in &page.rows {
            seen.push(row.record_id.clone());
            cursor = Some(row.cursor.clone());
        }
    }
    assert_eq!(seen.len(), 25, "分页覆盖全量");
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 25, "同一时间戳下无重复行");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn large_history_summarizes_streaming_with_bounded_retention() {
    // 20 万条合成事件（debug 构建下的可接受规模）：汇总必须走**流式读取**。
    // 判据是"记账与事件数无关"这一性质——会话只保留行索引，不保留事件本体
    //（保留事件本体时每条还要计 model/session/project/record 四个字符串），
    // 因此按每行成本断言，规模可线性放大。
    let dir = temp_dir("large");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let total = 200_000usize;
    seed_events(&history, "bulk", total, false);

    let o = opts(&dir);
    let snap = query::begin_query(&o).unwrap();
    assert_eq!(snap.rows.len(), total, "行索引覆盖全部事件");

    let summary = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(summary.totals.requests as usize, total);
    assert_eq!(summary.totals.tokens.input, 100 * total as u64);
    assert_eq!(summary.totals.tokens.output, 10 * total as u64);

    let charged = query::query_retained_bytes_for_tests();
    assert!(
        charged < total * 64,
        "会话保留数据必须只含行索引（每行 < 64 字节）：实际 {charged} 字节 / {total} 行"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn read_transaction_can_be_released_and_expires_with_the_session() {
    let dir = temp_dir("release");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    seed_events(&history, "release", 2, false);

    // 显式释放：只读事务随 `close()`（以及 Drop）结束，不长期占住 WAL 读视图。
    let mut read = history.read_snapshot().unwrap();
    assert!(!read.is_closed());
    assert_eq!(read.event_count(None).unwrap(), 2, "快照内可正常读取");
    read.close();
    assert!(read.is_closed(), "close() 后必须标记为已释放");
    drop(read);

    // 会话淘汰：闲置超时后旧 query_id 结构化过期，且其读事务随快照释放
    //（快照被最后一个 Arc 释放时 Drop 兜底关闭）。
    let o = opts(&dir);
    let snap = query::begin_query(&o).unwrap();
    assert_eq!(
        query::query_summary(&snap.query_id)
            .unwrap()
            .totals
            .requests,
        2
    );
    query::backdate_query_for_tests(&snap.query_id, std::time::Duration::from_secs(3600));
    let err = query::query_summary(&snap.query_id)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("query_expired"),
        "过期会话必须结构化拒绝：{err}"
    );

    // 淘汰后新查询照常工作（没有残留的长期读事务阻塞）。
    let fresh = query::begin_query(&o).unwrap();
    assert_eq!(
        query::query_summary(&fresh.query_id)
            .unwrap()
            .totals
            .requests,
        2
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn query_budget_still_applies() {
    // RC05：预算仍约束会话的保留数据——行索引超预算时明确失败。
    let dir = temp_dir("budget");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    seed_events(&history, "budget", 5000, false);
    let o = opts(&dir);

    let saved = query::query_retained_bytes_for_tests();
    query::set_query_budget_for_tests(4096);
    let err = match query::begin_query(&o) {
        Ok(_) => panic!("超预算必须明确报错，而不是放行一个超大快照"),
        Err(e) => e.to_string(),
    };
    query::set_query_budget_for_tests(saved.max(32 * 1024 * 1024));
    query::reset_query_budget_for_tests();
    assert!(err.contains("预算"), "超预算必须明确报错：{err}");

    // 恢复预算后同一个查询正常建立。
    assert_eq!(
        query::query_summary(&query::begin_query(&o).unwrap().query_id)
            .unwrap()
            .totals
            .requests,
        5000
    );
    std::fs::remove_dir_all(&dir).ok();
}
