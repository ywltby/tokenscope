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
//! 14 同刻候选收敛：同身份、相同事件时间的不同终值候选，完整采集与增量采集
//!    （分批到达）必须收敛到同一事实，不由到达顺序决定（2026-11-05）；
//! 15 Codex 重播族时间不后移：同保守身份的后到重播副本不得把已保存事件的
//!    完成时间推到重播时刻，重扫必须幂等（2026-11-05）；
//! 以及计划要求：仅明细查询、升级前备份、事务内 generation 校验。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{
    DailyRollupWrite, EventAlias, EventOrigin, EventWrite, HistoryDb, RollupConflictPolicy,
    WriteClass, WritePrecedence, WriteSummary,
};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

/// L-P2-1：本二进制内触碰进程级钩子（最大并发采集计数 swap(0)、保留字节
/// before/after）的测试与并行采集/会话记账互斥——计数被并行测试污染会
/// 造成 max_parallel/after-delta 假失败。cargo 逐个运行测试二进制，
/// 二进制内互斥即全局互斥。
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-review-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts(dir: &Path, claude: Option<PathBuf>, codex: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

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
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
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
    let _g = serial();
    let dir = temp_dir("deadlock");
    let history = history_of(&dir);
    let rollups: Vec<DailyRollupWrite> = (0..3)
        .map(|i| DailyRollupWrite {
            logical_source: "ccs".into(),
            day: format!("2026-06-0{}", i + 1),
            source_tz: "Asia/Shanghai".into(),
            // 2026-11-05：统计只选取 Claude 的日汇总（CCS 只补充 Claude）。
            app: AgentKind::ClaudeCode,
            provider_id: "_claude_session".into(),
            model: "claude-opus-5-5".into(),
            request_model: String::new(),
            pricing_model: String::new(),
            request_count: 2,
            tokens: TokenCounts {
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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

/// 审阅 #3a（2026-11-05 口径：CCS 只补充 Claude）：**身份不足**（无 session_id
/// 的 proxy 行，构造不出任何别名）的 CCS 来源行，与本地已有请求同时间同用量
/// → 预览列为重叠候选（未解决）、默认拒绝提交；显式确认后按新增导入并留审计。
/// 带 session_id 的会话行登记可证实身份别名，两侧直接命中同一事件。
#[test]
fn ccs_overlap_candidate_requires_explicit_confirmation() {
    let _g = serial();
    let dir = temp_dir("overlap-ccs");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    history
        .write_batch(&[EventWrite {
            event_key: "native|claude|/logs/s.jsonl|3|fp".into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("sess-1".into()),
            record_id: None,
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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
            app_type: "claude".into(),
            model: "claude-opus-5-5".into(),
            session_id: None,
            data_source: "proxy".into(),
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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

// ================================================================ 14：同刻候选收敛

/// 审阅 #14：同 `(session, message.id)`、**相同事件时间**但 token 终值不同的
/// 两份候选（跨文件副本），无论完整采集一次看到两份，还是增量采集分两轮先后
/// 到达，历史库必须收敛到**同一**事实——终值选择不由到达顺序决定。
#[test]
fn claude_same_instant_candidates_converge_regardless_of_arrival_order() {
    let _g = serial();
    let ts = "2026-09-15T08:00:05Z";
    // 文件 a（字典序靠前）：终值 20；文件 b：终值 50。
    let lines_a = vec![
        claude_context("s1", "2026-09-15T08:00:00Z", "/work/app"),
        claude_assistant("s1", "m1", ts, "claude-opus-5-5", 100, 20),
    ];
    let lines_b = vec![
        claude_context("s1", "2026-09-15T08:00:00Z", "/work/app"),
        claude_assistant("s1", "m1", ts, "claude-opus-5-5", 100, 50),
    ];

    // 路径 1：完整采集一轮看到两份。
    let dir = temp_dir("same-instant-full");
    let claude = dir.join("claude");
    write_lines(&claude.join("projects/p/a.jsonl"), &lines_a);
    write_lines(&claude.join("projects/p/b.jsonl"), &lines_b);
    let o = opts(&dir, Some(claude.clone()), None);
    summary(&o).unwrap();
    let history = history_of(&dir);
    assert_eq!(history.event_count().unwrap(), 1);
    let full_run_output = history.totals().unwrap().output;
    assert_eq!(
        full_run_output, 50,
        "全量去重取同刻候选的确定性代表（四桶总量最大者）"
    );
    std::fs::remove_dir_all(&dir).ok();

    // 路径 2：增量到达——先 a 后 b；再反向先 b 后 a。终值必须与完整采集一致。
    for (first, second, tag) in [
        ("a.jsonl", "b.jsonl", "a-then-b"),
        ("b.jsonl", "a.jsonl", "b-then-a"),
    ] {
        let dir = temp_dir(&format!("same-increment-{tag}"));
        let claude = dir.join("claude");
        let source_of = |name: &str| {
            if name == "a.jsonl" {
                lines_a.clone()
            } else {
                lines_b.clone()
            }
        };
        write_lines(&claude.join("projects/p").join(first), &source_of(first));
        let o = opts(&dir, Some(claude.clone()), None);
        summary(&o).unwrap();
        let history = history_of(&dir);
        assert_eq!(history.event_count().unwrap(), 1, "{tag}: 首轮单候选");
        let after_first = history.totals().unwrap().output;
        write_lines(&claude.join("projects/p").join(second), &source_of(second));
        summary(&o).unwrap();
        assert_eq!(history.event_count().unwrap(), 1, "{tag}: 候选合并不新增");
        let after_second = history.totals().unwrap().output;
        assert_eq!(
            after_second, full_run_output,
            "{tag}: 增量收敛必须等于完整采集结果（先 {after_first} 后 {after_second}）"
        );
        // 重启幂等：再来一轮全量重扫，终值不漂移。
        summary(&opts_with_refresh(&o)).unwrap();
        assert_eq!(
            history.totals().unwrap().output,
            full_run_output,
            "{tag}: 重扫后仍收敛到同一事实"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

fn opts_with_refresh(o: &SummaryOptions) -> SummaryOptions {
    let mut r = o.clone();
    r.refresh = true;
    r
}

// ================================================================ 15：Codex 重播时间不后移

/// 审阅 #15：同一保守重播身份（同 session/模型/四桶）的请求先以 t0 入库
///（首条 = 请求完成时刻），随后 t1 > t0 的重播副本/重发增量到达。历史事件
/// 的时间必须保持 t0——重播不得把完成时间推后（跨日重播会污染日期统计）；
/// 之后重扫（含 refresh）也不得翻转回 t1，全量与增量收敛一致。
#[test]
fn codex_replay_keeps_earliest_event_time() {
    let _g = serial();
    let dir = temp_dir("codex-replay-time");
    let codex = dir.join("codex");
    let t0 = "2026-07-17T15:02:00.000Z";
    let t1 = "2026-07-18T16:08:00.000Z";
    let lines_a = vec![
        codex_meta("sess-1", "/work/app"),
        codex_context("gpt-5.6-sol", "/work/app"),
        codex_token_count(t0, 750, 100),
    ];
    let lines_b = vec![
        codex_meta("sess-1", "/work/app"),
        codex_context("gpt-5.6-sol", "/work/app"),
        codex_token_count(t1, 750, 100),
    ];
    let o = opts(&dir, None, Some(codex.clone()));

    // 增量：t0 文件先到（归属日期 07-17），t1 重播副本后到。
    codex_rollout(&codex, "sessions/2026/07/17/rollout-a.jsonl", &lines_a);
    summary(&o).unwrap();
    let history = history_of(&dir);
    let after_first = history.stored_events().unwrap();
    assert_eq!(after_first.len(), 1);
    assert_eq!(after_first[0].ts.to_string(), "2026-07-17T15:02:00Z");

    codex_rollout(&codex, "sessions/2026/07/17/rollout-b.jsonl", &lines_b);
    summary(&o).unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "重播合并，不新增事件");
    let stored = history.stored_events().unwrap();
    assert_eq!(
        stored[0].ts.to_string(),
        "2026-07-17T15:02:00Z",
        "重播副本不得把已保存事件的完成时间推后到重播时刻"
    );
    assert_eq!(history.totals().unwrap().input, 750, "用量不翻倍");

    // 重扫（refresh）：事件时间不得翻转回重播时刻。
    summary(&opts_with_refresh(&o)).unwrap();
    let stored = history.stored_events().unwrap();
    assert_eq!(
        stored[0].ts.to_string(),
        "2026-07-17T15:02:00Z",
        "重扫不得翻转回重播时刻"
    );

    // 完整采集（两份同时可见）也收敛到最早完成时刻。
    let dir2 = temp_dir("codex-replay-time-full");
    let codex2 = dir2.join("codex");
    codex_rollout(&codex2, "sessions/2026/07/17/rollout-a.jsonl", &lines_a);
    codex_rollout(&codex2, "sessions/2026/07/17/rollout-b.jsonl", &lines_b);
    summary(&opts(&dir2, None, Some(codex2))).unwrap();
    let history2 = history_of(&dir2);
    assert_eq!(history2.event_count().unwrap(), 1);
    let stored2 = history2.stored_events().unwrap();
    assert_eq!(
        stored2[0].ts.to_string(),
        "2026-07-17T15:02:00Z",
        "完整采集同样收敛到最早事件时间"
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}

// ================================================================ 6：日汇总主键

/// 审阅 #6：日汇总的完整主键含 `request_model`——两条仅该列不同的合法汇总
/// 必须各自保留，不折叠、不丢用量。
#[test]
fn rollups_with_distinct_request_model_are_not_collapsed() {
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
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
    let _g = serial();
    let dir = temp_dir("upgrade-backup");
    let path = dir.join("history.db");
    {
        let history = HistoryDb::open(&path).unwrap();
        assert_eq!(history.schema_version().unwrap(), 5, "新库直接是当前版本");
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
    }
    let history = HistoryDb::open(&path).unwrap();
    assert_eq!(history.schema_version().unwrap(), 5, "升级到当前版本");
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
    let _g = serial();
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
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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

// ================================================================ 第五轮复核 P2

/// 第五轮 P2-1（2026-11-05 口径：CCS 只补充 Claude）：会话行与 proxy 行按
/// 有效用量去重后，被剔除行捐献的 `claude-message` 身份让原生采集命中同一
/// 事件；内容差异按原生优先级更新为原生值，不重复入库。
#[test]
fn ccs_session_row_donates_identity_so_native_collection_merges() {
    let _g = serial();
    let dir = temp_dir("p2-1-donation");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            CcsRow {
                request_id: "px-1".into(),
                app_type: "claude".into(),
                model: "claude-opus-5-5".into(),
                session_id: None,
                data_source: "proxy".into(),
                created_at: ts.as_second(),
                input: 110,
                output: 30,
            },
            CcsRow {
                request_id: "session:msg-9".into(),
                app_type: "claude".into(),
                model: "claude-opus-5-5".into(),
                session_id: Some("sess-1".into()),
                data_source: "session_log".into(),
                created_at: ts.as_second(),
                input: 110,
                output: 30,
            },
        ],
        &[],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(
        preview.requests_skipped_duplicate_of_proxy, 1,
        "会话行与 proxy 行同请求，只保留一条"
    );
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
        "被剔除会话行的身份必须转移给保留的 proxy 行"
    );

    // 原生采集同一 session/message：命中别名并入同一事件，内容以原生为准。
    let claude = dir.join("claude");
    write_lines(
        &claude.join("proj/s.jsonl"),
        &[
            claude_context("sess-1", "2026-07-17T15:00:00Z", "/work/app"),
            claude_assistant(
                "sess-1",
                "msg-9",
                "2026-07-17T15:00:00Z",
                "claude-opus-5-5",
                110,
                30,
            ),
        ],
    );
    summary(&opts(&dir, Some(claude), None)).unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        1,
        "原生并入已导入事件，不新增"
    );
    let totals = history.totals().unwrap();
    assert_eq!(totals.input, 110, "token 不翻倍");
    assert_eq!(totals.output, 30);
    let events = history.stored_events().unwrap();
    assert_eq!(
        events[0].project_key.as_deref(),
        Some("/work/app"),
        "原生优先级更高：项目归属由原生补齐"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 第五轮 P2-2：内容等值的原生写入必须升级 origin_rank——否则先 CCS 后
/// 原生的确认停留在低优先级，后续同请求的 CCS 变化会被静默覆盖。
#[test]
fn native_confirmation_blocks_later_ccs_overwrite() {
    let _g = serial();
    let dir = temp_dir("p2-2-rank");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-15T08:00:00Z".parse().unwrap();
    let alias = EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "sess-1|m1".into(),
    };
    let ccs_write = |key: &str, output: u64| EventWrite {
        event_key: key.into(),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("sess-1".into()),
        record_id: Some("m1".into()),
        project_key: None,
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 10,
            output,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::CcsImport,
        observed_at: ts,
        aliases: vec![alias.clone()],
        origins: Vec::new(),
    };
    // 1) CCS 先导入 X。
    history.write_batch(&[ccs_write("ccs|p1", 5)]).unwrap();
    // 2) 原生采集同内容 X：内容等值 + 更高优先级 → Update（仅升级 rank）。
    let native = EventWrite {
        event_key: "native|a.jsonl|0".into(),
        precedence: WritePrecedence::NativeLog,
        ..ccs_write("ccs|p0", 5)
    };
    let s = history.write_batch(&[native]).unwrap();
    assert_eq!(s.updated, 1, "内容等值的原生确认按更新处理（升级 rank）");
    // 3) CCS 再导 Y：与已确认的原生值冲突 → 保留原生值，不静默覆盖。
    let s2 = history.write_batch(&[ccs_write("ccs|p2", 50)]).unwrap();
    assert_eq!(s2.conflicts, 1, "CCS 后到按冲突保留原生值");
    assert_eq!(history.totals().unwrap().output, 5);
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ 复审第二轮

/// 复审 #3（2026-11-05 口径：CCS 只补充 Claude）：先导入 CCS、后采集必须与
/// 反向顺序得到**相同并集**——会话行与原生采集共享可证实身份，两侧命中同一
/// 事件；缺 session_id 的行仍走重叠候选（见上一测试）。
#[test]
fn ccs_import_and_collect_yield_the_same_union() {
    let _g = serial();
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    // 2026-11-05 口径：CCS 只补充 Claude Code。会话行与原生采集共享可证实
    // 身份 `session|message.id` → 两个顺序都必须得到同一并集。
    let ccs_row = CcsRow {
        request_id: "session:msg-9".into(),
        app_type: "claude".into(),
        model: "claude-opus-5-5".into(),
        session_id: Some("sess-1".into()),
        data_source: "session_log".into(),
        created_at: ts.as_second(),
        input: 100,
        output: 30,
    };
    let lines = vec![
        claude_context("sess-1", "2026-07-17T15:00:00Z", "/work/app"),
        claude_assistant(
            "sess-1",
            "msg-9",
            "2026-07-17T15:00:00Z",
            "claude-opus-5-5",
            100,
            30,
        ),
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
    assert_eq!(preview.would_overlap, 0, "可证实身份命中，不是重叠候选");
    assert_eq!(preview.would_insert, 1);
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 1);
    let claude = dir.join("claude");
    write_lines(&claude.join("proj/s.jsonl"), &lines);
    summary(&opts(&dir, Some(claude), None)).unwrap();
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
    let claude = dir.join("claude");
    write_lines(&claude.join("proj/s.jsonl"), &lines);
    summary(&opts(&dir, Some(claude), None)).unwrap();
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

/// 复审 #3 的会话行分支（2026-11-05 口径：CCS 只补充 Claude）：会话行因
/// "与 proxy 行重复"被剔除时，它的可证实身份（`claude-message`）必须转移给
/// 被保留的 proxy 行——否则先导入（留 proxy 行）后采集（带 session）仍会为
/// 同一请求再建一条事件。
#[test]
fn deduped_ccs_session_row_donates_identity_to_proxy_row() {
    let _g = serial();
    let dir = temp_dir("ccs-alias-donate");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let proxy = CcsRow {
        request_id: "px-1".into(),
        app_type: "claude".into(),
        model: "claude-opus-5-5".into(),
        session_id: None,
        data_source: "proxy".into(),
        created_at: ts.as_second(),
        input: 100,
        output: 30,
    };
    let session = CcsRow {
        request_id: "session:msg-9".into(),
        session_id: Some("sess-1".into()),
        data_source: "session_log".into(),
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
        "被剔除会话行的身份必须转移给保留的 proxy 行"
    );

    // 原生采集带同一 session/message → 命中转移的别名，不新增事件。
    let claude = dir.join("claude");
    write_lines(
        &claude.join("proj/s.jsonl"),
        &[
            claude_context("sess-1", "2026-07-17T15:00:00Z", "/work/app"),
            claude_assistant(
                "sess-1",
                "msg-9",
                "2026-07-17T15:00:00Z",
                "claude-opus-5-5",
                100,
                30,
            ),
        ],
    );
    summary(&opts(&dir, Some(claude), None)).unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "采集并入 proxy 行事件");
    assert_eq!(history.totals().unwrap().input, 100, "token 不翻倍");
    std::fs::remove_dir_all(&dir).ok();
}

/// 复审 #1/#2/#5（迁移分支）：v2 旧库升级到 v3 时——
/// 补齐 Codex 重播身份（文件移动后重扫不再重复计量）、把采集时刻改写为
/// 事件时间（真实终值不再被判过期）、同键日汇总仅时区不同的重复行折叠。
#[test]
fn upgraded_v2_history_backfills_identity_time_and_rollup_key() {
    let _g = serial();
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
                 VALUES('ccs', '2026-06-01', ?1, 'claude-code', 'p1', 'claude-opus-5-5', '', '', 10,
                        '1000', '200', '0', '0', 2, '1.5', NULL, 1)",
                [tz],
            )
            .unwrap();
        }
    }
    let history = HistoryDb::open(&path).unwrap();
    assert_eq!(history.schema_version().unwrap(), 5, "升级到当前版本");
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
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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
    let _g = serial();
    let dir = temp_dir("rollup-tz");
    let history = history_of(&dir);
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[("2026-06-01", "claude", "claude-opus-5-5", "", 10, 1000, 200)],
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
    let _g = serial();
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
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
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

/// N02：并集身份矩阵（2026-11-05 口径：CCS 只补充 Claude）——
/// 1) 不同会话、同时间同用量：独立请求，两条都保留，CCS 会话行各自命中；
/// 2) 同 `(session, message.id)` 的流式重发：合并为一条（终值规则）；
/// 3) 同一保守身份的不同来源记录：一条事件、两条来源证据（旧数据仍成立）。
#[test]
fn ccs_union_identity_matrix() {
    let _g = serial();
    // 场景 1：独立请求不被并集吞掉。
    let dir = temp_dir("n02-distinct");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let claude = dir.join("claude");
    for (name, session, message) in [("s-a", "sess-A", "msg-A"), ("s-b", "sess-B", "msg-B")] {
        write_lines(
            &claude.join(format!("proj/{name}.jsonl")),
            &[
                claude_context(session, "2026-07-17T15:00:00Z", "/work/app"),
                claude_assistant(
                    session,
                    message,
                    "2026-07-17T15:00:00Z",
                    "claude-opus-5-5",
                    100,
                    30,
                ),
            ],
        );
    }
    summary(&opts(&dir, Some(claude), None)).unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        2,
        "不同会话的同时间同用量是两条真实请求"
    );
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            CcsRow {
                request_id: "session:msg-A".into(),
                app_type: "claude".into(),
                model: "claude-opus-5-5".into(),
                session_id: Some("sess-A".into()),
                data_source: "session_log".into(),
                created_at: ts.as_second(),
                input: 100,
                output: 30,
            },
            CcsRow {
                request_id: "session:msg-B".into(),
                app_type: "claude".into(),
                model: "claude-opus-5-5".into(),
                session_id: Some("sess-B".into()),
                data_source: "session_log".into(),
                created_at: ts.as_second(),
                input: 100,
                output: 30,
            },
        ],
        &[],
    );
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(
        preview.would_overlap, 0,
        "可证实身份按消息区分，不产生重叠候选"
    );
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
        false,
    )
    .unwrap();
    assert_eq!(
        history.event_count().unwrap(),
        2,
        "导入不新增、不合并独立请求"
    );
    assert_eq!(report.requests_conflicted + report.requests_unchanged, 2);
    std::fs::remove_dir_all(&dir).ok();

    // 场景 2：同身份流式重发、不同时间 → 合并为一条（终值）。
    let dir = temp_dir("n02-replay");
    let history = history_of(&dir);
    let claude = dir.join("claude");
    write_lines(
        &claude.join("proj/s.jsonl"),
        &[
            claude_context("sess-1", "2026-07-17T15:00:00Z", "/work/app"),
            claude_assistant(
                "sess-1",
                "msg-1",
                "2026-07-17T15:00:00Z",
                "claude-opus-5-5",
                100,
                10,
            ),
            claude_assistant(
                "sess-1",
                "msg-1",
                "2026-07-17T15:00:05Z",
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
        "同一 (session, message.id) 的流式重发合并为一条"
    );
    assert_eq!(history.totals().unwrap().output, 30, "保留终值");
    std::fs::remove_dir_all(&dir).ok();

    // 场景 3：旧数据的两条 CCS 来源记录同身份 → 一个事件、两条来源证据。
    let dir = temp_dir("n02-two-origins");
    let history = history_of(&dir);
    let alias = EventAlias {
        app: AgentKind::Codex,
        scheme: "codex-usage".into(),
        value: "sess-1|gpt-5.6-sol|100|30|0|0".into(),
    };
    let write = |rid: &str| EventWrite {
        event_key: format!("ccs|codex|proxy|{rid}"),
        app: AgentKind::Codex,
        ts,
        model_raw: "gpt-5.6-sol".into(),
        model_identity: "gpt56sol".into(),
        session_id: Some("sess-1".into()),
        record_id: None,
        project_key: None,
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 100,
            output: 30,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::CcsImport,
        observed_at: ts,
        aliases: vec![alias.clone()],
        origins: vec![EventOrigin {
            origin_kind: "ccs_request".into(),
            origin_key: format!("ccs|codex|proxy|{rid}"),
            app: AgentKind::Codex,
            import_run_id: None,
            parser_revision: 3,
            source_model_raw: Some("gpt-5.6-sol".into()),
            pricing_model: Some("gpt-5.6-sol".into()),
            source_cost_usd: None,
            ts_precision_seconds: true,
        }],
    };
    history
        .write_batch(&[write("px-1"), write("px-2")])
        .unwrap();
    assert_eq!(history.event_count().unwrap(), 1, "同一身份只新增一个事件");
    assert_eq!(
        history.origin_count().unwrap(),
        2,
        "两条来源记录各自登记证据，不复制用量"
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ================================================================ R01：预览/提交对账

/// R01 反例 1：同一事实经不同来源键先后更新时，预览（`classify_batch`）与
/// 提交（`write_batch`）的处置必须一致；事实始终一条，token 不被拼接。
/// （2026-11-05：CCS 只补充 Claude，这里用 `claude-message` 身份直接验证
/// 库级判定，不依赖导入路径。）
#[test]
fn batch_preview_matches_commit_for_same_fact_updates() {
    let _g = serial();
    let dir = temp_dir("r01-parity");
    let history = history_of(&dir);
    let t0: jiff::Timestamp = "2026-07-17T15:00:00Z".parse().unwrap();
    let t1: jiff::Timestamp = "2026-07-17T15:00:01Z".parse().unwrap();
    let alias = EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: "sess-1|msg-1".into(),
    };
    let write = |rid: &'static str, ts: jiff::Timestamp, output: u64| EventWrite {
        event_key: format!("ccs|claude|proxy|{rid}"),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("sess-1".into()),
        record_id: Some("msg-1".into()),
        project_key: None,
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 100,
            output,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::CcsImport,
        observed_at: ts,
        aliases: vec![alias.clone()],
        origins: vec![EventOrigin {
            origin_kind: "ccs_request".into(),
            origin_key: format!("ccs|claude|proxy|{rid}"),
            app: AgentKind::ClaudeCode,
            import_run_id: None,
            parser_revision: 3,
            source_model_raw: Some("claude-opus-5-5".into()),
            pricing_model: Some("claude-opus-5-5".into()),
            source_cost_usd: None,
            ts_precision_seconds: true,
        }],
    };

    // 同一事实两条来源记录：先插入、后到更晚观察（内容不同）→ 更新。
    let w1 = write("p1", t0, 10);
    let w2 = write("p2", t1, 20);
    assert_eq!(
        history.classify_batch(&[w1.clone(), w2.clone()]).unwrap(),
        vec![WriteClass::Insert, WriteClass::Update]
    );
    let s = history.write_batch(&[w1, w2]).unwrap();
    assert_eq!(s.inserted, 1);
    assert_eq!(s.updated, 1, "预览与提交一致");
    assert_eq!(history.event_count().unwrap(), 1, "事实始终一条");
    assert_eq!(
        history.totals().unwrap().output,
        20,
        "流式终值：更晚观察胜出"
    );

    // 更早的观察（内容不同）→ 预览与提交都按陈旧保留。
    let w3 = write("p3", t0, 99);
    assert_eq!(
        history.classify_batch(std::slice::from_ref(&w3)).unwrap(),
        vec![WriteClass::Stale]
    );
    let s = history.write_batch(&[w3]).unwrap();
    assert_eq!(s.stale, 1, "预览与提交一致");
    assert_eq!(history.totals().unwrap().output, 20, "不回退");
    std::fs::remove_dir_all(&dir).ok();
}

/// R01 反例 2：同一事实持有多个别名（a、b）。write1 经 a 更新，write2 经
/// a、b 再更新——两个别名都指向**同一事实**，不得误报"别名指向不同事件"。
#[test]
fn batch_preview_multi_alias_of_same_fact_does_not_conflict() {
    let _g = serial();
    let dir = temp_dir("r01-multi-alias");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-15T08:00:00Z".parse().unwrap();
    let alias = |v: &str| EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: v.into(),
    };
    history
        .write_batch(&[EventWrite {
            event_key: "fact|1".into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("s1".into()),
            record_id: Some("m1".into()),
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
                input: 10,
                output: 10,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: ts,
            aliases: vec![alias("s1|a"), alias("s1|b")],
            origins: Vec::new(),
        }])
        .unwrap();

    let write = |key: &str, output: u64, at: &str, aliases: Vec<EventAlias>| EventWrite {
        event_key: key.into(),
        app: AgentKind::ClaudeCode,
        ts: at.parse().unwrap(),
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("s1".into()),
        record_id: Some("m1".into()),
        project_key: Some("/work/app".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 10,
            output,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: at.parse().unwrap(),
        aliases,
        origins: Vec::new(),
    };
    let w1 = write("w|1", 20, "2026-09-15T08:01:00Z", vec![alias("s1|a")]);
    let w2 = write(
        "w|2",
        30,
        "2026-09-15T08:02:00Z",
        vec![alias("s1|a"), alias("s1|b")],
    );

    // 预览：两个别名命中同一事实 → [更新, 更新]，不报身份冲突。
    let classes = history.classify_batch(&[w1.clone(), w2.clone()]).unwrap();
    assert_eq!(
        classes,
        vec![
            tokenscope::history::WriteClass::Update,
            tokenscope::history::WriteClass::Update
        ],
        "多别名同事实按统一终值判定"
    );

    // 提交对账：两次都是更新，最终一条事实、一个终值。
    let s = history.write_batch(&[w1, w2]).unwrap();
    assert_eq!(s.updated, 2);
    assert_eq!(history.event_count().unwrap(), 1);
    assert_eq!(history.totals().unwrap().output, 30, "终值统一");
    std::fs::remove_dir_all(&dir).ok();
}

/// R01 反例 3：内容未变化的行携带**新别名**时，提交会把别名补记到同一
/// 事实——后续只携带该别名的写入必须命中同一事实（未变化），而不是误报
/// 新增。
#[test]
fn batch_preview_unchanged_row_claims_new_aliases() {
    let _g = serial();
    let dir = temp_dir("r01-unchanged-claims");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-15T08:00:00Z".parse().unwrap();
    let alias = |v: &str| EventAlias {
        app: AgentKind::ClaudeCode,
        scheme: "claude-message".into(),
        value: v.into(),
    };
    let base = EventWrite {
        event_key: "fact|1".into(),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("s1".into()),
        record_id: Some("m1".into()),
        project_key: Some("/work/app".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 10,
            output: 10,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases: vec![alias("s1|a")],
        origins: Vec::new(),
    };
    history.write_batch(std::slice::from_ref(&base)).unwrap();

    // write1：内容相同、携带 a + 新别名 b → 未变化（提交会把 b 补记）。
    let mut w1 = base.clone();
    w1.event_key = "w|1".into();
    w1.aliases = vec![alias("s1|a"), alias("s1|b")];
    // write2：内容相同、只携带 b → 必须经 write1 补记的别名命中同一事实。
    let mut w2 = base.clone();
    w2.event_key = "w|2".into();
    w2.aliases = vec![alias("s1|b")];

    let classes = history.classify_batch(&[w1.clone(), w2.clone()]).unwrap();
    assert_eq!(
        classes,
        vec![
            tokenscope::history::WriteClass::Unchanged,
            tokenscope::history::WriteClass::Unchanged
        ],
        "未变化行补记的别名对后续写入可见"
    );

    // 提交对账：两条都未变化、别名净增一条、事实一条。
    let s = history.write_batch(&[w1, w2]).unwrap();
    assert_eq!(s.unchanged, 2);
    assert_eq!(s.aliases_added, 1, "只有 b 是新别名");
    assert_eq!(history.event_count().unwrap(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// R01 第二轮反例 1：来源记录已绑定另一事实时，预览必须与提交同口径报错
/// （此前预览 `Ok([Unchanged])`、提交整批失败"来源记录冲突"）。
#[test]
fn batch_preview_rejects_origin_bound_to_another_fact() {
    let _g = serial();
    let dir = temp_dir("r01b-origin-conflict");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-15T08:00:00Z".parse().unwrap();
    let event = |key: &str, output: u64, alias_v: &str, origin: Option<&str>| EventWrite {
        event_key: key.into(),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("s1".into()),
        record_id: Some("m1".into()),
        project_key: Some("/work/app".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 10,
            output,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::CcsImport,
        observed_at: ts,
        aliases: vec![EventAlias {
            app: AgentKind::ClaudeCode,
            scheme: "claude-message".into(),
            value: format!("s1|{alias_v}"),
        }],
        origins: match origin {
            Some(o) => vec![tokenscope::history::EventOrigin {
                origin_kind: "ccs-request".into(),
                origin_key: o.into(),
                app: AgentKind::ClaudeCode,
                import_run_id: None,
                parser_revision: 1,
                source_model_raw: None,
                pricing_model: None,
                source_cost_usd: None,
                ts_precision_seconds: true,
            }],
            None => Vec::new(),
        },
    };
    // 事实 A（别名 a）与事实 B（别名 b + 来源记录 bound-to-b）。
    history
        .write_batch(&[
            event("fact-a", 10, "a", None),
            event("fact-b", 20, "b", Some("bound-to-b")),
        ])
        .unwrap();
    assert_eq!(history.event_count().unwrap(), 2);

    // 对 A 写入相同内容但携带 B 的来源记录：提交必然整批失败，预览同口径。
    let mut w = event("fact-a", 10, "a", None);
    w.origins = event("fact-b", 20, "b", Some("bound-to-b")).origins;
    let err = history
        .classify_batch(&[w])
        .expect_err("预览必须与提交同口径拒绝来源记录改绑");
    assert!(err.to_string().contains("来源记录冲突"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

/// R01 第二轮反例 2：非 Insert 类别不得把本条的替代事件键登记为身份——
/// 否则后续同键写入被错误命中旧事实（预览 [未变化, 更新]、提交
/// inserted=1/unchanged=1）。修复后两侧一致为 [未变化, 新增]。
#[test]
fn batch_preview_does_not_persist_alternate_event_keys() {
    let _g = serial();
    let dir = temp_dir("r01b-alt-key");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-15T08:00:00Z".parse().unwrap();
    let write = |key: &str, output: u64, alias: Option<&str>| EventWrite {
        event_key: key.into(),
        app: AgentKind::ClaudeCode,
        ts,
        model_raw: "claude-opus-5-5".into(),
        model_identity: "claudeopus55".into(),
        session_id: Some("s1".into()),
        record_id: Some("m1".into()),
        project_key: Some("/work/app".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
            input: 10,
            output,
            cache_write: 0,
            cache_read: 0,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases: match alias {
            Some(v) => vec![EventAlias {
                app: AgentKind::ClaudeCode,
                scheme: "claude-message".into(),
                value: format!("s1|{v}"),
            }],
            None => Vec::new(),
        },
        origins: Vec::new(),
    };
    // 既有事实：规范键 canonical、别名 a、output 10。
    history
        .write_batch(&[write("canonical", 10, Some("a"))])
        .unwrap();

    // write1：替代键 other-source、经别名 a 命中、内容相同 → 未变化；
    // 其替代键不得被登记为身份。write2：同替代键、output 20、无别名 →
    // 提交按新增处理（键与别名都不命中）。
    let w1 = write("other-source", 10, Some("a"));
    let w2 = write("other-source", 20, None);
    let classes = history.classify_batch(&[w1.clone(), w2.clone()]).unwrap();
    assert_eq!(
        classes,
        vec![
            tokenscope::history::WriteClass::Unchanged,
            tokenscope::history::WriteClass::Insert
        ],
        "替代事件键不持久化：write2 按新增分类"
    );

    // 提交对账：inserted=1、unchanged=1，最终两条事实（与预览一致）。
    let s = history.write_batch(&[w1, w2]).unwrap();
    assert_eq!(s.inserted, 1);
    assert_eq!(s.unchanged, 1);
    assert_eq!(s.updated, 0);
    assert_eq!(history.event_count().unwrap(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// T01：批内来源身份是**完整二元组** `(origin_kind, origin_key)`——不同
/// 种类的同值键是不同身份。两个合法场景的预览/提交对账：
/// 1) 空库插入 A/native_file 与 B/ccs_request（同键）→ 两条独立事实；
/// 2) 已有 A/B 各自补同键不同种类的来源证据 → 两条未变化、证据各自落位。
#[test]
fn batch_preview_origin_identity_includes_kind() {
    let _g = serial();
    let dir = temp_dir("t01-origin-kind");
    let history = history_of(&dir);
    let ts: jiff::Timestamp = "2026-09-15T08:00:00Z".parse().unwrap();
    let origin = |kind: &str, key: &str| tokenscope::history::EventOrigin {
        origin_kind: kind.into(),
        origin_key: key.into(),
        app: AgentKind::ClaudeCode,
        import_run_id: None,
        parser_revision: 1,
        source_model_raw: None,
        pricing_model: None,
        source_cost_usd: None,
        ts_precision_seconds: true,
    };
    let write =
        |key: &str, output: u64, origins: Vec<tokenscope::history::EventOrigin>| EventWrite {
            event_key: key.into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: "claude-opus-5-5".into(),
            model_identity: "claudeopus55".into(),
            session_id: Some("s1".into()),
            record_id: Some("m1".into()),
            project_key: Some("/work/app".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
                input: 10,
                output,
                cache_write: 0,
                cache_read: 0,
            },
            precedence: WritePrecedence::CcsImport,
            observed_at: ts,
            aliases: Vec::new(),
            origins,
        };

    // 场景 1：空库插入两种来源的同值键 → 两条独立事实。
    let w1 = write("fact-a", 10, vec![origin("native_file", "shared-key")]);
    let w2 = write("fact-b", 20, vec![origin("ccs-request", "shared-key")]);
    let classes = history.classify_batch(&[w1.clone(), w2.clone()]).unwrap();
    assert_eq!(
        classes,
        vec![
            tokenscope::history::WriteClass::Insert,
            tokenscope::history::WriteClass::Insert
        ],
        "不同来源种类的同值键不是同一身份"
    );
    let s = history.write_batch(&[w1, w2]).unwrap();
    assert_eq!(s.inserted, 2);
    assert_eq!(history.event_count().unwrap(), 2);

    // 场景 2：两条事实（无来源证据）各自补同键不同种类的证据 → 未变化、
    // 证据各自落位，不被误报为来源冲突。
    let dir2 = temp_dir("t01-origin-kind-evidence");
    let history2 = history_of(&dir2);
    let bare = |key: &str, output: u64| write(key, output, Vec::new());
    history2
        .write_batch(&[bare("fact-a", 10), bare("fact-b", 20)])
        .unwrap();
    let w3 = write("fact-a", 10, vec![origin("native_file", "shared-key")]);
    let w4 = write("fact-b", 20, vec![origin("ccs-request", "shared-key")]);
    let classes = history2.classify_batch(&[w3.clone(), w4.clone()]).unwrap();
    assert_eq!(
        classes,
        vec![
            tokenscope::history::WriteClass::Unchanged,
            tokenscope::history::WriteClass::Unchanged
        ],
        "补证据不得被误报为来源冲突"
    );
    let s = history2.write_batch(&[w3, w4]).unwrap();
    assert_eq!(s.unchanged, 2);
    assert_eq!(s.origins_added, 2, "两种来源证据各自登记");
    assert_eq!(history2.event_count().unwrap(), 2);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}
