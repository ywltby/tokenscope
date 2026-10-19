//! 并发首次初始化的统一重试预算、时钟与判定。
//!
//! `HistoryDb::open` 与 `Cache::open` 的打开序列都会在「两个连接同时对
//! 新建/旧版库做 WAL 切换与迁移」时拿到 SQLITE_READONLY 或 SQLITE_BUSY：
//! 2026-10-19 探针实测 HistoryDb 并发新建 30 轮 11 轮失败（extended=8，
//! 立即返回而非锁等待），Cache v3 并发打开 100 次 2 次失败（同一错误
//! 形态）。
//!
//! 预算语义（与旧版的关键区别）：
//! - **绝对截止时间只在外层创建一次**（[`Deadline::new`]），随后传入
//!   `open_once`、WAL 重试与迁移等待路径；内层一律用 [`Deadline::remaining`]，
//!   **不得重新 `now + budget`**——否则嵌套重试各自获得完整预算，
//!   实际总时长不受控。
//! - 每个可能锁等待的 SQLite 阶段按**剩余预算**设置 `busy_timeout`
//!   （[`Deadline::busy_budget`]），不是只在连接建立时设一次完整预算。
//! - 每次睡眠结束后重新检查截止时间；预算耗尽不再启动下一次重试，
//!   返回最后一个类型化错误。
//!
//! 类型化前提：错误链必须保留 `rusqlite::Error`——调用方不得在重试判定
//! 之前用 `anyhow!("{:#}")` 之类把错误字符串化；`is_concurrent_init_transient`
//! 只做类型判定，不匹配消息文本（单测固化这一约束）。
//!
//! 时钟可注入（[`Clock`]）：生产用 [`RealClock`]；单测用假时间精确验证
//! 预算边界、嵌套调用共享截止时间、睡眠后不再尝试与最后错误返回。

use std::time::{Duration, Instant};

/// 并发首次初始化的重试总预算（外层一次创建绝对 deadline）。
pub(crate) const CONCURRENT_INIT_RETRY_BUDGET: Duration = Duration::from_millis(2000);

/// 重试间隔（与 busy_timeout 的小步等待语义一致）。
pub(crate) const CONCURRENT_INIT_RETRY_BACKOFF: Duration = Duration::from_millis(20);

/// 可注入的时钟与睡眠。
pub(crate) trait Clock {
    fn now(&self) -> Instant;
    fn sleep(&self, duration: Duration);
}

/// 生产时钟：真实时间与线程睡眠。
pub(crate) struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// 一次打开流程的绝对截止时间。
///
/// **只在最外层创建一次**；`open_once`、WAL 重试、迁移等待都接收同一个
/// `Deadline`，内层只能用 [`Deadline::remaining`] / [`Deadline::busy_budget`]，
/// 不得重新计算 `now + budget`。
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline {
    at: Instant,
}

impl Deadline {
    pub(crate) fn new(clock: &dyn Clock, budget: Duration) -> Self {
        Self {
            at: clock.now() + budget,
        }
    }

    /// 剩余预算（相对同一时钟）。
    pub(crate) fn remaining(&self, clock: &dyn Clock) -> Duration {
        self.at.saturating_duration_since(clock.now())
    }

    /// 预算是否已耗尽。
    pub(crate) fn expired(&self, clock: &dyn Clock) -> bool {
        self.remaining(clock).is_zero()
    }

    /// 某个可能锁等待阶段可用的 `busy_timeout`：剩余预算；`None` 表示
    /// 预算已耗尽，调用方不得再启动该阶段的等待。
    pub(crate) fn busy_budget(&self, clock: &dyn Clock) -> Option<Duration> {
        let remaining = self.remaining(clock);
        (!remaining.is_zero()).then_some(remaining)
    }
}

/// 瞬态错误：SQLITE_READONLY（extended=8，探针实测形态）或 SQLITE_BUSY。
pub(crate) fn is_concurrent_init_transient(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        cause.downcast_ref::<rusqlite::Error>().is_some_and(|err| {
            matches!(
                err,
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ReadOnly
                        || f.code == rusqlite::ErrorCode::DatabaseBusy
            )
        })
    })
}

