//! T1（2026-11-13）：OpenRouter 有效候选的缓存写 5m/1h 分档接入。
//!
//! 事实（用户已核实 + 本机快照只读取证）：
//! - `anthropic/claude-fable-5.2` 的 OpenRouter 快照四价齐全（$8/$40/$0.20/$10 /M），
//!   models.dev 快照没有该模型（同步曾超时）；旧实现遇到「事件有可信分档、
//!   候选只有合并写价」时把缓存写整体记为未计价——有效价格被无效化。
//! - 官方价（platform.claude.com/docs/en/about-claude/pricing）：输入 8、输出 40、
//!   5m 写 10、1h 写 16、缓存读 0.20。缓存读 0.20 是快照显式值，不是 0.1 × input。
//! - 派生规则与 models.dev Anthropic 官方渠道同款：发布价自证
//!   （cache_write == 1.25 × prompt）才认定官方 API 价，5m = cache_write、
//!   1h = 2 × prompt；不按模型名猜价、不跨渠道拼价。
//!
//! 验收数字：Fable 5.2 的 5m/1h 各 100 万 token → 写费 10 + 16 = 26；
//! 再加输入/输出/缓存读各 100 万 → 合计 8 + 40 + 0.20 + 26 = 74.20。

use tokenscope::model::TokenCounts;
use tokenscope::pricing::{CostLineKind, Pricing};

