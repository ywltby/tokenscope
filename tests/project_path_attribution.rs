//! A02（阶段 A：启动路径统一）：Claude 会话初始 cwd 的项目归属。
//!
//! 每条用例都通过 `ClaudeSource::parse_file` 走真实管线（fixture 目录即
//! 采集根），验证的不是内部中间值，而是最终 `UsageEvent.project`。

use std::path::{Path, PathBuf};

use tokenscope::source::Source;
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::codex::CodexSource;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-path")
}

fn claude_parse(name: &str) -> tokenscope::source::FileParse {
    let r = root();
    ClaudeSource::new(&r).parse_file(&r.join(name))
}

fn codex_parse(name: &str) -> tokenscope::source::FileParse {
    let r = root();
    CodexSource::new(&r).parse_file(&r.join(name))
}

fn projects(p: &tokenscope::source::FileParse) -> Vec<String> {
    p.events.iter().map(|e| e.project.clone()).collect()
}

#[test]
fn claude_initial_cwd_is_shared_by_session_events() {
    let p = claude_parse("claude-initial-cwd.jsonl");
    assert_eq!(p.stats.bad_lines, 0);
    assert_eq!(p.events.len(), 2);
    // `C:\work\alpha` 与 `C:/work/alpha/` 归一为同一身份。
    assert_eq!(projects(&p), ["C:/work/alpha", "C:/work/alpha"]);
}

#[test]
fn claude_subdirectory_drift_does_not_split_phase_a() {
    let p = claude_parse("claude-cwd-drift.jsonl");
    assert_eq!(p.stats.bad_lines, 0);
    assert_eq!(p.events.len(), 2);
    // 阶段 A：会话按**首个**有效 cwd 归属，后续行漂移到子目录不重写历史。
    assert_eq!(projects(&p), ["C:/work/alpha", "C:/work/alpha"]);
}

#[test]
fn claude_sessions_do_not_share_initial_cwd() {
    let p = claude_parse("claude-multi-session.jsonl");
    assert_eq!(p.stats.bad_lines, 0);
    assert_eq!(p.events.len(), 3);
    let by_id: Vec<(String, String)> = p
        .events
        .iter()
        .map(|e| (e.record_id.clone(), e.project.clone()))
        .collect();
    assert_eq!(
        by_id,
        [
            ("x1".to_string(), "C:/work/alpha".to_string()),
            ("x2".to_string(), "D:/other/beta".to_string()),
            ("x3".to_string(), "C:/work/alpha".to_string()),
        ]
    );
}

#[test]
fn invalid_cwd_does_not_drop_usage() {
    let p = claude_parse("claude-invalid-cwd.jsonl");
    // 缺失、空值、类型异常、不可解释的路径都**不**影响 usage 合法性。
    assert_eq!(p.stats.bad_lines, 0);
    assert_eq!(p.events.len(), 4);
    // 无可信身份时回落到文件身份（fixture 文件直接在采集根下）。
    assert_eq!(
        projects(&p),
        ["(根目录)", "(根目录)", "(根目录)", "(根目录)"]
    );
}

#[test]
fn codex_initial_path_matches_claude() {
    let c = claude_parse("claude-initial-cwd.jsonl");
    let x = codex_parse("codex-cwd-switch.jsonl");
    assert_eq!(x.stats.bad_lines, 0);
    assert_eq!(c.events[0].project, "C:/work/alpha");
    assert_eq!(
        x.events[0].project, "C:/work/alpha",
        "同一路径的跨工具会话必须是同一身份 key"
    );
}

#[test]
fn codex_turn_context_changes_only_later_events() {
    let p = codex_parse("codex-cwd-switch.jsonl");
    assert_eq!(p.events.len(), 6);
    assert_eq!(p.events[0].project, "C:/work/alpha");
    assert_eq!(
        p.events[1].project, "D:/other/beta",
        "切换只影响其后事件，不回溯改写已发生请求"
    );
}

#[test]
fn codex_a_b_a_switch_is_preserved() {
    let p = codex_parse("codex-cwd-switch.jsonl");
    let seq: Vec<&str> = p.events.iter().map(|e| e.project.as_str()).collect();
    assert_eq!(
        &seq[..3],
        ["C:/work/alpha", "D:/other/beta", "C:/work/alpha"],
        "A → B → A 切换不得被冻结成文件首值"
    );
    assert_eq!(seq[3], "C:/work/alpha", "空 cwd 不清掉同会话已知目录");
}

#[test]
fn codex_new_session_does_not_inherit_cwd() {
    let p = codex_parse("codex-cwd-switch.jsonl");
    assert_eq!(
        p.events[4].project, "(未知)",
        "新 session 无 cwd 时不得继承上一个 session 的目录"
    );
    assert_eq!(
        p.events[5].project, "E:/third/gamma",
        "该 session 首个有效 turn_context 生效，且不回溯"
    );
}
