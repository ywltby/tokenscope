//! H04：统一历史库的增量采集语义（计划 §4、§2 不变量 3/4/9）。
//!
//! 覆盖：来源文件删除/截短/替换都不丢已保存用量、追加只增不减、停用来源
//! 停止新采集但历史仍可查看、重扫日志只重置指纹不清用量、重复采集幂等。
//! 全部路径在临时目录，注入隔离的数据目录（计划不变量 10）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, rebuild_cache, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-history-collection-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn claude_line(session: &str, id: &str, ts: &str, input: u64, output: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "message": {
            "id": id,
            "model": "claude-sonnet-4-5",
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0,
            }
        }
    })
    .to_string()
}

fn codex_lines(session: &str, ts: &str, input: u64, output: u64) -> Vec<String> {
    vec![
        serde_json::json!({
            "timestamp": "2026-07-17T15:00:00.000Z",
            "type": "session_meta",
            "payload": {"id": session, "session_id": session, "cwd": "/work/codex"},
        })
        .to_string(),
        serde_json::json!({
            "timestamp": "2026-07-17T15:01:00.000Z",
            "type": "turn_context",
            "payload": {"turn_id": "t", "model": "gpt-5.6-sol", "cwd": "/work/codex"},
        })
        .to_string(),
        serde_json::json!({
            "timestamp": ts,
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
        })
        .to_string(),
    ]
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

fn opts(dir: &Path, claude: Option<PathBuf>, codex: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Agent,
        claude_dir: claude.clone(),
        codex_dir: codex.clone(),
        claude_enabled: Some(claude.is_some()),
        codex_enabled: Some(codex.is_some()),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("no-such-openrouter.json")),
        modelsdev_path: Some(dir.join("no-such-modelsdev.json")),
        ..Default::default()
    }
}

