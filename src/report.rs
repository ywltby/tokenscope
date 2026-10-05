//! CLI / GUI 共用的汇总管线：组装源 → 按文件缓存增量采集 → 全局去重 → 过滤 → 聚合。
//! 两端只做参数转换与渲染，数字永远同源。
//!
//! M4 缓存原则：**缓存是纯优化，不是事实源**——任何缓存故障都降级为全量内存
//! 扫描并告警，数字必须与无缓存一致。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

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
    generation: u64,
    events: Vec<UsageEvent>,
    sources: Vec<SourceReport>,
    pricing: std::sync::Arc<crate::pricing::Pricing>,
    warnings: Vec<String>,
}

/// D1：采集快照（不可变共享）——并发同参查询复用同一次采集，
/// 事件/来源/警告/价格表一份冻结，各查询自行做时间与行级过滤。
#[derive(Clone, Debug)]
struct CollectionSnapshot {
    generation: u64,
    events: Vec<UsageEvent>,
    sources: Vec<SourceReport>,
    warnings: Vec<String>,
    pricing: std::sync::Arc<crate::pricing::Pricing>,
}

/// 单飞槽：key → (互斥结果, 条件变量)。None = 空闲。
type FlightCell = Arc<(Mutex<Option<Result<CollectionSnapshot, String>>>, Condvar)>;

/// 占用中的航班表（Task 1）：key → cell。按 key 管理——不同参数的采集
/// 可并发进行，同参跟随者共享同一航班；清理时以 cell 指针身份核对。
static INFLIGHT: std::sync::LazyLock<Mutex<std::collections::HashMap<String, FlightCell>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));
static COLLECT_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 采集键：只含影响**采集**的参数（by/tz/from/to/days 是采集后的过滤，
/// 不参与——汇总与明细同参并发时必须合并为一次采集）。
fn collection_key(opts: &SummaryOptions) -> String {
    format!(
        "agent={:?}|cd={:?}|xd={:?}|ce={:?}|xe={:?}|refresh={}|cache={:?}|pp={:?}|or={:?}|md={:?}|idx={:?}",
        opts.agent,
        opts.claude_dir,
        opts.codex_dir,
        opts.claude_enabled,
        opts.codex_enabled,
        opts.refresh,
        opts.cache_dir,
        opts.pricing_path,
        opts.openrouter_path,
        opts.modelsdev_path,
        opts.pricing_index
    )
}

fn wait_flight(cell: &FlightCell) -> Result<Arc<CollectionSnapshot>> {
    let mut r = cell.0.lock().unwrap();
    loop {
        if let Some(res) = r.as_ref() {
            return match res {
                Ok(s) => Ok(Arc::new(s.clone())),
                Err(e) => Err(anyhow::anyhow!(e.clone())),
            };
        }
        r = cell.1.wait(r).unwrap();
    }
}

/// 单飞入口：领队采集并发布快照；同参跟随者等待复用。
/// Task 1（RAII）：成功、错误、panic 三条路径都发布结果并释放槽位——
/// 失败必须可重试，等待者必须拿到真实错误而非悬挂。
fn collect_flighted(opts: &SummaryOptions) -> Result<Arc<CollectionSnapshot>> {
    collect_flighted_with(opts, &|generation| collect_inner(opts, generation))
}

/// Task 2：来源包装器——discover 结果已被防御性去重（固定文件清单）。
struct DedupSource {
    inner: Box<dyn Source>,
    keep: Vec<PathBuf>,
    errors: Vec<String>,
}

impl Source for DedupSource {
    fn agent(&self) -> AgentKind {
        self.inner.agent()
    }

    fn root(&self) -> &std::path::Path {
        self.inner.root()
    }

    fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
        (self.keep.clone(), self.errors.clone())
    }

    fn parse_file(&self, path: &std::path::Path) -> crate::source::FileParse {
        self.inner.parse_file(path)
    }
}

