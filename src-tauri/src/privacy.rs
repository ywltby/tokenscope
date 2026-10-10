//! 首次启动隐私同意：进程内生命周期、引导 IPC 与业务闸门（P02/P03）。
//!
//! 不变量（先于实现登记）：
//! 1. 只有「真实 settings.toml 中同意字段为 true」才可能进入业务；UI 可见性、
//!    前端布尔值、事件伪造都不构成授权——闸门在后端；
//! 2. 顺序固定：用户点击同意 → `settings` 事务原子保存成功 → 才初始化业务
//!    （日志/窗口状态/托盘/价格线程）；初始化完成才允许业务命令；
//! 3. 读取或保存失败保持封闭，不覆盖损坏文件、不回退成已同意；
//! 4. 全部业务命令在进入重活闭包**之前**经 [`PrivacyState::require_ready`]
//!    拦截；未 Ready 的命令不落任何文件/网络日志；
//! 5. 引导期的关闭请求不读取已记忆的关闭动作、不保存窗口状态、不最小化。

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokenscope::settings::ConsentRead;

/// 打包进程序的政策全文（构建期编译，运行时不访问 GitHub 或任何网络）。
pub const POLICY_MARKDOWN: &str = include_str!("../../docs/privacy.md");

/// 未同意时业务命令的统一拒绝码（前端据此给出可操作提示）。
pub const ERR_CONSENT_REQUIRED: &str = "privacy_consent_required";

/// 引导期关闭请求事件名（前端监听前到达的请求由 bootstrap 响应的
/// `exitPromptPending` 标记恢复显示）。
pub const EXIT_EVENT: &str = "privacy-exit-requested";

/// 等待另一个引导动作落定的上限（超过则把当前状态交回前端，由其重试）。
const SETTLE_TIMEOUT: Duration = Duration::from_secs(60);

/// 引导命令：未 Ready 时也可以调用（唯一不设闸门的 IPC 面）。
/// 由命令覆盖测试消费（生产代码不需要遍历它）。
#[cfg_attr(not(test), allow(dead_code))]
pub const BOOTSTRAP_COMMANDS: &[&str] = &[
    "privacy_bootstrap",
    "privacy_accept",
    "privacy_exit_resolve",
];

/// 业务命令：必须先通过 [`PrivacyState::require_ready`]。
///
/// 这张表是命令覆盖测试的单一事实源：`lib.rs` 的 `generate_handler!` 里
/// 出现的每个命令名要么在这里，要么在 [`BOOTSTRAP_COMMANDS`]，否则
/// `privacy_all_business_commands_reject_before_ready` 失败。
#[cfg_attr(not(test), allow(dead_code))]
pub const PROTECTED_COMMANDS: &[&str] = &[
    "summarize",
    "list_events",
    "query_begin",
    "query_summary",
    "query_events",
    "startup_diagnostics",
    "view_cache_load",
    "view_cache_save",
    "source_status",
    "source_config_set",
    "cache_stats",
    "refresh_cache",
    "pricing_entries",
    "pricing_status",
    "open_pricing_file",
    "sync_pricing_openrouter",
    "autostart_status",
    "autostart_set",
    "settings_get",
    "settings_set_price_auto_sync",
    "settings_set_close_action",
    "close_resolve",
    "open_settings_file",
];

/// 引导生命周期阶段（默认封闭：`Checking` 也不是 Ready）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// 正在检查必要设置。
    Checking,
    /// 等待用户在隐私政策上选择。
    NeedsConsent,
    /// 用户已点击同意，原子保存尚未成功。
    SavingConsent,
    /// 已有可靠同意记录，正在单次初始化业务。
    Starting,
    /// 允许业务命令与后台任务。
    Ready,
    /// 读取/保存/初始化失败：继续封闭，等用户修复后重试。
    BlockedError,
    /// 用户确认退出：拒绝新的同意请求，晚到响应不得复活进程。
    Exiting,
}

impl Phase {
    /// 前端消费的稳定字符串（snake_case，与 DTO 命名一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Checking => "checking",
            Phase::NeedsConsent => "needs_consent",
            Phase::SavingConsent => "saving_consent",
            Phase::Starting => "starting",
            Phase::Ready => "ready",
            Phase::BlockedError => "blocked_error",
            Phase::Exiting => "exiting",
        }
    }

    /// 是否允许业务读取/写入/联网。
    pub fn allows_business(self) -> bool {
        matches!(self, Phase::Ready)
    }
}

/// 运行时一次性资源：每个资源有独立成功标记，重试只补未完成部分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStep {
    /// 文件日志（失败按既有非阻断降级处理）。
    Logging,
    /// 窗口状态恢复。
    WindowState,
    /// 窗口状态节流保存线程。
    Saver,
    /// 托盘图标。
    Tray,
    /// 价格自动同步线程（120 秒等待从业务解锁开始计时）。
    PriceSync,
}

