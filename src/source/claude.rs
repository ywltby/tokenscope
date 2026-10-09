//! Claude Code 适配器：扫描 `~/.claude/projects/<项目slug>/*.jsonl`。
//!
//! 口径（依据本机实测，见 docs/plans/archive 的 M1 计划与 M4 重构）：
//! - 只取 `type == "assistant"` 行的 `message.usage`，产出**未去重**事件
//!   （`record_id` = message.id，跨文件去重由全局 dedupe 步骤执行）；
//! - `isSidechain` 与 `<synthetic>` 跳过并计数；解析失败 / 缺 usage / 缺时间戳 /
//!   缺 message.id 计入坏行；
//! - 项目身份：按会话维护**项目根**（阶段 B / B01 定稿规则）——进入当前根的
//!   子目录仍归该根，越出当前根即视为新项目（`ProjectRootTracker`）；
//!   会话完全无可信 cwd 时回落文件身份（A03：唯一正向映射解析出的真实路径 >
//!   相对根的父目录路径，即真实布局下的 slug；根下散放记 "(根目录)"）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::claude_projects::ProjectMapping;
use super::project_path::{self, ProjectRootTracker};
use super::{CollectStats, FileParse, Source, read_text, walk_jsonl};
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
    /// A03：项目映射（slug → 真实路径身份）。默认禁用——只有采集层明确
    /// 注入（默认账户配置或显式配置路径）才启用，自定义来源不读本机映射。
    mapping: Arc<ProjectMapping>,
}

impl ClaudeSource {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            mapping: Arc::new(ProjectMapping::disabled()),
        }
    }

    /// A03：带项目映射的构造器（采集层注入；测试同样显式注入）。
    pub fn with_mapping(root: impl Into<PathBuf>, mapping: Arc<ProjectMapping>) -> Self {
        Self {
            root: root.into(),
            mapping,
        }
    }

    pub fn mapping(&self) -> &ProjectMapping {
        &self.mapping
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

    /// A03：默认账户的 Claude 配置路径（`~/.claude.json`）。验收模式走隔离根
    /// 下的 `sources/claude/claude.json`，**绝不**读取真实账户配置。
    pub fn default_mapping_path() -> Result<PathBuf> {
        if let Some(dir) = crate::acceptance::source_dir_override(true) {
            return Ok(dir.join("claude.json"));
        }
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(home.join(".claude.json"))
    }

    /// 文件身份 = 会话 cwd 不可用时的兜底：相对根的父目录路径（C2/R03，
    /// 真实布局 `projects/<slug>/` 下即 slug 本身）；A03 起，该 slug 若能在
    /// 注入的映射里唯一正向命中，则改用归一化真实路径作为身份。
    fn project_of(&self, path: &Path) -> String {
        let legacy = self.slug_of(path);
        if legacy == "(根目录)" {
            return legacy;
        }
        match self.mapping.resolve(&legacy) {
            Some(identity) => identity.to_string(),
            // 未命中 / 冲突 slug / 映射禁用：保持独立 slug 身份，不做前缀猜测。
            None => legacy,
        }
    }

    /// 相对采集根的父目录路径（真实布局下 = 项目 slug）。
    fn slug_of(&self, path: &Path) -> String {
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

    /// A03：映射状态与内容摘要——变化即让文件缓存与查询复用键失效。
    fn context_revision(&self) -> &str {
        self.mapping.revision()
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
    // 阶段 B：一次读取、按行序推进每个 session 的项目根状态机（子目录归并、
    // 越出当前根即新项目），事件直接带归宿；不得为每个事件重读文件。
    // 完全无可信 cwd 的会话回落文件身份（唯一映射 > slug，见 project_of）。
    let mut scan = ClaudeScan::default();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(line, file_project, stats, events, &mut scan);
    }
}

/// 单文件扫描状态（阶段 B）：按 sessionId 维护项目根状态机与会话初始目录。
#[derive(Default)]
struct ClaudeScan {
    /// sessionId → 项目根（无 sessionId 的记录共用空串分组，仅限本文件）。
    trackers: HashMap<String, ProjectRootTracker>,
    /// sessionId → 会话首个有效 cwd（B02 的 `session_initial_cwd`）。
    initials: HashMap<String, String>,
}

fn ingest_line(
    line: &str,
    file_project: &str,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
    scan: &mut ClaudeScan,
) {
    let rec: ClaudeLine = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };
    // 身份上下文采集——非 assistant 行（user/system/attachment 等）同样提供；
    // sidechain 记录属子代理上下文，不参与主会话身份。归一化失败（相对路径、
    // 类型异常、空值）不写入、不影响后续行，也不计坏行。
    if !rec.is_sidechain
        && let Some(key) = rec
            .cwd
            .as_ref()
            .and_then(serde_json::Value::as_str)
            .and_then(project_path::normalize_project_path)
    {
        scan.initials
            .entry(rec.session_id.clone())
            .or_insert_with(|| key.clone());
        scan.trackers
            .entry(rec.session_id.clone())
            .or_default()
            .observe(&key);
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

    // 阶段 B：归属 = 该会话当前项目根（子目录归并、越界成新项目）；
    // 没有可信 cwd 的会话回落文件身份。
    let tracker = scan.trackers.get(&rec.session_id);
    let project = tracker
        .and_then(ProjectRootTracker::root)
        .map_or_else(|| file_project.to_string(), str::to_string);
    let event_cwd = tracker
        .and_then(ProjectRootTracker::cwd)
        .map(str::to_string);
    let session_initial_cwd = scan.initials.get(&rec.session_id).cloned();

    let event = UsageEvent {
        ts,
        agent: AgentKind::ClaudeCode,
        model: msg.model.clone(),
        session_id: rec.session_id.clone(),
        project,
        session_initial_cwd,
        event_cwd,
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
    events.push(event);
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
