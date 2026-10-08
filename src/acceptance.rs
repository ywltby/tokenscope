//! RC10：原生生产验收的**隔离路径根**。
//!
//! 只有以 `--features acceptance` 构建时才识别进程级环境变量
//! `TOKENSCOPE_ACCEPTANCE_ROOT`；普通构建里本模块的所有查询都返回 `None`，
//! 路径解析行为与验收前完全一致（不引入任何环境变量后门）。
//!
//! 规则（计划 RC10）：
//! - 启动最早阶段解析并验证；缺失/非法一律**拒绝验收启动**，绝不回退真实
//!   HOME——回退会拿用户 1.2 GB 真实日志和 `~/.tokenscope` 当验收对象，既
//!   破坏隐私也会污染缓存；
//! - 隔离根覆盖**全部**落盘目标：TokenScope 数据目录（cache.db、settings.toml
//!   与迁移备份、双源价格快照、pricing-index、view-cache、日志）、两个来源根、
//!   窗口状态，以及壳侧的 WebView 用户数据目录；
//! - 一次解析、进程内冻结（`OnceLock`），因此不存在"运行中途换根"导致的
//!   半隔离状态；重复设置按当前值不变，测试通过子进程获得干净进程。

use std::path::PathBuf;

/// 验收根环境变量名（仅 acceptance 构建读取）。
pub const ENV_ROOT: &str = "TOKENSCOPE_ACCEPTANCE_ROOT";

/// 隔离根（已解析并冻结）；普通构建恒为 `None`。
pub fn root() -> Option<PathBuf> {
    imp::root()
}

/// TokenScope 自有数据目录的覆盖值（`None` = 走默认 `~/.tokenscope`）。
pub fn data_dir_override() -> Option<PathBuf> {
    imp::root().map(|r| r.join("tokenscope"))
}

/// agent 来源根的覆盖值。验收模式下**不允许**回退到真实 `~/.claude`、
/// `~/.codex`：即使隔离目录不存在也照实报告"目录不存在"，不改读真实来源。
pub fn source_dir_override(claude: bool) -> Option<PathBuf> {
    imp::root().map(|r| {
        r.join("sources")
            .join(if claude { "claude" } else { "codex" })
    })
}

/// 日志目录覆盖值（`None` = 由 data_dir 派生）。
pub fn logs_dir_override() -> Option<PathBuf> {
    data_dir_override().map(|d| d.join("logs"))
}

/// 启动最早阶段调用：解析并验证环境变量。返回 Err 时调用方必须拒绝启动。
pub fn bootstrap() -> Result<(), String> {
    imp::bootstrap_from_env()
}

/// 显式注入验收根（仅供 acceptance 构建的测试/子进程使用；不读环境变量）。
#[doc(hidden)]
pub fn bootstrap_with(value: Option<String>) -> Result<(), String> {
    imp::bootstrap_with(value)
}

/// 本进程是否已完成验收根解析（区分"未 bootstrap"与"普通构建"）。
#[doc(hidden)]
pub fn bootstrapped() -> bool {
    imp::bootstrapped()
}

#[cfg(not(feature = "acceptance"))]
mod imp {
    use std::path::PathBuf;

    /// 普通构建：完全没有这条通道，任何路径解析都保持原样。
    pub fn root() -> Option<PathBuf> {
        None
    }
    pub fn bootstrap_from_env() -> Result<(), String> {
        Ok(())
    }
    pub fn bootstrap_with(_value: Option<String>) -> Result<(), String> {
        Err("当前构建未启用 acceptance feature，不接受验收根注入".to_string())
    }
    pub fn bootstrapped() -> bool {
        false
    }
}

#[cfg(feature = "acceptance")]
mod imp {
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;

    use super::ENV_ROOT;

    /// 冻结的验收根：`None` 仍表示"已 bootstrap，但本进程不是验收实例"——
    /// 只有 bootstrap 成功才会写入，因此不会出现半解析状态。
    static ROOT: OnceLock<Option<PathBuf>> = OnceLock::new();

