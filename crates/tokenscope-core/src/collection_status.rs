//! PR2（2026-10-15 计划 F01）：真实采集生命周期的内核状态与观察者接口。
//!
//! 职责边界（计划 §3.1 / §6.1）：
//! - 状态只由**真实采集边界**推进：领队取得采集串行锁后才进入 `running`；
//!   采集闭包与历史提交结果确定后进入终态。单飞跟随者不创建轮次；等待
//!   串行锁（排队）不算正在读取。
//! - `revision` 进程内每次状态转换单调递增；`run_id` 仅真正执行的领队递增。
//! - 观察者通知必须在**状态锁外**进行；单个观察者 panic 不影响采集结果，
//!   也不影响其他观察者。观察者不得同步重入采集（会与采集串行锁互等）。
//! - 不依赖 Tauri：宿主（GUI 壳）注册观察者做轻量事件投递；本模块的快照
//!   读取（[`CollectionStatusTracker::snapshot`]）是纯内存读取，不触发采集。
//!
//! 初始 `idle` 不意味着统计已经更新；`latest_generation` 保存最近**成功**
//! 采集的历史代次（失败不清空），`last_completed_at` 记录最近一次成功或
//! 失败的完成时刻（不是查询 as_of）。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

/// 采集触发来源（与 `report::Trigger` 对应；独立定义避免模块间反向依赖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionTrigger {
    Startup,
    Manual,
    Auto,
}

impl CollectionTrigger {
    /// IPC / 事件载荷里的稳定字符串（与前端约定一致）。
    pub fn as_str(self) -> &'static str {
        match self {
            CollectionTrigger::Startup => "startup",
            CollectionTrigger::Manual => "manual",
            CollectionTrigger::Auto => "auto",
        }
    }
}

/// 采集阶段。`idle` = 本进程尚未开始任何采集轮次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionPhase {
    Idle,
    Running,
    Succeeded,
    Failed,
}

impl CollectionPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            CollectionPhase::Idle => "idle",
            CollectionPhase::Running => "running",
            CollectionPhase::Succeeded => "succeeded",
            CollectionPhase::Failed => "failed",
        }
    }
}

/// 统一状态快照（IPC DTO 与观察者载荷同形）。字段语义见计划 §3.1：
/// 普通状态不带文件路径、模型列表或用量事实；诊断内容沿用既有查询渠道。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionStatus {
    /// 进程内每次状态转换单调递增（IPC 限安全整数）。
    pub revision: u64,
    /// 仅真正执行的领队递增；终态保留本轮 run_id。
    pub run_id: u64,
    /// idle | running | succeeded | failed
    pub phase: &'static str,
    /// 本轮（或最近一轮）采集的触发来源；从未采集时为 null。
    pub trigger: Option<&'static str>,
    /// 最近**成功**采集的历史代次；失败不清空。
    pub latest_generation: Option<u64>,
    /// 最近完成的这一轮是否有事实变动（不以文件 mtime 代替）。
    pub changed: bool,
    /// 最近完成的这一轮是否带采集警告。
    pub has_warnings: bool,
    /// 最近成功或失败的完成时刻（UTC RFC3339）；非查询 as_of。
    pub last_completed_at: Option<String>,
    /// 精简可展示错误（完整链只写现有日志）。
    pub error: Option<String>,
}

/// 状态观察者。回调在状态锁外、采集串行锁内被调用（start/终态都发生在
/// 串行锁持有期间）——**不得同步重入采集**；宿主桥只做轻量事件投递。
pub trait CollectionObserver: Send + Sync {
    fn on_collection_status(&self, status: &CollectionStatus);
}

struct StatusInner {
    revision: u64,
    run_id: u64,
    phase: CollectionPhase,
    trigger: Option<CollectionTrigger>,
    latest_generation: Option<u64>,
    changed: bool,
    has_warnings: bool,
    last_completed_at: Option<String>,
    error: Option<String>,
}

impl Default for StatusInner {
    fn default() -> Self {
        Self {
            revision: 0,
            run_id: 0,
            phase: CollectionPhase::Idle,
            trigger: None,
            latest_generation: None,
            changed: false,
            has_warnings: false,
            last_completed_at: None,
            error: None,
        }
    }
}

/// 采集状态存储。进程级单例见 [`global_tracker`]；状态机测试用独立实例，
/// 不共享全局状态。
pub struct CollectionStatusTracker {
    inner: Mutex<StatusInner>,
    observers: Mutex<Vec<(u64, Arc<dyn CollectionObserver>)>>,
    next_observer_id: AtomicU64,
}

