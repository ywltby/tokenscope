//! SF07（安全与数据一致性审查 Task 7）：价格列表与对照价保留三态及规则结构。
//!
//! 不变量（docs/stats-semantics.md §4 展示口径）：
//! 1. PricingEntry / OpenRouterPrice 的四类单价以 RateSpec 三态透出——
//!    `数字 | "same_as_input" | null`；缺失 ≠ 免费（不得 unwrap_or(0)），
//!    SameAsInput 不被压平成 Unknown；
//! 2. `base_incomplete`（原 incomplete）只描述**基础**费率可解析性：
//!    任一分项 Unknown，或 cache_read SameAsInput 但基础输入价 Unknown
//!    （依赖未定）；"任意请求是否完整"仍以实际 breakdown 为准；
//! 3. 分段 / 峰谷 schedule / period 的嵌套价格同样保留三态——缺覆盖值
//!    表示继承上层（Unknown），不标成免费；
//! 4. estimate 的 breakdown 中 cache_read SameAsInput 引用同一解析层
//!    的最终输入价（rate_kind = same_as_input），原估算结果不变；
//!    输入价未知时 cache_read 保持未知。
//!
//! 全部使用合成外置/快照 fixture 与临时目录，不触碰真实数据目录。

use std::path::PathBuf;

use tokenscope::model::TokenCounts;
use tokenscope::pricing::{Pricing, RateKind, RateSpec};

fn fresh_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "tokenscope-price-view-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// 基础价三态视图：Fixed / Fixed(0) / SameAsInput / Unknown 全部保留，
/// 序列化为 `数字 | "same_as_input" | null`；base_incomplete 语义正确。
#[test]
fn pricing_view_preserves_unknown_zero_and_same_as_input() {
    let dir = fresh_dir("three-state");
    let toml = dir.join("pricing.toml");
    std::fs::write(
        &toml,
        // full：四态齐全（cache_read 显式沿用输入价）；partial：缺三个分项。
        "[[model]]\nprefix = \"full\"\ninput = 2.0\noutput = 1.0\ncache_write = 0.0\ncache_read = \"same_as_input\"\n\n\
         [[model]]\nprefix = \"partial\"\ninput = 2.0\n",
    )
    .unwrap();
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let entries = p.entries();

    let full = entries.iter().find(|e| e.prefix == "full").unwrap();
    assert_eq!(full.input, RateSpec::Fixed(2.0));
    assert_eq!(full.output, RateSpec::Fixed(1.0));
    assert_eq!(full.cache_write, RateSpec::Fixed(0.0), "显式 0 = 免费");
    assert_eq!(
        full.cache_read,
        RateSpec::SameAsInput,
        "SameAsInput 不得压平成 Unknown/0"
    );
    assert!(
        !full.base_incomplete,
        "四态齐全（SameAsInput 有可解析输入价）→ 基础完整"
    );

    let partial = entries.iter().find(|e| e.prefix == "partial").unwrap();
    assert_eq!(partial.input, RateSpec::Fixed(2.0));
    assert_eq!(partial.output, RateSpec::Unknown);
    assert_eq!(partial.cache_write, RateSpec::Unknown);
    assert_eq!(partial.cache_read, RateSpec::Unknown);
    assert!(partial.base_incomplete);

    // serde 线格式：数字 | "same_as_input" | null。
    let json = serde_json::to_string(&full).unwrap();
    assert!(json.contains("\"cache_read\":\"same_as_input\""), "{json}");
    assert!(json.contains("\"cache_write\":0.0"), "{json}");
    let pjson = serde_json::to_string(&partial).unwrap();
    assert!(pjson.contains("\"output\":null"), "{pjson}");
    // 更名后的字段名随 DTO 透出。
    assert!(pjson.contains("base_incomplete"), "{pjson}");
    assert!(!pjson.contains("\"incomplete\""), "{pjson}");

    // SameAsInput 依赖未定：input Unknown + cache_read SameAsInput。
    let toml2 = dir.join("policy.toml");
    std::fs::write(
        &toml2,
        "[[model]]\nprefix = \"dep\"\ninput = 2.0\n\n\
         [[model_policy]]\nprefix = \"dep2\"\ncache_read = \"same_as_input\"\n\n\
         [[model]]\nprefix = \"dep2\"\n",
    )
    .unwrap();
    let (p2, w2) = Pricing::load(Some(&toml2), None, None);
    assert!(w2.is_empty(), "{w2:?}");
    let entries2 = p2.entries();
    let dep2 = entries2
        .iter()
        .find(|e| e.prefix == "dep2")
        .expect("dep2 条目应存在");
    assert_eq!(dep2.cache_read, RateSpec::SameAsInput);
    assert_eq!(dep2.input, RateSpec::Unknown);
    assert!(
        dep2.base_incomplete,
        "SameAsInput 的基础输入价未知 → 依赖未定，必须提示"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// OpenRouter 对照价缺失分项 = Unknown（null），不得变成 0 冒充免费；
/// 对照价缺失（None）与分项缺失是两个状态。
#[test]
fn comparison_price_does_not_turn_missing_into_free() {
    let dir = fresh_dir("or-compare");
    // models.dev 主源条目：cache 分项未知；OpenRouter 同前缀只有基础两价。
    let md = dir.join("pricing-modelsdev.json");
    std::fs::write(
        &md,
        r#"{"v":3,"synced_at":"t","entries":[
            {"id":"zenmux/mix","name":null,"input":1.0,"output":2.0}
        ]}"#,
    )
    .unwrap();
    let or = dir.join("pricing-openrouter.json");
    std::fs::write(
        &or,
        r#"{"v":2,"synced_at":"t","entries":[
            {"id":"zenmux/mix","name":null,"prompt":0.000003,"completion":0.000006}
        ]}"#,
    )
    .unwrap();
    let (p, warnings) = Pricing::load(None, Some(&md), Some(&or));
    assert!(warnings.is_empty(), "{warnings:?}");
    let entries = p.entries();
    let main = entries.iter().find(|e| e.source == "models.dev").unwrap();
    let cmp = main.openrouter.as_ref().expect("同前缀对照价应存在");
    assert_eq!(cmp.input, RateSpec::Fixed(3.0), "3e-6 USD/token → 3 USD/M");
    assert_eq!(cmp.output, RateSpec::Fixed(6.0));
    assert_eq!(
        cmp.cache_read,
        RateSpec::Unknown,
        "对照价缺失分项必须保持未知"
    );
    assert_eq!(
        cmp.cache_write,
        RateSpec::Unknown,
        "缺失不得压成 0 冒充免费"
    );
    // 序列化线格式为 null，不是 0。
    let json = serde_json::to_string(&main).unwrap();
    assert!(json.contains("\"cache_read\":null"), "{json}");
    assert!(!json.contains("\"cache_read\":0"), "{json}");
    // 无同前缀对照：openrouter = None（另一个状态）。
    let md2 = dir.join("pricing-modelsdev2.json");
    std::fs::write(
        &md2,
        r#"{"v":3,"synced_at":"t","entries":[
            {"id":"solo/model","name":null,"input":1.0,"output":2.0}
        ]}"#,
    )
    .unwrap();
    let (p2, _) = Pricing::load(None, Some(&md2), Some(&or));
    let solo_entries = p2.entries();
    let solo = solo_entries
        .iter()
        .find(|e| e.prefix == "solo/model")
        .unwrap();
    assert!(solo.openrouter.is_none(), "无对应模型 = None");
    let _ = std::fs::remove_dir_all(&dir);
}

