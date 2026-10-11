//! H07：冻结查询与流式读取（计划 §4、§7 H07）。
//!
//! 覆盖：采集/导入提交后旧查询不变、新查询见新 generation、冻结的时间边界、
//! 同时间戳分页无重不漏、大历史流式汇总（保留字节与事件数无关）、只读事务
//! 随会话淘汰释放（且可显式释放）、预算仍然生效。

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::HistoryDb;
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// N05：建表/汇总窗口的**临时峰值**单独量测——保留预算不覆盖临时峰值
/// （docs/stats-semantics.md §3.5）。计数分配器只存在于本测试二进制内，
/// 真实释放仍交给系统分配器，计数只回退不下溢；百万测试以 `--ignored`
/// 单线程手工运行，窗口数字不受并行套件干扰。
struct TrackingAlloc;

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let prev = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            let live = prev.saturating_add(layout.size());
            PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size();
        let _ = LIVE_BYTES.try_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            Some(v.saturating_sub(size))
        });
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static TRACKING_ALLOC: TrackingAlloc = TrackingAlloc;

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-history-queries-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
    // 4 万条合成事件（debug 构建下的可接受规模；性质断言与规模无关）：
    // 汇总必须走**流式读取**——判据是"会话保留的数据与事件本体无关"。
    let dir = temp_dir("large");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let total = 40_000usize;
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
        charged < total * 256,
        "会话保留数据必须只含行索引（每行 < 256 字节，远小于事件本体）：实际 {charged} 字节 / {total} 行"
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

/// N05：百万级合成记录的规模验收——流式汇总、跨页游标连续、元数据保留
/// 与构建耗时测量（数字打印到输出供复核记录）。
///
/// 覆盖范围与限制如实声明：
/// - 汇总走**流式读取**，会话保留只含行索引（与事件本体无关）；
/// - 行索引与命中请求数**线性**相关（本测试量化每行字节数），不能声称
///   "查询内存与事件数无关"；
/// - `query_events` 单页上限 1000（产品钳制），百万级全量走查不可行——
///   跨页无重不漏的性质由 210 条/4 万条规模的专项测试覆盖，这里验证
///   大规模下的游标连续性、total 一致性与预算表现。
///
/// 真实规模验收：手工运行（约 15 分钟，含 100 万行合成入库）——
/// 测量数字打印到输出，记录在整合计划 N05 执行账。
#[test]
#[ignore = "百万级规模测量：手工运行（约 15 分钟），默认套件由 4 万条流式测试守护性质断言"]
fn million_row_history_summarizes_and_pages_within_budget() {
    let dir = temp_dir("million");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let total = 1_000_000usize;

    let t_seed = std::time::Instant::now();
    seed_events(&history, "m", total, false);
    let seed_elapsed = t_seed.elapsed();

    // 行索引构建：覆盖全部事件（元数据保留、构建耗时与**窗口峰值**的
    // 测量点——窗口前清零峰值计数，构建后取回）。
    let o = opts(&dir);
    PEAK_BYTES.store(0, Ordering::Relaxed);
    let live_before = LIVE_BYTES.load(Ordering::Relaxed);
    let t_build = std::time::Instant::now();
    let snap = query::begin_query(&o).unwrap();
    let build_elapsed = t_build.elapsed();
    assert_eq!(snap.rows.len(), total, "行索引覆盖全部事件");
    let build_peak = PEAK_BYTES.load(Ordering::Relaxed);
    let live_after = LIVE_BYTES.load(Ordering::Relaxed);

    let charged = query::query_retained_bytes_for_tests();
    let per_row = charged / total;
    println!(
        "N05 规模测量：rows={total} seed={}ms build={}ms retained={charged}B（≈{per_row}B/行，预算上限 256MiB）",
        seed_elapsed.as_millis(),
        build_elapsed.as_millis()
    );
    println!(
        "N05 构建窗口：live {live_before}B → {live_after}B，窗口峰值 {build_peak}B（≈{}/行）",
        build_peak / total
    );
    // 峰值仅作量测记录（保留记账是容量估算口径，与分配器实际字节不同一，
    // 二者不构成可断言的大小关系——N09/R04 修订）。
    assert!(
        charged < total * 256,
        "每行元数据（含 record_id 堆串）必须 ≤ 256 字节：实际 {per_row}B/行"
    );
    assert!(
        charged < query::MAX_RETAINED_QUERY_BYTES,
        "百万行索引必须落在默认预算内"
    );

    // 流式汇总：数字与规模精确一致（不受分批影响）；同样记录窗口峰值。
    PEAK_BYTES.store(0, Ordering::Relaxed);
    let t_summary = std::time::Instant::now();
    let summary = query::query_summary(&snap.query_id).unwrap();
    let summary_peak = PEAK_BYTES.load(Ordering::Relaxed);
    println!(
        "N05 汇总耗时：{}ms，窗口峰值 {summary_peak}B",
        t_summary.elapsed().as_millis()
    );
    assert_eq!(summary.totals.requests as usize, total);
    assert_eq!(summary.totals.tokens.input, 100 * total as u64);
    assert_eq!(summary.totals.tokens.output, 10 * total as u64);

    // 跨页：前两页 + 游标连续；total 与行索引一致。
    let page1 = query::query_events(
        &snap.query_id,
        &EventFilter {
            limit: Some(1000),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, total as u64);
    assert_eq!(page1.rows.len(), 1000);
    let page2 = query::query_events(
        &snap.query_id,
        &EventFilter {
            limit: Some(1000),
            before: Some(page1.rows.last().unwrap().cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.rows.len(), 1000);
    // 页间无重叠、无跳行：两页 record_id 全局唯一（时间互异 → id 唯一）。
    let mut ids: Vec<String> = page1
        .rows
        .iter()
        .chain(page2.rows.iter())
        .map(|r| r.record_id.clone())
        .collect();
    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), before, "跨页无重复");

    std::fs::remove_dir_all(&dir).ok();
}
