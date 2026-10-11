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
        "tokenscope-history-migration-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
fn v2_backfill_writes_rfc3339_observed_at_utc() {
    // v1→v2 把"采集时刻"改写为事件时间时，utc 文本必须与运行期写入同格式
    //（RFC3339）——此前回填把纪元秒数值塞进 TEXT 列，与其它行格式不一致。
    let dir = temp_dir("utc-backfill");
    let db = dir.join("history.db");
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(tokenscope::history::legacy_v1_schema_for_tests())
            .unwrap();
        for (key, ts, nanos) in [
            ("k-sec", 1_784_307_600i64, 0i64),
            ("k-frac", 1_784_307_601, 500_000_000),
        ] {
            conn.execute(
                "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                    model_identity, identity_revision, session_id, record_id, project_key,
                    session_initial_cwd, event_cwd, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                    observed_at_seconds, observed_at_nanos, parser_revision)
                 VALUES(?1, 'codex', ?2, ?3, 'gpt-5.6', 'gpt56', 1, NULL, NULL, NULL,
                        NULL, NULL, '1', '2', '0', '0', 1, '1970-01-01T00:00:00Z', ?2, ?3, 1)",
                rusqlite::params![key, ts, nanos],
            )
            .unwrap();
        }
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let _history = HistoryDb::open(&db).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    let rows: Vec<(String, String, i64, i64)> = conn
        .prepare(
            "SELECT event_key, observed_at_utc, observed_at_seconds, observed_at_nanos
                  FROM usage_events ORDER BY event_key",
        )
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(
        rows,
        vec![
            (
                "k-frac".into(),
                "2026-07-17T17:00:01Z".into(),
                1_784_307_601,
                500_000_000
            ),
            (
                "k-sec".into(),
                "2026-07-17T17:00:00Z".into(),
                1_784_307_600,
                0
            ),
        ],
        "迁移回填的 utc 是 RFC3339；亚秒只留在 observed_at_nanos"
    );
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

// ================================================================ N01/N03

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, summary};

fn summary_opts(dir: &std::path::Path, claude: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: claude.clone(),
        codex_dir: None,
        claude_enabled: Some(claude.is_some()),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    }
}

/// N03：遗留缓存中的负数字段是坏数据——受检拒绝并计诊断，不用 `.max(0)`
/// 把坏值伪装成合法零值混进用量；同行合法记录照常迁移。
#[test]
fn negative_legacy_values_are_rejected_not_clamped() {
    let dir = temp_dir("negative");
    let legacy = dir.join("cache.db");
    seed_legacy_cache(
        &legacy,
        &[
            ("2026-07-17T08:00:00.000Z", "neg-1", -5, 20, 0, 0),
            ("2026-07-17T08:01:00.000Z", "ok-1", 10, 5, 0, 0),
            (
                "2026-07-17T08:02:00.000Z",
                "overflow",
                i64::MAX,
                i64::MAX,
                i64::MAX,
                0,
            ),
        ],
    );
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();
    let report = migrate_legacy_cache(&history, &legacy).unwrap().unwrap();
    assert_eq!(
        report.events_seen, 2,
        "负值行在读取阶段被拒绝，不计入可迁事件"
    );
    assert_eq!(report.rejected, 2, "读取期拒绝 + 溢出拒绝都计入诊断");
    assert_eq!(report.inserted, 1);
    assert_eq!(
        history.totals().unwrap(),
        tokens(10, 5, 0, 0),
        "负值不得伪装成 0，坏组合不得入库"
    );
}

