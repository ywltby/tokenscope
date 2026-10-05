//! Codex 适配器：扫描 `~/.codex/sessions/**/*.jsonl`（rollout 文件，append-only）。
//!
//! 口径（依据本机 47 文件全量实测 + 上游权威语义，见 docs/stats-semantics.md）：
//! - 只取 `event_msg`/`token_count` 的 `info.last_token_usage`；`token_usage_record`
//!   是累计回显，一律忽略并计数（双计防线）；
//! - 语义（R02 已查证）：OpenAI Responses 的 `input_tokens` 是总量桶，
//!   `input_tokens_details.cached_tokens` 与 `cache_write_tokens` 均为其子集
//!   （官方文档示例 15000 = 12000 cached + 3000 cache_write + 0 未缓存）；
//!   Codex CLI 0.145.0 起才把 cache_write 透传为 `cache_write_input_tokens`，
//!   订阅流服务端恒返 0。归一化：`input = raw - cached - cache_write`、
//!   `cache_read = cached`、`cache_write = cache_write`，展示总量守恒
//!   `input+output+cw+cr = raw_total`；
//! - `cached + cache_write > input` 属未知字段组合（如未经查证的第三方
//!   provider 映射），按坏行计数不入账，不猜测语义；
//! - 零分量占位行（只升 total）跳过计数；分量非零但 total 不符按坏行计数；
//! - 模型按时间序最近的 `turn_context` 归属，遇 `session_meta` 重置；
//! - 同请求重发的去重自 M4 起上移到全局 dedupe 步骤（按 `(session, 用量五元组)`
//!   保首条），本层原样产出事件（record_id 为空）。

use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::{CollectStats, FileParse, Source, read_text, walk_jsonl};
use crate::model::{AgentKind, UsageEvent};

pub struct CodexSource {
    root: PathBuf,
}

impl CodexSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(home.join(".codex").join("sessions"))
    }
}

#[derive(Default, Deserialize)]
struct RolloutLine {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    payload: Option<RolloutPayload>,
}

/// 宽松结构：一次声明覆盖 session_meta / turn_context / event_msg 三类 payload。
#[derive(Default, Deserialize)]
struct RolloutPayload {
    #[serde(rename = "type", default)]
    kind: String,
    // session_meta
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    cwd: Option<String>,
    // turn_context
    #[serde(default)]
    model: Option<String>,
    // event_msg/token_count
    #[serde(default)]
    info: Option<TokenInfo>,
}

#[derive(Default, Deserialize)]
struct TokenInfo {
    #[serde(default)]
    last_token_usage: Option<LastUsage>,
}

#[derive(Default, Deserialize)]
struct LastUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cached_input_tokens: u64,
    #[serde(default)]
    cache_write_input_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}

/// 文件内解析状态：session 边界与模型归属。
#[derive(Default)]
struct ScanState {
    session_id: String,
    model: Option<String>,
    cwd: Option<String>,
}

impl Source for CodexSource {
    fn agent(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn discover(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk_jsonl(&self.root, &mut out);
        out.sort();
        out
    }

    fn parse_file(&self, path: &Path) -> FileParse {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        match read_text(path) {
            Ok(text) => ingest_text(&text, &mut stats, &mut events),
            // 读取失败：计一个坏行，不中断整体扫描。
            Err(_) => stats.bad_lines += 1,
        }
        stats.events = events.len() as u64;
        FileParse { stats, events }
    }
}

fn ingest_text(text: &str, stats: &mut CollectStats, events: &mut Vec<UsageEvent>) {
    let mut state = ScanState::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(line, &mut state, stats, events);
    }
}

fn ingest_line(
    line: &str,
    state: &mut ScanState,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
) {
    let rec: RolloutLine = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };
    let Some(payload) = rec.payload.as_ref() else {
        return;
    };
    match rec.kind.as_str() {
        "session_meta" => {
            // session 边界：重置模型归属；项目随 cwd 更新。
            state.session_id = payload
                .session_id
                .clone()
                .or_else(|| payload.id.clone())
                .unwrap_or_default();
            state.model = None;
            if payload.cwd.is_some() {
                state.cwd = payload.cwd.clone();
            }
        }
        "turn_context" => {
            if payload.model.is_some() {
                state.model = payload.model.clone();
            }
            if payload.cwd.is_some() {
                state.cwd = payload.cwd.clone();
            }
        }
        "token_usage_record" => stats.ignored_token_usage_record += 1,
        "event_msg" if payload.kind == "token_count" => {
            ingest_token_count(rec.timestamp.as_deref(), payload, state, stats, events);
        }
        _ => {}
    }
}

