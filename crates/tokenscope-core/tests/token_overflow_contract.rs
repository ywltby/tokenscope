//! SF08（安全与数据一致性审查 Task 8）：异常 token 使用受检算术。
//!
//! 不变量（docs/stats-semantics.md §2.1）：
//! 1. codex 零值判断逐字段比较；守恒校验（total = input + output，
//!    cached + cache_write ≤ input）全部受检——异常行计 bad_lines 跳过，
//!    后续正常行存活；debug/release 语义一致（无回绕、无 panic）；
//! 2. 聚合层单事件更新先在临时值上全部成功再提交；多个**各自合法**的
//!    事件累计溢出 → 整体明确报错（Result），不产生部分结果；
//! 3. 缓存恢复与适配器解析经过同一桶校验边界——旧缓存不合法事件不得
//!    绕过新检查（拒绝并计 bad_lines）。
//!
//! 全部合成 fixture + 临时目录；release 运行只覆盖合成数据（绝不加
//! `--ignored`）。

use std::path::PathBuf;

use tokenscope::aggregate::{GroupBy, aggregate};
use tokenscope::cache::{Cache, FileKey};
use tokenscope::history::{
    DailyRollupWrite, EventWrite, HistoryDb, ImportRunRecord, RollupConflictPolicy, WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::pricing::Pricing;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions};
use tokenscope::source::{CollectStats, FileParse, Source};

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-overflow-{tag}-{}-{}",
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

/// codex 事件行构造（session_meta + turn_context + token_count）。
fn codex_file(input: &str, output: &str, cached: &str, cw: &str, total: &str) -> String {
    let usage_line = format!(
        r#"{{"timestamp":"2026-07-17T15:02:00.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"output_tokens":{output},"cached_input_tokens":{cached},"cache_write_input_tokens":{cw},"reasoning_output_tokens":0,"total_tokens":{total}}}}}}}}}"#
    );
    format!(
        "{}\n{}\n{}\n",
        r#"{"timestamp":"2026-07-17T15:00:00.000Z","type":"session_meta","payload":{"id":"s1","session_id":"s1","cwd":"C:/w/a"}}"#,
        r#"{"timestamp":"2026-07-17T15:01:00.000Z","type":"turn_context","payload":{"turn_id":"t1","model":"m","cwd":"C:/w/a"}}"#,
        usage_line,
    )
}

