//! TokenScope 桌面壳：只做窗口/托盘生命周期与 command 装配，
//! 全部数据逻辑在根 crate 的 report 管线（CLI/GUI 同源）。

mod commands;
mod window_state;

#[cfg(feature = "acceptance")]
mod acceptance;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    // RC10：验收构建在**任何路径被解析之前**确认隔离根。缺失/非法直接退出
    // （退出码 2），绝不带着真实 ~/.tokenscope 与真实 agent 日志继续启动——
    // 那种"通过"其实是拿用户数据签字。普通构建编译期就没有这段代码。
    #[cfg(feature = "acceptance")]
    let acceptance_root = match acceptance::start() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("验收模式启动失败：{e}");
            std::process::exit(2);
        }
    };
    // SF06：日志初始化可失败——降级不阻断窗口创建；WorkerGuard 仍与进程
    // 同生命周期（run 阻塞至退出）。状态经 managed state 暴露给前端。
    let logging = tokenscope::logging::try_init("gui");
    let log_status = logging.status;
    let _log_guard = logging.guard;
    let t_boot = std::time::Instant::now();
    log::info!("TokenScope 启动（GUI）");
    // RC10：验收构建把 WebView 用户数据目录与 identifier 也指向隔离根，
    // 这样首帧主题/恢复行为不受真实 localStorage 干扰，且能与普通实例并存。
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(feature = "acceptance")]
    acceptance::configure(context.config_mut(), &acceptance_root);
    tauri::Builder::default()
        // 单实例必须最先注册：二次启动走回调唤起已有窗口，不新建实例。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            log::info!("检测到二次启动，唤起已有窗口");
            show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::summarize,
            commands::list_events,
            commands::query_begin,
            commands::query_summary,
            commands::query_events,
            commands::startup_diagnostics,
            commands::view_cache_load,
            commands::view_cache_save,
            commands::source_status,
            commands::source_config_set,
            commands::cache_stats,
            commands::refresh_cache,
            commands::pricing_entries,
            commands::pricing_status,
            commands::open_pricing_file,
            commands::sync_pricing_openrouter,
            commands::autostart_status,
            commands::autostart_set,
            commands::settings_get,
            commands::settings_set_price_auto_sync,
            commands::settings_set_close_action,
            commands::close_resolve,
            commands::open_settings_file,
        ])
        .setup(move |app| {
            // SF06：日志状态存入 managed state，App 挂载后经
            // startup_diagnostics 只读获取并展示非阻断通知。
            app.manage(log_status);
            restore_window_state(app.handle())?;
            setup_tray(app.handle())?;
            start_window_state_saver(app.handle());
            start_price_auto_sync();
            log::info!(
                "GUI 初始化完成（窗口状态/托盘/自同步线程），{} ms",
                t_boot.elapsed().as_millis()
            );
            Ok(())
        })
        // 关闭行为三态（关闭确认与配置文件计划）：配置了默认动作（设置页或
        // 弹窗记忆）则直接执行；未配置 → prevent_close + emit close-requested，
        // 由前端弹窗询问（最小化/退出/取消 + 记忆勾选）。关窗时机顺带落盘
        // 一次窗口状态。
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if let Some(w) = window.get_webview_window("main") {
                    save_window_state_now(&w);
                }
                match commands::close_decision_from(&commands::load_settings_or_default()) {
                    commands::CloseDecision::Minimize => {
                        let _ = window.hide();
                        api.prevent_close();
                    }
                    commands::CloseDecision::Quit => {
                        log::info!("窗口关闭：按设置直接退出");
                        window.app_handle().exit(0);
                    }
                    commands::CloseDecision::Ask => {
                        // 前端未就绪时事件无人接收：窗口保持打开（不静默退出）。
                        let _ = window.emit("close-requested", ());
                        api.prevent_close();
                    }
                }
            }
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                if let Some(w) = window.get_webview_window("main") {
                    update_window_state(&w);
                }
            }
            _ => {}
        })
        .run(context)
        .expect("TokenScope 应用运行失败");
}

