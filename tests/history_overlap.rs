//! H06：原生采集与 CCS 导入的并集与日汇总桶选择（计划 §5.3、§7 H06）。
//!
//! 覆盖：A/B 部分交集只算一次、CCS 独有请求补入、两种导入顺序结果一致、
//! 源文件删除/重启后仍靠持久身份去重、同时间同用量的不同请求不误删、
//! 只有日粒度的历史计入统计但费用未知且不伪造成请求、明细覆盖的桶不双加、
//! 来源日时区与展示时区不一致时按日视图拒绝重切、无项目记录显式未知、
//! 模型等价身份把两侧归入同一桶。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::HistoryDb;
use tokenscope::import::ccs::{self, CcsSource};
use tokenscope::model::AgentKind;
use tokenscope::report::{SummaryOptions, SummaryReport, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-history-overlap-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_claude(path: &Path, session: &str, id: &str, ts: &str, input: u64, output: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let line = serde_json::json!({
        "type": "assistant",
        "timestamp": ts,
        "sessionId": session,
        "message": {
            "id": id,
            "model": "claude-opus-5-5",
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0
            }
        }
    });
    let mut body = std::fs::read_to_string(path).unwrap_or_default();
    body.push_str(&line.to_string());
    body.push('\n');
    std::fs::write(path, body).unwrap();
}

fn opts(dir: &Path, claude_root: &Path, tz: &str) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(claude_root.to_path_buf()),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some(tz.to_string()),
        ..Default::default()
    }
}

/// 合成 CCS 库：`requests` 与 `rollups` 复用 H05 测试的列集（幂等建表，
/// 便于同一个测试里多次追加数据）。
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

/// 合成一条 CCS 请求行（测试 fixture：列多但都是短标量，`allow` 比包一层
/// 结构体更直白）。
#[allow(clippy::too_many_arguments)]
fn seed_ccs_request(
    db: &Path,
    request_id: &str,
    session_id: Option<&str>,
    model: &str,
    created_at: i64,
    input: i64,
    output: i64,
    data_source: &str,
) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(CCS_SCHEMA).unwrap();
    conn.execute(
        "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model, request_model,
            input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
            total_cost_usd, status_code, session_id, created_at, data_source, pricing_model,
            input_token_semantics)
         VALUES(?1, 'p1', 'claude', ?2, ?2, ?3, ?4, 0, 0, '0.5', 200, ?5, ?6, ?7, ?2, 0)",
        rusqlite::params![
            request_id,
            model,
            input,
            output,
            session_id,
            created_at,
            data_source
        ],
    )
    .unwrap();
}

fn seed_ccs_rollup(
    db: &Path,
    date: &str,
    model: &str,
    provider: &str,
    request_count: i64,
    input: i64,
    output: i64,
) {
    let conn = rusqlite::Connection::open(db).unwrap();
    conn.execute_batch(CCS_SCHEMA).unwrap();
    conn.execute(
        "INSERT INTO usage_daily_rollups(date, app_type, provider_id, model, request_model,
            request_count, input_tokens, output_tokens, cache_read_tokens,
            cache_creation_tokens, total_cost_usd, input_token_semantics)
         VALUES(?1, 'claude', ?2, ?3, '', ?4, ?5, ?6, 0, 0, '9.0', 2)",
        rusqlite::params![date, provider, model, request_count, input, output],
    )
    .unwrap();
}

fn import_all(dir: &Path, source_day_tz: &str) -> ccs::ImportReport {
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, source_day_tz).unwrap();
    ccs::commit(
        &preview.plan_id,
        &history,
        tokenscope::history::RollupConflictPolicy::KeepExisting,
    )
    .unwrap()
}

fn report(dir: &Path, root: &Path, tz: &str) -> SummaryReport {
    summary(&opts(dir, root, tz)).unwrap()
}

