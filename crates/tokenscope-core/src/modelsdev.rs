//! models.dev 价格同步（M9 主源）：拉取 `https://models.dev/api.json` 写入本地
//! 快照 `~/.tokenscope/pricing-modelsdev.json`。
//!
//! 与 openrouter.rs 同构：同步是显式动作，统计管线永不联网只读快照；
//! 快照损坏 → 加载层警告并忽略该层。
//! models.dev 的 cost 单位已是 USD/百万 token（与本项目口径一致，无需换算）。

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const API_URL: &str = "https://models.dev/api.json";

pub fn api_url() -> Result<std::borrow::Cow<'static, str>> {
    crate::acceptance::pricing_url(API_URL, "/modelsdev/api.json")
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiEntry {
    /// 供应商名称。MP03：**有意不读**——provider 名不是模型展示名，不得兜底
    /// 成模型的 `name`（快照里只保留模型级 name）。
    #[serde(default)]
    #[allow(dead_code)]
    name: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, ApiModel>,
}

#[derive(Debug, Default, Deserialize)]
struct ApiModel {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cost: Option<ApiCost>,
}

#[derive(Debug, Default, Deserialize)]
struct ApiCost {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
    /// Task 2：上下文分段价格（只接受 tier.type = "context"）。
    #[serde(default)]
    tiers: Vec<ApiTier>,
    /// Task 2：旧字段兼容——仅在没有等价 tiers 时作为 200K 高档。
    #[serde(default)]
    context_over_200k: Option<ApiTierRates>,
}

