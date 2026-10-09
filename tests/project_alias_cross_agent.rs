//! Task 8（C2/R03）+ A05：跨工具项目身份。
//!
//! 分两种必须成立的行为：
//! 1. **同一路径**（两侧都有可靠 cwd）→ 同一项目、一次聚合，两侧 agent 都计入；
//! 2. 仅展示名（basename）相同、身份不同（Claude 只有 slug）→ **不推断合并**，
//!    展示名相同属预期，关联必须显式可解释。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, summary};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-a05-alias-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Claude 会话：`with_cwd = true` 写顶层 cwd（可靠身份）；否则只有目录名（slug）。
fn write_claude(dir: &std::path::Path, with_cwd: bool) -> PathBuf {
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let cwd = if with_cwd {
        r#""cwd":"C:\\work\\alpha","#
    } else {
        ""
    };
    let line = format!(
        r#"{{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c1",{cwd}"message":{{"id":"c-m1","model":"claude-sonnet-4-5","usage":{{"input_tokens":100,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
    );
    std::fs::write(project.join("s.jsonl"), format!("{line}\n")).unwrap();
    dir.join("claude")
}

fn write_codex(dir: &std::path::Path) -> PathBuf {
    let codex = dir.join("codex");
    let day = codex.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join("rollout-a.jsonl"),
        concat!(
            r#"{"timestamp":"2026-08-01T12:00:00.000Z","type":"session_meta","payload":{"id":"x","session_id":"x","cwd":"C:/work/alpha"}}"#, "\n",
            r#"{"timestamp":"2026-08-01T12:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/work/alpha"}}"#, "\n",
            r#"{"timestamp":"2026-08-01T12:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":55}}}}"#, "\n",
        ),
    )
    .unwrap();
    codex
}

fn opts(
    dir: &std::path::Path,
    claude: &std::path::Path,
    codex: &std::path::Path,
) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(claude.to_path_buf()),
        codex_dir: Some(codex.to_path_buf()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

#[test]
fn same_path_merges_agents() {
    let dir = tmp("merge");
    let claude = write_claude(&dir, true);
    let codex = write_codex(&dir);
    let r = summary(&opts(&dir, &claude, &codex)).unwrap();
    // 同一路径 → 一行项目 + 合计行。
    assert_eq!(r.groups.len(), 2, "同一路径必须合并：{:?}", r.groups);
    let g = &r.groups[0];
    assert_eq!(g.key, "C:/work/alpha");
    assert_eq!(g.requests, 2, "两侧请求都计入同一项目");
    let mut agents = g.agents.clone();
    agents.sort();
    assert_eq!(agents, ["claude-code", "codex"]);
    assert_eq!(g.label.as_deref(), Some("alpha"), "展示名 = 路径末段");
}

#[test]
fn same_basename_different_paths_stay_separate() {
    let dir = tmp("split");
    // Claude 侧没有可靠 cwd → 身份是目录名（slug），与 Codex 的完整路径不同。
    let claude = write_claude(&dir, false);
    let codex = write_codex(&dir);
    let r = summary(&opts(&dir, &claude, &codex)).unwrap();
    assert_eq!(
        r.groups.len(),
        3,
        "两个身份 + 合计（同名不自动合并）：{:?}",
        r.groups
    );
    let mut ids: Vec<&str> = r.groups[..2].iter().map(|g| g.key.as_str()).collect();
    ids.sort();
    assert_eq!(ids, ["C:/work/alpha", "alpha"]);
    let labels: Vec<&str> = r.groups[..2]
        .iter()
        .map(|g| g.label.as_deref().unwrap())
        .collect();
    assert!(labels.iter().all(|l| *l == "alpha"), "展示名相同");
    std::fs::remove_dir_all(&dir).ok();
}
