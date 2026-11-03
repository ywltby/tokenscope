//! N18 隔离端到端功能验收（PR #53 合并前收口，2026-11-03）：
//! 真实解析链 Claude JSONL → 用量事件 → 历史库/汇总 → cost_breakdown 明细。
//!
//! 场景 1（已知分档）：5 分钟/1 小时各 100 万 token，隔离价格 $3.75/$6——
//! 写费合计 $9.75，明细拆 `cache_write_5m`/`cache_write_1h` 两行，写入总量
//! 200 万且不重复计入（分项之和 = 总量，不再有合并行）。
//! 场景 2（分档未知）：事件只有写入总量——外置候选声明分档价时按合并价
//! 估算并带 `cache_write_split_estimated` 标示（complete=false），不冒充精确、
//! 也不显示为未计价；候选未声明分档时保持合并价完整估算（既有兼容口径）。

use std::path::{Path, PathBuf};
use tokenscope::model::AgentKind;
use tokenscope::pricing::CostLineKind;
use tokenscope::query;
use tokenscope::report::{EventFilter, SourcePlan, SummaryOptions};

/// 隔离价格表：分档候选（5m=$3.75 / 1h=$6）+ 未声明分档候选（兼容口径对照）。
const PRICING_TOML: &str = r#"
# N18 验收专用合成价格表
[[model]]
prefix = "external/opus-tiers"
input = 1.0
output = 2.0
cache_write = 3.0
cache_read = 0.1
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model]]
prefix = "external/opus-nodrive"
input = 1.0
output = 2.0
cache_write = 1.5
cache_read = 0.1
"#;

/// 合成 Claude JSONL：input 1000 / output 100 / 缓存写总量 200 万 / 缓存读 50 万。
/// `cache_creation` 段由调用方给出（含分档键或空对象），模型名可替换。
fn session_line(cache_creation: &str, model: &str) -> String {
    let cc = if cache_creation == "__NONE__" {
        String::new()
    } else {
        format!(r#","cache_creation":{}"#, cache_creation)
    };
    r#"{"type":"assistant","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","timestamp":"2026-10-06T01:00:00.000Z","cwd":"C:\\acceptance\\workspace\\pricing","message":{"id":"m-n18","model":"__MODEL__","usage":{"input_tokens":1000,"output_tokens":100,"cache_creation_input_tokens":2000000,"cache_read_input_tokens":500000__CC__}}}"#
        .replace("__MODEL__", model)
        .replace("__CC__", &cc)
}

fn setup(dir: &Path, cache_creation: &str, model: &str) {
    std::fs::create_dir_all(
        dir.join("projects")
            .join("-acceptance-workspace-tokenscope"),
    )
    .unwrap();
    std::fs::write(dir.join("pricing.toml"), PRICING_TOML).unwrap();
    let line = session_line(cache_creation, model);
    std::fs::write(
        dir.join("projects")
            .join("-acceptance-workspace-tokenscope")
            .join("diag.jsonl"),
        line + "\n",
    )
    .unwrap();
}

fn begin(dir: &Path) -> std::sync::Arc<tokenscope::query::QuerySnapshot> {
    let opts = SummaryOptions {
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        claude_dir: Some(dir.join("unused-claude")),
        codex_dir: Some(dir.join("unused-codex")),
        claude_projects_path: Some(dir.join("unused-claude.json")),
        cache_dir: Some(dir.join("cache")),
        history_path: Some(dir.join("history.db")),
        pricing_path: Some(dir.join("pricing.toml")),
        pricing_index: Some(dir.join("pricing-index.json")),
        openrouter_path: Some(dir.join("no-openrouter.json")),
        modelsdev_path: Some(dir.join("no-modelsdev.json")),
        tz: Some("Asia/Shanghai".to_string()),
        source_plans: Some(vec![SourcePlan {
            agent: AgentKind::ClaudeCode,
            root: dir.join("projects"),
            claude_mapping: None,
        }]),
        ..Default::default()
    };
    query::begin_query_with(&opts, true).expect("begin_query")
}

fn write_lines(bd: &tokenscope::report::EventCostBreakdown) -> Vec<&tokenscope::pricing::CostLine> {
    bd.lines
        .iter()
        .filter(|l| {
            matches!(
                l.kind,
                CostLineKind::CacheWrite | CostLineKind::CacheWrite5m | CostLineKind::CacheWrite1h
            )
        })
        .collect()
}

/// 场景 1：已知分档（5m/1h 各 100 万，$3.75/$6）。
/// 两档明细行、写费合计 $9.75、写总量 200 万且不重复计入。
#[test]
fn n18_e2e_known_tiers_two_lines_and_total() {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "n18-e2e-known-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    setup(
        &dir,
        r#"{"ephemeral_5m_input_tokens":1000000,"ephemeral_1h_input_tokens":1000000}"#,
        "external/opus-tiers",
    );
    let snap = begin(&dir);
    let page = query::query_events(&snap.query_id, &EventFilter::default()).expect("events");
    assert_eq!(page.rows.len(), 1);
    let bd = page.rows[0]
        .cost_breakdown
        .as_ref()
        .expect("分档可信 + 候选声明分档 → 必有明细");

    let writes = write_lines(bd);
    assert_eq!(
        writes.iter().map(|l| l.kind).collect::<Vec<_>>(),
        vec![CostLineKind::CacheWrite5m, CostLineKind::CacheWrite1h],
        "拆成两行，无合并 CacheWrite 行"
    );
    let total_tokens: u64 = writes.iter().map(|l| l.tokens).sum();
    assert_eq!(total_tokens, 2_000_000, "两档之和=写入总量，无重复计入");
    assert_eq!(writes[0].unit_price, Some(3.75), "5m 档单价 $3.75");
    assert_eq!(writes[1].unit_price, Some(6.0), "1h 档单价 $6");
    let write_cost: f64 = writes.iter().map(|l| l.subtotal).sum();
    assert!(
        (write_cost - 9.75).abs() < 1e-9,
        "写费合计应为 $9.75，实际 {write_cost}"
    );
    assert!(bd.complete, "两档齐且可信 → 完整计价");
    assert!(!bd.cache_write_split_estimated, "分档已知时不得标估算");
    std::fs::remove_dir_all(&dir).ok();
}

