//! 日汇总费率适用性（v0.9.2 第四/五/六/七/九/十轮复核）：只采用**能够证明
//! 适用**的费率。
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
//!   把**所有价格候选（Entry）× 该时刻适用规则**放进同一个池，按请求级
//!   语义择价（见 `judge_scenario`）：完整优先 → 费用最高 → 稳定决胜。
//!   请求级本来就是「逐请求选最高后相加」，先按日总量给每个候选算一遍再比
//!   日费用与它不等价——候选价格交叉时（如 A input3/output1 与 B
//!   input1/output3）balanced 与 separated 两种真实构成合计 $4 / $6，
//!   日总量相同却不能给出任一侧费率。
//! - 每个支持集上先取**语义完整**的单元：缺价、线上按 0 估算、写入费按
//!   合并价估算（N18）都不算完整——完整性按该支持集实际使用的分项判断，
//!   不用「单价存在」代替。完整单元存在时只在其中判胜出，没有才回退到
//!   「已知费用最高」的部分单元（缺价分项保持未知）。
//! - 胜出可行性复用同一套 `could_win`（任意正配比线性可行性 / 凸包支配，
//!   非等权向量）；再**逐分项**判断可证明费率：某分项在所有可能胜出单元中
//!   都有价且费率一致才计价，否则只清该分项。于是一条部分定价规则的已知
//!   分项、或交叉优势规则共有的不变分项（如两侧 cache_read 都是 $1）都能
//!   保留，而不因其他分项不确定被整体清零。禁止逐字段取最高价、禁止编造
//!   组合价格、禁止把日汇总当一个请求。
//! - 同一分项在所有场景中单价一致才计价；依赖缺失条件的分项保持真实
//!   未定价。日期/时区不可解析时不作精确切分，退回保守的分支枚举（候选 ×
//!   世界的组合数封顶，超限即全部保持不可证明）。
//!
//! 扁平化只重建「已证明的费率」：不恢复未经证明的完整性，保留 N18 分档
//! 估算标记与线上按 0 估算的披露。

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
    /// 日汇总估算（v0.9.2，第十轮复核修订）：只使用能够证明适用的费率。
    ///
    /// 择价单元是「价格候选 × 该时刻适用规则」，所有候选在**同一个池**里按
    /// 请求级语义竞争（完整优先 → 费用最高 → 稳定决胜）——不先按日总量给每个
    /// 候选算一遍再比日费用，那与「逐请求选最高后相加」不等价。
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
        let used: Vec<Item> = ITEMS.iter().copied().filter(|it| it.used(t) > 0).collect();
        let cache_write = used.iter().position(|it| matches!(it, Item::CacheWrite));
        let mut ledger = Ledger::new(used.len());
        let mut complete_entry = vec![false; cand_count];
        if !used.is_empty() {
            let limit = t.prompt_tokens();
            let mut bases: Vec<u64> = Vec::new();
            for (e, _) in &cands {
                bases.extend(basis_reps(&e.plan, limit));
            }
            bases.sort_unstable();
            bases.dedup();
            let plans: Vec<&PricePlan> = cands.iter().map(|(e, _)| &e.plan).collect();
            if let Some(instants) = day_instants(&plans, day, source_tz) {
                // 精确路径：时间片 × 规模代表 = 一个真实场景。场景内**所有**
                // 候选（Entry）的全部适用规则放在同一个池里竞争——请求级本来就是
                // 「逐请求选最高后相加」，先按日总量给每个候选算一遍再比日费用
                // 与它不等价（跨候选时尤甚）。
                for at in &instants {
                    for &basis in &bases {
                        let mut units = Vec::new();
                        for (ci, (e, _)) in cands.iter().enumerate() {
                            let rules = matching_time_rules(&e.plan, *at, basis);
                            push_units(&mut units, ci, e, &rules, basis);
                        }
                        judge_scenario(
                            &units,
                            basis,
                            t,
                            &used,
                            cache_write,
                            &mut ledger,
                            &mut complete_entry,
                        );
                    }
                }
            } else {
                // 保守路径：日期/时区不可解析时，每个候选的世界无法与其它候选
                // 对齐，只能枚举「每个候选取哪个世界」的组合；组合数超过上限就
                // 不做猜测（全部分项保持不可证明）——既不构造不可能同时成立的
                // 规则组合，也不引入无界枚举。
                let worlds: Vec<Vec<Vec<Rule>>> =
                    cands.iter().map(|(e, _)| time_worlds(&e.plan)).collect();
                let dims: Vec<usize> = worlds.iter().map(Vec::len).collect();
                if dims.iter().product::<usize>() <= MAX_FALLBACK_SCENARIOS {
                    odometer(&dims, |idx| {
                        for &basis in &bases {
                            let mut units = Vec::new();
                            for (ci, (e, _)) in cands.iter().enumerate() {
                                push_units(&mut units, ci, e, &worlds[ci][idx[ci]], basis);
                            }
                            judge_scenario(
                                &units,
                                basis,
                                t,
                                &used,
                                cache_write,
                                &mut ledger,
                                &mut complete_entry,
                            );
                        }
                    });
                }
            }
        }

        let proved = ledger.proved();
        // 基准承载明细：选与「已证明分项」匹配最多且价格正确的候选当底稿，
        // 避免拿一条恰好缺该分项价格的规则生成明细行。
        let base = (0..cands.len())
            .max_by_key(|&ci| {
                let est = estimate_entry(cands[ci].0, 0, t, None);
                used.iter()
                    .enumerate()
                    .filter(|&(k, &item)| match (proved[k], item.line_meta(&est)) {
                        (Some(want), Some((v, _))) => (v - want).abs() < 1e-12,
                        _ => false,
                    })
                    .count()
            })
            .unwrap_or(0);
        let (entry, mode) = &cands[base];
        let mut rates = PriceRates::default();
        for (k, &item) in used.iter().enumerate() {
            // 全部登记都是「线上缺缓存价按 0 估算」时不写死费率，让
            // `estimate_entry` 重新产生 assumed_zero 标记（保留披露）。
            if let Some(v) = proved[k]
                && !ledger.only_assumed_zero[k]
            {
                item.set_rate(&mut rates, RateSpec::Fixed(v));
            }
        }
        let mut flat = (*entry).clone();
        flat.plan = PricePlan {
            basis: entry.plan.basis,
            application: entry.plan.application,
            base: rates,
            segments: Vec::new(),
            schedules: Vec::new(),
        };
        let mut est = estimate_entry(&flat, 0, t, None);
        for (k, &item) in used.iter().enumerate() {
            if proved[k].is_none() {
                force_unpriced(&mut est, t, item);
            }
        }
        // N18：写入费是按合并价估算出来的，金额可保留，但不得恢复成完整定价，
        // 也不得丢掉估算披露——扁平化只重建费率，不重建「精确」这一结论。
        if cache_write.is_some_and(|cw| proved[cw].is_some() && ledger.split_estimated[cw]) {
            est.cache_write_split_estimated = true;
        }
        // T3：金额可确定 ≠ 依据无假设。只要**任一可能胜出路径**按 0 估算过，
        // 金额可以照常保留、token 不记未知，但必须披露「按 0 估算」并保持
        // 不完整。被排除的候选不登记、零用量分项不在 used 里、最终未获覆盖
        // 的分项 proved 为 None——三者都不会产生估算标记。
        let zero_estimated = used.iter().enumerate().any(|(k, &item)| {
            proved[k].is_some()
                && ledger.assumed_zero_used[k]
                && est
                    .lines
                    .iter()
                    .any(|line| item.matches_kind(line.kind) && line.priced)
        });
        if zero_estimated {
            est.cache_price_zero_estimated = true;
        }
        est.complete = est.unknown.is_zero()
            && !est.cache_price_zero_estimated
            && !est.cache_write_split_estimated;
        let complete_count = complete_entry.iter().filter(|&&c| c).count();
        let reason = if complete_count > 0 {
            "highest_complete_cost"
        } else {
            "highest_partial_cost"
        };
        est.matched = Some(MatchedCandidate {
            raw_key: entry.display.clone(),
            channel: entry.display.split_once('/').map(|(c, _)| c.to_string()),
            source: tier_name(entry.tier).to_string(),
            matched_key: entry.prefix.clone(),
            match_mode: *mode,
            candidate_count: cand_count,
            reason: reason.to_string(),
            schedule_label: None,
            schedule_timezone: None,
            // 依据标注：日汇总估算（无请求时刻；只采用可证明适用的费率）。
            request_at: None,
            complete_candidate_count: complete_count,
            incomplete_candidate_count: cand_count - complete_count,
            incomplete_candidates_excluded: 0,
        });
        est.excluded_incomplete_schedules = 0;
        est.excluded_candidate_warning = None;
        Some(est)
    }
}

