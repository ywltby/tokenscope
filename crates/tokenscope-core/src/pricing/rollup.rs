//! 日汇总费率适用性（v0.9.2 第四/五轮复核）：只采用**能够证明适用**的费率。
//!
//! 与请求级 `estimate` 使用同一套 `estimate_entry` 语义（分段、时段、规则
//! 择价、`same_as_input` 解析），差异只在「条件是否可能发生」：
//! - 日汇总有来源统计日与来源时区 → 排除该日不可能出现的星期规则；
//! - 没有请求时刻/单请求上下文 → 枚举该日可能的时段分支与 prompt 规模
//!   区间，每个场景内部先按现有规则择价（完整优先 → 最高费用 → 稳定
//!   顺序），再比较各场景的分项单价；
//! - 同一分项在所有场景中单价一致才计价；依赖缺失条件的分项保持真实
//!   未定价。竞争规则绝不被逐字段拼成不存在的组合。
//!
//! 候选选择仍遵循请求级原则（完整优先 → 最高费用 → tie-break）。

use super::*;

#[derive(Clone, Copy)]
enum Item {
    Input,
    Output,
    CacheWrite,
    CacheRead,
}

const ITEMS: [Item; 4] = [Item::Input, Item::Output, Item::CacheWrite, Item::CacheRead];

impl Item {
    fn used(self, t: &TokenCounts) -> u64 {
        match self {
            Item::Input => t.input,
            Item::Output => t.output,
            Item::CacheWrite => t.cache_write,
            Item::CacheRead => t.cache_read,
        }
    }

    fn unpriced(self, u: &TokenCounts) -> u64 {
        match self {
            Item::Input => u.input,
            Item::Output => u.output,
            Item::CacheWrite => u.cache_write,
            Item::CacheRead => u.cache_read,
        }
    }

    fn set_unpriced(self, u: &mut TokenCounts, n: u64) {
        match self {
            Item::Input => u.input = n,
            Item::Output => u.output = n,
            Item::CacheWrite => u.cache_write = n,
            Item::CacheRead => u.cache_read = n,
        }
    }

    fn set_rate(self, r: &mut PriceRates, spec: RateSpec) {
        match self {
            Item::Input => r.input = spec,
            Item::Output => r.output = spec,
            Item::CacheWrite => r.cache_write = spec,
            Item::CacheRead => r.cache_read = spec,
        }
    }

    fn matches_kind(self, kind: CostLineKind) -> bool {
        match self {
            Item::Input => kind == CostLineKind::Input,
            Item::Output => kind == CostLineKind::Output,
            Item::CacheWrite => matches!(
                kind,
                CostLineKind::CacheWrite | CostLineKind::CacheWrite5m | CostLineKind::CacheWrite1h
            ),
            Item::CacheRead => kind == CostLineKind::CacheRead,
        }
    }

    /// 某场景胜出估算中该分项的**已解析**单价与来源类型（`same_as_input`
    /// 已由 `estimate_entry` 按该场景自己的输入价解析，不会跨场景丢失相关）。
    fn line_meta(self, est: &CostEstimate) -> Option<(f64, RateKind)> {
        let line = est.lines.iter().find(|l| self.matches_kind(l.kind))?;
        if !line.priced {
            return None;
        }
        line.unit_price.map(|v| (v, line.rate_kind))
    }
}

type Rule<'a> = (&'a PriceSchedule, Option<&'a SchedulePeriod>);

