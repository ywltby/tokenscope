//! 统一日志（M8/M9 完善）：CLI 与 GUI 共用。
//!
//! - 落盘 `~/.tokenscope/logs/tokenscope.log`（每日滚动，非阻塞写入）；
//! - 每行自带 时间戳（UTC）/ 等级 / 目标模块:行号 / 消息；
//! - `log_error` 附加 anyhow 错误链与捕获点调用堆栈，便于排障；
//! - 级别可用 `RUST_LOG` 覆盖（默认 info）。

use std::path::PathBuf;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

/// 初始化全局日志，返回的 WorkerGuard 必须与进程同生命周期（调用方持有）。
/// `component` 用于排障时区分来源（"cli" / "gui"）。
pub fn init(component: &str) -> Option<WorkerGuard> {
    // data_dir() 即用户数据根 ~/.tokenscope；日志在其下 logs/。
    let dir = base_data_dir()?.join("logs");
    let appender = tracing_appender::rolling::daily(&dir, "tokenscope.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_line_number(true)
        .with_writer(writer)
        .init();
    log::info!("日志系统初始化完成（{component}），目录: {}", dir.display());
    Some(guard)
}

/// 记录错误：上下文 + anyhow 错误链 + 捕获点堆栈。
pub fn log_error(context: &str, e: &anyhow::Error) {
    let bt = std::backtrace::Backtrace::force_capture();
    log::error!("{context}: {e:#}\n--- 堆栈 ---\n{bt}");
}

fn base_data_dir() -> Option<PathBuf> {
    crate::report::data_dir().ok()
}
