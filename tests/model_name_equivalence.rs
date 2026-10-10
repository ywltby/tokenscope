//! MP02：模型名称等价——候选集合、完整/前缀优先、变体隔离、冲突与顺序无关。
//!
//! 计划：docs/plans/active/2026-10-10-model-pricing-name-equivalence.md §2/§3。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::model::TokenCounts;
use tokenscope::pricing::{INDEX_VERSION, MatchMode, Pricing, clear_price_cache_for_tests};

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("tokenscope-mp02-{tag}-{}-{n}", std::process::id()));
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

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

/// 只有外置表的最小来源组合（隔离：不读本机任何价格文件）。
fn external_only(dir: &Path, name: &str, body: &str) -> Pricing {
    let p = write(dir, name, body);
    let (pricing, _warn) = Pricing::load(Some(&p), None, None);
    pricing
}

#[test]
fn model_equivalence_three_spellings_same_candidates_and_cost() {
    let dir = tmp("three");
    let spellings = ["claude-opus-5-5", "claude-opus-5.5", "Claude_Opus_5_5"];
    let c = counts(1_000_000, 0, 0, 0);
    let mut costs: Vec<f64> = Vec::new();
    for (ei, entry) in spellings.iter().enumerate() {
        let body = format!(
            "[[model]]\nprefix = \"{entry}\"\ninput = 3.0\noutput = 15.0\ncache_write = 3.75\ncache_read = 0.3\n"
        );
        let p = external_only(&dir, &format!("ext-{ei}.toml"), &body);
        for query in spellings {
            let est = p
                .estimate(query, &c, at())
                .unwrap_or_else(|| panic!("价格表 {entry} 对查询 {query} 未命中"));
            let m = est.matched.clone().unwrap();
            assert_eq!(m.match_mode, MatchMode::Full, "{entry} vs {query}");
            assert!(est.complete, "{entry} vs {query}");
            assert_eq!(est.unknown.total(), 0, "{entry} vs {query}");
            assert!(
                (est.cost - 3.0).abs() < 1e-9,
                "{entry} vs {query}: {}",
                est.cost
            );
            costs.push(est.cost);
        }
    }
    assert_eq!(costs.len(), 9, "3×3 交叉组合全覆盖");
    assert!(costs.iter().all(|c| (c - 3.0).abs() < 1e-9), "{costs:?}");
}

