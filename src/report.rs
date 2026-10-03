//! CLI / GUI 共用的汇总管线：组装源 → 按文件缓存增量采集 → 全局去重 → 过滤 → 聚合。
//! 两端只做参数转换与渲染，数字永远同源。
//!
//! M4 缓存原则：**缓存是纯优化，不是事实源**——任何缓存故障都降级为全量内存
//! 扫描并告警，数字必须与无缓存一致。

use std::path::PathBuf;

use anyhow::Result;
use serde::Serialize;

use crate::aggregate::{GroupBy, aggregate, filter_days, local_tz};
use crate::cache::{Cache, CacheStats, mtime_ms};
use crate::dedupe::dedupe_events;
use crate::model::{AgentKind, UsageEvent};
use crate::pricing::Pricing;
use crate::render;
use crate::source::claude::ClaudeSource;
use crate::source::codex::CodexSource;
use crate::source::{CollectStats, Source};

#[derive(Debug, Default, Clone)]
pub struct SummaryOptions {
    pub by: GroupBy,
    pub agent: Option<AgentKind>,
    pub days: Option<u32>,
    pub claude_dir: Option<PathBuf>,
    pub codex_dir: Option<PathBuf>,
    /// 缓存目录（None = `~/.tokenscope`；测试注入用）。
    pub cache_dir: Option<PathBuf>,
    /// 外置价格文件路径（None = `~/.tokenscope/pricing.toml`；测试注入用）。
    pub pricing_path: Option<PathBuf>,
    /// OpenRouter 快照文件路径（None = `~/.tokenscope/pricing-openrouter.json`；测试注入用）。
    pub openrouter_path: Option<PathBuf>,
    /// 强制全量重解析并重建缓存。
    pub refresh: bool,
}

/// 单个来源的采集统计（GUI 表格脚注与 JSON 共用）。
#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub agent: AgentKind,
    pub stats: CollectStats,
}

#[derive(Debug, Clone, Serialize)]
pub struct SummaryReport {
    pub by: &'static str,
    pub groups: Vec<crate::aggregate::Group>,
    pub totals: crate::aggregate::Group,
    pub sources: Vec<SourceReport>,
    pub warnings: Vec<String>,
    pub generated_at: String,
}

impl SummaryReport {
    /// 终端表格（CLI）。
    pub fn to_table(&self) -> String {
        render::table(self)
    }

    /// 机器可读 JSON（CLI --json 与 Tauri summarize 共用）。
    pub fn to_json(&self) -> Result<String> {
        render::json::to_json(self)
    }
}

/// TokenScope 自有数据目录（M4）：`~/.tokenscope`。
pub fn data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("无法定位用户主目录"))?;
    Ok(home.join(".tokenscope"))
}

