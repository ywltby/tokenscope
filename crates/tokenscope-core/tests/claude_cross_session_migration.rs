//! Claude 跨会话复制去重的**旧库纠错**（计划 2026-11-07，C03/C04）。
//!
//! 场景：升级前已经按 `(session, message.id)` 写入了两份事实（副本各一条）。
//! 新版本在正常采集时补齐复制证据（`requestId` + `uuid`），必须把两条旧事实
//! 合成一条：来源关联、旧会话别名、CCS 请求别名全部保留，重复导入/重扫/
//! 重启都不再复活重复；证据不足的旧行原样保留。
//!
//! 全部使用合成数据与独立临时目录：不读写真实 history.db、CCS 库或会话日志。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::HistoryDb;
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::query;
use tokenscope::report::{
    EventFilter, PARSE_RULES_REVISION, SummaryOptions, SummaryReport, summary,
};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-claude-migrate-{tag}-{}-{n}-{}",
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
        by: GroupBy::Day,
        claude_dir: Some(root.to_path_buf()),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 一行 assistant 用量。`with_evidence = false` 时模拟"旧版本写库时的观测"：
/// 日志里没有可用的复制证据（或旧解析器不读它），因此只能按会话内身份入账。
fn assistant_line(
    session: &str,
    message_id: &str,
    ts: &str,
    input: u64,
    output: u64,
    with_evidence: bool,
) -> String {
    let mut obj = serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "message": {
            "id": message_id,
            "model": "claude-opus-5-5",
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_creation_input_tokens": 3,
                "cache_read_input_tokens": 4,
            }
        }
    });
    if with_evidence {
        obj["requestId"] = serde_json::Value::String("req_01".to_string());
        obj["uuid"] = serde_json::Value::String("uuid-01".to_string());
    }
    obj.to_string()
}

fn write_session(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn report(dir: &Path, root: &Path) -> SummaryReport {
    summary(&opts(dir, root)).unwrap()
}

fn history_path(dir: &Path) -> PathBuf {
    dir.join("data").join("history.db")
}

/// 库里"两条事实"的旧状态：副本各一条，且没有跨会话复制证据。
fn seed_old_duplicates(dir: &Path, root: &Path) {
    write_session(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            false,
        )],
    );
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            false,
        )],
    );
    let r = report(dir, root);
    assert_eq!(r.totals.requests, 2, "旧实现：副本各自成为一条事实");
    assert_eq!(r.totals.tokens.total(), 38);
}

fn correction_backups(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir.join("data"))
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            let hit = p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.contains("correction-backup"));
            hit.then_some(p)
        })
        .collect();
    out.sort();
    out
}

