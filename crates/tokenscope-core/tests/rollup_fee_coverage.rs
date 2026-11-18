//! 8.x（2026-11-15，v0.9.2）：日汇总的费用覆盖判定——
//!
//! 旧口径（撤销）：「日汇总全部 token 无条件计入 unknown_tokens、
//! unknown_pricing=true、unknown_reason=rollup_only」。用户明确更正：
//! rollup_only 只表示**数据粒度**，不等同缺价。
//!
//! 新口径：
//! - 来源费用已提供（Some(v)，含 Some(0)）→ 该汇总 token 不进 unknown，
//!   费用走来源通道，不叠加估算（同一份用量只选一次费用依据）；
//! - 来源费用缺失 → 按模型价格估算该汇总：完整估算计入 cost_usd、token
//!   不进 unknown；部分缺价只把**实际未获覆盖的分项**进 unknown；无候选
//!   才是真实未定价（悬浮详情写明缺项，不笼统说「模型没有价格」）；
//! - 不把一天的 token 总量当作单请求上下文（basis 如实标注日 prompt
//!   总量），不编造请求时间/缓存分档/历史价格。
//!
//! 全部 fixture 合成、路径在临时目录，不触碰真实用户数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-rollup-cost-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 外置价格表（pricing.toml，USD/1M）：完整价 = input 1.0 / output 2.0 /
/// cache_write 0 / cache_read 0.5；部分缺价 = partial-model 只声明
/// input/output（cache_read/cache_write 未声明 → 分层回退后 Unknown，
/// 外置来源不套 T3 的线上 assumed_zero 规则——「未声明」就是「未知」）。
const REVIEW_PRICING_TOML: &str = "[[model]]\nprefix = \"full-model\"\ninput = 1.0\noutput = 2.0\ncache_write = 0.0\ncache_read = 0.5\n\n[[model]]\nprefix = \"partial-model\"\ninput = 1.0\noutput = 2.0\n";

