//! SF04：短生命周期不可变查询快照。
//!
//! 一次 `begin_query` 冻结：单飞采集的事件集（Arc 共享，零复制）、价格表
//! 与修订号、SF05 解析一次的 as_of/时区/范围、主查询指纹。`query_summary`
//! / `query_events` 显式携带 query_id 从同一快照读取——翻页不重新采集、
//! 不重新排序、不重新编号、不换价格；排序与组内 seq 在快照建立时只算一次
//!（快照行只存事件索引，不复制事件本体）。
//!
//! 游标 v2 绑定 query_id、主查询指纹、下钻过滤指纹与快照内唯一行位置；
//! 版本/归属/位置不匹配一律结构化拒绝（`query_expired` / 游标非法），
//! 允许刷新重建，绝不静默换第一页或新采集结果（docs/stats-semantics.md §3.5）。
//!
//! 会话限制（具名常量，选值依据）：
//! - [`MAX_ACTIVE_QUERIES`] = 8：一次刷新批次一个会话，8 个远超单窗口
//!   交互需要；淘汰后旧游标显式过期，重建即可恢复。
//! - [`MAX_RETAINED_QUERY_BYTES`] = 256 MiB：查询子系统**保留数据**的
//!   字节预算。记账对象是每个会话完整可达的保留对象图（事件 Vec 与其
//!   四个 String、逐源统计、诊断字符串、价格表的候选/嵌套规则/索引、
//!   快照行索引与身份/时区元数据），**不由筛选命中行数代替**——2026-10-08
//!   复核 RC05 已复现：零命中行的查询仍保留整份采集事件，而旧实现按
//!   `rows.len()` 记账，等于不计。
//! - [`QUERY_IDLE_TTL`] = 10 分钟：一次分页浏览极少超过 10 分钟无读取；
//!   闲置会话优先淘汰，活跃会话（持续翻页）不被回收。
//!
//! 预算边界（不夸大）：本预算只约束**查询子系统的保留数据**，**不宣称**
//! 整个进程 RSS、分配器缓存、或采集/建表期间的临时峰值被同一数值限制
//!（见 docs/stats-semantics.md §3.5 与 tests/query_memory_budget.rs）。
//!
//! 查询身份（RC01）：`query_id` = 每次进程启动从系统随机源取的 128 位
//! 命名空间 + 进程内单调计数 + 采集 generation。**跨启动唯一性只由随机
//! 命名空间保证**——时间戳/PID/纯计数（单独或简单拼接）在重启后都会
//! 重新产生同一身份，使旧游标被新进程错误接受（2026-10-08 复核已复现
//! `q0-g0` 重放）。随机源失败或计数溢出一律返回明确错误，绝不回退。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};

use crate::aggregate::resolve_tz;
use crate::model::UsageEvent;
use crate::report::{self, EventFilter, SourceReport, SummaryOptions};

pub const MAX_ACTIVE_QUERIES: usize = 8;
/// RC05：查询子系统保留数据的字节预算（见模块文档的边界说明）。
pub const MAX_RETAINED_QUERY_BYTES: usize = 256 * 1024 * 1024;
pub const QUERY_IDLE_TTL: Duration = Duration::from_secs(600);

/// 字节 → MiB（错误信息与诊断用）。
const MIB: usize = 1024 * 1024;

/// 预算错误里的字节写法：小预算（测试注入）也要给出**可读且不为 0** 的量，
/// 不能出现"约 0 MiB"这种看不出实际大小的提示。
fn display_bytes(n: usize) -> String {
    if n >= MIB {
        format!("{:.2} MiB（{} 字节）", n as f64 / MIB as f64, n)
    } else if n >= 1024 {
        format!("{:.1} KiB（{} 字节）", n as f64 / 1024.0, n)
    } else {
        format!("{n} 字节")
    }
}

/// 游标格式版本：v2 = 绑定 query_id / 主查询指纹 / 下钻指纹（SF04）。
/// v1（ts|rid|seq）游标缺字段且无法验证归属，解析时直接拒绝。
pub const CURSOR_VERSION: u8 = 2;

