//! Codex 适配器：扫描 `~/.codex/sessions/**/*.jsonl`（rollout 文件，append-only）。
//!
//! 口径（依据本机 47 文件全量实测，见 docs/plans 的 M2 计划）：
//! - 只取 `event_msg`/`token_count` 的 `info.last_token_usage`；`token_usage_record`
//!   是累计回显，一律忽略并计数（双计防线）；
//! - 语义：`total = input + output` 且 `cached ⊆ input`；归一化
//!   `input = input - cached`、`cache_read = cached`、`cache_write = cache_write`；
//! - 零分量占位行（只升 total）跳过计数；分量非零但 total 不符按坏行计数；
//! - 模型按时间序最近的 `turn_context` 归属，遇 `session_meta` 重置；无模型可归属
//!   计入 skipped_no_model，不虚构模型名；
//! - rollout 无流式重写（实测 0 重复），不引入去重。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::{CollectStats, Collection, Source, is_jsonl};
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

    fn collect(&self) -> Result<Collection> {
        let mut stats = CollectStats::default();
        let mut warnings = Vec::new();
        let mut events: Vec<UsageEvent> = Vec::new();
        let mut dedupe: HashSet<DedupeKey> = HashSet::new();

        // 不变量：目录缺失是常态（机器上没装该 agent），警告后以空结果正常返回。
        if !self.root.is_dir() {
            warnings.push(format!("Codex 目录不存在：{}", self.root.display()));
            return Ok(Collection {
                agent: self.agent(),
                events,
                stats,
                warnings,
            });
        }

        for path in discover(&self.root) {
            stats.files_scanned += 1;
            match fs::read(&path) {
                // lossy 容错：个别非法字节不应让整份文件失败，垃圾行由坏行计数接住。
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    ingest_text(&text, &mut stats, &mut events, &mut dedupe);
                }
                Err(e) => warnings.push(format!("读取失败 {}: {e}", path.display())),
            }
        }
        stats.events = events.len() as u64;
        Ok(Collection {
            agent: self.agent(),
            events,
            stats,
            warnings,
        })
    }
}

/// 同请求重发去重键：`(session, input, output, cached, cache_write, total)`。
/// 实测同一请求的 last_token_usage 会被原样重发（时间戳不同、数值逐字相同）。
type DedupeKey = (String, u64, u64, u64, u64, u64);

/// 递归收集 root 下全部 jsonl（sessions/YYYY/MM/DD/rollout-*.jsonl），按路径排序保证稳定。
fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            walk(&child, out);
        } else if is_jsonl(&child) {
            out.push(child);
        }
    }
}

fn ingest_text(
    text: &str,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
    dedupe: &mut HashSet<DedupeKey>,
) {
    let mut state = ScanState::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(line, &mut state, stats, events, dedupe);
    }
}

fn ingest_line(
    line: &str,
    state: &mut ScanState,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
    dedupe: &mut HashSet<DedupeKey>,
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
            ingest_token_count(
                rec.timestamp.as_deref(),
                payload,
                state,
                stats,
                events,
                dedupe,
            );
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
    dedupe: &mut HashSet<DedupeKey>,
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
    // 口径校验：total = input + output；cached ⊆ input。不符不入账。
    if total != input + output || cached > input {
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
    // 同请求重发：数值逐字相同，保留首条（请求完成时刻），丢弃后续回显。
    let key = (
        state.session_id.clone(),
        input,
        output,
        cached,
        cache_write,
        total,
    );
    if !dedupe.insert(key) {
        stats.duplicates_dropped += 1;
        return;
    }
    events.push(UsageEvent {
        ts,
        agent: AgentKind::Codex,
        model: model.to_string(),
        session_id: state.session_id.clone(),
        project,
        input_tokens: input - cached,
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

    fn collect_lines(lines: &[String]) -> Collection {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        let mut dedupe = HashSet::new();
        ingest_text(&lines.join("\n"), &mut stats, &mut events, &mut dedupe);
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
            meta("sess-a", r"C:/work/alpha"),
            ctx("gpt-5.6-sol", r"C:/work/alpha"),
            tc("2026-07-17T15:59:00.000Z", 1000, 100, 200, 50, 1100),
        ]);
        assert_eq!(col.events.len(), 1);
        let e = &col.events[0];
        // 归一化：input 剔除缓存，cached 归 cache_read。
        assert_eq!(e.input_tokens, 800);
        assert_eq!(e.output_tokens, 100);
        assert_eq!(e.cache_write_tokens, 50);
        assert_eq!(e.cache_read_tokens, 200);
        assert_eq!(e.model, "gpt-5.6-sol");
        assert_eq!(e.project, "alpha");
        assert_eq!(e.session_id, "sess-a");
        assert_eq!(col.stats.bad_lines, 0);
    }

    #[test]
    fn test_codex_model_attribution_and_session_boundary() {
        let col = collect_lines(&[
            meta("sess-a", r"C:/work/alpha"),
            ctx("gpt-5.6-sol", r"C:/work/alpha"),
            tc("2026-07-17T15:59:00.000Z", 10, 5, 0, 0, 15),
            // 新 session：模型重置，此行无模型可归属
            meta("sess-b", r"C:/work/beta"),
            tc("2026-07-17T16:00:00.000Z", 9, 9, 0, 0, 18),
            ctx("gpt-5.5", r"C:/work/beta"),
            tc("2026-07-17T16:01:00.000Z", 7, 3, 2, 0, 10),
            // 模型切换
            ctx("gpt-5.4", r"C:/work/beta"),
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
            meta("s", r"C:/w"),
            ctx("m", r"C:/w"),
            tc("2026-07-17T15:59:00.000Z", 0, 0, 0, 0, 19457),
        ]);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.skipped_zero_usage, 1);
    }

    #[test]
    fn test_codex_badline() {
        let col = collect_lines(&[
            "not json".to_string(),
            meta("s", r"C:/w"),
            ctx("m", r"C:/w"),
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
    fn test_codex_dedupe_keeps_first() {
        // 同一请求原样重发（时间戳不同、数值相同）：保留首条，丢弃计数。
        let col = collect_lines(&[
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            tc("2026-07-17T15:59:00.000Z", 1000, 100, 200, 50, 1100),
            tc("2026-07-17T15:59:05.000Z", 1000, 100, 200, 50, 1100),
            tc("2026-07-17T16:00:00.000Z", 10, 5, 0, 0, 15),
        ]);
        assert_eq!(col.events.len(), 2);
        assert_eq!(col.stats.duplicates_dropped, 1);
        assert_eq!(col.events[0].input_tokens, 800);
        assert!(col.events[0].ts < col.events[1].ts, "首条在前");
    }

    #[test]
    fn test_codex_token_usage_record_ignored() {
        let col = collect_lines(&[
            meta("s", r"C:/w"),
            ctx("m", r"C:/w"),
            r#"{"timestamp":"2026-07-17T15:59:00.000Z","type":"token_usage_record","payload":{"session_id":"s","thread_id":"t","turn_id":"t","root_turn_id":"t","response_id":"r","usage":{"input_tokens":9,"output_tokens":9,"total_tokens":18},"turn_token_usage":{},"thread_token_usage":{}}}"#.to_string(),
        ]);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.ignored_token_usage_record, 1);
    }

    fn fixture(p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/codex")
            .join(p)
    }

    #[test]
    fn test_codex_discovery_finds_nested_files() {
        let col = CodexSource::new(fixture("basic")).collect().unwrap();
        assert_eq!(col.stats.files_scanned, 2);
        assert_eq!(col.stats.events, 4);
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
