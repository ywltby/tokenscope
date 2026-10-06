//! Tauri commands：参数校验 + 调用 report 管线，零业务逻辑。

use anyhow::Context;
use serde::Serialize;
use tauri::Manager;
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
use tokenscope::settings::{CloseAction, Settings};

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

pub(crate) fn load_settings_or_default() -> tokenscope::settings::Settings {
    tokenscope::settings::settings_path()
        .ok()
        .and_then(|p| tokenscope::settings::load(&p).ok())
        .unwrap_or_default()
}

// ── 关闭行为三态（关闭确认与配置文件计划）──────────────────

/// 窗口关闭请求的处置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseDecision {
    /// 按设置最小化到托盘（隐藏窗口，进程驻留）。
    Minimize,
    /// 按设置直接退出进程。
    Quit,
    /// 未配置默认动作 → 通知前端弹窗询问。
    Ask,
}

/// 纯函数：由设置解析关闭决策（lib.rs 的 CloseRequested 处理器消费）。
pub fn close_decision_from(s: &Settings) -> CloseDecision {
    match s.close_action {
        Some(CloseAction::Minimize) => CloseDecision::Minimize,
        Some(CloseAction::Quit) => CloseDecision::Quit,
        None => CloseDecision::Ask,
    }
}

/// 设置页「关闭窗口时」：每次询问（None）/ 最小化到托盘 / 直接退出。
#[tauri::command]
pub async fn settings_set_close_action(action: Option<String>) -> Result<Option<String>, String> {
    run_blocking("settings_set_close_action", move || {
        let path = tokenscope::settings::settings_path()?;
        settings_set_close_action_impl(&path, action)
    })
    .await
}

/// 损坏设置直接报错——绝不覆盖用户文件（沿用 Task 7.1 不变量）。
pub(crate) fn settings_set_close_action_impl(
    path: &std::path::Path,
    action: Option<String>,
) -> anyhow::Result<Option<String>> {
    let parsed = match action.as_deref() {
        None => None,
        Some("minimize") => Some(CloseAction::Minimize),
        Some("quit") => Some(CloseAction::Quit),
        Some(other) => return Err(anyhow::anyhow!("未知关闭动作: {other}")),
    };
    let mut s = tokenscope::settings::load(path)?;
    s.close_action = parsed;
    tokenscope::settings::save(path, &s)?;
    log::info!("关闭窗口默认动作已设置: {:?}", parsed);
    Ok(action)
}

/// 关闭确认弹窗的用户决定：remember=true 先持久化默认动作（写盘前重读
/// 最新设置，只改 close_action 一个字段），再隐藏窗口或退出。
#[tauri::command]
pub async fn close_resolve(
    app: tauri::AppHandle,
    minimize: bool,
    remember: bool,
) -> Result<(), String> {
    if remember {
        run_blocking("close_resolve", move || {
            let path = tokenscope::settings::settings_path()?;
            persist_close_action(&path, minimize)
        })
        .await?;
    }
    if minimize {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.hide();
            log::info!(
                "窗口关闭：最小化到托盘{}",
                if remember { "（已记忆）" } else { "" }
            );
        }
    } else {
        log::info!("窗口关闭：用户选择直接退出");
        app.exit(0);
    }
    Ok(())
}

fn persist_close_action(path: &std::path::Path, minimize: bool) -> anyhow::Result<()> {
    let mut s = tokenscope::settings::load(path)?;
    s.close_action = Some(if minimize {
        CloseAction::Minimize
    } else {
        CloseAction::Quit
    });
    tokenscope::settings::save(path, &s)
}

/// 打开（必要时先创建）设置配置文件；返回实际路径。
/// 首建经 ensure_toml：无遗留 → 全字段注释模板；有遗留 json → 先迁移，
/// 避免"打开空模板后旧设置被遮蔽"。
#[tauri::command]
pub async fn open_settings_file(app: tauri::AppHandle) -> Result<String, String> {
    let path = tokenscope::settings::settings_path().map_err(|e| e.to_string())?;
    let path = run_blocking("open_settings_file", move || {
        tokenscope::settings::ensure_toml(&path)?;
        Ok(path)
    })
    .await?;
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    log::info!("已打开设置配置文件: {}", path.display());
    Ok(path.display().to_string())
}

/// C1：保存单一来源配置（启停 + 目录覆盖；dir=None 回默认目录）。
#[tauri::command]
pub async fn source_config_set(
    agent: String,
    enabled: bool,
    dir: Option<String>,
) -> Result<tokenscope::settings::SourceConfig, String> {
    run_blocking("source_config_set", move || {
        let path = tokenscope::settings::settings_path()?;
        source_config_set_impl(&path, &agent, enabled, dir)
    })
    .await
}