/// 兜底路径允许枚举的「候选 × 世界」组合上限。超过即不做猜测：全部分项保持
/// 不可证明——宁可未知，也不构造不可能同时成立的规则组合或无界枚举。
const MAX_FALLBACK_SCENARIOS: usize = 64;

/// `could_win` 顶点枚举的变量上限（rival 去重、剔除被支配者之后）。
/// 顶点枚举是指数级的，变量数必须封顶；超过上限时保持「可能胜出」，
/// 只损失排除精度，绝不因此误报某条规则不可能胜出。
const MAX_LP_RIVALS: usize = 8;

/// 场景内参与择价的一个计价单元：某个价格候选（`Entry`）在该场景下的
/// 一条适用规则；`rule = None` 表示该候选此时只能走基础价。
struct Unit<'a> {
    candidate: usize,
    entry: &'a Entry,
    rule: Option<Rule<'a>>,
}

/// 把某候选在该场景适用的规则展开成计价单元；一条都不适用时退回基础价。
fn push_units<'a>(
    units: &mut Vec<Unit<'a>>,
    candidate: usize,
    entry: &'a Entry,
    rules: &[Rule<'a>],
    basis: u64,
) {
    let mut any = false;
    for &(sched, period) in rules {
        if !rule_applies(sched, period, basis) {
            continue;
        }
        units.push(Unit {
            candidate,
            entry,
            rule: Some((sched, period)),
        });
        any = true;
    }
    if !any {
        units.push(Unit {
            candidate,
            entry,
            rule: None,
        });
    }
}

