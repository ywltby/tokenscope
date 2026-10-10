//! H02：纳入所有代理用量与请求身份（计划 §2 不变量 1、§4 采集与去重）。
//!
//! 覆盖：子代理用量并入而非排除、同请求跨文件/跨目录去重取终值、混合 cwd
//! 不串项目、Codex 默认根覆盖 `sessions/` + `archived_sessions/`、发现范围
//! 不误纳非 rollout 文件、可证实的跨来源身份（以及**身份不足时不猜测**）。
//!
//! 所有路径都在临时目录，测试注入隔离的缓存与价格索引（计划不变量 10）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::{AgentKind, UsageEvent};
use tokenscope::report::{SummaryOptions, SummaryReport, summary};
use tokenscope::source::Source;
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::codex::CodexSource;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-all-sources-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 合成一条 Claude assistant 行（真实 JSON 序列化，不做字符串拼接）。
fn claude_assistant(
    session: &str,
    id: &str,
    ts: &str,
    input: u64,
    output: u64,
    sidechain: bool,
) -> String {
    let mut v = serde_json::json!({
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
    });
    if sidechain {
        v["isSidechain"] = serde_json::Value::Bool(true);
    }
    v.to_string()
}

fn claude_context(session: &str, ts: &str, cwd: &str) -> String {
    serde_json::json!({
        "type": "user",
        "timestamp": ts,
        "sessionId": session,
        "cwd": cwd,
        "message": {"role": "user"},
    })
    .to_string()
}

fn codex_meta(session: &str, cwd: &str) -> String {
    serde_json::json!({
        "timestamp": "2026-07-17T15:00:00.000Z",
        "type": "session_meta",
        "payload": {"id": session, "session_id": session, "cwd": cwd},
    })
    .to_string()
}

fn codex_context(model: &str, cwd: &str) -> String {
    serde_json::json!({
        "timestamp": "2026-07-17T15:01:00.000Z",
        "type": "turn_context",
        "payload": {"turn_id": "t", "model": model, "cwd": cwd},
    })
    .to_string()
}

fn codex_token_count(ts: &str, input: u64, output: u64, cached: u64, cw: u64) -> String {
    let total = input + output;
    serde_json::json!({
        "timestamp": ts,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cached_input_tokens": cached,
                    "cache_write_input_tokens": cw,
                    "reasoning_output_tokens": 0,
                    "total_tokens": total,
                }
            }
        }
    })
    .to_string()
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

/// 隔离的 report 选项：不触碰真实 `~/.tokenscope`（不变量 10）。
fn opts(dir: &Path, claude: Option<PathBuf>, codex: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Agent,
        claude_dir: claude.clone(),
        codex_dir: codex.clone(),
        claude_enabled: Some(claude.is_some()),
        codex_enabled: Some(codex.is_some()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("no-such-openrouter.json")),
        modelsdev_path: Some(dir.join("no-such-modelsdev.json")),
        ..Default::default()
    }
}

fn report(opts: &SummaryOptions) -> SummaryReport {
    summary(opts).unwrap()
}

#[test]
fn claude_sidechain_usage_is_included_end_to_end() {
    let dir = temp_dir("claude-sidechain");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess.jsonl"),
        &[
            claude_context("s1", "2026-07-17T08:00:00.000Z", "/work/main"),
            claude_assistant("s1", "m1", "2026-07-17T08:00:01.000Z", 10, 5, false),
            claude_assistant("s1", "m2", "2026-07-17T08:00:02.000Z", 100, 50, true),
            claude_assistant("s1", "m3", "2026-07-17T08:00:03.000Z", 1, 1, false),
        ],
    );

    let col = ClaudeSource::new(&root).collect().unwrap();
    assert_eq!(col.events.len(), 3, "子代理用量并入，独立请求相加");
    assert_eq!(col.stats.skipped_sidechain, 0);
    assert_eq!(
        col.events.iter().map(|e| e.total_tokens()).sum::<u64>(),
        167
    );
    // sidechain 的目录上下文不参与归属：三条请求都归主链项目。
    assert!(
        col.events.iter().all(|e| e.project == "/work/main"),
        "混合执行上下文不串项目: {:?}",
        col.events
            .iter()
            .map(|e| e.project.clone())
            .collect::<Vec<_>>()
    );

    let r = report(&opts(&dir, Some(root), None));
    assert_eq!(r.totals.requests, 3, "汇总口径与采集一致");
    assert_eq!(r.totals.tokens.total(), 167);
    assert_eq!(r.totals.tokens.input, 111);
    assert_eq!(r.totals.tokens.output, 56);
}

#[test]
fn same_request_across_project_dirs_is_deduped_to_latest() {
    let dir = temp_dir("claude-cross-file");
    let root = dir.join("projects");
    // 同一 (sessionId, message.id) 在归档副本与主文件中各出现一次：
    // 上游流式终值语义 → 只计一次，保留时间戳最晚的用量。
    write_lines(
        &root.join("proj-a").join("sess.jsonl"),
        &[
            claude_assistant("s1", "m1", "2026-07-17T08:00:01.000Z", 10, 5, false),
            claude_assistant("s1", "m2", "2026-07-17T08:00:02.000Z", 7, 3, false),
        ],
    );
    write_lines(
        &root.join("proj-a-archive").join("sess-copy.jsonl"),
        &[claude_assistant(
            "s1",
            "m1",
            "2026-07-17T08:00:09.000Z",
            40,
            20,
            false,
        )],
    );

    let col = ClaudeSource::new(&root).collect().unwrap();
    assert_eq!(col.events.len(), 3, "source 层原样产出未去重事件");

    let r = report(&opts(&dir, Some(root), None));
    assert_eq!(r.totals.requests, 2, "同一请求跨文件只计一次");
    assert_eq!(
        r.totals.tokens.total(),
        (40 + 20) + (7 + 3),
        "保留时间戳最晚的终值，不把两份相加"
    );
}

