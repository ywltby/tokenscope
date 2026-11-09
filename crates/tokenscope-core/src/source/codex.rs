//! Codex 适配器：扫描 `~/.codex/sessions/**/*.jsonl` 与
//! `~/.codex/archived_sessions/**/*.jsonl`（rollout 文件，append-only）。
//!
//! 口径（见 docs/stats-semantics.md）：
//! - 通常取 `event_msg`/`token_count` 的 `info.last_token_usage`；顶层
//!   `token_usage_record` 是单次请求用量，挂起到下一条 token_count：只有后者
//!   last 四分量存在且全零时单独入账；一致或非零不一致时只保留 token_count。
//!   后一条记录顶替前一条，session 边界和文件末尾丢弃未配对记录；
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
//! - 模型按行序最近的 `turn_context` / `thread_settings_applied` 归属，
//!   缺失、非字符串或空白模型不覆盖，遇 `session_meta` 重置；
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
use super::{CollectStats, FileParse, Source, read_text};
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
        if let Some(dir) = crate::acceptance::source_dir_override(crate::model::AgentKind::Codex) {
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
    // 顶层 token_usage_record：在对应分支严格解码，不影响其他记录的元数据。
    #[serde(default)]
    usage: Option<serde_json::Value>,
    // token_usage_record 的原始线程/响应身份（宽松类型：类型异常只表示
    // 没有可信值，不让整条记录解析失败）。thread_id 是响应所属线程——
    // legacy fork 复制历史后，它优先于文件内解析状态。turn_id 不参与身份：
    // 一轮工具循环内可有多次模型调用，禁止按 turn 去重。
    #[serde(default)]
    thread_id: Option<serde_json::Value>,
    #[serde(default)]
    response_id: Option<serde_json::Value>,
    #[serde(default)]
    thread_settings: Option<serde_json::Value>,
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

    /// 非空字符串字段（空白按缺失处理）。
    fn non_empty_str_field(v: &Option<serde_json::Value>) -> Option<&str> {
        Self::as_str_field(v).filter(|s| !s.trim().is_empty())
    }

    fn thread_id(&self) -> Option<&str> {
        Self::non_empty_str_field(&self.thread_id)
    }

    fn response_id(&self) -> Option<&str> {
        Self::non_empty_str_field(&self.response_id)
    }
}

#[derive(Default, Deserialize)]
struct TokenInfo {
    #[serde(default)]
    last_token_usage: Option<LastUsage>,
}

#[derive(Default, Clone, Deserialize)]
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

impl LastUsage {
    fn is_zero(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cached_input_tokens == 0
            && self.cache_write_input_tokens == 0
    }

    /// 四桶一致（total 逐桶守恒的产物，不参与比较——历史日志存在 total
    /// 计入口径差异，以四桶为准）。
    fn matches(&self, other: &LastUsage) -> bool {
        self.input_tokens == other.input_tokens
            && self.output_tokens == other.output_tokens
            && self.cached_input_tokens == other.cached_input_tokens
            && self.cache_write_input_tokens == other.cache_write_input_tokens
    }
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
    /// 记录位置的归一化用量与全部上下文；下一条 token_count 才决定是否产出。
    /// 仅承载**弱记录**（缺线程/响应身份的旧格式）——强记录立即产出。
    pending_record: Option<UsageEvent>,
    /// 挂起弱记录对应的 response_id（有则配对 token_count 时用于占位；
    /// 身份不足的旧格式为 None）。
    pending_response_id: Option<String>,
    /// 最近一次强记录观察：`(单次 usage, response_id)`。紧随其后的配对
    /// token_count（last 四桶一致）是同一响应的第二次观察，按此抑制；
    /// 用量不一致或没有该标记时走旧路径。
    last_strong_observation: Option<(LastUsage, String)>,
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
        // 独立调用（单测/状态）：自带台账，同样防环
        let mut ledger = super::DirVisitLedger::default();
        self.discover_with_ledger(&mut ledger)
    }

    fn discover_with_ledger(
        &self,
        ledger: &mut super::DirVisitLedger,
    ) -> (Vec<PathBuf>, Vec<String>) {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        for root in &self.roots {
            // 根目录缺失由调用方统一告警（"目录不存在"），不在此重复记异常。
            if !root.is_dir() {
                continue;
            }
            let mut found = Vec::new();
            super::walk_jsonl_guarded(root, self.agent(), ledger, &mut found, &mut errors);
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
    let lines: Vec<&str> = text.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(
            line,
            idx as u64,
            &lines,
            source_path,
            &mut state,
            stats,
            events,
        );
    }
    if state.pending_record.take().is_some() {
        stats.ignored_token_usage_record += 1;
    }
}

