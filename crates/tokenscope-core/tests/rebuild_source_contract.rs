//! AP03：重建缓存的来源契约——重建必须与普通查询使用**同一份**生效来源
//! 配置。修复前 `refresh_cache` 走 `rebuild_cache(None)` →
//! `SummaryOptions::default()`：设置里停用的来源被重新采集、自定义目录被
//! 忽略，重建出的缓存与查询口径不一致。
//!
//! 本文件全程注入采集选项（来源目录、cache_dir、pricing_index、价格文件与
//! 双快照），不触碰真实 `~/.tokenscope` 与真实 agent 日志。

use std::path::{Path, PathBuf};

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, SummaryReport, cache_stats, rebuild_cache, summary};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-rebuild-src-{tag}-{}-{}",
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

/// 写一个合法的 Claude 会话文件（`events` 条 assistant 事件 = `events` 个请求）。
fn write_claude_log(root: &Path, session: &str, events: usize) {
    std::fs::create_dir_all(root).unwrap();
    let mut body = String::new();
    for i in 0..events {
        body.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-08-01T10:0{i}:00.000Z","sessionId":"{session}","message":{{"id":"{session}-m{i}","model":"claude-sonnet-4-5","usage":{{"input_tokens":100,"output_tokens":10}}}}}}"#
        ));
        body.push('\n');
    }
    std::fs::write(root.join(format!("{session}.jsonl")), body).unwrap();
}

/// 写一个合法的 Codex rollout 文件（1 个请求）。
fn write_codex_log(root: &Path, session: &str) {
    let day = root.join("2026").join("08").join("01");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
        day.join(format!("rollout-{session}.jsonl")),
        concat!(
            r#"{"timestamp":"2026-08-01T10:00:00.000Z","type":"session_meta","payload":{"id":"s1","session_id":"s1","cwd":"C:/w/alpha"}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T10:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/w/alpha"}}"#,
            "\n",
            r#"{"timestamp":"2026-08-01T10:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":10,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":110}}}}"#,
            "\n",
        ),
    )
    .unwrap();
}

