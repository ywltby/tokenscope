//! clap 入口与命令装配。main.rs 只做薄壳，数据管线在 report.rs（CLI/GUI 共用）。

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use crate::aggregate::GroupBy;
use crate::model::AgentKind;
use crate::report::{SummaryOptions, summary};

#[derive(Debug, Parser)]
#[command(name = "tokenscope", version, about = "本地 AI agent 使用量统计", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// 模型价格表管理
    Pricing {
        #[command(subcommand)]
        cmd: PricingCmd,
    },
    /// 浏览逐请求明细（时间倒序，可按模型/项目/日过滤）
    Events {
        /// 只看指定 agent（默认全部）
        #[arg(long, value_enum)]
        agent: Option<AgentArg>,
        /// 只看最近 N 个自然日
        #[arg(long)]
        days: Option<u32>,
        /// 按模型精确过滤
        #[arg(long = "model", value_name = "MODEL")]
        model: Option<String>,
        /// 按项目精确过滤
        #[arg(long = "project", value_name = "NAME")]
        project: Option<String>,
        /// 按自然日过滤（YYYY-MM-DD，按解析时区）
        #[arg(long = "day", value_name = "YYYY-MM-DD")]
        day: Option<String>,
        /// 返回条数上限（默认 200，最大 1000）
        #[arg(long, default_value_t = 200)]
        limit: usize,
        /// 输出 JSON
        #[arg(long)]
        json: bool,
        /// 聚合/展示时区：local=本机，或 IANA 名；缺省 Asia/Shanghai
        #[arg(long = "tz", value_name = "TZ")]
        tz: Option<String>,
        #[arg(long = "claude-dir", value_name = "PATH")]
        claude_dir: Option<PathBuf>,
        #[arg(long = "codex-dir", value_name = "PATH")]
        codex_dir: Option<PathBuf>,
        #[arg(long)]
        refresh: bool,
    },
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
        /// 强制全量重解析并重建缓存
        #[arg(long)]
        refresh: bool,
        /// 聚合/展示时区：local=本机，或 IANA 名（如 Asia/Shanghai）；缺省 Asia/Shanghai
        #[arg(long = "tz", value_name = "TZ")]
        tz: Option<String>,
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

#[derive(Debug, Subcommand)]
pub enum PricingCmd {
    /// 从 OpenRouter 同步价格快照到 ~/.tokenscope/pricing-openrouter.json
    Sync,
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Pricing { cmd } => run_pricing(cmd),
        Command::Events {
            agent,
            days,
            model,
            project,
            day,
            limit,
            json,
            tz,
            claude_dir,
            codex_dir,
            refresh,
        } => run_events(EventsArgs {
            agent,
            days,
            model,
            project,
            day,
            limit,
            json,
            tz,
            claude_dir,
            codex_dir,
            refresh,
        }),
        Command::Summary { .. } => run_summary(cli),
    }
}

pub struct EventsArgs {
    pub agent: Option<AgentArg>,
    pub days: Option<u32>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub day: Option<String>,
    pub limit: usize,
    pub json: bool,
    pub tz: Option<String>,
    pub claude_dir: Option<PathBuf>,
    pub codex_dir: Option<PathBuf>,
    pub refresh: bool,
}

fn run_events(a: EventsArgs) -> anyhow::Result<()> {
    let opts = crate::report::SummaryOptions {
        by: GroupBy::Day,
        agent: a.agent.map(|x| x.kind()),
        days: a.days,
        claude_dir: a.claude_dir,
        codex_dir: a.codex_dir,
        refresh: a.refresh,
        tz: a.tz,
        ..Default::default()
    };
    let filter = crate::report::EventFilter {
        model: a.model,
        project: a.project,
        day: a.day,
        limit: Some(a.limit),
    };
    let list = crate::report::list_events(&opts, &filter)?;
    if a.json {
        println!("{}", serde_json::to_string_pretty(&list)?);
    } else {
        println!("{}", crate::render::events_table(&list));
    }
    Ok(())
}

fn run_pricing(cmd: PricingCmd) -> anyhow::Result<()> {
    match cmd {
        PricingCmd::Sync => {
            let path = crate::report::openrouter_file_path(None);
            let report = crate::openrouter::sync(&path)?;
            println!(
                "已同步 {} 个模型价格 → {}（{}）",
                report.count, report.path, report.synced_at
            );
            Ok(())
        }
    }
}

fn run_summary(cli: Cli) -> anyhow::Result<()> {
    let Command::Summary {
        by,
        agent,
        json,
        days,
        claude_dir,
        codex_dir,
        refresh,
        tz,
    } = cli.command
    else {
        unreachable!("run_summary 只接收 summary 子命令");
    };
    let opts = SummaryOptions {
        by: by.into(),
        agent: agent.map(|a| a.kind()),
        days,
        claude_dir,
        codex_dir,
        cache_dir: None,
        pricing_path: None,
        openrouter_path: None,
        refresh,
        tz,
    };
    let report = summary(&opts)?;
    for w in &report.warnings {
        eprintln!("[warn] {w}");
    }
    let out = if json {
        report.to_json()?
    } else {
        report.to_table()
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
        } = &cli.command
        else {
            unreachable!("应解析出 summary 子命令");
        };
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
            "--tz",
            "UTC",
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
            refresh,
            tz,
        } = &cli.command
        else {
            unreachable!("应解析出 summary 子命令");
        };
        assert_eq!(*by, ByArg::Model);
        assert_eq!(tz.as_deref(), Some("UTC"));
        assert!(!refresh);
        assert_eq!(*agent, Some(AgentArg::Codex));
        assert!(json);
        assert_eq!(*days, Some(7));
        assert_eq!(claude_dir.as_deref(), Some(std::path::Path::new("/tmp/x")));
        assert_eq!(codex_dir.as_deref(), Some(std::path::Path::new("/tmp/y")));
    }
}