impl Pricing {
    /// 日汇总估算（v0.9.2，第五轮复核修订）：只使用能够证明适用的费率。
    ///
    /// `day` + `source_tz` 是来源统计日与来源时区：用于排除该日不可能出现
    /// 的星期规则（例如周一汇总不采用仅周日生效的折扣）。缺失时不能排除，
    /// 保持保守。
    pub fn estimate_rollup(
        &self,
        model: &str,
        t: &TokenCounts,
        day: Option<jiff::civil::Date>,
        source_tz: Option<&jiff::tz::TimeZone>,
    ) -> Option<CostEstimate> {
        let cands = self.collect_candidates(model);
        if cands.is_empty() {
            return None;
        }
        let cand_count = cands.len();
        type Cand<'a> = (CostEstimate, MatchMode, &'a Entry);
        let mut complete_set: Vec<Cand> = Vec::new();
        let mut partial_set: Vec<Cand> = Vec::new();
        for (e, mode) in cands {
            let est = candidate_rollup_estimate(e, t, day, source_tz);
            if est.complete {
                complete_set.push((est, mode, e));
            } else {
                partial_set.push((est, mode, e));
            }
        }
        let (pool, reason) = if !complete_set.is_empty() {
            (&complete_set, "highest_complete_cost")
        } else {
            (&partial_set, "highest_partial_cost")
        };
        let mut best: Option<&Cand> = None;
        for cand in pool.iter() {
            best = match best {
                Some((b, bm, be)) => {
                    if cand.0.cost > b.cost
                        || (cand.0.cost == b.cost && tie_rank(cand.2, cand.1) > tie_rank(be, *bm))
                    {
                        Some(cand)
                    } else {
                        best
                    }
                }
                None => Some(cand),
            };
        }
        let (mut est, mode, e) = best.expect("候选非空时池必非空").clone();
        est.matched = Some(MatchedCandidate {
            raw_key: e.display.clone(),
            channel: e.display.split_once('/').map(|(c, _)| c.to_string()),
            source: tier_name(e.tier).to_string(),
            matched_key: e.prefix.clone(),
            match_mode: mode,
            candidate_count: cand_count,
            reason: reason.to_string(),
            schedule_label: None,
            schedule_timezone: None,
            // 依据标注：日汇总估算（无请求时刻；只采用可证明适用的费率）。
            request_at: None,
            complete_candidate_count: complete_set.len(),
            incomplete_candidate_count: partial_set.len(),
            incomplete_candidates_excluded: 0,
        });
        est.excluded_incomplete_schedules = 0;
        est.excluded_candidate_warning = None;
        Some(est)
    }
}

/// 单个候选在日汇总条件下的可证明估算：枚举可能的时段分支 × prompt 规模
/// 区间，每个场景先按请求规则择价，再逐分项比较结果。
fn candidate_rollup_estimate(
    e: &Entry,
    t: &TokenCounts,
    day: Option<jiff::civil::Date>,
    source_tz: Option<&jiff::tz::TimeZone>,
) -> CostEstimate {
    let limit = t.prompt_tokens();
    let worlds = time_worlds(&e.plan, day, source_tz);
    let bases = basis_reps(&e.plan, limit);
    let mut scenarios: Vec<CostEstimate> = Vec::new();
    for rules in &worlds {
        for &basis in &bases {
            scenarios.push(scenario_estimate(e, rules, basis, t));
        }
    }
    if scenarios.is_empty() {
        scenarios.push(estimate_entry(e, 0, t, None));
    }

    let mut rates = PriceRates::default();
    let mut ambiguous = [false; 4];
    for (index, item) in ITEMS.iter().enumerate() {
        if item.used(t) == 0 {
            // 未使用分项不参与依据判定，也不制造已知小计/缺价。
            continue;
        }
        let metas: Vec<Option<(f64, RateKind)>> =
            scenarios.iter().map(|s| item.line_meta(s)).collect();
        let Some(first) = metas[0] else {
            if metas.iter().all(Option::is_none) {
                // 所有场景都真缺价：保持 Unknown（线上来源的 assumed_zero
                // 规则仍按既有产品口径处理）。
                continue;
            }
            ambiguous[index] = true;
            continue;
        };
        let same = metas
            .iter()
            .all(|m| m.is_some_and(|(v, _)| (v - first.0).abs() < 1e-12));
        if !same {
            ambiguous[index] = true;
            continue;
        }
        // 全部场景都按「线上缺缓存价 → 0」估算时，保留 Unknown 让
        // estimate_entry 重新产生 assumed_zero 标记（不丢失披露）。
        let all_assumed_zero = metas
            .iter()
            .all(|m| m.is_some_and(|(_, kind)| kind == RateKind::AssumedZero));
        if !all_assumed_zero {
            item.set_rate(&mut rates, RateSpec::Fixed(first.0));
        }
    }

    let mut flat = e.clone();
    flat.plan = PricePlan {
        basis: e.plan.basis,
        application: e.plan.application,
        base: rates,
        segments: Vec::new(),
        schedules: Vec::new(),
    };
    let mut est = estimate_entry(&flat, 0, t, None);
    for (index, item) in ITEMS.iter().enumerate() {
        if ambiguous[index] {
            force_unpriced(&mut est, t, *item);
        }
    }
    est.complete = est.unknown.total() == 0 && !est.cache_price_zero_estimated;
    est
}

