//! WorkBuddy 适配器（国内版 `~/.workbuddy` 与国际版 WorkBuddy AI
//! `~/.workbuddy-ai` 共用同一实现，以 [`AgentKind`] 区分实例）。
//!
//! 口径（docs/research 的两份核验 + 计划 WB03）：
//! - 只读 `projects/` 下的 `*.jsonl` 转写（`<slug>/<sessionId>.jsonl` 与
//!   `<slug>/<sessionId>/subagents/agent-*.jsonl`）；**不打开** `workbuddy.db`，
//!   也不读 `traces/`、`file-history/`、`models.json` 等其他目录；
//! - 一次 LLM 调用 = 一行 = 一个 `providerData.messageId`（跨文件全局唯一）；
//!   只有同时带 `providerData.rawUsage` 与 `providerData.messageId` 的行才
//!   产生事件，三处用量容器（`rawUsage` / `usage` / `message.usage`）只取
//!   `rawUsage` 一处，从不相加；
//! - `prompt_tokens` 含缓存命中（OpenAI 语义）：`input = prompt − cached`，
//!   `cache_read = cached`；缓存真值只在 `prompt_tokens_details.cached_tokens`，
//!   缺失回退 `prompt_cache_hit_tokens`；同名键 `cache_read_input_tokens` 与
//!   顶层 `cached_tokens` 是死字段，一律不用；
//! - `completion_tokens` 已包含推理 token，不另加 `reasoning_tokens`；
//! - 失败调用（`status: incomplete`）不带用量与 `messageId`，不补造请求；
//!   没有模型的用量行计 `skipped_no_model`；
//! - 模型取 `providerData.model`（渠道前缀出现在 `requestModelId`），缺失时
//!   回退 `requestModelId`，原样保存不裁剪；
//! - `timestamp` 是 Unix 毫秒（UTC），存储层转换一次；
//! - 项目身份：按会话维护项目根（进入子目录仍归当前根，越出当前根才切换），
//!   与 Claude 同款规则；`session_initial_cwd` 取文件中第一条带 `cwd` 的行。
//!   子 agent 文件的 `sessionId` 独立于父会话，按文件自身解析。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use jiff::Timestamp;
use serde::Deserialize;

use super::project_path::{self, ProjectRootTracker};
use super::{CollectStats, FileParse, Source, read_text};
use crate::model::{AgentKind, UsageEvent};

pub struct WorkBuddySource {
    root: PathBuf,
    agent: AgentKind,
}

impl WorkBuddySource {
    /// 单根来源：`root` 即该应用的 `projects/` 目录。
    ///
    /// `agent` 必须是两个 WorkBuddy 应用之一；Claude/Codex 有各自的适配器。
    pub fn new(root: impl Into<PathBuf>, agent: AgentKind) -> Self {
        debug_assert!(
            matches!(agent, AgentKind::WorkBuddy | AgentKind::WorkBuddyAi),
            "WorkBuddySource 只接受两个 WorkBuddy 应用"
        );
        Self {
            root: root.into(),
            agent,
        }
    }

    /// 默认根：`~/.workbuddy/projects`（国内版）或 `~/.workbuddy-ai/projects`
    /// （国际版）。RC10：验收模式一律用隔离根下的来源目录，绝不回退真实目录。
    pub fn default_root(agent: AgentKind) -> Result<PathBuf> {
        if let Some(dir) = crate::acceptance::source_dir_override(agent) {
            return Ok(dir.join("projects"));
        }
        let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
        Ok(match agent {
            AgentKind::WorkBuddy => home.join(".workbuddy").join("projects"),
            AgentKind::WorkBuddyAi => home.join(".workbuddy-ai").join("projects"),
            _ => anyhow::bail!("应用 {} 没有 WorkBuddy 默认根", agent.as_str()),
        })
    }

    /// 文件身份兜底：相对根的父目录路径（真实布局 `projects/<slug>/` 下即
    /// slug 本身）；WorkBuddy 没有 slug → 路径映射，保持 slug 身份。
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

impl Source for WorkBuddySource {
    fn agent(&self) -> AgentKind {
        self.agent
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        let mut ledger = super::DirVisitLedger::default();
        self.discover_with_ledger(&mut ledger)
    }

    fn discover_with_ledger(
        &self,
        ledger: &mut super::DirVisitLedger,
    ) -> (Vec<PathBuf>, Vec<String>) {
        // 根目录缺失由调用方统一告警（"目录不存在"），不在此重复记异常。
        // 附属物（`.file-rollback.ndjson`、`.meta.json`、`tool-results/*.txt`）
        // 都不是 `.jsonl` 扩展，共享的守卫遍历天然不收。
        if !self.root.is_dir() {
            return (Vec::new(), Vec::new());
        }
        let mut out = Vec::new();
        let mut errors = Vec::new();
        super::walk_jsonl_guarded(&self.root, self.agent, ledger, &mut out, &mut errors);
        out.sort();
        (out, errors)
    }

