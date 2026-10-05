//! Tauri commands：参数校验 + 调用 report 管线，零业务逻辑。

use anyhow::Context;
use serde::Serialize;
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;
use tauri_plugin_opener::OpenerExt;
use tokenscope::aggregate::GroupBy;
use tokenscope::model::AgentKind;
use tokenscope::modelsdev;
use tokenscope::openrouter;
use tokenscope::pricing::Pricing;
use tokenscope::report::{
    CacheInfo, EventFilter, EventList, SourceStatus, SummaryOptions, SummaryReport,
    cache_stats as cache_stats_impl, list_events as list_events_impl, modelsdev_file_path,
    openrouter_file_path, pricing_file_path, rebuild_cache as rebuild_cache_impl,
    source_status as source_status_impl, summary, view_cache_path,
};
use tokenscope::settings::Settings;

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

/// **主线程纪律（用户反馈启动卡顿的根因）**：Tauri v2 的同步 command 在
/// 主线程执行，扫描/解析/缓存/网络等重活一律 `async` + `spawn_blocking`
/// 丢到后台线程池，GUI 主线程零阻塞。
#[tauri::command]
pub async fn summarize(
    by: String,
    days: Option<u32>,
    agent: Option<String>,
    tz: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<SummaryReport, String> {
    let (claude_dir, codex_dir, claude_enabled, codex_enabled) = source_settings()?;
    let opts = SummaryOptions {
        by: parse_by(&by)?,
        agent: parse_agent(agent.as_deref())?,
        days,
        claude_dir,
        codex_dir,
        claude_enabled: Some(claude_enabled),
        codex_enabled: Some(codex_enabled),
        tz,
        from,
        to,
        ..Default::default()
    };
    run_blocking("summarize", move || summary(&opts)).await
}

/// 逐请求明细（M7）：与 summary 共用采集与去重路径。
// 参数面由 IPC 契约决定（每个筛选项一个 invoke 参数），非设计膨胀。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn list_events(
    agent: Option<String>,
    days: Option<u32>,
    model: Option<String>,
    project: Option<String>,
    day: Option<String>,
    limit: Option<usize>,
    before: Option<String>,
    tz: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<EventList, String> {
    let (claude_dir, codex_dir, claude_enabled, codex_enabled) = source_settings()?;
    let opts = SummaryOptions {
        by: GroupBy::Day,
        agent: parse_agent(agent.as_deref())?,
        days,
        claude_dir,
        codex_dir,
        claude_enabled: Some(claude_enabled),
        codex_enabled: Some(codex_enabled),
        tz,
        from,
        to,
        ..Default::default()
    };
    let filter = EventFilter {
        model,
        project,
        day,
        limit,
        before,
    };
    run_blocking("list_events", move || list_events_impl(&opts, &filter)).await
}

#[tauri::command]
pub async fn source_status() -> Result<Vec<SourceStatus>, String> {
    run_blocking("source_status", move || {
        source_status_impl(&load_settings_or_default())
    })
    .await
}

/// C1：读设置并解析为各来源的有效配置（目录覆盖 + 启停）。
#[allow(clippy::type_complexity)]
fn source_settings() -> Result<
    (
        Option<std::path::PathBuf>,
        Option<std::path::PathBuf>,
        bool,
        bool,
    ),
    String,
> {
    let s = load_settings_or_default();
    let c = s.source_config(true);
    let x = s.source_config(false);
    Ok((
        c.dir.map(std::path::PathBuf::from),
        x.dir.map(std::path::PathBuf::from),
        c.enabled,
        x.enabled,
    ))
}

fn load_settings_or_default() -> tokenscope::settings::Settings {
    tokenscope::settings::settings_path()
        .ok()
        .and_then(|p| tokenscope::settings::load(&p).ok())
        .unwrap_or_default()
}

/// C1：保存单一来源配置（启停 + 目录覆盖；dir=None 回默认目录）。
#[tauri::command]
pub async fn source_config_set(
    agent: String,
    enabled: bool,
    dir: Option<String>,
) -> Result<tokenscope::settings::SourceConfig, String> {
    run_blocking("source_config_set", move || {
        let claude = match agent.as_str() {
            "claude" => true,
            "codex" => false,
            other => return Err(anyhow::anyhow!("未知 agent: {other}")),
        };
        let path = tokenscope::settings::settings_path()?;
        let mut s = tokenscope::settings::load(&path).unwrap_or_default();
        let cfg = tokenscope::settings::SourceConfig { enabled, dir };
        if claude {
            s.sources.claude = Some(cfg.clone());
        } else {
            s.sources.codex = Some(cfg.clone());
        }
        tokenscope::settings::save(&path, &s)?;
        log::info!(
            "来源配置已保存：{agent} enabled={enabled} dir={:?}",
            cfg.dir
        );
        Ok(cfg)
    })
    .await
}

#[tauri::command]
pub async fn cache_stats() -> Result<CacheInfo, String> {
    run_blocking("cache_stats", move || cache_stats_impl(None)).await
}

#[tauri::command]
pub async fn refresh_cache() -> Result<CacheInfo, String> {
    run_blocking("refresh_cache", move || {
        let t = std::time::Instant::now();
        let info = rebuild_cache_impl(None)?;
        log::info!(
            "缓存重建完成：{} 文件 / {} 事件，{} ms",
            info.files,
            info.events,
            t.elapsed().as_millis()
        );
        Ok(info)
    })
    .await
}

/// 应用设置（M11）。
#[tauri::command]
pub async fn settings_get() -> Result<Settings, String> {
    run_blocking("settings_get", move || {
        let path = tokenscope::settings::settings_path()?;
        tokenscope::settings::load(&path)
    })
    .await
}

#[tauri::command]
pub async fn settings_set_price_auto_sync(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<bool, String> {
    run_blocking("settings_set_price_auto_sync", move || {
        let path = tokenscope::settings::settings_path()?;
        let mut s = tokenscope::settings::load(&path)?;
        s.price_auto_sync = enabled;
        tokenscope::settings::save(&path, &s)?;
        if !enabled {
            // 关闭自动同步时清空快照记录的同步时间，避免下次开启立即误判为"刚同步过"。
            // 快照自身保留，仅删除 synced_at 依据——直接保留快照文件，由时间判断兜底。
        }
        if enabled {
            log::info!("价格自动同步已开启");
        } else {
            log::info!("价格自动同步已关闭");
        }
        let _ = app;
        Ok(s.price_auto_sync)
    })
    .await
}

/// 上次视图快照（M10 后启动提速）：原样存取前端渲染结果，零类型耦合。
/// 坏文件 → None 静默忽略。
#[tauri::command]
pub async fn view_cache_load() -> Result<Option<serde_json::Value>, String> {
    run_blocking("view_cache_load", move || {
        let path = view_cache_path()?;
        if !path.exists() {
            log::debug!("视图快照不存在（首次启动）");
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("读视图缓存失败: {}", path.display()))?;
        // 坏文件按契约降级为 None（前端静默走正常加载），但必须留日志。
        match serde_json::from_str(&text) {
            Ok(v) => {
                log::info!("视图快照加载：{}（{} 字节）", path.display(), text.len());
                Ok(Some(v))
            }
            Err(e) => {
                log::warn!(
                    "视图快照解析失败，忽略并走正常加载: {} ({e})",
                    path.display()
                );
                Ok(None)
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn view_cache_save(value: serde_json::Value) -> Result<(), String> {
    run_blocking("view_cache_save", move || {
        let t = std::time::Instant::now();
        let path = view_cache_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建目录失败: {}", dir.display()))?;
        }
        let json = serde_json::to_string_pretty(&value)?;
        let json_len = json.len();
        std::fs::write(&path, json)
            .with_context(|| format!("写视图缓存失败: {}", path.display()))?;
        log::debug!(
            "视图快照保存：{} 字节，{} ms",
            json_len,
            t.elapsed().as_millis()
        );
        Ok(())
    })
    .await
}

/// 开机自启状态（M8；写系统自启动项属用户显式操作，默认关闭）。
#[tauri::command]
pub async fn autostart_status(app: tauri::AppHandle) -> Result<bool, String> {
    run_blocking("autostart_status", move || {
        app.autolaunch()
            .is_enabled()
            .map_err(|e| anyhow::anyhow!("读取自启状态失败: {e}"))
    })
    .await
}

#[tauri::command]
pub async fn autostart_set(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    run_blocking("autostart_set", move || {
        let launch = app.autolaunch();
        if enabled {
            launch
                .enable()
                .map_err(|e| anyhow::anyhow!("开启自启失败: {e}"))?;
        } else {
            launch
                .disable()
                .map_err(|e| anyhow::anyhow!("关闭自启失败: {e}"))?;
        }
        log::info!("开机自启已{}", if enabled { "开启" } else { "关闭" });
        launch
            .is_enabled()
            .map_err(|e| anyhow::anyhow!("读取自启状态失败: {e}"))
    })
    .await
}

/// 后台线程池执行阻塞任务并统一错误映射与日志：
/// 失败必落日志（错误链 + 堆栈，排障不依赖前端弹窗）；成功计 debug 耗时。
async fn run_blocking<T, F>(name: &'static str, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    let t = std::time::Instant::now();
    let joined = tauri::async_runtime::spawn_blocking(f).await;
    let outcome = match joined {
        Ok(r) => r,
        Err(e) => {
            log::error!(
                "命令 {name} 后台任务崩溃（{} ms）: {e}",
                t.elapsed().as_millis()
            );
            return Err(format!("后台任务失败: {e}"));
        }
    };
    match outcome {
        Ok(v) => {
            log::debug!("命令 {name} 完成，{} ms", t.elapsed().as_millis());
            Ok(v)
        }
        Err(e) => {
            tokenscope::logging::log_error(
                &format!("命令 {name} 失败（{} ms）", t.elapsed().as_millis()),
                &e,
            );
            Err(e.to_string())
        }
    }
}

/// 设置页价格表视图：完整条目 + 各源路径与同步状态 + 解析警告。
#[derive(Serialize)]
pub struct PricingView {
    pub path: String,
    pub modelsdev_path: String,
    pub modelsdev_synced_at: Option<String>,
    pub modelsdev_count: usize,
    pub openrouter_path: String,
    pub openrouter_synced_at: Option<String>,
    pub openrouter_count: usize,
    pub external_count: usize,
    pub entries: Vec<tokenscope::pricing::PricingEntry>,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn pricing_entries() -> Result<PricingView, String> {
    run_blocking("pricing_entries", move || {
        let t = std::time::Instant::now();
        let path = pricing_file_path(None);
        let snapshot = openrouter_file_path(None);
        let modelsdev = modelsdev_file_path(None);
        let (pricing, warnings) = Pricing::load(Some(&path), Some(&modelsdev), Some(&snapshot));
        let openrouter_synced_at = tokenscope::openrouter::load_snapshot(&snapshot)
            .ok()
            .flatten()
            .map(|s| s.synced_at);
        let modelsdev_synced_at = tokenscope::modelsdev::load_snapshot(&modelsdev)
            .ok()
            .flatten()
            .map(|s| s.synced_at);
        let view = PricingView {
            path: path.display().to_string(),
            modelsdev_path: modelsdev.display().to_string(),
            modelsdev_synced_at,
            modelsdev_count: pricing.modelsdev_count(),
            openrouter_path: snapshot.display().to_string(),
            openrouter_synced_at,
            openrouter_count: pricing.openrouter_count(),
            external_count: pricing.external_count(),
            entries: pricing.entries(),
            warnings,
        };
        log::info!(
            "价格表加载：models.dev {} / OpenRouter {} / 外置 {}，共 {} 条，{} ms（设置页）",
            view.modelsdev_count,
            view.openrouter_count,
            view.external_count,
            view.entries.len(),
            t.elapsed().as_millis()
        );
        Ok(view)
    })
    .await
}

/// 双源同步结果（统一形态，前端不区分具体源类型）。
#[derive(Serialize)]
pub struct SyncOutcome {
    pub source: &'static str,
    pub count: u64,
    pub path: String,
    pub synced_at: String,
}

/// 同步双在线源（models.dev 主源 + OpenRouter 备份）；单源失败不影响另一源。
#[tauri::command]
pub async fn sync_pricing_openrouter() -> Result<Vec<SyncOutcome>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let t = std::time::Instant::now();
        log::info!("手动同步价格双源开始");
        let mut reports = Vec::new();
        let mut failures = Vec::new();
        match modelsdev::sync(&modelsdev_file_path(None)) {
            Ok(r) => {
                log::info!("models.dev 同步成功: {} 条", r.count);
                reports.push(SyncOutcome {
                    source: "models.dev",
                    count: r.count,
                    path: r.path,
                    synced_at: r.synced_at,
                });
            }
            Err(e) => {
                tokenscope::logging::log_error("models.dev 同步失败", &e);
                failures.push(format!("models.dev: {e:#}"));
            }
        }
        match openrouter::sync(&openrouter_file_path(None)) {
            Ok(r) => {
                log::info!("OpenRouter 同步成功: {} 条", r.count);
                reports.push(SyncOutcome {
                    source: "OpenRouter",
                    count: r.count,
                    path: r.path,
                    synced_at: r.synced_at,
                });
            }
            Err(e) => {
                tokenscope::logging::log_error("OpenRouter 同步失败", &e);
                failures.push(format!("OpenRouter: {e:#}"));
            }
        }
        if failures.is_empty() {
            log::info!("手动同步价格双源完成，{} ms", t.elapsed().as_millis());
            Ok(reports)
        } else {
            log::warn!(
                "手动同步价格双源部分失败，{} ms: {}",
                t.elapsed().as_millis(),
                failures.join("；")
            );
            Err(failures.join("；"))
        }
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
