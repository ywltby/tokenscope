//! source 层契约：各 agent 适配器把本地日志采集为归一化的 `Collection`。
//! 适配器必须只读（M1 不变量 1），采集统计随事件一并返回供渲染层展示。

pub mod claude;

use anyhow::Result;
use serde::Serialize;

use crate::model::{AgentKind, UsageEvent};

/// 一次采集的观测计数；坏行 / 去重 / 跳过口径见 M1 plan 不变量 2、3。
#[derive(Debug, Default, Clone, Serialize)]
pub struct CollectStats {
    pub files_scanned: u64,
    pub lines_seen: u64,
    pub bad_lines: u64,
    pub duplicates_dropped: u64,
    pub skipped_sidechain: u64,
    pub skipped_synthetic: u64,
    pub events: u64,
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
