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
    cache_stats as cache_stats_impl, modelsdev_file_path, openrouter_file_path, pricing_file_path,
    rebuild_cache as rebuild_cache_impl, source_status as source_status_impl, summary,
    view_cache_path,
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
    let opts = query_opts(&by, agent, days, tz, from, to)?;
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
    run_blocking("list_events", move || {
        let snap = tokenscope::query::begin_query(&opts)?;
        tokenscope::query::query_events(&snap.query_id, &filter)
    })
    .await
}

// ── SF04：查询快照命令（query_begin → query_summary/query_events）──

/// 主筛选参数 → SummaryOptions（query_begin 与旧 summarize/list_events 共用）。
fn query_opts(
    by: &str,
    agent: Option<String>,
    days: Option<u32>,
    tz: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<SummaryOptions, String> {
    let (claude_dir, codex_dir, claude_enabled, codex_enabled) = source_settings()?;
    Ok(SummaryOptions {
        by: parse_by(by)?,
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
    })
}

/// SF04：创建查询会话——冻结一次采集的事件、价格修订、时间基准与来源
/// 身份，返回 query_id 供汇总/明细（含分页）显式绑定。
#[tauri::command]
pub async fn query_begin(
    by: String,
    days: Option<u32>,
    agent: Option<String>,
    tz: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<tokenscope::query::QueryHandle, String> {
    let opts = query_opts(&by, agent, days, tz, from, to)?;
    run_blocking("query_begin", move || {
        tokenscope::query::begin_query_handle(&opts)
    })
    .await
}

/// SF04：从会话快照聚合汇总（不重新采集；generated_at = 冻结 as_of）。
#[tauri::command]
pub async fn query_summary(query_id: String) -> Result<SummaryReport, String> {
    run_blocking("query_summary", move || {
        tokenscope::query::query_summary(&query_id)
    })
    .await
}

/// SF04：从会话快照分页读取明细；游标校验 query_id/指纹/行位置归属。
#[tauri::command]
pub async fn query_events(
    query_id: String,
    model: Option<String>,
    project: Option<String>,
    day: Option<String>,
    limit: Option<usize>,
    before: Option<String>,
) -> Result<EventList, String> {
    let filter = EventFilter {
        model,
        project,
        day,
        limit,
        before,
    };
    run_blocking("query_events", move || {
        tokenscope::query::query_events(&query_id, &filter)
    })
    .await
}

/// SF06：启动诊断（只读）——App 挂载后取一次日志初始化状态，展示
/// 非阻断通知；不触发任何重活。
#[tauri::command]
pub fn startup_diagnostics(
    status: tauri::State<tokenscope::logging::LogInitStatus>,
) -> tokenscope::logging::LogInitStatus {
    status.inner().clone()
}

#[tauri::command]
pub async fn source_status() -> Result<Vec<SourceStatus>, String> {
    run_blocking("source_status", move || {
        // AP02：严格读取——配置存在但坏掉时**拒绝**，绝不用默认来源顶替
        //（那会把用户停用的来源重新拉回统计，并谎报数据范围）。
        let s = load_settings_strict()?;
        source_status_impl(&s)
    })
    .await
}

/// AP02：读设置的**严格**入口。文件缺失 = 产品默认值（首次启动的正常
/// 路径）；文件存在但读取/解析失败 = Err（含路径与原因）。所有决定
/// 「采集哪些来源」的入口（查询、状态、重建）都用它。
pub(crate) fn load_settings_strict() -> anyhow::Result<tokenscope::settings::Settings> {
    let path = tokenscope::settings::settings_path()?;
    tokenscope::settings::load(&path)
}

/// C1：由设置值解析各来源的有效配置（目录覆盖 + 启停）。
fn source_settings_from(
    s: &tokenscope::settings::Settings,
) -> (
    Option<std::path::PathBuf>,
    Option<std::path::PathBuf>,
    bool,
    bool,
) {
    let c = s.source_config(true);
    let x = s.source_config(false);
    (
        c.dir.map(std::path::PathBuf::from),
        x.dir.map(std::path::PathBuf::from),
        c.enabled,
        x.enabled,
    )
}

/// AP02：采集/状态入口的统一来源解析内核（可注入读取结果以便测试）。
/// 读取失败 → 明确错误；**不**回退默认来源。
#[allow(clippy::type_complexity)]
fn source_settings_impl(
    loaded: anyhow::Result<tokenscope::settings::Settings>,
) -> Result<
    (
        Option<std::path::PathBuf>,
        Option<std::path::PathBuf>,
        bool,
        bool,
    ),
    String,
> {
    match loaded {
        Ok(s) => Ok(source_settings_from(&s)),
        Err(e) => {
            log::warn!("设置读取失败，拒绝按默认来源采集: {e:#}");
            Err(format!("{e:#}"))
        }
    }
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
    source_settings_impl(load_settings_strict())
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

/// AP02：关闭决策的读取入口——设置不可读时**退回「每次询问」**（安全默认：
/// 不隐藏窗口、不退出进程，由用户当场决定），并记录原因。
///
/// 该退路**只**服务关闭行为：它绝不出现在采集/状态/重建路径上——那里配置
/// 读不出来就是错误，否则坏配置会被悄悄当成"用户重新启用了默认来源"。
pub fn close_decision_now() -> CloseDecision {
    close_decision_impl(load_settings_strict())
}

/// AP02：关闭决策内核（可注入读取结果以便测试）。
pub(crate) fn close_decision_impl(loaded: anyhow::Result<Settings>) -> CloseDecision {
    match loaded {
        Ok(s) => close_decision_from(&s),
        Err(e) => {
            log::warn!("关闭行为：设置读取失败，退回「每次询问」: {e:#}");
            CloseDecision::Ask
        }
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
/// SF02：经 settings::update 事务写盘，锁内基于最新文件只改 close_action。
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
    tokenscope::settings::update(path, |s| {
        s.close_action = parsed;
        Ok(action.clone())
    })?;
    log::info!("关闭窗口默认动作已设置: {:?}", parsed);
    Ok(action)
}

/// F08：隐藏主窗口——窗口缺失或 hide 失败都返回可操作错误（前端沿
/// 既有 await/catch 路径展示并允许重试），不再静默吞掉。
fn hide_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("main")
        .ok_or_else(|| "主窗口不存在，无法最小化；可改用「直接退出」".to_string())?;
    w.hide().map_err(|e| format!("隐藏窗口失败: {e}"))
}

/// F08：最小化动作的统一处理——成功才记成功日志；失败透传为可重试
/// 错误。窗口动作可注入（测试验证错误传播，不构造真实 AppHandle）。
fn close_minimize_with(hide: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    match hide() {
        Ok(()) => {
            log::info!("窗口关闭：最小化到托盘");
            Ok(())
        }
        Err(e) => Err(format!("{e}（可重试）")),
    }
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
        close_minimize_with(|| hide_main_window(&app))?;
    } else {
        log::info!("窗口关闭：用户选择直接退出");
        app.exit(0);
    }
    Ok(())
}

fn persist_close_action(path: &std::path::Path, minimize: bool) -> anyhow::Result<()> {
    // SF02：事务内只改 close_action，其他字段基于最新文件保留。
    tokenscope::settings::update(path, |s| {
        s.close_action = Some(if minimize {
            CloseAction::Minimize
        } else {
            CloseAction::Quit
        });
        Ok(())
    })
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
/// SF02：来源重叠校验在事务锁内基于最新其他来源值执行。
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
    let cfg = tokenscope::settings::SourceConfig {
        enabled,
        dir: dir.clone(),
    };
    let cfg_for_save = cfg.clone();
    tokenscope::settings::update(path, move |s| {
        // 先落本次修改，再按"保存后的全量配置"校验（校验对象 = 生效配置）。
        if claude {
            s.sources.claude = Some(cfg_for_save);
        } else {
            s.sources.codex = Some(cfg_for_save);
        }
        // SF09/AP01：解析启用来源的**有效**目录后检查相同/嵌套冲突——
        // 缺省字段（None）= 默认启用 + 工具默认根，因此同样参与校验；
        // 解析函数与采集层共用（report::effective_source_dirs_from_settings），
        // 不再把 None 当成"不参与校验"而放行采集层必然拒绝的配置。
        // 校验与写入在同一临界区内完成：失败即不落盘。
        let dirs = tokenscope::report::effective_source_dirs_from_settings(s)?;
        for i in 0..dirs.len() {
            for j in i + 1..dirs.len() {
                let (ka, da) = &dirs[i];
                let (kb, db) = &dirs[j];
                tokenscope::settings::validate_dir_conflict(da, db).map_err(|e| {
                    anyhow::anyhow!("{e}（来源: {} / {}）", ka.as_str(), kb.as_str())
                })?;
            }
        }
        Ok(())
    })?;
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
        // SF02：事务内基于最新文件只改 auto_sync 字段；用户关闭自动同步的
        // 值不得被另一字段的旧副本复原（含并发关闭动作/来源保存）。
        let committed = tokenscope::settings::update(&path, |s| {
            s.price_auto_sync = enabled;
            Ok(enabled)
        })?;
        if enabled {
            log::info!("价格自动同步已开启");
        } else {
            log::info!("价格自动同步已关闭");
        }
        let _ = app;
        Ok(committed)
    })
    .await
}

