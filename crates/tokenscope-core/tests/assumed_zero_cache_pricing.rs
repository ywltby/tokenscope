//! T3（2026-11-13）：「网站缺失缓存价按 0 估算」产品规则。
//!
//! 口径（用户指令）：
//! - 线上价格来源（models.dev / OpenRouter）确实未提供的 `cache_read` /
//!   `cache_write` 单价，按 0 参与估算——不再让该分项整体进入未知；
//! - 原始快照里的缺失值保留（事实层不变），计算时记录「缺失缓存价按 0
//!   估算」，与来源明确给出的 0（免费）区分；
//! - 已有有效非零价继续使用，不被零假设覆盖；
//! - 不把缺失输入/输出价、整个模型无匹配价、非法价格、计算溢出、分档
//!   缺档（N18 既有口径）变成免费；
//! - 完整候选保持原选择优先级：按 0 估算的候选仍是不完整候选，只有没有
//!   完整候选时才回退胜出（`highest_partial_cost`），不冒充完整官方价。
//!
//! 验收：同末段两个渠道候选——A 渠道四价齐全（费率高）、B 渠道缺缓存价
//! （按 0 估算后费率低）→ 仍选 A（完整优先）；只有 B 时按 B 估算且缓存
//! 行 `rate_kind = assumed_zero`、金额 0、complete=false。

use tokenscope::model::TokenCounts;
use tokenscope::pricing::{CostLineKind, Pricing, RateKind};

