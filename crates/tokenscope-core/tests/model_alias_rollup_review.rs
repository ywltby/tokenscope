//! v0.9.4 独立复核补修一/二：真实多渠道价格快照下的确认别名，以及
//! 「未知项目」下钻纳入 CCS 日汇总。
//!
//! 计划：docs/plans/active/2026-11-19-v094-model-alias-rollup-drilldown.md §8。
//! 红-first：补修一的夹具放**24 个**末段同键的渠道候选（本机真实快照形态），
//! 旧实现的「唯一候选才确认」分支在此放弃归一；补修二的「未知」组下钻在旧
//! 实现里被 `filter.project.is_none()` 整体排除日行。
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
use tokenscope::report::{EventFilter, EventList, EventRow, SummaryOptions, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// 本机真实快照形态：末段归一为 `claudefable52` 的渠道 ID 共 24 个。
const FABLE_CHANNELS: [&str; 24] = [
    "anthropic",
    "openrouter",
    "zenmux",
    "deepinfra",
    "bedrock",
    "vertex",
    "azure",
    "groq",
    "together",
    "fireworks",
    "perplexity",
    "xai",
    "mistral",
    "cohere",
    "ai21",
    "nlpcloud",
    "replicate",
    "huggingface",
    "sambanova",
    "cerebras",
    "lambda",
    "ovhcloud",
    "scaleway",
    "upstage",
];

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-alias-review-{tag}-{}-{n}-{}",
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

fn write_md(dir: &Path, entries: serde_json::Value) {
    let snap = serde_json::json!({
        "v": 4,
        "synced_at": "2026-11-19T00:00:00Z",
        "entries": entries,
    });
    std::fs::write(dir.join("md.json"), snap.to_string()).unwrap();
}

/// 多渠道 Fable 快照：每个渠道一条同末段候选（价格一致，名字一致）。
fn fable_channels_md(channels: &[&str]) -> serde_json::Value {
    serde_json::Value::Array(
        channels
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": format!("{c}/claude-fable-5.2"),
                    "name": "Claude Fable 5.2",
                    "input": 3.0, "output": 15.0, "cache_write": 3.75, "cache_read": 0.3
                })
            })
            .collect(),
    )
}

fn opus_md() -> serde_json::Value {
    serde_json::json!([{
        "id": "anthropic/claude-opus-4-7",
        "name": "Claude Opus 4.7",
        "input": 3.0, "output": 15.0, "cache_write": 3.75, "cache_read": 0.3
    }])
}

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

fn is_rollup(row: &EventRow) -> bool {
    row_json(row).get("rollup").is_some()
}

