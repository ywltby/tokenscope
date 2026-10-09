//! A03：Claude 项目映射（`~/.claude.json` 的 `projects`）。
//!
//! 覆盖：唯一正向映射把历史 slug 解析回真实路径、冲突 slug 保持独立、
//! 自定义来源不读本机配置、配置缺失/损坏不丢用量，以及**配置变化**同时让
//! 磁盘缓存与采集复用键失效（而已建立的查询会话仍用旧冻结快照）。

use std::path::{Path, PathBuf};

use tokenscope::aggregate::GroupBy;
use tokenscope::query;
use tokenscope::report::{SummaryOptions, summary};
use tokenscope::source::Source;
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::claude_projects::{MappingState, ProjectMapping};

fn fixture(p: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/project-path")
        .join(p)
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-pmap-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 一条典型用量事件（**不带** cwd → 必然走文件身份/映射解析路径）。
fn session_line(id: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"2026-08-02T09:00:00.000Z","sessionId":"s1","message":{{"id":"{id}","model":"claude-sonnet-4-5","usage":{{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    )
}

fn write_session(roots: &Path, slug: &str, id: &str) {
    let dir = roots.join(slug);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("s.jsonl"), format!("{}\n", session_line(id))).unwrap();
}

fn opts_for(dir: &Path, claude_root: &Path, mapping: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(claude_root.to_path_buf()),
        codex_dir: Some(dir.join("no-codex")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        claude_projects_path: mapping,
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

fn keys(report: &tokenscope::report::SummaryReport) -> Vec<String> {
    report.groups.iter().map(|g| g.key.clone()).collect()
}

#[test]
fn unique_forward_mapping_resolves_legacy_slug() {
    let dir = tmp("unique");
    let roots = dir.join("sources");
    write_session(&roots, "C--work-alpha", "m1");
    let src = ClaudeSource::with_mapping(
        &roots,
        ProjectMapping::load_cached(&fixture("claude-projects.json")),
    );
    let col = src.collect().unwrap();
    assert_eq!(col.stats.bad_lines, 0);
    assert_eq!(col.events.len(), 1);
    assert_eq!(col.events[0].project, "C:/work/alpha");
    // 采集层的缓存身份就是该映射修订。
    assert_eq!(src.context_revision(), src.mapping().revision());
}

#[test]
fn colliding_slugs_remain_unmerged() {
    let dir = tmp("collide");
    let roots = dir.join("sources");
    // `C:\work\a.b` 与 `C:\work\a-b` 编码后是同一个 slug（`.` 与 `-` 都编码为
    // `-`）——真实存在的歧义：不合并、也不选第一个。
    write_session(&roots, "C--work-a-b", "m1");
    let src = ClaudeSource::with_mapping(
        &roots,
        ProjectMapping::load_cached(&fixture("claude-projects.json")),
    );
    let col = src.collect().unwrap();
    assert_eq!(col.events.len(), 1);
    assert_eq!(
        col.events[0].project, "C--work-a-b",
        "同一 slug 的两个候选不合并，也不选第一个"
    );
}

#[test]
fn custom_root_does_not_use_home_mapping() {
    let dir = tmp("custom");
    let roots = dir.join("sources");
    write_session(&roots, "C--work-alpha", "m1");
    // 适配器默认构造器 = 禁用（不读本机 ~/.claude.json）。
    let src = ClaudeSource::new(&roots);
    assert_eq!(src.mapping().state(), MappingState::Disabled);
    let col = src.collect().unwrap();
    assert_eq!(col.events[0].project, "C--work-alpha");
    // 采集层策略同源：自定义来源且未注入映射路径 → 同样禁用（保持 slug 身份）。
    let report = summary(&opts_for(&dir, &roots, None)).unwrap();
    assert!(keys(&report).contains(&"C--work-alpha".to_string()));
}

#[test]
fn missing_or_bad_config_keeps_usage() {
    let dir = tmp("bad");
    let roots = dir.join("sources");
    write_session(&roots, "C--work-alpha", "m1");
    // 配置缺失。
    let missing = ProjectMapping::load_cached(&dir.join("nope.json"));
    assert_eq!(missing.state(), MappingState::Missing);
    let src = ClaudeSource::with_mapping(&roots, missing);
    let col = src.collect().unwrap();
    assert_eq!(col.events.len(), 1);
    assert_eq!(col.events[0].project, "C--work-alpha");
    // 配置损坏。
    let bad_path = dir.join("bad.json");
    std::fs::write(&bad_path, "{ not json").unwrap();
    let broken = ProjectMapping::load_cached(&bad_path);
    assert_eq!(broken.state(), MappingState::Unusable);
    let src = ClaudeSource::with_mapping(&roots, broken);
    let col = src.collect().unwrap();
    assert_eq!(col.stats.bad_lines, 0);
    assert_eq!(col.events.len(), 1);
    assert_eq!(col.events[0].project, "C--work-alpha");
}

#[test]
fn mapping_change_invalidates_disk_and_memory_results() {
    let dir = tmp("invalidate");
    let roots = dir.join("sources");
    write_session(&roots, "C--work-alpha", "m1");
    let mapping_path = dir.join("claude.json");
    std::fs::write(&mapping_path, r#"{"projects":{"C:\\work\\alpha":{}}}"#).unwrap();
    let opts = opts_for(&dir, &roots, Some(mapping_path.clone()));

    let before = summary(&opts).unwrap();
    assert!(keys(&before).contains(&"C:/work/alpha".to_string()));
    let frozen_id = before.query_id.clone();

    // 日志指纹完全不变，只改配置：内容与尺寸都变（缓存键必须跟着变）。
    std::fs::write(
        &mapping_path,
        r#"{"projects":{"D:\\elsewhere\\x":{},"E:\\another\\y":{}}}"#,
    )
    .unwrap();

    // 已建立的查询会话仍使用旧冻结快照。
    let frozen = query::query_summary(&frozen_id).unwrap();
    assert!(
        keys(&frozen).contains(&"C:/work/alpha".to_string()),
        "旧查询不得被配置变化改写：{:?}",
        keys(&frozen)
    );

    // 下一次查询：磁盘缓存与采集复用键都随映射修订失效 → 归属更新。
    let after = summary(&opts).unwrap();
    let after_keys = keys(&after);
    assert!(
        after_keys.contains(&"C--work-alpha".to_string()),
        "配置变化后必须重新解析：{after_keys:?}"
    );
    assert!(!after_keys.contains(&"C:/work/alpha".to_string()));
}
