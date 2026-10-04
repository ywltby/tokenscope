//! 模型价格表：USD / 百万 token，四类单价（input / output / cache 写 / cache 读）。
//! 查找为**最长前缀匹配**（`gpt-5.6-luna` 先于 `gpt-5.6`、`claude-opus-4-5` 先于
//! `claude-opus-4`），且键与查询统一经 `normalize_model_id` 归一化（lowercase、
//! 剥 `vendor/` 前缀、`.` → `-`），让 `claude-sonnet-4.5`（OpenRouter）与
//! `claude-sonnet-4-5-20250929`（日志）汇合到同一前缀。
//!
//! M9 四层来源，**层级优先，层内最长前缀**（外置 > models.dev > openrouter > 内置）：
//! - 内置：静态快照（取自 cc-switch `model_pricing` 2026-10-03），兜底；
//! - openrouter：同步快照（备份，含显示名）；
//! - models.dev：同步快照（M9 主源，含显示名）；
//! - 外置：`~/.tokenscope/pricing.toml`（用户补充/覆盖，最高优先）。
//!
//! 未收录模型返回 None，由聚合层按 unknown 单独呈现——不得按 0 静默吞掉（M1 不变量 5）。

use std::path::{Path, PathBuf};

use anyhow::Context;

use serde::{Deserialize, Serialize};

use crate::model::TokenCounts;
use crate::openrouter;

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

const TIER_EXTERNAL: u8 = 0;
const TIER_MODELSDEV: u8 = 1;
const TIER_OPENROUTER: u8 = 2;
const TIER_BUILTIN: u8 = 3;

/// 归一化模型标识（键与查询共用同一函数）：lowercase、剥 `vendor/` 前缀、
/// `.` → `-`。变体后缀（`:free` 等）保留参与匹配。
pub fn normalize_model_id(s: &str) -> String {
    let t = s.trim().to_ascii_lowercase();
    let no_vendor = match t.find('/') {
        Some(pos) => &t[pos + 1..],
        None => &t,
    };
    no_vendor.replace('.', "-")
}

#[derive(Debug, Clone)]
struct Entry {
    /// 归一化前缀（匹配用）。
    prefix: String,
    /// 原始写法（GUI 展示用）。
    display: String,
    name: Option<String>,
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
    tier: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct ModelPrice {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

/// GUI 悬浮对照用：该前缀在 OpenRouter 层的价格（无对应模型则 None）。
#[derive(Debug, Clone, Serialize)]
pub struct OpenRouterPrice {
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
    pub name: Option<String>,
}

/// GUI 设置页展示用条目（含来源与显示名）。
#[derive(Debug, Clone, Serialize)]
pub struct PricingEntry {
    pub prefix: String,
    pub name: Option<String>,
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
    pub source: &'static str,
    /// 同前缀 OpenRouter 条目价格；None = OpenRouter 无对应模型。
    pub openrouter: Option<OpenRouterPrice>,
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
pub const PRICING_TEMPLATE: &str = r#"# TokenScope 外置价格表（USD / 百万 token）
# 优先级：本文件 > OpenRouter 同步 > 内置表；同前缀覆盖，最长前缀匹配。
# 模型前缀支持 vendor 写法（会归一化）；修改保存后下一次统计即生效。

[[model]]
prefix = "claude-opus-5"
input = 5.0
output = 25.0
cache_write = 6.25
cache_read = 0.5
"#;

/// 拆出变体后缀：`"hy3:free"` → `("hy3", Some("free"))`；无变体 → `("hy3", None)`。
fn split_variant(normalized: &str) -> (&str, Option<&str>) {
    match normalized.find(':') {
        Some(pos) => (&normalized[..pos], Some(&normalized[pos + 1..])),
        None => (normalized, None),
    }
}

/// 前缀索引：归一化前缀（字节串，避免多字节切片 panic）→ 同前缀全部条目。
/// 查找时只枚举查询串自身的 ~30 个前缀做哈希命中，复杂度与表大小无关。
type PrefixIndex = std::collections::HashMap<Vec<u8>, Vec<Entry>>;

/// 价格索引快照（M11）：四层合并结果的持久化形态。
/// 加载它即可跳过双快照解析与合并（扁平结构，毫秒级）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingIndex {
    /// 三源签名（路径+大小+mtime），用于判断是否需要重建。
    pub sig: String,
    pub synced_at: String,
    pub entries: Vec<IndexEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// 归一化前缀。
    pub prefix: String,
    pub display: String,
    pub name: Option<String>,
    pub tier: u8,
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

pub fn save_index(path: &Path, index: &PricingIndex) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(index)?;
    std::fs::write(path, json).with_context(|| format!("写价格索引失败: {}", path.display()))?;
    Ok(())
}

pub fn load_index(path: &Path) -> anyhow::Result<Option<PricingIndex>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读价格索引失败: {}", path.display()))?;
    serde_json::from_str(&text)
        .map(Some)
        .with_context(|| format!("价格索引解析失败: {}", path.display()))
}

/// 进程内缓存：签名一致则直接复用，避免每次调用重读快照。
static PRICE_CACHE: std::sync::Mutex<Option<(String, std::sync::Arc<Pricing>)>> =
    std::sync::Mutex::new(None);

fn source_sig(paths: &[Option<PathBuf>]) -> String {
    let mut parts = Vec::new();
    for p in paths {
        match p {
            Some(path) => {
                let meta = std::fs::metadata(path).ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime = meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                parts.push(format!("{}:{}:{}", path.display(), size, mtime));
            }
            None => parts.push("-".to_string()),
        }
    }
    parts.join("|")
}

#[derive(Debug, Clone)]
pub struct Pricing {
    by_prefix: PrefixIndex,
}

impl Default for Pricing {
    fn default() -> Self {
        Self::builtin()
    }
}

impl Pricing {
    /// 纯内置表。
    pub fn builtin() -> Self {
        let mut pricing = Self {
            by_prefix: PrefixIndex::new(),
        };
        for (p, i, o, cw, cr) in TABLE {
            pricing.add_entry(Entry {
                prefix: normalize_model_id(p),
                display: (*p).to_string(),
                name: None,
                input: *i,
                output: *o,
                cache_write: *cw,
                cache_read: *cr,
                tier: TIER_BUILTIN,
            });
        }
        pricing
    }

