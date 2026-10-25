//! CP04：规则升级必须重解析内容未变的旧文件，补用量且保留原事实。
//! 所有来源、历史库与价格文件均限定在本测试创建的临时目录。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use tokenscope::aggregate::GroupBy;
use tokenscope::cache::mtime_ms;
use tokenscope::history::{
    EventAlias, EventOrigin, EventWrite, HistoryDb, ORIGIN_KIND_NATIVE_FILE, PARSER_REVISION,
    SourceFileKey, SourceFileRecord, StoredEvent, WritePrecedence,
};
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::report::{CollectedView, SummaryOptions, collect_all_with_sources_for_test};
use tokenscope::source::codex::CodexSource;
use tokenscope::source::{CollectStats, DirVisitLedger, FileParse, Source};

const SESSION: &str = "synthetic-cp04-session";
const MODEL: &str = "gpt-5.6-sol";
const PROJECT: &str = "C:/synthetic/cp04-project";
const NORMAL_TS: &str = "2026-10-22T08:00:02Z";
const COMPACTION_TS: &str = "2026-10-22T08:00:03Z";

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        // Windows 时钟分辨率内并发用例可能同刻取名；序号保证进程内隔离。
        static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "tokenscope-codex-parse-revision-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        // 仅清理本测试成功创建的唯一临时目录；失败断言也执行清理。
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct CountingCodexSource {
    inner: CodexSource,
    parse_calls: Arc<AtomicUsize>,
}

impl Source for CountingCodexSource {
    fn agent(&self) -> AgentKind {
        self.inner.agent()
    }

    fn root(&self) -> &Path {
        self.inner.root()
    }

    fn roots(&self) -> Vec<PathBuf> {
        self.inner.roots()
    }

    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        self.inner.discover_with_errors()
    }

    fn discover_with_ledger(&self, ledger: &mut DirVisitLedger) -> (Vec<PathBuf>, Vec<String>) {
        self.inner.discover_with_ledger(ledger)
    }

    fn parse_file(&self, path: &Path) -> FileParse {
        self.parse_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.parse_file(path)
    }

    fn context_revision(&self) -> &str {
        self.inner.context_revision()
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FileSnapshot {
    bytes: Vec<u8>,
    len: u64,
    modified: SystemTime,
    mtime_ms: i64,
}

fn file_snapshot(path: &Path) -> FileSnapshot {
    let metadata = std::fs::metadata(path).unwrap();
    FileSnapshot {
        bytes: std::fs::read(path).unwrap(),
        len: metadata.len(),
        modified: metadata.modified().unwrap(),
        mtime_ms: mtime_ms(path).unwrap(),
    }
}

// 旧版登记键采用 canonicalize 后剥离 Windows verbatim 前缀并小写。
// 来源根与文件在这里都真实存在，不需要复制生产路径函数的缺失路径分支。
fn registered_path_key(path: &Path) -> String {
    let canonical = path.canonicalize().unwrap();
    let text = canonical.to_string_lossy();
    let normalized = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        text.into_owned()
    };
    normalized.to_lowercase()
}

fn native_key(event: &UsageEvent) -> String {
    format!(
        "native|{}|{}|{}|{}",
        event.agent.as_str(),
        event.source_path,
        event.line,
        event.content_fingerprint()
    )
}

