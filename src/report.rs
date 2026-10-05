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
    /// 价格索引快照路径（None = `~/.tokenscope/pricing-index.json`；测试注入用）。
    /// 密闭性修复（2026-10-05）：此前索引路径恒指真实数据目录，测试会把
    /// 测试签名的索引写进用户目录，导致 GUI 每次启动都重建索引。
    pub pricing_index: Option<PathBuf>,
    /// 聚合与展示时区：None = 默认 Asia/Shanghai；"local" = 本机；其余按 IANA 名。
    pub tz: Option<String>,
    /// 自然日区间下界（YYYY-MM-DD，解析时区，闭区间；与 days 互斥）。
    pub from: Option<String>,
    /// 自然日区间上界（YYYY-MM-DD，解析时区，闭区间；与 days 互斥）。
    pub to: Option<String>,
    /// 强制全量重解析并重建缓存。
    pub refresh: bool,
    /// C1：来源启停（GUI 从设置注入；None = 启用）。停用的来源完全不采集，
    /// 也不出现在来源统计里。
    pub claude_enabled: Option<bool>,
    pub codex_enabled: Option<bool>,
}

impl SummaryOptions {
    fn enabled(&self, claude: bool) -> bool {
        if claude {
            self.claude_enabled != Some(false)
        } else {
            self.codex_enabled != Some(false)
        }
    }
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

pub fn pricing_index_path(index_file: Option<&PathBuf>) -> PathBuf {
    index_file
        .cloned()
        .or_else(|| data_dir().ok().map(|d| d.join("pricing-index.json")))
        .unwrap_or_else(|| PathBuf::from("pricing-index.json"))
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
/// 任何降级路径都同步 warn 落日志——纯优化失效必须可追溯。
fn open_cache(opts: &SummaryOptions) -> (Option<Cache>, Vec<String>) {
    let mut warnings = Vec::new();
    if opts.cache_dir.is_none() && data_dir().is_err() {
        let msg = "无法定位数据目录，已退回全量扫描（不使用缓存）";
        log::warn!("缓存降级: {msg}");
        warnings.push(msg.to_string());
        return (None, warnings);
    }
    let path = cache_file_path(opts.cache_dir.as_ref());
    match Cache::open(&path) {
        Ok(c) => {
            if opts.refresh
                && let Err(e) = c.clear()
            {
                let msg = format!("缓存重置失败，已退回全量扫描: {e}");
                log::warn!("缓存降级: {msg}");
                warnings.push(msg);
                return (None, warnings);
            }
            (Some(c), warnings)
        }
        Err(e) => {
            let msg = format!("缓存打开失败，已退回全量扫描: {e:#}");
            log::warn!("缓存降级: {msg}");
            warnings.push(msg);
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
/// 各阶段 INFO 计时落日志（用户排障依据；粒度 = 每 agent 一行，不逐文件刷屏）。
fn collect_all(opts: &SummaryOptions) -> Result<Collected> {
    let kinds: Vec<AgentKind> = match opts.agent {
        Some(k) => vec![k],
        None => vec![AgentKind::ClaudeCode, AgentKind::Codex],
    };
    let mut sources: Vec<Box<dyn Source>> = Vec::new();
    for kind in kinds {
        if !opts.enabled(kind == AgentKind::ClaudeCode) {
            continue; // C1：停用的来源零采集、零告警，状态由 source_status 呈现
        }
        let dir = match kind {
            AgentKind::ClaudeCode => &opts.claude_dir,
            AgentKind::Codex => &opts.codex_dir,
        };
        sources.push(make_source(kind, dir)?);
    }
    collect_all_with_sources(sources, opts)
}

/// 可注入来源的采集实现（测试用 MockSource 走同一管线）。
fn collect_all_with_sources(
    sources: Vec<Box<dyn Source>>,
    opts: &SummaryOptions,
) -> Result<Collected> {
    let t_total = std::time::Instant::now();
    let pricing_path = pricing_file_path(opts.pricing_path.as_ref());
    let openrouter_path = openrouter_file_path(opts.openrouter_path.as_ref());
    let modelsdev_path = modelsdev_file_path(opts.modelsdev_path.as_ref());
    let index_path = pricing_index_path(opts.pricing_index.as_ref());
    // M11：签名一致时复用进程内缓存/索引文件，仅签名变化才重解析双快照。
    let t_pricing = std::time::Instant::now();
    let (pricing, mut warnings, cache_hit) = Pricing::load_cached(
        Some(&pricing_path),
        Some(&modelsdev_path),
        Some(&openrouter_path),
        &index_path,
    );
    log::info!(
        "价格加载：{}，{:.0} ms",
        if cache_hit {
            "进程内缓存命中"
        } else {
            "重建（快照/索引）"
        },
        t_pricing.elapsed().as_millis()
    );
    let (cache, mut cache_warnings) = open_cache(opts);
    warnings.append(&mut cache_warnings);

    let mut reports: Vec<SourceReport> = Vec::new();
    let mut all_events: Vec<UsageEvent> = Vec::new();

    for src in sources {
        let kind = src.agent();
        let t_agent = std::time::Instant::now();
        let mut stats = CollectStats::default();
        let mut events: Vec<UsageEvent> = Vec::new();
        let (files, discovery_errors) = src.discover_with_errors();
        // B4：发现期异常必须可见；发现失败的来源不参与缓存清理。
        for e in &discovery_errors {
            warnings.push(e.clone());
        }
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
        let mut agent_keep: Vec<String> = Vec::new();
        let mut cached_hits = 0u32;
        let mut reparsed = 0u32;
        let mut parse_ms_total = 0u128;
        let mut store_ms_total = 0u128;
        let mut lookup_errs = 0u32;
        let mut store_errs = 0u32;
        let mut unstable = 0u32;
        for file in &files {
            let path_str = file.display().to_string();
            agent_keep.push(path_str.clone());
            let mut cached: Option<crate::source::FileParse> = None;
            if !opts.refresh
                && let (Some(c), Some(size), Ok(mt)) =
                    (cache.as_ref(), file_size(file), mtime_ms(file))
            {
                match c.lookup_file(&path_str, kind, size, mt) {
                    Ok(hit) => cached = hit.map(|cf| cf.parse),
                    Err(e) => {
                        lookup_errs += 1;
                        warnings.push(format!("缓存读取失败（该文件全量解析）: {e:#}"));
                    }
                }
            }
            let parse = match cached {
                Some(p) => {
                    cached_hits += 1;
                    p
                }
                None => {
                    // B4：解析前取指纹，解析后复核——追加中的文件不写"成功"
                    // 缓存（否则旧数据配上新指纹，此后永远命中陈旧内容）。
                    let fp_before = if cache.is_some() {
                        Some((file_size(file), mtime_ms(file).ok()))
                    } else {
                        None
                    };
                    let t = std::time::Instant::now();
                    let p = src.parse_file(file);
                    parse_ms_total += t.elapsed().as_millis();
                    reparsed += 1;
                    if let Some(c) = &cache {
                        let fp_after = (file_size(file), mtime_ms(file).ok());
                        let stable = fp_before == Some(fp_after);
                        if !stable {
                            unstable += 1;
                            warnings
                                .push(format!("采集期间文件变化，本轮不计入缓存: {}", path_str));
                        } else if p.stats.io_errors > 0 {
                            warnings.push(format!(
                                "文件读取失败，本轮不缓存（不影响统计）: {}",
                                path_str
                            ));
                        } else {
                            let size = fp_after.0.unwrap_or(0);
                            let mt = fp_after.1.unwrap_or(0);
                            let t = std::time::Instant::now();
                            if let Err(e) = c.store_file(&path_str, kind, size, mt, &p) {
                                store_errs += 1;
                                warnings.push(format!("缓存写入失败（不影响统计）: {e:#}"));
                            }
                            store_ms_total += t.elapsed().as_millis();
                        }
                    }
                    p
                }
            };
            stats.add_file(&parse);
            events.extend(parse.events);
        }
        stats.files_scanned = files.len() as u64;
        log::info!(
            "采集 {}: 文件 {}（缓存命中 {} / 重解析 {}），解析 {} ms，写缓存 {} ms，解析层事件 {}，累计 {} ms",
            kind.as_str(),
            files.len(),
            cached_hits,
            reparsed,
            parse_ms_total,
            store_ms_total,
            events.len(),
            t_agent.elapsed().as_millis()
        );
        // 缓存错误只聚合一行 warn（逐文件明细已进 warnings 随报告返回前端）。
        if lookup_errs > 0 {
            log::warn!(
                "采集 {}: {lookup_errs} 个文件缓存读取失败，均已回退全量解析",
                kind.as_str()
            );
        }
        if store_errs > 0 {
            log::warn!(
                "采集 {}: {store_errs} 个文件缓存写入失败（不影响统计）",
                kind.as_str()
            );
        }
        if unstable > 0 {
            log::warn!(
                "采集 {}: {unstable} 个文件采集期间持续变化，本轮未入缓存",
                kind.as_str()
            );
        }
        // B5/F06：清理只作用于本来源；发现失败绝不清理（无法区分"已删除"
        // 与"暂时读不到"）。
        if let Some(c) = &cache
            && discovery_errors.is_empty()
            && src.root().is_dir()
        {
            let t = std::time::Instant::now();
            match c.purge_agent(kind, &agent_keep) {
                Ok(n) => {
                    if n > 0 {
                        log::info!(
                            "缓存清理（{}）：{} 条过期行，{} ms",
                            kind.as_str(),
                            n,
                            t.elapsed().as_millis()
                        );
                    }
                }
                Err(e) => warnings.push(format!("缓存清理失败（不影响统计）: {e:#}")),
            }
        }
        reports.push(SourceReport { agent: kind, stats });
        all_events.extend(events);
    }

    // 全局去重（跨文件、按 agent 规则），并回填 per-agent 的丢弃数与事件数。
    let t_dedupe = std::time::Instant::now();
    let before = all_events.len();
    let (events, dropped) = dedupe_events(all_events);
    log::info!(
        "去重：{} → {}（丢弃 {}），{} ms",
        before,
        events.len(),
        dropped.iter().map(|(_, n)| n).sum::<u64>(),
        t_dedupe.elapsed().as_millis()
    );
    for s in &mut reports {
        s.stats.duplicates_dropped = dropped
            .iter()
            .find(|(a, _)| *a == s.agent)
            .map(|(_, n)| *n)
            .unwrap_or(0);
        s.stats.events = events.iter().filter(|e| e.agent == s.agent).count() as u64;
    }
    log::info!(
        "采集完成：事件 {}，警告 {}，总计 {} ms",
        events.len(),
        warnings.len(),
        t_total.elapsed().as_millis()
    );
    Ok(Collected {
        events,
        sources: reports,
        pricing,
        warnings,
    })
}

pub fn summary(opts: &SummaryOptions) -> Result<SummaryReport> {
    let t = std::time::Instant::now();
    let Collected {
        mut events,
        sources,
        pricing,
        warnings,
    } = collect_all(opts)?;

    let (tz, tz_label) = resolve_tz(opts.tz.as_deref())?;
    events = apply_time_filter(events, opts, &tz)?;
    let t_agg = std::time::Instant::now();
    let agg = aggregate(&events, opts.by, &tz, &pricing);
    log::info!(
        "聚合（{}）：{} 组 / {} 请求，{} ms",
        agg.by,
        agg.groups.len(),
        agg.totals.requests,
        t_agg.elapsed().as_millis()
    );
    log::info!(
        "汇总完成：by={} agent={} 天数={:?} 区间={:?}..{:?} 时区={}，{} ms",
        agg.by,
        opts.agent.as_ref().map(|k| k.as_str()).unwrap_or("all"),
        opts.days,
        opts.from,
        opts.to,
        tz_label,
        t.elapsed().as_millis()
    );
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
    let t = std::time::Instant::now();
    let Collected {
        mut events,
        pricing,
        warnings,
        ..
    } = collect_all(opts)?;
    let (tz, _) = resolve_tz(opts.tz.as_deref())?;
    // F02（计划 A3）：明细先复用与汇总完全相同的主时间过滤（days / from-to
    // 同一校验与口径），再叠加行级下钻——此前只应用 days，前端传的 from/to
    // 被静默忽略，选择历史区间后明细与汇总范围不一致。
    events = apply_time_filter(events, opts, &tz)?;
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
    let rows: Vec<EventRow> = events
        .into_iter()
        .take(limit)
        .map(|e| {
            // B3：部分计价模型的明细行展示已计价小计（unknown 分项随总计披露）。
            let cost_usd = pricing
                .estimate(&e.model, &TokenCounts::from_event(&e))
                .map(|est| est.cost);
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
    log::info!(
        "明细完成：筛选 模型={:?} 项目={:?} 日={:?}，返回 {} 行 / 共 {} 条，{} ms",
        filter.model,
        filter.project,
        filter.day,
        rows.len(),
        total,
        t.elapsed().as_millis()
    );
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
/// state（C1）：disabled=已停用；missing=目录不存在；empty=无日志；
/// ready=正常。四态可区分是"首用纠正路径"的基础（计划 3.3.2）。
#[derive(Debug, Serialize)]
pub struct SourceStatus {
    pub agent: AgentKind,
    pub dir: String,
    pub enabled: bool,
    pub exists: bool,
    pub files: u64,
    pub state: &'static str,
}

pub fn source_status(settings: &crate::settings::Settings) -> Result<Vec<SourceStatus>> {
    let mut out = Vec::new();
    for claude in [true, false] {
        let kind = if claude {
            AgentKind::ClaudeCode
        } else {
            AgentKind::Codex
        };
        let cfg = settings.source_config(claude);
        let dir: Option<PathBuf> = cfg.dir.map(PathBuf::from);
        let root = match dir {
            Some(p) => p,
            None => make_source(kind, &None)?.root().to_path_buf(),
        };
        let exists = root.is_dir();
        let files = count_jsonl(&root);
        let state = if !cfg.enabled {
            "disabled"
        } else if !exists {
            "missing"
        } else if files == 0 {
            "empty"
        } else {
            "ready"
        };
        out.push(SourceStatus {
            agent: kind,
            dir: root.display().to_string(),
            enabled: cfg.enabled,
            exists,
            files,
            state,
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
        // 密闭性（2026-10-05 修复）：cache=None 也绝不落回真实 ~/.tokenscope——
        // 此前默认路径会把 fixture 写进用户 cache.db 并 purge 掉全部真实行，
        // 导致 GUI 每次启动都全量冷扫描（1.2 GB 日志，分钟级加载）。
        let cache_dir =
            Some(cache.unwrap_or_else(|| {
                tmp_dir(&format!("hermetic-{:?}", std::thread::current().id()))
            }));
        let pricing_index = cache_dir.as_ref().map(|d| d.join("pricing-index.json"));
        SummaryOptions {
            by: GroupBy::Day,
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir,
            pricing_index,
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
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
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
            pricing_index: Some(dir.join("pricing-index.json")),
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
        // 外置价后费用 = (1000*99 + 200*99) / 1M；另有 codex gpt-5.6-sol 6020/1M
        //（B1 语义修复：raw 1000 = 750 未缓存 + 200 读 + 50 写）。
        let expected = (1000.0 * 99.0 + 200.0 * 99.0 + 6020.0) / 1_000_000.0;
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
        let hermetic = tmp_dir("tz");
        let base = SummaryOptions {
            by: GroupBy::Day,
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: Some(hermetic.join("cache")),
            pricing_index: Some(hermetic.join("pricing-index.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
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
        let hermetic = tmp_dir("list-events");
        let base = SummaryOptions {
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: Some(hermetic.join("cache")),
            pricing_index: Some(hermetic.join("pricing-index.json")),
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

        // 项目过滤（C2：项目身份 = 完整 cwd）：codex e3 所在 beta 项目 1 行
        let l = list_events(
            &base,
            &EventFilter {
                project: Some(r"C:\work\beta".into()),
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
        let hermetic = tmp_dir("dedupe-consistent");
        let base = SummaryOptions {
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: Some(hermetic.join("cache")),
            pricing_index: Some(hermetic.join("pricing-index.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
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
            r#"{"v":2,"synced_at":"t","entries":[
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
            pricing_index: Some(dir.join("pricing-index.json")),
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
        // C1：四态可区分（ready/missing/disabled）+ 目录配置生效。
        let mut s = crate::settings::Settings::default();
        s.sources.claude = Some(crate::settings::SourceConfig {
            enabled: true,
            dir: Some(fixture("claude", "basic").display().to_string()),
        });
        s.sources.codex = Some(crate::settings::SourceConfig {
            enabled: false,
            dir: Some(fixture("codex", "no-such-dir").display().to_string()),
        });
        let st = source_status(&s).unwrap();
        assert_eq!(st.len(), 2);
        assert_eq!(st[0].state, "ready");
        assert_eq!(st[0].files, 2);
        assert_eq!(st[1].state, "disabled", "停用优先于目录状态");
        // 未配置 → 默认目录多半不存在 → missing（不依赖具体家目录，只验状态字段存在）。
        let st = source_status(&crate::settings::Settings::default()).unwrap();
        assert!(st.iter().all(|x| x.enabled));
        assert!(st.iter().all(|x| !x.state.is_empty()));
    }

    #[test]
    fn test_disabled_source_skipped() {
        // C1：停用来源零采集——events、来源统计、缓存清理都不触达。
        let dir = tmp_dir("disabled");
        let mut s = crate::settings::Settings::default();
        let opts = SummaryOptions {
            agent: None,
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            claude_enabled: Some(true),
            codex_enabled: Some(false),
            ..Default::default()
        };
        let _ = &mut s;
        let r = summary(&opts).unwrap();
        assert_eq!(r.totals.requests, 3, "只剩 claude 的 3 条");
        assert_eq!(r.sources.len(), 1);
        assert_eq!(r.sources[0].agent, AgentKind::ClaudeCode);
        std::fs::remove_dir_all(&dir).ok();
    }
}

/// B4 测试：解析期间文件被追加 → 指纹复核失败 → 不写成功缓存。
/// （独立测试模块，避免与上方 tests 的助手命名冲突。）
#[cfg(test)]
mod collect_stability_tests {
    use super::*;
    use std::path::Path;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tokenscope-b4-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    struct AppendMock {
        root: PathBuf,
        file: PathBuf,
    }

    impl Source for AppendMock {
        fn agent(&self) -> AgentKind {
            AgentKind::Codex
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
            (vec![self.file.clone()], Vec::new())
        }

        fn parse_file(&self, path: &Path) -> crate::source::FileParse {
            // 模拟"解析期间日志被源工具追加"：解析中追加一行，返回的是
            // 追加前内容的解析结果。
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
            writeln!(f, r#"{{"appended":true}}"#).unwrap();
            let stats = CollectStats {
                lines_seen: 1,
                events: 1,
                ..CollectStats::default()
            };
            crate::source::FileParse {
                stats,
                events: vec![UsageEvent {
                    ts: "2026-07-17T15:00:00Z".parse().unwrap(),
                    agent: AgentKind::Codex,
                    model: "m".into(),
                    session_id: "s".into(),
                    project: "p".into(),
                    record_id: String::new(),
                    input_tokens: 1,
                    output_tokens: 1,
                    cache_write_tokens: 0,
                    cache_read_tokens: 0,
                }],
            }
        }
    }

    #[test]
    fn test_append_during_parse_not_cached_as_complete() {
        let dir = tmp_dir("append-stability");
        let root = dir.join("codex");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("rollout-x.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        let opts = SummaryOptions {
            agent: Some(AgentKind::Codex),
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let sources: Vec<Box<dyn Source>> = vec![Box::new(AppendMock {
            root: root.clone(),
            file: file.clone(),
        })];
        let c = collect_all_with_sources(sources, &opts).unwrap();
        assert_eq!(c.events.len(), 1, "解析结果正常入账");
        assert!(
            c.warnings.iter().any(|w| w.contains("本轮不计入缓存")),
            "应有文件变化警告: {:?}",
            c.warnings
        );
        // 关键断言：不稳定文件绝不能配上"追加后"的指纹写成成功缓存——
        // 否则此后永远命中这份缺尾数据。
        let info = cache_stats(opts.cache_dir.clone()).unwrap();
        assert_eq!(info.files, 0, "采集期间变化的文件不得入缓存");
        std::fs::remove_dir_all(&dir).ok();
    }
}