fn pricing_of(dir: &Path) -> Pricing {
    let path = dir.join("pricing.toml");
    std::fs::write(&path, REVIEW_PRICING_TOML).unwrap();
    let (pricing, warnings) = Pricing::load(Some(&path), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    pricing
}

fn rollup(model: &str, day: &str, tokens: TokenCounts, fee: Option<f64>) -> RollupContribution {
    RollupContribution {
        day: day.into(),
        source_tz: "UTC".into(),
        app: AgentKind::ClaudeCode,
        model: model.into(),
        requests: 1,
        tokens,
        source_cost_usd: fee,
        source: "synthetic-source".into(),
    }
}

fn input_tok(n: u64) -> TokenCounts {
    TokenCounts {
        input: n,
        ..Default::default()
    }
}

/// T-B1：有来源费用的汇总不再计未定价（旧实现：全部 token 进 unknown、
/// unknown_pricing=true）。
#[test]
fn rollup_with_source_fee_is_not_unpriced() {
    let dir = temp_dir("b1");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // 模型无价格候选（full-model 未用到）→ 旧实现下必为 unknown。
    let r = [rollup(
        "no-price-model",
        "2026-11-13",
        input_tok(2_000_000),
        Some(7.25),
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "来源费用照常累计"
    );
    let unknown_total = g.unknown_tokens.total();
    assert_eq!(
        unknown_total, 0,
        "有来源费用的汇总 token 不得计入未定价（实际 {unknown_total}）"
    );
    assert!(!g.unknown_pricing, "费用已覆盖 → 该组不是未计价组");
    assert_eq!(g.unknown_reason, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B2：显式 0 同样是已提供费用（零费用 ≠ 缺失），token 不进 unknown。
#[test]
fn explicit_zero_fee_covers_rollup_tokens() {
    let dir = temp_dir("b2");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    let r = [rollup(
        "no-price-model",
        "2026-11-13",
        input_tok(500_000),
        Some(0.0),
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.source_cost_usd, Some(0.0), "显式 0 是合法记录值");
    assert_eq!(g.unknown_tokens.total(), 0, "零费用也覆盖 token");
    assert!(!g.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B3：部分提供、部分缺失 → 逐条判定，只把缺失部分计未定价
/// （不能因一个来源金额把整组算已定价，也不能因一条缺失把整组算未定价）。
#[test]
fn partial_fee_coverage_counts_only_missing_tokens() {
    let dir = temp_dir("b3");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    let r = [
        rollup(
            "no-price-model",
            "2026-11-13",
            input_tok(1_000_000),
            Some(7.25),
        ),
        rollup("no-price-model", "2026-11-14", input_tok(800_000), None),
    ];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!((g.source_cost_usd.unwrap() - 7.25).abs() < 1e-9);
    assert_eq!(
        g.unknown_tokens.input, 800_000,
        "只有缺失费用的那条汇总的 token 计未定价"
    );
    assert!(g.unknown_pricing, "组内存在未覆盖 token → 置位");
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B4：缺失费用但有可适用价格 → 按估算计价，token 不进 unknown，
/// basis 如实标注（日 prompt 总量，不冒充单请求）。
#[test]
fn missing_fee_with_applicable_price_is_estimated() {
    let dir = temp_dir("b4");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // full-model：input 1.0 USD/1M → 2M input = $2.0（无 cache 分项参与）。
    let r = [rollup(
        "full-model",
        "2026-11-13",
        input_tok(2_000_000),
        None,
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 2.0).abs() < 1e-9,
        "缺失费用 + 可适用价格 → 按估算计价，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert!(!g.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B5：估算不完整（部分分项缺价）→ 已知分项计金额、实际未获覆盖的
/// 分项 token 进 unknown（有价格候选 ≠ 全部分项已定价）。
#[test]
fn incomplete_estimate_keeps_only_uncovered_tokens_unknown() {
    let dir = temp_dir("b5");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // partial-model：只有 input/output 价；汇总含 1M input（有价）+
    // 300k cache_read（无价）。
    let mut tokens = input_tok(1_000_000);
    tokens.cache_read = 300_000;
    let r = [rollup("partial-model", "2026-11-13", tokens, None)];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 1.0).abs() < 1e-9,
        "已知分项照常计金额，实际 {}",
        g.cost_usd
    );
    assert_eq!(
        g.unknown_tokens.cache_read, 300_000,
        "实际缺价的 cache_read token 计未定价"
    );
    assert_eq!(g.unknown_tokens.input, 0, "已定价分项不计未定价");
    assert!(g.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B6：真正无可用价格 → 保留真实未定价（旧实现的行为在这一支是正确的）。
#[test]
fn no_price_candidate_keeps_real_unpriced() {
    let dir = temp_dir("b6");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    let r = [rollup(
        "no-price-model",
        "2026-11-13",
        input_tok(900_000),
        None,
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.source_cost_usd, None);
    assert_eq!(g.unknown_tokens.total(), 900_000);
    assert!(g.unknown_pricing);
    assert_eq!(g.unknown_reason, Some("rollup_only"));
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B7：不双重计费——有来源费用的汇总即使模型有价也不追加估算
/// （同一份用量只选一次费用依据）。
#[test]
fn source_fee_prevents_double_estimation() {
    let dir = temp_dir("b7");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // full-model 有价（2M input = $2.0），但来源已提供 7.25 → 只用来源值。
    let r = [rollup(
        "full-model",
        "2026-11-13",
        input_tok(2_000_000),
        Some(7.25),
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "来源费用保留"
    );
    assert_eq!(
        g.cost_usd, 0.0,
        "有来源费用时不得叠加请求式估算（防双重计费）"
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert!(!g.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B9：混合明细 + 汇总——明细侧真实缺价照旧计入 unknown，已获费用
/// 覆盖的汇总 token 不叠加；cost_detail 仍是 mixed（数据粒度披露不变）。
#[test]
fn mixed_detail_and_rollup_keep_each_side_orthogonal() {
    let dir = temp_dir("b9");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // 明细：unpriced-model（无候选 → 1M 全部未定价）；汇总：有来源费用。
    let detail = UsageEvent {
        ts: "2026-11-14T00:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "no-price-model".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "r1".into(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 1,
        source_path: "synthetic-only.jsonl".into(),
        input_tokens: 1_000_000,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let r = [rollup(
        "covered-model",
        "2026-11-13",
        input_tok(2_000_000),
        Some(7.25),
    )];
    let result = aggregate_with_rollups(
        std::slice::from_ref(&detail),
        &r,
        GroupBy::Agent,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.unknown_tokens.input, 1_000_000, "明细缺价照旧");
    assert!(g.unknown_pricing);
    assert_eq!(g.cost_detail, Some("mixed"), "数据粒度披露不变");
    assert!(
        (g.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "汇总来源费用不丢失"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B8：请求数与四桶原始 token 不变（费用覆盖判定不影响统计口径）。
#[test]
fn requests_and_tokens_are_unchanged_by_coverage() {
    let dir = temp_dir("b8");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    let mut tokens = input_tok(1_000_000);
    tokens.output = 500_000;
    let r = [rollup("full-model", "2026-11-13", tokens, Some(3.5))];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Day, &tz, &pricing).unwrap();
    let g = result
        .groups
        .iter()
        .find(|g| g.key == "2026-11-13")
        .unwrap();
    assert_eq!(g.requests, 1);
    assert_eq!(g.tokens.input, 1_000_000);
    assert_eq!(g.tokens.output, 500_000);
    assert!((g.source_cost_usd.unwrap() - 3.5).abs() < 1e-9);
    // 合计行同规则。
    assert_eq!(result.totals.unknown_tokens.total(), 0);
    assert!(!result.totals.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// T-B10：跨分组（明细组 + 汇总组）的合计——合计金额 = 明细估算 + 来源
/// 费用（互不重叠贡献合计一次），合计 unknown 只含真实缺价部分。
#[test]
fn cross_group_totals_merge_non_overlapping_costs() {
    let dir = temp_dir("b10");
    let pricing = pricing_of(&dir);
    let tz = jiff::tz::TimeZone::UTC;
    // 明细 full-model 1M input（估算 $1.0）+ 汇总另一模型带来源 7.25。
    let detail = UsageEvent {
        ts: "2026-11-14T00:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: "full-model".into(),
        session_id: "s".into(),
        project: "p".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: "r1".into(),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 1,
        source_path: "synthetic-only.jsonl".into(),
        input_tokens: 1_000_000,
        output_tokens: 0,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    };
    let r = [rollup(
        "fee-model",
        "2026-11-13",
        input_tok(2_000_000),
        Some(7.25),
    )];
    let result = aggregate_with_rollups(
        std::slice::from_ref(&detail),
        &r,
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let t = &result.totals;
    assert!((t.cost_usd - 1.0).abs() < 1e-9, "明细估算入 cost");
    assert!(
        (t.source_cost_usd.unwrap() - 7.25).abs() < 1e-9,
        "来源费用入 source"
    );
    assert_eq!(
        t.unknown_tokens.total(),
        0,
        "明细有价 + 汇总有费用 → 合计无未定价"
    );
    assert!(!t.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

// ── 第三轮反例（2026-11-15 用户复核确认的三个缺陷）──────────────────────

/// 反例 1：零元价格（免费模型 × token）→ 显示 $0.00 而不是未知†。
/// 旧实现按「金额是否非零」判定 total_cost_usd → 估算结果恰为 0 时误显示
/// 未知†（把免费当成缺价）。
#[test]
fn zero_priced_estimate_shows_zero_not_unknown() {
    let dir = temp_dir("z1");
    // 外置价格表：free-model 全部单价显式 0（免费）。
    let path = dir.join("pricing.toml");
    std::fs::write(
        &path,
        "[[model]]\nprefix = \"free-model\"\ninput = 0.0\noutput = 0.0\ncache_write = 0.0\ncache_read = 0.0\n",
    )
    .unwrap();
    let (pricing, warnings) = Pricing::load(Some(&path), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let tz = jiff::tz::TimeZone::UTC;
    // 日汇总缺失费用 → estimate_flat 发生（估算结果 $0.00）。
    let r = [rollup(
        "free-model",
        "2026-11-13",
        input_tok(2_000_000),
        None,
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.cost_detail, Some("rollup_only"), "数据粒度披露不变");
    assert_eq!(g.unknown_tokens.total(), 0, "免费价也覆盖 token");
    assert!(!g.unknown_pricing);
    assert!((g.cost_usd - 0.0).abs() < 1e-12, "估算金额为 0（免费）");
    // 关键断言：total_cost_usd = Some(0.0)（有依据），不是 None（无依据）。
    assert_eq!(
        g.total_cost_usd,
        Some(0.0),
        "零元价格必须显示 $0.00——total_cost_usd 不得为 None"
    );
    assert!(!g.cost_partially_unknown);
    assert!(g.has_estimated_cost, "零元费率覆盖实际用量是费用依据");
    // 合计行同规则。
    assert_eq!(result.totals.total_cost_usd, Some(0.0));
    assert!(result.totals.has_estimated_cost);
    assert!(!result.totals.unknown_pricing);
    std::fs::remove_dir_all(&dir).ok();
}

/// 反例 2：带峰谷时间规则 + 分段价的价格表——日汇总不得按日 prompt 总量
/// 选档，也不得按某个臆造时刻命中时间规则；缺失条件的分项真实未定价。
/// 价格表：base input = 1.0；分段：>1M prompt 时 input = 8.0（高档）；
/// 峰时（UTC 08:00-12:00）input = 99.0。日汇总 2M input：
/// - 旧实现（estimate + UTC 零点）：总量 2M 切到高档段 → $16；
/// - 固定基础价（已撤销的错误假设）：2M × $1.0/M = $2.0；
/// - 正确口径：单请求上下文与时刻都缺失 → input 全部真实未定价，
///   total = None（不从日汇总恢复精确金额）。
#[test]
fn rollup_segment_pricing_not_guessed_from_day_total() {
    let dir = temp_dir("z2");
    let path = dir.join("pricing.toml");
    std::fs::write(
        &path,
        "[[model]]\nprefix = \"peak-model\"\ninput = 1.0\noutput = 0.0\n\
         [[model.segment]]\nmin_tokens = 1000000\ninput = 8.0\noutput = 0.0\n\
         [[model.schedule]]\nlabel = \"peak\"\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"08:00\"\nend_time = \"12:00\"\ninput = 99.0\n",
    )
    .unwrap();
    let (pricing, warnings) = Pricing::load(Some(&path), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let tz = jiff::tz::TimeZone::UTC;
    let r = [rollup(
        "peak-model",
        "2026-11-13",
        input_tok(2_000_000),
        None,
    )];
    let result = aggregate_with_rollups(&[], &r, GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        g.cost_usd.abs() < 1e-9,
        "不能按日总量选档（$16）或默认基础档（$2.0）；input 无可证明费率，实际 {}",
        g.cost_usd
    );
    assert_eq!(
        g.unknown_tokens.input, 2_000_000,
        "单请求上下文缺失 → input 分项真实未定价"
    );
    assert_eq!(g.total_cost_usd, None);
    assert!(g.unknown_pricing);
    // 对照（旧路径的可复现错误值，防回归锚点）：请求级 estimate 按 UTC 零点
    // 计价时，2M 落入 >1M 高档段 → 2M × $8/M = $16。这不是目标行为，只证明
    // 按日总量交给请求级公式确实算错。
    let tokens = input_tok(2_000_000);
    let midnight: jiff::Timestamp = "2026-11-13T00:00:00Z".parse().unwrap();
    let old_midnight = pricing.estimate("peak-model", &tokens, midnight).unwrap();
    assert!(
        (old_midnight.cost - 16.0).abs() < 1e-9,
        "对照：旧路径按日总量切高档段 → $16，实际 {}",
        old_midnight.cost
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 反例 2b：有适用时段但日汇总没有请求时刻——请求级 estimate 在峰时命中
/// $99/M，但日汇总不能挑一个时刻；受影响分项真实未定价，不退回基础价。
#[test]
fn time_limited_rule_does_not_guess_day_time() {
    let dir = temp_dir("z2b");
    let path = dir.join("pricing.toml");
    std::fs::write(
        &path,
        "[[model]]\nprefix = \"peak-model\"\ninput = 1.0\noutput = 0.0\n\
         [[model.schedule]]\nlabel = \"peak\"\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"23:59\"\ninput = 99.0\n",
    )
    .unwrap();
    let (pricing, warnings) = Pricing::load(Some(&path), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let tokens = input_tok(1_000_000);
    // 请求级估算（UTC 08:00 命中全天峰时段）→ $99。
    let at = "2026-11-13T08:00:00Z".parse().unwrap();
    let est_request = pricing.estimate("peak-model", &tokens, at).unwrap();
    assert!(
        (est_request.cost - 99.0).abs() < 1e-9,
        "对照：请求级估算按时间规则命中峰价，实际 {}",
        est_request.cost
    );
    // 日汇总：时刻缺失 → $1 与 $99 都可能，不能猜任何一个。
    let est_rollup = pricing
        .estimate_rollup("peak-model", &tokens, None, None)
        .unwrap();
    assert!(
        est_rollup.cost.abs() < 1e-9,
        "时段依赖缺失 → 不计任意一侧价格，实际 {}",
        est_rollup.cost
    );
    assert_eq!(est_rollup.unknown.input, 1_000_000);
    assert!(!est_rollup.complete);
    assert!(
        est_rollup
            .matched
            .as_ref()
            .unwrap()
            .schedule_label
            .is_none()
    );
    std::fs::remove_dir_all(&dir).ok();
}
