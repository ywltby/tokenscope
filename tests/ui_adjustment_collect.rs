//! U06：自动读取（`collect_only`）与查询入口的 `collect` 拆分。
//!
//! 覆盖：
//! 1. `collect_interval_secs` 的缺省/非法/旧 TOML/手工非法值回退；
//! 2. `collect_only` 的 `changed` 语义与历史库 generation 递增；
//! 3. `begin_query_with(_, false)` 不采集（复用最近一次采集结果），
//!    缓存缺失或来源配置身份不一致时退回采集；
//! 4. `trigger` 沿查询快照传递（供日志分级使用）。
//!
//! 密闭性（CLAUDE.md）：全部用例注入临时 `cache_dir` / `pricing_index` /
//! `history_path`，绝不落回真实 `~/.tokenscope`。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::query;
use tokenscope::report::{self, SummaryOptions, Trigger};
use tokenscope::settings::{self, Settings};

static SEQ: AtomicU64 = AtomicU64::new(0);
// 最近采集槽位是进程级单份；本文件各用例不可互相驱逐其缓存。
static RECENT_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 每个用例独占的临时目录（并行测试之间不共享任何进程级缓存键）。
fn tmp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("tokenscope-u06-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 采集选项：只启用 Claude，来源目录为 `<dir>/claude`（用例自行放日志）。
fn base_opts(dir: &Path) -> SummaryOptions {
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    SummaryOptions {
        by: GroupBy::Day,
        agent: Some(AgentKind::ClaudeCode),
        claude_dir: Some(dir.join("claude")),
        codex_dir: Some(dir.join("no-codex")),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(data.clone()),
        history_path: Some(data.join("history.db")),
        pricing_index: Some(data.join("pricing-index.json")),
        pricing_path: Some(data.join("pricing.toml")),
        openrouter_path: Some(data.join("no-openrouter.json")),
        modelsdev_path: Some(data.join("no-modelsdev.json")),
        tz: Some("UTC".to_string()),
        trigger: Trigger::Manual,
        ..Default::default()
    }
}

/// 合成 Claude 会话日志（最小 fixture，不含任何真实日志内容）。
fn write_claude_session(root: &Path, session: &str, requests: &[(&str, u64)]) {
    let dir = root.join("proj");
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = String::new();
    for (i, (message_id, input)) in requests.iter().enumerate() {
        text.push_str(&format!(
            r#"{{"type":"assistant","timestamp":"2026-09-15T08:{:02}:00.000Z","sessionId":"{session}","cwd":"C:/work/alpha","message":{{"id":"{message_id}","model":"claude-opus-5-5","usage":{{"input_tokens":{input},"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#,
            i % 60
        ));
        text.push('\n');
    }
    std::fs::write(dir.join(format!("{session}.jsonl")), text).unwrap();
}

/// U06-1：缺省 300；非法值拒绝；旧 TOML 无该字段可读；手工非法值回退默认。
#[test]
fn collect_interval_defaults_validation_and_fallback() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(settings::default_collect_interval_secs(), 300);

    // 合法取值：5 / 10 / 30 / 60 / 300 / 600（其它一律拒绝，含 0 与 1）。
    for ok in [5u32, 10, 30, 60, 300, 600] {
        assert_eq!(settings::validate_collect_interval_secs(ok).unwrap(), ok);
    }
    for bad in [0u32, 1, 7, 51, 61, 299, 301, 601, 3600] {
        let err = settings::validate_collect_interval_secs(bad).unwrap_err();
        assert!(
            err.contains("5") && err.contains("600"),
            "错误必须列出可选值且中文可读: {err}"
        );
    }

    // 旧 TOML（无该字段）→ None（等价默认 300）。
    let dir = tmp_dir("settings-legacy");
    let path = dir.join("settings.toml");
    std::fs::write(&path, "price_auto_sync = false\n").unwrap();
    let s: Settings = settings::load(&path).unwrap();
    assert!(s.collect_interval_secs.is_none(), "旧文件无字段必须可读");
    assert_eq!(settings::collect_interval_secs_from(&s), 300);

    // 手工写入非法值 → 读取回退默认（不阻断启动）。
    std::fs::write(&path, "collect_interval_secs = 45\n").unwrap();
    let s = settings::load(&path).unwrap();
    assert_eq!(s.collect_interval_secs, Some(45), "字段照常解析出来");
    assert_eq!(
        settings::collect_interval_secs_from(&s),
        300,
        "非法值必须回退默认值"
    );

    // 合法值的显式写入照常生效。
    std::fs::write(&path, "collect_interval_secs = 30\n").unwrap();
    let s = settings::load(&path).unwrap();
    assert_eq!(settings::collect_interval_secs_from(&s), 30);
    std::fs::remove_dir_all(&dir).ok();
}

