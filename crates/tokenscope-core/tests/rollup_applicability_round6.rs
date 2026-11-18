//! PR #70 第六轮复核（2026-11-16）：日汇总费率适用性的三个实际核心库反例。
//!
//! 全部合成数据 + 临时定价表，调用真实核心库 `aggregate_with_rollups`，
//! 不依赖 gitignored 的独立探针目录，不触碰真实用户数据。
//!
//! 三个反例（见 qa-artifacts/pr70-r6-review/review-notes.md）：
//! 1. 部分重叠时段被当成整段同时生效 → 应保留未定价；
//! 2. 日总量 token 配比被当成单请求配比择价 → 应保留未定价；
//! 3. 跨时区 weekday 可发生但时间窗不可发生 → 应能证明 $1/M。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-r6-{tag}-{}-{n}-{}",
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

fn event(model: &str, id: &str, at: &str, input: u64, output: u64) -> UsageEvent {
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
        cache_read_tokens: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn daily(model: &str, input: u64, output: u64, requests: u64) -> RollupContribution {
    RollupContribution {
        day: "2026-11-16".into(),
        source_tz: "UTC".into(),
        app: AgentKind::ClaudeCode,
        model: model.into(),
        requests,
        tokens: TokenCounts {
            input,
            output,
            ..Default::default()
        },
        source_cost_usd: None,
        source: "synthetic-only".into(),
    }
}

// ---------------------------------------------------------------- 反例 1

const PARTIAL_OVERLAP_TOML: &str = r#"
[[model]]
prefix = "partial-overlap"
input = 1.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 3.0
[[model.schedule.period]]
start_time = "08:00"
end_time = "12:00"
input = 2.0
[[model.schedule]]
timezone = "UTC"
input = 1.0
[[model.schedule.period]]
start_time = "10:00"
end_time = "14:00"
input = 3.0
"#;

/// 反例 1：两条规则时段部分重叠。请求级在 09:00 取 $2、11:00 取 $3、
/// 15:00 取 $3——费率在日内变化。日汇总没有请求时刻，无法证明唯一费率，
/// input 必须真实未定价，不能宣称完整 $3。
#[test]
fn partial_overlap_worlds_keep_input_unpriced() {
    let dir = temp_dir("partial-overlap");
    let pricing = pricing_with(&dir, PARTIAL_OVERLAP_TOML);
    let tz = jiff::tz::TimeZone::UTC;
    let request_costs: Vec<f64> = [
        "2026-11-16T09:00:00Z",
        "2026-11-16T11:00:00Z",
        "2026-11-16T15:00:00Z",
    ]
    .iter()
    .map(|at| {
        aggregate_with_rollups(
            &[event("partial-overlap", at, at, 1_000_000, 0)],
            &[],
            GroupBy::Model,
            &tz,
            &pricing,
        )
        .unwrap()
        .totals
        .cost_usd
    })
    .collect();
    assert_eq!(
        request_costs,
        vec![2.0, 3.0, 3.0],
        "请求级费率本就随时段变化"
    );

    let r = aggregate_with_rollups(
        &[],
        &[daily("partial-overlap", 1_000_000, 0, 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(
        t.unknown_tokens.input, 1_000_000,
        "无法证明唯一费率 → input 真实未定价"
    );
    assert_eq!(t.total_cost_usd, None, "不存在唯一显示金额（不是完整 $3）");
    assert!(t.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 反例 2

const COMPOSITION_TOML: &str = r#"
[[model]]
prefix = "composition-competition"
input = 1.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 3.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 1.0
output = 3.0
"#;

/// 反例 2：两条交叉优势的始终生效规则。日总量相同（input=1M/output=1M、
/// 两次请求），但拆开请求与均分请求分别得 $6 和 $4——胜出规则依赖单请求
/// token 配比。日汇总不能从日总量推断配比，必须保留未定价，不能宣称 $4。
#[test]
fn daily_token_ratio_cannot_pick_request_winner() {
    let dir = temp_dir("composition");
    let pricing = pricing_with(&dir, COMPOSITION_TOML);
    let tz = jiff::tz::TimeZone::UTC;
    let split = [
        event(
            "composition-competition",
            "split-a",
            "2026-11-16T09:00:00Z",
            1_000_000,
            0,
        ),
        event(
            "composition-competition",
            "split-b",
            "2026-11-16T10:00:00Z",
            0,
            1_000_000,
        ),
    ];
    let balanced = [
        event(
            "composition-competition",
            "balanced-a",
            "2026-11-16T09:00:00Z",
            500_000,
            500_000,
        ),
        event(
            "composition-competition",
            "balanced-b",
            "2026-11-16T10:00:00Z",
            500_000,
            500_000,
        ),
    ];
    let split_t = aggregate_with_rollups(&split, &[], GroupBy::Model, &tz, &pricing)
        .unwrap()
        .totals;
    let balanced_t = aggregate_with_rollups(&balanced, &[], GroupBy::Model, &tz, &pricing)
        .unwrap()
        .totals;
    assert_eq!(
        split_t.tokens, balanced_t.tokens,
        "对照：两种构成日总量相同"
    );
    assert_eq!(split_t.requests, balanced_t.requests);
    assert!((split_t.cost_usd - 6.0).abs() < 1e-9, "拆开请求 = $6");
    assert!((balanced_t.cost_usd - 4.0).abs() < 1e-9, "均分请求 = $4");

    let r = aggregate_with_rollups(
        &[],
        &[daily("composition-competition", 1_000_000, 1_000_000, 2)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert!(
        t.unknown_pricing,
        "两种合法构成得 $4/$6 → 日汇总不能宣称完整 $4"
    );
    assert_eq!(t.total_cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 反例 3

const TIMEZONE_IMPOSSIBLE_TOML: &str = r#"
[[model]]
prefix = "timezone-impossible"
input = 1.0
output = 1.0
[[model.schedule]]
timezone = "Asia/Shanghai"
[[model.schedule.period]]
weekdays = ["mon"]
start_time = "00:00"
end_time = "01:00"
input = 0.1
"#;

/// 反例 3：来源日 UTC 周一 [00:00, 次日 00:00)，规则时区上海。来源日对应
/// 上海周一 08:00 至周二 08:00——周一 00:00–01:00 折扣窗口已过，周二凌晨
/// 不满足 weekday。整段来源日只能 $1/M，必须能证明并计价 $1。
#[test]
fn cross_timezone_window_outside_source_day_is_excluded() {
    let dir = temp_dir("timezone-impossible");
    let pricing = pricing_with(&dir, TIMEZONE_IMPOSSIBLE_TOML);
    let tz = jiff::tz::TimeZone::UTC;
    let request_costs: Vec<f64> = [
        "2026-11-16T00:00:00Z",
        "2026-11-16T12:00:00Z",
        "2026-11-16T23:59:00Z",
    ]
    .iter()
    .map(|at| {
        aggregate_with_rollups(
            &[event("timezone-impossible", at, at, 1_000_000, 0)],
            &[],
            GroupBy::Model,
            &tz,
            &pricing,
        )
        .unwrap()
        .totals
        .cost_usd
    })
    .collect();
    assert_eq!(request_costs, vec![1.0, 1.0, 1.0], "请求级三个时刻都是 $1");

    let r = aggregate_with_rollups(
        &[],
        &[daily("timezone-impossible", 1_000_000, 0, 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(
        t.total_cost_usd,
        Some(1.0),
        "折扣窗口不可发生 → 可证明 $1/M"
    );
    assert_eq!(t.unknown_tokens.input, 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 对照：重叠形态

/// 可控 day/source_tz 的汇总贡献。
fn daily_at(
    model: &str,
    day: &str,
    source_tz: &str,
    input: u64,
    output: u64,
) -> RollupContribution {
    RollupContribution {
        day: day.into(),
        source_tz: source_tz.into(),
        ..daily(model, input, output, 1)
    }
}

/// 对照·完全重叠：单条规则的两个 period 时段完全相同、费率与默认价也相同
/// （恒为 $3）→ 各时间片费率一致，可证明计价，不会因重叠制造虚假不确定性。
#[test]
fn fully_overlapping_equal_rates_are_priced() {
    let dir = temp_dir("full-overlap");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"fo\"\ninput = 3.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\ninput = 3.0\n\
         [[model.schedule.period]]\nstart_time = \"08:00\"\nend_time = \"12:00\"\ninput = 3.0\n\
         [[model.schedule.period]]\nstart_time = \"08:00\"\nend_time = \"12:00\"\ninput = 3.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let r = aggregate_with_rollups(
        &[],
        &[daily("fo", 1_000_000, 0, 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(t.total_cost_usd, Some(3.0), "各时段费率都 $3 → 可证明");
    assert_eq!(t.unknown_tokens.total(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 对照·互不重叠：同一规则的两个折扣时段完全错开且费率不同（默认 $5、
/// 08-12 为 $2、12-16 为 $4）→ 日内费率随时段变化，不能证明唯一费率 →
/// 未定价。单条规则避免"默认价互相竞争"掩盖时段差异。
#[test]
fn non_overlapping_different_rates_stay_unpriced() {
    let dir = temp_dir("non-overlap");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"no\"\ninput = 5.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\ninput = 5.0\n\
         [[model.schedule.period]]\nstart_time = \"08:00\"\nend_time = \"12:00\"\ninput = 2.0\n\
         [[model.schedule.period]]\nstart_time = \"12:00\"\nend_time = \"16:00\"\ninput = 4.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let r = aggregate_with_rollups(
        &[],
        &[daily("no", 1_000_000, 0, 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert!(t.unknown_pricing, "费率 2/4/5 随时段变化 → 未定价");
    assert_eq!(t.unknown_tokens.input, 1_000_000);
    assert_eq!(t.total_cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 对照：跨时区可发生

/// 对照·跨时区可发生：上海周一 00:00–01:00 = UTC 周日 16:00–17:00。来源日取
/// UTC 周日（2026-11-15），该折扣窗口确实落在来源日内；折扣价与默认价不同，
/// 无请求时刻 → 无法证明 → 未定价（不是误报为已定价，也不是过度清零）。
#[test]
fn cross_timezone_window_inside_source_day_stays_unpriced() {
    let dir = temp_dir("tz-inside");
    let pricing = pricing_with(&dir, TIMEZONE_IMPOSSIBLE_TOML);
    let tz = jiff::tz::TimeZone::UTC;
    // 请求级对照：UTC 周日 16:30（= 上海周一 00:30）命中折扣价 $0.1。
    let hit = aggregate_with_rollups(
        &[event(
            "timezone-impossible",
            "hit",
            "2026-11-15T16:30:00Z",
            1_000_000,
            0,
        )],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    assert!(
        (hit.totals.cost_usd - 0.1).abs() < 1e-9,
        "折扣窗口在来源日内确实可发生，实际 {}",
        hit.totals.cost_usd
    );
    let r = aggregate_with_rollups(
        &[],
        &[daily_at(
            "timezone-impossible",
            "2026-11-15",
            "UTC",
            1_000_000,
            0,
        )],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(
        t.unknown_tokens.input, 1_000_000,
        "时段价与默认价不同 → 未定价"
    );
    assert_eq!(t.total_cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 对照：优势规则

/// 对照·全局优势规则：input-only 下始终生效的 $3 与 $2 两条规则，$3/M 逐项
/// 支配 $2/M，胜出规则与配比无关 → 可证明 $3（上一轮修复继续通过）。
#[test]
fn globally_dominant_rule_selects_highest() {
    let dir = temp_dir("dominant");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"dom\"\ninput = 1.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\ninput = 3.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\ninput = 2.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let r = aggregate_with_rollups(
        &[],
        &[daily("dom", 1_000_000, 0, 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(t.total_cost_usd, Some(3.0));
    assert_eq!(t.unknown_tokens.total(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 对照：DST

/// 对照·DST：来源日取纽约 2026-03-08（春季前移，当日仅 23 小时）。切分按绝对
/// 时间进行，不假定自然日恒为 24 小时；始终生效的 $99 规则仍可证明计价。
#[test]
fn dst_source_day_does_not_break_always_active_rule() {
    let dir = temp_dir("dst");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"dst\"\ninput = 1.0\n\
         [[model.schedule]]\ntimezone = \"America/New_York\"\ninput = 99.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let r = aggregate_with_rollups(
        &[],
        &[daily_at(
            "dst",
            "2026-03-08",
            "America/New_York",
            1_000_000,
            0,
        )],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(t.total_cost_usd, Some(99.0));
    assert_eq!(t.unknown_tokens.total(), 0);
    std::fs::remove_dir_all(&dir).ok();
}