    fn add_entry(&mut self, e: Entry) {
        self.by_prefix
            .entry(e.prefix.as_bytes().to_vec())
            .or_default()
            .push(e);
    }

    fn all_entries(&self) -> impl Iterator<Item = &Entry> {
        self.by_prefix.values().flatten()
    }

    /// 导出索引快照（同步后/重建后写入，供下次启动快速加载）。
    pub fn to_index(&self, sig: String, synced_at: String) -> PricingIndex {
        PricingIndex {
            sig,
            synced_at,
            entries: self
                .all_entries()
                .map(|e| IndexEntry {
                    prefix: e.prefix.clone(),
                    display: e.display.clone(),
                    name: e.name.clone(),
                    tier: e.tier,
                    input: e.input,
                    output: e.output,
                    cache_write: e.cache_write,
                    cache_read: e.cache_read,
                })
                .collect(),
        }
    }

    /// 从索引恢复（扁平结构，跳过双快照解析与合并）。
    pub fn from_index(index: &PricingIndex) -> Self {
        let mut pricing = Self {
            by_prefix: PrefixIndex::new(),
        };
        for e in &index.entries {
            pricing.add_entry(Entry {
                prefix: e.prefix.clone(),
                display: e.display.clone(),
                name: e.name.clone(),
                input: e.input,
                output: e.output,
                cache_write: e.cache_write,
                cache_read: e.cache_read,
                tier: e.tier,
            });
        }
        pricing
    }

