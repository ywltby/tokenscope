//! A05：跨工具路径合并后的下钻闭环。
//!
//! 全程走真实查询管线（`query_begin` → 汇总 → 下钻事件），断言合并项目在
//! 汇总、下钻、分页与来源筛选四个出口上身份一致，且合并只重排分组、不改
//! 总量（请求数、四类 token、费用、未知价格状态逐字段守恒）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::{AgentKind, TokenCounts};
use tokenscope::query;
use tokenscope::report::{EventFilter, SummaryOptions, summary};

fn tmp(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-a05-drill-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Claude 侧 3 条事件，全部带同一 cwd。
fn write_claude(dir: &Path, with_cwd: bool) -> PathBuf {
    let project = dir.join("claude").join("alpha");
    std::fs::create_dir_all(&project).unwrap();
    let cwd = if with_cwd {
        r#""cwd":"C:\\work\\alpha","#
    } else {
        ""
    };
    let mut body = String::new();
    for (i, id) in ["c-m1", "c-m2", "c-m3"].iter().enumerate() {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-08-01T10:0{i}:00.000Z","sessionId":"c1",{cwd}"message":{{"id":"{id}","model":"claude-sonnet-4-5","usage":{{"input_tokens":100,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(project.join("s.jsonl"), body).unwrap();
    dir.join("claude")
}

/// Codex 侧 3 条事件，全部带同一 cwd。
fn write_codex(dir: &Path) -> PathBuf {
    let codex = dir.join("codex");
    let day = codex.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    let mut body = String::new();
    body.push_str(
        r#"{"timestamp":"2026-08-01T12:00:00.000Z","type":"session_meta","payload":{"id":"x","session_id":"x","cwd":"C:/work/alpha"}}"#,
    );
    body.push('\n');
    body.push_str(
        r#"{"timestamp":"2026-08-01T12:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/work/alpha"}}"#,
    );
    body.push('\n');
    for i in 0..3u64 {
        // 三条用量各不相同：避免命中 Codex 的"同 session 同用量重播"去重规则。
        let input = 50 * (i + 1);
        let output = 5 * (i + 1);
        let line = serde_json::json!({
            "timestamp": format!("2026-08-01T12:1{i}:00.000Z"),
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "last_token_usage": {
                        "input_tokens": input,
                        "output_tokens": output,
                        "cached_input_tokens": 0,
                        "cache_write_input_tokens": 0,
                        "reasoning_output_tokens": 0,
                        "total_tokens": input + output,
                    }
                }
            }
        });
        body.push_str(&line.to_string());
        body.push('\n');
    }
    std::fs::write(day.join("rollout-a.jsonl"), body).unwrap();
    codex
}

/// 外置价格表：两个模型都收录（费用守恒可比）。
fn write_pricing(dir: &Path) -> PathBuf {
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
prefix = "gpt-5.6"
input = 4.0
output = 20.0
cache_write = 5.0
cache_read = 0.4
"#,
    )
    .unwrap();
    pricing
}

fn opts(dir: &Path, claude: &Path, codex: &Path, agent: Option<AgentKind>) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Project,
        agent,
        claude_dir: Some(claude.to_path_buf()),
        codex_dir: Some(codex.to_path_buf()),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

const MERGED_KEY: &str = "C:/work/alpha";