    fn parse_file(&self, path: &Path) -> FileParse {
        let mut stats = CollectStats::default();
        let mut events = Vec::new();
        let file_project = self.slug_of(path);
        let source_path = path.display().to_string();
        match read_text(path) {
            Ok(text) => ingest_text(
                &text,
                &source_path,
                &file_project,
                self.agent,
                &mut stats,
                &mut events,
            ),
            // 读取失败（B4）：计 io_errors、返回空产物——报告层据此跳过
            // 成功缓存并告警，不与"坏行"（内容问题）混计。
            Err(_) => stats.io_errors += 1,
        }
        stats.events = events.len() as u64;
        FileParse { stats, events }
    }
}

/// 单文件扫描状态：按 sessionId 维护项目根状态机与会话初始目录。
/// 子 agent 文件是独立文件、独立 sessionId，状态机按行内 `sessionId` 分组，
/// 与 Claude 的会话分组同构（WorkBuddy 无 sidechain 上下文）。
#[derive(Default)]
struct WorkBuddyScan {
    trackers: HashMap<String, ProjectRootTracker>,
    initials: HashMap<String, String>,
}

/// 宽松顶层：按「字段是否存在」取值，不假设固定 schema（转写字段集因行类型
/// 而异）；只关心身份上下文与 providerData，其余字段不进类型。
#[derive(Default, Deserialize)]
struct WorkBuddyLine {
    #[serde(rename = "sessionId", default)]
    session_id: String,
    #[serde(default)]
    cwd: Option<serde_json::Value>,
    /// Unix 毫秒（UTC）；整数由 usage 分支严格校验，其他行不校验。
    #[serde(default)]
    timestamp: Option<serde_json::Value>,
    #[serde(rename = "providerData", default)]
    provider_data: Option<ProviderData>,
}

#[derive(Default, Deserialize)]
struct ProviderData {
    #[serde(rename = "messageId", default)]
    message_id: Option<String>,
    #[serde(default)]
    model: Option<serde_json::Value>,
    #[serde(rename = "requestModelId", default)]
    request_model_id: Option<serde_json::Value>,
    /// 唯一采信的用量容器；`usage` 与 `message.usage` 是同一数值的副本，
    /// 读取它们只会引入重复计费风险，一律不用。
    #[serde(rename = "rawUsage", default)]
    raw_usage: Option<RawUsage>,
}

#[derive(Default, Deserialize)]
struct RawUsage {
    #[serde(rename = "prompt_tokens", default)]
    prompt_tokens: Option<serde_json::Value>,
    #[serde(rename = "completion_tokens", default)]
    completion_tokens: Option<serde_json::Value>,
    #[serde(rename = "prompt_tokens_details", default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
    /// 旧形态回退键（与 details.cached_tokens 恒等）
    #[serde(rename = "prompt_cache_hit_tokens", default)]
    prompt_cache_hit_tokens: Option<serde_json::Value>,
    /// cache_write 桶；缺失按 0（本机实测恒 0）
    #[serde(rename = "cache_creation_input_tokens", default)]
    cache_creation_input_tokens: Option<serde_json::Value>,
}

#[derive(Default, Deserialize)]
struct PromptTokensDetails {
    #[serde(rename = "cached_tokens", default)]
    cached_tokens: Option<serde_json::Value>,
}

fn ingest_text(
    text: &str,
    source_path: &str,
    file_project: &str,
    agent: AgentKind,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
) {
    let mut scan = WorkBuddyScan::default();
    for (line_no, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        stats.lines_seen += 1;
        ingest_line(
            line,
            line_no as u64,
            source_path,
            file_project,
            agent,
            stats,
            events,
            &mut scan,
        );
    }
}

/// 非负整数（bool 不是整数；浮点、字符串、负数、超 u64 都拒绝）。
fn as_u64(v: Option<&serde_json::Value>) -> Option<u64> {
    match v {
        Some(serde_json::Value::Number(n)) => n.as_u64(),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)] // 解析上下文沿 Claude 适配器同构拆分
fn ingest_line(
    line: &str,
    line_no: u64,
    source_path: &str,
    file_project: &str,
    agent: AgentKind,
    stats: &mut CollectStats,
    events: &mut Vec<UsageEvent>,
    scan: &mut WorkBuddyScan,
) {
    let rec: WorkBuddyLine = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };
    // 身份上下文采集——所有行类型都提供（首行可能是 message 或 session-meta）。
    // 归一化失败（相对路径、类型异常、空值）不写入、不影响后续行，也不计坏行。
    if let Some(key) = rec
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
    // 事件门槛（不变量 2）：同时带 rawUsage 与 messageId；其余行（失败调用、
    // 压缩摘要、file-history-snapshot、普通 user 行）到此为止，不补造请求。
    let Some(provider) = rec.provider_data.as_ref() else {
        return;
    };
    let Some(raw_usage) = provider.raw_usage.as_ref() else {
        return;
    };
    let Some(message_id) = provider.message_id.as_deref().filter(|m| !m.is_empty()) else {
        return;
    };

