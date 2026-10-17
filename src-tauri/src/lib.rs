//! TokenScope 桌面壳：只做窗口/托盘生命周期与 command 装配，
//! 全部数据逻辑在根 crate 的 report 管线（CLI/GUI 同源）。

mod commands;
mod privacy;
mod window_state;

#[cfg(feature = "acceptance")]
mod acceptance;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use tauri::{
    Emitter, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_autostart::MacosLauncher;

use privacy::{RuntimeFlags, RuntimeStep};
use tokenscope::collection_status::CollectionObserver;

/// SF06 + P03：日志运行时状态（managed state）。
///
/// 未同意前**不**创建 `logs` 目录、不安装全局 subscriber，也不建立永久
/// stderr subscriber 阻挡后续 logger；同意后由 [`initialize_business_runtime`]
/// 初始化一次，WorkerGuard 随本 holder 存活至进程退出（不随局部函数返回丢弃）。
pub(crate) struct LogState(Mutex<Option<tokenscope::logging::Logging>>);

impl LogState {
    fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// 当前状态：未初始化时为 `pending`（前端只显示非阻断通知，不报故障）。
    pub(crate) fn status(&self) -> tokenscope::logging::LogInitStatus {
        match self.lock().as_ref() {
            Some(logging) => logging.status.clone(),
            None => tokenscope::logging::LogInitStatus {
                state: "pending",
                dir: None,
                message: None,
            },
        }
    }

