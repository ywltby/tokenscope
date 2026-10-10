//! 聚合层：把事件按日（Asia/Shanghai 自然日）/ 模型 / 项目分组并计价。
//! 层间契约：只依赖 model 与 pricing，不触碰任何 source 内部类型。

use std::collections::BTreeMap;

use jiff::tz::TimeZone;
use serde::Serialize;

use crate::model::{TokenCounts, UsageEvent};
use crate::model_identity::ModelIdentity;
use crate::pricing::Pricing;

/// MP04：聚合中间态——分组累计 + 该组原始模型名的稳定代表写法。
#[derive(Default)]
struct GroupAccum {
    group: Group,
    representative: Option<String>,
}

/// MP04：Model 维度的展示名——优先可信 models.dev 模型名（按完整等价 ID
/// 命中且与条目顺序无关），其次该组原始模型名的稳定代表，绝不展示压缩后的
/// 身份键（`claudeopus55` 这类写法只用于筛选与下钻匹配）。
fn model_label(key: &str, representative: Option<&str>, pricing: &Pricing) -> String {
    let rep = representative.unwrap_or(key);
    pricing
        .display_name_for(rep)
        .or_else(|| pricing.display_name_for(key))
        .unwrap_or_else(|| rep.to_string())
}

/// 时区解析（M6）：显式传入 > 本机系统（`--tz local`）> 默认 Asia/Shanghai。
/// 存储层永远持有 UTC（见 cache.rs），全链路只在此解析一次。
pub fn resolve_tz(explicit: Option<&str>) -> anyhow::Result<(TimeZone, String)> {
    let label = |tz: &TimeZone, fallback: &str| -> String {
        tz.iana_name().unwrap_or(fallback).to_string()
    };
    match explicit {
        Some(name) if name.eq_ignore_ascii_case("local") => {
            let tz = TimeZone::system();
            Ok((tz.clone(), label(&tz, "local")))
        }
        Some(name) => {
            let tz = TimeZone::get(name).map_err(|e| anyhow::anyhow!("未知时区 {name:?}: {e}"))?;
            Ok((tz, name.to_string()))
        }
        None => {
            let tz = TimeZone::get("Asia/Shanghai").expect("仓库默认时区常量且合法");
            Ok((tz, "Asia/Shanghai".to_string()))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GroupBy {
    #[default]
    Day,
    Model,
    Project,
    Agent,
}

impl GroupBy {
    pub fn as_str(self) -> &'static str {
        match self {
            GroupBy::Day => "day",
            GroupBy::Model => "model",
            GroupBy::Project => "project",
            GroupBy::Agent => "agent",
        }
    }

    /// 表格首列标题。
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::Day => "日期",
            GroupBy::Model => "模型",
            GroupBy::Project => "项目",
            GroupBy::Agent => "应用",
        }
    }
}

