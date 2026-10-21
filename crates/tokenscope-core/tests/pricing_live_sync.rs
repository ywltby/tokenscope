//! 真实联网价格同步（默认 `#[ignore]`，不进常规套件与 CI）。
//!
//! 用途：更新 TLS 相关依赖（rustls / rustls-webpki / webpki-roots）后，确认
//! 对 models.dev 与 OpenRouter 的真实 HTTPS 同步仍然成功——证书链校验、
//! 下载与快照解析走的都是生产代码路径。快照只写临时目录，不触碰真实
//! `~/.tokenscope`。
//!
//! 显式运行（需要网络；本机经代理时同时设置 `HTTPS_PROXY`）：
//!
//! ```text
//! TOKENSCOPE_LIVE_PRICING=1 cargo test -p tokenscope --test pricing_live_sync -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use tokenscope::{modelsdev, openrouter};

fn enabled() -> bool {
    if std::env::var("TOKENSCOPE_LIVE_PRICING").ok().as_deref() == Some("1") {
        return true;
    }
    eprintln!("跳过：未设置 TOKENSCOPE_LIVE_PRICING=1");
    false
}

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-live-pricing-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
#[ignore = "真实联网：需显式设置 TOKENSCOPE_LIVE_PRICING=1"]
fn modelsdev_live_sync_succeeds() {
    if !enabled() {
        return;
    }
    let dir = fresh_dir("modelsdev");
    let path = dir.join("pricing-modelsdev.json");
    let report = modelsdev::sync(&path).expect("models.dev 同步失败");
    let snapshot = modelsdev::load_snapshot(&path)
        .expect("models.dev 快照读取失败")
        .expect("models.dev 快照缺失");
    eprintln!("models.dev：{} 条，{}", report.count, report.synced_at);
    assert!(report.count > 0, "models.dev 同步条目为 0");
    assert_eq!(snapshot.entries.len() as u64, report.count);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
#[ignore = "真实联网：需显式设置 TOKENSCOPE_LIVE_PRICING=1"]
fn openrouter_live_sync_succeeds() {
    if !enabled() {
        return;
    }
    let dir = fresh_dir("openrouter");
    let path = dir.join("pricing-openrouter.json");
    let report = openrouter::sync(&path).expect("OpenRouter 同步失败");
    let snapshot = openrouter::load_snapshot(&path)
        .expect("OpenRouter 快照读取失败")
        .expect("OpenRouter 快照缺失");
    eprintln!("OpenRouter：{} 条，{}", report.count, report.synced_at);
    assert!(report.count > 0, "OpenRouter 同步条目为 0");
    assert_eq!(snapshot.entries.len() as u64, report.count);
    std::fs::remove_dir_all(&dir).ok();
}
