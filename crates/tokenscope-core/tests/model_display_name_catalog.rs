//! 展示名独立目录：裸 ID 多渠道、无价有名与离线兼容的回归测试。
//!
//! 只用合成 JSON 与公开 models.dev 的规范名称；本机历史/缓存不进入测试。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::model::TokenCounts;
use tokenscope::modelsdev::sync_with_body_for_tests;
use tokenscope::pricing::{DisplayNameReason, Pricing};

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-dn-catalog-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn sync(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("modelsdev.json");
    sync_with_body_for_tests(&path, body).unwrap();
    path
}

/// DN-C01：本机形状——同末段有几十个带渠道前缀的完整 ID，全部有价有名。
/// 旧实现在 `unique_modelsdev_identity` 处因多身份直接返回 None，裸 ID 显示原文。
#[test]
fn bare_id_resolves_when_many_channels_have_priced_names() {
    let dir = tmp("many-channels");
    let mut providers = String::new();
    for i in 0..52 {
        providers.push_str(&format!(
            "\"channel-{i:02}\":{{\"models\":{{\"claude-opus-4-8\":{{\"name\":\"Claude Opus 4.8\",\"cost\":{{\"input\":3,\"output\":15}}}}}}}},"
        ));
    }
    providers.push_str(
        r#""anthropic":{"models":{"claude-opus-4-8":{"name":"Claude Opus 4.8","cost":{"input":3,"output":15}}}}"#,
    );
    let snap = sync(&dir, &format!("{{{providers}}}"));
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("claude-opus-4-8").as_deref(),
        Some("Claude Opus 4.8")
    );
    assert_eq!(
        p.display_name_for("anthropic/claude-opus-4-8").as_deref(),
        Some("Claude Opus 4.8")
    );
}

/// DN-C02：合成目录只有真实身份关系（canonical_model_id）能证明多个渠道属于
/// 同一模型；渠道数量、字典序与名称相似都不参与判定。
#[test]
fn bare_id_uses_verified_canonical_relation_not_channel_count() {
    let dir = tmp("canonical-relation");
    let snap = sync(
        &dir,
        r#"{
            "zzz-channel":{"models":{"demo-multi-1":{"name":"Demo Multi One","cost":{"input":1,"output":2}}}},
            "aaa-channel":{"models":{"demo-multi-1":{"name":"Channel Alias","cost":{"input":1,"output":2}}}},
            "publisher":{"models":{"demo-multi-1":{
                "name":"Demo Multi One",
                "canonical_model_id":"publisher/demo-multi-1",
                "cost":{"input":1,"output":2}
            }}}
        }"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("demo-multi-1").as_deref(),
        Some("Demo Multi One")
    );
    assert_eq!(
        p.display_name_for("publisher/demo-multi-1").as_deref(),
        Some("Demo Multi One")
    );
    // 没有身份关系的渠道按自身完整身份取自己的名称，不借发布方名称。
    assert_eq!(
        p.display_name_for("aaa-channel/demo-multi-1").as_deref(),
        Some("Channel Alias")
    );
}