impl Default for CollectionStatusTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl CollectionStatusTracker {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(StatusInner::default()),
            observers: Mutex::new(Vec::new()),
            next_observer_id: AtomicU64::new(1),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, StatusInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 只读内存快照。不触发采集、不打开历史库、不推进任何状态。
    pub fn snapshot(&self) -> CollectionStatus {
        let g = self.lock_inner();
        CollectionStatus {
            revision: g.revision,
            run_id: g.run_id,
            phase: g.phase.as_str(),
            trigger: g.trigger.map(CollectionTrigger::as_str),
            latest_generation: g.latest_generation,
            changed: g.changed,
            has_warnings: g.has_warnings,
            last_completed_at: g.last_completed_at.clone(),
            error: g.error.clone(),
        }
    }

    /// 注册观察者，返回 RAII 句柄：句柄丢弃时自动注销（测试与临时订阅用）。
    pub fn register_observer(&self, observer: Arc<dyn CollectionObserver>) -> ObserverHandle<'_> {
        let id = self.next_observer_id.fetch_add(1, Ordering::SeqCst);
        self.observers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, observer));
        ObserverHandle {
            tracker: self,
            id: Some(id),
        }
    }

    fn notify(&self, status: &CollectionStatus) {
        // 状态锁早已释放；观察者表锁也只在取快照时持有——回调期间不持任何锁，
        // 单个观察者 panic 不影响其他观察者（catch_unwind 隔离，记 warn）。
        let observers: Vec<Arc<dyn CollectionObserver>> = {
            let list = self.observers.lock().unwrap_or_else(|e| e.into_inner());
            list.iter().map(|(_, o)| o.clone()).collect()
        };
        for observer in observers {
            // 观察者来自任意宿主， unwind 不允许打断采集路径（结果已确定）。
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                observer.on_collection_status(status);
            }));
            if result.is_err() {
                log::warn!("采集状态观察者 panic（已隔离，不影响采集结果）");
            }
        }
    }

    /// 进入 running：run_id 递增，本轮 changed/has_warnings/error 清零，
    /// 最近成功代次与上次完成时刻保留。调用方必须已持有采集串行锁。
    pub(crate) fn begin_run(&self, trigger: CollectionTrigger) -> RunGuard<'_> {
        let snapshot = {
            let mut g = self.lock_inner();
            g.revision += 1;
            g.run_id += 1;
            g.phase = CollectionPhase::Running;
            g.trigger = Some(trigger);
            g.changed = false;
            g.has_warnings = false;
            g.error = None;
            self.snapshot_from(&g)
        };
        self.notify(&snapshot);
        RunGuard {
            tracker: self,
            run_id: snapshot.run_id,
            finished: false,
        }
    }

    /// 从（已加锁的）内部状态构造快照——避免在持锁时重入自己的锁。
    fn snapshot_from(&self, g: &StatusInner) -> CollectionStatus {
        CollectionStatus {
            revision: g.revision,
            run_id: g.run_id,
            phase: g.phase.as_str(),
            trigger: g.trigger.map(CollectionTrigger::as_str),
            latest_generation: g.latest_generation,
            changed: g.changed,
            has_warnings: g.has_warnings,
            last_completed_at: g.last_completed_at.clone(),
            error: g.error.clone(),
        }
    }

    /// 终态转换（成功或失败）。仅当 run_id 仍是守卫登记的那一轮时生效——
    /// 迟到的旧守卫不覆盖新一轮的状态。内存状态转换在锁内完成，通知在锁外。
    fn finish(
        &self,
        run_id: u64,
        success: bool,
        generation: Option<u64>,
        changed: bool,
        has_warnings: bool,
        error: Option<String>,
    ) {
        let snapshot = {
            let mut g = self.lock_inner();
            if g.run_id != run_id || g.phase != CollectionPhase::Running {
                return;
            }
            g.revision += 1;
            g.phase = if success {
                CollectionPhase::Succeeded
            } else {
                CollectionPhase::Failed
            };
            if success {
                // 成功才推进最近成功代次；失败保留旧值（计划 §6.1 第 2 条）。
                g.latest_generation = generation.or(g.latest_generation);
            }
            g.changed = changed;
            g.has_warnings = has_warnings;
            g.last_completed_at = Some(now_rfc3339());
            g.error = error;
            self.snapshot_from(&g)
        };
        self.notify(&snapshot);
    }
}

fn now_rfc3339() -> String {
    jiff::Timestamp::now().to_string()
}

/// 观察者句柄：丢弃时注销对应观察者（未注销的句柄不能跨 tracker 生命周期）。
pub struct ObserverHandle<'a> {
    tracker: &'a CollectionStatusTracker,
    id: Option<u64>,
}

impl Drop for ObserverHandle<'_> {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.tracker
                .observers
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(existing, _)| *existing != id);
        }
    }
}

