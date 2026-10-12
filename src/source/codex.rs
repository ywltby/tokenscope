//! Codex 适配器：扫描 `~/.codex/sessions/**/*.jsonl` 与
//! `~/.codex/archived_sessions/**/*.jsonl`（rollout 文件，append-only）。
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
//! - 项目身份（阶段 B）：按会话维护**项目根**——`turn_context` 的有效 cwd 在
//!   当前根之下则保持根（子目录归并），越出当前根即视为新项目；`session_meta`
//!   重置目录上下文（新会话不继承上一会话的目录）；空/无效 cwd 不清掉同会话
//!   已知目录；完全没有可信 cwd 时记 "(未知)"；展示名由聚合层派生；
//! - H02：默认发现范围 = `CODEX_HOME`（缺失时 `~/.codex`）下的 `sessions/`
//!   与 `archived_sessions/` 两个根，会话是否来自子代理一律不排除；**显式**
//!   自定义根只在选定目录内递归，不扩大到目录之外；
//! - 同请求重发的去重自 M4 起上移到全局 dedupe 步骤（按 `(session, 原始模型,
//!   用量五元组)` 保首条），本层原样产出事件（record_id 为空）。rollout 的
//!   `token_count` 不带请求标识，因此该来源**没有**可证实的跨来源身份
//!   （见 `UsageEvent::native_identity`），不猜测。

use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::project_path::{self, ProjectRootTracker};
use super::{CollectStats, FileParse, Source, read_text, walk_jsonl};
use crate::model::{AgentKind, UsageEvent};

pub struct CodexSource {
    roots: Vec<PathBuf>,
}

impl CodexSource {
    /// 单根来源：只在给定目录内递归（显式自定义根语义）。
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            roots: vec![root.into()],
        }
    }

    /// 多根来源：逐根递归，各自独立发现（默认来源的 `sessions` +
    /// `archived_sessions`）。
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    /// `CODEX_HOME`（缺失或空值时用 `~/.codex`）。
    pub fn codex_home() -> Result<PathBuf> {
        if let Some(raw) = std::env::var_os("CODEX_HOME") {
            let p = PathBuf::from(raw);
            if !p.as_os_str().is_empty() {
                return Ok(p);
            }
        }
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(home.join(".codex"))
    }

    /// 默认根集合：`<CODEX_HOME>/sessions` 与 `<CODEX_HOME>/archived_sessions`。
    /// RC10：验收模式一律用隔离根下的来源目录，绝不回退真实 `~/.codex`。
    pub fn default_roots() -> Result<Vec<PathBuf>> {
        if let Some(dir) = crate::acceptance::source_dir_override(false) {
            return Ok(vec![dir.join("sessions"), dir.join("archived_sessions")]);
        }
        let home = Self::codex_home()?;
        Ok(vec![home.join("sessions"), home.join("archived_sessions")])
    }

    /// 兼容入口：默认根集合中的第一个（`sessions`）。
    pub fn default_root() -> Result<PathBuf> {
        Ok(Self::default_roots()?
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from(".")))
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
///
/// 上下文元数据（id / session_id / cwd / model）一律用**宽容类型**：类型异常
/// 只表示"没有可信值"，不得让整条记录解析失败——否则 `session_meta` 的会话
/// 边界重置与 `turn_context` 的目录上下文会被静默跳过，新会话的请求被算到
/// 旧会话与旧项目上。用量字段（`info`）保持严格：类型异常按坏行计数。
#[derive(Default, Deserialize)]
struct RolloutPayload {
    #[serde(rename = "type", default)]
    kind: String,
    // session_meta
    #[serde(default)]
    id: Option<serde_json::Value>,
    #[serde(default)]
    session_id: Option<serde_json::Value>,
    #[serde(default)]
    cwd: Option<serde_json::Value>,
    // turn_context
    #[serde(default)]
    model: Option<serde_json::Value>,
    // event_msg/token_count
    #[serde(default)]
    info: Option<TokenInfo>,
    // 额度更新也复用 token_count，此时 info 为 null；不参与用量计量。
    #[serde(default)]
    rate_limits: Option<serde_json::Value>,
}

impl RolloutPayload {
    fn as_str_field(v: &Option<serde_json::Value>) -> Option<&str> {
        v.as_ref().and_then(serde_json::Value::as_str)
    }

    fn id(&self) -> Option<&str> {
        Self::as_str_field(&self.id)
    }

    fn session_id(&self) -> Option<&str> {
        Self::as_str_field(&self.session_id)
    }

    fn cwd(&self) -> Option<&str> {
        Self::as_str_field(&self.cwd)
    }

