//! PR #70 第十轮复核（基线 76dfcea）：跨候选配比 + 缓存写分档完整性。
//!
//! 1. 跨候选：旧 `estimate_rollup` 先按日总量给每个价格候选算一遍、再取最高
//!    日费用。但请求级是「逐请求选最高后相加」，两者在候选价格交叉时不同：
//!    A(input3/output1) 与 B(input1/output3) 下，balanced 两次各 0.5M+0.5M
//!    合计 $4，separated（1M 纯输入 + 1M 纯输出）合计 $6 —— 日总量相同，
//!    输入/输出各自费率都不可确定。
//! 2. 缓存写分档：完整性不能只看「单价是否存在」。A 声明分档、按合并价估算
//!    时 complete=false；B 完全没声明分档才算完整。日汇总必须沿用同一判定，
//!    且扁平化后不得丢掉估算状态。
//!
//! 全部断言走真实核心库（合成 pricing.toml + aggregate_with_rollups），
//! 不依赖任何 gitignored 复核目录。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{Group, GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::pricing::{CostLineKind, Pricing};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-r10-{tag}-{}-{n}-{}",
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

fn daily(model: &str, tokens: TokenCounts, requests: u64) -> RollupContribution {
    RollupContribution {
        day: "2026-11-16".into(),
        source_tz: "UTC".into(),
        app: AgentKind::ClaudeCode,
        model: model.into(),
        requests,
        tokens,
        source_cost_usd: None,
        source: "synthetic-only".into(),
    }
}

fn rollup_for(pricing: &Pricing, model: &str, tokens: TokenCounts, requests: u64) -> Group {
    aggregate_with_rollups(
        &[],
        &[daily(model, tokens, requests)],
        GroupBy::Model,
        &jiff::tz::TimeZone::UTC,
        pricing,
    )
    .unwrap()
    .totals
}

fn pair(input: u64, output: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        ..Default::default()
    }
}

/// 两个独立候选：A(input3/output1)、B(input1/output3)。
const CROSS: &str = r#"
[[model]]
prefix = "provider-a/cross-model"
input = 3.0
output = 1.0
[[model]]
prefix = "provider-b/cross-model"
input = 1.0
output = 3.0
"#;

/// 声明顺序交换的同一组候选。
const CROSS_SWAPPED: &str = r#"
[[model]]
prefix = "provider-b/cross-model"
input = 1.0
output = 3.0
[[model]]
prefix = "provider-a/cross-model"
input = 3.0
output = 1.0
"#;

/// 请求级对照：两种真实构成日总量相同、请求级合计不同（$4 / $6）。
#[test]
fn request_decompositions_differ_between_candidates() {
    let dir = temp_dir("cross-request");
    let pricing = pricing_with(&dir, CROSS);
    let at: jiff::Timestamp = "2026-11-16T12:00:00Z".parse().unwrap();
    let mut balanced = 0.0;
    for _ in 0..2 {
        let est = pricing
            .estimate("cross-model", &pair(500_000, 500_000), at)
            .unwrap();
        assert!(est.complete, "两个候选都完整");
        balanced += est.cost;
    }
    let mut separated = 0.0;
    for (i, o) in [(1_000_000, 0), (0, 1_000_000)] {
        let est = pricing.estimate("cross-model", &pair(i, o), at).unwrap();
        assert!(est.complete, "单一分项请求由对应候选胜出");
        separated += est.cost;
    }
    assert!((balanced - 4.0).abs() < 1e-9, "balanced 合计 {balanced}");
    assert!((separated - 6.0).abs() < 1e-9, "separated 合计 {separated}");
}

