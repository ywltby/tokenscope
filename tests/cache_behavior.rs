//! 计划 B4/B5 缓存行为回归：
//! - test_agent_filter_preserves_other_cache（F06）：查看单 agent 不得清空
//!   其他来源的缓存（修复前 keep_paths 只含选中来源，全局 purge 清掉其余）；
//! - test_read_failure_not_cached（B4）：读取失败的文件不写成功缓存，
//!   且有可见警告；恢复可读后正常入库。

use std::path::PathBuf;

use tokenscope::report::{SummaryOptions, cache_stats, summary};

fn fixture(agent: &str, p: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(agent)
        .join(p)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-cache-behavior-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn opts(dir: &std::path::Path, agent: Option<&str>) -> SummaryOptions {
    SummaryOptions {
        by: tokenscope::aggregate::GroupBy::Day,
        agent: agent.map(|a| match a {
            "claude" => tokenscope::model::AgentKind::ClaudeCode,
            _ => tokenscope::model::AgentKind::Codex,
        }),
        claude_dir: Some(fixture("claude", "basic")),
        codex_dir: Some(fixture("codex", "basic")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

#[test]
fn test_agent_filter_preserves_other_cache() {
    let dir = tmp_dir("scope");
    // 预热全量：4 个 fixture 文件（claude 2 + codex 2）。
    let all = opts(&dir, None);
    let r0 = summary(&all).unwrap();
    assert_eq!(cache_stats(all.cache_dir.clone()).unwrap().files, 4);
    // 只看 Claude：F06 修复前会全局 purge 掉 codex 两行。
    let claude = opts(&dir, Some("claude"));
    let r1 = summary(&claude).unwrap();
    assert_eq!(
        cache_stats(claude.cache_dir.clone()).unwrap().files,
        4,
        "查看单 agent 不得清空其他来源的缓存"
    );
    assert_eq!(r1.totals.requests, 3);
    // 切回全部：codex 直接命中缓存（数字不变；此处以结果一致性断言）。
    let r2 = summary(&all).unwrap();
    assert_eq!(r2.totals.requests, r0.totals.requests);
    assert_eq!(cache_stats(all.cache_dir.clone()).unwrap().files, 4);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[cfg(windows)]
fn test_read_failure_not_cached() {
    use std::os::windows::fs::OpenOptionsExt as _;
    let dir = tmp_dir("readfail");
    // codex 目录放一个内容合法的 rollout 文件。
    let codex_dir = dir.join("sessions");
    let day_dir = codex_dir.join("2026").join("07").join("17");
    std::fs::create_dir_all(&day_dir).unwrap();
    let file = day_dir.join("rollout-x.jsonl");
    std::fs::write(
        &file,
        concat!(
            r#"{"timestamp":"2026-07-17T15:00:00.000Z","type":"session_meta","payload":{"id":"s1","session_id":"s1","cwd":"C:/w/alpha"}}"#, "\n",
            r#"{"timestamp":"2026-07-17T15:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/w/alpha"}}"#, "\n",
            r#"{"timestamp":"2026-07-17T15:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":110}}}}"#, "\n",
        ),
    )
    .unwrap();
    let o = SummaryOptions {
        by: tokenscope::aggregate::GroupBy::Day,
        agent: Some(tokenscope::model::AgentKind::Codex),
        claude_dir: Some(dir.join("no-claude")),
        codex_dir: Some(codex_dir.clone()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    // 独占句柄（共享模式 0）阻止任何其他打开：模拟读取失败。
    let _guard = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let r = summary(&o).unwrap();
    assert_eq!(r.totals.requests, 0, "读取失败事件不入账");
    assert!(
        r.sources[0].stats.io_errors > 0,
        "读取失败必须计入 io_errors: {:?}",
        r.sources[0].stats
    );
    assert!(
        r.warnings.iter().any(|w| w.contains("读取失败")),
        "读取失败必须有可见警告: {:?}",
        r.warnings
    );
    assert_eq!(
        cache_stats(o.cache_dir.clone()).unwrap().files,
        0,
        "读取失败的文件不得写成成功缓存"
    );
    drop(_guard);
    // 恢复可读：正常解析入库。
    let r2 = summary(&o).unwrap();
    assert_eq!(r2.totals.requests, 1);
    assert_eq!(cache_stats(o.cache_dir.clone()).unwrap().files, 1);
    std::fs::remove_dir_all(&dir).ok();
}
