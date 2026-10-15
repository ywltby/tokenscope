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

/// RC08：OpenRouter 的单价字段在真实 API 中**既可能是 JSON 字符串**
///（`"0.000003"`）**也可能是 JSON 数字**（`0`、`0.000003`）。用局部输入
/// 类型同时接受两者。
///
/// 边界（刻意收紧）：
/// - 只放宽**价格字段**——`id`/`name`/`utc_start` 等仍是字符串，不对整份
///   响应文本做替换，也不把任意 JSON 宽松转成数字；
/// - 布尔/数组/对象等**显式非法**（`Other`），随模型与分项报告，不因宽松
///   匹配变成"未知的合法值"；
/// - 根响应结构损坏仍由 `ApiResponse` 反序列化明确失败。
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum PriceInput {
    /// JSON 数字（含整数 0）。
    Number(f64),
    /// JSON 字符串（数字串；空串仍表示"缺失"）。
    Str(String),
    /// 布尔/数组/对象等——非法价格，携带原始形态用于诊断。
    Other(serde_json::Value),
}

#[derive(Debug, Default, Deserialize)]
struct ApiPricing {
    #[serde(default)]
    prompt: Option<PriceInput>,
    #[serde(default)]
    completion: Option<PriceInput>,
    #[serde(default)]
    input_cache_read: Option<PriceInput>,
    #[serde(default)]
    input_cache_write: Option<PriceInput>,
    /// Task 3：条件价格覆盖（长上下文等）。时间条件条目（utc_start/utc_end）
    /// 在 Task 4A 落地前跳过并留 warning，不猜测其适用条件。
    #[serde(default)]
    overrides: Vec<ApiOverride>,
}

/// 条件价格覆盖条目：价格字段内联（USD/token，数字或数字串），条件字段可选。
#[derive(Debug, Default, Deserialize)]
struct ApiOverride {
    #[serde(default)]
    min_prompt_tokens: Option<f64>,
    #[serde(default)]
    prompt: Option<PriceInput>,
    #[serde(default)]
    completion: Option<PriceInput>,
    #[serde(default)]
    input_cache_read: Option<PriceInput>,
    #[serde(default)]
    input_cache_write: Option<PriceInput>,
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

/// Task 7.4 / RC08：解析单价——`None` 表示字段缺失（未知），非负有限数值
/// 才有效。数字与数字字符串走**同一**校验；显式 0 合法（免费）。
/// 返回 Err 表示非法（负价 / NaN / Infinity / 坏数字串 / 非数字类型），
/// 由调用方整条拒绝该模型并给出含模型与分项的诊断。
fn parse_price(v: &Option<PriceInput>) -> Result<Option<f64>, String> {
    let Some(raw) = v else {
        return Ok(None);
    };
    match raw {
        PriceInput::Number(n) => validate_price(*n),
        PriceInput::Str(s) => {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None); // 空串 = 未知（既有语义不变）
            }
            let n: f64 = s.parse().map_err(|e| format!("价格串非法 {s:?}: {e}"))?;
            validate_price(n)
        }
        PriceInput::Other(v) => Err(format!("价格类型非法（须为数字或数字字符串）: {v}")),
    }
}

/// 有限非负校验（数字与数字字符串共用）。
/// R06：API 可能返回 NaN/Infinity/-Infinity——必须在此拒绝，避免脏值进入
/// 快照（快照字段是数值，无法二次拦截）。
fn validate_price(n: f64) -> Result<Option<f64>, String> {
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
    parse_snapshot_text(&text)
        .map(Some)
        .with_context(|| format!("快照解析失败: {}", snapshot_path.display()))
}

