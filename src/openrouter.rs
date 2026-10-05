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
    if n < 0.0 {
        return Err(format!("负价 {n}"));
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
        let parsed = (|| -> Result<SnapshotEntry, String> {
            Ok(SnapshotEntry {
                id: m.id.clone(),
                name: m.name.clone(),
                prompt: parse_price(&p.prompt)?,
                completion: parse_price(&p.completion)?,
                cache_read: parse_price(&p.input_cache_read)?,
                cache_write: parse_price(&p.input_cache_write)?,
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
        log::warn!("OpenRouter 同步拒绝脏条目: {w}");
    }
    let _ = rejected;
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
    crate::fsutil::atomic_write(snapshot_path, json.as_bytes())
        .with_context(|| format!("写快照失败: {}", snapshot_path.display()))?;
    Ok(SyncReport {
        count,
        path: snapshot_path.display().to_string(),
        synced_at,
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
                prompt: Some(2e-6),
                completion: Some(6e-6),
                cache_read: Some(3e-7),
                cache_write: Some(0.0),
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