fn legacy_normal_write(event: &UsageEvent) -> EventWrite {
    let key = native_key(event);
    let (scheme, value) = event.same_source_identity().unwrap();
    EventWrite {
        event_key: key.clone(),
        app: event.agent,
        ts: event.ts,
        model_raw: event.model.clone(),
        model_identity: ModelIdentity::parse(&event.model).identity_key(),
        session_id: Some(event.session_id.clone()),
        record_id: None,
        project_key: Some(event.project.clone()),
        session_initial_cwd: event.session_initial_cwd.clone(),
        event_cwd: event.event_cwd.clone(),
        tokens: TokenCounts {
            input: event.input_tokens,
            output: event.output_tokens,
            cache_write: event.cache_write_tokens,
            cache_read: event.cache_read_tokens,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: event.ts,
        aliases: vec![EventAlias {
            app: event.agent,
            scheme: scheme.to_string(),
            value,
        }],
        origins: vec![EventOrigin {
            origin_kind: ORIGIN_KIND_NATIVE_FILE.to_string(),
            origin_key: key,
            app: event.agent,
            import_run_id: None,
            parser_revision: 1,
            source_model_raw: None,
            pricing_model: None,
            source_cost_usd: None,
            ts_precision_seconds: false,
        }],
    }
}

fn sql_rows(connection: &Connection, sql: &str, event_id: i64) -> Vec<Vec<Value>> {
    let mut statement = connection.prepare(sql).unwrap();
    let columns = statement.column_count();
    statement
        .query_map([event_id], |row| {
            (0..columns)
                .map(|column| row.get::<_, Value>(column))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

#[derive(Debug, PartialEq)]
struct FactRows {
    event: Vec<Vec<Value>>,
    origins: Vec<Vec<Value>>,
    aliases: Vec<Vec<Value>>,
}

fn fact_rows(path: &Path, event_id: i64) -> FactRows {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    FactRows {
        event: sql_rows(
            &connection,
            "SELECT * FROM usage_events WHERE id = ?1 ORDER BY id",
            event_id,
        ),
        origins: sql_rows(
            &connection,
            "SELECT * FROM event_origins WHERE event_id = ?1 ORDER BY id",
            event_id,
        ),
        aliases: sql_rows(
            &connection,
            "SELECT * FROM event_aliases WHERE event_id = ?1 ORDER BY id",
            event_id,
        ),
    }
}

#[test]
fn v2_fingerprint_reparses_unchanged_codex_file_once_and_keeps_old_fact() {
    let temp = TempRoot::new();
    let root = temp.0.join("sessions");
    let data = temp.0.join("data");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&data).unwrap();
    let file = root.join("rollout-cp04.jsonl");
    let normal_usage = json!({
        "input_tokens": 100, "output_tokens": 10, "cached_input_tokens": 20,
        "cache_write_input_tokens": 0, "total_tokens": 110,
    });
    let compaction_usage = json!({
        "input_tokens": 200, "output_tokens": 4, "cached_input_tokens": 100,
        "cache_write_input_tokens": 0, "total_tokens": 204,
    });
    let lines = [
        json!({"type":"session_meta", "timestamp":"2026-10-22T08:00:00Z",
            "payload":{"id":SESSION, "session_id":SESSION, "cwd":PROJECT}}),
        json!({"type":"turn_context", "timestamp":"2026-10-22T08:00:00Z",
            "payload":{"model":MODEL, "cwd":PROJECT}}),
        json!({"type":"token_usage_record", "timestamp":"2026-10-22T08:00:01Z",
            "payload":{"response_id":"synthetic-normal", "usage":normal_usage}}),
        json!({"type":"event_msg", "timestamp":NORMAL_TS,
            "payload":{"type":"token_count", "info":{"last_token_usage":normal_usage}}}),
        json!({"type":"token_usage_record", "timestamp":COMPACTION_TS,
            "payload":{"response_id":"synthetic-compaction", "usage":compaction_usage}}),
        json!({"type":"event_msg", "timestamp":"2026-10-22T08:00:04Z",
        "payload":{"type":"token_count", "info":{
            "last_token_usage":{"input_tokens":0, "output_tokens":0,
                "cached_input_tokens":0, "cache_write_input_tokens":0, "total_tokens":0},
            "total_token_usage":{"input_tokens":300, "output_tokens":14,
                "cached_input_tokens":120, "cache_write_input_tokens":0, "total_tokens":314}
        }}}),
    ];
    let text = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, text).unwrap();
    let source_before = file_snapshot(&file);

    // 精确预置 v2 会保存的正常 token_count：0-based 第 3 行；压缩尚未入库。
    // 不用新解析器的输出构造旧基线，以免新解析器的错误同步污染预期。
    let normal = UsageEvent {
        ts: NORMAL_TS.parse().unwrap(),
        agent: AgentKind::Codex,
        model: MODEL.to_string(),
        session_id: SESSION.to_string(),
        project: PROJECT.to_string(),
        session_initial_cwd: Some(PROJECT.to_string()),
        event_cwd: Some(PROJECT.to_string()),
        record_id: String::new(),
        line: 3,
        source_path: file.display().to_string(),
        input_tokens: 80,
        output_tokens: 10,
        cache_write_tokens: 0,
        cache_read_tokens: 20,
    };
    let compaction = UsageEvent {
        ts: COMPACTION_TS.parse().unwrap(),
        line: 4,
        input_tokens: 100,
        output_tokens: 4,
        cache_read_tokens: 100,
        ..normal.clone()
    };
    let history_path = data.join("history.db");
    let history = HistoryDb::open(&history_path).unwrap();
    let seeded = history
        .write_batch(&[legacy_normal_write(&normal)])
        .unwrap();
    assert_eq!(seeded.inserted, 1);
    assert_eq!(seeded.aliases_added, 1);
    assert_eq!(seeded.origins_added, 1);
    history.set_meta("parser_revision", "1").unwrap();
    assert!(CodexSource::new(root.clone()).context_revision().is_empty());
    history
        .touch_source_file(&SourceFileRecord {
            key: SourceFileKey {
                app: AgentKind::Codex,
                root: registered_path_key(&root),
                path: registered_path_key(&file),
            },
            fingerprint: format!("{}:{}", source_before.len, source_before.mtime_ms),
            size_bytes: source_before.len,
            mtime_ms: source_before.mtime_ms,
            context_revision: "2|".to_string(),
            last_success_utc: Some(NORMAL_TS.to_string()),
            stats: CollectStats {
                files_scanned: 1,
                lines_seen: 6,
                skipped_zero_usage: 1,
                ignored_token_usage_record: 2,
                events: 1,
                ..Default::default()
            },
        })
        .unwrap();
    let old_id = history.stored_events().unwrap()[0].id;
    let generation_before = history.generation().unwrap();
    let schema_before = history.schema_version().unwrap();
    drop(history);

    // EventWrite 没有旧版本覆盖接口；仅 fixture 的这一列需 SQL 设置为旧值。
    // 来源版本已通过公开 EventOrigin API 置 1；别名表本身没有 parser 列。
    let connection = Connection::open(&history_path).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE usage_events SET parser_revision = 1 WHERE id = ?1",
                [old_id]
            )
            .unwrap(),
        1
    );
    drop(connection);
    let old_fact = fact_rows(&history_path, old_id);
    assert_eq!(old_fact.event.len(), 1);
    assert_eq!(old_fact.origins.len(), 1);
    assert_eq!(old_fact.aliases.len(), 1);
    let history = HistoryDb::open(&history_path).unwrap();
    let old_event = history.stored_events().unwrap().remove(0);
    assert_eq!(old_event.parser_revision, 1);
    let old_source = history.source_files().unwrap().remove(0);
    assert_eq!(old_source.context_revision, "2|");
    assert_eq!(old_source.state, "present");
    assert_eq!(file_snapshot(&file), source_before);
    drop(history);

    let opts = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Agent,
        agent: Some(AgentKind::Codex),
        claude_dir: Some(temp.0.join("disabled-claude")),
        codex_dir: Some(root.clone()),
        claude_enabled: Some(false),
        codex_enabled: Some(true),
        cache_dir: Some(data.clone()),
        history_path: Some(history_path.clone()),
        pricing_path: Some(data.join("pricing.toml")),
        modelsdev_path: Some(data.join("pricing-modelsdev.json")),
        openrouter_path: Some(data.join("pricing-openrouter.json")),
        pricing_index: Some(data.join("pricing-index.json")),
        claude_projects_path: Some(temp.0.join("disabled-claude-projects.json")),
        tz: Some("UTC".to_string()),
        refresh: false,
        ..Default::default()
    };
    let parse_calls = Arc::new(AtomicUsize::new(0));
    let sources = || {
        vec![Box::new(CountingCodexSource {
            inner: CodexSource::new(root.clone()),
            parse_calls: parse_calls.clone(),
        }) as Box<dyn Source>]
    };
    let first = collect_all_with_sources_for_test(sources(), &opts).unwrap();
    // PARSE_RULES_REVISION 仍为 2 时，这里应因暖命中而缺少压缩事件，断言红。
    assert_eq!(
        first.events.len(),
        2,
        "内容未变的 v2 登记文件也必须补回压缩事件；真实 parse 调用数={}",
        parse_calls.load(Ordering::Relaxed)
    );
    assert_eq!(parse_calls.load(Ordering::Relaxed), 1);
    assert_eq!(file_snapshot(&file), source_before);
    assert_eq!(fact_rows(&history_path, old_id), old_fact);

    let history = HistoryDb::open(&history_path).unwrap();
    assert_eq!(history.schema_version().unwrap(), schema_before);
    assert_eq!(
        history.meta("parser_revision").unwrap().as_deref(),
        Some("1")
    );
    let after_events = history.stored_events().unwrap();
    assert_eq!(after_events.len(), 2);
    assert_eq!(
        after_events.iter().find(|e| e.id == old_id).unwrap(),
        &old_event
    );
    let added = after_events.iter().find(|e| e.id != old_id).unwrap();
    assert_eq!(added.event_key, native_key(&compaction));
    assert_eq!(added.ts, compaction.ts);
    assert_eq!(added.model_raw, MODEL);
    assert_eq!(added.session_id.as_deref(), Some(SESSION));
    assert_eq!(added.project_key.as_deref(), Some(PROJECT));
    assert_eq!(added.session_initial_cwd.as_deref(), Some(PROJECT));
    assert_eq!(added.event_cwd.as_deref(), Some(PROJECT));
    assert_eq!(added.parser_revision, PARSER_REVISION);
    assert_eq!(
        added.tokens,
        TokenCounts {
            input: 100,
            output: 4,
            cache_write: 0,
            cache_read: 100
        }
    );
    assert_eq!(
        history.totals().unwrap(),
        TokenCounts {
            input: 180,
            output: 14,
            cache_write: 0,
            cache_read: 120
        }
    );
    let upgraded_sources = history.source_files().unwrap();
    assert_eq!(upgraded_sources.len(), 1);
    let upgraded = &upgraded_sources[0];
    assert_eq!(upgraded.key, old_source.key);
    assert_eq!(upgraded.fingerprint, old_source.fingerprint);
    assert_eq!(upgraded.size_bytes, old_source.size_bytes);
    assert_eq!(upgraded.mtime_ms, old_source.mtime_ms);
    assert_eq!(upgraded.context_revision, "3|");
    assert_eq!(upgraded.state, "present");
    assert_eq!(upgraded.stats.events, 2);
    assert_eq!(upgraded.observations, old_source.observations + 1);
    let generation_after = history.generation().unwrap();
    assert_eq!(generation_after, generation_before + 1);
    let connection =
        Connection::open_with_flags(&history_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let added_origin_revision: i64 = connection
        .query_row(
            "SELECT parser_revision FROM event_origins WHERE event_id = ?1",
            [added.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(added_origin_revision, PARSER_REVISION);
    drop(connection);
    drop(history);

    // 第二次按新指纹暖命中：不重解析、不复制事件、不更新旧事实、不递增代次。
    let second = collect_all_with_sources_for_test(sources(), &opts).unwrap();
    assert_eq!(second.events.len(), 2);
    assert_eq!(parse_calls.load(Ordering::Relaxed), 1);
    assert_eq!(file_snapshot(&file), source_before);
    assert_eq!(fact_rows(&history_path, old_id), old_fact);
    let history = HistoryDb::open(&history_path).unwrap();
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(history.stored_events().unwrap(), after_events);
    assert_eq!(history.generation().unwrap(), generation_after);
    assert_eq!(history.source_files().unwrap(), upgraded_sources);
}

struct ModelUpgradeFixture {
    temp: TempRoot,
    root: PathBuf,
    file: PathBuf,
    history_path: PathBuf,
    source_before: FileSnapshot,
    old: UsageEvent,
    old_stored: StoredEvent,
    old_fact: FactRows,
    generation: u64,
    opts: SummaryOptions,
}

impl ModelUpgradeFixture {
    fn collect_from(&self, root: &Path, refresh: bool) -> CollectedView {
        let opts = SummaryOptions {
            workbuddy_enabled: Some(false),
            workbuddy_ai_enabled: Some(false),

            codex_dir: Some(root.to_path_buf()),
            refresh,
            ..self.opts.clone()
        };
        collect_all_with_sources_for_test(vec![Box::new(CodexSource::new(root))], &opts).unwrap()
    }

    fn collect(&self) -> CollectedView {
        self.collect_from(&self.root, false)
    }
}

fn model_upgrade_fixture(parser_revision: i64) -> ModelUpgradeFixture {
    let temp = TempRoot::new();
    let root = temp.0.join("sessions");
    let data = temp.0.join("data");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&data).unwrap();
    let file = root.join("rollout-model-upgrade.jsonl");
    let lines = [
        json!({"type":"session_meta","payload":{"id":SESSION,"cwd":PROJECT}}),
        json!({"type":"turn_context","payload":{"model":"model-a","cwd":PROJECT}}),
        json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"model-b"}}}),
        json!({"type":"event_msg","timestamp":NORMAL_TS,"payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"cached_input_tokens":20,"total_tokens":110}}}}),
    ];
    std::fs::write(
        &file,
        lines
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let source_before = file_snapshot(&file);
    // v2 忽略 settings，已经保存为 model-a；新解析器应归 model-b，但不能双计。
    let old = UsageEvent {
        ts: NORMAL_TS.parse().unwrap(),
        agent: AgentKind::Codex,
        model: "model-a".to_string(),
        session_id: SESSION.to_string(),
        project: PROJECT.to_string(),
        session_initial_cwd: Some(PROJECT.to_string()),
        event_cwd: Some(PROJECT.to_string()),
        record_id: String::new(),
        line: 3,
        source_path: file.display().to_string(),
        input_tokens: 80,
        output_tokens: 10,
        cache_write_tokens: 0,
        cache_read_tokens: 20,
    };
    let history_path = data.join("history.db");
    let history = HistoryDb::open(&history_path).unwrap();
    let mut write = legacy_normal_write(&old);
    write.origins[0].parser_revision = parser_revision;
    history.write_batch(&[write]).unwrap();
    history
        .touch_source_file(&SourceFileRecord {
            key: SourceFileKey {
                app: AgentKind::Codex,
                root: registered_path_key(&root),
                path: registered_path_key(&file),
            },
            fingerprint: format!("{}:{}", source_before.len, source_before.mtime_ms),
            size_bytes: source_before.len,
            mtime_ms: source_before.mtime_ms,
            context_revision: "2|".to_string(),
            last_success_utc: Some(NORMAL_TS.to_string()),
            stats: CollectStats {
                events: 1,
                ..Default::default()
            },
        })
        .unwrap();
    drop(history);
    let conn = Connection::open(&history_path).unwrap();
    conn.execute(
        "UPDATE usage_events SET parser_revision=?1",
        [parser_revision],
    )
    .unwrap();
    drop(conn);
    let history = HistoryDb::open(&history_path).unwrap();
    let old_stored = history.stored_events().unwrap().remove(0);
    let generation = history.generation().unwrap();
    drop(history);
    let old_fact = fact_rows(&history_path, old_stored.id);
    let opts = SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Agent,
        agent: Some(AgentKind::Codex),
        claude_enabled: Some(false),
        codex_enabled: Some(true),
        claude_dir: Some(temp.0.join("no-claude")),
        codex_dir: Some(root.clone()),
        cache_dir: Some(data.clone()),
        history_path: Some(history_path.clone()),
        pricing_index: Some(data.join("index.json")),
        pricing_path: Some(data.join("pricing.toml")),
        openrouter_path: Some(data.join("or.json")),
        modelsdev_path: Some(data.join("md.json")),
        claude_projects_path: Some(temp.0.join("no-claude-projects.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    };
    ModelUpgradeFixture {
        temp,
        root,
        file,
        history_path,
        source_before,
        old,
        old_stored,
        old_fact,
        generation,
        opts,
    }
}

fn assert_model_corrected_without_replacing_fact(fixture: &ModelUpgradeFixture) -> StoredEvent {
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.event_count().unwrap(), 1);
    let actual = history.stored_events().unwrap().remove(0);
    let mut expected = fixture.old_stored.clone();
    expected.model_raw = "model-b".to_string();
    expected.model_identity = ModelIdentity::parse("model-b").identity_key();
    expected.parser_revision = PARSER_REVISION;
    assert_eq!(actual, expected, "仅模型、模型身份和解析版本允许纠正");
    assert_eq!(
        history.totals().unwrap(),
        TokenCounts {
            input: 80,
            output: 10,
            cache_write: 0,
            cache_read: 20,
        }
    );
    drop(history);

    let actual_fact = fact_rows(&fixture.history_path, fixture.old_stored.id);
    let connection =
        Connection::open_with_flags(&fixture.history_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement = connection
        .prepare("PRAGMA table_info(usage_events)")
        .unwrap();
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let mut expected_row = fixture.old_fact.event.clone();
    for (name, value) in [
        ("model_raw", Value::Text("model-b".to_string())),
        (
            "model_identity",
            Value::Text(ModelIdentity::parse("model-b").identity_key()),
        ),
        ("parser_revision", Value::Integer(PARSER_REVISION)),
    ] {
        expected_row[0][columns.iter().position(|column| column == name).unwrap()] = value;
    }
    assert_eq!(
        actual_fact.event, expected_row,
        "完整 SQL 行的其他字段必须原样保留"
    );
    assert_eq!(
        actual_fact.origins, fixture.old_fact.origins,
        "不新增或改写旧原生来源证据"
    );
    assert_eq!(actual_fact.aliases.len(), 2);
    assert!(
        actual_fact.aliases.contains(&fixture.old_fact.aliases[0]),
        "旧别名完整行原样保留"
    );
    let new_model_event = UsageEvent {
        model: "model-b".to_string(),
        ..fixture.old.clone()
    };
    let mut aliases = connection.prepare(
        "SELECT identity_value FROM event_aliases WHERE event_id=?1 AND identity_scheme='codex-usage' ORDER BY identity_value"
    ).unwrap();
    let values = aliases
        .query_map([fixture.old_stored.id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let mut expected_values = vec![
        fixture.old.same_source_identity().unwrap().1,
        new_model_event.same_source_identity().unwrap().1,
    ];
    expected_values.sort();
    assert_eq!(values, expected_values);
    actual
}

#[test]
fn reparsing_thread_settings_model_change_must_not_duplicate_old_usage() {
    let fixture = model_upgrade_fixture(1);
    let collected = fixture.collect();
    assert_eq!(
        collected.events.len(),
        1,
        "同一条旧请求改归模型后不能与旧模型事实同时计入"
    );
    assert_eq!(file_snapshot(&fixture.file), fixture.source_before);
    assert_model_corrected_without_replacing_fact(&fixture);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.generation().unwrap(), fixture.generation + 1);
}

fn model_log(event: &UsageEvent, settings_model: &str) -> String {
    let input = event.input_tokens + event.cache_write_tokens + event.cache_read_tokens;
    [
        json!({"type":"session_meta","payload":{"id":event.session_id,"cwd":event.project}}),
        json!({"type":"turn_context","payload":{"model":event.model,"cwd":event.project}}),
        json!({"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":settings_model}}}),
        json!({"type":"event_msg","timestamp":event.ts.to_string(),"payload":{"type":"token_count","info":{"last_token_usage":{
            "input_tokens":input,"output_tokens":event.output_tokens,"cached_input_tokens":event.cache_read_tokens,
            "cache_write_input_tokens":event.cache_write_tokens,"total_tokens":input+event.output_tokens
        }}}}),
    ].iter().map(ToString::to_string).collect::<Vec<_>>().join("\n")
}

#[test]
fn archived_file_with_changed_path_and_line_corrects_the_same_old_fact() {
    let fixture = model_upgrade_fixture(1);
    let archive = fixture.temp.0.join("archived_sessions");
    std::fs::create_dir(&archive).unwrap();
    let moved = archive.join("renamed-rollout.jsonl");
    std::fs::rename(&fixture.file, &moved).unwrap();
    // 归档后前置空行，明确证明纠正候选不依赖旧文件路径或旧行号。
    let original = std::fs::read_to_string(&moved).unwrap();
    std::fs::write(&moved, format!("\n\n{original}")).unwrap();
    let parsed = CodexSource::new(&archive).parse_file(&moved);
    assert_eq!(parsed.events.len(), 1);
    assert_ne!(parsed.events[0].source_path, fixture.old.source_path);
    assert_ne!(parsed.events[0].line, fixture.old.line);
    assert_eq!(parsed.events[0].model, "model-b");
    assert_eq!(fixture.collect_from(&archive, false).events.len(), 1);
    assert_model_corrected_without_replacing_fact(&fixture);
}

const MODEL_CCS_SCHEMA: &str = r"
CREATE TABLE proxy_request_logs (
    request_id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, app_type TEXT NOT NULL,
    model TEXT NOT NULL, request_model TEXT,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', status_code INTEGER NOT NULL DEFAULT 200,
    session_id TEXT, created_at INTEGER NOT NULL, data_source TEXT NOT NULL DEFAULT 'proxy',
    pricing_model TEXT, input_token_semantics INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE usage_daily_rollups (
    date TEXT NOT NULL, app_type TEXT NOT NULL, provider_id TEXT NOT NULL, model TEXT NOT NULL,
    request_model TEXT NOT NULL DEFAULT '', pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL DEFAULT 0, success_count INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    total_cost_usd TEXT NOT NULL DEFAULT '0', input_token_semantics INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (date, app_type, provider_id, model, request_model, pricing_model)
);
";

#[test]
fn old_model_ccs_alias_import_does_not_duplicate_or_revert_corrected_fact() {
    let fixture = model_upgrade_fixture(1);
    assert_eq!(fixture.collect().events.len(), 1);
    let corrected = assert_model_corrected_without_replacing_fact(&fixture);
    let source_path = fixture.temp.0.join("synthetic-cc-switch.db");
    let connection = Connection::open(&source_path).unwrap();
    connection.execute_batch(MODEL_CCS_SCHEMA).unwrap();
    connection
        .execute(
            "INSERT INTO proxy_request_logs(request_id,provider_id,app_type,model,request_model,
         input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,session_id,
         created_at,data_source,pricing_model,input_token_semantics)
         VALUES('codex_thread:synthetic:usage:old-model','synthetic','codex','model-a','model-a',
         80,10,20,0,?1,?2,'codex_session','model-a',2)",
            rusqlite::params![SESSION, fixture.old.ts.as_second()],
        )
        .unwrap();
    drop(connection);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    let source = CcsSource::open(&source_path).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(preview.requests_importable, 1);
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        tokenscope::history::RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 0);
    assert_eq!(history.event_count().unwrap(), 1);
    assert_eq!(history.totals().unwrap(), corrected.tokens);
    assert_eq!(
        history.stored_events().unwrap()[0],
        corrected,
        "CCS 使用旧 model-a 别名不能把已纠正的模型改回去"
    );
}

#[test]
fn ambiguous_old_native_model_candidates_are_skipped_without_new_usage() {
    let fixture = model_upgrade_fixture(1);
    // 第二条合法旧事实来自另一路径，故来源键不同；不放别名，避免人为别名冲突。
    let duplicate = UsageEvent {
        source_path: fixture
            .temp
            .0
            .join("old-other-root/previous.jsonl")
            .display()
            .to_string(),
        ..fixture.old.clone()
    };
    let mut write = legacy_normal_write(&duplicate);
    write.aliases.clear();
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    history.write_batch(&[write]).unwrap();
    drop(history);
    let connection = Connection::open(&fixture.history_path).unwrap();
    connection
        .execute("UPDATE usage_events SET parser_revision=1", [])
        .unwrap();
    drop(connection);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    let before = history.stored_events().unwrap();
    assert_eq!(before.len(), 2);
    let before_facts: Vec<_> = before
        .iter()
        .map(|event| fact_rows(&fixture.history_path, event.id))
        .collect();
    let totals = history.totals().unwrap();
    let generation = history.generation().unwrap();
    drop(history);
    let collected = fixture.collect();
    assert_eq!(collected.events.len(), 2, "候选不唯一时不能另增 model-b");
    assert_eq!(
        collected
            .warnings
            .iter()
            .filter(|warning| warning.as_str() == "Codex 模型归属纠正候选不唯一：跳过 1 条")
            .count(),
        1
    );
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.stored_events().unwrap(), before);
    assert_eq!(history.totals().unwrap(), totals);
    assert_eq!(history.generation().unwrap(), generation);
    for (event, original) in before.iter().zip(before_facts) {
        assert_eq!(fact_rows(&fixture.history_path, event.id), original);
    }
}