/// 旧解析器镜像（K04 升级映射）：强记录在**旧解析器表示**里对应哪条来源行。
///
/// 旧实现把 `token_usage_record` 挂起，由下一条 token_count 决定产物：
/// - 配对 count 的 last 四桶与记录一致 → 旧事件出在 count 行，
///   事件时间 = count 行时间戳，来源行 = count 行；
/// - last 四桶全零（compaction）→ 旧事件出在挂起记录本体：事件时间与
///   来源行都是记录行；
/// - last 非零且不一致 → 旧解析器忽略记录、按 count 出另一份事实——该记录
///   没有旧对应物，不建立映射（边界：旧 count 事实与新强记录并存）；
/// - 途中出现 session_meta / 下一条 token_usage_record → 挂起记录被丢弃，
///   无产物，不映射；
/// - 无关通知（非 token_count 的 event_msg 等）不打断配对；
/// - 坏行（JSON 解析失败）→ 旧解析器按坏行消费挂起记录，无产物，不映射；
/// - 文件结束 → 挂起记录被丢弃，不映射。
///
/// 返回 `(旧事件时间戳, 旧来源行号)`；行号为 0-based（与 line_no 一致）。
fn legacy_mirror(
    lines: &[&str],
    start: usize,
    record_ts: Timestamp,
    usage: &LastUsage,
) -> Option<(Timestamp, u64)> {
    // start 是记录行自己的下标：从下一行开始向前看；返回的行号还原为全文件
    // 0-based 下标（配对行 = start + 1 + off）。
    for (off, raw) in lines[start + 1..].iter().enumerate() {
        let value: serde_json::Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            // 坏行：旧解析器消费挂起记录并计坏行，无产物。
            Err(_) => return None,
        };
        let kind = value.get("type").and_then(serde_json::Value::as_str);
        match kind {
            Some("session_meta") | Some("token_usage_record") => return None,
            Some("event_msg") => {
                let payload = value.get("payload")?;
                let kind = payload.get("type").and_then(serde_json::Value::as_str);
                if kind != Some("token_count") {
                    // 无关通知：旧解析器不触碰挂起记录，继续等待。
                    continue;
                }
                // 配对点：info 缺失或 last 非法都是坏行语义——无产物。
                let last = payload
                    .pointer("/info/last_token_usage")
                    .and_then(|v| serde_json::from_value::<LastUsage>(v.clone()).ok())?;
                let pair_line = (start + 1 + off) as u64;
                if last.is_zero() {
                    // compaction 分支：旧事件 = 挂起记录本体——事件时间与
                    // **来源行号都是记录行**（复审修复 2026-11-09：旧实现把
                    // 挂起记录直接产出，来源证据记在 record 行；此前误返回
                    // count 行号导致镜像键错位、旧事实不升级而双计）。
                    return Some((record_ts, start as u64));
                }
                if last.matches(usage) {
                    let ts = value
                        .get("timestamp")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|s| s.parse::<Timestamp>().ok())?;
                    return Some((ts, pair_line));
                }
                // 非零不一致：旧解析器忽略记录、按 count 出另一份事实。
                return None;
            }
            _ => continue,
        }
    }
    None
}

