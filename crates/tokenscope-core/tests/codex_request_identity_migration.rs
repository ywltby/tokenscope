//! Codex 请求身份旧库恢复（计划 2026-11-08，K04）。
//!
//! 背景：v6 及更早的解析把 `token_usage_record` 挂起后按相邻
//! `token_count.last_token_usage` 出数，事件没有任何请求身份；历史库因此
//! 用 `(session, model, 四桶)` 保守弱指纹把四桶相同的独立响应合并成一条
//! 旧事实。升级到强响应身份后，重新解析同一文件会得到更多独立请求——
//! 必须能证明"哪些旧事实对应哪次响应"，才允许原行升级补回；证明不了就
//! 保持原值并披露边界。
//!
//! 映射规则（不按时间窗口猜测）：旧事实的来源键是
//! `native|codex|<路径>|<行序>|<内容指纹>`，内容指纹由旧解析产物的
//! `(ts, model, session, record_id, source_path, 四桶)` 决定。重新解析时，
//! 新解析器为同一文件**按旧解析器行为**逐行重建一份"镜像来源观察"
//! （record-only 行使用旧实现会采用的配对 count 行时间戳与四桶），镜像键
//! 与旧事实来源键**逐字节相等**才算建立映射；模型等价身份与四桶一致只在
//! 映射建立后做校验，不能单独证明同一请求。真实 Codex 0.166.0 落盘中
//! record 与 count 可相差 1–3ms，镜像时间戳必须取旧实现实际出数的那一行，
//! 因此旧 ts 与新强身份事件 ts 不同不是障碍，也不引入模糊窗口。
//!
//! 本文件只使用合成日志与合成旧库（真实 agent 日志不入库、不动用户数据）；
//! 历史库、缓存全部落在测试独立临时目录。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{HistoryDb, PARSER_REVISION};
use tokenscope::report::{SummaryOptions, SummaryReport, summary};

use rusqlite::Connection;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-codex-identity-mig-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts(dir: &Path, root: &Path) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        by: GroupBy::Agent,
        claude_dir: Some(dir.join("no-claude")),
        claude_enabled: Some(false),
        codex_dir: Some(root.to_path_buf()),
        codex_enabled: Some(true),
        cache_dir: Some(dir.join("data")),
        history_path: Some(dir.join("data").join("history.db")),
        pricing_index: Some(dir.join("data").join("idx.json")),
        pricing_path: Some(dir.join("data").join("pricing.toml")),
        openrouter_path: Some(dir.join("data").join("or.json")),
        modelsdev_path: Some(dir.join("data").join("md.json")),
        claude_projects_path: Some(dir.join("no-claude-projects.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    }
}

fn report(dir: &Path, root: &Path) -> SummaryReport {
    summary(&opts(dir, root)).unwrap()
}

const MODEL: &str = "gpt-5.1";
const PROJECT: &str = "C:/synthetic/codex-identity-mig";

#[derive(Clone, Copy)]
struct Usage {
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

const USAGE_1100: Usage = Usage {
    input: 1000,
    cached: 600,
    cache_write: 100,
    output: 100,
    reasoning: 40,
    total: 1100,
};

impl Usage {
    fn value(self) -> serde_json::Value {
        serde_json::json!({
            "input_tokens": self.input,
            "cached_input_tokens": self.cached,
            "cache_write_input_tokens": self.cache_write,
            "output_tokens": self.output,
            "reasoning_output_tokens": self.reasoning,
            "total_tokens": self.total,
        })
    }
}

fn record(session: &str, turn: &str, response: &str, usage: Usage, ts: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": ts,
        "type": "token_usage_record",
        "payload": {
            "thread_id": session,
            "turn_id": turn,
            "session_id": session,
            "root_turn_id": turn,
            "response_id": response,
            "usage": usage.value(),
            "turn_token_usage": usage.value(),
            "thread_token_usage": usage.value(),
        }
    })
}

fn count(last: Usage, total: Usage, ts: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": ts,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "total_token_usage": total.value(),
                "last_token_usage": last.value(),
                "last_usage_includes_reasoning": false,
                "model_context_window": 258400,
            }
        }
    })
}

fn meta(session: &str, ts: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": ts,
        "type": "session_meta",
        "payload": {"id": session, "session_id": session, "cwd": PROJECT}
    })
}

