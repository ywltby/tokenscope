//! CLI / GUI 共用的汇总管线：组装源 → 采集 → 过滤 → 聚合。
//! 两端只做参数转换与渲染，数字永远同源（M3 计划任务 1）。

use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use crate::aggregate::{GroupBy, aggregate, filter_days};
use crate::model::{AgentKind, UsageEvent};
use crate::pricing::Pricing;
use crate::render;
use crate::source::claude::ClaudeSource;
use crate::source::codex::CodexSource;
use crate::source::{CollectStats, Source};

#[derive(Debug, Default, Clone)]
pub struct SummaryOptions {
    pub by: GroupBy,
    pub agent: Option<AgentKind>,
    pub days: Option<u32>,
    pub claude_dir: Option<PathBuf>,
    pub codex_dir: Option<PathBuf>,
}

/// 单个来源的采集统计（GUI 表格脚注与 JSON 共用）。
#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub agent: AgentKind,
    pub stats: CollectStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryReport {
    pub by: &'static str,
    pub groups: Vec<crate::aggregate::Group>,
    pub totals: crate::aggregate::Group,
    pub sources: Vec<SourceReport>,
    pub warnings: Vec<String>,
    pub generated_at: String,
}

impl SummaryReport {
    /// 终端表格（CLI）。
    pub fn to_table(&self) -> String {
        render::table(self)
    }

    /// 机器可读 JSON（CLI --json 与 Tauri summarize 共用）。
    pub fn to_json(&self) -> Result<String> {
        render::json::to_json(self)
    }
}

pub fn summary(opts: &SummaryOptions) -> Result<SummaryReport> {
    let want = opts.agent;
    let mut cols: Vec<(AgentKind, CollectStats)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut events: Vec<UsageEvent> = Vec::new();

    if want.is_none_or(|k| k == AgentKind::ClaudeCode) {
        let root = match &opts.claude_dir {
            Some(p) => p.clone(),
            None => ClaudeSource::default_root()?,
        };
        let col = ClaudeSource::new(root).collect()?;
        warnings.extend(col.warnings.iter().cloned());
        events.extend(col.events.iter().cloned());
        cols.push((AgentKind::ClaudeCode, col.stats));
    }
    if want.is_none_or(|k| k == AgentKind::Codex) {
        let root = match &opts.codex_dir {
            Some(p) => p.clone(),
            None => CodexSource::default_root()?,
        };
        let col = CodexSource::new(root).collect()?;
        warnings.extend(col.warnings.iter().cloned());
        events.extend(col.events.iter().cloned());
        cols.push((AgentKind::Codex, col.stats));
    }

    let tz = crate::aggregate::local_tz();
    let events = match opts.days {
        Some(n) => filter_days(events, &tz, n),
        None => events,
    };
    let agg = aggregate(&events, opts.by, &tz, &Pricing);
    Ok(SummaryReport {
        by: agg.by,
        groups: agg.groups,
        totals: agg.totals,
        sources: cols
            .into_iter()
            .map(|(agent, stats)| SourceReport { agent, stats })
            .collect(),
        warnings,
        generated_at: jiff::Zoned::now().with_time_zone(tz).to_string(),
    })
}

/// 各 agent 来源状态（GUI 首页提示目录缺失用；只读统计 jsonl 数量）。
#[derive(Debug, Serialize)]
pub struct SourceStatus {
    pub agent: AgentKind,
    pub dir: String,
    pub exists: bool,
    pub files: u64,
}

pub fn source_status(
    claude_dir: Option<PathBuf>,
    codex_dir: Option<PathBuf>,
) -> Result<Vec<SourceStatus>> {
    use crate::source::claude as claude_mod;
    use crate::source::codex as codex_mod;

    let mut out = Vec::new();
    let root = match claude_dir {
        Some(p) => p,
        None => claude_mod::ClaudeSource::default_root()?,
    };
    out.push(SourceStatus {
        agent: AgentKind::ClaudeCode,
        dir: root.display().to_string(),
        exists: root.is_dir(),
        files: count_jsonl(&root),
    });
    let root = match codex_dir {
        Some(p) => p,
        None => codex_mod::CodexSource::default_root()?,
    };
    out.push(SourceStatus {
        agent: AgentKind::Codex,
        dir: root.display().to_string(),
        exists: root.is_dir(),
        files: count_jsonl(&root),
    });
    Ok(out)
}

fn count_jsonl(root: &std::path::Path) -> u64 {
    let mut n = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if crate::source::is_jsonl(&p) {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::GroupBy;

    fn fixture(agent: &str, p: &str) -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(agent)
            .join(p)
    }

    #[test]
    fn test_report_pipeline_both_agents() {
        let opts = SummaryOptions {
            by: GroupBy::Day,
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            ..Default::default()
        };
        let r = summary(&opts).unwrap();
        assert_eq!(r.by, "day");
        assert_eq!(r.sources.len(), 2);
        assert_eq!(r.totals.requests, 7); // claude 3 + codex 4
        assert_eq!(r.groups.len(), 3); // 07-17、07-18、合计
        // 多 agent 时分组带 agents 字段数据
        assert_eq!(r.groups[0].agents, ["claude-code", "codex"]);
        assert!(r.generated_at.contains('+'), "generated_at 带时区偏移");
        // JSON 序列化可用且含逐源统计
        let j = r.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["sources"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_report_pipeline_agent_filter() {
        let opts = SummaryOptions {
            by: GroupBy::Day,
            agent: Some(AgentKind::ClaudeCode),
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            ..Default::default()
        };
        let r = summary(&opts).unwrap();
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.sources[0].agent, AgentKind::ClaudeCode);
        assert_eq!(r.totals.requests, 3);
    }

    #[test]
    fn test_report_pipeline_missing_dir_warns() {
        let opts = SummaryOptions {
            by: GroupBy::Day,
            agent: Some(AgentKind::Codex),
            codex_dir: Some(fixture("codex", "no-such-dir")),
            ..Default::default()
        };
        let r = summary(&opts).unwrap();
        assert_eq!(r.totals.requests, 0);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("不存在"));
    }

    #[test]
    fn test_source_status() {
        let st = source_status(
            Some(fixture("claude", "basic")),
            Some(fixture("codex", "no-such-dir")),
        )
        .unwrap();
        assert_eq!(st.len(), 2);
        assert!(st[0].exists);
        assert_eq!(st[0].files, 2);
        assert!(!st[1].exists);
        assert_eq!(st[1].files, 0);
    }
}
