//! 展示名独立目录：裸 ID 多渠道、无价有名与离线兼容的回归测试。
//!
//! 只用合成 JSON 与公开 models.dev 的规范名称；本机历史/缓存不进入测试。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::aggregate::{GroupBy, aggregate, resolve_tz};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::modelsdev::sync_with_body_for_tests;
use tokenscope::openrouter::sync_with_body_for_tests as sync_openrouter_for_tests;
use tokenscope::pricing::{DisplayNameReason, Pricing, SourceLookupStatus};

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
    // 仅 models.dev 可用、OpenRouter 未同步：未命中时区分来源不可用，不得写成双源确认无名。
    let unnamed_partial = p.display_name_resolution_for("gamma/demo-unnamed");
    assert_eq!(unnamed_partial.name, None);
    assert_eq!(unnamed_partial.reason, DisplayNameReason::SourceUnavailable);
    assert_eq!(
        unnamed_partial.modelsdev_status,
        SourceLookupStatus::CheckedNoMatch
    );
    assert_eq!(
        unnamed_partial.openrouter_status,
        SourceLookupStatus::UnavailableNotSynced
    );

    // 补充已加载的 OpenRouter 快照后，双源均已检查且无名才标记 NoTrustedName / NoIdentityEvidence。
    let or_empty = dir.join("openrouter-empty.json");
    sync_openrouter_for_tests(&or_empty, r#"{"data":[]}"#).unwrap();
    let (p_both, _) = Pricing::load(None, Some(&snap), Some(&or_empty));
    let unnamed = p_both.display_name_resolution_for("gamma/demo-unnamed");
    assert_eq!(unnamed.name, None);
    assert_eq!(unnamed.reason, DisplayNameReason::NoTrustedName);
    assert_eq!(unnamed.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
    assert_eq!(
        unnamed.openrouter_status,
        SourceLookupStatus::CheckedNoMatch
    );

    let missing = p_both.display_name_resolution_for("totally-unknown-model");
    assert_eq!(missing.name, None);
    assert_eq!(missing.reason, DisplayNameReason::NoIdentityEvidence);
    assert_eq!(missing.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
    assert_eq!(
        missing.openrouter_status,
        SourceLookupStatus::CheckedNoMatch
    );

    // 完全离线时两路快照均未同步：标为 SourceUnavailable，不伪装成双源确认无名。
    let (p2, _) = Pricing::load(None, None, None);
    let offline_missing = p2.display_name_resolution_for("totally-unknown-model");
    assert_eq!(offline_missing.name, None);
    assert_eq!(offline_missing.reason, DisplayNameReason::SourceUnavailable);
    assert_eq!(
        offline_missing.modelsdev_status,
        SourceLookupStatus::UnavailableNotSynced
    );
    assert_eq!(
        offline_missing.openrouter_status,
        SourceLookupStatus::UnavailableNotSynced
    );
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
    assert_eq!(bare_res.reason, DisplayNameReason::SourceUnavailable);

    let full_res = offline.display_name_resolution_for(full_id);
    assert_eq!(
        full_res.name, None,
        "弱证据不得用于完整 ID 取名: {full_res:?}"
    );
    assert_eq!(full_res.reason, DisplayNameReason::SourceUnavailable);

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
            assert_eq!(res.reason, DisplayNameReason::SourceUnavailable, "{raw}");
            let agg = aggregate(&[synthetic_event(raw)], GroupBy::Model, &tz, &p).unwrap();
            let groups: Vec<_> = agg.groups.iter().filter(|g| g.key != "合计").collect();
            assert_eq!(groups.len(), 1);
            assert_eq!(groups[0].key, "relay/presentm", "{raw}");
            assert_eq!(groups[0].label.as_deref(), Some(raw), "{raw}");
        }
        let bare = p.display_name_resolution_for("present-m");
        assert_eq!(bare.name, None, "{bare:?}");
        assert_eq!(bare.reason, DisplayNameReason::SourceUnavailable);
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

fn sync_or(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    sync_openrouter_for_tests(&path, body).unwrap();
    path
}

/// DN-OR01：双源解析全矩阵——仅 models.dev 有名、仅 OpenRouter 有名、两源一致
/// （models.dev 优先与无名条目由 OpenRouter 补名）、两源冲突（保留原始 ID 与冲突原因）、
/// 渠道/日期/Preview/Free 边界与双源均未命中，且不依赖条目顺序。
#[test]
fn dual_source_modelsdev_and_openrouter_matrix() {
    let dir = tmp("dual-source-matrix");
    let md_body = r#"{
        "publisher": {"models": {
            "md-only-1": {"name": "MD Only One", "cost": {"input": 1, "output": 2}},
            "both-same-1": {"name": "Both Same Canonical", "cost": {"input": 1, "output": 2}},
            "priced-no-name": {"cost": {"input": 2, "output": 4}}
        }},
        "zhipuai": {"models": {
            "glm-alias-demo": {"cost": {"input": 1, "output": 2}}
        }},
        "openrouter": {"models": {
            "z-ai/glm-alias-demo": {
                "canonical_model_id": "zhipuai/glm-alias-demo",
                "cost": {"input": 1, "output": 2}
            }
        }},
        "author-a": {"models": {
            "split-target": {"name": "Author A Split", "cost": {"input": 1, "output": 2}}
        }}
    }"#;
    let or_body = r#"{"data": [
        {"id": "publisher/both-same-1", "name": "Publisher: Both Same OR", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
        {"id": "publisher/or-only-1", "name": "Publisher: OR Only One", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
        {"id": "publisher/priced-no-name", "name": "Publisher: OR Supplemented", "pricing": {"prompt": "0.000002", "completion": "0.000004"}},
        {"id": "z-ai/glm-alias-demo", "name": "Z.ai: GLM Alias Demo", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
        {"id": "author-b/split-target", "name": "Author B Split", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
        {"id": "publisher/or-only-1:free", "name": "Publisher: OR Only One (free)", "pricing": {"prompt": "0", "completion": "0"}}
    ]}"#;
    let md = sync(&dir, md_body);
    let or = sync_or(&dir, "openrouter.json", or_body);

    // 同时构造倒序快照，验证双源解析对条目顺序不敏感。
    let md_rev = dir.join("modelsdev-rev.json");
    let mut md_val: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&md).unwrap()).unwrap();
    md_val["entries"].as_array_mut().unwrap().reverse();
    std::fs::write(&md_rev, serde_json::to_vec(&md_val).unwrap()).unwrap();

    let or_rev = dir.join("openrouter-rev.json");
    let mut or_val: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&or).unwrap()).unwrap();
    or_val["entries"].as_array_mut().unwrap().reverse();
    std::fs::write(&or_rev, serde_json::to_vec(&or_val).unwrap()).unwrap();

    for (md_path, or_path) in [(&md, &or), (&md_rev, &or_rev)] {
        let (p, _) = Pricing::load(None, Some(md_path), Some(or_path));

        // 1) 仅 models.dev 有名
        let md_only = p.display_name_resolution_for("md-only-1");
        assert_eq!(md_only.name.as_deref(), Some("MD Only One"));
        assert_eq!(md_only.reason, DisplayNameReason::ResolvedSnapshot);
        assert_eq!(md_only.modelsdev_status, SourceLookupStatus::Matched);
        assert_eq!(
            md_only.openrouter_status,
            SourceLookupStatus::CheckedNoMatch
        );

        // 2) 仅 OpenRouter 有名（裸 ID、完整 ID、openrouter/ 前缀）
        for raw in [
            "or-only-1",
            "publisher/or-only-1",
            "openrouter/publisher/or-only-1",
        ] {
            let r = p.display_name_resolution_for(raw);
            assert_eq!(r.name.as_deref(), Some("Publisher: OR Only One"), "{raw}");
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{raw}");
            assert_eq!(r.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
            assert_eq!(r.openrouter_status, SourceLookupStatus::Matched);
        }

        // 3) 两源一致：models.dev 规范名优先
        let same = p.display_name_resolution_for("both-same-1");
        assert_eq!(same.name.as_deref(), Some("Both Same Canonical"));
        assert_eq!(same.reason, DisplayNameReason::ResolvedSnapshot);
        assert_eq!(same.modelsdev_status, SourceLookupStatus::Matched);
        assert_eq!(same.openrouter_status, SourceLookupStatus::Matched);

        // 4) models.dev 有价格但无名称 -> OpenRouter 补名（同 ID 与显式别名）
        for raw in ["priced-no-name", "publisher/priced-no-name"] {
            let r = p.display_name_resolution_for(raw);
            assert_eq!(
                r.name.as_deref(),
                Some("Publisher: OR Supplemented"),
                "{raw}"
            );
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{raw}");
        }
        for raw in [
            "glm-alias-demo",
            "zhipuai/glm-alias-demo",
            "z-ai/glm-alias-demo",
        ] {
            let r = p.display_name_resolution_for(raw);
            assert_eq!(r.name.as_deref(), Some("Z.ai: GLM Alias Demo"), "{raw}");
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{raw}");
        }

        // 5) 两源指向不同型号的真实冲突 -> 裸 ID 保留原文并报告冲突原因，完整 ID 各用各名
        let conflict = p.display_name_resolution_for("split-target");
        assert_eq!(conflict.name, None);
        assert_eq!(conflict.reason, DisplayNameReason::ConflictingSources);
        assert_eq!(
            conflict.modelsdev_status,
            SourceLookupStatus::AmbiguousOrConflict
        );
        assert_eq!(
            conflict.openrouter_status,
            SourceLookupStatus::AmbiguousOrConflict
        );
        assert_eq!(
            p.display_name_for("author-a/split-target").as_deref(),
            Some("Author A Split")
        );
        assert_eq!(
            p.display_name_for("author-b/split-target").as_deref(),
            Some("Author B Split")
        );

        // 6) 保留渠道、日期、Preview、:free 边界
        assert_eq!(
            p.display_name_for("or-only-1:free").as_deref(),
            Some("Publisher: OR Only One (free)")
        );
        for boundary in [
            "other-channel/or-only-1",
            "publisher/or-only-1-preview",
            "publisher/or-only-1-20261126",
            "or-only-1-preview",
            "or-only-1-20261126",
            "publisher/md-only-1:free",
        ] {
            let r = p.display_name_resolution_for(boundary);
            assert_eq!(r.name, None, "边界查询不得借名: {boundary} -> {r:?}");
        }

        // 7) 双源均已检查且未命中
        let miss_bare = p.display_name_resolution_for("unmatched-bare-id-999");
        assert_eq!(miss_bare.name, None);
        assert_eq!(miss_bare.reason, DisplayNameReason::NoIdentityEvidence);
        assert_eq!(
            miss_bare.modelsdev_status,
            SourceLookupStatus::CheckedNoMatch
        );
        assert_eq!(
            miss_bare.openrouter_status,
            SourceLookupStatus::CheckedNoMatch
        );

        let miss_full = p.display_name_resolution_for("unknown-ns/unmatched-model-999");
        assert_eq!(miss_full.name, None);
        assert_eq!(miss_full.reason, DisplayNameReason::NoTrustedName);
        assert_eq!(
            miss_full.modelsdev_status,
            SourceLookupStatus::CheckedNoMatch
        );
        assert_eq!(
            miss_full.openrouter_status,
            SourceLookupStatus::CheckedNoMatch
        );
    }
}

