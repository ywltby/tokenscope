//! Claude Code 适配器：扫描 `~/.claude/projects/<项目slug>/*.jsonl`。
//!
//! 口径（依据本机实测，见 docs/plans/archive 的 M1 计划与 M4 重构）：
//! - 只取 `type == "assistant"` 行的 `message.usage`，产出**未去重**事件
//!   （`record_id` = message.id，跨文件去重由全局 dedupe 步骤执行）；
//! - `isSidechain` 与 `<synthetic>` 跳过并计数；解析失败 / 缺 usage / 缺时间戳 /
//!   缺 message.id 计入坏行；
//! - 项目身份（A02）：会话**初始 cwd**——顶层 `cwd` 归一化后按 sessionId 取
//!   首个有效值；无可信 cwd 时回落文件父目录相对根的路径（真实布局下即
//!   slug，根下散放记 "(根目录)"）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::{CollectStats, FileParse, Source, project_path, read_text, walk_jsonl};
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
    /// A02：会话上下文里的 cwd（顶层字段）。缺失、空值、类型异常一律只是
    /// 「没有可信身份」——路径提取容错与 usage 校验分开，不产生坏行。
    #[serde(default)]
    cwd: Option<serde_json::Value>,
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
        // RC10：验收模式一律用隔离根下的来源目录，**绝不**回退真实
        // ~/.claude/projects（哪怕该目录不存在也只报"目录不存在"）。
        if let Some(dir) = crate::acceptance::source_dir_override(true) {
            return Ok(dir.join("projects"));
        }
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(home.join(".claude").join("projects"))
    }

    /// 项目名 = projects 下第一级目录名；根下散放的 jsonl 记为 "(根目录)"。
    /// 项目身份 = 相对根的父目录路径（C2/R03）：真实布局（projects/<slug>/
    /// *.jsonl）下即 slug 本身、行为不变；自定义目录树嵌套时 "a/sub" 与
    /// "sub" 可区分，不再因父目录同名误合并。
    fn project_of(&self, path: &Path) -> String {
        let parent = match path.parent() {
            Some(p) if p == self.root => return "(根目录)".to_string(),
            Some(p) => p,
            None => return "(根目录)".to_string(),
        };
        let rel = parent
            .strip_prefix(&self.root)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| parent.display().to_string());
        if rel.is_empty() {
            "(根目录)".to_string()
        } else {
            rel
        }
    }
}

impl Source for ClaudeSource {
    fn agent(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        // 根目录缺失由调用方统一告警（"目录不存在"），不在此重复记异常。
        if !self.root.is_dir() {
            return (Vec::new(), Vec::new());
        }
        let mut out = Vec::new();
        let mut errors = Vec::new();
        walk_jsonl(&self.root, &mut out, &mut errors);
        out.sort();
        (out, errors)
    }

    fn parse_file(&self, path: &Path) -> FileParse {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        let project = self.project_of(path);
        match read_text(path) {
            Ok(text) => ingest_text(&text, &project, &mut stats, &mut events),
            // 读取失败（B4）：计 io_errors、返回空产物——报告层据此跳过
            // 成功缓存并告警，不与"坏行"（内容问题）混计。
            Err(_) => stats.io_errors += 1,
        }
        stats.events = events.len() as u64;
        FileParse { stats, events }
    }
}

fn ingest_text(
    text: &str,
    file_project: &str,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
) {
    // A02：一次读取、一轮解析——先按 sessionId 收集「首个有效 cwd」，最后统一
    // 赋值给该会话的事件（不为每个事件重读文件）。日志前缀缺失时较晚出现的
    // 首值同样可用于本会话已解析事件；跨 session 绝不共用首值（含无 ID 分组，
    // 该分组只在文件内有效）。
    let mut initial: HashMap<String, String> = HashMap::new();
    let mut pending: Vec<(String, UsageEvent)> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(line, stats, &mut initial, &mut pending);
    }
    for (session, mut event) in pending {
        event.project = initial
            .get(&session)
            .cloned()
            .unwrap_or_else(|| file_project.to_string());
        events.push(event);
    }
}