#[test]
fn codex_roots_cover_sessions_and_archived_without_escaping() {
    let dir = temp_dir("codex-roots");
    let codex_home = dir.join("codex-home");
    let sessions = codex_home.join("sessions");
    let archived = codex_home.join("archived_sessions");
    write_lines(
        &sessions.join("2026").join("07").join("rollout-a.jsonl"),
        &[
            codex_meta("s-a", "/work/a"),
            codex_context("gpt-5.6-sol", "/work/a"),
            codex_token_count("2026-07-17T15:02:00.000Z", 100, 20, 0, 0),
        ],
    );
    write_lines(
        &archived.join("2026").join("06").join("rollout-b.jsonl"),
        &[
            codex_meta("s-b", "/work/b"),
            codex_context("gpt-5.6-sol", "/work/b"),
            codex_token_count("2026-06-01T15:02:00.000Z", 200, 40, 0, 0),
        ],
    );

    // 默认来源形态：两个根一起发现。
    let multi = CodexSource::with_roots(vec![sessions.clone(), archived.clone()]);
    let files = multi.discover();
    assert_eq!(files.len(), 2, "归档目录同样被纳入：{files:?}");
    let col = multi.collect().unwrap();
    assert_eq!(col.events.len(), 2);
    assert_eq!(
        col.events.iter().map(|e| e.total_tokens()).sum::<u64>(),
        360
    );

    // 显式自定义根：只在选定目录内递归，不扩大到目录之外。
    let single = CodexSource::new(&sessions);
    assert_eq!(single.discover().len(), 1);
    let col = single.collect().unwrap();
    assert_eq!(col.events.len(), 1, "自定义根不越界采集");

    // 缺归档目录时只报确实缺失的根（不把"从未归档"混进发现失败）。
    let missing = CodexSource::with_roots(vec![sessions.clone(), dir.join("no-archive")]);
    assert_eq!(missing.discover().len(), 1);
}

#[test]
fn codex_discovery_ignores_non_rollout_history_file() {
    let dir = temp_dir("codex-history");
    let root = dir.join("sessions");
    write_lines(
        &root.join("history.jsonl"),
        &[r#"{"session_id":"s1","ts":1,"text":"user prompt"}"#.to_string()],
    );
    write_lines(
        &root.join("2026").join("07").join("rollout-a.jsonl"),
        &[
            codex_meta("s-a", "/work/a"),
            codex_context("gpt-5.6-sol", "/work/a"),
            codex_token_count("2026-07-17T15:02:00.000Z", 100, 20, 0, 0),
        ],
    );
    let files = CodexSource::new(&root).discover();
    assert_eq!(files.len(), 1, "非 rollout 文件不参与用量发现：{files:?}");
    assert!(files[0].ends_with("rollout-a.jsonl"));
}

#[test]
fn native_identity_is_provable_only_where_upstream_provides_one() {
    let claude = sample_event(AgentKind::ClaudeCode, "s1", "m1");
    assert_eq!(
        claude.native_identity(),
        Some(("claude-message", "s1|m1".to_string())),
        "Claude 的 (session_id, message.id) 是可证实的稳定请求身份"
    );

    let mut no_id = sample_event(AgentKind::ClaudeCode, "s1", "");
    assert_eq!(no_id.native_identity(), None, "缺 message.id 不猜测身份");
    no_id.record_id = "m1".into();
    no_id.session_id = String::new();
    assert_eq!(no_id.native_identity(), None, "缺 session 不猜测身份");

    let codex = sample_event(AgentKind::Codex, "s9", "");
    assert_eq!(
        codex.native_identity(),
        None,
        "Codex rollout 的 token_count 不带请求标识——身份不足即返回 None"
    );
}

#[test]
fn claude_identity_matches_ccs_compatible_alias_value() {
    // CCS 导入用同一函数按"会话 + 消息"生成身份：两侧规则同源，
    // 因此同一条请求在原生采集与 CCS 导入里得到完全相同的别名值。
    let dir = temp_dir("claude-identity");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess.jsonl"),
        &[claude_assistant(
            "sess-7",
            "msg-9",
            "2026-07-17T08:00:01.000Z",
            3,
            2,
            false,
        )],
    );
    let parsed = ClaudeSource::new(&root).parse_file(&root.join("proj-a").join("sess.jsonl"));
    assert_eq!(parsed.events.len(), 1);
    let (scheme, value) = parsed.events[0].native_identity().unwrap();
    assert_eq!(scheme, "claude-message");
    assert_eq!(value, "sess-7|msg-9");
    assert_eq!(parsed.events[0].line, 0, "来源定位带行序");
}

fn sample_event(agent: AgentKind, session: &str, record: &str) -> UsageEvent {
    UsageEvent {
        ts: "2026-07-17T08:00:00Z".parse().unwrap(),
        agent,
        model: "m".into(),
        session_id: session.into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: record.into(),
        line: 0,
        source_path: String::new(),
        input_tokens: 1,
        output_tokens: 1,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    }
}