fn ingest_line(
    line: &str,
    line_no: u64,
    all_lines: &[&str],
    source_path: &str,
    state: &mut ScanState,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
) {
    let rec: RolloutLine = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            stats.bad_lines += 1;
            // 用量字段仍严格解码；即使该行坏掉，也不能跨过已识别的配对/
            // 会话边界，拿之后的零行给旧记录补配对。
            if let Ok(raw) = serde_json::from_str::<serde_json::Value>(line) {
                let kind = raw.get("type").and_then(serde_json::Value::as_str);
                if kind == Some("session_meta") {
                    if state.pending_record.take().is_some() {
                        stats.ignored_token_usage_record += 1;
                    }
                    *state = ScanState::default();
                } else if kind == Some("token_usage_record")
                    || (kind == Some("event_msg")
                        && raw
                            .pointer("/payload/type")
                            .and_then(serde_json::Value::as_str)
                            == Some("token_count"))
                {
                    if state.pending_record.take().is_some() {
                        stats.ignored_token_usage_record += 1;
                    }
                    if kind == Some("token_usage_record") {
                        stats.ignored_token_usage_record += 1;
                    }
                }
            }
            return;
        }
    };
    if rec.kind == "session_meta" {
        if state.pending_record.take().is_some() {
            stats.ignored_token_usage_record += 1;
        }
        *state = ScanState::default();
    }
    let Some(payload) = rec.payload.as_ref() else {
        if rec.kind == "token_usage_record" {
            if state.pending_record.take().is_some() {
                stats.ignored_token_usage_record += 1;
            }
            stats.bad_lines += 1;
            stats.ignored_token_usage_record += 1;
        }
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
            observe_cwd(state, payload.cwd());
        }
        "turn_context" => {
            // 新 turn 开始：上一响应的配对窗口终结——其后出现的 count 与该
            // 响应无关（同响应的观察在下一 turn 开始前落盘）。
            state.last_strong_observation = None;
            if let Some(m) = payload.model().filter(|m| !m.trim().is_empty()) {
                state.model = Some(m.to_string());
            }
            observe_cwd(state, payload.cwd());
        }
        "token_usage_record" => {
            if state.pending_record.take().is_some() {
                stats.ignored_token_usage_record += 1;
            }
            state.pending_response_id = None;
            let usage = payload
                .usage
                .clone()
                .and_then(|value| serde_json::from_value::<LastUsage>(value).ok());
            let timestamp = rec
                .timestamp
                .as_deref()
                .and_then(|value| value.parse::<Timestamp>().ok());
            match (usage, timestamp) {
                (Some(u), Some(ts)) if !u.is_zero() => {
                    // 强记录（thread_id + response_id 齐备）：立即产出，身份以
                    // 记录的原始线程为准（优先于复制历史遗留的解析状态）；
                    // 配对的 token_count 只是同一响应的第二次观察，不再出数。
                    // 缺身份的记录走旧路径：挂起等待下一条 token_count 决定。
                    let thread = payload.thread_id();
                    let response = payload.response_id();
                    match (thread, response) {
                        (Some(thread), Some(response)) => {
                            // 镜像：旧解析器会从哪条来源行给这个响应记账。
                            // 行号以当前行为起点向前看（当前行在 all_lines 的
                            // 下标即 line_no）。
                            let mirror = legacy_mirror(all_lines, line_no as usize, ts, &u);
                            match event_from_usage(
                                ts,
                                line_no,
                                source_path,
                                &u,
                                Some((thread, response)),
                                mirror,
                                state,
                                stats,
                            ) {
                                Some(event) => {
                                    state.last_strong_observation =
                                        Some((u.clone(), response.to_string()));
                                    events.push(event);
                                }
                                None => stats.ignored_token_usage_record += 1,
                            }
                        }
                        _ => {
                            state.pending_response_id = response.map(str::to_string);
                            state.pending_record = event_from_usage(
                                ts,
                                line_no,
                                source_path,
                                &u,
                                None,
                                None,
                                state,
                                stats,
                            );
                            if state.pending_record.is_none() {
                                stats.ignored_token_usage_record += 1;
                            }
                        }
                    }
                }
                (Some(_), Some(_)) => stats.ignored_token_usage_record += 1,
                _ => {
                    stats.bad_lines += 1;
                    stats.ignored_token_usage_record += 1;
                }
            }
        }
        "event_msg" if payload.kind == "thread_settings_applied" => {
            if let Some(model) = payload
                .thread_settings
                .as_ref()
                .and_then(|settings| settings.get("model"))
                .and_then(serde_json::Value::as_str)
                .filter(|model| !model.trim().is_empty())
            {
                state.model = Some(model.to_string());
            }
        }
        "event_msg" if payload.kind == "token_count" => {
            if let Some(record) = state.pending_record.take() {
                state.pending_response_id = None;
                if payload
                    .info
                    .as_ref()
                    .and_then(|info| info.last_token_usage.as_ref())
                    .is_some_and(LastUsage::is_zero)
                {
                    events.push(record);
                    return;
                }
                stats.ignored_token_usage_record += 1;
            }
            // 强记录的配对观察（count 的 last 四桶与同一响应的 record 一致）
            // 不再出数——同一响应只有一条事实，观察消费后清除标记。
            //
            // 与最近强观察**不一致**的 count：强记录的迟到配对与另一响应的
            // count-only 在此无法区分——不猜测，按旧解析器行为照常出数并
            // 清除悬挂标记（复审修复 2026-11-09：此前直接 return 且不清理，
            // 一个悬挂观察会吞掉之后全部 count-only 请求）。
            //
            // 与最近强观察**一致**的 count（本分支抑制）：若它真是刚出数
            // 响应的配对观察，正确；若是独立同四桶请求，则是漏计——两者无
            // 身份证据可区分，不猜测，如实披露（docs/stats-semantics.md
            // §3.2"同用量配对的不可区分边界"）。新 turn 边界已在
            // turn_context 分支清除观察，边界后的 count 不进本分支。
            if let Some((observed_usage, _)) = state.last_strong_observation.take() {
                let same_usage = payload
                    .info
                    .as_ref()
                    .and_then(|info| info.last_token_usage.as_ref())
                    .is_some_and(|u| u.matches(&observed_usage));
                if same_usage {
                    return;
                }
                ingest_token_count(
                    rec.timestamp.as_deref(),
                    line_no,
                    source_path,
                    payload,
                    state,
                    stats,
                    events,
                );
                return;
            }
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
    if let Some(event) = event_from_usage(ts, line_no, source_path, u, None, None, state, stats) {
        events.push(event);
    }
}