    /// 单次初始化（重复调用返回既有状态，不重复安装 subscriber）。
    fn initialize(&self) -> tokenscope::logging::LogInitStatus {
        let mut guard = self.lock();
        if let Some(logging) = guard.as_ref() {
            return logging.status.clone();
        }
        let logging = tokenscope::logging::try_init("gui");
        let status = logging.status.clone();
        *guard = Some(logging);
        status
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<tokenscope::logging::Logging>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

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
    // P03：未同意前零业务副作用——日志初始化（建 logs 目录、打开日志文件）
    // 延后到同意之后的单次业务初始化；引导阶段只在内存里维护状态。
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
            privacy::privacy_bootstrap,
            privacy::privacy_accept,
            privacy::privacy_exit_resolve,
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
            commands::source_paths_list,
            commands::source_path_add,
            commands::source_path_remove,
            commands::source_path_set_enabled,
            commands::source_auto_set,
            commands::cache_stats,
            commands::refresh_cache,
            commands::ccs_import_defaults,
            commands::ccs_import_preview,
            commands::ccs_import_commit,
            commands::ccs_import_discard,
            commands::pricing_entries,
            commands::pricing_status,
            commands::open_pricing_file,
            commands::sync_pricing_openrouter,
            commands::autostart_status,
            commands::autostart_set,
            commands::settings_get,
            commands::settings_set_price_auto_sync,
            commands::settings_set_close_action,
            commands::settings_set_timezone,
            commands::settings_set_collect_interval,
            commands::collection_status,
            commands::close_resolve,
            commands::open_settings_file,
        ])
        .setup(move |app| {
            #[cfg(feature = "acceptance")]
            acceptance::create_windows(app)?;
            // 最小引导初始化：只注册内存容器与状态，零业务副作用。
            // 窗口事件可能随时到达，先把 holder 注册好（否则 app.state 会 panic）；
            // 日志/窗口恢复/托盘/价格线程一律等同意后的单次业务初始化。
            install_window_state_holder(app.handle());
            app.manage(LogState::new());
            app.manage(privacy::PrivacyState::new());
            Ok(())
        })
        // 关闭行为三态（关闭确认与配置文件计划）：配置了默认动作（设置页或
        // 弹窗记忆）则直接执行；未配置 → prevent_close + emit close-requested，
        // 由前端弹窗询问（最小化/退出/取消 + 记忆勾选）。
        //
        // AP07：事件回调只做「阻止默认关闭 + 记录内存态 + 启动协调」三件
        // 立即返回的事——设置读取（磁盘 IO）与窗口状态落盘全部移出回调，
        // 由后台线程完成；窗口在后台结果出来前不会消失。协调是单飞的：
        // 处理期间重复关窗不并发执行、不排队。
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                // P02/P03：未同意（非 Ready）时不读 close_action、不写窗口状态、
                // 不最小化——只记录待处理标记并提示前端显示退出确认。
                match privacy::close_event_action(privacy::phase_of(window.app_handle())) {
                    privacy::WindowEventAction::Business => {
                        if let Some(w) = window.get_webview_window("main") {
                            // 仅更新内存并置脏（落盘交给每秒 saver 或退出前的最终保存）。
                            update_window_state(&w);
                            request_close(&w);
                        }
                    }
                    _ => privacy::request_exit_prompt(window.app_handle()),
                }
            }
            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                if privacy::geometry_event_action(privacy::phase_of(window.app_handle()))
                    == privacy::WindowEventAction::Business
                    && let Some(w) = window.get_webview_window("main")
                {
                    update_window_state(&w);
                    emit_window_visibility(&w);
                }
            }
            tauri::WindowEvent::Focused(_) => {
                if let Some(w) = window.get_webview_window("main") {
                    emit_window_visibility(&w);
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

/// P03：只注册窗口状态的内存容器（setup 期调用，纯内存、零磁盘/窗口副作用）。
fn install_window_state_holder(app: &tauri::AppHandle) {
    app.manage(SharedStateHolder {
        state: window_state_shared(),
        dirty: Arc::new(AtomicBool::new(false)),
    });
}

/// P03：读取窗口状态文件（纯 IO，调用方须在后台线程）。
fn load_window_state() -> (Option<window_state::WindowState>, Vec<String>) {
    let mut diagnostics = Vec::new();
    let path = match window_state::state_path() {
        Ok(p) => p,
        Err(e) => {
            diagnostics.push(format!("无法定位窗口状态路径，本次使用默认尺寸：{e:#}"));
            return (None, diagnostics);
        }
    };
    match window_state::load(&path) {
        Ok(ws) => (ws, diagnostics),
        Err(e) => {
            diagnostics.push(format!("窗口状态读取失败，本次使用默认尺寸：{e:#}"));
            (None, diagnostics)
        }
    }
}

/// P03：把已读取的窗口状态应用到窗口并初始化内存基线（**主线程**调用）。
fn apply_loaded_window_state(app: &tauri::AppHandle, loaded: Option<window_state::WindowState>) {
    let Some(ws) = loaded else { return };
    *state_mutex(app).lock().unwrap_or_else(|e| e.into_inner()) = Some(ws.clone());
    if let Some(win) = app.get_webview_window("main") {
        if ws.maximized {
            let _ = win.maximize();
        } else {
            // G-P3-3：多显示器拔除后旧 x/y 可能完全在屏幕外——与任一
            // 可用监视器无交集就放弃恢复（保留 tauri.conf 默认尺寸/居中）。
            // I-P3-5 注记：内存基线仍写入旧值；托盘退出/异常终止保留它
            // （重插显示器后可找回），X 关闭路径会以实际可见位置覆盖基线。
            // 无论哪条路径，下次启动都会重过相交门，屏外状态不会导致
            // 不可见窗口。
            let monitors: Vec<(i32, i32, u32, u32)> = win
                .available_monitors()
                .map(|list| {
                    list.iter()
                        .map(|m| {
                            let p = m.position();
                            let s = m.size();
                            (p.x, p.y, s.width, s.height)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if window_state::restorable_on_any_monitor(&ws, &monitors) {
                let _ = win.set_size(tauri::PhysicalSize::new(ws.width, ws.height));
                let _ = win.set_position(tauri::PhysicalPosition::new(ws.x, ws.y));
            } else {
                log::warn!(
                    "窗口状态恢复被跳过：保存的位置 ({},{}) 不在任何可用显示器内",
                    ws.x,
                    ws.y
                );
            }
        }
    }
}

/// 在窗口主线程执行并取回结果（Tauri 的窗口 API 只能在主线程调用）。
pub(crate) fn on_main_thread<T: Send + 'static>(
    app: &tauri::AppHandle,
    work: impl FnOnce(&tauri::AppHandle) -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        let out = work(&app_for_main);
        let _ = tx.send(out);
    })
    .map_err(|e| format!("调度主线程任务失败: {e}"))?;
    rx.recv_timeout(Duration::from_secs(30))
        .map_err(|e| format!("等待主线程任务失败: {e}"))
}

/// P03：同意成立后的**单次**业务初始化——只补 [`RuntimeFlags`] 未完成的步骤。
///
/// 必须在后台线程调用（含磁盘 IO 与线程启动）；窗口 API 经主线程调度。
/// 返回非阻断诊断（日志降级、托盘失败等）；Err = 关键失败（主线程不可用），
/// 此时保持闸门关闭，用户重试时只补未完成部分。
pub(crate) fn initialize_business_runtime(
    app: &tauri::AppHandle,
    flags: &mut RuntimeFlags,
) -> Result<Vec<String>, String> {
    let t0 = std::time::Instant::now();
    let mut diagnostics = Vec::new();

    // 0) PR3：启动环境快照（CODEX_HOME / CLAUDE_CONFIG_DIR 只在启动时捕获，
    //    采集轮次不重读——修改环境变量后需要重启）。
    commands::capture_startup_environment();

    // 1) 文件日志：失败按 SF06 的非阻断降级路径（stderr / 关闭输出）。
    if !flags.done(RuntimeStep::Logging) {
        let status = app.state::<LogState>().initialize();
        flags.mark(RuntimeStep::Logging);
        if status.state != "ok" {
            diagnostics.push(
                status
                    .message
                    .clone()
                    .unwrap_or_else(|| "文件日志不可用（已降级）".to_string()),
            );
        }
        log::info!("日志系统初始化：state={}", status.state);
    }

    // 2) 窗口状态：读取在后台（本函数已在后台线程），应用到窗口在主线程。
    if !flags.done(RuntimeStep::WindowState) {
        let (loaded, read_diagnostics) = load_window_state();
        diagnostics.extend(read_diagnostics);
        match on_main_thread(app, move |app| apply_loaded_window_state(app, loaded)) {
            Ok(()) => flags.mark(RuntimeStep::WindowState),
            Err(e) => {
                log::warn!("窗口状态恢复失败: {e}");
                return Err(format!("窗口初始化失败：{e}"));
            }
        }
    }

    // 3) 托盘（同意后才创建；失败只报诊断，应用仍可用）。
    if !flags.done(RuntimeStep::Tray) {
        match on_main_thread(app, |app| setup_tray(app).map_err(|e| e.to_string())) {
            Ok(Ok(())) => flags.mark(RuntimeStep::Tray),
            Ok(Err(e)) => {
                log::warn!("托盘创建失败: {e}");
                diagnostics.push(format!(
                    "托盘图标创建失败：{e}（应用仍可用；关闭窗口会询问最小化或退出）"
                ));
            }
            Err(e) => {
                log::warn!("托盘初始化调度失败: {e}");
                return Err(format!("托盘初始化失败：{e}"));
            }
        }
    }

    // 4) 窗口状态节流保存线程（一次）。
    if !flags.done(RuntimeStep::Saver) {
        start_window_state_saver(app);
        flags.mark(RuntimeStep::Saver);
    }

    // 5) 价格自动同步线程：默认开启、按快照到期判断、每小时轮询；
    //    首次等待 120 秒从**业务解锁**开始计时（线程在这里才启动）。
    if !flags.done(RuntimeStep::PriceSync) {
        start_price_auto_sync(app.clone());
        flags.mark(RuntimeStep::PriceSync);
    }

    // 6) H04：原生日志定时采集线程（5 分钟轮询，与手动刷新共用单飞协调器）。
    if !flags.done(RuntimeStep::Collection) {
        start_periodic_collection(app.clone());
        flags.mark(RuntimeStep::Collection);
    }

    // 7) PR2：采集状态事件桥（业务 Ready 后注册一次；之后任何入口的真实
    //    采集轮次都会向前端投递 collection-status 事件）。
    install_collection_status_bridge(app);

    // 8) PR3：v1 来源目录迁移（设置写锁内一次性；保存成功才向采集发布新
    //    设置——本轮失败保留旧文件并按 v1 语义读取，不扩大读取范围）。
    if !flags.done(RuntimeStep::SourcePathMigration) {
        match tokenscope::settings::settings_path() {
            Ok(path) => match tokenscope::settings::migrate_source_paths(&path) {
                Ok(migration) => {
                    flags.mark(RuntimeStep::SourcePathMigration);
                    if migration.changed {
                        let converted = migration
                            .converted
                            .iter()
                            .map(|c| format!("{}: {} → {}", c.agent, c.old_dir, c.path))
                            .collect::<Vec<_>>()
                            .join("；");
                        log::info!("来源目录迁移完成：{converted}");
                        diagnostics.push(format!(
                            "旧版自定义目录已保留为追加目录；自动发现已开启，来源启用时默认目录的历史用量也会纳入，统计总量可能增加。迁移明细：{converted}"
                        ));
                        for note in &migration.overlap_notes {
                            diagnostics.push(note.clone());
                        }
                    }
                }
                Err(e) => {
                    // 迁移失败：保留旧文件字节，按 v1 语义读取（不扩大范围）；
                    // 诊断可见，不静默。
                    flags.mark(RuntimeStep::SourcePathMigration);
                    let msg = format!("来源目录迁移失败（仍按旧配置读取，未扩大读取范围）：{e:#}");
                    log::error!("{msg}");
                    diagnostics.push(msg);
                }
            },
            Err(e) => {
                flags.mark(RuntimeStep::SourcePathMigration);
                diagnostics.push(format!("无法定位设置文件，跳过来源目录迁移：{e:#}"));
            }
        }
    }

    log::info!(
        "业务初始化完成（日志/窗口/托盘/自同步线程，剩余步骤 {}），{} ms",
        flags.pending().len(),
        t0.elapsed().as_millis()
    );
    Ok(diagnostics)
}

// ── PR2：采集状态事件桥 ───────────────────────────────────
// 内核（collection_status::global_tracker）推进真实采集生命周期；本桥只做
// 轻量事件投递：回调期间不查询、不扫描、不打开任何库。注册恰好一次，观察
// 者与进程同生命周期（句柄故意遗忘——Drop 会注销观察者）。发事件失败只
// 记 warn，绝不影响采集结果与用量提交。

struct CollectionStatusSink {
    app: tauri::AppHandle,
}

impl tokenscope::collection_status::CollectionObserver for CollectionStatusSink {
    fn on_collection_status(&self, status: &tokenscope::collection_status::CollectionStatus) {
        if let Err(e) = self.app.emit("collection-status", status) {
            log::warn!("发送采集状态事件失败: {e}");
        }
    }
}

/// 可测的一次性注册：返回是否本次完成注册（重复调用不重复注册）。
fn install_status_bridge_once(
    installed: &AtomicBool,
    tracker: &tokenscope::collection_status::CollectionStatusTracker,
    sink: std::sync::Arc<dyn tokenscope::collection_status::CollectionObserver>,
) -> bool {
    if installed.swap(true, Ordering::SeqCst) {
        return false;
    }
    let handle = tracker.register_observer(sink);
    std::mem::forget(handle);
    true
}

static STATUS_BRIDGE_INSTALLED: AtomicBool = AtomicBool::new(false);

fn install_collection_status_bridge(app: &tauri::AppHandle) {
    let tracker = tokenscope::collection_status::global_tracker();
    let installed = install_status_bridge_once(
        &STATUS_BRIDGE_INSTALLED,
        tracker,
        std::sync::Arc::new(CollectionStatusSink { app: app.clone() }),
    );
    if !installed {
        return;
    }
    // 注册后立即投递一次当前快照：覆盖"前端订阅之前已发生的状态转换"。
    CollectionStatusSink { app: app.clone() }.on_collection_status(&tracker.snapshot());
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

/// AP07：把内存中的窗口状态落盘（退出前的最终保存）。**调用方必须在后台
/// 线程**：这里是磁盘 IO，不能出现在窗口事件回调里；同时不触碰窗口 API
///（Tauri 的窗口操作只能在主线程执行）。
fn persist_window_state(app: &tauri::AppHandle) {
    let path = match window_state::state_path() {
        Ok(p) => p,
        Err(e) => {
            log::warn!("无法定位窗口状态路径，退出前保存跳过: {e:#}");
            return;
        }
    };
    let _save_guard = WINDOW_SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // 先清脏再取快照；保存期间的新窗口事件会重新置脏，不会被成功写入清掉。
    dirty_flag(app).store(false, Ordering::Relaxed);
    let snapshot = state_mutex(app).lock().unwrap().clone();
    let Some(ws) = snapshot else {
        return; // 从未记录过窗口状态：不写空文件。
    };
    match window_state::save(&path, &ws) {
        Ok(()) => {}
        Err(e) => {
            dirty_flag(app).store(true, Ordering::Relaxed);
            log::warn!("退出前窗口状态保存失败: {e:#}");
        }
    }
}

// ── U06：原生日志定时采集（间隔可调 + 变化通知）───────────────
// 与启动采集、手动刷新共用 `report::collect_flighted` 的后台单飞协调器：
// 并发时只会真正采集一次。定时线程每轮只做「采集 + 写历史库」
// （`report::collect_only`，不建查询快照、不聚合），只有本轮确有新增或更新
// 时才向前端发 `usage-updated`。只走原生日志，**不**读取、也不导入 CCS 库
//（计划不变量 11），且只在业务解锁（Ready）后运行。

/// 首轮延迟上限：启动本身已经采集过一次，避开启动瞬间的磁盘竞争；
/// 实际首轮延迟 = min(当前间隔, 30s)。
const COLLECT_FIRST_DELAY_CAP: std::time::Duration = std::time::Duration::from_secs(30);

/// U06：一次可中断等待的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    /// 等满整个时长（该采集了）。
    Elapsed,
    /// 被设置变更唤醒：按新间隔重新计时。
    Woken,
    /// 应用正在退出：立即结束循环。
    Stop,
}

/// U06：定时采集的等待信号——「退出中断」与「设置变更唤醒」统一到一次
/// 可中断等待上（条件变量，不做分片轮询）：退出与设置保存都在毫秒级生效。
#[derive(Clone)]
struct CollectSignal {
    inner: Arc<CollectSignalInner>,
}

struct CollectSignalInner {
    /// 间隔版本：`wake()` 递增，等待者据此判断"按新间隔重新计时"。
    version: Mutex<u64>,
    cv: Condvar,
    stop: AtomicBool,
}

impl CollectSignal {
    fn new() -> Self {
        Self {
            inner: Arc::new(CollectSignalInner {
                version: Mutex::new(0),
                cv: Condvar::new(),
                stop: AtomicBool::new(false),
            }),
        }
    }

    /// 等待 `total`：被设置变更唤醒 → `Woken`；退出请求 → `Stop`；否则 `Elapsed`。
    fn wait(&self, total: Duration) -> WaitOutcome {
        let mut version = self.inner.version.lock().unwrap_or_else(|e| e.into_inner());
        let baseline = *version;
        let deadline = std::time::Instant::now() + total;
        loop {
            if self.inner.stop.load(Ordering::SeqCst) {
                return WaitOutcome::Stop;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return WaitOutcome::Elapsed;
            }
            let (guard, _) = self
                .inner
                .cv
                .wait_timeout(version, left)
                .unwrap_or_else(|e| e.into_inner());
            version = guard;
            if *version != baseline {
                return WaitOutcome::Woken;
            }
        }
    }

    /// 设置保存成功后的唤醒：等待者按新间隔重新计时。
    fn wake(&self) {
        let mut version = self.inner.version.lock().unwrap_or_else(|e| e.into_inner());
        *version = version.wrapping_add(1);
        self.inner.cv.notify_all();
    }

    /// 退出请求：立即中断等待（幂等）。
    fn request_stop(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        self.inner.cv.notify_all();
    }
}

/// U06：一轮采集的处置。
enum CollectRound {
    /// 本轮不采集（业务未解锁、退出中或设置不可用）。
    Skipped,
    /// 完成一轮采集。
    Done(tokenscope::report::CollectOutcome),
}

/// U06：定时采集循环——**可注入**（间隔读取、采集、通知均经闭包），
/// 不依赖 `tauri::AppHandle`，因此测试可用毫秒级间隔驱动两条关键断言：
/// 改设置后按新间隔触发、采集耗时超过间隔时**不并发**（同一时刻只有一个
/// 采集在跑）。
///
/// 时序：
/// - 每轮等待前读取**当前**间隔（设置页改动下一轮即生效）；
/// - 首轮延迟 = min(间隔, 30s)；
/// - 等待排在采集**之后**，所以间隔从上一轮结束开始计时（不叠加）；
/// - 设置保存成功唤醒 → 立即按新间隔重新计时；
/// - 退出请求 → 立即结束（不忙轮询、不等满整周期）。
fn run_collect_loop<I, C, N>(
    signal: &CollectSignal,
    mut next_interval: I,
    mut collect: C,
    mut notify: N,
) where
    I: FnMut() -> Duration,
    C: FnMut() -> CollectRound,
    N: FnMut(u64),
{
    let mut first_round = true;
    loop {
        let interval = next_interval();
        let delay = if first_round {
            interval.min(COLLECT_FIRST_DELAY_CAP)
        } else {
            interval
        };
        match signal.wait(delay) {
            WaitOutcome::Stop => break,
            WaitOutcome::Woken => {
                // 设置变更：按新间隔重新计时（不再享受首轮缩短）。
                first_round = false;
                continue;
            }
            WaitOutcome::Elapsed => {}
        }
        first_round = false;
        match collect() {
            CollectRound::Skipped => {}
            CollectRound::Done(outcome) => {
                if outcome.changed {
                    notify(outcome.generation);
                }
            }
        }
    }
}

/// 进程级信号：生产路径共用（测试自行构造实例，避免相互干扰）。
fn collect_signal() -> &'static CollectSignal {
    static SIGNAL: std::sync::OnceLock<CollectSignal> = std::sync::OnceLock::new();
    SIGNAL.get_or_init(CollectSignal::new)
}

/// U06：设置保存成功后唤醒定时线程，按新间隔重新计时（幂等；未启动线程时
/// 只是记下一次版本，不产生副作用）。
pub(crate) fn wake_collection_scheduler() {
    collect_signal().wake();
}

/// U06：向前端的变化通知载荷（字段名 `generation`，与前端约定一致）。
#[derive(Clone, serde::Serialize)]
struct UsageUpdated {
    generation: u64,
}

/// U06：每轮的生效间隔——每轮从设置读取；手工写入的非法值由 settings 层
/// 回退默认并 warn。设置整体不可用时按默认间隔计时（不重复 warn：同一原因
/// 由本轮采集的跳过告警给出），只记 debug 说明间隔来源。
fn collect_interval() -> Duration {
    match commands::load_settings_strict() {
        Ok(s) => Duration::from_secs(u64::from(tokenscope::settings::collect_interval_secs_from(
            &s,
        ))),
        Err(e) => {
            let fallback = tokenscope::settings::default_collect_interval_secs();
            log::debug!("定时采集间隔读取失败，本轮按默认 {fallback} 秒计时: {e:#}");
            Duration::from_secs(u64::from(fallback))
        }
    }
}

/// U06：一轮采集（装配层只做"解锁判定 → 读设置 → collect_only"的接线）。
fn collect_round(app: &tauri::AppHandle) -> CollectRound {
    let phase = privacy::phase_of(app);
    if !phase.allows_business() {
        // 未同意/退出中：不打开历史库、不读任何来源日志。
        log::debug!(
            "定时采集轮询：phase={} → 跳过（未解锁业务）",
            phase.as_str()
        );
        return CollectRound::Skipped;
    }
    let loaded = commands::load_settings_strict();
    match commands::collection_opts_from(loaded) {
        Err(e) => {
            log::warn!("定时采集跳过（设置不可用）: {e}");
            CollectRound::Skipped
        }
        Ok(mut opts) => {
            // 自动触发：采集路径的 info 降为 debug（唯一例外是确有写入的摘要）。
            opts.trigger = tokenscope::report::Trigger::Auto;
            match tokenscope::report::collect_only(&opts) {
                Ok(outcome) => CollectRound::Done(outcome),
                Err(e) => {
                    tokenscope::logging::log_error("定时采集失败", &e);
                    CollectRound::Skipped
                }
            }
        }
    }
}

static COLLECT_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn start_periodic_collection(app: tauri::AppHandle) {
    if COLLECT_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let signal = collect_signal().clone();
    std::thread::spawn(move || {
        let app_for_loop = app;
        run_collect_loop(
            &signal,
            collect_interval,
            || collect_round(&app_for_loop),
            |generation| {
                // 只有本轮确有新增或更新才通知（`collect_only` 的 changed 判定）。
                log::debug!("定时采集有变化：generation={generation}，通知界面刷新");
                if let Err(e) = app_for_loop.emit("usage-updated", UsageUpdated { generation }) {
                    log::warn!("发送用量变化通知失败: {e}");
                }
            },
        );
        log::info!("定时采集线程已退出");
    });
}

/// 退出路径：请求停止定时采集，并等待**已经开始的提交**完成（有限等待，
/// 超时即不再阻塞退出——已提交的事务在库内，未提交的由 SQLite 事务回滚）。
/// 等待观察在共享采集边界上（N06）：手动刷新、重扫与定时采集的领队工作
/// 全部登记，任何入口的进行中提交都被等待。
pub(crate) fn stop_periodic_collection(wait: std::time::Duration) {
    // U06：退出中断与设置唤醒共用同一次可中断等待——先让定时线程立即结束等待。
    collect_signal().request_stop();
    let deadline = std::time::Instant::now() + wait;
    while tokenscope::report::active_collection_submissions() > 0
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn exit_after_saving(save: impl FnOnce(), exit: impl FnOnce()) {
    save();
    exit();
}

static WINDOW_SAVE_LOCK: Mutex<()> = Mutex::new(());

/// 后台调用：所有直接退出入口都等待最终状态保存，不经过关闭询问。
pub(crate) fn quit_with_final_save(app: &tauri::AppHandle) {
    // H04：先请定时采集收尾（等待已开始的提交），再落窗口状态并退出。
    stop_periodic_collection(std::time::Duration::from_secs(5));
    exit_after_saving(|| persist_window_state(app), || app.exit(0));
}

// ── AP07：关闭请求协调 ────────────────────────────────────
// 关窗事件入口必须**立即返回**（配置读取与状态落盘都是磁盘 IO），且同一
// 时刻只处理一个关闭请求：协调期间的重复关窗被忽略而不是并发执行或提前
// 退出。窗口动作（hide/退出）仍由主线程执行——后台线程只调度并等待结果。

/// 一次关闭请求的执行结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CloseOutcome {
    /// 已隐藏窗口（窗口在托盘保留）。
    Minimized,
    /// 已通知前端询问（窗口保持可见）。
    AskRequested,
    /// 已按设置退出进程。
    QuitRequested,
    /// 已记忆的最小化失败：窗口保持可见，原因需上报给用户重试。
    MinimizeFailed(String),
}

/// AP07：关闭请求单飞门（进程级）——协调期间重复关窗不并发执行、不排队。
struct CloseGate {
    busy: AtomicBool,
}

impl CloseGate {
    const fn new() -> Self {
        Self {
            busy: AtomicBool::new(false),
        }
    }

    /// true = 本次取得执行权；false = 已有请求在处理（本次忽略）。
    fn try_begin(&self) -> bool {
        !self.busy.swap(true, Ordering::SeqCst)
    }

    fn finish(&self) {
        self.busy.store(false, Ordering::SeqCst);
    }
}

static CLOSE_GATE: CloseGate = CloseGate::new();

/// AP07：在后台线程执行一次关闭协调；返回 None 表示已有请求在处理
///（调用方=关窗事件，本身不做任何阻塞工作）。
fn spawn_close_work(
    work: impl FnOnce() -> CloseOutcome + Send + 'static,
) -> Option<std::thread::JoinHandle<CloseOutcome>> {
    if !CLOSE_GATE.try_begin() {
        log::debug!("关闭请求处理中，忽略重复触发");
        return None;
    }
    Some(std::thread::spawn(move || {
        let outcome = work();
        CLOSE_GATE.finish();
        outcome
    }))
}

/// AP07：关闭决策的执行内核（窗口动作注入以便测试）。最小化失败**必须**
/// 返回可上报的原因，绝不静默吞掉——否则用户点了关闭后窗口既不隐藏也
/// 没有任何反馈。
fn execute_close_decision(
    decision: commands::CloseDecision,
    hide: impl FnOnce() -> Result<(), String>,
) -> CloseOutcome {
    match decision {
        commands::CloseDecision::Minimize => match hide() {
            Ok(()) => {
                log::info!("窗口关闭：按设置最小化到托盘");
                CloseOutcome::Minimized
            }
            Err(e) => CloseOutcome::MinimizeFailed(format!("{e}（可重试，或改用「直接退出」）")),
        },
        commands::CloseDecision::Quit => CloseOutcome::QuitRequested,
        commands::CloseDecision::Ask => CloseOutcome::AskRequested,
    }
}

/// AP07：需要向用户上报的关闭失败（None = 无需上报）。
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct CloseActionFailure {
    pub action: &'static str,
    pub reason: String,
}

fn close_failure_payload(outcome: &CloseOutcome) -> Option<CloseActionFailure> {
    match outcome {
        CloseOutcome::MinimizeFailed(reason) => Some(CloseActionFailure {
            action: "minimize",
            reason: reason.clone(),
        }),
        _ => None,
    }
}

/// 主线程上执行一次窗口隐藏并取回结果（窗口 API 只能在主线程调用）。
fn hide_main_window_on_main_thread(app: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(feature = "acceptance")]
    if acceptance::fail_hide_once() {
        return Err("隐藏窗口失败: acceptance-hide-once".to_string());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let app_for_main = app.clone();
    app.run_on_main_thread(move || {
        let out = match app_for_main.get_webview_window("main") {
            Some(w) => w
                .hide()
                .map(|()| emit_window_visibility(&w))
                .map_err(|e| format!("隐藏窗口失败: {e}")),
            None => Err("主窗口不存在，无法最小化".to_string()),
        };
        let _ = tx.send(out);
    })
    .map_err(|e| format!("调度窗口隐藏失败: {e}"))?;
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(r) => r,
        Err(e) => Err(format!("等待窗口隐藏结果失败: {e}")),
    }
}

/// AP07：一次关闭请求的完整处理（在后台线程运行）——读设置（磁盘）、
/// 调度窗口动作、必要时上报失败；退出路径先完成最终落盘再退出。
fn run_close_coordination(app: &tauri::AppHandle) -> CloseOutcome {
    let decision = commands::close_decision_now();
    let app_for_hide = app.clone();
    let outcome = execute_close_decision(decision, move || {
        hide_main_window_on_main_thread(&app_for_hide)
    });
    match &outcome {
        CloseOutcome::QuitRequested => {
            log::info!("窗口关闭：按设置直接退出");
            // 最终保存**完成之后**才退出：不是 fire-and-forget。
            quit_with_final_save(app);
        }
        CloseOutcome::AskRequested => {
            // 前端未就绪时事件无人接收：窗口保持打开（不静默退出）。
            if let Err(e) = app.emit("close-requested", ()) {
                log::warn!("发送关闭询问事件失败: {e}");
            }
        }
        CloseOutcome::MinimizeFailed(reason) => log::warn!("已记忆的最小化失败: {reason}"),
        CloseOutcome::Minimized => {}
    }
    outcome
}

/// AP07：关窗事件入口——立即返回，重活全部在后台线程。
fn request_close(window: &tauri::WebviewWindow) {
    let app = window.app_handle().clone();
    let handle = spawn_close_work({
        let app = app.clone();
        move || run_close_coordination(&app)
    });
    let Some(handle) = handle else { return };
    std::thread::spawn(move || {
        let outcome = handle.join().unwrap_or(CloseOutcome::AskRequested);
        if let Some(payload) = close_failure_payload(&outcome) {
            log::warn!("关闭动作失败，向界面请求重试: {}", payload.reason);
            if let Err(e) = app.emit("close-action-failed", payload) {
                log::warn!("上报关闭失败事件失败: {e}");
            }
        }
    });
}

/// 每秒检查脏标记并落盘（节流）。
fn start_window_state_saver(app: &tauri::AppHandle) {
    let state = state_mutex(app);
    let dirty = dirty_flag(app);
    let path = window_state::state_path().ok();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            // 与退出保存串行，必须先取得锁再取快照，避免旧快照后写覆盖新值。
            let _save_guard = WINDOW_SAVE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            if !dirty.swap(false, Ordering::Relaxed) {
                continue;
            }
            let (Some(path), Some(ws)) = (path.clone(), state.lock().unwrap().clone()) else {
                continue;
            };
            if let Err(e) = window_state::save(&path, &ws) {
                dirty.store(true, Ordering::Relaxed);
                log::warn!("窗口状态保存失败: {e:#}");
            }
        }
    });
}

