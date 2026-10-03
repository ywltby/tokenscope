//! clap 入口与命令装配。main.rs 只做薄壳，逻辑在此便于库级测试。

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use jiff::tz::TimeZone;

use crate::aggregate::{GroupBy, aggregate, filter_days};
use crate::pricing::Pricing;
use crate::render;
use crate::source::Source;
use crate::source::claude::ClaudeSource;

#[derive(Debug, Parser)]
#[command(name = "tokenscope", version, about = "本地 AI agent 使用量统计", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 汇总用量（当前支持 Claude Code）
    Summary {
        /// 聚合维度（默认按日）
        #[arg(long, value_enum, default_value_t = ByArg::Day)]
        by: ByArg,
        /// 输出 JSON（机器可读）
        #[arg(long)]
        json: bool,
        /// 只统计最近 N 个自然日（Asia/Shanghai 落日界，含今天）
        #[arg(long)]
        days: Option<u32>,
        /// 覆盖 Claude projects 目录（默认 ~/.claude/projects）
        #[arg(long = "claude-dir", value_name = "PATH")]
        claude_dir: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ByArg {
    Day,
    Model,
    Project,
}

impl From<ByArg> for GroupBy {
    fn from(v: ByArg) -> Self {
        match v {
            ByArg::Day => GroupBy::Day,
            ByArg::Model => GroupBy::Model,
            ByArg::Project => GroupBy::Project,
        }
    }
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let Command::Summary {
        by,
        json,
        days,
        claude_dir,
    } = cli.command;
    let root = match claude_dir {
        Some(p) => p,
        None => ClaudeSource::default_root()?,
    };
    let src = ClaudeSource::new(root);
    let col = src.collect()?;
    for w in &col.warnings {
        eprintln!("[warn] {w}");
    }
    let tz = TimeZone::get("Asia/Shanghai")?;
    let events = match days {
        Some(n) => filter_days(col.events, &tz, n),
        None => col.events,
    };
    let agg = aggregate(&events, by.into(), &tz, &Pricing);
    let generated_at = jiff::Zoned::now().with_time_zone(tz.clone()).to_string();
    let out = if json {
        render::json::to_json(&agg, col.agent, &col.stats, &col.warnings, &generated_at)?
    } else {
        render::table(&agg, &col.stats)
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
            json,
            days,
            claude_dir,
        } = &cli.command;
        assert_eq!(*by, ByArg::Day);
        assert!(!json);
        assert_eq!(*days, None);
        assert!(claude_dir.is_none());
    }

    #[test]
    fn test_summary_flags() {
        let cli = Cli::try_parse_from([
            "tokenscope",
            "summary",
            "--by",
            "model",
            "--json",
            "--days",
            "7",
            "--claude-dir",
            "/tmp/x",
        ])
        .unwrap();
        let Command::Summary {
            by,
            json,
            days,
            claude_dir,
        } = &cli.command;
        assert_eq!(*by, ByArg::Model);
        assert!(json);
        assert_eq!(*days, Some(7));
        assert_eq!(claude_dir.as_deref(), Some(std::path::Path::new("/tmp/x")));
    }
}
