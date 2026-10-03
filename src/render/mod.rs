//! 渲染层：终端表格输出。JSON 输出在 `json` 子模块，两者都吃 report 层的
//! `SummaryReport`（CLI 与 GUI 数字同源）。

pub mod json;

use comfy_table::presets::UTF8_FULL;
use comfy_table::{ContentArrangement, Table};

use crate::aggregate::GroupBy;
use crate::model::AgentKind;
use crate::report::{SourceReport, SummaryReport};

pub fn table(report: &SummaryReport) -> String {
    let by_label = match report.by {
        "day" => GroupBy::Day.label(),
        "model" => GroupBy::Model.label(),
        "agent" => GroupBy::Agent.label(),
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
    for (i, g) in report.groups.iter().enumerate() {
        let is_total = i + 1 == report.groups.len();
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
    // 单源保持 M1 脚注格式；多源逐源一行。
    if report.sources.len() == 1 {
        out.push_str(&source_footer(&report.sources[0]));
    } else {
        let lines: Vec<String> = report
            .sources
            .iter()
            .map(|s| format!("{}: {}", s.agent.as_str(), source_footer(s)))
            .collect();
        out.push_str(&lines.join("\n"));
    }
    if report.totals.unknown_pricing {
        out.push_str("\n† 部分用量来自无价格模型，费用仅含已计价部分（未知用量见 --json）");
    }
    out
}

fn source_footer(s: &SourceReport) -> String {
    let st = &s.stats;
    let mut f = format!(
        "文件 {} · 行 {} · 事件 {} · 去重丢弃 {} · 坏行 {}",
        st.files_scanned, st.lines_seen, st.events, st.duplicates_dropped, st.bad_lines,
    );
    match s.agent {
        AgentKind::ClaudeCode => {
            f.push_str(&format!(
                " · 跳过 sidechain {} / synthetic {}",
                st.skipped_sidechain, st.skipped_synthetic,
            ));
        }
        AgentKind::Codex => {
            f.push_str(&format!(
                " · 跳过 零分量 {} / 无模型 {} · 忽略 usage_record {}",
                st.skipped_zero_usage, st.skipped_no_model, st.ignored_token_usage_record,
            ));
        }
    }
    f
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
        use crate::report::{SourceReport, SummaryReport};
        use jiff::tz::TimeZone;

        let events = vec![UsageEvent {
            ts: "2026-07-17T08:00:00.000Z".parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: "tencent/hy3:free".into(),
            session_id: "s".into(),
            record_id: String::new(),
            project: "p".into(),
            input_tokens: 1234567,
            output_tokens: 42,
            cache_write_tokens: 0,
            cache_read_tokens: 5,
        }];
        let tz = TimeZone::get("Asia/Shanghai").unwrap();
        let agg = crate::aggregate::aggregate(
            &events,
            crate::aggregate::GroupBy::Day,
            &tz,
            &Pricing::default(),
        );
        let report = SummaryReport {
            timezone: "Asia/Shanghai".into(),
            by: agg.by,
            groups: agg.groups,
            totals: agg.totals,
            sources: vec![SourceReport {
                agent: AgentKind::ClaudeCode,
                stats: crate::source::CollectStats {
                    files_scanned: 1,
                    lines_seen: 9,
                    bad_lines: 0,
                    duplicates_dropped: 3,
                    skipped_sidechain: 0,
                    skipped_synthetic: 0,
                    events: 1,
                    ..crate::source::CollectStats::default()
                },
            }],
            warnings: Vec::new(),
            generated_at: "t".into(),
        };
        let out = table(&report);
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