/// N01：被旧实现污染的 v2 库（Codex 重复事件 + 被回退的 Claude 终值）在
/// 升级后得到安全纠错——v4 合并同保守身份的重复行、v3 改写观察时间、
/// 解析规则修订触发一次重解析让真实终值恢复；升级前备份可恢复，重复
/// 升级不产生新备份也不改变事实。
#[test]
fn polluted_v2_history_is_repaired_by_upgrade_and_rescan() {
    let dir = temp_dir("polluted-v2");
    let db = dir.join("data").join("history.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();

    // Claude 日志的当前内容 = 真实终值 21（旧实现曾把它回退成 6）。
    let claude = dir.join("claude");
    let log = claude.join("projects").join("p").join("s1.jsonl");
    std::fs::create_dir_all(log.parent().unwrap()).unwrap();
    std::fs::write(
        &log,
        format!(
            "{}\n{}\n",
            r#"{"type":"user","timestamp":"2026-09-15T08:00:00.000Z","sessionId":"s1c","cwd":"/work/app","message":{"role":"user"}}"#,
            r#"{"type":"assistant","timestamp":"2026-09-15T08:00:05.000Z","sessionId":"s1c","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":10,"output_tokens":21,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#
        ),
    )
    .unwrap();
    // 与 report 相同口径的指纹与键（canonicalize + 剥 \\?\ 前缀 + 小写）。
    let fp = format!(
        "{}:{}",
        std::fs::metadata(&log).unwrap().len(),
        tokenscope::cache::mtime_ms(&log).unwrap()
    );
    let norm = |p: &std::path::Path| -> String {
        let c = std::fs::canonicalize(p).unwrap();
        let s = c.to_string_lossy().to_string();
        let s = s
            .strip_prefix(r"\\?\UNC\")
            .map(|r| format!(r"\\{r}"))
            .unwrap_or_else(|| s.strip_prefix(r"\\?\").unwrap_or(&s).to_string());
        s.to_lowercase()
    };
    let path_key = norm(&log);
    let root_key = norm(&claude);

    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(tokenscope::history::legacy_v1_schema_for_tests())
            .unwrap();
        conn.execute_batch(tokenscope::history::legacy_v2_schema_for_tests())
            .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        // 污染 #1：同一 Codex 请求的两份来源各自成行（v2 路径来源键产物）。
        for (key, obs) in [
            ("native|codex|/logs/a.jsonl|0|fp1", 1_792_000_000i64),
            ("native|codex|/archive/a.jsonl|0|fp2", 1_792_000_100),
        ] {
            conn.execute(
                "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                    model_identity, identity_revision, session_id, record_id, project_key,
                    session_initial_cwd, event_cwd, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                    observed_at_seconds, observed_at_nanos, parser_revision)
                 VALUES(?1, 'codex', 1784307600, 0, 'gpt-x', 'gptx', 1, 's1', NULL, NULL,
                        NULL, NULL, '100', '20', '0', '0', 1, '2026-10-10T00:00:00Z',
                        ?2, 0, 1)",
                rusqlite::params![key, obs],
            )
            .unwrap();
            let id = conn.last_insert_rowid();
            conn.execute(
                "INSERT INTO event_origins(event_id, origin_kind, origin_key, app,
                    import_run_id, parser_revision, source_model_raw, pricing_model,
                    source_cost_usd, ts_precision_seconds)
                 VALUES(?1, 'native_file', ?2, 'codex', NULL, 1, NULL, NULL, NULL, 0)",
                rusqlite::params![id, key],
            )
            .unwrap();
        }
        // 污染 #2：被旧副本回退的 Claude 终值（output 6，观察时间 = 采集时刻）。
        conn.execute(
            "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                model_identity, identity_revision, session_id, record_id, project_key,
                session_initial_cwd, event_cwd, input_tokens, output_tokens,
                cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                observed_at_seconds, observed_at_nanos, parser_revision)
             VALUES('native|claude-code|/logs/old/c.jsonl|1|fpc', 'claude-code', 1784307700, 0,
                    'claude-opus-5-5', 'claudeopus55', 1, 's1c', 'm1', NULL, NULL, NULL,
                    '10', '6', '0', '0', 1, '2026-10-10T00:00:00Z', 1792000000, 0, 1)",
            [],
        )
        .unwrap();
        let cid = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO event_aliases(event_id, app, identity_scheme, identity_value,
                created_generation)
             VALUES(?1, 'claude-code', 'claude-message', 's1c|m1', 0)",
            [cid],
        )
        .unwrap();
        // 指纹命中当前文件、但 context_revision 是旧式（无规则前缀）的登记。
        conn.execute(
            "INSERT INTO source_files(app, root, path, fingerprint, size_bytes, mtime_ms,
                context_revision, state)
             VALUES('claude-code', ?1, ?2, ?3, 1, 1, '', 'present')",
            rusqlite::params![root_key, path_key, fp],
        )
        .unwrap();
    }

    let history = HistoryDb::open(&db).unwrap();
    assert_eq!(history.schema_version().unwrap(), 4, "升级到当前版本");
    let backup = db.with_extension("db.backup-v2");
    assert!(backup.is_file(), "升级前必须留可恢复备份");
    {
        let b = rusqlite::Connection::open(&backup).unwrap();
        let n: i64 = b
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 3, "备份（自 v2 升级）包含合并前的全部事实（可恢复）");
    }
    // 合并后：Codex 重复行归一，别名补齐，来源证据都挂在保留行上；
    // Claude 终值不被迁移编造（仍是 6），等待重解析恢复。
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(
        history.totals().unwrap(),
        tokens(110, 26, 0, 0),
        "Codex 100/20 只算一次；Claude 仍是污染值 6（不猜造终值）"
    );
    assert_eq!(
        history.origin_count().unwrap(),
        2,
        "两份 Codex 来源证据都保留在合并后的行上"
    );
    assert_eq!(
        history.alias_count().unwrap(),
        2,
        "保留行补齐 codex-usage 别名"
    );

    // 热扫（不带 refresh）：解析规则修订让登记失配 → 重解析 → 真实终值 21。
    summary(&summary_opts(&dir, Some(claude))).unwrap();
    assert_eq!(
        history.totals().unwrap(),
        tokens(110, 41, 0, 0),
        "热扫即恢复真实终值（20 + 21），无需手动清库或重扫"
    );
    assert!(
        history
            .source_files()
            .unwrap()
            .iter()
            .all(|f| f.context_revision.starts_with("2|")),
        "重解析后按新规则修订重新登记"
    );

    // 重复升级：事实稳定，不产生新备份。
    drop(history);
    let again = HistoryDb::open(&db).unwrap();
    assert_eq!(again.schema_version().unwrap(), 4);
    assert_eq!(again.totals().unwrap(), tokens(110, 41, 0, 0));
    assert!(
        !db.with_extension("db.backup-v4").exists(),
        "当前版本重复打开不触发迁移与备份"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// N03/H03：v3 时代旧缓存（files 缺 root / context_rev，events 缺
/// session_initial_cwd / event_cwd / line）也按"仅迁合法字段"迁移——列探测
/// 兜底空值，缺列不再让整库迁移失败（此前直接报错，违背可重试语义）。
#[test]
fn v3_legacy_cache_without_later_columns_still_migrates() {
    let dir = temp_dir("v3-legacy");
    let legacy = dir.join("cache.db");
    {
        let conn = rusqlite::Connection::open(&legacy).unwrap();
        conn.execute_batch(include_str!("fixtures/cache/schema-v3.sql"))
            .unwrap();
        // v3 events.files_id 外键指向 files：先插文件行再插事件行。
        conn.execute(
            "INSERT INTO files(id, path, agent, size, mtime_ms) VALUES(2, '/logs/new.jsonl', 'claude-code', 10, 5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events(file_id, ts, record_id, model, session_id, project,
                                input, output, cache_write, cache_read)
             VALUES(2, '2026-07-17T08:00:00Z', 'm9', 'claude-sonnet-4-5', 's9', '/work/old',
                    10, 5, 0, 0)",
            [],
        )
        .unwrap();
    }
    let history = HistoryDb::open(&dir.join("history.db")).unwrap();
    let report = migrate_legacy_cache(&history, &legacy).unwrap().unwrap();
    assert_eq!(report.events_seen, 2, "v3 自带一行 + 本测试补一行");
    assert_eq!(report.inserted, 2, "缺列旧结构照常迁移");
    assert_eq!(history.totals().unwrap(), tokens(20, 6, 0, 0));
    // 迁移行的项目/目录：v3 没有这些列 → 兜底空值，不得猜测。
    std::fs::remove_dir_all(&dir).ok();
}