/// 一个分组的累计：requests / token 四类 / 已计价费用 / 无价格模型的 token（不变量 5：
/// 未收录模型的用量单独可见，费用只含已计价部分，不得按 0 静默吞掉）。
/// `agents` 仅在多 agent 数据时填充（单 agent 报告保持 M1 输出不变）。
#[derive(Debug, Default, Clone, Serialize)]
pub struct Group {
    pub key: String,
    /// C2：展示名（Project 维度 = 路径末段；其他维度 None，展示用 key）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub requests: u64,
    pub tokens: TokenCounts,
    pub cost_usd: f64,
    pub unknown_pricing: bool,
    pub unknown_tokens: TokenCounts,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agents: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct Aggregated {
    pub by: &'static str,
    pub groups: Vec<Group>,
    pub totals: Group,
}

/// SF08：聚合算术全部受检——单事件更新先在临时值上全部成功再提交；
/// 多个各自合法的事件累计溢出/金额非有限时返回明确错误（不回绕、
/// 不饱和、不跳过"最后一个"制造不确定统计）。
///
/// MP04：Model 维度的分组键是**模型等价身份**（`mode_identity` 规则），
/// `claude-opus-5-5` / `claude-opus-5.5` 等写法合并为同一组；`Group.label`
/// 优先取可信 models.dev 模型名，否则退回该组原始模型名的稳定代表写法
///（字典序最小，与事件顺序无关）。下钻仍按同一身份规则匹配。
pub fn aggregate(
    events: &[UsageEvent],
    by: GroupBy,
    tz: &TimeZone,
    pricing: &Pricing,
) -> anyhow::Result<Aggregated> {
    let mut map: BTreeMap<String, GroupAccum> = BTreeMap::new();
    // 多 agent 数据才填充 groups[].agents，单 agent 报告与 M1 输出保持一致。
    let first_agent = events.first().map(|e| e.agent);
    let multi_agent = first_agent.is_some_and(|f| events.iter().any(|e| e.agent != f));
    for e in events {
        let key = match by {
            GroupBy::Day => e.ts.to_zoned(tz.clone()).date().to_string(),
            GroupBy::Model => ModelIdentity::parse(&e.model).identity_key(),
            GroupBy::Project => e.project.clone(),
            GroupBy::Agent => e.agent.as_str().to_string(),
        };
        let key_err = key.clone();
        let acc = map.entry(key.clone()).or_insert_with(|| GroupAccum {
            group: Group {
                label: None,
                key,
                ..Group::default()
            },
            representative: None,
        });
        // MP04：无可信展示名时的代表写法 = 该组原始模型名的最小字典序（稳定）。
        if by == GroupBy::Model
            && acc
                .representative
                .as_deref()
                .is_none_or(|r| e.model.as_str() < r)
        {
            acc.representative = Some(e.model.clone());
        }
        let g = &mut acc.group;
        // 临时值上全部成功才提交（requests/tokens/unknown/cost）。
        let new_requests = g
            .requests
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("分组 {} 的请求数累计超出可表示范围", key_err))?;
        let mut new_tokens = g.tokens;
        let mut new_unknown = g.unknown_tokens;
        new_tokens = new_tokens
            .checked_add(&TokenCounts::from_event(e))
            .ok_or_else(|| anyhow::anyhow!("分组 {} 的 token 累计超出可表示范围", key_err))?;
        let mut new_cost = g.cost_usd;
        match pricing.estimate(&e.model, &TokenCounts::from_event(e), e.ts) {
            Some(est) => {
                new_cost += est.cost;
                if !new_cost.is_finite() {
                    anyhow::bail!("分组 {} 的费用累计出现非有限值", key_err);
                }
                // B3：部分计价（分项缺价格）不按 0——未知分项的 token 单列，
                // † 标记语义即"费用仅含已计价部分"。
                if !est.complete {
                    new_unknown = new_unknown.checked_add(&est.unknown).ok_or_else(|| {
                        anyhow::anyhow!("分组 {} 的未知 token 累计超出可表示范围", key_err)
                    })?;
                }
            }
            None => {
                new_unknown = new_unknown
                    .checked_add(&TokenCounts::from_event(e))
                    .ok_or_else(|| {
                        anyhow::anyhow!("分组 {} 的未知 token 累计超出可表示范围", key_err)
                    })?;
            }
        }
        // 全部成功 → 提交（agents 追加不涉算术，随后执行）。
        g.requests = new_requests;
        g.tokens = new_tokens;
        g.unknown_tokens = new_unknown;
        g.cost_usd = new_cost;
        if let Some(est) = pricing.estimate(&e.model, &TokenCounts::from_event(e), e.ts) {
            if !est.complete {
                g.unknown_pricing = true;
            }
        } else {
            g.unknown_pricing = true;
        }
        if multi_agent && !g.agents.contains(&e.agent.as_str()) {
            g.agents.push(e.agent.as_str());
        }
    }
    /// 其他维度的展示名生成器（Project = 路径末段；Day/Agent = None）。
    fn other_label(by: &GroupBy, key: &str) -> Option<String> {
        if *by != GroupBy::Project {
            return None;
        }
        Some(
            key.rsplit(['/', '\\'])
                .find(|s| !s.is_empty())
                .unwrap_or(key)
                .to_string(),
        )
    }
    let mut groups: Vec<Group> = map
        .into_values()
        .map(|acc| {
            let mut g = acc.group;
            g.label = match by {
                GroupBy::Model => Some(model_label(&g.key, acc.representative.as_deref(), pricing)),
                _ => other_label(&by, &g.key),
            };
            g
        })
        .collect();
    let mut totals = Group {
        key: "合计".to_string(),
        ..Group::default()
    };
    for g in &groups {
        totals.requests = totals
            .requests
            .checked_add(g.requests)
            .ok_or_else(|| anyhow::anyhow!("总计请求数累计超出可表示范围"))?;
        totals.tokens = totals
            .tokens
            .checked_add(&g.tokens)
            .ok_or_else(|| anyhow::anyhow!("总计 token 累计超出可表示范围"))?;
        totals.cost_usd += g.cost_usd;
        if !totals.cost_usd.is_finite() {
            anyhow::bail!("总计费用累计出现非有限值");
        }
        totals.unknown_pricing |= g.unknown_pricing;
        totals.unknown_tokens = totals
            .unknown_tokens
            .checked_add(&g.unknown_tokens)
            .ok_or_else(|| anyhow::anyhow!("总计未知 token 累计超出可表示范围"))?;
        for a in &g.agents {
            if !totals.agents.contains(a) {
                totals.agents.push(a);
            }
        }
    }
    groups.push(totals.clone());
    Ok(Aggregated {
        by: by.as_str(),
        groups,
        totals,
    })
}

