//! 内置价格表：USD / 百万 token，四类单价（input / output / cache 写 / cache 读）。
//! 牌价为公开快照、仅作估算；前缀匹配、表序即优先级（长前缀在前）。
//! 未收录模型返回 None，由聚合层按 unknown 单独呈现——不得按 0 静默吞掉（M1 不变量 5）。

use crate::model::TokenCounts;

/// (模型前缀, input, output, cache_write, cache_read)
const TABLE: &[(&str, f64, f64, f64, f64)] = &[
    ("claude-opus-4", 15.0, 75.0, 18.75, 1.5),
    ("claude-sonnet-4-5", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-4", 3.0, 15.0, 3.75, 0.3),
    ("claude-3-7-sonnet", 3.0, 15.0, 3.75, 0.3),
    ("claude-3-5-sonnet", 3.0, 15.0, 3.75, 0.3),
    ("claude-haiku-4-5", 1.0, 5.0, 1.25, 0.1),
    ("claude-3-5-haiku", 0.8, 4.0, 1.0, 0.08),
];

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Pricing;

impl Pricing {
    pub fn lookup(&self, model: &str) -> Option<ModelPrice> {
        let m = model.to_ascii_lowercase();
        TABLE.iter().find(|(p, ..)| m.starts_with(p)).map(
            |&(_, input, output, cache_write, cache_read)| ModelPrice {
                input,
                output,
                cache_write,
                cache_read,
            },
        )
    }

    /// 返回 None 表示模型未收录（unknown），不是 0 费用。
    pub fn cost(&self, model: &str, t: &TokenCounts) -> Option<f64> {
        let p = self.lookup(model)?;
        Some(
            (t.input as f64 * p.input
                + t.output as f64 * p.output
                + t.cache_write as f64 * p.cache_write
                + t.cache_read as f64 * p.cache_read)
                / 1_000_000.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(input: u64, output: u64, cw: u64, cr: u64) -> TokenCounts {
        TokenCounts {
            input,
            output,
            cache_write: cw,
            cache_read: cr,
        }
    }

    #[test]
    fn test_pricing_known_model() {
        // 带日期后缀的完整模型串也按前缀命中；sonnet-4-5 优先于 sonnet-4。
        let p = Pricing.lookup("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(p.input, 3.0);
        assert_eq!(p.output, 15.0);
        let p = Pricing.lookup("Claude-Sonnet-4-20250514").unwrap();
        assert_eq!(p.input, 3.0);
    }

    #[test]
    fn test_pricing_unknown_model() {
        assert!(Pricing.lookup("grok-4.5-build").is_none());
        assert!(Pricing.lookup("<synthetic>").is_none());
        assert!(Pricing.cost("gpt-5.6-luna", &counts(1, 1, 0, 0)).is_none());
    }

    #[test]
    fn test_pricing_cost_math() {
        // 1M input + 1M output 的 sonnet-4-5 = $3 + $15。
        let c = Pricing
            .cost("claude-sonnet-4-5", &counts(1_000_000, 1_000_000, 0, 0))
            .unwrap();
        assert!((c - 18.0).abs() < 1e-9);
        let c = Pricing
            .cost("claude-sonnet-4-5", &counts(0, 0, 1_000_000, 1_000_000))
            .unwrap();
        assert!((c - 4.05).abs() < 1e-9);
    }
}
