//! PR #70 第八轮复核的两处日汇总定价反例（基线 49bdc30）。
//!
//! R8-1. 春季前拨：时区偏移切换点本身不是切分边界 → 真实价格窗口被漏掉；
//! R8-2. 完整优先必须按「分项实际被使用的请求」判定 → 全局 Pareto 前沿
//!       会把不可能在有缓存读取的请求中胜出的规则算进可能胜出集合。
//!
//! 两个用例都先失败（金额/覆盖断言），修实现后转绿。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-r8-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pricing_with(dir: &Path, toml: &str) -> Pricing {
    let path = dir.join("pricing.toml");
    std::fs::write(&path, toml).unwrap();
    let (pricing, warnings) = Pricing::load(Some(&path), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    pricing
}

fn event(model: &str, id: &str, at: &str, input: u64, output: u64, read: u64) -> UsageEvent {
    UsageEvent {
        ts: at.parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: model.into(),
        session_id: "synthetic-review".into(),
        project: "synthetic-review".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: id.into(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 1,
        source_path: "synthetic-only.jsonl".into(),
        input_tokens: input,
        output_tokens: output,
        cache_write_tokens: 0,
        cache_read_tokens: read,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn daily(
    model: &str,
    day: &str,
    source_tz: &str,
    input: u64,
    output: u64,
    read: u64,
    requests: u64,
) -> RollupContribution {
    RollupContribution {
        day: day.into(),
        source_tz: source_tz.into(),
        app: AgentKind::ClaudeCode,
        model: model.into(),
        requests,
        tokens: TokenCounts {
            input,
            output,
            cache_read: read,
            ..Default::default()
        },
        source_cost_usd: None,
        source: "synthetic-only".into(),
    }
}

fn request_cost(
    pricing: &Pricing,
    model: &str,
    tz: &jiff::tz::TimeZone,
    at: &str,
    input: u64,
    output: u64,
    read: u64,
) -> f64 {
    aggregate_with_rollups(
        &[event(model, at, at, input, output, read)],
        &[],
        GroupBy::Model,
        tz,
        pricing,
    )
    .unwrap()
    .totals
    .cost_usd
}

// ---------------------------------------------------------------- R8-1

/// 规则时区与来源日同为纽约，周日 01:45-02:30 折扣 $10/M；2026-03-08 前拨日
/// 02:00-03:00 的本地时刻不存在。窗口起点 01:45 真实存在（EST），终点 02:30
/// 不存在，只能 compatible 映射到 03:30（EDT）。
///
/// 请求级事实：06:30Z=01:30 EST→$1；06:50Z=01:50 EST→$10；07:10Z=03:10 EDT→$1。
/// 日汇总只有 1M input、时刻未知：费率确有变化 → 必须未定价。
const SPRING_GAP_TOML: &str = r#"
[[model]]
prefix = "spring-gap"
input = 1.0
[[model.schedule]]
timezone = "America/New_York"
[[model.schedule.period]]
weekdays = ["sun"]
start_time = "01:45"
end_time = "02:30"
input = 10.0
"#;

#[test]
fn spring_forward_transition_splits_real_price_window() {
    let dir = temp_dir("spring-gap");
    let pricing = pricing_with(&dir, SPRING_GAP_TOML);
    let tz = jiff::tz::TimeZone::get("America/New_York").unwrap();

    let costs = vec![
        request_cost(
            &pricing,
            "spring-gap",
            &tz,
            "2026-03-08T06:30:00Z",
            1_000_000,
            0,
            0,
        ),
        request_cost(
            &pricing,
            "spring-gap",
            &tz,
            "2026-03-08T06:50:00Z",
            1_000_000,
            0,
            0,
        ),
        request_cost(
            &pricing,
            "spring-gap",
            &tz,
            "2026-03-08T07:10:00Z",
            1_000_000,
            0,
            0,
        ),
    ];
    assert_eq!(
        costs,
        vec![1.0, 10.0, 1.0],
        "请求级三次时刻费率必须变化（前拨前后各一个价）"
    );

    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "spring-gap",
            "2026-03-08",
            "America/New_York",
            1_000_000,
            0,
            0,
            1,
        )],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert!(
        t.unknown_pricing,
        "前拨日真实价格窗口未被切分覆盖时必须报未定价，实际 {:?}",
        t.total_cost_usd
    );
    assert_eq!(t.unknown_tokens.input, 1_000_000, "整日输入费不可证明");
    assert_eq!(
        t.total_cost_usd, None,
        "不得误报完整 $1，实际 {:?}",
        t.total_cost_usd
    );

    // 半开边界对照：窗口 [01:45, 02:30)。01:44 EST 与 01:45 EST 分属两侧。
    assert_eq!(
        request_cost(
            &pricing,
            "spring-gap",
            &tz,
            "2026-03-08T06:44:00Z",
            1_000_000,
            0,
            0
        ),
        1.0,
        "01:44 不在窗口内"
    );
    assert_eq!(
        request_cost(
            &pricing,
            "spring-gap",
            &tz,
            "2026-03-08T06:45:00Z",
            1_000_000,
            0,
            0
        ),
        10.0,
        "01:45 命中窗口起点"
    );
    // 前拨之后：03:10 已过 02:30，回到 $1。
    assert_eq!(
        request_cost(
            &pricing,
            "spring-gap",
            &tz,
            "2026-03-08T07:10:00Z",
            1_000_000,
            0,
            0
        ),
        1.0,
        "03:10 不在窗口内"
    );

    // 对照：完全落在 gap 里的窗口（02:10-02:20 本地不存在）不得虚构价格。
    let gap_only = temp_dir("spring-gap-only");
    let gap_pricing = pricing_with(
        &gap_only,
        r#"
[[model]]
prefix = "spring-gap"
input = 1.0
[[model.schedule]]
timezone = "America/New_York"
[[model.schedule.period]]
weekdays = ["sun"]
start_time = "02:10"
end_time = "02:20"
input = 10.0
"#,
    );
    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "spring-gap",
            "2026-03-08",
            "America/New_York",
            1_000_000,
            0,
            0,
            1,
        )],
        GroupBy::Model,
        &tz,
        &gap_pricing,
    )
    .unwrap();
    assert_eq!(
        r.totals.total_cost_usd,
        Some(1.0),
        "窗口整段落在不存在的本地时刻里，当日只能按 $1 计价"
    );
    assert!(!r.totals.unknown_pricing);
}