/// 可注入 leader 工作的单飞实现（测试注入失败/panic 闭包）。
fn collect_flighted_with(
    opts: &SummaryOptions,
    leader: &dyn Fn(u64) -> Result<CollectionSnapshot>,
) -> Result<Arc<CollectionSnapshot>> {
    let key = collection_key(opts);
    // entry 分支一次性决定身份：插入者 = 领队，命中者 = 跟随者。
    let (cell, is_leader): (FlightCell, bool) = {
        let mut g = INFLIGHT.lock().unwrap();
        match g.entry(key.clone()) {
            std::collections::hash_map::Entry::Occupied(e) => (e.get().clone(), false),
            std::collections::hash_map::Entry::Vacant(e) => {
                let c: FlightCell = Arc::new((Mutex::new(None), Condvar::new()));
                e.insert(c.clone());
                (c, true)
            }
        }
    };
    if !is_leader {
        return wait_flight(&cell);
    }

    /// RAII 守卫：显式 publish 成功/错误；Drop 兜底 panic 路径并清槽。
    /// 锁顺序固定为先 cell 后 INFLIGHT，且不在持有 INFLIGHT 时等 cell。
    struct FlightGuard {
        key: String,
        cell: FlightCell,
        published: bool,
    }
    impl FlightGuard {
        fn publish_ok(&mut self, snap: CollectionSnapshot) {
            let mut r = self.cell.0.lock().unwrap();
            *r = Some(Ok(snap));
            self.cell.1.notify_all();
            self.published = true;
        }

        fn publish_error(&mut self, msg: String) {
            let mut r = self.cell.0.lock().unwrap();
            *r = Some(Err(msg));
            self.cell.1.notify_all();
            self.published = true;
        }
    }
    impl Drop for FlightGuard {
        fn drop(&mut self) {
            if !self.published {
                let mut r = self.cell.0.lock().unwrap();
                if r.is_none() {
                    *r = Some(Err("采集线程异常退出".to_string()));
                }
                self.cell.1.notify_all();
            }
            // 释放槽位：仅当仍是本航班（旧 leader 不能清理被顶替的槽）。
            let mut g = INFLIGHT.lock().unwrap();
            if g.get(&self.key).is_some_and(|c| Arc::ptr_eq(c, &self.cell)) {
                g.remove(&self.key);
            }
        }
    }

    let mut guard = FlightGuard {
        key,
        cell: cell.clone(),
        published: false,
    };
    let generation = COLLECT_GENERATION.fetch_add(1, Ordering::Relaxed);
    match leader(generation) {
        Ok(snapshot) => {
            guard.publish_ok(snapshot.clone());
            drop(guard);
            Ok(Arc::new(snapshot))
        }
        Err(e) => {
            let msg = e.to_string();
            guard.publish_error(msg.clone());
            drop(guard);
            Err(anyhow::anyhow!(msg))
        }
    }
}

/// Task 2：路径规范化——canonicalize 优先；不存在时用绝对路径 +
/// 组件清理（剥离 `.` 与空组件）作为稳定 fallback（大小写保留，
/// Windows 大小写不敏感重叠由后扫描方的精确键比对兜底）。
pub(crate) fn normalize_path(p: &std::path::Path) -> PathBuf {
    match p.canonicalize() {
        Ok(c) => c,
        // Task 2（审阅）：fallback 折叠 `.`、`..` 与空组件——`a\..\shared`
        // 与 `a\shared` 规范化到同一路径（目录不存在时 canonicalize 失败）。
        Err(_) => {
            let abs = if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir().unwrap_or_default().join(p)
            };
            let mut stack: Vec<std::ffi::OsString> = Vec::new();
            for comp in abs.components() {
                match comp {
                    std::path::Component::CurDir => {}
                    std::path::Component::ParentDir => {
                        stack.pop();
                    }
                    other => stack.push(other.as_os_str().to_os_string()),
                }
            }
            let mut out = PathBuf::new();
            for s in stack {
                out.push(s);
            }
            out
        }
    }
}

fn collect_inner(opts: &SummaryOptions, generation: u64) -> Result<CollectionSnapshot> {
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
    let collected = collect_all_with_sources(sources, opts, generation)?;
    Ok(CollectionSnapshot {
        generation: collected.generation,
        events: collected.events,
        sources: collected.sources,
        warnings: collected.warnings,
        pricing: collected.pricing,
    })
}

/// 共用采集路径（M7）：价格加载 → 缓存增量采集 → 全局去重 → 回填统计。
/// 各阶段 INFO 计时落日志（用户排障依据；粒度 = 每 agent 一行，不逐文件刷屏）。
fn collect_all(opts: &SummaryOptions) -> Result<Collected> {
    let snap = collect_flighted(opts)?;
    Ok(Collected {
        generation: snap.generation,
        events: snap.events.clone(),
        sources: snap.sources.clone(),
        pricing: snap.pricing.clone(),
        warnings: snap.warnings.clone(),
    })
}

