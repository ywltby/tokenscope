//! 审阅回归：13 项缺陷的定向测试（合成数据 + 临时目录）。
//!
//! 覆盖清单（编号 = 审阅意见）：
//! 2  流式读取不得因"锁内回调 + 分批取本体"自锁死锁；
//! 3a Codex 重叠候选：身份不足 + 同时间同用量 → 预览未解决、默认拒绝提交；
//! 3b Claude 会话行被去重时，原生别名必须留给 proxy 行（否则原生采集重复计费）；
//! 4  Codex 文件归档移动 / 后到副本 / 遗留缓存迁移后重扫都不重复累计；
//! 5  Claude 旧副本不得覆盖较新的用量终值；
//! 6  日汇总完整主键（含 request_model）不漏列、不折叠；
//! 7  日期筛选同样作用于日汇总；
//! 8  明细分页不得绕过会话预算（不再整份物化事件）；
//! 9  单条坏记录不得让整批采集回滚（已有历史仍可展示）；
//! 10 遗留迁移整批原子（失败保持事务前状态）；
//! 11 "净新增 token"只统计本批真正新增的事件；
//! 13 采集统一单飞：不同参数的采集也必须串行；
//! 以及计划要求：仅明细查询、升级前备份、事务内 generation 校验。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{
    DailyRollupWrite, EventWrite, HistoryDb, RollupConflictPolicy, WritePrecedence, WriteSummary,
};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-review-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts(dir: &Path, claude: Option<PathBuf>, codex: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: claude.clone(),
        codex_dir: codex.clone(),
        claude_enabled: Some(claude.is_some()),
        codex_enabled: Some(codex.is_some()),
        // 测试密闭性：写入路径必须落在临时目录。
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    }
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n")).unwrap();
}

// ---------------------------------------------------------------- Claude fixture

fn claude_assistant(
    session: &str,
    id: &str,
    ts: &str,
    model: &str,
    input: u64,
    output: u64,
) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "message": {
            "id": id,
            "model": model,
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        }
    })
    .to_string()
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

// ---------------------------------------------------------------- Codex fixture

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

fn codex_token_count(ts: &str, input: u64, output: u64) -> String {
    serde_json::json!({
        "timestamp": ts,
        "type": "event_msg",
        "payload": {
            "type": "token_count",
            "info": {
                "last_token_usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cached_input_tokens": 0,
                    "cache_write_input_tokens": 0,
                    "reasoning_output_tokens": 0,
                    "total_tokens": input + output,
                }
            }
        }
    })
    .to_string()
}

/// 一条 Codex rollout（根目录下的相对路径）。
fn codex_rollout(dir: &Path, rel: &str, lines: &[String]) -> PathBuf {
    let path = dir.join(rel);
    write_lines(&path, lines);
    path
}

// ---------------------------------------------------------------- CCS fixture

