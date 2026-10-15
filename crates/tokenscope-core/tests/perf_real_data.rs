//! 真实数据性能验收（默认 `#[ignore]`，不进常规套件与提交钩子）。
//!
//! 双用途：
//! a) 冷/热两轮全管线计时（阶段耗时由 collect_all 的 INFO 日志输出）；
//! b) 预热真实 `~/.tokenscope/cache.db`（冷轮即全量解析入库）。
//!
//! 显式运行（在本机有真实日志时）：
//!
//! ```text
//! TOKENSCOPE_REAL_PERF=1 cargo test --release --test perf_real_data -- --ignored --nocapture
//! ```

use std::time::Instant;

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, summary};

#[test]
#[ignore = "真实数据性能验收：需显式设置 TOKENSCOPE_REAL_PERF=1"]
fn real_data_cold_warm_timing() {
    if std::env::var("TOKENSCOPE_REAL_PERF").ok().as_deref() != Some("1") {
        eprintln!("跳过：未设置 TOKENSCOPE_REAL_PERF=1");
        return;
    }
    // 阶段日志打到 stdout（--nocapture 可见）；不装文件 appender，避免与 GUI 日志混写。
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_ansi(false)
        .with_target(true)
        .try_init();

    let opts = || SummaryOptions {
        by: GroupBy::Day,
        ..Default::default()
    };

    let t = Instant::now();
    let cold = summary(&opts()).expect("冷轮汇总失败");
    let cold_ms = t.elapsed().as_millis();

    let t = Instant::now();
    let warm = summary(&opts()).expect("热轮汇总失败");
    let warm_ms = t.elapsed().as_millis();

    // 冷/热数字必须一致（缓存纯优化不变量，在真实数据上钉住；
    // 仅当两轮之间有活跃会话追加日志时才可能不等）。
    assert_eq!(
        cold.totals.requests, warm.totals.requests,
        "冷/热请求总数不一致（缓存命中路径数字必须与全量一致）"
    );
    assert!((cold.totals.cost_usd - warm.totals.cost_usd).abs() < 1e-9);
    assert!(cold.totals.requests > 0, "本机应有真实用量数据");

    println!(
        "冷轮（全量解析入库）: {cold_ms} ms / 热轮（缓存命中）: {warm_ms} ms / 请求 {} / 费用 ${:.4}",
        warm.totals.requests, warm.totals.cost_usd
    );
}
