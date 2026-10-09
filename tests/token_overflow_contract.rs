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
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;
use tokenscope::source::{CollectStats, FileParse, Source};

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-overflow-{tag}-{}", std::process::id()));
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

/// 两个**各自合法**的事件累计溢出 → aggregate 整体报错（Result），
/// 不产生部分结果；金额非有限同样报错。
#[test]
fn aggregate_overflow_returns_error_without_partial_result() {
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let mk = |input: u64| UsageEvent {
        ts: "2026-07-17T15:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "m".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: String::new(),
        input_tokens: input,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    };
    // 各自合法（四桶可表示），累计溢出。
    let events = vec![mk(u64::MAX - 10), mk(20)];
    let err = aggregate(&events, GroupBy::Day, &tz, &Pricing::default()).unwrap_err();
    assert!(
        err.to_string().contains("超出可表示范围"),
        "累计溢出必须可读报错: {err}"
    );

    // 正常事件聚合不受影响（错误只在真溢出时出现）。
    let ok = aggregate(&[mk(1), mk(2)], GroupBy::Day, &tz, &Pricing::default()).unwrap();
    assert_eq!(ok.totals.tokens.input, 3);
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
        ts: "2026-07-17T15:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "bad".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "bad-1".into(),
        input_tokens: i64_max,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    };
    let good = UsageEvent {
        ts: "2026-07-17T15:01:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "good".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "good-1".into(),
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
        input: u64::MAX,
        output: 0,
        cache_write: 0,
        cache_read: 0,
    };
    let b = TokenCounts {
        input: 1,
        output: 0,
        cache_write: 0,
        cache_read: 0,
    };
    assert!(a.checked_add(&b).is_none());
    assert!(b.checked_add(&a).is_none());
    assert!(TokenCounts::default().checked_add(&b).is_some());
}
