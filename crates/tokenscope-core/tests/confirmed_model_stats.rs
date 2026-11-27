//! 合成 WorkBuddy/Claude 日志、已导入 CCS 事实和离线价格目录；不读取用户目录。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use tokenscope::aggregate::{Group, GroupBy};
use tokenscope::history::{DailyRollupWrite, HistoryDb, ImportRunRecord, RollupConflictPolicy};
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::modelsdev::sync_with_body_for_tests;
use tokenscope::pricing::Pricing;
use tokenscope::query::{begin_query, query_events, query_summary};
use tokenscope::report::{EventFilter, EventRow, SummaryOptions, SummaryReport};

const DAY: &str = "2026-05-28";
const PRIOR_DAY: &str = "2026-05-27";
const HY3: &str = "tencent/hy3";
const HY3_FREE: &str = "tencent/hy3:free";
const GEMMA: &str = "google/gemma-4-26b-a4b-it";
const GEMMA_FREE: &str = "google/gemma-4-26b-a4b-it:free";

struct Fixture {
    root: PathBuf,
    opts: SummaryOptions,
}

impl Fixture {
    fn new() -> Self {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "model-stats-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let opts = SummaryOptions {
            by: GroupBy::Model,
            tz: Some("Asia/Shanghai".into()),
            claude_dir: Some(root.join("claude")),
            workbuddy_dir: Some(root.join("workbuddy")),
            codex_enabled: Some(false),
            workbuddy_ai_enabled: Some(false),
            cache_dir: Some(root.join("data")),
            pricing_index: Some(root.join("index.json")),
            pricing_path: Some(root.join("prices.toml")),
            openrouter_path: Some(root.join("or.json")),
            modelsdev_path: Some(root.join("md.json")),
            ..Default::default()
        };
        Self { root, opts }
    }

    fn write_modelsdev(&self, entries: serde_json::Value) {
        std::fs::write(
            self.root.join("md.json"),
            json!({ "v": 4, "synced_at": "2026-05-28T12:00:00Z", "entries": entries }).to_string(),
        )
        .unwrap();
    }

    fn write_workbuddy(&self) {
        let time: jiff::Timestamp = "2026-05-28T01:00:00Z".parse().unwrap();
        write_lines(
            &self.root.join("workbuddy/test-project/wb.jsonl"),
            &[json!({
                "type": "function_call", "id": "wb-row", "sessionId": "wb-session",
                "cwd": "C:/synthetic/wb", "timestamp": time.as_millisecond(),
                "providerData": { "messageId": "wb-msg", "model": "hy3", "rawUsage": {
                    "prompt_tokens": 1100, "completion_tokens": 200,
                    "prompt_tokens_details": { "cached_tokens": 100 }
                } }
            })],
        );
    }

    fn write_claude(&self, rows: &[serde_json::Value]) {
        write_lines(&self.root.join("claude/test-project/cc.jsonl"), rows);
    }

    fn seed_rollups(&self, rolls: &[DailyRollupWrite]) {
        let history = HistoryDb::open(&self.root.join("data/history.db")).unwrap();
        history
            .apply_import_batch(
                &[],
                rolls,
                RollupConflictPolicy::KeepExisting,
                &ImportRunRecord {
                    logical_source: "ccs".into(),
                    source_path: "synthetic".into(),
                    source_schema: "test".into(),
                    started_utc: "2026-05-28T12:00:00Z".into(),
                    status: "committed".into(),
                    detail: None,
                    import_revision: 2,
                },
                None,
            )
            .unwrap();
    }
}

fn claude(id: &str, ts: &str, model: &str, [i, o, w, r]: [u64; 4]) -> serde_json::Value {
    json!({
        "type": "assistant", "timestamp": ts, "sessionId": "cc-session",
        "message": { "id": id, "model": model, "usage": {
            "input_tokens": i, "output_tokens": o,
            "cache_creation_input_tokens": w, "cache_read_input_tokens": r,
        } }
    })
}

fn write_lines(path: &Path, rows: &[serde_json::Value]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text = rows.iter().map(ToString::to_string).collect::<Vec<_>>();
    std::fs::write(path, text.join("\n")).unwrap();
}

fn rollup(
    provider: &str,
    day: &str,
    model: &str,
    requests: u64,
    [input, output, cache_write, cache_read]: [u64; 4],
    cost: Option<f64>,
) -> DailyRollupWrite {
    DailyRollupWrite {
        logical_source: "ccs".into(),
        day: day.into(),
        source_tz: "Asia/Shanghai".into(),
        app: AgentKind::ClaudeCode,
        provider_id: provider.into(),
        model: model.into(),
        request_model: String::new(),
        pricing_model: String::new(),
        request_count: requests,
        tokens: TokenCounts {
            input,
            output,
            cache_write,
            cache_read,
            ..Default::default()
        },
        input_semantics: 2,
        source_cost_usd: cost,
        revision: 0,
    }
}

fn only_group(report: &SummaryReport) -> &Group {
    let groups: Vec<_> = report.groups.iter().filter(|g| g.key != "合计").collect();
    assert_eq!(
        groups.len(),
        1,
        "应只有一个模型组，实测 (key, label, requests): {:?}",
        groups
            .iter()
            .map(|g| (&g.key, &g.label, g.requests))
            .collect::<Vec<_>>()
    );
    groups[0]
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
}

