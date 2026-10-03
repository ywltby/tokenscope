//! Claude Code 适配器：扫描 `~/.claude/projects/<项目slug>/*.jsonl`。
//!
//! 口径（依据本机实测，见 docs/plans/archive 的 M1 计划）：
//! - 只取 `type == "assistant"` 行的 `message.usage`；
//! - 同一 `(sessionId, message.id)` 去重并保留时间戳最晚的一条（时间戳相同则
//!   保留文件行序靠后的一条——流式重写总是追加在尾部，约 70% 行是这类重写）；
//! - `isSidechain` 与 `<synthetic>` 跳过并计数；解析失败 / 缺 usage / 缺时间戳 /
//!   缺 message.id 计入坏行。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::{CollectStats, Collection, Source, is_jsonl};
use crate::model::{AgentKind, UsageEvent};

pub const SYNTHETIC_MODEL: &str = "<synthetic>";

#[derive(Default, Deserialize)]
struct ClaudeLine {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(rename = "sessionId", default)]
    session_id: String,
    #[serde(default)]
    message: Option<ClaudeMessage>,
}

#[derive(Default, Deserialize)]
struct ClaudeMessage {
    #[serde(default)]
    id: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    usage: Option<ClaudeUsage>,
}

#[derive(Default, Deserialize)]
struct ClaudeUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

pub struct ClaudeSource {
    root: PathBuf,
}

impl ClaudeSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> Result<PathBuf> {
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(home.join(".claude").join("projects"))
    }
}

