//! OpenRouter 价格同步（M5 主源）：拉取 `https://openrouter.ai/api/v1/models`
//! 写入本地快照 `~/.tokenscope/pricing-openrouter.json`。
//!
//! **同步是显式动作，统计永不联网**：summary 只读快照文件；同步失败 → 显式
//! 报错（GUI/CLI），已有快照保持不动；快照损坏 → 加载层警告并忽略。

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const API_URL: &str = "https://openrouter.ai/api/v1/models";

#[derive(Debug, Deserialize)]
pub(crate) struct ApiModel {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    pricing: Option<ApiPricing>,
}

#[derive(Debug, Default, Deserialize)]
struct ApiPricing {
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
    #[serde(default)]
    input_cache_read: Option<String>,
    #[serde(default)]
    input_cache_write: Option<String>,
    /// Task 3：条件价格覆盖（长上下文等）。时间条件条目（utc_start/utc_end）
    /// 在 Task 4A 落地前跳过并留 warning，不猜测其适用条件。
    #[serde(default)]
    overrides: Vec<ApiOverride>,
}

/// 条件价格覆盖条目：价格字段内联（USD/token 串），条件字段可选。
#[derive(Debug, Default, Deserialize)]
struct ApiOverride {
    #[serde(default)]
    min_prompt_tokens: Option<f64>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    completion: Option<String>,
    #[serde(default)]
    input_cache_read: Option<String>,
    #[serde(default)]
    input_cache_write: Option<String>,
    #[serde(default)]
    utc_start: Option<String>,
    #[serde(default)]
    utc_end: Option<String>,
}

/// 快照中的条件价格档（Task 3）：USD/token 原单位，导入计价层 ×1e6。
/// 上游语义：prompt tokens >= min_prompt_tokens 即命中（inclusive 下界）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotOverride {
    pub min_prompt_tokens: u64,
    #[serde(default)]
    pub prompt: Option<f64>,
    #[serde(default)]
    pub completion: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

/// 快照条目：只保留计价所需字段（单位：USD / token）。
/// Task 7.4：prompt/completion 缺价保留为 None（未知 ≠ 0）；
/// 负价条目在同步时拒绝（脏数据不入快照）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub id: String,
    pub name: Option<String>,
    pub prompt: Option<f64>,
    pub completion: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    /// Task 3：条件价格档（升序、去重后）；v2 快照起写入，旧快照缺省为空。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<SnapshotOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// 快照格式版本：v2 起带条件价格档（overrides）；旧快照缺 v 字段
    /// 按 v1 读取（仅基础价，兼容不伪造分段）。
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
    /// 同步丢弃诊断（F04：整条拒绝的模型与分项随报告携带，调用方沿
    /// 既有日志路径记录）。
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Task 7.4：解析价格串；None 表示字段缺失（未知），非负数值才有效。
/// 返回 Err 表示负价（脏数据，由调用方拒绝该条目）。
fn parse_price(v: &Option<String>) -> Result<Option<f64>, String> {
    let Some(s) = v else {
        return Ok(None);
    };
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let n: f64 = s.parse().map_err(|e| format!("价格串非法 {s:?}: {e}"))?;
    // R06：API 可能返回 "NaN"/"Infinity"/"-Infinity"——parse 成功但非法，
    // 必须在此拒绝，避免脏值进入快照（快照字段是数值，无法二次拦截）。
    if !n.is_finite() || n < 0.0 {
        return Err(format!("价格非法（须为有限非负）: {n}"));
    }
    Ok(Some(n))
}

/// 拉取 OpenRouter 模型清单并写快照（原子替换；同 provider 串行）。
pub fn sync(snapshot_path: &Path) -> Result<SyncReport> {
    let _guard = SYNC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    sync_with(snapshot_path, fetch_openrouter)
}

/// Task 7.3：同一 provider 的同步互斥锁（手动/自动两条入口共用）。
pub static SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn fetch_openrouter() -> Result<ApiResponse> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent();
    let mut response = agent
        .get(API_URL)
        .header("User-Agent", "tokenscope")
        .call()
        .with_context(|| format!("请求 OpenRouter 失败: {API_URL}"))?;
    response
        .body_mut()
        .read_json()
        .context("解析 OpenRouter 响应失败")
}

