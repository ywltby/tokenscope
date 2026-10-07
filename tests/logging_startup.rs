//! SF06（安全与数据一致性审查 Task 6）：日志初始化可失败，GUI 启动可继续。
//!
//! 不变量：
//! 1. 日志目录无法解析/创建/写入、appender 初始化失败、全局 subscriber
//!    已存在——都进入可观测降级路径（stderr 或关闭输出 + 诊断），不 panic；
//! 2. 成功路径返回 WorkerGuard（进程生命周期持有）；失败不返回假 guard；
//! 3. 全局 subscriber 是进程级单例——相关测试放隔离子进程（同测试可执行
//!    文件 + 阶段环境变量），避免多测试线程抢全局 subscriber。
//!
//! 隔离要求：所有子进程使用注入的临时数据目录（try_init_in），不触碰
//! 真实 ~/.tokenscope；子进程退出码非 0 即失败，不得当通过。

use std::path::PathBuf;
use std::process::Command;

const STAGE_ENV: &str = "TOKENSCOPE_LOG_STAGE";
const DIR_ENV: &str = "TOKENSCOPE_LOG_DIR";

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-log-startup-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn child_dir() -> PathBuf {
    std::env::var_os(DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("子进程必须由父进程通过 {DIR_ENV} 传入隔离目录"))
}

fn run_stage(self_test: &str, stage: &str, dir: &std::path::Path) -> std::process::Output {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(&exe)
        .args(["--exact", self_test, "--nocapture"])
        .env(STAGE_ENV, stage)
        .env(DIR_ENV, dir)
        .output()
        .expect("启动日志回归子进程失败");
    assert!(
        out.status.success(),
        "子进程阶段 {stage} 退出码 {:?}——失败不得当通过\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    out
}

/// logs 路径是普通文件 → 初始化必须进入 stderr 降级（不 panic、不阻断），
/// 且"启动后标记"继续执行；guard 不伪装成功（None）。
#[test]
fn unwritable_log_target_does_not_abort_startup() {
    match std::env::var(STAGE_ENV).ok().as_deref() {
        None => {
            let dir = fresh_dir("unwritable");
            // base/logs = 普通文件：create_dir_all 必然失败。
            std::fs::write(dir.join("logs"), b"not a directory").unwrap();
            let out = run_stage(
                "unwritable_log_target_does_not_abort_startup",
                "child",
                &dir,
            );
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                stdout.contains("STARTUP-CONTINUED"),
                "初始化失败后必须继续执行启动标记: {stdout}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
        Some("child") => {
            let base = child_dir();
            let logging = tokenscope::logging::try_init_in("test", &base);
            let st = logging.status;
            assert_eq!(st.state, "stderr", "文件失败应退回 stderr: {st:?}");
            assert!(
                st.message.as_deref().unwrap_or_default().contains("失败"),
                "诊断必须携带原因: {st:?}"
            );
            assert!(logging.guard.is_none(), "失败路径不得返回假 guard");
            // 降级路径的 subscriber 可用：继续执行启动后标记（不 panic）。
            log::info!("启动继续标记");
            println!("STARTUP-CONTINUED");
        }
        other => panic!("未知阶段 {other:?}"),
    }
}

/// 全局 subscriber 已存在（重复初始化）不是 panic：状态 off、无 guard，
/// 原有 subscriber 不被顶替。
#[test]
fn existing_subscriber_is_not_a_panic() {
    match std::env::var(STAGE_ENV).ok().as_deref() {
        None => {
            let dir = fresh_dir("dup");
            run_stage("existing_subscriber_is_not_a_panic", "child", &dir);
            let _ = std::fs::remove_dir_all(&dir);
        }
        Some("child") => {
            let base = child_dir();
            // 先占全局 subscriber（模拟重复初始化）。
            tracing_subscriber::fmt()
                .with_writer(std::io::sink)
                .try_init()
                .expect("fixture：首次占用全局 subscriber 应成功");
            let logging = tokenscope::logging::try_init_in("test", &base);
            let st = logging.status;
            assert_eq!(st.state, "off", "重复初始化应报告 off: {st:?}");
            assert!(
                st.message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("重复初始化"),
                "诊断必须说明原因: {st:?}"
            );
            assert!(logging.guard.is_none());
            println!("STARTUP-CONTINUED");
        }
        other => panic!("未知阶段 {other:?}"),
    }
}

/// 有效目录：状态 ok、WorkerGuard 存在；guard 持有至进程退出 → 子进程
/// 退出后日志文件已落盘（父进程断言）。
#[test]
fn successful_logger_keeps_guard() {
    match std::env::var(STAGE_ENV).ok().as_deref() {
        None => {
            let dir = fresh_dir("valid");
            let out = run_stage("successful_logger_keeps_guard", "child", &dir);
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(stdout.contains("STARTUP-CONTINUED"), "{stdout}");
            // 子进程退出（guard drop + flush）后日志文件应存在且非空。
            let logs = dir.join("logs");
            let entries: Vec<_> = std::fs::read_dir(&logs)
                .expect("logs 目录必须存在")
                .filter_map(|e| e.ok())
                .collect();
            assert!(
                entries.iter().any(|e| e
                    .file_name()
                    .to_string_lossy()
                    .starts_with("tokenscope.log")),
                "滚动日志文件必须存在: {:?}",
                entries.iter().map(|e| e.file_name()).collect::<Vec<_>>()
            );
            let log_path = entries
                .iter()
                .find(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("tokenscope.log")
                })
                .unwrap()
                .path();
            let body = std::fs::read_to_string(&log_path).unwrap();
            assert!(
                body.contains("启动继续标记"),
                "guard 生命周期内的日志必须落盘: {body}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
        Some("child") => {
            let base = child_dir();
            let logging = tokenscope::logging::try_init_in("test", &base);
            let st = logging.status;
            assert_eq!(st.state, "ok", "有效目录必须成功: {st:?}");
            assert!(logging.guard.is_some(), "成功路径必须持有 WorkerGuard");
            log::info!("启动继续标记");
            println!("STARTUP-CONTINUED");
            // guard 随 logging drop：作用域结束 = 子进程退出前 flush。
        }
        other => panic!("未知阶段 {other:?}"),
    }
}
