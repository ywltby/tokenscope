//! 端到端：合成 fixture 目录 → report 管线 → 结构断言（CLI 渲染层已随 CLI 移除）。
//! 数字期望全部为手算固定值，与 M1 plan 不变量一一对应。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, SummaryReport, summary};
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::{Collection, Source};

fn fixture(agent: &str, p: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(agent)
        .join(p)
}

fn basic() -> Collection {
    ClaudeSource::new(fixture("claude", "basic"))
        .collect()
        .unwrap()
}

/// 密闭性（2026-10-05 修复）：缓存/索引一律进临时目录。此前默认落真实
/// `~/.tokenscope/cache.db`，测试套件每跑一遍就把用户缓存 purge 成 fixture，
/// GUI 每次启动都全量冷扫描（分钟级加载）。
fn hermetic_dir() -> PathBuf {
    std::env::temp_dir().join(format!("tokenscope-e2e-claude-{}", std::process::id()))
}

/// 只走 Claude 源的 report 管线（agent 过滤，codex 目录不会触达）。
fn claude_report(by: GroupBy) -> SummaryReport {
    let dir = hermetic_dir();
    // Task 1：内置表已删除——外置 TOML 提供与旧内置同值的 sonnet 价。
    let pricing = dir.join("pricing.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &pricing,
        r#"
[[model]]
prefix = "claude-sonnet-4-5"
input = 3.0
output = 15.0
cache_write = 3.75
cache_read = 0.3
"#,
    )
    .unwrap();
    summary(&SummaryOptions {
        by,
        agent: Some(AgentKind::ClaudeCode),
        claude_dir: Some(fixture("claude", "basic")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(pricing),
        // 固定指向不存在的快照，测试不依赖真实 ~/.tokenscope 状态
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn test_e2e_summary_collection() {
    let col = basic();
    // 采集口径：2 个 jsonl、11 行、5 个未去重事件（M4 起去重上移到 report 层）。
    assert_eq!(col.stats.files_scanned, 2);
    assert_eq!(col.stats.lines_seen, 11);
    assert_eq!(col.stats.events, 5);
    assert_eq!(col.stats.duplicates_dropped, 0);
    assert_eq!(col.stats.bad_lines, 3);
    assert_eq!(col.stats.skipped_sidechain, 1);
    assert_eq!(col.stats.skipped_synthetic, 1);
    assert!(col.warnings.is_empty());
}

#[test]
fn test_e2e_summary_struct() {
    let r = claude_report(GroupBy::Day);
    assert_eq!(r.timezone, "Asia/Shanghai");
    let groups = &r.groups;
    // groups 含末尾合计行（渲染层之前由 CLI 剥离，现在由消费方处理）
    assert_eq!(groups.len(), 3);

    let d1 = &groups[0];
    assert_eq!(d1.key, "2026-07-17");
    assert_eq!(d1.requests, 1);
    assert_eq!(d1.tokens.input, 1000);
    assert_eq!(d1.tokens.output, 200);
    assert_eq!(d1.tokens.cache_write, 5000);
    assert_eq!(d1.tokens.cache_read, 10000);
    assert!((d1.cost_usd - 0.02775).abs() < 1e-9);
    assert!(!d1.unknown_pricing);

    let d2 = &groups[1];
    assert_eq!(d2.key, "2026-07-18", "UTC 16:01 应落本地次日");
    assert_eq!(d2.requests, 2);
    assert_eq!(d2.tokens.input, 200);
    assert_eq!(d2.tokens.output, 150);
    assert!(d2.unknown_pricing);
    assert_eq!(d2.unknown_tokens.input, 100);
    assert_eq!(d2.unknown_tokens.output, 50);
    assert!((d2.cost_usd - 0.0018).abs() < 1e-9);

    let t = &r.totals;
    assert_eq!(t.requests, 3);
    assert_eq!(t.tokens.input, 1200);
    assert_eq!(t.tokens.output, 350);
    assert_eq!(t.tokens.cache_write, 5000);
    assert_eq!(t.tokens.cache_read, 10000);
    assert_eq!(t.unknown_tokens.input, 100);
    assert!((t.cost_usd - 0.02955).abs() < 1e-9);

    assert_eq!(r.sources[0].stats.duplicates_dropped, 2);
    assert_eq!(r.sources[0].stats.bad_lines, 3);
    assert_eq!(r.sources[0].stats.skipped_sidechain, 1);
    assert_eq!(r.sources[0].stats.skipped_synthetic, 1);
}

#[test]
fn test_e2e_summary_dimensions() {
    // 模型维度（末行为合计）
    let rm = claude_report(GroupBy::Model);
    let keys: Vec<&str> = rm.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(
        keys,
        ["claude-sonnet-4-5-20250929", "tencent/hy3:free", "合计"]
    );
    assert_eq!(rm.groups[0].requests, 2);
    assert_eq!(rm.groups[0].tokens.input, 1100);
    assert!(rm.groups[1].unknown_pricing);

    // 项目维度
    let rp = claude_report(GroupBy::Project);
    let keys: Vec<&str> = rp.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(keys, ["proj-alpha", "proj-beta", "合计"]);
    assert_eq!(rp.groups[0].requests, 2);
    assert_eq!(rp.groups[1].requests, 1);
}

#[test]
fn test_e2e_summary_json_roundtrip() {
    // GUI invoke 走 serde 序列化（与 CLI 渲染无关）
    let r = claude_report(GroupBy::Day);
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
    assert_eq!(v["timezone"], "Asia/Shanghai");
    assert_eq!(v["sources"][0]["agent"], "claude-code");
    assert_eq!(v["groups"].as_array().unwrap().len(), 3);
    assert_eq!(v["groups"][0]["key"], "2026-07-17");
    assert_eq!(v["groups"][0]["tokens"]["input"], 1000);
    assert_eq!(v["totals"]["requests"], 3);
    // 末行为合计行（结构体序列化含它；GUI 渲染层自行跳过）
    assert_eq!(v["groups"][2]["key"], "合计");
    assert_eq!(v["sources"][0]["stats"]["duplicates_dropped"], 2);
    assert_eq!(v["sources"][0]["stats"]["bad_lines"], 3);
    assert_eq!(v["sources"][0]["stats"]["skipped_sidechain"], 1);
    assert_eq!(v["sources"][0]["stats"]["skipped_synthetic"], 1);
}
