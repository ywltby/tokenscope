//! 渲染层：终端表格输出。JSON 输出在 `json` 子模块。

pub mod json;

use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};

use crate::aggregate::{Aggregated, GroupBy};
use crate::source::CollectStats;

pub fn table(agg: &Aggregated, stats: &CollectStats) -> String {
    let by_label = match agg.by {
        "day" => GroupBy::Day.label(),
        "model" => GroupBy::Model.label(),
        _ => GroupBy::Project.label(),
    };
    let mut t = Table::new();
    t.load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_header(vec![
            by_label,
            "请求",
            "输入",
            "输出",
            "缓存写",
            "缓存读",
            "合计",
            "费用$",
        ]);
    for (i, g) in agg.groups.iter().enumerate() {
        let is_total = i + 1 == agg.groups.len();
        let cost = if g.unknown_pricing {
            format!("{}†", fmt_cost(g.cost_usd))
        } else {
            fmt_cost(g.cost_usd)
        };
        let key = if is_total {
            "合计".to_string()
        } else {
            g.key.clone()
        };
        t.add_row(vec![
            key,
            fmt_thousands(g.requests),
            fmt_thousands(g.tokens.input),
            fmt_thousands(g.tokens.output),
            fmt_thousands(g.tokens.cache_write),
            fmt_thousands(g.tokens.cache_read),
            fmt_thousands(g.tokens.total()),
            cost,
        ]);
    }
    let mut out = t.to_string();
    out.push('\n');
    out.push_str(&format!(
        "文件 {} · 行 {} · 事件 {} · 去重丢弃 {} · 坏行 {} · 跳过 sidechain {} / synthetic {}",
        stats.files_scanned,
        stats.lines_seen,
        stats.events,
        stats.duplicates_dropped,
        stats.bad_lines,
        stats.skipped_sidechain,
        stats.skipped_synthetic,
    ));
    if agg.totals.unknown_pricing {
        out.push_str("\n† 部分用量来自无价格模型，费用仅含已计价部分（未知用量见 --json）");
    }
    out
}

fn fmt_cost(v: f64) -> String {
    if v == 0.0 {
        "0.00".to_string()
    } else if v < 0.01 {
        format!("{v:.6}")
    } else {
        format!("{v:.2}")
    }
}

pub fn fmt_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_table() {
        use crate::model::{AgentKind, UsageEvent};
        use crate::pricing::Pricing;
        use jiff::tz::TimeZone;

        let events = vec![UsageEvent {
            ts: "2026-07-17T08:00:00.000Z".parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: "grok-4.5-build".into(),
            session_id: "s".into(),
            project: "p".into(),
            input_tokens: 1234567,
            output_tokens: 42,
            cache_write_tokens: 0,
            cache_read_tokens: 5,
        }];
        let tz = TimeZone::get("Asia/Shanghai").unwrap();
        let agg =
            crate::aggregate::aggregate(&events, crate::aggregate::GroupBy::Day, &tz, &Pricing);
        let stats = crate::source::CollectStats {
            files_scanned: 1,
            lines_seen: 9,
            bad_lines: 0,
            duplicates_dropped: 3,
            skipped_sidechain: 0,
            skipped_synthetic: 0,
            events: 1,
        };
        let out = table(&agg, &stats);
        assert!(out.contains("2026-07-17"), "应包含日期分组：{out}");
        assert!(out.contains("1,234,567"), "千分位格式：{out}");
        assert!(out.contains('†'), "未知计价标记：{out}");
        assert!(out.contains("去重丢弃 3"));
    }

    #[test]
    fn test_fmt_thousands() {
        assert_eq!(fmt_thousands(0), "0");
        assert_eq!(fmt_thousands(999), "999");
        assert_eq!(fmt_thousands(1000), "1,000");
        assert_eq!(fmt_thousands(1234567), "1,234,567");
    }
}
