//! Codex 正常请求身份与漏计（计划 2026-11-08，K01）。
//!
//! 背景（qa-artifacts/codex-normal-d011823e 与 codex-operations-1b1ad785 的
//! 合成接口对账，真实 Codex 0.166.0 落盘、未手工改写；此处按最小结构重写为
//! 仓库内合成 fixture，路径、会话与响应 ID 均为合成值）：rollout 的
//! `token_usage_record` 已带 `thread_id/session_id/turn_id/response_id` 与
//! 单次 `usage`，但解析器把它挂起后按相邻 `token_count.last_token_usage`
//! 出数，事件没有任何请求身份。全局去重与历史库使用
//! `(session, model, 四桶)` 保守重播指纹，两次独立响应只要四桶相同就被
//! 合并——普通会话、一轮工具循环、回退后的新请求与 legacy fork 的子请求
//! 都会漏计。
//!
//! 不变量（计划 §2）：
//! 1. 不同 response_id 即使用量完全相同也分别计入；同一响应的副本
//!    （复制历史/重播）只计一次。
//! 2. 禁止按 turn_id 去重——一轮工具循环内可有多次模型调用，turn_id 相同、
//!    response_id 不同是两个请求。
//! 3. `token_usage_record` 的原始线程身份优先于复制历史后遗留的解析状态；
//!    legacy fork 子文件中 thread_id 是谁的，请求就归谁。
//! 4. `token_usage_record` 与 `token_count` 是同一响应的两种观察，合计只产
//!    一条事实。
//!
//! 本文件只使用合成日志：真实 agent 日志不入库（CLAUDE.md）。历史库、缓存、
//! 价格索引全部落在本次测试的独立临时目录，并通过 `run_rust_tests.py` 的
//! 隔离临时根执行。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::GroupBy;
use tokenscope::report::{SummaryOptions, SummaryReport, summary};
use tokenscope::source::Source;
use tokenscope::source::codex::CodexSource;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-codex-identity-{tag}-{}-{n}-{}",
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
        by: GroupBy::Agent,
        claude_dir: Some(dir.join("no-claude")),
        claude_enabled: Some(false),
        codex_dir: Some(root.to_path_buf()),
        codex_enabled: Some(true),
        cache_dir: Some(dir.join("data")),
        history_path: Some(dir.join("data").join("history.db")),
        pricing_index: Some(dir.join("data").join("idx.json")),
        pricing_path: Some(dir.join("data").join("pricing.toml")),
        openrouter_path: Some(dir.join("data").join("or.json")),
        modelsdev_path: Some(dir.join("data").join("md.json")),
        claude_projects_path: Some(dir.join("no-claude-projects.json")),
        tz: Some("UTC".to_string()),
        ..Default::default()
    }
}

fn report(dir: &Path, root: &Path) -> SummaryReport {
    summary(&opts(dir, root)).unwrap()
}

const MODEL: &str = "gpt-5.1";
const PROJECT: &str = "C:/synthetic/codex-identity-workspace";

/// 基础单响应真值（研究口径）：raw input=1000、cached=600、cache_write=100、
/// output=100（reasoning=40 已含在内）、total=1100。TS 四桶 = 300/100/600/100。
const USAGE_1100: Usage = Usage {
    input: 1000,
    cached: 600,
    cache_write: 100,
    output: 100,
    reasoning: 40,
    total: 1100,
};

#[derive(Clone, Copy)]
struct Usage {
    input: u64,
    cached: u64,
    cache_write: u64,
    output: u64,
    reasoning: u64,
    total: u64,
}

impl Usage {
    /// usage JSON（token_usage_record 与 token_count.info.last_token_usage 同形）。
    fn value(self) -> serde_json::Value {
        serde_json::json!({
            "input_tokens": self.input,
            "cached_input_tokens": self.cached,
            "cache_write_input_tokens": self.cache_write,
            "output_tokens": self.output,
            "reasoning_output_tokens": self.reasoning,
            "total_tokens": self.total,
        })
    }
}

/// `token_usage_record` 行：单次响应用量 + 原始线程/turn/响应身份。
struct Record<'a> {
    ts: &'a str,
    session: &'a str,
    turn: &'a str,
    response: &'a str,
    usage: Usage,
}

impl Record<'_> {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "timestamp": self.ts,
            "type": "token_usage_record",
            "payload": {
                "thread_id": self.session,
                "turn_id": self.turn,
                "session_id": self.session,
                "root_turn_id": self.turn,
                "response_id": self.response,
                "usage": self.usage.value(),
                "turn_token_usage": self.usage.value(),
                "thread_token_usage": self.usage.value(),
            }
        })
    }
}