/// 一个单元对「本次汇总实际使用的分项」的价格与语义状态。
///
/// `priced` 不是「有没有单价」——分档缺档、分档被阻断、行未计价都算没价；
/// `assumed` 记录线上来源按 0 估算（金额算 0，但不是完整官方价）；
/// `split` 记录写入费是按合并价估算（N18：金额成立，但不是精确分档价）。
struct UnitProbe {
    prices: Vec<Option<f64>>,
    priced: Vec<bool>,
    assumed: Vec<bool>,
    split: bool,
}

impl UnitProbe {
    fn new(e: &Entry, basis: u64, t: &TokenCounts, used: &[Item], rule: Option<Rule<'_>>) -> Self {
        let est = estimate_entry(e, basis, t, rule);
        let mut prices = Vec::with_capacity(used.len());
        let mut priced = Vec::with_capacity(used.len());
        let mut assumed = Vec::with_capacity(used.len());
        for &item in used {
            let mut first = None;
            let mut all_priced = false;
            let mut assumed_any = false;
            for line in est.lines.iter().filter(|l| item.matches_kind(l.kind)) {
                all_priced = line.priced;
                if line.priced && first.is_none() {
                    first = line.unit_price;
                }
                if line.rate_kind == RateKind::AssumedZero {
                    assumed_any = true;
                }
            }
            // 同一分项可能拆成两行（N18 的 5m/1h）：任何一行没价都算整体没价，
            // 不能拿另一行的单价冒充。
            let complete = all_priced && first.is_some();
            priced.push(complete);
            prices.push(if complete { first } else { None });
            assumed.push(assumed_any);
        }
        Self {
            prices,
            priced,
            assumed,
            split: est.cache_write_split_estimated,
        }
    }