pub fn cache_file_path(cache_dir: Option<&PathBuf>) -> PathBuf {
    cache_dir
        .cloned()
        .or_else(|| data_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("cache.db")
}

/// 外置价格**文件**路径：传入即用；None = `~/.tokenscope/pricing.toml`。
pub fn openrouter_file_path(snapshot_file: Option<&PathBuf>) -> PathBuf {
    snapshot_file
        .cloned()
        .or_else(|| data_dir().ok().map(|d| d.join("pricing-openrouter.json")))
        .unwrap_or_else(|| PathBuf::from("pricing-openrouter.json"))
}

pub fn pricing_file_path(pricing_file: Option<&PathBuf>) -> PathBuf {
    pricing_file
        .cloned()
        .or_else(|| data_dir().ok().map(|d| d.join("pricing.toml")))
        .unwrap_or_else(|| PathBuf::from("pricing.toml"))
}

fn make_source(kind: AgentKind, dir: &Option<PathBuf>) -> Result<Box<dyn Source>> {
    Ok(match kind {
        AgentKind::ClaudeCode => Box::new(ClaudeSource::new(match dir {
            Some(p) => p.clone(),
            None => ClaudeSource::default_root()?,
        })),
        AgentKind::Codex => Box::new(CodexSource::new(match dir {
            Some(p) => p.clone(),
            None => CodexSource::default_root()?,
        })),
    })
}

/// 缓存开关解析：返回 Ok(None) 表示放弃缓存（含原因，调用方告警）。
fn open_cache(opts: &SummaryOptions) -> (Option<Cache>, Vec<String>) {
    let mut warnings = Vec::new();
    if opts.cache_dir.is_none() && data_dir().is_err() {
        warnings.push("无法定位数据目录，已退回全量扫描（不使用缓存）".to_string());
        return (None, warnings);
    }
    let path = cache_file_path(opts.cache_dir.as_ref());
    match Cache::open(&path) {
        Ok(c) => {
            if opts.refresh
                && let Err(e) = c.clear()
            {
                warnings.push(format!("缓存重置失败，已退回全量扫描: {e}"));
                return (None, warnings);
            }
            (Some(c), warnings)
        }
        Err(e) => {
            warnings.push(format!("缓存打开失败，已退回全量扫描: {e:#}"));
            (None, warnings)
        }
    }
}

/// 缓存状态（GUI 设置页）。
#[derive(Debug, Clone, Serialize)]
pub struct CacheInfo {
    pub path: String,
    pub files: u64,
    pub events: u64,
}

pub fn cache_stats(cache_dir: Option<PathBuf>) -> Result<CacheInfo> {
    let path = cache_file_path(cache_dir.as_ref());
    if !path.exists() {
        return Ok(CacheInfo {
            path: path.display().to_string(),
            files: 0,
            events: 0,
        });
    }
    let c = Cache::open(&path)?;
    let CacheStats { files, events } = c.stats()?;
    Ok(CacheInfo {
        path: path.display().to_string(),
        files,
        events,
    })
}

/// 重建缓存：清库后按当前日志全量重解析（GUI 设置页按钮）。
pub fn rebuild_cache(cache_dir: Option<PathBuf>) -> Result<CacheInfo> {
    summary(&SummaryOptions {
        by: GroupBy::Day,
        refresh: true,
        cache_dir: cache_dir.clone(),
        ..Default::default()
    })?;
    cache_stats(cache_dir)
}

pub fn summary(opts: &SummaryOptions) -> Result<SummaryReport> {
    let pricing_path = pricing_file_path(opts.pricing_path.as_ref());
    let openrouter_path = openrouter_file_path(opts.openrouter_path.as_ref());
    let (pricing, mut warnings) = Pricing::load(Some(&pricing_path), Some(&openrouter_path));
    let (cache, mut cache_warnings) = open_cache(opts);
    warnings.append(&mut cache_warnings);

    let mut sources: Vec<SourceReport> = Vec::new();
    let mut all_events: Vec<UsageEvent> = Vec::new();
    let mut keep_paths: Vec<String> = Vec::new();

    let kinds: Vec<AgentKind> = match opts.agent {
        Some(k) => vec![k],
        None => vec![AgentKind::ClaudeCode, AgentKind::Codex],
    };
    for kind in kinds {
        let dir = match kind {
            AgentKind::ClaudeCode => &opts.claude_dir,
            AgentKind::Codex => &opts.codex_dir,
        };
        let src = make_source(kind, dir)?;
        let mut stats = CollectStats::default();
        let mut events: Vec<UsageEvent> = Vec::new();
        let files = src.discover();
        if files.is_empty() && !src.root().is_dir() {
            warnings.push(format!(
                "{} 目录不存在：{}",
                match kind {
                    AgentKind::ClaudeCode => "Claude",
                    AgentKind::Codex => "Codex",
                },
                src.root().display()
            ));
        }
        for file in &files {
            let path_str = file.display().to_string();
            keep_paths.push(path_str.clone());
            let mut cached: Option<crate::source::FileParse> = None;
            if !opts.refresh
                && let (Some(c), Some(size), Ok(mt)) =
                    (cache.as_ref(), file_size(file), mtime_ms(file))
            {
                match c.lookup_file(&path_str, kind, size, mt) {
                    Ok(hit) => cached = hit.map(|cf| cf.parse),
                    Err(e) => warnings.push(format!("缓存读取失败（该文件全量解析）: {e:#}")),
                }
            }
            let parse = match cached {
                Some(p) => p,
                None => {
                    let p = src.parse_file(file);
                    if let Some(c) = &cache {
                        let size = file_size(file).unwrap_or(0);
                        let mt = mtime_ms(file).unwrap_or(0);
                        if let Err(e) = c.store_file(&path_str, kind, size, mt, &p) {
                            warnings.push(format!("缓存写入失败（不影响统计）: {e:#}"));
                        }
                    }
                    p
                }
            };
            stats.add_file(&parse);
            events.extend(parse.events);
        }
        stats.files_scanned = files.len() as u64;
        sources.push(SourceReport { agent: kind, stats });
        all_events.extend(events);
    }
    if let Some(c) = &cache
        && let Err(e) = c.purge_missing(&keep_paths)
    {
        warnings.push(format!("缓存清理失败（不影响统计）: {e:#}"));
    }

    // 全局去重（跨文件、按 agent 规则），并回填 per-agent 的丢弃数与事件数。
    let (mut events, dropped) = dedupe_events(all_events);
    for s in &mut sources {
        s.stats.duplicates_dropped = dropped
            .iter()
            .find(|(a, _)| *a == s.agent)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        s.stats.events = events.iter().filter(|e| e.agent == s.agent).count() as u64;
    }

    let tz = local_tz();
    if let Some(n) = opts.days {
        events = filter_days(events, &tz, n);
    }
    let agg = aggregate(&events, opts.by, &tz, &pricing);
    Ok(SummaryReport {
        by: agg.by,
        groups: agg.groups,
        totals: agg.totals,
        sources,
        warnings,
        generated_at: jiff::Zoned::now().with_time_zone(tz).to_string(),
    })
}

fn file_size(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path).map(|m| m.len()).ok()
}

