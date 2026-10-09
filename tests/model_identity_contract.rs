//! MP03：可信模型展示名——来源、独立性、顺序无关与旧快照兜底。
//!
//! 计划：docs/plans/active/2026-10-10-model-pricing-name-equivalence.md §2.12/§3。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::model::TokenCounts;
use tokenscope::model_identity::ModelIdentity;
use tokenscope::modelsdev::{load_snapshot, sync_with_body_for_tests};
use tokenscope::pricing::Pricing;

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("tokenscope-mp03-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
    }
}

fn at() -> jiff::Timestamp {
    "2026-10-07T05:00:00Z".parse().unwrap()
}

fn sync_to(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    sync_with_body_for_tests(&path, body).unwrap();
    path
}

#[test]
fn model_display_name_uses_model_not_provider() {
    let dir = tmp("provider");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{
            "acme": {"name": "Acme Provider", "models": {
                "opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}},
                "legacy-x": {"cost": {"input": 1, "output": 2}}
            }}
        }"#,
    );
    let s = load_snapshot(&snap).unwrap().unwrap();
    assert_eq!(s.v, 4, "同步产出的快照必须是 v4");
    let by = |id: &str| s.entries.iter().find(|e| e.id == id).unwrap();
    assert_eq!(by("acme/opus-5.5").name.as_deref(), Some("Opus 5.5"));
    assert_eq!(
        by("acme/legacy-x").name,
        None,
        "模型无 name 时不得用 provider 名兜底"
    );
}

#[test]
fn model_display_name_is_not_identity() {
    let dir = tmp("name-identity");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{
            "acme": {"models": {
                "opus-5.5": {"name": "Shared Name", "cost": {"input": 3, "output": 15}},
                "other-model": {"name": "Shared Name", "cost": {"input": 5, "output": 25}}
            }}
        }"#,
    );
    let (p, _warn) = Pricing::load(None, Some(&snap), None);
    // 名称相同不等于身份相同：两个 ID 各自命中自己的 name 与价格。
    assert_eq!(
        p.display_name_for("acme/opus55").as_deref(),
        Some("Shared Name")
    );
    assert_eq!(
        p.display_name_for("acme/other_model").as_deref(),
        Some("Shared Name")
    );
    assert_ne!(
        ModelIdentity::parse("acme/opus55").identity_key(),
        ModelIdentity::parse("acme/other-model").identity_key()
    );
    let a = p
        .estimate("acme/opus55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    let b = p
        .estimate("acme/other-model", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert!((a.cost - 3.0).abs() < 1e-9, "{}", a.cost);
    assert!((b.cost - 5.0).abs() < 1e-9, "{}", b.cost);
}

#[test]
fn model_display_name_is_order_independent() {
    let dir = tmp("order");
    let body_acme_first = r#"{
        "acme": {"models": {"opus-5.5": {"name": "Acme Name", "cost": {"input": 3, "output": 15}}}},
        "beta": {"models": {"opus55": {"name": "Beta Name", "cost": {"input": 5, "output": 25}}}}
    }"#;
    let body_beta_first = r#"{
        "beta": {"models": {"opus55": {"name": "Beta Name", "cost": {"input": 5, "output": 25}}}},
        "acme": {"models": {"opus-5.5": {"name": "Acme Name", "cost": {"input": 3, "output": 15}}}}
    }"#;
    let s1 = sync_to(&dir, "a.json", body_acme_first);
    let s2 = sync_to(&dir, "b.json", body_beta_first);
    let (p1, _) = Pricing::load(None, Some(&s1), None);
    let (p2, _) = Pricing::load(None, Some(&s2), None);
    assert_eq!(
        p1.display_name_for("opus55"),
        p2.display_name_for("opus55"),
        "同一等价 ID 下的展示名必须与条目顺序无关"
    );
    assert_eq!(
        p1.display_name_for("opus55").as_deref(),
        Some("Acme Name"),
        "按 (display, name) 稳定序择一"
    );
}

#[test]
fn model_display_name_requires_full_id_match() {
    let dir = tmp("full-id");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("Opus_5.5").as_deref(),
        Some("Opus 5.5"),
        "等价 ID 命中"
    );
    assert!(
        p.display_name_for("opus5.5-20261010").is_none(),
        "仅前缀命中的旧版本不得冒充本模型的展示名"
    );
    assert!(
        p.estimate("opus5.5-20261010", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "定价允许边界前缀回退（与展示名解析相互独立）"
    );
}

#[test]
fn model_display_name_legacy_snapshot_falls_back() {
    let dir = tmp("legacy");
    let snap = dir.join("pricing-modelsdev.json");
    // v3 旧快照：name 可能是供应商名称（旧同步把 provider name 兜底进来）。
    std::fs::write(
        &snap,
        r#"{"v":3,"synced_at":"t","entries":[{"id":"acme/opus-5.5","name":"Acme Provider","input":3.0,"output":15.0}]}"#,
    )
    .unwrap();
    let s = load_snapshot(&snap).unwrap().unwrap();
    assert_eq!(s.entries[0].name, None, "来源不明的 name 不得保留");
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert!(
        p.display_name_for("opus55").is_none(),
        "无可信 name → 聚合退回原始代表写法"
    );
    assert!(
        p.estimate("opus55", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "旧快照仍可离线提供价格"
    );
}