    /// 该单元能否把「恰好使用 `support` 这些分项」的请求完整定价。
    fn complete_for(&self, support: &[usize], cache_write: Option<usize>) -> bool {
        support.iter().all(|&k| self.priced[k] && !self.assumed[k])
            && !(self.split && cache_write.is_some_and(|cw| support.contains(&cw)))
    }
}

/// 跨场景累计的「可能被请求级选中」登记：分项费率只有在全部登记一致时
/// 才可证明；`None` 表示存在可能被选中的单元不给该分项定价。
///
/// 「金额是否确定」与「依据是否含假设」是两个独立判断：
/// - `possible` 一致 → 费率可证明（金额确定）；
/// - `only_assumed_zero`：所有登记都是按 0 估算（用于决定是否需要让
///   `estimate_entry` 重新产生 assumed_zero 行）；
/// - `assumed_zero_used`：**任一**可能胜出路径按 0 估算过——金额可以一致，
///   但依据含假设，必须披露且不得声称完整。
struct Ledger {
    possible: Vec<Vec<Option<f64>>>,
    only_assumed_zero: Vec<bool>,
    assumed_zero_used: Vec<bool>,
    split_estimated: Vec<bool>,
}

impl Ledger {
    fn new(n: usize) -> Self {
        Self {
            possible: vec![Vec::new(); n],
            only_assumed_zero: vec![true; n],
            assumed_zero_used: vec![false; n],
            split_estimated: vec![false; n],
        }
    }

    fn record(&mut self, k: usize, value: Option<f64>, assumed_zero: bool, split: bool) {
        self.possible[k].push(value);
        if value.is_some() {
            if assumed_zero {
                self.assumed_zero_used[k] = true;
            } else {
                self.only_assumed_zero[k] = false;
            }
            if split {
                self.split_estimated[k] = true;
            }
        }
    }

    fn proved(&self) -> Vec<Option<f64>> {
        self.possible
            .iter()
            .map(|seen| {
                let mut first: Option<f64> = None;
                for value in seen {
                    let Some(v) = value else {
                        return None;
                    };
                    match first {
                        None => first = Some(*v),
                        Some(f) if (f - v).abs() < 1e-12 => {}
                        _ => return None,
                    }
                }
                first
            })
            .collect()
    }
}

/// 多维里程表：按 `dims` 给出的各维长度枚举全部组合（空维视为单点）。
fn odometer(dims: &[usize], mut visit: impl FnMut(&[usize])) {
    if dims.is_empty() || dims.contains(&0) {
        return;
    }
    let mut idx = vec![0usize; dims.len()];
    'outer: loop {
        visit(&idx);
        for i in (0..dims.len()).rev() {
            idx[i] += 1;
            if idx[i] < dims[i] {
                continue 'outer;
            }
            idx[i] = 0;
        }
        break;
    }
}

