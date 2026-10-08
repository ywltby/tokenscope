//! RC10：原生验收隔离入口的密闭性验证（只在 `--features acceptance` 下有意义）。
//!
//! 与 RC01 同一手法：**子进程**验证。验收根是进程级冻结状态（`OnceLock`），
//! 在同一进程里来回设置会污染其它测试，也不符合"启动最早阶段解析一次"的真实
//! 形态，因此每个场景都跑一个独立子进程（`current_exe()`），环境只传测试阶段
//! 与临时根目录。
//!
//! 三个具名验证（计划 RC10 第 5 项）：
//! - `native_acceptance_paths_are_hermetic`：所有读写目标都落在隔离根内，
//!   且真实 `~/.tokenscope` 一个字节都没变；
//! - `acceptance_mode_never_falls_back_to_real_sources`：来源目录未显式指定时
//!   用隔离根下的路径，**绝不**读真实 `~/.claude`、`~/.codex`；
//! - `acceptance_without_root_fails_before_collection`：缺失/非法验收根时在
//!   采集之前失败，并带上可指导下一步的错误信息。
//!
//! 全部使用合成日志与临时目录，不触碰真实 agent 日志。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

static SEQ: AtomicU32 = AtomicU32::new(0);

/// 子进程退出码约定（父进程按码判定，不看措辞）。
const CODE_OK: i32 = 0;
const CODE_NOT_ACCEPTANCE: i32 = 2;
const CODE_NO_ROOT: i32 = 3;
const CODE_ASSERTION: i32 = 4;

fn temp_root(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let d = std::env::temp_dir().join(format!(
        "tokenscope-acceptance-{tag}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 只有 acceptance 构建才执行这些验证；普通构建里隔离根通道整体不存在，
/// 子进程会立刻以 CODE_NOT_ACCEPTANCE 退出，父进程据此跳过（不误报通过）。
fn acceptance_supported() -> bool {
    cfg!(feature = "acceptance")
}

fn run_child(phase: &str, root: Option<&Path>) -> std::process::Output {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", phase, "--ignored", "--nocapture"]);
    // 环境：只传阶段与临时根；剥掉宿主里可能存在的验收根/真实性能开关，
    // 否则子进程会继承到不受控的路径开关。
    envs_of_interest(&mut cmd);
    match root {
        Some(r) => {
            cmd.env(tokenscope::acceptance::ENV_ROOT, r);
        }
        None => {
            cmd.env_remove(tokenscope::acceptance::ENV_ROOT);
        }
    }
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("启动验收子进程失败")
}

fn envs_of_interest(cmd: &mut Command) {
    for key in [
        tokenscope::acceptance::ENV_ROOT,
        "TOKENSCOPE_REAL_PERF",
        "RUST_LOG",
    ] {
        cmd.env_remove(key);
    }
}

fn real_data_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".tokenscope"))
}

/// 目录树的 (相对路径, 大小, mtime 纳秒) 指纹——用于证明真实数据目录未被触碰。
fn tree_fingerprint(root: &Path) -> Vec<(String, u64, i128)> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let meta = match std::fs::symlink_metadata(&p) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if p.is_dir() {
                stack.push(p.clone());
            }
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((
                rel,
                meta.len(),
                meta.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos() as i128)
                    .unwrap_or(-1),
            ));
        }
    }
    out.sort();
    out
}

