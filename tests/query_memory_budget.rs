//! RC05：查询会话按**完整保留数据**记账与回收。
//!
//! 2026-10-08 复核复现：旧实现按主过滤后的 `rows.len()` 计预算，
//! `filtered_rows=0 retained_events=5` —— 零命中行的查询仍保留整份采集
//! 事件与价格表，预算各计零行；`MAX_TOTAL_SNAPSHOT_EVENTS`（行数上限）
//! 因此不是字节预算，注释所称的 300 MB 上界不成立。
//!
//! 本文件验证：
//! 1. 记账覆盖**完整保留对象**（事件、字符串、价格规则、行索引），与
//!    筛选命中行数无关；
//! 2. 小额预算下并发/顺序准入不超预算，旧会话显式失效；
//! 3. 单个快照超预算**明确失败且不截断事件**，失败准入不泄漏额度；
//! 4. 被淘汰但仍有读取者持有的快照**继续占账**，直到最后一个 Arc 释放。
//!
//! 全部使用合成日志与临时目录（cache_dir/pricing_index 显式注入），
//! 不触碰真实 `~/.tokenscope` 与 agent 日志。预算用测试钩子注入小值，
//! 不分配数百 MB。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 预算与注册表都是进程级全局状态：串行执行，避免并行测试互相清掉对方
/// 的活跃会话或额度。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn hermetic(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-query-mem-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 合成 codex 日志：`count` 条独立时间戳的 token_count 事件。
/// `model`/`cwd` 可注入长字符串（验证字符串计入保留量）。
fn codex_body(count: usize, model: &str, cwd: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        r#"{{"timestamp":"2026-07-17T16:00:00.000Z","type":"session_meta","payload":{{"id":"s1","session_id":"s1","cwd":"{cwd}"}}}}"#
    ));
    s.push('\n');
    s.push_str(&format!(
        r#"{{"timestamp":"2026-07-17T16:00:00.000Z","type":"turn_context","payload":{{"turn_id":"t1","model":"{model}","cwd":"{cwd}"}}}}"#
    ));
    s.push('\n');
    for i in 0..count {
        s.push_str(&format!(
            r#"{{"timestamp":"2026-07-17T16:00:{:02}.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{}, "output_tokens":0,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":{}}}}}}}}}"#,
            (i % 60) + 1,
            (i + 1) * 10,
            (i + 1) * 10
        ));
        s.push('\n');
    }
    s
}

fn write_log(root: &Path, body: &str) {
    let day = root.join("2026").join("07").join("17");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(day.join("rollout-mem.jsonl"), body).unwrap();
}

