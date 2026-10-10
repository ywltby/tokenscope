//! 计划 B6：黄金数据集对账——合成数据 + **独立手算**期望（非"与上一版一致"），
//! 一处钉住：四桶互斥守恒、去重（Claude 保末条 / Codex 保首条）、按日/模型/
//! 项目/应用分组、时间区间、无缓存/命中/重建三路径一致、单源与双源一致、
//! 汇总与明细同源。计价用外置 TOML（四键俱全，手算含缓存分项）。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

/// SF04：一次性明细读取 = 建会话 + 读一次（本文件用例不跨调用翻页）。
fn list_events(
    opts: &SummaryOptions,
    filter: &EventFilter,
) -> anyhow::Result<tokenscope::report::EventList> {
    let snap = tokenscope::query::begin_query(opts)?;
    tokenscope::query::query_events(&snap.query_id, filter)
}

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-golden-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 构造黄金数据集（两来源各一文件；数字与手算表见测试内注释）。
fn setup(dir: &std::path::Path) -> SummaryOptions {
    let claude_dir = dir.join("claude");
    // Claude 项目身份取文件父目录名（与 ~/.claude/projects/<slug> 一致）。
    let claude_proj = claude_dir.join("alpha");
    std::fs::create_dir_all(&claude_proj).unwrap();
    // m1 两条：流式重发，去重保末条（10:30Z 的 1500/250/3000/5000）。
    // m2：模型 unknown-model-x（不定价）。
    std::fs::write(
        claude_proj.join("sess-g1.jsonl"),
        concat!(
            r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"g1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":1000,"output_tokens":200,"cache_creation_input_tokens":3000,"cache_read_input_tokens":5000}}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-08-01T11:00:00.000Z","sessionId":"g1","message":{"id":"m2","model":"unknown-model-x","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-08-01T10:30:00.000Z","sessionId":"g1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":1500,"output_tokens":250,"cache_creation_input_tokens":3000,"cache_read_input_tokens":5000}}}"#, "\n",
        ),
    )
    .unwrap();
    let codex_dir = dir.join("codex");
    let day = codex_dir.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    // T1：raw input 15000 = 0 + 12000 读 + 3000 写；T2 与 T4 逐字相同（重发，
    // 去重保首条）；T3 跨日。展示总量守恒：15100/15/150。
    std::fs::write(
        day.join("rollout-g.jsonl"),
        concat!(
            r#"{"timestamp":"2026-08-01T11:59:00.000Z","type":"session_meta","payload":{"id":"g2","session_id":"g2","cwd":"C:/work/beta"}}"#, "\n",
            r#"{"timestamp":"2026-08-01T11:59:30.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/work/beta"}}"#, "\n",
            r#"{"timestamp":"2026-08-01T12:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":15000,"output_tokens":100,"cached_input_tokens":12000,"cache_write_input_tokens":3000,"reasoning_output_tokens":0,"total_tokens":15100}}}}"#, "\n",
            r#"{"timestamp":"2026-08-01T12:05:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":15}}}}"#, "\n",
            r#"{"timestamp":"2026-08-01T12:06:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":15}}}}"#, "\n",
            r#"{"timestamp":"2026-08-02T09:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":50,"cached_input_tokens":20,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":150}}}}"#, "\n",
        ),
    )
    .unwrap();
    let pricing = dir.join("pricing.toml");
    std::fs::write(
        &pricing,
        r#"
[[model]]
prefix = "claude-sonnet-4-5"
input = 3.0
output = 15.0
cache_write = 3.75
cache_read = 0.3

[[model]]
prefix = "gpt-5.6-sol"
input = 4.0
output = 20.0
cache_write = 5.0
cache_read = 0.4
"#,
    )
    .unwrap();
    SummaryOptions {
        by: GroupBy::Day,
        claude_dir: Some(claude_dir),
        codex_dir: Some(codex_dir),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(pricing),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

fn normalize_json(r: &tokenscope::report::SummaryReport) -> String {
    let mut r = r.clone();
    r.generated_at = String::new();
    // SF04：query_id 是会话身份（每次查询必然不同），不参与数字一致性。
    r.query_id = String::new();
    // H04：`duplicates_dropped` 是**本轮解析**的产物（指纹命中轮次没有解析、
    // 自然没有可丢弃的重复行）；请求数 / 四桶 / 费用 / 分组 / 警告仍逐字段
    // 参与比较，查询数字的三路径一致性保证不变。
    for s in &mut r.sources {
        s.stats.duplicates_dropped = 0;
    }
    serde_json::to_string(&r).unwrap()
}

#[test]
fn test_golden_summary_events_parity() {
    let dir = tmp_dir("parity");
    let opts = setup(&dir);

    // ── 手算总账（UTC→上海时区；T4 重发丢弃；m1 保 10:30Z 末条）──────
    // 08-01：claude m1(1500/250/3000/5000)=21000µ$、m2 不定价；
    //        codex T1(0/100/3000/12000)=21800µ$、T2(10/5)=140µ$
    //        → 4 请求，cost 0.021+0.02194=0.04294，unknown in100/out50
    // 08-02：codex T3(80/50/cr20)=1328µ$ → 1 请求
    // 合计：5 请求；in 1690 / out 455 / cw 6000 / cr 17020；cost 0.044268
    let r = summary(&opts).unwrap();
    assert_eq!(r.totals.requests, 5);
    assert_eq!(r.totals.tokens.input, 1690);
    assert_eq!(r.totals.tokens.output, 455);
    assert_eq!(r.totals.tokens.cache_write, 6000);
    assert_eq!(r.totals.tokens.cache_read, 17020);
    assert!(
        (r.totals.cost_usd - 0.044268).abs() < 1e-9,
        "{}",
        r.totals.cost_usd
    );
    assert!(r.totals.unknown_pricing);
    assert_eq!(r.totals.unknown_tokens.input, 100);
    assert_eq!(r.totals.unknown_tokens.output, 50);
    assert_eq!(r.totals.unknown_tokens.cache_write, 0);
    assert_eq!(r.totals.unknown_tokens.cache_read, 0);

    // 分组：08-01 / 08-02 / 合计。
    assert_eq!(r.groups.len(), 3);
    let d1 = &r.groups[0];
    assert_eq!(d1.key, "2026-08-01");
    assert_eq!(d1.requests, 4);
    assert!((d1.cost_usd - 0.04294).abs() < 1e-9);
    let d2 = &r.groups[1];
    assert_eq!(d2.key, "2026-08-02");
    assert_eq!(d2.requests, 1);
    assert!((d2.cost_usd - 1328.0 / 1_000_000.0).abs() < 1e-9);

    // 模型 / 项目 / 应用维度（去重后口径）。
    let m = summary(&SummaryOptions {
        by: GroupBy::Model,
        ..opts.clone()
    })
    .unwrap();
    let keys: Vec<&str> = m.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(
        keys,
        ["claudesonnet45", "gpt56sol", "unknownmodelx", "合计"],
        "MP04：模型维度 key 是等价身份键"
    );
    let labels: Vec<&str> = m
        .groups
        .iter()
        .map(|g| g.label.as_deref().unwrap_or(""))
        .collect();
    assert_eq!(
        labels,
        ["claude-sonnet-4-5", "gpt-5.6-sol", "unknown-model-x", ""],
        "展示名退回原始代表写法（合计行无 label）"
    );
    let p = summary(&SummaryOptions {
        by: GroupBy::Project,
        ..opts.clone()
    })
    .unwrap();
    // C2：项目身份 = 完整 cwd（codex）/ 相对目录（claude）；label 为展示名。
    assert_eq!(p.groups[0].key, "C:/work/beta");
    assert_eq!(p.groups[0].label.as_deref(), Some("beta"));
    assert_eq!(p.groups[0].requests, 3);
    assert_eq!(p.groups[1].key, "alpha");
    assert_eq!(p.groups[1].requests, 2);
    let a = summary(&SummaryOptions {
        by: GroupBy::Agent,
        ..opts.clone()
    })
    .unwrap();
    assert_eq!(a.groups[0].requests, 2); // claude-code
    assert_eq!(a.groups[1].requests, 3); // codex

    // 时间区间（解析时区自然日闭区间）：与按日分组严格同口径。
    let range = |from: &str, to: &str| SummaryOptions {
        from: Some(from.into()),
        to: Some(to.into()),
        ..opts.clone()
    };
    assert_eq!(
        summary(&range("2026-08-01", "2026-08-01"))
            .unwrap()
            .totals
            .requests,
        4
    );
    assert_eq!(
        summary(&range("2026-08-02", "2026-08-02"))
            .unwrap()
            .totals
            .requests,
        1
    );

    // 明细与汇总同源：total == requests；行数受 limit 200 上限内。
    let l = list_events(&opts, &EventFilter::default()).unwrap();
    assert_eq!(l.total, 5);
    assert_eq!(l.rows.len(), 5);

    // 单源与双源一致：claude 2 / codex 3。
    let rc = summary(&SummaryOptions {
        agent: Some(AgentKind::ClaudeCode),
        ..opts.clone()
    })
    .unwrap();
    assert_eq!(rc.totals.requests, 2);
    let rx = summary(&SummaryOptions {
        agent: Some(AgentKind::Codex),
        ..opts.clone()
    })
    .unwrap();
    assert_eq!(rx.totals.requests, 3);

    // 缓存三路径逐字段一致（无缓存→命中→重建）。
    let cold = normalize_json(&r);
    let warm = normalize_json(&summary(&opts).unwrap());
    assert_eq!(cold, warm, "缓存命中必须与首次全量一致");
    let rebuilt = normalize_json(
        &summary(&SummaryOptions {
            refresh: true,
            ..opts.clone()
        })
        .unwrap(),
    );
    assert_eq!(cold, rebuilt, "重建后必须一致");
    std::fs::remove_dir_all(&dir).ok();
}
