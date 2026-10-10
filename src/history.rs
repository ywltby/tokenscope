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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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
        let mut summary = WriteSummary::default();
        let mut changed = false;
        for write in batch {
            validate_write(write)?;
            let existing = lookup_event(&tx, write)?;
            let event_id = match existing {
                Some(stored) => merge_event(&tx, stored, write, &mut summary, &mut changed)?,
                None => {
                    let id = insert_event(&tx, write)?;
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
                                self.generation()? as i64
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
        if changed {
            bump_generation(&tx)?;
        }
        tx.commit()?;
        Ok(summary)
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

    fn count(&self, table: &str) -> Result<u64> {
        let n: i64 = self
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        Ok(n.max(0) as u64)
    }

    /// 全部事件（按 `(ts_seconds, ts_nanos, id)` 稳定排序）。
    pub fn stored_events(&self) -> Result<Vec<StoredEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, event_key, app, ts_seconds, ts_nanos, model_raw, model_identity,
                    identity_revision, session_id, record_id, project_key,
                    session_initial_cwd, event_cwd, input_tokens, output_tokens,
                    cache_write_tokens, cache_read_tokens, origin_rank, observed_at_seconds,
                    observed_at_nanos, parser_revision
             FROM usage_events
             ORDER BY ts_seconds, ts_nanos, id",
        )?;
        let rows = stmt.query_map([], row_to_stored_event)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
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
                                      observations)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'present', ?8, ?9, 1)
             ON CONFLICT(app, root, path) DO UPDATE SET
                fingerprint = excluded.fingerprint,
                size_bytes = excluded.size_bytes,
                mtime_ms = excluded.mtime_ms,
                context_revision = excluded.context_revision,
                state = 'present',
                last_seen_utc = excluded.last_seen_utc,
                last_success_utc = COALESCE(excluded.last_success_utc, source_files.last_success_utc),
                observations = source_files.observations + 1",
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
                    state, last_seen_utc, last_success_utc, observations
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
fn lookup_event(tx: &Transaction<'_>, w: &EventWrite) -> Result<Option<StoredEvent>> {
    let by_key = tx
        .query_row(
            "SELECT id FROM usage_events WHERE event_key = ?1",
            params![w.event_key],
            |r| r.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(id) = by_key {
        return load_event(tx, id);
    }
    for origin in &w.origins {
        let hit: Option<i64> = tx
            .query_row(
                "SELECT event_id FROM event_origins WHERE origin_kind = ?1 AND origin_key = ?2",
                params![origin.origin_kind, origin.origin_key],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = hit {
            return load_event(tx, id);
        }
    }
    let mut hits: Vec<i64> = Vec::new();
    for alias in &w.aliases {
        let hit: Option<i64> = tx
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
        1 => load_event(tx, hits[0]),
        // 同一请求的两个身份各自绑定了不同事件：这是"库内已有重复事件"，
        // 合并策略需按来源优先级决策（计划 §5.3 的冲突处理），此处不猜测，
        // 明确拒绝整批，交由调用方呈现为未解决冲突。
        _ => bail!("身份冲突：同一请求的多个别名分别指向事件 {hits:?}，无法在不猜测的前提下合并"),
    }
}

fn load_event(tx: &Transaction<'_>, id: i64) -> Result<Option<StoredEvent>> {
    Ok(tx
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
/// 较新的终值；其余按本次观察更新该事件。
fn merge_event(
    tx: &Transaction<'_>,
    existing: StoredEvent,
    w: &EventWrite,
    summary: &mut WriteSummary,
    changed: &mut bool,
) -> Result<i64> {
    if content_equal(&existing, w) {
        summary.unchanged += 1;
        return Ok(existing.id);
    }
    if existing.precedence > w.precedence {
        summary.conflicts += 1;
        log::debug!(
            "历史库冲突：来源 {} 与已存原生终值冲突，保留原生值",
            w.event_key
        );
        return Ok(existing.id);
    }
    if w.observed_at < existing.observed_at {
        summary.stale += 1;
        log::debug!(
            "历史库陈旧写入：{} 的观察时间早于已存终值，保留较新值",
            w.event_key
        );
        return Ok(existing.id);
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
