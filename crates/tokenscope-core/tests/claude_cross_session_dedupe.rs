//! Claude 跨会话复制请求去重（计划 2026-11-07，C01/C02/C04）。
//!
//! 背景：同一次模型调用会被复制到不同会话（分支/复制会话），四桶、模型、
//! 事件时间、`message.id`、`requestId` 与消息 `uuid` 完全一致，只有
//! `sessionId` 不同。旧实现把 `(session_id, message.id)` 当作全部身份，
//! 两份副本各自成为一条事实，请求数、token 与费用双计。
//!
//! 本文件只使用合成日志：真实 agent 日志不入库（CLAUDE.md）。历史库、缓存、
//! 价格索引全部落在本次测试的独立临时目录。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, SummaryReport, summary};

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-claude-copy-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn opts(dir: &Path, root: &Path) -> SummaryOptions {
    SummaryOptions {
        workbuddy_enabled: Some(false),
        workbuddy_ai_enabled: Some(false),
        by: GroupBy::Day,
        claude_dir: Some(root.to_path_buf()),
        claude_enabled: Some(true),
        codex_enabled: Some(false),
        cache_dir: Some(dir.join("data")),
        pricing_index: Some(dir.join("idx.json")),
        pricing_path: Some(dir.join("pricing.toml")),
        openrouter_path: Some(dir.join("or.json")),
        modelsdev_path: Some(dir.join("md.json")),
        tz: Some("Asia/Shanghai".to_string()),
        ..Default::default()
    }
}

/// 一条 assistant 用量行。`copy` = `(session, requestId, uuid)`：
/// 复制副本保留原请求的 (message.id, requestId, uuid) 与时间，只换会话。
struct Line<'a> {
    session: &'a str,
    request_id: Option<&'a str>,
    uuid: Option<&'a str>,
    message_id: &'a str,
    ts: &'a str,
    input: u64,
    output: u64,
    cache_write: u64,
    cache_read: u64,
    cwd: Option<&'a str>,
}

impl<'a> Line<'a> {
    fn copied(session: &'a str) -> Self {
        Self {
            session,
            request_id: Some("req_01"),
            uuid: Some("uuid-01"),
            message_id: "msg_01",
            ts: "2026-09-15T08:00:00.000Z",
            input: 10,
            output: 2,
            cache_write: 3,
            cache_read: 4,
            cwd: None,
        }
    }

    fn to_json(&self) -> String {
        let mut obj = serde_json::json!({
            "type": "assistant",
            "timestamp": self.ts,
            "sessionId": self.session,
            "message": {
                "id": self.message_id,
                "model": "claude-opus-5-5",
                "usage": {
                    "input_tokens": self.input,
                    "output_tokens": self.output,
                    "cache_creation_input_tokens": self.cache_write,
                    "cache_read_input_tokens": self.cache_read,
                }
            }
        });
        if let Some(rid) = self.request_id {
            obj["requestId"] = serde_json::Value::String(rid.to_string());
        }
        if let Some(u) = self.uuid {
            obj["uuid"] = serde_json::Value::String(u.to_string());
        }
        if let Some(cwd) = self.cwd {
            obj["cwd"] = serde_json::Value::String(cwd.to_string());
        }
        obj.to_string()
    }
}

fn write_lines(path: &Path, lines: &[Line<'_>]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body = lines
        .iter()
        .map(Line::to_json)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(path, body + "\n").unwrap();
}

fn report(dir: &Path, root: &Path) -> SummaryReport {
    summary(&opts(dir, root)).unwrap()
}

/// 复制到另一会话的同一次调用只计一次；分支后真正发出的新调用照常计入。
#[test]
fn copied_request_counts_once_and_branched_call_counts_again() {
    let dir = temp_dir("once");
    let root = dir.join("projects");
    // 原会话：一次调用（10/2/3/4 = 19 token）。
    write_lines(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[Line::copied("s-a")],
    );
    // 复制出来的会话：同一次调用（同 message.id / requestId / uuid / 时间 / 四桶）
    // + 分支后真正发出的新调用（新 message.id / requestId / uuid）。
    write_lines(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[
            Line::copied("s-b"),
            Line {
                session: "s-b",
                request_id: Some("req_02"),
                uuid: Some("uuid-02"),
                message_id: "msg_02",
                ts: "2026-09-15T08:05:00.000Z",
                input: 5,
                output: 1,
                cache_write: 0,
                cache_read: 0,
                cwd: None,
            },
        ],
    );

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "复制副本只计一次，分支后的新调用额外计一次"
    );
    assert_eq!(
        r.totals.tokens.total(),
        19 + 6,
        "四桶 = 原调用 + 新调用；副本不贡献 token"
    );
    assert_eq!(r.totals.tokens.input, 15);
    assert_eq!(r.totals.tokens.output, 3);
    assert_eq!(r.totals.tokens.cache_write, 3);
    assert_eq!(r.totals.tokens.cache_read, 4);

    // 重复查询（重启/重扫）不改变结果。
    let again = report(&dir, &root);
    assert_eq!(again.totals.requests, 2);
    assert_eq!(again.totals.tokens.total(), 25);
    std::fs::remove_dir_all(&dir).ok();
}

/// 相同 token、相同时间、相同模型的不同真实请求不得合并——身份不同就是
/// 不同调用，不按用量猜测。
#[test]
fn distinct_requests_with_identical_usage_are_not_merged() {
    let dir = temp_dir("same-usage");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[
            Line {
                session: "s-a",
                request_id: Some("req_a"),
                uuid: Some("uuid-a"),
                message_id: "msg_a",
                ts: "2026-09-15T08:00:00.000Z",
                input: 10,
                output: 2,
                cache_write: 3,
                cache_read: 4,
                cwd: None,
            },
            Line {
                session: "s-a",
                request_id: Some("req_b"),
                uuid: Some("uuid-b"),
                message_id: "msg_b",
                ts: "2026-09-15T08:00:00.000Z",
                input: 10,
                output: 2,
                cache_write: 3,
                cache_read: 4,
                cwd: None,
            },
        ],
    );
    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 2, "标识不同即独立请求");
    assert_eq!(r.totals.tokens.total(), 38);
    std::fs::remove_dir_all(&dir).ok();
}

/// 只有 `message.id` 相同（requestId/uuid 不同或缺失）时不得合并：
/// 这是"缺少证据不猜"的边界，副本必须保留。
#[test]
fn missing_copy_evidence_is_not_merged() {
    let dir = temp_dir("missing");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[Line::copied("s-a")],
    );
    write_lines(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[Line {
            request_id: None,
            uuid: None,
            ..Line::copied("s-b")
        }],
    );
    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "缺 requestId/uuid 的副本缺少复制证据，不得强行合并"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 标识齐备但内容矛盾（同 uuid 不同四桶）时不得合并。
#[test]
fn contradictory_content_is_not_merged() {
    let dir = temp_dir("contradiction");
    let root = dir.join("projects");
    write_lines(
        &root.join("proj-a").join("sess-a.jsonl"),
        &[Line::copied("s-a")],
    );
    write_lines(
        &root.join("proj-b").join("sess-b.jsonl"),
        &[Line {
            input: 99,
            ..Line::copied("s-b")
        }],
    );
    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "同一标识的观察内容矛盾 → 不能作为复制证据"
    );
    assert_eq!(r.totals.tokens.total(), 19 + 108);
    std::fs::remove_dir_all(&dir).ok();
}