/// 查询快照：冻结的主过滤行序 + 采集/价格上下文。
pub struct QuerySnapshot {
    pub query_id: String,
    pub generation: u64,
    /// 价格签名（SF03 内容摘要签名）：分页会话内冻结，价格同步不影响旧分页。
    pub pricing_revision: String,
    /// 主查询指纹（采集参数 + 时间范围 + 时区）。
    pub main_fingerprint: String,
    pub tz: TimeZone,
    pub tz_label: String,
    /// 冻结的查询时间基准（SF05：一次查询只解析一次 today）。
    pub as_of: jiff::Zoned,
    pub by: crate::aggregate::GroupBy,
    /// 主过滤后的固定行序（ts/rid 降序 + 组内 seq），只存事件索引。
    pub rows: Vec<SnapshotRow>,
    /// H06：本次查询固定的日汇总桶选择——只有日粒度的外部历史按桶决定统计
    /// 来源（明细覆盖 / 汇总口径），在建快照时**算一次**后冻结。
    rollup_selection: report::RollupSelection,
    collection: Arc<report::CollectionSnapshot>,
    /// RC05：保留字节额度凭证——随本快照的**最后一个 Arc** 释放。
    /// 会话被淘汰但仍有读取者持有时继续占账
    ///（`evicted_but_borrowed_snapshot_stays_charged`）。
    _reservation: QuotaReservation,
}

#[derive(Debug, Clone)]
pub struct SnapshotRow {
    /// 采集事件集中的位置（零复制）。
    pub index: usize,
    /// 相同 (ts, record_id) 组内的序号（0..n）——游标第三分量。
    pub seq: u64,
}

impl QuerySnapshot {
    pub fn event(&self, row: &SnapshotRow) -> &UsageEvent {
        &self.collection.events[row.index]
    }

    pub fn events(&self) -> &[UsageEvent] {
        &self.collection.events
    }

    pub fn pricing(&self) -> &Arc<crate::pricing::Pricing> {
        &self.collection.pricing
    }

    pub fn sources(&self) -> &[SourceReport] {
        &self.collection.sources
    }

    pub fn warnings(&self) -> &[String] {
        &self.collection.warnings
    }

    /// H06：本次查询固定的日汇总桶选择（建快照时**只算一次**——汇总、明细与
    /// 下钻读同一份结果，不因筛选条件改变统计优先级）。
    pub fn rollup_selection(&self) -> &report::RollupSelection {
        &self.rollup_selection
    }
}

/// 下钻过滤指纹（模型/项目/日——明细侧子查询身份；游标/limit 不参与）。
/// MP04：模型筛选按**等价身份**归一——同一模型的两种写法（`claude-opus-5-5`
/// 与 `claude-opus-5.5`）视为同一子查询，游标可在其间续用；不同模型（含
/// 变体）仍是不同指纹。
pub(crate) fn drill_fingerprint(filter: &EventFilter) -> String {
    let model = filter
        .model
        .as_deref()
        .map(|m| crate::model_identity::ModelIdentity::parse(m).identity_key());
    format!(
        "model={:?}|project={:?}|day={:?}",
        model, filter.project, filter.day
    )
}

fn main_fingerprint(opts: &SummaryOptions, tz_label: &str) -> String {
    format!(
        "{}|days={:?}|from={:?}|to={:?}|tz={tz_label}",
        report::collection_key(opts),
        opts.days,
        opts.from,
        opts.to
    )
}

struct Session {
    snapshot: Arc<QuerySnapshot>,
    last_used: Instant,
}

static QUERY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 跨启动唯一的进程命名空间（RC01）：每次进程启动从系统随机源取 128 位，
/// 渲染为 32 位十六进制。惰性初始化一次，结果（含失败）缓存整个进程。
///
/// 为什么不回退：时间戳/PID/纯计数在重启后都可能重现同一取值，正是
/// `q0-g0` 重放的根因；宁可让 `begin_query` 明确失败，也不产生会被
/// 误认的"合法"身份。
static QUERY_NAMESPACE: OnceLock<Result<String, String>> = OnceLock::new();

fn query_namespace() -> Result<&'static str> {
    QUERY_NAMESPACE
        .get_or_init(|| {
            let mut bytes = [0u8; 16]; // 128 位
            getrandom::getrandom(&mut bytes)
                .map_err(|e| format!("查询身份随机源不可用，无法建立唯一查询会话: {e}"))?;
            let mut hex = String::with_capacity(32);
            for b in bytes {
                use std::fmt::Write as _;
                let _ = write!(hex, "{b:02x}");
            }
            Ok(hex)
        })
        .as_deref()
        .map_err(|e| anyhow!("{e}"))
}

