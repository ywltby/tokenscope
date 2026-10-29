//! N18：Claude 缓存写 5 分钟 / 1 小时分档计价契约。
//!
//! 口径（用户验收 + docs/research/2026-10-27-usage-semantics-cross-tool-audit.md §4.5）：
//! - 输入 $3/百万、5 分钟写 $3.75、1 小时写 $6 时，两档各 100 万 token
//!   的写入费用合计 **$9.75**（旧实现共用 $3.75 → $7.5，1h 写少计 37.5%）；
//! - 分项是写总量的内部构成：汇总总量不变，逐请求明细与汇总费用一致；
//! - 分项未知（旧日志/源日志已删）且候选声明了分档 → 费用按总量单价
//!   **估算**并标示（complete=false），不得显示为完整精确费用；
//! - Anthropic 官方派生仅适用于 provider = anthropic 且发布价自证
//!   （cache_write == 1.25 × input）；其他渠道一律不派生。

use tokenscope::model::{TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;

fn write_toml(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("pricing.toml");
    std::fs::write(&p, body).unwrap();
    p
}

fn snapshot_json(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("pricing-modelsdev.json");
    std::fs::write(&p, body).unwrap();
    p
}

fn split_counts(cw: u64, w5: u64, w1: u64) -> TokenCounts {
    TokenCounts {
        input: 0,
        output: 0,
        cache_write: cw,
        cache_read: 0,
        cache_write_5m: w5,
        cache_write_1h: w1,
        cache_write_split_known: true,
    }
}

fn unknown_split_counts(cw: u64) -> TokenCounts {
    TokenCounts {
        input: 0,
        output: 0,
        cache_write: cw,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

const TIERED_TOML: &str = r#"
[[model]]
prefix = "claude-sonnet-4-5"
input = 3.0
output = 15.0
cache_read = 0.3
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model]]
prefix = "acme/one-price-model"
input = 2.0
output = 10.0
cache_write = 2.5
"#;

fn at() -> jiff::Timestamp {
    "2026-07-17T08:00:00Z".parse().unwrap()
}

#[test]
fn n18_split_write_costs_975_per_million_and_totals_stay_flat() {
    let dir = std::env::temp_dir().join(format!("n18-975-{}", std::process::id()));
    let toml = write_toml(&dir, TIERED_TOML);
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");

    let est = p
        .estimate(
            "claude-sonnet-4-5",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert!(
        (est.cost - 9.75).abs() < 1e-9,
        "两档各 1M token 写费合计 $9.75，实际 {}",
        est.cost
    );
    assert!(est.complete);
    assert!(!est.cache_write_split_estimated);
    use tokenscope::pricing::CostLineKind;
    let line = |k| est.lines.iter().find(|l| l.kind == k).unwrap();
    let w5 = line(CostLineKind::CacheWrite5m);
    let w1 = line(CostLineKind::CacheWrite1h);
    assert_eq!(w5.tokens, 1_000_000);
    assert!((w5.unit_price.unwrap() - 3.75).abs() < 1e-9);
    assert!((w5.subtotal - 3.75).abs() < 1e-9);
    assert_eq!(w1.tokens, 1_000_000);
    assert!((w1.unit_price.unwrap() - 6.0).abs() < 1e-9);
    assert!((w1.subtotal - 6.0).abs() < 1e-9);
    assert!(
        !est.lines.iter().any(|l| l.kind == CostLineKind::CacheWrite),
        "可信分档事件不再输出合并写行"
    );
    // 汇总总量口径：分项不得重复计入 total / prompt 基数。
    let counts = split_counts(2_000_000, 1_000_000, 1_000_000);
    assert_eq!(counts.total(), 2_000_000);
    assert_eq!(counts.prompt_tokens(), 2_000_000);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_unknown_split_with_tiered_candidate_is_estimated_not_complete() {
    let dir = std::env::temp_dir().join(format!("n18-est-{}", std::process::id()));
    let toml = write_toml(&dir, TIERED_TOML);
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("claude-sonnet-4-5", &unknown_split_counts(2_000_000), at())
        .unwrap();
    assert!(
        (est.cost - 7.5).abs() < 1e-9,
        "分项未知按总量单价估算（2M × $3.75），实际 {}",
        est.cost
    );
    assert!(!est.complete, "估算不得显示为完整精确费用");
    assert!(est.cache_write_split_estimated);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_split_known_without_tier_prices_is_unknown_not_guessed() {
    let dir = std::env::temp_dir().join(format!("n18-notier-{}", std::process::id()));
    let toml = write_toml(&dir, TIERED_TOML);
    let (p, _) = Pricing::load(Some(&toml), None, None);
    // acme 模型只有单写价：可信分档事件无法分档计价 → 整体未知（不猜档）。
    let est = p
        .estimate(
            "acme/one-price-model",
            &split_counts(1_000_000, 400_000, 600_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete, "缺分档价不得用总单价冒充");
    assert!(!est.cache_write_split_estimated, "这不是估算而是未知");
    assert_eq!(est.unknown.cache_write, 1_000_000, "写 token 记为未计价");
    assert_eq!(est.cost, 0.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_anthropic_modelsdev_official_tiers_are_derived_with_proof() {
    let dir = std::env::temp_dir().join(format!("n18-md-{}", std::process::id()));
    let md = snapshot_json(
        &dir,
        r#"{"v":4,"synced_at":"t","entries":[
            {"id":"anthropic/claude-sonnet-4-5","name":"Claude Sonnet 4.5",
             "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75},
            {"id":"bedrock/sonnet-x","name":"Bedrock Sonnet X",
             "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75},
            {"id":"acme/sol","name":"Sol","input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75},
            {"id":"anthropic/offratio","name":"Odd","input":3.0,"output":15.0,"cache_read":0.3,"cache_write":4.0}
        ]}"#,
    );
    let (p, warnings) = Pricing::load(None, Some(&md), None);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");

    // 官方渠道：3.75 == 1.25 × 3 自证 → 派生 1h = 2 × 3 = 6。
    let est = p
        .estimate(
            "anthropic/claude-sonnet-4-5",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert!((est.cost - 9.75).abs() < 1e-9, "实际 {}", est.cost);
    assert!(est.complete);

    // Bedrock 渠道（同样数字）不派生：可信分档 → 写费未知。
    let est = p
        .estimate(
            "bedrock/sonnet-x",
            &split_counts(1_000_000, 400_000, 600_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete, "非官方渠道不得套官方倍率");

    // 同为官方价数字的 acme 渠道不派生。
    let est = p
        .estimate("acme/sol", &split_counts(1_000_000, 400_000, 600_000), at())
        .unwrap();
    assert!(!est.complete);

    // 比例不自证的 anthropic 条目不派生（宁可缺价）。
    let est = p
        .estimate(
            "anthropic/offratio",
            &split_counts(1_000_000, 400_000, 600_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete);

    // 分档未知 + anthropic 官方候选（有派生分档）→ 估算标示。
    let est = p
        .estimate(
            "anthropic/claude-sonnet-4-5",
            &unknown_split_counts(2_000_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete && est.cache_write_split_estimated);
    assert!((est.cost - 7.5).abs() < 1e-9);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_split_events_keep_segments_and_time_rules_for_other_components() {
    let dir = std::env::temp_dir().join(format!("n18-seg-{}", std::process::id()));
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "tiered"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.segment]]
label = ">200K"
min_tokens = 200_000
input = 6.0
output = 30.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    // prompt = 1M 写 + 200K 读 → 命中 >200K 分段；输入价随分段，
    // 写分档为平价（不受分段影响）。
    let counts = TokenCounts {
        input: 0,
        output: 0,
        cache_write: 1_000_000,
        cache_read: 200_000,
        cache_write_5m: 1_000_000,
        cache_write_1h: 0,
        cache_write_split_known: true,
    };
    let est = p.estimate("tiered", &counts, at()).unwrap();
    use tokenscope::pricing::CostLineKind;
    let line = |k| est.lines.iter().find(|l| l.kind == k).unwrap();
    assert!(
        line(CostLineKind::CacheRead).unit_price.is_none(),
        "cache_read 未声明 → Unknown（缺价 ≠ 免费）"
    );
    let w1 = line(CostLineKind::CacheWrite5m);
    assert!((w1.unit_price.unwrap() - 3.75).abs() < 1e-9, "分档价平价");
    assert!(
        est.segment_label.as_deref() == Some(">200K"),
        "分段照常命中"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_per_request_costs_sum_to_tiered_total() {
    // 逐请求明细与汇总一致性：两笔请求（各 1M 单档）费用之和 =
    // 单笔两档混合请求的费用（$3.75 + $6.0 = $9.75）。
    let dir = std::env::temp_dir().join(format!("n18-sum-{}", std::process::id()));
    let toml = write_toml(&dir, TIERED_TOML);
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let counts_a = split_counts(1_000_000, 1_000_000, 0);
    let counts_b = split_counts(1_000_000, 0, 1_000_000);
    let counts_both = split_counts(2_000_000, 1_000_000, 1_000_000);
    let a = p.estimate("claude-sonnet-4-5", &counts_a, at()).unwrap();
    let b = p.estimate("claude-sonnet-4-5", &counts_b, at()).unwrap();
    let both = p.estimate("claude-sonnet-4-5", &counts_both, at()).unwrap();
    assert!((a.cost + b.cost - both.cost).abs() < 1e-9);
    assert_eq!(counts_a.total() + counts_b.total(), counts_both.total());
    assert_eq!(
        counts_both.cache_write_5m + counts_both.cache_write_1h,
        counts_both.cache_write
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_event_from_claude_jsonl_flows_through_estimate() {
    // 端到端（解析 → 事件 → 计价）：真实 JSONL 行的 5m/1h 分项直接参与
    // 分档计价，四桶总量不变。
    let line = r#"{"type":"assistant","timestamp":"2026-07-17T08:00:01.000Z","sessionId":"s1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":2000000,"cache_read_input_tokens":0,"cache_creation":{"ephemeral_5m_input_tokens":1000000,"ephemeral_1h_input_tokens":1000000}}}}"#;
    let mut stats = tokenscope::source::CollectStats::default();
    let mut events = Vec::new();
    tokenscope::source::claude::claude_ingest_for_test(
        line,
        "t.jsonl",
        "proj",
        &mut stats,
        &mut events,
    );
    assert_eq!(events.len(), 1);
    let e = &events[0];
    assert_eq!(e.cache_write_tokens, 2_000_000);
    let counts = TokenCounts::from_event(e);
    assert!(counts.cache_write_split_known);
    let dir = std::env::temp_dir().join(format!("n18-e2e-{}", std::process::id()));
    let toml = write_toml(&dir, TIERED_TOML);
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p.estimate(&e.model, &counts, e.ts).unwrap();
    assert!((est.cost - 9.75).abs() < 1e-9);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_usage_event_construction_keeps_buckets_flat() {
    let e = UsageEvent {
        ts: at(),
        agent: tokenscope::model::AgentKind::ClaudeCode,
        model: "m".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "r".into(),
        line: 0,
        source_path: String::new(),
        input_tokens: 1,
        output_tokens: 2,
        cache_write_tokens: 9,
        cache_read_tokens: 3,
        cache_write_5m: 4,
        cache_write_1h: 5,
        cache_write_split_known: true,
    };
    assert_eq!(e.total_tokens(), 15, "分项不得加进总量");
    assert!(e.validate_buckets().is_ok());
}

// ── N18 第二轮：分档价参与分段 / 峰谷规则 ────────────────────────────
// 有依据的层声明分档才生效；缺层不得跨层拼齐；无依据一律未知。

fn tiered_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("n18-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn n18_segment_level_tiers_override_base_tiers() {
    let dir = tiered_dir("seg-tier");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.segment]]
label = ">200K"
min_tokens = 200_000
input = 6.0
cache_write_5m = 7.5
cache_write_1h = 12.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    // prompt = 100K < 200K → 基础层分档价。
    let lo = p
        .estimate("m", &split_counts(100_000, 40_000, 60_000), at())
        .unwrap();
    assert!(
        (lo.cost - (0.04 * 3.75 + 0.06 * 6.0)).abs() < 1e-9,
        "实际 {}",
        lo.cost
    );
    assert!(lo.complete);
    assert_eq!(lo.segment_label, None);
    // 命中 >200K 段（prompt = 2M 写 ≥ 200K）→ 段级分档价覆盖基础层。
    let hi = p
        .estimate("m", &split_counts(2_000_000, 800_000, 1_200_000), at())
        .unwrap();
    assert!(
        (hi.cost - (0.8 * 7.5 + 1.2 * 12.0)).abs() < 1e-9,
        "段级分档价应覆盖基础层，实际 {}",
        hi.cost
    );
    assert!(hi.complete);
    assert_eq!(hi.segment_label.as_deref(), Some(">200K"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_segment_without_tiers_falls_back_to_base_tiers() {
    let dir = tiered_dir("seg-fallback");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.segment]]
label = ">200K"
min_tokens = 200_000
input = 6.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let hi = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    // 段**未改写写入价**（沿用基础 3.75）→ 回退基础层分档价是合理的。
    assert!(
        (hi.cost - (1.0 * 3.75 + 1.0 * 6.0)).abs() < 1e-9,
        "段未改写写入价时应回退基础层分档价，实际 {}",
        hi.cost
    );
    assert!(hi.complete);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_segment_overriding_write_without_tiers_is_unknown() {
    // 反例（复核项 3）：段显式改写写入价（9.0）却没有段级分档依据时，
    // 不得借用基础层 3.75/6.0 的比率并声称完整——写入分档未知。
    let dir = tiered_dir("seg-write-no-tiers");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.segment]]
label = ">200K"
min_tokens = 200_000
input = 6.0
cache_write = 9.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let hi = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!(!hi.complete, "段改写写入价但无分档依据 → 不得完整");
    assert_eq!(hi.unknown.cache_write, 2_000_000);
    assert!(!hi.cache_write_split_estimated, "这是未知不是估算");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_segment_partial_tier_is_unknown_not_mixed_across_layers() {
    let dir = tiered_dir("seg-partial");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.segment]]
label = ">200K"
min_tokens = 200_000
input = 6.0
cache_write_5m = 7.5
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let hi = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    // 段声明了分档但缺 1h 档：该层已声明 → 不跨层拼基础层 1h 价，
    // 1h token 进 unknown（不完整），不得用总量单价冒充。
    assert!(!hi.complete, "段级分档缺档不得拼层凑齐");
    assert_eq!(hi.unknown.cache_write, 1_000_000, "1h token 记为未计价");
    use tokenscope::pricing::CostLineKind;
    let w5 = hi
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite5m)
        .unwrap();
    assert!((w5.unit_price.unwrap() - 7.5).abs() < 1e-9);
    assert!(!hi.cache_write_split_estimated, "这是未知不是估算");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_time_rule_tiers_override_base_and_segment() {
    let dir = tiered_dir("time-tier");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule]]
label = "peak"
timezone = "UTC"
input = 6.0
cache_write_5m = 9.0
cache_write_1h = 15.0

[[model.schedule.period]]
start_time = "00:00"
end_time = "23:59"
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    // at() = 08:00Z 命中规则 → 规则级分档价优先于基础层。
    let est = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!(
        (est.cost - (1.0 * 9.0 + 1.0 * 15.0)).abs() < 1e-9,
        "规则级分档价应覆盖基础层，实际 {}",
        est.cost
    );
    assert!(est.complete);
    assert!(est.matched.as_ref().unwrap().schedule_label.is_some());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_no_tiers_anywhere_keeps_unknown_split_and_marks_estimate() {
    let dir = tiered_dir("no-tier");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    // 可信分档但候选无分档 → 写费未知（不猜档）。
    let known = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!(!known.complete);
    assert_eq!(known.unknown.cache_write, 2_000_000);
    assert_eq!(known.cost, 0.0);
    // 分项未知 + 候选无分档 → 单价的既有完整语义（无估算标示）。
    let unknown = p
        .estimate("m", &unknown_split_counts(2_000_000), at())
        .unwrap();
    assert!(unknown.complete, "无分档候选不应标估算");
    assert!(!unknown.cache_write_split_estimated);
    assert!((unknown.cost - 2.0 * 3.75).abs() < 1e-9);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_tiered_costs_sum_to_summary_across_mixed_requests() {
    // 混合档 + 逐请求/汇总一致性：可信分档两笔 + 分项未知一笔，
    // 聚合总价 == 逐请求求和；汇总写总量不含分项重复。
    let dir = tiered_dir("mixed-sum");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let a = p
        .estimate("m", &split_counts(1_000_000, 1_000_000, 0), at())
        .unwrap();
    let b = p
        .estimate("m", &split_counts(1_000_000, 0, 1_000_000), at())
        .unwrap();
    let c = p
        .estimate("m", &unknown_split_counts(1_000_000), at())
        .unwrap();
    let both = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    // 估算请求单独计价（complete=false），与完整分档请求同池求和。
    let row_sum = a.cost + b.cost + c.cost;
    let agg = both.cost + c.cost;
    assert!(
        (row_sum - agg).abs() < 1e-9,
        "逐请求 {} vs 聚合 {}",
        row_sum,
        agg
    );
    assert!(c.cache_write_split_estimated && !c.complete);
    assert!(both.complete);
    std::fs::remove_dir_all(&dir).ok();
}

// ── N18 三轮（复核项 3/4/5/6/8）──────────────────────────────────────

fn md_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("n18-md-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn n18_anthropic_high_segment_self_proved_tiers_cost_1950() {
    // 项 3：官方渠道高档 input=6、cache_write=7.5（= 1.25 × 6）→ 该段
    // 自证派生 1h = 12；两档各 1M token 写入费 = 7.5 + 12 = **$19.5**。
    let dir = md_dir("seg-self-proof");
    let md = snapshot_json(
        &dir,
        r#"{"v":4,"synced_at":"t","entries":[
            {"id":"anthropic/claude-x","name":"X",
             "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75,
             "segments":[
               {"label":">272K","min_tokens":272001,
                "input":6.0,"output":30.0,"cache_read":0.6,"cache_write":7.5}
             ]}
        ]}"#,
    );
    let (p, warnings) = Pricing::load(None, Some(&md), None);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
    // prompt = 2M 写 ≥ 272001 → 命中高档段。
    let est = p
        .estimate(
            "anthropic/claude-x",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert!(
        (est.cost - 19.5).abs() < 1e-9,
        "高档自证派生应为 $19.5，实际 {}",
        est.cost
    );
    assert!(est.complete);
    assert_eq!(est.segment_label.as_deref(), Some(">272K"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_anthropic_segment_without_proof_is_unknown_not_guessed() {
    // 项 3：高档 cache_write 与 1.25 × input 不一致 → 依据不足，标未知。
    let dir = md_dir("seg-no-proof");
    let md = snapshot_json(
        &dir,
        r#"{"v":4,"synced_at":"t","entries":[
            {"id":"anthropic/claude-x","name":"X",
             "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75,
             "segments":[
               {"label":">272K","min_tokens":272001,
                "input":6.0,"output":30.0,"cache_read":0.6,"cache_write":9.0}
             ]}
        ]}"#,
    );
    let (p, _) = Pricing::load(None, Some(&md), None);
    let est = p
        .estimate(
            "anthropic/claude-x",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    // 段显式改写写入价（9.0）且自证不通过 → 分档价未知：不得回退基础层
    // 3.75/6.0 的比率并声称完整（复核项 3 的原反例）。
    assert!(!est.complete, "高档无分档依据 → 不得声称完整");
    assert_eq!(est.unknown.cache_write, 2_000_000, "写 token 记未计价");
    assert!(!est.cache_write_split_estimated, "这是未知不是估算");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_anthropic_segment_without_any_tier_evidence_is_unknown() {
    // 段与基础层都无分档依据（基础 cache_write 也不自证）→ 未知。
    let dir = md_dir("seg-no-evidence");
    let md = snapshot_json(
        &dir,
        r#"{"v":4,"synced_at":"t","entries":[
            {"id":"anthropic/claude-x","name":"X",
             "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":4.0,
             "segments":[
               {"label":">272K","min_tokens":272001,
                "input":6.0,"output":30.0,"cache_read":0.6,"cache_write":9.0}
             ]}
        ]}"#,
    );
    let (p, _) = Pricing::load(None, Some(&md), None);
    let est = p
        .estimate(
            "anthropic/claude-x",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete, "无任何自证依据 → 分档价未知");
    assert_eq!(est.unknown.cache_write, 2_000_000);
    assert!(!est.cache_write_split_estimated, "这是未知不是估算");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_non_anthropic_segment_never_derives_anthropic_multiplier() {
    // 项 3：其他渠道即使数字相同也不套 Anthropic 倍率。
    let dir = md_dir("seg-other-channel");
    let md = snapshot_json(
        &dir,
        r#"{"v":4,"synced_at":"t","entries":[
            {"id":"bedrock/claude-x","name":"X",
             "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75,
             "segments":[
               {"label":">272K","min_tokens":272001,
                "input":6.0,"output":30.0,"cache_read":0.6,"cache_write":7.5}
             ]}
        ]}"#,
    );
    let (p, _) = Pricing::load(None, Some(&md), None);
    let est = p
        .estimate(
            "bedrock/claude-x",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete, "非官方渠道不得派生");
    assert_eq!(est.unknown.cache_write, 2_000_000);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 4：各层分档价统一校验 ─────────────────────────────────────────

#[test]
fn n18_invalid_tier_prices_are_rejected_with_diagnostics() {
    for (tag, body) in [
        ("negative", "cache_write_5m = -1.0\ncache_write_1h = 6.0\n"),
        ("non-finite", "cache_write_5m = nan\ncache_write_1h = 6.0\n"),
        (
            "same-as-input",
            "cache_write_5m = \"same_as_input\"\ncache_write_1h = 6.0\n",
        ),
        (
            "segment-negative",
            "cache_write_5m = 3.75\ncache_write_1h = 6.0\n\n[[model.segment]]\nlabel = \">200K\"\nmin_tokens = 200000\ncache_write_5m = -1.0\n",
        ),
        (
            "rule-negative",
            "cache_write_5m = 3.75\ncache_write_1h = 6.0\n\n[[model.schedule]]\nlabel = \"peak\"\ncache_write_5m = -1.0\n",
        ),
        (
            "period-negative",
            "cache_write_5m = 3.75\ncache_write_1h = 6.0\n\n[[model.schedule]]\nlabel = \"peak\"\n\n[[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"23:59\"\ncache_write_5m = -1.0\n",
        ),
        (
            "rule-segment-negative",
            "cache_write_5m = 3.75\ncache_write_1h = 6.0\n\n[[model.schedule]]\nlabel = \"peak\"\n\n[[model.schedule.segment]]\nmin_tokens = 200000\ncache_write_1h = -1.0\n",
        ),
    ] {
        let dir = md_dir(&format!("invalid-{tag}"));
        let toml = write_toml(
            &dir,
            &format!(
                "[[model]]\nprefix = \"m\"\ninput = 3.0\noutput = 15.0\ncache_write = 3.75\n{body}"
            ),
        );
        let (p, warnings) = Pricing::load(Some(&toml), None, None);
        let est = p.estimate("m", &split_counts(1_000_000, 400_000, 600_000), at());
        assert!(
            est.is_none(),
            "{tag}：非法分档价不得进入计价（候选应被拒绝）"
        );
        assert!(
            warnings.iter().any(|w| w.contains("分档")),
            "{tag}：必须保留诊断，实际 {warnings:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

// ── 项 5：多候选 + 分档未知的不确定性披露 ────────────────────────────

#[test]
fn n18_multi_candidate_unknown_split_discloses_exclusion() {
    // 两条同末段候选：A 有完整分档、B 只有合并写入价（分档未知）。
    // 完整候选优先（既有规则），但 B 被排除的事实必须披露。
    let dir = md_dir("multi-candidate");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "chan-a/tiered"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model]]
prefix = "chan-b/tiered"
input = 3.0
output = 15.0
cache_write = 9.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate(
            "chan-a/tiered",
            &split_counts(1_000_000, 400_000, 600_000),
            at(),
        )
        .unwrap();
    // 可信分档 + A 完整分档 → 完整且费用来自 A。
    assert!(est.complete);
    assert!((est.cost - (0.4 * 3.75 + 0.6 * 6.0)).abs() < 1e-9);
    let matched = est.matched.as_ref().unwrap();
    assert_eq!(matched.raw_key, "chan-a/tiered");
    assert_eq!(
        matched.reason, "highest_complete_cost",
        "完整候选优先规则不变"
    );
    assert!(matched.candidate_count >= 2, "两条同末段候选");
    // B 缺分档 → 不完整候选被排除，必须披露数量。
    assert!(
        matched.incomplete_candidates_excluded >= 1,
        "被排除的不完整候选必须披露：{:?}",
        matched
    );
    assert!(
        est.excluded_candidate_warning.is_some(),
        "排除原因必须结构化披露"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 6：估算与未计价的区分必须贯穿汇总/明细 ────────────────────────

#[test]
fn n18_estimated_and_unpriced_are_distinguishable_in_breakdown() {
    let dir = md_dir("est-vs-unpriced");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "tiered"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model]]
prefix = "no-tier"
input = 3.0
output = 15.0
cache_write = 3.75
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);

    // (a) 分项未知 + 有分档 → 按合并价**估算**（complete=false、标示置位、
    //    金额 = 总量 × 合并价、unknown 不含写 token）。
    let estimated = p
        .estimate("tiered", &unknown_split_counts(2_000_000), at())
        .unwrap();
    assert!((estimated.cost - 7.5).abs() < 1e-9, "估算金额按合并价");
    assert!(!estimated.complete);
    assert!(estimated.cache_write_split_estimated, "估算必须标示");
    assert_eq!(estimated.unknown.cache_write, 0, "估算不是未计价");

    // (b) 分项已知 + 无分档 → **未计价**（cost 不含写费、unknown = 写总量、
    //    不得标估算）。
    let unpriced = p
        .estimate(
            "no-tier",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert_eq!(unpriced.cost, 0.0, "未计价不计金额");
    assert!(!unpriced.complete);
    assert!(!unpriced.cache_write_split_estimated, "未计价不是估算");
    assert_eq!(unpriced.unknown.cache_write, 2_000_000, "写 token 记未计价");

    // (c) 混合：两个请求分别估算与未计价时，估算金额与未计价数量
    //    不互相冒充（汇总口径下两者并存）。
    let both = p
        .estimate(
            "tiered",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    assert!((both.cost - 9.75).abs() < 1e-9, "可信分档用分档价");

    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 4（复核）：models.dev 基础层/分段层显式分档价校验 ─────────────

#[test]
fn n18_modelsdev_invalid_explicit_tiers_are_rejected_with_diagnostics() {
    for (tag, entry) in [
        (
            "base-negative",
            r#"{"id":"anthropic/claude-x","name":"X","input":3.0,"output":15.0,
                "cache_read":0.3,"cache_write":3.75,"cache_write_5m":-1.0,
                "cache_write_1h":6.0}"#,
        ),
        (
            "base-nonfinite",
            r#"{"id":"anthropic/claude-x","name":"X","input":3.0,"output":15.0,
                "cache_read":0.3,"cache_write":3.75,"cache_write_5m":1e400,
                "cache_write_1h":6.0}"#,
        ),
        (
            "segment-negative",
            r#"{"id":"anthropic/claude-x","name":"X","input":3.0,"output":15.0,
                "cache_read":0.3,"cache_write":3.75,
                "segments":[{"label":">272K","min_tokens":272001,
                  "input":6.0,"output":30.0,"cache_read":0.6,"cache_write":7.5,
                  "cache_write_1h":-1.0}]}"#,
        ),
    ] {
        let dir = md_dir(&format!("md-invalid-{tag}"));
        let md = snapshot_json(
            &dir,
            &format!(r#"{{"v":4,"synced_at":"t","entries":[{entry}]}}"#),
        );
        let (p, warnings) = Pricing::load(None, Some(&md), None);
        // 非法值不得进入计价：候选被拒绝（未收录 → estimate 返回 None）。
        let est = p.estimate(
            "anthropic/claude-x",
            &split_counts(2_000_000, 1_000_000, 1_000_000),
            at(),
        );
        assert!(est.is_none(), "{tag}：非法分档价不得进入计价");
        assert!(
            !warnings.is_empty(),
            "{tag}：必须保留诊断，实际 {warnings:?}"
        );
        if tag != "base-nonfinite" {
            // 负数在条目级拒绝：诊断必须指出条目。非有限 JSON 数值在快照
            // 解析阶段即被 serde 拒绝（number out of range），诊断指出文件，
            // 同样不会进入计价。
            assert!(
                warnings.iter().any(|w| w.contains("claude-x")),
                "{tag}：诊断必须指出条目，实际 {warnings:?}"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[test]
fn n18_unknown_split_multi_candidate_discloses_event_gap_not_candidate_price_gap() {
    // 复核项 5 原反例：事件分档未知；A 只有合并价（完整），B 分档价齐全
    // （因事件缺分档而估算/不完整）。完整候选优先规则不变，但排除原因
    // 必须写成「事件缺少分档」，不得误写成「候选缺少价格」。
    let dir = md_dir("unknown-split-multi");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "chan-a/tiered"
input = 3.0
output = 15.0
cache_write = 3.75

[[model]]
prefix = "chan-b/tiered"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("chan-a/tiered", &unknown_split_counts(2_000_000), at())
        .unwrap();
    // A 完整（合并价），按既有规则胜出。
    let matched = est.matched.as_ref().unwrap();
    assert_eq!(matched.raw_key, "chan-a/tiered");
    assert_eq!(matched.reason, "highest_complete_cost");
    assert!(est.complete);
    assert!((est.cost - 7.5).abs() < 1e-9);
    // B 被排除：原因必须是事件缺分档，而不是候选缺价。
    assert!(matched.incomplete_candidates_excluded >= 1);
    let warning = est
        .excluded_candidate_warning
        .as_deref()
        .expect("必须披露被排除的候选");
    assert!(
        warning.contains("事件缺少 5 分钟/1 小时写入分项"),
        "原因必须如实写成事件缺分档：{warning}"
    );
    assert!(
        !warning.contains("缺少价格"),
        "不得把事件缺分档写成候选缺价：{warning}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_tier_unknown_accumulates_overflow_and_missing_total() {
    // 复核项 8（真实 estimate 路径）：5m 200 万 token 单价合法但金额溢出、
    // 1h 100 万 token 缺价 → unknown.cache_write 必须累计为 300 万
    //（不得被后一档覆盖），两档都不计金额。
    let dir = md_dir("tier-unknown-sum");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 1e308
"#,
    );
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
    let est = p
        .estimate("m", &split_counts(3_000_000, 2_000_000, 1_000_000), at())
        .unwrap();
    assert!(!est.complete);
    assert_eq!(
        est.unknown.cache_write, 3_000_000,
        "溢出档与缺价档的未计价 token 必须累计"
    );
    assert_eq!(est.cost, 0.0, "两档都不得计入金额");
    use tokenscope::pricing::CostLineKind;
    let w5 = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite5m)
        .unwrap();
    let w1 = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite1h)
        .unwrap();
    assert!(w5.overflow && !w5.priced, "5m 档金额溢出");
    assert!(!w1.priced && w1.unit_price.is_none(), "1h 档缺价");
    assert_eq!((w5.tokens, w1.tokens), (2_000_000, 1_000_000));
    assert!(!est.cache_write_split_estimated, "这是未计价不是估算");
    std::fs::remove_dir_all(&dir).ok();
}

// ── N18 五轮（复核项 1/2/3）─────────────────────────────────────────

#[test]
fn n18_time_rule_declaring_only_tiers_is_recognized() {
    // 复核项 1：规则只声明 5m/1h（没有 period、没有四类单价）也必须被
    // 识别为已声明规则；两档各 1M → 7.5 + 12 = $19.5，而不是回退基础 9.75。
    let dir = tiered_dir("rule-only-tiers");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule]]
label = "peak"
timezone = "UTC"
cache_write_5m = 7.5
cache_write_1h = 12.0
"#,
    );
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
    let est = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!((est.cost - 19.5).abs() < 1e-9, "实际 {}", est.cost);
    assert!(est.complete);
    assert_eq!(
        est.matched.as_ref().unwrap().schedule_label.as_deref(),
        Some("peak"),
        "只声明分档价的规则必须被选中"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_time_rule_segment_declaring_only_tiers_is_recognized() {
    // 复核项 1：规则内分段只声明分档价，同样必须保留规则并参与计价。
    let dir = tiered_dir("rule-seg-only-tiers");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule]]
label = "peak"
timezone = "UTC"

[[model.schedule.segment]]
min_tokens = 0
cache_write_5m = 7.5
cache_write_1h = 12.0
"#,
    );
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "warnings: {warnings:?}");
    let est = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!((est.cost - 19.5).abs() < 1e-9, "实际 {}", est.cost);
    assert!(est.complete);
    assert_eq!(
        est.matched.as_ref().unwrap().schedule_label.as_deref(),
        Some("peak")
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_period_overriding_write_without_tiers_is_unknown_not_schedule_tiers() {
    // 复核项 2：period 显式 cache_write=9 但无分档价 → 不得借用 schedule
    // 的 3.75/6 并标完整；写 token 记未计价。
    let dir = tiered_dir("period-write-no-tiers");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule]]
label = "peak"
timezone = "UTC"
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule.period]]
start_time = "00:00"
end_time = "23:59"
cache_write = 9.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!(!est.complete, "period 改写写入价但无分档依据 → 不得完整");
    assert_eq!(est.unknown.cache_write, 2_000_000);
    assert!(!est.cache_write_split_estimated, "这是未知不是估算");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_period_without_override_inherits_schedule_tiers() {
    // 对照：period 未覆盖价格 → 正常继承 schedule 的分档价。
    let dir = tiered_dir("period-inherit");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule]]
label = "peak"
timezone = "UTC"
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule.period]]
start_time = "00:00"
end_time = "23:59"
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!((est.cost - 9.75).abs() < 1e-9, "实际 {}", est.cost);
    assert!(est.complete);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_period_declaring_own_tiers_prices_correctly() {
    // 对照：period 自己声明分档价 → 用 period 的分档（7.5/12）。
    let dir = tiered_dir("period-own-tiers");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule]]
label = "peak"
timezone = "UTC"
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model.schedule.period]]
start_time = "00:00"
end_time = "23:59"
cache_write = 9.0
cache_write_5m = 7.5
cache_write_1h = 12.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("m", &split_counts(2_000_000, 1_000_000, 1_000_000), at())
        .unwrap();
    assert!((est.cost - 19.5).abs() < 1e-9, "实际 {}", est.cost);
    assert!(est.complete);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_unknown_split_without_merged_price_is_unpriced_not_estimated() {
    // 复核项 3：候选声明分档价但没有合并写价 → 写 token 未计价，
    // 不得声称按合并价估算。
    let dir = tiered_dir("no-merged-write");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write_5m = 3.75
cache_write_1h = 6.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("m", &unknown_split_counts(2_000_000), at())
        .unwrap();
    assert!(!est.complete);
    assert_eq!(est.unknown.cache_write, 2_000_000, "写 token 未计价");
    assert!(!est.cache_write_split_estimated, "缺合并价不是估算");
    assert_eq!(est.cost, 0.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_unknown_split_with_overflowing_merged_price_is_unpriced_not_estimated() {
    // 复核项 3：合并写价金额溢出 → 同样未计价，不得声称估算。
    let dir = tiered_dir("overflow-merged-write");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 1e308
cache_write_5m = 3.75
cache_write_1h = 6.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("m", &unknown_split_counts(2_000_000), at())
        .unwrap();
    assert!(!est.complete);
    assert_eq!(est.unknown.cache_write, 2_000_000);
    assert!(!est.cache_write_split_estimated, "金额溢出不是估算");
    assert_eq!(est.cost, 0.0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_unknown_split_with_explicit_zero_merged_price_is_estimated_zero() {
    // 复核项 3 正例：合并价为合法显式 0 → 估算成立（金额 0、无未计价），
    // 分档价仍由候选声明（1h=6）作为依据。
    let dir = tiered_dir("zero-merged-write");
    let toml = write_toml(
        &dir,
        r#"
[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 0.0
cache_write_5m = 0.0
cache_write_1h = 6.0
"#,
    );
    let (p, _) = Pricing::load(Some(&toml), None, None);
    let est = p
        .estimate("m", &unknown_split_counts(2_000_000), at())
        .unwrap();
    assert!(!est.complete);
    assert!(est.cache_write_split_estimated, "显式 0 是合法估算依据");
    assert_eq!(est.unknown.cache_write, 0, "显式 0 不是未计价");
    assert_eq!(est.cost, 0.0);
    std::fs::remove_dir_all(&dir).ok();
}