    /// 四层合并：内置兜底，openrouter/models.dev 快照叠加，外置最终覆盖。
    /// 各快照文件缺失 → 静默跳过该层；解析失败 → 警告并跳过该层。
    pub fn load(
        external: Option<&Path>,
        modelsdev_snapshot: Option<&Path>,
        openrouter_snapshot: Option<&Path>,
    ) -> (Self, Vec<String>) {
        let mut pricing = Self::builtin();
        let mut warnings = Vec::new();

        if let Some(path) = modelsdev_snapshot {
            match crate::modelsdev::load_snapshot(path) {
                Ok(Some(snapshot)) => {
                    for e in snapshot.entries {
                        pricing.add_entry(Entry {
                            prefix: normalize_model_id(&e.id),
                            display: e.id,
                            name: e.name,
                            input: e.input,
                            output: e.output,
                            cache_write: e.cache_write,
                            cache_read: e.cache_read,
                            tier: TIER_MODELSDEV,
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => warnings.push(format!(
                    "models.dev 快照解析失败，该层已忽略: {}（{e:#}）",
                    path.display()
                )),
            }
        }

        if let Some(path) = openrouter_snapshot {
            match openrouter::load_snapshot(path) {
                Ok(Some(snapshot)) => {
                    for e in snapshot.entries {
                        pricing.add_entry(Entry {
                            prefix: normalize_model_id(&e.id),
                            display: e.id,
                            name: e.name,
                            input: e.prompt * 1_000_000.0,
                            output: e.completion * 1_000_000.0,
                            cache_write: e.cache_write * 1_000_000.0,
                            cache_read: e.cache_read * 1_000_000.0,
                            tier: TIER_OPENROUTER,
                        });
                    }
                }
                Ok(None) => {}
                Err(e) => warnings.push(format!(
                    "OpenRouter 快照解析失败，该层已忽略: {}（{e:#}）",
                    path.display()
                )),
            }
        }

        if let Some(path) = external {
            let Ok(text) = std::fs::read_to_string(path) else {
                return (pricing, warnings);
            };
            let parsed: ExternalFile = match toml::from_str(&text) {
                Ok(p) => p,
                Err(e) => {
                    warnings.push(format!(
                        "外置价格文件解析失败，该层已忽略: {}（{e}）",
                        path.display()
                    ));
                    return (pricing, warnings);
                }
            };
            for m in parsed.model {
                pricing.add_entry(Entry {
                    prefix: normalize_model_id(&m.prefix),
                    display: m.prefix,
                    name: None,
                    input: m.input,
                    output: m.output,
                    cache_write: m.cache_write,
                    cache_read: m.cache_read,
                    tier: TIER_EXTERNAL,
                });
            }
        }

        (pricing, warnings)
    }

    /// 层级优先（外置 > models.dev > openrouter > 内置），层内最长前缀。
    /// **变体隔离**：带 `:变体` 的查询只匹配同变体条目——免费（`:free`）等变体
    /// 不得套用基名价格，宁可 unknown。
    ///
    /// 热路径（每事件一次 × 数千条目）：前缀在加载时已归一化，这里零分配；
    /// 首字节不等直接跳过，把线性扫描的实际比较量压到两位数。
    pub fn lookup(&self, model: &str) -> Option<ModelPrice> {
        let key = normalize_model_id(model);
        let (_, key_variant) = split_variant(&key);
        // 枚举查询串（含变体后缀）自身的前缀（≤30 个）做哈希命中；
        // 变体隔离由显式校验保证：条目变体必须与查询变体完全一致。
        let bytes = key.as_bytes();
        let mut best: Option<(&Entry, (u8, usize))> = None;
        for k in (1..=bytes.len()).rev() {
            if let Some(group) = self.by_prefix.get(&bytes[..k]) {
                for e in group {
                    let (_, variant) = split_variant(&e.prefix);
                    if variant != key_variant {
                        continue;
                    }
                    let rank = (u8::MAX - e.tier, k);
                    if best.is_none_or(|(_, b_rank)| rank > b_rank) {
                        best = Some((e, rank));
                    }
                }
            }
        }
        best.map(|(e, _)| ModelPrice {
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

    /// GUI 设置页条目（合并视图，含来源、显示名与同前缀 OpenRouter 对照价）。
    pub fn entries(&self) -> Vec<PricingEntry> {
        // openrouter 层按归一化前缀索引，供非 openrouter 行对照（精确同前缀）。
        let or_by_prefix: std::collections::HashMap<&str, &Entry> = self
            .all_entries()
            .filter(|e| e.tier == TIER_OPENROUTER)
            .map(|e| (e.prefix.as_str(), e))
            .collect();
        self.all_entries()
            .map(|e| {
                let openrouter = or_by_prefix
                    .get(e.prefix.as_str())
                    .map(|o| OpenRouterPrice {
                        input: o.input,
                        output: o.output,
                        cache_write: o.cache_write,
                        cache_read: o.cache_read,
                        name: o.name.clone(),
                    });
                PricingEntry {
                    prefix: e.display.clone(),
                    name: e.name.clone(),
                    input: e.input,
                    output: e.output,
                    cache_write: e.cache_write,
                    cache_read: e.cache_read,
                    source: match e.tier {
                        TIER_EXTERNAL => "外置",
                        TIER_OPENROUTER => "openrouter",
                        _ => "内置",
                    },
                    openrouter,
                }
            })
            .collect()
    }

    pub fn external_count(&self) -> usize {
        self.count_tier(TIER_EXTERNAL)
    }

    pub fn openrouter_count(&self) -> usize {
        self.count_tier(TIER_OPENROUTER)
    }

    pub fn modelsdev_count(&self) -> usize {
        self.count_tier(TIER_MODELSDEV)
    }

    fn count_tier(&self, tier: u8) -> usize {
        self.all_entries().filter(|e| e.tier == tier).count()
    }

    /// 带进程内缓存的加载（M11）：三源签名一致直接复用，否则重建并写索引文件。
    /// 返回 (Pricing, 警告, 是否命中缓存)。
    pub fn load_cached(
        external: Option<&Path>,
        modelsdev_snapshot: Option<&Path>,
        openrouter_snapshot: Option<&Path>,
        index_path: &Path,
    ) -> (std::sync::Arc<Pricing>, Vec<String>, bool) {
        let paths = [
            external.map(Path::to_path_buf),
            modelsdev_snapshot.map(Path::to_path_buf),
            openrouter_snapshot.map(Path::to_path_buf),
        ];
        let sig = source_sig(&paths);
        let mut guard = PRICE_CACHE.lock().unwrap();
        if let Some((cached_sig, cached)) = guard.as_ref()
            && cached_sig == &sig
        {
            return (cached.clone(), Vec::new(), true);
        }
        let (pricing, mut warnings) = Self::load(external, modelsdev_snapshot, openrouter_snapshot);
        // 重建后写索引快照，供下次进程启动快速加载。
        let index = pricing.to_index(sig.clone(), jiff::Zoned::now().to_string());
        if let Err(e) = save_index(index_path, &index) {
            warnings.push(format!("价格索引写入失败（不影响统计）: {e:#}"));
        }
        let arc = std::sync::Arc::new(pricing);
        *guard = Some((sig, arc.clone()));
        (arc, warnings, false)
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

    fn write(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn test_pricing_normalize() {
        assert_eq!(
            normalize_model_id("Anthropic/Claude-Sonnet-4.5"),
            "claude-sonnet-4-5"
        );
        assert_eq!(
            normalize_model_id("claude-sonnet-4-5-20250929"),
            "claude-sonnet-4-5-20250929"
        );
        assert_eq!(normalize_model_id("Tencent/HY3:free"), "hy3:free");
        assert_eq!(normalize_model_id("GPT-5.6-Sol"), "gpt-5-6-sol");
    }

    #[test]
    fn test_pricing_known_model() {
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
        let p = Pricing::builtin().lookup("gpt-5.6-luna").unwrap();
        assert_eq!(p.input, 0.2);
        assert_eq!(Pricing::builtin().lookup("gpt-5.6-sol").unwrap().input, 4.0);
        assert_eq!(
            Pricing::builtin().lookup("gpt-5.4-nano").unwrap().input,
            0.2
        );
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
    fn test_pricing_normalize_matches_dot_naming() {
        // OpenRouter 点号命名经归一化后命中内置表（反之亦然）。
        assert_eq!(
            Pricing::builtin()
                .lookup("anthropic/claude-sonnet-4.5")
                .unwrap()
                .input,
            3.0
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
        let c = Pricing::builtin()
            .cost("claude-sonnet-4-5", &counts(1_000_000, 1_000_000, 0, 0))
            .unwrap();
        assert!((c - 18.0).abs() < 1e-9);
        let c = Pricing::builtin()
            .cost("claude-sonnet-4-5", &counts(0, 0, 1_000_000, 1_000_000))
            .unwrap();
        assert!((c - 4.05).abs() < 1e-9);
        let c = Pricing::builtin()
            .cost("gpt-5.6-sol", &counts(800, 100, 50, 200))
            .unwrap();
        assert!((c - 6220.0 / 1_000_000.0).abs() < 1e-12);
    }

    #[test]
    fn test_pricing_external_missing_is_silent() {
        let (p, warnings) = Pricing::load(Some(Path::new("Z:/no-such/pricing.toml")), None, None);
        assert_eq!(p.external_count(), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_pricing_external_override_and_append() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-ext-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = write(
            &dir,
            "pricing.toml",
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
        );
        let (p, warnings) = Pricing::load(Some(&path), None, None);
        assert!(warnings.is_empty());
        assert_eq!(p.external_count(), 2);
        assert_eq!(p.lookup("claude-opus-5").unwrap().input, 9.0);
        assert_eq!(p.lookup("my-model/zen-2").unwrap().output, 2.0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_external_broken_falls_back() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-extbad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = write(&dir, "pricing.toml", "not valid toml [[[");
        let (p, warnings) = Pricing::load(Some(&path), None, None);
        assert_eq!(p.external_count(), 0);
        assert_eq!(p.lookup("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("解析失败"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_tier_order() {
        // 三层优先级：外置 > openrouter > 内置；层内最长前缀。
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-tier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let external = write(
            &dir,
            "pricing.toml",
            // 外置与 openrouter 同前缀（claude-sonnet-4-5）：层级优先，外置 42 胜出
            "[[model]]\nprefix = \"claude-sonnet-4-5\"\ninput = 42.0\noutput = 42.0\ncache_write = 0.0\ncache_read = 0.0\n",
        );
        let snapshot = write(
            &dir,
            "pricing-openrouter.json",
            r#"{"synced_at":"t","entries":[
                {"id":"anthropic/claude-sonnet-4.5","name":"Claude Sonnet 4.5",
                 "prompt":0.000003,"completion":0.000015,
                 "cache_read":0.0000003,"cache_write":0.00000375},
                {"id":"openai/gpt-5.2","name":"GPT-5.2",
                 "prompt":0.000007,"completion":0.00005,"cache_read":0,"cache_write":0},
                {"id":"tencent/hy3:free","name":"HY3 free",
                 "prompt":0,"completion":0,"cache_read":0,"cache_write":0}
            ]}"#,
        );
        let (p, warnings) = Pricing::load(Some(&external), None, Some(&snapshot));
        assert!(warnings.is_empty(), "warnings: {warnings:?}");

        // 外置层压过 openrouter（同前缀，层级优先）
        let hit = p.lookup("claude-sonnet-4-5-20250929").unwrap();
        assert_eq!(hit.input, 42.0);
        // openrouter 层生效：点号命名归一化命中，压过同前缀的内置 1.75
        assert_eq!(p.lookup("gpt-5.2-20260101").unwrap().input, 7.0);
        // 内置兜底：openrouter/外置都没有的模型走内置
        assert_eq!(p.lookup("claude-sonnet-4").unwrap().input, 3.0);
        // 免费变体经最长前缀命中 :free 条目 → 0 价（known，非 unknown）
        let free = p.lookup("tencent/hy3:free").unwrap();
        assert_eq!(free.input, 0.0);
        // 条目来源标识与显示名
        let entries = p.entries();
        assert!(
            entries
                .iter()
                .any(|e| e.source == "openrouter" && e.name.as_deref() == Some("Claude Sonnet 4.5"))
        );
        // 悬浮对照：openrouter 行自带对照价；无对应模型的行标注 None
        let sonnet_builtin = entries
            .iter()
            .find(|e| e.source == "内置" && e.prefix == "claude-sonnet-4-5")
            .expect("内置 sonnet-4-5 行应存在");
        let or = sonnet_builtin
            .openrouter
            .as_ref()
            .expect("同前缀 openrouter 条目应挂上对照价");
        assert!((or.input - 3.0).abs() < 1e-9);
        assert_eq!(or.name.as_deref(), Some("Claude Sonnet 4.5"));
        let doubao = entries
            .iter()
            .find(|e| e.source == "内置" && e.prefix == "doubao-seed-2-0")
            .expect("内置 doubao 行应存在");
        assert!(
            doubao.openrouter.is_none(),
            "openrouter 无对应模型 → None（前端显示未知价格）"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_variant_isolation() {
        // :free 变体不得套用基名价格；基名查询也不吃变体条目。
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-var-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snapshot = write(
            &dir,
            "pricing-openrouter.json",
            r#"{"synced_at":"t","entries":[
                {"id":"tencent/hy3","name":"HY3",
                 "prompt":0.0000000825,"completion":0.0000004,"cache_read":0,"cache_write":0},
                {"id":"tencent/hy3:free","name":"HY3 free",
                 "prompt":0,"completion":0,"cache_read":0,"cache_write":0}
            ]}"#,
        );
        let (p, warnings) = Pricing::load(None, None, Some(&snapshot));
        assert!(warnings.is_empty());
        // 精确变体命中 0 价
        assert_eq!(p.lookup("tencent/hy3:free").unwrap().input, 0.0);
        // 基名命中基名价格
        assert!((p.lookup("tencent/hy3").unwrap().input - 0.0825).abs() < 1e-9);
        // 未知变体：基名价格不外溢 → unknown
        assert!(p.lookup("tencent/hy3:preview").is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_pricing_corrupt_snapshot_warns() {
        let dir =
            std::env::temp_dir().join(format!("tokenscope-m5-snapbad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snapshot = write(&dir, "pricing-openrouter.json", "not json");
        let (p, warnings) = Pricing::load(None, None, Some(&snapshot));
        assert_eq!(p.openrouter_count(), 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("OpenRouter"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