/// 单个场景的逐分项可证明性判定。
///
/// 与请求级共用同一套语义，只是把所有候选（Entry）的适用规则放进同一个池：
/// 1. 对每个非空支持集，先取**语义完整**的单元（缺价、按 0 估算、写入费
///    按合并价估算都不算完整——完整性按该支持集实际使用的分项判断）；
/// 2. 完整单元存在时只在其中判胜出（完整优先）；没有完整单元才回退到
///    「已知费用最高」的部分单元，并让缺价分项保持不可证明；
/// 3. 胜出判定复用 `could_win`（任意正配比线性可行性），不按日总量配比猜。
fn judge_scenario(
    units: &[Unit<'_>],
    basis: u64,
    t: &TokenCounts,
    used: &[Item],
    cache_write: Option<usize>,
    ledger: &mut Ledger,
    complete_entry: &mut [bool],
) {
    if units.is_empty() || used.is_empty() {
        return;
    }
    let probes: Vec<UnitProbe> = units
        .iter()
        .map(|u| UnitProbe::new(u.entry, basis, t, used, u.rule))
        .collect();
    let n = used.len();
    let full = (1u32 << n) - 1;
    for bits in 1u32..=full {
        let support: Vec<usize> = (0..n).filter(|&k| bits & (1 << k) != 0).collect();
        let complete: Vec<usize> = (0..probes.len())
            .filter(|&i| probes[i].complete_for(&support, cache_write))
            .collect();
        if !complete.is_empty() {
            if bits == full {
                for &i in &complete {
                    complete_entry[units[i].candidate] = true;
                }
            }
            for &ui in &complete {
                let own: Vec<f64> = support
                    .iter()
                    .map(|&k| probes[ui].prices[k].unwrap_or_default())
                    .collect();
                let rivals: Vec<Vec<f64>> = complete
                    .iter()
                    .filter(|&&oi| oi != ui)
                    .map(|&oi| {
                        support
                            .iter()
                            .map(|&k| probes[oi].prices[k].unwrap_or_default())
                            .collect()
                    })
                    .collect();
                if could_win(&own, &rivals) {
                    for (pos, &k) in support.iter().enumerate() {
                        let split = cache_write == Some(k) && probes[ui].split;
                        ledger.record(k, own.get(pos).copied(), probes[ui].assumed[k], split);
                    }
                }
            }
            continue;
        }
        // 无完整单元：回退到「已知费用最高」。缺价分项按 0 参与已知费用比较，
        // 但一旦该单元胜出，它缺价的分项就整体落空——因此任一可能胜出者缺价，
        // 该分项就不可证明。
        let known: Vec<Vec<f64>> = probes
            .iter()
            .map(|p| {
                support
                    .iter()
                    .map(|&k| p.prices[k].unwrap_or(0.0))
                    .collect()
            })
            .collect();
        let winners: Vec<usize> = (0..known.len())
            .filter(|&i| {
                let rivals: Vec<Vec<f64>> = known
                    .iter()
                    .enumerate()
                    .filter(|&(oi, _)| oi != i)
                    .map(|(_, v)| v.clone())
                    .collect();
                could_win(&known[i], &rivals)
            })
            .collect();
        if winners.is_empty() {
            continue;
        }
        for &k in &support {
            if winners.iter().any(|&i| probes[i].prices[k].is_none()) {
                ledger.record(k, None, false, false);
            } else {
                for &i in &winners {
                    let split = cache_write == Some(k) && probes[i].split;
                    ledger.record(k, probes[i].prices[k], probes[i].assumed[k], split);
                }
            }
        }
    }
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
    plans: &[&PricePlan],
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
    for plan in plans {
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
                            if matches!(ambiguous.offset(), jiff::tz::AmbiguousOffset::Fold { .. })
                            {
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
    // 日汇总把「候选 × 规则」都放进同一个池后，rival 数可能变大，而顶点枚举
    // 是指数级的：先去掉与自身相同的向量（它们永远不产生严格支配）、去掉
    // 被单条对手逐项支配的向量（这些对手永远不会改变结论），再对变量数封顶。
    // 封顶时返回「可能胜出」——只损失排除精度，绝不误判某条规则不可能胜出。
    let mut uniq: Vec<&Vec<f64>> = Vec::with_capacity(rivals.len());
    for rival in rivals {
        if rival.len() != own.len() {
            continue;
        }
        if rival.iter().zip(own).all(|(a, b)| a == b) {
            continue;
        }
        if !uniq
            .iter()
            .any(|u| u.iter().zip(rival).all(|(a, b)| a == b))
        {
            uniq.push(rival);
        }
    }
    if uniq
        .iter()
        .any(|r| r.iter().zip(own).all(|(a, b)| a >= b) && r.iter().zip(own).any(|(a, b)| a > b))
    {
        return false;
    }
    let n = uniq.len();
    if n == 0 {
        return true;
    }
    if n > MAX_LP_RIVALS {
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
            r[o] = -(uniq[o][j] - own[j]);
        }
        constraints.push(r); // -f_j(z) <= 0
    }
    let objective: Vec<f64> = (0..n)
        .map(|o| (0..k).map(|j| uniq[o][j] - own[j]).sum::<f64>())
        .collect();
    match lp_max(n, &constraints, &objective, 1e-9) {
        Some(best) => best <= 1e-9, // 无支配组合 → c 可能胜出
        None => true,               // 系统不可行 → c 可能胜出
    }
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
