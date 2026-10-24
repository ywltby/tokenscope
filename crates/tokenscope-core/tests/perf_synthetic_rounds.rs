//! PR3 §4.4 性能取证（合成来源 + 隔离数据目录，不设硬门槛）：
//! 改前/改后用同一份合成树对比冷/热轮耗时，中位数记入计划 §7 S06 执行记录。
//! 真实数据基准（`perf_real_data`）按计划单列待验，本文件绝不触真实 `~/.tokenscope`。
//!
//! 共享约束：本文件在改前基线 worktree 中同样编译运行（§9.0 第 2 条"相同条件比较"），
//! 因此热轮计时只用改前/改后共有的 `SummaryOptions` 注入字段。

use std::path::{Path, PathBuf};
use std::time::Instant;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, summary};

/// 数据规模：4 个项目 slug × 10 个文件 × 300 事件 = 12 000 条合成请求，
/// 时间跨 5 个自然日（Day 粒度聚合非平凡）。
const SLUGS: usize = 4;
const FILES_PER_SLUG: usize = 10;
const EVENTS_PER_FILE: usize = 300;
const TOTAL_EVENTS: u64 = (SLUGS * FILES_PER_SLUG * EVENTS_PER_FILE) as u64;

fn hermetic_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "tokenscope-perf-synthetic-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn claude_event(slug: usize, file: usize, event: usize) -> String {
    let day = 1 + (event % 5);
    let id = format!("msg-{slug}-{file}-{event}");
    format!(
        concat!(
            r#"{{"type":"assistant","sessionId":"perf-{s}-{f}","timestamp":"2026-09-{d:02}T10:{mm:02}:00.000Z","#,
            r#""cwd":"C:/perf/work{s}","message":{{"id":"{id}","model":"claude-sonnet-4-5","#,
            r#""usage":{{"input_tokens":100,"output_tokens":40,"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}}}}"#
        ),
        s = slug,
        f = file,
        d = day,
        mm = event % 60,
        id = id,
    )
}

fn write_synthetic_tree(claude_root: &Path) {
    for slug in 0..SLUGS {
        let slug_dir = claude_root.join(format!("perf-project-{slug}"));
        std::fs::create_dir_all(&slug_dir).unwrap();
        for file in 0..FILES_PER_SLUG {
            let mut body = String::new();
            for event in 0..EVENTS_PER_FILE {
                body.push_str(&claude_event(slug, file, event));
                body.push('\n');
            }
            std::fs::write(slug_dir.join(format!("session-{file}.jsonl")), body).unwrap();
        }
    }
}

/// 冷轮（全量解析入库）一次 + 热轮（缓存命中）三次；打印全部耗时与中位数。
/// 断言冷/热请求数一致（缓存纯优化不变量）且等于合成规模。
#[test]
fn synthetic_cold_then_three_warm_rounds() {
    let dir = hermetic_dir("rounds");
    let claude = dir.join("claude").join("projects");
    write_synthetic_tree(&claude);
    let pricing = dir.join("pricing.toml");
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
    let opts = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Day,
        agent: Some(AgentKind::ClaudeCode),
        claude_dir: Some(claude),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(pricing),
        // 固定指向不存在的快照：不读、不写真实 ~/.tokenscope
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        ..Default::default()
    };

    let t = Instant::now();
    let cold = summary(&opts).expect("冷轮汇总失败");
    let cold_ms = t.elapsed().as_millis();
    assert_eq!(
        cold.totals.requests, TOTAL_EVENTS,
        "冷轮请求数应等于合成规模"
    );

    let mut warm_ms = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let warm = summary(&opts).expect("热轮汇总失败");
        warm_ms.push(t.elapsed().as_millis());
        assert_eq!(warm.totals.requests, TOTAL_EVENTS, "冷/热请求数必须一致");
    }
    warm_ms.sort();
    println!(
        "数据规模：{} 文件 / {} 请求 / 5 天；冷轮 {cold_ms} ms；热轮 {:?} ms（中位数 {} ms）",
        SLUGS * FILES_PER_SLUG,
        TOTAL_EVENTS,
        warm_ms,
        warm_ms[1],
    );
}

/// 来源解析单独计时（§4.4：预期毫秒级）。改前代码没有集合解析入口，
/// 本用例只在改后分支存在，结果只作记录、不作跨版本对比。
#[test]
fn source_resolution_latency_recorded() {
    use tokenscope::settings::SourceConfig;
    use tokenscope::source::discovery::{
        DiscoveryContext, StartupEnv, filesystem_probe, resolve_sources,
    };

    let home = hermetic_dir("resolve");
    // 探测真实文件系统：默认根不存在（Missing 分类）也是一次真实 canonicalize。
    let ctx = DiscoveryContext::for_test(home, StartupEnv::default(), filesystem_probe());
    let claude = SourceConfig::default();
    let codex = SourceConfig::default();

    let mut samples = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let resolved = resolve_sources(
            &ctx,
            &claude,
            &codex,
            &SourceConfig::default(),
            &SourceConfig::default(),
        );
        samples.push(t.elapsed().as_micros());
        assert!(!resolved.roots.is_empty(), "默认三根至少应解析出候选");
    }
    samples.sort();
    println!(
        "来源解析（默认候选，真实探测）5 次耗时（µs）：{:?}（中位数 {} µs）",
        samples, samples[2],
    );
}