const CCS_SCHEMA: &str = r"
CREATE TABLE proxy_request_logs (
    request_id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    app_type TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    status_code INTEGER NOT NULL DEFAULT 200,
    session_id TEXT,
    created_at INTEGER NOT NULL,
    data_source TEXT NOT NULL DEFAULT 'proxy',
    pricing_model TEXT,
    input_token_semantics INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE usage_daily_rollups (
    date TEXT NOT NULL,
    app_type TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '',
    pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0',
    input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
";

#[derive(Clone)]
struct CcsRow {
    request_id: String,
    app_type: String,
    model: String,
    session_id: Option<String>,
    data_source: String,
    created_at: i64,
    input: i64,
    output: i64,
}

/// (date, app, model, request_model, count, input, output)
type RollupFixture<'a> = (&'a str, &'a str, &'a str, &'a str, i64, i64, i64);

fn seed_ccs(path: &Path, rows: &[CcsRow], rollups: &[RollupFixture<'_>]) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(CCS_SCHEMA).unwrap();
    conn.pragma_update(None, "user_version", 20).unwrap();
    for r in rows {
        conn.execute(
            "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd, status_code, session_id, created_at, data_source, pricing_model,
                input_token_semantics)
             VALUES(?1, 'p1', ?2, ?3, ?3, ?4, ?5, 0, 0, '0.25', 200, ?6, ?7, ?8, ?3, 0)",
            rusqlite::params![
                r.request_id,
                r.app_type,
                r.model,
                r.input,
                r.output,
                r.session_id,
                r.created_at,
                r.data_source,
            ],
        )
        .unwrap();
    }
    for (date, app, model, request_model, count, input, output) in rollups {
        conn.execute(
            "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
                request_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, input_token_semantics)
             VALUES(?1, ?2, 'p1', ?3, ?4, ?5, ?6, ?7, 0, 0, '1.5', 0)",
            rusqlite::params![date, app, model, request_model, count, input, output],
        )
        .unwrap();
    }
}

fn history_of(dir: &Path) -> HistoryDb {
    HistoryDb::open(&dir.join("data").join("history.db")).unwrap()
}

fn synthetic_event(i: u64) -> EventWrite {
    let ts = jiff::Timestamp::new(1_789_545_600 + i as i64, 0).unwrap();
    EventWrite {
        event_key: format!("synthetic|{i}"),
        app: AgentKind::Codex,
        ts,
        model_raw: "gpt-5.6-sol".into(),
        model_identity: "gpt56sol".into(),
        session_id: Some("s".into()),
        record_id: Some(format!("r{i}")),
        project_key: Some("/work/app".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            input: 100,
            output: 10,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases: Vec::new(),
        origins: Vec::new(),
    }
}

// ================================================================ 2：死锁

/// 审阅 #2：明细超过分批阈值（旧实现 4096 条）且同时存在日汇总时，汇总与
/// 分页必须在限定时间内返回——旧实现持锁遍历、回调里再取同一把锁会挂死。
#[test]
fn large_detail_plus_rollup_does_not_deadlock() {
    let dir = temp_dir("deadlock");
    let history = history_of(&dir);
    let rollups: Vec<DailyRollupWrite> = (0..3)
        .map(|i| DailyRollupWrite {
            logical_source: "ccs".into(),
            day: format!("2026-06-0{}", i + 1),
            source_tz: "Asia/Shanghai".into(),
            app: AgentKind::Codex,
            provider_id: "_codex_session".into(),
            model: "gpt-5.6-sol".into(),
            request_model: String::new(),
            pricing_model: String::new(),
            request_count: 2,
            tokens: TokenCounts {
                input: 10,
                output: 1,
                cache_write: 0,
                cache_read: 0,
            },
            input_semantics: 2,
            source_cost_usd: None,
            revision: 0,
        })
        .collect();
    history
        .apply_import_batch(
            &[],
            &rollups,
            RollupConflictPolicy::KeepExisting,
            &tokenscope::history::ImportRunRecord {
                logical_source: "ccs".into(),
                source_path: "synthetic".into(),
                source_schema: "test".into(),
                started_utc: "2026-10-10T00:00:00Z".into(),
                status: "committed".into(),
                detail: None,
                import_revision: 1,
            },
            None,
        )
        .unwrap();
    // 5000 条事件（> 旧的 4096 分批阈值）。
    let batch: Vec<EventWrite> = (0..5000u64).map(synthetic_event).collect();
    history.write_batch(&batch).unwrap();

    let (tx, rx) = std::sync::mpsc::channel::<(u64, u64)>();
    let dir_for_thread = dir.clone();
    std::thread::spawn(move || {
        let o = opts(&dir_for_thread, None, None);
        let snap = query::begin_query(&o).unwrap();
        let s = query::query_summary(&snap.query_id).unwrap();
        let page = query::query_events(
            &snap.query_id,
            &EventFilter {
                limit: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
        let _ = tx.send((s.totals.requests, page.total));
    });
    match rx.recv_timeout(Duration::from_secs(30)) {
        Ok((requests, total)) => {
            assert_eq!(requests, 5000, "汇总计入全部事件");
            assert_eq!(total, 5000, "分页 total 覆盖全部行");
        }
        Err(e) => panic!("查询在 30 秒内未返回（疑似持锁回调自锁）：{e}"),
    }
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 3a：重叠候选

/// 审阅 #3a：Codex 没有可证实身份——CCS 行与本地已有请求"同时间同用量"时，
/// 必须列为未解决重叠候选：预览可见、默认拒绝提交（不静默重复计费），
/// 用户显式确认后才按新增导入并留下审计计数。
/// 审阅 #3a：**身份不足**（无 session_id，构造不出任何别名）的 Codex 来源行，
/// 与本地已有请求同时间同用量 → 预览列为重叠候选（未解决）、默认拒绝提交；
/// 显式确认后按新增导入并留审计。带 session_id 的行登记保守重播身份别名，
/// 两侧直接命中同一事件（见 codex_import_and_collect_yield_the_same_union）。
#[test]
fn codex_overlap_candidate_requires_explicit_confirmation() {
    let dir = temp_dir("overlap-codex");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    history
        .write_batch(&[EventWrite {
            event_key: "native|codex|/logs/rollout.jsonl|3|fp".into(),
            app: AgentKind::Codex,
            ts,
            model_raw: "gpt-5.6-sol".into(),
            model_identity: "gpt56sol".into(),
            session_id: Some("sess-1".into()),
            record_id: None,
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 100,
                output: 30,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: ts,
            aliases: Vec::new(),
            origins: Vec::new(),
        }])
        .unwrap();

    seed_ccs(
        &dir.join("cc-switch.db"),
        &[CcsRow {
            request_id: "ccs-1".into(),
            app_type: "codex".into(),
            model: "gpt-5.6-sol".into(),
            session_id: None,
            data_source: "codex_session".into(),
            created_at: ts.as_second(),
            input: 100,
            output: 30,
        }],
        &[],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.would_overlap, 1, "同时间同用量必须列为重叠候选");
    assert!(preview.overlap_unresolved);
    assert_eq!(preview.would_insert, 0, "重叠候选不计入可新增");
    assert!(!preview.overlap_examples.is_empty());

    let err = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("重叠候选"), "{err}");
    assert_eq!(history.event_count().unwrap(), 1, "拒绝提交时零写入");
    assert_eq!(history.totals().unwrap().input, 100, "拒绝提交不改用量");

    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 1);
    assert_eq!(report.requests_overlap, 1, "审计必须标出重叠候选");
    assert_eq!(history.event_count().unwrap(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 3b：别名继承

/// 审阅 #3b：Claude 会话行因"与 proxy 行重复"被剔除时，它携带的原生别名必须
/// 交给被保留的 proxy 行——否则原生采集之后会为同一请求再建一条事件。
#[test]
fn deduped_session_row_donates_native_alias_to_proxy_row() {
    let dir = temp_dir("alias-donate");
    let history = history_of(&dir);
    let claude = dir.join("claude");
    let ts = 1_790_000_000i64;
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            CcsRow {
                request_id: "proxy-1".into(),
                app_type: "claude".into(),
                model: "claude-opus-5-5".into(),
                session_id: Some("sess-A".into()),
                data_source: "proxy".into(),
                created_at: ts,
                input: 100,
                output: 30,
            },
            CcsRow {
                request_id: "session:msg-77".into(),
                app_type: "claude".into(),
                model: "claude-opus-5-5".into(),
                session_id: Some("sess-A".into()),
                data_source: "session_log".into(),
                created_at: ts,
                input: 100,
                output: 30,
            },
        ],
        &[],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.requests_skipped_duplicate_of_proxy, 1);
    assert_eq!(preview.requests_importable, 1);
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "同一次请求只导一条");
    assert_eq!(history.alias_count().unwrap(), 1, "会话行的别名必须被继承");

    // 原生采集同一 (session, message) 请求：命中别名 → 不新增、不翻倍。
    write_lines(
        &claude.join("projects/p/sess-A.jsonl"),
        &[
            claude_context("sess-A", "2026-09-15T08:00:00Z", "/work/app"),
            claude_assistant(
                "sess-A",
                "msg-77",
                "2026-09-15T08:00:01Z",
                "claude-opus-5-5",
                100,
                30,
            ),
        ],
    );
    summary(&opts(&dir, Some(claude), None)).unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        1,
        "原生采集同一请求必须绑定到已有事件（不重复计费）"
    );
    assert_eq!(history.totals().unwrap().input, 100);
    assert_eq!(history.totals().unwrap().output, 30);
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 4：重复累计

/// 审阅 #4：Codex 文件被归档移动、出现后到副本，都不得重复累计
///（持久化身份不能含文件路径）。
#[test]
fn codex_moved_and_copied_rollouts_do_not_double_count() {
    let dir = temp_dir("codex-move");
    let codex = dir.join("codex");
    let lines = vec![
        codex_meta("sess-1", "/work/app"),
        codex_context("gpt-5.6-sol", "/work/app"),
        codex_token_count("2026-07-17T15:02:00.000Z", 100, 20),
    ];
    codex_rollout(&codex, "sessions/2026/07/17/rollout-a.jsonl", &lines);
    let o = opts(&dir, None, Some(codex.clone()));
    summary(&o).unwrap();
    let history = history_of(&dir);
    assert_eq!(history.event_count().unwrap(), 1);
    assert_eq!(history.totals().unwrap().input, 100);

    std::fs::create_dir_all(codex.join("archived_sessions/2026/07/17")).unwrap();
    std::fs::rename(
        codex.join("sessions/2026/07/17/rollout-a.jsonl"),
        codex.join("archived_sessions/2026/07/17/rollout-a.jsonl"),
    )
    .unwrap();
    summary(&o).unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "归档移动不新增事件");
    assert_eq!(history.totals().unwrap().input, 100, "token 不翻倍");

    codex_rollout(&codex, "sessions/2026/07/17/rollout-copy.jsonl", &lines);
    summary(&o).unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "重复副本不新增事件");
    assert_eq!(history.totals().unwrap().output, 20, "token 不翻倍");
    std::fs::remove_dir_all(&dir).ok();
}

/// 审阅 #4（迁移分支）：旧缓存迁移后重扫同一份日志，命中迁移登记的身份。
#[test]
fn migrated_codex_usage_is_not_recounted_on_rescan() {
    let dir = temp_dir("codex-migrate");
    let codex = dir.join("codex");
    let lines = vec![
        codex_meta("sess-9", "/work/app"),
        codex_context("gpt-5.6-sol", "/work/app"),
        codex_token_count("2026-07-17T15:02:00.000Z", 120, 30),
    ];
    codex_rollout(&codex, "sessions/2026/07/17/rollout-m.jsonl", &lines);

    // 旧缓存（v10 真实结构：meta / files / events，无 line 列）。
    let cache_dir = dir.join("data");
    std::fs::create_dir_all(&cache_dir).unwrap();
    let legacy = cache_dir.join("cache.db");
    {
        let conn = rusqlite::Connection::open(&legacy).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE files (
                id INTEGER PRIMARY KEY, path TEXT NOT NULL, root TEXT NOT NULL DEFAULT '',
                agent TEXT NOT NULL, size INTEGER NOT NULL, mtime_ms INTEGER NOT NULL,
                context_rev TEXT NOT NULL DEFAULT '', lines_seen INTEGER NOT NULL DEFAULT 0,
                bad_lines INTEGER NOT NULL DEFAULT 0,
                skipped_sidechain INTEGER NOT NULL DEFAULT 0,
                skipped_synthetic INTEGER NOT NULL DEFAULT 0,
                skipped_zero_usage INTEGER NOT NULL DEFAULT 0,
                skipped_no_model INTEGER NOT NULL DEFAULT 0,
                ignored_token_usage_record INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE events (
                file_id INTEGER NOT NULL, ts TEXT NOT NULL,
                record_id TEXT NOT NULL DEFAULT '', model TEXT NOT NULL,
                session_id TEXT NOT NULL, project TEXT NOT NULL,
                session_initial_cwd TEXT, event_cwd TEXT, input INTEGER NOT NULL,
                output INTEGER NOT NULL, cache_write INTEGER NOT NULL,
                cache_read INTEGER NOT NULL
             );",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('schema_version', '10')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO files(id, path, root, agent, size, mtime_ms)
             VALUES(1, '/logs/rollout-m.jsonl', '/logs', 'codex', 100, 200)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events(file_id, ts, record_id, model, session_id, project,
                session_initial_cwd, event_cwd, input, output, cache_write, cache_read)
             VALUES(1, '2026-07-17T15:02:00Z', '', 'gpt-5.6-sol', 'sess-9', '/work/app',
                    '/work/app', NULL, 120, 30, 0, 0)",
            [],
        )
        .unwrap();
    }
    let history = history_of(&dir);
    let report = tokenscope::history::migrate_legacy_cache(&history, &legacy)
        .unwrap()
        .unwrap();
    assert_eq!(report.inserted, 1);
    assert_eq!(history.totals().unwrap().input, 120);
    assert_eq!(
        history.alias_count().unwrap(),
        1,
        "迁移必须登记保守重播身份（否则重扫会重复累计）"
    );

    summary(&opts(&dir, None, Some(codex))).unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "迁移后重扫不新增事件");
    assert_eq!(history.totals().unwrap().input, 120, "token 不翻倍");
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 5：终值不回退

/// 审阅 #5：重新解析一份较旧的副本不得把已保存的较新终值改回小值；
/// 新增请求照常累计。
#[test]
fn stale_claude_copy_cannot_lower_saved_terminal_value() {
    let dir = temp_dir("terminal-value");
    let claude = dir.join("claude");
    write_lines(
        &claude.join("projects/p/s1.jsonl"),
        &[
            claude_context("s1", "2026-09-15T08:00:00Z", "/work/app"),
            claude_assistant(
                "s1",
                "m1",
                "2026-09-15T08:00:05Z",
                "claude-opus-5-5",
                100,
                20,
            ),
        ],
    );
    let o = opts(&dir, Some(claude.clone()), None);
    summary(&o).unwrap();
    let history = history_of(&dir);
    assert_eq!(history.totals().unwrap().output, 20);

    // 旧副本：同请求、更早时间戳、更小的输出值（模拟流式中间态）。
    write_lines(
        &claude.join("projects/p/s1-old-copy.jsonl"),
        &[
            claude_context("s1", "2026-09-15T08:00:00Z", "/work/app"),
            claude_assistant(
                "s1",
                "m1",
                "2026-09-15T08:00:01Z",
                "claude-opus-5-5",
                100,
                5,
            ),
        ],
    );
    summary(&o).unwrap();
    assert_eq!(
        history.totals().unwrap().output,
        20,
        "较旧的副本不得回退已保存的终值"
    );

    write_lines(
        &claude.join("projects/p/s1-new.jsonl"),
        &[
            claude_context("s1", "2026-09-15T09:00:00Z", "/work/app"),
            claude_assistant("s1", "m2", "2026-09-15T09:00:01Z", "claude-opus-5-5", 10, 1),
        ],
    );
    summary(&o).unwrap();
    assert_eq!(history.totals().unwrap().output, 21, "期望 20 + 1");
    assert_eq!(history.event_count().unwrap(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 6：日汇总主键

/// 审阅 #6：日汇总的完整主键含 `request_model`——两条仅该列不同的合法汇总
/// 必须各自保留，不折叠、不丢用量。
#[test]
fn rollups_with_distinct_request_model_are_not_collapsed() {
    let dir = temp_dir("rollup-pk");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[
            (
                "2026-06-01",
                "claude",
                "mimo-v2.5-pro",
                "client-a",
                1,
                100,
                10,
            ),
            (
                "2026-06-01",
                "claude",
                "mimo-v2.5-pro",
                "client-b",
                2,
                200,
                20,
            ),
        ],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.rollups_total, 2);
    assert_eq!(preview.rollups_new, 2);
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    assert_eq!(history.rollup_count().unwrap(), 2, "两条合法汇总都要保留");
    assert_eq!(
        history.rollup_request_count().unwrap(),
        3,
        "请求数合计必须等于来源的 3 次"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 7：日期过滤

/// 审阅 #7：查询限定日期时，只有日粒度的历史同样受该范围约束。
#[test]
fn date_filter_excludes_rollups_outside_range() {
    let dir = temp_dir("rollup-date");
    let history = history_of(&dir);
    let claude = dir.join("claude");
    write_lines(
        &claude.join("projects/p/s1.jsonl"),
        &[
            claude_context("s1", "2026-10-09T08:00:00Z", "/work/app"),
            claude_assistant(
                "s1",
                "m1",
                "2026-10-09T08:00:01Z",
                "claude-opus-5-5",
                100,
                10,
            ),
        ],
    );
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[("2026-06-01", "claude", "mimo-v2.5-pro", "", 10, 1000, 100)],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();

    let mut o = opts(&dir, Some(claude), None);
    o.tz = Some("Asia/Shanghai".to_string());
    o.from = Some("2026-10-09".into());
    o.to = Some("2026-10-09".into());
    let report = summary(&o).unwrap();
    assert_eq!(
        report.totals.requests, 1,
        "范围外的日汇总（6 月 1 日 10 次）不得混入"
    );
    assert_eq!(report.totals.tokens.input, 100);
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 8：分页预算

/// 审阅 #8：`limit=1` 的明细分页不得物化整份历史（旧实现让保留量远超预算）。
#[test]
fn paged_events_respect_session_budget_without_full_materialization() {
    let dir = temp_dir("paging-budget");
    let history = history_of(&dir);
    let batch: Vec<EventWrite> = (0..4000u64).map(synthetic_event).collect();
    history.write_batch(&batch).unwrap();

    let o = opts(&dir, None, None);
    let snap = query::begin_query(&o).unwrap();
    let before = query::query_retained_bytes_for_tests();
    let page = query::query_events(
        &snap.query_id,
        &EventFilter {
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    let after = query::query_retained_bytes_for_tests();
    assert_eq!(page.total, 4000);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(after, before, "分页不得改变会话保留量（不物化整份事件）");
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 9：坏记录

/// 审阅 #9：一条模型名为空的坏行不得让整批采集回滚——其余用量照常入账，
/// 且已有历史仍可查询/展示。
#[test]
fn one_bad_event_does_not_roll_back_the_whole_batch() {
    let dir = temp_dir("bad-event");
    let claude = dir.join("claude");
    let bad_line = r#"{"type":"assistant","timestamp":"2026-10-09T08:00:01Z","sessionId":"s1","message":{"id":"bad","model":"","usage":{"input_tokens":5,"output_tokens":5}}}"#.to_string();
    write_lines(
        &claude.join("projects/p/s1.jsonl"),
        &[
            claude_context("s1", "2026-10-09T08:00:00Z", "/work/app"),
            bad_line.clone(),
            claude_assistant(
                "s1",
                "m1",
                "2026-10-09T08:00:02Z",
                "claude-opus-5-5",
                100,
                10,
            ),
        ],
    );
    let o = opts(&dir, Some(claude.clone()), None);
    let report = summary(&o).unwrap();
    let history = history_of(&dir);
    assert_eq!(history.event_count().unwrap(), 1, "正常行照常入库");
    assert_eq!(history.totals().unwrap().input, 100);
    let bad: u64 = report
        .sources
        .iter()
        .map(|s| s.stats.bad_lines)
        .sum::<u64>();
    assert!(bad >= 1, "坏行必须计数（不静默吞掉）：{bad}");

    write_lines(
        &claude.join("projects/p/s2.jsonl"),
        &[
            claude_context("s2", "2026-10-09T09:00:00Z", "/work/app"),
            bad_line,
        ],
    );
    let report2 = summary(&o).unwrap();
    assert_eq!(report2.totals.requests, 1, "坏行不影响已有用量展示");
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 10：迁移原子

/// 审阅 #10：单事务写入（数据 + 一次性标记）必须整批原子——写入失败时库内
/// 保持事务前状态，不留"前 N 条已提交"的半迁移。
#[test]
fn write_batch_with_marker_is_atomic_on_failure() {
    let dir = temp_dir("atomic-migration");
    let history = history_of(&dir);
    let mut batch: Vec<EventWrite> = (0..500u64).map(synthetic_event).collect();
    let mut bad = synthetic_event(500);
    bad.model_raw = String::new();
    batch.push(bad);

    let err = history.write_batch_with_marker(&batch, Some("legacy_cache_migrated"));
    assert!(err.is_err(), "坏记录必须让整批失败");
    assert_eq!(
        history.event_count().unwrap(),
        0,
        "失败时零写入（事务前状态）"
    );
    assert_eq!(
        history.meta("legacy_cache_migrated").unwrap(),
        None,
        "标记不得单独落库"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 11：净新增

/// 审阅 #11：只给已有请求补来源证据时，"净新增 token"必须是 0（不是该请求的
/// 全部 token）。
#[test]
fn net_new_tokens_counts_only_newly_inserted_events() {
    let dir = temp_dir("net-new");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let write = EventWrite {
        event_key: "ccs|claude|proxy|proxy-1".into(),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("sess-A".into()),
        record_id: Some("msg-1".into()),
        project_key: None,
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            input: 100,
            output: 30,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases: vec![tokenscope::history::EventAlias {
            app: AgentKind::ClaudeCode,
            scheme: "claude-message".into(),
            value: "sess-A|msg-1".into(),
        }],
        origins: Vec::new(),
    };
    history.write_batch(std::slice::from_ref(&write)).unwrap();
    assert_eq!(history.totals().unwrap().input, 100);

    // 再次导入同一请求：只新增来源证据（例如同一请求换了来源记录键）。
    let mut again = write.clone();
    again.event_key = "ccs|claude|proxy|proxy-2".into();
    again.origins = vec![tokenscope::history::EventOrigin {
        origin_kind: "ccs".into(),
        origin_key: "ccs|claude|proxy|proxy-2".into(),
        app: AgentKind::ClaudeCode,
        import_run_id: None,
        parser_revision: 1,
        source_model_raw: None,
        pricing_model: None,
        source_cost_usd: None,
        ts_precision_seconds: true,
    }];
    let summary: WriteSummary = history.write_batch(&[again]).unwrap();
    assert_eq!(summary.inserted, 0);
    assert_eq!(summary.origins_added, 1, "只补来源证据");
    assert_eq!(
        summary.inserted_tokens,
        TokenCounts::default(),
        "净新增必须为 0"
    );
    assert_eq!(history.totals().unwrap().input, 100, "用量不翻倍");
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 13：统一单飞

/// 审阅 #13：不同参数（不同 agent 筛选）的采集也必须串行——同一时刻只能有
/// 一条采集管线（启动采集/手动刷新/定时采集走同一个协调器）。
#[test]
fn different_query_params_do_not_run_collections_in_parallel() {
    let dir = temp_dir("single-flight");
    let claude = dir.join("claude");
    let codex = dir.join("codex");
    write_lines(
        &claude.join("projects/p/s1.jsonl"),
        &[
            claude_context("s1", "2026-10-09T08:00:00Z", "/work/app"),
            claude_assistant(
                "s1",
                "m1",
                "2026-10-09T08:00:01Z",
                "claude-opus-5-5",
                100,
                10,
            ),
        ],
    );
    codex_rollout(
        &codex,
        "sessions/2026/07/17/rollout-a.jsonl",
        &[
            codex_meta("sess-1", "/work/app"),
            codex_context("gpt-5.6-sol", "/work/app"),
            codex_token_count("2026-07-17T15:02:00.000Z", 50, 5),
        ],
    );

    let _ = tokenscope::report::take_max_concurrent_collections_for_tests();
    let mut handles = Vec::new();
    for agent in ["claude-code", "codex", "all", "all"] {
        let mut o = opts(&dir, Some(claude.clone()), Some(codex.clone()));
        o.agent = match agent {
            "claude-code" => Some(AgentKind::ClaudeCode),
            "codex" => Some(AgentKind::Codex),
            _ => None,
        };
        handles.push(std::thread::spawn(move || {
            summary(&o).map(|r| r.totals.requests)
        }));
    }
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &results {
        assert!(r.is_ok(), "并发查询必须都成功：{r:?}");
    }
    let max_parallel = tokenscope::report::take_max_concurrent_collections_for_tests();
    assert!(
        max_parallel <= 1,
        "不同参数的采集也必须串行（观测到 {max_parallel} 条并行管线）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 仅明细查询

/// 计划 §5.2 的恢复路径：`detail_only` 时只有日粒度的历史不参与统计。
#[test]
fn detail_only_mode_excludes_rollup_history() {
    let dir = temp_dir("detail-only");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[("2026-06-01", "claude", "mimo-v2.5-pro", "", 10, 1000, 100)],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();

    let mut o = opts(&dir, None, None);
    // 来源时区与展示时区一致时，日汇总才参与按日统计。
    o.tz = Some("Asia/Shanghai".to_string());
    let auto = summary(&o).unwrap();
    assert_eq!(auto.totals.requests, 10, "自动模式计入只有日粒度的历史");

    o.rollups = tokenscope::report::RollupsMode::DetailOnly;
    let detail_only = summary(&o).unwrap();
    assert_eq!(detail_only.totals.requests, 0, "仅明细模式排除日粒度历史");
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 1：IPC 字段

/// 审阅 #1：IPC 载荷字段必须是**蛇形**（与 `frontend/src/types.ts` 逐字段
/// 对应）——早期给这些结构加了 camelCase 改名，前端读到 `undefined`：
/// 预览计数渲染报错、`plan_id` 拿不到、仪表盘的时区提示整段消失。
#[test]
fn ipc_payload_field_names_are_snake_case() {
    let dir = temp_dir("ipc-fields");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[CcsRow {
            request_id: "ccs-1".into(),
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            session_id: Some("sess-A".into()),
            data_source: "proxy".into(),
            created_at: 1_790_000_000,
            input: 100,
            output: 30,
        }],
        &[("2026-06-01", "claude", "mimo-v2.5-pro", "", 3, 300, 30)],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    let json = serde_json::to_value(&preview).unwrap();
    for key in [
        "plan_id",
        "requests_total",
        "requests_importable",
        "requests_skipped_duplicate_of_proxy",
        "would_insert",
        "would_overlap",
        "overlap_unresolved",
        "overlap_examples",
        "net_new_tokens",
        "rollups_total",
        "records_without_project",
        "history_generation",
        "source_day_timezone",
    ] {
        assert!(
            json.get(key).is_some(),
            "预览载荷缺少蛇形字段 {key}（前端按此名读取）"
        );
    }
    assert!(
        json.get("planId").is_none(),
        "不得出现 camelCase 字段（前端读不到）"
    );

    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    let json = serde_json::to_value(&report).unwrap();
    for key in [
        "run_id",
        "requests_inserted",
        "requests_overlap",
        "rollups_snapshotted",
        "net_new_tokens",
        "generation_after",
    ] {
        assert!(json.get(key).is_some(), "导入结果缺少蛇形字段 {key}");
    }

    // 日汇总覆盖诊断（仪表盘时区提示）同样是蛇形。
    let coverage = tokenscope::aggregate::RollupCoverage {
        detail_buckets: 1,
        rollup_buckets: 0,
        unresolved_buckets: 0,
        timezone_mismatch: Some("来源时区 Asia/Shanghai 与展示时区不一致".into()),
        ..Default::default()
    };
    let json = serde_json::to_value(&coverage).unwrap();
    assert!(json.get("timezone_mismatch").is_some());
    assert!(json.get("detail_buckets").is_some());
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 备份与竞态

/// 计划 §6：结构升级前必须在数据目录留下可恢复备份。
#[test]
fn schema_upgrade_creates_restorable_backup() {
    let dir = temp_dir("upgrade-backup");
    let path = dir.join("history.db");
    {
        let history = HistoryDb::open(&path).unwrap();
        assert_eq!(history.schema_version().unwrap(), 3, "新库直接是当前版本");
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let history = HistoryDb::open(&path).unwrap();
    assert_eq!(history.schema_version().unwrap(), 3, "升级到当前版本");
    let backup = dir.join("history.db.backup-v1");
    assert!(backup.is_file(), "必须生成升级前备份：{}", backup.display());
    let conn = rusqlite::Connection::open(&backup).unwrap();
    let tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(tables > 0, "备份必须是可恢复的完整 SQLite 文件");
    std::fs::remove_dir_all(&dir).ok();
}

/// 计划不变量 12：提交必须在**事务内**校验 generation——预览之后、提交之前
/// 有新写入时整批拒绝（不拿过期预览插入重复请求）。
#[test]
fn import_commit_rechecks_generation_inside_transaction() {
    let dir = temp_dir("generation-race");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[CcsRow {
            request_id: "ccs-1".into(),
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            session_id: Some("sess-A".into()),
            data_source: "proxy".into(),
            created_at: 1_790_000_000,
            input: 100,
            output: 30,
        }],
        &[],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    // 预览之后发生一次写入（模拟并发采集）。
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    history
        .write_batch(&[EventWrite {
            event_key: "native|race".into(),
            app: AgentKind::Codex,
            ts,
            model_raw: "gpt-5.6-sol".into(),
            model_identity: "gpt56sol".into(),
            session_id: Some("s".into()),
            record_id: None,
            project_key: None,
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 1,
                output: 1,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: ts,
            aliases: Vec::new(),
            origins: Vec::new(),
        }])
        .unwrap();
    let err = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("重新预览"), "{err}");
    assert_eq!(history.event_count().unwrap(), 1, "拒绝时零写入");
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 复审第二轮

use tokenscope::history::EventAlias;

/// 复审 #3：先导入 CCS、后采集 Codex 必须与反向顺序得到**相同并集**。
/// CCS 侧现在登记与原生采集同构的保守重播身份（`codex-usage`），两侧命中
/// 同一事件；缺 session_id 的行仍走重叠候选（见上一测试）。
#[test]
fn codex_import_and_collect_yield_the_same_union() {
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let ccs_row = CcsRow {
        request_id: "ccs-1".into(),
        app_type: "codex".into(),
        model: "gpt-5.6-sol".into(),
        session_id: Some("sess-1".into()),
        data_source: "codex_session".into(),
        created_at: ts.as_second(),
        input: 100,
        output: 30,
    };
    let lines = vec![
        codex_meta("sess-1", "/work/app"),
        codex_context("gpt-5.6-sol", "/work/app"),
        codex_token_count("2026-07-17T15:00:00.000Z", 100, 30),
    ];

    // 顺序 A：先导入，后采集。
    let dir = temp_dir("union-import-first");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        std::slice::from_ref(&ccs_row),
        &[],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(preview.would_overlap, 0, "重播身份命中，不再是重叠候选");
    assert_eq!(preview.would_insert, 1);
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 1);
    let codex = dir.join("codex");
    codex_rollout(&codex, "sessions/2026/07/17/rollout-u.jsonl", &lines);
    summary(&opts(&dir, None, Some(codex))).unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        1,
        "采集并入已导入事件，不新增"
    );
    assert_eq!(history.totals().unwrap().input, 100, "token 不翻倍");
    assert_eq!(history.totals().unwrap().output, 30);
    std::fs::remove_dir_all(&dir).ok();

    // 顺序 B：先采集，后导入。
    let dir = temp_dir("union-collect-first");
    let history = history_of(&dir);
    let codex = dir.join("codex");
    codex_rollout(&codex, "sessions/2026/07/17/rollout-u.jsonl", &lines);
    summary(&opts(&dir, None, Some(codex))).unwrap();
    assert_eq!(history.event_count().unwrap(), 1);
    seed_ccs(&dir.join("cc-switch.db"), &[ccs_row], &[]);
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(preview.would_overlap, 0, "两个顺序都必须免提示命中同一事件");
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        1,
        "导入并入已采集事件，不新增"
    );
    assert_eq!(history.totals().unwrap().input, 100, "token 不翻倍");
    assert_eq!(report.requests_unchanged + report.requests_conflicted, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// 复审 #3 的会话行分支：Codex 会话行因"与 proxy 行重复"被剔除时，它的
/// 保守重播身份必须转移给被保留的 proxy 行——否则先导入（留 proxy 行）后
/// 采集（带 session_id）仍会为同一请求再建一条事件。
#[test]
fn deduped_codex_session_row_donates_replay_identity_to_proxy_row() {
    let dir = temp_dir("codex-alias-donate");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let proxy = CcsRow {
        request_id: "px-1".into(),
        app_type: "codex".into(),
        model: "gpt-5.6-sol".into(),
        session_id: None,
        data_source: "proxy".into(),
        created_at: ts.as_second(),
        input: 100,
        output: 30,
    };
    let session = CcsRow {
        request_id: "cs-1".into(),
        session_id: Some("sess-1".into()),
        data_source: "codex_session".into(),
        created_at: ts.as_second() + 10,
        ..proxy.clone()
    };
    seed_ccs(&dir.join("cc-switch.db"), &[proxy, session], &[]);
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(
        preview.requests_importable, 1,
        "会话行与 proxy 行同请求，只保留一条"
    );
    assert_eq!(preview.requests_skipped_duplicate_of_proxy, 1);
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 1);
    assert_eq!(
        history.alias_count().unwrap(),
        1,
        "被剔除会话行的重播身份必须转移给保留的 proxy 行"
    );

    // 原生采集带同一 session/模型/四桶 → 命中转移的别名，不新增事件。
    let codex = dir.join("codex");
    codex_rollout(
        &codex,
        "sessions/2026/07/17/rollout-d.jsonl",
        &[
            codex_meta("sess-1", "/work/app"),
            codex_context("gpt-5.6-sol", "/work/app"),
            codex_token_count("2026-07-17T15:00:00.000Z", 100, 30),
        ],
    );
    summary(&opts(&dir, None, Some(codex))).unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "采集并入 proxy 行事件");
    assert_eq!(history.totals().unwrap().input, 100, "token 不翻倍");
    std::fs::remove_dir_all(&dir).ok();
}

/// 复审 #1/#2/#5（迁移分支）：v2 旧库升级到 v3 时——
/// 补齐 Codex 重播身份（文件移动后重扫不再重复计量）、把采集时刻改写为
/// 事件时间（真实终值不再被判过期）、同键日汇总仅时区不同的重复行折叠。
#[test]
fn upgraded_v2_history_backfills_identity_time_and_rollup_key() {
    let dir = temp_dir("v2-upgrade");
    let path = dir.join("data").join("history.db");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let codex_ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let claude_ts: jiff::Timestamp = "2026-07-17T15:01:40Z".parse().unwrap();
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(tokenscope::history::legacy_v1_schema_for_tests())
            .unwrap();
        conn.execute_batch(tokenscope::history::legacy_v2_schema_for_tests())
            .unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        // 旧 Codex 事件：观察时间 = 采集时刻（旧语义），没有 codex-usage 别名。
        conn.execute(
            "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                model_identity, identity_revision, session_id, record_id, project_key,
                session_initial_cwd, event_cwd, input_tokens, output_tokens,
                cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                observed_at_seconds, observed_at_nanos, parser_revision)
             VALUES('native|codex|/logs/a.jsonl|0|fp', 'codex', ?1, 0, 'gpt-5.6-sol',
                    'gpt56sol', 1, 'sess-1', NULL, NULL, NULL, NULL, '100', '30', '0', '0',
                    1, '2026-10-10T00:00:00Z', 1792000000, 0, 1)",
            [codex_ts.as_second()],
        )
        .unwrap();
        // 旧 Claude 事件：终值 5，观察时间 = 采集时刻（晚于事件时间）。
        conn.execute(
            "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                model_identity, identity_revision, session_id, record_id, project_key,
                session_initial_cwd, event_cwd, input_tokens, output_tokens,
                cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                observed_at_seconds, observed_at_nanos, parser_revision)
             VALUES('native|claude|/logs/c.jsonl|0|fp', 'claude-code', ?1, 0,
                    'claude-opus-5-5', 'claudeopus55', 1, 'sess-C', 'm1', NULL, NULL, NULL,
                    '10', '5', '0', '0', 1, '2026-10-10T00:00:00Z', 1792000000, 0, 1)",
            [claude_ts.as_second()],
        )
        .unwrap();
        // 同键日汇总存了两份（v2 把 source_tz 纳入唯一键的缺陷产物）。
        for tz in ["UTC", "Asia/Shanghai"] {
            conn.execute(
                "INSERT INTO ccs_daily_usage(logical_source, day, source_tz, app, provider_id,
                    model, request_model, pricing_model, request_count, input_tokens,
                    output_tokens, cache_write_tokens, cache_read_tokens, input_semantics,
                    source_cost_usd, import_run_id, revision)
                 VALUES('ccs', '2026-06-01', ?1, 'codex', 'p1', 'gpt-5.6', '', '', 10,
                        '1000', '200', '0', '0', 2, '1.5', NULL, 1)",
                [tz],
            )
            .unwrap();
        }
    }
    let history = HistoryDb::open(&path).unwrap();
    assert_eq!(history.schema_version().unwrap(), 3, "升级到当前版本");
    assert_eq!(
        history.rollup_count().unwrap(),
        1,
        "仅时区不同的重复汇总折叠"
    );
    assert_eq!(
        history.rollup_request_count().unwrap(),
        10,
        "请求数不因时区重复翻倍"
    );
    assert_eq!(
        history.daily_rollups(None).unwrap()[0].source_tz,
        "UTC",
        "折叠保留最早一条（与 KeepExisting 策略一致）"
    );

    // 复审 #2：迁移后写回真实终值（同事件键、同事件时间）必须按更新处理，
    // 不再被旧的"采集时刻"观察时间判为过期。
    let ts: jiff::Timestamp = "2026-07-17T15:01:40Z".parse().unwrap();
    let s = history
        .write_batch(&[EventWrite {
            event_key: "native|claude|/logs/c.jsonl|0|fp".into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("sess-C".into()),
            record_id: Some("m1".into()),
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 10,
                output: 20,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: ts,
            aliases: vec![EventAlias {
                app: AgentKind::ClaudeCode,
                scheme: "claude-message".into(),
                value: "sess-C|m1".into(),
            }],
            origins: Vec::new(),
        }])
        .unwrap();
    assert_eq!(s.updated, 1, "真实终值按更新处理（此前被判 stale）");
    assert_eq!(
        history.totals().unwrap().output,
        50,
        "Claude 终值 5 → 20（Codex 30 + Claude 20）"
    );

    // 复审 #1：文件移动后重扫（新来源键 + 重播身份别名）必须命中迁移回填的
    // 别名，不新增事件。
    let moved_ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    history
        .write_batch(&[EventWrite {
            event_key: "native|codex|/archive/a.jsonl|0|fp".into(),
            app: AgentKind::Codex,
            ts: moved_ts,
            model_raw: "gpt-5.6-sol".into(),
            model_identity: "gpt56sol".into(),
            session_id: Some("sess-1".into()),
            record_id: None,
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 100,
                output: 30,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: moved_ts,
            aliases: vec![EventAlias {
                app: AgentKind::Codex,
                scheme: "codex-usage".into(),
                value: "sess-1|gpt-5.6-sol|100|30|0|0".into(),
            }],
            origins: Vec::new(),
        }])
        .unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        2,
        "移动后重扫命中回填的别名，不新增 Codex 事件"
    );
    assert_eq!(
        history.alias_count().unwrap(),
        2,
        "codex-usage 别名只此一条"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 复审 #5（运行期）：同一 CCS 库改选来源时区再次导入，日汇总不重复累计。
#[test]
fn changing_source_timezone_does_not_duplicate_rollups() {
    let dir = temp_dir("rollup-tz");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[("2026-06-01", "codex", "gpt-5.6", "", 10, 1000, 200)],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(preview.rollups_new, 1);
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(history.rollup_count().unwrap(), 1);

    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.rollups_new, 0, "同键快照不再因时区不同变成新行");
    assert_eq!(preview.rollups_unchanged, 1);
    assert_eq!(preview.rollups_conflicting, 0, "仅时区差异不构成内容冲突");
    ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(history.rollup_count().unwrap(), 1, "不产生第二份快照");
    assert_eq!(history.rollup_request_count().unwrap(), 10, "请求不翻倍");
    assert_eq!(
        history.daily_rollups(None).unwrap()[0].source_tz,
        "Asia/Shanghai",
        "时区假设修订为本次导入的选择"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 复审 #4：优先级判定先于观察时间——原生日志比 CCS 导入更可信，即使原生
/// 事件时间早于已存 CCS 值也要胜出；反向（CCS 后到）仍按冲突保留原生值。
#[test]
fn native_precedence_beats_observed_time() {
    let dir = temp_dir("precedence");
    let history = history_of(&dir);
    let t0: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let t5: jiff::Timestamp = "2026-07-17T15:00:05Z".parse().unwrap();
    let alias = EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "sess-1|m1".into(),
    };
    // CCS 先到（created_at 比原生晚 5 秒，输出 50）。
    history
        .write_batch(&[EventWrite {
            event_key: "ccs|claude|proxy|p1".into(),
            app: AgentKind::ClaudeCode,
            ts: t5,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("sess-1".into()),
            record_id: Some("m1".into()),
            project_key: None,
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 10,
                output: 50,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::CcsImport,
            observed_at: t5,
            aliases: vec![alias.clone()],
            origins: Vec::new(),
        }])
        .unwrap();
    // 原生后到（事件时间早 5 秒，输出 30）。
    let s = history
        .write_batch(&[EventWrite {
            event_key: "native|claude|/logs/a.jsonl|0|fp".into(),
            app: AgentKind::ClaudeCode,
            ts: t0,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("sess-1".into()),
            record_id: Some("m1".into()),
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 10,
                output: 30,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: t0,
            aliases: vec![alias.clone()],
            origins: Vec::new(),
        }])
        .unwrap();
    assert_eq!(s.updated, 1, "更高优先级来源不得被观察时间判定拦下");
    assert_eq!(history.totals().unwrap().output, 30, "原生终值胜出");
    // 反向：CCS 再后到不能覆盖原生终值（保持冲突语义）。
    let s2 = history
        .write_batch(&[EventWrite {
            event_key: "ccs|claude|proxy|p2".into(),
            app: AgentKind::ClaudeCode,
            ts: t5,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("sess-1".into()),
            record_id: Some("m1".into()),
            project_key: None,
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 10,
                output: 50,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::CcsImport,
            observed_at: t5,
            aliases: vec![alias],
            origins: Vec::new(),
        }])
        .unwrap();
    assert_eq!(s2.conflicts, 1, "低优先级来源保持冲突语义");
    assert_eq!(history.totals().unwrap().output, 30);
    std::fs::remove_dir_all(&dir).ok();
}