fn write_modelsdev(dir: &std::path::Path, entries: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("pricing-modelsdev.json");
    std::fs::write(
        &p,
        format!(r#"{{"v":2,"synced_at":"t","entries":[{entries}]}}"#),
    )
    .unwrap();
    p
}

fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn at() -> jiff::Timestamp {
    "2026-11-12T10:00:00Z".parse().unwrap()
}

/// 核心：快照缺缓存价 → 缓存分项按 0 计入金额（不再是未知 token），
/// 但 complete=false、行 rate_kind=assumed_zero（不冒充完整官方价）。
#[test]
fn missing_online_cache_prices_estimate_at_zero() {
    let dir = std::env::temp_dir().join(format!("t3-zero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries = r#"
        {"id":"prov/no-cache-model","name":null,"input":1.0,"output":2.0}
    "#;
    let md = write_modelsdev(&dir, entries);
    let (p, warnings) = Pricing::load(None, Some(&md), None);
    assert!(warnings.is_empty(), "按 0 估算不是采集警告：{warnings:?}");

    // 1M 输入 + 1M 输出 + 1M 缓存写 + 0.5M 缓存读：
    // 金额 = 1×1 + 1×2 + 0（缺价按 0）+ 0（缺价按 0）= 3。
    let est = p
        .estimate(
            "no-cache-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .expect("模型有候选，必须计价");
    assert!(
        (est.cost - 3.0).abs() < 1e-9,
        "缓存价缺失按 0 参与估算：金额 3.00，实际 {}",
        est.cost
    );
    // 按 0 估算的 token 不再进 unknown（金额已按 0 计入），但整体不完整。
    assert!(!est.complete, "按 0 估算不得冒充完整官方价");
    assert_eq!(
        est.unknown.cache_write, 0,
        "缓存写按 0 计入后不是未知 token"
    );
    assert_eq!(est.unknown.cache_read, 0, "缓存读按 0 计入后不是未知 token");
    assert_eq!(est.unknown.input, 0);
    assert_eq!(est.unknown.output, 0);
    let cw = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite)
        .unwrap();
    assert!(cw.priced, "按 0 估算是已计价行为");
    assert_eq!(cw.unit_price, Some(0.0));
    assert_eq!(cw.subtotal, 0.0);
    assert_eq!(cw.rate_kind, RateKind::AssumedZero, "必须与来源显式 0 区分");
    assert!(est.cache_price_zero_estimated, "估算依据标志必须随结果披露");
    let cr = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheRead)
        .unwrap();
    assert_eq!(cr.rate_kind, RateKind::AssumedZero);
    std::fs::remove_dir_all(&dir).ok();
}

/// 来源显式 0（免费）保持 Fixed 语义，不与按 0 估算混淆。
#[test]
fn explicit_zero_from_source_stays_fixed_not_assumed() {
    let dir = std::env::temp_dir().join(format!("t3-explicit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries = r#"
        {"id":"prov/free-cache-model","name":null,"input":1.0,"output":2.0,"cache_read":0,"cache_write":0}
    "#;
    let md = write_modelsdev(&dir, entries);
    let (p, _) = Pricing::load(None, Some(&md), None);
    let est = p
        .estimate(
            "free-cache-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    assert!((est.cost - 3.0).abs() < 1e-9);
    assert!(est.complete, "来源显式 0 是完整价格");
    let cw = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite)
        .unwrap();
    assert_eq!(cw.rate_kind, RateKind::Fixed, "显式 0 是来源明确给出的免费");
    std::fs::remove_dir_all(&dir).ok();
}

/// 已有有效非零缓存价继续使用，不被零假设覆盖。
#[test]
fn existing_nonzero_cache_prices_unchanged() {
    let dir = std::env::temp_dir().join(format!("t3-nonzero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries = r#"
        {"id":"prov/full-model","name":null,"input":1.0,"output":2.0,"cache_read":0.5,"cache_write":6.25}
    "#;
    let md = write_modelsdev(&dir, entries);
    let (p, _) = Pricing::load(None, Some(&md), None);
    let est = p
        .estimate(
            "full-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    // 1 + 2 + 6.25 + 0.25 = 9.5；价格照旧，完整计价。
    assert!((est.cost - 9.5).abs() < 1e-9, "{}", est.cost);
    assert!(est.complete);
    let cw = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite)
        .unwrap();
    assert!((cw.unit_price.unwrap() - 6.25).abs() < 1e-9);
    assert_eq!(cw.rate_kind, RateKind::Fixed);
    std::fs::remove_dir_all(&dir).ok();
}

/// 完整候选保持优先：缺缓存价候选按 0 估算后总额更低，但完整候选胜出。
#[test]
fn complete_candidate_still_wins_over_zero_assumed() {
    let dir = std::env::temp_dir().join(format!("t3-priority-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries = r#"
        {"id":"full/both-model","name":null,"input":1.0,"output":2.0,"cache_read":0.5,"cache_write":6.25},
        {"id":"cheap/no-cache-model","name":null,"input":0.1,"output":0.2}
    "#;
    let md = write_modelsdev(&dir, entries);
    let (p, _) = Pricing::load(None, Some(&md), None);
    // 查询名命中两个候选（末段不同——直接用完整名查询分别验证即可）。
    // 这里验证：查询 both-model（唯一候选）照常完整；查询 no-cache-model
    // 走按 0 估算且 reason=highest_partial_cost。
    let est_full = p
        .estimate(
            "full/both-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    assert!(est_full.complete);
    assert!((est_full.cost - 9.5).abs() < 1e-9);
    let est_zero = p
        .estimate(
            "cheap/no-cache-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    assert!(!est_zero.complete);
    assert!((est_zero.cost - 0.3).abs() < 1e-9, "{}", est_zero.cost);
    // 两候选同末段时的完整优先：构造同名候选（同末段 both-model）。
    let entries_same = r#"
        {"id":"full/both-model","name":null,"input":1.0,"output":2.0,"cache_read":0.5,"cache_write":6.25},
        {"id":"cheap/both-model","name":null,"input":0.1,"output":0.2}
    "#;
    let md2 = write_modelsdev(&dir, entries_same);
    let (p2, _) = Pricing::load(None, Some(&md2), None);
    let est = p2
        .estimate(
            "both-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    assert!(
        (est.cost - 9.5).abs() < 1e-9,
        "完整候选（9.5）必须胜出按 0 估算候选（0.3），实际 {}",
        est.cost
    );
    assert!(est.complete, "完整候选胜出时保持完整");
    assert_eq!(
        est.matched.as_ref().map(|m| m.reason.as_str()),
        Some("highest_complete_cost")
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 边界：输入/输出价缺失不得按 0（保持未知）；分档缺档保持 N18 口径；
/// 无任何候选的模型保持全未知（返回 None）。
#[test]
fn boundaries_input_output_and_no_candidate_unchanged() {
    let dir = std::env::temp_dir().join(format!("t3-boundary-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // 输入价缺失：该分项保持未知，不按 0。
    let entries = r#"
        {"id":"prov/no-input-model","name":null,"output":2.0,"cache_read":0.5,"cache_write":6.25}
    "#;
    let md = write_modelsdev(&dir, entries);
    let (p, _) = Pricing::load(None, Some(&md), None);
    let est = p
        .estimate(
            "no-input-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete);
    assert_eq!(est.unknown.input, 1_000_000, "输入价缺失不得按 0");
    assert_eq!(est.unknown.cache_write, 0);
    assert_eq!(est.unknown.cache_read, 0);
    // 金额只有输出 + 缓存两项：2 + 6.25 + 0.25 = 8.5。
    assert!((est.cost - 8.5).abs() < 1e-9, "{}", est.cost);

    // 整模型无匹配：照旧 None（全未知），不是 0 费用。
    let est_none = p.estimate("totally-unknown", &counts(1, 1, 1, 1), at());
    assert!(est_none.is_none(), "无匹配候选不得按 0 估算");
    std::fs::remove_dir_all(&dir).ok();
}

/// 外置 pricing.toml（用户自己的价格文件）不套用线上按 0 规则：
/// 显式省略键的语义保持「未声明 → 分层回退 → 全部层未声明 = 未知」。
#[test]
fn external_toml_omitted_cache_keys_stay_unknown() {
    let dir = std::env::temp_dir().join(format!("t3-external-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let toml_path = dir.join("pricing.toml");
    std::fs::write(
        &toml_path,
        "[[model]]\nprefix = \"ext-model\"\ninput = 1.0\noutput = 2.0\n",
    )
    .unwrap();
    let (p, _) = Pricing::load(Some(&toml_path), None, None);
    let est = p
        .estimate(
            "ext-model",
            &counts(1_000_000, 1_000_000, 1_000_000, 500_000),
            at(),
        )
        .unwrap();
    assert!(!est.complete);
    assert_eq!(est.unknown.cache_write, 1_000_000, "外置表缺键保持未知语义");
    assert_eq!(est.unknown.cache_read, 500_000);
    assert!((est.cost - 3.0).abs() < 1e-9);
    let cw = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite)
        .unwrap();
    assert!(!cw.priced);
    std::fs::remove_dir_all(&dir).ok();
}