/// DN-OR02：OpenRouter 无报价模型仍保留展示名，且不进入可解析报价候选（不伪造 $0）。
#[test]
fn openrouter_unpriced_entry_keeps_display_name_without_pricing_candidate() {
    let dir = tmp("or-unpriced-named");
    let md = sync(&dir, "{}");
    let or = sync_or(
        &dir,
        "openrouter.json",
        r#"{"data": [
            {"id": "lab/no-price-or-model", "name": "Lab: No Price OR Model"},
            {"id": "lab/null-price-or-model", "name": "Lab: Null Price OR Model", "pricing": {}}
        ]}"#,
    );
    let (p, _) = Pricing::load(None, Some(&md), Some(&or));
    assert_eq!(p.openrouter_resolvable_count(), 0);
    let counts = TokenCounts {
        input: 1000,
        output: 500,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let at: jiff::Timestamp = "2026-11-26T00:00:00Z".parse().unwrap();
    let (tz, _) = resolve_tz(None).unwrap();
    for (raw, expected) in [
        ("no-price-or-model", "Lab: No Price OR Model"),
        ("lab/no-price-or-model", "Lab: No Price OR Model"),
        ("null-price-or-model", "Lab: Null Price OR Model"),
    ] {
        let res = p.display_name_resolution_for(raw);
        assert_eq!(res.name.as_deref(), Some(expected), "{raw}");
        assert_eq!(res.reason, DisplayNameReason::ResolvedOpenRouter, "{raw}");
        if let Some(est) = p.estimate(raw, &counts, at) {
            assert_eq!(est.cost, 0.0, "{raw}");
            assert!(!est.complete, "无报价模型不得标记为完整计价: {raw}");
            assert_eq!(est.unknown.input, counts.input, "{raw}");
            assert_eq!(est.unknown.output, counts.output, "{raw}");
        }
        let agg = aggregate(&[synthetic_event(raw)], GroupBy::Model, &tz, &p).unwrap();
        let g = agg
            .groups
            .iter()
            .find(|g| g.key != "合计")
            .expect("应存在模型聚合组");
        assert_eq!(g.label.as_deref(), Some(expected), "{raw}");
        assert!(g.unknown_pricing, "{raw}");
        assert!(!g.has_estimated_cost, "{raw}");
    }
}