    // 模型：providerData.model 为准（渠道前缀在 requestModelId），缺失回退；
    // 两处都没有或为空 → 计 skipped_no_model（不产出无法归属模型的事件）。
    let model = ["model", "requestModelId"]
        .iter()
        .find_map(|key| {
            let v = if *key == "model" {
                &provider.model
            } else {
                &provider.request_model_id
            };
            v.as_ref()
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|m| !m.is_empty())
        })
        .map(str::to_string);
    let Some(model) = model else {
        stats.skipped_no_model += 1;
        return;
    };

    // 时间：Unix 毫秒整数；缺失或非法按坏行（事件没有可信时间不入库）。
    let Some(ms) = as_u64(rec.timestamp.as_ref()) else {
        stats.bad_lines += 1;
        return;
    };
    // u64 → i64 受检转换：`as i64` 会把超过 i64::MAX 的值回绕成负数，
    // 生成 1969 年的伪事件；越界计坏行（i64 范围内但超出 jiff 表示的
    // 值由 from_millisecond 的 Err 分支处理）。
    let ts = match i64::try_from(ms)
        .map_err(|_| ())
        .and_then(|v| Timestamp::from_millisecond(v).map_err(|_| ()))
    {
        Ok(t) => t,
        Err(_) => {
            stats.bad_lines += 1;
            return;
        }
    };

    // 四桶（不变量 3，全程受检）：cached 取 details.cached_tokens，缺失回退
    // prompt_cache_hit_tokens，再缺失按 0；input = prompt − cached；输出不叠
    // 加 reasoning。字段**存在但非法**（负数、浮点、字符串、布尔）与缺失
    // 不同——计坏行跳过，不静默按 0、也不借回退键混过。
    let Some(prompt) = as_u64(raw_usage.prompt_tokens.as_ref()) else {
        stats.bad_lines += 1;
        return;
    };
    let Some(output) = as_u64(raw_usage.completion_tokens.as_ref()) else {
        stats.bad_lines += 1;
        return;
    };
    let details_cached = raw_usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|d| d.cached_tokens.as_ref());
    let cached = match details_cached {
        Some(v) => match as_u64(Some(v)) {
            Some(n) => n,
            None => {
                stats.bad_lines += 1;
                return;
            }
        },
        None => match raw_usage.prompt_cache_hit_tokens.as_ref() {
            Some(v) => match as_u64(Some(v)) {
                Some(n) => n,
                None => {
                    stats.bad_lines += 1;
                    return;
                }
            },
            None => 0,
        },
    };
    let cache_write = match raw_usage.cache_creation_input_tokens.as_ref() {
        Some(v) => match as_u64(Some(v)) {
            Some(n) => n,
            None => {
                stats.bad_lines += 1;
                return;
            }
        },
        None => 0,
    };
    let Some(input) = prompt.checked_sub(cached) else {
        // cached > prompt：未知字段组合，不猜测语义
        stats.bad_lines += 1;
        return;
    };
    let event = UsageEvent {
        ts,
        agent,
        model,
        session_id: rec.session_id.clone(),
        // 项目归属：会话项目根；完全无可信 cwd 的会话回落文件身份（slug）。
        project: scan
            .trackers
            .get(&rec.session_id)
            .and_then(ProjectRootTracker::root)
            .map(str::to_string)
            .unwrap_or_else(|| file_project.to_string()),
        session_initial_cwd: scan.initials.get(&rec.session_id).cloned(),
        event_cwd: scan
            .trackers
            .get(&rec.session_id)
            .and_then(ProjectRootTracker::cwd)
            .map(str::to_string),
        record_id: message_id.to_string(),
        line: line_no,
        source_path: source_path.to_string(),
        input_tokens: input,
        output_tokens: output,
        cache_write_tokens: cache_write,
        cache_read_tokens: cached,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    // SF08：source→model 公共边界（与既有适配器同一路径）防御性兜底。
    if event.validate_buckets().is_err() {
        stats.bad_lines += 1;
        return;
    }
    events.push(event);
}
