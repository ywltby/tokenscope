//! N18 项 1/2/7/8 回归：真实采集路径下的分档补齐、旧库指纹、可信标志与
//! 受检累计。全部使用临时目录与合成日志，不触碰真实数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{EventOrigin, EventWrite, HistoryDb, SourceFileKey, WritePrecedence};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::report::{SummaryOptions, collect_only, summary};
use tokenscope::source::CollectStats;
use tokenscope::source::claude_projects::ProjectMapping;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-n18-collect-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts(dir: &Path, claude: Option<PathBuf>) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        by: GroupBy::Agent,
        claude_dir: claude.clone(),
        codex_dir: Some(dir.join("no-such-codex")),
        claude_enabled: Some(claude.is_some()),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("pricing-index.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("no-such-openrouter.json")),
        modelsdev_path: Some(dir.join("no-such-modelsdev.json")),
        ..Default::default()
    }
}

fn claude_split_line(session: &str, id: &str, ts: &str, cw: u64, w5: u64, w1: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "message": {
            "id": id,
            "model": "claude-sonnet-4-5",
            "usage": {
                "input_tokens": 1000,
                "output_tokens": 10,
                "cache_creation_input_tokens": cw,
                "cache_read_input_tokens": 0,
                "cache_creation": {
                    "ephemeral_5m_input_tokens": w5,
                    "ephemeral_1h_input_tokens": w1
                }
            }
        }
    })
    .to_string()
}

