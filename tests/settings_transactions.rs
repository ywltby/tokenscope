//! SF02（安全与数据一致性审查 Task 2）：设置统一事务。
//!
//! 不变量（先于实现登记）：
//! 1. 全体应用内写者（关闭动作、记忆关闭、自动同步开关、来源配置、
//!    ensure_toml 首建/迁移）共享同一进程级读改写锁——后到的写者必须
//!    基于先到写者**提交后**的最新文件做读改写，不得用旧副本覆盖无关字段；
//! 2. 解析失败、校验失败、mutate 拒绝、写入失败都不落盘，原文件字节不动；
//! 3. 锁只在阻塞线程内等待（不跨 await 由调用方保证），测试用 channel
//!    同步点做确定性交错，不用 sleep 猜顺序，也不在锁内等双方 barrier。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;

use tokenscope::settings::{self, CloseAction};

static SEQ: AtomicU32 = AtomicU32::new(0);

fn tmp(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-settings-tx-{tag}-{}-{n}",
        std::process::id(),
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.join("settings.toml")
}

fn cleanup(path: &std::path::Path) {
    std::fs::remove_dir_all(path.parent().unwrap()).ok();
}

/// 两个 writer 的确定性交错：A（改 close_action）先进入临界区并在 mutate
/// 内暂停；B（关 auto_sync）在 A 持锁期间发起，只能等 A 提交后基于最新
/// 文件读改写。最终两个字段都必须生效——任何一方丢更新即失败。
#[test]
fn settings_transactions_preserve_unrelated_fields() {
    let path = tmp("interleave");
    std::fs::write(&path, "price_auto_sync = true\n").unwrap();

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    let p1 = path.clone();
    let writer_a = std::thread::spawn(move || {
        settings::update(&p1, |s| {
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap(); // 主线程恢复前一直持有写锁
            s.close_action = Some(CloseAction::Minimize);
            Ok(())
        })
    });
    entered_rx.recv().unwrap(); // A 已 load 最新值并进入临界区

    // A 持锁期间发起 B：B 必须等锁（而非与 A 互等 barrier）。
    let p2 = path.clone();
    let writer_b = std::thread::spawn(move || {
        settings::update(&p2, |s| -> Result<(), anyhow::Error> {
            s.price_auto_sync = false;
            Ok(())
        })
    });

    resume_tx.send(()).unwrap();
    writer_a.join().unwrap().unwrap();
    writer_b.join().unwrap().unwrap();

    let committed = settings::load(&path).unwrap();
    assert_eq!(
        committed.close_action,
        Some(CloseAction::Minimize),
        "close_action 不得被 B 的旧副本复原"
    );
    assert!(
        !committed.price_auto_sync,
        "关闭自动同步的更新不得被 A 的提交覆盖"
    );
    // close_action 是 A 唯一要写的字段：auto_sync 仍为 B 写的 false 已在上面断言。
    cleanup(&path);
}

/// ensure_toml 首建与并发 update 竞争：模板写入不得覆盖已提交的更新值，
/// update 也不得绕过 ensure_toml 的锁（文件在位时 ensure_toml 必须 no-op）。
#[test]
fn ensure_toml_cannot_overwrite_concurrent_update() {
    let path = tmp("ensure-vs-update"); // toml 缺失

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    let p1 = path.clone();
    let upd = std::thread::spawn(move || {
        settings::update(&p1, |s| {
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap();
            s.price_auto_sync = false;
            Ok(())
        })
    });
    entered_rx.recv().unwrap(); // update 持锁（文件缺失 → load 得默认值）

    let p2 = path.clone();
    let ensure = std::thread::spawn(move || settings::ensure_toml(&p2));

    resume_tx.send(()).unwrap();
    upd.join().unwrap().unwrap();
    ensure.join().unwrap().unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("price_auto_sync = false"),
        "更新值必须落盘且不被模板覆盖: {text}"
    );
    assert!(
        !text.contains("# price_auto_sync = true | false"),
        "ensure_toml 必须因文件已在位而 no-op: {text}"
    );
    assert!(!settings::load(&path).unwrap().price_auto_sync);
    cleanup(&path);
}

/// mutate 拒绝 / 文件损坏都不得触碰原文件字节。
#[test]
fn failed_update_preserves_original_bytes() {
    let path = tmp("failed-update");
    let original = "price_auto_sync = false\n";
    std::fs::write(&path, original).unwrap();

    // 校验失败：内存里改过的值不得落盘
    let rejected: anyhow::Result<()> = settings::update(&path, |s| {
        s.price_auto_sync = true;
        Err(anyhow::anyhow!("校验拒绝本次修改"))
    });
    assert!(rejected.is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        original,
        "mutate 拒绝后原文件必须原样保留"
    );

    // 损坏文件：解析失败，绝不静默用默认值覆盖
    let corrupt = "not [valid toml".to_string();
    std::fs::write(&path, &corrupt).unwrap();
    assert!(settings::update(&path, |_| Ok(())).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
    cleanup(&path);
}

/// ensure_toml 迁移路径（遗留 json → toml）也在锁内完成值保留。
#[test]
fn ensure_toml_migration_keeps_values() {
    let path = tmp("ensure-migrate");
    let dir = path.parent().unwrap();
    std::fs::write(dir.join("settings.json"), r#"{"price_auto_sync": false}"#).unwrap();

    settings::ensure_toml(&path).unwrap();
    let loaded = settings::load(&path).unwrap();
    assert!(!loaded.price_auto_sync, "迁移必须保留遗留值");
    assert!(
        !dir.join("settings.json").exists(),
        "迁移后遗留文件必须改名 .bak"
    );
    // 再次调用：文件在位 → no-op（值不变）
    settings::ensure_toml(&path).unwrap();
    assert!(!settings::load(&path).unwrap().price_auto_sync);
    cleanup(&path);
}
