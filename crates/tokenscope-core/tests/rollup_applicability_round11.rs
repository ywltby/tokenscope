//! PR #70 第十一轮复核（基线 feff517）：日汇总丢失「缓存缺价按零估算」标记。
//!
//! 同一模型两个候选：`external/mixed-zero` 显式 `cache_read = $0`、
//! `provider/mixed-zero`（线上来源）缺缓存读价 → 按 0 估算。
//! 日总量 1M 缓存读 + 1M 缓存写有两种真实构成，合计都是 $2：
//! 1. 分开：1M 只读（external 显式 $0，完整）+ 1M 只写（provider $2，完整）；
//! 2. 合并：两次各 0.5M 读 + 0.5M 写 —— 没有完整候选，回退到已知费用最高者
//!    （provider），缓存读按 0 **估算**，complete=false。
//!
//! 金额一致不等于依据一致：日汇总必须保留金额 $2、不把 token 记未知，同时
//! 置 `cache_price_zero_estimated=true` 并保持 `complete=false`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{Group, GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::pricing::Pricing;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-r11-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pricing(dir: &Path, toml: &str, openrouter: Option<&str>) -> Pricing {
    let external = dir.join("pricing.toml");
    std::fs::write(&external, toml).unwrap();
    let online = openrouter.map(|json| {
        let path = dir.join("openrouter.json");
        std::fs::write(&path, json).unwrap();
        path
    });
    let (pricing, warnings) = Pricing::load(Some(&external), None, online.as_deref());
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

