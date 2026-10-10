//! H01：持久用量库的存储契约。
//!
//! 覆盖计划 §2 不变量与 §3 表结构：事件/别名/来源唯一约束、幂等重写、
//! 终值更新、竞争写入不复制事件、无变更不递增 generation、u64 边界、
//! 事务失败整批回滚、版本化迁移保留历史、来源缺失不删除已存用量。
//!
//! 全部路径落在临时目录，绝不触碰真实 `~/.tokenscope`（计划不变量 10）。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};

use tokenscope::history::{
    EventAlias, EventOrigin, EventWrite, HistoryDb, SourceFileKey, SourceFileRecord,
    WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "tokenscope-history-storage-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn db(tag: &str) -> (PathBuf, HistoryDb) {
    let dir = temp_dir(tag);
    let path = dir.join("history.db");
    let h = HistoryDb::open(&path).unwrap();
    (dir, h)
}

fn ts(s: &str) -> jiff::Timestamp {
    s.parse().unwrap()
}

fn tokens(input: u64, output: u64, cache_write: u64, cache_read: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write,
        cache_read,
    }
}

fn base_event(key: &str) -> EventWrite {
    EventWrite {
        event_key: key.to_string(),
        app: AgentKind::ClaudeCode,
        ts: ts("2026-07-17T08:00:00Z"),
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("s1".into()),
        record_id: Some("m1".into()),
        project_key: Some("/work/app".into()),
        session_initial_cwd: Some("/work/app".into()),
        event_cwd: Some("/work/app/sub".into()),
        tokens: tokens(10, 5, 1, 2),
        precedence: WritePrecedence::NativeLog,
        observed_at: ts("2026-07-17T08:00:00Z"),
        aliases: Vec::new(),
        origins: Vec::new(),
    }
}

fn native_origin(key: &str) -> EventOrigin {
    EventOrigin {
        origin_kind: "native_file".into(),
        origin_key: key.into(),
        app: AgentKind::ClaudeCode,
        import_run_id: None,
        parser_revision: 1,
        source_model_raw: None,
        pricing_model: None,
        source_cost_usd: None,
        ts_precision_seconds: false,
    }
}

/// 原生身份别名（Claude 的 `(session_id, message.id)`）——原生采集与 CCS
/// 兼容解析都以它作跨来源请求身份。
fn native_identity_alias(session: &str, record: &str) -> EventAlias {
    EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: format!("{session}|{record}"),
    }
}

#[test]
fn idempotent_rewrite_keeps_generation_stable() {
    let (_dir, h) = db("idem");
    let mut e = base_event("claude-code|message|s1|m1");
    e.origins = vec![native_origin("/logs/a.jsonl#0")];

    let first = h.write_batch(&[e.clone()]).unwrap();
    assert_eq!(first.inserted, 1, "首次写入插入一条事件");
    assert_eq!(first.origins_added, 1);
    let g1 = h.generation().unwrap();
    assert_eq!(g1, 1, "首次写入递增一次 generation");

    let second = h.write_batch(&[e.clone()]).unwrap();
    assert_eq!(second.inserted, 0);
    assert_eq!(second.unchanged, 1);
    assert_eq!(second.origins_added, 0, "同一来源记录不重复登记");
    assert_eq!(
        h.generation().unwrap(),
        g1,
        "完全相同的重写不递增 generation"
    );
    assert_eq!(h.event_count().unwrap(), 1);
    assert_eq!(h.origin_count().unwrap(), 1);
    assert_eq!(h.totals().unwrap(), tokens(10, 5, 1, 2), "用量不翻倍");
}

#[test]
fn terminal_value_update_keeps_single_event() {
    let (_dir, h) = db("terminal");
    let e = base_event("claude-code|message|s1|m1");
    h.write_batch(std::slice::from_ref(&e)).unwrap();
    let g1 = h.generation().unwrap();

    // 流式终值：同请求后到的观察带来完整用量。
    let mut later = e.clone();
    later.tokens = tokens(30, 9, 4, 6);
    later.observed_at = ts("2026-07-17T08:00:05Z");
    let s = h.write_batch(&[later.clone()]).unwrap();
    assert_eq!(s.updated, 1);
    assert_eq!(s.inserted, 0);
    assert_eq!(h.event_count().unwrap(), 1, "终值更新不新增事件");
    assert!(h.generation().unwrap() > g1, "用量变化递增 generation");

    let stored = h.stored_events().unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].tokens, later.tokens);
    assert_eq!(stored[0].ts, e.ts);

    // 更旧的观察不得覆盖较新的终值。
    let mut older = e.clone();
    older.tokens = tokens(1, 1, 1, 1);
    older.observed_at = ts("2026-07-17T07:59:00Z");
    let s = h.write_batch(&[older]).unwrap();
    assert_eq!(s.stale, 1, "较旧的观察计为过期，不覆盖");
    assert_eq!(h.stored_events().unwrap()[0].tokens, later.tokens);
}