#[test]
fn current_parser_model_is_not_a_correction_candidate_while_old_peer_is_corrected() {
    let fixture = model_upgrade_fixture(PARSER_REVISION);
    let eligible = UsageEvent {
        session_id: "synthetic-eligible-old-session".to_string(),
        ts: COMPACTION_TS.parse().unwrap(),
        source_path: fixture
            .root
            .join("eligible-old.jsonl")
            .display()
            .to_string(),
        ..fixture.old.clone()
    };
    std::fs::write(&eligible.source_path, model_log(&eligible, "model-b")).unwrap();
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    history
        .write_batch(&[legacy_normal_write(&eligible)])
        .unwrap();
    let old_id = history
        .stored_events()
        .unwrap()
        .into_iter()
        .find(|event| event.session_id.as_deref() == Some(eligible.session_id.as_str()))
        .unwrap()
        .id;
    drop(history);
    let connection = Connection::open(&fixture.history_path).unwrap();
    connection
        .execute(
            "UPDATE usage_events SET parser_revision=1 WHERE id=?1",
            [old_id],
        )
        .unwrap();
    drop(connection);
    let collected = fixture.collect();
    // 当前版本的A与新B视作两条事实；另一会话的旧A仅纠正，不插入第四条。
    // 正向纠正同时出现，确保旧实现会红，而不冒称原本成立的保护条件是新红灯。
    assert_eq!(collected.events.len(), 3);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    let stored = history.stored_events().unwrap();
    assert_eq!(stored.len(), 3);
    assert_eq!(
        stored
            .iter()
            .find(|event| event.id == fixture.old_stored.id)
            .unwrap(),
        &fixture.old_stored
    );
    assert_eq!(
        fact_rows(&fixture.history_path, fixture.old_stored.id),
        fixture.old_fact
    );
    let corrected = stored.iter().find(|event| event.id == old_id).unwrap();
    assert_eq!(corrected.model_raw, "model-b");
    assert_eq!(corrected.parser_revision, PARSER_REVISION);
    assert_eq!(corrected.event_key, native_key(&eligible));
    assert_eq!(corrected.ts, eligible.ts);
    assert_eq!(
        corrected.tokens,
        TokenCounts {
            input: 80,
            output: 10,
            cache_write: 0,
            cache_read: 20
        }
    );
    assert_eq!(
        stored
            .iter()
            .filter(|event| event.session_id.as_deref() == Some(SESSION))
            .count(),
        2
    );
}