fn write_openrouter(dir: &std::path::Path, entries: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("pricing-openrouter.json");
    std::fs::write(&p, format!(r#"{{"synced_at":"t","entries":[{entries}]}}"#)).unwrap();
    p
}

fn split_counts(input: u64, output: u64, cw: u64, cr: u64, w5: u64, w1: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
        cache_write_5m: w5,
        cache_write_1h: w1,
        cache_write_split_known: true,
    }
}

fn at() -> jiff::Timestamp {
    "2026-11-12T10:00:00Z".parse().unwrap()
}

const FABLE_52: &str = r#"{"id":"anthropic/claude-fable-5.2","name":"Anthropic: Claude Fable 5.2","prompt":8e-06,"completion":4e-05,"cache_read":2e-07,"cache_write":1e-05}"#;

#[test]
fn openrouter_official_candidate_prices_split_cache_writes() {
    let dir = std::env::temp_dir().join(format!("t1-fable-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let or = write_openrouter(&dir, FABLE_52);
    let (p, warnings) = Pricing::load(None, None, Some(&or));
    assert!(warnings.is_empty(), "warnings: {warnings:?}");

    // 5m/1h 各 100 万：写费 = 10 + 16 = 26（派生自快照自证价，非旧 0.1×input）。
    let est = p
        .estimate(
            "claude-fable-5.2",
            &split_counts(0, 0, 2_000_000, 0, 1_000_000, 1_000_000),
            at(),
        )
        .expect("OpenRouter 候选存在，必须计价");
    assert!(
        (est.cost - 26.0).abs() < 1e-9,
        "写费两档合计 $26，实际 {}",
        est.cost
    );
    assert!(est.complete, "分档齐全时必须完整计价");
    assert!(!est.cache_write_split_estimated);
    let w5 = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite5m)
        .expect("必须有 5m 行");
    let w1 = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheWrite1h)
        .expect("必须有 1h 行");
    assert_eq!(w5.tokens, 1_000_000);
    assert!(
        (w5.unit_price.unwrap() - 10.0).abs() < 1e-9,
        "{}",
        w5.unit_price.unwrap()
    );
    assert!((w5.subtotal - 10.0).abs() < 1e-9);
    assert_eq!(w1.tokens, 1_000_000);
    assert!(
        (w1.unit_price.unwrap() - 16.0).abs() < 1e-9,
        "{}",
        w1.unit_price.unwrap()
    );
    assert!((w1.subtotal - 16.0).abs() < 1e-9);

    // 全桶验收：输入/输出/缓存读/写各 100 万（写拆两档各 100 万）
    // = 8 + 40 + 0.20 + 10 + 16 = 74.20。缓存读用快照显式 0.20，不是 0.1×input=0.80。
    let est = p
        .estimate(
            "claude-fable-5.2",
            &split_counts(
                1_000_000, 1_000_000, 2_000_000, 1_000_000, 1_000_000, 1_000_000,
            ),
            at(),
        )
        .unwrap();
    assert!(
        (est.cost - 74.20).abs() < 1e-9,
        "全桶合计 $74.20，实际 {}",
        est.cost
    );
    let cr = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheRead)
        .unwrap();
    assert!(
        (cr.unit_price.unwrap() - 0.20).abs() < 1e-9,
        "缓存读价必须是快照显式 0.20，实际 {}",
        cr.unit_price.unwrap()
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn openrouter_official_tiers_survive_equivalent_model_spelling() {
    // 模型等价写法：`claude-fable-5-2`（`-` 归一）命中同一 OpenRouter 候选。
    let dir = std::env::temp_dir().join(format!("t1-equiv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let or = write_openrouter(&dir, FABLE_52);
    let (p, _) = Pricing::load(None, None, Some(&or));
    let est = p
        .estimate(
            "claude-fable-5-2",
            &split_counts(0, 0, 2_000_000, 0, 1_000_000, 1_000_000),
            at(),
        )
        .expect("等价写法必须命中同一候选");
    assert!((est.cost - 26.0).abs() < 1e-9, "{}", est.cost);
    assert!(est.complete);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn openrouter_non_official_channel_does_not_derive_tiers() {
    // 非官方渠道（自证不过：cache_write ≠ 1.25 × prompt）不得派生分档，
    // 也不得借官方候选的派生比率拼价——保持既有「合并价口径」。
    let dir = std::env::temp_dir().join(format!("t1-nonoff-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries = r#"
        {"id":"acme/claude-fable-5.2","name":null,"prompt":8e-06,"completion":4e-05,"cache_read":2e-07,"cache_write":9e-06},
        {"id":"anthropic/claude-fable-5.1","name":null,"prompt":1e-05,"completion":5e-05,"cache_read":2.5e-07,"cache_write":1.25e-05}
    "#;
    let or = write_openrouter(&dir, entries);
    let (p, _) = Pricing::load(None, None, Some(&or));
    // acme 渠道自证不过：分档未知事件保持合并价估算口径（不派生、不猜档）。
    let est = p
        .estimate(
            "claude-fable-5.2",
            &split_counts(0, 0, 2_000_000, 0, 0, 0),
            at(),
        )
        .unwrap();
    assert_eq!(
        est.lines
            .iter()
            .find(|l| l.kind == CostLineKind::CacheWrite5m),
        None,
        "自证不过的渠道不得产出分档行"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn openrouter_official_split_with_zero_and_missing_tiers() {
    // 分档为零：显式 0 是合法价（免费档照常计 0）；缺档进 unknown。
    let dir = std::env::temp_dir().join(format!("t1-zero-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries = r#"
        {"id":"anthropic/free-write-model","name":null,"prompt":8e-06,"completion":4e-05,"cache_read":0,"cache_write":1e-05},
        {"id":"anthropic/partial-tiers-model","name":null,"prompt":8e-06,"completion":4e-05,"cache_read":2e-07,"cache_write":1e-05}
    "#;
    let or = write_openrouter(&dir, entries);
    let (p, _) = Pricing::load(None, None, Some(&or));
    // 自证通过（1e-5 == 1.25 × 8e-6）→ 5m = 10、1h = 16；缓存读显式 0 保留免费。
    let est = p
        .estimate(
            "free-write-model",
            &split_counts(0, 0, 2_000_000, 500_000, 1_000_000, 1_000_000),
            at(),
        )
        .unwrap();
    let cr = est
        .lines
        .iter()
        .find(|l| l.kind == CostLineKind::CacheRead)
        .unwrap();
    assert_eq!(cr.unit_price, Some(0.0), "显式 0 = 免费，不是未知");
    assert_eq!(cr.subtotal, 0.0);
    assert!(
        (est.cost - 26.0).abs() < 1e-9,
        "缓存读免费，写费仍 26，实际 {}",
        est.cost
    );
    assert!(est.complete);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn openrouter_candidate_without_cache_prices_estimate_at_zero() {
    // T3（2026-11-13）：线上来源确实未提供缓存价 → 按 0 参与估算（不再是
    // 未知 token），但 complete=false 且与来源显式 0 区分（见
    // assumed_zero_cache_pricing.rs 的完整边界）。已知分项照常计价。
    let dir = std::env::temp_dir().join(format!("t1-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let entries =
        r#"{"id":"anthropic/no-cache-model","name":null,"prompt":8e-06,"completion":4e-05}"#;
    let or = write_openrouter(&dir, entries);
    let (p, _) = Pricing::load(None, None, Some(&or));
    let est = p
        .estimate(
            "no-cache-model",
            &split_counts(1_000_000, 1_000_000, 1_000_000, 500_000, 0, 0),
            at(),
        )
        .unwrap();
    assert!(!est.complete, "按 0 估算不冒充完整官方价");
    assert_eq!(est.unknown.cache_write, 0, "缓存写按 0 计入，不再进未知");
    assert_eq!(est.unknown.cache_read, 0);
    // 已知分项照常：8 + 40，缓存两项按 0。
    assert!((est.cost - 48.0).abs() < 1e-9, "{}", est.cost);
    std::fs::remove_dir_all(&dir).ok();
}