/// 该日所有可能的时段分支。基准分支 = 每条规则的规则级价格（`None`）；
/// 每个**该日可能发生**的 period 生成一个分支：目标规则换成 period 分支，
/// 其他规则若无可证明的重叠时段则回到规则级。
fn time_worlds<'a>(
    plan: &'a PricePlan,
    day: Option<jiff::civil::Date>,
    source_tz: Option<&jiff::tz::TimeZone>,
) -> Vec<Vec<Rule<'a>>> {
    let mut base: Vec<Rule<'a>> = Vec::new();
    for sched in &plan.schedules {
        // 时区非法的规则在 matching_time_rules 中整体跳过。
        if schedule_tz(sched).is_none() {
            continue;
        }
        base.push((sched, None));
    }
    let mut worlds = vec![base];
    for sched in &plan.schedules {
        let Some(tz) = schedule_tz(sched) else {
            continue;
        };
        if sched.periods.is_empty() {
            continue;
        }
        for period in &sched.periods {
            if !period_feasible(period, day, source_tz, &tz) {
                continue;
            }
            let mut rules: Vec<Rule<'a>> = Vec::new();
            for other in &plan.schedules {
                let Some(other_tz) = schedule_tz(other) else {
                    continue;
                };
                if std::ptr::eq(other, sched) {
                    rules.push((other, Some(period)));
                    continue;
                }
                if other.periods.is_empty() {
                    rules.push((other, None));
                    continue;
                }
                let overlap = other.periods.iter().find(|p| {
                    period_feasible(p, day, source_tz, &other_tz)
                        && periods_overlap(
                            period,
                            sched.timezone.as_deref(),
                            p,
                            other.timezone.as_deref(),
                        )
                });
                match overlap {
                    Some(p) => rules.push((other, Some(p))),
                    None => rules.push((other, None)),
                }
            }
            worlds.push(rules);
        }
    }
    worlds
}

/// prompt 规模区间代表值：所有分段边界（计划级与规则级）切分 `[0, limit]`，
/// 每个区间取起点。区间内分段选择不变，因此一个代表值即可。
fn basis_reps(plan: &PricePlan, limit: u64) -> Vec<u64> {
    let mut bounds = vec![0u64];
    for seg in plan
        .segments
        .iter()
        .chain(plan.schedules.iter().flat_map(|s| s.segments.iter()))
    {
        if seg.min_tokens <= limit {
            bounds.push(seg.min_tokens);
        }
        if let Some(max) = seg.max_tokens
            && max <= limit
        {
            bounds.push(max);
        }
    }
    bounds.sort_unstable();
    bounds.dedup();
    bounds
}