#[test]
fn model_equivalence_full_precedes_prefix() {
    let dir = tmp("full");
    // 前缀条目更便宜、完整条目更贵：完整命中即返回，不回退比较。
    let body = "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 1.0\n\n\
                [[model]]\nprefix = \"vendorB/opus-5.5-20261010\"\ninput = 9.0\n";
    let p = external_only(&dir, "ext.toml", body);
    let est = p
        .estimate("opus5.5-20261010", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    let m = est.matched.clone().unwrap();
    assert_eq!(m.match_mode, MatchMode::Full);
    assert!(
        (est.cost - 9.0).abs() < 1e-9,
        "完整等价匹配优先于更便宜的前缀条目: {}",
        est.cost
    );
}

#[test]
fn model_equivalence_prefix_keeps_boundary() {
    let dir = tmp("boundary");
    let body = "[[model]]\nprefix = \"gpt-5\"\ninput = 1.0\n\n\
                [[model]]\nprefix = \"gpt-50\"\ninput = 9.0\n";
    let p = external_only(&dir, "ext.toml", body);

    // 等价完整命中：`gpt-50` 与 `gpt50`、`gpt_5.0` 同为 `gpt50`。
    let est = p
        .estimate("gpt-50", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert_eq!(est.matched.clone().unwrap().match_mode, MatchMode::Full);
    assert!((est.cost - 9.0).abs() < 1e-9);

    // 带日期后缀：只在分隔符边界截断 → 命中 `gpt-50`（不是 `gpt-5`）。
    let est = p
        .estimate("gpt-50-20260101", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert_eq!(est.matched.clone().unwrap().match_mode, MatchMode::Prefix);
    assert!((est.cost - 9.0).abs() < 1e-9, "{}", est.cost);

    // 无分隔符边界：不得猜测后缀边界。
    assert!(
        p.estimate("gpt-500", &counts(1_000_000, 0, 0, 0), at())
            .is_none(),
        "gpt-500 不得命中 gpt-50"
    );
    assert!(
        p.estimate("gpt5x", &counts(1_000_000, 0, 0, 0), at())
            .is_none(),
        "gpt5x 不得命中 gpt5"
    );
    // `opus550` 类：无边界，不得命中更短的 `opus55`。
    let dir2 = tmp("boundary2");
    let p2 = external_only(
        &dir2,
        "ext.toml",
        "[[model]]\nprefix = \"opus-5.5\"\ninput = 3.0\n",
    );
    assert!(
        p2.estimate("opus550", &counts(1_000_000, 0, 0, 0), at())
            .is_none()
    );
    assert!(
        p2.estimate("opus-5.5-20260101", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "有分隔符边界时可回退"
    );
}

#[test]
fn model_equivalence_variant_isolation() {
    let dir = tmp("variant");
    let body = "[[model]]\nprefix = \"opus-5.5\"\ninput = 3.0\n\n\
                [[model]]\nprefix = \"opus-5.5:free\"\ninput = 0.0\n\n\
                [[model]]\nprefix = \"opus-5.5:thinking\"\ninput = 9.0\n";
    let p = external_only(&dir, "ext.toml", body);

    let base = p
        .estimate("opus55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert_eq!(base.matched.clone().unwrap().match_mode, MatchMode::Full);
    assert!((base.cost - 3.0).abs() < 1e-9, "基础查询不串用变体价");

    let free = p
        .estimate("opus5-5:free", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert_eq!(free.matched.clone().unwrap().match_mode, MatchMode::Full);
    assert!((free.cost - 0.0).abs() < 1e-9, "显式 0 = 免费，不是未知");
    assert!(free.complete);

    let thinking = p
        .estimate("Opus_5.5:THINKING", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert!((thinking.cost - 9.0).abs() < 1e-9, "变体大小写折叠但不合并");

    // 未知变体 → 只能走显式标记的基名回退。
    let unknown = p
        .estimate("opus55:mystery", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert_eq!(
        unknown.matched.clone().unwrap().match_mode,
        MatchMode::FullVariantFallback
    );
    assert!((unknown.cost - 3.0).abs() < 1e-9);
}

#[test]
fn model_equivalence_collisions_keep_all_candidates() {
    let dir = tmp("collide");
    let ext = write(
        &dir,
        "ext.toml",
        "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 3.0\n",
    );
    let md = write(
        &dir,
        "modelsdev.json",
        r#"{"v":3,"synced_at":"t","entries":[{"id":"vendorB/opus5_5","name":null,"input":4.0,"output":40.0}]}"#,
    );
    let orp = write(
        &dir,
        "openrouter.json",
        // OpenRouter 快照价格单位是「每 token」，5e-6 → 5.0 USD / 1M。
        r#"{"v":2,"synced_at":"t","entries":[{"id":"vendorC/opus55","name":"Opus 55","prompt":0.000005,"completion":0.00005}]}"#,
    );
    let (p, _warn) = Pricing::load(Some(&ext), Some(&md), Some(&orp));

    let est = p
        .estimate("Opus.55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    let m = est.matched.clone().unwrap();
    assert_eq!(m.candidate_count, 3, "三条等价条目全部进入候选");
    assert!(
        (est.cost - 5.0).abs() < 1e-9,
        "按最高费用保守估算: {}",
        est.cost
    );
    assert_eq!(m.source, "openrouter");
    assert_eq!(m.channel.as_deref(), Some("vendorC"));
}

#[test]
fn model_equivalence_candidate_order_independent() {
    let dir = tmp("order");
    let a = "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 2.0\n\n\
             [[model]]\nprefix = \"vendorB/opus_5_5\"\ninput = 2.0\n";
    let b = "[[model]]\nprefix = \"vendorB/opus_5_5\"\ninput = 2.0\n\n\
             [[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 2.0\n";
    let p1 = external_only(&dir, "a.toml", a);
    let p2 = external_only(&dir, "b.toml", b);
    let m1 = p1
        .estimate("opus55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap()
        .matched
        .unwrap();
    let m2 = p2
        .estimate("opus55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap()
        .matched
        .unwrap();
    assert_eq!(m1.candidate_count, 2);
    assert_eq!(m1.candidate_count, m2.candidate_count);
    assert_eq!(m1.raw_key, m2.raw_key, "并列裁决必须与条目顺序无关");
}

#[test]
fn model_equivalence_policy_respects_channel() {
    let dir = tmp("policy");
    // 策略按等价键命中 vendorA；cache_read 未知 → 沿用输入价。
    let body = "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 3.0\n\n\
                [[model_policy]]\nprefix = \"opus5-5\"\nchannel = \"vendorA\"\ncache_read = \"same_as_input\"\n";
    let p = external_only(&dir, "ext.toml", body);
    let est = p
        .estimate("opus5.5", &counts(0, 0, 0, 1_000_000), at())
        .unwrap();
    assert!(est.complete, "策略声明沿用输入价后应完整计价");
    assert!((est.cost - 3.0).abs() < 1e-9, "{}", est.cost);

    // 渠道限定不匹配 → 策略不生效（cache_read 仍未知，进 unknown）。
    let dir2 = tmp("policy2");
    let body2 = "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 3.0\n\n\
                 [[model_policy]]\nprefix = \"opus5-5\"\nchannel = \"vendorB\"\ncache_read = \"same_as_input\"\n";
    let p2 = external_only(&dir2, "ext.toml", body2);
    let est = p2
        .estimate("opus5.5", &counts(0, 0, 0, 1_000_000), at())
        .unwrap();
    assert!(!est.complete, "渠道不匹配时策略不得生效");
    assert_eq!(est.unknown.cache_read, 1_000_000);

    // 显式价格不被政策覆盖。
    let dir3 = tmp("policy3");
    let body3 = "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 3.0\ncache_read = 0.3\n\n\
                 [[model_policy]]\nprefix = \"opus5-5\"\nchannel = \"vendorA\"\ncache_read = \"same_as_input\"\n";
    let p3 = external_only(&dir3, "ext.toml", body3);
    let est = p3
        .estimate("opus5.5", &counts(0, 0, 0, 1_000_000), at())
        .unwrap();
    assert!(
        (est.cost - 0.3).abs() < 1e-9,
        "显式 cache_read 覆盖策略: {}",
        est.cost
    );
}

#[test]
fn model_equivalence_price_view_comparison_is_deterministic() {
    let dir = tmp("view");
    let ext = write(
        &dir,
        "ext.toml",
        "[[model]]\nprefix = \"vendorE/opus-55\"\ninput = 3.0\n",
    );
    let or_a = write(
        &dir,
        "or-a.json",
        r#"{"v":2,"synced_at":"t","entries":[
            {"id":"vendorB/opus-5.5","name":"B Name","prompt":4.0,"completion":40.0},
            {"id":"vendorA/opus_5_5","name":"A Name","prompt":5.0,"completion":50.0}]}"#,
    );
    let or_b = write(
        &dir,
        "or-b.json",
        r#"{"v":2,"synced_at":"t","entries":[
            {"id":"vendorA/opus_5_5","name":"A Name","prompt":5.0,"completion":50.0},
            {"id":"vendorB/opus-5.5","name":"B Name","prompt":4.0,"completion":40.0}]}"#,
    );
    let (p1, _) = Pricing::load(Some(&ext), None, Some(&or_a));
    let (p2, _) = Pricing::load(Some(&ext), None, Some(&or_b));
    let pick = |p: &Pricing| {
        p.entries()
            .iter()
            .find(|e| e.prefix.contains("opus"))
            .and_then(|e| e.openrouter.as_ref())
            .map(|o| (o.name.clone(), o.input))
    };
    assert_eq!(
        pick(&p1),
        pick(&p2),
        "同前缀多条 OpenRouter 对照价必须与加载顺序无关"
    );
    assert_eq!(
        pick(&p1).and_then(|(n, _)| n).as_deref(),
        Some("A Name"),
        "按 display 稳定择一并对应该候选的原名称"
    );
}

// ---- MP05：索引重建、离线恢复与价格修订 ----

#[test]
fn model_equivalence_old_index_rebuilds_offline() {
    let dir = tmp("index-old");
    let ext = write(
        &dir,
        "ext.toml",
        "[[model]]\nprefix = \"vendorA/gpt-5.6\"\ninput = 4.0\n",
    );
    let idx = dir.join("idx.json");
    clear_price_cache_for_tests();
    let (cold, _rev, _w, _hit) = Pricing::load_cached_revision(Some(&ext), None, None, &idx);
    assert!(
        cold.estimate("gpt-5.6-sol", &counts(1_000_000, 0, 0, 0), at())
            .is_some()
    );

    // 把索引改造成旧版本（v8，匹配键是旧规则 `gpt-5-6`）——来源字节不变。
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&idx).unwrap()).unwrap();
    json["v"] = serde_json::json!(8);
    json["entries"][0]["prefix"] = serde_json::json!("gpt-5-6");
    std::fs::write(&idx, serde_json::to_string(&json).unwrap()).unwrap();

    clear_price_cache_for_tests();
    let (rebuilt, rev, _w2, _hit2) = Pricing::load_cached_revision(Some(&ext), None, None, &idx);
    assert!(
        rebuilt
            .estimate("gpt-5.6-sol", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "旧索引必须被拒绝并按本地来源离线重建（不联网）"
    );
    // 索引已写回为当前版本，且键是等价形式、与保留的 display 一致。
    let after: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&idx).unwrap()).unwrap();
    assert_eq!(after["v"].as_u64(), Some(u64::from(INDEX_VERSION)));
    assert_eq!(after["entries"][0]["prefix"].as_str(), Some("gpt56"));
    assert!(rev.contains(&format!("rules:{INDEX_VERSION}")), "{rev}");
}

#[test]
fn empty_model_key_entry_is_rejected_with_diagnostic() {
    let dir = tmp("empty-key");
    let ext = write(
        &dir,
        "ext.toml",
        "[[model]]\nprefix = \"provider/-._\"\ninput = 3.0\n\n\
         [[model]]\nprefix = \"provider/ok-1\"\ninput = 1.0\n",
    );
    let (p, warnings) = Pricing::load(Some(&ext), None, None);
    assert!(
        p.estimate("provider/-._", &counts(1_000_000, 0, 0, 0), at())
            .is_none(),
        "归一后为空的模型键不得进入价格表"
    );
    assert!(
        p.estimate("provider/ok-1", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "同文件里的正常条目不受影响"
    );
    assert!(
        warnings.iter().any(|w| w.contains("归一后为空")),
        "拒绝必须给出诊断: {warnings:?}"
    );
}

#[test]
fn model_equivalence_restart_matches_cold_build() {
    let dir = tmp("restart");
    let ext = write(
        &dir,
        "ext.toml",
        "[[model]]\nprefix = \"vendorA/claude-opus-5.5\"\ninput = 3.0\noutput = 15.0\n",
    );
    let idx = dir.join("idx.json");
    clear_price_cache_for_tests();
    let (cold, rev_cold, _w, hit_cold) =
        Pricing::load_cached_revision(Some(&ext), None, None, &idx);
    assert!(!hit_cold, "首次加载没有索引可命中（冷建）");
    let cold_cost = cold
        .estimate("claude-opus-5-5", &counts(1_000_000, 0, 0, 0), at())
        .unwrap()
        .cost;
    clear_price_cache_for_tests();
    let (warm, rev_warm, _w2, hit_warm) =
        Pricing::load_cached_revision(Some(&ext), None, None, &idx);
    let warm_cost = warm
        .estimate("claude-opus-5.5", &counts(1_000_000, 0, 0, 0), at())
        .unwrap()
        .cost;
    assert!(
        hit_warm,
        "清空进程缓存后必须命中磁盘索引——否则只是再次重建，无法证明恢复路径"
    );
    assert_eq!(cold_cost, warm_cost, "索引恢复与冷建必须给同一金额");
    assert_eq!(rev_cold, rev_warm, "同来源同规则 → 同价格修订");
}

#[test]
fn model_equivalence_revision_changes_with_rules() {
    let dir = tmp("revision");
    let ext = write(
        &dir,
        "ext.toml",
        "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 3.0\n",
    );
    clear_price_cache_for_tests();
    let (_, rev, _w, _hit) =
        Pricing::load_cached_revision(Some(&ext), None, None, &dir.join("idx.json"));
    assert!(
        rev.contains(&format!("rules:{INDEX_VERSION}")),
        "价格修订必须携带匹配规则版本：{rev}"
    );
    // 来源内容变化 → 修订变化（前端快照据此拒绝旧金额）。
    let ext2 = write(
        &dir,
        "ext2.toml",
        "[[model]]\nprefix = \"vendorA/opus-5.5\"\ninput = 3.5\n",
    );
    clear_price_cache_for_tests();
    let (_, rev2, _w2, _hit2) =
        Pricing::load_cached_revision(Some(&ext2), None, None, &dir.join("idx2.json"));
    assert_ne!(rev, rev2);
}