/// 可注入来源的采集实现（测试用 MockSource 走同一管线）。
#[cfg(test)]
static TEST_COLLECT_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// Task 2（审阅）：跨 agent 来源重叠去重——同一规范化文件（大小写不敏感
/// 比较键）只保留先扫描者，并返回可诊断 warning（进 Collected.warnings
/// 而非只写日志）。原始路径保留用于文件 IO。
fn dedup_source_overlap(sources: Vec<Box<dyn Source>>) -> (Vec<Box<dyn Source>>, Vec<String>) {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut wrapped: Vec<Box<dyn Source>> = Vec::new();
    for src in sources {
        let (files, errors) = src.discover_with_errors();
        let mut keep = Vec::new();
        for f in files {
            let norm = normalize_path(&f);
            let key = norm.to_string_lossy().to_lowercase();
            if seen.insert(key) {
                keep.push(f);
            } else {
                let w = format!(
                    "来源目录重叠：{} 同时被多个 agent 扫描，重复文件只统计一次",
                    norm.display()
                );
                if !warnings.contains(&w) {
                    warnings.push(w.clone());
                }
                log::warn!("{w}");
            }
        }
        wrapped.push(Box::new(DedupSource {
            inner: src,
            keep,
            errors,
        }));
    }
    (wrapped, warnings)
}

/// 供集成测试注入 mock source（Task 2）。
#[doc(hidden)]
pub struct CollectedView {
    pub events: Vec<UsageEvent>,
    pub warnings: Vec<String>,
}

/// 供集成测试注入 mock source（Task 2）——走与生产完全相同的重叠去重。
#[doc(hidden)]
pub fn collect_all_with_sources_for_test(
    sources: Vec<Box<dyn Source>>,
    opts: &SummaryOptions,
) -> Result<CollectedView> {
    let c = collect_all_with_sources(sources, opts, 0)?;
    Ok(CollectedView {
        events: c.events,
        warnings: c.warnings,
    })
}

fn collect_all_with_sources(
    sources: Vec<Box<dyn Source>>,
    opts: &SummaryOptions,
    generation: u64,
) -> Result<Collected> {
    // Task 2（审阅）：跨 agent 来源重叠去重——同文件只统计一次，
    // 重叠诊断进 Collected.warnings（而非只写日志）。
    let (sources, mut overlap_warnings) = dedup_source_overlap(sources);
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
    warnings.append(&mut overlap_warnings);
    #[cfg(test)]
    {
        let d = TEST_COLLECT_DELAY_MS.load(Ordering::Relaxed);
        if d > 0 {
            std::thread::sleep(std::time::Duration::from_millis(d));
        }
    }

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
        generation,
        events,
        sources: reports,
        pricing,
        warnings,
    })
}