#[test]
fn ccs_alias_merges_into_native_event_without_double_count() {
    let (_dir, h) = db("alias");
    let mut native = base_event("claude-code|message|s1|m1");
    native.aliases = vec![native_identity_alias("s1", "m1")];
    native.origins = vec![native_origin("/logs/a.jsonl#0")];
    h.write_batch(&[native.clone()]).unwrap();
    let g1 = h.generation().unwrap();

    // 同一请求的 CCS 来源记录：不同来源键，但经兼容规则解析出同一原生身份别名。
    let mut ccs = base_event("ccs|request|42");
    ccs.precedence = WritePrecedence::CcsImport;
    ccs.aliases = vec![EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "s1|m1".into(),
    }];
    ccs.origins = vec![EventOrigin {
        origin_kind: "ccs_request".into(),
        origin_key: "ccs|42".into(),
        app: AgentKind::ClaudeCode,
        import_run_id: None,
        parser_revision: 1,
        source_model_raw: Some("claude-opus-5-5".into()),
        pricing_model: Some("claude-opus-5-5".into()),
        source_cost_usd: Some(0.5),
        ts_precision_seconds: true,
    }];
    let s = h.write_batch(&[ccs]).unwrap();
    assert_eq!(s.inserted, 0, "别名命中已有事件，不新增事件");
    assert_eq!(s.unchanged, 1);
    assert_eq!(s.aliases_added, 0, "同一原生身份已登记，不重复新增别名");
    assert_eq!(s.origins_added, 1, "CCS 来源记录作为来源证据登记");
    assert_eq!(h.event_count().unwrap(), 1);
    assert_eq!(h.alias_count().unwrap(), 1);
    assert_eq!(h.origin_count().unwrap(), 2);
    assert_eq!(h.totals().unwrap(), tokens(10, 5, 1, 2), "交集只算一次");
    assert!(h.generation().unwrap() > g1, "来源证据变化递增 generation");

    // 再导入同一 CCS 记录：完全跳过，行数与 generation 不变。
    let g2 = h.generation().unwrap();
    let mut again = base_event("ccs|request|42");
    again.precedence = WritePrecedence::CcsImport;
    again.aliases = vec![EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "s1|m1".into(),
    }];
    again.origins = vec![EventOrigin {
        origin_kind: "ccs_request".into(),
        origin_key: "ccs|42".into(),
        app: AgentKind::ClaudeCode,
        import_run_id: None,
        parser_revision: 1,
        source_model_raw: Some("claude-opus-5-5".into()),
        pricing_model: Some("claude-opus-5-5".into()),
        source_cost_usd: Some(0.5),
        ts_precision_seconds: true,
    }];
    let s = h.write_batch(&[again]).unwrap();
    assert_eq!(s.unchanged, 1);
    assert_eq!(s.aliases_added, 0);
    assert_eq!(s.origins_added, 0);
    assert_eq!(h.generation().unwrap(), g2, "重复导入不改变任何行");
    assert_eq!(h.totals().unwrap(), tokens(10, 5, 1, 2));
}

#[test]
fn ccs_conflicting_usage_keeps_native_value() {
    let (_dir, h) = db("conflict");
    let mut native = base_event("claude-code|message|s1|m1");
    native.aliases = vec![native_identity_alias("s1", "m1")];
    native.origins = vec![native_origin("/logs/a.jsonl#0")];
    h.write_batch(&[native.clone()]).unwrap();

    let mut ccs = base_event("ccs|request|42");
    ccs.precedence = WritePrecedence::CcsImport;
    ccs.tokens = tokens(999, 999, 0, 0);
    ccs.aliases = vec![native_identity_alias("s1", "m1")];
    let s = h.write_batch(&[ccs]).unwrap();
    assert_eq!(s.conflicts, 1, "CCS 值不得覆盖原生值，冲突显式计数");
    assert_eq!(s.updated, 0);
    assert_eq!(h.event_count().unwrap(), 1);
    assert_eq!(
        h.stored_events().unwrap()[0].tokens,
        tokens(10, 5, 1, 2),
        "保留原生终值（与导入顺序无关）"
    );

    // 反向顺序：先 CCS 后原生 —— 原生值胜出，最终仍是原生值。
    let (_dir2, h2) = db("conflict-order");
    let mut ccs2 = base_event("ccs|request|42");
    ccs2.precedence = WritePrecedence::CcsImport;
    ccs2.tokens = tokens(999, 999, 0, 0);
    ccs2.aliases = vec![native_identity_alias("s1", "m1")];
    h2.write_batch(&[ccs2]).unwrap();
    let mut native2 = base_event("claude-code|message|s1|m1");
    native2.aliases = vec![native_identity_alias("s1", "m1")];
    native2.origins = vec![native_origin("/logs/a.jsonl#0")];
    let s2 = h2.write_batch(&[native2]).unwrap();
    assert_eq!(s2.updated, 1, "原生终值覆盖 CCS 值");
    assert_eq!(h2.event_count().unwrap(), 1);
    assert_eq!(h2.stored_events().unwrap()[0].tokens, tokens(10, 5, 1, 2));
}

