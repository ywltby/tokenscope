//! 统一用量历史库（H01）：`~/.tokenscope/history.db`。
//!
//! 定位（计划 §3）：**持久事实存储**，不是可丢弃缓存。原生日志采集与 CCS
//! 手动导入写入同一份用量事实；来源文件被删除、来源停用、指纹重置或重新
//! 扫描都不删除已保存的用量。所有写入只落在 TokenScope 自己的数据目录。
//!
//! 结构要点：
//! - `usage_events` 一次已归一化请求一行，四桶 token 为经 Rust 校验的十进制
//!   `u64` **文本**（SQLite 有符号整数上限不得截断有效数值，也禁止 SQLite
//!   隐式转 REAL 汇总）；
//! - `event_aliases` 把同一请求的不同身份（原生身份、经兼容规则验证的 CCS
//!   身份）映射到同一事件行——跨来源导入去重靠它，不靠文件路径或批次；
//! - `event_origins` 记录该事件来自哪个源文件位置或哪条 CCS 来源记录（同一
//!   来源记录键唯一），并保留 CCS 侧的原模型/计价模型/来源费用/时间精度；
//! - `source_files` 只承载采集指纹与诊断，**不级联删除用量**；
//! - `ccs_daily_usage` 单独保存只有日粒度的外部汇总（H05 起使用），不与请求
//!   明细混表；`import_runs` 承载导入批次审计。
//!
//! 版本与身份修订（`history_meta` + `PRAGMA user_version`）：未知的更高版本
//! 直接报错并**保留文件**，绝不删表重建；迁移在写事务内完成，失败整体回滚。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::model::{AgentKind, TokenCounts};

/// 当前库结构版本（`PRAGMA user_version`）。
///
/// - v1 = `history_meta` + `source_files` + `usage_events`；
/// - v2 = 别名 / 来源记录 / CCS 日汇总 / 导入批次四表（H01 起）。
pub const HISTORY_SCHEMA_VERSION: i64 = 2;

/// 解析规则版本：适配器输出语义变化时递增（与事件一并存储，供重解析判定）。
pub const PARSER_REVISION: i64 = 1;

/// 模型身份规则版本：等价键规则升级时递增（只重建派生键，不删 token 事实）。
pub const IDENTITY_REVISION: i64 = 1;

/// 来源记录种类：原生日志文件内的位置。
pub const ORIGIN_KIND_NATIVE_FILE: &str = "native_file";
/// 来源记录种类：CCS 请求明细行。
pub const ORIGIN_KIND_CCS_REQUEST: &str = "ccs_request";
/// 来源记录种类：遗留 `cache.db` 迁移过来的已缓存用量。
pub const ORIGIN_KIND_LEGACY_CACHE: &str = "legacy_cache";

const META_GENERATION: &str = "generation";
const META_PARSER_REVISION: &str = "parser_revision";
const META_IDENTITY_REVISION: &str = "identity_revision";
const META_CREATED_UTC: &str = "created_utc";
/// H03：遗留 `cache.db` 一次性迁移标记（"done" = 已完成，不重复迁移）。
const META_LEGACY_MIGRATION: &str = "legacy_cache_migration";

/// v1 结构（历史版本，仅供迁移与测试构造旧库使用）。
const SCHEMA_V1: &str = r"
CREATE TABLE IF NOT EXISTS history_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS source_files (
    id INTEGER PRIMARY KEY,
    app TEXT NOT NULL,
    root TEXT NOT NULL,
    path TEXT NOT NULL,
    fingerprint TEXT NOT NULL DEFAULT '',
    size_bytes INTEGER NOT NULL DEFAULT 0,
    mtime_ms INTEGER NOT NULL DEFAULT 0,
    context_revision TEXT NOT NULL DEFAULT '',
    state TEXT NOT NULL DEFAULT 'present',
    last_seen_utc TEXT,
    last_success_utc TEXT,
    observations INTEGER NOT NULL DEFAULT 0,
    diagnostic TEXT,
    lines_seen INTEGER NOT NULL DEFAULT 0,
    bad_lines INTEGER NOT NULL DEFAULT 0,
    skipped_sidechain INTEGER NOT NULL DEFAULT 0,
    skipped_synthetic INTEGER NOT NULL DEFAULT 0,
    skipped_zero_usage INTEGER NOT NULL DEFAULT 0,
    skipped_no_model INTEGER NOT NULL DEFAULT 0,
    ignored_token_usage_record INTEGER NOT NULL DEFAULT 0,
    parsed_events INTEGER NOT NULL DEFAULT 0,
    UNIQUE(app, root, path)
);
CREATE TABLE IF NOT EXISTS usage_events (
    id INTEGER PRIMARY KEY,
    event_key TEXT NOT NULL UNIQUE,
    app TEXT NOT NULL,
    ts_seconds INTEGER NOT NULL,
    ts_nanos INTEGER NOT NULL CHECK (ts_nanos BETWEEN 0 AND 999999999),
    model_raw TEXT NOT NULL,
    model_identity TEXT NOT NULL,
    identity_revision INTEGER NOT NULL,
    session_id TEXT,
    record_id TEXT,
    project_key TEXT,
    session_initial_cwd TEXT,
    event_cwd TEXT,
    input_tokens TEXT NOT NULL,
    output_tokens TEXT NOT NULL,
    cache_write_tokens TEXT NOT NULL,
    cache_read_tokens TEXT NOT NULL,
    origin_rank INTEGER NOT NULL DEFAULT 1,
    observed_at_utc TEXT NOT NULL,
    observed_at_seconds INTEGER NOT NULL,
    observed_at_nanos INTEGER NOT NULL CHECK (observed_at_nanos BETWEEN 0 AND 999999999),
    parser_revision INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_usage_time
    ON usage_events(ts_seconds, ts_nanos, id);
CREATE INDEX IF NOT EXISTS idx_usage_app_time
    ON usage_events(app, ts_seconds, ts_nanos, id);
CREATE INDEX IF NOT EXISTS idx_usage_project_time
    ON usage_events(project_key, ts_seconds, ts_nanos, id);
CREATE INDEX IF NOT EXISTS idx_usage_model_time
    ON usage_events(model_identity, ts_seconds, ts_nanos, id);
";

/// v2 结构：跨来源身份、来源证据、CCS 日粒度汇总、导入批次审计。
const SCHEMA_V2: &str = r"
CREATE TABLE IF NOT EXISTS event_aliases (
    id INTEGER PRIMARY KEY,
    event_id INTEGER NOT NULL REFERENCES usage_events(id),
    app TEXT NOT NULL,
    identity_scheme TEXT NOT NULL,
    identity_value TEXT NOT NULL,
    created_generation INTEGER NOT NULL DEFAULT 0,
    UNIQUE(app, identity_scheme, identity_value)
);
CREATE INDEX IF NOT EXISTS idx_alias_event ON event_aliases(event_id);
CREATE TABLE IF NOT EXISTS event_origins (
    id INTEGER PRIMARY KEY,
    event_id INTEGER NOT NULL REFERENCES usage_events(id),
    origin_kind TEXT NOT NULL,
    origin_key TEXT NOT NULL,
    app TEXT NOT NULL,
    import_run_id INTEGER,
    parser_revision INTEGER NOT NULL,
    source_model_raw TEXT,
    pricing_model TEXT,
    source_cost_usd TEXT,
    ts_precision_seconds INTEGER NOT NULL DEFAULT 0,
    UNIQUE(origin_kind, origin_key)
);
CREATE INDEX IF NOT EXISTS idx_origin_event ON event_origins(event_id);
CREATE TABLE IF NOT EXISTS import_runs (
    id INTEGER PRIMARY KEY,
    logical_source TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_schema TEXT NOT NULL,
    started_utc TEXT NOT NULL,
    committed_utc TEXT,
    status TEXT NOT NULL,
    detail TEXT,
    requests_inserted INTEGER NOT NULL DEFAULT 0,
    requests_updated INTEGER NOT NULL DEFAULT 0,
    requests_unchanged INTEGER NOT NULL DEFAULT 0,
    requests_conflicted INTEGER NOT NULL DEFAULT 0,
    requests_stale INTEGER NOT NULL DEFAULT 0,
    requests_rejected INTEGER NOT NULL DEFAULT 0,
    rollups_snapshotted INTEGER NOT NULL DEFAULT 0,
    rollups_conflicted INTEGER NOT NULL DEFAULT 0,
    net_input_tokens TEXT NOT NULL DEFAULT '0',
    net_output_tokens TEXT NOT NULL DEFAULT '0',
    net_cache_write_tokens TEXT NOT NULL DEFAULT '0',
    net_cache_read_tokens TEXT NOT NULL DEFAULT '0',
    import_revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS ccs_daily_usage (
    id INTEGER PRIMARY KEY,
    logical_source TEXT NOT NULL,
    day TEXT NOT NULL,
    source_tz TEXT NOT NULL,
    app TEXT NOT NULL,
    provider_id TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    request_model TEXT NOT NULL DEFAULT '',
    pricing_model TEXT NOT NULL DEFAULT '',
    request_count INTEGER NOT NULL,
    input_tokens TEXT NOT NULL,
    output_tokens TEXT NOT NULL,
    cache_write_tokens TEXT NOT NULL,
    cache_read_tokens TEXT NOT NULL,
    input_semantics INTEGER NOT NULL,
    source_cost_usd TEXT,
    import_run_id INTEGER,
    revision INTEGER NOT NULL DEFAULT 1,
    UNIQUE(logical_source, day, source_tz, app, provider_id, model, request_model, pricing_model)
);
CREATE INDEX IF NOT EXISTS idx_ccs_daily_day ON ccs_daily_usage(day, app, model);
";

/// v1 建表 SQL（迁移使用；集成测试据此构造旧版本库以验证升级保留历史）。
#[doc(hidden)]
pub fn legacy_v1_schema_for_tests() -> &'static str {
    SCHEMA_V1
}

/// 版本化迁移表：`(目标版本, SQL)`，从 0 起按序执行。
const MIGRATIONS: &[(i64, &str)] = &[(1, SCHEMA_V1), (2, SCHEMA_V2)];

/// H03：遗留缓存迁移的每批事务规模（过大则事务时长与内存都失控）。
const LEGACY_MIGRATION_CHUNK: usize = 500;

/// 写入优先级：数值越大越可信。原生日志终值优先于 CCS 导入值；同一优先级
/// 内部按观察时间取更晚者（计划 §5.3：与导入顺序无关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WritePrecedence {
    CcsImport = 0,
    NativeLog = 1,
}