/// `token_count` 行：`info.last_token_usage` 是当前实现实际出数的观察，
/// `info.total_token_usage` 是线程累计（fixture 只在多轮时给出）。
struct Count<'a> {
    ts: &'a str,
    last: Usage,
    total: Usage,
}

impl Count<'_> {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "timestamp": self.ts,
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": self.total.value(),
                    "last_token_usage": self.last.value(),
                    "last_usage_includes_reasoning": false,
                    "model_context_window": 258400,
                }
            }
        })
    }
}
/// 简单会话骨架：一条 session_meta + 一条 turn_context（模型与项目根）。
fn meta(session: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": "2026-11-07T16:00:00.000Z",
        "type": "session_meta",
        "payload": {
            "id": session,
            "session_id": session,
            "cwd": PROJECT,
        }
    })
}

fn ctx(ts: &str) -> serde_json::Value {
    serde_json::json!({
        "timestamp": ts,
        "type": "turn_context",
        "payload": {
            "turn_id": "ctx-turn",
            "model": MODEL,
            "cwd": PROJECT,
        }
    })
}

/// 把记录写成 `<root>/sessions/<file>.jsonl`。
fn write_rollout(root: &Path, name: &str, lines: &[serde_json::Value]) -> PathBuf {
    let file = root.join("sessions").join(format!("{name}.jsonl"));
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let body = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, body + "\n").unwrap();
    file
}

/// 一次普通 turn：record → token_count(last=本次, total=累计)。
///
/// 真实 Codex 0.166.0 落盘中 record 与配对 token_count 的时间可相差
/// 1–3ms：`record_ts` 与 `count_ts` 分开传入，两行不再是同一时刻。
fn turn(
    session: &str,
    turn_id: &str,
    response: &str,
    usage: Usage,
    cumulative: Usage,
    record_ts: &str,
    count_ts: &str,
) -> Vec<serde_json::Value> {
    vec![
        Record {
            ts: record_ts,
            session,
            turn: turn_id,
            response,
            usage,
        }
        .to_json(),
        Count {
            ts: count_ts,
            last: usage,
            total: cumulative,
        }
        .to_json(),
    ]
}

// ── §1 反例 1：普通三轮，四桶完全相同，三个不同 response_id ──────────────
// 真值 3300 token / 3 请求；旧实现 1100 / 1。

#[test]
fn three_equal_usage_responses_count_separately() {
    let dir = temp_dir("three-equal");
    let root = dir.join("codex");
    let s = "sess-equal";
    let mut lines = vec![meta(s), ctx("2026-11-07T16:00:00.000Z")];
    let scale = |n: u64| Usage {
        input: USAGE_1100.input * n,
        cached: USAGE_1100.cached * n,
        cache_write: USAGE_1100.cache_write * n,
        output: USAGE_1100.output * n,
        reasoning: USAGE_1100.reasoning * n,
        total: USAGE_1100.total * n,
    };
    for (n, ts) in [
        (1u64, "2026-11-07T16:00:01.000Z"),
        (2, "2026-11-07T16:00:02.000Z"),
        (3, "2026-11-07T16:00:03.000Z"),
    ] {
        let response = format!("resp_synthetic_{n}");
        let count_ts = offset_ms(ts, 1);
        lines.extend(turn(
            s,
            "turn-1",
            &response,
            USAGE_1100,
            scale(n),
            ts,
            &count_ts,
        ));
    }
    write_rollout(&root, "rollout-equal", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 3, "三个不同响应各自计一次");
    assert_eq!(r.totals.tokens.total(), 3300);
    assert_eq!(r.totals.tokens.input, 900, "四桶逐项核对，不只看总量");
    assert_eq!(r.totals.tokens.output, 300);
    assert_eq!(r.totals.tokens.cache_write, 300);
    assert_eq!(r.totals.tokens.cache_read, 1800);
    // 重启/重扫一致。
    assert_eq!(report(&dir, &root).totals.requests, 3);
    std::fs::remove_dir_all(&dir).ok();
}

// ── §1 反例 2：非相邻重复用量 1100、2200、1100 ──────────────────────────
// 真值 4400 / 3；旧实现 3300 / 2（第三轮被第二轮+第一轮组合指纹吞掉……实际
// 是第三轮与第一轮同指纹）。