impl RuntimeStep {
    /// 全部步骤（顺序即初始化顺序）。
    pub const ALL: [RuntimeStep; 5] = [
        RuntimeStep::Logging,
        RuntimeStep::WindowState,
        RuntimeStep::Saver,
        RuntimeStep::Tray,
        RuntimeStep::PriceSync,
    ];
}

/// 已完成标记（跨重试保持；不因一次失败而重复启动线程/托盘）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeFlags {
    logging: bool,
    window_state: bool,
    saver: bool,
    tray: bool,
    price_sync: bool,
}

impl RuntimeFlags {
    /// 尚未完成的步骤（初始化按此顺序补齐）。
    pub fn pending(&self) -> Vec<RuntimeStep> {
        RuntimeStep::ALL
            .into_iter()
            .filter(|step| !self.done(*step))
            .collect()
    }

    pub fn done(&self, step: RuntimeStep) -> bool {
        match step {
            RuntimeStep::Logging => self.logging,
            RuntimeStep::WindowState => self.window_state,
            RuntimeStep::Saver => self.saver,
            RuntimeStep::Tray => self.tray,
            RuntimeStep::PriceSync => self.price_sync,
        }
    }

    pub fn mark(&mut self, step: RuntimeStep) {
        match step {
            RuntimeStep::Logging => self.logging = true,
            RuntimeStep::WindowState => self.window_state = true,
            RuntimeStep::Saver => self.saver = true,
            RuntimeStep::Tray => self.tray = true,
            RuntimeStep::PriceSync => self.price_sync = true,
        }
    }
}

/// 打包进程序的政策（离线完整可读）。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyInfo {
    /// 标题（取自政策首个一级标题）。
    pub title: String,
    /// 政策日期（取自「更新日期：」行）。
    pub date: String,
    /// 政策全文（Markdown 源文本；前端做确定性渲染，不引入远程资源）。
    pub markdown: String,
}

/// 引导状态的只读 DTO。不携带来源目录清单、缓存内容、历史查询或价格状态。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySnapshot {
    /// checking | needs_consent | saving_consent | starting | ready | blocked_error | exiting
    pub phase: &'static str,
    /// 阻断原因（`blocked_error` 时非空，含路径与可操作修复指引）。
    pub detail: Option<String>,
    /// 关闭事件在前端监听就绪前到达：由 bootstrap 响应恢复显示退出确认。
    pub exit_prompt_pending: bool,
    /// 非阻断的初始化诊断（日志降级、托盘失败等）。
    pub diagnostics: Vec<String>,
    /// 打包进程序的政策全文。
    pub policy: PolicyInfo,
}

/// 窗口事件在引导期应执行的动作（纯函数，便于测试「未 Ready 不落盘」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEventAction {
    /// 走既有业务路径（更新内存态、落盘、关闭协调）。
    Business,
    /// 引导期关闭请求：记录待处理标记并提示前端，绝不做业务动作。
    PromptExit,
    /// 引导期尺寸/位置事件：忽略（不更新内存、不置脏、不落盘）。
    Ignore,
}

/// 关闭请求的处置（非 Ready 一律进入引导退出询问）。
pub fn close_event_action(phase: Phase) -> WindowEventAction {
    if phase.allows_business() {
        WindowEventAction::Business
    } else {
        WindowEventAction::PromptExit
    }
}

/// 尺寸/位置事件的处置（非 Ready 一律忽略，避免窗口状态落盘）。
pub fn geometry_event_action(phase: Phase) -> WindowEventAction {
    if phase.allows_business() {
        WindowEventAction::Business
    } else {
        WindowEventAction::Ignore
    }
}

/// 价格自动同步是否应在本轮执行：**未 Ready 永不联网**，与设置开关、
/// 快照年龄共同决定。
pub fn price_sync_due(
    phase: Phase,
    auto_sync: bool,
    age_hours: Option<i64>,
    interval_hours: i64,
) -> bool {
    phase.allows_business() && auto_sync && age_hours.is_none_or(|h| h >= interval_hours)
}

/// 引导所需的外部副作用（生产实现读真实设置并初始化真实运行时；
/// 测试注入 fake 以断言调用次数与顺序）。
pub(crate) trait GateIo: Send + Sync {
    /// 严格读取同意记录——引导期**唯一**允许的磁盘读取。
    fn read_consent(&self) -> ConsentRead;
    /// 用户明确点击同意后的原子保存（失败即保持封闭）。
    fn save_accept(&self) -> Result<(), String>;
    /// 同意成立后的单次业务初始化；返回非阻断诊断。
    fn init_runtime(&self) -> Result<Vec<String>, String>;
}

struct Inner {
    phase: Phase,
    /// 是否有引导动作正在进行（检查/保存/初始化同一时刻只允许一个）。
    in_progress: bool,
    /// 阻断原因（BlockedError 时非空）。
    detail: Option<String>,
    /// 前端监听就绪前的关闭请求。
    exit_prompt_pending: bool,
    /// 非阻断诊断累积。
    diagnostics: Vec<String>,
}

