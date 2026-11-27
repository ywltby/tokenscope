//! v0.9.4：模型跨渠道别名合并 + CCS 日汇总下钻行。
//!
//! 计划：docs/plans/active/2026-11-19-v094-model-alias-rollup-drilldown.md。
//! 红-first：先在 v0.9.3 语义下复现两个缺陷（同名两行 / 下钻为空 /
//! 别名分桶双计），再实施修复。新 DTO 字段经 serde_json 访问，保证本文件
//! 在修复前即可编译并按断言失败。
//!
//! 全部 fixture 合成、历史库与价格路径在临时目录，不触碰真实用户数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::history::{
    DailyRollupWrite, EventWrite, HistoryDb, ImportRunRecord, RollupConflictPolicy, WritePrecedence,
};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::model_identity::ModelIdentity;
use tokenscope::query;
use tokenscope::report::{EventFilter, EventList, EventRow, RollupsMode, SummaryOptions, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-alias-drill-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn opts(dir: &Path, claude_dir: Option<PathBuf>, by: GroupBy, tz: &str) -> SummaryOptions {
    SummaryOptions {
        by,
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        claude_dir,
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

fn history_of(dir: &Path) -> HistoryDb {
    HistoryDb::open(&dir.join("data").join("history.db")).unwrap()
}

/// models.dev v4 快照（模型级 name；entries 由调用方以 JSON 给出）。
fn write_md(dir: &Path, entries: serde_json::Value) {
    let snap = serde_json::json!({
        "v": 4,
        "synced_at": "2026-11-19T00:00:00Z",
        "entries": entries,
    });
    std::fs::write(dir.join("md.json"), snap.to_string()).unwrap();
}

fn fable_md() -> serde_json::Value {
    serde_json::json!([{
        "id": "anthropic/claude-fable-5.2",
        "name": "Claude Fable 5.2",
        "input": 3.0, "output": 15.0, "cache_write": 3.75, "cache_read": 0.3
    }])
}

fn opus_md() -> serde_json::Value {
    serde_json::json!([{
        "id": "anthropic/claude-opus-4-7",
        "name": "Claude Opus 4.7",
        "input": 3.0, "output": 15.0, "cache_write": 3.75, "cache_read": 0.3
    }])
}

/// 直接写入一条 CCS 日汇总（四桶 + 来源费用 + 快照边界完全可控）。
#[allow(clippy::too_many_arguments)]
fn write_rollup(
    history: &HistoryDb,
    provider: &str,
    day: &str,
    model: &str,
    requests: u64,
    tokens: TokenCounts,
    started_utc: &str,
    source_cost_usd: Option<f64>,
) {
    let rollup = DailyRollupWrite {
        logical_source: "ccs".into(),
        day: day.into(),
        source_tz: "Asia/Shanghai".into(),
        app: AgentKind::ClaudeCode,
        provider_id: provider.into(),
        model: model.into(),
        request_model: String::new(),
        pricing_model: String::new(),
        request_count: requests,
        tokens,
        input_semantics: 2,
        source_cost_usd,
        revision: 0,
    };
    let run = ImportRunRecord {
        logical_source: "ccs".into(),
        source_path: "synthetic".into(),
        source_schema: "test".into(),
        started_utc: started_utc.into(),
        status: "committed".into(),
        detail: None,
        import_revision: 2,
    };
    history
        .apply_import_batch(
            &[],
            &[rollup],
            RollupConflictPolicy::KeepExisting,
            &run,
            None,
        )
        .unwrap();
}

/// 直接写入一条原生明细（无 CCS 来源证据；模型写法任意）。
fn write_detail(
    history: &HistoryDb,
    key: &str,
    ts: &str,
    model: &str,
    tokens: TokenCounts,
    project: Option<&str>,
) {
    let ts: jiff::Timestamp = ts.parse().unwrap();
    history
        .write_batch(&[EventWrite {
            event_key: key.into(),
            app: AgentKind::ClaudeCode,
            ts,
            model_raw: model.into(),
            model_identity: ModelIdentity::parse(model).identity_key(),
            session_id: Some("s1".into()),
            record_id: None,
            project_key: project.map(str::to_string),
            session_initial_cwd: None,
            event_cwd: None,
            tokens,
            precedence: WritePrecedence::NativeLog,
            observed_at: ts,
            aliases: Vec::new(),
            origins: Vec::new(),
        }])
        .unwrap();
}

/// 一条 Claude assistant 行（原生采集 fixture）。
fn claude_line(
    session: &str,
    message: &str,
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
            "id": message,
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

fn write_claude_log(root: &Path, rel: &str, lines: &[String]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, lines.join("\n")).unwrap();
}

fn drill(o: &SummaryOptions, f: EventFilter) -> EventList {
    let snapshot = query::begin_query(o).unwrap();
    query::query_events(&snapshot.query_id, &f).unwrap()
}

fn row_json(row: &EventRow) -> serde_json::Value {
    serde_json::to_value(row).unwrap()
}

fn group_of<'a>(
    report: &'a tokenscope::report::SummaryReport,
    key: &str,
) -> &'a tokenscope::aggregate::Group {
    report
        .groups
        .iter()
        .find(|g| g.key == key)
        .unwrap_or_else(|| {
            panic!(
                "缺少分组 {key}：{:?}",
                report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
            )
        })
}

/// summary() 的 groups 末尾附「合计」行；断言分组数时排除它。
fn real_groups(report: &tokenscope::report::SummaryReport) -> Vec<&tokenscope::aggregate::Group> {
    report.groups.iter().filter(|g| g.key != "合计").collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ── 问题 1：同一模型跨渠道合并 ─────────────────────────────────────────

/// 用户核实例：`anthropic/claude-fable-5.2` 与 `claude-fable-5-2` 必须
/// 合并为一行（可信名「Claude Fable 5.2」），下钻两种写法都看到全部请求，
/// 请求明细保留各自原始 ID。
#[test]
fn fable_two_channel_spellings_merge_into_one_group() {
    let dir = temp_dir("fable-merge");
    write_md(&dir, fable_md());
    let claude = dir.join("claude");
    write_claude_log(
        &claude,
        "proj/s1.jsonl",
        &[
            claude_line(
                "s1",
                "m1",
                "2026-11-10T09:00:00.000+08:00",
                "anthropic/claude-fable-5.2",
                1000,
                500,
            ),
            claude_line(
                "s1",
                "m2",
                "2026-11-10T09:05:00.000+08:00",
                "anthropic/claude-fable-5.2",
                1000,
                500,
            ),
            claude_line(
                "s1",
                "m3",
                "2026-11-10T09:10:00.000+08:00",
                "claude-fable-5-2",
                1000,
                500,
            ),
        ],
    );
    write_claude_log(
        &claude,
        "proj/s2.jsonl",
        &[
            claude_line(
                "s2",
                "m4",
                "2026-11-10T10:00:00.000+08:00",
                "claude-fable-5-2",
                1000,
                500,
            ),
            claude_line(
                "s2",
                "m5",
                "2026-11-10T10:05:00.000+08:00",
                "claude-fable-5-2",
                1000,
                500,
            ),
        ],
    );
    let o = opts(&dir, Some(claude), GroupBy::Model, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        1,
        "两个渠道写法必须是同一行：{:?}",
        report
            .groups
            .iter()
            .map(|g| (&g.key, &g.label))
            .collect::<Vec<_>>()
    );
    let g = real_groups(&report)[0];
    assert_eq!(g.key, "anthropic/claudefable52", "规范键取可信完整标识");
    assert_eq!(g.label.as_deref(), Some("Claude Fable 5.2"));
    assert_eq!(g.requests, 5);
    assert_eq!(g.tokens.input, 5000);
    assert_eq!(g.tokens.output, 2500);
    // 每请求 (1000×3 + 500×15)/1e6 = 0.0105，5 请求 = 0.0525。
    assert!(close(g.cost_usd, 0.0525), "{}", g.cost_usd);
    assert!(close(g.total_cost_usd.unwrap(), 0.0525));

    // 下钻：规范键与两种原始写法都命中全部 5 行，行内仍是原始 ID。
    for spelling in [
        g.key.as_str(),
        "anthropic/claude-fable-5.2",
        "claude-fable-5-2",
    ] {
        let list = drill(
            &o,
            EventFilter {
                model: Some(spelling.into()),
                ..Default::default()
            },
        );
        assert_eq!(list.total, 5, "下钻 {spelling}");
        assert_eq!(list.rows.len(), 5, "下钻 {spelling}");
        let namespaced = list
            .rows
            .iter()
            .filter(|r| r.model == "anthropic/claude-fable-5.2")
            .count();
        let bare = list
            .rows
            .iter()
            .filter(|r| r.model == "claude-fable-5-2")
            .count();
        assert_eq!((namespaced, bare), (2, 3), "明细保留原始 ID：{spelling}");
        assert!(
            list.rows
                .iter()
                .all(|r| row_json(r).get("rollup").is_none())
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 边界：日期后缀版本、变体（含显式空变体）、未确认渠道前缀、仅展示名
/// 相同的不同 ID——一律保持独立分组（前缀回退同价也不合并）。
#[test]
fn distinct_versions_variants_and_same_name_stay_separate() {
    let dir = temp_dir("separate");
    write_md(
        &dir,
        serde_json::json!([
            {"id": "anthropic/claude-fable-5.2", "name": "Claude Fable 5.2",
             "input": 3.0, "output": 15.0, "cache_write": 3.75, "cache_read": 0.3},
            {"id": "vendor/fable-five-two", "name": "Claude Fable 5.2",
             "input": 1.0, "output": 5.0, "cache_write": 1.25, "cache_read": 0.1}
        ]),
    );
    let history = history_of(&dir);
    let spellings = [
        "anthropic/claude-fable-5.2",
        "claude-fable-5-2-20261101",
        "claude-fable-5-2:free",
        "claude-fable-5-2:",
        "zenmux/claude-fable-5-2",
        "vendor/fable-five-two",
    ];
    for (i, m) in spellings.iter().enumerate() {
        write_detail(
            &history,
            &format!("native|claude|/logs/a.jsonl|{i}|fp"),
            &format!("2026-11-10T09:0{i}:00Z"),
            m,
            counts(1000, 500, 0, 0),
            None,
        );
    }
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        spellings.len(),
        "版本/变体/未确认渠道/仅同名不得合并：{:?}",
        report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
    );
    for m in spellings {
        let key = ModelIdentity::parse(m).identity_key();
        let g = group_of(&report, &key);
        assert_eq!(g.requests, 1, "{m}");
        let list = drill(
            &o,
            EventFilter {
                model: Some(key),
                ..Default::default()
            },
        );
        assert_eq!(list.total, 1, "下钻 {m} 只见自身");
        assert_eq!(list.rows[0].model, m);
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 裸末段命中多个 models.dev 模型但没有明确关系 → 不确认、不合并。
#[test]
fn ambiguous_bare_leaf_not_merged() {
    let dir = temp_dir("ambiguous");
    write_md(
        &dir,
        serde_json::json!([
            {"id": "lab/demo-4-7", "name": "Demo 4.7",
             "input": 3.0, "output": 15.0},
            {"id": "mirror/demo-4-7", "name": "Demo 4.7",
             "input": 1.0, "output": 5.0}
        ]),
    );
    let history = history_of(&dir);
    for (i, m) in ["demo-4-7", "lab/demo-4-7", "mirror/demo-4-7"]
        .iter()
        .enumerate()
    {
        write_detail(
            &history,
            &format!("native|claude|/logs/a.jsonl|{i}|fp"),
            &format!("2026-11-10T09:0{i}:00Z"),
            m,
            counts(1000, 0, 0, 0),
            None,
        );
    }
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        3,
        "歧义裸名保持独立：{:?}",
        report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 覆盖桶别名分桶：裸名原生明细 + 渠道前缀写法的 CCS 汇总同日 →
/// 必须按同一身份判 B 类覆盖，总量 = 汇总 + 快照后新增，不双计。
#[test]
fn alias_split_coverage_bucket_must_not_double_count() {
    let dir = temp_dir("alias-coverage");
    // 身份确认需要可信快照：opus 已收录但价格全缺（null）——只确认别名
    // 归属，不引入估算金额，总额保持 = 来源费用 1.5。
    write_md(
        &dir,
        serde_json::json!([{
            "id": "anthropic/claude-opus-4-7",
            "name": "Claude Opus 4.7",
            "input": null, "output": null, "cache_write": null, "cache_read": null
        }]),
    );
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-05-28",
        "anthropic/claude-opus-4-7",
        10,
        counts(1000, 0, 0, 0),
        "2026-05-28T12:00:00Z",
        Some(1.5),
    );
    // 快照内裸名明细（应被覆盖）与快照后新增（独立计入）。
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|0|fp",
        "2026-05-28T01:00:00Z",
        "claude-opus-4-7",
        counts(100, 0, 0, 0),
        None,
    );
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|1|fp",
        "2026-05-28T13:00:00Z",
        "claude-opus-4-7",
        counts(200, 0, 0, 0),
        None,
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        1,
        "别名必须同组：{:?}",
        report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
    );
    assert_eq!(
        report.totals.tokens.input, 1200,
        "汇总 1000 + 快照后 200，不双计"
    );
    assert_eq!(report.totals.requests, 11);
    assert!(close(report.totals.total_cost_usd.unwrap(), 1.5));

    let list = drill(
        &o,
        EventFilter {
            model: Some("anthropic/claudeopus47".into()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 3, "2 条明细 + 1 条日汇总行");
    let covered = list
        .rows
        .iter()
        .find(|r| r.ts.starts_with("2026-05-28 09:"))
        .unwrap();
    assert_eq!(
        row_json(covered)
            .get("covered_by_rollup")
            .and_then(|v| v.as_str()),
        Some("2026-05-28"),
        "被覆盖请求必须标记计入当日汇总"
    );
    let post = list
        .rows
        .iter()
        .find(|r| r.ts.starts_with("2026-05-28 21:"))
        .unwrap();
    assert!(
        row_json(post).get("covered_by_rollup").is_none(),
        "快照后请求不标记"
    );
    let rollup_rows: Vec<&EventRow> = list
        .rows
        .iter()
        .filter(|r| row_json(r).get("rollup").is_some())
        .collect();
    assert_eq!(rollup_rows.len(), 1);
    let meta = &row_json(rollup_rows[0])["rollup"];
    assert_eq!(meta["requests"].as_u64(), Some(10));
    assert_eq!(meta["day"].as_str(), Some("2026-05-28"));
    std::fs::remove_dir_all(&dir).ok();
}

// ── 问题 2：CCS 日汇总下钻行 ───────────────────────────────────────────

/// 用户核实例：claude-opus-4-7 无逐请求记录，两条 CCS 日汇总
/// （2026-05-28：223 请求；2026-05-29：11 请求；合计 110,198,123 token）
/// → 下钻必须按两天显示两条 CCS 行，逐项与聚合一致。
#[test]
fn opus_rollup_only_model_drills_two_ccs_day_rows() {
    let dir = temp_dir("opus-two-days");
    write_md(&dir, opus_md());
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-05-28",
        "claude-opus-4-7",
        223,
        counts(100_000_000, 5_000_000, 2_000_000, 3_000_000),
        "2026-05-28T12:00:00Z",
        Some(42.5),
    );
    write_rollup(
        &history,
        "p1",
        "2026-05-29",
        "claude-opus-4-7",
        11,
        counts(100_000, 50_000, 20_000, 28_123),
        "2026-05-29T12:00:00Z",
        Some(1.25),
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 1);
    let g = real_groups(&report)[0];
    assert_eq!(g.requests, 234);
    assert_eq!(g.tokens.total(), 110_198_123);
    assert_eq!(g.label.as_deref(), Some("Claude Opus 4.7"));
    assert!(close(g.total_cost_usd.unwrap(), 43.75));
    assert_eq!(g.cost_detail, Some("rollup_only"));
    assert_eq!(g.source_cost_state, Some("full"));

    let list = drill(
        &o,
        EventFilter {
            model: Some(g.key.clone()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 2, "纯汇总模型下钻 = 两条日汇总行");
    assert_eq!(list.rows.len(), 2);
    // 日降序：05-29 在前。
    let first = row_json(&list.rows[0]);
    let second = row_json(&list.rows[1]);
    assert_eq!(
        first["ts"].as_str(),
        Some("2026-05-29"),
        "日粒度，不补造时分秒"
    );
    assert_eq!(second["ts"].as_str(), Some("2026-05-28"));
    for (v, day, req, inp, out, cw, cr, cost) in [
        (
            &first,
            "2026-05-29",
            11u64,
            100_000u64,
            50_000u64,
            20_000u64,
            28_123u64,
            1.25f64,
        ),
        (
            &second,
            "2026-05-28",
            223,
            100_000_000,
            5_000_000,
            2_000_000,
            3_000_000,
            42.5,
        ),
    ] {
        assert_eq!(v["agent"].as_str(), Some("claude-code"));
        assert_eq!(v["record_id"].as_str(), Some(""), "不补造请求 ID");
        assert_eq!(v["session_id"].as_str(), Some(""));
        assert_eq!(v["project"].as_str(), Some(""), "不补造项目归属");
        assert_eq!(v["input"].as_u64(), Some(inp));
        assert_eq!(v["output"].as_u64(), Some(out));
        assert_eq!(v["cache_write"].as_u64(), Some(cw));
        assert_eq!(v["cache_read"].as_u64(), Some(cr));
        let meta = &v["rollup"];
        assert_eq!(meta["day"].as_str(), Some(day));
        assert_eq!(meta["requests"].as_u64(), Some(req));
        assert_eq!(meta["source_tz"].as_str(), Some("Asia/Shanghai"));
        assert_eq!(meta["label"].as_str(), Some("Claude Opus 4.7"));
        assert_eq!(meta["models"], serde_json::json!(["claude-opus-4-7"]));
        assert!(close(meta["total_cost_usd"].as_f64().unwrap(), cost));
        assert!(close(meta["source_cost_usd"].as_f64().unwrap(), cost));
        assert_eq!(meta["source_cost_state"].as_str(), Some("full"));
        assert_eq!(meta["cost_detail"].as_str(), Some("rollup_only"));
        assert_eq!(meta["cost_partially_unknown"].as_bool(), Some(false));
        assert!(
            close(v["cost_usd"].as_f64().unwrap(), cost),
            "行费用 = 显示金额"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 同日多渠道（provider）汇总合并为一行：请求数/四桶/费用 = 两渠道之和，
/// 浮层列出合并的原始渠道 ID。
#[test]
fn same_day_multi_channel_rollups_merge_one_row() {
    let dir = temp_dir("multi-channel");
    write_md(&dir, opus_md());
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-05-28",
        "claude-opus-4-7",
        10,
        counts(600, 0, 0, 0),
        "2026-05-28T12:00:00Z",
        Some(1.0),
    );
    write_rollup(
        &history,
        "p2",
        "2026-05-28",
        "anthropic/claude-opus-4-7",
        5,
        counts(400, 0, 0, 0),
        "2026-05-28T12:00:00Z",
        Some(0.5),
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 1);
    assert_eq!(real_groups(&report)[0].requests, 15);
    assert_eq!(real_groups(&report)[0].tokens.input, 1000);
    assert!(close(real_groups(&report)[0].total_cost_usd.unwrap(), 1.5));

    let list = drill(
        &o,
        EventFilter {
            model: Some(real_groups(&report)[0].key.clone()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 1, "同日多渠道合并一行");
    let v = row_json(&list.rows[0]);
    let meta = &v["rollup"];
    assert_eq!(meta["requests"].as_u64(), Some(15));
    assert_eq!(v["input"].as_u64(), Some(1000));
    assert!(close(meta["total_cost_usd"].as_f64().unwrap(), 1.5));
    assert_eq!(
        meta["models"],
        serde_json::json!(["anthropic/claude-opus-4-7", "claude-opus-4-7"]),
        "合并的原始渠道 ID 去重排序"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 混合场景对账：原生 + 被覆盖 + 快照后新增 + 日汇总。
/// Σ(未覆盖请求) + Σ(汇总行) == 分组 requests / 四桶 / total_cost_usd。
fn setup_mixed(dir: &Path) {
    write_md(dir, opus_md());
    let history = history_of(dir);
    write_rollup(
        &history,
        "p1",
        "2026-05-28",
        "anthropic/claude-opus-4-7",
        10,
        counts(1000, 0, 0, 0),
        "2026-05-28T12:00:00Z",
        Some(2.0),
    );
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|0|fp",
        "2026-05-28T01:00:00Z",
        "claude-opus-4-7",
        counts(100, 0, 0, 0),
        Some("proj-a"),
    );
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|1|fp",
        "2026-05-28T13:00:00Z",
        "claude-opus-4-7",
        counts(200, 0, 0, 0),
        Some("proj-a"),
    );
    write_detail(
        &history,
        "native|claude|/logs/b.jsonl|0|fp",
        "2026-05-27T10:00:00Z",
        "claude-opus-4-7",
        counts(50, 0, 0, 0),
        Some("proj-b"),
    );
}

#[test]
fn mixed_native_covered_and_rollup_reconciles() {
    let dir = temp_dir("mixed");
    setup_mixed(&dir);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 1);
    let g = real_groups(&report)[0];
    assert_eq!(g.requests, 12, "汇总 10 + 快照后 1 + 另一日 1");
    assert_eq!(g.tokens.input, 1250);
    // 快照后 200×3/1e6 + 另一日 50×3/1e6 + 来源 2.0。
    assert!(
        close(g.total_cost_usd.unwrap(), 2.00075),
        "{}",
        g.total_cost_usd.unwrap()
    );

    let list = drill(
        &o,
        EventFilter {
            model: Some(g.key.clone()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 4, "3 条明细 + 1 条日汇总");
    let mut uncovered_input = 0u64;
    let mut uncovered_requests = 0u64;
    let mut uncovered_cost = 0.0f64;
    let mut rollup_input = 0u64;
    let mut rollup_requests = 0u64;
    let mut rollup_cost = 0.0f64;
    for r in &list.rows {
        let v = row_json(r);
        if v.get("rollup").is_some() {
            rollup_input += r.input.to_u64().unwrap();
            rollup_requests += v["rollup"]["requests"].as_u64().unwrap();
            rollup_cost += v["rollup"]["total_cost_usd"].as_f64().unwrap();
        } else if v.get("covered_by_rollup").is_none() {
            uncovered_input += r.input.to_u64().unwrap();
            uncovered_requests += 1;
            uncovered_cost += r.cost_usd.unwrap_or(0.0);
        } else {
            assert_eq!(v["covered_by_rollup"].as_str(), Some("2026-05-28"));
        }
    }
    assert_eq!(uncovered_requests + rollup_requests, g.requests);
    assert_eq!(uncovered_input + rollup_input, g.tokens.input);
    assert!(close(
        uncovered_cost + rollup_cost,
        g.total_cost_usd.unwrap()
    ));
    std::fs::remove_dir_all(&dir).ok();
}

/// 筛选对齐：日 / 项目 / 模型（两种写法）/ 应用主筛选。
#[test]
fn filters_align_with_aggregation() {
    let dir = temp_dir("filters");
    setup_mixed(&dir);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );

    let day28 = drill(
        &o,
        EventFilter {
            day: Some("2026-05-28".into()),
            ..Default::default()
        },
    );
    assert_eq!(day28.total, 3, "当日 2 明细 + 1 汇总行");
    assert_eq!(
        day28
            .rows
            .iter()
            .filter(|r| row_json(r).get("rollup").is_some())
            .count(),
        1
    );
    let day27 = drill(
        &o,
        EventFilter {
            day: Some("2026-05-27".into()),
            ..Default::default()
        },
    );
    assert_eq!(day27.total, 1);
    assert!(
        day27
            .rows
            .iter()
            .all(|r| row_json(r).get("rollup").is_none())
    );

    let proj_b = drill(
        &o,
        EventFilter {
            project: Some("proj-b".into()),
            ..Default::default()
        },
    );
    assert_eq!(proj_b.total, 1, "项目下钻不含汇总行");
    assert!(
        proj_b
            .rows
            .iter()
            .all(|r| row_json(r).get("rollup").is_none())
    );
    let proj_a = drill(
        &o,
        EventFilter {
            project: Some("proj-a".into()),
            ..Default::default()
        },
    );
    assert_eq!(proj_a.total, 2);
    assert!(
        proj_a
            .rows
            .iter()
            .all(|r| row_json(r).get("rollup").is_none())
    );

    let bare = drill(
        &o,
        EventFilter {
            model: Some("claude-opus-4-7".into()),
            ..Default::default()
        },
    );
    assert_eq!(bare.total, 4, "裸写法下钻与规范键一致");

    let mut codex_opts = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    codex_opts.agent = Some(AgentKind::Codex);
    let codex = drill(&codex_opts, EventFilter::default());
    assert_eq!(codex.total, 0, "CCS 汇总只补充 Claude Code");
    std::fs::remove_dir_all(&dir).ok();
}

/// 混合分页：limit=1/2 走完事件段 + 汇总段，无漏无重、顺序确定；
/// 两种写法共享下钻指纹（游标可续用）；旧格式游标仍可定位。
#[test]
fn mixed_pagination_no_gap_no_dup() {
    let dir = temp_dir("pagination");
    setup_mixed(&dir);
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let key = "anthropic/claudeopus47".to_string();
    // 产品流程：一次 begin_query、同一会话内多次 query_events——游标绑定
    // query_id，跨会话续页必然过期（每页新建会话不是分页语义）。
    let snapshot = query::begin_query(&o).unwrap();
    let page = |f: EventFilter| query::query_events(&snapshot.query_id, &f).unwrap();

    let expected: Vec<(String, bool)> = vec![
        ("2026-05-28 21:00:00".into(), false),
        ("2026-05-28 09:00:00".into(), false),
        ("2026-05-27 18:00:00".into(), false),
        ("2026-05-28".into(), true),
    ];
    for limit in [1usize, 2] {
        let mut got: Vec<(String, bool)> = Vec::new();
        let mut before: Option<String> = None;
        let mut pages = 0;
        loop {
            let list = page(EventFilter {
                model: Some(key.clone()),
                limit: Some(limit),
                before: before.clone(),
                ..Default::default()
            });
            assert_eq!(list.total, 4);
            if list.rows.is_empty() {
                break;
            }
            pages += 1;
            assert!(pages <= 4, "limit={limit} 不得死循环");
            for r in &list.rows {
                got.push((r.ts.clone(), row_json(r).get("rollup").is_some()));
            }
            before = Some(list.rows.last().unwrap().cursor.clone());
        }
        assert_eq!(got.len(), 4, "limit={limit} 无漏行");
        let mut dedup = got.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(dedup.len(), 4, "limit={limit} 无重复行");
        assert_eq!(got, expected, "limit={limit} 顺序：事件降序 → 汇总日行");
    }

    // 跨写法续页：第一页裸写法，第二页规范写法 + 同一游标。
    let page1 = page(EventFilter {
        model: Some("claude-opus-4-7".into()),
        limit: Some(2),
        ..Default::default()
    });
    assert_eq!(page1.rows.len(), 2);
    let cursor = page1.rows.last().unwrap().cursor.clone();
    let page2 = page(EventFilter {
        model: Some(key.clone()),
        limit: Some(2),
        before: Some(cursor.clone()),
        ..Default::default()
    });
    assert_eq!(page2.rows.len(), 2, "两种写法同一子查询指纹");
    assert_eq!(page2.rows[1].ts, "2026-05-28");

    // 旧格式游标（无 kind/ridx 字段）仍可定位事件行。
    let mut old: serde_json::Value = serde_json::from_str(&cursor).unwrap();
    if let Some(obj) = old.as_object_mut() {
        obj.remove("kind");
        obj.remove("ridx");
    }
    let page_old = page(EventFilter {
        model: Some(key.clone()),
        limit: Some(2),
        before: Some(old.to_string()),
        ..Default::default()
    });
    assert_eq!(page_old.rows.len(), 2);
    assert_eq!(page_old.rows[0].ts, page2.rows[0].ts);
    std::fs::remove_dir_all(&dir).ok();
}

/// 既有规则保持：detail_only / 时刻范围 / 按日敏感时区不匹配 → 无汇总行。
#[test]
fn detail_only_instant_and_tz_mismatch_keep_no_rollup_rows() {
    let dir = temp_dir("rules");
    setup_mixed(&dir);

    let mut detail_only = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    detail_only.rollups = RollupsMode::DetailOnly;
    let list = drill(&detail_only, EventFilter::default());
    assert_eq!(list.total, 3, "只看请求明细");
    assert!(
        list.rows
            .iter()
            .all(|r| row_json(r).get("rollup").is_none())
    );

    let mut instant = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    // 非整日对齐才是时刻模式（00:00–23:59 按既有规则转自然日模式，汇总
    // 合法参与）；时刻区间精确到分钟，日粒度历史一律不取。
    instant.start_at = Some("2026-05-28T00:30".into());
    instant.end_at = Some("2026-05-28T23:00".into());
    let list = drill(&instant, EventFilter::default());
    assert_eq!(list.total, 2, "时刻范围不取日汇总");
    assert!(
        list.rows
            .iter()
            .all(|r| row_json(r).get("rollup").is_none())
    );

    // 时区不匹配只在**按日敏感**（by=Day 或限定范围）时排除汇总。
    let utc = opts(&dir, Some(dir.join("no-claude")), GroupBy::Day, "UTC");
    let report = summary(&utc).unwrap();
    assert_eq!(
        report.totals.tokens.input, 350,
        "时区不匹配按日敏感排除汇总"
    );
    let list = drill(&utc, EventFilter::default());
    assert_eq!(list.total, 3);
    assert!(
        list.rows
            .iter()
            .all(|r| row_json(r).get("rollup").is_none())
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 升级零迁移：同一历史库，缺少关系证据时两行；加入含基础模型 ID 的目录
/// 快照后不重导即合并；库内 model_identity 列值不变。
#[test]
fn upgrade_without_reimport_merges_after_snapshot_added() {
    let dir = temp_dir("upgrade");
    let history = history_of(&dir);
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|0|fp",
        "2026-11-10T09:00:00Z",
        "lab/demo-4-7",
        counts(1000, 500, 0, 0),
        None,
    );
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|1|fp",
        "2026-11-10T09:05:00Z",
        "mirror/demo-4-7",
        counts(1000, 500, 0, 0),
        None,
    );
    drop(history);

    let stored_identities = || -> Vec<String> {
        let conn = rusqlite::Connection::open(dir.join("data").join("history.db")).unwrap();
        let mut stmt = conn
            .prepare("SELECT DISTINCT model_identity FROM usage_events ORDER BY model_identity")
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    };
    let before = stored_identities();
    assert_eq!(before, vec!["lab/demo47", "mirror/demo47"]);

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        2,
        "无快照 = 保守不合并（旧行为）"
    );

    write_md(
        &dir,
        serde_json::json!([{
            "id": "mirror/demo-4-7", "canonical_model_id": "lab/demo-4-7",
            "name": "Demo 4.7", "input": 2.0, "output": 4.0
        }]),
    );
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 1, "放入快照后无需重导即合并");
    assert_eq!(real_groups(&report)[0].requests, 2);
    assert_eq!(stored_identities(), before, "存储键不改写（升级零迁移）");
    std::fs::remove_dir_all(&dir).ok();
}

/// 费用四态：来源显式 0 / 缺失可估算 / 部分缺价 / 无候选——
/// 汇总行金额与聚合分组逐项一致（共享费用函数保证）。
#[test]
fn rollup_row_cost_matches_aggregation_four_states() {
    let dir = temp_dir("cost-states");
    write_md(
        &dir,
        serde_json::json!([
            {"id": "vendor/m-est", "name": null, "input": 2.0, "output": 4.0,
             "cache_write": 0.0, "cache_read": 0.0},
            {"id": "vendor/m-partial", "name": null, "input": 2.0, "output": null}
        ]),
    );
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-06-01",
        "m-zero",
        3,
        counts(1000, 0, 0, 0),
        "2026-06-01T12:00:00Z",
        Some(0.0),
    );
    write_rollup(
        &history,
        "p1",
        "2026-06-01",
        "m-est",
        4,
        counts(1_000_000, 500_000, 0, 0),
        "2026-06-01T12:00:00Z",
        None,
    );
    write_rollup(
        &history,
        "p1",
        "2026-06-01",
        "m-partial",
        5,
        counts(1_000_000, 500_000, 0, 0),
        "2026-06-01T12:00:00Z",
        None,
    );
    write_rollup(
        &history,
        "p1",
        "2026-06-01",
        "m-none",
        6,
        counts(700, 0, 0, 0),
        "2026-06-01T12:00:00Z",
        None,
    );

    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 4);

    let all = drill(&o, EventFilter::default());
    assert_eq!(all.total, 4, "无下钻 = 每模型每日一条汇总行");

    for g in real_groups(&report) {
        let list = drill(
            &o,
            EventFilter {
                model: Some(g.key.clone()),
                ..Default::default()
            },
        );
        assert_eq!(list.total, 1, "{}", g.key);
        let v = row_json(&list.rows[0]);
        let meta = &v["rollup"];
        assert_eq!(meta["requests"].as_u64(), Some(g.requests));
        assert_eq!(meta["day"].as_str(), Some("2026-06-01"));
        assert_eq!(meta["cost_detail"].as_str(), Some("rollup_only"));
        assert_eq!(
            meta["total_cost_usd"].as_f64(),
            g.total_cost_usd,
            "{} 显示金额一致",
            g.key
        );
        assert_eq!(
            meta["source_cost_usd"].as_f64(),
            g.source_cost_usd,
            "{} 来源费用一致",
            g.key
        );
        assert_eq!(meta["source_cost_state"].as_str(), g.source_cost_state);
        assert_eq!(
            meta["has_estimated_cost"].as_bool(),
            Some(g.has_estimated_cost)
        );
        assert_eq!(
            meta["cost_partially_unknown"].as_bool(),
            Some(g.cost_partially_unknown)
        );
        assert_eq!(
            meta["unknown_tokens"]["input"].as_u64(),
            g.unknown_tokens.input.to_u64()
        );
        assert_eq!(
            meta["unknown_tokens"]["output"].as_u64(),
            g.unknown_tokens.output.to_u64()
        );
    }

    let zero = group_of(&report, "mzero");
    assert!(
        close(zero.total_cost_usd.unwrap(), 0.0),
        "显式 0 是有费用依据"
    );
    assert_eq!(zero.source_cost_state, Some("full"));
    let est = group_of(&report, "mest");
    assert!(close(est.total_cost_usd.unwrap(), 4.0), "1M×2 + 0.5M×4");
    assert!(est.has_estimated_cost);
    let partial = group_of(&report, "mpartial");
    assert!(close(partial.total_cost_usd.unwrap(), 2.0));
    assert!(partial.cost_partially_unknown);
    assert_eq!(partial.unknown_tokens.output, 500_000);
    let none = group_of(&report, "mnone");
    assert_eq!(none.total_cost_usd, None, "完全无费用依据 = 未知");
    assert_eq!(none.unknown_tokens.input, 700);
    std::fs::remove_dir_all(&dir).ok();
}

/// 跨查询刷新稳定的行身份（`row_id`）必须与会话绑定的分页游标（`cursor`）分离：
/// - 原生请求：同秒多条请求 `row_id` 互异；新建查询会话且顶部新增请求后，`cursor` 随
///   `qid` 变化，原请求的 `row_id` 保持不变；
/// - CCS 日汇总：新建查询会话且上方新增更晚日期汇总使 `ridx` 顺延后，`cursor` 随
///   `qid`/`ridx` 变化，原日汇总的 `row_id` 保持不变，且与原生请求前缀互斥。
#[test]
fn event_row_id_stays_stable_across_sessions_and_prepends_while_cursor_rotates() {
    let dir = temp_dir("stable-row-id-vs-cursor");
    write_md(&dir, opus_md());
    let claude = dir.join("claude");
    // 两条同秒原生请求 + 一条较早请求
    write_claude_log(
        &claude,
        "proj/s1.jsonl",
        &[
            claude_line(
                "s1",
                "m-same-1",
                "2026-05-29T10:00:00.100Z",
                "claude-opus-4-7",
                100,
                50,
            ),
            claude_line(
                "s1",
                "m-same-2",
                "2026-05-29T10:00:00.100Z",
                "claude-opus-4-7",
                120,
                60,
            ),
            claude_line(
                "s1",
                "m-early",
                "2026-05-29T09:00:00.000Z",
                "claude-opus-4-7",
                80,
                40,
            ),
        ],
    );
    let history = history_of(&dir);
    write_rollup(
        &history,
        "p1",
        "2026-05-27",
        "claude-opus-4-7",
        15,
        counts(1000, 500, 0, 0),
        "2026-05-27T12:00:00Z",
        Some(1.0),
    );
    write_rollup(
        &history,
        "p1",
        "2026-05-26",
        "claude-opus-4-7",
        8,
        counts(800, 400, 0, 0),
        "2026-05-26T12:00:00Z",
        Some(0.5),
    );

    let o = opts(&dir, Some(claude.clone()), GroupBy::Model, "Asia/Shanghai");
    let filter = EventFilter {
        limit: Some(50),
        ..Default::default()
    };
    let h1 = tokenscope::query::begin_query_handle(&o).unwrap();
    let list1 = tokenscope::query::query_events(&h1.query_id, &filter).unwrap();
    assert_eq!(list1.rows.len(), 5, "3 条原生请求 + 2 条 CCS 日汇总");

    let by_rid1 = |rid: &str| list1.rows.iter().find(|r| r.record_id == rid).unwrap();
    let by_day1 = |day: &str| {
        list1
            .rows
            .iter()
            .find(|r| r.rollup.as_ref().is_some_and(|m| m.day == day))
            .unwrap()
    };
    let same1_a = by_rid1("m-same-1");
    let same2_a = by_rid1("m-same-2");
    let early_a = by_rid1("m-early");
    let day27_a = by_day1("2026-05-27");
    let day26_a = by_day1("2026-05-26");

    assert!(same1_a.row_id.starts_with("event:"), "{}", same1_a.row_id);
    assert!(same2_a.row_id.starts_with("event:"), "{}", same2_a.row_id);
    assert_ne!(same1_a.row_id, same2_a.row_id, "同秒不同请求 row_id 互异");
    assert!(day27_a.row_id.starts_with("rollup:"), "{}", day27_a.row_id);
    assert!(day26_a.row_id.starts_with("rollup:"), "{}", day26_a.row_id);
    assert_ne!(day27_a.row_id, day26_a.row_id);

    // 模拟后台刷新前顶部新增更晚的原生请求与更晚日期的 CCS 日汇总（使旧行下标与 ridx 均顺延）
    write_claude_log(
        &claude,
        "proj/s2.jsonl",
        &[
            claude_line(
                "s2",
                "m-fresh-1",
                "2026-05-29T12:00:00.000Z",
                "claude-opus-4-7",
                200,
                90,
            ),
            claude_line(
                "s2",
                "m-fresh-2",
                "2026-05-29T11:00:00.000Z",
                "claude-opus-4-7",
                180,
                70,
            ),
        ],
    );
    write_rollup(
        &history,
        "p2",
        "2026-05-28",
        "claude-opus-4-7",
        30,
        counts(2000, 900, 0, 0),
        "2026-05-28T12:00:00Z",
        Some(2.0),
    );

    let h2 = tokenscope::query::begin_query_handle(&o).unwrap();
    assert_ne!(h1.query_id, h2.query_id);
    let list2 = tokenscope::query::query_events(&h2.query_id, &filter).unwrap();
    assert_eq!(list2.rows.len(), 8, "5 条原生请求 + 3 条 CCS 日汇总");

    let by_rid2 = |rid: &str| list2.rows.iter().find(|r| r.record_id == rid).unwrap();
    let by_day2 = |day: &str| {
        list2
            .rows
            .iter()
            .find(|r| r.rollup.as_ref().is_some_and(|m| m.day == day))
            .unwrap()
    };
    for (before, after, label) in [
        (same1_a, by_rid2("m-same-1"), "m-same-1"),
        (same2_a, by_rid2("m-same-2"), "m-same-2"),
        (early_a, by_rid2("m-early"), "m-early"),
        (day27_a, by_day2("2026-05-27"), "rollup 2026-05-27"),
        (day26_a, by_day2("2026-05-26"), "rollup 2026-05-26"),
    ] {
        assert_ne!(
            before.cursor, after.cursor,
            "{label}: 分页游标必须随新会话切换"
        );
        assert_eq!(
            before.row_id, after.row_id,
            "{label}: 跨刷新行身份 row_id 必须稳定不变"
        );
    }

    // 确认旧会话分页游标在新会话仍会被严格拒绝（分页安全边界未放宽）
    let stale_page = tokenscope::query::query_events(
        &h2.query_id,
        &EventFilter {
            before: Some(same1_a.cursor.clone()),
            limit: Some(50),
            ..Default::default()
        },
    );
    assert!(
        stale_page.is_err(),
        "旧会话游标在新会话必须仍返回 query_expired"
    );

    std::fs::remove_dir_all(&dir).ok();
}
