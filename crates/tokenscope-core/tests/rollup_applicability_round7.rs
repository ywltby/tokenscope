//! PR #70 第七轮复核（2026-11-16）：费用分项保留 + DST 回拨重复小时。
//!
//! 全部合成数据 + 临时定价表，调用真实核心库 `aggregate_with_rollups`，
//! 不依赖 gitignored 的独立探针目录，不触碰真实用户数据。
//!
//! 反例（见 qa-artifacts/pr70-r7-review/review-notes.md）：
//! A1. 单条部分定价规则（input 有价、output 缺价）→ 已知 input 费用不得丢弃；
//! A2. 交叉优势规则但 cache_read 费率一致 → 公共 cache_read 费用不得丢弃；
//! B.  DST 回拨第二次价格窗口被漏掉 → 不得误报完整 $1。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-r7-{tag}-{}-{n}-{}",
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

// ---------------------------------------------------------------- A1

const PARTIAL_ONE_RULE_TOML: &str = r#"
[[model]]
prefix = "partial-one-rule"
input = 1.0
[[model.schedule]]
timezone = "UTC"
input = 1.0
"#;

/// A1：单条部分定价规则（input=$1/M，output 无价），日汇总 1M+1M。
/// 输入费 $1 可独立证明，必须保留；只有 output 1M 未定价。
#[test]
fn single_partial_rule_keeps_known_input() {
    let dir = temp_dir("partial-one");
    let pricing = pricing_with(&dir, PARTIAL_ONE_RULE_TOML);
    let tz = jiff::tz::TimeZone::UTC;

    let req = aggregate_with_rollups(
        &[event(
            "partial-one-rule",
            "r1",
            "2026-11-16T12:00:00Z",
            1_000_000,
            1_000_000,
            0,
        )],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let rt = req.totals;
    assert!(
        (rt.cost_usd - 1.0).abs() < 1e-9,
        "请求路径 $1，实际 {}",
        rt.cost_usd
    );
    assert_eq!(rt.unknown_tokens.input, 0);
    assert_eq!(rt.unknown_tokens.output, 1_000_000);

    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "partial-one-rule",
            "2026-11-16",
            "UTC",
            1_000_000,
            1_000_000,
            0,
            1,
        )],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert_eq!(
        t.total_cost_usd,
        Some(1.0),
        "已知 input 费用不得丢弃，实际 {:?}",
        t.total_cost_usd
    );
    assert_eq!(t.unknown_tokens.input, 0);
    assert_eq!(t.unknown_tokens.output, 1_000_000, "仅 output 未定价");
    assert!(t.cost_partially_unknown, "保留部分缺失标记");
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- A2

const COMMON_PART_TOML: &str = r#"
[[model]]
prefix = "common-part"
input = 1.0
output = 1.0
cache_read = 1.0
[[model.schedule]]
timezone = "UTC"
input = 3.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 1.0
output = 3.0
"#;