/// 按剩余预算为某个可能锁等待的 SQLite 阶段设置 `busy_timeout`，返回实际
/// 设置的时长（供诊断与测试断言）。
///
/// 每个阶段（WAL 切换的每次尝试、读版本、升级前备份前后、迁移事务前）都
/// 必须重新调用一次——不能只在连接建立时设置一次完整预算，否则前面的
/// 阶段（尤其是升级前备份）消耗的时间不会反映到后续等待里。
/// 预算耗尽时返回错误：该阶段不得再启动等待。
pub(crate) fn apply_busy_budget(
    conn: &rusqlite::Connection,
    deadline: &Deadline,
    clock: &dyn Clock,
) -> anyhow::Result<Duration> {
    let Some(budget) = deadline.busy_budget(clock) else {
        anyhow::bail!("并发初始化重试预算已耗尽，不再等待数据库锁");
    };
    conn.busy_timeout(budget)?;
    Ok(budget)
}

/// 在**给定**绝对 deadline 内重试瞬态失败（内层不得新建 deadline）。
///
/// - 非瞬态错误立即返回；
/// - 剩余预算不足一个 backoff 时不再重试；
/// - 睡眠结束后重新检查截止时间，耗尽即返回最后一个错误。
pub(crate) fn retry_until<T>(
    deadline: &Deadline,
    clock: &dyn Clock,
    backoff: Duration,
    mut attempt: impl FnMut() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    loop {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(e) => {
                if !is_concurrent_init_transient(&e) {
                    return Err(e);
                }
                // 剩余预算不足一个 backoff：不再重试，当前错误就是最后一个。
                if deadline.remaining(clock) < backoff {
                    return Err(e);
                }
                clock.sleep(backoff);
                // 睡眠结束后重新检查：预算已耗尽不得再启动下一次重试
                //（返回最后一个类型化错误）。
                if deadline.expired(clock) {
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// 假时钟：`now` 由 `advance`/`sleep` 推进；记录全部睡眠时长。
    struct FakeClock {
        now: Cell<Instant>,
        sleeps: RefCell<Vec<Duration>>,
    }

    impl FakeClock {
        fn new() -> Self {
            Self {
                now: Cell::new(Instant::now()),
                sleeps: RefCell::new(Vec::new()),
            }
        }

        fn advance(&self, d: Duration) {
            self.now.set(self.now.get() + d);
        }

        fn total_sleep(&self) -> Duration {
            self.sleeps.borrow().iter().sum()
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Instant {
            self.now.get()
        }

        fn sleep(&self, d: Duration) {
            self.sleeps.borrow_mut().push(d);
            self.advance(d);
        }
    }

    fn readonly() -> anyhow::Error {
        anyhow::Error::new(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ReadOnly,
                extended_code: 8,
            },
            Some("attempt to write a readonly database".into()),
        ))
    }

    fn busy() -> anyhow::Error {
        anyhow::Error::new(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".into()),
        ))
    }

    #[test]
    fn transient_error_is_matched_by_type_only() {
        assert!(is_concurrent_init_transient(&readonly()));
        assert!(is_concurrent_init_transient(&busy()));
        // 非瞬态（即便是只读文案）不匹配：判定只看错误类型。
        assert!(!is_concurrent_init_transient(&anyhow::anyhow!(
            "disk read only"
        )));
        // 字符串化会丢失类型 → 不再算瞬态：约束调用方保留类型化错误链。
        let flattened = anyhow::anyhow!("{:#}", readonly());
        assert!(!is_concurrent_init_transient(&flattened));
        // 包装后的类型化错误仍然匹配（context 不丢 source）。
        let wrapped = readonly().context("执行 v1 迁移失败");
        assert!(is_concurrent_init_transient(&wrapped));
    }

    #[test]
    fn budget_bounds_attempts_and_stops_after_sleep() {
        let clock = FakeClock::new();
        let deadline = Deadline::new(&clock, Duration::from_millis(60));
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_until(&deadline, &clock, Duration::from_millis(20), || {
                calls.set(calls.get() + 1);
                Err(readonly())
            });
        assert!(out.is_err());
        // t=0 尝试 → sleep 到 20 → 尝试 → sleep 到 40 → 尝试 → sleep 到 60
        // → 睡眠后检查发现预算耗尽，不再尝试：共 3 次。
        assert_eq!(calls.get(), 3, "睡眠后预算耗尽不得再启动重试");
        assert_eq!(clock.total_sleep(), Duration::from_millis(60));
        // 最后错误保留且仍是类型化的瞬态错误。
        let last = out.unwrap_err();
        assert!(is_concurrent_init_transient(&last));
        assert!(last.to_string().contains("readonly"));
    }

    #[test]
    fn nested_retries_share_one_absolute_deadline() {
        let clock = FakeClock::new();
        let deadline = Deadline::new(&clock, Duration::from_millis(100));
        let inner_calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_until(&deadline, &clock, Duration::from_millis(20), || {
                // 本次尝试内的锁等待/I/O 消耗掉 90ms：外层只剩 10ms。
                clock.advance(Duration::from_millis(90));
                // 内层必须用**同一个** deadline：剩余 10ms < backoff，
                // 一次尝试后即返回，不得睡眠。
                let _inner: anyhow::Result<u32> =
                    retry_until(&deadline, &clock, Duration::from_millis(20), || {
                        inner_calls.set(inner_calls.get() + 1);
                        Err(readonly())
                    });
                Err(readonly())
            });
        assert!(out.is_err());
        // 共享 deadline：内层与外层都不再睡眠（若内层重新 now + 100ms，
        // 它会睡眠 100ms 并使本断言失败——变异验证过）。
        assert_eq!(
            clock.total_sleep(),
            Duration::ZERO,
            "嵌套重试不得各自重算预算：总睡眠 {:?}",
            clock.total_sleep()
        );
        assert_eq!(inner_calls.get(), 1);
    }

    #[test]
    fn retries_transient_until_success() {
        let clock = FakeClock::new();
        let deadline = Deadline::new(&clock, Duration::from_millis(200));
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_until(&deadline, &clock, Duration::from_millis(10), || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err(readonly())
                } else {
                    Ok(42)
                }
            });
        assert_eq!(out.unwrap(), 42);
        assert_eq!(calls.get(), 3);
        assert_eq!(clock.total_sleep(), Duration::from_millis(20));
    }

    #[test]
    fn non_transient_fails_immediately() {
        let clock = FakeClock::new();
        let deadline = Deadline::new(&clock, Duration::from_millis(200));
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_until(&deadline, &clock, Duration::from_millis(10), || {
                calls.set(calls.get() + 1);
                Err(anyhow::anyhow!("历史库结构版本过高"))
            });
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
        assert_eq!(clock.total_sleep(), Duration::ZERO);
    }

    #[test]
    fn zero_budget_runs_one_attempt_then_stops() {
        let clock = FakeClock::new();
        let deadline = Deadline::new(&clock, Duration::ZERO);
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_until(&deadline, &clock, Duration::from_millis(20), || {
                calls.set(calls.get() + 1);
                Err(readonly())
            });
        assert!(out.is_err());
        // 首次尝试允许（“不得再启动**下一次**”）；之后剩余 0 < backoff，
        // 不再重试也不再睡眠。
        assert_eq!(calls.get(), 1);
        assert_eq!(clock.total_sleep(), Duration::ZERO);
    }

    #[test]
    fn busy_budget_tracks_remaining_time() {
        let clock = FakeClock::new();
        let deadline = Deadline::new(&clock, Duration::from_millis(100));
        assert_eq!(
            deadline.busy_budget(&clock),
            Some(Duration::from_millis(100))
        );
        clock.advance(Duration::from_millis(60));
        assert_eq!(
            deadline.busy_budget(&clock),
            Some(Duration::from_millis(40))
        );
        clock.advance(Duration::from_millis(40));
        assert_eq!(deadline.busy_budget(&clock), None);
        assert!(deadline.expired(&clock));
    }
}
