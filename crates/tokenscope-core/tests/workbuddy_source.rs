//! WB03：WorkBuddy 共享解析器（两应用同一实现，AgentKind 区分实例）。
//!
//! fixture 全部手工合成（`tests/fixtures/workbuddy/`），不含真实转写的
//! 任何内容；动态用例使用临时目录。密闭性：不构造 SummaryOptions 的
//! 用例不涉及缓存/价格路径；含聚合的用例注入临时 cache_dir 与 pricing_index。

use std::path::PathBuf;

use tokenscope::model::AgentKind;
use tokenscope::source::{CollectStats, Source, workbuddy::WorkBuddySource};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("workbuddy")
        .join("projects")
}

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-wb03-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn collect(
    source: &WorkBuddySource,
) -> (
    Vec<tokenscope::model::UsageEvent>,
    CollectStats,
    Vec<String>,
) {
    let mut stats = CollectStats::default();
    let mut events = Vec::new();
    let mut warnings = Vec::new();
    let (files, errors) = source.discover_with_errors();
    warnings.extend(errors);
    for f in files {
        stats.files_scanned += 1;
        let parsed = source.parse_file(&f);
        stats.add_file(&parsed);
        events.extend(parsed.events);
    }
    stats.events = events.len() as u64;
    (events, stats, warnings)
}

fn buckets(e: &tokenscope::model::UsageEvent) -> (u64, u64, u64, u64) {
    (
        e.input_tokens,
        e.output_tokens,
        e.cache_write_tokens,
        e.cache_read_tokens,
    )
}

// ── A：路径发现 ──────────────────────────────────────────────

#[test]
fn test_discover_main_and_subagent_files_ignore_attachments() {
    let source = WorkBuddySource::new(fixture_root(), AgentKind::WorkBuddy);
    let (files, errors) = source.discover_with_errors();
    assert!(errors.is_empty(), "{errors:?}");
    let names: Vec<String> = files
        .iter()
        .map(|p| {
            p.strip_prefix(fixture_root())
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        names.contains(&"c-users-u-desktop-demo/sess-main.jsonl".to_string()),
        "主文件必须在场: {names:?}"
    );
    assert!(
        names.contains(
            &"c-users-u-desktop-demo/sess-main/subagents/agent-abc12345.jsonl".to_string()
        ),
        "子 agent 文件（8 hex）必须在场: {names:?}"
    );
    // 附属物不进转写清单
    assert!(!names.iter().any(|n| n.ends_with(".ndjson")), "{names:?}");
    assert!(
        !names.iter().any(|n| n.ends_with(".meta.json")),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains("tool-results")),
        "{names:?}"
    );
    // 空文件也在清单里（解析为零事件）
    assert!(
        names.contains(&"empty-project/sess-empty.jsonl".to_string()),
        "{names:?}"
    );
}

