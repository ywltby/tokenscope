//! 展示名独立目录：裸 ID 多渠道、无价有名与离线兼容的回归测试。
//!
//! 只用合成 JSON 与公开 models.dev 的规范名称；本机历史/缓存不进入测试。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::aggregate::{GroupBy, aggregate, resolve_tz};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
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

/// DN-C12：统计层接受同末段显式 canonical 关系，但该映射不得覆盖完整 ID
/// 自身的规范名；两侧都有报价，确保不是无价名称路径掩盖问题。
#[test]
fn same_leaf_stats_mapping_does_not_override_own_catalog_name() {
    let dir = tmp("same-leaf-stats");
    let snap = sync(
        &dir,
        r#"{
            "deepseek":{"models":{"deepseek-v4-flash":{
                "name":"Snapshot Channel Flash",
                "canonical_model_id":"sample-lab/deepseek-v4-flash",
                "cost":{"input":1,"output":2}
            }}},
            "sample-lab":{"models":{"deepseek-v4-flash":{
                "name":"Sample Lab Flash",
                "cost":{"input":3,"output":4}
            }}}
        }"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.model_stats_key("deepseek/deepseek-v4-flash"),
        "samplelab/deepseekv4flash"
    );
    let own = p.display_name_resolution_for("deepseek/deepseek-v4-flash");
    assert_eq!(own.name.as_deref(), Some("DeepSeek V4 Flash"), "{own:?}");
    assert_eq!(
        own.canonical_id.as_deref(),
        Some("deepseek/deepseek-v4-flash"),
        "{own:?}"
    );
    assert_eq!(own.reason, DisplayNameReason::ResolvedCatalog);
    assert_eq!(
        p.display_name_for("sample-lab/deepseek-v4-flash")
            .as_deref(),
        Some("Sample Lab Flash")
    );
}

/// DN-C15：上游退休 provider 与 v2_only 规范模型不应让已有裸 ID 丢名；
/// 生成器只补有提交证据的历史身份，当前上游改指向仍优先。
#[test]
fn retired_catalog_identities_keep_verified_offline_names() {
    let (p, _) = Pricing::load(None, None, None);
    let cases = [
        ("Qwen2.5-7B-Instruct-Turbo", "Qwen2.5 7B Instruct"),
        (
            "Qwen3-235B-A22B-Instruct-2507-tput",
            "Qwen3 235B-A22B Instruct 2507",
        ),
        ("databricks-claude-sonnet-4", "Claude Sonnet 4"),
        ("databricks-gemini-2-5-flash", "Gemini 2.5 Flash"),
        ("databricks-gemini-2-5-pro", "Gemini 2.5 Pro"),
        ("databricks-gemini-3-pro", "Gemini 3 Pro Preview"),
        ("databricks-kimi-k2-7-code", "Kimi K2.7 Code"),
        ("doubao-seed-1-6-251015", "Seed 1.6"),
        ("doubao-seed-1-6-flash-250828", "Seed 1.6 Flash"),
        ("doubao-seed-1-8-251228", "Seed 1.8"),
        ("mimo-v2-tts", "MiMo-V2-TTS"),
        ("mimo-v2.5-tts", "MiMo-V2.5-TTS"),
        ("mimo-v2.5-tts-voiceclone", "MiMo-V2.5-TTS-VoiceClone"),
        ("mimo-v2.5-tts-voicedesign", "MiMo-V2.5-TTS-VoiceDesign"),
        ("gemini-4-argon@us", "Gemini 4 Argon"),
        ("gemma327b", "Gemma 3 27B IT"),
    ];
    for (id, name) in cases {
        let r = p.display_name_resolution_for(id);
        assert_eq!(r.name.as_deref(), Some(name), "{id}: {r:?}");
        assert_eq!(r.reason, DisplayNameReason::ResolvedCatalog, "{id}");
    }
    let conflict = p.display_name_resolution_for("gemini-pro-latest");
    assert_eq!(conflict.name, None, "{conflict:?}");
    assert_eq!(conflict.reason, DisplayNameReason::AmbiguousIdentity);
    assert_eq!(
        p.display_name_for("databricks/databricks-gemini-3-1-flash-lite")
            .as_deref(),
        Some("Gemini 3.1 Flash Lite"),
        "当前上游改指向不得被历史层覆盖"
    );
    assert_eq!(
        p.display_name_for("fireworks-ai/accounts/fireworks/routers/kimi-latest")
            .as_deref(),
        Some("Kimi K3.1 Pro")
    );
    // 完整 ID 等价自指别名（如 `.` vs `-` 或大小写差异）不写入别名表，仍直接命中自身规范名。
    for (id, expected) in [
        ("cohere/north-mini-code-1.0", "North Mini Code"),
        ("cohere/north-small-translate-1.0", "North Small Translate"),
        ("reflection/Beam-1.0-501B-A23B", "Beam 1.0 501B A23B"),
    ] {
        let r = p.display_name_resolution_for(id);
        assert_eq!(r.name.as_deref(), Some(expected), "{id}: {r:?}");
        assert_eq!(r.reason, DisplayNameReason::ResolvedCatalog, "{id}");
    }
}