impl WritePrecedence {
    fn rank(self) -> i64 {
        self as i64
    }

    fn from_rank(rank: i64) -> Self {
        if rank >= WritePrecedence::NativeLog.rank() {
            WritePrecedence::NativeLog
        } else {
            WritePrecedence::CcsImport
        }
    }
}

/// 同一请求的一条来源身份（`(app, scheme, value)` 全局唯一）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAlias {
    pub app: AgentKind,
    pub scheme: String,
    pub value: String,
}

/// 事件的一条来源证据（同一 `(kind, key)` 只能指向一个事件）。
#[derive(Debug, Clone, PartialEq)]
pub struct EventOrigin {
    pub origin_kind: String,
    pub origin_key: String,
    pub app: AgentKind,
    pub import_run_id: Option<i64>,
    pub parser_revision: i64,
    /// CCS 侧原模型名（保留原值，供口径核对）。
    pub source_model_raw: Option<String>,
    /// CCS 侧计价模型名。
    pub pricing_model: Option<String>,
    /// CCS 来源费用（与 TokenScope 当前费率估算分开保存）。
    pub source_cost_usd: Option<f64>,
    /// true = 来源时间只有秒级精度（CCS 请求），不伪造纳秒。
    pub ts_precision_seconds: bool,
}

/// 一次事件写入（合并语义由 [`HistoryDb::write_batch`] 统一实施）。
#[derive(Debug, Clone, PartialEq)]
pub struct EventWrite {
    /// 来源记录主键（原生 = 原生记录身份；CCS = 逻辑来源 + 原始 request_id）。
    pub event_key: String,
    pub app: AgentKind,
    pub ts: Timestamp,
    pub model_raw: String,
    pub model_identity: String,
    pub session_id: Option<String>,
    pub record_id: Option<String>,
    pub project_key: Option<String>,
    pub session_initial_cwd: Option<String>,
    pub event_cwd: Option<String>,
    pub tokens: TokenCounts,
    pub precedence: WritePrecedence,
    /// 本次观察时间（同一优先级下判定终值新旧；不影响展示口径）。
    pub observed_at: Timestamp,
    /// 该请求可证实的身分别名（原生身份 / CCS 兼容解析出的原生身份）。
    pub aliases: Vec<EventAlias>,
    /// 该请求的来源证据（源文件位置 / CCS 来源记录）。
    pub origins: Vec<EventOrigin>,
}

/// 批次写入结果（逐类计数，与计划 §7 H05 的预览/审计口径一致）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct WriteSummary {
    /// 新插入的事件数。
    pub inserted: u64,
    /// 同一事件的终值更新数。
    pub updated: u64,
    /// 内容完全相同、跳过的记录数。
    pub unchanged: u64,
    /// 低优先级来源（CCS）与高优先级来源（原生）冲突、保留原生值的记录数。
    pub conflicts: u64,
    /// 观察时间早于已存终值、保留较新值的记录数。
    pub stale: u64,
    /// 新增的别名行数。
    pub aliases_added: u64,
    /// 新增的来源证据行数。
    pub origins_added: u64,
}

impl WriteSummary {
    /// 本批次是否改变了用量行（插入或更新）。
    pub fn changed_events(&self) -> u64 {
        self.inserted + self.updated
    }
}

/// 单条写入在库内的处置分类（预览与提交共用同一判定，见
/// [`HistoryDb::classify_write`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteClass {
    /// 库内没有该请求 → 新增。
    Insert,
    /// 同一请求的新终值 → 更新。
    Update,
    /// 内容完全相同 → 跳过。
    Unchanged,
    /// 低优先级来源与已存高优先级终值冲突 → 保留已存值。
    Conflict,
    /// 观察时间早于已存终值 → 保留较新值。
    Stale,
}

/// 日粒度汇总快照的一条写入（CCS 完整主键 + 来源时区 + 归一化四桶）。
///
/// 与请求明细分表保存：日汇总没有逐请求时间、会话与 cwd，不能伪造成请求
/// 事件，也不能与明细直接相加（计划 §5.3）。
#[derive(Debug, Clone, PartialEq)]
pub struct DailyRollupWrite {
    pub logical_source: String,
    /// CCS 原始统计日（`YYYY-MM-DD`，来源时区下的自然日，不重切时区）。
    pub day: String,
    /// 来源统计时区（CCS 用 `localtime` 生成日键，这里存导入时的本机时区假设）。
    pub source_tz: String,
    pub app: AgentKind,
    pub provider_id: String,
    pub model: String,
    pub request_model: String,
    pub pricing_model: String,
    pub request_count: u64,
    pub tokens: TokenCounts,
    /// 归一化后的输入口径版本（`2` = fresh，与 CCS rollup 写入口径一致）。
    pub input_semantics: i64,
    pub source_cost_usd: Option<f64>,
    /// 来源可验证修订；CCS 不提供 → 0（内容不同的同键快照一律计冲突，
    /// 不自动认定"后导入的就是更新版本"）。
    pub revision: i64,
}

/// 同键快照内容冲突时的处置策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RollupConflictPolicy {
    /// 默认：保留已存快照，把差异列为冲突交给用户决定。
    #[default]
    KeepExisting,
    /// 用户在本轮预览中明确选择"以来源为准"。
    TakeSource,
}

/// 日汇总批次结果。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RollupSummary {
    pub inserted: u64,
    pub unchanged: u64,
    pub replaced: u64,
    pub conflicted: u64,
}

/// 日汇总的库内处置分类（预览与提交共用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RollupClass {
    /// 库内没有同键快照。
    New,
    /// 同键内容完全相同。
    Unchanged,
    /// 同键内容不同：来源缺少可验证修订时按冲突呈现，不静默覆盖较新值。
    Conflicting,
}

