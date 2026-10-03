//! clap 入口与命令装配。main.rs 只做薄壳，逻辑在此便于库级测试。

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use jiff::tz::TimeZone;

use crate::aggregate::{GroupBy, aggregate, filter_days};
use crate::model::{AgentKind, UsageEvent};
use crate::pricing::Pricing;
use crate::render;
use crate::source::Collection;
use crate::source::Source;
use crate::source::claude::ClaudeSource;
use crate::source::codex::CodexSource;

#[derive(Debug, Parser)]
#[command(name = "tokenscope", version, about = "本地 AI agent 使用量统计", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 汇总用量（当前支持 Claude Code 与 Codex，默认合并全部已装 agent）
    Summary {
        /// 聚合维度（默认按日）
        #[arg(long, value_enum, default_value_t = ByArg::Day)]
        by: ByArg,
        /// 只统计指定 agent（默认全部）
        #[arg(long, value_enum)]
        agent: Option<AgentArg>,
        /// 输出 JSON（机器可读）
        #[arg(long)]
        json: bool,
        /// 只统计最近 N 个自然日（Asia/Shanghai 落日界，含今天）
        #[arg(long)]
        days: Option<u32>,
        /// 覆盖 Claude projects 目录（默认 ~/.claude/projects）
        #[arg(long = "claude-dir", value_name = "PATH")]
        claude_dir: Option<PathBuf>,
        /// 覆盖 Codex sessions 目录（默认 ~/.codex/sessions）
        #[arg(long = "codex-dir", value_name = "PATH")]
        codex_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ByArg {
    Day,
    Model,
    Project,
    Agent,
}

impl From<ByArg> for GroupBy {
    fn from(v: ByArg) -> Self {
        match v {
            ByArg::Day => GroupBy::Day,
            ByArg::Model => GroupBy::Model,
            ByArg::Project => GroupBy::Project,
            ByArg::Agent => GroupBy::Agent,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentArg {
    Claude,
    Codex,
}

impl AgentArg {
    fn kind(self) -> AgentKind {
        match self {
            AgentArg::Claude => AgentKind::ClaudeCode,
            AgentArg::Codex => AgentKind::Codex,
        }
    }
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let Command::Summary {
        by,
        agent,
        json,
        days,
        claude_dir,
        codex_dir,
    } = cli.command;
    let want = agent.map(|a| a.kind());
    let mut cols: Vec<Collection> = Vec::new();
    if want.is_none_or(|k| k == AgentKind::ClaudeCode) {
        let root = match claude_dir {
            Some(p) => p,
            None => ClaudeSource::default_root()?,
        };
        cols.push(ClaudeSource::new(root).collect()?);
    }
    if want.is_none_or(|k| k == AgentKind::Codex) {
        let root = match codex_dir {
            Some(p) => p,
            None => CodexSource::default_root()?,
        };
        cols.push(CodexSource::new(root).collect()?);
    }
    let warnings: Vec<String> = cols
        .iter()
        .flat_map(|c| c.warnings.iter().cloned())
        .collect();
    for w in &warnings {
        eprintln!("[warn] {w}");
    }
    let mut events: Vec<UsageEvent> = Vec::new();
    for c in &cols {
        events.extend(c.events.iter().cloned());
    }
    let tz = TimeZone::get("Asia/Shanghai")?;
    let events = match days {
        Some(n) => filter_days(events, &tz, n),
        None => events,
    };
    let agg = aggregate(&events, by.into(), &tz, &Pricing);
    let generated_at = jiff::Zoned::now().with_time_zone(tz.clone()).to_string();
    let out = if json {
        render::json::to_json(&agg, &cols, &warnings, &generated_at)?
    } else {
        render::table(&agg, &cols)
    };
    println!("{out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_flag() {
        let err = Cli::try_parse_from(["tokenscope", "--version"])
            .expect_err("--version 应触发 DisplayVersion 而非报错");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn test_summary_defaults() {
        let cli = Cli::try_parse_from(["tokenscope", "summary"]).unwrap();
        let Command::Summary {
            by,
            agent,
            json,
            days,
            ..
        } = &cli.command;
        assert_eq!(*by, ByArg::Day);
        assert_eq!(*agent, None);
        assert!(!json);
        assert_eq!(*days, None);
    }

    #[test]
    fn test_summary_flags() {
        let cli = Cli::try_parse_from([
            "tokenscope",
            "summary",
            "--by",
            "model",
            "--agent",
            "codex",
            "--json",
            "--days",
            "7",
            "--claude-dir",
            "/tmp/x",
            "--codex-dir",
            "/tmp/y",
        ])
        .unwrap();
        let Command::Summary {
            by,
            agent,
            json,
            days,
            claude_dir,
            codex_dir,
        } = &cli.command;
        assert_eq!(*by, ByArg::Model);
        assert_eq!(*agent, Some(AgentArg::Codex));
        assert!(json);
        assert_eq!(*days, Some(7));
        assert_eq!(claude_dir.as_deref(), Some(std::path::Path::new("/tmp/x")));
        assert_eq!(codex_dir.as_deref(), Some(std::path::Path::new("/tmp/y")));
    }
}