/// Task 7.1：损坏设置直接报错——绝不 unwrap_or_default 后覆盖用户文件。
pub(crate) fn source_config_set_impl(
    path: &std::path::Path,
    agent: &str,
    enabled: bool,
    dir: Option<String>,
) -> anyhow::Result<tokenscope::settings::SourceConfig> {
    let claude = match agent {
        "claude" => true,
        "codex" => false,
        other => return Err(anyhow::anyhow!("未知 agent: {other}")),
    };
    let mut s = tokenscope::settings::load(path)?;
    let cfg = tokenscope::settings::SourceConfig {
        enabled,
        dir: dir.clone(),
    };
    // Task 2：保存前按"保存后的全量配置"校验重叠（仅校验两个启用的来源）。
    {
        let other_dir = if claude {
            s.sources.codex.as_ref().filter(|c| c.enabled)
        } else {
            s.sources.claude.as_ref().filter(|c| c.enabled)
        };
        if enabled && let Some(other) = other_dir {
            tokenscope::settings::validate_no_overlap(
                if claude {
                    Some(dir.as_deref().unwrap_or(""))
                } else {
                    other.dir.as_deref()
                },
                if claude {
                    other.dir.as_deref()
                } else {
                    Some(dir.as_deref().unwrap_or(""))
                },
            )
            .map_err(anyhow::Error::msg)?;
        }
    }
    if claude {
        s.sources.claude = Some(cfg.clone());
    } else {
        s.sources.codex = Some(cfg.clone());
    }
    tokenscope::settings::save(path, &s)?;
    log::info!(
        "来源配置已保存：{agent} enabled={enabled} dir={:?}",
        cfg.dir
    );
    Ok(cfg)
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

/// Task 2：定价可用性状态（全局横幅数据源；与 pricing_entries 同路径解析）。
#[tauri::command]
pub async fn pricing_status() -> Result<tokenscope::pricing::PricingStatus, String> {
    run_blocking("pricing_status", move || {
        Ok(tokenscope::pricing::pricing_status(
            Some(&pricing_file_path(None)),
            Some(&modelsdev_file_path(None)),
            Some(&openrouter_file_path(None)),
        ))
    })
    .await
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
/// D4：建模板涉及磁盘 IO，一律后台执行（主线程纪律）。
#[tauri::command]
pub async fn open_pricing_file(app: tauri::AppHandle) -> Result<String, String> {
    let path = pricing_file_path(None);
    let path = run_blocking("open_pricing_file", move || {
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, tokenscope::pricing::PRICING_TEMPLATE)?;
        }
        Ok(path)
    })
    .await?;
    app.opener()
        .open_path(path.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pricing_status_missing_modelsdev_snapshot() {
        // Task 2：无 models.dev 快照 → needs_sync=true（首次启动横幅依据）。
        let dir = std::env::temp_dir().join(format!("tokenscope-ps-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let st = tokenscope::pricing::pricing_status(
            None,
            Some(&dir.join("no-md.json")),
            Some(&dir.join("no-or.json")),
        );
        assert!(!st.modelsdev_available);
        assert!(st.needs_sync);
        assert!(!st.has_any_pricing);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_status_uses_cached_modelsdev_snapshot() {
        // Task 2：本地快照在位 → 离线可用，横幅不出现。
        let dir = std::env::temp_dir().join(format!("tokenscope-ps-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("pricing-modelsdev.json");
        std::fs::write(
            &md,
            r#"{"v":2,"synced_at":"2026-10-05T00:00:00Z","entries":[
                {"id":"prov/x","name":null,"input":1.0,"output":2.0}
            ]}"#,
        )
        .unwrap();
        let st = tokenscope::pricing::pricing_status(None, Some(&md), None);
        assert!(st.modelsdev_available);
        assert_eq!(st.modelsdev_count, 1);
        assert_eq!(
            st.modelsdev_synced_at.as_deref(),
            Some("2026-10-05T00:00:00Z")
        );
        assert!(!st.needs_sync, "离线快照可用时不提示同步");
        assert!(st.has_any_pricing);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_status_invalid_snapshot_is_degraded() {
        // Task 2：快照损坏 → 主源降级 + 警告，needs_sync=true。
        let dir = std::env::temp_dir().join(format!("tokenscope-ps-bad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md = dir.join("pricing-modelsdev.json");
        std::fs::write(&md, "not json").unwrap();
        let st = tokenscope::pricing::pricing_status(None, Some(&md), None);
        assert!(!st.modelsdev_available);
        assert!(st.needs_sync);
        assert!(
            st.warnings.iter().any(|w| w.contains("主源不可用")),
            "损坏快照必须有诊断: {:?}",
            st.warnings
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_parse_by_valid() {
        assert!(matches!(parse_by("day"), Ok(GroupBy::Day)));
        assert!(matches!(parse_by("model"), Ok(GroupBy::Model)));
        assert!(matches!(parse_by("project"), Ok(GroupBy::Project)));
        assert!(matches!(parse_by("agent"), Ok(GroupBy::Agent)));
        assert!(parse_by("week").is_err());
    }

    #[test]
    fn test_source_config_set_overlap_rejected() {
        // Task 2：两个启用的 agent 指向同一目录 → 保存被拒绝。
        let dir = std::env::temp_dir().join(format!("tokenscope-t2-ovl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "price_auto_sync = true\n").unwrap();
        // 先保存 claude 配置
        source_config_set_impl(&path, "claude", true, Some("C:/shared/logs".into())).unwrap();
        // codex 同目录 → 拒绝
        let r = source_config_set_impl(&path, "codex", true, Some("C:/shared/logs".into()));
        assert!(r.is_err(), "重叠目录必须拒绝保存");
        // 文件内容保留（claude 配置仍在，codex 未写入）
        let s = tokenscope::settings::load(&path).unwrap();
        assert!(s.sources.claude.as_ref().unwrap().enabled);
        assert!(s.sources.codex.is_none(), "被拒绝的配置不得写入");
        // 停用的来源不参与重叠校验
        source_config_set_impl(&path, "codex", false, Some("C:/shared/logs".into())).unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_source_config_set_invalid_settings_keeps_file() {
        // Task 7.1：损坏设置直接报错，绝不 unwrap_or_default 后覆盖用户文件。
        let dir = std::env::temp_dir().join(format!("tokenscope-t71-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        let corrupt = r#"{"price_auto_sync": true, "broken""#;
        std::fs::write(&path, corrupt).unwrap();
        let r = source_config_set_impl(&path, "claude", false, None);
        assert!(r.is_err(), "损坏设置必须报错");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            corrupt,
            "用户文件必须原样保留"
        );
        std::fs::remove_dir_all(&dir).ok();
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

    #[test]
    fn test_close_decision_from_settings() {
        // 关闭三态：未配置 = 询问；配置了默认动作 = 直接执行。
        assert!(matches!(
            close_decision_from(&Settings::default()),
            CloseDecision::Ask
        ));
        let s = Settings {
            close_action: Some(CloseAction::Minimize),
            ..Default::default()
        };
        assert!(matches!(close_decision_from(&s), CloseDecision::Minimize));
        let s = Settings {
            close_action: Some(CloseAction::Quit),
            ..Default::default()
        };
        assert!(matches!(close_decision_from(&s), CloseDecision::Quit));
    }

    #[test]
    fn test_settings_set_close_action_impl() {
        let dir = std::env::temp_dir().join(format!("tokenscope-close-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        // minimize → 落盘
        let out = settings_set_close_action_impl(&path, Some("minimize".into())).unwrap();
        assert_eq!(out.as_deref(), Some("minimize"));
        assert_eq!(
            tokenscope::settings::load(&path).unwrap().close_action,
            Some(CloseAction::Minimize)
        );
        // quit → 落盘
        settings_set_close_action_impl(&path, Some("quit".into())).unwrap();
        assert_eq!(
            tokenscope::settings::load(&path).unwrap().close_action,
            Some(CloseAction::Quit)
        );
        // None = 恢复每次询问
        settings_set_close_action_impl(&path, None).unwrap();
        assert_eq!(
            tokenscope::settings::load(&path).unwrap().close_action,
            None
        );
        // 非法值：报错且文件原样保留
        let before = std::fs::read_to_string(&path).unwrap();
        assert!(settings_set_close_action_impl(&path, Some("tray".into())).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_persist_close_action_merges_without_touching_other_fields() {
        // 记忆勾选：只写 close_action，其他字段（price_auto_sync/sources）保留；
        // minimize=true → Minimize，false → Quit。
        let dir =
            std::env::temp_dir().join(format!("tokenscope-close-persist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "price_auto_sync = false\n").unwrap();
        persist_close_action(&path, true).unwrap();
        let s = tokenscope::settings::load(&path).unwrap();
        assert_eq!(s.close_action, Some(CloseAction::Minimize));
        assert!(!s.price_auto_sync, "已有字段必须保留");
        persist_close_action(&path, false).unwrap();
        assert_eq!(
            tokenscope::settings::load(&path).unwrap().close_action,
            Some(CloseAction::Quit)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_ensure_toml_migrates_legacy_or_writes_template() {
        // open_settings_file 首建语义：
        // a) 无遗留 → 写全字段注释模板（解析即默认值）；
        // b) 有遗留 json → load+save 迁移（值保留、json 改名 .bak）；
        // c) toml 已在位 → 原样不动。
        let dir =
            std::env::temp_dir().join(format!("tokenscope-ensure-toml-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        // c) 已存在：不改内容
        std::fs::write(&path, "price_auto_sync = false\n").unwrap();
        tokenscope::settings::ensure_toml(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "price_auto_sync = false\n"
        );
        // a) 空目录 → 模板
        std::fs::remove_file(&path).unwrap();
        tokenscope::settings::ensure_toml(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            tokenscope::settings::SETTINGS_TEMPLATE
        );
        // b) 遗留 json → 迁移
        std::fs::remove_file(&path).unwrap();
        std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": false}"#).unwrap();
        tokenscope::settings::ensure_toml(&path).unwrap();
        assert!(!tokenscope::settings::load(&path).unwrap().price_auto_sync);
        assert!(dir.join("settings.json.bak").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