/// U06-3：无写入时 `changed == false`；有写入时 `changed == true` 且 generation 递增。
#[test]
fn collect_only_reports_change_and_bumps_generation() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("collect-only");
    let opts = base_opts(&dir);

    let first = report::collect_only(&opts).unwrap();
    assert!(!first.changed, "空来源目录不得报告变化");
    let generation_after_first = first.generation;

    write_claude_session(&dir.join("claude"), "s1", &[("m1", 10), ("m2", 20)]);
    let second = report::collect_only(&opts).unwrap();
    assert!(second.changed, "新写入的会话必须报告变化");
    assert!(
        second.generation > generation_after_first,
        "有写入时 generation 必须递增（{} → {}）",
        generation_after_first,
        second.generation
    );

    // 重复采集：指纹命中、无新增更新 → 不报告变化，generation 不动。
    let third = report::collect_only(&opts).unwrap();
    assert!(!third.changed, "同一内容重复采集不得报告变化");
    assert_eq!(
        third.generation, second.generation,
        "无写入时 generation 不得前进"
    );

    // 采集结果确实进了历史库（供后续查询读取）。
    let snap = query::begin_query_with(&opts, false).unwrap();
    let summary = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(summary.totals.requests, 2, "写入的 2 个请求必须可见");
    std::fs::remove_dir_all(&dir).ok();
}

/// U06-4a：`collect=false` 复用"最近一次采集结果"，不触发任何采集。
#[test]
fn begin_query_without_collect_reuses_recent_collection() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("collect-false-hit");
    let opts = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "s1", &[("m1", 10)]);

    let collected = report::collect_only(&opts).unwrap();
    write_claude_session(&dir.join("claude"), "uncollected", &[("m2", 99)]);

    let snap = query::begin_query_with(&opts, false).unwrap();
    assert_eq!(
        snap.generation, collected.generation,
        "快照必须复用最近一次采集的 generation"
    );
    assert_eq!(snap.trigger, Trigger::Manual, "trigger 必须从 opts 复制");

    let summary = query::query_summary(&snap.query_id).unwrap();
    assert_eq!(summary.totals.requests, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// 全应用采集覆盖单应用自动查询；来源统计随查询应用筛选。
#[test]
fn filtered_automatic_query_reuses_all_app_collection() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("filtered-reuse");
    let mut all = base_opts(&dir);
    all.agent = None;
    all.codex_enabled = Some(true);
    write_claude_session(&dir.join("claude"), "collected", &[("m1", 10)]);
    let collected = report::collect_only(&all).unwrap();
    // 如果应用筛选导致重新采集，下面这条不应可见的记录就会暴露它。
    write_claude_session(&dir.join("claude"), "uncollected", &[("m2", 99)]);
    for agent in [AgentKind::ClaudeCode, AgentKind::Codex] {
        let mut filtered = all.clone();
        filtered.agent = Some(agent);
        let snap = query::begin_query_with(&filtered, false).unwrap();
        assert_eq!(snap.generation, collected.generation, "应用筛选不得重采集");
        let summary = query::query_summary(&snap.query_id).unwrap();
        assert_eq!(
            summary.totals.requests,
            if agent == AgentKind::ClaudeCode { 1 } else { 0 }
        );
        assert_eq!(summary.sources.len(), 1);
        assert_eq!(summary.sources[0].agent, agent);
    }
    let snap = query::begin_query_with(&all, false).unwrap();
    assert_eq!(snap.sources().len(), 2, "筛选视图不得修改共享采集结果");
    assert_eq!(
        query::query_summary(&snap.query_id)
            .unwrap()
            .totals
            .requests,
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 单应用采集不能充当全部应用采集；改变采集范围必须补采集。
#[test]
fn filtered_collection_does_not_cover_all_apps() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("filtered-not-all");
    let filtered = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "s1", &[("m1", 10)]);
    report::collect_only(&filtered).unwrap();
    write_claude_session(&dir.join("claude"), "s2", &[("m2", 20)]);
    let mut all = filtered.clone();
    all.agent = None;
    let snap = query::begin_query_with(&all, false).unwrap();
    assert_eq!(
        query::query_summary(&snap.query_id)
            .unwrap()
            .totals
            .requests,
        2
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// U06-4b：缓存缺失（或来源配置身份不一致）时退回采集，而不是空快照。
#[test]
fn begin_query_without_collect_falls_back_when_cache_missing() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("collect-false-miss");
    let opts = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "s1", &[("m1", 10)]);

    // 从未采集过 → 没有"最近一次采集结果"可用。
    let snap = query::begin_query_with(&opts, false).unwrap();
    assert!(
        query::query_summary(&snap.query_id)
            .unwrap()
            .totals
            .requests
            == 1,
        "退回采集后数字必须完整"
    );

    // 来源配置身份变化（换目录）→ 旧缓存不得被误用。
    let mut moved = opts.clone();
    let other = dir.join("claude-moved");
    write_claude_session(&other, "s2", &[("m9", 5), ("m10", 5)]);
    moved.claude_dir = Some(other);
    let snap = query::begin_query_with(&moved, false).unwrap();
    assert_eq!(
        query::query_summary(&snap.query_id)
            .unwrap()
            .totals
            .requests,
        3,
        "换目录后按新目录重新采集（同一历史库累计两个来源目录的事实：1 + 2）"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 从 report 单元测试迁入：覆盖同键命中、异键隔离和连续成功采集的发布。
/// 必须与本测试二进制内所有采集共享互斥域，不能依赖全量 lib 测试的调度时序。
#[test]
fn recent_collection_hits_only_matching_key() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("recent-key");
    let other_dir = tmp_dir("recent-other-key");
    let opts = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "first", &[("m1", 10)]);
    let first = report::collect_only(&opts).unwrap();
    assert!(first.changed);
    write_claude_session(&dir.join("claude"), "second", &[("m2", 20)]);
    let hit = query::begin_query_with(&opts, false).unwrap();
    assert_eq!(hit.generation, first.generation);
    assert_eq!(
        query::query_summary(&hit.query_id).unwrap().totals.requests,
        1
    );

    // 不同目录绝不能拿前一个目录的快照冒充已采集；缺失时必须采集自己的事实。
    let other = base_opts(&other_dir);
    write_claude_session(
        &other_dir.join("claude"),
        "other",
        &[("m3", 30), ("m4", 40), ("m5", 50)],
    );
    let other_hit = query::begin_query_with(&other, false).unwrap();
    assert_eq!(
        query::query_summary(&other_hit.query_id)
            .unwrap()
            .totals
            .requests,
        3
    );

    let second = report::collect_only(&opts).unwrap();
    assert!(second.changed);
    assert!(second.generation > first.generation);
    write_claude_session(&dir.join("claude"), "third", &[("m6", 60)]);
    let hit = query::begin_query_with(&opts, false).unwrap();
    assert_eq!(hit.generation, second.generation);
    assert_eq!(
        query::query_summary(&hit.query_id).unwrap().totals.requests,
        2
    );
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&other_dir).ok();
}

