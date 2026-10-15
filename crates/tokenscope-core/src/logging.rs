//! 统一日志（M8/M9 完善；SF06 可失败初始化）：CLI 与 GUI 共用。
//!
//! - 落盘 `~/.tokenscope/logs/tokenscope.log`（每日滚动，非阻塞写入）；
//! - 每行自带 时间戳（UTC）/ 等级 / 目标模块:行号 / 消息；
//! - `log_error` 附加 anyhow 错误链与捕获点调用堆栈，便于排障；
//! - 级别可用 `RUST_LOG` 覆盖（默认 info）。
//!
//! SF06（安全与数据一致性审查 Task 6）：辅助日志**不得阻断启动**——
//! 目录无法解析/创建/写入、appender 初始化失败、全局 subscriber 已存在
//! 都进入可观测降级路径（退回 stderr 或关闭输出并给出诊断），
//! 绝不 expect/panic；成功路径的 WorkerGuard 仍由调用方持有至进程退出。

use std::path::Path;

use serde::Serialize;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::RollingFileAppender;
use tracing_subscriber::EnvFilter;

/// 日志初始化状态（GUI 经 startup_diagnostics 只读获取，前端展示非阻断通知）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogInitStatus {
    /// `ok` = 文件日志正常；`stderr` = 文件日志不可用，已退回 stderr；
    /// `off` = 无任何日志输出。
    pub state: &'static str,
    /// 文件日志目录（state = ok 时非空）。
    pub dir: Option<String>,
    /// 降级原因（state != ok 时非空，可直接展示）。
    pub message: Option<String>,
}

/// 初始化产物：状态（可托管/返回给前端）+ WorkerGuard（进程生命周期持有）。
pub struct Logging {
    pub status: LogInitStatus,
    /// 成功路径持有至进程退出；降级路径为 None（不返回假 guard）。
    pub guard: Option<WorkerGuard>,
}

/// 生产入口：数据目录 = `~/.tokenscope`。
pub fn try_init(component: &str) -> Logging {
    match crate::report::data_dir() {
        Ok(dir) => try_init_in(component, &dir),
        Err(e) => {
            let msg = format!("无法定位数据目录，文件日志不可用（{e:#}）");
            Logging {
                status: stderr_or_off(&msg),
                guard: None,
            }
        }
    }
}

/// 可注入数据目录的初始化（测试隔离用；生产走 [`try_init`]）。
/// 任何失败都返回降级状态，不 panic。
#[doc(hidden)]
pub fn try_init_in(component: &str, base: &Path) -> Logging {
    let dir = base.join("logs");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        let msg = format!("日志目录创建失败: {}（{e:#}）", dir.display());
        return Logging {
            status: stderr_or_off(&msg),
            guard: None,
        };
    }
    // SF06：RollingFileAppender::builder().build 返回 Result——显式处理，
    // 不依赖 tracing_appender::rolling::daily 内部的 expect。
    let appender = RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("tokenscope.log")
        .build(&dir);
    let appender = match appender {
        Ok(a) => a,
        Err(e) => {
            let msg = format!("文件日志不可用: {}（{e}）", dir.display());
            return Logging {
                status: stderr_or_off(&msg),
                guard: None,
            };
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let init = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_line_number(true)
        .with_writer(writer)
        .try_init();
    if init.is_err() {
        // 全局 subscriber 已存在（重复初始化）：不是可恢复的文件故障，
        // 丢弃本 writer（不返回假 guard），状态 off。
        return Logging {
            status: LogInitStatus {
                state: "off",
                dir: None,
                message: Some("全局日志 subscriber 已存在（重复初始化），本次输出关闭".to_string()),
            },
            guard: None,
        };
    }
    log::info!("日志系统初始化完成（{component}），目录: {}", dir.display());
    Logging {
        status: LogInitStatus {
            state: "ok",
            dir: Some(dir.display().to_string()),
            message: None,
        },
        guard: Some(guard),
    }
}

/// 文件日志不可用时的降级：尝试 stderr subscriber，仍失败则关闭输出。
/// 诊断经 eprintln 直出（不依赖同一文件 logger，不递归报错）。
fn stderr_or_off(reason: &str) -> LogInitStatus {
    eprintln!("TokenScope 日志降级: {reason}");
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let init = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_line_number(true)
        .with_writer(std::io::stderr)
        .try_init();
    match init {
        Ok(()) => {
            log::warn!("{reason}（已退回 stderr 输出）");
            LogInitStatus {
                state: "stderr",
                dir: None,
                message: Some(reason.to_string()),
            }
        }
        Err(_) => {
            eprintln!("TokenScope 日志输出关闭：stderr subscriber 也不可用（重复初始化）");
            LogInitStatus {
                state: "off",
                dir: None,
                message: Some(format!("{reason}（stderr 亦不可用：重复初始化）")),
            }
        }
    }
}

/// 记录错误：上下文 + anyhow 错误链 + 捕获点堆栈。
pub fn log_error(context: &str, e: &anyhow::Error) {
    let bt = std::backtrace::Backtrace::force_capture();
    log::error!("{context}: {e:#}\n--- 堆栈 ---\n{bt}");
}
