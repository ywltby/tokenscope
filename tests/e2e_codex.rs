//! 端到端：Codex fixture 目录 → report 管线 → 表格 / JSON，以及
//! Claude + Codex 双 agent 合并。数字期望全部为手算固定值，与 M2 plan 不变量对应。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, SummaryReport, summary};
use tokenscope::source::codex::CodexSource;
use tokenscope::source::{Collection, Source};

fn fixture(agent: &str, p: &str) -> PathBuf {
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

/// 密闭性（2026-10-05 修复）：缓存/索引一律进临时目录。此前默认落真实
/// `~/.tokenscope/cache.db`，测试套件每跑一遍就把用户缓存 purge 成 fixture，
/// GUI 每次启动都全量冷扫描（分钟级加载）。
fn hermetic_dir() -> PathBuf {
    std::env::temp_dir().join(format!("tokenscope-e2e-codex-{}", std::process::id()))
}

/// 只走 Codex 源的 report 管线（agent 过滤，claude 目录不会触达）。
fn codex_report(by: GroupBy) -> SummaryReport {
    let dir = hermetic_dir();
    summary(&SummaryOptions {
        by,
        agent: Some(AgentKind::Codex),
        codex_dir: Some(fixture("codex", "basic")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        // 固定指向不存在的快照，e2e 断言不依赖真实 ~/.tokenscope 状态
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        ..Default::default()
    })
    .unwrap()
}

/// 双 agent 合并管线。
fn both_report(by: GroupBy) -> SummaryReport {
    let dir = hermetic_dir();
    summary(&SummaryOptions {
        by,
        claude_dir: Some(fixture("claude", "basic")),
        codex_dir: Some(fixture("codex", "basic")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn test_e2e_codex_collection() {
    let col = codex_basic();
    // 2 文件 16 行：5 个未去重事件（同请求重发 1 由全局 dedupe 处理）；
    // 跳过 零分量 1 / 无模型 1、忽略 usage_record 1、坏行 2。
    assert_eq!(col.stats.files_scanned, 2);
    assert_eq!(col.stats.lines_seen, 16);
    assert_eq!(col.stats.events, 5);
    assert_eq!(col.stats.duplicates_dropped, 0);
    assert_eq!(col.stats.skipped_zero_usage, 1);
    assert_eq!(col.stats.skipped_no_model, 1);
    assert_eq!(col.stats.ignored_token_usage_record, 1);
    assert_eq!(col.stats.bad_lines, 2);
    assert!(col.warnings.is_empty());

    // 归一化首事件：input 剔除缓存、cached 归 cache_read。
    let e1 = &col.events[0];
    assert_eq!(e1.model, "gpt-5.6-sol");
    assert_eq!(e1.session_id, "sess-a");
    // C2：项目身份 = 完整 cwd（fixture 里是反斜杠写法）。
    assert_eq!(e1.project, r"C:\work\alpha");
    // B1 语义修复：raw input 1000 = 750 未缓存 + 200 读 + 50 写。
    assert_eq!(e1.input_tokens, 750);
    assert_eq!(e1.output_tokens, 100);
    assert_eq!(e1.cache_write_tokens, 50);
    assert_eq!(e1.cache_read_tokens, 200);
}

#[test]
fn test_e2e_codex_json() {
    let r = codex_report(GroupBy::Day);
    assert_eq!(r.sources[0].agent, AgentKind::Codex);
    assert_eq!(r.by, "day");
    let groups = &r.groups;
    // groups 含末尾合计行
    assert_eq!(groups.len(), 3);

    // 07-17：仅 15:59Z（本地 23:59）一条；费用 gpt-5.6-sol 750*4+100*20+50*5.0+200*0.4 = 5330/1M
    //（Task 4：内置表缓存两列校正——cw=1.25×input、cr=0.1×input）。
    let d1 = &groups[0];
    assert_eq!(d1.key, "2026-07-17");
    assert_eq!(d1.requests, 1);
    assert_eq!(d1.tokens.input, 750);
    assert_eq!(d1.tokens.output, 100);
    assert_eq!(d1.tokens.cache_write, 50);
    assert_eq!(d1.tokens.cache_read, 200);
    assert!((d1.cost_usd - 5330.0 / 1_000_000.0).abs() < 1e-12);
    assert!(!d1.unknown_pricing);

    // 07-18：16:01Z/16:06Z 落本地次日 + 次日 02:00Z。
    let d2 = &groups[1];
    assert_eq!(d2.key, "2026-07-18");
    assert_eq!(d2.requests, 3);
    assert_eq!(d2.tokens.input, 10 + 5 + 80);
    assert_eq!(d2.tokens.output, 5 + 3 + 50);
    // gpt-5.6-sol 140 + gpt-5.5 116 + gpt-5.4 955（/1M；Task 4 表校正后
    // 缓存读价 0.5/0.25 计入 e4/e6 的 cr 2/20）。
    assert!((d2.cost_usd - 1211.0 / 1_000_000.0).abs() < 1e-9);

    let t = &r.totals;
    assert_eq!(t.requests, 4);
    assert!((t.cost_usd - 6541.0 / 1_000_000.0).abs() < 1e-9);

    // 模型维度：BTreeMap 序（末行为合计）。
    let rm = codex_report(GroupBy::Model);
    let keys: Vec<&str> = rm.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(keys, ["gpt-5.4", "gpt-5.5", "gpt-5.6-sol", "合计"]);
    let sol = &rm.groups[2];
    assert_eq!(sol.requests, 2);
    assert_eq!(sol.tokens.input, 760);
    assert_eq!(sol.tokens.cache_read, 200);

    // agent 维度
    let ra = codex_report(GroupBy::Agent);
    assert_eq!(ra.groups[0].key, "codex");
    assert_eq!(ra.groups[0].requests, 4);
}

#[test]
fn test_e2e_codex_report_shape() {
    let r = codex_report(GroupBy::Day);
    assert_eq!(r.by, "day");
    // 单 agent 报告：sources 仅一行且无 agents 标注（多 agent 才填充）。
    assert_eq!(r.sources.len(), 1);
    assert!(r.groups.iter().all(|g| g.agents.is_empty()));
}

#[test]
fn test_e2e_multi_agent_merge() {
    let r = both_report(GroupBy::Agent);
    assert_eq!(r.sources.len(), 2);
    assert_eq!(r.groups.len(), 3); // claude-code、codex、合计
    assert_eq!(r.groups[0].key, "claude-code");
    assert_eq!(r.groups[0].requests, 3);
    assert_eq!(r.groups[1].key, "codex");
    assert_eq!(r.groups[1].requests, 4);
    assert_eq!(r.totals.requests, 7);
    assert_eq!(r.totals.agents, ["claude-code", "codex"]);

    // 日维度合并：claude 07-17 1 条 + codex 07-17 1 条 = 2；07-18 = 2 + 3 = 5。
    let d = both_report(GroupBy::Day);
    assert_eq!(d.groups.len(), 3);
    assert_eq!(d.groups[0].key, "2026-07-17");
    assert_eq!(d.groups[0].requests, 2);
    assert_eq!(d.groups[1].requests, 5);
    // 多 agent 数据填充 agents 字段。
    assert_eq!(d.groups[0].agents, ["claude-code", "codex"]);

    // 多 agent JSON 带 agents 字段与逐源统计（GUI invoke 同构）。
    let v: serde_json::Value = serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();
    assert_eq!(v["sources"].as_array().unwrap().len(), 2);
    assert_eq!(
        v["groups"][0]["agents"],
        serde_json::json!(["claude-code", "codex"])
    );
}
