//! 主窗口状态持久化（M8）：尺寸/位置/最大化标记存
//! `~/.tokenscope/window-state.json`，启动恢复。
//!
//! 容忍策略：文件缺失或坏 JSON → 返回 None，窗口用 tauri.conf.json 的默认尺寸，
//! 下一次保存事件会重写文件。存储为物理像素（同机恢复场景）。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WindowState {
    pub width: f64,
    pub height: f64,
    pub x: f64,
    pub y: f64,
    pub maximized: bool,
}

pub fn state_path() -> Result<PathBuf> {
    Ok(tokenscope::report::data_dir()?.join("window-state.json"))
}

/// 读状态：缺失 → Ok(None)；坏 JSON → Err（调用方警告并忽略）。
pub fn load(path: &Path) -> Result<Option<WindowState>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读窗口状态失败: {}", path.display()))?;
    let ws: WindowState = serde_json::from_str(&text)
        .with_context(|| format!("窗口状态解析失败: {}", path.display()))?;
    Ok(Some(ws))
}

pub fn save(path: &Path, ws: &WindowState) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(ws)?;
    tokenscope::fsutil::atomic_write(path, json.as_bytes())
        .with_context(|| format!("写窗口状态失败: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-m8-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("window-state.json")
    }

    #[test]
    fn test_window_state_roundtrip() {
        let path = tmp("roundtrip");
        let ws = WindowState {
            width: 1280.0,
            height: 820.0,
            x: 100.0,
            y: 50.0,
            maximized: false,
        };
        save(&path, &ws).unwrap();
        assert_eq!(load(&path).unwrap(), Some(ws));
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_window_state_missing_is_none() {
        assert!(
            load(Path::new("Z:/no-such/window-state.json"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_window_state_corrupt_is_err() {
        let path = tmp("corrupt");
        std::fs::write(&path, "not json").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_dir_all(path.parent().unwrap()).ok();
    }
}