/// 一次导入批次的审计记录（与数据同事务提交）。
#[derive(Debug, Clone, PartialEq)]
pub struct ImportRunRecord {
    pub logical_source: String,
    pub source_path: String,
    pub source_schema: String,
    pub started_utc: String,
    pub status: String,
    pub detail: Option<String>,
    pub import_revision: i64,
}

/// 导入批次结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ImportOutcome {
    pub run_id: i64,
    pub events: WriteSummary,
    pub rollups: RollupSummary,
}

/// 库内事件（读取形态）。
#[derive(Debug, Clone, PartialEq)]
pub struct StoredEvent {
    pub id: i64,
    pub event_key: String,
    pub app: AgentKind,
    pub ts: Timestamp,
    pub model_raw: String,
    pub model_identity: String,
    pub identity_revision: i64,
    pub session_id: Option<String>,
    pub record_id: Option<String>,
    pub project_key: Option<String>,
    pub session_initial_cwd: Option<String>,
    pub event_cwd: Option<String>,
    pub tokens: TokenCounts,
    pub precedence: WritePrecedence,
    pub observed_at: Timestamp,
    pub parser_revision: i64,
}

/// 来源文件身份（`(app, root, path)` 唯一）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileKey {
    pub app: AgentKind,
    pub root: String,
    pub path: String,
}

/// 来源文件登记（缺失只更新状态，绝不级联删除用量）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFileRecord {
    pub key: SourceFileKey,
    pub fingerprint: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub context_revision: String,
    pub last_success_utc: Option<String>,
    /// H04：本次文件解析的逐文件统计。指纹命中（本轮未重解析）时由采集层
    /// 沿用上一次写入的值，来源统计因此始终完整可见（与旧缓存的语义一致）。
    pub stats: crate::source::CollectStats,
}

/// 来源文件读取形态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredSourceFile {
    pub key: SourceFileKey,
    pub fingerprint: String,
    pub size_bytes: u64,
    pub mtime_ms: i64,
    pub context_revision: String,
    pub state: String,
    pub last_seen_utc: Option<String>,
    pub last_success_utc: Option<String>,
    pub observations: u64,
    pub stats: crate::source::CollectStats,
}

/// TokenScope 数据目录下的历史库路径（`~/.tokenscope/history.db`）。
pub fn history_file_path() -> Result<PathBuf> {
    Ok(crate::report::data_dir()?.join("history.db"))
}

/// 打开/创建历史库（含版本迁移）。未知的更高版本一律报错且保留文件。
#[derive(Debug)]
pub struct HistoryDb {
    conn: Connection,
    path: PathBuf,
}