/// DN-OR03：一路未同步、读取失败或解析损坏时，严格区分「该来源不可用」与「已查且无匹配」，
/// 不得把不可用写成双源确认无名。
#[test]
fn single_source_unavailable_distinguishes_not_synced_and_load_failed() {
    let dir = tmp("source-unavailable");
    let md = sync(
        &dir,
        r#"{"vendor": {"models": {"md-known": {"name": "MD Known", "cost": {"input": 1, "output": 2}}}}}"#,
    );
    let or = sync_or(
        &dir,
        "openrouter.json",
        r#"{"data": [{"id": "vendor/or-known", "name": "Vendor: OR Known", "pricing": {"prompt": "0.000001", "completion": "0.000002"}}]}"#,
    );
    let corrupt = dir.join("corrupt.json");
    std::fs::write(&corrupt, "{not valid json").unwrap();
    let missing = dir.join("missing.json");

    // 1) models.dev 已加载、OpenRouter 未同步
    let (p1, _) = Pricing::load(None, Some(&md), Some(&missing));
    let r1 = p1.display_name_resolution_for("missing-model-xyz");
    assert_eq!(r1.name, None);
    assert_eq!(r1.reason, DisplayNameReason::SourceUnavailable);
    assert_eq!(r1.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
    assert_eq!(
        r1.openrouter_status,
        SourceLookupStatus::UnavailableNotSynced
    );

    // 2) models.dev 已加载、OpenRouter JSON 损坏
    let (p2, _) = Pricing::load(None, Some(&md), Some(&corrupt));
    let r2 = p2.display_name_resolution_for("missing-model-xyz");
    assert_eq!(r2.name, None);
    assert_eq!(r2.reason, DisplayNameReason::SourceUnavailable);
    assert_eq!(r2.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
    assert_eq!(
        r2.openrouter_status,
        SourceLookupStatus::UnavailableLoadFailed
    );

    // 3) models.dev 未同步、OpenRouter 已加载（OpenRouter 仍能正常补名）
    let (p3, _) = Pricing::load(None, Some(&missing), Some(&or));
    let hit3 = p3.display_name_resolution_for("or-known");
    assert_eq!(hit3.name.as_deref(), Some("Vendor: OR Known"));
    assert_eq!(hit3.reason, DisplayNameReason::ResolvedOpenRouter);
    assert_eq!(
        hit3.modelsdev_status,
        SourceLookupStatus::UnavailableNotSynced
    );
    assert_eq!(hit3.openrouter_status, SourceLookupStatus::Matched);
    let miss3 = p3.display_name_resolution_for("missing-model-xyz");
    assert_eq!(miss3.name, None);
    assert_eq!(miss3.reason, DisplayNameReason::SourceUnavailable);
    assert_eq!(
        miss3.modelsdev_status,
        SourceLookupStatus::UnavailableNotSynced
    );
    assert_eq!(miss3.openrouter_status, SourceLookupStatus::CheckedNoMatch);

    // 4) 注入 I/O 读取失败（PermissionDenied）：标为 UnavailableLoadFailed
    let deny_or = dir.join("deny-or.json");
    let deny_or_clone = deny_or.clone();
    let outcome = Pricing::load_outcome_with(None, Some(&md), Some(&deny_or), &move |path| {
        if path == deny_or_clone {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "injected read failure",
            ))
        } else {
            std::fs::read(path)
        }
    });
    let r4 = outcome
        .pricing
        .display_name_resolution_for("missing-model-xyz");
    assert_eq!(r4.name, None);
    assert_eq!(r4.reason, DisplayNameReason::SourceUnavailable);
    assert_eq!(r4.modelsdev_status, SourceLookupStatus::CheckedNoMatch);
    assert_eq!(
        r4.openrouter_status,
        SourceLookupStatus::UnavailableLoadFailed
    );
}