/// breakdown 中 cache_read 的 SameAsInput 引用同一解析层的最终输入价：
/// 分段命中 → 单价 = 分段输入价、rate_kind = same_as_input；输入价未知 →
/// cache_read 保持未知（不猜 0）。原估算结果不变。
#[test]
fn same_as_input_tracks_resolved_tier_input_in_breakdown() {
    let dir = fresh_dir("sai-breakdown");
    let toml = dir.join("pricing.toml");
    std::fs::write(
        &toml,
        "[[model]]\nprefix = \"seg\"\ninput = 1.0\noutput = 1.0\ncache_read = \"same_as_input\"\n\n\
         [[model.segment]]\nlabel = \"大请求\"\nmin_tokens = 1000000\ninput = 3.0\noutput = 3.0\n",
    )
    .unwrap();
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let at = "2026-01-05T10:00:00Z".parse().unwrap();

    // 小请求：基础价 input=1 → cache_read 单价 = 1（SameAsInput）。
    let small = p
        .estimate(
            "seg",
            &TokenCounts {
                input: 500_000,
                output: 0,
                cache_write: 0,
                cache_read: 250_000,
            },
            at,
        )
        .unwrap();
    let cr = small
        .lines
        .iter()
        .find(|l| l.kind == tokenscope::pricing::CostLineKind::CacheRead)
        .unwrap();
    assert_eq!(cr.unit_price, Some(1.0), "基础层输入价");
    assert!(
        matches!(cr.rate_kind, RateKind::SameAsInput),
        "rate_kind 必须标注 same_as_input: {:?}",
        cr.rate_kind
    );
    assert!((small.cost - (0.5 + 0.25)).abs() < 1e-12, "原估算结果不变");

    // 大请求：命中分段 input=3 → cache_read 单价 = 3（随分段解析）。
    let big = p
        .estimate(
            "seg",
            &TokenCounts {
                input: 2_000_000,
                output: 0,
                cache_write: 0,
                cache_read: 1_000_000,
            },
            at,
        )
        .unwrap();
    let cr_big = big
        .lines
        .iter()
        .find(|l| l.kind == tokenscope::pricing::CostLineKind::CacheRead)
        .unwrap();
    assert_eq!(cr_big.unit_price, Some(3.0), "随分段解析的输入价");
    assert!(matches!(cr_big.rate_kind, RateKind::SameAsInput));
    assert!((big.cost - (6.0 + 3.0)).abs() < 1e-12, "原估算结果不变");

    // 输入价未知 + SameAsInput → cache_read 未知（complete=false）。
    let toml2 = dir.join("policy.toml");
    std::fs::write(
        &toml2,
        "[[model]]\nprefix = \"dep2\"\n\n[[model_policy]]\nprefix = \"dep2\"\ncache_read = \"same_as_input\"\n",
    )
    .unwrap();
    let (p2, w2) = Pricing::load(Some(&toml2), None, None);
    assert!(w2.is_empty(), "{w2:?}");
    let est = p2
        .estimate(
            "dep2",
            &TokenCounts {
                input: 100,
                output: 0,
                cache_write: 0,
                cache_read: 50,
            },
            at,
        )
        .unwrap();
    assert!(!est.complete, "输入价未知 → cache_read 依赖未定");
    assert_eq!(est.unknown.cache_read, 50, "cache_read token 保持未计价");
    assert_eq!(est.cost, 0.0, "没有任何可计价分项");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 嵌套视图：schedule / period / 分段的价格同样保留三态——period 缺
/// cache_read 覆盖 → 继承上层语义为 Unknown（null），不得标成免费。
#[test]
fn nested_schedule_views_keep_rate_specs() {
    let dir = fresh_dir("nested");
    let toml = dir.join("pricing.toml");
    std::fs::write(
        &toml,
        "[[model]]\nprefix = \"sched\"\ninput = 1.0\noutput = 1.0\n\n\
         [[model.schedule]]\nlabel = \"峰谷\"\ntimezone = \"UTC\"\ninput = 0.5\noutput = 0.5\n\n\
         [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"08:00\"\ninput = 0.2\noutput = 0.2\n",
    )
    .unwrap();
    let (p, warnings) = Pricing::load(Some(&toml), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let entries = p.entries();
    let e = entries.iter().find(|e| e.prefix == "sched").unwrap();
    assert!(e.has_tiered_pricing);
    assert_eq!(e.schedules.len(), 1);
    let sched = &e.schedules[0];
    // 规则级：input/output 显式，cache 两项 Unknown（null，继承 = 无声明）。
    let sched_json = serde_json::to_string(sched).unwrap();
    assert!(sched_json.contains("\"input\":0.5"), "{sched_json}");
    assert!(sched_json.contains("\"cache_read\":null"), "{sched_json}");
    assert!(
        !sched_json.contains("\"cache_read\":0"),
        "缺覆盖值不得标成免费: {sched_json}"
    );
    // period 级：仅声明 input/output，cache 分项保持 null。
    assert_eq!(sched.periods.len(), 1);
    let period_json = serde_json::to_string(&sched.periods[0]).unwrap();
    assert!(period_json.contains("\"input\":0.2"), "{period_json}");
    assert!(
        period_json.contains("\"cache_write\":null"),
        "{period_json}"
    );
    // 分段视图同理：外部 fixture 的分段价格（无 cache 声明）→ null。
    let toml2 = dir.join("segmented.toml");
    std::fs::write(
        &toml2,
        "[[model]]\nprefix = \"segv\"\ninput = 1.0\noutput = 1.0\n\n\
         [[model.segment]]\nmin_tokens = 1000000\ninput = 3.0\noutput = 3.0\n",
    )
    .unwrap();
    let (p2, w2) = Pricing::load(Some(&toml2), None, None);
    assert!(w2.is_empty(), "{w2:?}");
    let entries2 = p2.entries();
    let e2 = entries2.iter().find(|e| e.prefix == "segv").unwrap();
    let seg_json = serde_json::to_string(&e2.segments[0]).unwrap();
    assert!(seg_json.contains("\"input\":3.0"), "{seg_json}");
    assert!(seg_json.contains("\"cache_read\":null"), "{seg_json}");
    let _ = std::fs::remove_dir_all(&dir);
}
