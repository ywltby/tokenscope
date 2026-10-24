//! WB02：两个 WorkBuddy 应用的枚举、身份与去重契约。
//!
//! 身份不变量（计划 §3-5）：身份按 `(app, sessionId, messageId)` 区分；
//! 同样的内容出现在两个应用的根下时，作为两个应用的事件分别计入，
//! 不跨应用合并，也不触发跨应用去重。
//! 去重规则（WB02）：两个新应用都按 `(session_id, record_id)` 强身份去重，
//! 同一身份行序靠后者胜，与 Claude 规则一致。
//! 历史库：`usage_events.app` 为 TEXT，新 app 值写入读出往返；库结构不变。

use jiff::Timestamp;
use tokenscope::dedupe::dedupe_events;
use tokenscope::history::{EventAlias, EventWrite, HistoryDb, WritePrecedence};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};

fn event(agent: AgentKind, ts: &str, session: &str, record: &str) -> UsageEvent {
    UsageEvent {
        ts: ts.parse::<Timestamp>().unwrap(),
        agent,
        model: "m".into(),
        session_id: session.into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: record.into(),
        line: 0,
        source_path: String::new(),
        input_tokens: 10,
        output_tokens: 10,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
    }
}

#[test]
fn test_agent_kind_roundtrip_and_display() {
    assert_eq!(AgentKind::parse("workbuddy").unwrap(), AgentKind::WorkBuddy);
    assert_eq!(
        AgentKind::parse("workbuddy-ai").unwrap(),
        AgentKind::WorkBuddyAi
    );
    assert_eq!(AgentKind::WorkBuddy.as_str(), "workbuddy");
    assert_eq!(AgentKind::WorkBuddyAi.as_str(), "workbuddy-ai");
    assert!(AgentKind::parse("claude-code").is_ok());
    assert!(AgentKind::parse("workbuddyx").is_err(), "未知应用必须报错");
    assert_eq!(AgentKind::ALL.len(), 4);
    // serde 往返（存储 app 为 TEXT）
    assert_eq!(
        serde_json::to_value(AgentKind::WorkBuddy).unwrap(),
        serde_json::json!("workbuddy")
    );
    assert_eq!(
        serde_json::to_value(AgentKind::WorkBuddyAi).unwrap(),
        serde_json::json!("workbuddy-ai")
    );
}

#[test]
fn test_native_identity_workbuddy_message_scheme() {
    let e = event(AgentKind::WorkBuddy, "2026-08-01T10:00:00Z", "s1", "msg-1");
    let (scheme, value) = e.native_identity().expect("强身份必须存在");
    assert_eq!(scheme, "workbuddy-message");
    assert_eq!(value, "s1|msg-1");
    let e2 = event(
        AgentKind::WorkBuddyAi,
        "2026-08-01T10:00:00Z",
        "s1",
        "msg-1",
    );
    let (scheme2, value2) = e2.native_identity().expect("强身份必须存在");
    assert_eq!(scheme2, "workbuddy-message");
    assert_eq!(
        value2, "s1|msg-1",
        "方案同 Claude；app 字段在库层区分两应用"
    );
    // 缺会话或记录 ID → 不猜测身份
    let empty_session = event(AgentKind::WorkBuddy, "2026-08-01T10:00:00Z", "", "msg-1");
    assert!(empty_session.native_identity().is_none());
    let empty_record = event(AgentKind::WorkBuddy, "2026-08-01T10:00:00Z", "s1", "");
    assert!(empty_record.native_identity().is_none());
}

#[test]
fn test_dedupe_workbuddy_strong_identity_same_app_keeps_last() {
    // 同应用同 (session, record_id)：保时间戳最晚（与 Claude 一致）。
    let events = vec![
        event(AgentKind::WorkBuddy, "2026-08-01T10:00:00Z", "s1", "m1"),
        event(AgentKind::WorkBuddy, "2026-08-01T10:05:00Z", "s1", "m1"),
        event(AgentKind::WorkBuddyAi, "2026-08-01T10:00:00Z", "s2", "m2"),
        event(AgentKind::WorkBuddyAi, "2026-08-01T10:05:00Z", "s2", "m2"),
    ];
    let (kept, dropped) = dedupe_events(events);
    assert_eq!(kept.len(), 2, "每个应用各保一条（末条）");
    let wb: Vec<&UsageEvent> = kept
        .iter()
        .filter(|e| e.agent == AgentKind::WorkBuddy)
        .collect();
    assert_eq!(wb.len(), 1);
    assert_eq!(wb[0].input_tokens, 10);
    let wba: Vec<&UsageEvent> = kept
        .iter()
        .filter(|e| e.agent == AgentKind::WorkBuddyAi)
        .collect();
    assert_eq!(wba.len(), 1);
    let dropped_sum: u64 = dropped.iter().map(|(_, n)| n).sum();
    assert_eq!(dropped_sum, 2, "各应用丢弃一条");
}

