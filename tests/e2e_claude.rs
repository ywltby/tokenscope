//! 端到端：合成 fixture 目录 → report 管线 → 表格 / JSON。
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

/// 只走 Claude 源的 report 管线（agent 过滤，codex 目录不会触达）。
fn claude_report(by: GroupBy) -> SummaryReport {
    summary(&SummaryOptions {
        by,
        agent: Some(AgentKind::ClaudeCode),
        claude_dir: Some(fixture("claude", "basic")),
        // 固定指向不存在的快照，e2e 断言不依赖真实 ~/.tokenscope 状态
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn test_e2e_summary_table() {
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

    let out = claude_report(GroupBy::Day).to_table();
    assert!(out.contains("2026-07-17"), "缺日期分组：{out}");
    assert!(out.contains("2026-07-18"), "UTC 16:01 应落本地次日：{out}");
    assert!(out.contains("1,000"), "缺输入 token：{out}");
    assert!(out.contains("合计"), "缺合计行：{out}");
    assert!(out.contains('†'), "未知计价标记：{out}");
}

#[test]
fn test_e2e_summary_json() {
    let v: serde_json::Value =
        serde_json::from_str(&claude_report(GroupBy::Day).to_json().unwrap()).unwrap();

    assert_eq!(v["sources"][0]["agent"], "claude-code");
    assert_eq!(v["timezone"], "Asia/Shanghai");
    assert_eq!(v["by"], "day");
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);

    let d1 = &groups[0];
    assert_eq!(d1["key"], "2026-07-17");
    assert_eq!(d1["requests"], 1);
    assert_eq!(d1["tokens"]["input"], 1000);
    assert_eq!(d1["tokens"]["output"], 200);
    assert_eq!(d1["tokens"]["cache_write"], 5000);
    assert_eq!(d1["tokens"]["cache_read"], 10000);
    assert!((d1["cost_usd"].as_f64().unwrap() - 0.02775).abs() < 1e-9);
    assert_eq!(d1["unknown_pricing"], false);

    let d2 = &groups[1];
    assert_eq!(d2["key"], "2026-07-18");
    assert_eq!(d2["requests"], 2);
    assert_eq!(d2["tokens"]["input"], 200);
    assert_eq!(d2["tokens"]["output"], 150);
    assert_eq!(d2["unknown_pricing"], true);
    assert_eq!(d2["unknown_tokens"]["input"], 100);
    assert_eq!(d2["unknown_tokens"]["output"], 50);
    assert!((d2["cost_usd"].as_f64().unwrap() - 0.0018).abs() < 1e-9);

    let t = &v["totals"];
    assert_eq!(t["requests"], 3);
    assert_eq!(t["tokens"]["input"], 1200);
    assert_eq!(t["tokens"]["output"], 350);
    assert_eq!(t["tokens"]["cache_write"], 5000);
    assert_eq!(t["tokens"]["cache_read"], 10000);
    assert_eq!(t["unknown_tokens"]["input"], 100);
    assert!((t["cost_usd"].as_f64().unwrap() - 0.02955).abs() < 1e-9);

    assert_eq!(v["sources"][0]["stats"]["duplicates_dropped"], 2);
    assert_eq!(v["sources"][0]["stats"]["bad_lines"], 3);
    assert_eq!(v["sources"][0]["stats"]["skipped_sidechain"], 1);
    assert_eq!(v["sources"][0]["stats"]["skipped_synthetic"], 1);

    // 模型维度
    let vm: serde_json::Value =
        serde_json::from_str(&claude_report(GroupBy::Model).to_json().unwrap()).unwrap();
    let keys: Vec<&str> = vm["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["claude-sonnet-4-5-20250929", "tencent/hy3:free"]);
    let known = &vm["groups"][0];
    assert_eq!(known["requests"], 2);
    assert_eq!(known["tokens"]["input"], 1100);
    let unknown = &vm["groups"][1];
    assert_eq!(unknown["unknown_pricing"], true);

    // 项目维度
    let vp: serde_json::Value =
        serde_json::from_str(&claude_report(GroupBy::Project).to_json().unwrap()).unwrap();
    let keys: Vec<&str> = vp["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["proj-alpha", "proj-beta"]);
    assert_eq!(vp["groups"][0]["requests"], 2);
    assert_eq!(vp["groups"][1]["requests"], 1);
}