fn ingest_line(
    line: &str,
    stats: &mut CollectStats,
    initial: &mut HashMap<String, String>,
    pending: &mut Vec<(String, UsageEvent)>,
) {
    let rec: ClaudeLine = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };
    // A02：身份上下文采集——非 assistant 行（user/system 等）同样提供上下文；
    // sidechain 记录属子代理上下文，不参与主会话身份。只取**首个有效**值：
    // 归一化失败（相对路径、类型异常、空值）不写入、不影响后续行，也不计坏行。
    if !rec.is_sidechain
        && let Some(key) = rec
            .cwd
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .and_then(project_path::normalize_project_path)
    {
        initial.entry(rec.session_id.clone()).or_insert(key);
    }
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
        // 无 id 无法参与全局去重，计入坏行避免重复计费风险。
        stats.bad_lines += 1;
        return;
    }

    let event = UsageEvent {
        ts,
        agent: AgentKind::ClaudeCode,
        model: msg.model.clone(),
        session_id: rec.session_id.clone(),
        // A02：项目身份在 ingest_text 收尾时按会话首值统一定稿，此处只占位。
        project: String::new(),
        record_id: msg.id.clone(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_write_tokens: usage.cache_creation_input_tokens,
        cache_read_tokens: usage.cache_read_input_tokens,
    };
    // SF08：source→model 公共桶边界——四字段独立来源同样校验可表示性
    //（异常组合计 bad_lines 跳过，不回绕）。
    if event.validate_buckets().is_err() {
        stats.bad_lines += 1;
        return;
    }
    pending.push((rec.session_id.clone(), event));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Collection;

    fn collect_text(text: &str) -> Collection {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        ingest_text(text, "proj-t", &mut stats, &mut events);
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
                assistant(
                    "m1",
                    "2026-07-17T08:00:01.000Z",
                    r#"{"input_tokens":10,"output_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4}"#,
                ),
                r#"{"type":"system","timestamp":"2026-07-17T08:00:02.000Z","content":"x"}"#.to_string(),
            ]
            .join("\n"),
        );
        assert_eq!(col.events.len(), 1);
        let e = &col.events[0];
        assert_eq!(e.model, "claude-sonnet-4-5");
        assert_eq!(e.project, "proj-t");
        assert_eq!(e.record_id, "m1");
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
            assistant(
                "m1",
                "2026-07-17T08:00:01.000Z",
                r#"{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#,
            ),
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
    fn test_parse_no_dedupe_here() {
        // M4 起重复行由全局 dedupe 处理，source 层原样产出。
        let col = collect_text(&[
            assistant(
                "m1",
                "2026-07-17T08:00:00.000Z",
                r#"{"input_tokens":10,"output_tokens":10,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#,
            ),
            assistant(
                "m1",
                "2026-07-17T08:00:01.000Z",
                r#"{"input_tokens":30,"output_tokens":30,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}"#,
            ),
        ].join("\n"));
        assert_eq!(col.events.len(), 2);
        assert_eq!(col.stats.duplicates_dropped, 0);
    }

    #[test]
    fn test_sidechain_excluded() {
        let line = r#"{"type":"assistant","isSidechain":true,"timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s1","message":{"id":"m1","model":"m","usage":{"input_tokens":9,"output_tokens":9}}}"#;
        let col = collect_text(line);
        assert!(col.events.is_empty());
        assert_eq!(col.stats.skipped_sidechain, 1);
    }

    #[test]
    fn test_claude_nested_project_identity() {
        // Task 8（R03/C2）：嵌套目录的项目身份 = 相对根路径——"a/sub" 与
        // "sub" 可区分，同名父目录不再误合并；真实平铺 slug 布局行为不变。
        let dir = std::env::temp_dir().join(format!("tokenscope-t8-nest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let nested = dir.join("a").join("sub");
        let flat = dir.join("sub2");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&flat).unwrap();
        let line = r#"{"type":"assistant","timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s","message":{"id":"m","model":"m","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        std::fs::write(nested.join("n.jsonl"), line).unwrap();
        std::fs::write(flat.join("f.jsonl"), line).unwrap();
        let col = ClaudeSource::new(&dir).collect().unwrap();
        let mut projects: Vec<&str> = col.events.iter().map(|e| e.project.as_str()).collect();
        projects.sort();
        assert_eq!(
            projects,
            ["a\\sub", "sub2"],
            "嵌套与平铺身份可区分（Windows 路径分隔符）"
        );
        std::fs::remove_dir_all(&dir).ok();
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
        let src = ClaudeSource::new(fixture("basic"));
        let files = src.discover();
        assert_eq!(files.len(), 2);
        let projects: std::collections::BTreeSet<String> =
            files.iter().map(|p| src.project_of(p)).collect();
        let expected: std::collections::BTreeSet<String> = ["proj-alpha", "proj-beta"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(projects, expected);
        let col = src.collect().unwrap();
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