fn write_lines(path: &Path, lines: &[String]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn stored(path: &Path) -> Vec<tokenscope::model::TokenCounts> {
    let h = HistoryDb::open(path).unwrap();
    h.stored_events()
        .unwrap()
        .into_iter()
        .map(|e| e.tokens)
        .collect()
}

/// 构造一条「旧版本认识」的源文件登记：指纹与真实文件一致、context_revision
/// 为旧值（规则 1 时代），模拟升级前已采集过的文件。
/// 与采集层同口径的库内路径键（规范化 + 去 Windows 前缀 + 小写）。
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

fn touch_legacy_source_file(
    history: &HistoryDb,
    app: AgentKind,
    root: &Path,
    file: &Path,
    context_revision: &str,
) {
    let meta = std::fs::metadata(file).unwrap();
    let fingerprint = format!(
        "{}:{}",
        meta.len(),
        meta.modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    history
        .touch_source_file(&tokenscope::history::SourceFileRecord {
            key: SourceFileKey {
                app,
                root: registered_path_key(root),
                path: registered_path_key(file),
            },
            fingerprint,
            size_bytes: meta.len(),
            mtime_ms: meta
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            context_revision: context_revision.to_string(),
            last_success_utc: None,
            stats: CollectStats::default(),
        })
        .unwrap();
}

// ── 项 1：旧 context_revision 必须让普通采集中重解析并补齐分档 ─────────

#[test]
fn n18_legacy_context_revision_forces_reparse_and_backfills_split() {
    let dir = temp_dir("legacy-reparse");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    // 合成日志：写总量 2M，5m 1M + 1h 1M（文件之后不再改动）。
    write_lines(
        &file,
        &[claude_split_line(
            "s1",
            "m1",
            "2026-07-17T08:00:00.000Z",
            2_000_000,
            1_000_000,
            1_000_000,
        )],
    );
    let db = dir.join("data").join("history.db");
    std::fs::create_dir_all(dir.join("data")).unwrap();
    let history = HistoryDb::open(&db).unwrap();
    // 旧版本（0ae242a）登记的指纹：context_revision 以规则号 3 开头
    // （真实旧库实测形如 "3|disabled-<hash>"），事件与来源证据的
    // parser_revision 均为 2。旧解析不读取 cache_creation 分项。
    touch_legacy_source_file(
        &history,
        AgentKind::ClaudeCode,
        &root,
        &file,
        &format!("3|{}", ProjectMapping::disabled().revision()),
    );
    // 事件本体也按旧版本写入：先以未知分档写入，再把行版本置 2。
    history
        .write_batch(&[EventWrite {
            event_key: "legacy-n18-k1".into(),
            app: AgentKind::ClaudeCode,
            ts: "2026-07-17T08:00:00Z".parse().unwrap(),
            model_raw: "claude-sonnet-4-5".into(),
            model_identity: tokenscope::model_identity::ModelIdentity::parse("claude-sonnet-4-5")
                .identity_key(),
            session_id: Some("s1".into()),
            record_id: Some("m1".into()),
            project_key: Some("proj-a".into()),
            session_initial_cwd: None,
            event_cwd: None,
            tokens: TokenCounts {
                input: 1000,
                output: 10,
                cache_write: 2_000_000,
                cache_read: 0,
                cache_write_5m: 0,
                cache_write_1h: 0,
                cache_write_split_known: false,
            },
            precedence: WritePrecedence::NativeLog,
            observed_at: "2026-07-17T08:00:00Z".parse().unwrap(),
            aliases: vec![tokenscope::history::EventAlias {
                app: AgentKind::ClaudeCode,
                scheme: "claude-message".into(),
                value: "s1|m1".into(),
            }],
            origins: vec![EventOrigin {
                origin_kind: "native_file".into(),
                origin_key: "legacy-n18-k1".into(),
                app: AgentKind::ClaudeCode,
                import_run_id: None,
                parser_revision: 2,
                source_model_raw: None,
                pricing_model: None,
                source_cost_usd: None,
                ts_precision_seconds: false,
            }],
        }])
        .unwrap();
    drop(history);

    // 普通采集（refresh = false）：旧 context_revision 失配 → 必须重解析。
    let o = opts(&dir, Some(root.clone()));
    let out = collect_only(&o).unwrap();
    assert!(out.changed, "旧 context_revision 失配 → 本轮必须有写入");

    let tokens = stored(&db);
    assert_eq!(tokens.len(), 1);
    let t = tokens[0];
    assert_eq!(t.cache_write, 2_000_000);
    assert!(
        t.cache_write_split_known,
        "重解析后必须补齐 5m/1h 可信分档（旧 context_revision 不得让文件被跳过）"
    );
    assert_eq!((t.cache_write_5m, t.cache_write_1h), (1_000_000, 1_000_000));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_current_context_revision_skips_reparse_without_duplicating() {
    // 幂等对照：context_revision 已是当前值时普通采集跳过，事件不重复。
    let dir = temp_dir("current-hit");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    write_lines(
        &file,
        &[claude_split_line(
            "s1",
            "m1",
            "2026-07-17T08:00:00.000Z",
            2_000_000,
            1_000_000,
            1_000_000,
        )],
    );
    let o = opts(&dir, Some(root.clone()));
    let first = collect_only(&o).unwrap();
    assert!(first.changed, "首轮采集应写入");
    let second = collect_only(&o).unwrap();
    assert!(!second.changed, "文件未变 → 本轮不重解析、不重复写入");
    let db = dir.join("data").join("history.db");
    let tokens = stored(&db);
    assert_eq!(tokens.len(), 1, "重复采集幂等");
    assert!(tokens[0].cache_write_split_known);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 7：TokenCounts 空累计与可信标志 ─────────────────────────────────

#[test]
fn n18_token_counts_accumulate_split_without_double_counting() {
    fn e(cw: u64, w5: u64, w1: u64, known: bool) -> tokenscope::model::UsageEvent {
        tokenscope::model::UsageEvent {
            ts: "2026-07-17T08:00:00Z".parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: "m".into(),
            session_id: "s".into(),
            project: "p".into(),
            session_initial_cwd: None,
            event_cwd: None,
            record_id: "r".into(),
            line: 0,
            source_path: String::new(),
            input_tokens: 1,
            output_tokens: 1,
            cache_write_tokens: cw,
            cache_read_tokens: 1,
            cache_write_5m: w5,
            cache_write_1h: w1,
            cache_write_split_known: known,
        }
    }
    // 全可信：分项之和 == 写总量。
    let mut all_known = TokenCounts::default();
    all_known.add_event(&e(10, 4, 6, true));
    all_known.add_event(&e(6, 2, 4, true));
    assert!(all_known.cache_write_split_known);
    assert_eq!(
        (
            all_known.cache_write,
            all_known.cache_write_5m,
            all_known.cache_write_1h
        ),
        (16, 6, 10)
    );
    assert_eq!(all_known.total(), 2 + 2 + 16 + 2, "分项不得重复计入总量");
    // 混入未知：整体退回未知，分项仍按原值累计（不再声称可信）。
    let mut mixed = all_known;
    mixed.add_event(&e(4, 0, 0, false));
    assert!(
        !mixed.cache_write_split_known,
        "混入任一未知分档事件即整体未知"
    );
    assert_eq!(
        (
            mixed.cache_write,
            mixed.cache_write_5m,
            mixed.cache_write_1h
        ),
        (20, 6, 10)
    );
    assert_eq!(
        mixed.total(),
        3 + 3 + 20 + 3,
        "未知事件的分项为 0，不重复计入"
    );
    // 空集合：默认未知且全零。
    let empty = TokenCounts::default();
    assert!(!empty.cache_write_split_known);
    assert_eq!(empty.total(), 0);
    // 单条未知事件：分项必须为 0（不猜造）。
    let mut single = TokenCounts::default();
    single.add_event(&e(5, 0, 0, false));
    assert!(!single.cache_write_split_known);
    assert_eq!((single.cache_write_5m, single.cache_write_1h), (0, 0));
}

// ── 项 8：多档未计价的受检累计 ────────────────────────────────────────

#[test]
fn n18_unpriced_tier_accumulation_is_checked() {
    // 一档溢出 + 一档缺价：unknown 按受检累计（不饱和、不回绕）。
    let a = TokenCounts {
        input: 0,
        output: 0,
        cache_write: u64::MAX,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let b = TokenCounts {
        input: 0,
        output: 0,
        cache_write: 1,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    assert!(a.checked_add(&b).is_none(), "溢出必须显式 None");
    // 两档均未计价：unknown 数量等于两档 token 之和。
    let w5 = TokenCounts {
        input: 0,
        output: 0,
        cache_write: 3,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let w1 = TokenCounts {
        input: 0,
        output: 0,
        cache_write: 4,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let sum = w5.checked_add(&w1).unwrap();
    assert_eq!(sum.cache_write, 7, "未计价 token 数按两档之和累计");
}

// ── 项 2：PARSER_REVISION 3 不得把合法 v2 记录当旧记录 ─────────────────

fn codex_write(key: &str, ts: &str, model: &str, session: &str, input: u64) -> EventWrite {
    let identity = format!("{session}|{model}|{input}|5|0|0");
    EventWrite {
        event_key: key.to_string(),
        app: AgentKind::Codex,
        ts: ts.parse().unwrap(),
        model_raw: model.to_string(),
        model_identity: tokenscope::model_identity::ModelIdentity::parse(model).identity_key(),
        session_id: Some(session.to_string()),
        record_id: None,
        project_key: Some("/work/codex".into()),
        session_initial_cwd: None,
        event_cwd: None,
        tokens: TokenCounts {
            input,
            output: 5,
            cache_write: 0,
            cache_read: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        },
        precedence: WritePrecedence::NativeLog,
        observed_at: ts.parse().unwrap(),
        aliases: vec![tokenscope::history::EventAlias {
            app: AgentKind::Codex,
            scheme: "codex-usage".into(),
            value: identity,
        }],
        origins: vec![EventOrigin {
            origin_kind: "native_file".into(),
            origin_key: key.to_string(),
            app: AgentKind::Codex,
            import_run_id: None,
            parser_revision: 2, // v2 写入
            source_model_raw: None,
            pricing_model: None,
            source_cost_usd: None,
            ts_precision_seconds: false,
        }],
    }
}

#[test]
fn n18_parser_revision_3_does_not_treat_valid_v2_codex_as_old() {
    // 同会话/同时间/同四桶、不同模型：v3 的新写入不得被 v2 已有记录吞掉，
    // 也不得整批失败。
    let dir = temp_dir("codex-v2-valid");
    let db = dir.join("history.db");
    let h = HistoryDb::open(&db).unwrap();
    let summary = h
        .write_batch(&[codex_write(
            "k1",
            "2026-07-17T15:00:00Z",
            "gpt-5.6-sol",
            "s1",
            100,
        )])
        .unwrap();
    assert_eq!(summary.inserted, 1);
    // v3 采集：同会话同时间同四桶、模型不同（线程设置模型归属）。
    let v3 = codex_write("k2", "2026-07-17T15:00:00Z", "gpt-5.6-luna", "s1", 100);
    let classes = h.classify_batch(&[v3]).unwrap();
    // 不得误合并：不同模型是不同事实（身份键含模型）。
    assert_eq!(h.stored_events().unwrap().len(), 1, "classify 不得写库");
    let _ = classes;
    let summary = h
        .write_batch(&[codex_write(
            "k2",
            "2026-07-17T15:00:00Z",
            "gpt-5.6-luna",
            "s1",
            100,
        )])
        .unwrap();
    // 不同模型 → 新事件（Insert），不是对旧事件的 Update/合并。
    assert_eq!(summary.inserted, 1, "不同模型必须新增，不得误合并");
    assert_eq!(summary.conflicts, 0);
    assert_eq!(summary.updated, 0, "不得改写已有 v2 记录的模型");
    let events = h.stored_events().unwrap();
    assert_eq!(events.len(), 2, "两个模型的记录各自保留");
    let models: Vec<&str> = events.iter().map(|e| e.model_raw.as_str()).collect();
    assert!(models.contains(&"gpt-5.6-sol"));
    assert!(models.contains(&"gpt-5.6-luna"));
    // v2 记录的模型未被改写。
    let kept = events
        .iter()
        .find(|e| e.model_raw == "gpt-5.6-sol")
        .unwrap();
    assert_eq!(kept.tokens.input, 100);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_parser_revision_3_same_model_same_buckets_is_not_duplicated() {
    // 同模型同四桶（v2 与 v3 各写一次）：同事实 → 不得重复插入。
    let dir = temp_dir("codex-v2-same");
    let db = dir.join("history.db");
    let h = HistoryDb::open(&db).unwrap();
    h.write_batch(&[codex_write(
        "k1",
        "2026-07-17T15:00:00Z",
        "gpt-5.6-sol",
        "s1",
        100,
    )])
    .unwrap();
    // 同一来源键的 v3 重写（同模型同四桶）：命中同一事件、幂等跳过。
    let v3 = codex_write("k1", "2026-07-17T15:00:00Z", "gpt-5.6-sol", "s1", 100);
    let summary = h.write_batch(&[v3]).unwrap();
    assert_eq!(h.stored_events().unwrap().len(), 1, "同事实不得重复插入");
    assert_eq!(summary.inserted, 0);
    assert_eq!(summary.unchanged, 1, "内容相同 → unchanged");
    // 带 codex-usage 别名的 v3 写入（登记别名），随后同事实的不同来源
    // 副本按别名命中同一事件——不重复插入、不改模型。
    let mut with_alias = codex_write("k-alias", "2026-07-17T15:00:00Z", "gpt-5.6-sol", "s1", 100);
    with_alias.aliases.push(tokenscope::history::EventAlias {
        app: AgentKind::Codex,
        scheme: "codex-usage".into(),
        value: "s1|gpt-5.6-sol|100|5|0|0".into(),
    });
    let summary = h.write_batch(&[with_alias]).unwrap();
    assert_eq!(summary.inserted, 0, "带别名的同事实写入命中同一事件");
    let mut replay = codex_write("k-replay", "2026-07-17T15:00:00Z", "gpt-5.6-sol", "s1", 100);
    replay.aliases.push(tokenscope::history::EventAlias {
        app: AgentKind::Codex,
        scheme: "codex-usage".into(),
        value: "s1|gpt-5.6-sol|100|5|0|0".into(),
    });
    let summary = h.write_batch(&[replay]).unwrap();
    assert_eq!(summary.inserted, 0, "别名重放不得新增事件");
    assert_eq!(h.stored_events().unwrap().len(), 1);
    let kept = h.stored_events().unwrap();
    assert_eq!(kept[0].model_raw, "gpt-5.6-sol", "模型不得被改写");
    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 6：估算与未计价必须可区分 ───────────────────────────────────────

#[test]
fn n18_summary_marks_estimated_and_unpriced_separately() {
    // 汇总层：可信分档未知 + 候选有分档 → 估算（complete=false）；
    // 分项已知 + 候选无分档 → 未知（unknown 计数）。两者文案与数量不同。
    let dir = temp_dir("est-vs-unknown");
    let toml = dir.join("pricing.toml");
    std::fs::write(
        &toml,
        r#"[[model]]
prefix = "m"
input = 3.0
output = 15.0
cache_write = 3.75
cache_write_5m = 3.75
cache_write_1h = 6.0

[[model]]
prefix = "no-tier"
input = 3.0
output = 15.0
cache_write = 3.75
"#,
    )
    .unwrap();
    let o = opts(&dir, None);
    let report = summary(&o).unwrap();
    // 无来源 → 汇总为零，但价格层必须可用（不报错）。
    assert_eq!(report.totals.requests, 0);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 2（复核）：write_collected_batch 不得把合法 v2 记录当旧记录 ─────

fn mark_event_revision_v2(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute("UPDATE usage_events SET parser_revision=2", [])
        .unwrap();
}

#[test]
fn n18_collected_batch_keeps_valid_v2_single_record_and_adds_new_model() {
    let dir = temp_dir("collected-v2-single");
    let db = dir.join("history.db");
    let h = HistoryDb::open(&db).unwrap();
    // 旧版（0ae242a）写入的 A：事件行与来源证据的 parser_revision 都是 2。
    h.write_batch(&[codex_write(
        "old-a",
        "2026-07-17T15:00:00Z",
        "gpt-5.6-sol",
        "s1",
        100,
    )])
    .unwrap();
    mark_event_revision_v2(&db);
    // 新版采集：同会话/同时间/同四桶、不同模型（线程设置归属正确）。
    let summary = h
        .write_collected_batch(&[codex_write(
            "new-b",
            "2026-07-17T15:00:00Z",
            "gpt-5.6-luna",
            "s1",
            100,
        )])
        .unwrap();
    assert_eq!(summary.events.inserted, 1, "不同模型是另一个合法请求");
    assert_eq!(summary.models_corrected, 0, "不得把 v2 记录当旧缺陷纠正");
    assert_eq!(summary.ambiguous_models, 0);
    let events = h.stored_events().unwrap();
    assert_eq!(events.len(), 2, "旧 A 与新 B 各自保留");
    let a = events
        .iter()
        .find(|e| e.model_raw == "gpt-5.6-sol")
        .unwrap();
    assert_eq!(a.parser_revision, 2, "旧记录版本不得被改写");
    assert_eq!(a.tokens.input, 100);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_collected_batch_with_existing_v2_ab_records_adds_c_without_failure() {
    let dir = temp_dir("collected-v2-ab");
    let db = dir.join("history.db");
    let h = HistoryDb::open(&db).unwrap();
    for (key, model) in [("old-a", "gpt-5.6-sol"), ("old-b", "gpt-5.6-luna")] {
        h.write_batch(&[codex_write(key, "2026-07-17T15:00:00Z", model, "s1", 100)])
            .unwrap();
    }
    mark_event_revision_v2(&db);
    let summary = h
        .write_collected_batch(&[codex_write(
            "new-c",
            "2026-07-17T15:00:00Z",
            "gpt-5.6-terra",
            "s1",
            100,
        )])
        .unwrap();
    assert_eq!(summary.events.inserted, 1, "第三模型同样新增，不整批失败");
    assert_eq!(summary.models_corrected, 0);
    let events = h.stored_events().unwrap();
    assert_eq!(events.len(), 3);
    let models: Vec<&str> = events.iter().map(|e| e.model_raw.as_str()).collect();
    for expected in ["gpt-5.6-sol", "gpt-5.6-luna", "gpt-5.6-terra"] {
        assert!(models.contains(&expected), "{} 必须保留", expected);
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn n18_collected_batch_still_corrects_v1_model_misattribution() {
    // 对照：v1 行（parser_revision=1）仍按既有规则纠正模型，证明收紧到
    // <2 没有破坏原修复路径。
    let dir = temp_dir("collected-v1");
    let db = dir.join("history.db");
    let h = HistoryDb::open(&db).unwrap();
    h.write_batch(&[codex_write(
        "old-v1",
        "2026-07-17T15:00:00Z",
        "gpt-5.6",
        "s1",
        100,
    )])
    .unwrap();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute("UPDATE usage_events SET parser_revision=1", [])
            .unwrap();
    }
    let summary = h
        .write_collected_batch(&[codex_write(
            "new-v1",
            "2026-07-17T15:00:00Z",
            "gpt-5.6-sol",
            "s1",
            100,
        )])
        .unwrap();
    assert_eq!(summary.models_corrected, 1, "v1 旧缺陷仍应纠正");
    let events = h.stored_events().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].model_raw, "gpt-5.6-sol");
    std::fs::remove_dir_all(&dir).ok();
}

// ── 项 7（复核）：零缓存写的未知事件与可信事件的顺序无关 ─────────────

fn unknown_event_without_write(input: u64) -> tokenscope::model::UsageEvent {
    tokenscope::model::UsageEvent {
        ts: "2026-07-17T08:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "m".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "r".into(),
        line: 0,
        source_path: String::new(),
        input_tokens: input,
        output_tokens: 1,
        cache_write_tokens: 0,
        cache_read_tokens: 1,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn known_event_with_write() -> tokenscope::model::UsageEvent {
    tokenscope::model::UsageEvent {
        cache_write_tokens: 10,
        cache_write_5m: 4,
        cache_write_1h: 6,
        cache_write_split_known: true,
        ..unknown_event_without_write(0)
    }
}

#[test]
fn n18_zero_write_unknown_event_contaminates_in_both_orders() {
    // add_event：未知（零写、其他 token 非零）→ 已知 与 已知 → 未知
    // 两种顺序都必须整体未知，且分项不重复计入总量。
    for unknown_first in [true, false] {
        let mut acc = TokenCounts::default();
        if unknown_first {
            acc.add_event(&unknown_event_without_write(7));
            acc.add_event(&known_event_with_write());
        } else {
            acc.add_event(&known_event_with_write());
            acc.add_event(&unknown_event_without_write(7));
        }
        assert!(
            !acc.cache_write_split_known,
            "顺序 unknown_first={unknown_first}：零写未知事件必须污染可信标志"
        );
        assert_eq!(
            (acc.cache_write, acc.cache_write_5m, acc.cache_write_1h),
            (10, 4, 6)
        );
        assert_eq!(acc.total(), 7 + 1 + 1 + 10 + 1 + 1, "分项不重复计入总量");

        // add：与 add_event 同语义。
        let mut via_add = TokenCounts::default();
        let unknown = TokenCounts::from_event(&unknown_event_without_write(7));
        let known = TokenCounts::from_event(&known_event_with_write());
        if unknown_first {
            via_add.add(&unknown);
            via_add.add(&known);
        } else {
            via_add.add(&known);
            via_add.add(&unknown);
        }
        assert!(!via_add.cache_write_split_known);
        assert_eq!(via_add, acc, "add 与 add_event 结果一致");

        // checked_add：同语义，且受检累计保持值。
        let sum = if unknown_first {
            unknown.checked_add(&known).unwrap()
        } else {
            known.checked_add(&unknown).unwrap()
        };
        assert!(!sum.cache_write_split_known);
        assert_eq!(sum, acc, "checked_add 与 add/add_event 一致");
    }
}