/// SF05：预设"近 N 天"的解析结果——统计时区下 `[起始自然日, 今天]` 的
/// 自然日闭区间（口径见 docs/stats-semantics.md §3.6）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresetRange {
    pub from: jiff::civil::Date,
    pub to: jiff::civil::Date,
}

/// 以注入的 as_of（统计时区）解析"近 N 天"：起始自然日到今天的**闭区间**，
/// 不含未来日期。days=0 按既有约定归一为 1；超大 days 使日期减法超界时
/// 返回可读错误，不 panic。纯函数：不读取系统时钟，today 由调用方注入
/// 一次（取代原 filter_days 的内部 now 读取）。
pub fn preset_days_range(as_of: &jiff::Zoned, days: u32) -> anyhow::Result<PresetRange> {
    let days = days.max(1);
    let today = as_of.date();
    // jiff Span 天数合法上限（±7304484 ≈ ±2 万年）；超过必然早于最小可表示
    // 日期，直接返回可读错误——Span::new().days() 对越界值会 panic，不能依赖
    // checked_sub 拦截。
    const MAX_SPAN_DAYS: i64 = 7_304_484;
    let back = i64::from(days) - 1;
    if back > MAX_SPAN_DAYS {
        anyhow::bail!("近 {days} 天的起始日期超出可表示范围");
    }
    let from = today
        .checked_sub(jiff::Span::new().days(back))
        .map_err(|e| anyhow::anyhow!("近 {days} 天的起始日期超出可表示范围（{e}）"))?;
    Ok(PresetRange { from, to: today })
}

