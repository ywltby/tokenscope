//! PR #70 第九轮复核：请求用量「比例」反例（基线 f8f4a69）。
//!
//! 根因：`scenario_estimate` 枚举使用分项子集后，把每个分项单价**等权相加**
//! 判断胜出，等价于只测试 v=(1,1) 的请求；但请求级费用是 token 配比的线性
//! 函数，skewed 配比下胜出规则不同。两条常驻规则 A(input3/output1) 与
//! B(input2/output1.5) 在相同日总量下，balanced 构成 input 费用 $3、skewed
//! 构成 $2.9，输入单价本身无法由日总量确定。
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
        "tokenscope-v092-r9-{tag}-{}-{n}-{}",
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

fn rollup(pricing: &Pricing, model: &str) -> Group {
    aggregate_with_rollups(
        &[],
        &[daily(model, 1_000_000, 1_000_000, 2)],
        GroupBy::Model,
        &jiff::tz::TimeZone::UTC,
        pricing,
    )
    .unwrap()
    .totals
}

/// A: input=$3/M output=$1/M；B: input=$2/M output=$1.5/M，都常驻。
const UNEQUAL: &str = r#"
[[model]]
prefix = "unequal-ratio"
[[model.schedule]]
timezone = "UTC"
input = 3.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 2.0
output = 1.5
"#;

/// 声明顺序交换的同一组规则。
const UNEQUAL_SWAPPED: &str = r#"
[[model]]
prefix = "unequal-ratio"
[[model.schedule]]
timezone = "UTC"
input = 2.0
output = 1.5
[[model.schedule]]
timezone = "UTC"
input = 3.0
output = 1.0
"#;

/// 请求级对照：两种真实构成，日总量相同但输入费用不同。
#[test]
fn request_decompositions_have_same_totals_but_different_input_fee() {
    let dir = temp_dir("request-decomp");
    let pricing = pricing_with(&dir, UNEQUAL);
    let at: jiff::Timestamp = "2026-11-16T12:00:00Z".parse().unwrap();
    let p = pricing
        .estimate(
            "unequal-ratio",
            &TokenCounts {
                input: 500_000,
                output: 500_000,
                ..Default::default()
            },
            at,
        )
        .unwrap();
    assert!(p.complete);
    let mut balanced_total = 0.0;
    let mut balanced_input = 0.0;
    for _ in 0..2 {
        let est = pricing
            .estimate(
                "unequal-ratio",
                &TokenCounts {
                    input: 500_000,
                    output: 500_000,
                    ..Default::default()
                },
                at,
            )
            .unwrap();
        balanced_total += est.cost;
        balanced_input += est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::Input)
            .unwrap()
            .subtotal;
    }
    let mut skewed_total = 0.0;
    let mut skewed_input = 0.0;
    for (i, o) in [(900_000, 0), (100_000, 1_000_000)] {
        let est = pricing
            .estimate(
                "unequal-ratio",
                &TokenCounts {
                    input: i,
                    output: o,
                    ..Default::default()
                },
                at,
            )
            .unwrap();
        assert!(est.complete);
        skewed_total += est.cost;
        skewed_input += est
            .lines
            .iter()
            .find(|l| l.kind == CostLineKind::Input)
            .unwrap()
            .subtotal;
    }
    assert!(
        (balanced_total - 4.0).abs() < 1e-9,
        "balanced 合计 {balanced_total}"
    );
    assert!(
        (balanced_input - 3.0).abs() < 1e-9,
        "balanced 输入费 {balanced_input}"
    );
    assert!(
        (skewed_total - 4.4).abs() < 1e-9,
        "skewed 合计 {skewed_total}"
    );
    assert!(
        (skewed_input - 2.9).abs() < 1e-9,
        "skewed 输入费 {skewed_input}"
    );
    let _ = p;
}

/// 核心反例：输入费用本身也无法由日总量确定，两个分项都必须未知。
#[test]
fn skewed_ratio_forces_both_components_unknown() {
    let dir = temp_dir("both-unknown");
    let pricing = pricing_with(&dir, UNEQUAL);
    let t = rollup(&pricing, "unequal-ratio");
    assert_eq!(t.unknown_tokens.input, 1_000_000, "输入单价随配比变化");
    assert_eq!(t.unknown_tokens.output, 1_000_000, "输出单价随配比变化");
    assert_eq!(t.total_cost_usd, None, "没有可确定的分项，金额必须整体未知");
}

/// 交换规则声明顺序，结论不应被顺序偶然决定。
#[test]
fn swapped_rule_declaration_order_is_equivalent() {
    let d1 = temp_dir("order-a");
    let d2 = temp_dir("order-b");
    let a = rollup(&pricing_with(&d1, UNEQUAL), "unequal-ratio");
    let b = rollup(&pricing_with(&d2, UNEQUAL_SWAPPED), "unequal-ratio");
    assert_eq!(a.unknown_tokens.input, b.unknown_tokens.input);
    assert_eq!(a.unknown_tokens.output, b.unknown_tokens.output);
    assert_eq!(a.total_cost_usd, b.total_cost_usd);
    assert_eq!(a.total_cost_usd, None);
}

/// 对照：全局逐项支配的规则仍可证明计价（A 逐项 >= B）。
#[test]
fn globally_dominant_rule_still_priced() {
    let dir = temp_dir("dominant");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "dominant"
[[model.schedule]]
timezone = "UTC"
input = 3.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 2.0
output = 0.5
"#,
    );
    let t = rollup(&pricing, "dominant");
    assert_eq!(t.unknown_tokens.input, 0);
    assert_eq!(t.unknown_tokens.output, 0);
    // input 3/M + output 1/M = $4
    assert_eq!(
        t.total_cost_usd,
        Some(4.0),
        "支配规则固定胜出，实际 {:?}",
        t.total_cost_usd
    );
}