fn opts(dir: &Path, root: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(dir.join("no-claude")),
        codex_dir: Some(root.to_path_buf()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 只保留指定日期区间（用于制造"零命中行但保留整份事件"）。
fn opts_range(dir: &Path, root: &Path, from: &str, to: &str) -> SummaryOptions {
    SummaryOptions {
        from: Some(from.to_string()),
        to: Some(to.to_string()),
        ..opts(dir, root)
    }
}

fn used() -> usize {
    query::query_retained_bytes_for_tests()
}

/// 清空会话与预算，返回"干净起点"的已用额度（应为 0）。
fn fresh(limit: usize) -> usize {
    query::clear_query_registry_for_tests();
    query::reset_query_budget_for_tests();
    query::set_query_budget_for_tests(limit);
    used()
}

const EVENT_SIZE: usize = std::mem::size_of::<tokenscope::model::UsageEvent>();

/// 零命中行仍为**整份采集事件**记账（旧实现按 rows.len()=0 计）。
#[test]
fn empty_range_charges_retained_collection() {
    let _g = serial();
    let dir = hermetic("empty-range");
    let root = dir.join("codex");
    const N: usize = 40;
    write_log(&root, &codex_body(N, "gpt-5.6-sol", "C:/w/a"));

    let base = fresh(usize::MAX);
    // 区间完全落在事件之外 → 主过滤命中 0 行。
    let o = opts_range(&dir, &root, "2020-01-01", "2020-01-02");
    let snap = query::begin_query(&o).unwrap();
    assert_eq!(snap.rows.len(), 0, "区间应命中 0 行");
    let page = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    assert_eq!(page.total, 0);

    let charged = used() - base;
    assert!(
        charged >= N * EVENT_SIZE,
        "零命中行仍须为保留的 {N} 条事件记账：{charged} < {}",
        N * EVENT_SIZE
    );
    drop(snap);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 事件数相同、模型/项目字符串更长 → 保留记账更高。
#[test]
fn long_strings_increase_snapshot_charge() {
    let _g = serial();
    const N: usize = 20;

    let short_dir = hermetic("short-strings");
    let short_root = short_dir.join("codex");
    write_log(&short_root, &codex_body(N, "gpt-5.6-sol", "C:/w/a"));
    let base = fresh(usize::MAX);
    let short = query::begin_query(&opts(&short_dir, &short_root)).unwrap();
    let short_charge = used() - base;
    drop(short);

    let long_dir = hermetic("long-strings");
    let long_root = long_dir.join("codex");
    let long_model = format!("vendor/{}", "m".repeat(400));
    let long_cwd = format!("C:/{}", "d".repeat(400));
    write_log(&long_root, &codex_body(N, &long_model, &long_cwd));
    let base2 = fresh(usize::MAX);
    let long = query::begin_query(&opts(&long_dir, &long_root)).unwrap();
    let long_charge = used() - base2;
    drop(long);

    assert!(
        long_charge > short_charge,
        "长模型/项目字符串必须增加保留记账：long={long_charge} short={short_charge}"
    );
    let _ = std::fs::remove_dir_all(&short_dir);
    let _ = std::fs::remove_dir_all(&long_dir);
}

/// 价格规则（候选、嵌套分段/峰谷）与快照行索引都计入保留量。
#[test]
fn pricing_and_sort_indices_are_accounted() {
    let _g = serial();
    let dir = hermetic("pricing-index");
    let root = dir.join("codex");
    const N: usize = 30;
    write_log(&root, &codex_body(N, "gpt-5.6-sol", "C:/w/a"));

    // (a) 无外置价格：0 行 vs 全量的差 = 行索引 Vec。
    let base = fresh(usize::MAX);
    let empty = query::begin_query(&opts_range(&dir, &root, "2020-01-01", "2020-01-02")).unwrap();
    let empty_charge = used() - base;
    let full = query::begin_query(&opts(&dir, &root)).unwrap();
    assert_eq!(full.rows.len(), N);
    let full_charge = used() - base - empty_charge;
    assert!(
        full_charge >= N * std::mem::size_of::<query::SnapshotRow>(),
        "快照行索引必须计入：{full_charge} < {}",
        N * std::mem::size_of::<query::SnapshotRow>()
    );

    // (b) 加一份含分段/峰谷的外置价格表 → 保留量增加。
    let mut pricing = String::new();
    for i in 0..30 {
        pricing.push_str(&format!(
            "[[model]]\nprefix = \"vendor-{i}\"\ninput = 1.0\noutput = 2.0\n\
             [[model.segment]]\nmin_tokens = 200000\ninput = 3.0\noutput = 4.0\n\
             [[model.schedule]]\nlabel = \"峰谷{i}\"\ntimezone = \"UTC\"\ninput = 0.5\noutput = 0.5\n\
             [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"08:00\"\ninput = 0.2\noutput = 0.2\n"
        ));
    }
    std::fs::write(dir.join("pricing.toml"), pricing).unwrap();
    let base2 = fresh(usize::MAX);
    let priced = query::begin_query(&opts(&dir, &root)).unwrap();
    let priced_charge = used() - base2;
    assert!(
        priced_charge > empty_charge + N * std::mem::size_of::<query::SnapshotRow>(),
        "价格规则必须计入保留量：priced={priced_charge} empty={empty_charge}"
    );
    drop(priced);
    drop(full);
    drop(empty);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 顺序建多会话不能绕过字节预算：超出后 LRU 淘汰旧会话（旧 ID 显式失效），
/// 且已用额度始终不超预算。
#[test]
fn sequential_queries_cannot_bypass_byte_budget() {
    let _g = serial();
    let dir = hermetic("sequential");
    let root = dir.join("codex");
    const N: usize = 30;
    write_log(&root, &codex_body(N, "gpt-5.6-sol", "C:/w/a"));
    let o = opts(&dir, &root);

    // 先测单个会话的保留量（无限额）。
    let base = fresh(usize::MAX);
    let probe = query::begin_query(&o).unwrap();
    let per = used() - base;
    assert!(per > 0);
    drop(probe);
    query::clear_query_registry_for_tests();
    assert_eq!(used(), base, "释放最后一个 Arc 后额度必须归还");

    // 预算只装得下 2 个会话。
    query::set_query_budget_for_tests(per * 2 + per / 2);
    let mut ids = Vec::new();
    for _ in 0..8 {
        match query::begin_query(&o) {
            Ok(s) => {
                ids.push(s.query_id.clone());
                assert!(used() <= per * 2 + per / 2, "已用额度不得超预算");
            }
            Err(e) => panic!("预算足够 2 个会话，不应失败：{e}"),
        }
    }
    // 8 次建立后只剩最近的会话可用，最早的显式过期。
    let err = query::query_events(&ids[0], &EventFilter::default()).unwrap_err();
    assert!(err.to_string().contains("query_expired"), "{err}");
    let newest = ids.last().unwrap();
    let page = query::query_events(newest, &EventFilter::default()).unwrap();
    assert_eq!(page.total, N as u64, "存活会话不得被截断");
    assert!(used() <= per * 2 + per / 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 单个快照自身超预算：明确失败、可读错误、**不截断**事件、不影响已有会话。
#[test]
fn single_snapshot_over_budget_is_rejected_without_truncation() {
    let _g = serial();
    let dir = hermetic("single-over");
    let root = dir.join("codex");
    const N: usize = 30;
    write_log(&root, &codex_body(N, "gpt-5.6-sol", "C:/w/a"));
    let o = opts(&dir, &root);

    let base = fresh(usize::MAX);
    let alive = query::begin_query(&o).unwrap();
    let per = used() - base;

    // 收紧到装不下任何一个会话。
    query::set_query_budget_for_tests(per / 2);
    let err = match query::begin_query(&o) {
        Ok(_) => panic!("单会话保留量超过预算时必须明确拒绝"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("预算") && msg.contains("MiB"),
        "错误必须可读并说明预算：{msg}"
    );
    assert_eq!(used(), per, "失败准入不得改变已用额度");

    // 已有会话未被淘汰、事件未被截断。
    let page = query::query_events(&alive.query_id, &EventFilter::default()).unwrap();
    assert_eq!(page.total, N as u64, "超预算拒绝不得截断已保留的事件");
    drop(alive);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 被淘汰但仍有读取者持有的快照**继续占账**，直到最后一个 Arc 释放。
#[test]
fn evicted_but_borrowed_snapshot_stays_charged() {
    let _g = serial();
    let dir = hermetic("borrowed");
    let root = dir.join("codex");
    write_log(&root, &codex_body(20, "gpt-5.6-sol", "C:/w/a"));
    let o = opts(&dir, &root);

    let base = fresh(usize::MAX);
    let held = query::begin_query(&o).unwrap();
    let held_charge = used() - base;

    // 建满容量，把 held 挤出注册表。
    for _ in 0..query::MAX_ACTIVE_QUERIES {
        let s = query::begin_query(&o).unwrap();
        drop(s);
    }
    let err = query::query_events(&held.query_id, &EventFilter::default()).unwrap_err();
    assert!(
        err.to_string().contains("query_expired"),
        "被淘汰后游标必须过期"
    );

    let before_drop = used();
    assert!(
        before_drop >= held_charge,
        "被淘汰但被借用的快照仍须占账：{before_drop} < {held_charge}"
    );
    drop(held);
    assert_eq!(
        used(),
        before_drop - held_charge,
        "只在最后一个 Arc 释放时退还该会话额度"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 并发准入不得超预算：用 barrier 让两个线程同时抢，只允许一个成功。
#[test]
fn concurrent_admission_respects_budget() {
    let _g = serial();
    let dir = hermetic("concurrent");
    let root = dir.join("codex");
    write_log(&root, &codex_body(30, "gpt-5.6-sol", "C:/w/a"));
    let o = opts(&dir, &root);

    let base = fresh(usize::MAX);
    let probe = query::begin_query(&o).unwrap();
    let per = used() - base;
    drop(probe);
    query::clear_query_registry_for_tests();

    let limit = per + per / 2; // 只装得下 1 个
    query::set_query_budget_for_tests(limit);

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    // 两个线程同时抢：胜者**保持 Arc 存活**（额度不会被败者回收），
    // 因此恰好一个成功且已用额度不超预算。
    let held = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let b = barrier.clone();
                let o = o.clone();
                scope.spawn(move || {
                    b.wait(); // 同时起跑，确定性交错（不用 sleep）
                    match query::begin_query(&o) {
                        Ok(s) => (true, Some(s)),
                        Err(_) => (false, None),
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    let ok = held.iter().filter(|(ok, _)| *ok).count();
    assert_eq!(ok, 1, "预算只够一个会话，必须恰好一个成功");
    assert!(used() <= limit, "并发准入不得超预算：{} > {limit}", used());
    drop(held);
    let _ = std::fs::remove_dir_all(&dir);
}
