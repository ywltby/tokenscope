//! 价格同步告警的统一收集、汇总与记录。
//!
//! 同步函数（modelsdev/openrouter）只收集结构化告警，不写日志；手动与
//! 自动两条同步路径在调用方统一调用 [`log_sync_warnings`]：默认级别每个
//! 来源每类告警至多一条 WARN 汇总，逐条明细降为 debug（文本与改造前的
//! 逐条告警一致，同一告警不再写两遍）。

use serde::Serialize;
use std::collections::BTreeMap;

/// 单条同步告警：类别键在产生告警的位置确定，不靠解析文本猜测。
#[derive(Debug, Clone, Serialize)]
pub struct SyncWarning {
    /// 稳定类别键（snake_case），WARN 汇总按它分组。
    pub category: &'static str,
    /// 模型 id（OpenRouter 结构异常取不到 id 时用数组序号标签）。
    pub model: String,
    /// 逐条明细的完整日志文本，与改造前的逐条告警一致。
    pub detail: String,
    /// true = 进入默认级别 WARN 汇总；false = 只写 debug 明细
    /// （models.dev 兼容字段不误报为冲突，见计划不变量 8）。
    pub for_summary: bool,
}

/// WARN 汇总（纯函数）：按类别分组、每类一行，写明来源、类别、受影响的
/// 模型数与告警条数，示例按字典序取前 3 个、超出注明「等 N 个」。
/// 只统计 `for_summary` 的告警；输入为空时输出为空；同一输入输出相同。
pub fn summarize(source: &str, warnings: &[SyncWarning]) -> Vec<String> {
    // BTreeMap/BTreeSet：类别按键字典序、示例按模型 id 字典序，输出确定。
    let mut groups: BTreeMap<&'static str, (std::collections::BTreeSet<&str>, usize)> =
        BTreeMap::new();
    for w in warnings.iter().filter(|w| w.for_summary) {
        let (models, count) = groups.entry(w.category).or_default();
        models.insert(w.model.as_str());
        *count += 1;
    }
    groups
        .into_iter()
        .map(|(category, (models, count))| {
            let total_models = models.len();
            let examples: Vec<&str> = models.iter().copied().take(3).collect();
            let note = if total_models > 3 {
                format!("（等 {total_models} 个）")
            } else {
                String::new()
            };
            format!(
                "{source} 同步丢弃数据汇总[{category}]: 模型 {total_models} 个 / 告警 {count} 条，示例: {}{note}",
                examples.join("、")
            )
        })
        .collect()
}

/// 统一记录：每条汇总行写 WARN，每条原始告警写 debug（明细文本不变）。
pub fn log_sync_warnings(source: &str, warnings: &[SyncWarning]) {
    for line in summarize(source, warnings) {
        log::warn!("{line}");
    }
    for w in warnings {
        log::debug!("{}", w.detail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(category: &'static str, model: &str, detail: &str, for_summary: bool) -> SyncWarning {
        SyncWarning {
            category,
            model: model.to_string(),
            detail: detail.to_string(),
            for_summary,
        }
    }

    #[test]
    fn summarize_groups_by_category_one_line_each() {
        let warnings = [
            w("a_cat", "p/m2", "d2", true),
            w("b_cat", "p/m1", "d1", true),
            w("a_cat", "p/m3", "d3", true),
        ];
        let lines = summarize("models.dev", &warnings);
        assert_eq!(lines.len(), 2, "两类各一行: {lines:?}");
        // 类别按键字典序稳定输出，行内写明来源与类别。
        assert!(lines[0].starts_with("models.dev "), "{:?}", lines[0]);
        assert!(lines[0].contains("a_cat"), "{:?}", lines[0]);
        assert!(lines[1].contains("b_cat"), "{:?}", lines[1]);
    }

    #[test]
    fn summarize_reports_model_and_warning_counts() {
        let warnings = [
            w("cat", "p/a", "d", true),
            w("cat", "p/a", "d", true),
            w("cat", "p/b", "d", true),
        ];
        let lines = summarize("OpenRouter", &warnings);
        assert_eq!(lines.len(), 1);
        // 同一模型的多条同类告警按 1 个模型、2 条告警计数。
        assert!(lines[0].contains("模型 2 个"), "{:?}", lines[0]);
        assert!(lines[0].contains("告警 3 条"), "{:?}", lines[0]);
    }

    #[test]
    fn summarize_takes_three_lexicographic_examples_and_notes_total() {
        let models = ["p/z", "p/m", "p/a", "p/b"];
        let warnings: Vec<SyncWarning> = models.iter().map(|m| w("cat", m, "d", true)).collect();
        let lines = summarize("OpenRouter", &warnings);
        assert_eq!(lines.len(), 1);
        // 超过 3 个：示例取字典序前 3（p/a、p/b、p/m），并注明总数。
        assert!(lines[0].contains("p/a"), "{:?}", lines[0]);
        assert!(lines[0].contains("p/b"), "{:?}", lines[0]);
        assert!(lines[0].contains("p/m"), "{:?}", lines[0]);
        assert!(!lines[0].contains("p/z"), "{:?}", lines[0]);
        assert!(lines[0].contains("等 4 个"), "{:?}", lines[0]);
    }

    #[test]
    fn summarize_empty_input_yields_no_lines() {
        assert!(summarize("models.dev", &[]).is_empty());
        assert!(summarize("OpenRouter", &[]).is_empty());
    }

    #[test]
    fn summarize_three_or_fewer_models_omit_total_note() {
        let warnings = [w("cat", "p/a", "d", true), w("cat", "p/b", "d", true)];
        let lines = summarize("models.dev", &warnings);
        assert_eq!(lines.len(), 1);
        assert!(
            !lines[0].contains("等"),
            "3 个及以内不注明总数: {:?}",
            lines[0]
        );
    }

    #[test]
    fn summarize_skips_debug_only_warnings() {
        // 兼容字段（for_summary=false）只写 debug 明细，不进入 WARN 汇总。
        let warnings = [
            w("over200k_compatible", "p/a", "d", false),
            w("over200k_conflict", "p/b", "d", true),
        ];
        let lines = summarize("models.dev", &warnings);
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("over200k_conflict"), "{:?}", lines[0]);
        assert!(
            !lines[0].contains("compatible"),
            "兼容类别不得产生汇总行: {:?}",
            lines[0]
        );
    }

    #[test]
    fn summarize_is_deterministic() {
        let warnings = [
            w("b", "p/x", "d", true),
            w("a", "p/y", "d", true),
            w("a", "p/x", "d", true),
        ];
        assert_eq!(summarize("s", &warnings), summarize("s", &warnings));
    }
}