fn ctx(ts: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": ts,
        "type": "turn_context",
        "payload": {"turn_id": "ctx", "model": MODEL, "cwd": PROJECT}
    })
}

fn write_rollout(root: &Path, name: &str, lines: &[serde_json::Value]) -> PathBuf {
    let file = root.join("sessions").join(format!("{name}.jsonl"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let body = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, body + "\n").unwrap();
    file
}

/// 时间戳加毫秒。
fn offset_ms(ts: &str, ms: i64) -> String {
    let mut t = ts.parse::<jiff::Timestamp>().unwrap();
    t += jiff::Span::new().milliseconds(ms);
    t.to_string()
}

/// 旧 v6 事实写入形态：无身份、弱别名（codex-usage）、
/// parser_revision = PARSER_REVISION（当前版本写出的正常行升级前就长这样，
/// 修复只改新解析产物的身份，不预先改旧事实）。
fn legacy_write(v6_event: &tokenscope::model::UsageEvent) -> tokenscope::history::EventWrite {
    use tokenscope::history::{
        EventAlias, EventOrigin, EventWrite, ORIGIN_KIND_NATIVE_FILE, WritePrecedence,
    };
    use tokenscope::model::{AgentKind, TokenCounts};
    use tokenscope::model_identity::ModelIdentity;
    let key = format!(
        "native|{}|{}|{}|{}",
        v6_event.agent.as_str(),
        v6_event.source_path,
        v6_event.line,
        v6_event.content_fingerprint()
    );
    let (scheme, value) = v6_event.same_source_identity().unwrap();
    EventWrite {
        event_key: key.clone(),
        app: AgentKind::Codex,
        ts: v6_event.ts,
        model_raw: v6_event.model.clone(),
        model_identity: ModelIdentity::parse(&v6_event.model).identity_key(),
        session_id: Some(v6_event.session_id.clone()),
        record_id: None,
        project_key: Some(v6_event.project.clone()),
        session_initial_cwd: v6_event.session_initial_cwd.clone(),
        event_cwd: v6_event.event_cwd.clone(),
        tokens: TokenCounts {
            input: v6_event.input_tokens,
            output: v6_event.output_tokens,
            cache_write: v6_event.cache_write_tokens,
            cache_read: v6_event.cache_read_tokens,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: v6_event.ts,
        aliases: vec![EventAlias {
            app: AgentKind::Codex,
            scheme: scheme.to_string(),
            value,
        }],
        origins: vec![EventOrigin {
            origin_kind: ORIGIN_KIND_NATIVE_FILE.to_string(),
            origin_key: key,
            app: AgentKind::Codex,
            import_run_id: None,
            parser_revision: PARSER_REVISION,
            source_model_raw: None,
            pricing_model: None,
            source_cost_usd: None,
            ts_precision_seconds: false,
        }],
    }
}

fn history_rows(dir: &Path) -> Vec<tokenscope::history::StoredEvent> {
    let db = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    db.stored_events().unwrap()
}

fn legacy_usage(
    ts: &str,
    session: &str,
    source_path: &Path,
    line: u64,
) -> tokenscope::model::UsageEvent {
    tokenscope::model::UsageEvent {
        ts: ts.parse().unwrap(),
        agent: tokenscope::model::AgentKind::Codex,
        model: MODEL.to_string(),
        session_id: session.to_string(),
        project: PROJECT.to_string(),
        session_initial_cwd: Some(PROJECT.to_string()),
        event_cwd: Some(PROJECT.to_string()),
        record_id: String::new(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line,
        source_path: source_path.display().to_string(),
        input_tokens: 300,
        output_tokens: 100,
        cache_write_tokens: 100,
        cache_read_tokens: 600,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

// ── 场景 1：record/count 时间相差 1–3ms 的三响应文件 ─────────────────────
//
// 真实形态：同一响应 record 在 t、配对 count 在 t+1~3ms；三个独立响应四桶
// 相同。旧库只有 1 条（count 行 ts 的弱事实）。升级后必须恰好恢复 3 条，
// 不是 1+3=4 条：被映射的旧事实原行升级为第一个响应，其余响应照常插入。

#[test]
fn migration_with_record_count_time_gap_restores_three_requests() {
    let dir = temp_dir("time-gap");
    let root = dir.join("codex");
    let data = dir.join("data");
    let s = "sess-gap";
    // 三轮：record 在整秒，count 在 +1/+2/+3ms（真实落盘节奏）。
    let mut lines = vec![
        meta(s, "2026-11-07T16:00:00.000Z"),
        ctx("2026-11-07T16:00:00.000Z"),
    ];
    let mut cumulative = Usage {
        input: 0,
        cached: 0,
        cache_write: 0,
        output: 0,
        reasoning: 0,
        total: 0,
    };
    let mut count_ts = Vec::new();
    for (n, ts) in [
        (1u64, "2026-11-07T16:00:01.000Z"),
        (2, "2026-11-07T16:00:02.000Z"),
        (3, "2026-11-07T16:00:03.000Z"),
    ] {
        cumulative = Usage {
            input: cumulative.input + USAGE_1100.input,
            cached: cumulative.cached + USAGE_1100.cached,
            cache_write: cumulative.cache_write + USAGE_1100.cache_write,
            output: cumulative.output + USAGE_1100.output,
            reasoning: cumulative.reasoning + USAGE_1100.reasoning,
            total: cumulative.total + USAGE_1100.total,
        };
        let cts = offset_ms(ts, n as i64);
        count_ts.push(cts.clone());
        lines.push(record(
            s,
            "turn-1",
            &format!("resp_synthetic_{n}"),
            USAGE_1100,
            ts,
        ));
        lines.push(count(USAGE_1100, cumulative, &cts));
    }
    let file = write_rollout(&root, "rollout-gap", &lines);

    // 旧库：v6 解析只会从每条 count 出数（四桶相同弱指纹）→ 1 条，ts 是
    // 第一条 count 的时刻（重播族取最早）。
    let history = HistoryDb::open(&data.join("history.db")).unwrap();
    let old = legacy_usage(
        &count_ts[0],
        s,
        &file,
        // 0-based：meta=0, ctx=1, record=2, count=3
        3,
    );
    let seeded = history.write_batch(&[legacy_write(&old)]).unwrap();
    assert_eq!(seeded.inserted, 1);
    drop(history);
    let before = history_rows(&dir);
    assert_eq!(before.len(), 1, "旧库预置 1 条弱事实");
    let old_id = before[0].id;
    let old_event = before[0].clone();

    // 升级采集（新解析器已生效后的首轮）。
    let r = report(&dir, &root);
    let debug_rows = history_rows(&dir);
    for (i, row) in debug_rows.iter().enumerate() {
        eprintln!(
            "DBG[{i}] id={} key={} ts={} session={:?} record={:?} tokens={:?}",
            row.id, row.event_key, row.ts, row.session_id, row.record_id, row.tokens
        );
    }
    assert_eq!(
        r.totals.requests, 3,
        "三个响应各计一次；不是旧 1 条 + 新 3 条 = 4 条"
    );
    assert_eq!(r.totals.tokens.total(), 3300);

    // 旧事实行必须被升级复用（保留原 id），不是删除重建。
    let after = history_rows(&dir);
    assert_eq!(after.len(), 3, "库内恰好 3 条事实");
    let upgraded = after
        .iter()
        .find(|e| e.id == old_id)
        .expect("旧事实 id 保留");
    assert_eq!(
        upgraded.tokens.total(),
        1100,
        "升级只改变身份归属，不叠加任何 token"
    );
    assert_eq!(
        upgraded.record_id.as_deref(),
        Some("resp_synthetic_1"),
        "被映射的旧事实升级为第一条响应"
    );
    assert_eq!(
        after.iter().filter(|e| e.record_id.is_some()).count(),
        3,
        "全部事实都有强身份"
    );
    assert_eq!(
        after
            .iter()
            .filter(|e| e.session_id.as_deref() == Some(s))
            .count(),
        3
    );
    let _ = old_event;
    std::fs::remove_dir_all(&dir).ok();
}

// ── 场景 1b：旧库同用量事实 + 新 turn 后的独立 count-only ────────────────
//
// 旧解析器忽略 turn_context，会把强记录 A 与**新 turn 后**同用量的 count B
// 配成一次调用，旧库因此只有 1 条弱事实（ts/来源行 = count 行）。新解析器
// 已用 turn 边界把 A、B 区分为两个请求：旧事实是 A（镜像键证明旧解析器在
// 该 count 行给 A 记账），B 必须额外计入。升级后 2200/2，不是保留 1100/1
// 或叠成 3300/3；旧行 id 保留，B 是无强身份的独立弱事实。

#[test]
fn migration_same_usage_count_only_after_new_turn_restores_two_requests() {
    let dir = temp_dir("boundary-split-mig");
    let root = dir.join("codex");
    let data = dir.join("data");
    let s = "sess-boundary-mig";
    let count_ts = offset_ms("2026-11-07T16:00:02.000Z", 100);
    let file = write_rollout(
        &root,
        "rollout-boundary-mig",
        &[
            meta(s, "2026-11-07T16:00:00Z"),
            ctx("2026-11-07T16:00:00Z"),
            record(
                s,
                "turn-1",
                "resp_synthetic_1",
                USAGE_1100,
                "2026-11-07T16:00:01.000Z",
            ),
            // 新 turn 边界：新解析器据此判定 A 的配对窗口已终结。
            ctx("2026-11-07T16:00:02.000Z"),
            count(USAGE_1100, USAGE_1100, &count_ts),
        ],
    );
    // 旧库：v6 忽略 turn_context，把 A 配对到 B 的 count 行，只有 1 条
    // 弱事实（0-based：meta=0, ctx=1, record=2, ctx=3, count=4）。
    let history = HistoryDb::open(&data.join("history.db")).unwrap();
    let old_id = {
        let seeded = history
            .write_batch(&[legacy_write(&legacy_usage(&count_ts, s, &file, 4))])
            .unwrap();
        assert_eq!(seeded.inserted, 1);
        history_rows(&dir)[0].id
    };
    drop(history);

    let out = report(&dir, &root);
    assert_eq!(
        out.totals.requests, 2,
        "旧事实升级为 A，新 turn 后的 B 独立计入"
    );
    assert_eq!(out.totals.tokens.total(), 2200);
    let after = history_rows(&dir);
    assert_eq!(after.len(), 2, "旧 1 条升级 + B 新增 = 2 条，不残留");
    let upgraded = after.iter().find(|e| e.id == old_id).expect("旧行 id 保留");
    assert_eq!(
        upgraded.record_id.as_deref(),
        Some("resp_synthetic_1"),
        "旧行升级为 A，保留强身份"
    );
    assert_eq!(upgraded.tokens.total(), 1100);
    let added = after.iter().find(|e| e.id != old_id).unwrap();
    assert_eq!(added.record_id, None, "B 是无强身份的独立弱事实");
    assert_eq!(added.tokens.total(), 1100);

    // 重扫与重启：不增不减。
    let mut refresh_opts = opts(&dir, &root);
    refresh_opts.refresh = true;
    let rescanned = summary(&refresh_opts).unwrap();
    assert_eq!(rescanned.totals.requests, 2);
    assert_eq!(rescanned.totals.tokens.total(), 2200);
    assert_eq!(history_rows(&dir).len(), 2);
    let restarted = report(&dir, &root);
    assert_eq!(restarted.totals.requests, 2);
    assert_eq!(restarted.totals.tokens.total(), 2200);
    assert_eq!(history_rows(&dir).len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 场景 2：legacy fork 旧行错挂父会话 ──────────────────────────────────
//
// 真实形态：legacy fork 子文件 = session_meta(子) → session_meta(父) → 复制
// 的父请求 A（thread=父）→ 子新请求 B（thread=子，与 A 四桶相同）。旧解析
// 把 B 归到父 session 并与 A 弱合并 → 旧库 1 条（ts=A 的 count 时刻）。
// 升级后：父复制 A 只计一次（子文件里的副本命中父线程强身份），B 独立计入；
// 旧行升级为 A，不额外残留。

#[test]
fn migration_legacy_fork_misattributed_row_splits_parent_and_child() {
    let dir = temp_dir("fork-mig");
    let root = dir.join("codex");
    let data = dir.join("data");
    let parent = "sess-fork-parent";
    let child = "sess-fork-child";
    // 父 rollout：A 一轮（count 在 +2ms）。
    let mut parent_lines = vec![
        meta(parent, "2026-11-07T16:00:00.000Z"),
        ctx("2026-11-07T16:00:00.000Z"),
    ];
    let a_count_ts = offset_ms("2026-11-07T16:00:01.000Z", 2);
    parent_lines.push(record(
        parent,
        "turn-a",
        "resp_synthetic_1",
        USAGE_1100,
        "2026-11-07T16:00:01.000Z",
    ));
    parent_lines.push(count(USAGE_1100, USAGE_1100, &a_count_ts));
    let parent_file = write_rollout(&root, "rollout-fork-parent", &parent_lines);
    // legacy fork 子文件：完整复制结构（session_meta 双条 + 父请求副本 + 子新请求 B）。
    let child_lines = vec![
        serde_json::json!({
            "timestamp": "2026-11-07T16:00:02.000Z",
            "type": "session_meta",
            "payload": {"id": child, "session_id": child, "cwd": PROJECT}
        }),
        serde_json::json!({
            "timestamp": "2026-11-07T16:00:00.000Z",
            "type": "session_meta",
            "payload": {"id": parent, "session_id": parent, "cwd": PROJECT}
        }),
        ctx("2026-11-07T16:00:00.000Z"),
        record(
            parent,
            "turn-a",
            "resp_synthetic_1",
            USAGE_1100,
            "2026-11-07T16:00:01.000Z",
        ),
        count(USAGE_1100, USAGE_1100, &a_count_ts),
        ctx("2026-11-07T16:00:02.000Z"),
        record(
            child,
            "turn-b",
            "resp_synthetic_2",
            USAGE_1100,
            "2026-11-07T16:00:03.000Z",
        ),
        count(
            USAGE_1100,
            USAGE_1100,
            &offset_ms("2026-11-07T16:00:03.000Z", 3),
        ),
    ];
    let child_file = write_rollout(&root, "rollout-fork-child", &child_lines);

    // 旧库：v6 把子文件的 B 也归到父 session；A（父文件与子文件副本）与 B
    // 四桶相同、同模型 → 弱指纹合并成 1 条父会话事实（重播族取最早 ts）。
    // 旧实现两个文件都解析：父文件 A 的 count（ts=a_count_ts）与子文件 A 副本、
    // B（都归父 session）同族；B 的 count 在 +3ms 更晚 → 保留最早 a_count_ts。
    // 旧行的来源键取父文件 A 的 count 行（0-based 行 3）。
    let history = HistoryDb::open(&data.join("history.db")).unwrap();
    let old = legacy_usage(&a_count_ts, parent, &parent_file, 3);
    let seeded = history.write_batch(&[legacy_write(&old)]).unwrap();
    assert_eq!(seeded.inserted, 1);
    drop(history);
    let before = history_rows(&dir);
    assert_eq!(before.len(), 1);
    let old_id = before[0].id;
    assert_eq!(
        before[0].session_id.as_deref(),
        Some(parent),
        "旧行错挂父会话"
    );
    let _ = child_file;

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "父请求 A（含子文件副本）只计一次，子新请求 B 独立计入"
    );
    assert_eq!(r.totals.tokens.total(), 2200);

    let after = history_rows(&dir);
    assert_eq!(after.len(), 2, "库内恰好 2 条：升级的 A + 新增的 B");
    let upgraded = after.iter().find(|e| e.id == old_id).expect("旧行 id 保留");
    assert_eq!(
        upgraded.session_id.as_deref(),
        Some(parent),
        "被映射旧行升级为父线程的 A"
    );
    assert_eq!(upgraded.record_id.as_deref(), Some("resp_synthetic_1"));
    let child_event = after.iter().find(|e| e.id != old_id).unwrap();
    assert_eq!(
        child_event.session_id.as_deref(),
        Some(child),
        "B 归子线程，不再错挂父会话"
    );
    assert_eq!(child_event.record_id.as_deref(), Some("resp_synthetic_2"));
    assert_eq!(upgraded.tokens.total() + child_event.tokens.total(), 2200);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 边界：无法可靠映射的旧事实保持原值 ──────────────────────────────────
//
// 日志已被删除（来源不存在）：升级无法重建镜像观察，旧事实必须原样保留，
// 不按数值猜测升级，也不报错。

#[test]
fn unmappable_legacy_fact_is_kept_when_source_missing() {
    let dir = temp_dir("unmappable");
    let data = dir.join("data");
    let history = HistoryDb::open(&data.join("history.db")).unwrap();
    let orphan = legacy_usage(
        "2026-11-07T16:00:01.001Z",
        "sess-gone",
        Path::new("C:/synthetic/gone/rollout-gone.jsonl"),
        3,
    );
    let seeded = history.write_batch(&[legacy_write(&orphan)]).unwrap();
    assert_eq!(seeded.inserted, 1);
    drop(history);
    let before = history_rows(&dir);

    // 空根采集：无来源可重解析，旧事实原样保留。
    let root = dir.join("codex");
    std::fs::create_dir_all(&root).unwrap();
    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "无证据不动旧事实");
    assert_eq!(history_rows(&dir), before, "库内容逐字段不变");
    std::fs::remove_dir_all(&dir).ok();
}

// ── 复审反例 1（2026-11-08）：旧弱事实携带多个 count 来源 ─────────────────
//
// 旧库增量累计：两个同四桶响应在旧实现下各出一条旧事实，第二条因弱指纹
// 相同被合并进第一条（真实写入 API 分两次批次，同一弱别名指向同一行，
// 该行保留**两条** count 来源）。升级后两个响应的镜像键分别对应各自的
// 来源行，但两键都指向同一条事实——第二个响应不得被已升级为第一个响应
// 的行吸收。预期 2200/2；缺陷形态 1100/1（resp2 被 Stale 静默丢弃）。

/// 构造旧库"一条弱事实 + 多条 count 来源"：按真实历史写入路径分多次
/// `write_batch`——第一条建立事实，后续条目经弱别名命中同一条事实
/// （Stale 不改值）并把各自的来源键登记到该行。
fn seed_multi_origin_legacy_fact(dir: &Path, file: &Path, entries: &[(&str, u64)]) -> i64 {
    let db = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    for (ts, line) in entries {
        let write = legacy_write(&legacy_usage(ts, "s", file, *line));
        db.write_batch(&[write]).unwrap();
    }
    let rows = history_rows(dir);
    assert_eq!(rows.len(), 1, "旧弱身份把多条来源合并为一条事实");
    drop(db);
    rows[0].id
}

/// 两来源旧行升级拆分：断言请求/四桶/身份/来源归属。
#[test]
fn review_old_merged_fact_with_multiple_origins_splits() {
    let dir = temp_dir("review-multi-origin");
    let root = dir.join("codex");
    let data = dir.join("data");
    let ts = "2026-11-07T16:00:00Z";
    let ts2 = offset_ms(ts, 1000);
    let file = write_rollout(
        &root,
        "rollout-origins",
        &[
            meta("s", ts),
            ctx(ts),
            record("s", "t1", "r1", USAGE_1100, ts),
            count(USAGE_1100, USAGE_1100, &offset_ms(ts, 1)),
            record("s", "t2", "r2", USAGE_1100, &ts2),
            count(USAGE_1100, USAGE_1100, &offset_ms(&ts2, 1)),
        ],
    );
    let old_id = seed_multi_origin_legacy_fact(
        &dir,
        &file,
        &[(&offset_ms(ts, 1), 3), (&offset_ms(&ts2, 1), 5)],
    );
    // 升级前旧事实确实带两条来源。
    {
        let connection = Connection::open(data.join("history.db")).unwrap();
        let origins: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM event_origins WHERE event_id = ?1",
                [old_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(origins, 2, "旧行保留两条 count 来源");
    }
    let out = report(&dir, &root);
    assert_eq!(
        out.totals.requests, 2,
        "不同强响应必须拆分，尽管旧事实带两条来源"
    );
    assert_eq!(out.totals.tokens.total(), 2200);
    let after = history_rows(&dir);
    assert_eq!(after.len(), 2, "旧 1 条升级 + 新增 1 条 = 2 条，不残留");
    let upgraded = after.iter().find(|e| e.id == old_id).expect("旧行 id 保留");
    assert_eq!(upgraded.record_id.as_deref(), Some("r1"), "旧行升级为 r1");
    assert_eq!(upgraded.tokens.total(), 1100, "升级不叠加 token");
    let added = after.iter().find(|e| e.id != old_id).unwrap();
    assert_eq!(added.record_id.as_deref(), Some("r2"), "r2 独立成事实");
    assert_eq!(added.tokens.total(), 1100);
    std::fs::remove_dir_all(&dir).ok();
}

/// 三个响应的旧行（三条来源）升级拆分：3300/3。
#[test]
fn review_three_origins_old_fact_splits_into_three() {
    let dir = temp_dir("review-three-origin");
    let root = dir.join("codex");
    let ts = "2026-11-07T16:00:00Z";
    let ts2 = offset_ms(ts, 1000);
    let ts3 = offset_ms(ts, 2000);
    let file = write_rollout(
        &root,
        "rollout-three",
        &[
            meta("s", ts),
            ctx(ts),
            record("s", "t1", "r1", USAGE_1100, ts),
            count(USAGE_1100, USAGE_1100, &offset_ms(ts, 1)),
            record("s", "t2", "r2", USAGE_1100, &ts2),
            count(USAGE_1100, USAGE_1100, &offset_ms(&ts2, 1)),
            record("s", "t3", "r3", USAGE_1100, &ts3),
            count(USAGE_1100, USAGE_1100, &offset_ms(&ts3, 1)),
        ],
    );
    let old_id = seed_multi_origin_legacy_fact(
        &dir,
        &file,
        &[
            (&offset_ms(ts, 1), 3),
            (&offset_ms(&ts2, 1), 5),
            (&offset_ms(&ts3, 1), 7),
        ],
    );
    let out = report(&dir, &root);
    assert_eq!(out.totals.requests, 3);
    assert_eq!(out.totals.tokens.total(), 3300);
    let after = history_rows(&dir);
    assert_eq!(after.len(), 3);
    let upgraded = after.iter().find(|e| e.id == old_id).unwrap();
    assert_eq!(upgraded.record_id.as_deref(), Some("r1"));
    let mut ids: Vec<&str> = after
        .iter()
        .filter_map(|e| e.record_id.as_deref())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec!["r1", "r2", "r3"]);
    std::fs::remove_dir_all(&dir).ok();
}

/// 镜像命中顺序反转（旧事实先被 r2 的镜像命中，r1 后到）：仍恰好 2 条。
/// 分批升级（第一轮只写一半日志、第二轮写全）与重扫、重启幂等。
#[test]
fn review_reversed_upgrade_order_and_replays_stay_exact() {
    let dir = temp_dir("review-reversed");
    let root = dir.join("codex");
    let ts = "2026-11-07T16:00:00Z";
    let ts2 = offset_ms(ts, 1000);
    let lines = vec![
        meta("s", ts),
        ctx(ts),
        record("s", "t1", "r1", USAGE_1100, ts),
        count(USAGE_1100, USAGE_1100, &offset_ms(ts, 1)),
        record("s", "t2", "r2", USAGE_1100, &ts2),
        count(USAGE_1100, USAGE_1100, &offset_ms(&ts2, 1)),
    ];
    let file = write_rollout(&root, "rollout-rev", &lines);
    let old_id = seed_multi_origin_legacy_fact(
        &dir,
        &file,
        &[(&offset_ms(ts, 1), 3), (&offset_ms(&ts2, 1), 5)],
    );

    // 分批：第一轮只写前半（meta/ctx/r1/count）——r2 的镜像不在场。
    let first_half: Vec<String> = lines[..4]
        .iter()
        .map(serde_json::Value::to_string)
        .collect();
    std::fs::write(&file, first_half.join("\n") + "\n").unwrap();
    let r1 = report(&dir, &root);
    assert_eq!(r1.totals.requests, 1, "第一批只有 r1");
    let mid = history_rows(&dir);
    assert_eq!(mid.len(), 1);
    assert_eq!(mid[0].record_id.as_deref(), Some("r1"), "旧行升级为 r1");

    // 第二轮：写全量日志（r2 的 record/count 出现）。
    write_rollout(&root, "rollout-rev", &lines);
    let r2 = report(&dir, &root);
    assert_eq!(r2.totals.requests, 2, "r2 独立入账，不被已升级行吸收");
    assert_eq!(r2.totals.tokens.total(), 2200);
    let after = history_rows(&dir);
    assert_eq!(after.len(), 2);
    let upgraded = after.iter().find(|e| e.id == old_id).unwrap();
    assert_eq!(
        upgraded.record_id.as_deref(),
        Some("r1"),
        "先到的 r1 保持代表"
    );
    let added = after.iter().find(|e| e.id != old_id).unwrap();
    assert_eq!(added.record_id.as_deref(), Some("r2"));

    // 重扫（refresh）与重启幂等：数字与库行不增长。
    let mut refresh_opts = opts(&dir, &root);
    refresh_opts.refresh = true;
    let r3 = summary(&refresh_opts).unwrap();
    assert_eq!(r3.totals.requests, 2);
    assert_eq!(history_rows(&dir).len(), 2);
    assert_eq!(report(&dir, &root).totals.requests, 2);
    assert_eq!(history_rows(&dir).len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 复审反例 2（2026-11-08）：compaction 的镜像行号 ────────────────────────
//
// 非零 record + 零 last 的 token_count：旧解析器从**挂起的 record** 出数，
// 事件来源行号 = record 行；缺陷形态里 legacy_mirror 返回了 count 行号——
// 镜像键错位 → 旧行不升级、新强记录另插一行 → 旧 1100/1 变 2200/2。
// 正确行为：升级复用旧行（id 保留、来源行 = record 行），始终 1100/1。

#[test]
fn review_compaction_legacy_mapping_no_extra_fact() {
    let dir = temp_dir("review-compaction");
    let root = dir.join("codex");
    let ts = "2026-11-07T16:00:00Z";
    let zero = Usage {
        input: 0,
        cached: 0,
        cache_write: 0,
        output: 0,
        reasoning: 0,
        total: 0,
    };
    let file = write_rollout(
        &root,
        "rollout-compaction",
        &[
            meta("s", ts),
            ctx(ts),
            record("s", "t", "r", USAGE_1100, ts),
            // compaction：last 全零，旧解析器按挂起 record 出数（来源行 2）。
            count(zero, USAGE_1100, &offset_ms(ts, 1)),
        ],
    );
    // 旧事实：ts = record 行时刻、来源行 = record 行（0-based 2）。
    let db = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let old_id = {
        let seeded = db
            .write_batch(&[legacy_write(&legacy_usage(ts, "s", &file, 2))])
            .unwrap();
        assert_eq!(seeded.inserted, 1);
        history_rows(&dir)[0].id
    };
    drop(db);

    let out = report(&dir, &root);
    assert_eq!(out.totals.requests, 1, "compaction 升级后仍是一条事实");
    assert_eq!(out.totals.tokens.total(), 1100);
    let after = history_rows(&dir);
    assert_eq!(after.len(), 1, "旧行升级复用，不残留第二条");
    let upgraded = after.iter().find(|e| e.id == old_id).unwrap();
    assert_eq!(upgraded.record_id.as_deref(), Some("r"), "旧行升级为强身份");
    assert_eq!(
        upgraded.ts,
        ts.parse().unwrap(),
        "升级后 ts = record 行时刻"
    );

    // 重复采集与重启：幂等，不增长。
    assert_eq!(report(&dir, &root).totals.requests, 1);
    assert_eq!(history_rows(&dir).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 对照：正常非零配对的镜像仍指向 count 行（不要全局替换行号规则）──────

#[test]
fn review_normal_pair_mapping_still_uses_count_line() {
    let dir = temp_dir("review-normal-pair");
    let root = dir.join("codex");
    let ts = "2026-11-07T16:00:00Z";
    let count_ts = offset_ms(ts, 1);
    let file = write_rollout(
        &root,
        "rollout-normal",
        &[
            meta("s", ts),
            ctx(ts),
            record("s", "t", "r", USAGE_1100, ts),
            count(USAGE_1100, USAGE_1100, &count_ts),
        ],
    );
    // 旧事实形态：旧解析器按 count 出数（ts = count 行时刻、来源行 3）。
    let db = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let old_id = {
        let seeded = db
            .write_batch(&[legacy_write(&legacy_usage(&count_ts, "s", &file, 3))])
            .unwrap();
        assert_eq!(seeded.inserted, 1);
        history_rows(&dir)[0].id
    };
    drop(db);
    let out = report(&dir, &root);
    assert_eq!(out.totals.requests, 1);
    assert_eq!(out.totals.tokens.total(), 1100);
    let after = history_rows(&dir);
    assert_eq!(after.len(), 1, "正常配对：旧行经 count 行镜像升级，无新行");
    let upgraded = after.iter().find(|e| e.id == old_id).unwrap();
    assert_eq!(upgraded.record_id.as_deref(), Some("r"));
    std::fs::remove_dir_all(&dir).ok();
}