    fn model(&self) -> Option<&str> {
        Self::as_str_field(&self.model)
    }
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

/// 文件内解析状态：session 边界、模型归属与项目根。
#[derive(Default)]
struct ScanState {
    session_id: String,
    model: Option<String>,
    /// 阶段 B：当前会话的项目根状态机（子目录归并、越界成新项目）。
    tracker: ProjectRootTracker,
    /// 本会话首个有效 cwd（B02 的 `session_initial_cwd`）。
    initial_cwd: Option<String>,
}

/// 观察一个（未归一化的）cwd：只接受可归一化的值；空值、类型异常与不可解释的
/// 路径既不写入，也**不清掉**同会话已知目录。
fn observe_cwd(state: &mut ScanState, raw: Option<&str>) {
    let Some(raw) = raw else {
        return;
    };
    if let Some(identity) = project_path::normalize_project_path(raw) {
        if state.initial_cwd.is_none() {
            state.initial_cwd = Some(identity.clone());
        }
        state.tracker.observe(&identity);
    }
}

/// 是否是用量 rollout 候选：排除 CLI 的 `history.jsonl`（提示词历史，
/// 不含 `token_count` 用量，纳入只会污染来源统计与缓存）。
fn is_rollout_candidate(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_none_or(|n| !n.eq_ignore_ascii_case("history.jsonl"))
}

impl Source for CodexSource {
    fn agent(&self) -> AgentKind {
        AgentKind::Codex
    }

    fn root(&self) -> &Path {
        // 兼容入口：多根来源报告第一个根；目录检查请用 `roots()`。
        self.roots
            .first()
            .map(PathBuf::as_path)
            .unwrap_or_else(|| Path::new(""))
    }

    fn roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }

    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for root in &self.roots {
            // 根目录缺失由调用方统一告警（"目录不存在"），不在此重复记异常。
            if !root.is_dir() {
                continue;
            }
            let mut found = Vec::new();
            walk_jsonl(root, &mut found, &mut errors);
            for f in found {
                if is_rollout_candidate(&f) {
                    out.push(f);
                } else {
                    // H02：发现只覆盖用量 rollout——`history.jsonl` 是 CLI 的
                    // 提示词历史，不是用量记录，误纳会污染来源统计。
                    log::debug!("Codex 发现：跳过非 rollout 文件 {}", f.display());
                }
            }
        }
        out.sort();
        out.dedup();
        (out, errors)
    }

    fn parse_file(&self, path: &Path) -> FileParse {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        let source_path = path.display().to_string();
        match read_text(path) {
            Ok(text) => ingest_text(&text, &source_path, &mut stats, &mut events),
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
    source_path: &str,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
) {
    let mut state = ScanState::default();
    for (line_no, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(line, line_no as u64, source_path, &mut state, stats, events);
    }
}

fn ingest_line(
    line: &str,
    line_no: u64,
    source_path: &str,
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
            // session 边界：模型归属、项目根与初始目录一起重置——新会话绝不
            // 继承上一个 session 的目录（A04 不变量 4）。上下文字段类型异常
            // 只影响该字段本身，不阻断边界重置（审查修订）。
            state.session_id = payload
                .session_id()
                .or_else(|| payload.id())
                .map(str::to_string)
                .unwrap_or_default();
            state.model = None;
            state.tracker.reset();
            state.initial_cwd = None;
            observe_cwd(state, payload.cwd());
        }
        "turn_context" => {
            if let Some(m) = payload.model() {
                state.model = Some(m.to_string());
            }
            observe_cwd(state, payload.cwd());
        }
        "token_usage_record" => stats.ignored_token_usage_record += 1,
        "event_msg" if payload.kind == "token_count" => {
            ingest_token_count(
                rec.timestamp.as_deref(),
                line_no,
                source_path,
                payload,
                state,
                stats,
                events,
            );
        }
        _ => {}
    }
}