fn group(pricing: &Pricing, model: &str, tokens: TokenCounts, requests: u64) -> Group {
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

fn at() -> jiff::Timestamp {
    "2026-11-16T12:00:00Z".parse().unwrap()
}

fn read(n: u64) -> TokenCounts {
    TokenCounts {
        cache_read: n,
        ..Default::default()
    }
}

fn write(n: u64) -> TokenCounts {
    TokenCounts {
        cache_write: n,
        ..Default::default()
    }
}

fn both(n: u64) -> TokenCounts {
    TokenCounts {
        cache_read: n,
        cache_write: n,
        ..Default::default()
    }
}

/// 外部候选显式 `cache_read = $0`、写入 $1（声明分档 → 分档未知时按合并价
/// 估算）；线上候选缺缓存读价（按 0 估算）、写入 $2。
const MIXED: &str = "[[model]]\nprefix='external/mixed-zero'\ninput=1.0\ncache_read=0.0\ncache_write=1.0\ncache_write_5m=1.0\ncache_write_1h=2.0\n";
const MIXED_ONLINE: &str = r#"{"v":2,"synced_at":"2026-11-16T00:00:00Z","entries":[{"id":"provider/mixed-zero","name":null,"prompt":0.000001,"completion":0.000001,"cache_read":null,"cache_write":0.000002}]}"#;

/// 请求级事实：两种构成合计都是 $2，但第二种依赖「按 0 估算」。
#[test]
fn request_paths_differ_in_estimate_state_not_amount() {
    let dir = temp_dir("request");
    let p = pricing(&dir, MIXED, Some(MIXED_ONLINE));

    let separated_read = p.estimate("mixed-zero", &read(1_000_000), at()).unwrap();
    assert!(separated_read.complete);
    assert!(separated_read.cost.abs() < 1e-9, "显式 $0 费率");
    assert!(!separated_read.cache_price_zero_estimated);

    let separated_write = p.estimate("mixed-zero", &write(1_000_000), at()).unwrap();
    assert!(separated_write.complete);
    assert!((separated_write.cost - 2.0).abs() < 1e-9);

    let together = p.estimate("mixed-zero", &both(500_000), at()).unwrap();
    assert!(!together.complete, "缺缓存读价按 0 估算，不算完整");
    assert!(together.cache_price_zero_estimated, "必须披露按 0 估算");
    assert!((together.cost - 1.0).abs() < 1e-9, "实际 {}", together.cost);
    assert_eq!(
        together.unknown.cache_read, 0,
        "按 0 估算的 token 不进 unknown"
    );
}

/// 核心反例：金额可确定，但依据含「按 0 估算」→ 保留金额、标记估算、不完整。
#[test]
fn daily_keeps_amount_and_flags_zero_estimate() {
    let dir = temp_dir("daily");
    let p = pricing(&dir, MIXED, Some(MIXED_ONLINE));
    let total = TokenCounts {
        cache_read: 1_000_000,
        cache_write: 1_000_000,
        ..Default::default()
    };
    let est = p
        .estimate_rollup(
            "mixed-zero",
            &total,
            Some("2026-11-16".parse().unwrap()),
            Some(&jiff::tz::TimeZone::UTC),
        )
        .unwrap();
    assert!(
        (est.cost - 2.0).abs() < 1e-9,
        "金额仍可确定，实际 {}",
        est.cost
    );
    assert!(
        est.cache_price_zero_estimated,
        "任一可能胜出路径按 0 估算就必须披露"
    );
    assert!(!est.complete, "存在依赖假设的依据 → 不得声称完整");
    assert_eq!(est.unknown.cache_read, 0);
    assert_eq!(est.unknown.cache_write, 0);

    let g = group(&p, "mixed-zero", total, 2);
    assert_eq!(g.total_cost_usd, Some(2.0), "金额保留");
    assert!((g.cost_usd - 2.0).abs() < 1e-9);
    assert!(g.cache_price_zero_estimated, "标记必须传到 Group 与合计");
    assert_eq!(g.unknown_tokens.cache_read, 0);
    assert_eq!(g.unknown_tokens.cache_write, 0);
    assert!(!g.unknown_pricing, "没有真实未定价 token");
}

/// 对照：所有可能胜出路径都用显式零价 → 保持完整计价、不置估算标记。
#[test]
fn explicit_zero_only_stays_complete() {
    let dir = temp_dir("explicit");
    let p = pricing(
        &dir,
        "[[model]]\nprefix='explicit-zero'\ninput=1.0\ncache_read=0.0\n",
        None,
    );
    let tokens = read(1_000_000);
    let est = p
        .estimate_rollup(
            "explicit-zero",
            &tokens,
            Some("2026-11-16".parse().unwrap()),
            Some(&jiff::tz::TimeZone::UTC),
        )
        .unwrap();
    assert!(est.cost.abs() < 1e-9);
    assert!(est.complete, "显式零价是完整计价");
    assert!(!est.cache_price_zero_estimated);

    let g = group(&p, "explicit-zero", tokens, 1);
    assert_eq!(g.total_cost_usd, Some(0.0), "零元费率仍算有依据");
    assert!(!g.cache_price_zero_estimated);
    assert_eq!(g.unknown_tokens.cache_read, 0);
}

/// 对照：只有按 0 估算这一条路径时，金额为 0 但必须标记估算且不完整。
#[test]
fn assumed_zero_only_stays_incomplete() {
    let dir = temp_dir("assumed");
    let p = pricing(
        &dir,
        "[[model]]\nprefix='unused-prefix'\ninput=1.0\n",
        Some(
            r#"{"v":2,"synced_at":"2026-11-16T00:00:00Z","entries":[{"id":"provider/assumed-zero","name":null,"prompt":0.000001,"completion":0.000001,"cache_read":null}]}"#,
        ),
    );
    let tokens = read(1_000_000);
    let est = p
        .estimate_rollup(
            "assumed-zero",
            &tokens,
            Some("2026-11-16".parse().unwrap()),
            Some(&jiff::tz::TimeZone::UTC),
        )
        .unwrap();
    assert!(est.cost.abs() < 1e-9);
    assert!(est.cache_price_zero_estimated);
    assert!(!est.complete);
    assert_eq!(est.unknown.cache_read, 0);

    let g = group(&p, "assumed-zero", tokens, 1);
    assert_eq!(g.total_cost_usd, Some(0.0));
    assert!(g.cache_price_zero_estimated);
    assert_eq!(g.unknown_tokens.cache_read, 0);
}