#[test]
#[ignore]
/// 子进程阶段：验收根下的全部读写目标必须密闭在根内。
fn acceptance_child_hermetic_paths() {
    let Some(root) = enter_acceptance() else {
        std::process::exit(CODE_NO_ROOT);
    };
    // 安全闸：没进验收模式就绝不允许跑后面的写入（否则会污染真实缓存）。
    assert_eq!(
        tokenscope::acceptance::root().as_deref(),
        Some(root.as_path())
    );

    let mut failures: Vec<String> = Vec::new();
    let mut checked: Vec<String> = Vec::new();

    // 1) 解析层：每个默认路径派生函数都必须在根内。
    let data = tokenscope::report::data_dir().unwrap();
    let paths: Vec<(&str, PathBuf)> = vec![
        ("data_dir", data.clone()),
        ("cache.db", tokenscope::report::cache_file_path(None)),
        (
            "pricing-index",
            tokenscope::report::pricing_index_path(None),
        ),
        ("pricing.toml", tokenscope::report::pricing_file_path(None)),
        (
            "pricing-modelsdev.json",
            tokenscope::report::modelsdev_file_path(None),
        ),
        (
            "pricing-openrouter.json",
            tokenscope::report::openrouter_file_path(None),
        ),
        (
            "view-cache.json",
            tokenscope::report::view_cache_path().unwrap(),
        ),
        (
            "settings.toml",
            tokenscope::settings::settings_path().unwrap(),
        ),
        (
            "sources/claude",
            tokenscope::source::claude::ClaudeSource::default_root().unwrap(),
        ),
        (
            "sources/codex",
            tokenscope::source::codex::CodexSource::default_root().unwrap(),
        ),
    ];
    for (label, p) in &paths {
        checked.push(format!("{label}={}", p.display()));
        if !p.starts_with(&root) {
            failures.push(format!("{label} 落在隔离根外：{}", p.display()));
        }
    }

    // 2) 真实数据目录指纹：写入前后必须完全一致。
    let real = real_data_dir();
    let before: Vec<(String, u64, i128)> = real
        .as_ref()
        .map(|d| tree_fingerprint(d))
        .unwrap_or_default();

    // 3) 实际写入：设置模板 + 采集（缓存/索引/视图）+ 日志。
    std::fs::create_dir_all(&data).unwrap();
    tokenscope::settings::ensure_toml(&tokenscope::settings::settings_path().unwrap())
        .expect("写设置模板");
    let logs = tokenscope::logging::try_init("acceptance-child");
    if logs.status.state != "ok" {
        failures.push(format!(
            "验收根内日志初始化失败：state={} 原因={:?}",
            logs.status.state, logs.status.message
        ));
    }
    let opts = tokenscope::report::SummaryOptions {
        by: tokenscope::aggregate::GroupBy::Day,
        // 不注入任何显式路径——默认值本身就必须在隔离根内（本用例证明的就是这个）。
        ..Default::default()
    };
    // 走完整管线：缓存、价格索引与视图数据全部由默认路径派生。
    let report = tokenscope::report::summary(&opts).expect("验收根内采集应成功");
    checked.push(format!("summary groups={}", report.groups.len()));

    // 4) 落盘产物必须出现在根内，真实目录不得出现任何新文件。
    if !tokenscope::settings::settings_path().unwrap().exists() {
        failures.push("settings.toml 未写进隔离根".to_string());
    }
    let log_dir = data.join("logs");
    if !log_dir.exists() {
        failures.push(format!("日志目录不在隔离根内：{}", log_dir.display()));
    }
    let after: Vec<(String, u64, i128)> = real
        .as_ref()
        .map(|d| tree_fingerprint(d))
        .unwrap_or_default();
    if before != after {
        let diff_a: Vec<_> = after.iter().filter(|x| !before.contains(x)).collect();
        let diff_b: Vec<_> = before.iter().filter(|x| !after.contains(x)).collect();
        failures.push(format!(
            "真实 {} 被改动（新增 {diff_a:?} 移除 {diff_b:?}）",
            real.unwrap().display()
        ));
    }

    for c in &checked {
        println!("CHECK {c}");
    }
    if failures.is_empty() {
        println!("ACCEPTANCE_HERMETIC_OK");
        std::process::exit(CODE_OK);
    }
    for f in &failures {
        println!("ACCEPTANCE_HERMETIC_FAIL {f}");
    }
    std::process::exit(CODE_ASSERTION);
}

#[test]
#[ignore]
/// 子进程阶段：来源根未显式指定时也必须指向隔离根（哪怕目录不存在）。
fn acceptance_child_never_falls_back_to_real_sources() {
    let Some(root) = enter_acceptance() else {
        std::process::exit(CODE_NO_ROOT);
    };
    let mut failures: Vec<String> = Vec::new();

    // 默认根：必须是隔离根下路径，且该路径此刻**不存在**（准备脚本可预建）。
    let claude = tokenscope::source::claude::ClaudeSource::default_root().unwrap();
    let codex = tokenscope::source::codex::CodexSource::default_root().unwrap();
    for (label, p) in [("claude", &claude), ("codex", &codex)] {
        if !p.starts_with(&root) {
            failures.push(format!("{label} 默认根回退到真实路径：{}", p.display()));
        }
    }
    let home = dirs::home_dir().unwrap();
    if claude.starts_with(home.join(".claude")) || codex.starts_with(home.join(".codex")) {
        failures.push("来源默认根仍指向真实 ~/.claude 或 ~/.codex".to_string());
    }

    // source_status：设置里未指定目录时，上报的生效目录也只能在隔离根内。
    let statuses = tokenscope::report::source_status(&tokenscope::settings::Settings::default())
        .expect("source_status 可读");
    if statuses.len() < 2 {
        failures.push(format!(
            "source_status 应覆盖两个来源，实际 {}",
            statuses.len()
        ));
    }
    for st in &statuses {
        let p = PathBuf::from(&st.dir);
        if !p.starts_with(&root) {
            failures.push(format!("source_status 上报真实来源目录：{}", st.dir));
        }
        println!(
            "CHECK source {:?} dir={} state={}",
            st.agent, st.dir, st.state
        );
    }
    // 空隔离根下采集必须读到 0 次请求（读到任何数据都说明偷看了真实来源）。
    let report = tokenscope::report::summary(&tokenscope::report::SummaryOptions::default())
        .expect("空隔离根采集应成功");
    if report.totals.requests != 0 {
        failures.push(format!(
            "空隔离根下仍读到 {} 次请求，说明读到了真实来源",
            report.totals.requests
        ));
    }

    if failures.is_empty() {
        println!("ACCEPTANCE_NO_FALLBACK_OK");
        std::process::exit(CODE_OK);
    }
    for f in &failures {
        println!("ACCEPTANCE_NO_FALLBACK_FAIL {f}");
    }
    std::process::exit(CODE_ASSERTION);
}