impl HistoryDb {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("创建历史库目录失败: {}", dir.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("打开历史库失败: {}", path.display()))?;
        // 并发打开时等锁而不是立刻报忙；WAL 切换需要短暂独占，小步重试
        //（与缓存层同一取舍：并发打开不得被误判为损坏）。
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        for attempt in 0..100 {
            match conn.pragma_update(None, "journal_mode", "WAL") {
                Ok(()) => break,
                Err(e)
                    if attempt < 99
                        && matches!(
                            &e,
                            rusqlite::Error::SqliteFailure(f, _)
                                if f.code == rusqlite::ErrorCode::DatabaseBusy
                        ) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                Err(e) => return Err(e).context("设置历史库 WAL 日志模式失败"),
            }
        }
        conn.pragma_update(None, "foreign_keys", "ON")
            .context("启用历史库外键约束失败")?;
        // 持久事实存储：提交即落盘（WAL + FULL），不因崩溃丢已确认的用量。
        conn.pragma_update(None, "synchronous", "FULL")
            .context("设置历史库同步级别失败")?;
        let db = Self {
            conn,
            path: path.to_path_buf(),
        };
        db.migrate().map_err(|e| {
            // 错误链整体带出：迁移失败原因（未知版本 / SQL 失败）必须可见，
            // 后续 context 不得把它压成一句无信息量的"迁移失败"。
            anyhow!(
                "{:#}（历史库文件保留在 {}，可重试；不执行删表重建）",
                e,
                path.display()
            )
        })?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 当前库结构版本（`PRAGMA user_version`）。
    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?)
    }

    /// 版本迁移：事务内按序执行，失败整体回滚并保留原库。
    fn migrate(&self) -> Result<()> {
        let current = self.schema_version()?;
        if current > HISTORY_SCHEMA_VERSION {
            bail!(
                "历史库结构版本 {current} 高于本程序支持的 {HISTORY_SCHEMA_VERSION}；\
                 保留文件不作修改，请升级 TokenScope 后再打开"
            );
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        for (version, sql) in MIGRATIONS {
            if *version > current {
                tx.execute_batch(sql)
                    .with_context(|| format!("执行 v{version} 迁移失败"))?;
                tx.pragma_update(None, "user_version", *version)?;
                log::info!("历史库迁移：v{current} → v{version}");
            }
        }
        // 元信息补齐（已存在的值一律保留：迁移不重置 generation 等计数）。
        let now = Timestamp::now().to_string();
        for (key, value) in [
            (META_GENERATION, "0".to_string()),
            (META_PARSER_REVISION, PARSER_REVISION.to_string()),
            (META_IDENTITY_REVISION, IDENTITY_REVISION.to_string()),
            (META_CREATED_UTC, now),
        ] {
            tx.execute(
                "INSERT INTO history_meta(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO NOTHING",
                params![key, value],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 读取元信息值。
    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM history_meta WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    /// 写入（或覆盖）元信息值。
    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO history_meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// 用量/身份 generation：只有用量与影响查询的身份数据变化才递增。
    pub fn generation(&self) -> Result<u64> {
        match self.meta(META_GENERATION)? {
            Some(v) => parse_u64_text(&v, META_GENERATION),
            None => Ok(0),
        }
    }

    /// H03：遗留缓存迁移标记是否已完成。
    pub fn legacy_cache_migrated(&self) -> Result<bool> {
        Ok(self.meta(META_LEGACY_MIGRATION)?.as_deref() == Some("done"))
    }

    /// H03：标记遗留缓存迁移完成（一次性，由调用方在整批提交后写入）。
    pub fn mark_legacy_cache_migrated(&self) -> Result<()> {
        self.set_meta(META_LEGACY_MIGRATION, "done")
    }

    /// 批次写入：在**一个写事务**内按已存在的来源键/原生身份别名合并事件。
    ///
    /// 顺序无关性：同一请求的原生值与 CCS 值无论谁先写，最终都保留原生终值
    ///（优先级比较，不做逐桶相加、不做逐桶取最大值拼新请求）。完全相同的
    /// 重复写入不改变任何行，也不递增 generation。
    pub fn write_batch(&self, batch: &[EventWrite]) -> Result<WriteSummary> {
        if batch.is_empty() {
            return Ok(WriteSummary::default());
        }
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let generation = self.generation()?;
        let (summary, changed) = write_events_in_tx(&tx, batch, generation)?;
        if changed {
            bump_generation(&tx)?;
        }
        tx.commit()?;
        Ok(summary)
    }

    /// 单条写入在库内的处置分类（**预览与提交共用同一判定**，因此预览数字
    /// 与提交结果不会分叉）。只读，不改变库内容。
    pub fn classify_write(&self, w: &EventWrite) -> Result<WriteClass> {
        validate_write(w)?;
        match lookup_event(&self.conn, w)? {
            None => Ok(WriteClass::Insert),
            Some(existing) => Ok(classify_against(&existing, w)),
        }
    }

    /// 日汇总快照在库内的处置分类（只读）。
    pub fn classify_rollup(&self, r: &DailyRollupWrite) -> Result<RollupClass> {
        let existing: Option<(i64, String, String, String, String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT request_count, input_tokens, output_tokens, cache_write_tokens,
                        cache_read_tokens, source_cost_usd
                 FROM ccs_daily_usage
                 WHERE logical_source = ?1 AND day = ?2 AND source_tz = ?3 AND app = ?4
                   AND provider_id = ?5 AND model = ?6 AND request_model = ?7 AND pricing_model = ?8",
                params![
                    r.logical_source,
                    r.day,
                    r.source_tz,
                    r.app.as_str(),
                    r.provider_id,
                    r.model,
                    r.request_model,
                    r.pricing_model
                ],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;
        Ok(match existing {
            None => RollupClass::New,
            Some((count, i, o, cw, cr, cost)) => {
                let same = count == r.request_count as i64
                    && i == r.tokens.input.to_string()
                    && o == r.tokens.output.to_string()
                    && cw == r.tokens.cache_write.to_string()
                    && cr == r.tokens.cache_read.to_string()
                    && cost == r.source_cost_usd.map(|v| v.to_string());
                if same {
                    RollupClass::Unchanged
                } else {
                    RollupClass::Conflicting
                }
            }
        })
    }

    /// 导入批次的净新增用量（该批次来源指向的事件的四桶之和）。
    pub fn import_run_net_tokens(&self, run_id: i64) -> Result<TokenCounts> {
        net_new_tokens_conn(&self.conn, run_id)
    }

    /// H05：CCS 导入的一个批次——事件、日汇总快照与导入审计**在同一事务**内
    /// 提交（计划 §5.1：批次与数据同事务，崩溃或失败整体回滚）。
    pub fn apply_import_batch(
        &self,
        writes: &[EventWrite],
        rollups: &[DailyRollupWrite],
        policy: RollupConflictPolicy,
        run: &ImportRunRecord,
    ) -> Result<ImportOutcome> {
        let tx = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let generation = self.generation()?;
        // 先落批次行拿到 id：来源证据要记录"这批用量来自哪次导入"。
        let run_id = insert_import_run(&tx, run)?;
        let mut writes = writes.to_vec();
        for w in &mut writes {
            for o in &mut w.origins {
                if o.import_run_id.is_none() {
                    o.import_run_id = Some(run_id);
                }
            }
        }
        let (write_summary, events_changed) = write_events_in_tx(&tx, &writes, generation)?;
        let (rollup_summary, rollups_changed) = write_rollups_in_tx(&tx, rollups, policy, run_id)?;
        let changed = events_changed || rollups_changed;
        // 完全相同的重复导入：不新增用量、不重复来源、不递增 generation，
        // 但审计批次照常登记（计划 §3：可以记录审计结果）。
        finish_import_run(&tx, run_id, run, &write_summary, &rollup_summary)?;
        if changed {
            bump_generation(&tx)?;
        }
        tx.commit()?;
        Ok(ImportOutcome {
            run_id,
            events: write_summary,
            rollups: rollup_summary,
        })
    }

    /// 事件总数。
    pub fn event_count(&self) -> Result<u64> {
        self.count("usage_events")
    }

    /// 别名行数。
    pub fn alias_count(&self) -> Result<u64> {
        self.count("event_aliases")
    }

    /// 来源证据行数。
    pub fn origin_count(&self) -> Result<u64> {
        self.count("event_origins")
    }

    /// 来源文件登记数。
    pub fn source_file_count(&self) -> Result<u64> {
        self.count("source_files")
    }

    /// 日汇总快照行数（诊断与测试）。
    pub fn rollup_count(&self) -> Result<u64> {
        self.count("ccs_daily_usage")
    }

    /// 日汇总快照的请求数合计（诊断与冲突核对）。
    pub fn rollup_request_count(&self) -> Result<u64> {
        let n: i64 = self.conn.query_row(
            "SELECT COALESCE(SUM(request_count), 0) FROM ccs_daily_usage",
            [],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    fn count(&self, table: &str) -> Result<u64> {
        let n: i64 = self
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// 全部事件（按 `(ts_seconds, ts_nanos, id)` 稳定排序）。
    pub fn stored_events(&self) -> Result<Vec<StoredEvent>> {
        self.stored_events_filtered(None)
    }

    /// H04：按应用读取事件（`None` = 全部应用）——采集提交后回填查询输入。
    ///
    /// 返回的是**库内全部事实**，与"本轮哪些文件被重新解析"无关：来源文件
    /// 已删除、来源已停用、指纹被重置都不会让已保存的用量消失（计划不变量 3）。
    pub fn stored_events_filtered(&self, app: Option<AgentKind>) -> Result<Vec<StoredEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_key, app, ts_seconds, ts_nanos, model_raw, model_identity,
                    identity_revision, session_id, record_id, project_key,
                    session_initial_cwd, event_cwd, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, origin_rank, observed_at_seconds,
                    observed_at_nanos, parser_revision
             FROM usage_events
             WHERE (?1 IS NULL OR app = ?1)
             ORDER BY ts_seconds, ts_nanos, id",
        )?;
        let rows = stmt.query_map(params![app.map(|a| a.as_str())], row_to_stored_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// H04：重扫日志——重置采集指纹（下次采集按新文件重新解析），
    /// **不**清 `usage_events`／`event_aliases`／`event_origins`／`ccs_daily_usage`
    /// （历史删除不是缓存操作）。返回清掉的来源登记行数。
    pub fn reset_fingerprints(&self) -> Result<u64> {
        let n = self.conn.execute("DELETE FROM source_files", [])?;
        Ok(n as u64)
    }

    /// 四桶合计：**流式读取 + Rust 受检累加**（不把全表拉进内存，也不让
    /// SQLite 隐式转 REAL）；溢出明确报错，不回绕、不饱和。
    pub fn totals(&self) -> Result<TokenCounts> {
        self.totals_filtered(None)
    }

    /// 按应用过滤的四桶合计（`None` = 全部应用）。
    pub fn totals_filtered(&self, app: Option<AgentKind>) -> Result<TokenCounts> {
        let sql = "SELECT input_tokens, output_tokens, cache_write_tokens, cache_read_tokens
                   FROM usage_events WHERE (?1 IS NULL OR app = ?1)";
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![app.map(|a| a.as_str())], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        let mut acc = TokenCounts::default();
        for row in rows {
            let (i, o, cw, cr) = row?;
            let counts = TokenCounts {
                input: parse_u64_text(&i, "input_tokens")?,
                output: parse_u64_text(&o, "output_tokens")?,
                cache_write: parse_u64_text(&cw, "cache_write_tokens")?,
                cache_read: parse_u64_text(&cr, "cache_read_tokens")?,
            };
            acc = acc
                .checked_add(&counts)
                .ok_or_else(|| anyhow!("历史库 token 合计溢出可表示范围"))?;
        }
        Ok(acc)
    }

    /// 登记来源文件指纹（幂等；重新出现时把状态改回 present）。
    pub fn touch_source_file(&self, rec: &SourceFileRecord) -> Result<()> {
        let now = Timestamp::now().to_string();
        self.conn.execute(
            "INSERT INTO source_files(app, root, path, fingerprint, size_bytes, mtime_ms,
                                      context_revision, state, last_seen_utc, last_success_utc,
                                      observations, lines_seen, bad_lines, skipped_sidechain,
                                      skipped_synthetic, skipped_zero_usage, skipped_no_model,
                                      ignored_token_usage_record, parsed_events)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'present', ?8, ?9, 1, ?10, ?11, ?12, ?13, ?14,
                    ?15, ?16, ?17)
             ON CONFLICT(app, root, path) DO UPDATE SET
                fingerprint = excluded.fingerprint,
                size_bytes = excluded.size_bytes,
                mtime_ms = excluded.mtime_ms,
                context_revision = excluded.context_revision,
                state = 'present',
                last_seen_utc = excluded.last_seen_utc,
                last_success_utc = COALESCE(excluded.last_success_utc, source_files.last_success_utc),
                observations = source_files.observations + 1,
                lines_seen = excluded.lines_seen,
                bad_lines = excluded.bad_lines,
                skipped_sidechain = excluded.skipped_sidechain,
                skipped_synthetic = excluded.skipped_synthetic,
                skipped_zero_usage = excluded.skipped_zero_usage,
                skipped_no_model = excluded.skipped_no_model,
                ignored_token_usage_record = excluded.ignored_token_usage_record,
                parsed_events = excluded.parsed_events",
            params![
                rec.key.app.as_str(),
                rec.key.root,
                rec.key.path,
                rec.fingerprint,
                rec.size_bytes as i64,
                rec.mtime_ms,
                rec.context_revision,
                now,
                rec.last_success_utc,
                rec.stats.lines_seen as i64,
                rec.stats.bad_lines as i64,
                rec.stats.skipped_sidechain as i64,
                rec.stats.skipped_synthetic as i64,
                rec.stats.skipped_zero_usage as i64,
                rec.stats.skipped_no_model as i64,
                rec.stats.ignored_token_usage_record as i64,
                rec.stats.events as i64,
            ],
        )?;
        Ok(())
    }

    /// 标记来源文件缺失：**只更新状态**，已保存用量不受影响（计划不变量 3）。
    pub fn mark_sources_missing(&self, keys: &[SourceFileKey]) -> Result<u64> {
        let now = Timestamp::now().to_string();
        let mut n = 0u64;
        for key in keys {
            n += self.conn.execute(
                "UPDATE source_files SET state = 'missing', last_seen_utc = ?4
                 WHERE app = ?1 AND root = ?2 AND path = ?3",
                params![key.app.as_str(), key.root, key.path, now],
            )? as u64;
        }
        Ok(n)
    }

    /// 登记为"采集失败/不可读"（保留原指纹，便于下轮重试）。
    pub fn mark_source_unreadable(&self, key: &SourceFileKey, diagnostic: &str) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE source_files SET state = 'unreadable', diagnostic = ?4
             WHERE app = ?1 AND root = ?2 AND path = ?3",
            params![key.app.as_str(), key.root, key.path, diagnostic],
        )?;
        Ok(n > 0)
    }

    /// 来源文件登记列表（按应用与路径排序）。
    pub fn source_files(&self) -> Result<Vec<StoredSourceFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT app, root, path, fingerprint, size_bytes, mtime_ms, context_revision,
                    state, last_seen_utc, last_success_utc, observations, lines_seen,
                    bad_lines, skipped_sidechain, skipped_synthetic, skipped_zero_usage,
                    skipped_no_model, ignored_token_usage_record, parsed_events
             FROM source_files ORDER BY app, root, path",
        )?;
        let rows = stmt.query_map([], |r| {
            let raw_app: String = r.get(0)?;
            let app = parse_agent(&raw_app).map_err(|e| conversion_error(0, e))?;
            Ok(StoredSourceFile {
                key: SourceFileKey {
                    app,
                    root: r.get(1)?,
                    path: r.get(2)?,
                },
                fingerprint: r.get(3)?,
                size_bytes: r.get::<_, i64>(4)?.max(0) as u64,
                mtime_ms: r.get(5)?,
                context_revision: r.get(6)?,
                state: r.get(7)?,
                last_seen_utc: r.get(8)?,
                last_success_utc: r.get(9)?,
                observations: r.get::<_, i64>(10)?.max(0) as u64,
                stats: crate::source::CollectStats {
                    lines_seen: r.get::<_, i64>(11)?.max(0) as u64,
                    bad_lines: r.get::<_, i64>(12)?.max(0) as u64,
                    skipped_sidechain: r.get::<_, i64>(13)?.max(0) as u64,
                    skipped_synthetic: r.get::<_, i64>(14)?.max(0) as u64,
                    skipped_zero_usage: r.get::<_, i64>(15)?.max(0) as u64,
                    skipped_no_model: r.get::<_, i64>(16)?.max(0) as u64,
                    ignored_token_usage_record: r.get::<_, i64>(17)?.max(0) as u64,
                    events: r.get::<_, i64>(18)?.max(0) as u64,
                    ..crate::source::CollectStats::default()
                },
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// 库内表名（诊断与迁移验证用）。
    #[doc(hidden)]
    pub fn table_names(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

/// H03：遗留缓存迁移结果（一次性）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LegacyMigrationReport {
    /// 读到的旧缓存文件数。
    pub files: u64,
    /// 读到的旧缓存事件行数。
    pub events_seen: u64,
    pub inserted: u64,
    pub updated: u64,
    pub unchanged: u64,
    pub conflicts: u64,
    pub stale: u64,
    /// 字段不合法（时间戳不可解析、token 越界等）而**未**迁入的行数。
    pub rejected: u64,
}

/// H03：把遗留 `cache.db` 里已缓存的原生用量一次性迁入历史库。
///
/// 顺序要求（计划 §6）：必须在**任何** `Cache::open` 的版本重建/purge 之前
/// 执行——本函数只以只读方式读取旧库，不打开、不重建、不清理它。
///
/// - 逐条只迁**合法**字段：时间戳不可解析、token 越界或桶组合不可表示的行
///   计 `rejected` 跳过，一条坏行不得毁掉整批迁移；
/// - 事件身份 = 旧缓存行位置（`(app, path, 行序)`），原生身份（Claude 的
///   `(session_id, message.id)`）另作别名登记：重扫同一文件时命中同一事件，
///   不产生重复用量；
/// - 迁移**不读源日志**：源文件已被清理时旧缓存里的用量照样保留；
/// - 完成后写一次性标记：重复调用直接跳过（`Ok(None)`）；失败不写标记，
///   整批可用同一份旧库重试。
pub fn migrate_legacy_cache(
    history: &HistoryDb,
    legacy_cache: &Path,
) -> Result<Option<LegacyMigrationReport>> {
    if history.legacy_cache_migrated()? {
        return Ok(None);
    }
    if !legacy_cache.exists() {
        // 没有旧缓存可迁：直接标记完成，避免每次启动都尝试打开。
        history.mark_legacy_cache_migrated()?;
        return Ok(Some(LegacyMigrationReport::default()));
    }
    let files = crate::cache::read_legacy_cache(legacy_cache)?;
    let mut report = LegacyMigrationReport {
        files: files.len() as u64,
        ..Default::default()
    };
    let mut batch: Vec<EventWrite> = Vec::new();
    for file in &files {
        for (idx, ev) in file.events.iter().enumerate() {
            report.events_seen += 1;
            match legacy_event_write(file, idx, ev) {
                Ok(w) => batch.push(w),
                Err(e) => {
                    report.rejected += 1;
                    log::debug!("遗留缓存迁移跳过一条事件（{} / {}）：{e}", file.path, idx);
                }
            }
        }
    }
    for chunk in batch.chunks(LEGACY_MIGRATION_CHUNK) {
        let s = history.write_batch(chunk)?;
        report.inserted += s.inserted;
        report.updated += s.updated;
        report.unchanged += s.unchanged;
        report.conflicts += s.conflicts;
        report.stale += s.stale;
    }
    // 只在整批成功提交后落标记：失败（含中途崩溃）可原样重试。
    history.mark_legacy_cache_migrated()?;
    log::info!(
        "遗留缓存迁移完成：文件 {}，事件行 {}，新增 {}，更新 {}，已存在 {}，冲突 {}，拒绝 {}",
        report.files,
        report.events_seen,
        report.inserted,
        report.updated,
        report.unchanged,
        report.conflicts,
        report.rejected
    );
    Ok(Some(report))
}

/// 单条遗留缓存事件的迁移形态（字段不合法即拒绝该条，不猜测）。
fn legacy_event_write(
    file: &crate::cache::LegacyCacheFile,
    idx: usize,
    ev: &crate::cache::LegacyEvent,
) -> Result<EventWrite> {
    let ts: Timestamp = ev
        .ts
        .parse()
        .map_err(|e| anyhow!("时间戳不可解析（{:?}）: {e}", ev.ts))?;
    if ev.model.trim().is_empty() {
        bail!("模型名为空");
    }
    let tokens = TokenCounts {
        input: ev.input,
        output: ev.output,
        cache_write: ev.cache_write,
        cache_read: ev.cache_read,
    };
    let total = tokens
        .input
        .checked_add(tokens.output)
        .and_then(|v| v.checked_add(tokens.cache_write))
        .and_then(|v| v.checked_add(tokens.cache_read))
        .ok_or_else(|| anyhow!("四桶合计超出可表示范围"))?;
    let _ = total;
    // 旧缓存里的行序（v11+）优先；更早版本没有该列，用行位置兜底。
    let position = if ev.line > 0 { ev.line } else { idx as u64 };
    let mut aliases = Vec::new();
    if !ev.session_id.is_empty() && !ev.record_id.is_empty() {
        aliases.push(EventAlias {
            app: file.app,
            scheme: "claude-message".to_string(),
            value: format!("{}|{}", ev.session_id, ev.record_id),
        });
    }
    Ok(EventWrite {
        event_key: format!(
            "legacy-cache|{}|{}|{position}",
            file.app.as_str(),
            file.path
        ),
        app: file.app,
        ts,
        model_raw: ev.model.clone(),
        model_identity: crate::model_identity::ModelIdentity::parse(&ev.model).identity_key(),
        session_id: non_empty(&ev.session_id),
        record_id: non_empty(&ev.record_id),
        project_key: non_empty(&ev.project),
        session_initial_cwd: ev.session_initial_cwd.clone(),
        event_cwd: ev.event_cwd.clone(),
        tokens,
        // 旧缓存保存的是已经解析过的原生用量，优先级与原生日志一致；
        // 观察时间取事件时间，重扫得到同一终值时按内容相等跳过。
        precedence: WritePrecedence::NativeLog,
        observed_at: ts,
        aliases,
        origins: vec![EventOrigin {
            origin_kind: ORIGIN_KIND_LEGACY_CACHE.to_string(),
            origin_key: format!(
                "legacy-cache|{}|{}|{position}",
                file.app.as_str(),
                file.path
            ),
            app: file.app,
            import_run_id: None,
            // 旧缓存的解析版本未知（可能早于版本号机制），记 0 = 遗留。
            parser_revision: 0,
            source_model_raw: None,
            pricing_model: None,
            source_cost_usd: None,
            ts_precision_seconds: false,
        }],
    })
}

fn non_empty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

/// 批次写入的**事务内**内核：写入事件、别名与来源证据，并回填统计。
/// 返回 `(统计, 是否改变了任何行)`；是否递增 generation 与提交由调用方决定。
fn write_events_in_tx(
    tx: &Transaction<'_>,
    batch: &[EventWrite],
    generation: u64,
) -> Result<(WriteSummary, bool)> {
    let mut summary = WriteSummary::default();
    let mut changed = false;
    for write in batch {
        validate_write(write)?;
        let existing = lookup_event(tx, write)?;
        let event_id = match existing {
            Some(stored) => merge_event(tx, stored, write, &mut summary, &mut changed)?,
            None => {
                let id = insert_event(tx, write)?;
                summary.inserted += 1;
                changed = true;
                id
            }
        };
        for alias in &write.aliases {
            if alias.scheme.is_empty() || alias.value.is_empty() {
                bail!(
                    "别名不得为空（scheme/value）: {} / {}",
                    alias.scheme,
                    alias.value
                );
            }
            let current: Option<i64> = tx
                .query_row(
                    "SELECT event_id FROM event_aliases
                     WHERE app = ?1 AND identity_scheme = ?2 AND identity_value = ?3",
                    params![alias.app.as_str(), alias.scheme, alias.value],
                    |r| r.get(0),
                )
                .optional()?;
            match current {
                Some(id) if id == event_id => {}
                Some(id) => bail!(
                    "别名冲突：{} / {} / {} 已绑定事件 {id}，不能改绑事件 {event_id}",
                    alias.app.as_str(),
                    alias.scheme,
                    alias.value
                ),
                None => {
                    tx.execute(
                        "INSERT INTO event_aliases(event_id, app, identity_scheme,
                                                   identity_value, created_generation)
                         VALUES(?1, ?2, ?3, ?4, ?5)",
                        params![
                            event_id,
                            alias.app.as_str(),
                            alias.scheme,
                            alias.value,
                            generation as i64
                        ],
                    )?;
                    summary.aliases_added += 1;
                    changed = true;
                }
            }
        }
        for origin in &write.origins {
            if origin.origin_key.is_empty() {
                bail!("来源记录键不得为空（{}）", origin.origin_kind);
            }
            let current: Option<i64> = tx
                .query_row(
                    "SELECT event_id FROM event_origins
                     WHERE origin_kind = ?1 AND origin_key = ?2",
                    params![origin.origin_kind, origin.origin_key],
                    |r| r.get(0),
                )
                .optional()?;
            match current {
                Some(id) if id == event_id => {}
                Some(id) => bail!(
                    "来源记录冲突：{} / {} 已指向事件 {id}，不能改指事件 {event_id}",
                    origin.origin_kind,
                    origin.origin_key
                ),
                None => {
                    tx.execute(
                        "INSERT INTO event_origins(event_id, origin_kind, origin_key, app,
                            import_run_id, parser_revision, source_model_raw, pricing_model,
                            source_cost_usd, ts_precision_seconds)
                         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            event_id,
                            origin.origin_kind,
                            origin.origin_key,
                            origin.app.as_str(),
                            origin.import_run_id,
                            origin.parser_revision,
                            origin.source_model_raw,
                            origin.pricing_model,
                            origin.source_cost_usd.map(|v| v.to_string()),
                            if origin.ts_precision_seconds { 1 } else { 0 },
                        ],
                    )?;
                    summary.origins_added += 1;
                    changed = true;
                }
            }
        }
    }
    Ok((summary, changed))
}

/// 库内处置判定（纯函数，预览与写入共用）。
fn classify_against(existing: &StoredEvent, w: &EventWrite) -> WriteClass {
    if content_equal(existing, w) {
        WriteClass::Unchanged
    } else if existing.precedence > w.precedence {
        WriteClass::Conflict
    } else if w.observed_at < existing.observed_at {
        WriteClass::Stale
    } else {
        WriteClass::Update
    }
}

/// 库内同键日汇总快照的读取形态（含批次归属，供诊断）。
type RollupSnapshotRow = (
    i64,
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    Option<i64>,
);

/// 日汇总快照写入：同键内容相同 → 跳过；内容不同 → 按策略保留或替换
/// （来源无可验证修订时默认保留已存值并计冲突）。
fn write_rollups_in_tx(
    tx: &Transaction<'_>,
    rollups: &[DailyRollupWrite],
    policy: RollupConflictPolicy,
    run_id: i64,
) -> Result<(RollupSummary, bool)> {
    let mut summary = RollupSummary::default();
    let mut changed = false;
    for r in rollups {
        if r.request_count == 0 && r.tokens.total() == 0 {
            // 空快照没有统计意义，不写入（避免用 0 覆盖已有历史）。
            continue;
        }
        let existing: Option<RollupSnapshotRow> = tx
            .query_row(
                "SELECT id, request_count, input_tokens, output_tokens, cache_write_tokens,
                        cache_read_tokens, source_cost_usd, import_run_id
                 FROM ccs_daily_usage
                 WHERE logical_source = ?1 AND day = ?2 AND source_tz = ?3 AND app = ?4
                   AND provider_id = ?5 AND model = ?6 AND request_model = ?7 AND pricing_model = ?8",
                params![
                    r.logical_source,
                    r.day,
                    r.source_tz,
                    r.app.as_str(),
                    r.provider_id,
                    r.model,
                    r.request_model,
                    r.pricing_model
                ],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                        row.get(7)?,
                    ))
                },
            )
            .optional()?;
        match existing {
            None => {
                tx.execute(
                    "INSERT INTO ccs_daily_usage(logical_source, day, source_tz, app, provider_id,
                        model, request_model, pricing_model, request_count, input_tokens,
                        output_tokens, cache_write_tokens, cache_read_tokens, input_semantics,
                        source_cost_usd, import_run_id, revision)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                            ?16, ?17)",
                    params![
                        r.logical_source,
                        r.day,
                        r.source_tz,
                        r.app.as_str(),
                        r.provider_id,
                        r.model,
                        r.request_model,
                        r.pricing_model,
                        r.request_count as i64,
                        r.tokens.input.to_string(),
                        r.tokens.output.to_string(),
                        r.tokens.cache_write.to_string(),
                        r.tokens.cache_read.to_string(),
                        r.input_semantics,
                        r.source_cost_usd.map(|v| v.to_string()),
                        run_id,
                        r.revision,
                    ],
                )?;
                summary.inserted += 1;
                changed = true;
            }
            Some((id, count, i, o, cw, cr, cost, _run)) => {
                let same = count == r.request_count as i64
                    && i == r.tokens.input.to_string()
                    && o == r.tokens.output.to_string()
                    && cw == r.tokens.cache_write.to_string()
                    && cr == r.tokens.cache_read.to_string()
                    && cost == r.source_cost_usd.map(|v| v.to_string());
                if same {
                    summary.unchanged += 1;
                } else if r.revision > 0 || policy == RollupConflictPolicy::TakeSource {
                    tx.execute(
                        "UPDATE ccs_daily_usage SET request_count = ?2, input_tokens = ?3,
                            output_tokens = ?4, cache_write_tokens = ?5, cache_read_tokens = ?6,
                            input_semantics = ?7, source_cost_usd = ?8, revision = ?9
                         WHERE id = ?1",
                        params![
                            id,
                            r.request_count as i64,
                            r.tokens.input.to_string(),
                            r.tokens.output.to_string(),
                            r.tokens.cache_write.to_string(),
                            r.tokens.cache_read.to_string(),
                            r.input_semantics,
                            r.source_cost_usd.map(|v| v.to_string()),
                            r.revision,
                        ],
                    )?;
                    summary.replaced += 1;
                    changed = true;
                } else {
                    // 来源缺少可验证修订：不静默覆盖较新值，计入冲突由预览呈现。
                    summary.conflicted += 1;
                }
            }
        }
    }
    Ok((summary, changed))
}