#[test]
fn deleting_and_truncating_source_files_never_drops_saved_usage() {
    let dir = temp_dir("keep");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    write_lines(
        &file,
        &[
            claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5),
            claude_line("s1", "m2", "2026-07-17T08:01:00.000Z", 20, 7),
        ],
    );
    let o = opts(&dir, Some(root.clone()), None);

    let first = summary(&o).unwrap();
    assert_eq!(first.totals.requests, 2);
    assert_eq!(first.totals.tokens.total(), 42);

    // 追加一行：只增不减，旧请求不重复计入。
    write_lines(
        &file,
        &[
            claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5),
            claude_line("s1", "m2", "2026-07-17T08:01:00.000Z", 20, 7),
            claude_line("s1", "m3", "2026-07-17T08:02:00.000Z", 1, 1),
        ],
    );
    let appended = summary(&o).unwrap();
    assert_eq!(appended.totals.requests, 3, "追加只增加新请求");
    assert_eq!(appended.totals.tokens.total(), 44);

    // 文件被截短（只剩一行）：已保存的两条请求仍在——旧事实不因源文件
    // 变小而消失（不变量 3）。
    write_lines(
        &file,
        &[claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5)],
    );
    let truncated = summary(&o).unwrap();
    assert_eq!(truncated.totals.requests, 3, "截短不清空已保存的用量");
    assert_eq!(truncated.totals.tokens.total(), 44);

    // 删除整个来源文件：同样保留，来源登记被标记为 missing。
    std::fs::remove_file(&file).unwrap();
    let removed = summary(&o).unwrap();
    assert_eq!(removed.totals.requests, 3, "删除源文件不删历史");
    assert_eq!(removed.totals.tokens.total(), 44);
    let history =
        tokenscope::history::HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let files = history.source_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].state, "missing", "缺失只更新状态");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn replaced_file_keeps_old_requests_and_adds_new_ones() {
    let dir = temp_dir("replace");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    write_lines(
        &file,
        &[claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5)],
    );
    let o = opts(&dir, Some(root.clone()), None);
    assert_eq!(summary(&o).unwrap().totals.requests, 1);

    // 文件被整体替换：同一行序上是另一条请求。旧请求必须保留，新请求正常入账。
    write_lines(
        &file,
        &[claude_line("s2", "n1", "2026-07-17T09:00:00.000Z", 100, 50)],
    );
    let replaced = summary(&o).unwrap();
    assert_eq!(
        replaced.totals.requests, 2,
        "替换后的新请求入账且不覆盖旧请求"
    );
    assert_eq!(replaced.totals.tokens.total(), 15 + 150);

    // 再采一次：完全幂等，不新增。
    let again = summary(&o).unwrap();
    assert_eq!(again.totals.requests, 2);
    assert_eq!(again.totals.tokens.total(), 165);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn disabled_source_stops_new_collection_but_history_stays_visible() {
    let dir = temp_dir("disabled");
    let claude_root = dir.join("projects");
    let codex_root = dir.join("sessions");
    write_lines(
        &claude_root.join("proj-a").join("sess.jsonl"),
        &[claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5)],
    );
    write_lines(
        &codex_root.join("2026").join("07").join("rollout-a.jsonl"),
        &codex_lines("c1", "2026-07-17T15:02:00.000Z", 100, 20),
    );

    let both = opts(&dir, Some(claude_root.clone()), Some(codex_root.clone()));
    let first = summary(&both).unwrap();
    assert_eq!(first.totals.requests, 2);
    assert_eq!(first.sources.len(), 2);

    // 停用 Codex：不再采集它，但已存历史仍可查看（不变量 3）。
    let claude_only = opts(&dir, Some(claude_root.clone()), None);
    let second = summary(&claude_only).unwrap();
    assert_eq!(second.sources.len(), 1, "停用来源零采集、零告警");
    assert_eq!(second.sources[0].agent, AgentKind::ClaudeCode);
    assert_eq!(
        second.totals.requests, 2,
        "已保存的 Codex 历史仍可通过应用筛选查看"
    );

    // 只看 Claude 时按 agent 过滤，Codex 历史不参与。
    let filtered = summary(&SummaryOptions {
        agent: Some(AgentKind::ClaudeCode),
        ..claude_only
    })
    .unwrap();
    assert_eq!(filtered.totals.requests, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rescan_resets_fingerprints_without_deleting_usage() {
    let dir = temp_dir("rescan");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess.jsonl"),
        &[
            claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5),
            claude_line("s1", "m2", "2026-07-17T08:01:00.000Z", 20, 7),
        ],
    );
    let o = opts(&dir, Some(root.clone()), None);
    assert_eq!(summary(&o).unwrap().totals.requests, 2);

    let info = rebuild_cache(&o).unwrap();
    assert_eq!(info.events, 2, "重扫日志只重置指纹，不清用量");
    let after = summary(&o).unwrap();
    assert_eq!(after.totals.requests, 2);
    assert_eq!(after.totals.tokens.total(), 42);

    // 源文件已删除后重扫：历史不因重扫而丢失。
    std::fs::remove_file(root.join("proj-a").join("sess.jsonl")).unwrap();
    rebuild_cache(&o).unwrap();
    assert_eq!(summary(&o).unwrap().totals.requests, 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn repeated_collection_is_idempotent_across_restarts() {
    let dir = temp_dir("restart");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess.jsonl"),
        &[
            claude_line("s1", "m1", "2026-07-17T08:00:00.000Z", 10, 5),
            claude_line("s1", "m2", "2026-07-17T08:01:00.000Z", 20, 7),
        ],
    );
    let o = opts(&dir, Some(root.clone()), None);
    let first = summary(&o).unwrap();

    // 模拟重启：重新打开历史库（新连接）后再次采集。
    let history_path = dir.join("data").join("history.db");
    let history = tokenscope::history::HistoryDb::open(&history_path).unwrap();
    let generation_before = history.generation().unwrap();
    let events_before = history.event_count().unwrap();
    drop(history);

    let second = summary(&o).unwrap();
    assert_eq!(second.totals.requests, first.totals.requests);
    let history = tokenscope::history::HistoryDb::open(&history_path).unwrap();
    assert_eq!(history.event_count().unwrap(), events_before, "不新增事件");
    assert_eq!(
        history.generation().unwrap(),
        generation_before,
        "完全相同的重复采集不递增 generation"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn concurrent_queries_share_one_collection_without_duplicating_usage() {
    let dir = temp_dir("concurrent");
    let root = dir.join("projects");
    for i in 0..8 {
        write_lines(
            &root.join(format!("proj-{i}")).join("sess.jsonl"),
            &[claude_line(
                "s1",
                &format!("m{i}"),
                &format!("2026-07-17T08:0{i}:00.000Z"),
                10,
                5,
            )],
        );
    }
    let o = opts(&dir, Some(root.clone()), None);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    let mut handles = Vec::new();
    for _ in 0..4 {
        let barrier = barrier.clone();
        let o = o.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            summary(&o).unwrap().totals.requests
        }));
    }
    for h in handles {
        assert_eq!(h.join().unwrap(), 8, "并发查询的用量口径一致");
    }
    let history =
        tokenscope::history::HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    assert_eq!(history.event_count().unwrap(), 8, "并发采集不复制事件");
    assert_eq!(history.totals().unwrap().input, 80);
    std::fs::remove_dir_all(&dir).ok();
}