/// 各 agent 来源状态（GUI 设置页；只读统计 jsonl 数量）。
#[derive(Debug, Serialize)]
pub struct SourceStatus {
    pub agent: AgentKind,
    pub dir: String,
    pub exists: bool,
    pub files: u64,
}

pub fn source_status(
    claude_dir: Option<PathBuf>,
    codex_dir: Option<PathBuf>,
) -> Result<Vec<SourceStatus>> {
    let mut out = Vec::new();
    for (kind, dir) in [
        (AgentKind::ClaudeCode, claude_dir),
        (AgentKind::Codex, codex_dir),
    ] {
        let root = match dir {
            Some(p) => p,
            None => make_source(kind, &None)?.root().to_path_buf(),
        };
        out.push(SourceStatus {
            agent: kind,
            dir: root.display().to_string(),
            exists: root.is_dir(),
            files: count_jsonl(&root),
        });
    }
    Ok(out)
}

fn count_jsonl(root: &std::path::Path) -> u64 {
    let mut n = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if crate::source::is_jsonl(&p) {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(agent: &str, p: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(agent)
            .join(p)
    }

    fn opts(cache: Option<PathBuf>, pricing: Option<PathBuf>, refresh: bool) -> SummaryOptions {
        SummaryOptions {
            by: GroupBy::Day,
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: cache,
            pricing_path: pricing,
            // 固定指向不存在的快照，测试不依赖真实 ~/.tokenscope 状态
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            refresh,
            ..Default::default()
        }
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-m4-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 剥离时间戳后序列化，用于三路径一致性比较。
    fn normalize(r: &SummaryReport) -> String {
        let mut r = r.clone();
        r.generated_at = String::new();
        serde_json::to_string(&r).unwrap()
    }

    #[test]
    fn test_report_pipeline_both_agents() {
        let r = summary(&opts(None, None, false)).unwrap();
        assert_eq!(r.by, "day");
        assert_eq!(r.sources.len(), 2);
        assert_eq!(r.totals.requests, 7); // claude 3 + codex 4（去重后）
        assert_eq!(r.groups.len(), 3); // 07-17、07-18、合计
        assert_eq!(r.groups[0].agents, ["claude-code", "codex"]);
        assert!(r.generated_at.contains('+'), "generated_at 带时区偏移");
        let j = r.to_json().unwrap();
        let v: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(v["sources"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_report_pipeline_agent_filter() {
        let opts = SummaryOptions {
            agent: Some(AgentKind::ClaudeCode),
            claude_dir: Some(fixture("claude", "basic")),
            ..opts(None, None, false)
        };
        let r = summary(&opts).unwrap();
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.sources[0].agent, AgentKind::ClaudeCode);
        assert_eq!(r.totals.requests, 3);
    }

    #[test]
    fn test_report_pipeline_missing_dir_warns() {
        let opts = SummaryOptions {
            agent: Some(AgentKind::Codex),
            codex_dir: Some(fixture("codex", "no-such-dir")),
            ..opts(None, None, false)
        };
        let r = summary(&opts).unwrap();
        assert_eq!(r.totals.requests, 0);
        assert_eq!(r.warnings.len(), 1);
        assert!(r.warnings[0].contains("不存在"));
    }

    #[test]
    fn test_report_cache_consistency() {
        // M4 核心验收：无缓存 / 缓存命中 / --refresh 三路径数字逐字段一致。
        let dir = tmp_dir("consistency");
        let cache_dir = Some(dir.clone());
        let cold = normalize(&summary(&opts(cache_dir.clone(), None, false)).unwrap());
        let warm = normalize(&summary(&opts(cache_dir.clone(), None, false)).unwrap());
        let refresh = normalize(&summary(&opts(cache_dir.clone(), None, true)).unwrap());
        assert_eq!(cold, warm, "缓存命中必须与首次全量一致");
        assert_eq!(cold, refresh, "--refresh 重建后必须一致");
        // 缓存里确实有数据（解析层未去重总数：claude 5 + codex 5）
        let info = cache_stats(Some(dir.clone())).unwrap();
        assert_eq!(info.files, 4); // claude 2 + codex 2
        assert_eq!(info.events, 10);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_report_cache_invalidation_on_change() {
        // 修改文件后（指纹变化）缓存必须失效重解析。
        let dir = tmp_dir("invalidate");
        let src_dir = dir.join("claude").join("proj-x");
        std::fs::create_dir_all(&src_dir).unwrap();
        let file = src_dir.join("s.jsonl");
        std::fs::write(
            &file,
            format!(
                "{}\n",
                r#"{"type":"assistant","timestamp":"2026-07-17T08:00:00.000Z","sessionId":"s1","message":{"id":"m1","model":"m","usage":{"input_tokens":1,"output_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#
            ),
        )
        .unwrap();
        let mk_opts = || SummaryOptions {
            agent: Some(AgentKind::ClaudeCode),
            claude_dir: Some(src_dir.clone()),
            cache_dir: Some(dir.join("cache")),
            pricing_path: Some(dir.join("no-pricing.toml")),
            openrouter_path: Some(dir.join("no-snapshot.json")),
            ..Default::default()
        };
        let first = summary(&mk_opts()).unwrap();
        assert_eq!(first.totals.requests, 1);
        let second = summary(&mk_opts()).unwrap();
        assert_eq!(second.totals.requests, 1, "缓存命中路径数字不变");
        // 追加一行 → mtime 变化 → 失效重解析
        std::thread::sleep(std::time::Duration::from_millis(50));
        use std::io::Write as _;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&file)
            .unwrap();
        writeln!(
            f,
            r#"{{"type":"assistant","timestamp":"2026-07-17T09:00:00.000Z","sessionId":"s1","message":{{"id":"m2","model":"m","usage":{{"input_tokens":5,"output_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        )
        .unwrap();
        drop(f);
        let third = summary(&mk_opts()).unwrap();
        assert_eq!(third.totals.requests, 2, "指纹变化后必须重解析");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_report_cache_corrupt_fallback() {
        // 坏缓存文件 → 警告 + 数字仍正确（纯优化原则）。
        let dir = tmp_dir("corrupt");
        std::fs::write(dir.join("cache.db"), b"this is not a sqlite database").unwrap();
        let r = summary(&opts(Some(dir.clone()), None, false)).unwrap();
        assert_eq!(r.totals.requests, 7);
        assert!(
            r.warnings.iter().any(|w| w.contains("缓存")),
            "应有缓存降级警告: {:?}",
            r.warnings
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_report_pricing_external_override() {
        // 外置价格覆盖内置：sonnet-4-5 内置 3/15/3.75/0.3，外置改 99/99/0/0。
        let dir = tmp_dir("pricing");
        let pricing = dir.join("pricing.toml");
        std::fs::write(
            &pricing,
            r#"
[[model]]
prefix = "claude-sonnet-4-5"
input = 99.0
output = 99.0
cache_write = 0.0
cache_read = 0.0
"#,
        )
        .unwrap();
        let o = opts(Some(dir.join("cache")), Some(pricing), false);
        let r = summary(&o).unwrap();
        // 07-17 组：sonnet-4-5 input 1000 / output 200 / cw 5000 / cr 10000，
        // 外置价后费用 = (1000*99 + 200*99) / 1M；另有 codex gpt-5.6-sol 6220/1M。
        let expected = (1000.0 * 99.0 + 200.0 * 99.0 + 6220.0) / 1_000_000.0;
        assert!((r.groups[0].cost_usd - expected).abs() < 1e-9);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_source_status() {
        let st = source_status(
            Some(fixture("claude", "basic")),
            Some(fixture("codex", "no-such-dir")),
        )
        .unwrap();
        assert_eq!(st.len(), 2);
        assert!(st[0].exists);
        assert_eq!(st[0].files, 2);
        assert!(!st[1].exists);
        assert_eq!(st[1].files, 0);
    }
}
