//! 端到端：Codex fixture 目录 → 发现 → 解析 → 聚合 → 表格 / JSON，以及
//! Claude + Codex 双 agent 合并。数字期望全部为手算固定值，与 M2 plan 不变量对应。

use jiff::tz::TimeZone;
use tokenscope::aggregate::{GroupBy, aggregate};
use tokenscope::pricing::Pricing;
use tokenscope::render;
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::codex::CodexSource;
use tokenscope::source::{Collection, Source};

fn fixture(agent: &str, p: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(agent)
        .join(p)
}

fn codex_basic() -> Collection {
    CodexSource::new(fixture("codex", "basic"))
        .collect()
        .unwrap()
}

fn claude_basic() -> Collection {
    ClaudeSource::new(fixture("claude", "basic"))
        .collect()
        .unwrap()
}

fn tz() -> TimeZone {
    TimeZone::get("Asia/Shanghai").unwrap()
}

#[test]
fn test_e2e_codex_collection() {
    let col = codex_basic();
    // 2 文件 16 行：4 事件（同请求重发去重 1）；跳过 零分量 1 / 无模型 1、
    // 忽略 usage_record 1、坏行 2。
    assert_eq!(col.stats.files_scanned, 2);
    assert_eq!(col.stats.lines_seen, 16);
    assert_eq!(col.stats.events, 4);
    assert_eq!(col.stats.duplicates_dropped, 1);
    assert_eq!(col.stats.skipped_zero_usage, 1);
    assert_eq!(col.stats.skipped_no_model, 1);
    assert_eq!(col.stats.ignored_token_usage_record, 1);
    assert_eq!(col.stats.bad_lines, 2);
    assert!(col.warnings.is_empty());

    // 归一化首事件：input 剔除缓存、cached 归 cache_read。
    let e1 = &col.events[0];
    assert_eq!(e1.model, "gpt-5.6-sol");
    assert_eq!(e1.session_id, "sess-a");
    assert_eq!(e1.project, "alpha");
    assert_eq!(e1.input_tokens, 800);
    assert_eq!(e1.output_tokens, 100);
    assert_eq!(e1.cache_write_tokens, 50);
    assert_eq!(e1.cache_read_tokens, 200);
}

