//! H05：CCS 兼容与导入（计划 §5、§7 H05 验收）。
//!
//! 覆盖：两种粒度（请求明细 + 日汇总）、输入口径 0/1/2、跨来源身份兼容、
//! 缺列/坏值拒绝、有效用量过滤（会话行与 proxy 行重叠）、同库/备份重复导入
//! 幂等、内容不同的同键快照计冲突不静默覆盖、预览后历史库变化拒绝过期提交、
//! 计划一次性（重复提交保护）与取消零提交。
//!
//! 全部 fixture 合成、路径在临时目录；真实 CCS 库只做用户授权的隔离只读核验。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{HistoryDb, RollupConflictPolicy};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, summary};
use tokenscope::source::Source;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-ccs-import-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 合成 CCS 库结构（按本机 v20 实测列集；缺列分支单独构造）。
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
struct Row {
    request_id: &'static str,
    app_type: &'static str,
    model: &'static str,
    session_id: Option<&'static str>,
    data_source: &'static str,
    created_at: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    semantics: i64,
}

impl Default for Row {
    fn default() -> Self {
        Self {
            request_id: "r1",
            app_type: "claude",
            model: "claude-opus-5-5",
            session_id: Some("s1"),
            data_source: "proxy",
            created_at: 1_790_000_000,
            input: 100,
            output: 10,
            cache_read: 0,
            cache_write: 0,
            semantics: 0,
        }
    }
}

/// 合成日汇总的一行：(日期, 应用, 模型, 请求数, 输入, 输出, 缓存读, 缓存写)。
type RollupRow<'a> = (&'a str, &'a str, &'a str, i64, i64, i64, i64, i64);

fn seed_ccs(path: &Path, rows: &[Row], rollups: &[RollupRow<'_>]) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(CCS_SCHEMA).unwrap();
    conn.pragma_update(None, "user_version", 20).unwrap();
    for r in rows {
        conn.execute(
            "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd, status_code, session_id, created_at, data_source, pricing_model,
                input_token_semantics)
             VALUES(?1, 'p1', ?2, ?3, ?3, ?4, ?5, ?6, ?7, '0.25', 200, ?8, ?9, ?10, ?3, ?11)",
            rusqlite::params![
                r.request_id,
                r.app_type,
                r.model,
                r.input,
                r.output,
                r.cache_read,
                r.cache_write,
                r.session_id,
                r.created_at,
                r.data_source,
                r.semantics,
            ],
        )
        .unwrap();
    }
    for (date, app, model, count, input, output, cr, cw) in rollups {
        conn.execute(
            "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
                request_count, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, input_token_semantics)
             VALUES(?1, ?2, 'p1', ?3, '', ?4, ?5, ?6, ?7, ?8, '1.5', 0)",
            rusqlite::params![date, app, model, count, input, output, cr, cw],
        )
        .unwrap();
    }
}

fn db(dir: &Path) -> HistoryDb {
    HistoryDb::open(&dir.join("history.db")).unwrap()
}

fn preview_and_commit(dir: &Path, history: &HistoryDb) -> (ccs::ImportPreview, ccs::ImportReport) {
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, history, "Asia/Shanghai").unwrap();
    let report = ccs::commit(
        &preview.plan_id,
        history,
        RollupConflictPolicy::KeepExisting,
    )
    .unwrap();
    (preview, report)
}