#[test]
fn merged_project_drilldown_returns_both_agents() {
    let dir = tmp("drill");
    let claude = write_claude(&dir, true);
    let codex = write_codex(&dir);
    write_pricing(&dir);
    let o = opts(&dir, &claude, &codex, None);

    let snap = query::begin_query(&o).unwrap();
    let s = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(s.groups.len(), 2, "合并 + 合计：{:?}", s.groups);
    assert_eq!(s.groups[0].key, MERGED_KEY);
    assert_eq!(s.groups[0].requests, 6);

    let filter = EventFilter {
        project: Some(MERGED_KEY.to_string()),
        ..Default::default()
    };
    let events = query::query_events(&snap.query_id, &filter).unwrap();
    assert_eq!(events.total, 6);
    let mut agents: Vec<&str> = events.rows.iter().map(|r| r.agent).collect();
    agents.sort();
    agents.dedup();
    assert_eq!(agents, ["claude-code", "codex"]);
    assert!(events.rows.iter().all(|r| r.project == MERGED_KEY));

    // 分页（limit=2）续页仍属同一项目，翻完 6 条不重不漏。
    let mut cursor: Option<String> = None;
    let mut seen = 0usize;
    let mut pages = 0;
    loop {
        let page = query::query_events(
            &snap.query_id,
            &EventFilter {
                project: Some(MERGED_KEY.to_string()),
                limit: Some(2),
                before: cursor.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        if page.rows.is_empty() {
            break;
        }
        assert!(
            page.rows.iter().all(|r| r.project == MERGED_KEY),
            "续页必须仍在同一项目"
        );
        assert_eq!(page.total, 6, "total 是过滤后全量，不随游标截断");
        seen += page.rows.len();
        cursor = Some(page.rows.last().unwrap().cursor.clone());
        pages += 1;
        assert!(pages <= 5, "分页不得发散");
    }
    assert_eq!(seen, 6);
    assert_eq!(pages, 3);

    // 来源筛选分别取出两侧事件（agent 维度过滤），合并 key 相同。
    for (kind, expected) in [(AgentKind::ClaudeCode, 3u64), (AgentKind::Codex, 3)] {
        let q = query::begin_query(&opts(&dir, &claude, &codex, Some(kind))).unwrap();
        let l = query::query_events(
            &q.query_id,
            &EventFilter {
                project: Some(MERGED_KEY.to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(l.total, expected, "{kind:?}");
        assert!(l.rows.iter().all(|r| r.agent == kind.as_str()));
        assert!(l.rows.iter().all(|r| r.project == MERGED_KEY));
    }
}

fn totals(o: &SummaryOptions) -> (u64, TokenCounts, f64, bool, TokenCounts) {
    let t = summary(o).unwrap().totals;
    (
        t.requests,
        t.tokens,
        t.cost_usd,
        t.unknown_pricing,
        t.unknown_tokens,
    )
}

#[test]
fn project_merge_preserves_totals() {
    let dir = tmp("totals");
    let claude = write_claude(&dir, true);
    let codex = write_codex(&dir);
    write_pricing(&dir);

    let merged = totals(&opts(&dir, &claude, &codex, None));
    let only_claude = totals(&opts(&dir, &claude, &codex, Some(AgentKind::ClaudeCode)));
    let only_codex = totals(&opts(&dir, &claude, &codex, Some(AgentKind::Codex)));

    // 合并只是重排分组：总量必须逐字段等于两侧之和。
    assert_eq!(merged.0, only_claude.0 + only_codex.0, "请求数守恒");
    assert_eq!(
        merged.1,
        accumulate(only_claude.1, only_codex.1),
        "四类 token 守恒"
    );
    assert!(
        (merged.2 - (only_claude.2 + only_codex.2)).abs() < 1e-9,
        "费用守恒：{} vs {}",
        merged.2,
        only_claude.2 + only_codex.2
    );
    assert!(merged.2 > 0.0, "价格表命中，费用必须非零");
    assert_eq!(merged.3, only_claude.3 || only_codex.3, "未知价格状态守恒");
    assert_eq!(
        merged.4,
        accumulate(only_claude.4, only_codex.4),
        "未知 token 守恒"
    );

    // 身份分裂（Claude 无可靠 cwd）只重排分组，总量逐字段不变。
    let split_dir = tmp("totals-split");
    let split_claude = write_claude(&split_dir, false);
    let split_codex = write_codex(&split_dir);
    write_pricing(&split_dir);
    let split = totals(&opts(&split_dir, &split_claude, &split_codex, None));
    assert_eq!(split.0, merged.0, "分裂分组不得改变请求数");
    assert_eq!(split.1, merged.1);
    assert!((split.2 - merged.2).abs() < 1e-9);
    assert_eq!(split.3, merged.3);
    assert_eq!(split.4, merged.4);
}

fn accumulate(a: TokenCounts, b: TokenCounts) -> TokenCounts {
    TokenCounts {
        input: a.input + b.input,
        output: a.output + b.output,
        cache_write: a.cache_write + b.cache_write,
        cache_read: a.cache_read + b.cache_read,
    }
}

/// B03 修订（审查）：目录切换后的**完整联动**验收——A/B 分组、下钻分页、
/// 冷/热缓存结果与费用守恒串成一组断言，用同一份会话内切换 fixture。
#[test]
fn switch_groups_drilldown_pages_cache_and_cost_are_consistent() {
    let dir = tmp("switch-link");
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/project-path/session-switch");
    // 只采集本用例需要的会话文件（项目身份来自行内 cwd，与文件位置无关）。
    let claude_root = dir.join("sources").join("switch");
    std::fs::create_dir_all(&claude_root).unwrap();
    std::fs::copy(
        fixture.join("claude-switch.jsonl"),
        claude_root.join("s.jsonl"),
    )
    .unwrap();
    std::fs::write(
        dir.join("pricing.toml"),
        "[[model]]\nprefix = \"claude-sonnet-4-5\"\ninput = 3.0\noutput = 15.0\ncache_write = 3.75\ncache_read = 0.3\n",
    )
    .unwrap();
    let opts = SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(claude_root),
        codex_dir: Some(dir.join("no-codex")),
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or-missing.json")),
        modelsdev_path: Some(dir.join("md-missing.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    };

    // 1) 分组：会话内 A → A/sub → A → B → B/sub 归成两个项目。
    let cold = summary(&opts).unwrap();
    assert_eq!(cold.groups.len(), 3, "两个项目 + 合计：{:?}", cold.groups);
    let pick = |k: &str| {
        cold.groups
            .iter()
            .find(|g| g.key == k)
            .cloned()
            .unwrap_or_else(|| panic!("缺少分组 {k}"))
    };
    let a = pick("C:/test");
    let b = pick("C:/bee");
    assert_eq!((a.requests, b.requests), (3, 2), "切换后的请求分别归两组");
    assert!(a.cost_usd > 0.0, "价格表命中，费用不应为 0");

    // 2) 冷/热缓存：第二次查询（命中磁盘缓存）必须与冷跑逐字段一致。
    let hot = summary(&opts).unwrap();
    let snap = |r: &tokenscope::report::SummaryReport| {
        r.groups
            .iter()
            .map(|g| {
                (
                    g.key.clone(),
                    g.requests,
                    g.tokens,
                    g.cost_usd,
                    g.unknown_pricing,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(snap(&cold), snap(&hot), "冷/热缓存结果必须逐字段一致");

    // 3) 费用守恒：两组之和 = 合计，且各自等于明细逐条费用之和。
    assert!(
        (a.cost_usd + b.cost_usd - cold.totals.cost_usd).abs() < 1e-12,
        "{} + {} != {}",
        a.cost_usd,
        b.cost_usd,
        cold.totals.cost_usd
    );
    let q = query::begin_query(&opts).unwrap();
    let detail = |project: &str, before: Option<String>, limit: usize| {
        query::query_events(
            &q.query_id,
            &EventFilter {
                project: Some(project.to_string()),
                limit: Some(limit),
                before,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let sum_cost = |project: &str| -> f64 {
        detail(project, None, 200)
            .rows
            .iter()
            .filter_map(|r| r.cost_usd)
            .sum()
    };
    assert!((sum_cost("C:/test") - a.cost_usd).abs() < 1e-12);
    assert!((sum_cost("C:/bee") - b.cost_usd).abs() < 1e-12);

    // 4) 下钻分页：切换后的分组各自翻页完整、不重不漏。
    let p1 = detail("C:/test", None, 2);
    assert_eq!(p1.total, 3);
    assert_eq!(p1.rows.len(), 2);
    let p2 = detail("C:/test", Some(p1.rows.last().unwrap().cursor.clone()), 2);
    assert_eq!(p2.total, 3, "total 是过滤后全量，不随游标截断");
    assert_eq!(p2.rows.len(), 1, "续页补齐剩余 1 条");
    let ids: BTreeSet<String> = p1
        .rows
        .iter()
        .chain(p2.rows.iter())
        .map(|r| r.record_id.clone())
        .collect();
    assert_eq!(ids.len(), 3, "跨页不重不漏");
    assert!(
        p1.rows
            .iter()
            .chain(p2.rows.iter())
            .all(|r| r.project == "C:/test")
    );
    let bp = detail("C:/bee", None, 200);
    assert_eq!(bp.total, 2);
    assert!(bp.rows.iter().all(|r| r.project == "C:/bee"));
    assert!(
        bp.rows
            .iter()
            .all(|r| r.event_cwd.as_deref() == Some("C:/bee/123")
                || r.event_cwd.as_deref() == Some("C:/bee")),
        "明细保留切换后的原始工作目录"
    );
}
