//! 应用设置（M11）：`~/.tokenscope/settings.json`。
//!
//! 容忍策略：文件缺失/坏 JSON → 逐字段回退默认值（price_auto_sync 默认 true）；
//! 未知字段忽略，向前兼容。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// 单一来源配置（C1）：稳定 ID = 结构字段（claude/codex），不引入字符串 ID。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceConfig {
    /// 停用后完全不采集该来源（统计与状态页均可见"已停用"）。
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 显式目录；None = 工具默认根（~/.claude/projects、~/.codex/sessions）。
    #[serde(default)]
    pub dir: Option<String>,
}

impl Default for SourceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: None,
        }
    }
}

/// 各来源的覆盖配置；None = 全默认（启用 + 默认目录）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AgentSources {
    #[serde(default)]
    pub claude: Option<SourceConfig>,
    #[serde(default)]
    pub codex: Option<SourceConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// 定时自动同步在线价格（默认开启，每 24h）。
    #[serde(default = "default_true")]
    pub price_auto_sync: bool,
    /// C1：来源目录与启停配置（缺省 = 全启用 + 默认目录）。
    #[serde(default)]
    pub sources: AgentSources,
}

fn default_true() -> bool {
    true
}

impl Settings {
    /// 取某来源的有效配置（未配置 → 全默认）。
    pub fn source_config(&self, claude: bool) -> SourceConfig {
        let c = if claude {
            &self.sources.claude
        } else {
            &self.sources.codex
        };
        c.clone().unwrap_or_default()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            price_auto_sync: true,
            sources: AgentSources::default(),
        }
    }
}

pub fn settings_path() -> Result<PathBuf> {
    Ok(crate::report::data_dir()?.join("settings.json"))
}

/// 读设置：缺失 → 默认值；坏 JSON → Err（调用方警告并使用默认值）。
pub fn load(path: &Path) -> Result<Settings> {
    if !path.exists() {
        return Ok(Settings::default());
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("读设置失败: {}", path.display()))?;
    let s: Settings =
        serde_json::from_str(&text).with_context(|| format!("设置解析失败: {}", path.display()))?;
    Ok(s)
}

/// D3/R06：自动同步的允许判定——设置读取失败时返回 false（默认离线）。
/// 关闭自动同步的用户意图绝不能因设置损坏被悄悄重置成联网。
pub fn auto_sync_allowed(s: &Result<Settings, anyhow::Error>) -> bool {
    match s {
        Ok(s) => s.price_auto_sync,
        Err(_) => false,
    }
}

pub fn save(path: &Path, s: &Settings) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(s)?;
    std::fs::write(path, json).with_context(|| format!("写设置失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-m11-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    #[test]
    fn test_settings_roundtrip() {
        let path = tmp("roundtrip");
        let s = Settings {
            price_auto_sync: false,
            sources: crate::settings::AgentSources {
                claude: Some(SourceConfig {
                    enabled: false,
                    dir: Some("D:/logs/claude".into()),
                }),
                codex: None,
            },
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), s);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_sync_disabled_on_invalid_settings() {
        // D3：设置损坏 → 自动同步按离线处理，不悄悄恢复联网。
        assert!(auto_sync_allowed(&Ok(Settings::default())));
        assert!(!auto_sync_allowed(&Err(anyhow::anyhow!("bad json"))));
        let off = Settings {
            price_auto_sync: false,
            ..Default::default()
        };
        assert!(!auto_sync_allowed(&Ok(off)));
    }

    #[test]
    fn test_source_config_defaults_and_lookup() {
        // C1（test_source_config_roundtrip 的默认侧）：未配置 → 启用 + 默认目录。
        let s = Settings::default();
        let c = s.source_config(true);
        assert!(c.enabled);
        assert!(c.dir.is_none());
        // 旧版 settings.json（无 sources 字段）→ 向前兼容回默认。
        let path = tmp("legacy");
        std::fs::write(&path, r#"{"price_auto_sync": true}"#).unwrap();
        let s = load(&path).unwrap();
        assert!(s.source_config(false).enabled);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_settings_missing_defaults() {
        assert_eq!(
            load(Path::new("Z:/no-such/settings.json")).unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn test_settings_corrupt_is_err() {
        let path = tmp("corrupt");
        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_settings_unknown_fields_ignored() {
        let path = tmp("unknown");
        std::fs::write(&path, r#"{"price_auto_sync": false, "future_key": 123}"#).unwrap();
        let s = load(&path).unwrap();
        assert!(!s.price_auto_sync);
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