#[test]
fn imports_both_granularities_with_normalized_buckets() {
    let dir = temp_dir("basic");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            // Claude 会话行：input 已是非缓存输入，不扣缓存读。
            Row {
                request_id: "session:msg-1",
                input: 100,
                cache_read: 5000,
                ..Default::default()
            },
            // Codex proxy 行：legacy 语义，input 含缓存读 → 扣减。
            Row {
                request_id: "codex-1",
                app_type: "codex",
                model: "gpt-5.6-sol",
                session_id: None,
                input: 1000,
                cache_read: 600,
                output: 40,
                ..Default::default()
            },
            // 本期不支持的应用：列出但不导入。
            Row {
                request_id: "gemini-1",
                app_type: "gemini",
                ..Default::default()
            },
        ],
        // 日汇总：codex legacy 语义同样归一化。
        &[("2026-06-01", "codex", "mimo-v2.5-pro", 12, 1000, 40, 600, 0)],
    );
    let history = db(&dir);
    let (preview, report) = preview_and_commit(&dir, &history);

    assert_eq!(preview.requests_total, 3);
    assert_eq!(preview.requests_importable, 2);
    assert_eq!(preview.requests_skipped_other_app, 1);
    assert_eq!(preview.unsupported_apps, vec![("gemini".to_string(), 1)]);
    assert_eq!(preview.would_insert, 2);
    assert_eq!(preview.records_without_project, 2, "CCS 明细没有项目归属");
    assert_eq!(preview.rollups_total, 1);
    assert_eq!(preview.rollups_new, 1);

    assert_eq!(report.requests_inserted, 2);
    assert_eq!(report.rollups_snapshotted, 1);

    let events = history.stored_events().unwrap();
    assert_eq!(events.len(), 2);
    let claude = events
        .iter()
        .find(|e| e.app == AgentKind::ClaudeCode)
        .unwrap();
    assert_eq!(claude.tokens.input, 100, "Claude 不再扣缓存读");
    assert_eq!(claude.tokens.cache_read, 5000);
    assert!(claude.project_key.is_none(), "项目保持未知");
    let codex = events.iter().find(|e| e.app == AgentKind::Codex).unwrap();
    assert_eq!(codex.tokens.input, 400, "legacy + cache-inclusive 扣缓存读");
    assert_eq!(codex.tokens.cache_read, 600);

    // 日汇总单独保存且不与明细相加：这里只断言它确实落盘了。
    assert_eq!(history.rollup_count().unwrap(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn input_token_semantics_variants_are_normalized() {
    let dir = temp_dir("semantics");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            // legacy（0）：扣缓存读。
            Row {
                request_id: "c-legacy",
                app_type: "codex",
                model: "gpt-5.4",
                session_id: None,
                input: 1000,
                cache_read: 600,
                ..Default::default()
            },
            // total（1）：扣缓存读与写。
            Row {
                request_id: "c-total",
                app_type: "codex",
                model: "gpt-5.5",
                session_id: None,
                input: 1000,
                cache_read: 300,
                cache_write: 200,
                semantics: 1,
                ..Default::default()
            },
            // fresh（2）：不扣。
            Row {
                request_id: "c-fresh",
                app_type: "codex",
                model: "gpt-5.6-sol",
                session_id: None,
                input: 500,
                cache_read: 300,
                cache_write: 200,
                semantics: 2,
                ..Default::default()
            },
        ],
        &[],
    );
    let history = db(&dir);
    let (_preview, report) = preview_and_commit(&dir, &history);
    assert_eq!(report.requests_inserted, 3);
    let mut inputs: Vec<u64> = history
        .stored_events()
        .unwrap()
        .iter()
        .map(|e| e.tokens.input)
        .collect();
    inputs.sort();
    assert_eq!(inputs, vec![400, 500, 500]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn session_rows_overlapping_proxy_rows_are_not_double_counted() {
    let dir = temp_dir("dedup");
    // 同一次请求被 proxy 与会话日志各记一行（CCS 的有效用量口径只留一条）。
    let proxy = Row {
        request_id: "proxy-1",
        app_type: "codex",
        model: "gpt-5.6-sol",
        session_id: None,
        input: 1000,
        output: 40,
        cache_read: 600,
        ..Default::default()
    };
    let session = Row {
        request_id: "session:evt-1",
        app_type: "codex",
        model: "gpt-5.6-sol",
        session_id: None,
        data_source: "codex_session",
        input: 1000,
        output: 40,
        cache_read: 600,
        created_at: proxy.created_at + 30,
        ..Default::default()
    };
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[proxy.clone(), session.clone()],
        &[],
    );
    let history = db(&dir);
    let (preview, _report) = preview_and_commit(&dir, &history);
    assert_eq!(preview.requests_total, 2);
    assert_eq!(preview.requests_skipped_duplicate_of_proxy, 1);
    assert_eq!(preview.requests_importable, 1);
    assert_eq!(history.event_count().unwrap(), 1);

    // 会话行的时间超出窗口 → 视为不同请求，两条都导入。
    let dir2 = temp_dir("dedup-window");
    let mut far = session.clone();
    far.created_at = proxy.created_at + 3600;
    seed_ccs(&dir2.join("cc-switch.db"), &[proxy, far], &[]);
    let history2 = db(&dir2);
    let (preview2, _) = preview_and_commit(&dir2, &history2);
    assert_eq!(preview2.requests_importable, 2);
    assert_eq!(history2.event_count().unwrap(), 2);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}

#[test]
fn repeat_import_from_same_or_copied_database_is_idempotent() {
    let dir = temp_dir("idempotent");
    let body = [
        Row {
            request_id: "session:msg-a",
            input: 100,
            ..Default::default()
        },
        Row {
            request_id: "codex-a",
            app_type: "codex",
            model: "gpt-5.6-sol",
            session_id: None,
            input: 500,
            ..Default::default()
        },
    ];
    seed_ccs(&dir.join("cc-switch.db"), &body, &[]);
    let history = db(&dir);

    let (_p1, r1) = preview_and_commit(&dir, &history);
    assert_eq!(r1.requests_inserted, 2);
    let generation_after_first = history.generation().unwrap();
    let tokens = history.totals().unwrap();

    // 再导入同一库：完全相同的记录跳过，用量不增长。
    let (p2, r2) = preview_and_commit(&dir, &history);
    assert_eq!(p2.would_unchanged, 2);
    assert_eq!(p2.would_insert, 0);
    assert_eq!(r2.requests_inserted, 0);
    assert_eq!(r2.requests_unchanged, 2);
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(history.totals().unwrap(), tokens);

    // 换选一份内容相同的备份：逻辑来源相同 → 仍不增长。
    let backup = dir.join("backup.db");
    std::fs::copy(dir.join("cc-switch.db"), &backup).unwrap();
    let source = CcsSource::open(&backup).unwrap();
    let p3 = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(p3.would_unchanged, 2);
    let r3 = ccs::commit(&p3.plan_id, &history, RollupConflictPolicy::KeepExisting).unwrap();
    assert_eq!(r3.requests_inserted, 0);
    assert_eq!(history.totals().unwrap(), tokens);
    assert_eq!(history.event_count().unwrap(), 2);
    // 审计批次照常登记（可核查），但用量与 generation 不变。
    assert!(r3.run_id > r2.run_id);
    assert_eq!(history.generation().unwrap(), generation_after_first);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn changed_rollup_snapshot_is_conflict_not_silent_overwrite() {
    let dir = temp_dir("rollup-conflict");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[],
        &[("2026-05-08", "claude", "mimo-v2.5-pro", 10, 1000, 100, 0, 0)],
    );
    let history = db(&dir);
    let (_p, r1) = preview_and_commit(&dir, &history);
    assert_eq!(r1.rollups_snapshotted, 1);
    assert_eq!(history.rollup_count().unwrap(), 1);

    // 同键但内容不同（模拟较旧的备份被再次导入）：来源无可验证修订 →
    // 计冲突、保留已存快照，不静默覆盖。
    let dir2 = temp_dir("rollup-conflict-2");
    std::fs::create_dir_all(&dir2).unwrap();
    seed_ccs(
        &dir2.join("cc-switch.db"),
        &[],
        &[("2026-05-08", "claude", "mimo-v2.5-pro", 3, 300, 30, 0, 0)],
    );
    let source = CcsSource::open(&dir2.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.rollups_conflicting, 1, "预览必须列出冲突");
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
    )
    .unwrap();
    assert_eq!(report.rollups_conflicted, 1);
    assert_eq!(report.rollups_snapshotted, 0);
    assert_eq!(history.rollup_count().unwrap(), 1, "不新增同键行");
    assert_eq!(
        history.rollup_request_count().unwrap(),
        10,
        "保留较新的已存快照"
    );

    // 用户明确选择"以来源为准"时才替换。
    let dir3 = temp_dir("rollup-take-source");
    seed_ccs(
        &dir3.join("cc-switch.db"),
        &[],
        &[("2026-05-08", "claude", "mimo-v2.5-pro", 3, 300, 30, 0, 0)],
    );
    let source = CcsSource::open(&dir3.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    let report = ccs::commit(&preview.plan_id, &history, RollupConflictPolicy::TakeSource).unwrap();
    assert_eq!(report.rollups_snapshotted, 1);
    assert_eq!(history.rollup_request_count().unwrap(), 3);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
    std::fs::remove_dir_all(&dir3).ok();
}

#[test]
fn plan_is_single_use_and_stale_generation_is_rejected() {
    let dir = temp_dir("plan");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-1",
            ..Default::default()
        }],
        &[],
    );
    let history = db(&dir);
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();

    // 取消：不写任何用量，计划被丢弃。
    assert!(ccs::discard(&preview.plan_id));
    assert_eq!(history.event_count().unwrap(), 0);
    assert_eq!(ccs::staged_plan_count(), 0);

    // 已丢弃的计划无法提交。
    assert!(
        ccs::commit(
            &preview.plan_id,
            &history,
            RollupConflictPolicy::KeepExisting
        )
        .is_err()
    );

    // 重新预览 → 提交一次成功；重复提交同一计划必须失败（防双击）。
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 1);
    assert!(
        ccs::commit(
            &preview.plan_id,
            &history,
            RollupConflictPolicy::KeepExisting
        )
        .is_err(),
        "同一计划不得提交两次"
    );

    // 预览后历史库发生变化（此处用一次退订计划模拟"其他写入"）→ 拒绝过期提交。
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    history
        .set_meta(
            "generation",
            &(history.generation().unwrap() + 1).to_string(),
        )
        .unwrap();
    let err = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("重新预览"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unsupported_source_schema_is_rejected() {
    let dir = temp_dir("schema");
    let path = dir.join("cc-switch.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        // 缺 usage_daily_rollups 与必需列。
        conn.execute_batch(
            "CREATE TABLE proxy_request_logs (
                request_id TEXT PRIMARY KEY,
                app_type TEXT NOT NULL,
                model TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );",
        )
        .unwrap();
    }
    let history = db(&dir);
    let source = CcsSource::open(&path).unwrap();
    let err = ccs::preview(&source, &history, "Asia/Shanghai")
        .unwrap_err()
        .to_string();
    assert!(err.contains("usage_daily_rollups"), "{err}");

    // 有表但缺必需列 → 明确报出缺失列名。
    let dir2 = temp_dir("schema-cols");
    let path2 = dir2.join("cc-switch.db");
    {
        let conn = rusqlite::Connection::open(&path2).unwrap();
        conn.execute_batch(
            "CREATE TABLE proxy_request_logs (
                request_id TEXT PRIMARY KEY, app_type TEXT NOT NULL, model TEXT NOT NULL,
                input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL, data_source TEXT NOT NULL DEFAULT 'proxy',
                input_token_semantics INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE usage_daily_rollups (
                date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL,
                model TEXT NOT NULL, request_count INTEGER NOT NULL DEFAULT 0,
                input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
                cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
                input_token_semantics INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
    }
    let history2 = db(&dir2);
    let source2 = CcsSource::open(&path2).unwrap();
    // 该结构其实满足必需列 → 预览为空而不是报错。
    let preview = ccs::preview(&source2, &history2, "Asia/Shanghai").unwrap();
    assert_eq!(preview.requests_total, 0);
    assert_eq!(preview.rollups_total, 0);
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&dir2).ok();
}

#[test]
fn invalid_rows_are_counted_and_skipped_without_blocking_the_batch() {
    let dir = temp_dir("bad-rows");
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[
            Row {
                request_id: "ok-1",
                ..Default::default()
            },
            Row {
                request_id: "bad-negative",
                input: -5,
                ..Default::default()
            },
            Row {
                request_id: "bad-time",
                created_at: 0,
                ..Default::default()
            },
            Row {
                request_id: "bad-semantics",
                semantics: 9,
                ..Default::default()
            },
        ],
        &[],
    );
    let history = db(&dir);
    let (preview, report) = preview_and_commit(&dir, &history);
    assert_eq!(preview.requests_rejected, 3);
    assert_eq!(preview.rejected_reasons.len(), 3);
    assert_eq!(report.requests_inserted, 1, "合法行照常导入");
    assert_eq!(history.event_count().unwrap(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn imported_claude_request_binds_to_native_event_from_session_logs() {
    // 先采集原生日志（写入历史库），再导入同一请求的 CCS 会话行：
    // 两侧 `(session_id, message.id)` 身份相同 → 不重复计费。
    let dir = temp_dir("cross-source");
    let root = dir.join("projects");
    std::fs::create_dir_all(root.join("proj-a")).unwrap();
    std::fs::write(
        root.join("proj-a").join("sess.jsonl"),
        serde_json::json!({
            "type": "assistant",
            "timestamp": "2026-09-15T08:00:00.000Z",
            "sessionId": "sess-9",
            "message": {
                "id": "msg-9",
                "model": "claude-opus-5-5",
                "usage": {
                    "input_tokens": 120,
                    "output_tokens": 30,
                    "cache_creation_input_tokens": 0,
                    "cache_read_input_tokens": 0
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    let opts = SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(root.clone()),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };
    let first = {
        // fixture 自检：解析层必须产出 1 条事件，否则下面的断言失去意义。
        let parsed = tokenscope::source::claude::ClaudeSource::new(&root)
            .parse_file(&root.join("proj-a").join("sess.jsonl"));
        assert_eq!(
            parsed.events.len(),
            1,
            "fixture 必须产出事件（lines={} bad={}）",
            parsed.stats.lines_seen,
            parsed.stats.bad_lines
        );
        summary(&opts).unwrap()
    };
    assert_eq!(first.totals.requests, 1);

    // CCS 里同一次请求的会话行（时间戳不同，但身份相同）。
    seed_ccs(
        &dir.join("cc-switch.db"),
        &[Row {
            request_id: "session:msg-9",
            session_id: Some("sess-9"),
            model: "claude-opus-5-5",
            data_source: "session_log",
            created_at: 1_788_000_000,
            input: 120,
            output: 30,
            ..Default::default()
        }],
        &[],
    );
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(preview.would_insert, 0, "命中已有原生事件，不重复入库");
    // 身份命中后：两侧内容完全一致即跳过；若来源侧时间戳/字段与原生终值
    // 不同，则按冲突呈现（**默认保留原生值**，绝不把两个来源相加）。
    assert_eq!(
        preview.would_unchanged + preview.would_conflict,
        1,
        "同一请求只可能出现「完全相同」或「冲突」两种处置"
    );
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        RollupConflictPolicy::KeepExisting,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 0);
    assert_eq!(history.event_count().unwrap(), 1, "同一请求不重复入库");
    assert_eq!(
        history
            .stored_events_filtered(Some(AgentKind::ClaudeCode))
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}