fn synthetic_event(model: &str) -> UsageEvent {
    UsageEvent {
        ts: "2026-11-25T10:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: model.to_string(),
        session_id: "sess-display".to_string(),
        project: "/demo".to_string(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: format!("rec-{model}"),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 0,
        source_path: String::new(),
        input_tokens: 100,
        output_tokens: 50,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

/// DN-C16：弱证据历史条目（Fireworks `kimi-k3-spot`，上游标记 `upstream_assumed`）
/// 只留档、不进入可用别名；无其他可信证据时裸 ID 与完整 ID 均保留原始 ID，
/// 若现行目录日后提供独立有效映射仍按现行规则解析。
#[test]
fn weak_evidence_kimi_spot_keeps_raw_id_until_current_catalog_verifies() {
    let (offline, _) = Pricing::load(None, None, None);
    let full_id = "fireworks-ai/accounts/fireworks/routers/kimi-k3-spot";

    let bare_res = offline.display_name_resolution_for("kimi-k3-spot");
    assert_eq!(
        bare_res.name, None,
        "弱证据不得用于裸 ID 取名: {bare_res:?}"
    );
    assert_eq!(bare_res.reason, DisplayNameReason::NoIdentityEvidence);

    let full_res = offline.display_name_resolution_for(full_id);
    assert_eq!(
        full_res.name, None,
        "弱证据不得用于完整 ID 取名: {full_res:?}"
    );
    assert_eq!(full_res.reason, DisplayNameReason::NoTrustedName);

    let (tz, _) = resolve_tz(None).unwrap();
    for (raw, expected_key) in [
        ("kimi-k3-spot", "kimik3spot"),
        (full_id, "fireworksai/accounts/fireworks/routers/kimik3spot"),
    ] {
        let report = aggregate(&[synthetic_event(raw)], GroupBy::Model, &tz, &offline).unwrap();
        let groups: Vec<_> = report.groups.iter().filter(|g| g.key != "合计").collect();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, expected_key, "分组键不得变化: {raw}");
        assert_eq!(
            groups[0].label.as_deref(),
            Some(raw),
            "聚合标签必须保留原始 ID: {raw}"
        );
    }

    // 现行目录日后给出独立、有效的 canonical_model_id 时，仍按现行规则解析。
    for (target, expected_name) in [
        ("moonshotai/kimi-k3", "Kimi K3"),
        ("moonshotai/kimi-k3.1-pro", "Kimi K3.1 Pro"),
    ] {
        let dir = tmp("kimi-spot-live");
        let snap = sync(
            &dir,
            &format!(
                r#"{{
                    "fireworks-ai":{{"models":{{
                        "accounts/fireworks/routers/kimi-k3-spot":{{
                            "name":"Kimi Spot",
                            "canonical_model_id":"{target}",
                            "cost":{{"input":2.1,"output":10.5}}
                        }}
                    }}}}
                }}"#
            ),
        );
        let (live, _) = Pricing::load(None, Some(&snap), None);
        assert_eq!(
            live.display_name_for(full_id).as_deref(),
            Some(expected_name),
            "现行目录独立映射应生效（完整 ID -> {target}）"
        );
        assert_eq!(
            live.display_name_for("kimi-k3-spot").as_deref(),
            Some(expected_name),
            "现行目录独立映射应生效（裸 ID -> {target}）"
        );
    }
}