/// 进程内单调计数（命名空间内的序号）：溢出即显式拒绝，不复用旧身份。
fn next_query_seq() -> Result<u64> {
    QUERY_COUNTER
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
        .map_err(|_| anyhow!("查询序号已耗尽（进程内计数溢出），请重启应用"))
}

fn registry() -> &'static Mutex<HashMap<String, Session>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn gc_expired(reg: &mut HashMap<String, Session>, now: Instant) {
    let before = reg.len();
    reg.retain(|_, s| now.duration_since(s.last_used) < QUERY_IDLE_TTL);
    if reg.len() != before {
        log::debug!("查询会话回收：{} 个空闲超 TTL", before - reg.len());
    }
}

/// RC05：保留字节额度（进程级）。`limit` 可在测试中注入更小值。
struct Quota {
    used: usize,
    limit: usize,
}

fn quota() -> &'static Mutex<Quota> {
    static QUOTA: OnceLock<Mutex<Quota>> = OnceLock::new();
    QUOTA.get_or_init(|| {
        Mutex::new(Quota {
            used: 0,
            limit: MAX_RETAINED_QUERY_BYTES,
        })
    })
}

fn quota_limit() -> usize {
    quota().lock().unwrap_or_else(|e| e.into_inner()).limit
}

/// RC05：保留字节额度凭证。
///
/// 额度**随快照的最后一个 Arc 释放**——被淘汰但仍有读取者持有的快照继续
/// 占账，直到读取结束；这样"淘汰即可腾额度"的假设不会被误用为超预算准入。
struct QuotaReservation {
    bytes: usize,
    released: bool,
}

impl QuotaReservation {
    /// 原子占额度：`used + bytes` 必须可表示且不超上限，否则**明确失败**
    ///（绝不超预算接受，也不截断事件集来凑数）。
    fn acquire(bytes: usize) -> Result<Self, String> {
        let mut q = quota().lock().unwrap_or_else(|e| e.into_inner());
        let need = q
            .used
            .checked_add(bytes)
            .ok_or_else(|| "查询保留字节记账溢出（超出 usize 可表示范围）".to_string())?;
        if need > q.limit {
            // RC05：建议必须与"什么才会减少保留量"一致——采集快照整份随会话
            // 保留，时间范围只是采集后的过滤，缩小范围**不会**降低保留字节。
            return Err(format!(
                "查询保留数据 {need}（本次 {bytes}，已保留 {used}，预算 {limit}，约合 {need_mib} MiB）：\
                 可减少同时进行的查询会话数，或稍后重试（闲置会话超过 {ttl} 秒自动回收）；\
                 需要立即腾出额度请在设置中停用暂不采集的来源目录（缩小时间范围不会减少保留量）",
                need = display_bytes(need),
                bytes = display_bytes(bytes),
                used = display_bytes(q.used),
                limit = display_bytes(q.limit),
                need_mib = need as f64 / MIB as f64,
                ttl = QUERY_IDLE_TTL.as_secs()
            ));
        }
        q.used = need;
        Ok(Self {
            bytes,
            released: false,
        })
    }

    fn release(&mut self) {
        if self.released {
            return;
        }
        self.released = true;
        let mut q = quota().lock().unwrap_or_else(|e| e.into_inner());
        q.used = q.used.saturating_sub(self.bytes);
    }
}

impl Drop for QuotaReservation {
    fn drop(&mut self) {
        self.release();
    }
}

fn oldest_key(reg: &HashMap<String, Session>) -> Option<String> {
    reg.iter()
        .min_by_key(|(_, s)| s.last_used)
        .map(|(k, _)| k.clone())
}

/// RC05：可回收额度的会话——**仅注册表持有**（`Arc::strong_count == 1`）
/// 的会话被淘汰时才真正释放额度；被读取者持有的会话淘汰了也不腾额度。
/// 因此额度不足时优先回收这一类；一个都没有就不要再牺牲活跃会话。
fn reclaimable_key(reg: &HashMap<String, Session>) -> Option<String> {
    reg.iter()
        .filter(|(_, s)| Arc::strong_count(&s.snapshot) == 1)
        .min_by_key(|(_, s)| s.last_used)
        .map(|(k, _)| k.clone())
}