fn reconcile(group: &Group, rows: &[EventRow]) {
    let adopted = rows.iter().filter(|row| row.covered_by_rollup.is_none());
    let (requests, tokens, fees) = adopted.fold((0, [0; 4], 0.0), |(n, mut t, fees), row| {
        let count = row.rollup.as_ref().map_or(1, |rollup| rollup.requests);
        for (sum, bucket) in
            t.iter_mut()
                .zip([&row.input, &row.output, &row.cache_write, &row.cache_read])
        {
            *sum += bucket.to_u64().unwrap();
        }
        (n + count, t, fees + row.cost_usd.unwrap_or(0.0))
    });
    assert_eq!(group.requests, requests);
    assert_eq!(tokens, group_tokens(group));
    close(group.total_cost_usd.unwrap(), fees);
}

fn group_tokens(group: &Group) -> [u64; 4] {
    [
        group.tokens.input.to_u64().unwrap(),
        group.tokens.output.to_u64().unwrap(),
        group.tokens.cache_write.to_u64().unwrap(),
        group.tokens.cache_read.to_u64().unwrap(),
    ]
}

/// v0.9.8（DN）：302ai 的 ID 抄写在目录里排前，也不得盖掉聚合表、图表与
/// CCS 日汇总共用的正式名；请求明细保留原始 ID，费用用量不被改写。
#[test]
fn formal_display_name_shared_by_aggregate_chart_and_rollup() {
    const OPUS: &str = "claude-opus-4-7";
    let f = Fixture::new();
    f.write_claude(&[claude(
        "opus-native",
        "2026-05-28T01:00:00Z",
        OPUS,
        [1500, 250, 0, 0],
    )]);
    f.seed_rollups(&[rollup("route", DAY, OPUS, 2, [2000, 400, 0, 0], Some(0.5))]);
    f.write_modelsdev(json!([
        {"id":"302ai/claude-opus-4-7", "name":"claude-opus-4-7", "input":3.0, "output":15.0, "cache_read":1.0},
        {"id":"anthropic/claude-opus-4-7", "name":"Claude Opus 4.7", "input":3.0, "output":15.0, "cache_read":1.0}
    ]));
    let handle = begin_query(&f.opts).unwrap();
    let report = query_summary(&handle.query_id).unwrap();
    let group = only_group(&report);
    assert_eq!(
        group.key, "anthropic/claudeopus47",
        "展示名不改变化既有确认别名产生的统计分组键"
    );
    assert_eq!(
        group.label.as_deref(),
        Some("Claude Opus 4.7"),
        "聚合表与图表共用的标签是正式名，不是 302ai 的 ID 抄写"
    );
    close(group.source_cost_usd.unwrap(), 0.5);

    let drill = query_events(
        &handle.query_id,
        &EventFilter {
            model: Some(OPUS.into()),
            ..Default::default()
        },
    )
    .unwrap();
    // 逐请求断言改为精确语义：原生行保留原始 ID；CCS 日行单独断言展示名与原始 ID。
    let native_rows: Vec<_> = drill.rows.iter().filter(|r| r.rollup.is_none()).collect();
    let rollup_rows: Vec<_> = drill.rows.iter().filter(|r| r.rollup.is_some()).collect();
    assert_eq!(native_rows.len(), 1, "需含一条原生请求明细");
    assert_eq!(rollup_rows.len(), 1, "需含一条 CCS 日汇总行");
    assert_eq!(
        native_rows[0].model, OPUS,
        "原生 Opus 请求明细必须是原始 ID，不写展示名也不写统计分组键"
    );
    let meta = rollup_rows[0].rollup.as_ref().unwrap();
    // 日行的 model 允许是统计分组键，但展示名与原始 ID 详情必须精确。
    assert_eq!(
        meta.label, "Claude Opus 4.7",
        "CCS 日汇总展示名与聚合表一致"
    );
    assert_eq!(meta.models, [OPUS], "CCS 日行保留原始模型 ID 详情");
    reconcile(group, &drill.rows);
    drop(handle);

    // v0.9.9（DN-C）：同一会话内接着验证裸 ID 命中几十个有价有名的渠道时，
    // 聚合表、图表共用的标签与 CCS 日行仍一致；下钻与对账口径不变。
    const ID48: &str = "claude-opus-4-8";
    let f2 = Fixture::new();
    f2.write_claude(&[claude(
        "opus-48-native",
        "2026-05-28T01:00:00Z",
        ID48,
        [1500, 250, 0, 0],
    )]);
    f2.seed_rollups(&[rollup("route", DAY, ID48, 2, [2000, 400, 0, 0], Some(0.5))]);
    let entries: Vec<serde_json::Value> = (0..12)
        .map(|i| {
            json!({
                "id": format!("relay-{i:02}/claude-opus-4-8"),
                "name": "Claude Opus 4.8",
                "input": 3.0, "output": 15.0, "cache_read": 1.0
            })
        })
        .collect();
    f2.write_modelsdev(json!(entries));
    let handle2 = begin_query(&f2.opts).unwrap();
    let report2 = query_summary(&handle2.query_id).unwrap();
    let group2 = only_group(&report2);
    assert_eq!(group2.key, "claudeopus48");
    assert_eq!(group2.label.as_deref(), Some("Claude Opus 4.8"));
    // 原生请求与 CCS 日行同日，按既有覆盖口径由日行取代，不重复计数。
    assert_eq!(group2.requests, 2);
    assert_eq!(group_tokens(group2), [2000, 400, 0, 0]);
    close(group2.source_cost_usd.unwrap(), 0.5);
    let drill2 = query_events(
        &handle2.query_id,
        &EventFilter {
            model: Some(ID48.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(drill2.total, 2, "原始 ID 下钻仍命中原生请求与 CCS 日行");
    let native2: Vec<_> = drill2.rows.iter().filter(|r| r.rollup.is_none()).collect();
    let rollups2: Vec<_> = drill2.rows.iter().filter(|r| r.rollup.is_some()).collect();
    assert_eq!(native2.len(), 1);
    assert_eq!(native2[0].model, ID48, "请求明细保留原始 ID");
    assert_eq!(rollups2.len(), 1);
    assert_eq!(
        rollups2[0].rollup.as_ref().unwrap().label,
        "Claude Opus 4.8"
    );
    assert_eq!(rollups2[0].rollup.as_ref().unwrap().models, [ID48]);
    reconcile(group2, &drill2.rows);
}

/// DN-C13：同末段统计映射进入 Group.key 后，原始型号自身的规范名仍须
/// 成为聚合表、图表与 CCS 日行的共同标签；身份、覆盖、明细与计价不变。
#[test]
fn same_leaf_mapping_keeps_original_name_across_summary_chart_and_rollup() {
    const CHILD: &str = "TOKENSCOPE_SAME_LEAF_LABEL_CHILD";
    const TEST: &str = "same_leaf_mapping_keeps_original_name_across_summary_chart_and_rollup";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "同末段标签回归需在独立查询会话命名空间运行：{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    const ORIGINAL: &str = "deepseek/deepseek-v4-flash";
    const ALIAS: &str = "sample-lab/deepseek-v4-flash";
    for scenario in ["single", "multiple", "alias-only"] {
        let f = Fixture::new();
        let mut rows = vec![claude(
            "same-leaf-original",
            "2026-05-28T01:00:00Z",
            ORIGINAL,
            [1000, 200, 0, 0],
        )];
        let mut rolls = if scenario == "alias-only" {
            vec![rollup(
                "sample-lab",
                "2026-05-29",
                ALIAS,
                1,
                [600, 120, 0, 0],
                Some(0.25),
            )]
        } else {
            vec![rollup(
                "deepseek",
                DAY,
                ORIGINAL,
                2,
                [2000, 400, 0, 0],
                Some(0.5),
            )]
        };
        if scenario != "single" {
            rows.push(claude(
                "same-leaf-alias",
                "2026-05-29T01:00:00Z",
                ALIAS,
                [500, 100, 0, 0],
            ));
        }
        if scenario == "multiple" {
            rolls.push(rollup(
                "sample-lab",
                "2026-05-29",
                ALIAS,
                1,
                [600, 120, 0, 0],
                Some(0.25),
            ));
            rows.reverse();
            rolls.reverse();
        }
        f.write_claude(&rows);
        f.seed_rollups(&rolls);
        f.write_modelsdev(json!([
            {"id":ORIGINAL, "name":"Snapshot Channel Flash",
             "canonical_model_id":ALIAS, "input":1.0, "output":2.0},
            {"id":ALIAS, "name":"Sample Lab Flash", "input":3.0, "output":4.0}
        ]));
        let handle = begin_query(&f.opts).unwrap();
        let report = query_summary(&handle.query_id).unwrap();
        let group = only_group(&report);
        assert_eq!(group.key, "samplelab/deepseekv4flash");
        assert_eq!(group.label.as_deref(), Some("DeepSeek V4 Flash"));

        if scenario == "alias-only" {
            let drill = query_events(
                &handle.query_id,
                &EventFilter {
                    model: Some(group.key.clone()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(drill.total, 3);
            let native = drill
                .rows
                .iter()
                .find(|r| r.rollup.is_none() && r.model == ORIGINAL)
                .unwrap();
            assert_eq!(native.model, ORIGINAL);
            let meta = drill
                .rows
                .iter()
                .find(|r| r.rollup.is_some())
                .unwrap()
                .rollup
                .as_ref()
                .unwrap();
            assert_eq!(meta.label, "DeepSeek V4 Flash");
            assert_eq!(meta.models, [ALIAS]);
            reconcile(group, &drill.rows);
            drop(handle);
            continue;
        }

        let multiple = scenario == "multiple";
        close(
            group.source_cost_usd.unwrap(),
            if multiple { 0.75 } else { 0.5 },
        );

        let mut expected_models: Vec<&str> = if multiple {
            vec![ORIGINAL, ALIAS]
        } else {
            vec![ORIGINAL]
        };
        expected_models.sort_unstable();
        for &raw in &expected_models {
            let drill = query_events(
                &handle.query_id,
                &EventFilter {
                    model: Some(raw.into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(drill.total, if multiple { 4 } else { 2 });
            let native = drill
                .rows
                .iter()
                .find(|r| r.rollup.is_none() && r.model == raw)
                .unwrap();
            assert_eq!(native.model, raw, "请求明细保留原始 ID");
            let meta = drill
                .rows
                .iter()
                .filter_map(|r| r.rollup.as_ref())
                .find(|m| m.models.contains(&raw.to_string()))
                .unwrap();
            assert_eq!(meta.label, "DeepSeek V4 Flash");
            assert_eq!(meta.models, [raw], "CCS 日行保留原始模型详情");
            if multiple && raw == ALIAS {
                assert_eq!(native.covered_by_rollup, None);
            } else {
                assert_eq!(
                    native.covered_by_rollup.as_deref(),
                    Some(if raw == ORIGINAL { DAY } else { "2026-05-29" }),
                    "覆盖仍按原始 canonical 身份键判定"
                );
            }
        }
        let all = query_events(
            &handle.query_id,
            &EventFilter {
                model: Some(ALIAS.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(all.total, if multiple { 4 } else { 2 });
        let mut rollup_models: Vec<_> = all
            .rows
            .iter()
            .filter_map(|r| r.rollup.as_ref().map(|m| m.models.clone()))
            .collect();
        rollup_models.sort();
        let mut expected_rollups: Vec<Vec<String>> = expected_models
            .iter()
            .map(|raw| vec![(*raw).to_string()])
            .collect();
        expected_rollups.sort();
        assert_eq!(rollup_models, expected_rollups);
        let fees: Vec<_> = all.rows.iter().filter_map(|r| r.cost_usd).collect();
        let expected_fees: Vec<f64> = if multiple {
            vec![0.0019, 0.0038, 0.25, 0.5]
        } else {
            vec![0.0038, 0.5]
        };
        assert_eq!(fees, expected_fees, "名称修复不改逐条计价与 CCS 来源费用");
        reconcile(group, &all.rows);
        drop(handle);
    }
}

#[test]
fn workbuddy_and_imported_hy3_are_one_statistics_group() {
    let f = Fixture::new();
    f.write_workbuddy();
    f.seed_rollups(&[rollup(
        "route",
        DAY,
        HY3_FREE,
        3,
        [3000, 600, 90, 200],
        Some(1.25),
    )]);
    f.write_modelsdev(json!([
        {"id":"tencent-tokenhub/hy3", "name":"Hy3", "input":2.0, "output":5.0, "cache_read":1.0},
        {"id":"openrouter/tencent/hy3:free", "name":"Hy3", "input":0.0, "output":0.0, "cache_read":0.0}
    ]));
    let handle = begin_query(&f.opts).unwrap();
    let report = query_summary(&handle.query_id).unwrap();
    let group = only_group(&report);
    assert_eq!(
        (group.key.as_str(), group.label.as_deref()),
        (HY3, Some("Hy3"))
    );
    assert_eq!(group.requests, 4);
    assert_eq!(group_tokens(group), [4000, 800, 90, 300]);
    close(group.cost_usd, 0.0031);
    close(group.source_cost_usd.unwrap(), 1.25);

    for id in ["hy3", HY3, HY3_FREE] {
        let drill = query_events(
            &handle.query_id,
            &EventFilter {
                model: Some(id.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(drill.total, 2);
        let [native, ccs] = drill.rows.as_slice() else {
            panic!("需要原生请求和 CCS 日行")
        };
        assert_eq!((native.agent, native.model.as_str()), ("workbuddy", "hy3"));
        assert_eq!(native.covered_by_rollup, None);
        let meta = ccs.rollup.as_ref().unwrap();
        assert_eq!(
            (meta.source.as_str(), meta.day.as_str(), meta.requests),
            ("ccs", DAY, 3)
        );
        assert_eq!(meta.label, "Hy3");
        assert_eq!(meta.models, [HY3_FREE]);
        reconcile(group, &drill.rows);
    }
}

#[test]
fn hy3_offline_costs_and_coverage_stay_per_record_before_grouping() {
    let f = Fixture::new();
    f.write_workbuddy();
    f.write_claude(&[
        claude(
            "covered",
            "2026-05-28T02:00:00Z",
            HY3_FREE,
            [100, 50, 0, 10],
        ),
        claude("paid", "2026-05-28T03:00:00Z", HY3, [700, 300, 0, 40]),
        claude("unknown", "2026-05-28T04:00:00Z", HY3, [0, 0, 150, 0]),
        claude("free", "2026-05-28T13:00:00Z", HY3_FREE, [200, 80, 5, 20]),
    ]);
    // 没有线上快照；外置费率仅用于费用对照。普通档没有缓存写费率；显式
    // 免费档有四个 0，不得把计费档归并后的基础分组键送入估算。
    std::fs::write(
        f.root.join("prices.toml"),
        r#"
[[model]]
prefix = "tencent/hy3"
input = 2.0
output = 5.0
cache_read = 1.0

[[model]]
prefix = "tencent/hy3:free"
input = 0.0
output = 0.0
cache_write = 0.0
cache_read = 0.0
"#,
    )
    .unwrap();
    f.seed_rollups(&[
        rollup("paid-discarded", DAY, HY3, 1, [700, 300, 0, 40], Some(99.0)),
        rollup(
            "free-with-source",
            DAY,
            HY3_FREE,
            3,
            [3000, 600, 90, 200],
            Some(1.25),
        ),
        rollup("paid-prior", PRIOR_DAY, HY3, 2, [600, 100, 0, 40], None),
        rollup(
            "free-prior",
            PRIOR_DAY,
            HY3_FREE,
            2,
            [500, 50, 10, 20],
            None,
        ),
    ]);
    let handle = begin_query(&f.opts).unwrap();
    let report = query_summary(&handle.query_id).unwrap();
    let group = only_group(&report);
    assert_eq!(
        (group.key.as_str(), group.label.as_deref()),
        (HY3, Some("Hy3"))
    );
    assert_eq!(group.requests, 11);
    assert_eq!(group_tokens(group), [6000, 1330, 255, 420]);
    assert_eq!(group.unknown_tokens.cache_write, 150);
    assert_eq!(group.unknown_tokens.total(), 150);
    assert!(group.cost_partially_unknown);
    close(group.cost_usd, 0.00778);
    close(group.source_cost_usd.unwrap(), 1.25);
    close(group.total_cost_usd.unwrap(), 1.25778);
    assert_eq!(report.rollup_coverage.detail_selected_buckets, 1);
    assert_eq!(report.rollup_coverage.covered_buckets, 1);
    assert_eq!(report.rollup_coverage.rollup_buckets, 2);

    // 同会话每页一行，轮换三种 ID，从请求游标跨入日行再以日行游标续页。
    let mut before = None;
    let mut rows = Vec::new();
    for id in ["hy3", HY3_FREE, HY3].into_iter().cycle().take(7) {
        let page = query_events(
            &handle.query_id,
            &EventFilter {
                model: Some(id.into()),
                limit: Some(1),
                before,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page.total, 7);
        let [row] = page.rows.as_slice() else {
            panic!("每页恰好一行")
        };
        assert!(
            !rows
                .iter()
                .any(|earlier: &EventRow| earlier.cursor == row.cursor)
        );
        before = Some(row.cursor.clone());
        rows.push(row.clone());
    }
    let last = query_events(
        &handle.query_id,
        &EventFilter {
            model: Some(HY3.into()),
            before: before.clone(),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(last.rows.is_empty());
    for filter in [
        EventFilter {
            model: Some("tencent/hy3-preview".into()),
            before: before.clone(),
            ..Default::default()
        },
        EventFilter {
            model: Some(HY3.into()),
            day: Some(DAY.into()),
            before,
            ..Default::default()
        },
    ] {
        assert!(
            query_events(&handle.query_id, &filter).is_err(),
            "不同子查询不能续页"
        );
    }
    let detail = |id| rows.iter().find(|r| r.record_id == id).unwrap();
    let paid = detail("paid");
    assert_eq!(
        (paid.model.as_str(), paid.covered_by_rollup.as_deref()),
        (HY3, None)
    );
    close(paid.cost_usd.unwrap(), 0.00294);
    let free = detail("free");
    assert_eq!(
        (free.model.as_str(), free.covered_by_rollup.as_deref()),
        (HY3_FREE, None)
    );
    assert_eq!(free.cost_usd, Some(0.0));
    let free_cost = free.cost_breakdown.as_ref().unwrap();
    assert!(free_cost.complete && !free_cost.cache_price_zero_estimated);
    assert_eq!(free_cost.unknown.total(), 0);
    assert_eq!(detail("covered").covered_by_rollup.as_deref(), Some(DAY));
    // 免费档的快照前原生请求被标记，同应用同日的普通档仍按原始贡献独立计入。
    let unknown = detail("unknown");
    assert_eq!(unknown.covered_by_rollup, None);
    let unknown_cost = unknown.cost_breakdown.as_ref().unwrap();
    assert!(!unknown_cost.complete);
    assert_eq!(unknown_cost.unknown.cache_write, 150);
    let [day, prior] = &rows[5..] else {
        panic!("需要两天日汇总")
    };
    let day_meta = day.rollup.as_ref().unwrap();
    assert_eq!(day.ts, DAY);
    assert_eq!(
        day_meta.models,
        [HY3_FREE],
        "当天未采用的普通档日行不能泄漏到下钻"
    );
    assert_eq!(day_meta.requests, 3);
    assert_eq!(day_meta.source_cost_usd, Some(1.25));
    let prior_meta = prior.rollup.as_ref().unwrap();
    assert_eq!(prior.ts, PRIOR_DAY);
    assert_eq!(
        prior_meta.models,
        [HY3, HY3_FREE],
        "同日已采用的两档合成一日行"
    );
    assert_eq!(prior_meta.requests, 4);
    close(prior_meta.total_cost_usd.unwrap(), 0.00174);
    reconcile(group, &rows);

    let day_drill = query_events(
        &handle.query_id,
        &EventFilter {
            model: Some(HY3_FREE.into()),
            day: Some(DAY.into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(day_drill.total, 6);
    assert!(day_drill.rows.iter().all(|row| row.ts.starts_with(DAY)));

    for (agent, date, expected_requests, expected_tokens, expected_cost, drill_rows) in [
        (AgentKind::WorkBuddy, DAY, 1, [1000, 200, 0, 100], 0.0031, 1),
        (
            AgentKind::ClaudeCode,
            DAY,
            6,
            [3900, 980, 245, 260],
            1.25294,
            5,
        ),
        (
            AgentKind::ClaudeCode,
            PRIOR_DAY,
            4,
            [1100, 150, 10, 60],
            0.00174,
            1,
        ),
    ] {
        let scoped = begin_query(&SummaryOptions {
            agent: Some(agent),
            from: Some(date.into()),
            to: Some(date.into()),
            ..f.opts.clone()
        })
        .unwrap();
        let summary = query_summary(&scoped.query_id).unwrap();
        let group = only_group(&summary);
        assert_eq!(
            (group.key.as_str(), group.label.as_deref()),
            (HY3, Some("Hy3"))
        );
        assert_eq!(group.requests, expected_requests);
        assert_eq!(group_tokens(group), expected_tokens);
        close(group.total_cost_usd.unwrap(), expected_cost);
        let drill = query_events(
            &scoped.query_id,
            &EventFilter {
                model: Some(HY3.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(drill.total, drill_rows);
        assert!(
            drill
                .rows
                .iter()
                .all(|row| row.agent == agent.as_str() && row.ts.starts_with(date))
        );
        reconcile(group, &drill.rows);
    }
}

#[test]
fn gemma_catalog_base_model_is_explicit_and_independent_of_channel_count_or_order() {
    let f = Fixture::new();
    let snapshot = f.opts.modelsdev_path.as_deref().unwrap();
    let routes = [GEMMA, GEMMA_FREE, "deepinfra/google/gemma-4-26B-A4B-it"];
    // models.dev 真实形状（数值是合成费率）；正常同步保留 canonical_model_id。
    let official = json!({"name":"Gemma 4 26B A4B", "canonical_model_id":GEMMA, "cost":{"input":2.0,"output":5.0}});
    let free = json!({"name":"Gemma 4 26B A4B (free)", "canonical_model_id":GEMMA, "cost":{"input":0.0,"output":0.0}});
    let body = json!({
        "openrouter":{"models":{GEMMA: official, GEMMA_FREE: free}},
        "deepinfra":{"models":{"google/gemma-4-26B-A4B-it": official}},
        "other":{"models":{"gemma-4-26b-a4b-it":{"name":"Gemma 4 26B A4B","cost":{"input":1.0}}}}
    });
    sync_with_body_for_tests(snapshot, &body.to_string()).unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(snapshot).unwrap()).unwrap();
    let mut entries = saved["entries"].as_array().unwrap().clone();
    assert_eq!(
        entries
            .iter()
            .filter(|e| e["canonical_model_id"] == GEMMA)
            .count(),
        3
    );

    f.write_claude(
        &routes
            .iter()
            .enumerate()
            .map(|(i, route)| {
                claude(
                    &format!("gemma-{i}"),
                    &format!("2026-05-28T01:0{i}:00Z"),
                    route,
                    [1000, 200, 0, 0],
                )
            })
            .collect::<Vec<_>>(),
    );
    let key = "google/gemma426ba4bit";
    for iteration in 0..3 {
        let handle = begin_query(&f.opts).unwrap();
        let summary = query_summary(&handle.query_id).unwrap();
        let group = only_group(&summary);
        assert_eq!(group.key, key);
        // 展示名取自公开规范目录（models.json），不随合成快照里的渠道写法变化。
        assert_eq!(group.label.as_deref(), Some("Gemma 4 26B A4B IT"));
        assert_eq!(group.requests, 3);
        let list = query_events(
            &handle.query_id,
            &EventFilter {
                model: Some(GEMMA_FREE.into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(list.total, 3);
        assert_eq!(
            list.rows
                .iter()
                .map(|r| r.model.as_str())
                .collect::<Vec<_>>(),
            routes.into_iter().rev().collect::<Vec<_>>()
        );
        let fees = list
            .rows
            .iter()
            .map(|r| r.cost_usd.unwrap())
            .collect::<Vec<_>>();
        assert_eq!(fees, [0.003, 0.0, 0.003]);
        reconcile(group, &list.rows);
        if iteration == 0 {
            for i in 0..24 {
                entries.push(json!({
                    "id":format!("noise-{i}/gemma-4-26b-a4b-it"),
                    "name":"Gemma 4 26B A4B", "input":2.0, "output":5.0
                }));
            }
        }
        entries.reverse();
        f.write_modelsdev(json!(entries));
    }
    let (p, _) = Pricing::load(None, Some(snapshot), None);
    assert_eq!(p.model_stats_key(key), key);
    assert_eq!(
        p.model_stats_key("other/gemma-4-26b-a4b-it"),
        "other/gemma426ba4bit"
    );
    assert_eq!(
        p.model_stats_key("gemma-4-26b-a4b-it"),
        "gemma426ba4bit",
        "不能从末段猜测裸名归属"
    );

    // 离线无快照时，对无明确固定映射的第二组保守不合并；Hy3 与 Fable 照常。
    let offline = Pricing::empty();
    assert_ne!(
        offline.model_stats_key(GEMMA),
        offline.model_stats_key(GEMMA_FREE)
    );
    assert_eq!(
        offline.model_stats_key("hy3"),
        offline.model_stats_key(HY3_FREE)
    );
    assert_eq!(
        offline.model_stats_key("claude-fable-5-2"),
        offline.model_stats_key("anthropic/claude-fable-5.2")
    );
}

#[test]
fn catalogue_does_not_merge_unconfirmed_versions_variants_or_conflicting_claims() {
    let f = Fixture::new();
    f.write_modelsdev(json!([
        {"id":"openrouter/tencent/hy3-preview", "name":"Hy3", "canonical_model_id":HY3},
        {"id":"openrouter/tencent/hy3-20260706", "name":"Hy3", "canonical_model_id":HY3},
        {"id":"openrouter/tencent/hy3:thinking", "name":"Hy3", "canonical_model_id":HY3},
        {"id":"openrouter/tencent/hy3:", "name":"Hy3", "canonical_model_id":HY3},
        {"id":"mirror/hy3:free", "name":"Hy3", "canonical_model_id":false},
        {"id":"mirror/hy3", "name":"Hy3"},
        {"id":"only/example", "name":"Example"},
        {"id":"openrouter/demo/ambiguous:free", "canonical_model_id":"demo/ambiguous"},
        {"id":"openrouter/demo/am-biguous:free", "canonical_model_id":"other/ambiguous"},
        {"id":"openrouter/demo/chained:free", "canonical_model_id":"demo/chained"},
        {"id":"openrouter/demo/chained", "canonical_model_id":"other/chained"}
    ]));
    let (p, warnings) = Pricing::load(None, f.opts.modelsdev_path.as_deref(), None);
    assert!(warnings.is_empty(), "无效展示元数据不能使价格表失效");
    for (model, expected) in [
        ("tencent/hy3-preview", "tencent/hy3preview"),
        ("tencent/hy3-20260706", "tencent/hy320260706"),
        ("tencent/hy3:thinking", "tencent/hy3:thinking"),
        ("tencent/hy3:", "tencent/hy3:"),
        ("mirror/hy3:free", "mirror/hy3:free"),
        ("mirror/hy3", "mirror/hy3"),
        ("example", "example"),
        ("demo/ambiguous:free", "demo/ambiguous:free"),
        ("demo/chained:free", "demo/chained:free"),
    ] {
        assert_eq!(p.model_stats_key(model), expected, "{model}");
    }
}

#[test]
fn stats_metadata_survives_restart_without_rebuilding_price_index() {
    const STAGE: &str = "TOKENSCOPE_STATS_INDEX_STAGE";
    const DIRECTORY: &str = "TOKENSCOPE_STATS_INDEX_DIR";
    const TEST: &str = "stats_metadata_survives_restart_without_rebuilding_price_index";
    if let Ok(stage) = std::env::var(STAGE) {
        let dir = PathBuf::from(std::env::var_os(DIRECTORY).expect("子进程必须使用隔离根"));
        let md = dir.join("md.json");
        let idx = dir.join("index.json");
        let (p, _, warnings, hit) = Pricing::load_cached_revision(None, Some(&md), None, &idx);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(hit, stage == "restart");
        let expected = "google/gemma426ba4bit";
        for spelling in [GEMMA, GEMMA_FREE, expected] {
            assert_eq!(p.model_stats_key(spelling), expected);
        }
        let at = "2026-05-28T01:00:00Z".parse().unwrap();
        let tokens = TokenCounts {
            input: 1000,
            output: 200,
            ..Default::default()
        };
        assert_eq!(p.estimate(GEMMA_FREE, &tokens, at).unwrap().cost, 0.0);
        close(p.estimate(GEMMA, &tokens, at).unwrap().cost, 0.003);
        return;
    }
    let f = Fixture::new();
    f.write_modelsdev(json!([
        {"id":format!("openrouter/{GEMMA}"),"canonical_model_id":GEMMA,"input":2.0,"output":5.0},
        {"id":format!("openrouter/{GEMMA_FREE}"),"canonical_model_id":GEMMA,"input":0.0,"output":0.0}
    ]));
    let mut seed_index = Vec::new();
    for stage in ["seed", "restart"] {
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(STAGE, stage)
            .env(DIRECTORY, &f.root)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{stage}: {}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let index = std::fs::read(f.root.join("index.json")).unwrap();
        if stage == "seed" {
            seed_index = index;
            assert!(!String::from_utf8_lossy(&seed_index).contains("canonical_model_id"));
        } else {
            assert_eq!(
                index, seed_index,
                "展示键必须可从旧版价格索引冷启动，不重建或改写报价索引"
            );
        }
    }
}

#[test]
fn invalid_modelsdev_version_cannot_supply_aliases_on_cold_or_index_load() {
    const STAGE: &str = "TOKENSCOPE_INVALID_ALIAS_STAGE";
    const ROOT: &str = "TOKENSCOPE_INVALID_ALIAS_ROOT";
    const TEST: &str = "invalid_modelsdev_version_cannot_supply_aliases_on_cold_or_index_load";
    if let Ok(stage) = std::env::var(STAGE) {
        let root = PathBuf::from(std::env::var_os(ROOT).expect("必须提供隔离根"));
        let (p, _, warnings, hit) = Pricing::load_cached_revision(
            None,
            Some(&root.join("md.json")),
            None,
            &root.join("index.json"),
        );
        assert_eq!(hit, stage == "restart");
        assert!(
            warnings.iter().any(|w| w.contains("models.dev")),
            "无效 v 必须仍有加载警告"
        );
        assert_eq!(
            p.model_stats_key(GEMMA_FREE),
            "google/gemma426ba4bit:free",
            "有加载警告的无效快照不可贡献别名：{stage}"
        );
        assert_ne!(p.model_stats_key(GEMMA_FREE), p.model_stats_key(GEMMA));
        assert_eq!(p.model_stats_key("hy3"), p.model_stats_key(HY3_FREE));
        assert_eq!(
            p.model_stats_key("claude-fable-5-2"),
            p.model_stats_key("anthropic/claude-fable-5.2")
        );
        return;
    }
    let f = Fixture::new();
    std::fs::write(f.root.join("md.json"), json!({"v":"nonsense", "synced_at":"synthetic", "entries":[
        {"id":format!("openrouter/{GEMMA}"), "canonical_model_id":GEMMA, "input":2.0,"output":5.0},
        {"id":format!("openrouter/{GEMMA_FREE}"), "canonical_model_id":GEMMA,"input":0.0,"output":0.0}
    ]}).to_string()).unwrap();
    let mut failed = Vec::new();
    for stage in ["cold", "restart"] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env(STAGE, stage)
            .env(ROOT, &f.root)
            .output()
            .unwrap();
        if !output.status.success() {
            failed.push(format!(
                "{stage}: {}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
}

#[test]
fn space_bunny_legacy_key_reproduction_uses_a_frozen_synthetic_query() {
    let f = Fixture::new();
    f.write_modelsdev(
        json!([{"id":"opencode-go/spacebunny","name":"Space Bunny","input":1.0,"output":2.0}]),
    );
    f.write_claude(&[claude(
        "space",
        "2026-05-28T01:00:00Z",
        "spacebunny",
        [100, 20, 0, 0],
    )]);
    let h = begin_query(&f.opts).unwrap();
    let read = |key: &str| {
        query_events(
            &h.query_id,
            &EventFilter {
                model: Some(key.into()),
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert_eq!(read("opencodego/spacebunny").total, 0);
    assert_eq!(read("spacebunny").total, 1);
    let migrated = tokenscope::query::query_events_with_legacy_model(
        &h.query_id,
        &EventFilter {
            model: Some("opencodego/spacebunny".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(migrated.total, 1);
    assert_eq!(
        migrated.model_filter_migration.unwrap().to.as_deref(),
        Some("spacebunny")
    );
    assert_eq!(
        read("opencodego/spacebunny").total,
        0,
        "普通查询仍严格匹配，不引入旧键兼容"
    );
}

#[test]
fn legacy_model_migration_distinguishes_empty_dates_and_uses_frozen_relations() {
    let f = Fixture::new();
    f.write_modelsdev(
        json!([{"id":"opencode-go/spacebunny","name":"Space Bunny","input":1.0,"output":2.0}]),
    );
    let native = claude(
        "space",
        "2026-05-28T01:00:00Z",
        "spacebunny",
        [100, 20, 0, 0],
    );
    f.write_claude(std::slice::from_ref(&native));
    let empty = begin_query(&SummaryOptions {
        from: Some(PRIOR_DAY.into()),
        to: Some(PRIOR_DAY.into()),
        ..f.opts.clone()
    })
    .unwrap();
    // 新请求令旧键歧义，但不能改变此前已冻结的关系。
    f.write_claude(&[
        native,
        claude(
            "prefixed",
            "2026-05-28T02:00:00Z",
            "opencode-go/spacebunny",
            [100, 20, 0, 0],
        ),
    ]);
    let current = begin_query(&f.opts).unwrap();
    let filter = EventFilter {
        model: Some("opencodego/spacebunny".into()),
        ..Default::default()
    };
    let old = tokenscope::query::query_events_with_legacy_model(&empty.query_id, &filter).unwrap();
    assert_eq!(old.total, 0);
    assert_eq!(
        old.model_filter_migration.unwrap().to.as_deref(),
        Some("spacebunny"),
        "日期为空不是旧键失效"
    );
    let ambiguous =
        tokenscope::query::query_events_with_legacy_model(&current.query_id, &filter).unwrap();
    assert_eq!(
        ambiguous.total, 2,
        "歧义只清模型过滤，正常返回原主筛选的明细"
    );
    assert!(ambiguous.model_filter_migration.unwrap().to.is_none());
    let missing = tokenscope::query::query_events_with_legacy_model(
        &current.query_id,
        &EventFilter {
            model: Some("no/such-model".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(missing.total, 2);
    assert!(missing.model_filter_migration.unwrap().to.is_none());
    assert!(
        tokenscope::query::query_events_with_legacy_model(
            &current.query_id,
            &EventFilter {
                before: Some(ambiguous.rows[0].cursor.clone()),
                ..filter
            }
        )
        .is_err()
    );
}

#[test]
fn malformed_optional_alias_keeps_prices_and_old_snapshot_compatibility() {
    let f = Fixture::new();
    for version in [1, 3, 4] {
        std::fs::write(f.root.join("md.json"), json!({"v":version,"synced_at":"synthetic","entries":[
            {"id":format!("openrouter/{GEMMA}"),"name":"Gemma","input":2.0,"output":5.0,"cache_read":0.0,"canonical_model_id":GEMMA},
            {"id":format!("openrouter/{GEMMA_FREE}"),"name":"Gemma free","input":0.0,"output":0.0,"cache_read":0.0,"canonical_model_id":false}
        ]}).to_string()).unwrap();
        let (p, warnings) = Pricing::load(None, f.opts.modelsdev_path.as_deref(), None);
        assert!(warnings.is_empty(), "可选字段坏值不影响其余费率");
        assert_eq!(p.modelsdev_count(), 2);
        assert_ne!(p.model_stats_key(GEMMA), p.model_stats_key(GEMMA_FREE));
        let t = TokenCounts {
            input: 1000,
            output: 200,
            ..Default::default()
        };
        let at = "2026-05-28T01:00:00Z".parse().unwrap();
        close(p.estimate(GEMMA, &t, at).unwrap().cost, 0.003);
        assert_eq!(p.estimate(GEMMA_FREE, &t, at).unwrap().cost, 0.0);
        assert_eq!(
            p.display_name_for(GEMMA).as_deref(),
            Some("Gemma 4 26B A4B IT"),
            "展示名独立于快照版本与可选别名字段"
        );
        let rate = p.lookup(GEMMA).unwrap().plan.base.cache_read;
        assert_eq!(
            rate,
            if version == 1 {
                tokenscope::pricing::RateSpec::Unknown
            } else {
                tokenscope::pricing::RateSpec::Fixed(0.0)
            }
        );
    }
}

#[test]
fn migrated_first_page_cursor_resumes_with_the_new_key_only() {
    let f = Fixture::new();
    f.write_modelsdev(
        json!([{"id":"opencode-go/spacebunny","name":"Space Bunny","input":1.0,"output":2.0}]),
    );
    f.write_claude(&[
        claude(
            "space-1",
            "2026-05-28T01:00:00Z",
            "spacebunny",
            [100, 20, 0, 0],
        ),
        claude(
            "space-2",
            "2026-05-28T02:00:00Z",
            "spacebunny",
            [200, 40, 0, 0],
        ),
    ]);
    let h = begin_query(&f.opts).unwrap();
    let first = tokenscope::query::query_events_with_legacy_model(
        &h.query_id,
        &EventFilter {
            model: Some("opencodego/spacebunny".into()),
            limit: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(first.total, 2);
    let second = query_events(
        &h.query_id,
        &EventFilter {
            model: Some("spacebunny".into()),
            limit: Some(1),
            before: Some(first.rows[0].cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(second.rows.len(), 1);
    assert_ne!(first.rows[0].record_id, second.rows[0].record_id);
    assert!(
        second.model_filter_migration.is_none(),
        "普通续页不重复迁移"
    );
}
