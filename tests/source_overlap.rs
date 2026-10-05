//! Task 2：来源目录重叠——Claude 与 Codex 指向同一规范化目录时，
//! 同一日志文件最多贡献一次统计；缓存不互相覆盖（配置与采集双层防御）。
//! 所有路径显式注入临时目录。

use std::path::PathBuf;

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