// ── 窗口状态（M8）────────────────────────────────────────
// 内存态由窗口事件更新，后台线程每秒检查脏标记落盘；最大化期间不更新
// 尺寸/位置（还原时回到最大化前的窗口矩形）。

type SharedState = Arc<Mutex<Option<window_state::WindowState>>>;

fn window_state_shared() -> SharedState {
    Arc::new(Mutex::new(None))
}

fn restore_window_state(app: &tauri::AppHandle) -> tauri::Result<()> {
    let path = window_state::state_path().ok();
    let loaded = path.as_deref().and_then(|p| match window_state::load(p) {
        Ok(ws) => ws,
        Err(e) => {
            log::warn!("窗口状态读取失败，使用默认尺寸: {e:#}");
            None
        }
    });
    if let (Some(win), Some(ws)) = (app.get_webview_window("main"), loaded.clone()) {
        if ws.maximized {
            let _ = win.maximize();
        } else {
            let _ = win.set_size(tauri::PhysicalSize::new(ws.width, ws.height));
            let _ = win.set_position(tauri::PhysicalPosition::new(ws.x, ws.y));
        }
    }
    app.manage(SharedStateHolder {
        state: window_state_shared(),
        dirty: Arc::new(AtomicBool::new(false)),
    });
    if let Some(ws) = loaded {
        *state_mutex(app).lock().unwrap() = Some(ws);
    }
    Ok(())
}

struct SharedStateHolder {
    state: SharedState,
    dirty: Arc<AtomicBool>,
}

fn state_mutex(app: &tauri::AppHandle) -> Arc<Mutex<Option<window_state::WindowState>>> {
    app.state::<SharedStateHolder>().state.clone()
}

fn dirty_flag(app: &tauri::AppHandle) -> Arc<AtomicBool> {
    app.state::<SharedStateHolder>().dirty.clone()
}

fn update_window_state(window: &tauri::WebviewWindow) {
    let app = window.app_handle();
    let state = state_mutex(app);
    let mut guard = state.lock().unwrap();
    let maximized = window.is_maximized().unwrap_or(false);
    let ws = guard.get_or_insert_with(|| window_state::WindowState {
        width: 1280.0,
        height: 820.0,
        x: 0.0,
        y: 0.0,
        maximized: false,
    });
    ws.maximized = maximized;
    if !maximized
        && let (Some(size), Some(pos)) = (window.inner_size().ok(), window.outer_position().ok())
    {
        ws.width = size.width as f64;
        ws.height = size.height as f64;
        ws.x = pos.x as f64;
        ws.y = pos.y as f64;
    }
    dirty_flag(app).store(true, Ordering::Relaxed);
}

fn save_window_state_now(window: &tauri::WebviewWindow) {
    update_window_state(window);
    let app = window.app_handle();
    let state = state_mutex(app);
    let guard = state.lock().unwrap();
    if let Some(ws) = guard.as_ref()
        && let Ok(path) = window_state::state_path()
        && let Err(e) = window_state::save(&path, ws)
    {
        log::warn!("窗口状态保存失败: {e:#}");
    }
    dirty_flag(app).store(false, Ordering::Relaxed);
}

/// 每秒检查脏标记并落盘（节流）。
fn start_window_state_saver(app: &tauri::AppHandle) {
    let state = state_mutex(app);
    let dirty = dirty_flag(app);
    let path = window_state::state_path().ok();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if !dirty.swap(false, Ordering::Relaxed) {
                continue;
            }
            let (Some(path), Some(ws)) = (path.clone(), state.lock().unwrap().clone()) else {
                continue;
            };
            if let Err(e) = window_state::save(&path, &ws) {
                log::warn!("窗口状态保存失败: {e:#}");
            }
        }
    });
}

// ── 价格定时同步（M11）──────────────────────────────────
// 默认开启、每 24h 检查一次：距上次成功同步 ≥24h 则后台同步双源并重建
// 索引。轮询而非精确定时（系统休眠会漂移）；关闭开关后完全不联网。
const PRICE_SYNC_INTERVAL_HOURS: i64 = 24;

