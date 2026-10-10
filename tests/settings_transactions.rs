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

use tokenscope::settings::{self, CloseAction, ConsentRead};

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

// ── P01：隐私同意记录与设置事务 ──────────────────────────────

/// 同意保存只改一个字段：价格同步、来源、关闭动作全部保留。
#[test]
fn privacy_accept_preserves_existing_settings() {
    let path = tmp("privacy-accept-keeps");
    std::fs::write(
        &path,
        "price_auto_sync = false\nclose_action = \"minimize\"\n\n[sources.claude]\nenabled = false\ndir = \"D:/logs/claude\"\n",
    )
    .unwrap();

    settings::accept_privacy_policy(&path).unwrap();

    let s = settings::load(&path).unwrap();
    assert!(s.privacy_policy_accepted, "同意位必须落盘");
    assert!(!s.price_auto_sync, "价格同步开关不得被重置");
    assert_eq!(s.close_action, Some(CloseAction::Minimize));
    assert!(!s.source_config(true).enabled, "来源配置不得被重置");
    assert_eq!(s.source_config(true).dir.as_deref(), Some("D:/logs/claude"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(
        text.contains("privacy_policy_accepted = true"),
        "落盘文本必须含同意位: {text}"
    );
    cleanup(&path);
}

/// 遗留 json 只在用户同意后的保存事务里读取并迁移；引导阶段不动它。
#[test]
fn privacy_accept_migrates_legacy_after_user_action() {
    let path = tmp("privacy-legacy-accept");
    let dir = path.parent().unwrap();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"price_auto_sync": false, "sources": {"claude": {"enabled": false, "dir": "D:/x"}}}"#,
    )
    .unwrap();

    // 同意之前：只有遗留 json → 不是授权，也不迁移、不改名
    assert_eq!(settings::read_consent(&path), ConsentRead::Missing);
    assert!(dir.join("settings.json").exists(), "引导阶段不得迁移");

    settings::accept_privacy_policy(&path).unwrap();

    let s = settings::load(&path).unwrap();
    assert!(s.privacy_policy_accepted, "同意必须写入 toml");
    assert!(!s.price_auto_sync, "遗留值必须保留");
    assert_eq!(s.source_config(true).dir.as_deref(), Some("D:/x"));
    assert!(!dir.join("settings.json").exists(), "迁移后遗留文件改名");
    assert!(dir.join("settings.json.bak").exists());
    assert_eq!(settings::read_consent(&path), ConsentRead::Granted);
    cleanup(&path);
}

/// 同意保存失败（目标不可写、遗留 json 损坏）保持闸门关闭且不动原文件。
#[test]
fn privacy_accept_save_failure_keeps_gate_closed() {
    // a) 原子替换失败：目标路径被目录占用
    let path = tmp("privacy-accept-fail");
    std::fs::write(&path, "price_auto_sync = false\n").unwrap();
    assert_eq!(settings::read_consent(&path), ConsentRead::NotGranted);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir_all(&path).unwrap();
    let failed = settings::accept_privacy_policy(&path);
    assert!(failed.is_err(), "保存失败必须返回错误: {failed:?}");
    assert_ne!(
        settings::read_consent(&path),
        ConsentRead::Granted,
        "保存失败不得留下授权"
    );
    // 故障恢复后重试成功
    std::fs::remove_dir_all(&path).unwrap();
    settings::accept_privacy_policy(&path).unwrap();
    assert_eq!(settings::read_consent(&path), ConsentRead::Granted);
    cleanup(&path);

    // b) 遗留 json 损坏：同意保存必须失败，原文件逐字节保留，继续阻断
    let path2 = tmp("privacy-accept-bad-legacy");
    let dir2 = path2.parent().unwrap();
    let corrupt = r#"{"price_auto_sync": true, "broken""#;
    std::fs::write(dir2.join("settings.json"), corrupt).unwrap();
    assert!(settings::accept_privacy_policy(&path2).is_err());
    assert_eq!(
        std::fs::read_to_string(dir2.join("settings.json")).unwrap(),
        corrupt,
        "损坏的遗留文件不得被覆盖"
    );
    assert!(!path2.exists(), "失败时不得写出半截 toml");
    assert_ne!(settings::read_consent(&path2), ConsentRead::Granted);
    cleanup(&path2);
}

/// 同意与并发普通设置写入互不丢失：两个字段都必须留在提交后的文件里，
/// 且后续设置写入不得把同意位覆盖回 false。
#[test]
fn privacy_concurrent_updates_preserve_acceptance() {
    let path = tmp("privacy-concurrent");
    std::fs::write(&path, "price_auto_sync = true\n").unwrap();

    let (entered_tx, entered_rx) = mpsc::channel::<()>();
    let (resume_tx, resume_rx) = mpsc::channel::<()>();
    let p1 = path.clone();
    let setter = std::thread::spawn(move || {
        settings::update(&p1, |s| {
            entered_tx.send(()).unwrap();
            resume_rx.recv().unwrap(); // 持锁等待：同意保存必须排队
            s.close_action = Some(CloseAction::Quit);
            Ok(())
        })
    });
    entered_rx.recv().unwrap();

    let p2 = path.clone();
    let accept = std::thread::spawn(move || settings::accept_privacy_policy(&p2));

    resume_tx.send(()).unwrap();
    setter.join().unwrap().unwrap();
    accept.join().unwrap().unwrap();

    let s = settings::load(&path).unwrap();
    assert_eq!(
        s.close_action,
        Some(CloseAction::Quit),
        "普通设置写入不得被同意保存的旧副本覆盖"
    );
    assert!(s.privacy_policy_accepted, "同意记录不得丢失");

    // 同意之后再写普通设置：字段级合并不丢同意位
    settings::update(&path, |s| {
        s.price_auto_sync = false;
        Ok(())
    })
    .unwrap();
    let s = settings::load(&path).unwrap();
    assert!(s.privacy_policy_accepted, "后续设置写入不得覆盖同意记录");
    assert!(!s.price_auto_sync);
    cleanup(&path);
}
