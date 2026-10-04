//! TokenScope 桌面壳：只做窗口/托盘生命周期与 command 装配，
//! 全部数据逻辑在根 crate 的 report 管线（CLI/GUI 同源）。

mod commands;
mod window_state;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    // WorkerGuard 与进程同生命周期（run 阻塞至退出）。
    let _log_guard = tokenscope::logging::init("gui");
    log::info!("TokenScope 启动（GUI）");
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
            commands::view_cache_load,
            commands::view_cache_save,
            commands::source_status,
            commands::cache_stats,
            commands::refresh_cache,
            commands::pricing_entries,
            commands::open_pricing_file,
            commands::sync_pricing_openrouter,
            commands::autostart_status,
            commands::autostart_set,
        ])
        .setup(|app| {
            restore_window_state(app.handle())?;
            setup_tray(app.handle())?;
            start_window_state_saver(app.handle());
            Ok(())
        })
        // 关闭主窗口 = 缩到托盘（用户要求），真正退出走托盘菜单；
        // 关窗时机顺带落盘一次窗口状态。
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if let Some(w) = window.get_webview_window("main") {
                    save_window_state_now(&w);
                }
                let _ = window.hide();
                api.prevent_close();
            }
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                if let Some(w) = window.get_webview_window("main") {
                    update_window_state(&w);
                }
            }
            _ => {}
        })
        .run(tauri::generate_context!())
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

// ── 托盘 ─────────────────────────────────────────────────

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
