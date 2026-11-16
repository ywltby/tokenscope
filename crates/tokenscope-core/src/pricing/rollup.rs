//! 日汇总费率适用性（v0.9.2 第四/五/六/七轮复核）：只采用**能够证明适用**的费率。
//!
//! 与请求级 `estimate` 使用同一套 `estimate_entry` 语义（分段、时段、规则
//! 择价、`same_as_input` 解析），差异只在「条件是否可能发生」：
//! - 日汇总有来源统计日与来源时区 → 把来源日按**每条时段规则在每个规则
//!   时区下的真实绝对时间边界**切分为若干区间，每个区间取代表时刻，直接
//!   调用请求级 `matching_time_rules` 得到该时刻**真正同时适用**的规则集。
//!   这样既不把「部分重叠」当成整段同时生效，也不会拼出不存在的规则组合；
//!   星期、半开边界、跨时区与 DST（含回拨重复小时的两个映射）都由同一套
//!   请求级逻辑统一处理，不假定自然日恒为 24 小时。
//! - 没有单请求 token 配比 → prompt 规模只按分段边界枚举代表值；每个场景
//!   先用支配关系筛出**可能胜出规则集合**（见 `scenario_estimate`：覆盖更少
//!   桶或逐项价更低的规则永不被请求级选中），再**逐分项**判断可证明费率：
//!   某分项在所有可能胜出规则中都有价且费率一致才计价，否则只清该分项。
//!   于是一条部分定价规则的已知分项、或交叉优势规则共有的不变分项（如
//!   两侧 cache_read 都是 $1）都能保留，而不因其他分项不确定被整体清零。
//!   禁止逐字段取最高价、禁止编造组合价格、禁止把日汇总当一个请求。
//! - 同一分项在所有场景中单价一致才计价；依赖缺失条件的分项保持真实
//!   未定价。日期/时区不可解析时不作精确切分，退回保守的分支枚举。
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