#[test]
fn nonadjacent_repeated_usage_counts_each_response() {
    let dir = temp_dir("nonadjacent");
    let root = dir.join("codex");
    let s = "sess-nonadj";
    let mut lines = vec![meta(s), ctx("2026-11-07T16:00:00.000Z")];
    let triple = [
        (USAGE_1100, "2026-11-07T16:00:01.000Z"),
        (
            Usage {
                input: 2000,
                cached: 1200,
                cache_write: 200,
                output: 200,
                reasoning: 80,
                total: 2200,
            },
            "2026-11-07T16:00:02.000Z",
        ),
        (USAGE_1100, "2026-11-07T16:00:03.000Z"),
    ];
    let mut cum = Usage::zero();
    for (n, (u, ts)) in triple.iter().enumerate() {
        cum = cum.plus(*u);
        let response = format!("resp_synthetic_{}", n + 1);
        let count_ts = offset_ms(ts, 2);
        lines.extend(turn(s, "turn-1", &response, *u, cum, ts, &count_ts));
    }
    write_rollout(&root, "rollout-nonadj", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 3, "第 1、3 轮用量相同但响应不同");
    assert_eq!(r.totals.tokens.total(), 4400);
    std::fs::remove_dir_all(&dir).ok();
}

// ── §1 反例 3：一轮工具循环，turn_id 相同、response_id 不同 ─────────────
// 真值 2200 / 2；旧实现按 turn/四桶弱指纹合并成 1100 / 1。

#[test]
fn tool_loop_two_calls_in_one_turn_count_twice() {
    let dir = temp_dir("tool-loop");
    let root = dir.join("codex");
    let s = "sess-tool";
    let mut lines = vec![meta(s), ctx("2026-11-07T16:00:00.000Z")];
    lines.extend(turn(
        s,
        "turn-same",
        "resp_synthetic_tool_a",
        USAGE_1100,
        USAGE_1100,
        "2026-11-07T16:00:01.000Z",
        "2026-11-07T16:00:01.003Z",
    ));
    lines.extend(turn(
        s,
        "turn-same",
        "resp_synthetic_tool_b",
        USAGE_1100,
        Usage {
            input: 2000,
            cached: 1200,
            cache_write: 200,
            output: 200,
            reasoning: 80,
            total: 2200,
        },
        "2026-11-07T16:00:02.000Z",
        "2026-11-07T16:00:02.002Z",
    ));
    write_rollout(&root, "rollout-tool", &lines);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "一轮工具循环的两次模型响应是两个请求，不能按 turn_id 去重"
    );
    assert_eq!(r.totals.tokens.total(), 2200);
    std::fs::remove_dir_all(&dir).ok();
}

// ── §1 反例 4：A=1100、B=2200，回退 B 后新请求 C=1100 ───────────────────
// 真值 4400 / 3。回退生成新 rollout 段文件（线程不变），C 与 A 四桶相同、
// response_id 不同。