// ── 价格定时同步（M11）──────────────────────────────────
// 默认开启、每 24h 检查一次：距上次成功同步 ≥24h 则后台同步双源并重建
// 索引。轮询而非精确定时（系统休眠会漂移）；关闭开关后完全不联网。
// P03：线程只在业务解锁（Ready）后启动，未同意时不联网、不读价格快照。
const PRICE_SYNC_INTERVAL_HOURS: i64 = 24;

fn start_price_auto_sync(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        // 首查延迟 2 分钟，避开启动瞬间的磁盘与网络竞争；从业务解锁开始计时。
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
            let (age_hours, age_note) = match last_sync_age_hours(&modelsdev_snapshot) {
                Ok(hours) => (Some(hours), format!("{hours}h")),
                Err(reason) => (None, format!("未知（{reason}）")),
            };
            // P02/P03：闸门也是这一层的判据——非 Ready（含退出中）永不联网。
            let phase = privacy::phase_of(&app);
            let run = privacy::price_sync_due(phase, auto, age_hours, PRICE_SYNC_INTERVAL_HOURS);
            log::debug!(
                "价格同步轮询：phase={} auto={auto} 距上次同步={age_note}（阈值 {PRICE_SYNC_INTERVAL_HOURS}h）→ {}",
                phase.as_str(),
                if run { "同步" } else { "跳过" }
            );
            if run {
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
            "quit" => {
                let app = app.clone();
                std::thread::spawn(move || quit_with_final_save(&app));
            }
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
        emit_window_visibility(&w);
    }
}

