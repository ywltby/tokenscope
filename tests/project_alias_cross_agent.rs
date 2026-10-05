//! Task 8（C2/R03）：test_project_alias_cross_agent——同一展示名跨工具
//! 是两个独立项目身份（Claude slug 与 Codex cwd 各自为政），不自动合并；
//! 展示名（label）相同属预期，关联必须显式可解释而非隐式拼接。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, summary};

#[test]
fn test_project_alias_cross_agent() {
    let dir = std::env::temp_dir().join(format!("tokenscope-t8-alias-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Claude：目录名 alpha（slug 身份）；Codex：cwd C:/work/alpha（完整路径身份）。
    let claude_dir = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&claude_dir).unwrap();
    std::fs::write(
        claude_dir.join("s.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c1","message":{"id":"m","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}}"#,
    )
    .unwrap();
    let codex_dir = dir.join("codex");
    let day = codex_dir.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join("rollout-a.jsonl"),
        concat!(
            r#"{"timestamp":"2026-08-01T12:00:00.000Z","type":"session_meta","payload":{"id":"x","session_id":"x","cwd":"C:/work/alpha"}}"#, "
",
            r#"{"timestamp":"2026-08-01T12:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/work/alpha"}}"#, "
",
            r#"{"timestamp":"2026-08-01T12:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":55}}}}"#, "
",
        ),
    )
    .unwrap();
    let opts = SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(dir.join("claude")),
        codex_dir: Some(codex_dir),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let r = summary(&opts).unwrap();
    assert_eq!(r.groups.len(), 3, "两个身份 + 合计行（同名不自动合并）");
    // 两个身份、同一展示名 alpha。
    let ids: Vec<&str> = r.groups[..2].iter().map(|g| g.key.as_str()).collect();
    assert!(ids.contains(&"alpha"), "claude slug 身份");
    assert!(ids.contains(&"C:/work/alpha"), "codex 完整 cwd 身份");
    let labels: Vec<&str> = r.groups[..2]
        .iter()
        .map(|g| g.label.as_deref().unwrap())
        .collect();
    assert!(
        labels.iter().all(|l| *l == "alpha"),
        "展示名相同: {labels:?}"
    );
    let _ = AgentKind::ClaudeCode;
    std::fs::remove_dir_all(&dir).ok();
}