fn start_price_auto_sync() {
    std::thread::spawn(move || {
        // 首查延迟 2 分钟，避开启动瞬间的磁盘与网络竞争。
        std::thread::sleep(std::time::Duration::from_secs(120));
        loop {
            // RC10：定位不到数据目录时**不**退回当前工作目录（那会把
            // settings.json 写到进程 CWD，脱离任何隔离根/用户目录约定）。
            let Ok(settings_path) = tokenscope::settings::settings_path() else {
                log::warn!("无法定位设置文件路径，本轮价格自动同步跳过（不回退 CWD）");
                std::thread::sleep(std::time::Duration::from_secs(3600));
                continue;
            };
            // D3/R06：设置异常 → 离线（关闭自动同步的用户意图不能被重置成联网）。
            let loaded = tokenscope::settings::load(&settings_path);
            let auto = tokenscope::settings::auto_sync_allowed(&loaded);
            if let Err(e) = &loaded {
                log::warn!("设置读取失败，价格自动同步本轮按离线处理: {e:#}");
            }
            let modelsdev_snapshot = tokenscope::report::modelsdev_file_path(None);
            let (due, age_note) = match last_sync_age_hours(&modelsdev_snapshot) {
                Ok(hours) => (hours >= PRICE_SYNC_INTERVAL_HOURS, format!("{hours}h")),
                Err(reason) => (true, format!("未知（{reason}）")),
            };
            log::debug!(
                "价格同步轮询：auto={auto} 距上次同步={age_note}（阈值 {PRICE_SYNC_INTERVAL_HOURS}h）→ {}",
                if auto && due { "同步" } else { "跳过" }
            );
            if auto && due {
                let t = std::time::Instant::now();
                log::info!("价格自动同步开始（距上次同步 {age_note}）");
                match tokenscope::modelsdev::sync(&tokenscope::report::modelsdev_file_path(None)) {
                    Ok(r) => log::info!("models.dev 自动同步成功: {} 条 → {}", r.count, r.path),
                    Err(e) => tokenscope::logging::log_error("models.dev 自动同步失败", &e),
                }
                match tokenscope::openrouter::sync(&tokenscope::report::openrouter_file_path(None))
                {
                    Ok(r) => log::info!("OpenRouter 自动同步成功: {} 条 → {}", r.count, r.path),
                    Err(e) => tokenscope::logging::log_error("OpenRouter 自动同步失败", &e),
                }
                // 同步后重建索引，供下次启动快速加载；失败不影响已落盘的快照。
                let t_index = std::time::Instant::now();
                let (_, index_warnings, _) = tokenscope::pricing::Pricing::load_cached(
                    Some(&tokenscope::report::pricing_file_path(None)),
                    Some(&tokenscope::report::modelsdev_file_path(None)),
                    Some(&tokenscope::report::openrouter_file_path(None)),
                    &tokenscope::report::pricing_index_path(None),
                );
                for w in &index_warnings {
                    log::warn!("价格索引重建警告: {w}");
                }
                log::debug!("价格索引重建完成，{} ms", t_index.elapsed().as_millis());
                log::info!("价格自动同步完成，{} ms", t.elapsed().as_millis());
            }
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    });
}

/// 距快照记录的上次同步的小时数（无快照/解析失败 → Err，视为立即同步）。
fn last_sync_age_hours(modelsdev_snapshot: &std::path::Path) -> Result<i64, String> {
    use jiff::Timestamp;
    let snapshot = tokenscope::modelsdev::load_snapshot(modelsdev_snapshot)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "从未同步".to_string())?;
    let last = snapshot
        .synced_at
        .parse::<Timestamp>()
        .map_err(|e| format!("synced_at 解析失败: {e}"))?;
    let now = Timestamp::now();
    let secs = now.as_second() - last.as_second();
    Ok(secs / 3600)
}

// ── 托盘 ─────────────────────────────────────────────────// ── 托盘 ─────────────────────────────────────────────────

/// 启动即创建托盘图标；左键单击恢复主窗口。
fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 TokenScope", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    let mut tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("TokenScope")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}
