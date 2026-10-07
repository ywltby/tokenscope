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
//! - [`MAX_TOTAL_SNAPSHOT_EVENTS`] = 1_500_000：快照行只存索引（8 字节/行），
//!   事件本体在 Arc 采集快照中单份共享；150 万事件（UsageEvent 约 200 字节
//!   含字符串堆内存，见 query_snapshot_contract 测量）≈ 300 MB 上界，
//!   远超本机 1.2 GB 日志解析出的事件量。
//! - [`QUERY_IDLE_TTL`] = 10 分钟：一次分页浏览极少超过 10 分钟无读取；
//!   闲置会话优先淘汰，活跃会话（持续翻页）不被回收。
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
pub const MAX_TOTAL_SNAPSHOT_EVENTS: usize = 1_500_000;
pub const QUERY_IDLE_TTL: Duration = Duration::from_secs(600);

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
    collection: Arc<report::CollectionSnapshot>,
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
}

/// 下钻过滤指纹（模型/项目/日——明细侧子查询身份；游标/limit 不参与）。
pub(crate) fn drill_fingerprint(filter: &EventFilter) -> String {
    format!(
        "model={:?}|project={:?}|day={:?}",
        filter.model, filter.project, filter.day
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

/// 容量策略：优先淘汰最久未使用的会话直到放得下。
/// 单个新会话自身超总预算由调用方先拒绝（不截断事件集）。
fn evict_to_fit(reg: &mut HashMap<String, Session>, incoming_rows: usize) {
    let total_events = |reg: &HashMap<String, Session>| -> usize {
        reg.values().map(|s| s.snapshot.rows.len()).sum()
    };
    while reg.len() + 1 > MAX_ACTIVE_QUERIES
        || total_events(reg) + incoming_rows > MAX_TOTAL_SNAPSHOT_EVENTS
    {
        let Some(oldest) = reg
            .iter()
            .min_by_key(|(_, s)| s.last_used)
            .map(|(k, _)| k.clone())
        else {
            break;
        };
        log::info!("查询会话 {oldest} 被淘汰（容量/TTL 策略），其游标将显式过期");
        reg.remove(&oldest);
    }
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
    if kept.len() > MAX_TOTAL_SNAPSHOT_EVENTS {
        anyhow::bail!(
            "查询结果 {} 行超出快照预算 {} 行，请缩小时间范围",
            kept.len(),
            MAX_TOTAL_SNAPSHOT_EVENTS
        );
    }

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

    let snapshot = Arc::new(QuerySnapshot {
        // RC01：命名空间 + 序号 + generation。跨启动唯一性来自命名空间，
        // generation 仅用于诊断（同参并发采集合并时相同）。
        query_id: format!("q{namespace}-{qseq}-g{}", collection.generation),
        generation: collection.generation,
        pricing_revision: collection.pricing_revision.clone(),
        main_fingerprint,
        tz,
        tz_label,
        as_of,
        by: opts.by,
        rows,
        collection,
    });

    let now = Instant::now();
    let mut reg = registry().lock().unwrap();
    gc_expired(&mut reg, now);
    evict_to_fit(&mut reg, snapshot.rows.len());
    reg.insert(
        snapshot.query_id.clone(),
        Session {
            snapshot: snapshot.clone(),
            last_used: now,
        },
    );
    log::debug!(
        "查询会话 {} 建立：{} 行，价格修订 {}",
        snapshot.query_id,
        snapshot.rows.len(),
        snapshot.pricing_revision
    );
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