impl Source for ClaudeSource {
    fn agent(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn collect(&self) -> Result<Collection> {
        let mut stats = CollectStats::default();
        let mut warnings = Vec::new();
        let mut events: Vec<UsageEvent> = Vec::new();
        let mut dedupe: HashMap<(String, String), usize> = HashMap::new();

        // 不变量：目录缺失是常态（机器上没装该 agent），警告后以空结果正常返回。
        if !self.root.is_dir() {
            warnings.push(format!("Claude 目录不存在：{}", self.root.display()));
            return Ok(Collection {
                agent: self.agent(),
                events,
                stats,
                warnings,
            });
        }

        for (project, path) in discover(&self.root) {
            stats.files_scanned += 1;
            match fs::read(&path) {
                // lossy 容错：个别非法字节不应让整份文件失败，混进来的垃圾行会被坏行计数接住。
                Ok(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    ingest_text(&text, &project, &mut stats, &mut events, &mut dedupe);
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

/// 返回 `(项目名, jsonl 路径)`，按目录名与文件名排序保证稳定输出。
/// 项目名 = projects 下第一级目录名；根下散放的 jsonl 记为 "(根目录)"。
fn discover(root: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            let project = child
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            collect_jsonl(&child, &project, &mut out);
        } else if is_jsonl(&child) {
            out.push(("(根目录)".to_string(), child));
        }
    }
    out
}

fn collect_jsonl(dir: &Path, project: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            collect_jsonl(&child, project, out);
        } else if is_jsonl(&child) {
            out.push((project.to_string(), child));
        }
    }
}

fn ingest_text(
    text: &str,
    project: &str,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
    dedupe: &mut HashMap<(String, String), usize>,
) {
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(line, project, stats, events, dedupe);
    }
}

fn ingest_line(
    line: &str,
    project: &str,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
    dedupe: &mut HashMap<(String, String), usize>,
) {
    let rec: ClaudeLine = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };
    if rec.kind != "assistant" {
        return;
    }
    if rec.is_sidechain {
        stats.skipped_sidechain += 1;
        return;
    }
    let Some(msg) = rec.message.as_ref() else {
        stats.bad_lines += 1;
        return;
    };
    if msg.model == SYNTHETIC_MODEL {
        stats.skipped_synthetic += 1;
        return;
    }
    let Some(usage) = msg.usage.as_ref() else {
        stats.bad_lines += 1;
        return;
    };
    let Some(ts_str) = rec.timestamp.as_deref() else {
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
    if msg.id.is_empty() {
        // 无 id 无法参与去重，计入坏行避免重复计费风险。
        stats.bad_lines += 1;
        return;
    }

    let event = UsageEvent {
        ts,
        agent: AgentKind::ClaudeCode,
        model: msg.model.clone(),
        session_id: rec.session_id.clone(),
        project: project.to_string(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_write_tokens: usage.cache_creation_input_tokens,
        cache_read_tokens: usage.cache_read_input_tokens,
    };
    let key = (rec.session_id.clone(), msg.id.clone());
    match dedupe.get(&key) {
        Some(&idx) => {
            stats.duplicates_dropped += 1;
            if event.ts >= events[idx].ts {
                events[idx] = event;
            }
        }
        None => {
            dedupe.insert(key, events.len());
            events.push(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_text(text: &str) -> Collection {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        let mut dedupe = HashMap::new();
        ingest_text(text, "proj-t", &mut stats, &mut events, &mut dedupe);
        stats.events = events.len() as u64;
        Collection {
            agent: AgentKind::ClaudeCode,
            events,
            stats,
            warnings: Vec::new(),
        }
    }

    fn assistant(id: &str, ts: &str, usage: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","sessionId":"s1","message":{{"id":"{id}","model":"claude-sonnet-4-5","usage":{usage}}}}}"#
        )
    }

    #[test]
    fn test_parse_typical() {
        let col = collect_text(
            &[
                r#"{"type":"user","timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s1","message":{"role":"user"}}"#.to_string(),
                assistant("m1", "2026-07-17T08:00:01.000Z", r#"{"input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4}"#),
                r#"{"type":"system","timestamp":"2026-07-17T08:00:02.000Z","content":"x"}"#.to_string(),
            ]
            .join("\n"),
        );
        assert_eq!(col.events.len(), 1);
        let e = &col.events[0];
        assert_eq!(e.model, "claude-sonnet-4-5");
        assert_eq!(e.project, "proj-t");
        assert_eq!(e.input_tokens, 10);
        assert_eq!(e.output_tokens, 2);
        assert_eq!(e.cache_write_tokens, 3);
        assert_eq!(e.cache_read_tokens, 4);
        assert_eq!(col.stats.lines_seen, 3);
        assert_eq!(col.stats.bad_lines, 0);
    }

    #[test]
    fn test_parse_badline() {
        let col = collect_text(&[
            "not json at all".to_string(),
            assistant("m1", "2026-07-17T08:00:01.000Z", r#"{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#),
            // 缺 usage
            r#"{"type":"assistant","timestamp":"2026-07-17T08:00:02.000Z","sessionId":"s1","message":{"id":"m2","model":"m"}}"#.to_string(),
            // 缺时间戳
            r#"{"type":"assistant","sessionId":"s1","message":{"id":"m3","model":"m","usage":{"input_tokens":1,"output_tokens":1}}}"#.to_string(),
            // 时间戳格式非法
            r#"{"type":"assistant","timestamp":"yesterday","sessionId":"s1","message":{"id":"m4","model":"m","usage":{"input_tokens":1,"output_tokens":1}}}"#.to_string(),
            // 缺 message.id
            r#"{"type":"assistant","timestamp":"2026-07-17T08:00:03.000Z","sessionId":"s1","message":{"model":"m","usage":{"input_tokens":1,"output_tokens":1}}}"#.to_string(),
        ].join("\n"));
        assert_eq!(col.events.len(), 1);
        assert_eq!(col.stats.bad_lines, 5);
        assert_eq!(col.stats.lines_seen, 6);
    }

    #[test]
    fn test_dedupe_keeps_last() {
        let col = collect_text(&[
            assistant("m1", "2026-07-17T08:00:00.000Z", r#"{"input_tokens":10,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#),
            assistant("m1", "2026-07-17T08:00:01.000Z", r#"{"input_tokens":30,"output_tokens":30,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#),
            assistant("m1", "2026-07-17T08:00:00.500Z", r#"{"input_tokens":20,"output_tokens":20,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#),
            // 同 sessionId 不同 id 不受影响
            assistant("m2", "2026-07-17T08:00:02.000Z", r#"{"input_tokens":7,"output_tokens":7,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#),
        ].join("\n"));
        assert_eq!(col.events.len(), 2);
        assert_eq!(col.stats.duplicates_dropped, 2);
        let kept = col.events.iter().find(|e| {
            e.session_id == "s1" && e.model == "claude-sonnet-4-5" && e.input_tokens == 30
        });
        assert!(kept.is_some(), "应保留时间戳最晚的 30/30 行");
        assert!(
            col.events
                .iter()
                .all(|e| e.input_tokens != 20 || e.output_tokens != 20)
        );
    }

    #[test]
    fn test_sidechain_excluded() {
        let line = r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s1","message":{"id":"m1","model":"m","usage":{"input_tokens":9,"output_tokens":9}}}"#;
        let col = collect_text(line);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.skipped_sidechain, 1);
    }

    #[test]
    fn test_synthetic_skipped() {
        let line = r#"{"type":"assistant","timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s1","message":{"id":"m1","model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}"#;
        let col = collect_text(line);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.skipped_synthetic, 1);
    }

    fn fixture(p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/claude")
            .join(p)
    }

    #[test]
    fn test_discovery_finds_project_files() {
        let col = ClaudeSource::new(fixture("basic")).collect().unwrap();
        assert_eq!(col.stats.files_scanned, 2);
        let projects: std::collections::BTreeSet<&str> =
            col.events.iter().map(|e| e.project.as_str()).collect();
        assert_eq!(projects, ["proj-alpha", "proj-beta"].into_iter().collect());
        assert!(col.warnings.is_empty());
    }

    #[test]
    fn test_discovery_missing_dir_warns() {
        let col = ClaudeSource::new(fixture("no-such-dir")).collect().unwrap();
        assert!(col.events.is_empty());
        assert_eq!(col.warnings.len(), 1);
        assert_eq!(col.stats.files_scanned, 0);
    }
}