/// 失焦不等于隐藏。宿主状态覆盖最小化与托盘隐藏，不依赖 WebView DOM 事件。
fn emit_window_visibility(window: &tauri::WebviewWindow) {
    if privacy::phase_of(window.app_handle()) != privacy::Phase::Ready {
        return;
    }
    if let (Ok(shown), Ok(minimized)) = (window.is_visible(), window.is_minimized()) {
        let _ = window.emit("window-visibility", shown && !minimized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn quit_waits_for_final_window_save() {
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (exit_tx, exit_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            exit_after_saving(
                || {
                    entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                },
                || exit_tx.send(()).unwrap(),
            );
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(exit_rx.try_recv().is_err(), "保存仍挂起时不得退出");
        release_tx.send(()).unwrap();
        exit_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
    }

    /// AP07：已记忆的最小化失败必须可上报（用户看到原因并可重试/取消），
    /// 成功的隐藏与退出路径都不上报；退出路径不调用窗口隐藏。
    #[test]
    fn remembered_minimize_failure_is_reported_and_retryable() {
        let failed = execute_close_decision(commands::CloseDecision::Minimize, || {
            Err("隐藏窗口失败: webview busy".to_string())
        });
        let payload = close_failure_payload(&failed).expect("失败必须可上报给界面");
        assert_eq!(payload.action, "minimize");
        assert!(
            payload.reason.contains("webview busy"),
            "必须携带原始原因: {}",
            payload.reason
        );
        assert!(
            payload.reason.contains("可重试"),
            "必须标注可重试: {}",
            payload.reason
        );

        let ok = execute_close_decision(commands::CloseDecision::Minimize, || Ok(()));
        assert_eq!(ok, CloseOutcome::Minimized);
        assert!(close_failure_payload(&ok).is_none(), "成功路径不得上报失败");

        // 与未记忆的 close_resolve 路径共用结果处理：询问本身不是失败。
        let ask = execute_close_decision(commands::CloseDecision::Ask, || Ok(()));
        assert_eq!(ask, CloseOutcome::AskRequested);
        assert!(close_failure_payload(&ask).is_none());

        let quit = execute_close_decision(commands::CloseDecision::Quit, || {
            panic!("退出路径不得调用窗口隐藏")
        });
        assert_eq!(quit, CloseOutcome::QuitRequested);
        assert!(close_failure_payload(&quit).is_none());
    }

    /// AP07：关窗事件入口不得等待配置读取/状态写入；协调期间的重复关窗
    /// 不并发执行也不提前退出；一轮结束后可再次关闭。
    #[test]
    fn close_handler_does_not_wait_for_settings_or_state_io() {
        let gate: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
        let g = gate.clone();
        let started = Instant::now();
        let handle = spawn_close_work(move || {
            // 模拟被 barrier 挂起的设置读取与窗口状态写入。
            let (m, cv) = &*g;
            let mut released = m.lock().unwrap();
            while !*released {
                released = cv.wait(released).unwrap();
            }
            CloseOutcome::AskRequested
        })
        .expect("首次关闭请求必须被接受");
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "事件入口不得等待配置读取或状态写入"
        );
        assert!(
            spawn_close_work(|| CloseOutcome::QuitRequested).is_none(),
            "协调期间的重复关窗不得并发执行或提前退出"
        );

        {
            let (m, cv) = &*gate;
            *m.lock().unwrap() = true;
            cv.notify_all();
        }
        assert_eq!(
            handle.join().unwrap(),
            CloseOutcome::AskRequested,
            "后续结果单次返回"
        );

        let again = spawn_close_work(|| CloseOutcome::QuitRequested)
            .expect("上一轮结束后必须可再次关闭（不残留 in-flight 状态）");
        assert_eq!(again.join().unwrap(), CloseOutcome::QuitRequested);
    }
    /// N06 验收（壳侧）：退出等待覆盖**真实**历史提交——进行中的采集提交期间
    /// 调用 `stop_periodic_collection`，它必须等到提交完成（事实持久化）才返回；
    /// 无进行中提交时立即返回。大来源量让真实采集持续足够久，等待行为可观察。
    #[test]
    fn stop_periodic_collection_waits_for_real_submission() {
        let dir = std::env::temp_dir().join(format!(
            "tokenscope-shell-exit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let claude = dir.join("projects");
        // 120 个独立日志文件：真实采集（发现 + 解析 + 历史库写入）持续足够久。
        for i in 0..300 {
            let f = claude.join("proj").join(format!("s{i}.jsonl"));
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(
            &f,
            format!(
                r#"{{"type":"assistant","timestamp":"2026-09-15T08:{:02}:{:02}.000Z","sessionId":"s{i}","cwd":"C:/work/alpha","message":{{"id":"m-{i}","model":"claude-opus-5-5","usage":{{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#,
                (i / 60) % 60,
                i % 60
            ),
        )
        .unwrap();
        }
        let opts = tokenscope::report::SummaryOptions {
            by: tokenscope::aggregate::GroupBy::Day,
            claude_dir: Some(claude.clone()),
            codex_dir: Some(dir.join("no-codex")),
            claude_enabled: Some(true),
            codex_enabled: Some(false),
            cache_dir: Some(dir.join("data")),
            pricing_index: Some(dir.join("idx.json")),
            pricing_path: Some(dir.join("pricing.toml")),
            openrouter_path: Some(dir.join("no-or.json")),
            modelsdev_path: Some(dir.join("no-md.json")),
            tz: Some("UTC".to_string()),
            ..Default::default()
        };

        let o = opts.clone();
        let worker = std::thread::spawn(move || tokenscope::report::summary(&o).unwrap());

        // 等真实提交在共享边界登记（N06 计数），再触发退出等待。
        let deadline = Instant::now() + Duration::from_secs(30);
        while tokenscope::report::active_collection_submissions() == 0 {
            assert!(
                Instant::now() < deadline,
                "真实采集必须经共享边界登记为进行中提交"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        // 退出等待：直到提交完成（计数归零）才返回。
        stop_periodic_collection(Duration::from_secs(30));
        assert_eq!(
            tokenscope::report::active_collection_submissions(),
            0,
            "退出等待返回时不得仍有进行中提交"
        );
        let report = worker.join().unwrap();
        assert_eq!(report.totals.requests, 300);

        // 事实已持久化：重新打开历史库读得到全部事件。
        let history =
            tokenscope::history::HistoryDb::open(&dir.join("data").join("history.db")).unwrap();
        assert_eq!(
            history.event_count().unwrap(),
            300,
            "退出等待覆盖的提交必须完整落库"
        );

        // 无进行中提交：等待立即返回（有限等待不空转）。
        let t = Instant::now();
        stop_periodic_collection(Duration::from_secs(30));
        assert!(t.elapsed() < Duration::from_secs(1), "无提交时立即返回");

        std::fs::remove_dir_all(&dir).ok();
    }

    // ── U06：定时采集调度（可注入间隔与采集实现）────────────────

    /// 收满 `n` 轮触发时刻；超时即断言失败（不静默跳过）。
    fn drain_triggers(rx: &std::sync::mpsc::Receiver<Instant>, n: usize) -> Vec<Instant> {
        let mut out = Vec::new();
        while out.len() < n {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(t) => out.push(t),
                Err(e) => panic!("等待第 {} 轮触发超时: {e}", out.len() + 1),
            }
        }
        out
    }

    /// U06：设置保存成功后的唤醒必须让定时线程**按新间隔重新计时**，
    /// 而不是等完旧间隔；首轮延迟 = min(间隔, 30s)；后续间隔不叠加。
    #[test]
    fn collect_loop_follows_updated_interval_after_wake() {
        let signal = CollectSignal::new();
        let interval_ms = Arc::new(std::sync::atomic::AtomicU64::new(500));
        let (tx, rx) = std::sync::mpsc::channel::<Instant>();
        let handle = std::thread::spawn({
            let signal = signal.clone();
            let interval_ms = interval_ms.clone();
            move || {
                run_collect_loop(
                    &signal,
                    || Duration::from_millis(interval_ms.load(Ordering::SeqCst)),
                    || {
                        let _ = tx.send(Instant::now());
                        CollectRound::Done(tokenscope::report::CollectOutcome {
                            generation: 1,
                            changed: false,
                        })
                    },
                    |_| {},
                )
            }
        });

        let first = drain_triggers(&rx, 2);
        let before = first[1].duration_since(first[0]);
        assert!(
            before >= Duration::from_millis(350),
            "初始间隔应约 500ms（首轮延迟 = min(间隔, 30s)）：{before:?}"
        );

        // 等价于 settings_set_collect_interval 保存成功后的唤醒。
        interval_ms.store(60, Ordering::SeqCst);
        signal.wake();

        let after = drain_triggers(&rx, 2);
        let woken = after[0].duration_since(first[1]);
        assert!(
            woken < Duration::from_millis(300),
            "唤醒后必须按新间隔重新计时（实测 {woken:?}，旧间隔 500ms）"
        );
        let next = after[1].duration_since(after[0]);
        assert!(
            next >= Duration::from_millis(20) && next < Duration::from_millis(400),
            "后续轮次按新间隔触发（不叠加）：{next:?}"
        );

        signal.request_stop();
        handle.join().unwrap();
    }

    /// U06：单轮采集耗时超过间隔时**不并发**——同一时刻只有一个采集在跑，
    /// 轮数由“采集耗时 + 间隔”决定，而不是按间隔堆积。
    #[test]
    fn collect_loop_rounds_never_overlap_when_slow() {
        let signal = CollectSignal::new();
        let running = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let max_running = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let rounds = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let handle = std::thread::spawn({
            let signal = signal.clone();
            let running = running.clone();
            let max_running = max_running.clone();
            let rounds = rounds.clone();
            move || {
                run_collect_loop(
                    &signal,
                    || Duration::from_millis(20),
                    || {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        max_running.fetch_max(now, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(80));
                        running.fetch_sub(1, Ordering::SeqCst);
                        rounds.fetch_add(1, Ordering::SeqCst);
                        CollectRound::Done(tokenscope::report::CollectOutcome {
                            generation: 0,
                            changed: false,
                        })
                    },
                    |_| {},
                )
            }
        });

        std::thread::sleep(Duration::from_millis(600));
        signal.request_stop();
        handle.join().unwrap();

        assert_eq!(
            max_running.load(Ordering::SeqCst),
            1,
            "同一时刻只能有一个采集在跑（同轮不得重叠）"
        );
        let n = rounds.load(Ordering::SeqCst);
        assert!(n >= 2, "慢采集期间仍要按间隔轮转（实测 {n} 轮）");
        assert!(
            n <= 10,
            "轮数必须由“采集耗时 + 间隔”决定（600ms / 100ms ≈ 6，实测 {n} 轮）"
        );
    }

    /// U06：只有历史库确有写入的轮次才发变化通知，恰好一次且携带该轮
    /// generation；无变化的轮次不发。
    #[test]
    fn collect_loop_notifies_only_on_change() {
        let signal = CollectSignal::new();
        let (tx, rx) = std::sync::mpsc::channel::<u64>();
        let rounds = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let handle = std::thread::spawn({
            let signal = signal.clone();
            let rounds = rounds.clone();
            move || {
                run_collect_loop(
                    &signal,
                    || Duration::from_millis(10),
                    || {
                        let n = rounds.fetch_add(1, Ordering::SeqCst) + 1;
                        CollectRound::Done(tokenscope::report::CollectOutcome {
                            generation: n,
                            changed: n.is_multiple_of(2),
                        })
                    },
                    move |generation| {
                        let _ = tx.send(generation);
                    },
                )
            }
        });

        let mut events = Vec::new();
        for _ in 0..3 {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(g) => events.push(g),
                Err(e) => panic!("等待变化通知超时: {e}"),
            }
        }
        signal.request_stop();
        handle.join().unwrap();

        assert_eq!(
            events,
            vec![2, 4, 6],
            "只有 changed 轮次发通知，且携带该轮 generation"
        );
        assert!(
            rounds.load(Ordering::SeqCst) >= 6,
            "收到第 3 个变化通知时至少已完成 6 轮（其中 3 轮无变化、未发通知）"
        );
        let extra: Vec<u64> = rx.try_iter().collect();
        assert!(
            extra.iter().all(|g| g.is_multiple_of(2)),
            "无变化轮次不得发通知：{extra:?}"
        );
    }

    /// U06：退出中断与设置唤醒共用同一次等待——请求停止后立即结束等待
    ///（不忙轮询、不等满整个周期）。
    #[test]
    fn collect_signal_wait_returns_immediately_on_stop() {
        let signal = CollectSignal::new();
        let waiter = {
            let signal = signal.clone();
            std::thread::spawn(move || {
                let t = Instant::now();
                let outcome = signal.wait(Duration::from_secs(30));
                (outcome, t.elapsed())
            })
        };
        std::thread::sleep(Duration::from_millis(50));
        signal.request_stop();
        let (outcome, elapsed) = waiter.join().unwrap();
        assert_eq!(outcome, WaitOutcome::Stop, "停止必须中断等待");
        assert!(
            elapsed < Duration::from_secs(2),
            "停止必须立即中断等待（实测 {elapsed:?}）"
        );
    }

    /// PR2：采集状态事件桥只注册一次——重复安装不产生第二个观察者
    ///（内核侧已证明观察者收到全部状态转换；这里守住"恰好一次"语义）。
    #[test]
    fn status_bridge_installs_exactly_once() {
        use std::sync::atomic::AtomicBool;
        let tracker = tokenscope::collection_status::CollectionStatusTracker::new();
        let installed = AtomicBool::new(false);
        let sink: std::sync::Arc<dyn tokenscope::collection_status::CollectionObserver> =
            std::sync::Arc::new(CountingSink {
                calls: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            });
        assert!(
            install_status_bridge_once(&installed, &tracker, sink),
            "首次安装必须完成注册"
        );
        assert!(
            !install_status_bridge_once(
                &installed,
                &tracker,
                std::sync::Arc::new(CountingSink {
                    calls: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                }),
            ),
            "重复安装不得重复注册"
        );
    }

    struct CountingSink {
        calls: std::sync::Arc<std::sync::atomic::AtomicU64>,
    }

    impl tokenscope::collection_status::CollectionObserver for CountingSink {
        fn on_collection_status(&self, _status: &tokenscope::collection_status::CollectionStatus) {
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }
}