/// 一轮采集的 RAII 守卫：显式提交成功/失败后 `finished = true`，Drop 不再
/// 改写状态；未提交就 Drop（含 panic unwind）按**失败**收尾——RAII 收尾
/// 不能在 panic 时伪报成功，也不能吞掉原始错误（原始错误由航班守卫与
/// 调用方现有路径记录）。
pub(crate) struct RunGuard<'a> {
    tracker: &'a CollectionStatusTracker,
    run_id: u64,
    finished: bool,
}

impl RunGuard<'_> {
    /// 采集成功并已取得实际结果后显式提交（generation = 本轮采集后的历史
    /// 代次；has_warnings = 本轮是否带采集警告）。
    pub(crate) fn finish_success(&mut self, generation: u64, changed: bool, has_warnings: bool) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.tracker.finish(
            self.run_id,
            true,
            Some(generation),
            changed,
            has_warnings,
            None,
        );
    }

    /// 采集返回错误后显式提交。error 只保留精简可展示消息。
    pub(crate) fn finish_error(&mut self, error: String) {
        if self.finished {
            return;
        }
        self.finished = true;
        self.tracker
            .finish(self.run_id, false, None, false, false, Some(error));
    }
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.tracker.finish(
                self.run_id,
                false,
                None,
                false,
                false,
                Some("采集线程异常退出".to_string()),
            );
        }
    }
}

