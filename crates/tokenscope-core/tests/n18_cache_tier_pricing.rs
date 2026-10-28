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