#[test]
fn u64_boundary_values_roundtrip_and_overflow_errors() {
    let (_dir, h) = db("u64");
    let mut e = base_event("k-max");
    e.tokens = tokens(u64::MAX, 0, 0, 0);
    h.write_batch(&[e]).unwrap();
    let stored = h.stored_events().unwrap();
    assert_eq!(stored[0].tokens.input, u64::MAX, "u64 边界不经 i64 截断");

    // 两条各自合法的用量合计溢出：明确报错，不回绕、不饱和、不静默吞掉。
    let half = u64::MAX / 2 + 1;
    let (_dir2, h2) = db("u64-overflow");
    for (i, key) in ["k1", "k2"].iter().enumerate() {
        let mut e = base_event(key);
        e.tokens = tokens(half, 0, 0, 0);
        e.observed_at = ts(&format!("2026-07-17T08:00:0{i}Z"));
        h2.write_batch(&[e]).unwrap();
    }
    let err = h2.totals().unwrap_err().to_string();
    assert!(err.contains("溢出"), "明确报错：{err}");
}

#[test]
fn concurrent_writers_do_not_duplicate_events() {
    let dir = temp_dir("concurrent");
    let path = dir.join("history.db");
    drop(HistoryDb::open(&path).unwrap());

    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for t in 0..2 {
        let path = path.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            let h = HistoryDb::open(&path).unwrap();
            barrier.wait();
            for round in 0..25u64 {
                let batch: Vec<EventWrite> = (0..10)
                    .map(|k| {
                        let mut e = base_event(&format!("k{k}"));
                        e.observed_at = ts(&format!("2026-07-17T08:00:00.0000000{}Z", round % 10));
                        e
                    })
                    .collect();
                h.write_batch(&batch).unwrap();
            }
            t
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    let h = HistoryDb::open(&path).unwrap();
    assert_eq!(
        h.event_count().unwrap(),
        10,
        "竞争写入不复制事件（唯一约束 + 写事务串行化）"
    );
    assert_eq!(h.totals().unwrap(), tokens(100, 50, 10, 20));
}

#[test]
fn conflicting_identity_batch_rolls_back_everything() {
    let (_dir, h) = db("rollback");
    let g0 = h.generation().unwrap();
    assert_eq!(g0, 0, "空库 generation 从 0 开始");

    // 库内已有两个不同请求，各自绑定自己的原生身份。
    let alias_a = EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "s1|m1".into(),
    };
    let alias_b = EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "s2|m2".into(),
    };
    let mut e1 = base_event("e1");
    e1.aliases = vec![alias_a.clone()];
    let mut e2 = base_event("e2");
    e2.session_id = Some("s2".into());
    e2.record_id = Some("m2".into());
    e2.ts = ts("2026-07-17T09:00:00Z");
    e2.observed_at = e2.ts;
    e2.aliases = vec![alias_b.clone()];
    h.write_batch(&[e1, e2]).unwrap();
    assert_eq!(h.event_count().unwrap(), 2);
    let g1 = h.generation().unwrap();

    // 矛盾批次：首条合法，第二条同时声称两个已有身份 → 归属无法判定，
    // 必须整批拒绝（不猜测合并、不留半条）。
    let ok = base_event("k-new");
    let mut bad = base_event("k-bad");
    bad.aliases = vec![alias_a, alias_b];
    let err = h.write_batch(&[ok, bad]).unwrap_err().to_string();
    assert!(err.contains("身份冲突"), "必须明确报身份冲突：{err}");

    let stored = h.stored_events().unwrap();
    assert_eq!(stored.len(), 2, "整批回滚：行数不多不少");
    assert!(
        !stored.iter().any(|e| e.event_key == "k-new"),
        "批次首条同样回滚"
    );
    assert_eq!(h.generation().unwrap(), g1, "回滚不递增 generation");

    // 回滚后同一连接仍可正常写入（事务未泄漏、未悬挂）。
    h.write_batch(&[base_event("k3")]).unwrap();
    assert_eq!(h.event_count().unwrap(), 3);
}

