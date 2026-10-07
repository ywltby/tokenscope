//! R05（全计划审核 Task 5）：事件缓存身份必须包含**来源类型 + 规范化根
//! 目录 + 解析语义版本**——否则换根目录后项目名沿用旧解析、换 agent 后
//! 旧事件被改标新来源。缓存是纯优化：任何上下文变化后的结果必须与强制
//! 重解析（refresh）一致。所有路径显式注入临时目录。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, summary};
use tokenscope::source::{FileParse, Source, claude::ClaudeSource};

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-csi-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 日志布局：`logs/a/b/s.jsonl`（Claude assistant 形态）。
/// 项目身份 = 相对根的父目录：根 `logs` → `a\b`；根 `logs/a` → `b`。
fn fixture(root: &std::path::Path) {
    let dir = root.join("a").join("b");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("s.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c1","message":{"id":"m","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}}"#,
    )
    .unwrap();
}

fn opts(
    dir: &std::path::Path,
    claude_root: PathBuf,
    codex_root: Option<PathBuf>,
) -> SummaryOptions {
    SummaryOptions {
        by: GroupBy::Project,
        claude_dir: Some(claude_root),
        codex_dir: codex_root,
        cache_dir: Some(dir.join("cache")),
        pricing_path: Some(dir.join("pricing.toml")),
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    }
}

fn first_project(
    dir: &std::path::Path,
    claude_root: PathBuf,
    codex_root: Option<PathBuf>,
) -> (String, u64) {
    let r = summary(&opts(dir, claude_root, codex_root)).unwrap();
    (
        r.groups.first().map(|g| g.key.clone()).unwrap_or_default(),
        r.totals.requests,
    )
}

#[test]
fn cache_source_identity_root_change_matches_refresh() {
    // 根 logs（项目 a\b）暖缓存后改根为 logs/a：**先验证换根暖查询**——
    // 缓存命中不得沿用旧根的项目名；refresh 只作对照，不能先用它清库。
    let dir = tmp_dir("root");
    let logs = dir.join("logs");
    fixture(&logs);

    // 首次：根 = logs → 项目 a\b，暖缓存
    let (proj, _n) = first_project(&dir, logs.clone(), Some(dir.join("no-codex")));
    assert_eq!(proj, "a\\b", "根 logs 下项目 = 相对父目录 a\\b");

    // 暖缓存 + 换根（不 refresh）：必须得到新根的解析 b（修复前返回旧解析 a\b）
    let (proj_warm, n_warm) = first_project(&dir, logs.join("a"), Some(dir.join("no-codex")));
    assert_eq!(proj_warm, "b", "换根后缓存命中不得沿用旧根的项目名");
    assert_eq!(n_warm, 1);

    // refresh 对照：强制重解析同样得到 b——证明暖缓存与重解析一致
    let mut o_ref = opts(&dir, logs.join("a"), Some(dir.join("no-codex")));
    o_ref.refresh = true;
    let r_ref = summary(&o_ref).unwrap();
    let proj_refresh = r_ref
        .groups
        .first()
        .map(|g| g.key.clone())
        .unwrap_or_default();
    assert_eq!(proj_refresh, "b", "refresh 对照：根 logs/a 的项目 = b");
    assert_eq!(proj_warm, proj_refresh, "暖缓存必须与重解析一致");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cache_source_identity_agent_change_reparses() {
    // 同一路径先用 ClaudeSource 再用 CodexSource：Codex 解析 Claude 形态
    // 日志 = 0 事件；缓存不得返回改标 agent 的旧事件。
    let dir = tmp_dir("agent");
    let shared = dir.join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(
        shared.join("s.jsonl"),
        r#"{"type":"assistant","timestamp":"2026-08-01T10:00:00.000Z","sessionId":"c1","message":{"id":"m","model":"claude-sonnet-4-5","usage":{"input_tokens":100,"output_tokens":10}}}"#,
    )
    .unwrap();

    // Claude 先行：1 事件暖缓存
    let (_p, n_claude) = first_project(&dir, shared.clone(), Some(dir.join("no-codex")));
    assert_eq!(n_claude, 1);

    // 换 Codex 指向同一目录：**先验证暖查询** = 0 事件（不是改标旧事件）
    let o_codex = opts(&dir, shared.join("no-such"), Some(shared.clone()));
    let r = summary(&o_codex).unwrap();
    assert_eq!(
        r.totals.requests, 0,
        "Codex 解析 Claude 形态日志 = 0 事件（缓存不得改标）"
    );

    // refresh 对照：强制重解析同样 0 事件
    let mut o_ref = opts(&dir, shared.join("no-such"), Some(shared.clone()));
    o_ref.refresh = true;
    let r_ref = summary(&o_ref).unwrap();
    assert_eq!(r_ref.totals.requests, 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 包装 ClaudeSource 并统计 parse_file 调用次数——暖命中证明用，
/// 不能只靠"两次结果相同"推断没有重解析。
struct CountingSource {
    inner: ClaudeSource,
    parse_calls: Arc<AtomicUsize>,
}

impl Source for CountingSource {
    fn agent(&self) -> tokenscope::model::AgentKind {
        self.inner.agent()
    }
    fn root(&self) -> &std::path::Path {
        self.inner.root()
    }
    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        self.inner.discover_with_errors()
    }
    fn parse_file(&self, path: &std::path::Path) -> FileParse {
        self.parse_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.parse_file(path)
    }
}

#[test]
fn cache_source_identity_warm_hit_skips_parse() {
    // 上下文不变仍命中：第二次采集 parse_file 调用次数不增长
    //（CountingSource 证明，不靠比较两个相同结果）。
    let dir = tmp_dir("warm");
    let logs = dir.join("logs");
    fixture(&logs);
    let o = opts(&dir, logs.clone(), Some(dir.join("no-codex")));

    // 第一次：冷启动必须真实解析
    let calls = Arc::new(AtomicUsize::new(0));
    let src1 = CountingSource {
        inner: ClaudeSource::new(logs.clone()),
        parse_calls: calls.clone(),
    };
    let r1 =
        tokenscope::report::collect_all_with_sources_for_test(vec![Box::new(src1)], &o).unwrap();
    assert_eq!(r1.events.len(), 1);
    let cold_calls = calls.load(Ordering::SeqCst);
    assert_eq!(cold_calls, 1, "冷启动必须解析 1 个文件");

    // 第二次：同上下文暖缓存——parse 次数不得增长
    let src2 = CountingSource {
        inner: ClaudeSource::new(logs.clone()),
        parse_calls: calls.clone(),
    };
    let r2 =
        tokenscope::report::collect_all_with_sources_for_test(vec![Box::new(src2)], &o).unwrap();
    assert_eq!(r2.events.len(), 1, "同上下文暖缓存结果不变");
    assert_eq!(
        calls.load(Ordering::SeqCst),
        cold_calls,
        "暖命中必须跳过解析（parse_file 次数不增长）"
    );
    std::fs::remove_dir_all(&dir).ok();
}