#[test]
fn test_e2e_codex_json() {
    let col = codex_basic();
    let tz = tz();
    let agg = aggregate(&col.events, GroupBy::Day, &tz, &Pricing);
    let out = render::json::to_json(&agg, std::slice::from_ref(&col), &col.warnings, "t").unwrap();
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();

    assert_eq!(v["sources"][0]["agent"], "codex");
    assert_eq!(v["by"], "day");
    let groups = v["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);

    // 07-17：仅 15:59Z（本地 23:59）一条；费用 gpt-5.6-sol 800*4+100*20+50*0.4+200*5 = 6220/1M。
    let d1 = &groups[0];
    assert_eq!(d1["key"], "2026-07-17");
    assert_eq!(d1["requests"], 1);
    assert_eq!(d1["tokens"]["input"], 800);
    assert_eq!(d1["tokens"]["output"], 100);
    assert_eq!(d1["tokens"]["cache_write"], 50);
    assert_eq!(d1["tokens"]["cache_read"], 200);
    assert!((d1["cost_usd"].as_f64().unwrap() - 6220.0 / 1_000_000.0).abs() < 1e-12);
    assert_eq!(d1["unknown_pricing"], false);

    // 07-18：16:01Z/16:06Z 落本地次日 + 次日 02:00Z。
    let d2 = &groups[1];
    assert_eq!(d2["key"], "2026-07-18");
    assert_eq!(d2["requests"], 3);
    assert_eq!(d2["tokens"]["input"], 10 + 5 + 80);
    assert_eq!(d2["tokens"]["output"], 5 + 3 + 50);
    // gpt-5.6-sol 140 + gpt-5.5 115 + gpt-5.4 950（/1M）。
    assert!((d2["cost_usd"].as_f64().unwrap() - 1205.0 / 1_000_000.0).abs() < 1e-9);

    let t = &v["totals"];
    assert_eq!(t["requests"], 4);
    assert!((t["cost_usd"].as_f64().unwrap() - 7425.0 / 1_000_000.0).abs() < 1e-9);

    // 模型维度：BTreeMap 序。
    let agg_m = aggregate(&col.events, GroupBy::Model, &tz, &Pricing);
    let out_m =
        render::json::to_json(&agg_m, std::slice::from_ref(&col), &col.warnings, "t").unwrap();
    let vm: serde_json::Value = serde_json::from_str(&out_m).unwrap();
    let keys: Vec<&str> = vm["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["gpt-5.4", "gpt-5.5", "gpt-5.6-sol"]);
    let sol = &vm["groups"][2];
    assert_eq!(sol["requests"], 2);
    assert_eq!(sol["tokens"]["input"], 810);
    assert_eq!(sol["tokens"]["cache_read"], 200);

    // agent 维度
    let agg_a = aggregate(&col.events, GroupBy::Agent, &tz, &Pricing);
    let out_a =
        render::json::to_json(&agg_a, std::slice::from_ref(&col), &col.warnings, "t").unwrap();
    let va: serde_json::Value = serde_json::from_str(&out_a).unwrap();
    assert_eq!(va["groups"][0]["key"], "codex");
    assert_eq!(va["groups"][0]["requests"], 4);
}

#[test]
fn test_e2e_codex_table() {
    let col = codex_basic();
    let agg = aggregate(&col.events, GroupBy::Day, &tz(), &Pricing);
    let out = render::table(&agg, std::slice::from_ref(&col));
    assert!(out.contains("2026-07-17"), "{out}");
    assert!(out.contains("2026-07-18"), "{out}");
    assert!(out.contains("跳过 零分量 1 / 无模型 1"), "{out}");
    assert!(out.contains("忽略 usage_record 1"), "{out}");
    // 单 agent 报告不带源前缀（M1 格式）。
    assert!(!out.contains("codex: 文件"), "{out}");
    assert!(!out.contains('†'), "全部模型已计价：{out}");
}

#[test]
fn test_e2e_multi_agent_merge() {
    let cc = claude_basic();
    let cx = codex_basic();
    let cols = [cc, cx];
    let mut events = Vec::new();
    for c in &cols {
        events.extend(c.events.iter().cloned());
    }
    assert_eq!(events.len(), 7);

    // agent 维度：3 + 4，合计行覆盖两个 agent。
    let agg = aggregate(&events, GroupBy::Agent, &tz(), &Pricing);
    assert_eq!(agg.groups.len(), 3);
    assert_eq!(agg.groups[0].key, "claude-code");
    assert_eq!(agg.groups[0].requests, 3);
    assert_eq!(agg.groups[1].key, "codex");
    assert_eq!(agg.groups[1].requests, 4);
    assert_eq!(agg.totals.requests, 7);
    assert_eq!(agg.totals.agents, ["claude-code", "codex"]);

    // 日维度合并：claude 07-17 1 条 + codex 07-17 1 条 = 2；07-18 = 2 + 3 = 5。
    let agg_d = aggregate(&events, GroupBy::Day, &tz(), &Pricing);
    assert_eq!(agg_d.groups.len(), 3);
    assert_eq!(agg_d.groups[0].key, "2026-07-17");
    assert_eq!(agg_d.groups[0].requests, 2);
    assert_eq!(agg_d.groups[1].requests, 5);
    // 多 agent 数据填充 agents 字段。
    assert_eq!(agg_d.groups[0].agents, ["claude-code", "codex"]);

    // 多 agent 表格脚注逐源一行。
    let out = render::table(&agg_d, &cols);
    assert!(out.contains("claude-code: 文件 2"), "{out}");
    assert!(out.contains("codex: 文件 2"), "{out}");

    // 多 agent JSON 带 agents 字段。
    let warnings: Vec<String> = cols
        .iter()
        .flat_map(|c| c.warnings.iter().cloned())
        .collect();
    let out_j = render::json::to_json(&agg_d, &cols, &warnings, "t").unwrap();
    let v: serde_json::Value = serde_json::from_str(&out_j).unwrap();
    assert_eq!(v["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        v["groups"][0]["agents"],
        serde_json::json!(["claude-code", "codex"])
    );
}