/// 可注入 fetch 的同步实现（测试 mock，不请求真实服务）。
pub(crate) fn sync_with(
    snapshot_path: &Path,
    fetch: impl FnOnce() -> Result<ApiResponse>,
) -> Result<SyncReport> {
    let body: ApiResponse = fetch()?;

    let mut rejected = 0u64;
    let mut warnings: Vec<String> = Vec::new();
    let mut entries: Vec<SnapshotEntry> = Vec::new();
    for m in body.data {
        let p = m.pricing.unwrap_or_default();
        // F04：数值预检先于任何结构性丢弃——原始候选（基础四价 + 每个
        // override 的四价）任一显式单价非法（NaN/inf/负）→ **整个模型
        // 拒绝**，不得丢弃脏 override 后让模型按基础价完整计费。诊断沿
        // 既有告警路径（含模型与分项），随 SyncReport.warnings 携带。
        let mut invalid: Vec<String> = Vec::new();
        for (comp, raw) in [
            ("prompt", &p.prompt),
            ("completion", &p.completion),
            ("input_cache_read", &p.input_cache_read),
            ("input_cache_write", &p.input_cache_write),
        ] {
            if let Err(reason) = parse_price(raw) {
                invalid.push(format!("基础 {comp}: {reason}"));
            }
        }
        for o in &p.overrides {
            for (comp, raw) in [
                ("prompt", &o.prompt),
                ("completion", &o.completion),
                ("input_cache_read", &o.input_cache_read),
                ("input_cache_write", &o.input_cache_write),
            ] {
                if let Err(reason) = parse_price(raw) {
                    invalid.push(format!(
                        "override(min_prompt_tokens={:?}) {comp}: {reason}",
                        o.min_prompt_tokens
                    ));
                }
            }
        }
        if !invalid.is_empty() {
            rejected += 1;
            warnings.push(format!(
                "{}: 存在非法单价，整条模型已拒绝（{}）",
                m.id,
                invalid.join("; ")
            ));
            continue;
        }
        // Task 3：条件价格 overrides → 快照档（升序、同阈值去重、脏数据跳过）。
        let mut overrides: Vec<SnapshotOverride> = Vec::new();
        for o in &p.overrides {
            let mut skip = |reason: String| {
                warnings.push(format!("{}: override 已跳过: {reason}", m.id));
            };
            if o.utc_start.is_some() || o.utc_end.is_some() {
                skip("含时间条件（utc_start/utc_end），暂不支持".into());
                continue;
            }
            let Some(min) = o
                .min_prompt_tokens
                .filter(|v| v.is_finite() && *v >= 0.0 && v.fract() == 0.0)
            else {
                skip(format!(
                    "min_prompt_tokens 非法或缺失: {:?}",
                    o.min_prompt_tokens
                ));
                continue;
            };
            let rates = [
                parse_price(&o.prompt),
                parse_price(&o.completion),
                parse_price(&o.input_cache_read),
                parse_price(&o.input_cache_write),
            ];
            if let Some(Err(reason)) = rates.iter().find(|r| r.is_err()) {
                skip(reason.clone());
                continue;
            }
            let [prompt, completion, cache_read, cache_write] = rates.map(|r| r.unwrap());
            overrides.push(SnapshotOverride {
                min_prompt_tokens: min as u64,
                prompt,
                completion,
                cache_read,
                cache_write,
            });
        }
        overrides.sort_by_key(|o| o.min_prompt_tokens);
        let mut deduped: Vec<SnapshotOverride> = Vec::new();
        for o in overrides {
            if deduped
                .last()
                .is_some_and(|prev| prev.min_prompt_tokens == o.min_prompt_tokens)
            {
                warnings.push(format!(
                    "{}: 重复 min_prompt_tokens {}，保留首个",
                    m.id, o.min_prompt_tokens
                ));
                continue;
            }
            deduped.push(o);
        }
        let parsed = (|| -> Result<SnapshotEntry, String> {
            Ok(SnapshotEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                prompt: parse_price(&p.prompt)?,
                completion: parse_price(&p.completion)?,
                cache_read: parse_price(&p.input_cache_read)?,
                cache_write: parse_price(&p.input_cache_write)?,
                overrides: deduped,
            })
        })();
        match parsed {
            Ok(e) => entries.push(e),
            Err(reason) => {
                rejected += 1;
                warnings.push(format!("{}: {reason}", m.id));
            }
        }
    }
    for w in &warnings {
        log::warn!("OpenRouter 同步丢弃数据: {w}");
    }
    let _ = rejected;
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| a.id == b.id);

    let synced_at = jiff::Zoned::now().to_string();
    let count = entries.len() as u64;
    let snapshot = Snapshot {
        v: 2,
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
        warnings,
    })
}

/// 同步响应体（Task 7.3：sync_with 可注入，类型对同 crate 测试可见）。
#[derive(Debug, Deserialize)]
pub(crate) struct ApiResponse {
    pub(crate) data: Vec<ApiModel>,
}

