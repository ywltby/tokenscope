//! 计划 B1（test_token_bucket_contract）：两个适配器的 token 桶契约——
//! 展示四桶（非缓存输入/输出/缓存写/缓存读）互斥且守恒：
//! - Codex：展示总量必须等于日志 raw total（raw input 含 cached 与 cache_write）；
//! - Claude：四桶一一对应 Anthropic usage 的四个独立字段，不得合并或挪列。
//! 期望值全部独立手算；适用日志版本见 docs/stats-semantics.md。

use std::path::PathBuf;

use tokenscope::source::Source;
use tokenscope::source::claude::ClaudeSource;
use tokenscope::source::codex::CodexSource;

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tokenscope-bucket-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn test_codex_display_total_conserved() {
    let dir = tmp_dir("codex");
    let file = dir.join("rollout-x.jsonl");
    std::fs::write(
        &file,
        format!(
            "{}\n{}\n{}\n",
            r#"{"timestamp":"2026-07-17T15:00:00.000Z","type":"session_meta","payload":{"id":"s1","session_id":"s1","cwd":"C:/w/alpha"}}"#,
            r#"{"timestamp":"2026-07-17T15:01:00.000Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","cwd":"C:/w/alpha"}}"#,
            // raw input 15000 = 0 未缓存 + 12000 读 + 3000 写；total = 15000+100
            r#"{"timestamp":"2026-07-17T15:02:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":15000,"output_tokens":100,"cached_input_tokens":12000,"cache_write_input_tokens":3000,"reasoning_output_tokens":60,"total_tokens":15100}}}}"#
        ),
    )
    .unwrap();
    let col = CodexSource::new(&dir).collect().unwrap();
    assert_eq!(col.events.len(), 1);
    let e = &col.events[0];
    // 手算：0 + 12000 + 3000 + 100 = 15100 == raw total（修复前为 15100+3000）。
    assert_eq!(
        e.input_tokens + e.output_tokens + e.cache_write_tokens + e.cache_read_tokens,
        15100
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn test_claude_buckets_map_one_to_one() {
    let dir = tmp_dir("claude");
    let file = dir.join("sess-x.jsonl");
    // 四字段各自独立：input 800 / cw 5000 / cr 10000 / out 200。
    std::fs::write(
        &file,
        r#"{"type":"assistant","timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s1","message":{"id":"m1","model":"claude-sonnet-4-5","usage":{"input_tokens":800,"output_tokens":200,"cache_creation_input_tokens":5000,"cache_read_input_tokens":10000}}}"#,
    )
    .unwrap();
    let col = ClaudeSource::new(&dir).collect().unwrap();
    assert_eq!(col.events.len(), 1);
    let e = &col.events[0];
    // 一一映射，不合并、不挪列；展示总量 = 四字段之和。
    assert_eq!(e.input_tokens, 800);
    assert_eq!(e.output_tokens, 200);
    assert_eq!(e.cache_write_tokens, 5000);
    assert_eq!(e.cache_read_tokens, 10000);
    assert_eq!(
        e.input_tokens + e.output_tokens + e.cache_write_tokens + e.cache_read_tokens,
        16000
    );
    std::fs::remove_dir_all(&dir).ok();
}
