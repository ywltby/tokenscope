//! source 层契约：各 agent 适配器把本地日志采集为归一化的 `Collection`。
//! 适配器必须只读（M1 不变量 1），采集统计随事件一并返回供渲染层展示。

pub mod claude;
pub mod codex;

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::model::{AgentKind, UsageEvent};

/// 一次采集的观测计数；坏行 / 去重 / 跳过口径见 M1/M2 plan 不变量。
/// Codex 专属计数器为 0 时不序列化，保证单 agent 报告与 M1 输出一致。
#[derive(Debug, Default, Clone, Serialize)]
pub struct CollectStats {
    pub files_scanned: u64,
    pub lines_seen: u64,
    pub bad_lines: u64,
    pub duplicates_dropped: u64,
    pub skipped_sidechain: u64,
    pub skipped_synthetic: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_zero_usage: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub skipped_no_model: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub ignored_token_usage_record: u64,
    pub events: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

/// jsonl 扩展名判断，供各适配器的发现逻辑共用。
pub(crate) fn is_jsonl(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
}

pub struct Collection {
    pub agent: AgentKind,
    pub events: Vec<UsageEvent>,
    pub stats: CollectStats,
    pub warnings: Vec<String>,
}

pub trait Source {
    fn agent(&self) -> AgentKind;
    fn collect(&self) -> Result<Collection>;
}