impl Inner {
    fn new() -> Self {
        Self {
            phase: Phase::Checking,
            in_progress: false,
            detail: None,
            exit_prompt_pending: false,
            diagnostics: Vec::new(),
        }
    }
}

struct Core {
    inner: Mutex<Inner>,
    flags: Mutex<RuntimeFlags>,
    settle: Condvar,
}

/// 进程内引导状态（Tauri managed state；clone 即共享同一状态）。
#[derive(Clone)]
pub struct PrivacyState(Arc<Core>);

impl Default for PrivacyState {
    fn default() -> Self {
        Self::new()
    }
}

impl PrivacyState {
    /// 新进程：`Checking`（封闭）。
    pub fn new() -> Self {
        Self(Arc::new(Core {
            inner: Mutex::new(Inner::new()),
            flags: Mutex::new(RuntimeFlags::default()),
            settle: Condvar::new(),
        }))
    }

    fn lock_inner(&self) -> MutexGuard<'_, Inner> {
        self.0.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn phase(&self) -> Phase {
        self.lock_inner().phase
    }

    /// 业务闸门：未 Ready 时返回统一错误码，调用方**不得**继续执行重活。
    pub fn require_ready(&self) -> Result<(), String> {
        let phase = self.phase();
        if phase.allows_business() {
            return Ok(());
        }
        let reason = match phase {
            Phase::Checking => "正在检查隐私设置，业务操作暂不可用",
            Phase::NeedsConsent => "尚未同意隐私政策：应用不会读取使用数据或同步价格",
            Phase::SavingConsent => "正在保存隐私政策同意记录，请稍候",
            Phase::Starting => "正在初始化，请稍候",
            Phase::BlockedError => "隐私设置无法确认或未保存成功，业务操作已阻断",
            Phase::Exiting => "应用正在退出",
            Phase::Ready => "",
        };
        Err(format!("{ERR_CONSENT_REQUIRED}：{reason}"))
    }

    /// 只读快照（含打包政策）。
    pub fn snapshot(&self) -> PrivacySnapshot {
        let g = self.lock_inner();
        PrivacySnapshot {
            phase: g.phase.as_str(),
            detail: g.detail.clone(),
            exit_prompt_pending: g.exit_prompt_pending,
            diagnostics: g.diagnostics.clone(),
            policy: policy_info(),
        }
    }

    fn publish(&self, phase: Phase, detail: Option<String>) {
        let mut g = self.lock_inner();
        g.phase = phase;
        g.detail = detail;
    }

    /// 进入 Ready（诊断累积保留）。
    fn publish_ready(&self, diagnostics: Vec<String>) {
        let mut g = self.lock_inner();
        g.phase = Phase::Ready;
        g.detail = None;
        g.diagnostics.extend(diagnostics);
    }

    /// 测试/内部：直接发布阻断原因。
    pub(crate) fn publish_blocked(&self, detail: String) {
        self.publish(Phase::BlockedError, Some(detail));
    }

    /// 记录「关闭事件已到达」；返回是否首次置位。
    pub fn mark_exit_prompt_pending(&self) -> bool {
        let mut g = self.lock_inner();
        let first = !g.exit_prompt_pending;
        g.exit_prompt_pending = true;
        first
    }

    /// 用户取消退出：清除待处理标记（继续停留在政策界面）。
    pub fn clear_exit_prompt(&self) {
        self.lock_inner().exit_prompt_pending = false;
    }

    /// 进入退出流程；返回 true 表示本次调用负责真正结束进程。
    pub fn begin_exit(&self) -> bool {
        let mut g = self.lock_inner();
        if g.phase == Phase::Exiting {
            return false;
        }
        g.phase = Phase::Exiting;
        g.detail = None;
        g.in_progress = false;
        drop(g);
        self.0.settle.notify_all();
        true
    }

    fn try_acquire(&self) -> bool {
        let mut g = self.lock_inner();
        if g.in_progress {
            false
        } else {
            g.in_progress = true;
            true
        }
    }

    fn release(&self) {
        self.lock_inner().in_progress = false;
        self.0.settle.notify_all();
    }

    /// 等待正在进行的引导动作落定（超时返回，不无限等待）。
    fn wait_settled(&self, timeout: Duration) {
        let mut g = self.lock_inner();
        let deadline = Instant::now() + timeout;
        while g.in_progress {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let (next, _) = self
                .0
                .settle
                .wait_timeout(g, deadline - now)
                .unwrap_or_else(|e| e.into_inner());
            g = next;
        }
    }

