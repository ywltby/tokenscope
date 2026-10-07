//! 计划 B3（test_partial_cost_totals）：部分计价在聚合层的端到端对账——
//! 已知分项入费用、未知分项 token 单列进 unknown_tokens 且 † 标记生效；
//! 合计 = 分组之和，与明细行展示同源。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

/// SF04：一次性明细读取 = 建会话 + 读一次（本文件用例不跨调用翻页）。
fn list_events(
    opts: &SummaryOptions,
    filter: &EventFilter,
) -> anyhow::Result<tokenscope::report::EventList> {
    let snap = tokenscope::query::begin_query(opts)?;
    tokenscope::query::query_events(&snap.query_id, filter)
}

fn fixture(agent: &str, p: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(agent)
        .join(p)
}

#[test]
fn test_partial_cost_totals() {
    let dir = std::env::temp_dir().join(format!("tokenscope-b3-totals-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // v2 快照：模型只有 input/output 价（cache 分项未知）。
    // 手算：codex fixture 07-18 的 gpt-5.6-sol 事件
    //   input 10 / output 5 / cache 0 → 全已知，cost = 10*1 + 5*2 = 20/1M；
    // 07-17 的 gpt-5.6-sol（750/100/cw50/cr200，B1 口径）：
    //   已知部分 = 750*1 + 100*2 = 950/1M；未知分项 cache_write 50 + cache_read 200。
    // claude fixture 的 tencent/hy3:free 不在快照 → 全 unknown（原有语义）。
    let snap = dir.join("pricing-modelsdev.json");
    std::fs::write(
        &snap,
        r#"{"v":2,"synced_at":"t","entries":[
            {"id":"prov/gpt56","name":null,"input":1.0,"output":2.0}
        ]}"#,
    )
    .unwrap();

    let opts = SummaryOptions {
        by: GroupBy::Day,
        agent: Some(AgentKind::Codex),
        claude_dir: Some(dir.join("no-claude")),
        codex_dir: Some(fixture("codex", "basic")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(snap.clone()),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };

    // 快照的 id 是 prov/gpt56，fixture 模型是 gpt-5.6-sol——改用精确前缀
    // 命中：gpt-5.6-sol 归一化后命中该条目（前缀必须匹配模型名）。
    // 上面的快照 id 不命中，改写为 gpt-5.6-sol。
    std::fs::write(
        &snap,
        r#"{"v":2,"synced_at":"t","entries":[
            {"id":"gpt-5.6-sol","name":null,"input":1.0,"output":2.0}
        ]}"#,
    )
    .unwrap();
    // 指纹变化后索引/加载自动重建，直接重跑即可。
    let r = summary(&opts).unwrap();

    // 07-17 组：gpt-5.6-sol 事件已知部分 950/1M + …该日 codex 仅此一条。
    let d1 = r.groups.iter().find(|g| g.key == "2026-07-17").unwrap();
    assert!((d1.cost_usd - 950.0 / 1_000_000.0).abs() < 1e-9, "{d1:?}");
    assert!(d1.unknown_pricing, "部分计价必须标记 †");
    assert_eq!(d1.unknown_tokens.cache_write, 50);
    assert_eq!(d1.unknown_tokens.cache_read, 200);
    assert_eq!(d1.unknown_tokens.input, 0);
    assert_eq!(d1.unknown_tokens.output, 0);

    // 07-18 组：gpt-5.6-sol 走快照层（缺 cache 键 → 部分计价，†）已知部分
    // 20/1M；gpt-5.5（115/1M）与 gpt-5.4（950/1M）由内置层完整兜底。
    let d2 = r.groups.iter().find(|g| g.key == "2026-07-18").unwrap();
    // Task 1 后无内置兜底：e2 走快照（缺 cache 价但 0 token → 完整），
    // e4/e6（gpt-5.5/5.4）无任何来源 → 全 unknown。
    assert!(d2.unknown_pricing);
    assert!(
        (d2.cost_usd - 20.0 / 1_000_000.0).abs() < 1e-9,
        "d2.cost={}",
        d2.cost_usd
    );
    assert_eq!(d2.unknown_tokens.input, 85);
    assert_eq!(d2.unknown_tokens.output, 53);
    assert_eq!(d2.unknown_tokens.cache_read, 22);
    assert_eq!(d2.unknown_tokens.cache_write, 0);

    // 合计 = 分组之和；unknown 同样汇总（仅 d1 的快照缺键分项）。
    assert!((r.totals.cost_usd - (950.0 + 20.0) / 1_000_000.0).abs() < 1e-9);
    assert!(r.totals.unknown_pricing);
    assert_eq!(r.totals.unknown_tokens.cache_write, 50);
    assert_eq!(r.totals.unknown_tokens.cache_read, 222);
    assert_eq!(r.totals.unknown_tokens.input, 85);
    assert_eq!(r.totals.unknown_tokens.output, 53);

    // 明细同源：行费用 = 已计价小计。
    let l = list_events(&opts, &EventFilter::default()).unwrap();
    let row = l
        .rows
        .iter()
        .find(|x| x.ts.starts_with("2026-07-17"))
        .unwrap();
    assert!((row.cost_usd.unwrap() - 950.0 / 1_000_000.0).abs() < 1e-9);

    std::fs::remove_dir_all(&dir).ok();
}