/// (别名 scheme, 别名值, 指向的事件 id)
fn alias_rows(dir: &Path) -> Vec<(String, String, i64)> {
    let conn = rusqlite::Connection::open(history_path(dir)).unwrap();
    let mut stmt = conn
        .prepare("SELECT identity_scheme, identity_value, event_id FROM event_aliases ORDER BY id")
        .unwrap();
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn origin_rows(dir: &Path) -> Vec<(String, i64)> {
    let conn = rusqlite::Connection::open(history_path(dir)).unwrap();
    let mut stmt = conn
        .prepare("SELECT origin_kind, event_id FROM event_origins ORDER BY id")
        .unwrap();
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

/// 旧库双份事实在补齐证据后合并；来源、别名、备份齐全。
#[test]
fn old_duplicate_facts_merge_with_evidence_and_keep_links() {
    let dir = temp_dir("merge");
    let root = dir.join("projects");
    seed_old_duplicates(&dir, &root);
    let before = alias_rows(&dir);
    assert_eq!(
        before
            .iter()
            .filter(|(s, _, _)| s == "claude-message")
            .count(),
        2,
        "旧库只有两条会话内身份，没有复制证据"
    );
    assert!(
        !before.iter().any(|(s, _, _)| s == "claude-copy"),
        "旧版本从未登记复制证据"
    );

    // 补齐证据（真实升级时由解析修订触发重新解析；这里直接重写同名文件）。
    write_session(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    let after = report(&dir, &root);
    assert_eq!(after.totals.requests, 1, "旧重复事实合并为一条");
    assert_eq!(after.totals.tokens.total(), 19, "不逐字段取最大拼造用量");
    assert_eq!(after.totals.tokens.input, 10);
    assert_eq!(after.totals.tokens.output, 2);

    // 事实只剩一条；两个会话的别名、两条来源都指向它。
    let history = HistoryDb::open(&history_path(&dir)).unwrap();
    assert_eq!(history.event_count().unwrap(), 1);
    let keeper = alias_rows(&dir)
        .into_iter()
        .find(|(s, _, _)| s == "claude-message")
        .map(|(_, _, id)| id)
        .expect("保留会话内身份");
    let aliases = alias_rows(&dir);
    assert!(
        aliases
            .iter()
            .any(|(s, v, id)| s == "claude-message" && v == "s-a|msg_01" && *id == keeper)
    );
    assert!(
        aliases
            .iter()
            .any(|(s, v, id)| s == "claude-message" && v == "s-b|msg_01" && *id == keeper),
        "旧会话别名必须转移到代表，后续重导才不会再造重复"
    );
    assert!(
        aliases
            .iter()
            .any(|(s, _, id)| s == "claude-copy" && *id == keeper),
        "复制证据落在代表上"
    );
    let origins = origin_rows(&dir);
    assert_eq!(origins.len(), 2, "两条来源关联都保留");
    assert!(
        origins
            .iter()
            .all(|(kind, id)| kind == "native_file" && *id == keeper)
    );

    // 纠错前备份：独立文件、可打开、与升级备份分开命名。
    let backups = correction_backups(&dir);
    assert_eq!(backups.len(), 1, "纠错前生成一份可恢复备份");
    let conn = rusqlite::Connection::open(&backups[0]).unwrap();
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2, "备份保存的是纠错前的两条事实");

    // generation 已前进；重复采集幂等。
    let generation = history.generation().unwrap();
    assert!(generation > 0);
    let again = report(&dir, &root);
    assert_eq!(again.totals.requests, 1);
    assert_eq!(
        history.generation().unwrap(),
        generation,
        "重复采集不改变事实"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 合并后删除其中一个来源文件、再放回同样内容：历史保留且不复活重复。
#[test]
fn merged_fact_survives_source_deletion_without_resurrection() {
    let dir = temp_dir("delete");
    let root = dir.join("projects");
    seed_old_duplicates(&dir, &root);
    let file_a = root.join("proj-a").join("sess-a.jsonl");
    let file_b = root.join("proj-b").join("sess-b.jsonl");
    write_session(
        &file_a,
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    write_session(
        &file_b,
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    assert_eq!(report(&dir, &root).totals.requests, 1);

    // 删除副本来源：用量保留（来源删除不删事实）。
    std::fs::remove_file(&file_b).unwrap();
    let after_delete = report(&dir, &root);
    assert_eq!(after_delete.totals.requests, 1);
    assert_eq!(after_delete.totals.tokens.total(), 19);

    // 放回同一份副本：来源位置已挂在代表上，不得复活第二条事实。
    write_session(
        &file_b,
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    let after_restore = report(&dir, &root);
    assert_eq!(after_restore.totals.requests, 1, "同一副本重扫不复活重复");
    assert_eq!(after_restore.totals.tokens.total(), 19);
    std::fs::remove_dir_all(&dir).ok();
}

/// 纠错事务中途失败：整批回滚，不留半合并。
#[test]
fn correction_failure_rolls_back_whole_batch() {
    let dir = temp_dir("rollback");
    let root = dir.join("projects");
    seed_old_duplicates(&dir, &root);
    write_session(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );

    // 直接驱动写入内核：第一条携带完整证据（会触发合并），第二条非法。
    let history = HistoryDb::open(&history_path(&dir)).unwrap();
    let good = tokenscope::history::EventWrite {
        event_key: "native|claude-code|C:/p/a.jsonl|0|deadbeef".to_string(),
        app: tokenscope::model::AgentKind::ClaudeCode,
        ts: "2026-09-15T08:00:00Z".parse().unwrap(),
        model_raw: "claude-opus-5-5".to_string(),
        model_identity: "claudeopus55".to_string(),
        session_id: Some("s-a".to_string()),
        record_id: Some("msg_01".to_string()),
        project_key: Some("proj-a".to_string()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: tokenscope::model::TokenCounts {
            input: 11,
            output: 2,
            cache_write: 3,
            cache_read: 4,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        },
        precedence: tokenscope::history::WritePrecedence::NativeLog,
        observed_at: "2026-09-15T08:00:00Z".parse().unwrap(),
        aliases: vec![
            tokenscope::history::EventAlias {
                app: tokenscope::model::AgentKind::ClaudeCode,
                scheme: "claude-message".to_string(),
                value: "s-a|msg_01".to_string(),
            },
            tokenscope::history::EventAlias {
                app: tokenscope::model::AgentKind::ClaudeCode,
                scheme: "claude-message".to_string(),
                value: "s-b|msg_01".to_string(),
            },
            tokenscope::history::EventAlias {
                app: tokenscope::model::AgentKind::ClaudeCode,
                scheme: "claude-copy".to_string(),
                value: "msg_01|req_01|uuid-01|00ff".to_string(),
            },
        ],
        origins: vec![
            tokenscope::history::EventOrigin {
                origin_kind: "native_file".to_string(),
                origin_key: "native|claude-code|C:/p/a.jsonl|0|deadbeef".to_string(),
                app: tokenscope::model::AgentKind::ClaudeCode,
                import_run_id: None,
                parser_revision: 3,
                source_model_raw: None,
                pricing_model: None,
                source_cost_usd: None,
                ts_precision_seconds: false,
            },
            tokenscope::history::EventOrigin {
                origin_kind: "native_file".to_string(),
                origin_key: "native|claude-code|C:/p/b.jsonl|0|deadbeef".to_string(),
                app: tokenscope::model::AgentKind::ClaudeCode,
                import_run_id: None,
                parser_revision: 3,
                source_model_raw: None,
                pricing_model: None,
                source_cost_usd: None,
                ts_precision_seconds: false,
            },
        ],
    };
    let mut bad = good.clone();
    bad.event_key = "native|claude-code|C:/p/c.jsonl|0|deadbeef".to_string();
    bad.model_raw = String::new();

    let err = history.write_collected_batch(&[good, bad]).unwrap_err();
    assert!(err.to_string().contains("模型名不得为空"), "{err}");
    assert_eq!(
        history.event_count().unwrap(),
        2,
        "整批回滚：没有半合并、没有新增事实"
    );
    let aliases = alias_rows(&dir);
    assert!(
        !aliases.iter().any(|(s, _, _)| s == "claude-copy"),
        "失败批次不留下任何证据行"
    );

    // 备份已生成（纠错前的可恢复副本），但事实未被改动。
    assert_eq!(correction_backups(&dir).len(), 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// CCS 导入在合并前/后都不产生重复：CCS 请求别名随事实转移，重导命中代表。
#[test]
fn ccs_reimport_after_merge_does_not_resurrect_duplicates() {
    let dir = temp_dir("ccs");
    let root = dir.join("projects");
    seed_old_duplicates(&dir, &root);

    // CCS 侧只有副本会话（s-b）的请求行：`session:<message.id>` 是原生别名。
    {
        let db = dir.join("cc-switch.db");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(CCS_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model,
                request_model, input_tokens, output_tokens, cache_read_tokens,
                cache_creation_tokens, total_cost_usd, status_code, session_id, created_at,
                data_source, pricing_model, input_token_semantics)
             VALUES('session:msg_01', 'p1', 'claude', 'claude-opus-5-5', 'claude-opus-5-5',
                    10, 2, 4, 3, '0.5', 200, 's-b', 1789545600, 'session_log',
                    'claude-opus-5-5', 0)",
            [],
        )
        .unwrap();
    }
    let import = |dir: &Path| {
        let history = HistoryDb::open(&history_path(dir)).unwrap();
        let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
        let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
        ccs::commit(
            &preview.plan_id,
            &history,
            tokenscope::history::RollupConflictPolicy::KeepExisting,
            true,
        )
        .unwrap()
    };
    let first = import(&dir);
    assert_eq!(
        first.requests_inserted, 0,
        "CCS 请求命中副本会话的原生身份，不新增消费"
    );

    // 补齐证据后合并：两个会话别名与 CCS 来源都转移到代表。
    write_session(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    assert_eq!(report(&dir, &root).totals.requests, 1);
    assert_eq!(report(&dir, &root).totals.tokens.total(), 19);

    // 合并后再次导入：命中代表，不复活重复。
    let again = import(&dir);
    assert_eq!(again.requests_inserted, 0, "合并后重导不新增事实");
    let after = report(&dir, &root);
    assert_eq!(after.totals.requests, 1);
    assert_eq!(after.totals.tokens.total(), 19);
    std::fs::remove_dir_all(&dir).ok();
}

/// **未变化的**来源文件也必须按新解析修订重新解析，补齐复制证据并合并。
///
/// 这是真实升级路径：日志字节早已包含 `requestId`/`uuid`，旧解析器不理它们，
/// 旧库因此是"两条事实 + `5|…` 指纹"。只升常量不重解析，证据永远补不上。
#[test]
fn unchanged_sources_are_reparsed_to_supply_evidence() {
    let dir = temp_dir("reparse");
    let root = dir.join("projects");
    for (session, path) in [("s-a", "proj-a"), ("s-b", "proj-b")] {
        write_session(
            &root.join(path).join(format!("sess-{session}.jsonl")),
            &[assistant_line(
                session,
                "msg_01",
                "2026-09-15T08:00:00.000Z",
                10,
                2,
                true,
            )],
        );
    }
    assert_eq!(report(&dir, &root).totals.requests, 1);

    // 还原"旧版本写库"的形态：两条事实、没有复制证据、指纹属于上一版规则。
    {
        let conn = rusqlite::Connection::open(history_path(&dir)).unwrap();
        let keeper: i64 = conn
            .query_row("SELECT id FROM usage_events", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
                model_identity, identity_revision, session_id, record_id, project_key,
                session_initial_cwd, event_cwd, input_tokens, output_tokens,
                cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                observed_at_seconds, observed_at_nanos, parser_revision,
                cache_write_5m, cache_write_1h)
             SELECT 'native|claude-code|old-proj-b|0|legacy', app, ts_seconds, ts_nanos,
                model_raw, model_identity, identity_revision, 's-b', record_id, 'proj-b',
                session_initial_cwd, event_cwd, input_tokens, output_tokens,
                cache_write_tokens, cache_read_tokens, origin_rank, observed_at_utc,
                observed_at_seconds, observed_at_nanos, parser_revision,
                cache_write_5m, cache_write_1h
             FROM usage_events WHERE id = ?1",
            [keeper],
        )
        .unwrap();
        let duplicate = conn.last_insert_rowid();
        conn.execute(
            "UPDATE event_origins SET event_id = ?1 WHERE origin_key LIKE '%proj-b%'",
            [duplicate],
        )
        .unwrap();
        conn.execute(
            "UPDATE event_aliases SET event_id = ?1 WHERE identity_value = 's-b|msg_01'",
            [duplicate],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM event_aliases WHERE identity_scheme = 'claude-copy'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE source_files SET context_revision = '5|' || substr(context_revision,
                instr(context_revision, '|') + 1)",
            [],
        )
        .unwrap();
    }
    let old = HistoryDb::open(&history_path(&dir)).unwrap();
    assert_eq!(old.event_count().unwrap(), 2, "旧形态：两条事实");
    assert_eq!(old.totals().unwrap().total(), 38);

    // 文件字节与指纹都没变；只有解析修订变了。
    let observations_before: Vec<u64> = old
        .source_files()
        .unwrap()
        .iter()
        .map(|f| f.observations)
        .collect();
    let fixed = report(&dir, &root);
    assert_eq!(fixed.totals.requests, 1, "未变化文件重解析后补齐证据并合并");
    assert_eq!(fixed.totals.tokens.total(), 19);
    let sources = HistoryDb::open(&history_path(&dir))
        .unwrap()
        .source_files()
        .unwrap();
    assert_eq!(sources.len(), 2);
    for (i, f) in sources.iter().enumerate() {
        assert_eq!(
            f.observations,
            observations_before[i] + 1,
            "指纹命中不等于重解析：这里必须真的重新解析了一次"
        );
        assert!(
            f.context_revision
                .starts_with(&format!("{PARSE_RULES_REVISION}|")),
            "新指纹记录当前解析修订：{}",
            f.context_revision
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 合并前后查询口径一致；更早建立的查询快照保持冻结（不出现半次合并）。
#[test]
fn merged_results_are_consistent_across_queries_and_snapshots() {
    let dir = temp_dir("query");
    let root = dir.join("projects");
    seed_old_duplicates(&dir, &root);

    let frozen = query::begin_query(&opts(&dir, &root)).unwrap();
    let frozen_rows = query::query_events(&frozen.query_id, &EventFilter::default()).unwrap();
    assert_eq!(frozen_rows.total, 2, "纠错前快照看到两条事实");

    write_session(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    let summary = report(&dir, &root);
    assert_eq!(summary.totals.requests, 1);

    // 旧快照冻结：仍是两条。
    let frozen_again = query::query_events(&frozen.query_id, &EventFilter::default()).unwrap();
    assert_eq!(frozen_again.total, 2, "旧查询快照不得看到半次合并");
    assert_eq!(frozen_again.rows.len(), 2);

    // 新快照与普通汇总同源一致。
    let fresh = query::begin_query(&opts(&dir, &root)).unwrap();
    let fresh_rows = query::query_events(&fresh.query_id, &EventFilter::default()).unwrap();
    assert_eq!(fresh_rows.total, 1);
    assert_eq!(fresh_rows.rows.len(), 1);
    let queried = query::query_summary(&fresh.query_id).unwrap();
    assert_eq!(queried.totals.requests, summary.totals.requests);
    assert_eq!(queried.totals.tokens, summary.totals.tokens);

    // 按项目分组：合并后只落在一个项目下（代表规则固定）。
    let by_project = summary_with(&dir, &root, GroupBy::Project);
    let project_keys: Vec<&str> = by_project
        .groups
        .iter()
        .map(|g| g.key.as_str())
        .filter(|k| *k != "合计")
        .collect();
    assert_eq!(
        project_keys,
        vec!["proj-a"],
        "同一请求只出现在一个项目分组（代表规则固定）"
    );
    assert_eq!(by_project.totals.tokens.total(), 19);
    std::fs::remove_dir_all(&dir).ok();
}

fn summary_with(dir: &Path, root: &Path, by: GroupBy) -> SummaryReport {
    let mut o = opts(dir, root);
    o.by = by;
    summary(&o).unwrap()
}

/// 一条会话内的流式多条观察（同 message.id，不同 uuid）仍只算一次请求；
/// 复制到另一会话时按终值观察合并。
#[test]
fn streaming_observations_count_once_per_request() {
    let dir = temp_dir("stream");
    let root = dir.join("projects");
    let a = [
        assistant_line_with(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            5,
            1,
            "req",
            "u1",
        ),
        assistant_line_with(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.500Z",
            10,
            2,
            "req",
            "u2",
        ),
    ];
    write_session(&root.join("proj-a").join("sess-a.jsonl"), &a);
    // 复制会话只抄到终值观察（终值一致 → 有证据）。
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line_with(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.500Z",
            10,
            2,
            "req",
            "u2",
        )],
    );
    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "流式观察不是多个请求");
    assert_eq!(r.totals.tokens.total(), 19, "终值 10/2/3/4");
    std::fs::remove_dir_all(&dir).ok();
}

fn assistant_line_with(
    session: &str,
    message_id: &str,
    ts: &str,
    input: u64,
    output: u64,
    request_id: &str,
    uuid: &str,
) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "requestId": format!("{request_id}-{message_id}"),
        "uuid": format!("{uuid}-{message_id}"),
        "message": {
            "id": message_id,
            "model": "claude-opus-5-5",
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_creation_input_tokens": 3,
                "cache_read_input_tokens": 4,
            }
        }
    })
    .to_string()
}

/// 三会话桥接：A 与 B 共享同一观察，C 的同一标识内容矛盾 → 只合并 A/B，
/// 绝不因桥接把矛盾的 C 一并吞掉。
#[test]
fn bridge_conflict_does_not_merge_contradictory_facts() {
    let dir = temp_dir("bridge");
    let root = dir.join("projects");
    write_session(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[assistant_line(
            "s-a",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    write_session(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[assistant_line(
            "s-b",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            10,
            2,
            true,
        )],
    );
    // C 使用同一 requestId/uuid，但四桶不同：内容矛盾，不得并入。
    write_session(
        &root.join("proj-c").join("sess-c.jsonl"),
        &[assistant_line(
            "s-c",
            "msg_01",
            "2026-09-15T08:00:00.000Z",
            99,
            9,
            true,
        )],
    );
    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 2, "A/B 合并，矛盾的 C 保留为独立事实");
    assert_eq!(r.totals.tokens.total(), 19 + (99 + 9 + 3 + 4));
    std::fs::remove_dir_all(&dir).ok();
}

const CCS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS proxy_request_logs (
    request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL,
    model TEXT NOT NULL, request_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', status_code INTEGER NOT NULL DEFAULT 200,
    session_id TEXT, created_at INTEGER NOT NULL, data_source TEXT NOT NULL DEFAULT 'proxy',
    pricing_model TEXT, input_token_semantics INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS usage_daily_rollups (
    date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL, model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '', pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0, success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
";