#[test]
fn schema_upgrade_preserves_history() {
    let dir = temp_dir("migrate");
    let path = dir.join("history.db");
    // 构造 v1 库（旧版本只有 meta/source_files/usage_events），写入真实用量。
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(tokenscope::history::legacy_v1_schema_for_tests())
            .unwrap();
        conn.execute(
            "INSERT INTO history_meta(key, value) VALUES('generation', '7')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                model_identity, identity_revision, session_id, record_id, project_key,
                session_initial_cwd, event_cwd, input_tokens, output_tokens,
                cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                observed_at_seconds, observed_at_nanos, parser_revision)
             VALUES('legacy|1', 'claude-code', 1786000000, 0, 'claude-opus-5-5',
                'claudeopus55', 1, 's1', 'm1', '/work/app', NULL, NULL,
                '11', '22', '33', '44', 1, '2026-07-17T08:00:00Z', 1786000000, 0, 1)",
            [],
        )
        .unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }

    let h = HistoryDb::open(&path).unwrap();
    assert_eq!(
        h.schema_version().unwrap(),
        tokenscope::history::HISTORY_SCHEMA_VERSION,
        "升级到当前版本"
    );
    assert_eq!(h.event_count().unwrap(), 1, "升级保留历史事件");
    assert_eq!(h.totals().unwrap(), tokens(11, 22, 33, 44));
    assert_eq!(h.generation().unwrap(), 7, "升级保留 generation 计数");
    // v2 表已建立：别名写入可用。
    let mut e = base_event("claude-code|message|s1|m1");
    e.aliases = vec![EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "s1|m1".into(),
    }];
    h.write_batch(&[e]).unwrap();
    assert_eq!(h.alias_count().unwrap(), 1);
}

#[test]
fn unknown_future_schema_is_rejected_and_preserved() {
    let dir = temp_dir("future");
    let path = dir.join("history.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(tokenscope::history::legacy_v1_schema_for_tests())
            .unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let err = HistoryDb::open(&path).unwrap_err().to_string();
    assert!(err.contains("99"), "错误必须指明未知版本：{err}");
    // 原文件与内容仍在（不删表重建）。
    let conn = rusqlite::Connection::open(&path).unwrap();
    let v: i64 = conn
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(v, 99);
    let n: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'usage_events'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1, "未知版本库保留原结构");
}

#[test]
fn missing_source_keeps_usage_and_records_state() {
    let (_dir, h) = db("missing");
    let mut e = base_event("k1");
    e.origins = vec![native_origin("/logs/a.jsonl#0")];
    h.write_batch(&[e]).unwrap();
    let rec = SourceFileRecord {
        key: SourceFileKey {
            app: AgentKind::ClaudeCode,
            root: "/logs".into(),
            path: "/logs/a.jsonl".into(),
        },
        fingerprint: "100:200".into(),
        size_bytes: 100,
        mtime_ms: 200,
        context_revision: "rev1".into(),
        last_success_utc: Some("2026-07-17T08:00:00Z".into()),
    };
    h.touch_source_file(&rec).unwrap();
    assert_eq!(h.source_file_count().unwrap(), 1);

    let n = h
        .mark_sources_missing(std::slice::from_ref(&rec.key))
        .unwrap();
    assert_eq!(n, 1);
    let files = h.source_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].state, "missing", "缺失只更新状态");
    assert_eq!(
        files[0].last_success_utc.as_deref(),
        Some("2026-07-17T08:00:00Z"),
        "保留最近成功采集时间"
    );
    assert_eq!(h.event_count().unwrap(), 1, "来源缺失不删用量");
    assert_eq!(h.totals().unwrap(), tokens(10, 5, 1, 2));

    // 重新出现：状态回到 present，指纹更新。
    let mut rec2 = rec.clone();
    rec2.fingerprint = "300:400".into();
    rec2.size_bytes = 300;
    h.touch_source_file(&rec2).unwrap();
    let files = h.source_files().unwrap();
    assert_eq!(files[0].state, "present");
    assert_eq!(files[0].fingerprint, "300:400");
}

#[test]
fn open_creates_isolated_data_dir_only_under_given_path() {
    // 计划不变量 10：库文件只在调用方给出的（临时）路径下创建。
    let dir = temp_dir("isolated");
    let path = dir.join("nested").join("history.db");
    let h = HistoryDb::open(&path).unwrap();
    assert!(path.exists());
    assert_eq!(h.path(), path.as_path());
    assert!(
        !Path::new(".").join("history.db").exists(),
        "不得在工作目录留下库文件"
    );
}