#[test]
fn test_discover_missing_root_returns_empty() {
    let dir = tmp_dir("missing-root");
    let source = WorkBuddySource::new(dir.join("no-such-projects"), AgentKind::WorkBuddy);
    let (files, errors) = source.discover_with_errors();
    assert!(files.is_empty());
    assert!(
        errors.is_empty(),
        "根缺失由调用方统一告警，不在发现层记异常"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── B：典型解析 ──────────────────────────────────────────────

/// 手算基线（fixture `sess-main.jsonl` 10 行 + 子 agent 1 行 = 5 事件）：
/// - wb-msg-1（旧形态）：prompt 1000 = hit 600 + miss 400；cached 600
///   → (400, 200, 0, 600)；credit=3 是死字段，不影响四桶
/// - wb-msg-2（新形态）：prompt 2000，cached 1500 → (500, 300, 0, 1500)
/// - wb-msg-3（三容器）：prompt 100，cached 40 → (60, 10, 0, 40)，只计一次
/// - wb-msg-4（渠道前缀）：模型取 model=hy4-preview → (500, 50, 0, 0)
/// - wb-sub-msg-1（子 agent）：prompt 80，cached 30 → (50, 20, 0, 30)
#[test]
fn test_parse_typical_lines_old_and_new_shapes() {
    let source = WorkBuddySource::new(fixture_root(), AgentKind::WorkBuddy);
    let (events, stats, warnings) = collect(&source);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(events.len(), 5, "5 个带 rawUsage+messageId 的行 → 5 事件");
    assert_eq!(stats.bad_lines, 0, "全部行合法: {stats:?}");
    assert_eq!(stats.skipped_no_model, 0);

    let by_record = |id: &str| {
        events
            .iter()
            .find(|e| e.record_id == id)
            .unwrap_or_else(|| panic!("缺事件 {id}: {events:?}"))
    };
    // 旧形态
    let e1 = by_record("wb-msg-1");
    assert_eq!(buckets(e1), (400, 200, 0, 600));
    assert_eq!(e1.model, "deepseek-v4.1-flash");
    assert_eq!(e1.session_id, "wb-sess-main");
    assert_eq!(e1.ts.to_string(), "2026-08-01T08:00:00Z");
    // 新形态
    let e2 = by_record("wb-msg-2");
    assert_eq!(buckets(e2), (500, 300, 0, 1500));
    assert_eq!(e2.model, "glm-5.2");
    // 三容器只计一次（三容器并存的行只产生一个事件）
    let e3 = by_record("wb-msg-3");
    assert_eq!(buckets(e3), (60, 10, 0, 40));
    assert_eq!(e3.model, "kimi-k2.6");
    // 渠道前缀：模型取 model
    let e4 = by_record("wb-msg-4");
    assert_eq!(buckets(e4), (500, 50, 0, 0));
    assert_eq!(
        e4.model, "hy4-preview",
        "requestModelId 的 custom-local: 前缀不得进入模型"
    );
    // 子 agent：独立 sessionId
    let e5 = by_record("wb-sub-msg-1");
    assert_eq!(buckets(e5), (50, 20, 0, 30));
    assert_eq!(e5.session_id, "wb-sess-subagent-1");
    assert_eq!(e5.agent, AgentKind::WorkBuddy);
}

#[test]
fn test_no_model_falls_back_to_request_model_id_and_counts() {
    let dir = tmp_dir("no-model");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    // model 缺失 → 回退 requestModelId；两者都缺 → skipped_no_model
    std::fs::write(
        proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m1","requestModelId":"fallback-model","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
            r#"{"type":"function_call","id":"r2","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571300000,"providerData":{"messageId":"m2","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (events, stats, _) = collect(&source);
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].model, "fallback-model",
        "model 缺失回退 requestModelId"
    );
    assert_eq!(
        stats.skipped_no_model, 1,
        "两字段都缺失计入 skipped_no_model"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── C：口径定向 ──────────────────────────────────────────────

#[test]
fn test_cache_truth_overrides_dead_fields() {
    let dir = tmp_dir("cache-truth");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    // rawUsage.cache_read_input_tokens = 0（死字段）、message.usage.cache_read_input_tokens = 77（不可靠），
    // 真值 prompt_tokens_details.cached_tokens = 55。
    std::fs::write(
        proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"message","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"role":"assistant","status":"completed","providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"prompt_tokens_details":{"cached_tokens":55},"cache_read_input_tokens":0},"usage":{"inputTokens":100,"outputTokens":10,"totalTokens":110}},"message":{"usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110,"cache_read_input_tokens":77}}}"#,
            "\n",
            // message.usage 缺 cache_read_input_tokens：结果不变
            r#"{"type":"message","id":"r2","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571300000,"role":"assistant","status":"completed","providerData":{"messageId":"m2","model":"m-a","rawUsage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"prompt_tokens_details":{"cached_tokens":55}},"usage":{"inputTokens":100,"outputTokens":10,"totalTokens":110}},"message":{"usage":{"input_tokens":100,"output_tokens":10,"total_tokens":110}}}"#,
            "\n",
            // 缺 prompt_tokens_details → 回退 prompt_cache_hit_tokens
            r#"{"type":"message","id":"r3","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571400000,"role":"assistant","status":"completed","providerData":{"messageId":"m3","model":"m-a","rawUsage":{"prompt_tokens":100,"completion_tokens":10,"total_tokens":110,"prompt_cache_hit_tokens":44,"prompt_cache_miss_tokens":56}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (events, _, _) = collect(&source);
    assert_eq!(events.len(), 3);
    assert_eq!(buckets(&events[0]), (45, 10, 0, 55), "死字段 77/0 不得采信");
    assert_eq!(
        buckets(&events[1]),
        (45, 10, 0, 55),
        "message.usage 缺键不得影响结果"
    );
    assert_eq!(buckets(&events[2]), (56, 10, 0, 44), "缺 details 回退 hit");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_completion_already_includes_reasoning() {
    let dir = tmp_dir("reasoning");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    // completion_tokens 300 已含 reasoning_tokens 100：输出桶必须是 300 不是 400。
    std::fs::write(
        proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":100,"completion_tokens":300,"total_tokens":400,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":100},"completion_thinking_tokens":100}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (events, _, _) = collect(&source);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].output_tokens, 300, "推理 token 不得叠加");
    std::fs::remove_dir_all(&dir).ok();
}

// ── D：不补造 ────────────────────────────────────────────────

#[test]
fn test_no_events_for_failed_or_unidentified_lines() {
    let source = WorkBuddySource::new(fixture_root(), AgentKind::WorkBuddy);
    let (events, stats, _) = collect(&source);
    // fixture 里的失败调用、压缩摘要、file-history-snapshot、无 messageId 用量行
    assert!(
        !events.iter().any(|e| e.record_id.is_empty()),
        "无 messageId 不产生事件"
    );
    assert_eq!(events.len(), 5);
    // 4 行被跳过（失败调用 1 + 摘要 1 + snapshot 1 + 无 messageId 用量 1）
    // ——它们不计 events 也不计 bad_lines（合法的"无用量"或"不补造"行）。
    assert_eq!(stats.bad_lines, 0, "{stats:?}");
}

// ── E：畸形容错 ──────────────────────────────────────────────

#[test]
fn test_malformed_lines_counted_as_bad_lines() {
    let dir = tmp_dir("malformed");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    let good = r#"{"type":"function_call","id":"rg","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m-good","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#;
    let lines = [
        // 非 JSON
        "not-json-at-all",
        // cached > prompt
        r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":50}}}}"#,
        // 负数
        r#"{"type":"function_call","id":"r2","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m2","model":"m-a","rawUsage":{"prompt_tokens":-10,"completion_tokens":2,"total_tokens":-8,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
        // 非整数
        r#"{"type":"function_call","id":"r3","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m3","model":"m-a","rawUsage":{"prompt_tokens":10.5,"completion_tokens":2,"total_tokens":12.5,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
        // u64 溢出
        r#"{"type":"function_call","id":"r4","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m4","model":"m-a","rawUsage":{"prompt_tokens":18446744073709551615,"completion_tokens":2,"total_tokens":18446744073709551617,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
        good,
    ];
    std::fs::write(proj.join("s1.jsonl"), lines.join("\n") + "\n").unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (events, stats, _) = collect(&source);
    assert_eq!(events.len(), 1, "只有合法行产生事件");
    assert_eq!(events[0].record_id, "m-good");
    assert_eq!(stats.bad_lines, 5, "五类畸形行全部计坏行: {stats:?}");
    assert_eq!(stats.lines_seen, 6);
    std::fs::remove_dir_all(&dir).ok();
}

// ── F：项目身份 ──────────────────────────────────────────────

#[test]
fn test_project_identity_matches_claude_across_case_and_separator() {
    let dir = tmp_dir("project");
    let wb_root = dir.join("wb-projects");
    let claude_root = dir.join("claude-projects");
    let (wb_proj, cl_proj) = (wb_root.join("slug-wb"), claude_root.join("proj-x"));
    std::fs::create_dir_all(&wb_proj).unwrap();
    std::fs::create_dir_all(&cl_proj).unwrap();
    // WB：小写盘符 + 反斜杠；Claude：大写盘符 + 正斜杠——同一路径。
    std::fs::write(
        wb_proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\wbdemo","timestamp":1785571200000,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
        ),
    )
    .unwrap();
    std::fs::write(
        cl_proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"assistant","timestamp":"2026-08-01T08:00:00.000Z","sessionId":"s1","cwd":"C:/work/wbdemo","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let wb = WorkBuddySource::new(&wb_root, AgentKind::WorkBuddy);
    let cl = tokenscope::source::claude::ClaudeSource::new(&claude_root);
    let (wb_events, _, _) = collect(&wb);
    let cl_events = cl.collect().unwrap().events;
    assert_eq!(wb_events.len(), 1);
    assert_eq!(cl_events.len(), 1);
    assert_eq!(
        wb_events[0].project, cl_events[0].project,
        "大小写与分隔符差异必须归一到同一项目 key"
    );
    assert!(!wb_events[0].project.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_project_root_tracks_subdirs_and_switches_on_escape() {
    let dir = tmp_dir("root-track");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    // 子目录 → 归当前根；越出 → 切换新根；session_initial_cwd 取首条 cwd。
    std::fs::write(
        proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"message","id":"r0","sessionId":"s1","cwd":"c:\\work\\alpha","timestamp":1785570000000,"role":"user"}"#,
            "\n",
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\alpha\\sub\\deeper","timestamp":1785571200000,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
            r#"{"type":"function_call","id":"r2","sessionId":"s1","cwd":"c:\\other\\beta","timestamp":1785571300000,"providerData":{"messageId":"m2","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (events, _, _) = collect(&source);
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].event_cwd.as_deref(),
        Some("C:/work/alpha/sub/deeper"),
        "event_cwd 保留子目录细节"
    );
    assert_eq!(
        events[0].project,
        tokenscope::source::project_path::normalize_project_path("c:\\work\\alpha").unwrap(),
        "子目录归根（c:\\work\\alpha\\sub\\deeper 归 c:\\work\\alpha）"
    );
    assert_eq!(
        events[0].session_initial_cwd.as_deref(),
        Some(events[0].project.as_str())
    );
    assert_eq!(
        events[1].project,
        tokenscope::source::project_path::normalize_project_path("c:\\other\\beta").unwrap(),
        "越出当前根切换项目"
    );
    assert_eq!(
        events[1].session_initial_cwd.as_deref(),
        tokenscope::source::project_path::normalize_project_path("c:\\work\\alpha").as_deref(),
        "会话初始 cwd 保持首条"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── G：时间口径 ──────────────────────────────────────────────

#[test]
fn test_millisecond_timestamp_crosses_day_in_shanghai() {
    let dir = tmp_dir("cross-day");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    // 2026-08-01T16:30:00Z = 上海 2026-08-02 00:30（跨日）；毫秒精度保留。
    std::fs::write(
        proj.join("s1.jsonl"),
        concat!(
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785601800123,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (events, _, _) = collect(&source);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].ts.to_string(), "2026-08-01T16:30:00.123Z");
    let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
    let sh = events[0].ts.to_zoned(tz);
    assert_eq!(
        sh.strftime("%Y-%m-%d").to_string(),
        "2026-08-02",
        "上海时区跨日"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── H：幂等与追加 ────────────────────────────────────────────

#[test]
fn test_rescan_and_append_are_idempotent_or_incremental() {
    let dir = tmp_dir("idempotent");
    let root = dir.join("projects");
    let proj = root.join("slug-a");
    std::fs::create_dir_all(&proj).unwrap();
    let path = proj.join("s1.jsonl");
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let source = WorkBuddySource::new(&root, AgentKind::WorkBuddy);
    let (first, _, _) = collect(&source);
    assert_eq!(first.len(), 1);
    // 重扫同一文件：事件内容一致（全局幂等性由历史库来源键保证，此处验证解析纯度）
    let (second, _, _) = collect(&source);
    assert_eq!(first.len(), second.len());
    assert_eq!(format!("{:?}", first[0]), format!("{:?}", second[0]));
    // 追加新行：只多出新行的事件
    std::fs::write(
        &path,
        concat!(
            r#"{"type":"function_call","id":"r1","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571200000,"providerData":{"messageId":"m1","model":"m-a","rawUsage":{"prompt_tokens":10,"completion_tokens":2,"total_tokens":12,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
            r#"{"type":"function_call","id":"r2","sessionId":"s1","cwd":"c:\\work\\a","timestamp":1785571300000,"providerData":{"messageId":"m2","model":"m-a","rawUsage":{"prompt_tokens":20,"completion_tokens":4,"total_tokens":24,"prompt_tokens_details":{"cached_tokens":0}}}}"#,
            "\n",
        ),
    )
    .unwrap();
    let (third, _, _) = collect(&source);
    assert_eq!(third.len(), 2, "追加后事件数 +1");
    assert_eq!(third[0].record_id, "m1");
    assert_eq!(third[1].record_id, "m2");
    std::fs::remove_dir_all(&dir).ok();
}

// ── 两应用同实现、不同实例 ────────────────────────────────────

#[test]
fn test_shared_parser_serves_both_apps_with_distinct_agent() {
    let source = WorkBuddySource::new(fixture_root(), AgentKind::WorkBuddyAi);
    let (files, _) = source.discover_with_errors();
    assert!(!files.is_empty());
    let (events, _, _) = collect(&source);
    assert_eq!(events.len(), 5);
    assert!(events.iter().all(|e| e.agent == AgentKind::WorkBuddyAi));
}
