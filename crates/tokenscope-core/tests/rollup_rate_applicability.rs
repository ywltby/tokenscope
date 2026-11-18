//! v0.9.2 第四轮复核（2026-11-16）：日汇总费率适用性 + 费用依据判定。
//!
//! 两个根因：
//! 1. `estimate_flat` 直接清空分段、忽略全部时间规则、固定按基础价计价——
//!    把「不能按日总量选档」错误扩大成「基础价一定适用」。日汇总没有请求
//!    上下文与时刻，只能使用**能够证明适用**的费率：分段依赖缺失的请求
//!    长度 → 受影响分项真实未定价；始终生效（无时段限制）的覆盖规则必须
//!    应用；有适用时段但日汇总缺时刻 → 受影响分项未定价；不受缺失条件
//!    影响的平价分项照常计价。
//! 2. `estimate.is_some()` 被当成「有费用依据」——候选只提供未使用分项的
//!    价格时，全部实际 token 都在 unknown，却得到 Some(0)/$0.00†。依据
//!    必须按**实际用量是否获得覆盖**判定（零元费率覆盖算覆盖，零 token
//!    的未使用分项不算）。
//!
//! 全部 fixture 合成、临时目录，不触碰真实用户数据。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tokenscope::aggregate::{GroupBy, RollupContribution, aggregate_with_rollups};
use tokenscope::model::{AgentKind, TokenCounts, UsageEvent};
use tokenscope::pricing::Pricing;

static SEQ: AtomicU64 = AtomicU64::new(0);

