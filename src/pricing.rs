//! 模型价格表：USD / 百万 token，四类单价（input / output / cache 写 / cache 读）。
//! 内置牌价快照自 cc-switch `model_pricing`（2026-10-03），仅作估算；查找为
//! **最长前缀匹配**（`gpt-5.6-luna` 先于 `gpt-5.6`、`claude-opus-4-5` 先于
//! `claude-opus-4`）。M4 起支持外置 `~/.tokenscope/pricing.toml`：与内置表合并、
//! 同前缀**外置覆盖内置**；文件缺失 = 纯内置（静默），解析失败 = 警告 + 纯内置。
//! 未收录模型返回 None，由聚合层按 unknown 单独呈现——不得按 0 静默吞掉（M1 不变量 5）。

use std::path::Path;

use serde::Serialize;

use crate::model::TokenCounts;

/// (模型前缀, input, output, cache_write, cache_read)
const TABLE: &[(&str, f64, f64, f64, f64)] = &[
    // Claude
    ("claude-opus-5", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-4-5", 5.0, 25.0, 6.25, 0.5),
    ("claude-opus-4", 15.0, 75.0, 18.75, 1.5),
    ("claude-sonnet-5", 2.0, 10.0, 2.5, 0.2),
    ("claude-sonnet-4-5", 3.0, 15.0, 3.75, 0.3),
    ("claude-sonnet-4", 3.0, 15.0, 3.75, 0.3),
    ("claude-3-7-sonnet", 3.0, 15.0, 3.75, 0.3),
    ("claude-3-5-sonnet", 3.0, 15.0, 3.75, 0.3),
    ("claude-haiku-4-5", 1.0, 5.0, 1.25, 0.1),
    ("claude-3-5-haiku", 0.8, 4.0, 1.0, 0.08),
    // GPT
    ("gpt-6-astra", 10.0, 50.0, 1.0, 12.5),
    ("gpt-5.6-luna", 0.2, 1.2, 0.02, 0.25),
    ("gpt-5.6-terra", 2.0, 12.0, 0.2, 2.5),
    ("gpt-5.6", 4.0, 20.0, 0.4, 5.0),
    ("gpt-5.5", 5.0, 30.0, 0.5, 0.0),
    ("gpt-5.4-nano", 0.2, 1.25, 0.02, 0.0),
    ("gpt-5.4-mini", 0.75, 4.5, 0.075, 0.0),
    ("gpt-5.4", 2.5, 15.0, 0.25, 0.0),
    ("gpt-5.3-codex", 1.75, 14.0, 0.175, 0.0),
    ("gpt-5.2", 1.75, 14.0, 0.175, 0.0),
    ("gpt-5.1", 1.25, 10.0, 0.125, 0.0),
    ("gpt-5-mini", 0.25, 2.0, 0.025, 0.0),
    ("gpt-5-nano", 0.05, 0.4, 0.005, 0.0),
    ("gpt-5", 1.25, 10.0, 0.125, 0.0),
    // Grok
    ("grok-4.5", 2.0, 6.0, 0.3, 0.0),
    ("grok-4.6", 2.0, 6.0, 0.5, 0.0),
    ("grok-4.7", 2.0, 6.0, 0.5, 0.0),
    ("grok-4.20", 1.25, 2.5, 0.2, 0.0),
    ("grok-4-1-fast", 0.2, 0.5, 0.05, 0.0),
    ("grok-4", 3.0, 15.0, 0.75, 0.0),
    ("grok-3-mini", 0.25, 0.5, 0.075, 0.0),
    ("grok-3", 3.0, 15.0, 0.75, 0.0),
    ("grok-code-fast", 1.0, 2.0, 0.2, 0.0),
    // DeepSeek
    ("deepseek-v4-flash", 0.3, 1.2, 0.006, 0.0),
    ("deepseek-v4-pro", 1.32, 3.96, 0.044, 0.0),
    ("deepseek-v3.2", 0.28, 0.42, 0.028, 0.0),
    ("deepseek-v3.1", 0.55, 1.67, 0.055, 0.0),
    // Kimi / Doubao / MiMo
    ("kimi-k2.5", 0.6, 3.0, 0.1, 0.0),
    ("doubao-seed-2-0-lite", 0.08, 0.5, 0.017, 0.0),
    ("doubao-seed-2-0", 0.47, 2.37, 0.09, 0.0),
    ("doubao-seed-2-1-pro", 0.84, 4.2, 0.17, 0.0),
    ("mimo-v2.5-pro", 0.435, 0.87, 0.0036, 0.0),
    ("mimo-v2.5", 0.14, 0.29, 0.0028, 0.0),
];

#[derive(Debug, Clone)]
struct Entry {
    prefix: String,
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
    external: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

/// GUI 设置页展示用条目（含来源标识）。
#[derive(Debug, Clone, Serialize)]
pub struct PricingEntry {
    pub prefix: String,
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
    pub source: &'static str,
}

/// 外置 pricing.toml 的反序列化结构。
#[derive(Debug, Default, serde::Deserialize)]
struct ExternalFile {
    #[serde(default)]
    model: Vec<ExternalModel>,
}

#[derive(Debug, serde::Deserialize)]
struct ExternalModel {
    prefix: String,
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
}

/// 外置文件不存在时「创建模板」写入的内容。
pub const PRICING_TEMPLATE: &str = r#"# TokenScope 外置价格表（USD / 百万 token；同前缀覆盖内置表，最长前缀匹配）
# 修改保存后，下一次统计即生效（无需重启）。

[[model]]
prefix = "claude-opus-5"
input = 5.0
output = 25.0
cache_write = 6.25
cache_read = 0.5
"#;

#[derive(Debug, Clone)]
pub struct Pricing {
    entries: Vec<Entry>,
}

impl Default for Pricing {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Pricing {
    /// 纯内置表。
    pub fn builtin() -> Self {
        Self {
            entries: TABLE
                .iter()
                .map(|(p, i, o, cw, cr)| Entry {
                    prefix: (*p).to_string(),
                    input: *i,
                    output: *o,
                    cache_write: *cw,
                    cache_read: *cr,
                    external: false,
                })
                .collect(),
        }
    }

    /// 内置 + 外置合并。`external` 为 None 或文件缺失 → 纯内置且无警告；
    /// 解析失败 → 警告 + 纯内置；同前缀外置覆盖内置，新前缀追加。
    pub fn load(external: Option<&Path>) -> (Self, Vec<String>) {
        let mut pricing = Self::builtin();
        let mut warnings = Vec::new();
        let Some(path) = external else {
            return (pricing, warnings);
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            // 文件不存在是常态，静默；其他读取错误也按缺失处理。
            return (pricing, warnings);
        };
        let parsed: ExternalFile = match toml::from_str(&text) {
            Ok(p) => p,
            Err(e) => {
                warnings.push(format!(
                    "外置价格文件解析失败，已退回内置价格表: {}（{e}）",
                    path.display()
                ));
                return (pricing, warnings);
            }
        };
        for m in parsed.model {
            let entry = Entry {
                prefix: m.prefix,
                input: m.input,
                output: m.output,
                cache_write: m.cache_write,
                cache_read: m.cache_read,
                external: true,
            };
            match pricing
                .entries
                .iter_mut()
                .find(|e| e.prefix == entry.prefix)
            {
                Some(builtin) => *builtin = entry,
                None => pricing.entries.push(entry),
            }
        }
        (pricing, warnings)
    }

    pub fn lookup(&self, model: &str) -> Option<ModelPrice> {
        let m = model.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|e| m.starts_with(e.prefix.to_ascii_lowercase().as_str()))
            .max_by_key(|e| e.prefix.len())
            .map(|e| ModelPrice {
                input: e.input,
                output: e.output,
                cache_write: e.cache_write,
                cache_read: e.cache_read,
            })
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

    /// GUI 设置页条目（内置 + 外置合并视图，含来源标识）。
    pub fn entries(&self) -> Vec<PricingEntry> {
        self.entries
            .iter()
            .map(|e| PricingEntry {
                prefix: e.prefix.clone(),
                input: e.input,
                output: e.output,
                cache_write: e.cache_write,
                cache_read: e.cache_read,
                source: if e.external { "外置" } else { "内置" },
            })
            .collect()
    }

    pub fn external_count(&self) -> usize {
        self.entries.iter().filter(|e| e.external).count()
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
        let p = Pricing::builtin()
            .lookup("claude-sonnet-4-5-20250929")
            .unwrap();
        assert_eq!(p.input, 3.0);
        assert_eq!(p.output, 15.0);
        let p = Pricing::builtin()
            .lookup("Claude-Sonnet-4-20250514")
            .unwrap();
        assert_eq!(p.input, 3.0);
    }

    #[test]
    fn test_pricing_longest_prefix() {
        // luna 必须先于 gpt-5.6 命中；nano 先于 gpt-5.4、gpt-5。
        let p = Pricing::builtin().lookup("gpt-5.6-luna").unwrap();
        assert_eq!(p.input, 0.2);
        assert_eq!(p.output, 1.2);
        assert_eq!(Pricing::builtin().lookup("gpt-5.6-sol").unwrap().input, 4.0);
        assert_eq!(
            Pricing::builtin().lookup("gpt-5.4-nano").unwrap().input,
            0.2
        );
        // opus-4-5 起降价：4-5 后缀命中 5/25，4 与 4-1 仍是 15/75。
        assert_eq!(
            Pricing::builtin()
                .lookup("claude-opus-4-5-20251101")
                .unwrap()
                .input,
            5.0
        );
        assert_eq!(
            Pricing::builtin()
                .lookup("claude-opus-4-20250514")
                .unwrap()
                .input,
            15.0
        );
        // deepseek / grok 带后缀的完整模型串。
        assert_eq!(
            Pricing::builtin()
                .lookup("deepseek-v4-flash-0731")
                .unwrap()
                .input,
            0.3
        );
        assert_eq!(
            Pricing::builtin().lookup("grok-4.5-build").unwrap().input,
            2.0
        );
        assert_eq!(
            Pricing::builtin()
                .lookup("grok-4-1-fast-reasoning")
                .unwrap()
                .input,
            0.2
        );
    }

    #[test]
    fn test_pricing_unknown_model() {
        assert!(Pricing::builtin().lookup("tencent/hy3:free").is_none());
        assert!(Pricing::builtin().lookup("<synthetic>").is_none());
        assert!(
            Pricing::builtin()
                .cost("qwen-x", &counts(1, 1, 0, 0))
                .is_none()
        );
    }

    #[test]
    fn test_pricing_cost_math() {
        // 1M input + 1M output 的 sonnet-4-5 = $3 + $15。
        let c = Pricing::builtin()
            .cost("claude-sonnet-4-5", &counts(1_000_000, 1_000_000, 0, 0))
            .unwrap();
        assert!((c - 18.0).abs() < 1e-9);
        let c = Pricing::builtin()
            .cost("claude-sonnet-4-5", &counts(0, 0, 1_000_000, 1_000_000))
            .unwrap();
        assert!((c - 4.05).abs() < 1e-9);
        // gpt-5.6-sol 混合四类：800*4 + 100*20 + 50*0.4 + 200*5 = 6220 / 1M。
        let c = Pricing::builtin()
            .cost("gpt-5.6-sol", &counts(800, 100, 50, 200))
            .unwrap();
        assert!((c - 6220.0 / 1_000_000.0).abs() < 1e-12);
    }

    #[test]
    fn test_pricing_external_missing_is_silent() {
        let (p, warnings) = Pricing::load(Some(Path::new("Z:/no-such/pricing.toml")));
        assert_eq!(p.external_count(), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_pricing_external_override_and_append() {
        let dir = std::env::temp_dir().join(format!("tokenscope-pricing-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing.toml");
        std::fs::write(
            &path,
            r#"
[[model]]
prefix = "claude-opus-5"
input = 9.0
output = 9.0
cache_write = 9.0
cache_read = 9.0

[[model]]
prefix = "my-model/zen"
input = 1.0
output = 2.0
cache_write = 0.0
cache_read = 0.0
"#,
        )
        .unwrap();
        let (p, warnings) = Pricing::load(Some(&path));
        assert!(warnings.is_empty());
        assert_eq!(p.external_count(), 2);
        // 同前缀覆盖内置
        assert_eq!(p.lookup("claude-opus-5").unwrap().input, 9.0);
        // 新前缀追加且最长前缀匹配仍然生效
        assert_eq!(p.lookup("my-model/zen-2").unwrap().output, 2.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_external_broken_falls_back() {
        let dir =
            std::env::temp_dir().join(format!("tokenscope-pricing-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing.toml");
        std::fs::write(&path, "not valid toml [[[").unwrap();
        let (p, warnings) = Pricing::load(Some(&path));
        assert_eq!(p.external_count(), 0);
        assert_eq!(p.lookup("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("解析失败"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
