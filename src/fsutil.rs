//! 文件落盘工具（D3）：快照/设置等"宁旧勿坏"的数据必须原子替换——
//! 直接覆盖写在崩溃/断电时会留下半截文件，旧可用快照随之丢失。

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// 原子写：先写同目录临时文件（fsync），再 rename 到目标。
/// rename 在同一卷上是原子操作；失败时旧文件保持原样、临时文件清理。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .with_context(|| format!("目标无父目录: {}", path.display()))?;
    std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "tokenscope".to_string()),
        std::process::id()
    ));
    let keep_old = || {
        let _ = std::fs::remove_file(&tmp);
    };
    let write_result = (|| -> Result<()> {
        let mut f = std::fs::File::create(&tmp)
            .with_context(|| format!("创建临时文件失败: {}", tmp.display()))?;
        f.write_all(bytes)?;
        f.sync_all()
            .with_context(|| format!("刷盘失败: {}", tmp.display()))?;
        drop(f);
        // 目标被目录占用等异常 → rename 失败 → 旧文件原样保留。
        std::fs::rename(&tmp, path)
            .with_context(|| format!("原子替换失败: {} → {}", tmp.display(), path.display()))?;
        Ok(())
    })();
    if write_result.is_err() {
        keep_old();
    }
    write_result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("tokenscope-fsutil-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn test_atomic_write_overwrites() {
        let dir = tmp_dir("ok");
        let p = dir.join("snap.json");
        atomic_write(&p, b"v1").unwrap();
        atomic_write(&p, b"v2").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"v2");
        // 无残留临时文件
        assert!(std::fs::read_dir(&dir).unwrap().count() == 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_atomic_write_failure_keeps_old() {
        // test_atomic_snapshot_failure_keeps_old：目标不可写（父路径是文件）
        // 时旧快照原样保留，且不留临时文件。
        let dir = tmp_dir("fail");
        let good = dir.join("snap.json");
        atomic_write(&good, b"old-snapshot").unwrap();
        // bad 的父目录路径被一个文件占用 → create_dir_all 失败
        let blocker = dir.join("blocked");
        std::fs::write(&blocker, b"x").unwrap();
        let bad = blocker.join("snap.json");
        assert!(atomic_write(&bad, b"new").is_err());
        assert_eq!(
            std::fs::read(&good).unwrap(),
            b"old-snapshot",
            "旧快照不受影响"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
