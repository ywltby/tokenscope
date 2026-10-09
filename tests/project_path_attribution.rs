//! A02（阶段 A：启动路径统一）：Claude 会话初始 cwd 的项目归属。
//!
//! 每条用例都通过 `ClaudeSource::parse_file` 走真实管线（fixture 目录即
//! 采集根），验证的不是内部中间值，而是最终 `UsageEvent.project`。

use std::path::{Path, PathBuf};

use tokenscope::cache::{Cache, FileKey};
use tokenscope::model::AgentKind;
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

// ---- 阶段 B（B02/B03）：项目根归并 ----

#[test]
fn claude_structured_cwd_switch() {
    let p = claude_parse("session-switch/claude-switch.jsonl");
    assert_eq!(p.stats.bad_lines, 0);
    let seq: Vec<&str> = p.events.iter().map(|e| e.project.as_str()).collect();
    assert_eq!(
        seq,
        ["C:/test", "C:/test", "C:/test", "C:/bee", "C:/bee"],
        "进入子目录仍归当前根；越界即新项目，其子目录同属新项目"
    );
    let cwds: Vec<Option<&str>> = p.events.iter().map(|e| e.event_cwd.as_deref()).collect();
    assert_eq!(
        cwds,
        [
            Some("C:/test"),
            Some("C:/test/123/456"),
            Some("C:/test"),
            Some("C:/bee"),
            Some("C:/bee/123"),
        ],
        "event_cwd 保留子目录细节"
    );
}

#[test]
fn codex_structured_cwd_switch() {
    let p = codex_parse("session-switch/codex-switch.jsonl");
    assert_eq!(p.stats.bad_lines, 0);
    let seq: Vec<&str> = p.events.iter().map(|e| e.project.as_str()).collect();
    assert_eq!(
        seq,
        ["C:/test", "C:/test", "C:/bee", "C:/bee", "C:/test"],
        "子目录归并 + 越界切换 + 回到旧目录按新根处理"
    );
}

#[test]
fn switch_does_not_reassign_previous_events() {
    // 归属在事件产生时定稿：后来的切换不得回溯改写更早的请求（两侧同理）。
    let c = claude_parse("session-switch/claude-switch.jsonl");
    assert_eq!(c.events[0].project, "C:/test");
    assert_eq!(c.events[2].project, "C:/test");
    assert_eq!(c.events[3].project, "C:/bee");
    let x = codex_parse("session-switch/codex-switch.jsonl");
    assert_eq!(x.events[0].project, "C:/test");
    assert_eq!(x.events[2].project, "C:/bee");
    assert_eq!(x.events[4].project, "C:/test", "回到旧目录不改写历史");
}

#[test]
fn shell_cd_text_does_not_change_project() {
    // 工具文本里的 `cd` 不改变结构化 cwd（B01 结论 3）：归属保持会话当前根。
    let p = claude_parse("session-switch/claude-shell-cd.jsonl");
    assert_eq!(p.stats.bad_lines, 0);
    assert_eq!(p.events.len(), 2);
    assert!(p.events.iter().all(|e| e.project == "C:/test"));
    assert!(
        p.events
            .iter()
            .all(|e| e.event_cwd.as_deref() == Some("C:/test"))
    );
}

#[test]
fn both_agents_follow_decided_subdirectory_policy() {
    // 子目录政策两侧一致：更深的工作目录仍归当前项目根。
    let c = claude_parse("session-switch/claude-switch.jsonl");
    let x = codex_parse("session-switch/codex-switch.jsonl");
    assert_eq!(
        c.events[1].project, "C:/test",
        "Claude：/test/123/456 → /test"
    );
    assert_eq!(x.events[1].project, "C:/test", "Codex：/test/123 → /test");
}

#[test]
fn initial_cwd_is_stable_while_event_cwd_changes() {
    // B02：会话初始目录稳定，event_cwd 随上下文变化；归属按项目根归并。
    let p = claude_parse("session-switch/claude-switch.jsonl");
    assert!(
        p.events
            .iter()
            .all(|e| e.session_initial_cwd.as_deref() == Some("C:/test")),
        "会话初始目录必须稳定"
    );
    let distinct: std::collections::BTreeSet<&str> = p
        .events
        .iter()
        .filter_map(|e| e.event_cwd.as_deref())
        .collect();
    assert!(distinct.len() >= 3, "event_cwd 应当变化: {distinct:?}");
    assert!(
        p.events
            .iter()
            .all(|e| e.project == "C:/test" || e.project == "C:/bee")
    );
}

#[test]
fn cwd_context_survives_cache_roundtrip() {
    // B02：两个目录上下文字段必须贯通缓存读写（序列化与恢复一致）。
    let dir = std::env::temp_dir().join(format!("tokenscope-b02-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cache = Cache::open(&dir.join("cache.db")).unwrap();
    let parse = claude_parse("session-switch/claude-switch.jsonl");
    let key = FileKey {
        path: "p.jsonl",
        agent: AgentKind::ClaudeCode,
        root: "root",
        context_rev: "rev",
    };
    cache.store_file(&key, 10, 20, &parse).unwrap();
    let hit = cache
        .lookup_file(&key, 10, 20)
        .unwrap()
        .expect("指纹一致必须命中");
    let take =
        |p: &tokenscope::source::FileParse| -> Vec<(Option<String>, Option<String>, String)> {
            p.events
                .iter()
                .map(|e| {
                    (
                        e.session_initial_cwd.clone(),
                        e.event_cwd.clone(),
                        e.project.clone(),
                    )
                })
                .collect()
        };
    assert_eq!(take(&hit.parse), take(&parse));
    std::fs::remove_dir_all(&dir).ok();
}
