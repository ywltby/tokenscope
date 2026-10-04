//! CLI / GUI 共用的汇总管线：组装源 → 按文件缓存增量采集 → 全局去重 → 过滤 → 聚合。
//! 两端只做参数转换与渲染，数字永远同源。
//!
//! M4 缓存原则：**缓存是纯优化，不是事实源**——任何缓存故障都降级为全量内存
//! 扫描并告警，数字必须与无缓存一致。

use std::path::PathBuf;

use anyhow::Result;
use jiff::tz::TimeZone;
use serde::Serialize;

use crate::aggregate::{GroupBy, aggregate, filter_days, resolve_tz};
use crate::cache::{Cache, CacheStats, mtime_ms};
use crate::dedupe::dedupe_events;
use crate::model::{AgentKind, TokenCounts, UsageEvent};
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
    /// models.dev 快照文件路径（None = `~/.tokenscope/pricing-modelsdev.json`；测试注入用）。
    pub modelsdev_path: Option<PathBuf>,
    /// 聚合与展示时区：None = 默认 Asia/Shanghai；"local" = 本机；其余按 IANA 名。
    pub tz: Option<String>,
    /// 自然日区间下界（YYYY-MM-DD，解析时区，闭区间；与 days 互斥）。
    pub from: Option<String>,
    /// 自然日区间上界（YYYY-MM-DD，解析时区，闭区间；与 days 互斥）。
    pub to: Option<String>,
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
    /// 解析后的时区标识（M6）。
    pub timezone: String,
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

pub fn view_cache_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("view-cache.json"))
}

pub fn pricing_index_path(_pricing_file: Option<&PathBuf>) -> PathBuf {
    data_dir()
        .map(|d| d.join("pricing-index.json"))
        .unwrap_or_else(|_| PathBuf::from("pricing-index.json"))
}