/// 单个候选在日汇总条件下的可证明估算：枚举该日真实时间片 × prompt 规模
/// 区间，每个场景先按支配判据择价，再逐分项比较结果。
fn candidate_rollup_estimate(
    e: &Entry,
    t: &TokenCounts,
    day: Option<jiff::civil::Date>,
    source_tz: Option<&jiff::tz::TimeZone>,
) -> CostEstimate {
    let limit = t.prompt_tokens();
    let bases = basis_reps(&e.plan, limit);
    let mut scenarios: Vec<CostEstimate> = Vec::new();
    if let Some(instants) = day_instants(&e.plan, day, source_tz) {
        // 精确路径：每个时间片代表时刻用请求级 matching_time_rules 求真实
        // 同时适用的规则集，语义与请求级 estimate 完全一致。
        for at in &instants {
            for &basis in &bases {
                let rules = matching_time_rules(&e.plan, *at, basis);
                scenarios.push(scenario_estimate(e, &rules, basis, t));
            }
        }
    } else {
        // 保守路径：日期/时区不可解析，无法做真实时间切分，退回枚举分支。
        let worlds = time_worlds(&e.plan);
        for rules in &worlds {
            for &basis in &bases {
                scenarios.push(scenario_estimate(e, rules, basis, t));
            }
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

/// 来源日按真实时间边界切分后的代表时刻列表。
///
/// 收集来源日绝对区间 `[day_start, day_end)` 内**所有 period 的实际切换
/// 边界**（每条规则按其时区把 weekday 命中的本地时间窗换算成绝对时间），
/// 连同来源日起止点排序去重；相邻边界构成半开区间，各取一个内部代表时刻。
/// 每个代表时刻交给请求级 `matching_time_rules` 求真实同时适用规则集——
/// 因此"部分重叠"会被切成独立区间、"不可能同时出现"的规则组合不会被构造。
///
/// 星期、半开边界、跨时区与 DST 都由 jiff 的日期/时区换算统一处理，不假定
/// 自然日恒为 24 小时。来源日/时区不可解析时返回 `None` 走保守分支。
fn day_instants(
    plan: &PricePlan,
    day: Option<jiff::civil::Date>,
    source_tz: Option<&jiff::tz::TimeZone>,
) -> Option<Vec<jiff::Timestamp>> {
    let day = day?;
    let source_tz = source_tz?;
    let day_start = day.at(0, 0, 0, 0).to_zoned(source_tz.clone()).ok()?;
    let tomorrow = day.tomorrow().ok()?;
    let day_end = tomorrow.at(0, 0, 0, 0).to_zoned(source_tz.clone()).ok()?;
    let start_ts = day_start.timestamp().as_second();
    let end_ts = day_end.timestamp().as_second();
    if start_ts >= end_ts {
        return None;
    }

    let mut cuts: Vec<i64> = vec![start_ts, end_ts];
    for sched in &plan.schedules {
        let Some(tz) = schedule_tz(sched) else {
            continue;
        };
        for period in &sched.periods {
            let (Some(start_sec), Some(end_sec)) =
                (parse_hhmm(&period.start_time), parse_hhmm(&period.end_time))
            else {
                continue;
            };
            if start_sec >= end_sec {
                continue;
            }
            // 来源日在规则时区下可能横跨两个本地日（跨时区），也可能因 DST
            // 不足/超过 24 小时。逐本地日检查 weekday 并换算时间窗边界。
            let first_local = day_start.timestamp().to_zoned(tz.clone()).date();
            let mut local = first_local;
            loop {
                let weekday_ok = period
                    .weekdays
                    .as_ref()
                    .is_none_or(|days| days.iter().any(|d| d == weekday_tag(local.weekday())));
                if weekday_ok {
                    for sec in [start_sec, end_sec] {
                        let h = (sec / 3600) as i8;
                        let m = ((sec % 3600) / 60) as i8;
                        let ambiguous = tz.to_ambiguous_zoned(local.at(h, m, 0, 0));
                        // 回拨 fold：同一本地端点对应两个真实绝对时刻，必须
                        // 都作为切分边界，否则第二次出现的价格窗口被漏掉。
                        // Gap（前拨，本地时刻不存在）与无歧义时刻只有一个
                        // compatible 映射（请求级 to_zoned 也是这个），只取一次。
                        if matches!(ambiguous.offset(), jiff::tz::AmbiguousOffset::Fold { .. }) {
                            for zoned in [ambiguous.clone().earlier(), ambiguous.later()]
                                .into_iter()
                                .flatten()
                            {
                                let ts = zoned.timestamp().as_second();
                                if ts > start_ts && ts < end_ts {
                                    cuts.push(ts);
                                }
                            }
                        } else if let Ok(zoned) = ambiguous.compatible() {
                            let ts = zoned.timestamp().as_second();
                            if ts > start_ts && ts < end_ts {
                                cuts.push(ts);
                            }
                        }
                    }
                }
                let next = local.tomorrow().ok()?;
                // 下一个本地日的零点若已不早于来源日结束，则覆盖完整，停止。
                let Ok(next_zero) = next.at(0, 0, 0, 0).to_zoned(tz.clone()) else {
                    break;
                };
                if next_zero.timestamp().as_second() >= end_ts {
                    break;
                }
                local = next;
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let mut instants = Vec::with_capacity(cuts.len().saturating_sub(1));
    for pair in cuts.windows(2) {
        // 半开区间 [pair[0], pair[1])：取内部中点，不落在边界上。
        let mid = pair[0] + (pair[1] - pair[0]) / 2;
        if let Ok(ts) = jiff::Timestamp::from_second(mid) {
            instants.push(ts);
        }
    }
    if instants.is_empty() {
        instants.push(day_start.timestamp());
    }
    Some(instants)
}

/// 日期/时区不可解析时的保守分支枚举：每条规则的规则级价格 + 每个 period
/// 单独成分支，不做日期可行性过滤也不推断重叠——宁可多列条件让跨场景
/// 一致性判定保持未定价，也不误报已定价。
fn time_worlds<'a>(plan: &'a PricePlan) -> Vec<Vec<Rule<'a>>> {
    let mut base: Vec<Rule<'a>> = Vec::new();
    for sched in &plan.schedules {
        if schedule_tz(sched).is_none() {
            continue;
        }
        base.push((sched, None));
    }
    let mut worlds = vec![base];
    for sched in &plan.schedules {
        if schedule_tz(sched).is_none() || sched.periods.is_empty() {
            continue;
        }
        for period in &sched.periods {
            let mut rules: Vec<Rule<'a>> = Vec::new();
            for other in &plan.schedules {
                if schedule_tz(other).is_none() {
                    continue;
                }
                if std::ptr::eq(other, sched) {
                    rules.push((other, Some(period)));
                } else {
                    rules.push((other, None));
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

/// 一个场景：固定时段分支 + prompt 规模代表值。
///
/// 日汇总没有单请求配比，整桶 cost 最高的规则只是在"日总量配比"下胜出，
/// 不能代表每个请求都选它（反例2：$4 与 $6 两种合法构成）。请求级 cost 是
/// 单请求 token 的线性函数 `Σ v[k]·p[i][k]`（完整优先）。据此分两步：
///
/// 1. **筛出可能胜出规则**：规则 a 支配 b，当 a 定价的使用桶集合 ⊇ b、
///    且对 b 每个已定价桶 a 的价 ≥ b，且至少一处更优（含多覆盖一个桶）。
///    被支配的 b 对任何配比 v 都不会在请求级胜出 → 从集合中剔除。保留的是
///    Pareto 前沿（互不支配），即“在不同配比下可能胜出”的规则集合。
/// 2. **逐分项判定**：对某分项 k，只有当**所有**可能胜出规则在 k 上都有价且
///    费率一致时，k 才计为已定价。这样既保留单条部分定价规则的已知分项、
///    也保留交叉优势规则共有的不变分项（两侧 cache_read 都是 $1）；不一致
///    的分项只清自己，不牵连其它分项。禁止逐字段拼价、单价始终来自同一条
///    可能胜出规则。
fn scenario_estimate(e: &Entry, rules: &[Rule<'_>], basis: u64, t: &TokenCounts) -> CostEstimate {
    let used: Vec<Item> = ITEMS.iter().copied().filter(|it| it.used(t) > 0).collect();
    let mut cands: Vec<CostEstimate> = Vec::new();
    for (sched, period) in rules {
        if !rule_applies(sched, *period, basis) {
            continue;
        }
        cands.push(estimate_entry(e, basis, t, Some((*sched, *period))));
    }
    if cands.is_empty() {
        return estimate_entry(e, basis, t, None);
    }
    if used.is_empty() {
        return cands[0].clone();
    }

    let price = |est: &CostEstimate, item: Item| item.line_meta(est).map(|(v, _)| v);
    // a 支配 b：a 覆盖所有 b 已定价的使用桶，逐个价 ≥ b，且至少有一处更优
    //（价严格更高，或多覆盖一个使用桶 → 完整优先意义上更优）。
    let dominates = |a: &CostEstimate, b: &CostEstimate| -> bool {
        let mut strictly_better = false;
        for it in &used {
            let pa = price(a, *it);
            let pb = price(b, *it);
            match (pa, pb) {
                (Some(x), Some(y)) => {
                    if x + 1e-12 < y {
                        return false;
                    }
                    if x > y + 1e-12 {
                        strictly_better = true;
                    }
                }
                (None, Some(_)) => return false, // a 缺 b 有的桶 → 不支配
                (Some(_), None) => strictly_better = true, // a 多覆盖 → 完整优先更优
                (None, None) => {}
            }
        }
        strictly_better
    };
    // Pareto 前沿 = 不被任何其他候选支配的规则（可能胜出者）。
    let frontier: Vec<usize> = (0..cands.len())
        .filter(|&i| !(0..cands.len()).any(|j| i != j && dominates(&cands[j], &cands[i])))
        .collect();
    let possible: Vec<&CostEstimate> = frontier.iter().map(|&i| &cands[i]).collect();

    // 以集合内第一条规则为基准承载明细；逐分项判定是否可证明。
    let mut est = possible[0].clone();
    for it in &used {
        let Some(first) = price(possible[0], *it) else {
            force_unpriced(&mut est, t, *it);
            continue;
        };
        let invariant = possible
            .iter()
            .all(|cand| price(cand, *it).is_some_and(|v| (v - first).abs() < 1e-12));
        if !invariant {
            force_unpriced(&mut est, t, *it);
        }
    }
    est
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