fn insert_import_run(tx: &Transaction<'_>, run: &ImportRunRecord) -> Result<i64> {
    tx.execute(
        "INSERT INTO import_runs(logical_source, source_path, source_schema, started_utc,
            committed_utc, status, detail, import_revision)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            run.logical_source,
            run.source_path,
            run.source_schema,
            run.started_utc,
            Timestamp::now().to_string(),
            run.status,
            run.detail,
            run.import_revision,
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

fn finish_import_run(
    tx: &Transaction<'_>,
    run_id: i64,
    run: &ImportRunRecord,
    write_summary: &WriteSummary,
    rollup_summary: &RollupSummary,
) -> Result<()> {
    let net = net_new_tokens(tx, run_id)?;
    tx.execute(
        "UPDATE import_runs SET requests_inserted = ?2, requests_updated = ?3,
            requests_unchanged = ?4, requests_conflicted = ?5, requests_stale = ?6,
            rollups_snapshotted = ?7, rollups_conflicted = ?8,
            net_input_tokens = ?9, net_output_tokens = ?10,
            net_cache_write_tokens = ?11, net_cache_read_tokens = ?12
         WHERE id = ?1",
        params![
            run_id,
            write_summary.inserted as i64,
            write_summary.updated as i64,
            write_summary.unchanged as i64,
            write_summary.conflicts as i64,
            write_summary.stale as i64,
            (rollup_summary.inserted + rollup_summary.replaced) as i64,
            rollup_summary.conflicted as i64,
            net.input.to_string(),
            net.output.to_string(),
            net.cache_write.to_string(),
            net.cache_read.to_string(),
        ],
    )?;
    let _ = run;
    Ok(())
}

/// 本批次**净新增**用量：来源为该批次的事件四桶之和（已按库内合并去重）。
fn net_new_tokens(tx: &Transaction<'_>, run_id: i64) -> Result<TokenCounts> {
    net_new_tokens_conn(tx, run_id)
}

fn net_new_tokens_conn(conn: &Connection, run_id: i64) -> Result<TokenCounts> {
    let mut stmt = conn.prepare(
        "SELECT e.input_tokens, e.output_tokens, e.cache_write_tokens, e.cache_read_tokens
         FROM event_origins o JOIN usage_events e ON e.id = o.event_id
         WHERE o.import_run_id = ?1",
    )?;
    let rows = stmt.query_map(params![run_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut acc = TokenCounts::default();
    for row in rows {
        let (i, o, cw, cr) = row?;
        let counts = TokenCounts {
            input: parse_u64_text(&i, "input_tokens")?,
            output: parse_u64_text(&o, "output_tokens")?,
            cache_write: parse_u64_text(&cw, "cache_write_tokens")?,
            cache_read: parse_u64_text(&cr, "cache_read_tokens")?,
        };
        acc = acc
            .checked_add(&counts)
            .ok_or_else(|| anyhow!("导入批次净新增用量溢出可表示范围"))?;
    }
    Ok(acc)
}

/// 校验单条写入（在事务开始后、写任何行之前逐条执行；失败整批回滚）。
fn validate_write(w: &EventWrite) -> Result<()> {
    if w.event_key.is_empty() {
        bail!("事件来源键不得为空");
    }
    if w.model_raw.trim().is_empty() {
        bail!("事件模型名不得为空（来源键 {}）", w.event_key);
    }
    if w.model_identity.is_empty() {
        bail!("事件模型身份键不得为空（来源键 {}）", w.event_key);
    }
    if !w.model_identity.is_ascii() {
        // 允许非 ASCII 等价键（中文模型名），仅拒绝空白键。
        if w.model_identity.trim().is_empty() {
            bail!("事件模型身份键不得为空白（来源键 {}）", w.event_key);
        }
    }
    let total = w
        .tokens
        .input
        .checked_add(w.tokens.output)
        .and_then(|v| v.checked_add(w.tokens.cache_write))
        .and_then(|v| v.checked_add(w.tokens.cache_read));
    if total.is_none() {
        bail!("事件四桶合计超出可表示范围（来源键 {}）", w.event_key);
    }
    if !(0..=999_999_999).contains(&w.ts.subsec_nanosecond()) {
        bail!("事件时间戳纳秒越界（来源键 {}）", w.event_key);
    }
    Ok(())
}

/// 按来源键 → 来源记录证据 → 原生身份别名 三级查找已存在事件。
fn lookup_event(conn: &Connection, w: &EventWrite) -> Result<Option<StoredEvent>> {
    let by_key = conn
        .query_row(
            "SELECT id FROM usage_events WHERE event_key = ?1",
            params![w.event_key],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(id) = by_key {
        return load_event(conn, id);
    }
    for origin in &w.origins {
        let hit: Option<i64> = conn
            .query_row(
                "SELECT event_id FROM event_origins WHERE origin_kind = ?1 AND origin_key = ?2",
                params![origin.origin_kind, origin.origin_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = hit {
            return load_event(conn, id);
        }
    }
    let mut hits: Vec<i64> = Vec::new();
    for alias in &w.aliases {
        let hit: Option<i64> = conn
            .query_row(
                "SELECT event_id FROM event_aliases
                 WHERE app = ?1 AND identity_scheme = ?2 AND identity_value = ?3",
                params![alias.app.as_str(), alias.scheme, alias.value],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = hit
            && !hits.contains(&id)
        {
            hits.push(id);
        }
    }
    match hits.len() {
        0 => Ok(None),
        1 => load_event(conn, hits[0]),
        // 同一请求的两个身份各自绑定了不同事件：这是"库内已有重复事件"，
        // 合并策略需按来源优先级决策（计划 §5.3 的冲突处理），此处不猜测，
        // 明确拒绝整批，交由调用方呈现为未解决冲突。
        _ => bail!("身份冲突：同一请求的多个别名分别指向事件 {hits:?}，无法在不猜测的前提下合并"),
    }
}

fn load_event(conn: &Connection, id: i64) -> Result<Option<StoredEvent>> {
    Ok(conn
        .query_row(
            "SELECT id, event_key, app, ts_seconds, ts_nanos, model_raw, model_identity,
                    identity_revision, session_id, record_id, project_key,
                    session_initial_cwd, event_cwd, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, origin_rank, observed_at_seconds,
                    observed_at_nanos, parser_revision
             FROM usage_events WHERE id = ?1",
            params![id],
            row_to_stored_event,
        )
        .optional()?)
}

fn row_to_stored_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    let raw_app: String = r.get(2)?;
    let app = parse_agent(&raw_app).map_err(|e| conversion_error(2, e))?;
    Ok(StoredEvent {
        id: r.get(0)?,
        event_key: r.get(1)?,
        app,
        ts: timestamp_from_parts(r.get(3)?, r.get(4)?),
        model_raw: r.get(5)?,
        model_identity: r.get(6)?,
        identity_revision: r.get(7)?,
        session_id: r.get(8)?,
        record_id: r.get(9)?,
        project_key: r.get(10)?,
        session_initial_cwd: r.get(11)?,
        event_cwd: r.get(12)?,
        tokens: TokenCounts {
            input: u64_from_row(r, 13)?,
            output: u64_from_row(r, 14)?,
            cache_write: u64_from_row(r, 15)?,
            cache_read: u64_from_row(r, 16)?,
        },
        precedence: WritePrecedence::from_rank(r.get(17)?),
        observed_at: timestamp_from_parts(r.get(18)?, r.get(19)?),
        parser_revision: r.get(20)?,
    })
}

/// token 列读取：TEXT → u64，非法值明确报错（不钳 0、不静默吞掉）。
fn u64_from_row(r: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let raw: String = r.get(index)?;
    parse_u64_text(&raw, "token 计数").map_err(|e| conversion_error(index, e))
}

/// 列值转换失败：把诊断文本原样带出（不丢"这一行坏在哪"的信息）。
fn conversion_error(index: usize, e: anyhow::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            e.to_string(),
        )),
    )
}

fn timestamp_from_parts(seconds: i64, nanos: i64) -> Timestamp {
    let n = nanos.clamp(0, 999_999_999) as i32;
    Timestamp::new(seconds, n).unwrap_or(Timestamp::UNIX_EPOCH)
}

fn parse_u64_text(raw: &str, what: &str) -> Result<u64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.starts_with('+') {
        bail!("{what} 非法（期望十进制无符号整数，得到 {raw:?}）");
    }
    trimmed
        .parse::<u64>()
        .map_err(|e| anyhow!("{what} 非法（{raw:?}: {e}）"))
}

fn parse_agent(raw: &str) -> Result<AgentKind> {
    AgentKind::parse(raw).map_err(|e| anyhow!("{e}"))
}

/// 合并判定：内容完全相同 → 跳过；低优先级不覆盖高优先级；更旧的观察不覆盖
/// 较新的终值；其余按本次观察更新该事件。判定与
/// [`HistoryDb::classify_write`] 共用同一纯函数，预览数字因此与提交结果一致。
fn merge_event(
    tx: &Transaction<'_>,
    existing: StoredEvent,
    w: &EventWrite,
    summary: &mut WriteSummary,
    changed: &mut bool,
) -> Result<i64> {
    match classify_against(&existing, w) {
        WriteClass::Unchanged => {
            summary.unchanged += 1;
            return Ok(existing.id);
        }
        WriteClass::Conflict => {
            summary.conflicts += 1;
            log::debug!(
                "历史库冲突：来源 {} 与已存更高优先级终值冲突，保留已存值",
                w.event_key
            );
            return Ok(existing.id);
        }
        WriteClass::Stale => {
            summary.stale += 1;
            log::debug!(
                "历史库陈旧写入：{} 的观察时间早于已存终值，保留较新值",
                w.event_key
            );
            return Ok(existing.id);
        }
        WriteClass::Insert | WriteClass::Update => {}
    }
    tx.execute(
        "UPDATE usage_events SET
            app = ?2, ts_seconds = ?3, ts_nanos = ?4, model_raw = ?5, model_identity = ?6,
            identity_revision = ?7, session_id = ?8, record_id = ?9, project_key = ?10,
            session_initial_cwd = ?11, event_cwd = ?12, input_tokens = ?13, output_tokens = ?14,
            cache_write_tokens = ?15, cache_read_tokens = ?16, origin_rank = ?17,
            observed_at_utc = ?18, observed_at_seconds = ?19, observed_at_nanos = ?20,
            parser_revision = ?21
         WHERE id = ?1",
        params![
            existing.id,
            w.app.as_str(),
            w.ts.as_second(),
            w.ts.subsec_nanosecond() as i64,
            w.model_raw,
            w.model_identity,
            IDENTITY_REVISION,
            w.session_id,
            w.record_id,
            w.project_key,
            w.session_initial_cwd,
            w.event_cwd,
            w.tokens.input.to_string(),
            w.tokens.output.to_string(),
            w.tokens.cache_write.to_string(),
            w.tokens.cache_read.to_string(),
            w.precedence.rank(),
            w.observed_at.to_string(),
            w.observed_at.as_second(),
            w.observed_at.subsec_nanosecond() as i64,
            w.parser_revision(),
        ],
    )?;
    summary.updated += 1;
    *changed = true;
    Ok(existing.id)
}

/// 影响统计与展示的字段是否完全相同（`observed_at` 是元数据，不参与）。
fn content_equal(existing: &StoredEvent, w: &EventWrite) -> bool {
    existing.app == w.app
        && existing.ts == w.ts
        && existing.model_raw == w.model_raw
        && existing.model_identity == w.model_identity
        && existing.session_id == w.session_id
        && existing.record_id == w.record_id
        && existing.project_key == w.project_key
        && existing.session_initial_cwd == w.session_initial_cwd
        && existing.event_cwd == w.event_cwd
        && existing.tokens == w.tokens
}

fn insert_event(tx: &Transaction<'_>, w: &EventWrite) -> Result<i64> {
    tx.execute(
        "INSERT INTO usage_events(event_key, app, ts_seconds, ts_nanos, model_raw,
            model_identity, identity_revision, session_id, record_id, project_key,
            session_initial_cwd, event_cwd, input_tokens, output_tokens, cache_write_tokens,
            cache_read_tokens, origin_rank, observed_at_utc, observed_at_seconds,
            observed_at_nanos, parser_revision)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                ?17, ?18, ?19, ?20, ?21)",
        params![
            w.event_key,
            w.app.as_str(),
            w.ts.as_second(),
            w.ts.subsec_nanosecond() as i64,
            w.model_raw,
            w.model_identity,
            IDENTITY_REVISION,
            w.session_id,
            w.record_id,
            w.project_key,
            w.session_initial_cwd,
            w.event_cwd,
            w.tokens.input.to_string(),
            w.tokens.output.to_string(),
            w.tokens.cache_write.to_string(),
            w.tokens.cache_read.to_string(),
            w.precedence.rank(),
            w.observed_at.to_string(),
            w.observed_at.as_second(),
            w.observed_at.subsec_nanosecond() as i64,
            w.parser_revision(),
        ],
    )?;
    Ok(tx.last_insert_rowid())
}

