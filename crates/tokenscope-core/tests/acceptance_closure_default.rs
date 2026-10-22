//! AC02：普通构建忽略全部新增验收变量，不生成追踪文件。
#![cfg(not(feature = "acceptance"))]

#[test]
fn ordinary_build_ignores_acceptance_closure_variables() {
    let root = std::env::temp_dir().join(format!(
        "tokenscope-ac02-normal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "closure_default_child",
            "--ignored",
            "--nocapture",
        ])
        .env("TOKENSCOPE_ACCEPTANCE_ROOT", &root)
        .env(
            "TOKENSCOPE_ACCEPTANCE_PRICING_BASE",
            "invalid-and-must-be-ignored",
        )
        .env("TOKENSCOPE_ACCEPTANCE_OPENER", "record")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "普通构建隔离断言失败：{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.join("acceptance-trace.jsonl").exists());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "由父用例以独立进程执行"]
fn closure_default_child() {
    let root = std::path::PathBuf::from(std::env::var("TOKENSCOPE_ACCEPTANCE_ROOT").unwrap());
    tokenscope::acceptance::bootstrap().unwrap();
    assert!(tokenscope::acceptance::root().is_none());
    assert!(!tokenscope::acceptance::bootstrapped());
    assert_eq!(
        tokenscope::import::ccs::default_source_path().unwrap(),
        dirs::home_dir().unwrap().join(".cc-switch/cc-switch.db")
    );
    assert_eq!(
        tokenscope::modelsdev::api_url().unwrap(),
        tokenscope::modelsdev::API_URL
    );
    assert_eq!(
        tokenscope::openrouter::api_url().unwrap(),
        tokenscope::openrouter::API_URL
    );
    // 显式临时路径上的真实 I/O 也不能意外启用追踪；不读写默认目录。
    drop(tokenscope::history::HistoryDb::open(&root.join("explicit-history.db")).unwrap());
    tokenscope::settings::ensure_toml(&root.join("explicit-settings.toml")).unwrap();
    assert!(!root.join("acceptance-trace.jsonl").exists());
}