pub fn modelsdev_file_path(snapshot_file: Option<&PathBuf>) -> PathBuf {
    snapshot_file
        .cloned()
        .or_else(|| data_dir().ok().map(|d| d.join("pricing-modelsdev.json")))
        .unwrap_or_else(|| PathBuf::from("pricing-modelsdev.json"))
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

/// 采集产物：去重后事件 + 逐源统计 + 价格表 + 警告。summary / list_events 共用。
struct Collected {
    events: Vec<UsageEvent>,
    sources: Vec<SourceReport>,
    pricing: std::sync::Arc<crate::pricing::Pricing>,
    warnings: Vec<String>,
}

/// 共用采集路径（M7）：价格加载 → 缓存增量采集 → 全局去重 → 回填统计。
fn collect_all(opts: &SummaryOptions) -> Result<Collected> {
    let pricing_path = pricing_file_path(opts.pricing_path.as_ref());
    let openrouter_path = openrouter_file_path(opts.openrouter_path.as_ref());
    let modelsdev_path = modelsdev_file_path(opts.modelsdev_path.as_ref());
    let index_path = pricing_index_path(opts.pricing_path.as_ref());
    // M11：签名一致时复用进程内缓存/索引文件，仅签名变化才重解析双快照。
    let (pricing, mut warnings, _cache_hit) = Pricing::load_cached(
        Some(&pricing_path),
        Some(&modelsdev_path),
        Some(&openrouter_path),
        &index_path,
    );
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
    let (events, dropped) = dedupe_events(all_events);
    for s in &mut sources {
        s.stats.duplicates_dropped = dropped
            .iter()
            .find(|(a, _)| *a == s.agent)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        s.stats.events = events.iter().filter(|e| e.agent == s.agent).count() as u64;
    }
    Ok(Collected {
        events,
        sources,
        pricing,
        warnings,
    })
}

pub fn summary(opts: &SummaryOptions) -> Result<SummaryReport> {
    let Collected {
        mut events,
        sources,
        pricing,
        warnings,
    } = collect_all(opts)?;

    let (tz, tz_label) = resolve_tz(opts.tz.as_deref())?;
    events = apply_time_filter(events, opts, &tz)?;
    let agg = aggregate(&events, opts.by, &tz, &pricing);
    Ok(SummaryReport {
        timezone: tz_label,
        by: agg.by,
        groups: agg.groups,
        totals: agg.totals,
        sources,
        warnings,
        generated_at: jiff::Zoned::now().with_time_zone(tz).to_string(),
    })
}

/// 明细过滤条件（M7）：在 SummaryOptions 的 agent/days/tz 之上叠加行级过滤。
#[derive(Debug, Default, Clone)]
pub struct EventFilter {
    pub model: Option<String>,
    pub project: Option<String>,
    /// 自然日（解析时区的 YYYY-MM-DD）。
    pub day: Option<String>,
    /// 返回条数上限：None = 200，最大 1000。
    pub limit: Option<usize>,
}

/// 一条去重后的用量明细（展示行）。
#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    /// 解析时区下的 "YYYY-MM-DD HH:MM:SS"（存储仍 UTC，见 cache.rs）。
    pub ts: String,
    pub agent: &'static str,
    pub model: String,
    pub session_id: String,
    pub project: String,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    /// None = 模型无价格（unknown），不按 0。
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventList {
    pub rows: Vec<EventRow>,
    /// 过滤后、截断前的总条数。
    pub total: u64,
    pub warnings: Vec<String>,
}

/// 逐请求明细（M7）：与 summary 共用 collect_all 采集与去重路径，数字同源。
pub fn list_events(opts: &SummaryOptions, filter: &EventFilter) -> Result<EventList> {
    let Collected {
        mut events,
        pricing,
        warnings,
        ..
    } = collect_all(opts)?;
    let (tz, _) = resolve_tz(opts.tz.as_deref())?;
    if let Some(n) = opts.days {
        events = filter_days(events, &tz, n);
    }
    if let Some(day) = &filter.day {
        events.retain(|e| e.ts.to_zoned(tz.clone()).date().to_string() == *day);
    }
    if let Some(m) = &filter.model {
        events.retain(|e| &e.model == m);
    }
    if let Some(pr) = &filter.project {
        events.retain(|e| &e.project == pr);
    }
    events.sort_by_key(|e| std::cmp::Reverse(e.ts));
    let total = events.len() as u64;
    let limit = filter.limit.unwrap_or(200).min(1000);
    let rows = events
        .into_iter()
        .take(limit)
        .map(|e| {
            let cost_usd = pricing.cost(&e.model, &TokenCounts::from_event(&e));
            EventRow {
                ts: e.ts.to_zoned(tz.clone()).strftime("%F %T").to_string(),
                agent: e.agent.as_str(),
                model: e.model,
                session_id: e.session_id,
                project: e.project,
                input: e.input_tokens,
                output: e.output_tokens,
                cache_write: e.cache_write_tokens,
                cache_read: e.cache_read_tokens,
                cost_usd,
            }
        })
        .collect();
    Ok(EventList {
        rows,
        total,
        warnings,
    })
}

/// 统一时间过滤（M10）：days（预设近 N 天）与 from/to（闭区间自然日）二选一，
/// 日期按解析时区解释。
fn apply_time_filter(
    mut events: Vec<UsageEvent>,
    opts: &SummaryOptions,
    tz: &TimeZone,
) -> Result<Vec<UsageEvent>> {
    use jiff::civil::Date;
    if opts.days.is_some() && (opts.from.is_some() || opts.to.is_some()) {
        anyhow::bail!("--days 与 --from/--to 互斥，二选一");
    }
    if let Some(n) = opts.days {
        return Ok(filter_days(events, tz, n));
    }
    if opts.from.is_none() && opts.to.is_none() {
        return Ok(events);
    }
    let parse = |s: &str| -> Result<Date> {
        s.parse::<Date>()
            .map_err(|e| anyhow::anyhow!("日期格式应为 YYYY-MM-DD: {s:?}（{e}）"))
    };
    let from = opts.from.as_deref().map(parse).transpose()?;
    let to = opts.to.as_deref().map(parse).transpose()?;
    if let (Some(f), Some(t)) = (from, to)
        && f > t
    {
        anyhow::bail!("起始日期晚于结束日期: {f} > {t}");
    }
    events.retain(|e| {
        let d = e.ts.to_zoned(tz.clone()).date();
        from.is_none_or(|f| d >= f) && to.is_none_or(|t| d <= t)
    });
    Ok(events)
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
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
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
            modelsdev_path: Some(dir.join("no-modelsdev.json")),
            tz: Some("Asia/Shanghai".to_string()),
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
    fn test_apply_time_filter_range() {
        let tz = jiff::tz::TimeZone::get("Asia/Shanghai").unwrap();
        let mk = |ts: &str| UsageEvent {
            ts: ts.parse().unwrap(),
            agent: AgentKind::ClaudeCode,
            model: "m".into(),
            session_id: "s".into(),
            project: "p".into(),
            record_id: String::new(),
            input_tokens: 1,
            output_tokens: 1,
            cache_write_tokens: 0,
            cache_read_tokens: 0,
        };
        let events = vec![
            mk("2026-08-01T10:00:00Z"),
            mk("2026-08-02T12:00:00Z"),
            mk("2026-08-13T23:00:00Z"),
        ];
        let opts = |from: Option<String>, to: Option<String>| SummaryOptions {
            from,
            to,
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        // 闭区间含端点（上海时区：08-01T10:00Z = 当日 18:00）
        let r = apply_time_filter(
            events.clone(),
            &opts(Some("2026-08-01".into()), Some("2026-08-01".into())),
            &tz,
        )
        .unwrap();
        assert_eq!(r.len(), 1);
        // 只给 from / 只给 to
        assert_eq!(
            apply_time_filter(events.clone(), &opts(Some("2026-08-02".into()), None), &tz)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            apply_time_filter(events.clone(), &opts(None, Some("2026-08-01".into())), &tz)
                .unwrap()
                .len(),
            1
        );
        // from > to 报错；days 互斥报错；非法日期报错
        assert!(
            apply_time_filter(
                events.clone(),
                &opts(Some("2026-08-13".into()), Some("2026-08-01".into())),
                &tz
            )
            .is_err()
        );
        let both = SummaryOptions {
            days: Some(7),
            from: Some("2026-08-01".into()),
            ..opts(None, None)
        };
        assert!(apply_time_filter(events.clone(), &both, &tz).is_err());
        let bad = opts(Some("2026/08/01".into()), None);
        assert!(apply_time_filter(events.clone(), &bad, &tz).is_err());
    }

    #[test]
    fn test_report_tz_resolution() {
        // 缺省 = Asia/Shanghai；显式 UTC 改变解析标识；非法名 = 参数错误。
        let base = SummaryOptions {
            by: GroupBy::Day,
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            ..Default::default()
        };
        let sh = summary(&SummaryOptions {
            tz: Some("Asia/Shanghai".into()),
            ..base.clone()
        })
        .unwrap();
        assert_eq!(sh.timezone, "Asia/Shanghai");
        assert_eq!(sh.groups[0].key, "2026-07-17");

        let utc = summary(&SummaryOptions {
            tz: Some("UTC".into()),
            ..base.clone()
        })
        .unwrap();
        assert_eq!(utc.timezone, "UTC");

        let local = summary(&SummaryOptions {
            tz: Some("local".into()),
            ..base.clone()
        })
        .unwrap();
        assert!(!local.timezone.is_empty());

        let bad = summary(&SummaryOptions {
            tz: Some("Mars/Olympus".into()),
            ..base
        });
        assert!(bad.is_err(), "非法时区应为参数错误");
    }

    #[test]
    fn test_list_events_filters() {
        let base = SummaryOptions {
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            // 钉住不存在的 models.dev 快照，测试不依赖真实 ~/.tokenscope 状态
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        // 全量：去重后 7 行（与汇总 requests 一致），按时间倒序
        let (all_ok, list) = {
            let l = list_events(&base, &EventFilter::default()).unwrap();
            (l.total == 7, l)
        };
        assert!(all_ok, "total={}", list.total);
        assert_eq!(list.rows.len(), 7);
        assert_eq!(list.warnings.len(), 0);
        let tss: Vec<&str> = list.rows.iter().map(|r| r.ts.as_str()).collect();
        let mut sorted = tss.clone();
        sorted.sort();
        sorted.reverse();
        assert_eq!(tss, sorted, "必须按时间倒序");

        // 模型过滤：gpt-5.6-sol 2 行
        let l = list_events(
            &base,
            &EventFilter {
                model: Some("gpt-5.6-sol".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(l.total, 2);
        assert!(l.rows.iter().all(|r| r.model == "gpt-5.6-sol"));

        // 项目过滤：proj-beta 1 行（codex e3）
        let l = list_events(
            &base,
            &EventFilter {
                project: Some("beta".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(l.total, 1);

        // 日过滤：2026-07-17（本地日）= claude msg-1 + codex e1 = 2 行
        let l = list_events(
            &base,
            &EventFilter {
                day: Some("2026-07-17".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(l.total, 2);

        // limit 截断与 total 语义
        let l = list_events(
            &base,
            &EventFilter {
                limit: Some(3),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(l.rows.len(), 3);
        assert_eq!(l.total, 7);

        // unknown 费用：grok-4.5-build 行 cost=None（fixture 用 hy3:free？不，
        // claude fixture 的 unknown 模型是 tencent/hy3:free）
        let unknown: Vec<&EventRow> = list.rows.iter().filter(|r| r.cost_usd.is_none()).collect();
        assert!(!unknown.is_empty(), "tencent/hy3:free 行应为 unknown 费用");
        assert!(unknown.iter().all(|r| r.model == "tencent/hy3:free"));
    }

    #[test]
    fn test_list_events_dedupe_consistent_with_summary() {
        // 明细与汇总同源：total（过滤后）== 汇总同过滤的 requests。
        let base = SummaryOptions {
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let l = list_events(&base, &EventFilter::default()).unwrap();
        let s = summary(&base).unwrap();
        assert_eq!(l.total as u64, s.totals.requests);
    }

    #[test]
    fn test_report_modelsdev_layer() {
        // models.dev 层兜住 openrouter 缺的 doubao；快照缺失时回内置/unknown。
        let dir = tmp_dir("modelsdev");
        let snapshot = dir.join("pricing-modelsdev.json");
        std::fs::write(
            &snapshot,
            r#"{"synced_at":"t","entries":[
                {"id":"volcengine/doubao-seed-2-0-pro-260215","name":"Doubao Pro",
                 "input":0.47,"output":2.37,"cache_read":0.09,"cache_write":0.0},
                {"id":"tencent/hy3:free","name":"HY3 free",
                 "input":0,"output":0,"cache_read":0,"cache_write":0}
            ]}"#,
        )
        .unwrap();
        let base = SummaryOptions {
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: Some(dir.join("cache")),
            pricing_path: Some(dir.join("no-pricing.toml")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(snapshot),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let r = summary(&base).unwrap();
        assert!(
            r.totals.unknown_tokens.total() == 0,
            "doubao 经 models.dev 层入价: {r:?}"
        );
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