/// DN-C17：现行条目仍存在但无可用 canonical 映射（缺失、自指、指向未收录模型）时，
/// 生成器不输出历史别名；离线解析与聚合绝不会显示旧目标 `Old Model`，
/// 有自身规范名时照常使用，否则保留原始 ID，且对大小写/分隔符与目录乱序不敏感。
#[test]
fn present_entry_without_usable_mapping_never_displays_old_target_offline() {
    let without_own_forward = r#"{
        "models": [
            {"id": "publisher/current", "name": "Current Model"},
            {"id": "publisher/old", "name": "Old Model"}
        ],
        "aliases": [
            {"id": "relay/retargeted-m", "canonical_id": "publisher/current"},
            {"id": "retired/gone-m", "canonical_id": "publisher/old"}
        ]
    }"#;
    let without_own_reversed = r#"{
        "aliases": [
            {"id": "retired/gone-m", "canonical_id": "publisher/old"},
            {"id": "relay/retargeted-m", "canonical_id": "publisher/current"}
        ],
        "models": [
            {"id": "publisher/old", "name": "Old Model"},
            {"id": "publisher/current", "name": "Current Model"}
        ]
    }"#;
    let (tz, _) = resolve_tz(None).unwrap();
    for json in [without_own_forward, without_own_reversed] {
        let p = Pricing::empty().with_display_catalog_json_for_tests(json);
        // 现行条目（含大小写/分隔符写法）与裸 ID 均不得显示 Old Model。
        for raw in ["relay/present-m", "Relay/Present_M", "relay/present.m"] {
            let res = p.display_name_resolution_for(raw);
            assert_eq!(res.name, None, "{raw}: {res:?}");
            assert_eq!(res.reason, DisplayNameReason::NoTrustedName, "{raw}");
            let agg = aggregate(&[synthetic_event(raw)], GroupBy::Model, &tz, &p).unwrap();
            let groups: Vec<_> = agg.groups.iter().filter(|g| g.key != "合计").collect();
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].key, "relay/presentm", "{raw}");
            assert_eq!(groups[0].label.as_deref(), Some(raw), "{raw}");
        }
        let bare = p.display_name_resolution_for("present-m");
        assert_eq!(bare.name, None, "{bare:?}");
        assert_eq!(bare.reason, DisplayNameReason::NoIdentityEvidence);
        // 真正退休的渠道条目与有效改指向仍照常解析。
        assert_eq!(
            p.display_name_for("retired/gone-m").as_deref(),
            Some("Old Model")
        );
        assert_eq!(
            p.display_name_for("relay/retargeted-m").as_deref(),
            Some("Current Model")
        );

        // 带现行快照（缺失、自指、指向未收录目标三种情形）同样不得回落到 Old Model，
        // 且计价与统计分组键保持原样。
        for (tag, snap_body) in [
            (
                "missing",
                r#"{"relay":{"models":{"present-m":{"cost":{"input":1,"output":2}}}}}"#,
            ),
            (
                "self-ref",
                r#"{"relay":{"models":{"present-m":{"canonical_model_id":"relay/present-m","cost":{"input":1,"output":2}}}}}"#,
            ),
            (
                "unlisted",
                r#"{"relay":{"models":{"present-m":{"canonical_model_id":"publisher/new","cost":{"input":1,"output":2}}}}}"#,
            ),
        ] {
            let dir = tmp(&format!("present-{tag}"));
            let snap = sync(&dir, snap_body);
            let (base_p, _) = Pricing::load(None, Some(&snap), None);
            let expected_key = base_p.model_stats_key("relay/present-m");
            let p_snap = base_p.with_display_catalog_json_for_tests(json);
            assert_eq!(
                p_snap.display_name_for("relay/present-m"),
                None,
                "{tag} 不得回落到 Old Model"
            );
            assert_eq!(
                p_snap.display_name_for("Relay/Present_M"),
                None,
                "{tag} 等价写法不得回落到 Old Model"
            );
            assert_eq!(
                p_snap.display_name_for("present-m"),
                None,
                "{tag} 裸 ID 不得回落到 Old Model"
            );
            let agg = aggregate(
                &[synthetic_event("relay/present-m")],
                GroupBy::Model,
                &tz,
                &p_snap,
            )
            .unwrap();
            let groups: Vec<_> = agg.groups.iter().filter(|g| g.key != "合计").collect();
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].key, expected_key, "{tag} 分组键不得受展示名影响");
            assert_eq!(
                groups[0].label.as_deref(),
                Some("relay/present-m"),
                "{tag} 聚合应保留原始 ID"
            );
            // 计价保持原值（100 input * $1/1M + 50 output * $2/1M = 0.0002）。
            assert!((groups[0].cost_usd - 0.0002).abs() < 1e-12, "{tag}");
        }
    }

    // 对照证明：若生成器误输出旧别名 relay/present-m -> publisher/old，离线会显示 Old Model。
    let leaked_history = r#"{
        "models": [{"id": "publisher/old", "name": "Old Model"}],
        "aliases": [{"id": "relay/present-m", "canonical_id": "publisher/old"}]
    }"#;
    let leaked_p = Pricing::empty().with_display_catalog_json_for_tests(leaked_history);
    assert_eq!(
        leaked_p.display_name_for("relay/present-m").as_deref(),
        Some("Old Model")
    );

    // 现行条目已有可信自身规范名时，照常显示自身规范名，不显示旧目标 Old Model。
    let with_own_forward = r#"{
        "models": [
            {"id": "publisher/old", "name": "Old Model"},
            {"id": "relay/present-m", "name": "Relay Present Model"}
        ],
        "aliases": [
            {"id": "retired/gone-m", "canonical_id": "publisher/old"}
        ]
    }"#;
    let with_own_reversed = r#"{
        "aliases": [
            {"id": "retired/gone-m", "canonical_id": "publisher/old"}
        ],
        "models": [
            {"id": "relay/present-m", "name": "Relay Present Model"},
            {"id": "publisher/old", "name": "Old Model"}
        ]
    }"#;
    for json in [with_own_forward, with_own_reversed] {
        let p = Pricing::empty().with_display_catalog_json_for_tests(json);
        for raw in ["relay/present-m", "Relay/Present_M", "present-m"] {
            assert_eq!(
                p.display_name_for(raw).as_deref(),
                Some("Relay Present Model"),
                "{raw}"
            );
            let agg = aggregate(&[synthetic_event(raw)], GroupBy::Model, &tz, &p).unwrap();
            let groups: Vec<_> = agg.groups.iter().filter(|g| g.key != "合计").collect();
            assert_eq!(groups.len(), 1);
            assert_eq!(
                groups[0].label.as_deref(),
                Some("Relay Present Model"),
                "{raw}"
            );
        }
    }
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
