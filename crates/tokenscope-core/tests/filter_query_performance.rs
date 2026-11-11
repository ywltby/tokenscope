//! F03：筛选查询的同规模合成基准。
//!
//! 5 万请求 / 四应用 / 9,000 条价格条目 / 30 天 CCS 日汇总，历史库、缓存与
//! 价格索引全部落在测试独立临时目录。基准只使用合成数据，不读取真实会话、
//! 不写默认用户目录。
//!
//! 运行（release，打印每阶段 p50/p95 与正确性指纹）：
//!   cargo test --release --test filter_query_performance -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{
    DailyRollupWrite, EventAlias, EventOrigin, EventWrite, HistoryDb, ImportRunRecord,
    ORIGIN_KIND_NATIVE_FILE, RollupConflictPolicy, WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::query;
use tokenscope::report::{self, EventFilter, SummaryOptions, Trigger};

static SEQ: AtomicU64 = AtomicU64::new(0);
const EVENTS: usize = 50_000;
const PRICE_ENTRIES: usize = 9_000;
const DAYS: usize = 30;
const MODELS: usize = 20;

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-filter-perf-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn token_counts(i: usize) -> TokenCounts {
    TokenCounts {
        input: 1_000 + (i % 97) as u64,
        output: 100 + (i % 31) as u64,
        cache_write: (i % 13) as u64,
        cache_read: 500 + (i % 53) as u64,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn apps() -> [AgentKind; 4] {
    [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::WorkBuddy,
        AgentKind::WorkBuddyAi,
    ]
}

fn alias_for(app: AgentKind, i: usize) -> EventAlias {
    let scheme = match app {
        AgentKind::ClaudeCode => "claude-message",
        AgentKind::Codex => "codex-response",
        AgentKind::WorkBuddy => "workbuddy-message",
        AgentKind::WorkBuddyAi => "workbuddy-ai-message",
    };
    EventAlias {
        app,
        scheme: scheme.to_string(),
        value: format!("synthetic-{i}"),
    }
}

fn event_ts(i: usize) -> jiff::Timestamp {
    let offset_hours = (i % DAYS) as i64 * 24;
    let offset_secs = (i as i64 * 37) % 86_400;
    jiff::Timestamp::now() - jiff::Span::new().hours(offset_hours).seconds(offset_secs)
}

fn day_key(offset_days: i64) -> String {
    (jiff::Timestamp::now() - jiff::Span::new().hours(offset_days * 24))
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

fn event_write(i: usize) -> EventWrite {
    let app = apps()[i % 4];
    let model = format!("synthetic-model-{}", i % MODELS);
    let ts = event_ts(i);
    let key = format!(
        "native|{}|C:/synthetic/perf.jsonl|{i}|perf-{i}",
        app.as_str()
    );
    EventWrite {
        event_key: key.clone(),
        app,
        ts,
        model_raw: model.clone(),
        model_identity: ModelIdentity::parse(&model).identity_key(),
        session_id: Some(format!("session-{}", i / 50)),
        record_id: Some(format!("resp-{i}")),
        project_key: Some(format!("C:/synthetic/project-{}", i % 10)),
        session_initial_cwd: Some("C:/synthetic".to_string()),
        event_cwd: Some("C:/synthetic".to_string()),
        tokens: token_counts(i),
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases: vec![alias_for(app, i)],
        origins: vec![EventOrigin {
            origin_kind: ORIGIN_KIND_NATIVE_FILE.to_string(),
            origin_key: key,
            app,
            import_run_id: None,
            parser_revision: tokenscope::history::PARSER_REVISION,
            source_model_raw: None,
            pricing_model: None,
            source_cost_usd: None,
            ts_precision_seconds: false,
        }],
    }
}

fn write_pricing(path: &Path) {
    let mut text = String::new();
    for i in 0..PRICE_ENTRIES {
        text.push_str(&format!(
            "[[model]]\nprefix = \"synthetic-model-{i}\"\ninput = 1.{:02}\noutput = 4.{:02}\ncache_write = 1.25\ncache_read = 0.25\n\n",
            i % 100,
            i % 100,
        ));
    }
    std::fs::write(path, text).unwrap();
}

fn seed_history(db_path: &Path) {
    let db = HistoryDb::open(db_path).unwrap();
    let mut batch = Vec::with_capacity(1_000);
    for i in 0..EVENTS {
        batch.push(event_write(i));
        if batch.len() == 1_000 {
            db.write_batch(&batch).unwrap();
            batch.clear();
        }
    }
    if !batch.is_empty() {
        db.write_batch(&batch).unwrap();
    }

    let rollups: Vec<DailyRollupWrite> = (0..DAYS as i64)
        .flat_map(|offset| {
            [0usize, 1]
                .into_iter()
                .map(move |model_index| DailyRollupWrite {
                    logical_source: "ccs-synthetic".to_string(),
                    day: day_key(offset),
                    source_tz: "UTC".to_string(),
                    app: AgentKind::ClaudeCode,
                    provider_id: "synthetic".to_string(),
                    model: format!("synthetic-model-{model_index}"),
                    request_model: format!("synthetic-model-{model_index}"),
                    pricing_model: format!("synthetic-model-{model_index}"),
                    request_count: 12,
                    tokens: TokenCounts {
                        input: 12_000,
                        output: 1_200,
                        cache_write: 120,
                        cache_read: 6_000,
                        cache_write_5m: 0,
                        cache_write_1h: 0,
                        cache_write_split_known: false,
                    },
                    input_semantics: 2,
                    source_cost_usd: None,
                    revision: 1,
                })
        })
        .collect();
    let run = ImportRunRecord {
        logical_source: "ccs-synthetic".to_string(),
        source_path: "C:/synthetic/cc-switch.db".to_string(),
        source_schema: "synthetic".to_string(),
        started_utc: "2026-09-30T23:00:00Z".to_string(),
        status: "succeeded".to_string(),
        detail: None,
        import_revision: 1,
    };
    db.apply_import_batch(
        &[],
        &rollups,
        RollupConflictPolicy::KeepExisting,
        &run,
        None,
    )
    .unwrap();
    assert_eq!(db.event_count().unwrap(), EVENTS as u64);
    assert_eq!(db.rollup_count().unwrap(), (DAYS * 2) as u64);
}

fn base_opts(dir: &Path) -> SummaryOptions {
    let data = dir.join("data");
    SummaryOptions {
        workbuddy_enabled: Some(true),
        workbuddy_ai_enabled: Some(true),
        by: GroupBy::Day,
        agent: None,
        claude_dir: Some(dir.join("sources").join("claude")),
        codex_dir: Some(dir.join("sources").join("codex")),
        workbuddy_dir: Some(dir.join("sources").join("workbuddy")),
        workbuddy_ai_dir: Some(dir.join("sources").join("workbuddy-ai")),
        claude_enabled: Some(true),
        codex_enabled: Some(true),
        cache_dir: Some(data.clone()),
        history_path: Some(data.join("history.db")),
        pricing_index: Some(data.join("pricing-index.json")),
        pricing_path: Some(data.join("pricing.toml")),
        openrouter_path: Some(data.join("no-openrouter.json")),
        modelsdev_path: Some(data.join("no-modelsdev.json")),
        tz: Some("UTC".to_string()),
        trigger: Trigger::Manual,
        ..Default::default()
    }
}

#[derive(Clone, Copy)]
enum Preset {
    All,
    Days7,
    Rolling24,
}

#[derive(Clone, Copy)]
struct Variant {
    name: &'static str,
    agent: Option<AgentKind>,
    by: GroupBy,
    preset: Preset,
}

const VARIANTS: [Variant; 10] = [
    Variant {
        name: "all-day",
        agent: None,
        by: GroupBy::Day,
        preset: Preset::All,
    },
    Variant {
        name: "claude-model",
        agent: Some(AgentKind::ClaudeCode),
        by: GroupBy::Model,
        preset: Preset::All,
    },
    Variant {
        name: "all-day-7",
        agent: None,
        by: GroupBy::Day,
        preset: Preset::Days7,
    },
    Variant {
        name: "all-project",
        agent: None,
        by: GroupBy::Project,
        preset: Preset::All,
    },
    Variant {
        name: "all-agent",
        agent: None,
        by: GroupBy::Agent,
        preset: Preset::All,
    },
    Variant {
        name: "claude-day",
        agent: Some(AgentKind::ClaudeCode),
        by: GroupBy::Day,
        preset: Preset::All,
    },
    Variant {
        name: "claude-day-7",
        agent: Some(AgentKind::ClaudeCode),
        by: GroupBy::Day,
        preset: Preset::Days7,
    },
    Variant {
        name: "all-day-24h",
        agent: None,
        by: GroupBy::Day,
        preset: Preset::Rolling24,
    },
    Variant {
        name: "all-model",
        agent: None,
        by: GroupBy::Model,
        preset: Preset::All,
    },
    Variant {
        name: "all-day-repeat",
        agent: None,
        by: GroupBy::Day,
        preset: Preset::All,
    },
];

fn variant_opts(base: &SummaryOptions, variant: Variant) -> SummaryOptions {
    let mut opts = base.clone();
    opts.agent = variant.agent;
    opts.by = variant.by;
    opts.days = None;
    opts.rolling_hours = None;
    match variant.preset {
        Preset::All => {}
        Preset::Days7 => opts.days = Some(7),
        Preset::Rolling24 => opts.rolling_hours = Some(24),
    }
    opts
}

type BenchRow = (String, Vec<f64>, Vec<f64>, Vec<f64>, String);

fn pct(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(|a, b| a.total_cmp(b));
    if samples.is_empty() {
        return 0.0;
    }
    let index = ((samples.len() - 1) as f64 * p).round() as usize;
    samples[index]
}

/// 一次筛选变体的完整读取：建快照（行索引/覆盖桶）→ 汇总 → 明细首页。
fn run_variant(opts: &SummaryOptions) -> (f64, f64, f64, String) {
    let started = Instant::now();
    let snapshot = query::begin_query_with(opts, false).expect("筛选查询建快照失败");
    let begin_ms = started.elapsed().as_secs_f64() * 1000.0;

    let started = Instant::now();
    let summary = query::query_summary(&snapshot.query_id).expect("汇总读取失败");
    let summary_ms = started.elapsed().as_secs_f64() * 1000.0;

    let started = Instant::now();
    let events = query::query_events(
        &snapshot.query_id,
        &EventFilter {
            limit: Some(200),
            ..Default::default()
        },
    )
    .expect("明细首页读取失败");
    let events_ms = started.elapsed().as_secs_f64() * 1000.0;

    let fingerprint = format!(
        "{}|{}|{}|{}|{}|{}",
        summary.totals.requests,
        summary.totals.tokens.total(),
        summary.groups.len(),
        summary.by,
        events.rows.len(),
        events.total
    );
    (begin_ms, summary_ms, events_ms, fingerprint)
}

#[test]
#[ignore = "同规模合成基准，按需 release 运行并记录前后数字"]
fn filter_query_performance_50k() {
    let dir = temp_dir("50k");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for app in ["claude", "codex", "workbuddy", "workbuddy-ai"] {
        std::fs::create_dir_all(dir.join("sources").join(app)).unwrap();
    }
    write_pricing(&data.join("pricing.toml"));
    seed_history(&data.join("history.db"));
    let base = base_opts(&dir);

    // 启动/首次装载另算：采集（空来源树，只建最近采集快照并建价格索引）。
    let started = Instant::now();
    let collected = report::collect_only(&base).expect("采集失败");
    let collect_ms = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "启动采集（空合成来源）：{collect_ms:.1} ms，generation {}，changed {}",
        collected.generation, collected.changed
    );

    // 暖轮：先完整跑一遍，建立 SQLite 页缓存与价格索引。
    for variant in VARIANTS {
        let opts = variant_opts(&base, variant);
        let _ = run_variant(&opts);
    }

    let mut rows: Vec<BenchRow> = Vec::new();
    for variant in VARIANTS {
        let opts = variant_opts(&base, variant);
        let mut begin = Vec::new();
        let mut summary = Vec::new();
        let mut events = Vec::new();
        let mut fingerprint = String::new();
        for _ in 0..5 {
            let (b, s, e, fp) = run_variant(&opts);
            begin.push(b);
            summary.push(s);
            events.push(e);
            fingerprint = fp;
        }
        rows.push((
            variant.name.to_string(),
            begin,
            summary,
            events,
            fingerprint,
        ));
    }

    println!("\n阶段耗时（暖态 5 轮，ms）：");
    println!(
        "{:<18} {:>18} {:>18} {:>18}  指纹",
        "变体", "建快照 p50/p95", "汇总 p50/p95", "明细 p50/p95"
    );
    let mut all_begin = Vec::new();
    let mut all_summary = Vec::new();
    let mut all_events = Vec::new();
    for (name, mut begin, mut summary, mut events, fingerprint) in rows {
        let b50 = pct(&mut begin, 0.5);
        let b95 = pct(&mut begin, 0.95);
        let s50 = pct(&mut summary, 0.5);
        let s95 = pct(&mut summary, 0.95);
        let e50 = pct(&mut events, 0.5);
        let e95 = pct(&mut events, 0.95);
        all_begin.push(b50);
        all_summary.push(s50);
        all_events.push(e50);
        println!(
            "{name:<18} {b50:>8.1}/{b95:<9.1} {s50:>8.1}/{s95:<9.1} {e50:>8.1}/{e95:<9.1}  {fingerprint}"
        );
    }
    println!(
        "跨变体合计 p50：建快照 {:.1} ms；汇总 {:.1} ms；明细 {:.1} ms",
        pct(&mut all_begin, 0.5),
        pct(&mut all_summary, 0.5),
        pct(&mut all_events, 0.5)
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 覆盖桶只为已有日汇总涉及的应用扫描（确定性工作量回归，acceptance）。
#[cfg(feature = "acceptance")]
#[test]
fn detail_bucket_scan_only_reads_apps_with_rollups() {
    let dir = temp_dir("scan-scope");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for app in ["claude", "codex", "workbuddy", "workbuddy-ai"] {
        std::fs::create_dir_all(dir.join("sources").join(app)).unwrap();
    }
    write_pricing(&data.join("pricing.toml"));
    let db = HistoryDb::open(&data.join("history.db")).unwrap();
    let writes: Vec<EventWrite> = (0..4_000).map(event_write).collect();
    db.write_batch(&writes).unwrap();
    let rollups = vec![DailyRollupWrite {
        logical_source: "ccs-synthetic".to_string(),
        day: day_key(0),
        source_tz: "UTC".to_string(),
        app: AgentKind::ClaudeCode,
        provider_id: "synthetic".to_string(),
        model: "synthetic-model-0".to_string(),
        request_model: "synthetic-model-0".to_string(),
        pricing_model: "synthetic-model-0".to_string(),
        request_count: 1,
        tokens: TokenCounts {
            input: 1,
            output: 1,
            cache_write: 0,
            cache_read: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        },
        input_semantics: 2,
        source_cost_usd: None,
        revision: 1,
    }];
    let run = ImportRunRecord {
        logical_source: "ccs-synthetic".to_string(),
        source_path: "C:/synthetic/cc-switch.db".to_string(),
        source_schema: "synthetic".to_string(),
        started_utc: jiff::Timestamp::now().to_string(),
        status: "succeeded".to_string(),
        detail: None,
        import_revision: 1,
    };
    db.apply_import_batch(
        &[],
        &rollups,
        RollupConflictPolicy::KeepExisting,
        &run,
        None,
    )
    .unwrap();
    drop(db);
    let base = base_opts(&dir);
    report::collect_only(&base).expect("采集失败");
    let _ = report::take_detail_bucket_scan_count_for_tests();
    let snapshot = query::begin_query_with(&base, false).expect("查询失败");
    let scanned = report::take_detail_bucket_scan_count_for_tests();
    assert!(scanned > 0, "有日汇总时必须执行覆盖桶统计");
    assert!(
        scanned < 4_000,
        "只为有日汇总的应用扫描，实际 {scanned} / 4000"
    );
    drop(snapshot);
    std::fs::remove_dir_all(&dir).ok();
}

/// 小规模确定性回归：同样操作在调试构建下也必须给出稳定结果（不设绝对耗时）。
#[test]
fn filter_query_sequence_is_deterministic() {
    let dir = temp_dir("small");
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    for app in ["claude", "codex", "workbuddy", "workbuddy-ai"] {
        std::fs::create_dir_all(dir.join("sources").join(app)).unwrap();
    }
    write_pricing(&data.join("pricing.toml"));
    let db = HistoryDb::open(&data.join("history.db")).unwrap();
    let writes: Vec<EventWrite> = (0..2_000).map(event_write).collect();
    db.write_batch(&writes).unwrap();
    drop(db);
    let base = base_opts(&dir);
    report::collect_only(&base).expect("采集失败");

    let first = (0..VARIANTS.len())
        .map(|index| run_variant(&variant_opts(&base, VARIANTS[index])).3)
        .collect::<Vec<_>>();
    for _ in 0..2 {
        let again = (0..VARIANTS.len())
            .map(|index| run_variant(&variant_opts(&base, VARIANTS[index])).3)
            .collect::<Vec<_>>();
        assert_eq!(again, first, "同一筛选序列必须得到稳定指纹");
    }
    std::fs::remove_dir_all(&dir).ok();
}
