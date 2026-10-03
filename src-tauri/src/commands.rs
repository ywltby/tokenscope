//! Tauri commands：参数校验 + 调用 report 管线，零业务逻辑。

use serde::Serialize;
use tauri_plugin_opener::OpenerExt;
use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::openrouter;
use tokenscope::pricing::Pricing;
use tokenscope::report::{
    CacheInfo, EventFilter, EventList, SourceStatus, SummaryOptions, SummaryReport,
    cache_stats as cache_stats_impl, list_events as list_events_impl, openrouter_file_path,
    pricing_file_path, rebuild_cache as rebuild_cache_impl, source_status as source_status_impl,
    summary,
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
    tz: Option<String>,
) -> Result<SummaryReport, String> {
    let opts = SummaryOptions {
        by: parse_by(&by)?,
        agent: parse_agent(agent.as_deref())?,
        days,
        claude_dir: None,
        codex_dir: None,
        tz,
        ..Default::default()
    };
    summary(&opts).map_err(|e| e.to_string())
}

/// 逐请求明细（M7）：与 summary 共用采集与去重路径。
#[tauri::command]
pub fn list_events(
    agent: Option<String>,
    days: Option<u32>,
    model: Option<String>,
    project: Option<String>,
    day: Option<String>,
    limit: Option<usize>,
    tz: Option<String>,
) -> Result<EventList, String> {
    let opts = SummaryOptions {
        by: GroupBy::Day,
        agent: parse_agent(agent.as_deref())?,
        days,
        tz,
        ..Default::default()
    };
    let filter = EventFilter {
        model,
        project,
        day,
        limit,
    };
    list_events_impl(&opts, &filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn source_status() -> Result<Vec<SourceStatus>, String> {
    source_status_impl(None, None).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cache_stats() -> Result<CacheInfo, String> {
    cache_stats_impl(None).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn refresh_cache() -> Result<CacheInfo, String> {
    rebuild_cache_impl(None).map_err(|e| e.to_string())
}

/// 设置页价格表视图：完整条目 + 内置/外置路径与同步状态 + 解析警告。
#[derive(Serialize)]
pub struct PricingView {
    pub path: String,
    pub openrouter_path: String,
    pub synced_at: Option<String>,
    pub openrouter_count: usize,
    pub external_count: usize,
    pub entries: Vec<tokenscope::pricing::PricingEntry>,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub fn pricing_entries() -> Result<PricingView, String> {
    let path = pricing_file_path(None);
    let snapshot = openrouter_file_path(None);
    let (pricing, warnings) = Pricing::load(Some(&path), Some(&snapshot));
    let synced_at = tokenscope::openrouter::load_snapshot(&snapshot)
        .ok()
        .flatten()
        .map(|s| s.synced_at);
    Ok(PricingView {
        path: path.display().to_string(),
        openrouter_path: snapshot.display().to_string(),
        synced_at,
        openrouter_count: pricing.openrouter_count(),
        external_count: pricing.external_count(),
        entries: pricing.entries(),
        warnings,
    })
}

/// 同步 OpenRouter 价格快照（网络操作，阻塞线程池执行）。
#[tauri::command]
pub async fn sync_pricing_openrouter() -> Result<tokenscope::openrouter::SyncReport, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let path = openrouter_file_path(None);
        openrouter::sync(&path).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 打开（必要时先创建模板）外置价格文件；返回实际路径。
#[tauri::command]
pub fn open_pricing_file(app: tauri::AppHandle) -> Result<String, String> {
    let path = pricing_file_path(None);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, tokenscope::pricing::PRICING_TEMPLATE).map_err(|e| e.to_string())?;
    }
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
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