pub fn summary(opts: &SummaryOptions) -> Result<SummaryReport> {
    let t = std::time::Instant::now();
    let Collected {
        generation: _,
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
    /// D1 稳定游标：`"ts|record_id"`（上一页最后一行），取严格小于该
    /// (ts, record_id) 元组的行——同时间戳靠 record_id 决序，翻页不重不漏。
    pub before: Option<String>,
}

/// 一条去重后的用量明细（展示行）。
#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    /// 解析时区下的 "YYYY-MM-DD HH:MM:SS"（存储仍 UTC，见 cache.rs）。
    pub ts: String,
    /// D1：游标第二分量（Claude = message.id；Codex 为空，靠 ts 唯一）。
    pub record_id: String,
    /// Task 2：不透明游标（完整精度 UTC 时间戳 + record_id），翻页原样回传；
    /// 前端不得从展示字符串反解。
    pub cursor: String,
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
        generation: _,
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
    events.sort_by(|a, b| b.ts.cmp(&a.ts).then(b.record_id.cmp(&a.record_id)));
    // Task 1：为相同 (ts, record_id) 的事件分配确定性组内序号（稳定排序后
    // 按位编号）——完全相同 timestamp + 空 record_id 不再共享游标。
    let mut tie: Vec<u64> = Vec::with_capacity(events.len());
    let mut prev: Option<(&jiff::Timestamp, &str)> = None;
    let mut seq: u64 = 0;
    for e in &events {
        let same = prev.is_some_and(|(pts, prid)| *pts == e.ts && prid == e.record_id);
        seq = if same { seq + 1 } else { 0 };
        tie.push(seq);
        prev = Some((&e.ts, e.record_id.as_str()));
    }
    // 每行的游标 = serde_json 序列化的不透明串（完整精度 ts + rid + seq），
    // record_id 含分隔符也不会破坏解析。
    let mut rows: Vec<(UsageEvent, u64, String)> = events
        .into_iter()
        .zip(tie)
        .map(|(e, s)| {
            let cursor = serde_json::to_string(&PageCursor {
                ts: e.ts.to_string(),
                rid: e.record_id.clone(),
                seq: s,
            })
            .unwrap_or_default();
            (e, s, cursor)
        })
        .collect();
    // D1：total = 主过滤 + 下钻后的全量（不含游标截断），翻页时恒定。
    let total = rows.len() as u64;
    // 游标过滤：排序为 ts/rid 降序、组内 seq 升序——下一页 = 排序位次
    // 严格位于游标之后的事件（ts 更小；同 ts 且 rid 更小；同 ts 同 rid
    // 且组内序号更大）。
    if let Some(cur) = &filter.before {
        let (cur_ts, cur_rid, cur_seq) = parse_cursor(cur)?;
        rows.retain(|(e, s, _)| {
            e.ts < cur_ts
                || (e.ts == cur_ts
                    && (e.record_id < cur_rid || (e.record_id == cur_rid && *s > cur_seq)))
        });
    }
    let limit = filter.limit.unwrap_or(200).min(1000);
    let rows: Vec<EventRow> = rows
        .into_iter()
        .take(limit)
        .map(|(e, _s, cursor)| {
            // B3：部分计价模型的明细行展示已计价小计（unknown 分项随总计披露）。
            let cost_usd = pricing
                .estimate(&e.model, &TokenCounts::from_event(&e), e.ts)
                .map(|est| est.cost);
            EventRow {
                ts: e.ts.to_zoned(tz.clone()).strftime("%F %T").to_string(),
                record_id: e.record_id.clone(),
                cursor,
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

/// Task 1：不透明游标（serde_json 序列化）——缺字段、非法时间戳、
/// 未知字段一律拒绝，record_id 中的分隔符不再破坏解析。
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PageCursor {
    ts: String,
    rid: String,
    seq: u64,
}

fn parse_cursor(s: &str) -> Result<(jiff::Timestamp, String, u64)> {
    let c: PageCursor =
        serde_json::from_str(s).map_err(|e| anyhow::anyhow!("游标格式非法: {e}"))?;
    let ts: jiff::Timestamp =
        c.ts.parse()
            .map_err(|e| anyhow::anyhow!("游标时间戳非法: {e}"))?;
    Ok((ts, c.rid, c.seq))
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

[[model]]
prefix = "gpt-5.6"
input = 4.0
output = 20.0
cache_write = 5.0
cache_read = 0.4
"#,
        )
        .unwrap();
        let o = opts(Some(dir.join("cache")), Some(pricing), false);
        let r = summary(&o).unwrap();
        // 07-17 组：sonnet-4-5 input 1000 / output 200 / cw 5000 / cr 10000，
        // 外置价后费用 = (1000*99 + 200*99) / 1M；另有 codex gpt-5.6-sol 5330/1M
        //（B1 桶语义 + 外置 gpt-5.6 价：750*4 + 100*20 + 50*5.0 + 200*0.4）。
        let expected = (1000.0 * 99.0 + 200.0 * 99.0 + 5330.0) / 1_000_000.0;
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

        // Task 1：该测试无任何定价来源 → 全部行 cost=None（unknown 语义）。
        let unknown: Vec<&EventRow> = list.rows.iter().filter(|r| r.cost_usd.is_none()).collect();
        assert_eq!(unknown.len(), list.rows.len(), "无来源时全部未知");
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
                 "input":0,"output":0,"cache_read":0,"cache_write":0},
                {"id":"anthropic/claude-sonnet-4-5","name":"Sonnet",
                 "input":3.0,"output":15.0,"cache_read":0.3,"cache_write":3.75},
                {"id":"openai/gpt-5.6","name":"GPT-5.6",
                 "input":4.0,"output":20.0,"cache_read":0.4,"cache_write":5.0},
                {"id":"openai/gpt-5.5","name":"GPT-5.5",
                 "input":5.0,"output":30.0,"cache_read":0.5,"cache_write":0.0},
                {"id":"openai/gpt-5.4","name":"GPT-5.4",
                 "input":2.5,"output":15.0,"cache_read":0.25,"cache_write":0.0}
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
    fn test_parallel_queries_single_collection() {
        // D1：同参并发查询合并为一次采集（generation 相同 = 同一份快照）。
        let dir = tmp_dir("single-flight");
        let opts = opts(Some(dir.join("cache")), None, false);
        TEST_COLLECT_DELAY_MS.store(300, Ordering::Relaxed);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let b = barrier.clone();
                let o = opts.clone();
                std::thread::spawn(move || {
                    b.wait();
                    collect_all(&o).unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        TEST_COLLECT_DELAY_MS.store(0, Ordering::Relaxed);
        assert_eq!(
            results[0].generation, results[1].generation,
            "并发同参必须复用同一采集快照"
        );
        assert_eq!(results[0].events.len(), results[1].events.len());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_events_pagination_parity() {
        // D1：稳定游标翻页不重不漏，且 total 恒为过滤后全量。
        let hermetic = tmp_dir("pagination");
        let base = SummaryOptions {
            claude_dir: Some(fixture("claude", "basic")),
            codex_dir: Some(fixture("codex", "basic")),
            cache_dir: Some(hermetic.join("cache")),
            pricing_index: Some(hermetic.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let full = list_events(&base, &EventFilter::default()).unwrap();
        assert_eq!(full.rows.len(), 7);
        let mut pages: Vec<Vec<String>> = Vec::new();
        let mut seen = 0usize;
        let mut cursor: Option<String> = None;
        loop {
            let f = EventFilter {
                limit: Some(3),
                before: cursor.clone(),
                ..Default::default()
            };
            let page = list_events(&base, &f).unwrap();
            assert_eq!(page.total, 7, "total 恒为过滤后全量");
            if page.rows.is_empty() {
                break;
            }
            assert!(page.rows.len() <= 3);
            pages.push(page.rows.iter().map(|r| r.cursor.clone()).collect());
            seen += page.rows.len();
            let last = page.rows.last().unwrap();
            cursor = Some(last.cursor.clone());
        }
        assert_eq!(seen, 7, "翻页覆盖全量");
        let flat: Vec<String> = pages.into_iter().flatten().collect();
        let expect: Vec<String> = full.rows.iter().map(|r| r.cursor.clone()).collect();
        assert_eq!(flat, expect);
        std::fs::remove_dir_all(&hermetic).ok();
    }

    #[test]
    fn test_event_pricing_uses_event_timestamp() {
        // Task 4A：费用估算使用历史事件时间与规则时区——同一模型两个事件
        // 落在不同峰谷档时明细费用必须不同；若读当前墙上时钟则两者恒同价。
        let dir = tmp_dir("event-pricing-ts");
        let proj = dir.join("proj-t");
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(
            proj.join("sess-t.jsonl"),
            concat!(
                r#"{"type":"assistant","timestamp":"2026-01-05T13:00:00.000Z","sessionId":"s-t","isSidechain":false,"message":{"id":"msg-2","model":"timed-model","usage":{"input_tokens":1000000,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
                "\n",
                r#"{"type":"assistant","timestamp":"2026-01-05T11:00:00.000Z","sessionId":"s-t","isSidechain":false,"message":{"id":"msg-1","model":"timed-model","usage":{"input_tokens":1000000,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}"#,
                "\n",
            ),
        )
        .unwrap();
        let toml = dir.join("pricing.toml");
        std::fs::write(
            &toml,
            r#"[[model]]
prefix = "timed-model"
input = 2.0
output = 0.0
cache_write = 0.0
cache_read = 0.0

[[model.schedule]]
label = "peak"
timezone = "UTC"

[[model.schedule.period]]
start_time = "10:00"
end_time = "12:00"
input = 10.0
output = 0.0
cache_write = 0.0
cache_read = 0.0
"#,
        )
        .unwrap();
        let cache = dir.join("cache");
        let opts = SummaryOptions {
            by: GroupBy::Day,
            claude_dir: Some(dir.clone()),
            codex_dir: Some(PathBuf::from("Z:/no-such-codex")),
            cache_dir: Some(cache.clone()),
            pricing_index: Some(cache.join("pricing-index.json")),
            pricing_path: Some(toml),
            openrouter_path: Some(PathBuf::from("Z:/no-such/or.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/md.json")),
            tz: Some("UTC".to_string()),
            refresh: true,
            ..Default::default()
        };
        let filter = EventFilter::default();
        let list = list_events(&opts, &filter).unwrap();
        assert_eq!(list.rows.len(), 2);
        // ts 降序：13:00 UTC（窗外 → 基础 2.0）在前；11:00 UTC（峰时 → 10.0）在后。
        assert!(
            (list.rows[0].cost_usd.unwrap() - 2.0).abs() < 1e-9,
            "rows: {:?}",
            list.rows
        );
        assert!(
            (list.rows[1].cost_usd.unwrap() - 10.0).abs() < 1e-9,
            "rows: {:?}",
            list.rows
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Task 1 红测试：失败航班必须可重试、唤醒全部等待者、panic 清槽。
    /// leader 工作经闭包注入（生产路径传真实 collect_inner）。
    #[test]
    fn test_failed_flight_is_retryable() {
        let dir = tmp_dir("flight-retry");
        let opts = opts(Some(dir.join("cache")), None, false);
        let calls = std::sync::Arc::new(AtomicU64::new(0));
        let calls2 = calls.clone();
        let leader = move |_gen: u64| -> Result<CollectionSnapshot> {
            calls2.fetch_add(1, Ordering::Relaxed);
            Err(anyhow::anyhow!("boom"))
        };
        let r1 = collect_flighted_with(&opts, &leader);
        assert!(r1.is_err());
        assert!(
            r1.unwrap_err().to_string().contains("boom"),
            "真实错误必须透传"
        );
        let r2 = collect_flighted_with(&opts, &leader);
        assert!(r2.is_err(), "失败后同参请求必须重新执行而非复用失败结果");
        assert!(r2.unwrap_err().to_string().contains("boom"));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "失败航班必须可重试（leader 跑两次）"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_failed_flight_wakes_all_waiters() {
        let dir = tmp_dir("flight-wake");
        let opts = opts(Some(dir.join("cache")), None, false);
        TEST_COLLECT_DELAY_MS.store(200, Ordering::Relaxed);
        let calls = std::sync::Arc::new(AtomicU64::new(0));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let mut handles = Vec::new();
        for _ in 0..3 {
            let b = barrier.clone();
            let o = opts.clone();
            let c = calls.clone();
            handles.push(std::thread::spawn(move || {
                b.wait();
                let leader = move |_gen: u64| -> Result<CollectionSnapshot> {
                    c.fetch_add(1, Ordering::Relaxed);
                    // 重叠窗口由闭包自身制造（延迟旋钮在真实采集路径上，
                    // 注入闭包的失败路径不经过它）。1s 保证并行测试负载下
                    // 三个线程都进入单飞。
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                    Err(anyhow::anyhow!("boom"))
                };
                collect_flighted_with(&o, &leader)
            }));
        }
        let mut errs = Vec::new();
        for h in handles {
            errs.push(
                h.join()
                    .unwrap()
                    .expect_err("应返回错误而非悬挂")
                    .to_string(),
            );
        }
        assert!(
            errs.iter().all(|e| e.contains("boom")),
            "全部等待者都应被唤醒并拿到真实错误: {errs:?}"
        );
        let n = calls.load(Ordering::Relaxed);
        assert!(n == 1, "重叠窗口内 leader 只跑一次: {n}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_panicked_flight_wakes_waiter_and_clears_slot() {
        let dir = tmp_dir("flight-panic");
        let opts = opts(Some(dir.join("cache")), None, false);
        // Task 6：channel 信号消除 sleep 竞态——
        // (1) 领队闭包被调用 = 已注册航班 → 发 registered；
        // (2) 主线程收到后，由独立定时线程在宽裕时间窗后发 go（此时主线程
        //     已作为跟随者阻塞在 wait_flight，领队阻塞在 go 上不会提前清槽）；
        // (3) 领队收到 go 才 panic → 守卫发布错误 + 清槽 → 跟随者唤醒。
        let (registered_tx, registered_rx) = std::sync::mpsc::channel::<()>();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let o1 = opts.clone();
        let leader = std::thread::spawn(move || {
            let leader_fn = |_gen: u64| -> Result<CollectionSnapshot> {
                registered_tx.send(()).unwrap();
                go_rx.recv().unwrap();
                panic!("采集炸了");
            };
            let _ = collect_flighted_with(&o1, &leader_fn);
        });
        registered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("领队必须先注册航班");
        // 独立线程发 go：主测试线程此刻正以跟随者身份进入 wait_flight。
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            go_tx.send(()).unwrap();
        });
        let ok_fn = move |_gen: u64| -> Result<CollectionSnapshot> {
            panic!("跟随者闭包不应被调用");
        };
        let follower = collect_flighted_with(&opts, &ok_fn);
        assert!(
            follower.is_err() && follower.unwrap_err().to_string().contains("异常退出"),
            "panic 必须唤醒等待者"
        );
        leader.join().unwrap_err();
        // 槽位已清空：后续同参请求可重新执行（成功闭包这次成为领队）。
        let ok2 = |_gen: u64| -> Result<CollectionSnapshot> {
            Ok(CollectionSnapshot {
                generation: 9,
                events: Vec::new(),
                sources: Vec::new(),
                warnings: Vec::new(),
                pricing: std::sync::Arc::new(crate::pricing::Pricing::default()),
            })
        };
        let r = collect_flighted_with(&opts, &ok2);
        assert!(r.is_ok(), "panic 清槽后必须可重试: {r:?}");
        assert_eq!(r.unwrap().generation, 9);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Task 2 fixture：同秒亚秒 + 空 record_id 的 codex 事件（去重前 3 条）。
    fn subsecond_fixture(dir: &std::path::Path) -> SummaryOptions {
        let codex_dir = dir.join("codex");
        let day = codex_dir.join("2026").join("07").join("17");
        std::fs::create_dir_all(&day).unwrap();
        let mut lines = String::from(concat!(
            r#"{"timestamp":"2026-07-17T14:59:00.000Z","type":"session_meta","payload":{"id":"sp","session_id":"sp","cwd":"C:/w/p"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-17T14:59:10.000Z","type":"turn_context","payload":{"model":"m","cwd":"C:/w/p"}}"#,
            "\n",
        ));
        // 同一秒内两条（.500 与 .200，排序键仅亚秒不同）+ 更早一条；record_id 全空。
        for (i, ts) in [
            "2026-07-17T15:00:00.500Z",
            "2026-07-17T15:00:00.200Z",
            "2026-07-17T14:59:59.900Z",
        ]
        .iter()
        .enumerate()
        {
            let input = 10 - i as u64;
            lines.push_str(
                format!(
                    r#"{{"timestamp":"{ts}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"output_tokens":1,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":{}}}}}}}}}"#,
                    input + 1
                )
                .as_str(),
            );
            lines.push('\n');
        }
        std::fs::write(day.join("rollout-p.jsonl"), lines).unwrap();
        SummaryOptions {
            agent: Some(AgentKind::Codex),
            claude_dir: Some(dir.join("no-claude")),
            codex_dir: Some(codex_dir),
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_events_pagination_same_second_subsecond() {
        // Task 2（P1）：页边界落在同一秒内时，展示秒级游标会丢掉同秒内
        // 更晚（排序更靠后）的事件——游标必须携带完整精度 UTC 时间戳。
        let dir = tmp_dir("subsecond");
        let opts = subsecond_fixture(&dir);
        let mut seen: Vec<(String, u64)> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = list_events(
                &opts,
                &EventFilter {
                    limit: Some(1),
                    before: cursor.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
            if page.rows.is_empty() {
                break;
            }
            let r = &page.rows[0];
            seen.push((r.cursor.clone(), r.input));
            cursor = Some(r.cursor.clone());
        }
        assert_eq!(seen.len(), 3, "翻页必须覆盖全量且不丢同秒事件: {seen:?}");
        assert_eq!(seen[0].1, 10, "亚秒 .500 先于 .200（时间倒序）");
        assert_eq!(seen[1].1, 9, "同秒 .200 不得被秒级游标丢弃");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_events_pagination_empty_record_id_tie_break() {
        // Task 2：空 record_id（Codex 全部如此）在同秒内靠完整精度 ts 决序。
        let dir = tmp_dir("empty-rid");
        let opts = subsecond_fixture(&dir);
        let page1 = list_events(
            &opts,
            &EventFilter {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page1.rows.len(), 2);
        // 两行 record_id 均为空，但 cursor 必须可区分（完整精度 ts）。
        assert_ne!(page1.rows[0].cursor, page1.rows[1].cursor);
        let cursor = &page1.rows[1].cursor;
        let page2 = list_events(
            &opts,
            &EventFilter {
                limit: Some(2),
                before: Some(cursor.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(page2.rows.len(), 1, "空 record_id 事件不得因游标歧义丢失");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Task 1 fixture：两条事件 UTC timestamp 逐字相同、record_id 均为空、
    /// token 数不同——游标必须靠稳定 tie-breaker 区分。
    fn identical_ts_fixture(dir: &std::path::Path) -> SummaryOptions {
        let codex_dir = dir.join("codex");
        let day = codex_dir.join("2026").join("07").join("17");
        std::fs::create_dir_all(&day).unwrap();
        let mut lines = String::from(concat!(
            r#"{"timestamp":"2026-07-17T14:59:00.000Z","type":"session_meta","payload":{"id":"se","session_id":"se","cwd":"C:/w/e"}}"#,
            "
",
            r#"{"timestamp":"2026-07-17T14:59:10.000Z","type":"turn_context","payload":{"model":"m","cwd":"C:/w/e"}}"#,
            "
",
            r#"{"timestamp":"2026-07-17T15:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10,"output_tokens":1,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":11}}}}"#,
            "
",
            r#"{"timestamp":"2026-07-17T15:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20,"output_tokens":2,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":22}}}}"#,
            "
",
            r#"{"timestamp":"2026-07-17T15:00:01.000Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":30,"output_tokens":3,"cached_input_tokens":0,"cache_write_input_tokens":0,"reasoning_output_tokens":0,"total_tokens":33}}}}"#,
            "
",
        ));
        let _ = &mut lines;
        std::fs::write(day.join("rollout-e.jsonl"), lines).unwrap();
        SummaryOptions {
            agent: Some(AgentKind::Codex),
            claude_dir: Some(dir.join("no-claude")),
            codex_dir: Some(codex_dir),
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn test_events_pagination_exact_timestamp_empty_record_id() {
        // Task 1（P1）：完全相同 timestamp + 空 record_id → 旧游标两行同键，
        // 页边界第二行丢失或游标重复。修复后两条都出现且 cursor 不同，
        // 重复读取 cursor 顺序稳定。
        let dir = tmp_dir("identical-ts");
        let opts = identical_ts_fixture(&dir);
        let mut seen: Vec<u64> = Vec::new();
        let mut cursors: Vec<String> = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let page = list_events(
                &opts,
                &EventFilter {
                    limit: Some(1),
                    before: cursor.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
            if page.rows.is_empty() {
                break;
            }
            let r = &page.rows[0];
            seen.push(r.input);
            cursors.push(r.cursor.clone());
            cursor = Some(r.cursor.clone());
        }
        // 时间倒序：input 30 先出；同键两行按稳定排序保留原始顺序（10、20），
        // 两行都必须出现且不丢行。
        assert_eq!(seen, [30, 10, 20], "同 timestamp 两行都必须出现: {seen:?}");
        assert!(
            cursors[1] != cursors[2],
            "同键两行的 cursor 必须不同: {cursors:?}"
        );
        // 稳定性：重复查询（无游标）得到的 cursor 序列一致。
        let again = list_events(&opts, &EventFilter::default()).unwrap();
        let again_cursors: Vec<String> = again.rows.iter().map(|r| r.cursor.clone()).collect();
        let first_pass: Vec<String> =
            vec![cursors[0].clone(), cursors[1].clone(), cursors[2].clone()];
        assert_eq!(again_cursors, first_pass, "cursor 顺序必须稳定");
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
        // Task 8（source_empty_error_ready_states）：存在但无日志 → empty。
        let empty_dir = tmp_dir("empty-src");
        std::fs::create_dir_all(&empty_dir).unwrap();
        let mut s2 = crate::settings::Settings::default();
        s2.sources.claude = Some(crate::settings::SourceConfig {
            enabled: true,
            dir: Some(empty_dir.display().to_string()),
        });
        let st2 = source_status(&s2).unwrap();
        assert_eq!(st2[0].state, "empty", "目录存在但无 jsonl → empty");
        std::fs::remove_dir_all(&empty_dir).ok();
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
        let c = collect_all_with_sources(sources, &opts, 0).unwrap();
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

    /// 计数 Mock：统计 parse_file 调用次数（no_reparse 验证）。
    struct CountingMock {
        root: PathBuf,
        file: PathBuf,
        parses: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Source for CountingMock {
        fn agent(&self) -> AgentKind {
            AgentKind::Codex
        }

        fn root(&self) -> &Path {
            &self.root
        }

        fn discover_with_errors(&self) -> (Vec<PathBuf>, Vec<String>) {
            (vec![self.file.clone()], Vec::new())
        }

        fn parse_file(&self, _path: &Path) -> crate::source::FileParse {
            self.parses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    fn test_filter_switch_no_reparse() {
        // D4：筛选切换后同参采集走缓存命中，不重复解析（缓存纯优化）。
        let dir = tmp_dir("no-reparse");
        let root = dir.join("codex");
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("rollout-x.jsonl");
        std::fs::write(&file, "{ }").unwrap();
        let parses = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let opts = SummaryOptions {
            agent: Some(AgentKind::Codex),
            cache_dir: Some(dir.join("cache")),
            pricing_index: Some(dir.join("idx.json")),
            openrouter_path: Some(PathBuf::from("Z:/no-such/openrouter-snapshot.json")),
            modelsdev_path: Some(PathBuf::from("Z:/no-such/modelsdev-snapshot.json")),
            tz: Some("Asia/Shanghai".to_string()),
            ..Default::default()
        };
        let mk_sources = || {
            vec![Box::new(CountingMock {
                root: root.clone(),
                file: file.clone(),
                parses: parses.clone(),
            }) as Box<dyn Source>]
        };
        collect_all_with_sources(mk_sources(), &opts, 0).unwrap();
        let after_first = parses.load(std::sync::atomic::Ordering::Relaxed);
        assert_eq!(after_first, 1);
        collect_all_with_sources(mk_sources(), &opts, 1).unwrap();
        assert_eq!(
            parses.load(std::sync::atomic::Ordering::Relaxed),
            after_first,
            "第二次采集必须缓存命中，不得重复解析"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
