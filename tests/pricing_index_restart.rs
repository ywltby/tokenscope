//! F07（修复后复核 Task 1）：价格索引冷启动回归必须在**独立子进程**中
//! 经过真实磁盘链路。
//!
//! 背景：`load_cached` 有进程内缓存 PRICE_CACHE——同进程单测第一次调用
//! 后内存已持有结果，篡改磁盘索引再调 `load_cached` 只会命中内存，
//! "必须失效重建"的断言单独运行必失败；此前整套测试通过依赖其他并行
//! 测试碰巧替换全局缓存。本文件用集成测试可执行文件的隔离子进程模式：
//! 父进程编排阶段并经 serde 篡改磁盘索引，子进程运行真实加载链并断言；
//! 每个子进程退出码必须为 0，失败不得当通过。

use std::path::PathBuf;
use std::process::Command;

use tokenscope::pricing::{
    INDEX_VERSION, PricePlan, PriceRates, Pricing, PricingIndex, RateSpec, load_index, save_index,
};

const STAGE_ENV: &str = "TOKENSCOPE_IDX_RESTART_STAGE";
const DIR_ENV: &str = "TOKENSCOPE_IDX_RESTART_DIR";

/// OpenRouter 快照：prompt 2e-6、completion 3e-6、cache_read 5e-7、
/// cache_write 0（USD/token → USD/M：读 0.5、写 0）。
const OR_SNAPSHOT: &str = r#"{"v":2,"synced_at":"t","entries":[
    {"id":"or/model","name":null,
     "prompt":0.000002,"completion":0.000003,
     "cache_read":0.0000005,"cache_write":0}
]}"#;

const EXTERNAL_TOML: &str = "[[model]]\nprefix = \"m\"\ninput = 2.0\noutput = 1.0\n";

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-idx-restart-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

/// 子进程取父进程传入的 fixture 目录；缺失即 panic——禁止回落默认数据目录。
fn child_dir() -> PathBuf {
    std::env::var_os(DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("子进程必须由父进程通过 {DIR_ENV} 传入隔离目录"))
}