/// RC05：额度准入——先原子尝试；不足则淘汰一个**仅注册表持有**的会话
///（淘汰即释放额度）后重试，直到腾出或没有可回收的会话。全部失败时返回
/// 可读错误，不超预算接受，也不会为一次注定失败的准入牺牲活跃会话。
fn acquire_with_eviction(bytes: usize) -> Result<QuotaReservation, String> {
    let mut last = String::new();
    for _ in 0..=MAX_ACTIVE_QUERIES {
        match QuotaReservation::acquire(bytes) {
            Ok(r) => return Ok(r),
            Err(e) => {
                last = e;
                let removed = {
                    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
                    reclaimable_key(&reg).and_then(|k| reg.remove(&k))
                };
                match removed {
                    Some(s) => {
                        log::info!(
                            "查询会话 {} 被淘汰（保留字节预算），其游标将显式过期",
                            s.snapshot.query_id
                        );
                        // 锁外丢弃：Drop 会取额度锁，避免与注册表锁形成反序。
                        drop(s);
                    }
                    None => break,
                }
            }
        }
    }
    Err(last)
}

/// RC05：注册会话。容量（会话数）淘汰在注册表锁内完成，被淘汰项在**锁外**
/// 丢弃——Drop 会触发额度释放，若在持锁时丢弃会与额度锁形成反序。
fn register(snapshot: Arc<QuerySnapshot>) {
    let now = Instant::now();
    let mut evicted: Vec<Session> = Vec::new();
    {
        let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
        gc_expired(&mut reg, now);
        while reg.len() >= MAX_ACTIVE_QUERIES {
            let Some(oldest) = oldest_key(&reg) else {
                break;
            };
            log::info!("查询会话 {oldest} 被淘汰（容量策略），其游标将显式过期");
            if let Some(s) = reg.remove(&oldest) {
                evicted.push(s);
            }
        }
        reg.insert(
            snapshot.query_id.clone(),
            Session {
                snapshot,
                last_used: now,
            },
        );
    }
    drop(evicted);
}

/// RC05：一个会话保留的字节 = 采集快照（事件/字符串/价格/诊断）+ 快照行
/// 索引 + 查询身份/价格修订/时区/主指纹元数据 + 快照结构自身。
/// 计算全程受检，溢出显式报错。
///
/// 采集快照与价格表按 Arc **共享**，但这里仍逐会话完整计账（保守、简单，
/// 宁可重复计账也不漏账）；快照自身持有的 `pricing_revision` 是采集修订号
/// 的一份**克隆**，同样单独计入。
fn snapshot_retained_bytes(
    collection: &report::CollectionSnapshot,
    rows: &Vec<SnapshotRow>,
    query_id: &str,
    tz_label: &str,
    main_fingerprint: &str,
) -> Result<usize, String> {
    use report::retained::{ByteCount, string_bytes, vec_bytes};
    let mut n = ByteCount::default();
    n.add(report::collection_retained_bytes(collection)?)?;
    n.add(vec_bytes(rows)?)?;
    n.add(string_bytes(&query_id.to_string())?)?;
    n.add(string_bytes(&tz_label.to_string())?)?;
    n.add(string_bytes(&main_fingerprint.to_string())?)?;
    // 本会话自身持有的价格修订克隆 + 时区对象（其内部名称/规则串由
    // TimeZone 自身结构保守按 size_of 计入，标签串已在 tz_label 计过）。
    n.add(string_bytes(&collection.pricing_revision)?)?;
    n.add(std::mem::size_of::<TimeZone>())?;
    n.add(std::mem::size_of::<QuerySnapshot>())?;
    Ok(n.get())
}

