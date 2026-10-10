//! H03：遗留 `cache.db` 迁移（计划 §6）。
//!
//! 覆盖：只读读取旧库（不触发版本重建）、仅迁合法字段、源文件已消失仍保留
//! 已缓存用量、重复迁移幂等、失败不落标记可重试、迁移后的原生重扫命中同一
//! 事件不重复计费。全部路径在临时目录（计划不变量 10）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::history::{
    EventAlias, EventWrite, HistoryDb, WritePrecedence, migrate_legacy_cache,
};
use tokenscope::model::{AgentKind, TokenCounts};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-history-migration-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 真实旧版（v10）缓存结构：`files` + `events`，**没有** `line` 列。
const LEGACY_V10_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    root TEXT NOT NULL DEFAULT '',
    agent TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime_ms INTEGER NOT NULL,
    context_rev TEXT NOT NULL DEFAULT '',
    lines_seen INTEGER NOT NULL DEFAULT 0,
    bad_lines INTEGER NOT NULL DEFAULT 0,
    skipped_sidechain INTEGER NOT NULL DEFAULT 0,
    skipped_synthetic INTEGER NOT NULL DEFAULT 0,
    skipped_zero_usage INTEGER NOT NULL DEFAULT 0,
    skipped_no_model INTEGER NOT NULL DEFAULT 0,
    ignored_token_usage_record INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS events (
    file_id INTEGER NOT NULL,
    ts TEXT NOT NULL,
    record_id TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL,
    session_id TEXT NOT NULL,
    project TEXT NOT NULL,
    session_initial_cwd TEXT,
    event_cwd TEXT,
    input INTEGER NOT NULL,
    output INTEGER NOT NULL,
    cache_write INTEGER NOT NULL,
    cache_read INTEGER NOT NULL
);
";

fn seed_legacy_cache(path: &PathBuf, rows: &[(&str, &str, i64, i64, i64, i64)]) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(LEGACY_V10_SCHEMA).unwrap();
    conn.execute(
        "INSERT INTO meta(key, value) VALUES('schema_version', '10')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO files(id, path, root, agent, size, mtime_ms)
         VALUES(1, '/logs/proj-a/sess.jsonl', '/logs', 'claude-code', 100, 200)",
        [],
    )
    .unwrap();
    for (i, (ts, record, input, output, cw, cr)) in rows.iter().enumerate() {
        conn.execute(
            "INSERT INTO events(file_id, ts, record_id, model, session_id, project,
                session_initial_cwd, event_cwd, input, output, cache_write, cache_read)
             VALUES(1, ?1, ?2, 'claude-sonnet-4-5', 's1', '/work/main', '/work/main', NULL,
                    ?3, ?4, ?5, ?6)",
            rusqlite::params![ts, record, input, output, cw, cr],
        )
        .unwrap_or_else(|e| panic!("第 {i} 行插入失败: {e}"));
    }
}

fn tokens(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
    }
}

#[test]
fn legacy_cache_usage_is_migrated_once() {
    let dir = temp_dir("basic");
    let legacy = dir.join("cache.db");
    seed_legacy_cache(
        &legacy,
        &[
            ("2026-07-17T08:00:00.000Z", "m1", 10, 5, 1, 2),
            ("2026-07-17T08:01:00.000Z", "m2", 20, 7, 0, 3),
        ],
    );
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();

    let report = migrate_legacy_cache(&history, &legacy).unwrap().unwrap();
    assert_eq!(report.files, 1);
    assert_eq!(report.events_seen, 2);
    assert_eq!(report.inserted, 2);
    assert_eq!(report.rejected, 0);
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(history.totals().unwrap(), tokens(30, 12, 1, 5));
    assert_eq!(history.origin_count().unwrap(), 2, "每条来源记录登记一次");
    assert_eq!(
        history.alias_count().unwrap(),
        2,
        "原生身份别名可用于重扫合并"
    );

    // 重复迁移：标记已写 → 直接跳过，不再触碰旧库。
    let again = migrate_legacy_cache(&history, &legacy).unwrap();
    assert!(again.is_none(), "已迁移过则不再执行");
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(history.totals().unwrap(), tokens(30, 12, 1, 5));

    // 清掉标记后重跑（等价于"同一份旧库再迁一次"）：全部命中已有事件。
    history.set_meta("legacy_cache_migration", "").unwrap();
    let rerun = migrate_legacy_cache(&history, &legacy).unwrap().unwrap();
    assert_eq!(rerun.inserted, 0, "再次迁移不新增事件");
    assert_eq!(rerun.unchanged, 2);
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(history.totals().unwrap(), tokens(30, 12, 1, 5));
}

