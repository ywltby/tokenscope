//! MP03/MP04：可信模型展示名 + 模型身份分组与下钻。
//!
//! 计划：docs/plans/active/2026-10-10-model-pricing-name-equivalence.md §2.12/§3。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::TokenCounts;
use tokenscope::model_identity::ModelIdentity;
use tokenscope::modelsdev::{load_snapshot, sync_with_body_for_tests};
use tokenscope::pricing::Pricing;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

static SEQ: AtomicUsize = AtomicUsize::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!("tokenscope-mp03-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        cache_write: cw,
        cache_read: cr,
    }
}

fn at() -> jiff::Timestamp {
    "2026-10-07T05:00:00Z".parse().unwrap()
}

fn sync_to(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    sync_with_body_for_tests(&path, body).unwrap();
    path
}

// ---- MP04：模型身份分组、下钻与守恒 ----

/// Claude 合成日志目录：每行一条 assistant 用量事件（同一会话、同一 cwd）。
fn claude_dir(dir: &Path, models: &[&str], input: u64) -> PathBuf {
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, model) in models.iter().enumerate() {
        let ts = format!("2026-10-07T05:{:02}:{:02}.000Z", i / 60, i % 60);
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"m-{i}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    dir.join("claude")
}

fn no_codex(dir: &Path) -> PathBuf {
    dir.join("no-codex")
}

/// 指定时间与模型的 Claude 合成事件（逐请求计费场景用）。
fn claude_events(dir: &Path, rows: &[(&str, &str, u64)]) -> PathBuf {
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, (ts, model, input)) in rows.iter().enumerate() {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"m-{i}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    dir.join("claude")
}