/// 创建查询会话：单飞采集（复用现有缓存/单飞管线）→ 主过滤一次 →
/// 排序与 seq 只算一次 → 注册。返回的 Arc 快照供汇总/分页共享。
pub fn begin_query(opts: &SummaryOptions) -> Result<Arc<QuerySnapshot>> {
    // RC01：先解析进程命名空间与序号——随机源不可用/计数耗尽时在采集前
    // 就明确失败，不做任何昂贵工作，也不产生可能被误认的身份。
    let namespace = query_namespace()?;
    let qseq = next_query_seq()?;

    let (tz, tz_label) = resolve_tz(opts.tz.as_deref())?;
    let as_of = jiff::Zoned::now().with_time_zone(tz.clone());
    let collection = report::collect_flighted(opts)?;
    let main_fingerprint = main_fingerprint(opts, &tz_label);

    // 主过滤（时间范围）一次应用：返回保留事件的索引，不复制事件。
    let kept = report::time_filter_indices(&collection.events, opts, &tz, &as_of)?;

    // 排序（ts/rid 降序）+ 组内 seq：同一快照只计算一次。
    let mut keyed: Vec<(usize, &jiff::Timestamp, &str)> = kept
        .into_iter()
        .map(|i| {
            let e = &collection.events[i];
            (i, &e.ts, e.record_id.as_str())
        })
        .collect();
    keyed.sort_by(|a, b| b.1.cmp(a.1).then(b.2.cmp(a.2)));
    let mut rows: Vec<SnapshotRow> = Vec::with_capacity(keyed.len());
    let mut prev: Option<(&jiff::Timestamp, &str)> = None;
    let mut seq: u64 = 0;
    for (index, ts, rid) in keyed {
        let same = prev.is_some_and(|(pts, prid)| pts == ts && prid == rid);
        seq = if same { seq + 1 } else { 0 };
        rows.push(SnapshotRow { index, seq });
        prev = Some((ts, rid));
    }

    // RC05：按**完整保留对象**记账（不是筛选命中行数）——零命中行的查询
    // 仍保留整份采集事件、字符串与价格表。
    let query_id = format!("q{namespace}-{qseq}-g{}", collection.generation);
    let charge =
        snapshot_retained_bytes(&collection, &rows, &query_id, &tz_label, &main_fingerprint)
            .map_err(|e| anyhow!(e))?;
    let limit = quota_limit();
    if charge > limit {
        // RC05：单会话超预算——明确失败、不截断事件；建议必须真实有效：
        // 采集到的事件整份随会话保留，时间范围是采集后的过滤，缩小范围并
        // 不减少保留量，所以不能这样提示。
        anyhow::bail!(
            "查询保留数据 {}（约合 {:.2} MiB）超出单会话预算 {}：可在设置中停用暂不采集的来源目录后重试，\
             或稍后重试等待闲置会话回收（缩小时间范围不会减少保留量——整份采集事件随会话保留，不按要求截断）",
            display_bytes(charge),
            charge as f64 / MIB as f64,
            display_bytes(limit)
        );
    }
    // 准入：额度原子占用（失败会按 LRU 淘汰后重试）；失败时 reservation
    // 未建立、快照未注册，额度不泄漏。
    let reservation = acquire_with_eviction(charge).map_err(|e| anyhow!(e))?;

    // H06：日汇总桶选择在这里固定一次——按日筛选/分组且时区与来源时区不同时，
    // 日汇总不参与（返回可恢复提示），绝不把来源日期当午夜事件重切。
    let day_sensitive = opts.by == crate::aggregate::GroupBy::Day
        || opts.days.is_some()
        || opts.from.is_some()
        || opts.to.is_some();
    let rollup_selection = report::select_rollup_contributions(
        &collection.events,
        &collection.rollups,
        day_sensitive,
        &tz_label,
        report::RollupBucketPolicy::default(),
    );

    let snapshot = Arc::new(QuerySnapshot {
        // RC01：命名空间 + 序号 + generation。跨启动唯一性来自命名空间，
        // generation 仅用于诊断（同参并发采集合并时相同）。
        query_id,
        generation: collection.generation,
        pricing_revision: collection.pricing_revision.clone(),
        main_fingerprint,
        tz,
        tz_label,
        as_of,
        by: opts.by,
        rows,
        rollup_selection,
        collection,
        _reservation: reservation,
    });

    log::debug!(
        "查询会话 {} 建立：{} 行，价格修订 {}，保留约 {} MiB",
        snapshot.query_id,
        snapshot.rows.len(),
        snapshot.pricing_revision,
        charge / MIB
    );
    register(snapshot.clone());
    Ok(snapshot)
}

/// 按会话读取：存在则续期并执行；不存在/已淘汰/进程重启 → 结构化过期错误。
pub fn with_snapshot<T>(query_id: &str, f: impl FnOnce(&QuerySnapshot) -> Result<T>) -> Result<T> {
    let snapshot = {
        let mut reg = registry().lock().unwrap();
        let now = Instant::now();
        gc_expired(&mut reg, now);
        match reg.get_mut(query_id) {
            Some(s) => {
                s.last_used = now;
                s.snapshot.clone()
            }
            None => {
                return Err(anyhow!("query_expired: 查询会话不存在或已失效，请刷新重试"));
            }
        }
    };
    f(&snapshot)
}

/// begin_query 的句柄形态：返回给前端的会话元数据（不含事件数据）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryHandle {
    pub query_id: String,
    pub generation: u64,
    pub pricing_revision: String,
    pub timezone: String,
    /// 冻结的时间基准（与 SF05 语义一致，供诊断）。
    pub as_of: String,
}

