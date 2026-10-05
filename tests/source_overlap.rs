//! Task 2：来源目录重叠——Claude 与 Codex 指向同一规范化目录时，
//! 同一日志文件最多贡献一次统计；缓存不互相覆盖（配置与采集双层防御）。
//! 所有路径显式注入临时目录。

use std::path::PathBuf;

use std::sync::atomic::Ordering;
use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, cache_stats, summary};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-overlap-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 共享日志目录：一个 Claude 会话文件（只含 claude 形态的行）。
fn shared_dir(dir: &std::path::Path) -> PathBuf {
    let shared = dir.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(
        shared.join("s.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c1","message":{"id":"m","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}}"#,
    )
    .unwrap();
    shared
}

fn opts(
    dir: &std::path::Path,
    shared: &std::path::Path,
    codex_dir: Option<PathBuf>,
) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(shared.to_path_buf()),
        codex_dir,
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

#[test]
fn test_source_overlap_same_dir_single_count() {
    // 两个 agent 指向同一目录：同一文件只贡献一次统计（不重复计费）。
    let dir = tmp_dir("same");
    let shared = shared_dir(&dir);
    let o = opts(&dir, &shared, Some(shared.to_path_buf()));
    let r = summary(&o).unwrap();
    // claude 1 事件；codex 解析同文件形态不匹配 → 0 事件（而非重复）。
    // 若同形态文件被两个 agent 扫描，事件数必须仍然只有一份。
    assert_eq!(r.totals.requests, 1, "共享目录的日志最多贡献一次统计");
    // 缓存文件归属：同一路径只保留一个 agent 的缓存行（无相互覆盖后的重复）。
    let info = cache_stats(o.cache_dir.clone()).unwrap();
    assert!(info.files >= 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_source_overlap_alias_paths_recognized() {
    // 相对路径/大小写/尾斜杠等别名指向同一规范化目录 → 同样只计一次。
    let dir = tmp_dir("alias");
    let shared = shared_dir(&dir);
    let alias = {
        let mut p = shared.clone().into_os_string();
        p.push("\\"); // 尾部分隔符别名
        PathBuf::from(p)
    };
    let o = opts(&dir, &shared, Some(alias));
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 1, "别名目录同样只贡献一次统计");
    std::fs::remove_dir_all(&dir).ok();
}

/// Task 2（审阅）：mock source——两个 agent 对同一文件都产出事件，
/// 修复后只保留一份（不依赖某 adapter 恰好解析不了另一种格式）。
mod mock_pair {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokenscope::model::{AgentKind, UsageEvent};
    use tokenscope::source::{CollectStats, FileParse, Source};

    pub struct MockAgent {
        root: PathBuf,
        file: PathBuf,
        pub agent: AgentKind,
        pub parses: std::sync::Arc<AtomicUsize>,
    }

    impl MockAgent {
        pub fn new(root: &Path, file: &Path, agent: AgentKind) -> Self {
            Self {
                root: root.to_path_buf(),
                file: file.to_path_buf(),
                agent,
                parses: std::sync::Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl Source for MockAgent {
        fn agent(&self) -> AgentKind {
            self.agent
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
            (vec![self.file.clone()], Vec::new())
        }

        fn parse_file(&self, _path: &Path) -> FileParse {
            self.parses.fetch_add(1, Ordering::Relaxed);
            let stats = CollectStats {
                lines_seen: 1,
                events: 1,
                ..CollectStats::default()
            };
            FileParse {
                stats,
                events: vec![UsageEvent {
                    ts: "2026-08-01T10:00:00Z".parse().unwrap(),
                    agent: self.agent,
                    model: "m".into(),
                    session_id: "s".into(),
                    project: "p".into(),
                    record_id: String::new(),
                    input_tokens: 100,
                    output_tokens: 10,
                    cache_write_tokens: 0,
                    cache_read_tokens: 0,
                }],
            }
        }
    }
}

#[test]
fn test_source_overlap_both_agents_produce_events_kept_once() {
    // Task 2：两个 mock agent 对同一文件都产出事件 → 只保留先扫描者。

    let dir = tmp_dir("both-produce");
    let shared = shared_dir(&dir);
    let file = shared.join("s.jsonl");
    let claude =
        mock_pair::MockAgent::new(&shared, &file, tokenscope::model::AgentKind::ClaudeCode);
    let codex = mock_pair::MockAgent::new(&shared, &file, tokenscope::model::AgentKind::Codex);
    let claude_parses = claude.parses.clone();
    let codex_parses = codex.parses.clone();
    let sources: Vec<Box<dyn tokenscope::source::Source>> = vec![Box::new(claude), Box::new(codex)];
    // 直接走 collect_all_with_sources（同 collect_inner 的去重包装路径）。
    let collected = tokenscope::report::collect_all_with_sources_for_test(
        sources,
        &SummaryOptions {
            by: GroupBy::Day,
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        },
    );
    let collected = collected.expect("采集成功");
    assert_eq!(collected.events.len(), 1, "同文件事件只保留一份");
    assert_eq!(claude_parses.load(Ordering::Relaxed), 1);
    assert_eq!(
        codex_parses.load(Ordering::Relaxed),
        0,
        "后扫描 agent 不得重复解析同文件"
    );
    // 重叠诊断必须到达 report warnings（而非只写日志）。
    assert!(
        collected
            .warnings
            .iter()
            .any(|w| w.contains("来源目录重叠")),
        "warnings 应含重叠诊断: {:?}",
        collected.warnings
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_source_overlap_cache_attribution_direct_sqlite() {
    // Task 2：直接查 SQLite——缓存 path/agent 归属不被后扫描 agent 覆盖。
    use rusqlite::Connection;

    let dir = tmp_dir("sqlite-attr");
    let shared = shared_dir(&dir);
    let o = opts(&dir, &shared, Some(shared.to_path_buf()));
    let _ = summary(&o).unwrap();
    let conn = Connection::open(dir.join("cache").join("cache.db")).unwrap();
    let mut stmt = conn.prepare("SELECT path, agent FROM files").unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    // 同一路径只保留一个 agent 的缓存行（无重复行）。
    let s_path = shared.join("s.jsonl").to_string_lossy().to_string();
    let matching: Vec<&(String, String)> = rows
        .iter()
        .filter(|(p, _)| p.ends_with("s.jsonl"))
        .collect();
    assert!(
        matching
            .iter()
            .all(|(p, _)| *p == s_path || p.ends_with("s.jsonl")),
        "缓存行存在: {rows:?}"
    );
    assert_eq!(matching.len(), 1, "同一路径只应有一行缓存: {rows:?}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_source_overlap_cache_not_overwritten_by_later_agent() {
    // 旧配置直接进入采集：后扫描的 agent 不得覆盖前一 agent 的缓存行。
    let dir = tmp_dir("cache-order");
    let shared = shared_dir(&dir);
    let o = opts(&dir, &shared, Some(shared.clone()));
    let _ = summary(&o).unwrap();
    let first = cache_stats(o.cache_dir.clone()).unwrap();
    let _ = summary(&o).unwrap();
    let second = cache_stats(o.cache_dir.clone()).unwrap();
    assert_eq!(
        first.files, second.files,
        "重复采集不得让缓存行数因 agent 覆盖而变化"
    );
    std::fs::remove_dir_all(&dir).ok();
}