#[test]
fn replaying_old_native_alias_does_not_restore_the_incorrect_model() {
    let fixture = model_upgrade_fixture(1);
    assert_eq!(fixture.collect().events.len(), 1);
    let corrected = assert_model_corrected_without_replacing_fact(&fixture);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    let generation = history.generation().unwrap();
    drop(history);
    let facts = fact_rows(&fixture.history_path, fixture.old_stored.id);
    // 原生回放再给出A；行号/时间/用量仍与旧别名完全一致。强制重扫避免暖缓存掩盖回退。
    std::fs::write(&fixture.file, model_log(&fixture.old, "model-a")).unwrap();
    let parsed = CodexSource::new(&fixture.root).parse_file(&fixture.file);
    assert_eq!(parsed.events.len(), 1);
    assert_eq!(parsed.events[0].model, "model-a");
    assert_eq!(
        parsed.events[0].same_source_identity(),
        fixture.old.same_source_identity()
    );
    assert_eq!(fixture.collect_from(&fixture.root, true).events.len(), 1);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.stored_events().unwrap(), vec![corrected]);
    assert_eq!(history.generation().unwrap(), generation);
    assert_eq!(
        fact_rows(&fixture.history_path, fixture.old_stored.id),
        facts
    );
}

#[test]
fn occupied_new_model_alias_rejects_collection_and_rolls_back_both_facts() {
    let fixture = model_upgrade_fixture(1);
    let source_path = fixture.temp.0.join("synthetic-new-model-cc-switch.db");
    let connection = Connection::open(&source_path).unwrap();
    connection.execute_batch(MODEL_CCS_SCHEMA).unwrap();
    connection
        .execute(
            "INSERT INTO proxy_request_logs(request_id,provider_id,app_type,model,request_model,
         input_tokens,output_tokens,cache_read_tokens,cache_creation_tokens,session_id,
         created_at,data_source,pricing_model,input_token_semantics)
         VALUES('codex_thread:synthetic:usage:new-model','synthetic','codex','model-b','model-b',
         80,10,20,0,?1,?2,'codex_session','model-b',2)",
            rusqlite::params![SESSION, fixture.old.ts.as_second()],
        )
        .unwrap();
    drop(connection);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    let source = CcsSource::open(&source_path).unwrap();
    let preview = ccs::preview(&source, &history, "UTC").unwrap();
    assert_eq!(preview.requests_importable, 1);
    assert_eq!(preview.would_insert, 1);
    let imported = ccs::commit(
        &preview.plan_id,
        &history,
        tokenscope::history::RollupConflictPolicy::KeepExisting,
        true,
    )
    .unwrap();
    assert_eq!(imported.requests_inserted, 1);
    let before = history.stored_events().unwrap();
    assert_eq!(before.len(), 2, "预置旧原生 A 和单独导入的 CCS B");
    let imported_b = before
        .iter()
        .find(|event| event.model_raw == "model-b")
        .unwrap();
    assert_ne!(imported_b.id, fixture.old_stored.id);
    assert_eq!(imported_b.session_id.as_deref(), Some(SESSION));
    assert_eq!(imported_b.ts, fixture.old.ts);
    assert_eq!(imported_b.tokens, fixture.old_stored.tokens);
    let new_model_event = UsageEvent {
        model: "model-b".to_string(),
        ..fixture.old.clone()
    };
    let new_alias = new_model_event.same_source_identity().unwrap().1;
    let connection =
        Connection::open_with_flags(&fixture.history_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let alias_owner: i64 = connection
        .query_row(
            "SELECT event_id FROM event_aliases
             WHERE app='codex' AND identity_scheme='codex-usage' AND identity_value=?1",
            [&new_alias],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(alias_owner, imported_b.id, "新模型别名已属于另一条事实");
    drop(connection);
    let before_facts: Vec<_> = before
        .iter()
        .map(|event| fact_rows(&fixture.history_path, event.id))
        .collect();
    let totals = history.totals().unwrap();
    let generation = history.generation().unwrap();
    drop(history);

    // 真实重解析会尝试把旧 A 纠正为 B；不能绕过已占用别名后继续保留两份用量。
    let result = collect_all_with_sources_for_test(
        vec![Box::new(CodexSource::new(&fixture.root))],
        &fixture.opts,
    );
    let error =
        result.expect_err("新模型别名属于另一事实时必须拒绝整轮采集，不能成功后双计或猜测合并");
    assert!(
        format!("{error:#}").contains("身份冲突"),
        "应明确报告身份冲突：{error:#}"
    );
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.event_count().unwrap(), 2);
    assert_eq!(history.stored_events().unwrap(), before);
    assert_eq!(history.totals().unwrap(), totals);
    assert_eq!(history.generation().unwrap(), generation);
    for (event, original) in before.iter().zip(before_facts) {
        assert_eq!(fact_rows(&fixture.history_path, event.id), original);
    }
}