/// models.dev 单档四类价格（tiers[] 元素与 context_over_200k 共用形状）。
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ApiTierRates {
    #[serde(default)]
    pub(crate) input: Option<f64>,
    #[serde(default)]
    pub(crate) output: Option<f64>,
    #[serde(default)]
    pub(crate) cache_read: Option<f64>,
    #[serde(default)]
    pub(crate) cache_write: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiTier {
    #[serde(default)]
    input: Option<f64>,
    #[serde(default)]
    output: Option<f64>,
    #[serde(default)]
    cache_read: Option<f64>,
    #[serde(default)]
    cache_write: Option<f64>,
    tier: ApiTierDescriptor,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ApiTierDescriptor {
    #[serde(rename = "type")]
    kind: String,
    size: f64,
}

impl ApiTier {
    fn rates(&self) -> ApiTierRates {
        ApiTierRates {
            input: self.input,
            output: self.output,
            cache_read: self.cache_read,
            cache_write: self.cache_write,
        }
    }
}

/// 快照中的规范分段（Task 2）：`[min_tokens, max_tokens)` 左闭右开，
/// max None = 无上限；models.dev 的 `size = S`（短档覆盖 prompt <= S）
/// 已在同步时转换为 `min = S + 1`，两来源不共用未转换的边界。
/// 分项缺键 = 未知（沿用基础价由计价层负责）；显式 0 = 免费。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotSegment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub min_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// 快照条目：cost 已是 USD/百万 token，直接入库。
/// B3（F04）：分项价格可空——models.dev 的 cost 对象**缺键**表示该分项
/// 未知（如未暴露缓存计价的 provider），**不得**当 0（非零缓存 token 会被
/// 当成免费）；显式 0（如旧版 OpenAI 模型无写入计费）才是真免费。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// `provider/model` 原始 id（展示用）。
    pub id: String,
    pub name: Option<String>,
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// Task 2：上下文分段（v3 快照起写入；v1/v2 旧快照缺省为空 = 仅基础价）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<SnapshotSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// 快照格式版本：v2 起分项可空（缺键=未知）；v3 起带上下文分段
    /// （tiers/context_over_200k）。v1 的缓存分项被 0 填充已损失信息，
    /// 加载时按未知保守处理，等待下一次同步升级；旧快照一律可按
    /// "只有基础价格"离线读取，不伪造分段。
    /// v4（MP03）：`name` 仅来自模型级字段——v1–v3 的 name 可能实为供应商
    /// 名称，加载时清空（价格继续可用），聚合展示名退回原始代表写法。
    #[serde(default = "default_snapshot_v1")]
    pub v: u8,
    pub synced_at: String,
    pub entries: Vec<SnapshotEntry>,
}

fn default_snapshot_v1() -> u8 {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub count: u64,
    pub path: String,
    pub synced_at: String,
    /// 同步告警（结构化，类别键在产生处确定）。同步函数本身不写日志，
    /// 由调用方经 `sync_diag::log_sync_warnings` 统一记录。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<crate::sync_diag::SyncWarning>,
}

/// 拉取 models.dev 并写快照（原子替换；同 provider 串行）。
pub fn sync(snapshot_path: &Path) -> Result<SyncReport> {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    sync_with(snapshot_path, || fetch_modelsdev(snapshot_path))
}

/// Task 7.3：同一 provider 的同步互斥锁（手动/自动两条入口共用）。
pub static SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fetch_modelsdev(_snapshot_path: &Path) -> Result<BTreeMap<String, ApiEntry>> {
    let url = api_url()?;
    #[cfg(feature = "acceptance")]
    crate::acceptance::record_path(
        "pricing_fetch",
        _snapshot_path,
        serde_json::json!({"source":"modelsdev", "url":url}),
    )?;
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .new_agent();
    let mut response = agent
        .get(url.as_ref())
        .header("User-Agent", "tokenscope")
        .call()
        .with_context(|| format!("请求 models.dev 失败: {url}"))?;
    response
        .body_mut()
        .read_json()
        .context("解析 models.dev 响应失败")
}

/// 可注入 fetch 的同步实现（测试 mock，不请求真实服务）。
pub(crate) fn sync_with(
    snapshot_path: &Path,
    fetch: impl FnOnce() -> Result<BTreeMap<String, ApiEntry>>,
) -> Result<SyncReport> {
    let body: BTreeMap<String, ApiEntry> = fetch()?;

    let mut entries: Vec<SnapshotEntry> = Vec::new();
    let mut report_warnings: Vec<crate::sync_diag::SyncWarning> = Vec::new();
    for (provider, entry) in body {
        for (model_id, m) in entry.models {
            let Some(cost) = m.cost else {
                continue; // 无 cost（免费/非 LLM/占位）不入快照
            };
            let (segments, warnings) = convert_tiers(&cost.tiers, cost.context_over_200k.as_ref());
            // 只收集不写日志：调用方经 sync_diag::log_sync_warnings 统一记录
            //（明细文本与改造前的逐条 warn 一致，仅级别降为 debug）。
            for w in warnings {
                report_warnings.push(crate::sync_diag::SyncWarning {
                    category: w.category,
                    model: format!("{provider}/{model_id}"),
                    detail: format!("models.dev {provider}/{model_id}: {}", w.text),
                    for_summary: w.for_summary,
                });
            }
            entries.push(SnapshotEntry {
                id: format!("{provider}/{model_id}"),
                // MP03：name 只取**模型级**字段——provider 名不是模型展示名，
                // 不得兜底（旧实现会在模型无 name 时写入供应商名称）。
                name: m.name,
                input: cost.input,
                output: cost.output,
                cache_read: cost.cache_read,
                cache_write: cost.cache_write,
                segments,
            });
        }
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| {
        // Task 2A：跨 provider 的同名模型保留为独立候选（渠道元数据在
        // 原始键中，估算按请求条件取最高费用），只去重完全相同的 id。
        a.id == b.id
    });

    let synced_at = jiff::Zoned::now().to_string();
    let count = entries.len() as u64;
    let snapshot = Snapshot {
        v: 4,
        synced_at: synced_at.clone(),
        entries,
    };
    if let Some(dir) = snapshot_path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(&snapshot)?;
    crate::fsutil::atomic_write(snapshot_path, json.as_bytes())
        .with_context(|| format!("写快照失败: {}", snapshot_path.display()))?;
    Ok(SyncReport {
        count,
        path: snapshot_path.display().to_string(),
        synced_at,
        warnings: report_warnings,
    })
}

/// convert_tiers 的单条档位告警：类别键在此（告警产生地）确定。
#[derive(Debug, Clone)]
pub(crate) struct TierWarning {
    pub category: &'static str,
    pub text: String,
    pub for_summary: bool,
}

/// 不变量 8 的兼容判定：旧字段（over）已给出的每一项单价，都必须在
/// `first`（分界点最小的 context 档）有对应项且完全相等；未给出的项不参与。
fn given_rates_match(over: &ApiTierRates, first: &ApiTierRates) -> bool {
    [
        (over.input, first.input),
        (over.output, first.output),
        (over.cache_read, first.cache_read),
        (over.cache_write, first.cache_write),
    ]
    .into_iter()
    .all(|(o, f)| match o {
        Some(ov) => f.is_some_and(|fv| fv == ov),
        None => true,
    })
}

/// models.dev tiers/context_over_200k → 规范分段（Task 2）。
///
/// 规则：
/// - 只接受 `tier.type = "context"`，其他类型忽略并留 warning；
/// - size 按升序连续成档：基础价覆盖到第一个 size，tier 价格从
///   `size + 1` 开始直到下一个 tier 的 `size`（含），最后一档无上限；
/// - 同一 size 重复时保留确定性的一项（排序后首个）并留 warning；
/// - `context_over_200k` 仅在没有等价 tiers（无 size=200000 档）时生效，
///   冲突时优先 tiers 并留 warning，不重复计价；
/// - 非法 size（负数/非整数）跳过并留 warning；
/// - 缺失价格字段保留 None，显式 0 保持免费。
pub(crate) fn convert_tiers(
    tiers: &[ApiTier],
    over200k: Option<&ApiTierRates>,
) -> (Vec<SnapshotSegment>, Vec<TierWarning>) {
    let mut warnings: Vec<TierWarning> = Vec::new();
    let mut sized: Vec<(u64, ApiTierRates)> = Vec::new();
    for t in tiers {
        if t.tier.kind != "context" {
            warnings.push(TierWarning {
                category: "non_context_tier",
                text: format!("忽略非 context 类型 tier（type={}）", t.tier.kind),
                for_summary: true,
            });
            continue;
        }
        let s = t.tier.size;
        if !s.is_finite() || s < 0.0 || s.fract() != 0.0 {
            warnings.push(TierWarning {
                category: "invalid_tier_size",
                text: format!("tier size 非法（{s}），该档跳过"),
                for_summary: true,
            });
            continue;
        }
        sized.push((s as u64, t.rates()));
    }
    sized.sort_by_key(|(s, _)| *s); // 稳定排序：同 size 保留 API 顺序首个
    let mut deduped: Vec<(u64, ApiTierRates)> = Vec::new();
    for (s, rates) in sized {
        if deduped.last().is_some_and(|(ps, _)| *ps == s) {
            warnings.push(TierWarning {
                category: "duplicate_tier_size",
                text: format!("重复 tier size {s}，保留首个"),
                for_summary: true,
            });
            continue;
        }
        deduped.push((s, rates));
    }
    if let Some(over) = over200k {
        if deduped.is_empty() {
            deduped.push((200_000, over.clone()));
        } else if !deduped.iter().any(|(s, _)| *s == 200_000) {
            // 不变量 8：上游为兼容旧客户端把档位价同时填进 context_over_200k。
            // 只比较旧字段已给出的单价项：与分界点最小的 context 档（deduped
            // 已升序）逐项完全相等 → 兼容字段，仅 debug 明细；任一项不等 →
            // 真冲突。两种情况的计价都只用 tiers，分段结果相同。浮点用 ==
            //：两侧来自上游同一十进制字面量，不引入容差。
            let first = &deduped[0].1;
            let compatible = given_rates_match(over, first);
            if compatible {
                warnings.push(TierWarning {
                    category: "over200k_compatible",
                    text: "context_over_200k 为兼容旧客户端的重复字段（单价与最小分界档一致），按 tiers 计价"
                        .to_string(),
                    for_summary: false,
                });
            } else {
                warnings.push(TierWarning {
                    category: "over200k_conflict",
                    text: "context_over_200k 与 tiers 冲突，优先 tiers".to_string(),
                    for_summary: true,
                });
            }
        }
        // 等价档（size=200000）已存在时静默去重，不重复计价。
    }
    let segments = deduped
        .iter()
        .enumerate()
        .map(|(i, (s, r))| SnapshotSegment {
            label: Some(format!(">{s}")),
            min_tokens: s + 1,
            max_tokens: deduped.get(i + 1).map(|(ns, _)| ns + 1),
            input: r.input,
            output: r.output,
            cache_read: r.cache_read,
            cache_write: r.cache_write,
        })
        .collect();
    (segments, warnings)
}

/// 读快照：缺失 → Ok(None)；损坏 → Err（调用方警告并忽略该层）。
pub fn load_snapshot(snapshot_path: &Path) -> Result<Option<Snapshot>> {
    if !snapshot_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(snapshot_path)
        .with_context(|| format!("读快照失败: {}", snapshot_path.display()))?;
    parse_snapshot_text(&text)
        .map(Some)
        .with_context(|| format!("快照解析失败: {}", snapshot_path.display()))
}

/// SF03：解析已读入内存的快照文本——读取与健康分类由调用方（pricing）
/// 统一负责；错误只含解析原因，路径上下文由调用方补充。
/// v1 旧快照（分项被 0 填充）→ 缓存分项降级为未知（保守），建议重新同步。
pub(crate) fn parse_snapshot_text(text: &str) -> Result<Snapshot> {
    let mut snapshot: Snapshot = serde_json::from_str(text)?;
    if snapshot.v < 2 {
        log::warn!(
            "models.dev 快照为 v1 格式（缓存分项曾被 0 填充），按未知保守处理；重新同步后恢复精确"
        );
        for e in &mut snapshot.entries {
            // 非零值是真实价格（price() 只把缺失键填 0），保留；
            // 0 无法区分真免费与 0 填充 → 按未知保守处理。v2 严格区分。
            e.cache_read = e.cache_read.filter(|v| *v != 0.0);
            e.cache_write = e.cache_write.filter(|v| *v != 0.0);
        }
        snapshot.v = 2;
    }
    // MP03：v4 之前的 name 可能实为供应商名称（旧同步把 provider name 兜底成
    // 模型名）——价格继续可用，但来源不明的 name 不得作为聚合展示名。
    if snapshot.v < 4 {
        for e in &mut snapshot.entries {
            e.name = None;
        }
    }
    Ok(snapshot)
}

/// 测试专用（`#[doc(hidden)]`）：以固定 JSON 响应体跑一次同步，不联网。
#[doc(hidden)]
pub fn sync_with_body_for_tests(snapshot_path: &Path, body: &str) -> Result<SyncReport> {
    let parsed: BTreeMap<String, ApiEntry> = serde_json::from_str(body)?;
    sync_with(snapshot_path, || Ok(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modelsdev_parse_fixture() {
        let body = r#"{
            "volcengine": {"name": "Volcengine", "models": {
                "doubao-seed-2-0-pro-260215": {"name": "Doubao Seed 2.0 Pro",
                    "cost": {"input": 0.47, "output": 2.37, "cache_read": 0.09, "cache_write": 0.0}}
            }},
            "anthropic": {"models": {
                "claude-sonnet-4-5": {"name": "Claude Sonnet 4.5",
                    "cost": {"input": 3, "output": 15, "cache_read": 0.3, "cache_write": 3.75}},
                "free-model": {"name": "Free"}
            }}
        }"#;
        let api: BTreeMap<String, ApiEntry> = serde_json::from_str(body).unwrap();
        let entries: Vec<SnapshotEntry> = api
            .into_iter()
            .flat_map(|(provider, entry)| {
                entry.models.into_iter().filter_map(move |(id, m)| {
                    m.cost.map(|c| {
                        let (segments, _) = convert_tiers(&c.tiers, c.context_over_200k.as_ref());
                        SnapshotEntry {
                            id: format!("{provider}/{id}"),
                            name: m.name,
                            input: c.input,
                            output: c.output,
                            cache_read: c.cache_read,
                            cache_write: c.cache_write,
                            segments,
                        }
                    })
                })
            })
            .collect();
        assert_eq!(entries.len(), 2, "无 cost 的模型应被跳过");
        let doubao = entries
            .iter()
            .find(|e| e.id.ends_with("doubao-seed-2-0-pro-260215"))
            .unwrap();
        assert!(
            doubao.input.is_some_and(|v| (v - 0.47).abs() < 1e-9),
            "models.dev 单位已是 USD/Mtok"
        );
        let sonnet = entries
            .iter()
            .find(|e| e.id == "anthropic/claude-sonnet-4-5")
            .unwrap();
        assert!(sonnet.output.is_some_and(|v| (v - 15.0).abs() < 1e-9));
    }

    #[test]
    fn test_modelsdev_tier() {
        // Task 2：真实响应形状——tiers（context 类型）与 context_over_200k
        // 必须保留到快照并参与计价；models.dev 语义 size = S 表示短档覆盖
        // prompt <= S，高档从 S+1 开始（272000 基础档、272001 高档）。
        let dir = std::env::temp_dir().join(format!("tokenscope-tier-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        let body = r#"{
            "openai": {"models": {
                "gpt-5.6": {"name": "GPT-5.6", "cost": {
                    "input": 4, "output": 20, "cache_read": 0.4, "cache_write": 5,
                    "tiers": [{
                        "input": 8, "output": 30, "cache_read": 0.8, "cache_write": 10,
                        "tier": {"type": "context", "size": 272000}
                    }],
                    "context_over_200k": {"input": 8, "output": 30, "cache_read": 0.8, "cache_write": 10}
                }}
            }}
        }"#;
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<BTreeMap<String, ApiEntry>>(body).unwrap())
        })
        .unwrap();
        assert_eq!(report.count, 1);
        let snapshot = load_snapshot(&path).unwrap().unwrap();
        let e = &snapshot.entries[0];
        assert_eq!(e.id, "openai/gpt-5.6");
        assert_eq!(
            e.segments.len(),
            1,
            "tiers 与 context_over_200k 等价 → 去重为一档"
        );
        let seg = &e.segments[0];
        assert_eq!(seg.min_tokens, 272_001, "size 272000 → 高档从 272001 开始");
        assert_eq!(seg.max_tokens, None);
        assert_eq!(seg.input, Some(8.0));
        assert_eq!(seg.cache_read, Some(0.8));
        // 端到端：快照 → Pricing → 272000 基础档 / 272001 高档。
        let (p, warnings) = crate::pricing::Pricing::load(None, Some(&path), None);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        let tc = crate::model::TokenCounts {
            input: 272_000,
            output: 0,
            cache_write: 0,
            cache_read: 0,
        };
        let est = p
            .estimate(
                "openai/gpt-5.6",
                &tc,
                "2026-01-05T10:00:00Z".parse().unwrap(),
            )
            .unwrap();
        assert!(
            (est.cost - 272_000.0 * 4.0 / 1e6).abs() < 1e-9,
            "272000 仍命中基础档"
        );
        let tc = crate::model::TokenCounts {
            input: 272_001,
            output: 0,
            cache_write: 0,
            cache_read: 0,
        };
        let est = p
            .estimate(
                "openai/gpt-5.6",
                &tc,
                "2026-01-05T10:00:00Z".parse().unwrap(),
            )
            .unwrap();
        assert!(
            (est.cost - 272_001.0 * 8.0 / 1e6).abs() < 1e-9,
            "272001 命中高档"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_modelsdev_tier_conversion_rules() {
        // 多个 size 升序连续成档；基础价覆盖到第一个 size。
        let tiers = vec![
            tier_of("context", 272000.0, 8.0),
            tier_of("context", 100000.0, 6.0),
        ];
        let (segments, warnings) = convert_tiers(&tiers, None);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].min_tokens, 100_001);
        assert_eq!(segments[0].max_tokens, Some(272_001));
        assert_eq!(segments[1].min_tokens, 272_001);
        assert_eq!(segments[1].max_tokens, None);

        // 非 context 类型被忽略并留下 warning。
        let tiers = vec![tier_of("time", 100000.0, 6.0)];
        let (segments, warnings) = convert_tiers(&tiers, None);
        assert!(segments.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].text.contains("time"));

        // 重复 size：保留第一个，其余 warning。
        let tiers = vec![
            tier_of("context", 272000.0, 8.0),
            tier_of("context", 272000.0, 9.0),
        ];
        let (segments, warnings) = convert_tiers(&tiers, None);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].input, Some(8.0), "保留首个");
        assert_eq!(warnings.len(), 1);

        // context_over_200k 仅在没有等价 tiers 时生效。
        let over = ApiTierRates {
            input: Some(8.0),
            output: Some(30.0),
            cache_read: Some(0.8),
            cache_write: Some(10.0),
        };
        let (segments, warnings) = convert_tiers(&[], Some(&over));
        assert!(warnings.is_empty());
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].min_tokens, 200_001);
        assert_eq!(segments[0].max_tokens, None);
        // tiers 存在但无 200000 档 → 冲突，优先 tiers 并 warning。
        let tiers = vec![tier_of("context", 100000.0, 6.0)];
        let (segments, warnings) = convert_tiers(&tiers, Some(&over));
        assert_eq!(segments.len(), 1, "只用 tiers，不重复计价");
        assert_eq!(segments[0].min_tokens, 100_001);
        assert_eq!(warnings.len(), 1, "冲突必须留 warning");

        // 非法 size（非整数/负数）跳过并 warning。
        let tiers = vec![
            tier_of("context", 272000.5, 8.0),
            tier_of("context", -1.0, 9.0),
        ];
        let (segments, warnings) = convert_tiers(&tiers, None);
        assert!(segments.is_empty());
        assert_eq!(warnings.len(), 2);
    }

    /// 构造单档 tier fixture（input 单价即可区分档位）。
    fn tier_of(kind: &str, size: f64, input: f64) -> ApiTier {
        serde_json::from_value(serde_json::json!({
            "input": input, "output": 30.0, "cache_read": 0.8, "cache_write": 10.0,
            "tier": {"type": kind, "size": size}
        }))
        .unwrap()
    }

    #[test]
    fn test_modelsdev_snapshot_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m9-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        let snapshot = Snapshot {
            v: 2,
            synced_at: "2026-10-04T00:00:00+08:00".into(),
            entries: vec![SnapshotEntry {
                id: "volcengine/doubao-seed-2-0-pro-260215".into(),
                name: Some("Doubao Seed 2.0 Pro".into()),
                input: Some(0.47),
                output: Some(2.37),
                cache_read: Some(0.09),
                cache_write: Some(0.0),
                segments: Vec::new(),
            }],
        };
        std::fs::write(&path, serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(
            loaded.entries[0].id,
            "volcengine/doubao-seed-2-0-pro-260215"
        );
        assert_eq!(loaded.v, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_modelsdev_v1_preserves_nonzero_cache_prices() {
        // Task 3：v1 的非零缓存价是真实数据（price() 只把缺失填 0），必须保留。
        let dir = std::env::temp_dir().join(format!("tokenscope-t3-nz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        std::fs::write(
            &path,
            r#"{"synced_at":"t","entries":[
                {"id":"prov/x","name":null,"input":1.0,"output":2.0,"cache_read":0.09,"cache_write":3.75}
            ]}"#,
        )
        .unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(
            loaded.entries[0].cache_read,
            Some(0.09),
            "非零缓存读必须保留"
        );
        assert_eq!(
            loaded.entries[0].cache_write,
            Some(3.75),
            "非零缓存写必须保留"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_modelsdev_v1_zero_cache_prices_become_unknown() {
        // Task 3：v1 的 0 无法区分真免费与 0 填充 → 按未知保守处理。
        let dir = std::env::temp_dir().join(format!("tokenscope-t3-z-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        std::fs::write(
            &path,
            r#"{"synced_at":"t","entries":[
                {"id":"prov/x","name":null,"input":1.0,"output":2.0,"cache_read":0.0,"cache_write":0.0}
            ]}"#,
        )
        .unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded.entries[0].cache_read, None);
        assert_eq!(loaded.entries[0].cache_write, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_modelsdev_v2_keeps_explicit_zero_prices() {
        // Task 3：v2 严格区分——显式 Some(0.0) 是真免费，不降级。
        let dir = std::env::temp_dir().join(format!("tokenscope-t3-v2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        std::fs::write(
            &path,
            r#"{"v":2,"synced_at":"t","entries":[
                {"id":"prov/x","name":null,"input":1.0,"output":2.0,"cache_read":0.0,"cache_write":0.0}
            ]}"#,
        )
        .unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded.entries[0].cache_read, Some(0.0));
        assert_eq!(loaded.entries[0].cache_write, Some(0.0));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_modelsdev_v1_legacy_cache_unknown() {
        // B3：v1 快照的分项被 0 填充、信息已损失——加载时缓存分项按未知
        // 保守处理（不冒充显式 0），重新同步后恢复精确。
        let dir = std::env::temp_dir().join(format!("tokenscope-b3-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        // v1 格式：无 v 字段，cache_read/cache_write 恒为数值 0.0
        std::fs::write(
            &path,
            r#"{"synced_at":"t","entries":[
                {"id":"prov/x","name":null,"input":1.0,"output":2.0,"cache_read":0.0,"cache_write":0.0}
            ]}"#,
        )
        .unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded.entries[0].input, Some(1.0));
        assert_eq!(loaded.entries[0].cache_read, None, "v1 缓存分项按未知");
        assert_eq!(loaded.entries[0].cache_write, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_sync_provider_retry_independent() {
        // Task 7.3：单源失败不覆盖另一源旧快照；两 provider 互不阻塞
        //（各自独立锁 + 独立 fetch mock，不请求真实服务）。
        let dir = std::env::temp_dir().join(format!("tokenscope-t73-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md_path = dir.join("pricing-modelsdev.json");
        let or_path = dir.join("pricing-openrouter.json");
        // 旧快照在位
        std::fs::write(&md_path, r#"{"v":2,"synced_at":"old","entries":[]}"#).unwrap();
        std::fs::write(&or_path, r#"{"synced_at":"old","entries":[]}"#).unwrap();

        // models.dev 失败（网络错）+ OpenRouter 成功
        let md_err = sync_with(&md_path, || Err(anyhow::anyhow!("network down")));
        assert!(md_err.is_err());
        let or_ok = crate::openrouter::sync_with(&or_path, || {
            Ok(serde_json::from_str(
                r#"{"data":[{"id":"prov/ok","pricing":{"prompt":"0.001","completion":"0.002"}}]}"#,
            )
            .unwrap())
        });
        assert!(or_ok.is_ok(), "models.dev 失败不得拖垮 OpenRouter");

        // models.dev 旧快照原样保留；OpenRouter 已更新
        let md = load_snapshot(&md_path).unwrap().unwrap();
        assert_eq!(md.synced_at, "old");
        let or = crate::openrouter::load_snapshot(&or_path).unwrap().unwrap();
        assert_ne!(or.synced_at, "old");

        // 反向：OpenRouter 失败不影响 models.dev 成功写入
        let or_err =
            crate::openrouter::sync_with(&or_path, || Err(anyhow::anyhow!("network down")));
        assert!(or_err.is_err());
        let md_ok = sync_with(&md_path, || {
            Ok(serde_json::from_str(
                r#"{"anthropic":{"models":{"claude-x":{"name":null,"cost":{"input":1.0,"output":2.0}}}}}"#,
            )
            .unwrap())
        });
        assert!(md_ok.is_ok(), "OpenRouter 失败不得拖垮 models.dev");
        let md2 = load_snapshot(&md_path).unwrap().unwrap();
        assert_ne!(md2.synced_at, "old");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_modelsdev_missing_silent_and_corrupt_err() {
        assert!(
            load_snapshot(Path::new("Z:/no-such/modelsdev.json"))
                .unwrap()
                .is_none()
        );
        let dir = std::env::temp_dir().join(format!("tokenscope-m9-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_snapshot(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── 同步告警汇总（2026-10-22 计划）──────────────────────────

    /// golden 基准的合成响应（与基准生成器一致）：兼容/冲突/多档/普通模型。
    fn golden_body() -> String {
        r#"{
  "prov-a": {"models": {
    "plain": {"name": "Plain", "cost": {"input": 1.5, "output": 6.0, "cache_read": 0.15, "cache_write": 1.875}},
    "compat": {"name": "Compat", "cost": {"input": 4, "output": 20, "cache_read": 0.4, "cache_write": 5,
        "tiers": [{"input": 8, "output": 30, "cache_read": 0.8, "cache_write": 10, "tier": {"type": "context", "size": 272000}}],
        "context_over_200k": {"input": 8, "output": 30, "cache_read": 0.8, "cache_write": 10}}},
    "conflict": {"name": "Conflict", "cost": {"input": 1, "output": 2,
        "tiers": [{"input": 8, "output": 30, "tier": {"type": "context", "size": 272000}}],
        "context_over_200k": {"input": 9, "output": 30}}},
    "multitier": {"name": "Multi", "cost": {"input": 1, "output": 2,
        "tiers": [{"input": 8, "output": 30, "tier": {"type": "context", "size": 272000}},
                  {"input": 6, "output": 25, "tier": {"type": "context", "size": 100000}},
                  {"input": 7, "output": 26, "tier": {"type": "time", "size": 50000}}]}}
  }}
}"#
        .to_string()
    }

    /// 快照里随同步时刻变化的 synced_at 归一为占位符，其余字节保持原样。
    fn normalize_synced_at(raw: &[u8]) -> Vec<u8> {
        let text = String::from_utf8(raw.to_vec()).expect("快照必须是 UTF-8");
        let marker = "\"synced_at\": \"";
        let start = text.find(marker).expect("快照缺 synced_at") + marker.len();
        let end = start + text[start..].find('"').expect("synced_at 值未闭合");
        format!("{}SYNCED_AT{}", &text[..start], &text[end..]).into_bytes()
    }

    /// 不变量 7：同一份上游响应，改造前后快照逐字节相同（synced_at 除外）。
    /// 基准 fixture 由改造前实现生成后固定，本测试不得改动基准。
    #[test]
    fn golden_snapshot_bytes_unchanged() {
        let dir = std::env::temp_dir().join(format!("tokenscope-golden-md-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        let report = sync_with_body_for_tests(&path, &golden_body()).unwrap();
        assert_eq!(report.count, 4);
        let actual = normalize_synced_at(&std::fs::read(&path).unwrap());
        let expected = std::fs::read("tests/fixtures/modelsdev/snapshot-golden.json")
            .expect("golden 基准 fixture 缺失");
        assert_eq!(
            String::from_utf8_lossy(&actual),
            String::from_utf8_lossy(&expected),
            "快照字节与改造前基准不一致"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// LA01：sync_with 用含冲突与重复档位的合成响应时，SyncReport 带回
    /// 结构化告警（类别键 + 模型 id + 原有文本）。
    #[test]
    fn sync_report_carries_structured_warnings() {
        let dir = std::env::temp_dir().join(format!("tokenscope-warn-md-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-modelsdev.json");
        let report = sync_with_body_for_tests(&path, &golden_body()).unwrap();
        let find = |model: &str| {
            report
                .warnings
                .iter()
                .find(|w| w.model == model)
                .unwrap_or_else(|| panic!("告警缺 {model}: {:?}", report.warnings))
        };
        // 兼容（prov-a/compat）：不再是冲突，是仅 debug 的兼容明细。
        let compat = find("prov-a/compat");
        assert_eq!(compat.category, "over200k_compatible", "{compat:?}");
        assert!(!compat.for_summary, "兼容明细不进入 WARN 汇总: {compat:?}");
        // 真冲突（prov-a/conflict）：类别与原文本保留。
        let conflict = find("prov-a/conflict");
        assert_eq!(conflict.category, "over200k_conflict", "{conflict:?}");
        assert!(conflict.for_summary);
        assert_eq!(
            conflict.detail,
            "models.dev prov-a/conflict: context_over_200k 与 tiers 冲突，优先 tiers",
            "明细文本与改造前一致"
        );
        // 非 context 类型（prov-a/multitier）。
        let non_context = find("prov-a/multitier");
        assert_eq!(non_context.category, "non_context_tier", "{non_context:?}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 不变量 8：旧字段单价与最小分界档完全相等 → 兼容（仅 debug 明细），
    /// 不再误报冲突；segments 与改造前相同。
    #[test]
    fn over200k_matching_first_tier_is_compatible_not_conflict() {
        let over = ApiTierRates {
            input: Some(8.0),
            output: Some(30.0),
            cache_read: Some(0.8),
            cache_write: Some(10.0),
        };
        let tiers = vec![tier_of("context", 272000.0, 8.0)];
        let (segments, warnings) = convert_tiers(&tiers, Some(&over));
        assert!(
            warnings.iter().all(|w| w.category != "over200k_conflict"),
            "完全相等不得报冲突: {warnings:?}"
        );
        assert_eq!(warnings.len(), 1, "应有且只有一条兼容明细: {warnings:?}");
        assert_eq!(warnings[0].category, "over200k_compatible");
        assert!(!warnings[0].for_summary, "兼容只写 debug 明细");
        assert_eq!(segments.len(), 1, "计价仍只用 tiers");
        assert_eq!(segments[0].min_tokens, 272_001);
    }

    /// 不变量 8：旧字段只给出部分单价时，只比较已给出的项。
    #[test]
    fn over200k_partial_rates_only_compare_given_items() {
        let over = ApiTierRates {
            input: Some(8.0),
            output: Some(30.0),
            cache_read: None,
            cache_write: None,
        };
        // 首档 cache 分项存在且不同——旧字段未给出的项不参与比较，仍兼容。
        let tiers = vec![ApiTier {
            input: Some(8.0),
            output: Some(30.0),
            cache_read: Some(0.9),
            cache_write: Some(11.0),
            tier: serde_json::from_value(serde_json::json!({
                "type": "context", "size": 272000
            }))
            .unwrap(),
        }];
        let (segments, warnings) = convert_tiers(&tiers, Some(&over));
        assert!(
            warnings.iter().all(|w| w.category != "over200k_conflict"),
            "未给出的项不参与比较: {warnings:?}"
        );
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].category, "over200k_compatible");
        assert_eq!(segments[0].cache_read, Some(0.9), "计价只用 tiers");
    }

    /// 不变量 8：任一项不等 → 真冲突，保留原文本。
    #[test]
    fn over200k_price_mismatch_is_real_conflict() {
        let over = ApiTierRates {
            input: Some(9.0),
            output: Some(30.0),
            cache_read: Some(0.8),
            cache_write: Some(10.0),
        };
        let tiers = vec![tier_of("context", 272000.0, 8.0)];
        let (segments, warnings) = convert_tiers(&tiers, Some(&over));
        assert_eq!(warnings.len(), 1, "真冲突必须留 warning: {warnings:?}");
        assert_eq!(warnings[0].category, "over200k_conflict");
        assert!(warnings[0].for_summary);
        assert_eq!(
            warnings[0].text, "context_over_200k 与 tiers 冲突，优先 tiers",
            "冲突文本与改造前一致"
        );
        assert_eq!(segments.len(), 1);
        assert_eq!(
            segments[0].min_tokens, 272_001,
            "计价只用 tiers，与改造前相同"
        );
    }

    /// 不变量 8：旧字段给出的项在最小分界档缺失 → 不相等，算真冲突。
    #[test]
    fn over200k_rate_missing_in_first_tier_is_conflict() {
        let over = ApiTierRates {
            input: Some(8.0),
            output: None,
            cache_read: None,
            cache_write: None,
        };
        // 首档 input 缺失（None）——旧字段给出了 input，无法判等 → 冲突。
        let tiers = vec![ApiTier {
            input: None,
            output: Some(30.0),
            cache_read: None,
            cache_write: None,
            tier: serde_json::from_value(serde_json::json!({
                "type": "context", "size": 100000
            }))
            .unwrap(),
        }];
        let (segments, warnings) = convert_tiers(&tiers, Some(&over));
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(warnings[0].category, "over200k_conflict");
        assert_eq!(segments[0].min_tokens, 100_001);
    }
}
