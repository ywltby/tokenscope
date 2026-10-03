//! Tauri commands：参数校验 + 调用 report 管线，零业务逻辑。

use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::report::{
    SourceStatus, SummaryOptions, SummaryReport, source_status as source_status_impl, summary,
};

pub fn parse_by(by: &str) -> Result<GroupBy, String> {
    match by {
        "day" => Ok(GroupBy::Day),
        "model" => Ok(GroupBy::Model),
        "project" => Ok(GroupBy::Project),
        "agent" => Ok(GroupBy::Agent),
        other => Err(format!("未知聚合维度: {other}")),
    }
}

pub fn parse_agent(agent: Option<&str>) -> Result<Option<AgentKind>, String> {
    match agent {
        None | Some("all") => Ok(None),
        Some("claude") => Ok(Some(AgentKind::ClaudeCode)),
        Some("codex") => Ok(Some(AgentKind::Codex)),
        Some(other) => Err(format!("未知 agent: {other}")),
    }
}

#[tauri::command]
pub fn summarize(
    by: String,
    days: Option<u32>,
    agent: Option<String>,
) -> Result<SummaryReport, String> {
    let opts = SummaryOptions {
        by: parse_by(&by)?,
        agent: parse_agent(agent.as_deref())?,
        days,
        claude_dir: None,
        codex_dir: None,
    };
    summary(&opts).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn source_status() -> Result<Vec<SourceStatus>, String> {
    source_status_impl(None, None).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_by_valid() {
        assert!(matches!(parse_by("day"), Ok(GroupBy::Day)));
        assert!(matches!(parse_by("model"), Ok(GroupBy::Model)));
        assert!(matches!(parse_by("project"), Ok(GroupBy::Project)));
        assert!(matches!(parse_by("agent"), Ok(GroupBy::Agent)));
        assert!(parse_by("week").is_err());
    }

    #[test]
    fn test_parse_agent_valid() {
        assert_eq!(parse_agent(None).unwrap(), None);
        assert_eq!(parse_agent(Some("all")).unwrap(), None);
        assert_eq!(
            parse_agent(Some("claude")).unwrap(),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(parse_agent(Some("codex")).unwrap(), Some(AgentKind::Codex));
        assert!(parse_agent(Some("gemini")).is_err());
    }
}