#[test]
fn existing_model_aliases_on_the_same_fact_allow_unique_model_correction() {
    let fixture = model_upgrade_fixture(1);
    let new_model_event = UsageEvent {
        model: "model-b".to_string(),
        ..fixture.old.clone()
    };
    let new_alias = new_model_event.same_source_identity().unwrap().1;
    let connection = Connection::open(&fixture.history_path).unwrap();
    // 仅合成合法旧库状态：schema 对 app/scheme/value 唯一，不限制同一事件
    // 只能持有一个 codex-usage 别名。预置 A/B 双别名，不更改事实或旧来源证据。
    connection
        .execute(
            "INSERT INTO event_aliases(event_id,app,identity_scheme,identity_value,
                                      created_generation)
             VALUES(?1,'codex','codex-usage',?2,?3)",
            rusqlite::params![fixture.old_stored.id, new_alias, fixture.generation as i64],
        )
        .unwrap();
    drop(connection);
    let before = fact_rows(&fixture.history_path, fixture.old_stored.id);
    assert_eq!(before.aliases.len(), 2, "旧事件已同时持有 A/B 两个别名");
    assert_eq!(before.origins, fixture.old_fact.origins);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.event_count().unwrap(), 1);
    assert_eq!(history.generation().unwrap(), fixture.generation);
    assert_eq!(history.stored_events().unwrap()[0], fixture.old_stored);
    drop(history);

    // B 别名命中同一条旧 A 是合法状态，不能误报另一事实占用的身份冲突。
    let result = collect_all_with_sources_for_test(
        vec![Box::new(CodexSource::new(&fixture.root))],
        &fixture.opts,
    );
    assert!(result.is_ok(), "同一事实已持有新模型别名时仍须允许唯一纠正");
    let collected = result.unwrap();
    assert_eq!(collected.events.len(), 1);
    let corrected = assert_model_corrected_without_replacing_fact(&fixture);
    assert_eq!(corrected.model_raw, "model-b");
    assert_eq!(
        corrected.tokens,
        TokenCounts {
            input: 80,
            output: 10,
            cache_write: 0,
            cache_read: 20,
        }
    );
    let after = fact_rows(&fixture.history_path, fixture.old_stored.id);
    assert_eq!(after.origins, before.origins, "不新增或改写旧原生来源证据");
    assert_eq!(after.aliases, before.aliases, "已有双别名完整行均原样保留");
    assert_eq!(file_snapshot(&fixture.file), fixture.source_before);
    let history = HistoryDb::open(&fixture.history_path).unwrap();
    assert_eq!(history.event_count().unwrap(), 1);
    assert_eq!(history.generation().unwrap(), fixture.generation + 1);
}