fn temp_dir(tag: &str) -> PathBuf {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-v092-rate-applicability-{tag}-{}-{n}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pricing_with(dir: &Path, toml: &str) -> Pricing {
    let path = dir.join("pricing.toml");
    std::fs::write(&path, toml).unwrap();
    let (pricing, warnings) = Pricing::load(Some(&path), None, None);
    assert!(warnings.is_empty(), "{warnings:?}");
    pricing
}

fn event(model: &str, input: u64, output: u64) -> UsageEvent {
    UsageEvent {
        ts: "2026-11-14T12:00:00Z".parse().unwrap(),
        agent: AgentKind::ClaudeCode,
        model: model.into(),
        session_id: "synthetic-session".into(),
        project: "synthetic-project".into(),
        session_initial_cwd: None,
        event_cwd: None,
        record_id: format!("synthetic-request-{input}-{output}"),
        claude_copy: None,
        codex_response: None,
        codex_legacy_origin: None,
        line: 1,
        source_path: "synthetic-only.jsonl".into(),
        input_tokens: input,
        output_tokens: output,
        cache_write_tokens: 0,
        cache_read_tokens: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        cache_write_split_known: false,
    }
}

fn rollup(model: &str, tokens: TokenCounts, requests: u64) -> RollupContribution {
    RollupContribution {
        day: "2026-11-14".into(),
        source_tz: "UTC".into(),
        app: AgentKind::ClaudeCode,
        model: model.into(),
        requests,
        tokens,
        source_cost_usd: None,
        source: "synthetic-source".into(),
    }
}

fn tok(input: u64, output: u64) -> TokenCounts {
    TokenCounts {
        input,
        output,
        ..Default::default()
    }
}

/// 反例 1（复核材料）：base input $1/M，>200K 上下文 $10/M；两个独立
/// 250K 请求请求路径共 $5.00。日汇总 500K input 没有单请求上下文长度，
/// 不能证明基础档适用——不得算出 $0.50 并标为全部已定价；正确结果是
/// input 真实未定价、total 为 None（不从日汇总恢复精确 $5.00）。
#[test]
fn context_segments_do_not_assume_base_rate() {
    let dir = temp_dir("segments");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"tier-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.segment]]\nmin_tokens = 200000\ninput = 10.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    // 请求路径对照：两个 250K 请求各自命中高档 → $5.00。
    let events = [
        event("tier-model", 250_000, 0),
        UsageEvent {
            record_id: "synthetic-request-second".into(),
            line: 2,
            ..event("tier-model", 250_000, 0)
        },
    ];
    let request_path = aggregate_with_rollups(&events, &[], GroupBy::Model, &tz, &pricing).unwrap();
    assert!(
        (request_path.totals.cost_usd - 5.0).abs() < 1e-9,
        "对照：两个 250K 请求按高档计价 = $5.00，实际 {}",
        request_path.totals.cost_usd
    );
    // 日汇总：500K input，无输出。
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("tier-model", tok(500_000, 0), 2)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        g.cost_usd.abs() < 1e-9,
        "不能把 500K 日总量当单请求、按缺失档位猜测后计出 $0.50；实际 {}",
        g.cost_usd
    );
    assert_eq!(
        g.unknown_tokens.input, 500_000,
        "依赖单请求长度的 input 分项必须真实未定价"
    );
    assert_eq!(
        g.total_cost_usd, None,
        "全部实际用量未获覆盖 → 无单一显示金额（不是 $0.00）"
    );
    assert!(g.unknown_pricing);
    assert_eq!(daily.totals.total_cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// 反例 2（复核材料）：base input $1/M，存在**不带任何时段限制、始终生效**
/// 的 $99/M 覆盖规则。请求路径 $99.00；日汇总必须应用该规则，不能丢弃后
/// 退回已被覆盖的基础价 $1.00。
#[test]
fn unconditional_schedule_override_applies_to_rollup() {
    let dir = temp_dir("always");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"always-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.schedule]]\nlabel = \"always-active\"\ntimezone = \"UTC\"\ninput = 99.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let request_path = aggregate_with_rollups(
        &[event("always-model", 1_000_000, 0)],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    assert!(
        (request_path.totals.cost_usd - 99.0).abs() < 1e-9,
        "对照：请求路径应用始终生效规则 = $99.00，实际 {}",
        request_path.totals.cost_usd
    );
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("always-model", tok(1_000_000, 0), 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 99.0).abs() < 1e-9,
        "日汇总必须应用始终生效的 $99/M 覆盖规则（不是基础价 $1.00），实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert_eq!(g.total_cost_usd, Some(99.0));
    std::fs::remove_dir_all(&dir).ok();
}

/// 有适用时段但日汇总没有请求时刻：受影响分项（input）真实未定价；不受
/// 缺失条件影响的平价分项（output $5/M）照常计价，不一刀切整模型未知。
#[test]
fn time_limited_schedule_keeps_only_affected_items_unknown() {
    let dir = temp_dir("time");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"time-model\"\ninput = 2.0\noutput = 5.0\n\
         [[model.schedule]]\nlabel = \"night\"\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"01:00\"\ninput = 0.1\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("time-model", tok(1_000_000, 1_000_000), 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(
        g.unknown_tokens.input, 1_000_000,
        "input 依赖缺失的请求时刻（$2 与 $0.1 都可能）→ 真实未定价"
    );
    assert_eq!(g.unknown_tokens.output, 0, "output 不受时段规则影响");
    assert!(
        (g.cost_usd - 5.0).abs() < 1e-9,
        "output 平价 $5/M 照常计价 = $5.00，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.total_cost_usd, Some(5.0));
    assert!(g.has_estimated_cost, "部分覆盖也构成估算依据");
    assert!(g.cost_partially_unknown, "已知小计 + 部分缺失必须保留 †");
    std::fs::remove_dir_all(&dir).ok();
}

/// 日汇总总量低于高档阈值时，可以证明没有请求能命中高档段（每请求 prompt
/// 不超过当日总量）——按基础价计价是**可证明**的，不是「默认基础档」。
#[test]
fn sub_threshold_rollup_can_prove_base_rate() {
    let dir = temp_dir("sub-threshold");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"tier-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.segment]]\nmin_tokens = 200000\ninput = 10.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("tier-model", tok(100_000, 0), 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 0.1).abs() < 1e-9,
        "100K < 200K 阈值，基础价 $1/M = $0.10，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert_eq!(g.total_cost_usd, Some(0.1));
    std::fs::remove_dir_all(&dir).ok();
}

/// 第五轮反例 1：周一汇总不得采用仅周日生效的规则。来源日与来源时区
/// 已知，星期限制可以确定性排除。
#[test]
fn weekday_rule_impossible_on_rollup_day_is_excluded() {
    let dir = temp_dir("weekday-impossible");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"weekday-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"12:00\"\n\
         weekdays = [\"sun\"]\ninput = 0.1\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    // 2026-11-16 是周一；仅周日的折扣不可能发生。
    let mut monday_event = event("weekday-model", 1_000_000, 0);
    monday_event.ts = "2026-11-16T12:00:00Z".parse().unwrap();
    let request_path =
        aggregate_with_rollups(&[monday_event], &[], GroupBy::Model, &tz, &pricing).unwrap();
    assert!(
        (request_path.totals.cost_usd - 1.0).abs() < 1e-9,
        "对照：周一请求按基础价 $1.00，实际 {}",
        request_path.totals.cost_usd
    );
    let mut monday_rollup = rollup("weekday-model", tok(1_000_000, 0), 1);
    monday_rollup.day = "2026-11-16".into();
    let daily =
        aggregate_with_rollups(&[], &[monday_rollup], GroupBy::Model, &tz, &pricing).unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 1.0).abs() < 1e-9,
        "周一汇总应按基础价 $1.00，不得把仅周日的规则当可能条件，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert_eq!(g.total_cost_usd, Some(1.0));
    std::fs::remove_dir_all(&dir).ok();
}

/// 第五轮反例 2：两条始终生效的规则在同一时刻同时适用，必须按现有
/// 「完整优先 → 最高费用」择价，而不是当成互斥条件判未知。
#[test]
fn simultaneous_unconditional_rules_select_highest_price() {
    let dir = temp_dir("simultaneous");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"full-day-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\ninput = 3.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\ninput = 2.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let request_path = aggregate_with_rollups(
        &[event("full-day-model", 1_000_000, 0)],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    assert!(
        (request_path.totals.cost_usd - 3.0).abs() < 1e-9,
        "对照：请求路径选最高规则 $3.00，实际 {}",
        request_path.totals.cost_usd
    );
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("full-day-model", tok(1_000_000, 0), 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 3.0).abs() < 1e-9,
        "两条始终生效规则应选最高 $3.00，不得判未知，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert_eq!(g.total_cost_usd, Some(3.0));
    std::fs::remove_dir_all(&dir).ok();
}

/// 第五轮反例 3：输入价随时段变化，但缓存读取价全天恒为 $1/M——必须先
/// 在每个场景内部解析 `same_as_input`，再做跨场景一致性判断，不能先折叠
/// 输入价而丢失分支相关性。
#[test]
fn same_as_input_keeps_provable_cache_read_rate() {
    let dir = temp_dir("same-as-input");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"read-invariant-model\"\ninput = 1.0\noutput = 2.0\n\
         cache_read = \"same_as_input\"\n\
         [[model.schedule]]\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"08:00\"\nend_time = \"20:00\"\n\
         input = 2.0\ncache_read = 1.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let mut read_event = event("read-invariant-model", 0, 0);
    read_event.cache_read_tokens = 1_000_000;
    let request_path =
        aggregate_with_rollups(&[read_event], &[], GroupBy::Model, &tz, &pricing).unwrap();
    assert!(
        (request_path.totals.cost_usd - 1.0).abs() < 1e-9,
        "对照：请求路径缓存读取 $1.00，实际 {}",
        request_path.totals.cost_usd
    );
    let read_tokens = TokenCounts {
        cache_read: 1_000_000,
        ..Default::default()
    };
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("read-invariant-model", read_tokens, 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 1.0).abs() < 1e-9,
        "缓存读取全天 $1/M，日汇总必须计价 $1.00，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    assert_eq!(g.total_cost_usd, Some(1.0));
    std::fs::remove_dir_all(&dir).ok();
}

/// 来源日结束边界（次日零点）不属于该日：周日汇总不得把仅周一生效的
/// 规则算作可能条件。
#[test]
fn weekday_rule_on_next_day_boundary_is_excluded() {
    let dir = temp_dir("weekday-boundary");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"next-day-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"01:00\"\n\
         weekdays = [\"mon\"]\ninput = 0.1\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let mut sunday_rollup = rollup("next-day-model", tok(1_000_000, 0), 1);
    sunday_rollup.day = "2026-11-15".into();
    let daily =
        aggregate_with_rollups(&[], &[sunday_rollup], GroupBy::Model, &tz, &pricing).unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!(
        (g.cost_usd - 1.0).abs() < 1e-9,
        "周日汇总不应采用次日（周一）规则；按基础价 $1.00，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.unknown_tokens.total(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 星期规则在当天**可能**发生但时刻缺失：仍不得猜时段价（星期过滤不是
/// 过度放行）。
#[test]
fn weekday_rule_possible_day_still_needs_request_time() {
    let dir = temp_dir("weekday-possible");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"monday-model\"\ninput = 1.0\noutput = 2.0\n\
         [[model.schedule]]\ntimezone = \"UTC\"\n\
         [[model.schedule.period]]\nstart_time = \"00:00\"\nend_time = \"01:00\"\n\
         weekdays = [\"mon\"]\ninput = 0.1\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let mut monday_rollup = rollup("monday-model", tok(1_000_000, 0), 1);
    monday_rollup.day = "2026-11-16".into();
    let daily =
        aggregate_with_rollups(&[], &[monday_rollup], GroupBy::Model, &tz, &pricing).unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.unknown_tokens.input, 1_000_000);
    assert_eq!(g.total_cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// 普通平价日汇总（无分段、无时间规则）继续正常估算。
#[test]
fn flat_rollup_still_estimates() {
    let dir = temp_dir("flat");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"flat-model\"\ninput = 1.0\noutput = 2.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let daily = aggregate_with_rollups(
        &[],
        &[rollup("flat-model", tok(1_000_000, 500_000), 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = daily.groups.iter().find(|g| g.key != "合计").unwrap();
    assert!((g.cost_usd - 2.0).abs() < 1e-9, "1M input + 0.5M output");
    assert_eq!(g.unknown_tokens.total(), 0);
    assert_eq!(g.total_cost_usd, Some(2.0));
    std::fs::remove_dir_all(&dir).ok();
}

/// 反例 3（复核材料）：候选只提供 output=$2/M，请求只用 1M input、output=0。
/// 全部实际用量都在 unknown_tokens 中 → 完全未定价（total None），不能因
/// 「找到候选」就显示 $0.00†。
#[test]
fn all_used_tokens_unpriced_is_unknown_not_zero() {
    let dir = temp_dir("unused");
    let pricing = pricing_with(&dir, "[[model]]\nprefix = \"output-only\"\noutput = 2.0\n");
    let tz = jiff::tz::TimeZone::UTC;
    let result = aggregate_with_rollups(
        &[event("output-only", 1_000_000, 0)],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.unknown_tokens.input, 1_000_000);
    assert!(g.unknown_pricing);
    assert!(
        g.total_cost_usd.is_none(),
        "全部实际用量缺价 → total 必须为 None（不是 $0.00），实际 {:?}",
        g.total_cost_usd
    );
    assert!(!g.has_estimated_cost, "找到候选不算费用依据");
    assert_eq!(result.totals.total_cost_usd, None);
    std::fs::remove_dir_all(&dir).ok();
}

/// 零 token 的未使用分项不制造已知小计，也不制造缺价：没有任何实际用量
/// 获得覆盖时 total=None，unknown 为空。
#[test]
fn zero_token_event_does_not_create_zero_subtotal() {
    let dir = temp_dir("zero-token");
    let pricing = pricing_with(&dir, "[[model]]\nprefix = \"output-only\"\noutput = 2.0\n");
    let tz = jiff::tz::TimeZone::UTC;
    let result = aggregate_with_rollups(
        &[event("output-only", 0, 0)],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.unknown_tokens.total(), 0, "未使用分项不制造缺价");
    assert!(!g.has_estimated_cost);
    assert_eq!(g.total_cost_usd, None, "未使用分项不制造 $0.00 小计");
    std::fs::remove_dir_all(&dir).ok();
}

/// 明确零元费率覆盖了**实际使用**的 token → Some(0)（已知零元），不是无依据。
#[test]
fn zero_rate_covering_used_tokens_has_basis() {
    let dir = temp_dir("zero");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"free-model\"\ninput = 0.0\noutput = 0.0\n\
         cache_write = 0.0\ncache_read = 0.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let result = aggregate_with_rollups(
        &[event("free-model", 1_000_000, 0)],
        &[],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.total_cost_usd, Some(0.0), "零元费率覆盖已知用量");
    assert!(g.has_estimated_cost, "零元费率覆盖实际用量也是费用依据");
    assert_eq!(g.unknown_tokens.total(), 0);
    std::fs::remove_dir_all(&dir).ok();
}

/// 已知零元分项 + 真实缺价分项混合：保留零元小计与部分缺失标记，
/// 不隐藏缺失、不误判为全部未知。
#[test]
fn zero_priced_part_plus_unknown_part_keeps_subtotal_and_mark() {
    let dir = temp_dir("mix");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"zero-input\"\ninput = 0.0\noutput = 2.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    // input 显式 0（已知零元）、output 平价 2、cache_write 未声明（缺价）。
    let mut ev = event("zero-input", 1_000_000, 1_000_000);
    ev.input_tokens = 0;
    ev.cache_write_tokens = 1_000_000;
    let result = aggregate_with_rollups(&[ev], &[], GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.unknown_tokens.cache_write, 1_000_000);
    assert_eq!(g.unknown_tokens.output, 0);
    assert!(
        (g.cost_usd - 2.0).abs() < 1e-9,
        "已知 output 分项 $2.00 照常累计，实际 {}",
        g.cost_usd
    );
    assert_eq!(g.total_cost_usd, Some(2.0));
    assert!(g.has_estimated_cost);
    assert!(g.cost_partially_unknown);
    std::fs::remove_dir_all(&dir).ok();
}

/// 已知零元分项已覆盖、另有真实缺价分项：total 必须保留 Some(0) 与 †，
/// 不能被误判为完全未知（与「全部实际用量缺价 → None」严格区分）。
#[test]
fn zero_known_subtotal_with_unknown_part_stays_zero_plus_mark() {
    let dir = temp_dir("zero-plus-unknown");
    let pricing = pricing_with(&dir, "[[model]]\nprefix = \"zero-input\"\ninput = 0.0\n");
    let tz = jiff::tz::TimeZone::UTC;
    let mut ev = event("zero-input", 1_000_000, 0);
    ev.cache_write_tokens = 1_000_000;
    let result = aggregate_with_rollups(&[ev], &[], GroupBy::Model, &tz, &pricing).unwrap();
    let g = result.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(g.unknown_tokens.cache_write, 1_000_000);
    assert_eq!(g.unknown_tokens.input, 0);
    assert!(
        g.total_cost_usd == Some(0.0),
        "零元已知分项覆盖实际用量 → Some(0)，实际 {:?}",
        g.total_cost_usd
    );
    assert!(g.has_estimated_cost);
    assert!(g.cost_partially_unknown, "仍有缺价分项 → †");
    std::fs::remove_dir_all(&dir).ok();
}

/// 来源显式 0 与缺失 None 严格区分：有来源 0 时 total=Some(0)、token 不进
/// unknown；来源缺失且模型无价 → total None。
#[test]
fn source_zero_and_missing_stay_distinct() {
    let dir = temp_dir("source");
    let pricing = pricing_with(&dir, "[[model]]\nprefix = \"priced\"\ninput = 1.0\n");
    let tz = jiff::tz::TimeZone::UTC;
    let zero = aggregate_with_rollups(
        &[],
        &[RollupContribution {
            source_cost_usd: Some(0.0),
            ..rollup("no-price-model", tok(100_000, 0), 1)
        }],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let z = zero.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(z.total_cost_usd, Some(0.0));
    assert_eq!(z.unknown_tokens.total(), 0);
    assert_eq!(z.source_cost_state, Some("full"));
    assert!(!z.has_estimated_cost, "来源 0 是来源通道，不是估算依据");
    let missing = aggregate_with_rollups(
        &[],
        &[rollup("no-price-model", tok(100_000, 0), 1)],
        GroupBy::Model,
        &tz,
        &pricing,
    )
    .unwrap();
    let m = missing.groups.iter().find(|g| g.key != "合计").unwrap();
    assert_eq!(m.total_cost_usd, None);
    assert_eq!(m.unknown_tokens.input, 100_000);
    std::fs::remove_dir_all(&dir).ok();
}

/// 依据状态跨分组维度一致：同一批数据按模型/日期/项目分组时，合计金额、
/// 未知 token、部分缺失与总览读数一致。
#[test]
fn total_basis_and_coverage_consistent_across_dimensions() {
    let dir = temp_dir("dims");
    let pricing = pricing_with(
        &dir,
        "[[model]]\nprefix = \"free-model\"\ninput = 0.0\noutput = 0.0\n\
         [[model]]\nprefix = \"paid-model\"\ninput = 1.0\n",
    );
    let tz = jiff::tz::TimeZone::UTC;
    let events = [
        event("free-model", 1_000_000, 0),
        event("paid-model", 1_000_000, 0),
    ];
    let rollups = [rollup("no-price-model", tok(500_000, 0), 1)];
    let mut snapshots = Vec::new();
    for by in [
        GroupBy::Model,
        GroupBy::Day,
        GroupBy::Project,
        GroupBy::Agent,
    ] {
        let r = aggregate_with_rollups(&events, &rollups, by, &tz, &pricing).unwrap();
        snapshots.push((
            r.totals.total_cost_usd,
            r.totals.unknown_tokens.total(),
            r.totals.cost_partially_unknown,
            r.totals.has_estimated_cost,
            r.totals.cost_usd,
        ));
    }
    for s in &snapshots[1..] {
        assert_eq!(s.0, snapshots[0].0, "合计金额跨维度一致");
        assert_eq!(s.1, snapshots[0].1, "未知 token 跨维度一致");
        assert_eq!(s.2, snapshots[0].2, "部分缺失标记跨维度一致");
        assert_eq!(s.3, snapshots[0].3, "估算依据状态跨维度一致");
        assert!((s.4 - snapshots[0].4).abs() < 1e-9, "已知小计跨维度一致");
    }
    std::fs::remove_dir_all(&dir).ok();
}