/// DN-OR04：复用现有同步快照——冷加载、已有 PricingIndex 恢复与 OpenRouter 同步后刷新
/// 均正确提供双源展示名（含无报价条目）。
#[test]
fn dual_source_names_survive_cold_load_index_recovery_and_post_sync_refresh() {
    let dir = tmp("dual-source-lifecycle");
    let md = sync(
        &dir,
        r#"{"vendor": {"models": {"md-base": {"name": "MD Base", "cost": {"input": 1, "output": 2}}}}}"#,
    );
    let or = sync_or(
        &dir,
        "openrouter.json",
        r#"{"data": [
            {"id": "vendor/or-cached-1", "name": "Vendor: OR Cached One", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
            {"id": "vendor/or-unpriced-1", "name": "Vendor: OR Unpriced One"}
        ]}"#,
    );
    let idx = dir.join("pricing-index.json");

    // 1) 冷加载并写出磁盘 PricingIndex
    let (cold, _, cold_hit) = Pricing::load_cached(None, Some(&md), Some(&or), &idx);
    assert!(!cold_hit);
    assert_eq!(
        cold.display_name_for("or-cached-1").as_deref(),
        Some("Vendor: OR Cached One")
    );
    assert_eq!(
        cold.display_name_for("or-unpriced-1").as_deref(),
        Some("Vendor: OR Unpriced One")
    );
    assert!(idx.exists(), "应已写出磁盘价格索引");

    // 2) 驱逐进程内 1 槽缓存，再按同一签名加载 -> 命中磁盘 PricingIndex 恢复路径
    let evict_dir = dir.join("evict");
    std::fs::create_dir_all(&evict_dir).unwrap();
    let other_md = sync(
        &evict_dir,
        r#"{"other": {"models": {"evict": {"name": "Evict", "cost": {"input": 1, "output": 1}}}}}"#,
    );
    let other_idx = evict_dir.join("other-index.json");
    let _ = Pricing::load_cached(None, Some(&other_md), None, &other_idx);

    let (recovered, _, recovered_hit) = Pricing::load_cached(None, Some(&md), Some(&or), &idx);
    assert!(recovered_hit, "应命中已有磁盘 PricingIndex 恢复路径");
    assert_eq!(
        recovered.display_name_for("md-base").as_deref(),
        Some("MD Base")
    );
    assert_eq!(
        recovered.display_name_for("or-cached-1").as_deref(),
        Some("Vendor: OR Cached One")
    );
    assert_eq!(
        recovered.display_name_for("or-unpriced-1").as_deref(),
        Some("Vendor: OR Unpriced One")
    );

    // 3) OpenRouter 同步后刷新：快照内容变化触发重建，新模型名称立即可见
    sync_openrouter_for_tests(
        &or,
        r#"{"data": [
            {"id": "vendor/or-cached-1", "name": "Vendor: OR Cached One", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
            {"id": "vendor/or-fresh-2", "name": "Vendor: OR Fresh Two", "pricing": {"prompt": "0.000003", "completion": "0.000006"}}
        ]}"#,
    )
    .unwrap();
    let (refreshed, _, refreshed_hit) = Pricing::load_cached(None, Some(&md), Some(&or), &idx);
    assert!(!refreshed_hit, "同步更新快照后应重建而非复用旧索引");
    assert_eq!(
        refreshed.display_name_for("or-fresh-2").as_deref(),
        Some("Vendor: OR Fresh Two")
    );
}

