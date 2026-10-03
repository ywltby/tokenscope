//! JSON 输出：机器可读的完整报告（含采集统计与警告）。

use anyhow::Result;
use serde::Serialize;

use crate::aggregate::Aggregated;
use crate::model::AgentKind;
use crate::source::CollectStats;

#[derive(Serialize)]
struct Report<'a> {
    generated_at: String,
    agent: &'static str,
    by: &'a str,
    groups: &'a [crate::aggregate::Group],
    totals: &'a crate::aggregate::Group,
    stats: &'a CollectStats,
    warnings: &'a [String],
}

/// `generated_at` 由调用方传入（RFC3339，带本地时区偏移），便于测试。
pub fn to_json(
    agg: &Aggregated,
    agent: AgentKind,
    stats: &CollectStats,
    warnings: &[String],
    generated_at: &str,
) -> Result<String> {
    let report = Report {
        generated_at: generated_at.to_string(),
        agent: agent.as_str(),
        by: agg.by,
        groups: &agg.groups[..agg.groups.len() - 1], // 合并行单列在 totals，不在 groups 里重复
        totals: &agg.totals,
        stats,
        warnings,
    };
    Ok(serde_json::to_string_pretty(&report)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aggregate::{GroupBy, aggregate};
    use crate::model::{AgentKind as AK, UsageEvent};
    use crate::pricing::Pricing;
    use jiff::tz::TimeZone;

    #[test]
    fn test_render_json() {
        let events = vec![UsageEvent {
            ts: "2026-07-17T08:00:00.000Z".parse().unwrap(),
            agent: AK::ClaudeCode,
            model: "grok-4.5-build".into(),
            session_id: "s".into(),
            project: "p".into(),
            input_tokens: 100,
            output_tokens: 50,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
        }];
        let tz = TimeZone::get("Asia/Shanghai").unwrap();
        let agg = aggregate(&events, GroupBy::Day, &tz, &Pricing);
        let stats = CollectStats {
            events: 1,
            ..Default::default()
        };
        let out = to_json(
            &agg,
            AgentKind::ClaudeCode,
            &stats,
            &["w".to_string()],
            "2026-10-03T12:00:00+08:00",
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["agent"], "claude-code");
        assert_eq!(v["by"], "day");
        assert_eq!(v["groups"].as_array().unwrap().len(), 1);
        assert_eq!(v["groups"][0]["key"], "2026-07-17");
        assert_eq!(v["groups"][0]["tokens"]["input"], 100);
        assert_eq!(v["groups"][0]["unknown_pricing"], true);
        assert_eq!(v["groups"][0]["unknown_tokens"]["input"], 100);
        assert_eq!(v["totals"]["requests"], 1);
        assert_eq!(v["stats"]["events"], 1);
        assert_eq!(v["warnings"][0], "w");
    }
}
