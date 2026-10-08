//! RC10：原生生产验收的启动前置（仅 `--features acceptance` 构建使用）。
//!
//! 职责：
//! 1. 在**任何路径被解析之前**读取并验证 `TOKENSCOPE_ACCEPTANCE_ROOT`；缺失或
//!    非法直接返回错误，`run()` 据此拒绝启动（绝不带着真实 HOME 继续跑，那样
//!    拿到的"验收证据"其实读的是用户 1.2 GB 真实日志）；
//! 2. 改写 Tauri 配置，把 WebView 用户数据目录（EBWebView / localStorage，
//!    会影响首帧主题与恢复行为）也放进隔离根；同时给验收实例一个独立
//!    identifier，避免与正在运行的普通实例撞单实例互斥名/窗口类名；
//! 3. 不在前端 IPC 上暴露任何验收钩子——壳内没有额外的 command，根库的注入
//!    函数只在 acceptance 构建里存在且只被进程级环境变量驱动。

use std::path::{Path, PathBuf};

use tauri::Config;
use tauri::utils::config::AppDirectoriesOverride;
use tokenscope::acceptance::ENV_ROOT;

/// 启动最早阶段调用。返回隔离根；缺失/非法时返回 Err（调用方必须退出进程）。
pub fn start() -> Result<PathBuf, String> {
    let raw = std::env::var(ENV_ROOT).ok();
    start_with(raw)
}

/// 可注入形态（测试用；不读写环境变量，也不依赖进程内的其它状态）。
pub fn start_with(raw: Option<String>) -> Result<PathBuf, String> {
    let trimmed = raw.clone().unwrap_or_default();
    if trimmed.trim().is_empty() {
        return Err(format!(
            "验收构建要求进程级 {ENV_ROOT} 指向已准备好的隔离根；未设置时拒绝启动"
        ));
    }
    tokenscope::acceptance::bootstrap_with(raw)?;
    tokenscope::acceptance::root()
        .ok_or_else(|| format!("{ENV_ROOT}={trimmed} 已设置但未被解析为合法隔离根"))
}

/// 把验收根写进 Tauri 配置：WebView 用户数据、应用数据目录与 identifier。
///
/// `identifier` 改名后，单实例互斥量/窗口类名与普通实例不同，两个实例可以
/// 并存（验收脚本只关闭本次记录的那个 PID）。
pub fn configure(config: &mut Config, root: &Path) {
    config.app.app_directories_override =
        Some(AppDirectoriesOverride::Root(webview_data_dir(root)));
    // 单实例互斥量/窗口类名由 identifier 派生——改 name 让验收实例与普通实例并存。
    config.identifier = format!("{}.acceptance", config.identifier);
    log::info!(
        "验收模式：WebView 用户数据与 identifier 已指向隔离根 {}",
        root.display()
    );
}

/// WebView 用户数据目录（Windows 下即 EBWebView 所在目录）。
pub fn webview_data_dir(root: &Path) -> PathBuf {
    root.join("webview")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验收入口只由进程级环境变量驱动：缺失/非法一律拒绝，壳不得带着
    /// 真实 `~/.tokenscope` 与真实 agent 日志继续启动。
    #[test]
    fn start_rejects_missing_relative_and_missing_dir() {
        for (label, value) in [
            ("缺失", None),
            ("空串", Some("   ".to_string())),
            ("相对路径", Some("relative/is/not/absolute".to_string())),
            (
                "不存在的目录",
                Some(
                    std::env::temp_dir()
                        .join(format!("tokenscope-shell-no-such-{}", std::process::id()))
                        .to_string_lossy()
                        .to_string(),
                ),
            ),
        ] {
            let err = start_with(value).expect_err(&format!("{label} 必须拒绝启动"));
            assert!(
                err.contains(ENV_ROOT) || err.contains("绝对路径") || err.contains("不存在"),
                "{label}：错误信息必须可指导下一步：{err}"
            );
        }
    }

    /// WebView 用户数据目录必须在隔离根内（真实首帧与恢复行为的入口）。
    #[test]
    fn webview_data_dir_is_inside_root() {
        let root = PathBuf::from("C:/tmp/acceptance-root");
        assert_eq!(webview_data_dir(&root), root.join("webview"));
    }
}