fn opts(dir: &Path, claude: &Path, modelsdev: Option<&Path>, by: GroupBy) -> SummaryOptions {
    SummaryOptions {
        by,
        claude_dir: Some(claude.to_path_buf()),
        codex_dir: Some(no_codex(dir)),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or-missing.json")),
        modelsdev_path: Some(
            modelsdev
                .map(Path::to_path_buf)
                .unwrap_or_else(|| dir.join("md-missing.json")),
        ),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

fn list_all(o: &SummaryOptions, filter: &EventFilter) -> tokenscope::report::EventList {
    let snap = tokenscope::query::begin_query(o).unwrap();
    tokenscope::query::query_events(&snap.query_id, filter).unwrap()
}

#[test]
fn model_identity_groups_aliases_across_agents() {
    let dir = tmp("groups");
    let claude = claude_dir(
        &dir,
        &["claude-opus-5-5", "claude-opus-5.5", "CLAUDE_OPUS_5_5"],
        10,
    );
    // 可信 models.dev 名（只挂在其中一个等价 ID 上）。
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"claude-opus-5.5": {"name": "Claude Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let r = summary(&opts(&dir, &claude, Some(&md), GroupBy::Model)).unwrap();
    assert_eq!(
        r.groups.len(),
        2,
        "三种拼写合并为一行 + 合计：{:?}",
        r.groups
    );
    let g = &r.groups[0];
    assert_eq!(g.key, "claudeopus55", "key 是等价身份键");
    assert_eq!(
        g.label.as_deref(),
        Some("Claude Opus 5.5"),
        "优先使用可信 models.dev 模型名"
    );
    assert_eq!(g.requests, 3, "三种拼写都计入同一模型");
    assert_eq!(g.tokens.input, 30);

    // 无可信名称时退回原始代表写法（字典序最小），不展示压缩键。
    let dir2 = tmp("groups-no-name");
    let claude2 = claude_dir(&dir2, &["claude-opus-5.5", "claude-opus-5-5"], 10);
    let r2 = summary(&opts(&dir2, &claude2, None, GroupBy::Model)).unwrap();
    assert_eq!(r2.groups.len(), 2);
    assert_eq!(r2.groups[0].key, "claudeopus55");
    assert_eq!(r2.groups[0].label.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn model_display_name_requires_trusted_source() {
    let dir = tmp("trusted-source");
    // models.dev 提供可信模型名；OpenRouter 的同等价 ID 只有渠道别名。
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "Trusted Model Name", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let orp = dir.join("openrouter.json");
    std::fs::write(
        &orp,
        r#"{"v":2,"synced_at":"t","entries":[{"id":"alias/opus5.5","name":"Channel Alias","prompt":0.000004,"completion":0.00002}]}"#,
    )
    .unwrap();
    let (p, _w) = Pricing::load(None, Some(&md), Some(&orp));
    assert_eq!(
        p.display_name_for("opus55").as_deref(),
        Some("Trusted Model Name"),
        "展示名只认可信来源（models.dev）的模型级 name"
    );

    // 只有 OpenRouter 时没有可信名 → None（聚合退回原始代表写法）。
    let dir2 = tmp("trusted-source-or-only");
    let orp2 = dir2.join("openrouter.json");
    std::fs::write(
        &orp2,
        r#"{"v":2,"synced_at":"t","entries":[{"id":"alias/opus5.5","name":"Channel Alias","prompt":0.000004,"completion":0.00002}]}"#,
    )
    .unwrap();
    let (p2, _w2) = Pricing::load(None, None, Some(&orp2));
    assert_eq!(
        p2.display_name_for("opus55"),
        None,
        "渠道别名不得成为模型展示名"
    );
}

#[test]
fn blank_model_name_is_unavailable() {
    let dir = tmp("blank-name");
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "   ", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let (p, _w) = Pricing::load(None, Some(&md), None);
    assert_eq!(p.display_name_for("opus55"), None, "空白名称视为缺失");

    // 聚合标签必须退回原始代表写法，不能显示空白。
    let claude = claude_dir(&dir, &["claude-opus-5.5"], 10);
    let r = summary(&opts(&dir, &claude, Some(&md), GroupBy::Model)).unwrap();
    assert_eq!(r.groups[0].key, "claudeopus55");
    assert_eq!(
        r.groups[0].label.as_deref(),
        Some("claude-opus-5.5"),
        "空白名称不得成为展示名"
    );
}

#[test]
fn model_identity_group_cost_uses_per_request_tiers_and_schedules() {
    // 场景 1：上下文分段阈值——两条各自落在基础档的请求，若被错误地
    // "先合并 token 再计价" 就会跨过 272K 阈值按高档计费（金额翻倍）。
    let dir = tmp("per-request-tier");
    let snap = dir.join("pricing-modelsdev.json");
    std::fs::write(
        &snap,
        r#"{"v":4,"synced_at":"t","entries":[{"id":"acme/claude-opus-5.5","name":"Opus 5.5",
            "input":4.0,"output":20.0,
            "segments":[{"min_tokens":272001,"input":8.0,"output":40.0}]}]}"#,
    )
    .unwrap();
    let claude = claude_events(
        &dir,
        &[
            ("2026-10-07T05:00:00.000Z", "claude-opus-5-5", 200_000),
            ("2026-10-07T06:00:00.000Z", "claude-opus-5.5", 100_000),
        ],
    );
    let o = opts(&dir, &claude, Some(&snap), GroupBy::Model);
    let r = summary(&o).unwrap();
    let g = &r.groups[0];
    assert_eq!(g.requests, 2);
    let merged_would_cost = 300_000.0 * 8.0 / 1e6;
    assert!(
        (g.cost_usd - 1.2).abs() < 1e-9,
        "逐请求计价应为 200k×4 + 100k×4 = 1.2/1M，实际 {}",
        g.cost_usd
    );
    assert!(
        (g.cost_usd - merged_would_cost).abs() > 1e-9,
        "不得按合并后的高档计费（{merged_would_cost}）"
    );
    let events = list_all(&o, &EventFilter::default());
    let per_request: f64 = events.rows.iter().filter_map(|row| row.cost_usd).sum();
    assert!(
        (per_request - g.cost_usd).abs() < 1e-9,
        "明细逐条之和必须等于分组费用"
    );

    // 场景 2：时间档（峰谷）——两条请求落在不同档，分组费用 = 各档之和。
    let dir2 = tmp("per-request-schedule");
    let ext = dir2.join("pricing.toml");
    std::fs::write(
        &ext,
        r#"
[[model]]
prefix = "claude-opus-5-5"
input = 2.0
[[model.schedule]]
label = "peak"
timezone = "UTC"
[[model.schedule.period]]
start_time = "09:00"
end_time = "18:00"
input = 6.0
"#,
    )
    .unwrap();
    let claude2 = claude_events(
        &dir2,
        &[
            ("2026-10-07T10:00:00.000Z", "claude-opus-5-5", 1_000_000),
            ("2026-10-07T02:00:00.000Z", "claude-opus-5-5", 1_000_000),
        ],
    );
    let o2 = opts(&dir2, &claude2, None, GroupBy::Model);
    let r2 = summary(&o2).unwrap();
    let g2 = &r2.groups[0];
    assert_eq!(g2.requests, 2);
    assert!(
        (g2.cost_usd - 8.0).abs() < 1e-9,
        "峰 6.0 + 谷 2.0 = 8.0/1M，实际 {}",
        g2.cost_usd
    );
    let events2 = list_all(&o2, &EventFilter::default());
    let per_request2: f64 = events2.rows.iter().filter_map(|row| row.cost_usd).sum();
    assert!((per_request2 - g2.cost_usd).abs() < 1e-9);
}

#[test]
fn model_identity_preserves_usage_and_requests() {
    let dir = tmp("conservation");
    let models = ["claude-opus-5-5", "claude-opus-5.5", "claude-opus-5-5"];
    let claude = claude_dir(&dir, &models, 100);
    let r = summary(&opts(&dir, &claude, None, GroupBy::Model)).unwrap();
    let g = &r.groups[0];
    assert_eq!(g.requests, 3);
    assert_eq!(g.tokens.input, 300);
    assert_eq!(g.tokens.output, 3);
    assert_eq!(r.totals.requests, 3);
    assert_eq!(r.totals.tokens, g.tokens, "合并只重排分组，不改总量");
}

#[test]
fn model_identity_keeps_versions_and_variants_separate() {
    let dir = tmp("separate");
    let claude = claude_dir(
        &dir,
        &[
            "claude-opus-5-5",
            "claude-opus-5-5-20260101",
            "claude-opus-5-5:free",
            "claude-opus-5-6",
        ],
        10,
    );
    let r = summary(&opts(&dir, &claude, None, GroupBy::Model)).unwrap();
    let keys: Vec<&str> = r.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "claudeopus55",
            "claudeopus5520260101",
            "claudeopus55:free",
            "claudeopus56",
            "合计"
        ],
        "不同版本与变体不得合并（前缀相同也不行）"
    );
    assert!(
        r.groups[..r.groups.len() - 1]
            .iter()
            .all(|g| g.requests == 1),
        "每个身份各一条请求"
    );
}

