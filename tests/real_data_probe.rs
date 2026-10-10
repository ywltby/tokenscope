//! 真实数据只读诊断（默认 `#[ignore]`）：列出指定项目的模型分布，用于核实
//! “某个项目里某模型为什么看不到”这类问题。
//!
//! 约定（与 `perf_real_data` 一致）：真实来源只读；缓存、索引与价格路径全部
//! 注入临时目录，**不触碰** `~/.tokenscope`；必须显式设置环境变量才执行。
//!
//! ```powershell
//! $env:TOKENSCOPE_REAL_PROBE = "1"
//! $env:TOKENSCOPE_REAL_PROBE_PROJECT = "weixiao"   # 可选，默认 weixiao
//! cargo test --test real_data_probe -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;

use tokenscope::aggregate::GroupBy;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::codex::CodexSource;

#[test]
#[ignore = "真实数据诊断：需设置 TOKENSCOPE_REAL_PROBE=1（只读来源 + 隔离缓存）"]
fn real_data_project_model_breakdown() {
    if std::env::var("TOKENSCOPE_REAL_PROBE").as_deref() != Ok("1") {
        eprintln!("未设置 TOKENSCOPE_REAL_PROBE=1，跳过（不伪造通过）");
        return;
    }
    let needle =
        std::env::var("TOKENSCOPE_REAL_PROBE_PROJECT").unwrap_or_else(|_| "weixiao".to_string());
    let dir = std::env::temp_dir().join(format!("tokenscope-real-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let opts = SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(ClaudeSource::default_root().unwrap()),
        codex_dir: Some(CodexSource::default_root().unwrap()),
        cache_dir: Some(dir.clone()),
        pricing_index: Some(dir.join("pricing-index.json")),
        // 价格源指向不存在的文件：本诊断只关心“识别到哪些模型与用量”。
        pricing_path: Some(dir.join("no-pricing.toml")),
        openrouter_path: Some(dir.join("no-openrouter.json")),
        modelsdev_path: Some(dir.join("no-modelsdev.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };

    let snap = query::begin_query(&opts).unwrap();
    let r = query::query_summary(&snap.query_id).unwrap();
    let lower = needle.to_lowercase();
    let hits: Vec<&tokenscope::aggregate::Group> = r
        .groups
        .iter()
        .filter(|g| g.key.to_lowercase().contains(&lower))
        .collect();
    println!("项目名匹配 {needle:?} 的分组：{} 个", hits.len());
    for g in &hits {
        println!(
            "  {} | 请求 {} | input {} output {} cw {} cr {} | cost {:.6} | unknown={} | label={:?}",
            g.key,
            g.requests,
            g.tokens.input,
            g.tokens.output,
            g.tokens.cache_write,
            g.tokens.cache_read,
            g.cost_usd,
            g.unknown_pricing,
            g.label
        );
        let l = query::query_events(
            &snap.query_id,
            &EventFilter {
                project: Some(g.key.clone()),
                limit: Some(1000),
                ..Default::default()
            },
        )
        .unwrap();
        let mut by_model: BTreeMap<String, u64> = BTreeMap::new();
        let mut by_agent: BTreeMap<&str, u64> = BTreeMap::new();
        for row in &l.rows {
            *by_model.entry(row.model.clone()).or_default() += 1;
            *by_agent.entry(row.agent).or_default() += 1;
        }
        println!("    明细 total={} 返回={}", l.total, l.rows.len());
        println!("    来源分布：{by_agent:?}");
        println!("    模型分布：");
        for (m, n) in &by_model {
            println!("      {m}: {n}");
        }
    }

    let rm = summary(&SummaryOptions {
        by: GroupBy::Model,
        ..opts.clone()
    })
    .unwrap();
    let mut models: Vec<(&str, u64, Option<&str>, bool)> = rm
        .groups
        .iter()
        .filter(|g| g.key != "合计")
        .map(|g| {
            (
                g.key.as_str(),
                g.requests,
                g.label.as_deref(),
                g.unknown_pricing,
            )
        })
        .collect();
    models.sort_by_key(|m| std::cmp::Reverse(m.1));
    println!("模型维度共 {} 个模型（前 30 按请求数）：", models.len());
    for (key, req, label, unknown) in models.iter().take(30) {
        println!("  {key} | 请求 {req} | label {label:?} | unknown={unknown}");
    }
    let mimo: Vec<String> = models
        .iter()
        .filter(|(key, _, label, _)| {
            key.to_lowercase().contains("mimo")
                || label.is_some_and(|l| l.to_lowercase().contains("mimo"))
        })
        .map(|(key, req, label, _)| format!("{key} ({label:?}) 请求 {req}"))
        .collect();
    println!("模型维度里含 mimo 的分组：{} 个 {:?}", mimo.len(), mimo);
    if let Some(t) = rm.groups.iter().find(|g| g.key == "合计") {
        println!("全部数据合计请求：{}", t.requests);
    }

    std::fs::remove_dir_all(&dir).ok();
}
