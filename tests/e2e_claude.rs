//! 端到端：合成 fixture 目录 → 发现 → 解析 → 聚合 → 表格 / JSON。
//! 数字期望全部为手算固定值，与 M1 plan 不变量一一对应。

use jiff::tz::TimeZone;
use tokenscope::aggregate::{GroupBy, aggregate};
use tokenscope::pricing::Pricing;
use tokenscope::render;
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::{Collection, Source};

fn fixture(p: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/claude")
        .join(p)
}

fn basic() -> Collection {
    ClaudeSource::new(fixture("basic")).collect().unwrap()
}

fn tz() -> TimeZone {
    TimeZone::get("Asia/Shanghai").unwrap()
}

#[test]
fn test_e2e_summary_table() {
    let col = basic();
    // 采集口径：2 个 jsonl、11 行、3 个事件（去重丢 2、坏行 3、跳过 sidechain 1 / synthetic 1）。
    assert_eq!(col.stats.files_scanned, 2);
    assert_eq!(col.stats.lines_seen, 11);
    assert_eq!(col.stats.events, 3);
    assert_eq!(col.stats.duplicates_dropped, 2);
    assert_eq!(col.stats.bad_lines, 3);
    assert_eq!(col.stats.skipped_sidechain, 1);
    assert_eq!(col.stats.skipped_synthetic, 1);
    assert!(col.warnings.is_empty());

    let agg = aggregate(&col.events, GroupBy::Day, &tz(), &Pricing);
    let out = render::table(&agg, &col.stats);
    assert!(out.contains("2026-07-17"), "缺日期分组：{out}");
    assert!(out.contains("2026-07-18"), "UTC 16:01 应落本地次日：{out}");
    assert!(out.contains("1,000"), "缺输入 token：{out}");
    assert!(out.contains("合计"), "缺合计行：{out}");
    assert!(out.contains('†'), "未知计价标记：{out}");
}

#[test]
fn test_e2e_summary_json() {
    let col = basic();
    let tz = tz();
    let agg = aggregate(&col.events, GroupBy::Day, &tz, &Pricing);
    let out = render::json::to_json(
        &agg,
        col.agent,
        &col.stats,
        &col.warnings,
        "2026-10-03T00:00:00+08:00",
    )
    .unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();

    assert_eq!(v["agent"], "claude-code");
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

    assert_eq!(v["stats"]["duplicates_dropped"], 2);
    assert_eq!(v["stats"]["bad_lines"], 3);
    assert_eq!(v["stats"]["skipped_sidechain"], 1);
    assert_eq!(v["stats"]["skipped_synthetic"], 1);

    // 模型维度
    let agg_m = aggregate(&col.events, GroupBy::Model, &tz, &Pricing);
    let out_m = render::json::to_json(&agg_m, col.agent, &col.stats, &col.warnings, "t").unwrap();
    let vm: serde_json::Value = serde_json::from_str(&out_m).unwrap();
    let keys: Vec<&str> = vm["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["claude-sonnet-4-5-20250929", "grok-4.5-build"]);
    let known = &vm["groups"][0];
    assert_eq!(known["requests"], 2);
    assert_eq!(known["tokens"]["input"], 1100);
    let unknown = &vm["groups"][1];
    assert_eq!(unknown["unknown_pricing"], true);

    // 项目维度
    let agg_p = aggregate(&col.events, GroupBy::Project, &tz, &Pricing);
    let out_p = render::json::to_json(&agg_p, col.agent, &col.stats, &col.warnings, "t").unwrap();
    let vp: serde_json::Value = serde_json::from_str(&out_p).unwrap();
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