/// DN-OR05：聚合层消费双源展示名，表/图/悬浮统一使用 Group.label，分组键、计价与
/// 四桶汇总保持原口径，冲突与未命中保留原始 ID。
#[test]
fn dual_source_aggregation_preserves_grouping_billing_and_raw_ids() {
    let dir = tmp("dual-source-aggregate");
    let md = sync(
        &dir,
        r#"{
            "vendor": {"models": {"priced-unnamed": {"cost": {"input": 2, "output": 4}}}},
            "author-a": {"models": {"ambig-m": {"name": "Author A", "cost": {"input": 1, "output": 2}}}}
        }"#,
    );
    let or = sync_or(
        &dir,
        "openrouter.json",
        r#"{"data": [
            {"id": "vendor/priced-unnamed", "name": "Vendor: Priced Unnamed", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
            {"id": "vendor/or-free-name", "name": "Vendor: OR Unpriced Only"},
            {"id": "author-b/ambig-m", "name": "Author B", "pricing": {"prompt": "0.000001", "completion": "0.000002"}}
        ]}"#,
    );
    let (p, _) = Pricing::load(None, Some(&md), Some(&or));
    let (tz, _) = resolve_tz(None).unwrap();
    let events = vec![
        synthetic_event("priced-unnamed"),
        synthetic_event("or-free-name"),
        synthetic_event("ambig-m"),
    ];
    let report = aggregate(&events, GroupBy::Model, &tz, &p).unwrap();

    let by_key = |k: &str| {
        report
            .groups
            .iter()
            .find(|g| g.key == k)
            .unwrap_or_else(|| panic!("缺少分组 {k}: {:?}", report.groups))
    };
    let g_priced = by_key("pricedunnamed");
    assert_eq!(g_priced.label.as_deref(), Some("Vendor: Priced Unnamed"));
    // 计价仍按 models.dev/OpenRouter 最高费用规则（input=2/M, output=4/M）：100*2e-6 + 50*4e-6 = 0.0004
    assert!((g_priced.cost_usd - 0.0004).abs() < 1e-9);
    assert_eq!(g_priced.tokens.input, 100);
    assert_eq!(g_priced.tokens.output, 50);

    let g_unpriced = by_key("orfreename");
    assert_eq!(
        g_unpriced.label.as_deref(),
        Some("Vendor: OR Unpriced Only")
    );
    assert!(g_unpriced.unknown_pricing);
    assert_eq!(g_unpriced.cost_usd, 0.0);

    let g_conflict = by_key("ambigm");
    assert_eq!(g_conflict.label.as_deref(), Some("ambig-m"));
}

