//! SF04（安全与数据一致性审查 Task 4）：查询快照分页契约。
//!
//! 不变量（docs/stats-semantics.md §3.5/§3.6）：
//! 1. 同一 query 会话内，total/排序/游标/价格修订/费用恒定——日志追加、
//!    删除、价格文件变化都只影响**新** query，旧 query 完整遍历不重不漏；
//! 2. 游标 v2 绑定 query_id/主指纹/下钻指纹/行位置：过期、外会话、旧
//!    磁盘视图（v1 游标）一律结构化拒绝（query_expired / 游标非法），
//!    绝不静默换第一页或新采集结果；
//! 3. summary 与 events 共享同一会话上下文（同 query_id、同价格修订、
//!    数字同源）；
//! 4. 会话注册表按空闲 TTL/容量淘汰，淘汰后旧游标显式过期（可注入时钟
//!    驱动，不用 sleep 猜时序）。
//!
//! 全部使用合成日志与临时目录（cache_dir/pricing_index 显式注入），
//! 不触碰真实 ~/.tokenscope 与 agent 日志。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 本文件全部测试共享进程级会话注册表（clear/backdate 是全局操作）：
/// 串行执行，避免并行测试互相清掉对方的活跃会话。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn hermetic(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-query-snap-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 合成 codex 日志目录：可变 fixture（测试中途修改触发"日志变化"）。
fn write_codex_log(root: &Path, body: &str) -> PathBuf {
    let day = root.join("2026").join("07").join("17");
    std::fs::create_dir_all(&day).unwrap();
    let f = day.join("rollout-snap.jsonl");
    std::fs::write(&f, body).unwrap();
    f
}