fn real_groups(report: &tokenscope::report::SummaryReport) -> Vec<&tokenscope::aggregate::Group> {
    report.groups.iter().filter(|g| g.key != "合计").collect()
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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// 五条 Fable 请求：两种渠道写法混用（2 条带前缀 + 3 条裸名）。
fn write_fable_requests(dir: &Path) -> PathBuf {
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
    claude
}

// ── 补修一：确认别名优先于价格目录唯一候选 ─────────────────────────────

/// 24 个同末段渠道候选（真实快照形态）下，已核实的两种写法必须仍是一组：
/// 表格/图表同一行，下钻两种原始 ID 都命中全部请求，四桶与逐请求费用对账。
#[test]
fn fable_merges_under_multi_channel_price_snapshot() {
    let dir = temp_dir("fable-24");
    write_md(&dir, fable_channels_md(&FABLE_CHANNELS));
    let claude = write_fable_requests(&dir);
    let o = opts(&dir, Some(claude), GroupBy::Model, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        1,
        "24 候选下确认别名仍须一组：{:?}",
        report
            .groups
            .iter()
            .map(|g| (&g.key, &g.label))
            .collect::<Vec<_>>()
    );
    let g = real_groups(&report)[0];
    assert_eq!(g.key, "anthropic/claudefable52");
    assert_eq!(g.label.as_deref(), Some("Claude Fable 5.2"));
    assert_eq!(g.requests, 5);
    assert_eq!(g.tokens.input, 5000);
    assert_eq!(g.tokens.output, 2500);
    // 每请求 (1000×3 + 500×15)/1e6 = 0.0105，5 请求 = 0.0525。
    assert!(close(g.cost_usd, 0.0525), "{}", g.cost_usd);
    assert!(close(g.total_cost_usd.unwrap(), 0.0525));

    // 下钻：规范键与两种原始写法都命中全部 5 行，行内保留原始 ID。
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
        assert_eq!(list.total, 5, "{spelling} 下钻条数");
        assert_eq!(list.rows.len(), 5);
        let mut spellings: Vec<&str> = list.rows.iter().map(|r| r.model.as_str()).collect();
        spellings.sort_unstable();
        assert_eq!(
            spellings,
            vec![
                "anthropic/claude-fable-5.2",
                "anthropic/claude-fable-5.2",
                "claude-fable-5-2",
                "claude-fable-5-2",
                "claude-fable-5-2",
            ],
            "明细必须保留各自原始 ID"
        );
        let sum: f64 = list.rows.iter().filter_map(|r| r.cost_usd).sum();
        assert!(close(sum, g.cost_usd), "{spelling} 逐请求费用之和 {sum}");
        assert!(
            list.rows.iter().all(|r| !is_rollup(r)),
            "纯原生模型下钻不得出现日汇总行"
        );
    }
    std::fs::remove_dir_all(&dir).ok();
}