    /// 在一次性资源标记上执行初始化（只补未完成部分）。
    pub(crate) fn with_flags<T>(&self, f: impl FnOnce(&mut RuntimeFlags) -> T) -> T {
        let mut g = self.0.flags.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

/// 业务命令的统一闸门包装：未 Ready 时**不执行**闭包。
/// （命令现在统一用 [`require_ready`]；本包装供测试与后续命令复用。）
#[cfg_attr(not(test), allow(dead_code))]
pub fn guarded<T>(
    state: &PrivacyState,
    work: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    state.require_ready()?;
    work()
}

/// 业务闸门的自由函数形态——命令里统一写
/// `privacy::require_ready(&privacy::state_of(&app))?;`，
/// 便于命令覆盖测试用同一模式审计新增命令。
pub fn require_ready(state: &PrivacyState) -> Result<(), String> {
    state.require_ready()
}

/// 取 managed state（clone 后可在后台线程使用）。
pub fn state_of(app: &AppHandle) -> PrivacyState {
    app.state::<PrivacyState>().inner().clone()
}

/// 事件回调路径的阶段读取：holder 尚未注册时按 `Checking`（封闭）处理，
/// 绝不用 panic 把窗口事件变成崩溃。
pub fn phase_of(app: &AppHandle) -> Phase {
    app.try_state::<PrivacyState>()
        .map(|s| s.phase())
        .unwrap_or(Phase::Checking)
}

fn policy_info() -> PolicyInfo {
    static CACHE: std::sync::OnceLock<PolicyInfo> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut title = "TokenScope 隐私政策".to_string();
            let mut date = String::new();
            for line in POLICY_MARKDOWN.lines() {
                if let Some(rest) = line.strip_prefix("# ")
                    && !rest.trim().is_empty()
                {
                    title = rest.trim().to_string();
                    continue;
                }
                if let Some(rest) = line.strip_prefix("更新日期：") {
                    date = rest.trim().to_string();
                }
            }
            PolicyInfo {
                title,
                date,
                markdown: POLICY_MARKDOWN.to_string(),
            }
        })
        .clone()
}

// ── 状态机流转（同步内核；由命令在后台线程调用）────────────────

/// 引导检查（幂等）：已有同意 → 单次初始化进入 Ready；无记录 → 等待选择。
///
/// 重复调用、前端重载都不会重复创建线程/托盘/logger：真正的检查与初始化
/// 由单飞门保护，其他调用等待落定后返回同一状态。
pub(crate) fn run_bootstrap(state: &PrivacyState, io: &dyn GateIo) -> PrivacySnapshot {
    match state.phase() {
        Phase::Ready | Phase::Exiting => return state.snapshot(),
        _ => {}
    }
    if !state.try_acquire() {
        state.wait_settled(SETTLE_TIMEOUT);
        return state.snapshot();
    }
    match state.phase() {
        Phase::Ready | Phase::Exiting => {
            state.release();
            return state.snapshot();
        }
        _ => {}
    }
    state.publish(Phase::Checking, None);
    match io.read_consent() {
        ConsentRead::Granted => start_business_runtime(state, io),
        ConsentRead::NotGranted | ConsentRead::Missing => {
            state.publish(Phase::NeedsConsent, None);
        }
        ConsentRead::Unreadable(reason) => {
            state.publish(Phase::BlockedError, Some(unreadable_detail(&reason)));
        }
    }
    state.release();
    state.snapshot()
}

/// 用户点击「同意并继续」后的保存与初始化（串行化，重复调用不重复写入）。
pub(crate) fn run_accept(state: &PrivacyState, io: &dyn GateIo) -> Result<PrivacySnapshot, String> {
    match state.phase() {
        Phase::Ready => return Ok(state.snapshot()),
        Phase::Exiting => return Err(EXITING_MESSAGE.to_string()),
        _ => {}
    }
    if !state.try_acquire() {
        state.wait_settled(SETTLE_TIMEOUT);
        return match state.phase() {
            Phase::Ready => Ok(state.snapshot()),
            Phase::Exiting => Err(EXITING_MESSAGE.to_string()),
            Phase::BlockedError => Err(state
                .lock_inner()
                .detail
                .clone()
                .unwrap_or_else(|| "上一次同意保存未成功，请重试".to_string())),
            _ => Ok(state.snapshot()),
        };
    }
    match state.phase() {
        Phase::Ready => {
            state.release();
            return Ok(state.snapshot());
        }
        Phase::Exiting => {
            state.release();
            return Err(EXITING_MESSAGE.to_string());
        }
        _ => {}
    }
    state.publish(Phase::SavingConsent, None);
    let outcome = io.save_accept();
    match outcome {
        Ok(()) => {
            start_business_runtime(state, io);
            state.release();
            Ok(state.snapshot())
        }
        Err(e) => {
            let detail = format!("保存同意记录失败：{e}");
            state.publish(Phase::BlockedError, Some(detail.clone()));
            state.release();
            Err(detail)
        }
    }
}

/// 同意成立后的单次业务初始化：日志 → 窗口 → 托盘 → 线程。
fn start_business_runtime(state: &PrivacyState, io: &dyn GateIo) {
    state.publish(Phase::Starting, None);
    match io.init_runtime() {
        Ok(diagnostics) => state.publish_ready(diagnostics),
        Err(e) => state.publish(Phase::BlockedError, Some(format!("业务初始化失败：{e}"))),
    }
}