/// 进程级单例。生产采集路径（启动/手动/定时/重扫）全部经它登记状态；
/// 状态机相关测试需要隔离实例时另行构造，不污染全局状态。
pub fn global_tracker() -> &'static CollectionStatusTracker {
    static GLOBAL: std::sync::LazyLock<CollectionStatusTracker> =
        std::sync::LazyLock::new(CollectionStatusTracker::new);
    &GLOBAL
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Recording {
        tx: mpsc::Sender<CollectionStatus>,
    }
    impl CollectionObserver for Recording {
        fn on_collection_status(&self, status: &CollectionStatus) {
            let _ = self.tx.send(status.clone());
        }
    }

    struct Panicking;
    impl CollectionObserver for Panicking {
        fn on_collection_status(&self, _status: &CollectionStatus) {
            panic!("观察者炸了");
        }
    }

    /// 观察者重入状态读取不得死锁（快照是独立锁，通知时不持任何锁）。
    struct ReentrantSnapshot {
        tracker: std::sync::Arc<CollectionStatusTracker>,
    }
    impl CollectionObserver for ReentrantSnapshot {
        fn on_collection_status(&self, _status: &CollectionStatus) {
            let _ = self.tracker.snapshot();
        }
    }

    fn drain(rx: &mpsc::Receiver<CollectionStatus>) -> Vec<CollectionStatus> {
        let mut out = Vec::new();
        while let Ok(s) = rx.try_recv() {
            out.push(s);
        }
        out
    }

    #[test]
    fn initial_snapshot_is_idle_and_read_only() {
        let tracker = CollectionStatusTracker::new();
        let s = tracker.snapshot();
        assert_eq!(s.phase, "idle");
        assert_eq!(s.revision, 0);
        assert_eq!(s.run_id, 0);
        assert_eq!(s.trigger, None);
        assert_eq!(s.latest_generation, None);
        assert!(!s.changed);
        // 快照是纯读取：重复读取不推进 revision/run_id。
        let again = tracker.snapshot();
        assert_eq!(s, again);
    }

    #[test]
    fn begin_and_success_transition_with_monotonic_revision() {
        let tracker = CollectionStatusTracker::new();
        let (tx, rx) = mpsc::channel();
        let _handle = tracker.register_observer(Arc::new(Recording { tx }));

        let mut guard = tracker.begin_run(CollectionTrigger::Auto);
        let running = tracker.snapshot();
        assert_eq!(running.phase, "running");
        assert_eq!(running.revision, 1);
        assert_eq!(running.run_id, 1);
        assert_eq!(running.trigger, Some("auto"));
        assert!(!running.changed);
        assert_eq!(running.latest_generation, None);
        guard.finish_success(42, true, true);
        drop(guard);

        let done = tracker.snapshot();
        assert_eq!(done.phase, "succeeded");
        assert_eq!(done.revision, 2);
        assert_eq!(done.run_id, 1, "终态保留本轮 run_id");
        assert_eq!(done.latest_generation, Some(42));
        assert!(done.changed);
        assert!(done.has_warnings);
        assert!(done.last_completed_at.is_some());
        assert_eq!(done.error, None);

        let events = drain(&rx);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].phase, "running");
        assert_eq!(events[1].phase, "succeeded");

        // 第二轮：changed/error 清零，但最近成功代次保留。
        let mut second = tracker.begin_run(CollectionTrigger::Manual);
        let running2 = tracker.snapshot();
        assert_eq!(running2.run_id, 2);
        assert_eq!(running2.revision, 3);
        assert!(!running2.changed);
        assert_eq!(running2.latest_generation, Some(42), "start 不清空成功代次");
        second.finish_success(42, false, false);
        let done2 = tracker.snapshot();
        assert_eq!(done2.latest_generation, Some(42));
        assert!(!done2.changed);
    }

    #[test]
    fn failure_keeps_latest_generation_and_reports_error() {
        let tracker = CollectionStatusTracker::new();
        let (tx, rx) = mpsc::channel();
        let _handle = tracker.register_observer(Arc::new(Recording { tx }));

        let mut ok = tracker.begin_run(CollectionTrigger::Startup);
        ok.finish_success(7, false, false);
        let failed_at = tracker.snapshot().last_completed_at;

        let mut bad = tracker.begin_run(CollectionTrigger::Manual);
        bad.finish_error("来源目录冲突（合成）".to_string());
        let failed = tracker.snapshot();
        assert_eq!(failed.phase, "failed");
        assert_eq!(failed.latest_generation, Some(7), "失败不推进成功代次");
        assert_eq!(failed.error.as_deref(), Some("来源目录冲突（合成）"));
        assert!(!failed.changed);
        assert!(failed.last_completed_at.is_some());

        // 成功后错误清除。
        let mut again = tracker.begin_run(CollectionTrigger::Auto);
        again.finish_success(9, true, false);
        let recovered = tracker.snapshot();
        assert_eq!(recovered.phase, "succeeded");
        assert_eq!(recovered.error, None);
        assert_eq!(recovered.latest_generation, Some(9));

        let events = drain(&rx);
        assert_eq!(events.len(), 6);
        assert_eq!(events[3].phase, "failed");
        // 终态时刻单调不回退（同一时刻可能相等，绝不倒退）。
        assert!(failed.last_completed_at.unwrap() >= failed_at.unwrap());
    }

    #[test]
    fn dropped_guard_marks_run_failed() {
        let tracker = CollectionStatusTracker::new();
        let (tx, rx) = mpsc::channel();
        let _handle = tracker.register_observer(Arc::new(Recording { tx }));
        {
            let _guard = tracker.begin_run(CollectionTrigger::Manual);
            // 未显式提交即丢弃（等价 panic unwind 路径）。
        }
        let s = tracker.snapshot();
        assert_eq!(s.phase, "failed", "RAII 收尾必须按失败处理，不伪报成功");
        assert_eq!(
            s.error.as_deref(),
            Some("采集线程异常退出"),
            "Drop 不能吞掉异常退出"
        );
        assert_eq!(s.latest_generation, None);
        assert!(s.last_completed_at.is_some());
        assert_eq!(drain(&rx).len(), 2);
    }

    #[test]
    fn stale_guard_cannot_override_newer_run() {
        let tracker = CollectionStatusTracker::new();
        let mut old = tracker.begin_run(CollectionTrigger::Manual);
        let old_run_id = old.run_id;
        let mut next = tracker.begin_run(CollectionTrigger::Auto);
        // 旧守卫迟到提交：run_id 已不是当前轮，不得覆盖新一轮状态。
        old.finish_success(99, true, false);
        assert_eq!(tracker.snapshot().run_id, old_run_id + 1);
        assert_eq!(tracker.snapshot().phase, "running");
        next.finish_success(100, false, false);
        let s = tracker.snapshot();
        assert_eq!(s.phase, "succeeded");
        assert_eq!(
            s.latest_generation,
            Some(100),
            "旧守卫的 99 不得覆盖新轮结果"
        );
    }

    #[test]
    fn observer_panic_is_isolated_and_handle_unregisters() {
        let tracker = std::sync::Arc::new(CollectionStatusTracker::new());
        let (tx, rx) = mpsc::channel();
        let _panicker = tracker.register_observer(Arc::new(Panicking));
        let _recorder = tracker.register_observer(Arc::new(Recording { tx }));
        let _reentrant = tracker.register_observer(Arc::new(ReentrantSnapshot {
            tracker: tracker.clone(),
        }));

        let mut guard = tracker.begin_run(CollectionTrigger::Manual);
        guard.finish_success(1, false, false);
        // panic 的观察者不影响采集结果语义（本测试能走到这里即为通过），
        // 其余观察者照常收到全部事件。
        let events = drain(&rx);
        assert_eq!(events.len(), 2);

        // 句柄注销：drop 后不再收到通知。
        {
            let handle = tracker.register_observer(Arc::new(Recording {
                tx: mpsc::channel().0,
            }));
            assert!(!handle.id.is_none());
        }
        let mut guard2 = tracker.begin_run(CollectionTrigger::Manual);
        guard2.finish_success(2, false, false);
        let events = drain(&rx);
        assert_eq!(events.len(), 2, "已注销的观察者不得再收到事件");
    }
}