/// 对照：竞争规则共有的恒定分项仍保留（cache_read 都 $1）。
#[test]
fn common_invariant_component_is_retained() {
    let dir = temp_dir("common");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "common"
input = 1.0
cache_read = 1.0
[[model.schedule]]
timezone = "UTC"
input = 3.0
output = 1.0
[[model.schedule]]
timezone = "UTC"
input = 1.0
output = 3.0
"#,
    );
    let t = aggregate_with_rollups(
        &[],
        &[RollupContribution {
            day: "2026-11-16".into(),
            source_tz: "UTC".into(),
            app: AgentKind::ClaudeCode,
            model: "common".into(),
            requests: 2,
            tokens: TokenCounts {
                input: 1_000_000,
                output: 1_000_000,
                cache_read: 1_000_000,
                ..Default::default()
            },
            source_cost_usd: None,
            source: "synthetic-only".into(),
        }],
        GroupBy::Model,
        &jiff::tz::TimeZone::UTC,
        &pricing,
    )
    .unwrap()
    .totals;
    assert_eq!(t.unknown_tokens.cache_read, 0, "两侧共有恒定价必须保留");
    assert_eq!(t.unknown_tokens.input, 1_000_000, "input 费率相互竞争");
    assert_eq!(t.unknown_tokens.output, 1_000_000, "output 费率相互竞争");
    // 只有 cache_read $1 可证明：input 3/1 与 output 1/3 都随配比变化。
    assert_eq!(t.total_cost_usd, Some(1.0), "实际 {:?}", t.total_cost_usd);
}

/// 对照：没有完整候选时，部分候选的已知输入费用不被整体丢弃。
#[test]
fn partial_candidate_known_input_is_kept() {
    let dir = temp_dir("partial");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "partial"
[[model.schedule]]
timezone = "UTC"
input = 1.0
"#,
    );
    // output 无价：只有 input 是已知分项。
    let t = aggregate_with_rollups(
        &[],
        &[RollupContribution {
            day: "2026-11-16".into(),
            source_tz: "UTC".into(),
            app: AgentKind::ClaudeCode,
            model: "partial".into(),
            requests: 1,
            tokens: TokenCounts {
                input: 1_000_000,
                output: 1_000_000,
                ..Default::default()
            },
            source_cost_usd: None,
            source: "synthetic-only".into(),
        }],
        GroupBy::Model,
        &jiff::tz::TimeZone::UTC,
        &pricing,
    )
    .unwrap()
    .totals;
    assert_eq!(t.unknown_tokens.input, 0, "单条部分规则已知 input 必须保留");
    assert_eq!(t.unknown_tokens.output, 1_000_000, "output 真缺价");
    assert_eq!(t.total_cost_usd, Some(1.0), "实际 {:?}", t.total_cost_usd);
}

/// 两条各只定价一个分项的规则：合并请求没有完整候选，请求级回退到
/// 「部分候选中已知费用最高者」，选中哪条取决于 input/output 配比。
/// 因此这两个分项的费率都无法由日总量确定，必须保持未知。
#[test]
fn competing_partial_rules_leave_both_components_unknown() {
    let dir = temp_dir("two-partials");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "two-partials"
[[model.schedule]]
timezone = "UTC"
input = 1.0
[[model.schedule]]
timezone = "UTC"
output = 2.0
"#,
    );
    let at: jiff::Timestamp = "2026-11-16T12:00:00Z".parse().unwrap();
    // 请求级事实：合并请求时 A 已知 $1、B 已知 $2 → 选 B，input 落空。
    let combined = pricing
        .estimate(
            "two-partials",
            &TokenCounts {
                input: 1_000_000,
                output: 1_000_000,
                ..Default::default()
            },
            at,
        )
        .unwrap();
    assert!(!combined.complete, "合并请求没有完整候选");
    assert!(
        (combined.cost - 2.0).abs() < 1e-9,
        "回退到部分候选的已知最高费用，实际 {}",
        combined.cost
    );
    let t = rollup(&pricing, "two-partials");
    assert_eq!(
        t.unknown_tokens.input, 1_000_000,
        "input 在合并请求里落空，费率不可确定"
    );
    assert_eq!(
        t.unknown_tokens.output, 1_000_000,
        "output 在大 input 请求里落空，费率不可确定"
    );
    assert_eq!(t.total_cost_usd, None, "实际 {:?}", t.total_cost_usd);
}

/// 对照：基础层只定 cache_read、完全不参与 input/output 竞争时，它在部分
/// 候选回退里永远赢不了已知费用比较，不得把可证明的 input 拖成未知。
#[test]
fn non_competing_base_layer_keeps_provable_input() {
    let dir = temp_dir("base-cache-only");
    let pricing = pricing_with(
        &dir,
        r#"
[[model]]
prefix = "base-cache-only"
cache_read = 1.0
[[model.schedule]]
timezone = "UTC"
input = 1.0
"#,
    );
    let t = rollup(&pricing, "base-cache-only");
    assert_eq!(t.unknown_tokens.input, 0, "input 唯一价 $1，可证明");
    assert_eq!(t.unknown_tokens.output, 1_000_000, "output 无任何候选定价");
    assert_eq!(t.total_cost_usd, Some(1.0), "实际 {:?}", t.total_cost_usd);
}
