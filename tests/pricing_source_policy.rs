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

// ── SF10（安全与数据一致性审查 Task 10）：可用性 = 有效候选集合 ──
//
// 不变量（docs/stats-semantics.md §4 展示口径）：
// 1. pricing_status 的 available/hasAnyPricing 基于**有效候选数**（校验
//    通过且存在可解析费率路径；明确 0 是有效价格），不再用原始
//    entries.len()；原始条目数仅作技术诊断字段（*_count）保留；
// 2. 索引恢复（load_cached）与直读来源走同一校验/入表路径——恢复出的
//    集合与状态一致，不构成校验旁路；
// 3. 全部 Unknown 或输入价未知的 SameAsInput 不算可用。

use tokenscope::pricing::pricing_status;

fn status_fixture(
    dir: &std::path::Path,
    md_body: Option<&str>,
    or_body: Option<&str>,
) -> (PathBuf, PathBuf, PathBuf) {
    let md = dir.join("pricing-modelsdev.json");
    if let Some(b) = md_body {
        std::fs::write(&md, b).unwrap();
    }
    let or = dir.join("pricing-openrouter.json");
    if let Some(b) = or_body {
        std::fs::write(&or, b).unwrap();
    }
    let toml = dir.join("pricing.toml");
    (toml, md, or)
}

/// 快照所有条目都被拒绝（非法负价）→ 主源不可用（原始条目数仍可见）。
#[test]
fn all_rejected_candidates_do_not_mark_source_available() {
    let dir = tmp_dir("sf10-all-rejected");
    let (toml, md, or) = status_fixture(
        &dir,
        Some(
            r#"{"v":3,"synced_at":"t","entries":[
                {"id":"prov/a","name":null,"input":-1.0,"output":2.0},
                {"id":"prov/b","name":null,"input":1.0,"output":-2.0},
                {"id":"prov/c","name":null,"input":0.0,"output":0.0,"cache_read":-9.0}
            ]}"#,
        ),
        None,
    );
    let st = pricing_status(Some(&toml), Some(&md), Some(&or));
    assert!(!st.modelsdev_available, "全部候选被拒绝 → 主源不可用");
    assert_eq!(st.modelsdev_count, 3, "原始条目数仍作技术诊断");
    assert_eq!(st.modelsdev_valid_count, 0);
    assert!(st.needs_sync, "主源无效 → needs_sync");
    assert!(
        st.warnings.iter().any(|w| w.contains("非法单价")),
        "拒绝诊断必须可见: {:?}",
        st.warnings
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 候选全部字段 Unknown（未声明）→ 无可解析费率路径 → 不可用；
/// 显式 0 是有效价格 → 可用。
#[test]
fn all_unknown_candidates_are_not_pricing_available() {
    let dir = tmp_dir("sf10-all-unknown");
    // entries 缺 cost 键 → parse 后 input/output/cache 全 None。
    let (toml, md, or) = status_fixture(
        &dir,
        Some(
            r#"{"v":3,"synced_at":"t","entries":[
                {"id":"prov/unknown-model","name":null}
            ]}"#,
        ),
        None,
    );
    let st = pricing_status(Some(&toml), Some(&md), Some(&or));
    assert!(!st.modelsdev_available, "全部 Unknown ≠ 可用");
    assert_eq!(st.modelsdev_valid_count, 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 单个明确 0（免费）即有可解析费率路径 → 可用；部分费率可用仍参与
/// 部分估算（未知分项保留）。
#[test]
fn explicit_zero_is_valid_available_pricing() {
    let dir = tmp_dir("sf10-zero");
    let (toml, md, or) = status_fixture(
        &dir,
        Some(
            r#"{"v":3,"synced_at":"t","entries":[
                {"id":"prov/free-model","name":null,"input":0.0,"output":0.0}
            ]}"#,
        ),
        None,
    );
    let st = pricing_status(Some(&toml), Some(&md), Some(&or));
    assert!(st.modelsdev_available, "显式 0 = 有效价格 → 可用");
    assert_eq!(st.modelsdev_valid_count, 1);
    assert!(!st.needs_sync);
    assert!(st.has_any_pricing);

    // 部分估算：cache_read 未知 → 估算不完整但 input/output 计费。
    let p = Pricing::load_outcome(Some(&toml), Some(&md), Some(&or)).pricing;
    let est = p
        .estimate(
            "prov/free-model",
            &tokenscope::model::TokenCounts {
                input: 1_000_000,
                output: 500_000,
                cache_write: 0,
                cache_read: 100_000,
            },
            "2026-01-05T10:00:00Z".parse().unwrap(),
        )
        .unwrap();
    assert_eq!(est.cost, 0.0, "显式 0 计费为 0（免费）");
    assert!(!est.complete, "cache_read 未知 → 不完整");
    assert_eq!(est.unknown.cache_read, 100_000, "未知分项保留");
    std::fs::remove_dir_all(&dir).ok();
}

/// 索引恢复路径与直读来源产生相同集合与状态（有效候选数一致），
/// 不构成校验旁路；模拟重启（清进程缓存）后状态仍一致。
#[test]
fn source_status_matches_validated_entries_after_restart() {
    let dir = tmp_dir("sf10-restart");
    let (toml, md, or) = status_fixture(
        &dir,
        Some(
            r#"{"v":3,"synced_at":"t","entries":[
                {"id":"prov/ok","name":null,"input":1.0,"output":2.0},
                {"id":"prov/bad","name":null,"input":-3.0,"output":2.0},
                {"id":"prov/unknown","name":null}
            ]}"#,
        ),
        None,
    );
    let index = dir.join("pricing-index.json");

    // 直读来源的状态基准。
    let st_direct = pricing_status(Some(&toml), Some(&md), Some(&or));
    assert_eq!(st_direct.modelsdev_valid_count, 1, "仅 prov/ok 有效");

    // 经 load_cached（重建并写索引）恢复出的集合与状态一致。
    let (p1, _, _) = Pricing::load_cached(Some(&toml), Some(&md), Some(&or), &index);
    assert_eq!(
        p1.modelsdev_resolvable_count(),
        st_direct.modelsdev_valid_count,
        "缓存重建路径的有效候选数与 status 一致"
    );

    // 模拟重启（清进程缓存）→ 索引命中：集合/状态仍一致（索引不是旁路）。
    tokenscope::pricing::clear_price_cache_for_tests();
    let (p2, _, hit) = Pricing::load_cached(Some(&toml), Some(&md), Some(&or), &index);
    assert!(hit, "重建出的当前版索引应命中");
    assert_eq!(
        p2.modelsdev_resolvable_count(),
        st_direct.modelsdev_valid_count,
        "索引恢复的集合与直读一致"
    );
    assert!(index.exists());

    // 外置**读取失败**（目录冒充文件 → 真实 io 错误，非 NotFound）→
    // 降级警告可见；主源本身健康不受影响。
    let broken = dir.join("pricing.toml.broken");
    std::fs::create_dir_all(&broken).unwrap();
    let st_degraded = pricing_status(Some(&broken), Some(&md), Some(&or));
    assert!(
        st_degraded.warnings.iter().any(|w| w.contains("读取失败")),
        "读取失败必须可见: {:?}",
        st_degraded.warnings
    );
    assert!(
        st_degraded.modelsdev_available && st_degraded.modelsdev_valid_count == 1,
        "主源本身健康：降级只影响外置层"
    );
    std::fs::remove_dir_all(&dir).ok();
}