/// 核心反例：候选间交叉定价时，日汇总不能给出任一侧费率。
#[test]
fn cross_candidate_composition_forces_both_components_unknown() {
    let dir = temp_dir("cross-daily");
    let pricing = pricing_with(&dir, CROSS);
    let t = rollup_for(&pricing, "cross-model", pair(1_000_000, 1_000_000), 2);
    assert_eq!(
        t.unknown_tokens.input, 1_000_000,
        "input 单价在候选间交叉，不可由日总量确定"
    );
    assert_eq!(
        t.unknown_tokens.output, 1_000_000,
        "output 单价在候选间交叉，不可由日总量确定"
    );
    assert_eq!(t.total_cost_usd, None, "没有可确定分项，金额必须整体未知");
}

/// 交换候选声明顺序，结论不应被顺序偶然决定。
#[test]
fn swapped_candidate_order_is_equivalent() {
    let d1 = temp_dir("cross-order-a");
    let d2 = temp_dir("cross-order-b");
    let a = rollup_for(
        &pricing_with(&d1, CROSS),
        "cross-model",
        pair(1_000_000, 1_000_000),
        2,
    );
    let b = rollup_for(
        &pricing_with(&d2, CROSS_SWAPPED),
        "cross-model",
        pair(1_000_000, 1_000_000),
        2,
    );
    assert_eq!(a.unknown_tokens.input, b.unknown_tokens.input);
    assert_eq!(a.unknown_tokens.output, b.unknown_tokens.output);
    assert_eq!(a.total_cost_usd, b.total_cost_usd);
    assert_eq!(a.total_cost_usd, None);
}

/// 对照：逐项支配的候选固定胜出，仍可完整计价。
#[test]
fn dominant_candidate_still_priced() {
    let dir = temp_dir("cross-dominant");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "provider-a/dominant-model"
input = 3.0
output = 1.0
[[model]]
prefix = "provider-b/dominant-model"
input = 2.0
output = 0.5
"#,
    );
    let t = rollup_for(&pricing, "dominant-model", pair(1_000_000, 1_000_000), 2);
    assert_eq!(t.unknown_tokens.input, 0);
    assert_eq!(t.unknown_tokens.output, 0);
    assert_eq!(t.total_cost_usd, Some(4.0), "实际 {:?}", t.total_cost_usd);
}

/// 对照：交叉候选共有恒定分项（cache_read 都 $1）仍保留。
#[test]
fn common_invariant_across_candidates_retained() {
    let dir = temp_dir("cross-common");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "provider-a/common-model"
input = 3.0
output = 1.0
cache_read = 1.0
[[model]]
prefix = "provider-b/common-model"
input = 1.0
output = 3.0
cache_read = 1.0
"#,
    );
    let tokens = TokenCounts {
        input: 1_000_000,
        output: 1_000_000,
        cache_read: 1_000_000,
        ..Default::default()
    };
    let t = rollup_for(&pricing, "common-model", tokens, 2);
    assert_eq!(t.unknown_tokens.cache_read, 0, "两侧共有恒定价必须保留");
    assert_eq!(t.unknown_tokens.input, 1_000_000, "input 在候选间交叉");
    assert_eq!(t.unknown_tokens.output, 1_000_000, "output 在候选间交叉");
    assert_eq!(t.total_cost_usd, Some(1.0), "实际 {:?}", t.total_cost_usd);
}

/// 对照：单一候选的平价照常完整计价。
#[test]
fn single_candidate_stays_complete() {
    let dir = temp_dir("single-candidate");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "solo-model"
input = 3.0
output = 1.0
"#,
    );
    let t = rollup_for(&pricing, "solo-model", pair(1_000_000, 1_000_000), 2);
    assert_eq!(t.unknown_tokens.input, 0);
    assert_eq!(t.unknown_tokens.output, 0);
    assert_eq!(t.total_cost_usd, Some(4.0));
}