// ---------------------------------------------------------------- R8-2

/// 两条常驻规则：A output=$1/M、cache_read=$1/M；B output=$3/M、cache_read 缺价。
///
/// 请求级事实：任何含缓存读取的请求里 B 都不完整，A 完整胜出 → 缓存读取
/// 必然 $1/M；输出在有/无缓存读取的请求中分别归 B/A，不可证明。
const COMPLETENESS_TOML: &str = r#"
[[model]]
prefix = "read-winner"
input = 1.0

[[model.schedule]]
timezone = "UTC"
output = 1.0
cache_read = 1.0

[[model.schedule]]
timezone = "UTC"
output = 3.0
"#;

#[test]
fn incomplete_rule_cannot_win_requests_that_carry_cache_read() {
    let dir = temp_dir("read-winner");
    let pricing = pricing_with(&dir, COMPLETENESS_TOML);
    let tz = jiff::tz::TimeZone::UTC;

    // 请求级对照：合并请求由完整规则 A 计价 $2；只读请求 $1；只输出请求 $3。
    assert_eq!(
        request_cost(
            &pricing,
            "read-winner",
            &tz,
            "2026-03-08T00:00:00Z",
            0,
            1_000_000,
            1_000_000
        ),
        2.0,
        "output+read 由完整的 A 胜出"
    );
    assert_eq!(
        request_cost(
            &pricing,
            "read-winner",
            &tz,
            "2026-03-08T01:00:00Z",
            0,
            0,
            1_000_000
        ),
        1.0,
        "只含缓冲读取时 A 完整"
    );
    assert_eq!(
        request_cost(
            &pricing,
            "read-winner",
            &tz,
            "2026-03-08T02:00:00Z",
            0,
            1_000_000,
            0
        ),
        3.0,
        "只含输出时 B 更贵且同样完整"
    );

    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "read-winner",
            "2026-03-08",
            "UTC",
            0,
            1_000_000,
            1_000_000,
            2,
        )],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(t.unknown_tokens.cache_read, 0, "缓存读取必须可证明为 $1/M");
    assert_eq!(
        t.unknown_tokens.output, 1_000_000,
        "输出仍取决于每次请求构成，必须保持未定价"
    );
    assert_eq!(
        t.total_cost_usd,
        Some(1.0),
        "只保留可证明的缓存读取小计 $1，实际 {:?}",
        t.total_cost_usd
    );
    assert!(
        t.cost_partially_unknown,
        "仍有未知分项，必须保留部分缺失标记"
    );
    assert!(t.unknown_pricing, "存在未定价分项");
}