#[test]
fn invalid_rows_are_rejected_without_blocking_valid_ones() {
    let dir = temp_dir("reject");
    let legacy = dir.join("cache.db");
    seed_legacy_cache(
        &legacy,
        &[
            ("2026-07-17T08:00:00.000Z", "ok-1", 10, 5, 0, 0),
            ("not-a-timestamp", "bad-1", 999, 999, 0, 0),
            ("2026-07-17T08:02:00.000Z", "ok-2", 1, 1, 0, 0),
        ],
    );
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();
    let report = migrate_legacy_cache(&history, &legacy).unwrap().unwrap();
    assert_eq!(report.events_seen, 3);
    assert_eq!(report.inserted, 2, "合法行照常迁入");
    assert_eq!(report.rejected, 1, "字段不可用的行明确计数并跳过");
    assert_eq!(history.totals().unwrap(), tokens(11, 6, 0, 0));
}

#[test]
fn migration_failure_is_retryable() {
    let dir = temp_dir("retry");
    let legacy = dir.join("cache.db");
    std::fs::write(&legacy, b"this is not a sqlite database").unwrap();
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();

    assert!(
        migrate_legacy_cache(&history, &legacy).is_err(),
        "不可读的旧库必须报错"
    );
    assert!(
        !history.legacy_cache_migrated().unwrap(),
        "失败不写一次性标记 → 可重试"
    );

    // 换上一份合法旧库后重试成功。
    std::fs::remove_file(&legacy).unwrap();
    seed_legacy_cache(&legacy, &[("2026-07-17T09:00:00.000Z", "m9", 3, 2, 0, 0)]);
    let report = migrate_legacy_cache(&history, &legacy).unwrap().unwrap();
    assert_eq!(report.inserted, 1);
    assert!(history.legacy_cache_migrated().unwrap());
    assert_eq!(history.totals().unwrap(), tokens(3, 2, 0, 0));
}

#[test]
fn missing_legacy_cache_marks_done_without_failure() {
    let dir = temp_dir("absent");
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();
    let report = migrate_legacy_cache(&history, &dir.join("no-such-cache.db"))
        .unwrap()
        .unwrap();
    assert_eq!(report.events_seen, 0);
    assert!(history.legacy_cache_migrated().unwrap());
    assert_eq!(history.event_count().unwrap(), 0);
}

#[test]
fn rescan_after_migration_hits_the_same_event() {
    let dir = temp_dir("rescan");
    let legacy = dir.join("cache.db");
    seed_legacy_cache(&legacy, &[("2026-07-17T08:00:00.000Z", "m1", 10, 5, 1, 2)]);
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();
    migrate_legacy_cache(&history, &legacy).unwrap().unwrap();

    // 重扫同一来源文件（H04 的原生写入形态）：来源键不同、原生身份相同、
    // 内容相同 → 命中同一事件，不重复计费，只补一条来源证据。
    let rescan = EventWrite {
        event_key: "native|claude-code|/logs/proj-a/sess.jsonl|0".into(),
        app: AgentKind::ClaudeCode,
        ts: "2026-07-17T08:00:00.000Z".parse().unwrap(),
        model_raw: "claude-sonnet-4-5".into(),
        model_identity: "claudesonnet45".into(),
        session_id: Some("s1".into()),
        record_id: Some("m1".into()),
        project_key: Some("/work/main".into()),
        session_initial_cwd: Some("/work/main".into()),
        event_cwd: None,
        tokens: tokens(10, 5, 1, 2),
        precedence: WritePrecedence::NativeLog,
        observed_at: "2026-07-17T08:00:00.000Z".parse().unwrap(),
        aliases: vec![EventAlias {
            app: AgentKind::ClaudeCode,
            scheme: "claude-message".into(),
            value: "s1|m1".into(),
        }],
        origins: vec![tokenscope::history::EventOrigin {
            origin_kind: "native_file".into(),
            origin_key: "native|claude-code|/logs/proj-a/sess.jsonl|0".into(),
            app: AgentKind::ClaudeCode,
            import_run_id: None,
            parser_revision: 1,
            source_model_raw: None,
            pricing_model: None,
            source_cost_usd: None,
            ts_precision_seconds: false,
        }],
    };
    let s = history.write_batch(&[rescan]).unwrap();
    assert_eq!(s.inserted, 0, "迁移过来的同一请求不被重扫重复插入");
    assert_eq!(s.unchanged, 1);
    assert_eq!(history.event_count().unwrap(), 1);
    assert_eq!(history.totals().unwrap(), tokens(10, 5, 1, 2));
    assert_eq!(
        history.origin_count().unwrap(),
        2,
        "两条来源证据指向同一事件"
    );
}
