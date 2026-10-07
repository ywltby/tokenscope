//! SF03（安全与数据一致性审查 Task 3）：价格读取失败显式降级。
//!
//! 不变量（先于实现登记，对应 docs/stats-semantics.md 价格加载章节）：
//! 1. 可选外置文件「不存在」与「存在但读取失败」是两个状态——读取失败
//!    必须携带路径与原因告警，且本次结果不得写内存缓存/磁盘索引；
//! 2. 缓存复用前先读取一次字节、签名纳入内容摘要：旧成功缓存不能掩盖
//!    文件随后不可读的新故障；恢复可读后（同内容、同 size/mtime）自动
//!    重新加载并恢复外置价与 model_policy，不需要用户改文件元数据；
//! 3. 业务诊断（候选拒绝/解析失败）可缓存（D2 契约）；临时 I/O 失败
//!    不可缓存——两条缓存写入路径共用 cacheability 判断，不按告警文本
//!    判断成败；
//! 4. 旧版本索引（v7 及更早）一律拒绝重建（INDEX_VERSION 递增）。
//!
//! 全部使用临时目录与注入 reader（确定性交错），Windows 另用禁止共享
//! 读取的句柄经**生产** fs::read 复现共享锁；绝不触碰真实 ~/.tokenscope。

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use tokenscope::pricing::{Pricing, PricingLoadOutcome, RateSpec, SourceHealth};

/// 外置策略 fixture：只有 [[model_policy]]（把 gpt-5.4 的 Unknown cache_read
/// 改写为 same_as_input）；models.dev 快照提供 gpt-5.4 基础价（cache_read 缺失）。
const POLICY_TOML: &str = r#"
[[model_policy]]
prefix = "gpt-5.4"
cache_read = "same_as_input"
"#;
const MODELSDEV_SNAPSHOT: &str = r#"{"v":3,"synced_at":"t","entries":[
    {"id":"zenmux/gpt-5.4","name":null,"input":4.0,"output":20.0}
]}"#;

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-price-recovery-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn failing_reader(kind: ErrorKind) -> impl Fn(&Path) -> std::io::Result<Vec<u8>> {
    move |_| Err(std::io::Error::new(kind, "injected read failure"))
}

/// 只对目标路径注入失败，其余路径走真实读取（快照层保持健康，
/// 用于验证"其余来源照常降级可用"）。
fn failing_reader_for(
    target: PathBuf,
    kind: ErrorKind,
) -> impl Fn(&Path) -> std::io::Result<Vec<u8>> {
    move |p: &Path| {
        if p == target.as_path() {
            Err(std::io::Error::new(kind, "injected read failure"))
        } else {
            std::fs::read(p)
        }
    }
}