/// DN-C03：上游无 cost 的条目仍保留模型级 name，且不进入报价候选。
#[test]
fn no_cost_catalog_entry_keeps_name_without_pricing_candidate() {
    let dir = tmp("no-cost");
    let snap = sync(
        &dir,
        r#"{"publisher":{"models":{"no-cost-model":{"name":"No Cost Model"}}}}"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("no-cost-model").as_deref(),
        Some("No Cost Model")
    );
    assert_eq!(p.external_count(), 0);
    let counts = TokenCounts {
        input: 1000,
        output: 0,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let at: jiff::Timestamp = "2026-11-24T00:00:00Z".parse().unwrap();
    let estimate = p.estimate("no-cost-model", &counts, at);
    assert!(estimate.is_none(), "无价名称不得变成计价候选");
}

/// DN-C04：目录乱序不改变结果；无确认关系的同末段异模按各自身份取名，
/// 裸 ID 保留原文。
#[test]
fn shuffled_catalog_and_same_leaf_different_publishers() {
    let dir = tmp("shuffle");
    let snap = sync(
        &dir,
        r#"{
            "author-a":{"models":{"demo-x":{"name":"Demo A One","cost":{"input":1,"output":2}}}},
            "author-b":{"models":{"demo-x":{"name":"Demo B Two","cost":{"input":1,"output":2}}}}
        }"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(p.display_name_for("demo-x"), None);
    assert_eq!(
        p.display_name_for("author-a/demo-x").as_deref(),
        Some("Demo A One")
    );
    assert_eq!(
        p.display_name_for("author-b/demo-x").as_deref(),
        Some("Demo B Two")
    );

    // 把快照 entries 倒序后重写，结果必须一致。
    let reversed = dir.join("reversed.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&snap).unwrap()).unwrap();
    if let Some(entries) = value.get_mut("entries").and_then(|v| v.as_array_mut()) {
        entries.reverse();
    }
    std::fs::write(&reversed, serde_json::to_vec(&value).unwrap()).unwrap();
    let (p2, _) = Pricing::load(None, Some(&reversed), None);
    assert_eq!(p2.display_name_for("demo-x"), None);
    assert_eq!(
        p2.display_name_for("author-a/demo-x").as_deref(),
        Some("Demo A One")
    );
    assert_eq!(
        p2.display_name_for("author-b/demo-x").as_deref(),
        Some("Demo B Two")
    );
}

/// DN-C05：公开规范目录中带日期后缀与不带后缀的型号分别取名，不互相借用；
/// Codex Spark / Flash / Pro 变体保留。
#[test]
fn public_catalog_keeps_date_and_variant_names() {
    let (p, _) = Pricing::load(None, None, None);
    let cases = [
        ("deepseek-v4-flash", "DeepSeek V4 Flash"),
        ("deepseek-v4-flash-0731", "DeepSeek V4 Flash 0731"),
        ("deepseek-v4.1-flash", "DeepSeek V4.1 Flash"),
        ("deepseek-v4.1-flash-1017", "DeepSeek V4.1 Flash 1017"),
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
        ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
    ];
    for (id, name) in cases {
        assert_eq!(p.display_name_for(id).as_deref(), Some(name), "{id}");
    }
    // 真正无公开名称的 ID 仍保留原文。
    assert_eq!(p.display_name_for("gpt-6-astra-cc-format"), None);
}

/// DN-C06：v0.9.8 旧目录缓存（无 canonical_model_id、只有带价条目）在离线时
/// 仍能由公开规范名称解析 11 个历史裸 ID。
#[test]
fn v098_snapshot_without_canonical_still_resolves_offline_names() {
    let dir = tmp("v098-offline");
    let ids = [
        "gpt-5.3-codex-spark",
        "claude-opus-4-8",
        "claude-opus-5-5",
        "deepseek-v4.1-flash",
        "deepseek-v4.1-flash-1017",
        "deepseek-v4-flash",
        "deepseek-v4-flash-0731",
        "deepseek-v4-pro",
        "doubao-seed-2-0-pro-260215",
        "glm-5.2",
        "glm-5.3",
    ];
    let mut body = String::from("{");
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            body.push(',');
        }
        body.push_str(&format!(
            "\"relay-{i:02}\":{{\"models\":{{\"{id}\":{{\"name\":\"{id}\",\"cost\":{{\"input\":1,\"output\":2}}}}}}}}"
        ));
    }
    body.push('}');
    let snap = sync(&dir, &body);
    let (p, _) = Pricing::load(None, Some(&snap), None);
    let expected = [
        "GPT-5.3 Codex Spark",
        "Claude Opus 4.8",
        "Claude Opus 5.5",
        "DeepSeek V4.1 Flash",
        "DeepSeek V4.1 Flash 1017",
        "DeepSeek V4 Flash",
        "DeepSeek V4 Flash 0731",
        "DeepSeek V4 Pro",
        "Seed 2.0 Pro",
        "GLM-5.2",
        "GLM-5.3",
    ];
    for (id, name) in ids.iter().zip(expected) {
        assert_eq!(p.display_name_for(id).as_deref(), Some(name), "{id}");
    }
}

/// DN-C07：未命中时给出结构化原因，便于全量审计逐条解释回退。
#[test]
fn unresolved_names_record_specific_reasons() {
    let dir = tmp("reasons");
    let snap = sync(
        &dir,
        r#"{
            "alpha":{"models":{"demo-conflict":{
                "name":"Alpha Name","canonical_model_id":"alpha/demo-conflict",
                "cost":{"input":1,"output":2}}}},
            "beta":{"models":{"demo-conflict":{
                "name":"Beta Name","canonical_model_id":"beta/demo-conflict",
                "cost":{"input":1,"output":2}}}},
            "gamma":{"models":{"demo-unnamed":{"cost":{"input":1,"output":2}}}}
        }"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    let conflict = p.display_name_resolution_for("demo-conflict");
    assert_eq!(conflict.name, None);
    assert_eq!(conflict.reason, DisplayNameReason::AmbiguousIdentity);
    let unnamed = p.display_name_resolution_for("gamma/demo-unnamed");
    assert_eq!(unnamed.name, None);
    assert_eq!(unnamed.reason, DisplayNameReason::NoTrustedName);

    let (p2, _) = Pricing::load(None, None, None);
    let missing = p2.display_name_resolution_for("totally-unknown-model");
    assert_eq!(missing.name, None);
    assert_eq!(missing.reason, DisplayNameReason::NoIdentityEvidence);
}

/// DN-C08：公开规范目录解析时记录规范完整 ID 与数据来源。
#[test]
fn catalog_resolution_records_canonical_identity() {
    let (p, _) = Pricing::load(None, None, None);
    let r = p.display_name_resolution_for("doubao-seed-2-0-pro-260215");
    assert_eq!(r.name.as_deref(), Some("Seed 2.0 Pro"));
    assert_eq!(r.reason, DisplayNameReason::ResolvedCatalog);
    assert_eq!(r.source, "models.json");
    assert_eq!(
        r.canonical_id.as_deref(),
        Some("bytedance-seed/seed-2.0-pro")
    );
}