/// SF03：解析已读入内存的快照文本——读取与健康分类由调用方（pricing）
/// 统一负责；错误只含解析原因，路径上下文由调用方补充。
pub(crate) fn parse_snapshot_text(text: &str) -> Result<Snapshot> {
    let snapshot: Snapshot = serde_json::from_str(text)?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 数字形态（RC08）。
    fn num(v: f64) -> Option<PriceInput> {
        Some(PriceInput::Number(v))
    }

    /// 字符串形态（RC08）。
    fn str_(v: &str) -> Option<PriceInput> {
        Some(PriceInput::Str(v.to_string()))
    }

    #[test]
    fn test_parse_price_accepts_number_and_string_equally() {
        // RC08：OpenRouter 的单价既可能是数字也可能是数字字符串，两者必须
        // 走**同一**校验并得到同一结果（此前 `Option<String>` 会把数字条目
        // 整条反序列化失败 → 整个响应解析失败 → 0 条导入）。
        assert!(parse_price(&None).unwrap().is_none(), "缺失 = 未知");
        assert!(parse_price(&str_("")).unwrap().is_none(), "空串 = 未知");
        for v in [0.0, 0.0000005, 3.0, 1e-8] {
            assert_eq!(
                parse_price(&num(v)).unwrap(),
                parse_price(&str_(&format!("{v}"))).unwrap(),
                "数字与数字串必须等价：{v}"
            );
        }
        // 整数 0 与 "0" 都是显式免费（未知 ≠ 0）
        assert_eq!(parse_price(&num(0.0)).unwrap(), Some(0.0));
        assert_eq!(parse_price(&str_("0")).unwrap(), Some(0.0));
        assert_ne!(parse_price(&None).unwrap(), Some(0.0), "未知不得变成 0");
    }

    #[test]
    fn test_parse_price_rejects_nonfinite_negative_and_bad_types() {
        // R06：OR API 价格可能是 NaN/Infinity/负数——同步期必须拒绝，不让
        // 脏值进入快照（快照 JSON 数值字段无法事后拦截字符串形式）。
        for bad in ["NaN", "Infinity", "-Infinity", "inf", "-5", "-0.1"] {
            assert!(parse_price(&str_(bad)).is_err(), "字符串 {bad} 必须被拒绝");
        }
        // RC08：数字形态同样受校验（f64::NAN / 负数不可能来自合法 API，
        // 但必须同路径拒绝，不能因绕过字符串解析而放行）。
        assert!(parse_price(&num(f64::NAN)).is_err(), "NaN 必须被拒绝");
        assert!(
            parse_price(&num(f64::INFINITY)).is_err(),
            "Infinity 必须被拒绝"
        );
        assert!(parse_price(&num(-0.1)).is_err(), "负价必须被拒绝");
        // 显式非法类型（布尔/数组/对象）→ 带形态的诊断，不当作"未知"
        let other: PriceInput = serde_json::from_value(serde_json::json!(true)).unwrap();
        let err = parse_price(&Some(other)).unwrap_err();
        assert!(err.contains("类型非法"), "{err}");
        assert!(err.contains("true"), "诊断须含原始形态：{err}");
    }

    /// RC08 测试专用临时目录（每个用例独立 tag + 序号，避免并行互相覆盖）。
    fn numeric_dir(tag: &str) -> std::path::PathBuf {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tokenscope-rc08-{tag}-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

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

    /// RC08：数字单价、数字串单价、整数 0、小数与 override 混用，经**真实
    /// serde `ApiResponse` → `sync_with` → 临时快照**链路完整导入。
    /// 复核现象：此前价格字段是 `Option<String>`，数字形态让**整个响应**
    /// 反序列化失败 → 0 条导入、对照价全丢。
    #[test]
    fn openrouter_accepts_numeric_and_string_prices() {
        let body =
            std::fs::read_to_string("tests/fixtures/openrouter/numeric-prices.json").unwrap();
        // 第一步：数字形态必须能被反序列化（此前整个响应会解析失败）。
        let api: ApiResponse =
            serde_json::from_str(&body).unwrap_or_else(|e| panic!("数字单价响应必须可解析: {e}"));
        assert_eq!(api.data.len(), 4, "fixture 四个模型全部解析");

        let dir = numeric_dir("accept");
        let path = dir.join("pricing-openrouter.json");
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(&body).unwrap())
        })
        .unwrap();

        // prov/bad-type 整条拒绝（显式非法类型）；其余三个保留。
        assert_eq!(report.count, 3, "非法类型的模型整条拒绝");
        let bad = report
            .warnings
            .iter()
            .find(|w| w.contains("prov/bad-type"))
            .unwrap_or_else(|| panic!("告警须含被拒模型: {:?}", report.warnings));
        // 诊断同时含**分项**与非法形态，不静默改成 0、也不当作"未知合法值"。
        for comp in [
            "input_cache_read",
            "input_cache_write",
            "prompt",
            "completion",
        ] {
            if comp == "input_cache_write" {
                assert!(
                    !bad.contains(&format!("基础 {comp}")),
                    "合法分项不得出现在非法诊断里: {bad}"
                );
            } else {
                assert!(
                    bad.contains(&format!("基础 {comp}")),
                    "诊断须含分项 {comp}: {bad}"
                );
            }
        }
        assert!(
            bad.contains("类型非法"),
            "布尔/数组/对象须报类型非法: {bad}"
        );
        assert!(
            bad.contains("整条模型已拒绝"),
            "须整条拒绝而非丢字段: {bad}"
        );

        let snap = load_snapshot(&path).unwrap().unwrap();
        let find = |id: &str| snap.entries.iter().find(|e| e.id == id).unwrap();

        // (a) 数字与字符串混用：两者等价导入，精度保留（USD/token 原单位）
        let mixed = find("prov/numeric-mixed");
        assert!((mixed.prompt.unwrap() - 0.000003).abs() < 1e-12, "数字价");
        assert!(
            (mixed.completion.unwrap() - 0.000015).abs() < 1e-12,
            "字符串价与数字同路径"
        );
        assert!((mixed.cache_read.unwrap() - 0.0000003).abs() < 1e-15);
        assert!((mixed.cache_write.unwrap() - 0.00000375).abs() < 1e-12);
        // 数字形态的 override 同样导入（此前数字会让整条丢失）
        assert_eq!(mixed.overrides.len(), 1, "数字 override 不得丢失");
        assert_eq!(mixed.overrides[0].min_prompt_tokens, 200_000);
        assert!((mixed.overrides[0].prompt.unwrap() - 0.000006).abs() < 1e-12);

        // (b) 数字 0 = 显式免费（不被当作缺失）
        let zero = find("prov/numeric-zero");
        assert_eq!(zero.prompt, Some(0.0), "数字 0 = 免费");
        assert_eq!(zero.completion, Some(0.0));
        assert_eq!(zero.cache_read, Some(0.0));
        assert_eq!(zero.cache_write, Some(0.0));

        // (c) override 里的数字 0 同样保留（min_prompt_tokens 是数字）
        let zero_ov = find("prov/zero-override");
        assert_eq!(zero_ov.overrides.len(), 1);
        assert_eq!(zero_ov.overrides[0].min_prompt_tokens, 100_000);
        assert_eq!(zero_ov.overrides[0].prompt, Some(0.0));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// RC08：宽松只作用于**价格字段**。`id`/`name`/时间条件仍按原类型要求，
    /// 根响应结构损坏必须明确失败——不得因宽松匹配把任意 JSON 转成数字。
    #[test]
    fn non_price_fields_and_root_structure_stay_strict() {
        // 根结构损坏：data 不是数组 / 缺失必填 id。
        for broken in [
            r#"{"data":"not-an-array"}"#,
            r#"{"data":[{"pricing":{"prompt":0}}]}"#,
            r#"{"items":[]}"#,
        ] {
            assert!(
                serde_json::from_str::<ApiResponse>(broken).is_err(),
                "根响应结构损坏必须明确失败: {broken}"
            );
        }
        // 非价格字段不被宽松转换：数字 id 仍非法。
        assert!(
            serde_json::from_str::<ApiResponse>(r#"{"data":[{"id":123,"pricing":{"prompt":0}}]}"#)
                .is_err(),
            "id 必须保持字符串语义"
        );
        // 时间条件字段仍是字符串（数字形态不得被接受）。
        assert!(
            serde_json::from_str::<ApiResponse>(
                r#"{"data":[{"id":"p/m","pricing":{"prompt":0,
                   "overrides":[{"min_prompt_tokens":1000,"utc_start":1630,"prompt":0}]}}]}"#
            )
            .is_err(),
            "utc_start 必须保持字符串语义"
        );
    }

    /// RC08：显式 0（基础价与 override）经同步后，在快照与价格视图里都保持
    /// "免费"，与"未知"逐字段区分；缺失分项仍是 Unknown，不压成 0。
    #[test]
    fn numeric_zero_is_preserved_in_snapshot_and_pricing_view() {
        let body =
            std::fs::read_to_string("tests/fixtures/openrouter/numeric-prices.json").unwrap();
        let dir = numeric_dir("zero-view");
        let path = dir.join("pricing-openrouter.json");
        sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(&body).unwrap())
        })
        .unwrap();

        // —— 快照层：数字 0 逐字段保留 ——
        let snap = load_snapshot(&path).unwrap().unwrap();
        let zero = snap
            .entries
            .iter()
            .find(|e| e.id == "prov/numeric-zero")
            .unwrap();
        assert_eq!(
            (
                zero.prompt,
                zero.completion,
                zero.cache_read,
                zero.cache_write
            ),
            (Some(0.0), Some(0.0), Some(0.0), Some(0.0)),
            "基础四价的数字 0 必须逐字段保留"
        );
        let zero_ov = snap
            .entries
            .iter()
            .find(|e| e.id == "prov/zero-override")
            .unwrap();
        assert_eq!(zero_ov.prompt, Some(0.000001), "字符串基础价不受影响");
        assert_eq!(zero_ov.overrides.len(), 1);
        assert_eq!(
            (
                zero_ov.overrides[0].prompt,
                zero_ov.overrides[0].completion,
                zero_ov.overrides[0].cache_read,
                zero_ov.overrides[0].cache_write
            ),
            (Some(0.0), Some(0.0), None, None),
            "override 的数字 0 保留，缺失分项保持未知"
        );

        // —— 视图层：Fixed(0) ≠ Unknown，且不被舍成非 0 ——
        let (pricing, warnings) = crate::pricing::Pricing::load(None, None, Some(&path));
        assert!(warnings.is_empty(), "{warnings:?}");
        let entries = pricing.entries();
        use crate::pricing::RateSpec;
        let view = |id: &str| {
            entries
                .iter()
                .find(|e| e.source == "OpenRouter" && e.prefix == id)
                .unwrap_or_else(|| panic!("缺少 OpenRouter 视图条目 {id}"))
        };
        let zero_v = view("prov/numeric-zero");
        assert_eq!(
            (
                &zero_v.input,
                &zero_v.output,
                &zero_v.cache_read,
                &zero_v.cache_write
            ),
            (
                &RateSpec::Fixed(0.0),
                &RateSpec::Fixed(0.0),
                &RateSpec::Fixed(0.0),
                &RateSpec::Fixed(0.0)
            ),
            "视图四价必须都是显式 0（免费），input={:?} output={:?} cache_read={:?} cache_write={:?}",
            zero_v.input,
            zero_v.output,
            zero_v.cache_read,
            zero_v.cache_write
        );
        assert!(!zero_v.base_incomplete, "全 0 基础价是完整的，不是部分计价");
        let zero_ov_v = view("prov/zero-override");
        assert_eq!(zero_ov_v.segments.len(), 1, "override → 视图分段");
        assert_eq!(
            zero_ov_v.segments[0].min_tokens, 100_000,
            "分段阈值来自数字 min_prompt_tokens"
        );
        assert_eq!(
            zero_ov_v.segments[0].prices.input,
            RateSpec::Fixed(0.0),
            "分段输入价的 0 保留"
        );
        assert_eq!(
            zero_ov_v.segments[0].prices.output,
            RateSpec::Fixed(0.0),
            "分段输出价的 0 保留"
        );
        assert_eq!(
            zero_ov_v.segments[0].prices.cache_read,
            RateSpec::Unknown,
            "缺失的分项不得被 0 冒充"
        );
        assert_eq!(zero_ov_v.segments[0].prices.cache_write, RateSpec::Unknown);
        // 序列化：显式 0 输出为 0，缺失输出为 null（前端三态依赖此形态）。
        let json = serde_json::to_string(&zero_v).unwrap();
        assert!(json.contains("\"input\":0.0"), "{json}");
        let seg_json = serde_json::to_string(&zero_ov_v.segments[0]).unwrap();
        assert!(seg_json.contains("\"cache_read\":null"), "{seg_json}");
        assert!(
            !seg_json.contains("\"cache_read\":0"),
            "未知不得序列化为 0: {seg_json}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// RC08：拉取失败与响应级解析失败都不得覆盖已有快照（原子写只发生在
    /// 完整成功之后）。
    #[test]
    fn failed_sync_preserves_previous_snapshot() {
        let dir = numeric_dir("failed-sync");
        let path = dir.join("pricing-openrouter.json");
        let previous = r#"{"v":2,"synced_at":"2026-10-01T00:00:00+08:00","entries":[
            {"id":"prov/keep","name":"Keep","prompt":0.000002,"completion":0.000006,
             "cache_read":0.0000002,"cache_write":0}
        ]}"#;
        std::fs::write(&path, previous).unwrap();
        let before = std::fs::read(&path).unwrap();

        // (a) 拉取失败
        let e1 = sync_with(&path, || anyhow::bail!("网络不可达"));
        assert!(e1.is_err(), "拉取失败必须显式报错");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "拉取失败不得改写快照"
        );

        // (b) 响应级解析失败（真实 fetch_openrouter 即 read_json → Err）：
        // 根结构损坏必须整体失败，宽松化只作用于价格字段，不掩盖结构损坏。
        let garbage = r#"{"data":"not-an-array"}"#;
        let e2 = sync_with(&path, || {
            serde_json::from_str::<ApiResponse>(garbage)
                .map_err(|e| anyhow::anyhow!("解析 OpenRouter 响应失败: {e}"))
        });
        assert!(e2.is_err(), "响应结构损坏必须明确失败");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "解析失败不得改写快照"
        );

        // (c) 离线读取仍拿到原有全部条目与三态（0 仍是 Some(0.0)）
        let snap = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(snap.entries.len(), 1);
        assert_eq!(snap.entries[0].id, "prov/keep");
        assert_eq!(snap.entries[0].cache_write, Some(0.0));

        // (d) 成功同步才替换快照，且仍是原子写
        let ok = r#"{"data":[{"id":"prov/new","pricing":{"prompt":0.000001,"completion":0}}]}"#;
        let report = sync_with(&path, || {
            Ok(serde_json::from_str::<ApiResponse>(ok).unwrap())
        })
        .unwrap();
        assert_eq!(report.count, 1);
        let snap = load_snapshot(&path).unwrap().unwrap();
        assert_eq!(snap.entries[0].id, "prov/new");
        assert_eq!(snap.entries[0].completion, Some(0.0));
        let tmp_leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|r| r.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != "pricing-openrouter.json")
            .collect();
        assert!(
            tmp_leftovers.is_empty(),
            "原子写不得留下临时文件: {tmp_leftovers:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// RC08 修复前对照：旧 schema 只接受字符串单价——数字形态会让**整个
    /// 响应**反序列化失败（不只是丢一个字段），导致 0 条导入、对照价全丢。
    /// 这里在同一份 fixture 上并排演示旧行为失败 / 新行为成功。
    #[test]
    fn test_rc08_old_string_only_schema_rejects_numeric_fixture() {
        // 这些结构只用于复现修复前的 schema，字段不需要被读取。
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct OldPricing {
            #[serde(default)]
            prompt: Option<String>,
            #[serde(default)]
            completion: Option<String>,
            #[serde(default)]
            input_cache_read: Option<String>,
            #[serde(default)]
            input_cache_write: Option<String>,
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct OldModel {
            #[serde(default)]
            pricing: Option<OldPricing>,
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct OldResponse {
            #[serde(default)]
            data: Vec<OldModel>,
        }

        let body =
            std::fs::read_to_string("tests/fixtures/openrouter/numeric-prices.json").unwrap();
        // 旧 schema（Option<String>）：数字单价 → 整份响应解析失败。
        let old = serde_json::from_str::<OldResponse>(&body);
        assert!(
            old.is_err(),
            "修复前：数字单价必须让旧 schema 解析失败（这正是复核现象）"
        );
        // 新 schema：同一份响应完整解析，四个模型都在。
        let new = serde_json::from_str::<ApiResponse>(&body).unwrap();
        assert_eq!(new.data.len(), 4);
        // 且数字与字符串各自落到同一 PriceInput 语义（数字 → Number 分支）
        let first = new.data[0].pricing.as_ref().unwrap();
        assert!(
            matches!(first.prompt, Some(PriceInput::Number(_))),
            "数字形态走 Number 分支：{:?}",
            first.prompt
        );
        assert!(
            matches!(first.completion, Some(PriceInput::Str(_))),
            "字符串形态走 Str 分支：{:?}",
            first.completion
        );
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
