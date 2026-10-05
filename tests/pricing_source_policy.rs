//! 定价来源策略（Task 1）：移除编译期内置价格表——
//! 生产定价只有三层：外置 pricing.toml > models.dev > OpenRouter；
//! 无匹配时 lookup 返回 None（unknown），不存在任何编译期 fallback。
//! 所有路径显式注入临时目录，不读写真实 ~/.tokenscope。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::pricing::{INDEX_VERSION, Pricing};
use tokenscope::report::{SummaryOptions, summary};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-policy-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write_modelsdev(dir: &std::path::Path, entries: &str) -> PathBuf {
    let p = dir.join("pricing-modelsdev.json");
    std::fs::write(
        &p,
        format!(r#"{{"v":2,"synced_at":"t","entries":[{entries}]}}"#),
    )
    .unwrap();
    p
}

fn write_openrouter(dir: &std::path::Path, entries: &str) -> PathBuf {
    let p = dir.join("pricing-openrouter.json");
    std::fs::write(&p, format!(r#"{{"synced_at":"t","entries":[{entries}]}}"#)).unwrap();
    p
}

#[test]
fn test_no_builtin_entries_after_source_policy_change() {
    // 无任何来源 → 空表；知名模型不再落到编译期猜测价。
    let dir = tmp_dir("no-builtin");
    let (p, w) = Pricing::load(None, None, None);
    assert!(w.is_empty() || w.iter().all(|x| !x.contains("内置")));
    assert_eq!(p.external_count(), 0);
    assert_eq!(p.modelsdev_count(), 0);
    assert_eq!(p.openrouter_count(), 0);
    assert!(p.lookup("gpt-5.6-sol").is_none(), "不存在内置 fallback");
    assert!(p.lookup("claude-sonnet-4-5").is_none());
    assert!(p.lookup("grok-4").is_none());
    let _ = write_modelsdev(&dir, "");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_unknown_model_without_sources_is_unknown() {
    // 无来源时 summary：模型全部 unknown，费用 0 但 unknown token 单列。
    let dir = tmp_dir("unknown-only");
    let claude_dir = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("s.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c","message":{"id":"m","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}}"#,
    )
    .unwrap();
    let opts = SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(claude_dir),
        codex_dir: Some(dir.join("no-codex")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(dir.join("no-or.json")),
        modelsdev_path: Some(dir.join("no-md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let r = summary(&opts).unwrap();
    assert_eq!(r.totals.requests, 1);
    assert!((r.totals.cost_usd).abs() < 1e-12, "无来源不产生费用");
    assert!(r.totals.unknown_pricing);
    assert_eq!(
        r.totals.unknown_tokens.input, 100,
        "未知 token 单列，不按 0 吞掉"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_external_overrides_modelsdev() {
    // 优先级 1：外置覆盖 models.dev 同前缀条目。
    let dir = tmp_dir("ext-over-md");
    let toml = dir.join("pricing.toml");
    std::fs::write(
        &toml,
        r#"
[[model]]
prefix = "prov/x"
input = 99.0
output = 199.0
cache_write = 0.0
cache_read = 0.0
"#,
    )
    .unwrap();
    let md = write_modelsdev(
        &dir,
        r#"{"id":"prov/x","name":null,"input":1.0,"output":2.0,"cache_read":0.1,"cache_write":0.2}"#,
    );
    let (p, w) = Pricing::load(Some(&toml), Some(&md), None);
    assert!(w.is_empty());
    let hit = p.lookup("prov/x").unwrap();
    assert_eq!(hit.plan.base.input, Some(99.0), "外置覆盖 models.dev");
    assert_eq!(hit.plan.base.output, Some(199.0));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_modelsdev_overrides_openrouter() {
    // 优先级 2：models.dev 覆盖 OpenRouter 同前缀条目。
    let dir = tmp_dir("md-over-or");
    let md = write_modelsdev(
        &dir,
        r#"{"id":"prov/y","name":null,"input":1.0,"output":2.0}"#,
    );
    let or = write_openrouter(
        &dir,
        r#"{"id":"prov/y","name":null,"prompt":0.000009,"completion":0.000019,"input_cache_read":0.000001,"input_cache_write":0.000002}"#,
    );
    let (p, w) = Pricing::load(None, Some(&md), Some(&or));
    assert!(w.is_empty(), "warnings: {:?}", w);
    let hit = p.lookup("prov/y").unwrap();
    assert_eq!(hit.plan.base.input, Some(1.0), "models.dev 覆盖 OpenRouter");
    assert_eq!(hit.plan.base.output, Some(2.0));
    // OpenRouter 独有条目仍可用（补充源）。
    assert!(p.lookup("prov/only-or").is_none(), "此 fixture 无独立条目");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_source_labels_distinguish_modelsdev_and_openrouter() {
    // 来源标签：外置 / models.dev / OpenRouter（不再有"内置"）。
    let dir = tmp_dir("labels");
    let toml = dir.join("pricing.toml");
    std::fs::write(
        &toml,
        r#"
[[model]]
prefix = "prov/ext"
input = 9.0
output = 19.0
cache_write = 0.0
cache_read = 0.0
"#,
    )
    .unwrap();
    let md = write_modelsdev(
        &dir,
        r#"{"id":"prov/md","name":null,"input":1.0,"output":2.0}"#,
    );
    let or = write_openrouter(
        &dir,
        r#"{"id":"prov/or","name":null,"prompt":0.000003,"completion":0.000004}"#,
    );
    let (p, _) = Pricing::load(Some(&toml), Some(&md), Some(&or));
    let sources: Vec<String> = p.entries().iter().map(|e| e.source.to_string()).collect();
    assert!(sources.iter().any(|s| s == "外置"));
    assert!(sources.iter().any(|s| s == "models.dev"));
    assert!(sources.iter().any(|s| s == "OpenRouter"));
    assert!(!sources.iter().any(|s| s == "内置"), "内置来源已移除");
    // 索引版本已随策略升级（旧含内置索引失效）：编译期断言。
    const _: () = assert!(INDEX_VERSION >= 3, "旧索引必须失效");
}