/// 不可确认隐私设置时的用户可操作诊断。
fn unreadable_detail(reason: &str) -> String {
    format!(
        "无法确认隐私设置：{reason}\n请修复或删除该设置文件后重试；应用在此之前不会读取使用数据。"
    )
}

const EXITING_MESSAGE: &str = "应用正在退出，无法保存隐私政策同意";

// ── 生产 GateIo（真实设置文件 + 真实运行时初始化）──────────────

struct RealGate {
    app: AppHandle,
    state: PrivacyState,
}

impl GateIo for RealGate {
    fn read_consent(&self) -> ConsentRead {
        match tokenscope::settings::settings_path() {
            Ok(path) => tokenscope::settings::read_consent(&path),
            Err(e) => ConsentRead::Unreadable(format!("无法定位设置文件路径：{e:#}")),
        }
    }

    fn save_accept(&self) -> Result<(), String> {
        let path = tokenscope::settings::settings_path()
            .map_err(|e| format!("无法定位设置文件路径：{e:#}"))?;
        tokenscope::settings::accept_privacy_policy(&path).map_err(|e| format!("{e:#}"))
    }

    fn init_runtime(&self) -> Result<Vec<String>, String> {
        self.state
            .with_flags(|flags| crate::initialize_business_runtime(&self.app, flags))
    }
}

// ── IPC：引导命令（未 Ready 时可调用）────────────────────────

/// 检查必要设置并返回状态与打包政策；重复调用幂等。
#[tauri::command]
pub async fn privacy_bootstrap(app: AppHandle) -> PrivacySnapshot {
    let state = state_of(&app);
    let io = RealGate {
        app: app.clone(),
        state: state.clone(),
    };
    let fallback = state.clone();
    match tauri::async_runtime::spawn_blocking(move || run_bootstrap(&state, &io)).await {
        Ok(snapshot) => snapshot,
        Err(e) => {
            fallback.publish_blocked(format!("引导检查失败：{e}"));
            fallback.snapshot()
        }
    }
}

/// 只响应用户明确的「同意并继续」：原子保存成功后才解锁业务。
#[tauri::command]
pub async fn privacy_accept(app: AppHandle) -> Result<PrivacySnapshot, String> {
    let state = state_of(&app);
    let io = RealGate {
        app: app.clone(),
        state: state.clone(),
    };
    match tauri::async_runtime::spawn_blocking(move || run_accept(&state, &io)).await {
        Ok(result) => result,
        Err(e) => Err(format!("保存同意记录失败：{e}")),
    }
}

/// 未解锁时的退出确认：`exit = false` 返回政策；`true` 直接结束进程。
///
/// 不读取 `close_action`、不写设置、不最小化、不做窗口状态最终保存。
#[tauri::command]
pub async fn privacy_exit_resolve(app: AppHandle, exit: bool) -> Result<(), String> {
    let state = state_of(&app);
    if !exit {
        state.clear_exit_prompt();
        return Ok(());
    }
    if state.begin_exit() {
        log::info!("未同意状态下确认退出：直接结束进程（不保存窗口状态）");
        let app = app.clone();
        std::thread::spawn(move || app.exit(0));
    }
    Ok(())
}

// ── 引导期窗口事件 ────────────────────────────────────────────