/// 上次视图快照（M10 后启动提速）：原样存取前端渲染结果，零类型耦合。
/// 坏文件 → None 静默忽略。
#[tauri::command]
pub async fn view_cache_load() -> Result<Option<serde_json::Value>, String> {
    run_blocking("view_cache_load", move || {
        let path = view_cache_path()?;
        // R04：读写共用 impl（round-trip 同源）。
        view_cache_load_impl(&path)
    })
    .await
}

#[tauri::command]
pub async fn view_cache_save(value: serde_json::Value) -> Result<(), String> {
    run_blocking("view_cache_save", move || {
        let t = std::time::Instant::now();
        let path = view_cache_path()?;
        view_cache_save_impl(&path, &value)?;
        log::debug!("视图快照保存：{} ms", t.elapsed().as_millis());
        Ok(())
    })
    .await
}

/// R04：视图快照原子写（宁旧勿坏）——直接覆盖写在崩溃/断电时会留下
/// 半截文件。路径可注入以便测试注入失败场景。
pub(crate) fn view_cache_save_impl(
    path: &std::path::Path,
    value: &serde_json::Value,
) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    tokenscope::fsutil::atomic_write(path, json.as_bytes())
        .with_context(|| format!("写视图缓存失败: {}", path.display()))?;
    Ok(())
}