/// codex 异常行跳过 + 后续正常行存活；debug/release 一致。
/// 覆盖：多字段溢出（total 加法溢出）、cached 子集溢出、子集不成立、
/// 单字段接近 u64::MAX 的合法 JSON、零分量行。
#[test]
fn overflowing_codex_line_is_skipped_and_next_line_survives() {
    let dir = fresh_dir("codex-lines");
    let file = dir.join("rollout-x.jsonl");
    let max = u64::MAX.to_string();
    let mut body = String::new();
    body.push_str(&codex_file("1", "1", "0", "0", "2")); // 正常行
    body.push_str(&codex_file(&max, "1", "0", "0", "2")); // total 溢出（max+1）
    body.push_str(&codex_file("5", "0", &max, "0", "5")); // cached+0 溢出? cached=MAX 自身不溢出，但子集 5 < MAX
    body.push_str(&codex_file("5", "5", "4", "4", "10")); // cached+cw=8 ≤ 5? 否——子集不成立
    body.push_str(&codex_file("100", "0", "60", "40", "100")); // 正常行（子集恰好成立）
    std::fs::write(&file, body).unwrap();

    let source = tokenscope::source::codex::CodexSource::new(dir.clone());
    let (files, errors) = source.discover_with_errors();
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(files.len(), 1);
    let parse = source.parse_file(&files[0]);

    // 异常行全部跳过：仅 2 条正常事件存活；坏行 = 溢出行 + 子集违规行。
    assert_eq!(parse.events.len(), 2, "异常行必须跳过，正常行存活");
    assert_eq!(
        parse.stats.bad_lines, 3,
        "total 溢出/子集溢出/子集违规各计一坏行"
    );
    // 正常行的数字保持原值（黄金 fixture 数字不变）。
    assert_eq!(parse.events[0].input_tokens, 1);
    assert_eq!(parse.events[1].input_tokens, 0, "cached+cw 全部扣除");
    assert_eq!(parse.events[1].cache_read_tokens, 60);
    assert_eq!(parse.events[1].cache_write_tokens, 40);

    // 单字段接近 u64::MAX 的合法 JSON：四桶可表示 → 合法事件。
    let mut body2 = codex_file(&max, "0", "0", "0", &max);
    body2.push_str(&format!(
        "{}\n{}\n{}\n",
        r#"{"timestamp":"2026-07-17T16:00:00.000Z","type":"session_meta","payload":{"id":"s2","session_id":"s2","cwd":"C:/w/a"}}"#,
        r#"{"timestamp":"2026-07-17T16:01:00.000Z","type":"turn_context","payload":{"turn_id":"t2","model":"m","cwd":"C:/w/a"}}"#,
        r#"{"timestamp":"2026-07-17T16:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":7}}}}"#
    ));
    let file2 = dir.join("rollout-y.jsonl");
    std::fs::write(&file2, body2).unwrap();
    let (files2, _) = source.discover_with_errors();
    let f2 = files2
        .iter()
        .find(|f| f.ends_with("rollout-y.jsonl"))
        .unwrap();
    let parse2 = source.parse_file(f2);
    assert_eq!(parse2.events.len(), 1, "MAX 单字段合法行必须保留");
    assert_eq!(parse2.events[0].input_tokens, u64::MAX);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 多个各自合法的事件累计超过 `u64::MAX`（乃至远超 `u64::MAX`）→ 聚合总计
/// 保持任意精度准确结果（不溢出、不回绕），JSON 在超过 JS `MAX_SAFE_INTEGER`
/// 时输出十进制字符串以防前端截断。
#[test]
fn aggregate_sums_beyond_u64_max_with_arbitrary_precision() {
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let mk = |input: u64| UsageEvent {
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
        ts: "2026-07-17T15:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "m".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: String::new(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 0,
        source_path: String::new(),
        input_tokens: input,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    };
    // 各自合法（单条事件仍受 u64 约束），聚合总计无缝突破 u64::MAX。
    let events = vec![mk(u64::MAX - 10), mk(20)];
    let agg = aggregate(&events, GroupBy::Day, &tz, &Pricing::default()).unwrap();
    assert_eq!(
        agg.totals.tokens.input.to_string(),
        "18446744073709551625",
        "累计超过 u64::MAX 必须保留精确大整数"
    );
    assert_eq!(
        agg.totals.unknown_tokens.input.to_string(),
        "18446744073709551625",
        "未定价聚合累计同样保留精确大整数"
    );
    let json = serde_json::to_value(&agg.totals).unwrap();
    assert_eq!(
        json["tokens"]["input"],
        serde_json::json!("18446744073709551625"),
        "超过 JS MAX_SAFE_INTEGER 必须以十进制字符串序列化"
    );

    // 连续多条事件累计超过 10^20，验证多片进位。
    let huge_events = vec![mk(u64::MAX); 6];
    let huge = aggregate(&huge_events, GroupBy::Day, &tz, &Pricing::default()).unwrap();
    assert_eq!(
        huge.totals.tokens.input.to_string(),
        (u128::from(u64::MAX) * 6).to_string()
    );

    // 常规安全范围内的数字仍以 JSON 整数序列化，保持向后兼容。
    let ok = aggregate(&[mk(1), mk(2)], GroupBy::Day, &tz, &Pricing::default()).unwrap();
    assert_eq!(ok.totals.tokens.input, 3);
    assert_eq!(
        serde_json::to_value(&ok.totals).unwrap()["tokens"]["input"],
        serde_json::json!(3)
    );
}

/// 旧缓存中存有越界事件（旧版本写入）→ 恢复路径同一校验边界拒绝，
/// 不得绕过新检查；同文件的好事件照常恢复。
#[test]
fn legacy_cached_event_cannot_bypass_validation() {
    let dir = fresh_dir("legacy-cache");
    let cache = Cache::open(&dir.join("cache.db")).unwrap();
    // i64::MAX 单字段可入库（旧版 as i64 不回绕），但多字段组合溢出 u64
    // —— 模拟旧版本缓存中的越界组合事件。
    let i64_max = i64::MAX as u64;
    let bad = UsageEvent {
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
        ts: "2026-07-17T15:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "bad".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "bad-1".into(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 0,
        source_path: String::new(),
        input_tokens: i64_max,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    };
    let good = UsageEvent {
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
        ts: "2026-07-17T15:01:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "good".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "good-1".into(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 0,
        source_path: String::new(),
        input_tokens: 10,
        output_tokens: 5,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    };
    let parse = FileParse {
        stats: CollectStats {
            lines_seen: 2,
            bad_lines: 0,
            events: 2,
            ..CollectStats::default()
        },
        events: vec![bad, good],
    };
    cache
        .store_file(
            &FileKey {
                path: "p/bad.jsonl",
                agent: AgentKind::ClaudeCode,
                root: "root",
                context_rev: "test-ctx",
            },
            100,
            1_000,
            &parse,
        )
        .unwrap();
    // 模拟旧版本缓存的越界组合：直接改库把 cache_write/cache_read 抬到
    // i64::MAX（新 store 路径会拒绝该组合，此处绕过以构造"旧数据"）。
    cache
        .execute_raw_for_tests(
            "UPDATE events SET cache_write = ?1, cache_read = ?1 WHERE record_id = 'bad-1'",
            &[i64_max as i64],
        )
        .unwrap();

    let cached = cache
        .lookup_file(
            &FileKey {
                path: "p/bad.jsonl",
                agent: AgentKind::ClaudeCode,
                root: "root",
                context_rev: "test-ctx",
            },
            100,
            1_000,
        )
        .unwrap()
        .expect("指纹一致必须命中");
    assert_eq!(
        cached.parse.events.len(),
        1,
        "越界事件不得经缓存恢复绕过校验"
    );
    assert_eq!(cached.parse.events[0].record_id, "good-1");
    assert!(
        cached.parse.stats.bad_lines >= 1,
        "恢复期拒绝必须可见（计坏行）: {:?}",
        cached.parse.stats
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// TokenCounts 受检累加的单元契约（供聚合层依赖）。
#[test]
fn token_counts_checked_add_overflows_to_none() {
    let a = TokenCounts {
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
        input: u64::MAX,
        output: 0,
        cache_write: 0,
        cache_read: 0,
    };
    let b = TokenCounts {
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
        input: 1,
        output: 0,
        cache_write: 0,
        cache_read: 0,
    };
    assert!(a.checked_add(&b).is_none());
    assert!(b.checked_add(&a).is_none());
    assert!(TokenCounts::default().checked_add(&b).is_some());
}

fn big_ccs_opts(dir: &std::path::Path) -> SummaryOptions {
    let empty_claude = dir.join("empty-claude");
    std::fs::create_dir_all(&empty_claude).unwrap();
    SummaryOptions {
        by: GroupBy::Model,
        claude_dir: Some(empty_claude),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

fn ccs_rollup_row(
    provider: &str,
    model: &str,
    requests: u64,
    tokens: TokenCounts,
) -> DailyRollupWrite {
    DailyRollupWrite {
        logical_source: "ccs".into(),
        day: "2026-07-17".into(),
        source_tz: "Asia/Shanghai".into(),
        app: AgentKind::ClaudeCode,
        provider_id: provider.into(),
        model: model.into(),
        request_model: String::new(),
        pricing_model: String::new(),
        request_count: requests,
        tokens,
        input_semantics: 2,
        source_cost_usd: None,
        revision: 0,
    }
}

fn ccs_import_run(started_utc: &str) -> ImportRunRecord {
    ImportRunRecord {
        logical_source: "ccs".into(),
        source_path: "synthetic".into(),
        source_schema: "test".into(),
        started_utc: started_utc.into(),
        status: "committed".into(),
        detail: None,
        import_revision: 2,
    }
}

fn native_event_write(key: &str, ts_str: &str, model: &str, tokens: TokenCounts) -> EventWrite {
    let ts: jiff::Timestamp = ts_str.parse().unwrap();
    EventWrite {
        event_key: key.into(),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: model.into(),
        model_identity: ModelIdentity::parse(model).identity_key(),
        session_id: Some("s1".into()),
        record_id: None,
        project_key: Some("/repo/demo".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens,
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases: Vec::new(),
        origins: Vec::new(),
    }
}

/// 两条各自合法的 CCS 日汇总落入同一桶且四桶累计突破 `u64::MAX`：
/// 汇总桶选择、聚合总计与日汇总下钻行均必须按任意精度准确返回，不报溢出。
#[test]
fn ccs_same_bucket_rollups_and_drilldown_sum_beyond_u64_max() {
    let dir = fresh_dir("ccs-rollup-big");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let t1 = TokenCounts {
        input: u64::MAX - 10,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let t2 = TokenCounts {
        input: 20,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    history
        .apply_import_batch(
            &[],
            &[
                ccs_rollup_row("prov-a", "claude-big-model", 1, t1),
                ccs_rollup_row("prov-b", "claude-big-model", 1, t2),
            ],
            RollupConflictPolicy::KeepExisting,
            &ccs_import_run("2026-07-18T00:00:00Z"),
            None,
        )
        .unwrap();

    let snap = query::begin_query(&big_ccs_opts(&dir)).unwrap();
    let sum = query::query_summary(&snap.query_id).unwrap();
    let expected = "18446744073709551625";
    assert_eq!(sum.totals.tokens.input.to_string(), expected);
    assert_eq!(sum.totals.unknown_tokens.input.to_string(), expected);
    assert_eq!(
        serde_json::to_value(&sum.totals).unwrap()["tokens"]["input"],
        serde_json::json!(expected)
    );

    let events = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    assert_eq!(
        events.rows.len(),
        1,
        "同桶两条日汇总应合并为一条下钻日汇总行"
    );
    let row = &events.rows[0];
    assert_eq!(row.input.to_string(), expected);
    let meta = row.rollup.as_ref().expect("必须携带日汇总元数据");
    assert_eq!(meta.unknown_tokens.input.to_string(), expected);
    assert_eq!(
        serde_json::to_value(row).unwrap()["input"],
        serde_json::json!(expected)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 加入 CCS 覆盖核对后，原生明细同桶累计突破 `u64::MAX`（含 A 类明细覆盖汇总、
/// B 类汇总覆盖明细、以及批次写入 `inserted_tokens`）全程按任意精度完成，不报溢出。
#[test]
fn native_details_beyond_u64_max_survive_ccs_coverage_check_class_a_and_b() {
    let expected_detail = "18446744073709551625"; // u64::MAX + 10
    let expected_rollup = "18446744073709551665"; // u64::MAX + 50
    let t1 = TokenCounts {
        input: u64::MAX - 10,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let t2 = TokenCounts {
        input: 20,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };

    // 1) A 类：原生明细合计 > u64::MAX，且覆盖同桶较小的 CCS 日汇总
    let dir_a = fresh_dir("ccs-cov-a");
    let history_a = HistoryDb::open(&dir_a.join("data").join("history.db")).unwrap();
    let batch_summary = history_a
        .write_batch(&[
            native_event_write("ev-1", "2026-07-17T08:00:00Z", "claude-big-model", t1),
            native_event_write("ev-2", "2026-07-17T09:00:00Z", "claude-big-model", t2),
        ])
        .unwrap();
    assert_eq!(
        batch_summary.inserted_tokens.input.to_string(),
        expected_detail,
        "单批写入 inserted_tokens 同样支持任意精度"
    );
    history_a
        .apply_import_batch(
            &[],
            &[ccs_rollup_row(
                "prov-small",
                "claude-big-model",
                1,
                TokenCounts {
                    input: 100,
                    ..TokenCounts::default()
                },
            )],
            RollupConflictPolicy::KeepExisting,
            &ccs_import_run("2026-07-18T00:00:00Z"),
            None,
        )
        .unwrap();

    let snap_a = query::begin_query(&big_ccs_opts(&dir_a)).unwrap();
    let sum_a = query::query_summary(&snap_a.query_id).unwrap();
    assert_eq!(sum_a.totals.tokens.input.to_string(), expected_detail);
    assert_eq!(sum_a.rollup_coverage.detail_selected_buckets, 1);

    // 2) B 类：原生明细合计 > u64::MAX，且被同桶两条更大的 CCS 日汇总覆盖
    let dir_b = fresh_dir("ccs-cov-b");
    let history_b = HistoryDb::open(&dir_b.join("data").join("history.db")).unwrap();
    history_b
        .write_batch(&[
            native_event_write("ev-1", "2026-07-17T08:00:00Z", "claude-big-model", t1),
            native_event_write("ev-2", "2026-07-17T09:00:00Z", "claude-big-model", t2),
        ])
        .unwrap();
    let r2 = TokenCounts {
        input: 60,
        ..TokenCounts::default()
    };
    history_b
        .apply_import_batch(
            &[],
            &[
                ccs_rollup_row("prov-a", "claude-big-model", 2, t1),
                ccs_rollup_row("prov-b", "claude-big-model", 1, r2),
            ],
            RollupConflictPolicy::KeepExisting,
            &ccs_import_run("2026-07-18T00:00:00Z"),
            None,
        )
        .unwrap();

    let snap_b = query::begin_query(&big_ccs_opts(&dir_b)).unwrap();
    let sum_b = query::query_summary(&snap_b.query_id).unwrap();
    assert_eq!(sum_b.rollup_coverage.covered_buckets, 1);
    assert_eq!(
        sum_b
            .rollup_coverage
            .covered_detail_tokens
            .input
            .to_string(),
        expected_detail
    );
    assert_eq!(sum_b.totals.tokens.input.to_string(), expected_rollup);

    let events_b = query::query_events(&snap_b.query_id, &EventFilter::default()).unwrap();
    let rollup_row = events_b
        .rows
        .iter()
        .find(|r| r.rollup.is_some())
        .expect("B 类应包含合并后的日汇总下钻行");
    assert_eq!(rollup_row.input.to_string(), expected_rollup);

    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}

/// 单条请求 token 超过 JS `Number.MAX_SAFE_INTEGER`（如 9,007,199,254,740,993 / 9,007,199,254,740,995）：
/// 明细行与费用浮层 `cost_breakdown`（`basis_value`、`lines[*].tokens`、`unknown.*`）
/// 必须按同一大整数规则输出精确十进制字符串，避免前端 `JSON.parse()` 尾数漂移。
#[test]
fn single_request_cost_breakdown_preserves_tokens_beyond_js_max_safe_int() {
    let dir = fresh_dir("breakdown-js-safe-int");
    // 部分定价：input 有价、output 缺价 → 同时覆盖已计价分项、未计价分项与 unknown 桶。
    std::fs::write(
        dir.join("md.json"),
        serde_json::json!({
            "v": 4,
            "synced_at": "2026-11-27T00:00:00Z",
            "entries": [{
                "id": "anthropic/claude-big-safe",
                "name": "Claude Big Safe",
                "input": 3.0
            }]
        })
        .to_string(),
    )
    .unwrap();

    let input_big: u64 = 9_007_199_254_740_993; // 2^53 + 1
    let output_big: u64 = 9_007_199_254_740_995; // 2^53 + 3
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    history
        .write_batch(&[native_event_write(
            "ev-js-unsafe",
            "2026-07-17T08:00:00Z",
            "claude-big-safe",
            TokenCounts {
                input: input_big,
                output: output_big,
                cache_write: 0,
                cache_read: 0,
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
            },
        )])
        .unwrap();

    let snap = query::begin_query(&big_ccs_opts(&dir)).unwrap();
    let events = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    assert_eq!(events.rows.len(), 1);
    let json = serde_json::to_value(&events.rows[0]).unwrap();
    assert_eq!(json["input"], serde_json::json!("9007199254740993"));
    assert_eq!(json["output"], serde_json::json!("9007199254740995"));

    let bd = &json["cost_breakdown"];
    assert_eq!(
        bd["basis_value"],
        serde_json::json!("9007199254740993"),
        "计价基数超过 2^53 - 1 必须以十进制字符串输出"
    );
    let lines = bd["lines"].as_array().expect("lines 必须存在");
    let input_line = lines.iter().find(|l| l["kind"] == "input").unwrap();
    let output_line = lines.iter().find(|l| l["kind"] == "output").unwrap();
    assert_eq!(input_line["tokens"], serde_json::json!("9007199254740993"));
    assert_eq!(output_line["tokens"], serde_json::json!("9007199254740995"));
    assert_eq!(
        bd["unknown"]["output"],
        serde_json::json!("9007199254740995"),
        "未定价 token 分项超过 2^53 - 1 必须以十进制字符串输出"
    );
    // 安全范围内的 0 仍保持 JSON 整数
    assert_eq!(bd["unknown"]["input"], serde_json::json!(0));

    let _ = std::fs::remove_dir_all(&dir);
}

/// 单条 CCS 日汇总各桶各自在 `0..=i64::MAX` 合法范围内，但四桶合计或 prompt 合计超过 `u64::MAX`：
/// 写入判空（`write_rollups_in_tx`）、日汇总估算（`estimate_rollup` / `covers_used_tokens`）
/// 与汇总/下钻查询均不得 panic 或静默回绕。
#[test]
fn single_ccs_rollup_row_with_four_bucket_sum_beyond_u64_max_does_not_panic_or_wrap() {
    let dir = fresh_dir("ccs-single-row-four-bucket-sum");
    // 部分定价：input 有价、output 缺价 → 触发 estimate_rollup + covers_used_tokens + unknown 判定。
    std::fs::write(
        dir.join("md.json"),
        serde_json::json!({
            "v": 4,
            "synced_at": "2026-11-27T00:00:00Z",
            "entries": [{
                "id": "anthropic/claude-big-rollup",
                "name": "Claude Big Rollup",
                "input": 3.0
            }]
        })
        .to_string(),
    )
    .unwrap();

    let half_plus = i64::MAX as u64; // 9,223,372,036,854,775,807
    let row_tokens = TokenCounts {
        input: half_plus,
        output: half_plus,
        cache_write: half_plus,
        cache_read: 10,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    assert!(
        row_tokens.total_u128() > u128::from(u64::MAX),
        "前置条件：单条日汇总四桶合计超过 u64::MAX"
    );

    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let outcome = history
        .apply_import_batch(
            &[],
            &[ccs_rollup_row(
                "prov-big",
                "claude-big-rollup",
                5,
                row_tokens,
            )],
            RollupConflictPolicy::KeepExisting,
            &ccs_import_run("2026-07-18T00:00:00Z"),
            None,
        )
        .unwrap();
    assert_eq!(outcome.rollups.inserted, 1);

    let snap = query::begin_query(&big_ccs_opts(&dir)).unwrap();
    let sum = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(sum.totals.tokens.input.to_string(), half_plus.to_string());
    assert_eq!(sum.totals.tokens.output.to_string(), half_plus.to_string());
    assert_eq!(
        sum.totals.tokens.cache_write.to_string(),
        half_plus.to_string()
    );
    assert_eq!(sum.totals.tokens.cache_read.to_string(), "10");
    assert!(
        sum.totals.has_estimated_cost,
        "input 分项已获估算覆盖，covers_used_tokens 必须精确判真"
    );
    assert!(
        sum.totals.cost_partially_unknown,
        "缺价的 output 分项必须保留为未定价"
    );
    assert_eq!(
        sum.totals.unknown_tokens.output.to_string(),
        half_plus.to_string()
    );

    let events = query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    let rollup_row = events
        .rows
        .iter()
        .find(|r| r.rollup.is_some())
        .expect("下钻视图应包含该日汇总行");
    assert_eq!(rollup_row.input.to_string(), half_plus.to_string());
    assert_eq!(rollup_row.output.to_string(), half_plus.to_string());
    assert_eq!(rollup_row.cache_write.to_string(), half_plus.to_string());

    let _ = std::fs::remove_dir_all(&dir);
}