/// A2：两条交叉优势规则（A input3/output1、B input1/output3），cache_read
/// 都是 $1/M。input/output 胜出规则无法由日总量确定，但 cache_read 恒为 $1，
/// 必须保留该分项费用；input/output 各 1M 保持未定价。
#[test]
fn crossing_rules_keep_identical_cache_read_rate() {
    let dir = temp_dir("common-part");
    let pricing = pricing_with(&dir, COMMON_PART_TOML);
    let tz = jiff::tz::TimeZone::UTC;

    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "common-part",
            "2026-11-16",
            "UTC",
            1_000_000,
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
    assert_eq!(
        t.total_cost_usd,
        Some(1.0),
        "公共 cache_read $1 不得丢弃，实际 {:?}",
        t.total_cost_usd
    );
    assert_eq!(t.unknown_tokens.cache_read, 0);
    assert_eq!(t.unknown_tokens.input, 1_000_000, "input 胜出规则不可证明");
    assert_eq!(
        t.unknown_tokens.output, 1_000_000,
        "output 胜出规则不可证明"
    );
    assert!(t.cost_partially_unknown);
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- B

const FOLD_WINDOW_TOML: &str = r#"
[[model]]
prefix = "fold-window"
input = 1.0
[[model.schedule]]
timezone = "America/New_York"
[[model.schedule.period]]
weekdays = ["sun"]
start_time = "01:30"
end_time = "01:45"
input = 10.0
"#;

/// B：来源日 UTC 2026-11-01（危地马拉，绝对区间 06:00Z~次日06:00Z），
/// 规则时区纽约，周日 01:30-01:45 折扣 $10。该本地窗在回拨日出现两次：
/// 第一次 05:30-05:45Z 在来源日之前；第二次 06:30-06:45Z 在来源日内。
/// 请求级 06:00Z→$1、06:35Z→$10、12:00Z→$1；日汇总不得误报完整 $1。
#[test]
fn second_fold_window_not_enumerated() {
    let dir = temp_dir("fold-window");
    let pricing = pricing_with(&dir, FOLD_WINDOW_TOML);
    let source_tz = jiff::tz::TimeZone::get("America/Guatemala").unwrap();

    let costs: Vec<f64> = [
        "2026-11-01T06:00:00Z",
        "2026-11-01T06:35:00Z",
        "2026-11-01T12:00:00Z",
    ]
    .iter()
    .map(|at| {
        aggregate_with_rollups(
            &[event("fold-window", at, at, 1_000_000, 0, 0)],
            &[],
            GroupBy::Model,
            &source_tz,
            &pricing,
        )
        .unwrap()
        .totals
        .cost_usd
    })
    .collect();
    assert_eq!(costs, vec![1.0, 10.0, 1.0], "请求级三次时刻费率应变化");

    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "fold-window",
            "2026-11-01",
            "America/Guatemala",
            1_000_000,
            0,
            0,
            1,
        )],
        GroupBy::Model,
        &source_tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    assert!(t.unknown_pricing, "请求时刻未知且费率真实变化 → 未定价");
    assert_eq!(t.unknown_tokens.input, 1_000_000);
    assert_eq!(
        t.total_cost_usd, None,
        "不得误报完整 $1，实际 {:?}",
        t.total_cost_usd
    );

    // 半开边界对照：第二次窗口 [06:30, 06:45)。起点含、终点不含，
    // 与请求级 matching_time_rules 的 tod >= start && tod < end 一致。
    let at_start = aggregate_with_rollups(
        &[event(
            "fold-window",
            "b0",
            "2026-11-01T06:30:00Z",
            1_000_000,
            0,
            0,
        )],
        &[],
        GroupBy::Model,
        &source_tz,
        &pricing,
    )
    .unwrap();
    let at_end = aggregate_with_rollups(
        &[event(
            "fold-window",
            "b1",
            "2026-11-01T06:45:00Z",
            1_000_000,
            0,
            0,
        )],
        &[],
        GroupBy::Model,
        &source_tz,
        &pricing,
    )
    .unwrap();
    assert!(
        (at_start.totals.cost_usd - 10.0).abs() < 1e-9,
        "06:30Z 含在折扣窗内"
    );
    assert!(
        (at_end.totals.cost_usd - 1.0).abs() < 1e-9,
        "06:45Z 是半开终点，不在窗内"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ---------------------------------------------------------------- 对照：前拨 gap

const SPRING_GAP_TOML: &str = r#"
[[model]]
prefix = "spring-gap"
input = 1.0
[[model.schedule]]
timezone = "America/New_York"
[[model.schedule.period]]
weekdays = ["sun"]
start_time = "02:30"
end_time = "03:30"
input = 10.0
"#;

/// 对照·前拨不存在时刻：纽约 2026-03-08 春季前拨 02:00→03:00（07:00Z），
/// period 02:30-03:30 的本地起点落在 gap 中。切分不得为不存在的时刻制造
/// 第二个虚假窗口、不 panic，且日汇总与请求级同口径（费率真实变化的时段
/// 保持未定价，而非误报完整 $1）。
#[test]
fn spring_forward_gap_does_not_fabricate_window() {
    let dir = temp_dir("spring-gap");
    let pricing = pricing_with(&dir, SPRING_GAP_TOML);
    let source_tz = jiff::tz::TimeZone::get("America/Guatemala").unwrap();

    let costs: Vec<f64> = [
        "2026-03-08T06:00:00Z",
        "2026-03-08T07:15:00Z",
        "2026-03-08T08:00:00Z",
        "2026-03-08T12:00:00Z",
    ]
    .iter()
    .map(|at| {
        aggregate_with_rollups(
            &[event("spring-gap", at, at, 1_000_000, 0, 0)],
            &[],
            GroupBy::Model,
            &source_tz,
            &pricing,
        )
        .unwrap()
        .totals
        .cost_usd
    })
    .collect();
    // 07:15Z = 03:15 EDT：起点 02:30 本身不存在，但前拨后 03:00-03:30 的本地
    // tod 仍落在 [02:30, 03:30) 内，请求级确实命中 $10 —— 窗口没有消失，只是
    // 变短了。08:00Z（04:00 EDT）与 12:00Z 已出窗。
    assert_eq!(
        costs,
        vec![1.0, 10.0, 1.0, 1.0],
        "请求级：前拨后仍有 03:00-03:30 落在窗口内"
    );

    let r = aggregate_with_rollups(
        &[],
        &[daily(
            "spring-gap",
            "2026-03-08",
            "America/Guatemala",
            1_000_000,
            0,
            0,
            1,
        )],
        GroupBy::Model,
        &source_tz,
        &pricing,
    )
    .unwrap();
    let t = r.totals;
    // 切分不得为不存在的 02:30 制造第二个伪窗口；但 03:00-03:30 是真实存在
    // 且确实命中的时段，因此当日费率确有变化，必须保持未定价而非误报 $1。
    assert!(
        t.unknown_pricing,
        "前拨日仍有真实 $10 时段，未定价才对，实际 {:?}",
        t.total_cost_usd
    );
    assert_eq!(t.unknown_tokens.input, 1_000_000);
    assert_eq!(
        t.total_cost_usd, None,
        "不得误报完整 $1，实际 {:?}",
        t.total_cost_usd
    );
    std::fs::remove_dir_all(&dir).ok();
}