fn opts(dir: &Path, codex_root: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(dir.join("no-claude")),
        codex_dir: Some(codex_root.to_path_buf()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 同一秒三条同键（ts+rid 相同）事件的 fixture——组内 seq 决序。
const SAME_TS_BODY: &str = "\
{\"timestamp\":\"2026-07-17T16:00:00.000Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"s1\",\"session_id\":\"s1\",\"cwd\":\"C:\\\\w\\\\a\"}}
{\"timestamp\":\"2026-07-17T16:00:00.000Z\",\"type\":\"turn_context\",\"payload\":{\"turn_id\":\"t1\",\"model\":\"gpt-5.6-sol\",\"cwd\":\"C:\\\\w\\\\a\"}}
{\"timestamp\":\"2026-07-17T16:00:00.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":10,\"output_tokens\":0,\"cached_input_tokens\":0,\"cache_write_input_tokens\":0,\"reasoning_output_tokens\":0,\"total_tokens\":10}}}}
{\"timestamp\":\"2026-07-17T16:00:00.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":20,\"output_tokens\":0,\"cached_input_tokens\":0,\"cache_write_input_tokens\":0,\"reasoning_output_tokens\":0,\"total_tokens\":20}}}}
{\"timestamp\":\"2026-07-17T16:00:00.000Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":30,\"output_tokens\":0,\"cached_input_tokens\":0,\"cache_write_input_tokens\":0,\"reasoning_output_tokens\":0,\"total_tokens\":30}}}}
";

/// 快照建立后修改日志（同时间戳插删）：旧 query 完整遍历不重不漏、
/// 内容不变；新 query 才反映变化。
#[test]
fn paging_same_timestamp_insert_delete_keeps_snapshot_rows() {
    let _g = serial();
    let dir = hermetic("insert-delete");
    let root = dir.join("codex");
    let log = write_codex_log(&root, SAME_TS_BODY);
    let o = opts(&dir, &root);

    let snap = query::begin_query(&o).unwrap();
    // 全量遍历（limit=1 逐页）——同时间戳三行全部出现、顺序稳定。
    let mut seen: Vec<u64> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = query::query_events(
            &snap.query_id,
            &EventFilter {
                limit: Some(1),
                before: cursor.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        if page.rows.is_empty() {
            break;
        }
        assert_eq!(page.total, 3, "同一快照内 total 恒定");
        seen.push(page.rows[0].input);
        cursor = Some(page.rows[0].cursor.clone());
    }
    assert_eq!(
        seen,
        [10, 20, 30],
        "同 ts+rid 行保持原序（组内 seq 升序）: {seen:?}"
    );

    // 快照建立后修改日志：同时间戳的一行 token 数被改写（插入/删除等价）。
    // input 与 total 同步改写，保持事件自身守恒（不被解析层守恒校验跳过）。
    std::fs::write(
        &log,
        SAME_TS_BODY
            .replace("input_tokens\":20,", "input_tokens\":200001,")
            .replace("total_tokens\":20}}}", "total_tokens\":200001}}}"),
    )
    .unwrap();

    // 旧 query 再次完整遍历：内容与 total 不变（不重新采集）。
    let mut seen2: Vec<u64> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = query::query_events(
            &snap.query_id,
            &EventFilter {
                limit: Some(1),
                before: cursor.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        if page.rows.is_empty() {
            break;
        }
        assert_eq!(page.total, 3, "旧快照不随日志变化重算");
        seen2.push(page.rows[0].input);
        cursor = Some(page.rows[0].cursor.clone());
    }
    assert_eq!(seen2, seen, "旧 query 完整遍历与首次一致（不重不漏）");

    // 新 query 才反映变化：20 被替换为 200001 → total 仍 3、行序含新值。
    let snap2 = query::begin_query(&o).unwrap();
    let fresh = query::query_events(&snap2.query_id, &EventFilter::default()).unwrap();
    assert_eq!(fresh.total, 3);
    assert!(
        fresh.rows.iter().any(|r| r.input == 200001),
        "新 query 反映日志修改"
    );
    // 新会话的 query_id 必然不同。
    assert_ne!(snap.query_id, snap2.query_id);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 价格文件在快照建立后变化：旧 query 的费用与价格修订冻结不变，
/// 新 query 使用新修订（估算算法本身不变）。
#[test]
fn paging_keeps_price_revision_until_refresh() {
    let _g = serial();
    let dir = hermetic("price-rev");
    let root = dir.join("codex");
    write_codex_log(&root, SAME_TS_BODY);
    // 外置价：gpt-5.6-sol input=2/M → 请求费用 = (input+output)*2/1e6。
    let pricing = dir.join("pricing.toml");
    std::fs::write(
        &pricing,
        "[[model]]\nprefix = \"gpt-5.6-sol\"\ninput = 2.0\noutput = 2.0\n",
    )
    .unwrap();
    let o = opts(&dir, &root);

    let snap = query::begin_query(&o).unwrap();
    let page1 = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    assert!(page1.rows[0].cost_usd.unwrap() > 0.0, "外置价应计费");
    let cost1 = page1.rows.iter().map(|r| r.cost_usd.unwrap()).sum::<f64>();
    let rev1 = page1.pricing_revision.clone();

    // 价格翻倍：旧 query 不变，新 query 费用翻倍。
    std::fs::write(
        &pricing,
        "[[model]]\nprefix = \"gpt-5.6-sol\"\ninput = 4.0\noutput = 4.0\n",
    )
    .unwrap();

    let page1_again = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    let cost1_again: f64 = page1_again.rows.iter().map(|r| r.cost_usd.unwrap()).sum();
    assert_eq!(cost1_again, cost1, "旧会话分页不偷偷重算价格");
    assert_eq!(page1_again.pricing_revision, rev1, "价格修订冻结");

    let snap2 = query::begin_query(&o).unwrap();
    assert_ne!(snap2.pricing_revision, rev1, "新会话必须使用新价格修订");
    let page2 = query::query_events(&snap2.query_id, &EventFilter::default()).unwrap();
    let cost2: f64 = page2.rows.iter().map(|r| r.cost_usd.unwrap()).sum();
    assert!((cost2 - 2.0 * cost1).abs() < 1e-12, "价格翻倍 → 费用翻倍");
    let _ = std::fs::remove_dir_all(&dir);
}

/// summary 与 events 显式共享同一会话上下文：同 query_id、同价格修订、
/// 汇总请求数 == 明细 total（无下钻）。
#[test]
fn summary_and_events_share_query_context() {
    let _g = serial();
    let dir = hermetic("share");
    let root = dir.join("codex");
    write_codex_log(&root, SAME_TS_BODY);
    let o = opts(&dir, &root);

    let handle = query::begin_query_handle(&o).unwrap();
    let s = query::query_summary(&handle.query_id).unwrap();
    let l = query::query_events(&handle.query_id, &EventFilter::default()).unwrap();
    assert_eq!(s.query_id, handle.query_id);
    assert_eq!(l.query_id, handle.query_id);
    assert_eq!(s.pricing_revision, handle.pricing_revision);
    assert_eq!(l.pricing_revision, handle.pricing_revision);
    assert_eq!(
        l.total as u64, s.totals.requests,
        "同会话明细 total 与汇总请求数同源"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 过期/外会话/筛选不匹配的游标一律结构化拒绝，允许刷新重建。
#[test]
fn expired_or_foreign_cursor_requires_refresh() {
    let _g = serial();
    let dir = hermetic("expired");
    let root = dir.join("codex");
    write_codex_log(&root, SAME_TS_BODY);
    let o = opts(&dir, &root);

    let snap1 = query::begin_query(&o).unwrap();
    let page1 = query::query_events(&snap1.query_id, &EventFilter::default()).unwrap();
    let cursor = page1.rows[0].cursor.clone();

    // (a) 外会话游标：另一个会话拿到自己的游标，互相不可用。
    let snap2 = query::begin_query(&o).unwrap();
    assert_ne!(snap1.query_id, snap2.query_id);
    let err = query::query_events(
        &snap2.query_id,
        &EventFilter {
            before: Some(cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("query_expired"),
        "外会话游标必须结构化拒绝: {err}"
    );

    // (b) 下钻筛选变化后旧游标拒绝。
    let err = query::query_events(
        &snap1.query_id,
        &EventFilter {
            model: Some("gpt-5.6-sol".to_string()),
            before: Some(cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("query_expired"),
        "下钻指纹不匹配必须拒绝: {err}"
    );

    // (c) 进程重启（注册表清空）→ 会话失效。
    query::clear_query_registry_for_tests();
    let err = query::query_events(
        &snap1.query_id,
        &EventFilter {
            before: Some(cursor),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("query_expired"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 旧磁盘视图的 v1 游标（无 v/qid/指纹字段）不得续用于活跃游标翻页。
#[test]
fn old_disk_view_cannot_resume_live_cursor() {
    let _g = serial();
    let dir = hermetic("old-disk");
    let root = dir.join("codex");
    write_codex_log(&root, SAME_TS_BODY);
    let o = opts(&dir, &root);

    let handle = query::begin_query_handle(&o).unwrap();
    // 旧格式游标：v1 = {"ts":..,"rid":..,"seq":..}（无 v/qid/mfp/dfp）。
    let v1 = r#"{"ts":"2026-07-17T16:00:00Z","rid":"","seq":0}"#;
    let err = query::query_events(
        &handle.query_id,
        &EventFilter {
            before: Some(v1.to_string()),
            ..Default::default()
        },
    )
    .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("query_expired") || msg.contains("游标格式非法"),
        "v1 游标必须显式拒绝: {msg}"
    );
    // 无游标读取仍正常（旧视图数据可展示，翻页需刷新重建会话）。
    let fresh = query::query_events(&handle.query_id, &EventFilter::default()).unwrap();
    assert_eq!(fresh.total, 3);
    let _ = std::fs::remove_dir_all(&dir);
}

/// 注册表按 TTL/容量淘汰：回拨 last_used（注入时钟）后新会话建立时
/// 旧会话被回收，其游标显式过期；容量上限同样触发 LRU 淘汰。
#[test]
fn query_registry_evicts_with_explicit_expiry() {
    let _g = serial();
    let dir = hermetic("evict");
    let root = dir.join("codex");
    write_codex_log(&root, SAME_TS_BODY);
    let o = opts(&dir, &root);

    // (a) TTL：回拨超过 QUERY_IDLE_TTL → 下一次 begin 回收 → 游标过期。
    let old = query::begin_query_handle(&o).unwrap();
    let page = query::query_events(&old.query_id, &EventFilter::default()).unwrap();
    let cur = page.rows[0].cursor.clone();
    query::backdate_query_for_tests(&old.query_id, query::QUERY_IDLE_TTL);
    let _fresh = query::begin_query_handle(&o).unwrap(); // 触发 gc
    let err = query::query_events(
        &old.query_id,
        &EventFilter {
            before: Some(cur),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("query_expired"), "{err}");

    // (b) 容量：占满 MAX_ACTIVE_QUERIES 后再建一个 → 最旧被淘汰。
    query::clear_query_registry_for_tests();
    let first = query::begin_query_handle(&o).unwrap();
    let page = query::query_events(&first.query_id, &EventFilter::default()).unwrap();
    assert_eq!(page.total, 3, "淘汰前会话可用");
    let _ = page;
    let mut last = first.query_id.clone();
    for _ in 0..query::MAX_ACTIVE_QUERIES {
        last = query::begin_query_handle(&o).unwrap().query_id;
    }
    assert_ne!(last, first.query_id, "容量淘汰后新会话照常建立");
    let err = query::query_events(&first.query_id, &EventFilter::default()).unwrap_err();
    assert!(err.to_string().contains("query_expired"), "{err}");
    // 最新会话仍可用。
    let ok = query::query_events(&last, &EventFilter::default()).unwrap();
    assert_eq!(ok.total, 3);
    let _ = std::fs::remove_dir_all(&dir);
}
