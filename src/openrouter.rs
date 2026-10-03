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
struct ApiModel {
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
}

/// 快照条目：只保留计价所需字段（单位：USD / token）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub id: String,
    pub name: Option<String>,
    pub prompt: f64,
    pub completion: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub synced_at: String,
    pub entries: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub count: u64,
    pub path: String,
    pub synced_at: String,
}

fn parse_price(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0)
}

/// 拉取 OpenRouter 模型清单并写快照（覆盖旧文件）。
pub fn sync(snapshot_path: &Path) -> Result<SyncReport> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .build()
        .new_agent();
    let mut response = agent
        .get(API_URL)
        .header("User-Agent", "tokenscope")
        .call()
        .with_context(|| format!("请求 OpenRouter 失败: {API_URL}"))?;
    let body: ApiResponse = response
        .body_mut()
        .read_json()
        .context("解析 OpenRouter 响应失败")?;

    let mut entries: Vec<SnapshotEntry> = body
        .data
        .into_iter()
        .map(|m| {
            let p = m.pricing.unwrap_or_default();
            let price = |v: &Option<String>| v.as_deref().map(parse_price).unwrap_or(0.0);
            SnapshotEntry {
                id: m.id,
                name: m.name,
                prompt: price(&p.prompt),
                completion: price(&p.completion),
                cache_read: price(&p.input_cache_read),
                cache_write: price(&p.input_cache_write),
            }
        })
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries.dedup_by(|a, b| a.id == b.id);

    let synced_at = jiff::Zoned::now().to_string();
    let count = entries.len() as u64;
    let snapshot = Snapshot {
        synced_at: synced_at.clone(),
        entries,
    };
    if let Some(dir) = snapshot_path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("创建目录失败: {}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(&snapshot)?;
    std::fs::write(snapshot_path, json)
        .with_context(|| format!("写快照失败: {}", snapshot_path.display()))?;
    Ok(SyncReport {
        count,
        path: snapshot_path.display().to_string(),
        synced_at,
    })
}

#[derive(Debug, Deserialize)]
struct ApiResponse {
    data: Vec<ApiModel>,
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
        assert!((parse_price(p.prompt.as_deref().unwrap_or("0")) * 1e6 - 3.0).abs() < 1e-9);
        assert!(
            (parse_price(p.input_cache_read.as_deref().unwrap_or("0")) * 1e6 - 0.3).abs() < 1e-9
        );
        // null / 空串 / 缺失字段安全回退 0
        assert_eq!(parse_price(""), 0.0);
        let broken = &api.data[2].pricing.as_ref().unwrap();
        let price = |v: &Option<String>| v.as_deref().map(parse_price).unwrap_or(0.0);
        assert_eq!(price(&broken.prompt) + price(&broken.completion), 0.0);
    }

    #[test]
    fn test_openrouter_snapshot_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tokenscope-m5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = write_fixture(&dir);
        let snapshot = Snapshot {
            synced_at: "2026-10-04T00:00:00+08:00".into(),
            entries: vec![SnapshotEntry {
                id: "x-ai/grok-4.5".into(),
                name: Some("xAI: Grok 4.5".into()),
                prompt: 2e-6,
                completion: 6e-6,
                cache_read: 3e-7,
                cache_write: 0.0,
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
