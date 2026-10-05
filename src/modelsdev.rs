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

#[derive(Debug, Deserialize)]
struct ApiEntry {
    #[serde(default)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    /// 快照格式版本：v2 起分项可空（缺键=未知）；v1 的缓存分项被 0 填充
    /// 已损失信息，加载时按未知保守处理，等待下一次同步升级。
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
}

/// 拉取 models.dev 并写快照（跳过无 cost 的模型；同键去重保留排序靠前者）。
pub fn sync(snapshot_path: &Path) -> Result<SyncReport> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(60)))
        .build()
        .new_agent();
    let mut response = agent
        .get(API_URL)
        .header("User-Agent", "tokenscope")
        .call()
        .with_context(|| format!("请求 models.dev 失败: {API_URL}"))?;
    let body: BTreeMap<String, ApiEntry> = response
        .body_mut()
        .read_json()
        .context("解析 models.dev 响应失败")?;

    let mut entries: Vec<SnapshotEntry> = Vec::new();
    for (provider, entry) in body {
        for (model_id, m) in entry.models {
            let Some(cost) = m.cost else {
                continue; // 无 cost（免费/非 LLM/占位）不入快照
            };
            entries.push(SnapshotEntry {
                id: format!("{provider}/{model_id}"),
                name: m.name.or(entry.name.clone()),
                input: cost.input,
                output: cost.output,
                cache_read: cost.cache_read,
                cache_write: cost.cache_write,
            });
        }
    }
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| {
        // 归一化后同键（跨 provider 转发同一模型）：保留排序靠前者
        crate::pricing::normalize_model_id(&a.id) == crate::pricing::normalize_model_id(&b.id)
    });

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
    })
}

/// 读快照：缺失 → Ok(None)；损坏 → Err（调用方警告并忽略该层）。
/// v1 旧快照（分项被 0 填充）→ 缓存分项降级为未知（保守），建议重新同步。
pub fn load_snapshot(snapshot_path: &Path) -> Result<Option<Snapshot>> {
    if !snapshot_path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(snapshot_path)
        .with_context(|| format!("读快照失败: {}", snapshot_path.display()))?;
    let mut snapshot: Snapshot = serde_json::from_str(&text)
        .with_context(|| format!("快照解析失败: {}", snapshot_path.display()))?;
    if snapshot.v < 2 {
        log::warn!(
            "models.dev 快照为 v1 格式（缓存分项曾被 0 填充），按未知保守处理；重新同步后恢复精确"
        );
        for e in &mut snapshot.entries {
            e.cache_read = None;
            e.cache_write = None;
        }
        snapshot.v = 2;
    }
    Ok(Some(snapshot))
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
                    m.cost.map(|c| SnapshotEntry {
                        id: format!("{provider}/{id}"),
                        name: m.name.or(entry.name.clone()),
                        input: c.input,
                        output: c.output,
                        cache_read: c.cache_read,
                        cache_write: c.cache_write,
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
}