fn ingest_token_count(
    ts_str: Option<&str>,
    line_no: u64,
    source_path: &str,
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
    // 仅额度通知没有请求用量。只放行对象形态，缺损的 info 或非法时间戳
    // 仍走坏行诊断；有 info 时绝不能用 rate_limits 掩盖缺失的 usage。
    if payload.info.is_none()
        && payload
            .rate_limits
            .as_ref()
            .is_some_and(serde_json::Value::is_object)
    {
        return;
    }
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
    // SF08：逐字段比较，不做加法（异常大值加法会回绕/panic）。
    if input == 0 && output == 0 && cached == 0 && cache_write == 0 {
        stats.skipped_zero_usage += 1;
        return;
    }
    // 口径校验（R02）：total = input + output；cached 与 cache_write 均是
    // input 的子集。cached + cache_write > input 属未查证的字段组合，
    // 不猜测语义，按坏行计数暴露。
    // SF08：全部受检——加法溢出/子集不成立都计 bad_lines 跳过本行，
    // 继续后续行；不回绕、不饱和、不 panic。
    let Some(total_calc) = input.checked_add(output) else {
        stats.bad_lines += 1;
        return;
    };
    let Some(cached_plus_cw) = cached.checked_add(cache_write) else {
        stats.bad_lines += 1;
        return;
    };
    if total != total_calc || cached_plus_cw > input {
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
    // A04/B：项目身份 = 当前会话的项目根（子目录归并、越出当前根即新项目）；
    // 不同路径的同名目录不合并，同路径的跨工具会话共用同一 key；无有效 cwd 时
    // 沿用既有 "(未知)" 兜底，展示名（basename）由聚合层派生为 Group.label。
    let project = state
        .tracker
        .root()
        .map_or_else(|| "(未知)".to_string(), str::to_string);
    let event_cwd = state.tracker.cwd().map(str::to_string);
    let session_initial_cwd = state.initial_cwd.clone();
    // 守恒已受检确认 cached + cache_write ≤ input 且 total 可表示：
    // 非缓存输入一次减法得到（不再逐项减）。
    let event = UsageEvent {
        ts,
        agent: AgentKind::Codex,
        model: model.to_string(),
        session_id: state.session_id.clone(),
        project,
        session_initial_cwd,
        event_cwd,
        record_id: String::new(),
        line: line_no,
        source_path: source_path.to_string(),
        input_tokens: input - cached_plus_cw,
        output_tokens: output,
        cache_write_tokens: cache_write,
        cache_read_tokens: cached,
    };
    // SF08：source→model 公共边界（与缓存恢复同一路径）防御性兜底。
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

    fn collect_lines(lines: &[String]) -> Collection {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        ingest_text(&lines.join("\n"), "test.jsonl", &mut stats, &mut events);
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
    fn test_codex_rate_limit_only_preserves_usage() {
        let parsed = CodexSource::new(fixture("")).parse_file(&fixture("rate-limit-only.jsonl"));
        assert_eq!(parsed.stats.bad_lines, 0);
        assert_eq!(parsed.stats.lines_seen, 5);
        assert_eq!(parsed.events.len(), 2);
        assert_eq!(parsed.events[0].input_tokens, 10);
        assert_eq!(parsed.events[0].output_tokens, 5);
        assert_eq!(parsed.events[1].input_tokens, 20);
        assert_eq!(parsed.events[1].output_tokens, 7);
        assert!(
            parsed
                .events
                .iter()
                .all(|e| e.model == "synthetic-model" && e.session_id == "synthetic-rate-limit")
        );
    }

    #[test]
    fn test_codex_rate_limits_do_not_hide_malformed_usage() {
        for payload in [
            serde_json::json!({"info": {}, "rate_limits": {}}),
            serde_json::json!({"info": {"last_token_usage": null}, "rate_limits": {}}),
            serde_json::json!({"info": null}),
            serde_json::json!({"info": null, "rate_limits": null}),
            serde_json::json!({"info": null, "rate_limits": 0}),
            serde_json::json!({"info": null, "rate_limits": []}),
            serde_json::json!({"info": {"last_token_usage": {"input_tokens": -1}}, "rate_limits": {}}),
        ] {
            let mut payload = payload;
            payload["type"] = serde_json::json!("token_count");
            let line = serde_json::json!({"timestamp": "2026-07-17T15:03:00Z", "type": "event_msg", "payload": payload}).to_string();
            let col = collect_lines(std::slice::from_ref(&line));
            assert_eq!(col.stats.bad_lines, 1, "{line}");
            assert!(col.events.is_empty());
        }
        let col = collect_lines(&[
            serde_json::json!({"timestamp": "invalid", "type": "event_msg", "payload": {"type": "token_count", "info": null, "rate_limits": {}}}).to_string(),
        ]);
        assert_eq!(col.stats.bad_lines, 1);
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
        assert_eq!(e.project, "C:/work/alpha");
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
        assert_eq!(projects, ["C:/work/alpha", "C:/work/beta", "C:/work/beta"]);
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
    fn test_project_same_basename_distinct() {
        // C2（R03）：不同路径的同名目录是两个项目（修复前按 basename 合并）。
        let col = collect_lines(&[
            meta("s1", "C:/work/alpha"),
            ctx("m", "C:/work/alpha"),
            tc("2026-07-17T15:59:00.000Z", 10, 5, 0, 0, 15),
            meta("s2", "D:/other/alpha"),
            ctx("m", "D:/other/alpha"),
            tc("2026-07-17T16:00:00.000Z", 10, 5, 0, 0, 15),
        ]);
        let projects: Vec<&str> = col.events.iter().map(|e| e.project.as_str()).collect();
        assert_eq!(projects, ["C:/work/alpha", "D:/other/alpha"]);
        // 展示名同名（聚合层派生 label），身份不同。
        assert_eq!(projects.len(), 2);
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