    pub fn root() -> Option<PathBuf> {
        ROOT.get().cloned().flatten()
    }

    pub fn bootstrapped() -> bool {
        ROOT.get().is_some()
    }

    pub fn bootstrap_from_env() -> Result<(), String> {
        bootstrap_with(std::env::var(ENV_ROOT).ok())
    }

    pub fn bootstrap_with(value: Option<String>) -> Result<(), String> {
        let resolved = match value {
            Some(raw) => validate(&raw)?,
            // 显式允许"无验收根"的普通 acceptance 构建运行（例如只跑单测）：
            // 只有 shell 的 run() 把它当硬性前置，库侧这里保持可组合。
            None => None,
        };
        if let Some(prev) = ROOT.get() {
            // 已冻结：不一致的二次注入必须暴露，不能静默沿用旧值。
            return match (prev.as_ref(), resolved.as_ref()) {
                (a, b) if a == b => Ok(()),
                _ => Err(format!(
                    "验收根已冻结为 {:?}，拒绝改为 {:?}",
                    prev.as_ref(),
                    resolved
                )),
            };
        }
        let _ = ROOT.set(resolved.clone());
        match resolved {
            Some(p) => {
                // 保证数据目录与来源根目录存在：缺失时由我们创建，而不是
                // 回退真实路径。
                std::fs::create_dir_all(p.join("tokenscope")).map_err(|e| {
                    format!(
                        "创建验收数据目录失败：{}（{e}）",
                        p.join("tokenscope").display()
                    )
                })?;
                log::info!("验收模式启动，隔离根 = {}", p.display());
                Ok(())
            }
            None => Ok(()),
        }
    }

    /// 去掉 Windows `\\?\` / `\\?\UNC\` 前缀：canonicalize 会加上它，而路径会
    /// 一路出现在 source_status、设置页与错误信息里（难看且部分外壳 API 不接受），
    /// 唯一性判断不受影响（两侧同样处理）。
    fn strip_verbatim(p: &Path) -> PathBuf {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(rest);
        }
        p.to_path_buf()
    }

    /// 验证隔离根：绝对路径、真实存在且是目录、不等于/不包含任何**真实**
    /// 用户数据目录。失败信息必须能指导使用者下一步（先跑准备脚本）。
    fn validate(raw: &str) -> Result<Option<PathBuf>, String> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(format!("{ENV_ROOT} 为空：验收构建需要隔离根绝对路径"));
        }
        let p = Path::new(trimmed);
        if !p.is_absolute() {
            return Err(format!("{ENV_ROOT} 必须是绝对路径，收到 {trimmed}"));
        }
        if !p.exists() {
            return Err(format!(
                "{ENV_ROOT} 指向的路径不存在：{trimmed}（先运行 scripts/prepare-native-acceptance.ps1）"
            ));
        }
        if !p.is_dir() {
            return Err(format!("{ENV_ROOT} 必须是目录：{trimmed}"));
        }
        let canon = p
            .canonicalize()
            .map_err(|e| format!("{ENV_ROOT} 解析失败：{trimmed}（{e}）"))?;
        let canon = strip_verbatim(&canon);
        let home = dirs::home_dir()
            .ok_or_else(|| "无法定位用户主目录，拒绝在未知环境下进入验收模式".to_string())?;
        let home_canon = strip_verbatim(&home.canonicalize().unwrap_or_else(|_| home.clone()));
        // 真实数据目录：~/.tokenscope（自有数据）、~/.claude、~/.codex（来源）。
        for protected in [
            home_canon.clone(),
            home_canon.join(".tokenscope"),
            home_canon.join(".claude"),
            home_canon.join(".codex"),
        ] {
            if canon == protected || protected.starts_with(&canon) {
                return Err(format!(
                    "{ENV_ROOT} 不得等于或包含真实用户数据目录（{} 与 {trimmed} 冲突）",
                    protected.display()
                ));
            }
        }
        Ok(Some(canon))
    }
}