/// DN-OR06：OpenRouter 复核补修回归——
/// 1) 精确条目无名时继续查已确认别名（原 ID 与裸 ID 均命中，含磁盘索引恢复）；
/// 2) 同步遇到非法报价（负价、非法 override、坏结构 pricing）时拒绝报价但保留有效名称。
#[test]
fn openrouter_unnamed_exact_alias_and_invalid_price_name_retention() {
    let dir = tmp("openrouter-recheck-two-fixes");
    let md = sync(
        &dir,
        r#"{
            "publisher": {"models": {"recheck-model": {"cost": {"input": 1, "output": 2}}}},
            "openrouter": {"models": {
                "alt-org/recheck-alias": {"canonical_model_id": "publisher/recheck-model", "cost": {"input": 1, "output": 2}}
            }}
        }"#,
    );
    let or_path = dir.join("openrouter.json");
    let or_report = sync_openrouter_for_tests(
        &or_path,
        r#"{"data": [
            {"id": "publisher/recheck-model", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
            {"id": "alt-org/recheck-alias", "name": "Recheck Good Name", "pricing": {"prompt": "0.000001", "completion": "0.000002"}},
            {"id": "vendor/neg-priced", "name": "Vendor Neg Priced", "pricing": {"prompt": "-0.001", "completion": "0.002"}},
            {"id": "vendor/nan-ov-priced", "name": "Vendor NaN Override", "pricing": {
                "prompt": "0.000001", "completion": "0.000002",
                "overrides": [{"min_prompt_tokens": 100000, "prompt": "NaN", "completion": "0.000004"}]
            }},
            {"id": "vendor/bad-shape-priced", "name": "Vendor Bad Shape", "pricing": "not-an-object"}
        ]}"#,
    )
    .expect("同步应成功并拒绝三条非法报价");
    // 只有合法报价的 2 条进入报价计数；三条非法报价均带告警拒绝。
    assert_eq!(or_report.count, 2, "{:?}", or_report.warnings);
    assert!(
        or_report
            .warnings
            .iter()
            .any(|w| w.contains("vendor/neg-priced")),
        "{:?}",
        or_report.warnings
    );

    let idx = dir.join("pricing-index.json");
    let counts = TokenCounts {
        input: 1_000,
        output: 500,
        cache_write: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let at: jiff::Timestamp = "2026-11-26T00:00:00Z".parse().unwrap();
    let check_pricing = |p: &Pricing, phase: &str| {
        // 1) 无名精确条目不得挡住已确认别名
        for q in ["publisher/recheck-model", "recheck-model"] {
            let r = p.display_name_resolution_for(q);
            assert_eq!(
                r.name.as_deref(),
                Some("Recheck Good Name"),
                "{phase} {q} -> {r:?}"
            );
            assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{phase}");
        }
        // 2) 非法报价条目的名称全部保留，但报价仍被彻底拒绝
        for (full, bare, expected) in [
            ("vendor/neg-priced", "neg-priced", "Vendor Neg Priced"),
            (
                "vendor/nan-ov-priced",
                "nan-ov-priced",
                "Vendor NaN Override",
            ),
            (
                "vendor/bad-shape-priced",
                "bad-shape-priced",
                "Vendor Bad Shape",
            ),
        ] {
            for q in [full, bare] {
                let r = p.display_name_resolution_for(q);
                assert_eq!(r.name.as_deref(), Some(expected), "{phase} {q} -> {r:?}");
                assert_eq!(r.reason, DisplayNameReason::ResolvedOpenRouter, "{phase}");
                assert!(
                    p.estimate(q, &counts, at).is_none(),
                    "{phase} 非法报价条目不得产生报价候选: {q}"
                );
            }
        }
        assert!(
            !p.entries().iter().any(|e| {
                e.prefix.contains("negpriced")
                    || e.prefix.contains("nanovpriced")
                    || e.prefix.contains("badshapepriced")
            }),
            "{phase} 非法报价条目不得出现在价格列表中"
        );
    };

    // 冷加载 + 生成磁盘索引
    let (p_cold, _, hit_cold) = Pricing::load_cached(None, Some(&md), Some(&or_path), &idx);
    assert!(!hit_cold);
    check_pricing(&p_cold, "cold");

    // 聚合层：负价模型显示友好名、费用保持未知 0
    let (tz, _) = resolve_tz(None).unwrap();
    let events = vec![
        synthetic_event("recheck-model"),
        synthetic_event("neg-priced"),
    ];
    let report = aggregate(&events, GroupBy::Model, &tz, &p_cold).unwrap();
    let g_recheck = report
        .groups
        .iter()
        .find(|g| g.key == "recheckmodel")
        .unwrap();
    assert_eq!(g_recheck.label.as_deref(), Some("Recheck Good Name"));
    let g_neg = report.groups.iter().find(|g| g.key == "negpriced").unwrap();
    assert_eq!(g_neg.label.as_deref(), Some("Vendor Neg Priced"));
    assert!(g_neg.unknown_pricing);
    assert!(!g_neg.has_estimated_cost);
    assert_eq!(g_neg.cost_usd, 0.0);

    // 驱逐内存缓存后经已有磁盘 PricingIndex 恢复
    let evict = dir.join("evict");
    std::fs::create_dir_all(&evict).unwrap();
    let _ = Pricing::load_cached(None, None, None, &evict.join("evict.json"));
    let (p_cached, _, hit_cached) = Pricing::load_cached(None, Some(&md), Some(&or_path), &idx);
    assert!(hit_cached, "应命中磁盘 PricingIndex");
    check_pricing(&p_cached, "cached");
}