/// 读回视图快照（round-trip 测试用）。
pub(crate) fn view_cache_load_impl(
    path: &std::path::Path,
) -> anyhow::Result<Option<serde_json::Value>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读视图缓存失败: {}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("视图快照解析失败: {}", path.display()))
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
                // F04：同步丢弃诊断（整条拒绝的模型与分项）沿既有日志路径记录
                for w in &r.warnings {
                    log::warn!("OpenRouter 同步丢弃数据: {w}");
                }
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
            st.warnings.iter().any(|w| w.contains("解析失败")),
            "损坏快照必须有诊断: {:?}",
            st.warnings
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 隔离临时目录（设置文件测试共用）。
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// AP02：坏配置必须拒绝查询与来源状态，绝不用默认来源顶替用户配置。
    #[test]
    fn corrupt_settings_rejects_query_and_source_status() {
        let dir = tmp_dir("ap02-corrupt");
        let path = dir.join("settings.toml");
        std::fs::write(&path, "price_auto_sync = true\n[sources.claude\n").unwrap();
        // 采集探针：若入口放行（旧行为 = 回退默认来源），才会走到采集一步。
        let mut collected = 0u32;
        match source_settings_impl(tokenscope::settings::load(&path)) {
            Ok(_) => collected += 1,
            Err(e) => assert!(
                e.contains("设置解析失败") || e.contains("读设置失败"),
                "错误必须携带原因: {e}"
            ),
        }
        assert_eq!(collected, 0, "配置损坏时不得回退默认来源继续采集");
        // 状态入口共用同一内核（同一错误形态）。
        let err = source_settings_impl(tokenscope::settings::load(&path)).unwrap_err();
        assert!(err.contains("设置解析失败"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// AP02：配置**在位但读不出来**（路径被目录占用）同样拒绝，不回退默认。
    #[test]
    fn unreadable_settings_does_not_enable_default_sources() {
        let dir = tmp_dir("ap02-unreadable");
        let path = dir.join("settings.toml");
        // exists() = true，read_to_string 必失败——「在位但读不出来」。
        std::fs::create_dir_all(&path).unwrap();
        let err = source_settings_impl(tokenscope::settings::load(&path)).unwrap_err();
        assert!(err.contains("读设置失败"), "必须报告读取失败: {err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// AP02：用户显式停用/自定义根在位时，配置损坏不得把它们还原成默认。
    #[test]
    fn corrupt_settings_does_not_restore_disabled_sources() {
        let dir = tmp_dir("ap02-disabled");
        let path = dir.join("settings.toml");
        std::fs::write(
            &path,
            "[sources.claude]\nenabled = false\ndir = \"D:/logs/claude\"\n[sources.codex]\nenabled = false\n",
        )
        .unwrap();
        let (cd, _xd, ce, xe) = source_settings_impl(tokenscope::settings::load(&path)).unwrap();
        assert!(!ce && !xe, "基线：两来源显式停用");
        assert_eq!(cd.as_deref(), Some(std::path::Path::new("D:/logs/claude")));
        // 同一路径被破坏后：拒绝，而不是变成"两来源默认启用 + 默认根"。
        std::fs::write(&path, "not [valid toml").unwrap();
        assert!(source_settings_impl(tokenscope::settings::load(&path)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// AP02：文件缺失 = 首次启动 → 产品默认值（默认启用 + 默认根）仍成立。
    #[test]
    fn missing_settings_keeps_documented_defaults() {
        let dir = tmp_dir("ap02-missing");
        let path = dir.join("nested").join("settings.toml");
        let (cd, xd, ce, xe) = source_settings_impl(tokenscope::settings::load(&path)).unwrap();
        assert!(ce && xe, "首次启动：两个来源默认启用");
        assert!(
            cd.is_none() && xd.is_none(),
            "首次启动：两来源走默认根（None）"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// AP02：关闭决策的读取失败退路**只**退回「每次询问」，不改变用户
    /// 明确表达的退出/最小化意图。
    #[test]
    fn close_decision_reading_failure_falls_back_to_ask() {
        assert_eq!(
            close_decision_impl(Err(anyhow::anyhow!("设置解析失败: settings.toml"))),
            CloseDecision::Ask
        );
        for (action, expected) in [
            (Some(CloseAction::Quit), CloseDecision::Quit),
            (Some(CloseAction::Minimize), CloseDecision::Minimize),
            (None, CloseDecision::Ask),
        ] {
            let s = Settings {
                close_action: action,
                ..Default::default()
            };
            assert_eq!(
                close_decision_impl(Ok(s)),
                expected,
                "close_action={action:?}"
            );
        }
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

    /// AP01：缺省字段（`None`）= 默认启用 + 默认根，因此**必须参与**保存校验。
    /// 修复前 `None` 被当成"不参与校验"，用户能保存一份采集层必然拒绝的
    /// 配置：保存返回成功，紧接着每次查询都报来源目录冲突。
    #[test]
    fn default_enabled_source_participates_in_overlap_validation() {
        let dir =
            std::env::temp_dir().join(format!("tokenscope-ap01-overlap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        // 无来源配置：两个来源均缺省 = 默认启用 + 默认根。
        std::fs::write(&path, "price_auto_sync = true\n").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let read = || std::fs::read_to_string(&path).unwrap();

        let codex_default = tokenscope::source::codex::CodexSource::default_root().unwrap();
        let claude_default = tokenscope::source::claude::ClaudeSource::default_root().unwrap();

        // 正例：Claude 显式指向 Codex 的默认根（Codex 仍缺省）→ 拒绝且不落盘。
        let r = source_config_set_impl(
            &path,
            "claude",
            true,
            Some(codex_default.display().to_string()),
        );
        assert!(
            r.is_err(),
            "缺省的 Codex 来源仍在生效，撞其默认根必须拒绝: {r:?}"
        );
        assert_eq!(read(), before, "被拒绝的配置不得落盘");

        // 嵌套：Codex 默认根的父目录同样拒绝。
        let parent = codex_default.parent().unwrap().display().to_string();
        assert!(
            source_config_set_impl(&path, "claude", true, Some(parent)).is_err(),
            "与默认根嵌套的目录必须拒绝"
        );
        assert_eq!(read(), before);

        // 反向：Codex 显式指向 Claude 的默认根（Claude 仍缺省）→ 拒绝。
        assert!(
            source_config_set_impl(
                &path,
                "codex",
                true,
                Some(claude_default.display().to_string())
            )
            .is_err(),
            "反向撞默认根同样拒绝"
        );
        assert_eq!(read(), before);

        // 显式停用允许保存到冲突目录（用户借停用恢复的路径）。
        source_config_set_impl(
            &path,
            "claude",
            false,
            Some(codex_default.display().to_string()),
        )
        .expect("停用来源不参与校验，必须允许保存");

        // 无配置来源的首次保存：无冲突目录 → 成功。
        let custom = dir.join("codex-custom");
        source_config_set_impl(&path, "codex", true, Some(custom.display().to_string()))
            .expect("无冲突的显式目录必须可保存");

        // 边界：显式目录与本来源默认目录一致（Claude 显式 = Claude 默认根）。
        source_config_set_impl(
            &path,
            "claude",
            true,
            Some(claude_default.display().to_string()),
        )
        .expect("显式目录与本来源默认根一致不构成冲突（另一来源为自定义目录）");
        let s = tokenscope::settings::load(&path).unwrap();
        assert_eq!(
            s.source_config(true).dir.as_deref(),
            Some(claude_default.to_string_lossy().as_ref())
        );
        assert_eq!(
            s.source_config(false).dir.as_deref(),
            Some(custom.to_string_lossy().as_ref())
        );
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
    fn test_view_cache_round_trip() {
        let dir = std::env::temp_dir().join(format!("tokenscope-vc-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("view-cache.json");
        assert!(
            view_cache_load_impl(&path).unwrap().is_none(),
            "缺文件 = None"
        );
        let payload = serde_json::json!({
            "v": 4,
            "saved_at": "2026-10-07T00:00:00Z",
            "filters": {"by": "model", "agent": "all", "range": null, "drill": null, "tz": "UTC"},
            "report": {"groups": [], "totals": {}},
            "events": {"rows": [], "total": 0, "warnings": []}
        });
        view_cache_save_impl(&path, &payload).unwrap();
        let back = view_cache_load_impl(&path).unwrap().unwrap();
        assert_eq!(back, payload, "完整 payload 原样往返");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_view_cache_atomic_failure_keeps_previous() {
        // 目标路径被目录占用 → rename 失败 → 旧文件原样保留（宁旧勿坏）。
        let dir = std::env::temp_dir().join(format!("tokenscope-vc-fail-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("view-cache.json");
        let good = serde_json::json!({"v": 4, "saved_at": "first"});
        view_cache_save_impl(&path, &good).unwrap();
        // 把目标路径变成目录：rename 必定失败
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        let r = view_cache_save_impl(&path, &serde_json::json!({"v": 4, "saved_at": "second"}));
        assert!(r.is_err(), "目标为目录必须失败");
        // 临时文件已清理
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "失败的临时文件必须清理: {leftovers:?}"
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
    fn test_close_hide_error_is_propagated() {
        // F08：hide 失败必须透传为可重试错误（含原始原因），不得吞掉；
        // 成功路径返回 Ok 且不产生错误信息。
        let err = close_minimize_with(|| Err("webview busy".to_string()))
            .expect_err("hide 失败必须返回 Err");
        assert!(
            err.contains("webview busy") && err.contains("可重试"),
            "错误须携带原因并标注可重试: {err}"
        );
        assert!(close_minimize_with(|| Ok(())).is_ok());
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