/// 创建查询会话并返回句柄（GUI 命令入口：一次批次调用一次）。
pub fn begin_query_handle(opts: &SummaryOptions) -> Result<QueryHandle> {
    let s = begin_query(opts)?;
    Ok(QueryHandle {
        query_id: s.query_id.clone(),
        generation: s.generation,
        pricing_revision: s.pricing_revision.clone(),
        timezone: s.tz_label.clone(),
        as_of: s.as_of.to_string(),
    })
}

/// 按会话聚合汇总（GUI 命令入口）。
pub fn query_summary(query_id: &str) -> Result<report::SummaryReport> {
    with_snapshot(query_id, report::query_summary_from)
}

/// 按会话分页读取明细（GUI 命令入口；游标必须属于同一会话）。
pub fn query_events(query_id: &str, filter: &EventFilter) -> Result<report::EventList> {
    with_snapshot(query_id, |snap| report::query_events_from(snap, filter))
}

/// 游标 v2：绑定 query_id、主查询指纹、下钻过滤指纹与快照内行位置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PageCursor {
    pub v: u8,
    pub qid: String,
    pub mfp: String,
    pub dfp: String,
    pub ts: String,
    pub rid: String,
    pub seq: u64,
}

/// 校验游标归属（版本/query_id/主指纹/下钻指纹）并返回其行在快照固定
/// 行序中的绝对位置。任何不匹配都是显式错误——绝不静默换第一页。
pub fn locate_cursor(snapshot: &QuerySnapshot, cursor_str: &str, drill_fp: &str) -> Result<usize> {
    let c: PageCursor =
        serde_json::from_str(cursor_str).map_err(|e| anyhow!("游标格式非法: {e}"))?;
    if c.v != CURSOR_VERSION {
        return Err(anyhow!("query_expired: 游标版本已更新，请刷新重试"));
    }
    if c.qid != snapshot.query_id {
        return Err(anyhow!("query_expired: 游标属于其他查询会话，请刷新重试"));
    }
    if c.mfp != snapshot.main_fingerprint {
        return Err(anyhow!("query_expired: 主筛选已变化，请刷新重试"));
    }
    if c.dfp != drill_fp {
        return Err(anyhow!("query_expired: 明细筛选已变化，请刷新重试"));
    }
    let ts: jiff::Timestamp = c.ts.parse().map_err(|_| anyhow!("游标时间戳非法"))?;
    snapshot
        .rows
        .iter()
        .position(|r| {
            let e = snapshot.event(r);
            e.ts == ts && e.record_id == c.rid && r.seq == c.seq
        })
        .ok_or_else(|| anyhow!("游标位置在当前查询中不存在，请刷新重试"))
}

/// 测试专用（#[doc(hidden)]）：清空查询会话注册表（模拟进程重启）。
#[doc(hidden)]
pub fn clear_query_registry_for_tests() {
    registry().lock().unwrap().clear();
}

/// 测试专用（#[doc(hidden)]）：把指定会话的最近使用时间回拨——注入时钟
/// 驱动 TTL 回收，不用 sleep 猜时序。
#[doc(hidden)]
pub fn backdate_query_for_tests(query_id: &str, by: Duration) {
    let mut reg = registry().lock().unwrap();
    if let Some(s) = reg.get_mut(query_id) {
        s.last_used = s.last_used.checked_sub(by).unwrap_or_else(Instant::now);
    }
}

/// 测试专用（#[doc(hidden)]）：注入更小的保留字节预算——用小额预算触发
/// 淘汰/拒绝路径，不必真的分配数百 MB。产品默认值不受影响。
#[doc(hidden)]
pub fn set_query_budget_for_tests(limit: usize) {
    let mut q = quota().lock().unwrap_or_else(|e| e.into_inner());
    q.limit = limit;
}

/// 测试专用（#[doc(hidden)]）：当前已占用的保留字节（会话淘汰/借用后仍
/// 计数，直到最后一个 Arc 释放）。
#[doc(hidden)]
pub fn query_retained_bytes_for_tests() -> usize {
    quota().lock().unwrap_or_else(|e| e.into_inner()).used
}

/// 测试专用（#[doc(hidden)]）：把预算恢复为产品默认值。
#[doc(hidden)]
pub fn reset_query_budget_for_tests() {
    let mut q = quota().lock().unwrap_or_else(|e| e.into_inner());
    q.limit = MAX_RETAINED_QUERY_BYTES;
}