/// 以隔离子进程运行阶段：同一测试可执行文件、`--exact` 只选本测试、
/// 环境只传阶段名与 fixture 目录。退出码非 0 即父进程断言失败。
fn run_stage(self_test: &str, stage: &str, dir: &std::path::Path) {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(&exe)
        .args(["--exact", self_test, "--nocapture"])
        .env(STAGE_ENV, stage)
        .env(DIR_ENV, dir)
        .output()
        .expect("启动索引回归子进程失败");
    assert!(
        out.status.success(),
        "子进程阶段 {stage} 退出码 {:?}——失败不得继续当通过\n--- stdout ---\n{}\n--- stderr ---\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// R01 冷启动回归：旧语义索引（有效签名、缓存价被错误置 Unknown）必须
/// 被拒绝并按来源重建；重建出的当前版索引在另一个新进程中可命中且价格
/// 一致。全程三个独立子进程 + 父进程 serde 篡改，磁盘链路真实经过。
/// F04 Task 4 起索引 v7 仅接受当前版本：旧 v6（含 v4/v5）一律拒绝——
/// fixture 篡改版本 = 6 覆盖"拒绝旧 v6 派生错误"。
#[test]
fn test_pricing_index_rebuilds_stale_cache_rates() {
    match std::env::var(STAGE_ENV).ok().as_deref() {
        None => {
            let dir = fresh_dir("r01");
            write(&dir, "pricing-openrouter.json", OR_SNAPSHOT);
            let index = dir.join("pricing-index.json");
            let name = "test_pricing_index_rebuilds_stale_cache_rates";
            // 阶段 1：seed 子进程从合法来源生成有效签名索引
            run_stage(name, "seed", &dir);
            assert!(index.exists(), "seed 子进程必须写出索引文件");
            // 阶段 2：父进程经 serde 把索引篡改为旧 v6 语义（同 sig）
            let mut stale: PricingIndex = load_index(&index).unwrap().unwrap();
            stale.v = 6;
            for e in &mut stale.entries {
                e.plan = Some(PricePlan {
                    base: PriceRates {
                        input: RateSpec::Fixed(2.0),
                        output: RateSpec::Fixed(3.0),
                        cache_write: RateSpec::Unknown,
                        cache_read: RateSpec::Unknown,
                    },
                    ..Default::default()
                });
                e.input = Some(2.0);
                e.output = Some(3.0);
                e.cache_write = None;
                e.cache_read = None;
            }
            save_index(&index, &stale).unwrap();
            // 阶段 3：新进程冷加载——必须失效重建出正确缓存价
            run_stage(name, "cold_rebuild", &dir);
            // 阶段 4：再一个新进程——干净 v6 索引可命中且价格一致
            run_stage(name, "readable", &dir);
            let _ = std::fs::remove_dir_all(&dir);
        }
        Some("seed") => {
            let dir = child_dir();
            let snap = dir.join("pricing-openrouter.json");
            let index = dir.join("pricing-index.json");
            let (p, warnings, hit) = Pricing::load_cached(None, None, Some(&snap), &index);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(!hit, "首次加载必须从来源重建");
            assert_eq!(
                p.lookup("model").unwrap().plan.base.cache_read,
                RateSpec::Fixed(0.5),
                "OR 数值缓存价必须保留（0.0000005 USD/token → 0.5 USD/M）"
            );
            assert!(index.exists(), "load_cached 必须写出索引文件");
        }
        Some("cold_rebuild") => {
            let dir = child_dir();
            let snap = dir.join("pricing-openrouter.json");
            let index = dir.join("pricing-index.json");
            let (p, _, hit) = Pricing::load_cached(None, None, Some(&snap), &index);
            assert!(!hit, "旧 v6 语义索引必须失效重建（真实磁盘读取路径）");
            assert_eq!(
                p.lookup("model").unwrap().plan.base.cache_read,
                RateSpec::Fixed(0.5),
                "重建后必须从快照恢复数值缓存价"
            );
        }
        Some("readable") => {
            let dir = child_dir();
            let snap = dir.join("pricing-openrouter.json");
            let index = dir.join("pricing-index.json");
            let (p, _, hit) = Pricing::load_cached(None, None, Some(&snap), &index);
            assert!(hit, "重建出的当前版索引在新进程中应命中");
            assert_eq!(
                p.lookup("model").unwrap().plan.base.cache_read,
                RateSpec::Fixed(0.5)
            );
        }
        other => panic!("未知阶段 {other:?}"),
    }
}

/// R06 冷启动回归：有效签名的**当前版本**索引携带非法数值（-5）不得被
/// 静默接受——新进程必须按来源重建并恢复合法价；重建后的干净索引可命中。
#[test]
fn test_invalid_price_cannot_survive_index_load() {
    match std::env::var(STAGE_ENV).ok().as_deref() {
        None => {
            let dir = fresh_dir("r06");
            write(&dir, "pricing.toml", EXTERNAL_TOML);
            let index = dir.join("pricing-index.json");
            let name = "test_invalid_price_cannot_survive_index_load";
            run_stage(name, "seed", &dir);
            assert!(index.exists(), "seed 子进程必须写出索引文件");
            // 父进程篡改：当前版本索引 plan 内 input=-5（有效签名）
            let mut bad: PricingIndex = load_index(&index).unwrap().unwrap();
            assert_eq!(
                bad.v, INDEX_VERSION,
                "fixture 依赖当前索引版本，版本变更时需同步更新本测试"
            );
            for e in &mut bad.entries {
                e.plan.as_mut().unwrap().base.input = RateSpec::Fixed(-5.0);
            }
            save_index(&index, &bad).unwrap();
            run_stage(name, "cold_rebuild", &dir);
            run_stage(name, "readable", &dir);
            let _ = std::fs::remove_dir_all(&dir);
        }
        Some("seed") => {
            let dir = child_dir();
            let toml = dir.join("pricing.toml");
            let index = dir.join("pricing-index.json");
            let (p, warnings, hit) = Pricing::load_cached(Some(&toml), None, None, &index);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(!hit, "首次加载必须从来源重建");
            assert_eq!(p.lookup("m").unwrap().plan.base.input, RateSpec::Fixed(2.0));
        }
        Some("cold_rebuild") => {
            let dir = child_dir();
            let toml = dir.join("pricing.toml");
            let index = dir.join("pricing-index.json");
            let (p, _, hit) = Pricing::load_cached(Some(&toml), None, None, &index);
            assert!(!hit, "含非法数值的当前版索引必须触发重建（真实磁盘路径）");
            assert_eq!(
                p.lookup("m").unwrap().plan.base.input,
                RateSpec::Fixed(2.0),
                "重建后从来源恢复合法价"
            );
        }
        Some("readable") => {
            let dir = child_dir();
            let toml = dir.join("pricing.toml");
            let index = dir.join("pricing-index.json");
            let (p, _, hit) = Pricing::load_cached(Some(&toml), None, None, &index);
            assert!(hit, "重建出的干净索引在新进程中应命中");
            assert_eq!(p.lookup("m").unwrap().plan.base.input, RateSpec::Fixed(2.0));
        }
        other => panic!("未知阶段 {other:?}"),
    }
}

/// 同进程正常重复加载仍命中内存——生产缓存策略不变、不为测试修改。
/// 两次调用在同一测试函数内连续执行，不存在被其他测试插入清缓存的窗口。
#[test]
fn test_same_process_repeated_load_hits_memory() {
    let dir = fresh_dir("mem");
    let snap = write(&dir, "pricing-openrouter.json", OR_SNAPSHOT);
    let index = dir.join("pricing-index.json");
    let (_p1, _, hit1) = Pricing::load_cached(None, None, Some(&snap), &index);
    assert!(!hit1, "同进程首次加载应从来源重建");
    let (p2, _, hit2) = Pricing::load_cached(None, None, Some(&snap), &index);
    assert!(hit2, "同进程同签名重复加载必须命中进程内缓存");
    assert_eq!(
        p2.lookup("model").unwrap().plan.base.cache_read,
        RateSpec::Fixed(0.5)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// F05 冷启动回归：plan=null、**扁平字段**携带非法数值（input=-5）的当前
/// 版索引不得被静默接受——恢复入口必须先把扁平字段归一成完整计划再校验，
/// 任一条目非法 → 整份索引按来源重建；显式 0 的合法扁平条目不受影响。
#[test]
fn test_invalid_flat_rate_in_current_index_forces_rebuild() {
    /// 外置来源：m 正常；m0 带**显式 0**（cache_write 免费，合法）。
    const TWO_MODEL_TOML: &str = "[[model]]\nprefix = \"m\"\ninput = 2.0\noutput = 1.0\n\n\
[[model]]\nprefix = \"m0\"\ninput = 2.0\noutput = 1.0\ncache_write = 0.0\n";

    match std::env::var(STAGE_ENV).ok().as_deref() {
        None => {
            let dir = fresh_dir("f05");
            write(&dir, "pricing.toml", TWO_MODEL_TOML);
            let index = dir.join("pricing-index.json");
            let name = "test_invalid_flat_rate_in_current_index_forces_rebuild";
            // 阶段 1：seed 生成有效签名的当前版索引
            run_stage(name, "seed", &dir);
            // 阶段 2：父进程把 m 的条目改成 plan=null + 扁平 input=-5
            let mut bad: PricingIndex = load_index(&index).unwrap().unwrap();
            let e = bad
                .entries
                .iter_mut()
                .find(|e| e.prefix == "m")
                .expect("seed 索引必含 m 条目");
            e.plan = None;
            e.input = Some(-5.0);
            e.output = Some(1.0);
            e.cache_write = None;
            e.cache_read = None;
            save_index(&index, &bad).unwrap();
            // 阶段 3：新进程冷加载——扁平非法必须触发整份重建
            run_stage(name, "cold_rebuild", &dir);
            // 阶段 4：重建后的干净索引上，把 m0 改成扁平显式 0（合法）
            // → 新进程必须照常命中且保留 Fixed(0)，不得误判非法。
            let mut zero: PricingIndex = load_index(&index).unwrap().unwrap();
            let e = zero
                .entries
                .iter_mut()
                .find(|e| e.prefix == "m0")
                .expect("seed 索引必含 m0 条目");
            e.plan = None;
            e.input = Some(2.0);
            e.output = Some(1.0);
            e.cache_write = Some(0.0);
            e.cache_read = None;
            save_index(&index, &zero).unwrap();
            run_stage(name, "flat_zero_ok", &dir);
            let _ = std::fs::remove_dir_all(&dir);
        }
        Some("seed") => {
            let dir = child_dir();
            let toml = dir.join("pricing.toml");
            let index = dir.join("pricing-index.json");
            let (p, warnings, hit) = Pricing::load_cached(Some(&toml), None, None, &index);
            assert!(warnings.is_empty(), "{warnings:?}");
            assert!(!hit);
            assert_eq!(p.lookup("m").unwrap().plan.base.input, RateSpec::Fixed(2.0));
            assert_eq!(
                p.lookup("m0").unwrap().plan.base.cache_write,
                RateSpec::Fixed(0.0)
            );
        }
        Some("cold_rebuild") => {
            let dir = child_dir();
            let toml = dir.join("pricing.toml");
            let index = dir.join("pricing-index.json");
            let (p, _, hit) = Pricing::load_cached(Some(&toml), None, None, &index);
            assert!(!hit, "扁平字段非法的当前版索引必须触发整份重建");
            assert_eq!(
                p.lookup("m").unwrap().plan.base.input,
                RateSpec::Fixed(2.0),
                "重建后从来源恢复合法价"
            );
            assert_eq!(
                p.lookup("m0").unwrap().plan.base.cache_write,
                RateSpec::Fixed(0.0)
            );
        }
        Some("flat_zero_ok") => {
            let dir = child_dir();
            let toml = dir.join("pricing.toml");
            let index = dir.join("pricing-index.json");
            let (p, _, hit) = Pricing::load_cached(Some(&toml), None, None, &index);
            assert!(hit, "扁平显式 0 是合法条目，不得触发重建");
            assert_eq!(
                p.lookup("m0").unwrap().plan.base.cache_write,
                RateSpec::Fixed(0.0),
                "显式 0（免费）经扁平恢复后仍须保留"
            );
        }
        other => panic!("未知阶段 {other:?}"),
    }
}
