//! 应用设置（M11）：`~/.tokenscope/settings.json`。
//!
//! 容忍策略：文件缺失/坏 JSON → 逐字段回退默认值（price_auto_sync 默认 true）；
//! 未知字段忽略，向前兼容。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// 定时自动同步在线价格（默认开启，每 24h）。
    #[serde(default = "default_true")]
    pub price_auto_sync: bool,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            price_auto_sync: true,
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
        };
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap(), s);
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
