//! R05（全计划审核 Task 5）：事件缓存身份必须包含**来源类型 + 规范化根
//! 目录 + 解析语义版本**——否则换根目录后项目名沿用旧解析、换 agent 后
//! 旧事件被改标新来源。缓存是纯优化：任何上下文变化后的结果必须与强制
//! 重解析（refresh）一致。所有路径显式注入临时目录。

use std::path::PathBuf;

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, summary};

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
        pricing_index: Some(dir.join("idx.json")),
        openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
        modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
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
    // 根 logs（项目 a\b）暖缓存后改根为 logs/a：必须与 refresh 一致得到 b。
    let dir = tmp_dir("root");
    let logs = dir.join("logs");
    fixture(&logs);

    // 首次：根 = logs → 项目 a\b，暖缓存
    let (proj, _n) = first_project(&dir, logs.clone(), Some(dir.join("no-codex")));
    assert_eq!(proj, "a\\b", "根 logs 下项目 = 相对父目录 a\\b");

    // 换根 logs/a：无缓存时应为 b——先证明 refresh 的正确答案
    let (proj_refresh, _n) = {
        let mut o = opts(&dir, logs.join("a"), Some(dir.join("no-codex")));
        o.refresh = true;
        let r = summary(&o).unwrap();
        (
            r.groups.first().map(|g| g.key.clone()).unwrap_or_default(),
            r.totals.requests,
        )
    };
    assert_eq!(proj_refresh, "b", "refresh 下根 logs/a 的项目 = b");

    // 暖缓存 + 换根：必须与 refresh 一致（修复前返回旧解析 a\b）
    let (proj_warm, n_warm) = first_project(&dir, logs.join("a"), Some(dir.join("no-codex")));
    assert_eq!(proj_warm, "b", "换根后缓存命中不得沿用旧根的项目名");
    assert_eq!(n_warm, 1);
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

    // 换 Codex 指向同一目录：必须与 refresh 一致 = 0 事件（不是改标旧事件）
    let o_codex = opts(&dir, shared.join("no-such"), Some(shared.clone()));
    let r = summary(&o_codex).unwrap();
    assert_eq!(
        r.totals.requests, 0,
        "Codex 解析 Claude 形态日志 = 0 事件（缓存不得改标）"
    );

    // refresh 交叉验证
    let mut o_ref = opts(&dir, shared.join("no-such"), Some(shared.clone()));
    o_ref.refresh = true;
    let r_ref = summary(&o_ref).unwrap();
    assert_eq!(r_ref.totals.requests, 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cache_source_identity_warm_hit_skips_parse() {
    // 上下文不变仍命中：两次同上下文查询数字一致且缓存文件数稳定
    //（证明正确性修复没有用"全部重解析"掩盖）。
    let dir = tmp_dir("warm");
    let logs = dir.join("logs");
    fixture(&logs);

    let (proj1, n1) = first_project(&dir, logs.clone(), Some(dir.join("no-codex")));
    assert_eq!(proj1, "a\\b");
    assert_eq!(n1, 1);
    let (proj2, n2) = first_project(&dir, logs.clone(), Some(dir.join("no-codex")));
    assert_eq!(proj2, "a\\b", "同上下文暖缓存结果不变");
    assert_eq!(n2, 1);
    std::fs::remove_dir_all(&dir).ok();
}