#[allow(clippy::too_many_arguments)]
fn event_from_usage(
    ts: Timestamp,
    line_no: u64,
    source_path: &str,
    u: &LastUsage,
    strong: Option<(&str, &str)>,
    mirror: Option<(Timestamp, u64)>,
    state: &ScanState,
    stats: &mut CollectStats,
) -> Option<UsageEvent> {
    let (input, output, cached, cache_write, total) = (
        u.input_tokens,
        u.output_tokens,
        u.cached_input_tokens,
        u.cache_write_input_tokens,
        u.total_tokens,
    );
    // 未转为压缩事件的零分量占位行：只升 total 不升分量，跳过计数。
    // SF08：逐字段比较，不做加法（异常大值加法会回绕/panic）。
    if input == 0 && output == 0 && cached == 0 && cache_write == 0 {
        stats.skipped_zero_usage += 1;
        return None;
    }
    // 口径校验（R02）：total = input + output；cached 与 cache_write 均是
    // input 的子集。cached + cache_write > input 属未查证的字段组合，
    // 不猜测语义，按坏行计数暴露。
    // SF08：全部受检——加法溢出/子集不成立都计 bad_lines 跳过本行，
    // 继续后续行；不回绕、不饱和、不 panic。
    let Some(total_calc) = input.checked_add(output) else {
        stats.bad_lines += 1;
        return None;
    };
    let Some(cached_plus_cw) = cached.checked_add(cache_write) else {
        stats.bad_lines += 1;
        return None;
    };
    if total != total_calc || cached_plus_cw > input {
        stats.bad_lines += 1;
        return None;
    }
    let Some(model) = state.model.as_deref() else {
        stats.skipped_no_model += 1;
        return None;
    };
    if state.session_id.is_empty() {
        stats.bad_lines += 1;
        return None;
    }
    // 强记录：会话归属以**记录的原始线程**为准——legacy fork 子文件里复制的
    // 父请求保留父线程，子线程新请求归子线程，不受复制历史遗留的解析状态
    // 影响。线程未知时回退文件内解析状态（旧格式兼容）。模型缺省时请求无
    // 从归属，按既有口径跳过。
    let session = match strong {
        Some((thread, _)) => thread.to_string(),
        None => state.session_id.clone(),
    };
    if session.is_empty() {
        stats.bad_lines += 1;
        return None;
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
    // 镜像来源键：按旧解析器的产物形态重建内容指纹——事件时间取旧出数行、
    // 会话取旧解析状态（强记录也在记录时刻的文件状态）、record_id 恒空、
    // 四桶与本记录一致。逐字节相等才与旧事实建立映射。
    let legacy_origin = mirror.map(|(old_ts, old_line)| {
        let probe = UsageEvent {
            ts: old_ts,
            agent: AgentKind::Codex,
            model: model.to_string(),
            session_id: state.session_id.clone(),
            project: String::new(),
            session_initial_cwd: None,
            event_cwd: None,
            record_id: String::new(),
            claude_copy: None,
            codex_response: None,
            codex_legacy_origin: None,
            line: 0,
            source_path: source_path.to_string(),
            input_tokens: input - cached_plus_cw,
            output_tokens: output,
            cache_write_tokens: cache_write,
            cache_read_tokens: cached,
            cache_write_5m: 0,
            cache_write_1h: 0,
            cache_write_split_known: false,
        };
        format!(
            "native|{}|{}|{}|{}",
            AgentKind::Codex.as_str(),
            source_path,
            old_line,
            probe.content_fingerprint()
        )
    });
    let event = UsageEvent {
        ts,
        agent: AgentKind::Codex,
        model: model.to_string(),
        session_id: session.clone(),
        project,
        session_initial_cwd,
        event_cwd,
        record_id: strong
            .map(|(_, response)| response.to_string())
            .unwrap_or_default(),
        claude_copy: None,
        codex_legacy_origin: legacy_origin,
        codex_response: strong.map(|(thread, response)| crate::model::CodexResponseIdentity {
            thread_id: thread.to_string(),
            response_id: response.to_string(),
        }),
        line: line_no,
        source_path: source_path.to_string(),
        input_tokens: input - cached_plus_cw,
        output_tokens: output,
        cache_write_tokens: cache_write,
        cache_read_tokens: cached,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    // SF08：source→model 公共边界（与缓存恢复同一路径）防御性兜底。
    if event.validate_buckets().is_err() {
        stats.bad_lines += 1;
        return None;
    }
    Some(event)
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

    mod cp01 {
        use super::*;

        const TS: &str = "2026-07-17T15:59:00Z";
        fn record(input: u64, output: u64, cached: u64, cw: u64) -> String {
            serde_json::json!({"type":"token_usage_record","timestamp":TS,"payload":{
            "response_id":"synthetic-response","usage":{
                "input_tokens":input,"output_tokens":output,"cached_input_tokens":cached,
                "cache_write_input_tokens":cw,"total_tokens":input.checked_add(output).unwrap_or(0)
            }}})
            .to_string()
        }

        fn zero() -> String {
            tc("2026-07-17T16:00:00Z", 0, 0, 0, 0, 99999)
        }

        fn settings(model: serde_json::Value) -> String {
            serde_json::json!({"type":"event_msg","payload":{
                "type":"thread_settings_applied","thread_settings":{"model":model}
            }})
            .to_string()
        }

        #[test]
        fn compaction_record_followed_by_zero_last_is_counted() {
            let col = collect_lines(&[
                meta("s", "C:/before"),
                ctx("model-a", "C:/before/sub"),
                record(26705, 491, 12000, 3000),
                ctx("model-b", "D:/after"),
                zero(),
                zero(),
            ]);
            assert_eq!(col.events.len(), 1);
            let e = &col.events[0];
            assert_eq!(
                (
                    e.input_tokens,
                    e.output_tokens,
                    e.cache_read_tokens,
                    e.cache_write_tokens
                ),
                (11705, 491, 12000, 3000)
            );
            assert_eq!(e.ts, TS.parse::<Timestamp>().unwrap());
            assert_eq!(
                (&*e.model, &*e.session_id, &*e.project),
                ("model-a", "s", "C:/before")
            );
            assert_eq!(e.event_cwd.as_deref(), Some("C:/before/sub"));
            assert_eq!(e.session_initial_cwd.as_deref(), Some("C:/before"));
            assert_eq!(e.line, 2);
            assert_eq!(col.stats.skipped_zero_usage, 1);
            assert_eq!(col.stats.ignored_token_usage_record, 0);
            assert_eq!(col.stats.bad_lines, 0);
        }

        #[test]
        fn record_matching_following_token_count_is_not_double_counted() {
            // 串接真实压缩，确认匹配配对已消费，下一轮不会复用旧记录。
            let col = collect_lines(&[
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                record(100, 5, 20, 10),
                tc(TS, 100, 5, 20, 10, 105),
                zero(),
                record(200, 7, 30, 0),
                zero(),
            ]);
            let plain = collect_lines(&[
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                tc(TS, 100, 5, 20, 10, 105),
            ]);
            assert_eq!(col.events.len(), 2);
            let mut expected = plain.events[0].clone();
            expected.line = 3;
            assert_eq!(format!("{:?}", col.events[0]), format!("{expected:?}"));
            assert_eq!(col.events[1].input_tokens, 170);
            assert_eq!(col.stats.ignored_token_usage_record, 1);
            assert_eq!(col.stats.skipped_zero_usage, 1);
        }

        #[test]
        fn record_mismatching_nonzero_token_count_is_ignored() {
            let col = collect_lines(&[
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                record(100, 5, 20, 0),
                tc(TS, 101, 6, 21, 0, 107),
                zero(),
                record(200, 7, 0, 0),
                zero(),
            ]);
            assert_eq!(col.events.len(), 2);
            assert_eq!(
                (col.events[0].input_tokens, col.events[0].output_tokens),
                (80, 6)
            );
            assert_eq!(col.events[1].input_tokens, 200);
            assert_eq!(col.stats.ignored_token_usage_record, 1);
            assert_eq!(col.stats.skipped_zero_usage, 1);
        }

        #[test]
        fn unpaired_record_at_eof_or_session_boundary_is_not_counted() {
            let start = [
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                record(10, 1, 0, 0),
                zero(),
            ];
            let mut eof = start.to_vec();
            eof.push(record(20, 2, 0, 0));
            let col = collect_lines(&eof);
            assert_eq!(col.events.len(), 1);
            assert_eq!(col.stats.ignored_token_usage_record, 1);
            let mut boundary = eof;
            boundary.extend([
                meta("new", "D:/w"),
                ctx("n", "D:/w"),
                zero(),
                record(30, 3, 0, 0),
                zero(),
            ]);
            let col = collect_lines(&boundary);
            assert_eq!(col.events.len(), 2);
            assert_eq!(col.events[1].session_id, "new");
            assert_eq!(col.events[1].input_tokens, 30);
            assert_eq!(col.stats.ignored_token_usage_record, 1);
            assert_eq!(col.stats.skipped_zero_usage, 1);
        }

        #[test]
        fn later_record_replaces_pending_record() {
            let col = collect_lines(&[
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                record(10, 1, 0, 0),
                record(20, 2, 0, 0),
                zero(),
            ]);
            assert_eq!(col.events.len(), 1);
            assert_eq!(col.events[0].input_tokens, 20);
            assert_eq!(col.stats.ignored_token_usage_record, 1);
        }

        #[test]
        fn zero_or_malformed_record_is_ignored() {
            let valid: serde_json::Value = serde_json::from_str(&record(10, 1, 0, 0)).unwrap();
            let mut malformed = Vec::new();
            for value in [
                serde_json::json!(-1),
                serde_json::json!("10"),
                serde_json::json!(null),
            ] {
                let mut line = valid.clone();
                line["payload"]["usage"]["input_tokens"] = value;
                malformed.push(line);
            }
            for value in [None, Some(serde_json::json!("invalid"))] {
                let mut line = valid.clone();
                line.as_object_mut().unwrap().remove("timestamp");
                if let Some(value) = value {
                    line["timestamp"] = value;
                }
                malformed.push(line);
            }
            for line in malformed {
                let col = collect_lines(&[
                    meta("s", "C:/w"),
                    ctx("m", "C:/w"),
                    record(99, 9, 0, 0),
                    line.to_string(),
                    zero(),
                ]);
                assert!(col.events.is_empty());
                assert_eq!(col.stats.bad_lines, 1);
                assert_eq!(col.stats.ignored_token_usage_record, 2);
            }
            let col = collect_lines(&[
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                record(0, 0, 0, 0),
                zero(),
            ]);
            assert!(col.events.is_empty());
            assert_eq!(col.stats.bad_lines, 0);
            assert_eq!(col.stats.ignored_token_usage_record, 1);
        }

        #[test]
        fn compaction_record_subset_violation_is_bad_line() {
            for (input, output, cached, cw) in [
                (100, 1, 90, 11),
                (u64::MAX, 0, u64::MAX, 1),
                (u64::MAX, 1, 0, 0),
            ] {
                let col = collect_lines(&[
                    meta("s", "C:/w"),
                    ctx("m", "C:/w"),
                    record(input, output, cached, cw),
                    zero(),
                ]);
                assert!(col.events.is_empty());
                assert_eq!(col.stats.bad_lines, 1);
                assert_eq!(col.stats.ignored_token_usage_record, 1);
            }
        }

        #[test]
        fn forked_child_replay_of_compaction_is_deduped() {
            let lines = [
                meta("root-session", "C:/w"),
                ctx("m", "C:/w"),
                record(26705, 491, 0, 0),
                zero(),
            ];
            let mut events = Vec::new();
            for path in ["parent.jsonl", "child.jsonl"] {
                ingest_text(
                    &lines.join("\n"),
                    path,
                    &mut CollectStats::default(),
                    &mut events,
                );
            }
            assert_eq!(events.len(), 2);
            let deduped = crate::dedupe::dedupe_events(events);
            assert_eq!(deduped.events.len(), 1);
            assert_eq!(deduped.dropped, vec![(AgentKind::Codex, 1)]);
        }

        #[test]
        fn thread_settings_model_applies_to_following_usage() {
            let col = collect_lines(&[
                meta("s", "C:/w"),
                ctx("a", "C:/w"),
                settings(serde_json::json!("b")),
                tc(TS, 10, 1, 0, 0, 11),
                ctx("c", "C:/w"),
                tc(TS, 20, 2, 0, 0, 22),
            ]);
            assert_eq!(
                col.events
                    .iter()
                    .map(|e| e.model.as_str())
                    .collect::<Vec<_>>(),
                ["b", "c"]
            );
        }

        #[test]
        fn thread_settings_model_without_prior_turn_context() {
            let col = collect_lines(&[
                meta("s", "C:/w"),
                settings(serde_json::json!("b")),
                tc(TS, 10, 1, 0, 0, 11),
                meta("new", "C:/w"),
                tc(TS, 20, 2, 0, 0, 22),
            ]);
            assert_eq!(col.events.len(), 1);
            assert_eq!(col.events[0].model, "b");
            assert_eq!(
                col.stats.skipped_no_model, 1,
                "只有 session_meta 重置后缺少模型的请求被跳过"
            );
        }

        #[test]
        fn thread_settings_invalid_model_is_ignored() {
            let mut lines = vec![
                meta("s", "C:/w"),
                ctx("a", "C:/w"),
                settings(serde_json::json!("b")),
            ];
            for invalid in [
                serde_json::json!({}),
                serde_json::json!({"thread_settings":{}}),
                serde_json::json!({"thread_settings":{"model":123}}),
                serde_json::json!({"thread_settings":{"model":"  \t"}}),
                serde_json::json!({"thread_settings":false}),
            ] {
                let mut payload = invalid;
                payload["type"] = serde_json::json!("thread_settings_applied");
                lines.push(serde_json::json!({"type":"event_msg","payload":payload}).to_string());
                lines.push(tc(TS, 10, 1, 0, 0, 11));
            }
            lines.push(ctx("   ", "C:/w"));
            lines.push(tc(TS, 20, 2, 0, 0, 22));
            let col = collect_lines(&lines);
            assert_eq!(col.events.len(), 6);
            assert!(col.events.iter().all(|e| e.model == "b"));
            assert_eq!(col.stats.bad_lines, 0);
        }

        #[test]
        fn missing_or_malformed_next_usage_consumes_pending_record() {
            for last in [
                serde_json::json!(null),
                serde_json::json!({"input_tokens":"invalid"}),
            ] {
                let col = collect_lines(&[
                    meta("s", "C:/w"), ctx("m", "C:/w"), record(10, 1, 0, 0),
                    serde_json::json!({"type":"event_msg","timestamp":TS,"payload":{"type":"token_count","info":{"last_token_usage":last}}}).to_string(),
                    zero(),
                ]);
                assert!(
                    col.events.is_empty(),
                    "不能越过缺失或非法的下一条 token_count 等待后续零行"
                );
                assert_eq!(col.stats.bad_lines, 1);
                assert_eq!(col.stats.ignored_token_usage_record, 1);
                assert_eq!(col.stats.skipped_zero_usage, 1);
            }
        }

        #[test]
        fn empty_session_meta_also_discards_pending_record_and_context() {
            let col = collect_lines(&[
                meta("s", "C:/w"),
                ctx("m", "C:/w"),
                record(10, 1, 0, 0),
                serde_json::json!({"type":"session_meta","payload":null}).to_string(),
                zero(),
                tc(TS, 20, 2, 0, 0, 22),
            ]);
            assert!(col.events.is_empty());
            assert_eq!(col.stats.ignored_token_usage_record, 1);
            assert_eq!(col.stats.skipped_no_model, 1);
        }

        #[test]
        fn record_without_payload_replaces_pending_and_is_diagnosed() {
            for line in [
                serde_json::json!({"type":"token_usage_record","timestamp":TS}),
                serde_json::json!({"type":"token_usage_record","timestamp":TS,"payload":null}),
            ] {
                let col = collect_lines(&[
                    meta("s", "C:/w"),
                    ctx("m", "C:/w"),
                    record(10, 1, 0, 0),
                    line.to_string(),
                    zero(),
                ]);
                assert!(col.events.is_empty());
                assert_eq!(col.stats.bad_lines, 1);
                assert_eq!(col.stats.ignored_token_usage_record, 2);
                assert_eq!(col.stats.skipped_zero_usage, 1);
            }
        }
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
        // v7：record 带 thread_id+response_id+合法 usage 时**立即出数**（强
        // 响应身份），不再是"忽略"；缺 response_id 的记录保持旧挂起路径。
        let col = collect_lines(&[
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            r#"{"timestamp":"2026-07-17T15:59:00.000Z","type":"token_usage_record","payload":{"session_id":"s","thread_id":"s","turn_id":"t","root_turn_id":"t","response_id":"r","usage":{"input_tokens":9,"output_tokens":9,"total_tokens":18},"turn_token_usage":{},"thread_token_usage":{}}}"#.to_string(),
        ]);
        assert_eq!(col.events.len(), 1, "强记录立即出数");
        assert_eq!(col.events[0].record_id, "r");
        // 缺 response_id：旧行为——挂起无配对，忽略。
        let col = collect_lines(&[
            meta("s", "C:/w"),
            ctx("m", "C:/w"),
            r#"{"timestamp":"2026-07-17T15:59:00.000Z","type":"token_usage_record","payload":{"session_id":"s","thread_id":"t","turn_id":"t","root_turn_id":"t","usage":{"input_tokens":9,"output_tokens":9,"total_tokens":18},"turn_token_usage":{},"thread_token_usage":{}}}"#.to_string(),
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
