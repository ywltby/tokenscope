//! 聚合层：把事件按日（Asia/Shanghai 自然日）/ 模型 / 项目分组并计价。
//! 层间契约：只依赖 model 与 pricing，不触碰任何 source 内部类型。

use std::collections::BTreeMap;

use jiff::tz::TimeZone;
use serde::Serialize;

use crate::model::{TokenCounts, UsageEvent};
use crate::pricing::Pricing;

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

pub fn aggregate(
    events: &[UsageEvent],
    by: GroupBy,
    tz: &TimeZone,
    pricing: &Pricing,
) -> Aggregated {
    let mut map: BTreeMap<String, Group> = BTreeMap::new();
    // 多 agent 数据才填充 groups[].agents，单 agent 报告与 M1 输出保持一致。
    let first_agent = events.first().map(|e| e.agent);
    let multi_agent = first_agent.is_some_and(|f| events.iter().any(|e| e.agent != f));
    for e in events {
        let key = match by {
            GroupBy::Day => e.ts.to_zoned(tz.clone()).date().to_string(),
            GroupBy::Model => e.model.clone(),
            GroupBy::Project => e.project.clone(),
            GroupBy::Agent => e.agent.as_str().to_string(),
        };
        let g = map.entry(key.clone()).or_insert_with(|| Group {
            label: project_label(&by, &key),
            key,
            ..Group::default()
        });
        g.requests += 1;
        g.tokens.add_event(e);
        if multi_agent && !g.agents.contains(&e.agent.as_str()) {
            g.agents.push(e.agent.as_str());
        }
        match pricing.estimate(&e.model, &TokenCounts::from_event(e)) {
            Some(est) => {
                g.cost_usd += est.cost;
                // B3：部分计价（分项缺价格）不按 0——未知分项的 token 单列，
                // † 标记语义即"费用仅含已计价部分"。
                if !est.complete {
                    g.unknown_pricing = true;
                    g.unknown_tokens.add(&est.unknown);
                }
            }
            None => {
                g.unknown_pricing = true;
                g.unknown_tokens.add_event(e);
            }
        }
    }
    let mut groups: Vec<Group> = map.into_values().collect();
    /// Project 维度的展示名 = 路径末段（正反斜杠都容忍）；其他维度 None。
    fn project_label(by: &GroupBy, key: &str) -> Option<String> {
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
    let mut totals = Group {
        key: "合计".to_string(),
        ..Group::default()
    };
    for g in &groups {
        totals.requests += g.requests;
        totals.tokens.add(&g.tokens);
        totals.cost_usd += g.cost_usd;
        totals.unknown_pricing |= g.unknown_pricing;
        totals.unknown_tokens.add(&g.unknown_tokens);
        for a in &g.agents {
            if !totals.agents.contains(a) {
                totals.agents.push(a);
            }
        }
    }
    groups.push(totals.clone());
    Aggregated {
        by: by.as_str(),
        groups,
        totals,
    }
}

/// 只保留最近 `days` 个自然日（含今天，按本地时区 Asia/Shanghai 落日界）的事件。
pub fn filter_days(events: Vec<UsageEvent>, tz: &TimeZone, days: u32) -> Vec<UsageEvent> {
    let days = days.max(1);
    let today = jiff::Zoned::now().with_time_zone(tz.clone()).date();
    let cutoff = today
        .checked_sub(jiff::Span::new().days(i64::from(days) - 1))
        .expect("日期减法不会溢出");
    events
        .into_iter()
        .filter(|e| e.ts.to_zoned(tz.clone()).date() >= cutoff)
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
            project: "proj-a".into(),
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
        let agg = aggregate(&events, GroupBy::Day, &tz(), &Pricing::default());
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
        let agg = aggregate(&events, GroupBy::Model, &tz(), &Pricing::default());
        assert_eq!(agg.groups.len(), 3); // grok、claude、合计
        assert_eq!(agg.groups[0].key, "claude-sonnet-4-5");
        assert_eq!(agg.groups[0].tokens.input, 5);
        // Task 1：默认空价格表 → 所有模型 unknown（无编译期兜底）。
        assert!(agg.groups[0].unknown_pricing);
        assert_eq!(agg.groups[0].unknown_tokens.input, 5);
        assert_eq!(agg.groups[1].key, "tencent/hy3:free");
        assert!(agg.groups[1].unknown_pricing);
        assert_eq!(agg.groups[1].unknown_tokens.input, 1);
        assert_eq!(agg.totals.unknown_tokens.input, 6);

        let agg = aggregate(&events, GroupBy::Project, &tz(), &Pricing::default());
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
        let agg = aggregate(&events, GroupBy::Agent, &tz(), &Pricing::default());
        assert_eq!(agg.groups.len(), 3); // claude-code、codex、合计
        assert_eq!(agg.groups[0].key, "claude-code");
        assert_eq!(agg.groups[0].requests, 1);
        assert_eq!(agg.groups[1].key, "codex");
        assert_eq!(agg.groups[1].requests, 1);
        assert_eq!(agg.groups[0].agents, ["claude-code"]);
        assert_eq!(agg.totals.requests, 2);
        assert_eq!(agg.totals.agents, ["claude-code", "codex"]);
    }

    #[test]
    fn test_filter_days() {
        let old = event("2020-01-01T00:00:00.000Z", "m", 1, 1);
        // 直接用 now：now - 1h 在本地午夜后一小时内会落到昨天，属测试自身的时间依赖缺陷。
        let now_ts = jiff::Timestamp::now();
        let mut recent = event("2026-01-01T00:00:00.000Z", "m", 1, 1);
        recent.ts = now_ts;
        let kept = filter_days(vec![old, recent], &tz(), 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].ts, now_ts);
    }
}
