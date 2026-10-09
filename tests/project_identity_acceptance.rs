//! A06 阶段 A 验收（隔离路径）：项目身份统一的升级与复用。
//!
//! 全部使用隔离的来源/缓存/配置/价格目录与合成日志，不触碰真实
//! `~/.tokenscope`、`~/.claude.json` 与真实 agent 日志。覆盖计划 §3 A06 的
//! 「v6 缓存首次升级重解析、随后热命中、两次结果一致」，以及同项目一行、
//! 展示名正常、下钻含两侧事件。

use std::path::{Path, PathBuf};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::TokenCounts;
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, cache_stats, summary};

fn tmp() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("tokenscope-a06-accept-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 隔离来源：Claude 3 条（带 cwd）+ Codex 3 条（同一路径）。
fn write_sources(dir: &Path) -> (PathBuf, PathBuf) {
    let claude = dir.join("sources").join("claude").join("alpha");
    std::fs::create_dir_all(&claude).unwrap();
    let mut body = String::new();
    for (i, id) in ["c-m1", "c-m2", "c-m3"].iter().enumerate() {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-08-01T10:0{i}:00.000Z","sessionId":"c1","cwd":"C:\\work\\alpha","message":{{"id":"{id}","model":"claude-sonnet-4-5","usage":{{"input_tokens":100,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(claude.join("s.jsonl"), body).unwrap();

    let codex = dir.join("sources").join("codex");
    let day = codex.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    let mut cbody = String::new();
    cbody.push_str(
        r#"{"timestamp":"2026-08-01T12:00:00.000Z","type":"session_meta","payload":{"id":"x","session_id":"x","cwd":"C:/work/alpha"}}"#,
    );
    cbody.push('\n');
    cbody.push_str(
        r#"{"timestamp":"2026-08-01T12:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/work/alpha"}}"#,
    );
    cbody.push('\n');
    for i in 0..3u64 {
        let input = 50 * (i + 1);
        let output = 5 * (i + 1);
        let line = serde_json::json!({
            "timestamp": format!("2026-08-01T12:1{i}:00.000Z"),
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": input,
                        "output_tokens": output,
                        "cached_input_tokens": 0,
                        "cache_write_input_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": input + output,
                    }
                }
            }
        });
        cbody.push_str(&line.to_string());
        cbody.push('\n');
    }
    std::fs::write(day.join("rollout-a.jsonl"), cbody).unwrap();
    (dir.join("sources").join("claude"), codex)
}

fn opts(dir: &Path, claude: &Path, codex: &Path) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(claude.to_path_buf()),
        codex_dir: Some(codex.to_path_buf()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("openrouter.json")),
        modelsdev_path: Some(dir.join("modelsdev.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 模拟升级前的缓存：把解析版本改回指定值（仅测试使用）。
fn downgrade_schema_version(db: &Path, version: &str) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
        [version],
    )
    .unwrap();
}

fn snapshot_tuple(
    r: &tokenscope::report::SummaryReport,
) -> (u64, TokenCounts, f64, bool, TokenCounts) {
    (
        r.totals.requests,
        r.totals.tokens,
        r.totals.cost_usd,
        r.totals.unknown_pricing,
        r.totals.unknown_tokens,
    )
}

#[test]
fn project_identity_upgrade_reparses_and_stays_consistent() {
    let dir = tmp();
    let (claude, codex) = write_sources(&dir);
    let o = opts(&dir, &claude, &codex);

    // 1) 首轮：同一路径的跨工具会话归入同一项目，展示名正常。
    let first = summary(&o).unwrap();
    assert_eq!(first.groups.len(), 2, "一行项目 + 合计：{:?}", first.groups);
    assert_eq!(first.groups[0].key, "C:/work/alpha");
    assert_eq!(first.groups[0].label.as_deref(), Some("alpha"));
    assert_eq!(first.groups[0].requests, 6);
    let mut agents = first.groups[0].agents.clone();
    agents.sort();
    assert_eq!(agents, ["claude-code", "codex"]);
    let after_first = cache_stats(o.cache_dir.clone()).unwrap();
    assert!(after_first.events > 0, "首轮必须写入缓存");

    // 2) 模拟升级（旧版本缓存）→ 首次打开整体失效重解析，结果逐字段一致。
    downgrade_schema_version(&dir.join("cache").join("cache.db"), "6");
    let second = summary(&o).unwrap();
    assert_eq!(
        snapshot_tuple(&second),
        snapshot_tuple(&first),
        "升级重解析后必须与升级前逐字段一致"
    );
    let after_upgrade = cache_stats(o.cache_dir.clone()).unwrap();
    assert_eq!(
        after_upgrade.events, after_first.events,
        "升级重解析必须与升级前得到同一事件集"
    );

    // 3) 再次查询（热命中路径）：结果与首轮逐字段一致。
    let third = summary(&o).unwrap();
    assert_eq!(third.groups.len(), 2);
    assert_eq!(third.groups[0].key, "C:/work/alpha");
    assert_eq!(third.groups[0].requests, 6);
    assert_eq!(snapshot_tuple(&third), snapshot_tuple(&first));

    // 4) 下钻闭环：合并项目里两侧事件都在，key 精确回传。
    let snap = query::begin_query(&o).unwrap();
    let l = query::query_events(
        &snap.query_id,
        &EventFilter {
            project: Some("C:/work/alpha".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(l.total, 6);
    assert!(l.rows.iter().all(|r| r.project == "C:/work/alpha"));
    let mut seen: Vec<&str> = l.rows.iter().map(|r| r.agent).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen, ["claude-code", "codex"]);
}