/// 未 Ready 时的关闭请求：只记录待处理标记并通知前端，不做任何业务动作。
pub fn request_exit_prompt(app: &AppHandle) {
    let Some(state) = app.try_state::<PrivacyState>().map(|s| s.inner().clone()) else {
        return;
    };
    if state.mark_exit_prompt_pending() {
        log::info!("引导期关闭请求：等待用户选择（不读取关闭设置、不写窗口状态）");
    }
    if let Err(e) = app.emit(EXIT_EVENT, ()) {
        log::debug!("引导期关闭提示事件发送失败（bootstrap 响应会恢复显示）: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// 注入式引导 IO：记录每类副作用的调用次数。
    struct FakeGate {
        consent: Mutex<ConsentRead>,
        save: Mutex<Result<(), String>>,
        init: Mutex<Result<Vec<String>, String>>,
        reads: AtomicUsize,
        saves: AtomicUsize,
        inits: AtomicUsize,
        save_delay: Duration,
    }

    impl FakeGate {
        fn new(consent: ConsentRead) -> Self {
            Self {
                consent: Mutex::new(consent),
                save: Mutex::new(Ok(())),
                init: Mutex::new(Ok(Vec::new())),
                reads: AtomicUsize::new(0),
                saves: AtomicUsize::new(0),
                inits: AtomicUsize::new(0),
                save_delay: Duration::ZERO,
            }
        }

        fn calls(&self) -> (usize, usize, usize) {
            (
                self.reads.load(Ordering::SeqCst),
                self.saves.load(Ordering::SeqCst),
                self.inits.load(Ordering::SeqCst),
            )
        }
    }

    impl GateIo for FakeGate {
        fn read_consent(&self) -> ConsentRead {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.consent.lock().unwrap().clone()
        }

        fn save_accept(&self) -> Result<(), String> {
            self.saves.fetch_add(1, Ordering::SeqCst);
            if !self.save_delay.is_zero() {
                std::thread::sleep(self.save_delay);
            }
            self.save.lock().unwrap().clone()
        }

        fn init_runtime(&self) -> Result<Vec<String>, String> {
            self.inits.fetch_add(1, Ordering::SeqCst);
            self.init.lock().unwrap().clone()
        }
    }

    /// 闸门拒绝时业务闭包必须零执行；就绪后放行。
    #[test]
    fn privacy_guard_rejects_without_calling_business_closure() {
        let state = PrivacyState::new();
        let ran = AtomicBool::new(false);
        let denied = guarded(&state, || {
            ran.store(true, Ordering::SeqCst);
            Ok::<(), String>(())
        });
        let err = denied.expect_err("Checking 阶段必须拒绝");
        assert!(err.starts_with(ERR_CONSENT_REQUIRED), "{err}");
        assert!(!ran.load(Ordering::SeqCst), "被拒绝的命令不得进入业务闭包");

        // SavingConsent / Starting / BlockedError / Exiting 都不是 Ready
        for phase in [
            Phase::SavingConsent,
            Phase::Starting,
            Phase::BlockedError,
            Phase::Exiting,
            Phase::NeedsConsent,
        ] {
            state.publish(phase, None);
            assert!(
                guarded(&state, || {
                    ran.store(true, Ordering::SeqCst);
                    Ok::<(), String>(())
                })
                .is_err(),
                "{phase:?} 不得当作 Ready"
            );
        }
        assert!(!ran.load(Ordering::SeqCst));

        state.publish_ready(Vec::new());
        guarded(&state, || {
            ran.store(true, Ordering::SeqCst);
            Ok::<(), String>(())
        })
        .expect("Ready 时必须放行");
        assert!(ran.load(Ordering::SeqCst));
    }

    /// 未同意时 bootstrap 只做「必要设置检查」：不保存、不初始化、不碰业务。
    #[test]
    fn privacy_bootstrap_has_no_business_io() {
        for consent in [
            ConsentRead::Missing,
            ConsentRead::NotGranted,
            ConsentRead::Unreadable("设置解析失败（合成）".into()),
        ] {
            let state = PrivacyState::new();
            let io = FakeGate::new(consent.clone());
            let snapshot = run_bootstrap(&state, &io);
            let (reads, saves, inits) = io.calls();
            assert_eq!(reads, 1, "只允许一次必要设置读取");
            assert_eq!(saves, 0, "未点击同意不得保存");
            assert_eq!(inits, 0, "未同意不得初始化业务（日志/窗口/托盘/价格）");
            let expected = match consent {
                ConsentRead::Missing | ConsentRead::NotGranted => "needs_consent",
                _ => "blocked_error",
            };
            assert_eq!(snapshot.phase, expected);
            assert!(!snapshot.policy.markdown.is_empty(), "政策必须随响应下发");
        }
    }

    /// 已有有效同意 → 单次初始化进入 Ready；重复 bootstrap 幂等。
    #[test]
    fn privacy_reload_bootstrap_is_idempotent() {
        let state = PrivacyState::new();
        let io = FakeGate::new(ConsentRead::Granted);
        for _ in 0..3 {
            let snapshot = run_bootstrap(&state, &io);
            assert_eq!(snapshot.phase, "ready");
        }
        let (reads, saves, inits) = io.calls();
        assert_eq!(reads, 1, "就绪后不再重复检查磁盘");
        assert_eq!(saves, 0);
        assert_eq!(inits, 1, "初始化只能执行一次");
    }

    /// 保存未成功时闸门保持关闭；保存成功后才 Ready。
    #[test]
    fn privacy_accept_publishes_ready_only_after_atomic_save() {
        let state = PrivacyState::new();
        let io = FakeGate::new(ConsentRead::Missing);
        *io.save.lock().unwrap() = Err("磁盘满（合成）".to_string());
        let err = run_accept(&state, &io).expect_err("保存失败必须报错");
        assert!(err.contains("磁盘满"), "{err}");
        assert_eq!(state.phase(), Phase::BlockedError, "保存失败不得 Ready");
        assert_eq!(io.calls().2, 0, "保存失败不得启动业务初始化");

        *io.save.lock().unwrap() = Ok(());
        let snapshot = run_accept(&state, &io).expect("恢复后重试成功");
        assert_eq!(snapshot.phase, "ready");
        assert_eq!(io.calls().1, 2, "重试只重复保存，不重复读取");
        assert_eq!(io.calls().2, 1, "初始化只执行一次");
    }

    /// 重复/并发同意只产生一次写事务与一套运行时。
    #[test]
    fn privacy_double_accept_starts_runtime_once() {
        let state = PrivacyState::new();
        // 让保存稍慢：第二个 accept 必须**等待**而不是并行写入或重复初始化。
        let gate = Arc::new(FakeGate {
            save_delay: Duration::from_millis(150),
            ..FakeGate::new(ConsentRead::Missing)
        });

        let mut handles = Vec::new();
        for _ in 0..2 {
            let state = state.clone();
            let gate = gate.clone();
            handles.push(std::thread::spawn(move || {
                run_accept(&state, gate.as_ref()).map(|s| s.phase)
            }));
        }
        for h in handles {
            assert_eq!(h.join().unwrap().unwrap(), "ready");
        }
        let (reads, saves, inits) = gate.calls();
        assert_eq!(saves, 1, "并发同意只允许一次写事务");
        assert_eq!(inits, 1, "并发同意只允许一套运行时");
        assert!(reads <= 1, "重复同意不需要重新检查磁盘: {reads}");
    }

    /// 退出确认后：晚到的 bootstrap/accept 不得复活初始化。
    #[test]
    fn privacy_late_response_cannot_reopen_exiting_app() {
        let state = PrivacyState::new();
        let io = FakeGate::new(ConsentRead::Granted);
        assert!(state.begin_exit(), "首次退出请求取得执行权");
        assert!(!state.begin_exit(), "重复退出不重复结束进程");

        let snapshot = run_bootstrap(&state, &io);
        assert_eq!(snapshot.phase, "exiting");
        assert!(run_accept(&state, &io).is_err());
        let (reads, saves, inits) = io.calls();
        assert_eq!(
            (reads, saves, inits),
            (0, 0, 0),
            "退出中不得再做任何引导副作用"
        );
    }

    /// 初始化只在未完成步骤上执行：重试只补缺失部分。
    #[test]
    fn privacy_runtime_initialization_runs_once() {
        let mut flags = RuntimeFlags::default();
        assert_eq!(flags.pending(), RuntimeStep::ALL.to_vec());
        flags.mark(RuntimeStep::Logging);
        flags.mark(RuntimeStep::WindowState);
        assert_eq!(
            flags.pending(),
            vec![
                RuntimeStep::Saver,
                RuntimeStep::Tray,
                RuntimeStep::PriceSync
            ],
            "已完成的步骤不得重复执行"
        );
        for step in RuntimeStep::ALL {
            flags.mark(step);
        }
        assert!(flags.pending().is_empty());
    }

    /// 引导期窗口事件：关闭 → 提示退出；尺寸/位置 → 忽略（不落盘）。
    #[test]
    fn privacy_window_events_before_ready_do_not_persist() {
        for phase in [
            Phase::Checking,
            Phase::NeedsConsent,
            Phase::SavingConsent,
            Phase::BlockedError,
        ] {
            assert_eq!(close_event_action(phase), WindowEventAction::PromptExit);
            assert_eq!(geometry_event_action(phase), WindowEventAction::Ignore);
        }
        assert_eq!(
            close_event_action(Phase::Ready),
            WindowEventAction::Business
        );
        assert_eq!(
            geometry_event_action(Phase::Ready),
            WindowEventAction::Business
        );
    }

    /// 价格同步：未 Ready 永不联网；Ready 后按开关与快照年龄判断。
    #[test]
    fn privacy_timer_cannot_sync_while_waiting_for_consent() {
        assert!(!price_sync_due(Phase::Checking, true, None, 24));
        assert!(!price_sync_due(Phase::NeedsConsent, true, Some(999), 24));
        assert!(!price_sync_due(Phase::SavingConsent, true, Some(999), 24));
        assert!(
            !price_sync_due(Phase::Ready, false, Some(999), 24),
            "关闭开关后不联网"
        );
        assert!(!price_sync_due(Phase::Ready, true, Some(23), 24));
        assert!(price_sync_due(Phase::Ready, true, Some(24), 24));
        assert!(
            price_sync_due(Phase::Ready, true, None, 24),
            "无快照视为到期"
        );
    }

    /// 关闭请求标记：重复 X 只出现一次提示；取消后清除。
    #[test]
    fn privacy_exit_prompt_is_single_and_cancellable() {
        let state = PrivacyState::new();
        assert!(state.mark_exit_prompt_pending(), "首次置位");
        assert!(!state.mark_exit_prompt_pending(), "重复关闭不重复提示");
        assert!(state.snapshot().exit_prompt_pending);
        state.clear_exit_prompt();
        assert!(!state.snapshot().exit_prompt_pending);
    }

    /// 真实设置文件往返：同意 → 重启后不再询问；删除/改 false → 重新询问。
    #[test]
    fn privacy_saved_consent_then_restart_recovers() {
        let dir = std::env::temp_dir().join(format!("tokenscope-privacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");

        // 首次启动：无设置 → 等待同意
        let state = PrivacyState::new();
        let io = FileGate { path: path.clone() };
        assert_eq!(run_bootstrap(&state, &io).phase, "needs_consent");
        // 用户同意 → 落盘 → Ready
        run_accept(&state, &io).unwrap();
        assert_eq!(state.phase(), Phase::Ready);

        // 重启：新进程实例读同一文件 → 直接 Ready，不再询问
        let restarted = PrivacyState::new();
        assert_eq!(run_bootstrap(&restarted, &io).phase, "ready");

        // 用户删除同意位（或删除文件）→ 重新询问
        std::fs::write(&path, "privacy_policy_accepted = false\n").unwrap();
        let again = PrivacyState::new();
        assert_eq!(run_bootstrap(&again, &io).phase, "needs_consent");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 使用真实设置文件的 GateIo（不初始化真实运行时）。
    struct FileGate {
        path: std::path::PathBuf,
    }

    impl GateIo for FileGate {
        fn read_consent(&self) -> ConsentRead {
            tokenscope::settings::read_consent(&self.path)
        }
        fn save_accept(&self) -> Result<(), String> {
            tokenscope::settings::accept_privacy_policy(&self.path).map_err(|e| format!("{e:#}"))
        }
        fn init_runtime(&self) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }
    }

    // ── 命令覆盖（新增命令漏门的防线）────────────────────────

    /// 打包政策与源文件一致：`include_str!` 直接指向 `docs/privacy.md`，
    /// 因此"打包进程序的那一份"就是源文件；这里守住逐字一致、关键章节
    /// 与日期解析，防止有人另写一份会漂移的政策副本。
    #[test]
    fn privacy_packaged_policy_matches_source() {
        let source =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/privacy.md"))
                .expect("docs/privacy.md 必须存在");
        assert_eq!(
            POLICY_MARKDOWN.replace("\r\n", "\n"),
            source.replace("\r\n", "\n"),
            "打包政策必须与 docs/privacy.md 逐字一致"
        );
        let policy = policy_info();
        assert_eq!(policy.title, "TokenScope 隐私政策");
        assert!(!policy.date.is_empty(), "政策日期必须可解析");
        for section in [
            "## 一、适用范围",
            "## 二、读取的数据及用途",
            "## 三、本地保存位置",
            "## 四、网络请求与数据传输",
            "## 五、第三方服务与运行时",
            "## 六、用户可以如何控制数据",
            "## 七、保留、删除与卸载",
            "## 八、反馈与联系方式",
            "## 九、政策更新",
        ] {
            assert!(policy.markdown.contains(section), "政策缺少章节: {section}");
        }
        // 政策正文必须说明"先同意后读取"的实际行为（不是早期版本的说法）
        assert!(
            policy.markdown.contains("同意"),
            "政策必须说明程序内的同意机制"
        );
    }

    const LIB_RS: &str = include_str!("lib.rs");
    const COMMANDS_RS: &str = include_str!("commands.rs");

    /// 从 `generate_handler![...]` 提取注册的命令名（`模块::名字` 取末段）。
    fn registered_commands() -> Vec<String> {
        let start = LIB_RS
            .find("tauri::generate_handler![")
            .expect("lib.rs 必须注册命令");
        let rest = &LIB_RS[start..];
        let end = rest.find("])").expect("generate_handler 必须以 ]) 结束");
        rest[..end]
            .lines()
            .filter_map(|line| line.trim().strip_suffix(','))
            .filter_map(|path| path.rsplit("::").next())
            .map(|name| name.trim().to_string())
            .filter(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .collect()
    }

    /// 命令 `name` 的实现体是否调用统一闸门。
    fn command_has_guard(name: &str) -> bool {
        let needle = format!("fn {name}(");
        let Some(pos) = COMMANDS_RS.find(&needle) else {
            return false;
        };
        let body = &COMMANDS_RS[pos..];
        let body_end = body.find("#[tauri::command]").unwrap_or(body.len());
        body[..body_end].contains("privacy::require_ready")
    }

    /// 审计 `generate_handler!` 全表：每个命令要么显式受保护（且首行调用
    /// 统一 guard），要么是引导命令；两边都不出现的命令即漏门。
    #[test]
    fn privacy_all_business_commands_reject_before_ready() {
        let registered = registered_commands();
        assert!(
            registered.len() >= 20,
            "命令注册表解析异常（{} 条）",
            registered.len()
        );
        for name in &registered {
            let protected = PROTECTED_COMMANDS.contains(&name.as_str());
            let bootstrap = BOOTSTRAP_COMMANDS.contains(&name.as_str());
            assert!(
                protected || bootstrap,
                "命令 {name} 未分类：新增命令必须显式声明为受保护命令或引导命令"
            );
            assert!(!(protected && bootstrap), "命令 {name} 分类冲突");
            if protected {
                assert!(
                    command_has_guard(name),
                    "受保护命令 {name} 缺少 privacy::require_ready 闸门"
                );
            }
        }
        // 分类表不得虚报未注册的命令
        for name in PROTECTED_COMMANDS.iter().chain(BOOTSTRAP_COMMANDS) {
            assert!(
                registered.contains(&name.to_string()),
                "{name} 在分类表中但未注册到 generate_handler!"
            );
        }
    }
}