fn ingest_token_count(
    ts_str: Option<&str>,
    payload: &RolloutPayload,
    state: &ScanState,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
) {
    let Some(ts_str) = ts_str else {
        stats.bad_lines += 1;
        return;
    };
    let ts: Timestamp = match ts_str.parse() {
        Ok(t) => t,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };
    let Some(u) = payload
        .info
        .as_ref()
        .and_then(|i| i.last_token_usage.as_ref())
    else {
        stats.bad_lines += 1;
        return;
    };
    let (input, output, cached, cache_write, total) = (
        u.input_tokens,
        u.output_tokens,
        u.cached_input_tokens,
        u.cache_write_input_tokens,
        u.total_tokens,
    );
    // 零分量占位行：只升 total 不升分量（本机实测 278 条），跳过计数。
    if input + output + cached + cache_write == 0 {
        stats.skipped_zero_usage += 1;
        return;
    }
    // 口径校验（R02）：total = input + output；cached 与 cache_write 均是
    // input 的子集。cached + cache_write > input 属未查证的字段组合，
    // 不猜测语义，按坏行计数暴露。
    if total != input + output || cached + cache_write > input {
        stats.bad_lines += 1;
        return;
    }
    let Some(model) = state.model.as_deref() else {
        stats.skipped_no_model += 1;
        return;
    };
    if state.session_id.is_empty() {
        stats.bad_lines += 1;
        return;
    }
    let project = state
        .cwd
        .as_deref()
        .and_then(basename)
        .unwrap_or_else(|| "(未知)".to_string());
    events.push(UsageEvent {
        ts,
        agent: AgentKind::Codex,
        model: model.to_string(),
        session_id: state.session_id.clone(),
        project,
        record_id: String::new(),
        input_tokens: input - cached - cache_write,
        output_tokens: output,
        cache_write_tokens: cache_write,
        cache_read_tokens: cached,
    });
}