/// U06-4c：`begin_query` 仍等价于 `begin_query_with(_, true)`（既有调用点零改动），
/// 且 `Trigger::Auto` 沿快照传递。
#[test]
fn begin_query_default_collects_and_carries_auto_trigger() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("collect-true");
    let opts = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "s1", &[("m1", 10)]);

    let snap = query::begin_query(&opts).unwrap();
    assert_eq!(
        query::query_summary(&snap.query_id)
            .unwrap()
            .totals
            .requests,
        1,
        "begin_query 必须采集（既有语义不变）"
    );
    assert_eq!(snap.trigger, Trigger::Manual);

    let mut auto = opts.clone();
    auto.trigger = Trigger::Auto;
    let auto_snap = query::begin_query_with(&auto, false).unwrap();
    assert_eq!(
        auto_snap.trigger,
        Trigger::Auto,
        "trigger 必须随快照传给 query_summary_from 的日志分级"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cached_generation_matches_fresh_history_snapshot() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("snapshot-generation");
    let opts = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "s1", &[("m1", 10)]);
    let initial = report::collect_only(&opts).unwrap();
    write_claude_session(&dir.join("claude"), "s2", &[("m2", 20)]);
    let manual = query::begin_query_with(&opts, true).unwrap();
    assert!(manual.generation > initial.generation);
    let automatic = query::begin_query_with(&opts, false).unwrap();
    let facts = query::query_summary(&automatic.query_id).unwrap();
    assert_eq!(facts.totals.requests, 2);
    assert_eq!(
        automatic.generation, manual.generation,
        "current facts must carry current generation"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn automatic_query_keeps_prices_loaded_by_manual_refresh() {
    let _serial = RECENT_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tmp_dir("price-refresh");
    let opts = base_opts(&dir);
    write_claude_session(&dir.join("claude"), "s1", &[("m1", 1_000_000)]);
    let write_price = |price| {
        std::fs::write(
            opts.pricing_path.as_ref().unwrap(),
            format!("[[model]]\nprefix = \"claude-opus-5-5\"\ninput = {price}.0\noutput = 0.0\n"),
        )
        .unwrap();
    };
    write_price(1);
    report::collect_only(&opts).unwrap();
    let old = query::begin_query_with(&opts, false).unwrap();
    assert_eq!(
        query::query_summary(&old.query_id).unwrap().totals.cost_usd,
        1.0
    );
    write_price(9);
    let manual = query::begin_query_with(&opts, true).unwrap();
    assert_eq!(
        query::query_summary(&manual.query_id)
            .unwrap()
            .totals
            .cost_usd,
        9.0
    );
    let automatic = query::begin_query_with(&opts, false).unwrap();
    assert_eq!(
        query::query_summary(&automatic.query_id)
            .unwrap()
            .totals
            .cost_usd,
        9.0
    );
    assert_eq!(automatic.pricing_revision, manual.pricing_revision);
    assert_eq!(
        query::query_summary(&old.query_id).unwrap().totals.cost_usd,
        1.0,
        "already published query remains frozen"
    );
    std::fs::remove_dir_all(dir).ok();
}