/// 对照：两个候选各只覆盖一个分项时，仍无完整候选 → 两项都未知。
#[test]
fn candidates_without_any_complete_option_stay_unknown() {
    let dir = temp_dir("cross-partial");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "provider-a/partial-model"
input = 1.0
[[model]]
prefix = "provider-b/partial-model"
output = 2.0
"#,
    );
    let t = rollup_for(&pricing, "partial-model", pair(1_000_000, 1_000_000), 2);
    assert_eq!(t.unknown_tokens.input, 1_000_000);
    assert_eq!(t.unknown_tokens.output, 1_000_000);
    assert_eq!(t.total_cost_usd, None);
}

/// 分档完整性反例 A：按合并价估算的候选不得被当作完整竞争者。
///
/// A 声明分档（5m $3 / 1h $6）、按合并价 $3 估算 → complete=false；
/// B 从未声明分档 → 既有兼容口径完整、费用 $1。请求级选 B，日汇总必须同样是 $1。
#[test]
fn split_estimated_candidate_is_not_a_complete_competitor() {
    let dir = temp_dir("tier-competition");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "tier-competition"
[[model.schedule]]
timezone = "UTC"
cache_write = 3.0
cache_write_5m = 3.0
cache_write_1h = 6.0
[[model.schedule]]
timezone = "UTC"
cache_write = 1.0
"#,
    );
    let tokens = TokenCounts {
        cache_write: 1_000_000,
        ..Default::default()
    };
    let at: jiff::Timestamp = "2026-11-16T12:00:00Z".parse().unwrap();
    let request = pricing.estimate("tier-competition", &tokens, at).unwrap();
    assert!(request.complete, "请求级完整优先选中 B");
    assert!((request.cost - 1.0).abs() < 1e-9, "实际 {}", request.cost);

    let t = rollup_for(&pricing, "tier-competition", tokens, 1);
    assert_eq!(
        t.total_cost_usd,
        Some(1.0),
        "日汇总必须与请求级一致，实际 {:?}",
        t.total_cost_usd
    );
    assert_eq!(t.unknown_tokens.cache_write, 0);
    assert!(!t.cache_write_split_estimated, "胜出候选是完整口径");
}

/// 分档完整性反例 B：单候选也不能把分档估算抹成完整定价。
///
/// 基础价 cache_write=$3、5m=$3、1h=$6，事件分档未知：
/// 请求级 cost=$3、complete=false、split_estimated=true。
/// 日汇总必须保留同一金额、同一不完整状态与估算标记。
#[test]
fn single_candidate_keeps_split_estimate_state() {
    let dir = temp_dir("tier-single");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "tier-single"
cache_write = 3.0
cache_write_5m = 3.0
cache_write_1h = 6.0
"#,
    );
    let tokens = TokenCounts {
        cache_write: 1_000_000,
        ..Default::default()
    };
    let at: jiff::Timestamp = "2026-11-16T12:00:00Z".parse().unwrap();
    let request = pricing.estimate("tier-single", &tokens, at).unwrap();
    assert!((request.cost - 3.0).abs() < 1e-9);
    assert!(!request.complete, "分档未知时按合并价估算，不算完整");
    assert!(request.cache_write_split_estimated);

    let est = pricing
        .estimate_rollup(
            "tier-single",
            &tokens,
            Some("2026-11-16".parse().unwrap()),
            Some(&jiff::tz::TimeZone::UTC),
        )
        .unwrap();
    assert!((est.cost - 3.0).abs() < 1e-9, "实际 {}", est.cost);
    assert!(!est.complete, "扁平化不得恢复成完整定价");
    assert!(est.cache_write_split_estimated, "必须保留估算标记");
    assert!(
        est.lines
            .iter()
            .any(|l| l.kind == CostLineKind::CacheWrite && l.priced),
        "合并写价仍作为可证明费率保留"
    );

    let t = rollup_for(&pricing, "tier-single", tokens, 1);
    assert_eq!(t.total_cost_usd, Some(3.0));
    assert_eq!(t.unknown_tokens.cache_write, 0, "金额已按合并价估算");
    assert!(t.cache_write_split_estimated, "汇总必须披露分档估算");
}