/// 子进程公共入口：验收构建下按环境变量 bootstrap；返回 None = 没有合法根。
fn enter_acceptance() -> Option<PathBuf> {
    if !acceptance_supported_in_child() {
        println!("ACCEPTANCE_NOT_BUILT");
        std::process::exit(CODE_NOT_ACCEPTANCE);
    }
    match tokenscope::acceptance::bootstrap() {
        Ok(()) => tokenscope::acceptance::root(),
        Err(e) => {
            println!("BOOTSTRAP_ERROR {e}");
            None
        }
    }
}

fn acceptance_supported_in_child() -> bool {
    cfg!(feature = "acceptance")
}

// ── 父进程断言 ────────────────────────────────────────────────────

#[test]
fn native_acceptance_paths_are_hermetic() {
    if !acceptance_supported() {
        eprintln!("跳过：未启用 acceptance feature（cargo test --features acceptance 才执行）");
        return;
    }
    let root = temp_root("hermetic");
    let out = run_child("acceptance_child_hermetic_paths", Some(&root));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.code() == Some(CODE_OK),
        "子进程失败：code={:?}\nstdout={stdout}\nstderr={stderr}",
        out.status.code()
    );
    assert!(stdout.contains("ACCEPTANCE_HERMETIC_OK"), "{stdout}");
    // 逐项覆盖：数据目录派生的 6 个文件 + 设置 + 日志 + 两个来源根。
    for label in [
        "data_dir=",
        "cache.db=",
        "pricing-index=",
        "pricing.toml=",
        "pricing-modelsdev.json=",
        "pricing-openrouter.json=",
        "view-cache.json=",
        "settings.toml=",
        "sources/claude=",
        "sources/codex=",
    ] {
        assert!(
            stdout
                .lines()
                .any(|l| l.starts_with("CHECK ") && l.contains(label)),
            "缺少 {label} 的量测记录：\n{stdout}"
        );
    }
    // 真实文件确实写在根内。
    assert!(root.join("tokenscope/settings.toml").exists());
    assert!(root.join("tokenscope/logs").exists());
    assert!(
        root.join("tokenscope/cache.db").exists(),
        "缓存必须落在隔离根内"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn acceptance_mode_never_falls_back_to_real_sources() {
    if !acceptance_supported() {
        eprintln!("跳过：未启用 acceptance feature");
        return;
    }
    let root = temp_root("no-fallback");
    let out = run_child(
        "acceptance_child_never_falls_back_to_real_sources",
        Some(&root),
    );
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(
        out.status.code() == Some(CODE_OK),
        "子进程失败：code={:?}\nstdout={stdout}\nstderr={stderr}",
        out.status.code()
    );
    assert!(stdout.contains("ACCEPTANCE_NO_FALLBACK_OK"), "{stdout}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn acceptance_without_root_fails_before_collection() {
    if !acceptance_supported() {
        eprintln!("跳过：未启用 acceptance feature");
        return;
    }
    // (a) 完全没有环境变量：子进程拿不到根，任何采集都不发生。
    let out = run_child("acceptance_child_never_falls_back_to_real_sources", None);
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(
        out.status.code(),
        Some(CODE_NO_ROOT),
        "缺失验收根必须在采集前就退出：\n{stdout}"
    );

    // (b) 非法值：相对路径 / 不存在的目录 / 指向真实数据目录，都要在解析阶段失败。
    let home = dirs::home_dir().unwrap();
    let bad_cases: Vec<(String, PathBuf)> = vec![
        (
            "relative".to_string(),
            PathBuf::from("relative-is-not-absolute"),
        ),
        (
            "missing".to_string(),
            std::env::temp_dir().join(format!("tokenscope-no-such-{}", std::process::id())),
        ),
        ("real-data".to_string(), home.join(".tokenscope")),
        ("home".to_string(), home.clone()),
    ];
    for (label, value) in bad_cases {
        let out = run_child_with_env(&value);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        assert_eq!(
            out.status.code(),
            Some(CODE_NO_ROOT),
            "{label}：非法验收根必须拒绝启动\n{stdout}"
        );
        assert!(
            stdout.contains("BOOTSTRAP_ERROR"),
            "{label}：必须给出拒绝原因\n{stdout}"
        );
    }
}

/// 用任意（可能非法）验收根值跑一次子进程。
fn run_child_with_env(root: &Path) -> std::process::Output {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(exe);
    cmd.args([
        "--exact",
        "acceptance_child_never_falls_back_to_real_sources",
        "--ignored",
        "--nocapture",
    ]);
    envs_of_interest(&mut cmd);
    cmd.env(tokenscope::acceptance::ENV_ROOT, root);
    cmd.stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("启动子进程失败")
}