/// 场景 2：分档未知（事件只有写总量）。
/// 按合并价估算 + `cache_write_split_estimated` 标示（complete=false）——
/// 已计价、不冒充精确、也不是未计价。
#[test]
fn n18_e2e_unknown_tiers_estimated_with_marker() {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "n18-e2e-unknown-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    setup(&dir, "{}", "external/opus-tiers");
    let snap = begin(&dir);
    let page = query::query_events(&snap.query_id, &EventFilter::default()).expect("events");
    assert_eq!(page.rows.len(), 1);
    let row = &page.rows[0];
    assert_eq!(row.cache_write, 2_000_000, "总量保留");
    assert!(
        row.cache_write_5m.is_none() && row.cache_write_1h.is_none(),
        "事件无分档键 → 分项未知（None，不当 0）"
    );
    let bd = row.cost_breakdown.as_ref().expect("合并价可估 → 有明细");
    assert!(
        bd.cache_write_split_estimated,
        "分档未知 + 候选声明分档 + 合并价计入 → 必须标估算"
    );
    assert!(!bd.complete, "估算不是完整精确计价");
    let writes = write_lines(bd);
    assert_eq!(writes.len(), 1, "只有合并写行");
    let write = writes[0];
    assert!(matches!(write.kind, CostLineKind::CacheWrite));
    assert_eq!(write.tokens, 2_000_000, "写总量不重复、不丢失");
    assert_eq!(write.unit_price, Some(3.0), "按该候选合并写价 $3 估算");
    assert!(write.priced, "已按合并价估算 → 已计价（不是未知/未计价）");
    assert!((write.subtotal - 6.0).abs() < 1e-9, "估算金额 = $6");
    std::fs::remove_dir_all(&dir).ok();
}

/// 对照：全链路（事件+候选）都无分档声明 → 既有兼容口径
/// （合并价、complete=true、无估算标志）。
#[test]
fn n18_e2e_unknown_tiers_nodeclared_keeps_legacy_complete() {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "n18-e2e-legacy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    setup(&dir, "{}", "external/opus-nodrive");
    let snap = begin(&dir);
    let page = query::query_events(&snap.query_id, &EventFilter::default()).expect("events");
    assert_eq!(page.rows.len(), 1);
    let bd = page.rows[0].cost_breakdown.as_ref().expect("应有明细");
    assert!(
        !bd.cache_write_split_estimated,
        "全链路无分档声明 → 无估算标志（既有口径）"
    );
    assert!(bd.complete, "既有兼容口径：合并价完整计价");
    let writes = write_lines(bd);
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].unit_price, Some(1.5), "未声明分档候选的合并写价");
    std::fs::remove_dir_all(&dir).ok();
}
