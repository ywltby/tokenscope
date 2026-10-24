//! WB01：现有来源等价基线（冻结测试）。
//!
//! 在来源层泛化（`bool` → `AgentKind`）**之前**冻结 Claude/Codex 合成输入的
//! 事件集合、四桶、去重、费用、来源配置与设置序列化结果；泛化后本文件除
//! `source_config` 调用点按同义 `AgentKind` 适配外，全部断言必须原样通过
//! （计划 WB01：行为不变的重构）。
//!
//! 覆盖点（§1.1）：Codex 压缩补计（`token_usage_record` + zero last）与线程
//! 设置模型归属（`thread_settings_applied`）已合入主线，属于必须冻结的路径。
//!
//! 密闭性：`cache_dir` / `pricing_index` / `pricing_path` 等全部注入临时目录。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{EventFilter, SummaryOptions, collect_all_with_sources_for_test, summary};
use tokenscope::settings::{AgentSources, Settings, SourceConfig};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-wb01-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 构造冻结数据集（数字与手算注释见各测试）。
fn setup(dir: &std::path::Path) -> SummaryOptions {
    let claude_dir = dir.join("claude");
    let claude_proj = claude_dir.join("alpha");
    std::fs::create_dir_all(&claude_proj).unwrap();
    // m1 两条：流式重发，去重保末条（10:30Z 的 1500/250/3000/5000）。
    // m2：模型 unknown-model-x（不定价）。
    std::fs::write(
        claude_proj.join("sess-wb01.jsonl"),
        concat!(
            r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"g1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":1000,"output_tokens":200,"cache_creation_input_tokens":3000,"cache_read_input_tokens":5000}}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-08-01T11:00:00.000Z","sessionId":"g1","message":{"id":"m2","model":"unknown-model-x","usage":{"input_tokens":100,"output_tokens":50,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
            "\n",
            r#"{"type":"assistant","timestamp":"2026-08-01T10:30:00.000Z","sessionId":"g1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":1500,"output_tokens":250,"cache_creation_input_tokens":3000,"cache_read_input_tokens":5000}}}"#,
            "\n",
        ),
    )
    .unwrap();

    let codex_dir = dir.join("codex");
    let day = codex_dir.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    // T1：raw input 15000 = 0 + 12000 读 + 3000 写（model-a）。
    // T2 与 T4 逐字相同（重发，全局去重丢弃）。
    // thread_settings_applied(model-b)：T5 / R / T3 归属 model-b。
    // R：压缩补计记录（token_usage_record）+ zero last（skipped_zero_usage）。
    // T3：跨日（2026-08-02）。
    std::fs::write(
        day.join("rollout-wb01.jsonl"),
        concat!(
            r#"{"timestamp":"2026-08-01T11:59:00.000Z","type":"session_meta","payload":{"id":"g2","session_id":"g2","cwd":"C:/work/beta"}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T11:59:30.000Z","type":"turn_context","payload":{"turn_id":"t","model":"model-a","cwd":"C:/work/beta"}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":15000,"output_tokens":100,"cached_input_tokens":12000,"cache_write_input_tokens":3000,"reasoning_output_tokens":0,"total_tokens":15100}}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:05:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":15}}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:06:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":5,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":15}}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:08:00.000Z","type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"model":"model-b"}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:10:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":50,"output_tokens":20,"cached_input_tokens":10,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":70}}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:20:00.000Z","type":"token_usage_record","payload":{"response_id":"synthetic-response-wb01","usage":{"input_tokens":26705,"output_tokens":491,"cached_input_tokens":12000,"cache_write_input_tokens":3000,"total_tokens":27196}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:25:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":99999}}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T12:26:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":0,"output_tokens":0,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":99999}}}}"#,
            "\n",
            r#"{"timestamp":"2026-08-02T09:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":50,"cached_input_tokens":20,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":150}}}}"#,
            "\n",
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
prefix = "model-a"
input = 4.0
output = 20.0
cache_write = 5.0
cache_read = 0.4

[[model]]
prefix = "model-b"
input = 2.0
output = 10.0
cache_write = 2.5
cache_read = 0.2
"#,
    )
    .unwrap();

    SummaryOptions {
        by: GroupBy::Day,
        // WB04 密闭注入：本文件冻结的是 Claude/Codex 行为等价基线，
        // 新增默认来源按计划显式禁用（不注入会读到真实 WorkBuddy 数据）。
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
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

/// 事件级冻结快照：每事件一行、去掉临时目录前缀，按行排序后拼接。
/// 字段覆盖：agent、时间、模型、会话、项目、record_id、行号与四桶。
/// 先走一次正常管线（summary 写历史库），再从历史库读回存储事件。
fn event_snapshot(dir: &std::path::Path, opts: &SummaryOptions) -> String {
    let _ = summary(opts).unwrap();
    let view = collect_all_with_sources_for_test(Vec::new(), opts).unwrap();
    let prefix = dir.display().to_string().replace('\\', "/");
    let mut lines: Vec<String> = view
        .events
        .iter()
        .map(|e| {
            let source = e.source_path.replace('\\', "/");
            let source = source.strip_prefix(&prefix).unwrap_or(&source);
            format!(
                "{}|{}|{}|{}|{}|{}|{}|line={}|{}|{}|{}|{}|{}",
                e.agent.as_str(),
                e.ts,
                e.model,
                e.session_id,
                e.project,
                e.record_id,
                e.event_cwd.as_deref().unwrap_or("-"),
                e.line,
                e.input_tokens,
                e.output_tokens,
                e.cache_write_tokens,
                e.cache_read_tokens,
                source,
            )
        })
        .collect();
    lines.sort();
    lines.join("\n")
}

fn normalize_json(r: &tokenscope::report::SummaryReport) -> String {
    let mut r = r.clone();
    r.generated_at = String::new();
    r.query_id = String::new();
    // 指纹命中轮次没有解析，自然没有可丢弃的重复行（与 golden 同策略）。
    for s in &mut r.sources {
        s.stats.duplicates_dropped = 0;
    }
    serde_json::to_string(&r).unwrap()
}

/// 手算总账（UTC→Asia/Shanghai；T4 重发丢弃；m1 保 10:30Z 末条）：
/// - claude：m1(1500/250/3000/5000)=21000µ$、m2 unknown(100/50) 不计价
/// - codex：T1(0/100/3000/12000)=21800µ$、T2(10/5)=140µ$、
///   T5(40/20/0/10 model-b)=282µ$、R(11705/491/3000/12000 model-b)=38220µ$、
///   T3(80/50/0/20 model-b 跨日 08-02)=664µ$
/// - 合计：7 请求；in 13435 / out 966 / cw 9000 / cr 29030；
///   cost 82106µ$ = 0.082106；unknown in100/out50
#[test]
fn test_frozen_pipeline_totals_and_groups() {
    let dir = tmp_dir("totals");
    let opts = setup(&dir);
    let r = summary(&opts).unwrap();
    assert_eq!(r.totals.requests, 7);
    assert_eq!(r.totals.tokens.input, 13435);
    assert_eq!(r.totals.tokens.output, 966);
    assert_eq!(r.totals.tokens.cache_write, 9000);
    assert_eq!(r.totals.tokens.cache_read, 29030);
    assert!(
        (r.totals.cost_usd - 0.082106).abs() < 1e-9,
        "{}",
        r.totals.cost_usd
    );
    assert!(r.totals.unknown_pricing);
    assert_eq!(r.totals.unknown_tokens.input, 100);
    assert_eq!(r.totals.unknown_tokens.output, 50);

    // 按日（Asia/Shanghai）：08-01 六请求、08-02 一请求。
    assert_eq!(r.groups.len(), 3);
    assert_eq!(r.groups[0].key, "2026-08-01");
    assert_eq!(r.groups[0].requests, 6);
    assert!((r.groups[0].cost_usd - 0.081442).abs() < 1e-9);
    assert_eq!(r.groups[1].key, "2026-08-02");
    assert_eq!(r.groups[1].requests, 1);
    assert!((r.groups[1].cost_usd - 664.0 / 1_000_000.0).abs() < 1e-9);

    // 按模型：四桶互斥 + 等价键分组（m1 去重保末条已并入）。
    let m = summary(&SummaryOptions {
        by: GroupBy::Model,
        ..opts.clone()
    })
    .unwrap();
    let keys: Vec<&str> = m.groups.iter().map(|g| g.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "claudesonnet45",
            "modela",
            "modelb",
            "unknownmodelx",
            "合计"
        ]
    );
    let by_key = |k: &str| m.groups.iter().find(|g| g.key == k).unwrap();
    assert_eq!(by_key("claudesonnet45").requests, 1);
    assert_eq!(by_key("modela").requests, 2);
    assert_eq!(by_key("modelb").requests, 3);
    assert_eq!(by_key("unknownmodelx").requests, 1);

    // 按应用：claude 2 / codex 5。
    let a = summary(&SummaryOptions {
        by: GroupBy::Agent,
        ..opts.clone()
    })
    .unwrap();
    assert_eq!(a.groups[0].key, "claude-code");
    assert_eq!(a.groups[0].requests, 2);
    assert_eq!(a.groups[1].key, "codex");
    assert_eq!(a.groups[1].requests, 5);

    // 按项目：alpha（claude 相对 slug）2 / C:/work/beta（codex cwd）5。
    let p = summary(&SummaryOptions {
        by: GroupBy::Project,
        ..opts.clone()
    })
    .unwrap();
    let keys: Vec<&str> = p.groups.iter().map(|g| g.key.as_str()).collect();
    assert!(keys.contains(&"alpha"), "keys = {keys:?}");
    assert!(keys.contains(&"C:/work/beta"), "keys = {keys:?}");

    // 单源过滤与全局去重：T4 重发在首轮被丢弃（duplicates_dropped=1；
    // 指纹命中轮次没有解析，不产生新的去重统计——H04 语义）。
    let codex_only = summary(&SummaryOptions {
        agent: Some(AgentKind::Codex),
        ..opts.clone()
    })
    .unwrap();
    assert_eq!(codex_only.totals.requests, 5);
    let first_codex = r
        .sources
        .iter()
        .find(|s| s.agent == AgentKind::Codex)
        .unwrap();
    assert_eq!(
        first_codex.stats.duplicates_dropped, 1,
        "T4 重发必须被去重丢弃"
    );
    assert_eq!(
        first_codex.stats.skipped_zero_usage, 1,
        "压缩补计的 zero last 计 skipped_zero_usage"
    );
    assert_eq!(first_codex.stats.events, 5, "压缩补计记录 R 必须计入事件");
    assert_eq!(first_codex.stats.bad_lines, 0);

    // 明细与汇总同源。
    let snap = tokenscope::query::begin_query(&opts).unwrap();
    let l = tokenscope::query::query_events(&snap.query_id, &EventFilter::default()).unwrap();
    assert_eq!(l.total, 7);
    assert_eq!(l.rows.len(), 7);

    // 缓存三路径逐字段一致（无缓存→命中→重建）。
    let cold = normalize_json(&summary(&opts).unwrap());
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

/// 事件级冻结：字段级快照（排序后整串断言），泛化前后必须逐字节相同。
#[test]
fn test_frozen_event_snapshot() {
    let dir = tmp_dir("events");
    let opts = setup(&dir);
    let snapshot = event_snapshot(&dir, &opts);
    // 存储形态：历史库精简事件不含行号与来源路径（H04），event_cwd 保留；
    // Claude 侧 cwd 由映射/父目录派生项目，原始 cwd 不入库。
    let expected = concat!(
        "claude-code|2026-08-01T10:30:00Z|claude-sonnet-4-5|g1|alpha|m1|-|line=0|1500|250|3000|5000|\n",
        "claude-code|2026-08-01T11:00:00Z|unknown-model-x|g1|alpha|m2|-|line=0|100|50|0|0|\n",
        "codex|2026-08-01T12:00:00Z|model-a|g2|C:/work/beta||C:/work/beta|line=0|0|100|3000|12000|\n",
        "codex|2026-08-01T12:05:00Z|model-a|g2|C:/work/beta||C:/work/beta|line=0|10|5|0|0|\n",
        "codex|2026-08-01T12:10:00Z|model-b|g2|C:/work/beta||C:/work/beta|line=0|40|20|0|10|\n",
        "codex|2026-08-01T12:20:00Z|model-b|g2|C:/work/beta||C:/work/beta|line=0|11705|491|3000|12000|\n",
        "codex|2026-08-02T09:00:00Z|model-b|g2|C:/work/beta||C:/work/beta|line=0|80|50|0|20|",
    );
    assert_eq!(snapshot, expected, "事件字段快照与冻结基线不一致");
    std::fs::remove_dir_all(&dir).ok();
}

/// 来源配置与设置序列化冻结：TOML 读写往返逐字节不变；两应用配置互不串扰。
#[test]
fn test_frozen_settings_serialization_and_source_config() {
    let claude_cfg = SourceConfig {
        auto_discover: false,
        excluded_auto_paths: vec!["D:/logs/claude/projects".to_string()],
        additional_paths: vec![tokenscope::settings::AdditionalSourcePath {
            path: "D:/history/claude/projects".to_string(),
            enabled: false,
        }],
        ..Default::default()
    };
    let codex_cfg = SourceConfig {
        enabled: false,
        ..Default::default()
    };
    let settings = Settings {
        sources: AgentSources {
            claude: Some(claude_cfg.clone()),
            codex: Some(codex_cfg.clone()),
            workbuddy: None,
            workbuddy_ai: None,
        },
        ..Default::default()
    };

    // TOML 序列化 → 反序列化 → 再序列化：字节级一致（键序与格式不漂移）。
    let text1 = format!(
        "{}{}",
        tokenscope::settings::SETTINGS_HEADER,
        toml::to_string_pretty(&settings).unwrap()
    );
    let parsed: Settings = toml::from_str(&text1).unwrap();
    let text2 = format!(
        "{}{}",
        tokenscope::settings::SETTINGS_HEADER,
        toml::to_string_pretty(&parsed).unwrap()
    );
    assert_eq!(text1, text2, "设置序列化往返必须逐字节不变");
    assert_eq!(parsed, settings, "往返后值相等");
    assert!(
        text1.contains("[sources.claude]") && text1.contains("[sources.codex]"),
        "来源配置键名保持 claude / codex：{text1}"
    );

    // source_config 按应用取值：泛化后实参为同义 AgentKind，断言行为不变
    // （claude 关自动发现；codex 停用；未配置 → 默认启用）。
    let s = Settings::default();
    assert!(s.source_config(AgentKind::ClaudeCode).enabled);
    assert!(s.source_config(AgentKind::Codex).enabled);
    let with_cfg = settings.clone();
    assert!(!with_cfg.source_config(AgentKind::ClaudeCode).auto_discover);
    assert_eq!(
        with_cfg
            .source_config(AgentKind::ClaudeCode)
            .excluded_auto_paths,
        vec!["D:/logs/claude/projects".to_string()]
    );
    assert!(!with_cfg.source_config(AgentKind::Codex).enabled);
    assert_eq!(
        with_cfg
            .source_config(AgentKind::Codex)
            .additional_paths
            .len(),
        0,
        "codex 不得串到 claude 的追加项"
    );

    // 事务写读（磁盘路径）：文件字节与序列化结果一致。
    let dir = tmp_dir("settings");
    let path = dir.join("settings.toml");
    tokenscope::settings::update(&path, |s| {
        *s = settings.clone();
        Ok(())
    })
    .unwrap();
    let disk = std::fs::read_to_string(&path).unwrap();
    assert_eq!(disk, text1, "落盘字节与内存序列化一致");
    let loaded = tokenscope::settings::load(&path).unwrap();
    assert_eq!(loaded, settings);
    std::fs::remove_dir_all(&dir).ok();
}