/// DN-C18：随包公开目录已确认裸 ID 的发布方身份时，OpenRouter 仅同末段、
/// 无显式 canonical 关系的渠道条目不构成跨源身份冲突，不得盖掉发布方规范名。
/// 对应原生 ui-identity 的 `model_variants_merge_with_friendly_name`：
/// `anthropic/claude-opus-5-5` 的发布方名必须可见，渠道名不得覆盖。
#[test]
fn embedded_catalog_name_wins_over_implicit_openrouter_same_leaf() {
    let dir = tmp("embedded-vs-implicit-or");
    // models.dev 快照沿用夹具渠道名（同末段近似名，非发布方规范名）。
    let md = sync(
        &dir,
        r#"{"anthropic": {"models": {
            "claude-opus-5.5": {"name": "Synthetic Opus 5.5", "cost": {"input": 5, "output": 25}}
        }}}"#,
    );
    // OpenRouter 仅同末段、无 canonical 关系：其 ID 不是身份证据。
    let or = sync_or(
        &dir,
        "openrouter.json",
        r#"{"data": [
            {"id": "synthetic/claude-opus-5.5", "name": "Synthetic Opus 5.5",
             "pricing": {"prompt": "0.000006", "completion": "0.000030"}}
        ]}"#,
    );
    let (p, _) = Pricing::load(None, Some(&md), Some(&or));

    // 裸 ID 的公开目录身份优先；渠道同末段隐式命中不得触发 conflicting_sources。
    let bare = p.display_name_resolution_for("claude-opus-5-5");
    assert_eq!(
        bare.name.as_deref(),
        Some("Claude Opus 5.5"),
        "发布方规范名应胜出: {bare:?}"
    );
    assert_ne!(bare.reason, DisplayNameReason::ConflictingSources);
    assert_eq!(
        p.display_name_for("claude-opus-5-5").as_deref(),
        Some("Claude Opus 5.5")
    );

    // 完整 ID 自身规范名不变；请求明细仍保留原始 ID。
    assert_eq!(
        p.display_name_for("anthropic/claude-opus-5-5").as_deref(),
        Some("Claude Opus 5.5")
    );

    // 聚合标签与图表共用同一规范名（三处同源），且身份键不变。
    let (tz, _) = resolve_tz(None).unwrap();
    let event = synthetic_event("claude-opus-5-5");
    let report = aggregate(&[event], GroupBy::Model, &tz, &p).unwrap();
    let group = report
        .groups
        .iter()
        .find(|g| g.key == "claudeopus55")
        .unwrap();
    assert_eq!(group.label.as_deref(), Some("Claude Opus 5.5"));
}
