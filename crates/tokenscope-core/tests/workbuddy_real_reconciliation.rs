//! WB08：真实数据只读对账（本机本地，#[ignore] + 显式环境变量门控）。
//!
//! 只读运行 WorkBuddy 解析器，分别读取真实 `~/.workbuddy/projects` 与
//! `~/.workbuddy-ai/projects`，输出每应用的 files/lines/bad_lines/skipped/
//! events 与四桶合计到指定目录；不打开 workbuddy.db、traces、file-history、
//! models.json，不写任何 WorkBuddy 目录，不触碰 `~/.tokenscope`。
//! 与独立 Python 脚本（qa-artifacts/workbuddy/reconcile_python.py）逐应用比对。
//!
//! 运行（本机本地，不进 CI）：
//! ```text
//! TOKENSCOPE_WB_RECONCILE=1 \
//! TOKENSCOPE_WB_RECONCILE_OUT=<隔离目录> \
//! python -B scripts/run_rust_tests.py -- cargo test --release -p tokenscope \
//!   --test workbuddy_real_reconciliation -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use tokenscope::model::AgentKind;
use tokenscope::source::workbuddy::WorkBuddySource;
use tokenscope::source::{CollectStats, Source};

#[test]
#[ignore = "需要 TOKENSCOPE_WB_RECONCILE=1 与输出目录；只读真实 WorkBuddy 数据，仅本机对账使用"]
fn workbuddy_real_data_reconciliation() {
    if std::env::var("TOKENSCOPE_WB_RECONCILE").ok().as_deref() != Some("1") {
        panic!("必须显式设置 TOKENSCOPE_WB_RECONCILE=1 才允许读取真实 WorkBuddy 目录");
    }
    let out_dir = std::env::var("TOKENSCOPE_WB_RECONCILE_OUT")
        .expect("必须设置 TOKENSCOPE_WB_RECONCILE_OUT（隔离证据目录）");
    let out_dir = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir).unwrap();

    let mut apps = Vec::new();
    for (label, kind) in [
        ("workbuddy", AgentKind::WorkBuddy),
        ("workbuddy-ai", AgentKind::WorkBuddyAi),
    ] {
        let root = WorkBuddySource::default_root(kind).unwrap();
        let source = WorkBuddySource::new(&root, kind);
        let mut stats = CollectStats::default();
        let mut events: Vec<(u64, u64, u64, u64)> = Vec::new();
        let mut event_count = 0u64;
        let (files, errors) = source.discover_with_errors();
        assert!(errors.is_empty(), "{label} 发现层异常: {errors:?}");
        for f in &files {
            stats.files_scanned += 1;
            let parsed = source.parse_file(f);
            stats.add_file(&parsed);
            for e in &parsed.events {
                event_count += 1;
                events.push((
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_write_tokens,
                    e.cache_read_tokens,
                ));
            }
        }
        let mut totals = [0u64; 4];
        for (i, o, cw, cr) in &events {
            totals[0] += i;
            totals[1] += o;
            totals[2] += cw;
            totals[3] += cr;
        }
        // add_file 不维护 events（调用方职责）；event_count 即解析产出事件数。
        let report = serde_json::json!({
            "app": label,
            "source": "rust-workbuddy-parser",
            "files_scanned": stats.files_scanned,
            "lines_seen": stats.lines_seen,
            "bad_lines": stats.bad_lines,
            "skipped_no_model": stats.skipped_no_model,
            "io_errors": stats.io_errors,
            "events": event_count,
            "input_tokens": totals[0],
            "output_tokens": totals[1],
            "cache_write_tokens": totals[2],
            "cache_read_tokens": totals[3],
        });
        apps.push(report);
        println!(
            "{label}: events={} in={} out={} cw={} cr={}",
            event_count, totals[0], totals[1], totals[2], totals[3]
        );
    }
    let out = serde_json::json!({
        "schema_version": 1,
        "apps": apps,
    });
    let path = out_dir.join("rust-reconciliation.json");
    std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap() + "\n").unwrap();
    println!("证据已写入 {}", path.display());
}