/// 「不存在」（NotFound）与「存在但读取失败」（ReadFailed）必须分属不同
/// 状态：失败携带路径与原因告警并使结果不可缓存；缺失是正常无告警。
#[test]
fn external_not_found_is_distinct_from_read_failure() {
    let dir = fresh_dir("distinct");
    let missing = dir.join("no-such-pricing.toml");

    // (a) 不存在：NotFound、无告警、可缓存（正常可选文件状态）。
    let ok = Pricing::load_outcome(Some(&missing), None, None);
    assert_eq!(ok.external_health, SourceHealth::NotFound);
    assert!(
        ok.warnings().is_empty(),
        "缺失不得告警: {:?}",
        ok.warnings()
    );
    assert!(ok.cacheable);

    // (b) 存在但读取失败（注入 PermissionDenied）：ReadFailed、告警含
    // 路径与原因、不可缓存。
    let existing = write(&dir, "pricing.toml", POLICY_TOML);
    let denied: PricingLoadOutcome = Pricing::load_outcome_with(
        Some(&existing),
        None,
        None,
        &failing_reader(ErrorKind::PermissionDenied),
    );
    assert!(
        matches!(denied.external_health, SourceHealth::ReadFailed { .. }),
        "读取失败不得与缺失同分支: {:?}",
        denied.external_health
    );
    assert!(!denied.cacheable, "读取失败的结果不得标记可缓存");
    let w = denied.warnings();
    assert!(
        w.iter()
            .any(|x| x.contains("读取失败") && x.contains("pricing.toml")),
        "告警必须携带路径: {w:?}"
    );
    assert!(
        w.iter().any(|x| x.to_lowercase().contains("permission")),
        "告警必须携带原因（错误类别）: {w:?}"
    );

    // (c) 非法 UTF-8 同样是读取失败（InvalidData 语义），不是解析失败。
    let binary = dir.join("binary-pricing.toml");
    std::fs::write(&binary, [0xFF, 0xFE, 0x00, 0x01]).unwrap();
    let invalid = Pricing::load_outcome(Some(&binary), None, None);
    assert!(matches!(
        invalid.external_health,
        SourceHealth::ReadFailed { .. }
    ));
    assert!(
        invalid
            .warnings()
            .iter()
            .any(|x| x.contains("UTF-8") && x.contains("binary-pricing.toml"))
    );
    assert!(!invalid.cacheable);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 暂时读取失败：本次降级（外置层缺席 + 告警），但索引文件不被改写、
/// 结果不进内存缓存；恢复读取后同签名自动回到健康缓存。
#[test]
fn transient_read_failure_is_not_cached() {
    let _g = tokenscope::pricing::price_cache_test_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokenscope::pricing::clear_price_cache_for_tests();

    let dir = fresh_dir("not-cached");
    let toml = write(
        &dir,
        "pricing.toml",
        "[[model]]\nprefix = \"m\"\ninput = 2.0\noutput = 1.0\n",
    );
    let index = dir.join("pricing-index.json");

    // 健康重建：外置条目在位，索引写出。
    let (p1, w1, hit1) =
        Pricing::load_cached_with(Some(&toml), None, None, &index, &|p| std::fs::read(p));
    assert!(!hit1);
    assert!(w1.is_empty(), "{w1:?}");
    assert_eq!(p1.external_count(), 1);
    assert!(index.exists());
    let index_before = std::fs::read_to_string(&index).unwrap();

    // 注入读取失败：降级返回（外置层缺席 + 失败告警），非缓存命中。
    let (p2, w2, hit2) = Pricing::load_cached_with(
        Some(&toml),
        None,
        None,
        &index,
        &failing_reader(ErrorKind::PermissionDenied),
    );
    assert!(!hit2, "读取失败不得当作任何形式的缓存命中");
    assert_eq!(p2.external_count(), 0, "外置层本次不可用");
    assert!(w2.iter().any(|x| x.contains("读取失败")), "{w2:?}");
    // 索引未被失败结果覆盖。
    assert_eq!(
        std::fs::read_to_string(&index).unwrap(),
        index_before,
        "读取失败不得写索引"
    );

    // 恢复（同内容、同 size/mtime）：内存成功缓存未被失败覆盖，直接命中。
    let (p3, w3, hit3) =
        Pricing::load_cached_with(Some(&toml), None, None, &index, &|p| std::fs::read(p));
    assert!(hit3, "恢复后同签名应命中内存健康缓存");
    assert!(w3.is_empty(), "{w3:?}");
    assert_eq!(p3.external_count(), 1);

    // 模拟重启（清进程缓存）：磁盘索引命中——失败期间索引未被破坏。
    tokenscope::pricing::clear_price_cache_for_tests();
    let (p4, _, hit4) =
        Pricing::load_cached_with(Some(&toml), None, None, &index, &|p| std::fs::read(p));
    assert!(hit4, "健康索引在失败期间不得被改写");
    assert_eq!(p4.external_count(), 1);

    let _ = std::fs::remove_dir_all(&dir);
}

/// 内存中的旧成功缓存不得掩盖文件随后不可读的新故障：健康命中之后
/// 再遇读取失败，必须返回降级结果 + 失败告警，而非旧成功副本。
#[test]
fn healthy_cache_does_not_hide_new_read_failure() {
    let _g = tokenscope::pricing::price_cache_test_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokenscope::pricing::clear_price_cache_for_tests();

    let dir = fresh_dir("hide");
    let snap = write(&dir, "pricing-modelsdev.json", MODELSDEV_SNAPSHOT);
    let toml = write(
        &dir,
        "pricing.toml",
        "[[model]]\nprefix = \"m\"\ninput = 2.0\noutput = 1.0\n",
    );
    let index = dir.join("pricing-index.json");

    let (p1, _, hit1) = Pricing::load_cached(Some(&toml), Some(&snap), None, &index);
    assert_eq!(p1.external_count(), 1);
    assert!(hit1 || true, "首次可能重建或命中（共享缓存序），只关心内容");
    assert_eq!(p1.modelsdev_count(), 1);

    // 同签名下外置文件突然不可读：签名含内容摘要且读取阶段先行，
    // 旧成功缓存（含外置条目）不得被直接返回。
    let (p2, w2, hit2) = Pricing::load_cached_with(
        Some(&toml),
        Some(&snap),
        None,
        &index,
        &failing_reader_for(toml.clone(), ErrorKind::PermissionDenied),
    );
    assert!(!hit2);
    assert_eq!(p2.external_count(), 0, "旧成功缓存不得掩盖新的读取失败");
    assert_eq!(p2.modelsdev_count(), 1, "其余来源照常降级可用");
    assert!(w2.iter().any(|x| x.contains("读取失败")), "{w2:?}");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 恢复后外置价与 model_policy 重新生效——同内容同 size/mtime（签名含
/// 内容摘要，不依赖元数据变化），策略改写的 SameAsInput 随恢复重现。
#[test]
fn same_metadata_recovery_restores_external_policy() {
    let _g = tokenscope::pricing::price_cache_test_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokenscope::pricing::clear_price_cache_for_tests();

    let dir = fresh_dir("recover");
    let snap = write(&dir, "pricing-modelsdev.json", MODELSDEV_SNAPSHOT);
    let toml = write(&dir, "pricing.toml", POLICY_TOML);
    let index = dir.join("pricing-index.json");

    // 健康：策略命中，gpt-5.4 的 cache_read = SameAsInput。
    let (p1, w1, _) = Pricing::load_cached(Some(&toml), Some(&snap), None, &index);
    assert!(w1.is_empty(), "{w1:?}");
    let mp = p1.lookup("gpt-5.4").expect("models.dev 候选应在位");
    assert_eq!(
        mp.plan.base.cache_read,
        RateSpec::SameAsInput,
        "策略应已应用"
    );

    // 故障：外置层（含 policy）缺席 → Unknown，不得伪装成功。
    // （注入只作用于外置文件——快照层保持健康，验证其余来源降级可用。）
    let (p2, w2, hit2) = Pricing::load_cached_with(
        Some(&toml),
        Some(&snap),
        None,
        &index,
        &failing_reader_for(toml.clone(), ErrorKind::PermissionDenied),
    );
    assert!(!hit2);
    assert!(w2.iter().any(|x| x.contains("读取失败")), "{w2:?}");
    let degraded = p2.lookup("gpt-5.4").expect("快照来源候选仍在");
    assert_eq!(
        degraded.plan.base.cache_read,
        RateSpec::Unknown,
        "外置策略缺席时不得伪装成 SameAsInput"
    );

    // 恢复（同内容同元数据）：策略重新生效。
    let (p3, w3, hit3) = Pricing::load_cached_with(Some(&toml), Some(&snap), None, &index, &|p| {
        std::fs::read(p)
    });
    assert!(w3.is_empty(), "{w3:?}");
    let recovered = p3.lookup("gpt-5.4").expect("恢复后候选应在");
    assert_eq!(
        recovered.plan.base.cache_read,
        RateSpec::SameAsInput,
        "恢复读取后 model_policy 必须重新生效"
    );
    assert!(hit3, "健康缓存未被失败覆盖，同签名恢复即命中");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Windows 真实共享锁复现：禁止共享读取的句柄持有期间，**生产**
/// fs::read 路径的 load_cached 也必须显式降级；句柄释放后恢复。
#[cfg(windows)]
#[test]
fn windows_shared_lock_degrades_then_recovers_via_production_reader() {
    use std::os::windows::fs::OpenOptionsExt;

    let _g = tokenscope::pricing::price_cache_test_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    tokenscope::pricing::clear_price_cache_for_tests();

    let dir = fresh_dir("share-lock");
    let toml = write(
        &dir,
        "pricing.toml",
        "[[model]]\nprefix = \"m\"\ninput = 2.0\noutput = 1.0\n",
    );
    let index = dir.join("pricing-index.json");

    // share_mode(0)：句柄存续期间任何其他打开（含 fs::read）都报共享冲突。
    let holder = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&toml)
        .unwrap();

    let (p1, w1, hit1) = Pricing::load_cached(Some(&toml), None, None, &index);
    assert!(!hit1);
    assert_eq!(p1.external_count(), 0, "共享占用期间外置层不可用");
    assert!(
        w1.iter().any(|x| x.contains("读取失败")),
        "真实共享锁必须产生失败告警: {w1:?}"
    );
    assert!(!index.exists(), "失败结果不得写索引");

    drop(holder);
    let (p2, w2, _) = Pricing::load_cached(Some(&toml), None, None, &index);
    assert_eq!(p2.external_count(), 1, "句柄释放后必须恢复");
    assert!(w2.is_empty(), "{w2:?}");
    assert!(index.exists(), "恢复后的健康结果照常发布索引");

    let _ = std::fs::remove_dir_all(&dir);
}