/// 只保留预设区间 `[from, to]` 内的事件：统计时区落日界、闭区间、
/// 不含未来日期；每个事件只做一次时区转换（落日语义不变量）。
pub fn filter_preset_days(
    events: Vec<UsageEvent>,
    tz: &TimeZone,
    range: PresetRange,
) -> Vec<UsageEvent> {
    events
        .into_iter()
        .filter(|e| {
            let d = e.ts.to_zoned(tz.clone()).date();
            d >= range.from && d <= range.to
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AgentKind;

    fn tz() -> TimeZone {
        TimeZone::get("Asia/Shanghai").unwrap()
    }

    fn event(ts: &str, model: &str, input: u64, output: u64) -> UsageEvent {
        UsageEvent {
            ts: ts.parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: model.into(),
            session_id: "s".into(),
            record_id: String::new(),
            line: 0,
            source_path: String::new(),
            project: "proj-a".into(),
            session_initial_cwd: None,
            event_cwd: None,
            input_tokens: input,
            output_tokens: output,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    #[test]
    fn test_daily_boundary() {
        // UTC 15:59:59 = 本地 23:59:59（同日）；UTC 16:00:00 = 本地次日 00:00:00。
        let events = [
            event("2026-07-17T15:59:59.000Z", "m", 1, 1),
            event("2026-07-17T16:00:00.000Z", "m", 10, 10),
        ];
        let agg = aggregate(&events, GroupBy::Day, &tz(), &Pricing::default()).unwrap();
        // 末行为合计
        assert_eq!(agg.groups.len(), 3);
        assert_eq!(agg.groups[0].key, "2026-07-17");
        assert_eq!(agg.groups[0].tokens.input, 1);
        assert_eq!(agg.groups[1].key, "2026-07-18");
        assert_eq!(agg.groups[1].tokens.input, 10);
        assert_eq!(agg.totals.requests, 2);
    }

    #[test]
    fn test_aggregate_by_model_and_project() {
        let events = [
            event("2026-07-17T08:00:00.000Z", "tencent/hy3:free", 1, 1),
            event("2026-07-17T08:01:00.000Z", "claude-sonnet-4-5", 2, 2),
            {
                let mut e = event("2026-07-17T08:02:00.000Z", "claude-sonnet-4-5", 3, 3);
                e.project = "proj-b".into();
                e
            },
        ];
        let agg = aggregate(&events, GroupBy::Model, &tz(), &Pricing::default()).unwrap();
        assert_eq!(agg.groups.len(), 3); // grok、claude、合计
        // MP04：Model 维度的 key 是等价身份键，label 是无可信名称时的原始代表写法。
        assert_eq!(agg.groups[0].key, "claudesonnet45");
        assert_eq!(agg.groups[0].label.as_deref(), Some("claude-sonnet-4-5"));
        assert_eq!(agg.groups[0].tokens.input, 5);
        // Task 1：默认空价格表 → 所有模型 unknown（无编译期兜底）。
        assert!(agg.groups[0].unknown_pricing);
        assert_eq!(agg.groups[0].unknown_tokens.input, 5);
        assert_eq!(agg.groups[1].key, "tencent/hy3:free");
        assert!(agg.groups[1].unknown_pricing);
        assert_eq!(agg.groups[1].unknown_tokens.input, 1);
        assert_eq!(agg.totals.unknown_tokens.input, 6);

        let agg = aggregate(&events, GroupBy::Project, &tz(), &Pricing::default()).unwrap();
        assert_eq!(agg.groups.len(), 3);
        assert_eq!(agg.groups[0].key, "proj-a");
        assert_eq!(agg.groups[0].tokens.input, 3);
        assert_eq!(agg.groups[1].key, "proj-b");
        assert_eq!(agg.groups[1].tokens.input, 3);
    }

    #[test]
    fn test_aggregate_by_agent() {
        let mut e2 = event("2026-07-17T08:01:00.000Z", "m", 2, 2);
        e2.agent = AgentKind::Codex;
        let events = [event("2026-07-17T08:00:00.000Z", "m", 1, 1), e2];
        let agg = aggregate(&events, GroupBy::Agent, &tz(), &Pricing::default()).unwrap();
        assert_eq!(agg.groups.len(), 3); // claude-code、codex、合计
        assert_eq!(agg.groups[0].key, "claude-code");
        assert_eq!(agg.groups[0].requests, 1);
        assert_eq!(agg.groups[1].key, "codex");
        assert_eq!(agg.groups[1].requests, 1);
        assert_eq!(agg.groups[0].agents, ["claude-code"]);
        assert_eq!(agg.totals.requests, 2);
        assert_eq!(agg.totals.agents, ["claude-code", "codex"]);
    }

    /// SF05：预设近 N 天 = `[起始自然日, 今天]` 闭区间，不含未来日期。
    /// 固定上海"今天"（注入 as_of，不读系统时钟），覆盖：前一日、起点、
    /// 今天 23:59:59.999999、未来一天，以及 UTC 与统计时区跨日。
    #[test]
    fn preset_days_excludes_future_dates() {
        let tz = tz();
        // 上海今天 = 2026-08-10（UTC 当日 16:00 起才算上海 08-11）。
        let as_of = "2026-08-10T12:00:00+08:00[Asia/Shanghai]".parse().unwrap();
        // 近 2 天 → [08-09, 08-10] 闭区间。
        let r = preset_days_range(&as_of, 2).unwrap();
        assert_eq!(r.from.to_string(), "2026-08-09");
        assert_eq!(r.to.to_string(), "2026-08-10");

        let mk = |ts: &str| event(ts, "m", 1, 1);
        let events = vec![
            // 前一日（上海 08-09 10:00 = UTC 02:00）→ 含。
            mk("2026-08-09T02:00:00Z"),
            // 起点当刻（上海 08-09 00:00:00 = UTC 08-08T16:00:00Z）→ 含。
            mk("2026-08-08T16:00:00Z"),
            // 今天 23:59:59.999999（UTC 15:59:59.999999）→ 含。
            mk("2026-08-10T15:59:59.999999999Z"),
            // 未来一天（上海 08-11 00:00:01）→ 排除（旧实现含未来）。
            mk("2026-08-10T16:00:01Z"),
            // 前天（上海 08-08）→ 区间外排除。
            mk("2026-08-08T15:59:59Z"),
        ];
        let kept = filter_preset_days(events, &tz, r);
        assert_eq!(kept.len(), 3, "起点/前日/今天23:59:59 含，未来与区间外排除");

        // days=0 按既有约定归一为 1；days=1 → [今天, 今天]。
        let r1 = preset_days_range(&as_of, 0).unwrap();
        assert_eq!(r1.from, r1.to);
        assert_eq!(r1.to.to_string(), "2026-08-10");

        // 超大 days：日期减法超界返回可读错误，不 panic（旧实现 expect）。
        let err = preset_days_range(&as_of, u32::MAX).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("超出可表示范围"), "可读错误: {msg}");
    }

    /// SF05：一次查询冻结一个 today——as_of 由入口解析一次后贯穿整个
    /// 过滤；跨午夜（as_of 变为 00:00:01）属于下一次查询，不会在同一
    /// 次过滤中出现第二个"今天"。
    #[test]
    fn one_query_uses_one_today_across_midnight() {
        let tz = tz();
        // 查询冻结在 上海今天 23:59:59.999999（午夜前一瞬）。
        let as_of: jiff::Zoned = "2026-08-10T23:59:59.999999999+08:00[Asia/Shanghai]"
            .parse()
            .unwrap();
        let range = preset_days_range(&as_of, 1).unwrap();
        assert_eq!(range.to.to_string(), "2026-08-10", "冻结 as_of 的今天");

        let boundary = vec![
            // 上海 08-10 23:59:59 → 今天内，含。
            event("2026-08-10T15:59:59Z", "m", 1, 1),
            // 上海 08-11 00:00:01 → 冻结查询中的"未来"，排除。
            event("2026-08-10T16:00:01Z", "m", 1, 1),
        ];
        let kept = filter_preset_days(boundary, &tz, range);
        assert_eq!(kept.len(), 1, "同一查询内不允许第二个 today");
        assert_eq!(
            kept[0].ts.to_zoned(tz.clone()).date().to_string(),
            "2026-08-10"
        );

        // 跨日之后的新查询：as_of 解析为 08-11，近 2 天 = [08-10, 08-11]，
        // 原边界事件（08-10 23:59:59 / 08-11 00:00:01）都落在区间内。
        let as_of_next: jiff::Zoned = "2026-08-11T00:00:01+08:00[Asia/Shanghai]".parse().unwrap();
        let range_next = preset_days_range(&as_of_next, 2).unwrap();
        assert_eq!(range_next.from.to_string(), "2026-08-10");
        let events_next = vec![
            event("2026-08-10T15:59:59Z", "m", 1, 1),
            event("2026-08-10T16:00:01Z", "m", 1, 1),
        ];
        assert_eq!(filter_preset_days(events_next, &tz, range_next).len(), 2);
    }
}