/// 确认别名与价格目录无关：候选删减、乱序、以及**完全没有价格快照**时，
/// 这一对已核实写法仍归一组（其余模型不受影响）。
#[test]
fn confirmed_alias_is_independent_of_price_catalog() {
    // (a) 候选删减到 3 个渠道，且**不含** anthropic 条目。
    let dir = temp_dir("fable-few");
    write_md(&dir, fable_channels_md(&["zenmux", "deepinfra", "groq"]));
    let claude = write_fable_requests(&dir);
    let o = opts(&dir, Some(claude), GroupBy::Model, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        1,
        "候选删减后仍一组：{:?}",
        report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
    );
    assert_eq!(real_groups(&report)[0].key, "anthropic/claudefable52");
    std::fs::remove_dir_all(&dir).ok();

    // (b) 24 候选乱序（anthropic 不在首位）。
    let dir = temp_dir("fable-shuffled");
    let mut channels = FABLE_CHANNELS.to_vec();
    channels.rotate_left(7);
    channels.reverse();
    write_md(&dir, fable_channels_md(&channels));
    let claude = write_fable_requests(&dir);
    let o = opts(&dir, Some(claude), GroupBy::Model, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 1, "候选乱序后仍一组");
    assert_eq!(real_groups(&report)[0].requests, 5);
    std::fs::remove_dir_all(&dir).ok();

    // (c) 没有 models.dev 快照：确认别名仍生效，展示名退回确定性原始 ID。
    let dir = temp_dir("fable-nosnap");
    let claude = write_fable_requests(&dir);
    let o = opts(&dir, Some(claude), GroupBy::Model, "Asia/Shanghai");
    let report = summary(&o).unwrap();
    assert_eq!(
        real_groups(&report).len(),
        1,
        "无快照时确认别名仍一组：{:?}",
        report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
    );
    let g = real_groups(&report)[0];
    assert_eq!(g.key, "anthropic/claudefable52");
    assert_eq!(g.requests, 5);
    assert_eq!(g.tokens.input, 5000);
    // 无价格依据：金额未知，不伪装成 0。
    assert!(g.total_cost_usd.is_none(), "{:?}", g.total_cost_usd);
    let list = drill(
        &o,
        EventFilter {
            model: Some("claude-fable-5-2".into()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 5, "无快照下钻仍命中两种写法");
    std::fs::remove_dir_all(&dir).ok();
}

/// 确认映射只覆盖已核实的那一对：变体、日期后缀、未确认渠道前缀与
/// 仅展示名相同的其他模型各自独立（宁可少合并，不可误合并）。
#[test]
fn confirmed_alias_keeps_unconfirmed_identities_separate() {
    let dir = temp_dir("fable-boundaries");
    write_md(&dir, fable_channels_md(&FABLE_CHANNELS));
    let history = history_of(&dir);
    let variants = [
        "claude-fable-5-2:free",
        "claude-fable-5-2:thinking",
        "claude-fable-5-2:",
        "claude-fable-5-2-20261101",
        "zenmux/claude-fable-5-2",
        "claude-fable-5-3",
    ];
    for (i, model) in variants.iter().enumerate() {
        write_detail(
            &history,
            &format!("native|claude|/logs/b.jsonl|{i}|fp"),
            &format!("2026-11-10T09:0{i}:00Z"),
            model,
            counts(100, 0, 0, 0),
            Some("C:/w/b"),
        );
    }
    write_detail(
        &history,
        "native|claude|/logs/b.jsonl|9|fp",
        "2026-11-10T09:09:00Z",
        "claude-fable-5-2",
        counts(100, 0, 0, 0),
        Some("C:/w/b"),
    );
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Model,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    let keys: Vec<&str> = real_groups(&report)
        .iter()
        .map(|g| g.key.as_str())
        .collect();
    assert_eq!(
        keys.len(),
        variants.len() + 1,
        "未确认写法各自独立：{keys:?}"
    );
    for model in variants {
        let expected = ModelIdentity::parse(model).identity_key();
        assert!(
            keys.contains(&expected.as_str()),
            "{model} → {expected} 缺失：{keys:?}"
        );
        assert_ne!(
            expected, "anthropic/claudefable52",
            "{model} 不得并入确认别名"
        );
    }
    assert!(keys.contains(&"anthropic/claudefable52"));
    // 确认别名那一组只有它自己的 1 条请求。
    assert_eq!(group_of(&report, "anthropic/claudefable52").requests, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// 原生明细与 CCS 日汇总分别使用两种写法（两个方向）：覆盖桶按同一身份
/// 判定，B 类总量 = 汇总 + 快照后新增，不双计；被覆盖请求带标记。
#[test]
fn alias_coverage_uses_one_identity_in_both_directions() {
    for (native_model, ccs_model) in [
        ("claude-fable-5-2", "anthropic/claude-fable-5.2"),
        ("anthropic/claude-fable-5.2", "claude-fable-5-2"),
    ] {
        let dir = temp_dir("fable-coverage");
        // 价格全缺：只确认别名归属，不引入估算金额（总额 = 来源费用）。
        let mut entries = fable_channels_md(&FABLE_CHANNELS);
        for e in entries.as_array_mut().unwrap() {
            for k in ["input", "output", "cache_write", "cache_read"] {
                e[k] = serde_json::Value::Null;
            }
        }
        write_md(&dir, entries);
        let history = history_of(&dir);
        write_rollup(
            &history,
            "p1",
            "2026-11-10",
            ccs_model,
            10,
            counts(1000, 0, 0, 0),
            "2026-11-10T12:00:00Z",
            Some(1.5),
        );
        // 快照内裸名/前缀明细（应被覆盖）与快照后新增（独立计入）。
        write_detail(
            &history,
            "native|claude|/logs/a.jsonl|0|fp",
            "2026-11-10T01:00:00Z",
            native_model,
            counts(100, 0, 0, 0),
            None,
        );
        write_detail(
            &history,
            "native|claude|/logs/a.jsonl|1|fp",
            "2026-11-10T13:00:00Z",
            native_model,
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
            "{native_model} / {ccs_model} 必须同组：{:?}",
            report.groups.iter().map(|g| &g.key).collect::<Vec<_>>()
        );
        assert_eq!(report.totals.requests, 11, "汇总 10 + 快照后 1，不双计");
        assert_eq!(report.totals.tokens.input, 1200);
        assert!(close(report.totals.total_cost_usd.unwrap(), 1.5));

        let list = drill(
            &o,
            EventFilter {
                model: Some("anthropic/claudefable52".into()),
                ..Default::default()
            },
        );
        assert_eq!(list.total, 3, "2 条明细 + 1 条日汇总行");
        let covered = list
            .rows
            .iter()
            .find(|r| r.ts.starts_with("2026-11-10 09:"))
            .unwrap();
        assert_eq!(
            row_json(covered)
                .get("covered_by_rollup")
                .and_then(|v| v.as_str()),
            Some("2026-11-10"),
            "被覆盖请求必须标记计入当日汇总"
        );
        let post = list
            .rows
            .iter()
            .find(|r| r.ts.starts_with("2026-11-10 21:"))
            .unwrap();
        assert!(row_json(post).get("covered_by_rollup").is_none());
        let rollups: Vec<&EventRow> = list.rows.iter().filter(|r| is_rollup(r)).collect();
        assert_eq!(rollups.len(), 1);
        assert_eq!(
            row_json(rollups[0])["rollup"]["requests"].as_u64(),
            Some(10)
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}

// ── 补修二：「未知项目」下钻纳入 CCS 日汇总 ────────────────────────────

/// 两条 CCS 日汇总（无逐请求记录）：按项目聚合归入「未知」组，点该组
/// 下钻必须看到两条日行，日期/请求数/四桶/费用逐项与聚合对账。
#[test]
fn unknown_project_drill_returns_ccs_day_rows() {
    let dir = temp_dir("unknown-pure-ccs");
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
        GroupBy::Project,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    assert_eq!(real_groups(&report).len(), 1);
    let g = real_groups(&report)[0];
    assert_eq!(g.key, "(未知)");
    assert_eq!(g.requests, 234);
    assert_eq!(g.tokens.total(), 110_198_123);
    assert!(close(g.total_cost_usd.unwrap(), 43.75));

    let list = drill(
        &o,
        EventFilter {
            project: Some(g.key.clone()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 2, "「未知」组下钻必须含两条 CCS 日行");
    assert_eq!(list.rows.len(), 2);
    assert!(list.rows.iter().all(is_rollup), "纯 CCS：两行都是日汇总行");
    // 日降序；日粒度停留（不补造时分秒），项目列不补造归属。
    let days: Vec<&str> = list.rows.iter().map(|r| r.ts.as_str()).collect();
    assert_eq!(days, vec!["2026-05-29", "2026-05-28"]);
    for row in &list.rows {
        assert_eq!(row.project, "", "日汇总行不得补造项目归属");
        assert_eq!(row.record_id, "");
        assert_eq!(row.session_id, "");
    }
    let day28 = list.rows.iter().find(|r| r.ts == "2026-05-28").unwrap();
    let meta = &row_json(day28)["rollup"];
    assert_eq!(meta["requests"].as_u64(), Some(223));
    assert_eq!(meta["source"].as_str(), Some("ccs"));
    assert_eq!(day28.input, 100_000_000);
    assert_eq!(day28.output, 5_000_000);
    assert_eq!(day28.cache_write, 2_000_000);
    assert_eq!(day28.cache_read, 3_000_000);
    assert!(close(day28.cost_usd.unwrap(), 42.5));
    let day29 = list.rows.iter().find(|r| r.ts == "2026-05-29").unwrap();
    assert_eq!(row_json(day29)["rollup"]["requests"].as_u64(), Some(11));
    assert!(close(day29.cost_usd.unwrap(), 1.25));
    // 对账：Σ日行 == 分组 requests / 四桶 / 费用。
    let requests: u64 = list
        .rows
        .iter()
        .map(|r| row_json(r)["rollup"]["requests"].as_u64().unwrap())
        .sum();
    assert_eq!(requests, g.requests);
    let total: u64 = list
        .rows
        .iter()
        .map(|r| r.input + r.output + r.cache_write + r.cache_read)
        .sum();
    assert_eq!(total, g.tokens.total());
    let cost: f64 = list.rows.iter().filter_map(|r| r.cost_usd).sum();
    assert!(close(cost, g.total_cost_usd.unwrap()));
    std::fs::remove_dir_all(&dir).ok();
}

/// 「未知」组混有无项目归属的原生请求：下钻同时返回该请求与已采用日行，
/// Σ(未覆盖请求 + 日行) == 分组；具体项目下钻仍只匹配有归属的请求。
#[test]
fn unknown_project_drill_mixes_requests_and_rollups() {
    let dir = temp_dir("unknown-mixed");
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
    // 无项目归属、快照之后新增 → 独立计入（不被覆盖）。
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|0|fp",
        "2026-05-28T13:00:00Z",
        "claude-opus-4-7",
        counts(200, 0, 0, 0),
        None,
    );
    // 有项目归属的请求 → 只属于具体项目组。
    write_detail(
        &history,
        "native|claude|/logs/a.jsonl|1|fp",
        "2026-05-28T14:00:00Z",
        "claude-opus-4-7",
        counts(300, 0, 0, 0),
        Some("C:/w/a"),
    );
    let o = opts(
        &dir,
        Some(dir.join("no-claude")),
        GroupBy::Project,
        "Asia/Shanghai",
    );
    let report = summary(&o).unwrap();
    let unknown = group_of(&report, "(未知)");
    assert_eq!(unknown.requests, 235, "223 + 11 + 1 条无归属新增");
    let named = group_of(&report, "C:/w/a");
    assert_eq!(named.requests, 1);

    let list = drill(
        &o,
        EventFilter {
            project: Some("(未知)".into()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 3, "1 条无归属请求 + 2 条日行");
    let events: Vec<&EventRow> = list.rows.iter().filter(|r| !is_rollup(r)).collect();
    let rollups: Vec<&EventRow> = list.rows.iter().filter(|r| is_rollup(r)).collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].project, "(未知)");
    assert_eq!(events[0].input, 200);
    assert_eq!(rollups.len(), 2);
    let requests: u64 = events
        .iter()
        .map(|_| 1)
        .chain(
            rollups
                .iter()
                .map(|r| row_json(r)["rollup"]["requests"].as_u64().unwrap()),
        )
        .sum();
    assert_eq!(requests, unknown.requests, "Σ明细 + Σ日行 == 分组 requests");
    let tokens: u64 = list
        .rows
        .iter()
        .map(|r| r.input + r.output + r.cache_write + r.cache_read)
        .sum();
    assert_eq!(tokens, unknown.tokens.total());
    let cost: f64 = list.rows.iter().filter_map(|r| r.cost_usd).sum();
    assert!(close(cost, unknown.total_cost_usd.unwrap()), "{cost}");

    // 具体项目：只匹配有该项目归属的请求，不含任何日汇总行。
    let named_list = drill(
        &o,
        EventFilter {
            project: Some("C:/w/a".into()),
            ..Default::default()
        },
    );
    assert_eq!(named_list.total, 1);
    assert_eq!(named_list.rows.len(), 1);
    assert!(!is_rollup(&named_list.rows[0]));
    assert_eq!(named_list.rows[0].project, "C:/w/a");
    assert_eq!(named_list.rows[0].input, 300);
    std::fs::remove_dir_all(&dir).ok();
}

/// 按模型下钻的既有验收复跑：claude-opus-4-7 两条 CCS 日行不受项目判定影响。
#[test]
fn opus_model_drill_still_shows_two_ccs_days() {
    let dir = temp_dir("opus-model-regression");
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
    let g = real_groups(&report)[0];
    assert_eq!(g.requests, 234);
    assert_eq!(g.tokens.total(), 110_198_123);
    let list = drill(
        &o,
        EventFilter {
            model: Some(g.key.clone()),
            ..Default::default()
        },
    );
    assert_eq!(list.total, 2);
    assert!(list.rows.iter().all(is_rollup));
    let days: Vec<&str> = list.rows.iter().map(|r| r.ts.as_str()).collect();
    assert_eq!(days, vec!["2026-05-29", "2026-05-28"]);
    std::fs::remove_dir_all(&dir).ok();
}