#[test]
fn test_dedupe_cross_app_same_identity_not_merged() {
    // 不变量 5：两个应用的同一 sessionId|messageId 互不合并。
    let events = vec![
        event(
            AgentKind::WorkBuddy,
            "2026-08-01T10:00:00Z",
            "shared",
            "same",
        ),
        event(
            AgentKind::WorkBuddyAi,
            "2026-08-01T10:00:00Z",
            "shared",
            "same",
        ),
    ];
    let (kept, dropped) = dedupe_events(events);
    assert_eq!(kept.len(), 2, "跨应用同身份不得合并");
    assert!(dropped.is_empty());
}

#[test]
fn test_dedupe_workbuddy_identical_rows_still_merge_by_identity() {
    // 强身份下即使四桶不同（如流式终值更新），也按身份合并保末条——
    // 不是 Codex 那样的保守五元组规则。
    let mut a = event(AgentKind::WorkBuddy, "2026-08-01T10:00:00Z", "s1", "m1");
    a.input_tokens = 100;
    let mut b = event(AgentKind::WorkBuddy, "2026-08-01T10:01:00Z", "s1", "m1");
    b.input_tokens = 200;
    let (kept, dropped) = dedupe_events(vec![a, b]);
    assert_eq!(kept.len(), 1, "同身份不同用量仍按强身份合并保末条");
    assert_eq!(kept[0].input_tokens, 200);
    assert_eq!(dropped, vec![(AgentKind::WorkBuddy, 1)]);
}

// ── 历史库往返（WB02：app 值写入读出、两应用互不合并）────────────

fn wb_write(key: &str, app: AgentKind, session: &str, record: &str) -> EventWrite {
    EventWrite {
        event_key: format!("{}|native|{session}|{record}|{key}", app.as_str()),
        app,
        ts: "2026-08-01T10:00:00Z".parse().unwrap(),
        model_raw: "deepseek-v4.1-flash".into(),
        model_identity: "deepseekv41flash".into(),
        session_id: Some(session.into()),
        record_id: Some(record.into()),
        project_key: Some("C:/work/demo".into()),
        session_initial_cwd: Some("C:/work/demo".into()),
        event_cwd: Some("C:/work/demo".into()),
        tokens: TokenCounts {
            input: 100,
            output: 20,
            cache_write: 0,
            cache_read: 50,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: "2026-08-01T10:00:00Z".parse().unwrap(),
        aliases: vec![EventAlias {
            app,
            scheme: "workbuddy-message".into(),
            value: format!("{session}|{record}"),
        }],
        origins: Vec::new(),
    }
}

#[test]
fn test_history_workbuddy_app_roundtrip_and_cross_app_isolation() {
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-wb02-history-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("history.db");
    let h = HistoryDb::open(&path).unwrap();

    // 同一 (session, record) 写入两个应用：两条独立事件（不变量 5）。
    let batch = vec![
        wb_write("a", AgentKind::WorkBuddy, "shared-sess", "shared-msg"),
        wb_write("b", AgentKind::WorkBuddyAi, "shared-sess", "shared-msg"),
    ];
    let summary = h.write_batch(&batch).unwrap();
    assert_eq!(summary.inserted, 2, "两应用的同一身份各记一条");

    // 读回：app 值精确往返；每应用各查到一条。
    let all = h.stored_events_filtered(None).unwrap();
    assert_eq!(all.len(), 2);
    let apps: Vec<AgentKind> = all.iter().map(|e| e.app).collect();
    assert!(apps.contains(&AgentKind::WorkBuddy));
    assert!(apps.contains(&AgentKind::WorkBuddyAi));
    let wb = h
        .stored_events_filtered(Some(AgentKind::WorkBuddy))
        .unwrap();
    assert_eq!(wb.len(), 1);
    assert_eq!(wb[0].session_id.as_deref(), Some("shared-sess"));
    assert_eq!(wb[0].record_id.as_deref(), Some("shared-msg"));
    let wba = h
        .stored_events_filtered(Some(AgentKind::WorkBuddyAi))
        .unwrap();
    assert_eq!(wba.len(), 1);

    // 幂等：同批重写不新增。
    let again = h.write_batch(&batch).unwrap();
    assert_eq!(again.inserted, 0);
    assert_eq!(again.unchanged, 2);

    // 关闭重开：持久读回（库结构未变的"升级"路径）。
    drop(h);
    let h2 = HistoryDb::open(&path).unwrap();
    let all2 = h2.stored_events_filtered(None).unwrap();
    assert_eq!(all2.len(), 2, "重开后事件保留");
    std::fs::remove_dir_all(&dir).ok();
}
