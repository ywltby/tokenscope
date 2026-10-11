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

/// 恢复可见性判定（G-P3-3）：保存的窗口矩形与任一可用监视器可见区域
/// 有交集才允许恢复。多显示器拔除/改布局后，旧 x/y 可能完全落在屏幕外，
/// 非最大化窗口会恢复到不可见位置且默认尺寸兜底因状态文件存在而不生效。
/// 返回 false 时调用方保留 tauri.conf 的默认尺寸/居中位置。
pub fn restorable_on_any_monitor(ws: &WindowState, monitors: &[(i32, i32, u32, u32)]) -> bool {
    if ws.maximized || monitors.is_empty() {
        // 最大化/拿不到监视器清单时不拦：最大化由系统落位，空清单（极
        // 少见的枚举失败）宁可恢复也不把用户窗口钉死在默认位置。
        return true;
    }
    let (wx, wy) = (ws.x as i32, ws.y as i32);
    let (ww, wh) = (ws.width as i32, ws.height as i32);
    monitors.iter().any(|(mx, my, mw, mh)| {
        // I-P3-1：状态文件是用户可编辑 JSON，width/height 可以是大到
        // `as i32` 饱和的值——用 i64 中间量做加法，debug 构建不 panic，
        // release 构建不回绕误判。
        let right = (*mx) as i64 + (*mw) as i64;
        let bottom = (*my) as i64 + (*mh) as i64;
        (wx as i64) < right
            && (wx as i64) + (ww as i64) > (*mx) as i64
            && (wy as i64) < bottom
            && (wy as i64) + (wh as i64) > (*my) as i64
    })
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

    fn ws(x: f64, y: f64, w: f64, h: f64) -> WindowState {
        WindowState {
            width: w,
            height: h,
            x,
            y,
            maximized: false,
        }
    }

    #[test]
    fn test_restorable_on_any_monitor() {
        // 单显示器 (0,0,1920,1080)。
        let monitors = vec![(0, 0, 1920, 1080)];
        assert!(restorable_on_any_monitor(
            &ws(100.0, 50.0, 1280.0, 820.0),
            &monitors
        ));
        // 部分相交（右边缘越界）→ 仍可恢复。
        assert!(restorable_on_any_monitor(
            &ws(1800.0, 100.0, 1280.0, 820.0),
            &monitors
        ));
        // 完全在屏幕外（左侧 1920 之外）→ 拒绝恢复（G-P3-3）。
        assert!(!restorable_on_any_monitor(
            &ws(3000.0, 100.0, 400.0, 300.0),
            &monitors
        ));
        assert!(!restorable_on_any_monitor(
            &ws(-1000.0, -1000.0, 400.0, 300.0),
            &monitors
        ));
        // 仅边角相触（间隙为 0）不算相交。
        assert!(!restorable_on_any_monitor(
            &ws(1920.0, 100.0, 400.0, 300.0),
            &monitors
        ));
        // 第二显示器覆盖时第一屏外位置可恢复。
        let dual = vec![(0, 0, 1920, 1080), (1920, 0, 1920, 1080)];
        assert!(restorable_on_any_monitor(
            &ws(3000.0, 100.0, 400.0, 300.0),
            &dual
        ));
    }

    #[test]
    fn test_restorable_maximized_and_empty_monitors_never_block() {
        let mut maximized = ws(3000.0, 100.0, 400.0, 300.0);
        maximized.maximized = true;
        assert!(restorable_on_any_monitor(&maximized, &[]));
        // 监视器枚举失败（空清单）不拦：宁可恢复也不钉死默认位置。
        assert!(restorable_on_any_monitor(
            &ws(3000.0, 100.0, 400.0, 300.0),
            &[]
        ));
    }

    #[test]
    fn test_restorable_saturating_values_do_not_overflow() {
        // I-P3-1：状态文件是用户可编辑 JSON，width/height/x/y 可写 1e300
        // 量级——`as i32` 饱和后 i64 加法不得 panic（debug overflow-checks），
        // 判定确定：与监视器相交的巨型矩形可恢复，屏外饱和值拒绝。
        let monitors = vec![(0, 0, 1920, 1080)];
        assert!(restorable_on_any_monitor(
            &ws(100.0, 100.0, 1e300, 300.0),
            &monitors
        ));
        assert!(!restorable_on_any_monitor(
            &ws(1e300, 1e300, 1e300, 1e300),
            &monitors
        ));
        assert!(!restorable_on_any_monitor(
            &ws(-1e300, -1e300, 1e300, 1e300),
            &monitors
        ));
    }
}