/// 读快照：文件缺失 → Ok(None)（静默）；损坏 → Err（调用方警告并忽略）。
pub fn load_snapshot(snapshot_path: &Path) -> Result<Option<Snapshot>> {
    if !snapshot_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(snapshot_path)
        .with_context(|| format!("读快照失败: {}", snapshot_path.display()))?;
    let snapshot: Snapshot = serde_json::from_str(&text)
        .with_context(|| format!("快照解析失败: {}", snapshot_path.display()))?;
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_price_rejects_nonfinite_and_negative() {
        // R06：OR API 价格串可能是 "NaN"/"Infinity"/负数——同步期必须拒绝，
        // 不让脏值进入快照（快照 JSON 数值字段无法事后拦截字符串形式）。
        assert!(super::parse_price(&None).unwrap().is_none());
        assert!(super::parse_price(&Some(String::new())).unwrap().is_none());
        assert_eq!(
            super::parse_price(&Some("0.0000005".into())).unwrap(),
            Some(0.0000005)
        );
        assert_eq!(super::parse_price(&Some("0".into())).unwrap(), Some(0.0));
        for bad in ["NaN", "Infinity", "-Infinity", "inf", "-5", "-0.1"] {
            assert!(
                super::parse_price(&Some(bad.to_string())).is_err(),
                "{bad} 必须被拒绝"
            );
        }
    }

    use super::*;

    fn write_fixture(dir: &Path) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        dir.join("pricing-openrouter.json")
    }

    #[test]
    fn test_openrouter_parse_fixture() {
        // 用构造的 API 响应验证解析与瘦身（不联网）。
        let body = r#"{"data":[
            {"id":"anthropic/claude-sonnet-4.5","name":"Anthropic: Claude Sonnet 4.5",
             "pricing":{"prompt":"0.000003","completion":"0.000015",
                        "input_cache_read":"0.0000003","input_cache_write":"0.00000375"}},
            {"id":"tencent/hy3:free","name":"Tencent: HY3 (free)",
             "pricing":{"prompt":"0","completion":"0"}},
            {"id":"broken/model","name":"Broken","pricing":{"prompt":"","completion":null}}
        ]}"#;
        let api: ApiResponse = serde_json::from_str(body).unwrap();
        assert_eq!(api.data.len(), 3);
        let p = &api.data[0].pricing.as_ref().unwrap();
        // Task 7.4：数值正常解析；缺失/空串 → None（未知 ≠ 0）。
        assert!(
            parse_price(&p.prompt)
                .unwrap()
                .is_some_and(|v| (v * 1e6 - 3.0).abs() < 1e-9)
        );
        assert!(
            parse_price(&p.input_cache_read)
                .unwrap()
                .is_some_and(|v| (v * 1e6 - 0.3).abs() < 1e-9)
        );
        assert_eq!(parse_price(&None).unwrap(), None, "缺失字段 = 未知");
        let broken = &api.data[2].pricing.as_ref().unwrap();
        assert_eq!(parse_price(&broken.prompt).unwrap(), None, "空串 = 未知");
        assert_eq!(
            parse_price(&broken.completion).unwrap(),
            None,
            "null = 未知"
        );
    }

    #[test]
    fn test_openrouter_missing_price_is_unknown() {
        // Task 7.4：缺价字段经 sync_with 保留为 None（不静默变 0）。
        let dir = std::env::temp_dir().join(format!("tokenscope-t74-miss-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-openrouter.json");
        let body = r#"{"data":[{"id":"prov/no-price","name":"NP"}]}"#;
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(body).unwrap())
        })
        .unwrap();
        assert_eq!(report.count, 1);
        let snap = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(snap.entries[0].prompt, None, "缺价 = 未知");
        assert_eq!(snap.entries[0].completion, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_openrouter_negative_price_is_rejected() {
        // Task 7.4：负价条目拒绝（不入快照），其余条目正常。
        let dir = std::env::temp_dir().join(format!("tokenscope-t74-neg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-openrouter.json");
        let body = r#"{"data":[
            {"id":"prov/neg","name":"Neg","pricing":{"prompt":"-0.001","completion":"0.002"}},
            {"id":"prov/ok","name":"Ok","pricing":{"prompt":"0.001","completion":"0.002"}}
        ]}"#;
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(body).unwrap())
        })
        .unwrap();
        assert_eq!(report.count, 1, "负价条目被拒绝");
        let snap = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(snap.entries[0].id, "prov/ok");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_invalid_openrouter_override_rejects_model_during_sync() {
        // F04：override 显式单价非法（"NaN"）→ **整个模型**不入快照——
        // 不得只丢弃脏 override 后让模型按基础价完整计费；合法兄弟模型
        // 保留；同步告警包含模型与分项。
        let dir = std::env::temp_dir().join(format!("tokenscope-f04-or-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-openrouter.json");
        let body = r#"{"data":[
            {"id":"prov/bad","name":"Bad","pricing":{
                "prompt":"0.000002","completion":"0.000003",
                "overrides":[
                    {"min_prompt_tokens":100000,"prompt":"NaN","completion":"0.000006"},
                    {"min_prompt_tokens":200000,"prompt":"0.000009","completion":"0.00001"}
                ]}},
            {"id":"prov/good","name":"Good","pricing":{"prompt":"0.000002","completion":"0.000003"}}
        ]}"#;
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(body).unwrap())
        })
        .unwrap();
        assert_eq!(report.count, 1, "含非法 override 的模型必须整条拒绝");
        let snap = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.entries[0].id, "prov/good", "合法兄弟模型保留");
        // 同步诊断包含模型与分项（沿既有告警路径，随 SyncReport 携带）
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("prov/bad") && w.contains("prompt")),
            "告警须含模型与分项: {:?}",
            report.warnings
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_openrouter_override() {
        // Task 3：pricing.overrides（min_prompt_tokens 条件价）必须整组
        // 保留到快照；乱序升序化，同阈值保留首个，结构性脏数据（时间条件/
        // 缺阈值/重复档）跳过并留 warning。
        // F04：显式单价非法（负价）的 override → **整个模型拒绝**，不再
        // "丢弃脏 override 后按基础价计费"。
        let dir = std::env::temp_dir().join(format!("tokenscope-or-ov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-openrouter.json");
        let body = r#"{"data":[
            {"id":"prov/long","name":"Long",
             "pricing":{
                 "prompt":"0.000004","completion":"0.00002",
                 "overrides":[
                     {"min_prompt_tokens":1000000,"prompt":"0.000008","completion":"0.00003",
                      "input_cache_read":"0.0000008","input_cache_write":"0.00001"},
                     {"min_prompt_tokens":272000,"prompt":"0.000006","completion":"0.000025",
                      "input_cache_read":"","input_cache_write":null},
                     {"min_prompt_tokens":272000,"prompt":"0.000099","completion":"0.000099"},
                     {"min_prompt_tokens":100000,"utc_start":"1630","utc_end":"1900",
                      "prompt":"0.000001","completion":"0.000001"},
                     {"prompt":"0.000005","completion":"0.000005"}
                 ]}},
            {"id":"prov/neg-ov","name":"NegOv",
             "pricing":{
                 "prompt":"0.000002","completion":"0.000003",
                 "overrides":[{"min_prompt_tokens":150000,"prompt":"-0.000001","completion":"0.00001"}]}},
            {"id":"prov/good","name":"Good","pricing":{"prompt":"0.000002","completion":"0.000003"}}
        ]}"#;
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(body).unwrap())
        })
        .unwrap();
        assert_eq!(report.count, 2, "合法模型保留；负价 override 模型整条拒绝");
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("prov/neg-ov") && w.contains("prompt")),
            "负价 override 须有含模型与分项的告警: {:?}",
            report.warnings
        );
        let snap = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(snap.entries[0].id, "prov/good");
        assert_eq!(snap.entries[1].id, "prov/long");
        let e = &snap.entries[1];
        assert_eq!(e.overrides.len(), 2, "时间条件/缺阈值/重复档被剔除");
        // 乱序 → 升序；重复 272000 保留首个；空串/null = 未知。
        assert_eq!(e.overrides[0].min_prompt_tokens, 272_000);
        assert_eq!(e.overrides[0].prompt, Some(0.000006));
        assert_eq!(e.overrides[0].cache_read, None, "空串 = 未知");
        assert_eq!(e.overrides[0].cache_write, None, "null = 未知");
        assert_eq!(e.overrides[1].min_prompt_tokens, 1_000_000);
        assert_eq!(e.overrides[1].cache_read, Some(0.0000008));
        assert_eq!(e.overrides[1].cache_write, Some(0.00001));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_openrouter_snapshot_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_fixture(&dir);
        let snapshot = Snapshot {
            v: 2,
            synced_at: "2026-10-04T00:00:00+08:00".into(),
            entries: vec![SnapshotEntry {
                id: "x-ai/grok-4.5".into(),
                name: Some("xAI: Grok 4.5".into()),
                prompt: Some(2e-6),
                completion: Some(6e-6),
                cache_read: Some(3e-7),
                cache_write: Some(0.0),
                overrides: Vec::new(),
            }],
        };
        std::fs::write(&path, serde_json::to_string_pretty(&snapshot).unwrap()).unwrap();
        let loaded = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(loaded.entries.len(), 1);
        assert_eq!(loaded.entries[0].id, "x-ai/grok-4.5");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_openrouter_missing_snapshot_silent() {
        assert!(
            load_snapshot(Path::new("Z:/no-such/snapshot.json"))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn test_openrouter_corrupt_snapshot_is_err() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pricing-openrouter.json");
        std::fs::write(&path, "not json").unwrap();
        assert!(load_snapshot(&path).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