#[test]
fn union_of_native_and_imported_requests_counts_each_once() {
    // 本地 A = {r1, r2}（原生采集），CCS B = {r2, r3}：r2 是同一请求。
    let dir = temp_dir("union");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    // 原生两侧的时间戳与会话身份与 CCS 一致（同一请求）。
    write_claude(&file, "s-1", "m1", "2026-09-15T08:00:00.000Z", 100, 10);
    write_claude(&file, "s-1", "m2", "2026-09-15T09:00:00.000Z", 200, 20);
    let first = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(first.totals.requests, 2);

    // CCS：r2 的会话行（同一身份）+ r3（CCS 独有，原生日志已清理）。
    seed_ccs_request(
        &dir.join("cc-switch.db"),
        "session:m2",
        Some("s-1"),
        "claude-opus-5-5",
        1_788_954_600, // 2026-09-15T09:00:00Z
        200,
        20,
        "session_log",
    );
    // r3 追加进同一库（换成两个语句）。
    {
        let conn = rusqlite::Connection::open(dir.join("cc-switch.db")).unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd, status_code, session_id, created_at, data_source, pricing_model,
                input_token_semantics)
             VALUES('session:r3', 'p1', 'claude', 'claude-opus-5-5', 'claude-opus-5-5',
                    300, 30, 0, 0, '0.5', 200, 's-9', 1789000000, 'session_log',
                    'claude-opus-5-5', 0)",
            [],
        )
        .unwrap();
    }
    let report_import = import_all(&dir, "Asia/Shanghai");
    assert_eq!(report_import.requests_inserted, 1, "只补入 CCS 独有请求 r3");
    assert_eq!(
        report_import.requests_unchanged + report_import.requests_conflicted,
        1,
        "r2 命中已有原生事件：完全相同或冲突，绝不重复计费"
    );

    let after = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(after.totals.requests, 3, "并集 = {{r1, r2, r3}}");
    assert_eq!(
        after.totals.tokens.total(),
        (100 + 10) + (200 + 20) + (300 + 30),
        "四桶等于三条唯一请求之和"
    );
    // 重复导入不改变任何数字。
    let again = import_all(&dir, "Asia/Shanghai");
    assert_eq!(again.requests_inserted, 0);
    assert_eq!(report(&dir, &root, "Asia/Shanghai").totals.requests, 3);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn import_first_then_collect_yields_the_same_union() {
    // 反向顺序：先导入后采集，结果必须与先采集后导入一致。
    let dir = temp_dir("order");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    // 先导入 r2（原生日志此刻还没有）。
    seed_ccs_request(
        &dir.join("cc-switch.db"),
        "session:m1",
        Some("s-1"),
        "claude-opus-5-5",
        1_789_545_600, // 2026-09-15T08:00:00Z
        100,
        10,
        "session_log",
    );
    let imported = import_all(&dir, "Asia/Shanghai");
    assert_eq!(imported.requests_inserted, 1);
    let after_import = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(after_import.totals.requests, 1);

    // 再采集原生日志（同一请求 + 一条独有请求）。
    write_claude(&file, "s-1", "m1", "2026-09-15T08:00:00.000Z", 100, 10);
    write_claude(&file, "s-1", "m9", "2026-09-15T10:00:00.000Z", 50, 5);
    let after_collect = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(
        after_collect.totals.requests, 2,
        "同一请求不被原生采集二次入库，原生独有请求正常补入"
    );
    assert_eq!(after_collect.totals.tokens.total(), 110 + 55);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn dedup_survives_source_deletion_and_restart() {
    let dir = temp_dir("restart");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    write_claude(&file, "s-1", "m1", "2026-09-15T08:00:00.000Z", 100, 10);
    assert_eq!(report(&dir, &root, "Asia/Shanghai").totals.requests, 1);

    seed_ccs_request(
        &dir.join("cc-switch.db"),
        "session:m1",
        Some("s-1"),
        "claude-opus-5-5",
        1_789_545_600,
        100,
        10,
        "session_log",
    );
    let imported = import_all(&dir, "Asia/Shanghai");
    assert_eq!(imported.requests_inserted, 0, "身份命中，不新增");

    // 删除原生源文件并重新打开历史库（模拟重启）后仍能去重。
    std::fs::remove_file(&file).unwrap();
    let reopened = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    assert_eq!(reopened.event_count().unwrap(), 1);
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &reopened, "Asia/Shanghai").unwrap();
    assert_eq!(preview.would_insert, 0, "重启后仍靠持久身份去重");
    assert_eq!(report(&dir, &root, "Asia/Shanghai").totals.requests, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn distinct_requests_with_identical_time_and_usage_are_kept() {
    let dir = temp_dir("identical");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    // 同会话、同模型、同用量、同时间戳，但 message.id 不同：是两条真实请求。
    write_claude(&file, "s-1", "m1", "2026-09-15T08:00:00.000Z", 100, 10);
    write_claude(&file, "s-1", "m2", "2026-09-15T08:00:00.000Z", 100, 10);
    let r = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(
        r.totals.requests, 2,
        "身份不同即独立请求，不按时间/用量猜测合并"
    );
    assert_eq!(r.totals.tokens.total(), 220);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rollup_only_history_counts_with_unknown_cost_and_no_fake_requests() {
    let dir = temp_dir("rollup-only");
    let root = dir.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    // 只有日粒度：CCS 已把明细归并成 rollup（原生侧没有对应记录）。
    seed_ccs_rollup(
        &dir.join("cc-switch.db"),
        "2026-06-01",
        "mimo-v2.5-pro",
        "_session",
        120,
        1_000_000,
        50_000,
    );
    import_all(&dir, "Asia/Shanghai");

    let r = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(r.totals.requests, 120, "请求数来自来源汇总的 request_count");
    assert_eq!(r.totals.tokens.input, 1_000_000);
    assert_eq!(r.totals.tokens.output, 50_000);
    assert!(
        r.totals.unknown_pricing,
        "日汇总没有逐请求规模，费用必须标记未知"
    );
    assert_eq!(
        r.totals.unknown_reason,
        Some("rollup_only"),
        "未知原因必须明确为「历史数据只有汇总」，不能误报模型价格未收录"
    );
    assert_eq!(
        r.totals.unknown_tokens.total(),
        1_050_000,
        "未知 token 计入整块汇总"
    );
    assert_eq!(r.rollup_coverage.rollup_buckets, 1);
    // 明细视图里没有伪造的请求事件。
    assert_eq!(r.sources.len(), 1);
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    assert_eq!(history.event_count().unwrap(), 0, "日汇总不伪造成请求事件");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rollup_bucket_covered_by_details_is_not_double_counted() {
    let dir = temp_dir("covered");
    let root = dir.join("projects");
    let file = root.join("proj-a").join("sess.jsonl");
    // 明细：模型等价写法与日汇总不同（claude-opus-5.5 vs claude-opus-5-5），
    // 归一小节后应落进同一桶。
    write_claude(&file, "s-1", "m1", "2026-09-15T08:00:00.000Z", 100, 10);
    seed_ccs_rollup(
        &dir.join("cc-switch.db"),
        "2026-09-15",
        "claude-opus-5.5",
        "_session",
        3,
        900,
        90,
    );
    import_all(&dir, "Asia/Shanghai");

    let r = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(r.totals.requests, 1, "命中桶按明细统计，汇总不追加");
    assert_eq!(r.totals.tokens.total(), 110);
    assert_eq!(
        r.totals.unknown_reason, None,
        "明细已覆盖的桶不引入「只有汇总」的未知原因"
    );
    assert_eq!(
        r.totals.unknown_tokens.total(),
        110,
        "未知 token 只来自本测试未配置价格的明细，不含被排除的日汇总"
    );
    assert_eq!(r.rollup_coverage.detail_buckets, 1);
    assert_eq!(r.rollup_coverage.unresolved_buckets, 1);
    assert_eq!(r.rollup_coverage.unresolved_covered_tokens.total(), 990);
    assert!(
        r.warnings.iter().any(|w| w.contains("未纳入的汇总")),
        "未解决覆盖必须在报告里可见: {:?}",
        r.warnings
    );
    // 桶选择固定在快照建立时：同一次查询的明细与汇总口径一致。
    assert_eq!(
        r.groups
            .iter()
            .find(|g| g.key == "2026-09-15")
            .map(|g| g.requests),
        Some(1)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rollup_timezone_mismatch_blocks_day_views_with_recoverable_hint() {
    let dir = temp_dir("tz");
    let root = dir.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    seed_ccs_rollup(
        &dir.join("cc-switch.db"),
        "2026-06-01",
        "mimo-v2.5-pro",
        "_session",
        10,
        1000,
        100,
    );
    // 导入时来源日时区按 UTC 记录。
    import_all(&dir, "UTC");

    // 按日分组 + 展示时区 Asia/Shanghai：日汇总不参与，并给出可恢复提示。
    let day = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(day.totals.requests, 0, "不把来源日期当午夜事件重切");
    assert!(
        day.rollup_coverage.timezone_mismatch.is_some(),
        "必须返回可恢复的时区不匹配提示"
    );

    // 切回来源时区即可看到这部分历史。
    let source_tz = report(&dir, &root, "UTC");
    assert_eq!(source_tz.totals.requests, 10);
    assert_eq!(source_tz.totals.tokens.total(), 1100);

    // 不限日期的模型视图同样不受重切影响。
    let mut model_opts = opts(&dir, &root, "Asia/Shanghai");
    model_opts.by = GroupBy::Model;
    let model = summary(&model_opts).unwrap();
    assert_eq!(model.totals.requests, 10, "模型/应用总量不受日时区影响");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn imported_requests_have_unknown_project_and_merge_by_model_identity() {
    let dir = temp_dir("project");
    let root = dir.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    seed_ccs_request(
        &dir.join("cc-switch.db"),
        "session:x1",
        Some("s-x"),
        "claude-opus-5.5",
        1_789_545_600,
        100,
        10,
        "session_log",
    );
    import_all(&dir, "Asia/Shanghai");

    let mut project_opts = opts(&dir, &root, "Asia/Shanghai");
    project_opts.by = GroupBy::Project;
    let by_project = summary(&project_opts).unwrap();
    assert_eq!(
        by_project.groups[0].key, "(未知)",
        "CCS 明细没有 cwd：项目保持未知，不按模型或日期猜分配"
    );

    let mut model_opts = opts(&dir, &root, "Asia/Shanghai");
    model_opts.by = GroupBy::Model;
    let by_model = summary(&model_opts).unwrap();
    assert_eq!(
        by_model.groups[0].key, "claudeopus55",
        "导入的模型名同样按等价身份归一"
    );
    assert_eq!(by_model.groups[0].requests, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn multi_channel_rollup_keys_stay_separate_rows() {
    let dir = temp_dir("channel");
    let root = dir.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    // 同日同模型、不同渠道（provider_id）→ 完整主键不同，各自成行。
    seed_ccs_rollup(
        &dir.join("cc-switch.db"),
        "2026-06-01",
        "m",
        "p-a",
        5,
        500,
        50,
    );
    seed_ccs_rollup(
        &dir.join("cc-switch.db"),
        "2026-06-01",
        "m",
        "p-b",
        7,
        700,
        70,
    );
    import_all(&dir, "Asia/Shanghai");
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    assert_eq!(history.rollup_count().unwrap(), 2);
    assert_eq!(history.rollup_request_count().unwrap(), 12);

    let r = report(&dir, &root, "Asia/Shanghai");
    assert_eq!(r.totals.requests, 12, "多渠道日键各自计入同一分组");
    assert_eq!(r.totals.tokens.total(), 550 + 770);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unknown_agent_kind_import_is_recorded_but_not_imported() {
    let dir = temp_dir("other-app");
    let root = dir.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    {
        let conn = rusqlite::Connection::open(dir.join("cc-switch.db")).unwrap();
        conn.execute_batch(CCS_SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO proxy_request_logs(request_id, provider_id, app_type, model, request_model,
                input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens,
                total_cost_usd, status_code, session_id, created_at, data_source, pricing_model,
                input_token_semantics)
             VALUES('g1', 'p1', 'gemini', 'g-model', 'g-model', 10, 1, 0, 0, '0', 200,
                    NULL, 1789000000, 'gemini_session', NULL, 0)",
            [],
        )
        .unwrap();
    }
    let history = HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
    let source = CcsSource::open(&dir.join("cc-switch.db")).unwrap();
    let preview = ccs::preview(&source, &history, "Asia/Shanghai").unwrap();
    assert_eq!(
        preview.unsupported_apps,
        vec![("gemini".to_string(), 1)],
        "其他应用在预览列出但不导入"
    );
    let report = ccs::commit(
        &preview.plan_id,
        &history,
        tokenscope::history::RollupConflictPolicy::KeepExisting,
    )
    .unwrap();
    assert_eq!(report.requests_inserted, 0);
    assert_eq!(history.event_count().unwrap(), 0);
    // 已保存的历史只含 Claude/Codex，未知应用不影响查询。
    let r = report_summary(&dir, &root);
    assert_eq!(r.totals.requests, 0);
    std::fs::remove_dir_all(&dir).ok();
}

fn report_summary(dir: &Path, root: &Path) -> SummaryReport {
    report(dir, root, "Asia/Shanghai")
}

#[test]
fn agent_filter_keeps_imported_history_visible() {
    let dir = temp_dir("agent-filter");
    let root = dir.join("projects");
    std::fs::create_dir_all(&root).unwrap();
    seed_ccs_request(
        &dir.join("cc-switch.db"),
        "session:z1",
        Some("s-z"),
        "claude-opus-5-5",
        1_789_545_600,
        10,
        1,
        "session_log",
    );
    import_all(&dir, "Asia/Shanghai");
    let mut filtered = opts(&dir, &root, "Asia/Shanghai");
    filtered.agent = Some(AgentKind::ClaudeCode);
    assert_eq!(summary(&filtered).unwrap().totals.requests, 1);
    let mut other = opts(&dir, &root, "Asia/Shanghai");
    other.agent = Some(AgentKind::Codex);
    assert_eq!(summary(&other).unwrap().totals.requests, 0);
    std::fs::remove_dir_all(&dir).ok();
}