fn basename(path: &str) -> Option<String> {
    Some(
        Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Collection;

    fn collect_lines(lines: &[String]) -> Collection {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        ingest_text(&lines.join("\n"), &mut stats, &mut events);
        stats.events = events.len() as u64;
        Collection {
            agent: AgentKind::Codex,
            events,
            stats,
            warnings: Vec::new(),
        }
    }

    fn meta(session: &str, cwd: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-07-17T15:00:00.000Z","type":"session_meta","payload":{{"id":"{session}","session_id":"{session}","cwd":"{cwd}"}}}}"#
        )
    }

    fn ctx(model: &str, cwd: &str) -> String {
        format!(
            r#"{{"timestamp":"2026-07-17T15:01:00.000Z","type":"turn_context","payload":{{"turn_id":"t","model":"{model}","cwd":"{cwd}"}}}}"#
        )
    }

    fn tc(ts: &str, input: u64, output: u64, cached: u64, cw: u64, total: u64) -> String {
        format!(
            r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"output_tokens":{output},"cached_input_tokens":{cached},"cache_write_input_tokens":{cw},"reasoning_output_tokens":0,"total_tokens":{total}}}}}}}}}"#
        )
    }

    #[test]
    fn test_codex_parse_typical() {
        let col = collect_lines(&[
            meta("sess-a", "C:/work/alpha"),
            ctx("gpt-5.6-sol", "C:/work/alpha"),
            tc("2026-07-17T15:59:00.000Z", 1000, 100, 200, 50, 1100),
        ]);
        assert_eq!(col.events.len(), 1);
        let e = &col.events[0];
        // 归一化（R02）：raw input 含 cached 与 cache_write，两者全部剔出；
        // 展示总量守恒：750+100+50+200 = 1100 = raw total。
        assert_eq!(e.input_tokens, 750);
        assert_eq!(e.output_tokens, 100);
        assert_eq!(e.cache_write_tokens, 50);
        assert_eq!(e.cache_read_tokens, 200);
        assert_eq!(
            e.input_tokens + e.output_tokens + e.cache_write_tokens + e.cache_read_tokens,
            1100
        );
        assert_eq!(e.model, "gpt-5.6-sol");
        assert_eq!(e.project, "alpha");
        assert_eq!(e.session_id, "sess-a");
        assert_eq!(e.record_id, "");
        assert_eq!(col.stats.bad_lines, 0);
    }

    #[test]
    fn test_codex_cache_write_semantics() {
        // R02（B1）：官方文档示例——input_tokens 是总量桶，cached 与
        // cache_write 均为其子集；展示总量必须等于 raw total（修复前
        // cache_write 被重复计一次，展示总量多出 cw）。
        let col = collect_lines(&[
            meta("sess-a", "C:/work/alpha"),
            ctx("gpt-5.6-sol", "C:/work/alpha"),
            // 全量命中写缓存：raw input 15000 = cached 12000 + cw 3000 + 0
            tc("2026-07-17T15:59:00.000Z", 15000, 100, 12000, 3000, 15100),
        ]);
        assert_eq!(col.events.len(), 1);
        let e = &col.events[0];
        assert_eq!(e.input_tokens, 0);
        assert_eq!(e.cache_read_tokens, 12000);
        assert_eq!(e.cache_write_tokens, 3000);
        assert_eq!(e.output_tokens, 100);
        assert_eq!(
            e.input_tokens + e.cache_read_tokens + e.cache_write_tokens + e.output_tokens,
            15100,
            "展示总量 == raw total（互斥桶守恒）"
        );
        // 旧版日志（<0.145.0）无 cache_write 字段：serde 缺省 0，退化为
        // input = raw - cached，与历史行为一致。
        let old = collect_lines(&[
            meta("sess-b", "C:/work/alpha"),
            ctx("gpt-5.6-sol", "C:/work/alpha"),
            tc("2026-07-17T16:00:00.000Z", 1000, 100, 200, 0, 1100),
        ]);
        assert_eq!(old.events[0].input_tokens, 800);
        // cached + cache_write > input：未知字段组合，坏行计数不入账。
        let bad = collect_lines(&[
            meta("sess-c", "C:/work/alpha"),
            ctx("gpt-5.6-sol", "C:/work/alpha"),
            tc("2026-07-17T16:01:00.000Z", 100, 100, 90, 50, 200),
        ]);
        assert!(bad.events.is_empty());
        assert_eq!(bad.stats.bad_lines, 1);
    }

    #[test]
    fn test_codex_model_attribution_and_session_boundary() {
        let col = collect_lines(&[
            meta("sess-a", "C:/work/alpha"),
            ctx("gpt-5.6-sol", "C:/work/alpha"),
            tc("2026-07-17T15:59:00.000Z", 10, 5, 0, 0, 15),
            // 新 session：模型重置，此行无模型可归属
            meta("sess-b", "C:/work/beta"),
            tc("2026-07-17T16:00:00.000Z", 9, 9, 0, 0, 18),
            ctx("gpt-5.5", "C:/work/beta"),
            tc("2026-07-17T16:01:00.000Z", 7, 3, 2, 0, 10),
            // 模型切换
            ctx("gpt-5.4", "C:/work/beta"),
            tc("2026-07-17T16:02:00.000Z", 6, 4, 1, 0, 10),
        ]);
        assert_eq!(col.events.len(), 3);
        assert_eq!(col.stats.skipped_no_model, 1);
        let models: Vec<&str> = col.events.iter().map(|e| e.model.as_str()).collect();
        assert_eq!(models, ["gpt-5.6-sol", "gpt-5.5", "gpt-5.4"]);
        let projects: Vec<&str> = col.events.iter().map(|e| e.project.as_str()).collect();
        assert_eq!(projects, ["alpha", "beta", "beta"]);
        let sessions: Vec<&str> = col.events.iter().map(|e| e.session_id.as_str()).collect();
        assert_eq!(sessions, ["sess-a", "sess-b", "sess-b"]);
    }

    #[test]
    fn test_codex_zero_usage_skipped() {
        let col = collect_lines(&[
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            tc("2026-07-17T15:59:00.000Z", 0, 0, 0, 0, 19457),
        ]);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.skipped_zero_usage, 1);
    }

    #[test]
    fn test_codex_badline() {
        let col = collect_lines(&[
            "not json".to_string(),
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            // total 与 input+output 不符
            tc("2026-07-17T15:59:00.000Z", 5, 5, 0, 0, 11),
            // cached > input
            tc("2026-07-17T15:59:01.000Z", 5, 5, 9, 0, 10),
            // 缺时间戳
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#.to_string(),
            // 缺 info
            r#"{"timestamp":"2026-07-17T15:59:02.000Z","type":"event_msg","payload":{"type":"token_count"}}"#.to_string(),
            // 时间戳非法
            tc("yesterday", 1, 1, 0, 0, 2),
        ]);
        assert_eq!(col.events.len(), 0);
        assert_eq!(col.stats.bad_lines, 6);
        assert_eq!(col.stats.lines_seen, 8);
    }

    #[test]
    fn test_codex_token_usage_record_ignored() {
        let col = collect_lines(&[
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            r#"{"timestamp":"2026-07-17T15:59:00.000Z","type":"token_usage_record","payload":{"session_id":"s","thread_id":"t","turn_id":"t","root_turn_id":"t","response_id":"r","usage":{"input_tokens":9,"output_tokens":9,"total_tokens":18},"turn_token_usage":{},"thread_token_usage":{}}}"#.to_string(),
        ]);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.ignored_token_usage_record, 1);
    }

    #[test]
    fn test_codex_no_dedupe_here() {
        // M4 起同请求重发由全局 dedupe 处理，source 层原样产出。
        let col = collect_lines(&[
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            tc("2026-07-17T15:59:00.000Z", 1000, 100, 200, 50, 1100),
            tc("2026-07-17T15:59:05.000Z", 1000, 100, 200, 50, 1100),
        ]);
        assert_eq!(col.events.len(), 2);
        assert_eq!(col.stats.duplicates_dropped, 0);
    }

    fn fixture(p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex")
            .join(p)
    }

    #[test]
    fn test_codex_discovery_finds_nested_files() {
        let src = CodexSource::new(fixture("basic"));
        assert_eq!(src.discover().len(), 2);
        let col = src.collect().unwrap();
        assert!(col.warnings.is_empty());
    }

    #[test]
    fn test_codex_missing_dir_warns() {
        let col = CodexSource::new(fixture("no-such-dir")).collect().unwrap();
        assert!(col.events.is_empty());
        assert_eq!(col.warnings.len(), 1);
        assert_eq!(col.stats.files_scanned, 0);
    }
}