impl EventWrite {
    /// 事件解析版本：调用方可用 `parser_revision` 字段覆盖的默认值。
    fn parser_revision(&self) -> i64 {
        PARSER_REVISION
    }
}

fn bump_generation(tx: &Transaction<'_>) -> Result<()> {
    let current: Option<String> = tx
        .query_row(
            "SELECT value FROM history_meta WHERE key = ?1",
            params![META_GENERATION],
            |r| r.get(0),
        )
        .optional()?;
    let next = match current {
        Some(v) => parse_u64_text(&v, META_GENERATION)?
            .checked_add(1)
            .ok_or_else(|| anyhow!("历史库 generation 溢出（超出 u64 可表示范围）"))?,
        None => 1,
    };
    tx.execute(
        "INSERT INTO history_meta(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![META_GENERATION, next.to_string()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_text_parsing_rejects_bad_values() {
        assert_eq!(parse_u64_text("0", "t").unwrap(), 0);
        assert_eq!(
            parse_u64_text("18446744073709551615", "t").unwrap(),
            u64::MAX
        );
        for bad in ["", " ", "-1", "+1", "1.0", "1e3", "abc", "18,446"] {
            assert!(parse_u64_text(bad, "t").is_err(), "{bad:?} 必须被拒绝");
        }
    }

    #[test]
    fn timestamp_parts_roundtrip() {
        let t: Timestamp = "2026-07-17T08:00:00.123456789Z".parse().unwrap();
        let rebuilt = timestamp_from_parts(t.as_second(), t.subsec_nanosecond() as i64);
        assert_eq!(rebuilt, t);
    }

    #[test]
    fn precedence_rank_roundtrip() {
        for p in [WritePrecedence::CcsImport, WritePrecedence::NativeLog] {
            assert_eq!(WritePrecedence::from_rank(p.rank()), p);
        }
        assert_eq!(WritePrecedence::from_rank(7), WritePrecedence::NativeLog);
    }
}