#[test]
fn model_identity_group_cost_sums_per_request() {
    let dir = tmp("cost");
    // 两条请求：token 不同 → 分组费用必须等于逐请求费用之和（不能先合并再计价）。
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, (model, input)) in [
        ("claude-opus-5-5", 1_000_000u64),
        ("claude-opus-5.5", 2_000_000u64),
    ]
    .iter()
    .enumerate()
    {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-10-07T0{}:00:00.000Z","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"c-{i}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#,
            i + 5
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    let md = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"claude-opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let o = opts(&dir, &dir.join("claude"), Some(&md), GroupBy::Model);
    let r = summary(&o).unwrap();
    let g = &r.groups[0];
    assert_eq!(g.requests, 2);
    assert!(
        (g.cost_usd - 9.0).abs() < 1e-9,
        "1M×3 + 2M×3 = 9：逐请求计价后求和，{}",
        g.cost_usd
    );

    // 明细逐条费用与分组一致（同源）。
    let events = list_all(
        &o,
        &EventFilter {
            model: Some(g.key.clone()),
            ..Default::default()
        },
    );
    let sum: f64 = events.rows.iter().filter_map(|row| row.cost_usd).sum();
    assert!((sum - g.cost_usd).abs() < 1e-9, "{sum} vs {}", g.cost_usd);
    assert_eq!(events.total, 2);
}