/// 密闭基础选项：所有路径都指向本次临时目录。
fn base_opts(dir: &Path) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),

        by: GroupBy::Day,
        cache_dir: Some(dir.join("cache")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

fn agents_of(r: &SummaryReport) -> Vec<tokenscope::model::AgentKind> {
    r.sources.iter().map(|s| s.agent).collect()
}

/// AP03：停用的来源在重建时也不得被采集。
#[test]
fn rebuild_respects_disabled_sources() {
    let dir = tmp_dir("disabled");
    let claude = dir.join("claude");
    write_claude_log(&claude, "c1", 2);
    let codex = dir.join("codex");
    std::fs::create_dir_all(&codex).unwrap();

    let mut o = base_opts(&dir);
    o.claude_dir = Some(claude);
    o.codex_dir = Some(codex);
    o.claude_enabled = Some(false);
    o.codex_enabled = Some(false);
    let info = rebuild_cache(&o).unwrap();
    assert_eq!(info.events, 0, "停用的来源不得被重建采集");
    assert_eq!(info.files, 0);
    assert_eq!(summary(&o).unwrap().totals.requests, 0);

    // 反向对照：启用后同一份配置确实采集到 2 条。
    let mut on = o.clone();
    on.claude_enabled = Some(true);
    let info = rebuild_cache(&on).unwrap();
    assert_eq!(info.events, 2, "启用来源后重建按当前配置采集");
    std::fs::remove_dir_all(&dir).ok();
}

/// AP03：重建只使用**当前配置的**来源根，默认根不在范围内。
#[test]
fn rebuild_uses_custom_source_roots() {
    let dir = tmp_dir("custom-roots");
    let default_root = dir.join("claude-default");
    write_claude_log(&default_root, "s-default", 2);
    let custom_root = dir.join("claude-custom");
    write_claude_log(&custom_root, "s-custom", 1);
    let codex = dir.join("codex");
    std::fs::create_dir_all(&codex).unwrap();

    let mut o = base_opts(&dir);
    o.codex_dir = Some(codex);
    o.codex_enabled = Some(false);
    // 自定义根在位：默认根的数据不得进入采集范围。
    o.claude_dir = Some(custom_root);
    let info = rebuild_cache(&o).unwrap();
    assert_eq!(info.events, 1, "重建只含当前配置的自定义根数据");
    assert_eq!(summary(&o).unwrap().totals.requests, 1);

    // 换成默认根：按新根采集（证明范围完全由配置决定，而非写死的默认）。
    // H04：换根**不删除**已保存的用量——旧根的事件保留，新根的事件补入；
    // 历史库是累积的用量事实，不是"当前扫描结果的镜像"。
    let mut o2 = o.clone();
    o2.claude_dir = Some(default_root);
    let info2 = rebuild_cache(&o2).unwrap();
    assert_eq!(info2.events, 3, "自定义根 1 条 + 默认根 2 条（历史累积）");
    assert_eq!(summary(&o2).unwrap().totals.requests, 3);
    std::fs::remove_dir_all(&dir).ok();
}

/// PR3 §4.1/§8.2：跨应用来源根重叠不再是采集错误——重叠根被隔离（诊断可
/// 见），其余照常采集；重建不清空已有缓存。硬拒绝移到保存事务（S04 接线）。
#[test]
fn overlapping_sources_are_isolated_and_keep_previous_cache() {
    let dir = tmp_dir("overlap-isolated");
    let claude = dir.join("claude");
    write_claude_log(&claude, "c1", 1);
    let codex = dir.join("codex");
    write_codex_log(&codex, "x1");

    let mut ok = base_opts(&dir);
    ok.claude_dir = Some(claude.clone());
    ok.codex_dir = Some(codex);
    let info = rebuild_cache(&ok).unwrap();
    assert_eq!(info.events, 2, "基线：claude 1 + codex 1");

    // codex 根 = claude 根（跨应用重叠）：不整体报错，codex 根被隔离
    //（诊断可见），claude 照常采集；已保存的历史事实不删除（H04）。
    let mut bad = ok.clone();
    bad.codex_dir = Some(claude.clone());
    let info2 = rebuild_cache(&bad).unwrap();
    assert_eq!(info2.events, 2, "历史事实保留：隔离不删除已保存用量");
    let report = summary(&bad).unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("跨应用来源根重叠")),
        "隔离必须诊断可见: {:?}",
        report.warnings
    );
    let after = cache_stats(ok.cache_dir.clone()).unwrap();
    assert_eq!(after.events, info.events, "隔离/重建不得删除已保存事实");
    assert!(after.files >= 1, "claude 根本轮照常登记");

    // 重建真正失败（历史库路径是目录 → 打不开）同样不得清空缓存。
    let mut broken = ok.clone();
    broken.history_path = Some(dir.join("history-as-dir"));
    std::fs::create_dir_all(dir.join("history-as-dir")).unwrap();
    assert!(rebuild_cache(&broken).is_err());
    let after2 = cache_stats(ok.cache_dir.clone()).unwrap();
    assert_eq!(after2.files, after.files, "失败不得清空缓存");
    assert_eq!(after2.events, after.events);
    std::fs::remove_dir_all(&dir).ok();
}

/// AP03：重建前后的查询口径一致——请求数、四类 token 与来源身份。
#[test]
fn rebuild_matches_query_totals_and_sources() {
    let dir = tmp_dir("identity");
    let claude = dir.join("claude");
    write_claude_log(&claude, "c1", 2);
    let codex = dir.join("codex");
    write_codex_log(&codex, "x1");

    let mut o = base_opts(&dir);
    o.claude_dir = Some(claude);
    o.codex_dir = Some(codex);
    let before = summary(&o).unwrap();
    let info = rebuild_cache(&o).unwrap();
    assert_eq!(info.events, 3, "重建覆盖全部启用来源");
    let after = summary(&o).unwrap();
    assert_eq!(after.totals.requests, before.totals.requests);
    assert_eq!(after.totals.tokens, before.totals.tokens);
    assert_eq!(agents_of(&after), agents_of(&before), "来源身份一致");
    std::fs::remove_dir_all(&dir).ok();
}