#[test]
fn new_request_after_revert_counts_again() {
    let dir = temp_dir("revert");
    let root = dir.join("codex");
    let s = "sess-revert";
    // 主 rollout：A、B 两轮（撤回的 B 用量仍在原始文件里，真实行为）。
    let mut lines = vec![meta(s), ctx("2026-11-07T16:00:00.000Z")];
    lines.extend(turn(
        s,
        "turn-a",
        "resp_synthetic_1",
        USAGE_1100,
        USAGE_1100,
        "2026-11-07T16:00:01.000Z",
        "2026-11-07T16:00:01.001Z",
    ));
    let b = Usage {
        input: 2000,
        cached: 1200,
        cache_write: 200,
        output: 200,
        reasoning: 80,
        total: 2200,
    };
    lines.extend(turn(
        s,
        "turn-b",
        "resp_synthetic_2",
        b,
        Usage {
            input: 3000,
            cached: 1800,
            cache_write: 300,
            output: 300,
            reasoning: 120,
            total: 3300,
        },
        "2026-11-07T16:00:02.000Z",
        "2026-11-07T16:00:02.003Z",
    ));
    write_rollout(&root, "rollout-revert-main", &lines);
    // 回退后的新段：同线程新 session_meta，新响应 C（与 A 同用量）。
    let c = vec![
        serde_json::json!({
            "timestamp": "2026-11-07T16:00:03.000Z",
            "type": "session_meta",
            "payload": {
                "id": s,
                "session_id": s,
                "cwd": PROJECT,
            }
        }),
        ctx("2026-11-07T16:00:03.000Z"),
        Record {
            ts: "2026-11-07T16:00:04.000Z",
            session: s,
            turn: "turn-c",
            response: "resp_synthetic_3",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:04.000Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-revert-segment", &c);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 3, "A、B 与回退后的 C 各计一次");
    assert_eq!(r.totals.tokens.total(), 4400);
    // 重扫/重启一致。
    assert_eq!(report(&dir, &root).totals.tokens.total(), 4400);
    std::fs::remove_dir_all(&dir).ok();
}

// ── §1 反例 5：legacy fork 复制 A 后新增 B（同用量） ────────────────────
// 真值 2200 / 2；旧实现把子文件的新请求归到父会话并与 A 弱合并 → 1100 / 1。

#[test]
fn legacy_fork_copied_parent_counts_once_child_counts_again() {
    let dir = temp_dir("legacy-fork");
    let root = dir.join("codex");
    let parent = "sess-fork-parent";
    let child = "sess-fork-child";
    // 父 rollout：A 一轮。
    let mut lines = vec![meta(parent), ctx("2026-11-07T16:00:00.000Z")];
    lines.extend(turn(
        parent,
        "turn-a",
        "resp_synthetic_1",
        USAGE_1100,
        USAGE_1100,
        "2026-11-07T16:00:01.000Z",
        "2026-11-07T16:00:01.002Z",
    ));
    write_rollout(&root, "rollout-fork-parent", &lines);
    // legacy fork 子文件：文件头 session_meta(子) → 复制历史的 session_meta(父)
    // → 复制的父请求 A（thread=父）→ 子新请求 B（thread=子，与 A 同用量）。
    let child_lines = vec![
        serde_json::json!({
            "timestamp": "2026-11-07T16:00:02.000Z",
            "type": "session_meta",
            "payload": {"id": child, "session_id": child, "cwd": PROJECT}
        }),
        serde_json::json!({
            "timestamp": "2026-11-07T16:00:00.000Z",
            "type": "session_meta",
            "payload": {"id": parent, "session_id": parent, "cwd": PROJECT}
        }),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: parent,
            turn: "turn-a",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:01.002Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
        ctx("2026-11-07T16:00:02.000Z"),
        Record {
            ts: "2026-11-07T16:00:03.000Z",
            session: child,
            turn: "turn-b",
            response: "resp_synthetic_2",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:03.003Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-fork-child", &child_lines);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "复制的父请求 A 只计一次，子线程新请求 B 照常计入"
    );
    assert_eq!(r.totals.tokens.total(), 2200);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 对照：同总量但四桶分配不同（修复前后都必须是 3 请求） ────────────────

#[test]
fn equal_total_different_buckets_keeps_three_requests() {
    let dir = temp_dir("equal-total");
    let root = dir.join("codex");
    let s = "sess-buckets";
    let mut lines = vec![meta(s), ctx("2026-11-07T16:00:00.000Z")];
    let variants = [
        // raw 1000 / out 100：in 300, out 100, cw 100, cr 600，总 1100
        Usage {
            input: 1000,
            cached: 600,
            cache_write: 100,
            output: 100,
            reasoning: 40,
            total: 1100,
        },
        // raw 900 / out 200：in 300, out 200, cw 100, cr 500，总 1100
        Usage {
            input: 900,
            cached: 500,
            cache_write: 100,
            output: 200,
            reasoning: 40,
            total: 1100,
        },
        // raw 800 / out 300：in 300, out 300, cw 100, cr 400，总 1100
        Usage {
            input: 800,
            cached: 400,
            cache_write: 100,
            output: 300,
            reasoning: 40,
            total: 1100,
        },
    ];
    for (n, u) in variants.iter().enumerate() {
        let response = format!("resp_synthetic_{n}");
        lines.extend(turn(
            s,
            "turn-1",
            &response,
            *u,
            *u,
            "2026-11-07T16:00:01.000Z",
            "2026-11-07T16:00:01.001Z",
        ));
    }
    write_rollout(&root, "rollout-buckets", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 3);
    assert_eq!(r.totals.tokens.total(), 3300);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 对照：reasoning 子项不同但四桶相同（旧实现的第 5 类反例） ────────────

#[test]
fn reasoning_difference_with_equal_buckets_counts_both() {
    let dir = temp_dir("reasoning");
    let root = dir.join("codex");
    let s = "sess-reasoning";
    let mut lines = vec![meta(s), ctx("2026-11-07T16:00:00.000Z")];
    // 两个响应四桶完全相同（raw 1000/600/100/100 → 300/100/100/600），仅
    // reasoning 子项 40 vs 80；TS 四桶口径不含 reasoning，两响应都应计入。
    lines.extend(turn(
        s,
        "turn-1",
        "resp_synthetic_r1",
        USAGE_1100,
        USAGE_1100,
        "2026-11-07T16:00:01.000Z",
        "2026-11-07T16:00:01.002Z",
    ));
    lines.extend(turn(
        s,
        "turn-1",
        "resp_synthetic_r2",
        Usage {
            reasoning: 80,
            ..USAGE_1100
        },
        Usage {
            input: 2000,
            cached: 1200,
            cache_write: 200,
            output: 200,
            reasoning: 120,
            total: 2200,
        },
        "2026-11-07T16:00:02.000Z",
        "2026-11-07T16:00:02.001Z",
    ));
    write_rollout(&root, "rollout-reasoning", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 2, "reasoning 子项变化不影响四桶口径");
    assert_eq!(r.totals.tokens.total(), 2200);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 同一响应的两类记录与重复副本 ────────────────────────────────────────

#[test]
fn record_and_token_count_are_one_observation() {
    let dir = temp_dir("pair");
    let root = dir.join("codex");
    let s = "sess-pair";
    // record + token_count 是同一响应的两种观察；合计只产一条事实。
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:01.001Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-pair", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "record 与 token_count 合计一条事实");
    assert_eq!(r.totals.tokens.total(), 1100);
    assert_eq!(r.totals.tokens.input, 300);
    assert_eq!(r.totals.tokens.cache_read, 600);
    assert_eq!(r.totals.tokens.cache_write, 100);
    assert_eq!(r.totals.tokens.output, 100);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn duplicated_response_copy_counts_once() {
    let dir = temp_dir("dup-copy");
    let root = dir.join("codex");
    let s = "sess-dup";
    // 同一会话的两个 rollout 文件包含同一响应的完整副本（复制历史场景），
    // 同一响应只计一次；之后的独立新响应照常计入。
    let base = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:01.002Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-dup-a", &base);
    // 副本文件：逐字节相同的记录序列（路径不同）。
    write_rollout(&root, "rollout-dup-b", &base);
    // 独立新响应（四桶相同、response_id 不同）。
    let extra = vec![
        meta(s),
        ctx("2026-11-07T16:00:02.000Z"),
        Record {
            ts: "2026-11-07T16:00:03.000Z",
            session: s,
            turn: "turn-2",
            response: "resp_synthetic_2",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:03.002Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-dup-c", &extra);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "同一响应的副本只计一次，独立新响应照常计入"
    );
    assert_eq!(r.totals.tokens.total(), 2200);
    std::fs::remove_dir_all(&dir).ok();
}

impl Usage {
    /// 累计起点（零值）。
    fn zero() -> Usage {
        Usage {
            input: 0,
            cached: 0,
            cache_write: 0,
            output: 0,
            reasoning: 0,
            total: 0,
        }
    }

    fn plus(self, other: Usage) -> Usage {
        Usage {
            input: self.input + other.input,
            cached: self.cached + other.cached,
            cache_write: self.cache_write + other.cache_write,
            output: self.output + other.output,
            reasoning: self.reasoning + other.reasoning,
            total: self.total + other.total,
        }
    }
}

/// 时间戳加毫秒（fixture 专用：`...T16:00:01.000Z` 形态）。
fn offset_ms(ts: &str, ms: i64) -> String {
    let mut t = ts.parse::<jiff::Timestamp>().unwrap();
    t += jiff::Span::new().milliseconds(ms);
    t.to_string()
}

// ── 配对边界（真实管线，不得无条件吞掉下一条 token_count） ────────────────
//
// 旧实现的挂起配对规则（保留）：record 只在「下一条 token_count 的 last 四桶
// 全零」时单独入账；一致 → 用 token_count；非零不一致 → 只保留 token_count。
// 新实现改为 record 优先出数，但这些边界的行为约束仍须成立：
// 无关通知不打断配对、record-only / count-only 不补造或重复出数、连续响应
// 不互相覆盖、文件截断后追加时新响应照常入账。

fn session_skeleton(session: &str) -> Vec<serde_json::Value> {
    vec![meta(session), ctx("2026-11-07T16:00:00.000Z")]
}

/// 无关通知插在 record 与 token_count 之间：不得打断配对。
#[test]
fn unrelated_notification_between_record_and_count_does_not_break_pairing() {
    let dir = temp_dir("unrelated");
    let root = dir.join("codex");
    let s = "sess-unrelated";
    let mut lines = session_skeleton(s);
    lines.push(
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
    );
    lines.push(serde_json::json!({
        "timestamp": "2026-11-07T16:00:01.001Z",
        "type": "event_msg",
        "payload": {"type": "task_complete", "last_agent_message": "x"}
    }));
    lines.push(
        Count {
            ts: "2026-11-07T16:00:01.002Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    );
    write_rollout(&root, "rollout-unrelated", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "无关通知插队后仍只计一条事实");
    assert_eq!(r.totals.tokens.total(), 1100);
    std::fs::remove_dir_all(&dir).ok();
}

/// record-only：配对 token_count 缺失（真实 last 非零或不出现）时不得重复
/// 出数——同一响应只有一条事实。
#[test]
fn record_only_yields_one_fact_not_two() {
    let dir = temp_dir("record-only");
    let root = dir.join("codex");
    let s = "sess-record-only";
    // 场景 A：record 后跟非零且一致的 token_count（新日志常态）。
    let mut lines = session_skeleton(s);
    lines.push(
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
    );
    lines.push(
        Count {
            ts: "2026-11-07T16:00:01.002Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    );
    write_rollout(&root, "rollout-record-only-a", &lines);
    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "record + 一致 token_count = 一条事实");
    assert_eq!(r.totals.tokens.total(), 1100);
    std::fs::remove_dir_all(&dir).ok();
}

/// count-only（旧格式无 record）：保持既有行为，token_count 直接出数。
#[test]
fn count_only_keeps_legacy_behavior() {
    let dir = temp_dir("count-only");
    let root = dir.join("codex");
    let s = "sess-count-only";
    let mut lines = session_skeleton(s);
    lines.push(
        Count {
            ts: "2026-11-07T16:00:01.000Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    );
    write_rollout(&root, "rollout-count-only", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "旧格式 count-only 照常入账");
    assert_eq!(r.totals.tokens.total(), 1100);
    std::fs::remove_dir_all(&dir).ok();
}

/// 连续响应（上一条的 token_count 尚未出现时下一条 record 到达）：
/// 两条响应都保留，第一条不得被覆盖，第二条不得被吞。
#[test]
fn consecutive_records_do_not_overwrite_each_other() {
    let dir = temp_dir("consecutive");
    let root = dir.join("codex");
    let s = "sess-consecutive";
    let mut lines = session_skeleton(s);
    for (n, ts, cts) in [
        (
            "resp_synthetic_1",
            "2026-11-07T16:00:01.000Z",
            "2026-11-07T16:00:01.001Z",
        ),
        (
            "resp_synthetic_2",
            "2026-11-07T16:00:02.000Z",
            "2026-11-07T16:00:02.002Z",
        ),
    ] {
        lines.push(
            Record {
                ts,
                session: s,
                turn: "turn-1",
                response: n,
                usage: USAGE_1100,
            }
            .to_json(),
        );
        lines.push(
            Count {
                ts: cts,
                last: USAGE_1100,
                total: USAGE_1100,
            }
            .to_json(),
        );
    }
    write_rollout(&root, "rollout-consecutive", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 2, "连续响应各自一条事实");
    assert_eq!(r.totals.tokens.total(), 2200);
    std::fs::remove_dir_all(&dir).ok();
}

/// 文件截断后追加：重扫整文件时，截断前的响应与追加的新响应各计一条。
#[test]
fn truncated_file_appended_recounts_consistently() {
    let dir = temp_dir("truncate-append");
    let root = dir.join("codex");
    let s = "sess-truncate";
    // 第一轮：完整 A（record + count）。
    let mut lines = session_skeleton(s);
    lines.extend(turn(
        s,
        "turn-1",
        "resp_synthetic_1",
        USAGE_1100,
        USAGE_1100,
        "2026-11-07T16:00:01.000Z",
        "2026-11-07T16:00:01.002Z",
    ));
    let file = write_rollout(&root, "rollout-truncate", &lines);
    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1);
    assert_eq!(r.totals.tokens.total(), 1100);

    // 追加新响应 B（模拟 Codex 继续写文件）。
    lines.extend(turn(
        s,
        "turn-2",
        "resp_synthetic_2",
        USAGE_1100,
        Usage {
            input: 2000,
            cached: 1200,
            cache_write: 200,
            output: 200,
            reasoning: 80,
            total: 2200,
        },
        "2026-11-07T16:00:02.000Z",
        "2026-11-07T16:00:02.001Z",
    ));
    let body = lines
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&file, body + "\n").unwrap();

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 2, "追加后的新响应照常入账");
    assert_eq!(r.totals.tokens.total(), 2200);
    // 再次查询（重启视角）一致。
    assert_eq!(report(&dir, &root).totals.requests, 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// 强 record A 后出现**新 turn_context**，随后是独立的 count-only 请求 B、C：
/// 新 turn_context 终结 A 的配对窗口（其后不可能再出现 A 的配对 count——
/// 同一响应的观察在下一 turn 开始前就已落盘），B、C 是无强记录旧格式的
/// 独立请求，必须照常按 count 出数。
///
/// 复审反例（2026-11-08）：悬挂的 `last_strong_observation` 在遇到不同用量
/// count 时既不消费也不清理，turn_context 也不终结配对状态——B、C 被持续
/// 丢弃，实际 1100/1（应 6600/3）。
#[test]
fn dangling_strong_observation_does_not_swallow_followup_counts() {
    let dir = temp_dir("dangling-obs");
    let root = dir.join("codex");
    let s = "sess-dangling";
    let b = Usage {
        input: 2000,
        cached: 1200,
        cache_write: 200,
        output: 200,
        reasoning: 80,
        total: 2200,
    };
    let c = Usage {
        input: 3000,
        cached: 1800,
        cache_write: 300,
        output: 300,
        reasoning: 120,
        total: 3300,
    };
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        // A 的配对 count 缺席（截断/丢行等异常）；turn_context 之后的
        // count 与 A 无关。
        ctx("2026-11-07T16:00:02.000Z"),
        Count {
            ts: "2026-11-07T16:00:02.100Z",
            last: b,
            total: b,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:03.100Z",
            last: c,
            total: c,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-dangling", &lines);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 3,
        "A(强记录) + B/C(count-only) 各计一次；悬挂观察不得吞掉后续请求"
    );
    assert_eq!(r.totals.tokens.total(), 6600);
    // 重启视角一致。
    assert_eq!(report(&dir, &root).totals.requests, 3);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 同用量配对的不可区分边界（2026-11-09 复审更正）────────────────────────
//
// 形态：强 record A 已产出 → A 的配对 count 缺失 → 后续出现独立 count-only
// B，四桶与 A 完全相同。B 与"A 的迟到配对 count"在没有额外可靠身份时**不可
// 区分**：count 行没有请求标识，四桶一致也不构成身份证据（不同响应四桶相同
// 是常态，见 §1 反例）。
//
// 当前实现在**两层**抑制 B，风险形态是**漏计**（此前描述"照常出数、可能双
// 计"有误，实测为漏计）：
// 1. 解析层：无新 turn 边界时，同用量 count 被当作 A 的配对观察抑制；
// 2. 写库层：即使解析层出数（如已有新 turn 边界），A 的镜像来源键与同四桶
//    弱 count 的事件键在"旧出数行 = 该 count 行"时逐字节相同（旧出数行、旧
//    时间戳、四桶、空 record_id 全同），弱 count 经镜像 origin 命中 A 的
//    行，按 Codex 重播"更早胜出"被 Stale 丢弃——旧表示下两者本就是同一条
//    弱事实（旧库漏计的根源），新表示下 A 有强身份、B 是无身份独立请求，
//    但没有证据可拆开。
//
// 本组用例是**边界行为测试**：钉住当前实现的可判定行为，不是"正确计量验收"
// ——漏计形态在无新证据时无法修复，不引入模糊时间窗口、不凭猜测改算法
//（"全部保留"会把漏计风险换成正常配对双计）。修复只有一条路：上游提供可
// 配对的响应标识（如 count 行携带 response_id）后再拆分。

/// 无边界标记的同用量 count-only B：被当作 A 的配对观察抑制（漏计形态，
/// 边界行为测试，钉住当前实现）。
#[test]
fn same_usage_count_only_after_strong_record_without_boundary_is_suppressed() {
    let dir = temp_dir("same-usage-noboundary");
    let root = dir.join("codex");
    let s = "sess-same-noboundary";
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        // A 的配对 count 缺席；无 turn_context；随后独立的 count-only B
        //（四桶与 A 完全相同）。
        Count {
            ts: "2026-11-07T16:00:02.000Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-same-noboundary", &lines);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 1,
        "边界行为：同用量 count-only B 与 A 的迟到配对不可区分，\
         按配对观察抑制（漏计形态，无身份证据时不猜测）"
    );
    assert_eq!(r.totals.tokens.total(), 1100);
    std::fs::remove_dir_all(&dir).ok();
}

/// 边界行为（写库层）：即使解析层把同用量 count-only B 出数（新 turn 边界
/// 已终结 A 的配对观察），B 的事件键与 A 的镜像来源键逐字节相同（同一旧出
/// 数行、同一旧指纹），写库经镜像 origin 命中 A 的行后按"更早胜出"被
/// Stale 丢弃——仍漏计。旧表示下两者本就是同一条弱事实；无证据不拆分。
#[test]
fn same_usage_count_only_absorbed_by_mirror_origin_despite_boundary() {
    let dir = temp_dir("same-usage-mirror");
    let root = dir.join("codex");
    let s = "sess-same-mirror";
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        // A 的配对 count 缺席；新 turn 边界后出现同四桶 count-only B，
        // ts 恰为旧解析器会记账的那一行（与 A 的镜像键同 ts）。
        ctx("2026-11-07T16:00:02.000Z"),
        Count {
            ts: "2026-11-07T16:00:02.100Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-same-mirror", &lines);

    // 解析层确实出 2 条（turn_context 已终结配对观察）。
    let parse = CodexSource::new(root.clone())
        .parse_file(&root.join("sessions").join("rollout-same-mirror.jsonl"));
    assert_eq!(
        parse.events.len(),
        2,
        "解析层：新 turn 边界终结配对观察，A 与 B 各出一事件"
    );

    // 写库层：B 经镜像 origin 命中 A 的行被吸收（边界行为，漏计形态）。
    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 1,
        "边界行为：同四桶弱 count 的事件键与 A 的镜像键逐字节相同，\
         写库按旧表示视为同一事实吸收（漏计形态，无身份证据不拆分）"
    );
    assert_eq!(r.totals.tokens.total(), 1100);
    // 重启视角一致。
    assert_eq!(report(&dir, &root).totals.requests, 1);
    std::fs::remove_dir_all(&dir).ok();
}

/// 对照 1：明确的**新 turn 边界** + **不同四桶**的 count-only 照常计入
///（可区分：镜像键内容指纹随四桶不同而不同，不与 A 的镜像键碰撞）。
#[test]
fn different_usage_count_only_after_new_turn_boundary_counts() {
    let dir = temp_dir("diff-usage-turn");
    let root = dir.join("codex");
    let s = "sess-diff-turn";
    let b = Usage {
        input: 2000,
        cached: 1200,
        cache_write: 200,
        output: 200,
        reasoning: 80,
        total: 2200,
    };
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        // A 的配对 count 缺席；新 turn 开始 → 配对窗口终结；B 四桶与 A
        // 不同 → B 的事件键与 A 的镜像键不碰撞，B 独立入账。
        ctx("2026-11-07T16:00:02.000Z"),
        Count {
            ts: "2026-11-07T16:00:02.100Z",
            last: b,
            total: b,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-diff-turn", &lines);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "新 turn 边界后的不同四桶 count-only 照常计入"
    );
    assert_eq!(r.totals.tokens.total(), 3300);
    std::fs::remove_dir_all(&dir).ok();
}

/// 对照 2：正常的 record/count 配对（四桶一致、无边界插入）仍只计一次。
#[test]
fn normal_same_usage_pair_still_counts_once() {
    let dir = temp_dir("same-usage-pair");
    let root = dir.join("codex");
    let s = "sess-same-pair";
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:01.001Z",
            last: USAGE_1100,
            total: USAGE_1100,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-same-pair", &lines);

    let r = report(&dir, &root);
    assert_eq!(r.totals.requests, 1, "正常配对仍只计一次");
    assert_eq!(r.totals.tokens.total(), 1100);
    std::fs::remove_dir_all(&dir).ok();
}

/// 强 record A 后，**另一响应 B 的 record+count 完整对**照常入账：B 的 count
/// 与 A 的强观察用量不同，不得吞掉 B 的记录——但 B 自己的 record 已经出数，
/// B 的配对 count 也不能二次出数（B 的 record 在 count 之前重置了观察）。
#[test]
fn new_response_pair_after_strong_record_without_count_counts_once() {
    let dir = temp_dir("obs-reset");
    let root = dir.join("codex");
    let s = "sess-obs-reset";
    let b = Usage {
        input: 2000,
        cached: 1200,
        cache_write: 200,
        output: 200,
        reasoning: 80,
        total: 2200,
    };
    let lines = vec![
        meta(s),
        ctx("2026-11-07T16:00:00.000Z"),
        Record {
            ts: "2026-11-07T16:00:01.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_1",
            usage: USAGE_1100,
        }
        .to_json(),
        // A 的配对 count 缺席。
        Record {
            ts: "2026-11-07T16:00:02.000Z",
            session: s,
            turn: "turn-1",
            response: "resp_synthetic_2",
            usage: b,
        }
        .to_json(),
        Count {
            ts: "2026-11-07T16:00:02.001Z",
            last: b,
            total: b,
        }
        .to_json(),
    ];
    write_rollout(&root, "rollout-obs-reset", &lines);

    let r = report(&dir, &root);
    assert_eq!(
        r.totals.requests, 2,
        "A 与 B 各计一次，B 的配对 count 不重复"
    );
    assert_eq!(r.totals.tokens.total(), 3300);
    std::fs::remove_dir_all(&dir).ok();
}