#[test]
fn model_identity_drill_pages_include_all_spellings() {
    let dir = tmp("pages");
    // 210 条事件、两种拼写交替 → 下钻必须跨页返回全部等价请求。
    let models: Vec<&str> = (0..210)
        .map(|i| {
            if i % 2 == 0 {
                "claude-opus-5-5"
            } else {
                "claude-opus-5.5"
            }
        })
        .collect();
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let mut body = String::new();
    for (i, model) in models.iter().enumerate() {
        let ts = format!(
            "2026-10-07T{:02}:{:02}:{:02}.000Z",
            5 + i / 3600,
            (i / 60) % 60,
            i % 60
        );
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","cwd":"C:/work/alpha","message":{{"id":"p-{i}","model":"{model}","usage":{{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();

    let o = opts(&dir, &dir.join("claude"), None, GroupBy::Model);
    let r = summary(&o).unwrap();
    assert_eq!(r.groups[0].requests, 210, "两种拼写全部合并");

    let key = r.groups[0].key.clone();
    let snap = tokenscope::query::begin_query(&o).unwrap();
    let page1 = tokenscope::query::query_events(
        &snap.query_id,
        &EventFilter {
            model: Some(key.clone()),
            limit: Some(200),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page1.total, 210);
    assert_eq!(page1.rows.len(), 200);
    let page2 = tokenscope::query::query_events(
        &snap.query_id,
        &EventFilter {
            model: Some(key.clone()),
            limit: Some(200),
            before: Some(page1.rows.last().unwrap().cursor.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(page2.total, 210, "total 是过滤后全量，不随游标截断");
    assert_eq!(page2.rows.len(), 10, "续页补齐剩余 10 条");
    let ids: std::collections::BTreeSet<String> = page1
        .rows
        .iter()
        .chain(page2.rows.iter())
        .map(|row| row.record_id.clone())
        .collect();
    assert_eq!(ids.len(), 210, "跨页不重不漏");
    assert!(
        page1
            .rows
            .iter()
            .chain(page2.rows.iter())
            .all(|row| row.project == "C:/work/alpha")
    );
}

#[test]
fn model_display_name_uses_model_not_provider() {
    let dir = tmp("provider");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{
            "acme": {"name": "Acme Provider", "models": {
                "opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}},
                "legacy-x": {"cost": {"input": 1, "output": 2}}
            }}
        }"#,
    );
    let s = load_snapshot(&snap).unwrap().unwrap();
    assert_eq!(s.v, 4, "同步产出的快照必须是 v4");
    let by = |id: &str| s.entries.iter().find(|e| e.id == id).unwrap();
    assert_eq!(by("acme/opus-5.5").name.as_deref(), Some("Opus 5.5"));
    assert_eq!(
        by("acme/legacy-x").name,
        None,
        "模型无 name 时不得用 provider 名兜底"
    );
}

#[test]
fn model_display_name_is_not_identity() {
    let dir = tmp("name-identity");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{
            "acme": {"models": {
                "opus-5.5": {"name": "Shared Name", "cost": {"input": 3, "output": 15}},
                "other-model": {"name": "Shared Name", "cost": {"input": 5, "output": 25}}
            }}
        }"#,
    );
    let (p, _warn) = Pricing::load(None, Some(&snap), None);
    // 名称相同不等于身份相同：两个 ID 各自命中自己的 name 与价格。
    assert_eq!(
        p.display_name_for("acme/opus55").as_deref(),
        Some("Shared Name")
    );
    assert_eq!(
        p.display_name_for("acme/other_model").as_deref(),
        Some("Shared Name")
    );
    assert_ne!(
        ModelIdentity::parse("acme/opus55").identity_key(),
        ModelIdentity::parse("acme/other-model").identity_key()
    );
    let a = p
        .estimate("acme/opus55", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    let b = p
        .estimate("acme/other-model", &counts(1_000_000, 0, 0, 0), at())
        .unwrap();
    assert!((a.cost - 3.0).abs() < 1e-9, "{}", a.cost);
    assert!((b.cost - 5.0).abs() < 1e-9, "{}", b.cost);
}

#[test]
fn model_display_name_is_order_independent() {
    let dir = tmp("order");
    let body_acme_first = r#"{
        "acme": {"models": {"opus-5.5": {"name": "Acme Name", "cost": {"input": 3, "output": 15}}}},
        "beta": {"models": {"opus55": {"name": "Beta Name", "cost": {"input": 5, "output": 25}}}}
    }"#;
    let body_beta_first = r#"{
        "beta": {"models": {"opus55": {"name": "Beta Name", "cost": {"input": 5, "output": 25}}}},
        "acme": {"models": {"opus-5.5": {"name": "Acme Name", "cost": {"input": 3, "output": 15}}}}
    }"#;
    let s1 = sync_to(&dir, "a.json", body_acme_first);
    let s2 = sync_to(&dir, "b.json", body_beta_first);
    let (p1, _) = Pricing::load(None, Some(&s1), None);
    let (p2, _) = Pricing::load(None, Some(&s2), None);
    assert_eq!(
        p1.display_name_for("opus55"),
        p2.display_name_for("opus55"),
        "同一等价 ID 下的展示名必须与条目顺序无关"
    );
    assert_eq!(
        p1.display_name_for("opus55").as_deref(),
        Some("Acme Name"),
        "按 (display, name) 稳定序择一"
    );
}

#[test]
fn model_display_name_requires_full_id_match() {
    let dir = tmp("full-id");
    let snap = sync_to(
        &dir,
        "pricing-modelsdev.json",
        r#"{"acme": {"models": {"opus-5.5": {"name": "Opus 5.5", "cost": {"input": 3, "output": 15}}}}}"#,
    );
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert_eq!(
        p.display_name_for("Opus_5.5").as_deref(),
        Some("Opus 5.5"),
        "等价 ID 命中"
    );
    assert!(
        p.display_name_for("opus5.5-20261010").is_none(),
        "仅前缀命中的旧版本不得冒充本模型的展示名"
    );
    assert!(
        p.estimate("opus5.5-20261010", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "定价允许边界前缀回退（与展示名解析相互独立）"
    );
}

#[test]
fn model_display_name_legacy_snapshot_falls_back() {
    let dir = tmp("legacy");
    let snap = dir.join("pricing-modelsdev.json");
    // v3 旧快照：name 可能是供应商名称（旧同步把 provider name 兜底进来）。
    std::fs::write(
        &snap,
        r#"{"v":3,"synced_at":"t","entries":[{"id":"acme/opus-5.5","name":"Acme Provider","input":3.0,"output":15.0}]}"#,
    )
    .unwrap();
    let s = load_snapshot(&snap).unwrap().unwrap();
    assert_eq!(s.entries[0].name, None, "来源不明的 name 不得保留");
    let (p, _) = Pricing::load(None, Some(&snap), None);
    assert!(
        p.display_name_for("opus55").is_none(),
        "无可信 name → 聚合退回原始代表写法"
    );
    assert!(
        p.estimate("opus55", &counts(1_000_000, 0, 0, 0), at())
            .is_some(),
        "旧快照仍可离线提供价格"
    );
}