/// 一个场景：固定时段分支 + prompt 规模代表值，按请求级规则择价
/// （完整优先 → 最高费用 → 声明顺序稳定）。
fn scenario_estimate(e: &Entry, rules: &[Rule<'_>], basis: u64, t: &TokenCounts) -> CostEstimate {
    let mut best: Option<CostEstimate> = None;
    for (sched, period) in rules {
        if !rule_applies(sched, *period, basis) {
            continue;
        }
        let est = estimate_entry(e, basis, t, Some((*sched, *period)));
        best = Some(match best {
            None => est,
            Some(b) => {
                if (est.complete && !b.complete)
                    || (est.complete == b.complete && est.cost > b.cost)
                {
                    est
                } else {
                    b
                }
            }
        });
    }
    best.unwrap_or_else(|| estimate_entry(e, basis, t, None))
}

/// 与 `matching_time_rules` 的非时间判定一致：period 命中即适用；规则级
/// 分支需要规则价已声明，或其分段在当前 prompt 规模下声明了价格。
fn rule_applies(sched: &PriceSchedule, period: Option<&SchedulePeriod>, basis: u64) -> bool {
    if period.is_some() {
        return true;
    }
    rates_declared(&sched.prices)
        || select_segment_in(&sched.segments, basis).is_some_and(|s| rates_declared(&s.prices))
}

fn schedule_tz(sched: &PriceSchedule) -> Option<jiff::tz::TimeZone> {
    match sched.timezone.as_deref() {
        Some(name) => jiff::tz::TimeZone::get(name).ok(),
        None => Some(jiff::tz::TimeZone::UTC),
    }
}

/// 该 period 是否可能在来源统计日出现：先看时间窗合法，再按规则时区检查
/// 来源日在规则时区覆盖的星期是否与 `weekdays` 相交。
fn period_feasible(
    period: &SchedulePeriod,
    day: Option<jiff::civil::Date>,
    source_tz: Option<&jiff::tz::TimeZone>,
    sched_tz: &jiff::tz::TimeZone,
) -> bool {
    let (Some(start), Some(end)) = (parse_hhmm(&period.start_time), parse_hhmm(&period.end_time))
    else {
        return false;
    };
    if start >= end {
        return false;
    }
    let Some(days) = &period.weekdays else {
        return true;
    };
    if days.is_empty() {
        return false;
    }
    let (Some(day), Some(source_tz)) = (day, source_tz) else {
        // 日期/来源时区不可解析时不能排除，保持保守。
        return true;
    };
    let Ok(start_source) = day.at(0, 0, 0, 0).to_zoned(source_tz.clone()) else {
        return true;
    };
    let Ok(tomorrow) = day.tomorrow() else {
        return true;
    };
    let Ok(end_source) = tomorrow.at(0, 0, 0, 0).to_zoned(source_tz.clone()) else {
        return true;
    };
    // 星期限制按**规则时区**判断：来源日的起止时刻换算到规则时区后，
    // 检查与该区间真正有交集的日期（不含恰好等于结束边界的次日零点）。
    let start_ts = start_source.timestamp();
    let end_ts = end_source.timestamp();
    let mut date = start_ts.to_zoned(sched_tz.clone()).date();
    let last = end_ts.to_zoned(sched_tz.clone()).date();
    loop {
        let Ok(day_start) = date.at(0, 0, 0, 0).to_zoned(sched_tz.clone()) else {
            return true;
        };
        let Ok(next) = date.tomorrow() else {
            return true;
        };
        let Ok(day_end) = next.at(0, 0, 0, 0).to_zoned(sched_tz.clone()) else {
            return true;
        };
        if day_start.timestamp() < end_ts
            && day_end.timestamp() > start_ts
            && days
                .iter()
                .any(|d| d.as_str() == weekday_tag(date.weekday()))
        {
            return true;
        }
        if date == last {
            break;
        }
        date = next;
    }
    false
}

fn periods_overlap(
    a: &SchedulePeriod,
    a_tz: Option<&str>,
    b: &SchedulePeriod,
    b_tz: Option<&str>,
) -> bool {
    let (Some(a_start), Some(a_end), Some(b_start), Some(b_end)) = (
        parse_hhmm(&a.start_time),
        parse_hhmm(&a.end_time),
        parse_hhmm(&b.start_time),
        parse_hhmm(&b.end_time),
    ) else {
        return false;
    };
    if a_start >= a_end || b_start >= b_end {
        return false;
    }
    if !weekday_sets_intersect(a.weekdays.as_deref(), b.weekdays.as_deref()) {
        return false;
    }
    // 规则时区一致（含 None = UTC）时才比较本地时间窗；时区不同则保守认为
    // 可能重叠（换算后无法从日粒度确定）。
    if a_tz.unwrap_or("UTC") != b_tz.unwrap_or("UTC") {
        return true;
    }
    a_start < b_end && b_start < a_end
}

fn weekday_sets_intersect(a: Option<&[String]>, b: Option<&[String]>) -> bool {
    match (a, b) {
        (None, _) | (_, None) => true,
        (Some(a), Some(b)) => a.iter().any(|d| b.iter().any(|e| e == d)),
    }
}

/// 把 ambiguous 分项强制回真实未定价：移除该分项已计小计（含线上来源
/// assumed_zero 产生的 0 价覆盖），token 计入 unknown，complete=false。
fn force_unpriced(est: &mut CostEstimate, used: &TokenCounts, item: Item) {
    let tokens = item.used(used);
    if tokens == 0 {
        return;
    }
    let mut removed = 0.0;
    for line in &mut est.lines {
        if item.matches_kind(line.kind) {
            removed += line.subtotal;
            line.subtotal = 0.0;
            line.priced = false;
            line.unit_price = None;
            line.rate_kind = RateKind::Unknown;
            line.overflow = false;
        }
    }
    est.cost = (est.cost - removed).max(0.0);
    if item.unpriced(&est.unknown) < tokens {
        item.set_unpriced(&mut est.unknown, tokens);
    }
    if matches!(item, Item::CacheWrite) {
        est.cache_write_split_estimated = false;
    }
    est.cache_price_zero_estimated = est
        .lines
        .iter()
        .any(|line| line.rate_kind == RateKind::AssumedZero);
    est.complete = false;
}
