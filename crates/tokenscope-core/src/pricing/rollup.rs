//! 日汇总费率适用性（v0.9.2 第四/五/六/七/九轮复核）：只采用**能够证明适用**
//! 的费率。
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
//!   先按**任意正配比下的费用可行性**筛出**可能胜出规则集合**（见
//!   `scenario_estimate`：对固定非零分项集合的完整候选，只有不存在其它完整
//!   候选价格的非负组合逐项主导它时才可能胜出——等权配比不足以覆盖 skewed
//!   请求），再**逐分项**判断可证明费率：
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
        // 时区偏移切换点本身就是切分边界：只映射 period 端点覆盖不了前拨日
        // —— 02:30 这个本地时刻不存在，compatible 会把它挪到 03:30，真实窗
        // 口 [01:45, 02:00) 就落不进任何切片，整日被误判成恒定费率。
        // 用 15 分钟粒度采样偏移、发现变化后二分到秒（DST 切换只落在整点
        // 或半点，该粒度不会漏）。
        let Some(mut prev_ts) = jiff::Timestamp::from_second(start_ts).ok() else {
            continue;
        };
        let mut prev_off = tz.to_offset(prev_ts);
        let mut probe = start_ts + 900;
        while probe < end_ts {
            let Ok(probe_ts) = jiff::Timestamp::from_second(probe) else {
                break;
            };
            let off = tz.to_offset(probe_ts);
            if off != prev_off {
                let (mut lo, mut hi) = (prev_ts.as_second(), probe);
                while hi - lo > 1 {
                    let mid = lo + (hi - lo) / 2;
                    let Ok(mid_ts) = jiff::Timestamp::from_second(mid) else {
                        break;
                    };
                    if tz.to_offset(mid_ts) == prev_off {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                if hi > start_ts && hi < end_ts {
                    cuts.push(hi);
                }
                prev_off = off;
            }
            prev_ts = probe_ts;
            probe += 900;
        }
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

/// 小型线性规划：在 `{x : A x <= b}`（每行 `[a_0..a_{n-1}, b]`，n ≤ 4）上
/// 最大化 `c·x`。多面体顶点必为 n 个线性独立边界等式之交，故枚举所有 n 元
/// 约束子集、高斯消元求交点、验可行后取目标最大值；不可行返回 None。
fn lp_max(n: usize, constraints: &[Vec<f64>], objective: &[f64], eps: f64) -> Option<f64> {
    fn combinations(m: usize, k: usize) -> Vec<Vec<usize>> {
        fn rec(
            start: usize,
            left: usize,
            m: usize,
            cur: &mut Vec<usize>,
            out: &mut Vec<Vec<usize>>,
        ) {
            if left == 0 {
                out.push(cur.clone());
                return;
            }
            for i in start..=m - left {
                cur.push(i);
                rec(i + 1, left - 1, m, cur, out);
                cur.pop();
            }
        }
        let (mut out, mut cur) = (Vec::new(), Vec::new());
        rec(0, k, m, &mut cur, &mut out);
        out
    }
    fn solve(mat: &[Vec<f64>], rhs: &[f64], n: usize) -> Option<Vec<f64>> {
        let mut a: Vec<Vec<f64>> = mat
            .iter()
            .zip(rhs)
            .map(|(row, &r)| {
                let mut v = row.clone();
                v.push(r);
                v
            })
            .collect();
        for col in 0..n {
            let mut piv = col;
            for r in (col + 1)..n {
                if a[r][col].abs() > a[piv][col].abs() {
                    piv = r;
                }
            }
            if a[piv][col].abs() < 1e-12 {
                return None;
            }
            a.swap(col, piv);
            let d = a[col][col];
            for cell in a[col].iter_mut().skip(col) {
                *cell /= d;
            }
            let pivot_row = a[col].clone();
            for (r, row) in a.iter_mut().enumerate() {
                if r == col {
                    continue;
                }
                let f = row[col];
                if f.abs() < 1e-15 {
                    continue;
                }
                for (cell, &p) in row.iter_mut().skip(col).zip(pivot_row.iter().skip(col)) {
                    *cell -= f * p;
                }
            }
        }
        Some((0..n).map(|i| a[i][n]).collect())
    }
    let m = constraints.len();
    if n == 0 || m < n {
        return None;
    }
    let mut best: Option<f64> = None;
    for comb in combinations(m, n) {
        let mat: Vec<Vec<f64>> = comb.iter().map(|&i| constraints[i][..n].to_vec()).collect();
        let rhs: Vec<f64> = comb.iter().map(|&i| constraints[i][n]).collect();
        let Some(x) = solve(&mat, &rhs, n) else {
            continue;
        };
        if constraints
            .iter()
            .any(|row| (0..n).map(|j| row[j] * x[j]).sum::<f64>() > row[n] + eps)
        {
            continue;
        }
        let val = (0..n).map(|j| objective[j] * x[j]).sum::<f64>();
        best = Some(best.map_or(val, |b| b.max(val)));
    }
    best
}

/// 判定候选 c（分项价格向量 `own`）是否**可能**成为请求级胜出者。
///
/// 请求级费用是配比 v（各使用分项 token 严格为正）的线性函数
/// `cost_x(v) = Σ v_k·p_x(k)`，完整优先后只在完整候选里比费用。按 Motzkin
/// 传递定理，c 不可能胜出 ⟺ 存在其它完整候选的非负组合 z（不全零）使
/// `Σ_o z_o·(p_o(k) - p_c(k)) ≥ 0`（每个分项 k）且至少一处严格 > 0——即 c
/// 的价格被对手价格的凸组合逐分量弱支配。这是可证明的可行性判定，不是
/// 等权采样、经验配比或逐字段取最高价。
fn could_win(own: &[f64], rivals: &[Vec<f64>]) -> bool {
    let n = rivals.len();
    if n == 0 {
        return true;
    }
    let k = own.len();
    let mut constraints: Vec<Vec<f64>> = Vec::new();
    for o in 0..n {
        let mut row = vec![0.0; n + 1];
        row[o] = -1.0; // z_o >= 0
        constraints.push(row);
    }
    let mut row = vec![1.0; n + 1];
    row[n] = 1.0;
    constraints.push(row); // Σz <= 1
    let mut row = vec![-1.0; n + 1];
    row[n] = -1.0;
    constraints.push(row); // Σz >= 1
    for j in 0..k {
        let mut r = vec![0.0; n + 1];
        for o in 0..n {
            r[o] = -(rivals[o][j] - own[j]);
        }
        constraints.push(r); // -f_j(z) <= 0
    }
    let objective: Vec<f64> = (0..n)
        .map(|o| (0..k).map(|j| rivals[o][j] - own[j]).sum::<f64>())
        .collect();
    match lp_max(n, &constraints, &objective, 1e-9) {
        Some(best) => best <= 1e-9, // 无支配组合 → c 可能胜出
        None => true,               // 系统不可行 → c 可能胜出
    }
}

/// 一个场景：固定时段分支 + prompt 规模代表值。
///
/// 日汇总没有单请求配比。请求级费用是单请求 token 的线性函数
/// `Σ v[k]·p[i][k]`（完整优先 → 费用最高 → 稳定决胜）；对固定的「实际使用
/// 分项集合」S，只有对 S 全部有价的候选能胜出（完整优先）。据此分两步：
///
/// 1. 枚举 used 的非空子集作为 S，取 S 上的完整候选；对每条完整候选用
///    `could_win` 判定它在**任意正配比**下是否可能胜出（线性可行性 / 凸包
///    支配，非等权向量）。被支配者排除，其余为「可能胜出规则」。
/// 2. 某分项 k 只有当**所有**可能胜出规则在 k 上都有价且费率一致时才计为
///    已定价；这样既保留单条部分规则的已知分项，也保留交叉优势规则共有的
///    不变分项（两侧 cache_read 都是 $1）；不一致的分项只清自己。禁止逐字段
///    拼价、单价始终来自同一条可能胜出规则。
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
    let prices: Vec<Vec<Option<f64>>> = cands
        .iter()
        .map(|cand| used.iter().map(|&it| price(cand, it)).collect())
        .collect();

    let n = used.len();
    // 每个分项被「可能胜出规则」登记到的费率；全部一致才可证明。
    let mut possible: Vec<Vec<f64>> = vec![Vec::new(); n];
    for bits in 1u32..(1u32 << n) {
        let support: Vec<usize> = (0..n).filter(|&k| bits & (1 << k) != 0).collect();
        // 完整优先：只有对 S 内每个分项都有价的候选可能在请求级胜出。
        let complete: Vec<usize> = (0..cands.len())
            .filter(|&ci| support.iter().all(|&k| prices[ci][k].is_some()))
            .collect();
        if complete.is_empty() {
            // 无完整候选：请求级只能取部分候选的已知费用，费率不可证明；
            // 保持这些分项未知，不整体清零其它子集已证明的分项。
            continue;
        }
        for &ci in &complete {
            let own: Vec<f64> = support.iter().map(|&k| prices[ci][k].unwrap()).collect();
            let rivals: Vec<Vec<f64>> = complete
                .iter()
                .filter(|&&oi| oi != ci)
                .map(|&oi| support.iter().map(|&k| prices[oi][k].unwrap()).collect())
                .collect();
            if could_win(&own, &rivals) {
                for &k in &support {
                    let pos = support.iter().position(|&x| x == k).unwrap();
                    possible[k].push(own[pos]);
                }
            }
        }
    }

    let mut proved: Vec<Option<f64>> = vec![None; n];
    for k in 0..n {
        if let Some(&first) = possible[k].first()
            && possible[k].iter().all(|&v| (v - first).abs() < 1e-12)
        {
            proved[k] = Some(first);
        }
    }

    // 基准承载明细：选对「已证明分项」覆盖最多且价格正确的候选，避免拿一条
    // 恰好缺该分项价格的规则当底稿。
    let mut base = 0usize;
    let mut base_hits = usize::MIN;
    for (ci, cand) in cands.iter().enumerate() {
        let hits = (0..n)
            .filter(|&i| {
                proved[i].is_some_and(|want| {
                    price(cand, used[i]).is_some_and(|v| (v - want).abs() < 1e-12)
                })
            })
            .count();
        if hits > base_hits {
            base_hits = hits;
            base = ci;
        }
    }
    let mut est = cands[base].clone();
    for (i, item) in used.iter().enumerate() {
        if proved[i].is_none() {
            force_unpriced(&mut est, t, *item);
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