/// DN-C09：完整 ID 自身在规范目录中有正式名时，渠道别名不得覆盖它；对应裸
/// ID 仍按规范末段落位，日期变体与基础版各用各的名字。
#[test]
fn full_id_own_catalog_name_beats_channel_alias() {
    let (p, _) = Pricing::load(None, None, None);
    let cases = [
        ("deepseek/deepseek-v4-flash", "DeepSeek V4 Flash"),
        ("deepseek/deepseek-v4-pro", "DeepSeek V4 Pro"),
        (
            "deepseek/deepseek-v4-flash-vision-exp",
            "DeepSeek V4 Flash Vision Exp",
        ),
        (
            "moonshotai/kimi-k2.7-code-highspeed",
            "Kimi K2.7 Code Highspeed",
        ),
        ("deepseek-v4-flash", "DeepSeek V4 Flash"),
        ("deepseek-v4-pro", "DeepSeek V4 Pro"),
        (
            "deepseek-v4-flash-vision-exp",
            "DeepSeek V4 Flash Vision Exp",
        ),
        ("kimi-k2.7-code-highspeed", "Kimi K2.7 Code Highspeed"),
        ("deepseek/deepseek-v4-flash-0731", "DeepSeek V4 Flash 0731"),
        ("deepseek/deepseek-v4-pro-0813", "DeepSeek V4 Pro 0813"),
        (
            "deepseek/deepseek-v4.1-flash-1017",
            "DeepSeek V4.1 Flash 1017",
        ),
        ("deepseek-v4-flash-0731", "DeepSeek V4 Flash 0731"),
        ("deepseek-v4-pro-0813", "DeepSeek V4 Pro 0813"),
        ("deepseek-v4.1-flash-1017", "DeepSeek V4.1 Flash 1017"),
    ];
    for (id, name) in cases {
        let r = p.display_name_resolution_for(id);
        assert_eq!(r.name.as_deref(), Some(name), "{id}: {r:?}");
        assert_eq!(r.reason, DisplayNameReason::ResolvedCatalog, "{id}");
        assert_eq!(r.source, "models.json", "{id}");
    }
    // 无自身规范名的渠道仍可按显式别名取名。
    assert_eq!(
        p.display_name_for("opencode/deepseek-v4-flash").as_deref(),
        Some("DeepSeek V4 Flash 0731")
    );
}

/// DN-C10：重新生成的随包目录保留末段相同的显式别名，完整 ID 与裸 ID 都能
/// 离线使用这条关系。
#[test]
fn generated_catalog_same_leaf_alias_resolves_offline() {
    let (p, _) = Pricing::load(None, None, None);
    let r = p.display_name_resolution_for("302ai/MiniMax-M2");
    assert_eq!(r.name.as_deref(), Some("MiniMax-M2"), "{r:?}");
    assert_eq!(r.canonical_id.as_deref(), Some("minimax/MiniMax-M2"));
    assert_eq!(r.reason, DisplayNameReason::ResolvedCatalog);
    assert_eq!(
        p.display_name_for("minimax-m2").as_deref(),
        Some("MiniMax-M2")
    );
}

/// DN-C11：快照里的 canonical 关系也不得覆盖完整 ID 自身的规范名；没有自身
/// 规范名的渠道条目仍按该关系取名。
#[test]
fn snapshot_canonical_claim_does_not_override_own_catalog_name() {
    let dir = tmp("claim-vs-own");
    let snap = sync(
        &dir,
        r#"{
            "zhipuai":{"models":{
                "glm-5.2":{
                    "name":"GLM-5.2 0901",
                    "canonical_model_id":"zhipuai/glm-5.2-0901",
                    "cost":{"input":1,"output":2}
                },
                "glm-5.2-0901":{
                    "name":"GLM-5.2 0901",
                    "cost":{"input":1,"output":2}
                }
            }},
            "relay":{"models":{"demo-pro-9":{
                "name":"Relay Demo Pro",
                "canonical_model_id":"publisher/demo-pro-9",
                "cost":{"input":1,"output":2}
            }}},
            "publisher":{"models":{"demo-pro-9":{
                "name":"Demo Pro Nine",
                "cost":{"input":1,"output":2}
            }}}
        }"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    let own = p.display_name_resolution_for("zhipuai/glm-5.2");
    assert_eq!(own.name.as_deref(), Some("GLM-5.2"), "{own:?}");
    assert_eq!(own.canonical_id.as_deref(), Some("zhipuai/glm-5.2"));
    // 没有自身规范名的渠道仍使用快照的显式关系。
    assert_eq!(
        p.display_name_for("relay/demo-pro-9").as_deref(),
        Some("Demo Pro Nine")
    );
}
