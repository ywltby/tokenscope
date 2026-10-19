//! 并发首次初始化的统一重试预算与判定。
//!
//! `HistoryDb::open` 与 `Cache::open` 的打开序列都会在「两个连接同时对
//! 新建/旧版库做 WAL 切换与迁移」时拿到 SQLITE_READONLY 或 SQLITE_BUSY：
//! 2026-10-19 探针实测 HistoryDb 并发新建 30 轮 11 轮失败（extended=8，
//! 立即返回而非锁等待），Cache v3 并发打开 100 次 2 次失败（同一错误
//! 形态）。两个打开序列共享本模块的预算与判定，避免各自独立叠加重试；
//! 预算耗尽时返回最后一个错误。
//!
//! 类型化前提：错误链必须保留 `rusqlite::Error`——调用方不得在重试判定
//! 之前用 `anyhow!("{:#}")` 之类把错误字符串化；`is_concurrent_init_transient`
//! 只做类型判定，不匹配消息文本（单测固化这一约束）。

use std::time::{Duration, Instant};

/// 并发首次初始化的重试总预算（WAL 切换与整个打开序列共享同一个 deadline）。
pub(crate) const CONCURRENT_INIT_RETRY_BUDGET: Duration = Duration::from_millis(2000);

/// 重试间隔（与 busy_timeout 的小步等待语义一致）。
pub(crate) const CONCURRENT_INIT_RETRY_BACKOFF: Duration = Duration::from_millis(20);

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

/// 在预算内重试瞬态失败：`attempt` 每次调用都会重建连接（连接级 journal
/// 状态已被破坏，同一连接重试无效——探针实测 20 次仍失败）。
/// 非瞬态错误立即返回；剩余预算不足以再等一轮时返回最后一个错误。
pub(crate) fn retry_within_budget<T>(
    budget: Duration,
    backoff: Duration,
    mut attempt: impl FnMut() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let deadline = Instant::now() + budget;
    loop {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(e) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if !is_concurrent_init_transient(&e) || remaining < backoff {
                    return Err(e);
                }
                std::thread::sleep(backoff);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

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
    fn retries_transient_until_success() {
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_within_budget(Duration::from_millis(200), Duration::from_millis(1), || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err(readonly())
                } else {
                    Ok(42)
                }
            });
        assert_eq!(out.unwrap(), 42);
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn non_transient_fails_immediately() {
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_within_budget(Duration::from_millis(200), Duration::from_millis(1), || {
                calls.set(calls.get() + 1);
                Err(anyhow::anyhow!("历史库结构版本过高"))
            });
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn zero_budget_does_not_retry() {
        let calls = Cell::new(0);
        let out: anyhow::Result<u32> =
            retry_within_budget(Duration::ZERO, Duration::from_millis(20), || {
                calls.set(calls.get() + 1);
                Err(readonly())
            });
        assert!(out.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn budget_bounds_total_attempts_and_returns_last_error() {
        let calls = Cell::new(0);
        let started = Instant::now();
        let out: anyhow::Result<u32> =
            retry_within_budget(Duration::from_millis(60), Duration::from_millis(20), || {
                calls.set(calls.get() + 1);
                Err(readonly())
            });
        let elapsed = started.elapsed();
        assert!(out.is_err());
        assert!(
            calls.get() >= 3 && calls.get() <= 5,
            "60ms/20ms 预算的尝试次数应有界：{}",
            calls.get()
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "总耗时应受预算约束：{elapsed:?}"
        );
        assert!(out.unwrap_err().to_string().contains("readonly"));
    }
}
